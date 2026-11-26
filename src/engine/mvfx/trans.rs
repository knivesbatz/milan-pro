//! MVFX transitions: the flash and blur dissolves, whip and zoom cuts, burn, leak, glitch and shake cuts of the era's
//! music videos, and the DVD-menu-era 3D and pattern transitions bedroom editors loved (Page Peel, Page Roll, Cube
//! Spin, Barn Door, Venetian Blinds, Spiral, Squeeze, Star Wipe, Portals, Explosion). Input 0 is A (outgoing), input 1
//! B (incoming); `recipe` is called for 0 < p < 1 only (`transition::mix` returns A and B at the ends). Kernels are
//! mirrored in `gpu/mvfx_trans.wgsl`.
use eframe::egui::Color32;
use super::pm::{self, Img, M, fw, hash3, mix, pack, rnd};
use super::{DEFAULT, Kernel, Src, Step, WHITE, choice, color, f, flag, k, preset, tr_row as row};
use crate::engine::composite::Affine;
use crate::engine::plugins::{FxEval, PluginInfo};

const SIDES: &[&str] = &["Left", "Right", "Up", "Down"];
const POSITIONS: &[&str] = &["Left", "Right", "Top", "Bottom", "Center"];

pub static PLUGINS: &[PluginInfo] = &[
    row("milan.tr.flash", "Flash", "Flash", "Light", "A bright flash through the cut, the picture diffusing into it", &[f("h_diff", "Horizontal diffusion", 0.0, 1.0, 0.25), f("v_diff", "Vertical diffusion", 0.0, 1.0, 0.25), color("tint", "Tint", WHITE)],
        &[DEFAULT, preset("Hard Flash", &[("h_diff", 0.0), ("v_diff", 0.0)]), preset("Soft Flash", &[("h_diff", 0.6), ("v_diff", 0.6)]), preset("Yellow Flash", &[("tint.b", 0.3)])]),
    row("milan.tr.cross_effect", "Cross Effect", "Dissolve", "Blur", "Zooms, pixelates or blurs the outgoing and incoming pictures as they cross", &[
        choice("effect", "Effect", &["None", "Zoom", "Pixelate", "Blur"], 1),
        choice("apply_to", "Apply to", &["Source", "Destination", "Both"], 2),
        f("fade", "Fade range", 0.0, 1.0, 0.5),
        f("max_zoom", "Max zoom", 1.0, 64.0, 4.0),
        f("max_x", "Max X", 0.0, 1.0, 0.1),
        f("max_y", "Max Y", 0.0, 1.0, 0.1),
        f("max_blur", "Max blur", 0.0, 1.0, 0.1),
    ], &[DEFAULT, preset("Cross Pixelate", &[("effect", 2.0)]), preset("Cross Blur", &[("effect", 3.0)]), preset("Cross Zoom A Only", &[("apply_to", 0.0)]), preset("Blur Dissolve", &[("effect", 3.0), ("fade", 1.0), ("max_blur", 0.05)])]),
    row("milan.tr.zoom_blur", "Zoom Blur", "Zoom", "Blur", "Crash-zooms into the cut with a radial blur and out of it on the other side", &[f("strength", "Strength", 0.0, 1.0, 0.7), f("zoom", "Zoom", 0.0, 1.0, 0.4), f("x", "Center X", 0.0, 1.0, 0.5), f("y", "Center Y", 0.0, 1.0, 0.5), f("crossfade", "Crossfade", 0.0, 0.5, 0.1)],
        &[DEFAULT, preset("Crash Zoom", &[("strength", 1.0), ("zoom", 0.8), ("crossfade", 0.02)]), preset("Gentle", &[("strength", 0.4), ("zoom", 0.2), ("crossfade", 0.3)])]),
    row("milan.tr.whip_pan", "Whip Pan", "Push", "Blur", "A fast pan with motion blur from one shot to the next", &[choice("direction", "Direction", SIDES, 0), f("blur", "Blur", 0.0, 1.0, 0.7)],
        &[DEFAULT, preset("Whip Right", &[("direction", 1.0)]), preset("Whip Up", &[("direction", 2.0)]), preset("Whip Down", &[("direction", 3.0)])]),
    row("milan.tr.film_burn", "Film Burn", "Film", "Light", "The outgoing frame burns through to white and the incoming one appears from the burn", &[color("color", "Color", [1.0, 0.45, 0.1, 1.0]), f("speed", "Speed", 0.0, 2.0, 0.6), f("scale", "Scale", 0.1, 2.0, 0.6), choice("position", "Position", POSITIONS, 4), f("seed", "Seed", 0.0, 1000.0, 0.0)],
        &[DEFAULT, preset("From the Left", &[("position", 0.0)]), preset("Fine Burn", &[("scale", 0.25)])]),
    row("milan.tr.light_leak", "Light Leak", "Film", "Light", "A dissolve washed with a bloom of warm leaking light", &[
        f("intensity", "Intensity", 0.0, 1.0, 0.8),
        f("size", "Size", 0.2, 2.0, 1.1),
        f("speed", "Speed", 0.0, 2.0, 1.0),
        color("color1", "Color 1", [1.0, 0.55, 0.15, 1.0]),
        color("color2", "Color 2", [0.95, 0.2, 0.35, 1.0]),
        choice("position", "Position", POSITIONS, 0),
        f("seed", "Seed", 0.0, 1000.0, 0.0),
    ], &[DEFAULT, preset("Golden", &[("color2.r", 1.0), ("color2.g", 0.8), ("color2.b", 0.3)]), preset("From Above", &[("position", 2.0)])]),
    row("milan.tr.glitch", "Glitch", "Stylize", "Creative", "The picture breaks into displaced bands and split colour and comes back as the next shot", &[f("amount", "Amount", 0.0, 1.0, 0.7), f("block", "Block size", 0.01, 0.25, 0.06), f("shift", "RGB shift", 0.0, 1.0, 0.6), f("seed", "Seed", 0.0, 1000.0, 0.0)],
        &[DEFAULT, preset("Datamosh Cut", &[("amount", 1.0), ("block", 0.12), ("shift", 0.3)]), preset("Light", &[("amount", 0.35)])]),
    row("milan.tr.shake_cut", "Shake Cut", "Push", "Creative", "The camera jolts into the cut and settles on the next shot, with a flash", &[f("amount", "Amount", 0.0, 0.2, 0.05), f("frequency", "Frequency", 0.0, 30.0, 12.0), f("rotation", "Rotation", 0.0, 10.0, 2.0), f("zoom", "Zoom", 1.0, 1.5, 1.1), f("flash", "Flash", 0.0, 1.0, 0.3), f("seed", "Seed", 0.0, 1000.0, 0.0)],
        &[DEFAULT, preset("Hard Hit", &[("amount", 0.1), ("flash", 0.6)]), preset("No Flash", &[("flash", 0.0)])]),
    row("milan.tr.strobe_cut", "Strobe Cut", "Flash", "Light", "Flash frames strobing between the two shots", &[f("flashes", "Flashes", 1.0, 12.0, 3.0), color("color", "Color", WHITE)],
        &[DEFAULT, preset("Single Flash Frame", &[("flashes", 1.0)]), preset("Black Strobe", &[("color.r", 0.0), ("color.g", 0.0), ("color.b", 0.0)])]),
    row("milan.tr.page_peel", "Page Peel", "Page Peel", "Creative", "Peels the outgoing picture away like a page, its back folded over", &[f("angle", "Peel angle", 0.0, 360.0, 45.0), f("fold", "Fold radius", 0.0, 1.0, 0.3), f("opacity", "Peel opacity", 0.0, 1.0, 1.0), color("light", "Light color", WHITE)],
        &[DEFAULT, preset("Peel Up", &[("angle", 270.0)]), preset("Tight Fold", &[("fold", 0.1)]), preset("See-Through", &[("opacity", 0.6)])]),
    row("milan.tr.page_roll", "Page Roll", "Page Peel", "Creative", "Rolls the outgoing picture up like a page", &[f("angle", "Roll angle", 0.0, 360.0, 0.0), f("fold", "Fold radius", 0.0, 1.0, 0.12), f("opacity", "Roll opacity", 0.0, 1.0, 1.0), color("light", "Light color", WHITE)],
        &[DEFAULT, preset("Roll Down", &[("angle", 90.0)]), preset("Fat Roll", &[("fold", 0.3)])]),
    row("milan.tr.cube_spin", "3D Cube Spin", "3D", "Creative", "The two pictures on two faces of a turning cube", &[choice("direction", "Direction", SIDES, 0), f("perspective", "Perspective", 0.0, 1.0, 0.5), f("shading", "Shading", 0.0, 1.0, 0.4)],
        &[DEFAULT, preset("Spin Right", &[("direction", 1.0)]), preset("Spin Up", &[("direction", 2.0)]), preset("Flat", &[("perspective", 0.1), ("shading", 0.0)])]),
    row("milan.tr.barn_door", "Barn Door", "Barn Door", "Utility", "Opens like two barn doors, or closes", &[choice("orientation", "Orientation", &["Horizontal", "Vertical"], 0), choice("direction", "Direction", &["In", "Out"], 1), f("border", "Size", 0.0, 0.1, 0.0), f("feather", "Feather", 0.0, 0.3, 0.02), color("border_color", "Color", WHITE)],
        &[DEFAULT, preset("Horizontal In", &[("direction", 0.0)]), preset("Vertical Out", &[("orientation", 1.0)]), preset("White Border", &[("border", 0.01), ("feather", 0.0)])]),
    row("milan.tr.venetian_blinds", "Venetian Blinds", "Venetian Blinds", "Utility", "Turns slats like a venetian blind", &[f("angle", "Angle", 0.0, 360.0, 0.0), f("feather", "Feather", 0.0, 1.0, 0.1), f("blinds", "Number of blinds", 1.0, 50.0, 10.0)],
        &[DEFAULT, preset("Vertical", &[("angle", 90.0)]), preset("Many Blinds", &[("blinds", 30.0)]), preset("Diagonal", &[("angle", 45.0), ("blinds", 14.0)])]),
    row("milan.tr.spiral", "Spiral", "Spiral", "Creative", "The incoming picture spirals in from an edge, turning and growing (or the outgoing one spirals out)", &[f("turns", "Turns", 0.0, 5.0, 1.0), f("zoom", "Zoom (%)", 0.0, 100.0, 50.0), choice("orientation", "Orientation", &["Left", "Top", "Right", "Bottom"], 0), choice("motion", "Motion", &["Clockwise", "Counterclockwise"], 0), choice("direction", "Direction", &["Spiral in", "Spiral out"], 0)],
        &[DEFAULT, preset("Spiral Out", &[("direction", 1.0)]), preset("Two Turns", &[("turns", 2.0), ("zoom", 10.0)]), preset("From the Top", &[("orientation", 1.0), ("motion", 1.0)])]),
    row("milan.tr.squeeze", "Squeeze", "Squeeze", "Utility", "The incoming picture squeezes in from an edge, pushing the outgoing one flat", &[choice("start", "Start", &["Top", "Bottom", "Left", "Right", "Lateral", "Vertical"], 2), flag("squeeze_previous", "Squeeze previous image", true)],
        &[DEFAULT, preset("From the Top", &[("start", 0.0)]), preset("Lateral", &[("start", 4.0)]), preset("Over the Previous", &[("squeeze_previous", 0.0)])]),
    row("milan.tr.star_wipe", "Star Wipe", "Iris", "Creative", "A star opening from the centre", &[
        f("arms", "Arms", 2.0, 32.0, 5.0),
        f("ratio", "Ratio", 0.05, 1.0, 0.45),
        f("angle", "Angle", 0.0, 360.0, 0.0),
        f("feather", "Feather", 0.0, 0.5, 0.02),
        f("border", "Border", 0.0, 0.1, 0.0),
        color("border_color", "Border color", WHITE),
        f("x", "Center X", 0.0, 1.0, 0.5),
        f("y", "Center Y", 0.0, 1.0, 0.5),
        choice("waveform", "Waveform", &["Seesaw", "Tooth"], 0),
    ], &[DEFAULT, preset("Gear", &[("arms", 16.0), ("ratio", 0.8)]), preset("Sun Rays", &[("arms", 24.0), ("ratio", 0.2), ("waveform", 1.0)]), preset("Gold Border", &[("border", 0.01), ("border_color.b", 0.2), ("border_color.g", 0.8)])]),
    row("milan.tr.portals", "Portals", "Portals", "Creative", "The incoming picture opens in squares that appear one by one", &[f("seed", "Random pattern seed", 0.0, 1000.0, 0.0), f("squares", "Squares", 2.0, 20.0, 8.0), f("transparency", "Max transparency", 0.0, 1.0, 0.5), f("offset", "Max offset", 0.0, 1.0, 0.3), f("scale", "Max scale", 0.0, 1.0, 0.5), f("border", "Border size", 0.0, 0.05, 0.0), color("border_color", "Border color", WHITE)],
        &[DEFAULT, preset("White Wash", &[("border", 0.01), ("squares", 6.0)]), preset("Mondrian", &[("squares", 12.0), ("border", 0.006), ("border_color.r", 0.0), ("border_color.g", 0.0), ("border_color.b", 0.0)])]),
    row("milan.tr.explosion", "Explosion", "3D", "Creative", "The outgoing picture shatters into pieces that fly at the viewer", &[f("pieces", "Pieces across", 4.0, 24.0, 10.0), f("force", "Force", 0.0, 2.0, 1.0), f("spin", "Spin", 0.0, 2.0, 1.0), f("seed", "Seed", 0.0, 1000.0, 0.0)],
        &[DEFAULT, preset("Big Pieces", &[("pieces", 5.0)]), preset("Shatter", &[("pieces", 18.0), ("force", 1.4), ("spin", 1.6)])]),
];

pub const TEMPORAL: &[&str] = &[];
#[cfg(test)]
pub const BLURS: &[&str] = &["milan.tr.flash", "milan.tr.cross_effect", "milan.tr.zoom_blur", "milan.tr.whip_pan"];

pub static KERNELS: &[(&str, Kernel)] = &[
    ("lerp", k_lerp),
    ("over", k_over),
    ("tint", k_tint),
    ("zoom_blur", k_zoom_blur),
    ("whip", k_whip),
    ("glitch_tr", k_glitch_tr),
    ("page", k_page),
    ("cube", k_cube),
    ("wipe", k_wipe),
    ("portals", k_portals),
    ("explode", k_explode),
];

const A: Src = Src::In(0);
const B: Src = Src::In(1);

fn lerp(a: Src, b: Src, t: f32) -> Step {
    let mut m = M::default();
    m.f[0][0] = t;
    k("lerp", m, vec![], a, b)
}

fn over(under: Src, top: Src) -> Step {
    k("over", M::default(), vec![], under, top)
}

/// `of` drawn through `m` (pixels to pixels) onto transparency.
fn draw(of: Src, m: Affine) -> Step {
    Step::Draw { of, m, bilinear: true }
}

/// About the frame centre: scale `s`, turn `deg`, then move by (dx, dy) pixels.
fn about_centre(w: usize, h: usize, sx: f64, sy: f64, deg: f64, dx: f64, dy: f64) -> Affine {
    let (cx, cy) = (w as f64 / 2.0, h as f64 / 2.0);
    Affine::translate(-cx, -cy).then(Affine::scale(sx, sy)).then(Affine::rotate(deg)).then(Affine::translate(cx + dx, cy + dy))
}

/// Peak 1 at the middle of the transition.
fn peak(p: f32) -> f32 {
    1.0 - (2.0 * p - 1.0).abs()
}

pub fn recipe(e: &FxEval, w: usize, h: usize, p: f32) -> Option<Vec<Step>> {
    let (wf, hf) = (w as f32, h as f32);
    let mut m = M::default();
    let side = if p < 0.5 { A } else { B };
    Some(match e.id {
        "milan.tr.flash" => {
            let pk = peak(p);
            let r = |d: f32| (d * pk * 0.05 * hf).round() as usize;
            let t = e.c("tint");
            m.f[0] = [t[0] * 255.0, t[1] * 255.0, t[2] * 255.0, pk * pk];
            vec![Step::Blur { of: side, rx: r(e.f("h_diff")), ry: r(e.f("v_diff")) }, k("tint", m, vec![], Src::Step(0), Src::Step(0))]
        }
        "milan.tr.cross_effect" => {
            let to = e.i("apply_to");
            let (sa, sb) = (if to != 1 { p } else { 0.0 }, if to != 0 { 1.0 - p } else { 0.0 });
            let fade = e.f("fade");
            let t = pm::sstep(0.5 - fade / 2.0, 0.5 + fade / 2.0, p);
            let mut steps = Vec::new();
            let fx = |of: Src, s: f32, steps: &mut Vec<Step>| -> Src {
                if s <= 0.0 {
                    return of;
                }
                match e.i("effect") {
                    1 => {
                        let z = 1.0 + (e.f("max_zoom") as f64 - 1.0) * (s * s) as f64;
                        steps.push(draw(of, about_centre(w, h, z, z, 0.0, 0.0, 0.0)));
                    }
                    2 => {
                        let mut pm = M::default();
                        let b = |v: f32, n: f32| ((v * 0.25 * n * s * s).round() as i32).max(1);
                        pm.i = [b(e.f("max_x"), wf), b(e.f("max_y"), hf), 0, 0];
                        steps.push(k("pixelate", pm, vec![], of, of));
                    }
                    3 => {
                        let r = (e.f("max_blur") * 0.1 * hf * s).round() as usize;
                        steps.push(Step::Blur { of, rx: r, ry: r });
                    }
                    _ => return of,
                }
                Src::Step(steps.len() - 1)
            };
            let a2 = fx(A, sa, &mut steps);
            let b2 = fx(B, sb, &mut steps);
            steps.push(lerp(a2, b2, t));
            steps
        }
        "milan.tr.zoom_blur" => {
            let cut = e.f("crossfade");
            let t = pm::sstep(0.5 - cut, 0.5 + cut, p);
            let mut steps = Vec::new();
            for (of, s) in [(A, (2.0 * p).min(1.0)), (B, (2.0 - 2.0 * p).min(1.0))] {
                let mut zm = M::default();
                const N: usize = 20;
                zm.i[1] = N as i32;
                zm.f[0] = [e.f("x") * wf, e.f("y") * hf, e.f("strength") * 0.5 * s * s, 1.0 / (1.0 + e.f("zoom") * s * s)];
                let d: Vec<f32> = (0..N).flat_map(|i| [i as f32 / (N - 1) as f32, 1.0 / N as f32]).collect();
                steps.push(k("zoom_blur", zm, pm::fwords(&d), of, of));
            }
            steps.push(lerp(Src::Step(0), Src::Step(1), t));
            steps
        }
        "milan.tr.whip_pan" => {
            let dir = e.i("direction");
            let (axis, sign) = (if dir < 2 { 0 } else { 1 }, if dir % 2 == 0 { 1 } else { -1 });
            let len = if axis == 0 { wf } else { hf };
            let ease = pm::sstep(0.0, 1.0, p);
            let speed = 6.0 * p * (1.0 - p) / 1.5;
            let blur = e.f("blur") * speed * 0.3 * len;
            const N: usize = 16;
            let offs: Vec<u32> = (0..N).map(|i| ((i as f32 / (N - 1) as f32 - 0.5) * blur).round() as i32 as u32).collect();
            m.i = [axis, sign, (ease * len).round() as i32, N as i32];
            m.f[0][0] = 1.0 / N as f32;
            vec![k("whip", m, offs, A, B)]
        }
        "milan.tr.film_burn" => {
            super::looks::film_burn_m(&mut m, e, wf, hf, p * 2.0, peak(p));
            vec![k("film_burn", m, vec![], side, side)]
        }
        "milan.tr.light_leak" => {
            super::looks::light_leak_m(&mut m, e, wf, hf, p * 2.0, e.f("intensity") * (peak(p) * 1.6).min(1.0));
            vec![lerp(A, B, pm::sstep(0.3, 0.7, p)), k("light_leak", m, vec![], Src::Step(0), Src::Step(0))]
        }
        "milan.tr.glitch" => {
            let step = (p * 12.0).floor() as u32;
            let pk = peak(p);
            m.j[0] = hash3(e.f("seed").round() as u32, step, 0x61c7) as i32;
            m.i = [((e.f("block") * hf).round() as i32).max(1), (e.f("shift") * 0.03 * wf * pk).round() as i32, 0, 0];
            m.f[0] = [e.f("amount") * pk, 0.3 * wf, p, 0.0];
            vec![k("glitch_tr", m, vec![], A, B)]
        }
        "milan.tr.shake_cut" => {
            let pk = peak(p);
            let shake = super::optics::shake(e, p as f64, pk as f64, w, h);
            m.f[0] = [255.0, 255.0, 255.0, e.f("flash") * pk * pk * pk];
            vec![draw(side, shake), k("tint", m, vec![], Src::Step(0), Src::Step(0))]
        }
        "milan.tr.strobe_cut" => {
            let n = e.f("flashes").round().max(1.0);
            let seg = (p * (2.0 * n + 1.0)).floor() as i64;
            if seg % 2 == 1 {
                let c = e.c("color");
                m.i[0] = 1;
                m.f[0] = [c[0] * c[3] * 255.0, c[1] * c[3] * 255.0, c[2] * c[3] * 255.0, c[3] * 255.0];
                vec![k("strobe", m, vec![], A, A)]
            } else {
                vec![k("copy", m, vec![], side, side)]
            }
        }
        "milan.tr.page_peel" | "milan.tr.page_roll" => {
            let (s, c) = e.f("angle").to_radians().sin_cos();
            let ends = [(0.0, 0.0), (wf, 0.0), (0.0, hf), (wf, hf)].map(|(x, y)| x * c + y * s);
            let (lo, hi) = (ends.iter().copied().fold(f32::INFINITY, f32::min), ends.iter().copied().fold(f32::NEG_INFINITY, f32::max));
            let r = e.f("fold") * 0.25 * wf.min(hf) + 1.0;
            m.i[0] = (e.id == "milan.tr.page_roll") as i32;
            m.f[0] = [c, s, lo + p * (hi - lo + r), r];
            m.f[1] = [lo, e.f("opacity"), 0.0, 0.0];
            m.f[2] = e.c("light");
            vec![k("page", m, vec![], A, B)]
        }
        "milan.tr.cube_spin" => cube(e, w, h, p),
        "milan.tr.barn_door" => {
            m.i = [0, e.i("orientation") as i32, e.i("direction") as i32, 0];
            m.f[0] = [p, e.f("feather"), e.f("border") * 4.0, 0.0];
            m.f[1] = e.c("border_color");
            m.f[2] = [wf / 2.0, hf / 2.0, 2.0 / wf, 2.0 / hf];
            vec![k("wipe", m, vec![], A, B)]
        }
        "milan.tr.venetian_blinds" => {
            let (s, c) = e.f("angle").to_radians().sin_cos();
            m.i = [1, 0, 0, 0];
            m.f[0] = [p, e.f("feather"), 0.0, e.f("blinds").round().max(1.0)];
            m.f[2] = [c, s, 1.0 / (c.abs() * wf + s.abs() * hf).max(1.0), 0.0];
            vec![k("wipe", m, vec![], A, B)]
        }
        "milan.tr.star_wipe" => {
            let (cx, cy) = (e.f("x") * wf, e.f("y") * hf);
            let far = [(0.0, 0.0), (wf, 0.0), (0.0, hf), (wf, hf)].iter().map(|(x, y)| (x - cx).hypot(y - cy)).fold(0.0f32, f32::max);
            let ratio = e.f("ratio").max(0.05);
            m.i = [2, e.i("waveform") as i32, 0, 0];
            m.f[0] = [p, e.f("feather"), e.f("border") * 4.0, e.f("arms").round()];
            m.f[1] = e.c("border_color");
            m.f[2] = [cx, cy, ratio / far.max(1.0), e.f("angle") / 360.0];
            m.f[3] = [ratio, 0.0, 0.0, 0.0];
            vec![k("wipe", m, vec![], A, B)]
        }
        "milan.tr.spiral" => {
            let out = e.i("direction") == 1;
            let q = if out { 1.0 - p } else { p } as f64;
            let sgn = if e.i("motion") == 1 { -1.0 } else { 1.0 };
            let z0 = e.f("zoom") as f64 / 100.0;
            let sc = z0 + (1.0 - z0) * q;
            let (ex, ey) = [(-1.0, 0.0), (0.0, -1.0), (1.0, 0.0), (0.0, 1.0)][e.i("orientation").min(3)];
            let travel = 1.0 - q;
            let m = about_centre(w, h, sc, sc, sgn * (1.0 - q) * e.f("turns") as f64 * 360.0, ex * travel * w as f64, ey * travel * h as f64);
            if out { vec![draw(A, m), over(B, Src::Step(0))] } else { vec![draw(B, m), over(A, Src::Step(0))] }
        }
        "milan.tr.squeeze" => {
            let (wd, hd, q) = (w as f64, h as f64, p as f64);
            // B grows from its edge (or from the middle), A is flattened into what is left.
            let (bm, am) = match e.i("start") {
                0 => (Affine::scale(1.0, q), Affine::scale(1.0, 1.0 - q).then(Affine::translate(0.0, q * hd))),
                1 => (Affine::scale(1.0, q).then(Affine::translate(0.0, (1.0 - q) * hd)), Affine::scale(1.0, 1.0 - q)),
                2 => (Affine::scale(q, 1.0), Affine::scale(1.0 - q, 1.0).then(Affine::translate(q * wd, 0.0))),
                3 => (Affine::scale(q, 1.0).then(Affine::translate((1.0 - q) * wd, 0.0)), Affine::scale(1.0 - q, 1.0)),
                4 => (about_centre(w, h, q, 1.0, 0.0, 0.0, 0.0), about_centre(w, h, 1.0 + q, 1.0, 0.0, 0.0, 0.0)),
                _ => (about_centre(w, h, 1.0, q, 0.0, 0.0, 0.0), about_centre(w, h, 1.0, 1.0 + q, 0.0, 0.0, 0.0)),
            };
            if e.on("squeeze_previous") {
                vec![draw(A, am), draw(B, bm), over(Src::Step(0), Src::Step(1))]
            } else {
                vec![draw(B, bm), over(A, Src::Step(0))]
            }
        }
        "milan.tr.portals" => {
            let n = e.f("squares").round().max(2.0);
            let cell = (wf / n).max(1.0);
            m.j[0] = hash3(e.f("seed").round() as u32, 0x9047, 1) as i32;
            m.f[0] = [p, cell, e.f("transparency"), e.f("offset")];
            m.f[1] = [e.f("scale"), e.f("border") * hf, 0.0, 0.0];
            m.f[2] = e.c("border_color");
            vec![k("portals", m, vec![], A, B)]
        }
        "milan.tr.explosion" => explosion(e, w, h, p),
        _ => return None,
    })
}

/// The cube's two faces at p: each face's screen-to-face homography (inverse), its shade and whether it shows.
fn cube(e: &FxEval, w: usize, h: usize, p: f32) -> Vec<Step> {
    let dir = e.i("direction");
    // A cube of side 2 centred on the origin (y down), the camera on -z at `cam`; A on the front face (z = -1) and
    // B on the face that turns to the front: Left brings the right face round, Right the left, Up the bottom, Down
    // the top. Corners in picture order: top left, top right, bottom right, bottom left.
    let (sign, b): (f64, [[f64; 3]; 4]) = match dir {
        0 => (1.0, [[1.0, -1.0, -1.0], [1.0, -1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, -1.0]]),
        1 => (-1.0, [[-1.0, -1.0, 1.0], [-1.0, -1.0, -1.0], [-1.0, 1.0, -1.0], [-1.0, 1.0, 1.0]]),
        2 => (-1.0, [[-1.0, 1.0, -1.0], [1.0, 1.0, -1.0], [1.0, 1.0, 1.0], [-1.0, 1.0, 1.0]]),
        _ => (1.0, [[-1.0, -1.0, 1.0], [1.0, -1.0, 1.0], [1.0, -1.0, -1.0], [-1.0, -1.0, -1.0]]),
    };
    let ang = p as f64 * std::f64::consts::FRAC_PI_2 * sign;
    let cam = 3.0 + (1.0 - e.f("perspective") as f64) * 12.0;
    let vertical = dir >= 2;
    let rot = |v: [f64; 3]| -> [f64; 3] {
        let (s, c) = ang.sin_cos();
        if vertical { [v[0], v[1] * c - v[2] * s, v[1] * s + v[2] * c] } else { [v[0] * c + v[2] * s, v[1], -v[0] * s + v[2] * c] }
    };
    let f = cam - 1.0;
    let (wd, hd) = (w as f64, h as f64);
    let proj = |v: [f64; 3]| -> (f64, f64) { ((v[0] * f / (v[2] + cam) + 1.0) * wd / 2.0, (v[1] * f / (v[2] + cam) + 1.0) * hd / 2.0) };
    let a = [[-1.0, -1.0, -1.0], [1.0, -1.0, -1.0], [1.0, 1.0, -1.0], [-1.0, 1.0, -1.0]];
    let mut m = M::default();
    let shading = e.f("shading");
    for (i, face) in [a, b].iter().enumerate() {
        let r = face.map(rot);
        let q = r.map(proj);
        // Facing the camera: the projected corners run clockwise on screen.
        let area: f64 = (0..4).map(|j| q[j].0 * q[(j + 1) % 4].1 - q[(j + 1) % 4].0 * q[j].1).sum();
        let n = { let (u, v) = ([r[1][0] - r[0][0], r[1][1] - r[0][1], r[1][2] - r[0][2]], [r[3][0] - r[0][0], r[3][1] - r[0][1], r[3][2] - r[0][2]]); [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]] };
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-9);
        let facing = (-n[2] / len).abs();
        let inv = square_to_quad(q).and_then(invert3);
        let vis = area > 1.0 && inv.is_some();
        let hm = inv.unwrap_or([0.0; 9]).map(|v| v as f32);
        m.f[i * 3] = [hm[0], hm[1], hm[2], hm[3]];
        m.f[i * 3 + 1] = [hm[4], hm[5], hm[6], hm[7]];
        m.f[i * 3 + 2] = [hm[8], 0.0, 0.0, 0.0];
        m.f[6][i] = 1.0 - shading * (1.0 - facing as f32);
        m.i[i] = vis as i32;
    }
    vec![k("cube", m, vec![], A, B)]
}

/// The homography taking the unit square (0,0), (1,0), (1,1), (0,1) to quad `q` (row-major 3x3).
fn square_to_quad(q: [(f64, f64); 4]) -> Option<[f64; 9]> {
    let [(x0, y0), (x1, y1), (x2, y2), (x3, y3)] = q;
    let (sx, sy) = (x0 - x1 + x2 - x3, y0 - y1 + y2 - y3);
    let (dx1, dy1, dx2, dy2) = (x1 - x2, y1 - y2, x3 - x2, y3 - y2);
    let den = dx1 * dy2 - dx2 * dy1;
    if den.abs() < 1e-12 {
        return None;
    }
    let g = (sx * dy2 - dx2 * sy) / den;
    let hh = (dx1 * sy - sx * dy1) / den;
    Some([x1 - x0 + g * x1, x3 - x0 + hh * x3, x0, y1 - y0 + g * y1, y3 - y0 + hh * y3, y0, g, hh, 1.0])
}

fn invert3(m: [f64; 9]) -> Option<[f64; 9]> {
    let [a, b, c, d, e, f, g, h, i] = m;
    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    if det.abs() < 1e-12 {
        return None;
    }
    let r = 1.0 / det;
    Some([(e * i - f * h) * r, (c * h - b * i) * r, (b * f - c * e) * r, (f * g - d * i) * r, (a * i - c * g) * r, (c * d - a * f) * r, (d * h - e * g) * r, (b * g - a * h) * r, (a * e - b * d) * r])
}

/// The pieces at p: 12 floats each (inverse affine a b c d tx ty, alpha, source rectangle x0 y0 x1 y1, 0).
fn explosion(e: &FxEval, w: usize, h: usize, p: f32) -> Vec<Step> {
    let (wd, hd) = (w as f64, h as f64);
    let cols = e.f("pieces").round().max(2.0) as usize;
    let rows = ((cols as f64 * hd / wd).round() as usize).max(1);
    let (pw, ph) = (wd / cols as f64, hd / rows as f64);
    let seed = e.f("seed").round() as u32;
    let q = p as f64;
    let mut d: Vec<f32> = Vec::new();
    for j in 0..rows {
        for i in 0..cols {
            let r = |s: u32| rnd(hash3(seed, (j * cols + i) as u32, s)) as f64;
            let (cx, cy) = ((i as f64 + 0.5) * pw, (j as f64 + 0.5) * ph);
            let (ox, oy) = (cx - wd / 2.0, cy - hd / 2.0);
            let dist = ox.hypot(oy).max(1.0);
            let push = e.f("force") as f64 * q * q * (0.6 + 0.8 * r(1)) * wd;
            let (dx, dy) = (ox / dist * push + (r(2) - 0.5) * push * 0.4, oy / dist * push + (r(3) - 0.5) * push * 0.4);
            let rot = (r(4) - 0.5) * 2.0 * e.f("spin") as f64 * 540.0 * q;
            let s = 1.0 + 1.5 * q * (0.5 + r(5));
            let m = Affine::translate(-cx, -cy).then(Affine::scale(s, s)).then(Affine::rotate(rot)).then(Affine::translate(cx + dx, cy + dy));
            let Some(inv) = m.inverse() else { continue };
            let alpha = (1.0 - q * q).clamp(0.0, 1.0);
            d.extend([inv.a, inv.b, inv.c, inv.d, inv.tx, inv.ty, alpha, i as f64 * pw, j as f64 * ph, (i + 1) as f64 * pw, (j + 1) as f64 * ph, 0.0].map(|v| v as f32));
        }
    }
    let mut m = M::default();
    m.i[0] = (d.len() / 12) as i32;
    vec![k("explode", m, pm::fwords(&d), A, B)]
}

// ---- kernels (twins in gpu/mvfx_trans.wgsl) ----

fn k_lerp(m: &M, _: &[u32], x: i32, y: i32, a: Img, b: Img) -> Color32 {
    let (p, q) = (a.at(x, y), b.at(x, y));
    let t = m.f[0][0];
    pack(std::array::from_fn(|i| p[i] + (q[i] - p[i]) * t))
}

/// `b` over `a` (premultiplied).
fn k_over(_: &M, _: &[u32], x: i32, y: i32, a: Img, b: Img) -> Color32 {
    let (p, q) = (a.at(x, y), b.at(x, y));
    let k = 1.0 - q[3] / 255.0;
    pack(std::array::from_fn(|i| q[i] + p[i] * k))
}

/// Towards an opaque colour (f[0].rgb, 0..255) by f[0].w.
fn k_tint(m: &M, _: &[u32], x: i32, y: i32, a: Img, _: Img) -> Color32 {
    let p = a.at(x, y);
    let t = m.f[0][3];
    let c = [m.f[0][0], m.f[0][1], m.f[0][2], 255.0];
    pack(std::array::from_fn(|i| p[i] + (c[i] - p[i]) * t))
}

fn k_zoom_blur(m: &M, d: &[u32], x: i32, y: i32, a: Img, _: Img) -> Color32 {
    let [cx, cy, s, iz] = m.f[0];
    let (ox, oy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
    let mut acc = [0.0f32; 4];
    for i in 0..m.i[1] as usize {
        let (t, wgt) = (fw(d, 2 * i), fw(d, 2 * i + 1));
        let k = (1.0 - s * t) * iz;
        let q = a.bil(cx + ox * k, cy + oy * k, true);
        for (v, c) in acc.iter_mut().zip(q) {
            *v += c * wgt;
        }
    }
    pack(acc)
}

fn k_whip(m: &M, d: &[u32], x: i32, y: i32, a: Img, b: Img) -> Color32 {
    let (axis, sign, shift, n) = (m.i[0], m.i[1], m.i[2], m.i[3]);
    let len = if axis == 0 { a.w } else { a.h };
    let mut acc = [0.0f32; 4];
    for i in 0..n as usize {
        let along = if axis == 0 { x } else { y } + sign * (shift + d[i] as i32);
        let q = if sign > 0 {
            if along < len { (a, along) } else { (b, along - len) }
        } else if along >= 0 {
            (a, along)
        } else {
            (b, along + len)
        };
        let c = if axis == 0 { q.0.at(q.1, y) } else { q.0.at(x, q.1) };
        for (v, c) in acc.iter_mut().zip(c) {
            *v += c;
        }
    }
    pack(acc.map(|v| v * m.f[0][0]))
}

fn k_glitch_tr(m: &M, _: &[u32], x: i32, y: i32, a: Img, b: Img) -> Color32 {
    let base = m.j[0] as u32;
    let bh = m.i[0];
    let band = (y / bh) as u32;
    let hb = hash3(base, band, 7);
    let [amt, reach, p, _] = m.f[0];
    let pic = if rnd(hash3(base, band, 11)) < p { b } else { a };
    let dx = if rnd(hb) < amt * 0.7 { pm::ifloor((rnd(pm::hash(hb)) - 0.5) * amt * reach + 0.5) } else { 0 };
    let sx = x + dx;
    let s = m.i[1];
    let c = pic.at(sx, y);
    let mut o = [pic.at(sx + s, y)[0], c[1], pic.at(sx - s, y)[2], c[3]];
    let hc = hash3(base, band, (sx.max(0) / (bh * 2)) as u32 + 101);
    if rnd(hc) < amt * 0.2 {
        o = [o[2], o[0], o[1], o[3]];
    }
    pack(o)
}

fn k_page(m: &M, _: &[u32], x: i32, y: i32, a: Img, b: Img) -> Color32 {
    let [dx, dy, l, r] = m.f[0];
    let [lo, opacity, _, _] = m.f[1];
    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
    let s = px * dx + py * dy;
    let u = s - l;
    let lifted = l - lo;
    let roll = m.i[0] == 1;
    let light = m.f[2];
    // (material distance back from the line, back side, shade) of what covers this pixel.
    let mut hit: Option<(f32, bool, f32)> = None;
    if u >= 0.0 {
        let dl = pm::PI * r + u;
        if !roll && dl <= lifted {
            hit = Some((dl, true, 0.9));
        }
    } else if u > -r {
        let t = -u / r;
        let t1 = pm::asin(t);
        let up = (1.0 - t * t).max(0.0).sqrt();
        let (d1, d2) = (r * t1, r * (pm::PI - t1));
        if d2 <= lifted {
            hit = Some((d2, true, 0.75 + 0.25 * up));
        } else if d1 <= lifted {
            hit = Some((d1, false, 0.4 + 0.6 * up));
        }
    }
    let under = if u >= 0.0 { a.at(x, y) } else { b.at(x, y) };
    let Some((dl, back, shade)) = hit else { return pack(under) };
    let back_by = u + dl;
    let mat = a.bil(px - dx * back_by, py - dy * back_by, false);
    let mc = pm::straight(mat);
    let col: [f32; 3] = std::array::from_fn(|i| if back { mix(mc[i], light[i], 0.4) * shade } else { mc[i] * shade });
    let top = pm::premul(col, mat[3]);
    let k = 1.0 - top[3] * opacity / 255.0;
    pack(std::array::from_fn(|i| top[i] * opacity + under[i] * k))
}

fn k_cube(m: &M, _: &[u32], x: i32, y: i32, a: Img, b: Img) -> Color32 {
    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
    for (face, pic) in [(0usize, a), (1, b)] {
        if m.i[face] == 0 {
            continue;
        }
        let (r0, r1, r2) = (m.f[face * 3], m.f[face * 3 + 1], m.f[face * 3 + 2]);
        let den = r1[2] * px + r1[3] * py + r2[0];
        if den == 0.0 {
            continue;
        }
        let u = (r0[0] * px + r0[1] * py + r0[2]) / den;
        let v = (r0[3] * px + r1[0] * py + r1[1]) / den;
        if (0.0..1.0).contains(&u) && (0.0..1.0).contains(&v) {
            let c = pic.bil(u * pic.w as f32, v * pic.h as f32, true);
            let s = m.f[6][face];
            return pack([c[0] * s, c[1] * s, c[2] * s, c[3]]);
        }
    }
    pack([0.0; 4])
}

fn band(s: f32, p: f32, f: f32) -> f32 {
    let e = p * (1.0 + f) - f;
    if f <= 0.0 { (s < e) as u8 as f32 } else { ((e + f - s) / f).clamp(0.0, 1.0) }
}

fn k_wipe(m: &M, _: &[u32], x: i32, y: i32, a: Img, b: Img) -> Color32 {
    let [p, fe, border, n] = m.f[0];
    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
    let s = match m.i[0] {
        0 => {
            let v = if m.i[1] == 0 { ((px - m.f[2][0]) * m.f[2][2]).abs() } else { ((py - m.f[2][1]) * m.f[2][3]).abs() };
            if m.i[2] == 0 { 1.0 - v } else { v }
        }
        1 => {
            let t = (px * m.f[2][0] + py * m.f[2][1]) * m.f[2][2] * n;
            t - t.floor()
        }
        _ => {
            let (ox, oy) = (px - m.f[2][0], py - m.f[2][1]);
            let ang = pm::atan2(oy, ox) * 0.15915494 * n + m.f[2][3];
            let fr = ang - ang.floor();
            let wave = if m.i[1] == 0 { 1.0 - (2.0 * fr - 1.0).abs() } else { fr };
            let star = mix(m.f[3][0], 1.0, wave);
            (ox * ox + oy * oy).sqrt() * m.f[2][2] / star
        }
    };
    let wt = band(s, p, fe);
    let (pa, pb) = (a.at(x, y), b.at(x, y));
    let mut o: [f32; 4] = std::array::from_fn(|i| pa[i] + (pb[i] - pa[i]) * wt);
    if border > 0.0 {
        let e = p * (1.0 + fe) - fe;
        if (s - e).abs() < border && p > 0.0 {
            let c = m.f[1];
            o = [c[0] * 255.0, c[1] * 255.0, c[2] * 255.0, 255.0];
        }
    }
    pack(o)
}

fn k_portals(m: &M, _: &[u32], x: i32, y: i32, a: Img, b: Img) -> Color32 {
    let [p, cell, transp, offset] = m.f[0];
    let [scale, border, _, _] = m.f[1];
    let base = m.j[0] as u32;
    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
    let (ci, cj) = ((px / cell).floor(), (py / cell).floor());
    let h = |s: u32| rnd(hash3(base, pm::ifloor(ci) as u32, (pm::ifloor(cj) as u32).wrapping_mul(8).wrapping_add(s)));
    let delay = h(0) * 0.5;
    let q = ((p - delay) * 2.0).clamp(0.0, 1.0);
    let pa = a.at(x, y);
    if q <= 0.0 {
        return pack(pa);
    }
    let rest = 1.0 - q;
    let (cx, cy) = ((ci + 0.5) * cell + (h(1) - 0.5) * offset * cell * rest, (cj + 0.5) * cell + (h(2) - 0.5) * offset * cell * rest);
    let half = q * cell * 0.5;
    let (dx, dy) = (px - cx, py - cy);
    if dx.abs() >= half || dy.abs() >= half {
        return pack(pa);
    }
    if border > 0.0 && (half - dx.abs() < border || half - dy.abs() < border) {
        let c = m.f[2];
        return pack([c[0] * 255.0, c[1] * 255.0, c[2] * 255.0, 255.0]);
    }
    let z = 1.0 + scale * rest;
    let pb = b.bil(cx + dx / z, cy + dy / z, true);
    let al = 1.0 - transp * rest;
    pack(std::array::from_fn(|i| pa[i] + (pb[i] - pa[i]) * al))
}

fn k_explode(m: &M, d: &[u32], x: i32, y: i32, a: Img, b: Img) -> Color32 {
    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
    let under = b.at(x, y);
    let mut i = m.i[0] as usize;
    while i > 0 {
        i -= 1;
        let q = |j: usize| fw(d, i * 12 + j);
        let sx = q(0) * px + q(1) * py + q(4);
        let sy = q(2) * px + q(3) * py + q(5);
        if sx >= q(7) && sx < q(9) && sy >= q(8) && sy < q(10) {
            let c = a.bil(sx, sy, true);
            let al = q(6);
            let k = 1.0 - c[3] * al / 255.0;
            return pack(std::array::from_fn(|j| c[j] * al + under[j] * k));
        }
    }
    pack(under)
}
