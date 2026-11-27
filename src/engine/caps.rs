//! R37: the format capability table. What the bundled FFmpeg can demux, decode, encode and mux (`ffmpeg -demuxers
//! -decoders -encoders -muxers -pix_fmts -filters`, asked once and cached per FFmpeg version in
//! `%LOCALAPPDATA%\MilanPro\caps.json`), and Milan Pro's own rules on top: what imports, what gets a proxy, what the
//! render templates can write. An import whose codec has no decoder becomes an offline item that says so
//! (`media::probe_media`), never a picture decoded as something else. Media Properties shows the verdict
//! (`verdict`), and the ignored test `format_corpus_e2e` generates the legal test corpus with lavfi, decodes every file
//! through the engine and rewrites the decode half of docs/FORMAT_SUPPORT.md.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex, OnceLock};
use crate::project::{MediaItem, MediaType};

/// What the FFmpeg build lists.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Caps {
    /// The first line of `ffmpeg -version` (the cache key).
    pub ffmpeg: String,
    /// Codec ids a decoder exists for (`h264`, `prores`, `aac`...): the decoder names and their `(codec x)` ids.
    pub decoders: BTreeSet<String>,
    /// Encoder names (`libx264`, `prores_ks`, `aac`...) and the codec ids they produce.
    pub encoders: BTreeSet<String>,
    pub demuxers: BTreeSet<String>,
    pub muxers: BTreeSet<String>,
    pub pix_fmts: BTreeSet<String>,
    pub filters: BTreeSet<String>,
}

impl Caps {
    /// A stream of `codec` (ffprobe's `codec_name`) decodes.
    pub fn can_decode(&self, codec: &str) -> bool {
        self.decoders.contains(codec)
    }

    /// `enc` (an encoder name, or a codec id one of them produces) is built in. `pcm` is the render templates' word for
    /// the PCM encoders, picked by bit depth.
    pub fn can_encode(&self, enc: &str) -> bool {
        match enc {
            "pcm" => self.encoders.contains("pcm_s16le"),
            _ => self.encoders.contains(enc),
        }
    }

    pub fn has_filter(&self, name: &str) -> bool {
        self.filters.contains(name)
    }

    pub fn has_demuxer(&self, name: &str) -> bool {
        self.demuxers.contains(name)
    }

    pub fn has_muxer(&self, name: &str) -> bool {
        self.muxers.contains(name)
    }

    /// The view transform (R40a) and linear stills need zimg's zscale and the tonemap filter.
    pub fn hdr_ok(&self) -> bool {
        self.has_filter("zscale") && self.has_filter("tonemap")
    }
}

// ---- Parsers ----

/// `ffmpeg -decoders` / `-encoders`: ` VFS..D h264   H.264 ...` and ` V....D libdav1d  dav1d ... (codec av1)`; the
/// names and the codec ids in the `(codec x)` suffixes. Experimental ones (`X` flag) count too: the user may allow them.
pub fn parse_codecs(out: &str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for l in out.lines().skip_while(|l| !l.trim_start().starts_with("------")).skip(1) {
        let mut it = l.split_whitespace();
        let (Some(flags), Some(name)) = (it.next(), it.next()) else { continue };
        if flags.len() != 6 || !flags.starts_with(['V', 'A', 'S']) {
            continue;
        }
        set.insert(name.to_string());
        if let Some(c) = l.rsplit_once("(codec ").and_then(|(_, c)| c.strip_suffix(')')) {
            set.insert(c.trim().to_string());
        }
    }
    set
}

/// `ffmpeg -demuxers` / `-muxers`: ` D   mov,mp4,m4a,3gp,3g2,mj2 QuickTime / MOV`; every comma-separated name.
pub fn parse_formats(out: &str) -> BTreeSet<String> {
    out.lines()
        .skip_while(|l| !l.trim_start().starts_with("---"))
        .skip(1)
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let flags = it.next()?;
            (flags.chars().all(|c| "DEd".contains(c))).then(|| it.next())?
        })
        .flat_map(|names| names.split(',').map(String::from).collect::<Vec<_>>())
        .collect()
}

/// `ffmpeg -pix_fmts`: `IO... yuv420p  3  12  8-8-8`.
pub fn parse_pix_fmts(out: &str) -> BTreeSet<String> {
    out.lines()
        .skip_while(|l| !l.starts_with("-----"))
        .skip(1)
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let flags = it.next()?;
            (flags.len() == 5).then(|| it.next())?.map(String::from)
        })
        .collect()
}

/// `ffmpeg -filters`: ` TS colorspace   V->V   Convert...` (two flag characters; three in some builds).
pub fn parse_filters(out: &str) -> BTreeSet<String> {
    out.lines()
        .skip_while(|l| !l.trim_start().starts_with("------"))
        .skip(1)
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let flags = it.next()?;
            (matches!(flags.len(), 2 | 3) && flags.chars().all(|c| "TSC.".contains(c))).then(|| it.next())?.map(String::from)
        })
        .collect()
}

// ---- The table ----

static WRITE: AtomicBool = AtomicBool::new(false);
static TABLE: OnceLock<Mutex<Option<Arc<Caps>>>> = OnceLock::new();

/// The app writes the cache; the screenshot harness and the tests never touch the user's AppData.
pub fn set_write(on: bool) {
    WRITE.store(on, Relaxed);
}

fn cache_path() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("MilanPro").join("caps.json"))
}

fn list(flag: &str) -> String {
    let mut cmd = crate::proc::ffmpeg();
    cmd.args(["-hide_banner", flag]);
    crate::proc::output(&mut cmd).map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
}

fn ffmpeg_version() -> String {
    let mut cmd = crate::proc::ffmpeg();
    cmd.arg("-version");
    let out = crate::proc::output(&mut cmd).ok();
    out.and_then(|o| String::from_utf8_lossy(&o.stdout).lines().next().map(|l| l.split(" Copyright").next().unwrap_or(l).trim().to_string())).unwrap_or_default()
}

/// Ask the bundled FFmpeg for its lists (six short runs).
pub fn probe() -> Caps {
    Caps {
        ffmpeg: ffmpeg_version(),
        decoders: parse_codecs(&list("-decoders")),
        encoders: parse_codecs(&list("-encoders")),
        demuxers: parse_formats(&list("-demuxers")),
        muxers: parse_formats(&list("-muxers")),
        pix_fmts: parse_pix_fmts(&list("-pix_fmts")),
        filters: parse_filters(&list("-filters")),
    }
}

/// The table: the cached one when it came from this FFmpeg, else probed now (a few hundred milliseconds, on the
/// importer's worker thread the first time). An FFmpeg that is missing gives an empty table, which decodes nothing:
/// `media::probe_media` has failed before that already.
pub fn get() -> Arc<Caps> {
    let slot = TABLE.get_or_init(|| Mutex::new(None));
    let mut s = slot.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(t) = &*s {
        return t.clone();
    }
    let path = cache_path();
    let cached: Option<Caps> = path.as_deref().and_then(|p| serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok());
    let version = ffmpeg_version();
    let caps = match cached {
        Some(c) if !c.ffmpeg.is_empty() && c.ffmpeg == version => c,
        _ => {
            let c = probe();
            if WRITE.load(Relaxed) && !c.ffmpeg.is_empty() {
                if let Some(p) = &path {
                    let _ = std::fs::create_dir_all(p.parent().unwrap_or(Path::new(".")));
                    let _ = serde_json::to_string_pretty(&c).map(|s| std::fs::write(p, s));
                }
            }
            c
        }
    };
    let t = Arc::new(caps);
    *s = Some(t.clone());
    t
}

/// The table if it has been built (the UI never waits for it).
pub fn try_get() -> Option<Arc<Caps>> {
    TABLE.get()?.lock().ok()?.clone()
}

/// Tests: a table in place of the probe's.
#[cfg(test)]
pub fn set_table(c: Option<Caps>) {
    let slot = TABLE.get_or_init(|| Mutex::new(None));
    *slot.lock().unwrap_or_else(|e| e.into_inner()) = c.map(Arc::new);
}

// ---- Milan's rules ----

/// The encoder a Render As template uses for a source codec, so Media Properties can say which templates write the
/// same format.
pub fn encoder_for(codec: &str) -> Option<&'static str> {
    Some(match codec {
        "h264" => "libx264",
        "hevc" => "libx265",
        "av1" => "libsvtav1",
        "vp9" => "libvpx-vp9",
        "vp8" => "libvpx",
        "mpeg2video" => "mpeg2video",
        "prores" => "prores_ks",
        "dnxhd" => "dnxhd",
        "cfhd" => "cfhd",
        "ffv1" => "ffv1",
        "mjpeg" => "mjpeg",
        "png" | "tiff" | "exr" | "dpx" => return Some(match codec {
            "png" => "png",
            "tiff" => "tiff",
            "exr" => "exr",
            _ => "dpx",
        }),
        "aac" => "aac",
        "mp3" => "libmp3lame",
        "flac" => "flac",
        "alac" => "alac",
        "opus" => "libopus",
        "vorbis" => "libvorbis",
        "ac3" => "ac3",
        "eac3" => "eac3",
        c if c.starts_with("pcm_") => "pcm",
        _ => return None,
    })
}

/// Why a stream of `codec` cannot be imported, if it cannot: no decoder in this FFmpeg build.
pub fn unsupported(caps: &Caps, codec: &str) -> Option<String> {
    let why = match codec {
        "svg" => "this FFmpeg build has no SVG decoder (it needs one built with librsvg)",
        _ => "this FFmpeg build has no decoder for it",
    };
    (!codec.is_empty() && !caps.can_decode(codec)).then(|| format!("unsupported codec {codec}: {why}"))
}

/// Media Properties: the capability verdict for one item, as (label, value) rows.
pub fn verdict(m: &MediaItem, caps: Option<&Caps>) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    if let Some(why) = &m.unsupported {
        rows.push(("Decode".into(), format!("No: {why}")));
        return rows;
    }
    if m.generator.is_some() {
        rows.push(("Decode".into(), "Generated media (drawn by its plug-in)".into()));
        return rows;
    }
    if m.nested {
        rows.push(("Decode".into(), "Nested timeline (composed by the engine)".into()));
        return rows;
    }
    let codec = m.codec.as_deref().unwrap_or("");
    let Some(caps) = caps else {
        rows.push(("Decode".into(), "Checking the FFmpeg build...".into()));
        return rows;
    };
    let decode = match (codec.is_empty(), caps.can_decode(codec)) {
        (true, _) => "Yes (ffprobe named no codec)".to_string(),
        (false, true) => {
            let hw = (m.media_type == MediaType::Video).then(|| crate::engine::hw::decode_hwaccel(Some(codec), m.width.unwrap_or(0))).flatten();
            match hw {
                Some(h) => format!("Yes: {codec}, hardware ({h}) during playback, software otherwise"),
                None => format!("Yes: {codec}, software decoder"),
            }
        }
        (false, false) => format!("No: this FFmpeg build has no {codec} decoder"),
    };
    rows.push(("Decode".into(), decode));
    if m.media_type != MediaType::Audio {
        let proxy = if m.sequence.is_some() {
            "No: image sequences are read from their files".to_string()
        } else if m.media_type == MediaType::Image {
            "No: a still needs none".to_string()
        } else if m.has_alpha {
            "No: the H.264 proxy has no alpha channel".to_string()
        } else if crate::proxy::is_video(m) {
            format!("Yes (Create Video Proxy{})", if crate::proxy::wants_auto(m) { ", made automatically when Preferences › Video says so" } else { "" })
        } else {
            "No".to_string()
        };
        rows.push(("Proxy".into(), proxy));
    }
    let render = match encoder_for(codec) {
        Some(enc) if caps.can_encode(enc) => {
            let names: Vec<&str> = crate::render::BUILTIN
                .iter()
                .filter(|t| t.video.as_ref().is_some_and(|v| v.codec == enc) || (m.media_type == MediaType::Audio && t.video.is_none() && t.audio.as_ref().is_some_and(|a| a.codec == enc)))
                .map(|t| t.name.as_str())
                .collect();
            if names.is_empty() { format!("Yes: {enc} through Customize Template") } else { format!("Yes: {}", names.join(", ")) }
        }
        Some(enc) => format!("No: this FFmpeg build has no {enc} encoder; render to another format"),
        None if codec.is_empty() => "-".into(),
        None => format!("No {codec} encoder in Milan Pro's templates; render to another format"),
    };
    rows.push(("Render as same".into(), render));
    if m.hdr().is_some() && !caps.hdr_ok() {
        rows.push(("Warning".into(), "This FFmpeg build lacks zscale or tonemap: HDR shows as coded".into()));
    }
    rows
}

// ---- The test corpus and the report ----

/// The legal test corpus (every file made from lavfi sources) and the sections of docs/FORMAT_SUPPORT.md the ignored
/// test `format_corpus_e2e` (and `tools/make-test-media.ps1`, which runs it) writes. Only tests use it.
#[cfg_attr(not(test), allow(dead_code))]
pub mod corpus {
use super::*;

/// One generated file of the corpus: `smptehdbars` picture (`size`, `rate`) and a 1 kHz `sine` (`layout` channels),
/// encoded with `args`, written as `file`. `video` / `audio` are the codec names ffprobe must report.
pub struct Row {
    pub file: &'static str,
    pub size: &'static str,
    pub rate: &'static str,
    pub video: Option<&'static str>,
    pub audio: Option<&'static str>,
    pub layout: &'static str,
    pub args: &'static [&'static str],
    pub note: &'static str,
    pub licence: &'static str,
}

const fn row(file: &'static str, size: &'static str, rate: &'static str, video: Option<&'static str>, audio: Option<&'static str>, layout: &'static str, args: &'static [&'static str], note: &'static str, licence: &'static str) -> Row {
    Row { file, size, rate, video, audio, layout, args, note, licence }
}

const FREE: &str = "royalty-free (FFmpeg LGPL/GPL)";
const MPEG: &str = "patent pools (MPEG LA / Via LA / Access Advance); FFmpeg LGPL/GPL";
const APPLE: &str = "Apple format; FFmpeg's own implementation, LGPL";
const AVID: &str = "Avid VC-3 (SMPTE 2019); FFmpeg's own implementation, LGPL";
const DOLBY: &str = "Dolby patents (AC-3 expired 2017 in most countries); FFmpeg LGPL";
const DTS_L: &str = "DTS patents; FFmpeg's dca decoder LGPL, its encoder experimental (core only)";
const MS: &str = "Microsoft format; FFmpeg's own implementation, LGPL";

/// The legal corpus `format_corpus_e2e` generates (every file from lavfi sources, nothing recorded).
pub static CORPUS: &[Row] = &[
    // Containers and video codecs.
    row("h264_420_8.mp4", "1920x1080", "30000/1001", Some("h264"), Some("aac"), "stereo", &["-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", "-c:a", "aac"], "H.264 8-bit 4:2:0, 29.97p", MPEG),
    row("h264_422_10.mov", "1920x1080", "25", Some("h264"), Some("pcm_s16le"), "stereo", &["-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv422p10le", "-c:a", "pcm_s16le"], "H.264 10-bit 4:2:2 (16-bit in a 32-bit floating point project)", MPEG),
    row("hevc_420_8.mp4", "1280x720", "50", Some("hevc"), Some("aac"), "stereo", &["-c:v", "libx265", "-preset", "ultrafast", "-x265-params", "log-level=error", "-pix_fmt", "yuv420p", "-c:a", "aac"], "HEVC 8-bit, 50p", MPEG),
    row("hevc_420_10_uhd.mp4", "3840x2160", "25", Some("hevc"), Some("aac"), "stereo", &["-c:v", "libx265", "-preset", "ultrafast", "-x265-params", "log-level=error", "-pix_fmt", "yuv420p10le", "-c:a", "aac"], "HEVC Main 10, 3840x2160 (hardware decode when it measures faster; auto proxy)", MPEG),
    row("av1.mp4", "640x360", "24000/1001", Some("av1"), Some("aac"), "stereo", &["-c:v", "libsvtav1", "-preset", "12", "-pix_fmt", "yuv420p", "-c:a", "aac"], "AV1 (SVT), 23.976p", "AOMedia royalty-free; SVT-AV1 BSD"),
    row("vp8.webm", "640x360", "25", Some("vp8"), Some("vorbis"), "stereo", &["-c:v", "libvpx", "-deadline", "realtime", "-cpu-used", "8", "-c:a", "libvorbis"], "VP8 / Vorbis", "royalty-free (Google); libvpx BSD"),
    row("vp9.webm", "1280x720", "60000/1001", Some("vp9"), Some("opus"), "stereo", &["-c:v", "libvpx-vp9", "-deadline", "realtime", "-cpu-used", "8", "-row-mt", "1", "-c:a", "libopus"], "VP9 / Opus, 59.94p", "royalty-free (Google); libvpx BSD"),
    row("mpeg1.mpg", "640x360", "25", Some("mpeg1video"), Some("mp2"), "stereo", &["-c:v", "mpeg1video", "-c:a", "mp2", "-f", "mpeg"], "MPEG-1 program stream", "patents expired"),
    row("mpeg2_480i.m2ts", "720x480", "30000/1001", Some("mpeg2video"), Some("ac3"), "stereo", &["-c:v", "mpeg2video", "-flags", "+ilme+ildct", "-field_order", "tt", "-c:a", "ac3", "-f", "mpegts"], "MPEG-2 480i upper field first in a transport stream (deinterlaced per Project Properties)", "MPEG-2 patents expired 2018"),
    row("xdcam_hd422.mxf", "1920x1080", "25", Some("mpeg2video"), Some("pcm_s16le"), "stereo", &["-c:v", "mpeg2video", "-pix_fmt", "yuv422p", "-b:v", "50M", "-minrate", "50M", "-maxrate", "50M", "-bufsize", "17825792", "-g", "12", "-bf", "2", "-c:a", "pcm_s16le", "-f", "mxf"], "XDCAM HD422 stand-in: MPEG-2 4:2:2 50 Mb/s in MXF OP1a", "MPEG-2 patents expired 2018; MXF SMPTE"),
    row("mpeg4.avi", "640x360", "25", Some("mpeg4"), Some("mp3"), "stereo", &["-c:v", "mpeg4", "-q:v", "3", "-c:a", "libmp3lame"], "MPEG-4 Part 2 / MP3 in AVI", "MPEG-4 Visual patents largely expired; MP3 expired 2017"),
    row("prores_422.mov", "1280x720", "25", Some("prores"), Some("pcm_s24le"), "stereo", &["-c:v", "prores_ks", "-profile:v", "2", "-c:a", "pcm_s24le"], "ProRes 422, 24-bit PCM", APPLE),
    row("prores_4444_alpha.mov", "640x360", "25", Some("prores"), None, "none", &["-vf", "format=yuva444p10le", "-c:v", "prores_ks", "-profile:v", "4", "-an"], "ProRes 4444 with alpha (shown over black; a proxy is refused)", APPLE),
    row("dnxhd_1080p.mxf", "1920x1080", "30000/1001", Some("dnxhd"), Some("pcm_s16le"), "stereo", &["-c:v", "dnxhd", "-pix_fmt", "yuv422p", "-b:v", "45M", "-c:a", "pcm_s16le", "-f", "mxf"], "DNxHD 1080p29.97 45 Mb/s in MXF OP1a", AVID),
    row("dnxhr_hq.mov", "1280x720", "50", Some("dnxhd"), Some("pcm_s16le"), "stereo", &["-c:v", "dnxhd", "-profile:v", "dnxhr_hq", "-pix_fmt", "yuv422p", "-c:a", "pcm_s16le"], "DNxHR HQ, 50p", AVID),
    row("cineform.mov", "640x360", "25", Some("cfhd"), Some("pcm_s16le"), "stereo", &["-c:v", "cfhd", "-pix_fmt", "yuv422p10le", "-c:a", "pcm_s16le"], "GoPro CineForm 10-bit", "GoPro CineForm SDK Apache-2.0/MIT; FFmpeg's own codec LGPL"),
    row("mjpeg.avi", "640x360", "25", Some("mjpeg"), Some("pcm_s16le"), "stereo", &["-c:v", "mjpeg", "-pix_fmt", "yuvj422p", "-q:v", "3", "-c:a", "pcm_s16le"], "Motion JPEG (full-range YUV)", FREE),
    row("theora.ogv", "640x360", "25", Some("theora"), Some("vorbis"), "stereo", &["-c:v", "libtheora", "-q:v", "6", "-c:a", "libvorbis"], "Theora / Vorbis in Ogg (decode only in the templates)", "royalty-free (Xiph); libtheora BSD"),
    row("ffv1.mkv", "640x360", "25", Some("ffv1"), Some("flac"), "stereo", &["-c:v", "ffv1", "-level", "3", "-pix_fmt", "yuv422p10le", "-c:a", "flac"], "FFV1 lossless 10-bit / FLAC in Matroska", FREE),
    row("qtrle.mov", "640x360", "25", Some("qtrle"), None, "none", &["-c:v", "qtrle", "-pix_fmt", "rgb24", "-an"], "QuickTime Animation (RLE), RGB", APPLE),
    row("png_in_mov.mov", "640x360", "25", Some("png"), Some("alac"), "stereo", &["-c:v", "png", "-pix_fmt", "rgb24", "-c:a", "alac"], "PNG video track / ALAC", FREE),
    row("utvideo.avi", "640x360", "25", Some("utvideo"), Some("pcm_s16le"), "stereo", &["-c:v", "utvideo", "-pix_fmt", "yuv422p", "-c:a", "pcm_s16le"], "Ut Video lossless", "Ut Video GPL; FFmpeg's own codec LGPL"),
    row("vfr.mp4", "640x360", "30", Some("h264"), None, "none", &["-vf", "select=not(eq(mod(n\\,5)\\,0))", "-fps_mode", "vfr", "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", "-an"], "Variable frame rate (every fifth frame dropped, timestamps kept): conformed by the fps filter", MPEG),
    // Audio.
    row("pcm16.wav", "", "", None, Some("pcm_s16le"), "stereo", &["-c:a", "pcm_s16le"], "WAV 16-bit", FREE),
    row("pcm24_mono.wav", "", "", None, Some("pcm_s24le"), "mono", &["-c:a", "pcm_s24le"], "WAV 24-bit mono (played at unity to both sides)", FREE),
    row("pcm_f32.wav", "", "", None, Some("pcm_f32le"), "stereo", &["-c:a", "pcm_f32le"], "WAV 32-bit float", FREE),
    row("aiff.aif", "", "", None, Some("pcm_s16be"), "stereo", &["-c:a", "pcm_s16be"], "AIFF 16-bit", FREE),
    row("mp3.mp3", "", "", None, Some("mp3"), "stereo", &["-c:a", "libmp3lame", "-b:a", "192k"], "MP3", "patents expired 2017; LAME LGPL"),
    row("aac.m4a", "", "", None, Some("aac"), "stereo", &["-c:a", "aac", "-b:a", "160k"], "AAC-LC in MPEG-4 audio", "Via LA patent pool; FFmpeg's encoder LGPL"),
    row("flac.flac", "", "", None, Some("flac"), "stereo", &["-c:a", "flac", "-sample_fmt", "s32"], "FLAC 24-bit", FREE),
    row("alac.m4a", "", "", None, Some("alac"), "stereo", &["-c:a", "alac"], "Apple Lossless", "Apple, Apache-2.0 reference; FFmpeg LGPL"),
    row("vorbis.ogg", "", "", None, Some("vorbis"), "stereo", &["-c:a", "libvorbis"], "Ogg Vorbis", "royalty-free (Xiph); libvorbis BSD"),
    row("opus.opus", "", "", None, Some("opus"), "stereo", &["-c:a", "libopus"], "Opus", "royalty-free (IETF); libopus BSD"),
    row("wma.wma", "", "", None, Some("wmav2"), "stereo", &["-c:a", "wmav2"], "Windows Media Audio 2 in ASF (decode only in the templates)", MS),
    row("ac3_51.ac3", "", "", None, Some("ac3"), "5.1", &["-c:a", "ac3"], "AC-3 5.1 (all six channels on a 5.1 track, folded down on a stereo one)", DOLBY),
    row("eac3_71.eac3", "", "", None, Some("eac3"), "7.1", &["-c:a", "eac3"], "E-AC-3 7.1 (all eight channels on a 7.1 track, folded down on a stereo one)", DOLBY),
    row("dts_51.dts", "", "", None, Some("dts"), "5.1", &["-c:a", "dca", "-strict", "-2"], "DTS 5.1 (decode only: the encoder is experimental, core only)", DTS_L),
];

/// The lavfi inputs and the output file for `r`: picture from `smptehdbars`, sound a 1 kHz sine fanned out to its
/// channel layout, 2 s.
pub fn corpus_args(r: &Row, out: &Path) -> Vec<String> {
    let mut a: Vec<String> = vec!["-v".into(), "error".into(), "-y".into()];
    if r.video.is_some() {
        a.extend(["-f".into(), "lavfi".into(), "-i".into(), format!("smptehdbars=s={}:r={}:d=2", r.size, r.rate)]);
    }
    if r.audio.is_some() {
        a.extend(["-f".into(), "lavfi".into(), "-i".into(), "sine=f=1000:r=48000:d=2".into()]);
        let n = match r.layout {
            "mono" => 1,
            "5.1" => 6,
            "7.1" => 8,
            _ => 2,
        };
        if n > 1 {
            let map: Vec<String> = (0..n).map(|k| format!("c{k}=c0")).collect();
            a.extend(["-af".into(), format!("pan={}|{}", r.layout, map.join("|"))]);
        }
    }
    a.extend(r.args.iter().map(|s| s.to_string()));
    a.push(out.to_string_lossy().into_owned());
    a
}

/// Where the corpus goes: `MILAN_TESTMEDIA`, else `target/testmedia` of this checkout.
pub fn corpus_dir() -> PathBuf {
    std::env::var_os("MILAN_TESTMEDIA").map(PathBuf::from).unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("testmedia"))
}

/// Codec families the capability section lists: (label, codec id, encoder name or "").
pub static FAMILIES: &[(&str, &str, &str)] = &[
    ("H.264 / AVC", "h264", "libx264"),
    ("H.265 / HEVC", "hevc", "libx265"),
    ("AV1", "av1", "libsvtav1"),
    ("VP8", "vp8", "libvpx"),
    ("VP9", "vp9", "libvpx-vp9"),
    ("MPEG-1", "mpeg1video", "mpeg1video"),
    ("MPEG-2", "mpeg2video", "mpeg2video"),
    ("MPEG-4 Part 2", "mpeg4", "mpeg4"),
    ("ProRes", "prores", "prores_ks"),
    ("ProRes RAW", "prores_raw", ""),
    ("DNxHD / DNxHR", "dnxhd", "dnxhd"),
    ("CineForm", "cfhd", "cfhd"),
    ("MJPEG", "mjpeg", "mjpeg"),
    ("Theora", "theora", "libtheora"),
    ("FFV1", "ffv1", "ffv1"),
    ("QuickTime Animation", "qtrle", "qtrle"),
    ("Ut Video", "utvideo", "utvideo"),
    ("PNG", "png", "png"),
    ("TIFF", "tiff", "tiff"),
    ("JPEG", "mjpeg", "mjpeg"),
    ("OpenEXR", "exr", "exr"),
    ("DPX", "dpx", "dpx"),
    ("Radiance HDR", "hdr", "hdr"),
    ("JPEG 2000", "jpeg2000", "jpeg2000"),
    ("WebP", "webp", "libwebp"),
    ("GIF", "gif", "gif"),
    ("BMP", "bmp", "bmp"),
    ("Photoshop PSD", "psd", ""),
    ("TGA", "targa", "targa"),
    ("SVG", "svg", ""),
    ("PCM", "pcm_s16le", "pcm"),
    ("AAC", "aac", "aac"),
    ("MP3", "mp3", "libmp3lame"),
    ("FLAC", "flac", "flac"),
    ("ALAC", "alac", "alac"),
    ("Vorbis", "vorbis", "libvorbis"),
    ("Opus", "opus", "libopus"),
    ("WMA", "wmav2", "wmav2"),
    ("AC-3", "ac3", "ac3"),
    ("E-AC-3", "eac3", "eac3"),
    ("DTS", "dts", "dca"),
    ("MP2", "mp2", "mp2"),
];

/// Containers the capability section lists: (label, demuxer, muxer).
pub static CONTAINERS: &[(&str, &str, &str)] = &[
    ("MP4", "mp4", "mp4"),
    ("QuickTime MOV", "mov", "mov"),
    ("Matroska MKV", "matroska", "matroska"),
    ("WebM", "webm", "webm"),
    ("AVI", "avi", "avi"),
    ("MPEG-2 TS (.ts, .m2ts, .mts)", "mpegts", "mpegts"),
    ("MPEG-2 PS (.mpg, .vob)", "mpeg", "vob"),
    ("MXF", "mxf", "mxf"),
    ("Ogg", "ogg", "ogg"),
    ("ASF / WMV / WMA", "asf", "asf"),
    ("WAV", "wav", "wav"),
    ("AIFF", "aiff", "aiff"),
    ("FLAC", "flac", "flac"),
    ("Image sequence (image2)", "image2", "image2"),
    ("Concatenation list (.ffconcat, spanned clips)", "concat", ""),
    ("GIF", "gif", "gif"),
    ("Animated WebP", "webp_anim", "webp"),
];

/// The capability section of docs/FORMAT_SUPPORT.md from a table.
pub fn capability_section(c: &Caps) -> String {
    let mut s = format!("### Capability table ({})\n\nContainers:\n\n| Container | Read | Write |\n|---|---|---|\n", c.ffmpeg);
    let yn = |b: bool| if b { "yes" } else { "no" };
    for (label, dem, mux) in CONTAINERS {
        s += &format!("| {label} | {} | {} |\n", yn(c.has_demuxer(dem)), if mux.is_empty() { "-".to_string() } else { yn(c.has_muxer(mux)).to_string() });
    }
    s += "\nCodecs:\n\n| Codec | Decode | Encode | Milan Pro render templates |\n|---|---|---|---|\n";
    for (label, id, enc) in FAMILIES {
        let templates: Vec<&str> = crate::render::BUILTIN.iter().filter(|t| t.video.as_ref().is_some_and(|v| v.codec == *enc) || (t.video.is_none() && t.audio.as_ref().is_some_and(|a| a.codec == *enc))).map(|t| t.name.as_str()).collect();
        let encode = if enc.is_empty() { "no encoder".to_string() } else { yn(c.can_encode(enc)).to_string() };
        s += &format!("| {label} | {} | {encode} | {} |\n", yn(c.can_decode(id)), if templates.is_empty() { "-".to_string() } else { templates.join(", ") });
    }
    s += &format!("\nFilters the view transform needs: zscale {}, tonemap {}. Hardware decoders and encoders: see Preferences › Video (R22).\n", yn(c.has_filter("zscale")), yn(c.has_filter("tonemap")));
    s
}

/// Replace (or append) the block between `<!-- {tag}:start -->` and `<!-- {tag}:end -->` in `text`.
pub fn replace_block(text: &str, tag: &str, body: &str) -> String {
    let (a, b) = (format!("<!-- {tag}:start -->"), format!("<!-- {tag}:end -->"));
    match (text.find(&a), text.find(&b)) {
        (Some(i), Some(k)) if k > i => format!("{}{a}\n{body}{b}{}", &text[..i], &text[k + b.len()..]),
        _ => format!("{}\n{a}\n{body}{b}\n", text.trim_end()),
    }
}
}

#[cfg(test)]
pub(crate) mod tests {
    use super::corpus::*;
    use super::*;

    const DECODERS: &str = "Decoders:
 V..... = Video
 ------
 V....D 012v                 Uncompressed 4:2:2 10-bit
 VFS..D h264                 H.264 / AVC / MPEG-4 AVC / MPEG-4 part 10
 V..... libdav1d             dav1d AV1 decoder by VideoLAN (codec av1)
 VFS..D prores               Apple ProRes (iCodec Pro)
 A....D mp3float             MP3 (MPEG audio layer 3) (codec mp3)
 A....D dca                  DCA (DTS Coherent Acoustics) (codec dts)
 S..... srt                  SubRip subtitle
";
    const FORMATS: &str = "Formats:
 D.. = Demuxing supported
 .E. = Muxing supported
 ..d = Is a device
 ---
 D   concat          Virtual concatenation script
 D   mov,mp4,m4a,3gp,3g2,mj2 QuickTime / MOV
  E  image2          image2 sequence
 DE  mxf             MXF (Material eXchange Format)
";
    const PIX: &str = "Pixel formats:
I.... = Supported Input  format for conversion
FLAGS NAME            NB_COMPONENTS BITS_PER_PIXEL BIT_DEPTHS
-----
IO... yuv420p                3             12      8-8-8
IO... gbrpf32le              3             96      32-32-32
..H.. d3d11                  0              0      0
";
    const FILTERS: &str = "Filters:
  T.. = Timeline support
  ------
 TS aap               AA->A      Apply Affine Projection algorithm to first audio stream.
 .S tonemap           V->V       Conversion to/from different dynamic ranges.
 .S zscale            V->V       Apply resizing, colorspace and bit depth conversion.
";

    /// A table like the bundled build's (every corpus codec decodes).
    pub(crate) fn full() -> Caps {
        let dec: BTreeSet<String> = CORPUS.iter().flat_map(|r| [r.video, r.audio]).flatten().map(String::from).chain(["png", "tiff", "exr", "hdr", "dpx", "psd", "webp", "gif", "bmp", "targa", "jpeg2000", "pcm_s16le", "mp2", "wmav2", "prores_raw"].map(String::from)).collect();
        let enc: BTreeSet<String> = FAMILIES.iter().map(|f| f.2.to_string()).filter(|e| !e.is_empty() && e != "psd" && e != "pcm").chain(["pcm_s16le".to_string()]).collect();
        Caps {
            ffmpeg: "ffmpeg version test".into(),
            decoders: dec,
            encoders: enc,
            demuxers: CONTAINERS.iter().map(|c| c.1.to_string()).collect(),
            muxers: CONTAINERS.iter().map(|c| c.2.to_string()).filter(|m| !m.is_empty()).collect(),
            pix_fmts: ["yuv420p", "rgba"].map(String::from).into_iter().collect(),
            filters: ["zscale", "tonemap", "scale"].map(String::from).into_iter().collect(),
        }
    }

    /// R37 acceptance: the capability parser on captured output.
    #[test]
    fn capability_parser() {
        let d = parse_codecs(DECODERS);
        assert!(d.contains("h264") && d.contains("prores") && d.contains("av1") && d.contains("libdav1d") && d.contains("mp3") && d.contains("dts") && d.contains("srt"));
        assert!(!d.contains("Video") && !d.contains("V.....") && !d.contains("Decoders:"));
        let f = parse_formats(FORMATS);
        assert_eq!(f.iter().map(String::as_str).collect::<Vec<_>>(), ["3g2", "3gp", "concat", "image2", "m4a", "mj2", "mov", "mp4", "mxf"]);
        let p = parse_pix_fmts(PIX);
        assert_eq!(p.iter().map(String::as_str).collect::<Vec<_>>(), ["d3d11", "gbrpf32le", "yuv420p"]);
        let fl = parse_filters(FILTERS);
        assert_eq!(fl.iter().map(String::as_str).collect::<Vec<_>>(), ["aap", "tonemap", "zscale"]);
        let caps = Caps { decoders: d, demuxers: f.clone(), muxers: f, pix_fmts: p, filters: fl, ..Default::default() };
        assert!(caps.can_decode("h264") && !caps.can_decode("braw") && caps.hdr_ok() && caps.has_demuxer("concat"));
        assert_eq!(unsupported(&caps, "cinepak").as_deref(), Some("unsupported codec cinepak: this FFmpeg build has no decoder for it"));
        assert_eq!(unsupported(&caps, "h264"), None);
        assert_eq!(unsupported(&caps, ""), None, "a stream ffprobe names no codec for is left to the decoder");
        // Every corpus row names a codec with a decoder in the full table, and an encoder the templates use.
        let full = full();
        for r in CORPUS {
            assert!(r.video.is_none_or(|v| full.can_decode(v)) && r.audio.is_none_or(|a| full.can_decode(a)), "{}", r.file);
        }
        assert_eq!(encoder_for("prores"), Some("prores_ks"));
        assert_eq!(encoder_for("pcm_s24le"), Some("pcm"));
        assert_eq!(encoder_for("theora"), None);
        // The corpus covers what R37 asks for.
        let files: Vec<&str> = CORPUS.iter().map(|r| r.file).collect();
        for want in ["h264", "hevc", "av1", "vp8", "vp9", "mpeg1", "mpeg2", "mpeg4", "prores", "dnxh", "cineform", "mjpeg", "theora", "ffv1", "qtrle", "png_in_mov", "utvideo", "vfr", "wav", "aif", "mp3", "aac", "flac", "alac", "vorbis", "opus", "wma", "ac3", "eac3", "dts"] {
            assert!(files.iter().any(|f| f.contains(want)), "{want}");
        }
        let layouts: BTreeSet<&str> = CORPUS.iter().map(|r| r.layout).collect();
        assert!(layouts.contains("mono") && layouts.contains("stereo") && layouts.contains("5.1") && layouts.contains("7.1"));
        let a = corpus_args(&CORPUS[0], Path::new(r"C:\t\h264_420_8.mp4")).join(" ");
        assert!(a.starts_with("-v error -y -f lavfi -i smptehdbars=s=1920x1080:r=30000/1001:d=2 -f lavfi -i sine=f=1000:r=48000:d=2 -af pan=stereo|c0=c0|c1=c0 -c:v libx264"), "{a}");
        let a = corpus_args(CORPUS.iter().find(|r| r.file == "eac3_71.eac3").unwrap(), Path::new("x.eac3")).join(" ");
        assert!(a.contains("-af pan=7.1|c0=c0|c1=c0|c2=c0|c3=c0|c4=c0|c5=c0|c6=c0|c7=c0 -c:a eac3 x.eac3") && !a.contains("smptehdbars"), "{a}");
        assert_eq!(replace_block("a\n<!-- t:start -->\nold\n<!-- t:end -->\nz", "t", "new\n"), "a\n<!-- t:start -->\nnew\n<!-- t:end -->\nz");
        assert_eq!(replace_block("a", "t", "new\n"), "a\n<!-- t:start -->\nnew\n<!-- t:end -->\n");
    }

    /// Media Properties' verdict rows: an unsupported item, a decodable video (with its proxy and templates), audio.
    #[test]
    fn verdict_rows() {
        let caps = full();
        let m = MediaItem { media_type: MediaType::Video, codec: Some("prores".into()), width: Some(1920), height: Some(1080), sample_rate: Some(48000), ..Default::default() };
        let v = verdict(&m, Some(&caps));
        assert!(v[0].1.starts_with("Yes: prores, software"), "{v:?}");
        assert!(v[1].1.starts_with("Yes (Create Video Proxy"), "{v:?}");
        assert!(v[2].1.contains("MOV ProRes 422") && v[2].1.contains("ProRes 4444 (alpha)"), "{v:?}");
        let alpha = MediaItem { has_alpha: true, ..m.clone() };
        assert!(verdict(&alpha, Some(&caps))[1].1.contains("no alpha"));
        let bad = MediaItem { unsupported: Some("unsupported codec cinepak: no decoder".into()), ..m.clone() };
        assert_eq!(verdict(&bad, Some(&caps)), [("Decode".to_string(), "No: unsupported codec cinepak: no decoder".to_string())]);
        let theora = MediaItem { codec: Some("theora".into()), ..m.clone() };
        assert!(verdict(&theora, Some(&caps))[2].1.starts_with("No theora encoder"));
        let a = MediaItem { media_type: MediaType::Audio, codec: Some("flac".into()), sample_rate: Some(48000), ..Default::default() };
        let v = verdict(&a, Some(&caps));
        assert_eq!(v.len(), 2, "{v:?}");
        assert!(v[1].1.contains("FLAC audio"), "{v:?}");
        assert_eq!(verdict(&m, None)[0].1, "Checking the FFmpeg build...");
        let seq = MediaItem { sequence: Some(crate::project::Sequence { pattern: "a_%04d.png".into(), start: 1, count: 2, fps: 24.0 }), codec: Some("png".into()), ..m.clone() };
        assert!(verdict(&seq, Some(&caps))[1].1.starts_with("No: image sequences"));
        let s = capability_section(&caps);
        assert!(s.contains("| H.264 / AVC | yes | yes | MP4 H.264 (AAC)") && s.contains("| ProRes RAW | yes | no encoder | - |") && s.contains("| MXF | yes | yes |"), "{s}");
    }

    /// Needs ffmpeg. The real build's table: everything the corpus and the view transform need is there.
    /// `cargo test --release -- --ignored bundled_ffmpeg_capabilities`
    #[test]
    #[ignore]
    fn bundled_ffmpeg_capabilities() {
        let c = probe();
        assert!(c.ffmpeg.starts_with("ffmpeg version"), "{}", c.ffmpeg);
        for r in CORPUS {
            assert!(r.video.is_none_or(|v| c.can_decode(v)) && r.audio.is_none_or(|a| c.can_decode(a)), "{}", r.file);
        }
        assert!(c.hdr_ok() && c.has_demuxer("concat") && c.has_demuxer("image2") && c.has_filter("exposure"));
        assert!(c.can_encode("prores_ks") && c.can_encode("dnxhd") && c.can_encode("cfhd") && c.can_encode("libx265"));
        assert!(!c.can_decode("braw") && !c.can_decode("r3d"));
    }
}
