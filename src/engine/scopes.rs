//! R16b: what VEGAS's Video Scopes window measures in a composited frame (premultiplied RGBA, as shown over black):
//! the waveform (luminance or composite, in IRE), the vectorscope (NTSC U/V with the 75 % bar targets), the
//! histogram (with its mean and standard deviation) and the RGB parade. Pure: `ui/scopes.rs` runs it on a worker.
use eframe::egui::{Color32, ColorImage};
use serde::{Deserialize, Serialize};

/// Video Scopes Settings: "7.5 IRE Setup" and "Studio RGB (16 to 235)" (both off, so white is 100 IRE). They change
/// how the waveform and vectorscope read the frame; the histogram and the RGB parade show code values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Levels {
    pub setup: bool,
    pub studio: bool,
}

impl Levels {
    /// A code value as a fraction of black (0) to white (1).
    fn unit(self, c: u8) -> f32 {
        if self.studio { (c as f32 - 16.0) / 219.0 } else { c as f32 / 255.0 }
    }

    /// Black-to-white fraction `y` in IRE: 0-100, or 7.5-100 with setup.
    pub fn ire(self, y: f32) -> f32 {
        if self.setup { 7.5 + 92.5 * y } else { 100.0 * y }
    }
}

/// Rec. 601 luma and the NTSC colour-difference axes U and V, which the vectorscope graticule is drawn in.
fn yuv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let y = 0.299 * r + 0.587 * g + 0.114 * b;
    (y, 0.492 * (b - y), 0.877 * (r - y))
}

/// Histogram Settings (VEGAS's list).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum HistMode {
    #[default]
    Luminance,
    Red,
    Green,
    Blue,
    Alpha,
    /// Luminance, Red, Green and Blue stacked.
    LumaRgb,
    /// CIE Lightness, a and b stacked.
    Lab,
}

impl HistMode {
    pub const ALL: [HistMode; 7] = [HistMode::Luminance, HistMode::Red, HistMode::Green, HistMode::Blue, HistMode::Alpha, HistMode::LumaRgb, HistMode::Lab];

    pub fn label(self) -> &'static str {
        ["Luminance", "Red", "Green", "Blue", "Alpha", "Luminance/R/G/B", "Lightness/a/b"][self as usize]
    }

    pub fn channels(self) -> &'static [Channel] {
        use Channel::*;
        match self {
            HistMode::Luminance => &[Luma],
            HistMode::Red => &[Red],
            HistMode::Green => &[Green],
            HistMode::Blue => &[Blue],
            HistMode::Alpha => &[Alpha],
            HistMode::LumaRgb => &[Luma, Red, Green, Blue],
            HistMode::Lab => &[L, A, B],
        }
    }
}

/// One histogram graph's channel. Bins are code values 0-255; Lightness is 0-100 over them, a and b -128..127.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Luma,
    Red,
    Green,
    Blue,
    Alpha,
    L,
    A,
    B,
}

impl Channel {
    /// The value bin `k` stands for, in the channel's own units.
    pub fn value(self, k: f32) -> f32 {
        match self {
            Channel::L => k / 2.55,
            Channel::A | Channel::B => k - 128.0,
            _ => k,
        }
    }

    /// The colour its graph is drawn in.
    pub fn color(self) -> Color32 {
        match self {
            Channel::Red => Color32::from_rgb(230, 70, 70),
            Channel::Green => Color32::from_rgb(80, 210, 90),
            Channel::Blue => Color32::from_rgb(90, 130, 245),
            _ => Color32::from_gray(225),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hist {
    pub bins: [u32; 256],
    /// In the channel's units (`Channel::value`).
    pub mean: f32,
    pub std: f32,
}

/// Plot sizes: the waveform's columns, every plot's rows (bottom = low), the vectorscope's square and each parade
/// channel's columns.
pub const WAVE_W: usize = 360;
pub const ROWS: usize = 256;
pub const VEC: usize = 256;
pub const PARADE_W: usize = 240;

/// The waveform's IRE span, bottom to top: composite needs room for chroma above white and below black.
pub fn wave_range(composite: bool) -> (f32, f32) {
    if composite { (-40.0, 140.0) } else { (-20.0, 120.0) }
}

/// U or V at the vectorscope's edge (Normal; Magnified shows half of it), so 100 % red and cyan (0.63) fit.
pub const VEC_EDGE: f32 = 0.7;

/// Where (U, V) lands on the vectorscope, -1..1 from the centre, +V up.
pub fn vec_pos(u: f32, v: f32, magnified: bool) -> (f32, f32) {
    let k = if magnified { 2.0 } else { 1.0 } / VEC_EDGE;
    (u * k, v * k)
}

/// The vectorscope's targets: where 75 % colour bars land (label, U, V).
pub fn targets() -> [(&'static str, f32, f32); 6] {
    [("R", 1.0, 0.0, 0.0), ("Mg", 1.0, 0.0, 1.0), ("B", 0.0, 0.0, 1.0), ("Cy", 0.0, 1.0, 1.0), ("G", 0.0, 1.0, 0.0), ("Yl", 1.0, 1.0, 0.0)].map(|(n, r, g, b)| {
        let (_, u, v) = yuv(0.75 * r, 0.75 * g, 0.75 * b);
        (n, u, v)
    })
}

/// Hit counts on a `w` × `h` grid, row 0 at the top.
#[derive(Clone, Debug)]
pub struct Grid {
    pub w: usize,
    pub h: usize,
    pub n: Vec<u32>,
}

impl Grid {
    fn new(w: usize, h: usize) -> Self {
        Self { w, h, n: vec![0; w * h] }
    }

    /// A hit in column `x` at `f` of the height (0 bottom, 1 top); off the plot it sticks to the edge.
    fn hit(&mut self, x: usize, f: f32) {
        let row = ((1.0 - f.clamp(0.0, 1.0)) * (self.h - 1) as f32).round() as usize;
        self.n[row * self.w + x] += 1;
    }

    /// The traces: bins brighter the more pixels land in them (square root against an even spread of `samples`),
    /// each column in `color(column)`.
    pub fn image(&self, samples: usize, color: impl Fn(usize) -> Color32) -> ColorImage {
        let k = 0.25 * (self.w * self.h) as f32 / samples.max(1) as f32;
        let pixels = self
            .n
            .iter()
            .enumerate()
            .map(|(i, &c)| {
                if c == 0 {
                    return Color32::TRANSPARENT;
                }
                let a = (c as f32 * k).sqrt().clamp(0.2, 1.0);
                let [r, g, b, _] = color(i % self.w).to_array();
                let m = |v: u8| (v as f32 * a).round() as u8;
                Color32::from_rgba_premultiplied(m(r), m(g), m(b), (255.0 * a).round() as u8)
            })
            .collect();
        ColorImage { size: [self.w, self.h], pixels }
    }
}

/// What to measure, and how.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Want {
    pub wave: bool,
    pub vector: bool,
    pub parade: bool,
    pub hist: Option<HistMode>,
    pub levels: Levels,
    /// Waveform Settings: Composite (luma plus chroma) instead of Luminance.
    pub composite: bool,
    /// Vectorscope Settings: Magnified (2×) instead of Normal.
    pub magnified: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Scopes {
    pub wave: Option<Grid>,
    pub vector: Option<Grid>,
    pub parade: Option<Grid>,
    pub hist: Vec<(Channel, Hist)>,
    /// Pixels read (every one, or every n-th of a large frame in both directions).
    pub samples: usize,
}

/// sRGB code value to linear light.
fn linear(c: u8) -> f32 {
    let v = c as f32 / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

/// CIE L*a*b* (D65) of an sRGB pixel, from a linear-light table.
fn lab(lin: &[f32; 256], r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let (r, g, b) = (lin[r as usize], lin[g as usize], lin[b as usize]);
    let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
    let f = |t: f32| if t > 216.0 / 24389.0 { t.cbrt() } else { (24389.0 / 27.0 * t + 16.0) / 116.0 };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    (116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz))
}

/// One pass over `img` for every scope `want` asks for.
pub fn analyze(img: &ColorImage, want: &Want) -> Scopes {
    let [w, h] = img.size;
    let lv = want.levels;
    let mut s = Scopes {
        wave: want.wave.then(|| Grid::new(WAVE_W, ROWS)),
        vector: want.vector.then(|| Grid::new(VEC, VEC)),
        parade: want.parade.then(|| Grid::new(3 * PARADE_W, ROWS)),
        ..Default::default()
    };
    let chans = want.hist.map_or(&[][..], HistMode::channels);
    let mut bins = vec![[0u32; 256]; chans.len()];
    let lab_on = want.hist == Some(HistMode::Lab);
    let lin: [f32; 256] = std::array::from_fn(|c| linear(c as u8));
    let (lo, hi) = wave_range(want.composite);
    let chroma_k = if lv.setup { 92.5 } else { 100.0 };
    // shortcut: a frame over 0.25 MP is read every `step`-th pixel both ways (960x540: every 2nd, 1080p: every 3rd),
    // which keeps all four scopes near 5 ms; read more if a fine detail ever goes missing.
    let step = ((w * h) as f32 / 0.25e6).sqrt().ceil().max(1.0) as usize;
    for y in (0..h).step_by(step) {
        let row = &img.pixels[y * w..(y + 1) * w];
        for x in (0..w).step_by(step) {
            let [r8, g8, b8, a8] = row[x].to_array();
            s.samples += 1;
            if s.wave.is_some() || s.vector.is_some() {
                let (yy, u, v) = yuv(lv.unit(r8), lv.unit(g8), lv.unit(b8));
                if let Some(g) = &mut s.wave {
                    let col = x * WAVE_W / w;
                    let ire = lv.ire(yy);
                    if want.composite {
                        // shortcut: the two peaks of the composite signal (luma ± chroma), not the whole swing between.
                        let c = chroma_k * (u * u + v * v).sqrt();
                        g.hit(col, (ire + c - lo) / (hi - lo));
                        g.hit(col, (ire - c - lo) / (hi - lo));
                    } else {
                        g.hit(col, (ire - lo) / (hi - lo));
                    }
                }
                if let Some(g) = &mut s.vector {
                    let (px, py) = vec_pos(u, v, want.magnified);
                    if px.abs() <= 1.0 && py.abs() <= 1.0 {
                        let col = ((px + 1.0) * 0.5 * (VEC - 1) as f32).round() as usize;
                        g.hit(col, (py + 1.0) * 0.5);
                    }
                }
            }
            if let Some(g) = &mut s.parade {
                for (k, c) in [r8, g8, b8].into_iter().enumerate() {
                    g.hit(k * PARADE_W + x * PARADE_W / w, c as f32 / 255.0);
                }
            }
            if !chans.is_empty() {
                let l = if lab_on { lab(&lin, r8, g8, b8) } else { (0.0, 0.0, 0.0) };
                for (b, ch) in bins.iter_mut().zip(chans) {
                    let k = match ch {
                        Channel::Luma => (0.299 * r8 as f32 + 0.587 * g8 as f32 + 0.114 * b8 as f32).round(),
                        Channel::Red => r8 as f32,
                        Channel::Green => g8 as f32,
                        Channel::Blue => b8 as f32,
                        Channel::Alpha => a8 as f32,
                        Channel::L => (l.0 * 2.55).round(),
                        Channel::A => (l.1 + 128.0).round(),
                        Channel::B => (l.2 + 128.0).round(),
                    };
                    b[k.clamp(0.0, 255.0) as usize] += 1;
                }
            }
        }
    }
    s.hist = chans.iter().zip(bins).map(|(&ch, bins)| (ch, hist(ch, bins))).collect();
    s
}

/// Mean and standard deviation of `bins`, in `ch`'s units.
fn hist(ch: Channel, bins: [u32; 256]) -> Hist {
    let n: f64 = bins.iter().map(|&c| c as f64).sum();
    let at = |k: usize| ch.value(k as f32) as f64;
    let mean = if n > 0.0 { bins.iter().enumerate().map(|(k, &c)| c as f64 * at(k)).sum::<f64>() / n } else { 0.0 };
    let var = if n > 0.0 { bins.iter().enumerate().map(|(k, &c)| c as f64 * (at(k) - mean).powi(2)).sum::<f64>() / n } else { 0.0 };
    Hist { bins, mean: mean as f32, std: var.sqrt() as f32 }
}

/// `cur` with `region` (0-1 of the frame: x0, y0, x1, y1) taken from `other` at the same place, whatever its size:
/// what the Video Preview shows in Split-Screen View › Clipboard, which the scopes then measure.
pub fn splice(cur: &ColorImage, other: &ColorImage, region: [f32; 4]) -> ColorImage {
    let [w, h] = cur.size;
    let [ow, oh] = other.size;
    let mut out = cur.clone();
    if ow == 0 || oh == 0 {
        return out;
    }
    let span = |a: f32, b: f32, n: usize| ((a.clamp(0.0, 1.0) * n as f32).round() as usize, (b.clamp(0.0, 1.0) * n as f32).round() as usize);
    let ((x0, x1), (y0, y1)) = (span(region[0], region[2], w), span(region[1], region[3], h));
    for y in y0..y1 {
        let sy = ((y as f32 + 0.5) / h as f32 * oh as f32) as usize;
        for x in x0..x1 {
            let sx = ((x as f32 + 0.5) / w as f32 * ow as f32) as usize;
            out.pixels[y * w + x] = other.pixels[sy.min(oh - 1) * ow + sx.min(ow - 1)];
        }
    }
    out
}

/// At most 10 analyses a second (VEGAS updates its scopes about that often).
#[derive(Default)]
pub struct Throttle {
    last: Option<f64>,
}

impl Throttle {
    pub const GAP: f64 = 0.1;

    /// Seconds to wait before an analysis may start at `now` (0 = now; it is then counted as started).
    pub fn wait(&mut self, now: f64) -> f64 {
        let left = self.last.map_or(0.0, |l| Self::GAP - (now - l));
        if left > 0.0 {
            return left;
        }
        self.last = Some(now);
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(c: Color32, w: usize, h: usize) -> ColorImage {
        ColorImage { size: [w, h], pixels: vec![c; w * h] }
    }

    fn all(levels: Levels) -> Want {
        Want { wave: true, vector: true, parade: true, hist: Some(HistMode::LumaRgb), levels, composite: false, magnified: false }
    }

    /// The rows of column `x` with hits, as fractions of the height from the bottom.
    fn hits(g: &Grid, x: usize) -> Vec<f32> {
        (0..g.h).filter(|r| g.n[r * g.w + x] > 0).map(|r| 1.0 - r as f32 / (g.h - 1) as f32).collect()
    }

    /// The IRE a waveform row stands for.
    fn ire_at(f: f32, composite: bool) -> f32 {
        let (lo, hi) = wave_range(composite);
        lo + f * (hi - lo)
    }

    /// A 100 % white frame peaks at 100 IRE on the waveform and 255 on the parade and histogram; black sits at 0
    /// (7.5 IRE with setup); with Studio RGB, 235 is white and 16 black.
    #[test]
    fn white_peaks_at_100_ire_and_255() {
        let tol = (wave_range(false).1 - wave_range(false).0) / (ROWS - 1) as f32;
        let s = analyze(&solid(Color32::WHITE, 64, 36), &all(Levels::default()));
        let wave = s.wave.as_ref().unwrap();
        assert!((ire_at(hits(wave, 0)[0], false) - 100.0).abs() <= tol * 0.5, "{:?}", hits(wave, 0));
        let parade = s.parade.as_ref().unwrap();
        for k in 0..3 {
            assert_eq!(hits(parade, k * PARADE_W), vec![1.0]);
        }
        for (_, h) in &s.hist {
            assert_eq!((h.bins[255], h.mean, h.std), (64 * 36, 255.0, 0.0));
        }
        let ire = |c: u8, levels| ire_at(hits(analyze(&solid(Color32::from_gray(c), 8, 8), &all(levels)).wave.as_ref().unwrap(), 0)[0], false);
        assert!(ire(0, Levels::default()).abs() <= tol * 0.5);
        assert!((ire(0, Levels { setup: true, studio: false }) - 7.5).abs() <= tol * 0.5);
        let studio = Levels { setup: false, studio: true };
        assert!((ire(235, studio) - 100.0).abs() <= tol * 0.5 && ire(16, studio).abs() <= tol * 0.5);
        // The setting does not change the histogram: 235 is still bin 235.
        assert_eq!(analyze(&solid(Color32::from_gray(235), 8, 8), &all(studio)).hist[0].1.bins[235], 64);
    }

    /// 75 % colour bars land on the vectorscope's targets (within 2 % of its width), with computer RGB and, at their
    /// studio levels, with Studio RGB; pure red sits on the red target's line, further out.
    #[test]
    fn colour_bars_land_on_the_vectorscope_targets() {
        for studio in [false, true] {
            let level = |on: bool| if !on { if studio { 16 } else { 0 } } else if studio { (16.0 + 0.75 * 219.0_f32).round() as u8 } else { 191 };
            let bars = [(1, 1, 0), (0, 1, 1), (0, 1, 0), (1, 0, 1), (1, 0, 0), (0, 0, 1)];
            let (bw, h) = (10, 4);
            let mut img = solid(Color32::BLACK, bw * bars.len(), h);
            for (k, &(r, g, b)) in bars.iter().enumerate() {
                for y in 0..h {
                    for x in 0..bw {
                        img.pixels[y * img.size[0] + k * bw + x] = Color32::from_rgb(level(r == 1), level(g == 1), level(b == 1));
                    }
                }
            }
            let s = analyze(&img, &all(Levels { setup: false, studio }));
            let g = s.vector.unwrap();
            let pos = |i: usize| {
                let (col, row) = (i % g.w, i / g.w);
                (col as f32 / (g.w - 1) as f32 * 2.0 - 1.0, 1.0 - row as f32 / (g.h - 1) as f32 * 2.0)
            };
            let lit: Vec<(f32, f32)> = (0..g.n.len()).filter(|&i| g.n[i] > 0).map(pos).collect();
            assert_eq!(lit.len(), 6, "{lit:?}");
            for (name, u, v) in targets() {
                let (tx, ty) = vec_pos(u, v, false);
                // 2 % of the scope's width, which spans -1..1.
                assert!(lit.iter().any(|&(x, y)| (x - tx).abs() <= 0.04 && (y - ty).abs() <= 0.04), "{name} studio={studio}: {lit:?}");
            }
        }
        let red = analyze(&solid(Color32::from_rgb(255, 0, 0), 4, 4), &all(Levels::default())).vector.unwrap();
        let i = red.n.iter().position(|&c| c > 0).unwrap();
        let (x, y) = ((i % VEC) as f32 / (VEC - 1) as f32 * 2.0 - 1.0, 1.0 - (i / VEC) as f32 / (VEC - 1) as f32 * 2.0);
        let (_, u, v) = targets()[0];
        let (tx, ty) = vec_pos(u, v, false);
        assert!((y.atan2(x) - ty.atan2(tx)).abs() < 0.03, "red at {x},{y}");
        assert!(((x * x + y * y).sqrt() / (tx * tx + ty * ty).sqrt() - 1.0 / 0.75).abs() < 0.03);
        assert!(y > 0.0 && x < 0.0, "red is up and to the left, as on an NTSC graticule");
    }

    /// A black frame's histograms are all at 0; counts, mean and standard deviation of a half-black, half-white frame.
    #[test]
    fn histogram_counts() {
        let s = analyze(&solid(Color32::BLACK, 20, 10), &all(Levels::default()));
        for (_, h) in &s.hist {
            assert_eq!((h.bins[0], h.bins[1..].iter().sum::<u32>(), h.mean, h.std), (200, 0, 0.0, 0.0));
        }
        let mut img = solid(Color32::BLACK, 20, 10);
        img.pixels[..100].fill(Color32::WHITE);
        let want = Want { hist: Some(HistMode::Luminance), ..all(Levels::default()) };
        let h = &analyze(&img, &want).hist[0].1;
        assert_eq!((h.bins[0], h.bins[255], h.mean, h.std), (100, 100, 127.5, 127.5));
        // Alpha counts the alpha channel; Lightness/a/b puts white at L 100 and a, b at 0.
        let alpha = analyze(&solid(Color32::from_rgba_premultiplied(64, 0, 0, 128), 4, 4), &Want { hist: Some(HistMode::Alpha), ..want });
        assert_eq!(alpha.hist[0].1.bins[128], 16);
        let lab = analyze(&solid(Color32::WHITE, 4, 4), &Want { hist: Some(HistMode::Lab), ..want });
        let means: Vec<f32> = lab.hist.iter().map(|(_, h)| h.mean).collect();
        assert!((means[0] - 100.0).abs() < 0.5 && means[1].abs() < 1.0 && means[2].abs() < 1.0, "{means:?}");
    }

    /// Composite adds chroma either side of luma: 100 % yellow reaches about 133 IRE, grey has none.
    #[test]
    fn composite_waveform_shows_chroma() {
        let want = Want { composite: true, ..all(Levels::default()) };
        let yl = analyze(&solid(Color32::from_rgb(255, 255, 0), 4, 4), &want).wave.unwrap();
        let top = ire_at(hits(&yl, 0)[0], true);
        assert!((top - 133.0).abs() < 3.0, "{top}");
        let grey = analyze(&solid(Color32::from_gray(128), 4, 4), &want).wave.unwrap();
        assert_eq!(hits(&grey, 0).len(), 1);
    }

    /// The scopes measure what Split-Screen View shows: the clipboard frame inside the region.
    #[test]
    fn splice_takes_the_region_from_the_other_frame() {
        let out = splice(&solid(Color32::BLACK, 8, 4), &solid(Color32::WHITE, 16, 8), [0.5, 0.0, 1.0, 1.0]);
        assert_eq!(out.pixels.iter().filter(|&&p| p == Color32::WHITE).count(), 16);
        assert_eq!((out.pixels[3], out.pixels[4]), (Color32::BLACK, Color32::WHITE));
    }

    /// At most one analysis per 0.1 s, however often the UI asks; a request in between is told how long to wait.
    #[test]
    fn throttle_allows_ten_a_second() {
        let mut t = Throttle::default();
        let started = (0..1000).map(|k| k as f64 * 0.004).filter(|&now| t.wait(now) == 0.0).count();
        // 4 s of 250 requests a second.
        assert!((40..=41).contains(&started), "{started}");
        let mut t = Throttle::default();
        assert_eq!(t.wait(5.0), 0.0);
        assert!((t.wait(5.03) - 0.07).abs() < 1e-9);
        assert_eq!(t.wait(5.11), 0.0);
    }

    /// `cargo test --release -- --ignored --nocapture bench_scopes`: the cost of one analysis of a preview frame.
    #[test]
    #[ignore]
    fn bench_scopes() {
        for (w, h) in [(480, 270), (960, 540), (1920, 1080)] {
            let img = ColorImage { size: [w, h], pixels: (0..w * h).map(|i| Color32::from_rgb((i % 251) as u8, (i % 241) as u8, (i / w % 256) as u8)).collect() };
            for (name, want) in [
                ("waveform", Want { vector: false, parade: false, hist: None, ..all(Levels::default()) }),
                ("vectorscope", Want { wave: false, parade: false, hist: None, ..all(Levels::default()) }),
                ("histogram L/R/G/B", Want { wave: false, vector: false, parade: false, ..all(Levels::default()) }),
                ("histogram Lab", Want { wave: false, vector: false, parade: false, hist: Some(HistMode::Lab), ..all(Levels::default()) }),
                ("all four", all(Levels::default())),
            ] {
                let t = std::time::Instant::now();
                for _ in 0..20 {
                    std::hint::black_box(analyze(&img, &want));
                }
                let ms = t.elapsed().as_secs_f64() * 1000.0 / 20.0;
                println!("bench_scopes {w}x{h} {name}: {ms:.2} ms ({:.1} % of a core at 10/s)", ms);
            }
        }
    }
}
