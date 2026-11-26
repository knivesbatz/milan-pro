//! R42: File › Save Version (Ctrl+Alt+S, VEGAS 23): the project is saved as the next numbered copy beside its file
//! (`Show.mnle` → `Show_01.mnle`, `Show_02.mnle`, ...) with an optional note, and editing goes on in the new file.
use eframe::egui::*;
use std::path::{Path, PathBuf};
use crate::ui::widgets;

pub struct SaveVersion {
    /// The file the version is saved as.
    pub path: PathBuf,
    pub note: String,
}

/// The next free `<base>_NN.mnle` beside `current`, where `<base>` is its stem without an `_NN` of its own.
pub fn next_version_path(current: &Path) -> PathBuf {
    let stem = current.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Untitled".into());
    let base = match stem.rsplit_once('_') {
        Some((b, n)) if !b.is_empty() && n.len() >= 2 && n.bytes().all(|c| c.is_ascii_digit()) => b.to_string(),
        _ => stem,
    };
    let ext = current.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_else(|| "mnle".into());
    (1..)
        .map(|n| current.with_file_name(format!("{base}_{n:02}.{ext}")))
        .find(|p| !p.exists())
        .expect("some version number is free")
}

impl SaveVersion {
    pub fn new(current: &Path) -> Self {
        Self { path: next_version_path(current), note: String::new() }
    }

    /// Some(true) on OK (save to `path` with `note`), Some(false) on Cancel or ×.
    pub fn show(&mut self, ctx: &Context) -> Option<bool> {
        let (path, note) = (&self.path, &mut self.note);
        let (hit, close) = super::modal(ctx, "save_version", "Save Version", vec2(460.0, 200.0), |ui| {
            ui.label("The project is saved as a new version and editing continues in it:");
            super::field_box(ui, 4, |ui| {
                ui.set_width(ui.available_width());
                ui.label(path.display().to_string());
            });
            ui.add_space(6.0);
            ui.label("Note (optional):");
            widgets::edit(ui, TextEdit::singleline(note).desired_width(f32::INFINITY).hint_text("What changed in this version"));
            ui.add_space(12.0);
            super::button_row(ui, &[("OK", true), ("Cancel", true)])
        });
        match hit {
            Some(0) => Some(true),
            Some(_) => Some(false),
            None if close => Some(false),
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_numbered_beside_the_file() {
        let dir = std::env::temp_dir().join(format!("milan-version-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let show = dir.join("Show.mnle");
        assert_eq!(next_version_path(&show), dir.join("Show_01.mnle"));
        std::fs::write(dir.join("Show_01.mnle"), b"x").unwrap();
        assert_eq!(next_version_path(&show), dir.join("Show_02.mnle"));
        // From a version, the base name is kept and the next free number taken.
        assert_eq!(next_version_path(&dir.join("Show_01.mnle")), dir.join("Show_02.mnle"));
        std::fs::write(dir.join("Show_02.mnle"), b"x").unwrap();
        assert_eq!(next_version_path(&dir.join("Show_01.mnle")), dir.join("Show_03.mnle"));
        // A stem that only looks numbered (one digit, or no base) is a base of its own.
        assert_eq!(next_version_path(&dir.join("Take_1.mnle")), dir.join("Take_1_01.mnle"));
        assert_eq!(next_version_path(&dir.join("_01.mnle")), dir.join("_01_01.mnle"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
