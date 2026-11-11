//! Audio playback (ffmpeg decode -> stereo f32 mix -> rodio) and clip waveform peaks.
use eframe::egui;
use rodio::{OutputStream, OutputStreamHandle, Source};
use std::collections::HashMap;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use crate::project::{MediaItem, Project, Track};

pub const RATE: u32 = 48_000;
/// Seconds decoded per chunk; the first chunk gates playback start.
const WINDOW: f64 = 5.0;
pub const PEAKS_PER_SEC: f64 = 100.0;
const PEAK_RATE: u32 = 8_000;

fn ffmpeg() -> Command {
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-nostdin", "-v", "error"]).stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    cmd
}

fn ffmpeg_err(e: std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => "ffmpeg not found on PATH: audio is silent".to_string(),
        _ => format!("ffmpeg failed: {e}"),
    }
}

// ---- Mixer ----

/// One audible clip: where it sits on the timeline, where it reads the source, and its L/R gain.
#[derive(Clone, Debug)]
pub struct Seg {
    pub path: PathBuf,
    pub pos: f64,
    pub dur: f64,
    pub src: f64,
    pub gain: [f32; 2],
}

/// L/R gain for a track, or None if it is muted or another track is soloed.
pub fn track_gain(t: &Track, any_solo: bool) -> Option<[f32; 2]> {
    if t.muted || (any_solo && !t.solo) {
        return None;
    }
    let p = t.pan.clamp(-1.0, 1.0);
    Some([t.volume * (1.0 - p).min(1.0), t.volume * (1.0 + p).min(1.0)])
}

/// Audible clips on audio tracks whose media has an audio stream.
pub fn plan(project: &Project) -> Vec<Seg> {
    let tracks = &project.timeline.audio_tracks;
    let any_solo = tracks.iter().any(|t| t.solo);
    let mut segs = Vec::new();
    for t in tracks {
        let Some(gain) = track_gain(t, any_solo) else { continue };
        for c in &t.clips {
            let media = c.media_id.as_deref().and_then(|id| project.media_pool.iter().find(|m| m.id == id));
            if let Some(m) = media.filter(|m| m.sample_rate.is_some()) {
                segs.push(Seg { path: m.path.clone(), pos: c.position, dur: c.duration, src: c.in_point, gain });
            }
        }
    }
    segs
}

/// Mix `frames` stereo frames starting at timeline time `t0`.
/// `decode(seg, source start, seconds)` returns interleaved stereo at RATE.
pub fn render(segs: &[Seg], t0: f64, frames: usize, mut decode: impl FnMut(&Seg, f64, f64) -> Vec<f32>) -> Vec<f32> {
    let mut out = vec![0.0_f32; frames * 2];
    let t1 = t0 + frames as f64 / RATE as f64;
    for s in segs {
        let (a, b) = (s.pos.max(t0), (s.pos + s.dur).min(t1));
        if b <= a {
            continue;
        }
        let off = (((a - t0) * RATE as f64).round() as usize * 2).min(out.len());
        let n = ((b - a) * RATE as f64).round() as usize * 2;
        let pcm = decode(s, s.src + (a - s.pos), b - a);
        for (i, (o, x)) in out[off..].iter_mut().zip(&pcm).take(n).enumerate() {
            *o += x * s.gain[i % 2];
        }
    }
    out
}

fn decode_pcm(path: &Path, start: f64, dur: f64) -> Result<Vec<f32>, String> {
    let out = ffmpeg()
        .args(["-ss", &format!("{:.4}", start.max(0.0)), "-t", &format!("{dur:.4}"), "-i"])
        .arg(path)
        .args(["-vn", "-f", "f32le", "-ac", "2", "-ar", "48000", "-"])
        .output()
        .map_err(ffmpeg_err)?;
    if out.stdout.is_empty() && !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("audio decode failed: {}", err.lines().next().unwrap_or("unknown error")));
    }
    Ok(out.stdout.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect())
}

// ---- Output ----

/// rodio source fed with mixed chunks; counts samples handed to the device (the master clock).
struct Feed {
    rx: Receiver<Vec<f32>>,
    buf: Vec<f32>,
    pos: usize,
    /// Underrun silence must come in whole frames or L/R would swap.
    odd: bool,
    played: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
}

impl Iterator for Feed {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.stop.load(Relaxed) {
            return None;
        }
        if self.pos >= self.buf.len() && !self.odd {
            match self.rx.try_recv() {
                Ok(b) => (self.buf, self.pos) = (b, 0),
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => return None,
            }
        }
        if let Some(&s) = self.buf.get(self.pos) {
            self.pos += 1;
            self.played.fetch_add(1, Relaxed);
            return Some(s);
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

struct Session {
    from: f64,
    played: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
}

pub struct Player {
    device_rx: Receiver<Result<OutputStreamHandle, String>>,
    device: Option<Result<OutputStreamHandle, String>>,
    session: Option<Session>,
    error: Arc<Mutex<Option<String>>>,
}

impl Player {
    pub fn new() -> Self {
        let (tx, device_rx) = mpsc::channel();
        // Opening the device can take a while, and OutputStream is !Send, so it lives on its own thread.
        std::thread::spawn(move || match OutputStream::try_default() {
            Ok((_stream, handle)) => {
                if tx.send(Ok(handle)).is_ok() {
                    loop {
                        std::thread::park();
                    }
                }
            }
            Err(e) => {
                let _ = tx.send(Err(format!("No audio device ({e}): playback is silent")));
            }
        });
        Self { device_rx, device: None, session: None, error: Arc::new(Mutex::new(None)) }
    }

    fn poll_device(&mut self) {
        if self.device.is_none() {
            self.device = self.device_rx.try_recv().ok();
        }
    }

    /// Play the timeline's audio from `from`. Without audible clips or a device no session starts,
    /// and `position()` stays None so the caller keeps its own clock.
    pub fn start(&mut self, ctx: &egui::Context, project: &Project, from: f64) {
        self.stop();
        self.poll_device();
        *self.error.lock().unwrap() = None;
        let Some(Ok(handle)) = &self.device else { return };
        let segs = plan(project);
        let end = crate::render::content_end(project);
        if segs.iter().all(|s| s.pos + s.dur <= from) {
            return;
        }
        let (tx, rx): (SyncSender<Vec<f32>>, _) = mpsc::sync_channel(2);
        let played = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let feed = Feed { rx, buf: Vec::new(), pos: 0, odd: false, played: played.clone(), stop: stop.clone() };
        if let Err(e) = handle.play_raw(feed) {
            *self.error.lock().unwrap() = Some(format!("Audio output failed: {e}"));
            return;
        }
        let (error, quit, ctx) = (self.error.clone(), stop.clone(), ctx.clone());
        std::thread::spawn(move || {
            let mut done = 0usize;
            loop {
                let t = from + done as f64 / RATE as f64;
                if t >= end || quit.load(Relaxed) {
                    break;
                }
                let frames = ((end - t).min(WINDOW) * RATE as f64).ceil() as usize;
                let pcm = render(&segs, t, frames, |s, a, d| {
                    // A stopped session skips the rest of its window instead of decoding every clip.
                    if quit.load(Relaxed) {
                        return Vec::new();
                    }
                    decode_pcm(&s.path, a, d).unwrap_or_else(|e| {
                        *error.lock().unwrap() = Some(e);
                        ctx.request_repaint();
                        Vec::new()
                    })
                });
                if tx.send(pcm).is_err() {
                    break; // playback stopped
                }
                done += frames;
            }
        });
        self.session = Some(Session { from, played, stop });
    }

    /// True until the output device thread has reported success or failure.
    pub fn device_pending(&mut self) -> bool {
        self.poll_device();
        self.device.is_none()
    }

    pub fn stop(&mut self) {
        if let Some(s) = self.session.take() {
            s.stop.store(true, Relaxed);
        }
    }

    /// Timeline time of the last sample handed to the device, while a session is active.
    pub fn position(&self) -> Option<f64> {
        self.session.as_ref().map(|s| s.from + s.played.load(Relaxed) as f64 / 2.0 / RATE as f64)
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

/// Peak per 1/PEAKS_PER_SEC s of the source, mono, 0..=1.
fn peaks(path: &Path) -> Vec<f32> {
    let child = ffmpeg()
        .arg("-i")
        .arg(path)
        .args(["-vn", "-ac", "1", "-ar", &PEAK_RATE.to_string(), "-f", "f32le", "-"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else { return Vec::new() };
    let per = (PEAK_RATE as f64 / PEAKS_PER_SEC) as usize;
    let (mut out, mut peak, mut n) = (Vec::new(), 0.0_f32, 0);
    // Streamed so long files never sit in memory as raw PCM.
    let mut r = BufReader::new(child.stdout.take().expect("piped stdout"));
    let mut b = [0u8; 4];
    while r.read_exact(&mut b).is_ok() {
        peak = peak.max(f32::from_le_bytes(b).abs().min(1.0));
        n += 1;
        if n == per {
            out.push(peak);
            (peak, n) = (0.0, 0);
        }
    }
    if n > 0 {
        out.push(peak);
    }
    let _ = child.wait();
    out
}

/// Per-media peak cache filled by a background thread. Failures cache as empty.
pub struct Waveforms {
    cache: HashMap<String, Option<Vec<f32>>>,
    tx: Sender<(String, PathBuf)>,
    rx: Receiver<(String, Vec<f32>)>,
}

impl Waveforms {
    pub fn new(ctx: &egui::Context) -> Self {
        let (tx, req_rx) = mpsc::channel::<(String, PathBuf)>();
        let (res_tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            for (id, path) in req_rx {
                if res_tx.send((id, peaks(&path))).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        Self { cache: HashMap::new(), tx, rx }
    }

    /// Peaks for `m`, requesting them on first use.
    pub fn get(&mut self, m: &MediaItem) -> Option<&[f32]> {
        for (id, p) in self.rx.try_iter() {
            self.cache.insert(id, Some(p));
        }
        if !self.cache.contains_key(&m.id) {
            self.cache.insert(m.id.clone(), None);
            let _ = self.tx.send((m.id.clone(), m.path.clone()));
        }
        self.cache[&m.id].as_deref().filter(|p| !p.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Clip, MediaType};

    fn ones(_: &Seg, _: f64, d: f64) -> Vec<f32> {
        vec![1.0; (d * RATE as f64).round() as usize * 2]
    }

    fn project() -> Project {
        let mut p = Project::new("t");
        let m = MediaItem {
            id: "a".into(),
            path: "a.wav".into(),
            name: "a".into(),
            media_type: MediaType::Audio,
            duration: 10.0,
            width: None,
            height: None,
            frame_rate: None,
            sample_rate: Some(48_000),
            codec: None,
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
    fn mix_mute_solo_volume_pan() {
        let mut p = project();
        // Track 0: 0.5 both sides; track 1: hard right at 1.0; track 2 muted.
        let out = render(&plan(&p), 0.0, 4, ones);
        assert_eq!(out, [0.5, 1.5, 0.5, 1.5, 0.5, 1.5, 0.5, 1.5]);

        p.timeline.audio_tracks[0].solo = true;
        assert_eq!(render(&plan(&p), 0.0, 2, ones), [0.5, 0.5, 0.5, 0.5]);
    }

    #[test]
    fn mix_respects_clip_position_and_source_offset() {
        let mut p = project();
        p.timeline.audio_tracks[1].muted = true;
        let c = &mut p.timeline.audio_tracks[0].clips[0];
        (c.position, c.duration, c.in_point) = (2.0 / RATE as f64, 2.0 / RATE as f64, 3.0);
        let mut asked = Vec::new();
        let out = render(&plan(&p), 0.0, 6, |s, a, d| {
            asked.push((a, d));
            ones(s, a, d)
        });
        assert_eq!(out, [0.0, 0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 0.5, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(asked, [(3.0, 2.0 / RATE as f64)]);
        // A window starting mid-clip reads from the matching source offset.
        let mut asked = Vec::new();
        render(&plan(&p), 3.0 / RATE as f64, 4, |s, a, d| {
            asked.push(a);
            ones(s, a, d)
        });
        assert!((asked[0] - (3.0 + 1.0 / RATE as f64)).abs() < 1e-12);
    }
}
