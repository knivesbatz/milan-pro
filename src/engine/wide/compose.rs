//! R40b: the CPU compositor in float: `composite.rs`'s steps (the same order, the same geometry, sampled with the
//! same 16.16 source positions) on `WImage`s, with the blend modes in f32 and nothing rounded to 8 bits. The GPU's
//! float passes (`engine/gpu/wide.rs`) follow this code.
use eframe::egui::ColorImage;
use std::sync::Arc;
use super::{WImage, Working, to8, via8, via8_2};
use crate::engine::composite::{Affine, ONE, Sources, bounds, decode_size, event_to_size, events, layer_to_out, spans, whole_pixel};
use crate::engine::plan::{EventPlan, FramePlan, Item, Nested, SrcAt, TrackPlan};
use crate::engine::plugins::FxEval;
use crate::engine::{raster, vfx};
use crate::project::{Composite, Halo};

type Px = [f32; 4];

fn par(img: &mut WImage, k: usize, f: impl Fn(usize, &mut [Px]) + Sync) {
    let w = img.size[0];
    super::par_chunks(&mut img.px, w, k, f);
}

/// Premultiplied source-over of `s` scaled by `a` onto `d`. Alpha is computed as `da + sa (1 - da)`, so an opaque
/// backdrop stays exactly opaque (the GPU's half floats would round 1 - 1 ulp down a step).
#[inline]
pub fn over(d: Px, s: Px, a: f32) -> Px {
    let k = 1.0 - s[3] * a;
    [s[0] * a + d[0] * k, s[1] * a + d[1] * k, s[2] * a + d[2] * k, d[3] + s[3] * a * (1.0 - d[3])]
}

/// Bilinear sample at 16.16 position (u, v) (pixel centres at integers), clamped to the edge pixels.
#[inline]
fn bilerp(src: &WImage, u: i64, v: i64) -> Px {
    let [sw, sh] = src.size;
    let (iu, iv) = (u >> 16, v >> 16);
    let (fu, fv) = ((u & 0xffff) as f32 / 65536.0, (v & 0xffff) as f32 / 65536.0);
    let (x0, x1) = (iu.clamp(0, sw as i64 - 1) as usize, (iu + 1).clamp(0, sw as i64 - 1) as usize);
    let (y0, y1) = (iv.clamp(0, sh as i64 - 1) as usize, (iv + 1).clamp(0, sh as i64 - 1) as usize);
    let p = |x: usize, y: usize| src.px[y * sw + x];
    let (p00, p10, p01, p11) = (p(x0, y0), p(x1, y0), p(x0, y1), p(x1, y1));
    std::array::from_fn(|i| {
        let top = p00[i] + (p10[i] - p00[i]) * fu;
        let bot = p01[i] + (p11[i] - p01[i]) * fu;
        top + (bot - top) * fv
    })
}

/// `composite::draw` in float.
pub fn draw(dst: &mut WImage, src: &WImage, m: Affine, alpha: f32, bilinear: bool, k: usize) {
    let a = alpha.clamp(0.0, 1.0);
    let Some(inv) = m.inverse() else { return };
    let ([dw, _], [sw, sh]) = (dst.size, src.size);
    if a <= 0.0 || sw == 0 || sh == 0 {
        return;
    }
    let Some([x0, x1, y0, y1]) = bounds(m, src.size, dst.size) else { return };
    if let Some((ox, oy)) = whole_pixel(m) {
        if let Some([x0, x1, y0, y1]) = spans((ox, oy), src.size, dst.size) {
            super::par_chunks(&mut dst.px[y0 * dw..y1 * dw], dw, k, |yr, row| {
                let srow = &src.px[((y0 + yr) as i64 - oy) as usize * sw..][..sw];
                for (x, px) in row.iter_mut().enumerate().take(x1).skip(x0) {
                    *px = over(*px, srow[(x as i64 - ox) as usize], a);
                }
            });
        }
        return;
    }
    let (du, dv) = ((inv.a * ONE).round() as i64, (inv.c * ONE).round() as i64);
    let (umax, vmax) = ((sw as i64) << 16, (sh as i64) << 16);
    super::par_chunks(&mut dst.px[y0 * dw..y1 * dw], dw, k, |yr, row| {
        let (u0, v0) = inv.apply(x0 as f64 + 0.5, (y0 + yr) as f64 + 0.5);
        let (mut u, mut v) = ((u0 * ONE).floor() as i64, (v0 * ONE).floor() as i64);
        for px in &mut row[x0..x1] {
            if u >= 0 && v >= 0 && u < umax && v < vmax {
                let s = if bilinear { bilerp(src, u - 32768, v - 32768) } else { src.px[(v >> 16) as usize * sw + (u >> 16) as usize] };
                *px = over(*px, s, a);
            }
            u += du;
            v += dv;
        }
    });
}

/// B(backdrop, source) of a separable mode on one channel; `hi` is the working space's brightest value (1 when video
/// coded, so these are `composite`'s formulas; linear light keeps highlights through Add and Dodge).
pub fn separable(mode: Composite, b: f32, s: f32, hi: f32) -> f32 {
    use Composite::*;
    match mode {
        Add => (b + s).min(hi),
        Subtract => (b - s).max(0.0),
        Multiply => b * s,
        Screen => b + s - b * s,
        Overlay => separable(HardLight, s, b, hi),
        Darken => b.min(s),
        Lighten => b.max(s),
        Difference => (b - s).abs(),
        HardLight if s <= 0.5 => 2.0 * b * s,
        HardLight => 1.0 - 2.0 * (1.0 - b) * (1.0 - s),
        Dodge if b <= 0.0 => 0.0,
        Dodge if s >= 1.0 => hi.min(1.0e6),
        Dodge => (b / (1.0 - s)).min(hi),
        Burn if b >= 1.0 => 1.0,
        Burn if s <= 0.0 => 0.0,
        Burn => 1.0 - ((1.0 - b) / s).min(1.0),
        DifferenceSquared => (b - s) * (b - s),
        SoftLight if s <= 0.5 => b - (1.0 - 2.0 * s) * b * (1.0 - b),
        SoftLight => {
            let d = if b <= 0.25 { ((16.0 * b - 12.0) * b + 4.0) * b } else { b.max(0.0).sqrt() };
            b + (2.0 * s - 1.0) * (d - b)
        }
        Exclusion => b + s - 2.0 * b * s,
        HardMix => (b + s >= 1.0) as u8 as f32,
        _ => s,
    }
}

pub fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    let c = c.map(|v| v + d);
    let l = lum(c);
    let (n, x) = (c[0].min(c[1]).min(c[2]), c[0].max(c[1]).max(c[2]));
    c.map(|v| if n < 0.0 && l > n { l + (v - l) * l / (l - n) } else { v }).map(|v| if x > 1.0 && x > l { l + (v - l) * (1.0 - l) / (x - l) } else { v })
}

fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

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

fn mix_mode(mode: Composite, cb: [f32; 3], cs: [f32; 3], hi: f32) -> [f32; 3] {
    match mode {
        Composite::Hue => set_lum(set_sat(cs, sat(cb)), lum(cb)),
        Composite::Saturation => set_lum(set_sat(cb, sat(cs)), lum(cb)),
        Composite::Color => set_lum(cs, lum(cb)),
        Composite::Luminosity => set_lum(cb, lum(cs)),
        _ => std::array::from_fn(|i| separable(mode, cb[i], cs[i], hi)),
    }
}

/// `composite::blend_px` in float: the W3C general formula for any backdrop, Cut, Multiply (Mask) in a group.
pub fn blend_px(d: Px, s: Px, mode: Composite, o: f32, group: bool, (lo, hi): (f32, f32)) -> Px {
    let sp = s.map(|v| v * o);
    let (ab, a) = (d[3], sp[3]);
    let straight = |c: Px, a: f32| if a > 0.0 { [c[0] / a, c[1] / a, c[2] / a] } else { [0.0; 3] };
    match mode {
        Composite::SourceAlpha | Composite::ThreeD => return over(d, s, o),
        Composite::Cut => {
            let k = 1.0 - a;
            return [d[0] * k, d[1] * k, d[2] * k, if ab >= 1.0 { 1.0 } else { ab * k }];
        }
        Composite::Multiply if group => {
            let m = lum(straight(sp, a)).min(1.0) * a + (1.0 - o);
            return d.map(|v| v * m);
        }
        _ if a <= 0.0 => return d,
        _ => {}
    }
    let b = mix_mode(mode, straight(d, ab), straight(sp, a), hi);
    let c: [f32; 3] = std::array::from_fn(|i| sp[i] * (1.0 - ab) + d[i] * (1.0 - a) + a * ab * b[i].clamp(lo, hi));
    [c[0], c[1], c[2], ab + a * (1.0 - ab)]
}

/// `composite::blend` in float. Custom runs its compositor on 8 bits.
#[allow(clippy::too_many_arguments)]
pub fn blend(acc: &mut WImage, layer: &WImage, mode: Composite, opacity: f32, compositor: Option<&FxEval>, group: bool, k: usize, wk: &Working) {
    if mode == Composite::Custom {
        match compositor {
            Some(c) if crate::engine::plugins::find(c.id).is_some() => {
                let l8 = to8(layer);
                return via8(acc, |a| vfx::compositor(a, &l8, c, opacity, k));
            }
            _ => return blend(acc, layer, Composite::SourceAlpha, opacity, None, group, k, wk),
        }
    }
    let o = opacity.clamp(0.0, 1.0);
    let mask = group && mode == Composite::Multiply;
    let (w, range) = (acc.size[0], wk.range());
    par(acc, k, |y, row| {
        for (d, &s) in row.iter_mut().zip(&layer.px[y * w..(y + 1) * w]) {
            if s[3] <= 0.0 && !mask {
                continue;
            }
            *d = blend_px(*d, s, mode, o, group, range);
        }
    });
}

/// Resampling: `a` becomes (1 - w) a + w b.
pub fn blend_frames(a: &mut WImage, b: &WImage, w: f32, k: usize) {
    if a.size != b.size {
        return;
    }
    let (t, cols) = (w.clamp(0.0, 1.0), a.size[0]);
    par(a, k, |y, row| {
        for (p, q) in row.iter_mut().zip(&b.px[y * cols..(y + 1) * cols]) {
            *p = std::array::from_fn(|i| p[i] + (q[i] - p[i]) * t);
        }
    });
}

/// Reduce Interlace Flicker: (above + 2 x row + below) / 4.
pub fn reduce_flicker(img: &mut WImage, k: usize) {
    let [w, h] = img.size;
    if h < 2 {
        return;
    }
    let src = img.px.clone();
    par(img, k, |y, row| {
        let line = |y: usize| &src[y * w..(y + 1) * w];
        let (up, mid, down) = (line(y.saturating_sub(1)), line(y), line((y + 1).min(h - 1)));
        for (x, p) in row.iter_mut().enumerate() {
            *p = std::array::from_fn(|i| (up[x][i] + 2.0 * mid[x][i] + down[x][i]) * 0.25);
        }
    });
}

/// `a` to `b` by `t`.
pub fn lerp(a: Px, b: Px, t: f32) -> Px {
    if t <= 0.0 {
        return a;
    }
    if t >= 1.0 {
        return b;
    }
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

/// Fade to Color: each pixel `t` of the way to `color` (working values) at its own alpha.
fn fade_to(img: &mut WImage, c: [f32; 3], t: f32, k: usize) {
    par(img, k, |_, row| {
        for p in row.iter_mut() {
            let a = p[3];
            *p = lerp(*p, [c[0] * a, c[1] * a, c[2] * a, a], t);
        }
    });
}

// ---- blur ----

fn box_row(src: &[Px], dst: &mut [Px], r: usize) {
    let n = src.len() as isize;
    let at = |i: isize| src[i.clamp(0, n - 1) as usize];
    let (r, d) = (r as isize, (2 * r + 1) as f32);
    let mut sum = [0.0f64; 4];
    for i in -r..=r {
        let p = at(i);
        (0..4).for_each(|c| sum[c] += p[c] as f64);
    }
    for (x, out) in dst.iter_mut().enumerate() {
        *out = sum.map(|s| (s / d as f64) as f32);
        let (add, sub) = (at(x as isize + r + 1), at(x as isize - r));
        (0..4).for_each(|c| sum[c] += add[c] as f64 - sub[c] as f64);
    }
}

fn box3_rows(img: &mut WImage, r: usize, k: usize) {
    par(img, k, |_, row| {
        let mut tmp = row.to_vec();
        for _ in 0..3 {
            box_row(&tmp, row, r);
            tmp.copy_from_slice(row);
        }
    });
}

fn transpose(img: &WImage) -> WImage {
    let [w, h] = img.size;
    WImage { size: [h, w], px: (0..w * h).map(|i| img.px[(i % h) * w + i / h]).collect() }
}

/// `vfx::blur` in float: three box passes per axis.
pub fn blur(img: &mut WImage, rx: usize, ry: usize, k: usize) {
    if rx > 0 {
        box3_rows(img, rx, k);
    }
    if ry > 0 {
        let mut t = transpose(img);
        box3_rows(&mut t, ry, k);
        *img = transpose(&t);
    }
}

/// Gaussian Blur's soft blur (`vfx::soft_blur`) in float.
fn soft_blur(img: &mut WImage, fx: f32, fy: f32, k: usize) {
    for (f, along_x) in [(fx, true), (fy, false)] {
        if f <= 0.0 {
            continue;
        }
        let r = (f.round() as usize).max(1);
        let before = (f < 1.0).then(|| img.clone());
        if along_x { blur(img, r, 0, k) } else { blur(img, 0, r, k) }
        if let Some(b) = before {
            img.px.iter_mut().zip(&b.px).for_each(|(p, q)| *p = lerp(*q, *p, f));
        }
    }
}

// ---- FX ----

/// `vfx::ColorOp` on straight values: its matrix, then its LUT read between entries (so a gradient stays smooth), a
/// mix, premultiplied again. Values outside 0..1 are clamped first, as the LUT covers 0..1.
fn color_op(img: &mut WImage, op: &vfx::ColorOp, k: usize) {
    if op.is_identity() {
        return;
    }
    let lut = |c: usize, v: f32| {
        let x = v.clamp(0.0, 1.0) * 255.0;
        let i = (x as usize).min(254);
        let t = x - i as f32;
        (op.lut[c][i] as f32 + (op.lut[c][i + 1] as f32 - op.lut[c][i] as f32) * t) / 255.0
    };
    par(img, k, |_, row| {
        for p in row.iter_mut() {
            let a = p[3];
            if a <= 0.0 {
                continue;
            }
            let orig = [p[0] / a, p[1] / a, p[2] / a].map(|v| v.clamp(0.0, 1.0));
            let mut c = orig;
            if let Some(m) = &op.m {
                c = std::array::from_fn(|i| (m[i][0] * c[0] + m[i][1] * c[1] + m[i][2] * c[2]).clamp(0.0, 1.0));
            }
            c = std::array::from_fn(|i| lut(i, c[i]));
            if op.mix < 1.0 {
                c = std::array::from_fn(|i| orig[i] + (c[i] - orig[i]) * op.mix);
            }
            *p = [c[0] * a, c[1] * a, c[2] * a, a];
        }
    });
}

/// Color Grading on straight values (`vfx::Grade::rgb`: its LUTs, wheels, curves and matrix in float).
fn grade(img: &mut WImage, g: &vfx::Grade, k: usize) {
    par(img, k, |_, row| {
        for p in row.iter_mut().filter(|p| p[3] > 0.0) {
            let a = p[3];
            let c = g.rgb([p[0] / a, p[1] / a, p[2] / a]);
            *p = [c[0] * a, c[1] * a, c[2] * a, a];
        }
    });
}

/// FX `e` on `img` (working values): a float kernel where there is one, else the 8-bit FX through `via8`.
pub fn apply_fx(img: &mut WImage, e: &FxEval, k: usize) {
    let [w, h] = img.size;
    if w == 0 || h == 0 {
        return;
    }
    if let Some(op) = vfx::color_op_for(e) {
        return color_op(img, &op, k);
    }
    match e.id {
        "milan.fx.gaussian_blur" => {
            let (fx, fy) = vfx::blur_radii(e, h);
            soft_blur(img, fx, fy, k)
        }
        crate::engine::plugins::GRADING => grade(img, &vfx::Grade::new(e), k),
        id if id.starts_with(crate::engine::ofx::PREFIX) => {
            // R40b: an OFX plug-in gets the float picture at its own depth.
            crate::engine::ofx::apply_wide(img, e);
        }
        _ => via8(img, |i| vfx::apply(i, e, k)),
    }
}

/// Transition `kind` from A into B at p: the dissolve in float, an OFX transition at its float depth, the others on 8
/// bits through `via8_2`.
pub fn mix(kind: &FxEval, a: &WImage, b: &WImage, p: f32, k: usize) -> WImage {
    if p <= 0.0 {
        return a.clone();
    }
    if p >= 1.0 || a.size != b.size {
        return b.clone();
    }
    if kind.id.starts_with(crate::engine::ofx::PREFIX) {
        if let Some(out) = crate::engine::ofx::transition_wide(kind, a, b, p) {
            return out;
        }
        return dissolve(a, b, p, k);
    }
    if kind.id == "milan.tr.dissolve" && !kind.on("additive") {
        return dissolve(a, b, p, k);
    }
    via8_2(a, b, p, |a8, b8| crate::engine::transition::mix(kind, a8, b8, p, k))
}

fn dissolve(a: &WImage, b: &WImage, p: f32, k: usize) -> WImage {
    let mut out = a.clone();
    blend_frames(&mut out, b, p, k);
    out
}

/// Pan/Crop masks (`raster::apply`) in float.
fn masks(img: &mut WImage, m: &[raster::MaskPlan], k: usize) {
    let [w, h] = img.size;
    if m.is_empty() || w == 0 || h == 0 {
        return;
    }
    let v = raster::mask_values(m, w, h, k);
    par(img, k, |y, row| {
        for (p, &c) in row.iter_mut().zip(&v[y * w..]) {
            let c = c.clamp(0.0, 1.0);
            *p = p.map(|x| x * c);
        }
    });
}

/// What an 8-bit step that reads sources (MVFX Echo, Stutter, Datamosh) sees: the float pictures as 8 bits.
struct Narrow<'a> {
    src: &'a mut dyn Sources,
    wk: &'a Working,
}

impl Sources for Narrow<'_> {
    fn get(&mut self, s: &SrcAt, w: u32, h: u32) -> Option<Arc<ColorImage>> {
        self.src.get_wide(s, w, h, self.wk).map(|i| Arc::new(to8(&i)))
    }
}

/// One event's source through resampling, the flicker filter, its event FX and masks (`composite::event_image`).
pub fn event_image(src: &mut dyn Sources, e: &EventPlan, to_out: Affine, bilinear: bool, k: usize, wk: &Working) -> Option<Arc<WImage>> {
    let (dw, dh) = decode_size(e, to_out);
    let mut img = match e.src.nested.as_deref() {
        Some(Nested::Frame(fs)) => Some(Arc::new(compose_nested(fs, src, dw, dh, bilinear, k, wk))),
        _ => src.get_wide(&e.src, dw, dh, wk),
    }?;
    if let Some((n, w)) = e.next {
        if let Some(b) = src.get_wide(&SrcAt { m: e.src.m.clone(), sf: Some(n), generator: None, nested: None }, dw, dh, wk) {
            blend_frames(Arc::make_mut(&mut img), &b, w, k);
        }
    }
    if e.flicker {
        reduce_flicker(Arc::make_mut(&mut img), k);
    }
    if crate::engine::mvfx::needs_sources(&e.fx) {
        // The chain reads earlier frames: all of it on 8 bits, as `mvfx::event_fx` runs it.
        let mut narrow = Narrow { src, wk };
        via8(Arc::make_mut(&mut img), |i| crate::engine::mvfx::event_fx(i, e, &mut narrow, (dw, dh), k));
    } else {
        for f in &e.fx {
            apply_fx(Arc::make_mut(&mut img), f, k);
        }
    }
    masks(Arc::make_mut(&mut img), &e.masks, k);
    Some(img)
}

fn compose_nested(fs: &[(FramePlan, f32)], src: &mut dyn Sources, w: u32, h: u32, bilinear: bool, k: usize, wk: &Working) -> WImage {
    let frames: Vec<WImage> = fs.iter().map(|f| compose(&f.0, src, w as usize, h as usize, bilinear, k, wk)).collect();
    if frames.len() == 1 { frames.into_iter().next().expect("one frame") } else { mean(&frames, &fs.iter().map(|f| f.1).collect::<Vec<_>>()) }
}

#[allow(clippy::too_many_arguments)]
fn draw_event(dst: &mut WImage, src: &mut dyn Sources, e: &EventPlan, to_out: Affine, place_out: Affine, alpha: f32, bilinear: bool, k: usize, wk: &Working) {
    if alpha <= 0.0 {
        return;
    }
    if let Some(img) = event_image(src, e, to_out, bilinear, k, wk) {
        draw(dst, &img, event_to_size(img.size, e, place_out), alpha, bilinear, k);
    }
}

/// `composite::transition_image` in float.
#[allow(clippy::too_many_arguments)]
pub fn transition_image(src: &mut dyn Sources, a: Option<&EventPlan>, b: Option<&EventPlan>, p: f32, kind: &FxEval, to_out: Affine, [w, h]: [usize; 2], bilinear: bool, k: usize, wk: &Working) -> WImage {
    let frame = Affine::scale(w as f64, h as f64);
    let mut side = |e: Option<&EventPlan>| {
        let mut img = WImage::new([w, h], [0.0; 4]);
        if let Some(e) = e {
            draw_event(&mut img, src, e, to_out, frame, e.alpha, bilinear, k, wk);
        }
        img
    };
    let (ia, ib) = (side(a), side(b));
    mix(kind, &ia, &ib, p, k)
}

#[allow(clippy::too_many_arguments)]
fn draw_items(dst: &mut WImage, src: &mut dyn Sources, l: &TrackPlan, gain: f32, bilinear: bool, k: usize, with_motion: bool, wk: &Working) {
    let [w, h] = dst.size;
    let to_out = layer_to_out(l, w, h);
    let place_out = if with_motion { to_out } else { Affine::scale(w as f64, h as f64) };
    for item in &l.items {
        match item {
            Item::Mix { a, b, p, kind: Some(kind) } => {
                let img = transition_image(src, a.as_ref(), b.as_ref(), *p, kind, to_out, [w, h], bilinear, k, wk);
                let m = if with_motion { Affine::scale(1.0 / w as f64, 1.0 / h as f64).then(to_out) } else { Affine::translate(0.0, 0.0) };
                draw(dst, &img, m, gain, bilinear, k);
            }
            _ => {
                let (p, out) = if let Item::Mix { p, b, .. } = item { (*p, b.is_none()) } else { (1.0, false) };
                let evs = events(item);
                let last = evs.len().saturating_sub(1);
                for (i, e) in evs.into_iter().enumerate() {
                    let wt = if out { 1.0 - p } else if i == last { p } else { 1.0 };
                    draw_event(dst, src, e, to_out, place_out, e.alpha * gain * wt, bilinear, k, wk);
                }
            }
        }
    }
}

fn inv3(m: [f64; 9]) -> Option<[f64; 9]> {
    let [a, b, c, d, e, f, g, h, i] = m;
    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    (det.abs() > 1e-12).then(|| [e * i - f * h, c * h - b * i, b * f - c * e, f * g - d * i, a * i - c * g, c * d - a * f, d * h - e * g, b * g - a * h, a * e - b * d].map(|v| v / det))
}

/// `composite::draw_persp` in float.
fn draw_persp(dst: &mut WImage, src: &WImage, hm: [f64; 9], bilinear: bool, k: usize) {
    let ([dw, dh], [sw, sh]) = (dst.size, src.size);
    let Some(q) = inv3(hm) else { return };
    par(dst, k, |y, row| {
        let v = (y as f64 + 0.5) / dh as f64;
        for (x, px) in row.iter_mut().enumerate() {
            let u = (x as f64 + 0.5) / dw as f64;
            let z = q[6] * u + q[7] * v + q[8];
            if z <= 1e-9 {
                continue;
            }
            let (su, sv) = ((q[0] * u + q[1] * v + q[2]) / z * sw as f64, (q[3] * u + q[4] * v + q[5]) / z * sh as f64);
            if su < 0.0 || sv < 0.0 || su >= sw as f64 || sv >= sh as f64 {
                continue;
            }
            let s = if bilinear { bilerp(src, (su * 65536.0) as i64 - 32768, (sv * 65536.0) as i64 - 32768) } else { src.px[sv as usize * sw + su as usize] };
            *px = over(*px, s, 1.0);
        }
    });
}

/// 2D Shadow / 2D Glow (`composite::add_halos`) in float.
fn add_halos(img: &mut WImage, halos: [Option<Halo>; 2], k: usize, wk: &Working) {
    let [w, h] = img.size;
    let mut under: Option<WImage> = None;
    for halo in halos.into_iter().flatten() {
        let i = halo.intensity.clamp(0.0, 1.0);
        let c = wk.color8(halo.color.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8));
        let (dx, dy) = ((halo.x * w as f32).round() as i64, (halo.y * h as f32).round() as i64);
        let mut sh = WImage::new([w, h], [0.0; 4]);
        par(&mut sh, k, |y, row| {
            let sy = y as i64 - dy;
            if !(0..h as i64).contains(&sy) {
                return;
            }
            for (x, p) in row.iter_mut().enumerate() {
                let sx = x as i64 - dx;
                if (0..w as i64).contains(&sx) {
                    let a = img.px[sy as usize * w + sx as usize][3] * i;
                    *p = [c[0] * a, c[1] * a, c[2] * a, a];
                }
            }
        });
        let r = (halo.blur.max(0.0) * h as f32).round() as usize;
        blur(&mut sh, r, r, k);
        match &mut under {
            Some(u) => u.px.iter_mut().zip(&sh.px).for_each(|(d, s)| *d = over(*d, *s, 1.0)),
            None => under = Some(sh),
        }
    }
    if let Some(mut u) = under {
        u.px.iter_mut().zip(&img.px).for_each(|(d, s)| *d = over(*d, *s, 1.0));
        *img = u;
    }
}

/// `composite::adjust` in float.
pub fn adjust(dst: &mut WImage, fx: &[FxEval], a: f32, k: usize) {
    let mut img = dst.clone();
    for f in fx {
        apply_fx(&mut img, f, k);
    }
    if a >= 1.0 {
        *dst = img;
        return;
    }
    let w = dst.size[0];
    par(dst, k, |y, row| {
        for (x, p) in row.iter_mut().enumerate() {
            *p = lerp(*p, img.px[y * w + x], a);
        }
    });
}

fn layer_onto(dst: &mut WImage, src: &mut dyn Sources, l: &TrackPlan, bilinear: bool, k: usize, group: bool, wk: &Working) {
    for (fx, a) in &l.adjust {
        adjust(dst, fx, *a, k);
    }
    if l.items.is_empty() {
        return;
    }
    let [w, h] = dst.size;
    let draws: usize = l.items.iter().map(|i| events(i).len()).sum();
    let halo = l.shadow.is_some() || l.glow.is_some();
    let plain = l.fx.is_empty() && l.fade.is_none() && !halo && l.persp.is_none();
    if plain && matches!(l.mode, Composite::SourceAlpha | Composite::ThreeD) && (l.opacity >= 1.0 || draws <= 1) {
        draw_items(dst, src, l, l.opacity, bilinear, k, true, wk);
        return;
    }
    let layer = layer_image(src, l, [w, h], bilinear, k, wk);
    blend(dst, &layer, l.mode, l.opacity, l.compositor.as_ref(), group, k, wk);
}

/// `composite::layer_image` in float.
pub fn layer_image(src: &mut dyn Sources, l: &TrackPlan, [w, h]: [usize; 2], bilinear: bool, k: usize, wk: &Working) -> WImage {
    let mut layer = WImage::new([w, h], [0.0; 4]);
    let direct = l.fx.is_empty() && l.persp.is_none();
    draw_items(&mut layer, src, l, 1.0, bilinear, k, direct, wk);
    if !direct {
        for f in &l.fx {
            apply_fx(&mut layer, f, k);
        }
        let mut moved = WImage::new([w, h], [0.0; 4]);
        match l.persp {
            Some(hm) => draw_persp(&mut moved, &layer, hm, bilinear, k),
            None => draw(&mut moved, &layer, Affine::scale(1.0 / w as f64, 1.0 / h as f64).then(layer_to_out(l, w, h)), 1.0, bilinear, k),
        }
        layer = moved;
    }
    if l.shadow.is_some() || l.glow.is_some() {
        add_halos(&mut layer, [l.shadow, l.glow], k, wk);
    }
    if let Some((c, t)) = l.fade {
        fade_to(&mut layer, wk.color8(c), t, k);
    }
    layer
}

/// `composite::compose` in float, in working space `wk`.
pub fn compose(fp: &FramePlan, src: &mut dyn Sources, w: usize, h: usize, bilinear: bool, k: usize, wk: &Working) -> WImage {
    let mut acc = WImage::new([w, h], if fp.alpha { [0.0; 4] } else { [0.0, 0.0, 0.0, 1.0] });
    if !fp.alpha {
        // Black in the working encoding (the log encodings code black above 0).
        let b = wk.encode(0.0);
        acc.px.fill([b, b, b, 1.0]);
    }
    let mut group: Option<WImage> = None;
    let blank = || WImage::new([w, h], [0.0; 4]);
    for l in &fp.layers {
        if l.child {
            layer_onto(group.get_or_insert_with(blank), src, l, bilinear, k, true, wk);
            continue;
        }
        let Some((mode, motion)) = l.parent else {
            if let Some(g) = group.take() {
                blend(&mut acc, &g, Composite::SourceAlpha, 1.0, None, false, k, wk);
            }
            layer_onto(&mut acc, src, l, bilinear, k, false, wk);
            continue;
        };
        let mut g = group.take().unwrap_or_else(blank);
        if !l.items.is_empty() || !l.adjust.is_empty() {
            layer_onto(&mut g, src, l, bilinear, k, true, wk);
        }
        if whole_pixel(motion).is_some_and(|o| o == (0, 0)) {
            blend(&mut acc, &g, mode, 1.0, None, false, k, wk);
        } else {
            let mut moved = blank();
            draw(&mut moved, &g, Affine::scale(1.0 / w as f64, 1.0 / h as f64).then(motion).then(Affine::scale(w as f64, h as f64)), 1.0, bilinear, k);
            blend(&mut acc, &moved, mode, 1.0, None, false, k, wk);
        }
    }
    if let Some(g) = group {
        blend(&mut acc, &g, Composite::SourceAlpha, 1.0, None, false, k, wk);
    }
    finish(&mut acc, fp, k, wk);
    acc
}

/// After the tracks: the Video Output FX (the output is opaque after them), then the bus's Fade to Color.
pub fn finish(acc: &mut WImage, fp: &FramePlan, k: usize, wk: &Working) {
    if !fp.out_fx.is_empty() {
        for f in &fp.out_fx {
            apply_fx(acc, f, k);
        }
        for p in &mut acc.px {
            p[3] = 1.0;
        }
    }
    if let Some((c, t)) = fp.fade {
        fade_to(acc, wk.color8(c), t, k);
    }
}

/// Motion blur / supersampling: the weighted mean of `frames`.
pub fn mean(frames: &[WImage], w: &[f32]) -> WImage {
    let total: f32 = w.iter().sum::<f32>().max(1e-6);
    let mut out = frames[0].clone();
    for (i, p) in out.px.iter_mut().enumerate() {
        let mut s = [0.0f32; 4];
        for (f, &q) in frames.iter().zip(w) {
            (0..4).for_each(|j| s[j] += f.px[i][j] * q / total);
        }
        *p = s;
    }
    out
}
