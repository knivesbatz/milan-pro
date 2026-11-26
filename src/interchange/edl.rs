//! R28: edit decision lists (File › Import / Export › EDL Text File...).
//!
//! Two kinds share the menu rows, told apart by the file:
//! - **CMX 3600** (`.edl`), what Resolve, Premiere, Avid and most other tools read: one video track (`V`) and the
//!   audio channels `A`/`A2`/`A3`/`A4`, cuts, dissolves (`D`, the from-source line then the to-source line) and
//!   `M2` speed lines, timecodes drop-frame (`;`, `FCM: DROP FRAME`) or non-drop. Reel names come from the file
//!   names (eight capitals and digits, unique), and `* FROM CLIP NAME:` / `* TO CLIP NAME:` / `* SOURCE FILE:`
//!   comments name the files, so the reader finds them again.
//! - **VEGAS's own EDL text** (`.txt`, the `"ID";"Track";...` table VEGAS's File › Export › EDL Text File writes
//!   and its Import reads): every track, times in milliseconds, fades and levels, no transitions.
//!
//! Both go through the cut list (`cutlist.rs`), so what an EDL cannot hold is listed the same way.
use std::collections::HashMap;
use std::path::Path;
use crate::interchange::cutlist::{Cut, CutClip, CutTrack, CutTransition, DISSOLVE, from_project};
use crate::project::{Project, RulerFormat, exact_fps, is_drop, parse_smpte, smpte};

/// The EDL kind a file holds.
pub fn is_vegas_txt(text: &str) -> bool {
    text.trim_start_matches('\u{feff}').trim_start().starts_with("\"ID\";\"Track\"")
}

/// Write `p` as an EDL: VEGAS's text table for a `.txt`, CMX 3600 otherwise. Returns what the EDL could not hold.
pub fn export(p: &Project, path: &Path) -> anyhow::Result<Vec<String>> {
    let vegas = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("txt"));
    let (cut, mut notes) = from_project(p, if vegas { "a VEGAS EDL text file" } else { "a CMX 3600 EDL" });
    let (text, more) = if vegas {
        write_vegas_txt(&cut)
    } else {
        let drop = is_drop(p.settings.frame_rate) && p.settings.ruler.format != RulerFormat::SmpteNonDrop;
        write_cmx(&cut, drop)
    };
    notes.extend(more);
    std::fs::write(path, text)?;
    Ok(notes)
}

/// Read either kind; `fps` is the project frame rate to count a CMX EDL's timecodes at (its own drop-frame marks
/// win), `dir` the EDL's folder (where bare file names are looked for).
pub fn read(text: &str, fps: f64, dir: Option<&Path>) -> anyhow::Result<(Cut, Vec<String>)> {
    if is_vegas_txt(text) { read_vegas_txt(text, fps) } else { read_cmx(text, fps, dir) }
}

// ---- CMX 3600 ----

/// Timecode at `fps` (drop-frame numbering when `drop`).
fn tc(seconds: f64, fps: f64, drop: bool) -> String {
    smpte(seconds.max(0.0), exact_fps(fps), fps.round() as u64, drop)
}

fn frames(seconds: f64, fps: f64) -> i64 {
    (seconds * exact_fps(fps)).round() as i64
}

/// Eight capitals and digits from a file name ("A001_C002.mov" → "A001C002"), unique among `taken`.
fn reel_name(path: &str, taken: &mut HashMap<String, String>) -> String {
    if let Some(r) = taken.get(path) {
        return r.clone();
    }
    let stem = Path::new(path).file_stem().map_or(path.to_string(), |s| s.to_string_lossy().into_owned());
    let mut base: String = stem.to_uppercase().chars().filter(|c| c.is_ascii_alphanumeric()).take(8).collect();
    if base.is_empty() || base == "BL" || base == "AX" {
        base = "REEL".into();
    }
    let mut reel = base.clone();
    let mut n = 1;
    while taken.values().any(|r| *r == reel) {
        n += 1;
        let digits = n.to_string();
        let keep = 8usize.saturating_sub(digits.len()).min(base.len());
        reel = format!("{}{digits}", &base[..keep]);
    }
    taken.insert(path.to_string(), reel.clone());
    reel
}

fn file_name(path: &str) -> String {
    Path::new(path).file_name().map_or(path.to_string(), |n| n.to_string_lossy().into_owned())
}

/// CMX 3600 text of `cut`; `drop` picks drop-frame timecode. Returns the text and what the EDL left out (video
/// tracks past the first, audio tracks past the fourth).
pub fn write_cmx(cut: &Cut, drop: bool) -> (String, Vec<String>) {
    let fps = if cut.fps > 0.0 { cut.fps } else { 29.97 };
    let drop = drop && is_drop(fps);
    let mut notes = Vec::new();
    let mut out = format!("TITLE: {}\nFCM: {}\n", if cut.name.is_empty() { "Untitled" } else { &cut.name }, if drop { "DROP FRAME" } else { "NON-DROP FRAME" });
    let mut reels: HashMap<String, String> = HashMap::new();
    // (record in, track order, lines with "###" for the event number)
    let mut blocks: Vec<(f64, usize, Vec<String>)> = Vec::new();
    let (mut video_n, mut audio_n) = (0, 0);
    for t in &cut.tracks {
        let chan = if t.video {
            video_n += 1;
            if video_n > 1 {
                notes.push(format!("{}: a CMX 3600 EDL has one video track; this one was left out", t.name));
                continue;
            }
            "V".to_string()
        } else {
            audio_n += 1;
            if audio_n > 4 {
                notes.push(format!("{}: a CMX 3600 EDL has four audio channels (A, A2, A3, A4); this track was left out", t.name));
                continue;
            }
            if audio_n == 1 { "A".to_string() } else { format!("A{audio_n}") }
        };
        let order = if t.video { 0 } else { audio_n };
        let t_tc = |s: f64| tc(s + cut.start_tc, fps, drop);
        let s_tc = |c: &CutClip, s: f64| tc(s + c.media.timecode, fps, drop);
        for (i, c) in t.clips.iter().enumerate() {
            let mut lines = Vec::new();
            // The event ends where the next one's dissolve begins: the dissolve's to-line plays the rest.
            let next_tr = t.clips.get(i + 1).and_then(|n| n.transition.as_ref().map(|tr| (n.start, tr)));
            let rec_out = next_tr.map_or(c.end, |(s, _)| s.min(c.end).max(c.start));
            let reel = reel_name(&c.path, &mut reels);
            let (src_in, src_out) = (s_tc(c, c.src_in), s_tc(c, c.src_out()));
            let m2 = |reel: &str, c: &CutClip| {
                let speed = fps * c.rate * if c.reverse { -1.0 } else { 1.0 };
                let speed = if speed < 0.0 { format!("-{:05.1}", -speed) } else { format!("{speed:05.1}") };
                format!("M2   {reel:<8} {speed:>6}                {}", s_tc(c, c.src_in))
            };
            let retimed = (c.rate - 1.0).abs() > 1e-6 || c.reverse;
            match &c.transition {
                Some(tr) if i > 0 => {
                    // From-line: the previous source where the dissolve starts; to-line: this source over the rest.
                    let prev = &t.clips[i - 1];
                    let prev_reel = reel_name(&prev.path, &mut reels);
                    let prev_src = prev.src_in + (c.start - prev.start).max(0.0) * prev.rate;
                    lines.push(format!("###  {prev_reel:<8} {chan:<5} C        {0} {0} {1} {1}", s_tc(prev, prev_src), t_tc(c.start)));
                    lines.push(format!("###  {reel:<8} {chan:<5} D    {:>3} {src_in} {src_out} {} {}", frames(tr.duration, fps).max(1), t_tc(c.start), t_tc(rec_out)));
                    if retimed {
                        lines.push(m2(&reel, c));
                    }
                    lines.push(format!("* FROM CLIP NAME: {}", file_name(&prev.path)));
                    lines.push(format!("* TO CLIP NAME: {}", file_name(&c.path)));
                    if tr.plugin != DISSOLVE {
                        lines.push(format!("* EFFECT NAME: {}", crate::interchange::cutlist::plugin_name(&tr.plugin)));
                    }
                }
                _ => {
                    lines.push(format!("###  {reel:<8} {chan:<5} C        {src_in} {src_out} {} {}", t_tc(c.start), t_tc(rec_out)));
                    if retimed {
                        lines.push(m2(&reel, c));
                    }
                    lines.push(format!("* FROM CLIP NAME: {}", file_name(&c.path)));
                }
            }
            if !c.path.is_empty() {
                lines.push(format!("* SOURCE FILE: {}", c.path));
            }
            if c.fade_in > 0.0 || c.fade_out > 0.0 {
                notes.push(format!("{}: the event at {:.3} s: its fades are not in a CMX 3600 EDL", t.name, c.start));
            }
            if !c.name.is_empty() && c.name != file_name(&c.path) {
                lines.push(format!("* COMMENT: {}", c.name));
            }
            blocks.push((c.start, order, lines));
        }
    }
    blocks.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    for (n, (_, _, lines)) in blocks.iter().enumerate() {
        for l in lines {
            out.push_str(&l.replacen("###", &format!("{:03}", n + 1), 1));
            out.push('\n');
        }
    }
    if !cut.markers.is_empty() {
        notes.push("Markers and regions are not in a CMX 3600 EDL".into());
    }
    (out, notes)
}

/// The channels an event line's third field addresses: (video, audio channels 1-4).
fn channels(field: &str) -> (bool, Vec<usize>) {
    let f = field.to_ascii_uppercase();
    let video = f.contains('V') || f == "B";
    let mut audio = Vec::new();
    for part in f.split('/') {
        match part {
            "A" | "A1" | "B" => audio.push(1),
            "AA" => audio.extend([1, 2]),
            "A2" | "A3" | "A4" => audio.push(part[1..].parse().unwrap_or(1)),
            "AA/V" => audio.extend([1, 2]),
            _ => {}
        }
    }
    audio.sort_unstable();
    audio.dedup();
    (video, audio)
}

/// One parsed event line.
struct Line {
    num: u32,
    reel: String,
    video: bool,
    audio: Vec<usize>,
    dissolve: Option<f64>,
    src_in: f64,
    rec_in: f64,
    rec_out: f64,
    /// `M2`: source frames per second (negative backwards).
    speed: Option<f64>,
    from_name: Option<String>,
    to_name: Option<String>,
    source_file: Option<String>,
    comment: Option<String>,
}

/// Read a CMX 3600 EDL. Timecodes count at `fps` (29.97 / 59.94 when the EDL marks drop frame); `dir` is where a
/// clip name alone is looked for.
pub fn read_cmx(text: &str, fps: f64, dir: Option<&Path>) -> anyhow::Result<(Cut, Vec<String>)> {
    let text = text.trim_start_matches('\u{feff}');
    let mut warnings = Vec::new();
    let mut cut = Cut { fps, ..Default::default() };
    let mut drop = false;
    let has_semicolon_tc = text.lines().any(|l| l.split_whitespace().any(|w| w.len() == 11 && w.matches(';').count() >= 1 && w.matches(':').count() >= 2));
    if text.lines().any(|l| l.trim().eq_ignore_ascii_case("FCM: DROP FRAME")) || has_semicolon_tc {
        drop = true;
        cut.fps = if fps.round() == 60.0 { 59.94 } else { 29.97 };
    }
    let fps = cut.fps;
    let (rate, nominal) = (exact_fps(fps), fps.round() as u64);
    let parse_tc = |s: &str, drop: bool| parse_smpte(s, rate, nominal, drop);
    let mut lines: Vec<Line> = Vec::new();
    let mut cur_drop = drop;
    for raw in text.lines() {
        let l = raw.trim();
        if l.is_empty() {
            continue;
        }
        if let Some(title) = l.strip_prefix("TITLE:") {
            cut.name = title.trim().to_string();
            continue;
        }
        if let Some(fcm) = l.strip_prefix("FCM:") {
            cur_drop = fcm.trim().eq_ignore_ascii_case("DROP FRAME") && is_drop(fps);
            continue;
        }
        if let Some(c) = l.strip_prefix('*') {
            let c = c.trim();
            let Some(last) = lines.last_mut() else { continue };
            let after = |key: &str| c.get(..key.len()).filter(|k| k.eq_ignore_ascii_case(key)).map(|_| c[key.len()..].trim().to_string());
            if let Some(v) = after("FROM CLIP NAME:") {
                last.from_name = Some(v);
            } else if let Some(v) = after("TO CLIP NAME:") {
                last.to_name = Some(v);
            } else if let Some(v) = after("SOURCE FILE:") {
                last.source_file = Some(v);
            } else if let Some(v) = after("COMMENT:") {
                last.comment = Some(v);
            }
            continue;
        }
        let f: Vec<&str> = l.split_whitespace().collect();
        if f[0].eq_ignore_ascii_case("M2") {
            if f.len() >= 3 {
                if let (Some(last), Ok(speed)) = (lines.last_mut(), f[2].parse::<f64>()) {
                    last.speed = Some(speed);
                }
            }
            continue;
        }
        // NNN REEL CH KIND [DUR] SRC_IN SRC_OUT REC_IN REC_OUT
        let Ok(num) = f[0].parse::<u32>() else { continue };
        if f.len() < 8 {
            warnings.push(format!("Line not understood: {l}"));
            continue;
        }
        let kind = f[3].to_ascii_uppercase();
        let (dissolve, times) = match kind.as_str() {
            "C" => (None, &f[4..]),
            "D" => (Some(f[4].parse::<f64>().unwrap_or(0.0) / rate), &f[5..]),
            k if k.starts_with('W') => {
                warnings.push(format!("Event {num}: wipe {k} plays as a dissolve"));
                (Some(f[4].parse::<f64>().unwrap_or(0.0) / rate), &f[5..])
            }
            k => {
                warnings.push(format!("Event {num}: edit type {k} is read as a cut"));
                (None, &f[f.len() - 4..])
            }
        };
        if times.len() < 4 {
            warnings.push(format!("Line not understood: {l}"));
            continue;
        }
        let tcs: Option<Vec<f64>> = times[..4].iter().map(|s| parse_tc(s, cur_drop || s.contains(';'))).collect();
        let Some(tcs) = tcs else {
            warnings.push(format!("Event {num}: a timecode was not understood: {l}"));
            continue;
        };
        let (video, audio) = channels(f[2]);
        lines.push(Line { num, reel: f[1].to_string(), video, audio, dissolve, src_in: tcs[0], rec_in: tcs[2], rec_out: tcs[3], speed: None, from_name: None, to_name: None, source_file: None, comment: None });
    }
    if lines.is_empty() {
        anyhow::bail!("no EDL events found");
    }
    // Record timecodes usually start at an hour: the timeline starts at zero.
    let first = lines.iter().map(|l| l.rec_in).fold(f64::MAX, f64::min);
    cut.start_tc = (first / 3600.0).floor() * 3600.0;
    if cut.start_tc > 0.0 {
        warnings.push(format!("Record timecodes start at {}; the timeline starts at 00:00:00:00", tc(cut.start_tc, fps, drop)));
    }
    let resolve = |l: &Line, name: Option<&String>| -> String {
        if let Some(f) = &l.source_file {
            return f.clone();
        }
        match name {
            Some(n) if !n.is_empty() => {
                let p = Path::new(n);
                if p.is_absolute() {
                    return n.clone();
                }
                dir.map(|d| d.join(n)).filter(|p| p.is_file()).map_or(n.clone(), |p| p.display().to_string())
            }
            _ => String::new(),
        }
    };
    let mut video = CutTrack { video: true, name: "V".into(), ..Default::default() };
    let mut audio: Vec<CutTrack> = (1..=4).map(|n| CutTrack { video: false, name: if n == 1 { "A".into() } else { format!("A{n}") }, ..Default::default() }).collect();
    let mut i = 0;
    while i < lines.len() {
        let l = &lines[i];
        // A dissolve: this zero-length from-line and the to-line of the same event number.
        let pair = l.dissolve.is_none() && (l.rec_out - l.rec_in).abs() < 0.5 / rate && lines.get(i + 1).is_some_and(|n| n.num == l.num && n.dissolve.is_some());
        let (from, to) = if pair { (Some(l), &lines[i + 1]) } else { (None, l) };
        i += if pair { 2 } else { 1 };
        let black = |reel: &str| reel.eq_ignore_ascii_case("BL") || reel.eq_ignore_ascii_case("BLACK");
        let tr_len = to.dissolve.unwrap_or(0.0);
        let name = to.to_name.clone().or_else(|| if from.is_none() { to.from_name.clone() } else { None }).unwrap_or_default();
        let path = resolve(to, Some(&name).filter(|n| !n.is_empty()));
        let display = if !name.is_empty() { name.clone() } else { to.reel.clone() };
        for t in std::iter::once(&mut video).filter(|_| to.video).chain(audio.iter_mut().enumerate().filter(|(n, _)| to.audio.contains(&(n + 1))).map(|(_, t)| t)) {
            let (start, end) = (to.rec_in - cut.start_tc, to.rec_out - cut.start_tc);
            if black(&to.reel) {
                // A dissolve to black: the previous event fades out over it (and plays through it).
                if let (Some(_), Some(prev)) = (from, t.clips.last_mut()) {
                    if (prev.end - start).abs() < 0.5 / rate {
                        prev.end = start + tr_len;
                        prev.fade_out = tr_len;
                    }
                }
                continue;
            }
            let mut c = CutClip { path: path.clone(), name: display.clone(), start, end, src_in: to.src_in, src_tc: true, ..Default::default() };
            if let Some(speed) = to.speed {
                c.rate = (speed / fps).abs().max(0.001);
                c.reverse = speed < 0.0;
            }
            if let Some(f) = from {
                if black(&f.reel) || t.clips.last().is_none_or(|prev| (prev.end - start).abs() > 0.5 / rate) {
                    c.fade_in = tr_len; // from black, or from nothing the EDL names before it
                } else if let Some(prev) = t.clips.last_mut() {
                    prev.end = start + tr_len; // the previous event plays on under the dissolve
                    c.transition = Some(CutTransition { plugin: DISSOLVE.into(), duration: tr_len });
                }
            } else if to.dissolve.is_some() {
                c.fade_in = tr_len;
            }
            t.clips.push(c);
        }
    }
    if !video.clips.is_empty() {
        cut.tracks.push(video);
    }
    cut.tracks.extend(audio.into_iter().filter(|t| !t.clips.is_empty()));
    Ok((cut, warnings))
}

// ---- VEGAS EDL text ----

const VEGAS_HEADER: &str = "\"ID\";\"Track\";\"StartTime\";\"Length\";\"PlayRate\";\"Locked\";\"Normalized\";\"StretchMethod\";\"Looped\";\"OnRuler\";\"MediaType\";\"FileName\";\"Stream\";\"StreamStart\";\"StreamLength\";\"FadeTimeIn\";\"FadeTimeOut\";\"SustainGain\";\"CurveIn\";\"GainIn\";\"CurveOut\";\"GainOut\";\"Layer\";\"Color\";\"CurveInR\";\"CurveOutR\";\"PlayPitch\";\"LockPitch\";\"FirstChannel\";\"Channels\"";

/// VEGAS's EDL text table. Transitions become the overlap they are (VEGAS's automatic crossfade).
pub fn write_vegas_txt(cut: &Cut) -> (String, Vec<String>) {
    let mut out = format!("{VEGAS_HEADER}\r\n");
    let mut notes = Vec::new();
    let mut id = 1;
    for (track, t) in cut.tracks.iter().enumerate() {
        for c in &t.clips {
            let ms = |s: f64| format!("{:.4}", s * 1000.0);
            let gain = if t.video { c.opacity } else { 10f32.powf(c.gain_db / 20.0) };
            let fade_in = c.transition.as_ref().map_or(c.fade_in, |tr| tr.duration);
            let media = if t.video { "VIDEO" } else { "AUDIO" };
            let rate = c.rate * if c.reverse { -1.0 } else { 1.0 };
            out.push_str(&format!(
                "{id}; {track}; {}; {}; {rate:.6}; FALSE; FALSE; 0; TRUE; FALSE; {media}; \"{}\"; 0; {}; {}; {}; {}; {gain:.6}; 4; 0.000000; 4; 0.000000; 0; -1; 4; 4; 0.000000; FALSE; 0; 0\r\n",
                ms(c.start),
                ms(c.duration()),
                c.path,
                ms(c.src_in),
                ms(c.src_span()),
                ms(fade_in),
                ms(c.fade_out)
            ));
            if c.transition.as_ref().is_some_and(|tr| tr.plugin != DISSOLVE) {
                notes.push(format!("{}: the event at {:.3} s: its transition goes out as a crossfade", t.name, c.start));
            }
            id += 1;
        }
    }
    if !cut.markers.is_empty() {
        notes.push("Markers and regions are not in a VEGAS EDL text file".into());
    }
    (out, notes)
}

/// Read VEGAS's EDL text table (`fps` is the project frame rate; the table has none).
pub fn read_vegas_txt(text: &str, fps: f64) -> anyhow::Result<(Cut, Vec<String>)> {
    let text = text.trim_start_matches('\u{feff}');
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header = lines.next().ok_or_else(|| anyhow::anyhow!("empty EDL"))?;
    let cols: Vec<String> = header.split(';').map(|c| c.trim().trim_matches('"').to_string()).collect();
    let col = |name: &str| cols.iter().position(|c| c == name);
    let need = |name: &str| col(name).ok_or_else(|| anyhow::anyhow!("the EDL has no {name} column"));
    let (i_track, i_start, i_len, i_media, i_file) = (need("Track")?, need("StartTime")?, need("Length")?, need("MediaType")?, need("FileName")?);
    let (i_rate, i_ss, i_fi, i_fo, i_gain, i_locked) = (col("PlayRate"), col("StreamStart"), col("FadeTimeIn"), col("FadeTimeOut"), col("SustainGain"), col("Locked"));
    let mut warnings = Vec::new();
    let mut cut = Cut { fps, ..Default::default() };
    // Tracks in order of first use, by VEGAS track index.
    let mut tracks: Vec<(i64, bool, CutTrack)> = Vec::new();
    for (n, l) in lines.enumerate() {
        let f: Vec<&str> = l.split(';').map(str::trim).collect();
        let get = |i: Option<usize>| i.and_then(|i| f.get(i)).map(|s| s.trim_matches('"'));
        let num = |i: Option<usize>| get(i).and_then(|s| s.parse::<f64>().ok());
        let (Some(track), Some(start), Some(len), Some(media), Some(file)) = (num(Some(i_track)), num(Some(i_start)), num(Some(i_len)), get(Some(i_media)), get(Some(i_file))) else {
            warnings.push(format!("Row {} was not understood", n + 2));
            continue;
        };
        let video = media.eq_ignore_ascii_case("VIDEO");
        if !video && !media.eq_ignore_ascii_case("AUDIO") {
            warnings.push(format!("Row {}: media type {media} was skipped", n + 2));
            continue;
        }
        let rate = num(i_rate).unwrap_or(1.0);
        let gain = num(i_gain).unwrap_or(1.0) as f32;
        let mut c = CutClip {
            path: file.to_string(),
            name: file_name(file),
            start: start / 1000.0,
            end: (start + len) / 1000.0,
            src_in: num(i_ss).unwrap_or(0.0) / 1000.0,
            rate: rate.abs().max(0.001),
            reverse: rate < 0.0,
            fade_in: num(i_fi).unwrap_or(0.0) / 1000.0,
            fade_out: num(i_fo).unwrap_or(0.0) / 1000.0,
            ..Default::default()
        };
        if video {
            c.opacity = gain.clamp(0.0, 1.0);
        } else {
            c.gain_db = if gain > 0.0 { 20.0 * gain.log10() } else { -120.0 };
        }
        let locked = get(i_locked).is_some_and(|s| s.eq_ignore_ascii_case("TRUE"));
        let key = track as i64;
        let slot = match tracks.iter().position(|(k, v, _)| *k == key && *v == video) {
            Some(i) => i,
            None => {
                tracks.push((key, video, CutTrack { video, name: String::new(), ..Default::default() }));
                tracks.len() - 1
            }
        };
        tracks[slot].2.locked |= locked;
        tracks[slot].2.clips.push(c);
    }
    tracks.sort_by_key(|(k, v, _)| (!*v, *k));
    for (_, _, mut t) in tracks {
        t.clips.sort_by(|a, b| a.start.total_cmp(&b.start));
        cut.tracks.push(t);
    }
    if cut.tracks.is_empty() {
        anyhow::bail!("no events in the EDL");
    }
    Ok((cut, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interchange::cutlist::{self, to_project};

    fn golden(fps: f64) -> Cut {
        let mut p = cutlist::tests::sample();
        p.settings.frame_rate = fps;
        p.name = if fps == 25.0 { "Golden 25".into() } else { "Golden 2997".into() };
        // A clip with its own start timecode (10:00:00:00) on A2, and a fade to black at the end of V.
        p.media_pool[2].timecode = Some(36000.0);
        let mut a2 = crate::project::Track::new("Audio 2", crate::project::TrackKind::Audio);
        let mut c = crate::project::Clip::from_media(&p.media_pool[2], 2.0);
        (c.duration, c.in_point, c.out_point) = (3.0, 1.0, 4.0);
        a2.clips.push(c);
        p.timeline.audio_tracks.push(a2);
        p.timeline.video_tracks[0].clips[2].fade_out = 0.5;
        let (cut, _) = from_project(&p, "a CMX 3600 EDL");
        cut
    }

    /// Writes the golden files to `%TEMP%\milan-agent/r28` for a look before they go into tests/fixtures.
    #[test]
    #[ignore]
    fn dump_goldens() {
        let dir = std::env::temp_dir().join("milan-agent").join("r28");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("golden_25.edl"), write_cmx(&golden(25.0), false).0).unwrap();
        std::fs::write(dir.join("golden_2997df.edl"), write_cmx(&golden(29.97), true).0).unwrap();
    }

    #[test]
    fn cmx_golden_25() {
        let (text, notes) = write_cmx(&golden(25.0), false);
        assert_eq!(text, include_str!("../../tests/fixtures/golden_25.edl").replace("\r\n", "\n"));
        assert!(notes.iter().any(|n| n.contains("Markers")), "{notes:?}");
    }

    #[test]
    fn cmx_golden_2997_drop_frame() {
        let (text, _) = write_cmx(&golden(29.97), true);
        assert_eq!(text, include_str!("../../tests/fixtures/golden_2997df.edl").replace("\r\n", "\n"));
        assert!(text.contains("FCM: DROP FRAME") && text.contains(";"));
    }

    #[test]
    fn cmx_round_trip() {
        for (fps, drop) in [(25.0, false), (29.97, true), (29.97, false), (23.976, false)] {
            let cut = golden(fps);
            let (text, _) = write_cmx(&cut, drop);
            let (back, w) = read_cmx(&text, fps, None).unwrap();
            assert_eq!(back.fps, fps, "{text}");
            assert!(w.is_empty(), "{w:?}");
            assert_eq!(back.name, cut.name);
            assert_eq!(back.tracks.len(), 3, "{text}");
            let (v, v0) = (&back.tracks[0], &cut.tracks[0]);
            assert_eq!(v.clips.len(), 3, "{text}");
            let half = 0.6 / fps; // the sample's times are not all on 29.97 frames: half a frame of rounding is the EDL's
            for (c, c0) in v.clips.iter().zip(&v0.clips) {
                assert!((c.start - c0.start).abs() < half && (c.end - c0.end).abs() < half, "{c:?} vs {c0:?}\n{text}");
                assert_eq!(c.path, c0.path);
                assert!((c.src_in - c0.src_in).abs() < half, "{} {}", c.src_in, c0.src_in);
                assert!((c.rate - c0.rate).abs() < 0.01 && c.reverse == c0.reverse, "{c:?}"); // M2 speeds have one decimal
                assert_eq!(c.transition.is_some(), c0.transition.is_some());
                if let (Some(t), Some(t0)) = (&c.transition, &c0.transition) {
                    assert!((t.duration - t0.duration).abs() < half);
                }
            }
            assert_eq!(v.clips[2].fade_out, 0.0, "fades are not in an EDL");
            // The source timecode of the t.wav event on A2 includes the file's 10:00:00:00.
            let a2 = &back.tracks[2];
            assert!((a2.clips[0].src_in - 36001.0).abs() < half && a2.clips[0].src_tc, "{:?}", a2.clips[0]);
            // Through the probe the timecode comes off again.
            let p = cutlist::tests::sample();
            let mut probed = cutlist::tests::probed(&p);
            probed.get_mut(r"C:\m\t.wav").unwrap().as_mut().unwrap().timecode = Some(36000.0);
            let (q, _) = to_project(&back, &probed);
            assert!((q.timeline.audio_tracks[1].clips[0].in_point - 1.0).abs() < half);
        }
    }

    #[test]
    fn reads_a_resolve_style_edl_with_black_and_an_hour_start() {
        let text = "TITLE: Show\nFCM: NON-DROP FRAME\n001  AX       V     C        00:00:02:00 00:00:07:00 01:00:00:00 01:00:05:00\n* FROM CLIP NAME: a.mp4\n002  AX       V     C        00:00:07:00 00:00:07:00 01:00:05:00 01:00:05:00\n002  BL       V     D    025 00:00:00:00 00:00:01:00 01:00:05:00 01:00:06:00\n* FROM CLIP NAME: a.mp4\n003  AX       AA    C        00:00:00:00 00:00:06:00 01:00:00:00 01:00:06:00\n* FROM CLIP NAME: t.wav\n";
        let (cut, w) = read_cmx(text, 25.0, None).unwrap();
        assert_eq!(cut.start_tc, 3600.0);
        assert!(w[0].contains("01:00:00:00"), "{w:?}");
        let v = &cut.tracks[0];
        assert_eq!(v.clips.len(), 1);
        assert_eq!((v.clips[0].start, v.clips[0].end, v.clips[0].fade_out, v.clips[0].src_in), (0.0, 6.0, 1.0, 2.0));
        assert_eq!((cut.tracks[1].name.as_str(), cut.tracks[2].name.as_str(), cut.tracks[2].clips[0].path.as_str()), ("A", "A2", "t.wav"));
    }

    #[test]
    fn vegas_text_round_trip() {
        let p = cutlist::tests::sample();
        let (cut, _) = from_project(&p, "a VEGAS EDL text file");
        let (text, _) = write_vegas_txt(&cut);
        assert!(is_vegas_txt(&text) && text.starts_with(VEGAS_HEADER));
        let (back, w) = read_vegas_txt(&text, 25.0).unwrap();
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(back.tracks.len(), 2);
        let (c, c0) = (&back.tracks[0].clips[1], &cut.tracks[0].clips[1]);
        assert_eq!((c.start, c.end, c.src_in, c.rate, c.fade_in), (c0.start, c0.end, c0.src_in, c0.rate, 1.0));
        assert!((back.tracks[1].clips[0].gain_db + 6.0).abs() < 1e-3);
        assert!((back.tracks[0].clips[0].opacity - 0.5).abs() < 1e-6);
        assert!(back.tracks[0].clips[2].reverse);
    }
}
