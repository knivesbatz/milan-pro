//! Screenshot + demo harness, driven by environment variables (`MILAN_LOOK`, which chose among the looks of 0.8.0 and
//! earlier, is no longer read; scripts that still pass it get the one look):
//! - `MILAN_DIALOG=menu-edit`: the Edit menu dropped open (see `open_from_env` for the dialogs).
//! - `MILAN_DEMO=1`: demo project on generated media (cached in %TEMP%\milan-demo); no autosave or prefs writes.
//! - `MILAN_SCREENSHOT=<file.png>`: 1920×1080 px unfocused window, wait for frames to settle, save, exit.
//! - `MILAN_PANELS=explorer,trimmer,…`: dock windows to bring to the front (names without spaces, any case).
//! - `MILAN_OFX_DIR=<folder>` (with `MILAN_DEMO` or `MILAN_SCREENSHOT`): the only OFX folder searched (otherwise none),
//!   scanned before the screenshot; the user's plug-in list is never read or written. `MILAN_AUDIO_PLUGIN_DIR=<folder>`
//!   (R34a) likewise for VST3 and CLAP modules. `MILAN_AEX_DIR=<folder>` (A1) likewise for AE-format effects, and
//!   turns that experimental feature on for the run (`MILAN_DIALOG=plugin-manager-aex` shows its switch, `fx-aex`
//!   the FX window with one).
//! - `MILAN_DROP=x,y` (with `MILAN_DEMO`): the first and last demo media hover at window point x,y as if dragged from
//!   Windows, so a screenshot shows the timeline's drop ghost.
//! - `MILAN_OVERLAY_LIBRARY=<folder>` (with `MILAN_DIALOG=overlays`): where the harness makes its overlay library.
//! - `MILAN_SHOT_TRACE=1`: the screenshot's wait printed to stderr (busy, sized, requested) when a run does not end.
//! - `MILAN_SCRUB=<frames>` (with `MILAN_SCREENSHOT`): step the cursor forward that many frames, one per UI frame
//!   once each preview frame has arrived, before the screenshot (the preview's frame swaps, as in playback, silently).
//! - `MILAN_HOLD=1` (with `MILAN_SCREENSHOT`): the run never takes its screenshot or exits; it stays open off-screen
//!   until killed (R43's crash-recovery test). `MILAN_APPDATA=<folder>` moves autosave and session files there, and
//!   `MILAN_AUTOSAVE_NOW=1` autosaves at the first tick (both in `autosave.rs`).
//! - `MILAN_SCALE=1.5` (R48, with `MILAN_SCREENSHOT`): the interface scale (1 to 3) the shot renders at, whatever the
//!   display's own: the window is still 1920×1080 physical pixels, so 1.5 lays the UI out in 1280×720 points.
//!   `MILAN_SHOT_SIZE=3840x2160` sets the window's physical size (a 4K display at 200 % is 1920×1080 points).
//! - R48: each screenshot run prints `overflow: ...` (stdout) for every text that does not fit (`overflow`), and
//!   `MILAN_IDLE_CHECK=1` waits 2 s with nothing requested after the picture settles and prints `idle frames: n`, the
//!   frames painted meanwhile (a paused preview must cost none at any refresh rate).
//! - `MILAN_THEME=dark` (or `silver`): the design system's token set for the run, whatever Preferences › Display ›
//!   Theme says (the demo harness reads no preferences, so it is Silver otherwise).
//! - `MILAN_HW_TABLE=<file.json>` (R22): the hardware acceleration table to show in Preferences › Video, loaded as it
//!   is (no probe); `hw_capability_table` writes one to %TEMP%\milan-agent\hw-probe\hw.json.
use eframe::egui::{self, ColorImage};
use std::path::{Path, PathBuf};
use crate::media::probe_media;
use crate::project::{Clip, MediaItem, Project, Track, TrackKind};
use crate::ui::ClipRef;
use crate::ui::theme::Theme;

pub struct Launch {
    pub demo: bool,
    pub screenshot: Option<PathBuf>,
    pub panels: Vec<String>,
    pub drop_at: Option<egui::Pos2>,
    /// R46: this version failed to start twice since the update (`updater::start_marker`): offer a roll back.
    pub rollback_offer: bool,
}

impl Launch {
    pub fn from_env() -> Self {
        let var = |k| std::env::var(k).ok().filter(|v| !v.is_empty());
        Self {
            demo: var("MILAN_DEMO").is_some_and(|v| v != "0"),
            screenshot: var("MILAN_SCREENSHOT").map(PathBuf::from),
            panels: var("MILAN_PANELS").map(|v| v.split(',').map(|s| s.trim().to_lowercase()).collect()).unwrap_or_default(),
            drop_at: var("MILAN_DROP").and_then(|v| {
                let (x, y) = v.split_once(',')?;
                Some(egui::pos2(x.trim().parse().ok()?, y.trim().parse().ok()?))
            }),
            rollback_offer: false,
        }
    }
}

/// Lavfi sources for the demo media: (file name, ffmpeg input/output args).
/// HD video is encoded and tagged BT.709, as cameras do, so it previews in its true colours.
const MEDIA: [(&str, &[&str]); 5] = [
    ("testsrc2.mp4", &[
        "-f", "lavfi", "-i", "testsrc2=size=1920x1080:rate=30000/1001:duration=10",
        "-f", "lavfi", "-i", "sine=frequency=440:beep_factor=4:duration=10",
        "-vf", "format=rgb24,scale=out_color_matrix=bt709:out_range=tv,format=yuv420p",
        "-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709",
        "-c:v", "libx264", "-preset", "ultrafast", "-c:a", "aac", "-ac", "2", "-shortest",
    ]),
    // smptehdbars is generated as BT.709 YUV already: tagged only.
    ("smptebars.mp4", &[
        "-f", "lavfi", "-i", "smptehdbars=size=1920x1080:rate=30000/1001:duration=8",
        "-f", "lavfi", "-i", "sine=frequency=1000:duration=8",
        "-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709",
        "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", "-c:a", "aac", "-ac", "2", "-shortest",
    ]),
    ("color.mp4", &[
        "-f", "lavfi", "-i", "color=c=0x2a5d8f:size=1920x1080:rate=30000/1001:duration=6",
        "-vf", "format=rgb24,scale=out_color_matrix=bt709:out_range=tv,format=yuv420p",
        "-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709",
        "-c:v", "libx264", "-preset", "ultrafast",
    ]),
    // Stereo, with different L/R envelopes, so the two stacked channel waveforms differ.
    ("tone.wav", &["-f", "lavfi", "-i", "aevalsrc=0.8*sin(2*PI*220*t)*abs(sin(PI*t/3))|0.6*sin(2*PI*330*t)*abs(cos(PI*t/4)):s=48000:d=12"]),
    ("still.png", &["-f", "lavfi", "-i", "testsrc=size=1920x1080:rate=1", "-frames:v", "1"]),
];

/// Folder of the generated demo media.
pub fn media_dir() -> PathBuf {
    std::env::temp_dir().join("milan-demo")
}

/// Generate the demo media once; a `ready3` marker means every file of this media set was written.
fn ensure_media() -> anyhow::Result<PathBuf> {
    let dir = media_dir();
    if dir.join("ready3").exists() {
        return Ok(dir);
    }
    std::fs::create_dir_all(&dir)?;
    for (name, args) in MEDIA {
        let mut cmd = crate::proc::ffmpeg();
        cmd.args(["-y", "-hide_banner", "-loglevel", "error"]).args(args).arg(dir.join(name));
        let out = crate::proc::output(&mut cmd).map_err(|e| anyhow::anyhow!(crate::proc::tool_error(&e, "ffmpeg")))?;
        anyhow::ensure!(out.status.success(), "ffmpeg {name}: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    std::fs::write(dir.join("ready3"), "")?;
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
    // The 6 s colour clip runs 2 s past its media end, so it loops (a notch at 8 s).
    tl.video_tracks[2].clips = vec![ev(2, &color, 2.0, 0.0, 8.0), ev(2, &src, 11.0, 3.0, 4.0)];
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

/// R48: the harness's interface scale (`MILAN_SCALE`, 1 to 3; 1 without it).
pub fn harness_scale() -> f32 {
    std::env::var("MILAN_SCALE").ok().and_then(|v| v.trim().trim_end_matches('%').parse::<f32>().ok()).map_or(1.0, |s| if s > 10.0 { s / 100.0 } else { s }).clamp(1.0, 3.0)
}

/// The harness window in physical pixels: 1920×1080, or `MILAN_SHOT_SIZE=3840x2160` (R48: a 4K display at 200 %).
pub fn harness_pixels() -> egui::Vec2 {
    let size = std::env::var("MILAN_SHOT_SIZE").ok().and_then(|v| {
        let (w, h) = v.trim().split_once(['x', 'X'])?;
        Some(egui::vec2(w.trim().parse().ok()?, h.trim().parse().ok()?))
    });
    size.filter(|s| s.x >= 640.0 && s.y >= 360.0 && s.x <= 7680.0 && s.y <= 4320.0).unwrap_or(egui::vec2(1920.0, 1080.0))
}

/// The harness window in points: `harness_pixels` at the harness scale.
pub fn harness_points() -> egui::Vec2 {
    harness_pixels() / harness_scale()
}

/// The harness window has its size (and scale) this frame.
pub fn harness_sized(ctx: &egui::Context) -> bool {
    let want = harness_points();
    let have = ctx.screen_rect().size();
    (have.x - want.x).abs() < 1.0 && (have.y - want.y).abs() < 1.0 && (ctx.pixels_per_point() - harness_scale()).abs() < 1e-3
}

/// Screenshot run state: waits until background work is quiet, captures, saves, closes.
pub struct Shot {
    path: PathBuf,
    start: Option<f64>,
    quiet_since: Option<f64>,
    requested: bool,
    /// R48 `MILAN_IDLE_CHECK`: when the idle wait began, and the frames painted since.
    idle: Option<(f64, u32)>,
    idle_check: bool,
}

/// R48: how long the idle check waits with nothing requested.
const IDLE_SECS: f64 = 2.0;

impl Shot {
    pub fn new(path: PathBuf) -> Self {
        Self { path, start: None, quiet_since: None, requested: false, idle: None, idle_check: std::env::var_os("MILAN_IDLE_CHECK").is_some_and(|v| v == "1") }
    }

    /// Call once per frame. `busy`: preview frames or waveforms still loading (or the harness dialog not open yet).
    pub fn tick(&mut self, ctx: &egui::Context, busy: bool) {
        let now = ctx.input(|i| i.time);
        // R48: after the shot, the idle check paints only what the app itself asks for.
        if let Some((since, frames)) = self.idle.as_mut() {
            if since.is_infinite() {
                return; // reported; the window is closing
            }
            *frames += 1;
            if *frames > 1 && *frames < 6 {
                // What asked for the frame (egui records the caller), for the report.
                for c in ctx.repaint_causes() {
                    println!("idle repaint: {c}");
                }
            }
            if now - *since >= IDLE_SECS {
                println!("idle frames: {}", *frames - 1); // the harness report (stdout, as MILAN_SHOT_TRACE)
                self.idle = Some((f64::INFINITY, 0));
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(IDLE_SECS - (now - *since) + 0.05));
            }
            return;
        }
        ctx.request_repaint();
        // Render at 1 physical px per point (the spec measures at 100 % scaling), or R48's `MILAN_SCALE`, 1920×1080 px.
        // The size is sent once the zoom is in (egui-winit scales it by the zoomed ppp).
        let native = ctx.native_pixels_per_point().unwrap_or(1.0);
        let scale = harness_scale();
        ctx.set_zoom_factor(scale / native);
        let sized = harness_sized(ctx);
        if !sized && (ctx.pixels_per_point() - scale).abs() < 1e-3 {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(harness_points()));
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
                log::error!("screenshot: {e}");
            }
            if std::env::var_os("MILAN_SHOT_TRACE").is_some() {
                // R21c: what composed the preview, and whether any frame left the GPU (a zero-copy preview reads none back).
                println!("shot compositor: {} readbacks={:?}", crate::engine::gpu::status(), crate::engine::gpu::readback_count()); // the harness trace (R44: stdout, not the log)
            }
            if self.idle_check {
                self.idle = Some((now, 0));
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(IDLE_SECS + 0.05));
                return;
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if std::env::var_os("MILAN_SHOT_TRACE").is_some() {
            log::info!("shot t={:.2} busy={busy} sized={sized} requested={}", now - start, self.requested);
        }
        if busy || !sized {
            self.quiet_since = None;
        }
        let quiet = now - *self.quiet_since.get_or_insert(now);
        let hold = std::env::var_os("MILAN_HOLD").is_some_and(|v| v == "1");
        if !hold && !self.requested && ((now - start > 1.5 && quiet > 0.5) || now - start > 8.0) {
            // R48: the layout check, on the frame that is shot.
            for line in overflow(ctx) {
                println!("overflow: {line}"); // the harness report (stdout)
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            self.requested = true;
        }
    }
}

/// R48: the layout check of this frame (not pixels): every window, dialog and menu fits on the screen, every text of
/// one fits across its clip (a label cut off at the right is a layout that does not fit at this scale), and no
/// text of the main window runs off the window. Text scrolled partly out of a list is not counted.
pub fn overflow(ctx: &egui::Context) -> Vec<String> {
    use egui::{Order, Shape};
    let screen = ctx.screen_rect().expand(0.5);
    let layers: Vec<egui::LayerId> = ctx.memory(|m| m.layer_ids().collect());
    let mut out = Vec::new();
    for l in &layers {
        if l.order == Order::Background || l.order == Order::Debug {
            continue;
        }
        if let Some(r) = ctx.memory(|m| m.area_rect(l.id)).filter(|r| r.is_positive() && !screen.contains_rect(*r)) {
            out.push(format!("window {:?} at {r:?} is off the {screen:?} screen", l.id));
        }
    }
    fn texts<'a>(shape: &'a Shape, out: &mut Vec<&'a egui::epaint::TextShape>) {
        match shape {
            Shape::Text(t) => out.push(t),
            Shape::Vec(v) => v.iter().for_each(|s| texts(s, out)),
            _ => {}
        }
    }
    ctx.graphics(|g| {
        for l in &layers {
            let Some(list) = g.get(*l) else { continue };
            for cs in list.all_entries() {
                let mut found = Vec::new();
                texts(&cs.shape, &mut found);
                for t in found {
                    let r = t.visual_bounding_rect().shrink(1.0);
                    let text = t.galley.text();
                    if !r.is_positive() || text.trim().is_empty() || !cs.clip_rect.intersects(r) {
                        continue;
                    }
                    let clip = cs.clip_rect.intersect(screen);
                    let across = r.min.x < clip.min.x - 0.5 || r.max.x > clip.max.x + 0.5;
                    // Cut by the window's own edge (what shows of it runs past the screen).
                    let off_screen = !screen.contains_rect(cs.clip_rect.intersect(r));
                    if off_screen || (l.order != Order::Background && across) {
                        out.push(format!("\"{}\" at {r:?} outside {clip:?} ({:?})", text.chars().take(60).collect::<String>(), l.order));
                    }
                }
            }
        }
    });
    out
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Run the built app in the screenshot harness with `env`; its stdout (the harness reports).
    fn harness(env: &[(&str, &str)]) -> String {
        let exe = std::env::current_exe().unwrap().parent().unwrap().parent().unwrap().join("milan-pro.exe");
        assert!(exe.is_file(), "{} is not built", exe.display());
        let appdata = std::env::temp_dir().join("milan-agent").join("r48-tests");
        let mut c = std::process::Command::new(&exe);
        c.env("MILAN_DEMO", "1").env("MILAN_APPDATA", &appdata);
        for (k, v) in env {
            c.env(k, v);
        }
        c.stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        let out = crate::proc::output(&mut c).expect("milan-pro.exe runs");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// R48 acceptance: the main window, the FX window, Render As, Preferences and Pan/Crop at 100, 125, 150 and
    /// 200 % (a 1080-line display at 100-150 %, a 4K one at 200 %): no window off the screen and no text cut off
    /// (`overflow`). The shots go to %TEMP%\milan-review\r48. Needs target\release\milan-pro.exe:
    /// cargo build --release && cargo test --release harness_fits_at_every_scale -- --ignored
    #[test]
    #[ignore]
    fn harness_fits_at_every_scale() {
        let review = std::env::temp_dir().join("milan-review").join("r48");
        std::fs::create_dir_all(&review).unwrap();
        let mut bad = Vec::new();
        for (scale, size) in [("1", "1920x1080"), ("1.25", "1920x1080"), ("1.5", "1920x1080"), ("2", "3840x2160")] {
            for dialog in ["", "fx-event", "render", "prefs", "pan-crop"] {
                let shot = review.join(format!("fit-{}-{scale}.png", if dialog.is_empty() { "main" } else { dialog }));
                let _ = std::fs::remove_file(&shot);
                let out = harness(&[("MILAN_SCREENSHOT", &shot.display().to_string()), ("MILAN_SCALE", scale), ("MILAN_SHOT_SIZE", size), ("MILAN_DIALOG", dialog)]);
                assert!(shot.is_file(), "no shot at {scale} of {dialog:?}");
                bad.extend(out.lines().filter(|l| l.starts_with("overflow:")).map(|l| format!("{scale} {dialog}: {l}")));
            }
        }
        assert!(bad.is_empty(), "{}", bad.join("\n"));
    }

    /// R48: paused, the app paints nothing it was not asked to (a 144 Hz display costs no more than a 60 Hz one): the
    /// idle check counts the frames of 2 s after the picture settled.
    #[test]
    #[ignore]
    fn paused_preview_paints_no_extra_frames() {
        let shot = std::env::temp_dir().join("milan-agent").join("r48-tests").join("idle.png");
        let out = harness(&[("MILAN_SCREENSHOT", &shot.display().to_string()), ("MILAN_IDLE_CHECK", "1")]);
        let frames: u32 = out.lines().find_map(|l| l.strip_prefix("idle frames: ")).and_then(|n| n.trim().parse().ok()).unwrap_or_else(|| panic!("no idle report in {out:?}"));
        assert!(frames <= 5, "{frames} frames painted while paused and idle");
    }

    #[test]
    fn harness_scale_and_size_parse() {
        // Without the variables (the test process has none of them set).
        if std::env::var_os("MILAN_SCALE").is_none() && std::env::var_os("MILAN_SHOT_SIZE").is_none() {
            assert_eq!((harness_scale(), harness_pixels()), (1.0, egui::vec2(1920.0, 1080.0)));
        }
    }
}
