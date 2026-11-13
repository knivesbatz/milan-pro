//! Render As: render templates (built-in and the user's), their validation and encoder arguments, and the background
//! `Job` the dialog watches (the render itself is `engine::render`).
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Instant;
use crate::decode::Quality;
use crate::project::{FieldOrder, Project, ProjectSettings};

// ---- Templates (R11) ----

/// A render template, as Render As lists it and Customize Template edits it. Plain data, so a queued render (R12) can
/// keep its own copy; user templates are kept in the preferences.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Template {
    pub name: String,
    pub notes: String,
    /// `CONTAINERS` id.
    pub container: String,
    /// None: no video stream (an audio file).
    pub video: Option<VideoEnc>,
    /// None: no audio stream (an image sequence, or video only).
    pub audio: Option<AudioEnc>,
    /// Custom Settings › Project › Video rendering quality; None = the project's Full-resolution rendering quality.
    pub quality: Option<Quality>,
}

/// The video settings of a template. None in `size`, `fps`, `field` and `par` follows the project.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoEnc {
    /// `VCODECS` encoder.
    pub codec: String,
    pub size: Option<(u32, u32)>,
    pub fps: Option<f64>,
    pub field: Option<FieldOrder>,
    pub par: Option<f64>,
    pub rate: Rate,
    /// The codec's speed preset ("" = its default).
    pub preset: String,
    /// The codec's profile ("" = its first).
    pub profile: String,
    /// "" = the codec's first pixel format.
    pub pix_fmt: String,
}

/// The audio settings of a template. Mixed in stereo; `channels` 1 downmixes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioEnc {
    /// `ACODECS` encoder ("pcm": the bit depth and the container choose the sample format).
    pub codec: String,
    /// Bit rate, kbps (codecs with a bit rate).
    pub kbps: u32,
    /// None = the project's sample rate (capped to what the codec takes).
    pub rate: Option<u32>,
    pub channels: u8,
    /// PCM, FLAC and ALAC: 8, 16, 24 or 32 (float for PCM); None = the project's Bit depth.
    pub bits: Option<u8>,
}

/// Video rate control. Bit rates in kbps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Rate {
    /// The codec decides (intra codecs whose profile sets the rate).
    #[default]
    Default,
    /// Constant quality: CRF (x264, x265, SVT-AV1, VP8, VP9) or qscale (MPEG-2, MJPEG).
    Quality(u32),
    Vbr { avg: u32, max: u32 },
    Cbr(u32),
}

impl Default for Template {
    fn default() -> Self {
        Self { name: "Untitled".into(), notes: String::new(), container: "mp4".into(), video: Some(VideoEnc::default()), audio: Some(AudioEnc::default()), quality: None }
    }
}

impl Default for VideoEnc {
    fn default() -> Self {
        Self { codec: "libx264".into(), size: None, fps: None, field: None, par: None, rate: Rate::Quality(18), preset: String::new(), profile: String::new(), pix_fmt: String::new() }
    }
}

impl Default for AudioEnc {
    fn default() -> Self {
        Self { codec: "aac".into(), kbps: 320, rate: None, channels: 2, bits: None }
    }
}

pub struct Container {
    pub id: &'static str,
    pub label: &'static str,
    /// "" for an image sequence: the codec names the extension.
    pub ext: &'static str,
    /// ffmpeg `-f`: the output is written to `<out>.partial`, so the format cannot come from the extension.
    pub muxer: &'static str,
    pub video: &'static [&'static str],
    pub audio: &'static [&'static str],
    /// Save project markers in media file: the format holds chapters.
    pub chapters: bool,
}

const ALL_V: &[&str] = &["libx264", "libx265", "libsvtav1", "libvpx-vp9", "libvpx", "mpeg2video", "prores_ks", "dnxhd", "cfhd", "ffv1", "mjpeg"];
pub static CONTAINERS: [Container; 18] = [
    Container { id: "mp4", label: "MP4", ext: "mp4", muxer: "mp4", video: &["libx264", "libx265", "libsvtav1", "libvpx-vp9"], audio: &["aac", "libmp3lame", "ac3", "eac3", "alac", "libopus"], chapters: true },
    Container { id: "mov", label: "QuickTime", ext: "mov", muxer: "mov", video: &["libx264", "libx265", "prores_ks", "dnxhd", "cfhd", "mjpeg"], audio: &["pcm", "aac", "alac"], chapters: true },
    Container { id: "mkv", label: "Matroska", ext: "mkv", muxer: "matroska", video: ALL_V, audio: &["pcm", "flac", "aac", "libopus", "libvorbis", "ac3", "eac3", "libmp3lame", "alac"], chapters: true },
    Container { id: "webm", label: "WebM", ext: "webm", muxer: "webm", video: &["libvpx-vp9", "libvpx", "libsvtav1"], audio: &["libopus", "libvorbis"], chapters: true },
    Container { id: "avi", label: "AVI", ext: "avi", muxer: "avi", video: &["mjpeg", "libx264", "ffv1", "mpeg2video"], audio: &["pcm", "libmp3lame", "ac3"], chapters: false },
    Container { id: "ts", label: "MPEG-2 Transport Stream", ext: "m2ts", muxer: "mpegts", video: &["mpeg2video", "libx264", "libx265"], audio: &["ac3", "eac3", "aac", "mp2"], chapters: false },
    Container { id: "mpg", label: "MPEG-2 Program Stream", ext: "mpg", muxer: "vob", video: &["mpeg2video"], audio: &["ac3", "mp2"], chapters: false },
    Container { id: "mxf", label: "MXF", ext: "mxf", muxer: "mxf", video: &["dnxhd", "mpeg2video"], audio: &["pcm"], chapters: false },
    Container { id: "image", label: "Image Sequence", ext: "", muxer: "image2", video: &["png", "tiff", "exr", "dpx"], audio: &[], chapters: false },
    Container { id: "wav", label: "Wave", ext: "wav", muxer: "wav", video: &[], audio: &["pcm"], chapters: false },
    Container { id: "aiff", label: "AIFF", ext: "aif", muxer: "aiff", video: &[], audio: &["pcm"], chapters: false },
    Container { id: "flac", label: "FLAC", ext: "flac", muxer: "flac", video: &[], audio: &["flac"], chapters: false },
    Container { id: "m4a", label: "MPEG-4 Audio", ext: "m4a", muxer: "ipod", video: &[], audio: &["aac", "alac"], chapters: true },
    Container { id: "mp3", label: "MP3", ext: "mp3", muxer: "mp3", video: &[], audio: &["libmp3lame"], chapters: false },
    Container { id: "ogg", label: "Ogg", ext: "ogg", muxer: "ogg", video: &[], audio: &["libvorbis"], chapters: false },
    Container { id: "opus", label: "Opus", ext: "opus", muxer: "opus", video: &[], audio: &["libopus"], chapters: false },
    Container { id: "ac3", label: "AC-3", ext: "ac3", muxer: "ac3", video: &[], audio: &["ac3"], chapters: false },
    Container { id: "eac3", label: "E-AC-3", ext: "ec3", muxer: "eac3", video: &[], audio: &["eac3"], chapters: false },
];

/// A profile: (value, label, the pixel formats it takes; empty = any).
pub type Profile = (&'static str, &'static str, &'static [&'static str]);

pub struct VCodec {
    pub enc: &'static str,
    pub label: &'static str,
    /// The first is the default.
    pub pix_fmts: &'static [&'static str],
    /// Rate controls offered: 'D'efault, 'Q'uality, 'V'BR, 'C'BR.
    pub rates: &'static str,
    /// The quality scale (best, worst) for Rate::Quality.
    pub q: (u32, u32),
    /// The preset option and its values (the first is the default).
    pub preset: (&'static str, &'static [&'static str]),
    /// The profile option and its values (the first is the default).
    pub profile: (&'static str, &'static [Profile]),
    /// Writes interlaced frames.
    pub interlace: bool,
    /// Image sequence extension ("" for a video codec).
    pub ext: &'static str,
    /// Further arguments it always takes.
    pub extra: &'static [&'static str],
}

const X26X_PRESETS: &[&str] = &["medium", "ultrafast", "superfast", "veryfast", "faster", "fast", "slow", "slower", "veryslow"];
const NONE_P: (&str, &[Profile]) = ("", &[]);
pub static VCODECS: [VCodec; 15] = [
    VCodec { enc: "libx264", label: "H.264", pix_fmts: &["yuv420p", "yuv422p", "yuv444p", "yuv420p10le"], rates: "QVC", q: (0, 51), preset: ("-preset", X26X_PRESETS), profile: NONE_P, interlace: true, ext: "", extra: &[] },
    VCodec { enc: "libx265", label: "HEVC", pix_fmts: &["yuv420p", "yuv420p10le", "yuv422p10le"], rates: "QVC", q: (0, 51), preset: ("-preset", X26X_PRESETS), profile: NONE_P, interlace: false, ext: "", extra: &[] },
    VCodec { enc: "libsvtav1", label: "AV1", pix_fmts: &["yuv420p", "yuv420p10le"], rates: "QV", q: (1, 63), preset: ("-preset", &["8", "4", "5", "6", "7", "9", "10", "11", "12"]), profile: NONE_P, interlace: false, ext: "", extra: &[] },
    VCodec { enc: "libvpx-vp9", label: "VP9", pix_fmts: &["yuv420p", "yuv444p"], rates: "QVC", q: (0, 63), preset: ("-deadline", &["good", "best", "realtime"]), profile: NONE_P, interlace: false, ext: "", extra: &["-row-mt", "1"] },
    VCodec { enc: "libvpx", label: "VP8", pix_fmts: &["yuv420p"], rates: "QVC", q: (4, 63), preset: ("-deadline", &["good", "best", "realtime"]), profile: NONE_P, interlace: false, ext: "", extra: &[] },
    VCodec { enc: "mpeg2video", label: "MPEG-2", pix_fmts: &["yuv420p", "yuv422p"], rates: "QVC", q: (2, 31), preset: ("", &[]), profile: NONE_P, interlace: true, ext: "", extra: &[] },
    VCodec {
        enc: "prores_ks",
        label: "ProRes",
        pix_fmts: &["yuv422p10le", "yuv444p10le", "yuva444p10le"],
        rates: "D",
        q: (0, 0),
        preset: ("", &[]),
        profile: ("-profile:v", &[("2", "422", &["yuv422p10le"]), ("0", "422 Proxy", &["yuv422p10le"]), ("1", "422 LT", &["yuv422p10le"]), ("3", "422 HQ", &["yuv422p10le"]), ("4", "4444", &["yuv444p10le", "yuva444p10le"]), ("5", "4444 XQ", &["yuv444p10le", "yuva444p10le"])]),
        interlace: true,
        ext: "",
        extra: &["-vendor", "apl0"],
    },
    VCodec {
        enc: "dnxhd",
        label: "DNxHR",
        pix_fmts: &["yuv422p", "yuv422p10le", "yuv444p10le"],
        rates: "DC",
        q: (0, 0),
        preset: ("", &[]),
        profile: ("-profile:v", &[("dnxhr_sq", "DNxHR SQ", &["yuv422p"]), ("dnxhr_lb", "DNxHR LB", &["yuv422p"]), ("dnxhr_hq", "DNxHR HQ", &["yuv422p"]), ("dnxhr_hqx", "DNxHR HQX", &["yuv422p10le"]), ("dnxhr_444", "DNxHR 444", &["yuv444p10le"]), ("dnxhd", "DNxHD", &["yuv422p", "yuv422p10le"])]),
        // DNxHR cannot be interlaced (ffmpeg 9), and DNxHD's sizes and rates are fixed per raster.
        interlace: false,
        ext: "",
        extra: &[],
    },
    VCodec {
        enc: "cfhd",
        label: "CineForm",
        pix_fmts: &["yuv422p10le", "gbrp12le", "gbrap12le"],
        rates: "D",
        q: (0, 0),
        preset: ("", &[]),
        profile: ("-quality", &[("film1", "Film Scan 1", &[]), ("film3+", "Film Scan 3+", &[]), ("film2", "Film Scan 2", &[]), ("high", "High", &[]), ("medium", "Medium", &[]), ("low", "Low", &[])]),
        interlace: false,
        ext: "",
        extra: &[],
    },
    VCodec { enc: "ffv1", label: "FFV1", pix_fmts: &["bgr0", "bgra", "yuv422p10le", "yuv444p10le"], rates: "D", q: (0, 0), preset: ("", &[]), profile: NONE_P, interlace: false, ext: "", extra: &["-level", "3", "-g", "1", "-slicecrc", "1"] },
    VCodec { enc: "mjpeg", label: "MJPEG", pix_fmts: &["yuvj422p", "yuvj420p", "yuvj444p"], rates: "Q", q: (2, 31), preset: ("", &[]), profile: NONE_P, interlace: false, ext: "", extra: &[] },
    VCodec { enc: "png", label: "PNG", pix_fmts: &["rgb24", "rgba"], rates: "D", q: (0, 0), preset: ("", &[]), profile: NONE_P, interlace: false, ext: "png", extra: &[] },
    VCodec { enc: "tiff", label: "TIFF", pix_fmts: &["rgb24", "rgba", "rgb48le"], rates: "D", q: (0, 0), preset: ("", &[]), profile: NONE_P, interlace: false, ext: "tif", extra: &[] },
    VCodec { enc: "exr", label: "OpenEXR", pix_fmts: &["gbrpf32le", "gbrapf32le"], rates: "D", q: (0, 0), preset: ("", &[]), profile: NONE_P, interlace: false, ext: "exr", extra: &[] },
    VCodec { enc: "dpx", label: "DPX", pix_fmts: &["rgb24", "rgb48le"], rates: "D", q: (0, 0), preset: ("", &[]), profile: NONE_P, interlace: false, ext: "dpx", extra: &[] },
];

pub struct ACodec {
    pub enc: &'static str,
    pub label: &'static str,
    /// Sample rates it takes, highest first (empty = any).
    pub rates: &'static [u32],
    /// Bit rates offered, kbps (empty = none: lossless).
    pub kbps: &'static [u32],
    /// Takes a bit depth (PCM, FLAC, ALAC).
    pub bits: bool,
}

pub static ACODECS: [ACodec; 10] = [
    ACodec { enc: "pcm", label: "PCM", rates: &[], kbps: &[], bits: true },
    ACodec { enc: "aac", label: "AAC", rates: &[96_000, 88_200, 64_000, 48_000, 44_100, 32_000, 24_000, 22_050, 16_000], kbps: &[128, 160, 192, 256, 320], bits: false },
    ACodec { enc: "libmp3lame", label: "MP3", rates: &[48_000, 44_100, 32_000, 24_000, 22_050, 16_000], kbps: &[128, 160, 192, 256, 320], bits: false },
    ACodec { enc: "libopus", label: "Opus", rates: &[48_000, 24_000, 16_000, 12_000, 8_000], kbps: &[64, 96, 128, 160, 192, 256], bits: false },
    ACodec { enc: "libvorbis", label: "Vorbis", rates: &[], kbps: &[96, 128, 160, 192, 256, 320], bits: false },
    ACodec { enc: "flac", label: "FLAC", rates: &[], kbps: &[], bits: true },
    ACodec { enc: "alac", label: "ALAC", rates: &[], kbps: &[], bits: true },
    ACodec { enc: "ac3", label: "AC-3", rates: &[48_000, 44_100, 32_000], kbps: &[192, 224, 256, 320, 384, 448, 640], bits: false },
    ACodec { enc: "eac3", label: "E-AC-3", rates: &[48_000, 44_100, 32_000], kbps: &[192, 256, 384, 448, 640, 768, 1024], bits: false },
    ACodec { enc: "mp2", label: "MPEG Layer 2", rates: &[48_000, 44_100, 32_000], kbps: &[192, 224, 256, 320, 384], bits: false },
];

pub fn container(id: &str) -> Option<&'static Container> {
    CONTAINERS.iter().find(|c| c.id == id)
}

pub fn vcodec(enc: &str) -> Option<&'static VCodec> {
    VCODECS.iter().find(|c| c.enc == enc)
}

pub fn acodec(enc: &str) -> Option<&'static ACodec> {
    ACODECS.iter().find(|c| c.enc == enc)
}

/// The lowest rate `allowed` (highest first) at or over `want`, else its highest: nothing is lost that the codec could
/// keep (Opus takes a 44.1 kHz project at 48 kHz, MP3 a 96 kHz one at 48 kHz). `want` itself when any rate goes.
pub fn pick_rate(allowed: &[u32], want: u32) -> u32 {
    if allowed.is_empty() { want } else { allowed.iter().rev().copied().find(|&r| r >= want).unwrap_or(allowed[0]) }
}

/// `pix_fmt` holds no chroma or luma subsampling in RGB (no YUV matrix, no range).
pub fn is_rgb(pix_fmt: &str) -> bool {
    ["rgb", "bgr", "gbr"].iter().any(|p| pix_fmt.starts_with(p))
}

impl VideoEnc {
    pub fn codec(&self) -> Option<&'static VCodec> {
        vcodec(&self.codec)
    }

    pub fn pix_fmt(&self) -> &str {
        match (self.pix_fmt.as_str(), self.codec()) {
            ("", Some(c)) => c.pix_fmts[0],
            (p, _) => p,
        }
    }

    pub fn profile(&self) -> Option<&'static Profile> {
        let c = self.codec()?;
        c.profile.1.iter().find(|p| p.0 == self.profile).or(c.profile.1.first())
    }

    /// The output size (before rotation, which the render applies to the project size it follows).
    pub fn size(&self, s: &ProjectSettings, rotate: bool) -> (u32, u32) {
        let (w, h) = (s.width.max(2) & !1, s.height.max(2) & !1);
        self.size.unwrap_or(if rotate && s.rotation % 180 == 90 { (h, w) } else { (w, h) })
    }

    pub fn fps(&self, s: &ProjectSettings) -> f64 {
        self.fps.unwrap_or(s.frame_rate)
    }

    pub fn field(&self, s: &ProjectSettings) -> FieldOrder {
        self.field.unwrap_or(s.field_order)
    }

    pub fn par(&self, s: &ProjectSettings) -> f64 {
        self.par.unwrap_or(s.par)
    }

    /// Writes an alpha channel.
    pub fn alpha(&self) -> bool {
        let p = self.pix_fmt();
        p.starts_with("yuva") || p.contains("rgba") || p.starts_with("bgra") || p.starts_with("gbrap")
    }
}

impl AudioEnc {
    pub fn codec(&self) -> Option<&'static ACodec> {
        acodec(&self.codec)
    }

    /// The sample rate rendered for a project at `project` Hz in `container`.
    pub fn sample_rate(&self, project: u32, container: &str) -> u32 {
        if container == "mxf" {
            return 48_000;
        }
        pick_rate(self.codec().map_or(&[][..], |c| c.rates), self.rate.unwrap_or(project))
    }

    pub fn bits(&self, s: &ProjectSettings) -> u8 {
        self.bits.unwrap_or(s.bit_depth)
    }
}

impl Template {
    pub fn container(&self) -> Option<&'static Container> {
        container(&self.container)
    }

    /// The output extension.
    pub fn ext(&self) -> &'static str {
        match self.container() {
            Some(c) if c.ext.is_empty() => self.video.as_ref().and_then(|v| v.codec()).map_or("png", |v| v.ext),
            Some(c) => c.ext,
            None => "",
        }
    }

    /// Frames go to numbered files, not one file.
    pub fn sequence(&self) -> bool {
        self.container == "image"
    }

    /// The format heading the VP16 tree groups templates under: "MP4 (H.264 / AAC)".
    pub fn format(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        parts.extend(self.video.as_ref().and_then(|v| v.codec()).map(|c| c.label));
        parts.extend(self.audio.as_ref().and_then(|a| a.codec()).map(|c| c.label));
        let c = self.container().map_or("?", |c| c.label);
        if parts.is_empty() || (parts.len() == 1 && parts[0] == c) { c.to_string() } else { format!("{c} ({})", parts.join(" / ")) }
    }

    /// The template's place in the VP23 Usecase tree: 0 YouTube, 1 Social Media, 2 Web & Streaming, 3 Video VFX &
    /// Master, 4 Image Compositing, 5 DVD & Blu-ray, 6 Audio Mix & Master, 7 Podcast & Audio Streaming.
    pub fn usecase(&self) -> usize {
        let Some(v) = &self.video else {
            return if self.audio.as_ref().is_some_and(|a| a.codec().is_some_and(|c| c.bits)) { 6 } else { 7 };
        };
        match v.codec.as_str() {
            _ if self.sequence() => 4,
            "prores_ks" | "dnxhd" | "cfhd" | "ffv1" => 3,
            "mpeg2video" => 5,
            _ if self.name.contains("YouTube") => 0,
            _ if v.size.is_some_and(|(w, h)| h > w) => 1,
            _ => 2,
        }
    }

    /// Why this template cannot render, if it cannot: an unknown codec, a codec its container cannot hold, a pixel
    /// format, profile or rate control the codec does not take, an interlaced codec that cannot be, an odd size.
    pub fn check(&self) -> Result<(), String> {
        let c = self.container().ok_or_else(|| format!("Unknown format \"{}\".", self.container))?;
        if self.video.is_none() && self.audio.is_none() {
            return Err("Include video, audio or both.".into());
        }
        if let Some(v) = &self.video {
            let k = v.codec().ok_or_else(|| format!("Unknown video codec \"{}\".", v.codec))?;
            if !c.video.contains(&k.enc) {
                return Err(format!("{} cannot hold {} video.", c.label, k.label));
            }
            let pix = v.pix_fmt();
            if !k.pix_fmts.contains(&pix) {
                return Err(format!("{} does not take the pixel format {pix}.", k.label));
            }
            if !k.profile.1.is_empty() && !v.profile.is_empty() && !k.profile.1.iter().any(|p| p.0 == v.profile) {
                return Err(format!("{} has no profile \"{}\".", k.label, v.profile));
            }
            if let Some(p) = v.profile().filter(|p| !p.2.is_empty() && !p.2.contains(&pix)) {
                return Err(format!("{} {} needs the pixel format {}.", k.label, p.1, p.2.join(" or ")));
            }
            let kind = match v.rate {
                Rate::Default => 'D',
                Rate::Quality(q) if q < k.q.0 || q > k.q.1 => return Err(format!("{} quality goes from {} to {}.", k.label, k.q.0, k.q.1)),
                Rate::Quality(_) => 'Q',
                Rate::Vbr { avg, max } if avg == 0 || max < avg => return Err("The maximum bit rate must be at or over the average.".into()),
                Rate::Vbr { .. } => 'V',
                Rate::Cbr(0) => return Err("Type a bit rate.".into()),
                Rate::Cbr(_) => 'C',
            };
            if !k.rates.contains(kind) {
                return Err(format!("{} does not take this rate control.", k.label));
            }
            if v.profile().is_some_and(|p| p.0 == "dnxhd") && !matches!(v.rate, Rate::Cbr(_)) {
                return Err("DNxHD needs a constant bit rate (36 to 440 Mbps, by frame size and rate).".into());
            }
            if v.field.is_some_and(|f| f != FieldOrder::Progressive) && !k.interlace {
                return Err(format!("{} cannot be interlaced.", k.label));
            }
            if let Some((w, h)) = v.size {
                if !(16..=8192).contains(&w) || !(16..=8192).contains(&h) {
                    return Err("The frame size goes from 16 to 8192.".into());
                }
                let sub = !is_rgb(pix) && !pix.contains("444") && pix != "bgr0";
                if sub && (w % 2 == 1 || (pix.contains("420") && h % 2 == 1)) {
                    return Err(format!("{pix} needs an even frame size."));
                }
            }
            if v.fps.is_some_and(|f| !(1.0..=240.0).contains(&f)) || v.par.is_some_and(|p| !(0.1..=10.0).contains(&p)) {
                return Err("The frame rate goes from 1 to 240 and the pixel aspect ratio from 0.1 to 10.".into());
            }
        }
        if let Some(a) = &self.audio {
            let k = a.codec().ok_or_else(|| format!("Unknown audio codec \"{}\".", a.codec))?;
            if !c.audio.contains(&k.enc) {
                return Err(if c.audio.is_empty() { format!("{} has no audio.", c.label) } else { format!("{} cannot hold {} audio.", c.label, k.label) });
            }
            if !(1..=2).contains(&a.channels) {
                return Err("Mono or stereo.".into());
            }
            if a.bits.is_some_and(|b| ![8, 16, 24, 32].contains(&b)) {
                return Err("The bit depth is 8, 16, 24 or 32.".into());
            }
            if !k.kbps.is_empty() && a.kbps == 0 {
                return Err("Choose an audio bit rate.".into());
            }
        }
        Ok(())
    }

    /// Encoder arguments for the video stream: codec, rate control, preset, profile, pixel format, interlacing.
    pub fn video_args(&self, s: &ProjectSettings) -> Vec<String> {
        let Some(v) = &self.video else { return Vec::new() };
        let Some(k) = v.codec() else { return Vec::new() };
        let mut a: Vec<String> = vec!["-c:v".into(), k.enc.into()];
        let push = |a: &mut Vec<String>, xs: &[&str]| a.extend(xs.iter().map(|x| x.to_string()));
        match (v.rate, k.enc) {
            (Rate::Quality(q), "mpeg2video" | "mjpeg") => push(&mut a, &["-q:v", &q.to_string()]),
            // libvpx's CRF is capped by -b:v: 0 lifts the cap on VP9; VP8 needs one.
            (Rate::Quality(q), "libvpx-vp9") => push(&mut a, &["-crf", &q.to_string(), "-b:v", "0"]),
            (Rate::Quality(q), "libvpx") => push(&mut a, &["-crf", &q.to_string(), "-b:v", "50M"]),
            (Rate::Quality(q), _) => push(&mut a, &["-crf", &q.to_string()]),
            (Rate::Vbr { avg, max }, _) => push(&mut a, &["-b:v", &format!("{avg}k"), "-maxrate", &format!("{max}k"), "-bufsize", &format!("{}k", max * 2)]),
            (Rate::Cbr(r), "dnxhd") => push(&mut a, &["-b:v", &format!("{r}k")]),
            (Rate::Cbr(r), _) => push(&mut a, &["-b:v", &format!("{r}k"), "-minrate", &format!("{r}k"), "-maxrate", &format!("{r}k"), "-bufsize", &format!("{r}k")]),
            (Rate::Default, _) => {}
        }
        if !k.preset.0.is_empty() {
            let p = if v.preset.is_empty() { k.preset.1[0] } else { v.preset.as_str() };
            push(&mut a, &[k.preset.0, p]);
        }
        if let Some(p) = v.profile() {
            push(&mut a, &[k.profile.0, p.0]);
        }
        push(&mut a, &["-pix_fmt", v.pix_fmt()]);
        push(&mut a, k.extra);
        if k.enc == "libx265" && matches!(self.container.as_str(), "mp4" | "mov") {
            push(&mut a, &["-tag:v", "hvc1"]);
        }
        match v.field(s) {
            FieldOrder::Progressive => {}
            f => {
                let top = f == FieldOrder::Upper;
                push(&mut a, &["-flags", "+ildct+ilme", "-field_order", if top { "tt" } else { "bb" }]);
                if k.enc == "libx264" {
                    push(&mut a, &["-x264-params", if top { "tff=1" } else { "bff=1" }]);
                }
            }
        }
        a
    }

    /// Encoder arguments for the audio stream: codec (PCM's sample format from the bit depth and container), bit rate,
    /// sample rate and channels.
    pub fn audio_args(&self, s: &ProjectSettings) -> Vec<String> {
        let Some(au) = &self.audio else { return Vec::new() };
        let Some(k) = au.codec() else { return Vec::new() };
        let bits = au.bits(s);
        let mxf = self.container == "mxf";
        let mut a: Vec<String> = Vec::new();
        let mut push = |xs: &[&str]| a.extend(xs.iter().map(|x| x.to_string()));
        match k.enc {
            "pcm" => {
                let be = self.container == "aiff";
                let fmt = match (bits, be) {
                    // MXF takes 16 and 24-bit PCM only.
                    (8, _) if mxf => "pcm_s16le",
                    (32, _) if mxf => "pcm_s24le",
                    (8, false) => "pcm_u8",
                    (8, true) => "pcm_s8",
                    (24, false) => "pcm_s24le",
                    (24, true) => "pcm_s24be",
                    (32, false) => "pcm_f32le",
                    (32, true) => "pcm_f32be",
                    (_, false) => "pcm_s16le",
                    (_, true) => "pcm_s16be",
                };
                push(&["-c:a", fmt]);
            }
            "flac" | "alac" => {
                let planar = if k.enc == "alac" { "p" } else { "" };
                push(&["-c:a", k.enc]);
                if bits <= 16 {
                    push(&["-sample_fmt", &format!("s16{planar}")]);
                } else {
                    push(&["-sample_fmt", &format!("s32{planar}"), "-bits_per_raw_sample", "24"]);
                }
            }
            enc => push(&["-c:a", enc, "-b:a", &format!("{}k", au.kbps)]),
        }
        push(&["-ar", &au.sample_rate(s.sample_rate, &self.container).to_string(), "-ac", &au.channels.to_string()]);
        a
    }

    /// One line of video settings for the description box.
    pub fn video_line(&self, s: &ProjectSettings, rotate: bool) -> Option<String> {
        let v = self.video.as_ref()?;
        let (w, h) = v.size(s, rotate);
        let field = match v.field(s) {
            FieldOrder::Progressive => "Progressive".to_string(),
            f => f.name().to_string(),
        };
        let rate = match v.rate {
            Rate::Default => String::new(),
            Rate::Quality(q) => format!("; quality {q}"),
            Rate::Vbr { avg, max } => format!("; VBR {:.1} Mbps (max {:.1})", avg as f64 / 1000.0, max as f64 / 1000.0),
            Rate::Cbr(r) => format!("; CBR {:.1} Mbps", r as f64 / 1000.0),
        };
        let codec = v.codec().map_or(v.codec.as_str(), |c| c.label);
        let codec = match v.profile() {
            Some(p) if p.1.starts_with(codec) => p.1.to_string(),
            Some(p) => format!("{codec} {}", p.1),
            None => codec.to_string(),
        };
        Some(format!("{w}x{h}; {:.3} fps; {field}; PAR {:.4}; {codec}; {}{rate}", v.fps(s), v.par(s), v.pix_fmt()))
    }

    /// One line of audio settings for the description box.
    pub fn audio_line(&self, s: &ProjectSettings) -> Option<String> {
        let a = self.audio.as_ref()?;
        let k = a.codec()?;
        let ch = if a.channels == 1 { "Mono" } else { "Stereo" };
        let q = if k.bits { format!("{}-bit", a.bits(s)) } else { format!("{} kbps", a.kbps) };
        Some(format!("{} Hz; {ch}; {}; {q}", a.sample_rate(s.sample_rate, &self.container), k.label))
    }

    /// Follows the project's video settings (Filter › Match project video settings): its size, rate, field order and
    /// pixel aspect ratio are the project's or unset.
    pub fn matches_video(&self, s: &ProjectSettings) -> bool {
        self.video.as_ref().is_none_or(|v| {
            v.size.is_none_or(|z| z == (s.width, s.height))
                && v.fps.is_none_or(|f| (f - s.frame_rate).abs() < 0.001)
                && v.field.is_none_or(|f| f == s.field_order)
                && v.par.is_none_or(|p| (p - s.par).abs() < 0.0001)
        })
    }

    /// Follows the project's audio settings: its sample rate and bit depth.
    pub fn matches_audio(&self, s: &ProjectSettings) -> bool {
        self.audio.as_ref().is_none_or(|a| a.sample_rate(s.sample_rate, &self.container) == s.sample_rate && (!a.codec().is_some_and(|c| c.bits) || a.bits(s) == s.bit_depth))
    }
}

fn tpl(name: &str, notes: &str, container: &str, video: Option<VideoEnc>, audio: Option<AudioEnc>) -> Template {
    Template { name: name.into(), notes: notes.into(), container: container.into(), video, audio, quality: None }
}

fn vid(codec: &str, size: Option<(u32, u32)>, rate: Rate, profile: &str, pix_fmt: &str) -> Option<VideoEnc> {
    Some(VideoEnc { codec: codec.into(), size, rate, profile: profile.into(), pix_fmt: pix_fmt.into(), ..Default::default() })
}

fn aud(codec: &str, kbps: u32) -> Option<AudioEnc> {
    Some(AudioEnc { codec: codec.into(), kbps, ..Default::default() })
}

/// The built-in delivery templates. The first five are Milan Pro's original presets, unchanged.
pub static BUILTIN: LazyLock<Vec<Template>> = LazyLock::new(|| {
    use Rate::*;
    let (hd, uhd, p720) = (Some((1920, 1080)), Some((3840, 2160)), Some((1280, 720)));
    let x264 = |size, rate| vid("libx264", size, rate, "", "");
    vec![
        tpl("MP4 H.264 (AAC)", "Use this setting to create a high-quality file at the project size, suited to uploading and playback.", "mp4", x264(None, Quality(18)), aud("aac", 320)),
        tpl("MP4 H.265 (HEVC)", "High-efficiency video at the project size: smaller files for the same quality.", "mp4", vid("libx265", None, Quality(22), "", ""), aud("aac", 320)),
        tpl("MOV ProRes 422", "Intermediate-quality master for further editing and grading.", "mov", vid("prores_ks", None, Default, "2", ""), aud("pcm", 0)),
        tpl("WAV audio only", "Uncompressed stereo audio mix at the project's bit depth.", "wav", None, aud("pcm", 0)),
        tpl("MP3 audio only", "Compressed 320 kbps stereo audio mix.", "mp3", None, aud("libmp3lame", 320)),
        tpl("YouTube 1080p", "1920x1080 H.264 at 16 Mbps for upload to YouTube.", "mp4", x264(hd, Vbr { avg: 16_000, max: 24_000 }), aud("aac", 320)),
        tpl("YouTube 2160p", "3840x2160 (4K) H.264 at 45 Mbps for upload to YouTube.", "mp4", x264(uhd, Vbr { avg: 45_000, max: 68_000 }), aud("aac", 320)),
        tpl("Vimeo 1080p", "1920x1080 H.264 at 20 Mbps for upload to Vimeo.", "mp4", x264(hd, Vbr { avg: 20_000, max: 30_000 }), aud("aac", 320)),
        tpl("Web 720p", "1280x720 H.264 at 6 Mbps for web pages and streaming.", "mp4", x264(p720, Vbr { avg: 6_000, max: 9_000 }), aud("aac", 192)),
        tpl("Social Vertical 1080x1920", "Portrait 1080x1920 H.264 for phone-first social video.", "mp4", x264(Some((1080, 1920)), Vbr { avg: 12_000, max: 18_000 }), aud("aac", 256)),
        tpl("ProRes 422 HQ", "High-quality 10-bit 4:2:2 master.", "mov", vid("prores_ks", None, Default, "3", ""), aud("pcm", 0)),
        tpl("ProRes 4444 (alpha)", "10-bit 4:4:4 master with an alpha channel: transparent where no track covers the frame.", "mov", vid("prores_ks", None, Default, "4", "yuva444p10le"), aud("pcm", 0)),
        tpl("DNxHR LB", "Avid DNxHR Low Bandwidth (8-bit 4:2:2) for offline editing.", "mov", vid("dnxhd", None, Default, "dnxhr_lb", "yuv422p"), aud("pcm", 0)),
        tpl("DNxHR SQ", "Avid DNxHR Standard Quality (8-bit 4:2:2).", "mov", vid("dnxhd", None, Default, "dnxhr_sq", "yuv422p"), aud("pcm", 0)),
        tpl("DNxHR HQ", "Avid DNxHR High Quality (8-bit 4:2:2).", "mov", vid("dnxhd", None, Default, "dnxhr_hq", "yuv422p"), aud("pcm", 0)),
        tpl("DNxHR HQX", "Avid DNxHR High Quality 10-bit 4:2:2 master.", "mov", vid("dnxhd", None, Default, "dnxhr_hqx", "yuv422p10le"), aud("pcm", 0)),
        tpl("DNxHR 444", "Avid DNxHR 10-bit 4:4:4 master.", "mov", vid("dnxhd", None, Default, "dnxhr_444", "yuv444p10le"), aud("pcm", 0)),
        tpl("MXF DNxHR HQ", "Avid DNxHR HQ in MXF (OP1a), 48 kHz PCM.", "mxf", vid("dnxhd", None, Default, "dnxhr_hq", "yuv422p"), aud("pcm", 0)),
        tpl("CineForm", "GoPro CineForm 10-bit 4:2:2 intermediate.", "mov", vid("cfhd", None, Default, "film1", "yuv422p10le"), aud("pcm", 0)),
        tpl("FFV1 MKV archive", "Lossless FFV1 (RGB) with FLAC audio in Matroska, for archiving.", "mkv", vid("ffv1", None, Default, "", "bgr0"), aud("flac", 0)),
        tpl("VP9 WebM", "VP9 with Opus audio in WebM for the web.", "webm", vid("libvpx-vp9", None, Quality(31), "", ""), aud("libopus", 192)),
        tpl("VP8 WebM", "VP8 with Vorbis audio in WebM for older browsers.", "webm", vid("libvpx", None, Quality(10), "", ""), aud("libvorbis", 192)),
        tpl("AV1 (SVT)", "AV1 (SVT-AV1) with AAC audio in MP4: small files for modern players.", "mp4", vid("libsvtav1", None, Quality(30), "", ""), aud("aac", 256)),
        tpl("MPEG-2 TS", "MPEG-2 video with AC-3 audio in a transport stream (.m2ts) for broadcast and Blu-ray authoring.", "ts", vid("mpeg2video", None, Vbr { avg: 15_000, max: 25_000 }, "", ""), aud("ac3", 384)),
        tpl("MPEG-2 Program Stream", "MPEG-2 video with AC-3 audio in a program stream (.mpg) for DVD authoring.", "mpg", vid("mpeg2video", None, Vbr { avg: 8_000, max: 9_500 }, "", ""), aud("ac3", 384)),
        tpl("AVI (MJPEG)", "Motion JPEG with PCM audio in AVI.", "avi", vid("mjpeg", None, Quality(3), "", ""), aud("pcm", 0)),
        tpl("PNG sequence", "One PNG image per frame.", "image", vid("png", None, Default, "", "rgb24"), None),
        tpl("TIFF sequence", "One TIFF image per frame.", "image", vid("tiff", None, Default, "", "rgb24"), None),
        tpl("OpenEXR sequence", "One OpenEXR (32-bit float) image per frame.", "image", vid("exr", None, Default, "", "gbrpf32le"), None),
        tpl("DPX sequence", "One DPX image per frame.", "image", vid("dpx", None, Default, "", "rgb24"), None),
        tpl("FLAC audio", "Lossless compressed audio at the project's bit depth.", "flac", None, aud("flac", 0)),
        tpl("ALAC audio", "Apple Lossless audio (.m4a).", "m4a", None, aud("alac", 0)),
        tpl("AAC audio", "AAC 256 kbps audio (.m4a).", "m4a", None, aud("aac", 256)),
        tpl("Opus audio", "Opus 160 kbps audio at 48 kHz.", "opus", None, aud("libopus", 160)),
        tpl("Ogg Vorbis audio", "Vorbis 192 kbps audio (.ogg).", "ogg", None, aud("libvorbis", 192)),
        tpl("AC-3 audio", "Dolby Digital AC-3 448 kbps audio.", "ac3", None, aud("ac3", 448)),
        tpl("E-AC-3 audio", "Dolby Digital Plus E-AC-3 640 kbps audio.", "eac3", None, aud("eac3", 640)),
        tpl("AIFF audio", "Uncompressed big-endian PCM audio (.aif).", "aiff", None, aud("pcm", 0)),
    ]
});

/// Render As › Render Options, and Options › Quickfade Audio Edits as playback has it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Opts {
    pub quickfade: bool,
    /// Stretch video to fill output frame size (do not letterbox).
    pub stretch: bool,
    /// Use project output rotation setting.
    pub rotate: bool,
    /// Save project markers in media file (formats that hold chapters).
    pub markers: bool,
}

impl Default for Opts {
    fn default() -> Self {
        Self { quickfade: true, stretch: false, rotate: true, markers: false }
    }
}

/// Where the composed picture goes in the output frame. The project frame (at its pixel aspect ratio, turned by
/// `rot` degrees) keeps its display shape inside the output frame (at its own pixel aspect ratio), centred with black
/// bars, unless `stretch` fills the frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    /// The size to compose at (before the rotation).
    pub compose: (u32, u32),
    /// The rotated picture's size and top-left in the output frame.
    pub inner: (u32, u32),
    pub at: (u32, u32),
    pub out: (u32, u32),
}

pub fn fit(s: &ProjectSettings, rot: u16, out: (u32, u32), out_par: f64, stretch: bool) -> Fit {
    let (pw, ph) = (s.width.max(2) & !1, s.height.max(2) & !1);
    let quarter = rot % 180 == 90;
    let shape = pw as f64 * s.par.max(0.01) / ph as f64;
    let a = if quarter { 1.0 / shape } else { shape };
    let (w, h) = out;
    let oa = w as f64 * out_par / h as f64;
    let inner = if stretch {
        (w, h)
    } else if a > oa + 1e-9 {
        (w, ((w as f64 * out_par / a).round() as u32).clamp(1, h))
    } else {
        (((h as f64 * a / out_par).round() as u32).clamp(1, w), h)
    };
    let compose = if quarter { (inner.1, inner.0) } else { inner };
    Fit { compose, inner, at: ((w - inner.0) / 2, (h - inner.1) / 2), out }
}

/// `t` as "2026-11-12 14:03:09" (UTC) for a render log.
pub fn utc(t: std::time::SystemTime) -> String {
    let secs = t.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + (m <= 2) as i64;
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}", rem / 3600, rem / 60 % 60, rem % 60)
}

// ---- Chapters and logs ----

/// Save project markers in media file: an ffmetadata file with a chapter per marker and region in `range`, times from
/// its start. A chapter runs to the next one's start (a region to its end, if sooner) or the range's end. None
/// without markers or regions there.
pub fn chapters(p: &Project, (a, b): (f64, f64)) -> Option<String> {
    let tl = &p.timeline;
    let mut cs: Vec<(f64, Option<f64>, String)> = tl.markers.iter().map(|m| (m.position, None, m.name.clone())).collect();
    cs.extend(tl.regions.iter().map(|r| (r.start, Some(r.end), r.name.clone())));
    cs.retain(|c| c.0 >= a && c.0 < b);
    cs.sort_by(|x, y| x.0.total_cmp(&y.0));
    if cs.is_empty() {
        return None;
    }
    let esc = |s: &str| s.chars().flat_map(|c| if "=;#\\\n".contains(c) { vec!['\\', c] } else { vec![c] }).collect::<String>();
    let ms = |t: f64| ((t - a) * 1000.0).round() as i64;
    let mut out = String::from(";FFMETADATA1\n");
    for (i, (t, end, name)) in cs.iter().enumerate() {
        let next = cs.get(i + 1).map_or(b, |n| n.0.max(*t));
        let e = end.map_or(next, |e| e.min(next).max(*t));
        let title = if name.is_empty() { format!("Marker {}", i + 1) } else { esc(name) };
        out += &format!("[CHAPTER]\nTIMEBASE=1/1000\nSTART={}\nEND={}\ntitle={title}\n", ms(*t), ms(e));
    }
    Some(out)
}

/// `%LOCALAPPDATA%\MilanPro\logs\render`, where every render leaves its log.
pub fn log_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map_or_else(std::env::temp_dir, PathBuf::from).join("MilanPro").join("logs").join("render")
}

/// The exact rate for ffmpeg (`project::rate_ratio`): 29.97 renders as 30000/1001, not 2997/100.
pub fn fps_str(fps: f64) -> String {
    match crate::project::rate_ratio(fps) {
        (n, 1) => n.to_string(),
        (n, d) => format!("{n}/{d}"),
    }
}

/// End of the last clip on any track.
pub fn content_end(project: &Project) -> f64 {
    let tl = &project.timeline;
    tl.video_tracks.iter().chain(&tl.audio_tracks).flat_map(|t| &t.clips).map(|c| c.position + c.duration).fold(0.0, f64::max)
}

// ---- Job ----

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Status {
    #[default]
    Running,
    Done,
    Failed(String),
    Cancelled,
}

/// What the render thread shares with the dialog's `Job`: progress, the encoder (so Cancel can kill it) and the cancel flag.
pub struct Ctl {
    /// (fraction done, status)
    state: Mutex<(f64, Status)>,
    child: Mutex<Option<Child>>,
    cancel: Arc<AtomicBool>,
    /// Files the render deletes when it stops short; `Job::drop` deletes them itself if the render thread is stuck.
    temp: Mutex<Vec<PathBuf>>,
    /// The render's log file, once written.
    log: Mutex<Option<PathBuf>>,
    ctx: egui::Context,
}

impl Ctl {
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Relaxed)
    }

    /// The cancel flag, for work that waits outside the render thread's own checks (the mixer's decoders).
    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }

    pub fn set_log(&self, path: PathBuf) {
        *self.log.lock().unwrap() = Some(path);
    }

    pub fn set_temp(&self, files: Vec<PathBuf>) {
        *self.temp.lock().unwrap() = files;
    }

    pub fn progress(&self, fraction: f64) {
        self.state.lock().unwrap().0 = fraction;
        self.ctx.request_repaint();
    }

    /// Hold the encoder so Cancel can kill it (at once, if Cancel came first).
    pub fn set_child(&self, c: Child) {
        *self.child.lock().unwrap() = Some(c);
        if self.cancelled() {
            self.kill();
        }
    }

    pub fn take_child(&self) -> Option<Child> {
        self.child.lock().unwrap().take()
    }

    pub fn kill(&self) {
        if let Some(c) = self.child.lock().unwrap().as_mut() {
            let _ = c.kill();
        }
    }
}

pub struct Job {
    pub out: PathBuf,
    started: Instant,
    ctl: Arc<Ctl>,
}

impl Job {
    /// Run `work` on a "milan-render" thread. Ok is Done; an error is Cancelled once Cancel was pressed, else Failed.
    pub fn start(out: PathBuf, ctx: egui::Context, work: impl FnOnce(&Ctl) -> Result<(), String> + Send + 'static) -> Job {
        let ctl = Arc::new(Ctl { state: Mutex::default(), child: Mutex::default(), cancel: Arc::default(), temp: Mutex::default(), log: Mutex::default(), ctx });
        let c = ctl.clone();
        std::thread::Builder::new()
            .name("milan-render".into())
            .spawn(move || {
                let status = match work(&c) {
                    Ok(()) => Status::Done,
                    Err(_) if c.cancelled() => Status::Cancelled,
                    Err(e) => Status::Failed(e),
                };
                c.state.lock().unwrap().1 = status;
                c.ctx.request_repaint();
            })
            .expect("spawn milan-render");
        Job { out, started: Instant::now(), ctl }
    }

    pub fn status(&self) -> Status {
        self.ctl.state.lock().unwrap().1.clone()
    }

    pub fn fraction(&self) -> f32 {
        self.ctl.state.lock().unwrap().0.clamp(0.0, 1.0) as f32
    }

    /// Remaining seconds, once there is enough progress to estimate.
    pub fn eta(&self) -> Option<f64> {
        let f = self.fraction() as f64;
        (f > 0.01).then(|| self.started.elapsed().as_secs_f64() * (1.0 - f) / f)
    }

    /// The render's log (`render::log_dir`), once the render has ended.
    pub fn log(&self) -> Option<PathBuf> {
        self.ctl.log.lock().unwrap().clone()
    }

    pub fn cancel(&self) {
        self.ctl.cancel.store(true, Relaxed);
        self.ctl.kill();
    }
}

/// std never kills a Child on drop, so an unfinished render must be cancelled here or it keeps encoding invisibly
/// after exit. Drop runs as Milan closes, so it waits (briefly: the killed ffmpeg exits at once) for the render
/// thread to delete the partial output and the temp audio before the process ends and takes the thread with it.
/// A thread still busy after that (a slow first frame from a decoder) cannot clean up in time, so Drop does it.
impl Drop for Job {
    fn drop(&mut self) {
        if self.status() == Status::Running {
            self.cancel();
            let until = Instant::now() + std::time::Duration::from_secs(2);
            while self.status() == Status::Running && Instant::now() < until {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            if self.status() == Status::Running {
                // The killed encoder must have exited, or Windows keeps the partial output locked.
                if let Some(mut c) = self.ctl.take_child() {
                    let _ = c.wait();
                }
                for f in self.ctl.temp.lock().unwrap().iter() {
                    // An image sequence's frames are staged in a folder.
                    let _ = if f.is_dir() { std::fs::remove_dir_all(f) } else { std::fs::remove_file(f) };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Markers and regions in the range become chapters from its start; names are escaped; nothing without any.
    #[test]
    fn chapters_from_markers_and_regions() {
        let mut p = Project::new("c");
        assert_eq!(chapters(&p, (0.0, 10.0)), None);
        let m = |t: f64, n: &str| crate::project::Marker { position: t, name: n.into(), color: [0; 3] };
        p.timeline.markers = vec![m(4.0, "Verse; 2 = #1"), m(1.0, ""), m(20.0, "after")];
        p.timeline.regions = vec![crate::project::Region { start: 2.0, end: 3.0, name: "Solo".into() }];
        let c = chapters(&p, (1.0, 10.0)).unwrap();
        let chapter = |a: u32, b: u32, t: &str| format!("[CHAPTER]\nTIMEBASE=1/1000\nSTART={a}\nEND={b}\ntitle={t}\n");
        let want = format!(";FFMETADATA1\n{}{}{}", chapter(0, 1000, "Marker 1"), chapter(1000, 2000, "Solo"), chapter(3000, 9000, r"Verse\; 2 \= \#1"));
        assert_eq!(c, want);
    }

    #[test]
    fn rates_codecs_and_dates() {
        assert_eq!((pick_rate(&[48_000, 24_000], 44_100), pick_rate(&[48_000, 44_100], 96_000), pick_rate(&[], 22_050)), (48_000, 48_000, 22_050));
        assert_eq!(utc(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_794_484_989)), "2026-11-12 12:03:09");
        let mut t = BUILTIN[0].clone();
        t.container = "mov".into();
        t.video.as_mut().unwrap().codec = "libvpx-vp9".into();
        assert_eq!(t.check(), Err("QuickTime cannot hold VP9 video.".into()), "VP9 in MOV is refused");
        let mut t = BUILTIN.iter().find(|t| t.name == "ProRes 422 HQ").unwrap().clone();
        t.video.as_mut().unwrap().pix_fmt = "yuva444p10le".into();
        assert!(t.check().unwrap_err().contains("422 HQ needs the pixel format yuv422p10le"));
        let mut t = BUILTIN.iter().find(|t| t.name == "WAV audio only").unwrap().clone();
        t.audio.as_mut().unwrap().codec = "aac".into();
        assert_eq!(t.check(), Err("Wave cannot hold AAC audio.".into()));
        let mut t = BUILTIN[0].clone();
        t.video.as_mut().unwrap().size = Some((1279, 720));
        assert_eq!(t.check(), Err("yuv420p needs an even frame size.".into()));
        assert_eq!(BUILTIN.iter().map(|t| t.usecase()).collect::<std::collections::BTreeSet<_>>().len(), 8, "every Usecase node has templates");
    }

    #[test]
    fn fps_strings() {
        assert_eq!(fps_str(29.97), "30000/1001");
        assert_eq!(fps_str(23.976), "24000/1001");
        assert_eq!(fps_str(59.94), "60000/1001");
        assert_eq!(fps_str(25.0), "25");
        assert_eq!(fps_str(24.0), "24");
    }

    #[test]
    fn job_status_follows_the_work_and_cancel() {
        let wait = |j: &Job| {
            while j.status() == Status::Running {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            j.status()
        };
        let ctx = egui::Context::default();
        let j = Job::start("o".into(), ctx.clone(), |c| {
            c.progress(0.5);
            Ok(())
        });
        assert_eq!((wait(&j), j.fraction()), (Status::Done, 0.5));
        assert_eq!(wait(&Job::start("o".into(), ctx.clone(), |_| Err("no".into()))), Status::Failed("no".into()));
        let j = Job::start("o".into(), ctx, |c| {
            while !c.cancelled() {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Err("stopped".into())
        });
        j.cancel();
        assert_eq!(wait(&j), Status::Cancelled);
    }

    #[test]
    fn drop_deletes_the_files_of_a_render_stuck_past_its_wait() {
        let f = std::env::temp_dir().join(format!("milan-job-drop-{}.partial", std::process::id()));
        std::fs::write(&f, b"x").unwrap();
        let (g, (tx, rx)) = (f.clone(), std::sync::mpsc::channel());
        let j = Job::start("o".into(), egui::Context::default(), move |c| {
            c.set_temp(vec![g]);
            tx.send(()).unwrap();
            std::thread::sleep(std::time::Duration::from_secs(3)); // a slow decoder: Cancel is not seen in time
            Err("late".into())
        });
        rx.recv().unwrap();
        drop(j);
        assert!(!f.exists());
    }
}
