//! Event Pan/Crop (§5.15.2–3): the crop box over the source picture in the light workspace, a property grid in source
//! pixels and the keyframe controller (rows "Position" and "Mask"). With the Mask row selected the workspace edits the
//! event's Bezier mask paths: a click adds a point to the selected path (a drag from it pulls out its curve handles),
//! points and handles drag, Ctrl+drag on a point pulls its handles out, and the right-click menu deletes points and
//! paths. Keys are event-relative; each drag or typed value is one undo step, and the preview follows through the
//! engine's content hash.
use eframe::egui::*;
use crate::decode::Thumbs;
use crate::history::History;
use crate::project::{Keyframe, Keyframes, Mask, MaskMode, MediaType, PanCrop, Project};
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
    /// The Mask row is selected: the workspace edits mask paths.
    mask: bool,
    /// The selected mask path, and the point (0: anchor, 1: in handle, 2: out handle) being dragged.
    path: usize,
    drag: Option<(usize, u8)>,
    /// Where the mask menu was opened (workspace pixels).
    menu_at: Option<Pos2>,
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

/// Add anchor `p` (normalised) to `m` after point `after` (None: at the end), in every key, so the keys keep matching.
pub fn add_point(m: &mut Mask, p: [f32; 2], after: Option<usize>) -> usize {
    let pt = [p[0], p[1], 0.0, 0.0, 0.0, 0.0];
    if m.path.keys.is_empty() {
        m.path.keys.push(Keyframe { t: 0.0, v: Vec::new(), curve: crate::project::Curve::Linear });
    }
    let mut at = 0;
    for k in &mut m.path.keys {
        at = after.map_or(k.v.len(), |a| (a + 1).min(k.v.len()));
        k.v.insert(at, pt);
    }
    at
}

/// Remove point `i` from every key of `m`.
pub fn delete_point(m: &mut Mask, i: usize) {
    for k in &mut m.path.keys {
        if i < k.v.len() {
            k.v.remove(i);
        }
    }
}

/// Point `i` of `path` moved to `p` (normalised): its anchor (`part` 0, the handles go with it), or one handle with
/// the other mirrored (1 in, 2 out).
pub fn move_point(path: &mut [[f32; 6]], i: usize, part: u8, p: [f32; 2]) {
    let Some(a) = path.get_mut(i) else { return };
    match part {
        0 => (a[0], a[1]) = (p[0], p[1]),
        1 => (a[2], a[3], a[4], a[5]) = (p[0] - a[0], p[1] - a[1], a[0] - p[0], a[1] - p[1]),
        _ => (a[4], a[5], a[2], a[3]) = (p[0] - a[0], p[1] - a[1], a[0] - p[0], a[1] - p[1]),
    }
}

/// The point of `path` (and which part: 0 anchor, 1 in handle, 2 out handle) within `tol` of `p`, all in workspace
/// pixels of a `src`-sized source.
fn hit_point(path: &[[f32; 6]], src: Vec2, p: Pos2, tol: f32) -> Option<(usize, u8)> {
    let px = |x: f32, y: f32| pos2(x * src.x, y * src.y);
    path.iter().enumerate().find_map(|(i, a)| {
        [(0u8, px(a[0], a[1])), (1, px(a[0] + a[2], a[1] + a[3])), (2, px(a[0] + a[4], a[1] + a[5]))]
            .into_iter()
            .filter(|(k, _)| *k == 0 || a[2..6].iter().any(|v| *v != 0.0))
            .find(|(_, q)| q.distance(p) <= tol)
            .map(|(k, _)| (i, k))
    })
}

impl PanCropWin {
    /// For video event `id`; None when it is gone or an audio event.
    pub fn open(project: &Project, id: &str) -> Option<Self> {
        project
            .find_clip(id)
            .filter(|r| r.is_video)
            .map(|_| Self { clip: id.to_string(), ws: Workspace::new(), kf: KfState::default(), gesture: false, backdrop: None, mask: false, path: 0, drag: None, menu_at: None })
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
        let mut masks = c.masks.clone();
        self.path = self.path.min(masks.len().saturating_sub(1));
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
            let mut bar_ui = ui.new_child(UiBuilder::new().max_rect(bar.shrink2(vec2(4.0, 0.0))).layout(Layout::left_to_right(Align::Center)));
            self.ws.toolbar(&mut bar_ui);
            if self.mask {
                bar_ui.add_space(8.0);
                bar_ui.label(RichText::new("Mask: click adds a point, Ctrl+drag a point pulls out its curve").color(t.text_dim));
            }

            let mut g = ui.new_child(UiBuilder::new().max_rect(grid_r).layout(Layout::top_down(Align::Min)));
            g.spacing_mut().item_spacing.y = 2.0;
            if !self.mask {
                // Property grid, in source pixels.
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
            } else if !masks.is_empty() {
                // The selected path: its mode, opacity and feather.
                let n = masks.len();
                workspace::category(&mut g, &format!("Mask path {} of {n}", self.path + 1));
                let m = &mut masks[self.path];
                g.add_enabled_ui(editable, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        ComboBox::from_id_salt("pc_mask_mode").width(150.0).selected_text(format!("Mode: {:?}", m.mode)).show_ui(ui, |ui| {
                            for mode in [MaskMode::Positive, MaskMode::Negative, MaskMode::Disabled] {
                                ui.selectable_value(&mut m.mode, mode, format!("{mode:?}"));
                            }
                        });
                    });
                });
                let (mut op, mut fe) = (m.opacity * 100.0, m.feather * 100.0);
                if workspace::prop(&mut g, "Opacity %", &mut op, 0.5, 1, editable) {
                    m.opacity = (op / 100.0).clamp(0.0, 1.0);
                }
                if workspace::prop(&mut g, "Feather %", &mut fe, 0.1, 1, editable) {
                    m.feather = (fe / 100.0).clamp(0.0, 0.5);
                }
                g.horizontal(|ui| {
                    ui.add_space(14.0);
                    if ui.add_enabled(self.path > 0, Button::new("◀")).on_hover_text("Previous path").clicked() {
                        self.path -= 1;
                    }
                    if ui.add_enabled(self.path + 1 < n, Button::new("▶")).on_hover_text("Next path").clicked() {
                        self.path += 1;
                    }
                });
            } else {
                workspace::category(&mut g, "Mask");
                g.label("  Click in the picture to start a path.");
            }

            // Workspace: the source picture with the crop box over it, and the mask paths.
            let shown: Vec<Vec<[f32; 6]>> = masks.iter().map(|m| m.path.at(at)).collect();
            let (sel_path, mask_mode) = (self.path, self.mask);
            let (resp, dragged) = self.ws.show(ui, ws_r, src, v.to_box(src), editable && !self.mask, |p, to| {
                let r = Rect::from_two_pos(to(Pos2::ZERO), to(src.to_pos2()));
                if let Some(tex) = tex {
                    p.image(tex, r, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
                } else {
                    p.rect_filled(r, 0.0, t.workspace_grid);
                }
                p.rect_stroke(r, 0.0, Stroke::new(1.0_f32, t.workspace_frame), StrokeKind::Outside);
                for (k, path) in shown.iter().enumerate() {
                    let sel = mask_mode && k == sel_path;
                    let ink = if sel { t.accent } else { Color32::from_rgb(230, 200, 40) };
                    let mut poly: Vec<Pos2> = crate::engine::raster::flatten(path, src.x, src.y).into_iter().map(|(x, y)| to(pos2(x, y))).collect();
                    if poly.len() > 2 {
                        poly.push(poly[0]);
                    }
                    p.add(Shape::line(poly, Stroke::new(if sel { 2.0_f32 } else { 1.0 }, ink)));
                    if sel {
                        for a in path {
                            let c = to(pos2(a[0] * src.x, a[1] * src.y));
                            for (hx, hy) in [(a[2], a[3]), (a[4], a[5])].into_iter().filter(|h| h.0 != 0.0 || h.1 != 0.0) {
                                let q = to(pos2((a[0] + hx) * src.x, (a[1] + hy) * src.y));
                                p.line_segment([c, q], Stroke::new(1.0_f32, ink));
                                p.circle_filled(q, 3.0, ink);
                            }
                            p.rect(Rect::from_center_size(c, vec2(6.0, 6.0)), 0.0, Color32::WHITE, Stroke::new(1.0_f32, ink), StrokeKind::Middle);
                        }
                    }
                }
            });
            if let Some(nb) = dragged {
                (v, edited) = (PanCrop::from_box(&nb, src), true);
            }

            if self.mask && editable && !self.ws.zoom_tool {
                let tol = 6.0 / self.ws.zoom();
                let norm = |q: Pos2| [q.x / src.x, q.y / src.y];
                // A click is seen on release, when there is no press origin any more.
                let press = if resp.clicked() { resp.interact_pointer_pos() } else { ui.input(|i| i.pointer.press_origin()) }.and_then(|o| self.ws.to_px(ws_r, o));
                let now = resp.interact_pointer_pos().and_then(|s| self.ws.to_px(ws_r, s));
                let ctrl = ui.input(|i| i.modifiers.command);
                if resp.drag_started_by(PointerButton::Primary) || resp.clicked() {
                    if let Some(p) = press {
                        let cur = masks.get(self.path).map(|m| m.path.at(at)).unwrap_or_default();
                        self.drag = match hit_point(&cur, src, p, tol) {
                            Some((i, 0)) if ctrl => Some((i, 2)),
                            Some(h) => Some(h),
                            None => {
                                if masks.is_empty() {
                                    masks.push(Mask { path: Keyframes::default(), mode: MaskMode::Positive, opacity: 1.0, feather: 0.0 });
                                    self.path = 0;
                                }
                                let i = add_point(&mut masks[self.path], norm(p), None);
                                // A drag from a new point pulls out its curve.
                                Some((i, 2))
                            }
                        };
                        if resp.clicked() {
                            self.drag = None;
                        }
                    }
                }
                if let (Some((i, part)), Some(p), true) = (self.drag, now, resp.dragged()) {
                    if let Some(m) = masks.get_mut(self.path) {
                        let mut path = m.path.at(at);
                        move_point(&mut path, i, part, norm(p));
                        keyframes::set_at(&mut m.path, at, path, fps);
                    }
                }
                if resp.drag_stopped() {
                    self.drag = None;
                }
            }
            if resp.secondary_clicked() {
                self.menu_at = resp.interact_pointer_pos().and_then(|s| self.ws.to_px(ws_r, s));
            }
            menus::context(&resp, |ui| {
                if !self.mask {
                    for (k, label) in MENU.into_iter().enumerate() {
                        if entry(ui, label, "", editable, Mark::None) {
                            (v, edited) = (menu_op(k, v, src, frame), true);
                        }
                    }
                    return;
                }
                let cur = masks.get(self.path).map(|m| m.path.at(at)).unwrap_or_default();
                let point = self.menu_at.and_then(|p| hit_point(&cur, src, p, 6.0 / self.ws.zoom())).map(|h| h.0);
                if entry(ui, "&Delete Point", "", editable && point.is_some(), Mark::None) {
                    if let (Some(i), Some(m)) = (point, masks.get_mut(self.path)) {
                        delete_point(m, i);
                    }
                }
                if entry(ui, "&New Mask Path", "", editable, Mark::None) {
                    masks.push(Mask { path: Keyframes::default(), mode: MaskMode::Positive, opacity: 1.0, feather: 0.0 });
                    self.path = masks.len() - 1;
                }
                if entry(ui, "Delete Mask &Path", "", editable && !masks.is_empty(), Mark::None) {
                    masks.remove(self.path);
                    self.path = self.path.saturating_sub(1);
                }
            });
            if edited {
                keyframes::set_at(&mut keys, at, v, fps);
            }
            // Controller rows: Position and Mask (the selected path's keys); the other row is drawn under it.
            let mask_times = masks.get(self.path).map_or(Vec::new(), |m| m.path.times());
            if self.mask {
                let lanes: Vec<_> = masks.get(self.path).map_or(Vec::new(), |m| m.path.keys.iter().map(|k| (k.t, k.curve)).collect());
                if let Some(r) = keyframes::controller(ui, kf_r, &mut self.kf, "Mask", &lanes, span, span, at, fps) {
                    seek = match masks.get_mut(self.path) {
                        Some(m) => keyframes::apply(r, &mut m.path, at, fps),
                        None => match r {
                            keyframes::KfResponse::Seek(t) => Some(t),
                            _ => None,
                        },
                    };
                }
                if keyframes::passive_lanes(ui, kf_r, span, fps, &[("Position", &keys.times())]).is_some() {
                    self.mask = false;
                }
            } else {
                let lanes: Vec<_> = keys.keys.iter().map(|k| (k.t, k.curve)).collect();
                if let Some(r) = keyframes::controller(ui, kf_r, &mut self.kf, "Position", &lanes, span, span, at, fps) {
                    seek = keyframes::apply(r, &mut keys, at, fps);
                }
                if keyframes::passive_lanes(ui, kf_r, span, fps, &[("Mask", &mask_times)]).is_some() {
                    self.mask = true;
                }
            }
            ui.allocate_rect(full, Sense::hover());
        });

        let c = &project.timeline.video_tracks[ti].clips[ci];
        let changed = editable && (c.pan_crop != keys || c.masks != masks);
        let down = ctx.input(|i| i.pointer.any_down());
        workspace::commit(project, history, &mut self.gesture, down, changed, |p| {
            let c = &mut p.timeline.video_tracks[ti].clips[ci];
            (c.pan_crop, c.masks) = (keys, masks);
        });
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

    /// R7: points go into every key of a path (so keyed paths keep interpolating), handles mirror, deletes match.
    #[test]
    fn mask_point_edits_keep_keys_matching() {
        let mut m = Mask { path: Keyframes::default(), mode: MaskMode::Positive, opacity: 1.0, feather: 0.0 };
        for p in [[0.1, 0.1], [0.9, 0.1], [0.5, 0.9]] {
            add_point(&mut m, p, None);
        }
        let mut later = m.path.keys[0].v.clone();
        later[0][0] = 0.3;
        m.path.keys.push(Keyframe { t: 2.0, v: later, curve: crate::project::Curve::Linear });
        assert!((m.path.at(1.0)[0][0] - 0.2).abs() < 1e-6, "keyed paths interpolate");
        assert_eq!(add_point(&mut m, [0.5, 0.5], Some(0)), 1);
        assert!(m.path.keys.iter().all(|k| k.v.len() == 4) && m.path.keys[1].v[1][..2] == [0.5, 0.5]);
        assert!((m.path.at(1.0)[0][0] - 0.2).abs() < 1e-6, "still interpolating after the insert");
        let mut path = m.path.keys[0].v.clone();
        move_point(&mut path, 0, 2, [0.2, 0.1]);
        assert_eq!(path[0], [0.1, 0.1, -0.1, 0.0, 0.1, 0.0]);
        move_point(&mut path, 0, 0, [0.4, 0.4]);
        assert_eq!(path[0], [0.4, 0.4, -0.1, 0.0, 0.1, 0.0]);
        assert_eq!(hit_point(&path, vec2(100.0, 100.0), pos2(50.0, 40.0), 2.0), Some((0, 2)));
        delete_point(&mut m, 1);
        assert!(m.path.keys.iter().all(|k| k.v.len() == 3));
    }
}
