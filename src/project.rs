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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        self.path = Some(path.clone());
        self.modified = false;
        Ok(())
    }

    pub fn load(path: &PathBuf) -> anyhow::Result<Self> {
        let json = std::fs::read_to_string(path)?;
        let mut project: Self = serde_json::from_str(&json)?;
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
            height: 64.0,
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
        }
    }
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

pub fn format_timecode(seconds: f64, fps: f64) -> String {
    let total_frames = (seconds * fps).round() as u64;
    let fps_int = fps.round() as u64;
    if fps_int == 0 {
        return "00:00:00:00".to_string();
    }
    let frames = total_frames % fps_int;
    let total_secs = total_frames / fps_int;
    let secs = total_secs % 60;
    let mins = (total_secs / 60) % 60;
    let hours = total_secs / 3600;
    format!("{hours:02}:{mins:02}:{secs:02}:{frames:02}")
}
