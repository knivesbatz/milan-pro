//! Render As: the presets, and the background `Job` the dialog watches (the render itself is `engine::render`).
use eframe::egui;
use std::path::PathBuf;
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use crate::project::Project;

pub struct Preset {
    pub name: &'static str,
    pub ext: &'static str,
    /// ffmpeg `-f`: the output is written to `<out>.partial`, so the format cannot come from the extension.
    pub muxer: &'static str,
    /// None = audio only.
    pub video: Option<&'static [&'static str]>,
    pub audio: &'static [&'static str],
}

pub static PRESETS: [Preset; 5] = [
    Preset {
        name: "MP4 H.264 (AAC)",
        ext: "mp4",
        muxer: "mp4",
        video: Some(&["-c:v", "libx264", "-crf", "18", "-preset", "medium", "-pix_fmt", "yuv420p"]),
        audio: &["-c:a", "aac", "-b:a", "320k"],
    },
    Preset {
        name: "MP4 H.265 (HEVC)",
        ext: "mp4",
        muxer: "mp4",
        video: Some(&["-c:v", "libx265", "-crf", "22", "-preset", "medium", "-pix_fmt", "yuv420p", "-tag:v", "hvc1"]),
        audio: &["-c:a", "aac", "-b:a", "320k"],
    },
    Preset {
        name: "MOV ProRes 422",
        ext: "mov",
        muxer: "mov",
        video: Some(&["-c:v", "prores_ks", "-profile:v", "2", "-pix_fmt", "yuv422p10le"]),
        audio: &["-c:a", "pcm_s16le"],
    },
    Preset { name: "WAV audio only", ext: "wav", muxer: "wav", video: None, audio: &["-c:a", "pcm_s16le"] },
    Preset { name: "MP3 audio only", ext: "mp3", muxer: "mp3", video: None, audio: &["-c:a", "libmp3lame", "-b:a", "320k"] },
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

/// End of the last clip on any track.
pub fn content_end(project: &Project) -> f64 {
    let tl = &project.timeline;
    tl.video_tracks.iter().chain(&tl.audio_tracks).flat_map(|t| &t.clips).map(|c| c.position + c.duration).fold(0.0, f64::max)
}

// ---- Job ----

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Status {
    #[default]
    Running,
    Done,
    Failed(String),
    Cancelled,
}

/// What the render thread shares with the dialog's `Job`: progress, the encoder (so Cancel can kill it) and the cancel flag.
pub struct Ctl {
    /// (fraction done, status)
    state: Mutex<(f64, Status)>,
    child: Mutex<Option<Child>>,
    cancel: Arc<AtomicBool>,
    /// Files the render deletes when it stops short; `Job::drop` deletes them itself if the render thread is stuck.
    temp: Mutex<Vec<PathBuf>>,
    ctx: egui::Context,
}

impl Ctl {
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Relaxed)
    }

    /// The cancel flag, for work that waits outside the render thread's own checks (the mixer's decoders).
    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }

    pub fn set_temp(&self, files: Vec<PathBuf>) {
        *self.temp.lock().unwrap() = files;
    }

    pub fn progress(&self, fraction: f64) {
        self.state.lock().unwrap().0 = fraction;
        self.ctx.request_repaint();
    }

    /// Hold the encoder so Cancel can kill it (at once, if Cancel came first).
    pub fn set_child(&self, c: Child) {
        *self.child.lock().unwrap() = Some(c);
        if self.cancelled() {
            self.kill();
        }
    }

    pub fn take_child(&self) -> Option<Child> {
        self.child.lock().unwrap().take()
    }

    pub fn kill(&self) {
        if let Some(c) = self.child.lock().unwrap().as_mut() {
            let _ = c.kill();
        }
    }
}

pub struct Job {
    pub out: PathBuf,
    started: Instant,
    ctl: Arc<Ctl>,
}

impl Job {
    /// Run `work` on a "milan-render" thread. Ok is Done; an error is Cancelled once Cancel was pressed, else Failed.
    pub fn start(out: PathBuf, ctx: egui::Context, work: impl FnOnce(&Ctl) -> Result<(), String> + Send + 'static) -> Job {
        let ctl = Arc::new(Ctl { state: Mutex::default(), child: Mutex::default(), cancel: Arc::default(), temp: Mutex::default(), ctx });
        let c = ctl.clone();
        std::thread::Builder::new()
            .name("milan-render".into())
            .spawn(move || {
                let status = match work(&c) {
                    Ok(()) => Status::Done,
                    Err(_) if c.cancelled() => Status::Cancelled,
                    Err(e) => Status::Failed(e),
                };
                c.state.lock().unwrap().1 = status;
                c.ctx.request_repaint();
            })
            .expect("spawn milan-render");
        Job { out, started: Instant::now(), ctl }
    }

    pub fn status(&self) -> Status {
        self.ctl.state.lock().unwrap().1.clone()
    }

    pub fn fraction(&self) -> f32 {
        self.ctl.state.lock().unwrap().0.clamp(0.0, 1.0) as f32
    }

    /// Remaining seconds, once there is enough progress to estimate.
    pub fn eta(&self) -> Option<f64> {
        let f = self.fraction() as f64;
        (f > 0.01).then(|| self.started.elapsed().as_secs_f64() * (1.0 - f) / f)
    }

    pub fn cancel(&self) {
        self.ctl.cancel.store(true, Relaxed);
        self.ctl.kill();
    }
}

/// std never kills a Child on drop, so an unfinished render must be cancelled here or it keeps encoding invisibly
/// after exit. Drop runs as Milan closes, so it waits (briefly: the killed ffmpeg exits at once) for the render
/// thread to delete the partial output and the temp audio before the process ends and takes the thread with it.
/// A thread still busy after that (a slow first frame from a decoder) cannot clean up in time, so Drop does it.
impl Drop for Job {
    fn drop(&mut self) {
        if self.status() == Status::Running {
            self.cancel();
            let until = Instant::now() + std::time::Duration::from_secs(2);
            while self.status() == Status::Running && Instant::now() < until {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            if self.status() == Status::Running {
                // The killed encoder must have exited, or Windows keeps the partial output locked.
                if let Some(mut c) = self.ctl.take_child() {
                    let _ = c.wait();
                }
                for f in self.ctl.temp.lock().unwrap().iter() {
                    let _ = std::fs::remove_file(f);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fps_strings() {
        assert_eq!(fps_str(29.97), "30000/1001");
        assert_eq!(fps_str(23.976), "24000/1001");
        assert_eq!(fps_str(59.94), "60000/1001");
        assert_eq!(fps_str(25.0), "25");
        assert_eq!(fps_str(24.0), "24");
    }

    #[test]
    fn job_status_follows_the_work_and_cancel() {
        let wait = |j: &Job| {
            while j.status() == Status::Running {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            j.status()
        };
        let ctx = egui::Context::default();
        let j = Job::start("o".into(), ctx.clone(), |c| {
            c.progress(0.5);
            Ok(())
        });
        assert_eq!((wait(&j), j.fraction()), (Status::Done, 0.5));
        assert_eq!(wait(&Job::start("o".into(), ctx.clone(), |_| Err("no".into()))), Status::Failed("no".into()));
        let j = Job::start("o".into(), ctx, |c| {
            while !c.cancelled() {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Err("stopped".into())
        });
        j.cancel();
        assert_eq!(wait(&j), Status::Cancelled);
    }

    #[test]
    fn drop_deletes_the_files_of_a_render_stuck_past_its_wait() {
        let f = std::env::temp_dir().join(format!("milan-job-drop-{}.partial", std::process::id()));
        std::fs::write(&f, b"x").unwrap();
        let (g, (tx, rx)) = (f.clone(), std::sync::mpsc::channel());
        let j = Job::start("o".into(), egui::Context::default(), move |c| {
            c.set_temp(vec![g]);
            tx.send(()).unwrap();
            std::thread::sleep(std::time::Duration::from_secs(3)); // a slow decoder: Cancel is not seen in time
            Err("late".into())
        });
        rx.recv().unwrap();
        drop(j);
        assert!(!f.exists());
    }
}
