//! R29: Final Cut Pro X XML, FCPXML 1.10 (File › Import / Export › Final Cut Pro X (*.fcpxml)...).
//!
//! Out: `resources` (one format, one asset per file with its `media-rep`, the Cross Dissolve effect), one library /
//! event / project / sequence whose primary storyline (the spine) is Milan's bottom video track, with the other
//! video tracks as connected storylines on lanes 1, 2, ... and the audio tracks on lanes -1, -2, ..., each clip an
//! `asset-clip` (`srcEnable` picks its video or audio), overlaps as `transition`s straddling the cut, speed changes
//! as a linear `timeMap`, levels as `adjust-blend` / `adjust-volume` (fades as keyframes and fadeIn/fadeOut),
//! markers on the element they fall in. In: the same, plus `clip`, `sync-clip`, `ref-clip` (as an offline event)
//! and connected clips on any lane; titles and generators are listed and dropped. Times are rationals ("1001/30000s").
//! Everything goes through the cut list (`cutlist.rs`).
use std::collections::HashMap;
use std::path::Path;
use crate::interchange::cutlist::{Cut, CutClip, CutMarker, CutTrack, CutTransition, DISSOLVE, from_project};
use crate::interchange::otio::{file_url, url_path};
use crate::interchange::xml::{El, Xml, parse};
use crate::project::{Project, is_drop, rate_ratio};

/// Write `p` as FCPXML 1.10; returns what the file could not hold.
pub fn export(p: &Project, path: &Path) -> anyhow::Result<Vec<String>> {
    let (cut, mut notes) = from_project(p, "FCPXML");
    let (text, more) = write(&cut);
    notes.extend(more);
    std::fs::write(path, text)?;
    Ok(notes)
}

/// A frame-aligned FCPXML time: "5s" or "3003/30000s".
fn ts(seconds: f64, fps: f64) -> String {
    let (num, den) = rate_ratio(fps);
    let frames = (seconds * num as f64 / den as f64).round() as i64;
    let n = frames * den as i64;
    if n % num as i64 == 0 { format!("{}s", n / num as i64) } else { format!("{n}/{num}s") }
}

/// "3003/30000s", "5s", "-1/25s" or "1.5s" in seconds.
pub fn parse_time(s: &str) -> Option<f64> {
    let s = s.trim().strip_suffix('s')?;
    match s.split_once('/') {
        Some((n, d)) => Some(n.trim().parse::<f64>().ok()? / d.trim().parse::<f64>().ok()?),
        None => s.parse().ok(),
    }
}

fn format_name(w: u32, h: u32, fps: f64) -> String {
    let rate = match fps {
        f if (f - 23.976).abs() < 0.01 => "2398".to_string(),
        f if (f - 29.97).abs() < 0.01 => "2997".to_string(),
        f if (f - 59.94).abs() < 0.01 => "5994".to_string(),
        f => format!("{}", f.round() as u32),
    };
    let _ = w;
    format!("FFVideoFormat{h}p{rate}")
}

/// A stable id for a file (FCPXML wants a `uid` on every asset).
fn uid(path: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in path.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016X}")
}

/// One item of a storyline, laid end to end.
enum Item<'a> {
    Gap { offset: f64, duration: f64 },
    Clip { c: &'a CutClip, offset: f64, duration: f64, src: f64 },
    Transition { offset: f64, duration: f64 },
}

impl Item<'_> {
    fn offset(&self) -> f64 {
        match self {
            Item::Gap { offset, .. } | Item::Clip { offset, .. } | Item::Transition { offset, .. } => *offset,
        }
    }

    fn end(&self) -> f64 {
        match self {
            Item::Gap { offset, duration } | Item::Clip { offset, duration, .. } => offset + duration,
            Item::Transition { .. } => f64::NAN,
        }
    }

    /// The element's local time at timeline `t` (markers and anchored items are placed in it).
    fn local(&self, t: f64) -> f64 {
        match self {
            Item::Clip { offset, src, .. } => src + (t - offset),
            Item::Gap { offset, .. } => t - offset,
            Item::Transition { .. } => 0.0,
        }
    }
}

/// A track as storyline items from `from` to `to` (gaps fill; overlaps become transitions straddling the cut).
fn items<'a>(t: Option<&'a CutTrack>, from: f64, to: f64, fps: f64, notes: &mut Vec<String>) -> Vec<Item<'a>> {
    let mut out = Vec::new();
    let clips: Vec<&CutClip> = t.map(|t| t.clips.iter().collect()).unwrap_or_default();
    let n = clips.len();
    let mut starts: Vec<f64> = clips.iter().map(|c| c.start).collect();
    let mut ends: Vec<f64> = clips.iter().map(|c| c.end).collect();
    let mut trans: Vec<Option<(f64, f64)>> = vec![None; n];
    for i in 1..n {
        if ends[i - 1] > starts[i] + 1e-9 {
            let (s, e) = (starts[i], ends[i - 1].min(ends[i]));
            // The cut sits on a frame (FCPXML times are frame-aligned); the dissolve need not be symmetric.
            let mid = crate::edit::quantize((s + e) / 2.0, fps);
            if mid > starts[i - 1] + 1e-9 && mid < ends[i] - 1e-9 {
                (ends[i - 1], starts[i]) = (mid, mid);
                trans[i] = Some((s, e - s));
            } else {
                notes.push(format!("{}: the event at {:.3} s overlaps more than one event; it is cut at its start", t.map_or("", |t| t.name.as_str()), clips[i].start));
                ends[i - 1] = s;
            }
        }
    }
    let mut at = from;
    for (i, c) in clips.iter().enumerate() {
        if starts[i] > at + 1e-9 {
            out.push(Item::Gap { offset: at, duration: starts[i] - at });
        }
        if let Some((o, d)) = trans[i] {
            out.push(Item::Transition { offset: o, duration: d });
        }
        out.push(Item::Clip { c, offset: starts[i], duration: ends[i] - starts[i], src: c.src_in + (starts[i] - c.start) * c.rate });
        at = ends[i];
    }
    if to > at + 1e-9 || out.is_empty() {
        out.push(Item::Gap { offset: at, duration: (to - at).max(0.0) });
    }
    out
}

/// FCPXML text of `cut`, and what it left out.
pub fn write(cut: &Cut) -> (String, Vec<String>) {
    let fps = if cut.fps > 0.0 { cut.fps } else { 29.97 };
    let t = |s: f64| ts(s, fps);
    let mut notes = Vec::new();
    let (num, den) = rate_ratio(fps);
    let (w, h) = (cut.width.max(1), cut.height.max(1));
    let total = cut.tracks.iter().flat_map(|c| &c.clips).map(|c| c.end).fold(0.0, f64::max).max(1.0 / fps);
    let mut x = Xml::new("<!DOCTYPE fcpxml>");
    x.open("fcpxml", &[("version", "1.10")]);
    x.open("resources", &[]);
    let fd = format!("{den}/{num}s");
    x.empty("format", &[("id", "r1"), ("name", &format_name(w, h, fps)), ("frameDuration", &fd), ("width", &w.to_string()), ("height", &h.to_string())]);
    let mut assets: HashMap<String, String> = HashMap::new(); // path (or #name) → id
    let mut n = 1;
    for c in cut.tracks.iter().flat_map(|t| &t.clips) {
        let key = if c.path.is_empty() { format!("#{}", c.name) } else { c.path.clone() };
        if assets.contains_key(&key) {
            continue;
        }
        n += 1;
        let id = format!("r{n}");
        let name = Path::new(&c.path).file_stem().map_or(c.name.clone(), |s| s.to_string_lossy().into_owned());
        let dur = t(c.media.duration.max(c.src_out()));
        let (start, uid) = (t(c.media.timecode), uid(&key));
        let rate = if c.media.sample_rate > 0 { c.media.sample_rate } else { cut.sample_rate.max(1) }.to_string();
        let mut attrs = vec![("id", id.as_str()), ("name", name.as_str()), ("uid", uid.as_str()), ("start", start.as_str()), ("duration", dur.as_str())];
        let (hv, ha) = (c.media.has_video || c.media.has_video == c.media.has_audio, c.media.has_audio || c.media.has_video == c.media.has_audio);
        if hv {
            attrs.extend([("hasVideo", "1"), ("format", "r1")]);
        }
        if ha {
            attrs.extend([("hasAudio", "1"), ("audioSources", "1"), ("audioChannels", "2"), ("audioRate", rate.as_str())]);
        }
        x.open("asset", &attrs);
        if !c.path.is_empty() {
            x.empty("media-rep", &[("kind", "original-media"), ("src", &file_url(Path::new(&c.path)))]);
        } else {
            notes.push(format!("The event {} has no file; it goes out as an asset without media", c.name));
        }
        x.close("asset");
        assets.insert(key, id);
    }
    n += 1;
    let dissolve = format!("r{n}");
    x.empty("effect", &[("id", &dissolve), ("name", "Cross Dissolve"), ("uid", "FxPlug:4731E73A-8DAC-4113-9A30-AE85B1761265")]);
    x.close("resources");
    let name = if cut.name.is_empty() { "Sequence" } else { &cut.name };
    x.open("library", &[]);
    x.open("event", &[("name", name)]);
    x.open("project", &[("name", name)]);
    let rate_label = format!("{}k", (cut.sample_rate.max(1) as f64 / 1000.0).round() as u32);
    x.open("sequence", &[("format", "r1"), ("duration", &t(total)), ("tcStart", &t(cut.start_tc)), ("tcFormat", if is_drop(fps) { "DF" } else { "NDF" }), ("audioLayout", "stereo"), ("audioRate", &rate_label)]);
    // The spine is the bottom video track; the others are connected storylines on lanes 1, 2, ...; audio on -1, -2, ...
    let video: Vec<&CutTrack> = cut.tracks.iter().filter(|t| t.video).collect();
    let audio: Vec<&CutTrack> = cut.tracks.iter().filter(|t| !t.video).collect();
    let spine_track = video.last().copied();
    let spine = items(spine_track, 0.0, total, fps, &mut notes);
    let mut lanes: Vec<(i32, &CutTrack)> = video.iter().rev().skip(1).enumerate().map(|(i, t)| (i as i32 + 1, *t)).collect();
    lanes.extend(audio.iter().enumerate().map(|(i, t)| (-(i as i32) - 1, *t)));
    // Markers go on the spine element that covers them.
    let covering = |at: f64| spine.iter().position(|it| !matches!(it, Item::Transition { .. }) && it.offset() <= at + 1e-9 && at < it.end() - 1e-9).or_else(|| spine.iter().rposition(|it| !matches!(it, Item::Transition { .. })));
    let mut markers_on: HashMap<usize, Vec<&CutMarker>> = HashMap::new();
    for m in &cut.markers {
        if let Some(i) = covering(m.at) {
            markers_on.entry(i).or_default().push(m);
        }
    }
    x.open("spine", &[]);
    let video_spine = spine_track.is_some_and(|t| t.video);
    for (i, it) in spine.iter().enumerate() {
        let anchored = if i == spine.iter().position(|it| !matches!(it, Item::Transition { .. })).unwrap_or(0) { lanes.as_slice() } else { &[] };
        write_item(&mut x, it, fps, &assets, &dissolve, video_spine, anchored, markers_on.get(&i).map(Vec::as_slice).unwrap_or(&[]), total, &mut notes);
    }
    x.close("spine");
    x.close("sequence");
    x.close("project");
    x.close("event");
    x.close("library");
    x.close("fcpxml");
    (x.out, notes)
}

/// One storyline element; `anchored` tracks hang off it as connected storylines.
fn write_item(x: &mut Xml, it: &Item, fps: f64, assets: &HashMap<String, String>, dissolve: &str, video: bool, anchored: &[(i32, &CutTrack)], markers: &[&CutMarker], total: f64, notes: &mut Vec<String>) {
    let t = |s: f64| ts(s, fps);
    let frame = 1.0 / crate::project::exact_fps(fps);
    let mut inner = |x: &mut Xml, it: &Item| {
        for (lane, track) in anchored {
            let Some(first) = track.clips.first() else { continue };
            let lane_items = items(Some(track), first.start, total, fps, notes);
            x.open("spine", &[("lane", &lane.to_string()), ("offset", &t(it.local(first.start)))]);
            for li in &lane_items {
                // Items of a connected storyline count from its start.
                let shifted = match li {
                    Item::Gap { offset, duration } => Item::Gap { offset: offset - first.start, duration: *duration },
                    Item::Clip { c, offset, duration, src } => Item::Clip { c, offset: offset - first.start, duration: *duration, src: *src },
                    Item::Transition { offset, duration } => Item::Transition { offset: offset - first.start, duration: *duration },
                };
                write_item(x, &shifted, fps, assets, dissolve, track.video, &[], &[], total, notes);
            }
            x.close("spine");
        }
        for m in markers {
            let start = t(it.local(m.at));
            let dur = t(m.end.map_or(frame, |e| (e - m.at).max(frame)));
            x.empty("marker", &[("start", &start), ("duration", &dur), ("value", &m.name)]);
        }
    };
    match it {
        Item::Gap { offset, duration } => {
            x.open("gap", &[("name", "Gap"), ("offset", &t(*offset)), ("start", "0s"), ("duration", &t(duration.max(frame)))]);
            inner(x, it);
            x.close("gap");
        }
        Item::Transition { offset, duration } => {
            x.open("transition", &[("name", "Cross Dissolve"), ("offset", &t(*offset)), ("duration", &t(*duration))]);
            x.empty("filter-video", &[("ref", dissolve), ("name", "Cross Dissolve")]);
            x.close("transition");
        }
        Item::Clip { c, offset, duration, src } => {
            let key = if c.path.is_empty() { format!("#{}", c.name) } else { c.path.clone() };
            let id = assets.get(&key).cloned().unwrap_or_default();
            let (off, start, dur) = (t(*offset), t(*src), t(*duration));
            let name = if c.name.is_empty() { "Clip" } else { &c.name };
            let mut attrs = vec![("ref", id.as_str()), ("offset", off.as_str()), ("name", name), ("start", start.as_str()), ("duration", dur.as_str()), ("tcFormat", if is_drop(fps) { "DF" } else { "NDF" })];
            if c.muted {
                attrs.push(("enabled", "0"));
            }
            let both = c.media.has_video && c.media.has_audio;
            if both {
                attrs.push(("srcEnable", if video { "video" } else { "audio" }));
            }
            if !video {
                attrs.push(("audioRole", "dialogue"));
            }
            x.open("asset-clip", &attrs);
            if (c.rate - 1.0).abs() > 1e-6 || c.reverse {
                let (v0, v1) = if c.reverse { (c.src_out(), *src) } else { (*src, src + duration * c.rate) };
                x.open("timeMap", &[]);
                x.empty("timept", &[("time", &t(*src)), ("value", &t(v0)), ("interp", "linear")]);
                x.empty("timept", &[("time", &t(src + duration)), ("value", &t(v1)), ("interp", "linear")]);
                x.close("timeMap");
            }
            if video && (c.opacity < 1.0 || c.fade_in > 0.0 || c.fade_out > 0.0) {
                let amount = format!("{}", (c.opacity * 1000.0).round() / 1000.0);
                x.open("adjust-blend", &[("amount", &amount)]);
                if c.fade_in > 0.0 || c.fade_out > 0.0 {
                    x.open("param", &[("name", "amount")]);
                    x.open("keyframeAnimation", &[]);
                    if c.fade_in > 0.0 {
                        x.empty("keyframe", &[("time", &t(*src)), ("value", "0")]);
                        x.empty("keyframe", &[("time", &t(src + c.fade_in.min(*duration))), ("value", &amount)]);
                    }
                    if c.fade_out > 0.0 {
                        x.empty("keyframe", &[("time", &t(src + duration - c.fade_out.min(*duration))), ("value", &amount)]);
                        x.empty("keyframe", &[("time", &t(src + duration)), ("value", "0")]);
                    }
                    x.close("keyframeAnimation");
                    x.close("param");
                }
                x.close("adjust-blend");
            }
            if !video && (c.gain_db != 0.0 || c.fade_in > 0.0 || c.fade_out > 0.0) {
                x.open("adjust-volume", &[("amount", &format!("{}dB", (c.gain_db * 100.0).round() / 100.0))]);
                if c.fade_in > 0.0 || c.fade_out > 0.0 {
                    x.open("param", &[("name", "amount")]);
                    if c.fade_in > 0.0 {
                        x.empty("fadeIn", &[("type", "easeIn"), ("duration", &t(c.fade_in))]);
                    }
                    if c.fade_out > 0.0 {
                        x.empty("fadeOut", &[("type", "easeOut"), ("duration", &t(c.fade_out))]);
                    }
                    x.close("param");
                }
                x.close("adjust-volume");
            }
            inner(x, it);
            x.close("asset-clip");
            if c.transition.as_ref().is_some_and(|tr| tr.plugin != DISSOLVE) {
                notes.push(format!("The event {name} at {:.3} s: its transition goes out as a Cross Dissolve", c.start));
            }
        }
    }
}

// ---- Import ----

struct Asset {
    name: String,
    path: String,
    start: f64,
    duration: f64,
    has_video: bool,
    has_audio: bool,
    sample_rate: u32,
    width: u32,
    height: u32,
    frame_rate: f64,
}

struct Formats(HashMap<String, (f64, u32, u32)>);

impl Formats {
    fn of(&self, id: Option<&str>) -> Option<(f64, u32, u32)> {
        self.0.get(id?).copied()
    }
}

/// The reader's state: lanes of events, assets, pending transitions per storyline.
struct Rd {
    assets: HashMap<String, Asset>,
    lanes: HashMap<i32, CutTrack>,
    markers: Vec<CutMarker>,
    warnings: Vec<String>,
    frame: f64,
}

impl Rd {
    fn track(&mut self, lane: i32) -> &mut CutTrack {
        self.lanes.entry(lane).or_insert_with(|| CutTrack { video: lane >= 0, ..Default::default() })
    }

    /// The items of a storyline (`spine`, or a clip's anchored children) at `base` (timeline time of local 0).
    fn walk(&mut self, parent: &El, base: f64, lane: i32, storyline: bool) {
        let mut pending: Option<(f64, f64)> = None; // a transition's (timeline offset, duration)
        let mut last_clip_end: Option<f64> = None;
        for el in &parent.children {
            let offset = el.attr("offset").and_then(parse_time).unwrap_or(0.0);
            let at = base + offset;
            let el_lane = el.attr("lane").and_then(|l| l.parse::<i32>().ok()).unwrap_or(lane);
            match el.name.as_str() {
                "transition" if storyline => {
                    let d = el.attr("duration").and_then(parse_time).unwrap_or(0.0);
                    pending = Some((at, d));
                }
                "gap" => {
                    let start = el.attr("start").and_then(parse_time).unwrap_or(0.0);
                    let dur = el.attr("duration").and_then(parse_time).unwrap_or(0.0);
                    self.items_in(el, at - start, el_lane);
                    if let Some((to, td)) = pending.take() {
                        // A dissolve into a gap: the previous event fades out through it.
                        if let Some(prev) = self.lanes.get_mut(&el_lane).and_then(|t| t.clips.last_mut()) {
                            prev.end = prev.end.max(to + td);
                            prev.fade_out = td;
                        }
                    }
                    last_clip_end = Some(at + dur);
                }
                "spine" => {
                    // A connected storyline: its items count from its own start.
                    self.walk(el, at, el_lane, true);
                }
                "asset-clip" | "clip" | "sync-clip" | "ref-clip" | "mc-clip" | "audition" => {
                    let el = if el.name == "audition" {
                        let Some(first) = el.children.iter().find(|c| c.name != "note") else { continue };
                        self.warnings.push(format!("The audition at {at:.3} s plays its first pick"));
                        first
                    } else {
                        el
                    };
                    let start = el.attr("start").and_then(parse_time).unwrap_or(0.0);
                    let dur = el.attr("duration").and_then(parse_time).unwrap_or(0.0);
                    let name = el.attr("name").unwrap_or("").to_string();
                    // The media: the asset-clip's ref, or a clip's first video/audio child's ref.
                    let (asset_id, src_shift) = match el.name.as_str() {
                        "asset-clip" => (el.attr("ref").map(str::to_string), 0.0),
                        "clip" | "sync-clip" => {
                            let media = el.children.iter().find(|c| (c.name == "video" || c.name == "audio") && c.attr("ref").is_some()).or_else(|| el.find("asset-clip"));
                            match media {
                                Some(m) => {
                                    let (mo, ms) = (m.attr("offset").and_then(parse_time).unwrap_or(0.0), m.attr("start").and_then(parse_time).unwrap_or(0.0));
                                    (m.attr("ref").map(str::to_string), ms - mo)
                                }
                                None => (None, 0.0),
                            }
                        }
                        _ => {
                            self.warnings.push(format!("The compound clip {name} at {at:.3} s is an offline event (its contents are not read)"));
                            (None, 0.0)
                        }
                    };
                    let asset = asset_id.as_deref().and_then(|id| self.assets.get(id));
                    let enable = el.attr("srcEnable").unwrap_or("all");
                    let (has_video, has_audio) = asset.map_or((true, false), |a| (a.has_video, a.has_audio));
                    let mut c = CutClip { start: at, end: at + dur, name: name.clone(), muted: el.attr("enabled") == Some("0"), ..Default::default() };
                    if let Some(a) = asset {
                        c.path = a.path.clone();
                        if c.name.is_empty() {
                            c.name = a.name.clone();
                        }
                        c.media.duration = a.duration;
                        c.media.timecode = a.start;
                        (c.media.has_video, c.media.has_audio) = (a.has_video, a.has_audio);
                        (c.media.sample_rate, c.media.width, c.media.height, c.media.frame_rate) = (a.sample_rate, a.width, a.height, a.frame_rate);
                        c.src_in = start + src_shift - a.start;
                    } else {
                        c.src_in = start + src_shift;
                    }
                    // Speed: a linear time map.
                    if let Some(tm) = el.child("timeMap") {
                        let pts: Vec<(f64, f64)> = tm.children_named("timept").filter_map(|p| Some((parse_time(p.attr("time")?)?, parse_time(p.attr("value")?)?))).collect();
                        if let (Some(a), Some(b)) = (pts.first(), pts.last()) {
                            if b.0 > a.0 {
                                let slope = (b.1 - a.1) / (b.0 - a.0);
                                c.rate = slope.abs().max(0.001);
                                c.reverse = slope < 0.0;
                                let v_at_start = a.1 + (start - a.0) * slope;
                                let src0 = if c.reverse { v_at_start - dur * c.rate } else { v_at_start };
                                c.src_in = src0 - asset.map_or(0.0, |a| a.start);
                            }
                            if pts.len() > 2 {
                                self.warnings.push(format!("The event {} at {at:.3} s has a speed ramp; its average speed is used", c.name));
                            }
                        }
                    }
                    if let Some(ab) = el.child("adjust-blend") {
                        c.opacity = ab.attr("amount").and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0).clamp(0.0, 1.0);
                        if let Some(keys) = ab.find("keyframeAnimation") {
                            let ks: Vec<(f64, f32)> = keys.children_named("keyframe").filter_map(|k| Some((parse_time(k.attr("time")?)?, k.attr("value")?.parse().ok()?))).collect();
                            if ks.len() >= 2 && ks[0].1 == 0.0 {
                                c.fade_in = (ks[1].0 - ks[0].0).max(0.0);
                            }
                            if ks.len() >= 2 && ks[ks.len() - 1].1 == 0.0 {
                                c.fade_out = (ks[ks.len() - 1].0 - ks[ks.len() - 2].0).max(0.0);
                            }
                            if ks.len() > 4 || (ks.len() > 2 && (c.fade_in == 0.0 || c.fade_out == 0.0)) {
                                self.warnings.push(format!("The event {} at {at:.3} s: opacity keyframes beyond its fades were left out", c.name));
                            }
                        }
                    }
                    if let Some(av) = el.child("adjust-volume") {
                        c.gain_db = av.attr("amount").and_then(|v| v.trim_end_matches("dB").parse::<f32>().ok()).unwrap_or(0.0);
                        if let Some(f) = av.find("fadeIn") {
                            c.fade_in = f.attr("duration").and_then(parse_time).unwrap_or(0.0);
                        }
                        if let Some(f) = av.find("fadeOut") {
                            c.fade_out = f.attr("duration").and_then(parse_time).unwrap_or(0.0);
                        }
                    }
                    for f in el.children.iter().filter(|c| c.name == "filter-video" || c.name == "filter-audio") {
                        self.warnings.push(format!("The event {} at {at:.3} s: the {} effect is not available in Milan Pro and was left out", c.name, f.attr("name").unwrap_or("?")));
                    }
                    // Markers and anchored items live in the clip's local time.
                    self.items_in(el, at - start, el_lane);
                    // A transition before it: the previous event plays on past the cut, this one starts before it.
                    let mut tr: Option<CutTransition> = None;
                    if let Some((to, td)) = pending.take() {
                        if last_clip_end.is_some_and(|e| (e - at).abs() < self.frame * 0.6) {
                            if let Some(prev) = self.lanes.get_mut(&el_lane).and_then(|t| t.clips.last_mut()) {
                                prev.end = prev.end.max(to + td);
                            }
                            let back = (at - to).max(0.0);
                            c.src_in -= back * c.rate;
                            c.start = to;
                            tr = Some(CutTransition { plugin: DISSOLVE.into(), duration: td });
                        } else {
                            c.fade_in = td;
                        }
                    }
                    c.transition = tr;
                    last_clip_end = Some(at + dur);
                    // Video and audio parts: the lane's video track and the matching audio track, grouped.
                    let video_part = has_video && enable != "audio" && el_lane >= 0;
                    let audio_part = has_audio && enable != "video";
                    let group = (video_part && audio_part).then(|| uuid::Uuid::new_v4().to_string());
                    if video_part {
                        let mut v = c.clone();
                        v.group = group.clone();
                        self.track(el_lane).clips.push(v);
                    }
                    if audio_part || (!video_part && !has_video) {
                        let alane = if el_lane >= 0 { -(el_lane + 1) } else { el_lane };
                        let mut a = c.clone();
                        (a.opacity, a.group) = (1.0, group);
                        self.track(alane).clips.push(a);
                    } else if !video_part {
                        self.track(el_lane.max(0)).clips.push(c);
                    }
                }
                "title" | "video" | "generator" if storyline || el.attr("lane").is_some() => {
                    self.warnings.push(format!("The {} {} at {at:.3} s is not available in Milan Pro and was left out", el.name, el.attr("name").unwrap_or("")));
                    if let Some((to, td)) = pending.take() {
                        if let Some(prev) = self.lanes.get_mut(&el_lane).and_then(|t| t.clips.last_mut()) {
                            prev.end = prev.end.max(to + td);
                            prev.fade_out = td;
                        }
                    }
                    last_clip_end = Some(at + el.attr("duration").and_then(parse_time).unwrap_or(0.0));
                }
                _ => {}
            }
        }
        if let Some((_, td)) = pending {
            if let Some(prev) = self.lanes.get_mut(&lane).and_then(|t| t.clips.last_mut()) {
                prev.fade_out = td;
            }
        }
    }

    /// A clip's or gap's markers and anchored items (`base`: the timeline time of its local 0).
    fn items_in(&mut self, el: &El, base: f64, lane: i32) {
        for m in el.children.iter().filter(|c| c.name == "marker" || c.name == "chapter-marker") {
            let Some(at) = m.attr("start").and_then(parse_time) else { continue };
            let dur = m.attr("duration").and_then(parse_time).unwrap_or(0.0);
            let name = m.attr("value").unwrap_or("").to_string();
            self.markers.push(CutMarker { at: base + at, end: (dur > self.frame * 1.5).then(|| base + at + dur), name });
        }
        if el.children.iter().any(|c| c.attr("lane").is_some() || c.name == "spine") {
            // Anchored items: each in its own lane (connected clips are not a storyline: no transitions).
            let anchored: Vec<&El> = el.children.iter().filter(|c| c.attr("lane").is_some()).collect();
            for a in anchored {
                let wrapper = El { name: "#anchored".into(), children: vec![a.clone()], ..Default::default() };
                let is_spine = a.name == "spine";
                self.walk(&wrapper, base, a.attr("lane").and_then(|l| l.parse().ok()).unwrap_or(lane), is_spine);
            }
        }
    }
}

/// Read FCPXML; `dir` is the file's folder.
pub fn read(text: &str, dir: Option<&Path>) -> anyhow::Result<(Cut, Vec<String>)> {
    let root = parse(text)?;
    anyhow::ensure!(root.name == "fcpxml", "not an FCPXML document (root <{}>)", root.name);
    let mut formats = Formats(HashMap::new());
    let mut rd = Rd { assets: HashMap::new(), lanes: HashMap::new(), markers: Vec::new(), warnings: Vec::new(), frame: 1.0 / 30.0 };
    if let Some(res) = root.child("resources") {
        for f in res.children_named("format") {
            let fd = f.attr("frameDuration").and_then(parse_time).filter(|d| *d > 0.0).map_or(0.0, |d| ((1.0 / d) * 1000.0).round() / 1000.0);
            formats.0.insert(f.attr("id").unwrap_or("").to_string(), (fd, f.attr("width").and_then(|v| v.parse().ok()).unwrap_or(0), f.attr("height").and_then(|v| v.parse().ok()).unwrap_or(0)));
        }
        for a in res.children_named("asset") {
            let src = a.child("media-rep").and_then(|m| m.attr("src")).or_else(|| a.attr("src")).unwrap_or("");
            let fmt = formats.of(a.attr("format"));
            rd.assets.insert(
                a.attr("id").unwrap_or("").to_string(),
                Asset {
                    name: a.attr("name").unwrap_or("").to_string(),
                    path: if src.is_empty() { String::new() } else { url_path(src, dir) },
                    start: a.attr("start").and_then(parse_time).unwrap_or(0.0),
                    duration: a.attr("duration").and_then(parse_time).unwrap_or(0.0),
                    has_video: a.attr("hasVideo") == Some("1"),
                    has_audio: a.attr("hasAudio") == Some("1"),
                    sample_rate: a.attr("audioRate").and_then(|v| v.parse().ok()).unwrap_or(0),
                    width: fmt.map_or(0, |f| f.1),
                    height: fmt.map_or(0, |f| f.2),
                    frame_rate: fmt.map_or(0.0, |f| f.0),
                },
            );
        }
    }
    let seq = root.find("sequence").ok_or_else(|| anyhow::anyhow!("the FCPXML has no <sequence>"))?;
    let project_name = root.find("project").and_then(|p| p.attr("name")).or_else(|| root.find("event").and_then(|e| e.attr("name"))).unwrap_or("");
    let (fps, w, h) = formats.of(seq.attr("format")).filter(|f| f.0 > 0.0).unwrap_or((29.97, 0, 0));
    rd.frame = 1.0 / fps;
    let tc_start = seq.attr("tcStart").and_then(parse_time).unwrap_or(0.0);
    let sample_rate = seq.attr("audioRate").map(|r| r.trim_end_matches('k')).and_then(|r| r.parse::<f64>().ok()).map_or(0, |k| if k < 1000.0 { (k * 1000.0).round() as u32 } else { k as u32 });
    let spine = seq.child("spine").ok_or_else(|| anyhow::anyhow!("the sequence has no <spine>"))?;
    rd.walk(spine, -tc_start, 0, true);
    let mut cut = Cut { name: project_name.to_string(), fps, width: w, height: h, sample_rate, start_tc: tc_start, ..Default::default() };
    let mut lanes: Vec<(i32, CutTrack)> = rd.lanes.into_iter().collect();
    lanes.sort_by_key(|(l, _)| *l);
    let (video, audio): (Vec<_>, Vec<_>) = lanes.into_iter().partition(|(l, _)| *l >= 0);
    for (n, (_, mut t)) in video.into_iter().rev().enumerate() {
        t.name = format!("Video {}", n + 1);
        t.clips.sort_by(|a, b| a.start.total_cmp(&b.start));
        cut.tracks.push(t);
    }
    for (n, (_, mut t)) in audio.into_iter().rev().enumerate() {
        t.name = format!("Audio {}", n + 1);
        t.clips.sort_by(|a, b| a.start.total_cmp(&b.start));
        cut.tracks.push(t);
    }
    rd.markers.sort_by(|a, b| a.at.total_cmp(&b.at));
    cut.markers = rd.markers;
    if cut.tracks.is_empty() {
        rd.warnings.push("The sequence has no clips".into());
    }
    Ok((cut, rd.warnings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interchange::cutlist::{self, to_project};

    #[test]
    fn times() {
        assert_eq!(ts(5.0, 25.0), "5s");
        assert_eq!(ts(0.1, 29.97), "3003/30000s");
        assert_eq!(parse_time("3003/30000s"), Some(0.1001));
        assert_eq!(parse_time("-2s"), Some(-2.0));
        assert_eq!(parse_time("1.5s"), Some(1.5));
    }

    #[test]
    fn round_trip() {
        let mut p = cutlist::tests::sample();
        // A second video track above, so the spine and a connected storyline are both exercised.
        let mut top = crate::project::Track::new("Video 2", crate::project::TrackKind::Video);
        let mut c = crate::project::Clip::from_media(&p.media_pool[1], 6.0);
        (c.duration, c.in_point, c.out_point, c.fade_in) = (3.0, 1.0, 4.0, 0.4);
        top.clips.push(c);
        p.timeline.video_tracks.insert(0, top);
        let (cut, _) = from_project(&p, "FCPXML");
        let (text, notes) = write(&cut);
        assert!(notes.is_empty(), "{notes:?}");
        assert!(text.contains("<fcpxml version=\"1.10\">") && text.contains("frameDuration=\"1/25s\"") && text.contains("<transition ") && text.contains("<timeMap>") && text.contains("lane=\"1\"") && text.contains("lane=\"-1\""), "{text}");
        let (back, w) = read(&text, None).unwrap();
        assert!(w.is_empty(), "{w:?}\n{text}");
        assert_eq!((back.fps, back.width, back.height, back.sample_rate), (25.0, 1280, 720, 48000));
        assert_eq!(back.tracks.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["Video 1", "Video 2", "Audio 1"]);
        for (t, t0) in back.tracks.iter().zip(&cut.tracks) {
            assert_eq!(t.clips.len(), t0.clips.len(), "{}", t0.name);
            for (c, c0) in t.clips.iter().zip(&t0.clips) {
                assert!((c.start - c0.start).abs() < 1e-6 && (c.end - c0.end).abs() < 1e-6 && (c.src_in - c0.src_in).abs() < 1e-6, "{c:?}\nvs {c0:?}\n{text}");
                assert!((c.rate - c0.rate).abs() < 1e-6 && c.reverse == c0.reverse && c.path == c0.path, "{c:?}");
                assert_eq!(c.transition.as_ref().map(|t| t.duration), c0.transition.as_ref().map(|t| t.duration), "{c:?}");
                assert!((c.fade_in - c0.fade_in).abs() < 1e-6 && (c.fade_out - c0.fade_out).abs() < 1e-6, "{c:?}");
                assert!((c.opacity - c0.opacity).abs() < 1e-3 && (c.gain_db - c0.gain_db).abs() < 0.01, "{c:?}");
            }
        }
        assert_eq!(back.markers, cut.markers);
        let (q, _) = to_project(&back, &cutlist::tests::probed(&p));
        assert_eq!(q.timeline.video_tracks.len(), 2);
    }

    #[test]
    fn reads_the_hand_written_fcpx_sequence() {
        let text = include_str!("../../tests/fixtures/fcpx_sample.fcpxml");
        let (cut, w) = read(text, None).unwrap();
        assert_eq!((cut.fps, cut.width, cut.height, cut.sample_rate, cut.name.as_str()), (29.97, 1920, 1080, 48000, "FCPX Sample"));
        assert_eq!(cut.tracks.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["Video 1", "Video 2", "Audio 1", "Audio 2"]);
        let spine = &cut.tracks[1];
        assert_eq!(spine.clips.len(), 2, "{:#?}", spine.clips);
        let f = crate::project::exact_fps(29.97);
        let a = &spine.clips[0];
        assert!(a.path.ends_with("A001.mov") && (a.start - 0.0).abs() < 1e-9 && (a.end - 150.0 / f).abs() < 1e-6 && (a.src_in - 30.0 / f).abs() < 1e-6, "{a:?}");
        let b = &spine.clips[1];
        assert!((b.start - 120.0 / f).abs() < 1e-6 && b.transition.as_ref().is_some_and(|t| (t.duration - 30.0 / f).abs() < 1e-6), "{b:?}");
        assert!((b.rate - 2.0).abs() < 1e-6 && (b.opacity - 0.75).abs() < 1e-6, "{b:?}");
        let top = &cut.tracks[0].clips[0];
        assert!((top.start - 60.0 / f).abs() < 1e-6 && top.muted, "{top:?}");
        let au = &cut.tracks[2].clips[0];
        assert!((au.gain_db + 3.0).abs() < 1e-6 && (au.fade_in - 15.0 / f).abs() < 1e-6 && au.group == a.group && a.group.is_some(), "{au:?}");
        assert!(cut.tracks[3].clips[0].path.ends_with("music.wav"));
        assert_eq!((cut.markers.len(), cut.markers[0].name.as_str(), cut.markers[1].end.is_some()), (2, "Beat", true));
        assert!(w.iter().any(|w| w.contains("title")) && w.iter().any(|w| w.contains("Gaussian")), "{w:?}");
    }
}
