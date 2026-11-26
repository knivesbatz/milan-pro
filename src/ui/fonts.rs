//! Segoe UI (VEGAS looks, §3) and Tahoma (Classic 2006, CLASSIC_2006_SPEC.md §3) loaded at runtime from the Windows
//! fonts folder; never redistributed. Missing files fall back to Segoe UI, then egui's bundled fonts.
use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub const SEMIBOLD: &str = "semibold";
pub const BOLD: &str = "bold";
/// Classic 2006 text face (Tahoma 8 pt = 11 px) and its bold.
pub const TAHOMA: &str = "tahoma";
pub const TAHOMA_BOLD: &str = "tahoma-bold";

/// `install` has bound the named families; before that (tests with a bare context) the theme uses egui's own.
static INSTALLED: AtomicBool = AtomicBool::new(false);

pub fn installed() -> bool {
    INSTALLED.load(Ordering::Relaxed)
}

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
    // Tahoma for the Classic 2006 look; its bold falls back to regular Tahoma, both to Segoe UI, then egui's fonts.
    let tahoma = load("tahoma.ttf");
    for (family, file) in [(TAHOMA, None), (TAHOMA_BOLD, Some("tahomabd.ttf"))] {
        let mut list = fallback.clone();
        if regular.is_some() {
            list.insert(0, "segoe".into());
        }
        if let Some(bytes) = &tahoma {
            if !defs.font_data.contains_key(TAHOMA) {
                defs.font_data.insert(TAHOMA.into(), Arc::new(FontData::from_owned(bytes.clone())));
            }
            list.insert(0, TAHOMA.into());
        }
        if let Some(bytes) = file.and_then(load) {
            defs.font_data.insert(family.into(), Arc::new(FontData::from_owned(bytes)));
            list.insert(0, family.into());
        }
        defs.families.insert(FontFamily::Name(family.into()), list);
    }
    ctx.set_fonts(defs);
    INSTALLED.store(true, Ordering::Relaxed);
}
