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
    /// VP23 plug-in chooser favourites (plug-in ids).
    pub fx_favorites: Vec<String>,
    /// Preferences › Editing: "Cut-to-overlap conversion" length (seconds) a transition dropped on a cut makes.
    pub cut_overlap: f64,
    /// Preferences › Editing: "Cursor preview duration (seconds)", what Preview at Cursor plays around the cursor.
    pub cursor_preview: f64,
    /// The FX chain new audio tracks start with (Set Default Track Properties); None = gate, EQ and compressor.
    pub default_track_fx: Option<Vec<crate::project::Fx>>,
    /// Preferences › Audio Device: the recording input (device name; None = the Windows default) and the recording
    /// latency offset, how many milliseconds earlier recorded events are placed.
    pub record_input: Option<String>,
    pub record_offset_ms: f64,
    /// Window layouts by `layout_key`: each look's last layout ("vp16:last") and user slots 0-9 ("vp16:3").
    /// Entries that do not parse are dropped, so a bad one never costs the rest of the preferences.
    #[serde(deserialize_with = "lenient_layouts")]
    pub layouts: BTreeMap<String, Layout>,
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
}

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
/// taken from `mine`.
pub fn merge_layouts(disk: BTreeMap<String, Layout>, mine: BTreeMap<String, Layout>, changed: &BTreeSet<String>) -> BTreeMap<String, Layout> {
    let mut out = disk;
    out.extend(mine.into_iter().filter(|(k, _)| changed.contains(k)));
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
            fx_favorites: Vec::new(),
            cut_overlap: 1.0,
            cursor_preview: 3.0,
            default_track_fx: None,
            record_input: None,
            record_offset_ms: 0.0,
            layouts: BTreeMap::new(),
        }
    }
}

impl Prefs {
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
    /// Some while the window is open: the edited copy.
    draft: Option<ProjectSettings>,
    tab: usize,
}

impl SettingsDialog {
    pub fn open(&mut self, project: &Project) {
        self.open_tab(project, 0);
    }

    /// Open on page `tab` (2: Ruler, for Set Project Tempo).
    pub fn open_tab(&mut self, project: &Project, tab: usize) {
        self.draft = Some(project.settings.clone());
        self.tab = tab;
    }

    pub fn is_open(&self) -> bool {
        self.draft.is_some()
    }

    pub fn show(&mut self, ctx: &egui::Context, project: &mut Project, history: &mut History) {
        let Some(d) = &mut self.draft else { return };
        let v23 = theme::get().look() == UiLook::Vegas23;
        let tab = &mut self.tab;
        let changed = *d != project.settings;
        let (hit, close) = dialogs::modal(ctx, "project_properties", "Project Properties", egui::vec2(427.0, 696.0), |ui| {
            let row: &[&str] = if v23 { &["Video", "Audio", "Ruler", "Summary", "Audio CD", "Advanced"] } else { &["Video", "Audio", "Ruler", "Summary", "Audio CD"] };
            dialogs::tabs(ui, &[row], tab);
            dialogs::page(ui, 590.0, |ui| match *tab {
                0 => video_tab(ui, d, project, v23),
                1 => audio_tab(ui, d),
                2 => ruler_tab(ui, d),
                3 => dialogs::empty_page(ui, Later("R16a")),
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
            if *d != project.settings {
                history.push(project.clone());
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
            dialogs::row(ui, "Field order:", 96.0, false, |ui| dialogs::fixed_combo(ui, "pp_field", "None (progressive scan)", 150.0, Later("R11")));
            dialogs::row(ui, "Pixel aspect ratio:", 96.0, false, |ui| dialogs::fixed_combo(ui, "pp_par", "1.0000 (Square)", 150.0, Later("R11")));
            dialogs::row(ui, "Output rotation:", 96.0, false, |ui| dialogs::fixed_combo(ui, "pp_rot", "0° (original)", 150.0, Later("R11")));
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
        ("Full-resolution rendering quality:", "Good", Later("R11")),
        ("Motion blur type:", "Gaussian", Later("R7")),
        ("Deinterlace method:", "Blend fields", Later("R11")),
    ];
    if v23 {
        grid.insert(1, ("HDR Mode:", "Off", Later("R40b")));
        grid.insert(6, ("Look modification transform:", "Off", Later("R40b")));
    }
    for (k, (label, value, why)) in grid.into_iter().enumerate() {
        dialogs::row(ui, label, 175.0, false, |ui| dialogs::fixed_combo(ui, &format!("pp_g{k}"), value, 200.0, why));
    }
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
    ui.add_enabled(false, egui::Checkbox::new(&mut true, "Adjust source media to better match project or render settings"))
        .on_disabled_hover_text(Later("R11").text());
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

fn audio_tab(ui: &mut Ui, d: &mut ProjectSettings) {
    dialogs::row(ui, "Master bus mode:", 150.0, false, |ui| dialogs::fixed_combo(ui, "pa_bus", "Stereo", 200.0, Later("R8a")));
    dialogs::row(ui, "Number of stereo busses:", 150.0, false, |ui| dialogs::fixed_combo(ui, "pa_n", "0", 200.0, Later("R8a")));
    dialogs::row(ui, "Sample rate (Hz):", 150.0, true, |ui| {
        egui::ComboBox::from_id_salt("pa_sr").width(200.0).selected_text(d.sample_rate.to_string()).show_ui(ui, |ui| {
            for r in SAMPLE_RATES {
                ui.selectable_value(&mut d.sample_rate, r, r.to_string());
            }
        });
    });
    dialogs::row(ui, "Bit depth:", 150.0, false, |ui| dialogs::fixed_combo(ui, "pa_bits", "16", 200.0, Later("R11")));
    dialogs::row(ui, "Resample and stretch quality:", 150.0, false, |ui| dialogs::fixed_combo(ui, "pa_q", "Good", 200.0, Later("R4b")));
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
    }

    /// Another window's saved slot survives this window's save; this window's changed slots win.
    #[test]
    fn merge_keeps_other_windows_layouts() {
        let t = crate::ui::theme::Theme::new(LookChoice::VEGAS16);
        let l = |n: &str| crate::ui::dock::Dock::new(&t).layout(n);
        let disk = BTreeMap::from([("vp16:3".to_string(), l("Theirs")), ("vp16:4".to_string(), l("Disk4"))]);
        let mine = BTreeMap::from([("vp16:4".to_string(), l("Mine4")), ("vp16:5".to_string(), l("Stale5"))]);
        let m = merge_layouts(disk, mine, &BTreeSet::from(["vp16:4".to_string()]));
        assert_eq!(m.iter().map(|(k, v)| (k.as_str(), v.name.as_str())).collect::<Vec<_>>(), [("vp16:3", "Theirs"), ("vp16:4", "Mine4")]);
    }
}
