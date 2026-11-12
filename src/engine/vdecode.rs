//! Persistent ffmpeg video decoders: raw RGBA frames streamed over a pipe and read forward.
//! A request near a stream's position reads on (dropping frames); anything else spawns a new stream.
use eframe::egui::{Color32, ColorImage};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, Stdio};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use crate::decode::Quality;
use crate::engine::plan::MediaRef;

/// At most this many decoders run at once; the least recently used goes first.
const MAX_STREAMS: usize = 8;
/// A decoder unused this long is closed.
const IDLE: Duration = Duration::from_secs(5);
/// Playback pre-warm reuses a stream only this many frames short of the target: reading farther misses the deadline.
const JUMP: i64 = 4;

pub struct DecSpec<'a> {
    pub path: &'a Path,
    pub sf: i64,
    /// Frames decoded before `sf` and dropped, so the seek cannot skip the source frame still on screen at `sf`.
    pub lead: i64,
    pub fps_str: &'a str,
    pub fps: f64,
    pub w: u32,
    pub h: u32,
    pub scaler: &'static str,
    pub hw: bool,
    pub matrix: Option<&'static str>,
    pub interlaced: bool,
}

/// ffmpeg arguments (after `-nostdin`) streaming frames `sf - lead`, ... on the project-fps grid.
/// Each frame is the latest source frame at or before its time (`round=up`), as VEGAS shows a 25p clip in a 29.97 project;
/// `-ss` alone would skip that frame whenever the seek time falls inside it.
pub fn args(s: &DecSpec) -> Vec<String> {
    let mut a: Vec<String> = ["-v", "error", "-threads", "2"].map(String::from).to_vec();
    if s.hw {
        a.extend(["-hwaccel", "auto"].map(String::from));
    }
    a.extend(["-ss".into(), format!("{:.6}", (s.sf - s.lead) as f64 / s.fps), "-i".into(), s.path.to_string_lossy().into_owned()]);
    a.extend(["-map", "0:v:0", "-an", "-sn", "-dn", "-vf"].map(String::from));
    a.push(format!(
        "{}fps={}:round=up:start_time=0,scale={}:{}:flags={}:in_color_matrix={}:out_range=full,format=rgba",
        if s.interlaced { "yadif=deint=interlaced," } else { "" },
        s.fps_str,
        s.w,
        s.h,
        s.scaler,
        s.matrix.unwrap_or("auto"),
    ));
    a.extend(["-f", "rawvideo", "-"].map(String::from));
    a
}

struct VideoStream {
    media: Arc<str>,
    w: u32,
    h: u32,
    scaler: &'static str,
    /// Reads the media's proxy (Draft and Preview), not the source.
    proxy: bool,
    /// `MediaRef::grid` it decodes on.
    grid: f64,
    hw: bool,
    /// The frame the next read returns.
    next_sf: i64,
    rx: Receiver<Vec<Color32>>,
    child: Child,
    /// Last stderr line, once ffmpeg has exited.
    err: Option<JoinHandle<String>>,
    used: Instant,
    last: Option<Arc<ColorImage>>,
    /// Opened by a late playback request (not a pre-warm): while it has no frame, it is waited on instead of respawned.
    chase: bool,
}

impl Drop for VideoStream {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn(spec: &DecSpec, media: Arc<str>, has_alpha: bool, proxy: bool, grid: f64) -> std::io::Result<VideoStream> {
    let mut cmd = crate::proc::ffmpeg();
    cmd.args(args(spec)).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = crate::proc::spawn(&mut cmd)?;
    let (mut out, err) = (child.stdout.take().expect("piped stdout"), child.stderr.take().expect("piped stderr"));
    let (tx, rx) = mpsc::sync_channel(4);
    let (n, mut lead) = (spec.w as usize * spec.h as usize, spec.lead);
    std::thread::spawn(move || {
        loop {
            let mut px = vec![Color32::TRANSPARENT; n];
            if out.read_exact(bytemuck::cast_slice_mut(&mut px)).is_err() {
                break;
            }
            if lead > 0 {
                lead -= 1;
                continue;
            }
            if has_alpha {
                crate::decode::premultiply(&mut px);
            }
            if tx.send(px).is_err() {
                break;
            }
        }
    });
    let err = std::thread::spawn(move || BufReader::new(err).lines().map_while(Result::ok).filter(|l| !l.trim().is_empty()).last().unwrap_or_default());
    Ok(VideoStream { media, w: spec.w, h: spec.h, scaler: spec.scaler, proxy, grid, hw: spec.hw, next_sf: spec.sf, rx, child, err: Some(err), used: Instant::now(), last: None, chase: false })
}

pub enum Pull {
    Exact(Arc<ColorImage>),
    /// Late (or past the end of the media): the stream's last frame, if it has one.
    Held(Option<Arc<ColorImage>>),
    Err(String),
}

#[derive(Default)]
pub struct DecoderPool {
    streams: Vec<VideoStream>,
    hw_failed: HashSet<Arc<str>>,
    /// Last decodable frame of a media (and whether read from its proxy, which may end sooner, and its grid) whose video
    /// ends before its container duration (which counts the audio).
    ends: HashMap<(Arc<str>, bool, u64), i64>,
    fps: f64,
    fps_str: String,
    /// Frames read past and late layers, for the stats.
    pub dropped: u64,
}

fn end_key(m: &MediaRef, q: Quality) -> (Arc<str>, bool, u64) {
    (m.id.clone(), m.proxy_for(q).is_some(), m.grid.to_bits())
}

fn image(px: Vec<Color32>, w: u32, h: u32) -> Arc<ColorImage> {
    Arc::new(ColorImage { size: [w as usize, h as usize], pixels: px })
}

impl DecoderPool {
    /// The project frame rate; a change closes every stream.
    pub fn set_rate(&mut self, fps: f64, fps_str: &str) {
        if fps != self.fps || fps_str != self.fps_str {
            self.streams.clear();
            self.ends.clear();
            (self.fps, self.fps_str) = (fps, fps_str.to_string());
        }
    }

    /// `sf`, held at the last video frame `q` reads (source or proxy) once that is known: frames past it show the last one.
    pub fn clamp(&self, m: &MediaRef, sf: i64, q: Quality) -> i64 {
        self.ends.get(&end_key(m, q)).map_or(sf, |&e| sf.min(e))
    }

    fn reach(&self) -> i64 {
        (2.0 * self.fps).ceil() as i64
    }

    pub fn live(&self) -> usize {
        self.streams.len()
    }

    /// Close streams idle for 5 s.
    pub fn prune(&mut self) {
        self.streams.retain(|s| s.used.elapsed() < IDLE);
    }

    /// Close the streams of media `id` and forget its end (the file changed on disk).
    pub fn forget(&mut self, id: &str) {
        self.streams.retain(|s| &*s.media != id);
        self.ends.retain(|k, _| &*k.0 != id);
    }

    /// The frame rate (and its ffmpeg form) `m`'s frame numbers count in.
    fn grid(&self, m: &MediaRef) -> (f64, String) {
        if m.grid > 0.0 { (m.grid, crate::render::fps_str(m.grid)) } else { (self.fps, self.fps_str.clone()) }
    }

    /// A stream decoding the picture `q` asks for: this media, size, scaler, file (proxy or source) and grid.
    fn same(s: &VideoStream, m: &MediaRef, w: u32, h: u32, q: Quality) -> bool {
        s.media == m.id && (s.w, s.h, s.scaler, s.proxy, s.grid) == (w, h, q.scaler(), m.proxy_for(q).is_some(), m.grid)
    }

    /// The stream that reaches `sf` reading forward the fewest frames, at most `reach`.
    fn find(&self, m: &MediaRef, sf: i64, w: u32, h: u32, q: Quality, reach: i64) -> Option<usize> {
        let s = &self.streams;
        (0..s.len()).filter(|&i| Self::same(&s[i], m, w, h, q) && (0..=reach).contains(&(sf - s[i].next_sf))).min_by_key(|&i| sf - s[i].next_sf)
    }

    /// Hardware decoding for heavy sources (HEVC, AV1, 2560 px and up) during playback, until it fails once.
    fn wants_hw(&self, m: &MediaRef) -> bool {
        !self.hw_failed.contains(&m.id) && (m.w >= 2560 || matches!(m.codec.as_deref(), Some("hevc" | "av1")))
    }

    fn open(&mut self, m: &MediaRef, sf: i64, w: u32, h: u32, q: Quality, hw: bool) -> Result<usize, String> {
        let (fps, fps_str) = self.grid(m);
        // Enough grid frames back to start inside the previous source frame (2 for 25p in 29.97).
        let per = if m.fps > 0.0 { (fps / m.fps - 1e-9).ceil() as i64 } else { 2 };
        let lead = per.clamp(1, 30).min(sf);
        // A proxy keeps the source's frame times and colour matrix; it is light H.264, so never hardware decoded.
        let proxy = m.proxy_for(q);
        let (path, hw) = (proxy.unwrap_or(&m.path), hw && proxy.is_none());
        // The proxy was deinterlaced when it was made.
        let interlaced = m.interlaced && proxy.is_none();
        let spec = DecSpec { path, sf, lead, fps_str: &fps_str, fps, w, h, scaler: q.scaler(), hw, matrix: m.matrix, interlaced };
        let s = spawn(&spec, m.id.clone(), m.has_alpha, proxy.is_some(), m.grid).map_err(|e| crate::proc::tool_error(&e, "ffmpeg"))?;
        if self.streams.len() >= MAX_STREAMS {
            if let Some(i) = (0..self.streams.len()).min_by_key(|&i| self.streams[i].used) {
                self.streams.remove(i);
            }
        }
        self.streams.push(s);
        Ok(self.streams.len() - 1)
    }

    /// Start a stream at `sf` ahead of time unless one can reach it in time: playing (`play`, hardware decoding allowed),
    /// within a few frames, so a jump cut inside one file gets its own stream; paused, within 2 s.
    pub fn prewarm(&mut self, m: &MediaRef, sf: i64, w: u32, h: u32, q: Quality, play: bool) {
        let sf = self.clamp(m, sf, q);
        let reach = if play { JUMP } else { self.reach() };
        if self.find(m, sf, w, h, q, reach).is_none() {
            let _ = self.open(m, sf, w, h, q, play && self.wants_hw(m));
        }
    }

    /// Frame `sf` of `m` at `w` x `h`. With a deadline (playback) a late stream gives its last frame instead.
    pub fn frame(&mut self, m: &MediaRef, sf: i64, w: u32, h: u32, q: Quality, deadline: Option<Instant>) -> Pull {
        let sf = self.clamp(m, sf, q);
        let i = match self.find(m, sf, w, h, q, self.reach()) {
            Some(i) => i,
            None => {
                // Playing, a stream still starting for this picture (a reversed event asks for an earlier frame each time)
                // is waited on rather than respawned every frame; its frame shows as held, about 5 updates a second.
                let starting = self.streams.iter().position(|s| s.chase && s.last.is_none() && Self::same(s, m, w, h, q));
                if let (Some(d), Some(j)) = (deadline, starting) {
                    self.streams[j].used = Instant::now();
                    return match self.streams[j].rx.recv_timeout(d.saturating_duration_since(Instant::now())) {
                        Ok(px) => {
                            self.streams.remove(j);
                            Pull::Held(Some(image(px, w, h)))
                        }
                        Err(RecvTimeoutError::Timeout) => {
                            self.dropped += 1;
                            Pull::Held(None)
                        }
                        Err(RecvTimeoutError::Disconnected) => {
                            self.streams.remove(j);
                            Pull::Held(None)
                        }
                    };
                }
                match self.open(m, sf, w, h, q, deadline.is_some() && self.wants_hw(m)) {
                    Ok(i) => {
                        self.streams[i].chase = deadline.is_some();
                        i
                    }
                    Err(e) => return Pull::Err(e),
                }
            }
        };
        let s = &mut self.streams[i];
        s.used = Instant::now();
        loop {
            let got = match deadline {
                Some(d) => s.rx.recv_timeout(d.saturating_duration_since(Instant::now())),
                None => s.rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            match got {
                Ok(px) => {
                    if s.next_sf < sf {
                        self.dropped += 1;
                    }
                    let img = image(px, w, h);
                    s.next_sf += 1;
                    s.last = Some(img.clone());
                    if s.next_sf > sf {
                        return Pull::Exact(img);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    self.dropped += 1;
                    return Pull::Held(s.last.clone());
                }
                Err(RecvTimeoutError::Disconnected) => {
                    let _ = s.child.wait();
                    let err = s.err.take().and_then(|j| j.join().ok()).unwrap_or_default();
                    let (hw, proxy, last, end) = (s.hw, s.proxy, s.last.clone(), s.next_sf - 1);
                    self.streams.remove(i);
                    if !err.is_empty() {
                        if hw && last.is_none() {
                            self.hw_failed.insert(m.id.clone());
                            return self.frame(m, sf, w, h, q, deadline);
                        }
                        if proxy && last.is_none() {
                            // A deleted or unreadable proxy: forget it, so the replan reads the source.
                            crate::proxy::forget(&m.path);
                        }
                        return last.map_or(Pull::Err(err), |img| Pull::Held(Some(img)));
                    }
                    // A clean end before `sf`: the video is shorter than the media. Its last frame is what `sf` shows.
                    return match last {
                        Some(img) => {
                            self.ends.insert(end_key(m, q), end);
                            Pull::Exact(img)
                        }
                        None => self.tail(m, sf, w, h, q),
                    };
                }
            }
        }
    }

    /// A stream at `sf` ended cleanly without a frame: read on from 1, 2, 4... s earlier (down to the start) to find
    /// the video's last frame, however far before `sf` it is (a short cover picture in a long audio file).
    fn tail(&mut self, m: &MediaRef, sf: i64, w: u32, h: u32, q: Quality) -> Pull {
        let fps = self.grid(m).0;
        let none = format!("No frame at {:.3} s", sf as f64 / fps);
        let mut back = (fps.ceil() as i64).max(1);
        loop {
            let from = (sf - back).max(0);
            if from == sf {
                return Pull::Err(none);
            }
            let i = match self.open(m, from, w, h, q, false) {
                Ok(i) => i,
                Err(e) => return Pull::Err(e),
            };
            let s = &mut self.streams[i];
            let mut last = None;
            while s.next_sf <= sf {
                let Ok(px) = s.rx.recv() else { break };
                (last, s.next_sf) = (Some(px), s.next_sf + 1);
            }
            let end = (s.next_sf <= sf).then_some(s.next_sf - 1);
            self.streams.remove(i);
            if let Some(px) = last {
                if let Some(e) = end {
                    self.ends.insert(end_key(m, q), e);
                }
                return Pull::Exact(image(px, w, h));
            }
            if from == 0 {
                return Pull::Err(none);
            }
            back *= 2;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(path: &Path) -> DecSpec<'_> {
        DecSpec { path, sf: 90, lead: 1, fps_str: "30000/1001", fps: 30000.0 / 1001.0, w: 960, h: 540, scaler: "fast_bilinear", hw: false, matrix: Some("bt709"), interlaced: false }
    }

    #[test]
    fn decoder_argv() {
        let p = Path::new(r"C:\clips\a b.mp4");
        assert_eq!(
            args(&spec(p)).join(" "),
            r"-v error -threads 2 -ss 2.969633 -i C:\clips\a b.mp4 -map 0:v:0 -an -sn -dn -vf fps=30000/1001:round=up:start_time=0,scale=960:540:flags=fast_bilinear:in_color_matrix=bt709:out_range=full,format=rgba -f rawvideo -"
        );
        let a = args(&DecSpec { hw: true, interlaced: true, matrix: None, ..spec(p) });
        assert_eq!(a[4..6], ["-hwaccel", "auto"]);
        assert!(a.iter().any(|x| x.starts_with("yadif=deint=interlaced,fps=") && x.contains("in_color_matrix=auto")));
    }

    #[test]
    fn hwaccel_only_for_heavy_sources() {
        let pool = DecoderPool::default();
        let m = |codec: &str, w| MediaRef {
            id: "m".into(),
            path: "m".into(),
            w,
            h: 1080,
            duration: 1.0,
            fps: 30.0,
            still: false,
            has_alpha: false,
            matrix: None,
            codec: Some(codec.into()),
            interlaced: false,
            proxy: None,
            grid: 0.0,
        };
        assert!(!pool.wants_hw(&m("h264", 1920)));
        assert!(pool.wants_hw(&m("hevc", 1920)) && pool.wants_hw(&m("av1", 1920)) && pool.wants_hw(&m("h264", 2560)));
        let mut pool = pool;
        pool.hw_failed.insert("m".into());
        assert!(!pool.wants_hw(&m("hevc", 1920)));
    }

    /// One-frame decode at the same -ss, the reference a stream must match.
    fn single(path: &Path, t: f64) -> u8 {
        let mut cmd = crate::proc::ffmpeg();
        cmd.args(["-v", "error", "-ss", &format!("{t:.6}"), "-i"]).arg(path).args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", "64x64", "-"]);
        crate::proc::output(&mut cmd).unwrap().stdout[0]
    }

    /// Gray level of frame `n` of the test clip (luma 16 + 4n mod 220, limited range).
    fn level(n: i64) -> f64 {
        (4 * n % 220) as f64 * 255.0 / 219.0
    }

    /// Needs ffmpeg. `cargo test --release -- --ignored frame_accuracy`
    #[test]
    #[ignore]
    fn frame_accuracy() {
        let dir = std::env::temp_dir().join(format!("milan-vdec-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (rate, name) in [("30000/1001", "ntsc.mp4"), ("25", "pal.mp4")] {
            let path = dir.join(name);
            let src = format!("nullsrc=s=64x64:r={rate}:d=12,geq=lum='mod(N*4,220)+16':cb=128:cr=128,format=yuv420p");
            let ok = crate::proc::ffmpeg()
                .args(["-v", "error", "-y", "-f", "lavfi", "-i", &src, "-c:v", "libx264", "-qp", "0", "-g", "48"])
                .arg(&path)
                .status()
                .unwrap()
                .success();
            assert!(ok);
            let m = MediaRef { id: name.into(), path: path.clone(), w: 64, h: 64, duration: 12.0, fps: if name == "pal.mp4" { 25.0 } else { 30000.0 / 1001.0 }, still: false, has_alpha: false, matrix: None, codec: None, interlaced: false, proxy: None, grid: 0.0 };
            let mut pool = DecoderPool::default();
            let fps = 30000.0 / 1001.0;
            pool.set_rate(fps, "30000/1001");
            let src_fps = if name == "pal.mp4" { 25.0 } else { fps };
            for start in [0i64, 37, 150, 299] {
                for sf in start..start + 8 {
                    let Pull::Exact(img) = pool.frame(&m, sf, 64, 64, Quality::Best, None) else { panic!("{name} {sf}: no frame") };
                    let got = img.pixels[64 * 32 + 32].r();
                    let t = sf as f64 / fps;
                    // The source frame on screen at t.
                    let want = level((t * src_fps + 1e-6).floor() as i64);
                    assert!((got as f64 - want).abs() <= 1.5, "{name} sf {sf}: stream {got}, want {want:.1}");
                    if src_fps == fps {
                        assert!(got.abs_diff(single(&path, t)) <= 1, "{name} sf {sf}: stream {got} vs single-frame decode");
                    }
                }
            }
            assert_eq!(pool.live(), 3, "{name}: 37 reads on from the first stream; 150 and 299 are too far ahead");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A 64x64 gray-ramp clip of `secs` s at 30 fps (`level(n)` on frame n), with `audio` s of sound.
    fn ramp(dir: &Path, secs: u32, audio: f64) -> MediaRef {
        let path = dir.join(format!("ramp{secs}.mp4"));
        let src = format!("nullsrc=s=64x64:r=30:d={secs},geq=lum='mod(N*4,220)+16':cb=128:cr=128,format=yuv420p");
        let ok = crate::proc::ffmpeg()
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", &src, "-f", "lavfi", "-i", &format!("sine=duration={audio}")])
            .args(["-c:v", "libx264", "-qp", "0", "-g", "30", "-c:a", "aac"])
            .arg(&path)
            .status()
            .unwrap()
            .success();
        assert!(ok);
        // shortcut: no hardware decoding here (DXVA rejects 64x64); that a clean end is not a hardware failure is read from the code.
        MediaRef { path, duration: audio, ..ramp_ref() }
    }

    /// A 64x64 30 fps video, for the ramp clips.
    fn ramp_ref() -> MediaRef {
        MediaRef { id: "ramp".into(), path: "".into(), w: 64, h: 64, duration: 1.0, fps: 30.0, still: false, has_alpha: false, matrix: None, codec: None, interlaced: false, proxy: None, grid: 0.0 }
    }

    fn gray(p: Pull) -> Option<u8> {
        match p {
            Pull::Exact(img) | Pull::Held(Some(img)) => Some(img.pixels[64 * 32 + 32].r()),
            Pull::Held(None) => None,
            Pull::Err(e) => panic!("{e}"),
        }
    }

    /// Needs ffmpeg. The container lasts 1.2 s (audio) but the video 1 s: later frames show the last video frame.
    #[test]
    #[ignore]
    fn past_the_video_end_holds_the_last_frame() {
        let dir = std::env::temp_dir().join(format!("milan-vend-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let m = ramp(&dir, 1, 1.2);
        let last = level(29);
        // Paused seeks past the end.
        let mut pool = DecoderPool::default();
        pool.set_rate(30.0, "30");
        for sf in [33, 31, 35] {
            let g = gray(pool.frame(&m, sf, 64, 64, Quality::Best, None)).unwrap();
            assert!((g as f64 - last).abs() <= 1.5, "sf {sf}: {g} vs {last:.1}");
        }
        assert_eq!(pool.clamp(&m, 35, Quality::Best), 29);
        // Playing into the end.
        let mut pool = DecoderPool::default();
        pool.set_rate(30.0, "30");
        for sf in 20..36 {
            let g = gray(pool.frame(&m, sf, 64, 64, Quality::Best, Some(Instant::now() + Duration::from_secs(5)))).unwrap();
            assert!((g as f64 - level(sf.min(29))).abs() <= 1.5, "sf {sf}: {g}");
        }
        assert_eq!(pool.clamp(&m, 35, Quality::Best), 29);
        // A proxy's end is its own: the source is not held at it.
        let mut m = m;
        m.proxy = Some("p.mp4".into());
        assert_eq!(pool.clamp(&m, 35, Quality::Preview), 35);
        assert_eq!(pool.clamp(&m, 35, Quality::Good), 29);
        drop(pool);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Needs ffmpeg. An audiogram-style file: 0.5 s of video in 5 s of audio. A paused seek far past the video end
    /// (more than a second) still finds and holds the last frame.
    #[test]
    #[ignore]
    fn far_past_a_short_video_holds_the_last_frame() {
        let dir = std::env::temp_dir().join(format!("milan-vfar-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("short.mp4");
        let ok = crate::proc::ffmpeg()
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", "nullsrc=s=64x64:r=30:d=0.5,geq=lum='mod(N*4,220)+16':cb=128:cr=128,format=yuv420p"])
            .args(["-f", "lavfi", "-i", "sine=duration=5", "-c:v", "libx264", "-qp", "0", "-c:a", "aac"])
            .arg(&path)
            .status()
            .unwrap()
            .success();
        assert!(ok);
        let m = MediaRef { path, duration: 5.0, ..ramp_ref() };
        for sf in [90, 60, 140] {
            let mut pool = DecoderPool::default();
            pool.set_rate(30.0, "30");
            let g = gray(pool.frame(&m, sf, 64, 64, Quality::Best, None)).unwrap();
            assert!((g as f64 - level(14)).abs() <= 1.5, "sf {sf}: {g}");
            assert_eq!(pool.clamp(&m, sf, Quality::Best), 14);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Needs ffmpeg. Reversed playback at 30 fps: one stream starts at a time and the picture keeps updating.
    #[test]
    #[ignore]
    fn reverse_playback_starts_one_stream_at_a_time() {
        let dir = std::env::temp_dir().join(format!("milan-vrev-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let m = ramp(&dir, 10, 10.0);
        let mut pool = DecoderPool::default();
        pool.set_rate(30.0, "30");
        let (mut shown, start) = (0, Instant::now());
        for (k, sf) in (100..250).rev().enumerate() {
            let due = start + Duration::from_secs_f64(k as f64 / 30.0);
            std::thread::sleep(due.saturating_duration_since(Instant::now()));
            let d = (k > 0).then(|| Instant::now() + Duration::from_millis(25));
            shown += gray(pool.frame(&m, sf, 64, 64, Quality::Best, d)).is_some() as u32;
            // The first (paused-style) stream plus at most one starting.
            assert!(pool.live() <= 2, "frame {k}: {} streams", pool.live());
        }
        println!("reverse: {shown} of 150 frames had a picture");
        assert!(shown >= 10, "{shown}");
        drop(pool);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
