//! Persistent ffmpeg video decoders: raw RGBA frames streamed over a pipe and read forward.
//! A request near a stream's position reads on (dropping frames); anything else spawns a new stream.
//! R20: a picture read backwards (reversed events, J shuttle, dragging left) comes from chunks decoded forward that
//! end at the frame asked for, kept while they are read, with the chunks before them decoding ahead.
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
/// R20: the chunks read backwards hold at most this many bytes of frames together (a chunk is at most 1 s, and
/// shorter for big pictures: 30 frames at 960x540, 10 at 1920x1080; only pictures over 85 MB, 8K, pass it with
/// one frame per chunk), and at most `MAX_STREAMS` chunks are open.
// shortcut: one budget for every picture read backwards; two reversed layers at once share it and may evict each
// other's read-ahead (a stutter, never a wrong frame). Give each its own share if that shows.
const BACK_BYTES: usize = 256 << 20;
/// Chunks decoding ahead of the one being read backwards.
const BACK_AHEAD: usize = 2;
/// Frames in the chunk of a single paused step back.
const STEP_BACK: i64 = 8;

/// Decoder threads for a source stream the UI waits on (a paused seek, a chunk read backwards): seeking long-GOP
/// media decodes from the keyframe before the target, about 1.5x as fast with 4 threads as with 2. Playback and
/// render streams (several at once, reading on) and proxies (a keyframe every 15 frames, where more threads only
/// cost their start-up) keep 2.
fn seek_threads() -> u32 {
    std::thread::available_parallelism().map_or(2, |n| (n.get() as u32 / 4).clamp(2, 4))
}

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
    /// The deinterlace filter ahead of the rate filter ("" for a progressive source or no deinterlacing).
    pub deint: &'static str,
    /// The video stream (`-map 0:v:N`).
    pub stream: u32,
    /// Optical Flow: motion-interpolated frames (minterpolate) instead of the latest source frame.
    pub flow: bool,
    /// Decoder threads.
    pub threads: u32,
    /// Stop after this many frames (`lead` included); None streams to the end.
    pub frames: Option<i64>,
}

/// ffmpeg arguments (after `-nostdin`) streaming frames `sf - lead`, ... on the project-fps grid.
/// Each frame is the latest source frame at or before its time (`round=up`), as VEGAS shows a 25p clip in a 29.97 project;
/// `-ss` alone would skip that frame whenever the seek time falls inside it. With Optical Flow the frames between are
/// motion-interpolated instead.
// shortcut: minterpolate runs on the CPU at a few 1080p frames a second (VEGAS's Optical Flow is slow too); move it to
// the GPU path (R21) if real-time Optical Flow previews are wanted.
pub fn args(s: &DecSpec) -> Vec<String> {
    let mut a: Vec<String> = vec!["-v".into(), "error".into(), "-threads".into(), s.threads.to_string()];
    if s.hw {
        a.extend(["-hwaccel", "auto"].map(String::from));
    }
    a.extend(["-ss".into(), format!("{:.6}", (s.sf - s.lead) as f64 / s.fps), "-i".into(), s.path.to_string_lossy().into_owned()]);
    a.extend(["-map".into(), format!("0:v:{}", s.stream)]);
    a.extend(["-an", "-sn", "-dn", "-vf"].map(String::from));
    let rate = if s.flow { format!("minterpolate=fps={}:mi_mode=mci", s.fps_str) } else { format!("fps={}:round=up:start_time=0", s.fps_str) };
    a.push(format!(
        "{}{rate},scale={}:{}:flags={}:in_color_matrix={}:out_range=full,format=rgba",
        s.deint,
        s.w,
        s.h,
        s.scaler,
        s.matrix.unwrap_or("auto"),
    ));
    if let Some(n) = s.frames {
        a.extend(["-frames:v".into(), n.to_string()]);
    }
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
}

/// R20: frames `from..=to` of one picture, decoded forward and kept to be read backwards; `s.next_sf` is the next one
/// to arrive and `frames` holds `from..s.next_sf`.
struct Chunk {
    s: VideoStream,
    from: i64,
    to: i64,
    frames: Vec<Arc<ColorImage>>,
    /// The last frame read from it (None before the first).
    at: Option<i64>,
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
    // A chunk (`frames` set) is read whole while nobody waits on it: room for all of it.
    let (tx, rx) = mpsc::sync_channel(spec.frames.map_or(4, |n| (n - spec.lead).max(1) as usize));
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
    Ok(VideoStream { media, w: spec.w, h: spec.h, scaler: spec.scaler, proxy, grid, hw: spec.hw, next_sf: spec.sf, rx, child, err: Some(err), used: Instant::now(), last: None })
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
    /// R20: chunks for reading backwards.
    chunks: Vec<Chunk>,
    hw_failed: HashSet<Arc<str>>,
    /// Last decodable frame of a media (and whether read from its proxy, which may end sooner, and its grid) whose video
    /// ends before its container duration (which counts the audio).
    ends: HashMap<(Arc<str>, bool, u64), i64>,
    fps: f64,
    fps_str: String,
    /// Frames read past and late layers, for the stats.
    pub dropped: u64,
    /// R20: the frames asked for are paused seeks the UI waits on (the Video Preview's Show): new source streams
    /// decode with `seek_threads`.
    pub seeking: bool,
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
            self.chunks.clear();
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
        self.streams.len() + self.chunks.len()
    }

    /// Close streams and chunks idle for 5 s.
    pub fn prune(&mut self) {
        self.streams.retain(|s| s.used.elapsed() < IDLE);
        self.chunks.retain(|c| c.s.used.elapsed() < IDLE);
    }

    /// Close the streams of media `id` and forget its end (the file changed on disk).
    pub fn forget(&mut self, id: &str) {
        self.streams.retain(|s| &*s.media != id);
        self.chunks.retain(|c| &*c.s.media != id);
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

    /// A stream of `m` from `sf` (`frames` of them, if set: a chunk). `play` streams may decode on the GPU; chunks and
    /// paused seeks (`seeking`) of a source decode with `seek_threads`, the rest with 2.
    fn start(&self, m: &MediaRef, sf: i64, w: u32, h: u32, q: Quality, play: bool, frames: Option<i64>) -> Result<VideoStream, String> {
        let (fps, fps_str) = self.grid(m);
        // Enough grid frames back to start inside the previous source frame (2 for 25p in 29.97).
        let per = if m.fps > 0.0 { (fps / m.fps - 1e-9).ceil() as i64 } else { 2 };
        let lead = per.clamp(1, 30).min(sf);
        // A proxy keeps the source's frame times and colour matrix; it is light H.264, so never hardware decoded.
        let proxy = m.proxy_for(q);
        let (path, hw) = (proxy.unwrap_or(&m.path), play && self.wants_hw(m) && proxy.is_none());
        // The proxy was deinterlaced when it was made.
        let deint = if proxy.is_none() { m.deint } else { "" };
        let threads = if proxy.is_none() && (frames.is_some() || (self.seeking && !play)) { seek_threads() } else { 2 };
        let frames = frames.map(|n| n + lead);
        let spec = DecSpec { path, sf, lead, fps_str: &fps_str, fps, w, h, scaler: q.scaler(), hw, matrix: m.matrix, deint, stream: m.stream, flow: m.flow, threads, frames };
        spawn(&spec, m.id.clone(), m.has_alpha, proxy.is_some(), m.grid).map_err(|e| crate::proc::tool_error(&e, "ffmpeg"))
    }

    fn open(&mut self, m: &MediaRef, sf: i64, w: u32, h: u32, q: Quality, play: bool) -> Result<usize, String> {
        let s = self.start(m, sf, w, h, q, play, None)?;
        if self.streams.len() >= MAX_STREAMS {
            if let Some(i) = (0..self.streams.len()).min_by_key(|&i| self.streams[i].used) {
                self.streams.remove(i);
            }
        }
        self.streams.push(s);
        Ok(self.streams.len() - 1)
    }

    /// R20: frames per chunk at `w` x `h`: 1 s, or less so that a chunk and those decoding ahead fit `BACK_BYTES`.
    fn chunk_len(&self, m: &MediaRef, w: u32, h: u32) -> i64 {
        let fit = BACK_BYTES / (BACK_AHEAD + 1) / (w as usize * h as usize * 4).max(1);
        (fit as i64).clamp(1, self.grid(m).0.ceil().max(1.0) as i64)
    }

    /// R20: the chunk holding `sf`, or a new one ending at `sf` when the picture is being read backwards (the frame read
    /// last, by a stream or from a chunk, is less than 2 s after `sf`). Chunks read past close; while `playing`, or
    /// when the frame read last came from a chunk or is a few frames on, the `BACK_AHEAD` before this one decode
    /// ahead (a single paused jump back reads one short chunk only).
    fn chunk(&mut self, m: &MediaRef, sf: i64, w: u32, h: u32, q: Quality, playing: bool) -> Option<usize> {
        let same = |s: &VideoStream| Self::same(s, m, w, h, q);
        // The frame read last, and whether it came from a chunk (then reading backwards goes on).
        let streams = self.streams.iter().filter(|s| same(s)).map(|s| (s.used, s.next_sf - 1, false));
        let chunks = self.chunks.iter().filter(|c| same(&c.s)).filter_map(|c| Some((c.s.used, c.at?, true)));
        let last = streams.chain(chunks).max_by_key(|r| r.0);
        let reach = self.reach();
        // A chunk read just before `sf` goes on forward: another layer of the file (a dissolve inside one recording)
        // may have read last, ahead of this one.
        let on = self.chunks.iter().any(|c| same(&c.s) && c.at.is_some_and(|at| (1..=reach).contains(&(sf - at))));
        // The chunk's end, and whether it is now read forward (played on from where a drag left off).
        let (to, forward) = match self.chunks.iter().find(|c| same(&c.s) && (c.from..=c.to).contains(&sf)) {
            Some(c) => (c.to, c.at.is_some_and(|at| at < sf)),
            None if !on && last.is_some_and(|r| (1..=reach).contains(&(r.1 - sf))) => (sf, false),
            None => return None,
        };
        if forward {
            // The stream after the chunk starts now, so playback goes on past it without a held frame.
            let next = self.clamp(m, to + 1, q);
            let ready = next <= to || self.chunks.iter().any(|c| same(&c.s) && (c.from..=c.to).contains(&next));
            if playing && !ready && self.find(m, next, w, h, q, JUMP).is_none() {
                let _ = self.open(m, next, w, h, q, true);
            }
        } else {
            self.chunks.retain(|c| !same(&c.s) || c.from <= to);
            // A click a second back on the ruler is one seek; a drag, J or Left arrow takes small steps and keeps going.
            let going = last.is_some_and(|r| r.2 || (1..=STEP_BACK).contains(&(r.1 - sf)));
            self.read_back(m, to, w, h, q, if playing || going { BACK_AHEAD } else { 0 });
        }
        self.chunks.iter().position(|c| same(&c.s) && (c.from..=c.to).contains(&sf))
    }

    /// R20: start reading a reversed event ahead of time: chunks ending at `sf` and before it.
    pub fn prewarm_back(&mut self, m: &MediaRef, sf: i64, w: u32, h: u32, q: Quality) {
        let sf = self.clamp(m, sf, q);
        self.read_back(m, sf, w, h, q, BACK_AHEAD);
    }

    /// R20: decode the chunk ending at `to` (unless one holds it) and the `ahead` chunks before it.
    fn read_back(&mut self, m: &MediaRef, to: i64, w: u32, h: u32, q: Quality, ahead: usize) {
        let same = |s: &VideoStream| Self::same(s, m, w, h, q);
        let mut end = to;
        for _ in 0..=ahead {
            if end < 0 {
                break;
            }
            let from = match self.chunks.iter().find(|c| same(&c.s) && (c.from..=c.to).contains(&end)) {
                Some(c) => c.from,
                None => {
                    // A single paused step back reads a few frames, not a second.
                    let len = if ahead == 0 { self.chunk_len(m, w, h).min(STEP_BACK) } else { self.chunk_len(m, w, h) };
                    let from = (end - len + 1).max(0);
                    let Ok(s) = self.start(m, from, w, h, q, false, Some(end - from + 1)) else { break };
                    // Over budget: the least recently read chunks go, but not those from here to `to` (they always fit).
                    self.chunks.push(Chunk { s, from, to: end, frames: Vec::new(), at: None });
                    let bytes = |c: &Chunk| (c.to - c.from + 1) as usize * c.s.w as usize * c.s.h as usize * 4;
                    let keep = |c: &Chunk| same(&c.s) && c.from >= from && c.to <= to;
                    while self.chunks.iter().map(bytes).sum::<usize>() > BACK_BYTES || self.chunks.len() > MAX_STREAMS {
                        let Some(i) = (0..self.chunks.len()).filter(|&i| !keep(&self.chunks[i])).min_by_key(|&i| self.chunks[i].s.used) else { break };
                        self.chunks.remove(i);
                    }
                    from
                }
            };
            end = from - 1;
        }
    }

    /// R20: frame `sf` from chunk `i`, waiting for it until `deadline` (then the layer holds); None if the chunk ended
    /// before it (the video is shorter, or ffmpeg failed): it closes, and `sf` is read as a seek.
    fn from_chunk(&mut self, i: usize, sf: i64, deadline: Option<Instant>) -> Option<Pull> {
        let c = &mut self.chunks[i];
        c.s.used = Instant::now();
        while c.s.next_sf <= sf {
            let got = match deadline {
                Some(d) => c.s.rx.recv_timeout(d.saturating_duration_since(Instant::now())),
                None => c.s.rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            match got {
                Ok(px) => {
                    c.frames.push(image(px, c.s.w, c.s.h));
                    c.s.next_sf += 1;
                }
                Err(RecvTimeoutError::Timeout) => {
                    self.dropped += 1;
                    return Some(Pull::Held(None));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    self.chunks.remove(i);
                    return None;
                }
            }
        }
        c.at = Some(sf);
        Some(Pull::Exact(c.frames[(sf - c.from) as usize].clone()))
    }

    /// Start a stream at `sf` ahead of time unless one can reach it in time: playing (`play`, hardware decoding allowed),
    /// within a few frames, so a jump cut inside one file gets its own stream; paused, within 2 s (or, R20, its chunks
    /// when the picture is read backwards).
    pub fn prewarm(&mut self, m: &MediaRef, sf: i64, w: u32, h: u32, q: Quality, play: bool) {
        let sf = self.clamp(m, sf, q);
        let reach = if play { JUMP } else { self.reach() };
        // Playing, the frames warmed are event starts, not the next frame of a picture read backwards.
        if self.find(m, sf, w, h, q, reach).is_none() && (play || self.chunk(m, sf, w, h, q, false).is_none()) {
            let _ = self.open(m, sf, w, h, q, play);
        }
    }

    /// Frame `sf` of `m` at `w` x `h`. With a deadline (playback) a late stream gives its last frame instead.
    pub fn frame(&mut self, m: &MediaRef, sf: i64, w: u32, h: u32, q: Quality, deadline: Option<Instant>) -> Pull {
        let sf = self.clamp(m, sf, q);
        // R20: a frame already in a chunk, or one behind what was read last, comes from the chunks.
        let found = self.chunks.iter().any(|c| Self::same(&c.s, m, w, h, q) && (c.from..=c.to).contains(&sf));
        let reuse = if found { None } else { self.find(m, sf, w, h, q, self.reach()) };
        if reuse.is_none() {
            if let Some(p) = self.chunk(m, sf, w, h, q, deadline.is_some()).and_then(|c| self.from_chunk(c, sf, deadline)) {
                return p;
            }
        }
        let i = match reuse.or_else(|| self.find(m, sf, w, h, q, self.reach())) {
            Some(i) => i,
            None => match self.open(m, sf, w, h, q, deadline.is_some()) {
                Ok(i) => i,
                Err(e) => return Pull::Err(e),
            },
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
        DecSpec { path, sf: 90, lead: 1, fps_str: "30000/1001", fps: 30000.0 / 1001.0, w: 960, h: 540, scaler: "fast_bilinear", hw: false, matrix: Some("bt709"), deint: "", stream: 0, flow: false, threads: 2, frames: None }
    }

    #[test]
    fn decoder_argv() {
        let p = Path::new(r"C:\clips\a b.mp4");
        assert_eq!(
            args(&spec(p)).join(" "),
            r"-v error -threads 2 -ss 2.969633 -i C:\clips\a b.mp4 -map 0:v:0 -an -sn -dn -vf fps=30000/1001:round=up:start_time=0,scale=960:540:flags=fast_bilinear:in_color_matrix=bt709:out_range=full,format=rgba -f rawvideo -"
        );
        let a = args(&DecSpec { hw: true, deint: "yadif=deint=interlaced,", matrix: None, ..spec(p) });
        assert_eq!(a[4..6], ["-hwaccel", "auto"]);
        assert!(a.iter().any(|x| x.starts_with("yadif=deint=interlaced,fps=") && x.contains("in_color_matrix=auto")));
        // R3b: Stream 2 and Optical Flow.
        let a = args(&DecSpec { stream: 1, flow: true, ..spec(p) }).join(" ");
        assert!(a.contains("-map 0:v:1 ") && a.contains("-vf minterpolate=fps=30000/1001:mi_mode=mci,scale="), "{a}");
        // R20: a chunk read backwards: more decoder threads, and it stops after its frames (lead included).
        let a = args(&DecSpec { threads: 8, frames: Some(31), ..spec(p) }).join(" ");
        assert!(a.starts_with("-v error -threads 8 -ss ") && a.ends_with(",format=rgba -frames:v 31 -f rawvideo -"), "{a}");
    }

    /// R20: a chunk is 1 s of frames, or fewer when the chunk and those decoding ahead would pass `BACK_BYTES`.
    #[test]
    fn chunks_are_a_second_within_the_budget() {
        let mut pool = DecoderPool::default();
        pool.set_rate(30000.0 / 1001.0, "30000/1001");
        let m = ramp_ref();
        assert_eq!(pool.chunk_len(&m, 960, 540), 30);
        assert_eq!(pool.chunk_len(&m, 1920, 1080), 10);
        assert_eq!(pool.chunk_len(&m, 7680, 4320), 1);
        for (w, h) in [(960, 540), (1920, 1080), (3840, 2160)] {
            assert!(pool.chunk_len(&m, w, h) as usize * w as usize * h as usize * 4 * (BACK_AHEAD + 1) <= BACK_BYTES);
        }
        // A 60 fps grid (slow motion from a 60p source) reads a second of its own frames.
        assert_eq!(pool.chunk_len(&MediaRef { grid: 60.0, ..ramp_ref() }, 320, 180), 60);
        assert!(2 <= seek_threads() && seek_threads() <= 4);
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
            deint: "",
            proxy: None,
            grid: 0.0,
            offset: 0.0,
            stream: 0,
            flow: false,
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
            let m = MediaRef { id: name.into(), path: path.clone(), w: 64, h: 64, duration: 12.0, fps: if name == "pal.mp4" { 25.0 } else { 30000.0 / 1001.0 }, still: false, has_alpha: false, matrix: None, codec: None, deint: "", proxy: None, grid: 0.0, offset: 0.0, stream: 0, flow: false };
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
        MediaRef { id: "ramp".into(), path: "".into(), w: 64, h: 64, duration: 1.0, fps: 30.0, still: false, has_alpha: false, matrix: None, codec: None, deint: "", proxy: None, grid: 0.0, offset: 0.0, stream: 0, flow: false }
    }

    fn gray(p: Pull) -> Option<u8> {
        match p {
            Pull::Exact(img) | Pull::Held(Some(img)) => Some(img.pixels[64 * 32 + 32].r()),
            Pull::Held(None) => None,
            Pull::Err(e) => panic!("{e}"),
        }
    }

    /// Needs ffmpeg. R3b Optical Flow: half-speed frames on a 60 fps grid are the source frames at whole source times and
    /// made-up frames between them.
    #[test]
    #[ignore]
    fn optical_flow_interpolates_between_source_frames() {
        let dir = std::env::temp_dir().join(format!("milan-vflow-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let m = MediaRef { grid: 60.0, flow: true, id: "ramp#0f".into(), ..ramp(&dir, 2, 2.0) };
        let mut pool = DecoderPool::default();
        pool.set_rate(30.0, "30");
        let g: Vec<f64> = (0..6).map(|sf| gray(pool.frame(&m, sf, 64, 64, Quality::Best, None)).unwrap() as f64).collect();
        for k in 0..3 {
            assert!((g[2 * k] - level(k as i64)).abs() <= 1.5, "frame {}: {} vs {}", 2 * k, g[2 * k], level(k as i64));
            assert!(g[2 * k + 1] > level(k as i64) && g[2 * k + 1] < level(k as i64 + 1), "between {k} and {}: {}", k + 1, g[2 * k + 1]);
        }
        drop(pool);
        std::fs::remove_dir_all(&dir).unwrap();
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

    /// Needs ffmpeg. R20: reversed playback at 30 fps reads every frame from chunks, with a bounded number of decoders;
    /// then a paused step forward past the chunks reads as a seek again.
    #[test]
    #[ignore]
    fn reverse_playback_reads_every_frame_from_chunks() {
        let dir = std::env::temp_dir().join(format!("milan-vrev-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let m = ramp(&dir, 10, 10.0);
        let mut pool = DecoderPool::default();
        pool.set_rate(30.0, "30");
        let (mut exact, start) = (0, Instant::now());
        for (k, sf) in (100..250).rev().enumerate() {
            let due = start + Duration::from_secs_f64(k as f64 / 30.0);
            std::thread::sleep(due.saturating_duration_since(Instant::now()));
            let d = (k > 0).then(|| Instant::now() + Duration::from_millis(25));
            let p = pool.frame(&m, sf, 64, 64, Quality::Best, d);
            exact += matches!(&p, Pull::Exact(_)) as u32;
            if let Pull::Exact(img) = p {
                assert!((img.pixels[64 * 32 + 32].r() as f64 - level(sf)).abs() <= 1.5, "frame {sf}");
            }
            // The first stream, the chunk being read and those decoding ahead.
            assert!(pool.live() <= 2 + BACK_AHEAD, "frame {k}: {} decoders", pool.live());
        }
        println!("reverse: {exact} of 150 frames exact");
        assert!(exact >= 143, "{exact}");
        // Read past, chunks close; forward again is a seek, not a chunk.
        assert!(pool.chunks.iter().all(|c| c.from <= 100));
        // Played forward from there: the chunk serves, and the stream after it starts at once.
        let to = pool.chunks.iter().find(|c| (c.from..=c.to).contains(&101)).unwrap().to;
        assert!(matches!(pool.frame(&m, 101, 64, 64, Quality::Best, Some(Instant::now() + Duration::from_millis(25))), Pull::Exact(_)));
        assert!(pool.streams.iter().any(|s| s.next_sf == to + 1), "a stream at {}", to + 1);
        let g = gray(pool.frame(&m, 260, 64, 64, Quality::Best, None)).unwrap();
        assert!((g as f64 - level(260)).abs() <= 1.5);
        assert!(pool.streams.iter().any(|s| s.next_sf == 261));
        // Paused steps back all over the file (dragging left here and there): exact, and the chunks stay bounded.
        for k in 0..40i64 {
            let sf = 280 - (k * 37) % 250 - 3;
            for sf in [sf + 3, sf] {
                let g = gray(pool.frame(&m, sf, 64, 64, Quality::Best, None)).unwrap();
                assert!((g as f64 - level(sf)).abs() <= 1.5, "step back to {sf}");
            }
            assert!(pool.chunks.len() <= MAX_STREAMS && pool.streams.len() <= MAX_STREAMS);
        }
        // Two layers of the file 1 s apart (a dissolve inside one recording), stepped forward paused: each reads on
        // from its own stream after the first step, rather than a chunk per step.
        drop(pool);
        let mut pool = DecoderPool::default();
        pool.set_rate(30.0, "30");
        for k in 0..12 {
            for sf in [150 + k, 180 + k] {
                let g = gray(pool.frame(&m, sf, 64, 64, Quality::Best, None)).unwrap();
                assert!((g as f64 - level(sf)).abs() <= 1.5, "dissolve frame {sf}");
            }
        }
        assert!(pool.live() <= 3, "{} decoders", pool.live());
        drop(pool);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
