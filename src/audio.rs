//! Audio playback (the engine's mixer on a "milan-mix" thread -> rodio) and clip waveform peaks.
use eframe::egui;
use rodio::{OutputStream, OutputStreamHandle, Source};
use std::collections::HashMap;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use crate::engine::mix::{self, Mixer, frame};
use crate::engine::plan::{AudioPlan, compile_audio};
use crate::project::{MediaItem, Project, Track};

pub const RATE: u32 = 48_000;
pub const PEAKS_PER_SEC: f64 = 100.0;
const PEAK_RATE: u32 = 8_000;
/// Blocks queued between the mixer and the device (about 128 ms): how soon an edit is heard.
const QUEUE: usize = 6;
/// The device asked for no samples this long: it is gone (unplugged headphones) and is reopened.
const STALL: Duration = Duration::from_millis(400);
/// At most one device reopen this often.
const REOPEN_EVERY: Duration = Duration::from_secs(2);

fn ffmpeg() -> Command {
    let mut cmd = crate::proc::ffmpeg();
    cmd.args(["-v", "error"]);
    cmd
}

/// L/R gain for a track, or None if it is muted or another track is soloed.
pub fn track_gain(t: &Track, any_solo: bool) -> Option<[f32; 2]> {
    if t.muted || (any_solo && !t.solo) {
        return None;
    }
    let p = t.pan.clamp(-1.0, 1.0);
    Some([t.volume * (1.0 - p).min(1.0), t.volume * (1.0 + p).min(1.0)])
}

// ---- Output ----

/// Master bus shared with the output thread: L/R fader gain in, peak levels out (f32 bits in atomics).
#[derive(Default)]
pub struct Master {
    gain: [AtomicU32; 2],
    peak: [AtomicU32; 2],
}

impl Master {
    fn new() -> Self {
        let m = Self::default();
        m.set_gain([1.0, 1.0]);
        m
    }

    pub fn set_gain(&self, g: [f32; 2]) {
        for (a, v) in self.gain.iter().zip(g) {
            a.store(v.max(0.0).to_bits(), Relaxed);
        }
    }

    fn gain(&self, ch: usize) -> f32 {
        f32::from_bits(self.gain[ch].load(Relaxed))
    }

    /// Highest |sample| per channel sent to the device since the last call (after the master gain).
    pub fn take_peaks(&self) -> [f32; 2] {
        [0, 1].map(|c| f32::from_bits(self.peak[c].swap(0, Relaxed)))
    }

    fn tap(&self, ch: usize, v: f32) {
        // Non-negative f32 bit patterns order like the floats, so an integer max works.
        self.peak[ch].fetch_max(v.abs().to_bits(), Relaxed);
    }
}

/// rodio source fed with mixed blocks (start frame, samples). Its clock is the master clock.
struct Feed {
    rx: Receiver<(i64, Vec<f32>)>,
    buf: Vec<f32>,
    pos: usize,
    /// Underrun silence must come in whole frames or L/R would swap.
    odd: bool,
    /// Timeline sample (frame x 2 + channel) after the last one handed to the device. Set from each block's start
    /// frame, so it follows loop wraps exactly.
    clock: Arc<AtomicI64>,
    /// Every call, underruns included: the watchdog's sign that the device is alive.
    calls: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    master: Arc<Master>,
}

impl Iterator for Feed {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        self.calls.fetch_add(1, Relaxed);
        if self.stop.load(Relaxed) {
            return None;
        }
        if self.pos >= self.buf.len() && !self.odd {
            // A finished mixer (Disconnected) leaves silence until the session stops.
            if let Ok((f0, b)) = self.rx.try_recv() {
                (self.buf, self.pos) = (b, 0);
                self.clock.store(f0 * 2, Relaxed);
            }
        }
        if let Some(&s) = self.buf.get(self.pos) {
            let ch = self.pos % 2;
            self.pos += 1;
            self.clock.fetch_add(1, Relaxed);
            let v = s * self.master.gain(ch);
            self.master.tap(ch, v);
            return Some(v);
        }
        // Underrun: play silence without advancing the clock, so the playhead waits for audio.
        self.odd = !self.odd;
        Some(0.0)
    }
}

impl Source for Feed {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        2
    }
    fn sample_rate(&self) -> u32 {
        RATE
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

/// Seconds of a Feed clock value.
fn clock_secs(clock: i64) -> f64 {
    clock as f64 / 2.0 / RATE as f64
}

/// Watchdog: true once `calls` has not changed for STALL. `watch` is (last count, when it changed).
fn stalled(watch: &mut (u64, Instant), calls: u64, now: Instant) -> bool {
    if calls != watch.0 {
        *watch = (calls, now);
        return false;
    }
    now.saturating_duration_since(watch.1) >= STALL
}

struct Session {
    clock: Arc<AtomicI64>,
    calls: Arc<AtomicU64>,
    watch: (u64, Instant),
    stop: Arc<AtomicBool>,
    /// The plan the mixer reads once per block, and the content hash it was compiled from.
    slot: Arc<Mutex<Arc<AudioPlan>>>,
    hash: u64,
}

pub struct Player {
    device_rx: Receiver<Result<OutputStreamHandle, String>>,
    device: Option<Result<OutputStreamHandle, String>>,
    /// Dropping it lets the device thread drop its OutputStream.
    release: Option<Sender<()>>,
    opened: Instant,
    reopen_at: Option<Instant>,
    session: Option<Session>,
    error: Arc<Mutex<Option<String>>>,
    /// Master fader and meter tap (§5.12).
    pub master: Arc<Master>,
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Player {
    pub fn new() -> Self {
        let mut p = Self {
            device_rx: mpsc::channel().1,
            device: None,
            release: None,
            opened: Instant::now(),
            reopen_at: None,
            session: None,
            error: Arc::new(Mutex::new(None)),
            master: Arc::new(Master::new()),
        };
        p.open();
        p
    }

    /// Open the default output device. Opening can take a while and OutputStream is !Send, so it lives on its own
    /// thread, which holds it until `release` is dropped.
    fn open(&mut self) {
        let (tx, rx) = mpsc::channel();
        let (release, released) = mpsc::channel::<()>();
        std::thread::spawn(move || match OutputStream::try_default() {
            Ok((_stream, handle)) => {
                if tx.send(Ok(handle)).is_ok() {
                    let _ = released.recv(); // returns once `release` is dropped
                }
            }
            Err(e) => {
                let _ = tx.send(Err(format!("No audio device ({e}): playback is silent")));
            }
        });
        (self.device_rx, self.device, self.release, self.opened) = (rx, None, Some(release), Instant::now());
    }

    /// Release the output device and open the default one again (at most every 2 s), e.g. after headphones were unplugged.
    pub fn reopen(&mut self) {
        self.stop();
        // A fresh receiver, so a still-pending open of the old device is ignored.
        (self.device_rx, self.device, self.release) = (mpsc::channel().1, None, None);
        self.reopen_at = Some((self.opened + REOPEN_EVERY).max(Instant::now()));
    }

    fn poll_device(&mut self) {
        if self.reopen_at.is_some_and(|t| Instant::now() >= t) {
            self.reopen_at = None;
            self.open();
        }
        if self.device.is_none() {
            self.device = self.device_rx.try_recv().ok();
        }
    }

    /// Play the timeline's audio from `from`, looping over `lp` (seconds) inside the mixer if set.
    /// Without a device no session starts, and `position()` stays None so the caller keeps its own clock.
    pub fn start(&mut self, ctx: &egui::Context, project: &Project, from: f64, lp: Option<(f64, f64)>) {
        self.stop();
        self.poll_device();
        let handle = match &self.device {
            Some(Ok(h)) => h.clone(),
            Some(Err(e)) => {
                // Keep the message while the device is tried again.
                *self.error.lock().unwrap() = Some(e.clone());
                self.reopen();
                return;
            }
            None => return,
        };
        *self.error.lock().unwrap() = None;
        let from = frame(from);
        let (tx, rx) = mpsc::sync_channel(QUEUE);
        let clock = Arc::new(AtomicI64::new(from * 2));
        let (calls, stop) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicBool::new(false)));
        let feed = Feed { rx, buf: Vec::new(), pos: 0, odd: false, clock: clock.clone(), calls: calls.clone(), stop: stop.clone(), master: self.master.clone() };
        if let Err(e) = handle.play_raw(feed) {
            *self.error.lock().unwrap() = Some(format!("Audio output failed: {e}"));
            self.reopen();
            return;
        }
        let slot = Arc::new(Mutex::new(Arc::new(compile_audio(project))));
        let lp = lp.map(|(a, b)| (frame(a), frame(b)));
        let (s, quit, error, ctx) = (slot.clone(), stop.clone(), self.error.clone(), ctx.clone());
        std::thread::Builder::new()
            .name("milan-mix".into())
            .spawn(move || {
                mix::run(&s, Mixer::ffmpeg(), from, lp, &tx, &quit, |e| {
                    *error.lock().unwrap() = Some(e);
                    ctx.request_repaint();
                })
            })
            .expect("spawn milan-mix");
        let hash = crate::engine::content_hash(project);
        self.session = Some(Session { clock, calls, watch: (0, Instant::now()), stop, slot, hash });
    }

    /// While playing: give the mixer the project's new plan after an edit. It is heard within about 150 ms.
    pub fn set_plan(&mut self, project: &Project) {
        let Some(s) = &mut self.session else { return };
        let hash = crate::engine::content_hash(project);
        if hash != s.hash {
            s.hash = hash;
            *s.slot.lock().unwrap() = Arc::new(compile_audio(project));
        }
    }

    pub fn stop(&mut self) {
        if let Some(s) = self.session.take() {
            s.stop.store(true, Relaxed);
        }
    }

    /// A session is running (it may be stalled).
    pub fn running(&self) -> bool {
        self.session.is_some()
    }

    /// The device has stopped asking for samples for 400 ms: the session should be stopped and the device reopened.
    pub fn stalled(&mut self) -> bool {
        self.session.as_mut().is_some_and(|s| stalled(&mut s.watch, s.calls.load(Relaxed), Instant::now()))
    }

    /// Timeline time of the last sample handed to the device, while a session runs and the device is alive.
    pub fn position(&mut self) -> Option<f64> {
        if self.stalled() {
            return None;
        }
        self.session.as_ref().map(|s| clock_secs(s.clock.load(Relaxed)))
    }

    pub fn status(&mut self) -> Option<String> {
        self.poll_device();
        if let Some(Err(e)) = &self.device {
            return Some(e.clone());
        }
        self.error.lock().unwrap().clone()
    }
}


// ---- Waveforms ----

/// Audio channels of the first audio stream (1 when unknown).
fn channel_count(path: &Path) -> usize {
    let mut cmd = crate::proc::ffprobe();
    cmd.args(["-v", "error", "-select_streams", "a:0", "-show_entries", "stream=channels", "-of", "csv=p=0"]).arg(path);
    crate::proc::output(&mut cmd).ok().and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok()).unwrap_or(1)
}

/// Peaks per 1/PEAKS_PER_SEC s of the source, 0..=1, one list per channel: stereo (or more, folded to 2) gives
/// L and R, as VEGAS draws two stacked waveforms; mono gives one.
fn peaks(path: &Path) -> Vec<Vec<f32>> {
    let ch = channel_count(path).clamp(1, 2);
    let child = crate::proc::spawn(
        ffmpeg()
            .arg("-i")
            .arg(path)
            .args(["-vn", "-ac", &ch.to_string(), "-ar", &PEAK_RATE.to_string(), "-f", "f32le", "-"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null()),
    );
    let Ok(mut child) = child else { return Vec::new() };
    let per = (PEAK_RATE as f64 / PEAKS_PER_SEC) as usize;
    let (mut out, mut peak, mut n) = (vec![Vec::new(); ch], vec![0.0_f32; ch], 0);
    // Streamed so long files never sit in memory as raw PCM. Samples are interleaved by channel.
    let mut r = BufReader::new(child.stdout.take().expect("piped stdout"));
    let mut b = [0u8; 4];
    let mut c = 0;
    while r.read_exact(&mut b).is_ok() {
        peak[c] = peak[c].max(f32::from_le_bytes(b).abs().min(1.0));
        c += 1;
        if c == ch {
            c = 0;
            n += 1;
            if n == per {
                for k in 0..ch {
                    out[k].push(peak[k]);
                }
                peak.fill(0.0);
                n = 0;
            }
        }
    }
    if n > 0 {
        for k in 0..ch {
            out[k].push(peak[k]);
        }
    }
    let _ = child.wait();
    out
}

/// Runs a peaks decode, and once more if it gives nothing (a busy disk or a file still being written).
fn retry_empty(mut decode: impl FnMut() -> Vec<Vec<f32>>) -> Vec<Vec<f32>> {
    let p = decode();
    if p.first().is_none_or(|c| c.is_empty()) { decode() } else { p }
}

/// Per-media peak cache filled by a background thread. Failures (after one retry) cache as empty.
pub struct Waveforms {
    cache: HashMap<String, Option<Vec<Vec<f32>>>>,
    tx: Sender<(String, PathBuf)>,
    rx: Receiver<(String, Vec<Vec<f32>>)>,
    /// Set while the timeline plays, so peak decodes leave the CPU to playback.
    paused: Arc<AtomicBool>,
}

impl Waveforms {
    pub fn new(ctx: &egui::Context) -> Self {
        let (tx, req_rx) = mpsc::channel::<(String, PathBuf)>();
        let (res_tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let paused = Arc::new(AtomicBool::new(false));
        let p = paused.clone();
        std::thread::spawn(move || {
            for (id, path) in req_rx {
                while p.load(Relaxed) {
                    std::thread::sleep(Duration::from_millis(50));
                }
                if res_tx.send((id, retry_empty(|| peaks(&path)))).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        Self { cache: HashMap::new(), tx, rx, paused }
    }

    pub fn set_paused(&self, on: bool) {
        self.paused.store(on, Relaxed);
    }

    /// Some requested peaks have not arrived yet.
    pub fn pending(&mut self) -> bool {
        for (id, p) in self.rx.try_iter() {
            self.cache.insert(id, Some(p));
        }
        self.cache.values().any(|v| v.is_none())
    }

    /// Peaks for `m`, one list per channel (1 or 2), requesting them on first use.
    pub fn get(&mut self, m: &MediaItem) -> Option<&[Vec<f32>]> {
        for (id, p) in self.rx.try_iter() {
            self.cache.insert(id, Some(p));
        }
        if !self.cache.contains_key(&m.id) {
            self.cache.insert(m.id.clone(), None);
            let _ = self.tx.send((m.id.clone(), m.path.clone()));
        }
        self.cache[&m.id].as_deref().filter(|p| p.first().is_some_and(|c| !c.is_empty()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(master: Arc<Master>) -> (mpsc::SyncSender<(i64, Vec<f32>)>, Feed) {
        let (tx, rx) = mpsc::sync_channel(4);
        let (clock, calls, stop) = (Arc::new(AtomicI64::new(0)), Arc::new(AtomicU64::new(0)), Arc::new(AtomicBool::new(false)));
        (tx, Feed { rx, buf: Vec::new(), pos: 0, odd: false, clock, calls, stop, master })
    }

    #[test]
    fn pan_and_volume_gain() {
        let mut t = Track::new("a", crate::project::TrackKind::Audio);
        t.volume = 0.8;
        assert_eq!(track_gain(&t, false), Some([0.8, 0.8]));
        t.pan = -1.0;
        assert_eq!(track_gain(&t, false), Some([0.8, 0.0]));
        t.pan = 0.5;
        assert_eq!(track_gain(&t, false), Some([0.4, 0.8]));
        assert_eq!(track_gain(&t, true), None); // another track is solo
        t.solo = true;
        t.muted = true;
        assert_eq!(track_gain(&t, true), None); // mute beats solo
    }

    #[test]
    fn master_gain_and_peak_tap() {
        let master = Arc::new(Master::new());
        master.set_gain([0.5, 0.0]);
        let (tx, mut feed) = feed(master.clone());
        tx.send((0, vec![0.8, 0.8, -1.0, 0.2])).unwrap();
        let out: Vec<f32> = (0..4).map(|_| feed.next().unwrap()).collect();
        assert_eq!(out, [0.4, 0.0, -0.5, 0.0]);
        assert_eq!(master.take_peaks(), [0.5, 0.0]);
        assert_eq!(master.take_peaks(), [0.0, 0.0]); // reading resets
    }

    #[test]
    fn feed_clock_follows_block_starts() {
        let (tx, mut feed) = feed(Arc::new(Master::new()));
        tx.send((100, vec![0.1; 4])).unwrap();
        tx.send((10, vec![0.2; 4])).unwrap(); // a loop wrap back to frame 10
        let clock = feed.clock.clone();
        let frames = || clock.load(Relaxed) as f64 / 2.0;
        for _ in 0..4 {
            feed.next();
        }
        assert_eq!(frames(), 102.0);
        feed.next();
        assert_eq!(frames(), 10.5);
        for _ in 0..3 {
            feed.next();
        }
        assert_eq!(frames(), 12.0);
        // Underrun silence and a finished mixer leave the clock where it is but still count as calls.
        drop(tx);
        assert_eq!((feed.next(), feed.next()), (Some(0.0), Some(0.0)));
        assert_eq!(frames(), 12.0);
        assert_eq!(feed.calls.load(Relaxed), 10);
        feed.stop.store(true, Relaxed);
        assert_eq!(feed.next(), None);
    }

    #[test]
    fn watchdog_fires_400_ms_after_calls_stop() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let mut watch = (0, t0);
        assert!(!stalled(&mut watch, 5, ms(100)));
        assert!(!stalled(&mut watch, 9, ms(450))); // still advancing
        assert!(!stalled(&mut watch, 9, ms(849)));
        assert!(stalled(&mut watch, 9, ms(850)));
        assert!(!stalled(&mut watch, 10, ms(900))); // calls changed: reset
        assert!(!stalled(&mut watch, 10, ms(1200)));
        assert!(stalled(&mut watch, 10, ms(1300)));
    }

    #[test]
    fn waveform_decode_retries_once() {
        let mut calls = 0;
        assert!(retry_empty(|| { calls += 1; Vec::new() }).is_empty());
        assert_eq!(calls, 2);
        let mut calls = 0;
        assert_eq!(retry_empty(|| { calls += 1; vec![vec![0.5]] }), [vec![0.5]]);
        assert_eq!(calls, 1);
    }
}
