//! File › Properties (§5.15.1 Project Properties), applied through undo history, and the app preferences file
//! (%APPDATA%\MilanPro\prefs.json).
use eframe::egui::{self, Ui};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use crate::history::History;
use crate::project::{Project, ProjectSettings};
use crate::ui::dialogs;
use crate::ui::dock::Layout;
use crate::ui::menus::{Later, NotPlanned, Why};
use crate::ui::theme::{self, LookChoice, UiLook};

/// Per-user app preferences. Unknown or missing fields fall back to defaults.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub look: LookChoice,
    /// Preferences › Display › Track Colors (sRGB); None = the look's palette.
    pub track_colors: Option<[[u8; 3]; 8]>,
    /// VP16 Preferences › General: "Make spacebar and F12 Play/Pause instead of Play/Stop".
    pub space_pauses: bool,
    /// VP23 Welcome screen on startup.
    pub show_welcome: bool,
    /// Recently used project list (most recent first) and its length.
    pub recent: Vec<String>,
    pub recent_max: usize,
    /// Preferences › General: "Check for updates automatically" (on startup).
    pub auto_update: bool,
    /// Version the user chose "Skip this version" for; the automatic check ignores it.
    pub skipped_update: Option<String>,
    /// Options › Quantize to Frames: moves, trims and pastes land on frame boundaries.
    pub quantize: bool,
    /// Preferences › Editing: "New still image length (seconds)" for stills added to the timeline.
    pub still_len: f64,
    /// Options › Automatic Crossfades: overlapping events on a track crossfade.
    pub auto_crossfade: bool,
    /// Options › Quickfade Audio Edits: a 10 ms fade at audio event edges without one, in playback and render.
    pub quickfade: bool,
    /// Options › Snap to Grid, Snap to All Events (event edges on every track, not only the dragged event's own)
    /// and Grid Spacing.
    pub snap_grid: bool,
    pub snap_all: bool,
    pub grid: crate::project::GridSpacing,
    /// Options › Auto Ripple and its mode.
    pub auto_ripple: bool,
    pub ripple_mode: crate::edit::RippleMode,
    /// Options › Ignore Event Grouping.
    pub ignore_grouping: bool,
    /// View › Show Envelopes: track envelopes are drawn and editable.
    pub show_envelopes: bool,
    /// View › Audio / Video Envelopes: the kinds turned off (every kind shows by default).
    pub hidden_envelopes: Vec<EnvView>,
    /// Options › Lock Envelopes to Events: envelope points move with the events over them.
    pub lock_envelopes: bool,
    /// Preferences › Video: create video proxies for 4K, HEVC and AV1 media as it is imported.
    pub auto_proxy: bool,
    /// Preferences › Video: the compositing-mode lists also offer Milan Pro's blend modes VEGAS has not (off).
    pub extra_blend_modes: bool,
    /// R6b: Preferences › General: the timeline shows the projects above a nested timeline as a breadcrumb (off: VEGAS
    /// shows only the nested timeline's name, in the title bar).
    pub nested_breadcrumb: bool,
    /// View › Video Bus Track.
    pub video_bus: bool,
    /// R8b: View › Audio Bus Tracks, and the Mixing Console's layout.
    pub bus_tracks: bool,
    /// R8c: View › Show OFX Effect Envelopes (Beta, VP23): video track FX keys are drawn as envelopes (off, as in VEGAS).
    pub ofx_envelopes: bool,
    pub console: crate::ui::ConsolePrefs,
    /// R8b: the audio track header's Show Output Meters, Use Vertical Meters and Channel Width (0-2) for its meter.
    pub output_meters: bool,
    pub vertical_meters: bool,
    pub meter_width: u8,
    /// VP23 plug-in chooser favourites (plug-in ids).
    pub fx_favorites: Vec<String>,
    /// Preferences › Editing: "Cut-to-overlap conversion" length (seconds) a transition dropped on a cut makes.
    pub cut_overlap: f64,
    /// Preferences › Editing: "Cursor preview duration (seconds)", what Preview at Cursor plays around the cursor.
    pub cursor_preview: f64,
    /// The FX chain new audio tracks start with (Set Default Track Properties); None = gate, EQ and compressor.
    pub default_track_fx: Option<Vec<crate::project::Fx>>,
    /// R18b: what new video tracks start with (video track Set Default Track Properties).
    pub default_video_track: crate::project::VideoDefaults,
    /// Preferences › Audio Device: the recording input (device name; None = the Windows default) and the recording
    /// latency offset, how many milliseconds earlier recorded events are placed.
    pub record_input: Option<String>,
    pub record_offset_ms: f64,
    /// R9: Preferences › Audio Device: the audio device type, the playback device (None = the Windows default) and the
    /// buffer size in frames (0 = the device's default).
    pub device_type: crate::audio::DeviceType,
    pub playback_device: Option<String>,
    pub buffer_frames: u32,
    /// Window layouts by `layout_key`: each look's last layout ("vp16:last") and user slots 0-9 ("vp16:3").
    /// Entries that do not parse are dropped, so a bad one never costs the rest of the preferences.
    #[serde(deserialize_with = "lenient_layouts")]
    pub layouts: BTreeMap<String, Layout>,
    /// R11: the user's render templates (Customize Template › Save), the favourite templates' names and the names of
    /// the templates last rendered with (most recent first).
    pub render_templates: Vec<crate::render::Template>,
    pub render_favorites: Vec<String>,
    pub render_recent: Vec<String>,
    /// R12: Render Options › Render in the background: Render closes the dialog and the render runs while editing
    /// goes on (progress in the status bar). Off, as VEGAS renders with a progress dialog.
    pub render_background: bool,
    /// R13: Preferences › Display: "Automatically hide docking area" (an area opens while the pointer is over its
    /// edge) and "Position tabs at top of docked windows"; both off, as in VEGAS.
    pub auto_hide_docks: bool,
    pub tabs_on_top: bool,
    /// R14: Preferences › Display › "Allow floating windows to dock (hold Ctrl to override)"; None = the look's
    /// default (off in VEGAS 16, on in VEGAS 23, as their installs show it).
    pub float_dock: Option<bool>,
    /// R14: Preferences › Preview Device (Windows Graphics Card): the display (`\\.\DISPLAY2`; None = the first one that
    /// is not the primary), "Scale output to fit display" (off), "Display frames in Video Preview window during
    /// playback" (on) and "Use project output rotation setting" (on).
    pub ext_display: Option<String>,
    pub ext_fit: bool,
    pub ext_in_window: bool,
    pub ext_rotate: bool,
    /// R15: Options › Customize Keyboard: per look, the commands whose keys the user changed (`keymap::Overrides`),
    /// and the keyboard map last chosen (None = [Default]; maps are .ini files in `keyboard_dir`).
    pub keys: crate::ui::keymap::Overrides,
    pub key_map: Option<String>,
    /// R15: Options › Customize Toolbar / Customize Timeline Toolbar: a bar's buttons by `toolbar::Bar::key`
    /// ("vp16:toolbar"); a bar not listed has the look's default buttons.
    pub toolbars: BTreeMap<String, Vec<String>>,
    /// R16b: the Video Scopes window's scopes, their settings and Update Scopes while Playing.
    pub scopes: crate::ui::ScopePrefs,
    /// R16c: Options › Loudness Meters, and Preferences › Audio › "Open editor when new loudness log is generated".
    pub loudness: crate::ui::LoudPrefs,
    /// R17b: Tools › Multicamera › Edit Multicamera Audio with Video (a take choice also switches the multicamera audio)
    /// and Show Full Frame in External Monitor; both off, as in VEGAS.
    pub multicam_audio: bool,
    pub multicam_full: bool,
    /// R18a: Preferences › Audio › "Preferred audio editor" (Tools › Open in Audio Editor); None = not chosen yet.
    pub audio_editor: Option<PathBuf>,
    /// R18a: Options › Metronome; View › Event Media Markers › Show Markers and Show Marker Labels; View › Event Length
    /// (VEGAS 23). All off, as in VEGAS.
    pub metronome: bool,
    pub media_markers: bool,
    pub marker_labels: bool,
    pub event_length: bool,
    /// R31: the OFX plug-in folders the user added and the plug-ins turned off in the Plug-In Manager.
    pub ofx: crate::engine::ofx::OfxPrefs,
}

/// R15: where Customize Keyboard keeps keyboard maps (%APPDATA%\MilanPro\Keyboard), as VEGAS keeps its .ini maps in
/// its AppData folder.
pub fn keyboard_dir() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("APPDATA")?).join("MilanPro").join("Keyboard"))
}

/// A row of View › Audio Envelopes and Video Envelopes: one kind of envelope (or the video Track Keyframes row) the
/// timeline can hide.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnvView {
    AudioMute,
    AudioVolume,
    AudioPan,
    VideoMute,
    CompositeLevel,
    FadeToColor,
    TrackKeyframes,
    Velocity,
    TransitionProgress,
    /// The Video Bus Track's Motion Blur Amount and Video Supersampling.
    MotionBlur,
    Supersampling,
}

/// R8c: in the timeline's View › Envelopes mask, not a row but View › Show OFX Effect Envelopes (set when on).
pub const OFX_SHOWN: u16 = 1 << 15;

impl EnvView {
    /// The row that shows envelope `kind` on a video (or audio) track or its events.
    pub fn of(kind: crate::project::EnvKind, video: bool) -> Option<Self> {
        use crate::project::EnvKind::*;
        Some(match (kind, video) {
            (Mute, false) => EnvView::AudioMute,
            (Volume, false) => EnvView::AudioVolume,
            (Pan, false) => EnvView::AudioPan,
            (Mute, true) => EnvView::VideoMute,
            (CompositeLevel, true) => EnvView::CompositeLevel,
            (FadeToColor, true) => EnvView::FadeToColor,
            (Velocity, true) => EnvView::Velocity,
            (TransitionProgress, true) => EnvView::TransitionProgress,
            (MotionBlur, true) => EnvView::MotionBlur,
            (Supersampling, true) => EnvView::Supersampling,
            _ => return None,
        })
    }

    /// Its bit in a set of hidden rows (`ui::EditOpts::hidden_envs`).
    pub fn bit(self) -> u16 {
        1 << self as u16
    }

    /// The set of `views` as bits.
    pub fn bits(views: &[EnvView]) -> u16 {
        views.iter().fold(0, |m, v| m | v.bit())
    }
}

/// Key of a saved layout: `slot` is "last" or a user slot number;VP23 Dark and White share one.
pub fn layout_key(look: UiLook, slot: impl std::fmt::Display) -> String {
    format!("{}:{slot}", if look == UiLook::Vegas16 { "vp16" } else { "vp23" })
}

/// The layouts to save: those on `disk` (another window may have saved some), with this window's `changed` keys
/// taken from `mine` (a changed key `mine` lacks was deleted: Organize Layouts).
pub fn merge_layouts(disk: BTreeMap<String, Layout>, mut mine: BTreeMap<String, Layout>, changed: &BTreeSet<String>) -> BTreeMap<String, Layout> {
    let mut out = disk;
    for k in changed {
        match mine.remove(k) {
            Some(l) => out.insert(k.clone(), l),
            None => out.remove(k),
        };
    }
    out
}

fn lenient_layouts<'de, D: serde::Deserializer<'de>>(d: D) -> Result<BTreeMap<String, Layout>, D::Error> {
    Ok(match serde_json::Value::deserialize(d)? {
        serde_json::Value::Object(m) => m.into_iter().filter_map(|(k, v)| Some((k, serde_json::from_value(v).ok()?))).collect(),
        _ => BTreeMap::new(),
    })
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            look: LookChoice::default(),
            track_colors: None,
            space_pauses: false,
            show_welcome: true,
            recent: Vec::new(),
            recent_max: 4,
            auto_update: true,
            skipped_update: None,
            quantize: true,
            still_len: 5.0,
            auto_crossfade: true,
            quickfade: true,
            snap_grid: false,
            snap_all: false,
            grid: crate::project::GridSpacing::RulerMarks,
            auto_ripple: false,
            ripple_mode: crate::edit::RippleMode::Affected,
            ignore_grouping: false,
            show_envelopes: true,
            hidden_envelopes: Vec::new(),
            lock_envelopes: true,
            auto_proxy: false,
            extra_blend_modes: false,
            nested_breadcrumb: false,
            video_bus: false,
            bus_tracks: false,
            ofx_envelopes: false,
            console: Default::default(),
            output_meters: true,
            vertical_meters: true,
            meter_width: 1,
            fx_favorites: Vec::new(),
            cut_overlap: 1.0,
            cursor_preview: 3.0,
            default_track_fx: None,
            default_video_track: Default::default(),
            record_input: None,
            record_offset_ms: 0.0,
            device_type: crate::audio::DeviceType::SoundMapper,
            playback_device: None,
            buffer_frames: 0,
            layouts: BTreeMap::new(),
            render_templates: Vec::new(),
            render_favorites: Vec::new(),
            render_recent: Vec::new(),
            render_background: false,
            auto_hide_docks: false,
            tabs_on_top: false,
            float_dock: None,
            ext_display: None,
            ext_fit: false,
            ext_in_window: true,
            ext_rotate: true,
            keys: BTreeMap::new(),
            key_map: None,
            toolbars: BTreeMap::new(),
            scopes: Default::default(),
            loudness: Default::default(),
            multicam_audio: false,
            multicam_full: false,
            audio_editor: None,
            metronome: false,
            media_markers: false,
            marker_labels: false,
            event_length: false,
            ofx: Default::default(),
        }
    }
}

impl Prefs {
    /// R14: a floating window moved over the docking areas docks (Ctrl inverts it).
    pub fn float_dock(&self, look: UiLook) -> bool {
        self.float_dock.unwrap_or(look == UiLook::Vegas23)
    }

    /// R9: the output device these preferences choose.
    pub fn device(&self) -> crate::audio::DeviceConfig {
        crate::audio::DeviceConfig { kind: self.device_type, device: self.playback_device.clone(), buffer: self.buffer_frames }
    }

    fn path() -> Option<PathBuf> {
        Some(PathBuf::from(std::env::var_os("APPDATA")?).join("MilanPro").join("prefs.json"))
    }

    pub fn load() -> Self {
        Self::try_load().unwrap_or_default()
    }

    /// The preferences on disk; None when the file is there but cannot be read or parsed, so a read-modify-write
    /// never puts defaults over it.
    pub fn try_load() -> Option<Self> {
        Self::path().map_or(Some(Self::default()), |p| Self::parse(std::fs::read_to_string(p)))
    }

    /// A field that does not parse (a variant from a newer build, a NaN written as null) falls back to its default
    /// alone; a file that is not a JSON object at all is None.
    fn parse(file: std::io::Result<String>) -> Option<Self> {
        use serde_json::Value;
        match file {
            Ok(s) => {
                let Value::Object(fields) = serde_json::from_str(&s).ok()? else { return None };
                let mut good = serde_json::Map::new();
                for (k, v) in fields {
                    let mut trial = good.clone();
                    trial.insert(k.clone(), v.clone());
                    if serde_json::from_value::<Self>(Value::Object(trial)).is_ok() {
                        good.insert(k, v);
                    }
                }
                serde_json::from_value(Value::Object(good)).ok()
            }
            Err(e) => (e.kind() == std::io::ErrorKind::NotFound).then(Self::default),
        }
    }

    /// An unreadable preferences file is kept as prefs.json.bad before a save writes over it.
    pub fn keep_unreadable() {
        if let Some(p) = Self::path().filter(|p| p.exists()) {
            let _ = std::fs::rename(&p, p.with_extension("json.bad"));
        }
    }

    /// The preferences to save from this window: those on `disk` (another window may have saved since), with the
    /// fields this window changed since it last read or wrote them (`seen`) taken from `self`. The recent list puts
    /// this window's new entries first over the disk's; layouts merge by the `changed` keys (`merge_layouts`).
    pub fn merged(self, disk: Prefs, seen: &Prefs, changed: &BTreeSet<String>) -> Prefs {
        use serde_json::{Value, to_value};
        let (Ok(Value::Object(mut out)), Ok(Value::Object(s)), Ok(Value::Object(m))) = (to_value(&disk), to_value(seen), to_value(&self)) else { return self };
        for (k, v) in m {
            if s.get(&k) != Some(&v) {
                out.insert(k, v);
            }
        }
        let Ok(mut p) = serde_json::from_value::<Prefs>(Value::Object(out)) else { return self };
        p.recent = disk.recent;
        let added: Vec<&String> = self.recent.iter().take_while(|r| seen.recent.first() != Some(*r)).collect();
        for r in added.into_iter().rev() {
            p.add_recent(std::path::Path::new(r));
        }
        p.layouts = merge_layouts(disk.layouts, self.layouts, changed);
        p
    }

    /// Written to a temporary file and renamed, so another window never reads a half-written file.
    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = Self::path() else { return Ok(()) };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
        let res = std::fs::write(&tmp, serde_json::to_string_pretty(self)?).and_then(|_| std::fs::rename(&tmp, &path));
        if res.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        res
    }

    /// Put `path` first in the recent list.
    pub fn add_recent(&mut self, path: &std::path::Path) {
        let p = path.display().to_string();
        self.recent.retain(|r| *r != p);
        self.recent.insert(0, p);
        self.recent.truncate(self.recent_max.clamp(1, 9));
    }

    pub fn palette(&self) -> Option<[egui::Color32; 8]> {
        self.track_colors.map(|cs| cs.map(|[r, g, b]| egui::Color32::from_rgb(r, g, b)))
    }
}

const FRAME_RATES: [(f64, &str); 8] = [
    (23.976, "23.976 (IVTC Film)"),
    (24.0, "24.000 (Film)"),
    (25.0, "25.000 (PAL)"),
    (29.97, "29.970 (NTSC)"),
    (30.0, "30.000"),
    (50.0, "50.000 (PAL Double)"),
    (59.94, "59.940 (NTSC Double)"),
    (60.0, "60.000"),
];
const SAMPLE_RATES: [u32; 4] = [44_100, 48_000, 88_200, 96_000];
/// Project templates: (name, width, height, fps).
const TEMPLATES: [(&str, u32, u32, f64); 9] = [
    ("HD 1080-24p (1920x1080, 23.976 fps)", 1920, 1080, 23.976),
    ("HD 1080-25p (1920x1080, 25.000 fps)", 1920, 1080, 25.0),
    ("HD 1080-30p (1920x1080, 29.970 fps)", 1920, 1080, 29.97),
    ("HD 1080-60p (1920x1080, 59.940 fps)", 1920, 1080, 59.94),
    ("HD 720-30p (1280x720, 29.970 fps)", 1280, 720, 29.97),
    ("HD 720-60p (1280x720, 59.940 fps)", 1280, 720, 59.94),
    ("UHD 2160-30p (3840x2160, 29.970 fps)", 3840, 2160, 29.97),
    ("UHD 2160-60p (3840x2160, 59.940 fps)", 3840, 2160, 59.94),
    ("Vertical 1080x1920, 29.970 fps", 1080, 1920, 29.97),
];

#[derive(Default)]
pub struct SettingsDialog {
    /// Some while the window is open: the edited copy, and the number of busses (R8a: kept on the timeline).
    draft: Option<(ProjectSettings, usize)>,
    tab: usize,
}

impl SettingsDialog {
    pub fn open(&mut self, project: &Project) {
        self.open_tab(project, 0);
    }

    /// Open on page `tab` (2: Ruler, for Set Project Tempo).
    pub fn open_tab(&mut self, project: &Project, tab: usize) {
        self.draft = Some((project.settings.clone(), project.timeline.audio.buses.len()));
        self.tab = tab;
    }

    pub fn is_open(&self) -> bool {
        self.draft.is_some()
    }

    pub fn show(&mut self, ctx: &egui::Context, project: &mut Project, history: &mut History) {
        let Some((d, buses)) = &mut self.draft else { return };
        let v23 = theme::get().look() == UiLook::Vegas23;
        let tab = &mut self.tab;
        let changed = *d != project.settings || *buses != project.timeline.audio.buses.len();
        let (hit, close) = dialogs::modal(ctx, "project_properties", "Project Properties", egui::vec2(427.0, 696.0), |ui| {
            let row: &[&str] = if v23 { &["Video", "Audio", "Ruler", "Summary", "Audio CD", "Advanced"] } else { &["Video", "Audio", "Ruler", "Summary", "Audio CD"] };
            dialogs::tabs(ui, &[row], tab);
            dialogs::page(ui, 590.0, |ui| match *tab {
                0 => video_tab(ui, d, project, v23),
                1 => audio_tab(ui, d, buses),
                2 => ruler_tab(ui, d),
                3 => summary_tab(ui, d),
                4 => dialogs::empty_page(ui, NotPlanned("R59")),
                _ => dialogs::empty_page(ui, Later("R49")),
            });
            ui.add_space(8.0);
            dialogs::button_row(ui, &[("OK", true), ("Cancel", true), ("Apply", changed)])
        });
        if matches!(hit, Some(0 | 2)) {
            // H.264/HEVC 4:2:0 output needs even dimensions.
            d.width &= !1;
            d.height &= !1;
            let before = project.clone();
            let bus_change = crate::edit::set_bus_count(project, *buses);
            if *d != project.settings || bus_change {
                history.push(before);
                project.settings = d.clone();
                project.mark_modified();
            }
        }
        if matches!(hit, Some(0 | 1)) || close {
            self.draft = None;
        }
    }
}

fn video_tab(ui: &mut Ui, d: &mut ProjectSettings, project: &Project, v23: bool) {
    let current = TEMPLATES.iter().find(|t| (t.1, t.2) == (d.width, d.height) && (t.3 - d.frame_rate).abs() < 0.001);
    ui.horizontal(|ui| {
        ui.label("Template:");
        egui::ComboBox::from_id_salt("pp_template")
            .width(230.0)
            .selected_text(current.map_or("(Custom)", |t| t.0))
            .show_ui(ui, |ui| {
                for t in TEMPLATES {
                    if ui.selectable_label(current.is_some_and(|c| c.0 == t.0), t.0).clicked() {
                        (d.width, d.height, d.frame_rate) = (t.1, t.2, t.3);
                    }
                }
            });
        // Match Media Settings: the first video in the project.
        let first = project.media_pool.iter().find(|m| m.width.is_some() && m.frame_rate.is_some());
        if ui.add_enabled(first.is_some(), egui::Button::new("Match Media")).on_hover_text("Match Media Settings").clicked() {
            if let Some(m) = first {
                (d.width, d.height) = (m.width.unwrap_or(d.width), m.height.unwrap_or(d.height));
                d.frame_rate = m.frame_rate.unwrap_or(d.frame_rate);
            }
        }
    });
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            dialogs::row(ui, "Width:", 60.0, true, |ui| ui.add(egui::DragValue::new(&mut d.width).range(16..=8192)));
            dialogs::row(ui, "Height:", 60.0, true, |ui| ui.add(egui::DragValue::new(&mut d.height).range(16..=8192)));
        });
        ui.vertical(|ui| {
            use crate::project::{FieldOrder, PARS, ROTATIONS};
            dialogs::row(ui, "Field order:", 96.0, true, |ui| {
                egui::ComboBox::from_id_salt("pp_field").width(150.0).selected_text(d.field_order.name()).show_ui(ui, |ui| {
                    for f in FieldOrder::ALL {
                        ui.selectable_value(&mut d.field_order, f, f.name());
                    }
                });
            });
            dialogs::row(ui, "Pixel aspect ratio:", 96.0, true, |ui| {
                let sel = PARS.iter().find(|p| (p.0 - d.par).abs() < 1e-4).map_or(format!("{:.4}", d.par), |p| p.1.to_string());
                egui::ComboBox::from_id_salt("pp_par").width(150.0).selected_text(sel).show_ui(ui, |ui| {
                    for (p, label) in PARS {
                        ui.selectable_value(&mut d.par, p, label);
                    }
                });
            });
            dialogs::row(ui, "Output rotation:", 96.0, true, |ui| {
                let sel = ROTATIONS.iter().find(|r| r.0 == d.rotation).map_or("0° (original)", |r| r.1);
                egui::ComboBox::from_id_salt("pp_rot").width(150.0).selected_text(sel).show_ui(ui, |ui| {
                    for (r, label) in ROTATIONS {
                        ui.selectable_value(&mut d.rotation, r, label);
                    }
                });
            });
        });
    });
    dialogs::row(ui, "Frame rate:", 70.0, true, |ui| {
        let sel = FRAME_RATES.iter().find(|f| (f.0 - d.frame_rate).abs() < 0.001).map_or(format!("{:.3}", d.frame_rate), |f| f.1.to_string());
        egui::ComboBox::from_id_salt("pp_fps").width(140.0).selected_text(sel).show_ui(ui, |ui| {
            for (f, label) in FRAME_RATES {
                ui.selectable_value(&mut d.frame_rate, f, label);
            }
        });
    });
    dialogs::row(ui, "Stereoscopic 3D mode:", 120.0, false, |ui| dialogs::fixed_combo(ui, "pp_s3d", "Off", 240.0, NotPlanned("R59")));
    ui.separator();
    let mut grid: Vec<(&str, &str, Why)> = vec![
        ("Pixel format:", "8-bit", Later("R40b")),
        ("Compositing gamma:", "2.222 (Video)", Later("R40b")),
        ("ACES version:", "1.0", Later("R40b")),
        ("ACES color space:", "Default (ACES2065-1)", Later("R40b")),
        ("View transform:", "Off", Later("R40b")),
    ];
    if v23 {
        grid.insert(1, ("HDR Mode:", "Off", Later("R40b")));
        grid.insert(6, ("Look modification transform:", "Off", Later("R40b")));
    }
    for (k, (label, value, why)) in grid.into_iter().enumerate() {
        dialogs::row(ui, label, 175.0, false, |ui| dialogs::fixed_combo(ui, &format!("pp_g{k}"), value, 200.0, why));
    }
    // VEGAS's order: Full-resolution rendering quality, Motion blur type, Deinterlace method, then Resample mode.
    dialogs::row(ui, "Full-resolution rendering quality:", 175.0, true, |ui| {
        egui::ComboBox::from_id_salt("pp_quality").width(200.0).selected_text(format!("{:?}", d.render_quality)).show_ui(ui, |ui| {
            for q in crate::decode::Quality::ALL {
                ui.selectable_value(&mut d.render_quality, q, format!("{q:?}"));
            }
        });
    });
    dialogs::row(ui, "Motion blur type:", 175.0, true, |ui| {
        use crate::project::BlurType;
        egui::ComboBox::from_id_salt("pp_blur").width(200.0).selected_text(d.motion_blur.name()).show_ui(ui, |ui| {
            for b in BlurType::ALL {
                ui.selectable_value(&mut d.motion_blur, b, b.name());
            }
        });
    });
    dialogs::row(ui, "Deinterlace method:", 175.0, true, |ui| {
        use crate::project::Deinterlace;
        egui::ComboBox::from_id_salt("pp_deint").width(200.0).selected_text(d.deinterlace.name()).show_ui(ui, |ui| {
            for m in Deinterlace::ALL {
                ui.selectable_value(&mut d.deinterlace, m, m.name());
            }
        });
    });
    dialogs::row(ui, "Resample mode:", 175.0, true, |ui| {
        use crate::project::Resample::*;
        // VEGAS 23 calls Smart "Frame blend" and offers Optical Flow in place of Force.
        let modes = if v23 { [Smart, OpticalFlow, Disable] } else { [Smart, Force, Disable] };
        egui::ComboBox::from_id_salt("pp_resample").width(200.0).selected_text(resample_name(d.resample, v23)).show_ui(ui, |ui| {
            for m in modes {
                ui.selectable_value(&mut d.resample, m, resample_name(m, v23));
            }
        });
    });
    ui.checkbox(&mut d.adjust_media, "Adjust source media to better match project or render settings");
    ui.add_enabled_ui(false, |ui| {
        ui.label("Prerendered files folder:").on_disabled_hover_text(Later("R23").text());
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut std::env::temp_dir().display().to_string()).desired_width(310.0))
                .on_disabled_hover_text(Later("R23").text());
            ui.button("Browse...").on_disabled_hover_text(Later("R23").text());
        });
        ui.add(egui::Checkbox::new(&mut false, "Start all new projects with these settings")).on_disabled_hover_text(Later("R49").text());
    });
}

/// Project Properties › Resample mode, as each look names it.
fn resample_name(r: crate::project::Resample, v23: bool) -> &'static str {
    use crate::project::Resample::*;
    match (r, v23) {
        (Smart, false) => "Smart resample",
        (Smart, true) => "Frame blend",
        (Force, _) => "Force resample",
        (Disable, false) => "Disable resample",
        (Disable, true) => "Disable resampling",
        (OpticalFlow, _) => "Optical Flow",
    }
}

/// The Ruler page: start time (typed in the ruler's format), format, tempo, meter and beat note.
fn ruler_tab(ui: &mut Ui, d: &mut ProjectSettings) {
    use crate::project::{RulerFormat, format_time, parse_time};
    let s = d.clone();
    dialogs::row(ui, "Ruler time starts at:", 150.0, true, |ui| {
        dialogs::time_edit(ui, "pr_start", &mut d.ruler.offset, |t| format_time(t, s.ruler.format, &s), |x| parse_time(x, s.ruler.format, &s));
    });
    dialogs::row(ui, "Ruler time format:", 150.0, true, |ui| {
        egui::ComboBox::from_id_salt("pr_format").width(230.0).selected_text(d.ruler.format.name()).show_ui(ui, |ui| {
            for f in RulerFormat::ALL {
                ui.selectable_value(&mut d.ruler.format, f, f.name());
            }
        });
    });
    ui.add_space(8.0);
    dialogs::row(ui, "Beats per minute:", 150.0, true, |ui| ui.add(egui::DragValue::new(&mut d.ruler.tempo).range(20.0..=500.0).speed(0.1).max_decimals(3)));
    dialogs::row(ui, "Beats per measure:", 150.0, true, |ui| ui.add(egui::DragValue::new(&mut d.ruler.beats_per_measure).range(1..=32)));
    dialogs::row(ui, "Note length (one beat):", 150.0, true, |ui| {
        egui::ComboBox::from_id_salt("pr_note").width(80.0).selected_text(d.ruler.beat_note.to_string()).show_ui(ui, |ui| {
            for n in [1, 2, 4, 8, 16, 32] {
                ui.selectable_value(&mut d.ruler.beat_note, n, n.to_string());
            }
        });
    });
}

/// R16a: the Summary page (VEGAS's five fields; Comments takes several lines).
fn summary_tab(ui: &mut Ui, d: &mut ProjectSettings) {
    let s = &mut d.summary;
    for (label, v) in [("Title:", &mut s.title), ("Artist:", &mut s.artist), ("Engineer:", &mut s.engineer), ("Copyright:", &mut s.copyright)] {
        dialogs::row(ui, label, 80.0, true, |ui| ui.add(egui::TextEdit::singleline(v).desired_width(290.0)));
    }
    dialogs::row(ui, "Comments:", 80.0, true, |ui| ui.add(egui::TextEdit::multiline(&mut s.comments).desired_width(290.0).desired_rows(8)));
}

/// `buses`: Number of stereo busses (R8a). Fewer removes the last ones: what went to them goes to the master.
fn audio_tab(ui: &mut Ui, d: &mut ProjectSettings, buses: &mut usize) {
    dialogs::row(ui, "Master bus mode:", 150.0, false, |ui| dialogs::fixed_combo(ui, "pa_bus", "Stereo", 200.0, Later("R10a")));
    dialogs::row(ui, "Number of stereo busses:", 150.0, true, |ui| {
        ui.add(egui::DragValue::new(buses).range(0..=crate::project::AudioSettings::MAX_BUSES))
    });
    dialogs::row(ui, "Sample rate (Hz):", 150.0, true, |ui| {
        egui::ComboBox::from_id_salt("pa_sr").width(200.0).selected_text(d.sample_rate.to_string()).show_ui(ui, |ui| {
            for r in SAMPLE_RATES {
                ui.selectable_value(&mut d.sample_rate, r, r.to_string());
            }
        });
    });
    // shortcut: Bit depth sets the PCM/FLAC/ALAC renders that follow the project; recordings stay 32-bit float (never
    // less than any setting); write recordings at the project depth if a user needs smaller takes.
    dialogs::row(ui, "Bit depth:", 150.0, true, |ui| {
        let name = |b: u8| if b == 32 { "32 (floating point)".to_string() } else { b.to_string() };
        egui::ComboBox::from_id_salt("pa_bits").width(200.0).selected_text(name(d.bit_depth)).show_ui(ui, |ui| {
            for b in [8, 16, 24, 32] {
                ui.selectable_value(&mut d.bit_depth, b, name(b));
            }
        });
    });
    dialogs::row(ui, "Resample and stretch quality:", 150.0, false, |ui| dialogs::fixed_combo(ui, "pa_q", "Good", 200.0, Later("R10a")));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefs_roundtrip_and_tolerate_missing_fields() {
        let p = Prefs { look: LookChoice::VEGAS23_WHITE, track_colors: Some([[1, 2, 3]; 8]), ..Default::default() };
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<Prefs>(&json).unwrap(), p);
        let d = serde_json::from_str::<Prefs>("{}").unwrap();
        assert_eq!((d.look, d.show_welcome, d.recent_max, d.auto_update, d.quantize, d.still_len), (LookChoice::VEGAS16, true, 4, true, true, 5.0));
        assert_eq!((d.auto_crossfade, d.auto_ripple, d.ripple_mode, d.ignore_grouping, d.quickfade), (true, false, crate::edit::RippleMode::Affected, false, true));
        assert!(d.show_envelopes && d.lock_envelopes && !d.auto_proxy);
        assert!(d.fx_favorites.is_empty() && d.cut_overlap == 1.0 && d.default_track_fx.is_none() && d.cursor_preview == 3.0);
        assert!(d.record_input.is_none() && d.record_offset_ms == 0.0);
        // R11: old preferences have no user templates, favourites or recent renders; a saved template reads back.
        assert!(d.render_templates.is_empty() && d.render_favorites.is_empty() && d.render_recent.is_empty());
        // R12: old preferences render with the progress dialog (VEGAS), not in the background.
        assert!(!d.render_background);
        // R13: old preferences keep the docking areas shown and the tabs at the bottom (VEGAS's defaults).
        assert!(!d.auto_hide_docks && !d.tabs_on_top);
        let k = Prefs { auto_hide_docks: true, tabs_on_top: true, ..Default::default() };
        assert_eq!(serde_json::from_str::<Prefs>(&serde_json::to_string(&k).unwrap()).unwrap(), k);
        // R14: old preferences dock floating windows as each look's install does, and preview externally on the first
        // secondary display at 1:1, with frames in the Video Preview window too and the output rotation applied.
        assert!(!d.float_dock(UiLook::Vegas16) && d.float_dock(UiLook::Vegas23));
        assert!(d.ext_display.is_none() && !d.ext_fit && d.ext_in_window && d.ext_rotate);
        let e = Prefs { float_dock: Some(false), ext_display: Some(r"\\.\DISPLAY2".into()), ext_fit: true, ext_in_window: false, ext_rotate: false, ..Default::default() };
        assert_eq!(serde_json::from_str::<Prefs>(&serde_json::to_string(&e).unwrap()).unwrap(), e);
        assert!(!e.float_dock(UiLook::Vegas23));
        // R17b: old preferences leave the multicamera audio alone and show the grid on the external monitor too (VEGAS).
        assert!(!d.multicam_audio && !d.multicam_full);
        let m = Prefs { multicam_audio: true, multicam_full: true, ..Default::default() };
        assert_eq!(serde_json::from_str::<Prefs>(&serde_json::to_string(&m).unwrap()).unwrap(), m);
        // R18a: no audio editor chosen; metronome, media markers, their labels and event lengths off (VEGAS).
        assert!(d.audio_editor.is_none() && !d.metronome && !d.media_markers && !d.marker_labels && !d.event_length);
        let r = Prefs { audio_editor: Some(r"C:\Editors\Sound.exe".into()), metronome: true, media_markers: true, marker_labels: true, event_length: true, ..Default::default() };
        assert_eq!(serde_json::from_str::<Prefs>(&serde_json::to_string(&r).unwrap()).unwrap(), r);
        // R18b: no video track defaults; saved ones read back.
        assert_eq!(d.default_video_track, crate::project::VideoDefaults::default());
        let v = Prefs { default_video_track: crate::project::VideoDefaults { height: Some(80.0), composite: Some(crate::project::Composite::Add), fx: Some(Vec::new()) }, ..Default::default() };
        assert_eq!(serde_json::from_str::<Prefs>(&serde_json::to_string(&v).unwrap()).unwrap(), v);
        // R15: old preferences have the default keys and toolbars; custom ones read back.
        assert!(d.keys.is_empty() && d.key_map.is_none() && d.toolbars.is_empty());
        let mut c = Prefs { key_map: Some("Mine".into()), ..Default::default() };
        c.keys.entry("vp23".into()).or_default().insert("Split".into(), vec!["Ctrl+Shift+F5".into()]);
        c.toolbars.insert("vp16:toolbar".into(), vec!["Save".into(), "|".into(), "Split".into()]);
        assert_eq!(serde_json::from_str::<Prefs>(&serde_json::to_string(&c).unwrap()).unwrap(), c);
        assert_eq!(Prefs::parse(Ok(r#"{"keys":{"vp23":5},"toolbars":3,"quantize":false}"#.into())).map(|p| (p.keys.is_empty(), p.toolbars.is_empty(), p.quantize)), Some((true, true, false)));
        let mine = crate::render::Template { name: "Mine".into(), ..crate::render::BUILTIN[5].clone() };
        let t = Prefs { render_templates: vec![mine], render_favorites: vec!["Mine".into()], render_recent: vec!["Web 720p".into()], ..Default::default() };
        assert_eq!(serde_json::from_str::<Prefs>(&serde_json::to_string(&t).unwrap()).unwrap(), t);
        let partial = serde_json::from_str::<crate::render::Template>(r#"{"name":"Old","container":"mov","video":{"codec":"prores_ks"}}"#).unwrap();
        assert_eq!((partial.video.unwrap().rate, partial.audio.unwrap().codec.as_str()), (crate::render::Rate::Quality(18), "aac"), "missing fields take the defaults");
        // R9: old preferences play on the Windows default device at its default buffer, as before.
        assert_eq!(d.device(), crate::audio::DeviceConfig::default());
        assert_eq!(d.device_type, crate::audio::DeviceType::SoundMapper);
        let o = Prefs { device_type: crate::audio::DeviceType::Exclusive, playback_device: Some("Speakers (USB)".into()), buffer_frames: 128, ..Default::default() };
        assert_eq!(serde_json::from_str::<Prefs>(&serde_json::to_string(&o).unwrap()).unwrap(), o);
        assert_eq!(Prefs::parse(Ok(r#"{"device_type":"Asio","quantize":false}"#.into())).map(|p| (p.device_type, p.quantize)), Some((crate::audio::DeviceType::SoundMapper, false)));
        assert!(!d.snap_grid && !d.snap_all && d.grid == crate::project::GridSpacing::RulerMarks);
        let g = Prefs { snap_grid: true, snap_all: true, grid: crate::project::GridSpacing::Note(16, true), ..Default::default() };
        assert_eq!(serde_json::from_str::<Prefs>(&serde_json::to_string(&g).unwrap()).unwrap(), g);
        let chain = crate::project::default_track_chain(None);
        let a = Prefs {
            auto_proxy: true,
            fx_favorites: vec!["milan.fx.invert".into()],
            cut_overlap: 0.5,
            default_track_fx: Some(chain),
            record_input: Some("Microphone (USB)".into()),
            record_offset_ms: 12.5,
            ..Default::default()
        };
        assert_eq!(serde_json::from_str::<Prefs>(&serde_json::to_string(&a).unwrap()).unwrap(), a);
        let r = Prefs { auto_ripple: true, ripple_mode: crate::edit::RippleMode::All, ..Default::default() };
        assert_eq!(serde_json::from_str::<Prefs>(&serde_json::to_string(&r).unwrap()).unwrap(), r);
        let mut r = Prefs { recent_max: 2, ..Default::default() };
        for p in ["a", "b", "a", "c"] {
            r.add_recent(std::path::Path::new(p));
        }
        assert_eq!(r.recent, ["c", "a"]);
    }

    /// A broken `layouts` (not a map, or a bad entry) loses only what is broken, never the other preferences.
    #[test]
    fn corrupt_layouts_fall_back() {
        let t = crate::ui::theme::Theme::new(LookChoice::VEGAS16);
        let good = crate::ui::dock::Dock::new(&t).layout("Mine");
        let mut p = Prefs { quantize: false, ..Default::default() };
        p.layouts.insert(layout_key(UiLook::Vegas16, 3), good.clone());
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<Prefs>(&json).unwrap(), p);
        assert_eq!(layout_key(UiLook::Vegas23, "last"), "vp23:last");

        let whole = json.replace(r#""layouts":{"#, r#""layouts":5,"x":{"#);
        let d = serde_json::from_str::<Prefs>(&whole).unwrap();
        assert!(d.layouts.is_empty() && !d.quantize);
        let entry = json.replace(r#""layouts":{"#, r#""layouts":{"vp16:last":{"groups":"x"},"vp16:4":{"name":"Future","groups":[{"tabs":["Hub"],"active":0,"width":1}],"height":null,"track_list_w":1},"#);
        let d = serde_json::from_str::<Prefs>(&entry).unwrap();
        assert_eq!(d.layouts.keys().collect::<Vec<_>>(), ["vp16:3"]);
        assert_eq!(d.layouts["vp16:3"], good);
    }

    /// A file that is there but unreadable is None (never defaults to write back); a missing one is the defaults.
    #[test]
    fn try_load_tells_missing_from_broken() {
        use std::io::{Error, ErrorKind};
        assert_eq!(Prefs::parse(Err(Error::from(ErrorKind::NotFound))), Some(Prefs::default()));
        assert_eq!(Prefs::parse(Err(Error::from(ErrorKind::PermissionDenied))), None);
        assert_eq!(Prefs::parse(Ok(r#"{"recent":["a""#.into())), None);
        assert_eq!(Prefs::parse(Ok(r#"{"quantize":false}"#.into())).map(|p| p.quantize), Some(false));
    }

    /// Window B toggling Quantize keeps window A's newer recent list and still length; B's own new recent entry goes
    /// first.
    #[test]
    fn merged_writes_only_this_windows_changes() {
        let seen = Prefs { recent: vec!["old".into()], ..Default::default() };
        let disk = Prefs { recent: vec!["Wedding".into(), "old".into()], still_len: 10.0, ..Default::default() };
        let mut mine = seen.clone();
        mine.quantize = false;
        assert_eq!(mine.clone().merged(disk.clone(), &seen, &BTreeSet::new()), Prefs { quantize: false, ..disk.clone() });
        mine.add_recent(std::path::Path::new("Mine"));
        assert_eq!(mine.merged(disk, &seen, &BTreeSet::new()).recent, ["Mine", "Wedding", "old"]);
    }

    /// One field from a newer build does not cost the others.
    #[test]
    fn a_bad_field_falls_back_alone() {
        let p = Prefs::parse(Ok(r#"{"quantize":false,"ripple_mode":"FromTheFuture","still_len":null,"recent":["a"]}"#.into())).unwrap();
        assert_eq!((p.quantize, p.ripple_mode, p.still_len, p.recent), (false, crate::edit::RippleMode::Affected, 5.0, vec!["a".to_string()]));
        // R8b: a file from before the Mixing Console gets its defaults (VU meters off); the new ripple mode reads back.
        assert_eq!((p.bus_tracks, p.console.regions, p.console.width), (false, [true, true, true, false, true, true], 1));
        let q = Prefs::parse(Ok(r#"{"ripple_mode":"AffectedMarkersOnly","console":{"regions":[true,true,true,true,true,true]}}"#.into())).unwrap();
        assert_eq!((q.ripple_mode, q.console.regions[3], q.console.channel_list), (crate::edit::RippleMode::AffectedMarkersOnly, true, true));
        // R16b: a file from before the Video Scopes gets VEGAS's defaults; a saved choice reads back.
        assert_eq!(p.scopes, crate::ui::ScopePrefs::default());
        let s = Prefs::parse(Ok(r#"{"scopes":{"view":"All","levels":{"studio":true},"while_playing":true}}"#.into())).unwrap().scopes;
        assert!(s.levels.studio && !s.levels.setup && s.while_playing && s != crate::ui::ScopePrefs::default());
        // R16c: likewise the Loudness Meters options.
        assert_eq!(p.loudness, crate::ui::LoudPrefs::default());
        let l = Prefs::parse(Ok(r#"{"loudness":{"mode":"Atsc","range":7,"open_editor":false}}"#.into())).unwrap().loudness;
        assert!(l.mode == crate::engine::loudness::Mode::Atsc && l.range == 7 && !l.open_editor && l.labels);
        // R31: a file from before OFX hosting searches no extra folders and has every plug-in on; saved choices read
        // back, and a damaged entry costs only itself.
        assert_eq!(p.ofx, crate::engine::ofx::OfxPrefs::default());
        let o = Prefs::parse(Ok(r#"{"ofx":{"folders":["D:\\OFX"],"disabled":["ofx:a@1"]},"quantize":false}"#.into())).unwrap();
        assert!(!o.quantize && o.ofx.folders == [std::path::PathBuf::from(r"D:\OFX")] && o.ofx.disabled.contains("ofx:a@1"));
        let bad = Prefs::parse(Ok(r#"{"ofx":{"folders":7},"quantize":false}"#.into())).unwrap();
        assert!(!bad.quantize && bad.ofx == crate::engine::ofx::OfxPrefs::default());
    }

    /// Another window's saved slot survives this window's save; this window's changed slots win.
    #[test]
    fn merge_keeps_other_windows_layouts() {
        let t = crate::ui::theme::Theme::new(LookChoice::VEGAS16);
        let l = |n: &str| crate::ui::dock::Dock::new(&t).layout(n);
        let disk = BTreeMap::from([("vp16:3".to_string(), l("Theirs")), ("vp16:4".to_string(), l("Disk4"))]);
        let mine = BTreeMap::from([("vp16:4".to_string(), l("Mine4")), ("vp16:5".to_string(), l("Stale5"))]);
        let m = merge_layouts(disk.clone(), mine.clone(), &BTreeSet::from(["vp16:4".to_string()]));
        assert_eq!(m.iter().map(|(k, v)| (k.as_str(), v.name.as_str())).collect::<Vec<_>>(), [("vp16:3", "Theirs"), ("vp16:4", "Mine4")]);
        // R13: a slot this window deleted (Organize Layouts) goes from the file too.
        let m = merge_layouts(disk, mine, &BTreeSet::from(["vp16:3".to_string()]));
        assert_eq!(m.keys().collect::<Vec<_>>(), ["vp16:4"]);
    }
}
