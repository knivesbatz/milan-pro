//! File > Properties: project video/audio settings, applied through undo history.
use eframe::egui;
use crate::history::History;
use crate::project::{Project, ProjectSettings};

const FRAME_RATES: [f64; 8] = [23.976, 24.0, 25.0, 29.97, 30.0, 50.0, 59.94, 60.0];
const SAMPLE_RATES: [u32; 4] = [44_100, 48_000, 88_200, 96_000];

#[derive(Default)]
pub struct SettingsDialog {
    /// Some while the window is open: the edited copy.
    draft: Option<ProjectSettings>,
}

impl SettingsDialog {
    pub fn open(&mut self, project: &Project) {
        self.draft = Some(project.settings.clone());
    }

    pub fn show(&mut self, ctx: &egui::Context, project: &mut Project, history: &mut History) {
        let Some(d) = &mut self.draft else { return };
        let (mut open, mut ok, mut cancel) = (true, false, false);
        egui::Window::new("Project Properties").collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
            egui::Grid::new("project_props").num_columns(2).show(ui, |ui| {
                ui.label("Width");
                ui.add(egui::DragValue::new(&mut d.width).range(16..=8192));
                ui.end_row();
                ui.label("Height");
                ui.add(egui::DragValue::new(&mut d.height).range(16..=8192));
                ui.end_row();
                ui.label("Frame rate");
                egui::ComboBox::from_id_salt("fps").selected_text(format!("{:.3}", d.frame_rate)).show_ui(ui, |ui| {
                    for f in FRAME_RATES {
                        ui.selectable_value(&mut d.frame_rate, f, format!("{f:.3}"));
                    }
                });
                ui.end_row();
                ui.label("Sample rate");
                egui::ComboBox::from_id_salt("sr").selected_text(format!("{} Hz", d.sample_rate)).show_ui(ui, |ui| {
                    for r in SAMPLE_RATES {
                        ui.selectable_value(&mut d.sample_rate, r, format!("{r} Hz"));
                    }
                });
                ui.end_row();
            });
            ui.separator();
            ui.horizontal(|ui| {
                ok = ui.button("OK").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        if ok {
            // H.264/HEVC 4:2:0 output needs even dimensions.
            d.width &= !1;
            d.height &= !1;
            if *d != project.settings {
                history.push(project.clone());
                project.settings = d.clone();
                project.mark_modified();
            }
        }
        if ok || cancel || !open {
            self.draft = None;
        }
    }
}
