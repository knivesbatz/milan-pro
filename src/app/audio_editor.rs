//! R18a: Tools › Open in Audio Editor (Ctrl+E) and Open Copy in Audio Editor: the focus audio event's media file (or a
//! copy of it, which becomes the event's active take) opens in Preferences › Audio › Preferred audio editor. The file
//! is watched while it is in the project: when the editor saves it (its modified time changes) the media is read again
//! and its peaks rebuilt, as VEGAS does.
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant, SystemTime};
use eframe::egui;
use crate::edit;
use crate::project::MediaItem;
use super::MeridianApp;

/// How often the watched files' modified times are read.
const EVERY: Duration = Duration::from_millis(500);

/// The files sent to the audio editor and the probes of the ones that changed.
pub(super) struct Watch {
    /// (file, modified time last seen)
    files: Vec<(PathBuf, Option<SystemTime>)>,
    checked: Option<Instant>,
    tx: Sender<(PathBuf, Result<MediaItem, String>)>,
    rx: Receiver<(PathBuf, Result<MediaItem, String>)>,
}

impl Default for Watch {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self { files: Vec::new(), checked: None, tx, rx }
    }
}

fn stamp(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// A free name for a copy of `path` beside it: "Take Copy.wav", "Take Copy 2.wav"...
pub(super) fn copy_name(path: &Path) -> PathBuf {
    let stem = path.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    let ext = path.extension().map_or_else(String::new, |e| format!(".{}", e.to_string_lossy()));
    let name = |k: u32| if k == 1 { format!("{stem} Copy{ext}") } else { format!("{stem} Copy {k}{ext}") };
    (1..).map(|k| path.with_file_name(name(k))).find(|p| !p.exists()).expect("a free name")
}

/// `fresh` (the file probed again) into `old`: the file's properties change, its place in the project (id, name, a
/// subclip's span) stays. True when anything changed.
pub(super) fn refresh(old: &mut MediaItem, fresh: MediaItem) -> bool {
    let before = old.clone();
    if old.subclip.is_none() {
        old.duration = fresh.duration;
    }
    (old.sample_rate, old.codec, old.audio_streams, old.video_streams, old.markers) = (fresh.sample_rate, fresh.codec, fresh.audio_streams, fresh.video_streams, fresh.markers);
    serde_json::to_value(&before).ok() != serde_json::to_value(&*old).ok()
}

impl MeridianApp {
    /// Open in Audio Editor, or with `copy` Open Copy in Audio Editor (the copy becomes the event's active take, one
    /// undo step).
    pub(super) fn open_in_audio_editor(&mut self, copy: bool) {
        let Some(editor) = self.prefs.audio_editor.clone() else {
            self.message = Some("Choose your audio editor in Preferences › Audio first.".into());
            self.open_prefs(Some(crate::ui::dialogs::prefs::AUDIO));
            return;
        };
        let Some(id) = self.focus_clip(false) else { return };
        let Some((m, in_point)) = self.project.clip(&id).and_then(|c| Some((edit::media_of(&self.project, c)?.clone(), c.in_point))) else { return };
        if m.generator.is_some() || m.nested || !m.path.is_file() {
            self.message = Some(format!("{} has no audio file to edit.", m.name));
            return;
        }
        let path = if copy {
            let to = copy_name(&m.path);
            if let Err(e) = std::fs::copy(&m.path, &to) {
                self.message = Some(format!("Could not copy {}: {e}", m.path.display()));
                return;
            }
            let name = to.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            let item = MediaItem { id: uuid::Uuid::new_v4().to_string(), path: to.clone(), name, rel_path: None, ..m };
            let added = self.edit(|p| {
                let Some(r) = p.find_clip(&id) else { return false };
                edit::add_as_take(&mut p.timeline.tracks_mut(r.is_video)[r.track_idx].clips[r.clip_idx], &item, in_point);
                p.media_pool.push(item);
                p.mark_modified();
                true
            });
            if !added {
                let _ = std::fs::remove_file(&to); // the copy just made, which nothing uses (a drag held the edit)
                return;
            }
            to
        } else {
            m.path.clone()
        };
        // Not through `proc::spawn`: the editor is the user's own program, which must not die (with unsaved work) when
        // Milan Pro exits; the loudness log opens the same way.
        match std::process::Command::new(&editor).arg(&path).spawn() {
            Ok(_) => {
                let w = &mut self.audio_edits;
                if !w.files.iter().any(|f| f.0 == path) {
                    w.files.push((path.clone(), stamp(&path)));
                }
                self.message = Some(format!("{} is open in {}.", path.display(), editor.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned())));
            }
            Err(e) => self.message = Some(format!("Could not start the audio editor {}: {e}", editor.display())),
        }
    }

    /// Each frame: a watched file saved since it was last read is probed again (on a thread); a probe that finished
    /// updates its media and rebuilds their peaks. Files no media in the project uses any more are let go.
    pub(super) fn tick_audio_editor(&mut self, ctx: &egui::Context) {
        while let Ok((path, r)) = self.audio_edits.rx.try_recv() {
            match r {
                Ok(fresh) => {
                    let (mut ids, mut changed) = (Vec::new(), false);
                    for m in self.project.media_pool.iter_mut().filter(|m| m.path == path) {
                        changed |= refresh(m, fresh.clone());
                        ids.push(m.id.clone());
                    }
                    // The sound changed even when the length did not: new peaks, and Normalize measures again.
                    for id in &ids {
                        self.waves.rebuild(Some(id));
                        for c in self.project.timeline.audio_tracks.iter_mut().flat_map(|t| &mut t.clips).filter(|c| c.media_id.as_deref() == Some(id.as_str())) {
                            c.normalized = None;
                        }
                    }
                    if changed {
                        // The file changed on disk, not the project: no undo step, but the project is saved with it.
                        self.project.mark_modified();
                    }
                    self.message = Some(format!("{} changed in the audio editor and was read again.", path.display()));
                }
                Err(e) => self.message = Some(format!("Could not read {} again: {e}", path.display())),
            }
        }
        let w = &mut self.audio_edits;
        if w.files.is_empty() || w.checked.is_some_and(|t| t.elapsed() < EVERY) {
            return;
        }
        w.checked = Some(Instant::now());
        ctx.request_repaint_after(EVERY);
        let pool = &self.project.media_pool;
        w.files.retain(|f| pool.iter().any(|m| m.path == f.0));
        for (path, seen) in &mut w.files {
            let now = stamp(path);
            if now != *seen {
                *seen = now;
                let (tx, path, ctx) = (w.tx.clone(), path.clone(), ctx.clone());
                std::thread::spawn(move || {
                    let r = crate::media::probe_media(&path).map_err(|e| e.to_string());
                    let _ = tx.send((path, r));
                    ctx.request_repaint();
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A copy gets a free name beside the file; a re-read file updates the media but keeps its id, name and subclip.
    #[test]
    fn copies_and_reloads() {
        let dir = std::env::temp_dir().join(format!("milan-audio-editor-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let take = dir.join("Take.wav");
        std::fs::write(&take, b"x").unwrap();
        let first = copy_name(&take);
        assert_eq!(first.file_name().unwrap(), "Take Copy.wav");
        std::fs::write(&first, b"x").unwrap();
        assert_eq!(copy_name(&take).file_name().unwrap(), "Take Copy 2.wav");
        for p in [&first, &take] {
            std::fs::remove_file(p).unwrap();
        }
        std::fs::remove_dir(&dir).unwrap();

        let mut old = MediaItem { id: "m".into(), name: "Take.wav".into(), duration: 4.0, sample_rate: Some(44_100), ..Default::default() };
        let fresh = MediaItem { id: "new".into(), name: "x".into(), duration: 6.0, sample_rate: Some(48_000), markers: vec![(1.0, "Cue".into())], ..Default::default() };
        assert!(refresh(&mut old, fresh.clone()));
        assert_eq!((old.id.as_str(), old.name.as_str(), old.duration, old.sample_rate, old.markers.len()), ("m", "Take.wav", 6.0, Some(48_000), 1));
        assert!(!refresh(&mut old, fresh.clone()), "nothing new");
        let mut sub = MediaItem { duration: 1.0, subclip: Some(crate::project::Subclip { parent: "m".into(), start: 0.5 }), ..Default::default() };
        refresh(&mut sub, fresh);
        assert_eq!(sub.duration, 1.0, "a subclip keeps its span");
    }
}
