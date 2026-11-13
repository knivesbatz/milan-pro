//! Event velocity (pure): how far into its media an event with a Velocity envelope has played. The envelope is
//! integrated exactly, segment by segment (`Curve::area`), so no table needs caching and preview and render agree.
use crate::project::{Clip, EnvKind, Envelope};

/// VEGAS's velocity range: -100% (full speed backwards) to 1000%.
pub const MIN: f32 = -1.0;
pub const MAX: f32 = 10.0;

/// The event's Velocity envelope, when it has points.
pub fn of(c: &Clip) -> Option<&Envelope> {
    c.envelopes.iter().find(|e| e.kind == EnvKind::Velocity && !e.points.keys.is_empty())
}

/// Seconds of media played from event time 0 to `t` (event-relative) at the envelope's speeds: ∫ v(τ) dτ, the value held
/// before the first point and after the last.
pub fn integral(env: &Envelope, t: f64) -> f64 {
    let k = &env.points.keys;
    let v = |x: f32| x.clamp(MIN, MAX) as f64;
    let (Some(first), Some(last)) = (k.first(), k.last()) else { return t };
    if t <= 0.0 {
        return v(first.v) * t;
    }
    let mut acc = v(first.v) * first.t.clamp(0.0, t);
    for w in k.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        let d = b.t - a.t;
        if b.t <= 0.0 || d <= 0.0 {
            continue;
        }
        if a.t >= t {
            break;
        }
        let (u0, u1) = ((a.t.max(0.0) - a.t) / d, (t.min(b.t) - a.t) / d);
        let (va, vb) = (v(a.v), v(b.v));
        acc += d * (va * (u1 - u0) + (vb - va) * (a.curve.area(u1) - a.curve.area(u0)));
    }
    if t > last.t {
        acc += v(last.v) * (t - last.t.max(0.0));
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Curve, Keyframe, Keyframes};

    fn env(keys: &[(f64, f32, Curve)]) -> Envelope {
        Envelope { kind: EnvKind::Velocity, points: Keyframes { keys: keys.iter().map(|&(t, v, curve)| Keyframe { t, v, curve }).collect() } }
    }

    #[test]
    fn constant_ramp_hold_and_reverse() {
        let e = env(&[(0.0, 2.0, Curve::Linear)]);
        assert_eq!([integral(&e, 0.0), integral(&e, 1.5), integral(&e, 4.0)], [0.0, 3.0, 8.0]);
        // 100% down to 0% over 1 s: half a second of media, then it stops.
        let e = env(&[(0.0, 1.0, Curve::Linear), (1.0, 0.0, Curve::Linear)]);
        assert!((integral(&e, 1.0) - 0.5).abs() < 1e-12 && (integral(&e, 3.0) - 0.5).abs() < 1e-12);
        assert!((integral(&e, 0.5) - 0.375).abs() < 1e-12);
        // A Hold point at 0% (a freeze frame) at 2 s: 100% up to it, then nothing.
        let e = env(&[(0.0, 1.0, Curve::Hold), (2.0, 0.0, Curve::Hold)]);
        assert_eq!([integral(&e, 1.0), integral(&e, 2.0), integral(&e, 5.0)], [1.0, 2.0, 2.0]);
        // Negative speeds run backwards; values past the range are held at it.
        assert_eq!(integral(&env(&[(0.0, -1.0, Curve::Linear)]), 0.5), -0.5);
        let e = env(&[(0.0, -1.0, Curve::Linear), (1.0, 20.0, Curve::Hold)]);
        assert!((integral(&e, 0.5) - 0.875).abs() < 1e-12);
        assert!((integral(&e, 2.0) - (-0.5 + 10.0 * 0.5 + 10.0)).abs() < 1e-9, "{}", integral(&e, 2.0)); // linear -1..10 over 1 s, then 1000%
    }
}
