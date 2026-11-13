//! Timeline engine: one plan of the project for preview, playback and render (docs/FEATURE_PLAN.md section 1).
//! Video: `plan` turns the project into frame plans, `worker` composes them on the "milan-video" thread
//! from persistent `vdecode` streams, and `VideoEngine` is the UI thread's handle to it.
//! Audio: `plan::compile_audio` and `mix` (driven by `audio::Player`), with `afx` FX chains.
//! Render: `render` runs both into one ffmpeg encoder.
pub mod afx;
pub mod cache;
pub mod composite;
// shortcut: the font list, plug-in catalogue, user presets, LUT export and chooser thumbnails are used by the
// batch-15 plug-in UI; drop these allows when it lands.
#[allow(dead_code)]
pub mod fonts;
pub mod generate;
// shortcut: key editing is used from batch 10 (keyframe UI) on.
#[allow(dead_code)]
pub mod keyframes;
#[allow(dead_code)]
pub mod lut;
pub mod loudness;
pub mod mix;
pub mod multicam;
pub mod nested;
pub mod ofx;
pub mod plan;
#[allow(dead_code)]
pub mod plugins;
pub mod raster;
pub mod render;
pub mod resample;
pub mod scopes;
pub mod sync;
#[allow(dead_code)]
pub mod thumbs;
pub mod transition;
pub mod vdecode;
pub mod velocity;
pub mod vfx;
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

/// The worker's playback figures, refreshed every 0.5 s while playing (R19: the Video Preview's playback statistics).
#[derive(Default)]
pub struct Stats {
    /// Frames composed per second.
    pub fps_x100: AtomicU32,
    /// Late or read-past layer frames so far.
    pub dropped: AtomicU64,
    pub live_decoders: AtomicU32,
    /// Source and composite cache lookups that hit, in the last half second.
    pub cache_hit_pct: AtomicU32,
}

/// R19: frames the Video Preview showed and skipped in one playback, and the frame rate it shows at.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Shows {
    pub shown: u64,
    pub dropped: u64,
    /// Frames shown per second of timeline played over the last second (the project rate when none dropped).
    pub fps: Option<f64>,
    /// (shown, dropped, frame) when the current second of timeline started.
    window: (u64, u64, i64),
}

impl Shows {
    /// Frame `u` (unwrapped) goes on screen after frame `prev` of the same session; the frames between were skipped.
    fn show(&mut self, prev: Option<i64>, u: i64, fps: f64) {
        self.dropped += prev.map_or(0, |p| (u - p - 1).max(0) as u64);
        self.shown += 1;
        let (s0, d0, u0) = self.window;
        if prev.is_none() || u < u0 {
            self.window = (self.shown, self.dropped, u);
        } else if (u - u0) as f64 >= fps {
            let (s, d) = (self.shown - s0, self.dropped - d0);
            self.fps = Some(fps * s as f64 / (s + d).max(1) as f64);
            self.window = (self.shown, self.dropped, u);
        }
    }
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
    // R6a: a nested timeline saved since (or any project file under it) changes the picture and the sound too.
    w.0.write_u64(nested::stamp(p));
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
    pub stats: Arc<Stats>,
    /// R19: what this playback showed (reset when playback starts).
    pub shows: Shows,
    /// The last frame shown in this play session.
    last_shown: Option<i64>,
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
    /// R16b: the picture without any video FX (Split-Screen View › FX Bypassed).
    no_fx: bool,
    /// R17b: the multicamera grid (`multicam::grid`) instead of the timeline's picture; set by the preview each frame.
    pub grid: bool,
}

/// `p` with every video FX taken off (event, track and Video Output FX), as Split-Screen View › FX Bypassed shows it.
pub fn without_video_fx(p: &Project) -> Project {
    let mut q = p.clone();
    for t in &mut q.timeline.video_tracks {
        t.fx.clear();
        t.clips.iter_mut().for_each(|c| c.fx.clear());
    }
    q.timeline.master.video_fx.clear();
    q
}

impl VideoEngine {
    /// R16b: an engine that composes the timeline with its video FX bypassed.
    pub fn without_fx(ctx: &egui::Context) -> Self {
        Self { no_fx: true, ..Self::new(ctx) }
    }

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
            shows: Shows::default(),
            last_shown: None,
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
            no_fx: false,
            grid: false,
        }
    }

    /// Start (or restart, after a seek) playback at the cursor, looping over `lp` if set.
    pub fn play(&mut self, lp: Option<(f64, f64)>) {
        if self.play.is_none() {
            self.shows = Shows::default();
        }
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
        // A proxy becoming ready replans, so Draft and Preview switch to it.
        let hash = content_hash(p) ^ crate::proxy::generation() ^ if self.grid { 0x6d75_6c74_6963_616d } else { 0 };
        if hash != self.sent_hash {
            let plan = match (self.grid, self.no_fx) {
                (true, _) => plan::compile(&multicam::grid(p)),
                (_, true) => plan::compile(&without_video_fx(p)),
                _ => plan::compile(p),
            };
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
            // R20: any answer newer than the picture on screen shows, so a drag's picture follows the cursor as fast as
            // the decoders go (waiting for the newest Show showed nothing until the cursor stopped).
            let shown = self.shown_seq;
            let (g, img, err) = self.latest.lock().unwrap().take_if(|s| s.0 > shown)?;
            self.shown_seq = g;
            return Some((img, err));
        };
        if self.play_rx.is_none() || self.session != Some((w, q)) {
            let (tx, rx) = mpsc::sync_channel(4);
            self.clock_frame.store(clock, Relaxed);
            let _ = self.tx.send(VCmd::Play { from_n: clock, w, h, q, lp, tx });
            (self.play_rx, self.ahead, self.session, self.last_shown) = (Some(rx), None, Some((w, q)), None);
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
        due.map(|(u, img, err)| {
            self.shows.show(self.last_shown, u, self.fps);
            self.last_shown = Some(u);
            (img, err)
        })
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

    /// R19: frames skipped between two shown are dropped, and the rate shown is the project rate times the share shown.
    #[test]
    fn shows_count_drops_and_rate() {
        let mut s = Shows::default();
        s.show(None, 10, 30.0);
        for u in 11..=40 {
            s.show(Some(u - 1), u, 30.0);
        }
        assert_eq!((s.shown, s.dropped, s.fps), (31, 0, Some(30.0)));
        for u in (42..=100).step_by(2) {
            s.show(Some(u - 2), u, 30.0);
        }
        assert_eq!((s.shown, s.dropped, s.fps), (61, 30, Some(15.0)));
        // A new session (a seek): no drop across it.
        s.show(None, 5, 30.0);
        assert_eq!(s.dropped, 30);
    }

    /// R16b: Split-Screen View › FX Bypassed composes the project without event, track and Video Output FX.
    #[test]
    fn fx_bypassed_plan_has_no_video_fx() {
        let mut p = Project::new("t");
        let fx = plugins::new_fx(plugins::find("milan.fx.invert").unwrap(), None);
        p.timeline.video_tracks[0].clips.push(Clip { fx: vec![fx.clone()], ..Clip::new("c", 0.0, 2.0) });
        p.timeline.video_tracks[0].fx = vec![fx.clone()];
        p.timeline.master.video_fx = vec![fx];
        let q = without_video_fx(&p);
        assert!(q.timeline.video_tracks[0].clips[0].fx.is_empty() && q.timeline.video_tracks[0].fx.is_empty() && q.timeline.master.video_fx.is_empty());
        assert_eq!(q.timeline.video_tracks[0].clips.len(), 1);
        assert_eq!(p.timeline.master.video_fx.len(), 1, "the project keeps its FX");
    }

    /// R19, needs ffmpeg; CPU heavy. Four blended 1080p layers at Best (Full), looped over their 12 s for 60 s, played
    /// through the Video Preview (`Preview`, a 60 Hz UI against a wall clock as audio drives it), with Adjust Size and
    /// Quality for Optimal Playback off and then on. `cargo test --release -- --ignored --nocapture bench_adaptive_quality`
    #[test]
    #[ignore]
    fn bench_adaptive_quality() {
        use crate::decode::{Preview, Size};
        use std::time::{Duration, Instant};
        let base = worker::tests::bench_project(4, 12);
        let run = |adjust: bool| {
            let ctx = egui::Context::default();
            let mut pv = Preview::timeline(&ctx);
            (pv.quality, pv.size, pv.display_w, pv.visible, pv.adjust) = (Quality::Best, Size::Full, 1920, true, adjust);
            let mut p = base.clone();
            let (start, mut at50, mut changes, mut setting) = (Instant::now(), None, Vec::new(), (Quality::Best, Size::Full));
            pv.play(Some((0.0, 12.0)));
            let mut shows = Shows::default();
            loop {
                let t = start.elapsed().as_secs_f64();
                if t >= 60.0 {
                    break;
                }
                p.timeline.cursor_position = t % 12.0;
                pv.update(&ctx, &p);
                // Drain the texture uploads as a UI frame would.
                let _ = ctx.run(egui::RawInput::default(), |_| {});
                shows = pv.playback().unwrap().0;
                if t >= 50.0 && at50.is_none() {
                    at50 = Some(shows);
                }
                if pv.effective(1920) != setting {
                    setting = pv.effective(1920);
                    changes.push(format!("{t:.1} s {:?} ({:?})", setting.0, setting.1));
                }
                std::thread::sleep(Duration::from_millis(16));
            }
            pv.stop();
            let pct = |s: u64, d: u64| 100.0 * d as f64 / (s + d).max(1) as f64;
            let a = at50.unwrap();
            let (all, last) = (pct(shows.shown, shows.dropped), pct(shows.shown - a.shown, shows.dropped - a.dropped));
            let name = if adjust { "on" } else { "off" };
            println!("bench_adaptive_quality adjust {name}: {all:.1} % dropped over 60 s, {last:.1} % over the last 10 s, settings {changes:?}");
            println!("BENCH adaptive adjust={name} drop_pct={all:.2} last10_drop_pct={last:.2} changes={} final={:?}({:?})", changes.len(), setting.0, setting.1);
            (all, last, changes.len())
        };
        let off = run(false);
        let on = run(true);
        assert_eq!(off.2, 0, "the setting never changes with the option off");
        if off.1 > 2.0 {
            assert!(on.1 < off.1 && on.2 >= 1, "adjusting drops fewer frames: off {off:?}, on {on:?}");
        }
        assert!(on.2 <= 6, "failed retries back off (10 s, 20 s, 40 s): {on:?}");
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
