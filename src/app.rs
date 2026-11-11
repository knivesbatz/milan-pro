use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};
use crate::audio::{Player, Waveforms};
use crate::autosave::Autosave;
use crate::decode::Preview;
use crate::edit;
use crate::history::History;
use crate::media::Importer;
use crate::project::Project;
use crate::render::{self, RenderDialog};
use crate::settings::SettingsDialog;
use crate::ui::{self, ClipRef, DragState, MenuAction};

pub struct MeridianApp {
    pub project: Project,
    pub history: History,
    pub selected_clip: Option<ClipRef>,
    pub selected_media: Option<usize>,
    pub playing: bool,
    pub snapping: bool,
    pub drag_state: Option<DragState>,
    pub timeline_scroll_y: f32,
    pub preview: Preview,
    pub audio: Player,
    pub waves: Waveforms,
    render: RenderDialog,
    settings: SettingsDialog,
    autosave: Autosave,
    last_frame_time: f64,
    was_playing: bool,
    /// Cursor as set by the playback clock this frame; any other change while playing is a seek.
    clock_pos: f64,
    importer: Importer,
    /// Last import/open error, shown in the status bar.
    message: Option<String>,
    /// Cursor where playback started; Space and Stop return there, as in VEGAS.
    play_start: f64,
    /// Content end the audio session was started with; a change while playing restarts it.
    play_end: f64,
    /// New/Open/Exit waiting on the "Save changes?" prompt.
    confirm: Option<MenuAction>,
    about: bool,
}

impl MeridianApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        ui::apply_theme(&cc.egui_ctx);
        Self {
            project: Project::new("Untitled"),
            history: History::default(),
            selected_clip: None,
            selected_media: None,
            playing: false,
            snapping: true,
            drag_state: None,
            timeline_scroll_y: 0.0,
            preview: Preview::new(&cc.egui_ctx),
            audio: Player::new(),
            waves: Waveforms::new(&cc.egui_ctx),
            render: RenderDialog::default(),
            settings: SettingsDialog::default(),
            autosave: Autosave::default(),
            last_frame_time: 0.0,
            was_playing: false,
            clock_pos: 0.0,
            importer: Importer::new(),
            message: None,
            play_start: 0.0,
            play_end: 0.0,
            confirm: None,
            about: false,
        }
    }

    /// New/Open/Exit, asking to save first if there are unsaved changes.
    fn request(&mut self, ctx: &egui::Context, action: MenuAction) {
        if self.project.modified {
            self.confirm = Some(action);
        } else {
            self.run(ctx, action);
        }
    }

    fn run(&mut self, ctx: &egui::Context, action: MenuAction) {
        let project = match action {
            MenuAction::New => Project::new("Untitled"),
            MenuAction::Open => {
                let Some(path) = rfd::FileDialog::new().add_filter("Milan Pro Project", &["mnle"]).pick_file() else { return };
                match Project::load(&path) {
                    Ok(p) => p,
                    Err(e) => return self.message = Some(format!("Could not open {}: {e}", path.display())),
                }
            }
            _ => {
                // Close (not process::exit) so the app is dropped and a running render's ffmpeg is killed.
                self.project.modified = false; // changes were saved or discarded; don't prompt again
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
        };
        self.project = project;
        self.history.clear();
        self.selected_clip = None;
        self.selected_media = None;
        self.drag_state = None;
        self.playing = false;
        self.message = None;
    }

    fn show_dialogs(&mut self, ctx: &egui::Context) {
        if let Some(action) = self.confirm {
            let (mut save, mut discard) = (false, false);
            let resp = egui::Modal::new(egui::Id::new("save_changes")).show(ctx, |ui| {
                ui.label(format!("Save changes to {}?", self.project.name));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    save = ui.button("Save").clicked();
                    discard = ui.button("Don't Save").clicked();
                    ui.button("Cancel").clicked()
                })
                .inner
            });
            if save || discard || resp.inner || resp.should_close() {
                self.confirm = None;
            }
            if save {
                ui::save(&mut self.project);
            }
            // A cancelled or failed save leaves the project modified, so nothing is discarded.
            if discard || (save && !self.project.modified) {
                self.run(ctx, action);
            }
        }
        egui::Window::new("About Milan Pro")
            .open(&mut self.about)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(format!("Milan Pro v{}", env!("CARGO_PKG_VERSION")));
                ui.label("Open-source nonlinear video editor.");
            });
    }
}

impl eframe::App for MeridianApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Playback clock: audio is master while it plays, frame dt otherwise
        if self.playing {
            let now = ctx.input(|i| i.time);
            let end = render::content_end(&self.project);
            let tl = &mut self.project.timeline;
            if let Some(t) = self.audio.position() {
                tl.cursor_position = t;
            } else if self.last_frame_time > 0.0 {
                tl.cursor_position += now - self.last_frame_time;
            }
            // Playback ends at the last event and stays there.
            if tl.cursor_position >= end {
                tl.cursor_position = end.max(self.play_start);
                self.playing = false;
            }
            self.last_frame_time = now;
            ctx.request_repaint();
        } else {
            self.last_frame_time = 0.0;
        }
        self.clock_pos = self.project.timeline.cursor_position;

        // Keyboard shortcuts (collected first; acted on outside the input lock since Save opens a dialog)
        let typing = ctx.wants_keyboard_input();
        let (mut undo, mut redo, mut save, mut split, mut delete, mut step) = (false, false, false, false, false, 0.0);
        let mut menu_action = None;
        let modal = self.confirm.is_some();
        ctx.input_mut(|i| {
            if modal {
                return;
            }
            let sc = |m, k| KeyboardShortcut::new(m, k);
            // Ctrl+Shift+Z must be consumed before Ctrl+Z, which matches it logically.
            redo = i.consume_shortcut(&sc(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z))
                | i.consume_shortcut(&sc(Modifiers::COMMAND, Key::Y));
            undo = i.consume_shortcut(&sc(Modifiers::COMMAND, Key::Z));
            save = i.consume_shortcut(&sc(Modifiers::COMMAND, Key::S));
            if i.consume_shortcut(&sc(Modifiers::COMMAND, Key::M)) {
                menu_action = Some(MenuAction::RenderAs);
            }
            if i.consume_shortcut(&sc(Modifiers::ALT, Key::Enter)) {
                menu_action = Some(MenuAction::Properties);
            }
            if typing {
                return;
            }
            let plain = i.modifiers.is_none();
            // Space: play / stop back at the start position. Enter: play / pause in place.
            if i.key_pressed(Key::Space) {
                if self.playing {
                    self.project.timeline.cursor_position = self.play_start;
                }
                self.playing = !self.playing;
            }
            if plain && i.key_pressed(Key::Enter) {
                self.playing = !self.playing;
            }
            if i.key_pressed(Key::Home) {
                self.project.timeline.cursor_position = 0.0;
            }
            if i.key_pressed(Key::F8) {
                self.snapping = !self.snapping;
            }
            split = plain && i.key_pressed(Key::S);
            delete = i.key_pressed(Key::Delete);
            if i.key_pressed(Key::ArrowLeft) {
                step -= 1.0;
            }
            if i.key_pressed(Key::ArrowRight) {
                step += 1.0;
            }
        });

        if undo || redo {
            let done = if undo { self.history.undo(&mut self.project) } else { self.history.redo(&mut self.project) };
            if done {
                self.selected_clip = None;
                self.drag_state = None;
            }
        }
        if save {
            ui::save(&mut self.project);
        }
        if step != 0.0 {
            let frame = 1.0 / self.project.settings.frame_rate.max(1.0);
            let tl = &mut self.project.timeline;
            tl.cursor_position = (tl.cursor_position + step * frame).max(0.0);
        }
        // Not mid-drag: the drag holds a ClipRef and snapshot that the edit would invalidate.
        if let Some(cr) = self.selected_clip.clone().filter(|_| (split || delete) && self.drag_state.is_none()) {
            let before = self.project.clone();
            let cursor = self.project.timeline.cursor_position;
            if split && edit::split_at(&mut self.project, &cr, cursor) {
                self.history.push(before);
            } else if delete && edit::delete_clip(&mut self.project, &cr) {
                self.history.push(before);
                self.selected_clip = None;
            }
        }

        // Title
        let title = if self.project.modified {
            format!("Milan Pro - {}*", self.project.name)
        } else {
            format!("Milan Pro - {}", self.project.name)
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));

        // Panels
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            let a = ui::draw_menu_bar(ui, &mut self.project, &mut self.history, &mut self.selected_clip);
            menu_action = menu_action.or(a);
        });
        if ctx.input(|i| i.viewport().close_requested()) && self.project.modified {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(MenuAction::Exit);
        }
        match menu_action {
            Some(MenuAction::RenderAs) => self.render.open = true,
            Some(MenuAction::Properties) => self.settings.open(&self.project),
            Some(MenuAction::Import) => ui::import_media(ctx, &self.importer),
            Some(MenuAction::About) => self.about = true,
            Some(a) => self.request(ctx, a),
            None => {}
        }
        self.show_dialogs(ctx);
        while let Some(batch) = self.importer.poll() {
            let before = self.project.clone();
            let mut errors = Vec::new();
            for r in batch {
                match r {
                    Ok(m) => self.project.media_pool.push(m),
                    Err(e) => errors.push(e),
                }
            }
            if self.project.media_pool.len() != before.media_pool.len() {
                self.history.push(before);
                self.project.mark_modified();
            }
            self.message = (!errors.is_empty()).then(|| format!("Import failed: {}", errors.join("; ")));
        }
        self.render.show(ctx, &self.project);
        self.settings.show(ctx, &mut self.project, &mut self.history);
        self.autosave.tick(ctx, &self.project);

        egui::TopBottomPanel::top("toolbar").exact_height(36.0).show(ctx, |ui| {
            ui::draw_toolbar(ui, &mut self.playing, &mut self.snapping, &mut self.project, self.play_start);
        });

        egui::TopBottomPanel::bottom("status_bar").exact_height(24.0).show(ctx, |ui| {
            let msg = self.message.clone().or_else(|| self.audio.status()).or_else(|| self.autosave.error());
            ui::draw_status_bar(ui, &self.project, msg.as_deref());
        });

        egui::SidePanel::left("media_browser")
            .default_width(220.0)
            .min_width(160.0)
            .show(ctx, |ui| {
                ui::draw_media_browser(
                    ui,
                    &mut self.project,
                    &mut self.selected_media,
                    &mut self.selected_clip,
                    &mut self.history,
                    &self.importer,
                );
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
                ui::draw_preview(ui, &self.project, &mut self.preview);
            });

            ui.add_space(2.0);

            ui::draw_timeline(
                ui,
                &mut self.project,
                &mut self.selected_clip,
                &mut self.drag_state,
                &mut self.timeline_scroll_y,
                &mut self.history,
                self.snapping,
                &mut self.waves,
                self.playing,
            );
        });
        // A clip selection replaces a media selection in Properties.
        if self.selected_clip.is_some() {
            self.selected_media = None;
        }

        // After the timeline so this frame's cursor/clip edits are what gets requested.
        self.preview.update(ctx, &self.project);

        // Start/stop audio on play state changes. While playing, restart it on seeks, when the
        // content end changes (the session only decodes up to the end it started with), and
        // while the output device is still opening (otherwise this whole run stays silent).
        let cursor = self.project.timeline.cursor_position;
        let end = render::content_end(&self.project);
        if self.playing && !self.was_playing {
            self.play_start = cursor;
        }
        let restart = self.playing
            && (cursor != self.clock_pos
                || end != self.play_end
                || (self.audio.position().is_none() && self.audio.device_pending()));
        if self.playing != self.was_playing || restart {
            self.audio.stop();
            if self.playing {
                self.audio.start(ctx, &self.project, cursor);
            }
            self.was_playing = self.playing;
            self.play_end = end;
        }
    }
}
