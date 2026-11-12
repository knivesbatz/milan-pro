//! Preview frame server: single RGBA frames decoded by ffmpeg on a worker thread.
use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use crate::project::{MediaItem, MediaType, Project};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Quality {
    Draft,
    Preview,
    Good,
    Best,
}

impl Quality {
    pub const ALL: [Quality; 4] = [Quality::Draft, Quality::Preview, Quality::Good, Quality::Best];

    /// ffmpeg scaler per quality; Draft also decodes at half the size's resolution.
    fn scaler(self) -> &'static str {
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

/// Preview frame width for a project `full_w` wide shown in a `display_w` wide frame.
/// Auto picks the largest of Full / Half / Quarter that fits the display, as VEGAS reports it.
pub fn preview_width(q: Quality, size: Size, full_w: u32, display_w: u32) -> u32 {
    let full = full_w.max(2);
    let w = match size {
        Size::Full => full,
        Size::Half => full / 2,
        Size::Quarter => full / 4,
        Size::Auto => [full, full / 2].into_iter().find(|&w| w <= display_w).unwrap_or(full / 4),
    };
    let w = if q == Quality::Draft { w / 2 } else { w };
    (w / 2 * 2).max(16)
}

struct FrameRequest {
    id: u64,
    path: PathBuf,
    time: f64,
    width: u32,
    height: u32,
    scaler: &'static str,
}

struct FrameResult {
    id: u64,
    image: Result<ColorImage, String>,
}

/// The media and source time visible at timeline time `t`: the topmost visible
/// video track (index 0 is the top) with a picture clip under `t`.
pub fn frame_source(project: &Project, t: f64) -> Option<(&MediaItem, f64)> {
    project.timeline.shown_video_tracks().find_map(|tr| {
        // Later clips are drawn on top in the timeline, so they win here too.
        tr.clips.iter().rev().find_map(|c| {
            if t < c.position || t >= c.position + c.duration {
                return None;
            }
            let id = c.media_id.as_deref()?;
            let m = project.media_pool.iter().find(|m| m.id == id)?;
            match m.media_type {
                MediaType::Video => Some((m, c.in_point + (t - c.position))),
                MediaType::Image => Some((m, 0.0)),
                _ => None,
            }
        })
    })
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
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-v", "error", "-ss", &format!("{:.3}", req.time.max(0.0)), "-i"])
        .arg(&req.path)
        .args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgba", "-sws_flags", req.scaler, "-s"])
        .arg(format!("{}x{}", req.width, req.height))
        .arg("-");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let out = cmd.output().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => "ffmpeg not found on PATH".to_string(),
        _ => format!("ffmpeg failed: {e}"),
    })?;
    let need = (req.width * req.height * 4) as usize;
    if out.stdout.len() < need {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(err.lines().next().unwrap_or("no frame at this time").to_string());
    }
    Ok(ColorImage::from_rgba_unmultiplied(
        [req.width as usize, req.height as usize],
        &out.stdout[..need],
    ))
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
        }
    }

    /// Decode width for a project `full_w` wide at the current quality and size.
    pub fn width(&self, full_w: u32) -> u32 {
        preview_width(self.quality, self.size, full_w, self.display_w)
    }

    /// A requested frame has not arrived yet.
    pub fn busy(&self) -> bool {
        self.done_id.max(self.min_id) < self.next_id
    }

    /// Request a frame if the visible (media, source frame) changed, and take finished frames.
    pub fn update(&mut self, ctx: &egui::Context, project: &Project) {
        let src = frame_source(project, project.timeline.cursor_position);
        self.show(ctx, src, project.settings.frame_rate, project.settings.width);
    }

    /// Like `update`, for an explicit source (the Trimmer's media at its own cursor).
    /// `fps` is the fallback frame rate; `full_w` the width that Size::Full means.
    pub fn show(&mut self, ctx: &egui::Context, src: Option<(&MediaItem, f64)>, fps: f64, full_w: u32) {
        let max_w = self.width(full_w);
        let key = src.map(|(m, t)| {
            let fps = m.frame_rate.filter(|f| *f > 0.0).unwrap_or(fps.max(1.0));
            (m.path.clone(), (t * fps).floor() as i64, max_w, self.quality)
        });
        if key != self.key {
            self.key = key;
            match src {
                Some((m, time)) => {
                    let (width, height) = decode_size(m, max_w);
                    let req = FrameRequest { id: self.next_id, path: m.path.clone(), time, width, height, scaler: self.quality.scaler() };
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
        for r in self.rx.try_iter() {
            if r.id < self.min_id {
                continue;
            }
            got = true;
            self.done_id = self.done_id.max(r.id + 1);
            match r.image {
                Ok(img) => {
                    self.error = None;
                    let img = Arc::new(img);
                    let data = egui::ImageData::Color(img.clone());
                    match &mut self.texture {
                        Some(t) => t.set(data, TextureOptions::LINEAR),
                        None => self.texture = Some(ctx.load_texture("preview_frame", data, TextureOptions::LINEAR)),
                    }
                    self.image = Some(img);
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

/// (media path, source time in ms)
type ThumbKey = (PathBuf, i64);

enum Thumb {
    Pending,
    Ready(TextureHandle),
    Failed,
}

/// Timeline event thumbnails: small frames decoded by ffmpeg on two worker threads, newest request first,
/// so the visible events fill in before anything scrolled away.
pub struct Thumbs {
    queue: Arc<(Mutex<Vec<(ThumbKey, FrameRequest)>>, Condvar)>,
    rx: Receiver<(ThumbKey, Result<ColorImage, String>)>,
    cache: HashMap<ThumbKey, Thumb>,
    ctx: egui::Context,
}

/// Queued requests kept; older ones are dropped (and re-requested if they come back into view).
const THUMB_QUEUE: usize = 96;
/// Decoded thumbnails kept before the cache is cleared (≈160×90 RGBA each).
const THUMB_CACHE: usize = 400;
pub const THUMB_WIDTH: u32 = 160;

impl Thumbs {
    pub fn new(ctx: &egui::Context) -> Self {
        let queue = Arc::new((Mutex::new(Vec::<(ThumbKey, FrameRequest)>::new()), Condvar::new()));
        let (tx, rx) = mpsc::channel();
        for _ in 0..2 {
            let (queue, tx, ctx) = (queue.clone(), tx.clone(), ctx.clone());
            std::thread::spawn(move || {
                loop {
                    let (key, req) = {
                        let (lock, cv) = &*queue;
                        let mut q = cv.wait_while(lock.lock().unwrap(), |q| q.is_empty()).unwrap();
                        q.pop().expect("non-empty queue")
                    };
                    if tx.send((key, decode_frame(&req))).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            });
        }
        Self { queue, rx, cache: HashMap::new(), ctx: ctx.clone() }
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
        let time = if m.media_type == MediaType::Image { 0.0 } else { time.max(0.0) };
        let key = (m.path.clone(), (time * 1000.0).round() as i64);
        if !self.cache.contains_key(&key) {
            if self.cache.len() >= THUMB_CACHE {
                self.cache.retain(|_, v| matches!(v, Thumb::Pending));
            }
            let (width, height) = decode_size(m, THUMB_WIDTH);
            let req = FrameRequest { id: 0, path: m.path.clone(), time, width, height, scaler: "bilinear" };
            let (lock, cv) = &*self.queue;
            let mut q = lock.lock().unwrap();
            q.push((key.clone(), req));
            if q.len() > THUMB_QUEUE {
                let (old, _) = q.remove(0);
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
    use crate::project::{Clip, Track, TrackKind};

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
            sample_rate: None,
            codec: None,
        }
    }

    fn clip(m: &MediaItem, pos: f64, dur: f64, inp: f64) -> Clip {
        let mut c = Clip::from_media(m, pos);
        c.duration = dur;
        c.in_point = inp;
        c.out_point = inp + dur;
        c
    }

    #[test]
    fn topmost_visible_clip_and_source_time() {
        let mut p = Project::new("t");
        let (a, b, img) = (media("a", MediaType::Video), media("b", MediaType::Video), media("i", MediaType::Image));
        p.timeline.video_tracks[0].clips.push(clip(&a, 10.0, 5.0, 2.0));
        p.timeline.video_tracks.push(Track::new("Video 2", TrackKind::Video));
        p.timeline.video_tracks[1].clips.push(clip(&b, 0.0, 20.0, 0.0));
        p.timeline.video_tracks[1].clips.push(clip(&img, 30.0, 5.0, 0.0));
        p.media_pool = vec![a, b, img];

        let (m, t) = frame_source(&p, 11.5).unwrap();
        assert_eq!((m.id.as_str(), t), ("a", 3.5)); // top track wins: 2.0 + (11.5 - 10)
        assert_eq!(frame_source(&p, 16.0).unwrap().0.id, "b"); // past the top clip
        p.timeline.video_tracks[0].visible = false;
        assert_eq!(frame_source(&p, 11.5).unwrap().0.id, "b"); // hidden track ignored
        p.timeline.video_tracks[0].visible = true;
        p.timeline.video_tracks[0].muted = true;
        assert_eq!(frame_source(&p, 11.5).unwrap().0.id, "b"); // muted track ignored
        p.timeline.video_tracks[0].muted = false;
        p.timeline.video_tracks[1].solo = true;
        assert_eq!(frame_source(&p, 11.5).unwrap().0.id, "b"); // solo isolates track 2
        p.timeline.video_tracks[1].solo = false;
        p.timeline.video_tracks[0].visible = false;
        assert_eq!(frame_source(&p, 32.0).map(|(m, t)| (m.id.as_str(), t)), Some(("i", 0.0)));
        assert!(frame_source(&p, 25.0).is_none()); // gap: black
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
