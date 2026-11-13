//! Chooser thumbnails: a preset rendered on a procedural sample picture (100 x 60).
use eframe::egui::{Color32, ColorImage};
use crate::engine::generate::{self, fill};
use crate::engine::plugins::{FxEval, PluginInfo, PluginKind, Preset, find, preset_eval};
use crate::engine::{transition, vfx};

pub const W: usize = 100;
pub const H: usize = 60;

/// Sky, sun, ground and a red box.
fn sample() -> ColorImage {
    fill(W, H, |x, y| {
        let (xf, yf) = (x as f32, y as f32);
        if (xf - 72.0).hypot(yf - 16.0) < 9.0 {
            Color32::from_rgb(255, 214, 90)
        } else if (14..34).contains(&x) && (26..46).contains(&y) {
            Color32::from_rgb(200, 50, 40)
        } else if y >= 38 {
            Color32::from_rgb(40, 120 + (x as u8 % 20), 50)
        } else {
            Color32::from_rgb(70, 130 + y as u8, 220)
        }
    })
}

/// `info` with `preset` (None = defaults); a transition is shown at progress `p` from the sample to its negative.
pub fn preset_thumb(info: &'static PluginInfo, preset: Option<&Preset>, p: f32) -> ColorImage {
    eval_thumb(info.kind, &preset_eval(info, preset), p)
}

/// `preset_thumb` for any evaluated plug-in (a user preset).
pub fn eval_thumb(kind: PluginKind, e: &FxEval, p: f32) -> ColorImage {
    match kind {
        PluginKind::VideoFx => {
            let mut img = sample();
            vfx::apply(&mut img, e, 1);
            img
        }
        PluginKind::Transition => {
            let a = sample();
            let mut b = a.clone();
            vfx::apply(&mut b, &preset_eval(find("milan.fx.invert").expect("built in"), None), 1);
            transition::mix(e, &a, &b, p, 1)
        }
        PluginKind::Generator => generate::render(e, W as u32, H as u32, 0.0),
        // The sample through the compositor, with a radial gradient as its map.
        PluginKind::Compositor => {
            let mut img = sample();
            let map = fill(W, H, |x, y| Color32::from_gray((255.0 * (1.0 - (x as f32 - 50.0).hypot(y as f32 - 30.0) / 50.0).clamp(0.0, 1.0)) as u8));
            vfx::compositor(&mut img, &map, e, 1.0, 1);
            img
        }
        // Audio FX have no chooser.
        PluginKind::AudioFx => sample(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::plugins::PLUGINS;

    #[test]
    fn every_catalogue_entry_renders() {
        for info in PLUGINS {
            for pr in info.presets.iter().map(Some).chain([None]) {
                assert_eq!(preset_thumb(info, pr, 0.5).size, [W, H], "{}", info.id);
            }
            if info.kind == PluginKind::Transition {
                assert!(preset_thumb(info, None, 0.0).pixels != preset_thumb(info, None, 1.0).pixels, "{}", info.id);
            }
        }
    }
}
