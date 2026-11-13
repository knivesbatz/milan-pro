//! R16a: the Project Notes window (VEGAS 23, View › Window › Project Notes): notes kept with the project, each at a
//! timeline position (a click on its time goes there) and marked Resolved when done; Add New Note (lower left) adds
//! one at the cursor. Notes are typed text, so they stay out of the undo history (`history::restore`).
use eframe::egui::*;
use crate::project::{Note, Project};
use super::theme;

pub fn project_notes(ui: &mut Ui, project: &mut Project) {
    let t = theme::get();
    let mut changed = false;
    let mut gone = None;
    ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            if ui.button("Add New Note").clicked() {
                project.notes.push(Note { at: project.timeline.cursor_position, ..Default::default() });
                changed = true;
            }
        });
        ui.with_layout(Layout::top_down(Align::Min), |ui| {
            ScrollArea::vertical().id_salt("project_notes").auto_shrink([false, false]).show(ui, |ui| {
                let w = ui.available_width() - 12.0;
                for (i, n) in project.notes.iter_mut().enumerate() {
                    Frame::NONE.inner_margin(6).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let at = project.settings.ruler_time(n.at);
                            if ui.link(at).on_hover_text("Go to this position").clicked() {
                                project.timeline.cursor_position = n.at;
                            }
                            changed |= ui.checkbox(&mut n.resolved, "Resolved").changed();
                            if ui.small_button("×").on_hover_text("Delete note").clicked() {
                                gone = Some(i);
                            }
                        });
                        let color = if n.resolved { t.text_dim } else { t.text };
                        changed |= ui.add(TextEdit::multiline(&mut n.text).desired_width(w).desired_rows(2).text_color(color).hint_text("Note")).changed();
                    });
                    ui.separator();
                }
            });
        });
    });
    if let Some(i) = gone {
        project.notes.remove(i);
        changed = true;
    }
    if changed {
        project.mark_modified();
    }
}
