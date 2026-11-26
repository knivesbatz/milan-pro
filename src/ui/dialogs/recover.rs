//! R42: the offer made when a project file cannot be read: open its `.mnle.bak` or its latest autosave instead (as an
//! unsaved copy, so neither is written over). R43: the "Recover Projects" dialog for the autosaves a crashed session
//! left behind: Open (as an unsaved copy), Discard, Keep for later.
use eframe::egui::*;
use crate::autosave::Recoverable;
use crate::project::CorruptOffer;

pub enum CorruptChoice {
    Backup,
    Autosave,
    Cancel,
}

/// The broken-file dialog for `offer`; Some once a button or × is clicked.
pub fn corrupt_window(ctx: &Context, offer: &CorruptOffer) -> Option<CorruptChoice> {
    let (hit, close) = super::modal(ctx, "corrupt_project", "Milan Pro", vec2(520.0, 230.0), |ui| {
        ui.label(RichText::new(format!("{} could not be opened.", offer.path.display())).strong());
        super::field_box(ui, 4, |ui| {
            ui.set_width(ui.available_width());
            ScrollArea::vertical().max_height(48.0).auto_shrink([false, false]).show(ui, |ui| ui.label(&offer.error));
        });
        ui.add_space(6.0);
        let when = |p: &std::path::Path| std::fs::metadata(p).and_then(|m| m.modified()).ok().map(local_time).unwrap_or_default();
        match &offer.backup {
            Some(b) => ui.label(format!("Backup from the last save: {} ({})", b.display(), when(b))),
            None => ui.label("No backup (.mnle.bak) beside the file."),
        };
        match &offer.autosave {
            Some(a) => ui.label(format!("Latest autosave: {} ({})", a.display(), when(a))),
            None => ui.label("No autosave for this file."),
        };
        ui.label("Either opens as an unsaved copy; the original and the backups are left as they are.");
        ui.add_space(10.0);
        super::button_row(ui, &[("Open the backup", offer.backup.is_some()), ("Open latest autosave", offer.autosave.is_some()), ("Cancel", true)])
    });
    match hit {
        Some(0) => Some(CorruptChoice::Backup),
        Some(1) => Some(CorruptChoice::Autosave),
        Some(_) => Some(CorruptChoice::Cancel),
        None if close => Some(CorruptChoice::Cancel),
        None => None,
    }
}

/// `t` as "YYYY-MM-DD HH:MM" in local time (R43: shown for autosaves; R44: crash report names).
pub fn local_time(t: std::time::SystemTime) -> String {
    crate::logger::format_time(t, false)
}

/// R43: the autosaves crashed sessions left, newer than their projects.
pub struct Recover {
    pub items: Vec<Recoverable>,
    sel: usize,
}

pub enum RecoverChoice {
    /// Open `items[n]`'s autosave as an unsaved copy.
    Open(usize),
    /// Delete `items[n]`'s autosave files.
    Discard(usize),
    /// Close; everything stays and is offered again at the next start.
    KeepForLater,
}

impl Recover {
    pub fn new(items: Vec<Recoverable>) -> Self {
        Self { items, sel: 0 }
    }

    pub fn show(&mut self, ctx: &Context) -> Option<RecoverChoice> {
        if self.items.is_empty() {
            return Some(RecoverChoice::KeepForLater);
        }
        self.sel = self.sel.min(self.items.len() - 1);
        let (items, sel) = (&self.items, &mut self.sel);
        let (hit, close) = super::modal(ctx, "recover_projects", "Recover Projects", vec2(560.0, 300.0), |ui| {
            ui.label(RichText::new("Milan Pro did not close properly last time. These projects have autosaved changes that were never saved:").strong());
            ui.add_space(4.0);
            super::field_box(ui, 4, |ui| {
                ui.set_width(ui.available_width());
                ScrollArea::vertical().max_height(120.0).auto_shrink([false, false]).show(ui, |ui| {
                    for (k, it) in items.iter().enumerate() {
                        let where_ = it.path.as_ref().map_or_else(|| "never saved".to_string(), |p| p.display().to_string());
                        let text = format!("{}  —  autosaved {}  ({where_})", it.name, local_time(it.saved_at));
                        if ui.selectable_label(*sel == k, text).clicked() {
                            *sel = k;
                        }
                    }
                });
            });
            ui.add_space(6.0);
            ui.label("Open loads the autosave as an unsaved copy; the project file is not changed until you save. Discard deletes the autosave. Keep for later asks again next time.");
            ui.add_space(10.0);
            super::button_row(ui, &[("Open", true), ("Discard", true), ("Keep for later", true)])
        });
        match hit {
            Some(0) => Some(RecoverChoice::Open(*sel)),
            Some(1) => Some(RecoverChoice::Discard(*sel)),
            Some(_) => Some(RecoverChoice::KeepForLater),
            None if close => Some(RecoverChoice::KeepForLater),
            None => None,
        }
    }

    /// `items[n]` is dealt with: it goes, and its marker too once nothing else refers to it.
    pub fn settled(&mut self, n: usize) {
        let it = self.items.remove(n);
        if !self.items.iter().any(|o| o.marker == it.marker) {
            let _ = std::fs::remove_file(&it.marker);
        }
    }
}
