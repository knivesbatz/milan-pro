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
use crate::engine::diskcache::{self, DiskCache};
use crate::engine::{caches, generate, lut, prerender};
use crate::engine::picture::Picture;
use crate::engine::plan::{self, SrcAt, VideoPlan};
use crate::engine::vdecode::{DecoderPool, Pull};
use crate::engine::{Played, RamStatus, Shown, Stats, VCmd};
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

/// R23: one frame of the Dynamic RAM Preview: what it showed (`plan::frame_key`), at what size and quality.
struct RamFrame {
    key: u64,
    w: u32,
    h: u32,
    q: Quality,
    img: Arc<Picture>,
}

pub struct Worker {
    plan: Arc<VideoPlan>,
    pool: DecoderPool,
    sources: ByteLru<SrcKey, Arc<ColorImage>>,
    /// Frames keyed by what they show, so edits need no invalidation (the cache playback fills by itself).
    composites: ByteLru<(u64, u32, u32, Quality), Arc<Picture>>,
    failed: HashMap<Arc<str>, (String, Instant)>,
    /// Each layer's picture in the last composed frame.
    held: HashMap<HeldKey, Arc<ColorImage>>,
    stills: Stills,
    k: usize,
    /// Modified time of each media file when last drawn: a file rewritten on disk is decoded again.
    mtimes: HashMap<Arc<str>, Option<SystemTime>>,
    /// R23: the Dynamic RAM Preview (Shift+B): frames pinned until an edit changes them or the next build; an edit
    /// drops only the frames it changed (`revalidate`). Only the timeline's own worker owns one (`ram_owner`).
    ram: HashMap<i64, RamFrame>,
    ram_bytes: usize,
    ram_owner: bool,
    ram_full: bool,
    building: bool,
    /// R23: the project's prerendered sections and whether each still matches the plan, and the registry generation
    /// they were read at (None: not read for this plan yet).
    pre: Vec<(prerender::Entry, bool)>,
    pre_gen: Option<u64>,
    /// R23: the disk cache for decoded frames of heavy sources (Preferences › Video; None while it is off).
    disk: Option<DiskCache>,
    /// This worker serves a render: no RAM preview, no disk cache (every frame is new), prerendered sections yes.
    render: bool,
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
    /// R23: the disk frame cache, read and written for paused frames of heavy sources.
    disk: Option<&'a mut DiskCache>,
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
            (None, Some(sf)) => {
                // R23: a paused frame of a heavy source (a seek, a RAM preview build) comes from the disk cache when
                // it is there, and goes into it once decoded; playback never waits on the disk.
                let heavy = self.deadline.is_none() && s.m.proxy_for(self.q).is_none() && crate::engine::hw::heavy(s.m.codec.as_deref(), s.m.w);
                let dkey = self.disk.as_ref().filter(|_| heavy).map(|_| diskcache::key(&key.0, key.1, w, h, key.3, key.4, key.5));
                if let Some(img) = dkey.and_then(|k| self.disk.as_mut()?.get(k)) {
                    self.sources.insert(key, img.clone(), w as usize * h as usize * 4);
                    return Some(img);
                }
                match self.pool.frame(&s.m, sf, w, h, self.q, self.deadline) {
                    Pull::Exact(img) => {
                        if let (Some(k), Some(d)) = (dkey, self.disk.as_mut()) {
                            d.put(k, img.clone());
                        }
                        Ok(img)
                    }
                    Pull::Held(img) => {
                        self.exact = false;
                        return img;
                    }
                    Pull::Err(e) => Err(e),
                }
            }
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
            ram: HashMap::new(),
            ram_bytes: 0,
            ram_owner: false,
            ram_full: false,
            building: false,
            pre: Vec::new(),
            pre_gen: None,
            disk: None,
            render: false,
        };
        w.pool.set_rate(w.plan.fps, &w.plan.fps_str);
        w
    }

    /// For a render: every frame is new, so there is no composite cache (sources stay cached for stills).
    pub fn for_render() -> Self {
        Self { composites: ByteLru::new(0), render: true, ..Self::new() }
    }

    pub fn set_plan(&mut self, p: Arc<VideoPlan>) {
        if p.fps != self.plan.fps {
            // Source frame numbers are on the project-fps grid: cached frames would show the wrong time.
            self.sources.clear();
            self.composites.clear();
        }
        self.pool.set_rate(p.fps, &p.fps_str);
        self.plan = p;
        self.revalidate();
    }

    /// R23: after an edit, the RAM preview keeps the frames the plan still composes the same way and drops the rest,
    /// and the prerendered sections are checked against the plan again; the ruler's bars follow.
    fn revalidate(&mut self) {
        if !self.ram.is_empty() {
            let plan = &self.plan;
            self.ram.retain(|n, r| plan::frame_key(plan, *n) == r.key);
            self.ram_bytes = self.ram.values().map(|r| r.img.bytes()).sum();
        }
        self.pre_gen = None;
        self.sync_prerender();
    }

    /// R23: the prerendered sections, read again when the registry or the plan changed, each checked against the
    /// plan without proxies (what they were rendered from); then the bars are published.
    fn sync_prerender(&mut self) {
        let generation = prerender::generation();
        if self.pre_gen == Some(generation) {
            return;
        }
        self.pre_gen = Some(generation);
        let entries = prerender::entries();
        self.pre = if entries.is_empty() {
            Vec::new()
        } else {
            let canonical = (*self.plan).clone().without_proxies();
            entries.into_iter().map(|e| (e.valid(&canonical), e)).map(|(ok, e)| (e, ok)).collect()
        };
        self.publish_bars();
    }

    /// R23: the ruler's bars: this worker's RAM preview (when it owns one) and the active prerendered sections.
    fn publish_bars(&mut self) {
        let fps = self.plan.fps;
        let ram = self.ram_owner.then(|| {
            let mut frames: Vec<i64> = self.ram.keys().copied().collect();
            frames.sort_unstable();
            let mut v: Vec<(i64, i64)> = Vec::new();
            for n in frames {
                match v.last_mut() {
                    Some(r) if r.1 == n => r.1 = n + 1,
                    _ => v.push((n, n + 1)),
                }
            }
            v.into_iter().map(|(a, b)| (a as f64 / fps, b as f64 / fps)).collect()
        });
        let pre = self.pre.iter().filter(|(_, ok)| *ok).map(|(e, _)| e.span()).collect();
        if !self.render {
            crate::engine::set_bars(ram, pre);
        }
        if self.ram_owner {
            crate::engine::set_ram_status(RamStatus { building: None, frames: self.ram.len() as i64, bytes: self.ram_bytes, full: self.ram_full });
        }
    }

    /// R23: the disk frame cache follows Preferences › Video (opened when a size is set, closed at 0).
    fn sync_disk(&mut self) {
        let want = if self.render { 0 } else { caches::disk_bytes() };
        match (&mut self.disk, want) {
            (Some(_), 0) => self.disk = None,
            (Some(d), n) => d.set_cap(n),
            (None, 0) => {}
            (None, n) => self.disk = caches::disk_dir().map(|dir| DiskCache::open(dir, n)),
        }
    }

    /// R23: frame `n` of a prerendered section that is still valid: read from its file through the decoder pool at
    /// `w` x `h` (None: not prerendered, or the file could not give the frame, which composes it as usual).
    fn from_prerender(&mut self, n: i64, w: u32, h: u32, q: Quality, deadline: Option<Instant>) -> Option<Arc<Picture>> {
        self.sync_prerender();
        let e = self.pre.iter().find(|(e, ok)| *ok && (e.a..e.b).contains(&n))?.0.clone();
        let m = e.media_ref();
        match self.pool.frame(&m, n - e.a, w, h, q, deadline) {
            Pull::Exact(img) | Pull::Held(Some(img)) => Some(Arc::new(Picture::cpu(img))),
            _ => None,
        }
    }

    /// R23: Build Dynamic RAM Preview: frames `a..b` composed now (waiting for every decoder) and pinned, up to the
    /// cap in Preferences › Video; `Cancel` stops it, a new plan applies as it goes (and drops frames it changed), and
    /// any other command arriving is handed back to run after.
    pub fn build(&mut self, a: i64, b: i64, w: u32, h: u32, q: Quality, rx: &Receiver<VCmd>) -> Option<VCmd> {
        self.ram_owner = true;
        self.ram.clear();
        (self.ram_bytes, self.ram_full, self.building) = (0, false, true);
        let cap = caches::ram_bytes();
        let mut pending = None;
        for n in a..b {
            for c in rx.try_iter() {
                match c {
                    VCmd::Plan(p) => self.set_plan(p),
                    VCmd::Cancel => {
                        self.building = false;
                        self.publish_bars();
                        return pending;
                    }
                    c => pending = Some(c),
                }
            }
            crate::engine::set_ram_status(RamStatus { building: Some((n - a, b - a)), frames: self.ram.len() as i64, bytes: self.ram_bytes, full: false });
            let key = plan::frame_key(&self.plan, n);
            let (img, _) = self.frame(n, w, h, q, None);
            if self.ram_bytes + img.bytes() > cap {
                self.ram_full = true;
                break;
            }
            self.ram_bytes += img.bytes();
            self.ram.insert(n, RamFrame { key, w, h, q, img });
        }
        self.building = false;
        self.publish_bars();
        pending
    }

    /// R23: Clear Dynamic RAM Preview.
    pub fn clear_ram(&mut self) {
        self.ram_owner = true;
        self.ram.clear();
        (self.ram_bytes, self.ram_full) = (0, false);
        self.publish_bars();
    }

    /// R23: the RAM preview's frames, for tests.
    #[cfg(test)]
    pub fn ram_frames(&self) -> Vec<i64> {
        let mut v: Vec<i64> = self.ram.keys().copied().collect();
        v.sort_unstable();
        v
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

    /// R20: a layer of frame `s` whose next frame is an earlier one of the same picture (a reversed event, here or
    /// starting soon) starts reading backwards now, so its first frames are not held while the chunks decode.
    fn warm_back(&mut self, s: i64, w: u32, h: u32, q: Quality) {
        let (a, b) = (plan::frame_plan(&self.plan, s), plan::frame_plan(&self.plan, s + 1));
        let next = composite::requests(&b, w as usize, h as usize);
        let reach = self.plan.fps.ceil() as i64;
        for (src, dw, dh) in composite::requests(&a, w as usize, h as usize) {
            let (Some(sf), None) = (src.sf, &src.generator) else { continue };
            if self.failed.get(&src.m.id).is_some_and(|(_, at)| at.elapsed() < RETRY) {
                continue;
            }
            let back = next.iter().find(|n| n.0.m.id == src.m.id && (n.1, n.2) == (dw, dh) && n.0.sf.is_some_and(|b| b < sf && sf - b <= reach));
            // Already decoded (a loop's second pass): nothing to read.
            if let Some((b, ..)) = back.filter(|n| self.sources.touch(&src_key(&self.pool, n.0, dw, q)).is_none()) {
                self.pool.prewarm_back(&b.m, b.sf.unwrap_or_default(), dw, dh, q);
            }
        }
    }

    /// Frame `n` at `w` x `h`, from the composite cache or composed now. Also returns the first decode error.
    pub fn frame(&mut self, n: i64, w: u32, h: u32, q: Quality, deadline: Option<Instant>) -> (Arc<Picture>, Option<String>) {
        self.take_stills();
        let fp = plan::frame_plan(&self.plan, n);
        self.check_files(&fp, w, h);
        // Motion blur / supersampling: the frame is the mean of sub-frames, so all of them name the picture.
        let subs: Vec<plan::FramePlan> = fp.samples.iter().map(|s| plan::frame_plan_at(&self.plan, n, s.0)).collect();
        let fk = plan::key_with_subs(&self.plan, n, &fp);
        // R23: the Dynamic RAM Preview first (its frames stay while the plan composes them the same way), then the
        // composite cache, then a prerendered section's file.
        if let Some(r) = self.ram.get(&n).filter(|r| r.key == fk && (r.w, r.h, r.q) == (w, h, q)) {
            return (r.img.clone(), None);
        }
        let key = (fk, w, h, q);
        if let Some(img) = self.composites.get(&key) {
            return (img, None);
        }
        if let Some(img) = self.from_prerender(n, w, h, q, deadline) {
            return (img, None);
        }
        self.sync_disk();
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
            disk: self.disk.as_mut(),
            shown: HashMap::new(),
            drawn: HashMap::new(),
            q,
            deadline,
            exact: true,
            err: None,
        };
        let (wu, hu, bilinear, k) = (w as usize, h as usize, q != Quality::Draft, self.k);
        // R21a: on the GPU when Preferences › Video chose one, renders too (so a render shows what was previewed); a
        // frame the GPU cannot do composes on the CPU.
        let compositor = crate::engine::gpu::compositor();
        let compose = |fp: &plan::FramePlan, live: &mut Live| {
            compositor.compose(fp, live, wu, hu, bilinear, k).unwrap_or_else(|| {
                live.drawn.clear(); // a GPU attempt may have drawn some layers already
                Picture::cpu(Arc::new(composite::compose(fp, live, wu, hu, bilinear, k)))
            })
        };
        let img = Arc::new(if subs.is_empty() {
            compose(&fp, &mut live)
        } else {
            // Motion blur / supersampling: the mean of the sub-frames, on the CPU (each read back from the GPU).
            let mut frames = Vec::new();
            for s in &subs {
                live.drawn.clear(); // each sub-frame draws the same layers: they hold the same pictures
                frames.push((*compose(s, &mut live).image()).clone());
            }
            Picture::cpu(Arc::new(composite::mean(&frames, &fp.samples.iter().map(|s| s.1).collect::<Vec<_>>())))
        });
        let (exact, mut err, shown) = (live.exact, live.err, live.shown);
        // A LUT that cannot load draws ungraded: say so in the preview, and fail a render like a missing file.
        err = err.or_else(|| fp.fx().into_iter().find_map(lut::error));
        self.held = shown;
        // A frame built for the RAM preview is pinned there, not cached twice.
        if exact && err.is_none() && !self.building {
            self.composites.insert(key, img.clone(), img.bytes());
        }
        (img, err)
    }

    /// R23: a prerendered section starting within `ahead` frames of `n` opens its stream now; the frame after one ends
    /// warms its sources, which the section left idle.
    fn warm_prerender(&mut self, n: i64, ahead: i64, w: u32, h: u32, q: Quality) {
        self.sync_prerender();
        let soon: Vec<prerender::Entry> = self.pre.iter().filter(|(e, ok)| *ok && ((n + 1..=n + ahead).contains(&e.a) || (n + 1..=n + ahead).contains(&e.b))).map(|(e, _)| e.clone()).collect();
        for e in soon {
            if (n + 1..=n + ahead).contains(&e.a) {
                self.pool.prewarm(&e.media_ref(), 0, w, h, q, true);
            }
            if (n + 1..=n + ahead).contains(&e.b) {
                let fp = plan::frame_plan(&self.plan, e.b);
                self.warm(&fp, w, h, q, true);
            }
        }
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
                let starts = plan::warm_frames(&self.plan, n, (1.5 * fps) as i64, lpn);
                for &s in &starts {
                    let fp = plan::frame_plan(&self.plan, s);
                    self.warm(&fp, w, h, q, true);
                }
                for s in std::iter::once(n).chain(starts) {
                    self.warm_back(s, w, h, q);
                }
                self.warm_prerender(n, (1.5 * fps) as i64, w, h, q);
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

    /// (hits, lookups) of the source, composite and disk caches so far.
    fn lookups(&self) -> (u64, u64) {
        let (s, c) = (&self.sources, &self.composites);
        let (dh, dm) = self.disk.as_ref().map_or((0, 0), |d| (d.hits, d.misses));
        (s.hits + c.hits + dh, s.hits + s.misses + c.hits + c.misses + dh + dm)
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
        // Plans apply in order; of the rest only the newest matters (scrubbing queues many Shows). R23: a Build runs
        // first (it hands back a Show or Play that arrived while it ran), and Clear is never dropped.
        let (mut pending, mut build) = (None, None);
        for c in std::iter::once(first).chain(rx.try_iter()) {
            match c {
                VCmd::Plan(p) => wk.set_plan(p),
                VCmd::ClearRam => wk.clear_ram(),
                VCmd::Cancel => {}
                c @ VCmd::Build { .. } => build = Some(c),
                c => pending = Some(c),
            }
        }
        if let Some(VCmd::Build { a, b, w, h, q }) = build {
            if let Some(c) = wk.build(a, b, w, h, q, &rx) {
                pending = Some(c);
            }
            ctx.request_repaint();
        }
        match pending {
            Some(VCmd::Show { seq, n, w, h, q }) => {
                wk.pool.seeking = true;
                let (img, err) = wk.frame(n, w, h, q, None);
                wk.pool.seeking = false;
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
            disk: None,
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

    /// Where the bench media are cached: `MILAN_BENCH_DIR`, else `%TEMP%\milan-bench`.
    fn bench_dir() -> std::path::PathBuf {
        let dir = std::env::var_os("MILAN_BENCH_DIR").map_or_else(|| std::env::temp_dir().join("milan-bench"), std::path::PathBuf::from);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// R20: a 20 s 1080p 30 fps H.264 clip with one keyframe every 250 frames (no scene cuts), `testsrc2` with its frame
    /// number burnt into the top left as 10 black or white 64x64 blocks (bit i at x = 64 i); cached in `bench_dir`.
    /// With `proxy`, also its video proxy (`proxy::args`), made ready so Draft and Preview read it.
    fn numbered_project(reverse: bool, proxy: bool) -> Project {
        let path = bench_dir().join("numbered-20.mp4");
        if !path.exists() {
            let mut vf = String::from("drawbox=x=0:y=0:w=640:h=64:color=black:t=fill");
            for i in 0..10 {
                vf += &format!(",drawbox=x={}:y=0:w=64:h=64:color=white:t=fill:enable='mod(floor(n/{}),2)'", 64 * i, 1 << i);
            }
            let ok = crate::proc::ffmpeg()
                .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=1920x1080:rate=30:duration=20", "-vf", &vf])
                .args(["-c:v", "libx264", "-preset", "veryfast", "-g", "250", "-sc_threshold", "0", "-pix_fmt", "yuv420p"])
                .arg(&path)
                .status()
                .unwrap()
                .success();
            assert!(ok);
        }
        let m = crate::media::probe_media(&path).unwrap();
        use_proxy(&m, proxy);
        let mut p = Project::new("numbered");
        p.settings.frame_rate = 30.0;
        let mut c = Clip::from_media(&m, 0.0);
        c.reverse = reverse;
        p.timeline.video_tracks[0].clips.push(c);
        p.media_pool.push(m);
        p
    }

    /// R20: read `m`'s video proxy at Draft and Preview (made beside it with `proxy::args` the first time), or not.
    fn use_proxy(m: &crate::project::MediaItem, on: bool) {
        if !on {
            crate::proxy::forget(&m.path);
            return;
        }
        let out = crate::proxy::proxy_path(&m.path);
        if !out.exists() {
            let partial = out.with_extension("partial.mp4");
            let a = crate::proxy::args(m, &partial);
            // Without the progress pipe, which nobody reads here.
            let a: Vec<&String> = a.iter().filter(|x| !matches!(x.as_str(), "-progress" | "pipe:1")).collect();
            assert!(crate::proc::ffmpeg().args(a).status().unwrap().success());
            std::fs::rename(&partial, &out).unwrap();
        }
        crate::proxy::set_ready(m.path.clone(), out);
    }

    /// The frame number burnt into a picture of `numbered_project` (any 16:9 size).
    fn frame_number(img: &ColorImage) -> i64 {
        let s = img.size[0] as f64 / 1920.0;
        let y = (32.0 * s) as usize;
        (0..10).map(|i| (((64 * i + 32) as f64 * s) as usize, i)).map(|(x, i)| ((img.pixels[y * img.size[0] + x].g() > 128) as i64) << i).sum()
    }

    /// Needs ffmpeg. R20: 200 random seeks on long-GOP media (and on its proxy) each show exactly the frame asked for;
    /// prints their latency. `cargo test --release -- --ignored --nocapture seeks_are_frame_exact_on_long_gop`
    #[test]
    #[ignore]
    fn seeks_are_frame_exact_on_long_gop() {
        for (proxy, q) in [(false, Quality::Good), (true, Quality::Preview)] {
            let mut wk = Worker::new();
            wk.set_plan(Arc::new(plan::compile(&numbered_project(false, proxy))));
            wk.pool.seeking = true; // as the Video Preview's Shows are
            let (mut x, mut times) = (12345u64, Vec::new());
            for _ in 0..200 {
                x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let n = (x >> 33) as i64 % 600;
                let t = Instant::now();
                let (img, err) = wk.frame(n, 960, 540, q, None);
                times.push(t.elapsed().as_secs_f64() * 1000.0);
                assert!(err.is_none(), "{err:?}");
                assert_eq!(frame_number(&img), n, "proxy {proxy}: seek to {n}");
            }
            let [p50, p95, max] = percentiles(&mut times);
            println!("random seeks, proxy {proxy}: p50 {p50:.1} ms, p95 {p95:.1} ms, max {max:.1} ms");
            println!("BENCH seek proxy={proxy} p50_ms={p50:.1} p95_ms={p95:.1} max_ms={max:.1}");
        }
    }

    /// Needs ffmpeg. R20: a reversed 1080p event with one keyframe every 250 frames, played at 1x through the worker
    /// against a 30 fps clock (as `play_project`): the share of frames due that went on screen showing exactly their
    /// source frame. `cargo test --release -- --ignored --nocapture reverse_playback_shows_every_frame`
    #[test]
    #[ignore]
    fn reverse_playback_shows_every_frame() {
        let p = numbered_project(true, false);
        let mut wk = Worker::new();
        wk.set_plan(Arc::new(plan::compile(&p)));
        let (tx, frames) = mpsc::sync_channel(4);
        let (_cmd_tx, cmd_rx) = mpsc::channel();
        let (clock, stats) = (Arc::new(AtomicI64::new(0)), Arc::new(Stats::default()));
        let (c2, s2) = (clock.clone(), stats.clone());
        let player = std::thread::spawn(move || wk.play(0, 960, 540, Quality::Preview, None, &tx, &cmd_rx, &c2, &s2));
        let first = frames.recv().unwrap();
        assert_eq!(frame_number(&first.1), 599);
        // 10 s of the 20 s event; frame u of the timeline is source frame 599 - u.
        let (start, total) = (Instant::now(), 300i64);
        let (mut exact, mut ahead, mut on_screen) = (1, None::<crate::engine::Played>, 0i64);
        while on_screen < total {
            let now = (start.elapsed().as_secs_f64() * 30.0) as i64;
            clock.store(now, Relaxed);
            // Like the Video Preview: of the frames due, the newest shows.
            let mut due = None;
            loop {
                ahead = ahead.or_else(|| frames.try_recv().ok());
                match &ahead {
                    Some(f) if f.0 <= now => due = ahead.take(),
                    _ => break,
                }
            }
            if let Some((u, img, _)) = due {
                exact += (frame_number(&img) == 599 - u) as i64;
                on_screen = u;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        drop(frames);
        let _ = player.join();
        let pct = 100.0 * exact as f64 / (total + 1) as f64;
        println!("reverse_playback_shows_every_frame: {exact} of {} frames exact ({pct:.1} %)", total + 1);
        println!("BENCH reverse frames={} exact={exact} pct={pct:.1}", total + 1);
        assert!(pct >= 95.0, "{pct:.1} %");
        // A render waits for every frame: each is exact.
        let mut wk = Worker::for_render();
        wk.set_plan(Arc::new(plan::compile(&p)));
        for u in 0..90 {
            assert_eq!(frame_number(&wk.frame(u, 960, 540, Quality::Good, None).0), 599 - u, "rendered frame {u}");
        }
    }

    /// Needs ffmpeg. R20: dragging the cursor at 2x over the Video Preview (the app's path: `VideoEngine` at 60 Hz,
    /// Shows coalesced by the worker), backwards and forwards, on the source and on its proxy: how far the picture on
    /// screen trails the cursor (the scrub "jump"), p95 and max in seconds of timeline.
    /// `cargo test --release -- --ignored --nocapture scrub_drag_follows_the_cursor`
    #[test]
    #[ignore]
    fn scrub_drag_follows_the_cursor() {
        for proxy in [false, true] {
            let mut p = numbered_project(false, proxy);
            for (name, from, to) in [("back", 12.0, 6.0), ("forward", 6.0, 12.0)] {
                let mut e = crate::engine::VideoEngine::new(&egui::Context::default());
                p.timeline.cursor_position = from;
                let until = Instant::now() + Duration::from_secs(10);
                while e.update(&p, 960, 540, Quality::Preview, true).is_none() {
                    assert!(Instant::now() < until, "timed out");
                    std::thread::sleep(Duration::from_millis(2));
                }
                let (start, secs) = (Instant::now(), f64::abs(to - from) / 2.0);
                let (mut lag, mut shown) = (Vec::new(), (from * 30.0) as i64);
                loop {
                    let t = start.elapsed().as_secs_f64().min(secs);
                    p.timeline.cursor_position = from + (to - from) * t / secs;
                    if let Some((img, err)) = e.update(&p, 960, 540, Quality::Preview, true) {
                        assert!(err.is_none(), "{err:?}");
                        shown = frame_number(&img);
                    }
                    lag.push((p.timeline.cursor_position * 30.0 - shown as f64).abs() / 30.0);
                    if t >= secs {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(16));
                }
                let [_, p95, max] = percentiles(&mut lag);
                println!("scrub drag {name} at 2x, proxy {proxy}: picture behind the cursor p95 {p95:.2} s, max {max:.2} s");
                println!("BENCH drag dir={name} proxy={proxy} lag_p95_s={p95:.2} lag_max_s={max:.2}");
            }
        }
    }

    /// `n` 1080p H.264 clips of `secs` seconds (cached between runs in `MILAN_BENCH_DIR`, else `%TEMP%\milan-bench`), on
    /// `n` tracks at Level 0.5 so every layer blends. `tools/bench.ps1` runs the benches below and writes PERFORMANCE.md.
    pub(crate) fn bench_project(n: usize, secs: u32) -> Project {
        let dir = bench_dir();
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
        play_worker(wk)
    }

    /// `wk` (its plan set) played at Preview (960×540) against a 29.97 fps clock.
    fn play_worker(mut wk: Worker) -> Played {
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
        // Forward steps, then jumps: a mix of live-stream reads, respawns and cache hits. R20: then 60 steps back from
        // frame 300 (dragging left, J at -1x; the first, a seek, is not counted), on the source and on its proxy.
        let seq: Vec<i64> = (0..30).chain([300, 100, 250, 20, 301, 302, 303, 0, 1, 2]).collect();
        let back: Vec<i64> = (240..=300).rev().collect();
        for proxy in [false, true] {
            use_proxy(&p.media_pool[0], proxy);
            for (name, seq, skip) in [("scrub", &seq, 0), ("scrub_back", &back, 1)] {
                let mut wk = Worker::new();
                wk.set_plan(Arc::new(plan::compile(&p)));
                wk.pool.seeking = true; // as the Video Preview's Shows are
                let mut times = Vec::new();
                for &n in seq {
                    let t = Instant::now();
                    let (_, err) = wk.frame(n, 960, 540, Quality::Preview, None);
                    assert!(err.is_none(), "{err:?}");
                    times.push(t.elapsed().as_secs_f64() * 1000.0);
                }
                let [p50, p95, max] = percentiles(&mut times[skip..]);
                println!("bench_scrub {name}, proxy {proxy}: p50 {p50:.1} ms, p95 {p95:.1} ms, max {max:.1} ms");
                println!("BENCH {name}{} p50_ms={p50:.1} p95_ms={p95:.1} max_ms={max:.1}", if proxy { "_proxy" } else { "" });
            }
        }
        use_proxy(&p.media_pool[0], false);
    }

    /// R23, no ffmpeg: a RAM preview of two generated events builds frame by frame; a change to the second event
    /// drops only its frames (the first event's stay, as the ruler's bar shows); frames come back from it whatever
    /// the deadline; the cap stops a build; Clear empties it.
    #[test]
    fn ram_preview_keeps_the_frames_an_edit_did_not_touch() {
        use crate::engine::plugins::{find, new_fx};
        let mut p = generated(red());
        p.settings.frame_rate = 30.0;
        let mut c = p.timeline.video_tracks[0].clips[0].clone();
        (c.id, c.position, c.duration) = ("b".into(), 2.0, 2.0);
        p.timeline.video_tracks[0].clips[0].duration = 2.0;
        p.timeline.video_tracks[0].clips.push(c);
        let (_tx, rx) = mpsc::channel();
        let mut wk = Worker::new();
        wk.set_plan(Arc::new(plan::compile(&p)));
        assert!(wk.build(0, 120, 64, 36, Quality::Good, &rx).is_none());
        assert_eq!(wk.ram_frames(), (0..120).collect::<Vec<i64>>());
        let bars = crate::engine::preview_bars();
        assert_eq!(bars.ram, [(0.0, 4.0)]);
        let st = crate::engine::ram_status();
        assert!(st.building.is_none() && st.frames == 120 && !st.full && st.bytes == 120 * 64 * 36 * 4, "{st:?}");
        // Playing or paused, a frame inside comes from the RAM preview itself (the same picture, not a copy).
        let held = wk.frame(30, 64, 36, Quality::Good, Some(Instant::now())).0;
        assert!(Arc::ptr_eq(&held, &wk.frame(30, 64, 36, Quality::Good, None).0));
        // Another size or quality is not it.
        assert!(!Arc::ptr_eq(&held, &wk.frame(30, 64, 36, Quality::Best, None).0));
        // An edit on the second event: its 60 frames go, the first event's 60 stay.
        p.timeline.video_tracks[0].clips[1].fx.push(new_fx(find("milan.fx.invert").unwrap(), None));
        wk.set_plan(Arc::new(plan::compile(&p)));
        assert_eq!(wk.ram_frames(), (0..60).collect::<Vec<i64>>());
        assert_eq!(crate::engine::preview_bars().ram, [(0.0, 2.0)]);
        assert!(Arc::ptr_eq(&held, &wk.frame(30, 64, 36, Quality::Good, None).0), "an untouched frame is still the built one");
        // The cap: a build stops when the next frame would pass it, and says so.
        crate::engine::caches::set_ram(10 * 64 * 36 * 4);
        assert!(wk.build(0, 120, 64, 36, Quality::Good, &rx).is_none());
        assert_eq!(wk.ram_frames().len(), 10);
        assert!(crate::engine::ram_status().full);
        assert!(crate::engine::ram_status_text().is_some_and(|t| t.contains("limit")));
        crate::engine::caches::set_ram(200 << 20);
        // Cancel mid-build keeps what was built; Clear empties it.
        let (tx, rx) = mpsc::channel();
        tx.send(VCmd::Cancel).unwrap();
        assert!(wk.build(0, 120, 64, 36, Quality::Good, &rx).is_none());
        assert!(wk.ram_frames().is_empty());
        assert!(wk.build(0, 30, 64, 36, Quality::Good, &rx).is_none());
        assert_eq!(wk.ram_frames().len(), 30);
        wk.clear_ram();
        assert!(wk.ram_frames().is_empty() && crate::engine::preview_bars().ram.is_empty());
    }

    /// R23, no ffmpeg: which frames Selectively Prerender Video renders: those that are more than one plain event.
    #[test]
    fn prerender_sections_are_the_frames_with_work() {
        use crate::engine::plugins::{find, new_fx};
        let mut p = generated(red());
        p.settings.frame_rate = 30.0;
        let mut c = p.timeline.video_tracks[0].clips[0].clone();
        (c.id, c.position, c.duration) = ("b".into(), 2.0, 2.0);
        p.timeline.video_tracks[0].clips[0].duration = 2.0;
        p.timeline.video_tracks[0].clips.push(c);
        // Generated media always needs the compositor (there is no file to play).
        let vp = plan::compile(&p);
        assert_eq!(plan::work_sections(&vp, 0, 150), [(0, 120)]);
        // A file-backed event: plain unless something is done to it.
        let m = crate::project::MediaItem { id: "v".into(), path: "v.mp4".into(), media_type: crate::project::MediaType::Video, duration: 10.0, width: Some(64), height: Some(36), frame_rate: Some(30.0), ..Default::default() };
        let mut q = Project::new("v");
        q.settings.frame_rate = 30.0;
        let mut a = Clip::from_media(&m, 0.0);
        a.duration = 2.0;
        let mut b = Clip::from_media(&m, 2.0);
        (b.id, b.duration) = ("b".into(), 2.0);
        b.fx.push(new_fx(find("milan.fx.invert").unwrap(), None));
        q.timeline.video_tracks[0].clips = vec![a, b];
        q.media_pool.push(m);
        let vq = plan::compile(&q);
        assert_eq!(plan::work_sections(&vq, 0, 150), [(60, 120)]);
        let h = plan::range_hash(&vq, 60, 120);
        q.timeline.video_tracks[0].clips[0].fade_in = 0.5;
        let vq2 = plan::compile(&q);
        assert_eq!(plan::work_sections(&vq2, 0, 150), [(0, 15), (60, 120)]);
        assert_eq!(plan::range_hash(&vq2, 60, 120), h, "an edit outside the section keeps its hash");
        q.timeline.video_tracks[0].clips[1].fx[0].enabled = false;
        assert_ne!(plan::range_hash(&plan::compile(&q), 60, 120), h, "an edit inside changes it");
    }

    /// Needs ffmpeg. R23: a RAM preview of 2 s of the bench clip plays with no decoder at all.
    #[test]
    #[ignore]
    fn ram_preview_plays_with_no_decoder() {
        let p = bench_project(1, 12);
        let (_tx, rx) = mpsc::channel();
        let mut wk = Worker::new();
        wk.set_plan(Arc::new(plan::compile(&p)));
        assert!(wk.build(0, 60, 320, 180, Quality::Preview, &rx).is_none());
        assert_eq!(wk.ram_frames().len(), 60);
        wk.pool = DecoderPool::default();
        wk.pool.set_rate(wk.plan.fps, &wk.plan.fps_str);
        for n in 0..60 {
            let (img, err) = wk.frame(n, 320, 180, Quality::Preview, Some(Instant::now() + Duration::from_millis(1)));
            assert!(err.is_none() && img.size == [320, 180]);
        }
        assert_eq!(wk.pool.live(), 0, "playback inside the RAM preview opened no decoder");
    }

    /// Needs ffmpeg. R23: the section of a project with work (an inverted event) prerenders to a file; playback and a
    /// render take the section's frames from it (the source file is renamed away: the frames inside still come, those
    /// outside fail), an edit inside the section makes it inactive, and Clean Up deletes only the project's files.
    #[test]
    #[ignore]
    fn prerendered_sections_serve_playback_and_render_until_edited() {
        use crate::engine::plugins::{find, new_fx};
        use crate::engine::prerender;
        let dir = std::env::temp_dir().join("milan-agent").join(format!("prerender-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("src.mp4");
        assert!(crate::proc::ffmpeg().args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=320x180:rate=30:duration=4", "-pix_fmt", "yuv420p", "-g", "15"]).arg(&src).status().unwrap().success());
        let m = crate::media::probe_media(&src).unwrap();
        let mut p = Project::new("pre");
        (p.settings.width, p.settings.height, p.settings.frame_rate) = (320, 180, 30.0);
        let mut a = Clip::from_media(&m, 0.0);
        a.duration = 2.0;
        let mut b = Clip::from_media(&m, 2.0);
        (b.id, b.in_point, b.duration) = ("b".into(), 2.0, 2.0);
        b.fx.push(new_fx(find("milan.fx.invert").unwrap(), None));
        p.timeline.video_tracks[0].clips = vec![a, b];
        p.media_pool.push(m);
        let folder = dir.join("pre-folder");
        prerender::sync_to(folder.clone());
        let vp = prerender::canonical(&p);
        assert_eq!(plan::work_sections(&vp, 0, 120), [(60, 120)]);
        // Render the section as the dialog does.
        let t = crate::render::Template { audio: None, ..crate::render::BUILTIN.iter().find(|t| t.name == prerender::DEFAULT_TEMPLATE).unwrap().clone() };
        let hash = plan::range_hash(&vp, 60, 120);
        let file = folder.join(format!("{hash:016x}.{}", t.ext()));
        std::fs::create_dir_all(&folder).unwrap();
        let job = crate::engine::render::start(&p, &t, file.clone(), (2.0, 4.0), crate::render::Opts { stretch: false, rotate: false, markers: false, loudness_log: None, quickfade: false, multichannel: false }, egui::Context::default());
        while job.status() == crate::render::Status::Running {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(job.status(), crate::render::Status::Done);
        prerender::add(prerender::Entry { file: file.clone(), a: 60, b: 120, fps: vp.fps, fps_str: vp.fps_str.clone(), w: 320, h: 180, hash, template: t.name.clone(), codec: "prores".into(), made: String::new() });
        // Reference frames, composed from the source.
        let mut reference = Worker::new();
        reference.set_plan(Arc::new(plan::compile(&p)));
        let want: Vec<Arc<ColorImage>> = [30i64, 75, 119].iter().map(|&n| reference.frame(n, 320, 180, Quality::Good, None).0.image()).collect();
        drop(reference);
        let gone = dir.join("gone.mp4");
        std::fs::rename(&src, &gone).unwrap();
        let close = |a: &ColorImage, b: &ColorImage| a.pixels.iter().zip(&b.pixels).map(|(x, y)| x.r().abs_diff(y.r()) as u64 + x.g().abs_diff(y.g()) as u64 + x.b().abs_diff(y.b()) as u64).sum::<u64>() as f64 / (3 * a.pixels.len()) as f64;
        let mut wk = Worker::new();
        wk.set_plan(Arc::new(plan::compile(&p)));
        assert_eq!(crate::engine::preview_bars().prerendered, [(2.0, 4.0)]);
        let (img, err) = wk.frame(75, 320, 180, Quality::Good, None);
        assert!(err.is_none(), "{err:?}");
        assert!(close(&img, &want[1]) < 3.0, "the prerendered frame is the composed one (ProRes round trip)");
        assert!(wk.frame(30, 320, 180, Quality::Good, None).1.is_some(), "outside the section the source is needed, and it is gone");
        // Playing through the section: every frame from the file.
        for n in 60..120 {
            let (img, err) = wk.frame(n, 320, 180, Quality::Good, Some(Instant::now() + Duration::from_millis(200)));
            assert!(err.is_none() && img.size == [320, 180], "frame {n}: {err:?}");
        }
        assert!(close(&wk.frame(119, 320, 180, Quality::Good, None).0, &want[2]) < 3.0);
        // A render takes the section from the file too.
        let mut rw = Worker::for_render();
        rw.set_plan(Arc::new(plan::compile(&p).without_proxies()));
        assert!(rw.frame(75, 320, 180, Quality::Good, None).1.is_none());
        // An edit inside the section: inactive, so the frames need the source again.
        p.timeline.video_tracks[0].clips[1].fx[0].enabled = false;
        wk.set_plan(Arc::new(plan::compile(&p)));
        assert!(crate::engine::preview_bars().prerendered.is_empty());
        assert!(wk.frame(75, 320, 180, Quality::Good, None).1.is_some());
        assert_eq!(prerender::inactive(&p).len(), 1);
        // Undo the edit: active again.
        p.timeline.video_tracks[0].clips[1].fx[0].enabled = true;
        wk.set_plan(Arc::new(plan::compile(&p)));
        assert_eq!(crate::engine::preview_bars().prerendered, [(2.0, 4.0)]);
        // Clean Up deletes the project's file and nothing else in the folder.
        std::fs::write(folder.join("other.txt"), "x").unwrap();
        drop(wk);
        drop(rw);
        assert_eq!(prerender::remove(&[file.clone()]), 1);
        assert!(!file.exists() && folder.join("other.txt").exists() && prerender::entries().is_empty());
        prerender::sync_to(dir.join("none"));
        std::fs::rename(&gone, &src).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Needs ffmpeg; CPU heavy. R23: Build Dynamic RAM Preview over 8 s of the 4-layer bench project at Preview, then
    /// its playback against the 29.97 fps clock with no decoder running. `cargo test --release -- --ignored --nocapture bench_ram_preview`
    #[test]
    #[ignore]
    fn bench_ram_preview() {
        let p = bench_project(4, 12);
        let (_tx, rx) = mpsc::channel();
        let mut wk = Worker::new();
        wk.set_plan(Arc::new(plan::compile(&p)));
        // 240 frames of 960x540 are 500 MB: over VEGAS's default cap of 200 MB, raised for the measurement.
        crate::engine::caches::set_ram(1 << 30);
        let t = Instant::now();
        assert!(wk.build(0, 240, 960, 540, Quality::Preview, &rx).is_none());
        crate::engine::caches::set_ram(200 << 20);
        let build_s = t.elapsed().as_secs_f64();
        let frames = wk.ram_frames().len();
        let mb = crate::engine::ram_status().bytes >> 20;
        wk.pool = DecoderPool::default();
        wk.pool.set_rate(wk.plan.fps, &wk.plan.fps_str);
        let r = play_worker(wk);
        println!("bench_ram_preview: built {frames} frames ({mb} MB) in {build_s:.2} s ({:.1} frames/s); playback {r}", frames as f64 / build_s);
        println!("BENCH ram frames={frames} mb={mb} build_s={build_s:.2} build_fps={:.1} {}", frames as f64 / build_s, r.fields());
        assert_eq!(frames, 240);
        assert!(r.fps >= 29.5 && r.drop_pct() < 1.0, "{r}");
    }

    /// Needs ffmpeg. R23: paused seeks into a 1080p HEVC clip (GOP 250, 10 s, made with libx265) at 960x540 Good: cold,
    /// with the disk frame cache filling, and again from the cache. `cargo test --release -- --ignored --nocapture bench_disk_cache`
    #[test]
    #[ignore]
    fn bench_disk_cache() {
        let path = bench_dir().join("hevc-10.mp4");
        if !path.exists() {
            let ok = crate::proc::ffmpeg()
                .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=1920x1080:rate=30:duration=10", "-c:v", "libx265", "-preset", "ultrafast", "-g", "250", "-pix_fmt", "yuv420p", "-tag:v", "hvc1"])
                .arg(&path)
                .status()
                .unwrap()
                .success();
            assert!(ok);
        }
        let m = crate::media::probe_media(&path).unwrap();
        assert_eq!(m.codec.as_deref(), Some("hevc"));
        let mut p = Project::new("hevc");
        p.settings.frame_rate = 30.0;
        p.timeline.video_tracks[0].clips.push(Clip::from_media(&m, 0.0));
        p.media_pool.push(m);
        let dir = std::env::temp_dir().join("milan-agent").join(format!("diskcache-bench-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let seeks: Vec<i64> = (0..30).map(|k| (k * 97 + 13) % 300).collect();
        let run = |name: &str| {
            let mut wk = Worker::new();
            wk.set_plan(Arc::new(plan::compile(&p)));
            wk.pool.seeking = true;
            let mut times = Vec::new();
            for &n in &seeks {
                let t = Instant::now();
                let (_, err) = wk.frame(n, 960, 540, Quality::Good, None);
                assert!(err.is_none(), "{err:?}");
                times.push(t.elapsed().as_secs_f64() * 1000.0);
            }
            let [p50, p95, max] = percentiles(&mut times);
            println!("bench_disk_cache {name}: p50 {p50:.1} ms, p95 {p95:.1} ms, max {max:.1} ms");
            println!("BENCH diskcache pass={name} p50_ms={p50:.1} p95_ms={p95:.1} max_ms={max:.1}");
            (wk, p50)
        };
        crate::engine::caches::set_disk(Some(dir.clone()), 0);
        let (_, cold) = run("off");
        crate::engine::caches::set_disk(Some(dir.clone()), 2 << 30);
        let (wk, _) = run("filling");
        drop(wk);
        let (wk, warm) = run("cached");
        drop(wk);
        crate::engine::caches::set_disk(None, 0);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(warm < cold, "cached seeks ({warm:.1} ms) are faster than decoding ({cold:.1} ms)");
    }
}
