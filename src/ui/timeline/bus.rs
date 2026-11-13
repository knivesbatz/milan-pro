//! The Video Bus Track (View › Video Bus Track, Ctrl+Shift+B): a row docked under the tracks, as VEGAS's bus tracks
//! are, for the video output. Its header opens the Video Output FX; its lane holds the Fade to Color, Motion Blur
//! Amount and Video Supersampling envelopes (right-click › Insert/Remove Envelope). Points drag (Ctrl: fine), a
//! double-click on a line adds one, and a point's menu deletes it. Each drag or menu edit is one undo step.
use eframe::egui::*;
use crate::edit;
use crate::history::History;
use crate::project::{EnvKind, Envelope, Project};
use crate::settings::EnvView;
use super::super::menus::{self, Mark, entry};
use super::super::theme;
use super::{Geo, envelopes};

/// The row's height.
pub const BUS_H: f32 = 48.0;

/// The envelopes the bus track takes, as its menu lists them.
pub const KINDS: [(&str, EnvKind); 3] = [("&Fade to Color", EnvKind::FadeToColor), ("Motion &Blur Amount", EnvKind::MotionBlur), ("Video &Supersampling", EnvKind::Supersampling)];

#[derive(Clone)]
struct Drag {
    kind: EnvKind,
    key: usize,
    orig: Envelope,
    press: Pos2,
}

fn color(kind: EnvKind) -> Color32 {
    // VP16 default envelope colours (spec §2).
    match kind {
        EnvKind::MotionBlur => Color32::from_rgb(0xBF, 0x7F, 0xD3),
        EnvKind::Supersampling => Color32::from_rgb(0xB8, 0x69, 0x3C),
        _ => Color32::from_rgb(0xFF, 0x45, 0x3F),
    }
}

/// The lane's y for value `v` of `kind`.
fn y_of(lane: Rect, kind: EnvKind, v: f32) -> f32 {
    let (top, bottom) = (lane.min.y + 2.0, lane.max.y - 4.0);
    bottom - edit::lane_frac(kind, v) * (bottom - top)
}

/// The shown envelopes (View › Video Envelopes can hide Motion Blur and Supersampling) with points.
fn shown(project: &Project, hidden: u16) -> Vec<&Envelope> {
    let visible = |k| EnvView::of(k, true).is_none_or(|v| hidden & v.bit() == 0);
    project.timeline.master.video_envelopes.iter().filter(|e| !e.points.keys.is_empty() && visible(e.kind)).collect()
}

/// What the pointer is on: (kind, Some(point) or None for the line).
fn hit(g: &Geo, lane: Rect, project: &Project, hidden: u16, p: Pos2) -> Option<(EnvKind, Option<usize>)> {
    let envs = shown(project, hidden);
    for e in envs.iter().rev() {
        if let Some(k) = e.points.keys.iter().position(|k| (p.x - g.to_x(k.t)).abs() <= 4.0 && (p.y - y_of(lane, e.kind, k.v)).abs() <= 4.0) {
            return Some((e.kind, Some(k)));
        }
    }
    let t = g.to_time(p.x);
    envs.iter().rev().find(|e| (p.y - y_of(lane, e.kind, e.at(t))).abs() <= 3.0).map(|e| (e.kind, None))
}

fn edit_env(project: &mut Project, kind: EnvKind, f: impl FnOnce(&mut Envelope) -> bool) -> bool {
    let done = project.timeline.master.video_envelopes.iter_mut().find(|e| e.kind == kind).is_some_and(f);
    if done {
        project.mark_modified();
    }
    done
}

/// Insert/Remove Envelope ▸ items, each checked while the bus has it.
fn env_items(ui: &mut Ui, project: &mut Project, history: &mut History) {
    for (label, kind) in KINDS {
        let has = project.timeline.master.video_envelopes.iter().any(|e| e.kind == kind);
        if entry(ui, label, "", true, Mark::Check(has)) {
            history.push(project.clone());
            edit::toggle_bus_envelope(project, kind);
        }
    }
}

/// The bus row in `g`'s strip under the lanes: header (`g.list`'s x) and lane (`g.lanes`'s x).
pub(super) fn show(ui: &mut Ui, g: &Geo, project: &mut Project, history: &mut History, hidden: u16) {
    let t = theme::get();
    let y = Rangef::new(g.lanes.max.y, g.lanes.max.y + BUS_H);
    let head = Rect::from_x_y_ranges(Rangef::new(g.rect.min.x, g.rect.min.x + g.header_w), y);
    let lane = Rect::from_x_y_ranges(g.lanes.x_range(), y);
    let id = ui.id().with("video_bus");

    // Header: the name and the Video Output FX button; right-click inserts or removes envelopes.
    let hr = ui.interact(head, id.with("head"), Sense::click());
    let fx = Rect::from_min_size(pos2(head.max.x - 30.0, head.min.y + 6.0), vec2(24.0, 18.0));
    let fr = ui.interact(fx, id.with("fx"), Sense::click()).on_hover_text("Video Output FX");
    if fr.clicked() {
        crate::ui::open_tool(ui.ctx(), crate::ui::ToolWin::Fx(edit::FxTarget::VideoOutput));
    }
    menus::context(&hr, |ui| menus::sub(ui, "Insert/Remove &Envelope", true, |ui| env_items(ui, project, history)));

    // Lane: point drags, double-click adds a point, right-click menu.
    let lr = ui.interact(lane, id.with("lane"), Sense::click_and_drag());
    let sid = id.with("drag");
    let mut drag: Option<Drag> = ui.data(|d| d.get_temp(sid));
    let hover = lr.hover_pos().and_then(|p| hit(g, lane, project, hidden, p));
    if lr.drag_started_by(PointerButton::Primary) {
        let press = ui.input(|i| i.pointer.press_origin());
        drag = press.and_then(|p| {
            let (kind, key) = hit(g, lane, project, hidden, p)?;
            let orig = project.timeline.master.video_envelopes.iter().find(|e| e.kind == kind)?.clone();
            Some(Drag { kind, key: key?, orig, press: p })
        });
    }
    if let (Some(d), Some(p)) = (&drag, lr.interact_pointer_pos().filter(|_| lr.dragged())) {
        let fine = if ui.input(|i| i.modifiers.command) { 0.1 } else { 1.0 };
        let df = -(p.y - d.press.y) / (lane.height() - 6.0).max(1.0) * fine;
        let dt = if fine < 1.0 { 0.0 } else { (p.x - d.press.x) as f64 / g.pps };
        let (orig, key) = (d.orig.clone(), d.key);
        edit_env(project, d.kind, |e| edit::env_move(e, &orig, &[key], dt, df));
    }
    if lr.drag_stopped() {
        if let Some(d) = drag.take() {
            // One undo step: the project as it was, the envelope back as the drag found it.
            let mut before = project.clone();
            if let Some(e) = before.timeline.master.video_envelopes.iter_mut().find(|e| e.kind == d.kind).filter(|e| **e != d.orig) {
                *e = d.orig;
                history.push(before);
            }
        }
    }
    if lr.double_clicked() {
        if let (Some(p), Some((kind, None))) = (lr.interact_pointer_pos(), hover) {
            let before = project.clone();
            let t0 = g.to_time(p.x).max(0.0);
            if edit_env(project, kind, |e| {
                edit::env_insert(e, t0, e.at(t0));
                true
            }) {
                history.push(before);
            }
        }
    }
    if lr.secondary_clicked() {
        ui.data_mut(|d| match hover {
            Some(h) => d.insert_temp(id.with("menu"), h),
            None => d.remove::<(EnvKind, Option<usize>)>(id.with("menu")),
        });
    }
    let menu_at = ui.data(|d| d.get_temp::<(EnvKind, Option<usize>)>(id.with("menu")));
    menus::context(&lr, |ui| {
        if let Some((kind, Some(k))) = menu_at {
            if entry(ui, "&Delete", "", k > 0, Mark::None) {
                let before = project.clone();
                if edit_env(project, kind, |e| edit::env_delete(e, &[k])) {
                    history.push(before);
                }
            }
            menus::sep(ui);
        }
        menus::sub(ui, "Insert/Remove &Envelope", true, |ui| env_items(ui, project, history));
    });
    if drag.is_some() || hover.is_some() {
        ui.ctx().set_cursor_icon(if drag.is_some() || hover.is_some_and(|h| h.1.is_some()) { CursorIcon::PointingHand } else { CursorIcon::ResizeVertical });
    }
    ui.data_mut(|d| match &drag {
        Some(dr) => d.insert_temp(sid, dr.clone()),
        None => d.remove::<Drag>(sid),
    });

    // Paint.
    let p = ui.painter_at(head.union(lane));
    p.rect_filled(head, 0.0, t.track_header_bg);
    p.text(head.left_center() + vec2(8.0, 0.0), Align2::LEFT_CENTER, "Video Bus", t.bold(t.font.ui), t.text_bright);
    p.rect_filled(fx, 2.0, if fr.hovered() { t.button_hover_bg } else { t.toolbar_bg });
    p.text(fx.center(), Align2::CENTER_CENTER, "fx", t.font(t.font.ui), if project.timeline.master.video_fx.is_empty() { t.text } else { t.accent });
    p.rect_filled(lane, 0.0, t.lane_bg);
    p.hline(head.min.x..=lane.max.x, y.min, Stroke::new(1.0_f32, t.track_separator));
    let pl = p.with_clip_rect(lane);
    for e in shown(project, hidden) {
        let c = color(e.kind);
        let mut xs: Vec<f32> = (0..).map(|k| lane.min.x + 2.0 * k as f32).take_while(|x| *x < lane.max.x).chain([lane.max.x]).collect();
        xs.extend(e.points.keys.iter().map(|k| g.to_x(k.t)).filter(|x| *x > lane.min.x && *x < lane.max.x));
        xs.sort_by(f32::total_cmp);
        let hovered = hover.is_some_and(|h| h.0 == e.kind) || drag.as_ref().is_some_and(|d| d.kind == e.kind);
        pl.add(Shape::line(xs.iter().map(|&x| pos2(x, y_of(lane, e.kind, e.at(g.to_time(x))))).collect(), Stroke::new(if hovered { 2.0_f32 } else { 1.0 }, c)));
        for k in &e.points.keys {
            let r = Rect::from_center_size(pos2(g.to_x(k.t).round(), y_of(lane, e.kind, k.v).round()), vec2(5.0, 5.0));
            pl.rect_filled(r, 0.0, c);
            pl.rect_stroke(r, 0.0, Stroke::new(1.0_f32, t.envelope_point), StrokeKind::Outside);
        }
    }
    // The value of the point dragged or hovered.
    let tip = drag.as_ref().map(|d| (d.kind, d.key)).or_else(|| hover.and_then(|h| Some((h.0, h.1?))));
    if let Some((kind, k)) = tip {
        if let Some(key) = project.timeline.master.video_envelopes.iter().find(|e| e.kind == kind).and_then(|e| e.points.keys.get(k)) {
            super::paint_fade_tag(&pl, &t, pos2(g.to_x(key.t) + 6.0, y_of(lane, kind, key.v) - 18.0), envelopes::value_text(kind, key.v));
        }
    }
}
