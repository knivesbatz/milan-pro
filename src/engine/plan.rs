//! Timeline -> video and audio plans (pure). The only code that interprets the timeline for preview and playback:
//! `compile` once per edit, `frame_plan` once per frame, resolving everything to plain numbers;
//! `compile_audio` once per edit for the mixer.
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;
use crate::edit::src_time;
use crate::engine::composite::Affine;
use crate::project::{Clip, Composite, Curve, EnvKind, Envelope, Keyframes, MediaItem, MediaType, Motion, PanCrop, Project};

/// A picture source as the decoders need it.
#[derive(Debug)]
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
}

#[derive(Debug)]
pub struct VideoPlan {
    pub w: u32,
    pub h: u32,
    pub fps: f64,
    pub fps_str: String,
    /// Bottom track first.
    pub tracks: Vec<VTrack>,
    /// Content end (seconds).
    pub end: f64,
}

#[derive(Debug)]
pub struct VTrack {
    composite: Composite,
    level: f32,
    level_env: Option<Envelope>,
    motion: Keyframes<Motion>,
    /// Stable-sorted by position: on ties the later Vec entry is on top.
    events: Vec<VEvent>,
}

#[derive(Debug)]
struct VEvent {
    c: Clip,
    m: Arc<MediaRef>,
}

/// One frame, resolved: tracks bottom to top.
#[derive(Debug)]
pub struct FramePlan {
    pub layers: Vec<TrackPlan>,
}

#[derive(Debug)]
pub struct TrackPlan {
    pub mode: Composite,
    pub opacity: f32,
    /// Track Motion in normalised frame coordinates.
    pub motion: Affine,
    pub items: Vec<Item>,
}

#[derive(Debug)]
pub enum Item {
    Event(EventPlan),
    /// A transition from `a` (None = transparent) into `b` at progress `p`.
    Mix { a: Option<EventPlan>, b: EventPlan, p: f32 },
}

#[derive(Debug)]
pub struct EventPlan {
    pub src: SrcAt,
    /// Normalised source (0..1) to normalised frame (0..1): Pan/Crop, then fit to the project frame.
    pub place: Affine,
    pub alpha: f32,
}

#[derive(Debug)]
pub struct SrcAt {
    pub m: Arc<MediaRef>,
    /// Source frame on the project-fps grid; None for a still.
    pub sf: Option<i64>,
}

impl FramePlan {
    /// Identity of the composited picture: equal keys give equal frames.
    // shortcut: hashes the Debug text (shortest round-trip floats, so every value counts); a few µs per frame.
    pub fn key(&self) -> u64 {
        let mut h = DefaultHasher::new();
        format!("{self:?}").hash(&mut h);
        h.finish()
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

fn media_ref(m: &MediaItem) -> Option<MediaRef> {
    // shortcut: generated media draws nothing until batch 14 registers generators.
    if m.generator.is_some() || !matches!(m.media_type, MediaType::Video | MediaType::Image) {
        return None;
    }
    let (w, h) = match (m.width, m.height) {
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
        still: m.media_type == MediaType::Image,
        has_alpha: m.has_alpha,
        matrix: matrix(m),
        codec: m.codec.clone(),
        interlaced: m.interlaced,
    })
}

pub fn compile(p: &Project) -> VideoPlan {
    let s = &p.settings;
    let refs: HashMap<&str, Arc<MediaRef>> = p.media_pool.iter().filter_map(|m| Some((m.id.as_str(), Arc::new(media_ref(m)?)))).collect();
    let tracks = p
        .timeline
        .shown_video_tracks()
        .rev()
        .map(|t| {
            let mut events: Vec<VEvent> = t
                .clips
                .iter()
                .filter(|c| !c.muted && c.duration > 0.0)
                .filter_map(|c| Some(VEvent { m: refs.get(c.media_id.as_deref()?)?.clone(), c: c.clone() }))
                .collect();
            events.sort_by(|a, b| a.c.position.total_cmp(&b.c.position));
            VTrack {
                composite: t.composite,
                level: t.volume.clamp(0.0, 1.0),
                level_env: t.envelopes.iter().find(|e| e.kind == EnvKind::CompositeLevel).cloned(),
                motion: t.motion.clone(),
                events,
            }
        })
        .collect();
    let fps = s.frame_rate.max(1.0);
    VideoPlan {
        w: s.width.max(2),
        h: s.height.max(2),
        fps,
        fps_str: crate::render::fps_str(fps),
        tracks,
        end: crate::render::content_end(p),
    }
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

/// Pan/Crop: the crop rectangle (normalised to the source, negative size = flip, `rot` degrees) fitted into the frame.
pub fn place(pc: PanCrop, (sw, sh): (f64, f64), (w, h): (f64, f64)) -> Affine {
    let (cw, ch) = ((pc.w.abs() as f64 * sw).max(1e-6), (pc.h.abs() as f64 * sh).max(1e-6));
    let s = (w / cw).min(h / ch);
    Affine::scale(sw, sh)
        .then(Affine::translate(-pc.cx as f64 * sw, -pc.cy as f64 * sh))
        // The crop frame turns one way, so the picture turns the other.
        .then(Affine::rotate(-pc.rot as f64))
        .then(Affine::scale(s * pc.w.signum() as f64, s * pc.h.signum() as f64))
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

pub fn frame_plan(p: &VideoPlan, n: i64) -> FramePlan {
    let t = n as f64 / p.fps;
    let te = t + 1e-6; // positions snapped to frames sit on, not after, the frame time
    let size = (p.w as f64, p.h as f64);
    let layers = p
        .tracks
        .iter()
        .filter_map(|tr| {
            let opacity = tr.level * tr.level_env.as_ref().map_or(1.0, |e| e.at(t).clamp(0.0, 1.0));
            let live: Vec<&VEvent> = tr.events.iter().filter(|e| e.c.position <= te && te < e.c.position + e.c.duration).collect();
            if opacity <= 0.0 || live.is_empty() {
                return None;
            }
            let mut items: Vec<Item> = Vec::new();
            for (i, e) in live.iter().enumerate() {
                let c = &e.c;
                let local = t - c.position;
                // Decision 0.9: an event overlapped by a later one stays at full alpha (a dissolve, not a dip).
                let fade_out = if i + 1 < live.len() { 1.0 } else { fade(c.fade_out_curve, c.duration - local, c.fade_out) };
                let alpha = fade_out * env_at(&c.envelopes, EnvKind::Opacity, local);
                // Reversed, a frame samples where it ends (its start, mirrored), so it shows exactly the forward frames backwards.
                let mut st = src_time(c, if c.reverse { t + 1.0 / p.fps } else { t });
                if e.m.duration > 0.0 {
                    st = st.clamp(0.0, (e.m.duration - 0.5 / p.fps).max(0.0));
                }
                let sf = (!e.m.still).then(|| (st * p.fps).round() as i64);
                let plan = |alpha| EventPlan {
                    src: SrcAt { m: e.m.clone(), sf },
                    place: place(c.pan_crop.at(local), (e.m.w as f64, e.m.h as f64), size),
                    alpha,
                };
                match &c.transition {
                    Some(_) if i > 0 => {
                        let prev = &live[i - 1].c;
                        let len = (prev.position + prev.duration).min(c.position + c.duration) - c.position;
                        let a = match items.pop() {
                            Some(Item::Event(a)) => Some(a),
                            other => {
                                items.extend(other);
                                None
                            }
                        };
                        items.push(Item::Mix { a, b: plan(alpha), p: (local / len.max(1e-9)).clamp(0.0, 1.0) as f32 });
                    }
                    Some(_) if local < c.fade_in => items.push(Item::Mix { a: None, b: plan(alpha), p: (local / c.fade_in) as f32 }),
                    _ => items.push(Item::Event(plan(alpha * fade(c.fade_in_curve, local, c.fade_in)))),
                }
            }
            Some(TrackPlan { mode: tr.composite, opacity, motion: motion(tr.motion.at(t), size), items })
        })
        .collect();
    FramePlan { layers }
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
}

#[derive(Debug)]
pub struct AEvent {
    pub src: ASrc,
    /// Event gain, linear.
    pub gain: f32,
    pub fade_in: f64,
    pub fade_out: f64,
    pub fade_in_curve: Curve,
    pub fade_out_curve: Curve,
    /// Event volume envelope (dB), event-relative.
    pub vol_env: Option<Envelope>,
}

#[derive(Debug)]
pub struct ATrack {
    /// Static Volume (linear) and Pan (-1 left .. 1 right). With a pan envelope, Pan is added to it as a trim, as in VEGAS.
    pub volume: f32,
    pub pan: f32,
    pub vol_env: Option<Envelope>,
    pub pan_env: Option<Envelope>,
    pub mute_env: Option<Envelope>,
    pub events: Vec<AEvent>,
}

#[derive(Debug)]
pub struct AudioPlan {
    /// Audible tracks only (muted and un-soloed tracks are left out).
    pub tracks: Vec<ATrack>,
}

pub fn compile_audio(p: &Project) -> AudioPlan {
    let media: HashMap<&str, &MediaItem> = p.media_pool.iter().filter(|m| m.sample_rate.is_some() && m.generator.is_none()).map(|m| (m.id.as_str(), m)).collect();
    let env = |envs: &[Envelope], kind| envs.iter().find(|e| e.kind == kind && !e.points.keys.is_empty()).cloned();
    let any_solo = p.timeline.audio_tracks.iter().any(|t| t.solo);
    let tracks = p
        .timeline
        .audio_tracks
        .iter()
        .filter_map(|t| {
            crate::audio::track_gain(t, any_solo)?; // muted or un-soloed: left out
            let events: Vec<AEvent> = t
                .clips
                .iter()
                .filter(|c| !c.muted && c.duration > 0.0 && c.rate > 0.0)
                .filter_map(|c| {
                    let m = media.get(c.media_id.as_deref()?)?;
                    Some(AEvent {
                        src: ASrc { path: m.path.clone(), pos: c.position, dur: c.duration, in_point: c.in_point, rate: c.rate, reverse: c.reverse },
                        gain: crate::engine::keyframes::db_to_gain(c.gain_db),
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
                volume: t.volume,
                pan: t.pan.clamp(-1.0, 1.0),
                vol_env: env(e, EnvKind::Volume),
                pan_env: env(e, EnvKind::Pan),
                mute_env: env(e, EnvKind::Mute),
                events,
            })
        })
        .collect();
    AudioPlan { tracks }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Keyframe, Track, TrackKind, Transition};

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
                Item::Mix { a, b, .. } => a.iter().chain([b]).map(ev).collect(),
            })
            .collect()
    }

    fn ids(p: &Project, t: f64) -> Vec<String> {
        drawn(p, t).into_iter().map(|d| d.0).collect()
    }

    fn project() -> Project {
        let mut p = Project::new("t");
        p.settings.frame_rate = 30.0;
        let (a, b, img) = (media("a", MediaType::Video), media("b", MediaType::Video), media("i", MediaType::Image));
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

        p.timeline.video_tracks[0].clips[1].transition = Some(Transition { plugin: "milan.tr.dissolve".into(), params: Default::default() });
        let vp = compile(&p);
        let mix_p = |n| match &frame_plan(&vp, n).layers[0].items[..] {
            [Item::Mix { a: Some(_), b, p }] => (*p, b.alpha),
            other => panic!("{other:?}"),
        };
        assert_eq!(mix_p(390), (0.0, 1.0)); // 13.0 s: start of the overlap
        let (end, _) = mix_p(449); // the last frame inside it
        assert!((end - 59.0 / 60.0).abs() < 1e-4);
        assert!(matches!(&frame_plan(&vp, 450).layers[0].items[..], [Item::Event(_)])); // 15.0 s: a has ended
        // A lone fade-in with a transition mixes from transparent.
        p.timeline.video_tracks[0].clips.remove(0);
        let vp = compile(&p);
        assert!(matches!(&frame_plan(&vp, 420).layers[0].items[..], [Item::Mix { a: None, p, .. }] if (*p - 0.5).abs() < 1e-6));
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
    fn upcoming_lists_event_starts() {
        let p = project();
        let vp = compile(&p);
        assert_eq!(upcoming(&vp, 0, 300), [300]); // a at 10 s; b at 0 is not after n0
        assert_eq!(upcoming(&vp, 300, 1000), [900]);
    }
}
