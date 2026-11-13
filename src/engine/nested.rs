//! R6a: nested timelines. A Milan Pro project (`.mnle`) used as media in another, as VEGAS uses a `.veg`: its picture is
//! its own timeline composed by the engine (transparent where nothing covers it, as a nested VEGAS project is) and its
//! sound is its master bus output (VEGAS renders that to an audio proxy; Milan Pro mixes it live, so there is nothing to
//! rebuild and it never goes stale). A child is loaded and compiled once per version of every project file under it
//! (checked by modified time), at most `MAX_DEPTH` deep, and a project never contains itself: a nested timeline that
//! refers to a project it is already inside is offline.
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::SystemTime;
use crate::engine::plan::{self, AudioPlan, VideoPlan};
use crate::project::{MediaItem, MediaType, Project, Track};

/// Why a nested timeline inside itself (directly or through others) is offline.
pub const SELF_REF: &str = "nested timeline refers to itself";
/// Nested timelines more than this many levels below the project are offline (VEGAS documents no limit; real projects
/// nest two or three levels).
pub const MAX_DEPTH: usize = 8;

/// A nested timeline's sound as the mixer reads it: its audio plan (or why it is offline), where its content ends and
/// its master gain (applied after its master FX, as its own render does).
pub struct NestedMix {
    pub plan: Result<AudioPlan, String>,
    pub end: f64,
    pub gain: [f32; 2],
}

/// One compile is one sound: a stream of an older version of the child is reopened, never continued.
impl PartialEq for NestedMix {
    fn eq(&self, o: &Self) -> bool {
        std::ptr::eq(self, o)
    }
}

impl NestedMix {
    /// R6b: one of its own events plays across its time `t` (not starting or ending there): an event of it cut there
    /// needs the parent's quickfade.
    pub fn sounds_across(&self, t: f64) -> bool {
        const EPS: f64 = 1e-6;
        self.plan.as_ref().is_ok_and(|p| p.tracks.iter().flat_map(|tr| &tr.events).any(|e| e.src.pos + EPS < t && t < e.src.pos + e.src.dur - EPS))
    }
}

impl std::fmt::Debug for NestedMix {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "NestedMix({:p}, end {})", self, self.end)
    }
}

/// A loaded and compiled child project.
pub struct Child {
    pub video: Arc<VideoPlan>,
    pub audio: Arc<NestedMix>,
    /// What it is as media now: length, frame size and rate, and whether it has sound.
    media: MediaItem,
    /// Every project file it was built from (itself and its own nested timelines, all the way down), with the modified
    /// time each had.
    deps: Vec<(PathBuf, Option<SystemTime>)>,
}

/// (child file, the files of the projects it was compiled inside): a child inside another stack may differ (the
/// project that would contain itself is offline there).
type Key = (PathBuf, Vec<PathBuf>);
static CACHE: LazyLock<Mutex<HashMap<Key, Arc<Child>>>> = LazyLock::new(Default::default);
/// Bumped by `reload`, so every plan is compiled again.
static GENERATION: AtomicU64 = AtomicU64::new(0);

fn cache() -> std::sync::MutexGuard<'static, HashMap<Key, Arc<Child>>> {
    CACHE.lock().unwrap_or_else(|e| e.into_inner())
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// One name per file (case and `..` resolved), so a project is recognised however it is referred to.
pub fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).or_else(|_| std::path::absolute(p)).unwrap_or_else(|_| p.to_path_buf())
}

/// The projects a top-level compile of `p` is inside: its own file, when it has one.
pub fn stack_of(p: &Project) -> Vec<PathBuf> {
    p.path.iter().map(|p| canon(p)).collect()
}

/// The child project at `path`, compiled inside the projects of `stack` (outermost first), from the cache while none of
/// its files changed. Err is why it is offline.
pub fn load(path: &Path, stack: &[PathBuf]) -> Result<Arc<Child>, String> {
    let path = canon(path);
    if stack.contains(&path) {
        return Err(SELF_REF.into());
    }
    if stack.len() > MAX_DEPTH {
        return Err(format!("nested more than {MAX_DEPTH} levels deep"));
    }
    let key = (path.clone(), stack.to_vec());
    if let Some(c) = cache().get(&key).filter(|c| c.deps.iter().all(|(p, t)| mtime(p) == *t)) {
        return Ok(c.clone());
    }
    // The time is read first: a save while loading leaves it older than the file, so the next check reloads.
    let t = mtime(&path);
    let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let p = Project::load(&path).map_err(|e| format!("{name}: {e}"))?;
    let mut inside = stack.to_vec();
    inside.push(path.clone());
    let mut video = plan::compile_in(&p, &inside);
    video.alpha = true;
    let end = crate::render::content_end(&p);
    // shortcut: a nested timeline always quickfades its bare event edges (Options › Quickfade Audio Edits' default) and
    // plays its FX under Bypass All Audio FX; hand the parent plan's options down if anyone turns those off and minds.
    let audio = NestedMix { plan: Ok(AudioPlan { quickfade: plan::QUICKFADE, ..plan::compile_audio_in(&p, &inside) }), end, gain: p.timeline.master.gain };
    let mut deps = vec![(path.clone(), t)];
    for m in p.media_pool.iter().filter(|m| m.nested) {
        deps.push((m.path.clone(), mtime(&m.path)));
        // Already compiled (and cached) by `compile_in` above.
        if let Ok(c) = load(&m.path, &inside) {
            deps.extend(c.deps.iter().cloned());
        }
    }
    let has = |tracks: &[Track]| tracks.iter().any(|t| t.clips.iter().any(|c| c.duration > 0.0));
    let (pictures, sound) = (has(&p.timeline.video_tracks), has(&p.timeline.audio_tracks));
    let media = MediaItem {
        media_type: if pictures || !sound { MediaType::Video } else { MediaType::Audio },
        duration: end,
        width: Some(video.w),
        height: Some(video.h),
        frame_rate: Some(p.settings.frame_rate),
        sample_rate: sound.then_some(p.settings.sample_rate),
        has_alpha: true,
        nested: true,
        ..Default::default()
    };
    let child = Arc::new(Child { video: Arc::new(video), audio: Arc::new(audio), media, deps });
    let mut c = cache();
    // shortcut: the whole cache goes past 256 entries (each holds two plans); keep the most recent if it ever matters.
    if c.len() >= 256 {
        c.clear();
    }
    c.insert(key, child.clone());
    Ok(child)
}

/// Media item `m` (a nested timeline) as child `c` is now: its length, size, rate and sound follow the child's edits.
pub fn as_media(m: &MediaItem, c: &Child) -> MediaItem {
    let k = &c.media;
    MediaItem {
        media_type: k.media_type,
        // A subclip keeps its own stretch.
        duration: if m.subclip.is_some() { m.duration } else { k.duration },
        width: k.width,
        height: k.height,
        frame_rate: k.frame_rate,
        sample_rate: k.sample_rate,
        has_alpha: true,
        ..m.clone()
    }
}

/// The nested timelines of `p` (compiled inside `stack`) by media id: the item as its child is now, and the child or
/// why it is offline. Empty, and free, for a project without any.
pub fn children<'a>(p: &'a Project, stack: &[PathBuf]) -> HashMap<&'a str, (MediaItem, Result<Arc<Child>, String>)> {
    p.media_pool
        .iter()
        .filter(|m| m.nested)
        .map(|m| {
            let c = load(&m.path, stack);
            let item = c.as_ref().map_or_else(|_| m.clone(), |c| as_media(m, c));
            (m.id.as_str(), (item, c))
        })
        .collect()
}

/// Import: a project file as a nested timeline (a media item named after its file).
pub fn probe(path: &Path) -> anyhow::Result<MediaItem> {
    let c = load(path, &[]).map_err(anyhow::Error::msg)?;
    anyhow::ensure!(c.media.duration > 0.0, "the project has no events");
    let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
    Ok(MediaItem { id: uuid::Uuid::new_v4().to_string(), path: path.to_path_buf(), name, ..c.media.clone() })
}

/// What `p`'s nested timelines depend on, for the engines' "replan?" check: every project file under them (a child
/// saved elsewhere, or in Milan Pro, is seen within a frame). 0 without nested timelines.
pub fn stamp(p: &Project) -> u64 {
    let nested: Vec<&MediaItem> = p.media_pool.iter().filter(|m| m.nested).collect();
    if nested.is_empty() {
        return 0;
    }
    let one = |x: &dyn Fn(&mut DefaultHasher)| {
        let mut h = DefaultHasher::new();
        x(&mut h);
        h.finish()
    };
    let mut s = one(&|h| GENERATION.load(Relaxed).hash(h));
    for m in nested {
        s = s.wrapping_add(one(&|h| (&m.path, mtime(&m.path)).hash(h)));
    }
    // Summed, so the cache's order does not count.
    for c in cache().values() {
        for (d, _) in &c.deps {
            s = s.wrapping_add(one(&|h| (d, mtime(d)).hash(h)));
        }
    }
    s
}

/// R6b: nested timeline `path`'s picture at its time `t`, `w` x `h`, composed by the engine (never ffmpeg on the
/// project file): event thumbnails and the Trimmer. Transparent where it shows nothing.
pub fn frame(path: &Path, t: f64, w: u32, h: u32, q: crate::decode::Quality) -> Result<eframe::egui::ColorImage, String> {
    let c = load(path, &[])?;
    let mut wk = crate::engine::worker::Worker::new();
    wk.set_plan(c.video.clone());
    // A missing file inside it leaves the rest drawn, as the timeline preview does.
    let img = wk.frame(plan::frame_at(t.max(0.0), c.video.fps), w, h, q, None).0;
    Ok(Arc::try_unwrap(img).unwrap_or_else(|a| (*a).clone()))
}

/// R6b: the waveform of nested timeline `path`: peaks of its master bus mix (`mix::mix_nested`, never ffmpeg on the
/// project file), one per 1/`per_sec` s, left and right. Empty when it is offline.
pub fn peaks(path: &Path, per_sec: f64) -> Vec<Vec<f32>> {
    let Ok(c) = load(path, &[]) else { return Vec::new() };
    let per = ((crate::audio::RATE as f64 / per_sec) as usize).max(1);
    let (mut out, mut peak, mut n) = (vec![Vec::new(); 2], [0.0f32; 2], 0);
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    crate::engine::mix::mix_nested(&c.audio, 0.0, c.audio.end, &stop, |pcm| {
        for s in pcm.chunks_exact(2) {
            peak = [peak[0].max(s[0].abs().min(1.0)), peak[1].max(s[1].abs().min(1.0))];
            n += 1;
            if n == per {
                (0..2).for_each(|k| out[k].push(peak[k]));
                (peak, n) = ([0.0; 2], 0);
            }
        }
        true
    });
    if n > 0 {
        (0..2).for_each(|k| out[k].push(peak[k]));
    }
    out
}

/// R6b: the modified time of nested timeline `path`'s file, so a picture or waveform of it made before a save is made
/// again. shortcut: only its own file; a timeline nested inside it saved since shows on its next save or a restart.
pub fn version(path: &Path) -> Option<SystemTime> {
    mtime(path)
}

/// Tools › Rebuild Offline Nested Audio: every nested timeline is read from disk again and every plan recompiled.
pub fn reload() {
    cache().clear();
    GENERATION.fetch_add(1, Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::Quality;
    use crate::engine::plan::{FramePlan, Item, Nested};
    use crate::engine::plugins::{find, new_fx};
    use crate::engine::worker::Worker;
    use crate::project::{Clip, Curve, Keyframe, Keyframes, Motion, TrackKind};
    use eframe::egui::{Color32, ColorImage};

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("milan-nested-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Saved, and dated `later` seconds ahead, so the change is seen whatever the file system's time resolution.
    fn save(p: &mut Project, path: &Path, later: u64) {
        p.save(&path.to_path_buf()).unwrap();
        std::fs::File::options().write(true).open(path).unwrap().set_modified(SystemTime::now() + std::time::Duration::from_secs(later)).unwrap();
    }

    fn solid(color: &str) -> MediaItem {
        let s = find("milan.gen.solid").unwrap();
        let fx = new_fx(s, s.presets.iter().find(|p| p.name == color));
        MediaItem { id: format!("solid-{color}"), name: color.into(), media_type: MediaType::Image, duration: 86400.0, generator: Some(fx), ..Default::default() }
    }

    /// A 320x180, 30 fps project showing `m` for `len` seconds from 0.
    fn project(m: MediaItem, len: f64) -> Project {
        let mut p = Project::new("p");
        (p.settings.width, p.settings.height, p.settings.frame_rate) = (320, 180, 30.0);
        let mut c = Clip::from_media(&m, 0.0);
        c.duration = len;
        p.timeline.video_tracks[0].clips.push(c);
        p.media_pool.push(m);
        p
    }

    /// The project at `child` as a nested timeline at `at` seconds, on a new top video track.
    fn nest(p: &mut Project, child: &Path, at: f64) {
        let m = crate::media::probe_media(child).unwrap();
        assert!(m.nested && m.has_alpha);
        p.timeline.video_tracks.insert(0, Track::new("Nested", TrackKind::Video));
        p.timeline.video_tracks[0].clips.push(Clip::from_media(&m, at));
        p.media_pool.push(m);
    }

    fn frame(p: &Project, t: f64) -> (Arc<ColorImage>, Option<String>) {
        let mut wk = Worker::new();
        wk.set_plan(Arc::new(plan::compile(p)));
        wk.frame(plan::frame_at(t, 30.0), 64, 36, Quality::Good, None)
    }

    /// Why each offline nested timeline in `fp` (at any depth) is offline.
    fn offline(fp: &FramePlan) -> Vec<String> {
        let mut v = Vec::new();
        for i in fp.layers.iter().flat_map(|l| &l.items) {
            let evs: Vec<_> = match i {
                Item::Event(e) => vec![e],
                Item::Mix { a, b, .. } => a.iter().chain(b).collect(),
            };
            for e in evs {
                match e.src.nested.as_deref() {
                    Some(Nested::Offline(why)) => v.push(why.to_string()),
                    Some(Nested::Frame(fs)) => fs.iter().for_each(|f| v.extend(offline(&f.0))),
                    None => {}
                }
            }
        }
        v
    }

    fn offline_at(path: &Path) -> Vec<String> {
        offline(&plan::frame_plan(&plan::compile(&Project::load(&path.to_path_buf()).unwrap()), 30))
    }

    /// A parent with a child holding a red solid renders red; where the child shows nothing it is transparent, as a
    /// nested VEGAS project is, so the parent's track below shows through.
    #[test]
    fn a_nested_red_solid_renders_red() {
        let d = dir("red");
        let path = d.join("child.mnle");
        let mut child = project(solid("Red"), 4.0);
        save(&mut child, &path, 0);
        let mut parent = project(solid("Blue"), 10.0);
        nest(&mut parent, &path, 2.0);
        assert_eq!(parent.timeline.video_tracks[0].clips[0].duration, 4.0, "the event is the child's length");
        let (img, err) = frame(&parent, 3.0);
        assert!(err.is_none() && img.pixels.iter().all(|p| *p == Color32::RED), "{err:?} {:?}", img.pixels[0]);
        assert!(frame(&parent, 1.0).0.pixels.iter().all(|p| *p == Color32::BLUE), "before the nested event");
        child.timeline.video_tracks[0].motion = Keyframes { keys: vec![Keyframe { t: 0.0, v: Motion { w: 0.5, h: 0.5, ..Default::default() }, curve: Curve::Linear }] };
        save(&mut child, &path, 10);
        let img = frame(&parent, 3.0).0;
        assert_eq!((img.pixels[18 * 64 + 32], img.pixels[0]), (Color32::RED, Color32::BLUE));
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// A→B→A: the inner A is offline, with the reason, and the rest draws; a project nesting its own file, and nesting
    /// more than MAX_DEPTH levels deep, are offline too. Nothing recurses for ever.
    #[test]
    fn a_project_never_contains_itself() {
        let d = dir("cycle");
        let (a, b) = (d.join("a.mnle"), d.join("b.mnle"));
        save(&mut project(solid("Red"), 4.0), &a, 0);
        let mut pb = project(solid("Green"), 4.0);
        nest(&mut pb, &a, 0.0);
        save(&mut pb, &b, 0);
        let mut pa = Project::load(&a).unwrap();
        nest(&mut pa, &b, 0.0);
        save(&mut pa, &a, 10);
        assert_eq!(offline_at(&a), [SELF_REF]);
        let pa = Project::load(&a).unwrap();
        let (img, err) = frame(&pa, 1.0);
        assert_eq!(err.as_deref(), Some("a.mnle: nested timeline refers to itself"));
        assert!(img.pixels.iter().all(|p| *p == Color32::GREEN), "B draws, without the A inside it");
        // A project that nests its own file.
        let c = d.join("c.mnle");
        let mut pc = project(solid("Red"), 4.0);
        save(&mut pc, &c, 0);
        nest(&mut pc, &c, 0.0);
        save(&mut pc, &c, 10);
        assert_eq!(offline_at(&c), [SELF_REF]);
        // A chain: d0 (red) inside d1 inside ... d9. Eight levels below the project draw; nine do not.
        let chain: Vec<PathBuf> = (0..=9).map(|i| d.join(format!("d{i}.mnle"))).collect();
        save(&mut project(solid("Red"), 4.0), &chain[0], 0);
        for i in 1..=9 {
            let mut p = project(solid("Black"), 4.0);
            nest(&mut p, &chain[i - 1], 0.0);
            save(&mut p, &chain[i], 0);
        }
        assert!(offline_at(&chain[8]).is_empty());
        assert!(frame(&Project::load(&chain[8]).unwrap(), 1.0).0.pixels.iter().all(|p| *p == Color32::RED));
        assert_eq!(offline_at(&chain[9]), ["nested more than 8 levels deep"]);
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// Editing and saving the child reaches the parent: the engines' change check sees it (they replan) and the new plan
    /// shows the new child; its new length follows too. Rebuild Offline Nested Audio reads every child again.
    #[test]
    fn saving_the_child_reaches_the_parent() {
        let d = dir("save");
        let path = d.join("child.mnle");
        let mut child = project(solid("Red"), 4.0);
        save(&mut child, &path, 0);
        let mut parent = project(solid("Blue"), 10.0);
        nest(&mut parent, &path, 0.0);
        let id = parent.media_pool[1].id.clone();
        let before = load(&path, &stack_of(&parent)).unwrap();
        assert!(Arc::ptr_eq(&before, &load(&path, &stack_of(&parent)).unwrap()), "cached while unchanged");
        assert_eq!(frame(&parent, 1.0).0.pixels[0], Color32::RED);
        let h = crate::engine::content_hash(&parent);
        child.media_pool[0].generator = solid("Green").generator;
        child.timeline.video_tracks[0].clips[0].duration = 6.0;
        save(&mut child, &path, 10);
        assert_ne!(crate::engine::content_hash(&parent), h, "the engines replan");
        assert!(!Arc::ptr_eq(&before, &load(&path, &stack_of(&parent)).unwrap()));
        assert_eq!(frame(&parent, 1.0).0.pixels[0], Color32::GREEN);
        assert_eq!(children(&parent, &stack_of(&parent))[id.as_str()].0.duration, 6.0);
        let h = crate::engine::content_hash(&parent);
        reload();
        assert_ne!(crate::engine::content_hash(&parent), h);
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// `p`'s frame at `t`, full size (320x180), from a fresh engine.
    fn full(p: &Project, t: f64) -> Arc<ColorImage> {
        let mut wk = Worker::new();
        wk.set_plan(Arc::new(plan::compile(p)));
        wk.frame(plan::frame_at(t, 30.0), 320, 180, Quality::Good, None).0
    }

    fn event(p: &mut Project, track: usize, m: &MediaItem, at: f64, len: f64) -> String {
        let mut c = Clip::from_media(m, at);
        c.duration = len;
        let id = c.id.clone();
        p.timeline.video_tracks[track].clips.push(c);
        id
    }

    fn json(p: &Project) -> String {
        serde_json::to_string(p).unwrap()
    }

    /// R6b: Create Nested Timeline keeps the picture. Events on one track with Track Motion: the nested event goes on
    /// that track, whose copy in the child is plain. Events on two tracks: a new plain track above them, the copies
    /// keeping their own motion. Each is one undo step; a file that cannot be written, or the project itself, leaves
    /// the project as it was.
    #[test]
    fn create_nested_keeps_the_picture_and_undoes() {
        let d = dir("create");
        let half = |x: f32| Keyframes { keys: vec![Keyframe { t: 0.0, v: Motion { w: 0.5, h: 0.5, x, ..Default::default() }, curve: Curve::Linear }] };
        let mut p = project(solid("Blue"), 10.0);
        p.timeline.video_tracks.insert(0, Track::new("Top", TrackKind::Video));
        p.timeline.video_tracks.insert(1, Track::new("Mid", TrackKind::Video));
        (p.timeline.video_tracks[0].motion, p.timeline.video_tracks[1].motion) = (half(-40.0), half(60.0));
        let (red, green, white) = (solid("Red"), solid("Green"), solid("White"));
        let ids = vec![event(&mut p, 0, &red, 1.0, 2.0), event(&mut p, 0, &green, 4.0, 2.0)];
        let mid = event(&mut p, 1, &white, 2.0, 3.0);
        p.media_pool.extend([red, green, white]);
        let times = [0.5, 1.5, 2.5, 3.5, 4.5, 5.5, 6.5];
        let before: Vec<_> = times.iter().map(|&t| full(&p, t)).collect();
        let same = |p: &Project| times.iter().zip(&before).all(|(&t, b)| full(p, t).pixels == b.pixels);

        // A file that cannot be written, and the project's own file: nothing changes.
        let (snapshot, mut h) = (p.clone(), crate::history::History::default());
        assert!(crate::edit::create_nested(&mut p, &ids, &d.join("no such folder").join("c.mnle"), &[]).is_err());
        assert!(crate::edit::create_nested(&mut p, &ids, &d.join("parent.mnle"), &[d.join("parent.mnle")]).unwrap_err().ends_with(SELF_REF));
        assert_eq!(json(&p), json(&snapshot));

        // One track: the nested event replaces the two events on it, over their span, its copy plain.
        let one = d.join("one.mnle");
        let new = crate::edit::create_nested(&mut p, &ids, &one, &[]).unwrap();
        h.push(snapshot.clone());
        let top = &p.timeline.video_tracks[0];
        assert_eq!((p.timeline.video_tracks.len(), new.len(), top.clips.len()), (3, 1, 1));
        let c = &top.clips[0];
        assert_eq!((c.id.as_str(), c.position, c.in_point, c.duration, c.group.is_none()), (new[0].as_str(), 1.0, 1.0, 5.0, true));
        let child = Project::load(&one).unwrap();
        assert_eq!((child.timeline.video_tracks.len(), child.timeline.video_tracks[0].clips.len(), child.media_pool.len()), (1, 2, 2));
        assert_eq!(child.timeline.video_tracks[0].motion, Keyframes::default(), "its copy is plain: Top's motion acts on the nested event");
        assert!(child.timeline.audio_tracks.is_empty() && p.modified);
        assert!(same(&p), "the picture is the same");
        assert!(h.undo(&mut p) && json(&p) == json(&snapshot), "one undo step restores the events");

        // Two tracks (Top's red and green, Mid's white): a new plain track above; the copies keep their motion.
        let mut both = ids.clone();
        both.push(mid);
        let two = d.join("two.mnle");
        crate::edit::create_nested(&mut p, &both, &two, &[]).unwrap();
        let tl = &p.timeline;
        assert_eq!((tl.video_tracks.len(), tl.video_tracks[0].motion == Keyframes::default()), (4, true));
        assert!(tl.video_tracks[1].clips.is_empty() && tl.video_tracks[2].clips.is_empty());
        let child = Project::load(&two).unwrap();
        assert_eq!(child.timeline.video_tracks.iter().map(|t| t.motion.clone()).collect::<Vec<_>>(), [half(-40.0), half(60.0)]);
        assert!(same(&p), "the picture is the same");
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// R6b: an event's sound plays across a time (it needs the parent's quickfade there) only strictly inside it; the
    /// picture of a nested timeline for thumbnails and the Trimmer is its own frame, composed by the engine.
    #[test]
    fn edges_and_pictures_of_a_nested_timeline() {
        let d = dir("edges");
        let path = d.join("child.mnle");
        let wav = MediaItem { id: "w".into(), path: d.join("w.wav"), media_type: MediaType::Audio, duration: 10.0, sample_rate: Some(48_000), ..Default::default() };
        let mut child = project(solid("Red"), 4.0);
        let mut c = Clip::from_media(&wav, 1.0);
        c.duration = 3.0;
        child.timeline.audio_tracks[0].clips.push(c);
        child.media_pool.push(wav);
        save(&mut child, &path, 0);
        let n = load(&path, &[]).unwrap().audio.clone();
        assert_eq!([0.5, 1.0, 2.0, 4.0, 4.5].map(|t| n.sounds_across(t)), [false, false, true, false, false]);
        let img = super::frame(&path, 1.5, 64, 36, Quality::Good).unwrap();
        assert!(img.size == [64, 36] && img.pixels.iter().all(|p| *p == Color32::RED));
        assert!(super::frame(&d.join("missing.mnle"), 0.0, 64, 36, Quality::Good).is_err());
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// A child with sound only is an audio item whose sound is its master bus (its plan and master gain); recompiling
    /// an unchanged child keeps the one sound, so a playing stream continues, and a saved change is a new one.
    #[test]
    fn nested_sound_is_the_childs_master_bus() {
        let d = dir("sound");
        let path = d.join("child.mnle");
        let wav = MediaItem { id: "w".into(), path: d.join("w.wav"), media_type: MediaType::Audio, duration: 10.0, sample_rate: Some(48_000), ..Default::default() };
        let mut child = Project::new("c");
        let mut c = Clip::from_media(&wav, 1.0);
        c.duration = 3.0;
        child.timeline.audio_tracks[0].clips.push(c);
        child.media_pool.push(wav);
        child.timeline.master.gain = [0.5, 0.5];
        save(&mut child, &path, 0);
        let m = crate::media::probe_media(&path).unwrap();
        assert_eq!((m.media_type, m.sample_rate, m.duration), (MediaType::Audio, Some(48_000), 4.0));
        let mut parent = Project::new("p");
        parent.timeline.audio_tracks[0].clips.push(Clip::from_media(&m, 2.0));
        parent.media_pool.push(m);
        let sound = |p: &Project| plan::compile_audio(p).tracks[0].events[0].src.nested.clone().unwrap();
        let s = sound(&parent);
        assert!(Arc::ptr_eq(&s, &sound(&parent)));
        assert_eq!((s.gain, s.end), ([0.5, 0.5], 4.0));
        assert_eq!(s.plan.as_ref().unwrap().tracks[0].events[0].src.path, d.join("w.wav"));
        assert!(plan::frame_plan(&plan::compile(&parent), 90).layers.is_empty(), "no picture");
        child.timeline.master.gain = [1.0, 1.0];
        save(&mut child, &path, 10);
        assert!(!Arc::ptr_eq(&s, &sound(&parent)) && sound(&parent).gain == [1.0, 1.0]);
        // Offline, the event says why when it is played.
        std::fs::remove_file(&path).unwrap();
        assert!(sound(&parent).plan.as_ref().is_err_and(|e| e.starts_with("child.mnle")));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
