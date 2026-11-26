//! R30: File › Import › Media from Project...: another `.mnle`'s media pool with a check box per item (files already
//! in this project greyed), Add puts the checked ones into Project Media as they were probed there.
use std::path::PathBuf;
use eframe::egui::*;
use crate::project::MediaItem;
use crate::ui::{theme, widgets};

pub struct MediaFromProject {
    pub path: PathBuf,
    items: Vec<MediaItem>,
    checked: Vec<bool>,
    error: Option<String>,
}

impl MediaFromProject {
    pub fn new(path: PathBuf) -> Self {
        let (items, error) = match crate::interchange::archive::media_of(&path) {
            Ok(items) => (items, None),
            Err(e) => (Vec::new(), Some(e.to_string())),
        };
        let checked = vec![true; items.len()];
        Self { path, items, checked, error }
    }

    /// Some(items to add) when closed (empty on Cancel); None while open. `pool` is this project's media, so items
    /// already here are shown greyed and left out.
    pub fn show(&mut self, ctx: &Context, pool: &[MediaItem]) -> Option<Vec<MediaItem>> {
        let in_pool = |m: &MediaItem| if m.generator.is_some() { pool.iter().any(|p| p.id == m.id) } else { pool.iter().any(|p| p.path == m.path) };
        let title = format!("Media from {}", self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        let (hit, close) = super::modal(ctx, "media_from_project", &title, vec2(560.0, 380.0), |ui| {
            if let Some(e) = &self.error {
                ui.colored_label(theme::get().warning_text, format!("Could not open the project: {e}"));
                ui.add_space(8.0);
                return super::button_row(ui, &[("Close", true)]);
            }
            let (new, already) = self.items.iter().fold((0, 0), |(n, a), m| if in_pool(m) { (n, a + 1) } else { (n + 1, a) });
            ui.label(format!("{new} media item(s) to add{}.", if already > 0 { format!("; {already} already in this project") } else { String::new() }));
            ui.horizontal(|ui| {
                if widgets::push_button(ui, "Check All").clicked() {
                    self.checked.iter_mut().for_each(|c| *c = true);
                }
                if widgets::push_button(ui, "Check None").clicked() {
                    self.checked.iter_mut().for_each(|c| *c = false);
                }
            });
            super::field_box(ui, 4, |ui| {
                ui.set_width(ui.available_width());
                ScrollArea::vertical().max_height(220.0).auto_shrink([false, false]).show(ui, |ui| {
                    for (m, checked) in self.items.iter().zip(&mut self.checked) {
                        let here = in_pool(m);
                        let kind = if m.generator.is_some() { "generated" } else if m.nested { "nested project" } else if m.is_offline() || !m.path.exists() { "offline" } else { "" };
                        let label = format!("{}{}{}", m.name, if kind.is_empty() { String::new() } else { format!("  ({kind})") }, if here { "  - already in this project" } else { "" });
                        widgets::checkbox_enabled(ui, !here, checked, label).on_hover_text(m.path.display().to_string());
                    }
                    if self.items.is_empty() {
                        ui.label("The project has no media.");
                    }
                });
            });
            ui.add_space(8.0);
            let any = self.items.iter().zip(&self.checked).any(|(m, c)| *c && !in_pool(m));
            super::button_row(ui, &[("Add", any), ("Cancel", true)])
        });
        match hit {
            Some(0) if self.error.is_none() => Some(self.items.iter().zip(&self.checked).filter(|(m, c)| **c && !in_pool(m)).map(|(m, _)| m.clone()).collect()),
            Some(_) => Some(Vec::new()),
            None => close.then(Vec::new),
        }
    }
}
