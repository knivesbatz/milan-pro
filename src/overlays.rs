//! Overlay packs (Milan Pro's own; VEGAS has no such window): collections of clips layered over footage (light leaks,
//! grain, dust, flares, frames, keyed elements). The library is a folder (Preferences › General; by default
//! Documents\Milan Pro\Overlay Packs) whose subfolders are packs, plus folders linked where they are. The index
//! (%APPDATA%\MilanPro\overlays.json) keeps the packs, their default blends, favourites, recent items and tags.
//! Scans, probes, copies, zip extraction, the sample pack and the Recycle Bin all run off the UI thread.
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::SystemTime;
use eframe::egui;
use serde::{Deserialize, Serialize};
use crate::engine::plugins;
use crate::media::Importer;
use crate::project::{Composite, MediaItem, MediaType, Project};

/// How an overlay composites over the footage below it: its new track's compositing mode, or the Chroma Keyer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Blend {
    Normal,
    Screen,
    Add,
    Multiply,
    Overlay,
    KeyGreen,
    KeyBlue,
}

impl Blend {
    pub const ALL: [Blend; 7] = [Blend::Normal, Blend::Screen, Blend::Add, Blend::Multiply, Blend::Overlay, Blend::KeyGreen, Blend::KeyBlue];

    pub fn label(self) -> &'static str {
        match self {
            Blend::Normal => "Normal (alpha)",
            Blend::Screen => "Screen",
            Blend::Add => "Add",
            Blend::Multiply => "Multiply",
            Blend::Overlay => "Overlay",
            Blend::KeyGreen => "Key out green",
            Blend::KeyBlue => "Key out blue",
        }
    }

    /// What an overlay gets when nothing was chosen: Normal for a clip with alpha, Screen for packs named like light
    /// leaks, flares, bokeh, dust or grain (bright on black), else Normal.
    pub fn suggest(pack: &str, alpha: bool) -> Blend {
        let n = pack.to_lowercase();
        if !alpha && ["leak", "flare", "bokeh", "dust", "grain"].iter().any(|w| n.contains(w)) { Blend::Screen } else { Blend::Normal }
    }
}

/// Give video track `i` (one an overlay was just placed on) `blend`: its compositing mode, or the Chroma Keyer's
/// Green Screen / Blue Screen preset as a track FX.
pub fn apply_blend(project: &mut Project, i: usize, blend: Blend) {
    let Some(t) = project.timeline.video_tracks.get_mut(i) else { return };
    t.composite = match blend {
        Blend::Screen => Composite::Screen,
        Blend::Add => Composite::Add,
        Blend::Multiply => Composite::Multiply,
        Blend::Overlay => Composite::Overlay,
        Blend::Normal | Blend::KeyGreen | Blend::KeyBlue => Composite::SourceAlpha,
    };
    let key = match blend {
        Blend::KeyGreen => Some("Green Screen"),
        Blend::KeyBlue => Some("Blue Screen"),
        _ => None,
    };
    if let (Some(name), Some(info)) = (key, plugins::find("milan.fx.chroma_key")) {
        t.fx.push(plugins::new_fx(info, info.presets.iter().find(|p| p.name == name)));
    }
    project.mark_modified();
}

/// After a drop: each new video track (one `before` lacks) holding an event of `ids` whose media is in a pack gets
/// that overlay's blend, once. Returns the overlays placed so.
pub fn blend_new_tracks(project: &mut Project, before: &Project, ids: &[String], index: &Index) -> Vec<PathBuf> {
    let (mut used, mut done) = (Vec::new(), HashSet::new());
    for id in ids {
        let Some(r) = project.find_clip(id).filter(|r| r.is_video) else { continue };
        let t = &project.timeline.video_tracks[r.track_idx];
        if before.find_track(&t.id).is_some() || done.contains(&t.id) {
            continue;
        }
        let media = t.clips[r.clip_idx].media_id.as_ref().and_then(|mid| project.media_pool.iter().find(|m| m.id == *mid));
        let Some((path, blend)) = media.and_then(|m| Some((m.path.clone(), index.blend_for(&m.path, m.has_alpha)?))) else { continue };
        done.insert(t.id.clone());
        apply_blend(project, r.track_idx, blend);
        used.push(path);
    }
    used
}

/// Double-click / Add at Cursor: overlay `m` (probed) at `time` on a new video track above video track `above` (the
/// top when None), with `blend`. The caller pushes the project from before as one undo step. The new event ids.
pub fn add_on_new_track(project: &mut Project, m: &MediaItem, above: Option<usize>, time: f64, still_len: f64, blend: Blend) -> Vec<String> {
    crate::media::merge_new(&mut project.media_pool, vec![MediaItem { id: uuid::Uuid::new_v4().to_string(), ..m.clone() }]);
    let Some(idx) = project.media_pool.iter().position(|p| p.path == m.path && p.subclip.is_none()) else { return Vec::new() };
    let t = crate::edit::insert_track(project, true, Some(above.unwrap_or(0)));
    let ids = crate::edit::drop_media_as(project, &[idx], None, Some((true, t)), time, still_len, false, crate::edit::DropMode::AcrossTime);
    apply_blend(project, t, blend);
    ids
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pack {
    pub name: String,
    pub path: PathBuf,
    /// Added with Add Existing Folder as Pack: the folder stays where it is, and Remove Pack only forgets it.
    #[serde(default)]
    pub linked: bool,
    /// The pack's default blend; None = suggested (`Blend::suggest`).
    #[serde(default)]
    pub blend: Option<Blend>,
}

/// overlays.json. Item settings are keyed by the file's path.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Index {
    pub packs: Vec<Pack>,
    pub favorites: Vec<PathBuf>,
    /// Most recent first.
    pub recent: Vec<PathBuf>,
    pub tags: BTreeMap<PathBuf, Vec<String>>,
    /// Per-item blends over the pack's.
    pub blends: BTreeMap<PathBuf, Blend>,
}

const RECENT_MAX: usize = 30;

impl Index {
    pub fn file() -> Option<PathBuf> {
        Some(PathBuf::from(std::env::var_os("APPDATA")?).join("MilanPro").join("overlays.json"))
    }

    /// The index in `file` (empty when there is none yet). A field that does not parse falls back to its default
    /// alone (pack entries one by one); a file that is no JSON object is set aside as overlays.json.bad, so a save
    /// never writes over it. None: the file is there but cannot be read (locked), so it must not be saved over.
    pub fn load(file: &Path) -> Option<Index> {
        use serde_json::Value;
        let text = match std::fs::read_to_string(file) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Some(Index::default()),
            Err(_) => return None,
        };
        let Ok(Value::Object(mut fields)) = serde_json::from_str::<Value>(&text) else {
            let _ = std::fs::rename(file, file.with_extension("json.bad"));
            return Some(Index::default());
        };
        if let Some(Value::Array(packs)) = fields.get_mut("packs") {
            packs.retain(|v| serde_json::from_value::<Pack>(v.clone()).is_ok());
        }
        let mut good = serde_json::Map::new();
        for (k, v) in fields {
            let mut trial = good.clone();
            trial.insert(k.clone(), v.clone());
            if serde_json::from_value::<Index>(Value::Object(trial)).is_ok() {
                good.insert(k, v);
            }
        }
        Some(serde_json::from_value(Value::Object(good)).unwrap_or_default())
    }

    /// Written to a temporary file and renamed, so a crash never leaves half an index.
    pub fn save(&self, file: &Path) -> std::io::Result<()> {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = file.with_extension(format!("{}.tmp", std::process::id()));
        let res = std::fs::write(&tmp, serde_json::to_string_pretty(self)?).and_then(|_| std::fs::rename(&tmp, file));
        if res.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        res
    }

    /// The pack holding `path` (the innermost when packs nest).
    pub fn pack_of(&self, path: &Path) -> Option<&Pack> {
        self.packs.iter().filter(|p| inside(path, &p.path)).max_by_key(|p| p.path.components().count())
    }

    /// The blend `path` gets on a new track: its own, else its pack's, else the suggestion. None outside every pack.
    pub fn blend_for(&self, path: &Path, alpha: bool) -> Option<Blend> {
        let pack = self.pack_of(path)?;
        Some(self.blends.get(path).copied().or(pack.blend).unwrap_or_else(|| Blend::suggest(&pack.name, alpha)))
    }

    pub fn used(&mut self, path: &Path) {
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_path_buf());
        self.recent.truncate(RECENT_MAX);
    }
}

/// `path` is `dir` or inside it (Windows paths: any case, either slash).
pub fn inside(path: &Path, dir: &Path) -> bool {
    let norm = |p: &Path| p.components().map(|c| c.as_os_str().to_string_lossy().to_lowercase()).collect::<Vec<_>>();
    let (p, d) = (norm(path), norm(dir));
    !d.is_empty() && p.len() >= d.len() && p[..d.len()] == d[..]
}

/// Documents\Milan Pro\Overlay Packs.
// shortcut: %USERPROFILE%\Documents, not the known-folder lookup; a Documents folder redirected elsewhere (OneDrive)
// gets the library beside it until the user picks a folder in Preferences.
pub fn default_library() -> PathBuf {
    std::env::var_os("USERPROFILE").map_or_else(std::env::temp_dir, PathBuf::from).join("Documents").join("Milan Pro").join("Overlay Packs")
}

/// A file an overlay pack offers: video or a still (anything FFmpeg reads by those extensions; alpha formats too).
pub fn is_overlay(p: &Path) -> bool {
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    !crate::media::PROJECT_EXTS.contains(&ext.as_str()) && matches!(crate::media::classify_extension(&ext), MediaType::Video | MediaType::Image)
}

// ---- Remove Pack ----

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Removal {
    /// Only taken out of the index; the folder stays.
    Forget,
    /// The folder goes to the Recycle Bin (after a confirm); `used`: the project's media in it, which goes offline.
    Recycle { used: usize },
}

/// What Remove Pack does with `pack`: a linked pack, a missing folder, or a folder not directly in the library `lib`
/// is only forgotten; a pack copied into the library goes to the Recycle Bin. Nothing is ever deleted for good, and
/// the project's references are never touched.
pub fn removal(pack: &Pack, lib: &Path, missing: bool, project: &Project) -> Removal {
    let in_lib = pack.path.parent().is_some_and(|p| inside(p, lib) && inside(lib, p));
    if pack.linked || missing || !in_lib {
        return Removal::Forget;
    }
    Removal::Recycle { used: project.media_pool.iter().filter(|m| inside(&m.path, &pack.path)).count() }
}

/// Move `path` (a whole folder) to the Recycle Bin. Where Windows cannot recycle it (some network drives), it asks
/// before deleting for good, and No keeps the folder.
#[cfg(windows)]
pub fn recycle(path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::*;
    if !path.is_absolute() {
        return Err(format!("{} is not a full path", path.display()));
    }
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
    // The shell's file operations want a single-threaded apartment on the calling (job) thread.
    // SAFETY: paired with CoUninitialize below when it succeeded.
    let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
    // A list of paths, each ending in a NUL, the list in another.
    let from: Vec<u16> = path.as_os_str().encode_wide().chain([0, 0]).collect();
    let mut op = SHFILEOPSTRUCTW {
        hwnd: std::ptr::null_mut(),
        wFunc: FO_DELETE,
        pFrom: from.as_ptr(),
        pTo: std::ptr::null(),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_WANTNUKEWARNING | FOF_SILENT) as _,
        fAnyOperationsAborted: 0,
        hNameMappings: std::ptr::null_mut(),
        lpszProgressTitle: std::ptr::null(),
    };
    // SAFETY: `from` is double-NUL terminated and outlives the call; the struct is fully initialised.
    let r = unsafe { SHFileOperationW(&mut op) };
    if com {
        // SAFETY: this thread's own successful CoInitializeEx.
        unsafe { CoUninitialize() };
    }
    match (r, op.fAnyOperationsAborted) {
        (0, 0) if !path.exists() => Ok(()),
        (0, _) => Err("Remove Pack was cancelled; the folder is kept.".into()),
        (e, _) => Err(format!("Could not move {} to the Recycle Bin (error {e:#x})", path.display())),
    }
}

#[cfg(not(windows))]
pub fn recycle(path: &Path) -> Result<(), String> {
    Err(format!("No Recycle Bin here: {} is kept", path.display()))
}

// ---- Import ----

/// A file or folder name with the characters Windows refuses replaced.
fn clean_name(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect();
    let s = s.trim().trim_end_matches('.').trim().to_string();
    if s.is_empty() || s.starts_with('.') { format!("Pack{s}") } else { s }
}

/// A new hidden folder `.<name>.part` in `lib` to build a pack in, and the free folder (`name`, `name (2)`...) it
/// becomes. Made here, so a failure removes only what this import made.
fn stage(lib: &Path, name: &str) -> Result<(PathBuf, PathBuf), String> {
    std::fs::create_dir_all(lib).map_err(|e| format!("Cannot create the overlay library {}: {e}", lib.display()))?;
    let name = clean_name(name);
    for k in 1..1000 {
        let n = if k == 1 { name.clone() } else { format!("{name} ({k})") };
        let (fin, st) = (lib.join(&n), lib.join(format!(".{n}.part")));
        if fin.exists() {
            continue;
        }
        match std::fs::create_dir(&st) {
            Ok(()) => return Ok((st, fin)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("Cannot write to the overlay library: {e}")),
        }
    }
    Err("No free folder name in the overlay library".into())
}

/// The finished `stage` becomes pack folder `fin` (a stage holding one folder and nothing else: that folder); after a
/// failure the stage is removed.
fn finish(stage: &Path, fin: &Path, r: Result<(), String>) -> Result<PathBuf, String> {
    let r = r.and_then(|_| {
        let entries: Vec<_> = std::fs::read_dir(stage).map_err(|e| e.to_string())?.flatten().collect();
        let one = match &entries[..] {
            [e] if e.file_type().is_ok_and(|t| t.is_dir()) => Some(e.path()),
            _ => None,
        };
        match one {
            Some(inner) => {
                std::fs::rename(&inner, fin).map_err(|e| e.to_string())?;
                std::fs::remove_dir(stage).map_err(|e| e.to_string())
            }
            None => std::fs::rename(stage, fin).map_err(|e| e.to_string()),
        }
    });
    match r {
        Ok(()) => Ok(fin.to_path_buf()),
        Err(e) => {
            // Only this import's own folder (created empty by `stage`), never anything it did not make.
            let _ = std::fs::remove_dir_all(stage);
            Err(e)
        }
    }
}

/// Every file under `dir` with its size, relative to it. Links are not followed (or copied).
fn files_under(dir: &Path, rel: &Path, out: &mut Vec<(PathBuf, u64)>) -> Result<(), String> {
    for e in std::fs::read_dir(dir.join(rel)).map_err(|e| format!("Cannot read {}: {e}", dir.join(rel).display()))?.flatten() {
        let (Ok(md), r) = (std::fs::symlink_metadata(e.path()), rel.join(e.file_name())) else { continue };
        if md.file_type().is_symlink() {
            continue;
        } else if md.is_dir() {
            files_under(dir, &r, out)?;
        } else {
            out.push((r, md.len()));
        }
    }
    Ok(())
}

fn name_of(p: &Path) -> String {
    p.file_stem().or(p.file_name()).map_or_else(|| "Pack".into(), |n| n.to_string_lossy().into_owned())
}

/// Import Pack from Folder: copy `src` (files and subfolders) into the library `lib` as a new pack. `progress`: per
/// mille of the bytes copied.
pub fn import_folder(src: &Path, lib: &Path, progress: &AtomicU32) -> Result<PathBuf, String> {
    if inside(lib, src) {
        return Err("The overlay library is inside that folder: pick a folder outside it.".into());
    }
    let mut files = Vec::new();
    files_under(src, Path::new(""), &mut files)?;
    let total = files.iter().map(|f| f.1).sum::<u64>().max(1);
    let (st, fin) = stage(lib, &src.file_name().map_or_else(|| "Pack".into(), |n| n.to_string_lossy().into_owned()))?;
    let mut done = 0;
    let r = files.iter().try_for_each(|(rel, len)| {
        let to = st.join(rel);
        if let Some(d) = to.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        std::fs::copy(src.join(rel), &to).map_err(|e| format!("Copying {}: {e}", rel.display()))?;
        done += len;
        progress.store((done * 1000 / total) as u32, Relaxed);
        Ok(())
    });
    finish(&st, &fin, r)
}

/// A zip entry name that stays inside the folder it is extracted to: relative, no drive or stream (`:`), no `..`.
pub fn zip_entry_ok(name: &str) -> bool {
    let n = name.replace('\\', "/");
    !n.trim().is_empty() && !n.starts_with('/') && !n.contains(':') && n.split('/').all(|c| c.trim() != "..")
}

fn run(cmd: &mut std::process::Command, tool: &str) -> Result<String, String> {
    let out = crate::proc::output(cmd).map_err(|e| crate::proc::tool_error(&e, tool))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("{tool}: {}", err.lines().next().unwrap_or("failed").trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Nothing under `dir` is a link (an archive could plant one pointing out of the pack).
fn no_links(dir: &Path) -> Result<(), String> {
    for e in std::fs::read_dir(dir).map_err(|e| e.to_string())?.flatten() {
        let t = e.file_type().map_err(|e| e.to_string())?;
        if t.is_symlink() {
            return Err(format!("Refused: the zip holds a link ({})", e.file_name().to_string_lossy()));
        } else if t.is_dir() {
            no_links(&e.path())?;
        }
    }
    Ok(())
}

/// Import Pack from .zip: extract `zip` into the library `lib` with Windows' own tar.exe. Every entry is checked first,
/// and a zip with one that would land outside the pack folder is refused whole (nothing is extracted).
pub fn import_zip(zip: &Path, lib: &Path, progress: &AtomicU32) -> Result<PathBuf, String> {
    let list = run(crate::proc::tar().arg("-tf").arg(zip), "tar")?;
    let names: Vec<&str> = list.lines().filter(|l| !l.trim().is_empty()).collect();
    if names.is_empty() {
        return Err(format!("{} is empty or not a zip", zip.display()));
    }
    if let Some(bad) = names.iter().find(|n| !zip_entry_ok(n)) {
        return Err(format!("Refused {}: the entry \"{bad}\" would be written outside the pack folder.", zip.display()));
    }
    progress.store(100, Relaxed);
    let (st, fin) = stage(lib, &name_of(zip))?;
    let r = run(crate::proc::tar().arg("-xf").arg(zip).arg("-C").arg(&st), "tar").and_then(|_| no_links(&st));
    progress.store(950, Relaxed);
    finish(&st, &fin, r)
}

/// Create Sample Pack: overlays made by FFmpeg from scratch (nothing third-party), with the blend each is made for.
const SAMPLES: [(&str, Blend, &[&str]); 3] = [
    // Animated noise on mid grey: Overlay leaves grey alone and keeps the grain.
    ("Film Grain.mp4", Blend::Overlay, &[
        "-f", "lavfi", "-i", "color=c=gray:s=1280x720:r=24:d=3,noise=alls=25:allf=t+u,format=gray,format=yuv420p",
        "-c:v", "libx264", "-preset", "veryfast", "-tune", "grain", "-crf", "28", "-pix_fmt", "yuv420p",
    ]),
    // A drifting warm gradient into black: Screen adds the light.
    ("Light Leak.mp4", Blend::Screen, &[
        "-f", "lavfi", "-i", "gradients=s=1280x720:r=24:d=4:n=3:c0=0xffb040:c1=0xff3a20:c2=0x000000:x0=0:y0=0:x1=1280:y1=720:speed=0.02:seed=7,gblur=sigma=40,format=yuv420p",
        "-c:v", "libx264", "-preset", "veryfast", "-crf", "23",
    ]),
    // A white border on a transparent frame (PNG with alpha): Normal.
    ("Frame Border.png", Blend::Normal, &[
        "-f", "lavfi", "-i", "color=c=black@0.0:s=1280x720,format=rgba,drawbox=x=0:y=0:w=iw:h=ih:t=36:color=white@1.0:replace=1",
        "-frames:v", "1",
    ]),
];

pub const SAMPLE_PACK: &str = "Milan Sample Overlays";

/// The sample overlays' own blends, for the pack at `dir`.
pub fn sample_blends(dir: &Path) -> Vec<(PathBuf, Blend)> {
    SAMPLES.iter().map(|(name, b, _)| (dir.join(name), *b)).collect()
}

/// Make the sample pack in `lib` (with bundled FFmpeg). Its folder and the blends of its overlays.
pub fn create_sample(lib: &Path, progress: &AtomicU32) -> Result<(PathBuf, Vec<(PathBuf, Blend)>), String> {
    let (st, fin) = stage(lib, SAMPLE_PACK)?;
    let r = SAMPLES.iter().enumerate().try_for_each(|(k, (name, _, args))| {
        let mut cmd = crate::proc::ffmpeg();
        cmd.args(["-y", "-hide_banner", "-loglevel", "error"]).args(*args).arg(st.join(name));
        run(&mut cmd, "ffmpeg")?;
        progress.store(((k + 1) * 1000 / SAMPLES.len()) as u32, Relaxed);
        Ok(())
    });
    let fin = finish(&st, &fin, r)?;
    let blends = sample_blends(&fin);
    Ok((fin, blends))
}

// ---- The library at run time ----

/// One overlay file of a pack.
#[derive(Clone, Debug)]
pub struct Item {
    pub path: PathBuf,
    pub name: String,
    pub pack: PathBuf,
    pub modified: SystemTime,
}

/// What a scan found: each pack's overlays (None: its folder is missing), and the library's own subfolders.
struct Scan {
    packs: Vec<(PathBuf, Option<Vec<Item>>)>,
    subdirs: Vec<PathBuf>,
}

/// Overlays in `dir` and its subfolders (dot-folders skipped, 4 levels deep), by name.
fn overlays_in(dir: &Path, pack: &Path, depth: usize, out: &mut Vec<Item>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let (path, name) = (e.path(), e.file_name().to_string_lossy().into_owned());
        let Ok(md) = e.metadata() else { continue };
        if md.is_dir() && depth > 0 && !name.starts_with('.') {
            overlays_in(&path, pack, depth - 1, out);
        } else if md.is_file() && is_overlay(&path) {
            out.push(Item { path, name, pack: pack.to_path_buf(), modified: md.modified().unwrap_or(SystemTime::UNIX_EPOCH) });
        }
    }
}

fn scan(packs: Vec<PathBuf>, lib: PathBuf) -> Scan {
    let packs = packs
        .into_iter()
        .map(|p| {
            let found = p.is_dir().then(|| {
                let mut v = Vec::new();
                overlays_in(&p, &p, 4, &mut v);
                v.sort_by_key(|i| i.name.to_lowercase());
                v
            });
            (p, found)
        })
        .collect();
    let subdirs = std::fs::read_dir(&lib)
        .map(|rd| rd.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir()) && !e.file_name().to_string_lossy().starts_with('.')).map(|e| e.path()).collect())
        .unwrap_or_default();
    Scan { packs, subdirs }
}

/// A background import, sample pack or Recycle Bin move.
pub struct Job {
    pub what: String,
    /// Per mille done.
    pub progress: Arc<AtomicU32>,
    rx: Receiver<Result<Done, String>>,
}

pub enum Done {
    /// A new pack folder, with blends for some of its overlays.
    Pack(PathBuf, Vec<(PathBuf, Blend)>),
    Recycled(PathBuf),
}

pub struct Library {
    pub index: Index,
    /// Where the index is saved; None in the screenshot harness, which never writes the user's files.
    file: Option<PathBuf>,
    pub dir: PathBuf,
    pub items: Vec<Item>,
    /// Packs whose folder is missing (greyed, with Locate...).
    pub missing: HashSet<PathBuf>,
    scanning: Option<Receiver<Scan>>,
    rescan: bool,
    importer: Importer,
    asked: HashSet<PathBuf>,
    /// Probe results (None: unreadable).
    probed: HashMap<PathBuf, Option<MediaItem>>,
    /// Probes asked for this frame, started together.
    want: Vec<PathBuf>,
    pub jobs: Vec<Job>,
    pub message: Option<String>,
}

impl Library {
    pub fn new(dir: PathBuf, file: Option<PathBuf>) -> Self {
        let loaded = file.as_deref().map(Index::load);
        let message = matches!(loaded, Some(None)).then(|| "The overlay library index could not be read; changes to it are not saved this session.".to_string());
        // An unreadable index is never saved over.
        let file = file.filter(|_| message.is_none());
        Self {
            index: loaded.flatten().unwrap_or_default(),
            file,
            dir,
            items: Vec::new(),
            missing: HashSet::new(),
            scanning: None,
            // The first scan waits for the window to be shown (`refresh`).
            rescan: false,
            importer: Importer::new(),
            asked: HashSet::new(),
            probed: HashMap::new(),
            want: Vec::new(),
            jobs: Vec::new(),
            message,
        }
    }

    pub fn save(&mut self) {
        if let Some(f) = &self.file {
            if let Err(e) = self.index.save(f) {
                self.message = Some(format!("Could not save the overlay library index: {e}"));
            }
        }
    }

    /// The library folder moved (Preferences): its packs are looked for there.
    pub fn set_dir(&mut self, dir: PathBuf) {
        if dir != self.dir {
            self.dir = dir;
            self.rescan = true;
        }
    }

    pub fn refresh(&mut self) {
        self.rescan = true;
    }

    /// Scans, probes and jobs still running (the screenshot harness waits for them).
    pub fn busy(&self) -> bool {
        self.scanning.is_some() || self.rescan || self.asked.len() > self.probed.len() || !self.jobs.is_empty()
    }

    /// `path`'s probed media (duration, size, rate, alpha), asking for the probe on first use.
    pub fn probe(&mut self, path: &Path) -> Option<&MediaItem> {
        if !self.probed.contains_key(path) && self.asked.insert(path.to_path_buf()) {
            self.want.push(path.to_path_buf());
        }
        self.probed.get(path)?.as_ref()
    }

    /// `path`'s probed media if it is known, asking for nothing.
    pub fn known(&self, path: &Path) -> Option<&MediaItem> {
        self.probed.get(path)?.as_ref()
    }

    /// The probe of `path` ran and failed.
    pub fn unreadable(&self, path: &Path) -> bool {
        matches!(self.probed.get(path), Some(None))
    }

    pub fn used(&mut self, path: &Path) {
        self.index.used(path);
        self.save();
    }

    /// Run `f` on its own thread as a job the window shows with its progress.
    pub fn start(&mut self, ctx: &egui::Context, what: String, f: impl FnOnce(&AtomicU32) -> Result<Done, String> + Send + 'static) {
        let (tx, rx) = mpsc::channel();
        let progress = Arc::new(AtomicU32::new(0));
        let (p, ctx) = (progress.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(f(&p));
            ctx.request_repaint();
        });
        self.jobs.push(Job { what, progress, rx });
    }

    /// Forget pack `path` (its folder stays).
    pub fn forget(&mut self, path: &Path) {
        self.index.packs.retain(|p| p.path != path);
        self.save();
        self.rescan = true;
    }

    /// Take in finished scans, probes and jobs; start the scan and probes asked for.
    pub fn tick(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.scanning {
            match rx.try_recv() {
                Ok(s) => {
                    self.scanning = None;
                    self.take_scan(s);
                }
                Err(mpsc::TryRecvError::Disconnected) => self.scanning = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.rescan && self.scanning.is_none() {
            self.rescan = false;
            let (tx, rx) = mpsc::channel();
            let (packs, lib, ctx) = (self.index.packs.iter().map(|p| p.path.clone()).collect(), self.dir.clone(), ctx.clone());
            std::thread::spawn(move || {
                let _ = tx.send(scan(packs, lib));
                ctx.request_repaint();
            });
            self.scanning = Some(rx);
        }
        while let Some(batch) = self.importer.poll() {
            self.probed.extend(batch.into_iter().map(|(p, r)| (p, r.ok())));
        }
        if !self.want.is_empty() {
            self.importer.import(ctx, std::mem::take(&mut self.want));
        }
        let mut k = 0;
        while k < self.jobs.len() {
            match self.jobs[k].rx.try_recv() {
                Ok(r) => {
                    let job = self.jobs.remove(k);
                    self.finished(job, r);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.jobs.remove(k);
                }
                Err(mpsc::TryRecvError::Empty) => k += 1,
            }
        }
    }

    fn finished(&mut self, job: Job, r: Result<Done, String>) {
        match r {
            Ok(Done::Pack(path, blends)) => {
                if !self.index.packs.iter().any(|p| p.path == path) {
                    let name = path.file_name().map_or_else(|| "Pack".into(), |n| n.to_string_lossy().into_owned());
                    self.index.packs.push(Pack { name, path, linked: false, blend: None });
                }
                self.index.blends.extend(blends);
                self.message = Some(format!("{}: done.", job.what));
                self.save();
                self.rescan = true;
            }
            Ok(Done::Recycled(path)) => {
                self.message = Some(format!("{} is in the Recycle Bin.", path.display()));
                self.forget(&path);
            }
            Err(e) => self.message = Some(format!("{}: {e}", job.what)),
        }
    }

    fn take_scan(&mut self, s: Scan) {
        // The library's own subfolders are packs, also ones made outside Milan Pro.
        let mut added = false;
        for d in s.subdirs {
            if !self.index.packs.iter().any(|p| inside(&d, &p.path) && inside(&p.path, &d)) {
                let name = d.file_name().map_or_else(|| "Pack".into(), |n| n.to_string_lossy().into_owned());
                self.index.packs.push(Pack { name, path: d, linked: false, blend: None });
                added = true;
            }
        }
        self.missing.clear();
        self.items.clear();
        for (p, found) in s.packs {
            match found {
                Some(v) => self.items.extend(v),
                None => {
                    self.missing.insert(p);
                }
            }
        }
        if added {
            self.save();
            self.rescan = true; // the new packs' files
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("milan-overlays-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn blend_suggestions_and_precedence() {
        assert_eq!(Blend::suggest("Light Leaks Vol 2", false), Blend::Screen);
        assert_eq!(Blend::suggest("LENS_FLARES", false), Blend::Screen);
        assert_eq!(Blend::suggest("Bokeh", false), Blend::Screen);
        assert_eq!(Blend::suggest("Dust & Scratches", false), Blend::Screen);
        assert_eq!(Blend::suggest("16mm Film Grain", false), Blend::Screen);
        assert_eq!(Blend::suggest("Light Leaks", true), Blend::Normal, "alpha clips composite as they are");
        assert_eq!(Blend::suggest("Frames and Borders", false), Blend::Normal);
        let mut ix = Index { packs: vec![Pack { name: "Light Leaks".into(), path: r"C:\Lib\Light Leaks".into(), linked: false, blend: None }], ..Default::default() };
        let a = Path::new(r"C:\Lib\Light Leaks\sub\a.mp4");
        assert_eq!(ix.blend_for(a, false), Some(Blend::Screen));
        assert_eq!(ix.blend_for(Path::new(r"c:/lib/light leaks/b.mov"), true), Some(Blend::Normal), "any case or slash");
        assert_eq!(ix.blend_for(Path::new(r"C:\Lib\Other\a.mp4"), false), None);
        ix.packs[0].blend = Some(Blend::Add);
        assert_eq!(ix.blend_for(a, true), Some(Blend::Add), "the pack's own beats the suggestion");
        ix.blends.insert(a.into(), Blend::KeyGreen);
        assert_eq!(ix.blend_for(a, false), Some(Blend::KeyGreen), "the item's own beats the pack's");
    }

    #[test]
    fn green_key_and_screen_on_a_track() {
        let mut p = Project::new("t");
        apply_blend(&mut p, 0, Blend::KeyGreen);
        let t = &p.timeline.video_tracks[0];
        assert_eq!((t.composite, t.fx.len(), t.fx[0].plugin.as_str()), (Composite::SourceAlpha, 1, "milan.fx.chroma_key"));
        assert!(t.fx[0].params.contains_key("color.g"), "the Green Screen preset's colour");
        apply_blend(&mut p, 0, Blend::Multiply);
        assert_eq!(p.timeline.video_tracks[0].composite, Composite::Multiply);
    }

    #[test]
    fn index_saves_loads_and_survives_bad_fields() {
        let d = temp("index");
        let f = d.join("overlays.json");
        let mut ix = Index { packs: vec![Pack { name: "Leaks".into(), path: d.join("Leaks"), linked: true, blend: Some(Blend::Screen) }], ..Default::default() };
        ix.favorites.push(d.join("Leaks").join("a.mp4"));
        ix.tags.insert(d.join("Leaks").join("a.mp4"), vec!["warm".into(), "35mm".into()]);
        ix.blends.insert(d.join("Leaks").join("b.png"), Blend::Normal);
        for k in 0..40 {
            ix.used(&d.join(format!("{k}.mp4")));
        }
        assert_eq!((ix.recent.len(), ix.recent[0].file_name().unwrap().to_str()), (RECENT_MAX, Some("39.mp4")));
        ix.save(&f).unwrap();
        assert_eq!(Index::load(&f), Some(ix));
        // An older or newer file: unknown fields ignored, a bad pack entry and a bad field lost alone.
        let json = r#"{"packs":[{"name":"Old","path":"C:\\Old"},{"name":5},{"name":"Future","path":"C:\\F","blend":"Sparkle"}],"favorites":"x","from_the_future":1}"#;
        std::fs::write(&f, json).unwrap();
        let old = Index::load(&f).unwrap();
        assert_eq!(old.packs, vec![Pack { name: "Old".into(), path: r"C:\Old".into(), linked: false, blend: None }]);
        assert!(old.favorites.is_empty());
        // Not an index at all: kept aside, and an empty library.
        std::fs::write(&f, "garbage").unwrap();
        assert_eq!(Index::load(&f), Some(Index::default()));
        assert!(d.join("overlays.json.bad").exists() && !f.exists());
        assert_eq!(Index::load(&d.join("none.json")), Some(Index::default()), "no file yet");
        // There but unreadable (here: a folder): None, and a library on it never saves over it.
        std::fs::create_dir(d.join("locked.json")).unwrap();
        assert_eq!(Index::load(&d.join("locked.json")), None);
        let mut lib = Library::new(d.clone(), Some(d.join("locked.json")));
        lib.used(Path::new("x.mp4"));
        assert!(lib.message.is_some() && d.join("locked.json").is_dir());
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// Preferences from before overlay packs load with the defaults (and the tab not yet placed).
    #[test]
    fn old_prefs_load_without_overlay_fields() {
        let p: crate::settings::Prefs = serde_json::from_str(r#"{"quantize":false,"recent":["a.mnle"]}"#).unwrap();
        assert!(p.overlay_library.is_none() && !p.overlays_placed && !p.quantize);
        assert!(default_library().ends_with(r"Documents\Milan Pro\Overlay Packs"));
    }

    #[test]
    fn remove_pack_forgets_or_recycles() {
        let lib = Path::new(r"C:\Users\u\Documents\Milan Pro\Overlay Packs");
        let mut p = Project::new("t");
        let copied = Pack { name: "Leaks".into(), path: lib.join("Leaks"), linked: false, blend: None };
        assert_eq!(removal(&copied, lib, false, &p), Removal::Recycle { used: 0 });
        p.media_pool.push(MediaItem { path: lib.join("Leaks").join("a.mp4"), ..Default::default() });
        p.media_pool.push(MediaItem { path: lib.join("Leaks2").join("b.mp4"), ..Default::default() });
        assert_eq!(removal(&copied, Path::new(&lib.to_string_lossy().to_uppercase()), false, &p), Removal::Recycle { used: 1 }, "used by the project");
        assert_eq!(removal(&Pack { linked: true, ..copied.clone() }, lib, false, &p), Removal::Forget, "linked: only forgotten");
        assert_eq!(removal(&copied, lib, true, &p), Removal::Forget, "missing folder");
        let elsewhere = Pack { path: r"D:\Footage\Leaks".into(), ..copied.clone() };
        assert_eq!(removal(&elsewhere, lib, false, &p), Removal::Forget, "a copied pack from another library folder");
        let deep = Pack { path: lib.join("Leaks").join("sub"), ..copied.clone() };
        assert_eq!(removal(&deep, lib, false, &p), Removal::Forget, "only the library's own subfolders");
        let root = Pack { path: lib.to_path_buf(), ..copied };
        assert_eq!(removal(&root, lib, false, &p), Removal::Forget, "never the library itself");
    }

    #[test]
    fn import_folder_copies_into_the_library() {
        let d = temp("import-folder");
        let (src, lib) = (d.join("Light Leaks"), d.join("lib"));
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("a.mp4"), b"aaaa").unwrap();
        std::fs::write(src.join("sub").join("b.png"), b"bb").unwrap();
        let p = AtomicU32::new(0);
        let fin = import_folder(&src, &lib, &p).unwrap();
        assert_eq!((fin.clone(), p.load(Relaxed)), (lib.join("Light Leaks"), 1000));
        assert_eq!(std::fs::read(fin.join("sub").join("b.png")).unwrap(), b"bb");
        assert!(src.join("a.mp4").exists(), "the source stays");
        assert_eq!(import_folder(&src, &lib, &p).unwrap(), lib.join("Light Leaks (2)"));
        // The library inside the source would copy itself.
        assert!(import_folder(&d, &lib, &p).is_err());
        let left: Vec<_> = std::fs::read_dir(&lib).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(left.len(), 2, "no staging folder left: {left:?}");
        // The scan finds the overlays (not other files) and adopts library folders.
        let s = scan(vec![fin.clone(), d.join("gone")], lib.clone());
        let names: Vec<&str> = s.packs[0].1.as_ref().unwrap().iter().map(|i| i.name.as_str()).collect();
        assert_eq!((names, s.packs[1].1.is_none(), s.subdirs.len()), (vec!["a.mp4", "b.png"], true, 2));
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// A stored (uncompressed) zip of `entries`, written by hand so a test can hold any entry name.
    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        fn crc32(d: &[u8]) -> u32 {
            let mut c = !0u32;
            for &b in d {
                c ^= b as u32;
                for _ in 0..8 {
                    c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
                }
            }
            !c
        }
        let (mut out, mut central) = (Vec::new(), Vec::new());
        for (name, data) in entries {
            let (off, crc, n, sz) = (out.len() as u32, crc32(data), name.len() as u16, data.len() as u32);
            let common = |v: &mut Vec<u8>| {
                for x in [0u16, 0, 0, 0x21] {
                    v.extend(x.to_le_bytes()); // flags, method (stored), time, date
                }
                v.extend(crc.to_le_bytes());
                v.extend(sz.to_le_bytes());
                v.extend(sz.to_le_bytes());
                v.extend(n.to_le_bytes());
                v.extend(0u16.to_le_bytes()); // extra
            };
            out.extend(0x0403_4b50u32.to_le_bytes());
            out.extend(20u16.to_le_bytes());
            common(&mut out);
            out.extend(name.as_bytes());
            out.extend(*data);
            central.extend(0x0201_4b50u32.to_le_bytes());
            central.extend([20u8, 0, 20, 0]);
            common(&mut central);
            central.extend([0u8; 10]); // comment length, disk, internal and external attributes
            central.extend(off.to_le_bytes());
            central.extend(name.as_bytes());
        }
        let (cd_off, cd_len, k) = (out.len() as u32, central.len() as u32, entries.len() as u16);
        out.extend(central);
        out.extend(0x0605_4b50u32.to_le_bytes());
        out.extend([0u8; 4]);
        out.extend(k.to_le_bytes());
        out.extend(k.to_le_bytes());
        out.extend(cd_len.to_le_bytes());
        out.extend(cd_off.to_le_bytes());
        out.extend([0u8; 2]);
        std::fs::write(path, out).unwrap();
    }

    #[test]
    fn zip_entry_names() {
        for ok in ["a.mp4", "Pack/sub/b.png", "dots..mp4", "x/.hidden"] {
            assert!(zip_entry_ok(ok), "{ok}");
        }
        for bad in ["../evil.mp4", "a/../../evil", r"a\..\..\evil", "/abs.mp4", r"\abs", "C:/x.mp4", "C:x", "a.mp4:stream", "", "a/ .. /b"] {
            assert!(!zip_entry_ok(bad), "{bad}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn import_zip_extracts_and_refuses_zip_slip() {
        let d = temp("import-zip");
        let lib = d.join("lib");
        let p = AtomicU32::new(0);
        // One top folder: it becomes the pack folder, named after the zip.
        write_zip(&d.join("Bokeh Pack.zip"), &[("Bokeh/", b""), ("Bokeh/a.mp4", b"aaaa"), ("Bokeh/sub/b.png", b"bb")]);
        let fin = import_zip(&d.join("Bokeh Pack.zip"), &lib, &p).unwrap();
        assert_eq!(fin, lib.join("Bokeh Pack"));
        assert_eq!(std::fs::read(fin.join("sub").join("b.png")).unwrap(), b"bb");
        // Loose files: the pack folder holds them.
        write_zip(&d.join("loose.zip"), &[("a.mp4", b"1"), ("b.mp4", b"2")]);
        assert!(import_zip(&d.join("loose.zip"), &lib, &p).unwrap().join("b.mp4").exists());
        // Zip slip: refused whole, nothing written outside (or inside) the library.
        write_zip(&d.join("evil.zip"), &[("ok.mp4", b"1"), ("../evil.txt", b"pwned")]);
        assert!(import_zip(&d.join("evil.zip"), &lib, &p).unwrap_err().contains("outside the pack folder"));
        write_zip(&d.join("evil2.zip"), &[(r"..\..\evil2.txt", b"pwned")]);
        assert!(import_zip(&d.join("evil2.zip"), &lib, &p).is_err());
        assert!(!d.join("evil.txt").exists() && !lib.join("evil.txt").exists() && !d.join("evil2.txt").exists());
        let mut left: Vec<String> = std::fs::read_dir(&lib).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        assert_eq!(left, ["Bokeh Pack", "loose"]);
        assert!(import_zip(&d.join("missing.zip"), &lib, &p).is_err());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
