//! Audio mixer: the audio plan mixed block by block from persistent ffmpeg PCM streams (stereo f32 at RATE).
//! `run` is the "milan-mix" thread body for playback; it loops inside the mixer, so a loop wrap is seamless.
//! R10a: tracks are stereo (a multichannel source is read as one of its stereo pairs or its BS.775 downmix); in a
//! 5.1 / 7.1 project (`AudioPlan::layout`) each track's output is placed by its surround pan (`surround::pan_matrix`),
//! the busses and the master carry the layout's interleaved channels, bus and master FX run per stereo pair, and
//! playback folds the master down to stereo for the device after metering it. A stereo project mixes exactly as before.
use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime};
use crate::audio::RATE;
use crate::engine::afx::Chain;
use crate::engine::keyframes::{db_to_gain, ramp};
use crate::engine::nested::NestedMix;
use crate::engine::plan::{ASrc, AudioPlan, Varispeed, fade};
use crate::engine::resample::Resampler;
use crate::engine::surround::{self, MAX};
use crate::project::{ChannelMap, Curve, Envelope, SurroundPan};

/// R10a: the master's meter in `Meters` (a surround project's playback feeds it; stereo meters the device output).
pub const MASTER_METER: &str = "master";

/// Frames per mixed block (about 21 ms).
pub const BLOCK: usize = 1024;
/// Frames per decoded chunk (0.25 s).
const CHUNK: usize = RATE as usize / 4;
/// A decoder silent this long is given up on, so a hung source cannot hold the mix thread forever.
const TIMEOUT: Duration = Duration::from_secs(2);
/// Varispeed slower than this (a freeze, a stopped shuttle) is silent: a held sample is only a DC offset.
const MIN_SPEED: f64 = 0.01;

/// Timeline frame (at RATE) of time `t`.
pub fn frame(t: f64) -> i64 {
    (t * RATE as f64).round() as i64
}

/// atempo filters for `rate`, each step within [0.5, 2]. atempo keeps the pitch, as VEGAS does by default.
fn atempo(rate: f64) -> Vec<String> {
    let (mut r, mut v) = (rate, Vec::new());
    while r > 2.0 {
        v.push("atempo=2".to_string());
        r /= 2.0;
    }
    while r < 0.5 {
        v.push("atempo=0.5".to_string());
        r /= 0.5;
    }
    if (r - 1.0).abs() > 1e-9 {
        v.push(format!("atempo={r}"));
    }
    v
}

/// The filters that play `src` at its rate and pitch. A pitch change (Event Properties › Pitch change) is a resample
/// (asetrate) that atempo puts back to length, so the event keeps its length and stays in sync.
fn tempo_pitch(src: &ASrc) -> Vec<String> {
    if src.pitch == 0.0 {
        return atempo(src.rate);
    }
    let sr = (RATE as f64 * 2f64.powf(src.pitch as f64 / 1200.0)).round();
    let mut v = vec![format!("aresample={RATE}"), format!("asetrate={sr}"), format!("aresample={RATE}")];
    v.extend(atempo(src.rate * RATE as f64 / sr));
    v
}

/// Source seconds `at` frames into the event `src` (forward).
fn src_time(src: &ASrc, at: i64) -> f64 {
    src.in_point + at as f64 * src.rate / RATE as f64
}

/// ffmpeg arguments (after `-nostdin`) streaming `src` from `at` frames into its event, in timeline order.
/// Forward, it reads on to the media end, so a trim or a split keeps the decoder. Reversed, the span still to play
/// is read and turned round (`areverse` holds it all in memory). A mono source (`channels` 1) goes to both sides at
/// unity, as a mono recording does (`-ac 2` would put it 3 dB down). R10a: a multichannel source plays the stereo
/// pair Channels ▸ chose (`pan=stereo|c0=c2k|c1=c2k+1`), or every channel folded down by `-ac 2` (FFmpeg's matrix is
/// ITU-R BS.775 for float samples, the one `surround::downmix` uses). A 5.1 / 7.1 track's decoder (`src.ch` 6 or 8)
/// converts the media to that layout with `-ac` (channel for channel when it already is; stereo fills the front pair,
/// mono the centre, as FFmpeg upmixes).
// shortcut: areverse buffers the whole span (about 23 MB per stereo minute); render a reversed temp file past ~10 min.
pub fn args(src: &ASrc, at: i64, channels: Option<usize>) -> Vec<String> {
    let wide = src.ch > 2;
    let mono = channels == Some(1) && !wide;
    let pair = src.pair.filter(|_| !wide && channels.is_some_and(|c| c > 2)).map(|k| (2 * k as usize).min(channels.unwrap_or(2) - 1));
    let mut a: Vec<String> = ["-v", "error", "-ss"].map(String::from).to_vec();
    if src.reverse {
        let len = (src.dur - at as f64 / RATE as f64).max(0.0) * src.rate;
        a.extend([format!("{:.6}", src.in_point), "-t".into(), format!("{len:.6}")]);
    } else {
        a.push(format!("{:.6}", src_time(src, at)));
    }
    a.extend(["-i".into(), src.path.to_string_lossy().into_owned()]);
    a.extend(["-vn".into(), "-map".into(), format!("0:a:{}", src.stream)]);
    if wide {
        a.extend(["-ac".into(), src.ch.to_string()]);
    } else if !mono && pair.is_none() {
        a.extend(["-ac", "2"].map(String::from));
    }
    a.extend(["-ar".into(), RATE.to_string()]);
    let mut af = tempo_pitch(src);
    if src.reverse {
        af.insert(0, "areverse".into());
    }
    if mono {
        af.insert(0, "pan=stereo|c0=c0|c1=c0".into());
    } else if let Some(k) = pair {
        let last = channels.unwrap_or(2) - 1;
        af.insert(0, format!("pan=stereo|c0=c{k}|c1=c{}", (k + 1).min(last)));
    }
    if !af.is_empty() {
        a.extend(["-af".into(), af.join(",")]);
    }
    a.extend(["-f", "f32le", "-"].map(String::from));
    a
}

/// A decoder process, shared by the thread that starts and reads it and its `Stream`: (the Stream is gone, process).
type Proc = Arc<Mutex<(bool, Option<Child>)>>;

/// One event's decoder: interleaved stereo chunks, read forward.
struct Stream {
    rx: Receiver<Vec<f32>>,
    buf: Vec<f32>,
    pos: usize,
    proc: Option<Proc>,
    /// The decoder's error (empty if none), once ffmpeg has exited.
    err: Option<JoinHandle<String>>,
    /// A chunk has arrived.
    started: bool,
    /// Samples played as silence while the decoder was starting, dropped when they arrive so the event stays in sync.
    skip: usize,
    /// R6a: a nested timeline's mixer, told to stop waiting on its own decoders when this goes.
    stop: Option<Arc<AtomicBool>>,
}

impl Drop for Stream {
    /// Killed and reaped on a thread of its own: the mix thread never waits on a process.
    fn drop(&mut self) {
        if let Some(s) = &self.stop {
            s.store(true, Relaxed);
        }
        let Some(p) = &self.proc else { return };
        let child = {
            let mut g = p.lock().unwrap_or_else(|e| e.into_inner());
            g.0 = true;
            g.1.take()
        };
        if let Some(mut c) = child {
            std::thread::spawn(move || {
                let _ = c.kill();
                let _ = c.wait();
            });
        }
    }
}

impl Stream {
    fn from_rx(rx: Receiver<Vec<f32>>) -> Self {
        Self { rx, buf: Vec::new(), pos: 0, proc: None, err: None, started: false, skip: 0, stop: None }
    }

    /// A stream that has already ended: silence.
    fn ended() -> Self {
        Self::from_rx(mpsc::sync_channel(0).1)
    }

    /// Fill `out`, with silence once the stream has ended. Returns the decoder's error the first time it ends with one.
    /// A decoder that has not started (or is still catching up) holds the caller only when `wait` is set: otherwise
    /// it plays silence, so a slow start (a reversed span, a cold disk) delays only its own event.
    /// With `stop` (a render) every read waits however long it takes, until `stop` is set.
    fn read(&mut self, out: &mut [f32], wait: bool, stop: Option<&AtomicBool>) -> Option<String> {
        let mut k = 0;
        while k < out.len() {
            if self.pos >= self.buf.len() {
                let behind = !self.started || self.skip > 0;
                let got = if behind && !wait {
                    self.rx.try_recv().map_err(|e| if e == TryRecvError::Empty { RecvTimeoutError::Timeout } else { RecvTimeoutError::Disconnected })
                } else if let Some(stop) = stop {
                    recv_until(&self.rx, stop)
                } else {
                    self.rx.recv_timeout(TIMEOUT)
                };
                match got {
                    Ok(b) => {
                        let d = self.skip.min(b.len());
                        (self.buf, self.pos, self.skip, self.started) = (b, d, self.skip - d, true);
                    }
                    Err(RecvTimeoutError::Timeout) if behind => {
                        self.skip += out.len() - k;
                        out[k..].fill(0.0);
                        return None;
                    }
                    Err(e) => {
                        out[k..].fill(0.0);
                        return self.ended_with(e);
                    }
                }
            }
            let n = (self.buf.len() - self.pos).min(out.len() - k);
            out[k..k + n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
            (self.pos, k) = (self.pos + n, k + n);
        }
        None
    }

    /// The stream has ended (Disconnected: the decoder's error, if any) or stalled after it started (Timeout: ffmpeg is
    /// killed, which ends its reader, so later reads are silent instead of waiting again).
    fn ended_with(&mut self, e: RecvTimeoutError) -> Option<String> {
        if e == RecvTimeoutError::Timeout {
            if let Some(p) = &self.proc {
                if let Some(c) = p.lock().unwrap_or_else(|e| e.into_inner()).1.as_mut() {
                    let _ = c.kill();
                }
            }
            return Some("Audio decoding stopped responding: an event is silent".into());
        }
        self.err.take().and_then(|j| j.join().ok()).filter(|e| !e.is_empty())
    }
}

/// R4b: a varispeed event's decoder and all it has read: the media from `s0` on, at rate 1 (and the event's pitch),
/// so the event can read anywhere in it (backwards, held, at any speed).
// shortcut: what was read stays until the event stops playing (about 23 MB per stereo minute, as with areverse);
// drop what the rest of the event no longer reaches if long varispeed events become common.
struct VLive {
    /// What the decoder reads (`in_point` is `s0` in the file).
    key: ASrc,
    s0: f64,
    buf: Vec<f32>,
    ended: bool,
    used: u64,
    s: Stream,
}

impl VLive {
    /// Read on until `frames` frames are held or the decoder ends. Waits for the decoder only when `wait` (a session's
    /// first block, a render: then until `stop`); otherwise frames not read yet play as silence.
    fn fill(&mut self, frames: usize, wait: bool, stop: Option<&AtomicBool>) -> Option<String> {
        while !self.ended && self.buf.len() < frames * self.key.ch.max(2) as usize {
            let got = if !wait {
                self.s.rx.try_recv().map_err(|e| if e == TryRecvError::Empty { RecvTimeoutError::Timeout } else { RecvTimeoutError::Disconnected })
            } else if let Some(stop) = stop {
                recv_until(&self.s.rx, stop)
            } else {
                self.s.rx.recv_timeout(TIMEOUT)
            };
            match got {
                Ok(b) => self.buf.extend_from_slice(&b),
                // Not started yet (a cold disk) is waited for again next block, as `Stream::read` does.
                Err(RecvTimeoutError::Timeout) if !wait || stop.is_some() || self.buf.is_empty() => break,
                Err(e) => {
                    self.ended = true;
                    return self.s.ended_with(e);
                }
            }
        }
        None
    }
}

/// Receive however long it takes; a Timeout once `stop` is set.
fn recv_until<T>(rx: &Receiver<T>, stop: &AtomicBool) -> Result<T, RecvTimeoutError> {
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Err(RecvTimeoutError::Timeout) if !stop.load(Relaxed) => {}
            r => return r,
        }
    }
}

/// f32le bytes as samples; NaN and infinity (a broken float file) become silence, or they would stick in the FX state.
fn samples(bytes: &[u8]) -> Vec<f32> {
    bytes.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).map(|v| if v.is_finite() { v } else { 0.0 }).collect()
}

/// Channel count of `path`'s audio stream `stream`, probed once per file version.
pub fn channels(path: &Path, stream: u32) -> Option<usize> {
    type Probed = HashMap<(PathBuf, u32, Option<SystemTime>), Option<usize>>;
    static CACHE: OnceLock<Mutex<Probed>> = OnceLock::new();
    let key = (path.to_path_buf(), stream, std::fs::metadata(path).and_then(|m| m.modified()).ok());
    if let Some(&c) = CACHE.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return c;
    }
    let c = crate::audio::channel_count(path, stream);
    CACHE.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner()).insert(key, c);
    c
}

/// Channels ▸ and Invert Phase on interleaved stereo `pcm`. Combine puts the mean of both sides on both.
pub fn map_channels(pcm: &mut [f32], ch: ChannelMap, invert: bool) {
    if ch == ChannelMap::Both && !invert {
        return;
    }
    let s = if invert { -1.0 } else { 1.0 };
    for f in pcm.chunks_exact_mut(2) {
        let (l, r) = (f[0], f[1]);
        let (a, b) = match ch {
            ChannelMap::Both => (l, r),
            ChannelMap::LeftOnly => (l, l),
            ChannelMap::RightOnly => (r, r),
            ChannelMap::Combine => ((l + r) * 0.5, (l + r) * 0.5),
            ChannelMap::Swap => (r, l),
        };
        (f[0], f[1]) = (a * s, b * s);
    }
}

/// Read ffmpeg's f32le stdout in CHUNK-frame blocks into `each` (whole stereo frames, NaN as silence) until it ends
/// or `each` returns false.
fn read_pcm(out: &mut impl Read, mut each: impl FnMut(Vec<f32>) -> bool) {
    let mut bytes = vec![0u8; CHUNK * 8];
    loop {
        let mut n = 0;
        while n < bytes.len() {
            match out.read(&mut bytes[n..]) {
                Ok(0) | Err(_) => break,
                Ok(k) => n += k,
            }
        }
        let pcm = samples(&bytes[..n - n % 8]);
        if pcm.is_empty() || !each(pcm) || n < bytes.len() {
            break;
        }
    }
}

/// Switches › Normalize: the peak |sample| of `len` seconds of `src` from its in point, read as the mixer reads it,
/// through the event's channel map (so Left Only is measured on the left channel).
pub fn peak(src: &ASrc, len: f64, ch: ChannelMap) -> Result<f32, String> {
    if src.nested.is_some() {
        // R6a: a nested timeline is measured from its mix.
        let (mut s, never) = (spawn(src, 0)?, AtomicBool::new(false));
        let (mut left, mut peak) = (frame(len) as usize, 0.0f32);
        let mut pcm = vec![0.0; CHUNK * 2];
        while left > 0 {
            let k = left.min(CHUNK);
            if let Some(e) = s.read(&mut pcm[..k * 2], true, Some(&never)) {
                return Err(e);
            }
            map_channels(&mut pcm[..k * 2], ch, false);
            peak = pcm[..k * 2].iter().fold(peak, |m, v| m.max(v.abs()));
            left -= k;
        }
        return Ok(peak);
    }
    let mut a = args(src, 0, channels(&src.path, src.stream));
    a.splice(4..4, ["-t".to_string(), format!("{len:.6}")]); // after "-ss <in point>"
    let mut cmd = crate::proc::ffmpeg();
    cmd.args(a).stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = crate::proc::spawn(&mut cmd).map_err(|e| crate::proc::tool_error(&e, "ffmpeg"))?;
    let mut out = child.stdout.take().expect("piped stdout");
    let mut peak = 0.0f32;
    read_pcm(&mut out, |mut pcm| {
        map_channels(&mut pcm, ch, false);
        peak = pcm.iter().fold(peak, |m, v| m.max(v.abs()));
        true
    });
    drop(out);
    let ok = child.wait().is_ok_and(|s| s.success());
    if ok { Ok(peak) } else { Err("The audio could not be read to normalize it".into()) }
}

/// A decoder for `src` from `at` frames in. The process is started (about 15 ms on Windows) by the reader thread,
/// so the mix thread returns at once and the stream plays silence until it delivers.
fn spawn(src: &ASrc, at: i64) -> Result<Stream, String> {
    if let Some(n) = &src.nested {
        return spawn_nested(src, n, at);
    }
    let src = src.clone();
    Ok(run_ffmpeg(move || args(&src, at, channels(&src.path, src.stream)), None))
}

/// What writes an ffmpeg's stdin (R6a: a nested timeline's mix), on a thread of its own.
type Feed = Box<dyn FnOnce(std::process::ChildStdin) + Send>;

/// ffmpeg with the arguments `argv` makes (on the reader thread: they may probe the file) as a stream of its stdout.
fn run_ffmpeg(argv: impl FnOnce() -> Vec<String> + Send + 'static, feed: Option<Feed>) -> Stream {
    let (tx, rx) = mpsc::sync_channel(8);
    let proc: Proc = Arc::default();
    let shared = proc.clone();
    let err = std::thread::spawn(move || {
        let mut cmd = crate::proc::ffmpeg();
        cmd.args(argv()).stdout(Stdio::piped()).stderr(Stdio::piped());
        if feed.is_some() {
            cmd.stdin(Stdio::piped());
        }
        let mut child = match crate::proc::spawn(&mut cmd) {
            Ok(c) => c,
            Err(e) => return format!("{}: audio is silent", crate::proc::tool_error(&e, "ffmpeg")),
        };
        if let (Some(f), Some(stdin)) = (feed, child.stdin.take()) {
            std::thread::spawn(move || f(stdin));
        }
        let (mut out, err) = (child.stdout.take().expect("piped stdout"), child.stderr.take().expect("piped stderr"));
        let last = std::thread::spawn(move || BufReader::new(err).lines().map_while(Result::ok).filter(|l| !l.trim().is_empty()).last().unwrap_or_default());
        {
            let mut g = shared.lock().unwrap_or_else(|e| e.into_inner());
            if g.0 {
                let _ = child.kill(); // the Stream is already gone
            }
            g.1 = Some(child);
        }
        let mut cut = false;
        read_pcm(&mut out, |pcm| {
            cut = tx.send(pcm).is_err(); // the Stream is gone
            !cut
        });
        drop(out);
        // Reaped here, unless the Stream took it to kill. `tx` is still held, so the Stream sees the end only once
        // this result is ready.
        let child = shared.lock().unwrap_or_else(|e| e.into_inner()).1.take();
        let ok = child.is_none_or(|mut c| {
            if cut {
                let _ = c.kill();
            }
            c.wait().is_ok_and(|s| s.success()) || cut
        });
        let line = last.join().unwrap_or_default();
        drop(tx);
        // ffmpeg logs a damaged packet at error level and decodes on: only a failed exit is an error (the missing
        // tail is silence).
        if ok { String::new() } else { line }
    });
    let mut s = Stream::from_rx(rx);
    (s.proc, s.err) = (Some(proc), Some(err));
    s
}

/// R6a: nested timeline `n`'s master bus from `t0` for `len` seconds, mixed (as its own render mixes it: every decoder
/// waited for, times its master gain) and handed to `each` in chunks until it refuses one or `stop` is set. Returns the
/// first decoder error.
pub(crate) fn mix_nested(n: &NestedMix, t0: f64, len: f64, stop: &Arc<AtomicBool>, mut each: impl FnMut(Vec<f32>) -> bool) -> String {
    let Ok(plan) = &n.plan else { return String::new() };
    let mut m = Mixer::ffmpeg();
    m.render = Some(stop.clone());
    let (mut f, f1) = (frame(t0), frame(t0) + frame(len));
    m.warm(plan, f, (f + RATE as i64).min(f1));
    let mut err = String::new();
    while f < f1 && !stop.load(Relaxed) {
        // Whole blocks from `t0`, so the same start always mixes the same blocks. R10a: a surround child is folded
        // down to stereo, as its own stereo render would be.
        let k = (f1 - f).min((BLOCK * 12) as i64) as usize;
        let mut pcm = vec![0.0f32; k * 2];
        for (i, b) in pcm.chunks_mut(BLOCK * 2).enumerate() {
            block_stereo(&mut m, plan, f + (i * BLOCK) as i64, b);
        }
        if let Some(e) = m.error.take().filter(|_| err.is_empty()) {
            err = e;
        }
        pcm.iter_mut().enumerate().for_each(|(i, v)| *v *= n.gain[i % 2]);
        if !each(pcm) {
            break;
        }
        f += k as i64;
    }
    err
}

/// R6a: a stream of nested timeline `n` from `at` frames into event `src`, as a file's decoder would read it: its mix
/// on a thread of its own, through ffmpeg (fed the mix) when the event plays at another rate or pitch, or reversed.
fn spawn_nested(src: &ASrc, n: &Arc<NestedMix>, at: i64) -> Result<Stream, String> {
    if let Err(e) = &n.plan {
        return Err(e.clone());
    }
    let (t0, len) = if src.reverse {
        (src.in_point, ((src.dur - at as f64 / RATE as f64).max(0.0) * src.rate).min(n.end - src.in_point))
    } else {
        let t = src_time(src, at);
        (t, n.end - t)
    };
    let len = len.max(0.0);
    let (stop, n) = (Arc::new(AtomicBool::new(false)), n.clone());
    let st = stop.clone();
    // R10a: a 5.1 / 7.1 track spreads the nested mix (stereo) to its layout.
    let ch = src.ch.max(2) as usize;
    let mut s = if src.rate == 1.0 && src.pitch == 0.0 && !src.reverse {
        let (tx, rx) = mpsc::sync_channel(8);
        let err = std::thread::spawn(move || {
            mix_nested(&n, t0, len, &st, |pcm| {
                if ch == 2 {
                    return tx.send(pcm).is_ok();
                }
                let mut wide = Vec::new();
                surround::convert(&pcm, 2, ch, &mut wide);
                tx.send(wide).is_ok()
            })
        });
        let mut s = Stream::from_rx(rx);
        s.err = Some(err);
        s
    } else {
        let mut af = tempo_pitch(src);
        if src.reverse {
            af.insert(0, "areverse".into());
        }
        let r = RATE.to_string();
        let mut a: Vec<String> = ["-v", "error", "-f", "f32le", "-ar", &r, "-ac", "2", "-i", "pipe:0", "-ar", &r].map(String::from).to_vec();
        a.extend(["-af".into(), af.join(",")]);
        if ch > 2 {
            a.extend(["-ac".into(), ch.to_string()]);
        }
        a.extend(["-f", "f32le", "-"].map(String::from));
        let feed: Feed = Box::new(move |mut stdin| {
            use std::io::Write;
            // shortcut: an error of the nested mix fed through ffmpeg is not reported (its tail plays as silence).
            mix_nested(&n, t0, len, &st, |pcm| stdin.write_all(&pcm.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>()).is_ok());
        });
        run_ffmpeg(move || a, Some(feed))
    };
    s.stop = Some(stop);
    Ok(s)
}

/// A stream positioned `next` frames into the event `src` it was opened for.
struct Live {
    src: ASrc,
    next: i64,
    /// Source seconds at `next` (forward streams), counted from what was read.
    t: f64,
    /// Block generation that last used or warmed it.
    used: u64,
    /// The block or warm pass that claimed it: a stream serves one event per pass.
    pass: u64,
    s: Stream,
}

impl Live {
    /// It can play on as `src` from `at` frames into that event.
    fn continues(&self, src: &ASrc, at: i64) -> bool {
        if src.reverse || self.src.reverse {
            return self.src == *src && self.next == at;
        }
        // Forward streams follow the source, not the event: a move or trim during playback that keeps the source time
        // under the playhead keeps the decoder (within one frame of frame rounding).
        self.src.path == src.path
            && self.src.nested == src.nested
            && self.src.stream == src.stream
            && self.src.rate == src.rate
            && self.src.pitch == src.pitch
            && self.src.pair == src.pair
            && self.src.ch == src.ch
            && (self.t - src_time(src, at)).abs() <= src.rate / RATE as f64 + 1e-9
    }
}

type Open = Box<dyn FnMut(&ASrc, i64) -> Result<Stream, String> + Send>;

/// R8a: the RMS window of the level meters.
pub const METER_WINDOW: usize = RATE as usize / 20;

/// R8a: a track's or bus's level meter, written by the mix thread and read by the UI: the peak |sample| since it was last
/// taken, and the RMS over the last 50 ms, per channel (f32 bits). Post-fader: what the node sends on to its output.
/// R10a: `channels` wide (2, or the layout's 6 / 8 in a surround project).
#[derive(Debug)]
pub struct Meter {
    peak: [AtomicU32; MAX],
    rms: [AtomicU32; MAX],
    channels: AtomicUsize,
}

impl Default for Meter {
    fn default() -> Self {
        Self { peak: Default::default(), rms: Default::default(), channels: AtomicUsize::new(2) }
    }
}

impl Meter {
    /// The peaks of the first two channels since the last call (reading resets them).
    pub fn take_peaks(&self) -> [f32; 2] {
        [0, 1].map(|c| f32::from_bits(self.peak[c].swap(0, Relaxed)))
    }

    pub fn rms(&self) -> [f32; 2] {
        [0, 1].map(|c| f32::from_bits(self.rms[c].load(Relaxed)))
    }

    /// R10a: channels the meter was last fed with.
    #[allow(dead_code)] // read by the Mixing Console's per-channel meters (R10b)
    pub fn channels(&self) -> usize {
        self.channels.load(Relaxed).clamp(1, MAX)
    }

    /// R10a: every channel's peak since the last call (reading resets them), and every channel's RMS.
    #[allow(dead_code)]
    pub fn take_peaks_all(&self) -> [f32; MAX] {
        std::array::from_fn(|c| f32::from_bits(self.peak[c].swap(0, Relaxed)))
    }

    #[allow(dead_code)]
    pub fn rms_all(&self) -> [f32; MAX] {
        std::array::from_fn(|c| f32::from_bits(self.rms[c].load(Relaxed)))
    }

    /// R9: a block's peak and RMS from the input monitor (lock-free, for the real-time output callback).
    pub fn put(&self, peak: [f32; 2], rms: [f32; 2]) {
        for c in 0..2 {
            self.peak[c].fetch_max(peak[c].abs().to_bits(), Relaxed);
            self.rms[c].store(rms[c].to_bits(), Relaxed);
        }
    }
}

/// Meters by track and bus id. The UI thread builds a new map with each plan (keeping the meters of ids it had), so the
/// mix thread only reads it: no lock.
pub type Meters = HashMap<String, Arc<Meter>>;

/// One block's peak and sum of squares per channel, of what a node sends on.
#[derive(Default, Clone, Copy)]
struct Level {
    peak: [f32; MAX],
    sq: [f32; MAX],
}

impl Level {
    fn add(&mut self, l: f32, r: f32) {
        (self.peak[0], self.peak[1]) = (self.peak[0].max(l.abs()), self.peak[1].max(r.abs()));
        (self.sq[0], self.sq[1]) = (self.sq[0] + l * l, self.sq[1] + r * r);
    }

    /// R10a: one channel's sample.
    #[inline]
    fn add_ch(&mut self, c: usize, v: f32) {
        self.peak[c] = self.peak[c].max(v.abs());
        self.sq[c] += v * v;
    }
}

pub struct Mixer {
    open: Open,
    streams: Vec<Live>,
    /// Varispeed events' decoders by clip id, and the resampler that reads them.
    vstreams: HashMap<String, VLive>,
    rs: Resampler,
    round: u64,
    pass: u64,
    /// Wait for decoders that have not started yet (a session's first block, a render); otherwise they play silence.
    wait: bool,
    /// A render's cancel flag: every decoder is waited for, however slow, until it is set.
    pub render: Option<Arc<AtomicBool>>,
    track: Vec<f32>,
    pcm: Vec<f32>,
    /// First decoder error since it was last taken.
    pub error: Option<String>,
    /// FX state by clip id (events playing in the last block) and track id, and the master's.
    ev_fx: HashMap<String, Chain>,
    tr_fx: HashMap<String, Chain>,
    master_fx: Chain,
    /// The last block had Bypass All Audio FX on: turning it off starts the chains fresh.
    bypassed: bool,
    /// Volume and Pan by track and bus id, and gain by clip id, in the last block: a change ramps across the next block.
    tr_last: HashMap<String, (f32, f32)>,
    ev_last: HashMap<String, f32>,
    /// R8a: FX state by bus id; one input buffer per bus of the plan; send levels by (track id, bus id) in the last block.
    bus_fx: HashMap<String, Chain>,
    bus_buf: Vec<Vec<f32>>,
    send_last: HashMap<(String, String), f32>,
    /// R8a: each metered node's last 50 ms (frames, sum of squares per channel) and the block that last fed it.
    rms: HashMap<String, (u64, VecDeque<(usize, [f32; MAX])>)>,
    /// R17b: the source each track with multicamera standby takes played last in the last block, and a scratch buffer
    /// the standby takes are read into.
    heard: HashMap<String, (PathBuf, u32)>,
    sb: Vec<f32>,
    /// R34a: plug-in delay compensation: a delay line per path (event, track output, send), and a scratch buffer; the
    /// end of the last block (a block that does not follow it pre-rolls the plug-ins).
    lines: HashMap<String, Line>,
    scratch: Vec<f32>,
    last_end: Option<i64>,
    /// R10a: in a surround project the master FX run per stereo pair (a chain each); the pair scratch buffer; the
    /// stems of "Enable multichannel mapping" (`AudioPlan::stems`: the busses that go to the master, kept apart); a
    /// stereo scratch for folding the master down.
    master_fx_n: Vec<Chain>,
    fxbuf: Vec<f32>,
    stems: Vec<Vec<f32>>,
    pub(crate) fold: Vec<f32>,
}

/// R34a: plug-in delay compensation for one plan. Every path from an event to the master is delayed so that all of
/// them take `total` samples (the longest one's latency); the mixer runs `total` samples ahead of the timeline, so the
/// output lands on time, sample-exact. Latencies come from the plug-ins (`afx::latency`); built-ins report none.
#[derive(Debug, Default, PartialEq)]
pub struct Lat {
    pub total: usize,
    event: HashMap<String, usize>,
    track: HashMap<String, usize>,
    send: HashMap<(String, String), usize>,
}

impl Lat {
    pub fn of(plan: &AudioPlan, fx_on: bool) -> Lat {
        let mut lat = Lat::default();
        if !fx_on {
            return lat;
        }
        let l = |fx: &[crate::project::Fx]| crate::engine::afx::latency(fx);
        let master = l(&plan.master_fx);
        let own: Vec<usize> = plan.buses.iter().map(|b| l(&b.fx)).collect();
        // Downstream of bus k's output: a later bus (its own latency and its downstream) or the master.
        let mut down = vec![master; own.len()];
        for k in (0..own.len()).rev() {
            down[k] = match plan.buses[k].out {
                Some(o) if o > k => own[o] + down[o],
                _ => master,
            };
        }
        // From a signal entering bus `b` (or the master) to the output.
        let into = |b: Option<usize>| b.map_or(master, |b| own[b] + down[b]);
        let mut total = master;
        let paths: Vec<(usize, usize)> = plan
            .tracks
            .iter()
            .map(|tr| {
                let max_e = tr.events.iter().map(|e| l(&e.fx)).max().unwrap_or(0);
                let own = l(&tr.fx);
                total = total.max(max_e + own + into(tr.out));
                for s in &tr.sends {
                    total = total.max(max_e + own + into(Some(s.to)));
                }
                (max_e, own)
            })
            .collect();
        if total == 0 {
            return lat;
        }
        lat.total = total;
        for (tr, (max_e, own)) in plan.tracks.iter().zip(paths) {
            for e in &tr.events {
                let d = max_e - l(&e.fx);
                if d > 0 {
                    lat.event.insert(e.id.clone(), d);
                }
            }
            let d = total - (max_e + own + into(tr.out));
            if d > 0 {
                lat.track.insert(tr.id.clone(), d);
            }
            for s in &tr.sends {
                let d = total - (max_e + own + into(Some(s.to)));
                if d > 0 {
                    lat.send.insert((tr.id.clone(), s.id.clone()), d);
                }
            }
        }
        lat
    }

    fn event(&self, id: &str) -> usize {
        self.event.get(id).copied().unwrap_or(0)
    }

    fn track(&self, id: &str) -> usize {
        self.track.get(id).copied().unwrap_or(0)
    }

    fn send(&self, key: &(String, String)) -> usize {
        self.send.get(key).copied().unwrap_or(0)
    }
}

/// A delay line of a fixed length (interleaved frames, R10a: of any width); it starts silent and again when its
/// length or width changes.
#[derive(Default)]
struct Line {
    buf: Vec<f32>,
    w: usize,
    used: u64,
}

/// `src` (frames `ch` wide) delayed by `d` frames through the line `key` (made when needed), in `scratch`; `src`
/// itself when `d` is 0.
#[allow(clippy::too_many_arguments)]
fn delayed<'a>(lines: &mut HashMap<String, Line>, key: &str, d: usize, round: u64, src: &'a [f32], scratch: &'a mut Vec<f32>, ch: usize) -> &'a [f32] {
    if d == 0 {
        return src;
    }
    let line = lines.entry(key.to_string()).or_default();
    if line.buf.len() != ch * d {
        (line.buf, line.w) = (vec![0.0; ch * d], 0);
    }
    line.used = round;
    scratch.clear();
    scratch.extend_from_slice(src);
    for fr in scratch.chunks_exact_mut(ch) {
        for (c, x) in fr.iter_mut().enumerate() {
            std::mem::swap(&mut line.buf[line.w + c], x);
        }
        line.w = (line.w + ch) % line.buf.len();
    }
    scratch
}

/// R10a: `fx` over every stereo pair of `buf` (frames `ch` wide) through the chains keyed `<id>#<pair>` (taken from
/// `old`, put in `new`); a stereo buffer through the chain keyed `id`, as before.
#[allow(clippy::too_many_arguments)]
fn run_wide(old: &mut HashMap<String, Chain>, new: &mut HashMap<String, Chain>, id: &str, fx: &[crate::project::Fx], t: f64, buf: &mut [f32], ch: usize, scratch: &mut Vec<f32>) {
    if ch == 2 {
        let mut chain = old.remove(id).unwrap_or_default();
        chain.run(fx, t, buf);
        new.insert(id.to_string(), chain);
        return;
    }
    for pair in 0..ch / 2 {
        let key = format!("{id}#{pair}");
        let mut chain = old.remove(&key).unwrap_or_default();
        run_pair(&mut chain, fx, pair, t, buf, ch, scratch);
        new.insert(key, chain);
    }
}

/// R10a: the left-right side of channel `c` of a surround frame: -1 left, 1 right, 0 the centre and LFE.
#[inline]
fn side(c: usize) -> i8 {
    match c {
        surround::FL | surround::BL | surround::SL => -1,
        surround::FR | surround::BR | surround::SR => 1,
        _ => 0,
    }
}

/// R10a: the Balance 0 dB gain of side `s` at pan `p`.
#[inline]
fn balance(s: i8, p: f32) -> f32 {
    match s {
        -1 => (1.0 - p).min(1.0),
        1 => (1.0 + p).min(1.0),
        _ => 1.0,
    }
}

/// R10a: frames of `sch` channels added into frames of `dch` channels (converted by `surround::convert` when they
/// differ), each frame times `g(i)` and Balance 0 dB at pan `p(i)` by the destination channel's side (a stereo
/// destination: channel 0 left, 1 right). Fed to `lv`.
#[allow(clippy::too_many_arguments)]
fn route(src: &[f32], sch: usize, dst: &mut [f32], dch: usize, conv: &mut Vec<f32>, g: impl Fn(usize) -> f32, p: impl Fn(usize) -> f32, lv: &mut Level) {
    let src: &[f32] = if sch == dch {
        src
    } else {
        surround::convert(src, sch, dch, conv);
        conv
    };
    for (i, (o, x)) in dst.chunks_exact_mut(dch).zip(src.chunks_exact(dch)).enumerate() {
        let (g, p) = (g(i), p(i).clamp(-1.0, 1.0));
        for (c, (o, x)) in o.iter_mut().zip(x).enumerate() {
            let s = if dch == 2 { [-1, 1][c] } else { side(c) };
            let v = x * g * balance(s, p);
            *o += v;
            lv.add_ch(c, v);
        }
    }
}

/// R10a: run `fx` over stereo pair `pair` (channels 2·pair and 2·pair + 1) of the `ch`-channel interleaved `buf` through
/// `chain`, de-interleaved into `scratch`. Only the first pair runs external (VST3 / CLAP) plug-ins: they are stereo
/// instances keyed by the FX id, so the other pairs run the built-in plug-ins of the chain alone.
// shortcut: a bus or master dynamics plug-in is not linked across the pairs in a surround project.
fn run_pair(chain: &mut Chain, fx: &[crate::project::Fx], pair: usize, t: f64, buf: &mut [f32], ch: usize, scratch: &mut Vec<f32>) {
    let n = buf.len() / ch.max(1);
    scratch.clear();
    scratch.reserve(n * 2);
    for f in buf.chunks_exact(ch) {
        scratch.extend_from_slice(&f[2 * pair..2 * pair + 2]);
    }
    let own: Vec<crate::project::Fx>;
    let run: &[crate::project::Fx] = if pair == 0 || !fx.iter().any(|f| crate::engine::aplug::is_ext(&f.plugin)) {
        fx
    } else {
        own = fx.iter().filter(|f| !crate::engine::aplug::is_ext(&f.plugin)).cloned().collect();
        &own
    };
    chain.run(run, t, scratch);
    for (f, s) in buf.chunks_exact_mut(ch).zip(scratch.chunks_exact(2)) {
        f[2 * pair..2 * pair + 2].copy_from_slice(s);
    }
}

/// R10a: a block of `plan` at `f0` folded down to stereo in `out` (mixed straight into it in a stereo project).
pub fn block_stereo(m: &mut Mixer, plan: &AudioPlan, f0: i64, out: &mut [f32]) {
    let ch = plan.layout.channels();
    if ch == 2 {
        return m.block(plan, f0, out);
    }
    let n = out.len() / 2;
    let mut wide = std::mem::take(&mut m.fold);
    wide.clear();
    wide.resize(n * ch, 0.0);
    m.block(plan, f0, &mut wide);
    for (o, f) in out.chunks_exact_mut(2).zip(wide.chunks_exact(ch)) {
        o.copy_from_slice(&surround::downmix_frame(f));
    }
    m.fold = wide;
}

/// A volume envelope's linear gain at `t0` and `t1` (unity without one).
fn env_gain(env: &Option<Envelope>, t0: f64, t1: f64) -> (f32, f32) {
    env.as_ref().map_or((1.0, 1.0), |e| {
        let (a, b) = ramp(e, t0, t1);
        (db_to_gain(a), db_to_gain(b))
    })
}

impl Mixer {
    pub fn ffmpeg() -> Self {
        Self::new(Box::new(spawn))
    }

    fn new(open: Open) -> Self {
        Self {
            open,
            streams: Vec::new(),
            vstreams: HashMap::new(),
            rs: Resampler::default(),
            round: 0,
            pass: 0,
            wait: true,
            render: None,
            track: Vec::new(),
            pcm: Vec::new(),
            error: None,
            ev_fx: HashMap::new(),
            tr_fx: HashMap::new(),
            master_fx: Chain::default(),
            bypassed: false,
            tr_last: HashMap::new(),
            ev_last: HashMap::new(),
            bus_fx: HashMap::new(),
            bus_buf: Vec::new(),
            send_last: HashMap::new(),
            rms: HashMap::new(),
            heard: HashMap::new(),
            sb: Vec::new(),
            lines: HashMap::new(),
            scratch: Vec::new(),
            last_end: None,
            master_fx_n: Vec::new(),
            fxbuf: Vec::new(),
            stems: Vec::new(),
            fold: Vec::new(),
        }
    }

    /// R10a: the stems of the last block (`AudioPlan::stems`): one interleaved stereo buffer per bus of the plan, in
    /// plan order, holding what busses that go to the master sent (the others are silent).
    pub fn stems(&self) -> &[Vec<f32>] {
        &self.stems
    }

    /// R8a: feed node `id`'s meter (if the plan has one) with a block of `frames` frames; R10a: `ch` channels of it.
    fn meter(&mut self, plan: &AudioPlan, id: &str, frames: usize, lv: Level, ch: usize) {
        let Some(m) = plan.meters.as_ref().and_then(|m| m.get(id)) else { return };
        if !self.rms.contains_key(id) {
            self.rms.insert(id.to_string(), (0, VecDeque::new()));
        }
        let (fed, w) = self.rms.get_mut(id).expect("just added");
        *fed = self.round;
        w.push_back((frames, lv.sq));
        let mut n: usize = w.iter().map(|e| e.0).sum();
        while w.len() > 1 && n - w[0].0 >= METER_WINDOW {
            n -= w.pop_front().map_or(0, |e| e.0);
        }
        let ch = ch.clamp(1, MAX);
        let sq = w.iter().fold([0.0f32; MAX], |a, e| std::array::from_fn(|c| a[c] + e.1[c]));
        m.channels.store(ch, Relaxed);
        for c in 0..ch {
            m.peak[c].fetch_max(lv.peak[c].to_bits(), Relaxed);
            m.rms[c].store((sq[c] / n.max(1) as f32).sqrt().to_bits(), Relaxed);
        }
    }

    /// R10a: playback in a surround project: meter the master's channels (after the master fader, as the device tap
    /// does for stereo) and hand them to the Loudness Meters' ring.
    pub fn tap_master(&mut self, plan: &AudioPlan, buf: &[f32], ch: usize) {
        let g = plan.master.as_ref().map_or([1.0; 2], |m| [m.gain(0), m.gain(1)]);
        let n = buf.len() / ch.max(1);
        let gains: [f32; MAX] = std::array::from_fn(|c| match c {
            surround::FL | surround::BL | surround::SL => g[0],
            surround::FR | surround::BR | surround::SR => g[1],
            _ => (g[0] + g[1]) * 0.5,
        });
        let mut lv = Level::default();
        self.fold.clear();
        for f in buf.chunks_exact(ch) {
            for (c, x) in f.iter().enumerate() {
                let v = x * gains[c];
                lv.add_ch(c, v);
                self.fold.push(v);
            }
        }
        self.meter(plan, MASTER_METER, n, lv, ch);
        if let Some(ring) = &plan.loud {
            ring.push_frames(&self.fold, ch);
        }
    }

    /// The stream that continues `src` at `at` frames into its event, opened if none does.
    fn stream(&mut self, src: &ASrc, at: i64) -> &mut Live {
        let pass = self.pass;
        let i = match self.streams.iter().position(|l| l.pass != pass && l.continues(src, at)) {
            Some(i) => i,
            None => {
                // A failed spawn leaves an ended stream, so it is not retried every block.
                let s = (self.open)(src, at).unwrap_or_else(|e| {
                    self.error.get_or_insert(e);
                    Stream::ended()
                });
                self.streams.push(Live { src: src.clone(), next: at, t: src_time(src, at), used: 0, pass: 0, s });
                self.streams.len() - 1
            }
        };
        let l = &mut self.streams[i];
        (l.used, l.pass) = (self.round, pass);
        l
    }

    /// The decoder of varispeed event `id`, opened (or opened again) so it holds media time `need` (seconds) on. A new
    /// one starts at the earliest time the event reaches from `local` (seconds into it) to its end.
    fn vlive(&mut self, id: &str, src: &ASrc, v: &Varispeed, local: f64, need: f64) -> &mut VLive {
        let key = ASrc { pos: 0.0, dur: v.len, in_point: 0.0, rate: 1.0, reverse: false, ..src.clone() };
        let same = |l: &VLive| {
            (&l.key.path, l.key.stream, l.key.pitch, l.key.dur, &l.key.nested) == (&key.path, key.stream, key.pitch, key.dur, &key.nested)
                && (l.key.in_point - l.s0 - v.offset).abs() < 1e-9
                && l.s0 <= need.max(0.0) + 1e-9
        };
        if !self.vstreams.get(id).is_some_and(same) {
            let s0 = (v.earliest(local).min(need) - 0.05).max(0.0);
            let key = ASrc { in_point: v.offset + s0, ..key };
            let s = (self.open)(&key, 0).unwrap_or_else(|e| {
                self.error.get_or_insert(e);
                Stream::ended()
            });
            self.vstreams.insert(id.to_string(), VLive { key, s0, buf: Vec::new(), ended: false, used: 0, s });
        }
        let l = self.vstreams.get_mut(id).expect("just opened");
        l.used = self.round;
        l
    }

    /// R4b: frames [k0, k1) of varispeed event `e` (counted from its start) into `out`: its media at each frame's time
    /// (`Varispeed::time`), band-limited to the speed there. Silence slower than MIN_SPEED and where the event is past
    /// its media.
    fn varispeed(&mut self, e: &crate::engine::plan::AEvent, v: &Varispeed, k0: i64, k1: i64, out: &mut [f32], wait: bool, stop: Option<&AtomicBool>) {
        let r = RATE as f64;
        // Each frame's unwrapped media time, and the next one's (the last frame's speed).
        let times: Vec<f64> = (k0..=k1).map(|k| v.time(k as f64 / r)).collect();
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for w in times.windows(2) {
            if let Some(s) = v.at(w[0]) {
                let d = self.rs.reach((w[1] - w[0]) * r) as f64 / r;
                (lo, hi) = (lo.min(s - d), hi.max(s + d));
            }
        }
        out.fill(0.0);
        if lo > hi {
            return; // silent throughout
        }
        let l = self.vlive(&e.id, &e.src, v, k0 as f64 / r, lo);
        if let Some(err) = l.fill(((hi - l.s0) * r).ceil() as usize + 1, wait, stop) {
            self.error.get_or_insert(err);
        }
        let l = &self.vstreams[&e.id];
        // R10a: a 5.1 / 7.1 track's frames are wider.
        let ch = e.src.ch.max(2) as usize;
        for (o, w) in out.chunks_exact_mut(ch).zip(times.windows(2)) {
            let speed = (w[1] - w[0]) * r;
            if let Some(s) = v.at(w[0]).filter(|_| speed.abs() >= MIN_SPEED) {
                if ch == 2 {
                    let [a, b] = self.rs.read(&l.buf, (s - l.s0) * r, speed);
                    (o[0], o[1]) = (a, b);
                } else {
                    self.rs.read_n(&l.buf, ch, (s - l.s0) * r, speed, o);
                }
            }
        }
    }

    /// R17b: read multicamera event `e`'s other takes over frames [a, b) (`ps` its start) of the block, so each one's
    /// decoder keeps up, and drop them: a take switched to while playing goes on from its running decoder, which this
    /// leaves free for the event that plays it next in the same block. The first block of an event heard from part way
    /// in after another take of it on the track (`was`: a take switch heard late) crossfades from that take over the
    /// quickfade, so the switch does not click.
    #[allow(clippy::too_many_arguments)]
    fn standby(&mut self, e: &crate::engine::plan::AEvent, track: &str, (a, b, ps): (i64, i64, i64), was: Option<&(PathBuf, u32)>, fresh: bool, qf: f64, pcm: &mut [f32]) {
        let r = RATE as f64;
        let mut sb = std::mem::take(&mut self.sb);
        for src in &e.standby {
            sb.clear();
            sb.resize(pcm.len(), 0.0);
            let l = self.stream(src, a - ps);
            (l.next, l.t, l.pass) = (b - ps, l.t + (b - a) as f64 * src.rate / r, 0);
            if let Some(err) = l.s.read(&mut sb, false, None) {
                self.error.get_or_insert(err);
            }
            let w = src.ch.max(2) as usize;
            let n = ((qf * r) as usize).min(pcm.len() / w);
            if fresh && a > ps && n > 0 && was.is_some_and(|(p, s)| *p == src.path && *s == src.stream) {
                for (i, (o, x)) in pcm.chunks_exact_mut(w).zip(sb.chunks_exact(w)).take(n).enumerate() {
                    let g = i as f32 / n as f32;
                    for (o, x) in o.iter_mut().zip(x) {
                        *o = x + (*o - x) * g;
                    }
                }
            }
        }
        self.sb = sb;
        self.heard.insert(track.to_string(), (e.src.path.clone(), e.src.stream));
    }

    /// Open the streams for what plays in frames [w0, w1) now, so their first decode is not heard as a gap.
    pub fn warm(&mut self, plan: &AudioPlan, w0: i64, w1: i64) {
        self.pass += 1;
        for e in plan.tracks.iter().flat_map(|t| &t.events) {
            // R17b: the multicamera takes that may be switched to, too.
            for src in e.standby.iter().filter(|_| plan.standby && e.vel.is_none()) {
                let (ps, pe) = (frame(src.pos), frame(src.pos + src.dur));
                let a = w0.max(ps);
                if a < pe.min(w1) {
                    self.stream(src, a - ps).pass = 0;
                }
            }
            if let Some(v) = &e.vel {
                let (ps, pe) = (frame(e.src.pos), frame(e.src.pos + e.src.dur));
                let a = w0.max(ps);
                if a < pe.min(w1) {
                    self.vlive(&e.id, &e.src, v, (a - ps) as f64 / RATE as f64, f64::INFINITY);
                }
                continue;
            }
            // Each pass of a looping event too, so the stream after a wrap is ready when it is heard.
            for src in &e.passes {
                let (ps, pe) = (frame(src.pos), frame(src.pos + src.dur));
                let a = w0.max(ps);
                if a < pe.min(w1) {
                    self.stream(src, a - ps);
                }
            }
        }
    }

    /// Mix `out.len() / 2` stereo frames from timeline frame `f0`: per event PCM, event FX, then x gain x fades x volume
    /// envelope; per track the track FX, then the volume, mute and pan envelopes (ramped across the block) x Volume and
    /// Pan (its Pan Type) into its output, and its sends (pre-volume: after the track FX); then each bus (R8a), in
    /// order, its FX, envelopes, Volume and Pan into its output; then the master FX. Event FX stop at the event's end;
    /// track and bus FX run on past it, so their tails are heard.
    /// R10a: `out` holds `plan.layout.channels()` interleaved channels per frame.
    pub fn block(&mut self, plan: &AudioPlan, f0: i64, out: &mut [f32]) {
        // R34a: plug-in delay compensation. The plan's paths are delayed to the longest latency, and the mix runs that
        // far ahead of the timeline. A block that does not follow the last one (the first, a seek) first pre-rolls the
        // frames before it, so the plug-ins and the delay lines hold the right past when it starts.
        let lat = Lat::of(plan, !plan.bypass_fx);
        if self.rs.taps() != plan.taps {
            self.rs = Resampler::new(plan.taps);
        }
        let n = out.len() / plan.layout.channels();
        if lat.total > 0 && n > 0 && self.last_end != Some(f0) {
            let mut scratch = vec![0.0; out.len()];
            let blocks = lat.total.div_ceil(n).min(8) as i64;
            for k in (1..=blocks).rev() {
                self.block_raw(plan, f0 + lat.total as i64 - k * n as i64, &mut scratch, &lat);
            }
        }
        self.block_raw(plan, f0 + lat.total as i64, out, &lat);
        self.last_end = Some(f0 + n as i64);
    }

    /// `block` at raw position `f0` (the timeline shifted by the compensated latency), with the paths' delays.
    fn block_raw(&mut self, plan: &AudioPlan, f0: i64, out: &mut [f32], lat: &Lat) {
        // Streams nothing used in the last block, or warmed for this one, are closed.
        let round = self.round;
        self.streams.retain(|l| l.used == round);
        self.vstreams.retain(|_, l| l.used == round);
        self.round += 1;
        self.pass += 1;
        out.fill(0.0);
        // R10a: the layout's channels in `out` and the bus buffers; tracks and events stay stereo.
        let ch = plan.layout.channels();
        let n = out.len() / ch;
        let (f1, r) = (f0 + n as i64, RATE as f64);
        let (mut track, mut pcm) = (std::mem::take(&mut self.track), std::mem::take(&mut self.pcm));
        let stop = self.render.clone();
        let fx_on = !plan.bypass_fx;
        if self.bypassed != plan.bypass_fx {
            (self.ev_fx, self.tr_fx, self.master_fx, self.bypassed) = (HashMap::new(), HashMap::new(), Chain::default(), plan.bypass_fx);
            self.bus_fx = HashMap::new();
        }
        // Chains of events, tracks and busses no longer playing are dropped with these.
        let (mut ev_old, mut tr_old, mut bus_old) = (std::mem::take(&mut self.ev_fx), std::mem::take(&mut self.tr_fx), std::mem::take(&mut self.bus_fx));
        let (mut tr_last, mut ev_last) = (std::mem::take(&mut self.tr_last), std::mem::take(&mut self.ev_last));
        let mut send_last = std::mem::take(&mut self.send_last);
        let heard_old = std::mem::take(&mut self.heard);
        let (mut lines, mut scratch) = (std::mem::take(&mut self.lines), std::mem::take(&mut self.scratch));
        let mut bus_buf = std::mem::take(&mut self.bus_buf);
        bus_buf.resize_with(plan.buses.len(), Vec::new);
        for (b, ab) in bus_buf.iter_mut().zip(&plan.buses) {
            b.clear();
            b.resize(n * ab.ch.max(2), 0.0);
        }
        // R10a: the stems of "Enable multichannel mapping" (stereo projects only).
        let stems = plan.stems && ch == 2;
        self.stems.resize_with(if stems { plan.buses.len() } else { 0 }, Vec::new);
        for st in &mut self.stems {
            st.clear();
            st.resize(out.len(), 0.0);
        }
        let mut bus_heard = vec![false; plan.buses.len()];
        let mut conv = Vec::new();
        for tr in &plan.tracks {
            // R10a: the track's width (2, or a 5.1 / 7.1 track's 6 / 8).
            let tw = tr.ch.max(2);
            track.clear();
            track.resize(n * tw, 0.0);
            let mut heard = false;
            // A fader moved since the last block glides to its new value (a step is a click).
            let (vol0, pan0) = tr_last.remove(&tr.id).unwrap_or((tr.volume, tr.pan));
            self.tr_last.insert(tr.id.clone(), (tr.volume, tr.pan));
            for e in &tr.events {
                let (ps, pe) = (frame(e.src.pos), frame(e.src.pos + e.src.dur));
                let (a, b) = (f0.max(ps), f1.min(pe));
                if b <= a {
                    continue;
                }
                heard = true;
                pcm.clear();
                pcm.resize((b - a) as usize * tw, 0.0);
                let wait = self.wait;
                if let Some(v) = &e.vel {
                    self.varispeed(e, v, a - ps, b - ps, &mut pcm, wait, stop.as_deref());
                }
                // A looping event reads each pass through its media from a stream of its own.
                for src in e.passes.iter().filter(|_| e.vel.is_none()) {
                    let qs = frame(src.pos);
                    let (pa, pb) = (a.max(qs), b.min(frame(src.pos + src.dur)));
                    if pb <= pa {
                        continue;
                    }
                    let l = self.stream(src, pa - qs);
                    (l.next, l.t) = (pb - qs, l.t + (pb - pa) as f64 * src.rate / r);
                    if let Some(err) = l.s.read(&mut pcm[(pa - a) as usize * tw..(pb - a) as usize * tw], wait, stop.as_deref()) {
                        self.error.get_or_insert(err);
                    }
                }
                if plan.standby && e.vel.is_none() {
                    self.standby(e, &tr.id, (a, b, ps), heard_old.get(&tr.id), !ev_last.contains_key(&e.id), plan.quickfade, &mut pcm);
                }
                if tw == 2 {
                    map_channels(&mut pcm, e.channels, e.invert);
                    // R10a: a mono track hears the mean of both sides on both.
                    if tr.mono {
                        map_channels(&mut pcm, ChannelMap::Combine, false);
                    }
                } else if e.invert {
                    pcm.iter_mut().for_each(|v| *v = -*v);
                }
                if fx_on && !e.fx.is_empty() {
                    run_wide(&mut ev_old, &mut self.ev_fx, &e.id, &e.fx, (a - ps) as f64 / r, &mut pcm, tw, &mut self.fxbuf);
                }
                // R34a: this event's FX are shorter than another's on the track: delayed to meet them.
                let d = lat.event(&e.id);
                if d > 0 {
                    delayed(&mut lines, &e.id, d, self.round, &pcm, &mut scratch, tw);
                    std::mem::swap(&mut pcm, &mut scratch);
                }
                let (la, lb) = ((a - ps) as f64 / r, (b - ps) as f64 / r);
                let (v0, v1) = env_gain(&e.vol_env, la, lb);
                // R8c: the event's Mute and Pan (Balance 0 dB) envelopes.
                let (m0, m1) = e.mute_env.as_ref().map_or((1.0, 1.0), |x| ramp(x, la, lb));
                let (p0, p1) = e.pan_env.as_ref().map_or((0.0, 0.0), |x| ramp(x, la, lb));
                let g0 = ev_last.remove(&e.id).unwrap_or(e.gain);
                self.ev_last.insert(e.id.clone(), e.gain);
                let len = (b - a) as f32;
                let shaped = e.pan_env.is_some() || e.mute_env.is_some();
                // R6b: a nested timeline's edge where none of its own events plays across it is already quickfaded (or
                // silent) inside it: a second ramp would change the sound Create Nested Timeline must keep.
                let (qf_in, qf_out) = match &e.src.nested {
                    Some(n) => {
                        let (s0, s1) = (e.src.in_point, e.src.in_point + e.src.dur * e.src.rate);
                        let (t_in, t_out) = if e.src.reverse { (s1, s0) } else { (s0, s1) };
                        (n.sounds_across(t_in), n.sounds_across(t_out))
                    }
                    None => (true, true),
                };
                for (i, (o, x)) in track[(a - f0) as usize * tw..].chunks_exact_mut(tw).zip(pcm.chunks_exact(tw)).enumerate() {
                    let local = (a - ps + i as i64) as f64 / r;
                    let s = i as f32 / len;
                    let mut g = (g0 + (e.gain - g0) * s) * (v0 + (v1 - v0) * s);
                    if local < e.fade_in {
                        g *= fade(e.fade_in_curve, local, e.fade_in);
                    }
                    if e.src.dur - local < e.fade_out {
                        g *= fade(e.fade_out_curve, e.src.dur - local, e.fade_out);
                    }
                    // Quickfade Audio Edits: a short fade at an edge without one of its own, so a cut never clicks.
                    let qf = plan.quickfade;
                    if local < qf && e.fade_in <= 0.0 && qf_in {
                        g *= fade(Curve::Linear, local, qf);
                    }
                    if e.src.dur - local < qf && e.fade_out <= 0.0 && qf_out {
                        g *= fade(Curve::Linear, (e.src.dur - local).max(0.0), qf);
                    }
                    if tw > 2 {
                        // R10a: a 5.1 / 7.1 event; its Pan envelope is a left-right balance over the room.
                        let p = if shaped {
                            g *= (m0 + (m1 - m0) * s).clamp(0.0, 1.0);
                            (p0 + (p1 - p0) * s).clamp(-1.0, 1.0)
                        } else {
                            0.0
                        };
                        for (c, (o, x)) in o.iter_mut().zip(x).enumerate() {
                            *o += x * g * balance(side(c), p);
                        }
                    } else if shaped {
                        g *= (m0 + (m1 - m0) * s).clamp(0.0, 1.0);
                        let p = (p0 + (p1 - p0) * s).clamp(-1.0, 1.0);
                        o[0] += x[0] * g * (1.0 - p).min(1.0);
                        o[1] += x[1] * g * (1.0 + p).min(1.0);
                    } else {
                        o[0] += x[0] * g;
                        o[1] += x[1] * g;
                    }
                }
            }
            let (t0, t1) = (f0 as f64 / r, f1 as f64 / r);
            if fx_on && !tr.fx.is_empty() {
                run_wide(&mut tr_old, &mut self.tr_fx, &tr.id, &tr.fx, t0, &mut track, tw, &mut self.fxbuf);
            } else if !heard {
                continue;
            }
            let (v0, v1) = env_gain(&tr.vol_env, t0, t1);
            let (m0, m1) = tr.mute_env.as_ref().map_or((1.0, 1.0), |e| ramp(e, t0, t1));
            let (p0, p1) = tr.pan_env.as_ref().map_or((0.0, 0.0), |e| ramp(e, t0, t1));
            let inv = if tr.invert { -1.0 } else { 1.0 };
            // The fader: Volume x volume and mute envelopes, at frame i.
            let fader = |i: usize| {
                let s = i as f32 / n as f32;
                let lerp = |a: f32, b: f32| a + (b - a) * s;
                lerp(vol0, tr.volume) * lerp(v0, v1) * lerp(m0, m1).clamp(0.0, 1.0)
            };
            let mut lv = Level::default();
            // R10a: the width of what the track goes into (a bus's layout, or the master's).
            let dch = tr.out.map_or(ch, |b| plan.buses[b].ch);
            if !tr.muted {
                let dest: &mut [f32] = match tr.out {
                    Some(b) => {
                        bus_heard[b] = true;
                        &mut bus_buf[b]
                    }
                    None => &mut *out,
                };
                // R34a: delayed so this path takes as long as the longest one.
                let main_src = delayed(&mut lines, &tr.id, lat.track(&tr.id), self.round, &track, &mut scratch, tw);
                if tw > 2 {
                    // R10a: a 5.1 / 7.1 track goes channel for channel (converted to another layout), its Pan and Pan
                    // envelope a left-right balance.
                    let pan = |i: usize| {
                        let s = i as f32 / n as f32;
                        pan0 + (tr.pan - pan0) * s + p0 + (p1 - p0) * s
                    };
                    route(main_src, tw, dest, dch, &mut conv, |i| fader(i) * inv, pan, &mut lv);
                } else if dch == 2 {
                    for (i, (o, x)) in dest.chunks_exact_mut(2).zip(main_src.chunks_exact(2)).enumerate() {
                        let s = i as f32 / n as f32;
                        let lerp = |a: f32, b: f32| a + (b - a) * s;
                        let g = fader(i) * inv;
                        // Pan trims the pan envelope (VEGAS), by the track's Pan Type.
                        let p = (lerp(pan0, tr.pan) + lerp(p0, p1)).clamp(-1.0, 1.0);
                        let m = tr.pan_type.matrix(p);
                        let (l, r) = (x[0] * g, x[1] * g);
                        let (a, b) = (l * m[0][0] + r * m[0][1], l * m[1][0] + r * m[1][1]);
                        o[0] += a;
                        o[1] += b;
                        lv.add(a, b);
                    }
                } else {
                    // R10a: into the room by the surround pan (its keyframes ramped across the block; the Pan slider and
                    // envelope trim its left-right position), by the track's Pan Type.
                    let sp = tr.surround.unwrap_or_default();
                    let (x0, x1) = tr.x_env.as_ref().map_or((sp.x, sp.x), |e| ramp(e, t0, t1));
                    let (y0, y1) = tr.y_env.as_ref().map_or((sp.y, sp.y), |e| ramp(e, t0, t1));
                    let mut last: Option<((f32, f32, f32), [[f32; MAX]; 2])> = None;
                    for (i, (o, x)) in dest.chunks_exact_mut(dch).zip(main_src.chunks_exact(2)).enumerate() {
                        let s = i as f32 / n as f32;
                        let lerp = |a: f32, b: f32| a + (b - a) * s;
                        let g = fader(i) * inv;
                        let key = (lerp(x0, x1), lerp(y0, y1), lerp(pan0, tr.pan) + lerp(p0, p1));
                        let m = match last {
                            Some((k, m)) if k == key => m,
                            _ => {
                                let m = surround::pan_matrix(tr.pan_type, &SurroundPan { x: key.0, y: key.1, ..sp }, key.2, dch);
                                last = Some((key, m));
                                m
                            }
                        };
                        let (l, r) = (x[0] * g, x[1] * g);
                        for (c, o) in o.iter_mut().enumerate() {
                            let v = l * m[0][c] + r * m[1][c];
                            *o += v;
                            lv.add_ch(c, v);
                        }
                    }
                }
            }
            self.meter(plan, &tr.id, n, lv, dch);
            // Sends: centre-panned by the track's Pan Type (VEGAS without Link to Main Track Pan); pre-volume sends are
            // pre-mute too, post-volume ones follow the fader and the mute.
            let centre = tr.pan_type.matrix(0.0);
            for snd in tr.sends.iter().filter(|s| s.pre || !tr.muted) {
                let key = (tr.id.clone(), snd.id.clone());
                let l0 = send_last.remove(&key).unwrap_or(snd.level);
                let d = lat.send(&key);
                self.send_last.insert(key, snd.level);
                bus_heard[snd.to] = true;
                let send_src = delayed(&mut lines, &format!("{}>{}", tr.id, snd.id), d, self.round, &track, &mut scratch, tw);
                let sch = plan.buses[snd.to].ch;
                if tw > 2 {
                    // R10a: a 5.1 / 7.1 track's send, channel for channel.
                    let g = |i: usize| (l0 + (snd.level - l0) * (i as f32 / n as f32)) * inv * if snd.pre { 1.0 } else { fader(i) };
                    route(send_src, tw, &mut bus_buf[snd.to], sch, &mut conv, g, |_| 0.0, &mut Level::default());
                } else if sch == 2 {
                    for (i, (o, x)) in bus_buf[snd.to].chunks_exact_mut(2).zip(send_src.chunks_exact(2)).enumerate() {
                        let s = i as f32 / n as f32;
                        let g = (l0 + (snd.level - l0) * s) * inv * if snd.pre { 1.0 } else { fader(i) };
                        let (l, r) = (x[0] * g, x[1] * g);
                        o[0] += l * centre[0][0] + r * centre[0][1];
                        o[1] += l * centre[1][0] + r * centre[1][1];
                    }
                } else {
                    // R10a: placed where the track's pan point is, left-right centred.
                    let m = surround::pan_matrix(tr.pan_type, &tr.surround.unwrap_or_default(), 0.0, sch);
                    for (i, (o, x)) in bus_buf[snd.to].chunks_exact_mut(sch).zip(send_src.chunks_exact(2)).enumerate() {
                        let s = i as f32 / n as f32;
                        let g = (l0 + (snd.level - l0) * s) * inv * if snd.pre { 1.0 } else { fader(i) };
                        let (l, r) = (x[0] * g, x[1] * g);
                        for (c, o) in o.iter_mut().enumerate() {
                            *o += l * m[0][c] + r * m[1][c];
                        }
                    }
                }
            }
        }
        // R8a: the busses, each after every bus that feeds it (`AudioPlan::buses` order), into a later bus or the master.
        let (t0, t1) = (f0 as f64 / r, f1 as f64 / r);
        for (k, b) in plan.buses.iter().enumerate() {
            let (head, tail) = bus_buf.split_at_mut(k + 1);
            let input = &mut head[k];
            // R10a: the bus's width, and its destination's.
            let bw = b.ch.max(2);
            if fx_on && !b.fx.is_empty() {
                // R10a: a 5.1 / 7.1 bus runs its FX per stereo pair, a chain each (keyed "<bus>#<pair>").
                run_wide(&mut bus_old, &mut self.bus_fx, &b.id, &b.fx, t0, input, bw, &mut self.fxbuf);
            } else if !bus_heard[k] {
                continue;
            }
            if b.mono {
                map_channels(input, ChannelMap::Combine, false);
            }
            let dch = b.out.map_or(ch, |o| plan.buses[o].ch.max(2));
            let (vol0, pan0) = tr_last.remove(&b.id).unwrap_or((b.volume, b.pan));
            self.tr_last.insert(b.id.clone(), (b.volume, b.pan));
            let (v0, v1) = env_gain(&b.vol_env, t0, t1);
            let (m0, m1) = b.mute_env.as_ref().map_or((1.0, 1.0), |e| ramp(e, t0, t1));
            let (p0, p1) = b.pan_env.as_ref().map_or((0.0, 0.0), |e| ramp(e, t0, t1));
            let dest: &mut [f32] = match b.out {
                Some(o) => {
                    bus_heard[o] = true;
                    &mut tail[o - k - 1]
                }
                // R10a: a stem of its own instead of the master when multichannel mapping asks.
                None if stems => &mut self.stems[k],
                None => &mut *out,
            };
            let mut lv = Level::default();
            if bw == 2 && dch == 2 {
                for (i, (o, x)) in dest.chunks_exact_mut(2).zip(input.chunks_exact(2)).enumerate() {
                    let s = i as f32 / n as f32;
                    let lerp = |a: f32, b: f32| a + (b - a) * s;
                    let g = lerp(vol0, b.volume) * lerp(v0, v1) * lerp(m0, m1).clamp(0.0, 1.0);
                    let p = (lerp(pan0, b.pan) + lerp(p0, p1)).clamp(-1.0, 1.0);
                    let (l, r) = (x[0] * g * (1.0 - p).min(1.0), x[1] * g * (1.0 + p).min(1.0));
                    o[0] += l;
                    o[1] += r;
                    lv.add(l, r);
                }
            } else {
                // R10a: the bus's Pan is a left-right balance over the room (Balance 0 dB); the centre and LFE keep
                // their level. Another layout is converted on the way (`surround::convert`).
                let lerp = |i: usize, a: f32, b: f32| a + (b - a) * (i as f32 / n as f32);
                let g = |i: usize| lerp(i, vol0, b.volume) * lerp(i, v0, v1) * lerp(i, m0, m1).clamp(0.0, 1.0);
                let p = |i: usize| lerp(i, pan0, b.pan) + lerp(i, p0, p1);
                route(input, bw, dest, dch, &mut conv, g, p, &mut lv);
            }
            self.meter(plan, &b.id, n, lv, dch);
        }
        // A meter nothing fed this block falls to silence.
        let round = self.round;
        if let Some(m) = &plan.meters {
            self.rms.retain(|id, (fed, _)| {
                let keep = *fed == round;
                if !keep {
                    if let Some(m) = m.get(id) {
                        m.rms.iter().for_each(|a| a.store(0, Relaxed));
                    }
                }
                keep
            });
        }
        self.bus_buf = bus_buf;
        if fx_on {
            if ch == 2 {
                self.master_fx.run(&plan.master_fx, f0 as f64 / r, out);
            } else {
                // R10a: the master FX per stereo pair.
                self.master_fx_n.resize_with(ch / 2, Chain::default);
                for (pair, chain) in self.master_fx_n.iter_mut().enumerate() {
                    run_pair(chain, &plan.master_fx, pair, f0 as f64 / r, out, ch, &mut self.fxbuf);
                }
            }
        }
        // (`round` was shadowed above: the lines used in this block carry the incremented round.)
        lines.retain(|_, l| l.used == self.round);
        (self.lines, self.scratch) = (lines, scratch);
        (self.track, self.pcm) = (track, pcm);
    }
}

/// The next block from frame `f`, wrapping to the loop start at the loop end. Returns its start frame and length.
pub fn next_block(f: i64, lp: Option<(i64, i64)>) -> (i64, usize) {
    match lp {
        Some((a, b)) => {
            let f = if f >= b { a } else { f };
            (f, (b - f).min(BLOCK as i64) as usize)
        }
        None => (f, BLOCK),
    }
}

/// Send `v`, waiting while the channel is full, unless `quit` is set or the receiver is gone (then false).
/// Never blocks for good, so a stopped session's thread always exits.
pub fn send_or_quit<T>(tx: &SyncSender<T>, mut v: T, quit: &AtomicBool) -> bool {
    loop {
        if quit.load(Relaxed) {
            return false;
        }
        match tx.try_send(v) {
            Ok(()) => return true,
            Err(TrySendError::Disconnected(_)) => return false,
            Err(TrySendError::Full(back)) => v = back,
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// R18a: Options › Metronome: a click on every beat at the project tempo (Project Properties › Ruler), accented on the
/// first beat of each measure, added to playback (and recording) only, as VEGAS's metronome is never rendered.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metronome {
    /// Seconds per beat and beats per measure.
    pub beat: f64,
    pub per_measure: u32,
}

impl Metronome {
    /// A click's length in frames (20 ms).
    const LEN: i64 = RATE as i64 / 50;

    pub fn of(s: &crate::project::ProjectSettings) -> Self {
        Self { beat: s.beat_secs(), per_measure: s.ruler.beats_per_measure.max(1) }
    }

    /// The timeline frame beat `k` falls on.
    pub fn frame(&self, k: i64) -> i64 {
        (k as f64 * self.beat * RATE as f64).round() as i64
    }

    /// Adds the clicks sounding in the stereo block `out` that starts at timeline frame `f0`: a decaying sine burst,
    /// 1.5 kHz on a measure's first beat, 1 kHz on the others.
    // shortcut: a synthesized click; VEGAS's Preferences › Audio metronome sound files and level are not offered.
    #[cfg(test)]
    pub fn add(&self, f0: i64, out: &mut [f32]) {
        self.add_to(f0, out, 2);
    }

    /// R10a: the clicks on the front pair of a block of `ch` interleaved channels.
    pub fn add_to(&self, f0: i64, out: &mut [f32], ch: usize) {
        let n = (out.len() / ch) as i64;
        let per = self.beat * RATE as f64;
        if per < 1.0 {
            return;
        }
        let mut k = (((f0 - Self::LEN) as f64 / per).ceil() as i64).max(0);
        while self.frame(k) < f0 + n {
            let at = self.frame(k);
            let (hz, amp) = if k % self.per_measure as i64 == 0 { (1500.0, 0.5) } else { (1000.0, 0.35) };
            for f in at.max(f0)..(at + Self::LEN).min(f0 + n) {
                let s = (f - at) as f32;
                let v = amp * (std::f32::consts::TAU * hz * s / RATE as f32).sin() * (1.0 - s / Self::LEN as f32);
                let i = (f - f0) as usize * ch;
                out[i] += v;
                out[i + 1] += v;
            }
            k += 1;
        }
    }
}

/// A mixed block for the device: its timeline start frame, the timeline frames each of its frames moves on (the play
/// rate), and the samples.
pub type Block = (i64, f64, Vec<f32>);

/// The "milan-mix" thread: blocks from frame `from`, each mixed from the newest plan in `slot` (so edits are heard
/// within the channel's depth) and sent with its start frame, looping over `lp` (frames). It never ends by itself:
/// past the content end it plays silence until the app stops it there, so an edit that extends the content is heard.
/// A `rate` other than 1 at the start is a shuttle (`shuttle`), which reads `rate` again each block.
pub fn run(slot: &Mutex<Arc<AudioPlan>>, mut mixer: Mixer, from: i64, lp: Option<(i64, i64)>, rate: &AtomicU64, tx: &SyncSender<Block>, quit: &AtomicBool, mut on_error: impl FnMut(String)) {
    if f64::from_bits(rate.load(Relaxed)) != 1.0 {
        return shuttle(slot, mixer, from, rate, tx, quit, on_error);
    }
    let ahead = RATE as i64; // streams open 1 s before they are heard
    let lp_end = lp.map_or(i64::MAX, |l| l.1);
    // The first block's decoders all start now, in parallel.
    mixer.warm(&slot.lock().unwrap().clone(), from, (from + ahead).min(lp_end));
    let mut f = from;
    let mut reset = true;
    // A stopped session opens no more decoders.
    while !quit.load(Relaxed) {
        let plan = slot.lock().unwrap().clone();
        let (f0, n) = next_block(f, lp);
        // R10a: mixed at the layout's width, metered, then folded down for the device.
        let ch = plan.layout.channels();
        let mut buf = vec![0.0; n * ch];
        mixer.block(&plan, f0, &mut buf);
        if let Some(m) = &plan.metronome {
            m.add_to(f0, &mut buf, ch);
        }
        if ch > 2 {
            if std::mem::take(&mut reset) {
                if let Some(ring) = &plan.loud {
                    ring.push_frames(&vec![f32::NAN; ch], ch); // a new session: the loudness meters start again
                }
            }
            mixer.tap_master(&plan, &buf, ch);
            let mut st = Vec::with_capacity(n * 2);
            surround::downmix(&buf, ch, &mut st);
            buf = st;
        }
        // Later decoders that are not ready play silence instead of holding the device queue.
        mixer.wait = false;
        f = f0 + n as i64;
        if quit.load(Relaxed) {
            break;
        }
        mixer.warm(&plan, f, (f + ahead).min(lp_end));
        if let Some((a, b)) = lp.filter(|&(_, b)| b - f < ahead) {
            mixer.warm(&plan, a, a + ahead - (b - f));
        }
        if let Some(e) = mixer.error.take() {
            on_error(e);
        }
        if !send_or_quit(tx, (f0, 1.0, buf), quit) {
            break;
        }
    }
}

/// R4b: J / L shuttle and the Rate slider. The timeline is mixed at 1x and played at `rate` (read again each block;
/// below 0 backwards), varispeed, so the pitch follows the speed as a tape's does (VEGAS's shuttle). Backwards, the
/// timeline is mixed a chunk at a time before what was heard, each chunk's decoders opened afresh. Playback only: no
/// loop, and it ends backwards at 0.
fn shuttle(slot: &Mutex<Arc<AudioPlan>>, mut mixer: Mixer, from: i64, rate: &AtomicU64, tx: &SyncSender<Block>, quit: &AtomicBool, mut on_error: impl FnMut(String)) {
    let mut rs = Resampler::new(slot.lock().unwrap().taps);
    // Mixed timeline frames lo .. lo + held.len() / 2.
    let (mut lo, mut held) = (from, Vec::<f32>::new());
    let mut t = from as f64;
    while !quit.load(Relaxed) && t >= 0.0 {
        let r = f64::from_bits(rate.load(Relaxed));
        let plan = slot.lock().unwrap().clone();
        let end = t + BLOCK as f64 * r;
        let reach = rs.reach(r) as i64;
        let (need_lo, need_hi) = (t.min(end).floor() as i64 - reach, t.max(end).ceil() as i64 + reach + 1);
        let hi = lo + (held.len() / 2) as i64;
        if held.is_empty() || need_hi <= lo || need_lo >= hi {
            // Nothing held is of use (the start): mix from where it is needed, in the direction of play.
            held.clear();
            lo = if r >= 0.0 { need_lo } else { need_hi };
        }
        // Forwards: mix on after what is held (its decoders read on).
        while lo + ((held.len() / 2) as i64) < need_hi {
            let (f, at) = (lo + (held.len() / 2) as i64, held.len());
            held.resize(at + BLOCK * 2, 0.0);
            block_stereo(&mut mixer, &plan, f, &mut held[at..]);
            mixer.wait = false;
        }
        if r > 0.0 {
            // Decoders for what plays in the next second open now, as in normal playback.
            let hi = lo + (held.len() / 2) as i64;
            mixer.warm(&plan, hi, hi + (RATE as f64 * r.max(1.0)) as i64);
        }
        // Backwards: mix a chunk (a quarter second of play) before it, waiting for its new decoders.
        let chunk = ((r.abs() * RATE as f64 / 4.0) as usize / BLOCK + 1) * BLOCK;
        while lo > need_lo {
            let f = lo - chunk as i64;
            let mut c = vec![0.0; chunk * 2];
            mixer.wait = true;
            for (i, b) in c.chunks_mut(BLOCK * 2).enumerate() {
                block_stereo(&mut mixer, &plan, f + (i * BLOCK) as i64, b);
            }
            held.splice(0..0, c);
            lo = f;
        }
        let mut buf = vec![0.0; BLOCK * 2];
        if r.abs() >= MIN_SPEED {
            for (k, o) in buf.chunks_exact_mut(2).enumerate() {
                let [a, b] = rs.read(&held, t + k as f64 * r - lo as f64, r);
                (o[0], o[1]) = (a, b);
            }
        }
        // What play has passed is dropped.
        if r >= 0.0 {
            let d = ((need_lo - lo).max(0) as usize).min(held.len() / 2);
            held.drain(..d * 2);
            lo += d as i64;
        } else {
            held.truncate(((need_hi - lo).max(0) as usize * 2).min(held.len()));
        }
        if let Some(e) = mixer.error.take() {
            on_error(e);
        }
        if quit.load(Relaxed) || !send_or_quit(tx, (t.round() as i64, r, buf), quit) {
            break;
        }
        t = end;
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::engine::plan::{QUICKFADE, compile_audio};
    use crate::project::{Clip, Curve, EnvKind, Keyframe, Keyframes, MediaItem, MediaType, Out, PanType, Project};
    use std::sync::atomic::AtomicUsize;
    use std::time::Instant;

    /// A fake decoder: sample k of the event (from its start) is `value(k)` on both channels, sent in 300-frame chunks.
    fn fake(value: fn(i64) -> f32) -> Mixer {
        Mixer::new(Box::new(move |src: &ASrc, at| {
            let total = frame(src.pos + src.dur) - frame(src.pos);
            let (tx, rx) = mpsc::sync_channel(1 << 16);
            let pcm: Vec<f32> = (at..total).flat_map(|k| [value(k); 2]).collect();
            for c in pcm.chunks(600) {
                tx.send(c.to_vec()).unwrap();
            }
            Ok(Stream::from_rx(rx))
        }))
    }

    fn ones() -> Mixer {
        fake(|_| 1.0)
    }

    /// R34a (for `aplug::tests`): a mixer whose events play `value(k)`.
    pub fn fake_mixer(value: fn(i64) -> f32) -> Mixer {
        fake(value)
    }

    /// R34a (for `aplug::tests`): `frames` frames of `plan` from `f0`.
    pub fn mix_frames(plan: &AudioPlan, m: &mut Mixer, f0: i64, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0; frames * 2];
        m.block(plan, f0, &mut out);
        out
    }

    /// R34a (for `aplug::tests`): three audible tracks at unity, each with the 10 s event at 0, and the sawtooth every
    /// event plays (sample k of the event).
    pub fn two_tracks() -> (Project, fn(i64) -> f32) {
        let mut p = project();
        for t in &mut p.timeline.audio_tracks {
            (t.volume, t.pan, t.muted) = (1.0, 0.0, false);
        }
        (p, |k| (k % 97) as f32 / 97.0 - 0.5)
    }

    pub fn plan_latency(plan: &AudioPlan) -> usize {
        Lat::of(plan, true).total
    }

    /// R34a: without latent plug-ins the compensation is nothing at all (no delay lines, the same blocks as before);
    /// the delay line itself delays sample-exactly and restarts when its length changes.
    #[test]
    fn pdc_is_nothing_without_latency_and_lines_delay_exactly() {
        let p = project();
        let plan = compile_audio(&p);
        assert_eq!(Lat::of(&plan, true), Lat::default());
        let mut m = ones();
        m.block(&plan, 0, &mut [0.0; 8]);
        assert!(m.lines.is_empty() && m.last_end == Some(4));
        let mut lines = HashMap::new();
        let mut scratch = Vec::new();
        let src: Vec<f32> = (0..16).map(|k| k as f32).collect();
        let d1 = delayed(&mut lines, "a", 3, 1, &src, &mut scratch, 2).to_vec();
        assert_eq!(&d1[..8], [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        assert_eq!(&d1[6..], &src[..10]);
        let d2 = delayed(&mut lines, "a", 3, 2, &src, &mut scratch, 2).to_vec();
        assert_eq!(&d2[..6], &src[10..]);
        assert!(std::ptr::eq(delayed(&mut lines, "a", 0, 3, &src, &mut scratch, 2), src.as_slice()));
        let d3 = delayed(&mut lines, "a", 1, 4, &src, &mut scratch, 2).to_vec();
        assert_eq!(&d3[..4], [0.0, 0.0, 0.0, 1.0], "a new length starts silent");
    }

    fn mix(p: &Project, m: &mut Mixer, f0: i64, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0; frames * 2];
        m.block(&compile_audio(p), f0, &mut out);
        out
    }

    /// Two audio tracks with a 10 s event at 0 each (plus the default track, muted): track 0 at volume 0.5, track 1 panned hard right.
    fn project() -> Project {
        let mut p = Project::new("t");
        let m = MediaItem {
            id: "a".into(),
            path: "a.wav".into(),
            name: "a".into(),
            media_type: MediaType::Audio,
            duration: 10.0,
            sample_rate: Some(48_000),
            ..Default::default()
        };
        let tl = &mut p.timeline;
        tl.add_audio_track();
        tl.add_audio_track();
        for t in &mut tl.audio_tracks {
            t.clips.push(Clip::from_media(&m, 0.0));
        }
        tl.audio_tracks[0].volume = 0.5;
        tl.audio_tracks[1].pan = 1.0;
        tl.audio_tracks[2].muted = true;
        p.media_pool.push(m);
        p
    }

    /// Only track 0, at unity.
    fn solo_track() -> Project {
        let mut p = project();
        p.timeline.audio_tracks[1].muted = true;
        p.timeline.audio_tracks[0].volume = 1.0;
        p
    }

    fn left(out: &[f32]) -> Vec<f32> {
        out.iter().step_by(2).map(|v| (v * 1e4).round() / 1e4).collect()
    }

    #[test]
    fn mix_mute_solo_volume_pan() {
        let mut p = project();
        // An empty master chain and the neutral default track chains: the exact batch 3 values.
        assert!(p.timeline.master.audio_fx.is_empty() && p.timeline.audio_tracks.iter().all(|t| t.fx.len() == 3));
        // Track 0: 0.5 both sides; track 1: hard right at 1.0; track 2 muted.
        assert_eq!(mix(&p, &mut ones(), 0, 4), [0.5, 1.5, 0.5, 1.5, 0.5, 1.5, 0.5, 1.5]);
        p.timeline.audio_tracks[0].solo = true;
        assert_eq!(mix(&p, &mut ones(), 0, 2), [0.5, 0.5, 0.5, 0.5]);
        p.timeline.audio_tracks[0].clips[0].muted = true; // event mute
        assert_eq!(mix(&p, &mut ones(), 0, 2), [0.0; 4]);
    }

    #[test]
    fn mix_respects_clip_position_and_source_offset() {
        let mut p = solo_track();
        p.timeline.audio_tracks[0].volume = 0.5;
        let c = &mut p.timeline.audio_tracks[0].clips[0];
        (c.position, c.duration, c.in_point) = (2.0 / RATE as f64, 2.0 / RATE as f64, 3.0);
        let asked = Arc::new(Mutex::new(Vec::new()));
        let opener = |asked: Arc<Mutex<Vec<(f64, i64)>>>| {
            Mixer::new(Box::new(move |src: &ASrc, at| {
                asked.lock().unwrap().push((src.in_point, at));
                let (tx, rx) = mpsc::sync_channel(1);
                tx.send(vec![1.0; 64]).unwrap();
                Ok(Stream::from_rx(rx))
            }))
        };
        let out = mix(&p, &mut opener(asked.clone()), 0, 6);
        assert_eq!(out, [0.0, 0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 0.5, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(*asked.lock().unwrap(), [(3.0, 0)]);
        // A block starting mid-event opens the stream that far in.
        asked.lock().unwrap().clear();
        mix(&p, &mut opener(asked.clone()), 3, 4);
        assert_eq!(*asked.lock().unwrap(), [(3.0, 1)]);
    }

    /// R3a: an 8 s clip extended to 20 s plays source 0..8, 0..8, 0..4, each pass from a stream of its own, and a block
    /// across a wrap reads both sides. The fake decoder plays its source time as the sample value.
    #[test]
    fn a_looping_event_restarts_its_media_at_each_pass() {
        let mut p = solo_track();
        p.media_pool[0].duration = 8.0;
        p.timeline.audio_tracks[0].clips[0].duration = 20.0;
        let opened = Arc::new(Mutex::new(Vec::new()));
        let log = opened.clone();
        let mut m = Mixer::new(Box::new(move |src: &ASrc, at| {
            log.lock().unwrap().push((src.pos, src.in_point, at));
            let total = frame(src.pos + src.dur) - frame(src.pos);
            let (tx, rx) = mpsc::sync_channel(1 << 16);
            let pcm: Vec<f32> = (at..total).flat_map(|k| [(src.in_point + k as f64 / RATE as f64) as f32; 2]).collect();
            for c in pcm.chunks(4800) {
                tx.send(c.to_vec()).unwrap();
            }
            Ok(Stream::from_rx(rx))
        }));
        let at = |m: &mut Mixer, t: f64| mix(&p, m, frame(t), 1)[0];
        for (t, src) in [(1.0, 1.0), (7.5, 7.5), (8.5, 0.5), (15.0, 7.0), (16.25, 0.25), (19.5, 3.5)] {
            let got = at(&mut m, t);
            assert!((got as f64 - src).abs() < 1e-4, "{t} s plays source {got}, not {src}");
        }
        // One block across the wrap at 8 s: the end of the first pass, then the start of the second.
        let out = left(&mix(&p, &mut m, frame(8.0) - 2, 4));
        let want = [8.0 - 2.0 / RATE as f64, 8.0 - 1.0 / RATE as f64, 0.0, 1.0 / RATE as f64].map(|v| (v as f32 * 1e4).round() / 1e4);
        assert_eq!(out, want);
        let opened = opened.lock().unwrap();
        assert!(opened.iter().all(|&(pos, inp, _)| inp == 0.0 && [0.0, 8.0, 16.0].contains(&pos)), "{opened:?}");
    }

    /// A fake decoder that plays `f(media seconds)` on both channels (from its in point).
    fn media(f: fn(f64) -> f32) -> Mixer {
        Mixer::new(Box::new(move |src: &ASrc, at| {
            let total = frame(src.pos + src.dur) - frame(src.pos);
            let (tx, rx) = mpsc::sync_channel(1 << 16);
            let pcm: Vec<f32> = (at..total).flat_map(|k| [f(src.in_point + k as f64 / RATE as f64); 2]).collect();
            for c in pcm.chunks(4800) {
                tx.send(c.to_vec()).unwrap();
            }
            Ok(Stream::from_rx(rx))
        }))
    }

    fn sine_1k(s: f64) -> f32 {
        (2.0 * std::f64::consts::PI * 1000.0 * s).sin() as f32
    }

    /// Sign changes of the left channel per second.
    fn crossings_hz(out: &[f32]) -> f64 {
        let l: Vec<f32> = out.iter().step_by(2).copied().collect();
        let n = l.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count();
        n as f64 / 2.0 / (l.len() as f64 / RATE as f64)
    }

    /// R4b: an audio event with a Velocity envelope plays varispeed: a 1 kHz sine at 200% measures 2 kHz; at -100% it
    /// runs backwards; a freeze is silent; past the media without Loop it is silent.
    #[test]
    fn velocity_plays_audio_varispeed() {
        let vel = |keys: &[(f64, f32, Curve)]| crate::project::Envelope { kind: EnvKind::Velocity, points: Keyframes { keys: keys.iter().map(|&(t, v, curve)| Keyframe { t, v, curve }).collect() } };
        let mut p = solo_track();
        let c = &mut p.timeline.audio_tracks[0].clips[0];
        c.duration = 1.0;
        c.envelopes.push(vel(&[(0.0, 2.0, Curve::Linear)]));
        assert!(compile_audio(&p).tracks[0].events[0].vel.is_some());
        let out = mix(&p, &mut media(sine_1k), 0, RATE as usize);
        let hz = crossings_hz(&out[2000..2 * 47_000]);
        assert!((hz - 2000.0).abs() < 20.0, "{hz} Hz");
        // Played in blocks it is the same sound (preview and render agree).
        let mut m = media(sine_1k);
        let blocks: Vec<f32> = (0..8).flat_map(|i| mix(&p, &mut m, i * BLOCK as i64, BLOCK)).collect();
        assert!(blocks.iter().zip(&out).all(|(a, b)| (a - b).abs() < 1e-6));
        // -100% from 0.5 s: media 0.5 - t.
        let c = &mut p.timeline.audio_tracks[0].clips[0];
        (c.in_point, c.looping, c.envelopes) = (0.5, false, vec![vel(&[(0.0, -1.0, Curve::Linear)])]);
        let out = left(&mix(&p, &mut media(|s| s as f32), 0, frame(0.8) as usize));
        assert!((out[frame(0.25) as usize] - 0.25).abs() < 1e-3, "{}", out[frame(0.25) as usize]);
        assert!(out[frame(0.6) as usize] == 0.0, "before the media start is silent");
        // A freeze (0% from 0.5 s) is silent there.
        p.timeline.audio_tracks[0].clips[0].envelopes = vec![vel(&[(0.0, 1.0, Curve::Hold), (0.5, 0.0, Curve::Hold)])];
        let out = left(&mix(&p, &mut media(|s| s as f32 + 1.0), 0, frame(0.8) as usize));
        assert!((out[frame(0.25) as usize] - 1.75).abs() < 1e-3 && out[frame(0.7) as usize] == 0.0);
    }

    /// R4b: the shuttle plays the timeline at its rate: 2x reads two timeline seconds a second, -1x runs backwards, and
    /// each block carries its rate (the device clock's).
    #[test]
    fn shuttle_plays_the_timeline_at_its_rate() {
        let p = solo_track();
        let run_at = |from: f64, rate: f64, blocks: usize| {
            let slot = Mutex::new(Arc::new(compile_audio(&p)));
            let (tx, rx) = mpsc::sync_channel(4);
            let (quit, r) = (AtomicBool::new(false), AtomicU64::new(rate.to_bits()));
            std::thread::scope(|s| {
                let (slot, quit, r) = (&slot, &quit, &r);
                s.spawn(move || run(slot, media(|s| s as f32 / 10.0), frame(from), None, r, &tx, quit, |e| panic!("{e}")));
                let got: Vec<Block> = rx.iter().take(blocks).collect();
                quit.store(true, Relaxed);
                got
            })
        };
        let b = run_at(1.0, 2.0, 3);
        assert_eq!((b[0].0, b[0].1, b[1].0), (frame(1.0), 2.0, frame(1.0) + 2 * BLOCK as i64));
        for (k, v) in b[1].2.iter().step_by(2).enumerate() {
            let want = (1.0 + (2 * BLOCK + 2 * k) as f64 / RATE as f64) / 10.0;
            assert!((*v as f64 - want).abs() < 1e-4, "{k}: {v} vs {want}");
        }
        let b = run_at(2.0, -1.0, 12);
        let v = b[10].2[200];
        let want = (2.0 - (10 * BLOCK + 100) as f64 / RATE as f64) / 10.0;
        assert!((v as f64 - want).abs() < 1e-4, "{v} vs {want}");
    }

    /// A decoder whose left channel is 0.25 and right 0.75.
    fn stereo() -> Mixer {
        Mixer::new(Box::new(|src: &ASrc, at| {
            let total = frame(src.pos + src.dur) - frame(src.pos);
            let (tx, rx) = mpsc::sync_channel(1 << 16);
            let pcm: Vec<f32> = (at..total).flat_map(|_| [0.25, 0.75]).collect();
            for c in pcm.chunks(600) {
                tx.send(c.to_vec()).unwrap();
            }
            Ok(Stream::from_rx(rx))
        }))
    }

    /// R3b: Invert Phase gives -x (event or track); Left Only puts L on both sides, Right Only R, Combine their mean,
    /// Swap swaps them.
    #[test]
    fn invert_phase_and_channels() {
        use crate::project::ChannelMap::*;
        let mut p = solo_track();
        assert_eq!(mix(&p, &mut stereo(), 0, 1), [0.25, 0.75]);
        for (ch, want) in [(LeftOnly, [0.25, 0.25]), (RightOnly, [0.75, 0.75]), (Combine, [0.5, 0.5]), (Swap, [0.75, 0.25])] {
            p.timeline.audio_tracks[0].clips[0].channels = ch;
            assert_eq!(mix(&p, &mut stereo(), 0, 1), want, "{ch:?}");
        }
        p.timeline.audio_tracks[0].clips[0].invert = true;
        assert_eq!(mix(&p, &mut stereo(), 0, 1), [-0.75, -0.25]);
        let c = &mut p.timeline.audio_tracks[0].clips[0];
        (c.invert, c.channels) = (false, Both);
        p.timeline.audio_tracks[0].invert = true;
        assert_eq!(mix(&p, &mut stereo(), 0, 1), [-0.25, -0.75]);
        // The decoder reads the chosen stream and keeps it apart from another stream of the same file.
        let mut q = solo_track();
        (q.media_pool[0].audio_streams, q.timeline.audio_tracks[0].clips[0].stream) = (2, 1);
        assert_eq!(compile_audio(&q).tracks[0].events[0].src.stream, 1);
        q.timeline.audio_tracks[0].clips[0].stream = 5;
        assert_eq!(compile_audio(&q).tracks[0].events[0].src.stream, 1, "a stream the file lacks plays its last");
    }

    /// R3b: Normalize to -0.1 dBFS scales a 0.5 peak to 0.9886, on top of the event gain.
    #[test]
    fn normalize_scales_the_peak_to_the_normalize_level() {
        let mut p = solo_track();
        let c = &mut p.timeline.audio_tracks[0].clips[0];
        c.normalize = true;
        c.normalized = Some(crate::project::Normalized { db: crate::edit::normalize_db(0.5), span: crate::edit::norm_span(c) });
        let out = mix(&p, &mut fake(|_| 0.5), 0, 2);
        assert!(out.iter().all(|v| (v - 0.9886).abs() < 1e-4), "{out:?}");
        p.timeline.audio_tracks[0].clips[0].normalize = false; // the gain stays stored but is not applied
        assert_eq!(mix(&p, &mut fake(|_| 0.5), 0, 1), [0.5, 0.5]);
    }

    /// R3b: Quickfade Audio Edits fades an edge without a fade over 10 ms, linearly; an edge with its own fade is left alone.
    #[test]
    fn quickfade_softens_bare_edges() {
        let mut p = solo_track();
        p.timeline.audio_tracks[0].clips[0].duration = 1.0;
        let q = frame(QUICKFADE);
        let mut plan = compile_audio(&p);
        plan.quickfade = QUICKFADE;
        let mut out = vec![0.0; 2 * frame(1.0) as usize];
        ones().block(&plan, 0, &mut out);
        let l = left(&out);
        assert_eq!((l[0], l[q as usize / 2], l[q as usize], l[frame(0.5) as usize]), (0.0, 0.5, 1.0, 1.0));
        assert!(l[l.len() - 1] < 0.01 && l[l.len() - 1 - q as usize / 2] > 0.45);
        // A fade-in of its own: no quickfade at that edge.
        p.timeline.audio_tracks[0].clips[0].fade_in = 0.5;
        let mut plan = compile_audio(&p);
        plan.quickfade = QUICKFADE;
        let mut with = vec![0.0; 2 * frame(1.0) as usize];
        ones().block(&plan, 0, &mut with);
        let mut plain = vec![0.0; 2 * frame(1.0) as usize];
        ones().block(&compile_audio(&p), 0, &mut plain);
        assert_eq!(with[..2 * frame(0.5) as usize], plain[..2 * frame(0.5) as usize]);
    }

    #[test]
    fn mix_applies_clip_fades_with_their_curves() {
        let mut p = solo_track();
        p.timeline.audio_tracks[0].volume = 0.5;
        let c = &mut p.timeline.audio_tracks[0].clips[0];
        (c.duration, c.fade_in, c.fade_out) = (8.0 / RATE as f64, 2.0 / RATE as f64, 2.0 / RATE as f64);
        // Gain 0.5 times the S-curve: 0 and 0.5 at the fade-in, full in the middle, 0.5 again at the fade-out start.
        assert_eq!(left(&mix(&p, &mut ones(), 0, 8)), [0.0, 0.25, 0.5, 0.5, 0.5, 0.5, 0.5, 0.25]);

        let mut p = solo_track();
        let c = &mut p.timeline.audio_tracks[0].clips[0];
        c.fade_in = 4.0 / RATE as f64;
        assert_eq!(left(&mix(&p, &mut ones(), 0, 5)), [0.0, 0.1464, 0.5, 0.8536, 1.0]); // Smooth
        p.timeline.audio_tracks[0].clips[0].fade_in_curve = Curve::Linear;
        assert_eq!(left(&mix(&p, &mut ones(), 0, 5)), [0.0, 0.25, 0.5, 0.75, 1.0]);
    }

    #[test]
    fn event_gain_minus_6_db_halves_the_amplitude() {
        let mut p = solo_track();
        p.timeline.audio_tracks[0].clips[0].gain_db = -6.0;
        let out = mix(&p, &mut ones(), 0, 4);
        assert!(out.iter().all(|v| (v - 0.5).abs() < 2e-3), "{out:?}");
    }

    #[test]
    fn volume_envelope_ramps_across_a_block() {
        let mut p = solo_track();
        let linear = |keys: &[(f64, f32)]| Keyframes { keys: keys.iter().map(|&(t, v)| Keyframe { t, v, curve: Curve::Linear }).collect() };
        let end = BLOCK as f64 / RATE as f64;
        p.timeline.audio_tracks[0].envelopes.push(crate::project::Envelope { kind: EnvKind::Volume, points: linear(&[(0.0, 0.0), (end, -6.0206)]) });
        let out = left(&mix(&p, &mut ones(), 0, BLOCK));
        assert_eq!(out[0], 1.0);
        assert!((out[BLOCK / 2] - 0.75).abs() < 1e-3, "{}", out[BLOCK / 2]); // ramped in gain, not stepped
        assert!((out[BLOCK - 1] - 0.5).abs() < 1e-3, "{}", out[BLOCK - 1]);
        assert!(out.windows(2).all(|w| w[1] < w[0]));
        // A mute envelope at 0 silences the track; an event envelope works the same way, event-relative.
        let mut q = solo_track();
        q.timeline.audio_tracks[0].envelopes.push(crate::project::Envelope { kind: EnvKind::Mute, points: linear(&[(0.0, 0.0)]) });
        assert_eq!(mix(&q, &mut ones(), 0, 4), [0.0; 8]);
        let mut q = solo_track();
        let c = &mut q.timeline.audio_tracks[0].clips[0];
        c.position = 1.0;
        c.envelopes.push(crate::project::Envelope { kind: EnvKind::Volume, points: linear(&[(0.0, -6.0206)]) });
        assert!((mix(&q, &mut ones(), frame(1.0), 4)[0] - 0.5).abs() < 1e-3);
        // A pan envelope hard left on top of centre Pan.
        let mut q = solo_track();
        q.timeline.audio_tracks[0].envelopes.push(crate::project::Envelope { kind: EnvKind::Pan, points: linear(&[(0.0, -1.0)]) });
        assert_eq!(mix(&q, &mut ones(), 0, 2), [1.0, 0.0, 1.0, 0.0]);
        // Pan trims the envelope: hard left plus an envelope hard right is centre at full level.
        q.timeline.audio_tracks[0].pan = -1.0;
        q.timeline.audio_tracks[0].envelopes[0].points = linear(&[(0.0, 1.0)]);
        assert_eq!(mix(&q, &mut ones(), 0, 2), [1.0; 4]);
        q.timeline.audio_tracks[0].pan = 0.5;
        q.timeline.audio_tracks[0].envelopes[0].points = linear(&[(0.0, 0.5)]);
        assert_eq!(mix(&q, &mut ones(), 0, 2), [0.0, 1.0, 0.0, 1.0]);
    }

    #[test]
    fn two_blocks_equal_one_long_block() {
        let mut p = project();
        let c = &mut p.timeline.audio_tracks[0].clips[0];
        (c.position, c.fade_in, c.fade_out, c.duration) = (0.01, 0.02, 0.01, 0.045);
        c.fade_out_curve = Curve::Fast;
        p.timeline.audio_tracks[1].pan = 0.3;
        let ramp = |k: i64| (k as f32 * 0.37).sin();
        let mut one = vec![0.0; 2 * 2048];
        fake(ramp).block(&compile_audio(&p), 100, &mut one);
        let mut m = fake(ramp);
        let (a, b) = (mix(&p, &mut m, 100, 1024), mix(&p, &mut m, 1124, 1024));
        assert_eq!([a, b].concat(), one);
        // The second block read on from the first block's streams instead of opening new ones.
        assert_eq!(m.streams.len(), 2);
    }

    #[test]
    fn plan_swap_is_heard_on_the_next_block() {
        let mut p = solo_track();
        let mut m = ones();
        let slot = Mutex::new(Arc::new(compile_audio(&p)));
        let mut out = vec![0.0; 8];
        m.block(&slot.lock().unwrap().clone(), 0, &mut out);
        assert_eq!(out[0], 1.0);
        p.timeline.audio_tracks[0].volume = 0.25;
        *slot.lock().unwrap() = Arc::new(compile_audio(&p));
        // The next block ramps from the old volume (no zipper or click), and the one after it is at the new one.
        m.block(&slot.lock().unwrap().clone(), 4, &mut out);
        assert_eq!(left(&out), [1.0, 0.8125, 0.625, 0.4375]);
        m.block(&slot.lock().unwrap().clone(), 8, &mut out);
        assert_eq!(out, [0.25; 8]);
        // Pan and event gain glide the same way.
        p.timeline.audio_tracks[0].pan = 1.0;
        p.timeline.audio_tracks[0].clips[0].gain_db = -6.0206;
        m.block(&compile_audio(&p), 12, &mut out);
        assert_eq!(out[0..2], [0.25, 0.25]);
        assert!(out[6] < 0.25 && out[7] > out[6], "{out:?}");
    }

    use crate::engine::afx::test_fx as afx;

    #[test]
    fn event_fx_run_before_the_fade() {
        let mut p = solo_track();
        let c = &mut p.timeline.audio_tracks[0].clips[0];
        (c.fade_in, c.fade_in_curve) = (100.0 / RATE as f64, Curve::Linear);
        // A pure 1 ms (48-frame) delay: before the fade, frame 60 is the fade's 0.6; after it, the delayed 0.12.
        c.fx = vec![afx("milan.afx.delay", &[("delay", 1.0), ("feedback", 0.0), ("wet", 1.0), ("dry", 0.0)])];
        let out = left(&mix(&p, &mut ones(), 0, 200));
        assert_eq!((out[47], out[48], out[60], out[150]), (0.0, 0.48, 0.6, 1.0));
    }

    #[test]
    fn track_fx_tails_continue_and_event_fx_tails_are_cut() {
        let mut p = solo_track();
        p.timeline.audio_tracks[0].clips[0].duration = 0.1;
        let end = frame(0.1) as usize;
        let reverb = afx("milan.afx.reverb", &[("room", 0.8)]);
        p.timeline.audio_tracks[0].clips[0].fx = vec![reverb.clone()];
        let mut m = ones();
        let a = [mix(&p, &mut m, 0, end), mix(&p, &mut m, end as i64, end)].concat();
        assert!(a[..2 * end].iter().any(|&v| v != 1.0), "the event reverb is heard");
        assert!(a[2 * end..].iter().all(|&v| v == 0.0), "and stops with the event");
        p.timeline.audio_tracks[0].clips[0].fx.clear();
        p.timeline.audio_tracks[0].fx.push(reverb);
        let mut m = ones();
        let b = [mix(&p, &mut m, 0, end), mix(&p, &mut m, end as i64, end)].concat();
        assert!(b[2 * end..].iter().any(|&v| v.abs() > 1e-4), "the track reverb rings on");
    }

    /// R8c: an audio event's Pan and Mute envelopes; a track's envelopes are left out with its automation Off.
    #[test]
    fn event_pan_and_mute_envelopes_and_automation_off() {
        let linear = |keys: &[(f64, f32)]| Keyframes { keys: keys.iter().map(|&(t, v)| Keyframe { t, v, curve: Curve::Linear }).collect() };
        let env = |kind, keys: &[(f64, f32)]| crate::project::Envelope { kind, points: linear(keys) };
        let mut q = solo_track();
        q.timeline.audio_tracks[0].clips[0].envelopes = vec![env(EnvKind::Pan, &[(0.0, 1.0)])];
        assert_eq!(mix(&q, &mut ones(), 0, 2), [0.0, 1.0, 0.0, 1.0]);
        q.timeline.audio_tracks[0].clips[0].envelopes = vec![env(EnvKind::Mute, &[(0.0, 0.0)])];
        assert_eq!(mix(&q, &mut ones(), 0, 2), [0.0; 4]);
        let mut q = solo_track();
        q.timeline.audio_tracks[0].envelopes = vec![env(EnvKind::Mute, &[(0.0, 0.0)]), env(EnvKind::Volume, &[(0.0, -6.0)])];
        assert_eq!(mix(&q, &mut ones(), 0, 2), [0.0; 4]);
        q.timeline.audio_tracks[0].automation = crate::project::AutoMode::Off;
        assert_eq!(mix(&q, &mut ones(), 0, 2), [1.0; 4], "Off: the envelopes are ignored");
    }

    /// R8c acceptance: a Track Compressor threshold envelope from -10 to -30 dB makes the gain reduction grow over
    /// time; with the track's automation Off the compressor's own threshold plays throughout.
    #[test]
    fn a_threshold_envelope_changes_the_gain_reduction() {
        let mut p = solo_track();
        let blocks = 6;
        let end = (blocks * BLOCK) as f64 / RATE as f64;
        let mut comp = afx("milan.afx.compressor", &[("threshold", -10.0), ("ratio", 10.0), ("attack", 0.1), ("release", 1.0)]);
        let keys = vec![Keyframe { t: 0.0, v: -10.0, curve: Curve::Linear }, Keyframe { t: end, v: -30.0, curve: Curve::Linear }];
        comp.auto.insert("threshold".into(), Keyframes { keys });
        p.timeline.audio_tracks[0].fx = vec![comp];
        // Each block's level over its second half (the detector settled).
        let levels = |p: &Project| {
            let mut m = ones();
            (0..blocks).map(|b| mix(p, &mut m, (b * BLOCK) as i64, BLOCK)[BLOCK..].iter().map(|v| v.abs()).sum::<f32>() / BLOCK as f32).collect::<Vec<f32>>()
        };
        let auto = levels(&p);
        assert!(auto.windows(2).all(|w| w[1] < w[0]), "the reduction grows: {auto:?}");
        assert!(auto[blocks - 1] < 0.4 * auto[0], "{auto:?}");
        p.timeline.audio_tracks[0].automation = crate::project::AutoMode::Off;
        let off = levels(&p);
        assert!(off[1..].iter().all(|l| (l - off[1]).abs() < 1e-3 * off[1]) && (off[1] - auto[0]).abs() < 0.05 * off[1], "{off:?}");
    }

    /// Event delay, track reverb and a master limiter on both tracks of `project()`.
    fn with_fx() -> Project {
        let mut p = project();
        let d = afx("milan.afx.delay", &[("delay", 3.0), ("feedback", 0.4)]);
        for t in &mut p.timeline.audio_tracks {
            t.clips[0].fx = vec![d.clone()];
            t.fx.push(afx("milan.afx.reverb", &[]));
        }
        p.timeline.master.audio_fx = vec![afx("milan.afx.limiter", &[("input", 6.0)])];
        p
    }

    #[test]
    fn bypass_equals_a_plan_without_fx() {
        let p = with_fx();
        let mut plan = compile_audio(&p);
        plan.bypass_fx = true;
        let mut bypassed = vec![0.0; 2 * BLOCK];
        fake(|k| (k as f32 * 0.37).sin()).block(&plan, 0, &mut bypassed);
        let mut q = p.clone();
        q.timeline.master.audio_fx.clear();
        for t in &mut q.timeline.audio_tracks {
            t.fx.clear();
            t.clips[0].fx.clear();
        }
        let mut plain = vec![0.0; 2 * BLOCK];
        fake(|k| (k as f32 * 0.37).sin()).block(&compile_audio(&q), 0, &mut plain);
        assert_eq!(bypassed, plain);
        let mut on = vec![0.0; 2 * BLOCK];
        fake(|k| (k as f32 * 0.37).sin()).block(&compile_audio(&p), 0, &mut on);
        assert_ne!(on, plain);
    }

    #[test]
    fn fx_blocks_are_deterministic_and_split_invariant() {
        let p = with_fx();
        let ramp = |k: i64| (k as f32 * 0.37).sin();
        let (mut one, mut two) = (vec![0.0; 2 * 2048], vec![0.0; 2 * 2048]);
        fake(ramp).block(&compile_audio(&p), 100, &mut one);
        fake(ramp).block(&compile_audio(&p), 100, &mut two);
        assert_eq!(one, two);
        let mut m = fake(ramp);
        assert_eq!([mix(&p, &mut m, 100, 1024), mix(&p, &mut m, 1124, 1024)].concat(), one);
    }

    #[test]
    fn master_limiter_holds_the_ceiling_and_neutral_fx_change_nothing() {
        let mut p = project();
        let plain = mix(&p, &mut ones(), 0, BLOCK);
        p.timeline.master.audio_fx = vec![afx("milan.afx.eq", &[]), afx("milan.afx.compressor", &[])];
        assert_eq!(mix(&p, &mut ones(), 0, BLOCK), plain, "neutral master FX are bit-exact");
        p.timeline.master.audio_fx.push(afx("milan.afx.limiter", &[("input", 12.0), ("ceiling", -1.0)]));
        let ceil = 10f32.powf(-1.0 / 20.0);
        let out = mix(&p, &mut ones(), 0, 4 * BLOCK);
        assert!(out.iter().all(|v| v.abs() <= ceil), "{:?}", out.iter().fold(0.0f32, |m, v| m.max(v.abs())));
        assert!(out[2 * BLOCK..].iter().any(|v| v.abs() > 0.9 * ceil));
    }

    /// `solo_track()` with `n` busses.
    fn with_buses(n: usize) -> Project {
        let mut p = solo_track();
        crate::edit::set_bus_count(&mut p, n);
        p
    }

    /// R8a acceptance: Track → Bus A at -6 dB → Master halves the amplitude; the bus's pan and mute work as a track's,
    /// and its volume envelope multiplies its fader.
    #[test]
    fn a_track_through_a_bus_at_minus_6_db_is_halved() {
        let mut p = with_buses(1);
        let a = p.timeline.audio.buses[0].id.clone();
        let tid = p.timeline.audio_tracks[0].id.clone();
        assert!(crate::edit::set_track_out(&mut p, &[tid], &crate::project::Out::Bus(a.clone())));
        assert_eq!(mix(&p, &mut ones(), 0, 4), [1.0; 8], "a bus at 0 dB changes nothing");
        p.timeline.audio.buses[0].volume = crate::engine::keyframes::db_to_gain(-6.0206);
        let out = mix(&p, &mut ones(), 0, 4);
        assert!(out.iter().all(|v| (v - 0.5).abs() < 1e-4), "{out:?}");
        p.timeline.audio.buses[0].pan = 1.0;
        assert!((mix(&p, &mut ones(), 0, 1)[0]).abs() < 1e-6, "panned hard right");
        p.timeline.audio.buses[0].pan = 0.0;
        let env = crate::project::Envelope { kind: EnvKind::Volume, points: Keyframes { keys: vec![Keyframe { t: 0.0, v: -6.0206, curve: Curve::Linear }] } };
        p.timeline.audio.buses[0].envelopes.push(env);
        assert!(mix(&p, &mut ones(), 0, 2).iter().all(|v| (v - 0.25).abs() < 1e-4));
        p.timeline.audio.buses[0].muted = true;
        assert_eq!(mix(&p, &mut ones(), 0, 4), [0.0; 8], "a muted bus is silent");
        // An assignable FX chain is not an output: a track routed to one (a hand-edited project) goes to the master.
        let mut q = solo_track();
        let fx = crate::edit::insert_assignable_fx(&mut q, Vec::new()).unwrap();
        let tid = q.timeline.audio_tracks[0].id.clone();
        assert!(!crate::edit::set_track_out(&mut q, &[tid], &crate::project::Out::Bus(fx.clone())));
        q.timeline.audio_tracks[0].out = crate::project::Out::Bus(fx);
        assert_eq!(compile_audio(&q).tracks[0].out, None);
    }

    /// R8a acceptance: a send at 0 dB pre-volume feeds its bus with the track fader at -inf, and with the track muted
    /// (pre-volume sends are pre-mute); a post-volume send follows the fader. Sends are centre-panned.
    #[test]
    fn a_pre_volume_send_feeds_the_bus_with_the_fader_down() {
        let mut p = with_buses(1);
        let a = p.timeline.audio.buses[0].id.clone();
        p.timeline.audio.buses[0].pan = -1.0; // the bus hard left tells its signal from the track's
        let t = &mut p.timeline.audio_tracks[0];
        (t.volume, t.pan) = (0.0, 1.0);
        crate::edit::set_send(t, &a, 1.0, true);
        assert_eq!(mix(&p, &mut ones(), 0, 2), [1.0, 0.0, 1.0, 0.0]);
        p.timeline.audio_tracks[0].muted = true;
        assert_eq!(compile_audio(&p).tracks.len(), 1, "kept for its pre-volume send");
        assert_eq!(mix(&p, &mut ones(), 0, 2), [1.0, 0.0, 1.0, 0.0]);
        // Post-volume: silent with the fader down or the track muted; at half the fader, half.
        assert!(crate::edit::set_send_pre(&mut p.timeline.audio_tracks[0], &a, false));
        assert!(compile_audio(&p).tracks.is_empty(), "a muted track with only post sends is left out");
        p.timeline.audio_tracks[0].muted = false;
        assert_eq!(mix(&p, &mut ones(), 0, 2), [0.0; 4]);
        p.timeline.audio_tracks[0].volume = 0.5;
        assert_eq!(mix(&p, &mut ones(), 0, 1), [0.5, 0.5], "the track (hard right) and the post send (the bus is hard left)");
        // A send at -inf is no send.
        crate::edit::set_send(&mut p.timeline.audio_tracks[0], &a, 0.0, true);
        assert!(compile_audio(&p).tracks[0].sends.is_empty());
    }

    /// R8a: busses mix in order: Track → Bus B → Bus A → Master, and a track's FX send through an assignable FX chain
    /// routed to Bus A; every gain multiplies once.
    #[test]
    fn busses_mix_after_everything_that_feeds_them() {
        use crate::project::Out;
        let mut p = with_buses(2);
        let (a, b) = (p.timeline.audio.buses[0].id.clone(), p.timeline.audio.buses[1].id.clone());
        let fx = crate::edit::insert_assignable_fx(&mut p, Vec::new()).unwrap();
        assert_eq!(crate::edit::set_bus_out(&mut p, &b, &Out::Bus(a.clone())), Ok(true));
        assert_eq!(crate::edit::set_bus_out(&mut p, &fx, &Out::Bus(a.clone())), Ok(true));
        let tid = p.timeline.audio_tracks[0].id.clone();
        crate::edit::set_track_out(&mut p, &[tid], &Out::Bus(b.clone()));
        crate::edit::set_send(&mut p.timeline.audio_tracks[0], &fx, 0.25, false);
        let au = &mut p.timeline.audio;
        (au.buses[0].volume, au.buses[1].volume, au.fx[0].volume) = (0.5, 0.5, 0.5);
        let plan = compile_audio(&p);
        let at = |id: &str| plan.buses.iter().position(|x| x.id == id).unwrap();
        assert!(at(&b) < at(&a) && at(&fx) < at(&a) && plan.buses[at(&b)].out == Some(at(&a)) && plan.buses[at(&a)].out.is_none());
        // Track 1.0 → B 0.5 → A 0.25; send 0.25 → FX 0.125 → A 0.0625.
        let out = mix(&p, &mut ones(), 0, 2);
        assert!(out.iter().all(|v| (v - 0.3125).abs() < 1e-6), "{out:?}");
    }

    /// R8a acceptance: a loop Bus A → Bus B → Bus A is refused with a message; one in a hand-edited project is cut at
    /// compile, so it still plays.
    #[test]
    fn a_bus_loop_is_rejected() {
        use crate::project::Out;
        let mut p = with_buses(2);
        let (a, b) = (p.timeline.audio.buses[0].id.clone(), p.timeline.audio.buses[1].id.clone());
        assert_eq!(crate::edit::set_bus_out(&mut p, &a, &Out::Bus(b.clone())), Ok(true));
        let err = crate::edit::set_bus_out(&mut p, &b, &Out::Bus(a.clone())).unwrap_err();
        assert_eq!(err, "Bus B cannot be routed to Bus A: that would make a loop");
        assert!(crate::edit::set_bus_out(&mut p, &a, &Out::Bus(a.clone())).is_err(), "nor to itself");
        assert_eq!(p.timeline.audio.buses[1].out, Out::Master);
        p.timeline.audio.buses[1].out = Out::Bus(a.clone());
        let plan = compile_audio(&p);
        assert_eq!(plan.buses.iter().filter(|x| x.out.is_none()).count(), 1, "{:?}", plan.buses.iter().map(|x| x.out).collect::<Vec<_>>());
        assert!(plan.buses.iter().enumerate().all(|(k, x)| x.out.is_none_or(|o| o > k)));
        let tid = p.timeline.audio_tracks[0].id.clone();
        crate::edit::set_track_out(&mut p, &[tid], &Out::Bus(a));
        assert_eq!(mix(&p, &mut ones(), 0, 2), [1.0; 4]);
    }

    /// R8a: Pan Type in the mixer: Add Channels hard right puts the left channel on the right.
    #[test]
    fn pan_type_shapes_the_track_pan() {
        let mut p = solo_track();
        p.timeline.audio_tracks[0].pan = 1.0;
        assert_eq!(mix(&p, &mut stereo(), 0, 1), [0.0, 0.75]);
        p.timeline.audio_tracks[0].pan_type = crate::project::PanType::AddChannels;
        assert_eq!(mix(&p, &mut stereo(), 0, 1), [0.0, 1.0]);
        p.timeline.audio_tracks[0].pan = 0.0;
        p.timeline.audio_tracks[0].pan_type = crate::project::PanType::Balance6;
        let out = mix(&p, &mut stereo(), 0, 1);
        assert!((out[0] - 0.125).abs() < 1e-3 && (out[1] - 0.375).abs() < 1e-3, "{out:?}");
    }

    /// R8a: busses with FX, sends and envelopes mix the same in one block or two, and a playback session (the device's
    /// blocks) gives what a render (`block` over the range, as `render::mix_range`) does: the same mixer.
    #[test]
    fn busses_play_and_render_the_same() {
        let mut p = with_fx();
        crate::edit::set_bus_count(&mut p, 1);
        let a = p.timeline.audio.buses[0].id.clone();
        p.timeline.audio.buses[0].fx = vec![afx("milan.afx.reverb", &[("room", 0.6)])];
        p.timeline.audio.buses[0].pan = 0.4;
        let fx = crate::edit::insert_assignable_fx(&mut p, vec![afx("milan.afx.delay", &[("delay", 2.0)])]).unwrap();
        let ids: Vec<String> = p.timeline.audio_tracks.iter().map(|t| t.id.clone()).collect();
        crate::edit::set_track_out(&mut p, &ids[..1], &crate::project::Out::Bus(a.clone()));
        crate::edit::set_send(&mut p.timeline.audio_tracks[1], &fx, 0.5, false);
        crate::edit::set_send(&mut p.timeline.audio_tracks[1], &a, 0.3, true);
        let ramp = |k: i64| (k as f32 * 0.37).sin();
        let mut one = vec![0.0; 2 * 2048];
        fake(ramp).block(&compile_audio(&p), 100, &mut one);
        let mut m = fake(ramp);
        assert_eq!([mix(&p, &mut m, 100, 1024), mix(&p, &mut m, 1124, 1024)].concat(), one);
        // The session from frame 0, against block() from 0 on the same decoder.
        let slot = Mutex::new(Arc::new(compile_audio(&p)));
        let (tx, rx) = mpsc::sync_channel(4);
        let quit = AtomicBool::new(false);
        let played: Vec<f32> = std::thread::scope(|s| {
            let (slot, quit) = (&slot, &quit);
            s.spawn(move || run(slot, fake(ramp), 0, None, &AtomicU64::new(1f64.to_bits()), &tx, quit, |e| panic!("{e}")));
            let got: Vec<f32> = rx.iter().take(3).flat_map(|b| b.2).collect();
            quit.store(true, Relaxed);
            got
        });
        let mut rendered = vec![0.0; 2 * 3 * BLOCK];
        let mut m = fake(ramp);
        for (i, b) in rendered.chunks_mut(2 * BLOCK).enumerate() {
            m.block(&compile_audio(&p), (i * BLOCK) as i64, b);
        }
        assert_eq!(played, rendered);
        assert!(played.iter().any(|v| v.abs() > 1e-3));
    }

    /// R8a: meters hold each node's peak since it was read, and the RMS of its last 50 ms; a node that stops is zeroed.
    #[test]
    fn meters_take_peak_and_rms() {
        let mut p = with_buses(1);
        let a = p.timeline.audio.buses[0].id.clone();
        let tid = p.timeline.audio_tracks[0].id.clone();
        crate::edit::set_track_out(&mut p, std::slice::from_ref(&tid), &crate::project::Out::Bus(a.clone()));
        p.timeline.audio.buses[0].volume = 0.5;
        let meters: Arc<Meters> = Arc::new([(tid.clone(), Arc::default()), (a.clone(), Arc::default())].into_iter().collect());
        let mut plan = compile_audio(&p);
        plan.meters = Some(meters.clone());
        let mut m = fake(|k| if k % 2 == 0 { 0.8 } else { -0.8 });
        let mut out = vec![0.0; 2 * BLOCK];
        for f in 0..4 {
            m.block(&plan, f * BLOCK as i64, &mut out);
        }
        let near = |a: [f32; 2], b: f32| a.iter().all(|v| (v - b).abs() < 1e-4);
        assert!(near(meters[&tid].take_peaks(), 0.8) && near(meters[&tid].rms(), 0.8));
        assert!(near(meters[&a].take_peaks(), 0.4) && near(meters[&a].rms(), 0.4));
        assert!(near(meters[&tid].take_peaks(), 0.0), "reading resets the peak");
        // Past the event's end (10 s) the RMS falls to silence within 50 ms.
        for f in 0..3 {
            m.block(&plan, frame(11.0) + f * BLOCK as i64, &mut out);
        }
        assert!(near(meters[&tid].rms(), 0.0) && near(meters[&a].rms(), 0.0));
        // A meter nothing feeds any more (its track is muted: left out of the plan) is zeroed at once.
        meters[&a].rms[0].store(1f32.to_bits(), Relaxed);
        p.timeline.audio_tracks[0].muted = true;
        m.block(&AudioPlan { meters: Some(meters.clone()), ..compile_audio(&p) }, 0, &mut out);
        assert!(near(meters[&tid].rms(), 0.0) && near(meters[&a].rms(), 0.0));
        // The RMS window is 50 ms: a step from silence reads full only once the window has passed.
        let mut q = solo_track();
        q.timeline.audio_tracks[0].clips[0].position = BLOCK as f64 / RATE as f64;
        q.timeline.audio_tracks[0].id = tid.clone();
        let mt: Arc<Meters> = Arc::new([(tid.clone(), Arc::default())].into_iter().collect());
        let plan = AudioPlan { meters: Some(mt.clone()), ..compile_audio(&q) };
        let mut m = ones();
        m.block(&plan, 0, &mut out);
        m.block(&plan, BLOCK as i64, &mut out);
        let half = mt[&tid].rms()[0];
        assert!(half > 0.5 && half < 1.0, "{half}");
        for f in 2..5 {
            m.block(&plan, f * BLOCK as i64, &mut out);
        }
        assert!(near(mt[&tid].rms(), 1.0));
    }

    /// Runs a session on a fake decoder and returns its first `blocks` blocks.
    fn session(p: &Project, from: f64, lp: Option<(f64, f64)>, blocks: usize) -> Vec<Block> {
        let slot = Mutex::new(Arc::new(compile_audio(p)));
        let (tx, rx) = mpsc::sync_channel(4);
        let quit = AtomicBool::new(false);
        let lp = lp.map(|(a, b)| (frame(a), frame(b)));
        std::thread::scope(|s| {
            let (slot, quit) = (&slot, &quit);
            // The thread owns `tx`, so the receiver sees the end of a session that stops by itself.
            s.spawn(move || run(slot, ones(), frame(from), lp, &AtomicU64::new(1f64.to_bits()), &tx, quit, |e| panic!("{e}")));
            let got: Vec<_> = rx.iter().take(blocks).collect();
            quit.store(true, Relaxed);
            got
        })
    }

    #[test]
    fn session_loops_inside_the_mixer_and_plays_silence_past_the_end() {
        let mut p = solo_track();
        for t in &mut p.timeline.audio_tracks {
            t.clips[0].duration = 1.0; // content ends at 1 s
        }
        // Loop 0.5 .. 2 s: past the content end.
        let blocks = session(&p, 0.5, Some((0.5, 2.0)), 80);
        let (a, b) = (frame(0.5), frame(2.0));
        let mut f = a;
        let mut wrapped = false;
        for (f0, _, buf) in &blocks {
            if f == b {
                (f, wrapped) = (a, true); // t0 jumps from b back to a
            }
            assert_eq!(*f0, f);
            let n = buf.len() as i64 / 2;
            assert!(f + n <= b, "a block crosses the loop end");
            let sound = buf.iter().any(|&v| v != 0.0);
            assert_eq!(sound, f0 < &frame(1.0), "block at {f0}");
            f += n;
        }
        assert!(wrapped);
        // Without a loop it plays on in silence past the content end until it is stopped.
        let blocks = session(&p, 0.98, None, 100);
        assert_eq!(blocks.len(), 100);
        let mut f = frame(0.98);
        for (f0, _, buf) in &blocks {
            assert_eq!(*f0, f);
            assert_eq!(buf.iter().any(|&v| v != 0.0), f < frame(1.0), "block at {f0}");
            f += buf.len() as i64 / 2;
        }
    }

    /// `ones()` that counts the decoders it opens.
    fn counted() -> (Mixer, Arc<AtomicUsize>) {
        let opens = Arc::new(AtomicUsize::new(0));
        let (n, mut open) = (opens.clone(), ones().open);
        (Mixer::new(Box::new(move |s: &ASrc, at| {
            n.fetch_add(1, Relaxed);
            open(s, at)
        })), opens)
    }

    #[test]
    fn trims_while_playing_keep_the_decoder_and_a_move_reopens_it() {
        let mut p = solo_track();
        let (mut m, opens) = counted();
        mix(&p, &mut m, 0, BLOCK);
        // TrimLeft by a fraction of a frame's worth: the source under the playhead is the same.
        let c = &mut p.timeline.audio_tracks[0].clips[0];
        let d = 2.1 / RATE as f64;
        (c.position, c.in_point, c.duration) = (c.position + d, c.in_point + d, c.duration - d);
        assert_eq!(mix(&p, &mut m, BLOCK as i64, BLOCK), [1.0; 2 * BLOCK]);
        p.timeline.audio_tracks[0].clips[0].duration = 5.0; // TrimRight
        mix(&p, &mut m, 2 * BLOCK as i64, BLOCK);
        assert_eq!((opens.load(Relaxed), m.streams.len()), (1, 1));
        // A move changes the source under the playhead: a new decoder there.
        p.timeline.audio_tracks[0].clips[0].position += 0.5;
        mix(&p, &mut m, frame(0.6), BLOCK);
        assert_eq!(opens.load(Relaxed), 2);
    }

    /// R17b: in multicamera editing (standby plans) a multicamera event's other take decodes along, so a take switch
    /// heard after the cut (the mixer runs ahead) goes on from that decoder without opening one, crossfading from the
    /// take heard before over the quickfade instead of clicking.
    #[test]
    fn a_take_switch_while_playing_goes_on_from_the_standby_decoder() {
        let mut p = solo_track();
        let b = MediaItem { id: "b".into(), path: "b.wav".into(), name: "b".into(), ..p.media_pool[0].clone() };
        p.timeline.audio_tracks[0].clips[0].takes = vec![crate::project::Take { media_id: "b".into(), in_point: 0.0, name: "b".into() }];
        p.media_pool.push(b);
        let opens = Arc::new(AtomicUsize::new(0));
        let n = opens.clone();
        let mut m = Mixer::new(Box::new(move |src: &ASrc, at| {
            n.fetch_add(1, Relaxed);
            let v = if src.path == Path::new("b.wav") { 0.75 } else { 0.25 };
            let total = frame(src.pos + src.dur) - frame(src.pos);
            let (tx, rx) = mpsc::sync_channel(1 << 16);
            for c in vec![v; (total - at) as usize * 2].chunks(600) {
                tx.send(c.to_vec()).unwrap();
            }
            Ok(Stream::from_rx(rx))
        }));
        let plan = |p: &Project| AudioPlan { standby: true, quickfade: QUICKFADE, ..compile_audio(p) };
        let (mut out, q) = (vec![0.0; 2 * BLOCK], (QUICKFADE * RATE as f64) as usize);
        m.block(&plan(&p), 0, &mut out);
        // (Past the quickfade in at the event's start.)
        assert!(out[2 * q..].iter().all(|&v| v == 0.25) && opens.load(Relaxed) == 2, "take 1 heard, take 2 decoding along");
        // Take 2 chosen 100 frames in, after the mixer has passed there.
        assert!(crate::edit::switch_take(&mut p, false, 100.0 / RATE as f64, None, 1));
        m.block(&plan(&p), BLOCK as i64, &mut out);
        let l = left(&out);
        assert!((l[0] - 0.25).abs() < 1e-3 && (l[q / 2] - 0.5).abs() < 0.01 && l[q..].iter().all(|&v| v == 0.75), "{:?}", &l[..4]);
        assert!(l.windows(2).all(|w| w[1] >= w[0]), "a smooth crossfade");
        assert_eq!(opens.load(Relaxed), 2, "no decoder opened for the switch");
    }

    #[test]
    fn identical_events_on_two_tracks_are_warmed_a_decoder_each() {
        let p = project();
        let (mut m, opens) = counted();
        m.warm(&compile_audio(&p), 0, BLOCK as i64);
        assert_eq!(opens.load(Relaxed), 2);
        mix(&p, &mut m, 0, BLOCK);
        assert_eq!(opens.load(Relaxed), 2);
    }

    #[test]
    fn a_slow_decoder_plays_silence_then_joins_in_sync() {
        let (tx, rx) = mpsc::sync_channel(8);
        let mut s = Stream::from_rx(rx);
        let mut out = vec![1.0; 4];
        let t = Instant::now();
        assert_eq!(s.read(&mut out, false, None), None);
        assert!(t.elapsed() < Duration::from_millis(50), "waited for the decoder");
        assert_eq!(out, [0.0; 4]);
        // The 4 samples played as silence are dropped when they arrive.
        tx.send((0..12).map(|v| v as f32).collect()).unwrap();
        assert_eq!(s.read(&mut out, false, None), None);
        assert_eq!(out, [4.0, 5.0, 6.0, 7.0]);
    }

    #[test]
    fn a_render_waits_past_the_timeout_until_cancelled() {
        let (tx, rx) = mpsc::sync_channel(8);
        let mut s = Stream::from_rx(rx);
        let stop = AtomicBool::new(false);
        let mut out = vec![0.0; 4];
        std::thread::scope(|sc| {
            sc.spawn(move || {
                std::thread::sleep(TIMEOUT + Duration::from_millis(300));
                tx.send(vec![1.0; 4]).unwrap();
            });
            assert_eq!(s.read(&mut out, true, Some(&stop)), None);
        });
        assert_eq!(out, [1.0; 4]);
        // Cancelled while waiting for a decoder that never starts: silence, at once.
        let (_tx, rx) = mpsc::sync_channel(8);
        let mut s = Stream::from_rx(rx);
        stop.store(true, Relaxed);
        let t = Instant::now();
        assert_eq!(s.read(&mut out, true, Some(&stop)), None);
        assert!(t.elapsed() < Duration::from_millis(500));
        assert_eq!(out, [0.0; 4]);
    }

    #[test]
    fn atempo_chains_steps_within_range() {
        assert_eq!(atempo(0.25), ["atempo=0.5", "atempo=0.5"]);
        assert_eq!(atempo(0.5), ["atempo=0.5"]);
        assert_eq!(atempo(1.5), ["atempo=1.5"]);
        assert_eq!(atempo(4.0), ["atempo=2", "atempo=2"]);
        assert_eq!(atempo(3.0), ["atempo=2", "atempo=1.5"]);
        assert!(atempo(1.0).is_empty());
    }

    #[test]
    fn decoder_argv() {
        let src = ASrc { path: r"C:\a b.wav".into(), pos: 5.0, dur: 4.0, in_point: 2.0, rate: 1.0, reverse: false, stream: 0, pitch: 0.0, nested: None, pair: None, ch: 2 };
        assert_eq!(
            args(&src, frame(1.0), None).join(" "),
            r"-v error -ss 3.000000 -i C:\a b.wav -vn -map 0:a:0 -ac 2 -ar 48000 -f f32le -"
        );
        // Rate 2, reversed, 1 s in: the source span still to play is 2 .. 2 + 3 s x 2, read backwards at double speed.
        let a = args(&ASrc { rate: 2.0, reverse: true, ..src.clone() }, frame(1.0), None);
        assert_eq!(a[2..6], ["-ss", "2.000000", "-t", "6.000000"]);
        assert!(a.windows(2).any(|w| w == ["-af", "areverse,atempo=2"]));
        // R4b: an octave up is a resample to 96 kHz read as 48 kHz, put back to length (at 1.5x: atempo 0.75).
        let a = args(&ASrc { pitch: 1200.0, rate: 1.5, ..src.clone() }, 0, None);
        assert!(a.windows(2).any(|w| w == ["-af", "aresample=48000,asetrate=96000,aresample=48000,atempo=0.75"]), "{a:?}");
        // Mono goes to both sides at unity.
        assert_eq!(
            args(&src, 0, Some(1)).join(" "),
            r"-v error -ss 2.000000 -i C:\a b.wav -vn -map 0:a:0 -ar 48000 -af pan=stereo|c0=c0|c1=c0 -f f32le -"
        );
    }

    #[test]
    fn decoded_nan_and_infinity_are_silence() {
        let bytes: Vec<u8> = [0.5, f32::NAN, f32::INFINITY, -0.25].iter().flat_map(|v: &f32| v.to_le_bytes()).collect();
        assert_eq!(samples(&bytes), [0.5, 0.0, 0.0, -0.25]);
    }

    #[test]
    fn send_or_quit_never_blocks_for_good() {
        let (tx, _rx) = mpsc::sync_channel::<u8>(0); // always full: nobody is receiving
        let t = Instant::now();
        assert!(!send_or_quit(&tx, 1, &AtomicBool::new(true)));
        assert!(t.elapsed() < Duration::from_millis(50));
        // Quit set while it waits.
        let quit = AtomicBool::new(false);
        std::thread::scope(|s| {
            s.spawn(|| {
                std::thread::sleep(Duration::from_millis(20));
                quit.store(true, Relaxed);
            });
            assert!(!send_or_quit(&tx, 1, &quit));
        });
        let (tx, rx) = mpsc::sync_channel::<u8>(0);
        drop(rx);
        assert!(!send_or_quit(&tx, 1, &AtomicBool::new(false)));
    }

    /// Remove a test folder once the dropped decoders, reaped on their own threads, have let go of its files.
    fn remove_dir(dir: &Path) {
        let until = Instant::now() + Duration::from_secs(5);
        while let Err(e) = std::fs::remove_dir_all(dir) {
            assert!(Instant::now() < until, "{e}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Needs ffmpeg. `cargo test --release -- --ignored ffmpeg_stream_levels_and_damage`
    /// A mono file plays at unity on both sides (as recorded mono does), and a damaged MP3 that ffmpeg decodes with an
    /// error line but a clean exit plays its tail as silence instead of failing the render.
    #[test]
    #[ignore]
    fn ffmpeg_stream_levels_and_damage() {
        let dir = std::env::temp_dir().join(format!("milan-mixlvl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let make = |name: &str, src: &str, codec: &[&str]| {
            let path = dir.join(name);
            assert!(crate::proc::ffmpeg().args(["-v", "error", "-y", "-f", "lavfi", "-i", src]).args(codec).arg(&path).status().unwrap().success());
            path
        };
        let mono = make("mono.wav", "aevalsrc=0.5:s=48000:d=1", &["-c:a", "pcm_s16le"]);
        let src = |path: &Path, dur| ASrc { path: path.to_path_buf(), pos: 0.0, dur, in_point: 0.0, rate: 1.0, reverse: false, stream: 0, pitch: 0.0, nested: None, pair: None, ch: 2 };
        let mut s = spawn(&src(&mono, 1.0), 0).unwrap();
        let mut out = vec![0.0; 2 * 4800];
        assert_eq!(s.read(&mut out, true, None), None);
        assert!(out.iter().all(|v| (v - 0.5).abs() < 1e-3), "{} {}", out[0], out[1]);
        drop(s);
        // 3 s of MP3 with 400 bytes broken in the middle: ffprobe still says 3 s, ffmpeg decodes less.
        let mp3 = make("damaged.mp3", "sine=f=440:d=3:sample_rate=48000", &["-c:a", "libmp3lame", "-b:a", "128k"]);
        let mut bytes = std::fs::read(&mp3).unwrap();
        let mid = bytes.len() / 2;
        bytes[mid..mid + 400].iter_mut().enumerate().for_each(|(i, b)| *b = (i * 37 % 251) as u8);
        std::fs::write(&mp3, bytes).unwrap();
        let mut s = spawn(&src(&mp3, 3.0), 0).unwrap();
        let mut out = vec![0.0; 2 * 3 * 48_000];
        assert_eq!(s.read(&mut out, true, None), None, "a damaged frame is not a failed decode");
        drop(s);
        remove_dir(&dir);
    }

    /// Needs ffmpeg. `cargo test --release -- --ignored ffmpeg_second_stream_and_peak`
    /// R3b: a file with two audio streams (0.2 and 0.6 DC) plays stream 2 when it is chosen, and Normalize measures the
    /// chosen stream's peak through the event's channel map.
    #[test]
    #[ignore]
    fn ffmpeg_second_stream_and_peak() {
        let dir = std::env::temp_dir().join(format!("milan-mixstreams-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("two.mka");
        let ok = crate::proc::ffmpeg()
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", "aevalsrc=0.2|0.2:s=48000:d=1", "-f", "lavfi", "-i", "aevalsrc=0.6|0.3:s=48000:d=1"])
            .args(["-map", "0", "-map", "1", "-c:a", "pcm_f32le"])
            .arg(&path)
            .status()
            .unwrap()
            .success();
        assert!(ok);
        let m = crate::media::probe_media(&path).unwrap();
        assert_eq!(m.audio_streams, 2);
        let src = |stream| ASrc { path: path.clone(), pos: 0.0, dur: 1.0, in_point: 0.0, rate: 1.0, reverse: false, stream, pitch: 0.0, nested: None, pair: None, ch: 2 };
        for (stream, want) in [(0, [0.2, 0.2]), (1, [0.6, 0.3])] {
            let mut s = spawn(&src(stream), 0).unwrap();
            let mut out = vec![0.0; 2 * 4800];
            assert_eq!(s.read(&mut out, true, None), None);
            assert!((out[2000] - want[0]).abs() < 1e-3 && (out[2001] - want[1]).abs() < 1e-3, "stream {stream}: {} {}", out[2000], out[2001]);
            drop(s);
        }
        assert!((peak(&src(1), 1.0, ChannelMap::Both).unwrap() - 0.6).abs() < 1e-3);
        assert!((peak(&src(1), 1.0, ChannelMap::RightOnly).unwrap() - 0.3).abs() < 1e-3);
        assert!((peak(&src(0), 0.5, ChannelMap::Both).unwrap() - 0.2).abs() < 1e-3);
        remove_dir(&dir);
    }

    /// Needs ffmpeg. `cargo test --release -- --ignored ffmpeg_pitch_keeps_the_length`
    /// R4b: a 1 kHz sine pitched +12 semitones measures 2 kHz and lasts as long; -12 measures 500 Hz.
    #[test]
    #[ignore]
    fn ffmpeg_pitch_keeps_the_length() {
        let dir = std::env::temp_dir().join(format!("milan-mixpitch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sine.wav");
        let ok = crate::proc::ffmpeg()
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", "sine=f=1000:d=1:sample_rate=48000", "-c:a", "pcm_f32le"])
            .arg(&path)
            .status()
            .unwrap()
            .success();
        assert!(ok);
        for (pitch, want) in [(1200.0, 2000.0), (-1200.0, 500.0)] {
            let src = ASrc { path: path.clone(), pos: 0.0, dur: 1.0, in_point: 0.0, rate: 1.0, reverse: false, stream: 0, pitch, nested: None, pair: None, ch: 2 };
            let mut s = spawn(&src, 0).unwrap();
            let mut out = vec![0.0; 2 * 72_000];
            assert_eq!(s.read(&mut out, true, None), None);
            drop(s);
            let len = out.iter().rposition(|v| v.abs() > 1e-3).unwrap() / 2;
            assert!((len as f64 / RATE as f64 - 1.0).abs() < 0.03, "pitch {pitch}: lasts {len} frames");
            let hz = crossings_hz(&out[2 * 4800..2 * 43_200]);
            assert!((hz - want).abs() < want * 0.02, "pitch {pitch}: {hz} Hz");
        }
        remove_dir(&dir);
    }

    /// Needs ffmpeg. A real decoder: a reversed 2x event plays the source backwards in half the time.
    #[test]
    #[ignore]
    fn ffmpeg_stream_reverse_and_rate() {
        let dir = std::env::temp_dir().join(format!("milan-mix-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ramp.wav");
        // Left channel = time in seconds / 10, a ramp that shows direction and speed.
        let ok = crate::proc::ffmpeg()
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", "aevalsrc=t/10|t/10:s=48000:d=4", "-c:a", "pcm_f32le"])
            .arg(&path)
            .status()
            .unwrap()
            .success();
        assert!(ok);
        let src = ASrc { path: path.clone(), pos: 0.0, dur: 1.0, in_point: 1.0, rate: 2.0, reverse: true, stream: 0, pitch: 0.0, nested: None, pair: None, ch: 2 };
        let mut s = spawn(&src, 0).unwrap();
        let mut out = vec![0.0; 2 * 48_000];
        assert_eq!(s.read(&mut out, true, None), None);
        // Timeline 0 shows source 3 s, timeline 0.5 s shows source 2 s.
        assert!((out[0] - 0.3).abs() < 0.01, "{}", out[0]);
        assert!((out[48_000] - 0.2).abs() < 0.01, "{}", out[48_000]);
        drop(s);
        // Forward, 0.5 s into the event: source 1.5 s, read on past the event end (no -t).
        let mut s = spawn(&ASrc { rate: 1.0, reverse: false, ..src }, frame(0.5)).unwrap();
        assert_eq!(s.read(&mut out, true, None), None);
        assert!((out[0] - 0.15).abs() < 0.01, "{}", out[0]);
        assert!((out[2 * 47_999] - 0.25).abs() < 0.01, "{}", out[2 * 47_999]);
        drop(s);
        remove_dir(&dir);
    }

    /// R18a acceptance: at 120 BPM the metronome clicks on every beat, 24000 samples apart at 48 kHz, accented on the
    /// measure's first; mixing in blocks gives the same samples as one pass; a render plan has no metronome.
    /// R10a: a stereo project mixes bit for bit as it did before the multichannel engine. The hash is of the f32 bit
    /// patterns of the blocks the fake decoder mixes through busses, sends, FX, envelopes, fades and a pan type; it was
    /// taken on the engine as R8c-R34a left it, and must never change for a stereo project.
    #[test]
    fn stereo_mix_is_bit_identical_to_the_pre_r10a_engine() {
        let mut p = with_fx();
        crate::edit::set_bus_count(&mut p, 2);
        let a = p.timeline.audio.buses[0].id.clone();
        let b = p.timeline.audio.buses[1].id.clone();
        {
            let tl = &mut p.timeline;
            tl.audio.buses[0].volume = 0.5;
            tl.audio.buses[0].pan = 0.3;
            tl.audio.buses[1].out = Out::Bus(a.clone());
            let t0 = &mut tl.audio_tracks[0];
            (t0.out, t0.pan_type, t0.volume, t0.pan) = (Out::Bus(b.clone()), PanType::ConstantPower, 0.8, -0.4);
            crate::edit::set_send(t0, &a, 0.6, true);
            t0.envelopes.push(Envelope { kind: EnvKind::Volume, points: Keyframes { keys: vec![Keyframe { t: 0.0, v: 0.0, curve: Curve::Linear }, Keyframe { t: 1.0, v: -12.0, curve: Curve::Linear }] } });
            t0.envelopes.push(Envelope { kind: EnvKind::Pan, points: Keyframes { keys: vec![Keyframe { t: 0.0, v: -1.0, curve: Curve::Linear }, Keyframe { t: 1.0, v: 1.0, curve: Curve::Linear }] } });
            let t1 = &mut tl.audio_tracks[1];
            (t1.pan_type, t1.invert) = (PanType::AddChannels, true);
            let c = &mut t1.clips[0];
            (c.fade_in, c.fade_out, c.gain_db, c.channels) = (0.3, 0.2, -3.0, ChannelMap::Swap);
            c.envelopes.push(Envelope { kind: EnvKind::Pan, points: Keyframes { keys: vec![Keyframe { t: 0.0, v: 0.5, curve: Curve::Linear }] } });
        }
        let mut plan = compile_audio(&p);
        plan.quickfade = QUICKFADE;
        let mut m = fake(|k| ((k % 211) as f32 / 211.0 - 0.5) * (k as f32 * 0.013).sin());
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        let mut out = vec![0.0f32; 2 * BLOCK];
        for f0 in [0i64, BLOCK as i64, 2 * BLOCK as i64, 47_000, 48_024, 9 * 48_000 + 900] {
            m.block(&plan, f0, &mut out);
            for v in &out {
                for byte in v.to_bits().to_le_bytes() {
                    hash = (hash ^ byte as u64).wrapping_mul(0x0000_0100_0000_01b3);
                }
            }
        }
        assert_eq!(hash, GOLDEN_STEREO_HASH, "the stereo mix changed: {hash:#x}");
    }

    /// R10a: a decoder that plays sine `HZ[c]` on channel c of whatever width the event decodes at.
    const HZ: [f64; 8] = [300.0, 500.0, 700.0, 1100.0, 1300.0, 1700.0, 1900.0, 2300.0];

    fn sines() -> Mixer {
        Mixer::new(Box::new(|src: &ASrc, at| {
            let total = frame(src.pos + src.dur) - frame(src.pos);
            let (tx, rx) = mpsc::sync_channel(1 << 16);
            let w = src.ch.max(2) as usize;
            let pcm: Vec<f32> = (at..total).flat_map(|k| (0..w).map(move |c| (0.25 * (std::f64::consts::TAU * HZ[c] * k as f64 / RATE as f64).sin()) as f32)).collect();
            for c in pcm.chunks(600 * w) {
                tx.send(c.to_vec()).unwrap();
            }
            Ok(Stream::from_rx(rx))
        }))
    }

    /// Goertzel amplitude of `hz` on channel `c` of `buf` (frames `ch` wide).
    pub(crate) fn goertzel(buf: &[f32], ch: usize, c: usize, hz: f64) -> f64 {
        let n = buf.len() / ch;
        let w = std::f64::consts::TAU * hz / RATE as f64;
        let (mut s1, mut s2) = (0.0f64, 0.0f64);
        for f in buf.chunks_exact(ch) {
            let s0 = f[c] as f64 + 2.0 * w.cos() * s1 - s2;
            (s2, s1) = (s1, s0);
        }
        (s1 * s1 + s2 * s2 - 2.0 * w.cos() * s1 * s2).max(0.0).sqrt() * 2.0 / n as f64
    }

    /// One 5.1 / 7.1 track of the 10 s event at unity in a project of master bus mode `mode`.
    fn wide_track(mode: crate::project::BusMode, layout: crate::project::AudioLayout) -> Project {
        let mut p = solo_track();
        p.settings.layout = mode;
        p.timeline.audio_tracks[0].layout = layout;
        p
    }

    /// R10a acceptance: a 5.1 source on a 5.1 track in a 5.1 project plays each channel to its position (six sines,
    /// each found at 0.25 on its own channel and nowhere else); in a stereo project it is folded down by BS.775; in a
    /// 7.1 project the sides stay silent.
    #[test]
    fn a_5_1_source_plays_each_channel_to_its_position() {
        use crate::project::{AudioLayout, BusMode};
        let frames = 9600;
        let p = wide_track(BusMode::Surround51, AudioLayout::Surround51);
        let plan = compile_audio(&p);
        assert_eq!((plan.tracks[0].ch, plan.tracks[0].events[0].src.ch), (6, 6));
        let mut out = vec![0.0f32; frames * 6];
        sines().block(&plan, 0, &mut out);
        for c in 0..6 {
            for (k, hz) in HZ[..6].iter().enumerate() {
                let a = goertzel(&out, 6, c, *hz);
                let want = if k == c { 0.25 } else { 0.0 };
                assert!((a - want).abs() < 2e-3, "channel {c} at {hz} Hz: {a}");
            }
        }
        // Stereo master: L = FL + 0.707 C + 0.707 Ls (LFE dropped), as `surround::downmix_frame`.
        let p = wide_track(BusMode::Stereo, AudioLayout::Surround51);
        let mut st = vec![0.0f32; frames * 2];
        sines().block(&compile_audio(&p), 0, &mut st);
        let mut want = Vec::new();
        surround::downmix(&out, 6, &mut want);
        assert!(st.iter().zip(&want).all(|(a, b)| (a - b).abs() < 1e-6));
        // 7.1 master: channel for channel, the sides silent.
        let p = wide_track(BusMode::Surround71, AudioLayout::Surround51);
        let mut wide = vec![0.0f32; frames * 8];
        sines().block(&compile_audio(&p), 0, &mut wide);
        for f in 0..frames {
            assert_eq!(&wide[f * 8..f * 8 + 6], &out[f * 6..f * 6 + 6]);
            assert_eq!(&wide[f * 8 + 6..f * 8 + 8], &[0.0, 0.0]);
        }
    }

    /// R10a: a 5.1 track's Pan is a left-right balance (the centre and LFE keep their level), a mono track hears the
    /// mean of both sides on both, a stereo bus in a 5.1 project fills the front pair, and a stereo track in a 5.1
    /// project at the front-left pan point feeds the front-left speaker alone.
    #[test]
    fn track_and_bus_layouts_route_by_width() {
        use crate::project::{AudioLayout, BusMode, SurroundPan};
        let frames = 4800;
        let mut p = wide_track(BusMode::Surround51, AudioLayout::Surround51);
        p.timeline.audio_tracks[0].pan = 1.0;
        let mut out = vec![0.0f32; frames * 6];
        sines().block(&compile_audio(&p), 0, &mut out);
        let amp = |out: &[f32], ch, c| goertzel(out, ch, c, HZ[c]);
        assert!(amp(&out, 6, 0) < 1e-3 && amp(&out, 6, 4) < 1e-3, "the left side is silent at pan 1");
        for c in [1, 2, 3, 5] {
            assert!((amp(&out, 6, c) - 0.25).abs() < 2e-3, "channel {c}");
        }
        // Mono track: both sides carry (l + r) / 2.
        let mut p = wide_track(BusMode::Stereo, AudioLayout::Mono);
        p.timeline.audio_tracks[0].layout = AudioLayout::Mono;
        let mut st = vec![0.0f32; frames * 2];
        sines().block(&compile_audio(&p), 0, &mut st);
        assert!(st.chunks_exact(2).all(|f| f[0] == f[1]));
        assert!((goertzel(&st, 2, 0, HZ[0]) - 0.125).abs() < 2e-3 && (goertzel(&st, 2, 0, HZ[1]) - 0.125).abs() < 2e-3);
        // A stereo bus in a 5.1 project: the track pans into the stereo bus, the bus fills the master's front pair.
        let mut p = wide_track(BusMode::Surround51, AudioLayout::Stereo);
        crate::edit::set_bus_count(&mut p, 1);
        p.timeline.audio.buses[0].layout = Some(AudioLayout::Stereo);
        p.timeline.audio_tracks[0].out = Out::Bus(p.timeline.audio.buses[0].id.clone());
        let plan = compile_audio(&p);
        assert_eq!((plan.buses[0].ch, plan.tracks[0].ch), (2, 2));
        sines().block(&plan, 0, &mut out);
        assert!((amp(&out, 6, 0) - 0.25).abs() < 2e-3 && (amp(&out, 6, 1) - 0.25).abs() < 2e-3);
        assert!((2..6).all(|c| out.chunks_exact(6).all(|f| f[c] == 0.0)));
        // A stereo track straight into the 5.1 master at the front-left pan point: FL alone.
        let mut p = wide_track(BusMode::Surround51, AudioLayout::Stereo);
        p.timeline.audio_tracks[0].surround = SurroundPan { x: -1.0, y: 1.0, ..Default::default() };
        sines().block(&compile_audio(&p), 0, &mut out);
        assert!((amp(&out, 6, 0) - 0.25).abs() < 2e-3);
        assert!((1..6).all(|c| out.chunks_exact(6).all(|f| f[c].abs() < 1e-6)), "only FL");
    }

    /// Taken on the engine before R10a (see `stereo_mix_is_bit_identical_to_the_pre_r10a_engine`).
    const GOLDEN_STEREO_HASH: u64 = 0xee8f_4818_5d64_cf93;

    #[test]
    fn metronome_clicks_land_on_beats_at_120_bpm() {
        let m = Metronome::of(&crate::project::ProjectSettings::default());
        assert_eq!((m.beat, m.per_measure, m.frame(1), m.frame(4)), (0.5, 4, 24_000, 96_000));
        let mut one = vec![0.0f32; 2 * 120_000];
        m.add(0, &mut one);
        let onsets: Vec<usize> = (1..120_000).filter(|&f| one[2 * f] != 0.0 && one[2 * (f - 1)] == 0.0).map(|f| f - 1).collect();
        assert_eq!(onsets, [0, 24_000, 48_000, 72_000, 96_000]);
        let peak = |at: usize| (at..at + 960).map(|f| one[2 * f].abs()).fold(0.0, f32::max);
        assert!(peak(0) > peak(24_000) && (peak(96_000) - peak(0)).abs() < 1e-3 && one[2 * 30_000] == 0.0);
        let mut blocks = vec![0.0f32; 2 * 120_000];
        for (i, b) in blocks.chunks_mut(2 * 1000).enumerate() {
            m.add(i as i64 * 1000, b);
        }
        assert_eq!(one, blocks);
        assert!(compile_audio(&crate::project::Project::new("t")).metronome.is_none());
    }
}
