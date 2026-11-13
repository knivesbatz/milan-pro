//! R17a: Tools › Multicamera › Synchronize Audio / Media Timecode / Date/Time Stamp to Align Events. As in VEGAS, the
//! selected events on the topmost track stay where they are and the other selected events move to match them. The
//! moves are one undo step and follow what a Move drag follows: locked events stay, group mates move along (unless
//! Ignore Event Grouping), Auto Ripple or Post-Edit Ripple, Lock Envelopes to Events and automatic crossfades. Sound is
//! matched in the background (`engine::sync::AudioJob`), with its progress and Cancel in the status bar.
use eframe::egui;
use crate::edit::{self, SyncUnit};
use crate::engine::plan::ASrc;
use crate::engine::sync::{self, AudioJob, By};
use crate::project::{Clip, MediaItem, Project};
use super::MeridianApp;

/// Synchronize Audio to Align Events analysing.
pub(super) struct Syncing {
    pub job: AudioJob,
    units: Vec<SyncUnit>,
    /// The units analysed (index into `units`), with their key event's position when the analysis started.
    asked: Vec<(usize, f64)>,
}

fn media<'a>(p: &'a Project, c: &Clip) -> Option<&'a MediaItem> {
    p.media_pool.iter().find(|m| Some(&m.id) == c.media_id.as_ref())
}

/// Audio event `c`'s sound as Synchronize reads it: its media's stream as the mixer plays it (in point, rate).
// shortcut: reversed events and nested timelines are not read (they are reported as having no audio to match).
fn sound(p: &Project, c: &Clip) -> Option<sync::Event> {
    let m = media(p, c).filter(|m| m.sample_rate.is_some() && m.generator.is_none() && !m.nested && !c.reverse)?;
    let stream = c.stream.min(m.streams(false) - 1);
    let src = ASrc { path: m.path.clone(), pos: 0.0, dur: c.duration, in_point: m.file_time(c.in_point), rate: c.rate.max(1e-3), reverse: false, stream, pitch: 0.0, nested: None };
    Some(sync::Event { src, pos: c.position, len: c.duration })
}

/// The command's name, and what it matches by.
fn names(by: By) -> (&'static str, &'static str) {
    match by {
        By::Audio => ("Synchronize Audio", "audio"),
        By::Timecode => ("Synchronize Media Timecode", "media timecode"),
        By::Date => ("Synchronize Date/Time Stamp", "date/time stamp"),
    }
}

impl MeridianApp {
    /// Tools › Multicamera › Synchronize ... to Align Events on the selected events.
    pub(super) fn synchronize(&mut self, ctx: &egui::Context, by: By) {
        let (what, kind) = names(by);
        if self.syncing.is_some() {
            self.message = Some(format!("{what}: the audio is still being analysed (Cancel stops it)."));
            return;
        }
        let p = &self.project;
        let units = edit::sync_units(p, &self.sel.clips, self.prefs.ignore_grouping, |c, video| match by {
            By::Audio => !video && sound(p, c).is_some(),
            _ => media(p, c).and_then(|m| sync::stamp(m, by)).is_some(),
        });
        if units.iter().all(|u| u.reference) {
            self.message = Some(format!("{what}: select events on two or more tracks. Those on the topmost track stay; the others move to match them."));
            return;
        }
        if !units.iter().any(|u| u.reference && u.key.is_some()) {
            self.message = Some(format!("{what}: the selected events on the topmost track have no {kind} to match. Nothing moved."));
            return;
        }
        let key = |u: &SyncUnit| u.key.as_deref().and_then(|k| p.clip(k));
        if by == By::Audio {
            let refs = units.iter().filter(|u| u.reference).filter_map(|u| sound(p, key(u)?)).collect();
            let asked: Vec<(usize, f64)> = units.iter().enumerate().filter(|(_, u)| !u.reference).filter_map(|(k, u)| Some((k, key(u)?.position))).collect();
            let others = asked.iter().filter_map(|&(k, _)| sound(p, key(&units[k])?)).collect();
            let range = p.timeline.loop_region.filter(|(a, b)| b > a);
            self.syncing = Some(Syncing { job: AudioJob::start(refs, others, range, ctx.clone()), units, asked });
            return;
        }
        // Timecode and date: a unit's stamp is its media's at the key event's start (its in point into the file).
        let stamp = |u: &SyncUnit| {
            let c = key(u)?;
            let m = media(p, c)?;
            Some((c.position, sync::stamp(m, by)? + m.file_time(c.in_point)))
        };
        let Some(anchor) = units.iter().filter(|u| u.reference).find_map(stamp) else { return };
        let results = units.iter().filter(|u| !u.reference).map(|u| stamp(u).map(|(pos, s)| anchor.0 + (s - anchor.1) - pos).ok_or_else(|| format!("no {kind}"))).collect();
        self.apply_sync(by, &units, results);
    }

    /// Each frame: a finished audio analysis moves the events it matched.
    pub(super) fn tick_sync(&mut self) {
        let Some(found) = self.syncing.as_ref().filter(|_| !self.dragging()).and_then(|s| s.job.poll()) else { return };
        let s = self.syncing.take().expect("syncing");
        if s.job.cancelled() {
            self.message = Some("Synchronize Audio cancelled: nothing moved.".into());
            return;
        }
        let p = &self.project;
        let mut found: std::collections::HashMap<usize, (f64, Result<f64, String>)> = s.asked.iter().zip(found).map(|(&(k, was), r)| (k, (was, r))).collect();
        let results = (0..s.units.len())
            .filter(|&k| !s.units[k].reference)
            .map(|k| {
                let Some((was, r)) = found.remove(&k) else { return Err("no audio".to_string()) };
                // An event moved (or gone) while it was analysed is left where it is now.
                match s.units[k].key.as_deref().and_then(|id| p.clip(id)).map(|c| c.position) {
                    Some(now) if now == was => r.map(|at| at - now),
                    _ => Err("it changed while the audio was analysed".into()),
                }
            })
            .collect();
        self.apply_sync(By::Audio, &s.units, results);
    }

    /// Move the non-reference `units` by `results` (seconds each, in order, or why not) in one undo step
    /// (`edit::plan_sync`, `edit::apply_sync`), and say what happened.
    fn apply_sync(&mut self, by: By, units: &[SyncUnit], results: Vec<Result<f64, String>>) {
        let (what, _) = names(by);
        let plan = edit::plan_sync(&self.project, units, results);
        if !plan.moves.is_empty() {
            let (lock_env, xfade) = (self.prefs.lock_envelopes, self.prefs.auto_crossfade);
            self.ripple_edit(false, |p, mode| edit::apply_sync(p, units, &plan.moves, mode, lock_env, xfade));
        }
        let (n, refused) = (plan.aligned, plan.refused.join(", "));
        let s = if n == 1 { "" } else { "s" };
        self.message = Some(match (n, refused.is_empty()) {
            (_, true) => format!("{what}: {n} event{s} aligned with the topmost track."),
            (0, false) => format!("{what}: unable to align the events. Nothing moved: {refused}."),
            _ => format!("{what}: {n} event{s} aligned; unable to align {refused}."),
        });
    }
}
