//! Audio playback through the output device and clip waveform peaks. R9: the device layer is cpal (WASAPI shared mode),
//! or WASAPI exclusive mode (`wasapi.rs`). The engine's mixer runs on a "milan-mix" thread and queues blocks; the device
//! callback (`Out::render`) resamples them to the device rate, adds the input monitor, applies the master and keeps the
//! playback clock. It takes no lock and frees nothing: what it lets go of goes back to the UI thread (`Trash`).
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, AtomicUsize, Ordering::{Acquire, Relaxed, Release}, fence};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use crate::engine::afx::Chain;
use crate::engine::mix::{self, Block, Meter, Meters, Mixer, frame};
use crate::engine::plan::{AudioPlan, QUICKFADE, compile_audio};
use crate::engine::plugins::{self, FxEval};
use crate::edit;
use crate::project::{AudioSettings, Fx, MediaItem, NormSpan, Normalized, Out as Route, PanType, Project};

pub const RATE: u32 = 48_000;
pub const PEAKS_PER_SEC: f64 = 100.0;
const PEAK_RATE: u32 = 8_000;
/// Blocks queued between the mixer and the device (about 128 ms): how soon an edit is heard.
const QUEUE: usize = 6;
/// The device asked for no samples this long: it is gone (unplugged headphones) and is reopened.
const STALL: Duration = Duration::from_millis(400);
/// At most one device reopen this often.
const REOPEN_EVERY: Duration = Duration::from_secs(2);
/// R8b: Dim Output's -20 dB.
const DIM: f32 = 0.1;
/// R9: the output callback works in pieces this long (frames), so its scratch buffers are allocated once.
const CHUNK: usize = 512;
/// R9: the input ring's length in frames (a power of two; about 340 ms at 48 kHz).
const RING: usize = 1 << 14;

fn ffmpeg() -> Command {
    let mut cmd = crate::proc::ffmpeg();
    cmd.args(["-v", "error"]);
    cmd
}

// ---- Output ----

/// Master bus shared with the output thread: L/R fader gain, Dim and Downmix in; peak and RMS levels out (f32 bits in
/// atomics).
#[derive(Default)]
pub struct Master {
    gain: [AtomicU32; 2],
    peak: [AtomicU32; 2],
    /// R8b: the RMS of the last 50 ms sent to the device, per channel.
    rms: [AtomicU32; 2],
    /// R8b: Dim Output (-20 dB) and Downmix Output (stereo to mono): playback monitoring only, never rendered.
    pub dim: AtomicBool,
    pub mono: AtomicBool,
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

    /// R8b: the RMS of the last 50 ms sent to the device (for the VU meters).
    pub fn rms(&self) -> [f32; 2] {
        [0, 1].map(|c| f32::from_bits(self.rms[c].load(Relaxed)))
    }

    /// The device stopped: the RMS falls to silence.
    pub fn clear_rms(&self) {
        self.rms.iter().for_each(|a| a.store(0, Relaxed));
    }

    fn tap(&self, ch: usize, v: f32) {
        // Non-negative f32 bit patterns order like the floats, so an integer max works.
        self.peak[ch].fetch_max(v.abs().to_bits(), Relaxed);
    }
}

/// R9: Preferences › Audio Device › Audio device type. Microsoft Sound Mapper plays on the Windows default device and
/// Windows Classic Wave Driver on the chosen one, both through WASAPI shared mode (as VEGAS's wave drivers go through
/// the Windows mixer). Windows Audio (Exclusive Mode) is Milan Pro's, not VEGAS's: it takes the chosen device for itself
/// at its smallest period, the low-latency path in place of ASIO (whose SDK licence forbids shipping it).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceType {
    #[default]
    SoundMapper,
    WaveDriver,
    Exclusive,
}

impl DeviceType {
    pub const ALL: [DeviceType; 3] = [DeviceType::SoundMapper, DeviceType::WaveDriver, DeviceType::Exclusive];

    pub fn name(self) -> &'static str {
        ["Microsoft Sound Mapper", "Windows Classic Wave Driver", "Windows Audio (Exclusive Mode)"][self as usize]
    }
}

/// R9: the output device as the preferences set it: the type, the device (None, or a name not found: the Windows
/// default) and the buffer size in frames (0: the device's default).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeviceConfig {
    pub kind: DeviceType,
    pub device: Option<String>,
    pub buffer: u32,
}

/// Names of the output devices (Preferences › Audio Device).
pub fn output_devices() -> Vec<String> {
    cpal::default_host().output_devices().map(|ds| ds.filter_map(|d| d.name().ok()).collect()).unwrap_or_default()
}

/// R9: the recording input on its way to the output callback: a lock-free single-producer (the input callback),
/// single-consumer (the output callback) ring of stereo frames, with the input's rate (0 while it is closed) and its
/// latency (capture to callback, µs).
pub struct Ring {
    buf: Box<[AtomicU32]>,
    /// Frames ever written and read (wrapping); the slot of frame k is k % RING.
    head: AtomicUsize,
    tail: AtomicUsize,
    pub rate: AtomicU32,
    pub latency_us: AtomicU32,
}

impl Default for Ring {
    fn default() -> Self {
        Self { buf: (0..2 * RING).map(|_| AtomicU32::new(0)).collect(), head: AtomicUsize::new(0), tail: AtomicUsize::new(0), rate: AtomicU32::new(0), latency_us: AtomicU32::new(0) }
    }
}

impl Ring {
    /// Producer: append stereo frames; those that do not fit are dropped (nobody is listening).
    pub fn push(&self, stereo: &[f32]) {
        let (h, t) = (self.head.load(Relaxed), self.tail.load(Acquire));
        let n = (stereo.len() / 2).min(RING.saturating_sub(h.wrapping_sub(t)));
        for (i, f) in stereo.chunks_exact(2).take(n).enumerate() {
            let k = 2 * (h.wrapping_add(i) % RING);
            self.buf[k].store(f[0].to_bits(), Relaxed);
            self.buf[k + 1].store(f[1].to_bits(), Relaxed);
        }
        self.head.store(h.wrapping_add(n), Release);
    }

    /// Consumer: append every frame waiting to `out` (stereo).
    pub fn take(&self, out: &mut Vec<f32>) {
        let at = out.len();
        out.resize(at + 2 * self.len(), 0.0);
        self.pop(&mut out[at..]);
    }

    /// Frames waiting.
    fn len(&self) -> usize {
        self.head.load(Acquire).wrapping_sub(self.tail.load(Relaxed))
    }

    /// Consumer: drop all but the newest `keep` frames (stale input would only add latency).
    fn keep(&self, keep: usize) {
        let n = self.len();
        if n > keep {
            self.tail.store(self.tail.load(Relaxed).wrapping_add(n - keep), Release);
        }
    }

    /// Consumer: the oldest frames into `out` (stereo), silence past what is there.
    fn pop(&self, out: &mut [f32]) {
        let t = self.tail.load(Relaxed);
        let n = self.len().min(out.len() / 2);
        for (i, f) in out.chunks_exact_mut(2).enumerate() {
            if i < n {
                let k = 2 * (t.wrapping_add(i) % RING);
                (f[0], f[1]) = (f32::from_bits(self.buf[k].load(Relaxed)), f32::from_bits(self.buf[k + 1].load(Relaxed)));
            } else {
                f.fill(0.0);
            }
        }
        self.tail.store(t.wrapping_add(n), Release);
    }
}

/// R9: one input monitor path, an armed track or an input bus: its FX (automation left out, as VEGAS bypasses it while
/// monitoring) with their values, then volume, pan, phase and the routing to the master as one stereo matrix
/// ([[L from L, L from R], [R from L, R from R]]).
#[derive(Clone)]
pub struct MonPath {
    pub id: String,
    /// An input bus (else a track).
    input: bool,
    fx: Vec<Fx>,
    vals: Vec<Option<FxEval>>,
    m: [[f32; 2]; 2],
    meter: Option<Arc<Meter>>,
}

/// R9: what the input monitor plays, built on the UI thread from the project each frame and handed to the output
/// callback when it changes.
#[derive(Clone, Default)]
pub struct MonPlan {
    pub paths: Vec<MonPath>,
}

impl PartialEq for MonPlan {
    fn eq(&self, o: &Self) -> bool {
        self.paths.len() == o.paths.len() && self.paths.iter().zip(&o.paths).all(|(a, b)| a.id == b.id && a.m == b.m && a.fx == b.fx)
    }
}

/// The gain from output `out` to the master: the volumes (and mutes) of the busses on the way.
// shortcut: the monitor skips the busses' FX, pan and envelopes (they run ahead in the mix thread); follow them if
// users route monitored inputs through bus effects.
fn route_gain<'a>(a: &'a AudioSettings, mut out: &'a Route) -> f32 {
    let mut g = 1.0;
    for _ in 0..=a.buses.len() {
        let Route::Bus(id) = out else { break };
        let Some(b) = a.buses.iter().find(|b| b.id == *id) else { break };
        g *= if b.muted { 0.0 } else { b.volume };
        out = &b.out;
    }
    g
}

impl MonPlan {
    /// The armed audio tracks whose Input Monitoring hears the input now (`playing`, `recording`), through their FX at
    /// time `t`, volume, pan (by their Pan Type), phase and output; and the unmuted input busses.
    pub fn new(p: &Project, playing: bool, recording: bool, t: f64) -> Self {
        let tl = &p.timeline;
        let a = &tl.audio;
        let vals = |fx: &[Fx]| -> Vec<Option<FxEval>> {
            fx.iter()
                .map(|f| {
                    let mut f = f.clone();
                    f.auto.clear();
                    f.enabled.then(|| plugins::eval(&f, t)).flatten()
                })
                .collect()
        };
        let scale = |m: [[f32; 2]; 2], g: f32| m.map(|r| r.map(|v| v * g));
        let solo = tl.any_solo(false);
        let tracks = tl.audio_tracks.iter().filter(|tr| tr.armed && tr.monitor.hears(playing, recording) && tl.plays(tr, solo)).map(|tr| {
            let g = tr.volume * if tr.invert { -1.0 } else { 1.0 } * route_gain(a, &tr.out);
            MonPath { id: tr.id.clone(), input: false, fx: tr.fx.clone(), vals: vals(&tr.fx), m: scale(tr.pan_type.matrix(tr.pan), g), meter: None }
        });
        // Input busses pan as the busses do (Balance, 0 dB centre).
        let inputs = a.inputs.iter().filter(|b| !b.muted).map(|b| {
            let g = b.volume * route_gain(a, &b.out);
            MonPath { id: b.id.clone(), input: true, fx: b.fx.clone(), vals: vals(&b.fx), m: scale(PanType::Balance0.matrix(b.pan), g), meter: None }
        });
        Self { paths: tracks.chain(inputs).collect() }
    }

    /// The same paths with the same plug-ins: only values changed, so the running chains keep their state.
    fn same_chains(&self, o: &Self) -> bool {
        self.paths.len() == o.paths.len()
            && self.paths.iter().zip(&o.paths).all(|(a, b)| a.id == b.id && a.fx.len() == b.fx.len() && a.fx.iter().zip(&b.fx).all(|(x, y)| x.id == y.id && x.plugin == y.plugin))
    }
}

/// The monitor's running state, owned by the output callback.
struct MonState {
    plan: Arc<MonPlan>,
    chains: Vec<Chain>,
}

impl MonState {
    /// Made on the UI thread: the plug-ins are created here, never in the callback.
    fn new(plan: Arc<MonPlan>) -> Self {
        let chains = plan
            .paths
            .iter()
            .map(|p| {
                let mut c = Chain::default();
                c.sync(&p.fx);
                c
            })
            .collect();
        Self { plan, chains }
    }

    /// The input frames in `inp` through every path, mixed into `out`; each input bus's meter fed.
    fn process(&mut self, inp: &[f32], tmp: &mut [f32], out: &mut [f32]) {
        out.fill(0.0);
        let n = (inp.len() / 2).max(1) as f32;
        for (p, ch) in self.plan.paths.iter().zip(&mut self.chains) {
            tmp.copy_from_slice(inp);
            ch.run_with(&p.fx, &p.vals, tmp);
            let (mut pk, mut sq) = ([0.0f32; 2], [0.0f32; 2]);
            for (o, x) in out.chunks_exact_mut(2).zip(tmp.chunks_exact(2)) {
                let y = [x[0] * p.m[0][0] + x[1] * p.m[0][1], x[0] * p.m[1][0] + x[1] * p.m[1][1]];
                for c in 0..2 {
                    o[c] += y[c];
                    pk[c] = pk[c].max(y[c].abs());
                    sq[c] += y[c] * y[c];
                }
            }
            if let Some(m) = &p.meter {
                m.put(pk, sq.map(|s| (s / n).sqrt()));
            }
        }
    }
}

/// To the output callback: new monitor values (same plug-ins), or a whole new monitor.
enum MonMsg {
    Plan(Arc<MonPlan>),
    State(Box<MonState>),
}

/// What the output callback lets go of, sent back so that the UI thread frees it.
#[allow(dead_code)] // only ever dropped
enum Trash {
    Block(Vec<f32>),
    Feed(Feed),
    Plan(Arc<MonPlan>),
    State(Box<MonState>),
}

fn trash(to: &SyncSender<Trash>, t: Trash) {
    // Full (the UI is not running): freed here after all, as it always was.
    let _ = to.try_send(t);
}

/// One playback session's mixed blocks (start frame, rate, samples), as the output callback takes them. Its clock is
/// the master clock.
struct Feed {
    rx: Receiver<Block>,
    buf: Vec<f32>,
    /// Frames of `buf` taken; the block's start (timeline frame) and play rate: its frame `pos` is timeline frame
    /// f0 + pos x rate.
    pos: usize,
    f0: i64,
    rate: f64,
    /// Timeline sample (frame x 2 + channel) after the last frame taken. Set from each block's start frame and rate, so
    /// it follows loop wraps and shuttle speeds exactly.
    clock: Arc<AtomicI64>,
    stop: Arc<AtomicBool>,
    /// The session, so a stamp from an earlier one is never read as this one's.
    serial: u64,
}

impl Feed {
    /// The next mixed frame; None on an underrun (the clock waits, so the playhead waits for the audio).
    fn frame(&mut self, bin: &SyncSender<Trash>) -> Option<[f32; 2]> {
        if 2 * self.pos + 1 >= self.buf.len() {
            // A finished mixer (Disconnected) leaves silence until the session stops.
            let (f0, rate, b) = self.rx.try_recv().ok()?;
            trash(bin, Trash::Block(std::mem::replace(&mut self.buf, b)));
            (self.pos, self.f0, self.rate) = (0, f0, rate);
            self.clock.store(f0 * 2, Relaxed);
            if self.buf.len() < 2 {
                return None;
            }
        }
        let i = 2 * self.pos;
        self.pos += 1;
        self.clock.store(self.f0 * 2 + (2.0 * self.pos as f64 * self.rate).round() as i64, Relaxed);
        Some([self.buf[i], self.buf[i + 1]])
    }
}

/// Nanoseconds since the first call: the time base of the playback stamp.
fn now_ns() -> u64 {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    EPOCH.get_or_init(Instant::now).elapsed().as_nanos() as u64
}

/// R9: what was heard when, written by the output callback (a seqlock: the writer never waits, a reader retries).
/// `at` is the timeline second heard at `t` (now_ns), playing on at `rate` for at most `cap` ns (up to the first frame
/// of the last buffer).
#[derive(Default)]
struct Stamp {
    seq: AtomicU64,
    serial: AtomicU64,
    at: AtomicU64,
    t: AtomicU64,
    rate: AtomicU64,
    cap: AtomicU64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Heard {
    serial: u64,
    at: f64,
    t: u64,
    rate: f64,
    cap: u64,
}

impl Heard {
    /// The timeline second heard at `now` (ns).
    fn at(&self, now: u64) -> f64 {
        self.at + self.rate * now.saturating_sub(self.t).min(self.cap) as f64 * 1e-9
    }
}

impl Stamp {
    fn write(&self, h: Heard) {
        let s = self.seq.load(Relaxed);
        self.seq.store(s.wrapping_add(1), Relaxed);
        fence(Release);
        self.serial.store(h.serial, Relaxed);
        self.at.store(h.at.to_bits(), Relaxed);
        self.t.store(h.t, Relaxed);
        self.rate.store(h.rate.to_bits(), Relaxed);
        self.cap.store(h.cap, Relaxed);
        self.seq.store(s.wrapping_add(2), Release);
    }

    fn read(&self) -> Option<Heard> {
        for _ in 0..8 {
            let s = self.seq.load(Acquire);
            let h = Heard {
                serial: self.serial.load(Relaxed),
                at: f64::from_bits(self.at.load(Relaxed)),
                t: self.t.load(Relaxed),
                rate: f64::from_bits(self.rate.load(Relaxed)),
                cap: self.cap.load(Relaxed),
            };
            fence(Acquire);
            if s % 2 == 0 && self.seq.load(Relaxed) == s {
                return Some(h);
            }
        }
        None
    }
}

/// What a device's callback shares with the player: the call count (the watchdog's sign of life), a lost device and
/// why, the playback stamp, the output latency and how full the input ring was (both µs) and a monitor whose input runs
/// at another rate.
#[derive(Default)]
struct Shared {
    calls: AtomicU64,
    lost: AtomicBool,
    why: Mutex<Option<String>>,
    stamp: Stamp,
    out_us: AtomicU32,
    fill_us: AtomicU32,
    mismatch: AtomicBool,
}

impl Shared {
    /// The device reported an error (from its own thread; never waits).
    fn fail(&self, e: String) {
        if let Ok(mut w) = self.why.try_lock() {
            w.get_or_insert(e);
        }
        self.lost.store(true, Relaxed);
    }
}

/// The output callback's state: the session feed, the monitor, the master and the 48 kHz-to-device resampler.
struct Out {
    feed: Option<Feed>,
    feeds: Receiver<Feed>,
    mons: Receiver<MonMsg>,
    bin: SyncSender<Trash>,
    master: Arc<Master>,
    shared: Arc<Shared>,
    ring: Arc<Ring>,
    /// R16c: the master output for the Loudness Meters (after the master fader, before Dim and Downmix, which only
    /// monitor), at the device rate; a session's first frames follow a NaN frame, which resets the meter.
    // shortcut: a device not at 48 kHz is metered after the playback resampler (linear interpolation), a hair off the
    // mix in the highs; the loudness log measures the 48 kHz mix exactly. Tap the feed if users meter at 44.1 kHz.
    loud: Arc<Ring>,
    ltap: Vec<f32>,
    mon: Box<MonState>,
    /// The device rate, and RATE / rate: mixer frames per device frame.
    rate: u32,
    step: f64,
    /// Linear interpolation between mixer frames `prev` and `next` (as rodio converted rates).
    frac: f64,
    prev: [f32; 2],
    next: [f32; 2],
    /// The master gain applied (with Dim), gliding to the fader's (about 2 ms), so a move never steps on one sample.
    gain: [f32; 2],
    /// R8b: how far Downmix Output has blended L and R (0 stereo .. 1 mono), gliding the same way.
    mono: f32,
    /// R8b: sum of squares and frame count of the current 50 ms RMS window.
    sq: [f32; 2],
    frames: usize,
    inp: Vec<f32>,
    tmp: Vec<f32>,
    mbuf: Vec<f32>,
}

impl Out {
    #[allow(clippy::too_many_arguments)]
    fn new(feeds: Receiver<Feed>, mons: Receiver<MonMsg>, bin: SyncSender<Trash>, master: Arc<Master>, shared: Arc<Shared>, ring: Arc<Ring>, loud: Arc<Ring>, mon: Box<MonState>) -> Self {
        let gain = [master.gain(0), master.gain(1)];
        let mut o = Self {
            feed: None,
            feeds,
            mons,
            bin,
            master,
            shared,
            ring,
            loud,
            ltap: vec![0.0; 2 * CHUNK],
            mon,
            rate: RATE,
            step: 1.0,
            frac: 0.0,
            prev: [0.0; 2],
            next: [0.0; 2],
            gain,
            mono: 0.0,
            sq: [0.0; 2],
            frames: 0,
            inp: vec![0.0; 2 * CHUNK],
            tmp: vec![0.0; 2 * CHUNK],
            mbuf: vec![0.0; 2 * CHUNK],
        };
        o.set_rate(RATE);
        o
    }

    fn set_rate(&mut self, rate: u32) {
        self.rate = rate.max(1);
        self.step = RATE as f64 / self.rate as f64;
        self.loud.rate.store(self.rate, Relaxed);
    }

    /// The device callback: fill `out` (`ch` interleaved channels at the device rate), whose first frame is heard
    /// `latency` from now.
    fn render(&mut self, out: &mut [f32], ch: usize, latency: Duration) {
        self.render_at(out, ch, latency, now_ns());
    }

    fn render_at(&mut self, out: &mut [f32], ch: usize, latency: Duration, now: u64) {
        self.shared.calls.fetch_add(1, Relaxed);
        while let Ok(f) = self.feeds.try_recv() {
            if let Some(old) = self.feed.replace(f) {
                trash(&self.bin, Trash::Feed(old));
            }
            self.loud.push(&[f32::NAN; 2]); // a new session: the loudness meters start again (VEGAS)
        }
        if self.feed.as_ref().is_some_and(|f| f.stop.load(Relaxed)) {
            if let Some(old) = self.feed.take() {
                trash(&self.bin, Trash::Feed(old));
            }
        }
        while let Ok(m) = self.mons.try_recv() {
            match m {
                MonMsg::Plan(p) => trash(&self.bin, Trash::Plan(std::mem::replace(&mut self.mon.plan, p))),
                MonMsg::State(s) => trash(&self.bin, Trash::State(std::mem::replace(&mut self.mon, s))),
            }
        }
        let ch = ch.max(1);
        let n = out.len() / ch;
        // The input monitor runs when its input runs at the device rate; the ring keeps at most a buffer of slack.
        let want = !self.mon.plan.paths.is_empty();
        let in_rate = self.ring.rate.load(Relaxed);
        let on = want && in_rate == self.rate;
        self.shared.mismatch.store(want && in_rate != 0 && in_rate != self.rate, Relaxed);
        self.ring.keep(if on { n + n.max(256) } else { 0 });
        if on {
            self.shared.fill_us.store((self.ring.len() as u64 * 1_000_000 / self.rate as u64) as u32, Relaxed);
        }
        let c0 = self.feed.as_ref().map(|f| f.clock.load(Relaxed));
        let mut under = false;
        let mut done = 0;
        while done < n {
            let m = (n - done).min(CHUNK);
            if on {
                self.ring.pop(&mut self.inp[..2 * m]);
                self.mon.process(&self.inp[..2 * m], &mut self.tmp[..2 * m], &mut self.mbuf[..2 * m]);
            } else {
                self.mbuf[..2 * m].fill(0.0);
            }
            for k in 0..m {
                let v = self.resampled(&mut under);
                let (y, l) = self.master_frame([v[0] + self.mbuf[2 * k], v[1] + self.mbuf[2 * k + 1]]);
                // A NaN or infinity from a plug-in must not read as the reset frame or poison the meter.
                (self.ltap[2 * k], self.ltap[2 * k + 1]) = (if l[0].is_finite() { l[0] } else { 0.0 }, if l[1].is_finite() { l[1] } else { 0.0 });
                let o = &mut out[(done + k) * ch..(done + k + 1) * ch];
                if ch == 1 {
                    o[0] = (y[0] + y[1]) * 0.5;
                } else {
                    (o[0], o[1]) = (y[0], y[1]);
                    o[2..].fill(0.0);
                }
            }
            if self.feed.is_some() {
                self.loud.push(&self.ltap[..2 * m]);
            }
            done += m;
        }
        let buffer = Duration::from_secs_f64(n as f64 / self.rate as f64);
        self.shared.out_us.store((latency + buffer).as_micros() as u32, Relaxed);
        if let (Some(f), Some(c0)) = (&self.feed, c0) {
            // The buffer's first frame (timeline c0) is heard `latency` from now: now, the one `latency` before it is.
            let at = clock_secs(c0) - latency.as_secs_f64() * f.rate;
            // An underrun holds what is heard where it is.
            let rate = if under { 0.0 } else { f.rate };
            // Carried on at most to that first frame: never past a loop end the mixer has not wrapped yet.
            self.shared.stamp.write(Heard { serial: f.serial, at, t: now, rate, cap: latency.as_nanos() as u64 });
        }
    }

    /// The next mixer frame (silence without a session).
    fn pull(&mut self, under: &mut bool) -> [f32; 2] {
        match &mut self.feed {
            Some(feed) => feed.frame(&self.bin).unwrap_or_else(|| {
                *under = true;
                [0.0; 2]
            }),
            None => [0.0; 2],
        }
    }

    /// The next frame at the device rate (a 48 kHz device takes the mixer's frames as they are).
    fn resampled(&mut self, under: &mut bool) -> [f32; 2] {
        if self.step == 1.0 {
            return self.pull(under);
        }
        let f = self.frac as f32;
        let v = [self.prev[0] + (self.next[0] - self.prev[0]) * f, self.prev[1] + (self.next[1] - self.prev[1]) * f];
        self.frac += self.step;
        while self.frac >= 1.0 {
            self.frac -= 1.0;
            self.prev = self.next;
            self.next = self.pull(under);
        }
        v
    }

    /// The master: fader gain and Dim (gliding), Downmix, the peak tap and the 50 ms RMS. Also the frame for the
    /// loudness meters: after the fader, before Dim and Downmix.
    fn master_frame(&mut self, x: [f32; 2]) -> ([f32; 2], [f32; 2]) {
        let glide = |g: &mut f32, to: f32| *g = if (to - *g).abs() < 1e-6 { to } else { *g + (to - *g) / 96.0 };
        glide(&mut self.mono, if self.master.mono.load(Relaxed) { 1.0 } else { 0.0 });
        let dim = if self.master.dim.load(Relaxed) { DIM } else { 1.0 };
        let m = self.mono * 0.5;
        let (mut y, mut l) = ([0.0; 2], [0.0; 2]);
        for c in 0..2 {
            let g = self.master.gain(c);
            l[c] = x[c] * g;
            glide(&mut self.gain[c], g * dim);
            let v = (x[c] * (1.0 - m) + x[1 - c] * m) * self.gain[c];
            self.master.tap(c, v);
            self.sq[c] += v * v;
            y[c] = v;
        }
        self.frames += 1;
        if self.frames >= mix::METER_WINDOW {
            for c in 0..2 {
                self.master.rms[c].store((self.sq[c] / self.frames as f32).sqrt().to_bits(), Relaxed);
            }
            (self.sq, self.frames) = ([0.0; 2], 0);
        }
        (y, l)
    }
}

/// The time from a callback to its buffer's first frame being heard, as the device reports it.
fn latency(i: &cpal::OutputCallbackInfo) -> Duration {
    let ts = i.timestamp();
    ts.playback.duration_since(&ts.callback).unwrap_or_default()
}

/// A scratch f32 buffer of `n` samples for a device that takes integers.
fn scratch(buf: &mut Vec<f32>, n: usize) -> &mut [f32] {
    if buf.len() < n {
        buf.resize(n, 0.0); // a bigger buffer than any before: once
    }
    &mut buf[..n]
}

/// Open the output stream for `cfg`, rendering through `out`. The stream (to hold, and drop to close), the device rate
/// and its name.
fn open_output(cfg: &DeviceConfig, mut out: Out) -> Result<(Box<dyn std::any::Any>, u32), String> {
    #[cfg(windows)]
    if cfg.kind == DeviceType::Exclusive {
        let shared = out.shared.clone();
        let (h, rate) = crate::wasapi::open(
            cfg.device.as_deref(),
            cfg.buffer,
            move |rate, ch| {
                out.set_rate(rate);
                move |buf: &mut [f32], lat: Duration| out.render(buf, ch, lat)
            },
            move |e| shared.fail(e),
        )?;
        return Ok((Box::new(h), rate));
    }
    let host = cpal::default_host();
    let chosen = match (cfg.kind, &cfg.device) {
        (DeviceType::SoundMapper, _) | (_, None) => None,
        (_, Some(n)) => host.output_devices().ok().and_then(|mut ds| ds.find(|d| d.name().is_ok_and(|x| x == *n))),
    };
    let dev = chosen.or_else(|| host.default_output_device()).ok_or("No audio device: playback is silent")?;
    let name = dev.name().unwrap_or_default();
    let fail = |e: &dyn std::fmt::Display| format!("Audio device {name}: {e}");
    let sc = dev.default_output_config().map_err(|e| fail(&e))?;
    let (fmt, ch, rate) = (sc.sample_format(), sc.channels().max(1) as usize, sc.sample_rate().0);
    out.set_rate(rate);
    let mut config = sc.config();
    if cfg.buffer > 0 {
        config.buffer_size = cpal::BufferSize::Fixed(match sc.buffer_size() {
            cpal::SupportedBufferSize::Range { min, max } => cfg.buffer.clamp(*min, (*max).max(*min)),
            cpal::SupportedBufferSize::Unknown => cfg.buffer,
        });
    }
    let shared = out.shared.clone();
    let err = move |e: cpal::StreamError| shared.fail(format!("The audio device stopped ({e})"));
    let mut buf = vec![0.0f32; 1 << 15];
    let stream = match fmt {
        cpal::SampleFormat::F32 => dev.build_output_stream(&config, move |d: &mut [f32], i: &_| out.render(d, ch, latency(i)), err, None),
        cpal::SampleFormat::I16 => dev.build_output_stream(
            &config,
            move |d: &mut [i16], i: &_| {
                let f = scratch(&mut buf, d.len());
                out.render(f, ch, latency(i));
                d.iter_mut().zip(f.iter()).for_each(|(o, s)| *o = (s.clamp(-1.0, 1.0) * 32767.0) as i16);
            },
            err,
            None,
        ),
        cpal::SampleFormat::U16 => dev.build_output_stream(
            &config,
            move |d: &mut [u16], i: &_| {
                let f = scratch(&mut buf, d.len());
                out.render(f, ch, latency(i));
                d.iter_mut().zip(f.iter()).for_each(|(o, s)| *o = (s.clamp(-1.0, 1.0) * 32767.0 + 32768.0) as u16);
            },
            err,
            None,
        ),
        f => return Err(fail(&format!("{f:?} samples are not supported"))),
    }
    .map_err(|e| fail(&e))?;
    stream.play().map_err(|e| fail(&e))?;
    Ok((Box::new(stream), rate))
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
    watch: (u64, Instant),
    stop: Arc<AtomicBool>,
    /// The plan the mixer reads once per block, and the content hash, bypass and quickfade it was compiled with.
    slot: Arc<Mutex<Arc<AudioPlan>>>,
    hash: (u64, bool, bool, bool, bool),
    /// The play rate (f64 bits): 1, or a shuttle's, which `set_rate` changes on the fly.
    rate: Arc<AtomicU64>,
    serial: u64,
    /// Where it started, and whether what is heard has got there (until then the playhead waits at the start).
    from: f64,
    reached: bool,
    /// The playhead last given (kept when no stamp can be read).
    last: f64,
}

impl Session {
    /// The playhead from the device's stamp (None: none of this session's yet, or a read raced the callback).
    fn heard(&mut self, h: Option<Heard>) -> f64 {
        if let Some(h) = h.filter(|h| h.serial == self.serial) {
            self.last = playhead(&mut self.reached, self.from, h.at(now_ns()), f64::from_bits(self.rate.load(Relaxed)));
        }
        self.last
    }
}

/// The playhead of a session that started at `from`, playing at `rate`, for heard time `t`: until the first sound is
/// heard (`reached`) it waits at the start, never behind it.
fn playhead(reached: &mut bool, from: f64, t: f64, rate: f64) -> f64 {
    *reached |= (t - from) * rate.signum() >= 0.0;
    if *reached { t } else { from }
}

/// An open output device: the channels to its callback and what it shares.
struct Device {
    feeds: SyncSender<Feed>,
    mons: SyncSender<MonMsg>,
    bin: Receiver<Trash>,
    shared: Arc<Shared>,
    rate: u32,
}

/// The channels of a device being opened, until it reports its rate.
type Pending = (SyncSender<Feed>, SyncSender<MonMsg>, Receiver<Trash>, Arc<Shared>);

pub struct Player {
    /// False in the screenshot harness: no device is ever opened.
    devices: bool,
    config: DeviceConfig,
    device_rx: Receiver<Result<u32, String>>,
    pending: Option<Pending>,
    device: Option<Result<Device, String>>,
    /// Dropping it lets the device thread drop its stream.
    release: Option<Sender<()>>,
    opened: Instant,
    reopen_at: Option<Instant>,
    /// The device watchdog while no session runs (a session has its own).
    watch: (u64, Instant),
    session: Option<Session>,
    serial: u64,
    error: Arc<Mutex<Option<String>>>,
    /// Master fader and meter tap (§5.12).
    pub master: Arc<Master>,
    /// Options › Bypass All Audio FX: the plans this player compiles skip every audio plug-in.
    pub bypass_fx: bool,
    /// Options › Quickfade Audio Edits.
    pub quickfade: bool,
    /// R17b: multicamera editing with Edit Multicamera Audio with Video: the plans keep the multicamera audio's other
    /// takes decoding, so a take switched to while playing is heard at once.
    pub standby: bool,
    /// R18a: Options › Metronome: the plans this player compiles click on the beats.
    pub metronome: bool,
    /// R8a: the level meters of the tracks and busses in the last plan, by id.
    pub meters: Arc<Meters>,
    /// R9: the recording input the monitor plays (the recorder pushes it), what it plays, and the input busses' meters.
    pub ring: Arc<Ring>,
    mon: MonPlan,
    pub mon_meters: Arc<Meters>,
    /// R16c: the master output the Loudness Meters measure (`Out::loud`).
    pub loud: Arc<Ring>,
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Player {
    /// A player on the output device `config` names; with `devices` false (the screenshot harness) it never opens one.
    pub fn new(devices: bool, config: DeviceConfig) -> Self {
        let mut p = Self {
            devices,
            config,
            device_rx: mpsc::channel().1,
            pending: None,
            device: None,
            release: None,
            opened: Instant::now(),
            reopen_at: None,
            watch: (0, Instant::now()),
            session: None,
            serial: 0,
            error: Arc::new(Mutex::new(None)),
            master: Arc::new(Master::new()),
            bypass_fx: false,
            quickfade: true,
            standby: false,
            metronome: false,
            meters: Arc::default(),
            ring: Arc::default(),
            mon: MonPlan::default(),
            mon_meters: Arc::default(),
            loud: Arc::default(),
        };
        p.open();
        p
    }

    /// Open the output device. Opening can take a while and a stream may not leave its thread, so it lives on its own
    /// thread, which holds it until `release` is dropped.
    fn open(&mut self) {
        if !self.devices {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let (release, released) = mpsc::channel::<()>();
        let (feeds_tx, feeds) = mpsc::sync_channel(4);
        let (mons_tx, mons) = mpsc::sync_channel(8);
        let (bin, bin_rx) = mpsc::sync_channel(256);
        let shared = Arc::new(Shared::default());
        let mon = Box::new(MonState::new(Arc::new(self.mon.clone())));
        let out = Out::new(feeds, mons, bin, self.master.clone(), shared.clone(), self.ring.clone(), self.loud.clone(), mon);
        let cfg = self.config.clone();
        std::thread::Builder::new()
            .name("milan-audio-device".into())
            .spawn(move || match open_output(&cfg, out) {
                Ok((stream, rate)) => {
                    if tx.send(Ok(rate)).is_ok() {
                        let _ = released.recv(); // returns once `release` is dropped
                    }
                    drop(stream);
                }
                Err(e) => {
                    let _ = tx.send(Err(format!("{e}: playback is silent")));
                }
            })
            .expect("spawn milan-audio-device");
        (self.device_rx, self.device, self.release, self.opened) = (rx, None, Some(release), Instant::now());
        self.pending = Some((feeds_tx, mons_tx, bin_rx, shared));
    }

    /// Release the output device and open it again (at most every 2 s), e.g. after headphones were unplugged.
    pub fn reopen(&mut self) {
        self.stop();
        // A fresh receiver, so a still-pending open of the old device is ignored.
        (self.device_rx, self.device, self.release, self.pending) = (mpsc::channel().1, None, None, None);
        self.reopen_at = Some((self.opened + REOPEN_EVERY).max(Instant::now()));
    }

    /// R9: Preferences › Audio Device changed: the output device is opened again with `config`.
    pub fn configure(&mut self, config: DeviceConfig) {
        if config != self.config {
            self.config = config;
            *self.error.lock().unwrap_or_else(|p| p.into_inner()) = None;
            self.reopen();
            self.reopen_at = Some(Instant::now());
        }
    }

    fn poll_device(&mut self) {
        if self.reopen_at.is_some_and(|t| Instant::now() >= t) {
            self.reopen_at = None;
            self.open();
        }
        if self.device.is_none() {
            self.device = match self.device_rx.try_recv() {
                Ok(Ok(rate)) => self.pending.take().map(|(feeds, mons, bin, shared)| {
                    if self.session.is_none() {
                        *self.error.lock().unwrap_or_else(|p| p.into_inner()) = None; // the device is back
                    }
                    self.watch = (shared.calls.load(Relaxed), Instant::now());
                    Ok(Device { feeds, mons, bin, shared, rate })
                }),
                Ok(Err(e)) => Some(Err(e)),
                Err(_) => None,
            };
        }
    }

    /// Each frame: the device opens, what its callback let go of is freed, and a device that failed or stopped asking
    /// for samples while nothing plays is reopened (a playing session is watched by the app, `stalled`).
    pub fn tick(&mut self) {
        self.poll_device();
        if let Some(Err(e)) = &self.device {
            // No device (or it is in use): tried again every 2 s, so one plugged in is found; the message stays.
            *self.error.lock().unwrap_or_else(|p| p.into_inner()) = Some(e.clone());
            self.reopen();
            return;
        }
        let Some(Ok(d)) = &self.device else { return };
        d.bin.try_iter().for_each(drop);
        let (lost, calls) = (d.shared.lost.load(Relaxed), d.shared.calls.load(Relaxed));
        if lost {
            let why = d.shared.why.lock().ok().and_then(|w| w.clone());
            *self.error.lock().unwrap_or_else(|p| p.into_inner()) = why;
        }
        if lost || (self.session.is_none() && stalled(&mut self.watch, calls, Instant::now())) {
            self.reopen();
        }
    }

    /// Play the timeline's audio from `from`, looping over `lp` (seconds) inside the mixer if set, at `rate` (1, or a
    /// J / L shuttle's: then varispeed, backwards below 0, and without a loop).
    /// Without a device no session starts, and `position()` stays None so the caller keeps its own clock.
    pub fn start(&mut self, ctx: &egui::Context, project: &Project, from: f64, lp: Option<(f64, f64)>, rate: f64) {
        self.stop();
        self.poll_device();
        let feeds = match &self.device {
            Some(Ok(d)) => d.feeds.clone(),
            Some(Err(e)) => {
                // Keep the message while the device is tried again.
                *self.error.lock().unwrap() = Some(e.clone());
                self.reopen();
                return;
            }
            None => return,
        };
        *self.error.lock().unwrap() = None;
        let start = from;
        let from = frame(from);
        let (tx, rx) = mpsc::sync_channel(QUEUE);
        let clock = Arc::new(AtomicI64::new(from * 2));
        let stop = Arc::new(AtomicBool::new(false));
        self.serial += 1;
        let feed = Feed { rx, buf: Vec::new(), pos: 0, f0: from, rate, clock, stop: stop.clone(), serial: self.serial };
        if feeds.try_send(feed).is_err() {
            // The callback has not taken the last ones: it is not running.
            *self.error.lock().unwrap() = Some("Audio output failed: the device is not playing".into());
            self.reopen();
            return;
        }
        let slot = Arc::new(Mutex::new(Arc::new(self.plan(project))));
        let lp = lp.map(|(a, b)| (frame(a), frame(b)));
        let rate = Arc::new(AtomicU64::new(rate.to_bits()));
        let (s, r, quit, error, ctx) = (slot.clone(), rate.clone(), stop.clone(), self.error.clone(), ctx.clone());
        std::thread::Builder::new()
            .name("milan-mix".into())
            .spawn(move || {
                mix::run(&s, Mixer::ffmpeg(), from, lp, &r, &tx, &quit, |e| {
                    *error.lock().unwrap() = Some(e);
                    ctx.request_repaint();
                })
            })
            .expect("spawn milan-mix");
        let hash = (crate::engine::content_hash(project), self.bypass_fx, self.quickfade, self.standby, self.metronome);
        let calls = self.calls();
        self.session = Some(Session { watch: (calls, Instant::now()), stop, slot, hash, rate, serial: self.serial, from: start, reached: false, last: start });
    }

    fn calls(&self) -> u64 {
        match &self.device {
            Some(Ok(d)) => d.shared.calls.load(Relaxed),
            _ => 0,
        }
    }

    /// Change a running shuttle's rate (heard within the device queue); false when there is no shuttle to change, as
    /// normal playback cannot become one.
    pub fn set_rate(&mut self, rate: f64) -> bool {
        let Some(s) = &self.session else { return false };
        let old = f64::from_bits(s.rate.load(Relaxed));
        let ok = old != 1.0 && rate != 1.0;
        if ok {
            s.rate.store(rate.to_bits(), Relaxed);
        }
        ok
    }

    /// The plan, with a meter for each of its tracks and busses (the ones they had kept, so a reader holds on).
    fn plan(&mut self, project: &Project) -> AudioPlan {
        let mut plan = AudioPlan { bypass_fx: self.bypass_fx, quickfade: if self.quickfade { QUICKFADE } else { 0.0 }, standby: self.standby, ..compile_audio(project) };
        plan.metronome = self.metronome.then(|| mix::Metronome::of(&project.settings));
        let ids = plan.tracks.iter().map(|t| &t.id).chain(plan.buses.iter().map(|b| &b.id));
        self.meters = Arc::new(ids.map(|id| (id.clone(), self.meters.get(id).cloned().unwrap_or_default())).collect());
        plan.meters = Some(self.meters.clone());
        plan
    }

    /// While playing: give the mixer the project's new plan after an edit (or a Bypass All Audio FX toggle). It is
    /// heard within about 150 ms.
    pub fn set_plan(&mut self, project: &Project) {
        let hash = (crate::engine::content_hash(project), self.bypass_fx, self.quickfade, self.standby, self.metronome);
        if self.session.as_ref().is_none_or(|s| s.hash == hash) {
            return;
        }
        let plan = Arc::new(self.plan(project));
        if let Some(s) = &mut self.session {
            (s.hash, *s.slot.lock().unwrap()) = (hash, plan);
        }
    }

    /// R9: what the input monitor plays from now on (each frame; only a change reaches the callback). The callback keeps
    /// its plug-ins' state when only values changed.
    pub fn set_monitor(&mut self, mut plan: MonPlan) {
        if plan == self.mon {
            return;
        }
        // Input busses are metered here; a monitored track keeps its playback meter.
        let ids: Vec<&String> = plan.paths.iter().filter(|p| p.input).map(|p| &p.id).collect();
        if !ids.iter().all(|id| self.mon_meters.contains_key(*id)) || ids.len() != self.mon_meters.len() {
            self.mon_meters = Arc::new(ids.iter().map(|id| ((*id).clone(), self.mon_meters.get(*id).cloned().unwrap_or_default())).collect());
        }
        for p in &mut plan.paths {
            p.meter = self.mon_meters.get(&p.id).cloned();
        }
        let same = plan.same_chains(&self.mon);
        // To the open device's callback, or the one being opened (it starts from the plan it was opened with).
        let to = match (&self.device, &self.pending) {
            (Some(Ok(d)), _) => Some(&d.mons),
            (None, Some(p)) => Some(&p.1),
            _ => None,
        };
        if let Some(to) = to {
            let plan = Arc::new(plan.clone());
            let msg = if same { MonMsg::Plan(plan) } else { MonMsg::State(Box::new(MonState::new(plan))) };
            if let Err(mpsc::TrySendError::Full(_)) = to.try_send(msg) {
                return; // the callback has not taken the last ones: tried again next frame
            }
        }
        self.mon = plan;
    }

    pub fn stop(&mut self) {
        if let Some(s) = self.session.take() {
            s.stop.store(true, Relaxed);
        }
        self.master.clear_rms();
    }

    /// A session is running (it may be stalled).
    pub fn running(&self) -> bool {
        self.session.is_some()
    }

    /// R9: the input monitor plays something (with an open device).
    pub fn monitoring(&self) -> bool {
        matches!(self.device, Some(Ok(_))) && !self.mon.paths.is_empty()
    }

    /// The device has stopped asking for samples for 400 ms: the session should be stopped and the device reopened.
    pub fn stalled(&mut self) -> bool {
        let calls = self.calls();
        let lost = matches!(&self.device, Some(Ok(d)) if d.shared.lost.load(Relaxed));
        self.session.as_mut().is_some_and(|s| lost || stalled(&mut s.watch, calls, Instant::now()))
    }

    /// The timeline time heard now, while a session runs and the device is alive. R9: the clock less the output latency
    /// the device reports (VEGAS's cursor is what is heard), carried on between callbacks.
    pub fn position(&mut self) -> Option<f64> {
        if self.stalled() {
            return None;
        }
        let stamp = match &self.device {
            Some(Ok(d)) => d.shared.stamp.read(),
            _ => None,
        };
        self.session.as_mut().map(|s| s.heard(stamp))
    }

    /// R9: the output latency (device plus buffer), the input latency and the input monitor's round trip (input,
    /// ring, output), in ms, as last measured; None before the device runs.
    pub fn latency_ms(&self) -> Option<(f64, f64, f64)> {
        let Some(Ok(d)) = &self.device else { return None };
        let out = d.shared.out_us.load(Relaxed) as f64 / 1000.0;
        let inp = self.ring.latency_us.load(Relaxed) as f64 / 1000.0;
        let fill = d.shared.fill_us.load(Relaxed) as f64 / 1000.0;
        (out > 0.0).then_some((out, inp, inp + fill + out))
    }

    pub fn status(&mut self) -> Option<String> {
        self.poll_device();
        match &self.device {
            Some(Err(e)) => return Some(e.clone()),
            Some(Ok(d)) if d.shared.mismatch.load(Relaxed) => {
                let r = self.ring.rate.load(Relaxed);
                return Some(format!("Input monitoring is off: the recording device runs at {r} Hz and the playback device at {} Hz", d.rate));
            }
            _ => {}
        }
        self.error.lock().unwrap().clone()
    }
}

// ---- Normalize ----

/// Switches › Normalize: measures the peak of each audio event that needs its Normalize gain (`edit::needs_normalize`)
/// on a thread of its own, and sets the gain when it arrives. Not an undo step of its own: the switch's edit is.
pub struct Normalizer {
    /// To the measuring thread, one event at a time (a Select All and Normalize is not a hundred decoders at once).
    tx: Sender<(String, NormSpan, crate::engine::plan::ASrc, f64, egui::Context)>,
    rx: Receiver<(String, NormSpan, f32)>,
    /// Events being measured.
    pending: std::collections::HashSet<String>,
}

impl Normalizer {
    pub fn new() -> Self {
        let (tx, req) = mpsc::channel::<(String, NormSpan, crate::engine::plan::ASrc, f64, egui::Context)>();
        let (done, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for (id, span, src, len, ctx) in req {
                // Unreadable audio measures as silence (no gain), so it is not tried again every frame.
                let peak = mix::peak(&src, len, span.channels).unwrap_or(0.0);
                if done.send((id, span, peak)).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        Self { tx, rx, pending: Default::default() }
    }

    /// Set the gains measured since the last call (on events still playing that audio) and start measuring the events
    /// that need it. True when the project changed.
    pub fn tick(&mut self, ctx: &egui::Context, p: &mut Project) -> bool {
        let mut done = false;
        for (id, span, peak) in self.rx.try_iter() {
            self.pending.remove(&id);
            let Some(r) = p.find_clip(&id) else { continue };
            let c = &mut p.timeline.tracks_mut(r.is_video)[r.track_idx].clips[r.clip_idx];
            // An event edited while it was measured is measured again.
            if edit::needs_normalize(c) && edit::norm_span(c) == span {
                c.normalized = Some(Normalized { db: edit::normalize_db(peak), span });
                done = true;
            }
        }
        let pending = &mut self.pending;
        for c in p.timeline.audio_tracks.iter().flat_map(|t| &t.clips).filter(|c| edit::needs_normalize(c)) {
            if pending.contains(&c.id) {
                continue;
            }
            let Some((src, len)) = crate::engine::plan::norm_source(p, c) else { continue };
            pending.insert(c.id.clone());
            let _ = self.tx.send((c.id.clone(), edit::norm_span(c), src, len, ctx.clone()));
        }
        if done {
            p.mark_modified();
        }
        done
    }
}

// ---- Waveforms ----

/// Audio channels of audio stream `stream`, if ffprobe can tell.
pub(crate) fn channel_count(path: &Path, stream: u32) -> Option<usize> {
    let mut cmd = crate::proc::ffprobe();
    cmd.args(["-v", "error", "-select_streams", &format!("a:{stream}"), "-show_entries", "stream=channels", "-of", "csv=p=0"]).arg(path);
    crate::proc::output(&mut cmd).ok().and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
}

/// Peaks per 1/PEAKS_PER_SEC s of the source, 0..=1, one list per channel: stereo (or more, folded to 2) gives
/// L and R, as VEGAS draws two stacked waveforms; mono gives one.
// shortcut: the first audio stream only; an event playing another (Stream ▸) draws the first one's waveform. Key the
// cache by stream if multi-stream files become common.
fn peaks(path: &Path) -> Vec<Vec<f32>> {
    let ch = channel_count(path, 0).unwrap_or(1).clamp(1, 2);
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
    /// (cache key, file, R6b: a nested timeline: its mix's peaks, the generation asked in)
    tx: Sender<(String, PathBuf, bool, u64)>,
    rx: Receiver<(String, Vec<Vec<f32>>, u64)>,
    /// R18a: Rebuild Audio Peaks starts a new generation; peaks read for an older one are dropped.
    generation: u64,
    /// Set while the timeline plays, so peak decodes leave the CPU to playback.
    paused: Arc<AtomicBool>,
}

impl Waveforms {
    pub fn new(ctx: &egui::Context) -> Self {
        let (tx, req_rx) = mpsc::channel::<(String, PathBuf, bool, u64)>();
        let (res_tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let paused = Arc::new(AtomicBool::new(false));
        let p = paused.clone();
        std::thread::spawn(move || {
            for (id, path, nested, generation) in req_rx {
                while p.load(Relaxed) {
                    std::thread::sleep(Duration::from_millis(50));
                }
                let read = || if nested { crate::engine::nested::peaks(&path, PEAKS_PER_SEC) } else { peaks(&path) };
                if res_tx.send((id, retry_empty(read), generation)).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        Self { cache: HashMap::new(), tx, rx, paused, generation: 0 }
    }

    pub fn set_paused(&self, on: bool) {
        self.paused.store(on, Relaxed);
    }

    /// Some requested peaks have not arrived yet.
    pub fn pending(&mut self) -> bool {
        self.receive();
        self.cache.values().any(|v| v.is_none())
    }

    fn receive(&mut self) {
        for (id, p, g) in self.rx.try_iter() {
            if g == self.generation {
                self.cache.insert(id, Some(p));
            }
        }
    }

    /// R18a: View › Rebuild Audio Peaks (every media, `None`), or a file changed by the audio editor (media `Some(id)`):
    /// their peaks are read again when next drawn.
    pub fn rebuild(&mut self, media: Option<&str>) {
        self.generation += 1;
        // Peaks still being read for the old generation are asked for again too.
        self.cache.retain(|k, v| v.is_some() && media.is_some_and(|id| k.split('|').next() != Some(id)));
    }

    /// Peaks for `m`, one list per channel (1 or 2), requesting them on first use.
    pub fn get(&mut self, m: &MediaItem) -> Option<&[Vec<f32>]> {
        if m.generator.is_some() {
            return None; // generated media has no sound and no file to read
        }
        self.receive();
        // R6b: a nested timeline's peaks are its mix's, made again once it is saved.
        let key = if m.nested { format!("{}|{:?}", m.id, crate::engine::nested::version(&m.path)) } else { m.id.clone() };
        if !self.cache.contains_key(&key) {
            self.cache.insert(key.clone(), None);
            let _ = self.tx.send((key.clone(), m.path.clone(), m.nested, self.generation));
        }
        self.cache[&key].as_deref().filter(|p| p.first().is_some_and(|c| !c.is_empty()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R18a: Rebuild Audio Peaks forgets every media's peaks, or one media's (a nested timeline's keys too), and those
    /// still being read.
    #[test]
    fn rebuild_forgets_peaks() {
        let mut w = Waveforms::new(&egui::Context::default());
        for (k, v) in [("a", Some(vec![vec![0.5]])), ("b|7", Some(vec![vec![0.5]])), ("c", None)] {
            w.cache.insert(k.into(), v);
        }
        w.rebuild(Some("b"));
        assert!(w.cache.contains_key("a") && !w.cache.contains_key("b|7") && !w.cache.contains_key("c") && w.generation == 1);
        w.rebuild(None);
        assert!(w.cache.is_empty());
    }

    /// The real default output device (shared mode) plays an empty timeline: inaudible (silence, master at 0), and the
    /// heard clock must advance in step with the wall clock. Never opens an input or exclusive mode.
    /// cargo test --release live_device_plays_silence -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_device_plays_silence() {
        let ctx = egui::Context::default();
        let mut p = Player::new(true, DeviceConfig::default());
        p.master.set_gain([0.0, 0.0]);
        let t0 = Instant::now();
        while p.latency_ms().is_none() && t0.elapsed() < Duration::from_secs(5) {
            p.tick();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(p.status().is_none(), "device error: {:?}", p.status());
        p.start(&ctx, &Project::new("silence"), 0.0, None, 1.0);
        let start = Instant::now();
        let mut last = None;
        while start.elapsed() < Duration::from_millis(2000) {
            p.tick();
            last = p.position().or(last);
            std::thread::sleep(Duration::from_millis(20));
        }
        let heard = last.expect("the clock never moved");
        println!("latency {:?} ms; heard {heard:.3} s after 2.0 s", p.latency_ms());
        assert!(!p.stalled() && (heard - 2.0).abs() < 0.4, "heard {heard}");
        p.stop();
    }

    /// An output callback at device `rate` and the channels that feed it, as `Player::open` makes them.
    struct Rig {
        out: Out,
        feeds: SyncSender<Feed>,
        mons: SyncSender<MonMsg>,
        master: Arc<Master>,
        loud: Arc<Ring>,
        _bin: Receiver<Trash>,
    }

    fn rig(rate: u32) -> Rig {
        let (feeds, frx) = mpsc::sync_channel(4);
        let (mons, mrx) = mpsc::sync_channel(8);
        let (bin, bin_rx) = mpsc::sync_channel(1024);
        let (master, loud) = (Arc::new(Master::new()), Arc::new(Ring::default()));
        let mut out = Out::new(frx, mrx, bin, master.clone(), Arc::default(), Arc::default(), loud.clone(), Box::new(MonState::new(Arc::default())));
        out.set_rate(rate);
        Rig { out, feeds, mons, master, loud, _bin: bin_rx }
    }

    /// R16c: the loudness tap is the mix after the master fader and before Dim and Downmix, only while a session plays,
    /// each session's frames after a NaN reset frame; at the device rate.
    #[test]
    fn loudness_tap_is_the_master_before_dim_and_downmix() {
        let mut r = rig(44_100);
        let mut got = Vec::new();
        pull(&mut r, 4);
        r.loud.take(&mut got);
        assert!(got.is_empty(), "nothing plays: {got:?}");
        assert_eq!(r.loud.rate.load(Relaxed), 44_100);
        let mut r = rig(RATE);
        r.master.set_gain([0.5, 0.5]);
        let (tx, ..) = session(&r, 1);
        tx.send((0, 1.0, [0.8, 0.2].repeat(8))).unwrap();
        (r.master.dim.store(true, Relaxed), r.master.mono.store(true, Relaxed));
        pull(&mut r, 3);
        r.loud.take(&mut got);
        assert!(got[0].is_nan() && got[1].is_nan(), "{got:?}");
        assert_eq!(&got[2..], [0.4, 0.1].repeat(3));
    }

    /// A playback session on `r`: its block sender, clock and stop flag.
    fn session(r: &Rig, serial: u64) -> (SyncSender<Block>, Arc<AtomicI64>, Arc<AtomicBool>) {
        let (tx, rx) = mpsc::sync_channel(8);
        let (clock, stop) = (Arc::new(AtomicI64::new(0)), Arc::new(AtomicBool::new(false)));
        r.feeds.send(Feed { rx, buf: Vec::new(), pos: 0, f0: 0, rate: 1.0, clock: clock.clone(), stop: stop.clone(), serial }).unwrap();
        (tx, clock, stop)
    }

    /// `n` stereo frames from the callback.
    fn pull(r: &mut Rig, n: usize) -> Vec<f32> {
        let mut b = vec![0.0; 2 * n];
        r.out.render_at(&mut b, 2, Duration::ZERO, 0);
        b
    }

    #[test]
    fn mute_and_solo_decide_what_plays() {
        let (tl, mut t) = (crate::project::Timeline::new(), crate::project::Track::new("a", crate::project::TrackKind::Audio));
        assert!(tl.plays(&t, false));
        assert!(!tl.plays(&t, true)); // another track is solo
        t.solo = true;
        assert!(tl.plays(&t, true));
        t.muted = true;
        assert!(!tl.plays(&t, true)); // mute beats solo
    }

    #[test]
    fn master_gain_and_peak_tap() {
        let mut r = rig(RATE);
        r.master.set_gain([0.5, 0.0]);
        r.out.gain = [0.5, 0.0];
        let (tx, ..) = session(&r, 1);
        tx.send((0, 1.0, vec![0.8, 0.8, -1.0, 0.2])).unwrap();
        assert_eq!(pull(&mut r, 2), [0.4, 0.0, -0.5, 0.0]);
        assert_eq!(r.master.take_peaks(), [0.5, 0.0]);
        assert_eq!(r.master.take_peaks(), [0.0, 0.0]); // reading resets
    }

    /// A master fader move glides over a few milliseconds instead of stepping on one sample (a click).
    #[test]
    fn master_gain_changes_glide() {
        let mut r = rig(RATE);
        let (tx, ..) = session(&r, 1);
        tx.send((0, 1.0, vec![1.0; 2 * RATE as usize / 50])).unwrap();
        assert_eq!(pull(&mut r, 1), [1.0, 1.0]);
        r.master.set_gain([0.0, 0.0]);
        let out: Vec<f32> = pull(&mut r, RATE as usize / 50 - 1).into_iter().step_by(2).collect();
        assert!(out[0] > 0.9, "stepped: {}", out[0]);
        assert!(out.windows(2).all(|w| w[1] <= w[0]));
        assert!(*out.last().unwrap() < 1e-3, "{}", out.last().unwrap());
    }

    /// R8b: Dim Output lowers the output 20 dB and Downmix Output plays (L + R) / 2 on both sides, each gliding in.
    #[test]
    fn dim_and_downmix_glide_in() {
        let mut r = rig(RATE);
        let (tx, ..) = session(&r, 1);
        tx.send((0, 1.0, [0.8, 0.2].repeat(RATE as usize / 10))).unwrap();
        (r.master.dim.store(true, Relaxed), r.master.mono.store(true, Relaxed));
        let out = pull(&mut r, RATE as usize / 10);
        assert!(out[0] > 0.5, "stepped: {}", out[0]);
        let end = &out[out.len() - 2..];
        assert!(end.iter().all(|v| (v - 0.05).abs() < 1e-4), "{end:?}");
        // RMS of each 50 ms sent: the last window is all 0.05.
        assert!(r.master.rms().iter().all(|x| (x - 0.05).abs() < 1e-3), "{:?}", r.master.rms());
        r.master.clear_rms();
        assert_eq!(r.master.rms(), [0.0, 0.0]);
    }

    #[test]
    fn feed_clock_follows_block_starts() {
        let mut r = rig(RATE);
        let (tx, clock, stop) = session(&r, 1);
        tx.send((100, 1.0, vec![0.1; 4])).unwrap();
        tx.send((10, 1.0, vec![0.2; 4])).unwrap(); // a loop wrap back to frame 10
        let frames = || clock.load(Relaxed) as f64 / 2.0;
        pull(&mut r, 2);
        assert_eq!(frames(), 102.0);
        pull(&mut r, 1);
        assert_eq!(frames(), 11.0);
        pull(&mut r, 1);
        assert_eq!(frames(), 12.0);
        // Underrun silence and a finished mixer leave the clock where it is but still count as calls.
        drop(tx);
        assert_eq!(pull(&mut r, 2), [0.0; 4]);
        assert_eq!(frames(), 12.0);
        assert_eq!(r.out.shared.calls.load(Relaxed), 4);
        stop.store(true, Relaxed);
        pull(&mut r, 1);
        assert!(r.out.feed.is_none(), "a stopped session leaves the callback");
    }

    /// R4b: shuttle 2x for 1 s advances the clock 2 s; backwards it runs back.
    #[test]
    fn shuttle_clock_advances_by_the_rate() {
        let mut r = rig(RATE);
        let (tx, clock, _) = session(&r, 1);
        tx.send((RATE as i64, 2.0, vec![0.0; 2 * RATE as usize])).unwrap();
        tx.send((3 * RATE as i64, -4.0, vec![0.0; 2 * RATE as usize / 4])).unwrap();
        pull(&mut r, RATE as usize);
        assert_eq!(clock_secs(clock.load(Relaxed)), 3.0);
        pull(&mut r, RATE as usize / 4);
        assert_eq!(clock_secs(clock.load(Relaxed)), 2.0);
    }

    /// R9: a 44.1 kHz device plays the 48 kHz mix in time (linear interpolation, as rodio did): one second of device
    /// frames takes one second of the mix, and a constant stays constant.
    #[test]
    fn a_44_1_khz_device_plays_the_mix_in_time() {
        let mut r = rig(44_100);
        let (tx, clock, _) = session(&r, 1);
        tx.send((0, 1.0, vec![0.5; 2 * RATE as usize + 8])).unwrap();
        let out = pull(&mut r, 44_100);
        assert!(out[4..].iter().all(|v| (v - 0.5).abs() < 1e-6));
        assert!((clock_secs(clock.load(Relaxed)) - 1.0).abs() < 2.0 / RATE as f64, "{}", clock_secs(clock.load(Relaxed)));
        // A mono device gets (L + R) / 2; a 5.1 one L and R in front and silence elsewhere.
        let mut m = vec![0.0; 4];
        r.out.render_at(&mut m, 1, Duration::ZERO, 0);
        let mut six = vec![1.0; 12];
        r.out.render_at(&mut six, 6, Duration::ZERO, 0);
        assert!(m.iter().all(|v| (v - 0.5).abs() < 1e-6) && six[..2].iter().all(|v| (v - 0.5).abs() < 1e-6) && six[2..6] == [0.0; 4]);
    }

    /// R9 acceptance: the playhead is the clock less the output latency the device reports, carried on between
    /// callbacks; an underrun holds it; before the first sound the playhead waits at the start.
    #[test]
    fn the_playhead_follows_what_is_heard() {
        let mut r = rig(RATE);
        let (tx, ..) = session(&r, 7);
        tx.send((0, 1.0, vec![0.0; 2 * 960])).unwrap(); // two callbacks' worth
        let ms = |v: u64| v * 1_000_000;
        let mut b = vec![0.0; 2 * 480];
        r.out.render_at(&mut b, 2, Duration::from_millis(20), ms(1000));
        let h = r.out.shared.stamp.read().unwrap();
        assert_eq!((h.serial, h.rate), (7, 1.0));
        assert!((h.at(ms(1000)) + 0.020).abs() < 1e-9, "{}", h.at(ms(1000)));
        assert!((h.at(ms(1005)) + 0.015).abs() < 1e-9);
        assert!(h.at(ms(2000)).abs() < 1e-9, "carried on no further than the buffer's first frame");
        // The next callback 10 ms later agrees with where the last one said it would be.
        r.out.render_at(&mut b, 2, Duration::from_millis(20), ms(1010));
        assert!((r.out.shared.stamp.read().unwrap().at(ms(1010)) + 0.010).abs() < 1e-9);
        assert_eq!(r.out.shared.out_us.load(Relaxed), 30_000);
        // The mixer falls behind: what is heard stops moving.
        drop(tx);
        r.out.render_at(&mut b, 2, Duration::from_millis(20), ms(1020));
        r.out.render_at(&mut b, 2, Duration::from_millis(20), ms(1030));
        let h = r.out.shared.stamp.read().unwrap();
        assert_eq!(h.at(ms(1030)), h.at(ms(1040)));
        let mut reached = false;
        assert_eq!(playhead(&mut reached, 1.0, 0.98, 1.0), 1.0);
        assert_eq!(playhead(&mut reached, 1.0, 1.001, 1.0), 1.001);
        assert_eq!(playhead(&mut reached, 1.0, 0.5, 1.0), 0.5, "after a loop wrap it follows");
        let mut back = false;
        assert_eq!(playhead(&mut back, 1.0, 1.02, -2.0), 1.0, "backwards it waits above the start");
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
    fn the_input_ring_keeps_order_and_drops_what_does_not_fit() {
        let ring = Ring::default();
        let frames = |a: usize, b: usize| (a..b).flat_map(|k| [k as f32, -(k as f32)]).collect::<Vec<f32>>();
        ring.push(&frames(0, 3));
        let mut out = vec![9.0; 8];
        ring.pop(&mut out);
        assert_eq!(out, [0.0, -0.0, 1.0, -1.0, 2.0, -2.0, 0.0, 0.0]);
        // Across the wrap, and full: the newest frames that do not fit are dropped.
        ring.push(&frames(3, RING + 10));
        assert_eq!(ring.len(), RING);
        ring.keep(2);
        let mut out = vec![0.0; 4];
        ring.pop(&mut out);
        assert_eq!(out, [(RING + 1) as f32, -((RING + 1) as f32), (RING + 2) as f32, -((RING + 2) as f32)]);
        assert_eq!(ring.len(), 0);
    }

    /// R9: which inputs are heard: armed tracks by their Input Monitoring mode (Auto: stopped or recording), through
    /// their volume and routing; unmuted input busses.
    #[test]
    fn the_monitor_plays_armed_tracks_and_input_busses() {
        use crate::project::Monitor;
        let mut p = Project::new("t");
        let paths = |p: &Project, playing, rec| MonPlan::new(p, playing, rec, 0.0).paths.into_iter().map(|x| (x.id, x.m)).collect::<Vec<_>>();
        assert!(paths(&p, false, false).is_empty());
        edit::set_bus_count(&mut p, 1);
        let tr = &mut p.timeline.audio_tracks[0];
        (tr.armed, tr.monitor, tr.volume) = (true, Monitor::Auto, 0.5);
        let id = tr.id.clone();
        assert_eq!(paths(&p, false, false), [(id.clone(), [[0.5, 0.0], [0.0, 0.5]])]);
        assert!(paths(&p, true, false).is_empty(), "Auto: playback plays the track");
        assert_eq!(paths(&p, true, true).len(), 1, "Auto: heard while recording");
        p.timeline.audio_tracks[0].monitor = Monitor::On;
        assert_eq!(paths(&p, true, false).len(), 1);
        p.timeline.audio_tracks[0].armed = false;
        assert!(paths(&p, false, false).is_empty());
        // An input bus to Bus A (-6 dB): heard at that gain; muted, not at all.
        let bid = edit::insert_input_bus(&mut p).unwrap();
        let a = p.timeline.audio.buses[0].id.clone();
        p.timeline.audio.buses[0].volume = 0.5;
        assert_eq!(edit::set_bus_out(&mut p, &bid, &Route::Bus(a)), Ok(true));
        assert_eq!(paths(&p, true, false), [(bid.clone(), [[0.5, 0.0], [0.0, 0.5]])]);
        p.timeline.audio.inputs[0].muted = true;
        assert!(paths(&p, false, false).is_empty());
        assert_eq!(p.timeline.audio.inputs[0].name, "Input Bus A");
        assert!(edit::delete_input_bus(&mut p, &bid) && p.timeline.audio.inputs.is_empty());
    }

    /// R9: the input reaches the output through the monitor (its FX and gain) with no session playing, its meter fed; an
    /// input at another rate than the device is not heard and says so.
    #[test]
    fn the_input_is_heard_through_the_output_callback() {
        let mut p = Project::new("t");
        let bid = edit::insert_input_bus(&mut p).unwrap();
        p.timeline.audio.inputs[0].volume = 0.5;
        let mut plan = MonPlan::new(&p, false, false, 0.0);
        let meter = Arc::new(Meter::default());
        plan.paths[0].meter = Some(meter.clone());
        let mut r = rig(RATE);
        r.mons.send(MonMsg::State(Box::new(MonState::new(Arc::new(plan))))).unwrap();
        r.out.ring.rate.store(RATE, Relaxed);
        r.out.ring.push(&[0.4; 2 * 256]);
        let out = pull(&mut r, 256);
        assert!(out.iter().all(|v| (v - 0.2).abs() < 1e-6), "{:?}", &out[..4]);
        assert!((meter.take_peaks()[0] - 0.2).abs() < 1e-6 && bid == p.timeline.audio.inputs[0].id);
        r.out.ring.rate.store(44_100, Relaxed);
        r.out.ring.push(&[0.4; 2 * 256]);
        assert!(pull(&mut r, 256).iter().all(|v| *v == 0.0));
        assert!(r.out.shared.mismatch.load(Relaxed));
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
