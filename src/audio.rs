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
use crate::engine::mix::{self, Block, Mixer, frame};
use crate::engine::plan::{AudioPlan, QUICKFADE, compile_audio};
use crate::edit;
use crate::project::{MediaItem, NormSpan, Normalized, Project, Track};

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

/// L/R gain for track `t` of `tl`, or None if it (or its group) is muted or another track is soloed.
pub fn track_gain(tl: &crate::project::Timeline, t: &Track, any_solo: bool) -> Option<[f32; 2]> {
    if !tl.plays(t, any_solo) {
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
    rx: Receiver<Block>,
    buf: Vec<f32>,
    pos: usize,
    /// The current block's start (timeline frame) and play rate: its sample `pos` is timeline sample f0 x 2 + pos x rate.
    f0: i64,
    rate: f64,
    /// Underrun silence must come in whole frames or L/R would swap.
    odd: bool,
    /// Timeline sample (frame x 2 + channel) after the last one handed to the device. Set from each block's start
    /// frame and rate, so it follows loop wraps and shuttle speeds exactly.
    clock: Arc<AtomicI64>,
    /// Every call, underruns included: the watchdog's sign that the device is alive.
    calls: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    master: Arc<Master>,
    /// The master gain applied, gliding to the fader's (about 2 ms), so a move never steps on one sample.
    gain: [f32; 2],
}

impl Feed {
    fn new(rx: Receiver<Block>, clock: Arc<AtomicI64>, calls: Arc<AtomicU64>, stop: Arc<AtomicBool>, master: Arc<Master>) -> Self {
        let gain = [master.gain(0), master.gain(1)];
        Self { rx, buf: Vec::new(), pos: 0, f0: 0, rate: 1.0, odd: false, clock, calls, stop, master, gain }
    }
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
            if let Ok((f0, rate, b)) = self.rx.try_recv() {
                (self.buf, self.pos, self.f0, self.rate) = (b, 0, f0, rate);
                self.clock.store(f0 * 2, Relaxed);
            }
        }
        if let Some(&s) = self.buf.get(self.pos) {
            let ch = self.pos % 2;
            self.pos += 1;
            self.clock.store(self.f0 * 2 + (self.pos as f64 * self.rate).round() as i64, Relaxed);
            let g = &mut self.gain[ch];
            let to = self.master.gain(ch);
            *g = if (to - *g).abs() < 1e-6 { to } else { *g + (to - *g) / 96.0 };
            let v = s * *g;
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
    /// The plan the mixer reads once per block, and the content hash, bypass and quickfade it was compiled with.
    slot: Arc<Mutex<Arc<AudioPlan>>>,
    hash: (u64, bool, bool),
    /// The play rate (f64 bits): 1, or a shuttle's, which `set_rate` changes on the fly.
    rate: Arc<AtomicU64>,
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
    /// Options › Bypass All Audio FX: the plans this player compiles skip every audio plug-in.
    pub bypass_fx: bool,
    /// Options › Quickfade Audio Edits.
    pub quickfade: bool,
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
            bypass_fx: false,
            quickfade: true,
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

    /// Play the timeline's audio from `from`, looping over `lp` (seconds) inside the mixer if set, at `rate` (1, or a
    /// J / L shuttle's: then varispeed, backwards below 0, and without a loop).
    /// Without a device no session starts, and `position()` stays None so the caller keeps its own clock.
    pub fn start(&mut self, ctx: &egui::Context, project: &Project, from: f64, lp: Option<(f64, f64)>, rate: f64) {
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
        let feed = Feed::new(rx, clock.clone(), calls.clone(), stop.clone(), self.master.clone());
        if let Err(e) = handle.play_raw(feed) {
            *self.error.lock().unwrap() = Some(format!("Audio output failed: {e}"));
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
        let hash = (crate::engine::content_hash(project), self.bypass_fx, self.quickfade);
        self.session = Some(Session { clock, calls, watch: (0, Instant::now()), stop, slot, hash, rate });
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

    fn plan(&self, project: &Project) -> AudioPlan {
        AudioPlan { bypass_fx: self.bypass_fx, quickfade: if self.quickfade { QUICKFADE } else { 0.0 }, ..compile_audio(project) }
    }

    /// While playing: give the mixer the project's new plan after an edit (or a Bypass All Audio FX toggle). It is
    /// heard within about 150 ms.
    pub fn set_plan(&mut self, project: &Project) {
        let hash = (crate::engine::content_hash(project), self.bypass_fx, self.quickfade);
        if self.session.as_ref().is_none_or(|s| s.hash == hash) {
            return;
        }
        let plan = Arc::new(self.plan(project));
        if let Some(s) = &mut self.session {
            (s.hash, *s.slot.lock().unwrap()) = (hash, plan);
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
        if m.generator.is_some() {
            return None; // generated media has no sound and no file to read
        }
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

    fn feed(master: Arc<Master>) -> (mpsc::SyncSender<Block>, Feed) {
        let (tx, rx) = mpsc::sync_channel(4);
        let (clock, calls, stop) = (Arc::new(AtomicI64::new(0)), Arc::new(AtomicU64::new(0)), Arc::new(AtomicBool::new(false)));
        (tx, Feed::new(rx, clock, calls, stop, master))
    }

    #[test]
    fn pan_and_volume_gain() {
        let (tl, mut t) = (crate::project::Timeline::new(), Track::new("a", crate::project::TrackKind::Audio));
        t.volume = 0.8;
        assert_eq!(track_gain(&tl, &t, false), Some([0.8, 0.8]));
        t.pan = -1.0;
        assert_eq!(track_gain(&tl, &t, false), Some([0.8, 0.0]));
        t.pan = 0.5;
        assert_eq!(track_gain(&tl, &t, false), Some([0.4, 0.8]));
        assert_eq!(track_gain(&tl, &t, true), None); // another track is solo
        t.solo = true;
        t.muted = true;
        assert_eq!(track_gain(&tl, &t, true), None); // mute beats solo
    }

    #[test]
    fn master_gain_and_peak_tap() {
        let master = Arc::new(Master::new());
        master.set_gain([0.5, 0.0]);
        let (tx, mut feed) = feed(master.clone());
        tx.send((0, 1.0, vec![0.8, 0.8, -1.0, 0.2])).unwrap();
        let out: Vec<f32> = (0..4).map(|_| feed.next().unwrap()).collect();
        assert_eq!(out, [0.4, 0.0, -0.5, 0.0]);
        assert_eq!(master.take_peaks(), [0.5, 0.0]);
        assert_eq!(master.take_peaks(), [0.0, 0.0]); // reading resets
    }

    /// A master fader move glides over a few milliseconds instead of stepping on one sample (a click).
    #[test]
    fn master_gain_changes_glide() {
        let master = Arc::new(Master::new());
        let (tx, mut feed) = feed(master.clone());
        tx.send((0, 1.0, vec![1.0; 2 * RATE as usize / 50])).unwrap();
        assert_eq!((feed.next(), feed.next()), (Some(1.0), Some(1.0)));
        master.set_gain([0.0, 0.0]);
        let out: Vec<f32> = (0..2 * RATE as usize / 50 - 2).map(|_| feed.next().unwrap()).collect();
        assert!(out[0] > 0.9, "stepped: {}", out[0]);
        assert!(out.windows(2).all(|w| w[1] <= w[0]));
        assert!(*out.last().unwrap() < 1e-3, "{}", out.last().unwrap());
    }

    #[test]
    fn feed_clock_follows_block_starts() {
        let (tx, mut feed) = feed(Arc::new(Master::new()));
        tx.send((100, 1.0, vec![0.1; 4])).unwrap();
        tx.send((10, 1.0, vec![0.2; 4])).unwrap(); // a loop wrap back to frame 10
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

    /// R4b: shuttle 2x for 1 s advances the clock 2 s; backwards it runs back.
    #[test]
    fn shuttle_clock_advances_by_the_rate() {
        let (tx, mut feed) = feed(Arc::new(Master::new()));
        tx.send((RATE as i64, 2.0, vec![0.0; 2 * RATE as usize])).unwrap();
        tx.send((3 * RATE as i64, -4.0, vec![0.0; 2 * RATE as usize / 4])).unwrap();
        for _ in 0..2 * RATE as usize {
            feed.next();
        }
        assert_eq!(clock_secs(feed.clock.load(Relaxed)), 3.0);
        for _ in 0..2 * RATE as usize / 4 {
            feed.next();
        }
        assert_eq!(clock_secs(feed.clock.load(Relaxed)), 2.0);
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
