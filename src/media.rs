use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use crate::project::{Mastering, MediaItem, MediaType, Sequence};

/// How a picked file is opened (R38: VEGAS's "Open still image sequence" check box).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ImportOpts {
    /// Open a numbered still as the whole sequence it belongs to, at this frame rate (the project's).
    pub sequence: Option<f64>,
}

pub fn probe_media(path: &Path) -> anyhow::Result<MediaItem> {
    probe_media_with(path, ImportOpts::default())
}

pub fn probe_media_with(path: &Path, opts: ImportOpts) -> anyhow::Result<MediaItem> {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let ext = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();

    // R6a: a Milan Pro project is a nested timeline.
    if PROJECT_EXTS.contains(&ext.as_str()) {
        return crate::engine::nested::probe(path);
    }
    let media_type = classify_extension(&ext);
    let mut item = MediaItem { id: uuid::Uuid::new_v4().to_string(), path: path.to_path_buf(), name, media_type, ..Default::default() };

    // R41: RAW camera formats are offline items that say why, never read as something else.
    if let Some(why) = crate::camera::raw_reason(&ext) {
        anyhow::ensure!(path.exists(), "file not found");
        item.unsupported = Some(why);
        item.duration = 5.0;
        return Ok(item);
    }
    if path.is_dir() {
        anyhow::bail!("a folder: use File > Import > Camera Card Folder, or drop it on Project Media");
    }

    // R38: a numbered still opened as its sequence (the image2 demuxer at the project's rate).
    // The item is the run's first file, whichever of its files was picked.
    let seq = opts.sequence.filter(|_| media_type == MediaType::Image).and_then(|fps| {
        let (pattern, start, count) = detect_sequence(path)?;
        Some(Sequence { pattern, start, count, fps })
    });
    if let Some(s) = &seq {
        item.path = path.with_file_name(s.file_name(s.start));
        item.name = s.file_name(s.start);
    }
    let mut cmd = crate::proc::ffprobe();
    cmd.args(["-v", "error", "-print_format", "json", "-show_format", "-show_streams", "-show_chapters"]);
    match &seq {
        Some(s) => {
            cmd.args(["-f", "image2", "-pattern_type", "sequence", "-start_number", &s.start.to_string(), "-framerate", &crate::render::fps_str(s.fps), "-i"]).arg(s.input_path(&item.path));
        }
        None => {
            cmd.arg(path);
        }
    }
    let output = crate::proc::output(&mut cmd).map_err(|e| anyhow::anyhow!(crate::proc::tool_error(&e, "ffprobe")))?;
    if !output.status.success() && item.media_type != MediaType::Image {
        let err = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("unreadable media: {}", err.lines().next().unwrap_or("unknown error").trim());
    }
    parse_ffprobe(&String::from_utf8_lossy(&output.stdout), &mut item);

    if let Some(s) = seq {
        item.media_type = MediaType::Video;
        item.frame_rate = Some(s.fps);
        item.duration = s.count as f64 / s.fps;
        item.sequence = Some(s);
    }
    // R38: an animated GIF or WebP whose length ffprobe does not know: count its frames.
    if item.media_type == MediaType::Video && item.sequence.is_none() && IMAGE_EXTS.contains(&ext.as_str()) && item.duration <= 0.0 {
        if let Some(n) = count_packets(path) {
            item.duration = n as f64 / item.frame_rate.filter(|f| *f > 0.0).unwrap_or(10.0);
        }
    }
    // R41: a spanned clip's parts, and its name after the first part; timecode from the H.264 / MPEG-2 SEI (AVCHD
    // and XAVC-S write it there, not as a tag).
    if ext == "ffconcat" {
        item.spanned = crate::camera::list_members(path);
        if let Some(first) = item.spanned.first() {
            item.name = format!("{} (+{} spanned)", first.file_name().unwrap_or_default().to_string_lossy(), item.spanned.len().saturating_sub(1));
        }
    }
    // R40a: HDR10 metadata a container did not carry is in the first frame's SEI (HEVC, AV1, VP9 as ffprobe reads it).
    let want_tc = item.timecode.is_none() && matches!(item.codec.as_deref(), Some("h264" | "hevc" | "mpeg2video"));
    let want_md = item.mastering.is_none() && item.hdr() == Some(crate::project::Hdr::Pq);
    if item.media_type == MediaType::Video && item.sequence.is_none() && (want_tc || want_md) {
        if let Some(frames) = first_frame_side_data(path) {
            if want_tc {
                item.timecode = parse_sei_timecode(&frames, item.frame_rate.unwrap_or(0.0));
            }
            if want_md {
                let side: Vec<serde_json::Value> = frames["frames"].as_array().into_iter().flatten().flat_map(|f| f["side_data_list"].as_array().cloned().unwrap_or_default()).collect();
                item.mastering = mastering_from(&side);
            }
        }
    }
    // R37: a stream whose codec the bundled FFmpeg cannot decode makes the item offline, with the reason.
    if item.media_type != MediaType::Unknown {
        let caps = crate::engine::caps::get();
        item.unsupported = item.codec.as_deref().and_then(|c| crate::engine::caps::unsupported(&caps, c));
    }

    if item.media_type == MediaType::Image && item.duration == 0.0 {
        item.duration = 5.0;
    }
    // A zero-length clip would be invisible on the timeline.
    if item.duration <= 0.0 {
        anyhow::bail!("could not read duration");
    }

    Ok(item)
}

/// Packets of the first video stream (an animated GIF/WebP's frames), read through the whole file.
fn count_packets(path: &Path) -> Option<u64> {
    let mut cmd = crate::proc::ffprobe();
    cmd.args(["-v", "error", "-select_streams", "v:0", "-count_packets", "-show_entries", "stream=nb_read_packets", "-of", "csv=p=0"]).arg(path);
    let out = crate::proc::output(&mut cmd).ok().filter(|o| o.status.success())?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// R40a/R41: the first frame's side data as ffprobe lists it: the SMPTE 12-1 timecode (H.264 / HEVC pic_timing SEI)
/// and the HDR10 mastering display and content light level SEI.
fn first_frame_side_data(path: &Path) -> Option<serde_json::Value> {
    let mut cmd = crate::proc::ffprobe();
    cmd.args(["-v", "error", "-select_streams", "v:0", "-read_intervals", "%+#1", "-show_frames", "-show_entries", "frame=side_data_list", "-of", "json"]).arg(path);
    let out = crate::proc::output(&mut cmd).ok().filter(|o| o.status.success())?;
    serde_json::from_slice(&out.stdout).ok()
}

/// R41: the first frame's SMPTE 12-1 timecode from `first_frame_side_data`; None without one.
fn parse_sei_timecode(frames: &serde_json::Value, fps: f64) -> Option<f64> {
    if fps <= 0.0 {
        return None;
    }
    let tc = frames["frames"]
        .as_array()?
        .iter()
        .flat_map(|f| f["side_data_list"].as_array().cloned().unwrap_or_default())
        .find(|d| d["side_data_type"].as_str().is_some_and(|t| t.contains("12-1 timecode")))?;
    let value = tc["timecodes"].as_array()?.first()?["value"].as_str()?.to_string();
    crate::engine::sync::parse_timecode(&value, fps)
}

/// R38: the numbered sequence `path` belongs to: (printf file name pattern in its folder, `%` doubled; first number;
/// count of consecutive files from it),
/// when at least one more file of the same name, digit width and extension follows in order. The run starts at the
/// lowest number present and stops at the first gap, as VEGAS's "Open still image sequence" does.
pub fn detect_sequence(path: &Path) -> Option<(PathBuf, u32, u32)> {
    let stem = path.file_stem()?.to_str()?;
    let ext = path.extension()?.to_str()?;
    let digits = stem.len() - stem.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 || digits > 9 {
        return None;
    }
    let prefix = &stem[..stem.len() - digits];
    let dir = path.parent()?;
    let mut numbers: Vec<u32> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let (s, x) = name.rsplit_once('.')?;
            if !x.eq_ignore_ascii_case(ext) || s.len() != stem.len() || !s.starts_with(prefix) {
                return None;
            }
            let n = &s[prefix.len()..];
            n.chars().all(|c| c.is_ascii_digit()).then(|| n.parse().ok())?
        })
        .collect();
    numbers.sort_unstable();
    numbers.dedup();
    let own: u32 = stem[prefix.len()..].parse().ok()?;
    let start_idx = numbers.iter().rposition(|&n| n <= own).unwrap_or(0);
    // Walk back to the start of the run that holds `own`, then forward to its end.
    let mut first = start_idx;
    while first > 0 && numbers[first - 1] + 1 == numbers[first] {
        first -= 1;
    }
    let mut last = start_idx;
    while last + 1 < numbers.len() && numbers[last] + 1 == numbers[last + 1] {
        last += 1;
    }
    let count = (last - first + 1) as u32;
    if count < 2 {
        return None;
    }
    let pattern = PathBuf::from(format!("{}%0{digits}d.{ext}", prefix.replace('%', "%%")));
    Some((pattern, numbers[first], count))
}

/// R38: the sequences among picked `paths` (stills with numbered siblings): the first picked file of each, its
/// pattern and the picked files that belong to it. Other paths are left to a plain import.
pub fn sequences_in(paths: &[PathBuf]) -> Vec<(PathBuf, (PathBuf, u32, u32), Vec<PathBuf>)> {
    let mut out: Vec<(PathBuf, (PathBuf, u32, u32), Vec<PathBuf>)> = Vec::new();
    for p in paths {
        let ext = p.extension().unwrap_or_default().to_string_lossy().to_lowercase();
        // CinemaDNG (a folder of DNG frames) is not supported: DNG stills open one by one.
        if classify_extension(&ext) != MediaType::Image || ext == "dng" || !p.is_file() {
            continue;
        }
        let Some(seq) = detect_sequence(p) else { continue };
        match out.iter_mut().find(|s| s.1 == seq) {
            Some(s) => s.2.push(p.clone()),
            None => out.push((p.clone(), seq, vec![p.clone()])),
        }
    }
    out
}

/// Each picked file with its probe result.
pub type ProbeBatch = Vec<(PathBuf, Result<MediaItem, String>)>;

/// Probes imported files on a worker thread so ffprobe never blocks the UI.
pub struct Importer {
    tx: Sender<ProbeBatch>,
    rx: Receiver<ProbeBatch>,
}

impl Importer {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self { tx, rx }
    }

    pub fn import(&self, ctx: &egui::Context, paths: Vec<PathBuf>) {
        self.import_with(ctx, paths.into_iter().map(|p| (p, ImportOpts::default())).collect());
    }

    /// Probe each path with its options. A folder (R41: a camera card) is scanned on the worker and every clip found
    /// comes back keyed by the folder; an empty folder comes back as an error under its path.
    pub fn import_with(&self, ctx: &egui::Context, paths: Vec<(PathBuf, ImportOpts)>) {
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let mut batch: ProbeBatch = Vec::new();
            for (p, opts) in paths {
                if p.is_dir() {
                    let mut notes = Vec::new();
                    let found = crate::camera::import_paths(&p, &mut notes);
                    if found.is_empty() {
                        batch.push((p.clone(), Err(format!("{}: no media found in the folder", p.display()))));
                    }
                    for f in found {
                        let r = probe_media(&f).map_err(|e| format!("{}: {e}", f.file_name().unwrap_or_default().to_string_lossy()));
                        batch.push((p.clone(), r));
                    }
                    for n in notes {
                        batch.push((p.clone(), Err(n)));
                    }
                    continue;
                }
                let r = probe_media_with(&p, opts).map_err(|e| format!("{}: {e}", p.file_name().unwrap_or_default().to_string_lossy()));
                batch.push((p, r));
            }
            let _ = tx.send(batch);
            ctx.request_repaint();
        });
    }

    /// A finished batch of probe results, one per picked file.
    pub fn poll(&self) -> Option<ProbeBatch> {
        self.rx.try_recv().ok()
    }

    /// Where finished batches arrive, for tests that fake a probe.
    #[cfg(test)]
    pub fn sender(&self) -> Sender<ProbeBatch> {
        self.tx.clone()
    }
}

/// Add probed `items` whose path is not in the pool yet (nor earlier in the same batch). An offline pool item (a
/// VEGAS import's missing file) takes the probe in place, keeping its id. True if any was added or filled in.
/// R38: a still and the sequence starting with it are different items.
pub fn merge_new(pool: &mut Vec<MediaItem>, items: Vec<MediaItem>) -> bool {
    let mut done = false;
    for m in items {
        match pool.iter_mut().find(|p| p.path == m.path && p.subclip.is_none() && p.sequence.is_some() == m.sequence.is_some()) {
            None => pool.push(m),
            Some(p) if p.is_offline() && !m.is_offline() => *p = MediaItem { id: std::mem::take(&mut p.id), ..m },
            Some(_) => continue,
        }
        done = true;
    }
    done
}

/// `path` has a usable (probed) item in the pool; an offline one is probed again on import or drop.
/// R38: a still image sequence is not its first file.
pub fn in_pool(pool: &[MediaItem], path: &std::path::Path) -> bool {
    pool.iter().any(|m| m.path == path && m.subclip.is_none() && m.sequence.is_none() && !m.is_offline())
}

/// R38/R41: every file an item reads: a still sequence's frames, a spanned clip's list and parts, else its file.
pub fn files_of(m: &MediaItem) -> Vec<PathBuf> {
    if let Some(s) = &m.sequence {
        return s.files(m.path.parent().unwrap_or(Path::new("")));
    }
    let mut v = vec![m.path.clone()];
    v.extend(m.spanned.iter().cloned());
    v
}

/// File extensions Milan Pro imports, by kind (lower case): the Import dialog, drops and the Explorer use these.
/// R41: `.ffconcat` is a spanned clip's list; the RAW camera extensions import as offline items that say why.
pub const VIDEO_EXTS: &[&str] = &["mp4", "mov", "avi", "mkv", "wmv", "flv", "webm", "mxf", "mpg", "mpeg", "ts", "m2ts", "mts", "m2t", "vob", "3gp", "m4v", "ogv", "ffconcat", "braw", "r3d", "ari", "arx", "crm", "nev"];
pub const AUDIO_EXTS: &[&str] = &["mp3", "wav", "flac", "aac", "ogg", "wma", "aiff", "aif", "opus", "m4a", "ac3", "eac3", "ec3", "dts", "pcm", "mka"];
/// R38: DPX, JPEG 2000, Radiance HDR, DNG (through FFmpeg's TIFF decoder), SVG (needs an FFmpeg built with librsvg;
/// else offline with the reason) and JPEG XL join the stills.
pub const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "tiff", "tif", "bmp", "webp", "gif", "exr", "tga", "psd", "dpx", "jp2", "j2k", "hdr", "dng", "svg", "jxl"];
/// R6a: projects imported as nested timelines, as VEGAS imports a `.veg`.
pub const PROJECT_EXTS: &[&str] = &["mnle"];

pub(crate) fn classify_extension(ext: &str) -> MediaType {
    if VIDEO_EXTS.contains(&ext) || PROJECT_EXTS.contains(&ext) {
        MediaType::Video
    } else if AUDIO_EXTS.contains(&ext) {
        MediaType::Audio
    } else if IMAGE_EXTS.contains(&ext) {
        MediaType::Image
    } else {
        MediaType::Unknown
    }
}

/// "30000/1001" -> 29.97; None for "0/0" or junk.
fn parse_rate(r: &str) -> Option<f64> {
    let (n, d) = r.split_once('/')?;
    let (n, d): (f64, f64) = (n.parse().ok()?, d.parse().ok()?);
    (n > 0.0 && d > 0.0).then(|| n / d)
}

/// "34000/50000" or "1000.5" -> a number.
fn parse_frac(s: &str) -> Option<f64> {
    match s.split_once('/') {
        Some((n, d)) => {
            let (n, d): (f64, f64) = (n.trim().parse().ok()?, d.trim().parse().ok()?);
            (d != 0.0).then(|| n / d)
        }
        None => s.trim().parse().ok(),
    }
}

/// pal8 (palette PNG/GIF) may carry transparency; a false positive only costs a premultiply pass.
fn pix_fmt_has_alpha(f: &str) -> bool {
    f == "pal8" || ["yuva", "gbrap", "ya8", "ya16"].iter().any(|p| f.starts_with(p)) || ["rgba", "bgra", "argb", "abgr"].iter().any(|p| f.contains(p))
}

/// R40a: HDR10 static metadata from a stream's or a frame's side data list, if it has either part.
fn mastering_from(side: &[serde_json::Value]) -> Option<Mastering> {
    let md = side.iter().find(|d| d["side_data_type"].as_str().is_some_and(|t| t.starts_with("Mastering display")));
    let cll = side.iter().find(|d| d["side_data_type"].as_str().is_some_and(|t| t.starts_with("Content light level")));
    if md.is_none() && cll.is_none() {
        return None;
    }
    let num = |v: &serde_json::Value| v.as_str().and_then(parse_frac).or_else(|| v.as_f64()).unwrap_or(0.0);
    Some(Mastering {
        min_luminance: md.map_or(0.0, |d| num(&d["min_luminance"])),
        max_luminance: md.map_or(0.0, |d| num(&d["max_luminance"])),
        max_cll: cll.map_or(0, |d| d["max_content"].as_u64().unwrap_or(0) as u32),
        max_fall: cll.map_or(0, |d| d["max_average"].as_u64().unwrap_or(0) as u32),
    })
}

/// R40a: the bit depth a pixel format name carries (yuv420p10le -> 10, gbrpf32le -> 32, rgb48le -> 16, pal8 -> 8).
fn pix_fmt_depth(f: &str) -> u8 {
    if let Some(k) = f.find('f') {
        if let Ok(b) = f[k + 1..].trim_end_matches(|c: char| !c.is_ascii_digit()).parse::<u8>() {
            return b;
        }
    }
    let digits: String = f.chars().rev().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).collect::<Vec<_>>().into_iter().rev().collect();
    match digits.parse::<u8>() {
        Ok(n) if f.contains('p') && (9..=16).contains(&n) && !f.starts_with("rgb") && !f.starts_with("bgr") => n,
        Ok(48) | Ok(64) => 16,
        Ok(n) if f.starts_with("gray") && (9..=16).contains(&n) => n,
        _ => 8,
    }
}

/// Fill `item` from ffprobe's JSON. `item.media_type` comes in from the extension; the streams decide it
/// for audio/video files (an `.mts` with a video stream is Video), images keep their extension type (R38: unless they
/// are animated).
fn parse_ffprobe(json: &str, item: &mut MediaItem) {
    let Ok(info) = serde_json::from_str::<serde_json::Value>(json) else {
        return;
    };

    if let Some(dur) = info["format"]["duration"].as_str() {
        item.duration = dur.parse().unwrap_or(0.0);
    }
    let format_name = info["format"]["format_name"].as_str().unwrap_or("");

    let streams = info["streams"].as_array().map(Vec::as_slice).unwrap_or_default();
    // Cover art in audio files is a video stream flagged attached_pic: not a picture track.
    let is_video = |s: &&serde_json::Value| s["codec_type"] == "video" && s["disposition"]["attached_pic"].as_i64() != Some(1);
    let video = streams.iter().find(is_video);
    let audio = streams.iter().find(|s| s["codec_type"] == "audio");
    // Stream ▸ lists the streams of each kind (`-map 0:v:N` / `0:a:N`); cover art is not a picture stream.
    item.video_streams = streams.iter().filter(is_video).count() as u32;
    item.audio_streams = streams.iter().filter(|s| s["codec_type"] == "audio").count() as u32;

    let tag = |v: &serde_json::Value, key: &str| v["tags"].as_object()?.iter().find(|(k, _)| k.eq_ignore_ascii_case(key))?.1.as_str().map(str::to_owned);
    let mut animated = false;
    if let Some(s) = video {
        let (w, h) = (s["width"].as_u64().map(|v| v as u32), s["height"].as_u64().map(|v| v as u32));
        // Anamorphic video (HDV, XDCAM HD, DV): the display width, in square pixels, which every decode scales to.
        let sar = s["sample_aspect_ratio"].as_str().and_then(|r| r.split_once(':')).and_then(|(n, d)| Some((n.parse::<f64>().ok()?, d.parse::<f64>().ok()?)));
        let w = match (w, sar) {
            (Some(w), Some((n, d))) if n > 0.0 && d > 0.0 => Some(((w as f64 * n / d / 2.0).round() as u32 * 2).max(2)),
            _ => w,
        };
        let rotation = s["side_data_list"]
            .as_array()
            .and_then(|l| l.iter().find_map(|d| d["rotation"].as_f64()))
            .or_else(|| s["tags"]["rotate"].as_str().and_then(|r| r.parse::<f64>().ok()).map(|r| -r)) // old tag, opposite sign
            .map_or(0, |r| (r.round() as i32).rem_euclid(360));
        let swap = rotation == 90 || rotation == 270;
        (item.width, item.height) = if swap { (h, w) } else { (w, h) };
        item.rotation = rotation;
        // r_frame_rate is the nominal rate (30, 30000/1001) of CFR and near-CFR phone footage whose avg drifts with
        // edit lists or dropped frames; avg_frame_rate is the real rate when r is a timebase (90000/1) or a field rate.
        let (r, avg) = (s["r_frame_rate"].as_str().and_then(parse_rate), s["avg_frame_rate"].as_str().and_then(parse_rate));
        item.frame_rate = match (r, avg) {
            (Some(r), Some(a)) if (r - a).abs() / a < 0.01 => Some(r),
            _ => avg.or(r),
        };
        item.codec = s["codec_name"].as_str().map(String::from);
        let pix_fmt = s["pix_fmt"].as_str().unwrap_or("");
        item.has_alpha = pix_fmt_has_alpha(pix_fmt);
        item.color_matrix = s["color_space"].as_str().filter(|c| *c != "unknown").map(String::from);
        // "unknown" is common on progressive files, so only real field orders count.
        item.interlaced = matches!(s["field_order"].as_str(), Some("tt" | "bb" | "tb" | "bt"));
        // R40a: transfer, primaries, bit depth and the HDR10 static metadata.
        let known = |k: &str| s[k].as_str().filter(|c| !matches!(*c, "unknown" | "unspecified" | "reserved")).map(String::from);
        (item.transfer, item.primaries) = (known("color_transfer"), known("color_primaries"));
        item.bit_depth = Some(s["bits_per_raw_sample"].as_str().and_then(|b| b.parse().ok()).filter(|&b| b > 0).unwrap_or_else(|| if pix_fmt.is_empty() { 8 } else { pix_fmt_depth(pix_fmt) }));
        item.mastering = mastering_from(s["side_data_list"].as_array().map(Vec::as_slice).unwrap_or_default());
        // R38: an animated GIF / WebP / APNG is a video (several frames, or a format that only holds animations).
        let frames = s["nb_frames"].as_str().and_then(|n| n.parse::<u64>().ok()).unwrap_or(0);
        animated = item.media_type == MediaType::Image && (frames > 1 || matches!(format_name, "webp_anim" | "apng") || (format_name == "gif" && item.duration > 0.0 && frames != 1));
        if animated && item.duration <= 0.0 && frames > 1 {
            item.duration = frames as f64 / item.frame_rate.filter(|f| *f > 0.0).unwrap_or(10.0);
        }
    }
    if let Some(s) = audio {
        item.sample_rate = s["sample_rate"].as_str().and_then(|sr| sr.parse().ok());
        if item.codec.is_none() {
            item.codec = s["codec_name"].as_str().map(String::from);
        }
    }
    // R17a: start timecode (the container's tag, a `tmcd` or video stream's) at the picture's rate (else the tmcd
    // track's), else a BWF time reference (samples since midnight); the recording's creation time.
    let tc = std::iter::once(&info["format"]).chain(streams).find_map(|v| Some((tag(v, "timecode")?, v)));
    item.timecode = tc
        .and_then(|(t, s)| crate::engine::sync::parse_timecode(&t, item.frame_rate.or_else(|| ["r_frame_rate", "avg_frame_rate"].iter().find_map(|k| s[k].as_str().and_then(parse_rate)))?))
        .or_else(|| Some(tag(&info["format"], "time_reference")?.parse::<u64>().ok()? as f64 / item.sample_rate? as f64));
    item.created = std::iter::once(&info["format"]).chain(streams).find_map(|v| crate::engine::sync::parse_date(&tag(v, "creation_time")?));
    // R41: camera make and model (QuickTime / MP4 tags; an MXF's company and product), the reel or tape name.
    let any = |keys: &[&str]| std::iter::once(&info["format"]).chain(streams).find_map(|v| keys.iter().find_map(|k| tag(v, k).filter(|s| !s.trim().is_empty())));
    // An MXF's identification names the program that wrote it: a camera's own files name the camera, FFmpeg's do not.
    let writer = any(&["company_name"]).filter(|c| !c.eq_ignore_ascii_case("FFmpeg"));
    let make = any(&["make", "com.apple.quicktime.make"]).or_else(|| writer.clone());
    let model = any(&["model", "com.apple.quicktime.model"]).or_else(|| writer.and_then(|_| any(&["product_name"])));
    item.camera = match (make, model) {
        (Some(a), Some(b)) if !b.to_lowercase().starts_with(&a.to_lowercase()) => Some(format!("{a} {b}")),
        (a, b) => b.or(a),
    };
    item.reel = any(&["reel_name", "material_package_name", "tape_name", "com.apple.quicktime.reel"]);
    // R18a: chapters (MP4, MKV, an ffmetadata file's) are the media markers View › Event Media Markers shows.
    item.markers = info["chapters"]
        .as_array()
        .map(|cs| cs.iter().filter_map(|c| Some((c["start_time"].as_str()?.parse::<f64>().ok()?, tag(c, "title").unwrap_or_default()))).collect())
        .unwrap_or_default();
    if item.media_type != MediaType::Image || animated {
        if video.is_some() {
            item.media_type = MediaType::Video;
        } else if audio.is_some() {
            item.media_type = MediaType::Audio;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unreadable_media_is_an_error() {
        // Err whether ffprobe is missing or fails on the file, never a 0-length Ok item.
        assert!(super::probe_media(std::path::Path::new("no-such-file.mp4")).is_err());
    }

    #[test]
    fn merge_new_skips_known_and_repeated_paths() {
        let item = |p: &str| MediaItem { path: p.into(), ..Default::default() };
        let mut pool = vec![item("a.mp4")];
        assert!(!merge_new(&mut pool, vec![item("a.mp4")]));
        assert!(merge_new(&mut pool, vec![item("b.wav"), item("a.mp4"), item("b.wav")]));
        assert_eq!(pool.iter().map(|m| m.path.to_string_lossy().into_owned()).collect::<Vec<_>>(), ["a.mp4", "b.wav"]);
        assert!(!merge_new(&mut pool, Vec::new()));
        // R38: the sequence starting at a still is another item than the still.
        let seq = MediaItem { sequence: Some(Sequence { pattern: "a_%04d.png".into(), start: 1, count: 2, fps: 24.0 }), ..item("a_0001.png") };
        assert!(merge_new(&mut pool, vec![item("a_0001.png"), seq.clone()]));
        assert!(!merge_new(&mut pool, vec![seq]));
        assert_eq!(pool.len(), 4);
    }

    fn parse(ext: &str, json: &str) -> MediaItem {
        let mut m = MediaItem { media_type: classify_extension(ext), ..Default::default() };
        parse_ffprobe(json, &mut m);
        m
    }

    #[test]
    fn cover_art_is_not_video() {
        let m = parse("mp3", r#"{"format":{"duration":"12.5"},"streams":[
            {"codec_type":"audio","codec_name":"mp3","sample_rate":"44100"},
            {"codec_type":"video","codec_name":"mjpeg","width":600,"height":600,"disposition":{"attached_pic":1}}]}"#);
        assert_eq!((m.media_type, m.width, m.codec.as_deref(), m.sample_rate), (MediaType::Audio, None, Some("mp3"), Some(44100)));
        assert_eq!(m.duration, 12.5);
    }

    #[test]
    fn anamorphic_sources_get_their_display_width() {
        let size = |w: u32, h: u32, sar: &str| {
            let m = parse("m2t", &format!(r#"{{"streams":[{{"codec_type":"video","width":{w},"height":{h},"sample_aspect_ratio":"{sar}"}}]}}"#));
            (m.width.unwrap(), m.height.unwrap())
        };
        assert_eq!(size(1440, 1080, "4:3"), (1920, 1080)); // HDV, XDCAM HD
        assert_eq!(size(720, 480, "40:33"), (872, 480)); // widescreen DV
        assert_eq!(size(720, 480, "10:11"), (654, 480)); // 4:3 DV
        assert_eq!(size(1920, 1080, "1:1"), (1920, 1080));
        assert_eq!(size(1920, 1080, "0:1"), (1920, 1080)); // unknown
        // Rotated: the display width is still the coded width, before the swap.
        let m = parse("mp4", r#"{"streams":[{"codec_type":"video","width":1440,"height":1080,"sample_aspect_ratio":"4:3",
            "side_data_list":[{"rotation":-90}]}]}"#);
        assert_eq!((m.width, m.height), (Some(1080), Some(1920)));
    }

    #[test]
    fn rotation_swaps_size() {
        let m = parse("mp4", r#"{"streams":[{"codec_type":"video","width":1920,"height":1080,
            "side_data_list":[{"side_data_type":"Display Matrix","rotation":-90}]}]}"#);
        assert_eq!((m.width, m.height, m.rotation), (Some(1080), Some(1920), 270));
        let m = parse("mp4", r#"{"streams":[{"codec_type":"video","width":1920,"height":1080,"tags":{"rotate":"180"}}]}"#);
        assert_eq!((m.width, m.height, m.rotation), (Some(1920), Some(1080), 180));
    }

    #[test]
    fn alpha_interlace_matrix() {
        let m = parse("mov", r#"{"streams":[{"codec_type":"video","width":720,"height":576,"pix_fmt":"yuva420p",
            "field_order":"tt","color_space":"bt470bg"}]}"#);
        assert!(m.has_alpha && m.interlaced);
        assert_eq!(m.color_matrix.as_deref(), Some("bt470bg"));
        let m = parse("mp4", r#"{"streams":[{"codec_type":"video","pix_fmt":"yuv420p","field_order":"unknown","color_space":"unknown"}]}"#);
        assert!(!m.has_alpha && !m.interlaced && m.color_matrix.is_none());
        assert!(pix_fmt_has_alpha("rgba64le") && pix_fmt_has_alpha("gbrap10le") && pix_fmt_has_alpha("pal8") && !pix_fmt_has_alpha("rgb24"));
    }

    #[test]
    fn vfr_uses_avg_frame_rate() {
        let m = parse("mp4", r#"{"streams":[{"codec_type":"video","r_frame_rate":"90000/1","avg_frame_rate":"30000/1001"}]}"#);
        assert!((m.frame_rate.unwrap() - 29.97).abs() < 0.001);
        let m = parse("mp4", r#"{"streams":[{"codec_type":"video","r_frame_rate":"25/1","avg_frame_rate":"0/0"}]}"#);
        assert_eq!(m.frame_rate, Some(25.0));
        // iPhone-style CFR: avg drifts to 29.89, the nominal 30 is kept.
        let m = parse("mov", r#"{"streams":[{"codec_type":"video","r_frame_rate":"30/1","avg_frame_rate":"8100/271"}]}"#);
        assert_eq!(m.frame_rate, Some(30.0));
        // AVCHD: r is the 59.94 field rate, avg the real 29.97.
        let m = parse("mts", r#"{"streams":[{"codec_type":"video","r_frame_rate":"60000/1001","avg_frame_rate":"30000/1001"}]}"#);
        assert!((m.frame_rate.unwrap() - 29.97).abs() < 0.001);
    }

    #[test]
    fn streams_classify_before_extension() {
        let m = parse("mts", r#"{"streams":[{"codec_type":"video","codec_name":"h264"},{"codec_type":"audio","sample_rate":"48000"}]}"#);
        assert_eq!(m.media_type, MediaType::Video);
        let m = parse("mp4", r#"{"streams":[{"codec_type":"audio","sample_rate":"48000"}]}"#);
        assert_eq!(m.media_type, MediaType::Audio);
        let m = parse("png", r#"{"streams":[{"codec_type":"video","codec_name":"png","width":64,"height":64}]}"#);
        assert_eq!(m.media_type, MediaType::Image);
        assert_eq!(parse("xyz", "{}").media_type, MediaType::Unknown);
        // R6a: a project imports (and drops) as a nested timeline; `probe_media` reads it as one.
        assert_eq!(classify_extension("mnle"), MediaType::Video);
        assert!(super::probe_media(std::path::Path::new("no-such-project.mnle")).is_err());
        // R38/R41: the new extensions.
        assert_eq!((classify_extension("dpx"), classify_extension("hdr"), classify_extension("ffconcat"), classify_extension("braw")), (MediaType::Image, MediaType::Image, MediaType::Video, MediaType::Video));
    }

    /// R38: a GIF with several frames (or an animated WebP, whose length ffprobe does not give) is a video; a one-frame
    /// GIF or a still WebP stays a picture.
    #[test]
    fn animated_pictures_are_video() {
        let m = parse("gif", r#"{"format":{"format_name":"gif","duration":"1.000000"},"streams":[{"codec_type":"video","codec_name":"gif","width":32,"height":32,"nb_frames":"10","r_frame_rate":"10/1","avg_frame_rate":"10/1"}]}"#);
        assert_eq!((m.media_type, m.duration, m.frame_rate), (MediaType::Video, 1.0, Some(10.0)));
        let m = parse("gif", r#"{"format":{"format_name":"gif","duration":"0.100000"},"streams":[{"codec_type":"video","codec_name":"gif","width":32,"height":32,"nb_frames":"1","r_frame_rate":"10/1"}]}"#);
        assert_eq!(m.media_type, MediaType::Image);
        let m = parse("webp", r#"{"format":{"format_name":"webp_anim"},"streams":[{"codec_type":"video","codec_name":"webp_anim","width":32,"height":32,"r_frame_rate":"10/1"}]}"#);
        assert_eq!((m.media_type, m.duration), (MediaType::Video, 0.0), "the length is counted afterwards");
        let m = parse("webp", r#"{"format":{"format_name":"webp_pipe"},"streams":[{"codec_type":"video","codec_name":"webp","width":32,"height":32,"r_frame_rate":"25/1"}]}"#);
        assert_eq!(m.media_type, MediaType::Image);
        let m = parse("png", r#"{"format":{"format_name":"apng","duration":"2.0"},"streams":[{"codec_type":"video","codec_name":"apng","nb_frames":"20","r_frame_rate":"10/1"}]}"#);
        assert_eq!((m.media_type, m.duration), (MediaType::Video, 2.0));
    }

    /// R40a: transfer, primaries, bit depth and the HDR10 metadata are read; `hdr()` classes the source.
    #[test]
    fn hdr_tags_are_read() {
        let m = parse("mp4", r#"{"streams":[{"codec_type":"video","codec_name":"hevc","width":3840,"height":2160,"pix_fmt":"yuv420p10le",
            "color_space":"bt2020nc","color_transfer":"smpte2084","color_primaries":"bt2020","bits_per_raw_sample":"10",
            "side_data_list":[{"side_data_type":"Mastering display metadata","red_x":"34000/50000","min_luminance":"50/10000","max_luminance":"10000000/10000"},
                              {"side_data_type":"Content light level metadata","max_content":1000,"max_average":400}]}]}"#);
        assert_eq!((m.transfer.as_deref(), m.primaries.as_deref(), m.bit_depth, m.color_matrix.as_deref()), (Some("smpte2084"), Some("bt2020"), Some(10), Some("bt2020nc")));
        assert_eq!(m.mastering, Some(Mastering { min_luminance: 0.005, max_luminance: 1000.0, max_cll: 1000, max_fall: 400 }));
        assert_eq!(m.hdr(), Some(crate::project::Hdr::Pq));
        let m = parse("mov", r#"{"streams":[{"codec_type":"video","codec_name":"hevc","pix_fmt":"yuv420p10le","color_transfer":"arib-std-b67","color_primaries":"bt2020","color_space":"bt2020nc"}]}"#);
        assert_eq!((m.hdr(), m.bit_depth, m.mastering), (Some(crate::project::Hdr::Hlg), Some(10), None));
        let m = parse("mp4", r#"{"streams":[{"codec_type":"video","codec_name":"h264","pix_fmt":"yuv420p","color_transfer":"unknown","color_primaries":"unknown"}]}"#);
        assert_eq!((m.hdr(), m.transfer, m.primaries, m.bit_depth), (None, None, None, Some(8)));
        let m = parse("exr", r#"{"streams":[{"codec_type":"video","codec_name":"exr","pix_fmt":"gbrpf32le","color_transfer":"linear"}]}"#);
        assert_eq!((m.hdr(), m.bit_depth, m.transfer.as_deref()), (None, Some(32), Some("linear")));
        assert_eq!((pix_fmt_depth("yuv422p10le"), pix_fmt_depth("yuv420p"), pix_fmt_depth("rgb48le"), pix_fmt_depth("gbrapf32le"), pix_fmt_depth("yuva444p12le"), pix_fmt_depth("pal8"), pix_fmt_depth("rgb24")), (10, 8, 16, 32, 12, 8, 8));
    }

    /// R41: make/model (QuickTime, MXF company/product) and the reel; a SMPTE 12-1 timecode in the first frame's side
    /// data; a spanned clip's name.
    #[test]
    fn camera_tags_and_sei_timecode() {
        let m = parse("mp4", r#"{"format":{"tags":{"com.apple.quicktime.make":"Sony","com.apple.quicktime.model":"ILCE-7M4"}},"streams":[{"codec_type":"video","codec_name":"hevc","r_frame_rate":"25/1"}]}"#);
        assert_eq!((m.camera.as_deref(), m.reel), (Some("Sony ILCE-7M4"), None));
        let m = parse("mxf", r#"{"format":{"tags":{"company_name":"Sony","product_name":"Sony PMW-F55","material_package_name":"A001C003","timecode":"01:00:00:00"}},"streams":[{"codec_type":"video","codec_name":"h264","r_frame_rate":"25/1"}]}"#);
        assert_eq!((m.camera.as_deref(), m.reel.as_deref(), m.timecode), (Some("Sony PMW-F55"), Some("A001C003"), Some(3600.0)));
        let m = parse("mov", r#"{"format":{"tags":{"make":"Canon"}},"streams":[{"codec_type":"video","codec_name":"prores","tags":{"reel_name":"R07"}}]}"#);
        assert_eq!((m.camera.as_deref(), m.reel.as_deref()), (Some("Canon"), Some("R07")));
        let frames: serde_json::Value = serde_json::from_str(r#"{"frames":[{"side_data_list":[{"side_data_type":"H.26[45] User Data Unregistered SEI message"},{"side_data_type":"SMPTE 12-1 timecode","timecodes":[{"value":"01:02:03:04"}]}]}]}"#).unwrap();
        assert_eq!(parse_sei_timecode(&frames, 25.0), Some(3600.0 + 123.0 + 4.0 / 25.0));
        assert_eq!(parse_sei_timecode(&serde_json::json!({"frames":[{"side_data_list":[]}]}), 25.0), None);
    }

    /// R38: a numbered run is found from any of its files, stops at a gap and needs the same name, width and extension;
    /// a lone still is none.
    #[test]
    fn numbered_sequences_are_detected() {
        let dir = std::env::temp_dir().join(format!("milan-seq-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for n in (1..=12).chain(20..=22) {
            std::fs::write(dir.join(format!("shot_{n:04}.png")), b"x").unwrap();
        }
        std::fs::write(dir.join("shot_0013.jpg"), b"x").unwrap(); // another extension
        std::fs::write(dir.join("shot_013.png"), b"x").unwrap(); // another digit width
        std::fs::write(dir.join("other_0001.png"), b"x").unwrap();
        std::fs::write(dir.join("lone.png"), b"x").unwrap();
        let pattern = PathBuf::from("shot_%04d.png");
        assert_eq!(detect_sequence(&dir.join("shot_0001.png")), Some((pattern.clone(), 1, 12)));
        assert_eq!(detect_sequence(&dir.join("shot_0007.png")), Some((pattern.clone(), 1, 12)), "picked mid-run: the run from its start");
        assert_eq!(detect_sequence(&dir.join("shot_0021.png")), Some((pattern.clone(), 20, 3)), "after the gap, a run of its own");
        assert_eq!(detect_sequence(&dir.join("other_0001.png")), None, "no sibling");
        assert_eq!(detect_sequence(&dir.join("lone.png")), None);
        assert_eq!(detect_sequence(&dir.join("shot_0013.jpg")), None);
        let picked = [dir.join("shot_0003.png"), dir.join("shot_0001.png"), dir.join("lone.png"), dir.join("shot_0020.png")];
        let seqs = sequences_in(&picked);
        assert_eq!(seqs.len(), 2);
        assert_eq!((seqs[0].0.clone(), seqs[0].1.clone(), seqs[0].2.len()), (dir.join("shot_0003.png"), (pattern.clone(), 1, 12), 2));
        assert_eq!((seqs[1].1.1, seqs[1].1.2), (20, 3));
        let s = Sequence { pattern, start: 1, count: 12, fps: 25.0 };
        assert_eq!(s.file_name(7), "shot_0007.png");
        assert_eq!(s.input_path(&dir.join("shot_0001.png")), dir.join("shot_%04d.png"));
        assert_eq!(s.files(&dir)[11], dir.join("shot_0012.png"));
        // A `%` in the name or the folder is doubled for image2, and a `d` before the number is no placeholder.
        std::fs::write(dir.join("dd 100%_01.png"), b"x").unwrap();
        std::fs::write(dir.join("dd 100%_02.png"), b"x").unwrap();
        let (pat, start, count) = detect_sequence(&dir.join("dd 100%_02.png")).unwrap();
        assert_eq!((pat.to_string_lossy().as_ref(), start, count), ("dd 100%%_%02d.png", 1, 2));
        let s = Sequence { pattern: pat, start, count, fps: 25.0 };
        assert_eq!((s.file_name(2), s.input_path(std::path::Path::new(r"C:\a 5%\x.png"))), ("dd 100%_02.png".to_string(), PathBuf::from(r"C:\a 5%%\dd 100%%_%02d.png")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// R37: a codec the table has no decoder for gives an offline item with the reason; RAW camera files likewise.
    #[test]
    fn unsupported_codec_is_an_offline_item() {
        let caps = crate::engine::caps::tests::full();
        let mut m = parse("avi", r#"{"format":{"duration":"3.0"},"streams":[{"codec_type":"video","codec_name":"cinepak","width":320,"height":240}]}"#);
        m.unsupported = crate::engine::caps::unsupported(&caps, m.codec.as_deref().unwrap());
        assert!(m.is_offline() && m.width == Some(320), "{:?}", m.unsupported);
        assert_eq!(m.unsupported.as_deref(), Some("unsupported codec cinepak: this FFmpeg build has no decoder for it"));
        let mut ok = parse("mp4", r#"{"streams":[{"codec_type":"video","codec_name":"h264","width":320,"height":240}]}"#);
        ok.unsupported = crate::engine::caps::unsupported(&caps, ok.codec.as_deref().unwrap());
        assert!(!ok.is_offline());
        // A RAW file that exists is offline with its reason; a missing one is an error.
        let dir = std::env::temp_dir().join(format!("milan-raw-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let raw = dir.join("A001_C001.braw");
        std::fs::write(&raw, b"not really").unwrap();
        let m = probe_media(&raw).unwrap();
        assert!(m.is_offline() && m.unsupported.as_deref().unwrap().starts_with("unsupported format Blackmagic RAW"));
        assert!(probe_media(&dir.join("missing.r3d")).is_err());
        assert!(probe_media(&dir).unwrap_err().to_string().contains("Camera Card Folder"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// R17a: a start timecode from the container, a tmcd track (at the picture's rate) or a BWF time reference; the
    /// creation time; none on a file without them.
    #[test]
    fn timecode_and_creation_time_are_read() {
        let m = parse("mov", r#"{"format":{"tags":{"creation_time":"2024-05-01T12:34:56.000000Z"}},"streams":[
            {"codec_type":"video","r_frame_rate":"30000/1001","avg_frame_rate":"30000/1001"},
            {"codec_type":"data","codec_tag_string":"tmcd","tags":{"timecode":"01:00:00;00"}}]}"#);
        assert!((m.timecode.unwrap() - 107_892.0 * 1001.0 / 30_000.0).abs() < 1e-9 && m.created == Some(1_714_566_896.0));
        let m = parse("mxf", r#"{"format":{"tags":{"TIMECODE":"10:00:00:00"}},"streams":[{"codec_type":"video","r_frame_rate":"25/1"}]}"#);
        assert_eq!(m.timecode, Some(36_000.0));
        let m = parse("wav", r#"{"format":{"tags":{"time_reference":"172800000"}},"streams":[{"codec_type":"audio","sample_rate":"48000"}]}"#);
        assert_eq!((m.timecode, m.created), (Some(3600.0), None));
        let m = parse("mp4", r#"{"streams":[{"codec_type":"video","r_frame_rate":"25/1"}]}"#);
        assert_eq!((m.timecode, m.created), (None, None));
    }

    /// Needs ffmpeg. `cargo test --release -- --ignored ffmpeg_reads_media_timecode`
    /// Real files with timecode metadata: a QuickTime with a drop-frame tmcd track, an MXF and a BWF WAV.
    #[test]
    #[ignore]
    fn ffmpeg_reads_media_timecode() {
        let dir = std::env::temp_dir().join(format!("milan-timecode-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let make = |name: &str, args: &[&str]| {
            let path = dir.join(name);
            assert!(crate::proc::ffmpeg().args(["-v", "error", "-y"]).args(args).arg(&path).status().unwrap().success(), "{name}");
            super::probe_media(&path).unwrap()
        };
        let v = ["-f", "lavfi", "-i", "testsrc=s=64x36:r=30000/1001:d=1"];
        let mov = make("df.mov", &[&v[..], &["-c:v", "mpeg4", "-timecode", "01:02:03;04", "-metadata", "creation_time=2024-05-01T12:34:56Z"]].concat());
        let frames = ((62 * 60 + 3) * 30 + 4 - 2 * (62 - 6)) as f64; // 2 numbers dropped a minute, not at 0/10/20...
        assert!((mov.timecode.unwrap() - frames * 1001.0 / 30_000.0).abs() < 1e-9, "{:?}", mov.timecode);
        assert_eq!(mov.created, Some(1_714_566_896.0));
        let mxf = make("ndf.mxf", &["-f", "lavfi", "-i", "testsrc=s=64x36:r=25:d=1", "-c:v", "mpeg2video", "-timecode", "10:00:00:12"]);
        assert_eq!(mxf.timecode, Some(36_000.48));
        let wav = make("bwf.wav", &["-f", "lavfi", "-i", "anullsrc=r=48000:cl=mono", "-t", "1", "-write_bext", "1", "-metadata", "time_reference=172824000"]);
        assert_eq!(wav.timecode, Some(3600.5));
        let plain = make("plain.wav", &["-f", "lavfi", "-i", "anullsrc=r=48000:cl=mono", "-t", "1"]);
        assert_eq!(plain.timecode, None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// R18a: chapters become media markers (start and title); a file without them has none.
    #[test]
    fn chapters_are_media_markers() {
        let m = parse("mp4", r#"{"streams":[{"codec_type":"audio","sample_rate":"48000"}],"chapters":[
            {"id":0,"start_time":"0.000000","end_time":"1.500000","tags":{"title":"Intro"}},
            {"id":1,"start_time":"1.500000","end_time":"4.000000"}]}"#);
        assert_eq!(m.markers, [(0.0, "Intro".to_string()), (1.5, String::new())]);
        assert!(parse("wav", r#"{"streams":[]}"#).markers.is_empty());
    }

    /// Needs ffmpeg. `cargo test --release -- --ignored ffmpeg_reads_media_markers`
    /// R18a acceptance: media markers parsed from an ffmetadata test file (its chapters muxed into a Matroska file).
    #[test]
    #[ignore]
    fn ffmpeg_reads_media_markers() {
        let dir = std::env::temp_dir().join(format!("milan-markers-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let meta = dir.join("chapters.txt");
        std::fs::write(&meta, ";FFMETADATA1
[CHAPTER]
TIMEBASE=1/1000
START=0
END=1250
title=Verse
[CHAPTER]
TIMEBASE=1/1000
START=1250
END=3000
title=Chorus
").unwrap();
        let out = dir.join("marked.mka");
        let ok = crate::proc::ffmpeg()
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", "anullsrc=r=48000:cl=mono", "-i"])
            .arg(&meta)
            .args(["-map", "0:a", "-map_chapters", "1", "-t", "3", "-c:a", "flac"])
            .arg(&out)
            .status()
            .unwrap()
            .success();
        assert!(ok);
        let m = super::probe_media(&out).unwrap();
        std::fs::remove_file(&out).unwrap();
        std::fs::remove_file(&meta).unwrap();
        std::fs::remove_dir(&dir).unwrap();
        assert_eq!(m.markers, [(0.0, "Verse".to_string()), (1.25, "Chorus".to_string())]);
    }

    /// R3b: Stream ▸ offers each audio and picture stream; cover art is not one.
    #[test]
    fn streams_are_counted_by_kind() {
        let m = parse("mov", r#"{"streams":[{"codec_type":"video"},{"codec_type":"audio"},{"codec_type":"audio"},{"codec_type":"data"},
            {"codec_type":"video","disposition":{"attached_pic":1}},{"codec_type":"audio"}]}"#);
        assert_eq!((m.video_streams, m.audio_streams, m.streams(true), m.streams(false)), (1, 3, 1, 3));
        assert_eq!(MediaItem::default().streams(false), 1, "an item probed before R3b has one");
    }
}
