mod app;
mod media;
mod project;
mod ui;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Milan Pro v0.1.0")
            .with_inner_size([1600.0, 900.0])
            .with_min_inner_size([960.0, 540.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Milan Pro",
        options,
        Box::new(|cc| Ok(Box::new(app::MeridianApp::new(cc)))),
    )
}
