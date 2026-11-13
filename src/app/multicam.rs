//! R17b: Tools › Multicamera: Create Multicamera Track, Expand to Multiple Tracks..., Enable Multicamera Editing (the
//! Video Preview shows the take grid; a click on a take, or 1-9, chooses it at the cursor, also while playing),
//! Edit Multicamera Audio with Video and Show Full Frame in External Monitor. Every take choice is one undo step.
use eframe::egui;
use crate::edit;
use crate::project::Transition;
use crate::ui::dialogs;
use super::MeridianApp;

impl MeridianApp {
    /// Each frame: the preview shows the grid while multicamera editing is on (and the external monitor the full frame
    /// if asked), and playback keeps the multicamera audio's other takes decoding when they may be switched to.
    pub(super) fn tick_multicam(&mut self) {
        self.preview.multicam = self.multicam;
        self.preview.ext_full = self.prefs.multicam_full && self.external.is_some();
        self.audio.standby = self.multicam && self.prefs.multicam_audio;
    }

    /// Events are selected on two or more tracks of one kind: Create Multicamera Track can make a track of them.
    pub(super) fn can_create_multicam(&self) -> bool {
        let refs = self.project.find_clips(&self.sel.clips);
        [true, false].into_iter().any(|v| {
            let mut tracks: Vec<usize> = refs.iter().filter(|r| r.is_video == v).map(|r| r.track_idx).collect();
            tracks.sort_unstable();
            tracks.dedup();
            tracks.len() >= 2
        })
    }

    /// The selected tracks that have multicamera events (Expand to Multiple Tracks works on them).
    pub(super) fn multicam_tracks(&self) -> Vec<String> {
        let tl = &self.project.timeline;
        self.sel.tracks.iter().filter(|id| tl.video_tracks.iter().chain(&tl.audio_tracks).any(|t| t.id == **id && t.clips.iter().any(edit::is_multicam))).cloned().collect()
    }

    /// Tools › Multicamera › Create Multicamera Track on the selected events (one undo step); the new events are
    /// selected.
    pub(super) fn create_multicam(&mut self) {
        let ids = self.sel.clips.clone();
        let mut made = Err(String::new());
        self.edit(|p| {
            made = edit::create_multicam(p, &ids);
            made.is_ok()
        });
        self.message = Some(match made {
            Ok(new) => {
                let n = new.len();
                self.sel.clips = new;
                format!("Create Multicamera Track: {n} multicamera event{}. Tools › Multicamera › Enable Multicamera Editing chooses the takes.", if n == 1 { "" } else { "s" })
            }
            Err(e) => format!("Create Multicamera Track: {e}. Nothing changed."),
        });
    }

    /// Choose take `k` (from 0) at the cursor, crossfading into it with `xfade` (over Preferences › Editing ›
    /// Cut-to-overlap conversion): VEGAS's click on a take or number key in multicamera editing. Stopped with a time
    /// selection, the selection gets the take. The multicamera audio follows with Edit Multicamera Audio with Video.
    pub(super) fn choose_take(&mut self, k: usize, xfade: bool) {
        let t = self.quantized(self.project.timeline.cursor_position);
        let range = self.project.timeline.loop_region.filter(|(a, b)| b > a && !self.playing);
        let (audio, overlap) = (self.prefs.multicam_audio, self.prefs.cut_overlap);
        let done = self.edit(|p| {
            let video = edit::switch_take(p, true, t, range, k);
            let audio = audio && edit::switch_take(p, false, t, range, k);
            if video && xfade && range.is_none() {
                // shortcut: the sound cuts where the picture crossfades; overlap the audio too if users ask.
                if let Some((ti, _)) = edit::multicam_at(&p.timeline, true, t) {
                    let id = p.timeline.video_tracks[ti].id.clone();
                    edit::set_transition(p, &id, t, Transition { plugin: "milan.tr.dissolve".into(), ..Default::default() }, overlap, true);
                }
            }
            video || audio
        });
        if !done && edit::multicam_at(&self.project.timeline, true, t).is_none() {
            self.message = Some("Multicamera editing: no multicamera event at the cursor (Tools › Multicamera › Create Multicamera Track makes one).".into());
        }
    }

    /// Tools › Multicamera › Expand to Multiple Tracks...: asks whether to keep the unused takes (VEGAS's question).
    pub(super) fn ask_expand(&mut self) {
        let ids = self.multicam_tracks();
        if !ids.is_empty() {
            self.expand_ask = Some(ids);
        }
    }

    /// The Expand to Multiple Tracks question, while it is asked: Yes keeps the unused takes as muted events, No deletes
    /// them, Cancel (or ×, Esc) does nothing.
    pub(super) fn show_expand(&mut self, ctx: &egui::Context) {
        let Some(ids) = self.expand_ask.clone() else { return };
        let (choice, close) = dialogs::modal(ctx, "expand_multicam", "Expand to Multiple Tracks", egui::vec2(430.0, 150.0), |ui| {
            ui.label("Keep the unused takes as muted events? Keeping them lets you create the multicamera track again later.");
            ui.add_space(14.0);
            dialogs::button_row(ui, &[("Yes", true), ("No", true), ("Cancel", true)])
        });
        if let Some(k) = choice.filter(|&k| k < 2) {
            self.edit(|p| edit::expand_multicam(p, &ids, k == 0));
        }
        if choice.is_some() || close {
            self.expand_ask = None;
        }
    }
}
