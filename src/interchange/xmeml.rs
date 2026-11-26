//! R29: Final Cut Pro 7 XML (`xmeml` version 5), the sequence XML Premiere Pro and DaVinci Resolve read and write
//! (File › Import / Export › Final Cut Pro 7/DaVinci Resolve (*.xml)..., and the Premiere rows).
//!
//! Out: one `<sequence>` with the project's rate (timebase + NTSC flag), every video track, and two audio tracks
//! per Milan audio track (channel 1 and 2 of each stereo event, linked, as Premiere and Resolve write stereo),
//! clip items with their `<file>` (path URL, duration, start timecode), Cross Dissolves for overlaps, fades as
//! dissolves from and to black, Time Remap for speed changes, Opacity and Audio Levels filters for static levels,
//! `<link>`s for groups, sequence markers. In: the same, with a Premiere/Resolve stereo pair merged back into one
//! event; other filters are listed and dropped. Everything goes through the cut list (`cutlist.rs`).
use std::collections::HashMap;
use std::path::Path;
use crate::interchange::cutlist::{Cut, CutClip, CutMarker, CutTrack, CutTransition, DISSOLVE, from_project};
use crate::interchange::otio::{file_url, url_path};
use crate::interchange::xml::{El, Xml, parse};
use crate::project::{Project, exact_fps, format_timecode, is_drop};

/// Write `p` as FCP7 XML; returns what the XML could not hold.
pub fn export(p: &Project, path: &Path) -> anyhow::Result<Vec<String>> {
    let (cut, mut notes) = from_project(p, "FCP7 XML");
    let (text, more) = write(&cut);
    notes.extend(more);
    std::fs::write(path, text)?;
    Ok(notes)
}

/// FCP7's rate: a whole timebase and whether it is really timebase × 1000/1001.
fn timebase(fps: f64) -> (u32, bool) {
    let nominal = fps.round();
    (nominal as u32, fps.fract() != 0.0 && (nominal - fps).abs() < 0.1)
}

fn frames(seconds: f64, fps: f64) -> i64 {
    (seconds * exact_fps(fps)).round() as i64
}

fn rate(x: &mut Xml, fps: f64) {
    let (tb, ntsc) = timebase(fps);
    x.open("rate", &[]);
    x.leaf("timebase", &tb.to_string());
    x.leaf("ntsc", if ntsc { "TRUE" } else { "FALSE" });
    x.close("rate");
}

fn timecode(x: &mut Xml, seconds: f64, fps: f64) {
    x.open("timecode", &[]);
    rate(x, fps);
    x.leaf("string", &format_timecode(seconds, fps));
    x.leaf("frame", &frames(seconds, fps).to_string());
    x.leaf("displayformat", if is_drop(fps) { "DF" } else { "NDF" });
    x.close("timecode");
}

fn tf(b: bool) -> &'static str {
    if b { "TRUE" } else { "FALSE" }
}

/// A one-parameter filter (`Opacity`, `Audio Levels`) or Time Remap.
fn filter(x: &mut Xml, name: &str, id: &str, category: &str, media: &str, params: &[(&str, &str, &str, &str, String)]) {
    x.open("filter", &[]);
    x.open("effect", &[]);
    x.leaf("name", name);
    x.leaf("effectid", id);
    x.leaf("effectcategory", category);
    x.leaf("effecttype", if media == "audio" { "audiolevels" } else { "motion" });
    x.leaf("mediatype", media);
    for (pid, pname, min, max, value) in params {
        x.open("parameter", &[]);
        x.leaf("parameterid", pid);
        x.leaf("name", pname);
        if !min.is_empty() {
            x.leaf("valuemin", min);
            x.leaf("valuemax", max);
        }
        x.leaf("value", value);
        x.close("parameter");
    }
    x.close("effect");
    x.close("filter");
}

fn transition_item(x: &mut Xml, start: i64, end: i64, alignment: &str, fps: f64, video: bool) {
    x.open("transitionitem", &[]);
    x.leaf("start", &start.to_string());
    x.leaf("end", &end.to_string());
    x.leaf("alignment", alignment);
    rate(x, fps);
    x.open("effect", &[]);
    x.leaf("name", if video { "Cross Dissolve" } else { "Cross Fade (+3dB)" });
    x.leaf("effectid", if video { "Cross Dissolve" } else { "Cross Fade (+3dB)" });
    x.leaf("effectcategory", if video { "Dissolve" } else { "Audio Transitions" });
    x.leaf("effecttype", "transition");
    x.leaf("mediatype", if video { "video" } else { "audio" });
    x.leaf("wipecode", "0");
    x.leaf("wipeaccuracy", "100");
    x.leaf("startratio", "0");
    x.leaf("endratio", "1");
    x.leaf("reverse", "FALSE");
    x.close("effect");
    x.close("transitionitem");
}

/// The id of every clip item that goes out, so links can be written: (track, clip, channel) → id.
type Ids = HashMap<(usize, usize, usize), String>;

/// xmeml text of `cut`, and what the XML left out.
pub fn write(cut: &Cut) -> (String, Vec<String>) {
    let fps = if cut.fps > 0.0 { cut.fps } else { 29.97 };
    let fr = |s: f64| frames(s, fps);
    let mut notes = Vec::new();
    let mut x = Xml::new("<!DOCTYPE xmeml>");
    x.open("xmeml", &[("version", "5")]);
    x.open("sequence", &[("id", "sequence-1")]);
    x.leaf("name", if cut.name.is_empty() { "Sequence" } else { &cut.name });
    let duration = cut.tracks.iter().flat_map(|t| &t.clips).map(|c| c.end).fold(0.0, f64::max);
    x.leaf("duration", &fr(duration).to_string());
    rate(&mut x, fps);
    timecode(&mut x, cut.start_tc, fps);
    x.leaf("in", "-1");
    x.leaf("out", "-1");
    x.open("media", &[]);
    // Ids first, so every link can name every member.
    let mut ids: Ids = HashMap::new();
    let mut n = 0;
    for (ti, t) in cut.tracks.iter().enumerate() {
        for (ci, _) in t.clips.iter().enumerate() {
            for ch in 0..if t.video { 1 } else { 2 } {
                n += 1;
                ids.insert((ti, ci, ch), format!("clipitem-{n}"));
            }
        }
    }
    // Groups: members by group id; a stereo pair is always linked.
    let mut groups: HashMap<&str, Vec<(usize, usize)>> = HashMap::new();
    for (ti, t) in cut.tracks.iter().enumerate() {
        for (ci, c) in t.clips.iter().enumerate() {
            if let Some(g) = &c.group {
                groups.entry(g).or_default().push((ti, ci));
            }
        }
    }
    let mut files: HashMap<String, String> = HashMap::new(); // path → file id
    let mut file_n = 0;
    for video in [true, false] {
        x.open(if video { "video" } else { "audio" }, &[]);
        if !video {
            x.leaf("numOutputChannels", "2");
        }
        x.open("format", &[]);
        x.open("samplecharacteristics", &[]);
        if video {
            rate(&mut x, fps);
            x.leaf("width", &cut.width.max(1).to_string());
            x.leaf("height", &cut.height.max(1).to_string());
            x.leaf("anamorphic", "FALSE");
            x.leaf("pixelaspectratio", "square");
            x.leaf("fielddominance", "none");
            x.leaf("colordepth", "24");
        } else {
            x.leaf("depth", "16");
            x.leaf("samplerate", &cut.sample_rate.max(1).to_string());
        }
        x.close("samplecharacteristics");
        x.close("format");
        if !video {
            x.open("outputs", &[]);
            for ch in 1..=2 {
                x.open("group", &[]);
                x.leaf("index", &ch.to_string());
                x.leaf("numchannels", "1");
                x.leaf("downmix", "0");
                x.open("channel", &[]);
                x.leaf("index", &ch.to_string());
                x.close("channel");
                x.close("group");
            }
            x.close("outputs");
        }
        for (ti, t) in cut.tracks.iter().enumerate().filter(|(_, t)| t.video == video) {
            for ch in 0..if video { 1 } else { 2 } {
                x.open("track", &[]);
                for (ci, c) in t.clips.iter().enumerate() {
                    let (start, end) = (fr(c.start), fr(c.end));
                    // A fade in with nothing under it: a dissolve from black before the clip item.
                    if c.transition.is_none() && c.fade_in > 0.0 {
                        transition_item(&mut x, start, start + fr(c.fade_in).max(1), "start-black", fps, video);
                    } else if let Some(tr) = &c.transition {
                        if ci > 0 {
                            transition_item(&mut x, start, start + fr(tr.duration).max(1), "center", fps, video);
                        }
                    }
                    let id = ids[&(ti, ci, ch)].clone();
                    x.open("clipitem", &[("id", &id)]);
                    x.leaf("name", if c.name.is_empty() { "Clip" } else { &c.name });
                    x.leaf("enabled", tf(!c.muted));
                    x.leaf("duration", &fr(c.media.duration.max(c.src_out())).to_string());
                    rate(&mut x, fps);
                    x.leaf("start", &start.to_string());
                    x.leaf("end", &end.to_string());
                    x.leaf("in", &fr(c.src_in).to_string());
                    x.leaf("out", &fr(c.src_out()).to_string());
                    if c.path.is_empty() {
                        notes.push(format!("{}: the event at {:.3} s has no file; it goes out as an offline clip", t.name, c.start));
                    }
                    let key = if c.path.is_empty() { format!("#{}", c.name) } else { c.path.clone() };
                    match files.get(&key) {
                        Some(fid) => x.empty("file", &[("id", fid)]),
                        None => {
                            file_n += 1;
                            let fid = format!("file-{file_n}");
                            x.open("file", &[("id", &fid)]);
                            let name = Path::new(&c.path).file_name().map_or(c.name.clone(), |n| n.to_string_lossy().into_owned());
                            x.leaf("name", &name);
                            if !c.path.is_empty() {
                                x.leaf("pathurl", &file_url(Path::new(&c.path)).replacen("file:///", "file://localhost/", 1));
                            }
                            rate(&mut x, fps);
                            x.leaf("duration", &fr(c.media.duration.max(c.src_out())).to_string());
                            timecode(&mut x, c.media.timecode, fps);
                            x.open("media", &[]);
                            if c.media.has_video || video {
                                x.open("video", &[]);
                                x.open("samplecharacteristics", &[]);
                                rate(&mut x, if c.media.frame_rate > 0.0 { c.media.frame_rate } else { fps });
                                x.leaf("width", &if c.media.width > 0 { c.media.width } else { cut.width.max(1) }.to_string());
                                x.leaf("height", &if c.media.height > 0 { c.media.height } else { cut.height.max(1) }.to_string());
                                x.close("samplecharacteristics");
                                x.close("video");
                            }
                            if c.media.has_audio || !video {
                                x.open("audio", &[]);
                                x.open("samplecharacteristics", &[]);
                                x.leaf("depth", "16");
                                x.leaf("samplerate", &if c.media.sample_rate > 0 { c.media.sample_rate } else { cut.sample_rate.max(1) }.to_string());
                                x.close("samplecharacteristics");
                                x.leaf("channelcount", "2");
                                x.close("audio");
                            }
                            x.close("media");
                            x.close("file");
                            files.insert(key, fid);
                        }
                    }
                    if (c.rate - 1.0).abs() > 1e-6 || c.reverse {
                        let speed = format!("{}", (c.rate * 100.0 * 1000.0).round() / 1000.0);
                        filter(&mut x, "Time Remap", "timeremap", "motion", if video { "video" } else { "audio" }, &[
                            ("variablespeed", "variablespeed", "0", "1", "0".into()),
                            ("speed", "speed", "-10000", "10000", speed),
                            ("reverse", "reverse", "", "", tf(c.reverse).into()),
                            ("frameblending", "frameblending", "", "", "FALSE".into()),
                        ]);
                    }
                    if video && c.opacity < 1.0 {
                        filter(&mut x, "Opacity", "opacity", "motion", "video", &[("opacity", "opacity", "0", "100", format!("{}", (c.opacity * 100.0 * 100.0).round() / 100.0))]);
                    }
                    if !video && c.gain_db != 0.0 {
                        let level = 10f32.powf(c.gain_db / 20.0);
                        filter(&mut x, "Audio Levels", "audiolevels", "audiolevels", "audio", &[("level", "Level", "0", "3.98109", format!("{level:.6}"))]);
                    }
                    if !video {
                        x.open("sourcetrack", &[]);
                        x.leaf("mediatype", "audio");
                        x.leaf("trackindex", &(ch + 1).to_string());
                        x.close("sourcetrack");
                    }
                    // Links: the group's members on every track (and this event's other channel).
                    let mut members: Vec<(usize, usize)> = c.group.as_deref().and_then(|g| groups.get(g)).cloned().unwrap_or_else(|| vec![(ti, ci)]);
                    if !members.contains(&(ti, ci)) {
                        members.push((ti, ci));
                    }
                    let mut link_n = 0;
                    for (mti, mci) in members {
                        let mt = &cut.tracks[mti];
                        for mch in 0..if mt.video { 1 } else { 2 } {
                            link_n += 1;
                            let (vi, ai) = track_number(cut, mti, mch);
                            x.open("link", &[]);
                            x.leaf("linkclipref", &ids[&(mti, mci, mch)]);
                            x.leaf("mediatype", if mt.video { "video" } else { "audio" });
                            x.leaf("trackindex", &if mt.video { vi } else { ai }.to_string());
                            x.leaf("clipindex", &(mci + 1).to_string());
                            if !mt.video {
                                x.leaf("groupindex", &link_n.to_string());
                            }
                            x.close("link");
                        }
                    }
                    x.close("clipitem");
                    if c.fade_out > 0.0 && t.clips.get(ci + 1).is_none_or(|n| n.transition.is_none()) {
                        transition_item(&mut x, end - fr(c.fade_out).max(1), end, "end-black", fps, video);
                    }
                    if ch == 0 && c.transition.as_ref().is_some_and(|tr| tr.plugin != DISSOLVE) {
                        notes.push(format!("{}: the event at {:.3} s: its transition goes out as a Cross Dissolve", t.name, c.start));
                    }
                }
                x.leaf("enabled", tf(!t.muted));
                x.leaf("locked", tf(t.locked));
                if !video {
                    x.leaf("outputchannelindex", &(ch + 1).to_string());
                }
                x.close("track");
            }
        }
        x.close(if video { "video" } else { "audio" });
    }
    x.close("media");
    for m in &cut.markers {
        x.open("marker", &[]);
        x.leaf("comment", "");
        x.leaf("name", &m.name);
        x.leaf("in", &fr(m.at).to_string());
        x.leaf("out", &m.end.map_or("-1".to_string(), |e| fr(e).to_string()));
        x.close("marker");
    }
    x.close("sequence");
    x.close("xmeml");
    (x.out, notes)
}

/// The 1-based xmeml track index of cut track `ti` channel `ch`: (video index, audio index).
fn track_number(cut: &Cut, ti: usize, ch: usize) -> (usize, usize) {
    let (mut v, mut a) = (0, 0);
    for (i, t) in cut.tracks.iter().enumerate() {
        if t.video {
            v += 1;
        } else {
            a += 2;
        }
        if i == ti {
            return (v, a - 1 + ch);
        }
    }
    (v, a)
}

// ---- Import ----

fn read_rate(el: &El, fallback: f64) -> f64 {
    let Some(r) = el.child("rate") else { return fallback };
    let tb = r.f64_of("timebase").unwrap_or(0.0);
    if tb <= 0.0 {
        return fallback;
    }
    if r.bool_of("ntsc").unwrap_or(false) { ((tb * 1000.0 / 1001.0) * 1000.0).round() / 1000.0 } else { tb }
}

struct FileInfo {
    path: String,
    name: String,
    duration: f64,
    timecode: f64,
    has_video: bool,
    has_audio: bool,
    width: u32,
    height: u32,
    sample_rate: u32,
}

/// The `<file>` of a clip item, remembered by id across the sequence (the second use is `<file id=.../>` alone).
fn file_info<'a>(files: &'a mut HashMap<String, FileInfo>, f: &El, fps: f64, dir: Option<&Path>) -> Option<&'a FileInfo> {
    let id = f.attr("id").unwrap_or("").to_string();
    if f.child("pathurl").is_some() || f.child("name").is_some() {
        let fr = read_rate(f, fps);
        let tc = f.child("timecode").and_then(|t| t.f64_of("frame")).map_or(0.0, |fr_| fr_ / exact_fps(fr));
        let tc = f.child("timecode").and_then(|t| t.text_of("string")).and_then(|s| crate::project::parse_timecode(s, fr)).unwrap_or(tc);
        let media = f.child("media");
        let video = media.and_then(|m| m.child("video"));
        let sc = video.and_then(|v| v.child("samplecharacteristics"));
        let info = FileInfo {
            path: f.text_of("pathurl").map_or(String::new(), |u| url_path(u, dir)),
            name: f.text_of("name").unwrap_or("").to_string(),
            duration: f.f64_of("duration").map_or(0.0, |d| d / exact_fps(fr)),
            timecode: tc,
            has_video: video.is_some(),
            has_audio: media.is_some_and(|m| m.child("audio").is_some()),
            width: sc.and_then(|s| s.i64_of("width")).unwrap_or(0) as u32,
            height: sc.and_then(|s| s.i64_of("height")).unwrap_or(0) as u32,
            sample_rate: media.and_then(|m| m.child("audio")).and_then(|a| a.find("samplerate")).and_then(|s| s.text.trim().parse().ok()).unwrap_or(0),
        };
        files.insert(id.clone(), info);
    }
    files.get(&id)
}

/// A filter's parameter value by id.
fn param(effect: &El, id: &str) -> Option<String> {
    effect.children_named("parameter").find(|p| p.text_of("parameterid") == Some(id)).and_then(|p| p.text_of("value")).map(str::to_string)
}

/// Read FCP7 XML; `dir` is the file's folder (relative paths).
pub fn read(text: &str, dir: Option<&Path>) -> anyhow::Result<(Cut, Vec<String>)> {
    let root = parse(text)?;
    anyhow::ensure!(root.name == "xmeml", "not an xmeml document (root <{}>)", root.name);
    let seq = root.find("sequence").ok_or_else(|| anyhow::anyhow!("the xmeml has no <sequence>"))?;
    let mut warnings = Vec::new();
    let fps = read_rate(seq, 29.97);
    let mut cut = Cut { name: seq.text_of("name").unwrap_or("").to_string(), fps, ..Default::default() };
    cut.start_tc = seq.child("timecode").and_then(|t| t.f64_of("frame")).map_or(0.0, |f| f / exact_fps(fps));
    let media = seq.child("media").ok_or_else(|| anyhow::anyhow!("the sequence has no <media>"))?;
    if let Some(sc) = media.child("video").and_then(|v| v.child("format")).and_then(|f| f.child("samplecharacteristics")) {
        cut.width = sc.i64_of("width").unwrap_or(0) as u32;
        cut.height = sc.i64_of("height").unwrap_or(0) as u32;
    }
    if let Some(sc) = media.child("audio").and_then(|v| v.child("format")).and_then(|f| f.child("samplecharacteristics")) {
        cut.sample_rate = sc.i64_of("samplerate").unwrap_or(0) as u32;
    }
    let mut files: HashMap<String, FileInfo> = HashMap::new();
    let mut links: Vec<(String, Vec<String>)> = Vec::new(); // clip item id → linked ids
    let secs = |f: i64| f as f64 / exact_fps(fps);
    let mut audio_tracks: Vec<(CutTrack, Vec<Option<usize>>, Vec<String>)> = Vec::new(); // track, each clip's source channel, clip ids
    for video in [true, false] {
        let Some(kind) = media.child(if video { "video" } else { "audio" }) else { continue };
        for (tn, track) in kind.children_named("track").enumerate() {
            let mut t = CutTrack { video, name: format!("{} {}", if video { "Video" } else { "Audio" }, tn + 1), muted: !track.bool_of("enabled").unwrap_or(true), locked: track.bool_of("locked").unwrap_or(false), clips: Vec::new() };
            let mut channels: Vec<Option<usize>> = Vec::new();
            let mut ids: Vec<String> = Vec::new();
            let mut pending: Option<f64> = None; // a transition's length waiting for the clip after it
            for item in &track.children {
                match item.name.as_str() {
                    "transitionitem" => {
                        let (s, e) = (item.i64_of("start").unwrap_or(0), item.i64_of("end").unwrap_or(0));
                        let len = secs((e - s).max(0));
                        let alignment = item.text_of("alignment").unwrap_or("center");
                        // Before a clip item: its dissolve (or fade in); after the last one: a fade out.
                        pending = Some(len);
                        if let Some(prev) = t.clips.last_mut() {
                            if alignment == "end-black" || alignment == "end" {
                                prev.fade_out = len;
                                pending = None;
                            }
                        }
                    }
                    "clipitem" => {
                        let (start, end) = (item.i64_of("start").unwrap_or(-1), item.i64_of("end").unwrap_or(-1));
                        if start < 0 || end <= start {
                            warnings.push(format!("{}: the clip item {} is not placed on the timeline and was skipped", t.name, item.text_of("name").unwrap_or("?")));
                            continue;
                        }
                        let info = item.child("file").and_then(|f| file_info(&mut files, f, fps, dir));
                        // Clip items count from the sequence's first frame whatever its start timecode.
                        let mut c = CutClip { start: secs(start), end: secs(end), ..Default::default() };
                        c.src_in = secs(item.i64_of("in").unwrap_or(0));
                        c.name = item.text_of("name").unwrap_or("").to_string();
                        c.muted = !item.bool_of("enabled").unwrap_or(true);
                        if let Some(i) = info {
                            c.path = i.path.clone();
                            if c.name.is_empty() {
                                c.name = i.name.clone();
                            }
                            c.media.duration = i.duration;
                            c.media.timecode = i.timecode;
                            (c.media.has_video, c.media.has_audio) = (i.has_video, i.has_audio);
                            (c.media.width, c.media.height, c.media.sample_rate) = (i.width, i.height, i.sample_rate);
                        }
                        for f in item.children_named("filter") {
                            let Some(e) = f.child("effect") else { continue };
                            let id = e.text_of("effectid").unwrap_or("").to_ascii_lowercase();
                            let name = e.text_of("name").unwrap_or("");
                            match id.as_str() {
                                "timeremap" => {
                                    if let Some(v) = param(e, "speed").and_then(|v| v.parse::<f64>().ok()) {
                                        c.rate = (v / 100.0).abs().max(0.001);
                                        c.reverse = v < 0.0;
                                    }
                                    if param(e, "reverse").is_some_and(|v| v.eq_ignore_ascii_case("true")) {
                                        c.reverse = true;
                                    }
                                    if param(e, "variablespeed").is_some_and(|v| v != "0") {
                                        warnings.push(format!("{}: the event at {:.3} s has a variable speed ramp; its average speed is used", t.name, c.start));
                                    }
                                }
                                "opacity" => {
                                    if let Some(v) = param(e, "opacity").and_then(|v| v.parse::<f32>().ok()) {
                                        c.opacity = (v / 100.0).clamp(0.0, 1.0);
                                    }
                                }
                                "audiolevels" => {
                                    if let Some(v) = param(e, "level").and_then(|v| v.parse::<f32>().ok()) {
                                        c.gain_db = if v > 0.0 { 20.0 * v.log10() } else { -120.0 };
                                    }
                                }
                                _ => warnings.push(format!("{}: the event at {:.3} s: the {} filter is not available in Milan Pro and was left out", t.name, c.start, if name.is_empty() { &id } else { name })),
                            }
                        }
                        if let Some(len) = pending.take() {
                            match t.clips.last_mut() {
                                Some(prev) if prev.end > c.start + 1e-6 => c.transition = Some(CutTransition { plugin: DISSOLVE.into(), duration: len.min(prev.end - c.start) }),
                                _ => c.fade_in = len,
                            }
                        }
                        let id = item.attr("id").unwrap_or("").to_string();
                        let linked: Vec<String> = item.children_named("link").filter_map(|l| l.text_of("linkclipref")).map(str::to_string).collect();
                        if !linked.is_empty() {
                            links.push((id.clone(), linked));
                        }
                        channels.push(item.child("sourcetrack").and_then(|s| s.i64_of("trackindex")).map(|i| i as usize));
                        ids.push(id);
                        t.clips.push(c);
                    }
                    _ => {}
                }
            }
            if let (Some(len), Some(prev)) = (pending, t.clips.last_mut()) {
                prev.fade_out = len;
            }
            if video {
                cut.tracks.push(t);
            } else {
                audio_tracks.push((t, channels, ids));
            }
        }
    }
    // Groups from links (a union over every clip item's list).
    let mut group_of: HashMap<String, String> = HashMap::new();
    for (id, linked) in &links {
        let existing = std::iter::once(id).chain(linked).find_map(|i| group_of.get(i).cloned());
        let g = existing.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        for i in std::iter::once(id).chain(linked) {
            group_of.insert(i.clone(), g.clone());
        }
    }
    // Premiere and Resolve write a stereo event as channel 1 on one track and channel 2 on the next: one event here.
    let mut merged = 0;
    let mut i = 0;
    while i < audio_tracks.len() {
        let pair = i + 1 < audio_tracks.len() && {
            let (a, b) = (&audio_tracks[i], &audio_tracks[i + 1]);
            !a.0.clips.is_empty()
                && a.0.clips.len() == b.0.clips.len()
                && a.0.clips.iter().zip(&b.0.clips).all(|(x, y)| x.path == y.path && (x.start - y.start).abs() < 1e-9 && (x.end - y.end).abs() < 1e-9 && (x.src_in - y.src_in).abs() < 1e-9)
                && b.1.iter().all(|c| *c == Some(2))
                && a.1.iter().all(|c| c.is_none_or(|c| c == 1))
        };
        if pair {
            audio_tracks.remove(i + 1);
            merged += 1;
        }
        i += 1;
    }
    if merged > 0 {
        warnings.push(format!("{merged} audio track pair(s) carrying channel 1 and channel 2 of the same events were merged into stereo events"));
    }
    // Video clip item ids in track order, for the groups.
    let placed = |c: &&El| c.i64_of("start").unwrap_or(-1) >= 0 && c.i64_of("end").unwrap_or(-1) > c.i64_of("start").unwrap_or(-1);
    let video_ids: Vec<Vec<String>> = media.child("video").map(|v| v.children_named("track").map(|t| t.children_named("clipitem").filter(placed).map(|c| c.attr("id").unwrap_or("").to_string()).collect()).collect()).unwrap_or_default();
    for (t, ids) in cut.tracks.iter_mut().zip(&video_ids) {
        for (c, id) in t.clips.iter_mut().zip(ids) {
            c.group = group_of.get(id).cloned();
        }
    }
    for (n, (mut t, _, ids)) in audio_tracks.into_iter().enumerate() {
        t.name = format!("Audio {}", n + 1);
        for (c, id) in t.clips.iter_mut().zip(&ids) {
            c.group = group_of.get(id).cloned();
        }
        cut.tracks.push(t);
    }
    for m in seq.children_named("marker") {
        let at = secs(m.i64_of("in").unwrap_or(0));
        let out = m.i64_of("out").unwrap_or(-1);
        cut.markers.push(CutMarker { at, end: (out > 0).then(|| secs(out)).filter(|e| *e > at), name: m.text_of("name").unwrap_or("").to_string() });
    }
    if cut.tracks.iter().all(|t| t.clips.is_empty()) {
        warnings.push("The sequence has no clips".into());
    }
    Ok((cut, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interchange::cutlist::{self, to_project};

    #[test]
    fn round_trip() {
        let p = cutlist::tests::sample();
        let (cut, _) = from_project(&p, "FCP7 XML");
        let (text, notes) = write(&cut);
        assert!(text.contains("<xmeml version=\"5\">") && text.contains("Cross Dissolve") && text.contains("timeremap"), "{text}");
        assert!(notes.is_empty(), "{notes:?}");
        let (back, w) = read(&text, None).unwrap();
        assert!(w.iter().all(|w| w.contains("merged")), "{w:?}");
        assert_eq!((back.fps, back.width, back.height, back.sample_rate), (25.0, 1280, 720, 48000));
        assert_eq!(back.tracks.len(), 2, "the stereo pair merges back");
        let (v, v0) = (&back.tracks[0], &cut.tracks[0]);
        assert_eq!(v.clips.len(), 3);
        for (c, c0) in v.clips.iter().zip(&v0.clips) {
            assert_eq!((c.start, c.end, c.src_in, c.rate, c.reverse, c.path.as_str()), (c0.start, c0.end, c0.src_in, c0.rate, c0.reverse, c0.path.as_str()));
            assert_eq!(c.transition.as_ref().map(|t| t.duration), c0.transition.as_ref().map(|t| t.duration));
            assert!((c.opacity - c0.opacity).abs() < 1e-4 && c.fade_in == c0.fade_in && c.fade_out == c0.fade_out, "{c:?}");
        }
        assert_eq!(v.clips[0].media.duration, 60.0);
        let a = &back.tracks[1].clips[0];
        assert!((a.gain_db + 6.0).abs() < 1e-3 && a.fade_out == 0.6, "{a:?}");
        assert_eq!(back.markers, cut.markers);
        let (q, _) = to_project(&back, &cutlist::tests::probed(&p));
        assert_eq!(q.timeline.video_tracks[0].clips[1].rate, 2.0);
    }

    #[test]
    fn reads_the_hand_written_resolve_sequence() {
        let text = include_str!("../../tests/fixtures/resolve_sample.xml");
        let (cut, w) = read(text, None).unwrap();
        assert_eq!((cut.fps, cut.width, cut.height, cut.name.as_str()), (23.976, 1920, 1080, "Resolve Sample"));
        let v = &cut.tracks[0];
        assert_eq!(v.clips.len(), 2);
        let f = exact_fps(23.976);
        assert!((v.clips[0].src_in - 24.0 / f).abs() < 1e-9 && v.clips[0].path.ends_with("A001_C001.mov"), "{:?}", v.clips[0]);
        assert!((v.clips[0].media.timecode - 3603.6).abs() < 1e-6, "01:00:00:00 at 23.976 NDF is 86400 frames");
        assert!(v.clips[1].transition.as_ref().is_some_and(|t| (t.duration - 24.0 / f).abs() < 1e-9), "{:?}", v.clips[1]);
        assert!((v.clips[1].rate - 0.5).abs() < 1e-9 && (v.clips[1].opacity - 0.8).abs() < 1e-6);
        assert!((v.clips[1].fade_out - 12.0 / f).abs() < 1e-9);
        assert_eq!(cut.tracks.len(), 2, "{:?}", cut.tracks.iter().map(|t| &t.name).collect::<Vec<_>>());
        assert!((cut.tracks[1].clips[0].gain_db + 6.02).abs() < 0.01);
        assert_eq!(cut.tracks[1].clips[0].group, v.clips[0].group);
        assert!(v.clips[0].group.is_some());
        assert_eq!((cut.markers.len(), cut.markers[1].end.is_some()), (2, true));
        assert!(w.iter().any(|w| w.contains("merged")) && w.iter().any(|w| w.contains("Gaussian Blur")), "{w:?}");
    }
}
