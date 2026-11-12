//! Event Pan/Crop (§5.15.2–3): the crop box over the source picture in the light workspace, a property grid in source
//! pixels and the keyframe controller ("Position"). Keys are event-relative; each drag or typed value is one undo step,
//! and the preview follows through the engine's content hash.
use eframe::egui::*;
use crate::decode::Thumbs;
use crate::history::History;
use crate::project::{MediaType, PanCrop, Project};
use super::dialogs;
use super::keyframes::{self, KfState};
use super::menus::{self, Mark, entry};
use super::theme;
use super::workspace::{self, BoxKey, BoxPx, Workspace};

pub const ID: &str = "pan_crop";
const SIZE: Vec2 = vec2(860.0, 580.0);

pub struct PanCropWin {
    /// The event's id.
    pub clip: String,
    ws: Workspace,
    kf: KfState,
    /// A pointer gesture has its undo step already (`workspace::commit`).
    gesture: bool,
    /// The source picture last shown behind the box.
    backdrop: Option<TextureHandle>,
}

/// The workspace menu's items, in order.
const MENU: [&str; 5] = ["&Restore", "&Center", "Flip &Horizontal", "Flip &Vertical", "&Match Output Aspect"];

/// Workspace menu item `k` applied to `v`, for a `src`-pixel source in a `frame`-pixel project.
fn menu_op(k: usize, v: PanCrop, src: Vec2, frame: Vec2) -> PanCrop {
    match k {
        0 => PanCrop::default(),
        1 => PanCrop { cx: 0.5, cy: 0.5, ..v },
        2 => PanCrop { w: -v.w, ..v },
        3 => PanCrop { h: -v.h, ..v },
        // The width stays; the height follows the project frame's aspect.
        _ => PanCrop { h: (v.w.abs() * src.x / src.y * frame.y / frame.x).copysign(v.h), ..v },
    }
}

impl PanCropWin {
    /// For video event `id`; None when it is gone or an audio event.
    pub fn open(project: &Project, id: &str) -> Option<Self> {
        project.find_clip(id).filter(|r| r.is_video).map(|_| Self { clip: id.to_string(), ws: Workspace::new(), kf: KfState::default(), gesture: false, backdrop: None })
    }

    /// Draw the window and apply its edits. False once it is closed (× or the event is gone).
    pub fn show(&mut self, ctx: &Context, project: &mut Project, history: &mut History, thumbs: &mut Thumbs) -> bool {
        let Some(r) = project.find_clip(&self.clip).filter(|r| r.is_video) else { return false };
        let (ti, ci) = (r.track_idx, r.clip_idx);
        let tr = &project.timeline.video_tracks[ti];
        let c = &tr.clips[ci];
        let editable = !tr.locked && !c.locked;
        let fps = project.settings.frame_rate;
        let frame = vec2(project.settings.width as f32, project.settings.height as f32).max(Vec2::splat(1.0));
        let media = c.media_id.as_deref().and_then(|id| project.media_pool.iter().find(|m| m.id == id));
        let src = media.and_then(|m| Some(vec2(m.width? as f32, m.height? as f32))).unwrap_or(frame).max(Vec2::splat(1.0));
        let (start, span) = (c.position, c.duration);
        let at = self.kf.cursor(project.timeline.cursor_position - start).clamp(0.0, span);
        let picture = media.filter(|m| m.media_type != MediaType::Audio);
        // Half-second steps like the trimmer, none during playback (the decoders are paused), and the last picture
        // stays up while the next decodes.
        let s = (crate::edit::src_time(c, start + at).max(0.0) * 2.0).floor() / 2.0;
        if let Some(h) = picture.filter(|_| !thumbs.paused()).and_then(|m| thumbs.get(m, s)) {
            self.backdrop = Some(h.clone());
        }
        let tex = self.backdrop.as_ref().filter(|_| picture.is_some()).map(|h| h.id());
        let title = format!("Event Pan/Crop: {}", c.name);
        let mut keys = c.pan_crop.clone();
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
            self.ws.toolbar(&mut ui.new_child(UiBuilder::new().max_rect(bar.shrink2(vec2(4.0, 0.0))).layout(Layout::left_to_right(Align::Center))));

            // Property grid, in source pixels.
            let mut g = ui.new_child(UiBuilder::new().max_rect(grid_r).layout(Layout::top_down(Align::Min)));
            g.spacing_mut().item_spacing.y = 2.0;
            let b = v.to_box(src);
            let (mut x, mut y, mut w, mut h, mut a) = (b.c.x, b.c.y, b.size.x.abs(), b.size.y.abs(), b.rot);
            workspace::category(&mut g, "Position");
            let mut typed = workspace::prop(&mut g, "Center X", &mut x, 1.0, 1, editable);
            typed |= workspace::prop(&mut g, "Center Y", &mut y, 1.0, 1, editable);
            typed |= workspace::prop(&mut g, "Width", &mut w, 1.0, 1, editable);
            typed |= workspace::prop(&mut g, "Height", &mut h, 1.0, 1, editable);
            workspace::category(&mut g, "Rotation");
            typed |= workspace::prop(&mut g, "Angle", &mut a, 0.1, 1, editable);
            workspace::category(&mut g, "Source");
            g.label(format!("  {} × {} pixels", src.x, src.y));
            if typed {
                let nb = BoxPx { c: pos2(x, y), size: vec2(w.max(1.0).copysign(b.size.x), h.max(1.0).copysign(b.size.y)), rot: a };
                (v, edited) = (PanCrop::from_box(&nb, src), true);
            }

            // Workspace: the source picture with the crop box over it.
            let (resp, dragged) = self.ws.show(ui, ws_r, src, v.to_box(src), editable, |p, to| {
                let r = Rect::from_two_pos(to(Pos2::ZERO), to(src.to_pos2()));
                if let Some(tex) = tex {
                    p.image(tex, r, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
                } else {
                    p.rect_filled(r, 0.0, t.workspace_grid);
                }
                p.rect_stroke(r, 0.0, Stroke::new(1.0_f32, t.workspace_frame), StrokeKind::Outside);
            });
            if let Some(nb) = dragged {
                (v, edited) = (PanCrop::from_box(&nb, src), true);
            }
            menus::context(&resp, |ui| {
                for (k, label) in MENU.into_iter().enumerate() {
                    if entry(ui, label, "", editable, Mark::None) {
                        (v, edited) = (menu_op(k, v, src, frame), true);
                    }
                }
            });
            if edited {
                keyframes::set_at(&mut keys, at, v, fps);
            }
            if let Some(r) = keyframes::controller(ui, kf_r, &mut self.kf, "Position", &lanes, span, span, at, fps) {
                seek = keyframes::apply(r, &mut keys, at, fps);
            }
            ui.allocate_rect(full, Sense::hover());
        });

        let changed = editable && project.timeline.video_tracks[ti].clips[ci].pan_crop != keys;
        let down = ctx.input(|i| i.pointer.any_down());
        workspace::commit(project, history, &mut self.gesture, down, changed, |p| p.timeline.video_tracks[ti].clips[ci].pan_crop = keys);
        if let Some(s) = seek.and_then(|s| self.kf.seek(s.clamp(0.0, span))) {
            project.timeline.cursor_position = start + s;
        }
        !close
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_menu_items() {
        let (src, frame) = (vec2(1000.0, 1000.0), vec2(1920.0, 1080.0));
        let v = PanCrop { cx: 0.2, cy: 0.3, w: 0.5, h: -0.5, rot: 10.0 };
        assert_eq!(menu_op(0, v, src, frame), PanCrop::default());
        assert_eq!(menu_op(1, v, src, frame), PanCrop { cx: 0.5, cy: 0.5, ..v });
        assert_eq!((menu_op(2, v, src, frame).w, menu_op(3, v, src, frame).h), (-0.5, 0.5));
        // Match Output Aspect: the crop in source pixels gets the frame's 16:9, still flipped.
        let m = menu_op(4, v, src, frame);
        assert!(m.h < 0.0 && ((m.w * src.x) / (m.h.abs() * src.y) - 16.0 / 9.0).abs() < 1e-5);
        assert_eq!((m.cx, m.w, m.rot), (v.cx, v.w, v.rot));
    }
}
