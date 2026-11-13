//! Preview: the timeline picture from the video engine, or single RGBA frames of one media (the Trimmer)
//! decoded by ffmpeg on a worker thread; and event thumbnails.
use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use crate::engine::VideoEngine;
use crate::project::{Fx, MediaItem, MediaType, Project};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, serde::Serialize, serde::Deserialize)]
pub enum Quality {
    Draft,
    Preview,
    Good,
    Best,
}

impl Quality {
    pub const ALL: [Quality; 4] = [Quality::Draft, Quality::Preview, Quality::Good, Quality::Best];

    /// ffmpeg scaler per quality; Draft also decodes at half the size's resolution.
    pub fn scaler(self) -> &'static str {
        match self {
            Quality::Draft | Quality::Preview => "fast_bilinear",
            Quality::Good => "bilinear",
            Quality::Best => "lanczos",
        }
    }
}

/// Preview size, the second half of "Quality (Size)" (§5.8).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Size {
    Auto,
    Full,
    Half,
    Quarter,
}

impl Size {
    pub const ALL: [Size; 4] = [Size::Auto, Size::Full, Size::Half, Size::Quarter];
}

/// R16b: Video Preview › Overlays: one channel of the frame shown (VEGAS's Red, Green, Blue and "as Grayscale" rows).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Isolate {
    #[default]
    None,
    Red,
    Green,
    Blue,
    RedGray,
    GreenGray,
    BlueGray,
    AlphaGray,
}

impl Isolate {
    pub const ALL: [(Isolate, &'static str); 7] = [
        (Isolate::Red, "Red"),
        (Isolate::Green, "Green"),
        (Isolate::Blue, "Blue"),
        (Isolate::RedGray, "Red as Grayscale"),
        (Isolate::GreenGray, "Green as Grayscale"),
        (Isolate::BlueGray, "Blue as Grayscale"),
        (Isolate::AlphaGray, "Alpha as Grayscale"),
    ];

    /// `img` (premultiplied, as shown over black) with only this channel, in its colour or as grey.
    pub fn apply(self, img: &ColorImage) -> ColorImage {
        let f = |p: &egui::Color32| {
            let [r, g, b, a] = p.to_array();
            let px = match self {
                Isolate::None => [r, g, b, a],
                Isolate::Red => [r, 0, 0, a],
                Isolate::Green => [0, g, 0, a],
                Isolate::Blue => [0, 0, b, a],
                Isolate::RedGray => [r, r, r, a],
                Isolate::GreenGray => [g, g, g, a],
                Isolate::BlueGray => [b, b, b, a],
                Isolate::AlphaGray => [a, a, a, 255],
            };
            egui::Color32::from_rgba_premultiplied(px[0], px[1], px[2], px[3])
        };
        ColorImage { size: img.size, pixels: img.pixels.iter().map(f).collect() }
    }
}

/// The frame of `img` as the preview shows it (`isolate`), into texture `slot`.
fn upload(ctx: &egui::Context, slot: &mut Option<TextureHandle>, name: &str, img: &Arc<ColorImage>, isolate: Isolate) {
    let data = egui::ImageData::Color(if isolate == Isolate::None { img.clone() } else { Arc::new(isolate.apply(img)) });
    match slot {
        Some(t) => t.set(data, TextureOptions::LINEAR),
        None => *slot = Some(ctx.load_texture(name, data, TextureOptions::LINEAR)),
    }
}

/// R16b: Split-Screen View › FX Bypassed: a second engine composing the timeline without video FX, and its frame.
struct Bypass {
    engine: VideoEngine,
    image: Option<Arc<ColorImage>>,
    texture: Option<TextureHandle>,
}

/// Auto as the size it picks: the largest of Full / Half / Quarter that fits the display, as VEGAS reports it.
fn resolve(size: Size, full_w: u32, display_w: u32) -> Size {
    let full = full_w.max(2);
    match size {
        Size::Auto => [(full, Size::Full), (full / 2, Size::Half)].into_iter().find(|&(w, _)| w <= display_w).map_or(Size::Quarter, |(_, s)| s),
        s => s,
    }
}

/// Preview frame width for a project `full_w` wide shown in a `display_w` wide frame.
pub fn preview_width(q: Quality, size: Size, full_w: u32, display_w: u32) -> u32 {
    let full = full_w.max(2);
    let w = match resolve(size, full_w, display_w) {
        Size::Half => full / 2,
        Size::Quarter => full / 4,
        _ => full,
    };
    let w = if q == Quality::Draft { w / 2 } else { w };
    (w / 2 * 2).max(16)
}

/// R19: the settings "Adjust Size and Quality for Optimal Playback" steps through below `q` (`size`), the chosen one
/// first: the size halves down to Quarter, then the quality drops to Draft.
pub fn ladder(q: Quality, size: Size, full_w: u32, display_w: u32) -> Vec<(Quality, Size)> {
    let mut s = resolve(size, full_w, display_w);
    let mut rungs = vec![(q, size)];
    while let Some(next) = match s {
        Size::Full => Some(Size::Half),
        Size::Half => Some(Size::Quarter),
        _ => None,
    } {
        s = next;
        rungs.push((q, s));
    }
    if q != Quality::Draft {
        rungs.push((Quality::Draft, Size::Quarter));
    }
    rungs
}

/// R19: the "Adjust Size and Quality for Optimal Playback" controller. While playing, more than 2 % of frames dropped
/// over 2 s steps one rung down the `ladder`; 10 s without a drop steps one back up. A step up that drops again within
/// 10 s doubles the clean time the next one needs (up to 160 s), so a setting that cannot keep up is not retried
/// every few seconds. After every change and play start, counting waits until the new session has spun up (its
/// decoders open and seek, which can take seconds on long-GOP media): 1 s and 15 frames shown, or 4 s.
/// Times are seconds on any clock; counts are a playback's totals (`engine::Shows`).
#[derive(Default, Debug)]
pub struct Adapt {
    /// Rungs below the chosen setting (0 = as chosen).
    pub level: usize,
    /// Start of the measuring window and the (shown, dropped) totals at it.
    window: Option<(f64, u64, u64)>,
    /// The dropped total last seen, and since when it has not grown.
    last_dropped: u64,
    clean: f64,
    /// Clean seconds the next step up needs (0 = not set yet, `CLEAN`).
    hold: f64,
    /// When the last step up happened, until it has held for `CLEAN`.
    raised: Option<f64>,
    /// When the session (re)started, and the shown total at the first update after it: counting waits for the spin-up.
    settling: Option<(f64, Option<u64>)>,
}

impl Adapt {
    const WINDOW: f64 = 2.0;
    const DROP: f64 = 0.02;
    const CLEAN: f64 = 10.0;
    const SETTLE: f64 = 1.0;
    const SETTLE_FRAMES: u64 = 15;
    const MAX_SETTLE: f64 = 4.0;
    const MAX_HOLD: f64 = 160.0;

    /// Playback (re)started at `now` (a seek, a loop change): the level is kept, counting starts after the spin-up.
    pub fn restart(&mut self, now: f64) {
        (self.settling, self.window) = (Some((now, None)), None);
    }

    /// One update while playing, with the playback's totals and the rungs below the chosen setting (`ladder` length
    /// less one). Returns whether the level changed.
    pub fn tick(&mut self, now: f64, shown: u64, dropped: u64, rungs: usize) -> bool {
        self.level = self.level.min(rungs);
        if self.hold == 0.0 {
            self.hold = Self::CLEAN;
        }
        if let Some((t, s0)) = &mut self.settling {
            let s0 = *s0.get_or_insert(shown);
            if now - *t < Self::SETTLE || (shown < s0 + Self::SETTLE_FRAMES && now - *t < Self::MAX_SETTLE) {
                return false;
            }
            self.settling = None;
        }
        let Some((t0, s0, d0)) = self.window else {
            (self.window, self.last_dropped, self.clean) = (Some((now, shown, dropped)), dropped, now);
            return false;
        };
        if dropped > self.last_dropped {
            (self.last_dropped, self.clean) = (dropped, now);
        }
        if self.raised.is_some_and(|t| now - t >= Self::CLEAN) {
            // The step up held: the next one waits the usual time.
            (self.raised, self.hold) = (None, Self::CLEAN);
        }
        if now - t0 >= Self::WINDOW {
            let (s, d) = (shown.saturating_sub(s0), dropped.saturating_sub(d0));
            self.window = Some((now, shown, dropped));
            if self.level < rungs && d as f64 > Self::DROP * (s + d) as f64 {
                if self.raised.take().is_some() {
                    self.hold = (self.hold * 2.0).min(Self::MAX_HOLD);
                }
                return self.change(now, self.level + 1);
            }
        }
        if self.level > 0 && now - self.clean >= self.hold {
            self.raised = Some(now);
            return self.change(now, self.level - 1);
        }
        false
    }

    fn change(&mut self, now: f64, level: usize) -> bool {
        self.level = level;
        self.restart(now);
        true
    }
}

struct FrameRequest {
    id: u64,
    path: PathBuf,
    time: f64,
    width: u32,
    height: u32,
    scaler: &'static str,
    /// ffmpeg `in_color_matrix` (None = auto), as the engine decodes the same media.
    matrix: Option<&'static str>,
    /// A video proxy of `path`, read instead; if it fails, `path` is read (and the proxy forgotten if that works).
    proxy: Option<PathBuf>,
    /// Generated media: rendered by this generator at `time` instead of decoded.
    generator: Option<Fx>,
    /// The media's frame rate, if known: `time` then shows the frame on screen at it, as the timeline does.
    fps: Option<f64>,
    /// R6b: `path` is a nested timeline: composed by the engine, never read by ffmpeg.
    nested: bool,
}

/// The `-ss` that gives the frame on screen at `t` (the latest at or before it). ffmpeg returns the first frame at or
/// after its seek time, so seek half a frame before that frame's start.
fn seek_time(t: f64, fps: Option<f64>) -> f64 {
    match fps.filter(|f| *f > 0.0) {
        Some(f) => ((crate::engine::plan::frame_at(t.max(0.0), f) as f64 - 0.5) / f).max(0.0),
        None => t.max(0.0),
    }
}

struct FrameResult {
    id: u64,
    image: Result<ColorImage, String>,
}

/// Even-sized decode dimensions preserving the source aspect (16:9 if unknown).
fn decode_size(m: &MediaItem, max_w: u32) -> (u32, u32) {
    let (sw, sh) = match (m.width, m.height) {
        (Some(w), Some(h)) if w > 0 && h > 0 => (w, h),
        _ => (1920, 1080),
    };
    let w = (max_w.min(sw) / 2 * 2).max(2);
    let h = ((w as f64 * sh as f64 / sw as f64 / 2.0).round() as u32 * 2).max(2);
    (w, h)
}

fn decode_frame(req: &FrameRequest) -> Result<ColorImage, String> {
    if let Some(g) = &req.generator {
        let e = crate::engine::plugins::eval(g, req.time).ok_or_else(|| format!("Unknown generator {}", g.plugin))?;
        return Ok(crate::engine::generate::render(&e, req.width, req.height, req.time));
    }
    if req.nested {
        let q = if req.scaler == "lanczos" { Quality::Best } else { Quality::Good };
        return crate::engine::nested::frame(&req.path, req.time, req.width, req.height, q);
    }
    let proxied = req.proxy.as_ref().map(|p| decode_file(req, p));
    if let Some(Ok(img)) = proxied {
        return Ok(img);
    }
    let r = decode_file(req, &req.path);
    if proxied.is_some() && r.is_ok() {
        // The source has this frame and the proxy does not: the proxy is broken or gone.
        crate::proxy::forget(&req.path);
    }
    r
}

fn decode_file(req: &FrameRequest, path: &Path) -> Result<ColorImage, String> {
    let mut cmd = crate::proc::ffmpeg();
    cmd.args(["-v", "error", "-ss", &format!("{:.6}", seek_time(req.time, req.fps)), "-i"])
        .arg(path)
        .args(["-frames:v", "1", "-vf"])
        .arg(format!(
            "scale={}:{}:flags={}:in_color_matrix={}:out_range=full,format=rgba",
            req.width,
            req.height,
            req.scaler,
            req.matrix.unwrap_or("auto")
        ))
        .args(["-f", "rawvideo", "-"]);
    let out = crate::proc::output(&mut cmd).map_err(|e| crate::proc::tool_error(&e, "ffmpeg"))?;
    let need = (req.width * req.height * 4) as usize;
    if out.stdout.len() < need {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(err.lines().next().unwrap_or("no frame at this time").to_string());
    }
    let mut pixels: Vec<egui::Color32> = bytemuck::cast_slice(&out.stdout[..need]).to_vec();
    premultiply(&mut pixels);
    Ok(ColorImage { size: [req.width as usize, req.height as usize], pixels })
}

/// Straight RGBA to premultiplied in gamma space, as the compositor blends (egui's `from_rgba_unmultiplied` works in
/// linear light, which makes soft edges too bright and RGB larger than alpha).
pub(crate) fn premultiply(px: &mut [egui::Color32]) {
    for p in px {
        let [r, g, b, a] = p.to_array();
        if a < 255 {
            let m = |c: u8| crate::engine::composite::mul255(c as u32, a as u32) as u8;
            *p = egui::Color32::from_rgba_premultiplied(m(r), m(g), m(b), a);
        }
    }
}

/// The first frame of `path` (a still) at `w` x `h`, premultiplied.
pub(crate) fn still(path: &Path, w: u32, h: u32, scaler: &'static str) -> Result<ColorImage, String> {
    decode_frame(&FrameRequest { id: 0, path: path.to_path_buf(), time: 0.0, width: w, height: h, scaler, matrix: None, proxy: None, generator: None, fps: None, nested: false })
}

/// Preview state owned by the app: worker channels plus the displayed texture.
pub struct Preview {
    tx: Sender<FrameRequest>,
    rx: Receiver<FrameResult>,
    pub quality: Quality,
    pub size: Size,
    /// Width of the frame on screen, for Size::Auto; set by the panel each frame.
    pub display_w: u32,
    key: Option<(PathBuf, i64, u32, Quality)>,
    next_id: u64,
    /// Results older than this were requested before the preview went black.
    min_id: u64,
    /// One past the newest request whose result arrived.
    done_id: u64,
    pub texture: Option<TextureHandle>,
    /// The frame behind `texture`, for snapshots.
    pub image: Option<Arc<ColorImage>>,
    pub error: Option<String>,
    /// The timeline preview's engine; None for a single-media preview (the Trimmer).
    engine: Option<VideoEngine>,
    /// The Video Preview window is on screen this frame (set by the app); frames are only made while it is.
    pub visible: bool,
    /// Options › Mute All Video: the Video Preview shows no frame.
    pub mute: bool,
    /// R16b: the Overlays channel shown, and the one the textures were made with.
    pub isolate: Isolate,
    shown_isolate: Isolate,
    /// R16b: Split-Screen View › FX Bypassed is on (set by the app each frame): the bypassed engine runs while it is.
    pub fx_bypassed: bool,
    bypass: Option<Bypass>,
    /// Playing (with its loop region) or not, for a bypassed engine started mid-playback.
    playing: Option<Option<(f64, f64)>>,
    /// R17b: Enable Multicamera Editing is on (set by the app each frame): the picture is the take grid.
    pub multicam: bool,
    /// R17b: Show Full Frame in External Monitor while the grid shows (set by the app each frame): a second engine
    /// makes the timeline's own picture for the external monitor.
    pub ext_full: bool,
    full: Option<Bypass>,
    /// R19: Adjust Size and Quality for Optimal Playback (on, as in VEGAS).
    pub adjust: bool,
    /// Its controller, and the chosen quality and size it was started for.
    adapt: Option<(Adapt, (Quality, Size))>,
    epoch: std::time::Instant,
}

impl Preview {
    pub fn new(ctx: &egui::Context) -> Self {
        let (tx, req_rx) = mpsc::channel::<FrameRequest>();
        let (res_tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            while let Ok(mut req) = req_rx.recv() {
                // Scrubbing queues many requests; only the newest matters.
                while let Ok(newer) = req_rx.try_recv() {
                    req = newer;
                }
                let image = decode_frame(&req);
                if res_tx.send(FrameResult { id: req.id, image }).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        Self {
            tx,
            rx,
            // Factory default "Preview (Auto)" (§5.8).
            quality: Quality::Preview,
            size: Size::Auto,
            display_w: 960,
            key: None,
            next_id: 0,
            min_id: 0,
            done_id: 0,
            texture: None,
            image: None,
            error: None,
            engine: None,
            visible: false,
            mute: false,
            isolate: Isolate::None,
            shown_isolate: Isolate::None,
            fx_bypassed: false,
            bypass: None,
            playing: None,
            multicam: false,
            ext_full: false,
            full: None,
            adjust: true,
            adapt: None,
            epoch: std::time::Instant::now(),
        }
    }

    /// R17b: the external monitor's picture: the full frame while the grid shows with Show Full Frame on, else the
    /// Video Preview's.
    pub fn external_texture(&self) -> Option<&TextureHandle> {
        match &self.full {
            Some(f) => f.texture.as_ref(),
            None => self.texture.as_ref(),
        }
    }

    /// R16b: the FX Bypassed frame's texture, while Split-Screen View › FX Bypassed is on.
    pub fn bypass_texture(&self) -> Option<&TextureHandle> {
        self.bypass.as_ref()?.texture.as_ref()
    }

    /// The timeline preview: every layer composited by the video engine.
    pub fn timeline(ctx: &egui::Context) -> Self {
        Self { engine: Some(VideoEngine::new(ctx)), ..Self::new(ctx) }
    }

    /// Decode width for a project `full_w` wide at the current quality and size.
    pub fn width(&self, full_w: u32) -> u32 {
        let (q, s) = self.effective(full_w);
        preview_width(q, s, full_w, self.display_w)
    }

    /// R19: the quality (size) the preview runs at: the chosen one, or the rung "Adjust Size and Quality for Optimal
    /// Playback" has stepped down to while playing.
    pub fn effective(&self, full_w: u32) -> (Quality, Size) {
        match self.adapt.as_ref().filter(|a| self.adjust && self.playing.is_some() && a.0.level > 0) {
            Some(a) => {
                let l = ladder(self.quality, self.size, full_w, self.display_w);
                l[a.0.level.min(l.len() - 1)]
            }
            None => (self.quality, self.size),
        }
    }

    /// R19: the timeline playback's figures while playing: what the preview showed and skipped, and the worker's.
    pub fn playback(&self) -> Option<(crate::engine::Shows, &crate::engine::Stats)> {
        let e = self.engine.as_ref().filter(|_| self.playing.is_some())?;
        Some((e.shows, &e.stats))
    }

    /// A requested frame has not arrived yet.
    pub fn busy(&self) -> bool {
        match &self.engine {
            Some(e) => e.busy(),
            None => self.done_id.max(self.min_id) < self.next_id,
        }
    }

    /// Timeline preview, once per UI frame: the frame at the cursor (paused) or the due frame (playing).
    pub fn update(&mut self, ctx: &egui::Context, project: &Project) {
        let s = &project.settings;
        // R19: a new chosen setting (or the option turned off) starts the controller again.
        let chosen = (self.quality, self.size);
        if !self.adjust || self.adapt.as_ref().is_some_and(|a| a.1 != chosen) {
            self.adapt = None;
        }
        let w = self.width(s.width);
        let h = ((w as f64 * s.height as f64 / s.width.max(1) as f64 / 2.0).round() as u32 * 2).max(2);
        let (q, visible) = (self.effective(s.width).0, self.visible);
        let Some(e) = &mut self.engine else { return };
        e.grid = self.multicam;
        let got = e.update(project, w, h, q, visible);
        let shows = e.shows;
        if let Some((img, err)) = got {
            // A failed layer still shows the others; the error goes to the status bar.
            self.error = err;
            self.set_image(ctx, img);
        }
        if self.adjust && self.playing.is_some() && visible {
            let now = self.epoch.elapsed().as_secs_f64();
            let rungs = ladder(chosen.0, chosen.1, s.width, self.display_w).len() - 1;
            let a = self.adapt.get_or_insert_with(|| {
                let mut a = Adapt::default();
                a.restart(now);
                (a, chosen)
            });
            if a.0.tick(now, shows.shown, shows.dropped, rungs) {
                ctx.request_repaint();
            }
        }
        // R16b: the FX Bypassed engine runs only while that split is on (it composes every frame a second time); R17b:
        // the external monitor's full frame only while the grid shows there.
        let playing = self.playing;
        let start = |on: bool, slot: &mut Option<Bypass>, engine: fn(&egui::Context) -> VideoEngine| match (on, slot.is_some()) {
            (false, _) => *slot = None,
            (true, false) => {
                let mut engine = engine(ctx);
                if let Some(lp) = playing {
                    engine.play(lp);
                }
                *slot = Some(Bypass { engine, image: None, texture: None });
            }
            (true, true) => {}
        };
        start(self.fx_bypassed, &mut self.bypass, VideoEngine::without_fx);
        start(self.multicam && self.ext_full, &mut self.full, VideoEngine::new);
        if let Some(b) = &mut self.bypass {
            if let Some((img, _)) = b.engine.update(project, w, h, q, visible) {
                upload(ctx, &mut b.texture, "preview_bypassed", &img, self.isolate);
                b.image = Some(img);
            }
        }
        if let Some(f) = &mut self.full {
            if let Some((img, _)) = f.engine.update(project, w, h, q, visible) {
                upload(ctx, &mut f.texture, "preview_full", &img, Isolate::None);
                f.image = Some(img);
            }
        }
        if self.isolate != self.shown_isolate {
            self.shown_isolate = self.isolate;
            if let Some(img) = self.image.clone() {
                upload(ctx, &mut self.texture, "preview_frame", &img, self.isolate);
            }
            if let Some(b) = &mut self.bypass {
                if let Some(img) = &b.image {
                    upload(ctx, &mut b.texture, "preview_bypassed", img, self.isolate);
                }
            }
        }
    }

    /// Play the timeline from the cursor (again, after a seek), looping over `loop_` if set.
    pub fn play(&mut self, loop_: Option<(f64, f64)>) {
        // R19: a seek while playing keeps the adjusted rung; counting waits for the new session.
        let now = self.epoch.elapsed().as_secs_f64();
        if let Some(a) = &mut self.adapt {
            a.0.restart(now);
        }
        self.playing = Some(loop_);
        for e in self.engines() {
            e.play(loop_);
        }
    }

    pub fn stop(&mut self) {
        // R19: stopped, the preview is back at the chosen quality; the next playback starts from it.
        self.adapt = None;
        self.playing = None;
        for e in self.engines() {
            e.stop();
        }
    }

    fn engines(&mut self) -> impl Iterator<Item = &mut VideoEngine> {
        self.engine.iter_mut().chain(self.bypass.as_mut().map(|b| &mut b.engine)).chain(self.full.as_mut().map(|b| &mut b.engine))
    }

    fn set_image(&mut self, ctx: &egui::Context, img: Arc<ColorImage>) {
        upload(ctx, &mut self.texture, "preview_frame", &img, self.shown_isolate);
        self.image = Some(img);
    }

    /// Like `update`, for an explicit source (the Trimmer's media at its own cursor).
    /// `fps` is the fallback frame rate; `full_w` the width that Size::Full means.
    pub fn show(&mut self, ctx: &egui::Context, src: Option<(&MediaItem, f64)>, fps: f64, full_w: u32) {
        let max_w = self.width(full_w);
        // A subclip's time is read from its file.
        let src = src.map(|(m, t)| (m, m.file_time(t)));
        let rate = |m: &MediaItem| m.frame_rate.filter(|f| *f > 0.0).unwrap_or(fps.max(1.0));
        let key = src.map(|(m, t)| (key_path(m), crate::engine::plan::frame_at(t, rate(m)), max_w, self.quality));
        if key != self.key {
            self.key = key;
            match src {
                Some((m, time)) => {
                    let (width, height) = decode_size(m, max_w);
                    // Draft and Preview read the video proxy, as the timeline preview does.
                    let proxy = matches!(self.quality, Quality::Draft | Quality::Preview) && crate::proxy::is_video(m);
                    let req = FrameRequest {
                        id: self.next_id,
                        path: m.path.clone(),
                        time,
                        width,
                        height,
                        scaler: self.quality.scaler(),
                        matrix: crate::engine::plan::matrix(m),
                        proxy: proxy.then(|| crate::proxy::ready(&m.path)).flatten(),
                        generator: m.generator.clone(),
                        fps: Some(rate(m)),
                        nested: m.nested,
                    };
                    self.next_id += 1;
                    let _ = self.tx.send(req);
                }
                None => {
                    self.min_id = self.next_id;
                    self.texture = None;
                    self.image = None;
                    self.error = None;
                }
            }
        }

        let mut got = false;
        let results: Vec<FrameResult> = self.rx.try_iter().collect();
        for r in results {
            if r.id < self.min_id {
                continue;
            }
            got = true;
            self.done_id = self.done_id.max(r.id + 1);
            match r.image {
                Ok(img) => {
                    self.error = None;
                    self.set_image(ctx, Arc::new(img));
                }
                Err(e) => {
                    self.error = Some(e);
                    self.texture = None;
                    self.image = None;
                }
            }
        }
        if got {
            // Polled after drawing, so show the new frame next pass.
            ctx.request_repaint();
        }
    }
}

/// (`key_path` of the media, source time in ms)
type ThumbKey = (PathBuf, i64);

/// What a picture of `m` is cached under: its file, or for generated media its generator's settings (an edit is a new picture).
fn key_path(m: &MediaItem) -> PathBuf {
    match &m.generator {
        Some(g) => {
            let mut h = std::hash::DefaultHasher::new();
            std::hash::Hash::hash(&format!("{g:?}"), &mut h);
            format!("<generated {:016x}>", std::hash::Hasher::finish(&h)).into()
        }
        // R6b: a nested timeline saved since is a new picture.
        None if m.nested => format!("{}|{:?}", m.path.display(), crate::engine::nested::version(&m.path)).into(),
        None => m.path.clone(),
    }
}

enum Thumb {
    Pending,
    Ready(TextureHandle),
    Failed,
}

/// Timeline event thumbnails: small frames decoded by ffmpeg on two worker threads, newest request first,
/// so the visible events fill in before anything scrolled away.
pub struct Thumbs {
    /// (key, request for the source, may read a video proxy)
    queue: Arc<(Mutex<Vec<(ThumbKey, FrameRequest, bool)>>, Condvar)>,
    rx: Receiver<(ThumbKey, Result<ColorImage, String>)>,
    cache: HashMap<ThumbKey, Thumb>,
    ctx: egui::Context,
    /// Set while the timeline plays, so thumbnail decodes leave the CPU to playback.
    paused: Arc<AtomicBool>,
}

/// Queued requests kept; older ones are dropped (and re-requested if they come back into view).
const THUMB_QUEUE: usize = 96;
/// Decoded thumbnails kept before the cache is cleared (≈160×90 RGBA each).
const THUMB_CACHE: usize = 400;
pub const THUMB_WIDTH: u32 = 160;

impl Thumbs {
    pub fn new(ctx: &egui::Context) -> Self {
        let queue = Arc::new((Mutex::new(Vec::<(ThumbKey, FrameRequest, bool)>::new()), Condvar::new()));
        let (tx, rx) = mpsc::channel();
        let paused = Arc::new(AtomicBool::new(false));
        for _ in 0..2 {
            let (queue, tx, ctx, paused) = (queue.clone(), tx.clone(), ctx.clone(), paused.clone());
            std::thread::spawn(move || {
                loop {
                    while paused.load(Relaxed) {
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                    let (key, req, proxyable) = {
                        let (lock, cv) = &*queue;
                        let mut q = cv.wait_while(lock.lock().unwrap(), |q| q.is_empty()).unwrap();
                        q.pop().expect("non-empty queue")
                    };
                    // The proxy is chosen here, not when queued, so thumbnails asked for before the proxy lookup
                    // finished (every project open) still read it.
                    let proxy = proxyable.then(|| crate::proxy::ready(&req.path).or_else(|| crate::proxy::find_own(&req.path))).flatten();
                    if tx.send((key, decode_frame(&FrameRequest { proxy, ..req }))).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            });
        }
        Self { queue, rx, cache: HashMap::new(), ctx: ctx.clone(), paused }
    }

    pub fn set_paused(&self, on: bool) {
        self.paused.store(on, Relaxed);
    }

    pub fn paused(&self) -> bool {
        self.paused.load(Relaxed)
    }

    fn receive(&mut self) {
        for (key, img) in self.rx.try_iter() {
            let v = match img {
                Ok(img) => Thumb::Ready(self.ctx.load_texture("event_thumb", img, TextureOptions::LINEAR)),
                Err(_) => Thumb::Failed,
            };
            self.cache.insert(key, v);
        }
    }

    /// Requested thumbnails have not all arrived yet.
    pub fn pending(&mut self) -> bool {
        self.receive();
        self.cache.values().any(|v| matches!(v, Thumb::Pending))
    }

    /// Thumbnail of `m` at source time `time` (callers quantise it), requesting it on first use.
    pub fn get(&mut self, m: &MediaItem, time: f64) -> Option<&TextureHandle> {
        self.receive();
        let time = if m.media_type == MediaType::Image { 0.0 } else { m.file_time(time.max(0.0)) };
        let key = (key_path(m), (time * 1000.0).round() as i64);
        if !self.cache.contains_key(&key) {
            if self.cache.len() >= THUMB_CACHE {
                self.cache.retain(|_, v| matches!(v, Thumb::Pending));
            }
            let (width, height) = decode_size(m, THUMB_WIDTH);
            // Keyed by the source, so thumbnails made before its proxy was ready stay; new ones read the proxy.
            let req = FrameRequest { id: 0, path: m.path.clone(), time, width, height, scaler: "bilinear", matrix: crate::engine::plan::matrix(m), proxy: None, generator: m.generator.clone(), fps: m.frame_rate, nested: m.nested };
            let (lock, cv) = &*self.queue;
            let mut q = lock.lock().unwrap();
            q.push((key.clone(), req, crate::proxy::is_video(m)));
            if q.len() > THUMB_QUEUE {
                let (old, ..) = q.remove(0);
                self.cache.remove(&old);
            }
            cv.notify_one();
            self.cache.insert(key.clone(), Thumb::Pending);
        }
        match &self.cache[&key] {
            Thumb::Ready(t) => Some(t),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn media(id: &str, kind: MediaType) -> MediaItem {
        MediaItem {
            id: id.into(),
            path: format!("{id}.bin").into(),
            name: id.into(),
            media_type: kind,
            duration: 100.0,
            width: Some(1920),
            height: Some(1080),
            frame_rate: Some(30.0),
            ..Default::default()
        }
    }

    #[test]
    fn preview_width_quality_and_size() {
        use Quality::*;
        assert_eq!(preview_width(Best, Size::Full, 1920, 500), 1920);
        assert_eq!(preview_width(Good, Size::Half, 1920, 500), 960);
        assert_eq!(preview_width(Draft, Size::Quarter, 1920, 500), 240);
        // Auto: the largest of full / half / quarter that fits the display (748 px shows 480, as in VEGAS).
        assert_eq!(preview_width(Preview, Size::Auto, 1920, 748), 480);
        assert_eq!(preview_width(Preview, Size::Auto, 1920, 1000), 960);
        assert_eq!(preview_width(Preview, Size::Auto, 1920, 2000), 1920);
    }

    /// R19: Adjust Size and Quality steps the size down to Quarter, then to Draft; Auto starts from the size it picks.
    #[test]
    fn adjust_ladder() {
        use Quality::*;
        use Size::*;
        assert_eq!(ladder(Best, Full, 1920, 500), [(Best, Full), (Best, Half), (Best, Quarter), (Draft, Quarter)]);
        assert_eq!(ladder(Preview, Auto, 1920, 1000), [(Preview, Auto), (Preview, Quarter), (Draft, Quarter)]);
        assert_eq!(ladder(Draft, Half, 1920, 1000), [(Draft, Half), (Draft, Quarter)]);
        assert_eq!(ladder(Draft, Quarter, 1920, 1000), [(Draft, Quarter)]);
        let w: Vec<u32> = ladder(Good, Full, 1920, 500).iter().map(|&(q, s)| preview_width(q, s, 1920, 500)).collect();
        assert_eq!(w, [1920, 960, 480, 240]);
    }

    /// Plays `secs` of 30 fps against the controller, ticked at 60 Hz, with a simulated worker that drops the share
    /// `cost(level, t)` of its frames. Returns every (time, new level).
    fn simulate(cost: impl Fn(usize, f64) -> f64, secs: f64, rungs: usize) -> Vec<(f64, usize)> {
        simulate_with_stall(cost, secs, rungs, 0.0)
    }

    /// `simulate`, where every change (and the start) first stalls `stall` seconds while the new session's decoders
    /// open and seek: nothing shows, then the frames missed count as dropped at once.
    fn simulate_with_stall(cost: impl Fn(usize, f64) -> f64, secs: f64, rungs: usize, stall: f64) -> Vec<(f64, usize)> {
        let mut a = Adapt::default();
        a.restart(0.0);
        let (mut shown, mut dropped, mut debt, mut changes, mut stall_until, mut missed) = (0u64, 0u64, 0.0, Vec::new(), stall, 0);
        for k in 1..=(secs * 60.0) as u64 {
            let t = k as f64 / 60.0;
            if k % 2 == 0 && t < stall_until {
                missed += 1;
            } else if k % 2 == 0 {
                dropped += std::mem::take(&mut missed);
                debt += cost(a.level, t);
                if debt >= 1.0 {
                    (debt, dropped) = (debt - 1.0, dropped + 1);
                } else {
                    shown += 1;
                }
            }
            if a.tick(t, shown, dropped, rungs) {
                changes.push((t, a.level));
                stall_until = t + stall;
            }
        }
        changes
    }

    /// R19 acceptance: the chosen setting drops 10 % of frames and one rung down none. The controller steps down
    /// after the 1 s settle and one 2 s window, retries the chosen setting after 10 s clean, and each failed retry
    /// doubles the wait: over 10 minutes the playback drops frames for under 25 s and never goes two rungs down.
    #[test]
    fn adaptive_quality_steps_down_then_up_without_oscillating() {
        let ch = simulate(|level, _| if level == 0 { 0.1 } else { 0.0 }, 600.0, 3);
        assert_eq!(ch[0].1, 1);
        assert!(ch[0].0 <= 3.1, "down after 2 s of drops: {ch:?}");
        assert!(ch.iter().all(|c| c.1 <= 1), "{ch:?}");
        assert!((ch[1].0 - ch[0].0 - 11.0).abs() < 0.1, "up after 10 s clean (after the 1 s settle): {ch:?}");
        let ups: Vec<f64> = ch.iter().filter(|c| c.1 == 0).map(|c| c.0).collect();
        let gaps: Vec<f64> = ups.windows(2).map(|w| w[1] - w[0]).collect();
        assert!(gaps.windows(2).all(|g| g[1] >= g[0] - 0.1), "retries back off: {gaps:?}");
        assert!((3..=7).contains(&ups.len()), "{ups:?}");
        let mut at_top = ch[0].0;
        for w in ch.windows(2).filter(|w| w[0].1 == 0) {
            at_top += w[1].0 - w[0].0;
        }
        assert!(at_top < 25.0, "{at_top} s at the setting that drops frames: {ch:?}");
    }

    /// R19: the heavy part ends at 20 s: the chosen setting comes back and stays.
    #[test]
    fn adaptive_quality_restores_when_playback_keeps_up() {
        let ch = simulate(|level, t| if level == 0 && t < 20.0 { 0.1 } else { 0.0 }, 120.0, 3);
        assert_eq!(ch.last().unwrap().1, 0, "{ch:?}");
        assert!(ch.last().unwrap().0 < 60.0, "{ch:?}");
        // Several heavy rungs: one step per settle and window, down to the last rung, and no further.
        let ch = simulate(|_, _| 0.5, 60.0, 3);
        assert_eq!(ch.iter().map(|c| c.1).collect::<Vec<_>>(), [1, 2, 3], "{ch:?}");
        assert!(ch[2].0 <= 9.1, "{ch:?}");
        // Under 2 % dropped: no change, either way.
        assert!(simulate(|_, _| 0.015, 120.0, 3).is_empty());
        // Nothing to step down to (Draft (Quarter) chosen): no change.
        assert!(simulate(|_, _| 0.5, 60.0, 0).is_empty());
        // Each change stalls 2.5 s while long-GOP decoders seek: the stall is not taken for the new rung dropping.
        let ch = simulate_with_stall(|level, _| if level == 0 { 0.1 } else { 0.0 }, 120.0, 3, 2.5);
        assert!(ch[0].1 == 1 && ch.iter().all(|c| c.1 <= 1), "{ch:?}");
    }

    /// ffmpeg's -ss gives the first frame at or after it: the seek must land inside the frame before the one on screen
    /// (0.050 s at 29.97 shows frame 1, -ss 0.050 gave frame 2; exactly on frame 2, "0.067" gave frame 3).
    #[test]
    fn trimmer_seeks_to_the_frame_on_screen() {
        let fps = 30000.0 / 1001.0;
        for (t, k) in [(0.0, 0), (0.050, 1), (2.0 / fps, 2), (2.0 / fps - 1e-4, 1), (10.0, 299)] {
            let s = seek_time(t, Some(fps));
            assert!(s > (k as f64 - 1.0) / fps && s <= k as f64 / fps, "{t}: {s}");
            assert_eq!(crate::engine::plan::frame_at(t, fps), k);
        }
        assert_eq!(seek_time(1.25, None), 1.25);
    }

    /// A 50% white pixel over black must show as 128, the compositor's gamma-space premultiplication (egui's gives 188).
    #[test]
    fn decoded_alpha_is_premultiplied_in_gamma_space() {
        use egui::Color32;
        let mut px = [[255, 255, 255, 128], [200, 100, 0, 255], [90, 90, 90, 0], [255, 0, 64, 64]].map(|[r, g, b, a]| Color32::from_rgba_premultiplied(r, g, b, a));
        premultiply(&mut px);
        assert_eq!(px.map(|p| p.to_array()), [[128, 128, 128, 128], [200, 100, 0, 255], [0, 0, 0, 0], [64, 0, 16, 64]]);
    }

    /// R16b: Overlays shows one channel in its colour or as grey, alpha as grey on opaque.
    #[test]
    fn isolate_shows_one_channel() {
        use egui::Color32;
        let img = ColorImage { size: [1, 1], pixels: vec![Color32::from_rgba_premultiplied(100, 60, 20, 200)] };
        let px = |i: Isolate| i.apply(&img).pixels[0].to_array();
        assert_eq!(px(Isolate::None), [100, 60, 20, 200]);
        assert_eq!(px(Isolate::Green), [0, 60, 0, 200]);
        assert_eq!(px(Isolate::BlueGray), [20, 20, 20, 200]);
        assert_eq!(px(Isolate::AlphaGray), [200, 200, 200, 255]);
    }

    #[test]
    fn decode_size_keeps_aspect_and_even() {
        let m = media("a", MediaType::Video);
        assert_eq!(decode_size(&m, 960), (960, 540));
        assert_eq!(decode_size(&m, 4000), (1920, 1080));
        let mut tall = m.clone();
        (tall.width, tall.height) = (Some(1080), Some(1920));
        assert_eq!(decode_size(&tall, 321), (320, 568));
    }
}
