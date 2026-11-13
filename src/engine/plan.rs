//! Timeline -> video and audio plans (pure). The only code that interprets the timeline for preview and playback:
//! `compile` once per edit, `frame_plan` once per frame, resolving everything to plain numbers;
//! `compile_audio` once per edit for the mixer.
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use crate::decode::Quality;
use crate::edit::src_time;
use crate::engine::composite::Affine;
use crate::engine::{generate, lut};
use crate::engine::plugins::{self, FxEval, PluginKind};
use crate::engine::raster::MaskPlan;
use crate::project::{BlurType, ChannelMap, Clip, Composite, Curve, EnvKind, Envelope, Fx, Halo, Keyframes, MaskMode, MediaItem, MediaType, Motion, PanCrop, Project, Resample};

/// A picture source as the decoders need it.
#[derive(Debug, Clone)]
pub struct MediaRef {
    pub id: Arc<str>,
    pub path: PathBuf,
    /// Display size (already swapped for 90/270 rotation; ffmpeg autorotates).
    pub w: u32,
    pub h: u32,
    pub duration: f64,
    /// Source frame rate, 0 if unknown.
    pub fps: f64,
    pub still: bool,
    pub has_alpha: bool,
    /// ffmpeg `in_color_matrix`, None = auto.
    pub matrix: Option<&'static str>,
    pub codec: Option<String>,
    pub interlaced: bool,
    /// A ready video proxy (`proxy::ready`), decoded instead of `path` at Draft and Preview quality.
    pub proxy: Option<PathBuf>,
    /// The frame rate `SrcAt::sf` counts in: 0 for the project's; the source's own when a higher-rate source plays
    /// slower or faster, so slow motion reaches every source frame.
    pub grid: f64,
    /// Where a subclip starts in its file (0 for a whole file): `sf` counts from the file's start.
    pub offset: f64,
    /// The file's video stream decoded (`-map 0:v:N`, Stream ▸).
    pub stream: u32,
    /// Optical Flow: frames are motion-interpolated onto the grid (ffmpeg minterpolate) instead of repeated.
    pub flow: bool,
}

impl MediaRef {
    /// The file `q` decodes instead of the source: the proxy at Draft and Preview; Good, Best and render never use it.
    pub fn proxy_for(&self, q: Quality) -> Option<&Path> {
        self.proxy.as_deref().filter(|_| matches!(q, Quality::Draft | Quality::Preview))
    }
}

#[derive(Debug)]
pub struct VideoPlan {
    pub w: u32,
    pub h: u32,
    pub fps: f64,
    pub fps_str: String,
    /// Bottom track first.
    pub tracks: Vec<VTrack>,
    /// Video Output FX.
    out_fx: Vec<Fx>,
    /// The Video Bus Track's envelopes and Fade Colors; Project Properties › Motion blur type.
    bus_envs: Vec<Envelope>,
    bus_colors: [[u8; 3]; 2],
    blur_type: BlurType,
    /// Content end (seconds).
    pub end: f64,
}

#[derive(Debug)]
pub struct VTrack {
    composite: Composite,
    compositor: Option<Fx>,
    /// A compositing child (composited into the parent above), and a parent (the track below is its child) with its
    /// Parent Composite Mode and Parent Motion.
    child: bool,
    parent: Option<(Composite, Keyframes<Motion>)>,
    shadow: Option<Keyframes<Halo>>,
    glow: Option<Keyframes<Halo>>,
    bypass_blur: bool,
    level: f32,
    level_env: Option<Envelope>,
    /// Track Mute envelope (1 shown, 0 hidden) and Fade to Color envelope with the [top, bottom] Fade Colors.
    mute_env: Option<Envelope>,
    fade_env: Option<Envelope>,
    fade_colors: [[u8; 3]; 2],
    motion: Keyframes<Motion>,
    fx: Vec<Fx>,
    /// Stable-sorted by position: on ties the later Vec entry is on top.
    events: Vec<VEvent>,
}

#[derive(Debug)]
struct VEvent {
    c: Clip,
    m: Arc<MediaRef>,
    /// The media's generator (generated media only).
    generator: Option<Fx>,
    /// The media length the event repeats (`edit::loop_len`).
    looped: Option<f64>,
    /// Resampled: the two source frames around each frame's time are blended (`m.grid` is the source's rate).
    blend: bool,
}

/// One frame, resolved: tracks bottom to top.
#[derive(Debug)]
pub struct FramePlan {
    pub layers: Vec<TrackPlan>,
    pub out_fx: Vec<FxEval>,
    /// The Video Bus Track's Fade to Color, after the Video Output FX.
    pub fade: Option<([u8; 3], f32)>,
    /// Motion blur and supersampling: the frame is the weighted mean of the frame planned at these time offsets
    /// (`frame_plan_at`); empty = this plan alone.
    pub samples: Vec<(f64, f32)>,
}

#[derive(Debug)]
pub struct TrackPlan {
    pub mode: Composite,
    pub opacity: f32,
    /// Track Motion in normalised frame coordinates.
    pub motion: Affine,
    /// 3D Source Alpha with depth or X / Y rotation: the track's frame (0..1) to the output frame (0..1), projective
    /// (row-major 3x3). It replaces `motion` for drawing.
    pub persp: Option<[f64; 9]>,
    /// Track FX, applied before Track Motion.
    pub fx: Vec<FxEval>,
    /// Fade to Color: the track's picture (after its FX and Track Motion) goes this far (0..=1) to this colour.
    pub fade: Option<([u8; 3], f32)>,
    /// The Custom mode's compositor.
    pub compositor: Option<FxEval>,
    /// Composited into the group of the parent above it, not onto the tracks below.
    pub child: bool,
    /// A compositing parent: its children are composited first, then this track over them with `mode`; the group
    /// then goes through Parent Motion onto the tracks below with the Parent Composite Mode.
    pub parent: Option<(Composite, Affine)>,
    /// Track Motion 2D Shadow and 2D Glow.
    pub shadow: Option<Halo>,
    pub glow: Option<Halo>,
    pub items: Vec<Item>,
}

#[derive(Debug)]
pub enum Item {
    Event(EventPlan),
    /// A transition from `a` into `b` at progress `p` (None = transparent: a lone fade-in, or a fade-out to nothing);
    /// `kind` None (an unknown plug-in) dissolves.
    Mix { a: Option<EventPlan>, b: Option<EventPlan>, p: f32, kind: Option<FxEval> },
}

#[derive(Debug)]
pub struct EventPlan {
    pub src: SrcAt,
    /// Normalised source (0..1) to normalised frame (0..1): Pan/Crop, then fit to the project frame.
    pub place: Affine,
    pub alpha: f32,
    /// Event FX, applied to the source before Pan/Crop.
    pub fx: Vec<FxEval>,
    /// Resampling: the next source frame and its weight, blended into `src` before the FX.
    pub next: Option<(i64, f32)>,
    /// Switches › Reduce Interlace Flicker: a vertical 1-2-1 blur of the source, before the FX.
    pub flicker: bool,
    /// Pan/Crop masks, applied to the source after its FX.
    pub masks: Vec<MaskPlan>,
}

#[derive(Debug)]
pub struct SrcAt {
    pub m: Arc<MediaRef>,
    /// Source frame on the project-fps grid (or `m.grid`); None for a still.
    pub sf: Option<i64>,
    /// Generated media: the generator and its time (0 unless it animates, so a static one renders once).
    pub generator: Option<(FxEval, f64)>,
}

impl FramePlan {
    /// Identity of the composited picture: equal keys give equal frames.
    // shortcut: hashes the Debug text (shortest round-trip floats, so every value counts); a few µs per frame.
    pub fn key(&self) -> u64 {
        let mut h = DefaultHasher::new();
        format!("{self:?}").hash(&mut h);
        // A LUT re-exported or edited in place changes the picture but not the plan.
        for p in self.fx().into_iter().flat_map(lut::paths) {
            std::fs::metadata(p).and_then(|m| m.modified()).ok().hash(&mut h);
        }
        h.finish()
    }

    /// Every effect this frame runs: Video Output, track and event FX.
    pub fn fx(&self) -> Vec<&FxEval> {
        let mut v: Vec<&FxEval> = self.out_fx.iter().collect();
        for l in &self.layers {
            v.extend(&l.fx);
            v.extend(&l.compositor);
            for i in &l.items {
                match i {
                    Item::Event(e) => v.extend(&e.fx),
                    Item::Mix { a, b, .. } => v.extend(a.iter().chain(b).flat_map(|e| &e.fx)),
                }
            }
        }
        v
    }
}

/// ffmpeg colour matrix for a probed `color_space`; untagged HD video is bt709 (images keep ffmpeg's choice: JPEG is BT.601).
/// Shared with the Trimmer and thumbnails so a clip looks the same everywhere.
pub fn matrix(m: &MediaItem) -> Option<&'static str> {
    match m.color_matrix.as_deref() {
        Some("bt709") => Some("bt709"),
        Some("bt470bg") => Some("bt470"),
        Some("smpte170m") => Some("smpte170m"),
        Some("smpte240m") => Some("smpte240m"),
        Some("fcc") => Some("fcc"),
        Some("bt2020nc" | "bt2020c") => Some("bt2020"),
        _ if m.media_type == MediaType::Video && m.height.unwrap_or(1080) >= 720 => Some("bt709"),
        _ => None,
    }
}

/// `project` is the project frame size, which generated media render at.
fn media_ref(m: &MediaItem, project: (u32, u32)) -> Option<MediaRef> {
    let generator = m.generator.as_ref();
    match generator {
        // A generator this build does not know draws nothing.
        Some(g) if plugins::find(&g.plugin).is_none_or(|i| i.kind != PluginKind::Generator) => return None,
        None if !matches!(m.media_type, MediaType::Video | MediaType::Image) => return None,
        _ => {}
    }
    let (w, h) = match (m.width, m.height) {
        _ if generator.is_some() => project,
        (Some(w), Some(h)) if w > 0 && h > 0 => (w, h),
        _ => (1920, 1080),
    };
    Some(MediaRef {
        id: m.id.as_str().into(),
        path: m.path.clone(),
        w,
        h,
        duration: m.duration,
        fps: m.frame_rate.unwrap_or(0.0),
        still: m.media_type == MediaType::Image || generator.is_some(),
        has_alpha: m.has_alpha || generator.is_some(),
        matrix: matrix(m),
        codec: m.codec.clone(),
        interlaced: m.interlaced,
        proxy: crate::proxy::is_video(m).then(|| crate::proxy::ready(&m.path)).flatten(),
        grid: 0.0,
        offset: m.file_time(0.0),
        stream: 0,
        flow: false,
    })
}

/// How event `c` shows source `m` in a project at `fps`: the source variant to decode (its grid, stream and Optical
/// Flow) and whether neighbouring source frames are blended (Switches › resample radios, Project Properties › Resample
/// mode). Smart (and Optical Flow) act when the source rate differs from the project's or the event plays off speed.
fn event_source(m: &Arc<MediaRef>, item: &MediaItem, c: &Clip, mode: Resample, fps: f64) -> (Arc<MediaRef>, bool) {
    let off_speed = (c.rate - 1.0).abs() > 1e-9 || crate::engine::velocity::of(c).is_some();
    let moving = !m.still && m.fps > 0.0;
    let off_rate = moving && ((m.fps - fps).abs() > fps * 1e-3 || off_speed);
    let flow = mode == Resample::OpticalFlow && off_rate;
    // A 60p source at normal speed in a 30p project lands on whole source frames: there is nothing between to blend.
    let ratio = m.fps / fps;
    let aligned = !off_speed && ratio >= 1.0 - 1e-6 && (ratio - ratio.round()).abs() < 1e-6;
    let blend = !flow && moving && !aligned && match mode {
        Resample::Force => true,
        Resample::Smart => off_rate,
        _ => false,
    };
    // Off normal speed, a source with more frames than the project is read on its own frame grid, as is a blended one;
    // Optical Flow makes the frames slow motion shows (the project rate over the playback rate).
    let fine = moving && m.fps > fps * 1.001 && off_speed;
    let grid = if flow { if off_speed { fps / c.rate } else { 0.0 } } else if blend || fine { m.fps } else { 0.0 };
    // A stream or Optical Flow is another picture of the file: its own id keeps its decoders and cached frames apart.
    let stream = if m.still { 0 } else { c.stream.min(item.streams(true) - 1) };
    let m = if grid == m.grid && stream == 0 && !flow {
        m.clone()
    } else {
        let id: Arc<str> = if stream > 0 || flow { format!("{}#{stream}{}", m.id, if flow { "f" } else { "" }).into() } else { m.id.clone() };
        // A proxy is made from the first video stream.
        let proxy = m.proxy.clone().filter(|_| stream == 0);
        Arc::new(MediaRef { id, grid, stream, flow, proxy, ..(**m).clone() })
    };
    (m, blend)
}

pub fn compile(p: &Project) -> VideoPlan {
    let s = &p.settings;
    // 30000/1001, not 29.97: frame n is at n × 1001 / 30000 s, on the same grid as the ruler and the mixer.
    let fps = crate::project::exact_fps(s.frame_rate.max(1.0));
    // Even: an odd size (an imported VEGAS project) cannot be encoded 4:2:0.
    let (w, h) = (s.width.max(2) & !1, s.height.max(2) & !1);
    let refs: HashMap<&str, (Arc<MediaRef>, &MediaItem)> =
        p.media_pool.iter().filter_map(|m| Some((m.id.as_str(), (Arc::new(media_ref(m, (w, h))?), m)))).collect();
    // Parent / child in the whole list (a muted parent leaves its children to composite on their own); the top track
    // cannot be a child.
    // shortcut: one level of compositing children (VEGAS nests further); add a parent stack if nested groups are asked for.
    let all = &p.timeline.video_tracks;
    let is_child = |i: usize| all[i].child && all[..i].iter().any(|t| !t.child);
    let is_parent = |i: usize| !is_child(i) && all.get(i + 1).is_some_and(|_| is_child(i + 1));
    let tracks = p
        .timeline
        .shown_video_tracks()
        .rev()
        .map(|t| {
            let i = all.iter().position(|x| x.id == t.id).unwrap_or(0);
            let mut events: Vec<VEvent> = t
                .clips
                .iter()
                .filter(|c| !c.muted && c.duration > 0.0)
                .filter_map(|c| {
                    let (m, item) = refs.get(c.media_id.as_deref()?)?;
                    let (m, blend) = event_source(m, item, c, c.resample.unwrap_or(s.resample), fps);
                    Some(VEvent { m, blend, generator: item.generator.clone(), looped: crate::edit::loop_len(c, item), c: c.clone() })
                })
                .collect();
            events.sort_by(|a, b| a.c.position.total_cmp(&b.c.position));
            let env = |kind| t.envelopes.iter().find(|e| e.kind == kind && !e.points.keys.is_empty()).cloned();
            let halo = |h: &crate::project::Halo2D| h.on.then(|| h.keys.clone());
            VTrack {
                composite: t.composite,
                compositor: t.compositor.clone().filter(|_| t.composite == Composite::Custom),
                child: is_child(i),
                parent: is_parent(i).then(|| (t.parent_mode, t.parent_motion.clone())),
                shadow: halo(&t.shadow),
                glow: halo(&t.glow),
                bypass_blur: t.bypass_blur,
                level: t.volume.clamp(0.0, 1.0),
                level_env: t.envelopes.iter().find(|e| e.kind == EnvKind::CompositeLevel).cloned(),
                mute_env: env(EnvKind::Mute),
                fade_env: env(EnvKind::FadeToColor),
                fade_colors: t.fade_colors,
                motion: t.motion.clone(),
                fx: t.fx.clone(),
                events,
            }
        })
        .collect();
    VideoPlan {
        w,
        h,
        fps,
        fps_str: crate::render::fps_str(fps),
        tracks,
        out_fx: p.timeline.master.video_fx.clone(),
        bus_envs: p.timeline.master.video_envelopes.iter().filter(|e| !e.points.keys.is_empty()).cloned().collect(),
        bus_colors: p.timeline.master.fade_colors,
        blur_type: s.motion_blur,
        end: crate::render::content_end(p),
    }
}

/// The enabled, known plug-ins of a chain at `t`.
fn evals(chain: &[Fx], t: f64) -> Vec<FxEval> {
    chain.iter().filter(|f| f.enabled).filter_map(|f| plugins::eval(f, t)).collect()
}

/// Frame shown at time `t` (the cursor's frame).
pub fn frame_at(t: f64, fps: f64) -> i64 {
    (t * fps + 1e-6).floor() as i64
}

/// Loop region in frames: [first frame, one past the last frame shown before the wrap).
pub fn loop_frames((a, b): (f64, f64), fps: f64) -> (i64, i64) {
    (frame_at(a, fps), (b * fps - 1e-6).ceil() as i64)
}

/// Where playback ends: the content end, or the loop end when looping past it.
pub fn effective_end(content_end: f64, loop_: Option<(f64, f64)>) -> f64 {
    loop_.map_or(content_end, |(_, b)| content_end.max(b))
}

/// Gain through a fade of `len` seconds, `x` seconds from its silent end.
pub fn fade(curve: Curve, x: f64, len: f64) -> f32 {
    if len > 0.0 { curve.shape(x / len) as f32 } else { 1.0 }
}

fn env_at(envs: &[Envelope], kind: EnvKind, t: f64) -> f32 {
    envs.iter().find(|e| e.kind == kind).map_or(1.0, |e| e.at(t).clamp(0.0, 1.0))
}

/// Pan/Crop: the crop rectangle (normalised to the source, negative size = flip, `rot` degrees) fitted into the frame;
/// with `keep_aspect` off (Switches › Maintain Aspect Ratio) stretched to fill it.
pub fn place(pc: PanCrop, (sw, sh): (f64, f64), (w, h): (f64, f64), keep_aspect: bool) -> Affine {
    let (cw, ch) = ((pc.w.abs() as f64 * sw).max(1e-6), (pc.h.abs() as f64 * sh).max(1e-6));
    let (sx, sy) = if keep_aspect { ((w / cw).min(h / ch), (w / cw).min(h / ch)) } else { (w / cw, h / ch) };
    Affine::scale(sw, sh)
        .then(Affine::translate(-pc.cx as f64 * sw, -pc.cy as f64 * sh))
        // The crop frame turns one way, so the picture turns the other.
        .then(Affine::rotate(-pc.rot as f64))
        .then(Affine::scale(sx * pc.w.signum() as f64, sy * pc.h.signum() as f64))
        .then(Affine::translate(w / 2.0, h / 2.0))
        .then(Affine::scale(1.0 / w, 1.0 / h))
}

/// Track Motion: size about the frame centre, rotate, then move the centre by (x, y) frame sizes.
pub fn motion(m: Motion, (w, h): (f64, f64)) -> Affine {
    Affine::scale(w, h)
        .then(Affine::translate(-w / 2.0, -h / 2.0))
        .then(Affine::scale(m.w as f64, m.h as f64))
        .then(Affine::rotate(m.rot as f64))
        .then(Affine::translate((0.5 + m.x as f64) * w, (0.5 + m.y as f64) * h))
        .then(Affine::scale(1.0 / w, 1.0 / h))
}

/// 3D Track Motion (3D Source Alpha): the track's frame (0..1) to the output frame (0..1) through a camera `w` (one
/// frame width) in front of the frame, as a row-major projective 3x3; None when there is no depth or X / Y rotation
/// (`motion` is exact then).
// shortcut: the camera distance is one frame width (VEGAS's is not documented); measure it against VEGAS if it matters.
pub fn motion3d(m: Motion, (w, h): (f64, f64)) -> Option<[f64; 9]> {
    if m.z == 0.0 && m.rx == 0.0 && m.ry == 0.0 {
        return None;
    }
    let d = w;
    let (sz, cz) = (m.rot as f64).to_radians().sin_cos();
    let (sy, cy) = (m.ry as f64).to_radians().sin_cos();
    let (sx, cx) = (m.rx as f64).to_radians().sin_cos();
    // Rx · Ry · Rz applied to a point in the track's plane (z = 0).
    let rot = |x: f64, y: f64| {
        let (x, y) = (cz * x - sz * y, sz * x + cz * y);
        let (x, z) = (cy * x, -sy * x);
        let (y, z) = (cx * y - sx * z, sx * y + cx * z);
        [x, y, z]
    };
    let (pw, ph) = (w * m.w as f64, h * m.h as f64);
    let (c0, c1) = (rot(pw, 0.0), rot(0.0, ph));
    let o = rot(-pw / 2.0, -ph / 2.0);
    let c2 = [o[0] + m.x as f64 * w, o[1] + m.y as f64 * h, o[2] - m.z as f64 * w];
    // Screen: (d X + w/2 (d + Z), d Y + h/2 (d + Z), d + Z), in output frames.
    let col = |c: [f64; 3], one: f64| [(d * c[0] + w / 2.0 * (c[2] + d * one)) / w, (d * c[1] + h / 2.0 * (c[2] + d * one)) / h, c[2] + d * one];
    let (a, b, c) = (col(c0, 0.0), col(c1, 0.0), col(c2, 1.0));
    Some([a[0], b[0], c[0], a[1], b[1], c[1], a[2], b[2], c[2]])
}

/// The motion blur and supersampling sub-frames at time `t`: (offset in seconds, weight), or none. Motion Blur Amount
/// is the blur's length in seconds, centred on the frame (trailing it for the asymmetric types); Video Supersampling
/// adds that many sub-frames per frame (alone, they spread over the frame's own time).
pub fn blur_samples(p: &VideoPlan, t: f64) -> Vec<(f64, f32)> {
    let env = |k| p.bus_envs.iter().find(|e| e.kind == k).map_or(0.0, |e| e.at(t));
    let (amount, ss) = (env(EnvKind::MotionBlur).clamp(0.0, 1.0) as f64, env(EnvKind::Supersampling).clamp(0.0, 8.0).round() as usize);
    let frame = 1.0 / p.fps;
    if amount * p.fps < 0.05 && ss == 0 {
        return Vec::new();
    }
    let (len, kind) = if amount * p.fps < 0.05 { (frame, BlurType::Box) } else { (amount, p.blur_type) };
    let n = ((len * p.fps - 1e-6).ceil() as usize * (ss + 1)).clamp(2, 32);
    let asym = matches!(kind, BlurType::AsymmetricGaussian | BlurType::AsymmetricPyramid | BlurType::AsymmetricBox);
    (0..n)
        .map(|i| {
            let u = (i as f64 + 0.5) / n as f64; // 0..1 across the window
            // x: distance from the frame's own time, 0..1 of the half window (of the whole one when asymmetric).
            let (dt, x) = if asym { (-len * u, u) } else { (len * (u - 0.5), (2.0 * u - 1.0).abs()) };
            let w = match kind {
                BlurType::Gaussian | BlurType::AsymmetricGaussian => (-0.5 * (x * 2.5).powi(2)).exp(),
                BlurType::Pyramid | BlurType::AsymmetricPyramid => 1.0 - x,
                BlurType::Box | BlurType::AsymmetricBox => 1.0,
            };
            (dt, w as f32)
        })
        .collect()
}

/// Frames `a..b` seconds cover, to render: [round(a), round(b)) on the exact grid.
pub fn frame_range((a, b): (f64, f64), fps: f64) -> (i64, i64) {
    let fps = crate::project::exact_fps(fps);
    ((a * fps).round() as i64, (b * fps).round() as i64)
}

/// Frame `n`, with its motion blur / supersampling sub-frames (`samples`) when the Video Bus Track asks for them.
pub fn frame_plan(p: &VideoPlan, n: i64) -> FramePlan {
    let mut fp = frame_plan_at(p, n, 0.0);
    fp.samples = blur_samples(p, crate::project::frame_time(n, p.fps));
    fp
}

/// Frame `n` with everything that animates (Track Motion, Pan/Crop, envelopes, FX, generated media) taken `dt`
/// seconds from the frame's time: the events and source frames stay the frame's own (VEGAS blurs motion made in the
/// project, not motion inside the footage). Tracks with Bypass Motion Blur ignore `dt`.
pub fn frame_plan_at(p: &VideoPlan, n: i64, dt: f64) -> FramePlan {
    let t = crate::project::frame_time(n, p.fps);
    let te = t + 1e-6; // positions snapped to frames sit on, not after, the frame time
    let size = (p.w as f64, p.h as f64);
    let tb = t + dt;
    let layers = p
        .tracks
        .iter()
        .filter_map(|tr| {
            // Animated values are taken at `ta`; which events show and their source frames at `t`.
            let ta = if tr.bypass_blur { t } else { tb };
            let dt = ta - t;
            let shown = tr.mute_env.as_ref().is_none_or(|e| e.at(ta) >= 0.5);
            let opacity = if shown { tr.level * tr.level_env.as_ref().map_or(1.0, |e| e.at(ta).clamp(0.0, 1.0)) } else { 0.0 };
            let live: Vec<&VEvent> = tr.events.iter().filter(|e| e.c.position <= te && te < e.c.position + e.c.duration).collect();
            // A parent stays even without a picture of its own: its children's group goes through it.
            if tr.parent.is_none() && (opacity <= 0.0 || live.is_empty()) {
                return None;
            }
            let mut items: Vec<Item> = Vec::new();
            for (i, e) in live.iter().enumerate().filter(|_| opacity > 0.0) {
                let c = &e.c;
                let local = t - c.position;
                // Animated values inside the event: Pan/Crop, masks, envelopes, FX, fades.
                let la = local + dt;
                // Decision 0.9: an event overlapped by a later one stays at full alpha (a dissolve, not a dip).
                let last = i + 1 == live.len();
                let fade_out = if !last { 1.0 } else { fade(c.fade_out_curve, c.duration - local, c.fade_out) };
                let level = env_at(&c.envelopes, EnvKind::Opacity, la);
                let alpha = fade_out * level;
                // Undersample rate: the event shows a new frame every 1 / r project frames, holding each in between.
                let tq = match c.undersample.filter(|r| *r > 0.0 && *r < 1.0) {
                    Some(r) => c.position + ((local * p.fps * r + 1e-6).floor() / (p.fps * r)).min(local),
                    None => t,
                };
                // Reversed, a frame samples where it ends (its start, mirrored), so it shows exactly the forward frames backwards.
                let mut st = src_time(c, if c.reverse { tq + 1.0 / p.fps } else { tq });
                // A looping event repeats its media past the end; otherwise the last frame holds.
                if let Some(len) = e.looped {
                    st = crate::edit::loop_time(st, len);
                }
                if e.m.duration > 0.0 {
                    st = st.clamp(0.0, (e.m.duration - 0.5 / p.fps).max(0.0));
                }
                let ft = st + e.m.offset;
                let sf = (!e.m.still).then(|| if e.m.grid > 0.0 { (ft * e.m.grid + 1e-6).floor() as i64 } else { (ft * p.fps).round() as i64 });
                // Resampled: the frame after `sf` too, weighted by how far between the two this frame's time falls.
                let next = sf.filter(|_| e.blend).map(|n| (n + 1, (ft * e.m.grid - n as f64).clamp(0.0, 1.0) as f32)).filter(|n| n.1 >= 1.0 / 512.0);
                // A generator is evaluated at its media time; one that does not animate always at 0.
                let generator = e.generator.as_ref().and_then(|g| plugins::eval(g, st)).map(|g| {
                    let gt = if generate::animated(&g) { st + dt } else { 0.0 };
                    (g, gt)
                });
                let fx = evals(&c.fx, la);
                let masks: Vec<MaskPlan> = c
                    .masks
                    .iter()
                    .filter(|m| m.mode != MaskMode::Disabled)
                    .map(|m| MaskPlan { path: m.path.at(la), negative: m.mode == MaskMode::Negative, opacity: m.opacity, feather: m.feather })
                    .filter(|m| m.path.len() >= 3)
                    .collect();
                let plan = |alpha| EventPlan {
                    src: SrcAt { m: e.m.clone(), sf, generator: generator.clone() },
                    place: place(c.pan_crop.at(la), (e.m.w as f64, e.m.h as f64), size, c.keep_aspect),
                    alpha,
                    fx: fx.clone(),
                    next,
                    flicker: c.reduce_flicker,
                    masks: masks.clone(),
                };
                let kind = |tr: &crate::project::Transition| plugins::eval_transition(tr);
                // Progress through a transition `x` (0..1) of the way along: its Transition Progress envelope, else x.
                let progress = |tr: &crate::project::Transition, x: f64| {
                    let x = x.clamp(0.0, 1.0);
                    (if tr.progress.is_empty() { x as f32 } else { tr.progress.at(x) }).clamp(0.0, 1.0)
                };
                let out_from = c.duration - c.fade_out;
                match (&c.transition, &c.transition_out) {
                    (Some(tr), _) if i > 0 => {
                        let prev = &live[i - 1].c;
                        let len = (prev.position + prev.duration).min(c.position + c.duration) - c.position;
                        let a = match items.pop() {
                            Some(Item::Event(a)) => Some(a),
                            other => {
                                items.extend(other);
                                None
                            }
                        };
                        items.push(Item::Mix { a, b: Some(plan(alpha)), p: progress(tr, local / len.max(1e-9)), kind: kind(tr) });
                    }
                    (Some(tr), _) if local < c.fade_in => items.push(Item::Mix { a: None, b: Some(plan(alpha)), p: progress(tr, local / c.fade_in), kind: kind(tr) }),
                    // A transition on a fade-out with nothing after it: the event goes out to transparent through it.
                    (_, Some(tr)) if last && c.fade_out > 0.0 && local >= out_from => {
                        items.push(Item::Mix { a: Some(plan(level)), b: None, p: progress(tr, (local - out_from) / c.fade_out), kind: kind(tr) });
                    }
                    _ => items.push(Item::Event(plan(alpha * fade(c.fade_in_curve, local, c.fade_in)))),
                }
            }
            // Fade to Color: towards the top colour above 0, the bottom colour below.
            let fade = fade_to(tr.fade_env.as_ref(), tr.fade_colors, ta);
            let m = tr.motion.at(ta);
            // No keys yet: the 2D Shadow's or 2D Glow's own defaults.
            let halo = |h: &Option<Keyframes<Halo>>, d: Halo| h.as_ref().map(|k| if k.is_empty() { d } else { k.at(ta) });
            Some(TrackPlan {
                mode: tr.composite,
                opacity,
                motion: motion(m, size),
                persp: if tr.composite == Composite::ThreeD { motion3d(m, size) } else { None },
                fx: evals(&tr.fx, ta),
                fade,
                compositor: tr.compositor.as_ref().and_then(|f| plugins::eval(f, ta)),
                child: tr.child,
                parent: tr.parent.as_ref().map(|(mode, keys)| (*mode, motion(keys.at(ta), size))),
                shadow: halo(&tr.shadow, Halo::default()),
                glow: halo(&tr.glow, Halo::glow()),
                items,
            })
        })
        .collect();
    let bus_fade = p.bus_envs.iter().find(|e| e.kind == EnvKind::FadeToColor);
    FramePlan { layers, out_fx: evals(&p.out_fx, tb), fade: fade_to(bus_fade, p.bus_colors, tb), samples: Vec::new() }
}

/// Fade to Color at `t`: towards the top colour above 0, the bottom colour below; None at 0.
fn fade_to(env: Option<&Envelope>, colors: [[u8; 3]; 2], t: f64) -> Option<([u8; 3], f32)> {
    env.map(|e| e.at(t).clamp(-1.0, 1.0)).filter(|v| v.abs() >= 1.0 / 512.0).map(|v| (colors[(v < 0.0) as usize], v.abs()))
}

/// First frames of events that start in (n0, n1], for decoder pre-warming.
pub fn upcoming(p: &VideoPlan, n0: i64, n1: i64) -> Vec<i64> {
    let mut v: Vec<i64> = p.tracks.iter().flat_map(|t| &t.events).map(|e| (e.c.position * p.fps - 1e-6).ceil() as i64).filter(|&s| s > n0 && s <= n1).collect();
    v.sort();
    v.dedup();
    v
}

/// Frames to pre-warm from frame `n`: event starts in the next `ahead` frames and, when the loop end `b` of `lp` (frames)
/// is that close, the loop start `a` (usually mid-event) and the starts just after it.
pub fn warm_frames(p: &VideoPlan, n: i64, ahead: i64, lp: Option<(i64, i64)>) -> Vec<i64> {
    match lp {
        Some((a, b)) if n + ahead >= b => {
            let mut v = upcoming(p, n, b - 1);
            v.push(a);
            v.extend(upcoming(p, a, a + n + ahead - b));
            v
        }
        _ => upcoming(p, n, n + ahead),
    }
}

// ---- Audio ----

/// What an audio decoder streams: an event's source span and where it sits on the timeline.
/// A forward stream is reused while it is at the source time the event needs; a reversed one only for an identical span.
#[derive(Debug, Clone, PartialEq)]
pub struct ASrc {
    pub path: PathBuf,
    pub pos: f64,
    pub dur: f64,
    pub in_point: f64,
    /// Source seconds per timeline second.
    pub rate: f64,
    pub reverse: bool,
    /// The file's audio stream (`-map 0:a:N`, Stream ▸).
    pub stream: u32,
    /// Pitch change in cents, at the same length.
    pub pitch: f32,
}

/// R4b: how an audio event with a Velocity envelope reads its media (varispeed, `mix::Mixer::block`).
#[derive(Debug)]
pub struct Varispeed {
    /// The event as `edit::src_time` needs it: place, length, in point, rate, direction and the envelope.
    pub clip: Clip,
    /// The media length it repeats past its end (Switches › Loop), and its length: past it the event is silent.
    pub looped: Option<f64>,
    pub len: f64,
    /// Where the media starts in its file (a subclip's start).
    pub offset: f64,
}

impl Varispeed {
    /// Media seconds heard `local` seconds into the event, unwrapped (a looping event runs on past the media end).
    pub fn time(&self, local: f64) -> f64 {
        src_time(&self.clip, self.clip.position + local)
    }

    /// Where unwrapped time `s` is in the media, or None where the event is silent (before the media, or past its
    /// end without Loop).
    pub fn at(&self, s: f64) -> Option<f64> {
        match self.looped {
            Some(len) => Some(crate::edit::loop_time(s, len)),
            None => (0.0..self.len).contains(&s).then_some(s),
        }
    }

    /// The earliest media time heard from `local` seconds in to the event's end (every 10 ms and at each envelope
    /// point), so a decoder opened there has all the rest.
    pub fn earliest(&self, local: f64) -> f64 {
        let d = self.clip.duration;
        let n = ((d - local) / 0.01).ceil().clamp(1.0, 100_000.0) as usize;
        let keys = crate::engine::velocity::of(&self.clip).into_iter().flat_map(|e| e.points.keys.iter().map(|k| k.t));
        let ts = (0..=n).map(|i| local + (d - local) * i as f64 / n as f64).chain(keys.filter(|t| *t > local && *t < d));
        let (lo, hi) = ts.map(|t| self.time(t)).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), s| (a.min(s), b.max(s)));
        match self.looped {
            // A wrap ahead brings the media start back.
            Some(len) if lo < 0.0 || hi >= len => 0.0,
            Some(len) => crate::edit::loop_time(lo, len),
            None => lo.max(0.0),
        }
    }
}

#[derive(Debug)]
pub struct AEvent {
    /// The clip id: keys its FX state.
    pub id: String,
    /// The whole event (its place and length: fades, envelopes and FX are event-relative).
    pub src: ASrc,
    /// What is read: the event, or for a looping event one stream per pass through its media (`edit::loop_passes`).
    pub passes: Vec<ASrc>,
    /// R4b: a Velocity envelope (the one its video event's has, `edit::sync_velocity`): read varispeed instead.
    pub vel: Option<Varispeed>,
    /// Event gain with the Normalize gain, linear.
    pub gain: f32,
    /// Channels ▸ and Switches › Invert Phase, applied to the PCM as it is read.
    pub channels: ChannelMap,
    pub invert: bool,
    pub fade_in: f64,
    pub fade_out: f64,
    pub fade_in_curve: Curve,
    pub fade_out_curve: Curve,
    /// Event volume envelope (dB), event-relative.
    pub vol_env: Option<Envelope>,
    /// Audio Event FX, keyed event-relative; they run on the PCM before gain, fades and envelope.
    pub fx: Vec<Fx>,
}

#[derive(Debug)]
pub struct ATrack {
    /// The track id: keys its FX state.
    pub id: String,
    /// Audio Track FX, run before the track's envelopes, Volume and Pan.
    pub fx: Vec<Fx>,
    /// Static Volume (linear) and Pan (-1 left .. 1 right). With a pan envelope, Pan is added to it as a trim, as in VEGAS.
    pub volume: f32,
    pub pan: f32,
    pub vol_env: Option<Envelope>,
    pub pan_env: Option<Envelope>,
    pub mute_env: Option<Envelope>,
    /// Track Switches › Invert Phase.
    pub invert: bool,
    pub events: Vec<AEvent>,
}

#[derive(Debug)]
pub struct AudioPlan {
    /// Audible tracks only (muted and un-soloed tracks are left out).
    pub tracks: Vec<ATrack>,
    /// Audio Master FX, run before the master gain.
    pub master_fx: Vec<Fx>,
    /// Options › Bypass All Audio FX (playback only; a render never sets it).
    pub bypass_fx: bool,
    /// Options › Quickfade Audio Edits: seconds of linear fade at each event edge that has no fade of its own (0 = off).
    /// Playback and render both set it from the option.
    pub quickfade: f64,
}

/// The quickfade length (VEGAS's Preferences › Editing default, 10 ms).
pub const QUICKFADE: f64 = 0.010;

pub fn compile_audio(p: &Project) -> AudioPlan {
    let media: HashMap<&str, &MediaItem> = p.media_pool.iter().filter(|m| m.sample_rate.is_some() && m.generator.is_none()).map(|m| (m.id.as_str(), m)).collect();
    let env = |envs: &[Envelope], kind| envs.iter().find(|e| e.kind == kind && !e.points.keys.is_empty()).cloned();
    let any_solo = p.timeline.any_solo(false);
    let tracks = p
        .timeline
        .audio_tracks
        .iter()
        .filter_map(|t| {
            crate::audio::track_gain(&p.timeline, t, any_solo)?; // muted or un-soloed: left out
            let events: Vec<AEvent> = t
                .clips
                .iter()
                .filter(|c| !c.muted && c.duration > 0.0 && c.rate > 0.0)
                .filter_map(|c| {
                    let m = media.get(c.media_id.as_deref()?)?;
                    // In the file: a subclip starts part way in.
                    let stream = c.stream.min(m.streams(false) - 1);
                    let pitch = c.pitch_cents;
                    let src = |pos, dur, in_point| ASrc { path: m.path.clone(), pos, dur, in_point: m.file_time(in_point), rate: c.rate, reverse: c.reverse, stream, pitch };
                    let vel = crate::engine::velocity::of(c).map(|_| Varispeed {
                        clip: Clip { fx: Vec::new(), takes: Vec::new(), pan_crop: Default::default(), ..c.clone() },
                        looped: crate::edit::loop_len(c, m),
                        len: m.duration,
                        offset: m.file_time(0.0),
                    });
                    Some(AEvent {
                        id: c.id.clone(),
                        fx: c.fx.clone(),
                        passes: crate::edit::loop_passes(c, crate::edit::loop_len(c, m)).into_iter().map(|(pos, dur, in_point)| src(pos, dur, in_point)).collect(),
                        vel,
                        src: src(c.position, c.duration, c.in_point),
                        gain: crate::engine::keyframes::db_to_gain(c.gain_db + crate::edit::normalize_gain(c)),
                        channels: c.channels,
                        invert: c.invert,
                        fade_in: c.fade_in,
                        fade_out: c.fade_out,
                        fade_in_curve: c.fade_in_curve,
                        fade_out_curve: c.fade_out_curve,
                        vol_env: env(&c.envelopes, EnvKind::Volume),
                    })
                })
                .collect();
            let e = &t.envelopes;
            (!events.is_empty()).then(|| ATrack {
                id: t.id.clone(),
                fx: t.fx.clone(),
                volume: t.volume,
                pan: t.pan.clamp(-1.0, 1.0),
                vol_env: env(e, EnvKind::Volume),
                pan_env: env(e, EnvKind::Pan),
                mute_env: env(e, EnvKind::Mute),
                invert: t.invert,
                events,
            })
        })
        .collect();
    AudioPlan { tracks, master_fx: p.timeline.master.audio_fx.clone(), bypass_fx: false, quickfade: 0.0 }
}

/// What a Normalize scan of audio event `c` reads (`mix::peak`): its media's stream from its in point for the span it
/// plays, or the whole file when it loops past the end; None without audio media.
pub fn norm_source(p: &Project, c: &Clip) -> Option<(ASrc, f64)> {
    let m = p.media_pool.iter().find(|m| Some(&m.id) == c.media_id.as_ref() && m.sample_rate.is_some() && m.generator.is_none())?;
    let span = c.duration * c.rate;
    let (from, len) = match crate::edit::loop_len(c, m) {
        Some(l) if c.in_point + span > l => (0.0, l),
        _ => (c.in_point, span.min(m.duration - c.in_point).max(0.0)),
    };
    let stream = c.stream.min(m.streams(false) - 1);
    Some((ASrc { path: m.path.clone(), pos: 0.0, dur: len, in_point: m.file_time(from), rate: 1.0, reverse: false, stream, pitch: 0.0 }, len))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Keyframe, Track, TrackKind, Transition};
    use crate::engine::plugins;

    fn media(id: &str, kind: MediaType) -> MediaItem {
        MediaItem {
            id: id.into(),
            path: format!("{id}.bin").into(),
            name: id.into(),
            media_type: kind,
            duration: 100.0,
            width: Some(1920),
            height: Some(1080),
            frame_rate: Some(30.0),
            ..Default::default()
        }
    }

    fn clip(m: &MediaItem, pos: f64, dur: f64, inp: f64) -> Clip {
        let mut c = Clip::from_media(m, pos);
        (c.duration, c.in_point, c.out_point) = (dur, inp, inp + dur);
        c
    }

    /// (media id, source frame, alpha) of every drawn event, bottom first.
    fn drawn(p: &Project, t: f64) -> Vec<(String, Option<i64>, f32)> {
        let vp = compile(p);
        let fp = frame_plan(&vp, frame_at(t, vp.fps));
        let ev = |e: &EventPlan| (e.src.m.id.to_string(), e.src.sf, e.alpha);
        fp.layers
            .iter()
            .flat_map(|l| &l.items)
            .flat_map(|i| match i {
                Item::Event(e) => vec![ev(e)],
                Item::Mix { a, b, .. } => a.iter().chain(b).map(ev).collect(),
            })
            .collect()
    }

    fn ids(p: &Project, t: f64) -> Vec<String> {
        drawn(p, t).into_iter().map(|d| d.0).collect()
    }

    fn project() -> Project {
        let mut p = Project::new("t");
        p.settings.frame_rate = 30.0;
        // Source frames as decoded, unblended (the resample tests turn it on).
        p.settings.resample = Resample::Disable;
        let (a, b, img) =(media("a", MediaType::Video), media("b", MediaType::Video), media("i", MediaType::Image));
        p.timeline.video_tracks[0].clips.push(clip(&a, 10.0, 5.0, 2.0));
        p.timeline.video_tracks.push(Track::new("Video 2", TrackKind::Video));
        p.timeline.video_tracks[1].clips.push(clip(&b, 0.0, 20.0, 0.0));
        p.timeline.video_tracks[1].clips.push(clip(&img, 30.0, 5.0, 0.0));
        p.media_pool = vec![a, b, img];
        p
    }

    #[test]
    fn track_order_and_hidden_muted_solo() {
        let mut p = project();
        assert_eq!(ids(&p, 11.5), ["b", "a"]); // bottom (track 2) first, top track drawn last
        assert_eq!(drawn(&p, 11.5)[1].1, Some(105)); // a: 2.0 + (11.5 - 10) = 3.5 s
        assert_eq!(ids(&p, 16.0), ["b"]);
        p.timeline.video_tracks[0].visible = false;
        assert_eq!(ids(&p, 11.5), ["b"]);
        p.timeline.video_tracks[0].visible = true;
        p.timeline.video_tracks[0].muted = true;
        assert_eq!(ids(&p, 11.5), ["b"]);
        p.timeline.video_tracks[0].muted = false;
        p.timeline.video_tracks[1].solo = true;
        assert_eq!(ids(&p, 11.5), ["b"]);
        p.timeline.video_tracks[1].solo = false;
        assert_eq!(drawn(&p, 32.0), [("i".to_string(), None, 1.0)]); // a still has no source frame
        assert!(ids(&p, 25.0).is_empty()); // gap: black
    }

    /// R5: a collapsed (or hidden) group still renders; group Mute and Solo reach the plan.
    #[test]
    fn track_groups_render_and_switch() {
        let mut p = project();
        let tracks: Vec<String> = p.timeline.video_tracks.iter().map(|t| t.id.clone()).collect();
        assert!(crate::edit::group_tracks(&mut p, &tracks[..1]));
        p.timeline.track_groups[0].collapsed = true;
        p.timeline.video_tracks[0].hidden = true;
        assert_eq!(ids(&p, 11.5), ["b", "a"]);
        p.timeline.track_groups[0].muted = true;
        assert_eq!(ids(&p, 11.5), ["b"]);
        (p.timeline.track_groups[0].muted, p.timeline.track_groups[0].solo, p.timeline.video_tracks[0].muted) = (false, true, true);
        assert_eq!(ids(&p, 11.5), ["a"], "a soloed group plays its muted track and solos the others out");
    }

    #[test]
    fn later_event_wins_and_muted_event_is_skipped() {
        let mut p = project();
        let a = p.media_pool[0].clone();
        let tr = &mut p.timeline.video_tracks[1].clips;
        tr.insert(0, clip(&a, 5.0, 2.0, 0.0)); // earlier in the Vec, later position: drawn on top of b
        assert_eq!(ids(&p, 6.0), ["b", "a"]);
        p.timeline.video_tracks[1].clips[1].position = 5.0; // a tie: the later Vec entry (b) is on top
        assert_eq!(ids(&p, 6.0), ["a", "b"]);
        p.timeline.video_tracks[1].clips[0].muted = true;
        assert_eq!(ids(&p, 6.0), ["b"]);
    }

    #[test]
    fn source_frame_rounding_rate_and_reverse() {
        let mut p = project();
        for fps in [29.97, 23.976] {
            p.settings.frame_rate = fps;
            let c = &mut p.timeline.video_tracks[0].clips[0];
            c.in_point = 2.0 + 0.4 / fps; // off the frame grid by 0.4 frame
            for n in [300i64, 301, 350] {
                let t = n as f64 / fps;
                let sf = drawn(&p, t)[1].1.unwrap();
                assert_eq!(sf, ((2.0 + 0.4 / fps + t - 10.0) * fps).round() as i64, "{fps} {n}");
            }
        }
        p.settings.frame_rate = 30.0;
        let c = &mut p.timeline.video_tracks[0].clips[0];
        c.in_point = 2.0;
        c.rate = 2.0;
        assert_eq!(drawn(&p, 11.0)[1].1, Some(120)); // 2 + 1 s x 2 = 4 s
        assert_eq!(drawn(&p, 12.0)[1].1, Some(180));
        let c = &mut p.timeline.video_tracks[0].clips[0];
        (c.rate, c.reverse) = (1.0, true);
        assert_eq!(src_time(c, 15.0), 2.0); // the event end shows in_point
        assert_eq!(src_time(c, 10.0), 7.0);
        // Reversed frames are the forward frames 60..=209 mirrored: frame k shows forward frame N - 1 - k.
        let fwd: Vec<_> = {
            let mut q = p.clone();
            q.timeline.video_tracks[0].clips[0].reverse = false;
            (300..450).map(|n| drawn(&q, n as f64 / 30.0)[1].1.unwrap()).collect()
        };
        assert_eq!((fwd[0], fwd[149]), (60, 209));
        for k in 0..150 {
            assert_eq!(drawn(&p, (300 + k) as f64 / 30.0)[1].1, Some(fwd[149 - k]), "frame {k}");
        }
    }

    /// 60p in a 30p project at half speed (shoot 60, conform to 30): every output frame is the next source frame.
    #[test]
    fn slow_motion_reaches_every_source_frame() {
        let mut p = project();
        p.timeline.video_tracks[1].visible = false;
        let m = &mut p.media_pool[0];
        m.frame_rate = Some(60.0);
        let c = &mut p.timeline.video_tracks[0].clips[0];
        (c.in_point, c.rate) = (0.0, 0.5);
        let sfs: Vec<i64> = (300..306).map(|n| drawn(&p, n as f64 / 30.0)[0].1.unwrap()).collect();
        assert_eq!(sfs, [0, 1, 2, 3, 4, 5]);
        // At normal speed the project grid is exact and stays (no extra frames read).
        p.timeline.video_tracks[0].clips[0].rate = 1.0;
        assert_eq!(drawn(&p, 301.0 / 30.0)[0].1, Some(1));
    }

    /// R3a: an 8 s clip extended to 20 s shows source 0..8, 0..8, 0..4; with Loop off the last frame holds. Its sound
    /// is read in the same three passes.
    #[test]
    fn a_looping_event_repeats_its_media() {
        let mut p = project();
        p.timeline.video_tracks[1].visible = false;
        p.media_pool[0].duration = 8.0;
        p.media_pool[0].sample_rate = Some(48_000);
        let c = &mut p.timeline.video_tracks[0].clips[0];
        (c.position, c.in_point, c.duration) = (0.0, 0.0, 20.0);
        let sf = |p: &Project, t: f64| drawn(p, t)[0].1.unwrap();
        assert_eq!([0.0, 7.9, 8.0, 15.0, 16.0, 19.9].map(|t| sf(&p, t)), [0, 237, 0, 210, 0, 117]);
        p.timeline.video_tracks[0].clips[0].looping = false;
        assert!((239..=240).contains(&sf(&p, 19.9)), "the last frame holds (as before R3a)");
        // A reversed loop runs back through each pass.
        let c = &mut p.timeline.video_tracks[0].clips[0];
        (c.looping, c.reverse) = (true, true);
        assert_eq!([0.0, 3.9, 4.0, 19.9, 599.0 / 30.0].map(|t| sf(&p, t)), [119, 2, 239, 2, 0]);
        // Audio: one stream per pass.
        let mut c = p.timeline.video_tracks[0].clips[0].clone();
        c.reverse = false;
        p.timeline.audio_tracks[0].clips.push(c);
        let a = compile_audio(&p);
        let passes: Vec<(f64, f64, f64)> = a.tracks[0].events[0].passes.iter().map(|s| (s.pos, s.dur, s.in_point)).collect();
        assert_eq!(passes, [(0.0, 8.0, 0.0), (8.0, 8.0, 0.0), (16.0, 4.0, 0.0)]);
        assert_eq!((a.tracks[0].events[0].src.pos, a.tracks[0].events[0].src.dur), (0.0, 20.0), "fades span the whole event");
    }

    /// The video event of `drawn` at `t`, as planned.
    fn event_at(p: &Project, t: f64) -> EventPlan {
        let vp = compile(p);
        let mut fp = frame_plan(&vp, frame_at(t, vp.fps));
        match fp.layers.pop().and_then(|mut l| l.items.pop()) {
            Some(Item::Event(e)) => e,
            other => panic!("{other:?}"),
        }
    }

    /// R3b: a 25p source in a 30p project is resampled (Smart): each frame blends the two source frames around its
    /// time, read on the source's own grid. Disable shows the latest frame; Force blends whenever frames fall between
    /// source frames, and Optical Flow interpolates instead.
    #[test]
    fn resample_blends_neighbouring_source_frames() {
        let mut p = project();
        p.timeline.video_tracks[1].visible = false;
        p.media_pool[0].frame_rate = Some(25.0);
        p.settings.resample = Resample::Smart;
        let c = &mut p.timeline.video_tracks[0].clips[0];
        c.in_point = 0.0; // a: 10..15 s, source 0..5 s
        // Frame 301 is 10.0333 s, source 1/30 s: 25p frame 0.8333.
        let e = event_at(&p, 301.0 / 30.0);
        assert_eq!((e.src.m.grid, e.src.sf), (25.0, Some(0)));
        let (n, w) = e.next.unwrap();
        assert!(n == 1 && (w - 5.0 / 6.0).abs() < 1e-4, "{n} {w}");
        p.timeline.video_tracks[0].clips[0].resample = Some(Resample::Disable);
        let e = event_at(&p, 301.0 / 30.0);
        assert!(e.next.is_none() && e.src.m.grid == 0.0 && e.src.sf == Some(1), "the project grid, the frame at or before");
        // A 30p source at normal speed: Smart leaves it, Force blends (on frame times: nothing to blend).
        p.media_pool[0].frame_rate = Some(30.0);
        p.timeline.video_tracks[0].clips[0].resample = None;
        assert!(event_at(&p, 10.5).next.is_none() && event_at(&p, 10.5).src.m.grid == 0.0);
        p.media_pool[0].frame_rate = Some(60.0); // every project frame is a whole 60p frame: read as before
        assert!(event_at(&p, 301.0 / 30.0).next.is_none() && event_at(&p, 301.0 / 30.0).src.m.grid == 0.0);
        p.media_pool[0].frame_rate = Some(30.0);
        let c = &mut p.timeline.video_tracks[0].clips[0];
        (c.resample, c.rate) = (Some(Resample::Force), 0.5);
        let e = event_at(&p, 301.0 / 30.0); // half speed: source 1/60 s, between frames 0 and 1
        assert_eq!(e.src.sf, Some(0));
        assert!(e.next.is_some_and(|(n, w)| n == 1 && (w - 0.5).abs() < 1e-4), "{:?}", e.next);
        // Optical Flow: the slow motion is interpolated onto a 60 fps grid of its own (another decoder), not blended.
        p.timeline.video_tracks[0].clips[0].resample = Some(Resample::OpticalFlow);
        let e = event_at(&p, 301.0 / 30.0);
        assert!(e.next.is_none() && e.src.m.flow && e.src.m.grid == 60.0 && &*e.src.m.id == "a#0f", "{:?}", e.src.m);
        assert_eq!(e.src.sf, Some(1));
        // An old project opens with resampling off.
        let old: Project = serde_json::from_str(&serde_json::to_string(&Project::new("o")).unwrap().replace(r#","resample":"Smart""#, "")).unwrap();
        assert_eq!(old.settings.resample, Resample::Disable);
    }

    /// R3b: Stream ▸ decodes another video stream under its own id; the Undersample rate holds frames; Maintain Aspect
    /// Ratio off stretches; Reduce Interlace Flicker reaches the compositor.
    #[test]
    fn stream_undersample_aspect_and_flicker() {
        let mut p = project();
        p.timeline.video_tracks[1].visible = false;
        p.media_pool[0].video_streams = 2;
        let c = &mut p.timeline.video_tracks[0].clips[0];
        (c.stream, c.reduce_flicker) = (1, true);
        let e = event_at(&p, 11.0);
        assert_eq!((&*e.src.m.id, e.src.m.stream, e.src.m.proxy.is_none(), e.flicker), ("a#1", 1, true, true));
        // Half the frame rate: each source frame shows for two project frames.
        let c = &mut p.timeline.video_tracks[0].clips[0];
        (c.stream, c.undersample) = (0, Some(0.5));
        let sfs: Vec<_> = (300..306).map(|n| event_at(&p, n as f64 / 30.0).src.sf.unwrap()).collect();
        assert_eq!(sfs, [60, 60, 62, 62, 64, 64]);
        // 1920x1080 media in a 4:3 project: fitted (letterboxed) or stretched to the full frame.
        (p.settings.width, p.settings.height) = (1440, 1080);
        let fit = event_at(&p, 11.0).place;
        p.timeline.video_tracks[0].clips[0].keep_aspect = false;
        let stretch = event_at(&p, 11.0).place;
        assert_eq!((fit.apply(0.0, 0.0).1 > 0.1, stretch.apply(0.0, 0.0), stretch.apply(1.0, 1.0)), (true, (0.0, 0.0), (1.0, 1.0)));
    }

    /// An odd size (an imported VEGAS project) is composed one pixel smaller, as 4:2:0 encoders need.
    #[test]
    fn odd_frame_sizes_compose_even() {
        let mut p = project();
        (p.settings.width, p.settings.height) = (1279, 719);
        let vp = compile(&p);
        assert_eq!((vp.w, vp.h), (1278, 718));
    }

    #[test]
    fn warm_frames_cross_the_loop_wrap() {
        let vp = compile(&project()); // event starts at frames 0, 300 and 900
        assert_eq!(warm_frames(&vp, 280, 45, None), [300]);
        assert_eq!(warm_frames(&vp, 280, 45, Some((150, 360))), [300]); // the loop end is not near yet
        assert_eq!(warm_frames(&vp, 340, 45, Some((150, 360))), [150]); // the loop start, mid-event
        assert_eq!(warm_frames(&vp, 890, 45, Some((360, 930))), [900, 360]);
        assert_eq!(warm_frames(&vp, 590, 45, Some((297, 600))), [297, 300]); // and a start just after the wrap
    }

    #[test]
    fn colour_matrix() {
        let mut m = media("a", MediaType::Video);
        assert_eq!(matrix(&m), Some("bt709")); // untagged HD
        m.height = Some(480);
        assert_eq!(matrix(&m), None);
        m.color_matrix = Some("bt470bg".into());
        assert_eq!(matrix(&m), Some("bt470"));
        assert_eq!(matrix(&media("i", MediaType::Image)), None); // an untagged JPEG stays BT.601
    }

    #[test]
    fn effective_end_and_loop_frames() {
        assert_eq!(effective_end(10.0, None), 10.0);
        assert_eq!(effective_end(10.0, Some((12.0, 14.0))), 14.0);
        assert_eq!(effective_end(10.0, Some((2.0, 4.0))), 10.0);
        assert_eq!(loop_frames((1.0, 2.0), 30.0), (30, 60));
        assert_eq!(loop_frames((1.01, 2.01), 30.0), (30, 61));
    }

    #[test]
    fn dissolve_rule_and_transition_progress() {
        let mut p = project();
        let b = p.media_pool[1].clone();
        let tr = &mut p.timeline.video_tracks[0].clips;
        tr[0].fade_out = 2.0; // a: 10..15, crossfades into b2 over 13..15
        let mut b2 = clip(&b, 13.0, 5.0, 0.0);
        b2.fade_in = 2.0;
        tr.push(b2);
        p.timeline.video_tracks[1].visible = false;
        let d = drawn(&p, 14.0);
        assert_eq!((d[0].0.as_str(), d[0].2), ("a", 1.0)); // outgoing stays opaque inside the overlap
        assert_eq!(d[1].0, "b");
        assert!((d[1].2 - 0.5).abs() < 1e-3); // incoming at its (smooth) fade-in midpoint

        p.timeline.video_tracks[0].clips[1].transition = Some(Transition { plugin: "milan.tr.dissolve".into(), ..Default::default() });
        let vp = compile(&p);
        let mix_p = |vp: &VideoPlan, n| match &frame_plan(vp, n).layers[0].items[..] {
            [Item::Mix { a: Some(_), b: Some(b), p, kind: Some(k) }] if k.id == "milan.tr.dissolve" => (*p, b.alpha),
            other => panic!("{other:?}"),
        };
        assert_eq!(mix_p(&vp, 390), (0.0, 1.0)); // 13.0 s: start of the overlap
        let (end, _) = mix_p(&vp, 449); // the last frame inside it
        assert!((end - 59.0 / 60.0).abs() < 1e-4);
        assert!(matches!(&frame_plan(&vp, 450).layers[0].items[..], [Item::Event(_)])); // 15.0 s: a has ended
        // R4a: a Transition Progress envelope sets the progress: held at 25% here, whatever the time.
        let q = |v: f32| Keyframes { keys: vec![Keyframe { t: 0.0, v, curve: Curve::Linear }] };
        p.timeline.video_tracks[0].clips[1].transition.as_mut().unwrap().progress = q(0.25);
        let vq = compile(&p);
        assert!([390, 420, 449].iter().all(|&n| mix_p(&vq, n).0 == 0.25));
        // 0 -> 100% -> 0 over the overlap: back to the outgoing picture at the end.
        p.timeline.video_tracks[0].clips[1].transition.as_mut().unwrap().progress =
            Keyframes { keys: vec![Keyframe { t: 0.0, v: 0.0, curve: Curve::Linear }, Keyframe { t: 0.5, v: 1.0, curve: Curve::Linear }, Keyframe { t: 1.0, v: 0.0, curve: Curve::Linear }] };
        let vq = compile(&p);
        assert!((mix_p(&vq, 420).0 - 1.0).abs() < 1e-6 && mix_p(&vq, 449).0 < 0.05);
        p.timeline.video_tracks[0].clips[1].transition.as_mut().unwrap().progress = Keyframes::default();
        // A lone fade-in with a transition mixes from transparent.
        p.timeline.video_tracks[0].clips.remove(0);
        let vp = compile(&p);
        assert!(matches!(&frame_plan(&vp, 420).layers[0].items[..], [Item::Mix { a: None, p, .. }] if (*p - 0.5).abs() < 1e-6));
        // R4a: a transition on a fade-out with nothing after it mixes out to transparent, at the event's own level.
        let c = &mut p.timeline.video_tracks[0].clips[0]; // b: 13..18 s
        (c.fade_out, c.transition_out) = (2.0, Some(Transition { plugin: "milan.tr.linear_wipe".into(), ..Default::default() }));
        let vp = compile(&p);
        assert!(matches!(&frame_plan(&vp, 450).layers[0].items[..], [Item::Event(e)] if e.alpha == 1.0)); // 15 s: before the fade
        match &frame_plan(&vp, 510).layers[0].items[..] {
            [Item::Mix { a: Some(a), b: None, p, kind: Some(k) }] => assert!(k.id == "milan.tr.linear_wipe" && (*p - 0.5).abs() < 1e-6 && a.alpha == 1.0),
            other => panic!("{other:?}"),
        }
    }

    /// R4a: video track Mute and Fade to Color envelopes; Velocity reaches the frame plan (and resamples, being off speed).
    #[test]
    fn track_mute_fade_to_color_and_velocity() {
        let mut p = project();
        p.timeline.video_tracks[1].visible = false;
        let key = |t, v, curve| Keyframe { t, v, curve };
        let t = &mut p.timeline.video_tracks[0]; // a: 10..15 s
        t.envelopes.push(Envelope { kind: EnvKind::Mute, points: Keyframes { keys: vec![key(0.0, 1.0, Curve::Hold), key(12.0, 0.0, Curve::Hold)] } });
        assert_eq!((ids(&p, 11.0), ids(&p, 12.5)), (vec!["a".to_string()], vec![]));
        let t = &mut p.timeline.video_tracks[0];
        t.envelopes.clear();
        t.fade_colors = [[255, 0, 0], [0, 0, 255]];
        t.envelopes.push(Envelope { kind: EnvKind::FadeToColor, points: Keyframes { keys: vec![key(10.0, 0.0, Curve::Linear), key(12.0, -1.0, Curve::Linear), key(14.0, 0.5, Curve::Linear)] } });
        let fade = |p: &Project, s: f64| frame_plan(&compile(p), frame_at(s, 30.0)).layers[0].fade;
        assert_eq!((fade(&p, 10.0), fade(&p, 12.0), fade(&p, 14.0)), (None, Some(([0, 0, 255], 1.0)), Some(([255, 0, 0], 0.5))));
        p.timeline.video_tracks[0].envelopes.clear();
        // Velocity 200%: source frame 60 + 2 x 30 a second after the start; Smart resampling now blends (off speed).
        p.settings.resample = Resample::Smart;
        assert_eq!(event_at(&p, 11.0).src.m.grid, 0.0, "30p at normal speed: nothing to blend");
        p.timeline.video_tracks[0].clips[0].envelopes.push(Envelope { kind: EnvKind::Velocity, points: Keyframes { keys: vec![key(0.0, 2.0, Curve::Linear)] } });
        assert_eq!(drawn(&p, 11.0)[0].1, Some(120));
        assert_eq!(event_at(&p, 11.0).src.m.grid, 30.0);
    }

    #[test]
    fn opacity_envelope_level_and_curves() {
        let mut p = project();
        p.timeline.video_tracks[1].visible = false;
        let c = &mut p.timeline.video_tracks[0].clips[0];
        (c.fade_in, c.fade_in_curve) = (2.0, Curve::Linear);
        c.envelopes.push(Envelope { kind: EnvKind::Opacity, points: Keyframes { keys: vec![Keyframe { t: 0.0, v: 0.5, curve: Curve::Linear }] } });
        assert!((drawn(&p, 10.5)[0].2 - 0.125).abs() < 1e-6); // 0.25 linear fade x 0.5 opacity
        let t = &mut p.timeline.video_tracks[0];
        t.volume = 0.5;
        t.envelopes.push(Envelope { kind: EnvKind::CompositeLevel, points: Keyframes { keys: vec![Keyframe { t: 0.0, v: 0.5, curve: Curve::Linear }] } });
        let vp = compile(&p);
        assert_eq!(frame_plan(&vp, 330).layers[0].opacity, 0.25);
    }

    /// R7: Make Compositing Child marks the track below a parent; the parent stays in the plan without a picture of
    /// its own; a child of a parent that does not show composites on its own; Custom carries its compositor.
    #[test]
    fn parent_child_and_custom_in_the_plan() {
        let mut p = project(); // track 0 (a: 10..15 s) over track 1 (b: 0..20 s)
        p.timeline.video_tracks[1].child = true;
        p.timeline.video_tracks[0].parent_mode = Composite::Add;
        let flags = |p: &Project, t: f64| frame_plan(&compile(p), frame_at(t, 30.0)).layers.iter().map(|l| (l.child, l.parent.map(|x| x.0), l.items.len())).collect::<Vec<_>>();
        assert_eq!(flags(&p, 11.0), [(true, None, 1), (false, Some(Composite::Add), 1)]);
        assert_eq!(flags(&p, 5.0), [(true, None, 1), (false, Some(Composite::Add), 0)], "the parent stays without its event");
        p.timeline.video_tracks[0].muted = true;
        assert_eq!(flags(&p, 11.0), [(true, None, 1)]);
        // The top track cannot be a child.
        let mut q = project();
        q.timeline.video_tracks[0].child = true;
        assert_eq!(flags(&q, 11.0), [(false, None, 1), (false, None, 1)]);
        let t = &mut q.timeline.video_tracks[0];
        (t.composite, t.compositor) = (Composite::Custom, Some(plugins::new_fx(plugins::find("milan.comp.bump_map").unwrap(), None)));
        assert_eq!(frame_plan(&compile(&q), 330).layers[1].compositor.as_ref().map(|c| c.id), Some("milan.comp.bump_map"));
    }

    /// R7: the Video Bus Track's Motion Blur Amount gives weighted sub-frames over its length (asymmetric types trail the
    /// frame); Supersampling alone spreads them over the frame; sub-frames move Track Motion and Pan/Crop but keep the
    /// source frame, and Bypass Motion Blur keeps a track still.
    #[test]
    fn motion_blur_samples_and_sub_frames() {
        let mut p = project();
        let env = |kind, v| Envelope { kind, points: Keyframes { keys: vec![Keyframe { t: 0.0, v, curve: Curve::Linear }] } };
        assert!(frame_plan(&compile(&p), 330).samples.is_empty());
        p.timeline.master.video_envelopes = vec![env(EnvKind::MotionBlur, 0.2)];
        let s = frame_plan(&compile(&p), 330).samples;
        assert_eq!(s.len(), 6); // 0.2 s at 30 fps
        assert!(s.iter().all(|x| x.0.abs() <= 0.1) && s[0].1 < s[2].1, "centred, Gaussian: {s:?}");
        p.settings.motion_blur = BlurType::AsymmetricBox;
        let s = frame_plan(&compile(&p), 330).samples;
        assert!(s.iter().all(|x| x.0 <= 0.0 && x.0 >= -0.2 && x.1 == 1.0), "{s:?}");
        p.timeline.master.video_envelopes = vec![env(EnvKind::Supersampling, 3.0)];
        let s = frame_plan(&compile(&p), 330).samples;
        assert!(s.len() == 4 && s.iter().all(|x| x.0.abs() < 0.5 / 30.0), "{s:?}");
        // Track Motion keyed across the second: sub-frames move it; the source frame stays.
        let t = &mut p.timeline.video_tracks[0];
        t.motion = Keyframes { keys: vec![Keyframe { t: 10.0, v: Motion::default(), curve: Curve::Linear }, Keyframe { t: 12.0, v: Motion { x: 1.0, ..Default::default() }, curve: Curve::Linear }] };
        let vp = compile(&p);
        let (a, b) = (frame_plan_at(&vp, 330, 0.0), frame_plan_at(&vp, 330, 0.5));
        assert!((b.layers[1].motion.tx - a.layers[1].motion.tx - 0.25).abs() < 1e-9);
        let item_sf = |fp: &FramePlan| match &fp.layers[1].items[0] {
            Item::Event(e) => e.src.sf,
            _ => None,
        };
        assert_eq!(item_sf(&a), item_sf(&b));
        p.timeline.video_tracks[0].bypass_blur = true;
        let vp = compile(&p);
        assert_eq!(frame_plan_at(&vp, 330, 0.5).layers[1].motion, frame_plan_at(&vp, 330, 0.0).layers[1].motion);
        // The bus Fade to Color, towards its bottom colour.
        p.timeline.master.video_envelopes = vec![env(EnvKind::FadeToColor, -0.5)];
        p.timeline.master.fade_colors[1] = [10, 20, 30];
        assert_eq!(frame_plan(&compile(&p), 330).fade, Some(([10, 20, 30], 0.5)));
    }

    /// R7: Pan/Crop masks reach the event plan at the frame's time (disabled ones and paths under 3 points do not).
    #[test]
    fn masks_reach_the_event_plan() {
        use crate::project::Mask;
        let mut p = project();
        p.timeline.video_tracks[1].visible = false;
        let path = vec![[0.0, 0.0, 0.0, 0.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0, 0.0, 0.0], [1.0, 1.0, 0.0, 0.0, 0.0, 0.0]];
        let m = |mode| Mask { path: Keyframes { keys: vec![Keyframe { t: 0.0, v: path.clone(), curve: Curve::Linear }] }, mode, opacity: 0.5, feather: 0.1 };
        p.timeline.video_tracks[0].clips[0].masks = vec![m(MaskMode::Negative), m(MaskMode::Disabled), Mask { path: Keyframes::default(), ..m(MaskMode::Positive) }];
        let e = event_at(&p, 11.0);
        assert_eq!(e.masks, vec![MaskPlan { path, negative: true, opacity: 0.5, feather: 0.1 }]);
    }

    #[test]
    fn frame_plan_key_is_stable_and_sensitive() {
        let mut p = project();
        let key = |p: &Project| frame_plan(&compile(p), 345).key();
        let k = key(&p);
        assert_eq!(k, key(&p));
        let edits: [&dyn Fn(&mut Project); 6] = [
            &|p| p.timeline.video_tracks[0].clips[0].in_point += 0.1,
            &|p| p.timeline.video_tracks[0].clips[0].fade_in = 3.0,
            &|p| p.timeline.video_tracks[0].volume = 0.9,
            &|p| p.timeline.video_tracks[0].composite = Composite::Screen,
            &|p| p.timeline.video_tracks[0].clips[0].pan_crop = Keyframes { keys: vec![Keyframe { t: 0.0, v: PanCrop { w: 0.5, h: 0.5, ..Default::default() }, curve: Curve::Linear }] },
            &|p| p.timeline.video_tracks[0].motion = Keyframes { keys: vec![Keyframe { t: 0.0, v: Motion { x: 0.1, ..Default::default() }, curve: Curve::Linear }] },
        ];
        for (i, edit) in edits.iter().enumerate() {
            let mut q = p.clone();
            edit(&mut q);
            assert_ne!(key(&q), k, "edit {i}");
        }
        p.timeline.video_tracks[0].clips[0].name = "renamed".into(); // does not change the picture
        assert_eq!(key(&p), k);
    }

    #[test]
    fn proxies_are_used_at_draft_and_preview_only() {
        let mut p = project();
        let (mut v, mut img) = (media("pv", MediaType::Video), media("pi", MediaType::Image));
        (v.path, img.path) = ("proxied-video.bin".into(), "proxied-image.bin".into());
        p.timeline.video_tracks[0].clips[0].media_id = Some("pv".into());
        p.timeline.video_tracks[1].clips[1].media_id = Some("pi".into());
        p.media_pool.extend([v, img]);
        let before = frame_plan(&compile(&p), 345).key();
        crate::proxy::set_ready("proxied-video.bin".into(), "pv.proxy.mp4".into());
        crate::proxy::set_ready("proxied-image.bin".into(), "pi.proxy.mp4".into());
        let vp = compile(&p);
        let refs: Vec<&MediaRef> = vp.tracks.iter().flat_map(|t| &t.events).map(|e| &*e.m).collect();
        let r = |id: &str| *refs.iter().find(|m| &*m.id == id).unwrap();
        assert_eq!(r("pv").proxy_for(Quality::Draft), Some(Path::new("pv.proxy.mp4")));
        assert!(r("pv").proxy_for(Quality::Preview).is_some());
        assert!(r("pv").proxy_for(Quality::Good).is_none() && r("pv").proxy_for(Quality::Best).is_none());
        assert!(r("pi").proxy.is_none(), "stills are never proxied");
        assert!(r("b").proxy.is_none());
        // A frame that now decodes from the proxy is a different cached picture.
        assert_ne!(frame_plan(&vp, 345).key(), before);
    }

    #[test]
    fn fx_chains_and_generated_media() {
        let mut p = project();
        p.timeline.video_tracks[1].visible = false;
        let fx = |id: &str, enabled| Fx { enabled, ..plugins::new_fx(plugins::find(id).unwrap(), None) };
        let mut blur = fx("milan.fx.gaussian_blur", true);
        let ramp = vec![Keyframe { t: 0.0, v: 0.0, curve: Curve::Linear }, Keyframe { t: 2.0, v: 0.02, curve: Curve::Linear }];
        blur.params.insert("h_range".into(), Keyframes { keys: ramp });
        let unknown = Fx { plugin: "ofx:someone.else".into(), ..fx("milan.fx.invert", true) };
        p.timeline.video_tracks[0].clips[0].fx = vec![blur, fx("milan.fx.invert", false), unknown];
        p.timeline.video_tracks[0].fx = vec![fx("milan.fx.sepia", true)];
        p.timeline.master.video_fx = vec![fx("milan.fx.vignette", true)];
        let fp = frame_plan(&compile(&p), 330); // 11 s: 1 s into event a
        let Item::Event(e) = &fp.layers[0].items[0] else { panic!("{fp:?}") };
        assert_eq!(e.fx.len(), 1, "disabled and unknown plug-ins are bypassed");
        assert!((e.fx[0].f("h_range") - 0.01).abs() < 1e-6, "event FX keys are event-relative");
        assert_eq!((fp.layers[0].fx[0].id, fp.out_fx[0].id), ("milan.fx.sepia", "milan.fx.vignette"));

        // Generated media draws at the project size; one whose generator is unknown draws nothing.
        let mut g = MediaItem { id: "g".into(), media_type: MediaType::Image, duration: 86400.0, generator: Some(fx("milan.gen.solid", true)), ..Default::default() };
        p.media_pool.push(g.clone());
        p.timeline.video_tracks[0].clips.push(clip(&g, 20.0, 5.0, 0.0));
        (g.id, g.generator) = ("u".into(), Some(Fx { plugin: "milan.gen.future".into(), ..fx("milan.gen.solid", true) }));
        p.media_pool.push(g.clone());
        p.timeline.video_tracks[0].clips.push(clip(&g, 30.0, 5.0, 0.0));
        let vp = compile(&p);
        let fp = frame_plan(&vp, 660);
        let Item::Event(e) = &fp.layers[0].items[0] else { panic!("{fp:?}") };
        assert_eq!((&*e.src.m.id, e.src.m.w, e.src.m.h, e.src.sf), ("g", 1920, 1080, None));
        let (ev, t) = e.src.generator.as_ref().unwrap();
        assert_eq!((ev.id, *t), ("milan.gen.solid", 0.0)); // static: always time 0, so it renders once
        assert!(frame_plan(&vp, 960).layers.is_empty());
    }

    #[test]
    fn upcoming_lists_event_starts() {
        let p = project();
        let vp = compile(&p);
        assert_eq!(upcoming(&vp, 0, 300), [300]); // a at 10 s; b at 0 is not after n0
        assert_eq!(upcoming(&vp, 300, 1000), [900]);
    }
}
