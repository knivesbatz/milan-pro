use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use crate::project::{MediaItem, MediaType};

pub fn probe_media(path: &Path) -> anyhow::Result<MediaItem> {
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

    let media_type = classify_extension(&ext);

    let mut item = MediaItem { id: uuid::Uuid::new_v4().to_string(), path: path.to_path_buf(), name, media_type, ..Default::default() };

    let mut cmd = crate::proc::ffprobe();
    cmd.args(["-v", "error", "-print_format", "json", "-show_format", "-show_streams"]).arg(path);
    let output = crate::proc::output(&mut cmd).map_err(|e| anyhow::anyhow!(crate::proc::tool_error(&e, "ffprobe")))?;
    if !output.status.success() && item.media_type != MediaType::Image {
        let err = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("unreadable media: {}", err.lines().next().unwrap_or("unknown error").trim());
    }
    parse_ffprobe(&String::from_utf8_lossy(&output.stdout), &mut item);

    if item.media_type == MediaType::Image && item.duration == 0.0 {
        item.duration = 5.0;
    }
    // A zero-length clip would be invisible on the timeline.
    if item.duration <= 0.0 {
        anyhow::bail!("could not read duration");
    }

    Ok(item)
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
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let batch = paths
                .into_iter()
                .map(|p| {
                    let r = probe_media(&p).map_err(|e| format!("{}: {e}", p.file_name().unwrap_or_default().to_string_lossy()));
                    (p, r)
                })
                .collect();
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
pub fn merge_new(pool: &mut Vec<MediaItem>, items: Vec<MediaItem>) -> bool {
    let mut done = false;
    for m in items {
        match pool.iter_mut().find(|p| p.path == m.path && p.subclip.is_none()) {
            None => pool.push(m),
            Some(p) if p.is_offline() && !m.is_offline() => *p = MediaItem { id: std::mem::take(&mut p.id), ..m },
            Some(_) => continue,
        }
        done = true;
    }
    done
}

/// `path` has a usable (probed) item in the pool; an offline one is probed again on import or drop.
pub fn in_pool(pool: &[MediaItem], path: &std::path::Path) -> bool {
    pool.iter().any(|m| m.path == path && m.subclip.is_none() && !m.is_offline())
}

/// File extensions Milan Pro imports, by kind (lower case): the Import dialog, drops and the Explorer use these.
pub const VIDEO_EXTS: &[&str] = &["mp4", "mov", "avi", "mkv", "wmv", "flv", "webm", "mxf", "mpg", "mpeg", "ts", "m2ts", "mts", "m2t", "vob", "3gp", "m4v"];
pub const AUDIO_EXTS: &[&str] = &["mp3", "wav", "flac", "aac", "ogg", "wma", "aiff", "opus", "m4a", "ac3", "dts", "pcm"];
pub const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "tiff", "tif", "bmp", "webp", "gif", "exr", "tga", "psd"];

pub(crate) fn classify_extension(ext: &str) -> MediaType {
    if VIDEO_EXTS.contains(&ext) {
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

/// pal8 (palette PNG/GIF) may carry transparency; a false positive only costs a premultiply pass.
fn pix_fmt_has_alpha(f: &str) -> bool {
    f == "pal8" || ["yuva", "gbrap", "ya8", "ya16"].iter().any(|p| f.starts_with(p)) || ["rgba", "bgra", "argb", "abgr"].iter().any(|p| f.contains(p))
}

/// Fill `item` from ffprobe's JSON. `item.media_type` comes in from the extension; the streams decide it
/// for audio/video files (an `.mts` with a video stream is Video), images keep their extension type.
fn parse_ffprobe(json: &str, item: &mut MediaItem) {
    let Ok(info) = serde_json::from_str::<serde_json::Value>(json) else {
        return;
    };

    if let Some(dur) = info["format"]["duration"].as_str() {
        item.duration = dur.parse().unwrap_or(0.0);
    }

    let streams = info["streams"].as_array().map(Vec::as_slice).unwrap_or_default();
    // Cover art in audio files is a video stream flagged attached_pic: not a picture track.
    let video = streams.iter().find(|s| s["codec_type"] == "video" && s["disposition"]["attached_pic"].as_i64() != Some(1));
    let audio = streams.iter().find(|s| s["codec_type"] == "audio");

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
        item.has_alpha = s["pix_fmt"].as_str().is_some_and(pix_fmt_has_alpha);
        item.color_matrix = s["color_space"].as_str().filter(|c| *c != "unknown").map(String::from);
        // "unknown" is common on progressive files, so only real field orders count.
        item.interlaced = matches!(s["field_order"].as_str(), Some("tt" | "bb" | "tb" | "bt"));
    }
    if let Some(s) = audio {
        item.sample_rate = s["sample_rate"].as_str().and_then(|sr| sr.parse().ok());
        if item.codec.is_none() {
            item.codec = s["codec_name"].as_str().map(String::from);
        }
    }
    if item.media_type != MediaType::Image {
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
    }
}
