//! System fonts for Titles & Text. A font is named by its file stem in the Windows fonts folders.
use ab_glyph::FontArc;
use eframe::egui;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

fn files() -> impl Iterator<Item = PathBuf> {
    let windir = std::env::var_os("WINDIR").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
    let user = std::env::var_os("LOCALAPPDATA").map(|l| PathBuf::from(l).join(r"Microsoft\Windows\Fonts"));
    std::iter::once(windir.join("Fonts"))
        .chain(user)
        .filter_map(|d| std::fs::read_dir(d).ok())
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()).is_some_and(|e| ["ttf", "otf", "ttc"].contains(&e.to_ascii_lowercase().as_str())))
}

/// Installed font names, sorted.
// shortcut: names are file stems (arialbd, segoeui); parse the name table with owned_ttf_parser for family names if users ask.
pub fn list() -> Vec<String> {
    let mut v: Vec<String> = files().filter_map(|p| Some(p.file_stem()?.to_string_lossy().into_owned())).collect();
    v.sort_by_key(|s| s.to_lowercase());
    v.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    v
}

/// The font named `name`, or egui's bundled font when it is not installed or will not parse.
pub fn load(name: &str) -> FontArc {
    static CACHE: OnceLock<Mutex<HashMap<String, FontArc>>> = OnceLock::new();
    let mut cache = CACHE.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    cache
        .entry(name.to_ascii_lowercase())
        .or_insert_with(|| {
            let path = files().find(|p| p.file_stem().is_some_and(|s| s.to_string_lossy().eq_ignore_ascii_case(name)));
            path.and_then(|p| FontArc::try_from_vec(std::fs::read(p).ok()?).ok()).unwrap_or_else(fallback)
        })
        .clone()
}

pub fn fallback() -> FontArc {
    static F: OnceLock<FontArc> = OnceLock::new();
    F.get_or_init(|| {
        let defs = egui::FontDefinitions::default();
        FontArc::try_from_vec(defs.font_data["Ubuntu-Light"].font.to_vec()).expect("egui's bundled font parses")
    })
    .clone()
}
