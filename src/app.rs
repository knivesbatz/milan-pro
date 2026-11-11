use eframe::egui;
use crate::project::Project;
use crate::ui::{self, ClipRef, DragState};

pub struct MeridianApp {
    pub project: Project,
    pub selected_clip: Option<ClipRef>,
    pub selected_media: Option<usize>,
    pub playing: bool,
    pub drag_state: Option<DragState>,
    pub timeline_scroll_y: f32,
    last_frame_time: f64,
}

impl MeridianApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        ui::apply_theme(&cc.egui_ctx);
        Self {
            project: Project::new("Untitled"),
            selected_clip: None,
            selected_media: None,
            playing: false,
            drag_state: None,
            timeline_scroll_y: 0.0,
            last_frame_time: 0.0,
        }
    }
}

impl eframe::App for MeridianApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Playback clock
        if self.playing {
            let now = ctx.input(|i| i.time);
            if self.last_frame_time > 0.0 {
                let dt = now - self.last_frame_time;
                self.project.timeline.cursor_position += dt;
                if self.project.timeline.cursor_position >= self.project.timeline.duration() {
                    self.project.timeline.cursor_position = 0.0;
                    self.playing = false;
                }
            }
            self.last_frame_time = now;
            ctx.request_repaint();
        } else {
            self.last_frame_time = 0.0;
        }

        // Keyboard shortcuts
        ctx.input(|i| {
            if i.key_pressed(egui::Key::Space) {
                self.playing = !self.playing;
            }
            if i.key_pressed(egui::Key::Home) {
                self.project.timeline.cursor_position = 0.0;
            }
            if i.key_pressed(egui::Key::Delete) {
                if let Some(ref cr) = self.selected_clip {
                    let tracks = if cr.is_video {
                        &mut self.project.timeline.video_tracks
                    } else {
                        &mut self.project.timeline.audio_tracks
                    };
                    if cr.track_idx < tracks.len() && cr.clip_idx < tracks[cr.track_idx].clips.len() {
                        tracks[cr.track_idx].clips.remove(cr.clip_idx);
                        self.project.mark_modified();
                    }
                    self.selected_clip = None;
                }
            }
        });

        // Title
        let title = if self.project.modified {
            format!("Milan Pro - {}*", self.project.name)
        } else {
            format!("Milan Pro - {}", self.project.name)
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));

        // Panels
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            ui::draw_menu_bar(ui, &mut self.project);
        });

        egui::TopBottomPanel::top("toolbar").exact_height(36.0).show(ctx, |ui| {
            ui::draw_toolbar(ui, &mut self.playing, &mut self.project);
        });

        egui::TopBottomPanel::bottom("status_bar").exact_height(24.0).show(ctx, |ui| {
            ui::draw_status_bar(ui, &self.project);
        });

        egui::SidePanel::left("media_browser")
            .default_width(220.0)
            .min_width(160.0)
            .show(ctx, |ui| {
                ui::draw_media_browser(ui, &mut self.project, &mut self.selected_media);
            });

        egui::SidePanel::right("properties")
            .default_width(240.0)
            .min_width(180.0)
            .show(ctx, |ui| {
                ui::draw_properties(ui, &self.project, &self.selected_clip, &self.selected_media);
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            let h = ui.available_height();
            let preview_h = (h * 0.38).max(180.0).min(h - 120.0);

            ui.allocate_ui(egui::vec2(ui.available_width(), preview_h), |ui| {
                ui::draw_preview(ui, &self.project, self.playing);
            });

            ui.add_space(2.0);

            ui::draw_timeline(
                ui,
                &mut self.project,
                &mut self.selected_clip,
                &mut self.drag_state,
                &mut self.timeline_scroll_y,
            );
        });
    }
}
