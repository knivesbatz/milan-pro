//! Keyframe and envelope maths (pure): curve shapes, interpolation and key editing.
use crate::project::{Curve, EnvKind, Envelope, Keyframe, Keyframes, Motion, PanCrop, smooth_fade};

impl Curve {
    /// Progress 0..=1 through a segment, shaped: shape(0) = 0, shape(1) = 1, monotonic.
    pub fn shape(self, x: f64) -> f64 {
        let x = x.clamp(0.0, 1.0);
        match self {
            Curve::Linear => x,
            Curve::Fast => 1.0 - (1.0 - x) * (1.0 - x),
            Curve::Slow => x * x,
            Curve::Smooth => smooth_fade(x),
            // The inverse of Smooth: steep at both ends, flat in the middle.
            Curve::Sharp => (1.0 - 2.0 * x).acos() / std::f64::consts::PI,
            Curve::Hold => {
                if x >= 1.0 {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }

    /// The area under `shape` from 0 to `u` (0..=1), exactly: what a velocity segment adds to the source time.
    pub fn area(self, u: f64) -> f64 {
        use std::f64::consts::PI;
        let u = u.clamp(0.0, 1.0);
        match self {
            Curve::Linear => u * u / 2.0,
            Curve::Fast => u - (1.0 - (1.0 - u).powi(3)) / 3.0,
            Curve::Slow => u * u * u / 3.0,
            Curve::Smooth => u / 2.0 - (PI * u).sin() / (2.0 * PI),
            Curve::Sharp => {
                let y = 1.0 - 2.0 * u;
                ((1.0 - y * y).max(0.0).sqrt() - y * y.clamp(-1.0, 1.0).acos()) / (2.0 * PI)
            }
            Curve::Hold => 0.0,
        }
    }
}

pub trait Lerp: Clone {
    fn lerp(&self, b: &Self, s: f32) -> Self;
}

impl Lerp for f32 {
    fn lerp(&self, b: &Self, s: f32) -> Self {
        self + (b - self) * s
    }
}

impl<const N: usize> Lerp for [f32; N] {
    fn lerp(&self, b: &Self, s: f32) -> Self {
        std::array::from_fn(|i| self[i].lerp(&b[i], s))
    }
}

impl Lerp for PanCrop {
    fn lerp(&self, b: &Self, s: f32) -> Self {
        let [cx, cy, w, h, rot] = [self.cx, self.cy, self.w, self.h, self.rot].lerp(&[b.cx, b.cy, b.w, b.h, b.rot], s);
        PanCrop { cx, cy, w, h, rot }
    }
}

impl Lerp for Motion {
    fn lerp(&self, b: &Self, s: f32) -> Self {
        let [x, y, w, h, rot] = [self.x, self.y, self.w, self.h, self.rot].lerp(&[b.x, b.y, b.w, b.h, b.rot], s);
        Motion { x, y, w, h, rot }
    }
}

impl<T: Lerp + Default> Keyframes<T> {
    /// Value at `t`: the default with no keys, held before the first and after the last key.
    pub fn at(&self, t: f64) -> T {
        let k = &self.keys;
        let (Some(first), Some(last)) = (k.first(), k.last()) else { return T::default() };
        // `!(t > ..)` also sends a NaN time (0-length event, rate 0) to the first key instead of panicking.
        if !(t > first.t) {
            return first.v.clone();
        }
        if t >= last.t {
            return last.v.clone();
        }
        let i = k.partition_point(|key| key.t <= t); // 1..len: k[i - 1].t <= t < k[i].t
        let (a, b) = (&k[i - 1], &k[i]);
        a.v.lerp(&b.v, a.curve.shape((t - a.t) / (b.t - a.t)) as f32)
    }
}

impl<T> Keyframes<T> {
    /// Set a key at `t`: replaces a key within half a frame, otherwise inserts in time order.
    /// Returns the key's index.
    pub fn set_key(&mut self, t: f64, v: T, curve: Curve, fps: f64) -> usize {
        let tol = 0.5 / fps.max(1.0);
        if let Some(i) = self.keys.iter().position(|k| (k.t - t).abs() < tol) {
            self.keys[i] = Keyframe { t: self.keys[i].t, v, curve };
            return i;
        }
        let i = self.keys.partition_point(|k| k.t < t);
        self.keys.insert(i, Keyframe { t, v, curve });
        i
    }

    pub fn remove_key(&mut self, i: usize) {
        if i < self.keys.len() {
            self.keys.remove(i);
        }
    }

    /// Move key `i` to time `t`, keeping the list sorted. Returns its new index (`i` if out of range).
    pub fn move_key(&mut self, i: usize, t: f64) -> usize {
        if i >= self.keys.len() {
            return i;
        }
        let mut key = self.keys.remove(i);
        key.t = t;
        let j = self.keys.partition_point(|k| k.t <= t);
        self.keys.insert(j, key);
        j
    }

    pub fn times(&self) -> Vec<f64> {
        self.keys.iter().map(|k| k.t).collect()
    }

    /// Add `dt` to every key time.
    pub fn shift(&mut self, dt: f64) {
        for k in &mut self.keys {
            k.t += dt;
        }
    }
}

impl EnvKind {
    /// The value of an envelope with no points.
    pub fn neutral(self) -> f32 {
        match self {
            EnvKind::Volume | EnvKind::Pan | EnvKind::FadeToColor | EnvKind::TransitionProgress => 0.0,
            EnvKind::Mute | EnvKind::CompositeLevel | EnvKind::Opacity | EnvKind::Velocity | EnvKind::Unknown => 1.0,
        }
    }
}

impl Envelope {
    pub fn at(&self, t: f64) -> f32 {
        if self.points.keys.is_empty() { self.kind.neutral() } else { self.points.at(t) }
    }
}

/// Envelope values at the start and end of a block, for a per-sample linear ramp.
pub fn ramp(env: &Envelope, t0: f64, t1: f64) -> (f32, f32) {
    (env.at(t0), env.at(t1))
}

/// dB to linear gain; -120 dB and below is silence.
pub fn db_to_gain(db: f32) -> f32 {
    if db <= -120.0 { 0.0 } else { 10f32.powf(db / 20.0) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Curve; 6] = [Curve::Linear, Curve::Fast, Curve::Slow, Curve::Smooth, Curve::Sharp, Curve::Hold];

    fn kf(keys: &[(f64, f32, Curve)]) -> Keyframes<f32> {
        Keyframes { keys: keys.iter().map(|&(t, v, curve)| Keyframe { t, v, curve }).collect() }
    }

    #[test]
    fn curves_run_from_0_to_1_monotonically() {
        for c in ALL {
            assert!(c.shape(0.0).abs() < 1e-12, "{c:?}");
            assert!((c.shape(1.0) - 1.0).abs() < 1e-12, "{c:?}");
            let ys: Vec<f64> = (0..=100).map(|i| c.shape(i as f64 / 100.0)).collect();
            assert!(ys.windows(2).all(|w| w[1] >= w[0]), "{c:?} not monotonic");
        }
        assert!(Curve::Fast.shape(0.25) > 0.25 && Curve::Slow.shape(0.25) < 0.25);
        assert!(Curve::Sharp.shape(0.25) > 0.25 && Curve::Smooth.shape(0.25) < 0.25);
    }

    /// `area` is the integral of `shape`: checked against a fine midpoint sum.
    #[test]
    fn curve_area_integrates_the_shape() {
        for c in ALL {
            for u in [0.0, 0.1, 0.25, 0.5, 0.9, 1.0] {
                let n = 20_000;
                let sum: f64 = (0..n).map(|i| c.shape((i as f64 + 0.5) / n as f64 * u)).sum::<f64>() * u / n as f64;
                assert!((c.area(u) - sum).abs() < 1e-6, "{c:?} {u}: {} vs {sum}", c.area(u));
            }
        }
    }

    #[test]
    fn hold_is_a_step() {
        assert_eq!(Curve::Hold.shape(0.999), 0.0);
        let k = kf(&[(0.0, 1.0, Curve::Hold), (2.0, 5.0, Curve::Linear)]);
        assert_eq!(k.at(1.99), 1.0);
        assert_eq!(k.at(2.0), 5.0);
    }

    #[test]
    fn interpolation_default_and_hold_outside() {
        assert_eq!(Keyframes::<f32>::default().at(3.0), 0.0);
        assert_eq!(Keyframes::<PanCrop>::default().at(3.0), PanCrop::default());
        let k = kf(&[(1.0, 2.0, Curve::Linear), (3.0, 6.0, Curve::Smooth), (5.0, 0.0, Curve::Linear)]);
        assert_eq!((k.at(0.0), k.at(1.0), k.at(9.0), k.at(f64::NAN)), (2.0, 2.0, 0.0, 2.0));
        assert_eq!(k.at(2.0), 4.0); // linear midpoint
        assert!((k.at(4.0) - 3.0).abs() < 1e-6); // smooth midpoint
        let m = Keyframes { keys: vec![Keyframe { t: 0.0, v: Motion::default(), curve: Curve::Linear }, Keyframe { t: 1.0, v: Motion { x: 1.0, ..Motion::default() }, curve: Curve::Linear }] };
        assert_eq!(m.at(0.5).x, 0.5);
    }

    #[test]
    fn envelopes_and_ramp() {
        let empty = Envelope { kind: EnvKind::Mute, points: Keyframes::default() };
        assert_eq!(empty.at(1.0), 1.0);
        let e = Envelope { kind: EnvKind::Volume, points: kf(&[(0.0, -12.0, Curve::Linear), (1.0, 0.0, Curve::Linear)]) };
        assert_eq!(ramp(&e, 0.0, 1.0), (-12.0, 0.0));
        assert_eq!(ramp(&e, -1.0, 0.5), (-12.0, -6.0));
    }

    #[test]
    fn set_key_replaces_within_half_a_frame() {
        let mut k = kf(&[(0.0, 1.0, Curve::Linear), (1.0, 2.0, Curve::Linear)]);
        assert_eq!(k.set_key(1.01, 9.0, Curve::Hold, 30.0), 1); // 0.01 s < half a frame at 30 fps
        assert_eq!((k.keys.len(), k.keys[1].t, k.keys[1].v, k.keys[1].curve), (2, 1.0, 9.0, Curve::Hold));
        assert_eq!(k.set_key(0.5, 3.0, Curve::Linear, 30.0), 1);
        assert_eq!(k.set_key(-1.0, 0.0, Curve::Linear, 30.0), 0);
        assert_eq!(k.times(), [-1.0, 0.0, 0.5, 1.0]);
    }

    #[test]
    fn move_remove_shift_keep_order() {
        let mut k = kf(&[(0.0, 0.0, Curve::Linear), (1.0, 1.0, Curve::Linear), (2.0, 2.0, Curve::Linear)]);
        assert_eq!(k.move_key(0, 1.5), 1);
        assert_eq!(k.times(), [1.0, 1.5, 2.0]);
        assert_eq!(k.keys[1].v, 0.0);
        assert_eq!(k.move_key(2, -1.0), 0);
        assert_eq!(k.times(), [-1.0, 1.0, 1.5]);
        assert_eq!(k.move_key(9, 0.0), 9); // out of range: ignored
        k.remove_key(1);
        k.remove_key(7); // out of range: ignored
        k.shift(0.5);
        assert_eq!(k.times(), [-0.5, 2.0]);
    }

    #[test]
    fn decibels() {
        assert!((db_to_gain(-6.02) - 0.5).abs() < 1e-3);
        assert_eq!(db_to_gain(0.0), 1.0);
        assert_eq!(db_to_gain(-120.0), 0.0);
    }
}
