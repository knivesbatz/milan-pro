use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    #[serde(skip)]
    pub path: Option<PathBuf>,
    pub settings: ProjectSettings,
    pub timeline: Timeline,
    pub media_pool: Vec<MediaItem>,
    #[serde(skip)]
    pub modified: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectSettings {
    pub width: u32,
    pub height: u32,
    pub frame_rate: f64,
    pub sample_rate: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Timeline {
    pub video_tracks: Vec<Track>,
    pub audio_tracks: Vec<Track>,
    pub markers: Vec<Marker>,
    /// Loop region / time selection (start, end), shown in the loop bar.
    #[serde(default)]
    pub loop_region: Option<(f64, f64)>,
    /// Named time ranges (Insert Region, R), drawn as numbered tag pairs in the marker bar.
    #[serde(default)]
    pub regions: Vec<Region>,
    #[serde(skip)]
    pub cursor_position: f64,
    #[serde(default = "default_zoom")]
    #[serde(skip)]
    pub zoom: f64,
    #[serde(skip)]
    pub scroll_x: f64,
    #[serde(default)]
    pub master: MasterBus,
}

fn default_zoom() -> f64 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: String,
    pub name: String,
    pub kind: TrackKind,
    pub clips: Vec<Clip>,
    pub muted: bool,
    pub solo: bool,
    pub locked: bool,
    pub visible: bool,
    /// Pixels; 0 = the look's default for this track type.
    pub height: f32,
    pub volume: f32,
    pub pan: f32,
    /// How a video track composites over the tracks below.
    #[serde(default)]
    pub composite: Composite,
    /// Track Motion keys (video).
    #[serde(default)]
    pub motion: Keyframes<Motion>,
    /// Track FX chain (video or audio, by track kind).
    #[serde(default)]
    pub fx: Vec<Fx>,
    /// Track envelopes; `volume` stays the static Level/Volume and multiplies with them.
    #[serde(default)]
    pub envelopes: Vec<Envelope>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackKind {
    Video,
    Audio,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub id: String,
    pub media_id: Option<String>,
    pub position: f64,
    pub duration: f64,
    pub in_point: f64,
    pub out_point: f64,
    pub name: String,
    pub color: [u8; 3],
    /// Fade lengths in seconds (smooth S-curve), applied in preview audio and render.
    #[serde(default)]
    pub fade_in: f64,
    #[serde(default)]
    pub fade_out: f64,
    /// Locked events cannot be moved, trimmed, split or deleted (transport Lock button).
    #[serde(default)]
    pub locked: bool,
    /// Event gain (audio) in dB.
    #[serde(default)]
    pub gain_db: f32,
    #[serde(default)]
    pub fade_in_curve: Curve,
    #[serde(default)]
    pub fade_out_curve: Curve,
    /// Playback rate: source seconds per timeline second.
    #[serde(default = "one")]
    pub rate: f64,
    #[serde(default)]
    pub reverse: bool,
    /// Event Switches > Mute.
    #[serde(default)]
    pub muted: bool,
    /// Event Pan/Crop keys, event-relative.
    #[serde(default)]
    pub pan_crop: Keyframes<PanCrop>,
    /// Event FX chain.
    #[serde(default)]
    pub fx: Vec<Fx>,
    /// Transition from the overlapped event into this one (stored on the incoming event).
    #[serde(default)]
    pub transition: Option<Transition>,
    /// Event envelopes, event-relative.
    #[serde(default)]
    pub envelopes: Vec<Envelope>,
    /// Id shared by the events of one group.
    #[serde(default)]
    pub group: Option<String>,
    /// Inactive takes; the active take is the event's own media.
    #[serde(default)]
    pub takes: Vec<Take>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MediaItem {
    pub id: String,
    pub path: PathBuf,
    pub name: String,
    pub media_type: MediaType,
    pub duration: f64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub frame_rate: Option<f64>,
    pub sample_rate: Option<u32>,
    pub codec: Option<String>,
    /// The picture has an alpha channel (pix_fmt with alpha).
    #[serde(default)]
    pub has_alpha: bool,
    /// Display rotation in degrees, 0..360; width and height are already swapped at 90 and 270.
    #[serde(default)]
    pub rotation: i32,
    /// ffprobe `color_space` (bt709, bt470bg, ...) when the source is tagged.
    #[serde(default)]
    pub color_matrix: Option<String>,
    #[serde(default)]
    pub interlaced: bool,
    /// Generated media (solid colour, text, ...): an Image with an empty path, drawn by this plug-in.
    #[serde(default)]
    pub generator: Option<Fx>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaType {
    Video,
    Audio,
    Image,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Marker {
    pub position: f64,
    pub name: String,
    pub color: [u8; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Region {
    pub start: f64,
    pub end: f64,
    pub name: String,
}

/// Fade and keyframe curve (VEGAS's six). The shapes are in `engine::keyframes`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Curve {
    Linear,
    Fast,
    Slow,
    #[default]
    Smooth,
    Sharp,
    Hold,
}

impl Curve {
    fn linear() -> Self {
        Curve::Linear
    }
}

/// One key; `curve` shapes the segment after it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Keyframe<T> {
    pub t: f64,
    pub v: T,
    #[serde(default = "Curve::linear")]
    pub curve: Curve,
}

/// Keys sorted by time; no keys means the type's default value.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Keyframes<T> {
    pub keys: Vec<Keyframe<T>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnvKind {
    /// dB, floor -120.
    Volume,
    /// -1 (left) ..= 1 (right).
    Pan,
    /// 1 on, 0 muted; Hold keys.
    Mute,
    /// 0..=1.
    CompositeLevel,
    /// 0..=1.
    Opacity,
    /// Reserved for event velocity.
    Velocity,
    /// A kind written by a later version: loads (instead of failing the whole project) and is ignored.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub kind: EnvKind,
    #[serde(default)]
    pub points: Keyframes<f32>,
}

/// A plug-in instance: video FX, generator or audio FX.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fx {
    /// Unique per instance; keys the DSP state.
    pub id: String,
    pub plugin: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub params: BTreeMap<String, Keyframes<f32>>,
    #[serde(default)]
    pub text: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transition {
    pub plugin: String,
    #[serde(default)]
    pub params: BTreeMap<String, f32>,
}

/// Event Pan/Crop, normalised to the source; negative w/h flip.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PanCrop {
    pub cx: f32,
    pub cy: f32,
    pub w: f32,
    pub h: f32,
    pub rot: f32,
}

impl Default for PanCrop {
    fn default() -> Self {
        Self { cx: 0.5, cy: 0.5, w: 1.0, h: 1.0, rot: 0.0 }
    }
}

/// Track Motion, normalised to the project frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Motion {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub rot: f32,
}

impl Default for Motion {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0, w: 1.0, h: 1.0, rot: 0.0 }
    }
}

/// Video track compositing mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Composite {
    #[default]
    SourceAlpha,
    Add,
    Subtract,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    Difference,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Take {
    pub media_id: String,
    pub in_point: f64,
    pub name: String,
}

/// Master bus: L/R gain and the Audio Master / Video Output FX chains.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MasterBus {
    pub gain: [f32; 2],
    pub audio_fx: Vec<Fx>,
    pub video_fx: Vec<Fx>,
}

impl Default for MasterBus {
    fn default() -> Self {
        Self { gain: [1.0, 1.0], audio_fx: Vec::new(), video_fx: Vec::new() }
    }
}

fn one() -> f64 {
    1.0
}

fn yes() -> bool {
    true
}

impl Project {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            path: None,
            settings: ProjectSettings::default(),
            timeline: Timeline::new(),
            media_pool: Vec::new(),
            modified: false,
        }
    }

    /// Writes a temp file and renames it over `path`, so a failed save never damages the old file.
    /// On error, name, path and modified are left unchanged.
    pub fn save(&mut self, path: &PathBuf) -> anyhow::Result<()> {
        // The project is named after its file, so titles and autosave backups are per-project.
        let name = file_stem(path).unwrap_or_else(|| self.name.clone());
        let old_name = std::mem::replace(&mut self.name, name);
        let res = serde_json::to_string_pretty(self).map_err(anyhow::Error::from).and_then(|json| write_atomic(path, &json));
        match res {
            Ok(()) => {
                self.path = Some(path.clone());
                self.modified = false;
            }
            Err(_) => self.name = old_name,
        }
        res
    }

    pub fn load(path: &PathBuf) -> anyhow::Result<Self> {
        let json = std::fs::read_to_string(path)?;
        let mut project: Self = serde_json::from_str(&json)?;
        if let Some(stem) = file_stem(path) {
            project.name = stem;
        }
        project.path = Some(path.clone());
        project.timeline.zoom = 1.0;
        Ok(project)
    }

    pub fn mark_modified(&mut self) {
        self.modified = true;
    }
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            frame_rate: 29.97,
            sample_rate: 48000,
        }
    }
}

impl Timeline {
    pub fn new() -> Self {
        Self {
            video_tracks: vec![Track::new("Video 1", TrackKind::Video)],
            audio_tracks: vec![Track::new("Audio 1", TrackKind::Audio)],
            markers: Vec::new(),
            loop_region: None,
            regions: Vec::new(),
            cursor_position: 0.0,
            zoom: 1.0,
            scroll_x: 0.0,
            master: MasterBus::default(),
        }
    }

    pub fn duration(&self) -> f64 {
        let mut max = 30.0;
        for track in self.video_tracks.iter().chain(self.audio_tracks.iter()) {
            for clip in &track.clips {
                let end = clip.position + clip.duration;
                if end > max {
                    max = end;
                }
            }
        }
        max
    }

    /// Video tracks that show in preview and render: visible, not muted, and soloed if any track is.
    pub fn shown_video_tracks(&self) -> impl DoubleEndedIterator<Item = &Track> {
        let any_solo = self.video_tracks.iter().any(|t| t.solo);
        self.video_tracks.iter().filter(move |t| t.visible && !t.muted && (!any_solo || t.solo))
    }

    pub fn tracks(&self, video: bool) -> &Vec<Track> {
        if video { &self.video_tracks } else { &self.audio_tracks }
    }

    pub fn tracks_mut(&mut self, video: bool) -> &mut Vec<Track> {
        if video { &mut self.video_tracks } else { &mut self.audio_tracks }
    }

    pub fn add_video_track(&mut self) {
        let n = self.video_tracks.len() + 1;
        self.video_tracks.push(Track::new(&format!("Video {n}"), TrackKind::Video));
    }

    pub fn add_audio_track(&mut self) {
        let n = self.audio_tracks.len() + 1;
        self.audio_tracks.push(Track::new(&format!("Audio {n}"), TrackKind::Audio));
    }
}

impl Track {
    pub fn new(name: &str, kind: TrackKind) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            kind,
            clips: Vec::new(),
            muted: false,
            solo: false,
            locked: false,
            visible: true,
            height: 0.0, // 0 = the look's default height (VP16 64 / 68 px, VP23 52 px)
            volume: 1.0,
            pan: 0.0,
            composite: Composite::default(),
            motion: Keyframes::default(),
            fx: Vec::new(),
            envelopes: Vec::new(),
        }
    }
}

impl Clip {
    pub fn new(name: &str, position: f64, duration: f64) -> Self {
        let hue = (position * 40.0) as u8;
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            media_id: None,
            position,
            duration,
            in_point: 0.0,
            out_point: duration,
            name: name.to_string(),
            color: hue_to_rgb(hue),
            fade_in: 0.0,
            fade_out: 0.0,
            locked: false,
            gain_db: 0.0,
            fade_in_curve: Curve::Smooth,
            fade_out_curve: Curve::Smooth,
            rate: 1.0,
            reverse: false,
            muted: false,
            pan_crop: Keyframes::default(),
            fx: Vec::new(),
            transition: None,
            envelopes: Vec::new(),
            group: None,
            takes: Vec::new(),
        }
    }

    pub fn from_media(media: &MediaItem, position: f64) -> Self {
        Self {
            media_id: Some(media.id.clone()),
            color: match media.media_type {
                MediaType::Video => [70, 130, 180],
                MediaType::Audio => [100, 180, 100],
                MediaType::Image => [180, 140, 70],
                MediaType::Unknown => [140, 140, 140],
            },
            ..Self::new(&media.name, position, media.duration)
        }
    }

    /// Keep both fades inside the clip after a trim or fade drag.
    pub fn clamp_fades(&mut self) {
        let d = self.duration.max(0.0);
        self.fade_in = self.fade_in.clamp(0.0, d);
        self.fade_out = self.fade_out.clamp(0.0, d - self.fade_in);
    }
}

/// Fade gain (0..=1) at `t` seconds into a `dur` long event: the product of the fade-in and fade-out curves.
pub fn fade_gain(t: f64, dur: f64, fade_in: f64, fade_out: f64) -> f32 {
    let ramp = |x: f64, len: f64| if len > 0.0 { smooth_fade((x / len).clamp(0.0, 1.0)) } else { 1.0 };
    (ramp(t, fade_in) * ramp(dur - t, fade_out)) as f32
}

/// VEGAS's default "Smooth" fade curve, an S-curve from 0 to 1 (ffmpeg afade curve=hsin).
pub fn smooth_fade(x: f64) -> f64 {
    (1.0 - (std::f64::consts::PI * x).cos()) * 0.5
}

fn write_atomic(path: &Path, text: &str) -> anyhow::Result<()> {
    let tmp = path.with_extension("mnle.tmp");
    let res = std::fs::File::create(&tmp)
        .and_then(|mut f| f.write_all(text.as_bytes()).and_then(|_| f.sync_all()))
        .and_then(|_| std::fs::rename(&tmp, path));
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    Ok(res?)
}

fn file_stem(path: &Path) -> Option<String> {
    path.file_stem().map(|s| s.to_string_lossy().into_owned())
}

fn hue_to_rgb(h: u8) -> [u8; 3] {
    let h = h as f32 / 255.0 * 6.0;
    let c = 0.6_f32;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let m = 0.3_f32;
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    ]
}

/// VEGAS "Time & Frames": hh:mm:ss:ff, or SMPTE drop-frame hh:mm:ss;ff at 29.97/59.94 fps.
pub fn format_timecode(seconds: f64, fps: f64) -> String {
    let nominal = fps.round() as u64;
    if nominal == 0 {
        return "00:00:00:00".to_string();
    }
    let mut frames = (seconds.max(0.0) * fps).round() as u64;
    let drop = fps.fract() != 0.0 && (nominal == 30 || nominal == 60);
    if drop {
        // Skip frame numbers 0..k at each minute except every tenth (SMPTE 12M).
        let k = nominal / 15; // 2 at 30, 4 at 60
        let per_10min = nominal * 600 - 9 * k;
        let per_min = nominal * 60 - k;
        let (d, m) = (frames / per_10min, frames % per_10min);
        frames += 9 * k * d + if m > k { k * ((m - k) / per_min) } else { 0 };
    }
    let ff = frames % nominal;
    let total_secs = frames / nominal;
    let (secs, mins, hours) = (total_secs % 60, (total_secs / 60) % 60, total_secs / 3600);
    let sep = if drop { ';' } else { ':' };
    format!("{hours:02}:{mins:02}:{secs:02}{sep}{ff:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timecode_drop_frame_and_ndf() {
        assert_eq!(format_timecode(10.0, 25.0), "00:00:10:00");
        assert_eq!(format_timecode(1.5, 30.0), "00:00:01:15");
        assert_eq!(format_timecode(5.0, 29.97), "00:00:05;00");
        // Frame 1800 at 29.97 is the first frame of minute 1: numbers ;00 and ;01 are skipped.
        assert_eq!(format_timecode(1800.0 * 1001.0 / 30000.0, 29.97), "00:01:00;02");
        assert_eq!(format_timecode(17982.0 * 1001.0 / 30000.0, 29.97), "00:10:00;00");
        assert_eq!(format_timecode(3600.0, 29.97), "01:00:00;00");
    }

    fn gain(c: &Clip, t: f64) -> f32 {
        fade_gain(t, c.duration, c.fade_in, c.fade_out)
    }

    #[test]
    fn fade_gain_curve_and_clamp() {
        let mut c = Clip::new("c", 0.0, 10.0);
        assert_eq!(gain(&c, 0.0), 1.0); // no fades
        (c.fade_in, c.fade_out) = (2.0, 4.0);
        assert_eq!(gain(&c, 0.0), 0.0);
        assert!((gain(&c, 1.0) - 0.5).abs() < 1e-6); // S-curve midpoint
        assert!(gain(&c, 0.5) < 0.25); // slow start
        assert_eq!(gain(&c, 4.0), 1.0);
        assert!((gain(&c, 8.0) - 0.5).abs() < 1e-6);
        assert_eq!(gain(&c, 10.0), 0.0);
        c.duration = 5.0;
        c.clamp_fades();
        assert_eq!((c.fade_in, c.fade_out), (2.0, 3.0));
        c.duration = 1.0;
        c.clamp_fades();
        assert_eq!((c.fade_in, c.fade_out), (1.0, 0.0));
    }

    #[test]
    fn old_projects_load_without_fades_or_loop() {
        let mut p = Project::new("t");
        p.timeline.video_tracks[0].clips.push(Clip::new("c", 0.0, 1.0));
        p.media_pool.push(MediaItem { id: "m".into(), name: "m".into(), ..Default::default() });
        let mut v: serde_json::Value = serde_json::to_value(&p).unwrap();
        let strip = |o: &mut serde_json::Value, keys: &[&str]| {
            for k in keys {
                assert!(o.as_object_mut().unwrap().remove(*k).is_some(), "{k} not serialized");
            }
        };
        strip(&mut v["timeline"], &["loop_region", "master"]);
        strip(&mut v["timeline"]["video_tracks"][0], &["composite", "motion", "fx", "envelopes"]);
        let clip_keys = ["fade_in", "gain_db", "fade_in_curve", "fade_out_curve", "rate", "reverse", "muted", "pan_crop", "fx", "transition", "envelopes", "group", "takes"];
        strip(&mut v["timeline"]["video_tracks"][0]["clips"][0], &clip_keys);
        strip(&mut v["media_pool"][0], &["has_alpha", "rotation", "color_matrix", "interlaced", "generator"]);
        let back: Project = serde_json::from_value(v).unwrap();
        let tl = &back.timeline;
        assert_eq!(tl.loop_region, None);
        assert_eq!(tl.master.gain, [1.0, 1.0]);
        assert!(tl.master.audio_fx.is_empty() && tl.master.video_fx.is_empty());
        let t = &tl.video_tracks[0];
        assert_eq!(t.composite, Composite::SourceAlpha);
        assert!(t.motion.keys.is_empty() && t.fx.is_empty() && t.envelopes.is_empty());
        let c = &t.clips[0];
        assert_eq!(c.fade_in, 0.0);
        assert_eq!((c.rate, c.gain_db, c.reverse, c.muted), (1.0, 0.0, false, false));
        assert_eq!((c.fade_in_curve, c.fade_out_curve), (Curve::Smooth, Curve::Smooth));
        assert!(c.group.is_none() && c.takes.is_empty() && c.fx.is_empty() && c.transition.is_none());
        assert!(c.pan_crop.keys.is_empty() && c.envelopes.is_empty());
        let m = &back.media_pool[0];
        assert!(!m.has_alpha && !m.interlaced && m.rotation == 0 && m.color_matrix.is_none() && m.generator.is_none());
    }

    #[test]
    fn every_new_field_round_trips() {
        let mut p = Project::new("t");
        let keys = |v: f32| Keyframes { keys: vec![Keyframe { t: 0.0, v, curve: Curve::Hold }, Keyframe { t: 1.5, v: v * 2.0, curve: Curve::Sharp }] };
        let fx = |plugin: &str| Fx {
            id: "fx1".into(),
            plugin: plugin.into(),
            enabled: false,
            params: BTreeMap::from([("amount".to_string(), keys(0.25))]),
            text: BTreeMap::from([("text".to_string(), "Hello".to_string())]),
        };
        let env = |kind| Envelope { kind, points: keys(-6.0) };
        let mut c = Clip::new("c", 1.0, 4.0);
        c.gain_db = -3.5;
        (c.fade_in_curve, c.fade_out_curve) = (Curve::Fast, Curve::Slow);
        (c.rate, c.reverse, c.muted) = (0.5, true, true);
        c.pan_crop = Keyframes { keys: vec![Keyframe { t: 0.5, v: PanCrop { cx: 0.25, cy: 0.75, w: -0.5, h: 0.5, rot: 30.0 }, curve: Curve::Linear }] };
        c.fx = vec![fx("milan.fx.blur")];
        c.transition = Some(Transition { plugin: "milan.tr.dissolve".into(), params: BTreeMap::from([("soft".to_string(), 0.5)]) });
        c.envelopes = vec![env(EnvKind::Volume), env(EnvKind::Velocity)];
        c.group = Some("g1".into());
        c.takes = vec![Take { media_id: "m2".into(), in_point: 2.0, name: "Take 2".into() }];
        let t = &mut p.timeline.video_tracks[0];
        t.composite = Composite::Screen;
        t.motion = Keyframes { keys: vec![Keyframe { t: 0.0, v: Motion { x: 0.1, y: -0.2, w: 0.5, h: 0.5, rot: 45.0 }, curve: Curve::Smooth }] };
        t.fx = vec![fx("milan.fx.grade")];
        t.envelopes = vec![env(EnvKind::CompositeLevel), env(EnvKind::Opacity)];
        t.clips.push(c);
        p.timeline.audio_tracks[0].envelopes = vec![env(EnvKind::Pan), env(EnvKind::Mute)];
        p.timeline.master = MasterBus { gain: [0.5, 0.75], audio_fx: vec![fx("milan.afx.eq")], video_fx: vec![fx("milan.fx.lut")] };
        p.media_pool.push(MediaItem {
            id: "g".into(),
            media_type: MediaType::Image,
            duration: 86400.0,
            has_alpha: true,
            rotation: 270,
            color_matrix: Some("bt709".into()),
            interlaced: true,
            generator: Some(fx("milan.gen.text")),
            ..Default::default()
        });
        let json = serde_json::to_string(&p).unwrap();
        let back: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(serde_json::to_value(&back).unwrap(), serde_json::to_value(&p).unwrap());
        assert_eq!(back.timeline.video_tracks[0].clips[0], p.timeline.video_tracks[0].clips[0]);
    }

    #[test]
    fn unknown_envelope_kind_still_loads() {
        let e: Envelope = serde_json::from_str(r#"{"kind":"Brightness","points":{"keys":[]}}"#).unwrap();
        assert_eq!(e.kind, EnvKind::Unknown);
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("milan-save-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn save_into_missing_dir_changes_nothing() {
        let mut p = Project::new("Untitled");
        p.modified = true;
        let path = std::env::temp_dir().join(format!("milan-no-such-dir-{}", std::process::id())).join("x.mnle");
        assert!(p.save(&path).is_err());
        assert_eq!((p.name.as_str(), p.path.is_none(), p.modified), ("Untitled", true, true));
    }

    #[test]
    fn save_replaces_atomically_and_round_trips() {
        let dir = temp_dir("rt");
        let path = dir.join("Show.mnle");
        std::fs::write(&path, "old contents").unwrap();
        let mut p = Project::new("Untitled");
        p.timeline.video_tracks[0].clips.push(Clip::new("c", 2.0, 3.0));
        p.modified = true;
        p.save(&path).unwrap();
        assert_eq!((p.name.as_str(), p.path.as_ref(), p.modified), ("Show", Some(&path), false));
        assert!(!path.with_extension("mnle.tmp").exists());
        let back = Project::load(&path).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(back.name, "Show");
        assert_eq!(back.timeline.video_tracks[0].clips, p.timeline.video_tracks[0].clips);
    }
}
