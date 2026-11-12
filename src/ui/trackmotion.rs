//! Track Motion (§5.15.2): the box marks the track's picture inside the project frame; the grid shows its X/Y offset
//! from the centre and its size in frame pixels; the toolbar's compositing-mode list sets how the track composites.
//! Keys are on absolute timeline time. 2D Shadow, 2D Glow, 3D and parenting are not here (FEATURE_PLAN §3).
use eframe::egui::*;
use crate::history::History;
use crate::project::{Composite, Motion, Project};
use super::dialogs;
use super::keyframes::{self, KfState};
use super::menus::{self, Mark, entry};
use super::theme;
use super::workspace::{self, BoxKey, BoxPx, Workspace};

pub const ID: &str = "track_motion";
const SIZE: Vec2 = vec2(860.0, 560.0);

/// VEGAS's compositing modes in its menu order; None marks one Milan Pro does not have (shown greyed).
pub const COMPOSITE: [(&str, Option<Composite>); 15] = [
    ("3D Source Alpha", None),
    ("Source Alpha", Some(Composite::SourceAlpha)),
    ("Add", Some(Composite::Add)),
    ("Subtract", Some(Composite::Subtract)),
    ("Cut", None),
    ("Multiply (Mask)", Some(Composite::Multiply)),
    ("Overlay", Some(Composite::Overlay)),
    ("Hard Light", None),
    ("Dodge", None),
    ("Burn", None),
    ("Darken", Some(Composite::Darken)),
    ("Lighten", Some(Composite::Lighten)),
    ("Difference", Some(Composite::Difference)),
    ("Difference Squared", None),
    ("Screen", Some(Composite::Screen)),
];

pub fn composite_name(mode: Composite) -> &'static str {
    COMPOSITE.iter().find(|c| c.1 == Some(mode)).map_or("Source Alpha", |c| c.0)
}

/// The compositing-mode items of a popup menu, `cur` checked; the one picked.
pub fn composite_menu(ui: &mut Ui, cur: Composite, enabled: bool) -> Option<Composite> {
    let mut out = None;
    for (label, mode) in COMPOSITE {
        if entry(ui, label, "", enabled && mode.is_some(), Mark::Radio(mode == Some(cur))) {
            out = mode;
        }
    }
    out
}

pub struct TrackMotionWin {
    /// The video track's id.
    pub track: String,
    ws: Workspace,
    kf: KfState,
    gesture: bool,
}

impl TrackMotionWin {
    /// For video track `id`; None when it is gone or an audio track.
    pub fn open(project: &Project, id: &str) -> Option<Self> {
        matches!(project.find_track(id), Some((true, _))).then(|| Self { track: id.to_string(), ws: Workspace::new(), kf: KfState::default(), gesture: false })
    }

    /// Draw the window and apply its edits. False once it is closed (× or the track is gone).
    pub fn show(&mut self, ctx: &Context, project: &mut Project, history: &mut History) -> bool {
        let Some((true, ti)) = project.find_track(&self.track) else { return false };
        let tr = &project.timeline.video_tracks[ti];
        let editable = !tr.locked;
        let fps = project.settings.frame_rate;
        let frame = vec2(project.settings.width as f32, project.settings.height as f32).max(Vec2::splat(1.0));
        let at = self.kf.cursor(project.timeline.cursor_position).max(0.0);
        let mut keys = tr.motion.clone();
        let mut comp = tr.composite;
        let span = crate::render::content_end(project).max(keys.keys.last().map_or(0.0, |k| k.t)).max(at).max(1.0);
        let title = format!("Track Motion (Track {})", ti + 1);
        let lanes: Vec<_> = keys.keys.iter().map(|k| (k.t, k.curve)).collect();
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
                .on_hover_text("Compositing Mode");
            });
            row.separator();
            self.ws.toolbar(&mut row);

            // Property grid: offset from the frame centre and size, in frame pixels.
            let mut g = ui.new_child(UiBuilder::new().max_rect(grid_r).layout(Layout::top_down(Align::Min)));
            g.spacing_mut().item_spacing.y = 2.0;
            let b = v.to_box(frame);
            // Y counts up from the centre, as in VEGAS (stored and drawn top-down).
            let (mut x, mut y, mut w, mut h, mut a) = (b.c.x - frame.x * 0.5, frame.y * 0.5 - b.c.y, b.size.x.abs(), b.size.y.abs(), b.rot);
            workspace::category(&mut g, "Position");
            let mut typed = workspace::prop(&mut g, "X", &mut x, 1.0, 1, editable);
            typed |= workspace::prop(&mut g, "Y", &mut y, 1.0, 1, editable);
            typed |= workspace::prop(&mut g, "Width", &mut w, 1.0, 1, editable);
            typed |= workspace::prop(&mut g, "Height", &mut h, 1.0, 1, editable);
            workspace::category(&mut g, "Orientation");
            typed |= workspace::prop(&mut g, "Angle", &mut a, 0.1, 1, editable);
            if typed {
                let nb = BoxPx { c: pos2(x + frame.x * 0.5, frame.y * 0.5 - y), size: vec2(w.max(1.0).copysign(b.size.x), h.max(1.0).copysign(b.size.y)), rot: a };
                (v, edited) = (Motion::from_box(&nb, frame), true);
            }

            // Workspace: the project frame, with the track's box in it.
            let (resp, dragged) = self.ws.show(ui, ws_r, frame, v.to_box(frame), editable, |p, to| {
                let r = Rect::from_two_pos(to(Pos2::ZERO), to(frame.to_pos2()));
                p.rect_stroke(r, 0.0, Stroke::new(1.0_f32, t.workspace_frame), StrokeKind::Outside);
            });
            if let Some(nb) = dragged {
                (v, edited) = (Motion::from_box(&nb, frame), true);
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
            if let Some(r) = keyframes::controller(ui, kf_r, &mut self.kf, "Position", &lanes, span, f64::INFINITY, at, fps) {
                seek = keyframes::apply(r, &mut keys, at, fps);
            }
            ui.allocate_rect(full, Sense::hover());
        });

        let tr = &project.timeline.video_tracks[ti];
        let changed = editable && (tr.motion != keys || tr.composite != comp);
        let down = ctx.input(|i| i.pointer.any_down());
        workspace::commit(project, history, &mut self.gesture, down, changed, |p| {
            let tr = &mut p.timeline.video_tracks[ti];
            (tr.motion, tr.composite) = (keys, comp);
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
}
