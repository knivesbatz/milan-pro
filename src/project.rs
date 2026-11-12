use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackKind {
    Video,
    Audio,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaType {
    Video,
    Audio,
    Image,
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

    pub fn save(&mut self, path: &PathBuf) -> anyhow::Result<()> {
        // The project is named after its file, so titles and autosave backups are per-project.
        if let Some(stem) = file_stem(path) {
            self.name = stem;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        self.path = Some(path.clone());
        self.modified = false;
        Ok(())
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
        }
    }

    pub fn from_media(media: &MediaItem, position: f64) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            media_id: Some(media.id.clone()),
            position,
            duration: media.duration,
            in_point: 0.0,
            out_point: media.duration,
            name: media.name.clone(),
            color: match media.media_type {
                MediaType::Video => [70, 130, 180],
                MediaType::Audio => [100, 180, 100],
                MediaType::Image => [180, 140, 70],
                MediaType::Unknown => [140, 140, 140],
            },
            fade_in: 0.0,
            fade_out: 0.0,
            locked: false,
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

fn file_stem(path: &std::path::Path) -> Option<String> {
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
        let mut v: serde_json::Value = serde_json::to_value(&p).unwrap();
        v["timeline"].as_object_mut().unwrap().remove("loop_region");
        v["timeline"]["video_tracks"][0]["clips"][0].as_object_mut().unwrap().remove("fade_in");
        let back: Project = serde_json::from_value(v).unwrap();
        assert_eq!(back.timeline.loop_region, None);
        assert_eq!(back.timeline.video_tracks[0].clips[0].fade_in, 0.0);
    }
}
