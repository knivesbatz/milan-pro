//! R28: OpenTimelineIO (File › Import / Export › OpenTimelineIO...). Hand-written serde structs for the schemas an
//! editing timeline uses (`Timeline.1`, `Stack.1`, `Track.1`, `Clip.2` (and `Clip.1`), `Gap.1`, `Transition.1`,
//! `Marker.2`, `ExternalReference.1`, `MissingReference.1`, `RationalTime.1`, `TimeRange.1`, `LinearTimeWarp.1`,
//! `FreezeFrame.1`, `Effect.1`); other schemas are skipped with a warning.
//!
//! Every Milan object goes out twice: as the OTIO object other tools read (clips with their media, gaps,
//! SMPTE dissolves for overlaps, time warps for speed changes, markers) and, under `metadata["milan"]`, as the Milan
//! struct itself (the project without its tracks on the Timeline, each Track without its events, each event, each
//! transition). Reading a file with that metadata rebuilds the project from it exactly (`lossless_round_trip`); a
//! file from another tool is read from the OTIO objects, which is where fidelity is lost (`docs/INTERCHANGE.md`).
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use crate::edit;
use crate::interchange::cutlist::{DISSOLVE, plugin_name};
use crate::project::*;

// ---- Schema ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RationalTime {
    #[serde(rename = "OTIO_SCHEMA", default = "s_rt")]
    schema: String,
    pub rate: f64,
    pub value: f64,
}

fn s_rt() -> String {
    "RationalTime.1".into()
}

impl RationalTime {
    fn new(seconds: f64, rate: f64) -> Self {
        Self { schema: s_rt(), rate, value: seconds * rate }
    }

    pub fn seconds(&self) -> f64 {
        if self.rate > 0.0 { self.value / self.rate } else { 0.0 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRange {
    #[serde(rename = "OTIO_SCHEMA", default = "s_tr")]
    schema: String,
    pub start_time: RationalTime,
    pub duration: RationalTime,
}

fn s_tr() -> String {
    "TimeRange.1".into()
}

impl TimeRange {
    fn new(start: f64, duration: f64, rate: f64) -> Self {
        Self { schema: s_tr(), start_time: RationalTime::new(start, rate), duration: RationalTime::new(duration, rate) }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkerNode {
    #[serde(rename = "OTIO_SCHEMA", default = "s_marker")]
    schema: String,
    #[serde(default)]
    pub metadata: Map<String, Value>,
    #[serde(default)]
    pub name: String,
    #[serde(default = "green")]
    pub color: String,
    pub marked_range: TimeRange,
    #[serde(default)]
    pub comment: String,
}

fn s_marker() -> String {
    "Marker.2".into()
}

fn green() -> String {
    "GREEN".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "OTIO_SCHEMA")]
pub enum MediaRef {
    #[serde(rename = "ExternalReference.1")]
    External {
        #[serde(default)]
        metadata: Map<String, Value>,
        #[serde(default)]
        name: String,
        #[serde(default)]
        available_range: Option<TimeRange>,
        #[serde(default)]
        available_image_bounds: Value,
        #[serde(default)]
        target_url: String,
    },
    #[serde(rename = "MissingReference.1")]
    Missing {
        #[serde(default)]
        metadata: Map<String, Value>,
        #[serde(default)]
        name: String,
        #[serde(default)]
        available_range: Option<TimeRange>,
        #[serde(default)]
        available_image_bounds: Value,
    },
    /// Image sequences, generators and schemas from later versions: read as missing media.
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "OTIO_SCHEMA")]
pub enum Effect {
    #[serde(rename = "LinearTimeWarp.1")]
    LinearTimeWarp {
        #[serde(default)]
        metadata: Map<String, Value>,
        #[serde(default)]
        name: String,
        #[serde(default = "ltw")]
        effect_name: String,
        time_scalar: f64,
    },
    #[serde(rename = "FreezeFrame.1")]
    FreezeFrame {
        #[serde(default)]
        metadata: Map<String, Value>,
        #[serde(default)]
        name: String,
        #[serde(default = "ff")]
        effect_name: String,
        #[serde(default)]
        time_scalar: f64,
    },
    #[serde(rename = "Effect.1")]
    Effect {
        #[serde(default)]
        metadata: Map<String, Value>,
        #[serde(default)]
        name: String,
        #[serde(default)]
        effect_name: String,
    },
    #[serde(other)]
    Other,
}

fn ltw() -> String {
    "LinearTimeWarp".into()
}

fn ff() -> String {
    "FreezeFrame".into()
}

/// A Track's or Stack's children.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "OTIO_SCHEMA")]
pub enum Child {
    #[serde(rename = "Clip.2", alias = "Clip.1")]
    Clip(ClipNode),
    #[serde(rename = "Gap.1")]
    Gap(GapNode),
    #[serde(rename = "Transition.1")]
    Transition(TransitionNode),
    #[serde(rename = "Stack.1")]
    Stack(Composition),
    #[serde(rename = "Track.1")]
    Track(Composition),
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClipNode {
    #[serde(default)]
    pub metadata: Map<String, Value>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub source_range: Option<TimeRange>,
    #[serde(default)]
    pub effects: Vec<Effect>,
    #[serde(default)]
    pub markers: Vec<MarkerNode>,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Clip.2: several references keyed by name, one active. Clip.1 had `media_reference` alone.
    #[serde(default)]
    pub media_references: BTreeMap<String, MediaRef>,
    #[serde(default = "default_media")]
    pub active_media_reference_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_reference: Option<MediaRef>,
}

fn yes() -> bool {
    true
}

fn default_media() -> String {
    "DEFAULT_MEDIA".into()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GapNode {
    #[serde(default)]
    pub metadata: Map<String, Value>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub source_range: Option<TimeRange>,
    #[serde(default)]
    pub effects: Vec<Effect>,
    #[serde(default)]
    pub markers: Vec<MarkerNode>,
    #[serde(default = "yes")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionNode {
    #[serde(default)]
    pub metadata: Map<String, Value>,
    #[serde(default)]
    pub name: String,
    #[serde(default = "dissolve")]
    pub transition_type: String,
    pub in_offset: RationalTime,
    pub out_offset: RationalTime,
}

fn dissolve() -> String {
    "SMPTE_Dissolve".into()
}

/// Stack.1 and Track.1 share their fields; a Track has `kind` ("Video" / "Audio").
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Composition {
    #[serde(default)]
    pub metadata: Map<String, Value>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub source_range: Option<TimeRange>,
    #[serde(default)]
    pub effects: Vec<Effect>,
    #[serde(default)]
    pub markers: Vec<MarkerNode>,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub children: Vec<Child>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Timeline {
    #[serde(rename = "OTIO_SCHEMA", default = "s_timeline")]
    schema: String,
    #[serde(default)]
    pub metadata: Map<String, Value>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub global_start_time: Option<RationalTime>,
    pub tracks: Composition,
}

fn s_timeline() -> String {
    "Timeline.1".into()
}

pub fn parse(text: &str) -> anyhow::Result<Timeline> {
    let text = text.trim_start_matches('\u{feff}');
    let v: Value = serde_json::from_str(text)?;
    let schema = v.get("OTIO_SCHEMA").and_then(Value::as_str).unwrap_or("");
    anyhow::ensure!(schema.starts_with("Timeline."), "not an OpenTimelineIO timeline (OTIO_SCHEMA {schema:?})");
    Ok(serde_json::from_value(v)?)
}

// ---- File URLs ----

/// `file:///C:/m/a%20b.mp4` for a path (what OTIO's adapters write).
pub fn file_url(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    let mut out = String::from("file://");
    if !s.starts_with('/') {
        out.push('/');
    }
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A path from a `file:` URL (or a plain path); relative ones are taken from `dir`.
pub fn url_path(url: &str, dir: Option<&Path>) -> String {
    let mut s = url.trim().to_string();
    if let Some(rest) = s.strip_prefix("file://") {
        let rest = rest.strip_prefix("localhost").unwrap_or(rest);
        s = percent_decode(rest);
        // "/C:/m/a.mp4" is a Windows path.
        if s.len() > 2 && s.as_bytes()[0] == b'/' && s.as_bytes()[2] == b':' {
            s.remove(0);
        }
    }
    if cfg!(windows) {
        s = s.replace('/', "\\");
    }
    let p = Path::new(&s);
    if !p.is_absolute() {
        if let Some(d) = dir {
            return d.join(p).display().to_string();
        }
    }
    s
}

// ---- Export ----

const COLORS: [(&str, [u8; 3]); 11] = [
    ("PINK", [255, 105, 180]),
    ("RED", [255, 0, 0]),
    ("ORANGE", [255, 140, 0]),
    ("YELLOW", [255, 220, 0]),
    ("GREEN", [0, 200, 0]),
    ("CYAN", [0, 220, 220]),
    ("BLUE", [0, 90, 255]),
    ("PURPLE", [140, 0, 200]),
    ("MAGENTA", [255, 0, 255]),
    ("BLACK", [0, 0, 0]),
    ("WHITE", [255, 255, 255]),
];

fn color_name(c: [u8; 3]) -> String {
    let dist = |k: [u8; 3]| (0..3).map(|i| (k[i] as i32 - c[i] as i32).pow(2)).sum::<i32>();
    COLORS.iter().min_by_key(|(_, k)| dist(*k)).map(|(n, _)| n.to_string()).unwrap_or_else(green)
}

fn color_rgb(name: &str) -> [u8; 3] {
    COLORS.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map_or([0xDB, 0x8E, 0x57], |(_, k)| *k)
}

fn milan(v: impl Serialize) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("milan".into(), serde_json::to_value(v).unwrap_or(Value::Null));
    m
}

fn media_ref(m: &MediaItem, rate: f64, extra: Value) -> MediaRef {
    let mut metadata = Map::new();
    metadata.insert("milan".into(), extra);
    let tc = m.timecode.unwrap_or(0.0);
    let range = Some(TimeRange::new(tc, m.duration, rate));
    if m.path.as_os_str().is_empty() {
        MediaRef::Missing { metadata, name: m.name.clone(), available_range: range, available_image_bounds: Value::Null }
    } else {
        MediaRef::External { metadata, name: m.name.clone(), available_range: range, available_image_bounds: Value::Null, target_url: file_url(&m.path) }
    }
}

/// The project as an OTIO timeline, and what other tools' readers will not see in it (the Milan metadata keeps it).
pub fn export(p: &Project) -> (Timeline, Vec<String>) {
    let mut notes: Vec<String> = Vec::new();
    let fps = p.settings.frame_rate;
    let rate = exact_fps(fps);
    let media = |id: &str| p.media_pool.iter().find(|m| m.id == id);
    let mut shell = p.clone();
    (shell.timeline.video_tracks, shell.timeline.audio_tracks) = (Vec::new(), Vec::new());
    let mut tracks: Vec<Child> = Vec::new();
    // OTIO stacks bottom to top; Milan's first video track is the top one.
    let order = p.timeline.video_tracks.iter().rev().map(|t| (true, t)).chain(p.timeline.audio_tracks.iter().map(|t| (false, t)));
    for (video, tr) in order {
        let at = tr.name.clone();
        let mut clips: Vec<&Clip> = tr.clips.iter().collect();
        clips.sort_by(|a, b| a.position.total_cmp(&b.position));
        // Cut points: an overlap with the previous event is a dissolve centred on the overlap's middle; its halves
        // are the transition's in and out offsets and the events' ranges end and start at the middle.
        let n = clips.len();
        let mut starts: Vec<f64> = clips.iter().map(|c| c.position).collect();
        let mut ends: Vec<f64> = clips.iter().map(|c| c.position + c.duration).collect();
        let mut trans: Vec<Option<(f64, f64)>> = vec![None; n];
        for i in 1..n {
            let (prev_end, start) = (ends[i - 1], starts[i]);
            if prev_end > start + 1e-9 {
                let ov = prev_end - start;
                let mid = start + ov / 2.0;
                if mid > starts[i - 1] + 1e-9 && mid < ends[i] - 1e-9 {
                    ends[i - 1] = mid;
                    starts[i] = mid;
                    trans[i] = Some((ov / 2.0, ov / 2.0));
                } else {
                    notes.push(format!("{at}: the event at {:.3} s overlaps more than one event; cut at its start for other readers", clips[i].position));
                    ends[i - 1] = start;
                }
            }
        }
        let mut children: Vec<Child> = Vec::new();
        let mut t = 0.0;
        for (i, c) in clips.iter().enumerate() {
            if starts[i] > t + 1e-9 {
                children.push(Child::Gap(GapNode { source_range: Some(TimeRange::new(0.0, starts[i] - t, rate)), name: "Gap".into(), ..Default::default() }));
            }
            if let Some((a, b)) = trans[i] {
                let tr = c.transition.clone().unwrap_or(Transition { plugin: DISSOLVE.into(), ..Default::default() });
                let name = plugin_name(&tr.plugin);
                children.push(Child::Transition(TransitionNode { metadata: milan(&tr), name, transition_type: dissolve(), in_offset: RationalTime::new(a, rate), out_offset: RationalTime::new(b, rate) }));
            }
            let dur = ends[i] - starts[i];
            let span = |m: Option<&MediaItem>| m.map_or(0.0, |m| m.timecode.unwrap_or(0.0) + m.subclip.as_ref().map_or(0.0, |s| s.start));
            let m = c.media_id.as_deref().and_then(media);
            let src_start = span(m) + c.in_point + (starts[i] - c.position) * c.rate;
            let mut node = ClipNode { metadata: milan(c), name: c.name.clone(), source_range: Some(TimeRange::new(src_start, dur, rate)), enabled: !c.muted, ..Default::default() };
            match m {
                Some(m) => {
                    node.media_references.insert(default_media(), media_ref(m, rate, json!({ "media_id": m.id })));
                }
                None => {
                    node.media_references.insert(default_media(), MediaRef::Missing { metadata: Map::new(), name: c.name.clone(), available_range: None, available_image_bounds: Value::Null });
                    if c.adjustment {
                        notes.push(format!("{at}: the adjustment event at {:.3} s has no media for other readers", c.position));
                    }
                }
            }
            if m.is_some_and(|m| m.generator.is_some()) {
                notes.push(format!("{at}: generated media at {:.3} s is a missing reference for other readers", c.position));
            }
            for (k, tk) in c.takes.iter().enumerate() {
                if let Some(tm) = media(&tk.media_id) {
                    node.media_references.insert(format!("TAKE_{}", k + 1), media_ref(tm, rate, json!({ "media_id": tm.id, "in_point": tk.in_point, "name": tk.name })));
                }
            }
            let freeze = c.envelopes.iter().find(|e| e.kind == EnvKind::Velocity).is_some_and(|e| !e.points.keys.is_empty() && e.points.keys.iter().all(|k| k.v == 0.0));
            if freeze {
                node.effects.push(Effect::FreezeFrame { metadata: Map::new(), name: "Freeze Frame".into(), effect_name: ff(), time_scalar: 0.0 });
            } else if (c.rate - 1.0).abs() > 1e-9 || c.reverse {
                let scalar = c.rate * if c.reverse { -1.0 } else { 1.0 };
                node.effects.push(Effect::LinearTimeWarp { metadata: Map::new(), name: "Speed".into(), effect_name: ltw(), time_scalar: scalar });
            } else if c.envelopes.iter().any(|e| e.kind == EnvKind::Velocity) {
                notes.push(format!("{at}: the Velocity envelope at {:.3} s is Milan metadata only; other readers play normal speed", c.position));
            }
            for f in &c.fx {
                node.effects.push(Effect::Effect { metadata: milan(f), name: plugin_name(&f.plugin), effect_name: f.plugin.clone() });
            }
            if !c.fx.is_empty() {
                notes.push(format!("{at}: event FX at {:.3} s ({}) are named for other readers, not played by them", c.position, c.fx.iter().map(|f| plugin_name(&f.plugin)).collect::<Vec<_>>().join(", ")));
            }
            if c.fade_in > 0.0 || c.fade_out > 0.0 {
                notes.push(format!("{at}: the fades at {:.3} s are Milan metadata only", c.position));
            }
            children.push(Child::Clip(node));
            t = ends[i];
        }
        let mut shell_track = tr.clone();
        shell_track.clips.clear();
        if !tr.fx.is_empty() || !tr.envelopes.is_empty() || !tr.motion.is_empty() {
            notes.push(format!("{at}: track FX, envelopes and Track Motion are Milan metadata only"));
        }
        tracks.push(Child::Track(Composition { metadata: milan(&shell_track), name: tr.name.clone(), enabled: !tr.muted, children, kind: Some(if video { "Video" } else { "Audio" }.into()), ..Default::default() }));
    }
    let tl = &p.timeline;
    let mut markers: Vec<MarkerNode> = tl.markers.iter().map(|m| MarkerNode { schema: s_marker(), metadata: Map::new(), name: m.name.clone(), color: color_name(m.color), marked_range: TimeRange::new(m.position, 0.0, rate), comment: String::new() }).collect();
    markers.extend(tl.regions.iter().map(|r| MarkerNode { schema: s_marker(), metadata: Map::new(), name: r.name.clone(), color: green(), marked_range: TimeRange::new(r.start, r.end - r.start, rate), comment: String::new() }));
    let stack = Composition { name: "tracks".into(), markers, children: tracks, ..Default::default() };
    let mut metadata = Map::new();
    metadata.insert("milan".into(), json!({ "version": 1, "project": shell }));
    let timeline = Timeline { schema: s_timeline(), metadata, name: p.name.clone(), global_start_time: Some(RationalTime::new(0.0, rate)), tracks: stack };
    (timeline, notes)
}

/// Write `p` as `path`; returns the notes for the report.
pub fn write(p: &Project, path: &Path) -> anyhow::Result<Vec<String>> {
    let (tl, notes) = export(p);
    std::fs::write(path, serde_json::to_string_pretty(&tl)?)?;
    Ok(notes)
}

// ---- Import ----

/// Every file the timeline's external references name (for `probe_all`); `dir` resolves relative URLs.
pub fn media_paths(tl: &Timeline, dir: Option<&Path>) -> Vec<String> {
    fn walk(children: &[Child], dir: Option<&Path>, out: &mut Vec<String>) {
        for ch in children {
            match ch {
                Child::Clip(c) => {
                    for r in c.media_references.values().chain(c.media_reference.iter()) {
                        if let MediaRef::External { target_url, .. } = r {
                            let p = url_path(target_url, dir);
                            if !out.contains(&p) {
                                out.push(p);
                            }
                        }
                    }
                }
                Child::Stack(s) | Child::Track(s) => walk(&s.children, dir, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&tl.tracks.children, dir, &mut out);
    out
}

struct Build<'a> {
    probed: &'a HashMap<String, Result<MediaItem, String>>,
    dir: Option<&'a Path>,
    fps: f64,
    warnings: Vec<String>,
    /// The Milan project the file carries (its pool holds the ids the clips' metadata names).
    shell: bool,
}

fn from_milan<T: for<'de> Deserialize<'de>>(m: &Map<String, Value>) -> Option<T> {
    serde_json::from_value(m.get("milan")?.clone()).ok()
}

/// The pool item for a media reference (added on first use).
fn ref_media(b: &mut Build, pool: &mut Vec<MediaItem>, r: Option<&MediaRef>, clip_name: &str, video: bool) -> (String, f64) {
    let (name, metadata, range, url) = match r {
        Some(MediaRef::External { name, metadata, available_range, target_url, .. }) => (name.clone(), Some(metadata), available_range.as_ref(), Some(target_url)),
        Some(MediaRef::Missing { name, metadata, available_range, .. }) => (name.clone(), Some(metadata), available_range.as_ref(), None),
        _ => (String::new(), None, None, None),
    };
    let avail_start = range.map_or(0.0, |r| r.start_time.seconds());
    // Our own export names the pool item.
    if b.shell {
        if let Some(id) = metadata.and_then(|m| m.get("milan")).and_then(|m| m.get("media_id")).and_then(Value::as_str) {
            if let Some(m) = pool.iter().find(|m| m.id == id) {
                return (m.id.clone(), m.timecode.unwrap_or(0.0) + m.subclip.as_ref().map_or(0.0, |s| s.start));
            }
        }
    }
    if let Some(url) = url.filter(|u| !u.is_empty()) {
        let path = url_path(url, b.dir);
        let id = super::vegas::media_id(pool, b.probed, &path, video, &mut b.warnings);
        let m = pool.iter_mut().find(|m| m.id == id).expect("just added");
        if m.duration <= 0.0 {
            m.duration = range.map_or(0.0, |r| r.duration.seconds());
        }
        if m.timecode.is_none() && avail_start > 0.0 {
            m.timecode = Some(avail_start);
        }
        return (id, m.timecode.unwrap_or(avail_start));
    }
    // Missing media: one offline item per name.
    let name = if !name.is_empty() { name } else if !clip_name.is_empty() { clip_name.to_string() } else { "Missing media".into() };
    if let Some(m) = pool.iter().find(|m| m.name == name && m.path.as_os_str().is_empty() && m.generator.is_none()) {
        return (m.id.clone(), avail_start);
    }
    b.warnings.push(format!("Media missing: {name} (the file has no path for it)"));
    let m = MediaItem { id: uuid::Uuid::new_v4().to_string(), name, media_type: if video { MediaType::Video } else { MediaType::Audio }, duration: range.map_or(0.0, |r| r.duration.seconds()), ..Default::default() };
    let id = m.id.clone();
    pool.push(m);
    (id, avail_start)
}

/// Lay `children` on `track` from `t0`; nested stacks flatten (their first track into this one, the rest into
/// `extra`). Returns the end time.
fn lay(b: &mut Build, pool: &mut Vec<MediaItem>, children: &[Child], t0: f64, video: bool, track: &mut Track, extra: &mut Vec<Track>, markers: &mut Vec<(f64, String, String)>) -> f64 {
    let fps = b.fps;
    let snap = |t: f64| edit::quantize(t.max(0.0), fps);
    let mut t = t0;
    let mut pending: Option<(f64, f64, Option<Transition>)> = None;
    for ch in children {
        match ch {
            Child::Gap(g) => t += g.source_range.as_ref().map_or(0.0, |r| r.duration.seconds()),
            Child::Transition(tr) => {
                let (a, o) = (tr.in_offset.seconds(), tr.out_offset.seconds());
                let plugin: Option<Transition> = from_milan(&tr.metadata);
                if tr.transition_type != "SMPTE_Dissolve" && plugin.is_none() {
                    b.warnings.push(format!("{}: the {} transition at {:.3} s plays as a crossfade", track.name, tr.transition_type, t));
                }
                pending = Some((a, o, plugin));
            }
            Child::Clip(c) => {
                let range = c.source_range.as_ref();
                let active = c.media_references.get(&c.active_media_reference_key).or_else(|| c.media_references.values().next()).or(c.media_reference.as_ref());
                let (id, avail_start) = ref_media(b, pool, active, &c.name, video);
                let m = pool.iter().find(|m| m.id == id).expect("just added").clone();
                let (src_start, dur) = match range {
                    Some(r) => (r.start_time.seconds(), r.duration.seconds()),
                    None => (avail_start, m.duration),
                };
                let own: Option<Clip> = if b.shell { from_milan(&c.metadata) } else { None };
                let mut clip = match own {
                    Some(mut own) => {
                        own.media_id = Some(id.clone());
                        own
                    }
                    None => {
                        let mut clip = Clip::from_media(&m, snap(t));
                        clip.duration = (snap(t + dur) - clip.position).max(1.0 / fps);
                        clip.in_point = (src_start - avail_start).max(0.0);
                        clip.name = if c.name.is_empty() { m.name.clone() } else { c.name.clone() };
                        clip.muted = !c.enabled;
                        for e in &c.effects {
                            match e {
                                Effect::LinearTimeWarp { time_scalar, .. } => {
                                    clip.rate = time_scalar.abs().max(0.001);
                                    clip.reverse = *time_scalar < 0.0;
                                }
                                Effect::FreezeFrame { .. } => {
                                    clip.envelopes.push(Envelope { kind: EnvKind::Velocity, points: Keyframes { keys: vec![Keyframe { t: 0.0, v: 0.0, curve: Curve::Hold }] } });
                                }
                                Effect::Effect { metadata, effect_name, name, .. } => match from_milan::<Fx>(metadata) {
                                    Some(fx) if crate::engine::plugins::find(&fx.plugin).is_some() || fx.plugin.starts_with("ofx:") => clip.fx.push(fx),
                                    _ => b.warnings.push(format!("{}: the effect {} on the event at {:.3} s is not available in Milan Pro and was left out", track.name, if name.is_empty() { effect_name } else { name }, t)),
                                },
                                Effect::Other => b.warnings.push(format!("{}: an effect of an unknown schema on the event at {:.3} s was left out", track.name, t)),
                            }
                        }
                        clip.out_point = clip.in_point + clip.duration * clip.rate;
                        // Other references are takes.
                        for (k, r) in &c.media_references {
                            if *k == c.active_media_reference_key {
                                continue;
                            }
                            let (tid, tavail) = ref_media(b, pool, Some(r), &c.name, video);
                            let tm = pool.iter().find(|m| m.id == tid).expect("just added");
                            let extra_meta = match r {
                                MediaRef::External { metadata, .. } | MediaRef::Missing { metadata, .. } => metadata.get("milan"),
                                MediaRef::Other => None,
                            };
                            let in_point = extra_meta.and_then(|m| m.get("in_point")).and_then(Value::as_f64).unwrap_or((src_start - tavail).max(0.0));
                            let name = extra_meta.and_then(|m| m.get("name")).and_then(Value::as_str).map_or(tm.name.clone(), str::to_string);
                            clip.takes.push(Take { media_id: tid, in_point, name });
                        }
                        for mk in &c.markers {
                            markers.push((t + mk.marked_range.start_time.seconds() - src_start, mk.name.clone(), mk.color.clone()));
                        }
                        clip
                    }
                };
                if let Some((a, o, plugin)) = pending.take() {
                    // The outgoing event plays on for `o` past the cut; this one starts `a` before it.
                    if let Some(prev) = track.clips.last_mut().filter(|_| !b.shell) {
                        prev.duration = snap(prev.position + prev.duration + o) - prev.position;
                        prev.out_point = prev.in_point + prev.duration * prev.rate;
                    }
                    if !b.shell {
                        let back = a.min(clip.in_point / clip.rate.max(1e-9)).min(clip.position);
                        clip.position = snap(clip.position - back);
                        clip.in_point -= back * clip.rate;
                        clip.duration = snap(t + dur) - clip.position;
                        clip.out_point = clip.in_point + clip.duration * clip.rate;
                        if track.clips.last().is_some_and(|p| p.position + p.duration > clip.position + 1e-9) {
                            clip.transition = Some(plugin.unwrap_or(Transition { plugin: DISSOLVE.into(), ..Default::default() }));
                        } else {
                            clip.fade_in = (a + o).min(clip.duration);
                        }
                    }
                }
                t += dur;
                track.clips.push(clip);
            }
            Child::Stack(s) => {
                // A nested stack: its first track lays into this one, the others become tracks of their own.
                let (offset, limit) = s.source_range.as_ref().map_or((0.0, f64::INFINITY), |r| (r.start_time.seconds(), r.duration.seconds()));
                let mut end = t;
                let mut first = true;
                for ch in &s.children {
                    let Child::Track(nt) = ch else { continue };
                    let nt_video = nt.kind.as_deref().is_none_or(|k| k == "Video");
                    if nt_video != video && !first {
                        continue;
                    }
                    let mut target = if first { std::mem::replace(track, Track::new("", track.kind)) } else { Track::new(&format!("{} ({})", track.name, if nt.name.is_empty() { "nested" } else { &nt.name }), track.kind) };
                    let before = target.clips.len();
                    let e = lay(b, pool, &nt.children, t - offset, video, &mut target, extra, markers);
                    // Trim what the stack's range leaves out.
                    for c in target.clips.iter_mut().skip(before) {
                        let (cs, ce) = (c.position.max(t), (c.position + c.duration).min(t + limit));
                        if ce <= cs {
                            c.duration = 0.0;
                            continue;
                        }
                        c.in_point += (cs - c.position) * c.rate;
                        c.position = cs;
                        c.duration = ce - cs;
                        c.out_point = c.in_point + c.duration * c.rate;
                    }
                    target.clips.retain(|c| c.duration > 0.0);
                    end = end.max(e.min(t + limit));
                    if first {
                        *track = target;
                    } else {
                        extra.push(target);
                    }
                    first = false;
                }
                b.warnings.push(format!("{}: the nested stack {} at {:.3} s was flattened", track.name, if s.name.is_empty() { "(unnamed)" } else { &s.name }, t));
                t = end;
            }
            Child::Track(nt) => {
                b.warnings.push(format!("{}: a track nested in a track at {:.3} s was laid in line", track.name, t));
                t = lay(b, pool, &nt.children, t, video, track, extra, markers);
            }
            Child::Other => b.warnings.push(format!("{}: an item of an unknown schema at {:.3} s was skipped", track.name, t)),
        }
    }
    t
}

/// Build the project from a parsed timeline; `probed` holds each media path's probe. A timeline Milan wrote is
/// rebuilt from its metadata; any other from the OTIO objects.
pub fn to_project(tl: &Timeline, probed: &HashMap<String, Result<MediaItem, String>>, dir: Option<&Path>) -> (Project, Vec<String>) {
    let shell: Option<Project> = tl.metadata.get("milan").and_then(|m| m.get("project")).and_then(|v| serde_json::from_value(v.clone()).ok());
    let has_shell = shell.is_some();
    let mut p = shell.unwrap_or_else(|| Project::new(if tl.name.is_empty() { "Imported Timeline" } else { &tl.name }));
    if !has_shell {
        // The frame rate: the first rational time's rate.
        let mut rate = tl.global_start_time.as_ref().map(|t| t.rate);
        fn first_rate(children: &[Child]) -> Option<f64> {
            children.iter().find_map(|c| match c {
                Child::Clip(c) => c.source_range.as_ref().map(|r| r.duration.rate),
                Child::Gap(g) => g.source_range.as_ref().map(|r| r.duration.rate),
                Child::Stack(s) | Child::Track(s) => first_rate(&s.children),
                _ => None,
            })
        }
        rate = rate.filter(|r| *r > 1.0).or_else(|| first_rate(&tl.tracks.children));
        if let Some(r) = rate.filter(|r| *r > 1.0 && *r <= 240.0) {
            p.settings.frame_rate = (r * 1000.0).round() / 1000.0;
        }
    }
    let mut b = Build { probed, dir, fps: p.settings.frame_rate, warnings: Vec::new(), shell: has_shell };
    let (mut video, mut audio): (Vec<Track>, Vec<Track>) = (Vec::new(), Vec::new());
    let mut clip_markers: Vec<(f64, String, String)> = Vec::new();
    let mut pool = std::mem::take(&mut p.media_pool);
    let add_track = |b: &mut Build, pool: &mut Vec<MediaItem>, tr: &Composition, video: &mut Vec<Track>, audio: &mut Vec<Track>, markers: &mut Vec<(f64, String, String)>| {
        let is_video = match tr.kind.as_deref().unwrap_or("Video") {
            "Video" => true,
            "Audio" => false,
            k => {
                b.warnings.push(format!("A track of kind {k:?} was skipped"));
                return;
            }
        };
        let kind = if is_video { TrackKind::Video } else { TrackKind::Audio };
        let n = if is_video { video.len() } else { audio.len() } + 1;
        let mut track = match from_milan::<Track>(&tr.metadata).filter(|_| b.shell) {
            Some(t) => t,
            None => {
                let mut t = Track::new(&if tr.name.is_empty() { format!("{} {n}", if is_video { "Video" } else { "Audio" }) } else { tr.name.clone() }, kind);
                t.muted = !tr.enabled;
                t
            }
        };
        track.clips.clear();
        let mut extra = Vec::new();
        lay(b, pool, &tr.children, 0.0, is_video, &mut track, &mut extra, markers);
        let list = if is_video { video } else { audio };
        list.push(track);
        list.extend(extra);
    };
    for ch in &tl.tracks.children {
        match ch {
            Child::Track(tr) => add_track(&mut b, &mut pool, tr, &mut video, &mut audio, &mut clip_markers),
            Child::Stack(s) => {
                for ch in &s.children {
                    if let Child::Track(tr) = ch {
                        add_track(&mut b, &mut pool, tr, &mut video, &mut audio, &mut clip_markers);
                    }
                }
            }
            Child::Clip(_) | Child::Gap(_) | Child::Transition(_) => b.warnings.push("An item directly in the timeline's stack (not in a track) was skipped".into()),
            Child::Other => b.warnings.push("A child of an unknown schema in the timeline's stack was skipped".into()),
        }
    }
    video.reverse(); // OTIO stacks bottom to top
    p.media_pool = pool;
    if video.is_empty() {
        video.push(Track::new("Video 1", TrackKind::Video));
    }
    if audio.is_empty() {
        audio.push(Track::new("Audio 1", TrackKind::Audio));
    }
    (p.timeline.video_tracks, p.timeline.audio_tracks) = (video, audio);
    if !has_shell {
        let fps = p.settings.frame_rate;
        let snap = |t: f64| edit::quantize(t.max(0.0), fps);
        for m in &tl.tracks.markers {
            let (start, dur) = (m.marked_range.start_time.seconds(), m.marked_range.duration.seconds());
            if dur > 0.0 {
                p.timeline.regions.push(Region { start: snap(start), end: snap(start + dur), name: m.name.clone() });
            } else {
                p.timeline.markers.push(Marker { position: snap(start), name: m.name.clone(), color: color_rgb(&m.color) });
            }
        }
        for (at, name, color) in clip_markers {
            p.timeline.markers.push(Marker { position: snap(at), name, color: color_rgb(&color) });
        }
        p.timeline.markers.sort_by(|a, b| a.position.total_cmp(&b.position));
    }
    p.modified = true;
    p.path = None;
    (p, b.warnings)
}


#[cfg(test)]
mod tests {
    use super::*;

    fn probed(p: &Project) -> HashMap<String, Result<MediaItem, String>> {
        p.media_pool.iter().filter(|m| m.generator.is_none() && !m.path.as_os_str().is_empty()).map(|m| (m.path.display().to_string(), Ok(m.clone()))).collect()
    }

    /// Milan → OTIO → Milan gives the very same project (transitions, takes, markers, regions, envelopes, a speed
    /// change, FX, generated media, groups, a fade with a transition on it).
    #[test]
    fn lossless_round_trip() {
        let p = crate::interchange::vegas_export::tests::sample();
        let (tl, _) = export(&p);
        let text = serde_json::to_string_pretty(&tl).unwrap();
        let back = parse(&text).unwrap();
        let (q, w) = to_project(&back, &probed(&p), None);
        assert!(w.is_empty(), "{w:?}");
        let (a, b) = (serde_json::to_value(&p).unwrap(), serde_json::to_value(&q).unwrap());
        assert_eq!(a, b);
        assert_eq!(q.media_pool.len(), p.media_pool.len());
    }

    /// The OTIO objects themselves (what other tools read) describe the same cut.
    #[test]
    fn export_writes_the_otio_objects_other_tools_read() {
        let p = crate::interchange::vegas_export::tests::sample();
        let (tl, _) = export(&p);
        let v: Value = serde_json::to_value(&tl).unwrap();
        assert_eq!(v["OTIO_SCHEMA"], "Timeline.1");
        let tracks = v["tracks"]["children"].as_array().unwrap();
        assert_eq!(tracks.len(), 3);
        // Bottom to top: Child, Parent, then audio.
        assert_eq!((tracks[0]["name"].as_str(), tracks[0]["kind"].as_str(), tracks[2]["kind"].as_str()), (Some("Child"), Some("Video"), Some("Audio")));
        let parent = tracks[1]["children"].as_array().unwrap();
        // Gap to 1 s, the clip (with its take as a second reference and a time warp), gap, the generated clip.
        assert_eq!(parent[0]["OTIO_SCHEMA"], "Gap.1");
        let clip = &parent[1];
        assert_eq!(clip["OTIO_SCHEMA"], "Clip.2");
        assert_eq!(clip["source_range"]["start_time"]["value"].as_f64().unwrap() / 25.0, 3.5);
        assert!(clip["media_references"]["TAKE_1"]["target_url"].as_str().unwrap().ends_with("b.mp4"));
        assert_eq!(clip["media_references"]["DEFAULT_MEDIA"]["target_url"], "file:///C:/m/a.mp4");
        assert!(clip["effects"].as_array().unwrap().iter().any(|e| e["OTIO_SCHEMA"] == "LinearTimeWarp.1" && e["time_scalar"] == 2.0));
        assert_eq!(clip["metadata"]["milan"]["name"], "Take A");
        assert_eq!(parent[3]["media_references"]["DEFAULT_MEDIA"]["OTIO_SCHEMA"], "MissingReference.1");
        let markers = v["tracks"]["markers"].as_array().unwrap();
        assert_eq!(markers.len(), 2);
        assert_eq!(markers[1]["marked_range"]["duration"]["value"].as_f64().unwrap() / 25.0, 2.0);
    }

    /// Overlapping events become a dissolve centred on the overlap, and come back as the overlap.
    #[test]
    fn overlaps_are_dissolves_for_other_readers() {
        let mut p = crate::interchange::cutlist::tests::sample();
        p.timeline.video_tracks[0].clips[1].fx.clear();
        let (tl, _) = export(&p);
        let Child::Track(v) = &tl.tracks.children[0] else { panic!() };
        let kinds: Vec<&str> = v.children.iter().map(|c| match c { Child::Clip(_) => "clip", Child::Gap(_) => "gap", Child::Transition(_) => "tr", _ => "?" }).collect();
        assert_eq!(kinds, ["clip", "tr", "clip", "gap", "clip"]);
        let Child::Transition(tr) = &v.children[1] else { panic!() };
        assert_eq!((tr.in_offset.seconds(), tr.out_offset.seconds()), (0.5, 0.5));
        let Child::Clip(a) = &v.children[0] else { panic!() };
        assert_eq!(a.source_range.as_ref().unwrap().duration.seconds(), 4.5);
        // Without the Milan metadata the OTIO objects alone rebuild the overlap.
        let mut tl = tl;
        tl.metadata.clear();
        let (q, w) = to_project(&tl, &probed(&p), None);
        assert!(w.is_empty(), "{w:?}");
        let c = &q.timeline.video_tracks[0].clips;
        assert_eq!((c[0].position, c[0].duration, c[1].position, c[1].duration, c[1].in_point, c[1].rate), (0.0, 5.0, 4.0, 4.0, 10.0, 2.0));
        assert!(c[1].transition.is_some());
        assert_eq!((q.settings.frame_rate, q.timeline.markers.len(), q.timeline.regions.len()), (25.0, 1, 1));
    }

    #[test]
    fn reads_the_hand_written_simple_cut() {
        let text = include_str!("../../tests/fixtures/simple_cut.otio");
        let tl = parse(text).unwrap();
        assert_eq!(media_paths(&tl, None).len(), 2);
        let (p, w) = to_project(&tl, &HashMap::new(), None);
        assert_eq!(p.settings.frame_rate, 24.0);
        assert_eq!(p.name, "Simple Cut");
        let v = &p.timeline.video_tracks[0].clips;
        assert_eq!(v.len(), 3, "{v:#?}");
        assert_eq!((v[0].position, v[0].duration, v[0].in_point), (0.0, 2.5, 3.0));
        // A 1 s dissolve centred on the 2 s cut: A plays on to 2.5, B starts at 1.5 from 0.5 s before its in point.
        assert_eq!((v[1].position, v[1].in_point, v[1].duration), (1.5, 9.5, 3.5));
        assert_eq!(v[1].transition.as_ref().map(|t| t.plugin.as_str()), Some(DISSOLVE));
        assert_eq!((v[2].position, v[2].muted), (6.0, true));
        assert!(w.iter().all(|w| w.starts_with("Media offline")), "{w:?}");
        assert_eq!(p.timeline.audio_tracks[0].clips.len(), 1);
        assert_eq!(p.timeline.markers.len(), 1);
    }

    #[test]
    fn reads_the_hand_written_nested_example() {
        let text = include_str!("../../tests/fixtures/nested_example.otio");
        let tl = parse(text).unwrap();
        let (p, w) = to_project(&tl, &HashMap::new(), None);
        assert_eq!(p.settings.frame_rate, 30.0);
        let v = &p.timeline.video_tracks;
        assert_eq!(v.len(), 2, "{:?}", v.iter().map(|t| &t.name).collect::<Vec<_>>());
        // The nested stack's first track lies in line (trimmed to the stack's 2 s range), its second track is a new one.
        let c = &v[1].clips;
        assert_eq!(c.len(), 3, "{c:#?}");
        assert_eq!((c[1].position, c[1].duration, c[1].in_point), (3.0, 2.0, 2.0));
        assert!((c[1].rate - 2.0).abs() < 1e-9 && !c[1].reverse);
        assert!(c[2].envelopes.iter().any(|e| e.kind == EnvKind::Velocity && e.points.keys[0].v == 0.0), "freeze frame");
        assert_eq!(v[0].name, "V1 (Nested V2)");
        assert_eq!(v[0].clips.len(), 1);
        assert!(w.iter().any(|w| w.contains("flattened")), "{w:?}");
        assert!(w.iter().any(|w| w.contains("Media missing")), "{w:?}");
        assert_eq!(p.timeline.regions.len(), 1);
    }

    #[test]
    fn file_urls() {
        assert_eq!(file_url(Path::new(r"C:\m\a b.mp4")), "file:///C:/m/a%20b.mp4");
        assert_eq!(url_path("file:///C:/m/a%20b.mp4", None), r"C:\m\a b.mp4");
        assert_eq!(url_path("file://localhost/C:/m/a.mp4", None), r"C:\m\a.mp4");
        assert_eq!(url_path("clips/a.mp4", Some(Path::new(r"D:\x"))), r"D:\x\clips\a.mp4");
    }
}
