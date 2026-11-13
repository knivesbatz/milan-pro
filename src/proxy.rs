//! Video proxies (Project Media › Create Video Proxy): small H.264 copies that Draft and Preview quality and the
//! thumbnails decode instead of heavy sources; Good, Best and render always read the source. Proxies belong to files,
//! not projects: `<src>.milanproxy.mp4` beside the source, else %LOCALAPPDATA%\MilanPro\proxies\<hash>.mp4 for
//! read-only folders. VEGAS's own `<src>.sfvp0` is used when ffprobe reads it as the same video.
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use eframe::egui;
use crate::project::{MediaItem, MediaType};

/// Sources with a usable proxy (source -> proxy), and a counter bumped on every change so the engine replans.
// shortcut: a global, so the Thumbs/Preview signatures do not change; pass a handle if a second registry is ever needed.
static READY: Mutex<BTreeMap<PathBuf, PathBuf>> = Mutex::new(BTreeMap::new());
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// The proxy of `src`, if one is known to be usable.
pub fn ready(src: &Path) -> Option<PathBuf> {
    READY.lock().ok()?.get(src).cloned()
}

pub fn generation() -> u64 {
    GENERATION.load(Relaxed)
}

pub(crate) fn set_ready(src: PathBuf, proxy: PathBuf) {
    if let Ok(mut r) = READY.lock() {
        if r.insert(src, proxy.clone()) != Some(proxy) {
            GENERATION.fetch_add(1, Relaxed);
        }
    }
}

/// Stop using the proxy of `src` (deleted, stale or unreadable): Draft and Preview read the source again.
pub fn forget(src: &Path) {
    if let Ok(mut r) = READY.lock() {
        if r.remove(src).is_some() {
            GENERATION.fetch_add(1, Relaxed);
        }
    }
}

/// `<src>.milanproxy.mp4`
pub fn proxy_path(src: &Path) -> PathBuf {
    let mut s = src.as_os_str().to_owned();
    s.push(".milanproxy.mp4");
    s.into()
}

/// `%LOCALAPPDATA%\MilanPro\proxies\<hash of the path>.mp4`, for sources in folders Milan cannot write to.
pub fn fallback_path(src: &Path) -> Option<PathBuf> {
    // FNV-1a over the lower-cased path: stable across builds (DefaultHasher is not), and Windows paths ignore case.
    let h = src.to_string_lossy().to_lowercase().bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3));
    Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("MilanPro").join("proxies").join(format!("{h:016x}.mp4")))
}

/// VEGAS's proxy of `src`: `<src>.sfvp0`.
fn sfvp0_path(src: &Path) -> PathBuf {
    let mut s = src.as_os_str().to_owned();
    s.push(".sfvp0");
    s.into()
}

/// Where an encode writes until it succeeds: `x.mp4` -> `x.partial.mp4` (ffmpeg picks the muxer by extension).
fn partial_path(out: &Path) -> PathBuf {
    out.with_extension("partial.mp4")
}

/// `p` exists and was written after `src` (a proxy older than its source shows old pictures).
fn newer(p: &Path, src: &Path) -> bool {
    let m = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    matches!((m(p), m(src)), (Some(a), Some(b)) if a > b)
}

/// Milan's own fresh proxy of `src` (file checks only).
pub(crate) fn find_own(src: &Path) -> Option<PathBuf> {
    [Some(proxy_path(src)), fallback_path(src)].into_iter().flatten().find(|p| newer(p, src))
}

/// The proxy to use for `m`: Milan's beside the source, then the fallback folder, then VEGAS's `.sfvp0` if ffprobe
/// sees video lasting the source's video length within one frame. Each must be newer than the source.
pub fn find(m: &MediaItem) -> Option<PathBuf> {
    find_own(&m.path).or_else(|| {
        let v = sfvp0_path(&m.path);
        let ok = newer(&v, &m.path) && probe_duration(&v).is_some_and(|d| same_length(d, probe_duration(&m.path).unwrap_or(m.duration), m));
        ok.then_some(v)
    })
}

/// Video duration of `p` as ffprobe reports it (the stream's, else the container's); None without a video stream.
fn probe_duration(p: &Path) -> Option<f64> {
    let mut cmd = crate::proc::ffprobe();
    cmd.args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=duration:format=duration", "-of", "json"]).arg(p);
    let out = crate::proc::output(&mut cmd).ok().filter(|o| o.status.success())?;
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let num = |x: &serde_json::Value| x.as_str()?.parse::<f64>().ok();
    let s = v["streams"].get(0)?;
    num(&s["duration"]).or_else(|| num(&v["format"]["duration"]))
}

/// A proxy's video lasting `d` matches a source whose video lasts `src` within one of its frames.
fn same_length(d: f64, src: f64, m: &MediaItem) -> bool {
    let frame = 1.0 / m.frame_rate.filter(|f| *f > 0.0).unwrap_or(30.0);
    (d - src).abs() <= frame + 1e-6
}

/// Media Create Video Proxy applies to: file-based video without alpha (the H.264 proxy has none, so an overlay
/// would turn opaque at Draft and Preview).
pub fn is_video(m: &MediaItem) -> bool {
    m.media_type == MediaType::Video && m.generator.is_none() && !m.has_alpha
}

/// Preferences › Video "proxy 4K, HEVC and AV1 automatically on import".
pub fn wants_auto(m: &MediaItem) -> bool {
    is_video(m) && (m.width.max(m.height).unwrap_or(0) >= 3840 || matches!(m.codec.as_deref(), Some("hevc" | "av1")))
}

/// ffmpeg arguments (after `-nostdin`): at most 1280 wide, short GOPs without B-frames for fast seeks.
/// - The first audio track comes along (tiny, mono) so the proxy's start time is the source's: decoders seek from the
///   container start, and a video-only proxy of a clip whose audio starts first would run ahead by the gap.
/// - The source's colour matrix is kept, since the decoder reads the proxy with it (RGB sources would shift hue).
/// - Interlaced sources are deinterlaced here; scaling woven fields would smear them.
pub fn args(m: &MediaItem, out: &Path) -> Vec<String> {
    let mut a: Vec<String> = ["-hide_banner", "-v", "error", "-i"].map(String::from).to_vec();
    a.push(m.path.to_string_lossy().into_owned());
    let matrix = crate::engine::plan::matrix(m).map(|x| format!(":in_color_matrix={x}:out_color_matrix={x}")).unwrap_or_default();
    let deint = if m.interlaced { "yadif=deint=interlaced," } else { "" };
    a.extend(["-map", "0:v:0", "-map", "0:a:0?", "-sn", "-dn", "-vf"].map(String::from));
    a.push(format!("{deint}scale='min(1280,iw)':-2:flags=bicubic{matrix},format=yuv420p"));
    a.extend(
        [
            "-c:v", "libx264", "-preset", "veryfast", "-crf", "23", "-g", "15", "-bf", "0", "-tune", "fastdecode", "-fps_mode", "passthrough", "-c:a", "aac", "-ac", "1",
            "-b:a", "32k", "-movflags", "+faststart", "-progress", "pipe:1", "-y",
        ]
        .map(String::from),
    );
    a.push(out.to_string_lossy().into_owned());
    a
}

/// Fraction done from a `-progress` line (`out_time_us=…`), for a source `dur` seconds long.
fn progress(line: &str, dur: f64) -> Option<f32> {
    let us: f64 = line.strip_prefix("out_time_us=")?.trim().parse().ok()?;
    (dur > 0.0).then(|| (us / 1e6 / dur).clamp(0.0, 1.0) as f32)
}

/// An ffmpeg encode writing `partial`.
struct Running {
    src: PathBuf,
    out: PathBuf,
    partial: PathBuf,
    child: Child,
    /// Last stderr line, once ffmpeg has exited.
    err: Option<JoinHandle<String>>,
}

impl Running {
    fn stderr(&mut self) -> String {
        self.err.take().and_then(|j| j.join().ok()).unwrap_or_default()
    }
}

/// One proxy being made on its own thread, so file I/O on a slow or offline share never stalls the UI.
struct Job {
    /// Progress in thousandths, from ffmpeg's stdout.
    done: Arc<AtomicU32>,
    stop: Arc<AtomicBool>,
    thread: JoinHandle<Result<(), String>>,
}

impl Job {
    fn spawn(work: impl FnOnce(&Arc<AtomicU32>, &AtomicBool) -> Result<(), String> + Send + 'static) -> Self {
        let (done, stop) = (Arc::<AtomicU32>::default(), Arc::<AtomicBool>::default());
        let (d, s) = (done.clone(), stop.clone());
        Job { done, stop, thread: std::thread::spawn(move || work(&d, &s)) }
    }
}

/// The Create Video Proxy queue: one encode at a time, at below-normal priority. It belongs to the app, not the
/// project, so jobs carry on through New and Open. Dropping it cancels.
#[derive(Default)]
pub struct ProxyJobs {
    queue: VecDeque<MediaItem>,
    running: Option<(PathBuf, Job)>,
    /// Jobs finished since the queue was last empty, for "2 of 5".
    finished: usize,
    /// Media already looked up for an existing proxy.
    scanned: HashSet<PathBuf>,
}

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;

impl ProxyJobs {
    /// Queue proxies for the video among `items`, skipping media queued or running already. Media that has a proxy is
    /// queued too: the job keeps a proxy that is still fresh and remakes a stale or deleted one.
    pub fn add<'a>(&mut self, items: impl IntoIterator<Item = &'a MediaItem>) {
        for m in items {
            let busy = self.running.as_ref().is_some_and(|(p, _)| *p == m.path) || self.queue.iter().any(|q| q.path == m.path);
            if is_video(m) && !busy {
                self.queue.push_back(m.clone());
            }
        }
    }

    /// Look up existing proxies (Milan's or VEGAS's) of media not seen before, in the background.
    pub fn scan(&mut self, ctx: &egui::Context, pool: &[MediaItem]) {
        let new: Vec<MediaItem> = pool.iter().filter(|m| is_video(m) && !self.scanned.contains(&m.path)).cloned().collect();
        if new.is_empty() {
            return;
        }
        self.scanned.extend(new.iter().map(|m| m.path.clone()));
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            for m in new {
                if let Some(p) = find(&m) {
                    set_ready(m.path, p);
                    ctx.request_repaint();
                }
            }
        });
    }

    /// An encode is running or waiting.
    pub fn busy(&self) -> bool {
        self.running.is_some() || !self.queue.is_empty()
    }

    /// "Creating video proxy 2 of 5: 43%" while busy.
    pub fn status(&self) -> Option<String> {
        let (_, j) = self.running.as_ref()?;
        let n = self.finished + 1;
        Some(format!("Creating video proxy {n} of {}: {}%", n + self.queue.len(), j.done.load(Relaxed) / 10))
    }

    /// Once per UI frame: collect a finished job, then start the next. Returns a failure to report.
    pub fn poll(&mut self) -> Option<String> {
        let mut err = None;
        if let Some((_, j)) = &self.running {
            if !j.thread.is_finished() {
                return None;
            }
            let (src, j) = self.running.take().expect("running");
            err = j.thread.join().unwrap_or_else(|_| Err(format!("Video proxy for {} failed", src.display()))).err();
            self.finished += 1;
        }
        match self.queue.pop_front() {
            Some(m) => self.running = Some((m.path.clone(), Job::spawn(move |done, stop| run(&m, done, stop)))),
            None => self.finished = 0,
        }
        err
    }

    /// Stop the running encode (its partial file is deleted) and drop the queue.
    pub fn cancel(&mut self) {
        self.queue.clear();
        if let Some((_, j)) = self.running.take() {
            j.stop.store(true, Relaxed);
            let _ = j.thread.join();
        }
        self.finished = 0;
    }
}

impl Drop for ProxyJobs {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// A job: keep a fresh proxy (made or found before), else encode one.
fn run(m: &MediaItem, done: &Arc<AtomicU32>, stop: &AtomicBool) -> Result<(), String> {
    if let Some(p) = ready(&m.path).filter(|p| newer(p, &m.path)).or_else(|| find_own(&m.path)) {
        set_ready(m.path.clone(), p);
        return Ok(());
    }
    forget(&m.path);
    wait(start(m, done.clone())?, stop)
}

/// Wait for an encode (killing it once `stop` is set), then finish it.
fn wait(mut r: Running, stop: &AtomicBool) -> Result<(), String> {
    loop {
        if stop.load(Relaxed) {
            let _ = r.child.kill();
        }
        match r.child.try_wait() {
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Ok(Some(s)) => return finish(r, s.success()),
            Err(_) => {
                let _ = r.child.kill();
                let _ = r.child.wait();
                return finish(r, false);
            }
        }
    }
}

/// A finished encode: its partial file becomes the proxy on success and is deleted otherwise.
fn finish(mut r: Running, ok: bool) -> Result<(), String> {
    let name = r.src.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let err = r.stderr();
    if ok {
        match std::fs::rename(&r.partial, &r.out) {
            Ok(()) => {
                set_ready(r.src, r.out);
                return Ok(());
            }
            Err(e) => {
                let _ = std::fs::remove_file(&r.partial);
                return Err(format!("Video proxy for {name} failed: {e}"));
            }
        }
    }
    let _ = std::fs::remove_file(&r.partial);
    Err(format!("Video proxy for {name} failed: {}", if err.is_empty() { "ffmpeg stopped" } else { &err }))
}

/// Start encoding the proxy of `m`, reporting progress into `done`.
fn start(m: &MediaItem, done: Arc<AtomicU32>) -> Result<Running, String> {
    let src = &m.path;
    let name = src.file_name().unwrap_or_default().to_string_lossy().into_owned();
    // Beside the source if that folder is writable, else the fallback folder.
    let beside = proxy_path(src);
    let out = if std::fs::File::create(partial_path(&beside)).is_ok() {
        beside
    } else {
        let f = fallback_path(src).ok_or_else(|| format!("Video proxy for {name} failed: no folder to write it to"))?;
        f.parent().map(std::fs::create_dir_all).transpose().map_err(|e| format!("Video proxy for {name} failed: {e}"))?;
        f
    };
    let partial = partial_path(&out);
    let mut cmd = crate::proc::ffmpeg();
    cmd.args(args(m, &partial)).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);
    }
    let mut child = match crate::proc::spawn(&mut cmd) {
        Ok(c) => c,
        Err(e) => {
            let _ = std::fs::remove_file(&partial);
            return Err(crate::proc::tool_error(&e, "ffmpeg"));
        }
    };
    let (out_pipe, err_pipe) = (child.stdout.take().expect("piped stdout"), child.stderr.take().expect("piped stderr"));
    let dur = m.duration;
    std::thread::spawn(move || {
        for line in BufReader::new(out_pipe).lines().map_while(Result::ok) {
            if let Some(f) = progress(&line, dur) {
                done.store((f * 1000.0) as u32, Relaxed);
            }
        }
    });
    let err = std::thread::spawn(move || BufReader::new(err_pipe).lines().map_while(Result::ok).filter(|l| !l.trim().is_empty()).last().unwrap_or_default());
    Ok(Running { src: src.to_path_buf(), out, partial, child, err: Some(err) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("milan-proxy-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn touch(p: &Path, age_s: u64) {
        let f = std::fs::File::create(p).unwrap();
        f.set_modified(SystemTime::now() - Duration::from_secs(age_s)).unwrap();
    }

    fn video(path: &Path) -> MediaItem {
        MediaItem { path: path.into(), media_type: MediaType::Video, duration: 10.0, frame_rate: Some(25.0), ..Default::default() }
    }

    #[test]
    fn proxy_names() {
        let src = Path::new(r"D:\Footage\C0001.MP4");
        assert_eq!(proxy_path(src), Path::new(r"D:\Footage\C0001.MP4.milanproxy.mp4"));
        assert_eq!(partial_path(&proxy_path(src)), Path::new(r"D:\Footage\C0001.MP4.milanproxy.partial.mp4"));
        assert_eq!(sfvp0_path(src), Path::new(r"D:\Footage\C0001.MP4.sfvp0"));
        if let Some(f) = fallback_path(src) {
            assert!(f.ends_with(Path::new(r"MilanPro\proxies").join(f.file_name().unwrap())));
            let stem = f.file_stem().unwrap().to_string_lossy().into_owned();
            assert!(stem.len() == 16 && stem.chars().all(|c| c.is_ascii_hexdigit()), "{stem}");
            // Windows paths ignore case; another file gets another name.
            assert_eq!(fallback_path(Path::new(r"d:\footage\c0001.mp4")), Some(f.clone()));
            assert_ne!(fallback_path(Path::new(r"D:\Footage\C0002.MP4")), Some(f));
        }
    }

    #[test]
    fn find_ignores_stale_and_prefers_milanproxy() {
        let d = tmp("find");
        let src = d.join("a.mp4");
        touch(&src, 100);
        let m = video(&src);
        assert_eq!(find(&m), None);
        touch(&proxy_path(&src), 200); // older than the source: stale
        touch(&sfvp0_path(&src), 50); // newer, but not readable video, so not used
        assert_eq!(find(&m), None);
        touch(&proxy_path(&src), 10);
        assert_eq!(find(&m), Some(proxy_path(&src)));
        std::fs::remove_dir_all(&d).unwrap();
        // A .sfvp0 counts when its video lasts the source's video length within one frame (1/25 s).
        assert!(same_length(10.03, 10.0, &m) && same_length(9.97, 10.0, &m) && !same_length(10.1, 10.0, &m));
    }

    #[test]
    fn ready_entries_change_the_generation_only_when_they_change() {
        let src = Path::new("gen-test.mp4");
        let g = generation();
        set_ready(src.into(), "gen-test.proxy.mp4".into());
        let g1 = generation();
        assert!(g1 > g);
        set_ready(src.into(), "gen-test.proxy.mp4".into());
        forget(src);
        assert!(ready(src).is_none() && generation() > g1);
    }

    #[test]
    fn encoder_argv() {
        let m = MediaItem { width: Some(1920), height: Some(1080), ..video(Path::new(r"C:\a b\in.mov")) };
        let out = Path::new(r"C:\a b\in.mov.milanproxy.partial.mp4");
        let tail = "-c:v libx264 -preset veryfast -crf 23 -g 15 -bf 0 -tune fastdecode -fps_mode passthrough -c:a aac -ac 1 -b:a 32k -movflags +faststart -progress pipe:1 -y C:\\a b\\in.mov.milanproxy.partial.mp4";
        assert_eq!(
            args(&m, out).join(" "),
            format!(r"-hide_banner -v error -i C:\a b\in.mov -map 0:v:0 -map 0:a:0? -sn -dn -vf scale='min(1280,iw)':-2:flags=bicubic:in_color_matrix=bt709:out_color_matrix=bt709,format=yuv420p {tail}")
        );
        // Interlaced SD (no matrix known: the decoder reads both with auto) is deinterlaced first.
        let sd = MediaItem { width: Some(720), height: Some(576), interlaced: true, ..m.clone() };
        assert!(args(&sd, out).join(" ").contains(" -vf yadif=deint=interlaced,scale='min(1280,iw)':-2:flags=bicubic,format=yuv420p "));
        assert_eq!(progress("out_time_us=2500000", 10.0), Some(0.25));
        assert_eq!(progress("out_time_us=N/A", 10.0), None);
        assert_eq!(progress("frame=12", 10.0), None);
    }

    #[test]
    fn queue_skips_duplicates_audio_alpha_and_auto_picks_heavy_media() {
        let mut j = ProxyJobs::default();
        let (a, b) = (video(Path::new("a.mp4")), video(Path::new("b.mp4")));
        let wav = MediaItem { path: "c.wav".into(), media_type: MediaType::Audio, ..Default::default() };
        let alpha = MediaItem { has_alpha: true, ..video(Path::new("logo.mov")) };
        j.add([&a, &b, &a, &wav, &alpha]);
        j.add([&b]);
        assert_eq!(j.queue.iter().map(|q| q.path.to_string_lossy().into_owned()).collect::<Vec<_>>(), ["a.mp4", "b.mp4"]);
        let uhd = MediaItem { width: Some(2160), height: Some(3840), ..video(Path::new("v.mp4")) };
        let hevc = MediaItem { codec: Some("hevc".into()), width: Some(1920), height: Some(1080), ..video(Path::new("h.mp4")) };
        let hd = MediaItem { codec: Some("h264".into()), width: Some(1920), height: Some(1080), ..video(Path::new("x.mp4")) };
        let hevc_alpha = MediaItem { has_alpha: true, ..hevc.clone() };
        assert!(wants_auto(&uhd) && wants_auto(&hevc) && !wants_auto(&hd) && !wants_auto(&wav) && !wants_auto(&hevc_alpha));
        j.queue.clear();
    }

    /// A process standing in for ffmpeg: exits with `code`, or (None) runs about 30 s.
    #[cfg(windows)]
    fn fake_child(code: Option<i32>) -> Child {
        use std::os::windows::process::CommandExt;
        let mut c = match code {
            Some(code) => {
                let mut c = std::process::Command::new("cmd");
                c.args(["/c", &format!("exit {code}")]);
                c
            }
            None => {
                let mut c = std::process::Command::new("ping");
                c.args(["-n", "30", "127.0.0.1"]);
                c
            }
        };
        c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(CREATE_NO_WINDOW);
        c.spawn().unwrap()
    }

    #[cfg(windows)]
    #[test]
    fn jobs_rename_on_success_and_delete_on_failure_or_cancel() {
        let d = tmp("poll");
        for (code, tag) in [(Some(0), "ok"), (Some(1), "bad"), (None, "cancel")] {
            let src = d.join(format!("{tag}.mp4"));
            let (out, partial) = (proxy_path(&src), partial_path(&proxy_path(&src)));
            std::fs::write(&partial, b"x").unwrap();
            let mut j = ProxyJobs::default();
            let r = Running { src: src.clone(), out: out.clone(), partial: partial.clone(), child: fake_child(code), err: None };
            j.running = Some((src.clone(), Job::spawn(move |_, stop| wait(r, stop))));
            assert!(j.status().is_some_and(|s| s == "Creating video proxy 1 of 1: 0%"));
            let mut err = None;
            if code.is_none() {
                j.cancel();
            }
            let until = std::time::Instant::now() + Duration::from_secs(10);
            while j.busy() {
                err = j.poll();
                assert!(std::time::Instant::now() < until, "timed out");
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(!partial.exists(), "{tag}: the partial file is gone");
            assert_eq!(out.exists(), code == Some(0), "{tag}");
            assert_eq!(ready(&src), (code == Some(0)).then_some(out));
            assert_eq!(err.is_some(), code == Some(1), "{tag}: {err:?}");
            assert!(j.status().is_none());
        }
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// A 1080p clip with the frame number in its luma (`level(n)` on frame n), at 30 fps. With `audio_first`, an AAC
    /// track starts 0.2 s before the video, as in many camera and phone files.
    fn numbered(dir: &Path, secs: u32, audio_first: bool) -> PathBuf {
        let path = dir.join(format!("numbered{secs}{}.mp4", if audio_first { "a" } else { "" }));
        let src = format!("nullsrc=s=64x36:r=30:d={secs},geq=lum='mod(N*4,220)+16':cb=128:cr=128,scale=1920:1080:flags=neighbor,format=yuv420p");
        let mut cmd = crate::proc::ffmpeg();
        cmd.args(["-v", "error", "-y"]);
        if audio_first {
            cmd.args(["-f", "lavfi", "-i", &format!("sine=d={}", secs as f64 + 0.2), "-itsoffset", "0.2"]);
        }
        cmd.args(["-f", "lavfi", "-i", &src]);
        if audio_first {
            cmd.args(["-map", "1:v", "-map", "0:a", "-c:a", "aac"]);
        }
        let ok = cmd.args(["-c:v", "libx264", "-preset", "veryfast", "-qp", "0", "-g", "250"]).arg(&path).status().unwrap().success();
        assert!(ok);
        path
    }

    /// Needs ffmpeg. `cargo test --release -- --ignored proxy_frames_match_the_source`
    #[test]
    #[ignore]
    fn proxy_frames_match_the_source() {
        use crate::decode::Quality;
        use crate::engine::vdecode::{DecoderPool, Pull};
        let d = tmp("e2e");
        for audio_first in [false, true] {
            let src = numbered(&d, 11, audio_first);
            let m = crate::media::probe_media(&src).unwrap();
            let mut j = ProxyJobs::default();
            j.add([&m]);
            let until = std::time::Instant::now() + Duration::from_secs(120);
            while j.busy() {
                assert_eq!(j.poll(), None);
                assert!(std::time::Instant::now() < until, "timed out");
                std::thread::sleep(Duration::from_millis(50));
            }
            let proxy = ready(&src).expect("proxy ready");
            assert_eq!(proxy, proxy_path(&src));
            assert_eq!(find(&m), Some(proxy.clone()));
            let media = |proxy| crate::engine::plan::MediaRef {
                id: "n".into(),
                path: src.clone(),
                w: 1920,
                h: 1080,
                duration: m.duration,
                fps: 30.0,
                still: false,
                has_alpha: false,
                matrix: Some("bt709"),
                codec: m.codec.clone(),
                interlaced: false,
                proxy,
                grid: 0.0,
                offset: 0.0,
                stream: 0,
                flow: false,
            };
            // Same quality (scaler) for both: Preview reads the proxy when there is one, else the source.
            let (with, without) = (media(Some(proxy.clone())), media(None));
            assert_eq!(with.proxy_for(Quality::Preview), Some(proxy.as_path()));
            let mut pool = DecoderPool::default();
            pool.set_rate(30.0, "30");
            for sf in [0i64, 37, 300] {
                let mut pull = |r| match pool.frame(r, sf, 64, 36, Quality::Preview, None) {
                    Pull::Exact(img) => img.pixels[64 * 18 + 32].r(),
                    _ => panic!("no frame at {sf}"),
                };
                let (from_proxy, from_src) = (pull(&with), pull(&without));
                // One frame apart is 4 to 5 levels; the proxy's own loss is 1 or 2.
                assert!(from_proxy.abs_diff(from_src) <= 2, "audio first {audio_first}, frame {sf}: proxy {from_proxy}, source {from_src}");
            }
            // A second Create Video Proxy keeps the fresh proxy.
            j.add([&m]);
            while j.busy() {
                assert_eq!(j.poll(), None);
                std::thread::sleep(Duration::from_millis(10));
            }
            assert_eq!(ready(&src), Some(proxy));
        }
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// Needs ffmpeg. A cancelled encode leaves neither a partial file nor a proxy.
    #[test]
    #[ignore]
    fn cancel_leaves_no_partial() {
        let d = tmp("cancel");
        let src = numbered(&d, 60, false);
        let m = crate::media::probe_media(&src).unwrap();
        let mut j = ProxyJobs::default();
        j.add([&m]);
        assert_eq!(j.poll(), None);
        let partial = partial_path(&proxy_path(&src));
        let until = std::time::Instant::now() + Duration::from_secs(10);
        while !partial.exists() {
            assert!(std::time::Instant::now() < until, "no partial file");
            std::thread::sleep(Duration::from_millis(20));
        }
        std::thread::sleep(Duration::from_millis(500));
        assert!(j.busy());
        j.cancel();
        assert!(!j.busy() && !partial.exists() && !proxy_path(&src).exists() && ready(&src).is_none());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
