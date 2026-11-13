//! Windowed-sinc resampler (pure): interleaved stereo read at fractional frame positions, for varispeed audio (an
//! event's Velocity envelope, J / L shuttle). 32 taps, Kaiser window, a polyphase table per anti-alias stretch: read
//! faster than 1x and the kernel widens (cutoff 1 / ratio), so speeding up does not alias.

/// Taps at ratios up to 1x; a kernel `s` times wider has `s` times as many.
const TAPS: usize = 32;
/// Kernel phases between two input frames (the coefficients in between are interpolated).
const PHASES: usize = 1024;
/// Kaiser window beta: about 95 dB of stopband.
const BETA: f64 = 9.5;
/// Anti-alias stretches a table is built for; a ratio uses the next one up. Past 8x the kernel stays 8x wide.
// shortcut: a ratio above 8x (Velocity 1000% on a fast event, 8x shuttle on a fast event) aliases a little; widen the
// kernel further if users hear it.
const STRETCHES: [f64; 7] = [1.0, 1.5, 2.0, 3.0, 4.0, 6.0, 8.0];

struct Table {
    taps: usize,
    /// (PHASES + 1) rows of `taps` coefficients, each row summing to 1.
    k: Vec<f32>,
}

fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term, mut k) = (1.0, 1.0, 1.0);
    while term > 1e-12 * sum {
        term *= (x / (2.0 * k)) * (x / (2.0 * k));
        sum += term;
        k += 1.0;
    }
    sum
}

impl Table {
    fn new(stretch: f64) -> Self {
        let taps = (TAPS as f64 * stretch).ceil() as usize & !1;
        let half = (taps / 2) as f64;
        let norm = bessel_i0(BETA);
        let mut k = Vec::with_capacity((PHASES + 1) * taps);
        for p in 0..=PHASES {
            let frac = p as f64 / PHASES as f64;
            let row: Vec<f64> = (0..taps)
                .map(|j| {
                    let x = j as f64 - (half - 1.0) - frac;
                    let u = x / half;
                    let w = if u.abs() >= 1.0 { 0.0 } else { bessel_i0(BETA * (1.0 - u * u).sqrt()) / norm };
                    let y = std::f64::consts::PI * x / stretch;
                    w * if y.abs() < 1e-12 { 1.0 } else { y.sin() / y }
                })
                .collect();
            let sum: f64 = row.iter().sum();
            k.extend(row.iter().map(|v| (v / sum) as f32));
        }
        Self { taps, k }
    }
}

/// Builds the tables it needs on first use (a few hundred microseconds each).
#[derive(Default)]
pub struct Resampler {
    tables: [Option<Box<Table>>; STRETCHES.len()],
}

impl Resampler {
    /// The stereo frame at fractional frame `pos` of `src` (interleaved stereo; frames outside it are silence), read at
    /// `ratio` input frames per output frame (its sign does not matter).
    pub fn read(&mut self, src: &[f32], pos: f64, ratio: f64) -> [f32; 2] {
        let r = ratio.abs();
        let i = STRETCHES.iter().position(|s| r <= s + 1e-9).unwrap_or(STRETCHES.len() - 1);
        let t = self.tables[i].get_or_insert_with(|| Box::new(Table::new(STRETCHES[i])));
        let n = t.taps;
        let base = pos.floor();
        let ph = (pos - base) * PHASES as f64;
        let p = (ph as usize).min(PHASES - 1);
        let w = (ph - p as f64) as f32;
        let (k0, k1) = (&t.k[p * n..(p + 1) * n], &t.k[(p + 1) * n..(p + 2) * n]);
        let first = base as i64 - (n as i64 / 2 - 1);
        let frames = (src.len() / 2) as i64;
        // Only the taps that land inside `src`.
        let (j0, j1) = ((-first).clamp(0, n as i64) as usize, (frames - first).clamp(0, n as i64) as usize);
        let (mut l, mut rr) = (0.0f32, 0.0f32);
        for j in j0..j1 {
            let c = k0[j] + (k1[j] - k0[j]) * w;
            let f = (first + j as i64) as usize * 2;
            l += src[f] * c;
            rr += src[f + 1] * c;
        }
        [l, rr]
    }

    /// Frames a read reaches on each side of its position, at `ratio`.
    pub fn reach(ratio: f64) -> usize {
        let r = ratio.abs();
        let s = STRETCHES.iter().copied().find(|s| r <= s + 1e-9).unwrap_or(STRETCHES[STRETCHES.len() - 1]);
        (TAPS as f64 * s).ceil() as usize / 2 + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R4b: a 1 kHz sine read at ratio 0.9 is within 90 dB of the ideal one.
    #[test]
    fn snr_above_90_db_at_ratio_0_9() {
        let w = 2.0 * std::f64::consts::PI * 1000.0 / 48_000.0;
        let src: Vec<f32> = (0..48_000).flat_map(|k| [(w * k as f64).sin() as f32; 2]).collect();
        let mut r = Resampler::default();
        let (mut sig, mut err) = (0.0f64, 0.0f64);
        for k in 1000..40_000 {
            let pos = k as f64 * 0.9;
            let want = (w * pos).sin();
            let got = r.read(&src, pos, 0.9);
            assert_eq!(got[0], got[1]);
            sig += want * want;
            err += (got[0] as f64 - want).powi(2);
        }
        let snr = 10.0 * (sig / err).log10();
        assert!(snr > 90.0, "SNR {snr:.1} dB");
    }

    /// Whole frames read back exactly; past the ends is silence; reading at 4x filters out what would alias.
    #[test]
    fn whole_frames_edges_and_anti_aliasing() {
        let src: Vec<f32> = (0..64).flat_map(|k| [k as f32, -(k as f32)]).collect();
        let mut r = Resampler::default();
        let got = r.read(&src, 20.0, 1.0);
        assert!((got[0] - 20.0).abs() < 1e-3 && (got[1] + 20.0).abs() < 1e-3, "{got:?}");
        assert_eq!(r.read(&src, -100.0, 1.0), [0.0, 0.0]);
        // 18 kHz, read at 4x: its alias (a 72 kHz image of a 12 kHz output rate) is stopped.
        let w = 2.0 * std::f64::consts::PI * 18_000.0 / 48_000.0;
        let hi: Vec<f32> = (0..4800).flat_map(|k| [(w * k as f64).sin() as f32; 2]).collect();
        let peak = (100..1000).map(|k| r.read(&hi, k as f64 * 4.0, 4.0)[0].abs()).fold(0.0f32, f32::max);
        assert!(peak < 1e-3, "{peak}");
        assert_eq!(Resampler::reach(0.9), 17);
    }
}
