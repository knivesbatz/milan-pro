//! CPU compositor: premultiplied 8-bit `Color32` layers, bottom to top, row stripes on scoped threads.
use eframe::egui::{Color32, ColorImage};
use std::sync::Arc;
use crate::engine::plan::{EventPlan, FramePlan, Item, SrcAt, TrackPlan};
use crate::engine::plugins::FxEval;
use crate::engine::{raster, transition, vfx};
use crate::project::{Composite, Halo};

/// x' = a x + b y + tx, y' = c x + d y + ty.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub tx: f64,
    pub ty: f64,
}

impl Affine {
    pub fn translate(x: f64, y: f64) -> Self {
        Self { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: x, ty: y }
    }

    pub fn scale(sx: f64, sy: f64) -> Self {
        Self { a: sx, b: 0.0, c: 0.0, d: sy, tx: 0.0, ty: 0.0 }
    }

    /// Clockwise on screen (y points down): 90 degrees maps (1, 0) to (0, 1).
    pub fn rotate(deg: f64) -> Self {
        let (s, c) = deg.to_radians().sin_cos();
        Self { a: c, b: -s, c: s, d: c, tx: 0.0, ty: 0.0 }
    }

    /// `self` first, then `o`.
    pub fn then(self, o: Affine) -> Affine {
        Affine {
            a: o.a * self.a + o.b * self.c,
            b: o.a * self.b + o.b * self.d,
            c: o.c * self.a + o.d * self.c,
            d: o.c * self.b + o.d * self.d,
            tx: o.a * self.tx + o.b * self.ty + o.tx,
            ty: o.c * self.tx + o.d * self.ty + o.ty,
        }
    }

    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (self.a * x + self.b * y + self.tx, self.c * x + self.d * y + self.ty)
    }

    pub fn inverse(&self) -> Option<Affine> {
        let det = self.a * self.d - self.b * self.c;
        if det.abs() < 1e-12 {
            return None;
        }
        let (a, b, c, d) = (self.d / det, -self.b / det, -self.c / det, self.a / det);
        Some(Affine { a, b, c, d, tx: -(a * self.tx + b * self.ty), ty: -(c * self.tx + d * self.ty) })
    }
}

/// Decoded pictures for the compositor; a None draws nothing (the implementation records why).
pub trait Sources {
    fn get(&mut self, s: &SrcAt, w: u32, h: u32) -> Option<Arc<ColorImage>>;
}

/// round(x * y / 255) for x, y in 0..=255.
#[inline]
pub fn mul255(x: u32, y: u32) -> u32 {
    let t = x * y + 128;
    (t + (t >> 8)) >> 8
}

/// Row stripes per frame: half the cores, 1..=6.
pub fn stripes() -> usize {
    (std::thread::available_parallelism().map_or(1, |n| n.get()) / 2).clamp(1, 6)
}

/// Run `f(y, row)` over the rows of `px` (`w` wide), on up to `k` scoped threads.
pub(crate) fn par_rows(px: &mut [Color32], w: usize, k: usize, f: impl Fn(usize, &mut [Color32]) + Sync) {
    let rows = px.len() / w.max(1);
    if k <= 1 || rows < 2 * k || px.len() < 32_768 {
        px.chunks_mut(w).enumerate().for_each(|(y, r)| f(y, r));
        return;
    }
    let per = rows.div_ceil(k);
    let f = &f;
    std::thread::scope(|s| {
        for (i, chunk) in px.chunks_mut(per * w).enumerate() {
            s.spawn(move || chunk.chunks_mut(w).enumerate().for_each(|(j, r)| f(i * per + j, r)));
        }
    });
}

/// Premultiplied source-over of `s` scaled by `a8` onto `d`.
#[inline]
pub(crate) fn over(d: Color32, s: Color32, a8: u32) -> Color32 {
    let s = if a8 == 255 { s.to_array() } else { s.to_array().map(|c| mul255(c as u32, a8) as u8) };
    if s[3] == 255 {
        return Color32::from_rgba_premultiplied(s[0], s[1], s[2], 255);
    }
    let (d, inv) = (d.to_array(), 255 - s[3] as u32);
    let o: [u8; 4] = std::array::from_fn(|i| (s[i] as u32 + mul255(d[i] as u32, inv)).min(255) as u8);
    Color32::from_rgba_premultiplied(o[0], o[1], o[2], o[3])
}

/// Bilinear sample at 16.16 position (u, v) (pixel centres at integers), clamped to the edge pixels.
#[inline]
fn bilerp(src: &ColorImage, u: i64, v: i64) -> Color32 {
    let [sw, sh] = src.size;
    let (iu, iv) = (u >> 16, v >> 16);
    let (fu, fv) = (((u >> 8) & 255) as u32, ((v >> 8) & 255) as u32);
    let (x0, x1) = (iu.clamp(0, sw as i64 - 1) as usize, (iu + 1).clamp(0, sw as i64 - 1) as usize);
    let (y0, y1) = (iv.clamp(0, sh as i64 - 1) as usize, (iv + 1).clamp(0, sh as i64 - 1) as usize);
    let p = |x: usize, y: usize| src.pixels[y * sw + x].to_array();
    let (p00, p10, p01, p11) = (p(x0, y0), p(x1, y0), p(x0, y1), p(x1, y1));
    let o: [u8; 4] = std::array::from_fn(|i| {
        let top = p00[i] as u32 * (256 - fu) + p10[i] as u32 * fu;
        let bot = p01[i] as u32 * (256 - fu) + p11[i] as u32 * fu;
        ((top * (256 - fv) + bot * fv + 32768) >> 16) as u8
    });
    Color32::from_rgba_premultiplied(o[0], o[1], o[2], o[3])
}

/// The whole-pixel offset when `m` is a scale-1 translation (within float noise), else None.
fn whole_pixel(m: Affine) -> Option<(i64, i64)> {
    let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
    (near(m.a, 1.0) && near(m.d, 1.0) && near(m.b, 0.0) && near(m.c, 0.0) && near(m.tx, m.tx.round()) && near(m.ty, m.ty.round()))
        .then(|| (m.tx.round() as i64, m.ty.round() as i64))
}

/// Resampling: `a` becomes (1 - w) a + w b, per premultiplied channel (pictures of one size).
pub fn blend_frames(a: &mut ColorImage, b: &ColorImage, w: f32, k: usize) {
    if a.size != b.size {
        return;
    }
    let (w8, cols) = ((w.clamp(0.0, 1.0) * 256.0).round() as u32, a.size[0]);
    par_rows(&mut a.pixels, cols, k, |y, row| {
        for (p, q) in row.iter_mut().zip(&b.pixels[y * cols..(y + 1) * cols]) {
            let (x, z) = (p.to_array(), q.to_array());
            let o: [u8; 4] = std::array::from_fn(|i| ((x[i] as u32 * (256 - w8) + z[i] as u32 * w8 + 128) >> 8) as u8);
            *p = Color32::from_rgba_premultiplied(o[0], o[1], o[2], o[3]);
        }
    });
}

/// Reduce Interlace Flicker: each row becomes (above + 2 x row + below) / 4, the edge rows repeated.
pub fn reduce_flicker(img: &mut ColorImage, k: usize) {
    let [w, h] = img.size;
    if h < 2 {
        return;
    }
    let src = img.pixels.clone();
    par_rows(&mut img.pixels, w, k, |y, row| {
        let line = |y: usize| &src[y * w..(y + 1) * w];
        let (up, mid, down) = (line(y.saturating_sub(1)), line(y), line((y + 1).min(h - 1)));
        for (x, p) in row.iter_mut().enumerate() {
            let (u, m, d) = (up[x].to_array(), mid[x].to_array(), down[x].to_array());
            let o: [u8; 4] = std::array::from_fn(|i| ((u[i] as u32 + 2 * m[i] as u32 + d[i] as u32 + 2) >> 2) as u8);
            *p = Color32::from_rgba_premultiplied(o[0], o[1], o[2], o[3]);
        }
    });
}

/// Draw `src` through `m` (source pixel edges -> destination pixel edges) at `alpha` onto `dst`, source-over.
/// Outside the source rectangle nothing is drawn, so letterbox bars stay as they were.
pub fn draw(dst: &mut ColorImage, src: &ColorImage, m: Affine, alpha: f32, bilinear: bool, k: usize) {
    let a8 = (alpha.clamp(0.0, 1.0) * 255.0).round() as u32;
    let Some(inv) = m.inverse() else { return };
    let ([dw, dh], [sw, sh]) = (dst.size, src.size);
    if a8 == 0 || sw == 0 || sh == 0 {
        return;
    }
    let pts = [(0.0, 0.0), (sw as f64, 0.0), (0.0, sh as f64), (sw as f64, sh as f64)].map(|(x, y)| m.apply(x, y));
    let lo = |f: fn(&(f64, f64)) -> f64, max: usize| (pts.iter().map(f).fold(f64::INFINITY, f64::min).floor().max(0.0) as usize).min(max);
    let hi = |f: fn(&(f64, f64)) -> f64, max: usize| (pts.iter().map(f).fold(f64::NEG_INFINITY, f64::max).ceil().max(0.0) as usize).min(max);
    let (x0, x1, y0, y1) = (lo(|p| p.0, dw), hi(|p| p.0, dw), lo(|p| p.1, dh), hi(|p| p.1, dh));
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    if let Some((ox, oy)) = whole_pixel(m) {
        // Fast path: the source at its own size on the pixel grid (a full-frame event decoded at the preview size).
        let span = |o: i64, s: usize, d: usize| (o.clamp(0, d as i64) as usize, (o + s as i64).clamp(0, d as i64) as usize);
        let ((x0, x1), (y0, y1)) = (span(ox, sw, dw), span(oy, sh, dh));
        if x0 < x1 && y0 < y1 {
            par_rows(&mut dst.pixels[y0 * dw..y1 * dw], dw, k, |yr, row| {
                let srow = &src.pixels[((y0 + yr) as i64 - oy) as usize * sw..][..sw];
                for (x, px) in row.iter_mut().enumerate().take(x1).skip(x0) {
                    *px = over(*px, srow[(x as i64 - ox) as usize], a8);
                }
            });
        }
        return;
    }
    const ONE: f64 = 65536.0;
    let (du, dv) = ((inv.a * ONE).round() as i64, (inv.c * ONE).round() as i64);
    let (umax, vmax) = ((sw as i64) << 16, (sh as i64) << 16);
    par_rows(&mut dst.pixels[y0 * dw..y1 * dw], dw, k, |yr, row| {
        let (u0, v0) = inv.apply(x0 as f64 + 0.5, (y0 + yr) as f64 + 0.5);
        let (mut u, mut v) = ((u0 * ONE).floor() as i64, (v0 * ONE).floor() as i64);
        for px in &mut row[x0..x1] {
            if u >= 0 && v >= 0 && u < umax && v < vmax {
                let s = if bilinear { bilerp(src, u - 32768, v - 32768) } else { src.pixels[(v >> 16) as usize * sw + (u >> 16) as usize] };
                *px = over(*px, s, a8);
            }
            u += du;
            v += dv;
        }
    });
}

/// B(backdrop, source) for one channel of an opaque source, in 0..=255: the modes kept in integer maths for speed.
fn blend_channel(mode: Composite, cd: u32, cs: u32) -> Option<u32> {
    Some(match mode {
        Composite::Add => (cd + cs).min(255),
        Composite::Subtract => cd.saturating_sub(cs),
        Composite::Multiply => mul255(cs, cd),
        Composite::Screen => cs + cd - mul255(cs, cd),
        Composite::Overlay if cd < 128 => mul255(2 * cs, cd),
        Composite::Overlay => 255 - mul255(2 * (255 - cs), 255 - cd),
        Composite::Darken => cs.min(cd),
        Composite::Lighten => cs.max(cd),
        Composite::Difference => cs.abs_diff(cd),
        _ => return None,
    })
}

/// B(backdrop, source) of a separable mode on one channel 0..=1 (W3C Compositing and Blending; VEGAS's Difference
/// Squared is the difference squared, Hard Mix Photoshop's).
fn separable(mode: Composite, b: f32, s: f32) -> f32 {
    use Composite::*;
    match mode {
        Add => (b + s).min(1.0),
        Subtract => (b - s).max(0.0),
        Multiply => b * s,
        Screen => b + s - b * s,
        Overlay => separable(HardLight, s, b),
        Darken => b.min(s),
        Lighten => b.max(s),
        Difference => (b - s).abs(),
        HardLight if s <= 0.5 => 2.0 * b * s,
        HardLight => 1.0 - 2.0 * (1.0 - b) * (1.0 - s),
        Dodge if b <= 0.0 => 0.0,
        Dodge if s >= 1.0 => 1.0,
        Dodge => (b / (1.0 - s)).min(1.0),
        Burn if b >= 1.0 => 1.0,
        Burn if s <= 0.0 => 0.0,
        Burn => 1.0 - ((1.0 - b) / s).min(1.0),
        DifferenceSquared => (b - s) * (b - s),
        SoftLight if s <= 0.5 => b - (1.0 - 2.0 * s) * b * (1.0 - b),
        SoftLight => {
            let d = if b <= 0.25 { ((16.0 * b - 12.0) * b + 4.0) * b } else { b.sqrt() };
            b + (2.0 * s - 1.0) * (d - b)
        }
        Exclusion => b + s - 2.0 * b * s,
        HardMix => (b + s >= 1.0) as u8 as f32,
        _ => s,
    }
}

fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

/// W3C SetLum (with ClipColor).
fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    let c = c.map(|v| v + d);
    let l = lum(c);
    let (n, x) = (c[0].min(c[1]).min(c[2]), c[0].max(c[1]).max(c[2]));
    c.map(|v| if n < 0.0 { l + (v - l) * l / (l - n) } else { v }).map(|v| if x > 1.0 { l + (v - l) * (1.0 - l) / (x - l) } else { v })
}

fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

/// W3C SetSat.
fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let mut i = [0, 1, 2];
    i.sort_by(|&a, &b| c[a].total_cmp(&c[b]));
    let (lo, mid, hi) = (c[i[0]], c[i[1]], c[i[2]]);
    let mut o = [0.0; 3];
    if hi > lo {
        (o[i[1]], o[i[2]]) = ((mid - lo) * s / (hi - lo), s);
    }
    o
}

/// The blended colour B(Cb, Cs) of straight colours.
fn mix(mode: Composite, cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    match mode {
        Composite::Hue => set_lum(set_sat(cs, sat(cb)), lum(cb)),
        Composite::Saturation => set_lum(set_sat(cb, sat(cs)), lum(cb)),
        Composite::Color => set_lum(cs, lum(cb)),
        Composite::Luminosity => set_lum(cb, lum(cs)),
        _ => std::array::from_fn(|i| separable(mode, cb[i], cs[i])),
    }
}

/// `s` (premultiplied, its alpha scaled by `o`) composited onto `d` with `mode`, for any backdrop alpha (W3C general
/// formula). Cut takes the source's alpha out of the backdrop (an opaque backdrop goes black there). In a compositing
/// group (`group`), Multiply (Mask) is a mask: the backdrop keeps the source's luminance times its alpha.
fn blend_px(d: Color32, s: Color32, mode: Composite, o: f32, group: bool) -> Color32 {
    let f = |c: Color32| c.to_array().map(|v| v as f32 / 255.0);
    let (dp, sp) = (f(d), f(s).map(|v| v * o));
    let (ab, a) = (dp[3], sp[3]);
    let out = |c: [f32; 4]| {
        let q = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
        Color32::from_rgba_premultiplied(q[0], q[1], q[2], q[3])
    };
    let straight = |c: [f32; 4], a: f32| if a > 0.0 { [c[0] / a, c[1] / a, c[2] / a] } else { [0.0; 3] };
    match mode {
        Composite::Cut => {
            let k = 1.0 - a;
            return out([dp[0] * k, dp[1] * k, dp[2] * k, if ab >= 1.0 { 1.0 } else { ab * k }]);
        }
        Composite::Multiply if group => {
            // At opacity o the mask goes 1 - o of the way back to clear.
            let m = lum(straight(sp, a)).min(1.0) * a + (1.0 - o);
            return out(dp.map(|v| v * m));
        }
        _ if a <= 0.0 => return d,
        _ => {}
    }
    let b = mix(mode, straight(dp, ab), straight(sp, a));
    let c: [f32; 3] = std::array::from_fn(|i| sp[i] * (1.0 - ab) + dp[i] * (1.0 - a) + a * ab * b[i].clamp(0.0, 1.0));
    out([c[0], c[1], c[2], a + ab - a * ab])
}

/// Composite `layer` onto `acc` with `mode` at `opacity`: (1 - a) Cd + a B(Cd, Cs) over an opaque backdrop, the W3C
/// general formula over a transparent one (inside a compositing group, `group`). Custom runs `compositor`.
fn blend(acc: &mut ColorImage, layer: &ColorImage, mode: Composite, opacity: f32, compositor: Option<&FxEval>, group: bool, k: usize) {
    if mode == Composite::Custom {
        // A compositor this build does not know draws the layer as Source Alpha.
        match compositor {
            Some(c) if crate::engine::plugins::find(c.id).is_some() => return vfx::compositor(acc, layer, c, opacity, k),
            _ => return blend(acc, layer, Composite::SourceAlpha, opacity, None, group, k),
        }
    }
    let o8 = (opacity.clamp(0.0, 1.0) * 255.0).round() as u32;
    let o = o8 as f32 / 255.0;
    let mask = group && mode == Composite::Multiply;
    let w = acc.size[0];
    par_rows(&mut acc.pixels, w, k, |y, row| {
        for (d, &s) in row.iter_mut().zip(&layer.pixels[y * w..(y + 1) * w]) {
            if s.a() == 0 && !mask {
                continue;
            }
            if matches!(mode, Composite::SourceAlpha | Composite::ThreeD) {
                *d = over(*d, s, o8);
                continue;
            }
            let dd = d.to_array();
            if dd[3] < 255 || mask || blend_channel(mode, 0, 0).is_none() {
                *d = blend_px(*d, s, mode, o, group);
                continue;
            }
            let s = s.to_array().map(|c| mul255(c as u32, o8));
            let sa = s[3];
            if sa == 0 {
                continue;
            }
            let ch = |i: usize| {
                let cs = ((s[i] * 255 + sa / 2) / sa).min(255);
                (mul255(dd[i] as u32, 255 - sa) + mul255(sa, blend_channel(mode, dd[i] as u32, cs).unwrap_or(cs))).min(255) as u8
            };
            *d = Color32::from_rgba_premultiplied(ch(0), ch(1), ch(2), 255);
        }
    });
}

/// Track Motion then the frame size: normalised frame -> output pixels.
fn layer_to_out(l: &TrackPlan, w: usize, h: usize) -> Affine {
    l.motion.then(Affine::scale(w as f64, h as f64))
}

/// Decode size for an event: the smallest of full, 1/2, 1/4, 1/8 of the source covering its size on screen.
fn decode_size(e: &EventPlan, to_out: Affine) -> (u32, u32) {
    let (sw, sh) = (e.src.m.w.max(2), e.src.m.h.max(2));
    let m = e.place.then(to_out);
    let need = m.a.hypot(m.c).max(m.b.hypot(m.d) * sw as f64 / sh as f64);
    let w = [8, 4, 2, 1].into_iter().map(|d| sw / d).find(|&w| w as f64 >= need).unwrap_or(sw);
    let w = (w / 2 * 2).max(2);
    (w, ((w as f64 * sh as f64 / sw as f64 / 2.0).round() as u32 * 2).max(2))
}

/// Every source a frame will ask for, at the size it will ask: for decoder pre-warming.
pub fn requests(fp: &FramePlan, w: usize, h: usize) -> Vec<(&SrcAt, u32, u32)> {
    let mut v = Vec::new();
    for l in &fp.layers {
        let to_out = layer_to_out(l, w, h);
        for e in l.items.iter().flat_map(events) {
            let (dw, dh) = decode_size(e, to_out);
            v.push((&e.src, dw, dh));
        }
    }
    v
}

/// The events an item draws, bottom first, with the alpha factor each gets.
fn events(i: &Item) -> Vec<&EventPlan> {
    match i {
        Item::Event(e) => vec![e],
        Item::Mix { a, b, .. } => a.iter().chain(b).collect(),
    }
}

/// One event's source, through its event FX (on a copy), drawn at `alpha`. `to_out` (with Track Motion) sets the
/// decode size; `place_out` is where it lands (without Track Motion when track FX come first).
#[allow(clippy::too_many_arguments)]
fn draw_event(dst: &mut ColorImage, src: &mut dyn Sources, e: &EventPlan, to_out: Affine, place_out: Affine, alpha: f32, bilinear: bool, k: usize) {
    if alpha <= 0.0 {
        return;
    }
    let (dw, dh) = decode_size(e, to_out);
    let Some(mut img) = src.get(&e.src, dw, dh) else { return };
    if let Some((n, w)) = e.next {
        if let Some(b) = src.get(&SrcAt { m: e.src.m.clone(), sf: Some(n), generator: None }, dw, dh) {
            blend_frames(Arc::make_mut(&mut img), &b, w, k);
        }
    }
    if e.flicker {
        reduce_flicker(Arc::make_mut(&mut img), k);
    }
    for f in &e.fx {
        vfx::apply(Arc::make_mut(&mut img), f, k);
    }
    raster::apply(Arc::make_mut(&mut img), &e.masks, k);
    let m = Affine::scale(1.0 / img.size[0] as f64, 1.0 / img.size[1] as f64).then(e.place).then(place_out);
    draw(dst, &img, m, alpha, bilinear, k);
}

fn draw_items(dst: &mut ColorImage, src: &mut dyn Sources, l: &TrackPlan, gain: f32, bilinear: bool, k: usize, with_motion: bool) {
    let [w, h] = dst.size;
    let to_out = layer_to_out(l, w, h);
    let place_out = if with_motion { to_out } else { Affine::scale(w as f64, h as f64) };
    for item in &l.items {
        match item {
            Item::Mix { a, b, p, kind: Some(kind) } => {
                // Both sides as whole frames without Track Motion, mixed, then moved as one picture: a wipe or push
                // stays inside a picture-in-picture, as in VEGAS, whichever path the track takes.
                let frame = Affine::scale(w as f64, h as f64);
                let mut side = |e: Option<&EventPlan>| {
                    let mut img = ColorImage::new([w, h], Color32::TRANSPARENT);
                    if let Some(e) = e {
                        draw_event(&mut img, src, e, to_out, frame, e.alpha, bilinear, k);
                    }
                    img
                };
                let (ia, ib) = (side(a.as_ref()), side(b.as_ref()));
                let m = if with_motion { Affine::scale(1.0 / w as f64, 1.0 / h as f64).then(to_out) } else { Affine::translate(0.0, 0.0) };
                draw(dst, &transition::mix(kind, &ia, &ib, *p, k), m, gain, bilinear, k);
            }
            _ => {
                // An unknown transition draws as a dissolve: A, then B at alpha p (or A alone at 1 - p, out to nothing).
                let (p, out) = if let Item::Mix { p, b, .. } = item { (*p, b.is_none()) } else { (1.0, false) };
                let evs = events(item);
                let last = evs.len().saturating_sub(1);
                for (i, e) in evs.into_iter().enumerate() {
                    let w = if out { 1.0 - p } else if i == last { p } else { 1.0 };
                    draw_event(dst, src, e, to_out, place_out, e.alpha * gain * w, bilinear, k);
                }
            }
        }
    }
}

/// Fade to Color: each pixel `t` of the way to `color`, at its own alpha (where the track is transparent it stays so).
fn fade_to(img: &mut ColorImage, [r, g, b]: [u8; 3], t: f32, k: usize) {
    let w = img.size[0];
    par_rows(&mut img.pixels, w, k, |_, row| {
        for p in row.iter_mut() {
            let a = p.a() as u32;
            let c = Color32::from_rgba_premultiplied(mul255(r as u32, a) as u8, mul255(g as u32, a) as u8, mul255(b as u32, a) as u8, a as u8);
            *p = vfx::lerp_px(*p, c, t);
        }
    });
}

/// 3x3 inverse (row-major); None when singular.
fn inv3(m: [f64; 9]) -> Option<[f64; 9]> {
    let [a, b, c, d, e, f, g, h, i] = m;
    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    (det.abs() > 1e-12).then(|| [e * i - f * h, c * h - b * i, b * f - c * e, f * g - d * i, a * i - c * g, c * d - a * f, d * h - e * g, b * g - a * h, a * e - b * d].map(|v| v / det))
}

/// Draw the frame-sized `src` through projective `hm` (source frame 0..1 to destination frame 0..1) onto `dst`,
/// source-over. Points behind the camera are not drawn.
fn draw_persp(dst: &mut ColorImage, src: &ColorImage, hm: [f64; 9], bilinear: bool, k: usize) {
    let ([dw, dh], [sw, sh]) = (dst.size, src.size);
    let Some(q) = inv3(hm) else { return };
    par_rows(&mut dst.pixels, dw, k, |y, row| {
        let v = (y as f64 + 0.5) / dh as f64;
        for (x, px) in row.iter_mut().enumerate() {
            let u = (x as f64 + 0.5) / dw as f64;
            let z = q[6] * u + q[7] * v + q[8];
            // z is 1 / depth: a point behind the camera maps back with z <= 0.
            if z <= 1e-9 {
                continue;
            }
            let (su, sv) = ((q[0] * u + q[1] * v + q[2]) / z * sw as f64, (q[3] * u + q[4] * v + q[5]) / z * sh as f64);
            if su < 0.0 || sv < 0.0 || su >= sw as f64 || sv >= sh as f64 {
                continue;
            }
            let s = if bilinear { bilerp(src, (su * 65536.0) as i64 - 32768, (sv * 65536.0) as i64 - 32768) } else { src.pixels[sv as usize * sw + su as usize] };
            *px = over(*px, s, 255);
        }
    });
}

/// 2D Shadow / 2D Glow: the picture's alpha, coloured at the halo's intensity, blurred and offset, drawn under it.
fn add_halos(img: &mut ColorImage, halos: [Option<Halo>; 2], k: usize) {
    let [w, h] = img.size;
    let mut under: Option<ColorImage> = None;
    for halo in halos.into_iter().flatten() {
        let i8 = (halo.intensity.clamp(0.0, 1.0) * 255.0).round() as u32;
        let c = halo.color.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u32);
        let (dx, dy) = ((halo.x * w as f32).round() as i64, (halo.y * h as f32).round() as i64);
        let mut sh = ColorImage::new([w, h], Color32::TRANSPARENT);
        par_rows(&mut sh.pixels, w, k, |y, row| {
            let sy = y as i64 - dy;
            if !(0..h as i64).contains(&sy) {
                return;
            }
            for (x, p) in row.iter_mut().enumerate() {
                let sx = x as i64 - dx;
                if (0..w as i64).contains(&sx) {
                    let a = mul255(img.pixels[sy as usize * w + sx as usize].a() as u32, i8);
                    *p = Color32::from_rgba_premultiplied(mul255(c[0], a) as u8, mul255(c[1], a) as u8, mul255(c[2], a) as u8, a as u8);
                }
            }
        });
        let r = (halo.blur.max(0.0) * h as f32).round() as usize;
        vfx::blur(&mut sh, r, r, k);
        match &mut under {
            Some(u) => u.pixels.iter_mut().zip(&sh.pixels).for_each(|(d, s)| *d = over(*d, *s, 255)),
            None => under = Some(sh),
        }
    }
    if let Some(mut u) = under {
        u.pixels.iter_mut().zip(&img.pixels).for_each(|(d, s)| *d = over(*d, *s, 255));
        *img = u;
    }
}

/// Track `l` composited onto `dst` (inside a compositing group when `group`: the backdrop may be transparent).
fn layer_onto(dst: &mut ColorImage, src: &mut dyn Sources, l: &TrackPlan, bilinear: bool, k: usize, group: bool) {
    let [w, h] = dst.size;
    let draws: usize = l.items.iter().map(|i| events(i).len()).sum();
    let halo = l.shadow.is_some() || l.glow.is_some();
    // Fast path: draw straight into the accumulator, sampling each pixel once.
    let plain = l.fx.is_empty() && l.fade.is_none() && !halo && l.persp.is_none();
    if plain && matches!(l.mode, Composite::SourceAlpha | Composite::ThreeD) && (l.opacity >= 1.0 || draws <= 1) {
        draw_items(dst, src, l, l.opacity, bilinear, k, true);
        return;
    }
    let mut layer = ColorImage::new([w, h], Color32::TRANSPARENT);
    // Track FX on the track's own frame, then Track Motion (or 3D Track Motion) moves the result.
    let direct = l.fx.is_empty() && l.persp.is_none();
    draw_items(&mut layer, src, l, 1.0, bilinear, k, direct);
    if !direct {
        for f in &l.fx {
            vfx::apply(&mut layer, f, k);
        }
        let mut moved = ColorImage::new([w, h], Color32::TRANSPARENT);
        match l.persp {
            Some(hm) => draw_persp(&mut moved, &layer, hm, bilinear, k),
            None => draw(&mut moved, &layer, Affine::scale(1.0 / w as f64, 1.0 / h as f64).then(layer_to_out(l, w, h)), 1.0, bilinear, k),
        }
        layer = moved;
    }
    if halo {
        add_halos(&mut layer, [l.shadow, l.glow], k);
    }
    if let Some((c, t)) = l.fade {
        fade_to(&mut layer, c, t, k);
    }
    blend(dst, &layer, l.mode, l.opacity, l.compositor.as_ref(), group, k);
}

/// Compose one frame `w` x `h`: opaque black, then each track bottom to top, then the Video Output FX and the Video
/// Bus Track's Fade to Color. A compositing parent's children (just below it) are composited together first, then the
/// parent over them, then the group through Parent Motion with the Parent Composite Mode.
pub fn compose(fp: &FramePlan, src: &mut dyn Sources, w: usize, h: usize, bilinear: bool, k: usize) -> ColorImage {
    let mut acc = ColorImage::new([w, h], Color32::BLACK);
    let mut group: Option<ColorImage> = None;
    let blank = || ColorImage::new([w, h], Color32::TRANSPARENT);
    for l in &fp.layers {
        if l.child {
            layer_onto(group.get_or_insert_with(blank), src, l, bilinear, k, true);
            continue;
        }
        let Some((mode, motion)) = l.parent else {
            // Children whose parent does not show composite as they are.
            if let Some(g) = group.take() {
                blend(&mut acc, &g, Composite::SourceAlpha, 1.0, None, false, k);
            }
            layer_onto(&mut acc, src, l, bilinear, k, false);
            continue;
        };
        let mut g = group.take().unwrap_or_else(blank);
        // A parent with no event here leaves its children as they are (an empty mask hides nothing).
        if !l.items.is_empty() {
            layer_onto(&mut g, src, l, bilinear, k, true);
        }
        if whole_pixel(motion).is_some_and(|o| o == (0, 0)) {
            blend(&mut acc, &g, mode, 1.0, None, false, k);
        } else {
            let mut moved = blank();
            draw(&mut moved, &g, Affine::scale(1.0 / w as f64, 1.0 / h as f64).then(motion).then(Affine::scale(w as f64, h as f64)), 1.0, bilinear, k);
            blend(&mut acc, &moved, mode, 1.0, None, false, k);
        }
    }
    if let Some(g) = group {
        blend(&mut acc, &g, Composite::SourceAlpha, 1.0, None, false, k);
    }
    if !fp.out_fx.is_empty() {
        for f in &fp.out_fx {
            vfx::apply(&mut acc, f, k);
        }
        // The output is opaque: whatever an output FX made transparent shows black.
        for p in &mut acc.pixels {
            let [r, g, b, _] = p.to_array();
            *p = Color32::from_rgb(r, g, b);
        }
    }
    if let Some((c, t)) = fp.fade {
        fade_to(&mut acc, c, t, k);
    }
    acc
}

/// Motion blur / supersampling: the weighted mean of `frames` (one size), weights in `w`.
pub fn mean(frames: &[ColorImage], w: &[f32]) -> ColorImage {
    let total: f32 = w.iter().sum::<f32>().max(1e-6);
    let q: Vec<u32> = w.iter().map(|v| (v / total * 65536.0).round() as u32).collect();
    let mut out = frames[0].clone();
    for (i, p) in out.pixels.iter_mut().enumerate() {
        let mut s = [32768u32; 4];
        for (f, &q) in frames.iter().zip(&q) {
            let c = f.pixels[i].to_array();
            (0..4).for_each(|j| s[j] += c[j] as u32 * q);
        }
        let c = s.map(|v| (v >> 16).min(255) as u8);
        *p = Color32::from_rgba_premultiplied(c[0], c[1], c[2], c[3]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::plan::{self, MediaRef};
    use crate::project::{Motion, PanCrop};

    fn media(w: u32, h: u32) -> Arc<MediaRef> {
        Arc::new(MediaRef {
            id: "m".into(),
            path: "m.mp4".into(),
            w,
            h,
            duration: 10.0,
            fps: 30.0,
            still: false,
            has_alpha: false,
            matrix: None,
            codec: None,
            interlaced: false,
            proxy: None,
            grid: 0.0,
            offset: 0.0,
            stream: 0,
            flow: false,
        })
    }

    /// Fake decoder: left half `l`, right half `r`, at whatever size is asked.
    struct Halves(Color32, Color32);
    impl Sources for Halves {
        fn get(&mut self, _: &SrcAt, w: u32, h: u32) -> Option<Arc<ColorImage>> {
            let (w, h) = (w as usize, h as usize);
            Some(Arc::new(ColorImage { size: [w, h], pixels: (0..w * h).map(|i| if i % w < w / 2 { self.0 } else { self.1 }).collect() }))
        }
    }

    fn event(m: &Arc<MediaRef>, pc: PanCrop, alpha: f32) -> EventPlan {
        EventPlan { src: SrcAt { m: m.clone(), sf: Some(0), generator: None }, place: plan::place(pc, (m.w as f64, m.h as f64), (1920.0, 1080.0), true), alpha, fx: Vec::new(), next: None, flicker: false, masks: Vec::new() }
    }

    fn layer(items: Vec<Item>, mode: Composite, opacity: f32, motion: Motion) -> TrackPlan {
        TrackPlan { mode, opacity, motion: plan::motion(motion, (1920.0, 1080.0)), persp: None, fx: Vec::new(), fade: None, compositor: None, child: false, parent: None, shadow: None, glow: None, items }
    }

    /// R4a acceptance, end to end from the project: a Transition Progress envelope at 0.25 renders the dissolve 25%
    /// mixed, a transition out to nothing leaves the event at 1 - p over black, and a Fade to Color envelope at -1
    /// draws the bottom colour where the track has a picture (and only there).
    #[test]
    fn progress_fade_out_transition_and_fade_to_color() {
        use crate::project::{Clip, Envelope, EnvKind, Keyframe, Keyframes, MediaItem, MediaType, Project, Transition, Curve};
        struct ById;
        impl Sources for ById {
            fn get(&mut self, s: &SrcAt, w: u32, h: u32) -> Option<Arc<ColorImage>> {
                let c = if &*s.m.id == "a" { Color32::from_gray(200) } else { Color32::from_gray(100) };
                Some(Arc::new(ColorImage::new([w as usize, h as usize], c)))
            }
        }
        let mut p = Project::new("t");
        p.settings.frame_rate = 30.0;
        for id in ["a", "b"] {
            p.media_pool.push(MediaItem { id: id.into(), media_type: MediaType::Video, duration: 100.0, width: Some(1920), height: Some(1080), frame_rate: Some(30.0), ..Default::default() });
        }
        let mut a = Clip::from_media(&p.media_pool[0], 0.0);
        (a.duration, a.fade_out) = (6.0, 2.0);
        let mut b = Clip::from_media(&p.media_pool[1], 4.0);
        (b.duration, b.fade_in) = (6.0, 2.0);
        let key = |t, v| Keyframe { t, v, curve: Curve::Linear };
        b.transition = Some(Transition { plugin: "milan.tr.dissolve".into(), progress: Keyframes { keys: vec![key(0.0, 0.25)] }, ..Default::default() });
        p.timeline.video_tracks[0].clips = vec![a, b];
        let frame = |p: &Project, t: f64| {
            let vp = plan::compile(p);
            compose(&plan::frame_plan(&vp, plan::frame_at(t, vp.fps)), &mut ById, W, H, true, 1)
        };
        assert_eq!(px(&frame(&p, 5.0), W / 2, H / 2), [175, 175, 175, 255]); // 200 x 0.75 + 100 x 0.25
        // b fades out to nothing through a dissolve: half way, half of b over black.
        let c = &mut p.timeline.video_tracks[0].clips[1];
        (c.fade_out, c.transition_out) = (2.0, Some(Transition { plugin: "milan.tr.dissolve".into(), ..Default::default() }));
        assert_eq!(px(&frame(&p, 9.0), W / 2, H / 2), [50, 50, 50, 255]);
        // Fade to Color at -1 over a half-size picture: the bottom colour on the picture, black around it.
        let t = &mut p.timeline.video_tracks[0];
        (t.fade_colors, t.motion) = ([[255, 255, 255], [0, 128, 255]], Keyframes { keys: vec![Keyframe { t: 0.0, v: Motion { w: 0.5, h: 0.5, ..Default::default() }, curve: Curve::Linear }] });
        t.envelopes.push(Envelope { kind: EnvKind::FadeToColor, points: Keyframes { keys: vec![key(0.0, -1.0)] } });
        let out = frame(&p, 1.0);
        assert_eq!((px(&out, W / 2, H / 2), px(&out, 2, 2)), ([0, 128, 255, 255], [0, 0, 0, 255]));
        // At +0.5, half way to the top colour (white) from a's grey 200.
        p.timeline.video_tracks[0].envelopes[0].points.keys[0].v = 0.5;
        assert_eq!(px(&frame(&p, 1.0), W / 2, H / 2), [228, 228, 228, 255]);
    }

    const W: usize = 192;
    const H: usize = 108;

    fn px(img: &ColorImage, x: usize, y: usize) -> [u8; 4] {
        img.pixels[y * img.size[0] + x].to_array()
    }

    #[test]
    fn mul255_rounds_exactly() {
        for a in 0..=255 {
            for b in 0..=255 {
                assert_eq!(mul255(a, b), (a * b + 127) / 255, "{a} {b}");
            }
        }
    }

    #[test]
    fn modes_match_f64_reference() {
        use Composite::*;
        let reference = |mode: Composite, d: f64, s: f64| match mode {
            SourceAlpha => s,
            Add => (d + s).min(1.0),
            Subtract => (d - s).max(0.0),
            Multiply => d * s,
            Screen => d + s - d * s,
            Overlay if d < 0.5 => 2.0 * s * d,
            Overlay => 1.0 - 2.0 * (1.0 - s) * (1.0 - d),
            Darken => d.min(s),
            Lighten => d.max(s),
            Difference => (d - s).abs(),
            _ => unreachable!(),
        };
        for mode in [SourceAlpha, Add, Subtract, Multiply, Screen, Overlay, Darken, Lighten, Difference] {
            for d in (0..=255).step_by(5) {
                for s in (0..=255).step_by(3) {
                    let mut acc = ColorImage::new([1, 1], Color32::from_gray(d as u8));
                    let layer = ColorImage::new([1, 1], Color32::from_gray(s as u8));
                    blend(&mut acc, &layer, mode, 1.0, None, false, 1);
                    let want = (reference(mode, d as f64 / 255.0, s as f64 / 255.0) * 255.0).round() as i32;
                    let got = acc.pixels[0].r() as i32;
                    assert!((got - want).abs() <= 1, "{mode:?} d={d} s={s}: {got} vs {want}");
                }
            }
        }
        // Source alpha with a half-transparent source.
        let mut acc = ColorImage::new([1, 1], Color32::from_gray(200));
        blend(&mut acc, &ColorImage::new([1, 1], Color32::from_rgba_premultiplied(50, 50, 50, 128)), SourceAlpha, 1.0, None, false, 1);
        assert!((acc.pixels[0].r() as f64 - (50.0 + 200.0 * 127.0 / 255.0)).abs() <= 1.0);
    }

    /// One pixel of `s` blended onto `d` (both opaque) with `mode`.
    fn one(mode: Composite, d: [u8; 3], s: [u8; 3]) -> [u8; 4] {
        let mut acc = ColorImage::new([1, 1], Color32::from_rgb(d[0], d[1], d[2]));
        blend(&mut acc, &ColorImage::new([1, 1], Color32::from_rgb(s[0], s[1], s[2])), mode, 1.0, None, false, 1);
        acc.pixels[0].to_array()
    }

    /// R7 acceptance: each new blend mode matches a hand-computed pixel (backdrop 0.6, source 0.4 / 0.8, per W3C).
    #[test]
    fn new_modes_match_hand_computed_pixels() {
        use Composite::*;
        let (d, lo, hi) = ([153; 3], [102; 3], [204; 3]); // 0.6, 0.4, 0.8
        let g = |v: f64| (v * 255.0).round() as u8;
        let cases = [
            (HardLight, lo, g(2.0 * 0.6 * 0.4)),                       // 0.48
            (HardLight, hi, g(1.0 - 2.0 * 0.4 * 0.2)),                 // 0.84
            (Dodge, lo, g(0.6 / 0.6)),                                 // 1.0
            (Burn, hi, g(1.0 - 0.4 / 0.8)),                            // 0.5
            (DifferenceSquared, lo, g(0.2 * 0.2)),                     // 0.04
            (SoftLight, lo, g(0.6 - 0.2 * 0.6 * 0.4)),                 // 0.552
            (SoftLight, hi, g(0.6 + 0.6 * (0.6f64.sqrt() - 0.6))),     // D(0.6) = sqrt(0.6)
            (Exclusion, lo, g(0.6 + 0.4 - 2.0 * 0.24)),                // 0.52
            (HardMix, lo, 255),                                         // 0.6 + 0.4 >= 1
            (HardMix, [50; 3], 0),
            (ThreeD, lo, 102),                                          // Source Alpha
            (Cut, lo, 0),                                               // an opaque source cuts it all out
        ];
        for (mode, s, want) in cases {
            let got = one(mode, d, s);
            assert!(got[..3].iter().all(|&c| c.abs_diff(want) <= 1) && got[3] == 255, "{mode:?} over {s:?}: {got:?} vs {want}");
        }
        // Non-separable (W3C): Luminosity gives the backdrop the source's luminosity, Color its hue and saturation.
        let (red, grey) = ([200, 50, 50], [128, 128, 128]);
        let lum = |c: [u8; 4]| 0.3 * c[0] as f64 + 0.59 * c[1] as f64 + 0.11 * c[2] as f64;
        let l = one(Luminosity, red, grey);
        assert!((lum(l) - 128.0).abs() <= 1.5 && l[0] > l[1], "{l:?}");
        let c = one(Color, grey, red);
        assert!((lum(c) - 128.0).abs() <= 1.5 && c[0] > c[1] && c[1] == c[2], "{c:?}");
        // Saturation of a grey source: the backdrop's luminosity (60 + 29.5 + 5.5 = 95), no colour.
        let s = one(Saturation, red, grey);
        assert!(s[..3].iter().all(|&v| v.abs_diff(95) <= 1) && s[0] == s[2], "{s:?}");
        let h = one(Hue, [200, 100, 100], [40, 40, 200]);
        assert!(h[2] > h[0] && (lum(h) - lum([200, 100, 100, 0])).abs() <= 1.5, "{h:?}");
        // Cut over a transparent backdrop (inside a compositing group) takes the source's alpha out.
        let mut half = ColorImage::new([1, 1], Color32::from_rgba_premultiplied(100, 100, 100, 200));
        blend(&mut half, &ColorImage::new([1, 1], Color32::from_rgba_premultiplied(0, 0, 0, 128)), Cut, 1.0, None, true, 1);
        assert_eq!(half.pixels[0].a(), 100); // 200 x (1 - 128 / 255)
        // A transparent backdrop (a group) gets the source as it is for every separable mode where nothing is below.
        let mut empty = ColorImage::new([1, 1], Color32::TRANSPARENT);
        blend(&mut empty, &ColorImage::new([1, 1], Color32::from_rgb(10, 20, 30)), Multiply, 1.0, None, false, 1);
        assert_eq!(empty.pixels[0].to_array(), [10, 20, 30, 255]);
    }

    /// R7: parent/child compositing. A white-and-black parent in Multiply (Mask) over its child lets the child through
    /// where it is white; where it is black the tracks below show. The Parent Composite Mode then applies to the group.
    #[test]
    fn compositing_parent_masks_its_child() {
        struct ById;
        impl Sources for ById {
            fn get(&mut self, s: &SrcAt, w: u32, h: u32) -> Option<Arc<ColorImage>> {
                let (w, h) = (w as usize, h as usize);
                let px = |i: usize| match &*s.m.id {
                    "bg" => Color32::from_rgb(0, 0, 200),
                    "child" => Color32::from_rgb(200, 0, 0),
                    // The mask: white left half, black right half.
                    _ => if i % w < w / 2 { Color32::WHITE } else { Color32::BLACK },
                };
                Some(Arc::new(ColorImage { size: [w, h], pixels: (0..w * h).map(px).collect() }))
            }
        }
        let track = |id: &str| {
            let mm = Arc::new(MediaRef { id: id.into(), ..(*media(1920, 1080)).clone() });
            layer(vec![Item::Event(event(&mm, PanCrop::default(), 1.0))], Composite::SourceAlpha, 1.0, Motion::default())
        };
        let bg = track("bg");
        let mut child = track("child");
        child.child = true;
        let mut parent = track("mask");
        (parent.mode, parent.parent) = (Composite::Multiply, Some((Composite::SourceAlpha, Affine::scale(1.0, 1.0))));
        let fp = FramePlan { fade: None, samples: Vec::new(), out_fx: Vec::new(), layers: vec![bg, child, parent] };
        let out = compose(&fp, &mut ById, W, H, true, 1);
        assert_eq!((px(&out, 10, 50), px(&out, W - 10, 50)), ([200, 0, 0, 255], [0, 0, 200, 255]));
        // Parent Composite Mode Add: the masked child adds onto the background.
        let mut fp = fp;
        let mask_items = std::mem::take(&mut fp.layers[2].items);
        let out = compose(&fp, &mut ById, W, H, true, 1);
        assert_eq!(px(&out, W - 10, 50), [200, 0, 0, 255], "a parent without an event here masks nothing");
        fp.layers[2].items = mask_items;
        fp.layers[2].parent = Some((Composite::Add, Affine::scale(1.0, 1.0)));
        let out = compose(&fp, &mut ById, W, H, true, 1);
        assert_eq!((px(&out, 10, 50), px(&out, W - 10, 50)), ([200, 0, 200, 255], [0, 0, 200, 255]));
        // Parent Motion moves the whole group: at half size in the middle, the edges show the background alone.
        fp.layers[2].parent = Some((Composite::SourceAlpha, plan::motion(Motion { w: 0.5, h: 0.5, ..Default::default() }, (1920.0, 1080.0))));
        let out = compose(&fp, &mut ById, W, H, true, 1);
        assert_eq!((px(&out, 2, 2), px(&out, W / 2 - 10, H / 2)), ([0, 0, 200, 255], [200, 0, 0, 255]));
    }

    /// R7: a 2D Shadow is the picture's shape, offset and coloured, under it; a mask on the event cuts the picture.
    #[test]
    fn shadow_and_mask() {
        let m = media(1920, 1080);
        let mut l = layer(vec![Item::Event(event(&m, PanCrop::default(), 1.0))], Composite::SourceAlpha, 1.0, Motion { w: 0.5, h: 0.5, ..Default::default() });
        l.shadow = Some(Halo { x: 0.3, y: 0.0, blur: 0.0, intensity: 1.0, color: [0.0, 1.0, 0.0] });
        let fp = FramePlan { fade: None, samples: Vec::new(), out_fx: Vec::new(), layers: vec![l] };
        let out = compose(&fp, &mut Halves(Color32::RED, Color32::RED), W, H, true, 1);
        // The picture spans x 48..144; its shadow 30% of 192 (58 px) to the right: green from 144 to 202.
        assert_eq!((px(&out, 100, 54), px(&out, 160, 54), px(&out, 20, 54)), ([255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 0, 255]));
        let mut e = event(&m, PanCrop::default(), 1.0);
        e.masks = vec![raster::MaskPlan { path: vec![[0.0, 0.0, 0.0, 0.0, 0.0, 0.0], [0.5, 0.0, 0.0, 0.0, 0.0, 0.0], [0.5, 1.0, 0.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0, 0.0, 0.0]], negative: false, opacity: 1.0, feather: 0.0 }];
        let fp = FramePlan { fade: None, samples: Vec::new(), out_fx: Vec::new(), layers: vec![layer(vec![Item::Event(e)], Composite::SourceAlpha, 1.0, Motion::default())] };
        let out = compose(&fp, &mut Halves(Color32::WHITE, Color32::WHITE), W, H, true, 1);
        assert_eq!((px(&out, 20, 54), px(&out, W - 20, 54)), ([255, 255, 255, 255], [0, 0, 0, 255]));
    }

    /// R7: 3D Source Alpha with no depth or turn is the 2D motion; turned 60° about Y the picture narrows.
    #[test]
    fn three_d_motion() {
        let size = (1920.0, 1080.0);
        assert!(plan::motion3d(Motion::default(), size).is_none());
        let h = plan::motion3d(Motion { ry: 60.0, ..Default::default() }, size).unwrap();
        let at = |u: f64, v: f64| {
            let z = h[6] * u + h[7] * v + h[8];
            ((h[0] * u + h[1] * v + h[2]) / z, (h[3] * u + h[4] * v + h[5]) / z)
        };
        let ((x0, _), (x1, _), (cx, cy)) = (at(0.0, 0.5), at(1.0, 0.5), at(0.5, 0.5));
        // Half as wide seen edge-on at 60° (cos 60°), the near side larger than the far one in perspective.
        assert!((cx - 0.5).abs() < 1e-9 && (cy - 0.5).abs() < 1e-9 && x1 - x0 < 0.75 && x1 - x0 > 0.4, "{x0} {x1}");
        assert!((x1 - 0.5) > (0.5 - x0), "the side turned towards the camera is larger: {x0} {x1}");
        // A small depth matches the 2D motion's centre; through the compositor the turned track is narrower.
        let m = media(1920, 1080);
        let mut l = layer(vec![Item::Event(event(&m, PanCrop::default(), 1.0))], Composite::ThreeD, 1.0, Motion::default());
        l.persp = Some(h);
        let out = compose(&FramePlan { fade: None, samples: Vec::new(), out_fx: Vec::new(), layers: vec![l] }, &mut Halves(Color32::RED, Color32::RED), W, H, true, 1);
        assert_eq!((px(&out, W / 2, H / 2), px(&out, 5, H / 2)), ([255, 0, 0, 255], [0, 0, 0, 255]));
    }

    /// R7: the Video Bus Track's motion blur is the weighted mean of its sub-frames.
    #[test]
    fn mean_of_sub_frames() {
        let a = ColorImage::new([2, 1], Color32::from_gray(0));
        let b = ColorImage::new([2, 1], Color32::from_gray(200));
        assert_eq!(mean(&[a.clone(), b.clone()], &[1.0, 1.0]).pixels[0].r(), 100);
        assert_eq!(mean(&[a, b], &[3.0, 1.0]).pixels[0].r(), 50);
    }

    #[test]
    fn fast_path_equals_general_path() {
        let m = media(1920, 1080);
        let fp = |mode| FramePlan { fade: None, samples: Vec::new(), out_fx: Vec::new(), layers: vec![layer(vec![Item::Event(event(&m, PanCrop::default(), 0.8))], mode, 0.5, Motion::default())] };
        let fast = compose(&fp(Composite::SourceAlpha), &mut Halves(Color32::RED, Color32::BLUE), W, H, true, 1);
        // Normal through the scratch layer: blending a layer with the general formula at opacity 0.5.
        let mut general = ColorImage::new([W, H], Color32::BLACK);
        let mut scratch = ColorImage::new([W, H], Color32::TRANSPARENT);
        draw_items(&mut scratch, &mut Halves(Color32::RED, Color32::BLUE), &fp(Composite::SourceAlpha).layers[0], 1.0, true, 1, true);
        blend(&mut general, &scratch, Composite::SourceAlpha, 0.5, None, false, 1);
        for (a, b) in fast.pixels.iter().zip(&general.pixels) {
            for (x, y) in a.to_array().into_iter().zip(b.to_array()) {
                assert!(x.abs_diff(y) <= 1, "{a:?} {b:?}");
            }
        }
    }

    #[test]
    fn stripes_equal_single_thread_bit_for_bit() {
        let src = ColorImage { size: [97, 61], pixels: (0..97 * 61).map(|i| Color32::from_rgba_premultiplied((i * 7) as u8 % 200, (i * 13) as u8 % 200, i as u8 % 200, 200)).collect() };
        let m = Affine::scale(3.3, 4.1).then(Affine::rotate(17.0)).then(Affine::translate(60.0, 10.0));
        let mut one = ColorImage::new([400, 300], Color32::from_gray(30));
        let mut many = one.clone();
        draw(&mut one, &src, m, 0.7, true, 1);
        draw(&mut many, &src, m, 0.7, true, 5);
        assert!(one.pixels == many.pixels);
        let (mut acc1, mut acc5) = (ColorImage::new([400, 300], Color32::from_gray(90)), ColorImage::new([400, 300], Color32::from_gray(90)));
        blend(&mut acc1, &one, Composite::Overlay, 0.6, None, false, 1);
        blend(&mut acc5, &one, Composite::Overlay, 0.6, None, false, 5);
        assert!(acc1.pixels == acc5.pixels);
    }

    #[test]
    fn whole_pixel_fast_path() {
        assert_eq!(whole_pixel(Affine::translate(3.0, -2.0)), Some((3, -2)));
        assert_eq!(whole_pixel(Affine::scale(1.0 / 960.0, 1.0 / 540.0).then(Affine::scale(960.0, 540.0))), Some((0, 0)));
        assert_eq!(whole_pixel(Affine::translate(3.5, 0.0)), None);
        assert_eq!(whole_pixel(Affine::scale(1.01, 1.0)), None);
        assert_eq!(whole_pixel(Affine::rotate(90.0)), None);
        let src = ColorImage { size: [37, 23], pixels: (0..37 * 23).map(|i| Color32::from_rgba_premultiplied((i * 7) as u8 % 200, i as u8 % 200, 9, 200)).collect() };
        let bg = Color32::from_gray(30);
        // Hanging off the top-left and bottom-right edges both ways.
        for (ox, oy) in [(-5i64, 7i64), (20, -10)] {
            let mut out = ColorImage::new([40, 30], bg);
            draw(&mut out, &src, Affine::translate(ox as f64, oy as f64), 0.7, true, 3);
            for y in 0..30i64 {
                for x in 0..40i64 {
                    let (sx, sy) = (x - ox, y - oy);
                    let inside = (0..37).contains(&sx) && (0..23).contains(&sy);
                    let want = if inside { over(bg, src.pixels[(sy * 37 + sx) as usize], (0.7f32 * 255.0).round() as u32) } else { bg };
                    assert_eq!(out.pixels[(y * 40 + x) as usize], want, "{ox},{oy} at {x},{y}");
                }
            }
        }
    }

    #[test]
    fn pillarbox_bars_are_transparent_and_the_right_width() {
        // A 4:3 source in a 16:9 frame: 1440 of 1920 wide, so 240 px bars (24 of 192 here).
        let m = media(640, 480);
        let l = layer(vec![Item::Event(event(&m, PanCrop::default(), 1.0))], Composite::SourceAlpha, 1.0, Motion::default());
        let mut scratch = ColorImage::new([W, H], Color32::TRANSPARENT);
        draw_items(&mut scratch, &mut Halves(Color32::RED, Color32::RED), &l, 1.0, true, 1, true);
        for y in [0, H / 2, H - 1] {
            assert_eq!(px(&scratch, 23, y)[3], 0);
            assert_eq!(px(&scratch, 24, y), [255, 0, 0, 255]);
            assert_eq!(px(&scratch, W - 25, y), [255, 0, 0, 255]);
            assert_eq!(px(&scratch, W - 24, y)[3], 0);
        }
    }

    /// R3b: Maintain Aspect Ratio off stretches the 4:3 picture over the whole 16:9 frame (no bars).
    #[test]
    fn stretched_events_fill_the_frame() {
        let m = media(640, 480);
        let mut e = event(&m, PanCrop::default(), 1.0);
        e.place = plan::place(PanCrop::default(), (640.0, 480.0), (1920.0, 1080.0), false);
        let l = layer(vec![Item::Event(e)], Composite::SourceAlpha, 1.0, Motion::default());
        let mut scratch = ColorImage::new([W, H], Color32::TRANSPARENT);
        draw_items(&mut scratch, &mut Halves(Color32::RED, Color32::BLUE), &l, 1.0, true, 1, true);
        assert_eq!((px(&scratch, 0, 0), px(&scratch, W - 1, H - 1)), ([255, 0, 0, 255], [0, 0, 255, 255]));
        assert_eq!(px(&scratch, W / 2 - 2, H / 2), [255, 0, 0, 255], "the source's middle lands in the frame's middle");
    }

    /// R3b: resampling mixes two frames by weight; Reduce Interlace Flicker is a vertical 1-2-1 blur.
    #[test]
    fn frame_blend_and_flicker_filter() {
        let mut a = ColorImage::new([2, 1], Color32::from_gray(0));
        blend_frames(&mut a, &ColorImage::new([2, 1], Color32::from_gray(200)), 0.25, 1);
        assert_eq!(a.pixels[0].to_array(), [50, 50, 50, 255]);
        // Alternating white and black lines (interlace twitter) become steady grey inside; edge rows repeat.
        let mut img = ColorImage { size: [1, 4], pixels: [255, 0, 255, 0].map(Color32::from_gray).to_vec() };
        reduce_flicker(&mut img, 1);
        let rows: Vec<u8> = img.pixels.iter().map(|p| p.r()).collect();
        assert_eq!(rows, [191, 128, 128, 64]);
        // Through compose: the event's next frame at weight 0.5, from a source that answers per frame.
        struct PerFrame;
        impl Sources for PerFrame {
            fn get(&mut self, s: &SrcAt, w: u32, h: u32) -> Option<Arc<ColorImage>> {
                Some(Arc::new(ColorImage::new([w as usize, h as usize], Color32::from_gray(if s.sf == Some(0) { 100 } else { 200 }))))
            }
        }
        let m = media(1920, 1080);
        let mut e = event(&m, PanCrop::default(), 1.0);
        e.next = Some((1, 0.5));
        let out = compose(&FramePlan { fade: None, samples: Vec::new(), out_fx: Vec::new(), layers: vec![layer(vec![Item::Event(e)], Composite::SourceAlpha, 1.0, Motion::default())] }, &mut PerFrame, W, H, true, 1);
        assert_eq!(px(&out, W / 2, H / 2), [150, 150, 150, 255]);
    }

    #[test]
    fn two_layers_at_half_level() {
        let m = media(1920, 1080);
        let l = |c: Color32| (layer(vec![Item::Event(event(&m, PanCrop::default(), 1.0))], Composite::SourceAlpha, 0.5, Motion::default()), c);
        let (bottom, top) = (l(Color32::from_gray(200)), l(Color32::from_gray(100)));
        let mut acc = ColorImage::new([W, H], Color32::BLACK);
        for (layer, c) in [bottom, top] {
            let fp = FramePlan { fade: None, samples: Vec::new(), out_fx: Vec::new(), layers: vec![layer] };
            let mut s = Halves(c, c);
            for l in &fp.layers {
                draw_items(&mut acc, &mut s, l, l.opacity, true, 1, true);
            }
        }
        // 0.5 * 100 + 0.5 * (0.5 * 200) = 100
        let v = px(&acc, W / 2, H / 2)[0] as i32;
        assert!((v - 100).abs() <= 1, "{v}");
    }

    #[test]
    fn pan_crop_zoom_and_flip() {
        let m = media(1920, 1080);
        let zoom = PanCrop { w: 0.5, h: 0.5, ..Default::default() };
        // 2x zoom on the centre: the left/right boundary stays in the middle, the source's x = 0.25 maps to the left edge.
        let p = event(&m, zoom, 1.0).place;
        let (x, y) = p.apply(0.25, 0.25);
        assert!(x.abs() < 1e-9 && y.abs() < 1e-9);
        let (x, _) = p.apply(0.75, 0.5);
        assert!((x - 1.0).abs() < 1e-9);
        let flip = PanCrop { w: -1.0, ..Default::default() };
        let fp = FramePlan { fade: None, samples: Vec::new(), out_fx: Vec::new(), layers: vec![layer(vec![Item::Event(event(&m, flip, 1.0))], Composite::SourceAlpha, 1.0, Motion::default())] };
        let out = compose(&fp, &mut Halves(Color32::RED, Color32::BLUE), W, H, true, 1);
        assert_eq!(px(&out, 5, 50), [0, 0, 255, 255]); // the source's right half is now on the left
        assert_eq!(px(&out, W - 5, 50), [255, 0, 0, 255]);
    }

    #[test]
    fn motion_and_rotation() {
        let mo = plan::motion(Motion { x: 0.25, y: 0.0, w: 0.5, h: 0.5, rot: 0.0, ..Default::default() }, (1920.0, 1080.0));
        let (cx, cy) = mo.apply(0.5, 0.5);
        assert!((cx - 0.75).abs() < 1e-9 && (cy - 0.5).abs() < 1e-9);
        let (x, y) = Affine::rotate(90.0).apply(1.0, 0.0);
        assert!(x.abs() < 1e-12 && (y - 1.0).abs() < 1e-12);
        let a = Affine::scale(2.0, 3.0).then(Affine::rotate(30.0)).then(Affine::translate(5.0, -1.0));
        let (x, y) = a.inverse().unwrap().apply(a.apply(0.3, 0.7).0, a.apply(0.3, 0.7).1);
        assert!((x - 0.3).abs() < 1e-12 && (y - 0.7).abs() < 1e-12);
    }

    #[test]
    fn decode_size_follows_the_size_on_screen() {
        let m = media(1920, 1080);
        let e = event(&m, PanCrop::default(), 1.0);
        let to = |w: usize| Affine::scale(w as f64, (w * 9 / 16) as f64);
        assert_eq!(decode_size(&e, to(960)), (960, 540));
        assert_eq!(decode_size(&e, to(1000)), (1920, 1080));
        assert_eq!(decode_size(&e, to(200)), (240, 136));
        let z = event(&m, PanCrop { w: 0.5, h: 0.5, ..Default::default() }, 1.0);
        assert_eq!(decode_size(&z, to(960)), (1920, 1080)); // 2x zoom needs the full source
    }

    /// A push on a picture-in-picture track stays inside the PiP, and adding a track FX does not change that.
    #[test]
    fn transitions_run_before_track_motion() {
        use crate::engine::plugins::{find, preset_eval};
        let m = media(1920, 1080);
        let push = preset_eval(find("milan.tr.push").unwrap(), None);
        let pip = Motion { w: 0.5, h: 0.5, ..Default::default() };
        let mix = || Item::Mix { a: Some(event(&m, PanCrop::default(), 1.0)), b: Some(event(&m, PanCrop::default(), 1.0)), p: 0.5, kind: Some(push.clone()) };
        let fast = compose(&FramePlan { fade: None, samples: Vec::new(), out_fx: Vec::new(), layers: vec![layer(vec![mix()], Composite::SourceAlpha, 1.0, pip)] }, &mut Halves(Color32::RED, Color32::BLUE), W, H, true, 1);
        for y in 0..H {
            for x in 0..W {
                let inside = (W / 4..3 * W / 4).contains(&x) && (H / 4..3 * H / 4).contains(&y);
                assert!(inside || px(&fast, x, y) == [0, 0, 0, 255], "{x},{y}: {:?}", px(&fast, x, y));
            }
        }
        assert_ne!(px(&fast, W / 2, H / 2), [0, 0, 0, 255]);
        let mut l = layer(vec![mix()], Composite::SourceAlpha, 1.0, pip);
        l.fx = vec![preset_eval(find("milan.fx.brightness_contrast").unwrap(), None)]; // identity
        let with_fx = compose(&FramePlan { fade: None, samples: Vec::new(), out_fx: Vec::new(), layers: vec![l] }, &mut Halves(Color32::RED, Color32::BLUE), W, H, true, 1);
        for (a, b) in fast.pixels.iter().zip(&with_fx.pixels) {
            assert!(a.to_array().into_iter().zip(b.to_array()).all(|(x, y)| x.abs_diff(y) <= 1), "{a:?} {b:?}");
        }
    }

    #[test]
    fn event_track_and_output_fx() {
        use crate::engine::plugins::{find, preset_eval};
        let m = media(1920, 1080);
        let inv = preset_eval(find("milan.fx.invert").unwrap(), None);
        let mut e = event(&m, PanCrop::default(), 1.0);
        e.fx = vec![inv.clone()];
        let fp = FramePlan { fade: None, samples: Vec::new(), out_fx: Vec::new(), layers: vec![layer(vec![Item::Event(e)], Composite::SourceAlpha, 1.0, Motion::default())] };
        let out = compose(&fp, &mut Halves(Color32::RED, Color32::BLUE), W, H, true, 1);
        assert_eq!((px(&out, 5, 50), px(&out, W - 5, 50)), ([0, 255, 255, 255], [255, 255, 0, 255]));
        // The same effect as a track FX, through the scratch layer.
        let mut l = layer(vec![Item::Event(event(&m, PanCrop::default(), 1.0))], Composite::SourceAlpha, 1.0, Motion::default());
        l.fx = vec![inv.clone()];
        let track = compose(&FramePlan { fade: None, samples: Vec::new(), out_fx: Vec::new(), layers: vec![l] }, &mut Halves(Color32::RED, Color32::BLUE), W, H, true, 1);
        assert!(track.pixels == out.pixels);
        // Video Output FX run on the finished frame, black background included.
        let white = compose(&FramePlan { fade: None, samples: Vec::new(), out_fx: vec![inv], layers: Vec::new() }, &mut Halves(Color32::RED, Color32::BLUE), W, H, true, 1);
        assert!(white.pixels.iter().all(|p| *p == Color32::WHITE));
    }
}
