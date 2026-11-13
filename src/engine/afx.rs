//! Audio FX (batch 16): the built-in plug-ins' DSP, made by plug-in id (their parameters and presets live in the
//! registry, `plugins.rs`), and `Chain`, which runs an event's, track's or the master's plug-in list over interleaved
//! stereo blocks at RATE and keeps each instance's state by its id.
// shortcut: parameters are read once per 1024-frame block; add per-sample smoothing if zipper noise is heard.
use crate::audio::RATE;
use crate::engine::plugins::{self, FxEval};
use crate::project::Fx;

const FS: f64 = RATE as f64;

pub trait AudioFx: Send {
    /// Process interleaved stereo `buf` in place with the values `p`.
    fn process(&mut self, p: &FxEval, buf: &mut [f32]);
    /// Forget all state: silent delay lines, gains at rest.
    fn reset(&mut self);
    /// With `p` it would pass the signal through unchanged, so the chain skips it.
    fn neutral(&self, p: &FxEval) -> bool;
}

/// The DSP of a built-in audio plug-in; None for any other id.
pub fn new(plugin: &str) -> Option<Box<dyn AudioFx>> {
    Some(match plugin {
        "milan.afx.gate" => Box::new(Gate::default()),
        "milan.afx.eq" => Box::<Eq>::default(),
        "milan.afx.compressor" => Box::new(Compressor::default()),
        "milan.afx.reverb" => Box::new(Reverb::new()),
        "milan.afx.delay" => Box::new(Delay::new()),
        "milan.afx.limiter" => Box::new(Limiter::new()),
        _ => return None,
    })
}

/// Some plug-in of `fx` changes the sound at `t`: enabled, built in and not at neutral settings (a track's fx dot).
pub fn audible(fx: &[Fx], t: f64) -> bool {
    fx.iter().any(|f| f.enabled && new(&f.plugin).zip(plugins::eval(f, t)).is_some_and(|(d, p)| !d.neutral(&p)))
}

/// One-pole smoothing coefficient for a time constant of `ms`.
fn coef(ms: f32) -> f32 {
    1.0 - (-1.0 / (ms as f64 * 1e-3 * FS).max(1.0)).exp() as f32
}

fn gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Tiny recursive values go to 0, so they never decay into slow denormals.
fn flush(x: f32) -> f32 {
    if x.abs() < 1e-20 { 0.0 } else { x }
}

struct Slot {
    id: String,
    plugin: String,
    dsp: Option<Box<dyn AudioFx>>,
    /// Skipped last block (disabled or neutral): reset before it runs again, so no stale tail comes back.
    idle: bool,
}

/// The running state of one plug-in list.
#[derive(Default)]
pub struct Chain {
    slots: Vec<Slot>,
}

impl Chain {
    /// Follow `fx`: slots whose id and plug-in still match keep their state; new ones start fresh.
    pub fn sync(&mut self, fx: &[Fx]) {
        if self.slots.len() == fx.len() && self.slots.iter().zip(fx).all(|(s, f)| s.id == f.id && s.plugin == f.plugin) {
            return;
        }
        let mut old = std::mem::take(&mut self.slots);
        self.slots = fx
            .iter()
            .map(|f| match old.iter().position(|s| s.id == f.id && s.plugin == f.plugin) {
                Some(i) => old.swap_remove(i),
                None => Slot { id: f.id.clone(), plugin: f.plugin.clone(), dsp: new(&f.plugin), idle: true },
            })
            .collect();
    }

    /// Run `fx` over `buf` with its values at `t` (event-relative for event FX). Disabled, unknown and neutral
    /// plug-ins pass the signal through.
    pub fn run(&mut self, fx: &[Fx], t: f64, buf: &mut [f32]) {
        let vals: Vec<Option<FxEval>> = fx.iter().map(|f| f.enabled.then(|| plugins::eval(f, t)).flatten()).collect();
        self.run_with(fx, &vals, buf);
    }

    /// R9: `run` with the values already evaluated (one per plug-in of `fx`, None to pass through), so the real-time
    /// input monitor allocates nothing while the chain keeps its plug-ins.
    pub fn run_with(&mut self, fx: &[Fx], vals: &[Option<FxEval>], buf: &mut [f32]) {
        self.sync(fx);
        for (s, p) in self.slots.iter_mut().zip(vals) {
            let (Some(dsp), Some(p)) = (s.dsp.as_mut(), p) else {
                s.idle = true;
                continue;
            };
            if dsp.neutral(p) {
                s.idle = true;
                continue;
            }
            if s.idle {
                dsp.reset();
                s.idle = false;
            }
            dsp.process(p, buf);
        }
    }
}

// ---- Track Noise Gate ----

/// Stereo-linked: opens while the peak level is at or above the threshold.
#[derive(Default)]
struct Gate {
    det: f32,
    g: f32,
}

impl AudioFx for Gate {
    fn process(&mut self, p: &FxEval, buf: &mut [f32]) {
        let thr = gain(p.f("threshold"));
        let (att, rel) = (coef(p.f("attack")), coef(p.f("release")));
        // The detector holds the peaks for the release time, so a low note does not chatter between its peaks.
        let fall = 1.0 - coef(p.f("release").max(20.0));
        for fr in buf.chunks_exact_mut(2) {
            self.det = flush((fr[0].abs().max(fr[1].abs())).max(self.det * fall));
            let target = if self.det >= thr { 1.0 } else { 0.0 };
            self.g = flush(self.g + (target - self.g) * if target > self.g { att } else { rel });
            fr[0] *= self.g;
            fr[1] *= self.g;
        }
    }

    fn reset(&mut self) {
        *self = Self::default();
    }

    /// The default threshold, −96 dB, is off.
    fn neutral(&self, p: &FxEval) -> bool {
        p.f("threshold") <= -96.0
    }
}

// ---- Track EQ ----

#[derive(Clone, Copy, PartialEq)]
enum Shape {
    LowShelf,
    HighShelf,
    Peak,
    HighPass,
    LowPass,
}

/// RBJ cookbook biquad, normalised: [b0, b1, b2, a1, a2].
fn design(shape: Shape, f: f64, g_db: f64, q: f64) -> [f64; 5] {
    let w = std::f64::consts::TAU * f.clamp(10.0, 0.45 * FS) / FS;
    let (cos, alpha) = (w.cos(), w.sin() / (2.0 * q.max(0.05)));
    let a = 10f64.powf(g_db / 40.0);
    let sq = 2.0 * a.sqrt() * alpha;
    let [b0, b1, b2, a0, a1, a2] = match shape {
        Shape::Peak => [1.0 + alpha * a, -2.0 * cos, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * cos, 1.0 - alpha / a],
        Shape::LowShelf => [
            a * ((a + 1.0) - (a - 1.0) * cos + sq),
            2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
            a * ((a + 1.0) - (a - 1.0) * cos - sq),
            (a + 1.0) + (a - 1.0) * cos + sq,
            -2.0 * ((a - 1.0) + (a + 1.0) * cos),
            (a + 1.0) + (a - 1.0) * cos - sq,
        ],
        Shape::HighShelf => [
            a * ((a + 1.0) + (a - 1.0) * cos + sq),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
            a * ((a + 1.0) + (a - 1.0) * cos - sq),
            (a + 1.0) - (a - 1.0) * cos + sq,
            2.0 * ((a - 1.0) - (a + 1.0) * cos),
            (a + 1.0) - (a - 1.0) * cos - sq,
        ],
        Shape::HighPass => [(1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha],
        Shape::LowPass => [(1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha],
    };
    [b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0]
}

/// One stereo band: transposed direct form II with f64 state; coefficients are recomputed only when values change.
#[derive(Default)]
struct Biquad {
    c: [f64; 5],
    s: [[f64; 2]; 2],
    key: Option<(u8, f32, f32, f32)>,
}

impl Biquad {
    fn run(&mut self, shape: Shape, f: f32, g: f32, q: f32, buf: &mut [f32]) {
        let key = Some((shape as u8, f, g, q));
        if self.key != key {
            (self.c, self.key) = (design(shape, f as f64, g as f64, q as f64), key);
        }
        let [b0, b1, b2, a1, a2] = self.c;
        for fr in buf.chunks_exact_mut(2) {
            for (x, s) in fr.iter_mut().zip(&mut self.s) {
                let xi = *x as f64;
                let y = b0 * xi + s[0];
                s[0] = b1 * xi - a1 * y + s[1];
                s[1] = b2 * xi - a2 * y;
                for v in s.iter_mut().filter(|v| v.abs() < 1e-30) {
                    *v = 0.0;
                }
                *x = y as f32;
            }
        }
    }
}

/// Band 1 low shelf or high-pass, bands 2-3 peaks, band 4 high shelf or low-pass.
#[derive(Default)]
struct Eq {
    bands: [Biquad; 4],
}

impl Eq {
    fn shape(p: &FxEval, k: usize) -> Shape {
        match (k, p.i(&format!("b{}.type", k + 1))) {
            (0, 0) => Shape::LowShelf,
            (0, _) => Shape::HighPass,
            (3, 0) => Shape::HighShelf,
            (3, _) => Shape::LowPass,
            _ => Shape::Peak,
        }
    }
}

impl AudioFx for Eq {
    fn process(&mut self, p: &FxEval, buf: &mut [f32]) {
        for (k, b) in self.bands.iter_mut().enumerate() {
            let shape = Self::shape(p, k);
            let v = |name: &str| p.f(&format!("b{}.{name}", k + 1));
            // A shelf or peak at 0 dB is a straight wire: left out, so it stays bit-exact.
            if !matches!(shape, Shape::HighPass | Shape::LowPass) && v("gain") == 0.0 {
                // Forget its state, or the old tail would click in when the band comes back on.
                *b = Biquad::default();
                continue;
            }
            b.run(shape, v("freq"), v("gain"), v("q"), buf);
        }
    }

    fn reset(&mut self) {
        *self = Self::default();
    }

    fn neutral(&self, p: &FxEval) -> bool {
        (0..4).all(|k| matches!(Self::shape(p, k), Shape::LowShelf | Shape::HighShelf | Shape::Peak) && p.f(&format!("b{}.gain", k + 1)) == 0.0)
    }
}

// ---- Track Compressor ----

/// Feed-forward and stereo-linked: a peak detector (instant attack, release fall), then the gain reduction (dB)
/// smoothed with the attack and release times.
#[derive(Default)]
struct Compressor {
    det: f32,
    gr: f32,
}

impl AudioFx for Compressor {
    fn process(&mut self, p: &FxEval, buf: &mut [f32]) {
        let (thr, ratio, out) = (p.f("threshold"), p.f("ratio").max(1.0), p.f("output"));
        let (att, rel) = (coef(p.f("attack")), coef(p.f("release")));
        let (slope, fall) = (1.0 - 1.0 / ratio, 1.0 - rel);
        for fr in buf.chunks_exact_mut(2) {
            self.det = flush((fr[0].abs().max(fr[1].abs())).max(self.det * fall));
            let target = -(20.0 * self.det.max(1e-9).log10() - thr).max(0.0) * slope;
            self.gr = flush(self.gr + (target - self.gr) * if target < self.gr { att } else { rel });
            let g = gain(out + self.gr);
            fr[0] *= g;
            fr[1] *= g;
        }
    }

    fn reset(&mut self) {
        *self = Self::default();
    }

    /// The default ratio, 1:1, with no output gain.
    fn neutral(&self, p: &FxEval) -> bool {
        p.f("ratio") <= 1.0 && p.f("output") == 0.0
    }
}

// ---- Reverb (Freeverb) ----

const COMBS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASSES: [usize; 4] = [556, 441, 341, 225];
/// Right-channel lines are this much longer (at 44.1 kHz), for width.
const SPREAD: usize = 23;

struct Line {
    buf: Vec<f32>,
    i: usize,
    store: f32,
}

impl Line {
    fn new(len_44k: usize) -> Self {
        Self { buf: vec![0.0; (len_44k as f64 * FS / 44_100.0) as usize], i: 0, store: 0.0 }
    }

    fn comb(&mut self, x: f32, fb: f32, damp: f32) -> f32 {
        let y = self.buf[self.i];
        self.store = flush(y * (1.0 - damp) + self.store * damp);
        self.buf[self.i] = flush(x + self.store * fb);
        self.i = (self.i + 1) % self.buf.len();
        y
    }

    fn allpass(&mut self, x: f32) -> f32 {
        let b = self.buf[self.i];
        self.buf[self.i] = flush(x + b * 0.5);
        self.i = (self.i + 1) % self.buf.len();
        b - x
    }
}

struct Reverb {
    combs: [Vec<Line>; 2],
    allpasses: [Vec<Line>; 2],
}

impl Reverb {
    fn new() -> Self {
        let lines = |lens: &[usize], ch: usize| lens.iter().map(|&n| Line::new(n + ch * SPREAD)).collect();
        Self { combs: [lines(&COMBS, 0), lines(&COMBS, 1)], allpasses: [lines(&ALLPASSES, 0), lines(&ALLPASSES, 1)] }
    }
}

impl AudioFx for Reverb {
    fn process(&mut self, p: &FxEval, buf: &mut [f32]) {
        let (fb, damp) = (p.f("room") * 0.28 + 0.7, p.f("damp") * 0.4);
        // Freeverb's wet scale is 3.
        let (wet, width, dry) = (p.f("wet") * 3.0, p.f("width"), p.f("dry"));
        let (wet1, wet2) = (wet * (width / 2.0 + 0.5), wet * (1.0 - width) / 2.0);
        for fr in buf.chunks_exact_mut(2) {
            let input = (fr[0] + fr[1]) * 0.015;
            let mut o = [0.0f32; 2];
            for (ch, o) in o.iter_mut().enumerate() {
                let mut y: f32 = self.combs[ch].iter_mut().map(|c| c.comb(input, fb, damp)).sum();
                for a in &mut self.allpasses[ch] {
                    y = a.allpass(y);
                }
                *o = y;
            }
            (fr[0], fr[1]) = (o[0] * wet1 + o[1] * wet2 + fr[0] * dry, o[1] * wet1 + o[0] * wet2 + fr[1] * dry);
        }
    }

    fn reset(&mut self) {
        for l in self.combs.iter_mut().chain(&mut self.allpasses).flatten() {
            l.buf.fill(0.0);
            (l.i, l.store) = (0, 0.0);
        }
    }

    fn neutral(&self, p: &FxEval) -> bool {
        p.f("wet") == 0.0 && p.f("dry") == 1.0
    }
}

// ---- Simple Delay ----

/// Up to 5 s.
struct Delay {
    buf: [Vec<f32>; 2],
    w: usize,
}

impl Delay {
    fn new() -> Self {
        let n = 5 * RATE as usize + 1;
        Self { buf: [vec![0.0; n], vec![0.0; n]], w: 0 }
    }
}

impl AudioFx for Delay {
    fn process(&mut self, p: &FxEval, buf: &mut [f32]) {
        let n = self.buf[0].len();
        let d = ((p.f("delay") as f64 * FS / 1000.0).round() as usize).clamp(1, n - 1);
        let (fb, dry, wet) = (p.f("feedback"), p.f("dry"), p.f("wet"));
        for fr in buf.chunks_exact_mut(2) {
            let r = (self.w + n - d) % n;
            for (x, line) in fr.iter_mut().zip(&mut self.buf) {
                let y = line[r];
                line[self.w] = flush(*x + y * fb);
                *x = *x * dry + y * wet;
            }
            self.w = (self.w + 1) % n;
        }
    }

    fn reset(&mut self) {
        self.buf.iter_mut().for_each(|b| b.fill(0.0));
        self.w = 0;
    }

    fn neutral(&self, p: &FxEval) -> bool {
        p.f("wet") == 0.0 && p.f("dry") == 1.0
    }
}

// ---- Limiter ----

/// 1.5 ms lookahead at 48 kHz.
const LOOK: usize = 72;

/// Brickwall: the gain reaches the level a peak needs before the peak leaves the lookahead, and never lets a sample
/// out above the ceiling. The output is LOOK samples late.
// shortcut: the 1.5 ms delay is not compensated (and cuts the last 1.5 ms of an event's FX); add plug-in delay
// compensation in the mixer if it is heard against other tracks.
struct Limiter {
    x: [[f32; LOOK]; 2],
    /// The gain each sample in the lookahead needs.
    need: [f32; LOOK],
    i: usize,
    g: f32,
}

impl Limiter {
    fn new() -> Self {
        Self { x: [[0.0; LOOK]; 2], need: [1.0; LOOK], i: 0, g: 1.0 }
    }
}

impl AudioFx for Limiter {
    fn process(&mut self, p: &FxEval, buf: &mut [f32]) {
        let (input, ceil) = (gain(p.f("input")), gain(p.f("ceiling")));
        // Mostly down within the lookahead; the clamp below catches the rest.
        let (att, rel) = (1.0 - (-4.0 / LOOK as f32).exp(), coef(p.f("release")));
        for fr in buf.chunks_exact_mut(2) {
            let (l, r) = (fr[0] * input, fr[1] * input);
            let need = (ceil / l.abs().max(r.abs()).max(1e-9)).min(1.0);
            let i = self.i;
            let (ol, or, oneed) = (self.x[0][i], self.x[1][i], self.need[i]);
            (self.x[0][i], self.x[1][i], self.need[i]) = (l, r, need);
            self.i = (i + 1) % LOOK;
            let target = self.need.iter().fold(oneed, |m, &v| m.min(v));
            self.g += (target - self.g) * if target < self.g { att } else { rel };
            let g = self.g.min(oneed);
            fr[0] = (ol * g).clamp(-ceil, ceil);
            fr[1] = (or * g).clamp(-ceil, ceil);
        }
    }

    fn reset(&mut self) {
        *self = Self::new();
    }

    fn neutral(&self, _: &FxEval) -> bool {
        false
    }
}

/// A built-in plug-in with `vals` set (tests).
#[cfg(test)]
pub fn test_fx(id: &str, vals: &[(&str, f32)]) -> Fx {
    use crate::project::{Curve, Keyframe, Keyframes};
    let mut f = plugins::new_fx(plugins::find(id).unwrap(), None);
    for &(k, v) in vals {
        f.params.insert(k.into(), Keyframes { keys: vec![Keyframe { t: 0.0, v, curve: Curve::Linear }] });
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::test_fx as fx;

    /// `f` run over `input` (one channel, copied to both) in 1024-frame blocks; the left channel back.
    fn run(f: &Fx, input: &[f32]) -> Vec<f32> {
        let mut chain = Chain::default();
        let mut buf: Vec<f32> = input.iter().flat_map(|&v| [v, v]).collect();
        for b in buf.chunks_mut(2048) {
            chain.run(std::slice::from_ref(f), 0.0, b);
        }
        buf.iter().step_by(2).copied().collect()
    }

    fn sine(hz: f64, amp: f32, n: usize) -> Vec<f32> {
        (0..n).map(|k| amp * (std::f64::consts::TAU * hz * k as f64 / FS).sin() as f32).collect()
    }

    fn rms_db(v: &[f32]) -> f64 {
        10.0 * (v.iter().map(|&x| x as f64 * x as f64).sum::<f64>() / v.len() as f64).log10()
    }

    fn peak(v: &[f32]) -> f32 {
        v.iter().fold(0.0, |m, x| m.max(x.abs()))
    }

    /// Gain in dB of `f` on a `hz` sine, measured over the last 0.1 s of 1 s (whole periods at 48 kHz).
    fn response(f: &Fx, hz: f64) -> f64 {
        let x = sine(hz, 0.25, RATE as usize);
        let y = run(f, &x);
        let tail = RATE as usize - 4800;
        rms_db(&y[tail..]) - rms_db(&x[tail..])
    }

    #[test]
    fn eq_is_bit_exact_at_0_db_and_its_bands_measure_right() {
        let x = sine(997.0, 0.5, 4096);
        let eq = fx("milan.afx.eq", &[]);
        assert_eq!(run(&eq, &x), x);
        let mut e = Eq::default();
        let mut buf: Vec<f32> = x.iter().flat_map(|&v| [v, v]).collect();
        let before = buf.clone();
        e.process(&plugins::eval(&eq, 0.0).unwrap(), &mut buf);
        assert_eq!(buf, before, "processing itself is exact too");
        let g = response(&fx("milan.afx.eq", &[("b2.freq", 1000.0), ("b2.gain", 6.0), ("b2.q", 1.0)]), 1000.0);
        assert!((g - 6.0).abs() < 0.1, "peak {g}");
        // A low shelf passes DC at its gain.
        let dc = run(&fx("milan.afx.eq", &[("b1.gain", 6.0), ("b1.freq", 100.0)]), &vec![0.25; RATE as usize]);
        assert!((20.0 * (dc[dc.len() - 1] / 0.25).log10() - 6.0).abs() < 0.01, "{}", dc[dc.len() - 1]);
        let hp = response(&fx("milan.afx.eq", &[("b1.type", 1.0), ("b1.freq", 100.0), ("b1.q", 0.7071)]), 100.0);
        assert!((hp + 3.0).abs() < 0.2, "high-pass {hp}");
        assert!(response(&fx("milan.afx.eq", &[("b4.type", 1.0), ("b4.freq", 1000.0)]), 8000.0) < -30.0, "low-pass");
    }

    #[test]
    fn eq_band_switched_back_on_starts_fresh() {
        let mut e = Eq::default();
        let mut buf: Vec<f32> = sine(500.0, 0.5, 1024).iter().flat_map(|&v| [v, v]).collect();
        e.process(&plugins::eval(&fx("milan.afx.eq", &[("b3.gain", 6.0)]), 0.0).unwrap(), &mut buf);
        // Band 3 off while band 2 keeps the EQ running, then back on: silence must stay silent.
        let both = fx("milan.afx.eq", &[("b2.gain", 6.0), ("b3.gain", 6.0)]);
        e.process(&plugins::eval(&fx("milan.afx.eq", &[("b2.gain", 6.0)]), 0.0).unwrap(), &mut [0.0; 2]);
        let mut quiet = [0.0f32; 8];
        e.process(&plugins::eval(&both, 0.0).unwrap(), &mut quiet);
        assert_eq!(quiet, [0.0; 8]);
    }

    #[test]
    fn compressor_ratio_and_neutral() {
        let x = sine(1000.0, gain(-10.0), RATE as usize);
        let c = fx("milan.afx.compressor", &[("threshold", -20.0), ("ratio", 4.0)]);
        let y = run(&c, &x);
        let out = 20.0 * peak(&y[RATE as usize - 4800..]).log10();
        assert!((out + 17.5).abs() < 0.3, "{out}");
        assert_eq!(run(&fx("milan.afx.compressor", &[("threshold", -40.0)]), &x), x, "ratio 1 is neutral");
    }

    #[test]
    fn gate_closes_below_the_threshold() {
        let n = RATE as usize / 2;
        let mut x = sine(220.0, gain(-10.0), n);
        x.extend(sine(220.0, gain(-40.0), 3 * n));
        let y = run(&fx("milan.afx.gate", &[("threshold", -30.0)]), &x);
        assert!(peak(&y[n - 4800..n]) > 0.95 * gain(-10.0), "open above it");
        let rest = 20.0 * (peak(&y[4 * n - 4800..]) / gain(-40.0)).log10();
        assert!(rest <= -60.0, "{rest} dB");
        assert_eq!(run(&fx("milan.afx.gate", &[]), &x), x, "-96 dB is off");
    }

    #[test]
    fn delay_echoes_and_feedback_decays_geometrically() {
        let mut x = vec![0.0; RATE as usize];
        x[0] = 1.0;
        let y = run(&fx("milan.afx.delay", &[("delay", 100.0), ("feedback", 0.5), ("wet", 0.8), ("dry", 1.0)]), &x);
        let d = 4800;
        assert_eq!((y[0], y[1], y[d - 1]), (1.0, 0.0, 0.0));
        assert_eq!((y[d], y[2 * d], y[3 * d]), (0.8, 0.4, 0.2));
        assert_eq!(y[d + 1], 0.0);
    }

    #[test]
    fn reverb_is_finite_and_its_tail_decays() {
        let mut x = sine(440.0, 0.5, RATE as usize / 2);
        x.resize(6 * RATE as usize, 0.0);
        let y = run(&fx("milan.afx.reverb", &[("room", 0.9)]), &x);
        assert!(y.iter().all(|v| v.is_finite()));
        let s = RATE as usize;
        let (a, b) = (peak(&y[s..2 * s]), peak(&y[5 * s..]));
        assert!(a > 1e-3 && b < a * 0.1, "tail {a} then {b}");
    }

    #[test]
    fn limiter_never_exceeds_the_ceiling() {
        let x = sine(1000.0, gain(12.0), RATE as usize);
        let ceil = gain(-1.0);
        let y = run(&fx("milan.afx.limiter", &[("ceiling", -1.0)]), &x);
        assert!(peak(&y) <= ceil, "{}", peak(&y));
        assert!(peak(&y[RATE as usize / 2..]) > 0.95 * ceil, "it still passes the signal");
    }

    #[test]
    fn audible_ignores_neutral_disabled_and_unknown_plug_ins() {
        let mut chain = crate::project::default_track_chain(None);
        assert!(chain.len() == 3 && !audible(&chain, 0.0), "the default track chain is neutral");
        let mut c = fx("milan.afx.compressor", &[("ratio", 4.0)]);
        c.enabled = false;
        chain.extend([c.clone(), fx("milan.fx.invert", &[])]);
        assert!(!audible(&chain, 0.0));
        c.enabled = true;
        chain.push(c);
        assert!(audible(&chain, 0.0));
    }

    #[test]
    fn chain_keeps_state_by_id_and_bypasses_disabled_slots() {
        let d = fx("milan.afx.delay", &[("delay", 2.0), ("feedback", 0.0), ("wet", 1.0), ("dry", 0.0)]);
        let mut c = Chain::default();
        let mut impulse = vec![0.0f32; 128];
        impulse[0] = 1.0;
        c.run(std::slice::from_ref(&d), 0.0, &mut impulse[..]);
        assert!(impulse.iter().all(|&v| v == 0.0));
        // 2 ms = 96 frames, past this 64-frame block: the same instance brings the echo out in the next one.
        let mut next = vec![0.0f32; 128];
        c.run(std::slice::from_ref(&d), 0.0, &mut next);
        assert_eq!((next[62], next[64]), (0.0, 1.0));
        // A new instance (new id) starts silent.
        let fresh = Fx { id: "other".into(), ..d.clone() };
        let mut buf = vec![0.0f32; 128];
        buf[0] = 1.0;
        c.run(std::slice::from_ref(&d), 0.0, &mut buf);
        let mut z = vec![0.0f32; 128];
        c.run(std::slice::from_ref(&fresh), 0.0, &mut z);
        assert!(z.iter().all(|&v| v == 0.0));
        // Disabled, unknown and video plug-ins pass the signal through.
        let x: Vec<f32> = (0..256).map(|k| (k as f32 * 0.1).sin()).collect();
        let off = Fx { enabled: false, ..d.clone() };
        let other = Fx { plugin: "ofx:somebody".into(), ..d.clone() };
        let video = plugins::new_fx(plugins::find("milan.fx.invert").unwrap(), None);
        let mut y = x.clone();
        Chain::default().run(&[off, other, video], 0.0, &mut y);
        assert_eq!(y, x);
    }
}
