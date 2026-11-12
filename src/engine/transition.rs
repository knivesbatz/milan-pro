//! Transitions: `mix` blends the outgoing picture A into the incoming B at progress p (both premultiplied, frame
//! size). p = 0 is exactly A and p = 1 exactly B for every kind.
use eframe::egui::{Color32, ColorImage};
use crate::engine::composite::{Affine, draw, par_rows};
use crate::engine::plugins::FxEval;
use crate::engine::vfx::{lerp_px, premul};

/// Weight of B for a pixel at `s` (0..1 along the wipe) at progress `p`, with a soft band `f` wide.
fn band(s: f32, p: f32, f: f32) -> f32 {
    let e = p * (1.0 + f) - f;
    if f <= 0.0 { (s < e) as u8 as f32 } else { ((e + f - s) / f).clamp(0.0, 1.0) }
}

/// A to B per pixel by `wt(x, y)`.
fn blend(a: &ColorImage, b: &ColorImage, k: usize, wt: impl Fn(f32, f32) -> f32 + Sync) -> ColorImage {
    let w = a.size[0];
    let mut out = a.clone();
    par_rows(&mut out.pixels, w, k, |y, row| {
        for (x, p) in row.iter_mut().enumerate() {
            *p = lerp_px(*p, b.pixels[y * w + x], wt(x as f32 + 0.5, y as f32 + 0.5));
        }
    });
    out
}

/// Each pixel through `f`.
fn map(src: &ColorImage, k: usize, f: impl Fn(Color32) -> Color32 + Sync) -> ColorImage {
    let mut out = src.clone();
    let w = out.size[0];
    par_rows(&mut out.pixels, w, k, |_, row| row.iter_mut().for_each(|p| *p = f(*p)));
    out
}

/// `s` + `c` x `t`, per channel, clamped.
fn add(s: Color32, c: Color32, t: f32) -> Color32 {
    let (s, c) = (s.to_array(), c.to_array());
    let o: [u8; 4] = std::array::from_fn(|i| (s[i] as f32 + c[i] as f32 * t).round().min(255.0) as u8);
    Color32::from_rgba_premultiplied(o[0], o[1], o[2], o[3])
}

/// B drawn at whole-pixel offset `ob` over A at `oa`; uncovered pixels are transparent.
fn shifted(a: &ColorImage, oa: (isize, isize), b: &ColorImage, ob: (isize, isize), k: usize) -> ColorImage {
    let [w, h] = a.size;
    let mut out = ColorImage::new([w, h], Color32::TRANSPARENT);
    let at = |img: &ColorImage, (ox, oy): (isize, isize), x: usize, y: usize| {
        let (sx, sy) = (x as isize - ox, y as isize - oy);
        ((0..w as isize).contains(&sx) && (0..h as isize).contains(&sy)).then(|| img.pixels[sy as usize * w + sx as usize])
    };
    par_rows(&mut out.pixels, w, k, |y, row| {
        for (x, p) in row.iter_mut().enumerate() {
            *p = at(b, ob, x, y).or_else(|| at(a, oa, x, y)).unwrap_or(Color32::TRANSPARENT);
        }
    });
    out
}

/// `img` scaled by `s` about the frame centre.
fn zoomed(img: &ColorImage, s: f64, k: usize) -> ColorImage {
    let [w, h] = img.size;
    let (cx, cy) = (w as f64 / 2.0, h as f64 / 2.0);
    let mut out = ColorImage::new([w, h], Color32::TRANSPARENT);
    draw(&mut out, img, Affine::translate(-cx, -cy).then(Affine::scale(s, s)).then(Affine::translate(cx, cy)), 1.0, true, k);
    out
}

/// Direction of travel for push and slide (`plugins::DIRECTIONS`).
fn direction(i: usize) -> (f32, f32) {
    [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0), (-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)][i.min(7)]
}

pub fn mix(e: &FxEval, a: &ColorImage, b: &ColorImage, p: f32, k: usize) -> ColorImage {
    if p <= 0.0 {
        return a.clone();
    }
    if p >= 1.0 || a.size != b.size {
        return b.clone();
    }
    let [w, h] = a.size;
    let (wf, hf) = (w as f32, h as f32);
    let (cx, cy) = (wf / 2.0, hf / 2.0);
    let feather = e.f("feather");
    let offset = |d: (f32, f32), t: f32| ((d.0 * t * wf).round() as isize, (d.1 * t * hf).round() as isize);
    match e.id {
        "milan.tr.fade_to_color" => {
            let (c, additive) = (premul(e.c("color")), e.on("additive"));
            // Out to the colour over the first half, in from it over the second.
            let (src, t) = if p < 0.5 { (a, p * 2.0) } else { (b, 2.0 - 2.0 * p) };
            map(src, k, |s| if additive { add(s, c, t) } else { lerp_px(s, c, t) })
        }
        "milan.tr.linear_wipe" => {
            let (s, c) = e.f("angle").to_radians().sin_cos();
            let ext = (c.abs() * wf + s.abs() * hf).max(1.0);
            blend(a, b, k, |x, y| band(((x - cx) * c + (y - cy) * s) / ext + 0.5, p, feather))
        }
        "milan.tr.iris" => {
            let shape = e.i("shape");
            blend(a, b, k, |x, y| {
                let (dx, dy) = (((x - cx) / cx).abs(), ((y - cy) / cy).abs());
                let s = match shape {
                    1 => dx.max(dy),
                    2 => (dx + dy) / 2.0,
                    _ => (dx * dx * cx * cx + dy * dy * cy * cy).sqrt() / (cx * cx + cy * cy).sqrt(),
                };
                band(s, p, feather)
            })
        }
        "milan.tr.clock_wipe" => blend(a, b, k, |x, y| {
            // 0 at twelve o'clock, growing clockwise (y points down).
            let ang = (x - cx).atan2(cy - y).rem_euclid(std::f32::consts::TAU);
            band(ang / std::f32::consts::TAU, p, feather)
        }),
        "milan.tr.push" => {
            let d = direction(e.i("direction"));
            shifted(a, offset(d, p), b, offset(d, p - 1.0), k)
        }
        "milan.tr.slide" => shifted(a, (0, 0), b, offset(direction(e.i("direction")), p - 1.0), k),
        "milan.tr.zoom" => {
            let amt = e.f("amount") as f64;
            let (za, zb) = (zoomed(a, 1.0 + p as f64 * amt, k), zoomed(b, 1.0 + (1.0 - p as f64) * amt, k));
            blend(&za, &zb, k, |_, _| p)
        }
        _ if e.on("additive") => {
            let (wa, wb) = ((2.0 - 2.0 * p).min(1.0), (2.0 * p).min(1.0));
            let mut out = map(a, k, |s| add(Color32::TRANSPARENT, s, wa));
            par_rows(&mut out.pixels, w, k, |y, row| {
                for (o, s) in row.iter_mut().zip(&b.pixels[y * w..]) {
                    *o = add(*o, *s, wb);
                }
            });
            out
        }
        // milan.tr.dissolve: A (1 - p) + B p.
        _ => blend(a, b, k, |_, _| p),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::plugins::{PluginKind, catalog, preset_eval};

    fn pic(w: usize, h: usize, seed: u32) -> ColorImage {
        ColorImage { size: [w, h], pixels: (0..w * h).map(|i| Color32::from_rgb((i as u32 * 7 + seed) as u8, (i as u32 / 3 + seed * 5) as u8, seed as u8)).collect() }
    }

    #[test]
    fn ends_are_exact_for_every_transition() {
        let (a, b) = (pic(40, 30, 1), pic(40, 30, 90));
        for info in catalog(PluginKind::Transition) {
            for pr in info.presets {
                let e = preset_eval(info, Some(pr));
                assert!(mix(&e, &a, &b, 0.0, 1).pixels == a.pixels, "{} {}", info.id, pr.name);
                assert!(mix(&e, &a, &b, 1.0, 1).pixels == b.pixels, "{} {}", info.id, pr.name);
                let mid = mix(&e, &a, &b, 0.5, 3);
                assert_eq!(mid.size, [40, 30]);
                assert!(mid.pixels == mix(&e, &a, &b, 0.5, 1).pixels);
            }
        }
    }

    #[test]
    fn dissolve_fade_and_wipe() {
        let (a, b) = (ColorImage::new([4, 4], Color32::from_gray(200)), ColorImage::new([4, 4], Color32::from_gray(100)));
        let d = mix(&preset_eval(crate::engine::plugins::find("milan.tr.dissolve").unwrap(), None), &a, &b, 0.25, 1);
        assert_eq!(d.pixels[0], Color32::from_gray(175)); // 200 x 0.75 + 100 x 0.25
        let ftc = crate::engine::plugins::find("milan.tr.fade_to_color").unwrap();
        for pr in &ftc.presets[..2] {
            let e = preset_eval(ftc, Some(pr));
            assert!(mix(&e, &a, &b, 0.5, 1).pixels.iter().all(|p| *p == premul(e.c("color"))), "{}", pr.name);
        }
        // A linear wipe: each pixel is A, B or (only inside the feather band) a blend.
        let wipe = crate::engine::plugins::find("milan.tr.linear_wipe").unwrap();
        let (a, b) = (ColorImage::new([100, 10], Color32::RED), ColorImage::new([100, 10], Color32::BLUE));
        let out = mix(&preset_eval(wipe, None), &a, &b, 0.5, 1); // feather 0.05: a 5 px band
        let blended: Vec<usize> = (0..100).filter(|&x| ![Color32::RED, Color32::BLUE].contains(&out.pixels[x])).collect();
        assert!(!blended.is_empty() && blended.len() <= 5, "{blended:?}");
        assert!((0..blended[0]).all(|x| out.pixels[x] == Color32::BLUE)); // B comes in from the left
        assert!((blended[blended.len() - 1] + 1..100).all(|x| out.pixels[x] == Color32::RED));
    }
}
