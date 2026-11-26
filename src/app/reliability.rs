//! R42-R46: the reliability features' app state and dialogs: Save Version, the offer when a project file cannot be
//! read, the notes from opening a file (migration, read-only, repairs), and (R43-R46) crash recovery, the crash-report
//! offer, the diagnostic report and the roll-back offer.
use eframe::egui;
use crate::project::CorruptOffer;
use crate::ui::dialogs::{self, CorruptChoice};
use super::MeridianApp;

#[derive(Default)]
pub struct Rel {
    /// File › Save Version, open.
    pub save_version: Option<dialogs::SaveVersion>,
    /// A project file that could not be read, with its backup and autosave.
    pub corrupt: Option<CorruptOffer>,
    /// What `Project::open` had to say about the file just opened (intro, lines).
    pub open_notes: Option<(String, Vec<String>)>,
}

impl Rel {
    /// A modal of this module is open (keys go to it, not the timeline).
    pub fn modal(&self) -> bool {
        self.save_version.is_some() || self.corrupt.is_some() || self.open_notes.is_some()
    }
}

impl MeridianApp {
    pub(super) fn show_reliability(&mut self, ctx: &egui::Context) {
        if let Some(d) = &mut self.rel.save_version {
            match d.show(ctx) {
                Some(true) => {
                    let (path, note) = (d.path.clone(), d.note.trim().to_string());
                    self.rel.save_version = None;
                    self.project.version_note = (!note.is_empty()).then_some(note);
                    let r = self.project.save(&path).map(|_| true).map_err(|e| format!("{}: {e}", path.display()));
                    if self.save_result(r) {
                        log::info!("saved version {}", path.display());
                    }
                }
                Some(false) => self.rel.save_version = None,
                None => {}
            }
        }
        if let Some(offer) = &self.rel.corrupt {
            let file = match dialogs::corrupt_window(ctx, offer) {
                Some(CorruptChoice::Backup) => offer.backup.clone(),
                Some(CorruptChoice::Autosave) => offer.autosave.clone(),
                Some(CorruptChoice::Cancel) => {
                    self.rel.corrupt = None;
                    None
                }
                None => None,
            };
            if let Some(file) = file {
                let offer = self.rel.corrupt.take().expect("the offer is open");
                match offer.open_copy(&file) {
                    Ok(p) => {
                        log::info!("opened {} as an unsaved copy of {}", file.display(), offer.path.display());
                        self.switch_project(p);
                        self.message = Some(format!("Opened {} as an unsaved copy; Save asks where to put it.", file.display()));
                    }
                    Err(e) => self.message = Some(format!("Could not open {}: {e}", file.display())),
                }
            }
        }
        if self.rel.open_notes.as_ref().is_some_and(|(intro, lines)| dialogs::report(ctx, "Open Project", intro, lines)) {
            self.rel.open_notes = None;
        }
    }
}
