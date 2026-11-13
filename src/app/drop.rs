//! Files dragged to the timeline from Windows or the Explorer window. They are probed in the background while they
//! hover, so the timeline can show their events, and a drop is placed once every probe of its files is done, at the
//! time and on the track captured when it was dropped.
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use eframe::egui;
use crate::edit;
use crate::media::{Importer, in_pool, merge_new};
use crate::project::{MediaItem, Project};

/// A drop waiting for probes: its files and where it goes on the timeline (None: only imported).
struct Pending {
    paths: Vec<PathBuf>,
    place: Option<Place>,
    /// R18b: Insert › Slideshow: the pictures are laid out as this says (at `place`'s time, on its track).
    slideshow: Option<edit::Slideshow>,
}

/// The target track's id (None: new tracks), the time and how the drop menu adds the files.
pub type Place = (Option<String>, f64, edit::DropMode);

/// A placed drop: the project before it (None when nothing changed), the new events, files that failed to probe,
/// and whether a locked track refused readable media.
pub struct Placed {
    pub before: Option<Project>,
    pub ids: Vec<String>,
    pub errors: Vec<String>,
    pub refused: bool,
}

pub struct FileDrop {
    importer: Importer,
    /// Files whose probe has been started.
    asked: HashSet<PathBuf>,
    probed: HashMap<PathBuf, Result<MediaItem, String>>,
    pending: Vec<Pending>,
}

impl FileDrop {
    pub fn new() -> Self {
        Self { importer: Importer::new(), asked: HashSet::new(), probed: HashMap::new(), pending: Vec::new() }
    }

    /// Start probing the `paths` that are not in the pool and not asked for yet. With nothing hovering or waiting,
    /// old probes are forgotten, so a file changed on disk is probed again next time.
    pub fn hover(&mut self, ctx: &egui::Context, paths: &[PathBuf], pool: &[MediaItem]) {
        if paths.is_empty() && self.pending.is_empty() {
            self.asked.clear();
            self.probed.clear();
            return;
        }
        let new: Vec<PathBuf> = paths.iter().filter(|p| !in_pool(pool, p) && self.asked.insert((*p).clone())).cloned().collect();
        if !new.is_empty() {
            self.importer.import(ctx, new);
        }
    }

    /// The media of `paths` (pool items first), or None while a probe runs. Unreadable files are left out.
    pub fn items(&self, paths: &[PathBuf], pool: &[MediaItem]) -> Option<Vec<MediaItem>> {
        let mut out = Vec::new();
        for p in paths {
            if let Some(m) = pool.iter().find(|m| m.path == *p && m.subclip.is_none() && !m.is_offline()) {
                out.push(m.clone());
            } else if let Ok(m) = self.probed.get(p)? {
                out.push(m.clone());
            }
        }
        Some(out)
    }

    /// Import `paths` once they are probed (reusing the hover probes), and place them at `place` if given.
    pub fn drop(&mut self, paths: Vec<PathBuf>, place: Option<Place>) {
        self.pending.push(Pending { paths, place, slideshow: None });
    }

    /// R18b: Insert › Slideshow: import the pictures `paths` once probed and lay them out from `time` on video track
    /// `track` (the first one when None) as `show` says, as one undo step.
    pub fn slideshow(&mut self, ctx: &egui::Context, paths: Vec<PathBuf>, track: Option<String>, time: f64, show: edit::Slideshow, pool: &[MediaItem]) {
        self.hover(ctx, &paths, pool);
        self.pending.push(Pending { paths, place: Some((track, time, edit::DropMode::AcrossTime)), slideshow: Some(show) });
    }

    /// New/Open: drops still waiting are cancelled, and probes still running are ignored.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Take finished probes, then place the first drop whose files are all known: new files are imported (a file
    /// already in the pool is reused) and placed as the drop menu chose (crossfading when `xfade`), as one undo step.
    pub fn poll(&mut self, project: &mut Project, still_len: f64, xfade: bool) -> Option<Placed> {
        while let Some(batch) = self.importer.poll() {
            self.probed.extend(batch);
        }
        let known = |p: &PathBuf| in_pool(&project.media_pool, p) || self.probed.contains_key(p);
        let k = self.pending.iter().position(|d| d.paths.iter().all(known))?;
        let d = self.pending.remove(k);
        let before = project.clone();
        let usable = |p: &PathBuf| in_pool(&project.media_pool, p);
        let errors: Vec<String> = d.paths.iter().filter(|p| !usable(p)).filter_map(|p| self.probed.get(p)?.as_ref().err().cloned()).collect();
        let new: Vec<MediaItem> = d.paths.iter().filter(|p| !usable(p)).filter_map(|p| self.probed.get(p)?.as_ref().ok().cloned()).collect();
        let added = merge_new(&mut project.media_pool, new);
        if added {
            project.mark_modified();
        }
        let idxs: Vec<usize> = d.paths.iter().filter_map(|p| project.media_pool.iter().position(|m| m.path == *p && m.subclip.is_none())).collect();
        let (ids, refused) = match &d.place {
            Some((track, time, _)) if d.slideshow.is_some() => {
                let target = track.as_deref().and_then(|id| project.find_track(id)).filter(|t| t.0).map(|t| t.1);
                let ids = d.slideshow.as_ref().map_or(Vec::new(), |s| edit::insert_slideshow(project, &idxs, target, *time, s));
                let refused = ids.is_empty() && project.timeline.video_tracks.get(target.unwrap_or(0)).is_some_and(|t| t.locked);
                (ids, refused)
            }
            Some((track, time, mode)) => {
                // A track deleted while probing: new tracks, as below the last track.
                let target = track.as_deref().and_then(|id| project.find_track(id));
                let ids = edit::drop_media_as(project, &idxs, None, target, *time, still_len, xfade, *mode);
                // Readable media always makes or changes an event unless a locked track refused it.
                let refused = ids.is_empty() && !idxs.is_empty();
                (ids, refused)
            }
            None => (Vec::new(), false),
        };

        Some(Placed { before: (added || !ids.is_empty()).then_some(before), ids, errors, refused })
    }
}

/// The pointer in window points. egui gets no pointer events while Windows drags files over the window, so it is
/// read from the OS.
pub fn os_pointer(ctx: &egui::Context) -> Option<egui::Pos2> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::POINT;
        use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let mut p = POINT { x: 0, y: 0 };
        // SAFETY: GetCursorPos only writes the POINT it is given.
        if unsafe { GetCursorPos(&mut p) } != 0 {
            // Both in physical pixels over pixels per point, so this holds at any display scale.
            let (ppp, inner) = ctx.input(|i| (i.pixels_per_point, i.viewport().inner_rect));
            return inner.map(|r| egui::pos2(p.x as f32 / ppp, p.y as f32 / ppp) - r.min.to_vec2());
        }
    }
    ctx.input(|i| i.pointer.latest_pos())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::MediaType;

    fn probed(path: &str) -> (PathBuf, Result<MediaItem, String>) {
        let m = MediaItem { id: path.into(), path: path.into(), name: path.into(), media_type: MediaType::Audio, duration: 4.0, sample_rate: Some(48_000), ..Default::default() };
        (path.into(), Ok(m))
    }

    #[test]
    fn a_probe_lands_at_the_captured_time_and_track() {
        let mut p = Project::new("t");
        p.timeline.add_audio_track();
        let a2 = p.timeline.audio_tracks[1].id.clone();
        let mut fd = FileDrop::new();
        fd.drop(vec!["a.wav".into(), "b.wav".into(), "bad.mp4".into()], Some((Some(a2.clone()), 3.0, edit::DropMode::AcrossTime)));
        // A track inserted above while probing: the drop follows the track, not its old index.
        crate::edit::insert_track(&mut p, false, Some(0));
        assert!(fd.poll(&mut p, 5.0, false).is_none(), "nothing arrived yet");
        let tx = fd.importer.sender();
        tx.send(vec![probed("a.wav"), probed("b.wav")]).unwrap();
        assert!(fd.poll(&mut p, 5.0, false).is_none(), "bad.mp4 is still probing");
        tx.send(vec![("bad.mp4".into(), Err("bad.mp4: unreadable media".into()))]).unwrap();
        let placed = fd.poll(&mut p, 5.0, false).expect("placed");
        assert!(placed.before.is_some() && placed.ids.len() == 2 && p.modified);
        assert_eq!(placed.errors, ["bad.mp4: unreadable media"]);
        assert_eq!(p.find_track(&a2), Some((false, 2)));
        let clips: Vec<(f64, f64)> = p.timeline.audio_tracks[2].clips.iter().map(|c| (c.position, c.duration)).collect();
        assert_eq!(clips, [(3.0, 4.0), (7.0, 4.0)]);
        assert_eq!(p.media_pool.len(), 2);
        // Dropped again: the pool items are reused, nothing is imported twice.
        fd.drop(vec!["a.wav".into()], Some((None, 0.0, edit::DropMode::AcrossTime)));
        let placed = fd.poll(&mut p, 5.0, false).expect("placed at once");
        assert_eq!((placed.ids.len(), p.media_pool.len()), (1, 2));
        // Add as Takes (the drop menu) onto the first event: it gets b as its active take.
        let first = p.timeline.audio_tracks[2].clips[0].id.clone();
        fd.drop(vec!["b.wav".into()], Some((Some(a2.clone()), 3.5, edit::DropMode::AsTakes)));
        let placed = fd.poll(&mut p, 5.0, false).expect("placed");
        assert_eq!((placed.ids, placed.refused), (vec![first.clone()], false));
        assert_eq!(p.clip(&first).map(|c| (c.media_id.clone(), c.takes.len())), Some((Some("b.wav".into()), 1)));
    }


    #[test]
    fn an_import_only_drop_reuses_the_probe_and_a_locked_track_is_reported() {
        let mut p = Project::new("t");
        let mut fd = FileDrop::new();
        fd.drop(vec!["c.wav".into()], None); // dropped on Project Media
        fd.importer.sender().send(vec![probed("c.wav")]).unwrap();
        let placed = fd.poll(&mut p, 5.0, false).expect("imported");
        assert!(placed.before.is_some() && placed.ids.is_empty() && !placed.refused);
        assert!(p.media_pool.len() == 1 && p.timeline.audio_tracks[0].clips.is_empty());
        // The pointed row is free, but the audio lands on the locked Audio 1.
        p.timeline.audio_tracks[0].locked = true;
        let v1 = p.timeline.video_tracks[0].id.clone();
        fd.drop(vec!["c.wav".into()], Some((Some(v1), 0.0, edit::DropMode::AcrossTime)));
        let placed = fd.poll(&mut p, 5.0, false).expect("handled");
        assert!(placed.refused && placed.before.is_none());
    }

    /// A VEGAS import's offline item is probed again on a drop and filled in place (events keep its id).
    #[test]
    fn an_offline_pool_item_is_probed_again() {
        let mut p = Project::new("t");
        p.media_pool.push(MediaItem { id: "old".into(), path: "a.wav".into(), name: "a.wav".into(), media_type: MediaType::Video, ..Default::default() });
        let mut fd = FileDrop::new();
        fd.drop(vec!["a.wav".into()], Some((None, 0.0, edit::DropMode::AcrossTime)));
        assert!(fd.poll(&mut p, 5.0, false).is_none(), "waits for the probe");
        fd.importer.sender().send(vec![probed("a.wav")]).unwrap();
        let placed = fd.poll(&mut p, 5.0, false).expect("placed");
        assert_eq!((p.media_pool.len(), p.media_pool[0].id.as_str(), p.media_pool[0].sample_rate), (1, "old", Some(48_000)));
        assert_eq!(p.clip(&placed.ids[0]).map(|c| c.duration), Some(4.0));
    }

    #[test]
    fn a_probe_arriving_after_new_is_discarded() {
        let mut p = Project::new("t");
        let mut fd = FileDrop::new();
        let old = fd.importer.sender();
        fd.drop(vec!["a.wav".into()], Some((None, 1.0, edit::DropMode::AcrossTime)));
        fd.reset(); // File › New
        assert!(old.send(vec![probed("a.wav")]).is_err(), "the old batch has nowhere to go");
        fd.importer.sender().send(vec![probed("a.wav")]).unwrap(); // even a result that does arrive places nothing
        assert!(fd.poll(&mut p, 5.0, false).is_none());
        assert!(p.media_pool.is_empty() && p.timeline.audio_tracks[0].clips.is_empty() && !p.modified);
    }
}
