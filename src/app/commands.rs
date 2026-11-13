//! Command execution for the app: keys (through the keymap), menu bar, context menus and toolbars all end up in
//! `MeridianApp::exec`. Also the modal dialogs and the save prompt.
use eframe::egui;
use crate::edit;
use crate::interchange::Imported;
use crate::project::{Project, TrackKind};
use crate::render::content_end;
use crate::settings::layout_key;
use crate::ui::dialogs::{self, UpdateResult, WelcomeResult};
use crate::ui::dock::{Area, Dock, LayoutRef, Panel};
use crate::ui::keymap::{Command, Command::*, Scope};
use crate::ui::menus::{self, CmdState};
use crate::ui::theme::{self, UiLook};
use crate::ui::{self, MenuAction, Playback};
use super::MeridianApp;

/// Heads the report of what an imported VEGAS project lost.
const IMPORTED_VEGAS: &str = "The VEGAS project is open as a new, unsaved project. These parts of it did not carry over:";

/// Copied events live in the app, but egui hears Ctrl+V only while the Windows clipboard holds text: put some there.
fn claim_clipboard<T>(ctx: &egui::Context, copied: &[T]) {
    if !copied.is_empty() {
        ctx.copy_text("Milan Pro events".into());
    }
}

impl MeridianApp {
    /// A modal dialog is up: shortcuts and drops wait.
    pub(super) fn modal_open(&self) -> bool {
        self.confirm.is_some()
            || self.about
            || self.settings.is_open()
            || self.prefs_dlg.is_open()
            || self.render.is_modal()
            || self.queue_win.is_modal()
            || self.batch.is_some()
            || self.render_tracks.is_some()
            || self.render_guard.is_some()
            || self.welcome.is_some()
            || self.shortcuts_open
            || self.unfinished_open
            || self.time_entry.is_some()
            || self.event_props.is_some()
            || self.take_chooser.is_some()
            || self.default_track.is_some()
            || self.layout_as.is_some()
            || self.organize.is_some()
            || self.customize_keys.is_some()
            || self.customize_bar.is_some()
            || self.vegas_report.is_some()
            || self.recorded.is_some()
            || self.paste_repeat.is_some()
            || self.paste_attrs.is_some()
            || self.remove_fx.is_some()
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
            clip_muted: clip.is_some_and(|c| c.muted),
            clip_looping: clip.is_some_and(|c| c.looping),
            video_clip: self.focus_clip(true).is_some(),
            audio_clip: self.focus_clip(false).is_some(),
            takes: self.project.find_clips(&self.sel.clips).iter().any(|r| !tl.tracks(r.is_video)[r.track_idx].clips[r.clip_idx].takes.is_empty()),
            n_sel: self.sel.clips.len(),
            any_tracks: !(tl.video_tracks.is_empty() && tl.audio_tracks.is_empty()),
            tracks_sel: !self.sel.tracks.is_empty(),
            track_group: self.sel.tracks.iter().any(|id| tl.group_of(id).is_some()),
            list_focus: self.sel.list_focus,
            quantize: self.prefs.quantize,
            clipboard: !self.clipboard.is_empty(),
            selection: self.project.timeline.loop_region.is_some_and(|(a, b)| b > a),
            looping: self.looping,
            snapping: self.snapping,
            snap_grid: self.prefs.snap_grid,
            snap_all: self.prefs.snap_all,
            grid: self.prefs.grid,
            ruler: self.project.settings.ruler.format,
            auto_crossfade: self.prefs.auto_crossfade,
            open_panels: Panel::ALL.into_iter().filter(|p| self.dock.is_open(*p)).collect(),
            quality: Some(self.preview.quality),
            recent: self.prefs.recent.clone(),
            space_pauses: self.prefs.space_pauses,
            tool: self.tool,
            in_group: self.project.find_clips(&self.sel.clips).iter().any(|r| tl.tracks(r.is_video)[r.track_idx].clips[r.clip_idx].group.is_some()),
            last_edit: !self.post_edit_ops().is_empty(),
            auto_ripple: self.prefs.auto_ripple,
            ripple_mode: self.prefs.ripple_mode,
            ignore_group: self.prefs.ignore_grouping,
            show_env: self.prefs.show_envelopes,
            lock_env: self.prefs.lock_envelopes,
            env_sel: self.sel.env.as_ref().is_some_and(|e| e.keys.iter().any(|&k| k > 0)),
            audio_track_focus: self.focus_of(false).is_some(),
            video_track_focus: self.focus_of(true).is_some(),
            focus_envs: self.focus_envs(false),
            focus_venvs: self.focus_envs(true),
            event_envs: self.focus_clip(false).and_then(|id| self.project.clip(&id)).map_or(Vec::new(), |c| c.envelopes.iter().map(|e| e.kind).collect()),
            ofx_envelopes: self.prefs.ofx_envelopes,
            hidden_envs: crate::settings::EnvView::bits(&self.prefs.hidden_envelopes),
            velocity: self.focus_clip(true).and_then(|id| self.project.clip(&id)).is_some_and(|c| crate::engine::velocity::of(c).is_some()),
            transition: self.focus_clip(true).and_then(|id| self.project.clip(&id)).is_some_and(|c| c.transition.is_some() || c.transition_out.is_some()),
            progress: self
                .focus_clip(true)
                .and_then(|id| self.project.clip(&id))
                .is_some_and(|c| [&c.transition, &c.transition_out].into_iter().flatten().any(|t| !t.progress.is_empty())),
            layouts: std::array::from_fn(|n| self.prefs.layouts.get(&layout_key(theme::get().look(), n)).map(|l| l.name.clone())),
            layout: self.layout,
            bypass_audio_fx: self.bypass_audio_fx,
            armed: self.focus_of(false).is_some_and(|i| tl.audio_tracks[i].armed),
            any_armed: tl.audio_tracks.iter().any(|t| t.armed),
            recording: self.rec_start.is_some(),
            input_level: self.input.as_ref().map(|_| self.input_level),
            edge_mode: self.sel.edge.is_some(),
            mute_audio: self.mute_audio,
            mute_video: self.mute_video,
            switches: {
                let (a, v) = (self.focus_clip(false), self.focus_clip(true));
                let (a, v) = (a.and_then(|id| self.project.clip(&id)), v.and_then(|id| self.project.clip(&id)));
                use edit::Switch::*;
                [Invert, Normalize, AutoNormalize, KeepAspect, ReduceFlicker]
                    .into_iter()
                    .filter(|s| if s.video() { v } else { a }.is_some_and(|c| s.get(c)))
                    .collect()
            },
            resample: self.focus_clip(true).and_then(|id| self.project.clip(&id)).and_then(|c| c.resample),
            channels: self.focus_clip(false).and_then(|id| self.project.clip(&id)).map(|c| c.channels).unwrap_or_default(),
            stream: clip.map_or(0, |c| c.stream),
            streams: clip.and_then(|c| edit::media_of(&self.project, c)).map_or(1, |m| m.streams(self.selected_clip.as_ref().is_some_and(|r| r.is_video))),
            quickfade: self.prefs.quickfade,
            audio_follows_velocity: tl.audio_follows_velocity,
            assignable_fx: !tl.audio.fx.is_empty(),
            video_bus: self.prefs.video_bus,
            extra_modes: self.prefs.extra_blend_modes,
            bus_tracks: self.prefs.bus_tracks,
            dim: self.audio.master.dim.load(std::sync::atomic::Ordering::Relaxed),
            output_meters: self.prefs.output_meters,
            vertical_meters: self.prefs.vertical_meters,
            meter_width: self.prefs.meter_width,
            external: self.external.is_some(),
            loudness: self.prefs.loudness,
        }
    }

    /// Each frame: the armed tracks' input meter opens and closes with arming, and a recording stops with playback.
    pub(super) fn tick_input(&mut self, ctx: &egui::Context) {
        use crate::record::Capture;
        if let Some(start) = self.rec_start {
            // Playback starts late (decoders warming up) and the input early or late, by a different amount each take:
            // the first time both run, the clock says where the file is.
            if self.rec_anchor.is_none() && self.clock_pos > start {
                self.rec_anchor = self.input.as_ref().and_then(|c| c.recorded_secs()).map(|s| self.clock_pos - s);
            }
            // Stop, Pause, Space or the end of a Play From Start: the recording ends with playback, as in VEGAS.
            if !self.playing {
                self.stop_recording();
            }
        } else if !self.devices || !(self.project.timeline.audio_tracks.iter().any(|t| t.armed) || !self.project.timeline.audio.inputs.is_empty()) {
            // R9: the input stays open while a track is armed or the project has an input bus (heard live, as in VEGAS).
            (self.input, self.input_failed) = (None, false);
        } else if self.input.is_none() && !self.input_failed {
            self.input = Some(Capture::start(self.prefs.record_input.clone(), None, Some(self.audio.ring.clone())));
        }
        let Some(c) = &self.input else {
            self.input_level = 0.0;
            return;
        };
        if let Some(e) = c.error() {
            if self.rec_start.is_some() {
                // The device failed or went away: keep and place what was recorded.
                self.stop_recording();
                self.message.get_or_insert(e);
            } else {
                self.message = Some(e);
            }
            (self.input, self.input_failed) = (None, true);
            return;
        }
        let peak = c.take_peaks().into_iter().fold(0.0, f32::max);
        self.input_level = peak.max(self.input_level * 0.8);
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }

    /// Record (Ctrl+R): a new file in the recording folder, then playback from the cursor with looping off.
    fn start_recording(&mut self) {
        let p = &self.project;
        let Some(track) = p.timeline.audio_tracks.iter().find(|t| t.armed) else { return };
        let Some(dir) = crate::record::folder(p) else {
            self.message = Some("Save the project or arm the track again to choose a folder for recordings.".into());
            return;
        };
        match crate::record::create_file(&dir, &p.name, &track.name) {
            Ok(out) => {
                self.message = Some(format!("Recording to {}", out.0.display()));
                self.input = None; // the meter lets go of the device first
                self.input = Some(crate::record::Capture::start(self.prefs.record_input.clone(), Some(out), Some(self.audio.ring.clone())));
                (self.rec_start, self.rec_anchor, self.rec_looping) = (Some(p.timeline.cursor_position), None, self.looping);
                (self.looping, self.shuttle, self.playing) = (false, 0.0, true);
            }
            Err(e) => self.message = Some(format!("Could not create a recording file in {}: {e}", dir.display())),
        }
    }

    /// End the recording: the file goes on the armed tracks (one undo step) and Recorded Files opens. A file that
    /// holds no audio is removed; one that could not be placed stays on disk and is named in the status bar.
    pub(super) fn stop_recording(&mut self) {
        let Some(start) = self.rec_start.take() else { return };
        self.playing = false;
        self.looping |= self.rec_looping;
        let Some(cap) = self.input.take() else { return };
        let path = cap.path.clone();
        let mut failed = None;
        let take = match cap.finish() {
            Ok(t) => t,
            Err(e) => match path.as_deref().map(crate::record::recover) {
                // The file broke off (a full disk): what reached it is still placed.
                Some(Ok((frames, rate))) if frames > 0 => {
                    failed = Some(e);
                    crate::record::Take { path: path.unwrap_or_default(), frames, rate, dropped: 0 }
                }
                _ => {
                    // Only a header (or nothing) was written: nothing of the user's is lost.
                    if let Some(p) = path.filter(|p| std::fs::metadata(p).is_ok_and(|m| m.len() <= 44)) {
                        let _ = std::fs::remove_file(p);
                    }
                    self.message = Some(format!("Recording failed: {e}"));
                    return;
                }
            },
        };
        if take.frames == 0 {
            let _ = std::fs::remove_file(&take.path);
            self.message = Some("Nothing was recorded.".into());
            return;
        }
        let at = self.rec_anchor.take().unwrap_or(start) - self.prefs.record_offset_ms / 1000.0;
        // The take is its own undo step: an event drag in progress is kept as the step before it.
        self.end_drag();
        let mut id = None;
        let placed = self.edit(|p| {
            let done = edit::place_recording(p, &take.path, take.frames, take.rate, at, start);
            id = done.then(|| p.media_pool.last().map(|m| m.id.clone())).flatten();
            done
        });
        self.message = if let Some(e) = failed {
            Some(format!("Recording stopped: {e}. What was saved is kept in {}.", take.path.display()))
        } else if !placed {
            Some(format!("Recorded {}, but no armed, unlocked audio track could take it; import it from there.", take.path.display()))
        } else if take.dropped > 0 {
            Some(format!("The recording lost {} blocks of audio (the disk was too slow); later audio is early.", take.dropped))
        } else {
            None
        };
        self.recorded = id.map(|id| dialogs::RecordedFiles::new(vec![id]));
    }

    /// Release an event or envelope drag as if the mouse let go: its changes become an undo step.
    fn end_drag(&mut self) {
        let before = match (self.drag_state.take(), self.env_drag.take()) {
            (Some(d), _) if d.changed => Some(d.before),
            (_, Some(e)) if e.changed => Some(e.before),
            _ => None,
        };
        if let Some(b) = before {
            self.history.push(b);
            self.project.mark_modified();
        }
    }

    /// Recorded Files › Delete: the media and its events leave the project (one undo step), then the file is deleted.
    fn delete_recording(&mut self, id: &str) {
        let Some(path) = self.project.media_pool.iter().find(|m| m.id == id).map(|m| m.path.clone()) else { return };
        let removed = self.edit(|p| {
            p.media_pool.retain(|m| m.id != id);
            edit::forget_media(&mut p.timeline, id);
            p.mark_modified();
            true
        });
        if removed {
            if let Err(e) = std::fs::remove_file(&path) {
                self.message = Some(format!("Could not delete {}: {e}", path.display()));
            }
        }
    }

    /// The focus track of one kind: the last selected track of that kind.
    fn focus_of(&self, video: bool) -> Option<usize> {
        self.sel.tracks.iter().rev().filter_map(|id| self.project.find_track(id)).find(|f| f.0 == video).map(|f| f.1)
    }

    /// An envelope was just inserted: View › Show Envelopes comes on, as in VEGAS.
    fn reveal_envelopes(&mut self) {
        if !self.prefs.show_envelopes {
            self.prefs.show_envelopes = true;
            self.save_prefs();
        }
    }

    /// The envelopes the focus track of one kind has, of the kinds it plays.
    fn focus_envs(&self, video: bool) -> Vec<crate::project::EnvKind> {
        let tracks = self.project.timeline.tracks(video);
        self.focus_of(video).map_or(Vec::new(), |i| tracks[i].envelopes.iter().filter(|e| edit::env_fits(e.kind, video)).map(|e| e.kind).collect())
    }

    /// Set Default Track Properties OK: restore the built-in chain, or save the track's chain, for new audio tracks.
    fn set_default_track(&mut self, d: &dialogs::DefaultTrackProps) {
        let fx = if d.restore {
            self.message = Some("New audio tracks will start with the original default FX.".into());
            None
        } else {
            let Some((_, i)) = self.project.find_track(&d.track).filter(|f| !f.0 && d.track_fx) else { return };
            let fx = self.project.timeline.audio_tracks[i].fx.clone();
            self.message = Some(format!("New audio tracks will start with the FX of track {} ({} plug-ins).", i + 1, fx.len()));
            Some(fx)
        };
        crate::project::set_default_track_fx(fx.clone());
        self.prefs.default_track_fx = fx;
        self.save_prefs();
    }

    /// The focus event of one kind: the last selected event of that kind (a grouped A/V pair has one of each).
    fn focus_clip(&self, video: bool) -> Option<String> {
        self.sel.clips.iter().rev().find(|id| self.project.find_clip(id).is_some_and(|r| r.is_video == video)).cloned()
    }

    /// The editing options the timeline follows.
    pub(super) fn edit_opts(&self) -> ui::EditOpts {
        let p = &self.prefs;
        ui::EditOpts {
            snapping: self.snapping,
            snap_grid: p.snap_grid,
            snap_all: p.snap_all,
            grid: p.grid,
            quantize: p.quantize,
            auto_crossfade: p.auto_crossfade,
            ripple: p.auto_ripple.then_some(p.ripple_mode),
            ignore_grouping: p.ignore_grouping,
            tool: self.tool,
            show_envelopes: p.show_envelopes,
            lock_envelopes: p.lock_envelopes,
            hidden_envs: crate::settings::EnvView::bits(&p.hidden_envelopes)
                | if p.ofx_envelopes && theme::get().look() == UiLook::Vegas23 { crate::settings::OFX_SHOWN } else { 0 },
            cut_overlap: p.cut_overlap,
            wave_zoom: self.wave_zoom,
            video_bus: p.video_bus,
            bus_tracks: p.bus_tracks,
        }
    }

    /// Expanded Edit Mode: the media and source time of the outgoing event's last frame and the incoming event's first
    /// frame at the selected edge (either may be missing: an edge with no event beside it).
    pub(super) fn edit_point(&self) -> Option<[ui::EditSide; 2]> {
        let e = self.sel.edge.as_ref().filter(|e| e.expanded)?;
        let p = &self.project;
        let c = p.clip(&e.clip)?;
        let frame = 1.0 / crate::project::exact_fps(p.settings.frame_rate.max(1.0));
        let at = if e.end { c.position + c.duration } else { c.position };
        let other = edit::adjacent(p, &e.clip, e.end, frame * 0.5);
        let (out, inc) = if e.end { (Some(e.clip.clone()), other) } else { (other, Some(e.clip.clone())) };
        let side = |id: Option<String>, last: bool| -> ui::EditSide {
            let c = p.clip(id.as_deref()?)?;
            let m = p.media_pool.iter().find(|m| Some(&m.id) == c.media_id.as_ref())?.clone();
            Some((m, edit::src_time(c, if last { at - frame } else { at }).max(0.0)))
        };
        Some([side(out, true), side(inc, false)])
    }

    /// Edge-trimming mode: trim the selected edge by `d` seconds (one undo step, rippled with Auto Ripple like a trim
    /// drag); the cursor follows the edge.
    fn trim_edge(&mut self, e: &ui::EdgeSel, d: f64) {
        let (id, end, xfade) = (e.clip.clone(), e.end, self.prefs.auto_crossfade);
        self.ripple_edit(false, |p, mode| {
            let before = p.clone();
            let r = before.find_clip(&id)?;
            if !edit::trim_edge(p, &id, end, d) {
                return None;
            }
            let (o, n) = (before.clip(&id)?, p.clip(&id)?.clone());
            let ids = vec![id.clone()];
            let ops = edit::trim_ripple(&before, o, &n, &ids, &[before.timeline.tracks(r.is_video)[r.track_idx].id.clone()]);
            if let Some(m) = mode {
                edit::ripple_all(p, &ops, m);
            }
            if xfade {
                edit::auto_crossfade(p, &before, &ids);
            }
            Some(ops)
        });
        if let Some(c) = self.project.clip(&e.clip) {
            self.project.timeline.cursor_position = if e.end { c.position + c.duration } else { c.position };
        }
    }

    /// Preview at Cursor ended (playback stopped or the span played): the cursor and time selection come back.
    pub(super) fn tick_cursor_preview(&mut self) {
        let Some((cursor, saved, span)) = self.cursor_preview else { return };
        let tl = &mut self.project.timeline;
        if self.playing && !self.looping && tl.cursor_position >= span.1 {
            self.playing = false;
        }
        if !self.playing {
            if tl.loop_region == Some(span) {
                tl.loop_region = saved;
            }
            tl.cursor_position = cursor;
            self.cursor_preview = None;
        }
    }

    /// Paste Repeat OK: `n` copies of the clipboard from the cursor (one undo step); they become the selection.
    fn paste_repeat(&mut self, n: usize, every: Option<f64>) {
        let at = self.quantized(self.project.timeline.cursor_position);
        let (clips, focus, xfade) = (self.clipboard.clone(), self.focus_track(), self.prefs.auto_crossfade);
        let mut ids = Vec::new();
        self.edit(|p| {
            let before = p.clone();
            ids = edit::paste_repeat(p, &clips, at, focus, n, every);
            if xfade && !ids.is_empty() {
                edit::auto_crossfade(p, &before, &ids);
            }
            !ids.is_empty()
        });
        if !ids.is_empty() {
            (self.sel.clips, self.sel.env) = (ids, None);
        }
    }

    /// The FX window on `target`, its last plug-in selected (a drop just put it there).
    pub(super) fn open_fx(&mut self, target: edit::FxTarget) {
        self.fx_win = ui::FxWindow::open(&self.project, target).map(|w| w.select_last(&self.project));
    }

    /// A chooser tile double-clicked: video FX go on the focus video event, a transition on its start, generated media
    /// at the cursor on the focus video track (New still image length long, as VEGAS). One undo step; the FX window then
    /// opens on it.
    pub(super) fn apply_plugin(&mut self, d: ui::PluginDrag) {
        use crate::engine::plugins::PluginKind;
        use edit::FxTarget;
        let p = &self.project;
        let clip = self.focus_clip(true);
        let (at, track, xfade, cut, len) = (self.quantized(p.timeline.cursor_position), self.focus_of(true), self.prefs.auto_crossfade, self.prefs.cut_overlap, self.prefs.still_len);
        let (mut target, mut inserted) = (None, None);
        self.edit(|p| {
            target = match d.kind {
                PluginKind::VideoFx => clip.map(FxTarget::Event).filter(|t| edit::add_fx(p, t, d.fx.clone())),
                PluginKind::Transition => clip
                    .filter(|id| {
                        let (Some(r), Some(start)) = (p.find_clip(id), p.clip(id).map(|c| c.position)) else { return false };
                        let tid = p.timeline.video_tracks[r.track_idx].id.clone();
                        edit::set_transition(p, &tid, start, edit::transition_of(&d.fx), cut, xfade)
                    })
                    .map(FxTarget::Transition),
                PluginKind::Generator => {
                    inserted = edit::insert_generated(p, track, at, d.fx.clone(), &d.name, len);
                    inserted.as_ref().and_then(|id| p.clip(id)?.media_id.clone()).map(FxTarget::Media)
                }
                // The Compositors window: the focus video track's mode becomes Custom with this compositor.
                PluginKind::Compositor => {
                    let id = track.and_then(|i| p.timeline.video_tracks.get(i)).map(|t| t.id.clone());
                    id.filter(|id| edit::set_compositor(p, std::slice::from_ref(id), &d.fx)).map(FxTarget::Compositor)
                }
                // No chooser offers audio FX.
                PluginKind::AudioFx => None,
            };
            target.is_some()
        });
        if let Some(id) = inserted {
            (self.sel.clips, self.sel.env) = (vec![id], None);
        }
        match target {
            Some(t) => self.open_fx(t),
            None => self.message = Some("Select an unlocked video event (or, for generated media, an unlocked video track) first.".into()),
        }
    }

    /// One undoable edit that rippled (or, with Auto Ripple off, may be rippled later by Post-Edit Ripple): `f`
    /// edits and returns the ripple ops that close or open its gaps, which are applied with the ripple mode when
    /// Auto Ripple is on (or `force`, for Paste Insert).
    fn ripple_edit(&mut self, force: bool, f: impl FnOnce(&mut Project, Option<edit::RippleMode>) -> Option<Vec<edit::RippleOp>>) {
        let mode = (force || self.prefs.auto_ripple).then_some(self.prefs.ripple_mode);
        let mut ops = None;
        if self.edit(|p| {
            ops = f(p, mode);
            ops.is_some()
        }) {
            self.last_edit = (self.history.generation(), if mode.is_some() { Vec::new() } else { ops.unwrap_or_default() });
        }
    }

    /// The ripple Post-Edit Ripple would apply: the last edit's, unless the history changed since.
    fn post_edit_ops(&self) -> &[edit::RippleOp] {
        if self.last_edit.0 == self.history.generation() { &self.last_edit.1 } else { &[] }
    }

    /// Keyboard shortcuts for this frame, run before the UI is drawn (as the old hard-wired keys were).
    pub(super) fn handle_keys(&mut self, ctx: &egui::Context) {
        let blocked = ctx.wants_keyboard_input() || menus::any_open(ctx) || self.modal_open();
        // Over the Trimmer, Project Media's items or a Pan/Crop / Track Motion window, plain keys (Delete) are that window's.
        let over_dock = ui::over_plain_key_area(ctx) | ui::over_tool_window(ctx);
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
        if self.dragging() {
            return false;
        }
        let before = self.project.clone();
        let done = f(&mut self.project);
        if done {
            self.history.push(before);
        }
        done
    }

    /// Delete events `ids`, closing their gaps when Auto Ripple is on.
    fn delete_rippled(&mut self, ids: &[String]) {
        let xfade = self.prefs.auto_crossfade;
        self.ripple_edit(false, |p, mode| {
            let ops = edit::gaps(p, ids);
            edit::delete_clips(p, ids).then(|| {
                if let Some(m) = mode {
                    let before = p.clone();
                    // Ripple All also moves tracks the gap is not on, which may overlap there.
                    if edit::ripple_all(p, &ops, m) && xfade {
                        edit::crossfade_moved(p, &before);
                    }
                }
                ops
            })
        });
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

    /// R16c: Tools › Generate Loudness Log: the time selection (else the whole timeline), mixed as a render mixes it, into
    /// `<project>_loud.txt` beside the project (an unsaved project asks where), in the background.
    fn generate_loudness_log(&mut self, ctx: &egui::Context) {
        if self.loud_log.is_some() {
            self.message = Some("A loudness log is already being generated.".into());
            return;
        }
        let range = self.project.timeline.loop_region.filter(|(a, b)| b > a).unwrap_or((0.0, content_end(&self.project)));
        let out = match &self.project.path {
            Some(p) => crate::render::loudness_log_path(p),
            None => match rfd::FileDialog::new().add_filter("Text", &["txt"]).set_file_name(format!("{}_loud.txt", self.project.name)).save_file() {
                Some(p) => p,
                None => return,
            },
        };
        self.loud_log = Some(crate::engine::render::loudness_log(&self.project, range, self.prefs.quickfade, self.prefs.loudness.mode, out, ctx.clone()));
        self.message = Some("Generating the loudness log...".into());
    }

    /// Each frame: a finished loudness log is reported and, with Preferences › Audio › "Open editor when new loudness log
    /// is generated", opened in the text editor.
    pub(super) fn tick_loudness_log(&mut self) {
        use crate::render::Status;
        let Some(job) = &self.loud_log else { return };
        match job.status() {
            Status::Running => return,
            Status::Done => {
                if let Some(p) = job.log() {
                    self.message = Some(format!("Loudness log saved to {}", p.display()));
                    if self.prefs.loudness.open_editor {
                        dialogs::open_file(&p);
                    }
                }
            }
            Status::Failed(e) => self.message = Some(format!("Generate Loudness Log: {e}")),
            Status::Cancelled => {}
        }
        self.loud_log = None;
    }

    pub(super) fn save_prefs(&mut self) {
        if !self.demo {
            // Another window may have saved since: keep its values except for what this one changed. An unreadable
            // file is set aside as prefs.json.bad rather than written over.
            let mine = std::mem::take(&mut self.prefs);
            self.prefs = match crate::settings::Prefs::try_load() {
                Some(disk) => mine.merged(disk, &self.prefs_seen, &self.layouts_changed),
                None => {
                    crate::settings::Prefs::keep_unreadable();
                    mine
                }
            };
            match self.prefs.save() {
                Ok(()) => {
                    self.layouts_changed.clear();
                    self.prefs_seen = self.prefs.clone();
                }
                Err(e) => self.message = Some(format!("Could not save preferences: {e}")),
            }
        }
    }

    /// Load a built-in layout, or user slot `n` of this look if it is saved; it becomes the current layout.
    fn load_layout(&mut self, l: LayoutRef) {
        let t = theme::get();
        match l {
            LayoutRef::Builtin(b) => self.dock = Dock::builtin(b, &t),
            LayoutRef::User(n) => match self.prefs.layouts.get(&layout_key(t.look(), n)) {
                Some(saved) => self.dock.apply(saved, &t),
                None => return,
            },
        }
        self.layout = Some(l);
    }

    /// Preferences, on page `page` (`dialogs::prefs::DISPLAY`...; screenshot harness) or the last one. The VP16 Display
    /// page shows where the dock area is.
    fn open_prefs(&mut self, page: Option<usize>) {
        match page {
            None => self.prefs_dlg.open(&self.prefs),
            Some(k) => self.prefs_dlg.open_on(&self.prefs, k),
        }
        self.prefs_dlg.timeline_bottom = !self.dock.timeline_top();
    }

    /// Keep the dock as user layout `n` of this look, named `name` (else the slot's name, else "Layout n").
    fn save_layout(&mut self, n: usize, name: Option<String>) {
        let key = layout_key(theme::get().look(), n);
        let name = name.or_else(|| self.prefs.layouts.get(&key).map(|l| l.name.clone())).unwrap_or_else(|| format!("Layout {n}"));
        self.prefs.layouts.insert(key.clone(), self.dock.layout(&name));
        self.layouts_changed.insert(key);
        self.layout = Some(LayoutRef::User(n));
        self.save_prefs();
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
        if changes_tracks && self.dragging() {
            return;
        }
        let v16 = st.look == UiLook::Vegas16;
        let frame = 1.0 / self.project.settings.frame_rate.max(1.0);
        let exact_frame = 1.0 / crate::project::exact_fps(self.project.settings.frame_rate.max(1.0));
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
            RenderAs => self.render.open(&self.project),
            RenderToNewTrack => self.render.open_new_track(&self.project),
            RenderQueue => self.queue_win.open = true,
            BatchRender => self.batch = Some(ui::render_queue::BatchRender::new(&self.project)),
            RenderAudioTracks => self.render_tracks = Some(ui::render_queue::RenderTracks::new(&self.project)),
            ImportMedia => ui::import_media(ctx, &self.importer),
            ImportVegas => {
                let picked = rfd::FileDialog::new().add_filter("VEGAS Project (Milan Pro export, .veg)", &["json", "veg"]).pick_file();
                if let Some(path) = picked {
                    self.vegas.start(ctx, path);
                    self.message = Some("Importing VEGAS project...".into());
                }
            }
            Properties => self.settings.open(&self.project),
            Welcome => self.welcome = Some(Default::default()),
            Exit => self.request(ctx, MenuAction::Exit),
            Preferences => self.open_prefs(None),
            KeyboardShortcuts => self.shortcuts_open = true,
            // The screenshot harness lists no keyboard maps and saves none.
            CustomizeKeyboard => {
                let dir = if self.demo { None } else { crate::settings::keyboard_dir() };
                self.customize_keys = Some(dialogs::CustomizeKeys::new(&self.prefs.keys, self.prefs.key_map.clone(), st.look, dir));
            }
            CustomizeToolbar | CustomizeTimelineToolbar => {
                let bar = if c == CustomizeToolbar { ui::Bar::Main } else { ui::Bar::Timeline };
                self.customize_bar = Some(dialogs::CustomizeToolbar::new(bar, bar.buttons(&self.prefs.toolbars, st.look), st.look));
            }
            UnfinishedFeatures => self.unfinished_open = true,
            About => self.about = true,
            CheckForUpdates => self.updater.check(ctx, true, std::time::Duration::ZERO),
            // Edit
            // A drag in progress holds its own undo step until release; undoing under it would stack a half drag.
            Undo | Redo | UndoAll if self.dragging() => {}
            Undo | Redo | UndoAll => {
                // Arming and the recording folder are not edits: they stay as they are through undo and redo.
                let armed: Vec<String> = self.project.timeline.audio_tracks.iter().filter(|t| t.armed).map(|t| t.id.clone()).collect();
                let record_dir = self.project.timeline.record_dir.clone();
                let mut done = false;
                loop {
                    let step = if c == Redo { self.history.redo(&mut self.project) } else { self.history.undo(&mut self.project) };
                    done |= step;
                    if !step || c != UndoAll {
                        break;
                    }
                }
                for t in &mut self.project.timeline.audio_tracks {
                    t.armed = armed.contains(&t.id);
                }
                self.project.timeline.record_dir = record_dir;
                if done {
                    // Selections keep the ids that still exist (`sync_sel`); a media or point index may point elsewhere now.
                    (self.selected_media, self.sel.env) = (None, None);
                    self.drag_state = None;
                    self.env_drag = None;
                }
            }
            ClearHistory => self.history.clear(),
            Copy => {
                self.clipboard = edit::copy_clips(&self.project, &self.sel.clips);
                claim_clipboard(ctx, &self.clipboard);
            }
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
                if !ids.is_empty() && !self.dragging() {
                    self.clipboard = edit::copy_clips(&self.project, &ids);
                    claim_clipboard(ctx, &self.clipboard);
                    self.delete_rippled(&ids);
                }
            }
            Paste | PasteInsert => {
                // From the focus track at the cursor (quantized); the pasted events become the selection. Paste
                // Insert (or Auto Ripple) first moves what is after the cursor along by the pasted length.
                let at = self.quantized(self.project.timeline.cursor_position);
                let (clips, focus, xfade) = (self.clipboard.clone(), self.focus_track(), self.prefs.auto_crossfade);
                let mut ids = Vec::new();
                self.ripple_edit(c == PasteInsert, |p, mode| {
                    let before = p.clone();
                    let op = edit::paste_ripple(p, &clips, at, focus);
                    if let Some(m) = mode {
                        edit::ripple(p, &op, m);
                    }
                    ids = edit::paste_clips(p, &clips, at, focus);
                    if ids.is_empty() {
                        *p = before; // a locked track refused it: no ripple either
                        return None;
                    }
                    if xfade {
                        edit::auto_crossfade(p, &before, &ids);
                    }
                    Some(vec![edit::RippleOp { exclude: ids.clone(), ..op }])
                });
                if !ids.is_empty() {
                    (self.sel.clips, self.sel.env) = (ids, None);
                }
            }
            Delete => {
                // What has the focus: the selected tracks after a click in the track list, else the events. With Auto
                // Ripple, a time selection and no events deletes that time from every track (as VEGAS does: a time
                // selection affects all events in it, which keeps grouped video and audio in sync).
                let (clips, tracks) = (self.sel.clips.clone(), self.sel.tracks.clone());
                let range = self.project.timeline.loop_region.filter(|(a, b)| b > a && self.prefs.auto_ripple);
                // Selected envelope points go first (the first point of an envelope stays: then the events go).
                let es = self.sel.env.clone().filter(|e| e.keys.iter().any(|&k| k > 0));
                if es.is_some_and(|es| self.edit(|p| edit::edit_envelope(p, &es.track, es.kind, |e| edit::env_delete(e, &es.keys)))) {
                    self.sel.env = None;
                } else if self.sel.list_focus && !tracks.is_empty() {
                    self.edit(|p| edit::delete_tracks(p, &tracks));
                } else if !clips.is_empty() {
                    self.delete_rippled(&clips);
                } else if let Some((a, b)) = range {
                    let tl = &self.project.timeline;
                    let tracks: Vec<String> = tl.video_tracks.iter().chain(&tl.audio_tracks).map(|t| t.id.clone()).collect();
                    let mode = self.prefs.ripple_mode;
                    self.edit(|p| edit::delete_range(p, a, b, &tracks, mode));
                }
            }
            Split => {
                let (at, ids) = (self.quantized(self.project.timeline.cursor_position), self.sel.clips.clone());
                self.edit(|p| edit::split_ids(p, &ids, at));
            }
            SelectAll => {
                let tl = &self.project.timeline;
                self.sel.clips = tl.video_tracks.iter().chain(&tl.audio_tracks).flat_map(|t| &t.clips).map(|c| c.id.clone()).collect();
                self.sel.env = None;
            }
            SelectNone => (self.sel.clips, self.sel.env) = (Vec::new(), None),
            Trim | TrimStart | TrimEnd => {
                let tl = &self.project.timeline;
                let (a, b) = match c {
                    Trim => tl.loop_region.map_or((None, None), |(a, b)| (Some(a), Some(b))),
                    TrimStart => (Some(tl.cursor_position), None),
                    _ => (None, Some(tl.cursor_position)),
                };
                let (a, b) = (a.map(|t| self.quantized(t)), b.map(|t| self.quantized(t)));
                // Ripples (or can be post-edit rippled) and crossfades as a trim drag does.
                let (Some(cr), xfade) = (self.selected_clip.clone(), self.prefs.auto_crossfade) else { return };
                self.ripple_edit(false, |p, mode| {
                    let before = p.clone();
                    let tr = before.timeline.tracks(cr.is_video).get(cr.track_idx)?;
                    let o = tr.clips.get(cr.clip_idx)?;
                    if !edit::trim_to(p, &cr, a, b) {
                        return None;
                    }
                    let ids = vec![o.id.clone()];
                    let ops = edit::trim_ripple(&before, o, &p.timeline.tracks(cr.is_video)[cr.track_idx].clips[cr.clip_idx], &ids, &[tr.id.clone()]);
                    if let Some(m) = mode {
                        edit::ripple_all(p, &ops, m);
                    }
                    if xfade {
                        edit::auto_crossfade(p, &before, &ids);
                    }
                    Some(ops)
                });
            }
            EventLock => {
                self.edit_selected(edit::toggle_clip_lock);
            }
            EventReverse | EventMute | EventLoop => {
                let ids = self.sel.clips.clone();
                self.edit(|p| match c {
                    EventReverse => edit::reverse_clips(p, &ids),
                    EventMute => edit::mute_clips(p, &ids),
                    _ => edit::loop_clips(p, &ids),
                });
            }
            EventSwitch(_) | SetResample(_) | SetChannels(_) | SetStream(_) => {
                let ids = self.sel.clips.clone();
                // Stream ▸ lists the focus event's streams, for the selected events of its kind.
                let video = self.selected_clip.as_ref().is_some_and(|r| r.is_video);
                self.edit(|p| match c {
                    EventSwitch(s) => edit::set_switch(p, &ids, s),
                    SetResample(r) => edit::set_resample(p, &ids, r),
                    SetChannels(ch) => edit::set_channels(p, &ids, ch),
                    SetStream(n) => edit::set_stream(p, &ids, video, n),
                    _ => false,
                });
            }
            CreateSubclip => {
                let Some(id) = self.sel.clips.last().cloned() else { return };
                let mut sub = None;
                self.edit(|p| {
                    sub = edit::subclip_event(p, &id);
                    sub.is_some()
                });
                self.message = Some(match sub.and_then(|s| self.project.media_pool.iter().find(|m| m.id == s)) {
                    Some(m) => format!("Created subclip \"{}\" in Project Media.", m.name),
                    None => "Only an event with video or audio media can make a subclip.".into(),
                });
            }
            PasteAttributes => {
                let (clips, ids) = (self.clipboard.clone(), self.sel.clips.clone());
                self.edit(|p| edit::paste_attrs(p, &clips, &ids, edit::Attrs::ALL));
            }
            PasteAttributesSelect => self.paste_attrs = Some(dialogs::PasteAttrs::new()),
            InsertEmptyEvent => {
                // On the selected tracks, else the focus event's track (VEGAS inserts on the selected tracks).
                let Some(range) = self.project.timeline.loop_region else { return };
                let mut tracks = self.sel.tracks.clone();
                if tracks.is_empty() {
                    let focus = self.selected_clip.as_ref().and_then(|r| self.project.timeline.tracks(r.is_video).get(r.track_idx));
                    tracks.extend(focus.map(|t| t.id.clone()));
                }
                let mut ids = Vec::new();
                self.edit(|p| {
                    ids = edit::insert_empty(p, &tracks, range);
                    !ids.is_empty()
                });
                if ids.is_empty() {
                    self.message = Some("Select an unlocked track to insert the empty event on.".into());
                } else {
                    (self.sel.clips, self.sel.env) = (ids, None);
                }
            }
            GroupCut | GroupCopy | GroupDelete => {
                // The whole group of every selected event, whatever Ignore Event Grouping says.
                self.sel.clips = edit::expand_groups(&self.project, &self.sel.clips);
                self.sel.env = None;
                match c {
                    GroupCut => self.exec_one(ctx, Cut),
                    GroupCopy => self.exec_one(ctx, Copy),
                    _ => {
                        let ids = self.sel.clips.clone();
                        self.delete_rippled(&ids);
                    }
                }
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
            EventProperties => self.event_props = self.sel.clips.last().and_then(|id| dialogs::EventProps::open(&self.project, id)),
            RenameEvent => {
                if let Some(id) = self.sel.clips.last() {
                    ui::rename_event(ctx, id);
                }
            }
            NextTake | PrevTake | DeleteActiveTake => {
                let ids = self.sel.clips.clone();
                self.edit(|p| match c {
                    NextTake | PrevTake => edit::next_take(p, &ids, c == NextTake),
                    _ => edit::delete_active_take(p, &ids),
                });
            }
            EventPanCrop => self.pan_crop = self.focus_clip(true).and_then(|id| ui::PanCropWin::open(&self.project, &id)),
            TrackMotion => {
                let id = self.focus_of(true).map(|i| self.project.timeline.video_tracks[i].id.clone());
                self.track_motion = id.and_then(|id| ui::TrackMotionWin::open(&self.project, &id));
            }
            VideoEventFx | TrackFx | VideoOutputFx => {
                let target = match c {
                    VideoEventFx => self.focus_clip(true).map(edit::FxTarget::Event),
                    TrackFx => self.focus_of(true).map(|i| edit::FxTarget::Track(self.project.timeline.video_tracks[i].id.clone())),
                    _ => Some(edit::FxTarget::VideoOutput),
                };
                if let Some(t) = target {
                    self.open_fx(t);
                }
            }
            AudioEventFx | AudioTrackFx => {
                let target = if c == AudioEventFx {
                    self.focus_clip(false).map(edit::FxTarget::Event)
                } else {
                    self.focus_of(false).map(|i| edit::FxTarget::Track(self.project.timeline.audio_tracks[i].id.clone()))
                };
                if let Some(t) = target {
                    self.open_fx(t);
                }
            }
            BypassAudioFx => self.bypass_audio_fx = !self.bypass_audio_fx,
            // Arm for Record: the selected audio tracks follow the focus one. An unsaved project first asks where its
            // recordings go.
            Arm => {
                let Some(i) = self.focus_of(false) else { return };
                let on = !self.project.timeline.audio_tracks[i].armed;
                if on && crate::record::folder(&self.project).is_none() {
                    let Some(dir) = rfd::FileDialog::new().set_title("Folder for recorded files").pick_folder() else { return };
                    // A project setting, not an edit: no undo step.
                    self.project.timeline.record_dir = Some(dir);
                    self.project.mark_modified();
                }
                let ids = &self.sel.tracks;
                for t in self.project.timeline.audio_tracks.iter_mut().filter(|t| ids.contains(&t.id)) {
                    t.armed = on;
                }
            }
            Record => {
                if self.rec_start.is_some() {
                    self.stop_recording();
                } else {
                    self.start_recording();
                }
            }
            // R8a: busses and assignable FX chains.
            InsertAudioBus => {
                if !self.edit(edit::insert_bus) {
                    ui::hint(ctx, "A project has at most 26 busses.");
                }
            }
            InsertAssignableFx => {
                // The chain opens in the FX window, where its plug-ins are chosen (VEGAS opens the Plug-In Chooser first).
                let mut id = None;
                self.edit(|p| {
                    id = edit::insert_assignable_fx(p, Vec::new());
                    id.is_some()
                });
                match id {
                    Some(id) => self.open_fx(edit::FxTarget::Bus(id)),
                    None => ui::hint(ctx, "A project has at most 32 assignable FX chains."),
                }
            }
            RemoveAssignableFx => self.remove_fx = Some(dialogs::RemoveAssignableFx::new(&self.project)),
            // R9: an input bus is a Mixing Console strip, so the console opens on it.
            InsertInputBus => {
                if self.edit(|p| edit::insert_input_bus(p).is_some()) {
                    self.dock.open(crate::ui::dock::Panel::MixingConsole);
                } else {
                    ui::hint(ctx, "A project has at most 26 input busses.");
                }
            }
            DefaultTrackFx => {
                self.default_track = self.focus_of(false).map(|i| dialogs::DefaultTrackProps::new(self.project.timeline.audio_tracks[i].id.clone()));
            }
            ChooseActiveTake | DeleteTake => {
                self.take_chooser = self.sel.clips.last().and_then(|id| dialogs::TakeChooser::open(&self.project, id, c == DeleteTake));
            }
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
                    // Esc also leaves edge-trimming mode.
                    (self.shuttle, self.sel.edge) = (0.0, None);
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
            GroupMute | GroupSolo => {
                let ids = self.sel.tracks.clone();
                self.edit(|p| edit::toggle_track_groups(p, &ids, c == GroupSolo));
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
            // Envelopes on the selected tracks they fit. V and P only insert (VEGAS's V shows and hides, it never
            // deletes points); Shift+V, Shift+P and a checked Insert/Remove item remove.
            VolumeEnvelope | PanEnvelope | RemoveVolumeEnvelope | RemovePanEnvelope | ToggleEnvelope(_) | ToggleVideoEnvelope(_) => {
                use crate::project::EnvKind;
                let (kind, on, video) = match c {
                    VolumeEnvelope => (EnvKind::Volume, true, false),
                    PanEnvelope => (EnvKind::Pan, true, false),
                    RemoveVolumeEnvelope => (EnvKind::Volume, false, false),
                    RemovePanEnvelope => (EnvKind::Pan, false, false),
                    ToggleEnvelope(k) => (k, !st.focus_envs.contains(&k), false),
                    ToggleVideoEnvelope(k) => (k, !st.focus_venvs.contains(&k), true),
                    _ => return,
                };
                // Audio and video tracks both have a Mute envelope: only the selected tracks of the menu's kind.
                let ids: Vec<String> = self.sel.tracks.iter().filter(|id| self.project.find_track(id).is_some_and(|f| f.0 == video)).cloned().collect();
                if self.edit(|p| edit::set_envelope(p, &ids, kind, on)) && on {
                    self.reveal_envelopes();
                }
            }
            ViewEnvelope(v) => {
                let hidden = &mut self.prefs.hidden_envelopes;
                match hidden.iter().position(|h| *h == v) {
                    Some(i) => _ = hidden.remove(i),
                    None => hidden.push(v),
                }
                self.sel.env = None; // a hidden envelope's points cannot stay selected
                self.save_prefs();
            }
            PitchUp | PitchDown | PitchCentUp | PitchCentDown | PitchOctUp | PitchOctDown => {
                let cents = match c {
                    PitchUp => 100.0,
                    PitchDown => -100.0,
                    PitchCentUp => 1.0,
                    PitchCentDown => -1.0,
                    PitchOctUp => 1200.0,
                    _ => -1200.0,
                };
                let ids = self.sel.clips.clone();
                self.edit(|p| edit::shift_pitch(p, &ids, cents));
            }
            EventVelocity | FreezeFrame | TransitionProgress => {
                let ids: Vec<String> = self.sel.clips.iter().filter(|id| self.project.find_clip(id).is_some_and(|r| r.is_video)).cloned().collect();
                let at = self.project.timeline.cursor_position;
                let done = self.edit(|p| match c {
                    EventVelocity => edit::toggle_velocity(p, &ids),
                    FreezeFrame => edit::freeze_frame(p, &ids, at),
                    _ => edit::toggle_progress(p, &ids),
                });
                if !done && c == FreezeFrame {
                    self.message = Some("Put the cursor inside the selected video event to freeze the frame there.".into());
                } else if done {
                    self.reveal_envelopes();
                }
            }
            EventEnvelope(k) => {
                let ids: Vec<String> = self.sel.clips.iter().filter(|id| self.project.find_clip(id).is_some_and(|r| !r.is_video)).cloned().collect();
                if self.edit(|p| edit::toggle_event_envelope(p, &ids, k)) {
                    self.reveal_envelopes();
                }
            }
            OfxEnvelopes => {
                self.prefs.ofx_envelopes = !self.prefs.ofx_envelopes;
                self.save_prefs();
            }
            NextFxEnvelope | PrevFxEnvelope => {
                // R8c (E / Shift+E): select the focus track's next or previous FX envelope (drawn on top, as hovered).
                let ofx = self.prefs.ofx_envelopes && theme::get().look() == UiLook::Vegas23;
                let all = self.sel.tracks.last().map_or(Vec::new(), |t| edit::fx_envelopes(&self.project, t, ofx));
                let n = all.len();
                if n == 0 {
                    self.message = Some("The track has no FX envelopes: its header menu's FX Automation Envelopes... adds them.".into());
                } else {
                    let cur = self.sel.env.as_ref().and_then(|e| all.iter().position(|o| *o == e.track));
                    let k = match (cur, c == NextFxEnvelope) {
                        (Some(i), true) => (i + 1) % n,
                        (Some(i), false) => (i + n - 1) % n,
                        (None, true) => 0,
                        (None, false) => n - 1,
                    };
                    self.sel.env = Some(ui::EnvSel { track: all[k].clone(), kind: crate::project::EnvKind::FxParam, keys: Vec::new() });
                    self.message = edit::fx_envelope_name(&self.project, &all[k]);
                    self.reveal_envelopes();
                }
            }
            AudioFollowsVelocity => {
                let on = !self.project.timeline.audio_follows_velocity;
                self.edit(|p| edit::set_audio_follows_velocity(p, on));
            }
            VideoBusTrack => {
                self.prefs.video_bus = !self.prefs.video_bus;
                self.save_prefs();
            }
            BusTracks => {
                self.prefs.bus_tracks = !self.prefs.bus_tracks;
                self.save_prefs();
            }
            DimMaster => {
                self.audio.master.dim.fetch_xor(true, std::sync::atomic::Ordering::Relaxed);
            }
            AudioProperties => self.settings.open_tab(&self.project, 1),
            Loudness(k) => {
                self.loudness.command(k);
                self.prefs.loudness.apply(k);
                self.save_prefs();
            }
            GenerateLoudnessLog => self.generate_loudness_log(ctx),
            RebuildNestedAudio => {
                // Nested sound is mixed live, so there is no proxy to render: the nested timelines are read again.
                crate::engine::nested::reload();
                self.message = Some("Nested timelines reloaded.".into());
            }
            OutputMeters | VerticalMeters | MeterWidth(_) => {
                let p = &mut self.prefs;
                match c {
                    OutputMeters => p.output_meters = !p.output_meters,
                    VerticalMeters => p.vertical_meters = !p.vertical_meters,
                    MeterWidth(w) => p.meter_width = w.min(2),
                    _ => {}
                }
                self.save_prefs();
            }
            ShowEnvelopes | LockEnvelopes => {
                let p = &mut self.prefs;
                if c == ShowEnvelopes {
                    p.show_envelopes = !p.show_envelopes;
                    self.sel.env = None;
                } else {
                    p.lock_envelopes = !p.lock_envelopes;
                }
                self.save_prefs();
            }
            Snapping => self.snapping = !self.snapping,
            SnapGrid | SnapAllEvents | SetGrid(_) => {
                let p = &mut self.prefs;
                match c {
                    SnapGrid => p.snap_grid = !p.snap_grid,
                    SnapAllEvents => p.snap_all = !p.snap_all,
                    SetGrid(g) => p.grid = g,
                    _ => {}
                }
                self.save_prefs();
            }
            // The ruler format is a project setting (Project Properties › Ruler), so it is an undo step like the rest.
            SetRuler(f) if f != self.project.settings.ruler.format => {
                self.edit(|p| {
                    p.settings.ruler.format = f;
                    p.mark_modified();
                    true
                });
            }
            GoTo | SetTimeAtCursor => {
                let shown = self.project.settings.ruler_time(self.project.timeline.cursor_position);
                self.time_entry = Some(dialogs::TimeEntry::new(c == SetTimeAtCursor, shown));
            }
            ProjectTempo => self.settings.open_tab(&self.project, 2),
            GridLeft | GridRight => {
                let grid = ui::grid_line(self.prefs.grid, &self.project.settings, self.pps());
                let tl = &mut self.project.timeline;
                tl.cursor_position = edit::grid_step(tl.cursor_position, grid, c == GridRight);
            }
            AutoCrossfade | AutoRipple | IgnoreGrouping | RippleMode(_) | Quickfade => {
                let p = &mut self.prefs;
                match c {
                    AutoCrossfade => p.auto_crossfade = !p.auto_crossfade,
                    Quickfade => p.quickfade = !p.quickfade,
                    AutoRipple => p.auto_ripple = !p.auto_ripple,
                    IgnoreGrouping => p.ignore_grouping = !p.ignore_grouping,
                    RippleMode(m) => p.ripple_mode = m,
                    _ => {}
                }
                self.save_prefs();
            }
            // Tools and groups
            NormalTool => self.tool = ui::EditTool::Normal,
            NextTool | PrevTool => self.tool = self.tool.cycle(c == NextTool),
            Tool(t) => self.tool = t,
            Group | RemoveFromGroup | ClearGroup => {
                let ids = self.sel.clips.clone();
                self.edit(|p| match c {
                    Group => edit::group(p, &ids),
                    RemoveFromGroup => edit::ungroup(p, &ids),
                    _ => edit::clear_groups(p, &ids),
                });
            }
            SelectGroup => (self.sel.clips, self.sel.env) = (edit::expand_groups(&self.project, &self.sel.clips), None),
            // Post-Edit Ripple: the last edit's ripple, once, in the item's mode.
            RippleAffected | RippleBus | RippleMarkers | RippleAll => {
                let mode = match c {
                    RippleAffected => edit::RippleMode::Affected,
                    RippleBus => edit::RippleMode::AffectedMarkers,
                    RippleMarkers => edit::RippleMode::AffectedMarkersOnly,
                    _ => edit::RippleMode::All,
                };
                let (ops, xfade) = (self.post_edit_ops().to_vec(), self.prefs.auto_crossfade);
                self.edit(|p| {
                    let before = p.clone();
                    let done = edit::ripple_all(p, &ops, mode);
                    if done && xfade {
                        edit::crossfade_moved(p, &before);
                    }
                    done
                });
            }
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
            LayoutDefault | LayoutVp14 | LayoutAudio | LayoutColor | LayoutSource | LayoutTimelineTop => {
                if let Some(b) = c.builtin_layout() {
                    self.load_layout(LayoutRef::Builtin(b));
                }
            }
            LayoutUser(n) => self.load_layout(LayoutRef::User(n)),
            // Back to the layout as last loaded or saved.
            ReloadLayout => {
                if let Some(l) = self.layout {
                    self.load_layout(l);
                }
            }
            OrganizeLayouts => {
                let look = theme::get().look();
                self.organize = Some(dialogs::OrganizeLayouts::new(std::array::from_fn(|n| self.prefs.layouts.get(&layout_key(look, n)).cloned())));
            }
            SaveLayoutUser(n) => self.save_layout(n, None),
            // Save Layout saves over the slot last loaded or saved; without one it asks, as Save Layout As does.
            SaveLayout | SaveLayoutAs => {
                let slot = match self.layout {
                    Some(LayoutRef::User(n)) => Some(n),
                    _ => None,
                };
                match slot.filter(|_| c == SaveLayout) {
                    Some(n) => self.save_layout(n, None),
                    None => {
                        let n = slot.or_else(|| st.layouts.iter().position(Option::is_none)).unwrap_or(0);
                        self.layout_as = Some((st.layouts[n].clone().unwrap_or_else(|| format!("Layout {n}")), n));
                    }
                }
            }
            ToggleDocks => self.dock.toggle_all(),
            TopDock => self.dock.toggle_area(Area::Top),
            BottomDock => self.dock.toggle_area(Area::Bottom),
            RightDock => self.dock.toggle_area(Area::Right),
            CollapseTrackList => self.dock.toggle_track_list(&theme::get()),
            MaximizeTimeline => self.dock.toggle_maximize(&theme::get()),
            NextWindow | PrevWindow => self.dock.cycle(c == PrevWindow),
            // Keys go to the timeline: no text field keeps them.
            FocusTimeline => {
                self.dock.focus = None;
                ctx.memory_mut(|m| {
                    if let Some(id) = m.focused() {
                        m.surrender_focus(id);
                    }
                });
            }
            Window(p) => self.dock.toggle(p),
            ShowWindow(p) => self.dock.open(p),
            // R14: on the Preferences › Preview Device display; off again here or by Esc on it.
            ExternalMonitor => {
                self.external = match self.external.take() {
                    Some(_) => None,
                    None => {
                        let mons = ui::monitors::list();
                        let m = ui::monitors::external(self.prefs.ext_display.as_deref(), &mons).cloned();
                        if m.is_none() {
                            self.message = Some("Preview on External Monitor: no display was found.".into());
                        }
                        m.map(|m| (m, super::PLACING))
                    }
                };
            }
            // Navigation and selection (R2)
            CursorToPlayhead => {
                // The cursor is the playhead while playing; this keeps it there when playback stops.
                if self.playing {
                    self.play_start = self.project.timeline.cursor_position;
                }
            }
            JogLeft | JogRight => {
                let d = if c == JogRight { exact_frame } else { -exact_frame };
                match self.sel.edge.clone() {
                    Some(e) => self.trim_edge(&e, d),
                    None if !self.playing => {
                        let tl = &mut self.project.timeline;
                        tl.cursor_position = (tl.cursor_position + d).max(0.0);
                    }
                    None => {}
                }
            }
            SelectToNextMarker => {
                let tl = &mut self.project.timeline;
                let cur = tl.cursor_position;
                let next = tl.markers.iter().map(|m| m.position).chain(tl.regions.iter().flat_map(|r| [r.start, r.end])).filter(|&p| p > cur + 1e-9).reduce(f64::min);
                if let Some(to) = next {
                    (tl.loop_region, tl.cursor_position) = (Some(edit::extend_selection(tl.loop_region, cur, to)), to);
                }
            }
            SelectLoopRegion | RestoreSelection => {
                let r = if c == SelectLoopRegion { self.sel_history.loop_region } else { self.sel_history.restore() };
                if r.is_some() {
                    self.project.timeline.loop_region = r;
                }
            }
            SelPrevEvent | SelNextEvent => {
                let cur = self.project.timeline.cursor_position;
                if let Some((a, b)) = edit::selected_event_span(&self.project, &self.sel.clips, cur, c == SelNextEvent) {
                    let tl = &mut self.project.timeline;
                    (tl.loop_region, tl.cursor_position) = (Some((a, b)), a);
                }
            }
            SelectAfterCursor => {
                (self.sel.clips, self.sel.env) = (edit::select_after(&self.project, self.project.timeline.cursor_position), None);
            }
            SelectEventsToEnd => {
                let Some(id) = self.sel.clips.last() else { return };
                let ids = edit::events_to_end(&self.project, id);
                self.sel.clips = if self.prefs.ignore_grouping { ids } else { edit::expand_groups(&self.project, &ids) };
                self.sel.env = None;
            }
            // Edge-trimming mode: [ selects the focus event's start, then walks to earlier edges on its track; ] the end
            // and later edges.
            SelectEventStart | SelectEventEnd => {
                let fwd = c == SelectEventEnd;
                let next = match &self.sel.edge {
                    Some(e) => edit::next_edge(&self.project, &e.clip, e.end, fwd),
                    None => self.sel.clips.last().and_then(|id| {
                        let k = self.project.clip(id)?;
                        Some((id.clone(), fwd, if fwd { k.position + k.duration } else { k.position }))
                    }),
                };
                if let Some((id, end, t)) = next {
                    let expanded = self.sel.edge.as_ref().is_some_and(|e| e.expanded);
                    self.sel.edge = Some(ui::EdgeSel { clip: id.clone(), end, expanded });
                    (self.sel.clips, self.sel.env) = (vec![id], None);
                    self.project.timeline.cursor_position = t;
                }
            }
            // Enter: the selected edge (else the focus event's edge nearest the cursor) in Expanded Edit Mode, with
            // the Trimmer showing the edit point. In Expanded Edit Mode it exits.
            ExpandedEdit => match self.sel.edge.as_ref().map(|e| e.expanded) {
                Some(true) => self.sel.edge = None,
                Some(false) => {
                    if let Some(e) = &mut self.sel.edge {
                        e.expanded = true;
                    }
                    self.dock.open(Panel::Trimmer);
                }
                None => {
                    let Some(id) = self.sel.clips.last().cloned() else { return };
                    let Some(k) = self.project.clip(&id) else { return };
                    let (a, b, cur) = (k.position, k.position + k.duration, self.project.timeline.cursor_position);
                    let end = (cur - b).abs() < (cur - a).abs();
                    self.project.timeline.cursor_position = if end { b } else { a };
                    (self.sel.clips, self.sel.env) = (vec![id.clone()], None);
                    self.sel.edge = Some(ui::EdgeSel { clip: id, end, expanded: true });
                    self.dock.open(Panel::Trimmer);
                }
            },
            ExitEditMode => self.sel.edge = None,
            // Nudge: in edge-trimming mode the frame and pixel keys trim the edge; otherwise they move the selected events
            // (and their groups) by a frame, a pixel or a track, one undo step each.
            NudgeFrameLeft | NudgeFrameRight | NudgePixelLeft | NudgePixelRight | NudgeTrackUp | NudgeTrackDown => {
                let sign = if matches!(c, NudgeFrameLeft | NudgePixelLeft) { -1.0 } else { 1.0 };
                let mut d = if matches!(c, NudgeFrameLeft | NudgeFrameRight) { exact_frame } else { 1.0 / self.pps() };
                if self.prefs.quantize {
                    d = (d / exact_frame).round().max(1.0) * exact_frame; // a pixel nudge stays on the frame grid
                }
                let dtrack = match c {
                    NudgeTrackUp => -1,
                    NudgeTrackDown => 1,
                    _ => 0,
                };
                let d = if dtrack == 0 { sign * d } else { 0.0 };
                match self.sel.edge.clone() {
                    Some(e) if dtrack == 0 => self.trim_edge(&e, d),
                    _ => {
                        let ids = if self.prefs.ignore_grouping { self.sel.clips.clone() } else { edit::expand_groups(&self.project, &self.sel.clips) };
                        let (lock, xfade) = (self.prefs.lock_envelopes, self.prefs.auto_crossfade);
                        self.edit(|p| {
                            let before = p.clone();
                            let done = edit::move_clips(p, &ids, d, dtrack, lock);
                            if done && xfade {
                                edit::auto_crossfade(p, &before, &ids); // as a drag does
                            }
                            done
                        });
                    }
                }
            }
            PasteRepeat => {
                let len = self.clipboard.iter().map(|(_, _, c)| c.position + c.duration).fold(0.0, f64::max);
                let s = &self.project.settings;
                self.paste_repeat = Some(dialogs::PasteRepeat::new(crate::project::format_time(len, s.ruler.format, s)));
            }
            MuteExclusive | SoloExclusive => {
                let ids = self.sel.tracks.clone();
                self.edit(|p| edit::exclusive_tracks(p, &ids, c == SoloExclusive));
            }
            ToggleListFocus => {
                self.sel.list_focus = !self.sel.list_focus;
                if self.sel.list_focus && self.sel.tracks.is_empty() {
                    let tl = &self.project.timeline;
                    self.sel.tracks = tl.video_tracks.iter().chain(&tl.audio_tracks).next().map(|t| t.id.clone()).into_iter().collect();
                }
            }
            ZoomFramePerThumb => {
                let (tl, s) = (&self.project.timeline, &self.project.settings);
                let tr = self.focus_of(true).or((!tl.video_tracks.is_empty()).then_some(0)).map(|i| &tl.video_tracks[i]);
                if let Some(tr) = tr {
                    let f = ui::frame_per_thumb_zoom(tr, s.width as f32 / s.height.max(1) as f32, s.frame_rate) / tl.zoom;
                    self.zoom_by(f);
                }
            }
            WaveZoomIn | WaveZoomOut => self.wave_zoom = (self.wave_zoom * if c == WaveZoomIn { 1.25 } else { 0.8 }).clamp(1.0, 64.0),
            // Preview at Cursor: plays the Cursor Preview Duration centred on the cursor as a temporary time selection
            // (looping with Loop Playback), then puts the cursor and selection back (`tick_cursor_preview`).
            PreviewAtCursor => {
                if self.cursor_preview.is_none() && self.rec_start.is_none() {
                    let tl = &mut self.project.timeline;
                    let (cur, half) = (tl.cursor_position, self.prefs.cursor_preview.max(0.1) / 2.0);
                    let span = ((cur - half).max(0.0), cur + half);
                    self.cursor_preview = Some((cur, tl.loop_region, span));
                    (tl.loop_region, tl.cursor_position) = (Some(span), span.0);
                    (self.playing, self.shuttle) = (true, 0.0);
                }
            }
            MuteAllAudio => self.mute_audio = !self.mute_audio,
            MuteAllVideo => self.mute_video = !self.mute_video,
            _ => {}
        }
    }

    /// New/Open/Exit, asking to save first if there are unsaved changes.
    pub(super) fn request(&mut self, ctx: &egui::Context, action: MenuAction) {
        if action == MenuAction::Exit && self.queue.busy() && self.exit_choice.is_none() {
            // R12: closing (or an update) would stop the render: ask first.
            self.render_guard = Some(self.install_on_exit.is_some());
            return;
        }
        if self.rec_start.is_some() {
            // A running take goes on the timeline first, so the save prompt covers it.
            self.stop_recording();
            self.recorded = None;
        }
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
            MenuAction::ImportVegas => {
                let Some((p, warnings)) = self.pending_import.take() else { return };
                if !warnings.is_empty() {
                    self.vegas_report = Some((IMPORTED_VEGAS.into(), warnings));
                }
                p
            }
            _ => {
                if let Some((exe, sha)) = self.install_on_exit.take() {
                    if let Err(e) = crate::updater::launch_installer(&exe, &sha, self.updater.auto) {
                        return self.message = Some(format!("Could not start the update installer: {e}"));
                    }
                }
                // R12: what the render guard chose, now that Milan Pro really closes.
                match self.exit_choice.take() {
                    Some(ui::render_queue::Guard::Keep) => self.queue.halt(),
                    Some(ui::render_queue::Guard::CancelRender) => self.queue.cancel_all(),
                    _ => {}
                }
                // Close (not process::exit) so the app is dropped and a running render's ffmpeg is killed.
                self.project.modified = false; // changes were saved or discarded; don't prompt again
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
        };
        self.project = project;
        self.project_serial += 1;
        self.history.clear();
        self.sel = ui::Selection::default();
        self.selected_clip = None;
        self.selected_media = None;
        self.minimized = None;
        // Copied events point at the old project's media.
        self.clipboard.clear();
        // A probe batch still running for the old project is dropped (its send fails).
        self.importer = crate::media::Importer::new();
        if action != MenuAction::ImportVegas {
            self.vegas = Default::default(); // New/Open after starting an import: the newer choice wins
        }
        // Files dropped on the old project's timeline are not placed in this one.
        self.file_drop.reset();
        self.drag_state = None;
        self.env_drag = None;
        self.event_props = None;
        self.take_chooser = None;
        self.default_track = None;
        self.remove_fx = None;
        self.recorded = None;
        (self.pan_crop, self.track_motion, self.fx_win) = (None, None, None);
        (self.paste_repeat, self.paste_attrs, self.cursor_preview, self.sel_history) = (None, None, None, Default::default());
        self.playing = false;
        self.message = None;
        if action == MenuAction::Open {
            self.remember();
            let missing: Vec<String> = self.project.media_pool.iter().filter(|m| !m.path.as_os_str().is_empty() && !m.path.exists()).map(|m| m.path.display().to_string()).collect();
            if !missing.is_empty() {
                self.message = Some(format!("Media offline: {}", missing.join("; ")));
            }
        }
        if std::mem::take(&mut self.props_after_new) && action == MenuAction::New {
            self.settings.open(&self.project);
        }
    }

    /// A finished VEGAS import: a project replaces this one through the save prompt, as Open does; the media a .veg
    /// scan found are imported into Project Media.
    pub(super) fn poll_vegas(&mut self, ctx: &egui::Context) {
        // The result waits while a dialog is up: replacing the project under Project Properties or a pending save
        // prompt (New/Open/Exit) would apply that dialog's answer to the wrong project.
        if self.modal_open() {
            if self.vegas.busy() {
                ctx.request_repaint_after(std::time::Duration::from_millis(250)); // look again once the dialog closes
            }
            return;
        }
        let Some(r) = self.vegas.poll() else { return };
        self.message = None;
        match r {
            Ok(Imported::Project(p, warnings)) => {
                self.pending_import = Some((p, warnings));
                self.request(ctx, MenuAction::ImportVegas);
            }
            Ok(Imported::Media(paths, missing)) => {
                let n = paths.len();
                self.import_new(ctx, paths);
                let dir = std::env::current_exe().ok().and_then(|e| Some(e.parent()?.join("vegas"))).filter(|d| d.is_dir());
                let from = dir.map_or("the vegas folder next to MilanPro.exe".into(), |d| d.display().to_string());
                let intro = format!(
                    "{n} media file(s) named in the .veg are added to Project Media. Tracks and events need VEGAS: choose Tools › \
                     Scripting › Run Script... there, pick \"Export to Milan Pro.cs\" from {from}, then import the .milan.json it writes."
                );
                self.vegas_report = Some((intro, missing));
            }
            Err(e) => self.message = Some(format!("VEGAS import failed: {e}")),
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
                (self.pending_open, self.new_settings, self.props_after_new, self.pending_import) = (None, None, false, None);
                self.install_on_exit = None; // the update waits for the next "Install and restart"
                self.exit_choice = None; // the renders carry on
            }
        }
        self.show_update(ctx);
        self.show_render_dialogs(ctx);
        if self.about {
            let (ok, close) = dialogs::modal(ctx, "about", "About Milan Pro", egui::vec2(360.0, 170.0), |ui| {
                ui.label(egui::RichText::new(format!("Milan Pro v{}", env!("CARGO_PKG_VERSION"))).strong());
                ui.label("Open-source nonlinear video editor.");
                ui.add_space(30.0);
                dialogs::button_row(ui, &[("OK", true)]).is_some()
            });
            self.about = !(ok || close);
        }
        if let Some(new) = self.prefs_dlg.show(ctx, &self.prefs, self.audio.latency_ms(), !self.dock.timeline_top()) {
            if new.track_colors != self.prefs.track_colors {
                theme::set_palette(ctx, new.palette());
            }
            if new.record_input != self.prefs.record_input && self.rec_start.is_none() {
                (self.input, self.input_failed) = (None, false); // the meter reopens on the new input
            }
            // VP16 Display: "Display timeline at bottom of main window" moves the dock area (kept in the layout).
            if theme::get().look() == UiLook::Vegas16 && self.prefs_dlg.timeline_bottom == self.dock.timeline_top() {
                self.dock.set_timeline_top(!self.prefs_dlg.timeline_bottom);
            }
            self.look = new.look;
            self.prefs = new;
            self.save_prefs();
        }
        self.show_organize(ctx);
        if let Some(r) = self.recorded.as_mut().and_then(|d| d.show(ctx, &self.project)) {
            match r {
                dialogs::Recorded::Done => self.recorded = None,
                dialogs::Recorded::Delete(id) => self.delete_recording(&id),
                dialogs::Recorded::Rename(id, name) => {
                    if let Err(e) = crate::record::rename_file(&mut self.project, &id, &name) {
                        self.message = Some(e);
                    }
                }
            }
        }
        if self.vegas_report.as_ref().is_some_and(|(intro, lines)| dialogs::report(ctx, "Import VEGAS Project", intro, lines)) {
            self.vegas_report = None;
        }
        self.show_time_entry(ctx);
        if let Some(d) = &mut self.paste_repeat {
            let s = self.project.settings.clone();
            let every = |x: &str| crate::project::parse_time(x, s.ruler.format, &s).filter(|v| *v > 0.0);
            if let Some(ok) = d.show(ctx, |x| every(x).is_some()) {
                if let Some(d) = self.paste_repeat.take().filter(|_| ok) {
                    self.paste_repeat(d.count.max(1) as usize, if d.even { every(&d.every) } else { None });
                }
            }
        }
        if let Some(ok) = self.paste_attrs.as_mut().and_then(|d| d.show(ctx)) {
            if let Some(d) = self.paste_attrs.take().filter(|_| ok) {
                let (clips, ids) = (self.clipboard.clone(), self.sel.clips.clone());
                self.edit(|p| edit::paste_attrs(p, &clips, &ids, d.attrs));
            }
        }
        dialogs::shortcuts_window(ctx, &mut self.shortcuts_open);
        dialogs::unfinished_window(ctx, &mut self.unfinished_open);
        self.show_customize(ctx);
        self.show_layout_as(ctx);
        if let Some(ok) = self.event_props.as_mut().and_then(|ep| ep.show(ctx, &self.project)) {
            // One undo step on OK, none when nothing changed (`apply_props`).
            if let Some(ep) = self.event_props.take().filter(|_| ok) {
                self.edit(|p| edit::apply_props(p, &ep.id, ep.draft));
            }
        }
        // The tool windows edit the project as they go (one undo step per gesture); closed when × or their target is gone.
        if self.pan_crop.as_mut().is_some_and(|w| !w.show(ctx, &mut self.project, &mut self.history, &mut self.thumbs)) {
            self.pan_crop = None;
        }
        if self.track_motion.as_mut().is_some_and(|w| !w.show(ctx, &mut self.project, &mut self.history)) {
            self.track_motion = None;
        }
        if let Some(w) = &mut self.fx_win {
            let (open, msg) = w.show(ctx, &mut self.project, &mut self.history, &mut self.user_presets);
            if msg.is_some() {
                self.message = msg;
            }
            if !open {
                self.fx_win = None;
            }
        }
        if let Some(ok) = self.take_chooser.as_mut().and_then(|tc| tc.show(ctx, &self.project)) {
            if let Some(tc) = self.take_chooser.take().filter(|_| ok) {
                self.edit(|p| edit::take_chooser(p, &tc.id, tc.pick, tc.delete));
            }
        }
        if let Some(ok) = self.default_track.as_mut().and_then(|d| d.show(ctx)) {
            if let Some(d) = self.default_track.take().filter(|_| ok) {
                self.set_default_track(&d);
            }
        }
        if let Some(ok) = self.remove_fx.as_mut().and_then(|d| d.show(ctx)) {
            if let Some(d) = self.remove_fx.take().filter(|_| ok) {
                self.edit(|p| edit::remove_assignable_fx(p, &d.ids()));
            }
        }
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

    /// Go to Time moves the cursor to the typed ruler time (on a frame with Quantize to Frames) and brings it into
    /// view; Set Time at Cursor makes the ruler read the typed time at the cursor (one undo step).
    fn show_time_entry(&mut self, ctx: &egui::Context) {
        let s = self.project.settings.clone();
        let Some(e) = &mut self.time_entry else { return };
        let set = e.set_ruler;
        let Some(answer) = e.show(ctx, |x| crate::project::parse_time(x, s.ruler.format, &s).is_some()) else { return };
        self.time_entry = None;
        let Some(text) = answer else { return };
        let cur = self.project.timeline.cursor_position;
        if set {
            let Some(off) = crate::project::parse_time(&text, s.ruler.format, &s).map(|v| v - cur).filter(|o| *o != s.ruler.offset) else { return };
            self.edit(|p| {
                p.settings.ruler.offset = off;
                p.mark_modified();
                true
            });
        } else if let Some(t) = s.parse_ruler_time(&text) {
            let (t, view) = (self.quantized(t.max(0.0)), self.view_secs(ctx));
            let tl = &mut self.project.timeline;
            tl.cursor_position = t;
            if t < tl.scroll_x || t > tl.scroll_x + view {
                tl.scroll_x = (t - view * 0.5).max(0.0);
            }
        }
    }

    /// View › Window Layouts › Organize Layouts: OK writes this look's slots 0-9 as arranged (renamed, moved,
    /// deleted); Activate loads the selected one now.
    fn show_organize(&mut self, ctx: &egui::Context) {
        let look = theme::get().look();
        let Some(r) = self.organize.as_mut().and_then(|d| d.show(ctx, look)) else { return };
        match r {
            dialogs::Organize::Activate(l, from) => {
                self.dock.apply(&l, &theme::get());
                self.layout = Some(LayoutRef::User(from));
            }
            dialogs::Organize::Cancel => self.organize = None,
            dialogs::Organize::Ok => {
                let Some(d) = self.organize.take() else { return };
                // Only the slots that changed, so another window's newer save of an untouched slot is kept.
                for (n, slot) in d.slots.iter().enumerate() {
                    let key = layout_key(look, n);
                    if self.prefs.layouts.get(&key) == slot.as_ref().map(|s| &s.0) {
                        continue;
                    }
                    match slot {
                        Some((l, _)) => self.prefs.layouts.insert(key.clone(), l.clone()),
                        None => self.prefs.layouts.remove(&key),
                    };
                    self.layouts_changed.insert(key);
                }
                // The current user layout follows its slot; a deleted one leaves no current layout.
                if let Some(LayoutRef::User(old)) = self.layout {
                    self.layout = d.slots.iter().position(|s| s.as_ref().is_some_and(|s| s.1 == old)).map(LayoutRef::User);
                }
                self.save_prefs();
            }
        }
    }

    /// R15: Customize Keyboard keeps its keys on OK; Customize (Timeline) Toolbar changes the bar as it goes, as VEGAS's do.
    fn show_customize(&mut self, ctx: &egui::Context) {
        if let Some(ok) = self.customize_keys.as_mut().and_then(|d| d.show(ctx)) {
            if let Some(d) = self.customize_keys.take().filter(|_| ok) {
                (self.prefs.keys, self.prefs.key_map) = (d.keys, d.map);
                ui::keymap::set_overrides(&self.prefs.keys);
                self.save_prefs();
            }
        }
        let Some(d) = &mut self.customize_bar else { return };
        let (changed, close) = d.show(ctx);
        if changed {
            let key = d.bar.key(d.look);
            if d.list == d.bar.defaults(d.look) {
                self.prefs.toolbars.remove(&key);
            } else {
                self.prefs.toolbars.insert(key, d.list.iter().map(|s| s.to_string()).collect());
            }
            self.save_prefs();
        }
        if close {
            self.customize_bar = None;
        }
    }

    /// View › Window Layouts › Save Layout As: a name and one of the slots 0-9.
    fn show_layout_as(&mut self, ctx: &egui::Context) {
        if self.layout_as.is_none() {
            return;
        }
        let look = theme::get().look();
        // As VEGAS: the slot is picked by the Alt+D, n shortcut that loads it.
        let slots: Vec<String> = (0..10)
            .map(|n| {
                let name = self.prefs.layouts.get(&layout_key(look, n)).map_or("<EMPTY>", |l| l.name.as_str());
                format!("{}  {name}", crate::ui::keymap::shortcut_text(LayoutUser(n), look))
            })
            .collect();
        let Some((name, slot)) = &mut self.layout_as else { return };
        let (hit, close) = dialogs::modal(ctx, "save_layout_as", "Save Layout As", egui::vec2(340.0, 140.0), |ui| {
            dialogs::row(ui, "Name:", 60.0, true, |ui| ui.add(egui::TextEdit::singleline(name).desired_width(250.0)));
            dialogs::row(ui, "Shortcut:", 60.0, true, |ui| {
                egui::ComboBox::from_id_salt("layout_as_slot").width(250.0).selected_text(slots[*slot].as_str()).show_ui(ui, |ui| {
                    for (n, label) in slots.iter().enumerate() {
                        ui.selectable_value(slot, n, label.as_str());
                    }
                });
            });
            ui.add_space(14.0);
            dialogs::button_row(ui, &[("OK", !name.trim().is_empty()), ("Cancel", true)])
        });
        if hit == Some(0) {
            if let Some((name, n)) = self.layout_as.take() {
                self.save_layout(n, Some(name.trim().to_string()));
            }
        } else if hit.is_some() || close {
            self.layout_as = None;
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

    /// R12: the render queue's progress (a finished Render to New Track file is placed), Exit waiting for it, Batch
    /// Render, Render Audio Tracks.
    pub(super) fn tick_renders(&mut self, ctx: &egui::Context) {
        for id in self.queue.tick(ctx) {
            self.place_rendered(&id);
        }
        // Cancelled or failed Render to New Track renders are not waited for.
        let queue = &self.queue;
        self.new_tracks.retain(|(id, ..)| queue.job(id).is_some_and(|j| matches!(j.state, crate::render::JobState::Queued | crate::render::JobState::Paused | crate::render::JobState::Running)));
        if self.exit_after_render && !self.queue.busy() {
            self.exit_after_render = false;
            self.message = None;
            self.request(ctx, MenuAction::Exit);
        }
        if let Some(b) = &mut self.batch {
            if let Some(ids) = b.show(ctx, &self.project, &self.prefs, &mut self.queue) {
                self.batch = None;
                self.batch_started(ids);
            }
        }
        if let Some(r) = &mut self.render_tracks {
            if let Some(ids) = r.show(ctx, &self.project, &self.prefs, &mut self.queue) {
                self.render_tracks = None;
                self.batch_started(ids);
            }
        }
    }

    /// Batch renders show their progress in the Render Queue window (modal, as VEGAS renders a script's renders),
    /// or in the status bar when Render in the background is on.
    fn batch_started(&mut self, ids: Vec<String>) {
        if !ids.is_empty() && !self.prefs.render_background {
            (self.queue_win.open, self.queue_win.batch) = (true, ids);
        }
    }

    /// A finished Render to New Track render: its file into Project Media and onto new tracks where the render began,
    /// in the project it was rendered from (not one opened since).
    fn place_rendered(&mut self, id: &str) {
        let Some(k) = self.new_tracks.iter().position(|(j, ..)| j == id) else { return };
        let (_, at, serial) = self.new_tracks.remove(k);
        let Some(out) = self.queue.job(id).map(|j| j.out.clone()) else { return };
        if serial != self.project_serial {
            self.message = Some(format!("{} is rendered; the project it was rendered from is closed, so it is not placed.", out.display()));
            return;
        }
        match crate::media::probe_media(&out) {
            Ok(m) => {
                let before = self.project.clone();
                let idx = match self.project.media_pool.iter().position(|x| x.path == m.path) {
                    Some(i) => i,
                    None => {
                        self.project.media_pool.push(m);
                        self.project.media_pool.len() - 1
                    }
                };
                let ids = edit::place_on_new_tracks(&mut self.project, idx, at);
                self.history.push(before);
                (self.sel.clips, self.sel.list_focus) = (ids, false);
            }
            Err(e) => self.message = Some(format!("Render to New Track: cannot read {}: {e}", out.display())),
        }
    }

    /// The guard asked at Exit (or Install and restart) while a render runs.
    fn show_render_dialogs(&mut self, ctx: &egui::Context) {
        let Some(install) = self.render_guard else { return };
        use crate::ui::render_queue::Guard;
        match ui::render_queue::guard(ctx, install) {
            Some(Guard::Wait) => {
                (self.render_guard, self.exit_after_render) = (None, true);
                self.message = Some(if install { "The update installs when the render queue has finished." } else { "Milan Pro closes when the render queue has finished." }.into());
            }
            // Applied when Milan Pro closes: a cancelled save prompt leaves the renders running.
            Some(g @ (Guard::CancelRender | Guard::Keep)) => {
                (self.render_guard, self.exit_choice) = (None, Some(g));
                self.request(ctx, MenuAction::Exit);
            }
            Some(Guard::Back) => {
                (self.render_guard, self.exit_after_render) = (None, false);
                self.install_on_exit = None; // the update waits for the next "Install and restart"
                self.message = None;
            }
            None => {}
        }
    }

    /// J / L shuttle (and the Rate slider) at rates other than normal playback: the sound plays at the shuttle's speed,
    /// varispeed (R4b), and its clock moves the cursor; without a device the frame time does.
    pub(super) fn tick_shuttle(&mut self, ctx: &egui::Context) {
        let rate = self.shuttle;
        if rate == 0.0 || rate == 1.0 {
            if self.shuttle_audio.take().is_some() {
                self.audio.stop();
            }
            return;
        }
        let cursor = self.project.timeline.cursor_position;
        // A seek (a click on the ruler) starts the sound again from the cursor; a new rate is heard on the fly.
        let seek = self.shuttle_audio.is_some() && cursor != self.shuttle_pos;
        if seek || !self.audio.running() || (self.shuttle_audio != Some(rate) && !self.audio.set_rate(rate)) {
            self.audio.start(ctx, &self.project, cursor, None, rate);
        } else {
            self.audio.set_plan(&self.project);
        }
        self.shuttle_audio = Some(rate);
        let dt = ctx.input(|i| i.stable_dt).min(0.1) as f64;
        let t = match self.audio.position() {
            Some(t) => t,
            None => cursor + rate * dt,
        };
        let tl = &mut self.project.timeline;
        tl.cursor_position = t.max(0.0);
        if tl.cursor_position == 0.0 {
            self.shuttle = 0.0;
        }
        self.shuttle_pos = tl.cursor_position;
        ctx.request_repaint();
    }

    /// `MILAN_DIALOG` (screenshot harness): open a dialog or menu at startup.
    pub(super) fn open_from_env(&mut self, ctx: &egui::Context) {
        let Ok(what) = std::env::var("MILAN_DIALOG") else { return };
        match what.as_str() {
            "prefs" => self.open_prefs(None),
            "prefs-display" => self.open_prefs(Some(dialogs::prefs::DISPLAY)),
            "prefs-audio" => self.open_prefs(Some(dialogs::prefs::AUDIO_DEVICE)),
            // R14: the Preview Device page; a floating Trimmer and the external preview (off-screen in the harness).
            "prefs-preview" => self.open_prefs(Some(dialogs::prefs::PREVIEW_DEVICE)),
            "float" => self.dock.float(crate::ui::dock::Panel::Trimmer, egui::Rect::from_min_size(egui::pos2(200.0, 200.0), egui::vec2(640.0, 400.0))),
            "external-preview" => self.exec(ctx, ExternalMonitor),
            "recorded" => {
                let audio = self.project.media_pool.iter().find(|m| m.media_type == crate::project::MediaType::Audio);
                self.recorded = audio.map(|m| dialogs::RecordedFiles::new(vec![m.id.clone()]));
            }
            "props" => self.settings.open(&self.project),
            "props-ruler" => self.settings.open_tab(&self.project, 2),
            "props-audio" => self.settings.open_tab(&self.project, 1),
            "props-summary" => self.settings.open_tab(&self.project, 3),
            // R16a: two notes (left unmodified: the harness never marks the project changed) in Project Notes.
            "notes" => {
                use crate::project::Note;
                self.project.notes = vec![Note { at: 1.0, text: "Tighten the cut into the title".into(), resolved: false }, Note { at: 4.5, text: "Music level checked".into(), resolved: true }];
                self.dock.open(crate::ui::dock::Panel::ProjectNotes);
            }
            "buses" => {
                // R8a: two busses and an assignable FX chain (its FX window open); audio tracks tall enough for the
                // multipurpose slider, the first one sending to Bus A.
                self.exec(ctx, InsertAudioBus);
                self.exec(ctx, InsertAudioBus);
                self.exec(ctx, InsertAssignableFx);
                let a = self.project.timeline.audio.buses[0].id.clone();
                for t in &mut self.project.timeline.audio_tracks {
                    t.height = 96.0;
                }
                if let Some(t) = self.project.timeline.audio_tracks.first_mut() {
                    edit::set_send(t, &a, 0.5, true);
                }
                self.project.modified = false; // the harness closes without a save prompt
            }
            "mixer" | "bus-tracks" => {
                // R8b: two busses (Bus A with a Volume envelope ramp) and an assignable FX chain, the first track
                // sending to Bus A; the Mixing Console open, or the Audio Bus Tracks shown.
                self.exec(ctx, InsertAudioBus);
                self.exec(ctx, InsertAudioBus);
                edit::insert_assignable_fx(&mut self.project, Vec::new()); // not through the command: no FX window
                let a = self.project.timeline.audio.buses[0].id.clone();
                if let Some(t) = self.project.timeline.audio_tracks.first_mut() {
                    edit::set_send(t, &a, 0.5, true);
                }
                edit::toggle_bus_envelope(&mut self.project, Some(&a), crate::project::EnvKind::Volume);
                if let Some(e) = self.project.timeline.audio.buses[0].envelopes.first_mut() {
                    edit::env_insert(e, 6.0, -12.0);
                }
                // Lit meters (no sound is played): each track and bus a different level, the first track clipping.
                let tl = &self.project.timeline;
                let ids: Vec<String> = tl.audio_tracks.iter().map(|t| t.id.clone()).chain(tl.audio.buses.iter().chain(&tl.audio.fx).map(|b| b.id.clone())).chain([ui::MASTER_LEVEL.to_string()]).collect();
                for (k, id) in ids.iter().enumerate() {
                    let d = if k == 0 { 0.0 } else { -6.0 - 5.0 * k as f32 };
                    ui::show_level(ctx, id, [d, d - 3.0]);
                }
                if what == "mixer" {
                    self.dock.open(crate::ui::dock::Panel::MixingConsole);
                } else {
                    self.prefs.bus_tracks = true;
                }
                self.project.modified = false; // the harness closes without a save prompt
            }
            "input-bus" => {
                // R9: an input bus (the Mixing Console opens on it) and the first audio track armed with Input
                // Monitoring Auto. The harness opens no audio device, so nothing is heard or recorded.
                self.exec(ctx, InsertInputBus);
                if let Some(t) = self.project.timeline.audio_tracks.first_mut() {
                    (t.armed, t.monitor) = (true, crate::project::Monitor::Auto);
                }
                let ids: Vec<String> = self.project.timeline.audio.inputs.iter().map(|b| b.id.clone()).chain([ui::MASTER_LEVEL.to_string()]).collect();
                for id in &ids {
                    ui::show_level(ctx, id, [-12.0, -14.0]);
                }
                self.project.modified = false; // the harness closes without a save prompt
            }
            "automation" => {
                // R8c: the first audio track (tall) in Write (Touch) with its automation controls shown, a Volume
                // envelope ramp, its compressor's Threshold as an FX envelope and its first event's Pan envelope; the
                // FX Automation chooser open on it.
                use crate::project::{AutoMode, EnvKind};
                if let Some(t) = self.project.timeline.audio_tracks.first_mut() {
                    (t.height, t.automation, t.auto_controls) = (96.0, AutoMode::Touch, true);
                    let (tid, comp, clip) = (t.id.clone(), t.fx.iter().find(|f| f.plugin == "milan.afx.compressor").map(|f| f.id.clone()), t.clips.first().map(|c| c.id.clone()));
                    edit::ensure_envelope(&mut self.project, &tid, EnvKind::Volume);
                    edit::with_envelope(&mut self.project, &tid, EnvKind::Volume, |e| {
                        edit::env_insert(e, 4.0, -9.0);
                        edit::env_insert(e, 8.0, 3.0);
                        true
                    });
                    if let Some(comp) = comp {
                        edit::toggle_fx_envelope(&mut self.project, &tid, &comp, "threshold");
                        edit::with_envelope(&mut self.project, &edit::fx_owner(&tid, &comp, "threshold"), EnvKind::FxParam, |e| {
                            edit::env_insert(e, 6.0, 0.4);
                            true
                        });
                    }
                    if let Some(c) = clip {
                        edit::toggle_event_envelope(&mut self.project, &[c], EnvKind::Pan);
                    }
                    ui::automation::open_chooser(ctx, &tid);
                }
                self.project.modified = false; // the harness closes without a save prompt
            }
            "goto" => self.exec(ctx, GoTo),
            "expanded-edit" => {
                // At the focus event's end, so the bracket and the outgoing frame show.
                if let Some(c) = self.sel.clips.last().and_then(|id| self.project.clip(id)) {
                    self.project.timeline.cursor_position = c.position + c.duration;
                }
                self.exec(ctx, ExpandedEdit);
            }
            "paste-repeat" => {
                self.exec(ctx, Copy);
                self.exec(ctx, PasteRepeat);
            }
            "render" => self.render.open(&self.project),
            "render-template" => {
                self.render.open(&self.project);
                self.render.customize();
            }
            "render-new-track" => self.render.open_new_track(&self.project),
            "render-queue" => {
                // R12: a queue as a session leaves it (nothing here renders: no job is Queued, nothing is saved).
                use crate::render::{BUILTIN, JobState, QueuedJob};
                let dir = std::env::temp_dir();
                let job = |k: usize, file: &str, state: JobState, range: (f64, f64)| QueuedJob { id: format!("demo{k}"), project: self.project.name.clone(), template: BUILTIN[k].clone(), out: dir.join(file), range, state, ..Default::default() };
                self.queue.jobs = vec![
                    job(5, "Show (YouTube).mp4", JobState::Done, (0.0, 12.0)),
                    job(3, "Show (mix).wav", JobState::Interrupted, (0.0, 12.0)),
                    job(12, "Show (master).mov", JobState::Paused, (4.0, 8.0)),
                    job(4, "Show (preview).mp3", JobState::Failed("Cannot write to the folder".into()), (0.0, 12.0)),
                ];
                self.exec(ctx, RenderQueue);
            }
            "batch-render" => self.exec(ctx, BatchRender),
            "render-tracks" => self.exec(ctx, RenderAudioTracks),
            "render-guard" => self.render_guard = Some(false),
            "save" => self.confirm = Some(MenuAction::Exit),
            "welcome" => self.welcome = Some(Default::default()),
            "shortcuts" => self.shortcuts_open = true,
            // R15: Customize Keyboard on Edit.Split; the toolbar dialogs.
            "customize-keyboard" => {
                self.exec(ctx, CustomizeKeyboard);
                if let Some(d) = &mut self.customize_keys {
                    d.select(Split);
                }
            }
            "customize-toolbar" => self.exec(ctx, CustomizeToolbar),
            "customize-timeline-toolbar" => self.exec(ctx, CustomizeTimelineToolbar),
            "unfinished" => self.unfinished_open = true,
            "event-props" => self.event_props = self.sel.clips.last().and_then(|id| dialogs::EventProps::open(&self.project, id)),
            "event-props-media" => self.event_props = self.sel.clips.last().and_then(|id| dialogs::EventProps::open(&self.project, id)).map(|e| e.media_tab()),
            "take-chooser" => self.take_chooser = self.sel.clips.last().and_then(|id| dialogs::TakeChooser::open(&self.project, id, false)),
            "pan-crop" => self.exec(ctx, EventPanCrop),
            "fx" => self.exec(ctx, VideoEventFx),
            "fx-output" => self.exec(ctx, VideoOutputFx),
            "track-motion" => {
                self.sel.tracks = self.project.timeline.video_tracks.first().map(|t| t.id.clone()).into_iter().collect();
                self.exec(ctx, TrackMotion);
            }
            "about" => self.about = true,
            "video-bus" => {
                // The bus row with a Motion Blur Amount ramp and a Fade to Color envelope on it.
                use crate::project::{Curve, EnvKind, Envelope, Keyframe, Keyframes};
                let env = |kind, keys: &[(f64, f32)]| Envelope { kind, points: Keyframes { keys: keys.iter().map(|&(t, v)| Keyframe { t, v, curve: Curve::Linear }).collect() } };
                self.project.timeline.master.video_envelopes = vec![env(EnvKind::MotionBlur, &[(0.0, 0.0), (4.0, 0.5), (8.0, 0.1)]), env(EnvKind::FadeToColor, &[(0.0, 0.0), (10.0, -1.0)])];
                self.exec(ctx, VideoBusTrack);
            }
            "compositors" => self.exec(ctx, ShowWindow(crate::ui::dock::Panel::Compositors)),
            "layout-as" => self.exec(ctx, SaveLayoutAs),
            // R13: the built-in layouts, the dock commands and Organize Layouts (with two demo slots, never saved).
            "layout-classic" => self.exec(ctx, LayoutVp14),
            "layout-audio" => self.exec(ctx, LayoutAudio),
            "layout-color" => self.exec(ctx, LayoutColor),
            // R16b: every scope at once in the Color Correction layout; the Video Scopes Settings dialog; Split-Screen
            // View (the left half without the Sepia put on the Video Output); Overlays › Red as Grayscale.
            "scopes-all" => {
                self.prefs.scopes.view = crate::ui::ScopeView::All;
                self.exec(ctx, LayoutColor);
            }
            "scopes-settings" => {
                self.exec(ctx, LayoutColor);
                self.scopes.settings = Some(self.prefs.scopes.levels);
            }
            "split" => {
                use crate::engine::plugins::{find, new_fx};
                self.project.timeline.master.video_fx.push(new_fx(find("milan.fx.sepia").unwrap(), None));
                (self.preview_view.split, self.preview_view.split_region) = (true, [0.0, 0.0, 0.5, 1.0]);
            }
            "isolate" => self.preview.isolate = crate::decode::Isolate::RedGray,
            "layout-source" => self.exec(ctx, LayoutSource),
            "layout-timeline-top" => self.exec(ctx, LayoutTimelineTop),
            "docks-hidden" => self.exec(ctx, ToggleDocks),
            "timeline-max" => self.exec(ctx, MaximizeTimeline),
            "tabs-top" => self.prefs.tabs_on_top = true,
            "organize-layouts" => {
                let look = theme::get().look();
                self.prefs.layouts.insert(layout_key(look, 1), Dock::builtin(crate::ui::dock::Builtin::AudioMixing, &theme::get()).layout("Mixing"));
                self.prefs.layouts.insert(layout_key(look, 4), self.dock.layout("Editing"));
                self.exec(ctx, OrganizeLayouts);
            }
            "vegas-report" => {
                let lines = ["Track 1, event at 00:00:01;00: event FX (VEGAS Gaussian Blur)", r"Media offline: D:\Footage\interview.mxf (file not found)"];
                self.vegas_report = Some((IMPORTED_VEGAS.into(), lines.map(String::from).to_vec()));
            }
            "update" => self.updater.check(ctx, true, std::time::Duration::ZERO),
            m => {
                if let Some(k) = m.strip_prefix("menu:").and_then(|k| k.parse::<usize>().ok()) {
                    menus::request_open(ctx, k);
                }
            }
        }
    }
}
