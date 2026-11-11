use eframe::egui::{self, *};
use crate::project::*;

// ---- Public Types ----

#[derive(Clone)]
pub struct ClipRef {
    pub is_video: bool,
    pub track_idx: usize,
    pub clip_idx: usize,
}

pub struct DragState {
    pub _clip_ref: ClipRef,
    pub _original_position: f64,
}

// ---- Theme Colors ----

const BG_DARKEST: Color32 = Color32::from_rgb(18, 18, 20);
const BG_HEADER: Color32 = Color32::from_rgb(28, 28, 32);
const BG_PANEL: Color32 = Color32::from_rgb(38, 38, 42);
const TEXT_PRIMARY: Color32 = Color32::from_rgb(210, 210, 215);
const TEXT_DIM: Color32 = Color32::from_rgb(130, 130, 140);
const ACCENT: Color32 = Color32::from_rgb(74, 144, 255);
const ACCENT_DIM: Color32 = Color32::from_rgb(50, 100, 180);
const PLAYHEAD: Color32 = Color32::from_rgb(255, 68, 68);
const SEPARATOR: Color32 = Color32::from_rgb(55, 55, 62);
const CLIP_SELECTED: Color32 = Color32::from_rgb(255, 200, 50);
const TRACK_VIDEO_BG: Color32 = Color32::from_rgb(32, 32, 42);
const TRACK_AUDIO_BG: Color32 = Color32::from_rgb(32, 42, 36);
const TRACK_VIDEO_ALT: Color32 = Color32::from_rgb(36, 36, 46);
const TRACK_AUDIO_ALT: Color32 = Color32::from_rgb(36, 46, 40);
const MUTE_COLOR: Color32 = Color32::from_rgb(220, 80, 80);
const SOLO_COLOR: Color32 = Color32::from_rgb(220, 200, 60);
const RULER_BG: Color32 = Color32::from_rgb(24, 24, 28);
const RULER_TICK: Color32 = Color32::from_rgb(80, 80, 90);
const RULER_TEXT: Color32 = Color32::from_rgb(150, 150, 160);

// ---- Theme Setup ----

pub fn apply_theme(ctx: &egui::Context) {
    let mut visuals = Visuals::dark();
    visuals.panel_fill = BG_PANEL;
    visuals.window_fill = Color32::from_rgb(30, 30, 34);
    visuals.faint_bg_color = BG_DARKEST;
    visuals.extreme_bg_color = BG_DARKEST;
    visuals.widgets.noninteractive.bg_fill = BG_PANEL;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(50, 50, 56);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(60, 60, 68);
    visuals.widgets.active.bg_fill = ACCENT_DIM;
    visuals.selection.bg_fill = ACCENT_DIM;
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    visuals.window_stroke = Stroke::new(1.0, SEPARATOR);
    ctx.set_visuals(visuals);
}

// ---- Menu Bar ----

pub fn draw_menu_bar(ui: &mut Ui, project: &mut Project) {
    egui::menu::bar(ui, |ui| {
        ui.menu_button("File", |ui| {
            if ui.button("New Project").clicked() {
                *project = Project::new("Untitled");
                ui.close_menu();
            }
            if ui.button("Open Project...").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Milan Pro Project", &["mnle"])
                    .pick_file()
                {
                    if let Ok(p) = Project::load(&path) {
                        *project = p;
                    }
                }
                ui.close_menu();
            }
            ui.separator();
            if ui.button("Save").clicked() {
                if let Some(path) = project.path.clone() {
                    let _ = project.save(&path);
                } else {
                    save_as(project);
                }
                ui.close_menu();
            }
            if ui.button("Save As...").clicked() {
                save_as(project);
                ui.close_menu();
            }
            ui.separator();
            if ui.button("Import Media...").clicked() {
                import_media(project);
                ui.close_menu();
            }
            ui.separator();
            if ui.button("Exit").clicked() {
                std::process::exit(0);
            }
        });

        ui.menu_button("Edit", |ui| {
            ui.add_enabled(false, Button::new("Undo  Ctrl+Z"));
            ui.add_enabled(false, Button::new("Redo  Ctrl+Y"));
            ui.separator();
            ui.add_enabled(false, Button::new("Cut   Ctrl+X"));
            ui.add_enabled(false, Button::new("Copy  Ctrl+C"));
            ui.add_enabled(false, Button::new("Paste Ctrl+V"));
        });

        ui.menu_button("View", |ui| {
            ui.add_enabled(false, Button::new("Media Browser"));
            ui.add_enabled(false, Button::new("Properties"));
            ui.add_enabled(false, Button::new("Audio Mixer"));
        });

        ui.menu_button("Insert", |ui| {
            if ui.button("Video Track").clicked() {
                project.timeline.add_video_track();
                project.mark_modified();
                ui.close_menu();
            }
            if ui.button("Audio Track").clicked() {
                project.timeline.add_audio_track();
                project.mark_modified();
                ui.close_menu();
            }
            ui.separator();
            if ui.button("Marker at Cursor").clicked() {
                project.timeline.markers.push(Marker {
                    position: project.timeline.cursor_position,
                    name: format!("Marker {}", project.timeline.markers.len() + 1),
                    color: [255, 200, 50],
                });
                project.mark_modified();
                ui.close_menu();
            }
        });

        ui.menu_button("Tools", |ui| {
            ui.add_enabled(false, Button::new("Preferences..."));
            ui.add_enabled(false, Button::new("Keyboard Shortcuts..."));
            ui.separator();
            ui.add_enabled(false, Button::new("Render As..."));
        });

        ui.menu_button("Help", |ui| {
            if ui.button("About Milan Pro").clicked() {
                ui.close_menu();
            }
        });
    });
}

fn save_as(project: &mut Project) {
    if let Some(path) = rfd::FileDialog::new()
        .add_filter("Milan Pro Project", &["mnle"])
        .set_file_name(&format!("{}.mnle", project.name))
        .save_file()
    {
        let _ = project.save(&path);
    }
}

fn import_media(project: &mut Project) {
    if let Some(paths) = rfd::FileDialog::new()
        .add_filter("All Media", &[
            "mp4", "mov", "avi", "mkv", "wmv", "flv", "webm", "mxf", "mpg", "mpeg", "ts",
            "mp3", "wav", "flac", "aac", "ogg", "wma", "aiff", "opus",
            "png", "jpg", "jpeg", "tiff", "bmp", "webp", "gif", "exr",
        ])
        .add_filter("Video", &["mp4", "mov", "avi", "mkv", "wmv", "flv", "webm", "mxf"])
        .add_filter("Audio", &["mp3", "wav", "flac", "aac", "ogg", "wma", "aiff", "opus"])
        .add_filter("Images", &["png", "jpg", "jpeg", "tiff", "bmp", "webp", "gif", "exr"])
        .pick_files()
    {
        for path in paths {
            if let Ok(item) = crate::media::probe_media(&path) {
                project.media_pool.push(item);
            }
        }
        project.mark_modified();
    }
}

// ---- Toolbar ----

pub fn draw_toolbar(ui: &mut Ui, playing: &mut bool, project: &mut Project) {
    ui.horizontal(|ui| {
        if ui.button("⏮").on_hover_text("Go to Start").clicked() {
            project.timeline.cursor_position = 0.0;
        }
        let play_label = if *playing { "⏸" } else { "▶" };
        if ui.button(play_label)
            .on_hover_text(if *playing { "Pause (Space)" } else { "Play (Space)" })
            .clicked()
        {
            *playing = !*playing;
        }
        if ui.button("⏹").on_hover_text("Stop").clicked() {
            *playing = false;
            project.timeline.cursor_position = 0.0;
        }

        ui.separator();

        let tc = format_timecode(project.timeline.cursor_position, project.settings.frame_rate);
        ui.monospace(RichText::new(tc).color(ACCENT).size(14.0));

        ui.separator();

        if ui.button("−").on_hover_text("Zoom Out").clicked() {
            project.timeline.zoom = (project.timeline.zoom * 0.8).max(0.1);
        }
        ui.label(format!("{:.0}%", project.timeline.zoom * 100.0));
        if ui.button("+").on_hover_text("Zoom In").clicked() {
            project.timeline.zoom = (project.timeline.zoom * 1.25).min(50.0);
        }

        ui.separator();

        ui.label(
            RichText::new(format!(
                "{}x{} @ {:.2}fps",
                project.settings.width, project.settings.height, project.settings.frame_rate
            ))
            .color(TEXT_DIM)
            .size(11.0),
        );
    });
}

// ---- Status Bar ----

pub fn draw_status_bar(ui: &mut Ui, project: &Project) {
    ui.horizontal(|ui| {
        let mod_flag = if project.modified { " *" } else { "" };
        ui.label(RichText::new(format!("{}{mod_flag}", project.name)).size(11.0).color(TEXT_DIM));
        ui.separator();
        ui.label(
            RichText::new(format!(
                "V:{} A:{} Media:{}",
                project.timeline.video_tracks.len(),
                project.timeline.audio_tracks.len(),
                project.media_pool.len(),
            ))
            .size(11.0)
            .color(TEXT_DIM),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new("Milan Pro v0.1.0").size(11.0).color(TEXT_DIM));
        });
    });
}

// ---- Media Browser ----

pub fn draw_media_browser(ui: &mut Ui, project: &mut Project, selected: &mut Option<usize>) {
    ui.horizontal(|ui| {
        ui.heading(RichText::new("Media").size(13.0));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.small_button("+").on_hover_text("Import Media").clicked() {
                import_media(project);
            }
        });
    });
    ui.separator();

    if project.media_pool.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.label(RichText::new("No media imported").color(TEXT_DIM));
            ui.add_space(8.0);
            if ui.button("Import Media...").clicked() {
                import_media(project);
            }
        });
        return;
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
        let pool_len = project.media_pool.len();
        let mut to_remove = None;
        let mut add_to_video: Option<usize> = None;
        let mut add_to_audio: Option<usize> = None;

        for i in 0..pool_len {
            let is_selected = *selected == Some(i);
            let item = &project.media_pool[i];
            let icon = match item.media_type {
                MediaType::Video => "🎬",
                MediaType::Audio => "🎵",
                MediaType::Image => "🖼",
                MediaType::Unknown => "📄",
            };
            let label = format!("{icon} {}", item.name);
            let response = ui.selectable_label(is_selected, &label);
            if response.clicked() {
                *selected = Some(i);
            }
            response.context_menu(|ui| {
                if ui.button("Add to Video Track").clicked() {
                    add_to_video = Some(i);
                    ui.close_menu();
                }
                if ui.button("Add to Audio Track").clicked() {
                    add_to_audio = Some(i);
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Remove from Project").clicked() {
                    to_remove = Some(i);
                    ui.close_menu();
                }
            });
        }

        if let Some(i) = add_to_video {
            let pos = project.timeline.cursor_position;
            let clip = Clip::from_media(&project.media_pool[i], pos);
            if let Some(track) = project.timeline.video_tracks.first_mut() {
                track.clips.push(clip);
                project.mark_modified();
            }
        }
        if let Some(i) = add_to_audio {
            let pos = project.timeline.cursor_position;
            let clip = Clip::from_media(&project.media_pool[i], pos);
            if let Some(track) = project.timeline.audio_tracks.first_mut() {
                track.clips.push(clip);
                project.mark_modified();
            }
        }
        if let Some(i) = to_remove {
            project.media_pool.remove(i);
            *selected = None;
            project.mark_modified();
        }
    });
}

// ---- Preview ----

pub fn draw_preview(ui: &mut Ui, project: &Project, playing: bool) {
    let rect = ui.available_rect_before_wrap();
    let _ = ui.allocate_rect(rect, Sense::hover());
    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, 0.0, Color32::BLACK);

    let aspect = project.settings.width as f32 / project.settings.height.max(1) as f32;
    let preview_rect = fit_rect_to_aspect(rect.shrink(8.0), aspect);
    painter.rect_filled(preview_rect, 0.0, Color32::from_rgb(10, 10, 12));

    let status = if playing { "▶ Playing" } else { "Preview" };
    painter.text(
        preview_rect.center(),
        Align2::CENTER_CENTER,
        status,
        FontId::proportional(16.0),
        TEXT_DIM,
    );

    let tc = format_timecode(project.timeline.cursor_position, project.settings.frame_rate);
    painter.text(
        preview_rect.left_bottom() + vec2(8.0, -8.0),
        Align2::LEFT_BOTTOM,
        tc,
        FontId::monospace(11.0),
        Color32::from_rgb(180, 180, 180),
    );

    painter.text(
        preview_rect.right_bottom() + vec2(-8.0, -8.0),
        Align2::RIGHT_BOTTOM,
        format!("{}x{}", project.settings.width, project.settings.height),
        FontId::monospace(10.0),
        Color32::from_rgb(120, 120, 120),
    );
}

fn fit_rect_to_aspect(container: Rect, aspect: f32) -> Rect {
    let ca = container.width() / container.height().max(0.001);
    let (w, h) = if ca > aspect {
        (container.height() * aspect, container.height())
    } else {
        (container.width(), container.width() / aspect.max(0.001))
    };
    Rect::from_center_size(container.center(), vec2(w, h))
}

// ---- Properties ----

pub fn draw_properties(
    ui: &mut Ui,
    project: &Project,
    selected_clip: &Option<ClipRef>,
    selected_media: &Option<usize>,
) {
    ui.heading(RichText::new("Properties").size(13.0));
    ui.separator();

    if let Some(clip_ref) = selected_clip {
        let tracks = if clip_ref.is_video {
            &project.timeline.video_tracks
        } else {
            &project.timeline.audio_tracks
        };
        if let Some(track) = tracks.get(clip_ref.track_idx) {
            if let Some(clip) = track.clips.get(clip_ref.clip_idx) {
                ui.label(RichText::new("Clip Properties").color(ACCENT).size(12.0));
                ui.add_space(4.0);
                egui::Grid::new("clip_props").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                    prop_row(ui, "Name", &clip.name);
                    prop_row(ui, "Position", &format!("{:.3}s", clip.position));
                    prop_row(ui, "Duration", &format!("{:.3}s", clip.duration));
                    prop_row(ui, "In Point", &format!("{:.3}s", clip.in_point));
                    prop_row(ui, "Out Point", &format!("{:.3}s", clip.out_point));
                    prop_row(ui, "Track", &track.name);
                });
                return;
            }
        }
    }

    if let Some(idx) = selected_media {
        if let Some(item) = project.media_pool.get(*idx) {
            ui.label(RichText::new("Media Properties").color(ACCENT).size(12.0));
            ui.add_space(4.0);
            egui::Grid::new("media_props").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                prop_row(ui, "Name", &item.name);
                prop_row(ui, "Type", &format!("{:?}", item.media_type));
                prop_row(ui, "Duration", &format!("{:.3}s", item.duration));
                if let Some(w) = item.width {
                    prop_row(ui, "Width", &format!("{w}"));
                }
                if let Some(h) = item.height {
                    prop_row(ui, "Height", &format!("{h}"));
                }
                if let Some(fps) = item.frame_rate {
                    prop_row(ui, "Frame Rate", &format!("{fps:.2}"));
                }
                if let Some(sr) = item.sample_rate {
                    prop_row(ui, "Sample Rate", &format!("{sr} Hz"));
                }
                if let Some(ref codec) = item.codec {
                    prop_row(ui, "Codec", codec);
                }
            });
            return;
        }
    }

    ui.label(RichText::new("Project Settings").color(ACCENT).size(12.0));
    ui.add_space(4.0);
    egui::Grid::new("proj_props").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        prop_row(ui, "Resolution", &format!("{}x{}", project.settings.width, project.settings.height));
        prop_row(ui, "Frame Rate", &format!("{:.2} fps", project.settings.frame_rate));
        prop_row(ui, "Sample Rate", &format!("{} Hz", project.settings.sample_rate));
        prop_row(ui, "Duration", &format_duration_short(project.timeline.duration()));
    });
}

fn prop_row(ui: &mut Ui, label: &str, value: &str) {
    ui.label(RichText::new(label).color(TEXT_DIM).size(11.0));
    ui.label(RichText::new(value).size(11.0));
    ui.end_row();
}

fn format_duration_short(seconds: f64) -> String {
    let total = seconds as u64;
    let s = total % 60;
    let m = (total / 60) % 60;
    let h = total / 3600;
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

// ---- Timeline ----

const HEADER_W: f32 = 160.0;
const RULER_H: f32 = 28.0;
const BASE_PPS: f64 = 80.0;

pub fn draw_timeline(
    ui: &mut Ui,
    project: &mut Project,
    selected_clip: &mut Option<ClipRef>,
    _drag_state: &mut Option<DragState>,
    _scroll_y: &mut f32,
) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Timeline").size(13.0).color(TEXT_PRIMARY));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.small_button("+ Audio").clicked() {
                project.timeline.add_audio_track();
                project.mark_modified();
            }
            if ui.small_button("+ Video").clicked() {
                project.timeline.add_video_track();
                project.mark_modified();
            }
        });
    });

    let rect = ui.available_rect_before_wrap();
    if rect.width() < 20.0 || rect.height() < 20.0 {
        return;
    }
    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    let painter = ui.painter_at(rect);

    let pps = project.timeline.zoom * BASE_PPS;
    let scroll_x = project.timeline.scroll_x;

    painter.rect_filled(rect, 0.0, BG_DARKEST);
    painter.rect_filled(
        Rect::from_min_size(rect.min, vec2(HEADER_W, rect.height())),
        0.0,
        BG_HEADER,
    );

    // Ruler
    let ruler_clip = Rect::from_min_size(
        rect.min + vec2(HEADER_W, 0.0),
        vec2(rect.width() - HEADER_W, RULER_H),
    );
    draw_ruler(&painter, ruler_clip, pps, scroll_x);
    painter.rect_filled(
        Rect::from_min_size(rect.min, vec2(HEADER_W, RULER_H)),
        0.0,
        BG_HEADER,
    );
    painter.text(
        rect.min + vec2(8.0, RULER_H * 0.5),
        Align2::LEFT_CENTER,
        "Tracks",
        FontId::proportional(11.0),
        TEXT_DIM,
    );

    let clips_left = rect.min.x + HEADER_W;
    let clips_w = rect.width() - HEADER_W;
    let mut y = rect.min.y + RULER_H;

    // Video tracks
    let num_video = project.timeline.video_tracks.len();
    for i in 0..num_video {
        let th = project.timeline.video_tracks[i].height;
        if y > rect.max.y { break; }
        let bg = if i % 2 == 0 { TRACK_VIDEO_BG } else { TRACK_VIDEO_ALT };
        painter.rect_filled(Rect::from_min_size(pos2(clips_left, y), vec2(clips_w, th)), 0.0, bg);
        draw_track_header(&painter, pos2(rect.min.x, y), th, &project.timeline.video_tracks[i], true);

        let num_clips = project.timeline.video_tracks[i].clips.len();
        for j in 0..num_clips {
            let clip = &project.timeline.video_tracks[i].clips[j];
            let x0 = ((clip.position - scroll_x) * pps) as f32 + clips_left;
            let x1 = (((clip.position + clip.duration) - scroll_x) * pps) as f32 + clips_left;
            if x1 > clips_left && x0 < rect.max.x {
                let cr = Rect::from_min_max(
                    pos2(x0.max(clips_left), y + 2.0),
                    pos2(x1.min(rect.max.x), y + th - 2.0),
                );
                let sel = selected_clip.as_ref().is_some_and(|s| s.is_video && s.track_idx == i && s.clip_idx == j);
                draw_clip(&painter, cr, clip, sel);
            }
        }

        painter.line_segment([pos2(rect.min.x, y + th), pos2(rect.max.x, y + th)], Stroke::new(1.0, SEPARATOR));
        y += th;
    }

    // Separator
    let sep_y = y;
    painter.rect_filled(Rect::from_min_size(pos2(rect.min.x, y), vec2(rect.width(), 3.0)), 0.0, SEPARATOR);
    y += 3.0;

    // Audio tracks
    let num_audio = project.timeline.audio_tracks.len();
    for i in 0..num_audio {
        let th = project.timeline.audio_tracks[i].height;
        if y > rect.max.y { break; }
        let bg = if i % 2 == 0 { TRACK_AUDIO_BG } else { TRACK_AUDIO_ALT };
        painter.rect_filled(Rect::from_min_size(pos2(clips_left, y), vec2(clips_w, th)), 0.0, bg);
        draw_track_header(&painter, pos2(rect.min.x, y), th, &project.timeline.audio_tracks[i], false);

        let num_clips = project.timeline.audio_tracks[i].clips.len();
        for j in 0..num_clips {
            let clip = &project.timeline.audio_tracks[i].clips[j];
            let x0 = ((clip.position - scroll_x) * pps) as f32 + clips_left;
            let x1 = (((clip.position + clip.duration) - scroll_x) * pps) as f32 + clips_left;
            if x1 > clips_left && x0 < rect.max.x {
                let cr = Rect::from_min_max(
                    pos2(x0.max(clips_left), y + 2.0),
                    pos2(x1.min(rect.max.x), y + th - 2.0),
                );
                let sel = selected_clip.as_ref().is_some_and(|s| !s.is_video && s.track_idx == i && s.clip_idx == j);
                draw_clip(&painter, cr, clip, sel);
            }
        }

        painter.line_segment([pos2(rect.min.x, y + th), pos2(rect.max.x, y + th)], Stroke::new(1.0, SEPARATOR));
        y += th;
    }

    // Markers
    for marker in &project.timeline.markers {
        let mx = ((marker.position - scroll_x) * pps) as f32 + clips_left;
        if mx >= clips_left && mx <= rect.max.x {
            let [r, g, b] = marker.color;
            painter.line_segment(
                [pos2(mx, rect.min.y + RULER_H), pos2(mx, rect.max.y)],
                Stroke::new(1.0, Color32::from_rgba_premultiplied(r, g, b, 120)),
            );
            painter.text(
                pos2(mx + 2.0, rect.min.y + RULER_H + 2.0),
                Align2::LEFT_TOP,
                &marker.name,
                FontId::proportional(9.0),
                Color32::from_rgb(r, g, b),
            );
        }
    }

    // Playhead
    let ph_x = ((project.timeline.cursor_position - scroll_x) * pps) as f32 + clips_left;
    if ph_x >= clips_left && ph_x <= rect.max.x {
        painter.line_segment(
            [pos2(ph_x, rect.min.y), pos2(ph_x, rect.max.y)],
            Stroke::new(2.0, PLAYHEAD),
        );
        painter.add(Shape::convex_polygon(
            vec![
                pos2(ph_x, rect.min.y + RULER_H),
                pos2(ph_x - 5.0, rect.min.y + RULER_H - 8.0),
                pos2(ph_x + 5.0, rect.min.y + RULER_H - 8.0),
            ],
            PLAYHEAD,
            Stroke::NONE,
        ));
    }

    // ---- Interaction ----

    // Zoom
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta);
        let ctrl = ui.input(|i| i.modifiers.ctrl);
        if ctrl && scroll.y.abs() > 0.1 {
            let old_zoom = project.timeline.zoom;
            project.timeline.zoom *= 1.0 + scroll.y as f64 * 0.003;
            project.timeline.zoom = project.timeline.zoom.clamp(0.1, 50.0);
            if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
                let mx = (pos.x - clips_left) as f64;
                let time_at_mouse = mx / (old_zoom * BASE_PPS) + scroll_x;
                let new_mx = (time_at_mouse - scroll_x) * project.timeline.zoom * BASE_PPS;
                project.timeline.scroll_x += (mx - new_mx) / (project.timeline.zoom * BASE_PPS);
            }
        } else if !ctrl {
            project.timeline.scroll_x -= scroll.x as f64 / pps;
            project.timeline.scroll_x = project.timeline.scroll_x.max(0.0);
        }
    }

    // Click
    if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if pos.x > clips_left {
                let mut found = false;
                let mut cy = rect.min.y + RULER_H;

                for i in 0..num_video {
                    let th = project.timeline.video_tracks[i].height;
                    if pos.y >= cy && pos.y < cy + th {
                        for j in 0..project.timeline.video_tracks[i].clips.len() {
                            let c = &project.timeline.video_tracks[i].clips[j];
                            let x0 = ((c.position - scroll_x) * pps) as f32 + clips_left;
                            let x1 = (((c.position + c.duration) - scroll_x) * pps) as f32 + clips_left;
                            if pos.x >= x0 && pos.x <= x1 {
                                *selected_clip = Some(ClipRef { is_video: true, track_idx: i, clip_idx: j });
                                found = true;
                                break;
                            }
                        }
                        break;
                    }
                    cy += th;
                }

                if !found {
                    let mut cy = sep_y + 3.0;
                    for i in 0..num_audio {
                        let th = project.timeline.audio_tracks[i].height;
                        if pos.y >= cy && pos.y < cy + th {
                            for j in 0..project.timeline.audio_tracks[i].clips.len() {
                                let c = &project.timeline.audio_tracks[i].clips[j];
                                let x0 = ((c.position - scroll_x) * pps) as f32 + clips_left;
                                let x1 = (((c.position + c.duration) - scroll_x) * pps) as f32 + clips_left;
                                if pos.x >= x0 && pos.x <= x1 {
                                    *selected_clip = Some(ClipRef { is_video: false, track_idx: i, clip_idx: j });
                                    found = true;
                                    break;
                                }
                            }
                            break;
                        }
                        cy += th;
                    }
                }

                if !found {
                    *selected_clip = None;
                    let time = (pos.x - clips_left) as f64 / pps + scroll_x;
                    project.timeline.cursor_position = time.max(0.0);
                }
            }
        }
    }

    // Context menu
    let sel_snapshot = selected_clip.clone();
    response.context_menu(|ui| {
        if ui.button("Add Video Track").clicked() {
            project.timeline.add_video_track();
            project.mark_modified();
            ui.close_menu();
        }
        if ui.button("Add Audio Track").clicked() {
            project.timeline.add_audio_track();
            project.mark_modified();
            ui.close_menu();
        }
        ui.separator();
        if let Some(sel) = &sel_snapshot {
            if ui.button("Delete Clip").clicked() {
                let tracks = if sel.is_video {
                    &mut project.timeline.video_tracks
                } else {
                    &mut project.timeline.audio_tracks
                };
                if sel.track_idx < tracks.len() && sel.clip_idx < tracks[sel.track_idx].clips.len() {
                    tracks[sel.track_idx].clips.remove(sel.clip_idx);
                    project.mark_modified();
                }
                *selected_clip = None;
                ui.close_menu();
            }
            if ui.button("Split at Cursor").clicked() {
                let cursor = project.timeline.cursor_position;
                let tracks = if sel.is_video {
                    &mut project.timeline.video_tracks
                } else {
                    &mut project.timeline.audio_tracks
                };
                if let Some(track) = tracks.get_mut(sel.track_idx) {
                    if sel.clip_idx < track.clips.len() {
                        let clip = track.clips[sel.clip_idx].clone();
                        if cursor > clip.position && cursor < clip.position + clip.duration {
                            let first_dur = cursor - clip.position;
                            track.clips[sel.clip_idx].duration = first_dur;
                            track.clips[sel.clip_idx].out_point = clip.in_point + first_dur;
                            let mut second = clip;
                            second.id = uuid::Uuid::new_v4().to_string();
                            second.position = cursor;
                            second.duration -= first_dur;
                            second.in_point += first_dur;
                            track.clips.insert(sel.clip_idx + 1, second);
                            project.mark_modified();
                        }
                    }
                }
                *selected_clip = None;
                ui.close_menu();
            }
            ui.separator();
        }
        if ui.button("Add Marker").clicked() {
            project.timeline.markers.push(Marker {
                position: project.timeline.cursor_position,
                name: format!("Marker {}", project.timeline.markers.len() + 1),
                color: [255, 200, 50],
            });
            project.mark_modified();
            ui.close_menu();
        }
    });
}

// ---- Timeline Helpers ----

fn draw_ruler(painter: &Painter, rect: Rect, pps: f64, scroll_x: f64) {
    painter.rect_filled(rect, 0.0, RULER_BG);

    let step = ruler_step(pps);
    let start = (scroll_x / step).floor() as i64;
    let end = ((scroll_x + rect.width() as f64 / pps) / step).ceil() as i64;

    for i in start..=end {
        let time = i as f64 * step;
        let x = ((time - scroll_x) * pps) as f32 + rect.min.x;
        if x < rect.min.x || x > rect.max.x {
            continue;
        }
        painter.line_segment(
            [pos2(x, rect.max.y - 12.0), pos2(x, rect.max.y)],
            Stroke::new(1.0, RULER_TICK),
        );
        painter.text(
            pos2(x + 3.0, rect.center().y - 2.0),
            Align2::LEFT_CENTER,
            format_ruler_time(time),
            FontId::monospace(9.0),
            RULER_TEXT,
        );
        for s in 1..4 {
            let sx = ((time + s as f64 * step / 4.0 - scroll_x) * pps) as f32 + rect.min.x;
            if sx >= rect.min.x && sx <= rect.max.x {
                painter.line_segment(
                    [pos2(sx, rect.max.y - 5.0), pos2(sx, rect.max.y)],
                    Stroke::new(1.0, Color32::from_rgb(50, 50, 58)),
                );
            }
        }
    }
    painter.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, SEPARATOR));
}

fn ruler_step(pps: f64) -> f64 {
    let target_px = 100.0;
    let target_sec = target_px / pps;
    let steps = [0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0];
    for &s in &steps {
        if s >= target_sec {
            return s;
        }
    }
    600.0
}

fn format_ruler_time(sec: f64) -> String {
    let t = sec.abs();
    let m = (t / 60.0).floor() as u64;
    let s = t % 60.0;
    if m > 0 { format!("{m}:{s:05.2}") } else { format!("{s:.2}s") }
}

fn draw_track_header(painter: &Painter, pos: Pos2, height: f32, track: &Track, is_video: bool) {
    let rect = Rect::from_min_size(pos, vec2(HEADER_W, height));
    painter.rect_filled(rect, 0.0, BG_HEADER);

    let indicator = if is_video {
        Color32::from_rgb(80, 80, 160)
    } else {
        Color32::from_rgb(80, 160, 80)
    };
    painter.rect_filled(Rect::from_min_size(pos, vec2(3.0, height)), 0.0, indicator);

    painter.text(
        pos + vec2(10.0, 12.0),
        Align2::LEFT_TOP,
        &track.name,
        FontId::proportional(11.0),
        TEXT_PRIMARY,
    );

    let by = pos.y + height - 18.0;
    let mc = if track.muted { MUTE_COLOR } else { TEXT_DIM };
    let sc = if track.solo { SOLO_COLOR } else { TEXT_DIM };
    let lc = if track.locked { ACCENT } else { TEXT_DIM };
    painter.text(pos2(pos.x + 10.0, by), Align2::LEFT_TOP, "M", FontId::proportional(10.0), mc);
    painter.text(pos2(pos.x + 28.0, by), Align2::LEFT_TOP, "S", FontId::proportional(10.0), sc);
    painter.text(pos2(pos.x + 46.0, by), Align2::LEFT_TOP, "L", FontId::proportional(10.0), lc);

    if !is_video {
        let vx = pos.x + 70.0;
        let vw = HEADER_W - 78.0;
        painter.rect_filled(Rect::from_min_size(pos2(vx, by + 2.0), vec2(vw, 8.0)), 2.0, Color32::from_rgb(30, 30, 35));
        painter.rect_filled(Rect::from_min_size(pos2(vx, by + 2.0), vec2(vw * track.volume, 8.0)), 2.0, Color32::from_rgb(60, 140, 80));
    }

    painter.line_segment(
        [pos2(pos.x + HEADER_W - 1.0, pos.y), pos2(pos.x + HEADER_W - 1.0, pos.y + height)],
        Stroke::new(1.0, SEPARATOR),
    );
}

fn draw_clip(painter: &Painter, rect: Rect, clip: &Clip, selected: bool) {
    let [r, g, b] = clip.color;
    let body = Color32::from_rgb(r / 2, g / 2, b / 2);
    let top = Color32::from_rgb(r, g, b);

    painter.rect_filled(rect, 3.0, body);
    painter.rect_filled(Rect::from_min_size(rect.min, vec2(rect.width(), 3.0)), 0.0, top);

    if selected {
        painter.rect_stroke(rect, 3.0, Stroke::new(2.0_f32, CLIP_SELECTED), StrokeKind::Outside);
    }

    if rect.width() > 30.0 {
        let text_rect = rect.shrink2(vec2(6.0, 0.0));
        painter.text(
            text_rect.left_center(),
            Align2::LEFT_CENTER,
            &clip.name,
            FontId::proportional(10.0),
            Color32::from_rgb(220, 220, 220),
        );
    }

    // Trim handles
    painter.rect_filled(Rect::from_min_size(rect.min, vec2(3.0, rect.height())), 0.0, Color32::from_rgba_premultiplied(255, 255, 255, 25));
    painter.rect_filled(
        Rect::from_min_size(pos2(rect.max.x - 3.0, rect.min.y), vec2(3.0, rect.height())),
        0.0,
        Color32::from_rgba_premultiplied(255, 255, 255, 25),
    );
}
