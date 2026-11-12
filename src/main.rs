#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod autosave;
mod decode;
mod demo;
mod edit;
mod history;
mod media;
mod project;
mod render;
mod settings;
mod ui;
mod updater;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let mut launch = demo::Launch::from_env();
    let look = launch.look.unwrap_or_else(|| if launch.demo { Default::default() } else { settings::Prefs::load().look });
    launch.look = Some(look);
    // VP23 draws its own 32 px title bar (4.6); the app switches decorations live on look changes.
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Untitled - Milan Pro")
        .with_inner_size([1600.0, 900.0])
        .with_min_inner_size([960.0, 540.0])
        .with_decorations(look.look == ui::theme::UiLook::Vegas16);
    if launch.screenshot.is_some() || std::env::var_os("MILAN_UPDATE_AUTO").is_some() {
        // Never steal focus from the user; the app resizes to 1920x1080 px on its first frame.
        viewport = viewport.with_active(false).with_position([0.0, 0.0]).with_min_inner_size([100.0, 100.0]);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(
        "Milan Pro",
        options,
        Box::new(|cc| Ok(Box::new(app::MeridianApp::new(cc, launch)))),
    )
}
