//! Generated media (`MediaItem.generator`): solid colour, gradient, checkerboard, noise, text, credit roll and test
//! pattern, at any size.
use ab_glyph::{Font, FontArc, GlyphId, ScaleFont, point};
use eframe::egui::{Color32, ColorImage};
use crate::engine::composite::{over, par_rows, stripes};
use crate::engine::fonts;
use crate::engine::plugins::FxEval;
use crate::engine::vfx::{blur, premul, transpose};

/// Only animated noise and the credit roll change with time; everything else renders the same at every t (so it
/// caches).
pub fn animated(e: &FxEval) -> bool {
    (e.id == "milan.gen.noise" && e.f("speed") > 0.0) || e.id == CREDITS
}

/// R18a: the Credit Roll scrolls over its generated media's length, which `engine::plan` adds to its values under
/// `LENGTH` (the generator's parameters do not hold it); without it (thumbnails) the credits stand centred.
pub const CREDITS: &str = "milan.gen.credits";
pub const LENGTH: &str = "length";

/// Generator `e` at `w` x `h`, at its own time `t` (seconds); an unknown id is transparent.
pub fn render(e: &FxEval, w: u32, h: u32, t: f64) -> ColorImage {
    let (w, h) = (w.max(1) as usize, h.max(1) as usize);
    let hf = h as f32;
    match e.id {
        "milan.gen.solid" => ColorImage::new([w, h], premul(e.c("color"))),
        "milan.gen.gradient" => gradient(e, w, h),
        "milan.gen.checkerboard" => {
            let cell = (e.f("size") * hf).round().max(1.0) as usize;
            let (a, b) = (premul(e.c("color1")), premul(e.c("color2")));
            fill(w, h, |x, y| if (x / cell + y / cell) % 2 == 0 { a } else { b })
        }
        "milan.gen.noise" => {
            let cell = (e.f("scale") * hf).max(1.0);
            let oct = e.f("octaves").round().clamp(1.0, 8.0) as u32;
            let seed = e.f("seed").round() as u32;
            let z = (t * e.f("speed") as f64) as f32;
            let (a, b) = (e.c("color1"), e.c("color2"));
            fill(w, h, |x, y| {
                let v = fbm(x as f32 / cell, y as f32 / cell, z, oct, seed);
                premul(std::array::from_fn(|i| a[i] + (b[i] - a[i]) * v))
            })
        }
        "milan.gen.text" => text(e, w, h),
        CREDITS => credits(e, w, h, t),
        "milan.gen.test_pattern" => test_pattern(e.i("type"), w, h),
        _ => ColorImage::new([w, h], Color32::TRANSPARENT),
    }
}

pub(crate) fn fill(w: usize, h: usize, f: impl Fn(usize, usize) -> Color32 + Sync) -> ColorImage {
    let mut img = ColorImage::new([w, h], Color32::TRANSPARENT);
    par_rows(&mut img.pixels, w, stripes(), |y, row| row.iter_mut().enumerate().for_each(|(x, p)| *p = f(x, y)));
    img
}

/// "#rrggbb" or "#rrggbbaa" as straight RGBA 0..1.
fn hex(s: &str) -> Option<[f32; 4]> {
    let s = s.trim().trim_start_matches('#');
    let byte = |i: usize| u8::from_str_radix(s.get(i..i + 2)?, 16).ok().map(|v| v as f32 / 255.0);
    match s.len() {
        6 => Some([byte(0)?, byte(2)?, byte(4)?, 1.0]),
        8 => Some([byte(0)?, byte(2)?, byte(4)?, byte(6)?]),
        _ => None,
    }
}

/// Gradient stops "pos:#rrggbbaa;..." sorted by position; black to white when none parse.
fn stops(s: &str) -> Vec<(f32, [f32; 4])> {
    let mut v: Vec<(f32, [f32; 4])> = s.split(';').filter_map(|p| {
        let (pos, c) = p.split_once(':')?;
        Some((pos.trim().parse::<f32>().ok()?.clamp(0.0, 1.0), hex(c)?))
    }).collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    if v.is_empty() { vec![(0.0, [0.0, 0.0, 0.0, 1.0]), (1.0, [1.0; 4])] } else { v }
}

fn stop_at(st: &[(f32, [f32; 4])], s: f32) -> [f32; 4] {
    let i = st.partition_point(|p| p.0 <= s);
    if i == 0 {
        return st[0].1;
    }
    if i == st.len() {
        return st[i - 1].1;
    }
    let ((p0, a), (p1, b)) = (st[i - 1], st[i]);
    let t = (s - p0) / (p1 - p0).max(1e-6);
    std::array::from_fn(|c| a[c] + (b[c] - a[c]) * t)
}

fn gradient(e: &FxEval, w: usize, h: usize) -> ColorImage {
    let st = stops(e.t("stops"));
    let lut: [Color32; 256] = std::array::from_fn(|i| premul(stop_at(&st, i as f32 / 255.0)));
    // Pixel centres: the first and last pixels sit exactly on the end stops.
    let (cx, cy) = ((w - 1) as f32 / 2.0, (h - 1) as f32 / 2.0);
    let (sn, cs) = e.f("angle").to_radians().sin_cos();
    let proj = |x: f32, y: f32| x * cs + y * sn;
    let ends = [(0.0, 0.0), (2.0 * cx, 0.0), (0.0, 2.0 * cy), (2.0 * cx, 2.0 * cy)].map(|(x, y)| proj(x, y));
    let (lo, hi) = (ends.iter().copied().fold(f32::INFINITY, f32::min), ends.iter().copied().fold(f32::NEG_INFINITY, f32::max));
    let kind = e.i("type");
    fill(w, h, |x, y| {
        let (x, y) = (x as f32, y as f32);
        let s = match kind {
            1 => (x - cx).hypot(y - cy) / cx.hypot(cy).max(1e-6),
            2 => ((x - cx).abs() / cx.max(1e-6)).max((y - cy).abs() / cy.max(1e-6)),
            _ => (proj(x, y) - lo) / (hi - lo).max(1e-6),
        };
        lut[(s.clamp(0.0, 1.0) * 255.0).round() as usize]
    })
}

fn hash3(x: i32, y: i32, z: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ (z as u32).wrapping_mul(0xcb1a_b31f) ^ seed.wrapping_mul(0x9e37_79b9);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0xff_ffff) as f32 / 0xff_ffff as f32
}

/// Value noise in 0..1, smoothly interpolated between lattice points.
fn value3(x: f32, y: f32, z: f32, seed: u32) -> f32 {
    let (xi, yi, zi) = (x.floor(), y.floor(), z.floor());
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (fx, fy, fz) = (s(x - xi), s(y - yi), s(z - zi));
    let (xi, yi, zi) = (xi as i32, yi as i32, zi as i32);
    let l = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let c = |dx, dy, dz| hash3(xi + dx, yi + dy, zi + dz, seed);
    let plane = |dz| l(l(c(0, 0, dz), c(1, 0, dz), fx), l(c(0, 1, dz), c(1, 1, dz), fx), fy);
    l(plane(0), plane(1), fz)
}

// shortcut: value-noise fBm only; add Perlin or marble types if presets need them.
fn fbm(x: f32, y: f32, z: f32, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut norm, mut f) = (0.0, 1.0, 0.0, 1.0);
    for o in 0..octaves {
        sum += amp * value3(x * f, y * f, z * f, seed.wrapping_add(o * 1013));
        norm += amp;
        amp *= 0.5;
        f *= 2.0;
    }
    sum / norm
}

/// An alpha mask image (premultiplied white) from 0..1 coverage.
fn mask(cov: &[f32], w: usize, h: usize) -> ColorImage {
    let px = cov.iter().map(|c| {
        let v = (c * 255.0).round() as u8;
        Color32::from_rgba_premultiplied(v, v, v, v)
    });
    ColorImage { size: [w, h], pixels: px.collect() }
}

/// `m` moved by (dx, dy) pixels.
fn shift(m: &ColorImage, dx: isize, dy: isize) -> ColorImage {
    let [w, h] = m.size;
    let at = |x: usize, y: usize| {
        let (sx, sy) = (x as isize - dx, y as isize - dy);
        if (0..w as isize).contains(&sx) && (0..h as isize).contains(&sy) { m.pixels[sy as usize * w + sx as usize] } else { Color32::TRANSPARENT }
    };
    fill(w, h, at)
}

/// Each row's alpha replaced by its running max over x - r ..= x + r (a monotonic queue: linear in the width).
fn max_rows(img: &mut ColorImage, r: usize) {
    let w = img.size[0];
    par_rows(&mut img.pixels, w, stripes(), |_, row| {
        let a: Vec<u8> = row.iter().map(|p| p.a()).collect();
        let (mut q, mut next) = (std::collections::VecDeque::new(), 0);
        for (x, p) in row.iter_mut().enumerate() {
            while next <= (x + r).min(w - 1) {
                while q.back().is_some_and(|&j: &usize| a[j] <= a[next]) {
                    q.pop_back();
                }
                q.push_back(next);
                next += 1;
            }
            while q.front().is_some_and(|&j| j + r < x) {
                q.pop_front();
            }
            let v = a[q[0]];
            *p = Color32::from_rgba_premultiplied(v, v, v, v);
        }
    });
}

// shortcut: a square max-dilate, so outline corners are boxy; switch to a round kernel if someone notices.
fn dilate(m: &ColorImage, r: usize) -> ColorImage {
    let mut img = m.clone();
    max_rows(&mut img, r);
    let mut t = transpose(&img);
    max_rows(&mut t, r);
    transpose(&t)
}

/// `color` (straight RGBA) drawn over `out` through mask `m`.
fn paint(out: &mut ColorImage, m: &ColorImage, color: [f32; 4]) {
    for (p, c) in out.pixels.iter_mut().zip(&m.pixels) {
        if c.a() > 0 && color[3] > 0.0 {
            *p = over(*p, premul([color[0], color[1], color[2], color[3] * c.a() as f32 / 255.0]), 255);
        }
    }
}

/// One line of text at `size`: glyphs with their x from the line start (kerned), and the line width.
fn layout(font: &FontArc, size: f32, line: &str) -> (Vec<(GlyphId, f32)>, f32) {
    let sf = font.as_scaled(size);
    let (mut x, mut prev, mut g) = (0.0, None, Vec::new());
    for ch in line.chars() {
        let id = font.glyph_id(ch);
        if let Some(p) = prev {
            x += sf.kern(p, id);
        }
        g.push((id, x));
        x += sf.h_advance(id);
        prev = Some(id);
    }
    (g, x)
}

/// Adds the coverage of `glyphs` (from `layout`, in `font` at `size`) drawn from `x0` on baseline `base` into `cov`
/// (`w` x `h`).
fn stamp(cov: &mut [f32], (w, h): (usize, usize), (font, size): (&FontArc, f32), glyphs: &[(GlyphId, f32)], x0: f32, base: f32) {
    for &(id, gx) in glyphs {
        let Some(g) = font.outline_glyph(id.with_scale_and_position(size, point(x0 + gx, base))) else { continue };
        let b = g.px_bounds();
        g.draw(|px, py, c| {
            let (x, y) = (b.min.x as i64 + px as i64, b.min.y as i64 + py as i64);
            if (0..w as i64).contains(&x) && (0..h as i64).contains(&y) {
                let m = &mut cov[y as usize * w + x as usize];
                *m = (*m + c).min(1.0);
            }
        });
    }
}

/// R18a: where the top of the credits (`total` px tall) is in a frame `h` px tall at time `t` of `len` seconds:
/// scrolling up they start just under the frame and end just above it (down: the other way); with no length, centred.
pub fn credit_top(h: f32, total: f32, t: f64, len: f64, up: bool) -> f32 {
    if len <= 0.0 {
        return (h - total) / 2.0;
    }
    let f = (t / len).clamp(0.0, 1.0) as f32;
    let f = if up { f } else { 1.0 - f };
    h - (h + total) * f
}

/// R18a: VEGAS's Credit Roll: header lines (a leading space) at the header size, single items centred, dual items (a
/// tab) as two columns either side of the centre, scrolling over the media's length.
fn credits(e: &FxEval, w: usize, h: usize, t: f64) -> ColorImage {
    let mut out = ColorImage::new([w, h], premul(e.c("background")));
    let font = fonts::load(e.t("font"));
    let hf = h as f32;
    let (hs, is) = ((e.f("header_size") * hf).max(1.0), (e.f("item_size") * hf).max(1.0));
    let text = e.t("text").replace("\r\n", "\n");
    let lines: Vec<(bool, &str)> = text.split('\n').map(|l| (l.starts_with(' '), l.trim())).collect();
    let line_h = |head: bool| {
        let sf = font.as_scaled(if head { hs } else { is });
        (sf.height() + sf.line_gap()) * e.f("spacing")
    };
    let total: f32 = lines.iter().map(|(head, _)| line_h(*head)).sum();
    let mut y = credit_top(hf, total, t, e.f(LENGTH) as f64, e.i("direction") == 0);
    let (mut head_cov, mut item_cov) = (vec![0f32; w * h], vec![0f32; w * h]);
    let (mid, gap) = (w as f32 / 2.0, w as f32 * 0.02);
    for &(head, l) in &lines {
        let (size, lh) = (if head { hs } else { is }, line_h(head));
        if y + lh > 0.0 && y < hf {
            let base = y + font.as_scaled(size).ascent();
            let cov = if head { &mut head_cov } else { &mut item_cov };
            match l.split_once('\t') {
                Some((a, b)) if !head => {
                    let (ga, wa) = layout(&font, size, a.trim());
                    stamp(cov, (w, h), (&font, size), &ga, mid - gap - wa, base);
                    stamp(cov, (w, h), (&font, size), &layout(&font, size, b.trim()).0, mid + gap, base);
                }
                _ => {
                    let (g, lw) = layout(&font, size, l);
                    stamp(cov, (w, h), (&font, size), &g, mid - lw / 2.0, base);
                }
            }
        }
        y += lh;
    }
    paint(&mut out, &mask(&head_cov, w, h), e.c("header_color"));
    paint(&mut out, &mask(&item_cov, w, h), e.c("item_color"));
    out
}

/// R18a: SMPTE colour bars (EG 1-1990, as computer RGB with 7.5 % setup black), full-height 100 % colour bars, a
/// left-to-right grey ramp, or a white grid on black (16 cells down the height).
fn test_pattern(kind: usize, w: usize, h: usize) -> ColorImage {
    const BARS: [[u8; 3]; 7] = [[191, 191, 191], [191, 191, 0], [0, 191, 191], [0, 191, 0], [191, 0, 191], [191, 0, 0], [0, 0, 191]];
    const BLACK: [u8; 3] = [19, 19, 19];
    let rgb = |c: [u8; 3]| Color32::from_rgb(c[0], c[1], c[2]);
    let (wf, hf) = (w as f32, h as f32);
    match kind {
        1 => {
            let full = [[255, 255, 255], [255, 255, 0], [0, 255, 255], [0, 255, 0], [255, 0, 255], [255, 0, 0], [0, 0, 255], [0, 0, 0]];
            fill(w, h, |x, _| rgb(full[(x * 8 / w).min(7)]))
        }
        2 => fill(w, h, |x, _| Color32::from_gray((x as f32 / (wf - 1.0).max(1.0) * 255.0).round() as u8)),
        3 => {
            let cell = (hf / 16.0).max(2.0);
            let line = |v: usize| (v as f32 % cell) < 1.0;
            fill(w, h, |x, y| if line(x) || line(y) || x + 1 == w || y + 1 == h { Color32::WHITE } else { Color32::BLACK })
        }
        _ => fill(w, h, |x, y| {
            let bar = x as f32 * 7.0 / wf; // in top-bar widths
            let yf = y as f32 / hf;
            if yf < 0.67 {
                rgb(BARS[(bar as usize).min(6)])
            } else if yf < 0.75 {
                // Reverse bars: blue, black, magenta, black, cyan, black, grey.
                rgb([BARS[6], BLACK, BARS[4], BLACK, BARS[2], BLACK, BARS[0]][(bar as usize).min(6)])
            } else if bar < 5.0 {
                // -I, white, +Q, black, each 5/4 of a bar wide.
                rgb([[0, 33, 76], [255, 255, 255], [50, 0, 106], BLACK][((bar / 1.25) as usize).min(3)])
            } else if bar < 6.0 {
                // PLUGE: below black, black, above black, a third of a bar each.
                rgb([[9, 9, 9], BLACK, [29, 29, 29]][(((bar - 5.0) * 3.0) as usize).min(2)])
            } else {
                rgb(BLACK)
            }
        }),
    }
}

/// Titles & Text: lines laid out with kerning, aligned on (x, y); drawn background, shadow, outline, then fill.
fn text(e: &FxEval, w: usize, h: usize) -> ColorImage {
    let mut out = ColorImage::new([w, h], premul(e.c("background")));
    let s = e.t("text").replace("\r\n", "\n");
    if s.trim().is_empty() {
        return out;
    }
    let font = fonts::load(e.t("font"));
    let hf = h as f32;
    let size = (e.f("size") * hf).max(1.0);
    let sf = font.as_scaled(size);
    let line_h = (sf.height() + sf.line_gap()) * e.f("spacing");
    let lines: Vec<(Vec<(GlyphId, f32)>, f32)> = s.split('\n').map(|l| layout(&font, size, l)).collect();
    let top = e.f("y") * hf - line_h * lines.len() as f32 / 2.0;
    let anchor = [0.0, 0.5, 1.0][e.i("align").min(2)];
    let mut cov = vec![0f32; w * h];
    for (i, (glyphs, lw)) in lines.iter().enumerate() {
        stamp(&mut cov, (w, h), (&font, size), glyphs, e.f("x") * w as f32 - lw * anchor, top + i as f32 * line_h + sf.ascent());
    }
    let fill_mask = mask(&cov, w, h);
    if e.on("shadow") {
        let mut sh = shift(&fill_mask, (e.f("shadow_x") * hf).round() as isize, (e.f("shadow_y") * hf).round() as isize);
        let r = (e.f("shadow_blur") * hf).round() as usize;
        blur(&mut sh, r, r, stripes());
        paint(&mut out, &sh, e.c("shadow_color"));
    }
    let r = (e.f("outline") * hf).round() as usize;
    if r > 0 {
        paint(&mut out, &dilate(&fill_mask, r), e.c("outline_color"));
    }
    paint(&mut out, &fill_mask, e.c("color"));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::plugins::{Val, find, preset_eval};

    fn ev(id: &str, set: &[(&str, Val)]) -> FxEval {
        let mut e = preset_eval(find(id).unwrap(), None);
        for (k, v) in set {
            e.vals.iter_mut().find(|p| p.0 == *k).unwrap().1 = v.clone();
        }
        e
    }

    #[test]
    fn solid_gradient_checkerboard() {
        let red = [1.0, 0.0, 0.0, 1.0];
        let s = render(&ev("milan.gen.solid", &[("color", Val::C(red))]), 33, 17, 0.0);
        assert!(s.size == [33, 17] && s.pixels.iter().all(|p| *p == Color32::RED));
        for (kind, ends) in [(0.0, [(0, 5), (48, 5)]), (2.0, [(24, 5), (0, 0)])] {
            let g = render(&ev("milan.gen.gradient", &[("type", Val::F(kind)), ("stops", Val::T("0:#102030ff;0.5:#808080;1:#ff0000ff".into()))]), 49, 11, 0.0);
            assert_eq!(g.pixels[ends[0].1 * 49 + ends[0].0], Color32::from_rgb(0x10, 0x20, 0x30), "type {kind}");
            assert_eq!(g.pixels[ends[1].1 * 49 + ends[1].0], Color32::RED, "type {kind}");
        }
        let c = render(&ev("milan.gen.checkerboard", &[("size", Val::F(0.1))]), 100, 50, 0.0); // 5 px cells
        let px = |x: usize, y: usize| c.pixels[y * 100 + x];
        assert_eq!((px(0, 0), px(4, 4), px(5, 0), px(0, 5), px(5, 5)), (Color32::WHITE, Color32::WHITE, Color32::BLACK, Color32::BLACK, Color32::WHITE));
    }

    #[test]
    fn noise_is_deterministic_in_range_and_animates() {
        let e = ev("milan.gen.noise", &[("seed", Val::F(7.0)), ("speed", Val::F(1.0))]);
        let a = render(&e, 64, 48, 0.0);
        assert!(a.pixels == render(&e, 64, 48, 0.0).pixels);
        assert!(a.pixels != render(&ev("milan.gen.noise", &[("seed", Val::F(8.0))]), 64, 48, 0.0).pixels);
        assert!(a.pixels != render(&e, 64, 48, 0.5).pixels);
        assert!(animated(&e) && !animated(&ev("milan.gen.noise", &[])));
        let (lo, hi) = a.pixels.iter().fold((255, 0), |(lo, hi), p| (lo.min(p.r()), hi.max(p.r())));
        assert!(lo < hi && a.pixels.iter().all(|p| p.a() == 255 && p.r() == p.g()));
    }

    fn txt(set: &[(&str, Val)]) -> ColorImage {
        let mut v = vec![("font", Val::T("no such font".into()))];
        v.extend(set.iter().cloned());
        render(&ev("milan.gen.text", &v), 400, 200, 0.0)
    }

    /// (min x, max x, min y, max y) of pixels with any alpha.
    fn bbox(img: &ColorImage) -> (usize, usize, usize, usize) {
        let w = img.size[0];
        img.pixels.iter().enumerate().filter(|(_, p)| p.a() > 0).fold((usize::MAX, 0, usize::MAX, 0), |b, (i, _)| (b.0.min(i % w), b.1.max(i % w), b.2.min(i / w), b.3.max(i / w)))
    }

    fn centroid(img: &ColorImage) -> (f64, f64) {
        let w = img.size[0];
        let (mut sx, mut sy, mut n) = (0.0, 0.0, 0.0);
        for (i, p) in img.pixels.iter().enumerate() {
            let a = p.a() as f64;
            (sx, sy, n) = (sx + a * (i % w) as f64, sy + a * (i / w) as f64, n + a);
        }
        (sx / n, sy / n)
    }

    #[test]
    fn text_layout() {
        // An unknown font falls back, and "A" covers only a box around the anchor.
        let a = txt(&[("text", Val::T("A".into()))]);
        let (x0, x1, y0, y1) = bbox(&a);
        assert!(x0 > 180 && x1 < 220 && y0 > 85 && y1 < 115, "{:?}", bbox(&a));
        for (align, want) in [(0.0, 100.0), (1.0, 200.0), (2.0, 300.0)] {
            let img = txt(&[("text", Val::T("HHH".into())), ("x", Val::F(if align == 0.0 { 0.25 } else if align == 1.0 { 0.5 } else { 0.75 })), ("align", Val::F(align))]);
            let (x0, x1, ..) = bbox(&img);
            let got = [x0 as f64, (x0 + x1) as f64 / 2.0, x1 as f64][align as usize];
            assert!((got - want).abs() <= 4.0, "align {align}: {got}");
        }
        assert!(txt(&[("outline", Val::F(0.0)), ("outline_color", Val::C([1.0, 0.0, 0.0, 1.0]))]).pixels == txt(&[]).pixels);
        assert!(txt(&[("outline", Val::F(0.01))]).pixels != txt(&[]).pixels);
        // The shadow alone is the fill moved by its offset.
        let fill = txt(&[]);
        let shadow = txt(&[("color", Val::C([1.0, 1.0, 1.0, 0.0])), ("shadow", Val::F(1.0)), ("shadow_x", Val::F(0.05)), ("shadow_y", Val::F(-0.03))]);
        let (f, s) = (centroid(&fill), centroid(&shadow));
        assert!((s.0 - f.0 - 10.0).abs() <= 1.0 && (s.1 - f.1 + 6.0).abs() <= 1.0, "{f:?} {s:?}");
        // Two lines are two bands of rows with a gap between.
        let two = txt(&[("text", Val::T("A\nB".into())), ("spacing", Val::F(2.0))]);
        let rows: Vec<bool> = (0..200).map(|y| two.pixels[y * 400..(y + 1) * 400].iter().any(|p| p.a() > 0)).collect();
        let bands = rows.windows(2).filter(|w| !w[0] && w[1]).count() + rows[0] as usize;
        assert_eq!(bands, 2);
        assert!(txt(&[("text", Val::T(" \n ".into()))]).pixels.iter().all(|p| *p == Color32::TRANSPARENT));
    }

    /// R18a acceptance: Test Pattern SMPTE bars pixel values (75 % bars, the reverse bars, -I, white, +Q and PLUGE).
    #[test]
    fn test_pattern_smpte_bars() {
        let img = render(&ev("milan.gen.test_pattern", &[]), 700, 100, 0.0);
        let px = |x: usize, y: usize| img.pixels[y * 700 + x];
        let rgb = |r, g, b| Color32::from_rgb(r, g, b);
        let top = [rgb(191, 191, 191), rgb(191, 191, 0), rgb(0, 191, 191), rgb(0, 191, 0), rgb(191, 0, 191), rgb(191, 0, 0), rgb(0, 0, 191)];
        for (k, c) in top.iter().enumerate() {
            assert_eq!(px(k * 100 + 50, 10), *c, "bar {k}");
        }
        assert_eq!((px(50, 70), px(150, 70), px(250, 70), px(650, 70)), (rgb(0, 0, 191), rgb(19, 19, 19), rgb(191, 0, 191), rgb(191, 191, 191)));
        assert_eq!((px(60, 90), px(180, 90), px(300, 90), px(450, 90)), (rgb(0, 33, 76), Color32::WHITE, rgb(50, 0, 106), rgb(19, 19, 19)));
        assert_eq!((px(510, 90), px(550, 90), px(580, 90), px(690, 90)), (rgb(9, 9, 9), rgb(19, 19, 19), rgb(29, 29, 29), rgb(19, 19, 19)));
        let bars = render(&ev("milan.gen.test_pattern", &[("type", Val::F(1.0))]), 80, 10, 0.0);
        assert_eq!((bars.pixels[5], bars.pixels[75]), (Color32::WHITE, Color32::BLACK));
        let ramp = render(&ev("milan.gen.test_pattern", &[("type", Val::F(2.0))]), 256, 4, 0.0);
        assert_eq!((ramp.pixels[0], ramp.pixels[255], ramp.pixels[128].r()), (Color32::BLACK, Color32::WHITE, 128));
        assert!(!animated(&ev("milan.gen.test_pattern", &[])));
    }

    /// R18a acceptance: a credit roll's scroll position at t: under the frame at the start, centred half-way, above it
    /// at the end (scrolling down the other way round); the rendered line follows.
    #[test]
    fn credit_roll_scroll_position_at_t() {
        assert_eq!((credit_top(100.0, 50.0, 0.0, 10.0, true), credit_top(100.0, 50.0, 5.0, 10.0, true), credit_top(100.0, 50.0, 10.0, 10.0, true)), (100.0, 25.0, -50.0));
        assert_eq!((credit_top(100.0, 50.0, 0.0, 10.0, false), credit_top(100.0, 50.0, 2.5, 0.0, true)), (-50.0, 25.0));
        let cr = |text: &str| ev(CREDITS, &[("text", Val::T(text.into())), ("font", Val::T("no such font".into())), ("item_size", Val::F(0.2)), ("header_size", Val::F(0.3))]);
        let mut e = cr("HHHH");
        assert!(animated(&e));
        e.vals.push((LENGTH, Val::F(10.0)));
        assert!(render(&e, 200, 100, 0.0).pixels.iter().all(|p| *p == Color32::BLACK), "starts below the frame");
        let mid = render(&e, 200, 100, 5.0);
        let rows: Vec<usize> = (0..100).filter(|y| mid.pixels[y * 200..(y + 1) * 200].iter().any(|p| p.r() > 100)).collect();
        let (y0, y1) = (rows[0] as f32, *rows.last().unwrap() as f32);
        assert!(((y0 + y1) / 2.0 - 50.0).abs() < 8.0, "{y0}..{y1}");
        // Two columns either side of the centre; a header is bigger than an item.
        let two = render(&cr("H\tH"), 200, 100, 0.0);
        let lit = |x: usize| (0..100).any(|y| two.pixels[y * 200 + x].r() > 100);
        assert!(!lit(100) && (80..100).any(lit) && (101..120).any(lit));
        let tall = |text: &str| {
            let img = render(&cr(text), 200, 100, 0.0);
            (0..100).filter(|y| img.pixels[y * 200..(y + 1) * 200].iter().any(|p| p.r() > 100)).count()
        };
        let (h, i) = (tall(" H"), tall("H"));
        assert!(h > i, "{h} {i}");
    }

    #[test]
    fn dilate_matches_the_brute_force_square_max() {
        let (w, h) = (300, 140); // big enough for par_rows to stripe
        let a = |i: usize| ((i * 2_654_435_761) >> 13) as u8 & if i % 7 == 0 { 255 } else { 0 };
        let m = mask(&(0..w * h).map(|i| a(i) as f32 / 255.0).collect::<Vec<_>>(), w, h);
        let tiny = mask(&[0.0, 0.2, 0.0, 0.0, 0.0, 0.0], 3, 2);
        assert!(dilate(&tiny, 10).pixels.iter().all(|p| p.a() == 51)); // a radius past both edges
        for r in [0, 1, 4, 23] {
            let got = dilate(&m, r);
            for y in 0..h {
                for x in 0..w {
                    let want = (y.saturating_sub(r)..=(y + r).min(h - 1)).flat_map(|j| (x.saturating_sub(r)..=(x + r).min(w - 1)).map(move |i| (i, j))).map(|(i, j)| m.pixels[j * w + i].a()).max().unwrap();
                    assert_eq!(got.pixels[y * w + x], Color32::from_rgba_premultiplied(want, want, want, want), "r {r} at {x},{y}");
                }
            }
        }
    }
}
