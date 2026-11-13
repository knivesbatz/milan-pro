//! Video Preview window (§5.8): toolbar, letterboxed frame with overlays, centred transport and the 2×2 status block.
use eframe::egui::*;
use std::sync::Arc;
use crate::decode::{Isolate, Preview, Quality, Size};
use crate::history::History;
use crate::project::*;
use super::icons::{Icon, paint_icon};
use super::menus::{Later, MenuAction};
use super::theme::{self, UiLook};
use super::toolbar::{arrow_menu, split_button, text_menu, tool_button};
use super::transport::{Cmd, Playback, cmd_button, cmd_menu};
use super::widgets;

/// View options of the Video Preview window (not saved with the project).
pub struct PreviewView {
    /// VP23 zoom; None = Fit.
    pub zoom: Option<f32>,
    pub grid: bool,
    pub safe_areas: bool,
    pub show_status: bool,
    pub show_transport: bool,
    /// Simulate Device Aspect Ratio: the frame is shown at the project's pixel aspect ratio, not square pixels.
    pub simulate_par: bool,
    /// R16b: Split-Screen View, its mode, and the part of the frame (0-1 of the unrotated frame: x0, y0, x1, y1)
    /// that shows the frame without FX or the clipboard's (not kept between sessions, as in VEGAS).
    pub split: bool,
    pub split_clipboard: bool,
    pub split_region: [f32; 4],
    /// The last Copy Snapshot to Clipboard frame (Split-Screen View › Clipboard shows it), and its texture.
    pub snapshot: Option<(Arc<ColorImage>, TextureHandle)>,
    /// Where a drag in the picture started (display 0-1), while one draws a new split region.
    split_drag: Option<Pos2>,
    /// The output rotation the frame was last drawn with (the toolbar's Select Left / Right Half go by it).
    rot: u16,
}

impl Default for PreviewView {
    fn default() -> Self {
        Self {
            zoom: None,
            grid: false,
            safe_areas: false,
            show_status: true,
            show_transport: true,
            simulate_par: false,
            split: false,
            split_clipboard: false,
            split_region: RIGHT_HALF,
            snapshot: None,
            split_drag: None,
            rot: 0,
        }
    }
}

const RIGHT_HALF: [f32; 4] = [0.5, 0.0, 1.0, 1.0];

impl PreviewView {
    /// The frame Split-Screen View › Clipboard puts in the region, while it is on (the scopes measure it too).
    pub fn split_snapshot(&self) -> Option<(&Arc<ColorImage>, [f32; 4])> {
        self.snapshot.as_ref().filter(|_| self.split && self.split_clipboard).map(|(img, _)| (img, self.split_region))
    }

    /// Select Left Half / Right Half / All of the frame as shown (turned by the output rotation); turns the split on.
    fn select(&mut self, shown: Rect, rot: u16) {
        self.split_region = region_of(shown, rot);
        self.split = true;
    }
}

/// Display point `p` (0-1 of the frame as shown, turned `rot` degrees clockwise) in the unrotated frame (0-1).
fn unturn(p: Pos2, rot: u16) -> Pos2 {
    match rot / 90 % 4 {
        1 => pos2(p.y, 1.0 - p.x),
        2 => pos2(1.0 - p.x, 1.0 - p.y),
        3 => pos2(1.0 - p.y, p.x),
        _ => p,
    }
}

/// The unrotated region (x0, y0, x1, y1) under display rect `r` (0-1 of the frame as shown).
fn region_of(r: Rect, rot: u16) -> [f32; 4] {
    let (a, b) = (unturn(r.min, rot), unturn(r.max, rot));
    [a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y)]
}

/// Where the unrotated `region` shows (0-1 of the frame as shown).
fn shown_of(region: [f32; 4], rot: u16) -> Rect {
    // Turning back is three more quarter turns.
    let back = (360 - rot % 360) % 360;
    let (a, b) = (unturn(pos2(region[0], region[1]), back), unturn(pos2(region[2], region[3]), back));
    Rect::from_two_pos(a, b)
}

/// Dock toolbar row. Returns a status message (snapshot copied / saved / failed).
pub fn draw_preview_toolbar(ui: &mut Ui, preview: &mut Preview, view: &mut PreviewView, action: &mut Option<MenuAction>) -> Option<String> {
    let v23 = theme::get().look() == UiLook::Vegas23;
    let mut msg = None;
    if tool_button(ui, Icon::Properties, "Project Video Properties", true, false).clicked() {
        *action = Some(MenuAction::Properties);
    }
    let external = super::menus::state(ui.ctx()).external;
    if tool_button(ui, Icon::ExternalMonitor, "Preview on External Monitor (Alt+Shift+4)", true, external).clicked() {
        super::menus::send(ui.ctx(), super::keymap::Command::ExternalMonitor);
    }
    if tool_button(ui, Icon::TrackFx, "Video Output FX", true, false).clicked() {
        super::menus::send(ui.ctx(), super::keymap::Command::VideoOutputFx);
    }
    // R16b: the region shows the frame without video FX, or the last snapshot copied to the clipboard.
    let (main, arrow) = split_button(ui, Icon::SplitScreen, "Split-Screen View", true, view.split);
    if main.clicked() {
        view.split = !view.split;
    }
    arrow_menu(ui, &arrow, |ui| {
        if ui.radio(!view.split_clipboard, "FX Bypassed").clicked() {
            view.split_clipboard = false;
        }
        if ui.radio(view.split_clipboard, "Clipboard").clicked() {
            view.split_clipboard = true;
        }
        ui.separator();
        let rot = view.rot;
        for (label, r) in [("Select Left Half", [0.0, 0.0, 0.5, 1.0]), ("Select Right Half", RIGHT_HALF), ("Select All", [0.0, 0.0, 1.0, 1.0])] {
            if ui.button(label).clicked() {
                view.select(Rect::from_min_max(pos2(r[0], r[1]), pos2(r[2], r[3])), rot);
            }
        }
    });
    preview.fx_bypassed = view.split && !view.split_clipboard;
    ui.add_space(2.0);
    let label = format!("{:?} ({:?})", preview.quality, preview.size);
    // VP16 "Best (Full) ▾" has a gap; VP23 "Preview (Auto)▾" is set tight.
    text_menu(ui, &label, if v23 { 1.0 } else { 7.0 }, "Preview Quality", |ui| quality_menu(ui, preview));
    if v23 {
        // VEGAS labels the fit-to-window zoom "100 %" (light2026: 748x421 display at "100 %").
        let z = view.zoom.map_or("100 %".to_owned(), |z| format!("{:.0} %", z * 100.0));
        text_menu(ui, &z, 24.0, "Zoom", |ui| {
            if ui.radio(view.zoom.is_none(), "Fit").clicked() {
                view.zoom = None;
                ui.close_menu();
            }
            for z in [0.25, 0.5, 1.0, 2.0, 4.0] {
                if ui.radio(view.zoom == Some(z), format!("{:.0} %", z * 100.0)).clicked() {
                    view.zoom = Some(z);
                    ui.close_menu();
                }
            }
        });
    }
    let (main, arrow) = split_button(ui, Icon::Overlays, "Overlays", true, view.grid || view.safe_areas || preview.isolate != Isolate::None);
    if main.clicked() {
        view.grid = !view.grid;
    }
    arrow_menu(ui, &arrow, |ui| {
        ui.checkbox(&mut view.grid, "Grid");
        ui.checkbox(&mut view.safe_areas, "Safe Areas");
        ui.separator();
        // R16b: one channel at a time; choosing the one shown turns it off.
        for (i, label) in Isolate::ALL {
            if ui.radio(preview.isolate == i, label).clicked() {
                preview.isolate = if preview.isolate == i { Isolate::None } else { i };
            }
        }
    });
    let has = preview.image.is_some();
    if tool_button(ui, Icon::SnapshotCopy, "Copy Snapshot to Clipboard", has, false).clicked() {
        if let Some(img) = &preview.image {
            ui.ctx().copy_image((**img).clone());
            // R16b: kept for Split-Screen View › Clipboard.
            let tex = ui.ctx().load_texture("split_snapshot", img.clone(), TextureOptions::LINEAR);
            view.snapshot = Some((img.clone(), tex));
            msg = Some(format!("Snapshot copied to the clipboard ({}x{}).", img.width(), img.height()));
        }
    }
    if tool_button(ui, Icon::SnapshotSave, "Save Snapshot to File", has, false).clicked() {
        msg = save_snapshot(preview);
    }
    if v23 {
        tool_button(ui, Icon::GeneratedMedia, "HDR (not applicable)", false, false);
        tool_button(ui, Icon::ExternalMonitor, "360° (not applicable)", false, false);
    }
    msg
}

/// Draft / Preview / Good / Best, each with an Auto / Full / Half / Quarter submenu ("Quality (Size)").
fn quality_menu(ui: &mut Ui, preview: &mut Preview) {
    for q in Quality::ALL {
        let mark = if q == preview.quality { "• " } else { "   " };
        ui.menu_button(format!("{mark}{q:?}"), |ui| {
            for s in Size::ALL {
                if ui.radio(q == preview.quality && s == preview.size, format!("{s:?}")).clicked() {
                    (preview.quality, preview.size) = (q, s);
                    ui.close_menu();
                }
            }
        });
    }
}

fn save_snapshot(preview: &Preview) -> Option<String> {
    let img = preview.image.as_ref()?;
    let path = rfd::FileDialog::new().add_filter("PNG Image", &["png"]).set_file_name("Snapshot.png").save_file()?;
    Some(match crate::demo::save_png(&path, img) {
        Ok(()) => format!("Snapshot saved to {}", path.display()),
        Err(e) => format!("Could not save snapshot: {e}"),
    })
}

/// Body: frame in the `preview_bg` surround, a 22 px transport row and the status block. A video FX tile dropped on
/// the picture goes on the Video Output FX (one undo step).
pub fn draw_preview(ui: &mut Ui, project: &mut Project, history: &mut History, preview: &mut Preview, view: &mut PreviewView, pb: &mut Playback) {
    let t = theme::get();
    let rect = ui.max_rect();
    let p = ui.painter().clone();
    p.rect_filled(rect, 0.0, t.preview_bg);
    let line_h = t.font.ui + 4.0;
    let v23 = t.look() == UiLook::Vegas23;
    // VP16: 2×2 labelled block; VP23: one centred row of icon + value pairs.
    let status_h = match (view.show_status, v23) {
        (false, _) => 0.0,
        (true, false) => 2.0 * line_h + 6.0,
        (true, true) => line_h + 8.0,
    };
    let transport_h = if view.show_transport { 28.0 } else { 0.0 };
    let video = Rect::from_min_max(rect.min, pos2(rect.max.x, rect.max.y - status_h - transport_h));

    let s = &project.settings;
    // R11: Simulate Device Aspect Ratio widens (or narrows) the frame by the pixel aspect ratio; Output rotation turns it.
    let par = if view.simulate_par { s.par.clamp(0.1, 10.0) as f32 } else { 1.0 };
    let quarter = s.rotation % 180 == 90;
    let shown = vec2(s.width as f32 * par, s.height.max(1) as f32);
    let shown = if quarter { vec2(shown.y, shown.x) } else { shown };
    let frame = match view.zoom {
        Some(z) => widgets::px(Rect::from_center_size(video.center(), shown * z)),
        None => fit_rect_to_aspect(video, shown.x / shown.y),
    };
    // The decode width: the frame's own width as shown, in project pixels.
    preview.display_w = ((if quarter { frame.height() } else { frame.width() }) / par).max(2.0) as u32;
    let pv = p.with_clip_rect(video.intersect(p.clip_rect()));
    pv.rect_filled(frame, 0.0, t.preview_frame);
    // Options › Mute All Video: the frame stays empty.
    if let Some(tex) = preview.texture.as_ref().filter(|_| !preview.mute) {
        pv.add(rotated_image(tex.id(), frame, s.rotation, [0.0, 0.0, 1.0, 1.0]));
        // R16b: Split-Screen View: the region from the frame without FX, or from the clipboard snapshot.
        let other = if view.split_clipboard { view.snapshot.as_ref().map(|(_, t)| t) } else { preview.bypass_texture() };
        if let Some(other) = other.filter(|_| view.split) {
            pv.add(rotated_image(other.id(), frame, s.rotation, view.split_region));
        }
    } else if let Some(err) = preview.error.as_ref().filter(|_| !preview.mute) {
        pv.text(frame.center(), Align2::CENTER_CENTER, err, t.font(12.0), t.warning_text);
    }
    view.rot = s.rotation;
    if view.split {
        // The region's edge, so the split shows where it is.
        let r = shown_of(view.split_region, s.rotation);
        let r = Rect::from_min_max(frame.lerp_inside(r.min.to_vec2()), frame.lerp_inside(r.max.to_vec2()));
        pv.rect_stroke(widgets::px(r), 0.0, Stroke::new(1.0_f32, Color32::from_white_alpha(140)), StrokeKind::Inside);
    }
    paint_overlays(&pv, frame, view);
    // With Split-Screen View on, a drag draws the region and a double-click takes the whole frame (VEGAS).
    let sense = if view.split { Sense::click_and_drag() } else { Sense::click() };
    let resp = ui.interact(video, ui.id().with("preview_video"), sense);
    if view.split {
        let at = |p: Pos2| ((p - frame.min) / frame.size()).clamp(Vec2::ZERO, Vec2::splat(1.0)).to_pos2();
        if resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
        }
        if resp.drag_started() {
            view.split_drag = resp.interact_pointer_pos().map(at);
        }
        if let (Some(a), Some(b)) = (view.split_drag, resp.interact_pointer_pos().map(at)) {
            if (b - a).length() > 0.005 {
                view.split_region = region_of(Rect::from_two_pos(a, b), s.rotation);
            }
        }
        if resp.drag_stopped() {
            view.split_drag = None;
        }
        if resp.double_clicked() {
            view.split_region = [0.0, 0.0, 1.0, 1.0];
        }
    }
    if let Some(d) = super::plugin_dropped(&resp) {
        let (target, before) = (crate::edit::FxTarget::VideoOutput, project.clone());
        if d.kind == crate::engine::plugins::PluginKind::VideoFx && crate::edit::add_fx(project, &target, d.fx.clone()) {
            history.push(before);
            super::open_tool(ui.ctx(), super::ToolWin::Fx(target));
        } else {
            super::hint(ui.ctx(), "Only video FX go on the Video Output.");
        }
    }
    let v16_menu = theme::get().look() == UiLook::Vegas16;
    super::menus::context(&resp, |ui| {
        if ui.checkbox(&mut view.simulate_par, "Simulate Device Aspect Ratio").clicked() {
            ui.close_menu();
        }
        if ui.radio(view.zoom.is_none(), "Scale Video to Fit Preview Window").clicked() {
            view.zoom = None;
            ui.close_menu();
        }
        ui.add_enabled(false, Button::new("Adjust Size and Quality for Optimal Playback")).on_disabled_hover_text(Later("R19").text());
        if ui.button("Display at Project Size").clicked() {
            (view.zoom, preview.size) = (Some(1.0), Size::Full);
            ui.close_menu();
        }
        ui.separator();
        ui.checkbox(&mut view.show_status, "Show Status Bar");
        ui.checkbox(&mut view.show_transport, "Show Transport Bar");
        if v16_menu {
            super::menus::cmd(ui, "Make spacebar and F12 Play/Pause instead of Play/Stop", super::keymap::Command::SpacePlayPause);
        }
    });

    if view.show_transport {
        // Centred: VP16 ▶ ❚❚ ■ ≡; VP23 every command but Record, then ⋯ (light2026 overrides spec §5.8's five).
        let cmds: &[Cmd] = if v23 { &Cmd::ALL[1..] } else { &[Cmd::Play, Cmd::Pause, Cmd::Stop] };
        let n = cmds.len() as f32 + 1.0;
        let tr = Rect::from_center_size(pos2(rect.center().x, video.max.y + transport_h * 0.5), vec2(n * t.m.button_pitch, t.m.button));
        let mut row = ui.new_child(UiBuilder::new().max_rect(tr).layout(Layout::left_to_right(Align::Center)));
        row.spacing_mut().item_spacing.x = 0.0;
        for &c in cmds {
            cmd_button(&mut row, c, project, pb);
        }
        let more = tool_button(&mut row, Icon::More, "More", true, false);
        arrow_menu(&row, &more, |ui| cmd_menu(ui, project, pb));
    }
    if view.show_status {
        status_block(&p, rect, status_h, line_h, frame, project, preview, v23);
    }
}

/// "Project / Preview" left, "Frame / Display" right (§5.8).
#[allow(clippy::too_many_arguments)]
fn status_block(p: &Painter, rect: Rect, status_h: f32, line_h: f32, frame: Rect, project: &Project, preview: &Preview, v23: bool) {
    let t = theme::get();
    let s = &project.settings;
    let fps = s.frame_rate;
    let pw = preview.width(s.width).min(s.width);
    let ph = (pw as f64 * s.height as f64 / s.width.max(1) as f64).round() as u32;
    let frame_no = (project.timeline.cursor_position * fps).round() as u64;
    let font = t.font(t.font.ui);
    if v23 {
        // light2026: page "1920x1080, 30.000p" · blue square "480x270, 30.000p" · frame "0" · display "748x421".
        let items = [
            (Icon::New, format!("{}x{}, {fps:.3}p", s.width, s.height), t.text_value),
            (Icon::StatusPreview, format!("{pw}x{ph}, {fps:.3}p"), t.warning_text),
            (Icon::StatusFrame, frame_no.to_string(), t.text_value),
            (Icon::StatusDisplay, format!("{}x{}", frame.width() as u32, frame.height() as u32), t.warning_text),
        ];
        let galleys: Vec<_> = items.iter().map(|(_, s, c)| p.layout_no_wrap(s.clone(), font.clone(), *c)).collect();
        let (icon_w, gap, sep) = (14.0, 6.0, 30.0);
        let total: f32 = galleys.iter().map(|g| icon_w + gap + g.size().x).sum::<f32>() + sep * (items.len() - 1) as f32;
        let cy = (rect.max.y - status_h * 0.5).round();
        let mut x = (rect.center().x - total * 0.5).round();
        for ((icon, _, c), g) in items.iter().zip(galleys) {
            paint_icon(p, Rect::from_min_size(pos2(x, cy - 7.0), vec2(icon_w, icon_w)), *icon, t.look(), None);
            x += icon_w + gap;
            let w = g.size().x;
            p.galley(pos2(x, cy - g.size().y * 0.5), g, *c);
            x += w + sep;
        }
        return;
    }
    let rows = [
        ("Project:", format!("{}x{}x32; {fps:.3}p", s.width, s.height), "Frame:", frame_no.to_string()),
        ("Preview:", format!("{pw}x{ph}x32; {fps:.3}p"), "Display:", format!("{}x{}x32", frame.width() as u32, frame.height() as u32)),
    ];
    let y0 = rect.max.y - status_h + 2.0;
    let half = rect.min.x + (rect.width() * 0.5).max(220.0);
    for (k, (l1, v1, l2, v2)) in rows.into_iter().enumerate() {
        let y = y0 + k as f32 * line_h;
        let warn = v23 && k == 1;
        p.text(pos2(rect.min.x + 6.0, y), Align2::LEFT_TOP, l1, font.clone(), t.text_label);
        p.text(pos2(rect.min.x + 64.0, y), Align2::LEFT_TOP, v1, font.clone(), if warn { t.warning_text } else { t.text_value });
        p.text(pos2(half, y), Align2::LEFT_TOP, l2, font.clone(), t.text_label);
        p.text(pos2(half + 58.0, y), Align2::LEFT_TOP, v2, font.clone(), if warn { t.warning_text } else { t.text_value });
    }
}

/// Grid (8 columns, square cells) and safe areas (action 90 %, title 80 %).
fn paint_overlays(p: &Painter, frame: Rect, view: &PreviewView) {
    let stroke = Stroke::new(1.0_f32, Color32::from_white_alpha(110));
    if view.grid {
        let step = frame.width() / 8.0;
        let mut x = frame.min.x + step;
        while x < frame.max.x - 1.0 {
            p.vline(x.round() + 0.5, frame.y_range(), stroke);
            x += step;
        }
        let mut y = frame.min.y + step;
        while y < frame.max.y - 1.0 {
            p.hline(frame.x_range(), y.round() + 0.5, stroke);
            y += step;
        }
    }
    if view.safe_areas {
        for f in [0.9, 0.8] {
            let r = widgets::px(Rect::from_center_size(frame.center(), frame.size() * f));
            p.rect_stroke(r, 0.0, stroke, StrokeKind::Inside);
        }
    }
}

/// R14: Preview on External Monitor's picture, filling `ui`: the Video Preview's frame on black, at one project pixel
/// per display pixel or scaled to fit (Preferences › Preview Device), turned by the project's output rotation if
/// `rotate`. The decode width follows the larger of this and the Video Preview window.
pub fn draw_external(ui: &mut Ui, project: &Project, preview: &mut Preview, fit: bool, rotate: bool, mute: bool) {
    let rect = ui.max_rect();
    let p = ui.painter();
    p.rect_filled(rect, 0.0, Color32::BLACK);
    let s = &project.settings;
    let rot = if rotate { s.rotation } else { 0 };
    let frame = external_frame(rect, vec2(s.width as f32, s.height.max(1) as f32), rot % 180 == 90, fit, ui.ctx().pixels_per_point());
    let w = if rot % 180 == 90 { frame.height() } else { frame.width() } * ui.ctx().pixels_per_point();
    preview.display_w = preview.display_w.max(w as u32);
    if let Some(tex) = preview.texture.as_ref().filter(|_| !mute) {
        p.add(rotated_image(tex.id(), frame, rot, [0.0, 0.0, 1.0, 1.0]));
    }
}

/// The external picture in `display` (points at `ppp` pixels per point) for a `size` project frame, turned a quarter
/// if `quarter`: fitted (letterboxed), or one project pixel per display pixel, centred (larger frames are cropped).
pub fn external_frame(display: Rect, size: Vec2, quarter: bool, fit: bool, ppp: f32) -> Rect {
    let shown = if quarter { vec2(size.y, size.x) } else { size };
    if fit {
        return fit_rect_to_aspect(display, shown.x / shown.y.max(1.0));
    }
    let r = Rect::from_center_size(display.center(), shown / ppp);
    let snap = |p: Pos2| ((p.to_vec2() * ppp).round() / ppp).to_pos2();
    Rect::from_min_max(snap(r.min), snap(r.max))
}

/// The `region` (0-1 of the unrotated frame: x0, y0, x1, y1) of `tex`, where it shows in the frame `r` turned `rot`
/// degrees clockwise (Project Properties › Output rotation); the whole region fills `r`.
fn rotated_image(tex: TextureId, r: Rect, rot: u16, region: [f32; 4]) -> Shape {
    let shown = shown_of(region, rot);
    let corners = [shown.left_top(), shown.right_top(), shown.right_bottom(), shown.left_bottom()];
    let mut mesh = epaint::Mesh::with_texture(tex);
    for c in corners {
        mesh.vertices.push(epaint::Vertex { pos: r.lerp_inside(c.to_vec2()), uv: unturn(c, rot), color: Color32::WHITE });
    }
    mesh.indices.extend([0, 1, 2, 0, 2, 3]);
    Shape::mesh(mesh)
}

fn fit_rect_to_aspect(container: Rect, aspect: f32) -> Rect {
    let ca = container.width() / container.height().max(0.001);
    let (w, h) = if ca > aspect {
        (container.height() * aspect, container.height())
    } else {
        (container.width(), container.width() / aspect.max(0.001))
    };
    let r = Rect::from_center_size(container.center(), vec2(w, h));
    Rect::from_min_max(r.min.round(), r.max.round())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R16b: Select Left Half picks the left of the frame as shown, whatever the output rotation, and the region
    /// shows where it was drawn.
    #[test]
    fn split_regions_follow_the_rotation() {
        let left = Rect::from_min_max(pos2(0.0, 0.0), pos2(0.5, 1.0));
        assert_eq!(region_of(left, 0), [0.0, 0.0, 0.5, 1.0]);
        // Turned 90° clockwise, the shown left half is the bottom of the frame.
        assert_eq!(region_of(left, 90), [0.0, 0.5, 1.0, 1.0]);
        assert_eq!(region_of(left, 180), [0.5, 0.0, 1.0, 1.0]);
        for rot in [0, 90, 180, 270] {
            let r = Rect::from_min_max(pos2(0.1, 0.2), pos2(0.4, 0.9));
            let back = shown_of(region_of(r, rot), rot);
            assert!((back.min - r.min).length() < 1e-6 && (back.max - r.max).length() < 1e-6, "{rot}: {back:?}");
        }
        let mut v = PreviewView::default();
        assert!(!v.split && v.split_snapshot().is_none());
        v.select(left, 0);
        assert!(v.split && v.split_region == [0.0, 0.0, 0.5, 1.0]);
    }

    /// R14: the external picture on a 2560x1440 display at 150 % (1706.7x960 points): 1:1 in display pixels and
    /// centred, or fitted (pillarboxed for 4:3); a quarter turn swaps the sides.
    #[test]
    fn external_monitor_frame_geometry() {
        let ppp = 1.5;
        let display = Rect::from_min_size(Pos2::ZERO, vec2(2560.0, 1440.0) / ppp);
        let hd = vec2(1920.0, 1080.0);
        let close = |a: Vec2, b: Vec2| (a - b).length() < 0.01;
        let one = external_frame(display, hd, false, false, ppp);
        assert!(close(one.size() * ppp, hd), "{one:?}");
        assert!((one.center() - display.center()).length() < 0.5);
        assert!(close(external_frame(display, hd, false, true, ppp).size(), display.size().round()));
        let sd = external_frame(display, vec2(640.0, 480.0), false, true, ppp);
        assert_eq!((sd.height(), sd.width()), (960.0, 1280.0));
        let turned = external_frame(display, hd, true, false, ppp);
        assert!(close(turned.size() * ppp, vec2(1080.0, 1920.0)));
        // Larger than the display at 1:1: cropped by the display, still centred.
        let uhd = external_frame(display, vec2(3840.0, 2160.0), false, false, ppp);
        assert!(uhd.contains_rect(display) && (uhd.center() - display.center()).length() < 0.5);
    }
}
