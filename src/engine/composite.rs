//! CPU compositor: premultiplied 8-bit `Color32` layers, bottom to top, row stripes on scoped threads.
use eframe::egui::{Color32, ColorImage};
use std::sync::Arc;
use crate::engine::plan::{EventPlan, FramePlan, Item, SrcAt, TrackPlan};
use crate::engine::{transition, vfx};
use crate::project::Composite;

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

/// B(backdrop, source) for one channel of an opaque source.
fn blend_channel(mode: Composite, cd: u32, cs: u32) -> u32 {
    match mode {
        Composite::SourceAlpha => cs,
        Composite::Add => (cd + cs).min(255),
        Composite::Subtract => cd.saturating_sub(cs),
        Composite::Multiply => mul255(cs, cd),
        Composite::Screen => cs + cd - mul255(cs, cd),
        Composite::Overlay if cd < 128 => mul255(2 * cs, cd),
        Composite::Overlay => 255 - mul255(2 * (255 - cs), 255 - cd),
        Composite::Darken => cs.min(cd),
        Composite::Lighten => cs.max(cd),
        Composite::Difference => cs.abs_diff(cd),
    }
}

/// Composite `layer` onto the opaque `acc` with `mode` at `opacity`: (1 - a) Cd + a B(Cd, Cs).
fn blend(acc: &mut ColorImage, layer: &ColorImage, mode: Composite, opacity: f32, k: usize) {
    let o8 = (opacity.clamp(0.0, 1.0) * 255.0).round() as u32;
    let w = acc.size[0];
    par_rows(&mut acc.pixels, w, k, |y, row| {
        for (d, &s) in row.iter_mut().zip(&layer.pixels[y * w..(y + 1) * w]) {
            if s.a() == 0 {
                continue;
            }
            if mode == Composite::SourceAlpha {
                *d = over(*d, s, o8);
                continue;
            }
            let s = s.to_array().map(|c| mul255(c as u32, o8));
            let sa = s[3];
            if sa == 0 {
                continue;
            }
            let dd = d.to_array();
            let ch = |i: usize| {
                let cs = ((s[i] * 255 + sa / 2) / sa).min(255);
                (mul255(dd[i] as u32, 255 - sa) + mul255(sa, blend_channel(mode, dd[i] as u32, cs))).min(255) as u8
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
        Item::Mix { a, b, .. } => a.iter().chain([b]).collect(),
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
    for f in &e.fx {
        vfx::apply(Arc::make_mut(&mut img), f, k);
    }
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
                let (ia, ib) = (side(a.as_ref()), side(Some(b)));
                let m = if with_motion { Affine::scale(1.0 / w as f64, 1.0 / h as f64).then(to_out) } else { Affine::translate(0.0, 0.0) };
                draw(dst, &transition::mix(kind, &ia, &ib, *p, k), m, gain, bilinear, k);
            }
            _ => {
                // An unknown transition draws as a dissolve: A, then B at alpha p.
                let p = if let Item::Mix { p, .. } = item { *p } else { 1.0 };
                let evs = events(item);
                let last = evs.len() - 1;
                for (i, e) in evs.into_iter().enumerate() {
                    draw_event(dst, src, e, to_out, place_out, e.alpha * gain * if i == last { p } else { 1.0 }, bilinear, k);
                }
            }
        }
    }
}

/// Compose one frame `w` x `h`: opaque black, then each track bottom to top, then the Video Output FX.
pub fn compose(fp: &FramePlan, src: &mut dyn Sources, w: usize, h: usize, bilinear: bool, k: usize) -> ColorImage {
    let mut acc = ColorImage::new([w, h], Color32::BLACK);
    for l in &fp.layers {
        let draws: usize = l.items.iter().map(|i| events(i).len()).sum();
        // Fast path: draw straight into the accumulator, sampling each pixel once.
        if l.fx.is_empty() && l.mode == Composite::SourceAlpha && (l.opacity >= 1.0 || draws <= 1) {
            draw_items(&mut acc, src, l, l.opacity, bilinear, k, true);
        } else if l.fx.is_empty() {
            let mut layer = ColorImage::new([w, h], Color32::TRANSPARENT);
            draw_items(&mut layer, src, l, 1.0, bilinear, k, true);
            blend(&mut acc, &layer, l.mode, l.opacity, k);
        } else {
            // Track FX on the track's own frame, then Track Motion moves the result.
            let mut layer = ColorImage::new([w, h], Color32::TRANSPARENT);
            draw_items(&mut layer, src, l, 1.0, bilinear, k, false);
            for f in &l.fx {
                vfx::apply(&mut layer, f, k);
            }
            let mut moved = ColorImage::new([w, h], Color32::TRANSPARENT);
            draw(&mut moved, &layer, Affine::scale(1.0 / w as f64, 1.0 / h as f64).then(layer_to_out(l, w, h)), 1.0, bilinear, k);
            blend(&mut acc, &moved, l.mode, l.opacity, k);
        }
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
    acc
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
        EventPlan { src: SrcAt { m: m.clone(), sf: Some(0), generator: None }, place: plan::place(pc, (m.w as f64, m.h as f64), (1920.0, 1080.0)), alpha, fx: Vec::new() }
    }

    fn layer(items: Vec<Item>, mode: Composite, opacity: f32, motion: Motion) -> TrackPlan {
        TrackPlan { mode, opacity, motion: plan::motion(motion, (1920.0, 1080.0)), fx: Vec::new(), items }
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
        };
        for mode in [SourceAlpha, Add, Subtract, Multiply, Screen, Overlay, Darken, Lighten, Difference] {
            for d in (0..=255).step_by(5) {
                for s in (0..=255).step_by(3) {
                    let mut acc = ColorImage::new([1, 1], Color32::from_gray(d as u8));
                    let layer = ColorImage::new([1, 1], Color32::from_gray(s as u8));
                    blend(&mut acc, &layer, mode, 1.0, 1);
                    let want = (reference(mode, d as f64 / 255.0, s as f64 / 255.0) * 255.0).round() as i32;
                    let got = acc.pixels[0].r() as i32;
                    assert!((got - want).abs() <= 1, "{mode:?} d={d} s={s}: {got} vs {want}");
                }
            }
        }
        // Source alpha with a half-transparent source.
        let mut acc = ColorImage::new([1, 1], Color32::from_gray(200));
        blend(&mut acc, &ColorImage::new([1, 1], Color32::from_rgba_premultiplied(50, 50, 50, 128)), SourceAlpha, 1.0, 1);
        assert!((acc.pixels[0].r() as f64 - (50.0 + 200.0 * 127.0 / 255.0)).abs() <= 1.0);
    }

    #[test]
    fn fast_path_equals_general_path() {
        let m = media(1920, 1080);
        let fp = |mode| FramePlan { out_fx: Vec::new(), layers: vec![layer(vec![Item::Event(event(&m, PanCrop::default(), 0.8))], mode, 0.5, Motion::default())] };
        let fast = compose(&fp(Composite::SourceAlpha), &mut Halves(Color32::RED, Color32::BLUE), W, H, true, 1);
        // Normal through the scratch layer: blending a layer with the general formula at opacity 0.5.
        let mut general = ColorImage::new([W, H], Color32::BLACK);
        let mut scratch = ColorImage::new([W, H], Color32::TRANSPARENT);
        draw_items(&mut scratch, &mut Halves(Color32::RED, Color32::BLUE), &fp(Composite::SourceAlpha).layers[0], 1.0, true, 1, true);
        blend(&mut general, &scratch, Composite::SourceAlpha, 0.5, 1);
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
        blend(&mut acc1, &one, Composite::Overlay, 0.6, 1);
        blend(&mut acc5, &one, Composite::Overlay, 0.6, 5);
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

    #[test]
    fn two_layers_at_half_level() {
        let m = media(1920, 1080);
        let l = |c: Color32| (layer(vec![Item::Event(event(&m, PanCrop::default(), 1.0))], Composite::SourceAlpha, 0.5, Motion::default()), c);
        let (bottom, top) = (l(Color32::from_gray(200)), l(Color32::from_gray(100)));
        let mut acc = ColorImage::new([W, H], Color32::BLACK);
        for (layer, c) in [bottom, top] {
            let fp = FramePlan { out_fx: Vec::new(), layers: vec![layer] };
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
        let fp = FramePlan { out_fx: Vec::new(), layers: vec![layer(vec![Item::Event(event(&m, flip, 1.0))], Composite::SourceAlpha, 1.0, Motion::default())] };
        let out = compose(&fp, &mut Halves(Color32::RED, Color32::BLUE), W, H, true, 1);
        assert_eq!(px(&out, 5, 50), [0, 0, 255, 255]); // the source's right half is now on the left
        assert_eq!(px(&out, W - 5, 50), [255, 0, 0, 255]);
    }

    #[test]
    fn motion_and_rotation() {
        let mo = plan::motion(Motion { x: 0.25, y: 0.0, w: 0.5, h: 0.5, rot: 0.0 }, (1920.0, 1080.0));
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
        let mix = || Item::Mix { a: Some(event(&m, PanCrop::default(), 1.0)), b: event(&m, PanCrop::default(), 1.0), p: 0.5, kind: Some(push.clone()) };
        let fast = compose(&FramePlan { out_fx: Vec::new(), layers: vec![layer(vec![mix()], Composite::SourceAlpha, 1.0, pip)] }, &mut Halves(Color32::RED, Color32::BLUE), W, H, true, 1);
        for y in 0..H {
            for x in 0..W {
                let inside = (W / 4..3 * W / 4).contains(&x) && (H / 4..3 * H / 4).contains(&y);
                assert!(inside || px(&fast, x, y) == [0, 0, 0, 255], "{x},{y}: {:?}", px(&fast, x, y));
            }
        }
        assert_ne!(px(&fast, W / 2, H / 2), [0, 0, 0, 255]);
        let mut l = layer(vec![mix()], Composite::SourceAlpha, 1.0, pip);
        l.fx = vec![preset_eval(find("milan.fx.brightness_contrast").unwrap(), None)]; // identity
        let with_fx = compose(&FramePlan { out_fx: Vec::new(), layers: vec![l] }, &mut Halves(Color32::RED, Color32::BLUE), W, H, true, 1);
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
        let fp = FramePlan { out_fx: Vec::new(), layers: vec![layer(vec![Item::Event(e)], Composite::SourceAlpha, 1.0, Motion::default())] };
        let out = compose(&fp, &mut Halves(Color32::RED, Color32::BLUE), W, H, true, 1);
        assert_eq!((px(&out, 5, 50), px(&out, W - 5, 50)), ([0, 255, 255, 255], [255, 255, 0, 255]));
        // The same effect as a track FX, through the scratch layer.
        let mut l = layer(vec![Item::Event(event(&m, PanCrop::default(), 1.0))], Composite::SourceAlpha, 1.0, Motion::default());
        l.fx = vec![inv.clone()];
        let track = compose(&FramePlan { out_fx: Vec::new(), layers: vec![l] }, &mut Halves(Color32::RED, Color32::BLUE), W, H, true, 1);
        assert!(track.pixels == out.pixels);
        // Video Output FX run on the finished frame, black background included.
        let white = compose(&FramePlan { out_fx: vec![inv], layers: Vec::new() }, &mut Halves(Color32::RED, Color32::BLUE), W, H, true, 1);
        assert!(white.pixels.iter().all(|p| *p == Color32::WHITE));
    }
}
