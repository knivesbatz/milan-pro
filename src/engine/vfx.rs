//! Video FX on premultiplied gamma-space `Color32` pictures. Spatial sizes are fractions of the picture height
//! (decision 0.7). Colour effects share one `ColorOp`; Color Grading compiles its wheels and curves into one.
use eframe::egui::{Color32, ColorImage};
use std::path::Path;
use std::sync::Arc;
use crate::engine::composite::{mul255, over, par_rows};
use crate::engine::lut::{self, Cube};
use crate::engine::plugins::{FxEval, GRADING};

/// Rec.709 luma weights.
const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];

type M3 = [[f32; 3]; 3];

/// Per pixel: unpremultiply (only when a < 255), the matrix (0..255 space), the per-channel LUT, a mix with the
/// input, then premultiply again.
pub struct ColorOp {
    pub lut: [[u8; 256]; 3],
    pub m: Option<M3>,
    pub mix: f32,
}

impl ColorOp {
    /// A LUT from `f(channel, x)` on 0..1.
    pub fn tone(f: impl Fn(usize, f32) -> f32) -> Self {
        let lut = std::array::from_fn(|c| std::array::from_fn(|i| (f(c, i as f32 / 255.0).clamp(0.0, 1.0) * 255.0).round() as u8));
        Self { lut, m: None, mix: 1.0 }
    }

    fn matrix(m: M3, mix: f32) -> Self {
        Self { m: Some(m), mix, ..Self::tone(|_, x| x) }
    }

    fn pixel(&self, p: Color32) -> Color32 {
        let [r, g, b, a] = p.to_array();
        if a == 0 {
            return p;
        }
        let orig = if a < 255 { [r, g, b].map(|v| ((v as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8) } else { [r, g, b] };
        let mut c = orig;
        if let Some(m) = &self.m {
            let f = c.map(|v| v as f32);
            c = std::array::from_fn(|i| (m[i][0] * f[0] + m[i][1] * f[1] + m[i][2] * f[2]).round().clamp(0.0, 255.0) as u8);
        }
        c = std::array::from_fn(|i| self.lut[i][c[i] as usize]);
        if self.mix < 1.0 {
            c = std::array::from_fn(|i| (orig[i] as f32 + (c[i] as f32 - orig[i] as f32) * self.mix).round() as u8);
        }
        if a < 255 {
            c = c.map(|v| mul255(v as u32, a as u32) as u8);
        }
        Color32::from_rgba_premultiplied(c[0], c[1], c[2], a)
    }
}

pub fn color_op(img: &mut ColorImage, op: &ColorOp, k: usize) {
    if op.mix <= 0.0 {
        return;
    }
    let w = img.size[0];
    par_rows(&mut img.pixels, w, k, |_, row| row.iter_mut().for_each(|p| *p = op.pixel(*p)));
}

/// Straight RGB 0..1 of a premultiplied pixel (black when transparent).
pub fn unpremul(p: Color32) -> [f32; 3] {
    let [r, g, b, a] = p.to_array();
    if a == 0 { [0.0; 3] } else { [r, g, b].map(|v| v as f32 / a as f32) }
}

/// Premultiply straight RGB 0..1 by alpha byte `a`, in gamma space like the compositor.
pub fn repremul(c: [f32; 3], a: u8) -> Color32 {
    let q = |v: f32| (v.clamp(0.0, 1.0) * a as f32).round() as u8;
    Color32::from_rgba_premultiplied(q(c[0]), q(c[1]), q(c[2]), a)
}

/// A straight RGBA 0..1 colour, premultiplied (egui's own `from_rgba_unmultiplied` works in linear light).
pub fn premul(c: [f32; 4]) -> Color32 {
    repremul([c[0], c[1], c[2]], (c[3].clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// `a` to `b` by `t`; exactly `a` at 0 and `b` at 1.
pub fn lerp_px(a: Color32, b: Color32, t: f32) -> Color32 {
    if t <= 0.0 {
        return a;
    }
    if t >= 1.0 {
        return b;
    }
    let (a, b) = (a.to_array(), b.to_array());
    let o: [u8; 4] = std::array::from_fn(|i| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).round() as u8);
    Color32::from_rgba_premultiplied(o[0], o[1], o[2], o[3])
}

/// Every channel scaled by `m`/255 (a coverage); unchanged at 255.
fn scale8(p: Color32, m: u8) -> Color32 {
    if m == 255 {
        return p;
    }
    let c = p.to_array().map(|v| mul255(v as u32, m as u32) as u8);
    Color32::from_rgba_premultiplied(c[0], c[1], c[2], c[3])
}

pub fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    if e1 <= e0 {
        return (x >= e0) as u8 as f32;
    }
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn mm(a: M3, b: M3) -> M3 {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
}

/// Luma-preserving saturation.
fn saturation(s: f32) -> M3 {
    std::array::from_fn(|i| std::array::from_fn(|j| (1.0 - s) * LUMA[j] + if i == j { s } else { 0.0 }))
}

// shortcut: hue turns in YIQ space, which is close to but not exact HSL; do per-pixel HSL if someone notices.
fn hsl(hue_deg: f32, sat: f32, lum: f32) -> Option<M3> {
    if hue_deg == 0.0 && sat == 1.0 && lum == 1.0 {
        return None;
    }
    const TO: M3 = [[0.299, 0.587, 0.114], [0.596, -0.274, -0.322], [0.211, -0.523, 0.312]];
    const FROM: M3 = [[1.0, 0.956, 0.621], [1.0, -0.272, -0.647], [1.0, -1.106, 1.703]];
    let (s, c) = hue_deg.to_radians().sin_cos();
    let rot = [[lum, 0.0, 0.0], [0.0, lum * sat * c, -lum * sat * s], [0.0, lum * sat * s, lum * sat * c]];
    Some(mm(FROM, mm(rot, TO)))
}

/// Apply plug-in `e` to `img` in place; an id this build does not know leaves it unchanged.
pub fn apply(img: &mut ColorImage, e: &FxEval, k: usize) {
    let [w, h] = img.size;
    if w == 0 || h == 0 {
        return;
    }
    let hf = h as f32;
    let px = |v: f32| (v * hf).round().max(0.0) as usize;
    match e.id {
        "milan.fx.brightness_contrast" => {
            let (b, c, mid) = (e.f("brightness"), e.f("contrast"), e.f("center"));
            let slope = if c >= 0.0 { 1.0 / (1.0 - 0.99 * c) } else { 1.0 + c };
            color_op(img, &ColorOp::tone(|_, x| (x - mid) * slope + mid + b), k);
        }
        "milan.fx.levels" => {
            let (i0, i1, g, o0, o1) = (e.f("in_start"), e.f("in_end"), e.f("gamma"), e.f("out_start"), e.f("out_end"));
            color_op(img, &ColorOp::tone(|_, x| o0 + (o1 - o0) * ((x - i0) / (i1 - i0).max(1e-6)).clamp(0.0, 1.0).powf(1.0 / g)), k);
        }
        "milan.fx.invert" => color_op(img, &ColorOp { mix: e.f("amount"), ..ColorOp::tone(|_, x| 1.0 - x) }, k),
        "milan.fx.black_white" => color_op(img, &ColorOp::matrix([LUMA; 3], e.f("blend")), k),
        "milan.fx.sepia" => {
            const SEPIA: M3 = [[0.393, 0.769, 0.189], [0.349, 0.686, 0.168], [0.272, 0.534, 0.131]];
            color_op(img, &ColorOp::matrix(SEPIA, e.f("amount")), k);
        }
        "milan.fx.hsl" => {
            if let Some(m) = hsl(e.f("hue"), e.f("saturation"), e.f("luminance")) {
                color_op(img, &ColorOp::matrix(m, 1.0), k);
            }
        }
        "milan.fx.color_corrector" => {
            let (s, g, gain, off) = (e.f("saturation"), e.f("gamma"), e.f("gain"), e.f("offset"));
            let op = ColorOp::tone(|_, x| ((x + off) * gain).clamp(0.0, 1.0).powf(1.0 / g));
            color_op(img, &ColorOp { m: (s != 1.0).then(|| saturation(s)), ..op }, k);
        }
        "milan.fx.gaussian_blur" => blur(img, px(e.f("h_range")), px(e.f("v_range")), k),
        "milan.fx.sharpen" => sharpen(img, e.f("amount"), px(e.f("radius")), e.f("threshold"), k),
        "milan.fx.vignette" => vignette(img, e, k),
        "milan.fx.mirror" => mirror(img, e.i("direction"), e.f("position"), k),
        "milan.fx.crop" => crop(img, e, k),
        "milan.fx.border" => {
            let (s, c) = (e.f("size") * hf, premul(e.c("color")));
            par_rows(&mut img.pixels, w, k, |y, row| {
                let dy = (y as f32 + 0.5).min(h as f32 - y as f32 - 0.5);
                for (x, p) in row.iter_mut().enumerate() {
                    if dy.min(x as f32 + 0.5).min(w as f32 - x as f32 - 0.5) < s {
                        *p = over(*p, c, 255);
                    }
                }
            });
        }
        "milan.fx.chroma_key" => chroma_key(img, e, px(e.f("blur")), k),
        GRADING => Grade::new(e).apply(img, k),
        _ => {}
    }
}

/// One horizontal box pass of radius `r` from `src` into `dst`, clamped at the edges.
fn box_row(src: &[Color32], dst: &mut [Color32], r: usize) {
    let n = src.len() as isize;
    let at = |i: isize| src[i.clamp(0, n - 1) as usize].to_array();
    let (r, d) = (r as isize, 2 * r as u32 + 1);
    let mut sum = [0u32; 4];
    for i in -r..=r {
        let p = at(i);
        (0..4).for_each(|c| sum[c] += p[c] as u32);
    }
    for (x, out) in dst.iter_mut().enumerate() {
        let o = sum.map(|s| ((s + d / 2) / d) as u8);
        *out = Color32::from_rgba_premultiplied(o[0], o[1], o[2], o[3]);
        let (add, sub) = (at(x as isize + r + 1), at(x as isize - r));
        (0..4).for_each(|c| sum[c] = sum[c] + add[c] as u32 - sub[c] as u32);
    }
}

fn box3_rows(img: &mut ColorImage, r: usize, k: usize) {
    let w = img.size[0];
    par_rows(&mut img.pixels, w, k, |_, row| {
        let mut tmp = row.to_vec();
        for _ in 0..3 {
            box_row(&tmp, row, r);
            tmp.copy_from_slice(row);
        }
    });
}

pub(crate) fn transpose(img: &ColorImage) -> ColorImage {
    let [w, h] = img.size;
    ColorImage { size: [h, w], pixels: (0..w * h).map(|i| img.pixels[(i % h) * w + i / h]).collect() }
}

/// Three box passes per axis, about a Gaussian with sigma `rx` / `ry` pixels. Radius 0 leaves an axis alone.
pub fn blur(img: &mut ColorImage, rx: usize, ry: usize, k: usize) {
    if rx > 0 {
        box3_rows(img, rx, k);
    }
    if ry > 0 {
        let mut t = transpose(img);
        box3_rows(&mut t, ry, k);
        *img = transpose(&t);
    }
}

/// Unsharp mask: push each channel away from its blur where they differ by more than `threshold`.
fn sharpen(img: &mut ColorImage, amount: f32, r: usize, threshold: f32, k: usize) {
    if r == 0 || amount <= 0.0 {
        return;
    }
    let mut b = img.clone();
    blur(&mut b, r, r, k);
    let (w, thr) = (img.size[0], threshold * 255.0);
    par_rows(&mut img.pixels, w, k, |y, row| {
        for (p, bl) in row.iter_mut().zip(&b.pixels[y * w..]) {
            let (s, bl) = (p.to_array(), bl.to_array());
            let o: [u8; 3] = std::array::from_fn(|c| {
                let d = s[c] as f32 - bl[c] as f32;
                if d.abs() <= thr { s[c] } else { (s[c] as f32 + amount * d).round().clamp(0.0, s[3] as f32) as u8 }
            });
            *p = Color32::from_rgba_premultiplied(o[0], o[1], o[2], s[3]);
        }
    });
}

fn vignette(img: &mut ColorImage, e: &FxEval, k: usize) {
    let [w, h] = img.size;
    let (amount, size, soft, c) = (e.f("amount"), e.f("size"), e.f("softness"), premul(e.c("color")));
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    par_rows(&mut img.pixels, w, k, |y, row| {
        let dy = (y as f32 + 0.5 - cy) / cy;
        for (x, p) in row.iter_mut().enumerate() {
            let dx = (x as f32 + 0.5 - cx) / cx;
            // 0 at the centre, 1 in the corners.
            let r = (dx * dx + dy * dy).sqrt() / std::f32::consts::SQRT_2;
            *p = lerp_px(*p, c, amount * smoothstep(size - soft / 2.0, size + soft / 2.0, r));
        }
    });
}

/// Mirror about `pos` (0..1 of the width or height): direction 0 keeps the left half, 1 the right, 2 the top, 3 the bottom.
fn mirror(img: &mut ColorImage, dir: usize, pos: f32, k: usize) {
    let [w, h] = img.size;
    let src = img.pixels.clone();
    let n = if dir < 2 { w } else { h } as isize;
    let axis = (pos.clamp(0.0, 1.0) * n as f32).round() as isize;
    let keep_low = dir % 2 == 0;
    let map = |i: isize| if (i >= axis) == keep_low { 2 * axis - 1 - i } else { i };
    par_rows(&mut img.pixels, w, k, |y, row| {
        for (x, p) in row.iter_mut().enumerate() {
            let (sx, sy) = if dir < 2 { (map(x as isize), y as isize) } else { (x as isize, map(y as isize)) };
            *p = if (0..w as isize).contains(&sx) && (0..h as isize).contains(&sy) { src[sy as usize * w + sx as usize] } else { Color32::TRANSPARENT };
        }
    });
}

fn crop(img: &mut ColorImage, e: &FxEval, k: usize) {
    let [w, h] = img.size;
    let (wf, hf) = (w as f32, h as f32);
    let f = e.f("feather") * hf;
    let (l, t, r, b) = (e.f("left") * wf, e.f("top") * hf, (1.0 - e.f("right")) * wf, (1.0 - e.f("bottom")) * hf);
    // Coverage of a pixel centre `d` px inside an edge.
    let edge = |d: f32| if f <= 0.0 { (d > 0.0) as u8 as f32 } else { (d / f).clamp(0.0, 1.0) };
    par_rows(&mut img.pixels, w, k, |y, row| {
        let yc = y as f32 + 0.5;
        let cy = edge(yc - t) * edge(b - yc);
        for (x, p) in row.iter_mut().enumerate() {
            let xc = x as f32 + 0.5;
            *p = scale8(*p, (cy * edge(xc - l) * edge(r - xc) * 255.0).round() as u8);
        }
    });
}

/// BT.601 chroma of straight RGB 0..1.
fn cbcr(c: [f32; 3]) -> (f32, f32) {
    (-0.168736 * c[0] - 0.331264 * c[1] + 0.5 * c[2], 0.5 * c[0] - 0.418688 * c[1] - 0.081312 * c[2])
}

/// Alpha from the chroma distance to the key colour: 0 below `low`, 1 above `high`, smooth between. No spill suppression.
fn chroma_key(img: &mut ColorImage, e: &FxEval, r: usize, k: usize) {
    let [w, _] = img.size;
    let key = e.c("color");
    let (kb, kr) = cbcr([key[0], key[1], key[2]]);
    let (lo, hi) = (e.f("low"), e.f("high"));
    let mut mask = img.clone();
    par_rows(&mut mask.pixels, w, k, |_, row| {
        for p in row.iter_mut() {
            let (b, r) = cbcr(unpremul(*p));
            let m = (smoothstep(lo, hi, (b - kb).hypot(r - kr)) * 255.0).round() as u8;
            *p = Color32::from_rgba_premultiplied(m, m, m, m);
        }
    });
    blur(&mut mask, r, r, k);
    let mask_only = e.on("mask_only");
    par_rows(&mut img.pixels, w, k, |y, row| {
        for (p, m) in row.iter_mut().zip(&mask.pixels[y * w..]) {
            *p = if mask_only { Color32::from_gray(m.a()) } else { scale8(*p, m.a()) };
        }
    });
}

/// "x:y;x:y" control points, sorted by x, each clamped to 0..1.
pub fn parse_points(s: &str) -> Vec<(f32, f32)> {
    let mut v: Vec<(f32, f32)> = s
        .split(';')
        .filter_map(|p| {
            let (x, y) = p.split_once(':')?;
            Some((x.trim().parse::<f32>().ok()?.clamp(0.0, 1.0), y.trim().parse::<f32>().ok()?.clamp(0.0, 1.0)))
        })
        .collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    v.dedup_by(|a, b| a.0 == b.0);
    v
}

/// Monotone cubic (Fritsch-Carlson) through `pts`, flat outside them; no points = the identity.
pub fn monotone(pts: &[(f32, f32)], x: f32) -> f32 {
    let n = pts.len();
    match n {
        0 => return x,
        1 => return pts[0].1,
        _ => {}
    }
    if x <= pts[0].0 {
        return pts[0].1;
    }
    if x >= pts[n - 1].0 {
        return pts[n - 1].1;
    }
    let d: Vec<f32> = pts.windows(2).map(|w| (w[1].1 - w[0].1) / (w[1].0 - w[0].0)).collect();
    let mut m: Vec<f32> = (0..n).map(|i| if i == 0 { d[0] } else if i == n - 1 { d[n - 2] } else if d[i - 1] * d[i] <= 0.0 { 0.0 } else { (d[i - 1] + d[i]) / 2.0 }).collect();
    for i in 0..n - 1 {
        if d[i] == 0.0 {
            (m[i], m[i + 1]) = (0.0, 0.0);
        } else {
            let (a, b) = (m[i] / d[i], m[i + 1] / d[i]);
            let s = a * a + b * b;
            if s > 9.0 {
                let t = 3.0 / s.sqrt();
                (m[i], m[i + 1]) = (t * a * d[i], t * b * d[i]);
            }
        }
    }
    let i = pts.partition_point(|p| p.0 <= x) - 1;
    let ((x0, y0), (x1, y1)) = (pts[i], pts[i + 1]);
    let hgt = x1 - x0;
    let t = (x - x0) / hgt;
    let (t2, t3) = (t * t, t * t * t);
    (2.0 * t3 - 3.0 * t2 + 1.0) * y0 + (t3 - 2.0 * t2 + t) * hgt * m[i] + (-2.0 * t3 + 3.0 * t2) * y1 + (t3 - t2) * hgt * m[i + 1]
}

/// A LUT applied to every pixel (straight colour), alpha kept.
fn cube_img(img: &mut ColorImage, lut: &Cube, k: usize) {
    let w = img.size[0];
    par_rows(&mut img.pixels, w, k, |_, row| {
        for p in row.iter_mut().filter(|p| p.a() > 0) {
            *p = repremul(lut.apply(unpremul(*p)), p.a());
        }
    });
}

/// `milan.color_grading` compiled: input LUT, offset, lift, gain, gamma, curves, then saturation / temperature / tint,
/// then the look LUT. A LUT that fails to load is skipped (the panel shows `lut::load`'s error).
pub struct Grade {
    offset: [f32; 3],
    lift: [f32; 3],
    gain: [f32; 3],
    gamma: [f32; 3],
    master: Vec<(f32, f32)>,
    curves: [Vec<(f32, f32)>; 3],
    m: Option<M3>,
    input: Option<Arc<Cube>>,
    look: Option<Arc<Cube>>,
}

impl Grade {
    pub fn new(e: &FxEval) -> Self {
        // A wheel is R, G, B plus Y on every channel (added, or multiplied for gain and gamma).
        let wheel = |n: &str, add: bool| -> [f32; 3] {
            let y = e.f(&format!("{n}.y"));
            std::array::from_fn(|i| {
                let v = e.f(&format!("{n}.{}", ["r", "g", "b"][i]));
                if add { v + y } else { v * y }
            })
        };
        let lut = |k: &str| Some(e.t(k)).filter(|p| !p.trim().is_empty()).and_then(|p| lut::load(Path::new(p.trim())).ok());
        let (s, temp, tint) = (e.f("saturation"), e.f("temperature"), e.f("tint"));
        let m = (s != 1.0 || temp != 0.0 || tint != 0.0).then(|| {
            let d = [1.0 + 0.2 * temp, 1.0 - 0.2 * tint, 1.0 - 0.2 * temp];
            let sat = saturation(s);
            std::array::from_fn(|i| sat[i].map(|v| v * d[i]))
        });
        Self {
            offset: wheel("offset", true),
            lift: wheel("lift", true),
            gain: wheel("gain", false),
            gamma: wheel("gamma", false),
            master: parse_points(e.t("curve.m")),
            curves: ["curve.r", "curve.g", "curve.b"].map(|k| parse_points(e.t(k))),
            m,
            input: lut("input_lut"),
            look: lut("look_lut"),
        }
    }

    /// Steps 2-6 on one channel.
    fn tone(&self, c: usize, x: f32) -> f32 {
        let x = x + self.offset[c];
        let x = x + self.lift[c] * (1.0 - x);
        let x = (x * self.gain[c]).clamp(0.0, 1.0).powf(1.0 / self.gamma[c].max(1e-3));
        monotone(&self.curves[c], monotone(&self.master, x))
    }

    /// The whole grade on one straight colour, in float (for LUT export).
    pub fn rgb(&self, c: [f32; 3]) -> [f32; 3] {
        let c = self.input.as_ref().map_or(c, |l| l.apply(c));
        let c: [f32; 3] = std::array::from_fn(|i| self.tone(i, c[i]).clamp(0.0, 1.0));
        let c = self.m.map_or(c, |m| std::array::from_fn(|i| (m[i][0] * c[0] + m[i][1] * c[1] + m[i][2] * c[2]).clamp(0.0, 1.0)));
        self.look.as_ref().map_or(c, |l| l.apply(c))
    }

    fn apply(&self, img: &mut ColorImage, k: usize) {
        if let Some(l) = &self.input {
            cube_img(img, l, k);
        }
        color_op(img, &ColorOp::tone(|c, x| self.tone(c, x)), k);
        if let Some(m) = self.m {
            color_op(img, &ColorOp::matrix(m, 1.0), k);
        }
        if let Some(l) = &self.look {
            cube_img(img, l, k);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::plugins::{find, new_fx, preset_eval, eval, PluginKind, catalog};
    use crate::project::{Curve, Keyframe, Keyframes};

    /// A busy test picture: every 8-bit level in red, gradients in green and blue, some half-transparent rows.
    fn picture(w: usize, h: usize) -> ColorImage {
        ColorImage {
            size: [w, h],
            pixels: (0..w * h)
                .map(|i| {
                    let (x, y) = (i % w, i / w);
                    let a = if y % 7 == 3 { 128 } else { 255 };
                    let c = [(i % 256) as u32, (x * 255 / w) as u32, (y * 255 / h) as u32].map(|v| mul255(v, a) as u8);
                    Color32::from_rgba_premultiplied(c[0], c[1], c[2], a as u8)
                })
                .collect(),
        }
    }

    fn fx(id: &str, vals: &[(&str, f32)]) -> FxEval {
        let mut f = new_fx(find(id).unwrap(), None);
        for (k, v) in vals {
            f.params.insert(k.to_string(), Keyframes { keys: vec![Keyframe { t: 0.0, v: *v, curve: Curve::Linear }] });
        }
        eval(&f, 0.0).unwrap()
    }

    fn with_text(mut e: FxEval, key: &str, v: &str) -> FxEval {
        for (k, val) in &mut e.vals {
            if *k == key {
                *val = crate::engine::plugins::Val::T(v.into());
            }
        }
        e
    }

    fn run(id: &str, vals: &[(&str, f32)], img: &ColorImage) -> ColorImage {
        let mut out = img.clone();
        apply(&mut out, &fx(id, vals), 1);
        out
    }

    #[test]
    fn identity_parameters_are_a_no_op() {
        let img = picture(64, 40);
        // Bit-exact for the LUT effects.
        for (id, vals) in [
            ("milan.fx.brightness_contrast", &[][..]),
            ("milan.fx.levels", &[]),
            ("milan.fx.invert", &[("amount", 0.0)]),
            ("milan.fx.black_white", &[("blend", 0.0)]),
            ("milan.fx.sepia", &[("amount", 0.0)]),
            ("milan.fx.hsl", &[]),
            ("milan.fx.color_corrector", &[]),
            ("milan.fx.gaussian_blur", &[("h_range", 0.0), ("v_range", 0.0)]),
            ("milan.fx.sharpen", &[("amount", 0.0)]),
            ("milan.fx.vignette", &[("amount", 0.0)]),
            ("milan.fx.crop", &[]),
            ("milan.fx.border", &[("size", 0.0)]),
            (GRADING, &[]),
        ] {
            assert!(run(id, vals, &img).pixels == img.pixels, "{id}");
        }
        // Mirror at the far edge keeps everything.
        assert!(run("milan.fx.mirror", &[("position", 1.0)], &img).pixels == img.pixels);
    }

    #[test]
    fn invert_twice_and_black_and_white() {
        let img = ColorImage { size: [256, 1], pixels: (0..256).map(|i| Color32::from_rgb(i as u8, 255 - i as u8, (i * 7 % 256) as u8)).collect() };
        let twice = run("milan.fx.invert", &[], &run("milan.fx.invert", &[], &img));
        assert!(twice.pixels == img.pixels);
        assert_eq!(run("milan.fx.invert", &[], &img).pixels[10].to_array(), [245, 10, 185, 255]);
        for p in run("milan.fx.black_white", &[], &picture(32, 20)).pixels {
            assert!(p.r() == p.g() && p.g() == p.b(), "{p:?}");
        }
    }

    #[test]
    fn color_op_on_half_transparent_matches_f64() {
        let op = ColorOp::tone(|_, x| x + 0.2);
        for a in [60u8, 128, 200] {
            for v in 0..=a {
                let got = op.pixel(Color32::from_rgba_premultiplied(v, v, v, a)).r() as f64;
                let want = ((v as f64 / a as f64 + 0.2).min(1.0)) * a as f64;
                assert!((got - want).abs() <= 1.0, "a={a} v={v}: {got} vs {want}");
            }
        }
    }

    #[test]
    fn blur_identity_constant_and_energy() {
        let img = picture(50, 30);
        let mut b = img.clone();
        blur(&mut b, 0, 0, 1);
        assert!(b.pixels == img.pixels);
        let mut flat = ColorImage::new([40, 30], Color32::from_rgb(90, 140, 200));
        let before = flat.pixels.clone();
        blur(&mut flat, 4, 6, 1);
        assert!(flat.pixels == before);
        // A blob in the middle keeps its total within 1%.
        let mut blob = ColorImage::new([80, 80], Color32::TRANSPARENT);
        for y in 30..50 {
            for x in 30..50 {
                blob.pixels[y * 80 + x] = Color32::WHITE;
            }
        }
        let sum = |i: &ColorImage| i.pixels.iter().map(|p| p.a() as f64).sum::<f64>();
        let s0 = sum(&blob);
        blur(&mut blob, 3, 3, 1);
        assert!((sum(&blob) - s0).abs() / s0 < 0.01);
        assert!(blob.pixels[40 * 80 + 25].a() > 0 && blob.pixels[40 * 80 + 40].a() == 255);
    }

    #[test]
    fn crop_mirror_and_chroma_key() {
        let img = ColorImage::new([40, 20], Color32::from_rgb(200, 100, 50));
        let c = run("milan.fx.crop", &[("left", 0.25), ("bottom", 0.5)], &img);
        for y in 0..20 {
            for x in 0..40 {
                let inside = x >= 10 && y < 10;
                assert_eq!(c.pixels[y * 40 + x], if inside { img.pixels[0] } else { Color32::TRANSPARENT }, "{x},{y}");
            }
        }
        let pic = picture(40, 20);
        let m = run("milan.fx.mirror", &[("position", 0.4)], &pic); // axis at x = 16
        for y in 0..20 {
            for d in 0..16 {
                assert_eq!(m.pixels[y * 40 + 15 - d], m.pixels[y * 40 + 16 + d]);
                assert_eq!(m.pixels[y * 40 + 15 - d], pic.pixels[y * 40 + 15 - d]);
            }
        }
        let key = Color32::from_rgb(0, 0, 255); // the default key colour, as in VEGAS
        let img = ColorImage { size: [2, 1], pixels: vec![key, Color32::from_rgb(220, 40, 160)] };
        let k = run("milan.fx.chroma_key", &[], &img);
        assert_eq!((k.pixels[0].a(), k.pixels[1].a()), (0, 255));
        assert_eq!(k.pixels[1], img.pixels[1]);
        let mask = run("milan.fx.chroma_key", &[("mask_only", 1.0)], &img);
        assert_eq!((mask.pixels[0], mask.pixels[1]), (Color32::BLACK, Color32::WHITE));
    }

    #[test]
    fn striped_equals_single_thread() {
        let img = picture(300, 240);
        for info in catalog(PluginKind::VideoFx) {
            for pr in info.presets {
                let e = preset_eval(info, Some(pr));
                let (mut one, mut many) = (img.clone(), img.clone());
                apply(&mut one, &e, 1);
                apply(&mut many, &e, 5);
                assert!(one.pixels == many.pixels, "{} {}", info.id, pr.name);
            }
        }
    }

    #[test]
    fn grading_maths() {
        let grey = |v: u8| ColorImage::new([1, 1], Color32::from_gray(v));
        let g = |vals: &[(&str, f32)], v: u8| run(GRADING, vals, &grey(v)).pixels[0].r();
        assert_eq!(g(&[("gain.y", 2.0)], 100), 200);
        assert_eq!(g(&[("lift.y", 0.1)], 0), 26); // 0.1 x 255
        assert_eq!(g(&[("lift.y", 0.1)], 255), 255);
        assert_eq!(g(&[("lift.r", 0.1)], 0), 26);
        for v in [30u8, 128, 220] {
            let want = ((v as f64 / 255.0).powf(1.0 / 2.2) * 255.0).round() as i32;
            assert!((g(&[("gamma.y", 2.2)], v) as i32 - want).abs() <= 1);
        }
        // Saturation 0 makes grey; a curve passes through its points.
        let red = ColorImage::new([1, 1], Color32::from_rgb(200, 40, 40));
        let p = run(GRADING, &[("saturation", 0.0)], &red).pixels[0];
        assert!(p.r() == p.g() && p.g() == p.b());
        let pts = parse_points("1:1; 0:0;0.25:0.6;0.5:0.7");
        assert_eq!(pts[0], (0.0, 0.0));
        for &(x, y) in &pts {
            assert!((monotone(&pts, x) - y).abs() < 1e-6);
        }
        let ys: Vec<f32> = (0..=100).map(|i| monotone(&pts, i as f32 / 100.0)).collect();
        assert!(ys.windows(2).all(|w| w[1] >= w[0] - 1e-6), "monotone");
        let curved = with_text(fx(GRADING, &[]), "curve.m", "0:0;0.5:0.75;1:1");
        let mut img = grey(128);
        apply(&mut img, &curved, 1);
        assert!((img.pixels[0].r() as i32 - 191).abs() <= 1);
    }
}
