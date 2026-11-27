//! VEGAS project import (File › Import › VEGAS Project...): the `.milan.json` that `tools/vegas/Export to Milan
//! Pro.cs` writes from inside VEGAS, or, without VEGAS, a scan of a `.veg` for the media it uses.
//!
//! R25 (script version 2): event, track, output and media video FX with their OFX parameters and keyframes,
//! transitions, generated media, Track Motion, event envelopes, parent/child compositing, the project's video and
//! audio settings and audio FX by name come across too. `vegas_map` turns VEGAS's plug-ins into Milan's; anything
//! unmapped is kept as a disabled `ofx:` placeholder and listed in the import report.
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use crate::edit;
use crate::engine::plugins;
use crate::interchange::vegas_map;
use crate::project::*;

/// The script's JSON. Every field is optional, so exports from older or newer scripts still load.
#[derive(Debug, Default, Serialize, Deserialize)]
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
    /// Video output FX (the Video Bus) and master bus audio FX.
    pub output_fx: Vec<VFx>,
    pub master_fx: Vec<VFx>,
    /// Media FX, per file.
    pub media_fx: Vec<VMediaFx>,
    /// What the script saw and could not export, as sentences.
    pub unsupported: Vec<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VProject {
    pub w: u32,
    pub h: u32,
    pub fps: f64,
    pub sample_rate: u32,
    /// VEGAS `VideoFieldOrder` name: None, UpperFieldFirst, LowerFieldFirst.
    pub field_order: String,
    /// 0 = not exported.
    pub pixel_aspect: f64,
    /// `VideoRenderQuality` name (Draft, Preview, Good, Best).
    pub quality: String,
    /// `MotionBlurType` and `VideoDeinterlaceMethod` names.
    pub motion_blur: String,
    pub deinterlace: String,
    /// Audio bit depth (16, 24, 32) and `AudioBusMode` name (Stereo, Surround51...).
    pub bit_depth: u32,
    pub bus_mode: String,
    /// Pixel format name, when a script exports it (VEGAS 15's API does not).
    pub pixel_format: String,
}

/// A marker (length 0), region or the loop region; times in seconds.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VSpan {
    pub t: f64,
    pub length: f64,
    pub name: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
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
    /// Track FX (video or audio).
    pub fx: Vec<VFx>,
    /// Video: a compositing child; a compositing parent, with its Parent Composite Mode and Parent Motion.
    pub child: bool,
    pub parent: bool,
    pub parent_mode: String,
    pub parent_motion: Vec<VMotionKey>,
    /// Video: Track Motion keyframes, and whether 2D Shadow / Glow are on.
    pub motion: Vec<VMotionKey>,
    pub shadow: bool,
    pub glow: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VEnvelope {
    /// VEGAS `EnvelopeType` name: Volume, Pan, Mute, Composite, Velocity...
    pub kind: String,
    /// The envelope's no-effect value (Mute: not muted).
    pub neutral: f64,
    pub points: Vec<VPoint>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VPoint {
    pub t: f64,
    pub v: f64,
    /// VEGAS `CurveType` name: Linear, Fast, Slow, Smooth, Sharp, None (hold).
    pub curve: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
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
    /// Empty for generated media (`generator`).
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
    #[serde(rename = "loop", default = "yes")]
    pub looping: bool,
    pub normalize: bool,
    /// Index of the event's group, shared by its members.
    pub group: Option<i64>,
    /// Inactive takes.
    pub takes: Vec<VTake>,
    /// Video: Pan/Crop keyframes, and the source frame size their corners are measured in.
    pub pan_crop: Vec<VPanKey>,
    pub src_size: Option<[f64; 2]>,
    /// Event FX, event envelopes (Velocity...), the transitions on the fade in / fade out, generated media.
    pub fx: Vec<VFx>,
    pub envelopes: Vec<VEnvelope>,
    pub transition_in: Option<VFx>,
    pub transition_out: Option<VFx>,
    pub generator: Option<VFx>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VTake {
    pub path: String,
    pub offset: f64,
    pub reverse: bool,
    pub name: String,
    pub stream: i32,
}

/// A Pan/Crop keyframe: event-relative time and three corners of the crop rectangle in source pixels.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VPanKey {
    pub t: f64,
    pub tl: [f64; 2],
    pub tr: [f64; 2],
    pub bl: [f64; 2],
    /// VEGAS `VideoKeyframeType` name.
    pub curve: String,
}

/// A plug-in instance: an OFX video FX, transition or generator (`id` is `PlugIn.UniqueID`), or an audio FX known
/// by `name` only.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VFx {
    pub id: String,
    pub name: String,
    pub bypass: bool,
    pub preset: String,
    pub params: Vec<VParam>,
}

/// One OFX parameter: `type` is the `OFXParameterType` name, `v` a number, bool, string, or an array for a 2D / 3D
/// / colour value; `keys` its keyframes (event-relative seconds) when animated.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VParam {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub v: Value,
    pub keys: Vec<VKey>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VKey {
    pub t: f64,
    pub v: Value,
    /// `OFXInterpolationType` name.
    pub curve: String,
}

/// A Track Motion keyframe in VEGAS's units: pixels from the frame centre (y up), size in pixels, degrees.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VMotionKey {
    pub t: f64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
    pub h: f64,
    pub rx: f64,
    pub ry: f64,
    pub rz: f64,
    pub curve: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VMediaFx {
    pub path: String,
    pub fx: Vec<VFx>,
}

fn one() -> f64 {
    1.0
}

fn yes() -> bool {
    true
}

/// Marker flag colour (VP16), as `ui::add_marker` gives new markers.
const MARKER_COLOR: [u8; 3] = [0xDB, 0x8E, 0x57];

/// Parse the script's JSON; anything but `format: "milan-vegas"` is an error.
pub fn read_vegas_json(text: &str) -> anyhow::Result<VegasExport> {
    let e: VegasExport = serde_json::from_str(text.trim_start_matches('\u{feff}'))?;
    anyhow::ensure!(e.format == "milan-vegas", "not an export from the Milan Pro VEGAS script (format \"{}\")", e.format);
    Ok(e)
}

impl VegasExport {
    /// Every media file the export names, once, in order: the events', the takes', the media FX's.
    pub fn media_paths(&self) -> Vec<String> {
        let mut paths: Vec<String> = Vec::new();
        let events = self.tracks.iter().flat_map(|t| &t.events);
        let from_events = events.flat_map(|ev| std::iter::once(&ev.path).chain(ev.takes.iter().map(|t| &t.path)));
        for p in from_events.chain(self.media_fx.iter().map(|m| &m.path)) {
            if !p.is_empty() && !paths.contains(p) {
                paths.push(p.clone());
            }
        }
        paths
    }
}

pub(super) fn curve(name: &str, default: Curve) -> Curve {
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

pub(super) fn composite(mode: &str) -> Option<Composite> {
    Some(match mode {
        "" | "SrcAlpha" => Composite::SourceAlpha,
        // The 3D track motion itself comes with the Track Motion keys.
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
        // VEGAS's velocity points are the rate itself (1 = 100 %), as Milan's are.
        EnvKind::Velocity => edit::env_clamp(kind, v as f32),
        _ => v.clamp(0.0, 1.0) as f32,
    }
}

/// VEGAS `VideoFieldOrder`, `VideoRenderQuality`, `MotionBlurType` and `VideoDeinterlaceMethod` names.
fn field_order(name: &str) -> Option<FieldOrder> {
    Some(match name {
        "" => return None,
        "None" | "ProgressiveScan" => FieldOrder::Progressive,
        "UpperFieldFirst" => FieldOrder::Upper,
        "LowerFieldFirst" => FieldOrder::Lower,
        _ => return None,
    })
}

fn quality(name: &str) -> Option<crate::decode::Quality> {
    use crate::decode::Quality;
    Some(match name {
        "Draft" => Quality::Draft,
        "Preview" => Quality::Preview,
        "Good" => Quality::Good,
        "Best" => Quality::Best,
        _ => return None,
    })
}

fn blur_type(name: &str) -> Option<BlurType> {
    BlurType::ALL.into_iter().find(|b| b.name().replace(' ', "") == name)
}

fn deinterlace(name: &str) -> Option<Deinterlace> {
    Some(match name {
        "None" => Deinterlace::None,
        "BlendFields" => Deinterlace::Blend,
        "InterpolateFields" => Deinterlace::Interpolate,
        _ => return None,
    })
}

/// A JSON parameter value's numeric components: a number or bool is one, an array its items (strings none).
fn components(v: &Value) -> Vec<f32> {
    match v {
        Value::Number(n) => vec![n.as_f64().unwrap_or(0.0) as f32],
        Value::Bool(b) => vec![*b as u8 as f32],
        Value::Array(a) => a.iter().flat_map(components).collect(),
        _ => Vec::new(),
    }
}

/// The key suffixes of a parameter's components, as the OFX host stores them (`key.r`.. for colours, `key.x`.. for
/// 2D / 3D values, none for a scalar).
fn suffixes(kind: &str, n: usize) -> &'static [&'static str] {
    let k = kind.to_ascii_lowercase();
    match (k.as_str(), n) {
        (_, 0 | 1) => &[""],
        ("rgb", _) | ("rgba", _) => &[".r", ".g", ".b", ".a"],
        (_, 2) => &[".x", ".y"],
        (_, 3) if k.contains("rgb") => &[".r", ".g", ".b"],
        (_, 3) => &[".x", ".y", ".z"],
        _ => &[".r", ".g", ".b", ".a"],
    }
}

/// A VEGAS FX's parameters as Milan's keys: one `Keyframes` per numeric component (a static value is one key at 0),
/// and the string parameters.
fn flatten(v: &VFx) -> (BTreeMap<String, Keyframes<f32>>, BTreeMap<String, String>) {
    let (mut params, mut text) = (BTreeMap::new(), BTreeMap::new());
    for p in &v.params {
        if let Value::String(s) = &p.v {
            text.insert(p.name.clone(), s.clone());
            continue;
        }
        let vals = components(&p.v);
        let sfx = suffixes(&p.kind, vals.len());
        for (i, sfx) in sfx.iter().enumerate().take(vals.len().max(1)) {
            let key = |t: f64, v: f32, c: &str| Keyframe { t, v, curve: curve(c, Curve::Linear) };
            let mut keys: Vec<Keyframe<f32>> = p.keys.iter().filter_map(|k| components(&k.v).get(i).map(|v| key(k.t.max(0.0), *v, &k.curve))).collect();
            if keys.is_empty() {
                let Some(v) = vals.get(i) else { continue };
                keys.push(key(0.0, *v, "Linear"));
            }
            keys.sort_by(|a, b| a.t.total_cmp(&b.t));
            params.insert(format!("{}{sfx}", p.name), Keyframes { keys });
        }
    }
    (params, text)
}

/// Plain text and the first font of an RTF document (VEGAS's Titles & Text keeps its text as RTF); None for plain
/// text.
pub fn rtf_plain(s: &str) -> Option<(String, Option<String>)> {
    if !s.starts_with("{\\rtf") {
        return None;
    }
    // Inside the root group from the start.
    let body = &s[1..];
    let font = body.find("\\fcharset").and_then(|i| {
        let rest = &body[i..];
        let name = rest.split_once(' ')?.1.split(';').next()?.trim();
        (!name.is_empty()).then(|| name.to_string())
    });
    let (mut out, mut depth, mut skip_depth) = (String::new(), 1i32, None::<i32>);
    let chars: Vec<char> = body.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '{' => {
                depth += 1;
                // Header groups (font and colour tables, destinations) hold no text.
                let rest: String = chars[i + 1..].iter().take(10).collect();
                if skip_depth.is_none() && (rest.starts_with("\\fonttbl") || rest.starts_with("\\colortbl") || rest.starts_with("\\*") || rest.starts_with("\\stylesheet")) {
                    skip_depth = Some(depth);
                }
            }
            '}' => {
                if skip_depth == Some(depth) {
                    skip_depth = None;
                }
                depth -= 1;
            }
            '\\' => {
                i += 1;
                let Some(&n) = chars.get(i) else { break };
                if n == '\'' {
                    let hex: String = chars[i + 1..].iter().take(2).collect();
                    if let Ok(b) = u8::from_str_radix(&hex, 16) {
                        if skip_depth.is_none() {
                            out.push(char::from(b));
                        }
                    }
                    i += 2;
                } else if n.is_ascii_alphabetic() {
                    let mut word = String::new();
                    while i < chars.len() && chars[i].is_ascii_alphabetic() {
                        word.push(chars[i]);
                        i += 1;
                    }
                    let mut num = String::new();
                    while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '-') {
                        num.push(chars[i]);
                        i += 1;
                    }
                    // A control word's one delimiting space belongs to it.
                    if i < chars.len() && chars[i] == ' ' {
                        i += 1;
                    }
                    if word == "u" {
                        // \uN? is the character N with a fallback character after it (\uc1).
                        if let Some(c) = num.parse::<i32>().ok().and_then(|n| char::from_u32(n.rem_euclid(65536) as u32)) {
                            if skip_depth.is_none() {
                                out.push(c);
                            }
                        }
                        i += 1;
                    } else if skip_depth.is_none() && matches!(word.as_str(), "par" | "line") {
                        out.push('\n');
                    }
                    continue;
                } else if skip_depth.is_none() {
                    // \{ \} \\ and \~ (a non-breaking space).
                    out.push(if n == '~' { ' ' } else { n });
                }
            }
            '\r' | '\n' => {}
            _ if skip_depth.is_none() && depth >= 1 => out.push(c),
            _ => {}
        }
        i += 1;
    }
    Some((out.trim_end_matches('\n').to_string(), font))
}

/// Milan's Crop (left, top, right, bottom fractions) from VEGAS's Crop (the kept rectangle's size and centre, y up).
fn crop_params(params: &BTreeMap<String, Keyframes<f32>>, out: &mut BTreeMap<String, Keyframes<f32>>) {
    let at = |k: &str, d: f32| params.get(k).map_or(d, |kf| kf.keys.first().map_or(d, |k| k.v));
    let (xs, ys, cx, cy) = (at("XScale", 1.0), at("YScale", 1.0), at("Location.x", 0.5), at("Location.y", 0.5));
    let one = |v: f32| Keyframes { keys: vec![Keyframe { t: 0.0, v: v.clamp(0.0, 0.5), curve: Curve::Linear }] };
    out.insert("left".into(), one(cx - xs / 2.0));
    out.insert("right".into(), one(1.0 - (cx + xs / 2.0)));
    out.insert("top".into(), one(1.0 - (cy + ys / 2.0)));
    out.insert("bottom".into(), one(cy - ys / 2.0));
}

/// A VEGAS video FX, transition or generator as a Milan `Fx`: the mapped built-in with its parameters converted, or
/// a disabled `ofx:` placeholder holding every parameter, reported in `warnings` under `at`.
pub fn fx_from_vegas(v: &VFx, at: &str, warnings: &mut Vec<String>) -> Fx {
    let (params, text) = flatten(v);
    let id = uuid::Uuid::new_v4().to_string();
    let Some(m) = vegas_map::to_milan(&v.id) else {
        let what = if v.name.is_empty() { vegas_map::normalize(&v.id).to_string() } else { format!("{} ({})", v.name, vegas_map::normalize(&v.id)) };
        warnings.push(format!("{at}: {what} is not available in Milan Pro; kept disabled as an OFX placeholder"));
        return Fx { id, plugin: vegas_map::placeholder(&v.id), enabled: false, params, text, auto: Default::default(), blob: Default::default() };
    };
    let mut fx = Fx { id, plugin: m.milan.to_string(), enabled: !v.bypass, params: BTreeMap::new(), text: BTreeMap::new(), auto: Default::default(), blob: Default::default() };
    // What the VEGAS plug-in implies first; its parameters override that.
    for (k, v) in m.set {
        fx.params.insert(k.to_string(), Keyframes { keys: vec![Keyframe { t: 0.0, v: *v, curve: Curve::Linear }] });
    }
    for pm in m.params {
        if let Some(kf) = params.get(pm.vegas) {
            let keys = kf.keys.iter().map(|k| Keyframe { t: k.t, v: pm.to_milan(k.v), curve: k.curve }).collect();
            fx.params.insert(pm.milan.to_string(), Keyframes { keys });
        }
    }
    for (vk, mk) in m.text {
        if let Some(s) = text.get(*vk) {
            fx.text.insert(mk.to_string(), s.clone());
        }
    }
    match m.milan {
        "milan.fx.crop" => crop_params(&params, &mut fx.params),
        "milan.gen.text" => {
            if let Some(s) = text.get("Text") {
                let (plain, font) = rtf_plain(s).unwrap_or((s.clone(), None));
                fx.text.insert("text".into(), plain);
                if let Some(f) = font {
                    fx.text.insert("font".into(), f);
                }
            }
        }
        _ => {}
    }
    fx
}

/// A VEGAS transition as Milan's (its parameters at the transition's start); an unmapped one plays as a crossfade
/// until the plug-in is installed.
fn transition_from_vegas(v: &VFx, at: &str, warnings: &mut Vec<String>) -> Transition {
    let n = warnings.len();
    let fx = fx_from_vegas(v, at, warnings);
    if warnings.len() > n {
        let last = warnings.last_mut().expect("just pushed");
        *last = last.replace("kept disabled as an OFX placeholder", "a crossfade plays until the plug-in is installed");
    }
    edit::transition_of(&fx)
}

/// A VEGAS audio FX, known by name: the Milan FX that stands in (with the preset of that name when Milan has one).
fn audio_fx_from_vegas(v: &VFx, at: &str, warnings: &mut Vec<String>) -> Option<Fx> {
    let Some(info) = vegas_map::audio_to_milan(&v.name).and_then(plugins::find) else {
        warnings.push(format!("{at}: audio FX {} is not available in Milan Pro and was left out", v.name));
        return None;
    };
    let preset = info.presets.iter().find(|p| p.name.eq_ignore_ascii_case(&v.preset));
    Some(Fx { enabled: !v.bypass, ..plugins::new_fx(info, preset) })
}

/// A Track Motion key in Milan's frame-relative units; `(pw, ph)` is the project frame.
fn motion(k: &VMotionKey, (pw, ph): (f64, f64)) -> Motion {
    // shortcut: VEGAS's Z axis and rotation sense were not measured against a VEGAS render; positive Z is taken as
    // towards the viewer and a positive RotationZ as counter-clockwise (y up), as the API's y-up frame suggests.
    Motion {
        x: (k.x / pw) as f32,
        y: (-k.y / ph) as f32,
        w: (if k.w > 0.0 { k.w } else { pw } / pw) as f32,
        h: (if k.h > 0.0 { k.h } else { ph } / ph) as f32,
        rot: -k.rz as f32,
        z: (k.z / pw) as f32,
        rx: k.rx as f32,
        ry: k.ry as f32,
    }
}

fn motion_keys(keys: &[VMotionKey], size: (f64, f64), snap: &dyn Fn(f64) -> f64) -> Keyframes<Motion> {
    let keys: Vec<_> = keys.iter().map(|k| Keyframe { t: snap(k.t), v: motion(k, size), curve: curve(&k.curve, Curve::Smooth) }).collect();
    // VEGAS gives every track one full-frame key; Milan stores none for that.
    if keys.iter().any(|k| k.v != Motion::default()) { Keyframes { keys } } else { Keyframes::default() }
}

/// The pool item for `path`, added on first use: the probed item, or an offline one (plus a warning) when the
/// probe failed.
pub(crate) fn media_id(pool: &mut Vec<MediaItem>, probed: &HashMap<String, Result<MediaItem, String>>, path: &str, video: bool, warnings: &mut Vec<String>) -> String {
    if let Some(m) = pool.iter().find(|m| m.path.as_os_str() == path && m.generator.is_none()) {
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
    let pr = &e.project;
    p.settings = ProjectSettings {
        width: nz(pr.w, d.width),
        height: nz(pr.h, d.height),
        frame_rate: fps,
        sample_rate: nz(pr.sample_rate, d.sample_rate),
        field_order: field_order(&pr.field_order).unwrap_or(d.field_order),
        par: if pr.pixel_aspect > 0.0 { pr.pixel_aspect } else { d.par },
        render_quality: quality(&pr.quality).unwrap_or(d.render_quality),
        motion_blur: blur_type(&pr.motion_blur).unwrap_or(d.motion_blur),
        deinterlace: deinterlace(&pr.deinterlace).unwrap_or(d.deinterlace),
        bit_depth: if [8, 16, 24, 32].contains(&pr.bit_depth) { pr.bit_depth as u8 } else { d.bit_depth },
        ..d
    };
    if !pr.bus_mode.is_empty() && pr.bus_mode != "Stereo" {
        warnings.push(format!("Master bus mode {}: Milan Pro's master bus is stereo (R10a)", pr.bus_mode));
    }
    if !pr.pixel_format.is_empty() && !pr.pixel_format.contains("8") {
        warnings.push(format!("Pixel format {}: Milan Pro composites in 8-bit (R40b)", pr.pixel_format));
    }
    let size = (p.settings.width as f64, p.settings.height as f64);
    let snap = |t: f64| edit::quantize(t.max(0.0), fps);
    let span = |s: &VSpan| {
        let (a, b) = if s.length < 0.0 { (s.t + s.length, s.t) } else { (s.t, s.t + s.length) };
        (snap(a), snap(b))
    };
    // Media FX go onto each event of the file (Milan has no per-file chain).
    let mut media_fx: HashMap<&str, Vec<Fx>> = HashMap::new();
    for m in e.media_fx.iter().filter(|m| !m.fx.is_empty()) {
        let file = Path::new(&m.path).file_name().map_or(m.path.clone(), |n| n.to_string_lossy().into_owned());
        let names: Vec<&str> = m.fx.iter().map(|f| f.name.as_str()).collect();
        warnings.push(format!("Media FX on {file} ({}) are applied to each of its events", names.join(", ")));
        let chain = m.fx.iter().map(|f| fx_from_vegas(f, &format!("Media FX on {file}"), &mut warnings)).collect();
        media_fx.insert(m.path.as_str(), chain);
    }
    let fresh = |chain: &[Fx]| chain.iter().map(|f| Fx { id: uuid::Uuid::new_v4().to_string(), ..f.clone() }).collect::<Vec<_>>();
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
            tr.fx = t.fx.iter().map(|f| fx_from_vegas(f, &format!("{name}: track FX"), &mut warnings)).collect();
            tr.motion = motion_keys(&t.motion, size, &snap);
            (tr.shadow.on, tr.glow.on) = (t.shadow, t.glow);
            if t.glow {
                tr.glow.keys = Keyframes { keys: vec![Keyframe { t: 0.0, v: Halo::glow(), curve: Curve::Linear }] };
            }
            tr.child = t.child;
            if t.parent {
                tr.parent_mode = composite(&t.parent_mode).unwrap_or_default();
                tr.parent_motion = motion_keys(&t.parent_motion, size, &snap);
            }
        } else {
            (tr.volume, tr.pan) = (t.volume.max(0.0) as f32, t.pan.clamp(-1.0, 1.0) as f32);
            // VEGAS's own chain by name (an export from before R25 names none: VEGAS's neutral default chain then,
            // not the user's saved default, which would change the imported mix).
            tr.fx = if e.version >= 2 {
                t.fx.iter().filter_map(|f| audio_fx_from_vegas(f, &format!("{name}: track FX"), &mut warnings)).collect()
            } else {
                crate::project::default_track_chain(None)
            };
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
            let at = format!("{name}: the event at {:.3} s", ev.start);
            let start = snap(ev.start);
            let duration = (snap(ev.start + ev.length) - start).max(1.0 / fps);
            let id = if let Some(g) = ev.generator.as_ref().filter(|_| video) {
                // Generated media: a pool item of its own, drawn by the generator.
                let fx = fx_from_vegas(g, &format!("{at}: generated media"), &mut warnings);
                let [w, h] = ev.src_size.filter(|[w, h]| *w > 0.0 && *h > 0.0).unwrap_or([size.0, size.1]);
                let gname = if !ev.name.is_empty() { ev.name.clone() } else if !g.preset.is_empty() { g.preset.clone() } else { g.name.clone() };
                let item = MediaItem {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: if gname.is_empty() { "Generated Media".into() } else { gname },
                    media_type: MediaType::Image,
                    duration: ev.length.max(duration),
                    generator: Some(fx),
                    width: Some(w as u32),
                    height: Some(h as u32),
                    ..Default::default()
                };
                let id = item.id.clone();
                p.media_pool.push(item);
                id
            } else if ev.path.is_empty() {
                warnings.push(format!("{at} without media was skipped"));
                continue;
            } else {
                media_id(&mut p.media_pool, probed, &ev.path, video, &mut warnings)
            };
            let m = p.media_pool.iter().find(|m| m.id == id).expect("just added");
            let mut c = Clip::from_media(m, start);
            c.duration = duration;
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
            (c.muted, c.locked, c.looping, c.normalize) = (ev.mute, ev.locked, ev.looping, ev.normalize && !video);
            if video {
                c.set_opacity(ev.gain.clamp(0.0, 1.0) as f32);
            } else {
                c.gain_db = db(ev.gain);
            }
            c.group = ev.group.map(|g| groups.entry(g).or_insert_with(|| uuid::Uuid::new_v4().to_string()).clone());
            // R3b: Stream ▸ plays the same stream of the file.
            c.stream = ev.stream.max(0) as u32;
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
            if video {
                c.fx = media_fx.get(ev.path.as_str()).map(|chain| fresh(chain)).unwrap_or_default();
                c.fx.extend(ev.fx.iter().map(|f| fx_from_vegas(f, &format!("{at}: event FX"), &mut warnings)));
                c.transition = ev.transition_in.as_ref().map(|t| transition_from_vegas(t, &format!("{at}: transition"), &mut warnings));
                c.transition_out = ev.transition_out.as_ref().map(|t| transition_from_vegas(t, &format!("{at}: transition"), &mut warnings));
                for env in &ev.envelopes {
                    if env.kind != "Velocity" {
                        warnings.push(format!("{at}: the {} envelope was not imported", env.kind));
                        continue;
                    }
                    let keys = env.points.iter().map(|pt| Keyframe { t: snap(pt.t), v: env_value(EnvKind::Velocity, pt.v, env.neutral), curve: curve(&pt.curve, Curve::Linear) }).collect();
                    c.envelopes.push(Envelope { kind: EnvKind::Velocity, points: Keyframes { keys } });
                }
            } else {
                c.fx = ev.fx.iter().filter_map(|f| audio_fx_from_vegas(f, &format!("{at}: event FX"), &mut warnings)).collect();
            }
            tr.clips.push(c);
        }
        p.timeline.tracks_mut(video).push(tr);
    }
    let tl = &mut p.timeline;
    tl.master.video_fx = e.output_fx.iter().map(|f| fx_from_vegas(f, "Video output FX", &mut warnings)).collect();
    tl.master.audio_fx = e.master_fx.iter().filter_map(|f| audio_fx_from_vegas(f, "Master bus FX", &mut warnings)).collect();
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

    /// R25: the version 2 script's FX, transitions, generators, Track Motion, envelopes, compositing, settings and
    /// audio FX.
    const FIXTURE_V2: &str = r#"{
        "format": "milan-vegas", "version": 2, "name": "FX Show",
        "project": { "w": 1920, "h": 1080, "fps": 25, "sample_rate": 48000, "field_order": "UpperFieldFirst", "pixel_aspect": 1.0926,
                     "quality": "Good", "motion_blur": "Pyramid", "deinterlace": "InterpolateFields", "bit_depth": 24, "bus_mode": "Surround51" },
        "output_fx": [ { "id": "{Svfx:com.sonycreativesoftware:brightnessandcontrast}", "name": "VEGAS Brightness and Contrast", "bypass": true,
                         "params": [ { "name": "Brightness", "type": "Double", "v": 0.1 } ] } ],
        "master_fx": [ { "id": "", "name": "VEGAS Wave Hammer", "bypass": false }, { "id": "", "name": "Acoustic Mirror" } ],
        "media_fx": [ { "path": "C:\\m\\a.mp4", "fx": [ { "id": "{Svfx:com.sonycreativesoftware:invert}", "name": "VEGAS Invert",
                        "params": [ { "name": "BlendAmount", "type": "Double", "v": 0.75 } ] } ] } ],
        "tracks": [
            { "kind": "video", "name": "Parent", "mode": "SrcAlpha", "parent": true, "parent_mode": "Multiply",
              "parent_motion": [ { "t": 0, "x": 0, "y": 0, "w": 1920, "h": 1080, "curve": "Smooth" } ],
              "fx": [ { "id": "{Svfx:com.sonycreativesoftware:gaussianblur}", "name": "VEGAS Gaussian Blur",
                        "params": [ { "name": "HorizontalRange", "type": "Double", "v": 0.06, "keys": [ { "t": 0, "v": 0.06, "curve": "Linear" }, { "t": 2, "v": 0.0, "curve": "Hold" } ] },
                                    { "name": "VerticalRange", "type": "Double", "v": 0.01 }, { "name": "R", "type": "Boolean", "v": true } ] },
                      { "id": "{Svfx:com.sonycreativesoftware:radialpixelate}", "name": "VEGAS Radial Pixelate", "preset": "Fine",
                        "params": [ { "name": "Amount", "type": "Double", "v": 0.3 }, { "name": "Tint", "type": "RGBA", "v": [1, 0.5, 0.25, 1] },
                                    { "name": "Center", "type": "Double2D", "v": [0.5, 0.5], "keys": [ { "t": 0, "v": [0.5, 0.5] }, { "t": 1, "v": [0.25, 0.75], "curve": "Smooth" } ] },
                                    { "name": "Label", "type": "String", "v": "grainy" } ] } ],
              "motion": [ { "t": 0, "x": 0, "y": 0, "w": 1920, "h": 1080, "rz": 0, "curve": "Smooth" },
                          { "t": 2, "x": 192, "y": -108, "w": 960, "h": 540, "rz": 10, "curve": "Linear" } ],
              "shadow": true,
              "events": [
                { "start": 0, "length": 4, "path": "C:\\m\\a.mp4", "stream": 1, "src_size": [1920, 1080],
                  "fx": [ { "id": "{Svfx:com.sonycreativesoftware:crop}", "name": "VEGAS Crop",
                            "params": [ { "name": "XScale", "type": "Double", "v": 0.5 }, { "name": "YScale", "type": "Double", "v": 0.5 }, { "name": "Location", "type": "Double2D", "v": [0.75, 0.25] } ] } ],
                  "envelopes": [ { "kind": "Velocity", "neutral": 1, "points": [ { "t": 0, "v": 1, "curve": "Linear" }, { "t": 2, "v": 0, "curve": "None" } ] },
                                 { "kind": "TransitionProgress", "neutral": 0, "points": [] } ],
                  "transition_in": { "id": "{Svfx:com.sonycreativesoftware:push}", "name": "VEGAS Push", "params": [ { "name": "Direction", "type": "Choice", "v": 3 } ] },
                  "transition_out": { "id": "{Svfx:com.sonycreativesoftware:flash}", "name": "VEGAS Flash", "params": [ { "name": "Tint", "type": "RGB", "v": [1, 1, 0] } ] } },
                { "start": 4, "length": 2, "path": "", "name": "", "src_size": [1920, 1080],
                  "generator": { "id": "{Svfx:com.sonycreativesoftware:solidcolor}", "name": "VEGAS Solid Color", "preset": "Red",
                                 "params": [ { "name": "Color", "type": "RGBA", "v": [1, 0, 0, 1] } ] },
                  "transition_in": { "id": "{Svfx:com.sonycreativesoftware:pageloop}", "name": "VEGAS Page Loop" } },
                { "start": 6, "length": 2, "path": "", "name": "Title",
                  "generator": { "id": "{Svfx:com.sonycreativesoftware:titlesandtext}", "name": "VEGAS Titles & Text",
                                 "params": [ { "name": "Text", "type": "String", "v": "{\\rtf1\\ansi\\ansicpg1252\\deff0{\\fonttbl{\\f0\\fnil\\fcharset0 Georgia;}}\n\\viewkind4\\uc1\\pard\\lang1033\\b\\f0\\fs96 Fall \\'e9 Down\\par\n}" },
                                             { "name": "Location", "type": "Double2D", "v": [0.5, 0.2] }, { "name": "Scale", "type": "Double", "v": 1 },
                                             { "name": "TextColor", "type": "RGBA", "v": [1, 1, 0, 1] } ] } },
                { "start": 8, "length": 1, "path": "",
                  "generator": { "id": "{Svfx:com.sonycreativesoftware:gradientwipe}", "name": "VEGAS Odd Generator" } } ] },
            { "kind": "video", "name": "Child", "child": true, "mode": "Add", "events": [] },
            { "kind": "audio", "name": "Music",
              "fx": [ { "id": "", "name": "VEGAS Track EQ" }, { "id": "", "name": "VEGAS Track Compressor", "bypass": true }, { "id": "", "name": "Vinyl Restoration" } ],
              "events": [ { "start": 0, "length": 4, "path": "C:\\m\\a.mp4", "normalize": true, "loop": false,
                            "fx": [ { "id": "", "name": "Sony Reverb", "preset": "Large Hall" } ] } ] }
        ],
        "markers": [], "regions": [], "unsupported": []
    }"#;

    fn probed() -> HashMap<String, Result<MediaItem, String>> {
        let item = |path: &str, t| Ok(MediaItem { id: uuid::Uuid::new_v4().to_string(), path: path.into(), name: path[5..].into(), media_type: t, duration: 60.0, audio_streams: 2, ..Default::default() });
        HashMap::from([
            (r"C:\m\a.mp4".to_string(), item(r"C:\m\a.mp4", MediaType::Video)),
            (r"C:\m\b.mp4".to_string(), item(r"C:\m\b.mp4", MediaType::Video)),
            (r"C:\m\gone.wav".to_string(), Err("file not found".to_string())),
        ])
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn val(fx: &Fx, key: &str) -> f32 {
        fx.params.get(key).unwrap_or_else(|| panic!("{key} in {:?}", fx.params.keys())).keys[0].v
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
        // A version 1 export names no audio FX: VEGAS's neutral chain.
        assert_eq!(a.fx.len(), 3);
        let env = |k| &a.envelopes.iter().find(|e| e.kind == k).unwrap().points.keys;
        assert!((env(EnvKind::Volume)[0].v + 6.0206).abs() < 1e-3 && env(EnvKind::Volume)[0].curve == Curve::Smooth);
        // +12 dB is above Milan's +6 dB ceiling.
        assert_eq!(env(EnvKind::Volume)[1].v, 6.0);
        // Mute switches, whatever curve VEGAS stored.
        assert_eq!(env(EnvKind::Mute).iter().map(|k| (k.v, k.curve)).collect::<Vec<_>>(), [(1.0, Curve::Hold), (0.0, Curve::Hold)]);
        assert_eq!(env(EnvKind::Pan)[0].v, 0.5);
        let (a0, a1) = (&a.clips[0], &a.clips[1]);
        assert!((a0.gain_db + 6.0206).abs() < 1e-3 && a0.group.is_some() && a0.group == c.group);
        // R3b: the second audio stream plays, as in VEGAS.
        assert_eq!(a0.stream, 1);
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
        assert!(has("Gaussian Blur") && !has("FadeToColor") && has("midi"), "{w:?}");
        assert!(has(r"Media offline: C:\m\gone.wav (file not found)"), "{w:?}");
        assert_eq!(w.len(), 3, "{w:?}");
    }

    /// R25: FX with parameters and keyframes land on the mapped built-ins; the unmapped one is a disabled OFX
    /// placeholder that keeps every parameter and is named in the report.
    #[test]
    fn v2_fx_map_or_become_placeholders() {
        let e = read_vegas_json(FIXTURE_V2).unwrap();
        let (p, w) = to_project(&e, &probed());
        let v = &p.timeline.video_tracks[0];
        assert_eq!(v.fx.len(), 2);
        let blur = &v.fx[0];
        assert_eq!((blur.plugin.as_str(), blur.enabled), ("milan.fx.gaussian_blur", true));
        let h = &blur.params["h_range"].keys;
        assert_eq!(h.len(), 2);
        assert!((h[0].v - 0.012).abs() < 1e-6 && h[1].t == 2.0 && h[1].v == 0.0 && h[1].curve == Curve::Hold);
        assert!((val(blur, "v_range") - 0.002).abs() < 1e-6);
        // The channel flags have no Milan counterpart and are dropped.
        assert!(!blur.params.contains_key("R"));
        let grain = &v.fx[1];
        assert_eq!((grain.plugin.as_str(), grain.enabled), ("ofx:com.sonycreativesoftware:radialpixelate@1", false));
        assert_eq!(val(grain, "Amount"), 0.3);
        assert_eq!((val(grain, "Tint.r"), val(grain, "Tint.g"), val(grain, "Tint.b"), val(grain, "Tint.a")), (1.0, 0.5, 0.25, 1.0));
        let cy = &grain.params["Center.y"].keys;
        assert!(cy.len() == 2 && cy[1].v == 0.75 && cy[1].curve == Curve::Smooth);
        assert_eq!(grain.text["Label"], "grainy");
        assert!(w.iter().any(|x| x == "Parent: track FX: VEGAS Radial Pixelate (com.sonycreativesoftware:radialpixelate) is not available in Milan Pro; kept disabled as an OFX placeholder"), "{w:?}");

        // The placeholder survives a save and load as it is.
        let json = serde_json::to_string(&p).unwrap();
        let back: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(back.timeline.video_tracks[0].fx[1], *grain);
        assert!(crate::engine::ofx::is_bypassed(&grain.plugin));

        // Crop: the kept rectangle (right half, upper half; y up) as left/top/right/bottom.
        let c = &v.clips[0];
        // Media FX first, then the event's own.
        assert_eq!(c.fx.len(), 2);
        assert_eq!((c.fx[0].plugin.as_str(), val(&c.fx[0], "amount")), ("milan.fx.invert", 0.75));
        let crop = &c.fx[1];
        assert_eq!(crop.plugin, "milan.fx.crop");
        assert_eq!((val(crop, "left"), val(crop, "right"), val(crop, "top"), val(crop, "bottom")), (0.5, 0.0, 0.5, 0.0));
        assert_eq!(c.stream, 1);
        // Video output FX (bypassed) and master audio FX by name.
        let out = &p.timeline.master.video_fx;
        assert_eq!((out.len(), out[0].plugin.as_str(), out[0].enabled, val(&out[0], "brightness")), (1, "milan.fx.brightness_contrast", false, 0.1));
        assert_eq!(p.timeline.master.audio_fx.iter().map(|f| f.plugin.as_str()).collect::<Vec<_>>(), ["milan.afx.limiter"]);
        assert!(w.iter().any(|x| x.contains("Acoustic Mirror is not available")), "{w:?}");
        assert!(w.iter().any(|x| x.starts_with("Media FX on a.mp4 (VEGAS Invert)")), "{w:?}");
    }

    #[test]
    fn v2_transitions_generators_and_envelopes() {
        let e = read_vegas_json(FIXTURE_V2).unwrap();
        let (p, w) = to_project(&e, &probed());
        let v = &p.timeline.video_tracks[0];
        let c = &v.clips[0];
        // Push Right (VEGAS 3) is Milan's Right (1); Flash is Milan's own Flash (MVFX), tinted yellow.
        let tin = c.transition.as_ref().unwrap();
        assert_eq!((tin.plugin.as_str(), tin.params["direction"]), ("milan.tr.push", 1.0));
        let tout = c.transition_out.as_ref().unwrap();
        assert_eq!(tout.plugin, "milan.tr.flash");
        assert_eq!((tout.params["tint.r"], tout.params["tint.g"], tout.params["tint.b"]), (1.0, 1.0, 0.0));
        // Velocity comes across (a 0 % hold is a freeze frame); Transition Progress is reported.
        let vel = c.envelopes.iter().find(|e| e.kind == EnvKind::Velocity).unwrap();
        assert_eq!(vel.points.keys.iter().map(|k| (k.t, k.v, k.curve)).collect::<Vec<_>>(), [(0.0, 1.0, Curve::Linear), (2.0, 0.0, Curve::Hold)]);
        assert!(w.iter().any(|x| x.contains("TransitionProgress envelope was not imported")), "{w:?}");

        // Generated media: a pool item per generator, the event's length, the frame size.
        let media = |c: &Clip| p.media_pool.iter().find(|m| Some(&m.id) == c.media_id.as_ref()).unwrap();
        let solid = media(&v.clips[1]);
        let g = solid.generator.as_ref().unwrap();
        assert_eq!((solid.media_type, solid.duration, solid.width, solid.name.as_str()), (MediaType::Image, 2.0, Some(1920), "Red"));
        assert_eq!((g.plugin.as_str(), val(g, "color.r"), val(g, "color.g")), ("milan.gen.solid", 1.0, 0.0));
        // An unmapped transition is a crossfade for now.
        let loop_tr = v.clips[1].transition.as_ref().unwrap();
        assert_eq!(loop_tr.plugin, "ofx:com.sonycreativesoftware:pageloop@1");
        assert!(w.iter().any(|x| x.contains("VEGAS Page Loop") && x.contains("crossfade")), "{w:?}");
        // Titles & Text: plain text and the font out of the RTF, the y-up location flipped, the size scaled.
        let title = media(&v.clips[2]).generator.as_ref().unwrap();
        assert_eq!((title.plugin.as_str(), title.text["text"].as_str(), title.text["font"].as_str()), ("milan.gen.text", "Fall é Down", "Georgia"));
        assert!((val(title, "y") - 0.8).abs() < 1e-6 && (val(title, "size") - 0.1).abs() < 1e-6 && val(title, "color.b") == 0.0);
        assert_eq!(media(&v.clips[2]).name, "Title");
        // An unmapped generator: a disabled placeholder item, named in the report.
        let odd = media(&v.clips[3]).generator.as_ref().unwrap();
        assert_eq!((odd.plugin.as_str(), odd.enabled), ("ofx:com.sonycreativesoftware:gradientwipe@1", false));
        assert!(w.iter().any(|x| x.contains("generated media: VEGAS Odd Generator")), "{w:?}");
    }

    #[test]
    fn v2_track_motion_compositing_settings_and_audio_fx() {
        let e = read_vegas_json(FIXTURE_V2).unwrap();
        let (p, w) = to_project(&e, &probed());
        let s = &p.settings;
        assert_eq!((s.field_order, s.par, s.render_quality, s.motion_blur, s.deinterlace, s.bit_depth), (FieldOrder::Upper, 1.0926, crate::decode::Quality::Good, BlurType::Pyramid, Deinterlace::Interpolate, 24));
        assert!(w.iter().any(|x| x.starts_with("Master bus mode Surround51")), "{w:?}");
        let v = &p.timeline.video_tracks[0];
        // Track Motion: the full-frame key and a half-size key down and to the right (VEGAS's y is up), turned 10 degrees.
        let m = &v.motion.keys;
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].v, Motion::default());
        let k = m[1].v;
        assert!((k.x - 0.1).abs() < 1e-6 && (k.y - 0.1).abs() < 1e-6 && k.w == 0.5 && k.h == 0.5 && k.rot == -10.0 && m[1].curve == Curve::Linear, "{k:?}");
        assert!(v.shadow.on && !v.glow.on);
        // Parent/child compositing.
        assert!(!v.child && v.parent_mode == Composite::Multiply && v.parent_motion.keys.is_empty());
        let child = &p.timeline.video_tracks[1];
        assert!(child.child && child.composite == Composite::Add);
        // Audio FX by name: Track EQ and a bypassed Track Compressor; Vinyl Restoration is left out and reported.
        let a = &p.timeline.audio_tracks[0];
        assert_eq!(a.fx.iter().map(|f| (f.plugin.as_str(), f.enabled)).collect::<Vec<_>>(), [("milan.afx.eq", true), ("milan.afx.compressor", false)]);
        assert!(w.iter().any(|x| x == "Music: track FX: audio FX Vinyl Restoration is not available in Milan Pro and was left out"), "{w:?}");
        let c = &a.clips[0];
        assert!(c.normalize && !c.looping);
        assert_eq!(c.fx.iter().map(|f| f.plugin.as_str()).collect::<Vec<_>>(), ["milan.afx.reverb"]);
    }

    #[test]
    fn rtf_becomes_plain_text() {
        let rtf = "{\\rtf1\\ansi\\ansicpg1252\\deff0{\\fonttbl{\\f0\\fnil\\fcharset0 Arial;}}\n\\viewkind4\\uc1\\pard\\lang1033\\fs96 Drop Split\\par\nSecond \\{line\\}\\par\n}";
        assert_eq!(rtf_plain(rtf), Some(("Drop Split\nSecond {line}".to_string(), Some("Arial".to_string()))));
        assert_eq!(rtf_plain("plain"), None);
        assert_eq!(rtf_plain("{\\rtf1 x}").unwrap().0, "x");
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
}
