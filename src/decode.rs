//! Preview frame server: single RGBA frames decoded by ffmpeg on a worker thread.
use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{self, Receiver, Sender};
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

    pub fn max_width(self) -> u32 {
        match self {
            Quality::Draft => 320,
            Quality::Preview => 640,
            Quality::Good => 960,
            Quality::Best => 1920,
        }
    }
}

struct FrameRequest {
    id: u64,
    path: PathBuf,
    time: f64,
    width: u32,
    height: u32,
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
        .args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgba", "-s"])
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
    key: Option<(PathBuf, i64, u32)>,
    next_id: u64,
    /// Results older than this were requested before the preview went black.
    min_id: u64,
    pub texture: Option<TextureHandle>,
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
            quality: Quality::Good,
            key: None,
            next_id: 0,
            min_id: 0,
            texture: None,
            error: None,
        }
    }

    /// Request a frame if the visible (media, source frame) changed, and take finished frames.
    pub fn update(&mut self, ctx: &egui::Context, project: &Project) {
        let max_w = self.quality.max_width();
        let src = frame_source(project, project.timeline.cursor_position);
        let key = src.map(|(m, t)| {
            let fps = m.frame_rate.filter(|f| *f > 0.0).unwrap_or(project.settings.frame_rate.max(1.0));
            (m.path.clone(), (t * fps).floor() as i64, max_w)
        });
        if key != self.key {
            self.key = key;
            match src {
                Some((m, time)) => {
                    let (width, height) = decode_size(m, max_w);
                    let req = FrameRequest { id: self.next_id, path: m.path.clone(), time, width, height };
                    self.next_id += 1;
                    let _ = self.tx.send(req);
                }
                None => {
                    self.min_id = self.next_id;
                    self.texture = None;
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
            match r.image {
                Ok(img) => {
                    self.error = None;
                    match &mut self.texture {
                        Some(t) => t.set(img, TextureOptions::LINEAR),
                        None => self.texture = Some(ctx.load_texture("preview_frame", img, TextureOptions::LINEAR)),
                    }
                }
                Err(e) => {
                    self.error = Some(e);
                    self.texture = None;
                }
            }
        }
        if got {
            // Polled after drawing, so show the new frame next pass.
            ctx.request_repaint();
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
    fn decode_size_keeps_aspect_and_even() {
        let m = media("a", MediaType::Video);
        assert_eq!(decode_size(&m, 960), (960, 540));
        assert_eq!(decode_size(&m, 4000), (1920, 1080));
        let mut tall = m.clone();
        (tall.width, tall.height) = (Some(1080), Some(1920));
        assert_eq!(decode_size(&tall, 321), (320, 568));
    }
}
