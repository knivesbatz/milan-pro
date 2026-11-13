//! Render through the engine: the same plan, compositor and mixer as preview and playback, piped into one ffmpeg encoder.
//! Audio is mixed first (to a temp file when there is video, straight into the encoder when there is not), then every
//! frame is composed at the template's size, rate and quality and written to the encoder's stdin. The output goes to
//! `<out>.partial` (a folder of frames for an image sequence) and replaces `out` only once the encoder has finished.
//! Every render leaves a log (`render::log_dir`): the command, the ffmpeg version, timings and everything ffmpeg said.
use eframe::egui::{self, ColorImage};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Stdio};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, mpsc};
use crate::audio::RATE;
use crate::engine::mix::{self, BLOCK, Mixer};
use crate::engine::plan::{self, VideoPlan, compile_audio};
use crate::engine::worker::Worker;
use crate::project::{FieldOrder, Project, ProjectSettings};
use crate::render::{Ctl, Fit, Job, Opts, Template, fit, is_rgb, utc};

/// Share of the progress bar the audio pass takes before the video pass.
const AUDIO_SHARE: f64 = 0.05;

/// Render `range` (seconds) of a snapshot of `project` to `out` with a copy of `template`. The master gain is
/// `timeline.master.gain`. For an image sequence `out` is the first frame (`<name>_000000.<ext>`).
pub fn start(project: &Project, template: &Template, out: PathBuf, range: (f64, f64), opts: Opts, ctx: egui::Context) -> Job {
    let (p, t) = (project.clone(), template.clone());
    let target = out.clone();
    Job::start(out, ctx, move |ctl| render(&p, &t, &target, range, opts, ctl))
}

/// `<out>.partial`
fn partial(out: &Path) -> PathBuf {
    let mut s = out.as_os_str().to_owned();
    s.push(".partial");
    s.into()
}

/// `%TEMP%\milan-render-<pid>-<n>.<ext>`, unique per render.
fn temp_file(ext: &str) -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!("milan-render-{}-{}.{ext}", std::process::id(), N.fetch_add(1, Relaxed)))
}

/// Delete render temp files (audio, chapters) in `dir` untouched for a day: left by a render that crashed or was
/// killed (Job::drop never ran). A running render's file is newer, or held open by its encoder.
pub fn clean_temp(dir: &Path) {
    let day = std::time::Duration::from_secs(24 * 3600);
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let old = e.metadata().and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().is_ok_and(|a| a > day));
        if name.starts_with("milan-render-") && (name.ends_with(".f32") || name.ends_with(".ffmeta")) && old {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// The image2 pattern of a sequence whose first frame is `first` (`a_000000.png` -> `a_%06d.png`), in `dir`.
fn pattern(first: &Path, dir: &Path) -> PathBuf {
    let name = first.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let (stem, ext) = name.rsplit_once('.').unwrap_or((&name, ""));
    let stem = stem.strip_suffix("_000000").unwrap_or(stem).replace('%', "%%");
    dir.join(format!("{stem}_%06d.{ext}"))
}

/// The output `-vf`: rotation, letterbox, pixel aspect ratio, the BT.709 conversion of YUV outputs, interlacing.
fn video_filter(t: &Template, s: &ProjectSettings, rot: u16, f: &Fit) -> String {
    let Some(v) = &t.video else { return String::new() };
    let mut parts: Vec<String> = Vec::new();
    match rot {
        90 => parts.push("transpose=clock".into()),
        180 => parts.push("hflip,vflip".into()),
        270 => parts.push("transpose=cclock".into()),
        _ => {}
    }
    if f.inner != f.out {
        let bars = if v.alpha() { "black@0" } else { "black" };
        parts.push(format!("pad={}:{}:{}:{}:color={bars}", f.out.0, f.out.1, f.at.0, f.at.1));
    }
    parts.push(format!("setsar=r={:.6}:max=10000", v.par(s)));
    let pix = v.pix_fmt();
    if !is_rgb(pix) {
        let range = if pix.starts_with("yuvj") { "pc" } else { "tv" };
        parts.push(format!("scale=out_color_matrix=bt709:out_range={range}"));
    }
    match v.field(s) {
        FieldOrder::Progressive => {}
        fo => parts.push(format!("format={pix},interlace=scan={}:lowpass=0", if fo == FieldOrder::Upper { "tff" } else { "bff" })),
    }
    parts.join(",")
}

/// ffmpeg arguments (after `-nostdin`). With video: RGBA frames of `f.compose` on stdin at `fps` (composed frames:
/// twice the output rate when interlaced) plus the mixed audio from `audio`; audio only: the mix on stdin. Chapters
/// from `meta`. Output in the template's format to `out` (an image2 pattern for a sequence).
#[allow(clippy::too_many_arguments)]
pub fn encoder_args(t: &Template, s: &ProjectSettings, rot: u16, f: &Fit, fps: &str, audio: &Path, meta: Option<&Path>, out: &Path) -> Vec<String> {
    let st = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let mut a = st(&["-hide_banner", "-loglevel", "repeat+level+info", "-nostats", "-y"]);
    let pcm = st(&["-f", "f32le", "-ar", &RATE.to_string(), "-ac", "2", "-i"]);
    let mut inputs = 1;
    if t.video.is_some() {
        a.extend(st(&["-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{}x{}", f.compose.0, f.compose.1), "-framerate", fps, "-i", "-"]));
        if t.audio.is_some() {
            a.extend(pcm);
            a.push(audio.to_string_lossy().into_owned());
            inputs += 1;
        }
    } else {
        a.extend(pcm);
        a.push("-".into());
    }
    if let Some(m) = meta {
        a.extend(st(&["-f", "ffmetadata", "-i"]));
        a.push(m.to_string_lossy().into_owned());
    }
    if t.video.is_some() {
        a.extend(st(&["-map", "0:v"]));
    }
    if t.audio.is_some() && (t.video.is_some() || meta.is_some()) {
        a.extend(st(&["-map", if t.video.is_some() { "1:a" } else { "0:a" }]));
    }
    if meta.is_some() {
        a.extend(st(&["-map_chapters", &inputs.to_string()]));
    }
    if let Some(v) = &t.video {
        a.extend(st(&["-vf", &video_filter(t, s, rot, f)]));
        if !is_rgb(v.pix_fmt()) {
            a.extend(st(&["-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709"]));
        }
        a.extend(t.video_args(s));
    }
    a.extend(t.audio_args(s));
    a.extend(st(&["-f", t.container().map_or("null", |c| c.muxer)]));
    if t.sequence() {
        a.extend(st(&["-start_number", "0"]));
    }
    a.push(out.to_string_lossy().into_owned());
    a
}

/// What a render's log records besides its outcome.
#[derive(Default)]
struct Log {
    args: Vec<String>,
    stderr: Vec<String>,
    frames: i64,
}

/// Encode into `<out>.partial`, then put it in place of `out`. Nothing but `out` (and the log) is left behind,
/// whatever happens.
fn render(p: &Project, t: &Template, out: &Path, range: (f64, f64), opts: Opts, ctl: &Ctl) -> Result<(), String> {
    let (began, clock) = (std::time::SystemTime::now(), std::time::Instant::now());
    let (part, tmp, meta) = (partial(out), temp_file("f32"), temp_file("ffmeta"));
    ctl.set_temp(vec![part.clone(), tmp.clone(), meta.clone()]);
    let mut log = Log::default();
    let r = t.check().and_then(|()| encode(p, t, range, opts, (&part, &tmp, &meta), ctl, &mut log));
    let _ = std::fs::remove_file(&tmp);
    let _ = std::fs::remove_file(&meta);
    let r = if r.is_err() || ctl.cancelled() {
        let _ = if part.is_dir() { std::fs::remove_dir_all(&part) } else { std::fs::remove_file(&part) };
        r.and(Err("Cancelled".into()))
    } else if t.sequence() {
        // The frames move next to where `out` names the first; a failed move keeps the rest in the .partial folder.
        let dir = out.parent().unwrap_or(Path::new("."));
        let moved = std::fs::read_dir(&part).and_then(|es| es.flatten().try_for_each(|e| std::fs::rename(e.path(), dir.join(e.file_name()))));
        moved.and_then(|()| std::fs::remove_dir(&part)).map_err(|e| format!("The frames are rendered in {} but could not be moved to {}: {e}", part.display(), dir.display()))
    } else {
        // A failed rename keeps the finished render under its .partial name rather than losing it.
        std::fs::rename(&part, out).map_err(|e| format!("The render is finished as {} but could not replace {}: {e}", part.display(), out.display()))
    };
    let status = match &r {
        Ok(()) => "Done".to_string(),
        Err(_) if ctl.cancelled() => "Cancelled".to_string(),
        Err(e) => format!("Failed: {e}"),
    };
    if let Some(path) = write_log(out, t, range, &log, &status, began, clock.elapsed()) {
        ctl.set_log(path);
    }
    r
}

/// The first line of `ffmpeg -version`, asked once.
fn ffmpeg_version() -> &'static str {
    static V: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    V.get_or_init(|| {
        let mut cmd = crate::proc::ffmpeg();
        cmd.arg("-version");
        let first = |o: std::process::Output| String::from_utf8_lossy(&o.stdout).lines().next().map(|l| l.split(" Copyright").next().unwrap_or(l).to_string());
        crate::proc::output(&mut cmd).ok().and_then(first).unwrap_or_else(|| "unknown".into())
    })
}

/// `<log_dir>\<yyyymmdd-hhmmss>-<n> <out file name>.log`; None when it cannot be written (the render is not failed for it).
fn write_log(out: &Path, t: &Template, range: (f64, f64), log: &Log, status: &str, began: std::time::SystemTime, took: std::time::Duration) -> Option<PathBuf> {
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = crate::render::log_dir();
    std::fs::create_dir_all(&dir).ok()?;
    let stamp: String = utc(began).chars().filter(char::is_ascii_digit).collect();
    let name = out.file_name().map_or("render".into(), |n| n.to_string_lossy().into_owned());
    let path = dir.join(format!("{}-{}-{} {name}.log", &stamp[..8], &stamp[8..], N.fetch_add(1, Relaxed)));
    let quote = |a: &String| if a.contains([' ', '\'', '"']) || a.is_empty() { format!("\"{a}\"") } else { a.clone() };
    let secs = took.as_secs_f64();
    let fps = if secs > 0.0 && log.frames > 0 { format!(" ({:.1} frames/s)", log.frames as f64 / secs) } else { String::new() };
    let text = format!(
        "Milan Pro {} render log\nOutput:   {}\nTemplate: {} ({})\nRange:    {:.3} s to {:.3} s\nStarted:  {} UTC\nTook:     {secs:.2} s; {} frames{fps}\nStatus:   {status}\nFFmpeg:   {}\n\nCommand:\nffmpeg -nostdin {}\n\nFFmpeg output:\n{}\n",
        env!("CARGO_PKG_VERSION"),
        out.display(),
        t.name,
        t.format(),
        range.0,
        range.1,
        utc(began),
        log.frames,
        ffmpeg_version(),
        log.args.iter().map(quote).collect::<Vec<_>>().join(" "),
        log.stderr.join("\n"),
    );
    std::fs::write(&path, text).ok().map(|()| path)
}

fn encode(p: &Project, t: &Template, (a, b): (f64, f64), opts: Opts, (part, tmp, meta): (&Path, &Path, &Path), ctl: &Ctl, log: &mut Log) -> Result<(), String> {
    let s = &p.settings;
    let rot = if opts.rotate { s.rotation % 360 } else { 0 };
    // Frames are composed at the template's rate (twice it, one per field, when interlaced) and quality.
    let vp = t.video.as_ref().map(|v| {
        let mut q = p.clone();
        let fields = if v.field(s) == FieldOrder::Progressive { 1.0 } else { 2.0 };
        q.settings.frame_rate = crate::project::exact_fps(v.fps(s)) * fields;
        let mut vp = plan::compile(&q).without_proxies();
        vp.alpha = v.alpha();
        vp
    });
    let frames = vp.as_ref().map(|vp| plan::frame_range((a, b), vp.fps));
    if b <= a || frames.is_some_and(|(n0, n1)| n1 <= n0) {
        return Err("Nothing to render: the range is empty".into());
    }
    let f = t.video.as_ref().map_or(Fit { compose: (2, 2), inner: (2, 2), at: (0, 0), out: (2, 2) }, |v| fit(s, rot, v.size(s, opts.rotate), v.par(s), opts.stretch));
    if vp.is_some() && t.audio.is_some() {
        let file = std::fs::File::create(tmp).map_err(|e| format!("Cannot create the render's temp audio {}: {e}", tmp.display()))?;
        let (mut file, mut err) = (BufWriter::new(file), None);
        // A refused write (disk full) leaves its error in `err`.
        mix_range(p, (a, b), opts.quickfade, ctl, AUDIO_SHARE, |bytes| file.write_all(bytes).map_err(|e| err = Some(e)).is_ok())?;
        if let Some(e) = err.or_else(|| file.flush().err()) {
            return Err(format!("Cannot write the render's temp audio {}: {e}", tmp.display()));
        }
    }
    let chapters = if opts.markers && t.container().is_some_and(|c| c.chapters) { crate::render::chapters(p, (a, b)) } else { None };
    if let Some(c) = &chapters {
        std::fs::write(meta, c).map_err(|e| format!("Cannot write the render's chapters {}: {e}", meta.display()))?;
    }
    let target = if t.sequence() {
        let _ = std::fs::remove_dir_all(part); // frames of a render that crashed
        std::fs::create_dir_all(part).map_err(|e| format!("Cannot create {}: {e}", part.display()))?;
        pattern(part.with_extension("").as_path(), part)
    } else {
        part.to_path_buf()
    };
    let fps = vp.as_ref().map_or_else(|| crate::render::fps_str(s.frame_rate), |vp| vp.fps_str.clone());
    log.args = encoder_args(t, s, rot, &f, &fps, tmp, chapters.is_some().then_some(meta), &target);
    let mut cmd = crate::proc::ffmpeg();
    cmd.args(&log.args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
    let mut child = crate::proc::spawn(&mut cmd).map_err(|e| format!("{}: cannot render", crate::proc::tool_error(&e, "ffmpeg")))?;
    let (mut stdin, stderr) = (child.stdin.take().expect("piped stdin"), child.stderr.take().expect("piped stderr"));
    ctl.set_child(child);
    // stderr drained on its own thread so a chatty ffmpeg never blocks on a full pipe.
    let said = std::thread::spawn(move || BufReader::new(stderr).lines().map_while(Result::ok).collect::<Vec<_>>());
    let alpha = vp.as_ref().is_some_and(|vp| vp.alpha);
    let quality = t.quality.unwrap_or(s.render_quality);
    let fed = match (vp, frames) {
        (Some(vp), Some(n)) => {
            log.frames = n.1 - n.0;
            feed_video(vp, n, f.compose, quality, alpha, stdin, ctl)
        }
        // `move`: stdin closes when the mix ends, which tells the encoder to finish.
        _ => mix_range(p, (a, b), opts.quickfade, ctl, 1.0, move |bytes| stdin.write_all(bytes).is_ok()),
    };
    // stdin is closed: the encoder finishes, or is killed when the frames stopped short.
    if !matches!(fed, Ok(true)) {
        ctl.kill();
    }
    log.stderr = said.join().unwrap_or_default();
    let tail = summary(&log.stderr);
    let status = ctl.take_child().map(|mut c| c.wait());
    let complete = fed?;
    match status {
        Some(Ok(s)) if s.success() && complete => Ok(()),
        Some(Ok(s)) => Err(if tail.is_empty() { format!("ffmpeg exited with {s}") } else { tail }),
        Some(Err(e)) => Err(format!("ffmpeg wait failed: {e}")),
        None => Err("ffmpeg process lost".into()),
    }
}

/// What a failed ffmpeg said, at most 12 lines: its errors (level tags removed; info and warnings are in the log). The
/// first lines name the cause; ffmpeg 9 ends an encoder failure with about seven lines of thread shutdown, which are
/// left out.
fn summary(lines: &[String]) -> String {
    const NOISE: [&str; 3] = ["Task finished", "Terminating thread", "Error sending frames"];
    const QUIET: [&str; 5] = ["[info] ", "[warning] ", "[verbose] ", "[debug] ", "[trace] "];
    let v: Vec<String> = lines
        .iter()
        .filter(|l| !l.trim().is_empty() && !NOISE.iter().any(|n| l.contains(n)) && !QUIET.iter().any(|q| l.contains(q)))
        .map(|l| ["[error] ", "[fatal] ", "[panic] "].iter().fold(l.clone(), |l, tag| l.replace(tag, "")))
        .collect();
    if v.len() <= 12 { v.join("\n") } else { [&v[..6], &["...".to_string()][..], &v[v.len() - 5..]].concat().join("\n") }
}

/// Mix seconds `a..b` at RATE through the master FX (Bypass All Audio FX never applies to a render), then the master
/// gain, clamped to [-1, 1], as f32le stereo into `write` (a master limiter's output never reaches the clamp while the
/// master gain is at or under 0 dB). Ok(false): `write` refused the data. Progress goes 0..`share`.
fn mix_range(p: &Project, (a, b): (f64, f64), quickfade: bool, ctl: &Ctl, share: f64, mut write: impl FnMut(&[u8]) -> bool) -> Result<bool, String> {
    let plan = plan::AudioPlan { quickfade: if quickfade { plan::QUICKFADE } else { 0.0 }, ..compile_audio(p) };
    let gain = p.timeline.master.gain;
    let mut mixer = Mixer::ffmpeg();
    // A render waits for every decoder, however slow, so no gap is baked into the file; Cancel still stops it.
    mixer.render = Some(ctl.cancel_flag());
    let (f0, f1, ahead) = (mix::frame(a), mix::frame(b), RATE as i64);
    // Streams open 1 s before they are needed, so their decoders start in parallel.
    mixer.warm(&plan, f0, (f0 + ahead).min(f1));
    let (mut f, mut buf, mut bytes) = (f0, Vec::new(), Vec::new());
    while f < f1 {
        if ctl.cancelled() {
            return Err("Cancelled".into());
        }
        let n = (f1 - f).min(BLOCK as i64) as usize;
        buf.resize(n * 2, 0.0);
        mixer.block(&plan, f, &mut buf);
        if let Some(e) = mixer.error.take() {
            return Err(e);
        }
        bytes.clear();
        bytes.extend(buf.iter().enumerate().flat_map(|(i, v)| (v * gain[i % 2]).clamp(-1.0, 1.0).to_le_bytes()));
        if !write(&bytes) {
            return Ok(false);
        }
        f += n as i64;
        mixer.warm(&plan, f, (f + ahead).min(f1));
        ctl.progress(share * (f - f0) as f64 / (f1 - f0) as f64);
    }
    Ok(true)
}

/// Compose frames `n0..n1` at `w` x `h` and quality `q` and write them to the encoder, which a writer thread feeds so
/// composing and encoding overlap. No deadlines (nothing is dropped), no composite cache, decoders read forward. With
/// `alpha` the premultiplied pixels are written unmultiplied, as ffmpeg's rgba is. Ok(false): the encoder stopped
/// taking frames. Progress goes AUDIO_SHARE..1.
#[allow(clippy::too_many_arguments)]
fn feed_video(vp: VideoPlan, (n0, n1): (i64, i64), (w, h): (u32, u32), q: crate::decode::Quality, alpha: bool, mut stdin: ChildStdin, ctl: &Ctl) -> Result<bool, String> {
    let fps = vp.fps;
    let (tx, rx) = mpsc::sync_channel::<Arc<ColorImage>>(3);
    let writer = std::thread::spawn(move || {
        rx.iter().all(|img| {
            if alpha {
                let px: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
                stdin.write_all(&px).is_ok()
            } else {
                stdin.write_all(bytemuck::cast_slice(&img.pixels)).is_ok()
            }
        })
    });
    let mut wk = Worker::for_render();
    wk.set_plan(Arc::new(vp));
    let mut r = Ok(true);
    for n in n0..n1 {
        if ctl.cancelled() {
            r = Err("Cancelled".into());
            break;
        }
        let (img, err) = wk.frame(n, w, h, q, None);
        if let Some(e) = err {
            r = Err(format!("Frame {} ({:.3} s): {e}", n - n0, n as f64 / fps));
            break;
        }
        if tx.send(img).is_err() {
            break; // the writer stopped: its result says so
        }
        if (n - n0) % 30 == 29 {
            wk.prune(); // decoders of events already rendered
        }
        ctl.progress(AUDIO_SHARE + (1.0 - AUDIO_SHARE) * (n + 1 - n0) as f64 / (n1 - n0) as f64);
    }
    drop(tx);
    let wrote = writer.join().unwrap_or(false);
    r.map(|ok| ok && wrote)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Clip, MediaItem, Track, TrackKind};
    use crate::render::{BUILTIN, Status, content_end};
    use std::time::{Duration, Instant};

    fn settings() -> ProjectSettings {
        ProjectSettings { width: 1920, height: 1080, frame_rate: 29.97, sample_rate: 44_100, ..Default::default() }
    }

    fn args(t: &Template, s: &ProjectSettings, out: &Path) -> Vec<String> {
        let f = t.video.as_ref().map_or(Fit { compose: (2, 2), inner: (2, 2), at: (0, 0), out: (2, 2) }, |v| fit(s, 0, v.size(s, true), v.par(s), false));
        encoder_args(t, s, 0, &f, "30000/1001", &std::env::temp_dir().join("milan-render-1234-0.f32"), None, out)
    }

    /// An args test per built-in template: each is valid, names its codecs, pixel format and muxer, and maps its streams.
    #[test]
    fn encoder_argv_for_every_template() {
        // A 260-character output path, the most Windows allows without long-path support.
        let out = PathBuf::from(format!(r"C:\{}.mp4", "x".repeat(253)));
        let tmp = std::env::temp_dir().join("milan-render-1234-0.f32");
        let s = settings();
        for t in BUILTIN.iter() {
            assert_eq!(t.check(), Ok(()), "{}", t.name);
            let a = args(t, &s, &partial(&out));
            let joined = a.join(" ");
            let muxer = t.container().unwrap().muxer;
            assert!(joined.ends_with(&format!("-f {muxer} {}.partial", out.display())) || t.sequence(), "{}: {joined}", t.name);
            // The command line does not grow with the timeline: events never appear in it.
            assert!(a.iter().map(|x| x.len() + 3).sum::<usize>() < 1100, "{}: {} characters", t.name, joined.len());
            match (&t.video, &t.audio) {
                (Some(v), au) => {
                    assert!(joined.contains("-f rawvideo -pix_fmt rgba -s 1920x1080 -framerate 30000/1001 -i - ") || v.size.is_some(), "{joined}");
                    assert!(joined.contains(&format!("-c:v {} ", v.codec)) && joined.contains(&format!("-pix_fmt {}", v.pix_fmt())), "{}: {joined}", t.name);
                    assert_eq!(joined.contains(&format!("-f f32le -ar 48000 -ac 2 -i {} -map 0:v -map 1:a", tmp.display())), au.is_some(), "{joined}");
                    assert_eq!(joined.contains("out_color_matrix=bt709") && joined.contains("-colorspace bt709"), !crate::render::is_rgb(v.pix_fmt()), "{joined}");
                }
                (None, _) => {
                    assert!(!joined.contains("rawvideo") && !joined.contains("-map") && !joined.contains(".f32"), "{joined}");
                    assert!(joined.contains("-f f32le -ar 48000 -ac 2 -i - "));
                }
            }
            if let Some(au) = &t.audio {
                assert!(joined.contains(" -c:a ") && joined.contains(&format!("-ac {}", au.channels)), "{}: {joined}", t.name);
            }
        }
        let find = |n: &str| BUILTIN.iter().find(|t| t.name == n).unwrap();
        let j = |n: &str| args(find(n), &s, Path::new("o")).join(" ");
        assert!(j("YouTube 1080p").contains("-c:v libx264 -b:v 16000k -maxrate 24000k -bufsize 48000k -preset medium -pix_fmt yuv420p"));
        assert!(j("YouTube 2160p").contains("-s 3840x2160 "));
        assert!(j("ProRes 4444 (alpha)").contains("-profile:v 4 -pix_fmt yuva444p10le -vendor apl0"));
        assert!(j("MP4 H.265 (HEVC)").contains("-tag:v hvc1"));
        assert!(j("VP9 WebM").contains("-crf 31 -b:v 0 -deadline good -pix_fmt yuv420p -row-mt 1") && j("VP9 WebM").contains("-c:a libopus -b:a 192k -ar 48000"));
        assert!(j("FLAC audio").contains("-c:a flac -sample_fmt s16 -ar 44100"));
        assert!(j("AIFF audio").contains("-c:a pcm_s16be") && j("WAV audio only").contains("-c:a pcm_s16le"));
        assert!(j("MXF DNxHR HQ").contains("-profile:v dnxhr_hq") && j("MXF DNxHR HQ").contains("-ar 48000"));
        assert!(j("AVI (MJPEG)").contains("out_range=pc") && j("AVI (MJPEG)").contains("-q:v 3"));
        assert!(j("PNG sequence").contains("-f image2 -start_number 0 o") && !j("PNG sequence").contains("-colorspace"));
        // The project's bit depth reaches PCM and FLAC renders that follow it.
        let s24 = ProjectSettings { bit_depth: 24, ..settings() };
        assert!(args(find("WAV audio only"), &s24, Path::new("o")).join(" ").contains("-c:a pcm_s24le"));
        assert!(args(find("FLAC audio"), &s24, Path::new("o")).join(" ").contains("-sample_fmt s32 -bits_per_raw_sample 24"));
        // Chapters come in as a third input, mapped.
        let f = fit(&s, 0, (1920, 1080), 1.0, false);
        let a = encoder_args(&BUILTIN[0], &s, 0, &f, "30", Path::new("a.f32"), Some(Path::new("c.ffmeta")), Path::new("o")).join(" ");
        assert!(a.contains("-i a.f32 -f ffmetadata -i c.ffmeta -map 0:v -map 1:a -map_chapters 2 "), "{a}");
    }

    /// MP3 stops at 48 kHz and AAC at 96 kHz; a 96 or 192 kHz project renders at the codec's highest rate.
    #[test]
    fn sample_rate_is_capped_per_codec() {
        let ar = |k: usize, rate| {
            let s = ProjectSettings { sample_rate: rate, ..settings() };
            let a = args(&BUILTIN[k], &s, Path::new("o"));
            a.windows(2).filter(|w| w[0] == "-ar").map(|w| w[1].clone()).last().unwrap()
        };
        assert_eq!(ar(4, 96_000), "48000");
        assert_eq!(ar(4, 44_100), "44100");
        assert_eq!(ar(0, 192_000), "96000");
        assert_eq!(ar(1, 88_200), "88200");
        assert_eq!(ar(3, 192_000), "192000");
    }

    /// Interlaced output composes two frames per output frame and weaves them; rotation and letterbox come first.
    #[test]
    fn interlace_rotation_and_letterbox_filters() {
        let mut t = BUILTIN[0].clone();
        let v = t.video.as_mut().unwrap();
        (v.size, v.field) = (Some((1920, 1080)), Some(FieldOrder::Lower));
        let s = ProjectSettings { width: 1080, height: 1920, rotation: 90, ..settings() };
        // Rotation off: a portrait project pillarboxed into 1920x1080.
        let f = fit(&s, 0, (1920, 1080), 1.0, false);
        assert_eq!((f.compose, f.at), ((608, 1080), (656, 0)));
        let vf = video_filter(&t, &s, 0, &f);
        assert_eq!(vf, "pad=1920:1080:656:0:color=black,setsar=r=1.000000:max=10000,scale=out_color_matrix=bt709:out_range=tv,format=yuv420p,interlace=scan=bff:lowpass=0");
        assert!(t.video_args(&s).join(" ").contains("-flags +ildct+ilme -field_order bb -x264-params bff=1"));
        // Rotation on: the portrait project fills the landscape frame, composed at its own (portrait) shape.
        let f = fit(&s, 90, (1920, 1080), 1.0, false);
        assert_eq!((f.compose, f.inner), ((1080, 1920), (1920, 1080)));
        assert!(video_filter(&t, &s, 90, &f).starts_with("transpose=clock,setsar"));
        // Stretch fills whatever the shape; a 16:9 project in NTSC DV widescreen (1.2121) is nearly full.
        assert_eq!(fit(&s, 0, (1920, 1080), 1.0, true).compose, (1920, 1080));
        let f = fit(&settings(), 0, (720, 480), 1.2121, false);
        assert_eq!((f.compose, f.at), ((704, 480), (8, 0)));
        // A template that cannot be interlaced is refused.
        let mut vp9 = BUILTIN.iter().find(|t| t.name == "VP9 WebM").unwrap().clone();
        vp9.video.as_mut().unwrap().field = Some(FieldOrder::Upper);
        assert_eq!(vp9.check(), Err("VP9 cannot be interlaced.".into()));
    }

    #[test]
    fn sequence_pattern_escapes_percent() {
        assert_eq!(pattern(Path::new(r"C:\v\Shot 100%_000000.png"), Path::new(r"C:\v\x.partial")), Path::new(r"C:\v\x.partial\Shot 100%%_%06d.png"));
    }

    /// The cause (first line) survives ffmpeg 9's list of supported rates and its thread-shutdown lines, with or
    /// without level tags; info lines (kept in the log) are left out.
    #[test]
    fn failure_message_keeps_the_cause() {
        let mut lines = vec!["[aac @ 0x1] Specified sample rate 192000 is not supported by the aac encoder".to_string()];
        lines.extend(["[aac @ 0x1] Supported sample rates are:", "96000", "88200", "64000", "48000", "44100", "32000", "24000", "22050", "16000", "12000", "11025", "8000", "7350"].map(String::from));
        lines.push("[aost#0:0/aac @ 0x2] Error while opening encoder - maybe incorrect parameters such as bit_rate, rate, width or height.".into());
        lines.extend(["[af#0:0 @ 0x3] Error sending frames to consumers: Invalid argument", "[af#0:0 @ 0x3] Task finished with error code: -22 (Invalid argument)", "[af#0:0 @ 0x3] Terminating thread with return code -22 (Invalid argument)"].map(String::from));
        lines.push("[out#0/mp4 @ 0x4] Nothing was written into output file, because at least one of its streams received no packets.".into());
        let s = summary(&lines);
        assert!(s.starts_with("[aac @ 0x1] Specified sample rate 192000"), "{s}");
        assert!(s.lines().count() <= 12 && s.contains("Error while opening encoder") && !s.contains("Terminating thread"), "{s}");
        assert_eq!(summary(&["a".into(), "".into(), "b".into()]), "a\nb");
        let tagged = ["[info] Input #0, lavfi", "[info] Stream mapping:", "[aac @ 0x1] [error] Specified sample rate 192000 is not supported"].map(String::from);
        assert_eq!(summary(&tagged), "[aac @ 0x1] Specified sample rate 192000 is not supported");
    }

    /// Temp files of a render that crashed (a day old) are swept at start-up; a running render's are left alone.
    #[test]
    fn old_render_temp_files_are_swept() {
        let dir = std::env::temp_dir().join(format!("milan-sweep-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = |name: &str, age_h: u64| {
            let p = dir.join(name);
            std::fs::write(&p, b"x").unwrap();
            std::fs::File::options().write(true).open(&p).unwrap().set_modified(std::time::SystemTime::now() - Duration::from_secs(age_h * 3600)).unwrap();
            p
        };
        let (old, meta, fresh, other) = (file("milan-render-1-0.f32", 30), file("milan-render-1-1.ffmeta", 30), file("milan-render-2-0.f32", 0), file("other-0.f32", 30));
        clean_temp(&dir);
        assert!(!old.exists() && !meta.exists() && fresh.exists() && other.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn partial_name() {
        assert_eq!(partial(Path::new(r"C:\v\a.b.mp4")), Path::new(r"C:\v\a.b.mp4.partial"));
    }

    fn wait(job: &Job) -> Status {
        while job.status() == Status::Running {
            std::thread::sleep(Duration::from_millis(20));
        }
        job.status()
    }

    /// f32 stereo at 48 kHz of a rendered file.
    fn pcm(path: &Path) -> Vec<f32> {
        let mut cmd = crate::proc::ffmpeg();
        cmd.args(["-v", "error", "-i"]).arg(path).args(["-vn", "-ac", "2", "-ar", "48000", "-f", "f32le", "-"]);
        crate::proc::output(&mut cmd).unwrap().stdout.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect()
    }

    /// RGBA of frame `k` of a rendered file, decoded back with its BT.709 limited-range tags.
    fn decoded_frame(path: &Path, k: i64) -> Vec<u8> {
        let mut cmd = crate::proc::ffmpeg();
        cmd.args(["-v", "error", "-i"]).arg(path);
        cmd.args(["-vf", &format!("select=eq(n\\,{k}),scale=in_color_matrix=bt709:in_range=tv:flags=bicubic+accurate_rnd+full_chroma_int,format=rgba"), "-frames:v", "1", "-f", "rawvideo", "-"]);
        crate::proc::output(&mut cmd).unwrap().stdout
    }

    fn temp_audio_left() -> usize {
        let prefix = format!("milan-render-{}-", std::process::id());
        std::fs::read_dir(std::env::temp_dir())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix) && e.file_name().to_string_lossy().ends_with(".f32"))
            .count()
    }

    /// Needs ffmpeg. `cargo test --release -- --ignored render_e2e`
    #[test]
    #[ignore]
    fn render_e2e() {
        let dir = std::env::temp_dir().join(format!("milan-render-e2e-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let make = |name: &str, src: &str| {
            let path = dir.join(name);
            let ok = crate::proc::ffmpeg()
                .args(["-v", "error", "-y", "-f", "lavfi", "-i", src, "-f", "lavfi", "-i", "sine=f=440:d=3"])
                .args(["-c:v", "libx264", "-qp", "0", "-pix_fmt", "yuv444p", "-c:a", "aac", "-shortest"])
                .arg(&path)
                .status()
                .unwrap()
                .success();
            assert!(ok);
            crate::media::probe_media(&path).unwrap()
        };
        let clip = |m: &MediaItem, pos: f64, dur: f64| {
            let mut c = Clip::from_media(m, pos);
            (c.duration, c.out_point) = (dur, c.in_point + dur);
            c
        };
        // Three layers: a picture at the bottom, a half-level one above it and a fading one on top; one audio event.
        let a = make("a.mp4", "testsrc2=d=3:s=320x240:r=30");
        let b = make("b.mp4", "smptebars=d=3:s=160x120:r=30");
        let c = make("c.mp4", "mandelbrot=s=320x240:r=30,trim=duration=3");
        let mut p = Project::new("e2e");
        (p.settings.width, p.settings.height, p.settings.frame_rate) = (320, 240, 30.0);
        p.timeline.video_tracks = (0..3).map(|i| Track::new(&format!("Video {i}"), TrackKind::Video)).collect();
        p.timeline.video_tracks[2].clips.push(clip(&a, 0.0, 3.0));
        p.timeline.video_tracks[1].clips.push(clip(&b, 0.5, 2.5));
        p.timeline.video_tracks[1].volume = 0.5;
        let mut top = clip(&c, 1.0, 2.0);
        top.fade_in = 1.0;
        p.timeline.video_tracks[0].clips.push(top);
        p.timeline.audio_tracks[0].clips.push(clip(&a, 0.0, 3.0));
        p.media_pool = vec![a, b, c];
        // R4a, in the parity frames below: a velocity ramp into a freeze on the bottom event, a Fade to Color envelope
        // on the middle track and a transition out to nothing on the top event's fade-out.
        {
            use crate::project::{Curve, EnvKind, Envelope, Keyframe, Keyframes, Transition};
            let env = |kind, keys: &[(f64, f32, Curve)]| Envelope { kind, points: Keyframes { keys: keys.iter().map(|&(t, v, curve)| Keyframe { t, v, curve }).collect() } };
            let vel = env(EnvKind::Velocity, &[(0.0, 1.0, Curve::Linear), (1.5, 0.5, Curve::Hold), (2.0, 0.0, Curve::Hold)]);
            p.timeline.video_tracks[2].clips[0].envelopes.push(vel);
            p.timeline.video_tracks[1].envelopes.push(env(EnvKind::FadeToColor, &[(0.5, 0.0, Curve::Linear), (2.5, -1.0, Curve::Linear)]));
            p.timeline.video_tracks[1].fade_colors[1] = [0, 128, 255];
            let t = &mut p.timeline.video_tracks[0].clips[0];
            (t.fade_out, t.transition_out) = (1.0, Some(Transition { plugin: "milan.tr.linear_wipe".into(), ..Default::default() }));
            // R7: a feathered negative mask on the top event and a 2D Glow on its track; Hard Light on the middle track;
            // the bottom track moving under a Video Bus Track Motion Blur Amount.
            let path = vec![[0.0, 0.0, 0.0, 0.0, 0.0, 0.0], [0.4, 0.0, 0.0, 0.0, 0.0, 0.0], [0.3, 1.0, 0.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0, 0.0, 0.0]];
            let mask = crate::project::Mask { path: Keyframes { keys: vec![Keyframe { t: 0.0, v: path, curve: Curve::Linear }] }, mode: crate::project::MaskMode::Negative, opacity: 1.0, feather: 0.05 };
            p.timeline.video_tracks[0].clips[0].masks = vec![mask];
            p.timeline.video_tracks[0].glow.on = true;
            p.timeline.video_tracks[1].composite = crate::project::Composite::HardLight;
            let m = |x| crate::project::Motion { x, w: 0.8, h: 0.8, ..Default::default() };
            p.timeline.video_tracks[2].motion = Keyframes { keys: vec![Keyframe { t: 0.0, v: m(-0.1), curve: Curve::Linear }, Keyframe { t: 3.0, v: m(0.1), curve: Curve::Linear }] };
            p.timeline.master.video_envelopes = vec![env(EnvKind::MotionBlur, &[(0.0, 0.1, Curve::Linear)])];
        }
        assert_eq!(content_end(&p), 3.0);
        let run = |p: &Project, t: &Template, name: &str, range: (f64, f64)| {
            let out = dir.join(name);
            let job = start(p, t, out.clone(), range, Opts::default(), egui::Context::default());
            assert_eq!(wait(&job), Status::Done, "{}", t.name);
            // R11: every render leaves its log, with the command and ffmpeg's own words.
            let log = std::fs::read_to_string(job.log().expect("a log")).unwrap();
            assert!(log.contains("Status:   Done") && log.contains("ffmpeg -nostdin -hide_banner") && log.contains("[info]"), "{log}");
            assert!(!partial(&out).exists());
            out
        };

        for (i, t) in BUILTIN[..5].iter().enumerate() {
            let out = run(&p, t, &format!("out{i}.{}", t.ext()), (0.0, 3.0));
            let m = crate::media::probe_media(&out).unwrap();
            assert!((m.duration - 3.0).abs() < 0.1, "{} duration {}", t.name, m.duration);
            assert_eq!(m.sample_rate, Some(48_000), "{}", t.name);
            if t.video.is_some() {
                assert_eq!((m.width, m.height), (Some(320), Some(240)), "{}", t.name);
            }
        }

        // The loop region only.
        let m = crate::media::probe_media(&run(&p, &BUILTIN[0], "loop.mp4", (1.0, 2.0))).unwrap();
        assert!((m.duration - 1.0).abs() < 0.07, "loop render {}", m.duration);

        // Parity: the ProRes render's frames are the compositor's frames at the project's rendering quality (Good, VEGAS's
        // default, for a new project).
        let out = dir.join("out2.mov");
        let mut wk = Worker::for_render();
        wk.set_plan(Arc::new(plan::compile(&p)));
        for k in [0i64, 20, 45, 80] {
            let (want, err) = wk.frame(k, 320, 240, p.settings.render_quality, None);
            assert!(err.is_none());
            let got = decoded_frame(&out, k);
            assert_eq!(got.len(), 320 * 240 * 4);
            let want: &[u8] = bytemuck::cast_slice(&want.pixels);
            // Compared in luma: ProRes 4:2:2 halves the chroma, which costs more than 2/255 on the Mandelbrot's detail
            // by itself; a wrong colour matrix or range still shows in luma.
            let y = |p: &[u8]| 0.2126 * p[0] as f64 + 0.7152 * p[1] as f64 + 0.0722 * p[2] as f64;
            let mae = got.chunks_exact(4).zip(want.chunks_exact(4)).map(|(g, w)| (y(g) - y(w)).abs()).sum::<f64>() / (320.0 * 240.0);
            assert!(mae <= 2.0, "frame {k}: mean absolute luma error {mae:.2}/255");
            // And in RGB over 8x8 block means: the subsampled chroma averages out there, a swapped or mis-scaled Cb/Cr does not.
            let blocks = |p: &[u8]| {
                let mut m = vec![0.0f64; 40 * 30 * 3];
                for (i, px) in p.chunks_exact(4).enumerate() {
                    for c in 0..3 {
                        m[(i / 320 / 8 * 40 + i % 320 / 8) * 3 + c] += px[c] as f64 / 64.0;
                    }
                }
                m
            };
            let (g, w) = (blocks(&got), blocks(want));
            let mae = g.iter().zip(&w).map(|(a, b)| (a - b).abs()).sum::<f64>() / g.len() as f64;
            assert!(mae <= 2.0, "frame {k}: mean absolute RGB error of 8x8 blocks {mae:.2}/255");
        }
        drop(wk);

        // Master gain 0.5 with neutral master FX halves the rendered audio.
        let full = pcm(&dir.join("out3.wav"));
        p.timeline.master.gain = [0.5, 0.5];
        p.timeline.master.audio_fx = vec![crate::engine::afx::test_fx("milan.afx.eq", &[]), crate::engine::afx::test_fx("milan.afx.compressor", &[])];
        let half = pcm(&run(&p, &BUILTIN[3], "half.wav", (0.0, 3.0)));
        let peak = |v: &[f32]| v.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(peak(&full) > 0.05 &&(peak(&half) / peak(&full) - 0.5).abs() < 0.01, "{} vs {}", peak(&half), peak(&full));
        // R8a: the audio track through Bus A at -6 dB (plus a pre-volume send to a reverb chain) renders as playback mixes
        // it: the playback mixer over the same range gives the rendered samples (within 16-bit WAV rounding).
        let mut bused = p.clone();
        bused.timeline.master.gain = [1.0, 1.0];
        crate::edit::set_bus_count(&mut bused, 1);
        let bus = bused.timeline.audio.buses[0].id.clone();
        let tid = bused.timeline.audio_tracks[0].id.clone();
        crate::edit::set_track_out(&mut bused, &[tid], &crate::project::Out::Bus(bus.clone()));
        bused.timeline.audio.buses[0].volume = crate::engine::keyframes::db_to_gain(-6.0206);
        let fx = crate::edit::insert_assignable_fx(&mut bused, vec![crate::engine::afx::test_fx("milan.afx.reverb", &[])]).unwrap();
        crate::edit::set_send(&mut bused.timeline.audio_tracks[0], &fx, 0.25, true);
        let parity = |p: &Project, name: &str| {
            let rendered = pcm(&run(p, &BUILTIN[3], name, (0.0, 3.0)));
            let plan = plan::AudioPlan { quickfade: plan::QUICKFADE, ..plan::compile_audio(p) };
            let mut mixer = crate::engine::mix::Mixer::ffmpeg();
            mixer.render = Some(Arc::new(std::sync::atomic::AtomicBool::new(false))); // every decoder waited for
            let mut played = vec![0.0f32; 2 * 3 * 48_000];
            for (i, b) in played.chunks_mut(2 * crate::engine::mix::BLOCK).enumerate() {
                mixer.block(&plan, (i * crate::engine::mix::BLOCK) as i64, b);
            }
            drop(mixer);
            let n = rendered.len().min(played.len());
            assert!(n > 2 * 47_000, "{n}");
            let worst = rendered[..n].iter().zip(&played[..n]).fold(0.0f32, |m, (a, b)| m.max((a - b.clamp(-1.0, 1.0)).abs()));
            assert!(worst < 2.0 / 32768.0, "{name}: render and playback differ by {worst}");
            rendered
        };
        parity(&bused, "bus.wav");
        // R8c: FX automation (a swept EQ band on the track, a bus compressor threshold) and an event Pan envelope render
        // as playback mixes them, and change the sound.
        let mut autod = bused.clone();
        {
            use crate::project::{Curve, EnvKind, Envelope, Keyframe, Keyframes};
            let ramp = |a: f32, b: f32| Keyframes { keys: vec![Keyframe { t: 0.0, v: a, curve: Curve::Linear }, Keyframe { t: 3.0, v: b, curve: Curve::Linear }] };
            let t = &mut autod.timeline.audio_tracks[0];
            let mut eq = crate::engine::afx::test_fx("milan.afx.eq", &[("b2.freq", 440.0)]);
            eq.auto.insert("b2.gain".into(), ramp(-12.0, 12.0));
            t.fx.push(eq);
            t.clips[0].envelopes.push(Envelope { kind: EnvKind::Pan, points: ramp(-0.5, 0.5) });
            let mut comp = crate::engine::afx::test_fx("milan.afx.compressor", &[("ratio", 4.0)]);
            comp.auto.insert("threshold".into(), ramp(0.0, -40.0));
            autod.timeline.audio.buses[0].fx.push(comp);
        }
        let with_auto = parity(&autod, "auto.wav");
        assert!(with_auto.iter().zip(&pcm(&dir.join("bus.wav"))).any(|(a, b)| (a - b).abs() > 0.01), "the automation is heard");
        bused.timeline.audio.fx.clear(); // the send goes with its chain: then the bus halves the track
        bused.timeline.audio_tracks[0].sends.clear();
        let via_bus = pcm(&run(&bused, &BUILTIN[3], "bus_half.wav", (0.0, 3.0)));
        assert!((peak(&via_bus) / peak(&full) - 0.5).abs() < 0.01, "{} vs {}", peak(&via_bus), peak(&full));
        // A master limiter at -1 dB, driven 24 dB into it, never lets the render past -1 dBFS.
        let mut loud = p.clone();
        loud.timeline.master.gain = [1.0, 1.0];
        loud.timeline.master.audio_fx = vec![crate::engine::afx::test_fx("milan.afx.limiter", &[("input", 24.0), ("ceiling", -1.0)])];
        let limited = peak(&pcm(&run(&loud, &BUILTIN[3], "limited.wav", (0.0, 3.0))));
        assert!(limited <= 10f32.powf(-1.0 / 20.0) + 1e-4 && limited > 0.85, "{limited}");

        // Cancel mid-render, then drop a running job (Milan closing): no output, no .partial, no temp audio.
        let mut long = p.clone();
        let c = &mut long.timeline.video_tracks[2].clips[0];
        (c.duration, c.out_point) = (600.0, 600.0); // the last frame holds: long enough to still be running
        for (k, drop_it) in [(0, false), (1, true)] {
            let out = dir.join(format!("cancelled{k}.mp4"));
            let job = start(&long, &BUILTIN[1], out.clone(), (0.0, 600.0), Opts::default(), egui::Context::default());
            let until = Instant::now() + Duration::from_secs(30);
            while !(job.fraction() > AUDIO_SHARE as f32 && partial(&out).exists()) {
                assert!(job.status() == Status::Running && Instant::now() < until, "{:?}", job.status());
                std::thread::sleep(Duration::from_millis(20));
            }
            if drop_it {
                drop(job);
            } else {
                job.cancel();
                assert_eq!(wait(&job), Status::Cancelled);
            }
            assert!(!out.exists() && !partial(&out).exists(), "cancel {k} left output");
        }
        assert_eq!(temp_audio_left(), 0);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// ffprobe's view of a rendered file: streams, chapters, format duration; video frames counted.
    fn probe(path: &Path) -> serde_json::Value {
        let mut cmd = crate::proc::ffprobe();
        cmd.args(["-v", "error", "-count_frames", "-show_entries", "stream=codec_name,codec_type,width,height,r_frame_rate,channels,sample_rate,field_order,pix_fmt,sample_aspect_ratio,nb_read_frames:format=duration", "-show_chapters", "-of", "json"]).arg(path);
        serde_json::from_slice(&crate::proc::output(&mut cmd).unwrap().stdout).unwrap()
    }

    fn stream<'a>(j: &'a serde_json::Value, kind: &str) -> Option<&'a serde_json::Value> {
        j["streams"].as_array()?.iter().find(|s| s["codec_type"] == kind)
    }

    /// R11: 2 s with every built-in template, each output checked with ffprobe (codec, size, rate, channels, length to
    /// a frame); the results go into docs/FORMAT_SUPPORT.md's render table. Then interlacing, rotation, letterbox,
    /// alpha, chapters, a frame rate conform, and a refused combination. Needs ffmpeg.
    /// `cargo test --release -- --ignored render_templates_e2e`
    #[test]
    #[ignore]
    fn render_templates_e2e() {
        let dir = std::env::temp_dir().join(format!("milan-render-tpl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("src.mp4");
        let ok = crate::proc::ffmpeg()
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=d=3:s=320x240:r=30", "-f", "lavfi", "-i", "sine=f=440:d=3"])
            .args(["-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"])
            .arg(&src)
            .status()
            .unwrap()
            .success();
        assert!(ok);
        let m = crate::media::probe_media(&src).unwrap();
        let mut p = Project::new("tpl");
        (p.settings.width, p.settings.height, p.settings.frame_rate) = (320, 240, 30.0);
        let mut c = Clip::from_media(&m, 0.0);
        (c.duration, c.out_point) = (3.0, c.in_point + 3.0);
        p.timeline.video_tracks[0].clips.push(c.clone());
        p.timeline.audio_tracks[0].clips.push(c);
        p.media_pool = vec![m];
        p.timeline.markers.push(crate::project::Marker { position: 0.5, name: "Intro = start".into(), color: [255, 0, 0] });
        p.timeline.regions.push(crate::project::Region { start: 1.0, end: 1.5, name: "Chorus".into() });
        let render = |p: &Project, t: &Template, name: &str, range: (f64, f64), opts: Opts| {
            let out = dir.join(name);
            let job = start(p, t, out.clone(), range, opts, egui::Context::default());
            (wait(&job), out, job.log())
        };
        let mut table = String::from("| Template | Format | Video | Audio | Checked with ffprobe |\n|---|---|---|---|---|\n");
        for t in BUILTIN.iter() {
            let first = if t.sequence() { format!("{}_000000.{}", t.name.replace(' ', "_"), t.ext()) } else { format!("{}.{}", t.name.replace([' ', '(', ')'], "_"), t.ext()) };
            let (status, out, log) = render(&p, t, &first, (0.0, 2.0), Opts::default());
            assert_eq!(status, Status::Done, "{}: {}", t.name, log.map(|l| std::fs::read_to_string(l).unwrap()).unwrap_or_default());
            assert!(!partial(&out).exists(), "{}", t.name);
            let j = probe(&out);
            let s = &p.settings;
            let mut seen = Vec::new();
            if let Some(v) = &t.video {
                let vs = stream(&j, "video").unwrap_or_else(|| panic!("{}: no video {j}", t.name));
                let want = match v.codec.as_str() {
                    "libx264" => "h264",
                    "libx265" => "hevc",
                    "libsvtav1" => "av1",
                    "libvpx-vp9" => "vp9",
                    "libvpx" => "vp8",
                    "mpeg2video" => "mpeg2video",
                    "prores_ks" => "prores",
                    "dnxhd" => "dnxhd",
                    "tiff" => "tiff",
                    c => c,
                };
                assert_eq!(vs["codec_name"], want, "{}", t.name);
                let (w, h) = v.size(s, true);
                assert_eq!((vs["width"].as_u64(), vs["height"].as_u64()), (Some(w as u64), Some(h as u64)), "{}", t.name);
                if t.sequence() {
                    let n = std::fs::read_dir(&dir).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with(&t.name.replace(' ', "_"))).count();
                    assert_eq!(n, 60, "{}: frames", t.name);
                    for k in 0..60 {
                        let _ = std::fs::remove_file(dir.join(format!("{}_{k:06}.{}", t.name.replace(' ', "_"), t.ext())));
                    }
                    seen.push(format!("{} {w}x{h}, 60 files", vs["codec_name"].as_str().unwrap()));
                } else {
                    assert_eq!(vs["r_frame_rate"], "30/1", "{}", t.name);
                    let n: i64 = vs["nb_read_frames"].as_str().and_then(|n| n.parse().ok()).unwrap_or(0);
                    assert!((n - 60).abs() <= 1, "{}: {n} frames", t.name);
                    seen.push(format!("{} {} {w}x{h} 30 fps, {n} frames", vs["codec_name"].as_str().unwrap(), vs["pix_fmt"].as_str().unwrap_or("")));
                }
            }
            if let Some(a) = &t.audio {
                let au = stream(&j, "audio").unwrap_or_else(|| panic!("{}: no audio {j}", t.name));
                assert_eq!(au["channels"].as_u64(), Some(a.channels as u64), "{}", t.name);
                let rate = a.sample_rate(s.sample_rate, &t.container);
                assert_eq!(au["sample_rate"].as_str(), Some(rate.to_string().as_str()), "{}", t.name);
                seen.push(format!("{} {rate} Hz {}ch", au["codec_name"].as_str().unwrap(), a.channels));
            }
            if !t.sequence() {
                let d: f64 = j["format"]["duration"].as_str().and_then(|d| d.parse().ok()).unwrap_or(0.0);
                // To a frame; AAC's and Opus's encoder priming adds up to one more audio frame.
                assert!((d - 2.0).abs() <= 1.0 / 30.0 + 0.025, "{}: {d} s", t.name);
                seen.push(format!("{d:.3} s"));
                std::fs::remove_file(&out).unwrap();
            }
            let line = |x: Option<String>| x.unwrap_or_else(|| "-".into());
            table += &format!("| {} | {} | {} | {} | {} |\n", t.name, t.format(), line(t.video_line(&p.settings, true)), line(t.audio_line(&p.settings)), seen.join("; "));
        }

        // Interlaced MPEG-2 (upper field first) at 29.97 from the 30p project: woven fields, conformed rate.
        let mut ts = BUILTIN.iter().find(|t| t.name == "MPEG-2 TS").unwrap().clone();
        let v = ts.video.as_mut().unwrap();
        (v.field, v.fps, v.size) = (Some(FieldOrder::Upper), Some(29.97), Some((720, 480)));
        let (st, out, _) = render(&p, &ts, "interlaced.m2ts", (0.0, 2.0), Opts::default());
        assert_eq!(st, Status::Done);
        let j = probe(&out);
        let vs = stream(&j, "video").unwrap();
        assert_eq!((vs["field_order"].as_str(), vs["r_frame_rate"].as_str(), vs["width"].as_u64()), (Some("tt"), Some("30000/1001"), Some(720)), "{j}");
        let mut idet = crate::proc::ffmpeg();
        idet.args(["-hide_banner", "-i"]).arg(&out).args(["-vf", "idet", "-f", "null", "-"]);
        let said = String::from_utf8_lossy(&crate::proc::output(&mut idet).unwrap().stderr).into_owned();
        // ffmpeg reports idet twice (its first, empty report is from probing): the larger count.
        let tff: u32 = said.lines().filter(|l| l.contains("Multi frame detection")).filter_map(|l| l.split("TFF:").nth(1)?.split_whitespace().next()?.parse().ok()).max().unwrap_or(0);
        assert!(tff > 30, "the fields move (testsrc2 scrolls): {said}");
        table += &format!("| (custom) MPEG-2 TS 720x480 29.97i | MPEG-2 Transport Stream | upper field first | AC-3 | {} {} fields, TFF frames by idet {tff} |\n", vs["codec_name"].as_str().unwrap(), vs["field_order"].as_str().unwrap());

        // Rotation 90 with a project-size template renders portrait; Stretch off letterboxes into a fixed frame.
        let mut r = p.clone();
        r.settings.rotation = 90;
        let (st, out, _) = render(&r, &BUILTIN[0], "rot.mp4", (0.0, 1.0), Opts::default());
        assert_eq!(st, Status::Done);
        assert_eq!((probe(&out)["streams"][0]["width"].as_u64(), probe(&out)["streams"][0]["height"].as_u64()), (Some(240), Some(320)));
        let (st, out, _) = render(&r, &BUILTIN[0], "unrot.mp4", (0.0, 1.0), Opts { rotate: false, ..Opts::default() });
        assert_eq!(st, Status::Done);
        assert_eq!(probe(&out)["streams"][0]["width"].as_u64(), Some(320));
        let mut wide = BUILTIN[0].clone();
        wide.video.as_mut().unwrap().size = Some((640, 240));
        let (st, out, _) = render(&p, &wide, "letterbox.mp4", (0.0, 1.0), Opts::default());
        assert_eq!(st, Status::Done);
        let px = decoded_frame(&out, 10);
        let at = |x: usize, y: usize| px[(y * 640 + x) * 4..][..3].to_vec();
        assert!(at(20, 120).iter().all(|&c| c < 20) && at(620, 120).iter().all(|&c| c < 20), "black bars");
        assert!(at(320, 120).iter().any(|&c| c > 40), "the picture in the middle");
        let (st, out, _) = render(&p, &wide, "stretch.mp4", (0.0, 1.0), Opts { stretch: true, ..Opts::default() });
        assert_eq!(st, Status::Done);
        let px = decoded_frame(&out, 10);
        assert!(px[(120 * 640 + 20) * 4..][..3].iter().any(|&c| c > 40), "no bars when stretched");

        // ProRes 4444 keeps transparency where no event covers the frame (the event ends at 1 s).
        let mut short = p.clone();
        let c = &mut short.timeline.video_tracks[0].clips[0];
        (c.duration, c.out_point) = (1.0, c.in_point + 1.0);
        let alpha = BUILTIN.iter().find(|t| t.name == "ProRes 4444 (alpha)").unwrap();
        let (st, out, _) = render(&short, alpha, "alpha.mov", (0.0, 2.0), Opts::default());
        assert_eq!(st, Status::Done);
        assert_eq!(stream(&probe(&out), "video").unwrap()["pix_fmt"], "yuva444p12le", "ProRes 4444 decodes as 12-bit with alpha");
        let alpha_of = |k: i64| {
            let mut cmd = crate::proc::ffmpeg();
            cmd.args(["-v", "error", "-i"]).arg(&out).args(["-vf", &format!("select=eq(n\\,{k}),format=rgba"), "-frames:v", "1", "-f", "rawvideo", "-"]);
            let px = crate::proc::output(&mut cmd).unwrap().stdout;
            px[(120 * 320 + 160) * 4 + 3]
        };
        assert_eq!((alpha_of(10), alpha_of(45)), (255, 0));

        // Markers and regions become MP4 chapters when asked.
        let (st, out, _) = render(&p, &BUILTIN[0], "chapters.mp4", (0.0, 2.0), Opts { markers: true, ..Opts::default() });
        assert_eq!(st, Status::Done);
        let ch = probe(&out)["chapters"].as_array().cloned().unwrap_or_default();
        let titles: Vec<&str> = ch.iter().filter_map(|c| c["tags"]["title"].as_str()).collect();
        assert_eq!(titles, ["Intro = start", "Chorus"], "{ch:?}");
        assert!(probe(&dir.join("rot.mp4"))["chapters"].as_array().is_none_or(|c| c.is_empty()), "none unless asked");

        // An invalid combination is refused before ffmpeg runs, and the log says why.
        let mut bad = BUILTIN.iter().find(|t| t.name == "VP9 WebM").unwrap().clone();
        bad.container = "mov".into();
        assert_eq!(bad.check(), Err("QuickTime cannot hold VP9 video.".into()));
        let (st, out, log) = render(&p, &bad, "bad.mov", (0.0, 1.0), Opts::default());
        assert_eq!(st, Status::Failed("QuickTime cannot hold VP9 video.".into()));
        assert!(!out.exists() && std::fs::read_to_string(log.unwrap()).unwrap().contains("Status:   Failed: QuickTime cannot hold VP9 video."));

        // The table, between its markers in docs/FORMAT_SUPPORT.md.
        let doc = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs").join("FORMAT_SUPPORT.md");
        let text = std::fs::read_to_string(&doc).unwrap_or_default();
        let (a, b) = ("<!-- render-table:start -->", "<!-- render-table:end -->");
        let body = format!("{a}\nGenerated by `render_templates_e2e` ({}), {} UTC: 2 s of a 320x240 30 fps, {} Hz project.\n\n{table}{b}", ffmpeg_version(), utc(std::time::SystemTime::now()), p.settings.sample_rate);
        let new = match (text.find(a), text.find(b)) {
            (Some(i), Some(k)) => format!("{}{body}{}", &text[..i], &text[k + b.len()..]),
            _ => format!("{text}{body}\n"),
        };
        std::fs::write(&doc, new).unwrap();
        assert_eq!(temp_audio_left(), 0);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
