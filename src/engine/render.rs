//! Render through the engine: the same plan, compositor and mixer as preview and playback, piped into one ffmpeg encoder.
//! Audio is mixed first (to a temp file when there is video, straight into the encoder when there is not), then every
//! frame is composed at full size and Best quality and written to the encoder's stdin. The output goes to
//! `<out>.partial` and replaces `out` only once the encoder has finished.
use eframe::egui::{self, ColorImage};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Stdio};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, mpsc};
use crate::audio::RATE;
use crate::decode::Quality;
use crate::engine::mix::{self, BLOCK, Mixer};
use crate::engine::plan::{self, VideoPlan, compile_audio};
use crate::engine::worker::Worker;
use crate::project::Project;
use crate::render::{Ctl, Job, Preset, fps_str};

/// Share of the progress bar the audio pass takes before the video pass.
const AUDIO_SHARE: f64 = 0.05;

/// Render `range` (seconds) of a snapshot of `project` to `out` with `preset`. The master gain is `timeline.master.gain`.
pub fn start(project: &Project, preset: &'static Preset, out: PathBuf, range: (f64, f64), ctx: egui::Context) -> Job {
    let p = project.clone();
    let target = out.clone();
    Job::start(out, ctx, move |ctl| render(&p, preset, &target, range, ctl))
}

/// `<out>.partial`
fn partial(out: &Path) -> PathBuf {
    let mut s = out.as_os_str().to_owned();
    s.push(".partial");
    s.into()
}

/// `%TEMP%\milan-render-<pid>-<n>.f32`, unique per render.
fn temp_audio() -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!("milan-render-{}-{}.f32", std::process::id(), N.fetch_add(1, Relaxed)))
}

/// ffmpeg arguments (after `-nostdin`). Video presets: RGBA frames on stdin plus the mixed audio from `audio`;
/// audio-only presets: the mix on stdin. Output in `muxer` format to `out`.
pub fn encoder_args(preset: &Preset, (w, h): (u32, u32), fps: &str, sample_rate: u32, audio: &Path, out: &Path) -> Vec<String> {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let mut a = s(&["-hide_banner", "-v", "error", "-y"]);
    let pcm = s(&["-f", "f32le", "-ar", &RATE.to_string(), "-ac", "2", "-i"]);
    match preset.video {
        Some(v) => {
            a.extend(s(&["-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{w}x{h}"), "-framerate", fps, "-i", "-"]));
            a.extend(pcm);
            a.push(audio.to_string_lossy().into_owned());
            a.extend(s(&["-map", "0:v", "-map", "1:a", "-vf", "scale=out_color_matrix=bt709:out_range=tv"]));
            a.extend(s(&["-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709"]));
            a.extend(s(v));
        }
        None => {
            a.extend(pcm);
            a.push("-".into());
        }
    }
    a.extend(s(preset.audio));
    a.extend(s(&["-ar", &sample_rate.to_string(), "-f", preset.muxer]));
    a.push(out.to_string_lossy().into_owned());
    a
}

/// Encode into `<out>.partial`, then put it in place of `out`. Nothing but `out` is left behind, whatever happens.
fn render(p: &Project, preset: &Preset, out: &Path, range: (f64, f64), ctl: &Ctl) -> Result<(), String> {
    let (part, tmp) = (partial(out), temp_audio());
    ctl.set_temp(vec![part.clone(), tmp.clone()]);
    let r = encode(p, preset, range, &part, &tmp, ctl);
    let _ = std::fs::remove_file(&tmp);
    if r.is_err() || ctl.cancelled() {
        let _ = std::fs::remove_file(&part);
        return r.and(Err("Cancelled".into()));
    }
    // A failed rename keeps the finished render under its .partial name rather than losing it.
    std::fs::rename(&part, out).map_err(|e| format!("The render is finished as {} but could not replace {}: {e}", part.display(), out.display()))
}

fn encode(p: &Project, preset: &Preset, (a, b): (f64, f64), part: &Path, tmp: &Path, ctl: &Ctl) -> Result<(), String> {
    let vp = preset.video.map(|_| plan::compile(p));
    let frames = vp.as_ref().map(|vp| ((a * vp.fps).round() as i64, (b * vp.fps).round() as i64));
    if b <= a || frames.is_some_and(|(n0, n1)| n1 <= n0) {
        return Err("Nothing to render: the range is empty".into());
    }
    if vp.is_some() {
        let file = std::fs::File::create(tmp).map_err(|e| format!("Cannot create the render's temp audio {}: {e}", tmp.display()))?;
        let (mut file, mut err) = (BufWriter::new(file), None);
        // A refused write (disk full) leaves its error in `err`.
        mix_range(p, (a, b), ctl, AUDIO_SHARE, |bytes| file.write_all(bytes).map_err(|e| err = Some(e)).is_ok())?;
        if let Some(e) = err.or_else(|| file.flush().err()) {
            return Err(format!("Cannot write the render's temp audio {}: {e}", tmp.display()));
        }
    }
    let s = &p.settings;
    let size = vp.as_ref().map_or((s.width, s.height), |vp| (vp.w, vp.h));
    let fps = vp.as_ref().map_or_else(|| fps_str(s.frame_rate), |vp| vp.fps_str.clone());
    let mut cmd = crate::proc::ffmpeg();
    cmd.args(encoder_args(preset, size, &fps, s.sample_rate, tmp, part)).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
    let mut child = crate::proc::spawn(&mut cmd).map_err(|e| format!("{}: cannot render", crate::proc::tool_error(&e, "ffmpeg")))?;
    let (mut stdin, stderr) = (child.stdin.take().expect("piped stdin"), child.stderr.take().expect("piped stderr"));
    ctl.set_child(child);
    // stderr drained on its own thread so a chatty ffmpeg never blocks on a full pipe.
    let tail = std::thread::spawn(move || {
        let lines: Vec<String> = BufReader::new(stderr).lines().map_while(Result::ok).collect();
        lines[lines.len().saturating_sub(12)..].join("\n")
    });
    let fed = match (vp, frames) {
        (Some(vp), Some(n)) => feed_video(vp, n, stdin, ctl),
        // `move`: stdin closes when the mix ends, which tells the encoder to finish.
        _ => mix_range(p, (a, b), ctl, 1.0, move |bytes| stdin.write_all(bytes).is_ok()),
    };
    // stdin is closed: the encoder finishes, or is killed when the frames stopped short.
    if !matches!(fed, Ok(true)) {
        ctl.kill();
    }
    let tail = tail.join().unwrap_or_default();
    let status = ctl.take_child().map(|mut c| c.wait());
    let complete = fed?;
    match status {
        Some(Ok(s)) if s.success() && complete => Ok(()),
        Some(Ok(s)) => Err(if tail.is_empty() { format!("ffmpeg exited with {s}") } else { tail }),
        Some(Err(e)) => Err(format!("ffmpeg wait failed: {e}")),
        None => Err("ffmpeg process lost".into()),
    }
}

/// Mix seconds `a..b` at RATE with the master gain, clamped to [-1, 1], as f32le stereo into `write`.
/// Ok(false): `write` refused the data. Progress goes 0..`share`.
// shortcut: always clamped; batch 16 skips the clamp when the master FX end in a limiter.
fn mix_range(p: &Project, (a, b): (f64, f64), ctl: &Ctl, share: f64, mut write: impl FnMut(&[u8]) -> bool) -> Result<bool, String> {
    let plan = compile_audio(p);
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

/// Compose frames `n0..n1` at full size and Best quality and write them to the encoder, which a writer thread feeds
/// so composing and encoding overlap. No deadlines (nothing is dropped), no composite cache, decoders read forward.
/// Ok(false): the encoder stopped taking frames. Progress goes AUDIO_SHARE..1.
fn feed_video(vp: VideoPlan, (n0, n1): (i64, i64), mut stdin: ChildStdin, ctl: &Ctl) -> Result<bool, String> {
    let (w, h, fps) = (vp.w, vp.h, vp.fps);
    let (tx, rx) = mpsc::sync_channel::<Arc<ColorImage>>(3);
    let writer = std::thread::spawn(move || rx.iter().all(|img| stdin.write_all(bytemuck::cast_slice(&img.pixels)).is_ok()));
    let mut wk = Worker::for_render();
    wk.set_plan(Arc::new(vp));
    let mut r = Ok(true);
    for n in n0..n1 {
        if ctl.cancelled() {
            r = Err("Cancelled".into());
            break;
        }
        let (img, err) = wk.frame(n, w, h, Quality::Best, None);
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
    use crate::render::{PRESETS, Status, content_end};
    use std::time::{Duration, Instant};

    #[test]
    fn encoder_argv_for_every_preset() {
        // A 260-character output path, the most Windows allows without long-path support.
        let out = PathBuf::from(format!(r"C:\{}.mp4", "x".repeat(253)));
        let tmp = std::env::temp_dir().join("milan-render-1234-0.f32");
        for p in &PRESETS {
            let a = encoder_args(p, (1920, 1080), "30000/1001", 44_100, &tmp, &partial(&out));
            let joined = a.join(" ");
            assert!(joined.ends_with(&format!("-ar 44100 -f {} {}.partial", p.muxer, out.display())), "{}: {joined}", p.name);
            assert!(a.windows(2).any(|w| w == ["-f", p.muxer]));
            // The command line does not grow with the timeline: events never appear in it.
            assert!(a.iter().map(|x| x.len() + 3).sum::<usize>() < 1000, "{}: {} characters", p.name, joined.len());
            if p.video.is_some() {
                assert!(joined.contains("-f rawvideo -pix_fmt rgba -s 1920x1080 -framerate 30000/1001 -i - "), "{joined}");
                assert!(joined.contains(&format!("-f f32le -ar 48000 -ac 2 -i {} -map 0:v -map 1:a", tmp.display())));
                assert!(joined.contains("scale=out_color_matrix=bt709:out_range=tv -colorspace bt709 -color_primaries bt709 -color_trc bt709"));
            } else {
                assert!(!joined.contains("rawvideo") && !joined.contains("-map") && !joined.contains(".f32"), "{joined}");
                assert!(joined.contains("-f f32le -ar 48000 -ac 2 -i - "));
            }
        }
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
        assert_eq!(content_end(&p), 3.0);
        let run = |p: &Project, preset: &'static Preset, name: &str, range: (f64, f64)| {
            let out = dir.join(name);
            let job = start(p, preset, out.clone(), range, egui::Context::default());
            assert_eq!(wait(&job), Status::Done, "{}", preset.name);
            assert!(!partial(&out).exists());
            out
        };

        for (i, preset) in PRESETS.iter().enumerate() {
            let out = run(&p, preset, &format!("out{i}.{}", preset.ext), (0.0, 3.0));
            let m = crate::media::probe_media(&out).unwrap();
            assert!((m.duration - 3.0).abs() < 0.1, "{} duration {}", preset.name, m.duration);
            assert_eq!(m.sample_rate, Some(48_000), "{}", preset.name);
            if preset.video.is_some() {
                assert_eq!((m.width, m.height), (Some(320), Some(240)), "{}", preset.name);
            }
        }

        // The loop region only.
        let m = crate::media::probe_media(&run(&p, &PRESETS[0], "loop.mp4", (1.0, 2.0))).unwrap();
        assert!((m.duration - 1.0).abs() < 0.07, "loop render {}", m.duration);

        // Parity: the ProRes render's frames are the compositor's frames.
        let out = dir.join("out2.mov");
        let mut wk = Worker::for_render();
        wk.set_plan(Arc::new(plan::compile(&p)));
        for k in [0i64, 20, 45, 80] {
            let (want, err) = wk.frame(k, 320, 240, Quality::Best, None);
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

        // Master gain 0.5 halves the rendered audio.
        let full = pcm(&dir.join("out3.wav"));
        p.timeline.master.gain = [0.5, 0.5];
        let half = pcm(&run(&p, &PRESETS[3], "half.wav", (0.0, 3.0)));
        let peak = |v: &[f32]| v.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(peak(&full) > 0.05 &&(peak(&half) / peak(&full) - 0.5).abs() < 0.01, "{} vs {}", peak(&half), peak(&full));

        // Cancel mid-render, then drop a running job (Milan closing): no output, no .partial, no temp audio.
        let mut long = p.clone();
        let c = &mut long.timeline.video_tracks[2].clips[0];
        (c.duration, c.out_point) = (600.0, 600.0); // the last frame holds: long enough to still be running
        for (k, drop_it) in [(0, false), (1, true)] {
            let out = dir.join(format!("cancelled{k}.mp4"));
            let job = start(&long, &PRESETS[1], out.clone(), (0.0, 600.0), egui::Context::default());
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
}
