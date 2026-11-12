//! Timeline engine: one plan of the project for preview, playback and render (docs/FEATURE_PLAN.md section 1).
//! Video: `plan` turns the project into frame plans, `worker` composes them on the "milan-video" thread
//! from persistent `vdecode` streams, and `VideoEngine` is the UI thread's handle to it.
//! Audio: `plan::compile_audio` and `mix` (driven by `audio::Player`).
//! Render: `render` runs both into one ffmpeg encoder.
pub mod cache;
pub mod composite;
// shortcut: key editing is used from batch 10 (keyframe UI) on.
#[allow(dead_code)]
pub mod keyframes;
pub mod mix;
pub mod plan;
pub mod render;
pub mod vdecode;
mod worker;

use eframe::egui::{self, ColorImage};
use std::hash::{DefaultHasher, Hasher};
use std::sync::atomic::{AtomicI64, AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use crate::decode::Quality;
use crate::project::Project;
use plan::VideoPlan;

pub enum VCmd {
    Plan(Arc<VideoPlan>),
    Show { seq: u64, n: i64, w: u32, h: u32, q: Quality },
    /// Stream frames from `from_n`, looping over `lp` (seconds) if set.
    Play { from_n: i64, w: u32, h: u32, q: Quality, lp: Option<(f64, f64)>, tx: SyncSender<Played> },
    Stop,
}

/// (Show generation, frame, first decode error)
pub type Shown = (u64, Arc<ColorImage>, Option<String>);
/// (unwrapped frame number, frame, first decode error)
pub type Played = (i64, Arc<ColorImage>, Option<String>);

/// Playback figures, refreshed every 0.5 s while playing.
// shortcut: read only by the bench tests until a UI shows them (a later Preview status item).
#[allow(dead_code)]
#[derive(Default)]
pub struct Stats {
    pub fps_x100: AtomicU32,
    pub dropped: AtomicU64,
    pub live_decoders: AtomicU32,
}

/// What the picture depends on: settings, timeline and media (cursor, zoom and scroll are not serialized).
// shortcut: hashes the whole project every UI frame (about 1 ms on big projects); switch to a generation counter in mark_modified if a profile shows it.
pub fn content_hash(p: &Project) -> u64 {
    struct HashWriter(DefaultHasher);
    impl std::io::Write for HashWriter {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.write(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut w = HashWriter(DefaultHasher::new());
    let _ = serde_json::to_writer(&mut w, &(&p.settings, &p.timeline, &p.media_pool));
    w.0.finish()
}

/// The UI thread's side of the video engine.
pub struct VideoEngine {
    tx: Sender<VCmd>,
    latest: Arc<Mutex<Option<Shown>>>,
    play_rx: Option<Receiver<Played>>,
    /// A played frame that is not due yet.
    ahead: Option<Played>,
    /// The frame on screen while playing (unwrapped over loop passes), read by the worker.
    clock_frame: Arc<AtomicI64>,
    #[allow(dead_code)]
    pub stats: Arc<Stats>,
    sent_hash: u64,
    fps: f64,
    want: Option<(i64, u32, Quality)>,
    seq: u64,
    shown_seq: u64,
    /// Playback wanted, with its loop region; None = paused.
    play: Option<Option<(f64, f64)>>,
    /// Size and quality of the running play session.
    session: Option<(u32, Quality)>,
    loop_n: Option<(i64, i64)>,
    wraps: i64,
    last_clock: i64,
}

impl VideoEngine {
    pub fn new(ctx: &egui::Context) -> Self {
        let (tx, rx) = mpsc::channel();
        let latest = Arc::new(Mutex::new(None));
        let clock_frame = Arc::new(AtomicI64::new(0));
        let stats = Arc::new(Stats::default());
        let (l, c, s, ctx) = (latest.clone(), clock_frame.clone(), stats.clone(), ctx.clone());
        std::thread::Builder::new().name("milan-video".into()).spawn(move || worker::run(rx, l, c, s, ctx)).expect("spawn milan-video");
        Self {
            tx,
            latest,
            play_rx: None,
            ahead: None,
            clock_frame,
            stats,
            sent_hash: 0,
            fps: 30.0,
            want: None,
            seq: 0,
            shown_seq: 0,
            play: None,
            session: None,
            loop_n: None,
            wraps: 0,
            last_clock: 0,
        }
    }

    /// Start (or restart, after a seek) playback at the cursor, looping over `lp` if set.
    pub fn play(&mut self, lp: Option<(f64, f64)>) {
        self.play = Some(lp);
        // Dropping the receiver ends the worker's current session.
        (self.play_rx, self.ahead) = (None, None);
    }

    pub fn stop(&mut self) {
        self.play = None;
    }

    /// A paused frame was asked for and has not arrived.
    pub fn busy(&self) -> bool {
        self.play.is_none() && self.shown_seq < self.seq
    }

    /// Once per UI frame: replan after edits, then take the wanted frame (paused) or the due frame (playing).
    /// Returns a new picture and the first decode error behind it.
    pub fn update(&mut self, p: &Project, w: u32, h: u32, q: Quality, visible: bool) -> Option<(Arc<ColorImage>, Option<String>)> {
        let hash = content_hash(p);
        if hash != self.sent_hash {
            let plan = plan::compile(p);
            self.fps = plan.fps;
            let _ = self.tx.send(VCmd::Plan(Arc::new(plan)));
            (self.sent_hash, self.want) = (hash, None);
        }
        let clock = plan::frame_at(p.timeline.cursor_position, self.fps);
        let Some(lp) = self.play.filter(|_| visible) else {
            if self.play_rx.take().is_some() {
                let _ = self.tx.send(VCmd::Stop);
                (self.ahead, self.session, self.want) = (None, None, None);
            }
            // Paused, the picture is the frame the timecode and Frame readouts name (nearest), also between frames.
            let n = (p.timeline.cursor_position * self.fps).round() as i64;
            let key = (n, w, q);
            if visible && self.want != Some(key) {
                self.seq += 1;
                let _ = self.tx.send(VCmd::Show { seq: self.seq, n, w, h, q });
                self.want = Some(key);
            }
            let seq = self.seq;
            let (g, img, err) = self.latest.lock().unwrap().take_if(|s| s.0 == seq)?;
            self.shown_seq = g;
            return Some((img, err));
        };
        if self.play_rx.is_none() || self.session != Some((w, q)) {
            let (tx, rx) = mpsc::sync_channel(4);
            self.clock_frame.store(clock, Relaxed);
            let _ = self.tx.send(VCmd::Play { from_n: clock, w, h, q, lp, tx });
            (self.play_rx, self.ahead, self.session) = (Some(rx), None, Some((w, q)));
            (self.loop_n, self.wraps, self.last_clock) = (lp.map(|l| plan::loop_frames(l, self.fps)), 0, clock);
        }
        // Seeks restart the session, so the clock only goes back when the app wraps the loop.
        if clock < self.last_clock {
            self.wraps += 1;
        }
        self.last_clock = clock;
        let v = clock + self.wraps * self.loop_n.map_or(0, |(a, b)| b - a);
        self.clock_frame.store(v, Relaxed);
        // Late frames are skipped and early ones held, so the picture stays locked to the clock.
        let rx = self.play_rx.as_ref()?;
        let mut due = None;
        loop {
            if self.ahead.is_none() {
                self.ahead = rx.try_recv().ok();
            }
            match &self.ahead {
                Some((u, ..)) if *u <= v => due = self.ahead.take(),
                _ => break,
            }
        }
        due.map(|(_, img, err)| (img, err))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Clip;

    #[test]
    fn content_hash_follows_edits_not_the_view() {
        let mut p = Project::new("t");
        p.timeline.video_tracks[0].clips.push(Clip::new("c", 1.0, 2.0));
        let h = content_hash(&p);
        assert_eq!(h, content_hash(&p.clone()));
        (p.timeline.cursor_position, p.timeline.zoom, p.timeline.scroll_x) = (5.0, 3.0, 40.0);
        p.modified = true;
        assert_eq!(content_hash(&p), h);
        let edits: [&dyn Fn(&mut Project); 4] = [
            &|p| p.timeline.video_tracks[0].clips[0].position += 0.5,
            &|p| p.timeline.video_tracks[0].volume = 0.2,
            &|p| p.settings.width = 1280,
            &|p| p.media_pool.push(Default::default()),
        ];
        for (i, e) in edits.iter().enumerate() {
            let mut q = p.clone();
            e(&mut q);
            assert_ne!(content_hash(&q), h, "edit {i}");
        }
    }

    /// Needs ffmpeg. The engine end to end: a paused frame arrives, then playback delivers frames in order.
    #[test]
    #[ignore]
    fn engine_show_and_play() {
        let mut p = Project::new("t");
        let dir = std::env::temp_dir().join(format!("milan-engine-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("c.mp4");
        assert!(
            crate::proc::ffmpeg()
                .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=320x180:rate=30:duration=3", "-pix_fmt", "yuv420p"])
                .arg(&path)
                .status()
                .unwrap()
                .success()
        );
        let m = crate::media::probe_media(&path).unwrap();
        p.timeline.video_tracks[0].clips.push(Clip::from_media(&m, 0.0));
        p.media_pool.push(m);
        p.settings.frame_rate = 30.0;
        let mut e = VideoEngine::new(&egui::Context::default());
        let wait = |e: &mut VideoEngine, p: &Project| {
            let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
            loop {
                if let Some(r) = e.update(p, 320, 180, Quality::Good, true) {
                    return r;
                }
                assert!(std::time::Instant::now() < until, "timed out");
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        };
        p.timeline.cursor_position = 1.0;
        let (img, err) = wait(&mut e, &p);
        assert!(err.is_none() && img.size == [320, 180] && !e.busy());
        e.play(None);
        for k in 0..10 {
            p.timeline.cursor_position = 1.0 + k as f64 / 30.0;
            wait(&mut e, &p);
        }
        e.stop();
        drop(e);
        // The worker kills its decoders as it exits, which releases the file.
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::fs::remove_dir_all(&dir).is_err() {
            assert!(std::time::Instant::now() < until, "decoders still hold {}", dir.display());
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}
