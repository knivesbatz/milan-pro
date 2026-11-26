//! MVFX portable maths: the CPU side of `gpu/mvfx_common.wgsl`. Every function is written as its WGSL twin is: the
//! same f32 operations in the same order (Rust never fuses a multiply into an add; the WGSL side pins each operation
//! with math.wgsl's `rd`), and no library transcendental per pixel (the sine, arc tangent and arc cosine here are
//! polynomials), so a kernel gives the same bits on the CPU and the GPU. Per-frame values (a rotation's sine, a
//! seed's offsets) are worked out once on the CPU and handed to both.
use eframe::egui::{Color32, ColorImage};

pub const PI: f32 = 3.1415927;
pub const TAU: f32 = 6.2831855;
const HALF_PI: f32 = 1.5707964;

/// A kernel's settings, worked out once per picture from a plug-in's values and read the same way by the CPU kernel
/// and its WGSL twin (`M` in mvfx_common.wgsl): integers `i`, `j` and floats `f`. `data` carries tables.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct M {
    pub i: [i32; 4],
    pub j: [i32; 4],
    pub f: [[f32; 4]; 12],
}

impl M {
    /// The uniform block's words (z.x, which must be 0, between `j` and `f`).
    pub fn words(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self.i.iter().chain(&self.j).map(|&x| x as u32).collect();
        v.extend([0u32; 4]);
        v.extend(self.f.iter().flatten().map(|f| f.to_bits()));
        v
    }
}

/// f32 values as data words.
pub fn fwords(v: &[f32]) -> Vec<u32> {
    v.iter().map(|f| f.to_bits()).collect()
}

/// A data word as f32.
pub fn fw(d: &[u32], i: usize) -> f32 {
    f32::from_bits(d[i])
}

/// The integer hash both sides share (lowbias32).
pub fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ (x >> 16)
}

pub fn hash3(a: u32, b: u32, c: u32) -> u32 {
    hash(a ^ hash(b ^ hash(c)))
}

/// A hash as 0..1 (24 bits, exact in f32).
pub fn rnd(h: u32) -> f32 {
    (h >> 8) as f32 * (1.0 / 16_777_216.0)
}

/// A channel value (any float) to a byte: clamped to 0..=255, a half rounded up.
pub fn q8(v: f32) -> u32 {
    (v.clamp(0.0, 255.0) + 0.5).floor() as u32
}

pub fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// `vfx::smoothstep`.
pub fn sstep(e0: f32, e1: f32, x: f32) -> f32 {
    if e1 <= e0 {
        return (x >= e0) as u8 as f32;
    }
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// atan2 to about 2e-6 radians (Hastings' polynomial), in -pi..pi.
pub fn atan2(y: f32, x: f32) -> f32 {
    let (ax, ay) = (x.abs(), y.abs());
    let mx = ax.max(ay);
    if mx == 0.0 {
        return 0.0;
    }
    let a = ax.min(ay) / mx;
    let s = a * a;
    let mut r = a * (0.99997726 + s * (-0.33262347 + s * (0.19354346 + s * (-0.11643287 + s * (0.05265332 + s * -0.0117212)))));
    if ay > ax {
        r = HALF_PI - r;
    }
    if x < 0.0 {
        r = PI - r;
    }
    if y < 0.0 { -r } else { r }
}

/// acos to about 7e-5 (Abramowitz and Stegun 4.4.45), x clamped to -1..1.
pub fn acos(x: f32) -> f32 {
    let a = x.abs().min(1.0);
    let r = (1.0 - a).sqrt() * (1.5707288 + a * (-0.2121144 + a * (0.074261 + a * -0.0187293)));
    if x < 0.0 { PI - r } else { r }
}

pub fn asin(x: f32) -> f32 {
    HALF_PI - acos(x)
}

/// Rec.709 luma of straight RGB 0..1.
pub fn luma(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

/// A picture to read from: premultiplied pixels as f32 0..255.
#[derive(Clone, Copy)]
pub struct Img<'a> {
    pub px: &'a [Color32],
    pub w: i32,
    pub h: i32,
}

impl<'a> Img<'a> {
    pub fn of(img: &'a ColorImage) -> Self {
        Img { px: &img.pixels, w: img.size[0] as i32, h: img.size[1] as i32 }
    }

    fn get(&self, x: i32, y: i32) -> [f32; 4] {
        let p = self.px[(y * self.w + x) as usize].to_array();
        [p[0] as f32, p[1] as f32, p[2] as f32, p[3] as f32]
    }

    /// The pixel at (x, y), the edge repeated outside.
    pub fn at(&self, x: i32, y: i32) -> [f32; 4] {
        self.get(x.clamp(0, self.w - 1), y.clamp(0, self.h - 1))
    }

    /// The pixel at (x, y), transparent outside.
    pub fn at0(&self, x: i32, y: i32) -> [f32; 4] {
        if x < 0 || y < 0 || x >= self.w || y >= self.h { [0.0; 4] } else { self.get(x, y) }
    }

    pub fn tap(&self, x: i32, y: i32, edge: bool) -> [f32; 4] {
        if edge { self.at(x, y) } else { self.at0(x, y) }
    }

    /// The nearest pixel to point (x, y) (pixel i spans i..i+1).
    pub fn near(&self, x: f32, y: f32, edge: bool) -> [f32; 4] {
        self.tap(ifloor(x), ifloor(y), edge)
    }

    /// Bilinear at point (x, y) (pixel centres at i + 0.5); `edge` repeats the edge, else transparent outside.
    pub fn bil(&self, x: f32, y: f32, edge: bool) -> [f32; 4] {
        let (fx, fy) = (x - 0.5, y - 0.5);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - x0, fy - y0);
        let (ix, iy) = (ifloor(x0), ifloor(y0));
        let (a, b, c, d) = (self.tap(ix, iy, edge), self.tap(ix + 1, iy, edge), self.tap(ix, iy + 1, edge), self.tap(ix + 1, iy + 1, edge));
        std::array::from_fn(|i| {
            let top = a[i] + (b[i] - a[i]) * tx;
            let bot = c[i] + (d[i] - c[i]) * tx;
            top + (bot - top) * ty
        })
    }
}

/// floor as an integer, kept in a range every picture fits.
pub fn ifloor(v: f32) -> i32 {
    v.clamp(-1_048_576.0, 1_048_576.0).floor() as i32
}

/// Premultiplied f32 0..255 to a pixel (colour never above alpha).
pub fn pack(c: [f32; 4]) -> Color32 {
    let a = q8(c[3]);
    Color32::from_rgba_premultiplied(q8(c[0]).min(a) as u8, q8(c[1]).min(a) as u8, q8(c[2]).min(a) as u8, a as u8)
}

/// Straight RGB 0..1 of a premultiplied f32 pixel (black when transparent): math.wgsl's `unpremul`.
pub fn straight(c: [f32; 4]) -> [f32; 3] {
    if c[3] <= 0.0 { [0.0; 3] } else { [c[0] / c[3], c[1] / c[3], c[2] / c[3]] }
}

/// Straight RGB 0..1 premultiplied by alpha `a` (0..255) back to f32 0..255: rounded as math.wgsl's `repremul`.
pub fn premul(c: [f32; 3], a: f32) -> [f32; 4] {
    let q = |v: f32| (v.clamp(0.0, 1.0) * a + 0.5).floor();
    [q(c[0]), q(c[1]), q(c[2]), a]
}

/// `kernel` reading two pictures of the same size (a transition's sides, a picture and its blur).
pub fn kernel2(a: &ColorImage, b: &ColorImage, k: usize, f: impl Fn(i32, i32, Img, Img) -> Color32 + Sync) -> ColorImage {
    let mut out = a.clone();
    let (sa, sb) = (Img::of(a), Img::of(b));
    let w = a.size[0];
    crate::engine::composite::par_rows(&mut out.pixels, w, k, |y, row| {
        for (x, p) in row.iter_mut().enumerate() {
            *p = f(x as i32, y as i32, sa, sb);
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polynomials_are_close_to_the_library() {
        for i in -400..400 {
            let x = i as f32 * 0.037;
            let y = (i * 7 % 13) as f32 - 6.0;
            assert!((atan2(y, x) - y.atan2(x)).abs() < 1e-5 || (x == 0.0 && y == 0.0), "atan2 {y} {x}");
        }
        for i in 0..=200 {
            let x = i as f32 / 100.0 - 1.0;
            assert!((acos(x) - x.acos()).abs() < 1e-4, "acos {x}");
        }
        assert_eq!((q8(-3.0), q8(127.5), q8(300.0)), (0, 128, 255));
    }
}
