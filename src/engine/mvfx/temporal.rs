//! MVFX temporal effects: Strobe (a function of the frame number), and Echo, Stutter and Datamosh, which read earlier
//! source frames of their event through the frame's `Sources` (`mvfx::event_fx`, the GPU's `mv_event`). Each is a
//! pure function of the frame: no state is kept between frames. Kernels are mirrored in `gpu/mvfx_temporal.wgsl`.
//!
//! Datamosh is an approximation of codec datamoshing (the "Welcome to Heartbreak" look): it does not touch a
//! compressed stream. Every `interval` frames a picture is held as a "keyframe"; for the next `length` frames it is
//! dragged along by block motion measured between consecutive source frames (16-pixel-ish blocks, a full search on a
//! quarter-resolution luma copy), and the true frame bleeds back in by `bleed` each step, so the old picture smears
//! with the new motion and then dissolves, as when P-frames run on after a deleted I-frame.
use eframe::egui::{Color32, ColorImage};
use super::pm::{Img, M, pack};
use super::{BLACK, DEFAULT, Kernel, Src, Step, choice, color, f, fx_row as row, k, preset};
use crate::engine::plugins::{FxEval, GpuPort, PluginInfo};

pub static PLUGINS: &[PluginInfo] = &[
    row("milan.fx.strobe", "Strobe", "Time", "Creative", "Blinks the picture off for some frames out of every few", GpuPort::Mvfx,
        &[f("on", "Frames on", 1.0, 30.0, 2.0), f("off", "Frames off", 1.0, 30.0, 2.0), choice("mode", "Off frames", &["Color", "Transparent", "Invert"], 0), color("color", "Color", BLACK), f("offset", "Offset", 0.0, 30.0, 0.0)],
        &[DEFAULT, preset("Club Strobe", &[("on", 1.0), ("off", 1.0), ("color.r", 1.0), ("color.g", 1.0), ("color.b", 1.0)]), preset("Slow Blink", &[("on", 6.0), ("off", 3.0)]), preset("Negative Flash", &[("on", 3.0), ("off", 1.0), ("mode", 2.0)])]),
    row("milan.fx.echo", "Echo", "Time", "Creative", "Trails of earlier frames: the low-shutter smear and ghosting of the era", GpuPort::Mvfx,
        &[f("frames", "Echoes", 1.0, 16.0, 4.0), f("spacing", "Spacing (frames)", 1.0, 8.0, 1.0), f("decay", "Decay", 0.0, 1.0, 0.6), choice("mode", "Mode", &["Blend", "Lighten", "Add"], 0)],
        &[DEFAULT, preset("Step Print Smear", &[("frames", 3.0), ("decay", 0.8)]), preset("Ghost Trail", &[("frames", 8.0), ("spacing", 2.0), ("decay", 0.7), ("mode", 1.0)]), preset("Light Trails", &[("frames", 6.0), ("decay", 0.55), ("mode", 2.0)])]),
    row("milan.fx.stutter", "Stutter", "Time", "Creative", "Holds or repeats frames in a rhythm: step-printed motion and stutter edits", GpuPort::Mvfx,
        &[f("hold", "Hold (frames)", 1.0, 30.0, 3.0), choice("mode", "Mode", &["Hold", "Repeat"], 0), f("slice", "Repeat length (frames)", 1.0, 30.0, 2.0)],
        &[DEFAULT, preset("Step Print", &[("hold", 2.0)]), preset("Stutter Edit", &[("hold", 8.0), ("mode", 1.0), ("slice", 2.0)]), preset("Choppy", &[("hold", 4.0)])]),
    row("milan.fx.datamosh", "Datamosh", "Time", "Creative", "Smears a held picture along the motion of the frames that follow, then lets them bleed through", GpuPort::Mvfx,
        &[f("interval", "Keyframe interval", 2.0, 120.0, 24.0), f("length", "Smear length", 1.0, 30.0, 12.0), f("bleed", "Bleed", 0.0, 1.0, 0.08), f("block", "Block size", 0.005, 0.1, 0.02), f("search", "Motion search", 0.005, 0.08, 0.02)],
        &[DEFAULT, preset("Datamosh 2009", &[("interval", 48.0), ("length", 24.0), ("bleed", 0.04)]), preset("Quick Smear", &[("interval", 12.0), ("length", 6.0), ("bleed", 0.15)]), preset("Big Blocks", &[("block", 0.05), ("search", 0.04)])]),
];

pub const TEMPORAL: &[&str] = &["milan.fx.strobe", "milan.fx.echo", "milan.fx.stutter", "milan.fx.datamosh"];

pub static KERNELS: &[(&str, Kernel)] = &[("strobe", k_strobe), ("echo", k_echo), ("copy", k_copy), ("mosh", k_mosh)];

/// Echo, Stutter and Datamosh read earlier source frames.
pub fn reads_sources(id: &str) -> bool {
    matches!(id, "milan.fx.echo" | "milan.fx.stutter" | "milan.fx.datamosh")
}

/// Stutter's offset back from frame `n`.
fn stutter_offset(e: &FxEval, n: i64) -> i64 {
    let hold = e.f("hold").round().max(1.0) as i64;
    let m = n.rem_euclid(hold);
    if e.i("mode") == 1 { m - m % (e.f("slice").round().max(1.0) as i64) } else { m }
}

/// Datamosh: how many frames into the smear frame `n` is (0: a clean frame).
fn mosh_step(e: &FxEval, n: i64) -> i64 {
    let m = n.rem_euclid(e.f("interval").round().max(2.0) as i64);
    if m > e.f("length").round() as i64 { 0 } else { m }
}

/// The source frames (offsets back) `e` reads at its frame.
pub fn offsets(e: &FxEval) -> Vec<i64> {
    let n = super::frame_no(e);
    match e.id {
        "milan.fx.echo" => {
            let sp = e.f("spacing").round().max(1.0) as i64;
            (1..=e.f("frames").round().max(1.0) as i64).map(|i| i * sp).collect()
        }
        "milan.fx.stutter" => match stutter_offset(e, n) {
            0 => vec![],
            o => vec![o],
        },
        // The keyframe, then each frame after it up to the one before this.
        "milan.fx.datamosh" => (1..=mosh_step(e, n)).rev().collect(),
        _ => vec![],
    }
}

/// The recipe of a temporal FX without earlier frames: Strobe; the others pass the picture on.
pub fn recipe(e: &FxEval, _w: usize, _h: usize) -> Option<Vec<Step>> {
    let mut m = M::default();
    Some(match e.id {
        "milan.fx.strobe" => {
            let (on, off) = (e.f("on").round().max(1.0) as i64, e.f("off").round().max(1.0) as i64);
            let dark = (super::frame_no(e) + e.f("offset").round() as i64).rem_euclid(on + off) >= on;
            let c = e.c("color");
            m.i[0] = if dark { 1 + e.i("mode") as i32 } else { 0 };
            m.f[0] = [c[0] * c[3] * 255.0, c[1] * c[3] * 255.0, c[2] * c[3] * 255.0, c[3] * 255.0];
            vec![k("strobe", m, vec![], Src::In(0), Src::In(0))]
        }
        id if reads_sources(id) => vec![k("copy", m, vec![], Src::In(0), Src::In(0))],
        _ => return None,
    })
}

/// The recipe of Echo, Stutter or Datamosh over input 0 (the frame) and inputs 1.. (the frames `offsets` named, in
/// that order); `raw` holds the same frames as decoded, before any FX (Datamosh measures motion on them). A missing
/// frame stands in as the current one.
pub fn recipe_with(e: &FxEval, w: usize, h: usize, raw: &[Option<&ColorImage>]) -> Vec<Step> {
    let n = raw.len().saturating_sub(1);
    let mut m = M::default();
    match e.id {
        "milan.fx.echo" if n > 0 => {
            let decay = e.f("decay");
            let mode = e.i("mode") as i32;
            let mut steps = Vec::new();
            let mut total = 1.0f32;
            for i in 1..=n {
                let wgt = decay.powi(i as i32);
                total += wgt;
                m.i[0] = mode;
                m.f[0] = [wgt / total, wgt, 0.0, 0.0];
                let acc = if i == 1 { Src::In(0) } else { Src::Step(i - 2) };
                steps.push(k("echo", m, vec![], acc, Src::In(i)));
            }
            steps
        }
        "milan.fx.stutter" if n > 0 => vec![k("copy", m, vec![], Src::In(1), Src::In(1))],
        "milan.fx.datamosh" if n > 0 => {
            // Inputs 1..=n: the keyframe (n back) up to the frame before; input 0 is this frame.
            let frame = |i: usize| if i > n { Src::In(0) } else { Src::In(i) };
            let rawf = |i: usize| if i > n { raw[0] } else { raw[i] };
            let bs = ((e.f("block") * h as f32).round() as usize).max(4);
            let reach = ((e.f("search") * h as f32).round() as usize).max(4);
            m.f[0][0] = e.f("bleed");
            let mut steps = Vec::new();
            for i in 1..=n {
                let v = match (rawf(i), rawf(i + 1)) {
                    (Some(a), Some(b)) if a.size == [w, h] && b.size == [w, h] => motion(a, b, bs, reach),
                    _ => Motion { bs, bx: w.div_ceil(bs), by: h.div_ceil(bs), v: vec![(0, 0); w.div_ceil(bs) * h.div_ceil(bs)] },
                };
                m.i = [v.bs as i32, v.bx as i32, v.by as i32, 0];
                let data = v.v.iter().flat_map(|&(x, y)| [x as u32, y as u32]).collect();
                let acc = if i == 1 { Src::In(1) } else { Src::Step(i - 2) };
                steps.push(k("mosh", m, data, acc, frame(i + 1)));
            }
            steps
        }
        _ => vec![k("copy", m, vec![], Src::In(0), Src::In(0))],
    }
}

/// `event_fx`'s combination on the CPU: `img` (the frame so far) with the earlier `frames` `e` asked for.
pub fn combine(img: &mut ColorImage, e: &FxEval, frames: &[Option<std::sync::Arc<ColorImage>>], raw: &[Option<std::sync::Arc<ColorImage>>], k: usize) {
    let [w, h] = img.size;
    let raws: Vec<Option<&ColorImage>> = raw.iter().map(|r| r.as_deref()).collect();
    let steps = recipe_with(e, w, h, &raws);
    let cur = img.clone();
    let mut inputs: Vec<&ColorImage> = vec![&cur];
    inputs.extend(frames.iter().map(|f| f.as_deref().unwrap_or(&cur)));
    *img = super::run(&steps, &inputs, k);
}

/// Block motion from `a` to `b`: per block of `b`, where its content was in `a` (pixels; b(x) ~ a(x - v)).
pub struct Motion {
    pub bs: usize,
    pub bx: usize,
    pub by: usize,
    pub v: Vec<(i32, i32)>,
}

/// A full search over +-`reach` pixels on a quarter-resolution luma copy, sum of absolute differences.
pub fn motion(a: &ColorImage, b: &ColorImage, bs: usize, reach: usize) -> Motion {
    const DS: usize = 4;
    let [w, h] = a.size;
    let luma = |img: &ColorImage| -> (Vec<i32>, usize, usize) {
        let (gw, gh) = (w.div_ceil(DS), h.div_ceil(DS));
        let mut g = vec![0i32; gw * gh];
        for gy in 0..gh {
            for gx in 0..gw {
                let p = img.pixels[(gy * DS).min(h - 1) * w + (gx * DS).min(w - 1)].to_array();
                g[gy * gw + gx] = (54 * p[0] as i32 + 183 * p[1] as i32 + 19 * p[2] as i32) >> 8;
            }
        }
        (g, gw, gh)
    };
    let ((la, gw, gh), (lb, _, _)) = (luma(a), luma(b));
    let (bx, by) = (w.div_ceil(bs), h.div_ceil(bs));
    let (gbs, gr) = ((bs / DS).max(1) as i32, (reach / DS).max(1) as i32);
    let at = |l: &[i32], x: i32, y: i32| l[(y.clamp(0, gh as i32 - 1) as usize) * gw + x.clamp(0, gw as i32 - 1) as usize];
    let mut v = Vec::with_capacity(bx * by);
    for j in 0..by {
        for i in 0..bx {
            let (x0, y0) = ((i * bs / DS) as i32, (j * bs / DS) as i32);
            let sad = |dx: i32, dy: i32| -> i32 {
                let mut s = 0;
                for y in 0..gbs {
                    for x in 0..gbs {
                        s += (at(&lb, x0 + x, y0 + y) - at(&la, x0 + x - dx, y0 + y - dy)).abs();
                    }
                }
                s
            };
            let mut best: (i32, i32, i32) = (sad(0, 0), 0, 0);
            for dy in -gr..=gr {
                for dx in -gr..=gr {
                    let s = sad(dx, dy);
                    // Ties keep the smaller move, so a flat block stays still.
                    if s < best.0 || (s == best.0 && dx.abs() + dy.abs() < best.1.abs() + best.2.abs()) {
                        best = (s, dx, dy);
                    }
                }
            }
            v.push((best.1 * DS as i32, best.2 * DS as i32));
        }
    }
    Motion { bs, bx, by, v }
}

// ---- kernels (twins in gpu/mvfx_temporal.wgsl) ----

fn k_strobe(m: &M, _: &[u32], x: i32, y: i32, a: Img, _: Img) -> Color32 {
    let p = a.at(x, y);
    pack(match m.i[0] {
        1 => m.f[0],
        2 => [0.0; 4],
        3 => [p[3] - p[0], p[3] - p[1], p[3] - p[2], p[3]],
        _ => p,
    })
}

fn k_echo(m: &M, _: &[u32], x: i32, y: i32, a: Img, b: Img) -> Color32 {
    let (p, q) = (a.at(x, y), b.at(x, y));
    let [t, wgt, _, _] = m.f[0];
    pack(std::array::from_fn(|i| match m.i[0] {
        1 => p[i].max(q[i] * wgt),
        2 => p[i] + q[i] * wgt * 0.5,
        _ => p[i] + (q[i] - p[i]) * t,
    }))
}

fn k_copy(_: &M, _: &[u32], x: i32, y: i32, a: Img, _: Img) -> Color32 {
    pack(a.at(x, y))
}

fn k_mosh(m: &M, d: &[u32], x: i32, y: i32, a: Img, b: Img) -> Color32 {
    let bs = m.i[0];
    let (i, j) = ((x / bs).min(m.i[1] - 1), (y / bs).min(m.i[2] - 1));
    let at = ((j * m.i[1] + i) * 2) as usize;
    let (vx, vy) = (d[at] as i32, d[at + 1] as i32);
    let (p, q) = (a.at(x - vx, y - vy), b.at(x, y));
    let t = m.f[0][0];
    pack(std::array::from_fn(|c| p[c] + (q[c] - p[c]) * t))
}
