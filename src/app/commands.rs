//! Command execution for the app: keys (through the keymap), menu bar, context menus and toolbars all end up in
//! `MeridianApp::exec`. Also the modal dialogs and the save prompt.
use eframe::egui;
use crate::edit;
use crate::project::{Project, TrackKind};
use crate::render::content_end;
use crate::ui::dialogs::{self, UpdateResult, WelcomeResult};
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
        let clip = self.sel.clips.last().and_then(|id| self.project.clip(id));
        let tl = &self.project.timeline;
        CmdState {
            look: theme::get().look(),
            can_undo: self.history.can_undo(),
            can_redo: self.history.can_redo(),
            clip: clip.is_some(),
            clip_locked: clip.is_some_and(|c| c.locked),
            n_sel: self.sel.clips.len(),
            any_tracks: !(tl.video_tracks.is_empty() && tl.audio_tracks.is_empty()),
            tracks_sel: !self.sel.tracks.is_empty(),
            list_focus: self.sel.list_focus,
            quantize: self.prefs.quantize,
            clipboard: !self.clipboard.is_empty(),
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

    /// One undoable edit (not while a timeline drag holds its own undo step).
    fn edit(&mut self, f: impl FnOnce(&mut Project) -> bool) -> bool {
        if self.drag_state.is_some() {
            return false;
        }
        let before = self.project.clone();
        let done = f(&mut self.project);
        if done {
            self.history.push(before);
        }
        done
    }

    fn edit_selected(&mut self, f: impl FnOnce(&mut Project, &ui::ClipRef) -> bool) -> bool {
        let Some(cr) = self.selected_clip.clone() else { return false };
        self.edit(|p| f(p, &cr))
    }

    /// `t` on the frame grid when Quantize to Frames is on: edits at the cursor or selection (Split, Trim, Paste)
    /// land on frames even when the cursor does not (playback stop, ruler scrub).
    fn quantized(&self, t: f64) -> f64 {
        if self.prefs.quantize { edit::quantize(t, self.project.settings.frame_rate) } else { t }
    }

    /// Index of the focus track (the last selected one), with its kind.
    fn focus_track(&self) -> Option<(bool, usize)> {
        self.sel.tracks.last().and_then(|id| self.project.find_track(id))
    }

    /// Remember a saved project; show why a save failed. True when saved.
    fn save_result(&mut self, r: Result<bool, String>) -> bool {
        match r {
            Ok(saved) => {
                if saved {
                    self.remember();
                }
                saved
            }
            Err(e) => {
                self.message = Some(format!("Save failed: {e}"));
                false
            }
        }
    }

    fn save_prefs(&mut self) {
        if !self.demo {
            // Another window may have changed the update choices since; keep its values unless this one changed them.
            let disk = crate::settings::Prefs::load();
            if self.prefs.auto_update == self.update_prefs_seen.0 {
                self.prefs.auto_update = disk.auto_update;
            }
            if self.prefs.skipped_update == self.update_prefs_seen.1 {
                self.prefs.skipped_update = disk.skipped_update;
            }
            self.update_prefs_seen = (self.prefs.auto_update, self.prefs.skipped_update.clone());
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
        self.sync_sel();
        self.exec_one(ctx, c);
        self.sync_sel();
    }

    fn exec_one(&mut self, ctx: &egui::Context, c: Command) {
        let st = self.cmd_state();
        if !st.enabled(c) {
            return;
        }
        // An event drag restores the tracks every frame and pushes its own undo step on release: anything else that
        // changes tracks or pushes history now would be wiped or leave a mid-drag snapshot.
        let changes_tracks = matches!(c, InsertMarker | InsertRegion | MinimizeTracks | TracksTaller | TracksShorter | DefaultTrackHeights);
        if changes_tracks && self.drag_state.is_some() {
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
                let r = menus::save(&mut self.project);
                self.save_result(r);
            }
            SaveAs => {
                let r = menus::save_as(&mut self.project);
                self.save_result(r);
            }
            RenderAs => self.render.open = true,
            ImportMedia => ui::import_media(ctx, &self.importer),
            Properties => self.settings.open(&self.project),
            Welcome => self.welcome = Some(Default::default()),
            Exit => self.request(ctx, MenuAction::Exit),
            Preferences => self.prefs_dlg.open(&self.prefs),
            KeyboardShortcuts => self.shortcuts_open = true,
            About => self.about = true,
            CheckForUpdates => self.updater.check(ctx, true, std::time::Duration::ZERO),
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
                    // Selections keep the ids that still exist (`sync_sel`); a media index may point elsewhere now.
                    self.selected_media = None;
                    self.drag_state = None;
                }
            }
            ClearHistory => self.history.clear(),
            Copy => self.clipboard = edit::copy_clips(&self.project, &self.sel.clips),
            Cut => {
                // Only what the delete removes goes to the clipboard: locked events stay, so pasting them would
                // duplicate them.
                let p = &self.project;
                let ids: Vec<String> = p
                    .find_clips(&self.sel.clips)
                    .iter()
                    .filter_map(|r| {
                        let t = &p.timeline.tracks(r.is_video)[r.track_idx];
                        let c = &t.clips[r.clip_idx];
                        (!t.locked && !c.locked).then(|| c.id.clone())
                    })
                    .collect();
                if !ids.is_empty() && self.drag_state.is_none() {
                    self.clipboard = edit::copy_clips(&self.project, &ids);
                    self.edit(|p| edit::delete_clips(p, &ids));
                }
            }
            Paste => {
                // From the focus track at the cursor (quantized); the pasted events become the selection.
                let at = self.quantized(self.project.timeline.cursor_position);
                let (clips, focus) = (self.clipboard.clone(), self.focus_track());
                let mut ids = Vec::new();
                self.edit(|p| {
                    ids = edit::paste_clips(p, &clips, at, focus);
                    !ids.is_empty()
                });
                if !ids.is_empty() {
                    self.sel.clips = ids;
                }
            }
            Delete => {
                // What has the focus: the selected tracks after a click in the track list, else the events.
                let (clips, tracks) = (self.sel.clips.clone(), self.sel.tracks.clone());
                if self.sel.list_focus && !tracks.is_empty() {
                    self.edit(|p| edit::delete_tracks(p, &tracks));
                } else {
                    self.edit(|p| edit::delete_clips(p, &clips));
                }
            }
            Split => {
                let (at, ids) = (self.quantized(self.project.timeline.cursor_position), self.sel.clips.clone());
                self.edit(|p| edit::split_ids(p, &ids, at));
            }
            SelectAll => {
                let tl = &self.project.timeline;
                self.sel.clips = tl.video_tracks.iter().chain(&tl.audio_tracks).flat_map(|t| &t.clips).map(|c| c.id.clone()).collect();
            }
            SelectNone => self.sel.clips.clear(),
            Trim | TrimStart | TrimEnd => {
                let tl = &self.project.timeline;
                let (a, b) = match c {
                    Trim => tl.loop_region.map_or((None, None), |(a, b)| (Some(a), Some(b))),
                    TrimStart => (Some(tl.cursor_position), None),
                    _ => (None, Some(tl.cursor_position)),
                };
                let (a, b) = (a.map(|t| self.quantized(t)), b.map(|t| self.quantized(t)));
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
                    self.selected_media = Some(idx);
                    self.sel.clips.clear();
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
                // Above the focus track of that kind (`insert_track`); the new track gets the focus.
                let v = c == InsertVideoTrack;
                let before = self.focus_track().filter(|f| f.0 == v).map(|f| f.1);
                let mut i = 0;
                if self.edit(|p| {
                    i = edit::insert_track(p, v, before);
                    true
                }) {
                    self.sel.tracks = vec![self.project.timeline.tracks(v)[i].id.clone()];
                }
            }
            MuteTrack | SoloTrack => {
                let ids = self.sel.tracks.clone();
                self.edit(|p| edit::toggle_tracks(p, &ids, c == SoloTrack));
            }
            FocusPrevTrack | FocusNextTrack => {
                let tl = &self.project.timeline;
                let ids: Vec<&String> = tl.video_tracks.iter().chain(&tl.audio_tracks).map(|t| &t.id).collect();
                let cur = self.sel.tracks.last().and_then(|f| ids.iter().position(|i| *i == f));
                let next = match cur {
                    None => 0,
                    Some(k) if c == FocusPrevTrack => k.saturating_sub(1),
                    Some(k) => (k + 1).min(ids.len().saturating_sub(1)),
                };
                if let Some(id) = ids.get(next) {
                    self.sel.tracks = vec![(*id).clone()];
                    self.sel.list_focus = true;
                }
            }
            Quantize => {
                self.prefs.quantize = !self.prefs.quantize;
                self.save_prefs();
            }
            MinimizeTracks => edit::toggle_minimize(&mut self.project, &mut self.minimized, ui::MIN_TRACK_H),
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
                        TracksShorter => (h / 1.25).max(ui::MIN_TRACK_H),
                        _ => 0.0,
                    };
                }
                // Heights are view state: saved with the project, never an undo step.
                self.project.mark_modified();
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
                if let Some((exe, sha)) = self.install_on_exit.take() {
                    if let Err(e) = crate::updater::launch_installer(&exe, &sha, self.updater.auto) {
                        return self.message = Some(format!("Could not start the update installer: {e}"));
                    }
                }
                // Close (not process::exit) so the app is dropped and a running render's ffmpeg is killed.
                self.project.modified = false; // changes were saved or discarded; don't prompt again
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
        };
        self.project = project;
        self.history.clear();
        self.sel = ui::Selection::default();
        self.selected_clip = None;
        self.selected_media = None;
        self.minimized = None;
        // Copied events point at the old project's media.
        self.clipboard.clear();
        // A probe batch still running for the old project is dropped (its send fails).
        self.importer = crate::media::Importer::new();
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
            let saved = save && {
                let r = menus::save(&mut self.project);
                self.save_result(r)
            };
            // A cancelled or failed save aborts the New/Open/Exit, so nothing is discarded.
            if discard || saved {
                self.run(ctx, action);
            } else if hit.is_some() || close {
                // Forget what the New/Open/Exit was going to use, so a later one doesn't pick it up.
                (self.pending_open, self.new_settings, self.props_after_new) = (None, None, false);
                self.install_on_exit = None; // the update waits for the next "Install and restart"
            }
        }
        self.show_update(ctx);
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

    /// The update window; "Skip this version" and the auto-check box go to prefs, "Install and restart" exits
    /// through the save prompt and starts the installer.
    fn show_update(&mut self, ctx: &egui::Context) {
        use crate::updater::State;
        self.updater.poll(ctx, self.prefs.skipped_update.as_deref());
        let mut auto_check = self.prefs.auto_update;
        let mut res = dialogs::update_window(ctx, &mut self.updater, &mut auto_check);
        if self.updater.auto {
            // MILAN_UPDATE_AUTO (end-to-end test): install without clicks; any other outcome ends the run.
            match &self.updater.state {
                State::Ready(r, exe) => {
                    res = Some(UpdateResult::Install(exe.clone(), r.digest.clone().unwrap_or_default()));
                    self.updater.dismiss();
                }
                // With MILAN_SCREENSHOT the message stays up for the screenshot, which then closes the app.
                State::Message(m) if self.shot.is_none() => {
                    eprintln!("update: {m}");
                    self.updater.dismiss();
                    self.request(ctx, MenuAction::Exit);
                }
                _ => {}
            }
        }
        if auto_check != self.prefs.auto_update {
            self.prefs.auto_update = auto_check;
            self.save_prefs();
        }
        match res {
            Some(UpdateResult::Skip(v)) => {
                self.prefs.skipped_update = Some(v);
                self.save_prefs();
            }
            Some(UpdateResult::Install(exe, sha)) => {
                self.install_on_exit = Some((exe, sha));
                self.request(ctx, MenuAction::Exit);
            }
            None => {}
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
            "update" => self.updater.check(ctx, true, std::time::Duration::ZERO),
            m => {
                if let Some(k) = m.strip_prefix("menu:").and_then(|k| k.parse::<usize>().ok()) {
                    menus::request_open(ctx, k);
                }
            }
        }
    }
}
