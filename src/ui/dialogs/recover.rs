//! R42: the offer made when a project file cannot be read: open its `.mnle.bak` or its latest autosave instead (as an
//! unsaved copy, so neither is written over). R43 adds the "Recover projects" dialog for the autosaves a crashed
//! session left behind.
use eframe::egui::*;
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
