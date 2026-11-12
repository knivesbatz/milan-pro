//! The Pan/Crop and Track Motion workspace (§5.15.2): a box (centre, size, rotation) in workspace pixels over a light
//! dotted grid, with 8 sizing handles and a rotation circle; the view zooms and pans. Also the windows' shared toolbar
//! toggles, property rows and one-undo-step-per-gesture commit. The box maths is pure and tested.
use eframe::egui::*;
use crate::engine::keyframes::Lerp;
use crate::history::History;
use crate::project::{Motion, PanCrop, Project};
use super::icons::Icon;
use super::theme;
use super::toolbar::tool_button;

/// A rectangle in workspace pixels: centre, size (negative = flipped) and clockwise rotation in degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxPx {
    pub c: Pos2,
    pub size: Vec2,
    pub rot: f32,
}

/// `v` turned clockwise on screen (y down) by `deg`, as `Affine::rotate` turns it.
fn turn(v: Vec2, deg: f32) -> Vec2 {
    let (s, c) = deg.to_radians().sin_cos();
    vec2(v.x * c - v.y * s, v.x * s + v.y * c)
}

impl BoxPx {
    /// The point at box coordinates (fx, fy) in [-1, 1]² (±1 = the edges), in workspace pixels.
    pub fn at(&self, fx: f32, fy: f32) -> Pos2 {
        self.c + turn(vec2(fx * self.size.x.abs(), fy * self.size.y.abs()) * 0.5, self.rot)
    }

    /// Workspace point `p` relative to the centre, in the box's own (unrotated) axes.
    fn local(&self, p: Pos2) -> Vec2 {
        turn(p - self.c, -self.rot)
    }

    /// The rotation circle's radius.
    pub fn radius(&self) -> f32 {
        self.size.x.abs().min(self.size.y.abs()) * 0.5
    }
}

/// A keyframed value the workspace edits as a box in a workspace `size` pixels big.
pub trait BoxKey: Lerp + Default + Copy + PartialEq {
    fn to_box(&self, size: Vec2) -> BoxPx;
    fn from_box(b: &BoxPx, size: Vec2) -> Self;
}

/// Pan/Crop: the crop rectangle over the source, in source pixels.
impl BoxKey for PanCrop {
    fn to_box(&self, s: Vec2) -> BoxPx {
        BoxPx { c: pos2(self.cx * s.x, self.cy * s.y), size: vec2(self.w * s.x, self.h * s.y), rot: self.rot }
    }
    fn from_box(b: &BoxPx, s: Vec2) -> Self {
        PanCrop { cx: b.c.x / s.x, cy: b.c.y / s.y, w: b.size.x / s.x, h: b.size.y / s.y, rot: b.rot }
    }
}

/// Track Motion: the track's picture inside the project frame, in frame pixels.
impl BoxKey for Motion {
    fn to_box(&self, s: Vec2) -> BoxPx {
        BoxPx { c: pos2((0.5 + self.x) * s.x, (0.5 + self.y) * s.y), size: vec2(self.w * s.x, self.h * s.y), rot: self.rot }
    }
    fn from_box(b: &BoxPx, s: Vec2) -> Self {
        Motion { x: b.c.x / s.x - 0.5, y: b.c.y / s.y - 0.5, w: b.size.x / s.x, h: b.size.y / s.y, rot: b.rot }
    }
}

/// The 8 sizing handles as box coordinates: corners and edge midpoints.
pub const HANDLES: [(i8, i8); 8] = [(-1, -1), (0, -1), (1, -1), (1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0)];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    Handle(i8, i8),
    Rotate,
    Move,
}

/// What a press at workspace point `p` grabs: a handle, the rotation circle, else the inside of the box. `tol` is the
/// grab distance in workspace pixels.
pub fn hit(b: &BoxPx, p: Pos2, tol: f32) -> Option<Hit> {
    if let Some(&(hx, hy)) = HANDLES.iter().find(|&&(hx, hy)| b.at(hx as f32, hy as f32).distance(p) <= tol) {
        return Some(Hit::Handle(hx, hy));
    }
    let l = b.local(p);
    if (l.length() - b.radius()).abs() <= tol {
        return Some(Hit::Rotate);
    }
    let half = b.size.abs() * 0.5;
    (l.x.abs() <= half.x && l.y.abs() <= half.y).then_some(Hit::Move)
}

/// Handle `h` dragged to workspace point `p`. Lock aspect keeps |w| : |h|; about the centre keeps the centre, else the
/// opposite edge or corner stays put. Sizes keep their sign (a flip) and stay at least 1 px.
pub fn resize(b: &BoxPx, h: (i8, i8), p: Pos2, lock_aspect: bool, about_center: bool) -> BoxPx {
    let l = b.local(p);
    let old = b.size.abs();
    let (hx, hy) = (h.0 as f32, h.1 as f32);
    // From the fixed side (or twice from the centre) to the pointer.
    let ext = |hk: f32, lk: f32, ok: f32| if hk == 0.0 { ok } else if about_center { 2.0 * lk * hk } else { lk * hk + ok * 0.5 };
    let mut e = vec2(ext(hx, l.x, old.x), ext(hy, l.y, old.y)).max(Vec2::splat(1.0));
    if lock_aspect && old.x > 0.0 && old.y > 0.0 {
        let k = match (hx != 0.0, hy != 0.0) {
            (true, true) => (e.x / old.x).max(e.y / old.y),
            (true, false) => e.x / old.x,
            _ => e.y / old.y,
        };
        e = (old * k).max(Vec2::splat(1.0));
    }
    // A dragged axis grows away from its fixed side; an undragged one (lock aspect on an edge) about the centre.
    let shift = if about_center { Vec2::ZERO } else { vec2(hx * (e.x - old.x), hy * (e.y - old.y)) * 0.5 };
    BoxPx { c: b.c + turn(shift, b.rot), size: vec2(e.x.copysign(b.size.x), e.y.copysign(b.size.y)), rot: b.rot }
}

/// The rotation after the pointer moved from `from` to `to` around the centre: plus the angle swept (clockwise).
pub fn rotate(b: &BoxPx, from: Pos2, to: Pos2) -> f32 {
    let a = |p: Pos2| (p - b.c).angle().to_degrees();
    let d = (a(to) - a(from) + 540.0).rem_euclid(360.0) - 180.0;
    b.rot + d
}

/// Grid spacing in workspace pixels: 1, 2 or 5 × 10ⁿ, at least 24 screen pixels apart at `zoom`.
pub fn grid_step(zoom: f32) -> f32 {
    let mut s = 1.0;
    while s < 1e6 {
        for m in [1.0, 2.0, 5.0] {
            if s * m * zoom >= 24.0 {
                return s * m;
            }
        }
        s *= 10.0;
    }
    s
}

fn snap(p: Pos2, step: f32) -> Pos2 {
    pos2((p.x / step).round() * step, (p.y / step).round() * step)
}

/// Which workspace pixel sits at the middle of the screen area, and screen pixels per workspace pixel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub center: Pos2,
    pub zoom: f32,
}

impl View {
    /// The whole `size` with a margin around it.
    pub fn fit(rect: Rect, size: Vec2) -> Self {
        View { center: (size * 0.5).to_pos2(), zoom: (rect.width() / (size.x * 1.4)).min(rect.height() / (size.y * 1.4)).max(1e-3) }
    }
    pub fn to_screen(&self, rect: Rect, p: Pos2) -> Pos2 {
        rect.center() + (p - self.center) * self.zoom
    }
    pub fn to_px(&self, rect: Rect, s: Pos2) -> Pos2 {
        self.center + (s - rect.center()) / self.zoom
    }
    /// Zoom by `f` keeping the workspace point under screen point `s` in place.
    pub fn zoom_at(&mut self, rect: Rect, s: Pos2, f: f32) {
        let p = self.to_px(rect, s);
        self.zoom = (self.zoom * f).clamp(1e-3, 64.0);
        self.center = p - (s - rect.center()) / self.zoom;
    }
}

/// Toolbar toggles and the view and drag of one window's workspace.
pub struct Workspace {
    pub zoom_tool: bool,
    pub snap: bool,
    pub lock_aspect: bool,
    pub about_center: bool,
    view: Option<View>,
    /// What the press grabbed, the box then, the press point and last frame's point (workspace pixels).
    drag: Option<(Hit, BoxPx, Pos2, Pos2)>,
}

impl Workspace {
    /// Lock Aspect Ratio and Size About Center start on, snapping off.
    pub fn new() -> Self {
        Self { zoom_tool: false, snap: false, lock_aspect: true, about_center: true, view: None, drag: None }
    }

    /// Normal / Zoom, Enable Snapping, Lock Aspect Ratio and Size About Center.
    pub fn toolbar(&mut self, ui: &mut Ui) {
        ui.spacing_mut().item_spacing.x = 0.0;
        if tool_button(ui, Icon::NormalEdit, "Normal Edit Tool", true, !self.zoom_tool).clicked() {
            self.zoom_tool = false;
        }
        if tool_button(ui, Icon::ZoomEdit, "Zoom Edit Tool: click zooms in, Shift+click out, drag pans", true, self.zoom_tool).clicked() {
            self.zoom_tool = true;
        }
        ui.add_space(8.0);
        for (on, icon, tip) in [
            (&mut self.snap, Icon::Snapping, "Enable Snapping (to the grid)"),
            (&mut self.lock_aspect, Icon::Lock, "Lock Aspect Ratio"),
            (&mut self.about_center, Icon::SizeAboutCenter, "Size About Center"),
        ] {
            if tool_button(ui, icon, tip, true, *on).clicked() {
                *on = !*on;
            }
        }
    }

    /// Draw the workspace in `rect` for a pixel space `size` big, with `backdrop` (given the workspace→screen mapping)
    /// under box `b`, and run this frame's drag. Returns the response (for a context menu) and the dragged box.
    pub fn show(&mut self, ui: &mut Ui, rect: Rect, size: Vec2, b: BoxPx, editable: bool, backdrop: impl FnOnce(&Painter, &dyn Fn(Pos2) -> Pos2)) -> (Response, Option<BoxPx>) {
        let t = theme::get();
        let resp = ui.interact(rect, ui.id().with("workspace"), Sense::click_and_drag());
        let view = self.view.get_or_insert_with(|| View::fit(rect, size));
        let (ptr, wheel, shift) = ui.input(|i| (i.pointer.interact_pos(), i.raw_scroll_delta.y, i.modifiers.shift || i.modifiers.command));
        if let Some(s) = ptr.filter(|_| resp.hovered() && wheel != 0.0) {
            view.zoom_at(rect, s, (wheel * 0.004).exp());
        }
        let mut out = None;
        if self.zoom_tool {
            if let Some(s) = ptr.filter(|_| resp.clicked()) {
                view.zoom_at(rect, s, if shift { 1.0 / 1.5 } else { 1.5 });
            } else if resp.dragged() {
                view.center -= resp.drag_delta() / view.zoom;
            }
        } else if editable {
            if resp.drag_started() {
                let p = ui.input(|i| i.pointer.press_origin()).map(|o| view.to_px(rect, o));
                self.drag = p.and_then(|p| Some((hit(&b, p, 6.0 / view.zoom)?, b, p, p)));
            }
            if let (Some((h, b0, p0, last)), Some(s)) = (self.drag, ptr.filter(|_| resp.dragged())) {
                let p = view.to_px(rect, s);
                let step = grid_step(view.zoom);
                let snapped = |q: Pos2| if self.snap { snap(q, step) } else { q };
                let nb = match h {
                    Hit::Move => BoxPx { c: snapped(b0.c + (p - p0)), ..b },
                    Hit::Handle(hx, hy) => resize(&b0, (hx, hy), snapped(p), self.lock_aspect, self.about_center),
                    Hit::Rotate => BoxPx { rot: rotate(&b, last, p), ..b },
                };
                self.drag = Some((h, b0, p0, p));
                out = Some(nb).filter(|nb| *nb != b);
            }
            if resp.drag_stopped() {
                self.drag = None;
            }
            let over = self.drag.map(|d| d.0).or_else(|| ptr.filter(|_| resp.hovered()).and_then(|s| hit(&b, view.to_px(rect, s), 6.0 / view.zoom)));
            if let Some(h) = over {
                ui.ctx().set_cursor_icon(match h {
                    Hit::Move => CursorIcon::Move,
                    Hit::Rotate => CursorIcon::Grab,
                    Hit::Handle(0, _) => CursorIcon::ResizeVertical,
                    Hit::Handle(_, 0) => CursorIcon::ResizeHorizontal,
                    Hit::Handle(x, y) if x == y => CursorIcon::ResizeNwSe,
                    Hit::Handle(..) => CursorIcon::ResizeNeSw,
                });
            }
        }
        if self.zoom_tool && resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::ZoomIn);
        }

        // Paint: light workspace, dotted grid, backdrop, then the box.
        let view = *view;
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, t.workspace_bg);
        let step = grid_step(view.zoom);
        let (a, z) = (view.to_px(rect, rect.min), view.to_px(rect, rect.max));
        let dots = Stroke::new(1.0_f32, t.workspace_grid);
        // Integer line counts: adding `step` to a far-panned f32 can round back to the same value and never end.
        for k in (a.x / step).ceil() as i64..=(z.x / step).floor() as i64 {
            let sx = view.to_screen(rect, pos2(k as f32 * step, 0.0)).x.round() + 0.5;
            p.extend(Shape::dashed_line(&[pos2(sx, rect.min.y), pos2(sx, rect.max.y)], dots, 1.0, 3.0));
        }
        for k in (a.y / step).ceil() as i64..=(z.y / step).floor() as i64 {
            let sy = view.to_screen(rect, pos2(0.0, k as f32 * step)).y.round() + 0.5;
            p.extend(Shape::dashed_line(&[pos2(rect.min.x, sy), pos2(rect.max.x, sy)], dots, 1.0, 3.0));
        }
        backdrop(&p, &|q| view.to_screen(rect, q));
        let sc = |fx: f32, fy: f32| view.to_screen(rect, b.at(fx, fy));
        let ink = t.workspace_frame;
        let outline = [sc(-1.0, -1.0), sc(1.0, -1.0), sc(1.0, 1.0), sc(-1.0, 1.0), sc(-1.0, -1.0)];
        p.extend(Shape::dashed_line(&outline, Stroke::new(1.0_f32, ink), 4.0, 3.0));
        let r = b.radius() * view.zoom;
        let c = view.to_screen(rect, b.c);
        let ring: Vec<Pos2> = (0..=64).map(|k| c + Vec2::angled(k as f32 / 64.0 * std::f32::consts::TAU) * r).collect();
        p.extend(Shape::dashed_line(&ring, Stroke::new(1.0_f32, ink), 3.0, 3.0));
        for (hx, hy) in HANDLES {
            p.rect(Rect::from_center_size(sc(hx as f32, hy as f32), vec2(6.0, 6.0)), 0.0, Color32::WHITE, Stroke::new(1.0_f32, ink), StrokeKind::Middle);
        }
        // The "F" shows which way up (and round) the picture is.
        let galley = p.layout_no_wrap("F".into(), t.bold(16.0), ink);
        let half = galley.size() * 0.5;
        let angle = b.rot.to_radians();
        p.add(epaint::TextShape::new(c - turn(half, b.rot), galley, ink).with_angle(angle));
        (resp, out)
    }
}

/// A property-grid category header (Position, Rotation, ...).
pub fn category(ui: &mut Ui, text: &str) {
    let t = theme::get();
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 16.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, t.toolbar_bg);
    ui.painter().text(r.left_center() + vec2(6.0, 0.0), Align2::LEFT_CENTER, format!("⊟ {text}"), t.bold(12.0), t.text);
}

/// One property row: the label, then a drag-value editor. True when the value was edited.
pub fn prop(ui: &mut Ui, label: &str, v: &mut f32, speed: f64, decimals: usize, enabled: bool) -> bool {
    ui.horizontal(|ui| {
        let t = theme::get();
        let (r, _) = ui.allocate_exact_size(vec2(90.0, 18.0), Sense::hover());
        ui.painter().text(r.left_center() + vec2(14.0, 0.0), Align2::LEFT_CENTER, label, t.font(12.0), t.text);
        // A typed value applies on Enter or leaving the field: one undo step, no partial values.
        ui.add_enabled(enabled, DragValue::new(v).speed(speed).fixed_decimals(decimals).update_while_editing(false)).changed()
    })
    .inner
}

/// One undo step per pointer gesture (a drag, a click, a typed value): history is pushed before the gesture's first
/// change. `changed`: this frame's edit differs from the project; `apply` makes it.
pub fn commit(project: &mut Project, history: &mut History, gesture: &mut bool, down: bool, changed: bool, apply: impl FnOnce(&mut Project)) {
    if changed {
        if !*gesture {
            history.push(project.clone());
        }
        apply(project);
        project.mark_modified();
    }
    *gesture = down && (*gesture || changed);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Pos2, b: Pos2) -> bool {
        a.distance(b) < 1e-3
    }

    #[test]
    fn lock_aspect_resize_keeps_the_pixel_aspect() {
        let b = BoxPx { c: pos2(960.0, 540.0), size: vec2(1920.0, 1080.0), rot: 0.0 };
        // Right edge dragged out by 192 px, fixed left edge: both sides grow 10 %.
        let r = resize(&b, (1, 0), pos2(960.0 + 960.0 + 192.0, 700.0), true, false);
        assert!((r.size.x / r.size.y - 16.0 / 9.0).abs() < 1e-4);
        assert!((r.size.x - 2112.0).abs() < 1e-3 && close(r.at(-1.0, 0.0), b.at(-1.0, 0.0)));
        // A corner drag takes the larger change; a flipped box stays flipped.
        let f = BoxPx { size: vec2(-1920.0, 1080.0), ..b };
        let r = resize(&f, (1, 1), pos2(960.0 + 1200.0, 540.0 + 540.0), true, false);
        assert!(r.size.x < 0.0 && (r.size.x.abs() / r.size.y - 16.0 / 9.0).abs() < 1e-4);
        assert!(close(r.at(-1.0, -1.0), f.at(-1.0, -1.0))); // the opposite corner stays
        // Unlocked: only the dragged axis changes.
        let r = resize(&b, (0, -1), pos2(0.0, 0.0 - 60.0), false, false);
        assert_eq!((r.size.x, r.size.y), (1920.0, 1140.0));
    }

    #[test]
    fn resize_about_the_centre_keeps_the_centre() {
        let b = BoxPx { c: pos2(100.0, 50.0), size: vec2(80.0, 40.0), rot: 30.0 };
        let p = b.at(1.5, 1.0); // half again out along the box's own x, rotated
        let r = resize(&b, (1, 1), p, false, true);
        assert!(close(r.c, b.c));
        assert!((r.size.x - 120.0).abs() < 1e-3 && (r.size.y - 40.0).abs() < 1e-3);
        // Never below a pixel.
        assert_eq!(resize(&b, (1, 0), b.c, false, true).size.x, 1.0);
    }

    #[test]
    fn rotate_gives_the_drag_vectors_angle() {
        let b = BoxPx { c: pos2(10.0, 10.0), size: vec2(4.0, 4.0), rot: 5.0 };
        assert!((rotate(&b, pos2(20.0, 10.0), pos2(10.0, 20.0)) - 95.0).abs() < 1e-4); // clockwise on screen
        assert!((rotate(&b, pos2(20.0, 10.0), pos2(10.0, 0.0)) + 85.0).abs() < 1e-4);
        // Across the ±180° seam the step stays small.
        assert!((rotate(&b, pos2(0.0, 9.0), pos2(0.0, 11.0)) - 5.0 + 11.4212).abs() < 1e-3);
    }

    #[test]
    fn pixels_and_normalised_values_round_trip() {
        let src = vec2(1280.0, 720.0);
        let pc = PanCrop { cx: 0.25, cy: 0.75, w: -0.5, h: 0.375, rot: 30.0 };
        let b = pc.to_box(src);
        assert_eq!((b.c, b.size), (pos2(320.0, 540.0), vec2(-640.0, 270.0)));
        assert_eq!(PanCrop::from_box(&b, src), pc);
        let frame = vec2(1920.0, 1080.0);
        let m = Motion { x: 0.1, y: -0.2, w: 0.5, h: 0.25, rot: -45.0 };
        let b = m.to_box(frame);
        assert!(close(b.c, pos2(1152.0, 324.0)) && b.size == vec2(960.0, 270.0));
        let back = Motion::from_box(&b, frame);
        assert!((back.x - m.x).abs() < 1e-6 && (back.y - m.y).abs() < 1e-6 && (back.w, back.h, back.rot) == (m.w, m.h, m.rot));
        assert_eq!(Motion::default().to_box(frame), BoxPx { c: pos2(960.0, 540.0), size: frame, rot: 0.0 });
        // Screen and workspace pixels.
        let rect = Rect::from_min_size(pos2(200.0, 100.0), vec2(600.0, 400.0));
        let mut v = View::fit(rect, frame);
        v.zoom_at(rect, pos2(300.0, 150.0), 2.0);
        let q = pos2(123.0, 456.0);
        assert!(close(v.to_px(rect, v.to_screen(rect, q)), q));
        assert!(close(v.to_screen(rect, v.to_px(rect, pos2(300.0, 150.0))), pos2(300.0, 150.0)));
    }

    #[test]
    fn hits_handles_the_circle_then_the_inside() {
        let b = BoxPx { c: pos2(0.0, 0.0), size: vec2(100.0, 60.0), rot: 0.0 };
        assert_eq!(hit(&b, pos2(51.0, 29.0), 4.0), Some(Hit::Handle(1, 1)));
        assert_eq!(hit(&b, pos2(0.0, -30.0), 4.0), Some(Hit::Handle(0, -1)));
        assert_eq!(hit(&b, pos2(30.0, 0.0), 4.0), Some(Hit::Rotate));
        assert_eq!(hit(&b, pos2(5.0, 5.0), 4.0), Some(Hit::Move));
        assert_eq!(hit(&b, pos2(80.0, 0.0), 4.0), None);
        assert_eq!(grid_step(1.0), 50.0);
        assert_eq!(grid_step(0.25), 100.0);
    }

    #[test]
    fn one_undo_step_per_gesture() {
        let (mut p, mut h, mut g) = (Project::new("t"), History::default(), false);
        for k in 0..3 {
            commit(&mut p, &mut h, &mut g, true, true, |p| p.timeline.video_tracks[0].motion.keys.clear());
            assert_eq!(h.lens().0, 1, "frame {k}");
        }
        commit(&mut p, &mut h, &mut g, false, false, |_| {});
        commit(&mut p, &mut h, &mut g, false, true, |_| {});
        assert_eq!(h.lens().0, 2);
        assert!(p.modified);
    }
}
