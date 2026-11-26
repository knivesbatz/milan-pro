//! The FX window (§5.15.2, decision 0.5): one modeless window for Video Event, Track and Output FX, Audio Event, Track
//! and Master FX, generated media and transitions. The chain bar (bypass box, select, drag to reorder, fx+ and fx×), the preset row (built-in and user
//! presets, save and delete), the parameter grid from the plug-in's `ParamSpec`s and, while Animate is on, the keyframe
//! controller with one lane for the selected plug-in. Each gesture is one undo step.
use eframe::egui::*;
use crate::ui::widgets;
use std::collections::HashSet;
use crate::edit::{self, FxTarget};
use crate::engine::aplug;
use crate::engine::ofx;
use crate::engine::plugins::{self, FxEval, ParamKind, PluginInfo, PluginKind, UserPreset, Val};
use crate::history::History;
use crate::project::{AutoMode, Curve, Fx, Keyframe, Keyframes, Project};
use super::dialogs;
use super::icons::Icon;
use super::keyframes::{self, KfResponse, KfState};
use super::toolbar::tool_button;
use super::workspace;

pub const ID: &str = "fx_window";
const SIZE: Vec2 = vec2(600.0, 500.0);
const LABEL_W: f32 = 150.0;

/// A chain-bar click: bypass, remove, reorder or add.
enum ChainOp {
    Enable(usize, bool),
    Remove(usize),
    Move(usize, usize),
    Add(Fx),
}

pub struct FxWindow {
    pub target: FxTarget,
    /// The selected plug-in in the chain.
    sel: usize,
    /// A gesture has its undo step already (`workspace::commit`).
    gesture: bool,
    /// Plug-in instances (by id) whose edits are keyed at the cursor.
    animate: HashSet<String>,
    kf: KfState,
    /// The preset last chosen or saved, (user preset, name): shown while the values still match it.
    preset: Option<(bool, String)>,
    /// The name being typed for Save Preset.
    save_as: Option<String>,
    /// The chain pill being dragged.
    drag: Option<usize>,
    /// Installed fonts, read the first time a Font parameter shows.
    fonts: Option<Vec<String>>,
    /// R33a: OFX groups (FX id, group) the user opened or closed against the plug-in's default.
    toggled: HashSet<(String, String)>,
    /// R34b: a plug-in editor opened from this window is made off-screen and inactive (the screenshot harness, tests).
    pub offscreen: bool,
    /// When an open plug-in editor was last polled for its edits.
    polled: f64,
}

/// A parameter's name cell: "Name *" when off its default, a hint on hover; true on a double-click (reset).
fn name_cell(ui: &mut Ui, name: &str, changed: bool, depth: usize, hint: &str) -> bool {
    let text = format!("{}{}", name, if changed { " *" } else { "" });
    let w = (LABEL_W - depth as f32 * 12.0).max(40.0);
    let lr = ui.add_sized([w, 18.0], Label::new(text).sense(Sense::click()).truncate());
    let lr = if hint.is_empty() { lr.on_hover_text("Double-click to reset") } else { lr.on_hover_text(format!("{hint}\nDouble-click to reset")) };
    lr.double_clicked()
}

/// A choice whose options the plug-in set at run time (R33a: `ofx::Row::Param::options`).
fn live_choice(ui: &mut Ui, fx: &mut Fx, key: &str, options: &[String], cur: f32, key_at: Option<f64>, fps: f64) {
    let k = (cur.round().max(0.0) as usize).min(options.len().saturating_sub(1));
    widgets::combo(ui, ("fx_choice", key), 180.0, options.get(k).map_or("", String::as_str), |ui| {
        for (j, o) in options.iter().enumerate() {
            if ui.selectable_label(j == k, o).clicked() {
                set_num(fx, key, j as f32, cur, key_at, fps);
            }
        }
    });
}

/// The name a chain shows for plug-in `id`: its name, or (an external plug-in that does not run) what is known of it.
fn chain_name(id: &str) -> String {
    plugins::find(id).map(|i| i.name.to_string()).or_else(|| ofx::label(id)).or_else(|| aplug::label(id)).unwrap_or_else(|| id.to_string())
}

/// Some parameter is keyed over time (a key off 0, or several).
pub fn animated(fx: &Fx) -> bool {
    fx.params.values().any(|k| k.keys.len() > 1 || k.keys.iter().any(|k| k.t != 0.0))
}

/// Give slider rails, check boxes and fields a visible fill and edge in the FX window.
pub fn field_style(ui: &mut Ui) {
    let t = super::theme::get();
    let w = &mut ui.visuals_mut().widgets;
    (w.inactive.bg_fill, w.inactive.weak_bg_fill) = (t.input_bg, t.input_bg);
    w.inactive.bg_stroke = Stroke::new(1.0_f32, t.text_dim.gamma_multiply(0.5));
}

/// Where `target`'s keys are at the timeline cursor, the span its keyframe lane shows, and where key time 0 is on the
/// timeline (event FX keys are event-relative). None for targets without keys (generated media, transitions).
pub fn key_time(p: &Project, target: &FxTarget) -> Option<(f64, f64, f64)> {
    let cur = p.timeline.cursor_position;
    match target {
        FxTarget::Event(id) => p.clip(id).map(|c| ((cur - c.position).clamp(0.0, c.duration), c.duration, c.position)),
        FxTarget::Track(_) | FxTarget::VideoOutput | FxTarget::AudioMaster | FxTarget::Compositor(_) | FxTarget::Bus(_) => Some((cur.max(0.0), crate::render::content_end(p).max(cur).max(1.0), 0.0)),
        FxTarget::Media(_) | FxTarget::Transition(_) => None,
    }
}

/// Set number `key` of `fx` to `v`. With `at` (Animate) it is keyed there, the old value `cur` kept at 0 by a first key;
/// without, it becomes one key at 0.
pub fn set_num(fx: &mut Fx, key: &str, v: f32, cur: f32, at: Option<f64>, fps: f64) {
    let kf = fx.params.entry(key.to_string()).or_default();
    match at {
        Some(t) => {
            if kf.keys.is_empty() {
                kf.keys.push(Keyframe { t: 0.0, v: cur, curve: Curve::Smooth });
            }
            keyframes::set_at(kf, t, v, fps);
        }
        None => *kf = Keyframes { keys: vec![Keyframe { t: 0.0, v, curve: Curve::Linear }] },
    }
}

/// Every number of `e` with its stored key (a colour as its four components).
fn numbers(info: &PluginInfo, e: &FxEval) -> Vec<(String, f32)> {
    let mut v = Vec::new();
    for s in info.params {
        match s.kind {
            ParamKind::Float { .. } | ParamKind::Choice { .. } | ParamKind::Bool { .. } => v.push((s.key.to_string(), e.f(s.key))),
            ParamKind::Color { .. } => v.extend(["r", "g", "b", "a"].iter().zip(e.c(s.key)).map(|(c, x)| (format!("{}.{c}", s.key), x))),
            ParamKind::Text { .. } | ParamKind::Font { .. } => {}
        }
    }
    v
}

/// The plug-in's one keyframe lane: every key time of its parameters (keys within half a frame are one), with the
/// curve of the first key there.
pub fn lane(fx: &Fx, fps: f64) -> Vec<(f64, Curve)> {
    let tol = 0.5 / fps.max(1.0);
    let mut v: Vec<(f64, Curve)> = fx.params.values().flat_map(|k| k.keys.iter().map(|k| (k.t, k.curve))).collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    v.dedup_by(|later, kept| (later.0 - kept.0).abs() < tol);
    v
}

/// Carry out keyframe-controller intent `r` on every parameter of `fx`, the cursor at `at`; Some(t) asks for a seek.
pub fn apply_lane(fx: &mut Fx, info: &PluginInfo, r: KfResponse, at: f64, fps: f64) -> Option<f64> {
    let tol = 0.5 / fps.max(1.0);
    let lane_t = |fx: &Fx, i: usize| lane(fx, fps).get(i).map(|k| k.0);
    let keys_at = |fx: &mut Fx, t: f64, f: &dyn Fn(&mut Keyframe<f32>)| fx.params.values_mut().flat_map(|k| k.keys.iter_mut()).filter(|k| (k.t - t).abs() < tol).for_each(f);
    match r {
        KfResponse::Seek(t) => return Some(t),
        KfResponse::Add => {
            let e = plugins::eval(fx, at)?;
            for (key, v) in numbers(info, &e) {
                set_num(fx, &key, v, v, Some(at), fps);
            }
        }
        // The first key holds the start value: it stays.
        KfResponse::Delete(i) if i > 0 => {
            if let Some(t) = lane_t(fx, i) {
                fx.params.values_mut().for_each(|k| k.keys.retain(|k| (k.t - t).abs() >= tol));
            }
        }
        KfResponse::Move(i, to) if i > 0 => {
            if let Some(t) = lane_t(fx, i) {
                keys_at(fx, t, &|k| k.t = to);
            }
        }
        KfResponse::SetCurve(i, c) => {
            if let Some(t) = lane_t(fx, i) {
                keys_at(fx, t, &|k| k.curve = c);
            }
        }
        _ => {}
    }
    None
}

/// The preset `fx` is set to, (user preset, name): `last` (the one last picked) while the values still match it, else
/// the first built-in or user preset with the same values. None once edited away from every preset, or keyed.
fn shown_preset(info: &PluginInfo, users: &[UserPreset], fx: &Fx, last: Option<&(bool, String)>) -> Option<(bool, String)> {
    if animated(fx) {
        return None;
    }
    let now = plugins::eval(fx, 0.0);
    let builtin = info.presets.iter().map(|p| ((false, p.name.to_string()), plugins::new_fx(info, Some(p))));
    let mine = users.iter().filter(|u| u.plugin == info.id).map(|u| ((true, u.name.clone()), plugins::new_user_fx(info, u)));
    let same: Vec<(bool, String)> = builtin.chain(mine).filter(|(_, o)| plugins::eval(o, 0.0) == now).map(|(k, _)| k).collect();
    last.filter(|l| same.contains(l)).cloned().or_else(|| same.into_iter().next())
}

/// Remove parameter `key`'s stored value(s): it is back at its default.
fn reset(fx: &mut Fx, key: &str, kind: ParamKind) {
    match kind {
        ParamKind::Color { .. } => {
            for c in ["r", "g", "b", "a"] {
                fx.params.remove(&format!("{key}.{c}"));
            }
        }
        ParamKind::Text { .. } | ParamKind::Font { .. } => {
            fx.text.remove(key);
        }
        _ => {
            fx.params.remove(key);
        }
    }
}

impl FxWindow {
    /// For `target`; None when it is gone (or holds no generator / transition).
    pub fn open(project: &Project, target: FxTarget) -> Option<Self> {
        let (list, _) = edit::fx_list(project, &target)?;
        let animate = list.iter().filter(|f| animated(f)).map(|f| f.id.clone()).collect();
        Some(Self { target, sel: 0, gesture: false, animate, kf: KfState::default(), preset: None, save_as: None, drag: None, fonts: None, toggled: HashSet::new(), offscreen: false, polled: 0.0 })
    }

    /// Select the plug-in last in the chain (just dropped on it).
    pub fn select_last(mut self, project: &Project) -> Self {
        self.sel = edit::fx_list(project, &self.target).map_or(0, |l| l.0.len().saturating_sub(1));
        self
    }

    fn title(&self, p: &Project) -> String {
        let clip = |id: &str| p.clip(id).map_or(String::new(), |c| c.name.clone());
        let kind = if edit::fx_is_audio(p, &self.target) { "Audio" } else { "Video" };
        match &self.target {
            FxTarget::Event(id) => format!("{kind} Event FX: {}", clip(id)),
            FxTarget::Track(id) => format!("{kind} Track FX: Track {}", p.find_track(id).map_or(0, |t| t.1 + 1)),
            FxTarget::VideoOutput => "Video Output FX".into(),
            FxTarget::AudioMaster => "Audio Master FX".into(),
            FxTarget::Media(id) => format!("Video Media Generators: {}", p.media_pool.iter().find(|m| m.id == *id).map_or("", |m| m.name.as_str())),
            FxTarget::Transition(id) => format!("Transition: {}", clip(edit::split_owner(id).0)),
            FxTarget::Compositor(id) => format!("Compositor: Track {}", p.find_track(id).map_or(0, |t| t.1 + 1)),
            FxTarget::Bus(id) => {
                let fx = p.timeline.audio.fx.iter().any(|b| b.id == *id);
                format!("{}: {}", if fx { "Audio Assignable FX" } else { "Audio Bus FX" }, p.timeline.audio.bus(id).map_or("", |b| b.name.as_str()))
            }
        }
    }

    /// One undo step per gesture: `list` replaces the target's plug-ins; `active` while a pointer is down or a field
    /// is being typed in.
    fn commit(&mut self, project: &mut Project, history: &mut History, list: Vec<Fx>, active: bool) {
        let changed = edit::fx_list(project, &self.target).is_some_and(|(l, editable)| editable && l != list);
        let target = &self.target;
        workspace::commit(project, history, &mut self.gesture, active, changed, |p| {
            edit::set_fx_list(p, target, list);
        });
    }

    /// Draw the window and apply its edits. False once it is closed (× or the target is gone); a status message.
    pub fn show(&mut self, ctx: &Context, project: &mut Project, history: &mut History, users: &mut Vec<UserPreset>) -> (bool, Option<String>) {
        let Some((orig, editable)) = edit::fx_list(project, &self.target) else { return (false, None) };
        let chain = matches!(self.target, FxTarget::Event(_) | FxTarget::Track(_) | FxTarget::VideoOutput | FxTarget::AudioMaster | FxTarget::Bus(_));
        let audio = edit::fx_is_audio(project, &self.target);
        let kind = if audio { PluginKind::AudioFx } else { PluginKind::VideoFx };
        let fps = project.settings.frame_rate;
        let (size, par) = ((project.settings.width, project.settings.height), project.settings.par);
        let keyed = key_time(project, &self.target);
        let (at, span, base) = keyed.map_or((0.0, 0.0, 0.0), |(_, span, base)| (self.kf.cursor(project.timeline.cursor_position - base).clamp(0.0, span), span, base));
        // Event FX keys stop at the event's end; track and output keys may go past the last event.
        let end = if matches!(self.target, FxTarget::Event(_)) { span } else { f64::INFINITY };
        let mut list = orig.clone();
        self.sel = self.sel.min(list.len().saturating_sub(1));
        // R8c: an audio track's or bus's automated parameters follow its Automation Settings.
        let parent = match &self.target {
            FxTarget::Track(id) | FxTarget::Bus(id) if audio => Some(id.clone()),
            _ => None,
        };
        let mode = parent.as_deref().and_then(|id| edit::automation_of(project, id)).map(|m| m.0);
        // (plug-in id, parameter, Some(new value 0..=1) or None: held) for the automation writes after the window.
        let mut auto_writes: Vec<(String, String, Option<f32>)> = Vec::new();
        let title = self.title(project);
        let label = title.split(':').next().unwrap_or_default().to_string();
        let (mut msg, mut seek, mut op) = (None, None, None);

        let (_, close) = dialogs::tool_window(ctx, ID, &title, SIZE, |ui| {
            let full = ui.available_rect_before_wrap();
            // A Video FX tile dropped anywhere on the window joins the chain.
            if let Some(d) = super::plugin_dropped(&ui.interact(full, ui.id().with("fx_drop"), Sense::hover())) {
                if chain && editable && d.kind == kind {
                    (op, self.sel) = (Some(ChainOp::Add(d.fx.clone())), list.len());
                } else {
                    msg = Some(match (chain, audio, editable) {
                        (false, ..) => "Only a plug-in chain takes video FX.",
                        (true, true, _) => "Audio plug-ins are added with fx+.",
                        (true, false, false) => "The target is locked.",
                        (true, false, true) => "Only video FX go in a video chain.",
                    }.to_string());
                }
            }
            let animating = keyed.is_some() && chain && list.get(self.sel).is_some_and(|f| self.animate.contains(&f.id));
            let kf_r = full.with_min_y(full.max.y - keyframes::HEIGHT);
            let top = if animating { full.with_max_y(kf_r.min.y - 5.0) } else { full };
            let mut ui_top = ui.new_child(UiBuilder::new().max_rect(top).layout(Layout::top_down(Align::Min)));
            let ui = &mut ui_top;
            field_style(ui);

            // ---- Chain bar ----
            ui.horizontal(|ui| {
                ui.label(RichText::new(&label).strong());
                ui.add_space(6.0);
                let mut rects = Vec::new();
                for (i, f) in list.iter().enumerate() {
                    if chain {
                        let mut on = f.enabled;
                        if ui.add_enabled(editable, Checkbox::without_text(&mut on)).on_hover_text("Bypass").changed() {
                            op = Some(ChainOp::Enable(i, on));
                        }
                    }
                    let name = chain_name(&f.plugin);
                    let sense = if chain && editable { Sense::click_and_drag() } else { Sense::click() };
                    let r = ui.add(Button::new(name).selected(i == self.sel).sense(sense));
                    if r.clicked() {
                        self.sel = i;
                    }
                    if r.drag_started() {
                        self.drag = Some(i);
                    }
                    rects.push(r.rect);
                }
                // A released pill goes between the pills either side of the pointer.
                if let Some(from) = self.drag.filter(|_| !ui.input(|i| i.pointer.any_down())) {
                    if let Some(x) = ui.input(|i| i.pointer.latest_pos()).map(|p| p.x) {
                        let to = rects.iter().enumerate().filter(|(k, r)| *k != from && r.center().x < x).count();
                        (op, self.sel) = (Some(ChainOp::Move(from, to)), to);
                    }
                    self.drag = None;
                }
                if chain {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::push_button_enabled(ui, editable && !list.is_empty(), "fx×").on_hover_text("Remove Selected Plug-in").clicked() {
                            op = Some(ChainOp::Remove(self.sel));
                            self.sel = self.sel.saturating_sub(1);
                        }
                        ui.add_enabled_ui(editable, |ui| {
                            // shortcut: a menu of plug-ins and their presets, without the chooser's thumbnails; reuse
                            // the chooser widget here if users miss them.
                            ui.menu_button("fx+", |ui| {
                                // A plug-in's presets as its submenu; a click adds it.
                                let mut add: Option<Fx> = None;
                                let mut item = |ui: &mut Ui, info: &'static PluginInfo| {
                                    ui.menu_button(info.name, |ui| {
                                        let builtin = info.presets.iter().map(|p| (p.name.to_string(), plugins::new_fx(info, Some(p))));
                                        let mine = users.iter().filter(|u| u.plugin == info.id).map(|u| (u.name.clone(), plugins::new_user_fx(info, u)));
                                        for (name, fx) in builtin.chain(mine) {
                                            if ui.button(name).clicked() {
                                                add = Some(fx);
                                                ui.close_menu();
                                            }
                                        }
                                    });
                                };
                                let all: Vec<&'static PluginInfo> = plugins::catalog(kind).collect();
                                for info in all.iter().filter(|i| !aplug::is_ext(i.id)) {
                                    item(ui, info);
                                }
                                // R34a: VST3 and CLAP effects under their vendor.
                                let mut vendors: Vec<&str> = all.iter().filter(|i| aplug::is_ext(i.id)).map(|i| i.group).collect();
                                vendors.dedup();
                                if !vendors.is_empty() {
                                    ui.separator();
                                }
                                for vendor in vendors {
                                    ui.menu_button(vendor, |ui| {
                                        for info in all.iter().filter(|i| aplug::is_ext(i.id) && i.group == vendor) {
                                            item(ui, info);
                                        }
                                    });
                                }
                                if let Some(fx) = add {
                                    (op, self.sel) = (Some(ChainOp::Add(fx)), list.len());
                                }
                            })
                            .response
                            .on_hover_text("Plug-in Chain...");
                        });
                    });
                }
            });
            ui.separator();

            // The controller's lanes for the rest of the chain: (chain index, name, key times).
            let others: Vec<(usize, String, Vec<f64>)> = list
                .iter()
                .enumerate()
                .filter(|&(i, _)| i != self.sel)
                .map(|(i, f)| (i, chain_name(&f.plugin), lane(f, fps).iter().map(|k| k.0).collect()))
                .collect();
            let Some(fx) = list.get_mut(self.sel) else {
                ui.label(if audio { "No plug-ins. Use fx+ to add one." } else { "No plug-ins. Drag one here from the Video FX window, or use fx+." });
                return;
            };
            // R33a/R34a: an external plug-in that is missing, turned off or failed says why it is bypassed; its settings stay.
            let why = ofx::bypassed_because(&fx.plugin).or_else(|| aplug::bypassed_because(&fx.plugin));
            let Some(info) = plugins::find(&fx.plugin) else {
                let why = why.unwrap_or_else(|| format!("\"{}\" is not available in this version of Milan Pro. It is kept, and bypassed.", fx.plugin));
                ui.add(Label::new(RichText::new(why).color(super::theme::get().warning_text)).wrap());
                return;
            };
            if let Some(why) = why {
                ui.add(Label::new(RichText::new(why).color(super::theme::get().warning_text)).wrap());
            }

            // ---- Preset row ----
            ui.horizontal(|ui| {
                ui.label("Preset:");
                let mut pick: Option<Fx> = None;
                let shown = shown_preset(info, users, fx, self.preset.as_ref());
                widgets::combo(ui, "fx_preset", 220.0, shown.as_ref().map_or("", |p| p.1.as_str()), |ui| {
                    for p in info.presets {
                        if ui.selectable_label(shown.as_ref().is_some_and(|s| !s.0 && s.1 == p.name), p.name).clicked() {
                            (pick, self.preset) = (Some(plugins::new_fx(info, Some(p))), Some((false, p.name.to_string())));
                        }
                    }
                    let mine: Vec<&UserPreset> = users.iter().filter(|u| u.plugin == info.id).collect();
                    if !mine.is_empty() {
                        ui.separator();
                    }
                    for u in mine {
                        if ui.selectable_label(shown.as_ref().is_some_and(|s| s.0 && s.1 == u.name), &u.name).clicked() {
                            (pick, self.preset) = (Some(plugins::new_user_fx(info, u)), Some((true, u.name.clone())));
                        }
                    }
                });
                if let Some(p) = pick.filter(|_| editable) {
                    // A preset sets the values; the plug-in's FX envelopes stay.
                    *fx = Fx { id: fx.id.clone(), enabled: fx.enabled, auto: std::mem::take(&mut fx.auto), ..p };
                }
                match &mut self.save_as {
                    Some(name) => {
                        widgets::edit(ui, TextEdit::singleline(name).desired_width(120.0).hint_text("Preset name"));
                        if widgets::push_button_enabled(ui, !name.trim().is_empty(), "Save").clicked() {
                            let name = name.trim().to_string();
                            match plugins::save_user_preset(plugins::user_preset_of(fx, &name, at)) {
                                Ok(()) => {
                                    *users = plugins::load_user_presets();
                                    self.preset = Some((true, name));
                                }
                                Err(e) => msg = Some(format!("Could not save the preset: {e}")),
                            }
                            self.save_as = None;
                        } else if widgets::push_button(ui, "Cancel").clicked() {
                            self.save_as = None;
                        }
                    }
                    None => {
                        if tool_button(ui, Icon::Save, "Save Preset", true, false).clicked() {
                            self.save_as = Some(shown.as_ref().map_or(String::new(), |p| p.1.clone()));
                        }
                    }
                }
                let user = shown.filter(|p| p.0).map(|p| p.1);
                if tool_button(ui, Icon::Delete, "Delete Preset", user.is_some(), false).clicked() {
                    if let Some(name) = user {
                        match plugins::delete_user_preset(info.id, &name) {
                            Ok(()) => (*users, self.preset) = (plugins::load_user_presets(), None),
                            Err(e) => msg = Some(format!("Could not delete the preset: {e}")),
                        }
                    }
                }
                if let Some(id) = parent.as_deref() {
                    if widgets::push_button_enabled(ui, editable, "Automation...").on_hover_text("FX Automation Envelopes...").clicked() {
                        super::automation::open_chooser(ui.ctx(), id);
                    }
                }
                if keyed.is_some() && chain {
                    let mut on = self.animate.contains(&fx.id);
                    if widgets::checkbox_enabled(ui, editable, &mut on, "Animate").changed() {
                        if on {
                            self.animate.insert(fx.id.clone());
                        } else {
                            // Off: every parameter keeps its value at the cursor, unkeyed.
                            self.animate.remove(&fx.id);
                            for k in fx.params.values_mut().filter(|k| !k.keys.is_empty()) {
                                *k = Keyframes { keys: vec![Keyframe { t: 0.0, v: k.at(at), curve: Curve::Linear }] };
                            }
                        }
                    }
                }
            });
            // R34b: a VST3 effect's own editor in a window of the plug-in host, and `.vstpreset` files, on a row of their own.
            if aplug::find(&fx.plugin).is_some() {
                ui.horizontal(|ui| {
                    if let Some(p) = aplug::find(&fx.plugin) {
                        let open = aplug::editor_state(&fx.id).is_some();
                        if p.desc.has_editor {
                            let label = if open { "Hide plug-in UI" } else { "Show plug-in UI" };
                            if widgets::push_button(ui, label).on_hover_text("The plug-in's own window; its changes are written to the project (as automation while recording)").clicked() {
                                let op = if open {
                                    aplug::EditorOp::Close
                                } else {
                                    let at = ui.ctx().input(|i| i.viewport().outer_rect.map(|r| (r.min + vec2(240.0, 160.0)) * i.pixels_per_point)).unwrap_or(pos2(120.0, 120.0));
                                    aplug::EditorOp::Open { x: at.x as i32, y: at.y as i32, offscreen: self.offscreen, title: format!("{} - Milan Pro", info.name) }
                                };
                                if let Err(e) = aplug::editor(fx, op) {
                                    msg = Some(format!("Could not open the plug-in's window: {e}"));
                                }
                            }
                            if open {
                                // Its edits come with each poll, ten times a second.
                                let now = ui.ctx().input(|i| i.time);
                                if now - self.polled > 0.1 {
                                    self.polled = now;
                                    let _ = aplug::editor(fx, aplug::EditorOp::Poll);
                                }
                                ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                            }
                        }
                        if widgets::push_button_enabled(ui, editable, "Load .vstpreset...").clicked() {
                            if let Some(f) = rfd::FileDialog::new().add_filter("VST preset", &["vstpreset"]).set_title("Load a .vstpreset").pick_file() {
                                if let Err(e) = aplug::load_preset(fx, &f) {
                                    msg = Some(format!("Could not load the preset: {e}"));
                                }
                            }
                        }
                        if widgets::push_button(ui, "Save .vstpreset...").clicked() {
                            if let Some(f) = rfd::FileDialog::new().add_filter("VST preset", &["vstpreset"]).set_file_name(format!("{}.vstpreset", info.name)).set_title("Save a .vstpreset").save_file() {
                                if let Err(e) = aplug::save_preset(fx, &f) {
                                    msg = Some(format!("Could not save the preset: {e}"));
                                }
                            }
                        }
                    }
                });
            }
            ui.separator();

            // ---- Parameters ----
            let key_at = self.animate.contains(&fx.id).then_some(at).filter(|_| keyed.is_some() && chain);
            // Automation Off: the parameters show (and set) their own values, not their envelopes.
            let off = mode == Some(AutoMode::Off);
            let shown = if off { Fx { auto: Default::default(), ..fx.clone() } } else { fx.clone() };
            let (Some(e), d) = (plugins::eval(&shown, at), plugins::preset_eval(info, None)) else { return };
            // R33a: an OFX plug-in's controls as it lays them out now (pages, groups, hidden and greyed parameters,
            // live labels and choice options, push buttons), with what the plug-in said and what it is doing.
            let toggled = &self.toggled;
            let ofx_rows = ofx::layout(&fx.plugin, &fx.id, &|g, open| toggled.contains(&(fx.id.clone(), g.to_string())) != open);
            let warn = super::theme::get().warning_text;
            if let Some(note) = aplug::note(&fx.id) {
                ui.add(Label::new(RichText::new(note).color(super::theme::get().text_dim)).wrap());
            }
            if ofx_rows.is_some() {
                if let Some(hint) = ofx::colour_hint(&fx.plugin, &fx.id, &e) {
                    ui.horizontal_wrapped(|ui| {
                        ui.add(Label::new(RichText::new(hint).color(warn)).wrap());
                        if widgets::push_button(ui, "OK").on_hover_text("Do not show this again for this plug-in").clicked() {
                            ofx::dismiss_hint(&fx.plugin);
                        }
                    });
                }
                if let Some(button) = ofx::busy(&fx.id) {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(format!("Running \"{button}\" (the plug-in may be showing a window of its own)..."));
                        if widgets::push_button(ui, "Stop").on_hover_text("Stop the plug-in host; it starts again with the next frame").clicked() {
                            ofx::stop(&fx.id);
                        }
                    });
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
                }
                if let Some(note) = ofx::note(&fx.id) {
                    let color = if note.starts_with("Error") { warn } else { super::theme::get().text_dim };
                    ui.add(Label::new(RichText::new(note).color(color)).wrap());
                }
            }
            let (mut pressed, mut toggle) = (None, None);
            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                if let Some(rows) = &ofx_rows {
                    let busy = ofx::busy(&fx.id).is_some();
                    for row in rows {
                        match row {
                            ofx::Row::Heading(h) => {
                                ui.add_space(6.0);
                                ui.label(RichText::new(h).strong().underline());
                            }
                            ofx::Row::Group { name, label, depth, open } => {
                                ui.horizontal(|ui| {
                                    ui.add_space(*depth as f32 * 12.0);
                                    let r = ui.add(Label::new(RichText::new(format!("{} {label}", if *open { "-" } else { "+" })).strong()).sense(Sense::click()));
                                    if r.clicked() {
                                        toggle = Some(name.clone());
                                    }
                                });
                            }
                            ofx::Row::Button { name, label, depth, enabled, hint } => {
                                ui.horizontal(|ui| {
                                    ui.add_space(*depth as f32 * 12.0 + LABEL_W);
                                    let b = widgets::push_button_enabled(ui, *enabled && editable && !busy, label);
                                    if b.on_hover_text(if hint.is_empty() { "Runs in the plug-in" } else { hint.as_str() }).clicked() {
                                        pressed = Some(name.clone());
                                    }
                                });
                            }
                            ofx::Row::Param { index, label, depth, enabled, options, slider, hint } => {
                                let (i, spec) = (*index, &info.params[*index]);
                                let changed = e.vals[i].1 != d.vals[i].1;
                                ui.horizontal(|ui| {
                                    ui.add_space(*depth as f32 * 12.0);
                                    if name_cell(ui, label, changed, *depth, hint) && editable {
                                        reset(fx, spec.key, spec.kind);
                                    }
                                    // The slider spans the plug-in's display range, widened to a value outside it (a
                                    // long seed); the value itself is only kept within the hard range (`spec.kind`).
                                    let kind = match (spec.kind, slider, &e.vals[i].1) {
                                        (ParamKind::Float { default, .. }, Some((lo, hi)), Val::F(v)) => ParamKind::Float { min: lo.min(*v), max: hi.max(*v), default },
                                        (k, ..) => k,
                                    };
                                    ui.add_enabled_ui(editable && *enabled, |ui| match (options, &e.vals[i].1) {
                                        (Some(o), Val::F(v)) => live_choice(ui, fx, spec.key, o, *v, key_at, fps),
                                        _ => {
                                            param(ui, fx, spec.key, kind, &e.vals[i].1, key_at, fps, &mut self.fonts);
                                        }
                                    });
                                });
                            }
                        }
                    }
                    return;
                }
                for (i, spec) in info.params.iter().enumerate() {
                    if !spec.section.is_empty() {
                        ui.add_space(if i > 0 { 6.0 } else { 0.0 });
                        ui.label(RichText::new(spec.section).strong());
                    }
                    let changed = e.vals[i].1 != d.vals[i].1;
                    ui.horizontal(|ui| {
                        if name_cell(ui, spec.name, changed, 0, "") && editable {
                            reset(fx, spec.key, spec.kind);
                        }
                        let automated = !off && fx.auto.contains_key(spec.key);
                        match mode.filter(|_| automated) {
                            // An automated parameter: Read shows its envelope; Touch and Latch write it.
                            Some(m) => {
                                let mut scratch = Fx { auto: Default::default(), params: Default::default(), ..fx.clone() };
                                let held = ui.add_enabled_ui(editable && m.writes(), |ui| param(ui, &mut scratch, spec.key, spec.kind, &e.vals[i].1, None, fps, &mut self.fonts)).inner;
                                let new = scratch.params.get(spec.key).and_then(|k| k.keys.first()).map(|k| plugins::to_unit(spec.kind, k.v));
                                if new.is_some() || held {
                                    auto_writes.push((fx.id.clone(), spec.key.to_string(), new));
                                }
                            }
                            None => {
                                ui.add_enabled_ui(editable, |ui| param(ui, fx, spec.key, spec.kind, &e.vals[i].1, key_at, fps, &mut self.fonts));
                            }
                        }
                    });
                }
            });
            if let Some(g) = toggle {
                let key = (fx.id.clone(), g);
                if !self.toggled.remove(&key) {
                    self.toggled.insert(key);
                }
            }
            if let Some(b) = pressed.filter(|_| editable) {
                // The button's action runs in the plug-in host on a thread of its own; its edits come back through the
                // app as one undo step (`App::take_ofx_edits`).
                ofx::press(Some(ui.ctx()), fx, at, &b, size, fps, par);
            }
            if animating {
                let lanes = lane(fx, fps);
                if let Some(r) = keyframes::controller(ui, kf_r, &mut self.kf, info.name, &lanes, span, end, at, fps) {
                    seek = apply_lane(fx, info, r, at, fps);
                }
                // The rest of the chain under it; a click on one's name selects that plug-in.
                let rows: Vec<(&str, &[f64])> = others.iter().map(|o| (o.1.as_str(), o.2.as_slice())).collect();
                if let Some(k) = keyframes::passive_lanes(ui, kf_r, span, fps, &rows) {
                    self.sel = others[k].0;
                }
            }
        });

        // A chain click is its own undo step; parameters wait for the next frame.
        if let Some(op) = op {
            let (t, before) = (&self.target, project.clone());
            let done = match op {
                ChainOp::Enable(i, on) => edit::set_fx_enabled(project, t, i, on),
                ChainOp::Remove(i) => {
                    // R34b: a removed VST3/CLAP instance leaves its host (its editor window closes).
                    if let Some(f) = orig.get(i).filter(|f| aplug::is_ext(&f.plugin)) {
                        aplug::close_instance(f);
                    }
                    edit::remove_fx(project, t, i)
                }
                ChainOp::Move(from, to) => edit::move_fx(project, t, from, to),
                ChainOp::Add(fx) => edit::add_fx(project, t, fx),
            };
            if done {
                history.push(before);
            }
        } else {
            let active = ctx.input(|i| i.pointer.any_down()) || ctx.wants_keyboard_input();
            self.commit(project, history, list, active);
        }
        if let (Some(id), Some(m)) = (parent.as_deref(), mode) {
            for (fx, key, v) in auto_writes {
                super::automation::touch(ctx, project, history, &edit::fx_owner(id, &fx, &key), crate::project::EnvKind::FxParam, m, v);
            }
        }
        if let Some(s) = seek.and_then(|s| self.kf.seek(s.clamp(0.0, span))) {
            project.timeline.cursor_position = base + s;
        }
        (!close, msg)
    }
}

/// One parameter's control, editing `fx`: `cur` is its value now; numbers are keyed at `key_at` (Animate) or set.
/// True while a slider is pressed (an automation write holds it).
#[allow(clippy::too_many_arguments)]
fn param(ui: &mut Ui, fx: &mut Fx, key: &str, kind: ParamKind, cur: &Val, key_at: Option<f64>, fps: f64, fonts: &mut Option<Vec<String>>) -> bool {
    match (kind, cur) {
        (ParamKind::Float { min, max, .. }, Val::F(v)) => {
            let mut x = *v;
            // Wide positive ranges (frequencies, times) are swept on a log scale.
            let r = widgets::slider(ui, &mut x, min..=max, widgets::Slide { clamp: true, log: plugins::log_scale(min, max), ..Default::default() });
            if r.changed() {
                set_num(fx, key, x, *v, key_at, fps);
            }
            return r.is_pointer_button_down_on();
        }
        (ParamKind::Bool { .. }, Val::F(v)) => {
            let mut on = *v >= 0.5;
            if widgets::checkbox(ui, &mut on, "").changed() {
                set_num(fx, key, on as u8 as f32, *v, key_at, fps);
            }
        }
        (ParamKind::Choice { options, .. }, Val::F(v)) => {
            let k = (v.round().max(0.0) as usize).min(options.len().saturating_sub(1));
            widgets::combo(ui, ("fx_choice", key), 180.0, options.get(k).copied().unwrap_or_default(), |ui| {
                for (j, o) in options.iter().enumerate() {
                    if ui.selectable_label(j == k, *o).clicked() {
                        set_num(fx, key, j as f32, *v, key_at, fps);
                    }
                }
            });
        }
        (ParamKind::Color { .. }, Val::C(c)) => {
            let mut u = c.map(|x| (x * 255.0).round() as u8);
            if ui.color_edit_button_srgba_unmultiplied(&mut u).changed() {
                for (j, comp) in ["r", "g", "b", "a"].iter().enumerate() {
                    set_num(fx, &format!("{key}.{comp}"), u[j] as f32 / 255.0, c[j], key_at, fps);
                }
            }
        }
        (ParamKind::Text { multiline, .. }, Val::T(s)) => {
            let mut text = s.clone();
            let edit = if multiline { TextEdit::multiline(&mut text).desired_rows(3) } else { TextEdit::singleline(&mut text) };
            let mut changed = ui.add(edit.desired_width(if key.ends_with("lut") { 230.0 } else { 300.0 })).changed();
            // LUT files are picked rather than typed.
            if key.ends_with("lut") && widgets::push_button(ui, "Browse...").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("Cube LUT", &["cube"]).pick_file() {
                    (text, changed) = (p.display().to_string(), true);
                }
            }
            if changed {
                fx.text.insert(key.to_string(), text);
            }
        }
        (ParamKind::Font { .. }, Val::T(s)) => {
            widgets::combo(ui, ("fx_font", key), 200.0, s.as_str(), |ui| {
                for f in fonts.get_or_insert_with(crate::engine::fonts::list).iter() {
                    if ui.selectable_label(f == s, f).clicked() {
                        fx.text.insert(key.to_string(), f.clone());
                    }
                }
            });
        }
        _ => {}
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Clip;

    /// R33a acceptance: a project using an OFX plug-in that is not installed loads with that FX and every saved
    /// parameter (keys, curves, text) untouched; the FX window shows why it is bypassed without changing anything; it
    /// renders as if absent; a save writes it back as it was.
    #[test]
    fn a_project_with_a_missing_ofx_plugin_keeps_its_parameters() {
        use eframe::egui::{Context, RawInput};
        let (mut p, id) = project();
        let mut gone = Fx { plugin: "ofx:com.example.notinstalled@2".into(), ..plugins::new_fx(plugins::find("milan.fx.invert").unwrap(), None) };
        gone.params.insert("gain".into(), Keyframes { keys: vec![Keyframe { t: 0.0, v: 0.25, curve: Curve::Linear }, Keyframe { t: 1.0, v: 3.5, curve: Curve::Smooth }] });
        gone.params.insert("tint.r".into(), Keyframes { keys: vec![Keyframe { t: 0.0, v: 0.7, curve: Curve::Linear }] });
        gone.text.insert("lut".into(), r"C:\looks\film.cube".into());
        let fx = vec![p.clip(&id).unwrap().fx[0].clone(), gone.clone()];
        p.timeline.video_tracks[0].clips[0].fx = fx.clone();
        let dir = std::env::temp_dir().join("milan-agent").join("ofx").join(format!("missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("uses an OFX plug-in.mnle");
        p.save(&path).unwrap();
        let mut back = Project::load(&path).unwrap();
        assert_eq!(back.clip(&id).unwrap().fx, fx);
        assert!(plugins::eval(&gone, 0.0).is_none(), "bypassed");
        let why = ofx::bypassed_because(&gone.plugin).unwrap();
        assert!(why.contains("com.example.notinstalled") && why.contains("version 2") && why.contains("kept"), "{why}");
        assert_eq!(chain_name(&gone.plugin), "com.example.notinstalled (missing)");
        let (mut h, mut users) = (History::default(), Vec::new());
        let ctx = Context::default();
        crate::ui::fonts::install(&ctx);
        for sel in 0..2 {
            let mut w = FxWindow::open(&back, FxTarget::Event(id.clone())).unwrap();
            w.sel = sel;
            let _ = ctx.run(RawInput::default(), |ctx| assert!(w.show(ctx, &mut back, &mut h, &mut users).0));
        }
        assert_eq!((h.lens(), &back.clip(&id).unwrap().fx), ((0, 0), &fx));
        // It plans and composes as if it were not there.
        let vp = crate::engine::plan::compile(&back);
        let fp = crate::engine::plan::frame_plan(&vp, (2.5 * vp.fps) as i64);
        assert!(fp.fx().iter().all(|e| !e.id.starts_with("ofx:")));
        back.save(&path).unwrap();
        assert_eq!(Project::load(&path).unwrap().clip(&id).unwrap().fx, fx);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn project() -> (Project, String) {
        let mut p = Project::new("t");
        let mut c = Clip::new("c", 2.0, 4.0);
        c.fx.push(plugins::new_fx(plugins::find("milan.fx.brightness_contrast").unwrap(), None));
        let id = c.id.clone();
        p.timeline.video_tracks[0].clips.push(c);
        (p, id)
    }

    /// A drag over several frames (pointer down) then its release: one undo step; the next change is another.
    #[test]
    fn a_parameter_gesture_is_one_undo_step() {
        let (mut p, id) = project();
        let mut h = History::default();
        let mut w = FxWindow::open(&p, FxTarget::Event(id.clone())).unwrap();
        let info = plugins::find("milan.fx.brightness_contrast").unwrap();
        for v in [0.1, 0.2, 0.3] {
            let mut list = edit::fx_list(&p, &w.target).unwrap().0;
            set_num(&mut list[0], "brightness", v, 0.0, None, 30.0);
            w.commit(&mut p, &mut h, list, true);
        }
        let list = edit::fx_list(&p, &w.target).unwrap().0;
        w.commit(&mut p, &mut h, list.clone(), false);
        assert_eq!(h.lens(), (1, 0));
        assert_eq!(plugins::eval(&p.clip(&id).unwrap().fx[0], 0.0).unwrap().f("brightness"), 0.3);
        let mut next = list;
        set_num(&mut next[0], "contrast", 0.5, 0.0, None, 30.0);
        w.commit(&mut p, &mut h, next, false);
        assert_eq!(h.lens(), (2, 0));
        h.undo(&mut p);
        h.undo(&mut p);
        assert_eq!(p.clip(&id).unwrap().fx[0].params, plugins::new_fx(info, None).params, "two undos: the defaults");
    }

    /// A dropped preset shows by name; a user preset named like a built-in one stays apart from it.
    #[test]
    fn the_preset_combo_follows_the_values() {
        let info = plugins::find("milan.fx.vignette").unwrap();
        let strong = plugins::new_fx(info, info.presets.iter().find(|p| p.name == "Strong"));
        let mine = UserPreset { plugin: info.id.into(), name: "Strong".into(), params: [("amount".to_string(), 0.3)].into(), text: Default::default() };
        let users = vec![mine.clone()];
        assert_eq!(shown_preset(info, &users, &strong, None), Some((false, "Strong".into())));
        let user_fx = plugins::new_user_fx(info, &mine);
        assert_eq!(shown_preset(info, &users, &user_fx, Some(&(false, "Strong".into()))), Some((true, "Strong".into())));
        let mut edited = strong;
        set_num(&mut edited, "amount", 0.9, 1.0, None, 30.0);
        assert_eq!(shown_preset(info, &users, &edited, None), None);
    }

    #[test]
    fn audio_targets_are_named_as_audio() {
        let (mut p, _) = project();
        let c = Clip::new("voice", 0.0, 2.0);
        let id = c.id.clone();
        p.timeline.audio_tracks[0].clips.push(c);
        let title = |t: FxTarget| FxWindow::open(&p, t).unwrap().title(&p);
        assert_eq!(title(FxTarget::Event(id)), "Audio Event FX: voice");
        assert_eq!(title(FxTarget::Track(p.timeline.audio_tracks[0].id.clone())), "Audio Track FX: Track 1");
        assert_eq!(title(FxTarget::AudioMaster), "Audio Master FX");
        assert_eq!(title(FxTarget::Track(p.timeline.video_tracks[0].id.clone())), "Video Track FX: Track 1");
    }

    #[test]
    fn the_window_closes_when_its_event_is_deleted() {
        let (mut p, id) = project();
        let (mut h, mut users) = (History::default(), Vec::new());
        let mut w = FxWindow::open(&p, FxTarget::Event(id.clone())).unwrap();
        let ctx = Context::default();
        crate::ui::fonts::install(&ctx);
        let mut open = false;
        let _ = ctx.run(RawInput::default(), |ctx| open = w.show(ctx, &mut p, &mut h, &mut users).0);
        assert!(open);
        assert!(edit::delete_clips(&mut p, &[id]));
        let _ = ctx.run(RawInput::default(), |ctx| open = w.show(ctx, &mut p, &mut h, &mut users).0);
        assert!(!open);
        assert_eq!(h.lens(), (0, 0), "showing it changed nothing");
    }

    #[test]
    fn animated_edits_key_at_the_cursor_and_the_lane_moves_every_parameter() {
        let info = plugins::find("milan.fx.vignette").unwrap();
        let mut fx = plugins::new_fx(info, None);
        let fps = 30.0;
        // Unanimated: one key at 0. Animated: the old value stays at 0 and the new one is keyed at 2 s.
        set_num(&mut fx, "amount", 0.8, 0.5, None, fps);
        assert_eq!(fx.params["amount"].times(), vec![0.0]);
        set_num(&mut fx, "size", 1.0, 0.7, Some(2.0), fps);
        assert_eq!((fx.params["size"].times(), fx.params["size"].keys[0].v), (vec![0.0, 2.0], 0.7));
        assert!(animated(&fx));
        // Add keys every parameter at the cursor; the lane merges times within half a frame.
        apply_lane(&mut fx, info, KfResponse::Add, 1.0, fps);
        assert!(fx.params.values().all(|k| k.times().contains(&1.0)) && fx.params.contains_key("color.a"));
        set_num(&mut fx, "amount", 0.2, 0.0, Some(2.01), fps);
        assert_eq!(lane(&fx, fps).iter().map(|k| k.0).collect::<Vec<_>>(), vec![0.0, 1.0, 2.0]);
        assert_eq!(apply_lane(&mut fx, info, KfResponse::Seek(1.0), 0.0, fps), Some(1.0));
        apply_lane(&mut fx, info, KfResponse::Move(1, 1.5), 0.0, fps);
        apply_lane(&mut fx, info, KfResponse::SetCurve(1, Curve::Hold), 0.0, fps);
        assert!(fx.params.values().flat_map(|k| &k.keys).filter(|k| k.t == 1.5).all(|k| k.curve == Curve::Hold));
        assert!(fx.params.values().all(|k| !k.times().contains(&1.0)));
        apply_lane(&mut fx, info, KfResponse::Delete(2), 0.0, fps);
        apply_lane(&mut fx, info, KfResponse::Delete(0), 0.0, fps); // the first key stays
        assert_eq!(lane(&fx, fps).iter().map(|k| k.0).collect::<Vec<_>>(), vec![0.0, 1.5]);
        // Reset puts a colour back to its default.
        reset(&mut fx, "color", ParamKind::Color { default: [0.0; 4] });
        assert!(!fx.params.keys().any(|k| k.starts_with("color.")));
    }
}
