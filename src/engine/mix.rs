//! Audio mixer: the audio plan mixed block by block from persistent ffmpeg PCM streams (stereo f32 at RATE).
//! `run` is the "milan-mix" thread body for playback; it loops inside the mixer, so a loop wrap is seamless.
use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use crate::audio::RATE;
use crate::engine::keyframes::{db_to_gain, ramp};
use crate::engine::plan::{ASrc, AudioPlan, fade};
use crate::project::Envelope;

/// Frames per mixed block (about 21 ms).
pub const BLOCK: usize = 1024;
/// Frames per decoded chunk (0.25 s).
const CHUNK: usize = RATE as usize / 4;
/// A decoder silent this long is given up on, so a hung source cannot hold the mix thread forever.
const TIMEOUT: Duration = Duration::from_secs(2);

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

/// Source seconds `at` frames into the event `src` (forward).
fn src_time(src: &ASrc, at: i64) -> f64 {
    src.in_point + at as f64 * src.rate / RATE as f64
}

/// ffmpeg arguments (after `-nostdin`) streaming `src` from `at` frames into its event, in timeline order.
/// Forward, it reads on to the media end, so a trim or a split keeps the decoder. Reversed, the span still to play
/// is read and turned round (`areverse` holds it all in memory).
// shortcut: areverse buffers the whole span (about 23 MB per stereo minute); render a reversed temp file past ~10 min.
pub fn args(src: &ASrc, at: i64) -> Vec<String> {
    let mut a: Vec<String> = ["-v", "error", "-ss"].map(String::from).to_vec();
    if src.reverse {
        let len = (src.dur - at as f64 / RATE as f64).max(0.0) * src.rate;
        a.extend([format!("{:.6}", src.in_point), "-t".into(), format!("{len:.6}")]);
    } else {
        a.push(format!("{:.6}", src_time(src, at)));
    }
    a.extend(["-i".into(), src.path.to_string_lossy().into_owned()]);
    a.extend(["-vn", "-map", "0:a:0", "-ac", "2", "-ar"].map(String::from));
    a.push(RATE.to_string());
    let mut af = atempo(src.rate);
    if src.reverse {
        af.insert(0, "areverse".into());
    }
    if !af.is_empty() {
        a.extend(["-af".into(), af.join(",")]);
    }
    a.extend(["-f", "f32le", "-"].map(String::from));
    a
}

/// One event's decoder: interleaved stereo chunks, read forward.
struct Stream {
    rx: Receiver<Vec<f32>>,
    buf: Vec<f32>,
    pos: usize,
    child: Option<Child>,
    /// Last stderr line, once ffmpeg has exited.
    err: Option<JoinHandle<String>>,
    /// A chunk has arrived.
    started: bool,
    /// Samples played as silence while the decoder was starting, dropped when they arrive so the event stays in sync.
    skip: usize,
}

impl Drop for Stream {
    fn drop(&mut self) {
        if let Some(c) = &mut self.child {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl Stream {
    fn from_rx(rx: Receiver<Vec<f32>>) -> Self {
        Self { rx, buf: Vec::new(), pos: 0, child: None, err: None, started: false, skip: 0 }
    }

    /// A stream that has already ended: silence.
    fn ended() -> Self {
        Self::from_rx(mpsc::sync_channel(0).1)
    }

    /// Fill `out`, with silence once the stream has ended. Returns the decoder's error the first time it ends with one.
    /// A decoder that has not started (or is still catching up) holds the caller only when `wait` is set: otherwise
    /// it plays silence, so a slow start (a reversed span, a cold disk) delays only its own event.
    fn read(&mut self, out: &mut [f32], wait: bool) -> Option<String> {
        let mut k = 0;
        while k < out.len() {
            if self.pos >= self.buf.len() {
                let behind = !self.started || self.skip > 0;
                let got = if behind && !wait {
                    self.rx.try_recv().map_err(|e| if e == TryRecvError::Empty { RecvTimeoutError::Timeout } else { RecvTimeoutError::Disconnected })
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
                        if e == RecvTimeoutError::Timeout {
                            // Started, then stalled. Killing ffmpeg ends its reader, so later reads are silent instead of waiting again.
                            if let Some(c) = &mut self.child {
                                let _ = c.kill();
                            }
                            return Some("Audio decoding stopped responding: an event is silent".into());
                        }
                        return self.err.take().and_then(|j| j.join().ok()).filter(|e| !e.is_empty());
                    }
                }
            }
            let n = (self.buf.len() - self.pos).min(out.len() - k);
            out[k..k + n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
            (self.pos, k) = (self.pos + n, k + n);
        }
        None
    }
}

fn spawn(src: &ASrc, at: i64) -> Result<Stream, String> {
    let mut cmd = crate::proc::ffmpeg();
    cmd.args(args(src, at)).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = crate::proc::spawn(&mut cmd).map_err(|e| format!("{}: audio is silent", crate::proc::tool_error(&e, "ffmpeg")))?;
    let (mut out, err) = (child.stdout.take().expect("piped stdout"), child.stderr.take().expect("piped stderr"));
    let (tx, rx) = mpsc::sync_channel(8);
    std::thread::spawn(move || {
        let mut bytes = vec![0u8; CHUNK * 8];
        loop {
            let mut n = 0;
            while n < bytes.len() {
                match out.read(&mut bytes[n..]) {
                    Ok(0) | Err(_) => break,
                    Ok(k) => n += k,
                }
            }
            let pcm: Vec<f32> = bytes[..n - n % 8].chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
            if pcm.is_empty() || tx.send(pcm).is_err() || n < bytes.len() {
                break;
            }
        }
    });
    let err = std::thread::spawn(move || BufReader::new(err).lines().map_while(Result::ok).filter(|l| !l.trim().is_empty()).last().unwrap_or_default());
    let mut s = Stream::from_rx(rx);
    (s.child, s.err) = (Some(child), Some(err));
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
        self.src.path == src.path && self.src.rate == src.rate && (self.t - src_time(src, at)).abs() <= src.rate / RATE as f64 + 1e-9
    }
}

type Open = Box<dyn FnMut(&ASrc, i64) -> Result<Stream, String> + Send>;

pub struct Mixer {
    open: Open,
    streams: Vec<Live>,
    round: u64,
    pass: u64,
    /// Wait for decoders that have not started yet (a session's first block, a render); otherwise they play silence.
    wait: bool,
    track: Vec<f32>,
    pcm: Vec<f32>,
    /// First decoder error since it was last taken.
    pub error: Option<String>,
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
        Self { open, streams: Vec::new(), round: 0, pass: 0, wait: true, track: Vec::new(), pcm: Vec::new(), error: None }
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

    /// Open the streams for what plays in frames [w0, w1) now, so their first decode is not heard as a gap.
    pub fn warm(&mut self, plan: &AudioPlan, w0: i64, w1: i64) {
        self.pass += 1;
        for e in plan.tracks.iter().flat_map(|t| &t.events) {
            let (ps, pe) = (frame(e.src.pos), frame(e.src.pos + e.src.dur));
            let a = w0.max(ps);
            if a < pe.min(w1) {
                self.stream(&e.src, a - ps);
            }
        }
    }

    /// Mix `out.len() / 2` stereo frames from timeline frame `f0`: per event PCM x gain x fades x volume envelope,
    /// then per track the volume, mute and pan envelopes (ramped across the block) x Volume and Pan.
    // shortcut: no FX chains until batch 16 (event FX after the PCM, track FX before the envelopes, master FX last).
    pub fn block(&mut self, plan: &AudioPlan, f0: i64, out: &mut [f32]) {
        // Streams nothing used in the last block, or warmed for this one, are closed.
        let round = self.round;
        self.streams.retain(|l| l.used == round);
        self.round += 1;
        self.pass += 1;
        out.fill(0.0);
        let n = out.len() / 2;
        let (f1, r) = (f0 + n as i64, RATE as f64);
        let (mut track, mut pcm) = (std::mem::take(&mut self.track), std::mem::take(&mut self.pcm));
        for tr in &plan.tracks {
            track.clear();
            track.resize(out.len(), 0.0);
            let mut heard = false;
            for e in &tr.events {
                let (ps, pe) = (frame(e.src.pos), frame(e.src.pos + e.src.dur));
                let (a, b) = (f0.max(ps), f1.min(pe));
                if b <= a {
                    continue;
                }
                heard = true;
                pcm.clear();
                pcm.resize((b - a) as usize * 2, 0.0);
                let wait = self.wait;
                let l = self.stream(&e.src, a - ps);
                (l.next, l.t) = (b - ps, l.t + (b - a) as f64 * e.src.rate / r);
                if let Some(err) = l.s.read(&mut pcm, wait) {
                    self.error.get_or_insert(err);
                }
                let (v0, v1) = env_gain(&e.vol_env, (a - ps) as f64 / r, (b - ps) as f64 / r);
                let len = (b - a) as f32;
                for (i, (o, x)) in track[(a - f0) as usize * 2..].chunks_exact_mut(2).zip(pcm.chunks_exact(2)).enumerate() {
                    let local = (a - ps + i as i64) as f64 / r;
                    let mut g = e.gain * (v0 + (v1 - v0) * i as f32 / len);
                    if local < e.fade_in {
                        g *= fade(e.fade_in_curve, local, e.fade_in);
                    }
                    if e.src.dur - local < e.fade_out {
                        g *= fade(e.fade_out_curve, e.src.dur - local, e.fade_out);
                    }
                    o[0] += x[0] * g;
                    o[1] += x[1] * g;
                }
            }
            if !heard {
                continue;
            }
            let (t0, t1) = (f0 as f64 / r, f1 as f64 / r);
            let (v0, v1) = env_gain(&tr.vol_env, t0, t1);
            let (m0, m1) = tr.mute_env.as_ref().map_or((1.0, 1.0), |e| ramp(e, t0, t1));
            let (p0, p1) = tr.pan_env.as_ref().map_or((0.0, 0.0), |e| ramp(e, t0, t1));
            for (i, (o, x)) in out.chunks_exact_mut(2).zip(track.chunks_exact(2)).enumerate() {
                let s = i as f32 / n as f32;
                let lerp = |a: f32, b: f32| a + (b - a) * s;
                let g = tr.volume * lerp(v0, v1) * lerp(m0, m1).clamp(0.0, 1.0);
                // Pan trims the pan envelope (VEGAS); one balance law, the same as audio::track_gain.
                let p = (tr.pan + lerp(p0, p1)).clamp(-1.0, 1.0);
                o[0] += x[0] * g * (1.0 - p).min(1.0);
                o[1] += x[1] * g * (1.0 + p).min(1.0);
            }
        }
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

/// The "milan-mix" thread: blocks from frame `from`, each mixed from the newest plan in `slot` (so edits are heard
/// within the channel's depth) and sent with its start frame, looping over `lp` (frames). It never ends by itself:
/// past the content end it plays silence until the app stops it there, so an edit that extends the content is heard.
pub fn run(slot: &Mutex<Arc<AudioPlan>>, mut mixer: Mixer, from: i64, lp: Option<(i64, i64)>, tx: &SyncSender<(i64, Vec<f32>)>, quit: &AtomicBool, mut on_error: impl FnMut(String)) {
    let ahead = RATE as i64; // streams open 1 s before they are heard
    let lp_end = lp.map_or(i64::MAX, |l| l.1);
    // The first block's decoders all start now, in parallel.
    mixer.warm(&slot.lock().unwrap().clone(), from, (from + ahead).min(lp_end));
    let mut f = from;
    // A stopped session opens no more decoders.
    while !quit.load(Relaxed) {
        let plan = slot.lock().unwrap().clone();
        let (f0, n) = next_block(f, lp);
        let mut buf = vec![0.0; n * 2];
        mixer.block(&plan, f0, &mut buf);
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
        if !send_or_quit(tx, (f0, buf), quit) {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::plan::compile_audio;
    use crate::project::{Clip, Curve, EnvKind, Keyframe, Keyframes, MediaItem, MediaType, Project};
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
        m.block(&slot.lock().unwrap().clone(), 4, &mut out);
        assert_eq!(out, [0.25; 8]);
    }

    /// Runs a session on a fake decoder and returns its first `blocks` blocks.
    fn session(p: &Project, from: f64, lp: Option<(f64, f64)>, blocks: usize) -> Vec<(i64, Vec<f32>)> {
        let slot = Mutex::new(Arc::new(compile_audio(p)));
        let (tx, rx) = mpsc::sync_channel(4);
        let quit = AtomicBool::new(false);
        let lp = lp.map(|(a, b)| (frame(a), frame(b)));
        std::thread::scope(|s| {
            let (slot, quit) = (&slot, &quit);
            // The thread owns `tx`, so the receiver sees the end of a session that stops by itself.
            s.spawn(move || run(slot, ones(), frame(from), lp, &tx, quit, |e| panic!("{e}")));
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
        for (f0, buf) in &blocks {
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
        for (f0, buf) in &blocks {
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
        assert_eq!(s.read(&mut out, false), None);
        assert!(t.elapsed() < Duration::from_millis(50), "waited for the decoder");
        assert_eq!(out, [0.0; 4]);
        // The 4 samples played as silence are dropped when they arrive.
        tx.send((0..12).map(|v| v as f32).collect()).unwrap();
        assert_eq!(s.read(&mut out, false), None);
        assert_eq!(out, [4.0, 5.0, 6.0, 7.0]);
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
        let src = ASrc { path: r"C:\a b.wav".into(), pos: 5.0, dur: 4.0, in_point: 2.0, rate: 1.0, reverse: false };
        assert_eq!(
            args(&src, frame(1.0)).join(" "),
            r"-v error -ss 3.000000 -i C:\a b.wav -vn -map 0:a:0 -ac 2 -ar 48000 -f f32le -"
        );
        // Rate 2, reversed, 1 s in: the source span still to play is 2 .. 2 + 3 s x 2, read backwards at double speed.
        let a = args(&ASrc { rate: 2.0, reverse: true, ..src }, frame(1.0));
        assert_eq!(a[2..6], ["-ss", "2.000000", "-t", "6.000000"]);
        assert!(a.windows(2).any(|w| w == ["-af", "areverse,atempo=2"]));
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
        let src = ASrc { path: path.clone(), pos: 0.0, dur: 1.0, in_point: 1.0, rate: 2.0, reverse: true };
        let mut s = spawn(&src, 0).unwrap();
        let mut out = vec![0.0; 2 * 48_000];
        assert_eq!(s.read(&mut out, true), None);
        // Timeline 0 shows source 3 s, timeline 0.5 s shows source 2 s.
        assert!((out[0] - 0.3).abs() < 0.01, "{}", out[0]);
        assert!((out[48_000] - 0.2).abs() < 0.01, "{}", out[48_000]);
        drop(s);
        // Forward, 0.5 s into the event: source 1.5 s, read on past the event end (no -t).
        let mut s = spawn(&ASrc { rate: 1.0, reverse: false, ..src }, frame(0.5)).unwrap();
        assert_eq!(s.read(&mut out, true), None);
        assert!((out[0] - 0.15).abs() < 0.01, "{}", out[0]);
        assert!((out[2 * 47_999] - 0.25).abs() < 0.01, "{}", out[2 * 47_999]);
        drop(s);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
