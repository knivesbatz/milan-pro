//! Startup splash: a borderless, centred, never-active window showing the brand splash
//! (assets/brand/splash-520x320.png) with a thin progress bar for `SECS`, then gone. Preferences › General
//! "Show splash screen at startup" (on by default); never in an off-screen run (the screenshot harness,
//! `MILAN_UPDATE_AUTO`), which must open no window the user can see.
use eframe::egui::*;
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
        let pos = ((mon - SIZE) * 0.5).round().to_pos2();
        let b = ViewportBuilder::default()
            .with_title("Milan Pro")
            .with_decorations(false)
            .with_active(false)
            .with_taskbar(false)
            .with_always_on_top()
            .with_resizable(false)
            .with_inner_size(SIZE)
            .with_position(pos)
            .with_icon(crate::icon());
        ctx.show_viewport_immediate(ViewportId::from_hash_of("milan_splash"), b, |vctx, class| {
            if class == ViewportClass::Embedded {
                return; // no native windows (tests): nothing to show
            }
            CentralPanel::default().frame(Frame::NONE).show(vctx, |ui| {
                let r = widgets::px(ui.max_rect());
                let p = ui.painter();
                let tex = widgets::brand_tex(ui.ctx(), Brand::Splash);
                p.image(tex.id(), r, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
                // A thin cyan progress bar in a dark groove along the bottom of the art.
                let bar = Rect::from_min_max(pos2(r.min.x + 24.0, r.max.y - 20.0), pos2(r.max.x - 24.0, r.max.y - 14.0));
                p.rect_filled(bar, 0.0, Color32::from_black_alpha(170));
                p.rect_stroke(bar, 0.0, Stroke::new(1.0_f32, Color32::from_rgb(0x3A, 0x44, 0x52)), StrokeKind::Inside);
                let fill = bar.shrink(1.0);
                p.rect_filled(fill.with_max_x(fill.min.x + fill.width() * f), 0.0, Color32::from_rgb(0x3C, 0xE6, 0xFF));
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
