//! File › Properties (§5.15.1 Project Properties), applied through undo history, and the app preferences file
//! (%APPDATA%\MilanPro\prefs.json).
use eframe::egui::{self, Ui};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use crate::history::History;
use crate::project::{Project, ProjectSettings};
use crate::ui::dialogs;
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
        }
    }
}

impl Prefs {
    fn path() -> Option<PathBuf> {
        Some(PathBuf::from(std::env::var_os("APPDATA")?).join("MilanPro").join("prefs.json"))
    }

    pub fn load() -> Self {
        Self::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = Self::path() else { return Ok(()) };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)
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
        self.draft = Some(project.settings.clone());
        self.tab = 0;
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
                _ => {
                    ui.add_space(8.0);
                    ui.label("Milan Pro has no settings on this page yet.");
                }
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
            dialogs::row(ui, "Field order:", 96.0, false, |ui| dialogs::fixed_combo(ui, "pp_field", "None (progressive scan)", 150.0));
            dialogs::row(ui, "Pixel aspect ratio:", 96.0, false, |ui| dialogs::fixed_combo(ui, "pp_par", "1.0000 (Square)", 150.0));
            dialogs::row(ui, "Output rotation:", 96.0, false, |ui| dialogs::fixed_combo(ui, "pp_rot", "0° (original)", 150.0));
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
    dialogs::row(ui, "Stereoscopic 3D mode:", 120.0, false, |ui| dialogs::fixed_combo(ui, "pp_s3d", "Off", 240.0));
    ui.separator();
    let mut grid: Vec<(&str, &str)> = vec![
        ("Pixel format:", "8-bit"),
        ("Compositing gamma:", "2.222 (Video)"),
        ("ACES version:", "1.0"),
        ("ACES color space:", "Default (ACES2065-1)"),
        ("View transform:", "Off"),
        ("Full-resolution rendering quality:", "Good"),
        ("Motion blur type:", "Gaussian"),
        ("Deinterlace method:", "Blend fields"),
        ("Resample mode:", if v23 { "Disable resampling" } else { "Smart resample" }),
    ];
    if v23 {
        grid.insert(1, ("HDR Mode:", "Off"));
        grid.insert(6, ("Look modification transform:", "Off"));
    }
    for (k, (label, value)) in grid.into_iter().enumerate() {
        dialogs::row(ui, label, 175.0, false, |ui| dialogs::fixed_combo(ui, &format!("pp_g{k}"), value, 200.0));
    }
    ui.add_enabled(false, egui::Checkbox::new(&mut true, "Adjust source media to better match project or render settings"));
    ui.add_enabled_ui(false, |ui| {
        ui.label("Prerendered files folder:");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut std::env::temp_dir().display().to_string()).desired_width(310.0));
            let _ = ui.button("Browse...");
        });
        ui.add(egui::Checkbox::new(&mut false, "Start all new projects with these settings"));
    });
}

fn audio_tab(ui: &mut Ui, d: &mut ProjectSettings) {
    dialogs::row(ui, "Master bus mode:", 150.0, false, |ui| dialogs::fixed_combo(ui, "pa_bus", "Stereo", 200.0));
    dialogs::row(ui, "Number of stereo busses:", 150.0, false, |ui| dialogs::fixed_combo(ui, "pa_n", "0", 200.0));
    dialogs::row(ui, "Sample rate (Hz):", 150.0, true, |ui| {
        egui::ComboBox::from_id_salt("pa_sr").width(200.0).selected_text(d.sample_rate.to_string()).show_ui(ui, |ui| {
            for r in SAMPLE_RATES {
                ui.selectable_value(&mut d.sample_rate, r, r.to_string());
            }
        });
    });
    dialogs::row(ui, "Bit depth:", 150.0, false, |ui| dialogs::fixed_combo(ui, "pa_bits", "16", 200.0));
    dialogs::row(ui, "Resample and stretch quality:", 150.0, false, |ui| dialogs::fixed_combo(ui, "pa_q", "Good", 200.0));
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
        let mut r = Prefs { recent_max: 2, ..Default::default() };
        for p in ["a", "b", "a", "c"] {
            r.add_recent(std::path::Path::new(p));
        }
        assert_eq!(r.recent, ["c", "a"]);
    }
}
