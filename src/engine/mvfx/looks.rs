//! MVFX looks: grades, film stock, glow and glints, video and webcam degradation, graphic looks. Kernels are mirrored
//! in `gpu/mvfx_looks.wgsl` (see `pm` for the rules).
use eframe::egui::Color32;
use super::pm::{self, Img, M, fw, hash, hash3, luma, mix, pack, premul, q8, rnd, straight};
use super::{BLACK, DEFAULT, Kernel, Src, Step, WHITE, choice, color, f, flag, k, preset, text, fx_row as row};
use crate::engine::plugins::{FxEval, GpuPort, PluginInfo, Preset};
use crate::engine::vfx::ColorOp;

const POSITIONS: &[&str] = &["Left", "Right", "Top", "Bottom", "Center"];

pub static PLUGINS: &[PluginInfo] = &[
    row("milan.fx.bleach_bypass", "Bleach Bypass", "Film", "Color", "Skips the bleach: desaturated, high-contrast, crushed blacks", GpuPort::ColorOp,
        &[f("amount", "Amount", 0.0, 1.0, 1.0), f("contrast", "Contrast", 0.0, 1.0, 0.5), f("saturation", "Saturation", 0.0, 1.0, 0.35), f("crush", "Black crush", 0.0, 0.3, 0.06)],
        &[DEFAULT, preset("Emo 2005 Bleach", &[("contrast", 0.75), ("saturation", 0.25), ("crush", 0.1)]), preset("Silver Retention", &[("contrast", 0.6), ("saturation", 0.1)]), preset("Light Skip", &[("amount", 0.5)])]),
    row("milan.fx.cross_process", "Cross Process", "Film", "Color", "Slide film in negative chemistry: cyan shadows, yellow highlights, hard colour", GpuPort::ColorOp,
        &[f("amount", "Amount", 0.0, 1.0, 1.0), f("strength", "Strength", 0.0, 1.0, 0.7), f("saturation", "Saturation", 0.0, 2.0, 1.25)],
        &[DEFAULT, preset("Pop-Punk Cross Process", &[("strength", 0.9), ("saturation", 1.4)]), preset("Subtle", &[("strength", 0.4), ("saturation", 1.1)])]),
    row("milan.fx.posterize", "Posterize", "Stylize", "Creative", "Reduces each colour channel to a few levels", GpuPort::ColorOp,
        &[f("levels", "Levels", 2.0, 32.0, 6.0)],
        &[DEFAULT, preset("Poster", &[("levels", 4.0)]), preset("Comic", &[("levels", 3.0)])]),
    row("milan.fx.threshold", "Threshold", "Stylize", "Creative", "Turns the picture black and white at a brightness threshold", GpuPort::ColorOp,
        &[f("threshold", "Threshold", 0.0, 1.0, 0.5)],
        &[DEFAULT, preset("25%", &[("threshold", 0.25)]), preset("75%", &[("threshold", 0.75)])]),
    row("milan.fx.gradient_map", "Gradient Map", "Adjust", "Color", "Maps brightness onto a gradient (duotones, cross-processed tints)", GpuPort::Mvfx,
        &[f("amount", "Amount", 0.0, 1.0, 1.0), choice("mode", "Mode", &["Blend", "Add"], 0), text("stops", "Gradient", "0:#000000ff;1:#ffffffff")],
        &[
            DEFAULT,
            Preset { name: "Teal and Orange", vals: &[("amount", 0.6)], text: &[("stops", "0:#0b2a33ff;0.5:#6f8f8aff;1:#ffc27aff")] },
            Preset { name: "Cyan Night", vals: &[("amount", 0.8)], text: &[("stops", "0:#000814ff;0.6:#1d6f8cff;1:#d8fbffff")] },
            Preset { name: "Sepia Tone", vals: &[], text: &[("stops", "0:#1a0e05ff;0.5:#8a5a32ff;1:#f6e3c1ff")] },
        ]),
    row("milan.fx.glow", "Glow", "Light", "Light", "Makes the bright parts of the picture glow", GpuPort::Mvfx,
        &[f("glow_percent", "Glow percent", 0.0, 1.0, 0.25), f("intensity", "Intensity", 0.0, 5.0, 1.0), f("suppression", "Suppression", 0.0, 1.0, 0.0), color("color", "Color", WHITE)],
        &[DEFAULT, preset("White Soft Glow", &[("glow_percent", 0.5), ("intensity", 1.0), ("suppression", 0.25)]), preset("White Intense Glow", &[("glow_percent", 0.8), ("intensity", 3.0)]), preset("Crunk Bling Glow", &[("glow_percent", 0.35), ("intensity", 2.2), ("color.r", 1.0), ("color.g", 0.85), ("color.b", 0.55)]), preset("Red Glow", &[("glow_percent", 0.4), ("intensity", 1.5), ("color.g", 0.15), ("color.b", 0.1)])]),
    row("milan.fx.soft_contrast", "Soft Contrast", "Light", "Light", "Diffusion-filter softness: blooming highlights, gentler contrast, a tint and a vignette", GpuPort::Mvfx,
        &[f("contrast", "Contrast", 0.0, 100.0, 80.0), f("diffusion", "Diffusion", 0.0, 100.0, 25.0), f("tint", "Tint", 0.0, 100.0, 0.0), f("hue", "Correction hue", 0.0, 360.0, 35.0), f("vignette", "Vignette strength", 0.0, 1.0, 0.0)],
        &[DEFAULT, preset("Soft Moderate Contrast", &[("contrast", 66.0), ("diffusion", 15.0)]), preset("Heavy Contrast", &[("contrast", 100.0), ("diffusion", 5.0)]), preset("Warm Vignette", &[("tint", 30.0), ("vignette", 0.6)]), preset("R&B Soft Glow", &[("contrast", 70.0), ("diffusion", 60.0), ("tint", 25.0), ("hue", 30.0)])]),
    row("milan.fx.film_grain", "Film Grain", "Film", "Creative", "Animated film grain, strongest in the mid-tones", GpuPort::Mvfx,
        &[f("amount", "Amount", 0.0, 1.0, 0.4), f("granularity", "Granularity", 0.0, 1.0, 0.3), f("chroma", "Chromaticity", 0.0, 1.0, 0.0), f("seed", "Seed", 0.0, 1000.0, 0.0)],
        &[DEFAULT, preset("Subtle", &[("amount", 0.2), ("granularity", 0.15)]), preset("16mm", &[("amount", 0.55), ("granularity", 0.5), ("chroma", 0.3)]), preset("Super 8", &[("amount", 0.75), ("granularity", 0.8), ("chroma", 0.5)])]),
    row("milan.fx.add_noise", "Add Noise", "Film", "Creative", "Adds random noise", GpuPort::Mvfx,
        &[f("level", "Noise level", 0.0, 1.0, 0.2), flag("mono", "Monochromatic", true), flag("gaussian", "Gaussian noise", false), flag("animate", "Animate", true), f("seed", "Seed", 0.0, 1000.0, 0.0)],
        &[DEFAULT, preset("Grainy", &[("level", 0.35), ("gaussian", 1.0)]), preset("Color", &[("level", 0.3), ("mono", 0.0)]), preset("Extreme", &[("level", 0.8)])]),
    row("milan.fx.film_effects", "Film Effects", "Film", "Creative", "Old film: grain, tint, dust, hair, scratches, flicker and gate weave", GpuPort::Mvfx,
        &[
            f("grain", "Grain", 0.0, 1.0, 0.1),
            f("tint", "Tint", 0.0, 1.0, 0.25),
            color("color", "Color", [0.85, 0.7, 0.45, 1.0]),
            flag("grey", "Convert to grayscale", false),
            flag("particles", "Enable particles", true),
            choice("type", "Type", &["Dust", "Flicker", "Hair", "Jitter", "Scratches"], 0),
            f("amount", "Amount", 0.0, 1.0, 0.3),
            f("seed", "Seed", 0.0, 1000.0, 0.0),
        ],
        &[DEFAULT, preset("Very Old Film", &[("grain", 0.25), ("tint", 0.35), ("grey", 1.0), ("type", 4.0), ("amount", 0.5)]), preset("Circa 1980", &[("grain", 0.15), ("tint", 0.2), ("type", 1.0), ("amount", 0.25)]), preset("Gate Weave", &[("grain", 0.12), ("tint", 0.1), ("type", 3.0), ("amount", 0.4)]), preset("Super 8 Memory", &[("grain", 0.2), ("tint", 0.3), ("type", 0.0), ("amount", 0.45)])]),
    row("milan.fx.light_leak", "Light Leak", "Film", "Light", "Drifting blooms of warm light, as from a camera that lets light in", GpuPort::Mvfx,
        &[
            f("intensity", "Intensity", 0.0, 1.0, 0.6),
            f("size", "Size", 0.2, 2.0, 0.9),
            f("speed", "Speed", 0.0, 2.0, 0.5),
            color("color1", "Color 1", [1.0, 0.55, 0.15, 1.0]),
            color("color2", "Color 2", [0.95, 0.2, 0.35, 1.0]),
            choice("position", "Position", POSITIONS, 0),
            f("seed", "Seed", 0.0, 1000.0, 0.0),
        ],
        &[DEFAULT, preset("Warm Corner", &[("intensity", 0.75), ("position", 2.0)]), preset("Magenta Flare", &[("color1.r", 1.0), ("color1.g", 0.25), ("color1.b", 0.6), ("position", 1.0)]), preset("Slow Drift", &[("speed", 0.15), ("size", 1.4)])]),
    row("milan.fx.film_burn", "Film Burn", "Film", "Light", "A frame of film burning through from an edge", GpuPort::Mvfx,
        &[
            f("amount", "Amount", 0.0, 1.0, 0.45),
            color("color", "Color", [1.0, 0.45, 0.1, 1.0]),
            f("speed", "Speed", 0.0, 2.0, 0.6),
            f("scale", "Scale", 0.1, 2.0, 0.6),
            choice("position", "Position", POSITIONS, 0),
            f("seed", "Seed", 0.0, 1000.0, 0.0),
        ],
        &[DEFAULT, preset("Hot Edge", &[("amount", 0.3)]), preset("Burn Through", &[("amount", 0.8), ("position", 4.0)])]),
    row("milan.fx.rgb_split", "RGB Split", "Stylize", "Creative", "Shifts the red and blue channels apart (chromatic aberration)", GpuPort::Mvfx,
        &[f("amount", "Amount", 0.0, 0.1, 0.01), f("angle", "Angle", 0.0, 360.0, 0.0), choice("mode", "Mode", &["Linear", "Radial"], 0)],
        &[DEFAULT, preset("Heavy", &[("amount", 0.03)]), preset("Lens Fringe", &[("amount", 0.015), ("mode", 1.0)]), preset("Vertical", &[("amount", 0.012), ("angle", 90.0)])]),
    row("milan.fx.glitch", "Glitch", "Stylize", "Creative", "Digital break-up: displaced bands, broken blocks and split colour on random frames", GpuPort::Mvfx,
        &[f("amount", "Amount", 0.0, 1.0, 0.5), f("block", "Block size", 0.01, 0.25, 0.06), f("rate", "Rate", 0.0, 1.0, 0.5), f("shift", "RGB shift", 0.0, 1.0, 0.5), f("seed", "Seed", 0.0, 1000.0, 0.0)],
        &[DEFAULT, preset("Every Frame", &[("rate", 1.0)]), preset("Light", &[("amount", 0.25), ("rate", 0.3)]), preset("Heavy", &[("amount", 0.9), ("rate", 0.8), ("shift", 0.9)])]),
    row("milan.fx.vhs", "VHS", "Video", "Creative", "Camcorder tape: chroma bleed, scan lines, tracking noise, line jitter and a date stamp", GpuPort::Mvfx,
        &[
            f("bleed", "Chroma bleed", 0.0, 1.0, 0.5),
            f("scanlines", "Scan lines", 0.0, 1.0, 0.3),
            f("tracking", "Tracking noise", 0.0, 1.0, 0.3),
            f("jitter", "Line jitter", 0.0, 1.0, 0.2),
            f("color", "Color shift", 0.0, 1.0, 0.3),
            f("noise", "Noise", 0.0, 1.0, 0.2),
            flag("stamp", "Date stamp", false),
            text("stamp_text", "Stamp text", "PLAY"),
            f("seed", "Seed", 0.0, 1000.0, 0.0),
        ],
        &[
            DEFAULT,
            Preset { name: "Camcorder 2003", vals: &[("stamp", 1.0), ("tracking", 0.15)], text: &[("stamp_text", "JUN 14 2003  9:41 PM")] },
            preset("Worn Tape", &[("bleed", 0.8), ("tracking", 0.7), ("jitter", 0.5), ("noise", 0.4)]),
            preset("Clean Tape", &[("bleed", 0.3), ("tracking", 0.0), ("jitter", 0.05), ("noise", 0.08)]),
        ]),
    row("milan.fx.webcam", "Webcam", "Video", "Creative", "A late-2000s webcam: low resolution, blocky compression, colour cast and noise", GpuPort::Mvfx,
        &[f("resolution", "Resolution", 0.0, 1.0, 0.6), f("compression", "Compression", 0.0, 1.0, 0.5), f("noise", "Noise", 0.0, 1.0, 0.3), f("cast", "Color cast", 0.0, 1.0, 0.4), f("lowlight", "Low light", 0.0, 1.0, 0.3), f("seed", "Seed", 0.0, 1000.0, 0.0)],
        &[DEFAULT, preset("Webcam 2007", &[("resolution", 0.45), ("compression", 0.7), ("lowlight", 0.45)]), preset("Bedroom Night", &[("lowlight", 0.8), ("noise", 0.6)])]),
    row("milan.fx.night_vision", "Night Vision", "Video", "Creative", "Green image-intensifier look with noise, scan lines and a vignette", GpuPort::Mvfx,
        &[f("gain", "Gain", 0.0, 4.0, 2.0), f("noise", "Noise", 0.0, 1.0, 0.35), f("scanlines", "Scan lines", 0.0, 1.0, 0.3), f("vignette", "Vignette", 0.0, 1.0, 0.6), color("color", "Color", [0.3, 1.0, 0.35, 1.0]), f("seed", "Seed", 0.0, 1000.0, 0.0)],
        &[DEFAULT, preset("Night Vision Hustle", &[("gain", 2.6), ("noise", 0.5), ("vignette", 0.8)]), preset("Infrared", &[("color.r", 1.0), ("color.g", 0.35), ("color.b", 0.3)])]),
    row("milan.fx.letterbox", "Letterbox", "Utility", "Utility", "Widescreen bars for a 2.35, 1.85 or other aspect ratio", GpuPort::Mvfx,
        &[f("aspect", "Aspect ratio", 1.0, 3.0, 2.35), color("color", "Color", BLACK), f("offset", "Vertical offset", -1.0, 1.0, 0.0), f("feather", "Feather", 0.0, 0.05, 0.0)],
        &[DEFAULT, preset("2.39:1", &[("aspect", 2.39)]), preset("1.85:1", &[("aspect", 1.85)]), preset("4:3 Pillarbox", &[("aspect", 1.3333334)])]),
    row("milan.fx.tv_simulator", "TV Simulator", "Video", "Creative", "A CRT television: aperture grill, interlacing, sync faults, phosphor and static", GpuPort::Mvfx,
        &[
            f("detail", "Detail zoom", 0.0, 1.0, 0.0),
            f("aperture", "Aperture grill", 0.0, 1.0, 0.32),
            f("interlacing", "Interlacing", 0.0, 1.0, 0.7),
            f("line_sync", "Line sync", 0.0, 1.0, 0.0),
            f("vsync", "Vertical sync", 0.0, 1.0, 0.0),
            f("phasing", "Scan phasing", 0.0, 1.0, 0.15),
            f("phosphor", "Phosphorescence", 0.0, 1.0, 0.5),
            f("static", "Static", 0.0, 1.0, 0.0),
        ],
        &[DEFAULT, preset("TV Look", &[("interlacing", 0.66), ("phosphor", 0.9)]), preset("Bad Sync", &[("line_sync", 0.32), ("vsync", 0.25), ("phasing", 0.2), ("interlacing", 0.78)]), preset("Static", &[("static", 0.5)])]),
    row("milan.fx.newsprint", "Newsprint", "Stylize", "Creative", "A halftone print of dots", GpuPort::Mvfx,
        &[f("dot", "Dot size", 0.0, 1.0, 0.25), choice("type", "Type", &["Color", "Monochrome", "Offset color"], 0)],
        &[DEFAULT, preset("Monochrome", &[("type", 1.0)]), preset("Offset Color", &[("type", 2.0), ("dot", 0.4)]), preset("Large Dots", &[("dot", 0.8)])]),
    row("milan.fx.pixelate", "Pixelate", "Stylize", "Creative", "Blocks of solid colour", GpuPort::Mvfx,
        &[f("h", "Horizontal pixelization", 0.0, 1.0, 0.1), f("v", "Vertical pixelization", 0.0, 1.0, 0.1)],
        &[DEFAULT, preset("Small", &[("h", 0.05), ("v", 0.05)]), preset("Large", &[("h", 0.75), ("v", 0.75)]), preset("Medium Horizontal", &[("h", 0.3), ("v", 0.02)])]),
    row("milan.fx.starburst", "Starburst", "Light", "Light", "Star-shaped glints on the brightest highlights", GpuPort::Mvfx,
        &[f("threshold", "Threshold", 0.0, 100.0, 72.5), f("brilliance", "Brilliance", 0.0, 2000.0, 1000.0), f("strength", "Strength", 0.0, 1.0, 0.5), f("radius", "Radius", 0.0, 1.0, 0.25), f("points", "Points", 2.0, 16.0, 4.0), f("orientation", "Orientation", 0.0, 360.0, 45.0), flag("effect_only", "Effect only", false)],
        &[DEFAULT, preset("Simple 4 Point", &[("points", 4.0), ("orientation", 0.0)]), preset("Bling", &[("threshold", 65.0), ("brilliance", 1600.0), ("strength", 0.7), ("points", 6.0), ("radius", 0.2)]), preset("Bright Center", &[("threshold", 85.0), ("brilliance", 1800.0), ("radius", 0.4)])]),
    row("milan.fx.light_rays", "Light Rays", "Light", "Light", "Rays of light streaming from the bright parts towards a light source", GpuPort::Mvfx,
        &[f("x", "Light source X", 0.0, 1.0, 0.5), f("y", "Light source Y", 0.0, 1.0, 0.3), color("tint", "Tint color", WHITE), f("sensitivity", "Sensitivity", 0.0, 1.0, 0.5), f("strength", "Strength", 0.0, 1.0, 0.5), f("radius", "Radius", 0.0, 1.0, 0.5)],
        &[DEFAULT, preset("Golden Rays", &[("tint.b", 0.6), ("tint.g", 0.85), ("strength", 0.7)]), preset("Spotlight", &[("y", 0.0), ("radius", 0.8), ("sensitivity", 0.35)])]),
];

/// Change every frame without a keyframe.
pub const TEMPORAL: &[&str] = &["milan.fx.film_grain", "milan.fx.add_noise", "milan.fx.film_effects", "milan.fx.light_leak", "milan.fx.film_burn", "milan.fx.glitch", "milan.fx.vhs", "milan.fx.webcam", "milan.fx.night_vision", "milan.fx.tv_simulator"];
/// Blur on the way (parity 3 in 255).
#[cfg(test)]
pub const BLURS: &[&str] = &["milan.fx.glow", "milan.fx.soft_contrast", "milan.fx.starburst", "milan.fx.light_rays"];

pub static KERNELS: &[(&str, Kernel)] = &[
    ("gradient_map", k_gradient_map),
    ("bright", k_bright),
    ("add", k_add),
    ("soft_contrast", k_soft_contrast),
    ("film_grain", k_film_grain),
    ("add_noise", k_add_noise),
    ("film_effects", k_film_effects),
    ("light_leak", k_light_leak),
    ("film_burn", k_film_burn),
    ("rgb_split", k_rgb_split),
    ("glitch", k_glitch),
    ("vhs", k_vhs),
    ("webcam", k_webcam),
    ("night_vision", k_night_vision),
    ("letterbox", k_letterbox),
    ("tv_sim", k_tv_sim),
    ("newsprint", k_newsprint),
    ("pixelate", k_pixelate),
    ("starburst", k_starburst),
    ("light_rays", k_light_rays),
];

// ---- colour ops ----

fn sat_matrix(s: f32) -> [[f32; 3]; 3] {
    const L: [f32; 3] = [0.2126, 0.7152, 0.0722];
    std::array::from_fn(|i| std::array::from_fn(|j| (1.0 - s) * L[j] + if i == j { s } else { 0.0 }))
}

fn s_curve(x: f32, amt: f32) -> f32 {
    x + amt * (x * x * (3.0 - 2.0 * x) - x)
}

pub fn color_op_for(e: &FxEval) -> Option<ColorOp> {
    Some(match e.id {
        "milan.fx.bleach_bypass" => {
            let (c, crush) = (e.f("contrast"), e.f("crush"));
            let op = ColorOp::tone(|_, x| s_curve(((x - crush) / (1.0 - crush)).clamp(0.0, 1.0), 1.4 * c));
            ColorOp { m: Some(sat_matrix(e.f("saturation"))), mix: e.f("amount"), ..op }
        }
        "milan.fx.cross_process" => {
            let s = e.f("strength");
            let op = ColorOp::tone(|ch, x| match ch {
                0 => s_curve(x, 0.9 * s),
                1 => s_curve(x, 0.45 * s) + 0.03 * s * (1.0 - x),
                _ => 0.14 * s + x * (1.0 - 0.36 * s),
            });
            ColorOp { m: Some(sat_matrix(e.f("saturation"))), mix: e.f("amount"), ..op }
        }
        "milan.fx.posterize" => {
            let n = e.f("levels").round().max(2.0) - 1.0;
            ColorOp::tone(|_, x| (x * n + 0.5).floor() / n)
        }
        "milan.fx.threshold" => {
            const L: [f32; 3] = [0.2126, 0.7152, 0.0722];
            let t = e.f("threshold");
            ColorOp { m: Some([L; 3]), ..ColorOp::tone(|_, x| (x >= t) as u8 as f32) }
        }
        _ => return None,
    })
}

// ---- recipes ----

/// The seed of frame `n` (a seed parameter and the frame number), as the kernels' `j.x`.
pub(crate) fn frame_seed(e: &FxEval, salt: u32) -> i32 {
    let n = super::frame_no(e) as u32;
    hash((e.f("seed").round() as u32).wrapping_mul(0x2545_f491) ^ n.wrapping_mul(0x9e37_79b9) ^ salt) as i32
}

fn m0() -> M {
    M::default()
}

fn one(entry: &'static str, m: M, data: Vec<u32>) -> Vec<Step> {
    vec![k(entry, m, data, Src::In(0), Src::In(0))]
}

/// A bright pass of `e`'s picture: threshold `t` (0..1 luma), `supp`, tint.
fn bright(t: f32, supp: f32, tint: [f32; 4]) -> Step {
    let mut m = m0();
    m.f[0] = [t, 1.0 / (1.0 - t).max(0.001), supp, 0.0];
    m.f[1] = tint;
    k("bright", m, vec![], Src::In(0), Src::In(0))
}

pub fn recipe(e: &FxEval, w: usize, h: usize) -> Option<Vec<Step>> {
    let (wf, hf) = (w as f32, h as f32);
    let mut m = m0();
    Some(match e.id {
        "milan.fx.gradient_map" => {
            m.f[0][0] = e.f("amount");
            m.i[0] = e.i("mode") as i32;
            let lut = crate::engine::generate::gradient_lut(e);
            one("gradient_map", m, lut.iter().map(|c| u32::from_le_bytes(c.to_array())).collect())
        }
        "milan.fx.glow" => {
            let gp = e.f("glow_percent");
            let r = ((0.004 + 0.03 * gp) * hf).round().max(1.0) as usize;
            m.f[0][0] = e.f("intensity");
            vec![bright(1.0 - gp, e.f("suppression"), e.c("color")), Step::Blur { of: Src::Step(0), rx: r, ry: r }, k("add", m, vec![], Src::In(0), Src::Step(1))]
        }
        "milan.fx.soft_contrast" => {
            let d = e.f("diffusion") / 100.0;
            let r = (d * 0.03 * hf).round().max(1.0) as usize;
            let (s, c) = (e.f("hue").to_radians().sin(), e.f("hue").to_radians().cos());
            // The correction hue as a colour direction: warm at 30-40 degrees.
            let tint = e.f("tint") / 100.0 * 0.12;
            m.f[0] = [d, 0.5 + e.f("contrast") / 200.0, e.f("vignette"), 0.0];
            m.f[1] = [tint * c, tint * (0.5 * s - 0.25 * c), -tint * (0.5 * c + 0.5 * s), 0.0];
            m.f[2] = [wf / 2.0, hf / 2.0, 2.0 / wf, 2.0 / hf];
            vec![Step::Blur { of: Src::In(0), rx: r, ry: r }, k("soft_contrast", m, vec![], Src::In(0), Src::Step(0))]
        }
        "milan.fx.film_grain" => {
            m.f[0] = [e.f("amount"), (1.0 + 3.0 * e.f("granularity")) * (hf / 1080.0).max(0.25), e.f("chroma"), 0.0];
            m.j[0] = frame_seed(e, 1);
            one("film_grain", m, vec![])
        }
        "milan.fx.add_noise" => {
            m.f[0][0] = e.f("level");
            m.i = [e.on("mono") as i32, e.on("gaussian") as i32, 0, 0];
            m.j[0] = if e.on("animate") { frame_seed(e, 2) } else { hash(e.f("seed").round() as u32 ^ 2) as i32 };
            one("add_noise", m, vec![])
        }
        "milan.fx.film_effects" => film_effects(e, wf, hf),
        "milan.fx.light_leak" => {
            light_leak_m(&mut m, e, wf, hf, e.f(super::T), e.f("intensity"));
            one("light_leak", m, vec![])
        }
        "milan.fx.film_burn" => {
            film_burn_m(&mut m, e, wf, hf, e.f(super::T), e.f("amount"));
            one("film_burn", m, vec![])
        }
        "milan.fx.rgb_split" => {
            let (s, c) = e.f("angle").to_radians().sin_cos();
            let d = e.f("amount") * hf;
            m.i = [e.i("mode") as i32, (c * d).round() as i32, (s * d).round() as i32, 0];
            m.f[0] = [wf / 2.0, hf / 2.0, 1.0 + e.f("amount"), 1.0 - e.f("amount")];
            one("rgb_split", m, vec![])
        }
        "milan.fx.glitch" => {
            let base = frame_seed(e, 3);
            let active = rnd(hash(base as u32)) < e.f("rate");
            m.i = [active as i32, ((e.f("block") * hf).round() as i32).max(1), w as i32, (e.f("shift") * 0.02 * wf).round() as i32];
            m.j[0] = base;
            m.f[0] = [e.f("amount"), 0.3 * wf, 0.0, 0.0];
            one("glitch", m, vec![])
        }
        "milan.fx.vhs" => vhs(e, wf, hf),
        "milan.fx.webcam" => {
            let b = (1.0 + (1.0 - e.f("resolution")) * 0.012 * hf).round().max(1.0);
            m.i = [b as i32, (b as i32) * 8, 0, 0];
            m.j[0] = frame_seed(e, 5);
            m.f[0] = [8.0 + (1.0 - e.f("compression")) * 56.0, e.f("noise"), e.f("cast"), e.f("lowlight")];
            one("webcam", m, vec![])
        }
        "milan.fx.night_vision" => {
            m.j[0] = frame_seed(e, 6);
            m.i[0] = ((hf / 288.0).round() as i32).max(1);
            m.f[0] = [e.f("gain"), e.f("noise"), e.f("scanlines"), e.f("vignette")];
            m.f[1] = e.c("color");
            m.f[2] = [wf / 2.0, hf / 2.0, 2.0 / wf, 2.0 / hf];
            one("night_vision", m, vec![])
        }
        "milan.fx.letterbox" => {
            let (asp, pa) = (e.f("aspect"), wf / hf);
            // Bars across (wider than the picture) or down the sides (narrower): the open part's start and end.
            let (across, open) = if asp >= pa { (1, wf / asp) } else { (0, hf * asp) };
            let full = if across == 1 { hf } else { wf };
            let start = (full - open) / 2.0 * (1.0 + e.f("offset"));
            m.i[0] = across;
            m.f[0] = [start, start + open, e.f("feather") * hf, 0.0];
            m.f[1] = e.c("color");
            one("letterbox", m, vec![])
        }
        "milan.fx.tv_simulator" => {
            let t = e.f(super::T);
            let roll = if e.f("vsync") > 0.0 { ((t * e.f("vsync") * 1.3).fract() * hf).floor() } else { 0.0 };
            m.i = [(1.0 + e.f("detail") * 3.0).round() as i32 * ((hf / 540.0).round() as i32).max(1), roll as i32, (super::frame_no(e) & 1) as i32, (e.f("phasing") * 0.006 * wf).round() as i32];
            m.j[0] = frame_seed(e, 7);
            m.f[0] = [e.f("aperture"), e.f("interlacing"), e.f("line_sync") * 0.04 * wf, e.f("phosphor")];
            m.f[1] = [e.f("static"), (0.04 * hf).max(1.0), hf, 0.0];
            one("tv_sim", m, vec![])
        }
        "milan.fx.newsprint" => {
            let s = (2.0 + e.f("dot") * 0.04 * hf).max(2.0);
            let ty = e.i("type");
            let angles: [f32; 3] = if ty == 1 { [45.0; 3] } else { [15.0, 75.0, 0.0] };
            m.i[0] = ty as i32;
            m.f[0] = [s, if ty == 2 { 0.3 * s } else { 0.0 }, 0.70710677 * s, 0.0];
            for (i, a) in angles.iter().enumerate() {
                let (sn, cs) = a.to_radians().sin_cos();
                m.f[1 + i] = [cs, sn, 0.0, 0.0];
            }
            one("newsprint", m, vec![])
        }
        "milan.fx.pixelate" => {
            let b = |v: f32| ((v * 0.1 * hf).round() as i32).max(1);
            m.i = [b(e.f("h")), b(e.f("v")), 0, 0];
            one("pixelate", m, vec![])
        }
        "milan.fx.starburst" => {
            let pts = e.f("points").round().clamp(2.0, 16.0) as usize;
            const K: usize = 16;
            let len = e.f("radius") * 0.5 * hf;
            let mut d = Vec::with_capacity(pts * K * 3);
            for j in 0..pts {
                let a = (e.f("orientation") + j as f32 * 360.0 / pts as f32).to_radians();
                for s in 1..=K {
                    let r = len * s as f32 / K as f32;
                    let wgt = (1.0 - s as f32 / (K + 1) as f32).powi(2);
                    d.extend([(a.cos() * r).round() as i32 as u32, (a.sin() * r).round() as i32 as u32, wgt.to_bits()]);
                }
            }
            m.i = [(pts * K) as i32, e.on("effect_only") as i32, 0, 0];
            m.f[0][0] = e.f("brilliance") / 1000.0 * e.f("strength") * 4.0 / (pts * K) as f32;
            vec![bright(e.f("threshold") / 100.0, 0.5, WHITE), k("starburst", m, d, Src::In(0), Src::Step(0))]
        }
        "milan.fx.light_rays" => {
            const N: i32 = 24;
            m.i[0] = N;
            m.f[0] = [e.f("x") * wf, e.f("y") * hf, e.f("radius") / N as f32, e.f("strength") * 3.0 / N as f32];
            vec![bright(1.0 - e.f("sensitivity"), 0.0, e.c("tint")), k("light_rays", m, vec![], Src::In(0), Src::Step(0))]
        }
        _ => return None,
    })
}

fn film_effects(e: &FxEval, wf: f32, hf: f32) -> Vec<Step> {
    let mut m = m0();
    let base = frame_seed(e, 4);
    let ty = if e.on("particles") { e.i("type") as i32 + 1 } else { 0 };
    let amt = e.f("amount");
    let r = |salt: u32| rnd(hash3(base as u32, salt, 0x51ed));
    // Flicker: the frame's gain; Jitter (gate weave): its offset, now and then a bigger jump.
    let gain = if ty == 2 { 1.0 + (r(1) - 0.5) * amt * 0.5 } else { 1.0 };
    let (dx, dy) = if ty == 4 {
        let jump = if r(2) < amt * 0.1 { 0.05 * hf } else { 0.0 };
        (((r(3) - 0.5) * amt * 0.006 * hf).round() as i32, ((r(4) - 0.5) * amt * 0.02 * hf + jump).round() as i32)
    } else {
        (0, 0)
    };
    // Particles, 8 floats each: Dust (x, y, r^2, tone), Hair (x, y, R, half width, dir x, dir y, cos, tone),
    // Scratches (x, half width, top, bottom, tone).
    let mut d: Vec<f32> = Vec::new();
    let seed = e.f("seed").round() as u32;
    let n = super::frame_no(e) as u32;
    let count = match ty {
        1 => (amt * 30.0).round() as usize,
        3 => 1 + (amt * 4.0).round() as usize,
        5 => 1 + (amt * 5.0).round() as usize,
        _ => 0,
    };
    for i in 0..count as u32 {
        let q = |s: u32| rnd(hash3(base as u32, i * 16 + s, 0xd057));
        let tone = if q(0) < 0.75 { 0.1 + 0.3 * q(1) } else { 0.9 };
        match ty {
            1 => {
                let rad = (0.0015 + 0.0045 * q(2)) * hf;
                d.extend([q(3) * wf, q(4) * hf, rad * rad, tone, 0.0, 0.0, 0.0, 0.0]);
            }
            3 => {
                let a = q(5) * pm::TAU;
                d.extend([q(3) * wf, q(4) * hf, (0.02 + 0.06 * q(2)) * hf, (0.0008 * hf).max(0.5), a.cos(), a.sin(), 0.2 + 0.5 * q(6), 0.12]);
            }
            _ => {
                // A scratch keeps its place over several frames and drifts.
                let life = 12;
                let slot = (n + i * 5) / life;
                let qs = |s: u32| rnd(hash3(seed ^ 0x5c4a, i * 31 + s, slot));
                let x = qs(1) * wf + ((n + i * 5) % life) as f32 * (qs(2) - 0.5) * 0.002 * wf;
                let top = qs(3) * hf * 0.5;
                d.extend([x, (0.0006 * hf).max(0.5), top, top + (0.4 + 0.6 * qs(4)) * hf, if qs(5) < 0.5 { 0.85 } else { 0.15 }, 0.0, 0.0, 0.0]);
            }
        }
    }
    m.i = [ty, count as i32, dx, dy];
    m.j = [base, e.on("grey") as i32, 0, 0];
    m.f[0] = [e.f("grain"), e.f("tint"), gain, 0.0];
    m.f[1] = e.c("color");
    one("film_effects", m, pm::fwords(&d))
}

/// Light Leak's blobs at time `t` (also the transition's, `strength` its intensity).
pub(crate) fn light_leak_m(m: &mut M, e: &FxEval, wf: f32, hf: f32, t: f32, strength: f32) {
    let seed = e.f("seed").round() as u32;
    let r = |s: u32| rnd(hash3(seed, s, 0x1ea4));
    let sp = e.f("speed");
    let (ax, ay) = match e.i("position") {
        0 => (0.0, 0.5),
        1 => (1.0, 0.5),
        2 => (0.5, 0.0),
        3 => (0.5, 1.0),
        _ => (0.5, 0.5),
    };
    let (c1, c2) = (e.c("color1"), e.c("color2"));
    for i in 0..3 {
        let ph = r(i * 4) * pm::TAU;
        let fr = 0.4 + 0.6 * r(i * 4 + 1);
        let a = t * sp * fr + ph;
        let cx = (ax + 0.35 * (a.cos() * 0.6 + (r(i * 4 + 2) - 0.5))) * wf;
        let cy = (ay + 0.35 * ((a * 1.3).sin() * 0.6 + (r(i * 4 + 3) - 0.5))) * hf;
        let rad = e.f("size") * hf * (0.45 + 0.25 * i as f32);
        let s = 0.65 + 0.35 * (a * 0.7 + i as f32).sin();
        m.f[i as usize] = [cx, cy, 1.0 / (rad * rad), s];
        let c = [c1, c2, [(c1[0] + c2[0]) / 2.0, (c1[1] + c2[1]) / 2.0, (c1[2] + c2[2]) / 2.0, 1.0]][i as usize];
        m.f[3 + i as usize] = [c[0], c[1], c[2], 0.0];
    }
    m.f[6][0] = strength;
}

/// Film Burn's field at time `t` (also the transition's, `amount` how far it has burnt).
pub(crate) fn film_burn_m(m: &mut M, e: &FxEval, wf: f32, hf: f32, t: f32, amount: f32) {
    let seed = e.f("seed").round() as u32;
    let z = t * e.f("speed") * 2.0;
    let layer = z.floor();
    m.j = [hash3(seed, layer as i32 as u32, 0xb0) as i32, hash3(seed, layer as i32 as u32 + 1, 0xb0) as i32, 0, 0];
    m.i[0] = e.i("position") as i32;
    let cell = (e.f("scale") * hf * 0.5).max(2.0);
    m.f[0] = [amount, 1.0 / cell, z - layer, 0.0];
    m.f[1] = e.c("color");
    m.f[2] = [1.0 / wf, 1.0 / hf, 0.0, 0.0];
}

fn vhs(e: &FxEval, wf: f32, hf: f32) -> Vec<Step> {
    let mut m = m0();
    let base = frame_seed(e, 8);
    let t = e.f(super::T);
    let seed = e.f("seed").round() as u32;
    let tr = e.f("tracking");
    let yb = ((t * 0.13 + rnd(hash(seed ^ 0x7a)) as f32).fract()) * hf * 1.2 - 0.1 * hf;
    m.f[0] = [((e.f("bleed") * 0.004 * wf).round()).max(1.0), (e.f("bleed") * 0.006 * wf).round(), e.f("scanlines"), e.f("color")];
    m.f[1] = [tr, yb, tr * 0.08 * hf, e.f("noise")];
    m.j[0] = base;
    let s = ((hf / 110.0).round() as i32).max(1);
    let mut data = Vec::new();
    let mut cols = 0;
    if e.on("stamp") {
        let txt: Vec<char> = e.t("stamp_text").to_uppercase().chars().take(28).collect();
        cols = (txt.len() * 6) as i32;
        data = vec![0u32; cols as usize * 7];
        for (i, ch) in txt.iter().enumerate() {
            let g = glyph(*ch);
            for (row, bits) in g.iter().enumerate() {
                for c in 0..5 {
                    if bits >> (4 - c) & 1 == 1 {
                        data[row * cols as usize + i * 6 + c] = 1;
                    }
                }
            }
        }
    }
    m.f[2] = [e.f("jitter") * 0.006 * wf, tr * 0.05 * wf, (0.05 * hf).round(), (hf - 0.07 * hf - 7.0 * s as f32).round()];
    m.i = [cols, s, ((hf / 288.0).round() as i32).max(1), 0];
    one("vhs", m, data)
}

/// A 5 x 7 glyph (rows, the leftmost column the high bit); unknown characters are blank.
fn glyph(c: char) -> [u8; 7] {
    const DIGITS: [[u8; 7]; 10] = [
        [14, 17, 19, 21, 25, 17, 14],
        [4, 12, 4, 4, 4, 4, 14],
        [14, 17, 1, 2, 4, 8, 31],
        [31, 2, 4, 2, 1, 17, 14],
        [2, 6, 10, 18, 31, 2, 2],
        [31, 16, 30, 1, 1, 17, 14],
        [6, 8, 16, 30, 17, 17, 14],
        [31, 1, 2, 4, 8, 8, 8],
        [14, 17, 17, 14, 17, 17, 14],
        [14, 17, 17, 15, 1, 2, 12],
    ];
    const LETTERS: [[u8; 7]; 26] = [
        [14, 17, 17, 31, 17, 17, 17],
        [30, 17, 17, 30, 17, 17, 30],
        [14, 17, 16, 16, 16, 17, 14],
        [28, 18, 17, 17, 17, 18, 28],
        [31, 16, 16, 30, 16, 16, 31],
        [31, 16, 16, 30, 16, 16, 16],
        [14, 17, 16, 23, 17, 17, 15],
        [17, 17, 17, 31, 17, 17, 17],
        [14, 4, 4, 4, 4, 4, 14],
        [7, 2, 2, 2, 2, 18, 12],
        [17, 18, 20, 24, 20, 18, 17],
        [16, 16, 16, 16, 16, 16, 31],
        [17, 27, 21, 21, 17, 17, 17],
        [17, 17, 25, 21, 19, 17, 17],
        [14, 17, 17, 17, 17, 17, 14],
        [30, 17, 17, 30, 16, 16, 16],
        [14, 17, 17, 17, 21, 18, 13],
        [30, 17, 17, 30, 20, 18, 17],
        [15, 16, 16, 14, 1, 1, 30],
        [31, 4, 4, 4, 4, 4, 4],
        [17, 17, 17, 17, 17, 17, 14],
        [17, 17, 17, 17, 17, 10, 4],
        [17, 17, 17, 21, 21, 21, 10],
        [17, 17, 10, 4, 10, 17, 17],
        [17, 17, 17, 10, 4, 4, 4],
        [31, 1, 2, 4, 8, 16, 31],
    ];
    match c {
        '0'..='9' => DIGITS[c as usize - '0' as usize],
        'A'..='Z' => LETTERS[c as usize - 'A' as usize],
        ':' => [0, 12, 12, 0, 12, 12, 0],
        '.' => [0, 0, 0, 0, 0, 12, 12],
        '-' => [0, 0, 0, 31, 0, 0, 0],
        '/' => [0, 1, 2, 4, 8, 16, 0],
        '>' => [16, 24, 28, 30, 28, 24, 16],
        _ => [0; 7],
    }
}

// ---- kernels (twins in gpu/mvfx_looks.wgsl) ----

fn out(c: [f32; 3], a: f32) -> Color32 {
    pack(premul(c, a))
}

fn k_gradient_map(m: &M, d: &[u32], x: i32, y: i32, a: Img, _: Img) -> Color32 {
    let p = a.at(x, y);
    let c = straight(p);
    let g = d[q8(luma(c) * 255.0) as usize];
    let gc = [(g & 255) as f32 / 255.0, (g >> 8 & 255) as f32 / 255.0, (g >> 16 & 255) as f32 / 255.0];
    let amt = m.f[0][0];
    let o: [f32; 3] = std::array::from_fn(|i| if m.i[0] == 1 { c[i] + gc[i] * amt } else { mix(c[i], gc[i], amt) });
    out(o, p[3])
}

fn k_bright(m: &M, _: &[u32], x: i32, y: i32, a: Img, _: Img) -> Color32 {
    let p = a.at(x, y);
    let l = luma(straight(p));
    let mut v = ((l - m.f[0][0]) * m.f[0][1]).clamp(0.0, 1.0);
    let s = m.f[0][2];
    v *= 1.0 - s + s * v;
    let t = m.f[1];
    pack([p[0] * v * t[0], p[1] * v * t[1], p[2] * v * t[2], p[3] * v])
}

fn k_add(m: &M, _: &[u32], x: i32, y: i32, a: Img, b: Img) -> Color32 {
    let (p, q) = (a.at(x, y), b.at(x, y));
    let i = m.f[0][0];
    pack(std::array::from_fn(|c| p[c] + q[c] * i))
}

fn k_soft_contrast(m: &M, _: &[u32], x: i32, y: i32, a: Img, b: Img) -> Color32 {
    let p = a.at(x, y);
    let (c, bl) = (straight(p), straight(b.at(x, y)));
    let [d, kc, vig, _] = m.f[0];
    let dx = (x as f32 + 0.5 - m.f[2][0]) * m.f[2][2];
    let dy = (y as f32 + 0.5 - m.f[2][1]) * m.f[2][3];
    let v = 1.0 - vig * pm::sstep(0.35, 1.6, dx * dx + dy * dy);
    let o: [f32; 3] = std::array::from_fn(|i| {
        let s = 1.0 - (1.0 - c[i]) * (1.0 - bl[i] * d);
        let k = 0.5 + (s - 0.5) * kc + m.f[1][i];
        k * v
    });
    out(o, p[3])
}

/// Value noise: a hash per lattice point, smoothly interpolated.
fn vnoise(base: u32, fx: f32, fy: f32, ch: u32) -> f32 {
    let (x0, y0) = (fx.floor(), fy.floor());
    let (tx, ty) = (fx - x0, fy - y0);
    let (sx, sy) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
    let (ix, iy) = (pm::ifloor(x0) as u32, pm::ifloor(y0) as u32);
    let hv = |dx: u32, dy: u32| rnd(hash3(base, ix.wrapping_add(dx), iy.wrapping_add(dy).wrapping_mul(4).wrapping_add(ch)));
    let top = mix(hv(0, 0), hv(1, 0), sx);
    let bot = mix(hv(0, 1), hv(1, 1), sx);
    mix(top, bot, sy)
}

fn k_film_grain(m: &M, _: &[u32], x: i32, y: i32, a: Img, _: Img) -> Color32 {
    let p = a.at(x, y);
    let c = straight(p);
    let [amt, cell, chroma, _] = m.f[0];
    let base = m.j[0] as u32;
    let (fx, fy) = ((x as f32 + 0.5) / cell, (y as f32 + 0.5) / cell);
    let l = luma(c);
    let wgt = 0.25 + 3.0 * l * (1.0 - l);
    let mono = vnoise(base, fx, fy, 0) - 0.5;
    let o: [f32; 3] = std::array::from_fn(|i| {
        let n = mix(mono, vnoise(base, fx, fy, 1 + i as u32) - 0.5, chroma);
        c[i] + n * amt * 0.7 * wgt
    });
    out(o, p[3])
}

fn k_add_noise(m: &M, _: &[u32], x: i32, y: i32, a: Img, _: Img) -> Color32 {
    let p = a.at(x, y);
    let c = straight(p);
    let base = m.j[0] as u32;
    let n = |ch: u32| {
        let h = |s: u32| rnd(hash3(base, x as u32, (y as u32).wrapping_mul(16).wrapping_add(ch * 4 + s)));
        if m.i[1] == 1 { (h(0) + h(1) + h(2) + h(3) - 2.0) * 0.8660254 } else { (h(0) - 0.5) * 2.0 }
    };
    let mono = n(0);
    let o: [f32; 3] = std::array::from_fn(|i| c[i] + if m.i[0] == 1 { mono } else { n(1 + i as u32) } * m.f[0][0] * 0.5);
    out(o, p[3])
}

fn k_film_effects(m: &M, d: &[u32], x: i32, y: i32, a: Img, _: Img) -> Color32 {
    let p = a.at(x - m.i[2], y - m.i[3]);
    let mut c = straight(p);
    let l = luma(c);
    if m.j[1] == 1 {
        c = [l; 3];
    }
    let [grain, tint, gain, _] = m.f[0];
    let mut o: [f32; 3] = std::array::from_fn(|i| mix(c[i], l * m.f[1][i], tint) * gain);
    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
    for i in 0..m.i[1] as usize {
        let q = |j: usize| fw(d, i * 8 + j);
        let hit = match m.i[0] {
            1 => {
                let (dx, dy) = (px - q(0), py - q(1));
                (dx * dx + dy * dy < q(2)).then(|| (q(3), 0.85))
            }
            3 => {
                let (dx, dy) = (px - q(0), py - q(1));
                let r = (dx * dx + dy * dy).sqrt();
                let on = (r - q(2)).abs() < q(3) && r > 0.0 && (dx * q(4) + dy * q(5)) / r > q(6);
                on.then(|| (q(7), 0.8))
            }
            5 => ((px - q(0)).abs() < q(1) && py >= q(2) && py < q(3)).then(|| (q(4), 0.6)),
            _ => None,
        };
        if let Some((tone, s)) = hit {
            o = o.map(|v| mix(v, tone, s));
        }
    }
    let g = (rnd(hash3(m.j[0] as u32, x as u32, y as u32)) - 0.5) * grain * 0.4;
    out(o.map(|v| v + g), p[3])
}

fn k_light_leak(m: &M, _: &[u32], x: i32, y: i32, a: Img, _: Img) -> Color32 {
    let p = a.at(x, y);
    let c = straight(p);
    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
    let mut leak = [0.0f32; 3];
    for i in 0..3 {
        let b = m.f[i];
        let (dx, dy) = (px - b[0], py - b[1]);
        let q = (dx * dx + dy * dy) * b[2];
        let fall = if q < 1.0 { (1.0 - q) * (1.0 - q) } else { 0.0 };
        let s = fall * b[3];
        for (ch, l) in leak.iter_mut().enumerate() {
            *l += m.f[3 + i][ch] * s;
        }
    }
    let k = m.f[6][0];
    let o: [f32; 3] = std::array::from_fn(|i| 1.0 - (1.0 - c[i]) * (1.0 - (leak[i] * k).min(1.0)));
    out(o, p[3])
}

/// Film Burn's field at (x, y): above 0 is burnt (the core above 0.25), just below 0 scorched.
pub(crate) fn burn_level(m: &M, x: i32, y: i32) -> f32 {
    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
    let (u, v) = (px * m.f[2][0], py * m.f[2][1]);
    let g = match m.i[0] {
        0 => u,
        1 => 1.0 - u,
        2 => v,
        3 => 1.0 - v,
        _ => {
            let (du, dv) = (u - 0.5, v - 0.5);
            (du * du + dv * dv).sqrt() * 1.6
        }
    };
    let inv = m.f[0][1];
    let mut n0 = 0.0;
    let mut n1 = 0.0;
    let mut sc = inv;
    let mut wt = 0.5;
    for o in 0..3u32 {
        n0 += vnoise(m.j[0] as u32, px * sc, py * sc, o) * wt;
        n1 += vnoise(m.j[1] as u32, px * sc, py * sc, o) * wt;
        sc *= 2.0;
        wt *= 0.5;
    }
    let n = mix(n0, n1, m.f[0][2]);
    m.f[0][0] * 1.4 - g + (n - 0.44) * 0.7
}

/// A burn of `c` (straight) at level `lv` in colour `col`: (colour, alpha cover 0..1).
pub(crate) fn burn(c: [f32; 3], lv: f32, col: [f32; 4]) -> ([f32; 3], f32) {
    if lv > 0.25 {
        ([1.0, 0.97, 0.88], 1.0)
    } else if lv > 0.0 {
        let t = lv * 4.0;
        (std::array::from_fn(|i| mix(col[i], [1.0, 0.97, 0.88][i], t * t)), 1.0)
    } else if lv > -0.12 {
        let t = (lv + 0.12) * 8.333333;
        (std::array::from_fn(|i| mix(c[i], c[i] * 0.25 + col[i] * 0.3, t)), 0.0)
    } else {
        (c, 0.0)
    }
}

fn k_film_burn(m: &M, _: &[u32], x: i32, y: i32, a: Img, _: Img) -> Color32 {
    let p = a.at(x, y);
    let (o, cover) = burn(straight(p), burn_level(m, x, y), m.f[1]);
    let al = if cover > 0.0 { 255.0 } else { p[3] };
    out(o, al)
}

fn k_rgb_split(m: &M, _: &[u32], x: i32, y: i32, a: Img, _: Img) -> Color32 {
    let (r, g, b) = if m.i[0] == 0 {
        (a.at(x - m.i[1], y - m.i[2]), a.at(x, y), a.at(x + m.i[1], y + m.i[2]))
    } else {
        let [cx, cy, sr, sb] = m.f[0];
        let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
        (a.bil(cx + dx / sr, cy + dy / sr, true), a.at(x, y), a.bil(cx + dx / sb, cy + dy / sb, true))
    };
    pack([r[0], g[1], b[2], r[3].max(g[3]).max(b[3])])
}

fn k_glitch(m: &M, _: &[u32], x: i32, y: i32, a: Img, _: Img) -> Color32 {
    if m.i[0] == 0 {
        return pack(a.at(x, y));
    }
    let base = m.j[0] as u32;
    let bh = m.i[1];
    let band = (y / bh) as u32;
    let hb = hash3(base, band, 7);
    let amt = m.f[0][0];
    let dx = if rnd(hb) < amt * 0.6 { pm::ifloor((rnd(hash(hb)) - 0.5) * amt * m.f[0][1] + 0.5) } else { 0 };
    let sx = x + dx;
    let s = m.i[3];
    let mut c = [a.at(sx + s, y)[0], a.at(sx, y)[1], a.at(sx - s, y)[2], a.at(sx, y)[3]];
    let hc = hash3(base, band, (sx.max(0) / (bh * 2)) as u32 + 101);
    if rnd(hc) < amt * 0.15 {
        match (hc >> 4) % 3 {
            0 => c = [c[2], c[0], c[1], c[3]],
            1 => c = [c[3] - c[0], c[3] - c[1], c[3] - c[2], c[3]],
            _ => c = c.map(|v| (v * 0.015625).floor() * 64.0),
        }
    }
    pack(c)
}

fn k_vhs(m: &M, d: &[u32], x: i32, y: i32, a: Img, _: Img) -> Color32 {
    let base = m.j[0] as u32;
    let [blur, cs, scan, colr] = m.f[0];
    let [_, yb, band, noise] = m.f[1];
    let mut jx = pm::ifloor((rnd(hash3(base, y as u32, 1)) - 0.5) * m.f[2][0] + 0.5);
    let inb = (y as f32 + 0.5 - yb).abs() < band;
    if inb {
        jx += pm::ifloor((rnd(hash3(base, y as u32, 2)) - 0.5) * m.f[2][1] + 0.5);
    }
    let sx = x + jx;
    let p = a.at(sx, y);
    let l = luma(straight(p));
    let (bi, ci) = (blur as i32, cs as i32);
    let mut sum = [0.0f32; 4];
    for j in 0..4 {
        let q = a.at(sx - ci - j * bi, y);
        for (s, v) in sum.iter_mut().zip(q) {
            *s += v;
        }
    }
    let ch = straight(sum.map(|v| v * 0.25));
    let cl = luma(ch);
    let (r, b) = (l + (ch[0] - cl), l + (ch[2] - cl));
    let g = (l - 0.2126 * r - 0.0722 * b) / 0.7152;
    let sat = 1.0 - colr * 0.35;
    let mut o = [mix(l, r, sat) + colr * 0.04, mix(l, g, sat) - colr * 0.02, mix(l, b, sat) + colr * 0.03];
    let nz = (rnd(hash3(base, x as u32, y as u32 ^ 0x9000_0000)) - 0.5) * noise * 0.25 * if inb { 4.0 } else { 1.0 };
    o = o.map(|v| v + nz);
    if (y / m.i[2]) & 1 == 1 {
        o = o.map(|v| v * (1.0 - scan * 0.35));
    }
    let cols = m.i[0];
    if cols > 0 {
        let (x0, y0, s) = (m.f[2][2] as i32, m.f[2][3] as i32, m.i[1]);
        if x >= x0 && y >= y0 {
            let (mx, my) = ((x - x0) / s, (y - y0) / s);
            if mx < cols && my < 7 && d[(my * cols + mx) as usize] == 1 {
                o = [mix(o[0], 1.0, 0.9), mix(o[1], 1.0, 0.9), mix(o[2], 0.85, 0.9)];
            }
        }
    }
    out(o, p[3])
}

fn k_webcam(m: &M, _: &[u32], x: i32, y: i32, a: Img, _: Img) -> Color32 {
    let (b, mb) = (m.i[0], m.i[1]);
    let p = a.at(x / b * b + b / 2, y / b * b + b / 2);
    let c = straight(p);
    let q = straight(a.at(x / mb * mb + mb / 2, y / mb * mb + mb / 2));
    let [levels, noise, cast, low] = m.f[0];
    let l = (luma(c) * levels + 0.5).floor() / levels;
    let ql = luma(q);
    let mut o = [l + (q[0] - ql), l + (q[1] - ql), l + (q[2] - ql)];
    o = [o[0] * (1.0 - cast * 0.12), o[1] * (1.0 + cast * 0.04), o[2] * (1.0 + cast * 0.14)];
    let dark = 1.0 - low * 0.45;
    let nz = (rnd(hash3(m.j[0] as u32, (x / b) as u32, (y / b) as u32)) - 0.5) * (noise * 0.2 + low * 0.15);
    out(o.map(|v| v * dark + nz), p[3])
}

fn k_night_vision(m: &M, _: &[u32], x: i32, y: i32, a: Img, _: Img) -> Color32 {
    let p = a.at(x, y);
    let [gain, noise, scan, vig] = m.f[0];
    let mut l = luma(straight(p)) * gain;
    l += (rnd(hash3(m.j[0] as u32, x as u32, y as u32)) - 0.5) * noise * 0.5;
    if (y / m.i[0]) & 1 == 1 {
        l *= 1.0 - scan * 0.4;
    }
    let dx = (x as f32 + 0.5 - m.f[2][0]) * m.f[2][2];
    let dy = (y as f32 + 0.5 - m.f[2][1]) * m.f[2][3];
    l *= 1.0 - vig * pm::sstep(0.2, 1.4, dx * dx + dy * dy);
    let hot = (l - 1.0).max(0.0) * 0.5;
    out(std::array::from_fn(|i| mix(m.f[1][i] * l, 1.0, hot)), p[3])
}

fn k_letterbox(m: &M, _: &[u32], x: i32, y: i32, a: Img, _: Img) -> Color32 {
    let p = a.at(x, y);
    let s = if m.i[0] == 1 { y as f32 + 0.5 } else { x as f32 + 0.5 };
    let [start, end, fe, _] = m.f[0];
    let inside = (s - start).min(end - s);
    let bar = if fe > 0.0 { 1.0 - (inside / fe + 0.5).clamp(0.0, 1.0) } else { (inside < 0.0) as u8 as f32 };
    let col = m.f[1];
    let ba = col[3] * 255.0 * bar;
    pack(std::array::from_fn(|i| if i < 3 { col[i] * ba + p[i] * (1.0 - ba / 255.0) } else { ba + p[3] * (1.0 - ba / 255.0) }))
}

fn k_tv_sim(m: &M, _: &[u32], x: i32, y: i32, a: Img, _: Img) -> Color32 {
    let base = m.j[0] as u32;
    let [ap, inter, ls, phos] = m.f[0];
    let [stat, gap, hf, _] = m.f[1];
    let hi = hf as i32;
    let mut sy = y + m.i[1];
    if sy >= hi {
        sy -= hi;
    }
    // The blank between fields as the picture rolls.
    let rolling = m.i[1] > 0 && ((hi - m.i[1]) as f32 - (y as f32 + 0.5)).abs() < gap;
    let jx = pm::ifloor((rnd(hash3(base, y as u32, 3)) - 0.5) * ls + 0.5);
    let sx = x + jx;
    let ph = m.i[3];
    let p = a.at(sx, sy);
    let mut c = [straight(a.at(sx + ph, sy))[0], straight(p)[1], straight(a.at(sx - ph, sy))[2]];
    let tri = (x / m.i[0]) % 3;
    for (i, v) in c.iter_mut().enumerate() {
        let wgt = if i as i32 == tri { 1.0 + ap } else { 1.0 - ap * 0.5 };
        *v *= wgt;
    }
    if (y + m.i[2]) & 1 == 1 {
        c = c.map(|v| v * (1.0 - inter * 0.35));
    }
    c = c.map(|v| v + phos * 0.25 * v * v);
    let nz = rnd(hash3(base, x as u32, y as u32 ^ 0x4000_0000));
    c = c.map(|v| mix(v, nz, stat * 0.6));
    if rolling {
        c = [0.0; 3];
    }
    out(c, p[3])
}

fn k_newsprint(m: &M, _: &[u32], x: i32, y: i32, a: Img, _: Img) -> Color32 {
    let [s, off, rmax, _] = m.f[0];
    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
    let mono = m.i[0] == 1;
    let alpha = a.at(x, y)[3];
    let mut o = [1.0f32; 3];
    let screens = if mono { 1 } else { 3 };
    for k in 0..screens {
        let [cs, sn, _, _] = m.f[1 + k];
        let (qx, qy) = (px + off * k as f32, py - off * k as f32);
        let u = qx * cs + qy * sn;
        let v = qy * cs - qx * sn;
        let uc = ((u / s).floor() + 0.5) * s;
        let vc = ((v / s).floor() + 0.5) * s;
        // The cell centre back in the picture.
        let cx = uc * cs - vc * sn - off * k as f32;
        let cy = uc * sn + vc * cs + off * k as f32;
        let c = straight(a.near(cx, cy, true));
        let ink = if mono { 1.0 - luma(c) } else { 1.0 - c[k] };
        let r = rmax * ink.max(0.0).sqrt();
        let (du, dv) = (u - uc, v - vc);
        let dot = (du * du + dv * dv < r * r) as u8 as f32;
        if mono {
            o = [1.0 - dot; 3];
        } else {
            o[k] = 1.0 - dot;
        }
    }
    out(o, alpha)
}

fn k_pixelate(m: &M, _: &[u32], x: i32, y: i32, a: Img, _: Img) -> Color32 {
    let (bw, bh) = (m.i[0], m.i[1]);
    pack(a.at(x / bw * bw + bw / 2, y / bh * bh + bh / 2))
}

fn k_starburst(m: &M, d: &[u32], x: i32, y: i32, a: Img, b: Img) -> Color32 {
    let p = if m.i[1] == 1 { [0.0; 4] } else { a.at(x, y) };
    let mut s = [0.0f32; 4];
    for i in 0..m.i[0] as usize {
        let (ox, oy, wgt) = (d[i * 3] as i32, d[i * 3 + 1] as i32, fw(d, i * 3 + 2));
        let q = b.at0(x - ox, y - oy);
        for (v, c) in s.iter_mut().zip(q) {
            *v += c * wgt;
        }
    }
    let g = m.f[0][0];
    pack(std::array::from_fn(|i| p[i] + s[i] * g))
}

fn k_light_rays(m: &M, _: &[u32], x: i32, y: i32, a: Img, b: Img) -> Color32 {
    let p = a.at(x, y);
    let [lx, ly, rn, st] = m.f[0];
    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
    let (dx, dy) = (lx - px, ly - py);
    let mut s = [0.0f32; 4];
    for i in 0..m.i[0] {
        let t = i as f32 * rn;
        let q = b.near(px + dx * t, py + dy * t, false);
        let wgt = 1.0 - i as f32 / m.i[0] as f32;
        for (v, c) in s.iter_mut().zip(q) {
            *v += c * wgt;
        }
    }
    pack(std::array::from_fn(|i| p[i] + s[i] * st))
}
