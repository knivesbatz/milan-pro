//! The "milan-video" thread: owns the plan, the decoder pool and both caches; answers Show and streams Play.
use eframe::egui::{self, ColorImage};
use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;
use std::sync::atomic::{AtomicI64, Ordering::Relaxed};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use crate::decode::Quality;
use crate::engine::cache::ByteLru;
use crate::engine::composite::{self, Sources};
use crate::engine::{generate, lut};
use crate::engine::plan::{self, SrcAt, VideoPlan};
use crate::engine::vdecode::{DecoderPool, Pull};
use crate::engine::{Played, Shown, Stats, VCmd};
use crate::project::Project;

/// (media, source frame or -1 for a still, decode width, scaler, read from the proxy, frame grid bits)
type SrcKey = (Arc<str>, i64, u32, &'static str, bool, u64);
/// (media, decode width, height, how many layers of it at that size were drawn before this one in the frame): one
/// layer's picture, held while its decoder is late. Two layers of one file each hold their own.
type HeldKey = (Arc<str>, u32, u32, usize);
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
    /// Modified time of each media file when last drawn: a file rewritten on disk is decoded again.
    mtimes: HashMap<Arc<str>, Option<SystemTime>>,
}

fn src_key(pool: &DecoderPool, s: &SrcAt, w: u32, q: Quality) -> SrcKey {
    let n = match (&s.generator, s.sf) {
        // Generated media: keyed by its settings (and time, when it animates) and its height, which follows the
        // project's aspect rather than the decode width.
        (Some((e, t)), _) => {
            let mut h = DefaultHasher::new();
            (e, t.to_bits(), s.m.h).hash(&mut h);
            h.finish() as i64
        }
        (None, Some(sf)) => pool.clamp(&s.m, sf, q),
        (None, None) => -1,
    };
    (s.m.id.clone(), n, w, q.scaler(), s.m.proxy_for(q).is_some(), s.m.grid.to_bits())
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
    /// Layers drawn so far in this frame per (media, width, height).
    drawn: HashMap<(Arc<str>, u32, u32), usize>,
    q: Quality,
    deadline: Option<Instant>,
    /// Every layer is the frame asked for (none late): the result may be cached.
    exact: bool,
    err: Option<String>,
}

impl Live<'_> {
    fn fetch(&mut self, s: &SrcAt, w: u32, h: u32) -> Option<Arc<ColorImage>> {
        // R6a: an offline nested timeline says why, as a missing file does.
        if let Some(plan::Nested::Offline(why)) = s.nested.as_deref() {
            self.err.get_or_insert_with(|| format!("{}: {why}", s.m.path.file_name().unwrap_or_default().to_string_lossy()));
            return None;
        }
        let key = src_key(self.pool, s, w, self.q);
        if let Some(img) = self.sources.get(&key) {
            return Some(img);
        }
        if let Some((e, _)) = self.failed.get(&s.m.id).filter(|(_, at)| at.elapsed() < RETRY) {
            self.err.get_or_insert_with(|| e.clone());
            return None;
        }
        let got = match (&s.generator, s.sf) {
            (Some((e, t)), _) => Ok(Arc::new(generate::render(e, w, h, *t))),
            // Playing, a still not decoded yet (pre-warm missed it) loads in the background.
            (None, None) if self.deadline.is_some() => {
                self.stills.start(key, &s.m.path, w, h);
                self.exact = false;
                return None;
            }
            (None, None) => crate::decode::still(&s.m.path, w, h, self.q.scaler()).map(Arc::new),
            (None, Some(sf)) => match self.pool.frame(&s.m, sf, w, h, self.q, self.deadline) {
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
        let n = self.drawn.entry((s.m.id.clone(), w, h)).or_default();
        let k = (s.m.id.clone(), w, h, *n);
        *n += 1;
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
            mtimes: HashMap::new(),
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

    /// A media file rewritten on disk (a re-exported graphic) since it was last drawn: its streams and every cached
    /// frame go, so preview shows what a render would.
    fn check_files(&mut self, fp: &plan::FramePlan, w: u32, h: u32) {
        let mut changed = false;
        for (src, ..) in composite::requests(fp, w as usize, h as usize) {
            if src.generator.is_some() {
                continue;
            }
            let t = std::fs::metadata(&src.m.path).and_then(|m| m.modified()).ok();
            if self.mtimes.insert(src.m.id.clone(), t).is_some_and(|old| old != t) {
                self.pool.forget(&src.m.id);
                self.failed.remove(&src.m.id);
                changed = true;
            }
        }
        if changed {
            self.sources.clear();
            self.composites.clear();
        }
    }

    /// Start every decoder frame `fp` needs (and background still decodes when playing), so they open in parallel.
    fn warm(&mut self, fp: &plan::FramePlan, w: u32, h: u32, q: Quality, play: bool) {
        for (src, dw, dh) in composite::requests(fp, w as usize, h as usize) {
            let key = src_key(&self.pool, src, dw, q);
            // Generated media render when drawn; there is nothing to open ahead.
            if src.generator.is_some() || self.sources.touch(&key).is_some() || self.failed.get(&src.m.id).is_some_and(|(_, at)| at.elapsed() < RETRY) {
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
        self.check_files(&fp, w, h);
        // Motion blur / supersampling: the frame is the mean of sub-frames, so all of them name the picture.
        let subs: Vec<plan::FramePlan> = fp.samples.iter().map(|s| plan::frame_plan_at(&self.plan, n, s.0)).collect();
        let mut fk = fp.key();
        for s in &subs {
            let mut hs = DefaultHasher::new();
            (fk, s.key()).hash(&mut hs);
            fk = hs.finish();
        }
        let key = (fk, w, h, q);
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
            drawn: HashMap::new(),
            q,
            deadline,
            exact: true,
            err: None,
        };
        let (wu, hu, bilinear) = (w as usize, h as usize, q != Quality::Draft);
        let img = Arc::new(if subs.is_empty() {
            composite::compose(&fp, &mut live, wu, hu, bilinear, self.k)
        } else {
            let mut frames = Vec::new();
            for s in &subs {
                live.drawn.clear(); // each sub-frame draws the same layers: they hold the same pictures
                frames.push(composite::compose(s, &mut live, wu, hu, bilinear, self.k));
            }
            composite::mean(&frames, &fp.samples.iter().map(|s| s.1).collect::<Vec<_>>())
        });
        let (exact, mut err, shown) = (live.exact, live.err, live.shown);
        // A LUT that cannot load draws ungraded: say so in the preview, and fail a render like a missing file.
        err = err.or_else(|| fp.fx().into_iter().find_map(lut::error));
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
        let (mut since, mut frames, mut warmed, mut looked) = (Instant::now(), 0u32, None::<Instant>, self.lookups());
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
                let now = self.lookups();
                let (hits, all) = (now.0 - looked.0, now.1 - looked.1);
                if all > 0 {
                    stats.cache_hit_pct.store((hits * 100 / all) as u32, Relaxed);
                }
                (since, frames, looked) = (Instant::now(), 0, now);
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

    /// (hits, lookups) of the source and composite caches so far.
    fn lookups(&self) -> (u64, u64) {
        let (s, c) = (&self.sources, &self.composites);
        (s.hits + c.hits, s.hits + s.misses + c.hits + c.misses)
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
pub(crate) mod tests {
    use super::*;
    use crate::project::{Clip, Track, TrackKind};
    use std::sync::mpsc;

    /// A project with one 5 s event of generated media.
    fn generated(generator: crate::project::Fx) -> Project {
        use crate::project::{MediaItem, MediaType};
        let m = MediaItem { id: "g".into(), media_type: MediaType::Image, duration: 86400.0, generator: Some(generator), ..Default::default() };
        let mut p = Project::new("g");
        let mut c = Clip::from_media(&m, 0.0);
        c.duration = 5.0;
        p.timeline.video_tracks[0].clips.push(c);
        p.media_pool.push(m);
        p
    }

    fn red() -> crate::project::Fx {
        use crate::engine::plugins::{find, new_fx};
        let solid = find("milan.gen.solid").unwrap();
        new_fx(solid, solid.presets.iter().find(|p| p.name == "Red"))
    }

    /// No ffmpeg: generated media render in the worker, and the Video Output FX apply to the frame.
    #[test]
    fn generated_media_and_output_fx() {
        use crate::engine::plugins::{find, new_fx};
        let mut p = generated(red());
        let mut wk = Worker::new();
        wk.set_plan(Arc::new(plan::compile(&p)));
        let (img, err) = wk.frame(30, 320, 180, Quality::Good, None);
        assert!(err.is_none() && img.size == [320, 180] && img.pixels.iter().all(|p| *p == eframe::egui::Color32::RED));
        p.timeline.master.video_fx.push(new_fx(find("milan.fx.invert").unwrap(), None));
        wk.set_plan(Arc::new(plan::compile(&p)));
        let (img, _) = wk.frame(30, 320, 180, Quality::Good, None);
        assert!(img.pixels.iter().all(|p| *p == eframe::egui::Color32::from_rgb(0, 255, 255)));
    }

    /// R7: the Video Bus Track's Motion Blur Amount smears a picture that Track Motion moves (the sub-frames average),
    /// and an edit that only changes a sub-frame still reaches the cached frame.
    #[test]
    fn motion_blur_smears_track_motion() {
        use crate::project::{Curve, EnvKind, Envelope, Keyframe, Keyframes, Motion};
        use eframe::egui::Color32;
        let mut p = generated(red());
        p.settings.frame_rate = 30.0;
        let t = &mut p.timeline.video_tracks[0];
        // Half size, sweeping left to right over 2 s.
        let m = |x| Motion { x, w: 0.25, h: 0.25, ..Default::default() };
        t.motion = Keyframes { keys: vec![Keyframe { t: 0.0, v: m(-0.5), curve: Curve::Linear }, Keyframe { t: 2.0, v: m(0.5), curve: Curve::Linear }] };
        let mut wk = Worker::new();
        let frame = |wk: &mut Worker, p: &Project| {
            wk.set_plan(Arc::new(plan::compile(p)));
            wk.frame(30, 192, 108, Quality::Good, None).0
        };
        let sharp = frame(&mut wk, &p);
        let partial = |img: &ColorImage| img.pixels.iter().filter(|c| c.r() > 10 && c.r() < 245).count();
        assert!(partial(&sharp) < 60, "a sharp edge: {}", partial(&sharp));
        let env = |kind, v| Envelope { kind, points: Keyframes { keys: vec![Keyframe { t: 0.0, v, curve: Curve::Linear }] } };
        p.timeline.master.video_envelopes = vec![env(EnvKind::MotionBlur, 0.2)];
        let blurred = frame(&mut wk, &p);
        assert!(partial(&blurred) > 200, "smeared along the motion: {}", partial(&blurred));
        assert_eq!(blurred.pixels[54 * 192 + 96], Color32::RED, "the middle of the box is covered at every sub-frame");
        // Bypass Motion Blur on the track: sharp again.
        p.timeline.video_tracks[0].bypass_blur = true;
        assert!(frame(&mut wk, &p).pixels == sharp.pixels);
    }

    /// Generated media render at the project size: a new aspect must not reuse the old picture from the source cache.
    #[test]
    fn generated_media_follow_a_project_aspect_change() {
        use crate::engine::plugins::{find, new_fx};
        let mut p = generated(new_fx(find("milan.gen.checkerboard").unwrap(), None));
        let mut wk = Worker::new();
        wk.set_plan(Arc::new(plan::compile(&p)));
        wk.frame(30, 320, 180, Quality::Good, None);
        p.settings.height = 800;
        let fresh = |p: &Project| {
            let mut wk = Worker::new();
            wk.set_plan(Arc::new(plan::compile(p)));
            wk.frame(30, 320, 134, Quality::Good, None).0
        };
        wk.set_plan(Arc::new(plan::compile(&p)));
        assert!(wk.frame(30, 320, 134, Quality::Good, None).0.pixels == fresh(&p).pixels);
    }

    /// A LUT that cannot load is reported (the frame draws ungraded), and a LUT rewritten in place shows at once.
    #[test]
    fn lut_errors_are_reported_and_an_edited_lut_shows() {
        use crate::engine::plugins::{find, new_fx, GRADING};
        use eframe::egui::Color32;
        let dir = std::env::temp_dir().join(format!("milan-lut-worker-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("look.cube");
        let mut p = generated(red());
        let mut g = new_fx(find(GRADING).unwrap(), None);
        g.text.insert("look_lut".into(), path.display().to_string());
        p.timeline.master.video_fx.push(g);
        let mut wk = Worker::new();
        wk.set_plan(Arc::new(plan::compile(&p)));
        let mut frame = |want: Color32| {
            let (img, err) = wk.frame(30, 64, 36, Quality::Good, None);
            assert!(img.pixels.iter().all(|p| *p == want), "{:?}", img.pixels[0]);
            err
        };
        assert!(frame(Color32::RED).is_some_and(|e| e.contains("look.cube")), "a missing LUT is an error");
        std::fs::write(&path, "LUT_1D_SIZE 2\n0 0 0\n1 1 1\n").unwrap();
        assert_eq!(frame(Color32::RED), None);
        // The same path, now inverting: the cached frame is stale.
        std::fs::write(&path, "LUT_1D_SIZE 2\n1 1 1\n0 0 0\n").unwrap();
        let later = std::time::SystemTime::now() + Duration::from_secs(10);
        std::fs::File::options().write(true).open(&path).unwrap().set_modified(later).unwrap();
        assert_eq!(frame(Color32::from_rgb(0, 255, 255)), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Two late layers of one file (a dissolve inside one recording) each hold their own last picture, not the other's.
    #[test]
    fn late_layers_of_one_file_hold_their_own_picture() {
        use eframe::egui::Color32;
        let m = Arc::new(plan::MediaRef {
            id: "s".into(),
            path: "s.png".into(),
            w: 4,
            h: 4,
            duration: 5.0,
            fps: 0.0,
            still: true,
            has_alpha: false,
            matrix: None,
            codec: None,
            deint: "",
            proxy: None,
            grid: 0.0,
            offset: 0.0,
            stream: 0,
            flow: false,
        });
        let at = SrcAt { m: m.clone(), sf: None, generator: None, nested: None };
        let solid = |c| Arc::new(ColorImage { size: [4, 4], pixels: vec![c; 16] });
        let held: HashMap<HeldKey, Arc<ColorImage>> = [((m.id.clone(), 4, 4, 0), solid(Color32::RED)), ((m.id.clone(), 4, 4, 1), solid(Color32::BLUE))].into();
        let (mut pool, mut sources, mut failed) = (DecoderPool::default(), ByteLru::new(1 << 20), HashMap::new());
        let (tx, rx) = mpsc::channel();
        // The still is already loading in the background: the layers are late.
        let mut stills = Stills { tx, rx, pending: HashSet::from([src_key(&pool, &at, 4, Quality::Good)]) };
        let mut live = Live {
            pool: &mut pool,
            sources: &mut sources,
            failed: &mut failed,
            held: &held,
            stills: &mut stills,
            shown: HashMap::new(),
            drawn: HashMap::new(),
            q: Quality::Good,
            deadline: Some(Instant::now() + LATE),
            exact: true,
            err: None,
        };
        let first = live.get(&at, 4, 4).unwrap().pixels[0];
        let second = live.get(&at, 4, 4).unwrap().pixels[0];
        assert_eq!((first, second), (Color32::RED, Color32::BLUE));
    }

    /// Needs ffmpeg. A still re-exported to the same path shows at once in preview, as it does in a render.
    #[test]
    #[ignore]
    fn a_still_rewritten_on_disk_shows_the_new_picture() {
        use crate::project::{MediaItem, MediaType};
        use eframe::egui::Color32;
        let dir = std::env::temp_dir().join(format!("milan-rewrite-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("lower-third.png");
        let write = |rgb: [u8; 3], later: u64| {
            let mut e = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(&path).unwrap()), 8, 8);
            e.set_color(png::ColorType::Rgb);
            e.write_header().unwrap().write_image_data(&rgb.repeat(64)).unwrap();
            std::fs::File::options().write(true).open(&path).unwrap().set_modified(std::time::SystemTime::now() + Duration::from_secs(later)).unwrap();
        };
        write([255, 0, 0], 0);
        let m = MediaItem { id: "png".into(), path: path.clone(), media_type: MediaType::Image, duration: 5.0, width: Some(8), height: Some(8), ..Default::default() };
        let mut p = Project::new("r");
        p.timeline.video_tracks[0].clips.push(Clip::from_media(&m, 0.0));
        p.media_pool.push(m);
        let mut wk = Worker::new();
        wk.set_plan(Arc::new(plan::compile(&p)));
        let centre = |wk: &mut Worker| wk.frame(10, 64, 36, Quality::Good, None).0.pixels[18 * 64 + 32];
        assert_eq!(centre(&mut wk), Color32::RED);
        write([0, 0, 255], 10);
        assert_eq!(centre(&mut wk), Color32::BLUE);
        drop(wk);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// `n` 1080p H.264 clips of `secs` seconds (cached between runs in `MILAN_BENCH_DIR`, else `%TEMP%\milan-bench`), on
    /// `n` tracks at Level 0.5 so every layer blends. `tools/bench.ps1` runs the benches below and writes PERFORMANCE.md.
    pub(crate) fn bench_project(n: usize, secs: u32) -> Project {
        let dir = std::env::var_os("MILAN_BENCH_DIR").map_or_else(|| std::env::temp_dir().join("milan-bench"), std::path::PathBuf::from);
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
            let r = play_layers(layers);
            let unpaced = unpaced_fps(layers);
            println!("bench_playback {layers} layer(s): {r}; {unpaced:.1} fps unpaced");
            println!("BENCH playback layers={layers} {} unpaced_fps={unpaced:.1}", r.fields());
            assert!(r.fps >= 29.5 && r.drop_pct() < 1.0, "{layers} layers: {r}");
        }
    }

    /// R16b: `bench_playback` with the Video Scopes open and Update Scopes while Playing on: all four scopes of a
    /// 960x540 frame 10 times a second on their own thread. `cargo test --release -- --ignored --nocapture bench_playback_with_scopes`
    #[test]
    #[ignore]
    fn bench_playback_with_scopes() {
        use crate::engine::scopes::{HistMode, Levels, Want, analyze};
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let s2 = stop.clone();
        let scopes = std::thread::spawn(move || {
            let (w, h) = (960, 540);
            let img = ColorImage { size: [w, h], pixels: (0..w * h).map(|i| egui::Color32::from_rgb((i % 251) as u8, (i % 241) as u8, (i / w % 256) as u8)).collect() };
            let want = Want { wave: true, vector: true, parade: true, hist: Some(HistMode::LumaRgb), levels: Levels::default(), composite: false, magnified: false };
            let mut n = 0;
            while !s2.load(Relaxed) {
                let t = Instant::now();
                std::hint::black_box(analyze(&img, &want));
                n += 1;
                std::thread::sleep(Duration::from_millis(100).saturating_sub(t.elapsed()));
            }
            n
        });
        for layers in 1..=4 {
            let r = play_layers(layers);
            println!("bench_playback_with_scopes {layers} layer(s): {r}");
            println!("BENCH scopes layers={layers} {}", r.fields());
            assert!(r.fps >= 29.5 && r.drop_pct() < 1.0, "{layers} layers: {r}");
        }
        stop.store(true, Relaxed);
        println!("bench_playback_with_scopes: {} analyses", scopes.join().unwrap());
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

    /// R17b: playback with the multicamera grid showing: the four 1080p clips of `bench_project` made one multicamera
    /// track (Create Multicamera Track), its four takes in the Video Preview's grid at Preview (960x540), against a
    /// 29.97 fps clock. `cargo test --release -- --ignored --nocapture bench_playback_multicam_grid`
    #[test]
    #[ignore]
    fn bench_playback_multicam_grid() {
        let mut p = bench_project(4, 12);
        let ids: Vec<String> = p.timeline.video_tracks.iter().flat_map(|t| t.clips.iter().map(|c| c.id.clone())).collect();
        crate::edit::create_multicam(&mut p, &ids).unwrap();
        assert_eq!(p.timeline.video_tracks[0].clips[0].takes.len(), 3);
        let grid = crate::engine::multicam::grid(&p);
        let r = play_project(&grid);
        println!("bench_playback_multicam_grid 4 takes: {r}");
        println!("BENCH multicam takes=4 {}", r.fields());
        assert!(r.fps >= 29.5 && r.drop_pct() < 1.0, "{r}");
    }

    /// R19: one measured playback of 240 frames.
    pub(crate) struct Played {
        /// Frames shown per second of wall time.
        pub fps: f64,
        pub shown: i64,
        /// Frames never shown: the worker skipped them to catch up with the clock, or a newer one was due first.
        pub skipped: i64,
        /// Late or read-past layer frames (`Stats::dropped`).
        pub late_layers: u64,
        /// From Play to the first frame.
        pub start_ms: f64,
        /// How far each frame shown was behind the clock when it went on screen (video behind audio): p50, p95, max.
        pub av_ms: [f64; 3],
    }

    impl Played {
        pub fn drop_pct(&self) -> f64 {
            100.0 * self.skipped as f64 / (self.shown + self.skipped).max(1) as f64
        }

        /// key=value fields for `tools/bench.ps1`.
        pub fn fields(&self) -> String {
            let [a, b, c] = self.av_ms;
            format!(
                "fps={:.2} drop_pct={:.2} late_layer_frames={} start_ms={:.0} av_p50_ms={a:.1} av_p95_ms={b:.1} av_max_ms={c:.1}",
                self.fps,
                self.drop_pct(),
                self.late_layers,
                self.start_ms
            )
        }
    }

    impl std::fmt::Display for Played {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            let [a, b, _] = self.av_ms;
            let (fps, skipped, pct, late, start) = (self.fps, self.skipped, self.drop_pct(), self.late_layers, self.start_ms);
            write!(f, "{fps:.2} fps real time, {skipped} skipped ({pct:.2} %), {late} late or read-past layer frames, first frame {start:.0} ms, video behind the clock p50 {a:.1} ms p95 {b:.1} ms")
        }
    }

    /// p50, p95 and max of `v` (sorted in place).
    pub(crate) fn percentiles(v: &mut [f64]) -> [f64; 3] {
        v.sort_by(f64::total_cmp);
        let pct = |p: f64| v.get(((v.len().max(1) - 1) as f64 * p).round() as usize).copied().unwrap_or(0.0);
        [pct(0.5), pct(0.95), pct(1.0)]
    }

    /// `layers` blended 1080p layers at Preview (960×540) against a 29.97 fps clock.
    fn play_layers(layers: usize) -> Played {
        play_project(&bench_project(layers, 12))
    }

    /// `p` played at Preview (960×540) against a 29.97 fps clock.
    fn play_project(p: &Project) -> Played {
        let mut wk = Worker::new();
        wk.set_plan(Arc::new(plan::compile(p)));
        let (tx, frames) = mpsc::sync_channel(4);
        let (_cmd_tx, cmd_rx) = mpsc::channel();
        let (clock, stats) = (Arc::new(AtomicI64::new(0)), Arc::new(Stats::default()));
        let (c2, s2) = (clock.clone(), stats.clone());
        let asked = Instant::now();
        let player = std::thread::spawn(move || wk.play(0, 960, 540, Quality::Preview, None, &tx, &cmd_rx, &c2, &s2));
        // A 29.97 fps clock from the first frame on, as audio would drive it. Like the UI, a frame is shown once its
        // number is due; a frame that arrives after a newer one is due is skipped.
        let first = frames.recv().unwrap();
        let start_ms = asked.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(first.0, 0);
        let (start, total, fps) = (Instant::now(), 240i64, 30000.0 / 1001.0);
        let (mut shown, mut skipped, mut on_screen, mut ahead, mut behind) = (1i64, 0i64, 0i64, None, Vec::new());
        while on_screen < total {
            let t = start.elapsed().as_secs_f64();
            let now = (t * fps) as i64;
            clock.store(now, Relaxed);
            let mut due = 0;
            loop {
                ahead = ahead.or_else(|| frames.try_recv().ok().map(|f| f.0));
                match ahead {
                    Some(u) if u <= now => (due, on_screen, ahead) = (due + 1, u, None),
                    _ => break,
                }
            }
            if due > 0 {
                behind.push((t - on_screen as f64 / fps) * 1000.0);
            }
            (shown, skipped) = (shown + due.min(1), skipped + (due - 1).max(0));
            std::thread::sleep(Duration::from_millis(1));
        }
        let secs = start.elapsed().as_secs_f64();
        drop(frames);
        let _ = player.join();
        Played { fps: shown as f64 / secs, shown, skipped, late_layers: stats.dropped.load(Relaxed), start_ms, av_ms: percentiles(&mut behind) }
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
        let [p50, p95, max] = percentiles(&mut times);
        println!("bench_scrub: p50 {p50:.1} ms, p95 {p95:.1} ms, max {max:.1} ms");
        println!("BENCH scrub p50_ms={p50:.1} p95_ms={p95:.1} max_ms={max:.1}");
    }
}
