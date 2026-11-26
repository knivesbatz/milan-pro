//! R28/R29: the flat cut list that EDL, FCP7 XML and FCPXML share. Each of those formats carries tracks of events
//! with a source file, timing, speed, fades, transitions, a static level and markers, and little else; `from_project`
//! turns a Milan project into that (listing what it leaves behind, in the house report style) and `to_project` builds
//! a Milan project back from it, probing the media it names the way the VEGAS import does. OTIO has its own, lossless
//! route (`otio.rs`).
use std::collections::HashMap;
use crate::edit;
use crate::engine::plugins;
use crate::project::*;

/// Milan's crossfade and the dissolve every format has.
pub const DISSOLVE: &str = "milan.tr.dissolve";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Cut {
    pub name: String,
    pub fps: f64,
    pub width: u32,
    pub height: u32,
    pub sample_rate: u32,
    /// Video tracks first (bottom to top in Milan's order), then audio.
    pub tracks: Vec<CutTrack>,
    pub markers: Vec<CutMarker>,
    /// Where the timeline's zero sits in the file's record timecode (01:00:00:00 in most EDLs and sequences).
    pub start_tc: f64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CutMarker {
    pub at: f64,
    /// A region when set.
    pub end: Option<f64>,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CutTrack {
    pub video: bool,
    pub name: String,
    pub muted: bool,
    pub locked: bool,
    pub clips: Vec<CutClip>,
}

/// One event. Timeline times in seconds from the timeline's zero; the source offset in the file's own seconds at
/// normal speed (the file's start timecode not included).
#[derive(Debug, Clone, PartialEq)]
pub struct CutClip {
    /// The media file; empty for media the format cannot name (an offline event is made, named after `name`).
    pub path: String,
    pub name: String,
    pub start: f64,
    pub end: f64,
    pub src_in: f64,
    /// `src_in` is a source timecode (an EDL's): the file's start timecode is taken off once the probe knows it.
    pub src_tc: bool,
    /// Source seconds per timeline second; `reverse` plays the span backwards.
    pub rate: f64,
    pub reverse: bool,
    /// The transition into this event. The previous event on the track overlaps it over the transition's length,
    /// as in Milan (and FCP7 XML); the writers of formats that abut events (EDL, FCPXML) cut the previous event short.
    pub transition: Option<CutTransition>,
    pub fade_in: f64,
    pub fade_out: f64,
    pub gain_db: f32,
    pub opacity: f32,
    pub muted: bool,
    pub group: Option<String>,
    /// What the writer knows about the file (the reader fills these from the probe).
    pub media: CutMedia,
}

impl Default for CutClip {
    fn default() -> Self {
        Self {
            path: String::new(),
            name: String::new(),
            start: 0.0,
            end: 0.0,
            src_in: 0.0,
            src_tc: false,
            rate: 1.0,
            reverse: false,
            transition: None,
            fade_in: 0.0,
            fade_out: 0.0,
            gain_db: 0.0,
            opacity: 1.0,
            muted: false,
            group: None,
            media: CutMedia::default(),
        }
    }
}

impl CutClip {
    pub fn duration(&self) -> f64 {
        (self.end - self.start).max(0.0)
    }

    /// Source seconds the event plays.
    pub fn src_span(&self) -> f64 {
        self.duration() * self.rate
    }

    pub fn src_out(&self) -> f64 {
        self.src_in + self.src_span()
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CutMedia {
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub frame_rate: f64,
    pub sample_rate: u32,
    /// The file's start timecode in seconds (0 when it has none).
    pub timecode: f64,
    pub has_video: bool,
    pub has_audio: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CutTransition {
    /// A Milan transition id (`DISSOLVE` for the plain crossfade).
    pub plugin: String,
    pub duration: f64,
}

impl Cut {
    /// Every media path once, for `probe_all`.
    pub fn media_paths(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for c in self.tracks.iter().flat_map(|t| &t.clips).filter(|c| !c.path.is_empty()) {
            if !out.contains(&c.path) {
                out.push(c.path.clone());
            }
        }
        out
    }
}

/// The display name of a plug-in id ("Gaussian Blur"; an OFX placeholder keeps its id).
pub fn plugin_name(id: &str) -> String {
    plugins::find(id).map_or_else(|| id.to_string(), |p| p.name.to_string())
}

fn names(fx: &[Fx]) -> String {
    fx.iter().map(|f| plugin_name(&f.plugin)).collect::<Vec<_>>().join(", ")
}

/// A project as a cut list, with what it leaves behind (`format` names the target: "an EDL").
pub fn from_project(p: &Project, format: &str) -> (Cut, Vec<String>) {
    let mut notes: Vec<String> = Vec::new();
    let s = &p.settings;
    let mut cut = Cut { name: p.name.clone(), fps: s.frame_rate, width: s.width, height: s.height, sample_rate: s.sample_rate, ..Default::default() };
    let media = |id: &str| p.media_pool.iter().find(|m| m.id == id);
    for (video, list) in [(true, &p.timeline.video_tracks), (false, &p.timeline.audio_tracks)] {
        for tr in list {
            let at = tr.name.clone();
            if !tr.fx.is_empty() {
                notes.push(format!("{at}: track FX {} are not in {format}", names(&tr.fx)));
            }
            if !tr.envelopes.is_empty() {
                notes.push(format!("{at}: track envelopes are not in {format}"));
            }
            if video {
                if !tr.motion.is_empty() || !tr.parent_motion.is_empty() {
                    notes.push(format!("{at}: Track Motion is not in {format}"));
                }
                if tr.composite != Composite::SourceAlpha {
                    notes.push(format!("{at}: the {:?} compositing mode is not in {format}", tr.composite));
                }
                if tr.child {
                    notes.push(format!("{at}: parent/child compositing is not in {format}; the track stands alone"));
                }
                if tr.adjustment {
                    notes.push(format!("{at}: an adjustment track is not in {format}"));
                }
            }
            let mut t = CutTrack { video, name: tr.name.clone(), muted: tr.muted, locked: tr.locked, clips: Vec::new() };
            let mut clips: Vec<&Clip> = tr.clips.iter().collect();
            clips.sort_by(|a, b| a.position.total_cmp(&b.position));
            let mut prev_end: Option<f64> = None;
            for c in clips {
                let where_ = format!("{at}: the event at {:.3} s", c.position);
                let Some(m) = c.media_id.as_deref().and_then(media) else {
                    notes.push(format!("{where_} has no media (an adjustment or empty event) and was left out"));
                    continue;
                };
                if m.generator.is_some() {
                    notes.push(format!("{where_}: generated media ({}) is not in {format}; the event was left out", m.name));
                    continue;
                }
                if m.nested {
                    notes.push(format!("{where_}: the nested timeline {} goes out as a plain file reference", m.name));
                }
                let mut e = CutClip {
                    path: m.path.display().to_string(),
                    name: c.name.clone(),
                    start: c.position,
                    end: c.position + c.duration,
                    src_in: c.in_point + m.subclip.as_ref().map_or(0.0, |s| s.start),
                    src_tc: false,
                    rate: c.rate,
                    reverse: c.reverse,
                    fade_in: c.fade_in,
                    fade_out: c.fade_out,
                    gain_db: if video { 0.0 } else { c.gain_db },
                    opacity: if video { c.opacity() } else { 1.0 },
                    muted: c.muted,
                    group: c.group.clone(),
                    media: CutMedia {
                        duration: m.duration + m.subclip.as_ref().map_or(0.0, |s| s.start),
                        width: m.width.unwrap_or(0),
                        height: m.height.unwrap_or(0),
                        frame_rate: m.frame_rate.unwrap_or(0.0),
                        sample_rate: m.sample_rate.unwrap_or(0),
                        timecode: m.timecode.unwrap_or(0.0),
                        has_video: m.media_type != MediaType::Audio,
                        has_audio: m.media_type != MediaType::Image && (m.audio_streams > 0 || m.media_type == MediaType::Audio),
                    },
                    transition: None,
                };
                // An overlap with the previous event is a transition (the incoming event's, or the automatic crossfade).
                if let Some(pe) = prev_end.filter(|pe| *pe > c.position + 1e-9) {
                    let overlap = (pe - c.position).min(c.duration);
                    let plugin = match &c.transition {
                        Some(t) if t.plugin == DISSOLVE || t.plugin.is_empty() => DISSOLVE.to_string(),
                        Some(t) => {
                            notes.push(format!("{where_}: the {} transition goes out as a dissolve", plugin_name(&t.plugin)));
                            t.plugin.clone()
                        }
                        None => DISSOLVE.to_string(),
                    };
                    e.transition = Some(CutTransition { plugin, duration: overlap });
                    e.fade_in = 0.0;
                } else if c.transition.is_some() {
                    notes.push(format!("{where_}: its transition has no overlapping event and goes out as a fade"));
                }
                if c.transition_out.is_some() {
                    notes.push(format!("{where_}: the transition on its fade-out goes out as a plain fade"));
                }
                if !c.fx.is_empty() {
                    notes.push(format!("{where_}: event FX {} are not in {format}", names(&c.fx)));
                }
                let dynamic = c.envelopes.iter().filter(|e| !(e.kind == EnvKind::Opacity && e.points.keys.len() <= 1)).map(|e| format!("{:?}", e.kind)).collect::<Vec<_>>();
                if !dynamic.is_empty() {
                    notes.push(format!("{where_}: the {} envelope(s) are not in {format}", dynamic.join(", ")));
                }
                if !c.pan_crop.is_empty() || !c.masks.is_empty() {
                    notes.push(format!("{where_}: Pan/Crop is not in {format}"));
                }
                if !c.takes.is_empty() {
                    notes.push(format!("{where_}: only the active take goes out ({} other take(s))", c.takes.len()));
                }
                if c.pitch_cents != 0.0 {
                    notes.push(format!("{where_}: the pitch change is not in {format}"));
                }
                prev_end = Some(e.end.max(prev_end.unwrap_or(0.0)));
                t.clips.push(e);
            }
            cut.tracks.push(t);
        }
    }
    let tl = &p.timeline;
    if !tl.master.video_fx.is_empty() || !tl.master.audio_fx.is_empty() {
        notes.push(format!("Video output and master bus FX are not in {format}"));
    }
    cut.markers = tl.markers.iter().map(|m| CutMarker { at: m.position, end: None, name: m.name.clone() }).collect();
    cut.markers.extend(tl.regions.iter().map(|r| CutMarker { at: r.start, end: Some(r.end), name: r.name.clone() }));
    cut.markers.sort_by(|a, b| a.at.total_cmp(&b.at));
    (cut, notes)
}

const MARKER_COLOR: [u8; 3] = [0xDB, 0x8E, 0x57];

/// A Milan project from a cut list; `probed` holds each media path's probe (`probe_all`). Timeline times snap to
/// the frame grid. Returns the project and what did not carry over.
pub fn to_project(cut: &Cut, probed: &HashMap<String, Result<MediaItem, String>>) -> (Project, Vec<String>) {
    let mut warnings: Vec<String> = Vec::new();
    let mut p = Project::new(if cut.name.is_empty() { "Imported Sequence" } else { &cut.name });
    let d = ProjectSettings::default();
    let fps = if cut.fps > 0.0 { (cut.fps * 1000.0).round() / 1000.0 } else { d.frame_rate };
    p.settings = ProjectSettings {
        width: if cut.width > 0 { cut.width } else { d.width },
        height: if cut.height > 0 { cut.height } else { d.height },
        frame_rate: fps,
        sample_rate: if cut.sample_rate > 0 { cut.sample_rate } else { d.sample_rate },
        ..d
    };
    let snap = |t: f64| edit::quantize(t.max(0.0), fps);
    (p.timeline.video_tracks, p.timeline.audio_tracks) = (Vec::new(), Vec::new());
    let mut groups: HashMap<&str, String> = HashMap::new();
    for t in &cut.tracks {
        let (kind, word) = if t.video { (TrackKind::Video, "Video") } else { (TrackKind::Audio, "Audio") };
        let name = if t.name.is_empty() { format!("{word} {}", p.timeline.tracks(t.video).len() + 1) } else { t.name.clone() };
        let mut tr = Track::new(&name, kind);
        (tr.muted, tr.locked) = (t.muted, t.locked);
        let mut prev_end: Option<f64> = None;
        for e in &t.clips {
            let at = format!("{name}: the event at {:.3} s", e.start);
            let start = snap(e.start);
            let duration = (snap(e.end) - start).max(1.0 / fps);
            let id = if e.path.is_empty() {
                // A reel with no file behind it: an offline item named after the event, so the cut is still there.
                let reel = if e.name.is_empty() { "Unknown reel".to_string() } else { e.name.clone() };
                match p.media_pool.iter().find(|m| m.name == reel && m.path.as_os_str().is_empty() && m.generator.is_none()) {
                    Some(m) => m.id.clone(),
                    None => {
                        warnings.push(format!("{at}: no file is named for reel {reel}; the event is offline"));
                        let m = MediaItem {
                            id: uuid::Uuid::new_v4().to_string(),
                            name: reel,
                            media_type: if t.video { MediaType::Video } else { MediaType::Audio },
                            duration: e.media.duration,
                            ..Default::default()
                        };
                        let id = m.id.clone();
                        p.media_pool.push(m);
                        id
                    }
                }
            } else {
                super::vegas::media_id(&mut p.media_pool, probed, &e.path, t.video, &mut warnings)
            };
            let m = p.media_pool.iter().find(|m| m.id == id).expect("just added");
            let mut c = Clip::from_media(m, start);
            c.duration = duration;
            c.rate = if e.rate > 0.0 { e.rate } else { 1.0 };
            c.reverse = e.reverse;
            c.in_point = e.src_in;
            if e.src_tc {
                // A source timecode: the file's own start timecode comes off; files without one usually count from
                // 00:00:00:00 or 01:00:00:00 in an EDL, so whole hours past the file's end come off too.
                c.in_point -= m.timecode.unwrap_or(0.0);
                if m.timecode.is_none() && m.duration > 0.0 && c.in_point >= m.duration {
                    c.in_point -= (c.in_point / 3600.0).floor() * 3600.0;
                }
            }
            c.in_point = c.in_point.max(0.0);
            if m.duration > 0.0 && c.in_point >= m.duration && !m.path.as_os_str().is_empty() {
                warnings.push(format!("{at}: its source offset {:.3} s is past the end of {}; the event starts at the file's start", c.in_point, m.name));
                c.in_point = 0.0;
            }
            c.out_point = c.in_point + c.duration * c.rate;
            if !e.name.is_empty() {
                c.name = e.name.clone();
            }
            (c.fade_in, c.fade_out) = (snap(e.fade_in), snap(e.fade_out));
            c.clamp_fades();
            c.muted = e.muted;
            if t.video {
                c.set_opacity(e.opacity.clamp(0.0, 1.0));
            } else {
                c.gain_db = e.gain_db;
            }
            c.group = e.group.as_deref().map(|g| groups.entry(g).or_insert_with(|| uuid::Uuid::new_v4().to_string()).clone());
            if let Some(tn) = &e.transition {
                // A transition needs the previous event under it; without one it is the fade it looks like.
                if prev_end.is_some_and(|pe| pe > start + 1e-9) {
                    let plugin = if plugins::find(&tn.plugin).is_some() { tn.plugin.clone() } else { DISSOLVE.to_string() };
                    if plugin != tn.plugin {
                        warnings.push(format!("{at}: the {} transition is not available; a crossfade plays", tn.plugin));
                    }
                    c.transition = Some(Transition { plugin, ..Default::default() });
                } else {
                    c.fade_in = snap(tn.duration).min(c.duration);
                }
            }
            prev_end = Some((start + duration).max(prev_end.unwrap_or(0.0)));
            tr.clips.push(c);
        }
        p.timeline.tracks_mut(t.video).push(tr);
    }
    if p.timeline.video_tracks.is_empty() {
        p.timeline.video_tracks.push(Track::new("Video 1", TrackKind::Video));
    }
    if p.timeline.audio_tracks.is_empty() {
        p.timeline.audio_tracks.push(Track::new("Audio 1", TrackKind::Audio));
    }
    for m in &cut.markers {
        match m.end {
            Some(end) if end > m.at => p.timeline.regions.push(Region { start: snap(m.at), end: snap(end), name: m.name.clone() }),
            _ => p.timeline.markers.push(Marker { position: snap(m.at), name: m.name.clone(), color: MARKER_COLOR }),
        }
    }
    p.modified = true;
    (p, warnings)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::engine::plugins::{find, new_fx};

    /// A project with the parts a cut list carries and a few it does not.
    pub(crate) fn sample() -> Project {
        let mut p = Project::new("Cut Sample");
        p.settings = ProjectSettings { width: 1280, height: 720, frame_rate: 25.0, sample_rate: 48000, ..Default::default() };
        let item = |name: &str, t, w: Option<u32>| MediaItem { id: name.into(), path: format!(r"C:\m\{name}").into(), name: name.into(), media_type: t, duration: 60.0, width: w, height: w.map(|_| 720), sample_rate: Some(48000), frame_rate: w.map(|_| 25.0), audio_streams: 1, video_streams: w.map_or(0, |_| 1), ..Default::default() };
        p.media_pool = vec![item("a.mp4", MediaType::Video, Some(1280)), item("b.mp4", MediaType::Video, Some(1280)), item("t.wav", MediaType::Audio, None)];
        let mut v = Track::new("Video 1", TrackKind::Video);
        let mut a = Clip::from_media(&p.media_pool[0], 0.0);
        (a.duration, a.in_point, a.out_point, a.fade_in) = (5.0, 2.0, 7.0, 1.0);
        a.set_opacity(0.5);
        let mut b = Clip::from_media(&p.media_pool[1], 4.0);
        (b.duration, b.in_point, b.rate, b.out_point) = (4.0, 10.0, 2.0, 18.0);
        b.transition = Some(Transition { plugin: DISSOLVE.into(), ..Default::default() });
        b.fx.push(new_fx(find("milan.fx.gaussian_blur").unwrap(), None));
        let mut r = Clip::from_media(&p.media_pool[0], 10.0);
        (r.duration, r.in_point, r.out_point, r.reverse) = (2.0, 20.0, 22.0, true);
        v.clips = vec![a, b, r];
        let mut au = Track::new("Audio 1", TrackKind::Audio);
        let mut s = Clip::from_media(&p.media_pool[2], 0.0);
        (s.duration, s.out_point, s.gain_db, s.fade_out) = (8.0, 8.0, -6.0, 0.6);
        au.clips = vec![s];
        p.timeline.video_tracks = vec![v];
        p.timeline.audio_tracks = vec![au];
        p.timeline.markers.push(Marker { position: 3.0, name: "Intro".into(), color: [1, 2, 3] });
        p.timeline.regions.push(Region { start: 4.0, end: 6.0, name: "Mix".into() });
        p
    }

    pub(crate) fn probed(p: &Project) -> HashMap<String, Result<MediaItem, String>> {
        p.media_pool.iter().filter(|m| m.generator.is_none()).map(|m| (m.path.display().to_string(), Ok(m.clone()))).collect()
    }

    #[test]
    fn project_to_cut_and_back() {
        let p = sample();
        let (cut, notes) = from_project(&p, "an EDL");
        assert_eq!(cut.tracks.len(), 2);
        let v = &cut.tracks[0];
        assert_eq!((v.clips[1].transition.as_ref().unwrap().duration, v.clips[1].rate, v.clips[2].reverse), (1.0, 2.0, true));
        assert!((v.clips[0].opacity - 0.5).abs() < 1e-6 && v.clips[0].fade_in == 1.0);
        assert!(notes.iter().any(|n| n.contains("Gaussian Blur")), "{notes:?}");
        assert_eq!(cut.markers.len(), 2);
        let (q, w) = to_project(&cut, &probed(&p));
        assert!(w.is_empty(), "{w:?}");
        let qv = &q.timeline.video_tracks[0];
        assert_eq!(qv.clips.len(), 3);
        assert_eq!((qv.clips[0].position, qv.clips[0].duration, qv.clips[0].in_point, qv.clips[0].fade_in, qv.clips[0].opacity()), (0.0, 5.0, 2.0, 1.0, 0.5));
        assert_eq!((qv.clips[1].position, qv.clips[1].in_point, qv.clips[1].rate, qv.clips[1].out_point), (4.0, 10.0, 2.0, 18.0));
        assert_eq!(qv.clips[1].transition.as_ref().map(|t| t.plugin.as_str()), Some(DISSOLVE));
        assert!(qv.clips[2].reverse);
        let qa = &q.timeline.audio_tracks[0].clips[0];
        assert_eq!((qa.gain_db, qa.fade_out), (-6.0, 0.6));
        assert_eq!((q.timeline.markers.len(), q.timeline.regions.len()), (1, 1));
        assert_eq!(q.settings.frame_rate, 25.0);
    }

    #[test]
    fn a_transition_without_an_overlap_becomes_a_fade() {
        let cut = Cut {
            fps: 25.0,
            tracks: vec![CutTrack { video: true, clips: vec![CutClip { start: 0.0, end: 4.0, name: "R1".into(), transition: Some(CutTransition { plugin: DISSOLVE.into(), duration: 1.0 }), ..Default::default() }], ..Default::default() }],
            ..Default::default()
        };
        let (p, w) = to_project(&cut, &HashMap::new());
        let c = &p.timeline.video_tracks[0].clips[0];
        assert_eq!((c.fade_in, c.transition.is_none()), (1.0, true));
        assert!(w[0].contains("reel R1"), "{w:?}");
    }
}
