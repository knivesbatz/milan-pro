//! The keyframe controller (§5.15.2) under Pan/Crop and Track Motion: a label column, a 15 px ruler, the lane with
//! grey diamonds, a white cursor flag, a checker past the end, and the button row (Sync Cursor, First / Previous /
//! Next / Last keyframe, Create, Delete, the cursor time). It reports one intent a frame; `apply` carries it out.
use eframe::egui::*;
use crate::engine::keyframes::Lerp;
use crate::project::{Curve, Keyframe, Keyframes, format_timecode};
use super::menus::{self, Mark, entry};
use super::theme;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum KfResponse {
    /// Move the controller's cursor (and the timeline's, while Sync Cursor is on) to this key time.
    Seek(f64),
    /// A key at the cursor holding the value there.
    Add,
    Delete(usize),
    Move(usize, f64),
    SetCurve(usize, Curve),
}

/// The controller's state in its window.
pub struct KfState {
    /// Sync Cursor: the controller's cursor is the timeline cursor (on by default, as in VEGAS).
    pub sync: bool,
    pub sel: Option<usize>,
    drag: Option<usize>,
    /// The cursor while Sync Cursor is off.
    own: f64,
    /// The key a right-click opened the curve menu on.
    menu: Option<usize>,
}

impl Default for KfState {
    fn default() -> Self {
        Self { sync: true, sel: None, drag: None, own: 0.0, menu: None }
    }
}

impl KfState {
    /// The controller's cursor in key time: `timeline` (the timeline cursor in key time) while synced, else its own.
    pub fn cursor(&mut self, timeline: f64) -> f64 {
        if self.sync {
            self.own = timeline;
        }
        self.own
    }

    /// Move the controller's cursor to `t`; Some(t) when the timeline cursor follows.
    pub fn seek(&mut self, t: f64) -> Option<f64> {
        self.own = t;
        self.sync.then_some(t)
    }
}

#[derive(Debug, PartialEq)]
pub enum KfHit {
    Key(usize),
    Seek(f64),
}

/// A click `x` pixels into a lane `w` pixels wide that shows 0..`span`: the nearest key within 5 px, else a seek there.
pub fn kf_hit(times: &[f64], span: f64, w: f32, x: f32) -> KfHit {
    let px = |t: f64| (t / span) as f32 * w;
    times
        .iter()
        .enumerate()
        .map(|(i, &t)| (i, (px(t) - x).abs()))
        .filter(|(_, d)| *d <= 5.0)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(KfHit::Seek((x / w).clamp(0.0, 1.0) as f64 * span), |(i, _)| KfHit::Key(i))
}

/// Key `i` dragged to `t`: on a frame, a frame clear of its neighbours (keys never pass each other), the last key not
/// past `end`. The first key is fixed where it is, as in VEGAS.
pub fn clamp_move(times: &[f64], i: usize, t: f64, fps: f64, end: f64) -> f64 {
    if i == 0 {
        return times[0];
    }
    let f = 1.0 / fps.max(1.0);
    let lo = times[i - 1] + f;
    let hi = times.get(i + 1).map_or(end, |n| n - f);
    crate::edit::quantize(t, fps).clamp(lo, hi.max(lo))
}

/// Set the value at `t`: a key there (within half a frame) keeps its curve, a new key is Smooth. The first edit away
/// from 0 also keeps the old value in a key at 0, as VEGAS's lists always start with one there.
pub fn set_at<T: Lerp + Default>(kf: &mut Keyframes<T>, t: f64, v: T, fps: f64) -> usize {
    let tol = 0.5 / fps.max(1.0);
    if kf.keys.is_empty() && t.abs() >= tol {
        kf.keys.push(Keyframe { t: 0.0, v: T::default(), curve: Curve::Smooth });
    }
    let curve = kf.keys.iter().find(|k| (k.t - t).abs() < tol).map_or(Curve::Smooth, |k| k.curve);
    kf.set_key(t, v, curve, fps)
}

/// Carry out intent `r` on `keys` with the controller's cursor at `at`; Some(t) asks for a seek.
pub fn apply<T: Lerp + Default>(r: KfResponse, keys: &mut Keyframes<T>, at: f64, fps: f64) -> Option<f64> {
    match r {
        KfResponse::Seek(t) => return Some(t),
        KfResponse::Add => {
            let v = keys.at(at);
            set_at(keys, at, v, fps);
        }
        // The first key stays: it holds the start value.
        KfResponse::Delete(0) | KfResponse::Move(0, _) => {}
        KfResponse::Delete(i) => keys.remove_key(i),
        KfResponse::Move(i, t) => {
            if let Some(k) = keys.keys.get_mut(i) {
                k.t = t;
            }
        }
        // The diamond shown at 0 on an empty list becomes a real key.
        KfResponse::SetCurve(0, c) if keys.keys.is_empty() => keys.keys.push(Keyframe { t: 0.0, v: keys.at(0.0), curve: c }),
        KfResponse::SetCurve(i, c) => {
            if let Some(k) = keys.keys.get_mut(i) {
                k.curve = c;
            }
        }
    }
    None
}

const LABEL_W: f32 = 140.0;
const RULER_H: f32 = 15.0;
const LANE_H: f32 = 12.0;
const BTN_H: f32 = 26.0;
pub const HEIGHT: f32 = 125.0;

const CURVES: [(&str, Curve); 6] =
    [("&Linear", Curve::Linear), ("&Fast", Curve::Fast), ("&Slow", Curve::Slow), ("S&mooth", Curve::Smooth), ("S&harp", Curve::Sharp), ("H&old", Curve::Hold)];

/// Draw the controller in `rect` for one lane `name` with `keys` (time, curve) over 0..`span`, the cursor at `at`; no
/// key moves past `end`.
#[allow(clippy::too_many_arguments)]
pub fn controller(ui: &mut Ui, rect: Rect, st: &mut KfState, name: &str, keys: &[(f64, Curve)], span: f64, end: f64, at: f64, fps: f64) -> Option<KfResponse> {
    // VEGAS always shows the first keyframe, even before anything is keyed.
    let keys = if keys.is_empty() { &[(0.0, Curve::Smooth)][..] } else { keys };
    let t = theme::get();
    let body = rect.with_max_y(rect.max.y - BTN_H);
    let labels = body.with_max_x(body.min.x + LABEL_W);
    let area = body.with_min_x(labels.max.x + 1.0);
    let ruler = area.with_max_y(area.min.y + RULER_H);
    let lanes = area.with_min_y(ruler.max.y);
    let lane0 = lanes.with_max_y(lanes.min.y + LANE_H);
    let view = (span * 1.05).max(1.0 / fps.max(1.0)); // a little of the checker past the end shows
    let x_of = |time: f64| lanes.min.x + (time / view) as f32 * lanes.width();
    let t_of = |x: f32| ((x - lanes.min.x) / lanes.width()).clamp(0.0, 1.0) as f64 * view;
    let times: Vec<f64> = keys.iter().map(|k| k.0).collect();
    let hit_at = |pos: Pos2| if lane0.y_range().contains(pos.y) { kf_hit(&times, view, lanes.width(), pos.x - lanes.min.x) } else { KfHit::Seek(t_of(pos.x)) };

    // ---- Interaction ----
    let mut out = None;
    let resp = ui.interact(area, ui.id().with("kf_area"), Sense::click_and_drag());
    if resp.drag_started() {
        st.drag = None;
        if let Some(KfHit::Key(i)) = ui.input(|i| i.pointer.press_origin()).map(hit_at) {
            (st.sel, st.drag) = (Some(i), Some(i));
        }
    }
    if let Some(pos) = resp.interact_pointer_pos().filter(|_| resp.dragged()) {
        out = Some(match st.drag.filter(|&i| i < times.len()) {
            Some(i) => KfResponse::Move(i, clamp_move(&times, i, t_of(pos.x), fps, end)),
            None => KfResponse::Seek(t_of(pos.x)),
        });
    }
    if resp.drag_stopped() {
        st.drag = None;
    }
    if let Some(pos) = resp.interact_pointer_pos().filter(|_| resp.clicked()) {
        out = Some(match hit_at(pos) {
            KfHit::Key(i) => {
                st.sel = Some(i);
                KfResponse::Seek(times[i])
            }
            KfHit::Seek(s) => {
                st.sel = None;
                KfResponse::Seek(s)
            }
        });
    }
    // Right-click a key: its curve, or Delete.
    if resp.secondary_clicked() {
        st.menu = resp.interact_pointer_pos().and_then(|pos| if let KfHit::Key(i) = hit_at(pos) { Some(i) } else { None });
        st.sel = st.menu.or(st.sel);
    }
    menus::context(&resp, |ui| {
        let Some(i) = st.menu.filter(|&i| i < keys.len()) else {
            ui.close_menu();
            return;
        };
        for (label, c) in CURVES {
            if entry(ui, label, "", true, Mark::Radio(keys[i].1 == c)) {
                out = Some(KfResponse::SetCurve(i, c));
            }
        }
        menus::sep(ui);
        if entry(ui, "&Delete", "", i > 0, Mark::None) {
            out = Some(KfResponse::Delete(i));
        }
    });
    // Delete over the window removes the selected key (the keymap leaves plain Delete to tool windows), unless a text
    // field is taking the key.
    let over = ui.ctx().pointer_hover_pos().and_then(|p| ui.ctx().layer_id_at(p)) == Some(ui.layer_id());
    if let Some(i) = st.sel.filter(|&i| over && out.is_none() && i > 0 && i < keys.len() && !ui.ctx().wants_keyboard_input()) {
        if ui.input_mut(|inp| inp.consume_key(Modifiers::NONE, Key::Delete)) {
            out = Some(KfResponse::Delete(i));
        }
    }
    if matches!(out, Some(KfResponse::Delete(_))) {
        st.sel = None;
    }

    // Button row: Sync Cursor, First / Previous / Next / Last, Create, Delete; the cursor time on the right.
    let eps = 0.5 / fps.max(1.0);
    let at_key = times.iter().position(|&k| (k - at).abs() < eps);
    let buttons = [
        ("Sync Cursor", None),
        ("First Keyframe", times.first().map(|&k| KfResponse::Seek(k))),
        ("Previous Keyframe", times.iter().rev().find(|&&k| k < at - eps).map(|&k| KfResponse::Seek(k))),
        ("Next Keyframe", times.iter().find(|&&k| k > at + eps).map(|&k| KfResponse::Seek(k))),
        ("Last Keyframe", times.last().map(|&k| KfResponse::Seek(k))),
        ("Create Keyframe", at_key.is_none().then_some(KfResponse::Add)),
        ("Delete Keyframe", st.sel.filter(|&i| i > 0 && i < times.len()).or(at_key.filter(|&i| i > 0)).map(KfResponse::Delete)),
    ];
    let p = ui.painter_at(rect);
    let row = Rect::from_min_max(pos2(rect.min.x, body.max.y), rect.max);
    p.rect_filled(row, 0.0, t.toolbar_bg);
    for (k, (tip, act)) in buttons.into_iter().enumerate() {
        let r = Rect::from_min_size(pos2(row.min.x + 4.0 + 26.0 * k as f32 + if k > 0 { 8.0 } else { 0.0 }, row.min.y + 3.0), vec2(22.0, 20.0));
        let enabled = k == 0 || act.is_some();
        let b = ui.interact(r, ui.id().with(("kf_btn", k)), if enabled { Sense::click() } else { Sense::hover() }).on_hover_text(tip);
        if (k == 0 && st.sync) || (enabled && b.hovered()) {
            p.rect_filled(r, CornerRadius::same(2), if k == 0 && st.sync { t.accent.gamma_multiply(0.6) } else { t.button_hover_bg });
        }
        glyph(&p, r, k, if enabled { t.text_bright } else { t.text_disabled });
        if b.clicked() {
            if k == 0 {
                st.sync = !st.sync;
            } else {
                out = act;
            }
        }
    }
    let tc = Rect::from_min_max(pos2(row.max.x - 96.0, row.min.y + 3.0), pos2(row.max.x - 4.0, row.max.y - 3.0));
    p.rect_filled(tc, 0.0, t.kf_lane);
    p.text(tc.center(), Align2::CENTER_CENTER, format_timecode(at.max(0.0), fps), t.font(12.0), t.text);

    // ---- Painting ----
    p.rect_filled(labels, 0.0, t.panel_bg);
    p.rect_filled(labels.with_max_y(ruler.max.y), 0.0, t.kf_ruler);
    let r0 = Rect::from_x_y_ranges(labels.x_range(), lane0.y_range());
    p.rect_filled(r0, 0.0, t.kf_row_sel);
    p.text(r0.left_center() + vec2(6.0, 0.0), Align2::LEFT_CENTER, name, t.font(11.0), t.text_bright);
    p.rect_filled(ruler, 0.0, t.kf_ruler);
    let mut y = lanes.min.y;
    let mut alt = false;
    while y < lanes.max.y {
        p.rect_filled(Rect::from_x_y_ranges(lanes.x_range(), Rangef::new(y, (y + LANE_H).min(lanes.max.y))), 0.0, if alt { t.kf_lane_alt } else { t.kf_lane });
        (y, alt) = (y + LANE_H, !alt);
    }
    // The checker past the end (of the event, or of the project for Track Motion).
    let end_x = x_of(span).max(lanes.min.x);
    let (mut cy, cell) = (lanes.min.y, 6.0);
    while cy < lanes.max.y {
        let mut cx = end_x;
        while cx < lanes.max.x {
            let odd = (((cx - end_x) / cell) as i32 + ((cy - lanes.min.y) / cell) as i32) % 2 == 1;
            let c = Rect::from_min_max(pos2(cx, cy), pos2((cx + cell).min(lanes.max.x), (cy + cell).min(lanes.max.y)));
            p.rect_filled(c, 0.0, if odd { t.checker_a } else { t.checker_b });
            cx += cell;
        }
        cy += cell;
    }
    // Ruler: a labelled tick at least 100 px apart.
    let pps = lanes.width() as f64 / view;
    let step = [1.0 / fps.max(1.0), 0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0, 600.0, 1800.0, 3600.0].into_iter().find(|s| s * pps >= 100.0).unwrap_or(7200.0);
    let mut k = 0.0;
    while k <= view {
        let x = x_of(k).round() + 0.5;
        p.vline(x, Rangef::new(ruler.max.y - 5.0, ruler.max.y), Stroke::new(1.0_f32, t.ruler_major_tick));
        p.text(pos2(x + 3.0, ruler.min.y + 1.0), Align2::LEFT_TOP, format_timecode(k, fps), t.font(10.0), t.ruler_text);
        k += step;
    }
    for (i, &kt) in times.iter().enumerate() {
        let c = pos2(x_of(kt), lane0.center().y);
        if lanes.x_range().contains(c.x) {
            let ink = if st.sel == Some(i) { t.text_bright } else { t.text_secondary };
            let d = 4.5;
            p.add(Shape::convex_polygon(vec![c + vec2(0.0, -d), c + vec2(d, 0.0), c + vec2(0.0, d), c + vec2(-d, 0.0)], ink, Stroke::new(1.0_f32, t.kf_lane)));
        }
    }
    let cx = x_of(at.clamp(0.0, view)).round() + 0.5;
    p.vline(cx, Rangef::new(ruler.min.y, lanes.max.y), Stroke::new(1.0_f32, t.text_bright));
    p.add(Shape::convex_polygon(vec![pos2(cx, ruler.min.y), pos2(cx + 6.0, ruler.min.y + 4.0), pos2(cx, ruler.min.y + 8.0)], t.text_bright, Stroke::NONE));
    out
}

/// More lanes under the controller's own in `rect` (the same `rect` and `span`): each (name, key times) is drawn
/// read-only; Some(k) when lane k's name is clicked. Lanes that do not fit are left out.
pub fn passive_lanes(ui: &Ui, rect: Rect, span: f64, fps: f64, lanes: &[(&str, &[f64])]) -> Option<usize> {
    let t = theme::get();
    // The controller's layout: label column, ruler, then its own lane.
    let body = rect.with_max_y(rect.max.y - BTN_H);
    let area = body.with_min_x(body.min.x + LABEL_W + 1.0);
    let view = (span * 1.05).max(1.0 / fps.max(1.0));
    let p = ui.painter_at(body);
    let mut picked = None;
    for (k, (name, times)) in lanes.iter().enumerate() {
        let y = body.min.y + RULER_H + LANE_H * (k + 1) as f32;
        if y + LANE_H > body.max.y {
            break;
        }
        let label = Rect::from_min_max(pos2(body.min.x, y), pos2(area.min.x - 1.0, y + LANE_H));
        if ui.interact(label, ui.id().with(("kf_lane", k)), Sense::click()).on_hover_text("Select this plug-in").clicked() {
            picked = Some(k);
        }
        p.text(label.left_center() + vec2(6.0, 0.0), Align2::LEFT_CENTER, *name, t.font(11.0), t.text);
        for &kt in *times {
            let c = pos2(area.min.x + (kt / view) as f32 * area.width(), label.center().y);
            if area.x_range().contains(c.x) {
                let d = 3.5;
                p.add(Shape::convex_polygon(vec![c + vec2(0.0, -d), c + vec2(d, 0.0), c + vec2(0.0, d), c + vec2(-d, 0.0)], t.text_dim, Stroke::NONE));
            }
        }
    }
    picked
}

/// Button glyphs: 0 Sync (a padlock), 1 First, 2 Previous, 3 Next, 4 Last, 5 Create (◆+), 6 Delete (◆×).
fn glyph(p: &Painter, r: Rect, k: usize, ink: Color32) {
    let c = r.center();
    let s = Stroke::new(1.5_f32, ink);
    let tri = |dir: f32, x: f32| Shape::convex_polygon(vec![pos2(x + 4.0 * dir, c.y - 5.0), pos2(x + 4.0 * dir, c.y + 5.0), pos2(x - 4.0 * dir, c.y)], ink, Stroke::NONE);
    let diamond = |o: Pos2| Shape::convex_polygon(vec![o + vec2(0.0, -5.0), o + vec2(5.0, 0.0), o + vec2(0.0, 5.0), o + vec2(-5.0, 0.0)], ink, Stroke::NONE);
    match k {
        0 => {
            p.rect_filled(Rect::from_center_size(c + vec2(0.0, 2.5), vec2(10.0, 7.0)), 1.0, ink);
            let arc: Vec<Pos2> = (0..=8).map(|i| c + vec2(0.0, -1.0) + Vec2::angled(std::f32::consts::PI * (1.0 + i as f32 / 8.0)) * 3.5).collect();
            p.add(Shape::line(arc, s));
        }
        1 | 4 => {
            let dir = if k == 1 { 1.0 } else { -1.0 };
            p.add(tri(dir, c.x + dir));
            p.vline(c.x - 4.5 * dir, Rangef::new(c.y - 5.0, c.y + 5.0), s);
        }
        2 | 3 => {
            p.add(tri(if k == 2 { 1.0 } else { -1.0 }, c.x));
        }
        _ => {
            p.add(diamond(c + vec2(-3.0, 0.0)));
            let o = c + vec2(5.0, 4.0);
            if k == 5 {
                p.hline(Rangef::new(o.x - 3.0, o.x + 3.0), o.y, s);
                p.vline(o.x, Rangef::new(o.y - 3.0, o.y + 3.0), s);
            } else {
                p.line_segment([o + vec2(-2.5, -2.5), o + vec2(2.5, 2.5)], s);
                p.line_segment([o + vec2(-2.5, 2.5), o + vec2(2.5, -2.5)], s);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_maps_to_the_nearest_key_or_a_seek() {
        let times = [0.0, 1.0, 1.02, 4.0];
        // 0..4 s over 400 px: 100 px a second.
        assert_eq!(kf_hit(&times, 4.0, 400.0, 3.0), KfHit::Key(0));
        assert_eq!(kf_hit(&times, 4.0, 400.0, 100.5), KfHit::Key(1)); // nearer 1.0 (100 px) than 1.02 (102 px)
        assert_eq!(kf_hit(&times, 4.0, 400.0, 102.5), KfHit::Key(2));
        assert_eq!(kf_hit(&times, 4.0, 400.0, 250.0), KfHit::Seek(2.5));
        assert_eq!(kf_hit(&times, 4.0, 400.0, 500.0), KfHit::Seek(4.0)); // past the end: clamped
        assert_eq!(kf_hit(&[], 4.0, 400.0, -10.0), KfHit::Seek(0.0));
    }

    #[test]
    fn moved_keys_stay_on_frames_between_their_neighbours() {
        let times = [0.0, 1.0, 2.0];
        let inf = f64::INFINITY;
        assert_eq!(clamp_move(&times, 1, 1.49, 10.0, inf), 1.5);
        assert_eq!(clamp_move(&times, 1, 5.0, 10.0, inf), 1.9);
        assert_eq!(clamp_move(&times, 1, -3.0, 10.0, inf), 0.1);
        assert_eq!(clamp_move(&times, 2, 9.0, 10.0, inf), 9.0);
        // The last key stops at the end (the event's, for Pan/Crop); the first key never moves.
        assert_eq!(clamp_move(&times, 2, 9.0, 10.0, 4.0), 4.0);
        assert_eq!(clamp_move(&times, 0, 0.5, 10.0, inf), 0.0);
        assert_eq!(clamp_move(&[0.3, 1.0], 0, -3.0, 10.0, inf), 0.3);
    }

    #[test]
    fn edits_at_the_cursor_and_intents() {
        let fps = 30.0;
        let mut k = Keyframes::<f32>::default();
        // The first edit at 2 s keeps the old value at 0.
        assert_eq!(set_at(&mut k, 2.0, 5.0, fps), 1);
        assert_eq!((k.times(), k.keys[0].v, k.keys[1].curve), (vec![0.0, 2.0], 0.0, Curve::Smooth));
        // Editing an existing key keeps its curve.
        k.keys[1].curve = Curve::Hold;
        assert_eq!(set_at(&mut k, 2.01, 7.0, fps), 1);
        assert_eq!((k.keys[1].v, k.keys[1].curve), (7.0, Curve::Hold));
        // An edit at 0 on an empty list makes just that key.
        let mut z = Keyframes::<f32>::default();
        set_at(&mut z, 0.0, 3.0, fps);
        assert_eq!(z.times(), vec![0.0]);
        // Intents.
        assert_eq!(apply(KfResponse::Seek(1.5), &mut k, 0.0, fps), Some(1.5));
        assert_eq!(apply(KfResponse::Add, &mut k, 1.0, fps), None);
        assert_eq!(k.times(), vec![0.0, 1.0, 2.0]);
        assert_eq!(k.keys[1].v, k.at(1.0));
        apply(KfResponse::Move(1, 1.5), &mut k, 0.0, fps);
        apply(KfResponse::SetCurve(1, Curve::Fast), &mut k, 0.0, fps);
        assert_eq!((k.keys[1].t, k.keys[1].curve), (1.5, Curve::Fast));
        // The first key can be neither deleted nor moved; the others can.
        apply(KfResponse::Delete(0), &mut k, 0.0, fps);
        apply(KfResponse::Move(0, 0.5), &mut k, 0.0, fps);
        assert_eq!(k.times(), vec![0.0, 1.5, 2.0]);
        apply(KfResponse::Delete(1), &mut k, 0.0, fps);
        assert_eq!(k.times(), vec![0.0, 2.0]);
        // A curve set on the diamond an empty list shows at 0 makes that key.
        let mut e = Keyframes::<f32>::default();
        apply(KfResponse::SetCurve(0, Curve::Hold), &mut e, 0.0, fps);
        assert_eq!((e.times(), e.keys[0].curve), (vec![0.0], Curve::Hold));
        // Sync Cursor: the timeline cursor drives it and follows its seeks; unsynced it keeps its own.
        let mut st = KfState::default();
        assert_eq!((st.cursor(3.0), st.seek(1.0)), (3.0, Some(1.0)));
        st.sync = false;
        assert_eq!((st.seek(2.0), st.cursor(9.0)), (None, 2.0));
    }
}
