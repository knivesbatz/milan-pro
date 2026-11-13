//! Masks (Pan/Crop Mask, Bezier Masking, Cookie Cutter): closed Bezier paths flattened to polygons, filled with exact
//! horizontal coverage on 4 sub-scanlines per pixel row, and feathered with the FX blur.
use eframe::egui::{Color32, ColorImage};
use crate::engine::composite::{mul255, par_rows};

/// One Pan/Crop mask path at a frame: anchors [x, y, in dx, in dy, out dx, out dy] normalised to the source.
#[derive(Debug, Clone, PartialEq)]
pub struct MaskPlan {
    pub path: Vec<[f32; 6]>,
    pub negative: bool,
    pub opacity: f32,
    /// A fraction of the picture height.
    pub feather: f32,
}

/// The closed Bezier through `path`, as a polygon in a `w` x `h` pixel picture. Straight segments (no handles) are one
/// edge; curved ones 16.
pub fn flatten(path: &[[f32; 6]], w: f32, h: f32) -> Vec<(f32, f32)> {
    let n = path.len();
    let mut out = Vec::new();
    for i in 0..n {
        let (a, b) = (path[i], path[(i + 1) % n]);
        let p = [(a[0], a[1]), (a[0] + a[4], a[1] + a[5]), (b[0] + b[2], b[1] + b[3]), (b[0], b[1])];
        let steps = if a[4] == 0.0 && a[5] == 0.0 && b[2] == 0.0 && b[3] == 0.0 { 1 } else { 16 };
        for k in 0..steps {
            let t = k as f32 / steps as f32;
            let u = 1.0 - t;
            let c = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
            let at = |f: fn(&(f32, f32)) -> f32| (0..4).map(|j| c[j] * f(&p[j])).sum::<f32>();
            out.push((at(|q| q.0) * w, at(|q| q.1) * h));
        }
    }
    out
}

/// `wgt` times the part of each pixel of `row` between x = a and x = b.
fn span(row: &mut [f32], a: f32, b: f32, wgt: f32) {
    let n = row.len() as f32;
    let (a, b) = (a.clamp(0.0, n), b.clamp(0.0, n));
    if b <= a {
        return;
    }
    let (ia, ib) = (a.floor() as usize, b.floor() as usize);
    if ia == ib {
        row[ia] += (b - a) * wgt;
        return;
    }
    row[ia] += (ia as f32 + 1.0 - a) * wgt;
    row[ia + 1..ib].iter_mut().for_each(|r| *r += wgt);
    if ib < row.len() {
        row[ib] += (b - ib as f32) * wgt;
    }
}

/// How much of each pixel of a `w` x `h` picture polygon `poly` (pixels, even-odd) covers, 0..=1.
pub fn coverage(poly: &[(f32, f32)], w: usize, h: usize) -> Vec<f32> {
    let mut cov = vec![0f32; w * h];
    if poly.len() < 3 || w == 0 {
        return cov;
    }
    let mut xs = Vec::new();
    for (y, row) in cov.chunks_mut(w).enumerate() {
        for s in 0..4 {
            let yy = y as f32 + (s as f32 + 0.5) / 4.0;
            xs.clear();
            for (i, &a) in poly.iter().enumerate() {
                let b = poly[(i + 1) % poly.len()];
                if (a.1 <= yy) != (b.1 <= yy) {
                    xs.push(a.0 + (yy - a.1) / (b.1 - a.1) * (b.0 - a.0));
                }
            }
            xs.sort_by(f32::total_cmp);
            for p in xs.chunks_exact(2) {
                span(row, p[0], p[1], 0.25);
            }
        }
    }
    cov
}

/// Soften `cov` by about `r` pixels on both sides of its edges (the FX blur on an alpha picture).
pub fn feather(cov: &mut [f32], w: usize, h: usize, r: f32, k: usize) {
    if r < 0.5 || cov.len() != w * h {
        return;
    }
    let a = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let mut img = ColorImage { size: [w, h], pixels: cov.iter().map(|&v| Color32::from_rgba_premultiplied(0, 0, 0, a(v))).collect() };
    let r = r.round() as usize;
    crate::engine::vfx::blur(&mut img, r, r, k);
    cov.iter_mut().zip(&img.pixels).for_each(|(v, p)| *v = p.a() as f32 / 255.0);
}

/// Every channel of each pixel scaled by its mask value 0..=1.
pub fn scale_by(img: &mut ColorImage, m: &[f32], k: usize) {
    let w = img.size[0];
    par_rows(&mut img.pixels, w, k, |y, row| {
        for (p, &v) in row.iter_mut().zip(&m[y * w..]) {
            let a = (v.clamp(0.0, 1.0) * 255.0).round() as u32;
            if a < 255 {
                let c = p.to_array().map(|c| mul255(c as u32, a) as u8);
                *p = Color32::from_rgba_premultiplied(c[0], c[1], c[2], c[3]);
            }
        }
    });
}

/// The combined mask of `masks` over a `w` x `h` picture: positive paths add (the largest wins), negative ones cut out;
/// with no positive path everything starts shown.
pub fn mask_values(masks: &[MaskPlan], w: usize, h: usize, k: usize) -> Vec<f32> {
    let mut m = vec![if masks.iter().any(|x| !x.negative) { 0.0 } else { 1.0 }; w * h];
    for mk in masks {
        let mut c = coverage(&flatten(&mk.path, w as f32, h as f32), w, h);
        feather(&mut c, w, h, mk.feather * h as f32, k);
        for (v, c) in m.iter_mut().zip(&c) {
            let c = c.min(1.0) * mk.opacity.clamp(0.0, 1.0);
            *v = if mk.negative { *v * (1.0 - c) } else { v.max(c) };
        }
    }
    m
}

/// Apply Pan/Crop masks to a source picture.
pub fn apply(img: &mut ColorImage, masks: &[MaskPlan], k: usize) {
    let [w, h] = img.size;
    if masks.is_empty() || w == 0 || h == 0 {
        return;
    }
    let m = mask_values(masks, w, h, k);
    scale_by(img, &m, k);
}

/// Bezier handles that make a smooth closed curve through `pts` (Catmull-Rom), or none (`smooth` off: a polygon).
pub fn smooth_path(pts: &[(f32, f32)], smooth: bool) -> Vec<[f32; 6]> {
    let n = pts.len();
    (0..n)
        .map(|i| {
            let (p, q) = (pts[(i + n - 1) % n], pts[(i + 1) % n]);
            let (tx, ty) = if smooth && n > 2 { ((q.0 - p.0) / 6.0, (q.1 - p.1) / 6.0) } else { (0.0, 0.0) };
            [pts[i].0, pts[i].1, -tx, -ty, tx, ty]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<[f32; 6]> {
        [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].iter().map(|&(x, y)| [x, y, 0.0, 0.0, 0.0, 0.0]).collect()
    }

    /// R7 acceptance: a unit square's raster area is 1 ± 0.5%, wherever it falls on the pixel grid.
    #[test]
    fn unit_square_area() {
        for (ox, oy) in [(10.0, 10.0), (10.3, 7.71), (3.5, 12.25)] {
            let poly = flatten(&square(ox, oy, ox + 1.0, oy + 1.0), 1.0, 1.0);
            let area: f32 = coverage(&poly, 32, 32).iter().sum();
            assert!((area - 1.0).abs() <= 0.005, "{ox},{oy}: {area}");
        }
        // A large one too, and a circle (Bezier) against πr².
        let area: f32 = coverage(&flatten(&square(0.2, 0.2, 0.7, 0.6), 100.0, 100.0), 100, 100).iter().sum();
        assert!((area - 2000.0).abs() / 2000.0 < 0.005, "{area}");
        let k = 0.5523; // Bezier circle handle length
        let r = 0.3;
        let circle = vec![[0.5, 0.2, -k * r, 0.0, k * r, 0.0], [0.8, 0.5, 0.0, -k * r, 0.0, k * r], [0.5, 0.8, k * r, 0.0, -k * r, 0.0], [0.2, 0.5, 0.0, k * r, 0.0, -k * r]];
        let area: f32 = coverage(&flatten(&circle, 200.0, 200.0), 200, 200).iter().sum();
        let want = std::f32::consts::PI * 60.0 * 60.0;
        assert!((area - want).abs() / want < 0.005, "{area} vs {want}");
    }

    /// R7 acceptance: feather 0 gives a hard edge; invert (a negative path) flips the coverage.
    #[test]
    fn hard_edge_feather_and_invert() {
        let pos = MaskPlan { path: square(0.25, 0.0, 0.75, 1.0), negative: false, opacity: 1.0, feather: 0.0 };
        let m = mask_values(std::slice::from_ref(&pos), 40, 10, 1);
        let row: Vec<f32> = m[..40].to_vec();
        assert!(row[..10].iter().all(|v| *v == 0.0) && row[10..30].iter().all(|v| *v == 1.0) && row[30..].iter().all(|v| *v == 0.0), "{row:?}");
        let neg = mask_values(&[MaskPlan { negative: true, ..pos.clone() }], 40, 10, 1);
        assert!(m.iter().zip(&neg).all(|(a, b)| (a + b - 1.0).abs() < 1e-6));
        // Feathered: soft across the edge, the middle untouched.
        let soft = mask_values(&[MaskPlan { feather: 0.3, ..pos }], 40, 10, 1);
        assert!(soft[10] > 0.2 && soft[10] < 0.8 && soft[9] > 0.0 && soft[20] == 1.0, "{:?}", &soft[..40]);
        // Positive paths add; a negative one cuts out of them.
        let two = mask_values(&[MaskPlan { path: square(0.0, 0.0, 0.25, 1.0), negative: false, opacity: 1.0, feather: 0.0 }, MaskPlan { path: square(0.0, 0.0, 0.1, 1.0), negative: true, opacity: 1.0, feather: 0.0 }], 40, 10, 1);
        assert_eq!((two[2], two[5], two[20]), (0.0, 1.0, 0.0));
    }

    #[test]
    fn masks_scale_premultiplied_pixels() {
        let mut img = ColorImage::new([4, 1], Color32::from_rgb(200, 100, 50));
        apply(&mut img, &[MaskPlan { path: square(0.0, 0.0, 0.5, 1.0), negative: false, opacity: 0.5, feather: 0.0 }], 1);
        assert_eq!((img.pixels[0].to_array(), img.pixels[3]), ([100, 50, 25, 128], Color32::TRANSPARENT));
        // Smooth handles pass through the points; corners have none.
        let p = smooth_path(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)], true);
        assert_eq!((p[1][0], p[1][1]), (1.0, 0.0));
        assert!(p[1][4] != 0.0 && smooth_path(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)], false)[1][4] == 0.0);
    }
}
