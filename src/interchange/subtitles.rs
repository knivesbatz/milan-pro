//! R39: subtitles. VEGAS has no subtitle track: a subtitle is a Titles & Text event (`milan.gen.text`, its
//! "Subtitle" preset) on a video track, so it is edited, keyed and burned in as any title. This reads SubRip (.srt),
//! WebVTT (.vtt) and Advanced SubStation Alpha (.ass / .ssa: its styles become Titles & Text settings) into cues,
//! makes the events (Insert › Subtitles from File...), reads a track's Titles & Text events back as cues and writes
//! SRT or WebVTT (File › Export › Subtitles..., and Render As's sidecar or embedded subtitle stream). Pure, but for
//! the remux that embeds a stream.
//!
//! Not supported, with the reason: CEA-608/708 closed captions (FFmpeg has no CEA-608/708 encoder, and its decoder
//! reads them only out of the video stream's side data, which Milan Pro's decode does not keep); ASS karaoke, drawing
//! commands, per-character overrides and bold / italic (Titles & Text draws one font, one style per event).
use std::collections::HashMap;
use std::path::Path;
use crate::edit;
use crate::engine::plugins;
use crate::project::{Clip, Curve, Fx, Keyframe, Keyframes, MediaItem, MediaType, Project, Track, TrackKind};

/// The screenshot harness's subtitle file (`MILAN_DIALOG=subtitles`), with two styles.
pub const DEMO_ASS: &str = r#"[Script Info]
Title: Milan Demo
ScriptType: v4.00+
PlayResX: 1920
PlayResY: 1080

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Arial,54,&H00FFFFFF,&H000000FF,&H00000000,&H80000000,0,0,0,0,100,100,0,0,1,3,2,2,20,20,40,1
Style: Sign,Georgia,72,&H0000FFFF,&H000000FF,&H00402000,&H00000000,-1,0,0,0,100,100,0,0,1,2,0,8,10,10,60,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:00.50,0:00:02.40,Sign,,0,0,0,,MILAN PRO
Dialogue: 0,0:00:01.00,0:00:03.20,Default,,0,0,0,,Every subtitle is a Titles & Text event
Dialogue: 0,0:00:03.40,0:00:05.80,Default,,0,0,0,,with the file's style:\Nfont, size, colours and position
Dialogue: 0,0:00:06.00,0:00:08.00,Default,,0,0,0,,{\an8}Moved to the top by an override
"#;

/// The generator a subtitle is made with.
pub const TEXT: &str = "milan.gen.text";

/// The formats read; SRT and WebVTT are also written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Format {
    #[default]
    Srt,
    Vtt,
    Ass,
}

impl Format {
    pub fn of(path: &Path) -> Option<Format> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "srt" => Some(Format::Srt),
            "vtt" => Some(Format::Vtt),
            "ass" | "ssa" => Some(Format::Ass),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Format::Srt => "SubRip (.srt)",
            Format::Vtt => "WebVTT (.vtt)",
            Format::Ass => "Advanced SubStation Alpha (.ass)",
        }
    }

    pub fn ext(self) -> &'static str {
        match self {
            Format::Srt => "srt",
            Format::Vtt => "vtt",
            Format::Ass => "ass",
        }
    }
}

/// One subtitle: seconds from the file's zero, its text (lines split by '\n'), and (ASS) its style's name and an
/// `{\anN}` alignment override.
#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub style: Option<String>,
    pub align: Option<u8>,
}

impl Cue {
    pub fn new(start: f64, end: f64, text: &str) -> Self {
        Self { start, end, text: text.into(), style: None, align: None }
    }
}

/// An ASS style, in script pixels (`Subs::res`).
#[derive(Debug, Clone, PartialEq)]
pub struct Style {
    pub name: String,
    pub font: String,
    pub size: f64,
    /// RGBA 0..1.
    pub primary: [f32; 4],
    pub outline_color: [f32; 4],
    pub back: [f32; 4],
    pub bold: bool,
    pub italic: bool,
    pub outline: f64,
    pub shadow: f64,
    /// Numpad: 1-3 bottom, 4-6 middle, 7-9 top; left, centre, right.
    pub alignment: u8,
    pub margin: (f64, f64, f64),
}

impl Default for Style {
    fn default() -> Self {
        let (w, b) = ([1.0, 1.0, 1.0, 1.0], [0.0, 0.0, 0.0, 1.0]);
        Self { name: "Default".into(), font: "Arial".into(), size: 20.0, primary: w, outline_color: b, back: b, bold: false, italic: false, outline: 2.0, shadow: 2.0, alignment: 2, margin: (10.0, 10.0, 10.0) }
    }
}

/// A parsed file: its cues in file order, and (ASS) its styles and script resolution.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Subs {
    pub format: Format,
    pub cues: Vec<Cue>,
    pub styles: Vec<Style>,
    pub res: (f64, f64),
}

/// Read `path` (UTF-8, a BOM allowed; else Windows-1252-ish bytes are taken as Latin-1).
pub fn read(path: &Path) -> Result<Subs, String> {
    let f = Format::of(path).ok_or_else(|| format!("{} is not a subtitle file Milan Pro reads (.srt, .vtt, .ass, .ssa)", path.display()))?;
    let bytes = std::fs::read(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    let text = match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => e.into_bytes().iter().map(|&b| b as char).collect(),
    };
    parse(&text, f)
}

pub fn parse(text: &str, f: Format) -> Result<Subs, String> {
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n");
    let subs = match f {
        Format::Srt => Subs { format: f, cues: parse_blocks(&text, false), ..Default::default() },
        Format::Vtt => {
            if !text.trim_start().starts_with("WEBVTT") {
                return Err("Not a WebVTT file: it does not start with WEBVTT".into());
            }
            Subs { format: f, cues: parse_blocks(&text, true), ..Default::default() }
        }
        Format::Ass => parse_ass(&text)?,
    };
    if subs.cues.is_empty() {
        return Err("The file has no subtitles Milan Pro can read".into());
    }
    Ok(subs)
}

/// "01:02:03,456", "01:02:03.456", "02:03.456" (WebVTT without hours), "1:02:03.45" (ASS centiseconds).
pub fn parse_time(s: &str) -> Option<f64> {
    let s = s.trim();
    let (hms, frac) = s.rsplit_once([',', '.']).unwrap_or((s, ""));
    let parts: Vec<&str> = hms.split(':').collect();
    let n = |p: &str| p.trim().parse::<u64>().ok();
    let secs = match parts.as_slice() {
        [h, m, s] => n(h)? * 3600 + n(m)? * 60 + n(s)?,
        [m, s] => n(m)? * 60 + n(s)?,
        _ => return None,
    };
    let frac = if frac.is_empty() { 0.0 } else { frac.parse::<u64>().ok()? as f64 / 10f64.powi(frac.len() as i32) };
    Some(secs as f64 + frac)
}

/// Milliseconds, as files store them.
fn ms(t: f64) -> i64 {
    (t * 1000.0).round() as i64
}

/// "HH:MM:SS<sep>mmm".
pub fn format_time(t: f64, sep: char) -> String {
    let m = ms(t.max(0.0));
    format!("{:02}:{:02}:{:02}{sep}{:03}", m / 3_600_000, m / 60_000 % 60, m / 1000 % 60, m % 1000)
}

/// SRT and WebVTT blocks: an optional id line, "start --> end [settings]", then the text up to a blank line.
fn parse_blocks(text: &str, vtt: bool) -> Vec<Cue> {
    let mut cues = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let Some(k) = (i..lines.len().min(i + 2)).find(|&k| lines[k].contains("-->")) else {
            // A WebVTT NOTE / STYLE / REGION block, the header or junk: skip to the next blank line.
            while i < lines.len() && !lines[i].trim().is_empty() {
                i += 1;
            }
            i += 1;
            continue;
        };
        let (a, b) = lines[k].split_once("-->").expect("checked");
        let end = b.split_whitespace().next().unwrap_or("");
        i = k + 1;
        let mut body = Vec::new();
        while i < lines.len() && !lines[i].trim().is_empty() {
            body.push(lines[i]);
            i += 1;
        }
        if let (Some(s), Some(e)) = (parse_time(a), parse_time(end)) {
            let raw = body.join("\n");
            let text = if vtt { vtt_text(&raw) } else { strip_tags(&raw) };
            cues.push(Cue::new(s, e, &text));
        }
    }
    cues
}

/// SRT's styling tags (<i>, <b>, <u>, <font ...>) and an ASS `{\anN}` left out (Titles & Text draws plain text).
fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find(['<', '{']) {
        out.push_str(&rest[..i]);
        let close = if rest[i..].starts_with('<') { '>' } else { '}' };
        let tag = rest[i..].find(close).map(|j| &rest[i..=i + j]);
        let known = tag.is_some_and(|t| {
            let l = t.to_ascii_lowercase();
            ["<i>", "</i>", "<b>", "</b>", "<u>", "</u>", "</font>"].contains(&l.as_str()) || l.starts_with("<font ") || l.starts_with("{\\")
        });
        match tag {
            Some(t) if known => rest = &rest[i + t.len()..],
            _ => {
                out.push(rest[i..].chars().next().expect("found"));
                rest = &rest[i + 1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// WebVTT cue text: tags left out, entities decoded.
fn vtt_text(s: &str) -> String {
    let mut out = String::new();
    let mut tag = false;
    for c in s.chars() {
        match c {
            '<' => tag = true,
            '>' if tag => tag = false,
            c if !tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&lt;", "<").replace("&gt;", ">").replace("&nbsp;", "\u{a0}").replace("&lrm;", "\u{200e}").replace("&rlm;", "\u{200f}").replace("&amp;", "&")
}

/// SRT: numbered from 1, "-->" between the times, a blank line after each cue.
pub fn write_srt(cues: &[Cue]) -> String {
    let mut s = String::new();
    for (i, c) in cues.iter().enumerate() {
        s.push_str(&format!("{}\n{} --> {}\n{}\n\n", i + 1, format_time(c.start, ','), format_time(c.end, ','), c.text));
    }
    s
}

/// WebVTT: the header, then each cue (its text's &, < and > escaped).
pub fn write_vtt(cues: &[Cue]) -> String {
    let mut s = String::from("WEBVTT\n\n");
    for c in cues {
        let text = c.text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
        s.push_str(&format!("{} --> {}\n{text}\n\n", format_time(c.start, '.'), format_time(c.end, '.')));
    }
    s
}

pub fn write(cues: &[Cue], f: Format) -> String {
    if f == Format::Vtt { write_vtt(cues) } else { write_srt(cues) }
}

/// "&H00BBGGRR" (alpha 00 opaque), "&HBBGGRR&" or an SSA decimal as RGBA.
fn ass_color(s: &str) -> Option<[f32; 4]> {
    let t = s.trim().trim_end_matches('&');
    let v = match t.strip_prefix("&H").or_else(|| t.strip_prefix("&h")) {
        Some(h) => u32::from_str_radix(h, 16).ok()?,
        None => t.parse::<i64>().ok()? as u32,
    };
    let c = |sh: u32| ((v >> sh) & 0xff) as f32 / 255.0;
    Some([c(0), c(8), c(16), 1.0 - c(24)])
}

/// The fields of a "Key: a,b,c" line by the section's Format line; the last field takes the rest (Text has commas).
fn fields<'a>(format: &[String], value: &'a str) -> HashMap<String, &'a str> {
    let parts: Vec<&str> = value.splitn(format.len().max(1), ',').collect();
    format.iter().cloned().zip(parts.into_iter().map(str::trim)).collect()
}

fn parse_ass(text: &str) -> Result<Subs, String> {
    let mut subs = Subs { format: Format::Ass, res: (384.0, 288.0), ..Default::default() };
    let (mut section, mut sfmt, mut efmt) = (String::new(), Vec::<String>::new(), Vec::<String>::new());
    let mut res = (None, None);
    let low = |v: &str| v.split(',').map(|f| f.trim().to_ascii_lowercase()).collect::<Vec<_>>();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            section = line.to_ascii_lowercase();
            continue;
        }
        let Some((key, value)) = line.split_once(':') else { continue };
        let value = value.trim_start();
        match (section.as_str(), key.trim().to_ascii_lowercase().as_str()) {
            ("[script info]", "playresx") => res.0 = value.trim().parse().ok(),
            ("[script info]", "playresy") => res.1 = value.trim().parse().ok(),
            ("[v4+ styles]" | "[v4 styles]", "format") => sfmt = low(value),
            ("[v4+ styles]" | "[v4 styles]", "style") => {
                let f = fields(&sfmt, value);
                let num = |k: &str| f.get(k).and_then(|v| v.parse::<f64>().ok());
                let d = Style::default();
                let ssa = section == "[v4 styles]";
                // SSA's alignment: 1-3 bottom, +4 top (5-7), +8 middle (9-11).
                let align = num("alignment").map_or(d.alignment, |a| {
                    let a = a as u8;
                    if ssa { [0, 1, 2, 3, 0, 7, 8, 9, 0, 4, 5, 6].get(a as usize).copied().filter(|v| *v > 0).unwrap_or(2) } else { a.clamp(1, 9) }
                });
                subs.styles.push(Style {
                    name: f.get("name").map_or(d.name.clone(), |s| s.to_string()),
                    font: f.get("fontname").map_or(d.font.clone(), |s| s.to_string()),
                    size: num("fontsize").unwrap_or(d.size),
                    primary: f.get("primarycolour").and_then(|c| ass_color(c)).unwrap_or(d.primary),
                    outline_color: f.get("outlinecolour").or(f.get("tertiarycolour")).and_then(|c| ass_color(c)).unwrap_or(d.outline_color),
                    back: f.get("backcolour").and_then(|c| ass_color(c)).unwrap_or(d.back),
                    bold: num("bold").is_some_and(|b| b != 0.0),
                    italic: num("italic").is_some_and(|b| b != 0.0),
                    outline: num("outline").unwrap_or(d.outline),
                    shadow: num("shadow").unwrap_or(d.shadow),
                    alignment: align,
                    margin: (num("marginl").unwrap_or(d.margin.0), num("marginr").unwrap_or(d.margin.1), num("marginv").unwrap_or(d.margin.2)),
                });
            }
            ("[events]", "format") => efmt = low(value),
            ("[events]", "dialogue") => {
                let f = fields(&efmt, value);
                let (Some(s), Some(e)) = (f.get("start").and_then(|t| parse_time(t)), f.get("end").and_then(|t| parse_time(t))) else { continue };
                let raw = f.get("text").copied().unwrap_or("");
                let align = raw.find("\\an").and_then(|i| raw[i + 3..].chars().next()).and_then(|c| c.to_digit(10)).map(|d| d as u8).filter(|d| (1..=9).contains(d));
                subs.cues.push(Cue { start: s, end: e, text: ass_text(raw), style: f.get("style").map(|s| s.trim_start_matches('*').to_string()), align });
            }
            _ => {}
        }
    }
    // A missing PlayRes takes the other from 4:3 (the ASS default is 384 x 288).
    subs.res = match res {
        (Some(x), Some(y)) => (x, y),
        (Some(x), None) => (x, x * 0.75),
        (None, Some(y)) => (y * 4.0 / 3.0, y),
        (None, None) => subs.res,
    };
    // Events in time order (ASS keeps them by layer and in any order).
    subs.cues.sort_by(|a, b| a.start.total_cmp(&b.start));
    Ok(subs)
}

/// ASS dialogue text: override blocks `{...}` left out, \N and \n line breaks, \h a hard space.
fn ass_text(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '{' => depth += 1,
            '}' if depth > 0 => depth -= 1,
            c if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.replace("\\N", "\n").replace("\\n", "\n").replace("\\h", "\u{a0}")
}

/// A constant parameter.
fn key(v: f32) -> Keyframes<f32> {
    Keyframes { keys: vec![Keyframe { t: 0.0, v, curve: Curve::Linear }] }
}

fn set_color(fx: &mut Fx, k: &str, c: [f32; 4]) {
    for (ch, v) in ["r", "g", "b", "a"].iter().zip(c) {
        fx.params.insert(format!("{k}.{ch}"), key(v));
    }
}

/// The Titles & Text instance of `cue`: the "Subtitle" preset (bottom centre, a thin outline), or with `style` (an
/// ASS style of a script `res` pixels) its font, size, colours, outline, shadow, alignment and margins.
pub fn title_fx(cue: &Cue, style: Option<&Style>, res: (f64, f64)) -> Fx {
    let info = plugins::find(TEXT).expect("built in");
    let preset = info.presets.iter().find(|p| p.name == "Subtitle");
    let mut fx = plugins::new_fx(info, preset);
    fx.text.insert("text".into(), cue.text.clone());
    let Some(st) = style else {
        if let Some(a) = cue.align {
            place(&mut fx, a, (0.0, 0.0, 0.0), 0.05, cue.text.lines().count().max(1));
        }
        return fx;
    };
    let (rx, ry) = (res.0.max(1.0), res.1.max(1.0));
    let size = (st.size / ry).clamp(0.01, 0.5) as f32;
    fx.text.insert("font".into(), st.font.clone());
    fx.params.insert("size".into(), key(size));
    set_color(&mut fx, "color", st.primary);
    set_color(&mut fx, "outline_color", st.outline_color);
    fx.params.insert("outline".into(), key((st.outline / ry).clamp(0.0, 0.05) as f32));
    let shadow = (st.shadow / ry).clamp(0.0, 0.1) as f32;
    fx.params.insert("shadow".into(), key(if shadow > 0.0 { 1.0 } else { 0.0 }));
    fx.params.insert("shadow_x".into(), key(shadow));
    fx.params.insert("shadow_y".into(), key(shadow));
    fx.params.insert("shadow_blur".into(), key(0.0));
    set_color(&mut fx, "shadow_color", st.back);
    let m = (st.margin.0 / rx, st.margin.1 / rx, st.margin.2 / ry);
    place(&mut fx, cue.align.unwrap_or(st.alignment), m, size, cue.text.lines().count().max(1));
    fx
}

/// Location and alignment from a numpad alignment and margins (fractions of the frame).
fn place(fx: &mut Fx, a: u8, (ml, mr, mv): (f64, f64, f64), size: f32, lines: usize) {
    let a = a.clamp(1, 9) - 1;
    let (col, row) = (a % 3, a / 3);
    let x = match col {
        0 => ml,
        1 => 0.5 + (ml - mr) * 0.5,
        _ => 1.0 - mr,
    };
    // Titles & Text centres its block of lines on y (about 1.2 x the size per line).
    let half = 0.6 * size as f64 * lines as f64;
    let y = match row {
        0 => 1.0 - mv - half,
        1 => 0.5,
        _ => mv + half,
    };
    fx.params.insert("x".into(), key(x.clamp(0.0, 1.0) as f32));
    fx.params.insert("y".into(), key(y.clamp(0.0, 1.0) as f32));
    fx.params.insert("align".into(), key(col as f32));
}

/// Where Insert › Subtitles from File puts the events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// A new video track at the top ("Subtitles").
    NewTrack,
    /// Video track N.
    Track(usize),
}

/// Insert `subs` as Titles & Text events (each its own generated media, as Insert › Generated Media makes) at `offset`
/// seconds plus each cue's time on `target`, with the ASS styles when `styles`. One edit; the new events' ids, or an
/// error when the track is locked.
pub fn insert(p: &mut Project, subs: &Subs, target: Target, offset: f64, styles: bool) -> Result<Vec<String>, String> {
    let ti = match target {
        Target::NewTrack => {
            let mut t = Track::new("Subtitles", TrackKind::Video);
            t.name = "Subtitles".into();
            p.timeline.video_tracks.insert(0, t);
            0
        }
        Target::Track(i) => {
            let t = p.timeline.video_tracks.get(i).ok_or("That video track is gone")?;
            if t.locked {
                return Err("The track is locked".into());
            }
            i
        }
    };
    let (w, h) = (Some(p.settings.width), Some(p.settings.height));
    let mut ids = Vec::new();
    for cue in subs.cues.iter().filter(|c| c.end > c.start) {
        let style = styles.then(|| cue.style.as_ref().and_then(|n| subs.styles.iter().find(|s| s.name.eq_ignore_ascii_case(n))).or(subs.styles.first())).flatten();
        let fx = title_fx(cue, style, subs.res);
        let name = cue.text.lines().next().unwrap_or("Subtitle").chars().take(40).collect::<String>();
        let m = MediaItem { id: uuid::Uuid::new_v4().to_string(), name, media_type: MediaType::Image, duration: 86400.0, generator: Some(fx), width: w, height: h, ..Default::default() };
        let mut c = Clip::from_media(&m, (offset + cue.start).max(0.0));
        let len = cue.end - cue.start;
        (c.duration, c.out_point) = (len, len);
        ids.push(c.id.clone());
        p.timeline.video_tracks[ti].clips.push(c);
        p.media_pool.push(m);
    }
    p.timeline.video_tracks[ti].clips.sort_by(|a, b| a.position.total_cmp(&b.position));
    p.mark_modified();
    Ok(ids)
}

/// R39: Insert Subtitles from Regions: each named region as a subtitle (its name the text).
pub fn regions(p: &Project) -> Subs {
    let cues = p.timeline.regions.iter().filter(|r| r.end > r.start && !r.name.trim().is_empty()).map(|r| Cue::new(r.start, r.end, &r.name)).collect();
    Subs { format: Format::Srt, cues, ..Default::default() }
}

/// The text of a Titles & Text event (its active take's generated media), None for any other event.
pub fn text_of(p: &Project, c: &Clip) -> Option<String> {
    let g = edit::media_of(p, c)?.generator.as_ref().filter(|g| g.plugin == TEXT)?;
    Some(g.text.get("text").cloned().unwrap_or_default())
}

/// The cues of video track `track`'s Titles & Text events over `range` (seconds; cues are clipped to it and counted
/// from its start), in time order. Muted events are left out.
pub fn cues_of(p: &Project, track: usize, range: (f64, f64)) -> Vec<Cue> {
    let Some(t) = p.timeline.video_tracks.get(track) else { return Vec::new() };
    let mut cues: Vec<Cue> = t
        .clips
        .iter()
        .filter(|c| !c.muted)
        .filter_map(|c| {
            let text = text_of(p, c)?;
            let (s, e) = (c.position.max(range.0), (c.position + c.duration).min(range.1));
            (e > s && !text.trim().is_empty()).then(|| Cue::new(s - range.0, e - range.0, &text))
        })
        .collect();
    cues.sort_by(|a, b| a.start.total_cmp(&b.start));
    cues
}

/// The video tracks that hold Titles & Text events: (index, "Video N: name").
pub fn text_tracks(p: &Project) -> Vec<(usize, String)> {
    p.timeline
        .video_tracks
        .iter()
        .enumerate()
        .filter(|(_, t)| t.clips.iter().any(|c| text_of(p, c).is_some()))
        .map(|(i, t)| (i, if t.name.is_empty() { format!("Track {}", i + 1) } else { format!("Track {}: {}", i + 1, t.name) }))
        .collect()
}

/// The soft subtitle codec container `ext` holds: mov_text in MP4 / MOV, SubRip in Matroska, WebVTT in WebM.
pub fn embed_codec(ext: &str) -> Option<&'static str> {
    match ext.to_ascii_lowercase().as_str() {
        "mp4" | "m4v" | "mov" => Some("mov_text"),
        "mkv" => Some("srt"),
        "webm" => Some("webvtt"),
        _ => None,
    }
}

/// Embed `cues` in the rendered `out` as a soft subtitle stream (FFmpeg stream copy of the rest), in place.
pub fn embed(out: &Path, cues: &[Cue], lang: &str) -> Result<(), String> {
    let ext = out.extension().and_then(|e| e.to_str()).unwrap_or("");
    let codec = embed_codec(ext).ok_or_else(|| format!("A .{ext} file cannot hold a subtitle stream: save the subtitles as a file beside it"))?;
    let srt = out.with_extension(format!("{ext}.subs.srt"));
    let tmp = out.with_extension(format!("subs.{ext}"));
    std::fs::write(&srt, write_srt(cues)).map_err(|e| format!("Cannot write {}: {e}", srt.display()))?;
    let mut cmd = crate::proc::ffmpeg();
    cmd.args(["-v", "error", "-y", "-i"]).arg(out).args(["-i"]).arg(&srt);
    cmd.args(["-map", "0", "-map", "1", "-c", "copy", "-c:s", codec, "-metadata:s:s:0", &format!("language={lang}")]);
    if codec == "mov_text" {
        cmd.args(["-movflags", "+faststart"]);
    }
    cmd.arg(&tmp);
    let res = crate::proc::output(&mut cmd);
    let _ = std::fs::remove_file(&srt);
    match res {
        Ok(o) if o.status.success() => std::fs::rename(&tmp, out).map_err(|e| format!("The subtitles are in {} but could not replace {}: {e}", tmp.display(), out.display())),
        Ok(o) => {
            let _ = std::fs::remove_file(&tmp);
            Err(format!("Embedding the subtitles failed: {}", String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or("ffmpeg failed")))
        }
        Err(e) => Err(crate::proc::tool_error(&e, "ffmpeg")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRT: &str = "1\n00:00:01,000 --> 00:00:03,250\nHello, wörld — ✓\n\n2\n00:00:02,500 --> 00:00:05,001\nTwo lines\nof text\n\n3\n01:02:03,456 --> 01:02:04,000\n<i>Overlaps</i> nothing\n\n";

    /// R39 acceptance: an SRT parse / write round trip keeps UTF-8, multi-line text and overlapping cues (the tags of a
    /// styled cue are left out); CRLF and a BOM read the same.
    #[test]
    fn srt_round_trip() {
        let s = parse(SRT, Format::Srt).unwrap();
        assert_eq!(s.cues.len(), 3);
        assert_eq!(s.cues[0], Cue::new(1.0, 3.25, "Hello, wörld — ✓"));
        assert_eq!((s.cues[1].start, s.cues[1].end, s.cues[1].text.as_str()), (2.5, 5.001, "Two lines\nof text"));
        assert!(s.cues[1].start < s.cues[0].end, "overlapping cues are kept");
        assert_eq!(s.cues[2].text, "Overlaps nothing");
        assert_eq!(write_srt(&s.cues), SRT.replace("<i>Overlaps</i>", "Overlaps"));
        let crlf = format!("\u{feff}{}", SRT.replace('\n', "\r\n"));
        assert_eq!(parse(&crlf, Format::Srt).unwrap(), s);
        assert_eq!(format_time(3723.456, ','), "01:02:03,456");
        assert_eq!(parse_time("02:03.5"), Some(123.5));
    }

    #[test]
    fn webvtt_reads_and_writes() {
        let vtt = "WEBVTT - a title\n\nNOTE a comment\nover two lines\n\nintro\n00:01.000 --> 00:02.500 line:90% align:center\n<v Ann>Fish &amp; chips</v>\n\n00:00:03.000 --> 00:00:04.000\nA &lt;b&gt; tag\n";
        let s = parse(vtt, Format::Vtt).unwrap();
        assert_eq!(s.cues, vec![Cue::new(1.0, 2.5, "Fish & chips"), Cue::new(3.0, 4.0, "A <b> tag")]);
        let out = write_vtt(&s.cues);
        assert!(out.starts_with("WEBVTT\n\n00:00:01.000 --> 00:00:02.500\nFish &amp; chips\n"), "{out}");
        assert_eq!(parse(&out, Format::Vtt).unwrap().cues, s.cues);
        assert!(parse("1\n00:00:01,000 --> 00:00:02,000\nx\n", Format::Vtt).is_err());
    }

    const ASS: &str = "[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,54,&H00FFFFFF,&H000000FF,&H00000000,&H80000000,0,0,0,0,100,100,0,0,1,3,2,2,20,20,40,1\nStyle: Sign,Georgia,108,&H0000FFFF,&H000000FF,&H00FF0000,&H00000000,-1,0,0,0,100,100,0,0,1,0,0,8,10,10,60,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:05.00,0:00:07.50,Sign,,0,0,0,,{\\fad(200,200)}TOP SIGN\nDialogue: 0,0:00:01.00,0:00:03.20,Default,,0,0,0,,Hello, there\\Nsecond line\nDialogue: 0,0:00:04.00,0:00:05.00,Default,,0,0,0,,{\\an7}Corner\n";

    /// R39 acceptance: an ASS file imports with its styles mapped to Titles & Text settings: font, size (of PlayResY),
    /// colours (&HAABBGGRR), outline, shadow, alignment and margins; `{\anN}` moves a cue; override tags are left out.
    #[test]
    fn ass_imports_with_styles_mapped() {
        let s = parse(ASS, Format::Ass).unwrap();
        assert_eq!(s.res, (1920.0, 1080.0));
        assert_eq!(s.styles.len(), 2);
        assert_eq!(s.cues.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), ["Hello, there\nsecond line", "Corner", "TOP SIGN"]);
        assert_eq!((s.cues[0].start, s.cues[0].end), (1.0, 3.2));
        let sign = &s.styles[1];
        assert_eq!((sign.font.as_str(), sign.size, sign.primary, sign.alignment, sign.bold), ("Georgia", 108.0, [1.0, 1.0, 0.0, 1.0], 8, true));
        assert_eq!(sign.outline_color, [0.0, 0.0, 1.0, 1.0]);
        let mut p = Project::new("t");
        let ids = insert(&mut p, &s, Target::NewTrack, 10.0, true).unwrap();
        assert_eq!((ids.len(), p.timeline.video_tracks[0].name.as_str(), p.timeline.video_tracks[0].clips.len()), (3, "Subtitles", 3));
        let fx_of = |i: usize| edit::media_of(&p, &p.timeline.video_tracks[0].clips[i]).unwrap().generator.clone().unwrap();
        let v = |fx: &Fx, k: &str| fx.params[k].keys[0].v;
        let first = fx_of(0);
        assert_eq!((p.timeline.video_tracks[0].clips[0].position, first.text["text"].as_str(), first.text["font"].as_str()), (11.0, "Hello, there\nsecond line", "Arial"));
        assert!((v(&first, "size") - 0.05).abs() < 1e-6 && (v(&first, "outline") - 3.0 / 1080.0).abs() < 1e-6 && v(&first, "shadow") == 1.0);
        assert_eq!((v(&first, "align"), v(&first, "color.a")), (1.0, 1.0));
        assert!(v(&first, "y") > 0.85 && v(&first, "y") < 0.97, "bottom centre: {}", v(&first, "y"));
        assert!((v(&first, "shadow_color.a") - (1.0 - 128.0 / 255.0)).abs() < 1e-6);
        let corner = fx_of(1);
        assert_eq!((v(&corner, "align"), v(&corner, "x")), (0.0, 20.0 / 1920.0));
        assert!(v(&corner, "y") < 0.2);
        let top = fx_of(2);
        assert_eq!((top.text["font"].as_str(), v(&top, "align"), v(&top, "color.b"), v(&top, "color.g")), ("Georgia", 1.0, 0.0, 1.0));
        assert!((v(&top, "size") - 0.1).abs() < 1e-6 && v(&top, "y") < 0.2);
        // Without styles: the Subtitle preset.
        let mut q = Project::new("t");
        insert(&mut q, &s, Target::Track(0), 0.0, false).unwrap();
        let plain = edit::media_of(&q, &q.timeline.video_tracks[0].clips[0]).unwrap().generator.clone().unwrap();
        assert!(!plain.text.contains_key("font") && (v(&plain, "y") - 0.9).abs() < 1e-6);
    }

    /// R39 acceptance: SRT import → export is text- and time-exact (the events read back as the same cues, written as
    /// the same file); a muted event and other events on the track are left out; a range clips and shifts the cues.
    #[test]
    fn srt_import_then_export_is_exact() {
        let clean = SRT.replace("<i>Overlaps</i>", "Overlaps");
        let s = parse(&clean, Format::Srt).unwrap();
        let mut p = Project::new("t");
        insert(&mut p, &s, Target::NewTrack, 0.0, false).unwrap();
        p.timeline.video_tracks[0].clips.push(Clip::new("not a title", 0.0, 2.0));
        let back = cues_of(&p, 0, (0.0, f64::INFINITY));
        assert_eq!(back, s.cues);
        assert_eq!(write_srt(&back), clean);
        assert_eq!(text_tracks(&p), vec![(0, "Track 1: Subtitles".to_string())]);
        let part = cues_of(&p, 0, (2.0, 4.0));
        assert_eq!(part, vec![Cue::new(0.0, 1.25, "Hello, wörld — ✓"), Cue::new(0.5, 2.0, "Two lines\nof text")]);
        p.timeline.video_tracks[0].clips[0].muted = true;
        assert_eq!(cues_of(&p, 0, (0.0, f64::INFINITY)).len(), 2);
        // Regions become cues.
        p.timeline.regions.push(crate::project::Region { start: 1.0, end: 2.0, name: "From a region".into() });
        assert_eq!(regions(&p).cues, vec![Cue::new(1.0, 2.0, "From a region")]);
        assert_eq!(embed_codec("MP4"), Some("mov_text"));
        let demo = parse(DEMO_ASS, Format::Ass).unwrap();
        assert_eq!((demo.cues.len(), demo.styles.len(), demo.cues[2].text.as_str()), (4, 2, "with the file's style:\nfont, size, colours and position"));
        assert_eq!(embed_codec("avi"), None);
    }
}
