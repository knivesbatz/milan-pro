use eframe::egui::{self, *};
use crate::audio::{Waveforms, PEAKS_PER_SEC};
use crate::decode::{Preview, Quality};
use crate::edit;
use crate::history::History;
use crate::media::Importer;
use crate::project::*;

// ---- Public Types ----

#[derive(Clone)]
pub struct ClipRef {
    pub is_video: bool,
    pub track_idx: usize,
    pub clip_idx: usize,
}

#[derive(Clone, Copy, PartialEq)]
pub enum DragMode {
    Move,
    TrimLeft,
    TrimRight,
}

pub struct DragState {
    pub clip_ref: ClipRef,
    pub original_position: f64,
    pub mode: DragMode,
    /// Pointer time minus the grabbed edge's time at drag start.
    pub grab_offset: f64,
    pub original: Clip,
    pub limit: Option<f64>,
    pub snap_targets: Vec<f64>,
    /// Project before the drag; pushed to history on release if anything changed.
    pub before: Project,
    pub changed: bool,
}

const EDGE_PX: f32 = 6.0;
const SNAP_PX: f64 = 8.0;

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
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT_PRIMARY);
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(50, 50, 56);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT_PRIMARY);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(60, 60, 68);
    visuals.widgets.active.bg_fill = ACCENT_DIM;
    visuals.selection.bg_fill = ACCENT_DIM;
    visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    visuals.window_stroke = Stroke::new(1.0_f32, SEPARATOR);
    ctx.set_visuals(visuals);
}

// ---- Menu Bar ----

/// Menu items handled by the app (dialogs, and actions that may discard unsaved changes).
#[derive(Clone, Copy, PartialEq)]
pub enum MenuAction {
    RenderAs,
    Properties,
    New,
    Open,
    Import,
    Exit,
    About,
}

pub fn draw_menu_bar(
    ui: &mut Ui,
    project: &mut Project,
    history: &mut History,
    selected_clip: &mut Option<ClipRef>,
) -> Option<MenuAction> {
    let mut action = None;
    egui::menu::bar(ui, |ui| {
        ui.menu_button("File", |ui| {
            if ui.button("New Project").clicked() {
                action = Some(MenuAction::New);
                ui.close_menu();
            }
            if ui.button("Open Project...").clicked() {
                action = Some(MenuAction::Open);
                ui.close_menu();
            }
            ui.separator();
            if ui.button("Save        Ctrl+S").clicked() {
                save(project);
                ui.close_menu();
            }
            if ui.button("Save As...").clicked() {
                save_as(project);
                ui.close_menu();
            }
            ui.separator();
            if ui.button("Render As...  Ctrl+M").clicked() {
                action = Some(MenuAction::RenderAs);
                ui.close_menu();
            }
            ui.separator();
            if ui.button("Import Media...").clicked() {
                action = Some(MenuAction::Import);
                ui.close_menu();
            }
            ui.separator();
            if ui.button("Properties...  Alt+Enter").clicked() {
                action = Some(MenuAction::Properties);
                ui.close_menu();
            }
            ui.separator();
            if ui.button("Exit").clicked() {
                action = Some(MenuAction::Exit);
                ui.close_menu();
            }
        });

        ui.menu_button("Edit", |ui| {
            if ui.add_enabled(history.can_undo(), Button::new("Undo  Ctrl+Z")).clicked() {
                history.undo(project);
                *selected_clip = None;
                ui.close_menu();
            }
            if ui.add_enabled(history.can_redo(), Button::new("Redo  Ctrl+Y")).clicked() {
                history.redo(project);
                *selected_clip = None;
                ui.close_menu();
            }
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
                history.push(project.clone());
                project.timeline.add_video_track();
                project.mark_modified();
                ui.close_menu();
            }
            if ui.button("Audio Track").clicked() {
                history.push(project.clone());
                project.timeline.add_audio_track();
                project.mark_modified();
                ui.close_menu();
            }
            ui.separator();
            if ui.button("Marker at Cursor").clicked() {
                add_marker(project, history);
                ui.close_menu();
            }
        });

        ui.menu_button("Tools", |ui| {
            ui.add_enabled(false, Button::new("Preferences..."));
            ui.add_enabled(false, Button::new("Keyboard Shortcuts..."));
            ui.separator();
            if ui.button("Render As...  Ctrl+M").clicked() {
                action = Some(MenuAction::RenderAs);
                ui.close_menu();
            }
        });

        ui.menu_button("Help", |ui| {
            if ui.button("About Milan Pro").clicked() {
                action = Some(MenuAction::About);
                ui.close_menu();
            }
        });
    });
    action
}

/// Save to the current path, or Save As if the project has none.
pub fn save(project: &mut Project) {
    if let Some(path) = project.path.clone() {
        let _ = project.save(&path);
    } else {
        save_as(project);
    }
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

fn add_marker(project: &mut Project, history: &mut History) {
    history.push(project.clone());
    project.timeline.markers.push(Marker {
        position: project.timeline.cursor_position,
        name: format!("Marker {}", project.timeline.markers.len() + 1),
        color: [255, 200, 50],
    });
    project.mark_modified();
}

/// Pick files and probe them in the background; the app adds the results when they arrive.
pub fn import_media(ctx: &Context, importer: &Importer) {
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
        importer.import(ctx, paths);
    }
}

// ---- Toolbar ----

/// `play_start`: where the current playback began; Stop returns there, as in VEGAS.
pub fn draw_toolbar(ui: &mut Ui, playing: &mut bool, snapping: &mut bool, project: &mut Project, play_start: f64) {
    ui.horizontal(|ui| {
        if ui.button("⏮").on_hover_text("Go to Start").clicked() {
            project.timeline.cursor_position = 0.0;
        }
        let play_label = if *playing { "⏸" } else { "▶" };
        if ui.button(play_label)
            .on_hover_text(if *playing { "Pause (Enter)" } else { "Play (Space)" })
            .clicked()
        {
            *playing = !*playing;
        }
        if ui.button("⏹").on_hover_text("Stop (Space)").clicked() && *playing {
            *playing = false;
            project.timeline.cursor_position = play_start;
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

        ui.toggle_value(snapping, "🧲 Snap").on_hover_text("Enable Snapping (F8)");

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

pub fn draw_status_bar(ui: &mut Ui, project: &Project, message: Option<&str>) {
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
        if let Some(msg) = message {
            ui.separator();
            ui.label(RichText::new(msg).size(11.0).color(MUTE_COLOR));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new("Milan Pro v0.1.0").size(11.0).color(TEXT_DIM));
        });
    });
}

// ---- Media Browser ----

pub fn draw_media_browser(
    ui: &mut Ui,
    project: &mut Project,
    selected: &mut Option<usize>,
    selected_clip: &mut Option<ClipRef>,
    history: &mut History,
    importer: &Importer,
) {
    ui.horizontal(|ui| {
        ui.heading(RichText::new("Media").size(13.0));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.small_button("+").on_hover_text("Import Media").clicked() {
                import_media(ui.ctx(), importer);
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
                import_media(ui.ctx(), importer);
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
            let response = ui.selectable_label(is_selected, &label).interact(Sense::drag());
            if response.clicked() {
                *selected = Some(i);
                *selected_clip = None; // so Properties shows the media
            }
            // Payload is the media pool index; the timeline drops it on release.
            response.dnd_set_drag_payload(i);
            if response.dragged() {
                ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
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

        // Same lock/type checks and linked audio as dropping on the timeline.
        for (idx, is_video) in [(add_to_video, true), (add_to_audio, false)] {
            if let Some(i) = idx {
                let (before, at) = (project.clone(), project.timeline.cursor_position);
                if edit::drop_media(project, i, is_video, 0, at) {
                    history.push(before);
                }
            }
        }
        if let Some(i) = to_remove {
            history.push(project.clone());
            let m = project.media_pool.remove(i);
            // Clips of removed media would play black/silent; remove them in the same undo step.
            let tl = &mut project.timeline;
            for t in tl.video_tracks.iter_mut().chain(&mut tl.audio_tracks) {
                t.clips.retain(|c| c.media_id.as_deref() != Some(m.id.as_str()));
            }
            *selected = None;
            *selected_clip = None;
            project.mark_modified();
        }
    });
}

// ---- Preview ----

pub fn draw_preview(ui: &mut Ui, project: &Project, preview: &mut Preview) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Video Preview").size(13.0).color(TEXT_PRIMARY));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            egui::ComboBox::from_id_salt("preview_quality")
                .selected_text(format!("{:?}", preview.quality))
                .width(80.0)
                .show_ui(ui, |ui| {
                    for q in Quality::ALL {
                        ui.selectable_value(&mut preview.quality, q, format!("{q:?}"));
                    }
                })
                .response
                .on_hover_text("Preview Quality");
        });
    });

    let rect = ui.available_rect_before_wrap();
    let _ = ui.allocate_rect(rect, Sense::hover());
    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, 0.0, Color32::BLACK);

    let aspect = project.settings.width as f32 / project.settings.height.max(1) as f32;
    let preview_rect = fit_rect_to_aspect(rect.shrink(8.0), aspect);
    painter.rect_filled(preview_rect, 0.0, Color32::BLACK);

    if let Some(tex) = &preview.texture {
        let [w, h] = tex.size();
        let r = fit_rect_to_aspect(preview_rect, w as f32 / h.max(1) as f32);
        painter.image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    } else if let Some(err) = &preview.error {
        painter.text(preview_rect.center(), Align2::CENTER_CENTER, err, FontId::proportional(12.0), MUTE_COLOR);
    }

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
    drag_state: &mut Option<DragState>,
    scroll_y: &mut f32,
    history: &mut History,
    snapping: bool,
    waves: &mut Waveforms,
    playing: bool,
) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Timeline").size(13.0).color(TEXT_PRIMARY));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.small_button("+ Audio").clicked() {
                history.push(project.clone());
                project.timeline.add_audio_track();
                project.mark_modified();
            }
            if ui.small_button("+ Video").clicked() {
                history.push(project.clone());
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
    let clips_left = rect.min.x + HEADER_W;
    let clips_w = rect.width() - HEADER_W;
    let rows_top = rect.min.y + RULER_H;

    // Wheel: Ctrl+wheel zooms (egui reports it as zoom_delta, not scroll), wheel scrolls tracks,
    // Shift+wheel scrolls time.
    if response.hovered() {
        let (zoom, scroll) = ui.input(|i| (i.zoom_delta() as f64, i.smooth_scroll_delta));
        let tl = &mut project.timeline;
        if zoom != 1.0 {
            let old_pps = tl.zoom * BASE_PPS;
            tl.zoom = (tl.zoom * zoom).clamp(0.1, 50.0);
            if let Some(pos) = response.hover_pos() {
                // Keep the time under the mouse fixed.
                let mx = (pos.x - clips_left).max(0.0) as f64;
                tl.scroll_x = (tl.scroll_x + mx / old_pps - mx / (tl.zoom * BASE_PPS)).max(0.0);
            }
        }
        tl.scroll_x = (tl.scroll_x - scroll.x as f64 / (tl.zoom * BASE_PPS)).max(0.0);
        *scroll_y -= scroll.y;
    }

    let pps = project.timeline.zoom * BASE_PPS;
    // Page-follow the playhead during playback.
    if playing {
        let tl = &mut project.timeline;
        if tl.cursor_position < tl.scroll_x || tl.cursor_position > tl.scroll_x + clips_w as f64 / pps {
            tl.scroll_x = tl.cursor_position;
        }
    }
    let scroll_x = project.timeline.scroll_x;
    let frame = 1.0 / project.settings.frame_rate.max(1.0);
    let to_x = |t: f64| ((t - scroll_x) * pps) as f32 + clips_left;
    let to_time = |x: f32| (x - clips_left) as f64 / pps + scroll_x;

    let tl = &project.timeline;
    let content_h = tl.video_tracks.iter().chain(&tl.audio_tracks).map(|t| t.height).sum::<f32>() + 3.0;
    *scroll_y = scroll_y.clamp(0.0, (content_h - (rect.max.y - rows_top)).max(0.0));

    // Track rows in screen order: (is_video, track index, top y, height).
    let mut rows: Vec<(bool, usize, f32, f32)> = Vec::new();
    let mut y = rows_top - *scroll_y;
    let mut sep_y = y;
    for is_video in [true, false] {
        for (i, t) in project.timeline.tracks(is_video).iter().enumerate() {
            rows.push((is_video, i, y, t.height));
            y += t.height;
        }
        if is_video {
            sep_y = y;
            y += 3.0;
        }
    }
    // Rows scrolled under the ruler are not hittable.
    let row_at = |py: f32| rows.iter().find(|r| py >= r.2.max(rows_top) && py < r.2 + r.3).map(|r| (r.0, r.1));
    let hit_clip = |project: &Project, p: Pos2| -> Option<(ClipRef, DragMode)> {
        if p.x <= clips_left {
            return None;
        }
        let (is_video, ti) = row_at(p.y)?;
        let clips = &project.timeline.tracks(is_video)[ti].clips;
        // Later clips are drawn on top, so test them first.
        clips.iter().enumerate().rev().find_map(|(j, c)| {
            let (x0, x1) = (to_x(c.position), to_x(c.position + c.duration));
            if p.x < x0 || p.x > x1 {
                return None;
            }
            let mode = if p.x - x0 <= EDGE_PX {
                DragMode::TrimLeft
            } else if x1 - p.x <= EDGE_PX {
                DragMode::TrimRight
            } else {
                DragMode::Move
            };
            Some((ClipRef { is_video, track_idx: ti, clip_idx: j }, mode))
        })
    };
    let is_locked = |project: &Project, cr: &ClipRef| project.timeline.tracks(cr.is_video)[cr.track_idx].locked;

    // ---- Interaction ----

    // Track header buttons (M / S / L, plus V for video) and audio volume bar
    for &(is_video, i, ty, th) in &rows {
        if ty > rect.max.y {
            break;
        }
        let by = ty + th - 18.0;
        if by - 2.0 < rows_top || by + 14.0 > rect.max.y {
            continue; // buttons scrolled out of view
        }
        let id = ui.id().with(("track_hdr", is_video, i));
        let n = if is_video { 4 } else { 3 };
        for (k, bx) in [10.0, 28.0, 46.0, 64.0].into_iter().enumerate().take(n) {
            let r = Rect::from_min_size(pos2(rect.min.x + bx - 3.0, by - 2.0), vec2(16.0, 16.0));
            let tip = ["Mute", "Solo", "Lock", "Show in Preview"][k];
            if ui.interact(r, id.with(k), Sense::click()).on_hover_text(tip).clicked() {
                history.push(project.clone());
                let t = &mut project.timeline.tracks_mut(is_video)[i];
                match k {
                    0 => t.muted = !t.muted,
                    1 => t.solo = !t.solo,
                    2 => t.locked = !t.locked,
                    _ => t.visible = !t.visible,
                }
                project.mark_modified();
            }
        }
        if !is_video {
            let (vx, vw) = (rect.min.x + 70.0, HEADER_W - 78.0);
            let r = Rect::from_min_size(pos2(vx, by - 2.0), vec2(vw, 14.0));
            let resp = ui.interact(r, id.with("vol"), Sense::click_and_drag());
            if resp.drag_started() || resp.clicked() {
                history.push(project.clone());
                project.mark_modified();
            }
            if resp.dragged() || resp.clicked() {
                if let Some(p) = resp.interact_pointer_pos() {
                    project.timeline.audio_tracks[i].volume = ((p.x - vx) / vw).clamp(0.0, 1.0);
                }
            }
            resp.on_hover_text(format!("Volume {:.0}%", project.timeline.audio_tracks[i].volume * 100.0));
        }
    }

    // Trim cursor on clip edges, kept for the whole trim drag
    if let Some(ds) = drag_state.as_ref() {
        if ds.mode != DragMode::Move {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
        }
    } else if let Some(p) = response.hover_pos() {
        if let Some((cr, DragMode::TrimLeft | DragMode::TrimRight)) = hit_clip(project, p) {
            if !is_locked(project, &cr) {
                ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
            }
        }
    }

    // Clip drag: move / trim
    if response.drag_started() {
        if let Some(p) = ui.input(|i| i.pointer.press_origin()) {
            if let Some((cr, mode)) = hit_clip(project, p).filter(|(cr, _)| !is_locked(project, cr)) {
                let clip = project.timeline.tracks(cr.is_video)[cr.track_idx].clips[cr.clip_idx].clone();
                let edge = if mode == DragMode::TrimRight { clip.position + clip.duration } else { clip.position };
                *selected_clip = Some(cr.clone());
                *drag_state = Some(DragState {
                    clip_ref: cr,
                    original_position: clip.position,
                    mode,
                    grab_offset: to_time(p.x) - edge,
                    limit: edit::media_limit(project, &clip),
                    snap_targets: edit::snap_targets(project, Some(&clip.id)),
                    original: clip,
                    before: project.clone(),
                    changed: false,
                });
            }
        }
    }
    if let Some(ds) = drag_state.as_mut() {
        if let Some(p) = response.interact_pointer_pos().filter(|_| response.dragged()) {
            let thr = SNAP_PX / pps;
            let edge = to_time(p.x) - ds.grab_offset;
            let snapped_edge = if snapping { edit::snap(edge, &ds.snap_targets, thr).unwrap_or(edge) } else { edge };

            // Vertical move to another unlocked track of the same kind
            if ds.mode == DragMode::Move {
                if let Some((v, ti)) = row_at(p.y) {
                    let cr = &mut ds.clip_ref;
                    let tracks = project.timeline.tracks_mut(v);
                    if v == cr.is_video && ti != cr.track_idx && !tracks[ti].locked && cr.clip_idx < tracks[cr.track_idx].clips.len() {
                        let c = tracks[cr.track_idx].clips.remove(cr.clip_idx);
                        tracks[ti].clips.push(c);
                        cr.track_idx = ti;
                        cr.clip_idx = tracks[ti].clips.len() - 1;
                        ds.changed = true;
                    }
                }
            }

            let cr = ds.clip_ref.clone();
            let clip = project.timeline.tracks_mut(cr.is_video).get_mut(cr.track_idx).and_then(|t| t.clips.get_mut(cr.clip_idx));
            if let Some(clip) = clip {
                match ds.mode {
                    DragMode::Move => {
                        let mut pos = edge.max(0.0);
                        if snapping {
                            pos = edit::snap_span(pos, clip.duration, &ds.snap_targets, thr).max(0.0);
                        }
                        clip.position = pos;
                    }
                    DragMode::TrimLeft => edit::trim_left(clip, &ds.original, snapped_edge, frame, ds.limit),
                    DragMode::TrimRight => edit::trim_right(clip, &ds.original, snapped_edge, frame, ds.limit),
                }
                ds.changed |= clip.position != ds.original_position || clip.duration != ds.original.duration;
                *selected_clip = Some(cr);
            }
        }
    }
    // Scrub: dragging that started on the ruler moves the cursor
    if drag_state.is_none() && response.dragged() {
        let origin = ui.input(|i| i.pointer.press_origin());
        if let (Some(o), Some(p)) = (origin, response.interact_pointer_pos()) {
            if o.y < rows_top && o.x > clips_left {
                project.timeline.cursor_position = to_time(p.x).max(0.0);
            }
        }
    }
    if response.drag_stopped() {
        if let Some(ds) = drag_state.take() {
            if ds.changed {
                history.push(ds.before);
                project.mark_modified();
            }
        }
    }

    // Drop media from the browser
    // Snapped once so the indicator line shows exactly where the clip will land.
    let drop_time = |project: &Project, x: f32| {
        let t = to_time(x).max(0.0);
        if snapping { edit::snap(t, &edit::snap_targets(project, None), SNAP_PX / pps).unwrap_or(t) } else { t }
    };
    let drop_x = response
        .dnd_hover_payload::<usize>()
        .and_then(|_| ui.input(|i| i.pointer.latest_pos()))
        .filter(|p| p.x > clips_left)
        .map(|p| to_x(drop_time(project, p.x)));
    if let Some(idx) = response.dnd_release_payload::<usize>() {
        if let Some(p) = ui.input(|i| i.pointer.latest_pos()).filter(|p| p.x > clips_left) {
            if let Some((v, ti)) = row_at(p.y) {
                let t = drop_time(project, p.x);
                let before = project.clone();
                if edit::drop_media(project, *idx, v, ti, t) {
                    history.push(before);
                }
            }
        }
    }

    // Click: select the clip under the pointer (or none) and put the cursor there, as VEGAS does.
    // A right-click selects too, so the context menu acts on the clicked clip.
    if response.clicked() || response.secondary_clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if pos.x > clips_left || response.secondary_clicked() {
                *selected_clip = hit_clip(project, pos).map(|(cr, _)| cr);
                if response.clicked() {
                    project.timeline.cursor_position = to_time(pos.x).max(0.0);
                }
            }
        }
    }

    // Context menu
    let sel_snapshot = selected_clip.clone();
    response.context_menu(|ui| {
        if ui.button("Add Video Track").clicked() {
            history.push(project.clone());
            project.timeline.add_video_track();
            project.mark_modified();
            ui.close_menu();
        }
        if ui.button("Add Audio Track").clicked() {
            history.push(project.clone());
            project.timeline.add_audio_track();
            project.mark_modified();
            ui.close_menu();
        }
        ui.separator();
        if let Some(sel) = &sel_snapshot {
            if ui.button("Delete Clip      Del").clicked() {
                let before = project.clone();
                if edit::delete_clip(project, sel) {
                    history.push(before);
                }
                *selected_clip = None;
                ui.close_menu();
            }
            if ui.button("Split at Cursor  S").clicked() {
                let before = project.clone();
                let cursor = project.timeline.cursor_position;
                if edit::split_at(project, sel, cursor) {
                    history.push(before);
                }
                ui.close_menu();
            }
            ui.separator();
        }
        if ui.button("Add Marker").clicked() {
            add_marker(project, history);
            ui.close_menu();
        }
    });

    // ---- Painting (after interaction so edits show this frame) ----

    painter.rect_filled(rect, 0.0, BG_DARKEST);
    painter.rect_filled(
        Rect::from_min_size(rect.min, vec2(HEADER_W, rect.height())),
        0.0,
        BG_HEADER,
    );

    // Tracks and clips
    for &(is_video, i, ty, th) in &rows {
        if ty > rect.max.y {
            break;
        }
        if ty + th < rows_top {
            continue; // scrolled above the ruler
        }
        let track = &project.timeline.tracks(is_video)[i];
        let bg = match (is_video, i % 2 == 0) {
            (true, true) => TRACK_VIDEO_BG,
            (true, false) => TRACK_VIDEO_ALT,
            (false, true) => TRACK_AUDIO_BG,
            (false, false) => TRACK_AUDIO_ALT,
        };
        painter.rect_filled(Rect::from_min_size(pos2(clips_left, ty), vec2(clips_w, th)), 0.0, bg);
        draw_track_header(&painter, pos2(rect.min.x, ty), th, track, is_video);

        for (j, clip) in track.clips.iter().enumerate() {
            let (x0, x1) = (to_x(clip.position), to_x(clip.position + clip.duration));
            if x1 > clips_left && x0 < rect.max.x {
                let cr = Rect::from_min_max(
                    pos2(x0.max(clips_left), ty + 2.0),
                    pos2(x1.min(rect.max.x), ty + th - 2.0),
                );
                let sel = selected_clip
                    .as_ref()
                    .is_some_and(|s| s.is_video == is_video && s.track_idx == i && s.clip_idx == j);
                let media = clip.media_id.as_deref().and_then(|id| project.media_pool.iter().find(|m| m.id == id));
                let wave = media
                    .filter(|m| !is_video && m.sample_rate.is_some())
                    .and_then(|m| waves.get(m))
                    .map(|p| (p, clip.in_point + (to_time(cr.min.x) - clip.position), 1.0 / pps));
                draw_clip(&painter, cr, clip, sel, wave);
            }
        }

        painter.line_segment([pos2(rect.min.x, ty + th), pos2(rect.max.x, ty + th)], Stroke::new(1.0_f32, SEPARATOR));
    }

    // Separator between video and audio
    painter.rect_filled(Rect::from_min_size(pos2(rect.min.x, sep_y), vec2(rect.width(), 3.0)), 0.0, SEPARATOR);

    // Ruler, painted after the tracks so rows scrolled up slide under it
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

    // Markers
    for marker in &project.timeline.markers {
        let mx = to_x(marker.position);
        if mx >= clips_left && mx <= rect.max.x {
            let [r, g, b] = marker.color;
            painter.line_segment(
                [pos2(mx, rect.min.y + RULER_H), pos2(mx, rect.max.y)],
                Stroke::new(1.0_f32, Color32::from_rgba_premultiplied(r, g, b, 120)),
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

    // Media drop indicator
    if let Some(x) = drop_x.filter(|x| *x > clips_left) {
        painter.line_segment([pos2(x, rect.min.y + RULER_H), pos2(x, rect.max.y)], Stroke::new(2.0_f32, ACCENT));
    }

    // Playhead
    let ph_x = to_x(project.timeline.cursor_position);
    if ph_x >= clips_left && ph_x <= rect.max.x {
        painter.line_segment(
            [pos2(ph_x, rect.min.y), pos2(ph_x, rect.max.y)],
            Stroke::new(2.0_f32, PLAYHEAD),
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
            Stroke::new(1.0_f32, RULER_TICK),
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
                    Stroke::new(1.0_f32, Color32::from_rgb(50, 50, 58)),
                );
            }
        }
    }
    painter.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0_f32, SEPARATOR));
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
    if is_video {
        let vc = if track.visible { ACCENT } else { TEXT_DIM };
        painter.text(pos2(pos.x + 64.0, by), Align2::LEFT_TOP, "V", FontId::proportional(10.0), vc);
    }

    if !is_video {
        let vx = pos.x + 70.0;
        let vw = HEADER_W - 78.0;
        painter.rect_filled(Rect::from_min_size(pos2(vx, by + 2.0), vec2(vw, 8.0)), 2.0, Color32::from_rgb(30, 30, 35));
        painter.rect_filled(Rect::from_min_size(pos2(vx, by + 2.0), vec2(vw * track.volume, 8.0)), 2.0, Color32::from_rgb(60, 140, 80));
    }

    painter.line_segment(
        [pos2(pos.x + HEADER_W - 1.0, pos.y), pos2(pos.x + HEADER_W - 1.0, pos.y + height)],
        Stroke::new(1.0_f32, SEPARATOR),
    );
}

/// `wave`: (peaks, source time at rect's left edge, seconds per pixel).
fn draw_clip(painter: &Painter, rect: Rect, clip: &Clip, selected: bool, wave: Option<(&[f32], f64, f64)>) {
    let [r, g, b] = clip.color;
    let body = Color32::from_rgb(r / 2, g / 2, b / 2);
    let top = Color32::from_rgb(r, g, b);

    painter.rect_filled(rect, 3.0, body);
    painter.rect_filled(Rect::from_min_size(rect.min, vec2(rect.width(), 3.0)), 0.0, top);

    if let Some((peaks, t0, spp)) = wave {
        let (mid, half) = (rect.center().y + 1.5, (rect.height() - 7.0) * 0.5);
        let mut x = rect.min.x;
        while x < rect.max.x {
            let t = t0 + (x - rect.min.x) as f64 * spp;
            let a = (t * PEAKS_PER_SEC).max(0.0) as usize;
            let z = (((t + spp) * PEAKS_PER_SEC) as usize).max(a + 1).min(peaks.len());
            let p = peaks.get(a..z).map_or(0.0, |s| s.iter().fold(0.0_f32, |m, v| m.max(*v)));
            if p > 0.0 {
                painter.line_segment([pos2(x, mid - p * half), pos2(x, mid + p * half)], Stroke::new(1.0_f32, top));
            }
            x += 1.0;
        }
    }

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
