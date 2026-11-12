//! Command execution for the app: keys (through the keymap), menu bar, context menus and toolbars all end up in
//! `MeridianApp::exec`. Also the modal dialogs and the save prompt.
use eframe::egui;
use crate::edit;
use crate::project::{Project, TrackKind};
use crate::render::content_end;
use crate::ui::dialogs::{self, WelcomeResult};
use crate::ui::dock::{Dock, Panel};
use crate::ui::keymap::{Command, Command::*, Scope};
use crate::ui::menus::{self, CmdState};
use crate::ui::theme::{self, UiLook};
use crate::ui::{self, MenuAction, Playback};
use super::MeridianApp;

impl MeridianApp {
    /// A modal dialog is up: shortcuts and drops wait.
    pub(super) fn modal_open(&self) -> bool {
        self.confirm.is_some()
            || self.about
            || self.settings.is_open()
            || self.prefs_dlg.is_open()
            || self.render.open
            || self.welcome.is_some()
            || self.shortcuts_open
    }

    pub(super) fn cmd_state(&self) -> CmdState {
        let clip = self.selected_clip.as_ref().and_then(|cr| self.project.timeline.tracks(cr.is_video).get(cr.track_idx)?.clips.get(cr.clip_idx));
        CmdState {
            look: theme::get().look(),
            can_undo: self.history.can_undo(),
            can_redo: self.history.can_redo(),
            clip: clip.is_some(),
            clip_locked: clip.is_some_and(|c| c.locked),
            clipboard: self.clipboard.is_some(),
            selection: self.project.timeline.loop_region.is_some_and(|(a, b)| b > a),
            looping: self.looping,
            snapping: self.snapping,
            auto_crossfade: self.auto_crossfade,
            open_panels: Panel::ALL.into_iter().filter(|p| self.dock.is_open(*p)).collect(),
            quality: Some(self.preview.quality),
            recent: self.prefs.recent.clone(),
            space_pauses: self.prefs.space_pauses,
        }
    }

    /// Keyboard shortcuts for this frame, run before the UI is drawn (as the old hard-wired keys were).
    pub(super) fn handle_keys(&mut self, ctx: &egui::Context) {
        let blocked = ctx.wants_keyboard_input() || menus::any_open(ctx) || self.modal_open();
        let over_dock = ctx.pointer_hover_pos().is_some_and(|p| self.dock_rect.contains(p));
        let look = theme::get().look();
        for c in self.keymap.poll(ctx, look, Scope { blocked, over_dock }) {
            self.exec(ctx, c);
        }
    }

    /// Commands queued by menus and context menus this frame.
    pub(super) fn run_bus(&mut self, ctx: &egui::Context) {
        for c in menus::drain(ctx) {
            self.exec(ctx, c);
        }
    }

    fn view_secs(&self, ctx: &egui::Context) -> f64 {
        let lanes_w = (ctx.screen_rect().width() - self.dock.track_list_w - 30.0).max(100.0) as f64;
        lanes_w / self.pps()
    }

    fn pps(&self) -> f64 {
        self.project.timeline.zoom * ui::BASE_PPS
    }

    /// Zoom by `f` keeping the cursor at the same place on screen.
    fn zoom_by(&mut self, f: f64) {
        let tl = &mut self.project.timeline;
        let px = (tl.cursor_position - tl.scroll_x) * tl.zoom * ui::BASE_PPS;
        tl.zoom = (tl.zoom * f).clamp(0.0005, 400.0);
        tl.scroll_x = (tl.cursor_position - px / (tl.zoom * ui::BASE_PPS)).max(0.0);
    }

    fn edit_selected(&mut self, f: impl FnOnce(&mut Project, &ui::ClipRef) -> bool) -> bool {
        let Some(cr) = self.selected_clip.clone().filter(|_| self.drag_state.is_none()) else { return false };
        let before = self.project.clone();
        let done = f(&mut self.project, &cr);
        if done {
            self.history.push(before);
        }
        done
    }

    fn save_prefs(&mut self) {
        if !self.demo {
            if let Err(e) = self.prefs.save() {
                self.message = Some(format!("Could not save preferences: {e}"));
            }
        }
    }

    fn remember(&mut self) {
        if let Some(p) = self.project.path.clone() {
            self.prefs.add_recent(&p);
            self.save_prefs();
        }
    }

    pub(super) fn exec(&mut self, ctx: &egui::Context, c: Command) {
        let st = self.cmd_state();
        if !st.enabled(c) {
            return;
        }
        let v16 = st.look == UiLook::Vegas16;
        let frame = 1.0 / self.project.settings.frame_rate.max(1.0);
        match c {
            // File
            New => {
                self.props_after_new = true;
                self.request(ctx, MenuAction::New);
            }
            NewEmpty | Close => self.request(ctx, MenuAction::New),
            Open => self.request(ctx, MenuAction::Open),
            OpenRecent(i) => {
                self.pending_open = self.prefs.recent.get(i).map(Into::into);
                self.request(ctx, MenuAction::Open);
            }
            Save => {
                if ui::save(&mut self.project) {
                    self.remember();
                }
            }
            SaveAs => {
                if menus::save_as(&mut self.project) {
                    self.remember();
                }
            }
            RenderAs => self.render.open = true,
            ImportMedia => ui::import_media(ctx, &self.importer),
            Properties => self.settings.open(&self.project),
            Welcome => self.welcome = Some(Default::default()),
            Exit => self.request(ctx, MenuAction::Exit),
            Preferences => self.prefs_dlg.open(&self.prefs),
            KeyboardShortcuts => self.shortcuts_open = true,
            About => self.about = true,
            // Edit
            Undo | Redo | UndoAll => {
                let mut done = false;
                loop {
                    let step = if c == Redo { self.history.redo(&mut self.project) } else { self.history.undo(&mut self.project) };
                    done |= step;
                    if !step || c != UndoAll {
                        break;
                    }
                }
                if done {
                    self.selected_clip = None;
                    self.drag_state = None;
                }
            }
            ClearHistory => self.history.clear(),
            Copy | Cut => {
                let Some(cr) = self.selected_clip.clone() else { return };
                let Some(clip) = self.project.timeline.tracks(cr.is_video).get(cr.track_idx).and_then(|t| t.clips.get(cr.clip_idx)) else { return };
                self.clipboard = Some((cr.is_video, clip.clone()));
                if c == Cut && self.edit_selected(edit::delete_clip) {
                    self.selected_clip = None;
                }
            }
            Paste => {
                // Onto the selected event's track when it is the same kind, else the first track of that kind.
                let Some((video, mut clip)) = self.clipboard.clone() else { return };
                let track = self.selected_clip.as_ref().filter(|cr| cr.is_video == video).map_or(0, |cr| cr.track_idx);
                clip.id = uuid::Uuid::new_v4().to_string();
                clip.position = self.project.timeline.cursor_position;
                clip.locked = false;
                let before = self.project.clone();
                let Some(t) = self.project.timeline.tracks_mut(video).get_mut(track).filter(|t| !t.locked) else { return };
                t.clips.push(clip);
                let idx = t.clips.len() - 1;
                self.project.mark_modified();
                self.history.push(before);
                self.selected_clip = Some(ui::ClipRef { is_video: video, track_idx: track, clip_idx: idx });
            }
            Delete => {
                if self.edit_selected(edit::delete_clip) {
                    self.selected_clip = None;
                }
            }
            Split => {
                let at = self.project.timeline.cursor_position;
                self.edit_selected(|p, cr| edit::split_at(p, cr, at));
            }
            Trim | TrimStart | TrimEnd => {
                let tl = &self.project.timeline;
                let (a, b) = match c {
                    Trim => tl.loop_region.map_or((None, None), |(a, b)| (Some(a), Some(b))),
                    TrimStart => (Some(tl.cursor_position), None),
                    _ => (None, Some(tl.cursor_position)),
                };
                self.edit_selected(|p, cr| edit::trim_to(p, cr, a, b));
            }
            EventLock => {
                self.edit_selected(edit::toggle_clip_lock);
            }
            EventOpenInTrimmer | EventSelectInMedia => {
                let Some(cr) = &self.selected_clip else { return };
                let Some(clip) = self.project.timeline.tracks(cr.is_video).get(cr.track_idx).and_then(|t| t.clips.get(cr.clip_idx)) else { return };
                let Some(idx) = self.project.media_pool.iter().position(|m| Some(&m.id) == clip.media_id.as_ref()) else { return };
                if c == EventOpenInTrimmer {
                    self.trimmer.open(self.project.media_pool[idx].path.clone());
                    self.dock.open(Panel::Trimmer);
                } else {
                    (self.selected_media, self.selected_clip) = (Some(idx), None);
                    self.dock.open(Panel::ProjectMedia);
                }
            }
            EventProperties => self.dock.open(Panel::Properties),
            // Playback
            PlayStop | PlayAnyWindow if v16 && !self.prefs.space_pauses => {
                if self.playing {
                    self.project.timeline.cursor_position = self.play_start;
                }
                self.playing = !self.playing;
                self.shuttle = 0.0;
            }
            PlayStop | PlayAnyWindow | PlayPause => {
                self.playing = !self.playing;
                self.shuttle = 0.0;
            }
            SpacePlayPause => {
                self.prefs.space_pauses = !self.prefs.space_pauses;
                self.save_prefs();
            }
            PlayFromStart | Stop | Loop | GoToStart | GoToEnd | PrevFrame | NextFrame => {
                let cmd = match c {
                    PlayFromStart => ui::Cmd::PlayFromStart,
                    Stop => ui::Cmd::Stop,
                    Loop => ui::Cmd::Loop,
                    GoToStart => ui::Cmd::GoToStart,
                    GoToEnd => ui::Cmd::GoToEnd,
                    PrevFrame => ui::Cmd::PrevFrame,
                    _ => ui::Cmd::NextFrame,
                };
                if c == Stop {
                    self.shuttle = 0.0;
                }
                let mut pb = Playback { playing: &mut self.playing, looping: &mut self.looping, play_start: self.play_start };
                ui::run_cmd(cmd, &mut self.project, &mut pb);
            }
            ShuttleForward | ShuttleReverse => {
                // J / L: 1×, 2×, 4×, 8× each way; 1× forward is normal playback with sound.
                let cur = if self.shuttle == 1.0 && !self.playing { 0.0 } else { self.shuttle };
                self.shuttle = if c == ShuttleForward {
                    if cur <= 0.0 { 1.0 } else { (cur * 2.0).min(8.0) }
                } else if cur >= 0.0 {
                    -1.0
                } else {
                    (cur * 2.0).max(-8.0)
                };
                self.playing = self.shuttle == 1.0;
            }
            ShuttlePause => {
                (self.shuttle, self.playing) = (0.0, false);
            }
            // Cursor
            SelOrViewStart | SelOrViewEnd => {
                let view = self.view_secs(ctx);
                let tl = &mut self.project.timeline;
                tl.cursor_position = match (c, tl.loop_region.filter(|(a, b)| b > a)) {
                    (SelOrViewStart, Some((a, _))) => a,
                    (SelOrViewStart, None) => tl.scroll_x,
                    (_, Some((_, b))) => b,
                    (_, None) => tl.scroll_x + view,
                };
            }
            PixelLeft | PixelRight => {
                let d = if c == PixelLeft { -1.0 } else { 1.0 } / self.pps();
                let tl = &mut self.project.timeline;
                tl.cursor_position = (tl.cursor_position + d).max(0.0);
            }
            PrevMarker | NextMarker | PrevEditPoint | NextEditPoint => {
                let tl = &self.project.timeline;
                let points: Vec<f64> = if matches!(c, PrevMarker | NextMarker) {
                    tl.markers.iter().map(|m| m.position).chain(tl.regions.iter().flat_map(|r| [r.start, r.end])).collect()
                } else {
                    tl.video_tracks.iter().chain(&tl.audio_tracks).flat_map(|t| &t.clips).flat_map(|c| [c.position, c.position + c.duration]).collect()
                };
                let cur = tl.cursor_position;
                let eps = frame * 0.01;
                let next = if matches!(c, PrevMarker | PrevEditPoint) {
                    points.into_iter().filter(|&p| p < cur - eps).reduce(f64::max)
                } else {
                    points.into_iter().filter(|&p| p > cur + eps).reduce(f64::min)
                };
                if let Some(p) = next {
                    self.project.timeline.cursor_position = p;
                }
            }
            SelEventStart | SelEventEnd => {
                let Some(cr) = &self.selected_clip else { return };
                if let Some(cl) = self.project.timeline.tracks(cr.is_video).get(cr.track_idx).and_then(|t| t.clips.get(cr.clip_idx)) {
                    self.project.timeline.cursor_position = if c == SelEventStart { cl.position } else { cl.position + cl.duration };
                }
            }
            Marker(n) => {
                if let Some(m) = self.project.timeline.markers.get(n - 1) {
                    self.project.timeline.cursor_position = m.position;
                }
            }
            CenterView => {
                let view = self.view_secs(ctx);
                let tl = &mut self.project.timeline;
                tl.scroll_x = (tl.cursor_position - view * 0.5).max(0.0);
            }
            // Selection
            SetSelStart | SetSelEnd => {
                let tl = &mut self.project.timeline;
                let cur = tl.cursor_position;
                let (a, b) = tl.loop_region.unwrap_or((cur, cur));
                tl.loop_region = Some(if c == SetSelStart { (cur, b.max(cur)) } else { (a.min(cur), cur) });
            }
            SelectionToView => {
                let view = self.view_secs(ctx);
                let tl = &mut self.project.timeline;
                tl.loop_region = Some((tl.scroll_x, tl.scroll_x + view));
            }
            SelectionToProject => {
                let end = content_end(&self.project);
                self.project.timeline.loop_region = (end > 0.0).then_some((0.0, end));
            }
            ZoomSelection => {
                let view_px = self.view_secs(ctx) * self.pps();
                let tl = &mut self.project.timeline;
                if let Some((a, b)) = tl.loop_region.filter(|(a, b)| b > a) {
                    tl.zoom = (view_px / ((b - a) * ui::BASE_PPS)).clamp(0.0005, 400.0);
                    tl.scroll_x = a;
                }
            }
            // Markers, tracks
            InsertMarker => ui::add_marker(&mut self.project, &mut self.history),
            InsertRegion => ui::add_region(&mut self.project, &mut self.history),
            InsertAudioTrack | InsertVideoTrack => {
                self.history.push(self.project.clone());
                if c == InsertAudioTrack {
                    self.project.timeline.add_audio_track();
                } else {
                    self.project.timeline.add_video_track();
                }
                self.project.mark_modified();
            }
            Snapping => self.snapping = !self.snapping,
            AutoCrossfade => self.auto_crossfade = !self.auto_crossfade,
            // Zoom
            ZoomIn => self.zoom_by(1.25),
            ZoomOut => self.zoom_by(0.8),
            ZoomOutLarge => self.zoom_by(0.25),
            TracksTaller | TracksShorter | DefaultTrackHeights => {
                let t = theme::get();
                let tl = &mut self.project.timeline;
                for tr in tl.video_tracks.iter_mut().chain(&mut tl.audio_tracks) {
                    let h = if tr.height > 0.0 { tr.height } else if tr.kind == TrackKind::Video { t.m.video_track_h } else { t.m.audio_track_h };
                    tr.height = match c {
                        TracksTaller => (h * 1.25).min(400.0),
                        TracksShorter => (h / 1.25).max(22.0),
                        _ => 0.0,
                    };
                }
            }
            PreviewQuality(q) => self.preview.quality = q,
            LayoutDefault => self.dock = Dock::new(&theme::get()),
            Window(p) => self.dock.toggle(p),
            _ => {}
        }
    }

    /// New/Open/Exit, asking to save first if there are unsaved changes.
    pub(super) fn request(&mut self, ctx: &egui::Context, action: MenuAction) {
        if self.project.modified {
            self.confirm = Some(action);
        } else {
            self.run(ctx, action);
        }
    }

    fn run(&mut self, ctx: &egui::Context, action: MenuAction) {
        let project = match action {
            MenuAction::New => {
                let mut p = Project::new("Untitled");
                if let Some(s) = self.new_settings.take() {
                    p.settings = s;
                }
                p
            }
            MenuAction::Open => {
                let path = match self.pending_open.take() {
                    Some(p) => p,
                    None => match rfd::FileDialog::new().add_filter("Milan Pro Project", &["mnle"]).pick_file() {
                        Some(p) => p,
                        None => return,
                    },
                };
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
        if action == MenuAction::Open {
            self.remember();
        }
        if std::mem::take(&mut self.props_after_new) && action == MenuAction::New {
            self.settings.open(&self.project);
        }
    }

    /// Save prompt, About, Preferences, Welcome and Keyboard Shortcuts.
    pub(super) fn show_dialogs(&mut self, ctx: &egui::Context) {
        if let Some(action) = self.confirm {
            let name = self.project.name.clone();
            let (hit, close) = dialogs::modal(ctx, "save_changes", "Milan Pro", egui::vec2(380.0, 150.0), |ui| {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::hover());
                    let d = theme::get().dialog();
                    ui.painter().circle_filled(r.center(), 14.0, d.accent);
                    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, "?", egui::FontId::proportional(20.0), d.accent_text);
                    ui.label(format!("Do you want to save the changes to {name}?"));
                });
                ui.add_space(18.0);
                dialogs::button_row(ui, &[("Yes", true), ("No", true), ("Cancel", true)])
            });
            let (save, discard) = (hit == Some(0), hit == Some(1));
            if hit.is_some() || close {
                self.confirm = None;
            }
            if hit == Some(2) || close {
                // Cancelled: forget what the New/Open was going to use.
                (self.pending_open, self.new_settings, self.props_after_new) = (None, None, false);
            }
            if save && ui::save(&mut self.project) {
                self.remember();
            }
            // A cancelled or failed save leaves the project modified, so nothing is discarded.
            if discard || (save && !self.project.modified) {
                self.run(ctx, action);
            }
        }
        if self.about {
            let (ok, close) = dialogs::modal(ctx, "about", "About Milan Pro", egui::vec2(360.0, 170.0), |ui| {
                ui.label(egui::RichText::new(format!("Milan Pro v{}", env!("CARGO_PKG_VERSION"))).strong());
                ui.label("Open-source nonlinear video editor.");
                ui.add_space(30.0);
                dialogs::button_row(ui, &[("OK", true)]).is_some()
            });
            self.about = !(ok || close);
        }
        if let Some(new) = self.prefs_dlg.show(ctx, &self.prefs) {
            if new.track_colors != self.prefs.track_colors {
                theme::set_palette(ctx, new.palette());
            }
            self.look = new.look;
            self.prefs = new;
            self.save_prefs();
        }
        dialogs::shortcuts_window(ctx, &mut self.shortcuts_open);
        if let Some(w) = &mut self.welcome {
            let mut show = self.prefs.show_welcome;
            let res = w.show(ctx, &self.prefs.recent, &mut show, self.project.settings.sample_rate);
            if show != self.prefs.show_welcome {
                self.prefs.show_welcome = show;
                self.save_prefs();
            }
            if let Some(r) = res {
                self.welcome = None;
                match r {
                    WelcomeResult::Create(settings, advanced) => {
                        // Through the save prompt like File › New; `run` applies the settings.
                        self.new_settings = Some(settings);
                        self.props_after_new = advanced;
                        self.request(ctx, MenuAction::New);
                    }
                    WelcomeResult::Open => self.request(ctx, MenuAction::Open),
                    WelcomeResult::OpenRecent(i) => {
                        self.pending_open = self.prefs.recent.get(i).map(Into::into);
                        self.request(ctx, MenuAction::Open);
                    }
                    WelcomeResult::Close => {}
                }
            }
        }
    }

    /// J / L shuttle at rates other than normal playback: the cursor moves silently.
    pub(super) fn tick_shuttle(&mut self, ctx: &egui::Context) {
        if self.shuttle == 0.0 || self.shuttle == 1.0 {
            return;
        }
        let dt = ctx.input(|i| i.stable_dt).min(0.1) as f64;
        let tl = &mut self.project.timeline;
        tl.cursor_position = (tl.cursor_position + self.shuttle * dt).max(0.0);
        if tl.cursor_position == 0.0 {
            self.shuttle = 0.0;
        }
        ctx.request_repaint();
    }

    /// `MILAN_DIALOG` (screenshot harness): open a dialog or menu at startup.
    pub(super) fn open_from_env(&mut self, ctx: &egui::Context) {
        let Ok(what) = std::env::var("MILAN_DIALOG") else { return };
        match what.as_str() {
            "prefs" => self.prefs_dlg.open(&self.prefs),
            "prefs-display" => self.prefs_dlg.open_display(&self.prefs),
            "props" => self.settings.open(&self.project),
            "render" => self.render.open = true,
            "save" => self.confirm = Some(MenuAction::Exit),
            "welcome" => self.welcome = Some(Default::default()),
            "shortcuts" => self.shortcuts_open = true,
            "about" => self.about = true,
            m => {
                if let Some(k) = m.strip_prefix("menu:").and_then(|k| k.parse::<usize>().ok()) {
                    menus::request_open(ctx, k);
                }
            }
        }
    }
}
