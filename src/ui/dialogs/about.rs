//! Help › About Milan Pro: the brand banner (assets/brand/about-banner-480x120.png) over the version and credits in
//! Tahoma, on the `#D4D0C8` sheet.
use eframe::egui::*;
use super::super::theme;
use super::super::widgets::{self, Brand};

/// Shows the dialog; true once OK or × closed it.
pub fn about(ctx: &Context) -> bool {
    let t = theme::get();
    let version = env!("CARGO_PKG_VERSION");
    let (ok, close) = super::modal(ctx, "about", "About Milan Pro", vec2(500.0, 296.0), |ui| {
        let d = t.dialog();
        ui.spacing_mut().item_spacing.y = 4.0;
        let (r, _) = ui.allocate_exact_size(vec2(480.0, 120.0), Sense::hover());
        let tex = widgets::brand_tex(ui.ctx(), Brand::AboutBanner);
        ui.painter().image(tex.id(), widgets::px(r), Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
        ui.add_space(6.0);
        ui.label(RichText::new(format!("Milan Pro version {version}")).font(t.bold(11.0)).color(d.text));
        ui.label("Nonlinear editing system. Open source under the MIT licence.");
        ui.add_space(4.0);
        super::group(ui, "Credits");
        for line in [
            "Built in Rust on eframe / egui; media through FFmpeg; OpenFX plug-in host.",
            "Interface after the 2005-2007 Windows video editors, laid out as VEGAS Pro 23.",
            "Brand art rendered by tools/brand (Blender and Pillow).",
        ] {
            ui.label(RichText::new(line).color(d.text));
        }
        ui.add_space(8.0);
        super::button_row(ui, &[("OK", true)]).is_some()
    });
    ok || close
}
