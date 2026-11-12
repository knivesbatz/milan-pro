//! Segoe UI loaded at runtime from the Windows fonts folder (§3); never redistributed.
//! Missing files fall back to egui's bundled fonts.
use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use std::sync::Arc;

pub const SEMIBOLD: &str = "semibold";
pub const BOLD: &str = "bold";

pub fn install(ctx: &egui::Context) {
    let dir = std::env::var_os("WINDIR").map_or_else(|| "C:\\Windows".into(), std::path::PathBuf::from).join("Fonts");
    let load = |file: &str| std::fs::read(dir.join(file)).ok();
    let mut defs = FontDefinitions::default();
    let fallback = defs.families[&FontFamily::Proportional].clone();
    let regular = load("segoeui.ttf");
    if let Some(bytes) = &regular {
        defs.font_data.insert("segoe".into(), Arc::new(FontData::from_owned(bytes.clone())));
        defs.families.get_mut(&FontFamily::Proportional).unwrap().insert(0, "segoe".into());
    }
    // Semibold/bold families fall back to regular Segoe, then egui's fonts (symbols, emoji).
    for (family, file) in [(SEMIBOLD, "seguisb.ttf"), (BOLD, "segoeuib.ttf")] {
        let mut list = fallback.clone();
        if regular.is_some() {
            list.insert(0, "segoe".into());
        }
        if let Some(bytes) = load(file) {
            defs.font_data.insert(family.into(), Arc::new(FontData::from_owned(bytes)));
            list.insert(0, family.into());
        }
        defs.families.insert(FontFamily::Name(family.into()), list);
    }
    ctx.set_fonts(defs);
}
