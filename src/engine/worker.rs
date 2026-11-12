//! The "milan-video" thread: owns the plan, the decoder pool and both caches; answers Show and streams Play.
use eframe::egui::{self, ColorImage};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicI64, Ordering::Relaxed};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use crate::decode::Quality;
use crate::engine::cache::ByteLru;
use crate::engine::composite::{self, Sources};
use crate::engine::plan::{self, SrcAt, VideoPlan};
use crate::engine::vdecode::{DecoderPool, Pull};
use crate::engine::{Played, Shown, Stats, VCmd};
use crate::project::Project;

/// (media, source frame or -1 for a still, decode width, scaler)
type SrcKey = (Arc<str>, i64, u32, &'static str);
/// (media, decode width, height): one layer's picture, held while its decoder is late.
type HeldKey = (Arc<str>, u32, u32);
type StillDone = (SrcKey, Result<ColorImage, String>);

/// Media that failed to decode are not retried for this long (no ffmpeg spawn per frame for a missing file).
const RETRY: Duration = Duration::from_secs(2);
/// How long playback waits for a layer before holding its last frame.
const LATE: Duration = Duration::from_millis(25);

/// Still decodes run on their own threads, so playback never waits on a photo.
struct Stills {
    tx: Sender<StillDone>,
    rx: Receiver<StillDone>,
    pending: HashSet<SrcKey>,
}

impl Stills {
    fn start(&mut self, key: SrcKey, path: &Path, w: u32, h: u32) {
        if self.pending.insert(key.clone()) {
            let (tx, path) = (self.tx.clone(), path.to_path_buf());
            std::thread::spawn(move || {
                let img = crate::decode::still(&path, w, h, key.3);
                let _ = tx.send((key, img));
            });
        }
    }
}

pub struct Worker {
    plan: Arc<VideoPlan>,
    pool: DecoderPool,
    sources: ByteLru<SrcKey, Arc<ColorImage>>,
    /// A light Dynamic RAM Preview: frames keyed by what they show, so edits need no invalidation.
    composites: ByteLru<(u64, u32, u32, Quality), Arc<ColorImage>>,
    failed: HashMap<Arc<str>, (String, Instant)>,
    /// Each layer's picture in the last composed frame.
    held: HashMap<HeldKey, Arc<ColorImage>>,
    stills: Stills,
    k: usize,
}

/// The real `Sources`: source cache, then stills or the decoder pool.
struct Live<'a> {
    pool: &'a mut DecoderPool,
    sources: &'a mut ByteLru<SrcKey, Arc<ColorImage>>,
    failed: &'a mut HashMap<Arc<str>, (String, Instant)>,
    held: &'a HashMap<HeldKey, Arc<ColorImage>>,
    stills: &'a mut Stills,
    /// This frame's layers, to hold in the next one.
    shown: HashMap<HeldKey, Arc<ColorImage>>,
    q: Quality,
    deadline: Option<Instant>,
    /// Every layer is the frame asked for (none late): the result may be cached.
    exact: bool,
    err: Option<String>,
}

impl Live<'_> {
    fn fetch(&mut self, s: &SrcAt, w: u32, h: u32) -> Option<Arc<ColorImage>> {
        let key = (s.m.id.clone(), s.sf.map_or(-1, |sf| self.pool.clamp(&s.m, sf)), w, self.q.scaler());
        if let Some(img) = self.sources.get(&key) {
            return Some(img);
        }
        if let Some((e, _)) = self.failed.get(&s.m.id).filter(|(_, at)| at.elapsed() < RETRY) {
            self.err.get_or_insert_with(|| e.clone());
            return None;
        }
        let got = match s.sf {
            // Playing, a still not decoded yet (pre-warm missed it) loads in the background.
            None if self.deadline.is_some() => {
                self.stills.start(key, &s.m.path, w, h);
                self.exact = false;
                return None;
            }
            None => crate::decode::still(&s.m.path, w, h, self.q.scaler()).map(Arc::new),
            Some(sf) => match self.pool.frame(&s.m, sf, w, h, self.q, self.deadline) {
                Pull::Exact(img) => Ok(img),
                Pull::Held(img) => {
                    self.exact = false;
                    return img;
                }
                Pull::Err(e) => Err(e),
            },
        };
        match got {
            Ok(img) => {
                self.sources.insert(key, img.clone(), w as usize * h as usize * 4);
                Some(img)
            }
            Err(e) => {
                self.failed.insert(s.m.id.clone(), (e.clone(), Instant::now()));
                self.err.get_or_insert(e);
                None
            }
        }
    }
}

impl Sources for Live<'_> {
    fn get(&mut self, s: &SrcAt, w: u32, h: u32) -> Option<Arc<ColorImage>> {
        let k = (s.m.id.clone(), w, h);
        let mut img = self.fetch(s, w, h);
        if img.is_none() && self.deadline.is_some() {
            // Plan 1.3: a late layer holds its last frame, even when its stream is new (reverse, loop wrap, slip).
            img = self.held.get(&k).cloned();
        }
        if let Some(i) = &img {
            self.shown.insert(k, i.clone());
        }
        img
    }
}

impl Worker {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        let mut w = Self {
            plan: Arc::new(plan::compile(&Project::new(""))),
            pool: DecoderPool::default(),
            sources: ByteLru::new(192 << 20),
            composites: ByteLru::new(256 << 20),
            failed: HashMap::new(),
            held: HashMap::new(),
            stills: Stills { tx, rx, pending: HashSet::new() },
            k: composite::stripes(),
        };
        w.pool.set_rate(w.plan.fps, &w.plan.fps_str);
        w
    }

    /// For a render: every frame is new, so there is no composite cache (sources stay cached for stills).
    pub fn for_render() -> Self {
        Self { composites: ByteLru::new(0), ..Self::new() }
    }

    pub fn set_plan(&mut self, p: Arc<VideoPlan>) {
        if p.fps != self.plan.fps {
            // Source frame numbers are on the project-fps grid: cached frames would show the wrong time.
            self.sources.clear();
            self.composites.clear();
        }
        self.pool.set_rate(p.fps, &p.fps_str);
        self.plan = p;
    }

    /// Finished background still decodes into the source cache.
    fn take_stills(&mut self) {
        for (key, img) in self.stills.rx.try_iter() {
            self.stills.pending.remove(&key);
            match img {
                Ok(img) => {
                    let bytes = img.pixels.len() * 4;
                    self.sources.insert(key, Arc::new(img), bytes);
                }
                Err(e) => {
                    self.failed.insert(key.0, (e, Instant::now()));
                }
            }
        }
    }

    /// Start every decoder frame `fp` needs (and background still decodes when playing), so they open in parallel.
    fn warm(&mut self, fp: &plan::FramePlan, w: u32, h: u32, q: Quality, play: bool) {
        for (src, dw, dh) in composite::requests(fp, w as usize, h as usize) {
            let key = (src.m.id.clone(), src.sf.map_or(-1, |sf| self.pool.clamp(&src.m, sf)), dw, q.scaler());
            if self.sources.get(&key).is_some() || self.failed.get(&src.m.id).is_some_and(|(_, at)| at.elapsed() < RETRY) {
                continue;
            }
            match src.sf {
                Some(sf) => self.pool.prewarm(&src.m, sf, dw, dh, q, play),
                None if play => self.stills.start(key, &src.m.path, dw, dh),
                None => {}
            }
        }
    }

    /// Frame `n` at `w` x `h`, from the composite cache or composed now. Also returns the first decode error.
    pub fn frame(&mut self, n: i64, w: u32, h: u32, q: Quality, deadline: Option<Instant>) -> (Arc<ColorImage>, Option<String>) {
        self.take_stills();
        let fp = plan::frame_plan(&self.plan, n);
        let key = (fp.key(), w, h, q);
        if let Some(img) = self.composites.get(&key) {
            return (img, None);
        }
        if deadline.is_none() {
            // A frame that waits for its decoders: spawn them all before the first blocking read.
            self.warm(&fp, w, h, q, false);
        }
        let mut live = Live {
            pool: &mut self.pool,
            sources: &mut self.sources,
            failed: &mut self.failed,
            held: &self.held,
            stills: &mut self.stills,
            shown: HashMap::new(),
            q,
            deadline,
            exact: true,
            err: None,
        };
        let img = Arc::new(composite::compose(&fp, &mut live, w as usize, h as usize, q != Quality::Draft, self.k));
        let (exact, err, shown) = (live.exact, live.err, live.shown);
        self.held = shown;
        if exact && err.is_none() {
            self.composites.insert(key, img.clone(), w as usize * h as usize * 4);
        }
        (img, err)
    }

    /// Stream frames from `from_n` until the receiver goes away or another command arrives (returned).
    /// Frame numbers sent are unwrapped: past the loop end they keep counting while the picture wraps.
    #[allow(clippy::too_many_arguments)]
    pub fn play(
        &mut self,
        from_n: i64,
        w: u32,
        h: u32,
        q: Quality,
        lp: Option<(f64, f64)>,
        tx: &SyncSender<Played>,
        rx: &Receiver<VCmd>,
        clock: &AtomicI64,
        stats: &Stats,
    ) -> Option<VCmd> {
        let mut u = from_n;
        let (mut since, mut frames, mut warmed) = (Instant::now(), 0u32, None::<Instant>);
        loop {
            for c in rx.try_iter() {
                match c {
                    VCmd::Plan(p) => self.set_plan(p),
                    c => return Some(c),
                }
            }
            let fps = self.plan.fps;
            if u > from_n {
                // Behind the clock: skip to the next frame it will show.
                u = u.max(clock.load(Relaxed) + 1);
            }
            let lpn = lp.map(|l| plan::loop_frames(l, fps)).filter(|(a, b)| b > a);
            let n = match lpn {
                Some((a, b)) if u >= b => a + (u - b) % (b - a),
                _ => u,
            };
            if n as f64 >= plan::effective_end(self.plan.end, lp) * fps {
                // Past the end: nothing to show until the next command.
                match rx.recv() {
                    Ok(VCmd::Plan(p)) => {
                        self.set_plan(p);
                        continue;
                    }
                    other => return other.ok(),
                }
            }
            if warmed.is_none_or(|t| t.elapsed() > Duration::from_millis(500)) {
                warmed = Some(Instant::now());
                for s in plan::warm_frames(&self.plan, n, (1.5 * fps) as i64, lpn) {
                    let fp = plan::frame_plan(&self.plan, s);
                    self.warm(&fp, w, h, q, true);
                }
                let secs = since.elapsed().as_secs_f64().max(1e-3);
                stats.fps_x100.store((frames as f64 / secs * 100.0) as u32, Relaxed);
                stats.dropped.store(self.pool.dropped, Relaxed);
                stats.live_decoders.store(self.pool.live() as u32, Relaxed);
                (since, frames) = (Instant::now(), 0);
            }
            // The first frame waits for its decoders; later ones hold a late layer's last frame.
            let deadline = (u > from_n).then(|| Instant::now() + LATE);
            let (img, err) = self.frame(n, w, h, q, deadline);
            if tx.send((u, img, err)).is_err() {
                return None;
            }
            frames += 1;
            u += 1;
        }
    }

    pub fn prune(&mut self) {
        self.pool.prune();
    }
}

pub fn run(rx: Receiver<VCmd>, latest: Arc<Mutex<Option<Shown>>>, clock: Arc<AtomicI64>, stats: Arc<Stats>, ctx: egui::Context) {
    let mut wk = Worker::new();
    let mut next = None;
    loop {
        let first = match next.take() {
            Some(c) => c,
            None => match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(c) => c,
                Err(RecvTimeoutError::Timeout) => {
                    wk.prune();
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => return,
            },
        };
        // Plans apply in order; of the rest only the newest matters (scrubbing queues many Shows).
        let mut pending = None;
        for c in std::iter::once(first).chain(rx.try_iter()) {
            match c {
                VCmd::Plan(p) => wk.set_plan(p),
                c => pending = Some(c),
            }
        }
        match pending {
            Some(VCmd::Show { seq, n, w, h, q }) => {
                let (img, err) = wk.frame(n, w, h, q, None);
                *latest.lock().unwrap() = Some((seq, img, err));
                ctx.request_repaint();
            }
            Some(VCmd::Play { from_n, w, h, q, lp, tx }) => next = wk.play(from_n, w, h, q, lp, &tx, &rx, &clock, &stats),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Clip, Track, TrackKind};
    use std::sync::mpsc;

    /// `n` 1080p H.264 clips of `secs` seconds (cached between runs), on `n` tracks at Level 0.5 so every layer blends.
    fn bench_project(n: usize, secs: u32) -> Project {
        let dir = std::env::temp_dir().join("milan-bench");
        std::fs::create_dir_all(&dir).unwrap();
        let mut p = Project::new("bench");
        p.timeline.video_tracks.clear();
        for i in 0..n {
            let path = dir.join(format!("bench{i}-{secs}.mp4"));
            if !path.exists() {
                let src = format!("testsrc2=size=1920x1080:rate=30000/1001:duration={secs},hue=h={}", i * 90);
                let ok = crate::proc::ffmpeg()
                    .args(["-v", "error", "-y", "-f", "lavfi", "-i", &src, "-c:v", "libx264", "-preset", "veryfast", "-g", "250", "-pix_fmt", "yuv420p"])
                    .arg(&path)
                    .status()
                    .unwrap()
                    .success();
                assert!(ok);
            }
            let m = crate::media::probe_media(&path).unwrap();
            let mut t = Track::new(&format!("Video {i}"), TrackKind::Video);
            t.volume = 0.5;
            t.clips.push(Clip::from_media(&m, 0.0));
            p.timeline.video_tracks.push(t);
            p.media_pool.push(m);
        }
        p
    }

    /// Needs ffmpeg; CPU heavy. Plan 1.9 per layer count, gate on each. `cargo test --release -- --ignored --nocapture bench_playback`
    #[test]
    #[ignore]
    fn bench_playback() {
        for layers in 1..=4 {
            let (fps, skipped, dropped) = play_layers(layers);
            let unpaced = unpaced_fps(layers);
            println!("bench_playback {layers} layer(s): {fps:.2} fps real time, {skipped} skipped, {dropped} late or read-past layer frames; {unpaced:.1} fps unpaced");
            assert!(fps >= 29.5 && (skipped as f64) < 0.01 * 240.0, "{layers} layers: {fps:.2} fps, {skipped} skipped");
        }
    }

    /// Frames composed per second when nothing paces the worker: each waits for its decoders, none is skipped.
    fn unpaced_fps(layers: usize) -> f64 {
        let mut wk = Worker::new();
        wk.set_plan(Arc::new(plan::compile(&bench_project(layers, 12))));
        let start = Instant::now();
        for n in 0..240 {
            assert!(wk.frame(n, 960, 540, Quality::Preview, None).1.is_none());
        }
        240.0 / start.elapsed().as_secs_f64()
    }

    /// `layers` blended 1080p layers at Preview (960×540) against a 29.97 fps clock: (fps shown, frames skipped, late layer frames).
    fn play_layers(layers: usize) -> (f64, i64, u64) {
        let p = bench_project(layers, 12);
        let mut wk = Worker::new();
        wk.set_plan(Arc::new(plan::compile(&p)));
        let (tx, frames) = mpsc::sync_channel(4);
        let (_cmd_tx, cmd_rx) = mpsc::channel();
        let (clock, stats) = (Arc::new(AtomicI64::new(0)), Arc::new(Stats::default()));
        let (c2, s2) = (clock.clone(), stats.clone());
        let player = std::thread::spawn(move || wk.play(0, 960, 540, Quality::Preview, None, &tx, &cmd_rx, &c2, &s2));
        // A 29.97 fps clock from the first frame on, as audio would drive it. Like the UI, a frame is shown once its
        // number is due; a frame that arrives after a newer one is due is skipped.
        let first = frames.recv().unwrap();
        assert_eq!(first.0, 0);
        let (start, total) = (Instant::now(), 240i64);
        let (mut shown, mut skipped, mut on_screen, mut ahead) = (1i64, 0i64, 0i64, None);
        while on_screen < total {
            let now = (start.elapsed().as_secs_f64() * 30000.0 / 1001.0) as i64;
            clock.store(now, Relaxed);
            let mut due = 0;
            loop {
                ahead = ahead.or_else(|| frames.try_recv().ok().map(|f| f.0));
                match ahead {
                    Some(u) if u <= now => (due, on_screen, ahead) = (due + 1, u, None),
                    _ => break,
                }
            }
            (shown, skipped) = (shown + due.min(1), skipped + (due - 1).max(0));
            std::thread::sleep(Duration::from_millis(1));
        }
        let secs = start.elapsed().as_secs_f64();
        drop(frames);
        let _ = player.join();
        (shown as f64 / secs, skipped, stats.dropped.load(Relaxed))
    }

    /// Needs ffmpeg. A jump cut inside one file (1 s removed): the frames after the cut are the new shot, not the old one held.
    #[test]
    #[ignore]
    fn jump_cut_plays_exact_frames() {
        let mut p = bench_project(1, 12);
        let t = &mut p.timeline.video_tracks[0];
        t.volume = 1.0;
        let mut b = t.clips[0].clone();
        t.clips[0].duration = 3.0;
        (b.id, b.position, b.in_point, b.duration) = ("b".into(), 3.0, 4.0, 3.0);
        t.clips.push(b);
        let plan = Arc::new(plan::compile(&p));
        let cut = (3.0 * plan.fps).round() as i64;
        let mut wk = Worker::new();
        wk.set_plan(plan.clone());
        let (tx, frames) = mpsc::sync_channel(4);
        let (_cmd_tx, cmd_rx) = mpsc::channel();
        let (clock, stats) = (Arc::new(AtomicI64::new(0)), Arc::new(Stats::default()));
        let (c2, s2) = (clock.clone(), stats.clone());
        let player = std::thread::spawn(move || wk.play(cut - 60, 960, 540, Quality::Preview, None, &tx, &cmd_rx, &c2, &s2));
        let start = Instant::now();
        let (mut got, mut ahead) = (HashMap::new(), None);
        while !got.keys().any(|&u| u >= cut + 6) {
            // Like the UI: a frame is taken once it is due, so the worker runs at the clock's pace.
            let now = cut - 60 + (start.elapsed().as_secs_f64() * plan.fps) as i64;
            clock.store(now, Relaxed);
            loop {
                ahead = ahead.or_else(|| frames.try_recv().ok());
                match ahead.take() {
                    Some((u, img, _)) if u <= now => _ = got.insert(u, img),
                    other => {
                        ahead = other;
                        break;
                    }
                }
            }
            assert!(start.elapsed() < Duration::from_secs(10), "timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
        drop(frames);
        let _ = player.join();
        let mut reference = Worker::new();
        reference.set_plan(plan);
        // A frame the clock skipped was never sent; most of them must be there.
        let sent: Vec<i64> = (cut..cut + 6).filter(|n| got.contains_key(n)).collect();
        assert!(sent.len() >= 4, "{sent:?}");
        for n in sent {
            let (want, _) = reference.frame(n, 960, 540, Quality::Preview, None);
            assert!(got[&n].pixels == want.pixels, "frame {} after the cut is not the exact frame", n - cut);
        }
    }

    /// Needs ffmpeg. `cargo test --release -- --ignored --nocapture bench_scrub`
    #[test]
    #[ignore]
    fn bench_scrub() {
        let p = bench_project(1, 12);
        let mut wk = Worker::new();
        wk.set_plan(Arc::new(plan::compile(&p)));
        let mut times = Vec::new();
        // Forward steps, then jumps: a mix of live-stream reads, respawns and cache hits.
        let seq: Vec<i64> = (0..30).chain([300, 100, 250, 20, 301, 302, 303, 0, 1, 2]).collect();
        for n in seq {
            let t = Instant::now();
            let (_, err) = wk.frame(n, 960, 540, Quality::Preview, None);
            assert!(err.is_none(), "{err:?}");
            times.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        times.sort_by(f64::total_cmp);
        let pct = |p: f64| times[((times.len() - 1) as f64 * p).round() as usize];
        println!("bench_scrub: p50 {:.1} ms, p95 {:.1} ms, max {:.1} ms", pct(0.5), pct(0.95), pct(1.0));
    }
}
