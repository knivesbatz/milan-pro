//! Render As: timeline -> one ffmpeg -filter_complex command, run on a worker with progress.
use eframe::egui;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use crate::audio::track_gain;
use crate::project::{MediaItem, MediaType, Project};

pub struct Preset {
    pub name: &'static str,
    pub ext: &'static str,
    /// None = audio only.
    pub video: Option<&'static [&'static str]>,
    pub audio: &'static [&'static str],
}

pub const PRESETS: [Preset; 5] = [
    Preset {
        name: "MP4 H.264 (AAC)",
        ext: "mp4",
        video: Some(&["-c:v", "libx264", "-crf", "18", "-preset", "medium", "-pix_fmt", "yuv420p"]),
        audio: &["-c:a", "aac", "-b:a", "320k"],
    },
    Preset {
        name: "MP4 H.265 (HEVC)",
        ext: "mp4",
        video: Some(&["-c:v", "libx265", "-crf", "22", "-preset", "medium", "-pix_fmt", "yuv420p", "-tag:v", "hvc1"]),
        audio: &["-c:a", "aac", "-b:a", "320k"],
    },
    Preset {
        name: "MOV ProRes 422",
        ext: "mov",
        video: Some(&["-c:v", "prores_ks", "-profile:v", "2", "-pix_fmt", "yuv422p10le"]),
        audio: &["-c:a", "pcm_s16le"],
    },
    Preset { name: "WAV audio only", ext: "wav", video: None, audio: &["-c:a", "pcm_s16le"] },
    Preset { name: "MP3 audio only", ext: "mp3", video: None, audio: &["-c:a", "libmp3lame", "-b:a", "320k"] },
];

/// NTSC rates as exact rationals so 29.97 renders as 30000/1001, not 2997/100.
pub fn fps_str(fps: f64) -> String {
    let n = (fps * 1.001).round();
    if fps.fract() != 0.0 && (fps - n * 1000.0 / 1001.0).abs() < 0.01 {
        format!("{}/1001", n as u64 * 1000)
    } else {
        format!("{fps}")
    }
}

/// Windows' command-line limit, less room for the ffmpeg path.
const MAX_ARGV: usize = 32_767 - 512;
/// The arguments after the inputs: graph file, maps, codecs and the output path (at most 260 characters).
const MAX_ARGV_TAIL: usize = 1024;

/// End of the last clip on any track.
pub fn content_end(project: &Project) -> f64 {
    let tl = &project.timeline;
    tl.video_tracks.iter().chain(&tl.audio_tracks).flat_map(|t| &t.clips).map(|c| c.position + c.duration).fold(0.0, f64::max)
}

/// Full ffmpeg argument list (without the program name) rendering the whole timeline to `out`.
pub fn build_args(project: &Project, preset: &Preset, out: &Path) -> Result<Vec<String>, String> {
    let end = content_end(project);
    if end <= 0.0 {
        return Err("The timeline is empty: nothing to render".into());
    }
    let s = &project.settings;
    let (w, h, fps, sr) = (s.width, s.height, fps_str(s.frame_rate), s.sample_rate);
    let media = |id: &Option<String>| -> Option<&MediaItem> {
        let id = id.as_deref()?;
        project.media_pool.iter().find(|m| m.id == id)
    };
    let mut args: Vec<String> = ["-hide_banner", "-nostdin", "-v", "error", "-y"].map(String::from).to_vec();
    let mut graph: Vec<String> = Vec::new();
    let mut n_in = 0usize;
    let mut input = |args: &mut Vec<String>, pre: Vec<String>, path: &Path| {
        args.extend(pre);
        args.push("-i".into());
        args.push(path.to_string_lossy().into_owned());
        n_in += 1;
        n_in - 1
    };

    if preset.video.is_some() {
        graph.push(format!("color=c=black:s={w}x{h}:r={fps}:d={end:.3}[base]"));
        let mut last = "base".to_string();
        // Bottom track first so video_tracks[0] is composited last, on top.
        let clips = project.timeline.shown_video_tracks().rev().flat_map(|t| t.clips.iter().map(move |c| (t.volume, c)));
        for (k, (level, c)) in clips.enumerate() {
            let Some(m) = media(&c.media_id) else { continue };
            let (pos, dur) = (c.position, c.duration);
            let pre = match m.media_type {
                MediaType::Video => vec!["-ss".into(), format!("{:.3}", c.in_point), "-t".into(), format!("{dur:.3}")],
                MediaType::Image => vec!["-loop".into(), "1".into(), "-framerate".into(), fps.clone(), "-t".into(), format!("{dur:.3}")],
                _ => continue,
            };
            let i = input(&mut args, pre, &m.path);
            // Fades and track Level go through alpha, so the event dissolves into the tracks below (as in VEGAS).
            // shortcut: video fades are linear (ffmpeg fade); audio uses the smooth curve. Upgrade with geq if it matters.
            let mut alpha = String::new();
            if c.fade_in > 0.0 {
                alpha += &format!(",fade=t=in:st={pos:.3}:d={:.3}:alpha=1", c.fade_in);
            }
            if c.fade_out > 0.0 {
                alpha += &format!(",fade=t=out:st={:.3}:d={:.3}:alpha=1", pos + dur - c.fade_out, c.fade_out);
            }
            if level < 1.0 {
                alpha += &format!(",colorchannelmixer=aa={:.3}", level.max(0.0));
            }
            if !alpha.is_empty() {
                alpha.insert_str(0, ",format=yuva420p");
            }
            graph.push(format!(
                "[{i}:v]scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2,setsar=1,fps={fps},setpts=PTS-STARTPTS+{pos:.3}/TB{alpha}[v{k}]"
            ));
            graph.push(format!(
                "[{last}][v{k}]overlay=eof_action=pass:enable='between(t,{pos:.3},{:.3})'[o{k}]",
                pos + dur
            ));
            last = format!("o{k}");
        }
        graph.push(format!("[{last}]null[vout]"));
    }

    let tracks = &project.timeline.audio_tracks;
    let any_solo = tracks.iter().any(|t| t.solo);
    let mut mix = Vec::new();
    for t in tracks {
        let Some([gl, gr]) = track_gain(t, any_solo) else { continue };
        for c in &t.clips {
            let Some(m) = media(&c.media_id).filter(|m| m.sample_rate.is_some()) else { continue };
            let pre = vec!["-ss".into(), format!("{:.3}", c.in_point), "-t".into(), format!("{:.3}", c.duration)];
            let i = input(&mut args, pre, &m.path);
            let (k, ms) = (mix.len(), (c.position * 1000.0).round() as u64);
            let mut fades = String::new();
            if c.fade_in > 0.0 {
                fades += &format!("afade=t=in:st=0:d={:.3}:curve=hsin,", c.fade_in);
            }
            if c.fade_out > 0.0 {
                fades += &format!("afade=t=out:st={:.3}:d={:.3}:curve=hsin,", c.duration - c.fade_out, c.fade_out);
            }
            graph.push(format!(
                "[{i}:a]aformat=sample_rates={sr}:channel_layouts=stereo,pan=stereo|c0={gl}*c0|c1={gr}*c1,{fades}adelay={ms}:all=1[a{k}]"
            ));
            mix.push(format!("[a{k}]"));
        }
    }
    // Silence base keeps the output exactly `end` long and gives silent timelines an audio stream.
    graph.push(format!("anullsrc=r={sr}:cl=stereo,atrim=duration={end:.3}[silence]"));
    graph.push(format!("[silence]{}amix=inputs={}:normalize=0:duration=first[aout]", mix.concat(), mix.len() + 1));

    // Windows caps a command line at 32767 characters. The graph goes through a file (graph_to_file) but every
    // event is still its own -i input, so fail clearly here instead of with os error 206 at spawn.
    // shortcut: about 280 events with 80-character media paths fit; batch 4's engine render removes the inputs.
    let cmdline: usize = args.iter().map(|a| a.len() + 3).sum::<usize>() + MAX_ARGV_TAIL;
    if cmdline > MAX_ARGV {
        return Err(format!(
            "This timeline has too many events to render in one pass (the ffmpeg command line would be about {cmdline} characters; Windows allows {MAX_ARGV}). Move the media to a folder with a shorter path, or render fewer events."
        ));
    }
    args.push("-filter_complex".into());
    args.push(graph.join(";"));
    if let Some(v) = preset.video {
        args.extend(["-map", "[vout]"].map(String::from));
        args.extend(v.iter().map(|s| s.to_string()));
    }
    args.extend(["-map", "[aout]"].map(String::from));
    args.extend(preset.audio.iter().map(|s| s.to_string()));
    args.extend(["-ar".into(), sr.to_string(), "-t".into(), format!("{end:.3}")]);
    args.extend(["-progress", "pipe:1", "-nostats"].map(String::from));
    args.push(out.to_string_lossy().into_owned());
    Ok(args)
}

// ---- Job ----

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Running,
    Done,
    Failed(String),
    Cancelled,
}

pub struct Job {
    pub out: PathBuf,
    started: Instant,
    total: f64,
    /// (seconds rendered, status)
    state: Arc<Mutex<(f64, Status)>>,
    child: Arc<Mutex<Option<Child>>>,
    cancel: Arc<AtomicBool>,
}

impl Job {
    pub fn start(args: Vec<String>, total: f64, out: PathBuf, ctx: egui::Context) -> Job {
        let state = Arc::new(Mutex::new((0.0, Status::Running)));
        let child = Arc::new(Mutex::new(None));
        let cancel = Arc::new(AtomicBool::new(false));
        let job = Job { out: out.clone(), started: Instant::now(), total, state: state.clone(), child: child.clone(), cancel: cancel.clone() };
        std::thread::spawn(move || {
            let stamp = |p: &Path| std::fs::metadata(p).ok().map(|m| (m.modified().ok(), m.len()));
            let before = stamp(&out);
            let graph = graph_path();
            let result = graph_to_file(args, &graph)
                .map_err(|e| format!("Cannot write the render graph to {}: {e}", graph.display()))
                .and_then(|args| run(&args, &state, &child, &cancel, &ctx));
            let _ = std::fs::remove_file(&graph);
            let status = match result {
                _ if cancel.load(Relaxed) => Status::Cancelled,
                Ok(()) => Status::Done,
                Err(e) => Status::Failed(e),
            };
            // A cancelled or failed render leaves no partial file. The dialog confirmed any overwrite;
            // a file ffmpeg never touched (it failed before opening the output) is kept.
            if status != Status::Done && stamp(&out) != before {
                let _ = std::fs::remove_file(&out);
            }
            state.lock().unwrap().1 = status;
            ctx.request_repaint();
        });
        job
    }

    pub fn status(&self) -> Status {
        self.state.lock().unwrap().1.clone()
    }

    pub fn fraction(&self) -> f32 {
        (self.state.lock().unwrap().0 / self.total.max(1e-6)).clamp(0.0, 1.0) as f32
    }

    /// Remaining seconds, once there is enough progress to estimate.
    pub fn eta(&self) -> Option<f64> {
        let f = self.fraction() as f64;
        (f > 0.01).then(|| self.started.elapsed().as_secs_f64() * (1.0 - f) / f)
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Relaxed);
        if let Some(c) = self.child.lock().unwrap().as_mut() {
            let _ = c.kill();
        }
    }
}

/// std never kills a Child on drop and Windows doesn't kill it with the parent, so an
/// unfinished render must be killed here or it keeps encoding invisibly after exit.
/// Drop runs as Milan closes, so it waits (briefly: the killed ffmpeg exits at once) for the worker
/// to delete the partial output and the graph file before the process ends and takes the worker with it.
impl Drop for Job {
    fn drop(&mut self) {
        if self.status() == Status::Running {
            self.cancel();
            let until = Instant::now() + std::time::Duration::from_secs(2);
            while self.status() == Status::Running && Instant::now() < until {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    }
}

/// `%TEMP%\milan-render-<pid>-<n>.txt`, unique per render.
fn graph_path() -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!("milan-render-{}-{}.txt", std::process::id(), N.fetch_add(1, Relaxed)))
}

/// Move the `-filter_complex` graph into `file` and pass it as `-/filter_complex <file>`, so the command line
/// does not grow with the graph past Windows' 32767-character limit.
// shortcut: the -i inputs still grow the command line (~30-80 chars per event); batch 4's engine render removes them.
fn graph_to_file(mut args: Vec<String>, file: &Path) -> std::io::Result<Vec<String>> {
    if let Some(i) = args.iter().position(|a| a == "-filter_complex") {
        std::fs::write(file, &args[i + 1])?;
        args[i] = "-/filter_complex".into();
        args[i + 1] = file.to_string_lossy().into_owned();
    }
    Ok(args)
}

fn run(
    args: &[String],
    state: &Mutex<(f64, Status)>,
    child_slot: &Mutex<Option<Child>>,
    cancel: &AtomicBool,
    ctx: &egui::Context,
) -> Result<(), String> {
    let mut cmd = crate::proc::ffmpeg();
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = crate::proc::spawn(&mut cmd).map_err(|e| format!("{}: cannot render", crate::proc::tool_error(&e, "ffmpeg")))?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    *child_slot.lock().unwrap() = Some(child);
    if cancel.load(Relaxed) {
        if let Some(c) = child_slot.lock().unwrap().as_mut() {
            let _ = c.kill();
        }
    }
    // stderr drained on its own thread so a chatty ffmpeg never blocks on a full pipe.
    let err_thread = std::thread::spawn(move || {
        let lines: Vec<String> = BufReader::new(stderr).lines().map_while(Result::ok).collect();
        lines[lines.len().saturating_sub(12)..].join("\n")
    });
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        if let Some(us) = line.strip_prefix("out_time_us=").and_then(|v| v.trim().parse::<f64>().ok()) {
            state.lock().unwrap().0 = us / 1e6;
            ctx.request_repaint();
        }
    }
    let status = child_slot.lock().unwrap().take().map(|mut c| c.wait());
    let tail = err_thread.join().unwrap_or_default();
    match status {
        Some(Ok(s)) if s.success() => Ok(()),
        Some(Ok(s)) => Err(if tail.is_empty() { format!("ffmpeg exited with {s}") } else { tail }),
        Some(Err(e)) => Err(format!("ffmpeg wait failed: {e}")),
        None => Err("ffmpeg process lost".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Clip, Track, TrackKind};

    fn media(id: &str, kind: MediaType, audio: bool) -> MediaItem {
        MediaItem {
            id: id.into(),
            path: format!("{id}.bin").into(),
            name: id.into(),
            media_type: kind,
            duration: 100.0,
            width: Some(1280),
            height: Some(720),
            frame_rate: Some(30.0),
            sample_rate: audio.then_some(48_000),
            ..Default::default()
        }
    }

    fn clip(m: &MediaItem, pos: f64, dur: f64, inp: f64) -> Clip {
        let mut c = Clip::from_media(m, pos);
        (c.duration, c.in_point, c.out_point) = (dur, inp, inp + dur);
        c
    }

    fn project() -> Project {
        let mut p = Project::new("t");
        let (top, bottom, snd, muted) = (
            media("top", MediaType::Video, false),
            media("bottom", MediaType::Image, false),
            media("snd", MediaType::Audio, true),
            media("muted", MediaType::Audio, true),
        );
        let tl = &mut p.timeline;
        tl.video_tracks[0].clips.push(clip(&top, 1.0, 3.0, 2.0));
        tl.video_tracks.push(Track::new("Video 2", TrackKind::Video));
        tl.video_tracks[1].clips.push(clip(&bottom, 0.0, 5.0, 0.0));
        tl.audio_tracks[0].clips.push(clip(&snd, 1.5, 2.0, 0.5));
        tl.audio_tracks[0].volume = 0.5;
        tl.add_audio_track();
        tl.audio_tracks[1].clips.push(clip(&muted, 0.0, 6.0, 0.0));
        tl.audio_tracks[1].muted = true;
        p.media_pool = vec![top, bottom, snd, muted];
        p
    }

    #[test]
    fn filter_graph_for_small_timeline() {
        let p = project();
        let args = build_args(&p, &PRESETS[0], Path::new("out.mp4")).unwrap();
        let joined = args.join(" ");
        let graph = &args[args.iter().position(|a| a == "-filter_complex").unwrap() + 1];

        // Bottom track's image is input 0 (composited first), top video input 1, then audio.
        assert!(joined.contains("-loop 1 -framerate 30000/1001 -t 5.000 -i bottom.bin"));
        assert!(joined.contains("-ss 2.000 -t 3.000 -i top.bin"));
        assert!(joined.contains("-ss 0.500 -t 2.000 -i snd.bin"));
        assert!(!joined.contains("muted.bin"), "muted track must not be an input");

        // Duration covers the muted clip too (whole timeline = last clip end).
        assert!(graph.starts_with("color=c=black:s=1920x1080:r=30000/1001:d=6.000[base]"));
        let bottom = graph.find("[base][v0]overlay=eof_action=pass:enable='between(t,0.000,5.000)'[o0]").unwrap();
        let top = graph.find("[o0][v1]overlay=eof_action=pass:enable='between(t,1.000,4.000)'[o1]").unwrap();
        assert!(bottom < top);
        assert!(graph.contains("[1:v]scale=1920:1080:force_original_aspect_ratio=decrease,pad=1920:1080"));
        assert!(graph.contains("setpts=PTS-STARTPTS+1.000/TB[v1]"));
        assert!(graph.contains("[o1]null[vout]"));
        assert!(graph.contains("[2:a]aformat=sample_rates=48000:channel_layouts=stereo,pan=stereo|c0=0.5*c0|c1=0.5*c1,adelay=1500:all=1[a0]"));
        assert!(graph.contains("[silence][a0]amix=inputs=2:normalize=0:duration=first[aout]"));
        for w in [["-map", "[vout]"], ["-map", "[aout]"], ["-c:v", "libx264"], ["-progress", "pipe:1"], ["-t", "6.000"]] {
            assert!(args.windows(2).any(|a| a == w), "missing {w:?}");
        }
        assert_eq!(args.last().unwrap(), "out.mp4");
    }

    #[test]
    fn audio_only_and_solo_and_empty() {
        let mut p = project();
        p.timeline.audio_tracks[1].muted = false;
        p.timeline.audio_tracks[1].solo = true; // solo silences track 0
        let args = build_args(&p, &PRESETS[3], Path::new("o.wav")).unwrap();
        let joined = args.join(" ");
        assert!(!joined.contains("[vout]") && !joined.contains("top.bin") && !joined.contains("snd.bin"));
        assert!(joined.contains("muted.bin") && joined.contains("adelay=0:all=1"));
        assert!(build_args(&Project::new("e"), &PRESETS[0], Path::new("o.mp4")).is_err());
    }

    #[test]
    fn fades_and_level_in_graph() {
        let mut p = project();
        let c = &mut p.timeline.video_tracks[0].clips[0]; // pos 1, dur 3
        (c.fade_in, c.fade_out) = (0.5, 1.0);
        p.timeline.video_tracks[0].volume = 0.75;
        let c = &mut p.timeline.audio_tracks[0].clips[0]; // dur 2
        (c.fade_in, c.fade_out) = (0.25, 0.5);
        let args = build_args(&p, &PRESETS[0], Path::new("o.mp4")).unwrap();
        let graph = &args[args.iter().position(|a| a == "-filter_complex").unwrap() + 1];
        assert!(graph.contains(
            "setpts=PTS-STARTPTS+1.000/TB,format=yuva420p,fade=t=in:st=1.000:d=0.500:alpha=1,fade=t=out:st=3.000:d=1.000:alpha=1,colorchannelmixer=aa=0.750[v1]"
        ));
        assert!(graph.contains("setpts=PTS-STARTPTS+0.000/TB[v0]")); // untouched bottom track
        assert!(graph.contains("afade=t=in:st=0:d=0.250:curve=hsin,afade=t=out:st=1.500:d=0.500:curve=hsin,adelay=1500:all=1[a0]"));
    }

    /// A project whose media sit at realistic 80-character paths, with `n` video and `n` audio events.
    fn long_paths(n: usize) -> Project {
        let mut p = project();
        for m in &mut p.media_pool {
            m.path = format!(r"C:\Users\someone\Videos\Projects\Wedding 2026\Camera A\{:0>21}.MP4", m.id).into();
            assert_eq!(m.path.as_os_str().len(), 80);
        }
        let (v, a) = (p.media_pool[0].clone(), p.media_pool[2].clone());
        p.timeline.video_tracks[0].clips = (0..n).map(|i| clip(&v, i as f64, 1.0, 0.0)).collect();
        p.timeline.audio_tracks[0].clips = (0..n).map(|i| clip(&a, i as f64, 1.0, 0.0)).collect();
        p
    }

    #[test]
    fn too_many_inputs_is_a_clear_error() {
        // 250 + 250 events at 80-character paths need ~58k characters of -i inputs.
        let e = build_args(&long_paths(250), &PRESETS[0], Path::new("o.mp4")).unwrap_err();
        assert!(e.contains("too many events"), "{e}");
    }

    #[test]
    fn graph_goes_through_a_file() {
        // 125 video + 125 audio events at 80-character paths: the inline graph alone would be ~45k characters.
        let p = long_paths(125);
        let args = build_args(&p, &PRESETS[0], Path::new("o.mp4")).unwrap();
        let graph = args[args.iter().position(|a| a == "-filter_complex").unwrap() + 1].clone();
        assert!(graph.len() > 32_767);

        let file = std::env::temp_dir().join(format!("milan-graph-test-{}.txt", std::process::id()));
        let args = graph_to_file(args, &file).unwrap();
        let written = std::fs::read_to_string(&file).unwrap();
        std::fs::remove_file(&file).unwrap();
        assert_eq!(written, graph);
        let i = args.iter().position(|a| a == "-/filter_complex").unwrap();
        assert_eq!(args[i + 1], file.to_string_lossy());
        assert!(!args.iter().any(|a| a == "-filter_complex" || a.contains("overlay")));
        let len: usize = args.iter().map(|a| a.len() + 1).sum();
        assert!(len < 32_767, "command line {len} characters");
    }

    #[test]
    fn fps_strings() {
        assert_eq!(fps_str(29.97), "30000/1001");
        assert_eq!(fps_str(23.976), "24000/1001");
        assert_eq!(fps_str(59.94), "60000/1001");
        assert_eq!(fps_str(25.0), "25");
        assert_eq!(fps_str(24.0), "24");
    }

    /// End to end: needs ffmpeg/ffprobe. `cargo test -- --ignored render_e2e`
    #[test]
    #[ignore]
    fn render_e2e() {
        let dir = std::env::temp_dir().join(format!("milan-render-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("src.mp4");
        let ok = crate::proc::ffmpeg()
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc=d=3:s=320x240:r=30", "-f", "lavfi", "-i", "sine=f=440:d=3"])
            .args(["-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"])
            .arg(&src)
            .status()
            .unwrap()
            .success();
        assert!(ok);
        let m = crate::media::probe_media(&src).unwrap();
        let mut p = Project::new("e2e");
        (p.settings.width, p.settings.height, p.settings.frame_rate) = (320, 240, 30.0);
        p.timeline.video_tracks[0].clips.push(clip(&m, 0.0, 3.0, 0.0));
        p.timeline.audio_tracks[0].clips.push(clip(&m, 1.0, 2.0, 0.0)); // ends at 3.0 too
        p.media_pool.push(m);

        let mut probes = Vec::new();
        for (i, preset) in PRESETS.iter().enumerate() {
            let out = dir.join(format!("out{i}.{}", preset.ext));
            let args = build_args(&p, preset, &out).unwrap();
            let job = Job::start(args, content_end(&p), out.clone(), egui::Context::default());
            while job.status() == Status::Running {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            probes.push((preset.name, job.status(), crate::media::probe_media(&out).unwrap()));
        }

        // Cancel mid-render: no partial output and no graph file are left behind.
        let mut long = p.clone();
        let c = &mut long.timeline.video_tracks[0].clips[0];
        (c.duration, c.out_point) = (600.0, 600.0); // long enough to still be running when cancelled
        let out = dir.join("cancelled.mp4");
        let job = Job::start(build_args(&long, &PRESETS[1], &out).unwrap(), content_end(&long), out.clone(), egui::Context::default());
        while job.fraction() == 0.0 && job.status() == Status::Running {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(out.exists());
        job.cancel();
        while job.status() == Status::Running {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let cancelled = (job.status(), out.exists());
        // Dropping a running job (Milan closing) cleans up before drop returns.
        let out = dir.join("dropped.mp4");
        let job = Job::start(build_args(&long, &PRESETS[1], &out).unwrap(), content_end(&long), out.clone(), egui::Context::default());
        while job.fraction() == 0.0 && job.status() == Status::Running {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        drop(job);
        let dropped_left = out.exists();
        let prefix = format!("milan-render-{}-", std::process::id());
        let graphs = std::fs::read_dir(std::env::temp_dir())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix) && e.file_name().to_string_lossy().ends_with(".txt"))
            .count();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(cancelled, (Status::Cancelled, false));
        assert!(!dropped_left, "a dropped job left its partial output");
        assert_eq!(graphs, 0);
        for (name, status, probed) in probes {
            assert_eq!(status, Status::Done, "{name}");
            assert!((probed.duration - 3.0).abs() < 0.1, "{name} duration {}", probed.duration);
            assert!(probed.sample_rate.is_some(), "{name}");
            if !name.contains("audio only") {
                assert_eq!((probed.width, probed.height), (Some(320), Some(240)), "{name}");
            }
        }
    }
}
