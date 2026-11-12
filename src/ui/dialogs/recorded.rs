//! Recorded Files: shown when a recording stops, with the files it made. Delete (asks first), Rename, Done.
use eframe::egui::*;
use crate::project::Project;

pub struct RecordedFiles {
    /// Media ids of the recorded files still in the project.
    pub ids: Vec<String>,
    sel: usize,
    /// Some while renaming: the new name, without ".wav".
    name: Option<String>,
    /// Delete was clicked: waiting on Yes / No.
    confirm: bool,
}

/// What the app does for the dialog.
pub enum Recorded {
    /// Remove the media and its events (one undo step), then the file.
    Delete(String),
    Rename(String, String),
    Done,
}

impl RecordedFiles {
    pub fn new(ids: Vec<String>) -> Self {
        Self { ids, sel: 0, name: None, confirm: false }
    }

    pub fn show(&mut self, ctx: &Context, project: &Project) -> Option<Recorded> {
        // Undo may have taken a recording away again.
        self.ids.retain(|id| project.media_pool.iter().any(|m| m.id == *id));
        if self.ids.is_empty() {
            return Some(Recorded::Done);
        }
        self.sel = self.sel.min(self.ids.len() - 1);
        let path = |id: &str| project.media_pool.iter().find(|m| m.id == id).map(|m| m.path.display().to_string()).unwrap_or_default();
        let (ids, sel, name, confirm) = (&self.ids, &mut self.sel, &mut self.name, &mut self.confirm);
        let (out, close) = super::modal(ctx, "recorded_files", "Recorded Files", vec2(520.0, 280.0), |ui| {
            ui.label("Recorded files:");
            let d = crate::ui::theme::get().dialog();
            Frame::NONE.fill(d.field).stroke(Stroke::new(1.0_f32, d.field_border)).inner_margin(4).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ScrollArea::vertical().max_height(120.0).auto_shrink([false, false]).show(ui, |ui| {
                    for (k, id) in ids.iter().enumerate() {
                        if ui.selectable_label(*sel == k, path(id)).clicked() {
                            *sel = k;
                        }
                    }
                });
            });
            ui.add_space(6.0);
            let id = ids[*sel].clone();
            if *confirm {
                ui.label("Delete this recording from the project and from the disk? This cannot be undone for the file.");
                return match super::button_row(ui, &[("Yes", true), ("No", true)]) {
                    Some(0) => {
                        *confirm = false;
                        Some(Recorded::Delete(id))
                    }
                    Some(_) => {
                        *confirm = false;
                        None
                    }
                    None => None,
                };
            }
            if let Some(n) = name {
                super::row(ui, "New name:", 70.0, true, |ui| ui.add(TextEdit::singleline(n).desired_width(330.0)));
                let ok = !n.trim().is_empty();
                return match super::button_row(ui, &[("OK", ok), ("Cancel", true)]) {
                    Some(0) => Some(Recorded::Rename(id, name.take().unwrap_or_default())),
                    Some(_) => {
                        *name = None;
                        None
                    }
                    None => None,
                };
            }
            match super::button_row(ui, &[("Done", true), ("Delete", true), ("Rename", true)]) {
                Some(0) => Some(Recorded::Done),
                Some(1) => {
                    *confirm = true;
                    None
                }
                Some(_) => {
                    let p = std::path::PathBuf::from(path(&id));
                    *name = Some(p.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned()));
                    None
                }
                None => None,
            }
        });
        if let Some(Recorded::Delete(id)) = &out {
            self.ids.retain(|i| i != id);
        }
        if close && out.is_none() {
            return Some(Recorded::Done);
        }
        out
    }
}
