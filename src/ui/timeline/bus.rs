//! Bus tracks, docked under the tracks as in VEGAS: the Video Bus Track (View › Video Bus Track, Ctrl+Shift+B) and,
//! R8b, one row per audio bus and assignable FX chain (View › Audio Bus Tracks, B).
//! The Video Bus Track's header opens the Video Output FX; its lane holds the Fade to Color, Motion Blur Amount and Video
//! Supersampling envelopes. An audio bus row's header has its FX button, Mute, Vol and Pan sliders and its meter; its
//! lane holds its Volume, Pan and Mute envelopes. Right-click › Insert/Remove Envelope; points drag (Ctrl: fine), a
//! double-click on a line adds one, and a point's menu deletes it. Each drag, click or menu edit is one undo step.
use eframe::egui::*;
use crate::edit;
use crate::history::History;
use crate::project::{Bus, EnvKind, Envelope, Project};
use crate::settings::EnvView;
use super::super::icons::Icon;
use super::super::menus::{self, Mark, entry};
use super::super::mixer;
use super::super::theme::{self, Theme};
use super::{Geo, envelopes, fader_gain, fader_pos};

/// The Video Bus Track's height, and an audio bus track's.
pub const BUS_H: f32 = 48.0;
pub const AUDIO_BUS_H: f32 = 64.0;

/// The envelopes each kind of bus track takes, as its menu lists them.
const VIDEO_KINDS: [(&str, EnvKind); 3] = [("&Fade to Color", EnvKind::FadeToColor), ("Motion &Blur Amount", EnvKind::MotionBlur), ("Video &Supersampling", EnvKind::Supersampling)];
const AUDIO_KINDS: [(&str, EnvKind); 3] = [("&Volume", EnvKind::Volume), ("&Pan", EnvKind::Pan), ("&Mute", EnvKind::Mute)];

/// The bus rows shown, top down: the Video Bus Track (None), then each audio bus and assignable FX chain (its id).
pub(super) fn rows(project: &Project, video: bool, audio: bool) -> Vec<Option<String>> {
    let a = &project.timeline.audio;
    let buses = a.buses.iter().chain(&a.fx).map(|b| Some(b.id.clone())).filter(|_| audio);
    video.then_some(None).into_iter().chain(buses).collect()
}

pub(super) fn row_h(row: &Option<String>) -> f32 {
    if row.is_some() { AUDIO_BUS_H } else { BUS_H }
}

#[derive(Clone)]
struct Drag {
    /// The envelope's owner (`shown`) and kind.
    owner: String,
    kind: EnvKind,
    key: usize,
    orig: Envelope,
    press: Pos2,
}

fn color(t: &Theme, kind: EnvKind) -> Color32 {
    // VP16 default envelope colours (spec §2).
    match kind {
        EnvKind::MotionBlur => Color32::from_rgb(0xBF, 0x7F, 0xD3),
        EnvKind::Supersampling => Color32::from_rgb(0xB8, 0x69, 0x3C),
        EnvKind::Volume => t.envelope_volume,
        EnvKind::Pan => t.envelope_pan,
        EnvKind::Mute => t.envelope_mute,
        EnvKind::FxParam => envelopes::FX_PARAM,
        _ => Color32::from_rgb(0xFF, 0x45, 0x3F),
    }
}

/// The lane's y for value `v` of `kind`.
fn y_of(lane: Rect, kind: EnvKind, v: f32) -> f32 {
    let (top, bottom) = (lane.min.y + 2.0, lane.max.y - 4.0);
    bottom - edit::lane_frac(kind, v) * (bottom - top)
}

fn bus<'a>(project: &'a Project, id: &str) -> Option<&'a Bus> {
    project.timeline.audio.bus(id)
}

/// The envelopes of bus track `owner` (None: the Video Bus Track).
fn envs<'a>(project: &'a Project, owner: Option<&str>) -> &'a [Envelope] {
    match owner {
        None => &project.timeline.master.video_envelopes,
        Some(id) => bus(project, id).map_or(&[], |b| &b.envelopes),
    }
}

/// The shown envelopes (View › Audio / Video Envelopes can hide kinds) with points, each with its owner: the bus's own
/// (owner `""` on the Video Bus Track) and, R8c, an audio bus's FX envelopes (`edit::FX_ENV` owners).
fn shown(project: &Project, owner: Option<&str>, hidden: u16) -> Vec<(String, Envelope)> {
    let visible = |k| EnvView::of(k, owner.is_none()).is_none_or(|v| hidden & v.bit() == 0);
    let own = envs(project, owner).iter().filter(|e| !e.points.keys.is_empty() && visible(e.kind)).map(|e| (owner.unwrap_or_default().to_string(), e.clone()));
    let fx = owner.and_then(|id| bus(project, id)).map_or(Vec::new(), |b| edit::chain_envelopes(&b.id, &b.fx, false));
    own.chain(fx).collect()
}

/// What the pointer is on: (owner, kind, Some(point) or None for the line).
type Hit = (String, EnvKind, Option<usize>);

fn hit(g: &Geo, lane: Rect, project: &Project, owner: Option<&str>, hidden: u16, p: Pos2) -> Option<Hit> {
    let envs = shown(project, owner, hidden);
    for (o, e) in envs.iter().rev() {
        if let Some(k) = e.points.keys.iter().position(|k| (p.x - g.to_x(k.t)).abs() <= 4.0 && (p.y - y_of(lane, e.kind, k.v)).abs() <= 4.0) {
            return Some((o.clone(), e.kind, Some(k)));
        }
    }
    let t = g.to_time(p.x);
    envs.iter().rev().find(|(_, e)| (p.y - y_of(lane, e.kind, e.at(t))).abs() <= 3.0).map(|(o, e)| (o.clone(), e.kind, None))
}

/// Edit envelope `kind` of `env_owner` on bus track `owner` (its own, or one of its FX envelopes).
fn edit_env(project: &mut Project, owner: Option<&str>, env_owner: &str, kind: EnvKind, f: impl FnOnce(&mut Envelope) -> bool) -> bool {
    if kind == EnvKind::FxParam {
        let done = edit::with_envelope(project, env_owner, kind, f);
        if done {
            project.mark_modified();
        }
        return done;
    }
    let a = &mut project.timeline.audio;
    let envs = match owner {
        None => Some(&mut project.timeline.master.video_envelopes),
        Some(id) => a.buses.iter_mut().chain(&mut a.fx).find(|b| b.id == id).map(|b| &mut b.envelopes),
    };
    let done = envs.and_then(|v| v.iter_mut().find(|e| e.kind == kind)).is_some_and(f);
    if done {
        project.mark_modified();
    }
    done
}

/// Insert/Remove Envelope ▸ items, each checked while the bus has it.
fn env_items(ui: &mut Ui, project: &mut Project, history: &mut History, owner: Option<&str>) {
    for (label, kind) in if owner.is_none() { VIDEO_KINDS } else { AUDIO_KINDS } {
        let has = envs(project, owner).iter().any(|e| e.kind == kind);
        if entry(ui, label, "", true, Mark::Check(has)) {
            history.push(project.clone());
            edit::toggle_bus_envelope(project, owner, kind);
        }
    }
    // R8c: an audio bus's FX automation envelopes, through the chooser.
    if let Some(id) = owner {
        menus::sep(ui);
        if entry(ui, "F&X Automation Envelopes...", "", true, Mark::None) {
            crate::ui::automation::open_chooser(ui.ctx(), id);
        }
    }
}

/// The bus rows `rows` in the strip of height `h` under `g`'s lanes: header (`g.list`'s x) and lane (`g.lanes`'s x).
// shortcut: rows past `h` are cut off (no scrolling of the bus-track area); add a scrollbar if many busses are common.
pub(super) fn show(ui: &mut Ui, g: &Geo, project: &mut Project, history: &mut History, hidden: u16, rows: &[Option<String>], h: f32) {
    let area = Rect::from_x_y_ranges(Rangef::new(g.rect.min.x, g.lanes.max.x), Rangef::new(g.lanes.max.y, g.lanes.max.y + h));
    let mut y = area.min.y;
    for row in rows {
        let rh = row_h(row);
        if y + rh > area.max.y + 0.5 {
            break; // only whole rows, so no control sits outside the strip
        }
        show_row(ui, g, project, history, hidden, row.as_deref(), Rangef::new(y, y + rh), area);
        y += rh;
    }
}

#[allow(clippy::too_many_arguments)]
fn show_row(ui: &mut Ui, g: &Geo, project: &mut Project, history: &mut History, hidden: u16, owner: Option<&str>, y: Rangef, area: Rect) {
    let t = theme::get();
    let head = Rect::from_x_y_ranges(Rangef::new(g.rect.min.x, g.rect.min.x + g.header_w), y);
    let lane = Rect::from_x_y_ranges(g.lanes.x_range(), y);
    let id = ui.id().with(("bus_track", owner));
    let Some(name) = (match owner {
        None => Some("Video Bus".to_string()),
        Some(b) => bus(project, b).map(|b| b.name.clone()),
    }) else {
        return;
    };
    let p = ui.painter_at(head.union(lane).intersect(area));
    p.rect_filled(head, 0.0, t.track_header_bg);
    p.rect_filled(lane, 0.0, t.lane_bg);
    p.hline(head.min.x..=lane.max.x, y.min, Stroke::new(1.0_f32, t.track_separator));

    // Header: the name and the FX button (audio busses: Mute, Vol, Pan and the meter too); right-click inserts or
    // removes envelopes.
    let hr = ui.interact(head.intersect(area), id.with("head"), Sense::click());
    let fx = Rect::from_min_size(pos2(head.max.x - 30.0, head.min.y + 6.0), vec2(24.0, 18.0));
    let (target, has_fx) = match owner {
        None => (edit::FxTarget::VideoOutput, !project.timeline.master.video_fx.is_empty()),
        Some(b) => (edit::FxTarget::Bus(b.to_string()), bus(project, b).is_some_and(|b| !b.fx.is_empty())),
    };
    let fr = ui.interact(fx, id.with("fx"), Sense::click()).on_hover_text(if owner.is_none() { "Video Output FX" } else { "Bus FX" });
    if fr.clicked() {
        crate::ui::open_tool(ui.ctx(), crate::ui::ToolWin::Fx(target));
    }
    menus::context(&hr, |ui| {
        menus::sub(ui, "Insert/Remove &Envelope", true, |ui| env_items(ui, project, history, owner));
        // R8c: an audio bus's Automation Settings, as a track's.
        if let Some(id) = owner {
            crate::ui::automation::menu(ui, project, history, &[id.to_string()]);
        }
    });
    p.text(head.left_top() + vec2(8.0, 15.0), Align2::LEFT_CENTER, &name, t.bold(t.font.ui), t.text_bright);
    p.rect_filled(fx, 2.0, if fr.hovered() { t.button_hover_bg } else { t.toolbar_bg });
    p.text(fx.center(), Align2::CENTER_CENTER, "fx", t.font(t.font.ui), if has_fx { t.accent } else { t.text });
    if let Some(bid) = owner {
        bus_controls(ui, &p, id, project, history, bid, head, fx);
    }

    // Lane: point drags, double-click adds a point, right-click menu.
    let lr = ui.interact(lane.intersect(area), id.with("lane"), Sense::click_and_drag());
    let sid = id.with("drag");
    let mut drag: Option<Drag> = ui.data(|d| d.get_temp(sid));
    let hover = lr.hover_pos().and_then(|p| hit(g, lane, project, owner, hidden, p));
    if lr.drag_started_by(PointerButton::Primary) {
        let press = ui.input(|i| i.pointer.press_origin());
        drag = press.and_then(|p| {
            let (o, kind, key) = hit(g, lane, project, owner, hidden, p)?;
            let orig = shown(project, owner, hidden).into_iter().find(|e| e.0 == o && e.1.kind == kind)?.1;
            Some(Drag { owner: o, kind, key: key?, orig, press: p })
        });
    }
    if let (Some(d), Some(p)) = (&drag, lr.interact_pointer_pos().filter(|_| lr.dragged())) {
        let fine = if ui.input(|i| i.modifiers.command) { 0.1 } else { 1.0 };
        let df = -(p.y - d.press.y) / (lane.height() - 6.0).max(1.0) * fine;
        let dt = if fine < 1.0 { 0.0 } else { (p.x - d.press.x) as f64 / g.pps };
        let (orig, key) = (d.orig.clone(), d.key);
        edit_env(project, owner, &d.owner, d.kind, |e| edit::env_move(e, &orig, &[key], dt, df));
    }
    if lr.drag_stopped() {
        if let Some(d) = drag.take() {
            // One undo step: the project as it was, the envelope back as the drag found it.
            let mut before = project.clone();
            if edit_env(&mut before, owner, &d.owner, d.kind, |e| {
                let moved = *e != d.orig;
                *e = d.orig.clone();
                moved
            }) {
                history.push(before);
            }
        }
    }
    if lr.double_clicked() {
        if let (Some(p), Some((o, kind, None))) = (lr.interact_pointer_pos(), hover.clone()) {
            let before = project.clone();
            let t0 = g.to_time(p.x).max(0.0);
            if edit_env(project, owner, &o, kind, |e| {
                edit::env_insert(e, t0, e.at(t0));
                true
            }) {
                history.push(before);
            }
        }
    }
    if lr.secondary_clicked() {
        ui.data_mut(|d| match hover.clone() {
            Some(h) => d.insert_temp(id.with("menu"), h),
            None => d.remove::<Hit>(id.with("menu")),
        });
    }
    let menu_at = ui.data(|d| d.get_temp::<Hit>(id.with("menu")));
    menus::context(&lr, |ui| {
        if let Some((o, kind, Some(k))) = menu_at {
            if entry(ui, "&Delete", "", k > 0, Mark::None) {
                let before = project.clone();
                if edit_env(project, owner, &o, kind, |e| edit::env_delete(e, &[k])) {
                    history.push(before);
                }
            }
            menus::sep(ui);
        }
        menus::sub(ui, "Insert/Remove &Envelope", true, |ui| env_items(ui, project, history, owner));
    });
    if drag.is_some() || hover.is_some() {
        ui.ctx().set_cursor_icon(if drag.is_some() || hover.as_ref().is_some_and(|h| h.2.is_some()) { CursorIcon::PointingHand } else { CursorIcon::ResizeVertical });
    }
    ui.data_mut(|d| match &drag {
        Some(dr) => d.insert_temp(sid, dr.clone()),
        None => d.remove::<Drag>(sid),
    });

    // Paint the envelopes.
    let pl = p.with_clip_rect(lane.intersect(area));
    let all = shown(project, owner, hidden);
    for (o, e) in &all {
        let c = color(&t, e.kind);
        let mut xs: Vec<f32> = (0..).map(|k| lane.min.x + 2.0 * k as f32).take_while(|x| *x < lane.max.x).chain([lane.max.x]).collect();
        xs.extend(e.points.keys.iter().map(|k| g.to_x(k.t)).filter(|x| *x > lane.min.x && *x < lane.max.x));
        xs.sort_by(f32::total_cmp);
        let hovered = hover.as_ref().is_some_and(|h| (&h.0, h.1) == (o, e.kind)) || drag.as_ref().is_some_and(|d| (&d.owner, d.kind) == (o, e.kind));
        pl.add(Shape::line(xs.iter().map(|&x| pos2(x, y_of(lane, e.kind, e.at(g.to_time(x))))).collect(), Stroke::new(if hovered { 2.0_f32 } else { 1.0 }, c)));
        for k in &e.points.keys {
            let r = Rect::from_center_size(pos2(g.to_x(k.t).round(), y_of(lane, e.kind, k.v).round()), vec2(5.0, 5.0));
            pl.rect_filled(r, 0.0, c);
            pl.rect_stroke(r, 0.0, Stroke::new(1.0_f32, t.envelope_point), StrokeKind::Outside);
        }
    }
    // The value of the point dragged or hovered.
    let tip = drag.as_ref().map(|d| (d.owner.clone(), d.kind, d.key)).or_else(|| hover.and_then(|h| Some((h.0, h.1, h.2?))));
    if let Some((o, kind, k)) = tip {
        if let Some(key) = all.iter().find(|e| e.0 == o && e.1.kind == kind).and_then(|e| e.1.points.keys.get(k)) {
            super::paint_fade_tag(&pl, &t, pos2(g.to_x(key.t) + 6.0, y_of(lane, kind, key.v) - 18.0), envelopes::value_label(project, &o, kind, key.v));
        }
    }
}

/// An audio bus track's Mute, Vol and Pan (one undo step per click, drag, wheel step or double-click reset) and its
/// meter at the right edge.
#[allow(clippy::too_many_arguments)]
fn bus_controls(ui: &mut Ui, p: &Painter, id: Id, project: &mut Project, history: &mut History, bid: &str, head: Rect, fx: Rect) {
    let t = theme::get();
    let Some((muted, volume, pan_v)) = bus(project, bid).map(|b| (b.muted, b.volume, b.pan)) else { return };
    let mute = Rect::from_min_size(pos2(fx.min.x - 24.0, fx.min.y + 1.0), vec2(18.0, 16.0));
    if mute.min.x > head.min.x + 70.0 && mixer::toggle(ui, p, mute, id.with("mute"), Some(Icon::Mute), "", muted, "Mute") {
        history.push(project.clone());
        if let Some(b) = bus_mut(project, bid) {
            b.muted = !b.muted;
        }
        project.mark_modified();
    }
    let meter = Rect::from_min_max(pos2(head.max.x - 14.0, head.min.y + 28.0), pos2(head.max.x - 4.0, head.max.y - 4.0));
    mixer::paint_well(p, meter);
    mixer::paint_levels(p, meter, &mixer::level(ui.ctx(), bid), t.look() == theme::UiLook::Vegas16);
    let font = t.font(t.font.ui);
    for (k, pan) in [(0, false), (1, true)] {
        let cy = head.min.y + 35.0 + 17.0 * k as f32;
        if cy + 6.0 > head.max.y {
            break;
        }
        let groove = Rangef::new(head.min.x + 100.0, meter.min.x - 10.0);
        // R8c: with Show Automation Controls, Vol and Pan show (and write) the bus's envelopes.
        let kind = if pan { EnvKind::Pan } else { EnvKind::Volume };
        let auto = crate::ui::automation::control_mode(project, bid).map(|m| (m, crate::ui::automation::value(ui.ctx(), project, bid, kind)));
        let (pan_v, volume) = match auto {
            Some((_, v)) if pan => (v, volume),
            Some((_, v)) => (pan_v, crate::ui::automation::gain_of(v)),
            None => (pan_v, volume),
        };
        let (label, value, pos, reset) = if pan {
            ("Pan:", mixer::pan_text(pan_v), (pan_v.clamp(-1.0, 1.0) + 1.0) * 0.5, 0.5)
        } else {
            ("Vol:", format!("{} dB", mixer::gain_text(volume)), fader_pos(volume), fader_pos(1.0))
        };
        let lr = p.text(pos2(head.min.x + 8.0, cy), Align2::LEFT_CENTER, label, font.clone(), t.text_label);
        p.text(pos2(lr.max.x + 4.0, cy), Align2::LEFT_CENTER, &value, font.clone(), t.text_value);
        if groove.span() < 20.0 {
            continue;
        }
        p.rect_filled(Rect::from_x_y_ranges(groove, Rangef::new(cy - 1.0, cy + 1.0)), 0.0, t.fader_track);
        let x = (groove.min + pos * groove.span()).round();
        let thumb = Rect::from_center_size(pos2(x, cy), vec2(16.0, 16.0));
        let tint = auto.map(|_| color(&t, kind));
        super::super::icons::paint_icon(p, thumb, if pan { Icon::PanThumb } else { Icon::FaderThumb }, t.look(), tint);
        let hit = Rect::from_x_y_ranges(Rangef::new(groove.min - 8.0, groove.max + 8.0), Rangef::new(cy - 7.0, cy + 7.0));
        let resp = ui.interact(hit, id.with(("slider", k)), Sense::click_and_drag()).on_hover_text(value);
        let span = groove.span();
        if let Some((mode, _)) = auto {
            let to_v = |np: f32| if pan { np * 2.0 - 1.0 } else { crate::ui::automation::db_of(fader_gain(np)) };
            crate::ui::automation::slider(ui, &resp, project, history, bid, kind, mode, pos, reset, |q| (q.x - groove.min) / span, |d| d.x / span, to_v);
            continue;
        }
        if let Some((undo, np)) = mixer::slider(ui, &resp, pos, reset, |q| (q.x - groove.min) / span, |d| d.x / span) {
            if undo {
                history.push(project.clone());
            }
            if let Some(b) = bus_mut(project, bid) {
                let np = np.clamp(0.0, 1.0);
                if pan { b.pan = np * 2.0 - 1.0 } else { b.volume = fader_gain(np) }
            }
            project.mark_modified();
        }
    }
}

fn bus_mut<'a>(project: &'a mut Project, id: &str) -> Option<&'a mut Bus> {
    let a = &mut project.timeline.audio;
    a.buses.iter_mut().chain(&mut a.fx).find(|b| b.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bus_rows_follow_the_view_switches() {
        let mut p = Project::new("t");
        assert!(rows(&p, false, true).is_empty());
        edit::set_bus_count(&mut p, 2);
        let fx = edit::insert_assignable_fx(&mut p, Vec::new()).unwrap();
        let a = &p.timeline.audio;
        assert_eq!(rows(&p, true, true), vec![None, Some(a.buses[0].id.clone()), Some(a.buses[1].id.clone()), Some(fx)]);
        assert_eq!(rows(&p, true, false), vec![None]);
        assert_eq!(rows(&p, true, true).iter().map(row_h).sum::<f32>(), BUS_H + 3.0 * AUDIO_BUS_H);
    }
}
