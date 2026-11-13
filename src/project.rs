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
    /// R16a: View › Window › Project Notes (VEGAS 23). Not in the undo history (`history::restore` keeps the live ones).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<Note>,
    #[serde(skip)]
    pub modified: bool,
}

/// R16a: one Project Notes entry: a timeline position (Add New Note takes the cursor), the text, and Resolved.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Note {
    pub at: f64,
    pub text: String,
    pub resolved: bool,
}

/// R16a: Project Properties › Summary.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Summary {
    pub title: String,
    pub artist: String,
    pub engineer: String,
    pub copyright: String,
    pub comments: String,
}

impl Summary {
    fn is_empty(&self) -> bool {
        *self == Summary::default()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectSettings {
    pub width: u32,
    pub height: u32,
    pub frame_rate: f64,
    pub sample_rate: u32,
    /// Project Properties › Ruler (VEGAS keeps the ruler format and tempo with the project).
    #[serde(default)]
    pub ruler: Ruler,
    /// Project Properties › Resample mode. A project saved before R3b never blended frames, so it opens with Disable.
    #[serde(default = "Resample::disable", deserialize_with = "lenient")]
    pub resample: Resample,
    /// Project Properties › Motion blur type: the weighting of the sub-frames the Video Bus Track's Motion Blur Amount
    /// envelope mixes.
    #[serde(default, deserialize_with = "lenient")]
    pub motion_blur: BlurType,
    /// R11: Project Properties › Field order, Pixel aspect ratio and Output rotation (degrees clockwise: 0, 90, 180,
    /// 270). Render templates that follow the project take the field order and pixel aspect ratio.
    #[serde(default, deserialize_with = "lenient")]
    pub field_order: FieldOrder,
    #[serde(default = "one")]
    pub par: f64,
    #[serde(default)]
    pub rotation: u16,
    /// R11: Full-resolution rendering quality. A project saved before R11 rendered at Best, so it keeps Best.
    #[serde(default = "best", deserialize_with = "lenient_best")]
    pub render_quality: crate::decode::Quality,
    /// R11: Deinterlace method. A project saved before R11 interpolated (yadif), so it keeps Interpolate.
    #[serde(default = "Deinterlace::interpolate", deserialize_with = "lenient")]
    pub deinterlace: Deinterlace,
    /// R11: "Adjust source media to better match project or render settings": interlaced sources are deinterlaced and
    /// sources keep their shape in a project of non-square pixels. Off, media is used as it is.
    #[serde(default = "yes")]
    pub adjust_media: bool,
    /// R11: audio Bit depth (8, 16, 24 or 32 = float): the PCM, FLAC and ALAC renders that follow the project.
    #[serde(default = "sixteen")]
    pub bit_depth: u8,
    /// R16a: Project Properties › Summary.
    #[serde(default, skip_serializing_if = "Summary::is_empty")]
    pub summary: Summary,
}

/// Project Properties › Field order (and a render template's).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldOrder {
    #[default]
    Progressive,
    Upper,
    Lower,
}

impl FieldOrder {
    pub const ALL: [FieldOrder; 3] = [FieldOrder::Progressive, FieldOrder::Upper, FieldOrder::Lower];

    pub fn name(self) -> &'static str {
        ["None (progressive scan)", "Upper field first", "Lower field first"][self as usize]
    }
}

/// Project Properties › Deinterlace method, for interlaced sources.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Deinterlace {
    None,
    /// Both fields averaged (VEGAS's default): a vertical [1 2 1] / 4 blend of the woven frame.
    #[default]
    Blend,
    /// One field at a time, the missing lines interpolated (yadif).
    Interpolate,
}

impl Deinterlace {
    pub const ALL: [Deinterlace; 3] = [Deinterlace::None, Deinterlace::Blend, Deinterlace::Interpolate];

    fn interpolate() -> Self {
        Deinterlace::Interpolate
    }

    pub fn name(self) -> &'static str {
        ["None", "Blend fields", "Interpolate fields"][self as usize]
    }

    /// The decoder's filter, ahead of its frame-rate filter ("" for None).
    pub fn filter(self) -> &'static str {
        match self {
            Deinterlace::None => "",
            Deinterlace::Blend => "convolution=0m='0 1 0 0 2 0 0 1 0':1m='0 1 0 0 2 0 0 1 0':2m='0 1 0 0 2 0 0 1 0':0rdiv=0.25:1rdiv=0.25:2rdiv=0.25,",
            Deinterlace::Interpolate => "yadif=deint=interlaced,",
        }
    }
}

/// Project Properties › Pixel aspect ratio: VEGAS's list.
pub const PARS: [(f64, &str); 8] = [
    (1.0, "1.0000 (Square)"),
    (0.9091, "0.9091 (NTSC DV)"),
    (1.2121, "1.2121 (NTSC DV Widescreen)"),
    (1.0926, "1.0926 (PAL DV)"),
    (1.4568, "1.4568 (PAL DV Widescreen)"),
    (1.3333, "1.3333 (HDV 1080)"),
    (1.5, "1.5000 (DVCPRO HD)"),
    (2.0, "2.0000 (Anamorphic 2:1)"),
];

/// Project Properties › Output rotation.
pub const ROTATIONS: [(u16, &str); 4] = [(0, "0° (original)"), (90, "90° clockwise"), (180, "180°"), (270, "90° counterclockwise")];

fn best() -> crate::decode::Quality {
    crate::decode::Quality::Best
}

fn sixteen() -> u8 {
    16
}

fn lenient_best<'de, D: serde::Deserializer<'de>>(d: D) -> Result<crate::decode::Quality, D::Error> {
    Ok(crate::decode::Quality::deserialize(serde_json::Value::deserialize(d)?).unwrap_or(crate::decode::Quality::Best))
}

/// Project Properties › Motion blur type (VEGAS's six): symmetric about the frame, or asymmetric (trailing only).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlurType {
    #[default]
    Gaussian,
    Pyramid,
    Box,
    AsymmetricGaussian,
    AsymmetricPyramid,
    AsymmetricBox,
}

impl BlurType {
    pub const ALL: [BlurType; 6] = [BlurType::Gaussian, BlurType::Pyramid, BlurType::Box, BlurType::AsymmetricGaussian, BlurType::AsymmetricPyramid, BlurType::AsymmetricBox];

    pub fn name(self) -> &'static str {
        ["Gaussian", "Pyramid", "Box", "Asymmetric Gaussian", "Asymmetric Pyramid", "Asymmetric Box"][self as usize]
    }
}

/// Project Properties › Resample mode and the video event Switches' resample radios (an event's None is Use Project
/// Resample Mode). Resampling blends the two source frames around each output frame's time.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Resample {
    /// When the source frame rate differs from the project's, or the event plays off normal speed (VEGAS 23 calls the
    /// project mode Frame blend).
    #[default]
    Smart,
    Force,
    Disable,
    /// Motion-interpolated frames (VEGAS 23), where Smart would blend.
    OpticalFlow,
}

impl Resample {
    fn disable() -> Self {
        Resample::Disable
    }
}

/// The audio event Channels ▸ radios.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChannelMap {
    #[default]
    Both,
    LeftOnly,
    RightOnly,
    /// Both channels mixed, on both sides.
    Combine,
    Swap,
}

/// The audio an event's Normalize gain was measured on: when an Auto Normalize event's differs, it is measured again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NormSpan {
    pub media: Option<String>,
    pub stream: u32,
    pub channels: ChannelMap,
    pub in_point: f64,
    pub duration: f64,
    pub rate: f64,
}

/// Switches › Normalize: the gain that takes the event's peak to the normalize level, and what it was measured on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Normalized {
    pub db: f32,
    pub span: NormSpan,
}

/// Options › Ruler Format: VEGAS's 15 formats.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RulerFormat {
    Samples,
    Time,
    Seconds,
    #[default]
    TimeFrames,
    AbsFrames,
    MeasuresBeats,
    Feet16,
    Feet35,
    SmpteFilmIvtc,
    SmpteFilm,
    SmpteEbu,
    SmpteNonDrop,
    SmpteDrop,
    Smpte30,
    AudioCd,
}

impl RulerFormat {
    pub const ALL: [RulerFormat; 15] = {
        use RulerFormat::*;
        [Samples, Time, Seconds, TimeFrames, AbsFrames, MeasuresBeats, Feet16, Feet35, SmpteFilmIvtc, SmpteFilm, SmpteEbu, SmpteNonDrop, SmpteDrop, Smpte30, AudioCd]
    };

    pub fn name(self) -> &'static str {
        use RulerFormat::*;
        match self {
            Samples => "Samples",
            Time => "Time",
            Seconds => "Seconds",
            TimeFrames => "Time & Frames",
            AbsFrames => "Absolute Frames",
            MeasuresBeats => "Measures & Beats",
            Feet16 => "Feet and Frames 16mm (40 fpf)",
            Feet35 => "Feet and Frames 35mm (16 fpf)",
            SmpteFilmIvtc => "SMPTE Film Sync IVTC (23.976 fps, Video)",
            SmpteFilm => "SMPTE Film Sync (24 fps, Film)",
            SmpteEbu => "SMPTE EBU (25 fps, Video)",
            SmpteNonDrop => "SMPTE Non-Drop (29.97 fps, Video)",
            SmpteDrop => "SMPTE Drop (29.97 fps, Video)",
            Smpte30 => "SMPTE 30 (30 fps, Audio)",
            AudioCd => "Audio CD Time",
        }
    }

    /// (exact rate, nominal frames per second, drop-frame) of the timecode formats; Time & Frames uses the project's.
    fn smpte(self, fps: f64) -> Option<(f64, u64, bool)> {
        use RulerFormat::*;
        let ntsc = |n: f64| n * 1000.0 / 1001.0;
        Some(match self {
            TimeFrames => (exact_fps(fps), fps.round() as u64, is_drop(fps)),
            SmpteFilmIvtc => (ntsc(24.0), 24, false),
            SmpteFilm => (24.0, 24, false),
            SmpteEbu => (25.0, 25, false),
            SmpteNonDrop => (ntsc(30.0), 30, false),
            SmpteDrop => (ntsc(30.0), 30, true),
            Smpte30 => (30.0, 30, false),
            _ => return None,
        })
    }

    /// Frames per second the format counts (its ruler ticks fall on these frames).
    pub fn frame_rate(self, fps: f64) -> f64 {
        match self {
            RulerFormat::AudioCd => 75.0,
            f => f.smpte(fps).map_or(exact_fps(fps), |s| s.0),
        }
    }
}

/// Project Properties › Ruler.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ruler {
    #[serde(deserialize_with = "lenient")]
    pub format: RulerFormat,
    /// Ruler time at timeline 0 ("Ruler time starts at", Set Time at Cursor), in seconds.
    pub offset: f64,
    /// Beats per minute.
    pub tempo: f64,
    pub beats_per_measure: u32,
    /// The note one beat is: 4 = a quarter note.
    pub beat_note: u32,
}

impl Default for Ruler {
    fn default() -> Self {
        Self { format: RulerFormat::TimeFrames, offset: 0.0, tempo: 120.0, beats_per_measure: 4, beat_note: 4 }
    }
}

/// Options › Grid Spacing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum GridSpacing {
    /// The ruler's major ticks.
    #[default]
    RulerMarks,
    Minutes,
    Seconds,
    Milliseconds,
    Frames,
    Measures,
    /// 1/n notes (4, 8, 16, 32), triplets when true.
    Note(u32, bool),
}

impl GridSpacing {
    /// Seconds from one grid line to the next before thinning for the zoom; None for Ruler Marks.
    pub fn base(self, s: &ProjectSettings) -> Option<f64> {
        use GridSpacing::*;
        Some(match self {
            RulerMarks => return None,
            Minutes => 60.0,
            Seconds => 1.0,
            Milliseconds => 0.001,
            Frames => 1.0 / exact_fps(s.frame_rate.max(1.0)),
            Measures => s.beat_secs() * s.ruler.beats_per_measure.max(1) as f64,
            Note(n, triplet) => s.beat_secs() * s.ruler.beat_note.max(1) as f64 / n.max(1) as f64 * if triplet { 2.0 / 3.0 } else { 1.0 },
        })
    }
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
    /// Folder new recordings go to; None = the project file's folder.
    #[serde(default)]
    pub record_dir: Option<PathBuf>,
    /// Options › Audio Follows Video Velocity: an A/V pair's audio plays its video event's Velocity envelope
    /// (`edit::sync_velocity`). Off: the audio stays at normal speed, as in VEGAS.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub audio_follows_velocity: bool,
    /// Track › Track Group: named sets of adjacent tracks under a header row (`edit::group_tracks`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub track_groups: Vec<TrackGroup>,
    /// R8a: the busses and assignable FX chains (Project Properties › Audio, Insert › Audio Bus / Audio Assignable FX).
    #[serde(default, skip_serializing_if = "AudioSettings::is_empty")]
    pub audio: AudioSettings,
    /// R18a: Insert › Command: metadata command markers, on the command bar.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<CommandMarker>,
    /// R18a: Insert › Audio CD Track Region and Audio CD Track Index, on the CD layout bar (burning is R59).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cd_tracks: Vec<CdTrack>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cd_indexes: Vec<f64>,
}

/// R18a: a metadata command marker (Insert › Command): VEGAS's Command Properties.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CommandMarker {
    pub position: f64,
    /// URL, Text, WMClosedCaption... or a custom command.
    pub command: String,
    pub parameter: String,
    pub comment: String,
}

/// R18a: an Audio CD track region with VEGAS's Edit Details columns (ISRC, copy protection, emphasis).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CdTrack {
    pub start: f64,
    pub end: f64,
    pub name: String,
    pub isrc: String,
    pub protect: bool,
    pub emphasis: bool,
}

impl Timeline {
    /// R18a: the CD layout bar or the command bar has something to show (each is only shown then).
    pub fn cd_layout(&self) -> bool {
        !(self.cd_tracks.is_empty() && self.cd_indexes.is_empty())
    }
}

/// R8a: the audio busses (the master bus is `Timeline::master`). Master bus mode stays stereo until R10a.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioSettings {
    /// Bus A, Bus B... (Project Properties › Number of stereo busses, 0-26).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub buses: Vec<Bus>,
    /// Assignable FX chains ("FX 1"...): busses that tracks only send to.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fx: Vec<Bus>,
    /// R9: input busses (Insert › Audio Input Bus): the recording input through FX, volume, pan and mute to the master or
    /// a bus, heard live (input monitoring) and never rendered. Not in the mix plan: the input monitor plays them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<Bus>,
}

impl AudioSettings {
    /// VEGAS allows 26 busses (A-Z), and 26 input busses.
    pub const MAX_BUSES: usize = 26;

    fn is_empty(&self) -> bool {
        self.buses.is_empty() && self.fx.is_empty() && self.inputs.is_empty()
    }

    /// Something a track can send to (a bus or an assignable FX chain): the track headers show the multipurpose slider.
    pub fn has_targets(&self) -> bool {
        !(self.buses.is_empty() && self.fx.is_empty())
    }

    /// The bus, assignable FX chain or input bus `id`.
    pub fn bus(&self, id: &str) -> Option<&Bus> {
        self.buses.iter().chain(&self.fx).chain(&self.inputs).find(|b| b.id == id)
    }

    pub fn bus_mut(&mut self, id: &str) -> Option<&mut Bus> {
        self.buses.iter_mut().chain(&mut self.fx).chain(&mut self.inputs).find(|b| b.id == id)
    }
}

/// An audio bus or an assignable FX chain: FX, then Volume (linear) and Pan, then its output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bus {
    pub id: String,
    pub name: String,
    #[serde(default = "one32")]
    pub volume: f32,
    #[serde(default)]
    pub pan: f32,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub fx: Vec<Fx>,
    /// Volume, Pan and Mute envelopes (timeline time).
    #[serde(default)]
    pub envelopes: Vec<Envelope>,
    #[serde(default, skip_serializing_if = "Out::is_master")]
    pub out: Out,
    /// R8c: Automation Settings and Show Automation Controls, as on a track.
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "AutoMode::is_read")]
    pub automation: AutoMode,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto_controls: bool,
}

impl Bus {
    pub fn new(name: String, fx: Vec<Fx>) -> Self {
        Self { id: uuid::Uuid::new_v4().to_string(), name, volume: 1.0, pan: 0.0, muted: false, fx, envelopes: Vec::new(), out: Out::Master, automation: AutoMode::Read, auto_controls: false }
    }
}

/// R8c: a track's or bus's Automation Settings (VEGAS). Off: its envelopes and FX automation are ignored and its
/// controls are trims. Read: they play, and automation controls follow them. Write (Touch): an automation control
/// writes points while it is held. Write (Latch): from the first move it writes until playback stops.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AutoMode {
    Off,
    #[default]
    Read,
    Touch,
    Latch,
}

impl AutoMode {
    pub const ALL: [AutoMode; 4] = [AutoMode::Off, AutoMode::Read, AutoMode::Touch, AutoMode::Latch];

    /// The Automation ▸ menu's name.
    pub fn name(self) -> &'static str {
        ["Off", "Read", "Write (Touch)", "Write (Latch)"][self as usize]
    }

    fn is_read(&self) -> bool {
        *self == AutoMode::Read
    }

    /// Envelopes and FX automation play (every mode but Off).
    pub fn plays(self) -> bool {
        self != AutoMode::Off
    }

    /// An automation control writes points.
    pub fn writes(self) -> bool {
        matches!(self, AutoMode::Touch | AutoMode::Latch)
    }
}

/// R9: an armed audio track's input monitoring (VEGAS's Record Input › Input Monitoring): Off; On, the input is heard
/// through the track's FX whenever the track is armed; Auto, while stopped and while recording (playback plays the
/// track instead).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Monitor {
    #[default]
    Off,
    On,
    Auto,
}

impl Monitor {
    pub const ALL: [Monitor; 3] = [Monitor::Off, Monitor::On, Monitor::Auto];

    pub fn name(self) -> &'static str {
        ["Input Monitoring Off", "Input Monitoring On", "Input Monitoring Auto"][self as usize]
    }

    fn is_off(&self) -> bool {
        *self == Monitor::Off
    }

    /// The input is heard now, on an armed track.
    pub fn hears(self, playing: bool, recording: bool) -> bool {
        match self {
            Monitor::Off => false,
            Monitor::On => true,
            Monitor::Auto => !playing || recording,
        }
    }
}

/// Where a track, bus or assignable FX chain goes: the master bus or a bus (by id).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Out {
    #[default]
    Master,
    Bus(String),
}

impl Out {
    pub fn is_master(&self) -> bool {
        *self == Out::Master
    }
}

/// A track's send to a bus or an assignable FX chain: level (linear) taken before or after the track's volume and mute.
/// VEGAS: bus sends are pre-volume, FX sends post-volume, by default; a send not listed is at -inf.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioSend {
    pub to: String,
    pub level: f32,
    #[serde(default)]
    pub pre: bool,
}

/// Audio track Pan Type (VEGAS's six). In stereo, Film is Constant Power.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PanType {
    /// Panning moves the far channel into the near one.
    AddChannels,
    /// The far channel falls from the centre level to -inf; the near one rises from it to 0 dB.
    #[default]
    Balance0,
    Balance3,
    Balance6,
    ConstantPower,
    Film,
}

impl PanType {
    pub const ALL: [PanType; 6] = [PanType::AddChannels, PanType::Balance0, PanType::Balance3, PanType::Balance6, PanType::ConstantPower, PanType::Film];

    pub fn name(self) -> &'static str {
        ["Add Channels (0 dB Center)", "Balance (0 dB Center)", "Balance (-3 dB Center)", "Balance (-6 dB Center)", "Constant Power", "Film"][self as usize]
    }

    fn is_default(&self) -> bool {
        *self == PanType::Balance0
    }

    /// The stereo matrix of pan `p` (-1 left .. 1 right): [[L from L, L from R], [R from L, R from R]].
    pub fn matrix(self, p: f32) -> [[f32; 2]; 2] {
        let p = p.clamp(-1.0, 1.0);
        let balance = |c: f32| {
            let (away, near) = (c * (1.0 - p.abs()), c + (1.0 - c) * p.abs());
            if p >= 0.0 { [[away, 0.0], [0.0, near]] } else { [[near, 0.0], [0.0, away]] }
        };
        match self {
            // Exactly the law every track had before R8a.
            PanType::Balance0 => [[(1.0 - p).min(1.0), 0.0], [0.0, (1.0 + p).min(1.0)]],
            PanType::Balance3 => balance(10f32.powf(-3.0 / 20.0)),
            PanType::Balance6 => balance(10f32.powf(-6.0 / 20.0)),
            PanType::AddChannels => [[1.0 - p.max(0.0), (-p).max(0.0)], [p.max(0.0), 1.0 - (-p).max(0.0)]],
            PanType::ConstantPower | PanType::Film => {
                let a = (p + 1.0) * std::f32::consts::FRAC_PI_4;
                [[a.cos(), 0.0], [0.0, a.sin()]]
            }
        }
    }
}

/// A track group: its header collapses the member rows and carries a group Mute and Solo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackGroup {
    pub id: String,
    pub name: String,
    /// Member track ids (either kind).
    pub tracks: Vec<String>,
    #[serde(default)]
    pub collapsed: bool,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub solo: bool,
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
    #[serde(default, deserialize_with = "lenient")]
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
    /// Audio track Switches › Invert Phase.
    #[serde(default)]
    pub invert: bool,
    /// Video track Fade Colors: [top, bottom] (sRGB), what a Fade to Color envelope fades to (VEGAS: white, black).
    #[serde(default = "fade_colors")]
    pub fade_colors: [[u8; 3]; 2],
    /// Track header › Hide: the row is left out of the timeline (it still plays).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    /// Make Compositing Child (video): the track composites into its parent, the nearest non-child track above it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub child: bool,
    /// A compositing parent's Parent Composite Mode and Parent Motion: how the parent and its children, composited
    /// together, go onto the tracks below, and where.
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Composite::is_default")]
    pub parent_mode: Composite,
    #[serde(default, skip_serializing_if = "Keyframes::is_empty")]
    pub parent_motion: Keyframes<Motion>,
    /// The Custom compositing mode's compositor plug-in (Compositors window).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compositor: Option<Fx>,
    /// Track Motion 2D Shadow and 2D Glow.
    #[serde(default, skip_serializing_if = "Halo2D::is_off")]
    pub shadow: Halo2D,
    #[serde(default, skip_serializing_if = "Halo2D::is_off")]
    pub glow: Halo2D,
    /// Bypass Motion Blur: the Video Bus Track's motion blur and supersampling leave this track alone.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bypass_blur: bool,
    /// R8a (audio): the track's output (track header › Output), its sends, and its Pan Type.
    #[serde(default, skip_serializing_if = "Out::is_master")]
    pub out: Out,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sends: Vec<AudioSend>,
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "PanType::is_default")]
    pub pan_type: PanType,
    /// R8c (audio): Automation Settings, and Show Automation Controls (the Vol and Pan sliders follow and write the
    /// envelopes instead of trimming).
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "AutoMode::is_read")]
    pub automation: AutoMode,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto_controls: bool,
    /// R9 (audio): Record Input › Input Monitoring (Off, as in VEGAS, unless set).
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Monitor::is_off")]
    pub monitor: Monitor,
    /// The bus or FX chain the header's multipurpose slider shows (a view choice: not saved).
    #[serde(skip)]
    pub send_view: Option<String>,
    /// Armed for recording (audio): this session only.
    #[serde(skip)]
    pub armed: bool,
}

impl Track {
    /// The send to bus `to`, if its level is above -inf.
    pub fn send(&self, to: &str) -> Option<&AudioSend> {
        self.sends.iter().find(|s| s.to == to)
    }
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
    #[serde(default, deserialize_with = "lenient")]
    pub fade_in_curve: Curve,
    #[serde(default, deserialize_with = "lenient")]
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
    /// Transition on a fade-out with nothing after it: the event goes out to transparent through it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition_out: Option<Transition>,
    /// Event envelopes, event-relative.
    #[serde(default)]
    pub envelopes: Vec<Envelope>,
    /// Id shared by the events of one group.
    #[serde(default)]
    pub group: Option<String>,
    /// Inactive takes; the active take is the event's own media.
    #[serde(default)]
    pub takes: Vec<Take>,
    /// R17b: the active take's number (from 0) in the event's fixed take order: the ring `[active, takes...]` starts at
    /// it, so Take 1, 2, ... (a multicamera event's cameras) keep their numbers as the active take changes.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub take_no: u32,
    /// Switches › Loop (on by default, as in VEGAS): the event may run past its media's end, which then repeats.
    #[serde(default = "yes")]
    pub looping: bool,
    /// Switches › Invert Phase (audio).
    #[serde(default)]
    pub invert: bool,
    /// Switches › Normalize (audio), and VEGAS 23's Auto Normalize, which measures again after each edit of the event's
    /// audio (`edit::needs_normalize`).
    #[serde(default)]
    pub normalize: bool,
    #[serde(default)]
    pub auto_normalize: bool,
    /// The Normalize gain, once a background scan has measured the peak.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalized: Option<Normalized>,
    /// Channels ▸ (audio).
    #[serde(default, deserialize_with = "lenient")]
    pub channels: ChannelMap,
    /// Stream ▸: which of the media's audio streams (audio event) or video streams (video event) plays, from 0.
    #[serde(default)]
    pub stream: u32,
    /// Switches › Maintain Aspect Ratio (video; on, as in VEGAS): off, the picture is stretched to fill the frame.
    #[serde(default = "yes")]
    pub keep_aspect: bool,
    /// Switches › Reduce Interlace Flicker (video): a light vertical blur.
    #[serde(default)]
    pub reduce_flicker: bool,
    /// Switches › resample radios (video); None = Use Project Resample Mode.
    #[serde(default, deserialize_with = "lenient")]
    pub resample: Option<Resample>,
    /// Event Properties › Undersample rate (video): the event shows `r` x the project frame rate (each frame held 1/r
    /// frames). None or 1 = off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undersample: Option<f64>,
    /// Event Properties › Pitch change (audio), in cents: the pitch moves, the length does not (VEGAS's Pitch Up /
    /// Down keys: = and - a semitone, with Ctrl a cent, with Shift an octave).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub pitch_cents: f32,
    /// Event Pan/Crop › Mask (video): Bezier paths over the source picture.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub masks: Vec<Mask>,
}

/// One Pan/Crop mask path. Positive shows inside it, Negative hides inside it; the positive paths add together, the
/// negative ones cut out of them (with only negative paths, everything outside them shows), as in VEGAS.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mask {
    /// Closed Bezier path, keyed event-relative: each anchor is [x, y, in dx, in dy, out dx, out dy], normalised to the
    /// source picture (the handles relative to their anchor).
    #[serde(default)]
    pub path: Keyframes<Vec<[f32; 6]>>,
    #[serde(default, deserialize_with = "lenient")]
    pub mode: MaskMode,
    /// 0..=1.
    #[serde(default = "one32")]
    pub opacity: f32,
    /// Soft edge, a fraction of the source height.
    #[serde(default)]
    pub feather: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaskMode {
    #[default]
    Positive,
    Negative,
    Disabled,
}

/// Track Motion 2D Shadow or 2D Glow: a blurred, coloured copy of the track's picture under it (the shadow offset).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Halo2D {
    #[serde(default)]
    pub on: bool,
    #[serde(default)]
    pub keys: Keyframes<Halo>,
}

impl Halo2D {
    fn is_off(&self) -> bool {
        !self.on && self.keys.is_empty()
    }
}

/// One 2D Shadow / Glow key: offset (frame sizes), blur (a fraction of the frame height), intensity 0..=1, colour.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Halo {
    pub x: f32,
    pub y: f32,
    pub blur: f32,
    pub intensity: f32,
    pub color: [f32; 3],
}

impl Default for Halo {
    /// The 2D Shadow's: down and right, soft, black.
    fn default() -> Self {
        Self { x: 0.02, y: 0.03, blur: 0.02, intensity: 0.75, color: [0.0; 3] }
    }
}

impl Halo {
    /// The 2D Glow's: around the picture, white.
    pub fn glow() -> Self {
        Self { x: 0.0, y: 0.0, blur: 0.03, intensity: 1.0, color: [1.0; 3] }
    }
}

fn one32() -> f32 {
    1.0
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
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
    /// `path` relative to the project's folder, written on save; where Open looks when `path` is gone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rel_path: Option<PathBuf>,
    /// A subclip (Create Subclip): a stretch of its parent's file, used as if it were the whole file. `path` is the
    /// parent's and `duration` the stretch's length.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subclip: Option<Subclip>,
    /// How many audio and video streams the file has (Stream ▸); 0 = not counted (probed before R3b), taken as 1.
    #[serde(default)]
    pub audio_streams: u32,
    #[serde(default)]
    pub video_streams: u32,
    /// R6a: a Milan Pro project (`.mnle`) used as media, a nested timeline (`engine::nested`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub nested: bool,
    /// R17a: the file's start timecode, in seconds since 00:00:00:00 (a `timecode` tag, a QuickTime `tmcd` track or a
    /// BWF `time_reference`); None when it has none, or was imported before R17a.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timecode: Option<f64>,
    /// R17a: the file's `creation_time` tag (recording start), Unix seconds; `engine::sync::recorded` falls back on
    /// the file's modified time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<f64>,
    /// R18a: the file's own markers (chapters, read with ffprobe `-show_chapters`) in seconds from the file's start, as
    /// View › Event Media Markers shows them; empty for media imported before R18a.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub markers: Vec<(f64, String)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Subclip {
    /// The media item it was made from.
    pub parent: String,
    /// Where it starts in the file, in seconds.
    pub start: f64,
}

/// `path` relative to the folder `base` (with `..` steps); None on another drive or for a relative `path`.
fn relative(path: &Path, base: &Path) -> Option<PathBuf> {
    use std::path::Component;
    if !path.is_absolute() {
        return None;
    }
    let (mut p, mut b) = (path.components().peekable(), base.components().peekable());
    if p.peek() != b.peek() {
        return None; // the drive differs: no way between them
    }
    while p.peek().is_some() && p.peek() == b.peek() {
        p.next();
        b.next();
    }
    let up = b.filter(|c| matches!(c, Component::Normal(_))).map(|_| Component::ParentDir);
    Some(up.chain(p).collect())
}

impl MediaItem {
    /// Never probed: a file that was missing when a VEGAS project was imported (no picture or sound known).
    pub fn is_offline(&self) -> bool {
        self.generator.is_none() && self.width.is_none() && self.sample_rate.is_none()
    }

    /// Time in the file of time `t` of this item (a subclip starts part way into its file).
    pub fn file_time(&self, t: f64) -> f64 {
        t + self.subclip.as_ref().map_or(0.0, |s| s.start)
    }

    /// Streams of one kind the Stream ▸ menu lists (at least 1).
    pub fn streams(&self, video: bool) -> u32 {
        if video { self.video_streams } else { self.audio_streams }.max(1)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaType {
    Video,
    Audio,
    Image,
    /// Also a type written by a later version.
    #[default]
    #[serde(other)]
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

/// A compositing mode or curve written by a later version (VEGAS's Hard Light, ...) loads as the default instead of
/// failing the whole project.
fn lenient<'de, D: serde::Deserializer<'de>, T: Deserialize<'de> + Default>(d: D) -> Result<T, D::Error> {
    Ok(T::deserialize(serde_json::Value::deserialize(d)?).unwrap_or_default())
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
    #[serde(default = "Curve::linear", deserialize_with = "lenient")]
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
    /// Event velocity (video): playback speed, 1 = 100%, -1 (reverse) ..= 10; a 0% Hold point is a freeze frame.
    Velocity,
    /// Video track Fade to Color: 0 none, 1 all the top colour, -1 all the bottom colour (`Track::fade_colors`).
    FadeToColor,
    /// Transition Progress, 0..=1 (kept on the `Transition`, its keys over the transition's length 0..=1).
    TransitionProgress,
    /// Video Bus Track: Motion Blur Amount, the length of the blur in seconds (0..=1), and Video Supersampling, the
    /// sub-frames rendered per frame (0..=8).
    MotionBlur,
    Supersampling,
    /// R8c: an FX parameter (`Fx::auto` on audio track and bus FX; a video track FX's keys), lent to the envelope
    /// editor as 0..=1 of the parameter's range (`edit::FX_ENV`). Never stored in `envelopes`.
    FxParam,
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
    /// R8c: FX automation envelopes of an audio track's or bus's plug-in, by parameter key, in the parameter's units
    /// and timeline time. They play over `params` unless the track's automation is Off.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub auto: BTreeMap<String, Keyframes<f32>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Transition {
    pub plugin: String,
    #[serde(default)]
    pub params: BTreeMap<String, f32>,
    /// Transition Progress envelope: progress (0..=1) at each fraction (0..=1) of the transition's length, so it
    /// follows the overlap when an edge moves; no keys = linear, 0 to 1.
    #[serde(default, skip_serializing_if = "Keyframes::is_empty")]
    pub progress: Keyframes<f32>,
}

impl<T> Keyframes<T> {
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
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

/// Track Motion, normalised to the project frame. `z` (frame widths towards the viewer) and the X / Y rotations
/// (degrees) are the 3D Source Alpha mode's; other modes ignore them.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Motion {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub rot: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub z: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rx: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub ry: f32,
}

impl Default for Motion {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0, w: 1.0, h: 1.0, rot: 0.0, z: 0.0, rx: 0.0, ry: 0.0 }
    }
}

/// Video track compositing mode. The first block is VEGAS's list; Soft Light .. Luminosity are Milan Pro's extras,
/// offered only with Preferences › Video › "Extra blend modes" (off by default), and always played.
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
    /// Source Alpha with 3D Track Motion (position in depth, rotation about X and Y).
    ThreeD,
    /// The source's alpha cuts a hole in what is below.
    Cut,
    HardLight,
    Dodge,
    Burn,
    DifferenceSquared,
    /// A compositor plug-in (`Track::compositor`).
    Custom,
    SoftLight,
    Exclusion,
    HardMix,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl Composite {
    fn is_default(&self) -> bool {
        *self == Composite::SourceAlpha
    }

    /// Not a VEGAS mode.
    pub fn extra(self) -> bool {
        self as u8 >= Composite::SoftLight as u8
    }
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
    /// The Video Bus Track's envelopes (Fade to Color, Motion Blur Amount, Video Supersampling) and its Fade Colors.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub video_envelopes: Vec<Envelope>,
    pub fade_colors: [[u8; 3]; 2],
}

impl Default for MasterBus {
    fn default() -> Self {
        Self { gain: [1.0, 1.0], audio_fx: Vec::new(), video_fx: Vec::new(), video_envelopes: Vec::new(), fade_colors: fade_colors() }
    }
}

fn one() -> f64 {
    1.0
}

fn yes() -> bool {
    true
}

fn fade_colors() -> [[u8; 3]; 2] {
    [[255; 3], [0; 3]]
}

impl Project {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            path: None,
            settings: ProjectSettings::default(),
            timeline: Timeline::new(),
            media_pool: Vec::new(),
            notes: Vec::new(),
            modified: false,
        }
    }

    /// Writes a temp file and renames it over `path`, so a failed save never damages the old file.
    /// On error, name, path and modified are left unchanged.
    pub fn save(&mut self, path: &PathBuf) -> anyhow::Result<()> {
        // The project is named after its file, so titles and autosave backups are per-project.
        let name = file_stem(path).unwrap_or_else(|| self.name.clone());
        let old_name = std::mem::replace(&mut self.name, name);
        // Media paths are also kept relative to the project, so a moved folder or a new drive letter still finds them.
        let dir = path.parent().map(std::path::absolute).and_then(Result::ok);
        for m in &mut self.media_pool {
            m.rel_path = dir.as_deref().and_then(|d| relative(&m.path, d));
        }
        let res =serde_json::to_string_pretty(self).map_err(anyhow::Error::from).and_then(|json| write_atomic(path, &json));
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
        // Notepad and PowerShell may save the hand-edited file with a BOM.
        let mut project: Self = serde_json::from_str(json.trim_start_matches('\u{feff}'))?;
        if let Some(stem) = file_stem(path) {
            project.name = stem;
        }
        // Media not where it was saved: relative to the project as it was then, else next to the project file.
        if let Some(dir) = path.parent().map(std::path::absolute).and_then(Result::ok) {
            for m in project.media_pool.iter_mut().filter(|m| !m.path.as_os_str().is_empty() && !m.path.exists()) {
                let found = [m.rel_path.as_ref().map(|r| dir.join(r)), m.path.file_name().map(|f| dir.join(f))].into_iter().flatten().find(|p| p.exists());
                if let Some(p) = found {
                    m.path = p;
                }
            }
        }
        project.path = Some(path.clone());
        project.timeline.zoom = 1.0;
        Ok(project)
    }

    pub fn mark_modified(&mut self) {
        self.modified = true;
    }

    /// Where the event with `id` is now.
    pub fn find_clip(&self, id: &str) -> Option<ClipRef> {
        [true, false].into_iter().find_map(|is_video| {
            self.timeline.tracks(is_video).iter().enumerate().find_map(|(track_idx, t)| {
                t.clips.iter().position(|c| c.id == id).map(|clip_idx| ClipRef { is_video, track_idx, clip_idx })
            })
        })
    }

    /// `find_clip` for many ids in one sweep over the timeline, in the order of `ids` (missing ones skipped).
    pub fn find_clips(&self, ids: &[String]) -> Vec<ClipRef> {
        let want: std::collections::HashSet<&str> = ids.iter().map(String::as_str).collect();
        let mut at = std::collections::HashMap::with_capacity(ids.len());
        for is_video in [true, false] {
            for (track_idx, t) in self.timeline.tracks(is_video).iter().enumerate() {
                for (clip_idx, c) in t.clips.iter().enumerate().filter(|(_, c)| want.contains(c.id.as_str())) {
                    at.insert(c.id.as_str(), ClipRef { is_video, track_idx, clip_idx });
                }
            }
        }
        ids.iter().filter_map(|id| at.get(id.as_str()).cloned()).collect()
    }

    pub fn clip(&self, id: &str) -> Option<&Clip> {
        let r = self.find_clip(id)?;
        Some(&self.timeline.tracks(r.is_video)[r.track_idx].clips[r.clip_idx])
    }

    /// (is video, index) of the track with `id`.
    pub fn find_track(&self, id: &str) -> Option<(bool, usize)> {
        [true, false].into_iter().find_map(|v| self.timeline.tracks(v).iter().position(|t| t.id == id).map(|i| (v, i)))
    }
}

/// An event's place on the timeline; indices go stale after edits, so selections keep ids and look this up.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipRef {
    pub is_video: bool,
    pub track_idx: usize,
    pub clip_idx: usize,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            frame_rate: 29.97,
            sample_rate: 48000,
            ruler: Ruler::default(),
            resample: Resample::Smart,
            motion_blur: BlurType::Gaussian,
            field_order: FieldOrder::Progressive,
            par: 1.0,
            rotation: 0,
            // VEGAS: "Unless you have specific performance problems, choose Good"; Blend fields; 16-bit audio.
            render_quality: crate::decode::Quality::Good,
            deinterlace: Deinterlace::Blend,
            adjust_media: true,
            bit_depth: 16,
            summary: Summary::default(),
        }
    }
}

impl ProjectSettings {
    /// Timeline time `t` as the ruler shows it (its format, from its start time).
    pub fn ruler_time(&self, t: f64) -> String {
        format_time(t + self.ruler.offset, self.ruler.format, self)
    }

    /// A length (no start time) in the ruler's format; in Measures & Beats it counts from 0.0.000, not 1.1.000.
    pub fn ruler_len(&self, d: f64) -> String {
        match self.ruler.format {
            RulerFormat::MeasuresBeats => measures(d.abs(), self, 0),
            f => format_time(d, f, self),
        }
    }

    /// R16a: a length typed in the ruler's format (`ruler_len`'s inverse: Measures & Beats count from 0.0.000).
    pub fn parse_ruler_len(&self, text: &str) -> Option<f64> {
        match self.ruler.format {
            RulerFormat::MeasuresBeats => {
                let f: Vec<f64> = text.trim().split('.').map(|x| x.trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0)).collect::<Option<_>>()?;
                let (m, b, t) = (f[0], f.get(1).copied().unwrap_or(0.0), f.get(2).copied().unwrap_or(0.0));
                (f.len() <= 3).then(|| (m * self.ruler.beats_per_measure.max(1) as f64 + b + t / TICKS) * self.beat_secs())
            }
            f => parse_time(text, f, self),
        }
    }

    /// Ruler time typed in the ruler's format, as timeline time.
    pub fn parse_ruler_time(&self, text: &str) -> Option<f64> {
        parse_time(text, self.ruler.format, self).map(|v| v - self.ruler.offset)
    }

    /// Seconds per beat at the project tempo.
    pub fn beat_secs(&self) -> f64 {
        60.0 / self.ruler.tempo.max(1.0)
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
            record_dir: None,
            audio_follows_velocity: false,
            track_groups: Vec::new(),
            audio: AudioSettings::default(),
            commands: Vec::new(),
            cd_tracks: Vec::new(),
            cd_indexes: Vec::new(),
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
        let any_solo = self.any_solo(true);
        self.video_tracks.iter().filter(move |t| t.visible && self.plays(t, any_solo))
    }

    /// The track group track `id` is in.
    pub fn group_of(&self, id: &str) -> Option<&TrackGroup> {
        self.track_groups.iter().find(|g| g.tracks.iter().any(|t| t == id))
    }

    /// Track `t`'s (Mute, Solo) as playback sees them: a muted group silences its tracks over their own Solo, and a
    /// soloed group plays them over their own Mute (VEGAS).
    pub fn switches(&self, t: &Track) -> (bool, bool) {
        match self.group_of(&t.id) {
            Some(g) if g.muted => (true, false),
            Some(g) if g.solo => (false, true),
            _ => (t.muted, t.solo),
        }
    }

    /// Some track of that kind is soloed (its own Solo or its group's).
    pub fn any_solo(&self, video: bool) -> bool {
        self.tracks(video).iter().any(|t| self.switches(t).1)
    }

    /// Track `t` is heard (seen): not muted, and soloed when `any_solo` of its kind is.
    pub fn plays(&self, t: &Track, any_solo: bool) -> bool {
        let (muted, solo) = self.switches(t);
        !muted && (!any_solo || solo)
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

    pub fn add_track(&mut self, video: bool) {
        if video { self.add_video_track() } else { self.add_audio_track() }
    }
}

/// The app's Set Default Track Properties chain (`Prefs::default_track_fx`); None = the built-in one.
static DEFAULT_TRACK_FX: std::sync::Mutex<Option<Vec<Fx>>> = std::sync::Mutex::new(None);

pub fn set_default_track_fx(fx: Option<Vec<Fx>>) {
    *DEFAULT_TRACK_FX.lock().unwrap_or_else(|e| e.into_inner()) = fx;
}

/// The FX a new audio track starts with: `saved` with fresh instance ids, or (None) Track Noise Gate, Track EQ and
/// Track Compressor at neutral settings, as VEGAS gives a new audio track.
pub fn default_track_chain(saved: Option<&[Fx]>) -> Vec<Fx> {
    use crate::engine::plugins;
    match saved {
        // A track's FX automation is its own: the default chain takes the settings only.
        Some(s) => s.iter().map(|f| Fx { id: uuid::Uuid::new_v4().to_string(), auto: Default::default(), ..f.clone() }).collect(),
        None => ["milan.afx.gate", "milan.afx.eq", "milan.afx.compressor"].into_iter().filter_map(plugins::find).map(|i| plugins::new_fx(i, None)).collect(),
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
            fx: match kind {
                TrackKind::Audio => default_track_chain(DEFAULT_TRACK_FX.lock().unwrap_or_else(|e| e.into_inner()).as_deref()),
                TrackKind::Video => Vec::new(),
            },
            envelopes: Vec::new(),
            invert: false,
            fade_colors: fade_colors(),
            hidden: false,
            child: false,
            parent_mode: Composite::SourceAlpha,
            parent_motion: Keyframes::default(),
            compositor: None,
            shadow: Halo2D::default(),
            glow: Halo2D::default(),
            bypass_blur: false,
            out: Out::Master,
            sends: Vec::new(),
            pan_type: PanType::Balance0,
            automation: AutoMode::Read,
            auto_controls: false,
            monitor: Monitor::Off,
            send_view: None,
            armed: false,
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
            transition_out: None,
            envelopes: Vec::new(),
            group: None,
            takes: Vec::new(),
            take_no: 0,
            looping: true,
            invert: false,
            normalize: false,
            auto_normalize: false,
            normalized: None,
            channels: ChannelMap::Both,
            stream: 0,
            keep_aspect: true,
            reduce_flicker: false,
            resample: None,
            undersample: None,
            pitch_cents: 0.0,
            masks: Vec::new(),
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

    /// Event opacity (video), set by the level handle: the first key of its Opacity envelope, 1 without one.
    pub fn opacity(&self) -> f32 {
        self.envelopes.iter().find(|e| e.kind == EnvKind::Opacity).and_then(|e| e.points.keys.first()).map_or(1.0, |k| k.v)
    }

    /// Set the opacity as a single-key Opacity envelope (none at full opacity). Replaces any Opacity keys, so the
    /// level handle that calls it is hidden when the envelope has more than one.
    pub fn set_opacity(&mut self, v: f32) {
        self.envelopes.retain(|e| e.kind != EnvKind::Opacity);
        if v < 1.0 {
            let keys = vec![Keyframe { t: 0.0, v: v.max(0.0), curve: Curve::Linear }];
            self.envelopes.push(Envelope { kind: EnvKind::Opacity, points: Keyframes { keys } });
        }
    }

    /// Keep both fades inside the clip after a trim or fade drag.
    pub fn clamp_fades(&mut self) {
        let d = self.duration.max(0.0);
        self.fade_in = self.fade_in.clamp(0.0, d);
        self.fade_out = self.fade_out.clamp(0.0, d - self.fade_in);
    }
}

/// VEGAS's default "Smooth" fade curve, an S-curve from 0 to 1 (ffmpeg afade curve=hsin).
pub fn smooth_fade(x: f64) -> f64 {
    (1.0 - (std::f64::consts::PI * x).cos()) * 0.5
}

pub(crate) fn write_atomic(path: &Path, text: &str) -> anyhow::Result<()> {
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

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// The project rate as an exact fraction: NTSC rates (23.976, 29.97, 59.94...) are n×1000/1001, others are taken to
/// the thousandth. Preview, render, audio and the ruler all count frames on this one grid, so nothing drifts.
pub fn rate_ratio(fps: f64) -> (u64, u64) {
    let n = (fps * 1.001).round();
    if fps.fract() != 0.0 && (fps - n * 1000.0 / 1001.0).abs() < 0.01 {
        return (n as u64 * 1000, 1001);
    }
    let num = (fps.max(0.0) * 1000.0).round() as u64;
    let g = gcd(num, 1000).max(1);
    (num / g, 1000 / g)
}

/// `rate_ratio` as a number: 30000/1001 for 29.97.
pub fn exact_fps(fps: f64) -> f64 {
    let (n, d) = rate_ratio(fps);
    n as f64 / d as f64
}

/// Time of frame `n` at `fps`: n × den / num, exact to the f64.
pub fn frame_time(n: i64, fps: f64) -> f64 {
    let (num, den) = rate_ratio(fps);
    n as f64 * den as f64 / num.max(1) as f64
}

/// 29.97 and 59.94 count in drop-frame timecode.
fn is_drop(fps: f64) -> bool {
    let nominal = fps.round();
    fps.fract() != 0.0 && (nominal == 30.0 || nominal == 60.0)
}

/// VEGAS "Time & Frames": hh:mm:ss:ff, or SMPTE drop-frame hh:mm:ss;ff at 29.97/59.94 fps.
pub fn format_timecode(seconds: f64, fps: f64) -> String {
    smpte(seconds, exact_fps(fps), fps.round() as u64, is_drop(fps))
}

/// Inverse of `format_timecode`: "hh:mm:ss:ff" (drop-frame "hh:mm:ss;ff" at 29.97 and 59.94, as shown), the
/// shorter "mm:ss:ff" and "ss:ff", or plain seconds ("1.5"). None when it is not a time.
pub fn parse_timecode(s: &str, fps: f64) -> Option<f64> {
    parse_smpte(s, exact_fps(fps), fps.round() as u64, is_drop(fps))
}

pub(crate) fn parse_smpte(s: &str, rate: f64, nominal: u64, drop: bool) -> Option<f64> {
    let s = s.trim();
    if !s.contains([':', ';']) {
        return s.parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0);
    }
    let nominal = nominal as i64;
    let parts: Vec<i64> = s.split([':', ';']).map(|p| p.trim().parse().ok()).collect::<Option<_>>()?;
    if nominal <= 0 || parts.len() > 4 || parts.iter().any(|&p| p < 0) {
        return None;
    }
    let mut p = [0; 4];
    p[4 - parts.len()..].copy_from_slice(&parts);
    let [h, m, sec, ff] = p;
    let mins = h * 60 + m;
    let mut frames = (mins * 60 + sec) * nominal + ff;
    if drop {
        // Frame numbers 0..k are skipped at each minute except every tenth (SMPTE 12M).
        frames -= nominal / 15 * (mins - mins / 10);
    }
    Some(frames as f64 / rate)
}

/// Measures & Beats ticks per beat (ACID's and VEGAS's 768).
const TICKS: f64 = 768.0;

/// `t` seconds in ruler format `fmt` (no ruler offset: `ProjectSettings::ruler_time` adds it).
pub fn format_time(t: f64, fmt: RulerFormat, s: &ProjectSettings) -> String {
    use RulerFormat::*;
    if t < -1e-9 {
        return format!("-{}", format_time(-t, fmt, s));
    }
    let t = t.max(0.0);
    let fps = s.frame_rate.max(1.0);
    let frames = |rate: f64| (t * rate).round() as u64;
    match fmt {
        Samples => frames(s.sample_rate.max(1) as f64).to_string(),
        Time => {
            let ms = frames(1000.0);
            format!("{:02}:{:02}:{:02}.{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
        }
        Seconds => format!("{t:.3}"),
        AbsFrames => frames(exact_fps(fps)).to_string(),
        MeasuresBeats => measures(t, s, 1),
        Feet16 | Feet35 => {
            let (n, fpf) = (frames(exact_fps(fps)), if fmt == Feet16 { 40 } else { 16 });
            format!("{}+{:02}", n / fpf, n % fpf)
        }
        AudioCd => {
            let n = frames(75.0);
            format!("{:02}:{:02}:{:02}", n / 75 / 60, n / 75 % 60, n % 75)
        }
        _ => {
            let (rate, nominal, drop) = fmt.smpte(fps).expect("timecode format");
            smpte(t, rate, nominal, drop)
        }
    }
}

/// measure.beat.ticks, counting measures and beats from `base` (1 for a position, 0 for a length).
fn measures(t: f64, s: &ProjectSettings, base: u64) -> String {
    let ticks = (t / s.beat_secs() * TICKS).round() as u64;
    let (beat, bpm) = (ticks / TICKS as u64, s.ruler.beats_per_measure.max(1) as u64);
    format!("{}.{}.{:03}", beat / bpm + base, beat % bpm + base, ticks % TICKS as u64)
}

/// Inverse of `format_time`, also taking shortened forms ("1:30" in Time, "5" in Measures & Beats) and a leading
/// minus. None when the text is not a time in that format.
pub fn parse_time(text: &str, fmt: RulerFormat, s: &ProjectSettings) -> Option<f64> {
    use RulerFormat::*;
    let text = text.trim();
    if let Some(rest) = text.strip_prefix('-') {
        return parse_time(rest, fmt, s).map(|v| -v);
    }
    let num = |x: &str| x.trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0);
    // Up to `n` numbers split by `sep`, right-aligned (missing leading fields are 0).
    let fields = |sep: &[char], n: usize| -> Option<Vec<f64>> {
        let v: Vec<f64> = text.split(sep).map(num).collect::<Option<_>>()?;
        (v.len() <= n).then(|| [vec![0.0; n - v.len()], v].concat())
    };
    let fps = s.frame_rate.max(1.0);
    match fmt {
        Samples => num(text).map(|n| n / s.sample_rate.max(1) as f64),
        Seconds => num(text),
        AbsFrames => num(text).map(|n| n / exact_fps(fps)),
        Time => fields(&[':'], 3).map(|f| f[0] * 3600.0 + f[1] * 60.0 + f[2]),
        MeasuresBeats => {
            let mut f: Vec<f64> = text.split('.').map(num).collect::<Option<_>>()?;
            if f.len() > 3 || f.iter().take(2).any(|v| *v < 1.0) {
                return None;
            }
            f.resize(3, 0.0);
            f.iter_mut().take(2).for_each(|v| *v = v.max(1.0));
            let beats = (f[0] - 1.0) * s.ruler.beats_per_measure.max(1) as f64 + f[1] - 1.0 + f[2] / TICKS;
            Some(beats * s.beat_secs())
        }
        Feet16 | Feet35 => {
            let fpf = if fmt == Feet16 { 40.0 } else { 16.0 };
            fields(&['+'], 2).map(|f| (f[0] * fpf + f[1]) / exact_fps(fps))
        }
        AudioCd => fields(&[':'], 3).map(|f| (f[0] * 60.0 + f[1]) + f[2] / 75.0),
        _ => {
            let (rate, nominal, drop) = fmt.smpte(fps)?;
            parse_smpte(text, rate, nominal, drop)
        }
    }
}

/// hh:mm:ss:ff at `rate` frames per second counted as `nominal` (";" and SMPTE 12M numbering when `drop`).
fn smpte(seconds: f64, rate: f64, nominal: u64, drop: bool) -> String {
    if nominal == 0 {
        return "00:00:00:00".to_string();
    }
    let mut frames = (seconds.max(0.0) * rate).round() as u64;
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

    #[test]
    fn timecode_parses_what_format_shows() {
        for fps in [29.97, 25.0, 59.94, 23.976, 30.0] {
            for n in [0, 1, 29, 1799, 1800, 1801, 17982, 107892, 123456] {
                let t = frame_time(n, fps);
                let back = parse_timecode(&format_timecode(t, fps), fps).unwrap();
                assert!((back - t).abs() < 1e-9, "{fps} fps frame {n}: {} -> {back}", format_timecode(t, fps));
            }
        }
        assert_eq!(parse_timecode("1.5", 25.0), Some(1.5));
        assert_eq!(parse_timecode("02:10", 25.0), Some(2.4)); // ss:ff
        assert_eq!(parse_timecode("1:00:00", 25.0), Some(60.0)); // mm:ss:ff
        assert!(parse_timecode("abc", 25.0).is_none() && parse_timecode("1:-2", 25.0).is_none() && parse_timecode("-1", 25.0).is_none());
    }

    /// R1: every ruler format reads back what it shows, to within its own resolution, at 23.976, 25, 29.97 DF and
    /// 59.94 DF, 48 kHz; with a ruler start time too.
    #[test]
    fn every_ruler_format_round_trips() {
        for fps in [23.976, 25.0, 29.97, 59.94] {
            let mut s = ProjectSettings { frame_rate: fps, sample_rate: 48000, ..Default::default() };
            s.ruler.tempo = 97.0;
            s.ruler.beats_per_measure = 3;
            for fmt in RulerFormat::ALL {
                let res = match fmt {
                    RulerFormat::Samples => 1.0 / 48000.0,
                    RulerFormat::Time | RulerFormat::Seconds => 0.001,
                    RulerFormat::MeasuresBeats => s.beat_secs() / TICKS,
                    f => 1.0 / f.frame_rate(fps),
                };
                for n in [0, 1, 29, 1799, 1800, 17982, 107892, 323_676] {
                    let t = frame_time(n, fps);
                    let shown = format_time(t, fmt, &s);
                    let back = parse_time(&shown, fmt, &s).unwrap_or_else(|| panic!("{fmt:?} {fps}: {shown}"));
                    assert!((back - t).abs() <= res / 2.0 + 1e-9, "{fmt:?} {fps} frame {n}: {shown} -> {back} (t {t})");
                    assert_eq!(format_time(back, fmt, &s), shown, "{fmt:?} {fps} frame {n}");
                    // From a ruler start time, and as a negative length.
                    (s.ruler.offset, s.ruler.format) = (3600.0, fmt);
                    let back = s.parse_ruler_time(&s.ruler_time(t));
                    assert!(back.is_some_and(|b| (b - t).abs() <= res / 2.0 + 1e-9), "{fmt:?} {fps} frame {n} offset: {} -> {back:?}", s.ruler_time(t));
                    s.ruler.offset = 0.0;
                    assert!((parse_time(&format_time(-t, fmt, &s), fmt, &s).unwrap() + t).abs() <= res / 2.0 + 1e-9);
                }
                assert!(parse_time("x", fmt, &s).is_none() && parse_time("", fmt, &s).is_none(), "{fmt:?}");
            }
        }
        let s = ProjectSettings::default(); // 29.97, 120 bpm in 4/4
        let f = |fmt, t| format_time(t, fmt, &s);
        assert_eq!(f(RulerFormat::Samples, 1.5), "72000");
        assert_eq!(f(RulerFormat::Time, 3725.25), "01:02:05.250");
        assert_eq!(f(RulerFormat::Seconds, 2.5), "2.500");
        assert_eq!(f(RulerFormat::AbsFrames, frame_time(150, 29.97)), "150");
        assert_eq!(f(RulerFormat::MeasuresBeats, 2.25), "2.1.384"); // 4.5 beats: measure 2, beat 1 and a half
        assert_eq!(f(RulerFormat::Feet16, frame_time(85, 29.97)), "2+05");
        assert_eq!(f(RulerFormat::Feet35, frame_time(85, 29.97)), "5+05");
        assert_eq!(f(RulerFormat::SmpteEbu, 10.0), "00:00:10:00");
        assert_eq!(f(RulerFormat::SmpteNonDrop, 60.0), "00:00:59:28");
        assert_eq!(f(RulerFormat::SmpteDrop, 60.0), "00:00:59;28"); // drop frame meets the clock every 10 min
        assert_eq!(f(RulerFormat::SmpteDrop, 600.0), "00:10:00;00");
        assert_eq!(f(RulerFormat::Smpte30, 1.5), "00:00:01:15");
        assert_eq!(f(RulerFormat::SmpteFilm, 1.5), "00:00:01:12");
        assert_eq!(f(RulerFormat::AudioCd, 61.4), "01:01:30");
        assert_eq!(parse_time("3", RulerFormat::MeasuresBeats, &s), Some(4.0));
        let mut m = s.clone();
        m.ruler.format = RulerFormat::MeasuresBeats;
        assert_eq!((m.ruler_time(3.0), m.ruler_len(3.0)), ("2.3.000".to_string(), "1.2.000".to_string())); // 6 beats
        assert_eq!(parse_time("1:30", RulerFormat::Time, &s), Some(90.0));
    }

    /// R1: 29.97 is 30000/1001 exactly, so frame n lands on n after hours, and an hour holds 107892 frames.
    #[test]
    fn exact_rates_do_not_drift() {
        assert_eq!([rate_ratio(29.97), rate_ratio(23.976), rate_ratio(59.94), rate_ratio(25.0), rate_ratio(12.5)], [(30000, 1001), (24000, 1001), (60000, 1001), (25, 1), (25, 2)]);
        assert_eq!(rate_ratio(exact_fps(29.97)), (30000, 1001));
        for n in [0, 1, 107_892, 323_676, 323_677, 1_000_003] {
            let t = frame_time(n, 29.97); // 323 676 is 3 h
            assert!((t * 30000.0 / 1001.0 - n as f64).abs() < 1e-6, "{n}");
            assert_eq!(crate::engine::plan::frame_at(t, exact_fps(29.97)), n);
        }
        assert_eq!(format_timecode(frame_time(323_676, 29.97), 29.97), "03:00:00;00");
        // A render of 1 h at 29.97 writes 107892 frames; the 3 h point is frame 323 676 of the video plan.
        assert_eq!(crate::engine::plan::frame_range((0.0, 3600.0), 29.97), (0, 107_892));
        assert_eq!(crate::engine::plan::frame_range((0.0, 10_800.0), 29.97).1, 323_676);
        let mut p = Project::new("t");
        p.timeline.video_tracks[0].clips.push(Clip::new("c", 0.0, 1.0));
        assert_eq!(crate::engine::plan::compile(&p).fps, 30000.0 / 1001.0);
    }

    // Fade gain curves are tested with the mixer (engine/mix.rs) and the video plan.
    #[test]
    fn clamp_fades_keeps_both_inside_the_event() {
        let mut c = Clip::new("c", 0.0, 10.0);
        (c.fade_in, c.fade_out) = (2.0, 4.0);
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
        strip(&mut v["timeline"], &["loop_region", "master", "record_dir"]);
        strip(&mut v["settings"], &["ruler", "resample", "field_order", "par", "rotation", "render_quality", "deinterlace", "adjust_media", "bit_depth"]);
        strip(&mut v["timeline"]["video_tracks"][0], &["composite", "motion", "fx", "envelopes", "invert", "fade_colors"]);
        strip(&mut v["timeline"]["audio_tracks"][0], &["fx", "invert", "fade_colors"]);
        let clip_keys = ["fade_in", "gain_db", "fade_in_curve", "fade_out_curve", "rate", "reverse", "muted", "pan_crop", "fx", "transition", "envelopes", "group", "takes", "looping"];
        let r3b_keys = ["invert", "normalize", "auto_normalize", "channels", "stream", "keep_aspect", "reduce_flicker", "resample"];
        strip(&mut v["timeline"]["video_tracks"][0]["clips"][0], &[&clip_keys[..], &r3b_keys].concat());
        strip(&mut v["media_pool"][0], &["has_alpha", "rotation", "color_matrix", "interlaced", "generator", "audio_streams", "video_streams"]);
        let back: Project = serde_json::from_value(v).unwrap();
        // R1: an old project opens at 120 bpm in 4/4, Time & Frames from 0.
        assert_eq!(back.settings.ruler, Ruler { format: RulerFormat::TimeFrames, offset: 0.0, tempo: 120.0, beats_per_measure: 4, beat_note: 4 });
        let r: Ruler = serde_json::from_str(r#"{"format":"Bars","tempo":90}"#).unwrap(); // a format from a newer build
        assert_eq!((r.format, r.tempo, r.beats_per_measure), (RulerFormat::TimeFrames, 90.0, 4));
        let tl = &back.timeline;
        assert_eq!((tl.loop_region, tl.record_dir.as_ref()), (None, None));
        assert_eq!(tl.master.gain, [1.0, 1.0]);
        assert!(tl.master.audio_fx.is_empty() && tl.master.video_fx.is_empty());
        let t = &tl.video_tracks[0];
        assert_eq!(t.composite, Composite::SourceAlpha);
        assert!(t.motion.keys.is_empty() && t.fx.is_empty() && t.envelopes.is_empty());
        assert!(tl.audio_tracks[0].fx.is_empty(), "an old audio track keeps its empty chain");
        let c = &t.clips[0];
        assert_eq!(c.fade_in, 0.0);
        assert_eq!((c.rate, c.gain_db, c.reverse, c.muted), (1.0, 0.0, false, false));
        assert_eq!((c.fade_in_curve, c.fade_out_curve), (Curve::Smooth, Curve::Smooth));
        assert!(c.group.is_none() && c.takes.is_empty() && c.fx.is_empty() && c.transition.is_none());
        // R17b: the active take is the first in the take order, and that is not written.
        assert!(c.take_no == 0 && !serde_json::to_string(c).unwrap().contains("take_no"));
        assert!(c.pan_crop.keys.is_empty() && c.envelopes.is_empty());
        assert!(c.looping, "R3a: an old event loops, as VEGAS events do by default");
        // R3b: switches off (Maintain Aspect Ratio on), both channels, the first stream, the project resample mode;
        // the project never resampled before, so it opens with resampling off.
        assert!(!c.invert && !c.normalize && !c.auto_normalize && c.normalized.is_none() && c.keep_aspect && !c.reduce_flicker);
        assert_eq!((c.channels, c.stream, c.resample, c.undersample), (ChannelMap::Both, 0, None, None));
        assert!(!t.invert && !tl.audio_tracks[0].invert);
        // R4a: Fade Colors white over black (VEGAS's), no transition out, no velocity.
        assert_eq!(t.fade_colors, [[255, 255, 255], [0, 0, 0]]);
        assert!(c.transition_out.is_none() && crate::engine::velocity::of(c).is_none());
        // R4b: no pitch change, and none written; audio does not follow video velocity (VEGAS), and that is not written.
        assert!(c.pitch_cents == 0.0 && !tl.audio_follows_velocity);
        // R5: no track groups and no hidden tracks, and neither is written.
        assert!(tl.track_groups.is_empty() && !t.hidden && !tl.audio_tracks[0].hidden);
        let v = serde_json::to_value(&p).unwrap();
        assert!(v["timeline"].get("track_groups").is_none() && v["timeline"]["video_tracks"][0].get("hidden").is_none());
        let g: TrackGroup = serde_json::from_str(r#"{"id":"g","name":"Drums","tracks":["t"]}"#).unwrap();
        assert!(!g.collapsed && !g.muted && !g.solo);
        assert!(v["timeline"].get("audio_follows_velocity").is_none());
        assert!(v["timeline"]["video_tracks"][0]["clips"][0].get("transition_out").is_none(), "nothing written without one");
        assert!(v["timeline"]["video_tracks"][0]["clips"][0].get("pitch_cents").is_none());
        let tr: Transition = serde_json::from_str(r#"{"plugin":"milan.tr.dissolve"}"#).unwrap();
        assert!(tr.progress.is_empty() && serde_json::to_value(&tr).unwrap().get("progress").is_none(), "an old transition is linear");
        assert_eq!(back.settings.resample, Resample::Disable);
        // R11: progressive, square, unrotated, adjusted, 16-bit; it renders at Best and deinterlaces with yadif, as before.
        let s = &back.settings;
        assert_eq!((s.field_order, s.par, s.rotation, s.adjust_media, s.bit_depth), (FieldOrder::Progressive, 1.0, 0, true, 16));
        assert_eq!((s.render_quality, s.deinterlace), (crate::decode::Quality::Best, Deinterlace::Interpolate));
        let n = Project::new("n").settings;
        assert_eq!((n.render_quality, n.deinterlace), (crate::decode::Quality::Good, Deinterlace::Blend), "VEGAS's defaults");
        let s: ProjectSettings = serde_json::from_str(r#"{"width":2,"height":2,"frame_rate":25,"sample_rate":48000,"render_quality":"Ultra","deinterlace":"Smart"}"#).unwrap();
        assert_eq!((s.render_quality, s.deinterlace), (crate::decode::Quality::Best, Deinterlace::Blend), "values from a newer build");
        assert_eq!(Project::new("n").settings.resample, Resample::Smart, "a new project resamples smartly, as VEGAS 16");
        let m = &back.media_pool[0];
        assert!(!m.has_alpha && !m.interlaced && m.rotation == 0 && m.color_matrix.is_none() && m.generator.is_none());
        assert!(m.subclip.is_none() && m.file_time(2.0) == 2.0);
        assert_eq!((m.audio_streams, m.streams(false)), (0, 1));
        assert!(serde_json::to_value(&p).unwrap()["media_pool"][0].get("subclip").is_none(), "nothing written for a whole file");
        // R6a: media is a file, not a nested timeline, and that is not written.
        assert!(!m.nested && serde_json::to_value(&p).unwrap()["media_pool"][0].get("nested").is_none());
        // R17a: media imported before it has no timecode or creation time (Synchronize skips it, the date falls back on
        // the file), and neither is written.
        assert!(m.timecode.is_none() && m.created.is_none());
        assert!(["timecode", "created"].iter().all(|k| serde_json::to_value(&p).unwrap()["media_pool"][0].get(*k).is_none()));
        // R7: no masks, no parent/child compositing, no Custom compositor, 2D Shadow and Glow off, motion blur on, no
        // Video Bus Track envelopes (white over black Fade Colors), Gaussian motion blur, 2D Track Motion; none written.
        assert!(c.masks.is_empty() && !t.child && t.parent_mode == Composite::SourceAlpha && t.parent_motion.is_empty() && t.compositor.is_none());
        assert!(!t.shadow.on && !t.glow.on && !t.bypass_blur && tl.master.video_envelopes.is_empty());
        assert_eq!((tl.master.fade_colors, back.settings.motion_blur), ([[255, 255, 255], [0, 0, 0]], BlurType::Gaussian));
        let vt = &v["timeline"]["video_tracks"][0];
        for k in ["child", "parent_mode", "parent_motion", "compositor", "shadow", "glow", "bypass_blur"] {
            assert!(vt.get(k).is_none(), "{k} written");
        }
        assert!(vt["clips"][0].get("masks").is_none() && v["timeline"]["master"].get("video_envelopes").is_none());
        let m: Motion = serde_json::from_str(r#"{"x":0.1,"y":0,"w":1,"h":1,"rot":0}"#).unwrap();
        assert_eq!((m.z, m.rx, m.ry), (0.0, 0.0, 0.0));
        assert!(serde_json::to_value(Motion::default()).unwrap().get("z").is_none());
        let mk: Mask = serde_json::from_str(r#"{"path":{"keys":[]}}"#).unwrap();
        assert_eq!((mk.mode, mk.opacity, mk.feather), (MaskMode::Positive, 1.0, 0.0));
        // R8a: no busses or assignable FX; every track to the master with no sends, Balance (0 dB Center); none written.
        let a = &tl.audio_tracks[0];
        assert!(tl.audio == AudioSettings::default() && a.out == Out::Master && a.sends.is_empty() && a.pan_type == PanType::Balance0);
        assert!(v["timeline"].get("audio").is_none());
        for k in ["out", "sends", "pan_type"] {
            assert!(v["timeline"]["audio_tracks"][0].get(k).is_none(), "{k} written");
        }
        let b: Bus = serde_json::from_str(r#"{"id":"b","name":"Bus A"}"#).unwrap();
        assert!(b.volume == 1.0 && b.pan == 0.0 && !b.muted && b.fx.is_empty() && b.envelopes.is_empty() && b.out == Out::Master);
        // R8c: automation Read with trim controls (envelopes play, as they always did), no FX automation; none written.
        assert!(a.automation == AutoMode::Read && !a.auto_controls && b.automation == AutoMode::Read && !b.auto_controls);
        assert!(a.fx.iter().chain(&t.fx).all(|f| f.auto.is_empty()));
        for k in ["automation", "auto_controls"] {
            assert!(v["timeline"]["audio_tracks"][0].get(k).is_none(), "{k} written");
        }
        let f: Fx = serde_json::from_str(r#"{"id":"f","plugin":"milan.afx.eq"}"#).unwrap();
        assert!(f.auto.is_empty() && serde_json::to_value(&f).unwrap().get("auto").is_none());
        // An automation mode from a newer build loads as Read; FxParam is never written in an envelope list, and an
        // old build reads a kind it does not know as Unknown.
        let t2: Track = serde_json::from_value({
            let mut t = serde_json::to_value(&tl.audio_tracks[0]).unwrap();
            t["automation"] = "Trim".into();
            t
        })
        .unwrap();
        assert_eq!(t2.automation, AutoMode::Read);
        // R9: no input busses and input monitoring off; neither written.
        assert!(tl.audio.inputs.is_empty() && a.monitor == Monitor::Off && v["timeline"]["audio_tracks"][0].get("monitor").is_none());
        // R16a: no Project Notes and an empty Summary; neither written.
        assert!(back.notes.is_empty() && back.settings.summary == Summary::default());
        assert!(v.get("notes").is_none() && v["settings"].get("summary").is_none());
        let n: Note = serde_json::from_str(r#"{"text":"x"}"#).unwrap();
        assert_eq!(n, Note { at: 0.0, text: "x".into(), resolved: false });
        // R18a: no command markers, CD track regions or indexes, and no media markers; none written.
        assert!(tl.commands.is_empty() && tl.cd_tracks.is_empty() && tl.cd_indexes.is_empty() && !tl.cd_layout() && back.media_pool[0].markers.is_empty());
        assert!(["commands", "cd_tracks", "cd_indexes"].iter().all(|k| v["timeline"].get(*k).is_none()) && v["media_pool"][0].get("markers").is_none());
        let cd: CdTrack = serde_json::from_str(r#"{"start":1,"end":2}"#).unwrap();
        assert!(cd.name.is_empty() && cd.isrc.is_empty() && !cd.protect && !cd.emphasis);
    }

    /// R16a: Project Notes and the Summary survive save and load.
    #[test]
    fn notes_and_summary_survive_save_and_load() {
        let dir = temp_dir("notes");
        let path = dir.join("notes.mnle");
        let mut p = Project::new("t");
        p.notes = vec![Note { at: 2.5, text: "Swap the B-roll\nand fix the fade".into(), resolved: false }, Note { at: 0.0, text: "Done".into(), resolved: true }];
        p.settings.summary = Summary { title: "Show".into(), artist: "Me".into(), comments: "Two\nlines".into(), ..Default::default() };
        p.save(&path).unwrap();
        let back = Project::load(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&dir).unwrap();
        assert_eq!((back.notes, back.settings.summary), (p.notes, p.settings.summary));
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
            auto: BTreeMap::from([("gain".to_string(), keys(-3.0))]),
        };
        let env = |kind| Envelope { kind, points: keys(-6.0) };
        let mut c = Clip::new("c", 1.0, 4.0);
        c.gain_db = -3.5;
        (c.fade_in_curve, c.fade_out_curve) = (Curve::Fast, Curve::Slow);
        (c.rate, c.reverse, c.muted) = (0.5, true, true);
        c.pan_crop = Keyframes { keys: vec![Keyframe { t: 0.5, v: PanCrop { cx: 0.25, cy: 0.75, w: -0.5, h: 0.5, rot: 30.0 }, curve: Curve::Linear }] };
        c.fx = vec![fx("milan.fx.blur")];
        c.transition = Some(Transition { plugin: "milan.tr.dissolve".into(), params: BTreeMap::from([("soft".to_string(), 0.5)]), progress: keys(0.25) });
        c.transition_out = Some(Transition { plugin: "milan.tr.linear_wipe".into(), ..Default::default() });
        c.envelopes = vec![env(EnvKind::Volume), env(EnvKind::Velocity)];
        c.group = Some("g1".into());
        c.takes = vec![Take { media_id: "m2".into(), in_point: 2.0, name: "Take 2".into() }];
        c.take_no = 1;
        c.looping = false;
        (c.invert, c.normalize, c.auto_normalize, c.channels, c.stream) = (true, true, true, ChannelMap::Combine, 2);
        c.normalized = Some(Normalized { db: 4.5, span: NormSpan { media: Some("m".into()), stream: 2, channels: ChannelMap::Combine, in_point: 1.0, duration: 4.0, rate: 0.5 } });
        (c.keep_aspect, c.reduce_flicker, c.resample, c.undersample) = (false, true, Some(Resample::OpticalFlow), Some(0.25));
        c.pitch_cents = -1250.0;
        let t = &mut p.timeline.video_tracks[0];
        t.composite = Composite::Screen;
        t.motion = Keyframes { keys: vec![Keyframe { t: 0.0, v: Motion { x: 0.1, y: -0.2, w: 0.5, h: 0.5, rot: 45.0, ..Default::default() }, curve: Curve::Smooth }] };
        t.fx = vec![fx("milan.fx.grade")];
        t.envelopes = vec![env(EnvKind::CompositeLevel), env(EnvKind::Opacity), env(EnvKind::Mute), env(EnvKind::FadeToColor)];
        t.fade_colors = [[10, 20, 30], [200, 100, 50]];
        t.clips.push(c);
        p.timeline.audio_tracks[0].envelopes = vec![env(EnvKind::Pan), env(EnvKind::Mute)];
        p.timeline.audio_tracks[0].invert = true;
        p.settings.resample = Resample::Force;
        p.timeline.master = MasterBus {
            gain: [0.5, 0.75],
            audio_fx: vec![fx("milan.afx.eq")],
            video_fx: vec![fx("milan.fx.lut")],
            video_envelopes: vec![env(EnvKind::MotionBlur), env(EnvKind::Supersampling), env(EnvKind::FadeToColor)],
            fade_colors: [[1, 2, 3], [4, 5, 6]],
        };
        p.settings.motion_blur = BlurType::AsymmetricPyramid;
        {
            let c = &mut p.timeline.video_tracks[0].clips[0];
            let path = Keyframes { keys: vec![Keyframe { t: 0.0, v: vec![[0.1, 0.2, 0.0, 0.0, 0.05, 0.0], [0.5, 0.9, -0.1, 0.0, 0.1, 0.0]], curve: Curve::Linear }] };
            c.masks = vec![Mask { path, mode: MaskMode::Negative, opacity: 0.5, feather: 0.02 }];
            let t = &mut p.timeline.video_tracks[0];
            (t.child, t.parent_mode, t.bypass_blur, t.composite) = (true, Composite::Cut, true, Composite::Custom);
            t.parent_motion = Keyframes { keys: vec![Keyframe { t: 1.0, v: Motion { z: 0.5, rx: 10.0, ry: -20.0, ..Default::default() }, curve: Curve::Linear }] };
            t.compositor = Some(fx("milan.comp.displacement"));
            t.shadow = Halo2D { on: true, keys: Keyframes { keys: vec![Keyframe { t: 0.0, v: Halo { x: 0.1, ..Halo::glow() }, curve: Curve::Linear }] } };
            t.glow.on = true;
        }
        p.timeline.record_dir = Some(PathBuf::from(r"D:\Takes"));
        p.timeline.audio_follows_velocity = true;
        p.timeline.audio_tracks[0].hidden = true;
        let ids = vec![p.timeline.video_tracks[0].id.clone(), p.timeline.audio_tracks[0].id.clone()];
        p.timeline.track_groups.push(TrackGroup { id: "g".into(), name: "Band".into(), tracks: ids, collapsed: true, muted: true, solo: true });
        let mut bus = Bus::new("Bus A".into(), vec![fx("milan.afx.reverb")]);
        (bus.volume, bus.pan, bus.muted, bus.envelopes) = (0.5, -0.25, true, vec![env(EnvKind::Volume)]);
        let mut fxb = Bus::new("FX 1".into(), Vec::new());
        fxb.out = Out::Bus(bus.id.clone());
        (bus.automation, bus.auto_controls) = (AutoMode::Latch, true);
        let a = &mut p.timeline.audio_tracks[0];
        (a.out, a.pan_type) = (Out::Bus(bus.id.clone()), PanType::ConstantPower);
        (a.automation, a.auto_controls) = (AutoMode::Off, true);
        a.sends = vec![AudioSend { to: fxb.id.clone(), level: 0.25, pre: true }];
        a.monitor = Monitor::Auto;
        let mut inb = Bus::new("Input Bus A".into(), vec![fx("milan.afx.gate")]);
        inb.out = Out::Bus(bus.id.clone());
        p.timeline.audio = AudioSettings { buses: vec![bus], fx: vec![fxb], inputs: vec![inb] };
        p.settings.ruler = Ruler { format: RulerFormat::MeasuresBeats, offset: 3600.0, tempo: 92.5, beats_per_measure: 3, beat_note: 8 };
        p.media_pool.push(MediaItem {
            id: "g".into(),
            media_type: MediaType::Image,
            duration: 86400.0,
            has_alpha: true,
            rotation: 270,
            color_matrix: Some("bt709".into()),
            interlaced: true,
            generator: Some(fx("milan.gen.text")),
            subclip: Some(Subclip { parent: "m".into(), start: 1.25 }),
            audio_streams: 3,
            video_streams: 2,
            timecode: Some(3600.5),
            created: Some(1_714_566_896.25),
            markers: vec![(1.5, "Chorus".into())],
            ..Default::default()
        });
        p.timeline.commands = vec![CommandMarker { position: 2.0, command: "URL".into(), parameter: "https://example.com".into(), comment: "Shop".into() }];
        p.timeline.cd_tracks = vec![CdTrack { start: 2.0, end: 5.0, name: "Intro".into(), isrc: "USRC17607839".into(), protect: true, emphasis: true }];
        p.timeline.cd_indexes = vec![3.5];
        assert_eq!(p.media_pool[0].file_time(0.5), 1.75);
        let json = serde_json::to_string(&p).unwrap();
        let back: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(serde_json::to_value(&back).unwrap(), serde_json::to_value(&p).unwrap());
        assert_eq!(back.timeline.video_tracks[0].clips[0], p.timeline.video_tracks[0].clips[0]);
        assert_eq!(back.timeline.video_tracks[0].fade_colors, [[10, 20, 30], [200, 100, 50]]);
        assert_eq!(back.timeline.video_tracks[0].envelopes[3].kind, EnvKind::FadeToColor);
        assert_eq!(back.timeline.record_dir, p.timeline.record_dir);
        assert!(back.timeline.audio_follows_velocity);
        assert!(back.timeline.audio_tracks[0].hidden && back.timeline.track_groups == p.timeline.track_groups);
        let (a, b) = (&back.timeline.audio_tracks[0], &p.timeline.audio_tracks[0]);
        assert!(back.timeline.audio == p.timeline.audio && a.out == b.out && a.sends == b.sends && a.pan_type == b.pan_type);
        assert!(a.automation == AutoMode::Off && a.auto_controls && back.timeline.master.audio_fx[0].auto == p.timeline.master.audio_fx[0].auto);
        assert!(a.monitor == Monitor::Auto && back.timeline.audio.inputs.len() == 1);
        assert_eq!(back.settings, p.settings);
        assert_eq!((back.media_pool[0].timecode, back.media_pool[0].created), (Some(3600.5), Some(1_714_566_896.25)));
        assert_eq!((&back.timeline.commands, &back.timeline.cd_tracks, &back.timeline.cd_indexes, &back.media_pool[0].markers), (&p.timeline.commands, &p.timeline.cd_tracks, &p.timeline.cd_indexes, &p.media_pool[0].markers));
        // A resample mode or channel map from a newer build loads as the default.
        let c: Clip = serde_json::from_str(&serde_json::to_string(&p.timeline.video_tracks[0].clips[0]).unwrap().replace("OpticalFlow", "Warp").replace(r#""channels":"Combine","stream""#, r#""channels":"Mid","stream""#)).unwrap();
        assert_eq!((c.resample, c.channels), (None, ChannelMap::Both));
    }

    /// Arming is for this session: it is never written, and a loaded track is not armed.
    #[test]
    fn armed_is_not_saved() {
        let mut p = Project::new("t");
        p.timeline.audio_tracks[0].armed = true;
        let v = serde_json::to_value(&p).unwrap();
        assert!(v["timeline"]["audio_tracks"][0].get("armed").is_none());
        let back: Project = serde_json::from_value(v).unwrap();
        assert!(!back.timeline.audio_tracks[0].armed);
    }

    /// R8a: the pan types at centre and hard right, as VEGAS describes them.
    #[test]
    fn pan_type_laws() {
        let near = |a: f32, b: f32| (a - b).abs() < 1e-3;
        let m = |t: PanType, p| t.matrix(p);
        assert_eq!(m(PanType::Balance0, 0.0), [[1.0, 0.0], [0.0, 1.0]]);
        assert_eq!(m(PanType::Balance0, 0.5), [[0.5, 0.0], [0.0, 1.0]]);
        let c3 = m(PanType::Balance3, 0.0);
        assert!(near(c3[0][0], 0.708) && near(c3[1][1], 0.708));
        assert_eq!(m(PanType::Balance6, 1.0), [[0.0, 0.0], [0.0, 1.0]]);
        assert!(near(m(PanType::Balance6, 0.0)[0][0], 0.501) && near(m(PanType::Balance6, -0.5)[0][0], 0.75));
        // Add Channels: the left channel moves into the right as the pan goes right.
        assert_eq!(m(PanType::AddChannels, 0.0), [[1.0, 0.0], [0.0, 1.0]]);
        assert_eq!(m(PanType::AddChannels, 1.0), [[0.0, 0.0], [1.0, 1.0]]);
        assert_eq!(m(PanType::AddChannels, -0.5), [[1.0, 0.5], [0.0, 0.5]]);
        // Constant power: -3 dB each at centre, the power the same everywhere; Film is the same in stereo.
        for p in [-1.0, -0.3, 0.0, 0.6, 1.0] {
            let k = m(PanType::ConstantPower, p);
            assert!(near(k[0][0].powi(2) + k[1][1].powi(2), 1.0) && k == m(PanType::Film, p));
        }
        assert!(near(m(PanType::ConstantPower, 0.0)[0][0], 0.7071));
    }

    #[test]
    fn new_audio_tracks_get_the_default_chain() {
        let t = Track::new("A", TrackKind::Audio);
        let ids: Vec<&str> = t.fx.iter().map(|f| f.plugin.as_str()).collect();
        assert_eq!(ids, ["milan.afx.gate", "milan.afx.eq", "milan.afx.compressor"]);
        // Neutral: each plug-in's DSP would pass the signal through.
        for f in &t.fx {
            let p = crate::engine::plugins::eval(f, 0.0).unwrap();
            assert!(crate::engine::afx::new(&f.plugin).unwrap().neutral(&p), "{}", f.plugin);
        }
        assert!(Track::new("V", TrackKind::Video).fx.is_empty());
        // A saved chain comes back with new instance ids (each track's FX state is its own).
        let saved = default_track_chain(Some(&t.fx));
        assert!(saved.iter().zip(&t.fx).all(|(a, b)| a.id != b.id && Fx { id: b.id.clone(), ..a.clone() } == *b));
        assert!(default_track_chain(Some(&[])).is_empty());
    }

    #[test]
    fn opacity_round_trips_through_a_single_key() {
        let mut c = Clip::new("c", 0.0, 1.0);
        assert_eq!(c.opacity(), 1.0);
        c.set_opacity(0.481);
        assert_eq!((c.opacity(), c.envelopes.len(), c.envelopes[0].points.keys.len()), (0.481, 1, 1));
        c.set_opacity(0.25);
        assert_eq!((c.opacity(), c.envelopes.len()), (0.25, 1));
        c.set_opacity(1.0); // full opacity needs no envelope
        assert!(c.envelopes.is_empty() && c.opacity() == 1.0);
    }

    #[test]
    fn find_clip_survives_a_reorder() {
        let mut p = Project::new("t");
        p.timeline.add_video_track();
        let (a, b) = (Clip::new("a", 0.0, 1.0), Clip::new("b", 2.0, 1.0));
        let id = b.id.clone();
        p.timeline.video_tracks[1].clips = vec![a, b];
        assert_eq!(p.find_clip(&id), Some(ClipRef { is_video: true, track_idx: 1, clip_idx: 1 }));
        p.timeline.video_tracks.swap(0, 1);
        p.timeline.video_tracks[0].clips.swap(0, 1);
        assert_eq!(p.find_clip(&id), Some(ClipRef { is_video: true, track_idx: 0, clip_idx: 0 }));
        assert_eq!(p.clip(&id).map(|c| c.name.as_str()), Some("b"));
        let tid = p.timeline.audio_tracks[0].id.clone();
        assert_eq!((p.find_track(&tid), p.find_clip("nope")), (Some((false, 0)), None));
        let a_id = p.timeline.video_tracks[0].clips[1].id.clone();
        let refs = p.find_clips(&[a_id.clone(), "nope".into(), id.clone()]);
        assert_eq!(refs, [p.find_clip(&a_id).unwrap(), p.find_clip(&id).unwrap()]);
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

    /// The project folder copied elsewhere: its media is found relative to the project; a BOM does not stop Open.
    #[test]
    fn a_moved_project_finds_its_media() {
        let dir = temp_dir("move");
        std::fs::create_dir_all(dir.join("old/footage")).unwrap();
        std::fs::write(dir.join("old/footage/a.mp4"), b"x").unwrap();
        let mut p = Project::new("t");
        p.media_pool.push(MediaItem { id: "a".into(), path: dir.join("old/footage/a.mp4"), ..Default::default() });
        p.save(&dir.join("old/Show.mnle")).unwrap();
        std::fs::rename(dir.join("old"), dir.join("new")).unwrap();
        let json = std::fs::read_to_string(dir.join("new/Show.mnle")).unwrap();
        std::fs::write(dir.join("new/Show.mnle"), format!("\u{feff}{json}")).unwrap();
        let back = Project::load(&dir.join("new/Show.mnle")).unwrap();
        let found = back.media_pool[0].path.clone();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(found, dir.join("new").join("footage").join("a.mp4"));
        assert_eq!(relative(Path::new(r"D:\Footage\a.mp4"), Path::new(r"D:\Projects")), Some(PathBuf::from(r"..\Footage\a.mp4")));
    }

    /// A compositing mode or curve from a newer Milan opens as the default instead of failing the project.
    #[test]
    fn newer_variants_fall_back() {
        #[derive(Deserialize)]
        struct T {
            #[serde(deserialize_with = "lenient")]
            c: Composite,
        }
        let c = serde_json::from_str::<T>(r#"{"c":"LinearLight"}"#).unwrap().c;
        let k = serde_json::from_str::<Keyframe<f32>>(r#"{"t":0,"v":1,"curve":"Bezier"}"#).unwrap().curve;
        let m: MediaType = serde_json::from_str(r#""Vector""#).unwrap();
        assert_eq!((c, k, m), (Composite::SourceAlpha, Curve::Smooth, MediaType::Unknown));
        assert_eq!(serde_json::from_str::<T>(r#"{"c":"Screen"}"#).unwrap().c, Composite::Screen);
    }
}
