//! Screenshot + demo harness, driven by environment variables:
//! - `MILAN_LOOK` = vegas16 | vegas23 | vegas23-white: look for this run (not persisted).
//! - `MILAN_DEMO=1`: demo project on generated media (cached in %TEMP%\milan-demo); no autosave or prefs writes.
//! - `MILAN_SCREENSHOT=<file.png>`: 1920×1080 px unfocused window, wait for frames to settle, save, exit.
//! - `MILAN_PANELS=explorer,trimmer,…`: dock windows to bring to the front (names without spaces, any case).
use eframe::egui::{self, ColorImage};
use std::path::{Path, PathBuf};
use std::process::Command;
use crate::media::probe_media;
use crate::project::{Clip, MediaItem, Project, Track, TrackKind};
use crate::ui::ClipRef;
use crate::ui::theme::{LookChoice, Theme};

pub struct Launch {
    pub look: Option<LookChoice>,
    pub demo: bool,
    pub screenshot: Option<PathBuf>,
    pub panels: Vec<String>,
}

impl Launch {
    pub fn from_env() -> Self {
        let var = |k| std::env::var(k).ok().filter(|v| !v.is_empty());
        Self {
            look: var("MILAN_LOOK").and_then(|v| LookChoice::parse(&v)),
            demo: var("MILAN_DEMO").is_some_and(|v| v != "0"),
            screenshot: var("MILAN_SCREENSHOT").map(PathBuf::from),
            panels: var("MILAN_PANELS").map(|v| v.split(',').map(|s| s.trim().to_lowercase()).collect()).unwrap_or_default(),
        }
    }
}

/// Lavfi sources for the demo media: (file name, ffmpeg input/output args).
const MEDIA: [(&str, &[&str]); 5] = [
    ("testsrc2.mp4", &[
        "-f", "lavfi", "-i", "testsrc2=size=1920x1080:rate=30000/1001:duration=10",
        "-f", "lavfi", "-i", "sine=frequency=440:beep_factor=4:duration=10",
        "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", "-c:a", "aac", "-ac", "2", "-shortest",
    ]),
    ("smptebars.mp4", &[
        "-f", "lavfi", "-i", "smptehdbars=size=1920x1080:rate=30000/1001:duration=8",
        "-f", "lavfi", "-i", "sine=frequency=1000:duration=8",
        "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", "-c:a", "aac", "-ac", "2", "-shortest",
    ]),
    ("color.mp4", &[
        "-f", "lavfi", "-i", "color=c=0x2a5d8f:size=1920x1080:rate=30000/1001:duration=6",
        "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p",
    ]),
    // Stereo, with different L/R envelopes, so the two stacked channel waveforms differ.
    ("tone.wav", &["-f", "lavfi", "-i", "aevalsrc=0.8*sin(2*PI*220*t)*abs(sin(PI*t/3))|0.6*sin(2*PI*330*t)*abs(cos(PI*t/4)):s=48000:d=12"]),
    ("still.png", &["-f", "lavfi", "-i", "testsrc=size=1920x1080:rate=1", "-frames:v", "1"]),
];

/// Folder of the generated demo media.
pub fn media_dir() -> PathBuf {
    std::env::temp_dir().join("milan-demo")
}

/// Generate the demo media once; a `ready2` marker means every file of this media set was written.
fn ensure_media() -> anyhow::Result<PathBuf> {
    let dir = media_dir();
    if dir.join("ready2").exists() {
        return Ok(dir);
    }
    std::fs::create_dir_all(&dir)?;
    for (name, args) in MEDIA {
        let mut cmd = Command::new("ffmpeg");
        cmd.args(["-y", "-hide_banner", "-loglevel", "error"]).args(args).arg(dir.join(name));
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        let out = cmd.output().map_err(|e| anyhow::anyhow!("ffmpeg: {e}"))?;
        anyhow::ensure!(out.status.success(), "ffmpeg {name}: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    std::fs::write(dir.join("ready2"), "")?;
    Ok(dir)
}

/// 3 video + 2 audio tracks with trimmed events and markers; Video 2's first event selected.
pub fn project(theme: &Theme) -> anyhow::Result<(Project, ClipRef)> {
    let dir = ensure_media()?;
    let probe = |n: &str| probe_media(&dir.join(n));
    let (src, bars, color, tone, still) =
        (probe("testsrc2.mp4")?, probe("smptebars.mp4")?, probe("color.mp4")?, probe("tone.wav")?, probe("still.png")?);

    let mut p = Project::new("Milan Demo");
    let tl = &mut p.timeline;
    tl.video_tracks = (1..=3).map(|n| Track::new(&format!("Video {n}"), TrackKind::Video)).collect();
    tl.audio_tracks = (1..=2).map(|n| Track::new(&format!("Audio {n}"), TrackKind::Audio)).collect();
    for t in &mut tl.video_tracks {
        t.height = theme.m.video_track_h;
    }
    for t in &mut tl.audio_tracks {
        t.height = theme.m.audio_track_h;
    }
    // (track slot in palette order, media, position, in point, duration)
    let ev = |slot: usize, m: &MediaItem, pos: f64, inp: f64, dur: f64| {
        let mut c = Clip::from_media(m, pos);
        (c.in_point, c.duration, c.out_point) = (inp, dur, inp + dur);
        let k = theme.track_color(slot);
        c.color = [k.r(), k.g(), k.b()];
        c
    };
    tl.video_tracks[0].clips = vec![ev(0, &bars, 6.0, 1.0, 5.0), ev(0, &still, 13.0, 0.0, 4.0)];
    tl.video_tracks[1].clips = vec![ev(1, &src, 0.0, 0.0, 10.0)];
    tl.video_tracks[2].clips = vec![ev(2, &color, 2.0, 0.0, 6.0), ev(2, &src, 11.0, 3.0, 4.0)];
    tl.audio_tracks[0].clips = vec![ev(3, &src, 0.0, 0.0, 10.0)];
    tl.audio_tracks[1].clips = vec![ev(4, &tone, 1.5, 0.5, 9.0), ev(4, &tone, 12.0, 0.0, 5.0)];
    let mk = theme.marker_flag;
    for (pos, name) in [(3.0, "Intro"), (9.0, "Cut"), (15.5, "Outro")] {
        tl.markers.push(crate::project::Marker { position: pos, name: name.into(), color: [mk.r(), mk.g(), mk.b()] });
    }
    // Fades on a few events and a loop region, so the screenshots show them.
    (tl.video_tracks[1].clips[0].fade_in, tl.video_tracks[1].clips[0].fade_out) = (1.0, 1.5);
    tl.video_tracks[0].clips[1].fade_in = 0.75;
    (tl.audio_tracks[1].clips[0].fade_in, tl.audio_tracks[1].clips[0].fade_out) = (1.0, 2.0);
    tl.loop_region = Some((11.0, 14.0));
    tl.cursor_position = 5.0;
    tl.zoom = 0.8;
    p.media_pool = vec![src, bars, color, tone, still];
    Ok((p, ClipRef { is_video: true, track_idx: 1, clip_idx: 0 }))
}

/// Screenshot run state: waits until background work is quiet, captures, saves, closes.
pub struct Shot {
    path: PathBuf,
    start: Option<f64>,
    quiet_since: Option<f64>,
    requested: bool,
}

impl Shot {
    pub fn new(path: PathBuf) -> Self {
        Self { path, start: None, quiet_since: None, requested: false }
    }

    /// Call once per frame. `busy`: preview frames or waveforms still loading.
    pub fn tick(&mut self, ctx: &egui::Context, busy: bool) {
        ctx.request_repaint();
        let now = ctx.input(|i| i.time);
        // Render at 1 physical px per point (the spec measures at 100 % scaling), 1920×1080 px.
        // Points == px once the zoom is in, so the size is sent then (egui-winit scales by the zoomed ppp).
        let native = ctx.native_pixels_per_point().unwrap_or(1.0);
        ctx.set_zoom_factor(1.0 / native);
        let sized = ctx.screen_rect().size() == egui::vec2(1920.0, 1080.0);
        if !sized && (ctx.pixels_per_point() - 1.0).abs() < 1e-3 {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(1920.0, 1080.0)));
        }
        let start = *self.start.get_or_insert(now);
        let image = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            if let Err(e) = save_png(&self.path, &image) {
                eprintln!("screenshot: {e}");
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if busy || !sized {
            self.quiet_since = None;
        }
        let quiet = now - *self.quiet_since.get_or_insert(now);
        if !self.requested && ((now - start > 1.5 && quiet > 0.5) || now - start > 8.0) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            self.requested = true;
        }
    }
}

pub fn save_png(path: &Path, image: &ColorImage) -> anyhow::Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let [w, h] = image.size;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(path)?), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(image.as_raw())?;
    Ok(())
}
