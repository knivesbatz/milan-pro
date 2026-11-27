//! R23: Selectively Prerender Video (Tools, Shift+M). The sections of the selection that are more than a plain event
//! (`plan::work_sections`) are rendered through the engine to files in the project's prerender folder (a folder per
//! project in Preferences › Video's prerendered files folder), named by the hash of their frames' keys
//! (`plan::range_hash`). The registry below holds the project's index (`index.json` beside the files); the video worker
//! plays a section's frames from its file while the hash still matches the plan, and a render does the same, so an edit
//! inside a section only makes that section inactive. Clean Up Prerendered Video deletes a project's files (all, or the
//! inactive ones), never anything the index does not list.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use crate::engine::plan::{self, MediaRef, VideoPlan};
use crate::project::Project;

/// One prerendered section: frames `a..b` of the project at `w` x `h` and `fps`, in `file`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Entry {
    pub file: PathBuf,
    pub a: i64,
    pub b: i64,
    pub fps: f64,
    pub fps_str: String,
    pub w: u32,
    pub h: u32,
    /// `plan::range_hash` of the plan it was rendered from.
    pub hash: u64,
    pub template: String,
    /// The codec as ffprobe names it (what the decoder is told).
    pub codec: String,
    pub made: String,
}

impl Default for Entry {
    fn default() -> Self {
        Entry { file: PathBuf::new(), a: 0, b: 0, fps: 30.0, fps_str: "30".into(), w: 0, h: 0, hash: 0, template: String::new(), codec: String::new(), made: String::new() }
    }
}

impl Entry {
    /// Still what the plan would compose: same size, rate, no alpha, and every frame's key unchanged.
    pub fn valid(&self, vp: &VideoPlan) -> bool {
        self.fps_str == vp.fps_str && (self.w, self.h) == (vp.w, vp.h) && !vp.alpha && self.b > self.a && plan::range_hash(vp, self.a, self.b) == self.hash
    }

    /// The file as the decoder pool reads it: frame `n - a` of the section is frame `n` of the project.
    pub fn media_ref(&self) -> MediaRef {
        MediaRef {
            id: format!("prerender:{}", self.file.display()).into(),
            path: self.file.clone(),
            w: self.w,
            h: self.h,
            duration: (self.b - self.a) as f64 / self.fps,
            fps: self.fps,
            still: false,
            has_alpha: false,
            // Renders write BT.709 limited-range YUV (`engine::render::video_filter`).
            matrix: Some("bt709"),
            codec: Some(self.codec.clone()),
            deint: "",
            proxy: None,
            grid: 0.0,
            offset: 0.0,
            stream: 0,
            flow: false,
            pre: String::new(),
            input: Vec::new(),
            wide: None,
        }
    }

    /// Seconds the section covers.
    pub fn span(&self) -> (f64, f64) {
        (self.a as f64 / self.fps, self.b as f64 / self.fps)
    }
}

#[derive(Default, Serialize, Deserialize)]
struct Index {
    entries: Vec<Entry>,
}

struct Reg {
    /// The current project's folder (None before `sync`).
    folder: Option<PathBuf>,
    entries: Vec<Entry>,
}

static REG: Mutex<Reg> = Mutex::new(Reg { folder: None, entries: Vec::new() });
/// Bumped on every change, so the worker and the ruler read the entries again.
static GENERATION: AtomicU64 = AtomicU64::new(0);

fn lock() -> std::sync::MutexGuard<'static, Reg> {
    REG.lock().unwrap_or_else(|e| e.into_inner())
}

/// `%LOCALAPPDATA%\MilanPro\prerender` (the temp folder without a profile).
pub fn default_folder() -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map_or_else(|| std::env::temp_dir().join("milan-prerender"), |l| PathBuf::from(l).join("MilanPro").join("prerender"))
}

/// FNV-1a of `s`, lower-cased (Windows paths ignore case).
fn fnv(s: &str) -> u64 {
    s.to_lowercase().bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3))
}

/// The project's own folder under `base`: `<file stem>-<hash of the path>` (an unsaved project: its name).
pub fn project_folder(p: &Project, base: &Path) -> PathBuf {
    let (stem, id) = match &p.path {
        Some(path) => (path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(), path.to_string_lossy().into_owned()),
        None => (p.name.clone(), format!("unsaved:{}", p.name)),
    };
    let stem: String = stem.chars().filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.')).take(40).collect();
    let stem = if stem.trim().is_empty() { "Untitled".to_string() } else { stem.trim().to_string() };
    base.join(format!("{stem}-{:08x}", fnv(&id) as u32))
}

fn index_path(folder: &Path) -> PathBuf {
    folder.join("index.json")
}

fn load(folder: &Path) -> Vec<Entry> {
    let ix: Index = std::fs::read_to_string(index_path(folder)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    ix.entries.into_iter().filter(|e| e.file.exists()).collect()
}

fn save(folder: &Path, entries: &[Entry]) {
    let _ = std::fs::create_dir_all(folder);
    if let Ok(s) = serde_json::to_string_pretty(&Index { entries: entries.to_vec() }) {
        let _ = std::fs::write(index_path(folder), s);
    }
}

/// Once per UI frame: the registry follows the project (its folder under `base`, None = `default_folder`).
pub fn sync(p: &Project, base: Option<&Path>) {
    let folder = project_folder(p, base.unwrap_or(&default_folder()));
    let mut r = lock();
    if r.folder.as_ref() != Some(&folder) {
        r.entries = load(&folder);
        r.folder = Some(folder);
        GENERATION.fetch_add(1, Relaxed);
    }
}

/// Tests: use `folder` for the project at hand.
#[cfg(test)]
pub fn sync_to(folder: PathBuf) {
    let mut r = lock();
    r.entries = load(&folder);
    r.folder = Some(folder);
    GENERATION.fetch_add(1, Relaxed);
}

/// The current project's prerender folder.
pub fn folder() -> Option<PathBuf> {
    lock().folder.clone()
}

pub fn entries() -> Vec<Entry> {
    lock().entries.clone()
}

pub fn generation() -> u64 {
    GENERATION.load(Relaxed)
}

/// A section rendered: into the index (replacing an entry of the same file).
pub fn add(e: Entry) {
    let mut r = lock();
    r.entries.retain(|x| x.file != e.file);
    r.entries.push(e);
    if let Some(f) = r.folder.clone() {
        save(&f, &r.entries);
    }
    GENERATION.fetch_add(1, Relaxed);
}

/// Clean Up: delete these entries' files and drop them from the index. Only files the index lists are ever deleted.
pub fn remove(files: &[PathBuf]) -> usize {
    let mut r = lock();
    let mut n = 0;
    r.entries.retain(|e| {
        if files.contains(&e.file) {
            n += std::fs::remove_file(&e.file).is_ok() as usize;
            false
        } else {
            true
        }
    });
    if let Some(f) = r.folder.clone() {
        save(&f, &r.entries);
    }
    GENERATION.fetch_add(1, Relaxed);
    n
}

/// The plan a prerendered section's hash is taken on: the project's, without proxies (a render never reads them, and a
/// preview playing a section reads the file, not the sources).
pub fn canonical(p: &Project) -> VideoPlan {
    plan::compile(p).without_proxies()
}

/// The entries that are inactive (an edit changed their frames) for the project as it is.
pub fn inactive(p: &Project) -> Vec<Entry> {
    let vp = canonical(p);
    entries().into_iter().filter(|e| !e.valid(&vp)).collect()
}

/// A template's codec as ffprobe names the stream (what `MediaRef::codec` carries).
pub fn codec_name(enc: &str) -> String {
    match enc {
        "libx264" | "h264_hw" | "h264_nvenc" | "h264_qsv" | "h264_amf" | "h264_mf" => "h264".into(),
        "libx265" | "hevc_hw" | "hevc_nvenc" | "hevc_qsv" | "hevc_amf" | "hevc_mf" => "hevc".into(),
        "libsvtav1" | "av1_hw" | "av1_nvenc" | "av1_qsv" | "av1_amf" | "av1_mf" => "av1".into(),
        "prores_ks" => "prores".into(),
        "libvpx-vp9" => "vp9".into(),
        "libvpx" => "vp8".into(),
        "dnxhd" => "dnxhd".into(),
        other => other.into(),
    }
}

/// The templates Selectively Prerender Video offers: video at the project's size, rate and field order, not an image
/// sequence (the built-in ones and the user's).
pub fn templates(prefs: &crate::settings::Prefs) -> Vec<crate::render::Template> {
    crate::render::BUILTIN.iter().chain(&prefs.render_templates).filter(|t| t.video.as_ref().is_some_and(|v| v.size.is_none() && v.fps.is_none() && v.field.is_none() && v.par.is_none()) && !t.sequence() && t.check().is_ok()).cloned().collect()
}

/// The default template: the one last used, else MOV ProRes 422 (an intermediate that decodes fast).
pub const DEFAULT_TEMPLATE: &str = "MOV ProRes 422";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_folders_are_per_project_and_safe() {
        let base = Path::new(r"C:\pre");
        let mut p = Project::new("My: Film?");
        let unsaved = project_folder(&p, base);
        assert!(unsaved.starts_with(base) && unsaved.file_name().unwrap().to_string_lossy().starts_with("My Film-"), "{}", unsaved.display());
        p.path = Some(PathBuf::from(r"C:\v\Show.mnle"));
        let saved = project_folder(&p, base);
        assert!(saved.file_name().unwrap().to_string_lossy().starts_with("Show-") && saved != unsaved);
        p.path = Some(PathBuf::from(r"c:\V\SHOW.MNLE"));
        let id = |f: &Path| f.file_name().unwrap().to_string_lossy().rsplit('-').next().unwrap().to_string();
        assert_eq!(id(&project_folder(&p, base)), id(&saved), "Windows paths ignore case");
        p.path = Some(PathBuf::from(r"C:\w\Show.mnle"));
        assert_ne!(project_folder(&p, base), saved, "another folder's Show is another project");
        assert_eq!(codec_name("prores_ks"), "prores");
        assert_eq!(codec_name("hevc_amf"), "hevc");
        let t = templates(&crate::settings::Prefs::default());
        assert!(t.iter().any(|t| t.name == DEFAULT_TEMPLATE) && t.iter().all(|t| t.video.is_some() && !t.sequence()));
        assert!(!t.iter().any(|t| t.name == "YouTube 1080p"), "a fixed-size template does not prerender the project");
    }
}
