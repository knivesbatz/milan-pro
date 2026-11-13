//! VEGAS project import (File › Import › VEGAS Project...): the `.milan.json` that `tools/vegas/Export to Milan
//! Pro.cs` writes from inside VEGAS, or, without VEGAS, a scan of a `.veg` for the media it uses.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use eframe::egui;
use serde::Deserialize;
use crate::edit;
use crate::project::*;

/// The script's JSON. Every field is optional, so exports from older or newer scripts still load.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct VegasExport {
    pub format: String,
    pub version: u32,
    /// The .veg's file name without extension.
    pub name: String,
    pub project: VProject,
    pub tracks: Vec<VTrack>,
    pub markers: Vec<VSpan>,
    pub regions: Vec<VSpan>,
    /// The time selection (VEGAS's loop region).
    #[serde(rename = "loop")]
    pub loop_region: Option<VSpan>,
    /// What the script saw and could not export (FX, generated media, transitions...), as sentences.
    pub unsupported: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct VProject {
    pub w: u32,
    pub h: u32,
    pub fps: f64,
    pub sample_rate: u32,
}

/// A marker (length 0), region or the loop region; times in seconds.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct VSpan {
    pub t: f64,
    pub length: f64,
    pub name: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct VTrack {
    /// "video" or "audio".
    pub kind: String,
    pub name: String,
    pub mute: bool,
    pub solo: bool,
    /// Audio: linear gain (1 = 0 dB).
    #[serde(default = "one")]
    pub volume: f64,
    /// Audio: -1 (left) ..= 1 (right).
    pub pan: f64,
    /// Video: composite level 0..=1.
    #[serde(default = "one")]
    pub level: f64,
    /// Video: VEGAS `CompositeMode` name (SrcAlpha, Add, ...).
    pub mode: String,
    pub envelopes: Vec<VEnvelope>,
    pub events: Vec<VEvent>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct VEnvelope {
    /// VEGAS `EnvelopeType` name: Volume, Pan, Mute, Composite, ...
    pub kind: String,
    /// The envelope's no-effect value (Mute: not muted).
    pub neutral: f64,
    pub points: Vec<VPoint>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct VPoint {
    pub t: f64,
    pub v: f64,
    /// VEGAS `CurveType` name: Linear, Fast, Slow, Smooth, Sharp, None (hold).
    pub curve: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct VEvent {
    pub start: f64,
    pub length: f64,
    /// Active take offset: the source time at the event start (a subclip's already added in).
    pub offset: f64,
    /// Played backwards (a reversed subclip): `offset` is the end of the source span.
    pub reverse: bool,
    #[serde(default = "one")]
    pub rate: f64,
    pub path: String,
    pub stream: i32,
    /// The active take's name.
    pub name: String,
    pub fade_in: f64,
    pub fade_out: f64,
    pub fade_in_curve: String,
    pub fade_out_curve: String,
    /// Event gain (audio, linear) or opacity (video): `FadeIn.Gain`.
    #[serde(default = "one")]
    pub gain: f64,
    pub mute: bool,
    pub locked: bool,
    /// Index of the event's group, shared by its members.
    pub group: Option<i64>,
    /// Inactive takes.
    pub takes: Vec<VTake>,
    /// Video: Pan/Crop keyframes, and the source frame size their corners are measured in.
    pub pan_crop: Vec<VPanKey>,
    pub src_size: Option<[f64; 2]>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct VTake {
    pub path: String,
    pub offset: f64,
    pub reverse: bool,
    pub name: String,
    pub stream: i32,
}

/// A Pan/Crop keyframe: event-relative time and three corners of the crop rectangle in source pixels.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct VPanKey {
    pub t: f64,
    pub tl: [f64; 2],
    pub tr: [f64; 2],
    pub bl: [f64; 2],
    /// VEGAS `VideoKeyframeType` name.
    pub curve: String,
}

fn one() -> f64 {
    1.0
}

/// Marker flag colour (VP16), as `ui::add_marker` gives new markers.
const MARKER_COLOR: [u8; 3] = [0xDB, 0x8E, 0x57];

/// Parse the script's JSON; anything but `format: "milan-vegas"` is an error.
pub fn read_vegas_json(text: &str) -> anyhow::Result<VegasExport> {
    let e: VegasExport = serde_json::from_str(text.trim_start_matches('\u{feff}'))?;
    anyhow::ensure!(e.format == "milan-vegas", "not an export from the Milan Pro VEGAS script (format \"{}\")", e.format);
    Ok(e)
}

fn curve(name: &str, default: Curve) -> Curve {
    match name {
        "Linear" => Curve::Linear,
        "Fast" => Curve::Fast,
        "Slow" => Curve::Slow,
        "Smooth" => Curve::Smooth,
        "Sharp" => Curve::Sharp,
        "None" | "Hold" => Curve::Hold,
        _ => default,
    }
}

/// A VEGAS crop rectangle (corners in source pixels, y down) as Milan's Pan/Crop.
fn pan_crop(k: &VPanKey, (sw, sh): (f64, f64)) -> PanCrop {
    let (u, v) = ((k.tr[0] - k.tl[0], k.tr[1] - k.tl[1]), (k.bl[0] - k.tl[0], k.bl[1] - k.tl[1]));
    // Corners in mirror order mean a flip.
    let flip = if u.0 * v.1 - u.1 * v.0 < 0.0 { -1.0 } else { 1.0 };
    let (mut w, mut h, mut rot) = (u.0.hypot(u.1) / sw, flip * v.0.hypot(v.1) / sh, u.1.atan2(u.0).to_degrees());
    // Turned past 90 degrees is the same picture as turned 180 back with both sides flipped (a mirror, not 180).
    if rot.abs() > 90.0 {
        (w, h, rot) = (-w, -h, rot - 180.0f64.copysign(rot));
    }
    // The fourth corner is tl + u + v, so the centre is tl + (u + v) / 2.
    let (cx, cy) = (k.tl[0] + (u.0 + v.0) / 2.0, k.tl[1] + (u.1 + v.1) / 2.0);
    PanCrop { cx: (cx / sw) as f32, cy: (cy / sh) as f32, w: w as f32, h: h as f32, rot: rot as f32 }
}

fn composite(mode: &str) -> Option<Composite> {
    Some(match mode {
        "" | "SrcAlpha" => Composite::SourceAlpha,
        // The 3D track motion itself is reported separately (R25).
        "SrcAlpha3D" => Composite::ThreeD,
        "Add" => Composite::Add,
        "Subtract" => Composite::Subtract,
        "Cut" => Composite::Cut,
        "Multiply" => Composite::Multiply,
        "Screen" => Composite::Screen,
        "Overlay" => Composite::Overlay,
        "HardLight" => Composite::HardLight,
        "Dodge" => Composite::Dodge,
        "Burn" => Composite::Burn,
        "Darken" => Composite::Darken,
        "Lighten" => Composite::Lighten,
        "Difference" => Composite::Difference,
        "DifferenceSquared" => Composite::DifferenceSquared,
        // Custom needs its compositor plug-in, which the export does not carry.
        _ => return None,
    })
}

fn db(gain: f64) -> f32 {
    if gain > 0.0 { (20.0 * gain.log10()).max(-120.0) as f32 } else { -120.0 }
}

/// A track envelope as Milan's kind and value, or None when Milan has no such envelope on this kind of track.
fn envelope(kind: &str, video: bool) -> Option<EnvKind> {
    let k = match kind {
        "Volume" => EnvKind::Volume,
        "Pan" => EnvKind::Pan,
        "Mute" => EnvKind::Mute,
        "Composite" => EnvKind::CompositeLevel,
        "FadeToColor" => EnvKind::FadeToColor,
        _ => return None,
    };
    edit::env_fits(k, video).then_some(k)
}

fn env_value(kind: EnvKind, v: f64, neutral: f64) -> f32 {
    match kind {
        EnvKind::Volume => db(v),
        EnvKind::Mute => if (v - neutral).abs() < 0.5 { 1.0 } else { 0.0 },
        EnvKind::Pan | EnvKind::FadeToColor => v.clamp(-1.0, 1.0) as f32,
        _ => v.clamp(0.0, 1.0) as f32,
    }
}

/// The pool item for `path`, added on first use: the probed item, or an offline one (plus a warning) when the
/// probe failed.
fn media_id(pool: &mut Vec<MediaItem>, probed: &HashMap<String, Result<MediaItem, String>>, path: &str, video: bool, warnings: &mut Vec<String>) -> String {
    if let Some(m) = pool.iter().find(|m| m.path.as_os_str() == path) {
        return m.id.clone();
    }
    let item = match probed.get(path) {
        Some(Ok(m)) => m.clone(),
        err => {
            let why = err.and_then(|r| r.as_ref().err()).map_or("not probed", String::as_str);
            warnings.push(format!("Media offline: {path} ({why})"));
            let p = PathBuf::from(path);
            MediaItem {
                id: uuid::Uuid::new_v4().to_string(),
                name: p.file_name().map_or(path.to_string(), |n| n.to_string_lossy().into_owned()),
                path: p,
                media_type: if video { MediaType::Video } else { MediaType::Audio },
                ..Default::default()
            }
        }
    };
    let id = item.id.clone();
    pool.push(item);
    id
}

/// Build the project from an export; `probed` holds each media path's probe. Timeline times snap to the project's
/// frames (source offsets do not). Returns the project and what did not carry over.
pub fn to_project(e: &VegasExport, probed: &HashMap<String, Result<MediaItem, String>>) -> (Project, Vec<String>) {
    let mut warnings = e.unsupported.clone();
    let mut p = Project::new(if e.name.is_empty() { "Imported VEGAS Project" } else { &e.name });
    let d = ProjectSettings::default();
    let nz = |v: u32, d: u32| if v > 0 { v } else { d };
    // VEGAS has 29.97002997...; Milan's rate list has 29.97.
    let fps = if e.project.fps > 0.0 { (e.project.fps * 1000.0).round() / 1000.0 } else { d.frame_rate };
    p.settings = ProjectSettings { width: nz(e.project.w, d.width), height: nz(e.project.h, d.height), frame_rate: fps, sample_rate: nz(e.project.sample_rate, d.sample_rate), ..d };
    let snap = |t: f64| edit::quantize(t.max(0.0), fps);
    let span = |s: &VSpan| {
        let (a, b) = if s.length < 0.0 { (s.t + s.length, s.t) } else { (s.t, s.t + s.length) };
        (snap(a), snap(b))
    };
    (p.timeline.video_tracks, p.timeline.audio_tracks) = (Vec::new(), Vec::new());
    let mut groups: HashMap<i64, String> = HashMap::new();
    for t in &e.tracks {
        let video = match t.kind.as_str() {
            "video" => true,
            "audio" => false,
            k => {
                warnings.push(format!("A track of kind \"{k}\" was skipped"));
                continue;
            }
        };
        let (kind, word) = if video { (TrackKind::Video, "Video") } else { (TrackKind::Audio, "Audio") };
        let name = if t.name.is_empty() { format!("{word} {}", p.timeline.tracks(video).len() + 1) } else { t.name.clone() };
        let mut tr = Track::new(&name, kind);
        (tr.muted, tr.solo) = (t.mute, t.solo);
        if video {
            tr.volume = t.level.clamp(0.0, 1.0) as f32;
            tr.composite = composite(&t.mode).unwrap_or_else(|| {
                warnings.push(format!("{name}: compositing mode {} is not available; Source Alpha is used", t.mode));
                Composite::SourceAlpha
            });
        } else {
            (tr.volume, tr.pan) = (t.volume.max(0.0) as f32, t.pan.clamp(-1.0, 1.0) as f32);
            // VEGAS's own neutral chain, not the user's saved default, which would change the imported mix.
            tr.fx = crate::project::default_track_chain(None);
        }
        for env in &t.envelopes {
            let Some(k) = envelope(&env.kind, video) else {
                warnings.push(format!("{name}: the {} envelope was not imported", env.kind));
                continue;
            };
            let keys = env.points.iter().map(|pt| {
                // Milan's Mute points always switch (as edit::env_insert makes them), whatever curve VEGAS stored.
                let curve = if k == EnvKind::Mute { Curve::Hold } else { curve(&pt.curve, Curve::Linear) };
                Keyframe { t: snap(pt.t), v: edit::env_clamp(k, env_value(k, pt.v, env.neutral)), curve }
            }).collect();
            tr.envelopes.push(Envelope { kind: k, points: Keyframes { keys } });
        }
        for ev in &t.events {
            if ev.path.is_empty() {
                warnings.push(format!("{name}: an event at {:.3} s without media was skipped", ev.start));
                continue;
            }
            let id = media_id(&mut p.media_pool, probed, &ev.path, video, &mut warnings);
            let m = p.media_pool.iter().find(|m| m.id == id).expect("just added");
            let start = snap(ev.start);
            let mut c = Clip::from_media(m, start);
            c.duration = (snap(ev.start + ev.length) - start).max(1.0 / fps);
            c.rate = if ev.rate > 0.0 { ev.rate } else { 1.0 };
            // A reversed event's offset is the end of the source span it plays (edit::src_time).
            let span = c.duration * c.rate;
            let in_point = |offset: f64, reverse: bool| (offset - if reverse { span } else { 0.0 }).max(0.0);
            (c.in_point, c.reverse) = (in_point(ev.offset, ev.reverse), ev.reverse);
            c.out_point = c.in_point + c.duration * c.rate;
            if !ev.name.is_empty() {
                c.name = ev.name.clone();
            }
            (c.fade_in, c.fade_out) = (snap(ev.fade_in), snap(ev.fade_out));
            (c.fade_in_curve, c.fade_out_curve) = (curve(&ev.fade_in_curve, Curve::Smooth), curve(&ev.fade_out_curve, Curve::Smooth));
            c.clamp_fades();
            (c.muted, c.locked) = (ev.mute, ev.locked);
            if video {
                c.set_opacity(ev.gain.clamp(0.0, 1.0) as f32);
            } else {
                c.gain_db = db(ev.gain);
            }
            c.group = ev.group.map(|g| groups.entry(g).or_insert_with(|| uuid::Uuid::new_v4().to_string()).clone());
            if ev.stream > 0 {
                warnings.push(format!("{name}: the event at {:.3} s uses stream {} of {}; Milan Pro plays the first", ev.start, ev.stream + 1, c.name));
            }
            for tk in ev.takes.iter().filter(|tk| !tk.path.is_empty()) {
                let media_id = media_id(&mut p.media_pool, probed, &tk.path, video, &mut warnings);
                let name = if tk.name.is_empty() { Path::new(&tk.path).file_name().map_or(tk.path.clone(), |n| n.to_string_lossy().into_owned()) } else { tk.name.clone() };
                // shortcut: a take plays in the event's direction; a take reversed differently from it plays the other way.
                c.takes.push(Take { media_id, in_point: in_point(tk.offset, tk.reverse), name });
            }
            if let Some([sw, sh]) = ev.src_size.filter(|[w, h]| *w > 0.0 && *h > 0.0) {
                let keys: Vec<_> = ev.pan_crop.iter().map(|k| Keyframe { t: snap(k.t), v: pan_crop(k, (sw, sh)), curve: curve(&k.curve, Curve::Smooth) }).collect();
                // VEGAS gives every video event one full-frame key; Milan stores none for that.
                if keys.iter().any(|k| k.v != PanCrop::default()) {
                    c.pan_crop = Keyframes { keys };
                }
            }
            tr.clips.push(c);
        }
        p.timeline.tracks_mut(video).push(tr);
    }
    let tl = &mut p.timeline;
    tl.markers = e.markers.iter().map(|m| Marker { position: snap(m.t), name: m.name.clone(), color: MARKER_COLOR }).collect();
    tl.regions = e.regions.iter().map(|r| {
        let (start, end) = span(r);
        Region { start, end, name: r.name.clone() }
    }).collect();
    tl.loop_region = e.loop_region.as_ref().map(span).filter(|(a, b)| b > a);
    // Not saved yet: the save prompt protects it, and Save asks where (never over the .veg).
    p.modified = true;
    (p, warnings)
}

/// Absolute media paths stored as UTF-16LE text in a .veg (either byte alignment), in file order, without repeats.
/// Candidates only: the caller keeps the ones that exist.
pub fn scan_veg(buf: &[u8]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for align in 0..2 {
        let units: Vec<u16> = buf.get(align..).unwrap_or_default().chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        let text = String::from_utf16_lossy(&units);
        // Runs of path characters; a path ends at a control character or one Windows forbids.
        for run in text.split(|c: char| c.is_control() || c == '\u{fffd}' || "<>\"|?*".contains(c)) {
            let chars: Vec<(usize, char)> = run.char_indices().collect();
            for (k, &(i, c)) in chars.iter().enumerate() {
                let next = |n: usize| chars.get(k + n).map(|x| x.1);
                let drive = c.is_ascii_alphabetic() && next(1) == Some(':') && next(2) == Some('\\');
                let unc = c == '\\' && next(1) == Some('\\') && k.checked_sub(1).and_then(|j| chars.get(j)).is_none_or(|x| x.1 != '\\');
                if !(drive || unc) {
                    continue;
                }
                // The shortest stretch from here that ends in a media extension (and not inside a longer word).
                let rest = &run[i..];
                let end = rest.char_indices().filter(|&(_, c)| c == '.').find_map(|(dot, _)| {
                    let ext: String = rest[dot + 1..].chars().take_while(char::is_ascii_alphanumeric).collect();
                    let known = crate::media::classify_extension(&ext.to_ascii_lowercase()) != MediaType::Unknown;
                    (known && !ext.is_empty()).then_some(dot + 1 + ext.len())
                });
                // A second drive colon means the stretch ran into another path.
                if let Some(path) = end.map(|e| &rest[..e]).filter(|p| !p[2..].contains(':')) {
                    if !out.iter().any(|o| o == path) {
                        out.push(path.to_string());
                    }
                }
            }
        }
    }
    out
}

/// What a finished import gives the app.
pub enum Imported {
    /// From a script export: the project (replaces the open one through the save prompt) and its warnings.
    Project(Project, Vec<String>),
    /// From a .veg: media files to add to Project Media, and the ones not found.
    Media(Vec<PathBuf>, Vec<String>),
}

/// A VEGAS import on its own thread: reading, probing every media file, building the project.
#[derive(Default)]
pub struct VegasImport {
    rx: Option<Receiver<Result<Imported, String>>>,
}

impl VegasImport {
    /// Start importing `path` (a .milan.json or a .veg); a running import is dropped.
    pub fn start(&mut self, ctx: &egui::Context, path: PathBuf) {
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(import(&path).map_err(|e| format!("{}: {e}", path.display())));
            ctx.request_repaint();
        });
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

fn import(path: &Path) -> anyhow::Result<Imported> {
    let bytes = std::fs::read(path)?;
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("veg")) {
        let (found, missing): (Vec<_>, Vec<_>) = scan_veg(&bytes).into_iter().map(PathBuf::from).partition(|p| p.is_file());
        return Ok(Imported::Media(found, missing.iter().map(|p| format!("Not found: {}", p.display())).collect()));
    }
    let mut e = read_vegas_json(std::str::from_utf8(&bytes)?)?;
    if e.name.is_empty() {
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        e.name = stem.trim_end_matches(".milan").to_string();
    }
    let mut paths: Vec<String> = Vec::new();
    for ev in e.tracks.iter().flat_map(|t| &t.events) {
        for p in std::iter::once(&ev.path).chain(ev.takes.iter().map(|t| &t.path)) {
            if !p.is_empty() && !paths.contains(p) {
                paths.push(p.clone());
            }
        }
    }
    let (p, warnings) = to_project(&e, &probe_all(&paths));
    Ok(Imported::Project(p, warnings))
}

/// Probe `paths`, eight ffprobes at a time.
fn probe_all(paths: &[String]) -> HashMap<String, Result<MediaItem, String>> {
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

    const FIXTURE: &str = r#"{
        "format": "milan-vegas", "version": 1, "name": "Show", "written_by": "a later script",
        "project": { "w": 1280, "h": 720, "fps": 29.97002997002997, "sample_rate": 44100, "pixel_aspect": 1.0 },
        "tracks": [
            { "kind": "video", "name": "", "mute": true, "level": 0.5, "mode": "Screen",
              "envelopes": [
                { "kind": "Composite", "neutral": 1, "points": [ { "t": 0, "v": 1, "curve": "Linear" }, { "t": 2.01, "v": 0.25, "curve": "None" } ] },
                { "kind": "FadeToColor", "neutral": 0, "points": [ { "t": 0, "v": -0.5 } ] } ],
              "events": [ { "start": 1.01, "length": 2.0, "offset": 3.5, "rate": 2, "path": "C:\\m\\a.mp4", "name": "Take A",
                  "fade_in": 0.5, "fade_out": 0.25, "fade_in_curve": "Fast", "fade_out_curve": "Sharp", "gain": 0.5,
                  "mute": true, "locked": true, "group": 0, "takes": [ { "path": "C:\\m\\b.mp4", "offset": 1.0, "name": "" } ],
                  "velocity": [] },
                { "start": 5, "length": 1, "offset": 10, "reverse": true, "path": "C:\\m\\a.mp4", "src_size": [1920, 1080],
                  "takes": [ { "path": "C:\\m\\b.mp4", "offset": 4, "reverse": true } ],
                  "pan_crop": [ { "t": 0, "tl": [480, 270], "tr": [1440, 270], "bl": [480, 810], "curve": "Smooth" },
                                { "t": 0.5, "tl": [1920, 0], "tr": [0, 0], "bl": [1920, 1080], "curve": "Hold" } ] },
                { "start": 7, "length": 1, "path": "C:\\m\\a.mp4", "src_size": [1920, 1080],
                  "pan_crop": [ { "t": 0, "tl": [0, 0], "tr": [1920, 0], "bl": [0, 1080], "curve": "Smooth" } ] } ] },
            { "kind": "audio", "name": "Dialog", "solo": true, "volume": 0.5, "pan": -0.25,
              "envelopes": [
                { "kind": "Volume", "neutral": 1, "points": [ { "t": 0, "v": 0.5, "curve": "Smooth" }, { "t": 1, "v": 4 } ] },
                { "kind": "Mute", "neutral": 1, "points": [ { "t": 0, "v": 1, "curve": "Linear" }, { "t": 1, "v": 0, "curve": "None" } ] },
                { "kind": "Pan", "neutral": 0, "points": [ { "t": 0, "v": 0.5 } ] } ],
              "events": [
                { "start": 1.01, "length": 2.0, "offset": 3.5, "path": "C:\\m\\a.mp4", "gain": 0.5, "group": 0, "stream": 1 },
                { "start": 4, "length": 1, "path": "C:\\m\\gone.wav" } ] },
            { "kind": "midi", "events": [] }
        ],
        "markers": [ { "t": 0.5, "name": "Intro" } ],
        "regions": [ { "t": 10, "length": -2, "name": "R" } ],
        "loop": { "t": 1, "length": 3 },
        "unsupported": [ "Video 1: event FX at 1.010 s (Gaussian Blur)" ]
    }"#;

    fn probed() -> HashMap<String, Result<MediaItem, String>> {
        let item = |path: &str, t| Ok(MediaItem { id: uuid::Uuid::new_v4().to_string(), path: path.into(), name: path[5..].into(), media_type: t, duration: 60.0, ..Default::default() });
        HashMap::from([
            (r"C:\m\a.mp4".to_string(), item(r"C:\m\a.mp4", MediaType::Video)),
            (r"C:\m\b.mp4".to_string(), item(r"C:\m\b.mp4", MediaType::Video)),
            (r"C:\m\gone.wav".to_string(), Err("file not found".to_string())),
        ])
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    /// R7: every VEGAS compositing mode but Custom (its compositor is not exported) maps onto Milan's.
    #[test]
    fn vegas_compositing_modes_map() {
        use Composite::*;
        for (name, mode) in [("SrcAlpha3D", ThreeD), ("Cut", Cut), ("HardLight", HardLight), ("Dodge", Dodge), ("Burn", Burn), ("DifferenceSquared", DifferenceSquared), ("Multiply", Multiply)] {
            assert_eq!(composite(name), Some(mode), "{name}");
        }
        assert_eq!(composite("Custom"), None);
    }

    #[test]
    fn fixture_maps_onto_the_project() {
        let e = read_vegas_json(FIXTURE).unwrap();
        let (p, w) = to_project(&e, &probed());
        assert_eq!((p.name.as_str(), p.modified, p.path.is_none()), ("Show", true, true));
        assert_eq!(p.settings, ProjectSettings { width: 1280, height: 720, frame_rate: 29.97, sample_rate: 44100, ..Default::default() });
        let f = |n: f64| n * 1001.0 / 30000.0;
        let tl = &p.timeline;
        assert_eq!((tl.video_tracks.len(), tl.audio_tracks.len()), (1, 1));

        let v = &tl.video_tracks[0];
        assert_eq!((v.name.as_str(), v.muted, v.volume, v.composite), ("Video 1", true, 0.5, Composite::Screen));
        assert_eq!(v.envelopes.len(), 2);
        let k = &v.envelopes[0].points.keys;
        assert_eq!(v.envelopes[0].kind, EnvKind::CompositeLevel);
        // R4a: Fade to Color comes across (towards the bottom colour below 0).
        assert_eq!((v.envelopes[1].kind, v.envelopes[1].points.keys[0].v), (EnvKind::FadeToColor, -0.5));
        assert!(close(k[1].t, f(60.0)) && k[1].v == 0.25 && k[1].curve == Curve::Hold && k[0].curve == Curve::Linear);
        let c = &v.clips[0];
        assert!(close(c.position, f(30.0)) && close(c.duration, f(60.0)), "{} {}", c.position, c.duration);
        assert!(close(c.in_point, 3.5) && c.rate == 2.0 && close(c.out_point, 3.5 + f(60.0) * 2.0));
        assert!(close(c.fade_in, f(15.0)) && close(c.fade_out, f(7.0)));
        assert_eq!((c.fade_in_curve, c.fade_out_curve, c.muted, c.locked), (Curve::Fast, Curve::Sharp, true, true));
        assert_eq!((c.name.as_str(), c.opacity()), ("Take A", 0.5));
        let media = |id: &str| p.media_pool.iter().find(|m| m.id == id).unwrap();
        assert_eq!(media(c.media_id.as_deref().unwrap()).path, PathBuf::from(r"C:\m\a.mp4"));
        assert_eq!(c.takes.len(), 1);
        assert_eq!((media(&c.takes[0].media_id).path.to_str(), c.takes[0].in_point, c.takes[0].name.as_str()), (Some(r"C:\m\b.mp4"), 1.0, "b.mp4"));
        assert!(c.pan_crop.keys.is_empty() && !c.reverse);

        // Reversed: the offset is where playback starts, the end of the source span.
        let r = &v.clips[1];
        assert!(r.reverse && close(r.duration, f(30.0)) && close(r.in_point, 10.0 - f(30.0)) && close(r.takes[0].in_point, 4.0 - f(30.0)));
        assert!(close(edit::src_time(r, r.position), 10.0));
        let pc = &r.pan_crop.keys;
        assert_eq!((pc[0].v, pc[0].curve), (PanCrop { w: 0.5, h: 0.5, ..Default::default() }, Curve::Smooth));
        assert!(close(pc[1].t, f(15.0)) && pc[1].curve == Curve::Hold);
        let flip = pc[1].v;
        assert!((flip.cx, flip.cy, flip.w, flip.h) == (0.5, 0.5, -1.0, 1.0) && flip.rot.abs() < 1e-4, "horizontal flip: {flip:?}");
        // A full-frame key alone is no Pan/Crop.
        assert!(v.clips[2].pan_crop.keys.is_empty());

        let a = &tl.audio_tracks[0];
        assert_eq!((a.name.as_str(), a.solo, a.volume, a.pan), ("Dialog", true, 0.5, -0.25));
        let env = |k| &a.envelopes.iter().find(|e| e.kind == k).unwrap().points.keys;
        assert!((env(EnvKind::Volume)[0].v + 6.0206).abs() < 1e-3 && env(EnvKind::Volume)[0].curve == Curve::Smooth);
        // +12 dB is above Milan's +6 dB ceiling.
        assert_eq!(env(EnvKind::Volume)[1].v, 6.0);
        // Mute switches, whatever curve VEGAS stored.
        assert_eq!(env(EnvKind::Mute).iter().map(|k| (k.v, k.curve)).collect::<Vec<_>>(), [(1.0, Curve::Hold), (0.0, Curve::Hold)]);
        assert_eq!(env(EnvKind::Pan)[0].v, 0.5);
        let (a0, a1) = (&a.clips[0], &a.clips[1]);
        assert!((a0.gain_db + 6.0206).abs() < 1e-3 && a0.group.is_some() && a0.group == c.group);
        // One pool item per file, shared by the video and audio events; the missing file is an offline item.
        assert_eq!(a0.media_id, c.media_id);
        assert_eq!(p.media_pool.len(), 3);
        let gone = media(a1.media_id.as_deref().unwrap());
        assert_eq!((gone.name.as_str(), gone.media_type), ("gone.wav", MediaType::Audio));
        assert!(close(a1.position, f(120.0)) && close(a1.duration, f(30.0)));

        assert_eq!(tl.markers.len(), 1);
        assert!(close(tl.markers[0].position, f(15.0)) && tl.markers[0].name == "Intro");
        assert!(close(tl.regions[0].start, f(240.0)) && close(tl.regions[0].end, f(300.0)) && tl.regions[0].name == "R");
        let (la, lb) = tl.loop_region.unwrap();
        assert!(close(la, f(30.0)) && close(lb, f(120.0)));

        let has = |s: &str| w.iter().any(|x| x.contains(s));
        assert!(has("Gaussian Blur") && !has("FadeToColor") && has("midi") && has("stream 2"), "{w:?}");
        assert!(has(r"Media offline: C:\m\gone.wav (file not found)"), "{w:?}");
        assert_eq!(w.len(), 4, "{w:?}");
    }

    #[test]
    fn pan_crop_corners_land_on_the_frame_corners() {
        // A frame the crop's size: VEGAS's top-left, top-right and bottom-left corners must show at the frame's.
        let (sw, sh) = (1920.0, 1080.0);
        let cases = [
            ([480.0, 270.0], [1440.0, 270.0], [480.0, 810.0]),
            ([1920.0, 0.0], [0.0, 0.0], [1920.0, 1080.0]),    // flipped horizontally
            ([0.0, 1080.0], [1920.0, 1080.0], [0.0, 0.0]),    // flipped vertically
            ([1000.0, 200.0], [1000.0, 740.0], [40.0, 200.0]), // turned 90 degrees
            ([900.0, 100.0], [1500.0, 700.0], [500.0, 500.0]), // turned 45 degrees
            ([1000.0, 900.0], [400.0, 900.0], [1000.0, 300.0]), // turned 180 degrees
        ];
        for (tl, tr, bl) in cases {
            let pc = pan_crop(&VPanKey { tl, tr, bl, ..Default::default() }, (sw, sh));
            let size = (((tr[0] - tl[0]) as f64).hypot(tr[1] - tl[1]), ((bl[0] - tl[0]) as f64).hypot(bl[1] - tl[1]));
            let a = crate::engine::plan::place(pc, (sw, sh), size, true);
            for (p, want) in [(tl, (0.0, 0.0)), (tr, (1.0, 0.0)), (bl, (0.0, 1.0))] {
                let got = a.apply(p[0] / sw, p[1] / sh);
                assert!((got.0 - want.0).abs() < 1e-4 && (got.1 - want.1).abs() < 1e-4, "{tl:?} {tr:?} {bl:?}: {pc:?} put {p:?} at {got:?}");
            }
            assert!(pc.rot.abs() <= 90.0, "{pc:?}");
        }
    }

    #[test]
    fn timeline_times_land_on_29_97_frames() {
        let (p, _) = to_project(&read_vegas_json(FIXTURE).unwrap(), &probed());
        let on_grid = |t: f64| ((t * 30000.0 / 1001.0) - (t * 30000.0 / 1001.0).round()).abs() < 1e-6;
        let tl = &p.timeline;
        for c in tl.video_tracks.iter().chain(&tl.audio_tracks).flat_map(|t| &t.clips) {
            assert!(on_grid(c.position) && on_grid(c.position + c.duration) && on_grid(c.fade_in) && on_grid(c.fade_out));
        }
        assert!(tl.markers.iter().all(|m| on_grid(m.position)) && tl.regions.iter().all(|r| on_grid(r.start) && on_grid(r.end)));
    }

    #[test]
    fn wrong_format_is_an_error() {
        assert!(read_vegas_json(r#"{"format":"something-else","tracks":[]}"#).is_err());
        assert!(read_vegas_json("{}").is_err());
        assert!(read_vegas_json("not json").is_err());
        // The script writes no BOM, but an editor that re-saves the file may add one.
        let e = read_vegas_json("\u{feff}{\"format\":\"milan-vegas\"}").unwrap();
        let (p, w) = to_project(&e, &HashMap::new());
        assert!(w.is_empty() && p.timeline.video_tracks.is_empty() && p.settings == ProjectSettings::default());
    }

    #[test]
    fn veg_scan_finds_utf16_paths_only() {
        let utf16 = |s: &str| s.encode_utf16().flat_map(u16::to_le_bytes).chain([0, 0]).collect::<Vec<u8>>();
        let mut buf = vec![0x13, 0x00, 0x7f, 0x02];
        buf.extend(utf16(r"C:\Media\clip one.mp4"));
        buf.extend(utf16("not a path.mp4"));
        buf.extend(utf16(r"D:\docs\notes.txt"));
        buf.extend(utf16(r"C:\Media\clip.mp4x"));
        // 8-bit text is not read; its odd length puts the strings after it at odd offsets.
        buf.extend(br"E:\ascii.mp4 ");
        buf.extend(utf16(r"\\nas\share\song.WAV"));
        buf.extend(utf16(r"C:\Media\clip one.mp4"));
        buf.extend(utf16(r"F:\a.mp4 F:\b.wav"));
        assert_eq!(scan_veg(&buf), [r"C:\Media\clip one.mp4", r"\\nas\share\song.WAV", r"F:\a.mp4", r"F:\b.wav"]);
        assert!(scan_veg(&[]).is_empty() && scan_veg(&[0x41]).is_empty());
    }

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
        let from_veg = import(&dir.join("Show.veg"));
        let from_json = import(&dir.join("Show.milan.json"));
        std::fs::remove_dir_all(&dir).unwrap();
        let Ok(Imported::Media(found, missing)) = from_veg else { panic!("not media") };
        assert_eq!((found, missing), (vec![clip], vec![format!("Not found: {}", gone.display())]));
        let Ok(Imported::Project(p, w)) = from_json else { panic!("not a project") };
        assert_eq!((p.name.as_str(), p.timeline.audio_tracks[0].clips.len()), ("Show", 1));
        assert_eq!(w, [format!("Media offline: {} (file not found)", gone.display())]);
    }
}
