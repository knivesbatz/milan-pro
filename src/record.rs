//! Audio recording (Ctrl+R into armed tracks): capture from one input device through cpal (rodio's), written as a
//! stereo IEEE-float WAV, and the live input level of armed tracks.
use rodio::cpal::{self, SampleFormat, traits::{DeviceTrait, HostTrait, StreamTrait}};
use std::fs::File;
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use crate::project::Project;

/// Bytes before the samples: RIFF header, a 16-byte fmt chunk and the data chunk header.
const HEADER: u64 = 44;

/// A stereo-or-any-channel 32-bit float WAV whose RIFF and data sizes are patched in place (`patch`), so a file cut
/// off by a crash still reads up to the last patch.
pub struct WavWriter<W: Write + Seek> {
    w: W,
    channels: u16,
    frames: u64,
}

impl<W: Write + Seek> WavWriter<W> {
    pub fn new(mut w: W, channels: u16, rate: u32) -> io::Result<Self> {
        let block = channels as u32 * 4;
        let mut h = Vec::with_capacity(HEADER as usize);
        h.extend_from_slice(b"RIFF");
        h.extend_from_slice(&(HEADER as u32 - 8).to_le_bytes());
        h.extend_from_slice(b"WAVEfmt ");
        h.extend_from_slice(&16u32.to_le_bytes());
        h.extend_from_slice(&3u16.to_le_bytes()); // WAVE_FORMAT_IEEE_FLOAT
        h.extend_from_slice(&channels.to_le_bytes());
        h.extend_from_slice(&rate.to_le_bytes());
        h.extend_from_slice(&(rate * block).to_le_bytes());
        h.extend_from_slice(&(block as u16).to_le_bytes());
        h.extend_from_slice(&32u16.to_le_bytes());
        h.extend_from_slice(b"data");
        h.extend_from_slice(&0u32.to_le_bytes());
        w.write_all(&h)?;
        Ok(Self { w, channels, frames: 0 })
    }

    /// Interleaved samples, whole frames.
    pub fn write(&mut self, samples: &[f32]) -> io::Result<()> {
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        self.w.write_all(&bytes)?;
        self.frames += (samples.len() / self.channels as usize) as u64;
        Ok(())
    }

    /// Write the current sizes into the header and go back to the end.
    // shortcut: the 32-bit sizes saturate past 4 GB (about 3 h of 48 kHz stereo); switch to RF64 if takes get that long.
    pub fn patch(&mut self) -> io::Result<()> {
        let data = (self.frames * self.channels as u64 * 4).min(u32::MAX as u64 - HEADER) as u32;
        self.w.seek(SeekFrom::Start(4))?;
        self.w.write_all(&(data + HEADER as u32 - 8).to_le_bytes())?;
        self.w.seek(SeekFrom::Start(40))?;
        self.w.write_all(&data.to_le_bytes())?;
        self.w.seek(SeekFrom::End(0))?;
        self.w.flush()
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }
}

pub fn i16_to_f32(s: i16) -> f32 {
    s as f32 / 32768.0
}

pub fn u16_to_f32(s: u16) -> f32 {
    (s as f32 - 32768.0) / 32768.0
}

/// Where recordings go: the project's record folder, else the project file's folder; None for an unsaved project
/// without one.
pub fn folder(p: &Project) -> Option<PathBuf> {
    p.timeline.record_dir.clone().or_else(|| p.path.as_deref().and_then(Path::parent).map(Path::to_path_buf))
}

/// A new, empty `<project> <track> Rec NNN.wav` in `dir`, numbered past every existing file (never overwrites, even
/// against another writer: the file is created with create_new).
pub fn create_file(dir: &Path, project: &str, track: &str) -> io::Result<(PathBuf, File)> {
    let clean = |s: &str| s.chars().map(|c| if r#"\/:*?"<>|"#.contains(c) || c.is_control() { '_' } else { c }).collect::<String>();
    let (project, track) = (clean(project), clean(track));
    for n in 1..10_000 {
        let path = dir.join(format!("{project} {track} Rec {n:03}.wav"));
        match File::options().write(true).create_new(true).open(&path) {
            Ok(f) => return Ok((path, f)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, "no free recording file name"))
}

/// Recorded Files › Rename: media `id`'s file becomes `<stem>.wav` in its folder (never over another file), and the
/// media and the events still named after it follow. Not an undo step: the file on disk does not come back.
pub fn rename_file(p: &mut Project, id: &str, stem: &str) -> Result<(), String> {
    let stem = stem.trim().trim_end_matches(".wav");
    if stem.is_empty() || stem.chars().any(|c| r#"\/:*?"<>|"#.contains(c) || c.is_control()) {
        return Err(format!("\"{stem}\" is not a valid file name"));
    }
    let m = p.media_pool.iter_mut().find(|m| m.id == id).ok_or("The recording is no longer in the project")?;
    let to = m.path.with_file_name(format!("{stem}.wav"));
    if to == m.path {
        return Ok(());
    }
    // shortcut: exists-then-rename can race another program creating the same name; fine for the user's own folder.
    if to.exists() {
        return Err(format!("{} already exists", to.display()));
    }
    std::fs::rename(&m.path, &to).map_err(|e| format!("Could not rename {}: {e}", m.path.display()))?;
    let (old, name) = (std::mem::take(&mut m.name), format!("{stem}.wav"));
    (m.path, m.name) = (to, name.clone());
    for t in p.timeline.audio_tracks.iter_mut() {
        for c in t.clips.iter_mut().filter(|c| c.media_id.as_deref() == Some(id) && c.name == old) {
            c.name = name.clone();
        }
    }
    p.mark_modified();
    Ok(())
}

/// Names of the input devices (Preferences › Audio Device).
pub fn input_devices() -> Vec<String> {
    cpal::default_host().input_devices().map(|ds| ds.filter_map(|d| d.name().ok()).collect()).unwrap_or_default()
}

/// A finished recording.
pub struct Take {
    pub path: PathBuf,
    pub frames: u64,
    pub rate: u32,
    /// Blocks lost because the writer fell behind (each shifts later audio earlier).
    pub dropped: u64,
}

/// An open input device: always the level meter, and with a file, a recording into it.
pub struct Capture {
    /// Dropping it ends the device thread, which drops the stream (and with it the samples' sender).
    stop: Option<mpsc::Sender<()>>,
    device: Option<JoinHandle<()>>,
    writer: Option<JoinHandle<io::Result<(u64, u32)>>>,
    peaks: Arc<[AtomicU32; 2]>,
    dropped: Arc<AtomicU64>,
    /// Frames handed to the writer, and the device rate (0 until it opens): where the file is now.
    sent: Arc<AtomicU64>,
    rate: Arc<AtomicU32>,
    error: Arc<Mutex<Option<String>>>,
    pub path: Option<PathBuf>,
}

impl Capture {
    /// Open input `device` (None or not found: the default one) on the "milan-rec" thread; with `out`, record into it.
    /// Returns at once; a failure to open shows in `error()`.
    pub fn start(device: Option<String>, out: Option<(PathBuf, File)>) -> Capture {
        let peaks: Arc<[AtomicU32; 2]> = Arc::new([AtomicU32::new(0), AtomicU32::new(0)]);
        let (dropped, error) = (Arc::new(AtomicU64::new(0)), Arc::new(Mutex::new(None)));
        let (sent, rate) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicU32::new(0)));
        let (stop, stopped) = mpsc::channel::<()>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<u32, String>>();
        let (data_tx, data_rx) = mpsc::sync_channel::<Vec<f32>>(64);
        let path = out.as_ref().map(|o| o.0.clone());
        let writer = out.map(|(_, file)| {
            let error = error.clone();
            std::thread::Builder::new().name("milan-rec-wav".into()).spawn(move || write_wav(file, ready_rx, data_rx, &error)).expect("spawn milan-rec-wav")
        });
        let tx = writer.is_some().then_some(data_tx);
        let (p, d, e, s, r) = (peaks.clone(), dropped.clone(), error.clone(), sent.clone(), rate.clone());
        let device = std::thread::Builder::new()
            .name("milan-rec".into())
            .spawn(move || {
                let stream = open(device.as_deref(), p, tx, d, s, r, e.clone());
                let _ = ready_tx.send(stream.as_ref().map(|s| s.1).map_err(Clone::clone));
                match stream {
                    Ok((s, _)) => {
                        let _ = stopped.recv(); // returns once `stop` is dropped
                        drop(s);
                    }
                    Err(msg) => *e.lock().unwrap_or_else(|p| p.into_inner()) = Some(msg),
                }
            })
            .expect("spawn milan-rec");
        Capture { stop: Some(stop), device: Some(device), writer, peaks, dropped, sent, rate, error, path }
    }

    /// Seconds of audio in the file so far; None until the first samples arrive.
    pub fn recorded_secs(&self) -> Option<f64> {
        let (n, rate) = (self.sent.load(Relaxed), self.rate.load(Relaxed));
        (n > 0 && rate > 0).then(|| n as f64 / rate as f64)
    }

    /// Highest |sample| per channel since the last call.
    pub fn take_peaks(&self) -> [f32; 2] {
        [0, 1].map(|c| f32::from_bits(self.peaks[c].swap(0, Relaxed)))
    }

    pub fn error(&self) -> Option<String> {
        self.error.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// Close the device and the file. The take when recording, or why there is none.
    pub fn finish(mut self) -> Result<Take, String> {
        self.close();
        let w = self.writer.take().ok_or("not recording")?;
        let (frames, rate) = w.join().map_err(|_| "the recording writer stopped".to_string())?.map_err(|e| self.error().unwrap_or_else(|| e.to_string()))?;
        let path = self.path.take().ok_or("not recording")?;
        Ok(Take { path, frames, rate, dropped: self.dropped.load(Relaxed) })
    }

    fn close(&mut self) {
        self.stop = None;
        if let Some(d) = self.device.take() {
            let _ = d.join();
        }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        // The writer patches the file when the samples' sender goes, so what was recorded stays readable.
        self.close();
        if let Some(w) = self.writer.take() {
            let _ = w.join();
        }
    }
}

/// Build and start the input stream: samples to stereo f32, peaks, and (when recording) blocks to `tx`.
#[allow(clippy::too_many_arguments)]
fn open(
    name: Option<&str>,
    peaks: Arc<[AtomicU32; 2]>,
    tx: Option<SyncSender<Vec<f32>>>,
    dropped: Arc<AtomicU64>,
    sent: Arc<AtomicU64>,
    rate_out: Arc<AtomicU32>,
    error: Arc<Mutex<Option<String>>>,
) -> Result<(cpal::Stream, u32), String> {
    let host = cpal::default_host();
    let dev = name.and_then(|n| host.input_devices().ok()?.find(|d| d.name().is_ok_and(|x| x == n))).or_else(|| host.default_input_device()).ok_or("No recording device")?;
    let cfg = dev.default_input_config().map_err(|e| format!("Recording device: {e}"))?;
    let (fmt, ch, rate) = (cfg.sample_format(), cfg.channels().max(1) as usize, cfg.sample_rate().0);
    rate_out.store(rate, Relaxed);
    let push = move |stereo: Vec<f32>| {
        for f in stereo.chunks_exact(2) {
            for c in 0..2 {
                peaks[c].fetch_max(f[c].abs().to_bits(), Relaxed);
            }
        }
        if let Some(tx) = &tx {
            let n = stereo.len() as u64 / 2;
            match tx.try_send(stereo) {
                Ok(()) => _ = sent.fetch_add(n, Relaxed),
                Err(_) => _ = dropped.fetch_add(1, Relaxed),
            }
        }
    };
    let err = move |e: cpal::StreamError| {
        // The writer keeps what it has; the app shows this once recording stops.
        *error.lock().unwrap_or_else(|p| p.into_inner()) = Some(format!("Recording device: {e}"));
    };
    let config = cfg.config();
    let stream = match fmt {
        SampleFormat::F32 => dev.build_input_stream(&config, move |d: &[f32], _: &_| push(to_stereo(d, ch, |s| *s)), err, None),
        SampleFormat::I16 => dev.build_input_stream(&config, move |d: &[i16], _: &_| push(to_stereo(d, ch, |s| i16_to_f32(*s))), err, None),
        SampleFormat::U16 => dev.build_input_stream(&config, move |d: &[u16], _: &_| push(to_stereo(d, ch, |s| u16_to_f32(*s))), err, None),
        f => return Err(format!("Recording device: {f:?} samples are not supported")),
    }
    .map_err(|e| format!("Recording device: {e}"))?;
    stream.play().map_err(|e| format!("Recording device: {e}"))?;
    Ok((stream, rate))
}

/// Interleaved `ch`-channel samples as stereo: mono goes to both sides, channels past the second are left out.
fn to_stereo<T>(d: &[T], ch: usize, f: impl Fn(&T) -> f32) -> Vec<f32> {
    d.chunks_exact(ch).flat_map(|fr| [f(&fr[0]), f(&fr[1.min(ch - 1)])]).collect()
}

/// The "milan-rec-wav" thread: waits for the device's rate, then writes blocks and patches the header every second.
/// Any failure (a full disk) is also put in `error`, so the app ends the recording at once.
fn write_wav(file: File, ready: Receiver<Result<u32, String>>, rx: Receiver<Vec<f32>>, error: &Mutex<Option<String>>) -> io::Result<(u64, u32)> {
    let r = write_samples(file, ready, rx);
    if let Err(e) = &r {
        error.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert_with(|| format!("Recording file: {e}"));
    }
    r
}

fn write_samples(file: File, ready: Receiver<Result<u32, String>>, rx: Receiver<Vec<f32>>) -> io::Result<(u64, u32)> {
    let rate = ready.recv().map_err(|_| io::Error::other("the recording device stopped"))?.map_err(io::Error::other)?;
    let mut w = WavWriter::new(BufWriter::new(file), 2, rate)?;
    let mut patched = Instant::now();
    loop {
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(b) => w.write(&b)?,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if patched.elapsed() >= Duration::from_secs(1) {
            w.patch()?;
            patched = Instant::now();
        }
    }
    // Whatever is still queued (the sender is gone, so this ends).
    loop {
        match rx.try_recv() {
            Ok(b) => w.write(&b)?,
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
        }
    }
    w.patch()?;
    Ok((w.frames(), rate))
}

/// A take whose writer failed (a full disk): the header is made to cover every whole stereo frame in the file, so it
/// can still be placed. (frames, rate).
pub fn recover(path: &Path) -> io::Result<(u64, u32)> {
    let mut f = File::options().read(true).write(true).open(path)?;
    let mut h = [0u8; HEADER as usize];
    f.read_exact(&mut h)?;
    let rate = u32::from_le_bytes([h[24], h[25], h[26], h[27]]);
    let frames = (f.metadata()?.len() - HEADER) / 8;
    let data = (frames * 8).min(u32::MAX as u64 - HEADER) as u32;
    // Overwrites header bytes only, so this works on a full disk.
    f.seek(SeekFrom::Start(4))?;
    f.write_all(&(data + HEADER as u32 - 8).to_le_bytes())?;
    f.seek(SeekFrom::Start(40))?;
    f.write_all(&data.to_le_bytes())?;
    Ok((frames, rate))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn wav_header_sizes_and_samples() {
        let mut w = WavWriter::new(Cursor::new(Vec::new()), 2, 48_000).unwrap();
        let h = w.w.get_ref().clone();
        assert_eq!(h.len(), 44);
        assert_eq!(&h[0..4], b"RIFF");
        assert_eq!(&h[8..16], b"WAVEfmt ");
        let u16_at = |b: &[u8], i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
        let u32_at = |b: &[u8], i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
        assert_eq!((u32_at(&h, 16), u16_at(&h, 20), u16_at(&h, 22)), (16, 3, 2)); // fmt size, IEEE float, channels
        assert_eq!((u32_at(&h, 24), u32_at(&h, 28), u16_at(&h, 32), u16_at(&h, 34)), (48_000, 384_000, 8, 32));
        assert_eq!(&h[36..40], b"data");
        let samples: Vec<f32> = (0..2000).map(|i| i as f32 / 2000.0 - 0.5).collect();
        w.write(&samples).unwrap();
        w.patch().unwrap();
        assert_eq!(w.frames(), 1000);
        let b = w.w.into_inner();
        assert_eq!(b.len(), 44 + 8000);
        assert_eq!((u32_at(&b, 4), u32_at(&b, 40)), (36 + 8000, 8000));
        let back: Vec<f32> = b[44..].chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
        assert_eq!(back, samples);
    }

    #[test]
    fn sample_conversion() {
        assert_eq!(i16_to_f32(-32768), -1.0);
        assert_eq!(i16_to_f32(0), 0.0);
        assert_eq!(u16_to_f32(32768), 0.0);
        assert_eq!(u16_to_f32(0), -1.0);
        assert_eq!(to_stereo(&[1.0f32, 2.0], 1, |s| *s), [1.0, 1.0, 2.0, 2.0]);
        assert_eq!(to_stereo(&[1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0], 3, |s| *s), [1.0, 2.0, 4.0, 5.0]);
    }

    #[test]
    fn file_names_never_overwrite() {
        let dir = std::env::temp_dir().join(format!("milan-rec-names-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, mut fa) = create_file(&dir, "Show", "Audio 1").unwrap();
        fa.write_all(b"take one").unwrap();
        let (b, _) = create_file(&dir, "Show", "Audio 1").unwrap();
        let (c, _) = create_file(&dir, "Show", "Vox: L/R").unwrap();
        let names: Vec<String> = [&a, &b, &c].iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        let first = std::fs::read(&a).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(names, ["Show Audio 1 Rec 001.wav", "Show Audio 1 Rec 002.wav", "Show Vox_ L_R Rec 001.wav"]);
        assert_eq!(first, b"take one");
    }

    #[test]
    fn rename_moves_the_file_and_never_overwrites() {
        let dir = std::env::temp_dir().join(format!("milan-rec-rename-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (path, mut f) = create_file(&dir, "Show", "Audio 1").unwrap();
        f.write_all(b"take").unwrap();
        drop(f);
        std::fs::write(dir.join("Taken.wav"), b"other").unwrap();
        let mut p = Project::new("Show");
        p.timeline.audio_tracks[0].armed = true;
        assert!(crate::edit::place_recording(&mut p, &path, 4800, 48_000, 0.0, 0.0));
        let id = p.media_pool[0].id.clone();
        let refused = [rename_file(&mut p, &id, "Taken"), rename_file(&mut p, &id, "a/b"), rename_file(&mut p, &id, " ")];
        let done = rename_file(&mut p, &id, "Vocal take");
        let (moved, other, old_gone) = (std::fs::read(dir.join("Vocal take.wav")).ok(), std::fs::read(dir.join("Taken.wav")).unwrap(), !path.exists());
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(refused.iter().all(Result::is_err));
        assert_eq!(done, Ok(()));
        assert_eq!((moved.as_deref(), other.as_slice(), old_gone), (Some(&b"take"[..]), &b"other"[..], true));
        assert_eq!((p.media_pool[0].name.as_str(), p.timeline.audio_tracks[0].clips[0].name.as_str()), ("Vocal take.wav", "Vocal take.wav"));
        assert_eq!(p.media_pool[0].path, dir.join("Vocal take.wav"));
    }

    #[test]
    fn recover_covers_what_was_written_after_a_failed_writer() {
        let dir = std::env::temp_dir().join(format!("milan-rec-recover-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (path, f) = create_file(&dir, "t", "A").unwrap();
        let mut w = WavWriter::new(f, 2, 44_100).unwrap();
        w.write(&[0.5; 200]).unwrap();
        w.patch().unwrap(); // 100 frames patched, then 50 more and half a frame written before the disk filled
        w.write(&[0.25; 100]).unwrap();
        w.w.write_all(&[0; 4]).unwrap();
        drop(w);
        let got = recover(&path).ok();
        let b = std::fs::read(&path).unwrap();
        let short = std::fs::write(&path, b"RIFF").and_then(|_| recover(&path));
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(got, Some((150, 44_100)));
        let u32_at = |i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
        assert_eq!((u32_at(4), u32_at(40)), (36 + 1200, 1200));
        assert!(short.is_err(), "no header: nothing to place");
    }

    /// Opens the real default input for half a second (run by hand: `cargo test -- --ignored live_capture`).
    #[test]
    #[ignore]
    fn live_capture_writes_a_readable_wav() {
        let dir = std::env::temp_dir().join(format!("milan-rec-live-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = create_file(&dir, "t", "A").unwrap();
        let cap = Capture::start(None, Some(out));
        std::thread::sleep(Duration::from_millis(500));
        let take = cap.finish();
        let len = take.as_ref().ok().and_then(|t| std::fs::metadata(&t.path).ok()).map(|m| m.len());
        std::fs::remove_dir_all(&dir).unwrap();
        let take = take.unwrap();
        assert!(take.frames > take.rate as u64 / 4, "{} frames at {} Hz", take.frames, take.rate);
        assert_eq!(len, Some(HEADER + take.frames * 8));
    }

    #[test]
    fn record_folder() {
        let mut p = Project::new("t");
        assert_eq!(folder(&p), None);
        p.path = Some(PathBuf::from(r"C:\Shows\a.mnle"));
        assert_eq!(folder(&p), Some(PathBuf::from(r"C:\Shows")));
        p.timeline.record_dir = Some(PathBuf::from(r"D:\Takes"));
        assert_eq!(folder(&p), Some(PathBuf::from(r"D:\Takes")));
    }
}
