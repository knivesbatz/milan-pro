//! R30: File › Export › Milan Pro Project Archive and Import › Media from Project.
//!
//! An archive is a folder holding a copy of the project and every media file it uses, under `Media\`, with the
//! project saved so its media paths are relative (`Project::save` writes `rel_path`): the folder opens from another
//! drive letter or machine with nothing offline. Optionally each file is trimmed to the stretch the timeline uses
//! plus handles on either side (ffmpeg: audio files by stream copy, video re-encoded so the cut is frame-accurate),
//! and the events' in points move with it so the timeline plays the same.
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use crate::project::{MediaItem, MediaType, Project};

#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// Trim media to the used ranges plus `handles` seconds on either side.
    pub trim: bool,
    pub handles: f64,
}

impl Default for Options {
    fn default() -> Self {
        Self { trim: false, handles: 2.0 }
    }
}

/// One file the archive copies: the pool items sharing it, its size, and how much of it the timeline uses.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub path: PathBuf,
    pub name: String,
    /// Bytes; 0 when offline.
    pub size: u64,
    pub online: bool,
    /// Seconds of the file the events and takes play (None: nothing plays it, or it is used whole).
    pub used: Option<(f64, f64)>,
    /// A video or audio file whose used range is a proper part of it, with no subclip cut from it and not a nested
    /// project: trimming is possible.
    pub trimmable: bool,
    pub duration: f64,
}

/// What the archive will hold: one row per distinct media file (generated media needs no file).
pub fn plan(p: &Project) -> Vec<Item> {
    let mut items: Vec<Item> = Vec::new();
    let mut subclip_parents: HashSet<PathBuf> = HashSet::new();
    for m in p.media_pool.iter().filter(|m| m.subclip.is_some()) {
        subclip_parents.insert(m.path.clone());
    }
    let tl = &p.timeline;
    for m in &p.media_pool {
        if m.path.as_os_str().is_empty() || m.generator.is_some() {
            continue;
        }
        if let Some(it) = items.iter_mut().find(|i| i.path == m.path) {
            it.duration = it.duration.max(m.file_time(m.duration));
            extend(it, p, m);
            continue;
        }
        let meta = std::fs::metadata(&m.path).ok();
        let mut it = Item {
            path: m.path.clone(),
            name: m.path.file_name().map_or(m.name.clone(), |n| n.to_string_lossy().into_owned()),
            size: meta.as_ref().map_or(0, |md| md.len()),
            online: meta.as_ref().is_some_and(|md| md.is_file()),
            used: None,
            trimmable: false,
            duration: m.file_time(m.duration),
        };
        extend(&mut it, p, m);
        it.trimmable = it.online && matches!(m.media_type, MediaType::Video | MediaType::Audio) && !m.nested && !subclip_parents.contains(&m.path);
        items.push(it);
    }
    let _ = tl;
    items
}

/// Widen `it.used` by every event and take of pool item `m`; a looping event that runs past the file's end uses
/// all of it.
fn extend(it: &mut Item, p: &Project, m: &MediaItem) {
    let tl = &p.timeline;
    let mut whole = false;
    let mut add = |a: f64, b: f64| {
        let (a, b) = (a.max(0.0), b.max(a));
        it.used = Some(match it.used {
            Some((x, y)) => (x.min(a), y.max(b)),
            None => (a, b),
        });
    };
    for c in tl.video_tracks.iter().chain(&tl.audio_tracks).flat_map(|t| &t.clips) {
        let span = c.duration * c.rate;
        if c.media_id.as_deref() == Some(&m.id) {
            if c.in_point + span > m.duration + 1e-6 {
                whole = true;
            }
            add(m.file_time(c.in_point), m.file_time(c.in_point + span));
        }
        for tk in c.takes.iter().filter(|t| t.media_id == m.id) {
            if tk.in_point + span > m.duration + 1e-6 {
                whole = true;
            }
            add(m.file_time(tk.in_point), m.file_time(tk.in_point + span));
        }
    }
    if whole {
        it.used = Some((0.0, it.duration));
    }
}

/// The trimmed stretch of `it` with `handles` on either side, inside the file; None when nothing would be saved.
pub fn trim_range(it: &Item, handles: f64) -> Option<(f64, f64)> {
    let (a, b) = it.used?;
    let (start, end) = ((a - handles).max(0.0), (b + handles).min(it.duration));
    (it.trimmable && end > start && end - start < it.duration - 0.5).then_some((start, end))
}

/// Bytes the archive will take, roughly: the whole file, or the used share of a trimmed one.
pub fn estimate(items: &[Item], opts: &Options) -> u64 {
    items
        .iter()
        .map(|it| match trim_range(it, opts.handles).filter(|_| opts.trim) {
            Some((a, b)) if it.duration > 0.0 => (it.size as f64 * (b - a) / it.duration) as u64,
            _ => it.size,
        })
        .sum()
}

pub struct Progress {
    pub done: usize,
    pub total: usize,
    pub current: String,
}

#[derive(Debug, Default)]
pub struct Report {
    pub project: PathBuf,
    pub copied: usize,
    pub trimmed: usize,
    /// Offline files, left as they were.
    pub skipped: Vec<String>,
    pub notes: Vec<String>,
}

/// A file name no other archived file has yet.
fn unique_name(dir: &Path, name: &str, taken: &mut HashSet<String>) -> PathBuf {
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    let mut n = 1;
    loop {
        let candidate = if n == 1 { format!("{stem}{ext}") } else { format!("{stem} ({n}){ext}") };
        if !taken.contains(&candidate.to_lowercase()) {
            taken.insert(candidate.to_lowercase());
            return dir.join(candidate);
        }
        n += 1;
    }
}

/// Copy or trim `project`'s media into `dest\Media` and save the project there as `<name>.mnle`. `progress` is told
/// before each file; `cancel` stops between files (what was written stays, no project file is saved).
pub fn run(mut project: Project, dest: &Path, opts: &Options, progress: &dyn Fn(Progress), cancel: &AtomicBool) -> anyhow::Result<Report> {
    let items = plan(&project);
    let media_dir = dest.join("Media");
    std::fs::create_dir_all(&media_dir)?;
    let mut report = Report::default();
    let mut taken: HashSet<String> = HashSet::new();
    // Each source path's new file and, when trimmed, where the trim starts.
    let mut moved: HashMap<PathBuf, (PathBuf, f64)> = HashMap::new();
    let total = items.len();
    for (i, it) in items.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            anyhow::bail!("cancelled");
        }
        progress(Progress { done: i, total, current: it.name.clone() });
        if !it.online {
            report.skipped.push(it.path.display().to_string());
            continue;
        }
        let trim = trim_range(it, opts.handles).filter(|_| opts.trim);
        let pool_item = project.media_pool.iter().find(|m| m.path == it.path);
        let media_type = pool_item.map_or(MediaType::Unknown, |m| m.media_type);
        let out_name = match (trim, media_type) {
            // A re-encoded video goes out as MP4 whatever the source container was.
            (Some(_), MediaType::Video) => format!("{}.mp4", it.path.file_stem().map_or(it.name.clone(), |s| s.to_string_lossy().into_owned())),
            _ => it.name.clone(),
        };
        let out = unique_name(&media_dir, &out_name, &mut taken);
        match trim {
            Some((start, end)) => {
                trim_file(&it.path, &out, start, end - start, media_type == MediaType::Video, pool_item.and_then(|m| m.codec.as_deref()))?;
                report.trimmed += 1;
                moved.insert(it.path.clone(), (out, start));
            }
            None => {
                std::fs::copy(&it.path, &out)?;
                report.copied += 1;
                moved.insert(it.path.clone(), (out, 0.0));
            }
        }
        if project.media_pool.iter().any(|m| m.path == it.path && m.nested) {
            report.notes.push(format!("{}: a nested project is copied as it is; its own media is not archived", it.name));
        }
    }
    progress(Progress { done: total, total, current: String::new() });
    // Point the pool at the copies; a trimmed file's events and takes start `start` seconds earlier in it.
    let mut shifted: HashMap<String, f64> = HashMap::new();
    for m in &mut project.media_pool {
        let Some((out, start)) = moved.get(&m.path) else { continue };
        m.path = out.clone();
        if *start > 0.0 || moved.values().any(|(o, s)| o == out && *s > 0.0) {
            if let Ok(probed) = crate::media::probe_media(out) {
                (m.duration, m.width, m.height, m.codec) = (probed.duration, probed.width, probed.height, probed.codec);
            } else {
                m.duration = (m.duration - start).max(0.0);
            }
            shifted.insert(m.id.clone(), *start);
        }
    }
    let tl = &mut project.timeline;
    for c in tl.video_tracks.iter_mut().chain(&mut tl.audio_tracks).flat_map(|t| &mut t.clips) {
        if let Some(s) = c.media_id.as_deref().and_then(|id| shifted.get(id)) {
            c.in_point = (c.in_point - s).max(0.0);
            c.out_point = (c.out_point - s).max(c.in_point);
        }
        for tk in &mut c.takes {
            if let Some(s) = shifted.get(&tk.media_id) {
                tk.in_point = (tk.in_point - s).max(0.0);
            }
        }
    }
    let name = if project.name.is_empty() { "Project".to_string() } else { project.name.clone() };
    let file = dest.join(format!("{}.mnle", name.replace(['\\', '/', ':', '*', '?', '"', '<', '>', '|'], "_")));
    project.save(&file)?;
    report.project = file;
    Ok(report)
}

/// `len` seconds of `src` from `start` into `out`: PCM audio re-written as the same PCM (lossless and
/// sample-accurate; a stream copy cuts on packets), other audio by stream copy (a packet's accuracy), video
/// re-encoded (H.264, AAC) so the first frame is exactly `start`.
fn trim_file(src: &Path, out: &Path, start: f64, len: f64, video: bool, codec: Option<&str>) -> anyhow::Result<()> {
    let mut cmd = crate::proc::ffmpeg();
    cmd.args(["-y", "-v", "error", "-ss", &format!("{start:.6}"), "-i"]).arg(src).args(["-t", &format!("{len:.6}")]);
    if video {
        cmd.args(["-map", "0:v:0", "-map", "0:a?", "-c:v", "libx264", "-preset", "veryfast", "-crf", "16", "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "256k", "-movflags", "+faststart"]);
    } else {
        match codec.filter(|c| c.starts_with("pcm_")) {
            Some(pcm) => cmd.args(["-c:a", pcm]),
            None => cmd.args(["-c", "copy"]),
        };
    }
    cmd.arg(out);
    let output = crate::proc::output(&mut cmd).map_err(|e| anyhow::anyhow!(crate::proc::tool_error(&e, "ffmpeg")))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("ffmpeg could not trim {}: {}", src.display(), err.lines().last().unwrap_or("unknown error").trim());
    }
    Ok(())
}

/// Import › Media from Project: another project's media pool, as it loads (paths resolved relative to that file).
pub fn media_of(path: &Path) -> anyhow::Result<Vec<MediaItem>> {
    let p = Project::load(&path.to_path_buf())?;
    Ok(p.media_pool.into_iter().filter(|m| m.subclip.is_none()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Clip, Take, Track, TrackKind};

    fn item(name: &str, dur: f64, t: MediaType) -> MediaItem {
        MediaItem { id: name.into(), path: std::env::temp_dir().join(name), name: name.into(), media_type: t, duration: dur, ..Default::default() }
    }

    #[test]
    fn plan_finds_used_ranges_and_dedupes_files() {
        let mut p = Project::new("Plan");
        p.media_pool = vec![item("a.mp4", 60.0, MediaType::Video), item("b.wav", 30.0, MediaType::Audio), item("c.png", 5.0, MediaType::Image)];
        // The same file twice in the pool (a second import) is one archive row.
        p.media_pool.push(MediaItem { id: "a2".into(), ..p.media_pool[0].clone() });
        let mut v = Track::new("V", TrackKind::Video);
        let mut c = Clip::from_media(&p.media_pool[0], 0.0);
        (c.in_point, c.duration, c.rate, c.out_point) = (10.0, 5.0, 2.0, 20.0);
        c.takes.push(Take { media_id: "b.wav".into(), in_point: 3.0, name: "b".into() });
        let mut c2 = Clip::from_media(&p.media_pool[3], 5.0);
        (c2.in_point, c2.duration, c2.out_point) = (30.0, 4.0, 34.0);
        v.clips = vec![c, c2];
        let mut a = Track::new("A", TrackKind::Audio);
        let mut ac = Clip::from_media(&p.media_pool[1], 0.0);
        // Runs past the file's end: it loops, so all of it is used.
        (ac.in_point, ac.duration, ac.out_point) = (20.0, 15.0, 35.0);
        a.clips = vec![ac];
        p.timeline.video_tracks = vec![v];
        p.timeline.audio_tracks = vec![a];
        let items = plan(&p);
        assert_eq!(items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["a.mp4", "b.wav", "c.png"]);
        assert_eq!(items[0].used, Some((10.0, 34.0)));
        assert_eq!(items[1].used, Some((0.0, 30.0)));
        assert_eq!(items[2].used, None);
        // Offline test files: nothing is trimmable, and the whole-file audio would not be anyway.
        assert!(items.iter().all(|i| !i.online && !i.trimmable && i.size == 0));
        let online = Item { online: true, trimmable: true, ..items[0].clone() };
        assert_eq!(trim_range(&online, 2.0), Some((8.0, 36.0)));
        assert_eq!(trim_range(&Item { used: Some((0.0, 59.8)), ..online.clone() }, 2.0), None);
        assert_eq!(estimate(&[Item { size: 600, ..online.clone() }], &Options { trim: true, handles: 2.0 }), 280);
        assert_eq!(estimate(&[Item { size: 600, ..online }], &Options::default()), 600);
    }

    #[test]
    fn names_stay_unique() {
        let mut taken = HashSet::new();
        let dir = Path::new("X:\\a");
        assert_eq!(unique_name(dir, "clip.mp4", &mut taken), dir.join("clip.mp4"));
        assert_eq!(unique_name(dir, "Clip.MP4", &mut taken), dir.join("Clip (2).MP4"));
        assert_eq!(unique_name(dir, "clip.mp4", &mut taken), dir.join("clip (3).mp4"));
        assert_eq!(unique_name(dir, "noext", &mut taken), dir.join("noext"));
    }

    /// Needs ffmpeg: two media files archived to a folder with trimming; the archive opens from another drive letter
    /// through its relative paths, and the trimmed media keeps the handles and the events' timing.
    #[test]
    #[ignore]
    fn archive_copies_trims_and_reopens_from_another_drive() {
        let dir = std::env::temp_dir().join(format!("milan-archive-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        let make = |name: &str, args: &[&str]| {
            let out = dir.join("src").join(name);
            let mut cmd = crate::proc::ffmpeg();
            cmd.args(["-v", "error", "-y"]).args(args).arg(&out);
            assert!(crate::proc::output(&mut cmd).unwrap().status.success(), "{name}");
            out
        };
        let a = make("a.mov", &["-f", "lavfi", "-i", "testsrc2=d=4:s=320x240:r=25", "-f", "lavfi", "-i", "sine=f=440:d=4:r=48000", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "pcm_s16le", "-shortest"]);
        let b = make("b.wav", &["-f", "lavfi", "-i", "sine=f=220:d=4:r=48000", "-c:a", "pcm_s16le"]);
        let mut p = Project::new("Show");
        p.media_pool = vec![crate::media::probe_media(&a).unwrap(), crate::media::probe_media(&b).unwrap()];
        let mut v = Track::new("V", TrackKind::Video);
        let mut c = Clip::from_media(&p.media_pool[0], 0.0);
        (c.in_point, c.duration, c.out_point) = (2.0, 1.0, 3.0);
        v.clips = vec![c];
        let mut t = Track::new("A", TrackKind::Audio);
        let mut ac = Clip::from_media(&p.media_pool[1], 0.0);
        (ac.in_point, ac.duration, ac.out_point) = (1.0, 1.0, 2.0);
        t.clips = vec![ac];
        p.timeline.video_tracks = vec![v];
        p.timeline.audio_tracks = vec![t];
        let dest = dir.join("archive");
        let seen = std::sync::Mutex::new(Vec::new());
        let report = run(p.clone(), &dest, &Options { trim: true, handles: 0.5 }, &|pr| seen.lock().unwrap().push((pr.done, pr.total)), &AtomicBool::new(false)).unwrap();
        assert_eq!((report.copied, report.trimmed, report.skipped.len()), (0, 2, 0), "{report:?}");
        assert_eq!(seen.lock().unwrap().last(), Some(&(2, 2)));
        assert!(dest.join("Media").join("a.mp4").is_file() && dest.join("Media").join("b.wav").is_file());
        // The trimmed files hold the used second plus half a second either side.
        let dur = |n: &str| crate::media::probe_media(&dest.join("Media").join(n)).unwrap().duration;
        assert!((dur("a.mp4") - 2.0).abs() < 0.15, "{}", dur("a.mp4"));
        assert!((dur("b.wav") - 2.0).abs() < 0.01, "{}", dur("b.wav"));
        // The saved project points at the copies, relatively, and the in points moved with the trim.
        let text = std::fs::read_to_string(&report.project).unwrap();
        assert!(text.contains(r#""rel_path": "Media\\a.mp4""#) || text.contains(r#""rel_path": "Media/a.mp4""#), "{text}");
        // As if the folder were on another drive: the absolute paths are gone, the relative ones find the files.
        let elsewhere = text.replace(&serde_json::to_string(&dest.join("Media")).unwrap().trim_matches('"').to_string(), r"Z:\\gone\\Media");
        std::fs::write(&report.project, elsewhere).unwrap();
        let q = Project::load(&report.project).unwrap();
        assert!(q.media_pool.iter().all(|m| m.path.starts_with(&dest) && m.path.exists()), "{:?}", q.media_pool.iter().map(|m| &m.path).collect::<Vec<_>>());
        let (vc, ac) = (&q.timeline.video_tracks[0].clips[0], &q.timeline.audio_tracks[0].clips[0]);
        assert!((vc.in_point - 0.5).abs() < 1e-9 && (vc.out_point - 1.5).abs() < 1e-9 && vc.duration == 1.0);
        assert!((ac.in_point - 0.5).abs() < 1e-9 && ac.duration == 1.0);
        assert!((q.media_pool[0].duration - 2.0).abs() < 0.15 && (q.media_pool[1].duration - 2.0).abs() < 0.01);
        // Without trimming, the files are copied whole and nothing moves.
        let dest2 = dir.join("plain");
        let report = run(p, &dest2, &Options::default(), &|_| {}, &AtomicBool::new(false)).unwrap();
        assert_eq!((report.copied, report.trimmed), (2, 0));
        let q = Project::load(&report.project).unwrap();
        assert!(q.timeline.video_tracks[0].clips[0].in_point == 2.0 && q.media_pool[0].path == dest2.join("Media").join("a.mov"));
        // Import › Media from Project lists the archive's media.
        assert_eq!(media_of(&report.project).unwrap().len(), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
