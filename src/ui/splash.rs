//! Startup splash: a borderless, centred, never-active window showing the brand splash
//! (assets/brand/splash-520x320.png) with a thin progress bar for `SECS`, then gone. Preferences › General
//! "Show splash screen at startup" (on by default); never in an off-screen run (the screenshot harness,
//! `MILAN_UPDATE_AUTO`), which must open no window the user can see.
use eframe::egui::*;
use super::theme;
use super::widgets::{self, Brand};

/// How long the splash stays, in seconds.
pub const SECS: f64 = 1.2;
const SIZE: Vec2 = vec2(520.0, 320.0);

pub struct Splash {
    start: Option<f64>,
}

impl Splash {
    pub fn new() -> Self {
        Self { start: None }
    }

    /// Whether this run shows the splash: the preference, and never off-screen (`crate::offscreen_run`).
    pub fn wanted(pref: bool, offscreen: bool) -> bool {
        pref && !offscreen
    }

    /// Call every frame; false once the splash has closed (drop it then).
    pub fn show(&mut self, ctx: &Context) -> bool {
        let now = ctx.input(|i| i.time);
        let start = *self.start.get_or_insert(now);
        let f = ((now - start) / SECS) as f32;
        if f >= 1.0 {
            return false;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(30));
        // Centred on the main window's monitor; `with_active(false)`: it never takes the focus.
        let mon = ctx.input(|i| i.viewport().monitor_size).filter(|m| m.x > 0.0 && m.y > 0.0).unwrap_or(vec2(1920.0, 1080.0));
        // R48: the bitmap at a whole number of pixels per pixel (`widgets::brand_size`), so never blurred.
        let size = widgets::brand_size(SIZE);
        let pos = ((mon - size) * 0.5).round().to_pos2();
        let b = ViewportBuilder::default()
            .with_title("Milan Pro")
            .with_decorations(false)
            .with_active(false)
            .with_taskbar(false)
            .with_always_on_top()
            .with_resizable(false)
            .with_inner_size(size)
            .with_position(pos)
            .with_icon(crate::icon());
        ctx.show_viewport_immediate(ViewportId::from_hash_of("milan_splash"), b, |vctx, class| {
            if class == ViewportClass::Embedded {
                return; // no native windows (tests): nothing to show
            }
            CentralPanel::default().frame(Frame::NONE).show(vctx, |ui| {
                let r = Rect::from_min_size(widgets::px(ui.max_rect()).min, size);
                let p = ui.painter();
                widgets::brand_image(p, r, Brand::Splash, SIZE);
                // Bitmap pixels to points.
                let u = size.x / SIZE.x;
                // The version at the right of the art's legal line, and the design system's progress bar (a navy
                // groove, the selection-blue fill) along the bottom of the brushed-aluminium footer.
                let ink = theme::get().ink.text;
                let version = format!("Version {}", env!("CARGO_PKG_VERSION"));
                p.text(pos2(r.max.x - 24.0 * u, r.min.y + 276.0 * u), Align2::RIGHT_TOP, version, FontId::proportional(11.0 * u), ink);
                let bar = widgets::px(Rect::from_min_max(pos2(r.min.x + 24.0 * u, r.max.y - 20.0 * u), pos2(r.max.x - 24.0 * u, r.max.y - 14.0 * u)));
                widgets::progress_blocks(p, bar.expand2(vec2(0.0, u)), f, Color32::TRANSPARENT, &theme::get().metal);
            });
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The harness and automatic updates never see a splash; the preference turns it off.
    #[test]
    fn splash_skipped_off_screen() {
        assert!(Splash::wanted(true, false));
        assert!(!Splash::wanted(true, true));
        assert!(!Splash::wanted(false, false));
        assert!(crate::settings::Prefs::default().show_splash);
        // Without native windows the splash runs its course and closes on its own.
        let ctx = Context::default();
        let mut s = Splash::new();
        let _ = ctx.run(RawInput::default(), |ctx| assert!(s.show(ctx)));
        let late = RawInput { time: Some(SECS + 0.1), ..Default::default() };
        let _ = ctx.run(late, |ctx| assert!(!s.show(ctx)));
    }
}
