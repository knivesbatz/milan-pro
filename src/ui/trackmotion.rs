//! Track Motion (§5.15.2): the box marks the track's picture inside the project frame; the grid shows its X/Y offset
//! from the centre and its size in frame pixels (and, in 3D Source Alpha, its depth and X / Y rotation); the toolbar's
//! compositing-mode list sets how the track composites. The keyframe controller's rows are Position, 2D Shadow and 2D
//! Glow (each with its check box). Opened on a compositing parent's Parent Motion, it moves the whole group. Keys are
//! on absolute timeline time.
use eframe::egui::*;
use crate::history::History;
use crate::project::{Composite, Halo, Halo2D, Motion, Project};
use super::dialogs;
use super::keyframes::{self, KfState};
use super::menus::{self, Mark, entry};
use super::theme;
use super::workspace::{self, BoxKey, BoxPx, Workspace};

pub const ID: &str = "track_motion";
const SIZE: Vec2 = vec2(860.0, 560.0);

/// VEGAS's compositing modes in its menu order (Custom, which takes a compositor, is the first row of the menu).
pub const COMPOSITE: [(&str, Composite); 15] = [
    ("3D Source Alpha", Composite::ThreeD),
    ("Source Alpha", Composite::SourceAlpha),
    ("Add", Composite::Add),
    ("Subtract", Composite::Subtract),
    ("Cut", Composite::Cut),
    ("Multiply (Mask)", Composite::Multiply),
    ("Overlay", Composite::Overlay),
    ("Hard Light", Composite::HardLight),
    ("Dodge", Composite::Dodge),
    ("Burn", Composite::Burn),
    ("Darken", Composite::Darken),
    ("Lighten", Composite::Lighten),
    ("Difference", Composite::Difference),
    ("Difference Squared", Composite::DifferenceSquared),
    ("Screen", Composite::Screen),
];

/// Milan Pro's extra blend modes (Preferences › Video), after VEGAS's.
pub const EXTRA: [(&str, Composite); 7] = [
    ("Soft Light", Composite::SoftLight),
    ("Exclusion", Composite::Exclusion),
    ("Hard Mix", Composite::HardMix),
    ("Hue", Composite::Hue),
    ("Saturation", Composite::Saturation),
    ("Color", Composite::Color),
    ("Luminosity", Composite::Luminosity),
];

pub fn composite_name(mode: Composite) -> &'static str {
    if mode == Composite::Custom {
        return "Custom";
    }
    COMPOSITE.iter().chain(&EXTRA).find(|c| c.1 == mode).map_or("Source Alpha", |c| c.0)
}

/// The compositing-mode items of a popup menu, `cur` checked; the one picked. Custom opens the Compositors window (a
/// compositor dropped on the track sets it); the extra modes show with their preference on, or when one is in use.
pub fn composite_menu(ui: &mut Ui, cur: Composite, enabled: bool) -> Option<Composite> {
    let mut out = None;
    if entry(ui, "Custom...", "", enabled, Mark::Radio(cur == Composite::Custom)) {
        menus::send(ui.ctx(), super::keymap::Command::ShowWindow(super::dock::Panel::Compositors));
    }
    for (label, mode) in COMPOSITE {
        if entry(ui, label, "", enabled, Mark::Radio(mode == cur)) {
            out = Some(mode);
        }
    }
    if menus::state(ui.ctx()).extra_modes || cur.extra() {
        menus::sep(ui);
        for (label, mode) in EXTRA {
            if entry(ui, label, "", enabled, Mark::Radio(mode == cur)) {
                out = Some(mode);
            }
        }
    }
    out
}

/// The keyframe controller row being edited.
#[derive(Clone, Copy, PartialEq)]
enum Row {
    Position,
    Shadow,
    Glow,
}

pub struct TrackMotionWin {
    /// The video track's id.
    pub track: String,
    /// Parent Motion (a compositing parent's group) instead of the track's own Track Motion.
    pub parent: bool,
    ws: Workspace,
    kf: KfState,
    gesture: bool,
    row: Row,
}

/// 2D Shadow / Glow `h` at `at` set to `v`; its first key keeps the old value at 0, as Track Motion's.
fn halo_set(h: &mut Halo2D, at: f64, v: Halo, fps: f64) {
    if h.keys.is_empty() {
        h.keys.keys.push(crate::project::Keyframe { t: 0.0, v: h.keys.at(0.0), curve: crate::project::Curve::Smooth });
    }
    keyframes::set_at(&mut h.keys, at, v, fps);
}

impl TrackMotionWin {
    /// For video track `id`; None when it is gone or an audio track.
    pub fn open(project: &Project, id: &str) -> Option<Self> {
        matches!(project.find_track(id), Some((true, _))).then(|| Self { track: id.to_string(), parent: false, ws: Workspace::new(), kf: KfState::default(), gesture: false, row: Row::Position })
    }

    /// Parent Motion of compositing parent `id`.
    pub fn open_parent(project: &Project, id: &str) -> Option<Self> {
        Self::open(project, id).map(|w| Self { parent: true, ..w })
    }

    /// Draw the window and apply its edits. False once it is closed (× or the track is gone).
    pub fn show(&mut self, ctx: &Context, project: &mut Project, history: &mut History) -> bool {
        let Some((true, ti)) = project.find_track(&self.track) else { return false };
        let tr = &project.timeline.video_tracks[ti];
        let editable = !tr.locked;
        let fps = project.settings.frame_rate;
        let frame = vec2(project.settings.width as f32, project.settings.height as f32).max(Vec2::splat(1.0));
        let at = self.kf.cursor(project.timeline.cursor_position).max(0.0);
        let mut keys = if self.parent { tr.parent_motion.clone() } else { tr.motion.clone() };
        let (mut comp, mut shadow, mut glow) = (if self.parent { tr.parent_mode } else { tr.composite }, tr.shadow.clone(), tr.glow.clone());
        if self.parent {
            self.row = Row::Position;
        }
        let span = crate::render::content_end(project).max(keys.keys.last().map_or(0.0, |k| k.t)).max(at).max(1.0);
        let title = format!("{} (Track {})", if self.parent { "Parent Track Motion" } else { "Track Motion" }, ti + 1);
        let three_d = comp == Composite::ThreeD && !self.parent;
        let mut v = keys.at(at);
        let mut edited = false;
        let mut seek = None;
        let t = theme::get();

        let (_, close) = dialogs::tool_window(ctx, ID, &title, SIZE, |ui| {
            let full = ui.available_rect_before_wrap();
            let bar = full.with_max_y(full.min.y + 24.0);
            let kf_r = full.with_min_y(full.max.y - keyframes::HEIGHT);
            let mid = Rect::from_min_max(pos2(full.min.x, bar.max.y + 4.0), pos2(full.max.x, kf_r.min.y - 5.0));
            let grid_r = mid.with_max_x(mid.min.x + 200.0);
            let ws_r = mid.with_min_x(grid_r.max.x + 5.0);
            ui.painter().rect_filled(bar, 0.0, t.toolbar_bg);
            let mut row = ui.new_child(UiBuilder::new().max_rect(bar.shrink2(vec2(4.0, 0.0))).layout(Layout::left_to_right(Align::Center)));
            row.add_enabled_ui(editable, |ui| {
                ComboBox::from_id_salt("tm_composite").width(130.0).selected_text(composite_name(comp)).show_ui(ui, |ui| {
                    if let Some(m) = composite_menu(ui, comp, true) {
                        comp = m;
                    }
                })
                .response
                .on_hover_text(if self.parent { "Parent Composite Mode" } else { "Compositing Mode" });
            });
            row.separator();
            self.ws.toolbar(&mut row);

            let mut g = ui.new_child(UiBuilder::new().max_rect(grid_r).layout(Layout::top_down(Align::Min)));
            g.spacing_mut().item_spacing.y = 2.0;
            if self.row == Row::Position {
                // Property grid: offset from the frame centre and size, in frame pixels.
                let b = v.to_box(frame);
                // Y counts up from the centre, as in VEGAS (stored and drawn top-down).
                let (mut x, mut y, mut w, mut h, mut a) = (b.c.x - frame.x * 0.5, frame.y * 0.5 - b.c.y, b.size.x.abs(), b.size.y.abs(), b.rot);
                let (mut z, mut rx, mut ry) = (v.z * frame.x, v.rx, v.ry);
                workspace::category(&mut g, "Position");
                let mut typed = workspace::prop(&mut g, "X", &mut x, 1.0, 1, editable);
                typed |= workspace::prop(&mut g, "Y", &mut y, 1.0, 1, editable);
                if three_d {
                    typed |= workspace::prop(&mut g, "Z", &mut z, 1.0, 1, editable);
                }
                typed |= workspace::prop(&mut g, "Width", &mut w, 1.0, 1, editable);
                typed |= workspace::prop(&mut g, "Height", &mut h, 1.0, 1, editable);
                workspace::category(&mut g, "Orientation");
                if three_d {
                    typed |= workspace::prop(&mut g, "X angle", &mut rx, 0.1, 1, editable);
                    typed |= workspace::prop(&mut g, "Y angle", &mut ry, 0.1, 1, editable);
                }
                typed |= workspace::prop(&mut g, if three_d { "Z angle" } else { "Angle" }, &mut a, 0.1, 1, editable);
                if typed {
                    let nb = BoxPx { c: pos2(x + frame.x * 0.5, frame.y * 0.5 - y), size: vec2(w.max(1.0).copysign(b.size.x), h.max(1.0).copysign(b.size.y)), rot: a };
                    (v, edited) = (Motion { z: z / frame.x, rx, ry, ..Motion::from_box(&nb, frame) }, true);
                }
            } else {
                // 2D Shadow / Glow: offset in frame pixels, blur and intensity in percent, colour.
                let (name, h, d) = if self.row == Row::Shadow { ("2D Shadow", &mut shadow, Halo::default()) } else { ("2D Glow", &mut glow, Halo::glow()) };
                let cur = if h.keys.is_empty() { d } else { h.keys.at(at) };
                let (mut x, mut y, mut blur, mut int) = (cur.x * frame.x, -cur.y * frame.y, cur.blur * 100.0, cur.intensity * 100.0);
                workspace::category(&mut g, name);
                let mut typed = false;
                if self.row == Row::Shadow {
                    typed |= workspace::prop(&mut g, "X offset", &mut x, 1.0, 1, editable);
                    typed |= workspace::prop(&mut g, "Y offset", &mut y, 1.0, 1, editable);
                }
                typed |= workspace::prop(&mut g, "Blur %", &mut blur, 0.1, 1, editable);
                typed |= workspace::prop(&mut g, "Intensity %", &mut int, 0.5, 1, editable);
                let mut c = Color32::from_rgb((cur.color[0] * 255.0) as u8, (cur.color[1] * 255.0) as u8, (cur.color[2] * 255.0) as u8);
                let picked = g.horizontal(|ui| {
                    ui.add_space(14.0);
                    ui.label("Color");
                    ui.add_enabled(editable, |ui: &mut Ui| color_picker::color_edit_button_srgba(ui, &mut c, color_picker::Alpha::Opaque)).changed()
                });
                if typed || picked.inner {
                    let color = [c.r(), c.g(), c.b()].map(|v| v as f32 / 255.0);
                    let nv = Halo { x: x / frame.x, y: -y / frame.y, blur: (blur / 100.0).clamp(0.0, 1.0), intensity: (int / 100.0).clamp(0.0, 1.0), color };
                    if h.keys.is_empty() {
                        h.keys.keys.push(crate::project::Keyframe { t: 0.0, v: d, curve: crate::project::Curve::Smooth });
                    }
                    halo_set(h, at, nv, fps);
                    h.on = true;
                }
            }

            // Workspace: the project frame, with the track's box in it.
            let (resp, dragged) = self.ws.show(ui, ws_r, frame, v.to_box(frame), editable && self.row == Row::Position, |p, to| {
                let r = Rect::from_two_pos(to(Pos2::ZERO), to(frame.to_pos2()));
                p.rect_stroke(r, 0.0, Stroke::new(1.0_f32, t.workspace_frame), StrokeKind::Outside);
            });
            if let Some(nb) = dragged {
                (v, edited) = (Motion { z: v.z, rx: v.rx, ry: v.ry, ..Motion::from_box(&nb, frame) }, true);
            }
            menus::context(&resp, |ui| {
                if entry(ui, "&Restore", "", editable, Mark::None) {
                    (v, edited) = (Motion::default(), true);
                }
                if entry(ui, "&Center", "", editable, Mark::None) {
                    (v, edited) = (Motion { x: 0.0, y: 0.0, ..v }, true);
                }
            });
            if edited {
                keyframes::set_at(&mut keys, at, v, fps);
            }

            // The controller's rows: the selected one is live, the others drawn under it (a click selects one).
            let rows: &[(Row, &str)] = if self.parent { &[(Row::Position, "Position")] } else { &[(Row::Position, "Position"), (Row::Shadow, "2D Shadow"), (Row::Glow, "2D Glow")] };
            let times = |r: Row| -> Vec<f64> {
                match r {
                    Row::Position => keys.times(),
                    Row::Shadow => shadow.keys.times(),
                    Row::Glow => glow.keys.times(),
                }
            };
            let others: Vec<(Row, &str, Vec<f64>)> = rows.iter().filter(|r| r.0 != self.row).map(|&(r, n)| (r, n, times(r))).collect();
            let name = rows.iter().find(|r| r.0 == self.row).map_or("Position", |r| r.1);
            let lanes: Vec<_> = match self.row {
                Row::Position => keys.keys.iter().map(|k| (k.t, k.curve)).collect(),
                Row::Shadow => shadow.keys.keys.iter().map(|k| (k.t, k.curve)).collect(),
                Row::Glow => glow.keys.keys.iter().map(|k| (k.t, k.curve)).collect(),
            };
            if let Some(r) = keyframes::controller(ui, kf_r, &mut self.kf, name, &lanes, span, f64::INFINITY, at, fps) {
                seek = match self.row {
                    Row::Position => keyframes::apply(r, &mut keys, at, fps),
                    Row::Shadow => keyframes::apply(r, &mut shadow.keys, at, fps),
                    Row::Glow => keyframes::apply(r, &mut glow.keys, at, fps),
                };
            }
            let passive: Vec<(&str, &[f64])> = others.iter().map(|o| (o.1, &o.2[..])).collect();
            if let Some(k) = keyframes::passive_lanes(ui, kf_r, span, fps, &passive) {
                self.row = others[k].0;
            }
            // The 2D Shadow / Glow check boxes, in the controller's label column (its own row first, then the others).
            if !self.parent {
                let order: Vec<Row> = std::iter::once(self.row).chain(others.iter().map(|o| o.0)).collect();
                for (lane, r) in order.into_iter().enumerate() {
                    let h = match r {
                        Row::Shadow => &mut shadow,
                        Row::Glow => &mut glow,
                        Row::Position => continue,
                    };
                    let y = kf_r.min.y + 15.0 + 12.0 * lane as f32;
                    let cb = Rect::from_min_size(pos2(kf_r.min.x + 120.0, y + 1.0), vec2(10.0, 10.0));
                    let resp = ui.interact(cb, ui.id().with(("tm_halo", lane)), Sense::click()).on_hover_text("On");
                    ui.painter().rect_stroke(cb, 0.0, Stroke::new(1.0_f32, t.text), StrokeKind::Inside);
                    if h.on {
                        ui.painter().rect_filled(cb.shrink(2.0), 0.0, t.text_bright);
                    }
                    if resp.clicked() && editable {
                        h.on = !h.on;
                    }
                }
            }
            ui.allocate_rect(full, Sense::hover());
        });

        let tr = &project.timeline.video_tracks[ti];
        let (cur_keys, cur_comp) = if self.parent { (&tr.parent_motion, tr.parent_mode) } else { (&tr.motion, tr.composite) };
        let changed = editable && (*cur_keys != keys || cur_comp != comp || tr.shadow != shadow || tr.glow != glow);
        let down = ctx.input(|i| i.pointer.any_down());
        let parent = self.parent;
        workspace::commit(project, history, &mut self.gesture, down, changed, |p| {
            let tr = &mut p.timeline.video_tracks[ti];
            if parent {
                (tr.parent_motion, tr.parent_mode) = (keys, comp);
            } else {
                (tr.motion, tr.composite, tr.shadow, tr.glow) = (keys, comp, shadow, glow);
            }
        });
        if let Some(s) = seek.and_then(|s| self.kf.seek(s.max(0.0))) {
            project.timeline.cursor_position = s;
        }
        !close
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A drag of the box in the workspace keys the new position at the cursor, as one undo step, and the release ends
    /// the gesture.
    #[test]
    fn dragging_the_box_keys_the_move_in_one_undo_step() {
        let ctx = Context::default();
        crate::ui::fonts::install(&ctx);
        let (mut p, mut h) = (Project::new("t"), History::default());
        p.timeline.cursor_position = 2.0;
        let id = p.timeline.video_tracks[0].id.clone();
        let mut w = TrackMotionWin::open(&p, &id).unwrap();
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1920.0, 1080.0));
        let mut frame = |events: Vec<Event>, p: &mut Project, h: &mut History| {
            let mut open = true;
            let _ = ctx.run(RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ctx| open = w.show(ctx, p, h));
            open
        };
        // Two frames lay the window out; then the box's centre is the workspace's centre.
        frame(vec![], &mut p, &mut h);
        frame(vec![], &mut p, &mut h);
        let ws = ctx.memory(|m| m.area_rect(Id::new(ID))).unwrap();
        let c = pos2(ws.min.x + 10.0 + 205.0 + (ws.width() - 20.0 - 205.0) * 0.5, ws.min.y + 31.0 + 6.0 + 28.0 + 120.0);
        let button = |pressed| Event::PointerButton { pos: c + vec2(60.0, 0.0), button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        frame(vec![Event::PointerMoved(c)], &mut p, &mut h);
        frame(vec![Event::PointerButton { pos: c, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }], &mut p, &mut h);
        for dx in [10.0, 30.0, 60.0] {
            frame(vec![Event::PointerMoved(c + vec2(dx, 0.0))], &mut p, &mut h);
        }
        frame(vec![button(false)], &mut p, &mut h);
        frame(vec![], &mut p, &mut h);
        let m = &p.timeline.video_tracks[0].motion;
        assert_eq!(m.times(), vec![0.0, 2.0], "a key at 0 keeps the start, the move is keyed at the cursor");
        assert!(m.keys[1].v.x > 0.01 && m.keys[1].v.y.abs() < 1e-4 && m.keys[0].v == Motion::default(), "{m:?}");
        assert_eq!(h.lens(), (1, 0));
        assert!(p.modified);
    }

    /// Every compositing mode has a name in the menus, and the extras are Milan Pro's, not VEGAS's.
    #[test]
    fn mode_names() {
        assert_eq!(composite_name(Composite::Multiply), "Multiply (Mask)");
        assert_eq!(composite_name(Composite::Custom), "Custom");
        assert!(COMPOSITE.iter().all(|c| !c.1.extra()) && EXTRA.iter().all(|c| c.1.extra()));
    }
}
