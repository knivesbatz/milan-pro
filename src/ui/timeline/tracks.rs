//! Track list (§5.9.3): colour strip with ≡ / ⋯, number and type badge; per-look Mute/Solo (and VP23 compositing
//! and fx buttons); Level / Vol / Pan sliders with the "II" thumb; audio meter; selection, rename, height drag.
use eframe::egui::*;
use crate::edit;
use crate::history::History;
use crate::project::*;
use crate::ui::Selection;
use crate::ui::icons::{Icon, paint_icon};
use crate::ui::theme::{self, Theme, UiLook};
use super::{GROUP_H, Geo, Head, MIN_TRACK_H, Rename, Row, TlState};

/// Audio fader law: −inf at the bottom, 0 dB at 70 % of travel, +12 dB at the top (cubic below 0 dB).
pub fn fader_gain(pos: f32) -> f32 {
    let p = pos.clamp(0.0, 1.0);
    if p < 0.7 { (p / 0.7).powi(3) } else { 10f32.powf((p - 0.7) / 0.3 * 12.0 / 20.0) }
}

pub fn fader_pos(gain: f32) -> f32 {
    if gain <= 1.0 { 0.7 * gain.max(0.0).cbrt() } else { (0.7 + 20.0 * gain.log10() / 12.0 * 0.3).min(1.0) }
}

fn db_text(gain: f32) -> String {
    // + 0.0 turns -0.0 into 0.0.
    if gain <= 1e-5 { "-Inf. dB".into() } else { format!("{:.1} dB", 20.0 * gain.log10() + 0.0) }
}

fn pan_text(pan: f32) -> String {
    let p = (pan * 100.0).round();
    if p == 0.0 { "Center".into() } else if p < 0.0 { format!("L {:.0}%", -p) } else { format!("R {p:.0}%") }
}

#[derive(Clone, Copy, PartialEq)]
enum Btn {
    /// Arm for Record: shown (lit) on armed audio tracks only, so the default button set stays VEGAS's.
    Arm,
    Comp,
    Fx,
    Mute,
    Solo,
}

#[derive(Clone, Copy, PartialEq)]
enum Slider {
    Level,
    Vol,
    Pan,
    /// R8a: the multipurpose slider: the send to a bus or assignable FX chain (shown when the project has one).
    Send,
}

/// R8a: what the multipurpose slider of a track shows: the bus or chain id and name, and whether a new send to it is
/// pre-volume.
struct SendCx {
    to: Option<String>,
    name: String,
    pre: bool,
}

/// The bus or chain `tr`'s multipurpose slider shows: the one picked, else the first.
fn send_cx(tl: &Timeline, tr: &Track) -> SendCx {
    let a = &tl.audio;
    let b = tr.send_view.as_deref().and_then(|id| a.bus(id)).or_else(|| a.buses.first().or(a.fx.first()));
    SendCx { to: b.map(|b| b.id.clone()), name: b.map_or(String::new(), |b| b.name.clone()), pre: b.is_some_and(|b| edit::send_pre_default(tl, &b.id)) }
}

impl Slider {
    fn send_level(tr: &Track, cx: &SendCx) -> f32 {
        cx.to.as_deref().and_then(|to| tr.send(to)).map_or(0.0, |s| s.level)
    }
    fn pos(self, tr: &Track, cx: &SendCx) -> f32 {
        match self {
            Slider::Level => tr.volume.clamp(0.0, 1.0),
            Slider::Vol => fader_pos(tr.volume),
            Slider::Pan => (tr.pan.clamp(-1.0, 1.0) + 1.0) * 0.5,
            Slider::Send => fader_pos(Self::send_level(tr, cx)),
        }
    }
    fn set(self, tr: &mut Track, pos: f32, cx: &SendCx) {
        let pos = pos.clamp(0.0, 1.0);
        match self {
            Slider::Level => tr.volume = pos,
            Slider::Vol => tr.volume = fader_gain(pos),
            Slider::Pan => tr.pan = pos * 2.0 - 1.0,
            Slider::Send => {
                if let Some(to) = &cx.to {
                    edit::set_send(tr, to, fader_gain(pos), cx.pre);
                }
            }
        }
    }
    fn reset(self, tr: &mut Track, cx: &SendCx) {
        match self {
            Slider::Level | Slider::Vol => tr.volume = 1.0,
            Slider::Pan => tr.pan = 0.0,
            Slider::Send => self.set(tr, fader_pos(1.0), cx),
        }
    }
    fn label(self, cx: &SendCx) -> String {
        match self {
            Slider::Level => "Level:".into(),
            Slider::Vol => "Vol:".into(),
            Slider::Pan => "Pan:".into(),
            Slider::Send => format!("{}:", cx.name),
        }
    }
    fn value(self, tr: &Track, cx: &SendCx) -> String {
        match self {
            Slider::Level => format!("{:.1} %", tr.volume * 100.0),
            Slider::Vol => db_text(tr.volume),
            Slider::Pan => pan_text(tr.pan),
            Slider::Send => db_text(Self::send_level(tr, cx)),
        }
    }
}

/// Where everything sits in one header.
struct Layout {
    /// Header without the 2 px separator.
    rect: Rect,
    strip: Rect,
    body: Rect,
    more: Rect,
    buttons: Vec<(Btn, Rect)>,
    /// (slider, row rect, groove x range)
    sliders: Vec<(Slider, Rect, Rangef)>,
    meter: Option<Rect>,
    /// R8b: the meter runs across the top row (Use Vertical Meters off).
    hmeter: bool,
}

/// R8b: the track meters' view (track header › Show Output Meters, Use Vertical Meters, Channel Width).
#[derive(Clone, Copy)]
struct MeterOpts {
    show: bool,
    vertical: bool,
    width: u8,
}

fn meter_opts(ctx: &Context) -> MeterOpts {
    let s = crate::ui::menus::state(ctx);
    MeterOpts { show: s.output_meters, vertical: s.vertical_meters, width: s.meter_width }
}

/// `sends`: the project has a bus or an assignable FX chain (an audio track shows the multipurpose slider).
fn layout(t: &Theme, g: &Geo, row: &Row, armed: bool, sends: bool, mo: MeterOpts) -> Layout {
    let v16 = t.look() == UiLook::Vegas16;
    let rect = Rect::from_min_size(pos2(g.rect.min.x, row.top), vec2(g.header_w, (row.h - 2.0).max(1.0)));
    let strip = rect.with_max_x((rect.min.x + t.m.color_strip_w).min(rect.max.x));
    let body = rect.with_min_x(strip.max.x + 1.0);
    let more = Rect::from_min_size(rect.min + vec2(5.0, 4.0), vec2(14.0, 14.0));
    let meter_w: f32 = match (row.is_video || !mo.show || !mo.vertical, v16) {
        (true, _) => 0.0,
        (false, true) => 21.0,
        (false, false) => 18.0,
    } * [0.6, 1.0, 1.5][mo.width.min(2) as usize];
    let meter_w = meter_w.round();
    let right = body.max.x - meter_w;
    let meter = (meter_w > 0.0 && body.width() > 120.0)
        // VP16 keeps 16 px above the meter for the peak readout on tall tracks only (68 px: labels fill it).
        .then(|| Rect::from_min_max(pos2(right, body.min.y + if v16 && rect.height() > 80.0 { 16.0 } else { 4.0 }), pos2(body.max.x - 2.0, body.max.y - 3.0)))
        .filter(|m| m.height() > 8.0);
    let kinds: &[Btn] = match (v16, row.is_video, armed) {
        (true, false, true) => &[Btn::Arm, Btn::Mute, Btn::Solo],
        (true, _, _) => &[Btn::Mute, Btn::Solo],
        (false, true, _) => &[Btn::Comp, Btn::Fx, Btn::Mute, Btn::Solo],
        (false, false, true) => &[Btn::Arm, Btn::Fx, Btn::Mute, Btn::Solo],
        (false, false, false) => &[Btn::Fx, Btn::Mute, Btn::Solo],
    };
    // Row 1: right-aligned on a 24 px pitch with a 6 px right padding; buttons that don't fit are left out.
    let mut buttons = Vec::new();
    for (k, b) in kinds.iter().rev().enumerate() {
        let x = right - 6.0 - 16.0 - 24.0 * k as f32;
        if x >= body.min.x + 4.0 {
            buttons.push((*b, Rect::from_min_size(pos2(x, rect.min.y + 3.0), vec2(16.0, 16.0))));
        }
    }
    let groove = Rangef::new(body.min.x + 100.0, right - 8.0);
    let mut sliders = Vec::new();
    let rows: &[Slider] = match (row.is_video, sends) {
        (true, _) => &[Slider::Level],
        (false, false) => &[Slider::Vol, Slider::Pan],
        (false, true) => &[Slider::Vol, Slider::Pan, Slider::Send],
    };
    // VP23 squeezes the rows to a 16 px pitch on short tracks so Vol and Pan both fit in 52 px (c_ui_tl).
    let (y0, pitch) = if !v16 && rect.height() < 64.0 { (19.0, 16.0) } else { (27.0, 22.0) };
    for (k, s) in rows.iter().enumerate() {
        let y = rect.min.y + y0 + pitch * k as f32;
        let r = Rect::from_min_max(pos2(body.min.x, y), pos2(right, y + 14.0));
        if r.max.y <= rect.max.y - 1.0 {
            sliders.push((*s, r, groove));
        }
    }
    // Horizontal meters: two bars across the top row, left of the buttons.
    let hmeter = mo.show && !mo.vertical && !row.is_video;
    let meter = if hmeter {
        let x1 = buttons.iter().map(|b| b.1.min.x).fold(right, f32::min) - 8.0;
        let th = [2.0, 3.0, 5.0][mo.width.min(2) as usize];
        Some(Rect::from_min_max(pos2(body.min.x + 6.0, rect.min.y + 11.0 - th), pos2(x1, rect.min.y + 12.0 + th))).filter(|m| m.width() > 24.0)
    } else {
        meter
    };
    Layout { rect, strip, body, more, buttons, sliders, meter, hmeter }
}

/// A track group header: the row, its ▾ / ▸ disclosure, and its Mute and Solo buttons (None when too narrow).
struct HeadLayout {
    rect: Rect,
    arrow: Rect,
    buttons: Option<(Rect, Rect)>,
}

fn head_layout(g: &Geo, h: &Head) -> HeadLayout {
    let rect = Rect::from_min_size(pos2(g.rect.min.x, h.top), vec2(g.header_w, GROUP_H - 2.0));
    let arrow = Rect::from_min_size(rect.min + vec2(4.0, 2.0), vec2(16.0, 16.0));
    // Solo rightmost, Mute 24 px to its left, as on a track header.
    let solo = Rect::from_min_size(pos2(rect.max.x - 22.0, rect.min.y + 2.0), vec2(16.0, 16.0));
    let mute = solo.translate(vec2(-24.0, 0.0));
    HeadLayout { rect, arrow, buttons: (mute.min.x > arrow.max.x + 40.0).then_some((mute, solo)) }
}

/// The member track ids of group `g`, in list order.
fn members(project: &Project, g: &TrackGroup) -> Vec<String> {
    let tl = &project.timeline;
    tl.video_tracks.iter().chain(&tl.audio_tracks).filter(|t| g.tracks.contains(&t.id)).map(|t| t.id.clone()).collect()
}

/// Group headers: a click selects the group's tracks, the disclosure collapses / expands it (view state, no undo
/// step), M / S toggle the group's Mute / Solo (one undo step), a double-click on the name renames it.
pub(super) fn interact_heads(ui: &mut Ui, g: &Geo, heads: &[Head], project: &mut Project, history: &mut History, sel: &mut Selection, st: &mut TlState) {
    let shown = |r: Rect| r.min.y >= g.list.min.y && r.max.y <= g.list.max.y;
    for (k, h) in heads.iter().enumerate() {
        let l = head_layout(g, h);
        let Some(grp) = project.timeline.track_groups.get(h.group).filter(|_| l.rect.intersects(g.list)) else { continue };
        let (gid, name, mates) = (grp.id.clone(), grp.name.clone(), members(project, grp));
        let id = ui.id().with(("group_hdr", k));
        let body = ui.interact(l.rect.intersect(g.list), id.with("body"), Sense::click());
        if body.clicked() || body.secondary_clicked() {
            (sel.tracks, sel.list_focus, sel.env) = (mates.clone(), true, None);
        }
        if body.double_clicked() && body.interact_pointer_pos().is_some_and(|p| p.x > l.arrow.max.x) {
            st.rename = Some((Rename::Group(gid.clone()), name));
        }
        if shown(l.arrow) && ui.interact(l.arrow, id.with("arrow"), Sense::click()).on_hover_text("Collapse / Expand").clicked() {
            let grp = &mut project.timeline.track_groups[h.group];
            grp.collapsed = !grp.collapsed;
            project.mark_modified(); // a view change: no undo step
        }
        let Some((mute, solo)) = l.buttons else { continue };
        for (r, is_solo, tip) in [(mute, false, "Mute Track Group"), (solo, true, "Solo Track Group")] {
            if shown(r) && ui.interact(r, id.with(tip), Sense::click()).on_hover_text(tip).clicked() {
                let before = project.clone();
                if edit::toggle_track_groups(project, &mates[..mates.len().min(1)], is_solo) {
                    history.push(before);
                }
            }
        }
    }
}

/// Paint the group headers: ▾ (▸ when collapsed), the name, and the group's Mute and Solo.
fn paint_heads(p: &Painter, t: &Theme, g: &Geo, heads: &[Head], project: &Project) {
    for h in heads {
        let (l, Some(grp)) = (head_layout(g, h), project.timeline.track_groups.get(h.group)) else { continue };
        if !l.rect.intersects(g.list) {
            continue;
        }
        p.rect_filled(l.rect, 0.0, t.track_header_bg);
        let c = l.arrow.center();
        let pts = if grp.collapsed {
            vec![c + vec2(-2.0, -4.0), c + vec2(2.0, 0.0), c + vec2(-2.0, 4.0)]
        } else {
            vec![c + vec2(-4.0, -2.0), c + vec2(4.0, -2.0), c + vec2(0.0, 2.0)]
        };
        p.add(Shape::convex_polygon(pts, t.text_bright, Stroke::NONE));
        let right = l.buttons.map_or(l.rect.max.x, |(m, _)| m.min.x) - 4.0;
        let name = Rect::from_min_max(pos2(l.arrow.max.x + 4.0, l.rect.min.y), pos2(right, l.rect.max.y));
        p.with_clip_rect(name.intersect(p.clip_rect())).text(name.left_center(), Align2::LEFT_CENTER, &grp.name, t.font(t.font.ui), t.text_bright);
        if let Some((mute, solo)) = l.buttons {
            paint_toggle(p, t, mute, Icon::Mute, grp.muted);
            paint_toggle(p, t, solo, Icon::Solo, grp.solo);
        }
        let y = l.rect.max.y;
        let x = Rangef::new(g.rect.min.x, g.rect.min.x + g.header_w);
        p.rect_filled(Rect::from_x_y_ranges(x, Rangef::new(y, y + 1.0)), 0.0, t.track_separator);
        p.rect_filled(Rect::from_x_y_ranges(x, Rangef::new(y + 1.0, y + 2.0)), 0.0, t.track_separator_2);
    }
}

/// Header reorder drag: the dragged tracks (one kind) and the gap they would land in.
#[derive(Clone)]
pub(super) struct TrackDrag {
    is_video: bool,
    ids: Vec<String>,
    origin_y: f32,
    to: Option<usize>,
}

/// Clicks, slider drags, renames, reorder and height drags in the visible headers.
pub(super) fn interact(ui: &mut Ui, g: &Geo, rows: &[Row], project: &mut Project, history: &mut History, sel: &mut Selection, st: &mut TlState) {
    let t = theme::get();
    let visible = |r: Rect| r.min.y >= g.list.min.y && r.max.y <= g.list.max.y;
    let track_id = |project: &Project, r: &Row| project.timeline.tracks(r.is_video)[r.idx].id.clone();
    for row in rows {
        if row.top > g.list.max.y || row.top + row.h < g.list.min.y {
            continue;
        }
        let sends = project.timeline.audio != AudioSettings::default();
        let l = layout(&t, g, row, project.timeline.tracks(row.is_video)[row.idx].armed, sends, meter_opts(ui.ctx()));
        let id = ui.id().with(("track_hdr", row.is_video, row.idx));
        let tid = track_id(project, row);

        // Body: select (Ctrl toggles, Shift extends; a right-click selects an unselected track); double-click on
        // the name area renames; a drag from the colour strip reorders.
        let body_r = l.rect.intersect(g.list);
        let br = ui.interact(body_r, id.with("body"), Sense::click_and_drag());
        if br.clicked() || br.secondary_clicked() {
            let m = ui.input(|i| i.modifiers);
            (sel.list_focus, sel.env) = (true, None);
            if br.secondary_clicked() {
                // A selected track stays selected and becomes the focus (last), so the menu acts on it.
                if let Some(k) = sel.tracks.iter().position(|s| *s == tid) {
                    let f = sel.tracks.remove(k);
                    sel.tracks.push(f);
                } else {
                    sel.tracks = vec![tid.clone()];
                }
            } else if m.shift {
                let a = st.anchor.unwrap_or(row.num);
                sel.tracks = rows.iter().filter(|r| r.num >= a.min(row.num) && r.num <= a.max(row.num)).map(|r| track_id(project, r)).collect();
            } else if m.command {
                if let Some(k) = sel.tracks.iter().position(|s| *s == tid) {
                    sel.tracks.remove(k);
                } else {
                    sel.tracks.push(tid.clone());
                }
                st.anchor = Some(row.num);
            } else {
                sel.tracks = vec![tid.clone()];
                st.anchor = Some(row.num);
            }
        }
        if let Some(o) = ui.input(|i| i.pointer.press_origin()).filter(|o| br.drag_started() && l.strip.contains(*o)) {
            // A selected track drags the selected tracks of its kind along, in list order.
            let ids = if sel.tracks.contains(&tid) {
                project.timeline.tracks(row.is_video).iter().filter(|t| sel.tracks.contains(&t.id)).map(|t| t.id.clone()).collect()
            } else {
                vec![tid.clone()]
            };
            st.track_drag = Some(TrackDrag { is_video: row.is_video, ids, origin_y: o.y, to: None });
        }
        if br.double_clicked() && br.interact_pointer_pos().is_some_and(|p| p.y < l.rect.min.y + 24.0 && p.x > l.body.min.x) {
            st.rename = Some((Rename::Track(row.is_video, row.idx), project.timeline.tracks(row.is_video)[row.idx].name.clone()));
        }
        // A video FX tile dropped on a video track's header goes on the track's FX (one undo step), and its FX window opens.
        if let Some(d) = crate::ui::plugin_dropped(&br) {
            let (target, before) = (edit::FxTarget::Track(tid.clone()), project.clone());
            let comp = d.kind == crate::engine::plugins::PluginKind::Compositor;
            // A compositor sets the track's Custom compositing mode.
            if row.is_video && comp && edit::set_compositor(project, std::slice::from_ref(&tid), &d.fx) {
                history.push(before);
                crate::ui::open_tool(ui.ctx(), crate::ui::ToolWin::Fx(edit::FxTarget::Compositor(tid.clone())));
            } else if row.is_video && d.kind == crate::engine::plugins::PluginKind::VideoFx && edit::add_fx(project, &target, d.fx.clone()) {
                history.push(before);
                crate::ui::open_tool(ui.ctx(), crate::ui::ToolWin::Fx(target));
            } else {
                crate::ui::hint(ui.ctx(), "Drop video FX on an unlocked video track's header.");
            }
        }
        // Right-click: the timeline's context-menu handler (ctx.rs) covers headers too.

        // ≡ / ⋯ menu: the buttons that are not shown plus the button-set entry.
        if visible(l.more) && l.strip.width() >= 19.0 {
            let mr = ui.interact(l.more, id.with("more"), Sense::click()).on_hover_text("More");
            let pid = id.with("more_popup");
            if mr.clicked() {
                ui.memory_mut(|m| m.toggle_popup(pid));
            }
            let t = theme::get();
            super::super::chrome::with_menu_style(&ui.ctx().clone(), &t, || {
                #[allow(deprecated)]
                popup::popup_below_widget(ui, pid, &mr, PopupCloseBehavior::CloseOnClick, |ui| {
                    ui.set_min_width(180.0);
                    super::ctx::more_menu(ui, project, history, sel, row.is_video, row.idx);
                });
            });
        }

        for (b, r) in &l.buttons {
            if !visible(*r) {
                continue;
            }
            let tip = match b {
                Btn::Arm => "Arm for Record",
                Btn::Comp => "Compositing Mode",
                Btn::Fx => "Track FX",
                Btn::Mute => "Mute",
                Btn::Solo => "Solo",
            };
            let resp = ui.interact(*r, id.with(tip), Sense::click()).on_hover_text(tip);
            if *b == Btn::Comp {
                // VP23 Compositing Mode: the mode list drops down from the button.
                let pid = id.with("comp_popup");
                if resp.clicked() {
                    ui.memory_mut(|m| m.toggle_popup(pid));
                }
                let tr = &project.timeline.tracks(row.is_video)[row.idx];
                let (cur, locked) = (tr.composite, tr.locked);
                let mut picked = None;
                super::super::chrome::with_menu_style(&ui.ctx().clone(), &t, || {
                    #[allow(deprecated)]
                    popup::popup_below_widget(ui, pid, &resp, PopupCloseBehavior::CloseOnClick, |ui| {
                        ui.set_min_width(180.0);
                        picked = crate::ui::trackmotion::composite_menu(ui, cur, !locked);
                    });
                });
                if let Some(m) = picked {
                    let before = project.clone();
                    if edit::set_composite(project, &tid, m) {
                        history.push(before);
                    }
                }
            }
            if resp.clicked() && *b == Btn::Arm {
                // Through the app (it owns the input device): this track becomes the focus, then Arm toggles it.
                (sel.tracks, sel.list_focus, sel.env) = (vec![tid.clone()], true, None);
                crate::ui::menus::send(ui.ctx(), crate::ui::keymap::Command::Arm);
            }
            if resp.clicked() && *b == Btn::Fx {
                crate::ui::open_tool(ui.ctx(), crate::ui::ToolWin::Fx(edit::FxTarget::Track(tid.clone())));
            }
            if resp.clicked() && matches!(b, Btn::Mute | Btn::Solo) {
                history.push(project.clone());
                let tr = &mut project.timeline.tracks_mut(row.is_video)[row.idx];
                if *b == Btn::Mute {
                    tr.muted = !tr.muted;
                } else {
                    tr.solo = !tr.solo;
                }
                project.mark_modified();
            }
        }

        // The meter (and the VP16 peak readout above it): a click resets the peak readout and the clip lamps.
        if let Some(m) = l.meter.filter(|m| visible(*m)) {
            let hit = if l.hmeter { m } else { m.with_min_y(l.body.min.y + 2.0) };
            let tip = if project.timeline.tracks(row.is_video)[row.idx].armed { "Recording input level" } else { "Playback level: click to reset the peaks" };
            if ui.interact(hit, id.with("meter"), Sense::click()).on_hover_text(tip).clicked() {
                crate::ui::mixer::reset_level(ui.ctx(), &tid);
            }
        }

        // Sliders: drag (Ctrl = fine), click jumps, double-click resets, wheel nudges.
        for (s, r, groove) in &l.sliders {
            if !visible(*r) || groove.span() < 20.0 {
                continue;
            }
            let hit = Rect::from_x_y_ranges(Rangef::new(groove.min - 9.0, groove.max + 9.0), r.y_range());
            let resp = ui.interact(hit, id.with(("slider", *s as u8)), Sense::click_and_drag());
            let tr = &project.timeline.tracks(row.is_video)[row.idx];
            let cx = send_cx(&project.timeline, tr);
            if *s == Slider::Send {
                send_menu(ui, id, Rect::from_x_y_ranges(Rangef::new(l.body.min.x, groove.min - 10.0), r.y_range()), project, history, row, &cx);
            }
            let tr = &project.timeline.tracks(row.is_video)[row.idx];
            let pos = s.pos(tr, &cx);
            let value = s.value(tr, &cx);
            if resp.double_clicked() {
                history.push(project.clone());
                s.reset(&mut project.timeline.tracks_mut(row.is_video)[row.idx], &cx);
                project.mark_modified();
            } else if resp.drag_started() || resp.clicked() {
                history.push(project.clone());
                project.mark_modified();
                // Pressing off the thumb jumps there first.
                if let Some(p) = resp.interact_pointer_pos() {
                    let thumb_x = groove.min + pos * groove.span();
                    if (p.x - thumb_x).abs() > 8.0 {
                        s.set(&mut project.timeline.tracks_mut(row.is_video)[row.idx], (p.x - groove.min) / groove.span(), &cx);
                    }
                }
            } else if resp.dragged() {
                let fine = if ui.input(|i| i.modifiers.command) { 0.1 } else { 1.0 };
                let np = pos + resp.drag_delta().x / groove.span() * fine;
                s.set(&mut project.timeline.tracks_mut(row.is_video)[row.idx], np, &cx);
            }
            if resp.hovered() {
                let d = ui.input(|i| i.raw_scroll_delta.y);
                if d != 0.0 {
                    history.push(project.clone());
                    s.set(&mut project.timeline.tracks_mut(row.is_video)[row.idx], pos + d.signum() * 0.01, &cx);
                    project.mark_modified();
                    ui.input_mut(|i| {
                        i.raw_scroll_delta = Vec2::ZERO;
                        i.smooth_scroll_delta = Vec2::ZERO;
                    });
                }
            }
            resp.on_hover_text(value);
        }
    }

    // Bottom-edge height drag (±3 px); registered last so it wins over the bodies.
    for row in rows {
        let bottom = row.top + row.h;
        if bottom < g.list.min.y + 3.0 || bottom > g.list.max.y {
            continue;
        }
        let zone = Rect::from_x_y_ranges(Rangef::new(g.rect.min.x, g.rect.min.x + g.header_w), Rangef::new(bottom - 3.0, bottom + 3.0));
        let r = ui.interact(zone, ui.id().with(("track_h", row.is_video, row.idx)), Sense::drag());
        if r.hovered() || r.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
        }
        if r.drag_started() {
            st.height_drag = Some(row.h);
        }
        if let (Some(h0), true) = (st.height_drag, r.dragged()) {
            let total = r.interact_pointer_pos().zip(ui.input(|i| i.pointer.press_origin())).map_or(0.0, |(p, o)| p.y - o.y);
            let max = (g.lanes.height()).max(MIN_TRACK_H);
            let h = (h0 + total).round().clamp(MIN_TRACK_H, max);
            // Ctrl+drag on a selected track sizes every selected track.
            let tid = track_id(project, row);
            let all = ui.input(|i| i.modifiers.command) && sel.tracks.contains(&tid);
            let tl = &mut project.timeline;
            for tr in tl.video_tracks.iter_mut().chain(tl.audio_tracks.iter_mut()) {
                if tr.id == tid || (all && sel.tracks.contains(&tr.id)) {
                    tr.height = h;
                }
            }
            project.mark_modified();
        }
        if r.drag_stopped() {
            st.height_drag = None;
        }
    }

    // Reorder drag: past 4 px the target gap follows the pointer; the release moves the tracks (one undo step).
    if let Some(d) = st.track_drag.as_mut() {
        let (ptr, down) = ui.input(|i| (i.pointer.latest_pos(), i.pointer.primary_down()));
        if let Some(p) = ptr {
            // The gap before the first row of the kind below the pointer (rows skip hidden and collapsed tracks).
            let len = project.timeline.tracks(d.is_video).len();
            d.to = ((p.y - d.origin_y).abs() > 4.0).then(|| rows.iter().find(|r| r.is_video == d.is_video && r.top + r.h * 0.5 >= p.y).map_or(len, |r| r.idx));
        }
        if !down {
            if let Some(to) = d.to {
                let before = project.clone();
                if edit::move_tracks(project, d.is_video, &d.ids, to) {
                    history.push(before);
                }
            }
            st.track_drag = None;
        }
    }
}

/// R8a: a click on the multipurpose slider's label picks the bus or assignable FX chain it shows, and whether that send
/// is taken before or after the track's volume (VEGAS: right-click the send fader for Pre / Post Volume).
// shortcut: Pre / Post Volume sit in the label's list rather than on a right-click of the fader, which opens the track
// menu here; move them when the header gets per-control context menus.
fn send_menu(ui: &mut Ui, id: Id, label: Rect, project: &mut Project, history: &mut History, row: &Row, cx: &SendCx) {
    let resp = ui.interact(label, id.with("send_label"), Sense::click()).on_hover_text("Choose the bus or assignable FX chain this slider sends to");
    let pid = id.with("send_popup");
    if resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(pid));
    }
    let t = theme::get();
    let (a, tr) = (&project.timeline.audio, &project.timeline.audio_tracks[row.idx]);
    let pre = cx.to.as_deref().map_or(cx.pre, |to| tr.send(to).map_or(cx.pre, |s| s.pre));
    let (mut view, mut set_pre) = (None, None);
    super::super::chrome::with_menu_style(&ui.ctx().clone(), &t, || {
        #[allow(deprecated)]
        popup::popup_below_widget(ui, pid, &resp, PopupCloseBehavior::CloseOnClick, |ui| {
            ui.set_min_width(160.0);
            for b in a.buses.iter().chain(&a.fx) {
                if crate::ui::menus::entry(ui, &b.name.replace('&', "&&"), "", true, crate::ui::menus::Mark::Radio(cx.to.as_ref() == Some(&b.id))) {
                    view = Some(b.id.clone());
                }
            }
            crate::ui::menus::sep(ui);
            for (label, on) in [("Pre Volume", true), ("Post Volume", false)] {
                if crate::ui::menus::entry(ui, label, "", cx.to.is_some(), crate::ui::menus::Mark::Radio(pre == on)) {
                    set_pre = Some(on);
                }
            }
        });
    });
    if let Some(v) = view {
        project.timeline.audio_tracks[row.idx].send_view = Some(v); // a view choice: no undo step
    }
    if let (Some(on), Some(to)) = (set_pre, &cx.to) {
        let before = project.clone();
        if edit::set_send_pre(&mut project.timeline.audio_tracks[row.idx], to, on) {
            history.push(before);
            project.mark_modified();
        }
    }
}

/// The 2 px accent insertion line of a reorder drag, across the headers and lanes.
pub(super) fn paint_reorder(p: &Painter, g: &Geo, rows: &[Row], st: &TlState) {
    let Some((v, to)) = st.track_drag.as_ref().and_then(|d| Some((d.is_video, d.to?))) else { return };
    let kind: Vec<&Row> = rows.iter().filter(|r| r.is_video == v).collect();
    let Some(y) = kind.iter().find(|r| r.idx == to).map(|r| r.top).or_else(|| kind.last().map(|r| r.top + r.h)) else { return };
    let x = Rangef::new(g.rect.min.x, g.lanes.max.x);
    let r = Rect::from_x_y_ranges(x, Rangef::new(y.round() - 1.0, y.round() + 1.0)).intersect(Rect::from_x_y_ranges(x, Rangef::new(g.list.min.y, g.list.max.y)));
    p.rect_filled(r, 0.0, theme::get().accent);
}

/// Paint every visible header, the empty area below them and the separators.
pub(super) fn paint(p: &Painter, g: &Geo, rows: &[Row], heads: &[Head], project: &Project, sel: &Selection) {
    let t = theme::get();
    let v16 = t.look() == UiLook::Vegas16;
    let p = p.with_clip_rect(g.list.intersect(p.clip_rect()));
    for row in rows {
        if row.top > g.list.max.y || row.top + row.h < g.list.min.y {
            continue;
        }
        let tr = &project.timeline.tracks(row.is_video)[row.idx];
        let l = layout(&t, g, row, tr.armed, project.timeline.audio != AudioSettings::default(), meter_opts(p.ctx()));
        let cx = send_cx(&project.timeline, tr);
        let color = t.track_color(row.num);
        let selected = sel.tracks.contains(&tr.id);

        // Colour strip: ≡ / ⋯, number, type badge.
        p.rect_filled(l.strip, 0.0, color);
        p.rect_filled(Rect::from_x_y_ranges(Rangef::new(l.strip.max.x, l.strip.max.x + 1.0), l.rect.y_range()), 0.0, t.strip_divider);
        if l.strip.width() >= 19.0 {
            paint_icon(&p, l.more, Icon::More, t.look(), (!v16).then_some(t.text_bright));
            let num = (row.num + 1).to_string();
            let font = if t.font.track_number_bold { t.bold(t.font.track_number) } else { t.font(t.font.track_number) };
            let galley = p.layout_no_wrap(num, font, t.text_bright);
            let size = galley.size();
            let top = if v16 { l.rect.min.y + 20.0 } else { l.rect.center().y - size.y * 0.5 };
            if top + size.y <= l.rect.max.y + 2.0 {
                let pos = pos2(l.rect.min.x + 4.0, top);
                p.galley(pos, galley, t.text_bright);
                // Subscript: below-right of the number, its top at the digit's lower third.
                let badge = Rect::from_min_size(pos2(pos.x + size.x, pos.y + size.y * 0.55), Vec2::splat(if v16 { 12.0 } else { 10.0 }));
                let icon = if row.is_video { Icon::TrackVideo } else { Icon::TrackAudio };
                paint_icon(&p, badge, icon, t.look(), Some(t.text_bright));
            }
        }
        if l.body.width() <= 1.0 {
            continue;
        }

        // Body.
        p.rect_filled(l.body, 0.0, if selected { t.track_header_sel_bg } else { t.track_header_bg });
        if selected && v16 {
            let line = |y: f32, c| p.rect_filled(Rect::from_x_y_ranges(l.body.x_range(), Rangef::new(y, y + 1.0)), 0.0, c);
            line(l.body.min.y, t.track_header_sel_top);
            line(l.body.max.y - 1.0, t.track_header_sel_bottom);
        }
        // A compositing child: a bar down its left edge ties it to the parent above.
        // shortcut: VEGAS indents a child's whole header; a marker bar only, until the header layout takes an indent.
        if row.is_video && tr.child && row.idx > 0 {
            p.rect_filled(Rect::from_min_size(l.body.min, vec2(3.0, l.body.height())), 0.0, t.accent);
        }
        // No track name: the scribble strip is off by default in both looks.
        let font = t.font(t.font.ui);
        for (b, r) in &l.buttons {
            paint_button(&p, &t, *b, *r, tr, project.timeline.cursor_position);
        }
        for (s, r, groove) in &l.sliders {
            let cy = r.center().y.round();
            let lr = p.text(pos2(l.body.min.x + 6.0, cy), Align2::LEFT_CENTER, s.label(&cx), font.clone(), t.text_label);
            // VP16 puts values in a column at x=48; VP23 sets them right after the label.
            let vx = if v16 { l.body.min.x + 48.0 } else { lr.max.x + 4.0 };
            p.text(pos2(vx, cy), Align2::LEFT_CENTER, s.value(tr, &cx), font.clone(), t.text_value);
            if groove.span() < 20.0 {
                continue;
            }
            if v16 {
                // Recessed groove: dark line over a lighter one.
                p.rect_filled(Rect::from_x_y_ranges(*groove, Rangef::new(cy - 1.0, cy)), 0.0, t.frame);
                p.rect_filled(Rect::from_x_y_ranges(*groove, Rangef::new(cy, cy + 1.0)), 0.0, t.fader_track);
            } else {
                // ~3 px bar with a dark border.
                let bar = Rect::from_x_y_ranges(*groove, Rangef::new(cy - 1.0, cy + 2.0));
                p.rect_filled(bar, 0.0, t.fader_track);
                p.rect_stroke(bar.expand(1.0), 0.0, Stroke::new(1.0_f32, t.frame), StrokeKind::Inside);
            }
            let x = (groove.min + s.pos(tr, &cx) * groove.span()).round();
            if *s == Slider::Pan {
                let (w, h) = if v16 { (20.0, 12.0) } else { (10.0, 8.0) };
                p.rect_filled(Rect::from_min_size(pos2(groove.center().round(), cy - 3.0), vec2(1.0, 7.0)), 0.0, t.fader_track);
                let pts = vec![pos2(x - w * 0.5, cy - h * 0.5), pos2(x + w * 0.5, cy - h * 0.5), pos2(x, cy + h * 0.5)];
                p.add(Shape::convex_polygon(pts, t.fader_thumb, Stroke::new(1.0_f32, t.fader_thumb_bar)));
            } else {
                paint_icon(&p, Rect::from_center_size(pos2(x, cy), vec2(16.0, 16.0)), Icon::FaderThumb, t.look(), None);
            }
        }
        if let Some(m) = l.meter {
            // An armed track meters the recording input; otherwise its playback level (post-fader).
            let input = crate::ui::menus::state(p.ctx()).input_level.filter(|_| tr.armed);
            let lv = crate::ui::mixer::level(p.ctx(), &tr.id);
            if l.hmeter {
                paint_hmeter(&p, &t, m, input, &lv);
            } else {
                paint_meter(&p, &t, m, l.body.min.y + 2.0, input, &lv);
            }
        }
    }
    paint_heads(&p, &t, g, heads, project);
    // Separators (2 px under each track) and the empty area below the last track.
    let last = rows.iter().map(|r| r.top + r.h).chain(heads.iter().map(|h| h.top + GROUP_H)).fold(g.list.min.y, f32::max);
    for row in rows {
        let y = row.top + row.h - 2.0;
        let r = Rect::from_x_y_ranges(Rangef::new(g.rect.min.x, g.rect.min.x + g.header_w), Rangef::new(y, y + 1.0));
        p.rect_filled(r, 0.0, t.track_separator);
        p.rect_filled(r.translate(vec2(0.0, 1.0)), 0.0, t.track_separator_2);
    }
    let empty = Rect::from_min_max(pos2(g.rect.min.x, last.max(g.list.min.y)), pos2(g.rect.min.x + g.header_w, g.list.max.y));
    p.rect_filled(empty, 0.0, if v16 { t.frame } else { t.empty_lane_bg });
}

/// `at`: the cursor time, at which an audio track's FX count as present when some plug-in changes the sound (every
/// audio track carries the neutral gate, EQ and compressor).
fn paint_button(p: &Painter, t: &Theme, b: Btn, r: Rect, tr: &Track, at: f64) {
    let v16 = t.look() == UiLook::Vegas16;
    let (icon, on) = match b {
        Btn::Arm => (Icon::Record, true),
        Btn::Comp => (Icon::CompositingMode, false),
        Btn::Fx if matches!(tr.kind, TrackKind::Audio) => (Icon::TrackFx, crate::engine::afx::audible(&tr.fx, at)),
        Btn::Fx => (Icon::TrackFx, !tr.fx.is_empty()),
        Btn::Mute => (Icon::Mute, tr.muted),
        Btn::Solo => (Icon::Solo, tr.solo),
    };
    if b == Btn::Fx && !v16 {
        // VP23: a 4 px accent dot at the lower right when FX are present, not a toggle fill (§5.9.3).
        paint_icon(p, r, icon, t.look(), None);
        if on {
            p.circle_filled(r.right_bottom() - vec2(3.0, 3.0), 2.0, t.accent);
        }
    } else {
        paint_toggle(p, t, r, icon, on);
    }
}

/// A header toggle button (Mute, Solo...), lit when `on`.
fn paint_toggle(p: &Painter, t: &Theme, r: Rect, icon: Icon, on: bool) {
    if t.look() == UiLook::Vegas16 {
        paint_icon(p, r, icon, t.look(), None);
        if on {
            p.circle_stroke(r.center(), 8.0, Stroke::new(1.0_f32, t.text_bright));
        }
    } else if on {
        p.rect_filled(r, CornerRadius::same(2), t.accent.gamma_multiply(0.3));
        paint_icon(p, r, icon, t.look(), Some(t.accent));
    } else {
        paint_icon(p, r, icon, t.look(), None);
    }
}

/// Level meter with dB labels: the track's playback level `lv` (L and R bars, peak holds, clip lamps), or lit to
/// `input` (a linear peak) on an armed track. VP16 shows the peak readout above it.
fn paint_meter(p: &Painter, t: &Theme, m: Rect, top: f32, input: Option<f32>, lv: &crate::ui::mixer::MeterView) {
    let v16 = t.look() == UiLook::Vegas16;
    let to_y = |d: f32| m.min.y + m.height() * (-d / 60.0).clamp(0.0, 1.0);
    crate::ui::mixer::paint_well(p, m);
    match input {
        Some(v) if v > 1e-3 => crate::ui::master_bus::paint_bar(p, m, to_y(20.0 * v.log10()), &to_y, v16, 1.0),
        Some(_) => {}
        None => crate::ui::mixer::paint_levels(p, m, lv, v16),
    }
    let font = t.font(t.font.meter);
    let labels: &[i32] = if v16 { &[12, 24, 36, 48] } else { &[9, 18, 27, 36, 45, 54] };
    // Every other label when they would crowd (short tracks).
    let every = if m.height() * (labels[1] - labels[0]) as f32 / 60.0 < 10.0 { 2 } else { 1 };
    for db in labels.iter().skip(every - 1).step_by(every) {
        let y = m.min.y + m.height() * (*db as f32 / 60.0);
        if y < m.max.y - 4.0 {
            p.text(pos2(m.center().x, y), Align2::CENTER_CENTER, db.to_string(), font.clone(), t.meter_label);
        }
    }
    if v16 && m.min.y - top >= 10.0 {
        let peak = if input.is_some() { "-Inf.".to_string() } else { lv.peak_text() };
        p.text(pos2(m.max.x, top), Align2::RIGHT_TOP, peak, font, if lv.clip.iter().any(|c| *c) && input.is_none() { t.meter_red } else { t.text_secondary });
    }
}

/// R8b: the horizontal meter (Use Vertical Meters off): L over R, -60 dB at the left, yellow above -6 dB, a red clip
/// lamp at the right end.
fn paint_hmeter(p: &Painter, t: &Theme, m: Rect, input: Option<f32>, lv: &crate::ui::mixer::MeterView) {
    let th = ((m.height() - 1.0) * 0.5).floor().max(1.0);
    let to_x = |d: f32| m.min.x + m.width() * (1.0 + d / 60.0).clamp(0.0, 1.0);
    let x6 = to_x(-6.0);
    for c in 0..2 {
        let y0 = m.min.y + c as f32 * (th + 1.0);
        let y = Rangef::new(y0, y0 + th);
        p.rect_filled(Rect::from_x_y_ranges(m.x_range(), y), 0.0, t.meter_bg);
        let d = input.map_or(lv.level[c], crate::ui::mixer::db);
        if d > -60.0 {
            let x = to_x(d.min(0.0));
            p.rect_filled(Rect::from_x_y_ranges(Rangef::new(m.min.x, x.min(x6)), y), 0.0, t.meter_green);
            if x > x6 {
                p.rect_filled(Rect::from_x_y_ranges(Rangef::new(x6, x), y), 0.0, t.meter_yellow);
            }
        }
        let h = lv.hold[c].0;
        if input.is_none() && h > -60.0 {
            let x = to_x(h.min(0.0)).round();
            p.rect_filled(Rect::from_x_y_ranges(Rangef::new(x - 1.0, x), y), 0.0, if h >= -6.0 { t.meter_yellow } else { t.meter_green });
        }
        if input.is_none() && lv.clip[c] {
            p.rect_filled(Rect::from_x_y_ranges(Rangef::new(m.max.x - 4.0, m.max.x), y), 0.0, t.meter_red);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fader_law_round_trips() {
        assert_eq!(fader_gain(0.0), 0.0);
        assert!((fader_gain(0.7) - 1.0).abs() < 1e-6);
        assert!((20.0 * fader_gain(1.0).log10() - 12.0).abs() < 1e-4);
        for g in [0.0, 0.05, 0.5, 1.0, 2.0, 3.98] {
            assert!((fader_gain(fader_pos(g)) - g).abs() < 1e-3, "gain {g}");
        }
        assert_eq!(db_text(1.0), "0.0 dB");
        assert_eq!(db_text(0.0), "-Inf. dB");
        assert_eq!((pan_text(0.0), pan_text(-0.35), pan_text(0.35)), ("Center".into(), "L 35%".into(), "R 35%".into()));
    }
}
