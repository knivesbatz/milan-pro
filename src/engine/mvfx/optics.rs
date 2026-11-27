//! MVFX optics: fisheye and punch lenses, zoom and motion blurs, camera shake. Kernels are mirrored in
//! `gpu/mvfx_optics.wgsl`.
use eframe::egui::Color32;
use super::pm::{self, Img, M, fw, hash3, pack, rnd};
use super::{DEFAULT, Kernel, Src, Step, choice, f, flag, fx_row as row, k, preset};
use crate::engine::composite::Affine;
use crate::engine::plugins::{FxEval, GpuPort, PluginInfo};

pub static PLUGINS: &[PluginInfo] = &[
    row("milan.fx.spherize", "Spherize", "Deform", "Creative", "Bulges the picture as through a sphere or fisheye lens (negative: inward)", GpuPort::Mvfx,
        &[f("x", "Center X", 0.0, 1.0, 0.5), f("y", "Center Y", 0.0, 1.0, 0.5), f("amount", "Amount", -1.0, 1.0, 0.5), f("sh", "Horizontal", 0.05, 2.0, 1.0), f("sv", "Vertical", 0.05, 2.0, 1.0), flag("proportional", "Proportional", false)],
        &[DEFAULT, preset("Maximum Sphere Out", &[("amount", 1.0)]), preset("Medium Sphere In", &[("amount", -0.5)]), preset("Fisheye", &[("amount", 0.8), ("sh", 1.15), ("sv", 1.15)]), preset("Half Scale", &[("sh", 0.5), ("proportional", 1.0)])]),
    row("milan.fx.pinch_punch", "Pinch/Punch", "Deform", "Creative", "Punches the centre out towards the viewer, or pinches it in (negative)", GpuPort::Mvfx,
        &[f("x", "Center X", 0.0, 1.0, 0.5), f("y", "Center Y", 0.0, 1.0, 0.5), f("amount", "Amount", -1.0, 1.0, 0.5), f("sh", "Horizontal", 0.05, 2.0, 1.0), f("sv", "Vertical", 0.05, 2.0, 1.0), flag("proportional", "Proportional", false)],
        &[DEFAULT, preset("Maximum Punch", &[("amount", 1.0)]), preset("Medium Pinch", &[("amount", -0.5)]), preset("Maximum Pinch", &[("amount", -1.0)])]),
    row("milan.fx.radial_blur", "Radial Blur", "Blur", "Blur", "Blurs outward from a centre, as a zoom during the exposure", GpuPort::Mvfx,
        &[f("x", "Center X", 0.0, 1.0, 0.5), f("y", "Center Y", 0.0, 1.0, 0.5), choice("type", "Type", &["Proportional", "Fixed radius", "Gaussian"], 0), f("strength", "Strength", 0.0, 1.0, 0.5)],
        &[DEFAULT, preset("Weak Proportional", &[("strength", 0.2)]), preset("Strong Proportional", &[("strength", 0.8)]), preset("Moderate Fixed Radius", &[("type", 1.0), ("strength", 0.6)]), preset("Crash Zoom", &[("type", 2.0), ("strength", 1.0)])]),
    row("milan.fx.linear_blur", "Linear Blur", "Blur", "Blur", "Blurs along one direction, as motion during the exposure", GpuPort::Mvfx,
        &[f("angle", "Angle", 0.0, 360.0, 0.0), f("amount", "Amount", 0.0, 1.0, 0.1)],
        &[DEFAULT, preset("Horizontal Streak", &[("amount", 0.3)]), preset("Vertical", &[("angle", 90.0), ("amount", 0.15)]), preset("Diagonal", &[("angle", 45.0), ("amount", 0.2)])]),
    row("milan.fx.camera_shake", "Camera Shake", "Deform", "Creative", "Hand-held shake, steady or hit on every beat and dying away", GpuPort::Mvfx,
        &[f("amount", "Amount", 0.0, 0.2, 0.03), f("frequency", "Frequency", 0.0, 30.0, 8.0), f("rotation", "Rotation", 0.0, 10.0, 1.0), f("zoom", "Zoom", 1.0, 1.5, 1.08), f("beat", "Beat interval", 0.0, 2.0, 0.0), f("decay", "Beat decay", 0.0, 20.0, 6.0), f("seed", "Seed", 0.0, 1000.0, 0.0)],
        &[DEFAULT, preset("Hand-Held", &[("amount", 0.01), ("frequency", 2.0), ("rotation", 0.3), ("zoom", 1.03)]), preset("Beat Hit 120 BPM", &[("amount", 0.06), ("frequency", 14.0), ("beat", 0.5), ("decay", 8.0), ("zoom", 1.12)]), preset("Earthquake", &[("amount", 0.08), ("frequency", 18.0), ("rotation", 3.0), ("zoom", 1.2)])]),
];

pub const TEMPORAL: &[&str] = &["milan.fx.camera_shake"];
#[cfg(test)]
pub const BLURS: &[&str] = &["milan.fx.radial_blur", "milan.fx.linear_blur"];

pub static KERNELS: &[(&str, Kernel)] = &[("lens", k_lens), ("radial_blur", k_radial_blur), ("linear_blur", k_linear_blur)];

/// The lens kernel's settings: centre, radii, amount, mode (0 Spherize, 1 Pinch/Punch).
fn lens_m(e: &FxEval, wf: f32, hf: f32, mode: i32) -> M {
    let mut m = M::default();
    let rx = (e.f("sh") * wf / 2.0).max(1.0);
    let ry = if e.on("proportional") { rx } else { (e.f("sv") * hf / 2.0).max(1.0) };
    m.f[0] = [e.f("x") * wf, e.f("y") * hf, 1.0 / rx, 1.0 / ry];
    m.f[1] = [e.f("amount"), 0.0, 0.0, 0.0];
    m.i[0] = mode;
    m
}

/// Radial Blur's sample positions (fractions toward the centre) and weights, `n` of each.
fn radial_data(ty: usize, n: usize) -> Vec<f32> {
    let mut d = Vec::with_capacity(2 * n);
    let wts: Vec<f32> = (0..n).map(|i| if ty == 2 { (-(i as f32 / n as f32).powi(2) * 3.0).exp() } else { 1.0 }).collect();
    let total: f32 = wts.iter().sum();
    for (i, w) in wts.iter().enumerate() {
        d.push(i as f32 / (n - 1) as f32);
        d.push(w / total);
    }
    d
}

pub fn recipe(e: &FxEval, w: usize, h: usize) -> Option<Vec<Step>> {
    let (wf, hf) = (w as f32, h as f32);
    let one = |entry, m, data| vec![k(entry, m, data, Src::In(0), Src::In(0))];
    Some(match e.id {
        "milan.fx.spherize" => one("lens", lens_m(e, wf, hf, 0), vec![]),
        "milan.fx.pinch_punch" => one("lens", lens_m(e, wf, hf, 1), vec![]),
        "milan.fx.radial_blur" => {
            const N: usize = 24;
            let ty = e.i("type");
            let mut m = M::default();
            m.i = [ty as i32, N as i32, 0, 0];
            let s = e.f("strength");
            m.f[0] = [e.f("x") * wf, e.f("y") * hf, if ty == 1 { s * 0.08 * hf } else { s * 0.5 }, 0.0];
            one("radial_blur", m, pm::fwords(&radial_data(ty, N)))
        }
        "milan.fx.linear_blur" => {
            const N: usize = 24;
            let (s, c) = e.f("angle").to_radians().sin_cos();
            let len = e.f("amount") * 0.25 * hf;
            let mut d = Vec::with_capacity(2 * N);
            for i in 0..N {
                let t = (i as f32 / (N - 1) as f32 - 0.5) * len;
                d.extend([c * t, s * t]);
            }
            let mut m = M::default();
            m.i[0] = N as i32;
            m.f[0][0] = 1.0 / N as f32;
            one("linear_blur", m, pm::fwords(&d))
        }
        "milan.fx.camera_shake" => vec![Step::Draw { of: Src::In(0), m: shake(e, e.f(super::T) as f64, 1.0, w, h), bilinear: true }],
        _ => return None,
    })
}

/// A smooth random wiggle in -1..1 at time `x` (knots one apart), channel `ch`.
fn wiggle(seed: u32, x: f64, ch: u32) -> f64 {
    let k = x.floor();
    let u = x - k;
    let v = |n: f64| rnd(hash3(seed, n as i64 as u32, ch)) as f64 * 2.0 - 1.0;
    let s = (1.0 - (u * std::f64::consts::PI).cos()) / 2.0;
    v(k) + (v(k + 1.0) - v(k)) * s
}

/// Camera Shake at time `t` (s), scaled by `env` (the transition's ramp): picture pixels to pixels.
pub(crate) fn shake(e: &FxEval, t: f64, env: f64, w: usize, h: usize) -> Affine {
    let seed = e.f("seed").round() as u32;
    let x = t * e.f("frequency") as f64;
    let beat = e.f("beat") as f64;
    let env = env * if beat > 0.0 { (-(e.f("decay") as f64) * t.rem_euclid(beat)).exp() } else { 1.0 };
    let (wf, hf) = (w as f64, h as f64);
    let amt = e.f("amount") as f64 * hf * env;
    let (dx, dy, rot) = (wiggle(seed, x, 1) * amt, wiggle(seed, x, 2) * amt, wiggle(seed, x, 3) * e.f("rotation") as f64 * env);
    let z = 1.0 + (e.f("zoom") as f64 - 1.0) * env.min(1.0);
    Affine::translate(-wf / 2.0, -hf / 2.0).then(Affine::scale(z, z)).then(Affine::rotate(rot)).then(Affine::translate(wf / 2.0 + dx, hf / 2.0 + dy))
}

// ---- kernels (twins in gpu/mvfx_optics.wgsl) ----

fn k_lens(m: &M, _: &[u32], x: i32, y: i32, a: Img, _: Img) -> Color32 {
    let [cx, cy, irx, iry] = m.f[0];
    let amt = m.f[1][0];
    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
    let (ox, oy) = (px - cx, py - cy);
    let (dx, dy) = (ox * irx, oy * iry);
    let r2 = dx * dx + dy * dy;
    if r2 >= 1.0 {
        return pack(a.at(x, y));
    }
    let r = r2.sqrt().max(0.000001);
    let rs = if m.i[0] == 0 {
        let sp = 1.0 - (1.0 - r2).sqrt();
        r + amt * (sp - r)
    } else {
        let q = (1.0 - r) * (1.0 - r);
        r * (1.0 - amt * 0.75 * q)
    };
    let sc = rs / r;
    pack(a.bil(cx + ox * sc, cy + oy * sc, true))
}

fn k_radial_blur(m: &M, d: &[u32], x: i32, y: i32, a: Img, _: Img) -> Color32 {
    let [cx, cy, s, _] = m.f[0];
    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
    let (ox, oy) = (px - cx, py - cy);
    let dist = (ox * ox + oy * oy).sqrt();
    let mut acc = [0.0f32; 4];
    for i in 0..m.i[1] as usize {
        let (t, wgt) = (fw(d, 2 * i), fw(d, 2 * i + 1));
        let (qx, qy) = if m.i[0] == 1 {
            if dist > 0.0 { (px - ox / dist * s * t, py - oy / dist * s * t) } else { (px, py) }
        } else {
            let k = 1.0 - s * t;
            (cx + ox * k, cy + oy * k)
        };
        let q = a.bil(qx, qy, true);
        for (v, c) in acc.iter_mut().zip(q) {
            *v += c * wgt;
        }
    }
    pack(acc)
}

fn k_linear_blur(m: &M, d: &[u32], x: i32, y: i32, a: Img, _: Img) -> Color32 {
    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
    let mut acc = [0.0f32; 4];
    for i in 0..m.i[0] as usize {
        let q = a.bil(px + fw(d, 2 * i), py + fw(d, 2 * i + 1), true);
        for (v, c) in acc.iter_mut().zip(q) {
            *v += c;
        }
    }
    let n = m.f[0][0];
    pack(acc.map(|v| v * n))
}
