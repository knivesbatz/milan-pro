//! The FX window (§5.15.2, decision 0.5): one modeless window for Video Event, Track and Output FX, generated media and
//! transitions. The chain bar (bypass box, select, drag to reorder, fx+ and fx×), the preset row (built-in and user
//! presets, save and delete), the parameter grid from the plug-in's `ParamSpec`s and, while Animate is on, the keyframe
//! controller with one lane for the selected plug-in. Each gesture is one undo step.
use eframe::egui::*;
use std::collections::HashSet;
use crate::edit::{self, FxTarget};
use crate::engine::plugins::{self, FxEval, ParamKind, PluginInfo, PluginKind, UserPreset, Val};
use crate::history::History;
use crate::project::{Curve, Fx, Keyframe, Keyframes, Project};
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
}

/// Some parameter is keyed over time (a key off 0, or several).
pub fn animated(fx: &Fx) -> bool {
    fx.params.values().any(|k| k.keys.len() > 1 || k.keys.iter().any(|k| k.t != 0.0))
}

/// VP23 draws widgets flat and borderless: give slider rails, check boxes and fields a visible fill and edge here.
pub fn field_style(ui: &mut Ui) {
    let t = super::theme::get();
    if t.look() == super::theme::UiLook::Vegas16 {
        return;
    }
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
        FxTarget::Track(_) | FxTarget::VideoOutput => Some((cur.max(0.0), crate::render::content_end(p).max(cur).max(1.0), 0.0)),
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
        Some(Self { target, sel: 0, gesture: false, animate, kf: KfState::default(), preset: None, save_as: None, drag: None, fonts: None })
    }

    /// Select the plug-in last in the chain (just dropped on it).
    pub fn select_last(mut self, project: &Project) -> Self {
        self.sel = edit::fx_list(project, &self.target).map_or(0, |l| l.0.len().saturating_sub(1));
        self
    }

    fn title(&self, p: &Project) -> String {
        let clip = |id: &str| p.clip(id).map_or(String::new(), |c| c.name.clone());
        match &self.target {
            FxTarget::Event(id) => format!("Video Event FX: {}", clip(id)),
            FxTarget::Track(id) => format!("Video Track FX: Track {}", p.find_track(id).map_or(0, |t| t.1 + 1)),
            FxTarget::VideoOutput => "Video Output FX".into(),
            FxTarget::Media(id) => format!("Video Media Generators: {}", p.media_pool.iter().find(|m| m.id == *id).map_or("", |m| m.name.as_str())),
            FxTarget::Transition(id) => format!("Transition: {}", clip(id)),
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
        let chain = matches!(self.target, FxTarget::Event(_) | FxTarget::Track(_) | FxTarget::VideoOutput);
        let fps = project.settings.frame_rate;
        let keyed = key_time(project, &self.target);
        let (at, span, base) = keyed.map_or((0.0, 0.0, 0.0), |(_, span, base)| (self.kf.cursor(project.timeline.cursor_position - base).clamp(0.0, span), span, base));
        // Event FX keys stop at the event's end; track and output keys may go past the last event.
        let end = if matches!(self.target, FxTarget::Event(_)) { span } else { f64::INFINITY };
        let mut list = orig.clone();
        self.sel = self.sel.min(list.len().saturating_sub(1));
        let title = self.title(project);
        let label = title.split(':').next().unwrap_or_default().to_string();
        let (mut msg, mut seek, mut op) = (None, None, None);

        let (_, close) = dialogs::tool_window(ctx, ID, &title, SIZE, |ui| {
            let full = ui.available_rect_before_wrap();
            // A Video FX tile dropped anywhere on the window joins the chain.
            if let Some(d) = super::plugin_dropped(&ui.interact(full, ui.id().with("fx_drop"), Sense::hover())) {
                if chain && editable && d.kind == PluginKind::VideoFx {
                    (op, self.sel) = (Some(ChainOp::Add(d.fx.clone())), list.len());
                } else {
                    msg = Some(if chain { "The target is locked." } else { "Only a plug-in chain takes video FX." }.to_string());
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
                    let name = plugins::find(&f.plugin).map_or(f.plugin.clone(), |i| i.name.to_string());
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
                        if ui.add_enabled(editable && !list.is_empty(), Button::new("fx×")).on_hover_text("Remove Selected Plug-in").clicked() {
                            op = Some(ChainOp::Remove(self.sel));
                            self.sel = self.sel.saturating_sub(1);
                        }
                        ui.add_enabled_ui(editable, |ui| {
                            // shortcut: a menu of plug-ins and their presets, without the chooser's thumbnails; reuse
                            // the chooser widget here if users miss them.
                            ui.menu_button("fx+", |ui| {
                                for info in plugins::catalog(PluginKind::VideoFx) {
                                    ui.menu_button(info.name, |ui| {
                                        let builtin = info.presets.iter().map(|p| (p.name.to_string(), plugins::new_fx(info, Some(p))));
                                        let mine = users.iter().filter(|u| u.plugin == info.id).map(|u| (u.name.clone(), plugins::new_user_fx(info, u)));
                                        for (name, fx) in builtin.chain(mine) {
                                            if ui.button(name).clicked() {
                                                (op, self.sel) = (Some(ChainOp::Add(fx)), list.len());
                                                ui.close_menu();
                                            }
                                        }
                                    });
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
                .map(|(i, f)| (i, plugins::find(&f.plugin).map_or(f.plugin.clone(), |p| p.name.to_string()), lane(f, fps).iter().map(|k| k.0).collect()))
                .collect();
            let Some(fx) = list.get_mut(self.sel) else {
                ui.label("No plug-ins. Drag one here from the Video FX window, or use fx+.");
                return;
            };
            let Some(info) = plugins::find(&fx.plugin) else {
                ui.label(format!("\"{}\" is not available in this version of Milan Pro. It is kept, and bypassed.", fx.plugin));
                return;
            };

            // ---- Preset row ----
            ui.horizontal(|ui| {
                ui.label("Preset:");
                let mut pick: Option<Fx> = None;
                let shown = shown_preset(info, users, fx, self.preset.as_ref());
                ComboBox::from_id_salt("fx_preset").width(220.0).selected_text(shown.as_ref().map_or("", |p| p.1.as_str())).show_ui(ui, |ui| {
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
                    *fx = Fx { id: fx.id.clone(), enabled: fx.enabled, ..p };
                }
                match &mut self.save_as {
                    Some(name) => {
                        ui.add(TextEdit::singleline(name).desired_width(120.0).hint_text("Preset name"));
                        if ui.add_enabled(!name.trim().is_empty(), Button::new("Save")).clicked() {
                            let name = name.trim().to_string();
                            match plugins::save_user_preset(plugins::user_preset_of(fx, &name, at)) {
                                Ok(()) => {
                                    *users = plugins::load_user_presets();
                                    self.preset = Some((true, name));
                                }
                                Err(e) => msg = Some(format!("Could not save the preset: {e}")),
                            }
                            self.save_as = None;
                        } else if ui.button("Cancel").clicked() {
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
                if keyed.is_some() && chain {
                    let mut on = self.animate.contains(&fx.id);
                    if ui.add_enabled(editable, Checkbox::new(&mut on, "Animate")).changed() {
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
            ui.separator();

            // ---- Parameters ----
            let key_at = self.animate.contains(&fx.id).then_some(at).filter(|_| keyed.is_some() && chain);
            let (Some(e), d) = (plugins::eval(fx, at), plugins::preset_eval(info, None)) else { return };
            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                for (i, spec) in info.params.iter().enumerate() {
                    if !spec.section.is_empty() {
                        ui.add_space(if i > 0 { 6.0 } else { 0.0 });
                        ui.label(RichText::new(spec.section).strong());
                    }
                    let changed = e.vals[i].1 != d.vals[i].1;
                    ui.horizontal(|ui| {
                        let text = format!("{}{}", spec.name, if changed { " *" } else { "" });
                        let lr = ui.add_sized([LABEL_W, 18.0], Label::new(text).sense(Sense::click()).truncate()).on_hover_text("Double-click to reset");
                        if lr.double_clicked() && editable {
                            reset(fx, spec.key, spec.kind);
                        }
                        ui.add_enabled_ui(editable, |ui| param(ui, fx, spec.key, spec.kind, &e.vals[i].1, key_at, fps, &mut self.fonts));
                    });
                }
            });
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
                ChainOp::Remove(i) => edit::remove_fx(project, t, i),
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
        if let Some(s) = seek.and_then(|s| self.kf.seek(s.clamp(0.0, span))) {
            project.timeline.cursor_position = base + s;
        }
        (!close, msg)
    }
}

/// One parameter's control, editing `fx`: `cur` is its value now; numbers are keyed at `key_at` (Animate) or set.
#[allow(clippy::too_many_arguments)]
fn param(ui: &mut Ui, fx: &mut Fx, key: &str, kind: ParamKind, cur: &Val, key_at: Option<f64>, fps: f64, fonts: &mut Option<Vec<String>>) {
    match (kind, cur) {
        (ParamKind::Float { min, max, .. }, Val::F(v)) => {
            let mut x = *v;
            if ui.add(Slider::new(&mut x, min..=max).clamping(SliderClamping::Always)).changed() {
                set_num(fx, key, x, *v, key_at, fps);
            }
        }
        (ParamKind::Bool { .. }, Val::F(v)) => {
            let mut on = *v >= 0.5;
            if ui.checkbox(&mut on, "").changed() {
                set_num(fx, key, on as u8 as f32, *v, key_at, fps);
            }
        }
        (ParamKind::Choice { options, .. }, Val::F(v)) => {
            let k = (v.round().max(0.0) as usize).min(options.len().saturating_sub(1));
            ComboBox::from_id_salt(("fx_choice", key)).width(180.0).selected_text(options.get(k).copied().unwrap_or_default()).show_ui(ui, |ui| {
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
            if key.ends_with("lut") && ui.button("Browse...").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("Cube LUT", &["cube"]).pick_file() {
                    (text, changed) = (p.display().to_string(), true);
                }
            }
            if changed {
                fx.text.insert(key.to_string(), text);
            }
        }
        (ParamKind::Font { .. }, Val::T(s)) => {
            ComboBox::from_id_salt(("fx_font", key)).width(200.0).selected_text(s.as_str()).show_ui(ui, |ui| {
                for f in fonts.get_or_insert_with(crate::engine::fonts::list).iter() {
                    if ui.selectable_label(f == s, f).clicked() {
                        fx.text.insert(key.to_string(), f.clone());
                    }
                }
            });
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Clip;

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
