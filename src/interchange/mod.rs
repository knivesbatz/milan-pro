//! Interchange with other editors (File › Import / Export):
//! - `vegas`: the `.milan.json` that `tools/vegas/Export to Milan Pro.cs` writes from inside VEGAS, with `vegas_map`
//!   turning VEGAS's OFX plug-ins into Milan's built-ins (R25), and the `.veg` media scan;
//! - `vegas_export`: the same JSON written from a Milan project for `tools/vegas/Import from Milan Pro.cs` (R26);
//! - `archive`: Export › Archive Project and Import › Media from Project (R30);
//! - `otio`: OpenTimelineIO, lossless for Milan's own files through `metadata["milan"]` (R28);
//! - `edl`: CMX 3600 EDLs and VEGAS's EDL text table (R28), through the shared `cutlist`.
//! `Format` tells the files apart and `import` / `export` route them. `docs/INTERCHANGE.md` says what each route carries.
pub mod archive;
pub mod cutlist;
pub mod edl;
pub mod otio;
pub mod vegas;
pub mod vegas_export;
pub mod vegas_map;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use eframe::egui;
use crate::project::{MediaItem, Project};
pub use vegas::{read_vegas_json, scan_veg, to_project};

/// What a finished import gives the app.
pub enum Imported {
    /// From a script export: the project (replaces the open one through the save prompt) and its warnings.
    Project(Project, Vec<String>),
    /// From a .veg: media files to add to Project Media, and the ones not found.
    Media(Vec<PathBuf>, Vec<String>),
}

/// The interchange formats of File › Import / Export, told apart by the file's extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    /// A `.milan.json` from the VEGAS script, or a `.veg` (media scan).
    #[default]
    Vegas,
    Otio,
    /// CMX 3600 (`.edl`) or VEGAS's EDL text table (`.txt`).
    Edl,
}

impl Format {
    pub fn of(path: &Path) -> Format {
        match path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).as_deref() {
            Some("otio") => Format::Otio,
            Some("edl") | Some("txt") => Format::Edl,
            _ => Format::Vegas,
        }
    }

    /// The menu row's noun: "VEGAS Project", "OpenTimelineIO", ...
    pub fn label(self) -> &'static str {
        match self {
            Format::Vegas => "VEGAS Project",
            Format::Otio => "OpenTimelineIO",
            Format::Edl => "EDL Text File",
        }
    }

    /// The file dialog's filter and extensions.
    pub fn filter(self) -> (&'static str, &'static [&'static str]) {
        match self {
            Format::Vegas => ("VEGAS Project (Milan Pro export, .veg)", &["json", "veg"]),
            Format::Otio => ("OpenTimelineIO (*.otio)", &["otio"]),
            Format::Edl => ("EDL (CMX 3600 *.edl, VEGAS *.txt)", &["edl", "txt"]),
        }
    }

    pub fn import_title(self) -> String {
        format!("Import {}", self.label())
    }

    pub fn export_title(self) -> String {
        format!("Export {}", self.label())
    }

    /// The import report's first line.
    pub fn import_intro(self) -> String {
        match self {
            Format::Vegas => "The VEGAS project is open as a new, unsaved project. These parts of it did not carry over:".into(),
            Format::Otio => "The timeline is open as a new, unsaved project. These parts of it did not carry over:".into(),
            Format::Edl => "The EDL is open as a new, unsaved project. These parts of it did not carry over:".into(),
        }
    }

    /// The export report's first line.
    pub fn export_intro(self, path: &Path, notes_follow: bool) -> String {
        format!("Written {}.{}", path.display(), if notes_follow { format!(" These parts have no {} counterpart:", self.label()) } else { String::new() })
    }
}

/// An import on its own thread: reading, probing every media file, building the project. (Named for the VEGAS route
/// it was built for; every `Format` goes through it.)
#[derive(Default)]
pub struct VegasImport {
    rx: Option<Receiver<Result<Imported, String>>>,
    format: Format,
}

impl VegasImport {
    /// Start importing `path` (its extension picks the format); `fps` is the open project's frame rate, which an EDL
    /// counts its timecodes at. A running import is dropped.
    pub fn start(&mut self, ctx: &egui::Context, path: PathBuf, fps: f64) {
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.format = Format::of(&path);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(import(&path, fps).map_err(|e| format!("{}: {e}", path.display())));
            ctx.request_repaint();
        });
    }

    /// The format of the last import started.
    pub fn format(&self) -> Format {
        self.format
    }

    /// Started and not yet taken by `poll`.
    pub fn busy(&self) -> bool {
        self.rx.is_some()
    }

    pub fn poll(&mut self) -> Option<Result<Imported, String>> {
        let r = self.rx.as_ref()?.try_recv().ok()?;
        self.rx = None;
        Some(r)
    }
}

/// Write `p` as `path` in `format`; returns the report's notes.
pub fn export(format: Format, p: &Project, path: &Path) -> anyhow::Result<Vec<String>> {
    match format {
        Format::Vegas => vegas_export::write(p, path),
        Format::Otio => otio::write(p, path),
        Format::Edl => edl::export(p, path),
    }
}

fn import(path: &Path, fps: f64) -> anyhow::Result<Imported> {
    let bytes = std::fs::read(path)?;
    let dir = path.parent();
    match Format::of(path) {
        Format::Otio => {
            let tl = otio::parse(std::str::from_utf8(&bytes)?)?;
            let (p, warnings) = otio::to_project(&tl, &probe_all(&otio::media_paths(&tl, dir)), dir);
            return Ok(Imported::Project(p, warnings));
        }
        Format::Edl => {
            let (mut cut, mut warnings) = edl::read(&String::from_utf8_lossy(&bytes), fps, dir)?;
            if cut.name.is_empty() {
                cut.name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            }
            let (p, more) = cutlist::to_project(&cut, &probe_all(&cut.media_paths()));
            warnings.extend(more);
            return Ok(Imported::Project(p, warnings));
        }
        Format::Vegas => {}
    }
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("veg")) {
        let (found, missing): (Vec<_>, Vec<_>) = scan_veg(&bytes).into_iter().map(PathBuf::from).partition(|p| p.is_file());
        return Ok(Imported::Media(found, missing.iter().map(|p| format!("Not found: {}", p.display())).collect()));
    }
    let mut e = read_vegas_json(std::str::from_utf8(&bytes)?)?;
    if e.name.is_empty() {
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        e.name = stem.trim_end_matches(".milan").to_string();
    }
    let (p, warnings) = to_project(&e, &probe_all(&e.media_paths()));
    Ok(Imported::Project(p, warnings))
}

/// Probe `paths`, eight ffprobes at a time.
pub fn probe_all(paths: &[String]) -> HashMap<String, Result<MediaItem, String>> {
    let probe = |p: &String| {
        let path = Path::new(p);
        let r = if path.is_file() { crate::media::probe_media(path).map_err(|e| e.to_string()) } else { Err("file not found".to_string()) };
        (p.clone(), r)
    };
    paths
        .chunks(8)
        .flat_map(|chunk| std::thread::scope(|s| chunk.iter().map(|p| s.spawn(move || probe(p))).collect::<Vec<_>>().into_iter().filter_map(|h| h.join().ok()).collect::<Vec<_>>()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_reads_both_kinds_of_file() {
        let dir = std::env::temp_dir().join(format!("milan-vegas-import-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (clip, gone) = (dir.join("clip.wav"), dir.join("gone.wav"));
        std::fs::write(&clip, b"").unwrap();
        let veg: Vec<u8> = format!("{}\0{}\0", clip.display(), gone.display()).encode_utf16().flat_map(u16::to_le_bytes).collect();
        std::fs::write(dir.join("Show.veg"), veg).unwrap();
        let json = format!(r#"{{"format":"milan-vegas","tracks":[{{"kind":"audio","events":[{{"length":1,"path":{}}}]}}]}}"#, serde_json::to_string(&gone).unwrap());
        std::fs::write(dir.join("Show.milan.json"), json).unwrap();
        let from_veg = import(&dir.join("Show.veg"), 25.0);
        let from_json = import(&dir.join("Show.milan.json"), 25.0);
        std::fs::remove_dir_all(&dir).unwrap();
        let Ok(Imported::Media(found, missing)) = from_veg else { panic!("not media") };
        assert_eq!((found, missing), (vec![clip], vec![format!("Not found: {}", gone.display())]));
        let Ok(Imported::Project(p, w)) = from_json else { panic!("not a project") };
        assert_eq!((p.name.as_str(), p.timeline.audio_tracks[0].clips.len()), ("Show", 1));
        assert_eq!(w, [format!("Media offline: {} (file not found)", gone.display())]);
    }
}
