//! R16c: loudness metering after ITU-R BS.1770-4 and EBU R 128: K-weighting, momentary (400 ms) and short-term (3 s)
//! loudness every 100 ms (EBU Tech 3341), integrated loudness over 400 ms gating blocks with 75 % overlap, the absolute
//! (-70 LUFS) and relative (-10 LU) gates, loudness range (EBU Tech 3342) and true peak by 4x oversampling with the
//! BS.1770-4 Annex 2 filter. Stereo, both channels weighted 1. Pure: the Loudness Meters window runs it on its own
//! thread from the master output's tap, and a loudness log runs it on a render's mix.
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Options › Loudness Meters › EBU R 128 Mode / ATSC A 85 Mode: the integrated target and the true-peak maximum.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    #[default]
    Ebu,
    Atsc,
}

impl Mode {
    pub fn target(self) -> f64 {
        [-23.0, -24.0][self as usize]
    }

    pub fn max_true_peak(self) -> f64 {
        [-1.0, -2.0][self as usize]
    }

    pub fn name(self) -> &'static str {
        ["EBU R 128", "ATSC A/85"][self as usize]
    }
}

/// BS.1770-4 Annex 2: the 48-tap, 4-phase FIR interpolating filter (one row per phase, tap j weighs x[n - j]).
const TP: [[f64; 12]; 4] = [
    [0.0017089843750, 0.0109863281250, -0.0196533203125, 0.0332031250000, -0.0594482421875, 0.1373291015625, 0.9721679687500, -0.1022949218750, 0.0476074218750, -0.0266113281250, 0.0148925781250, -0.0083007812500],
    [-0.0291748046875, 0.0292968750000, -0.0517578125000, 0.0891113281250, -0.1665039062500, 0.4650878906250, 0.7797851562500, -0.2003173828125, 0.1015625000000, -0.0582275390625, 0.0330810546875, -0.0189208984375],
    [-0.0189208984375, 0.0330810546875, -0.0582275390625, 0.1015625000000, -0.2003173828125, 0.7797851562500, 0.4650878906250, -0.1665039062500, 0.0891113281250, -0.0517578125000, 0.0292968750000, -0.0291748046875],
    [-0.0083007812500, 0.0148925781250, -0.0266113281250, 0.0476074218750, -0.1022949218750, 0.9721679687500, 0.1373291015625, -0.0594482421875, 0.0332031250000, -0.0196533203125, 0.0109863281250, 0.0017089843750],
];

/// Sub-blocks (100 ms) in a momentary window, a gating block (400 ms) and a short-term window (3 s).
const M_SUBS: usize = 4;
const S_SUBS: usize = 30;
const ABS_GATE: f64 = -70.0;

/// A biquad, transposed direct form II.
#[derive(Clone, Copy, Debug, Default)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    z: [f64; 2],
}

impl Biquad {
    fn run(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

/// The K-weighting filter at `rate`: the high shelf and the RLB high-pass, from their analogue prototypes (at 48 kHz
/// these are BS.1770-4's tabled coefficients).
fn k_weighting(rate: u32) -> [Biquad; 2] {
    let r = rate.max(1) as f64;
    let (f0, g, q) = (1681.974450955533, 3.999843853973347, 0.7071752369554196);
    let k = (std::f64::consts::PI * f0 / r).tan();
    let (vh, a0) = (10f64.powf(g / 20.0), 1.0 + k / q + k * k);
    let vb = vh.powf(0.4996667741545416);
    let shelf = Biquad {
        b: [(vh + vb * k / q + k * k) / a0, 2.0 * (k * k - vh) / a0, (vh - vb * k / q + k * k) / a0],
        a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
        z: [0.0; 2],
    };
    let (f0, q) = (38.13547087602444, 0.5003270373238773);
    let k = (std::f64::consts::PI * f0 / r).tan();
    let a0 = 1.0 + k / q + k * k;
    let rlb = Biquad { b: [1.0, -2.0, 1.0], a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0], z: [0.0; 2] };
    [shelf, rlb]
}

/// One channel's true-peak detector: the last 12 samples and the four interpolated points after each.
#[derive(Clone, Copy, Debug, Default)]
struct TruePeak {
    hist: [f64; 12],
    at: usize,
}

impl TruePeak {
    /// The largest |value| of the four points the filter interpolates on taking `x`.
    fn run(&mut self, x: f64) -> f64 {
        self.at = (self.at + 11) % 12;
        self.hist[self.at] = x;
        let mut m = 0.0f64;
        for ph in &TP {
            let s: f64 = (0..12).map(|j| ph[j] * self.hist[(self.at + j) % 12]).sum();
            m = m.max(s.abs());
        }
        m
    }
}

/// True Peak Blocking Filter: a first-order DC blocker (10 Hz) ahead of a second true-peak detector.
#[derive(Clone, Copy, Debug, Default)]
struct DcBlock {
    r: f64,
    x1: f64,
    y1: f64,
}

impl DcBlock {
    fn run(&mut self, x: f64) -> f64 {
        let y = x - self.x1 + self.r * self.y1;
        (self.x1, self.y1) = (x, y);
        y
    }
}

/// LUFS of a channel-summed mean square (-inf for silence).
pub fn lufs(energy: f64) -> f64 {
    -0.691 + 10.0 * energy.log10()
}

fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len().max(1) as f64
}

/// The loudness of the blocks above the absolute gate and `rel` LU under their own loudness, and those blocks' LUFS.
fn gated(energies: &[f64], rel: f64) -> (f64, Vec<f64>) {
    let abs: Vec<f64> = energies.iter().copied().filter(|&e| lufs(e) > ABS_GATE).collect();
    if abs.is_empty() {
        return (f64::NEG_INFINITY, Vec::new());
    }
    let gate = lufs(mean(&abs)) - rel;
    let kept: Vec<f64> = abs.into_iter().filter(|&e| lufs(e) > gate).collect();
    (lufs(mean(&kept)), kept.into_iter().map(lufs).collect())
}

/// What the meter reads: LUFS (momentary, short-term, integrated, and the highest and lowest momentary and short-term
/// values of full windows), LU (range) and dBTP (true peak per channel), each since the last reset. -inf: nothing yet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Readings {
    pub momentary: f64,
    pub short_term: f64,
    pub integrated: f64,
    pub range: f64,
    pub true_peak: [f64; 2],
    pub max_momentary: f64,
    pub max_short: f64,
    pub min_momentary: f64,
    pub min_short: f64,
}

impl Default for Readings {
    fn default() -> Self {
        let n = f64::NEG_INFINITY;
        Self { momentary: n, short_term: n, integrated: n, range: 0.0, true_peak: [n; 2], max_momentary: n, max_short: n, min_momentary: f64::INFINITY, min_short: f64::INFINITY }
    }
}

/// A stereo loudness meter at one sample rate.
#[derive(Clone, Debug)]
pub struct Meter {
    k: [[Biquad; 2]; 2],
    /// Frames per 100 ms sub-block; the current one's channel-summed square sum and frame count.
    step: usize,
    acc: f64,
    n: usize,
    /// The last 30 sub-blocks' mean squares, and how many there have been.
    subs: VecDeque<f64>,
    count: usize,
    /// Every 400 ms gating block's and every full 3 s window's mean square, one per sub-block.
    blocks: Vec<f64>,
    shorts: Vec<f64>,
    tp: [TruePeak; 2],
    blocking: Option<[(DcBlock, TruePeak); 2]>,
    /// The true peak per channel since the reset, and over both since `take_window_peak` (linear).
    peak: [f64; 2],
    window_peak: f64,
    max: [f64; 2],
    min: [f64; 2],
}

impl Meter {
    /// A meter at `rate`; `blocking`: Options › Loudness Meters › True Peak Blocking Filter.
    pub fn new(rate: u32, blocking: bool) -> Self {
        let k = k_weighting(rate);
        let dc = DcBlock { r: 1.0 - 2.0 * std::f64::consts::PI * 10.0 / rate.max(1) as f64, ..Default::default() };
        Self {
            k: [k, k],
            step: (rate as usize).div_ceil(10).max(1),
            acc: 0.0,
            n: 0,
            subs: VecDeque::with_capacity(S_SUBS),
            count: 0,
            blocks: Vec::new(),
            shorts: Vec::new(),
            tp: Default::default(),
            blocking: blocking.then(|| [(dc, TruePeak::default()); 2]),
            peak: [0.0; 2],
            window_peak: 0.0,
            max: [f64::NEG_INFINITY; 2],
            min: [f64::INFINITY; 2],
        }
    }

    /// Take interleaved stereo frames.
    pub fn feed(&mut self, stereo: &[f32]) {
        for f in stereo.chunks_exact(2) {
            for c in 0..2 {
                // A NaN from a misbehaving plug-in must not poison everything measured after it.
                let x = if f[c].is_finite() { f[c] as f64 } else { 0.0 };
                let [a, b] = &mut self.k[c];
                let y = b.run(a.run(x));
                self.acc += y * y;
                let mut p = self.tp[c].run(x);
                if let Some(bl) = &mut self.blocking {
                    // The larger of the filtered and the unfiltered signal's peaks (VEGAS's wording).
                    let (dc, tp) = &mut bl[c];
                    p = p.max(tp.run(dc.run(x)));
                }
                self.peak[c] = self.peak[c].max(p);
                self.window_peak = self.window_peak.max(p);
            }
            self.n += 1;
            if self.n == self.step {
                self.sub_block();
            }
        }
    }

    fn sub_block(&mut self) {
        if self.subs.len() == S_SUBS {
            self.subs.pop_front();
        }
        self.subs.push_back(self.acc / self.step as f64);
        (self.acc, self.n) = (0.0, 0);
        self.count += 1;
        for (k, (subs, list)) in [(M_SUBS, &mut self.blocks), (S_SUBS, &mut self.shorts)].into_iter().enumerate() {
            if self.count >= subs {
                let e = self.subs.iter().rev().take(subs).sum::<f64>() / subs as f64;
                list.push(e);
                self.max[k] = self.max[k].max(lufs(e));
                self.min[k] = self.min[k].min(lufs(e));
            }
        }
    }

    /// Loudness of the last `subs` sub-blocks; a window not yet full counts silence before the start.
    fn window(&self, subs: usize) -> f64 {
        lufs(self.subs.iter().rev().take(subs).sum::<f64>() / subs as f64)
    }

    /// Momentary loudness (400 ms), updated every 100 ms.
    pub fn momentary(&self) -> f64 {
        self.window(M_SUBS)
    }

    /// Short-term loudness (3 s), updated every 100 ms.
    pub fn short_term(&self) -> f64 {
        self.window(S_SUBS)
    }

    /// Integrated loudness: the 400 ms blocks over -70 LUFS and over 10 LU under their own loudness.
    pub fn integrated(&self) -> f64 {
        gated(&self.blocks, 10.0).0
    }

    /// Loudness range (EBU Tech 3342): the 10th to 95th percentile of the short-term values over -70 LUFS and over 20
    /// LU under their own loudness.
    pub fn range(&self) -> f64 {
        let mut v = gated(&self.shorts, 20.0).1;
        if v.len() < 2 {
            return 0.0;
        }
        v.sort_by(f64::total_cmp);
        let at = |p: f64| v[((v.len() - 1) as f64 * p).round() as usize];
        at(0.95) - at(0.10)
    }

    /// Options › Loudness Meters › Reset Clip: the true peaks start again.
    pub fn reset_peaks(&mut self) {
        (self.peak, self.window_peak) = ([0.0; 2], 0.0);
    }

    /// True peak per channel since the reset, dBTP.
    pub fn true_peak(&self) -> [f64; 2] {
        self.peak.map(|p| 20.0 * p.log10())
    }

    /// The true peak of both channels since the last call, dBTP.
    pub fn take_window_peak(&mut self) -> f64 {
        20.0 * std::mem::take(&mut self.window_peak).log10()
    }

    pub fn readings(&self) -> Readings {
        Readings {
            momentary: self.momentary(),
            short_term: self.short_term(),
            integrated: self.integrated(),
            range: self.range(),
            true_peak: self.true_peak(),
            max_momentary: self.max[0],
            max_short: self.max[1],
            min_momentary: self.min[0],
            min_short: self.min[1],
        }
    }
}

/// One second of a loudness log: its end (seconds from the range start) and its momentary and short-term loudness at
/// that end, and the true peak within it.
#[derive(Clone, Copy, Debug)]
struct Row {
    end: f64,
    momentary: f64,
    short: f64,
    peak: f64,
}

/// A loudness log in the making (Tools › Generate Loudness Log, Render Options › Save loudness log next to media
/// file): the meter over the mix, and a row per second.
pub struct Log {
    meter: Meter,
    rate: u32,
    /// Frames into the current second.
    frames: usize,
    rows: Vec<Row>,
}

impl Log {
    pub fn new(rate: u32) -> Self {
        Self { meter: Meter::new(rate, false), rate, frames: 0, rows: Vec::new() }
    }

    pub fn feed(&mut self, mut stereo: &[f32]) {
        let sec = self.rate.max(1) as usize;
        while !stereo.is_empty() {
            let n = (sec - self.frames).min(stereo.len() / 2);
            self.meter.feed(&stereo[..2 * n]);
            (stereo, self.frames) = (&stereo[2 * n..], self.frames + n);
            if self.frames == sec {
                self.row();
            }
            if n == 0 {
                break; // an odd sample
            }
        }
    }

    fn row(&mut self) {
        let end = self.rows.len() as f64 + self.frames as f64 / self.rate.max(1) as f64;
        let m = &mut self.meter;
        self.rows.push(Row { end, momentary: m.momentary(), short: m.short_term(), peak: m.take_window_peak() });
        self.frames = 0;
    }

    pub fn readings(&self) -> Readings {
        self.meter.readings()
    }

    /// The log's text: what was measured (`source`, `format`), the mode, the timeline range (seconds), the summary and a
    /// row per second (the last one shorter when the range is).
    pub fn text(&mut self, source: &str, format: &str, mode: Mode, (a, b): (f64, f64)) -> String {
        if self.frames > 0 {
            self.row();
        }
        let r = self.readings();
        let v = |x: f64| if x.is_finite() { format!("{x:.1}") } else { "-Inf.".into() };
        let hms = |t: f64| {
            let s = t.max(0.0);
            format!("{}:{:02}:{:06.3}", (s / 3600.0) as u64, (s / 60.0) as u64 % 60, s % 60.0)
        };
        let mut out = format!(
            "Milan Pro {} loudness log\r\nFile:     {source}\r\nFormat:   {format}\r\nMode:     {} (target {:.1} LUFS, maximum true peak {:.1} dBTP)\r\nRange:    {} to {}\r\n\r\n",
            env!("CARGO_PKG_VERSION"),
            mode.name(),
            mode.target(),
            mode.max_true_peak(),
            hms(a),
            hms(b),
        );
        let tp = r.true_peak[0].max(r.true_peak[1]);
        out += &format!("Integrated loudness:  {} LUFS\r\n", v(r.integrated));
        out += &format!("Loudness range:       {} LU\r\n", v(r.range));
        out += &format!("Maximum momentary:    {} LUFS\r\n", v(r.max_momentary));
        out += &format!("Maximum short-term:   {} LUFS\r\n", v(r.max_short));
        out += &format!("Maximum true peak:    {} dBTP{}\r\n", v(tp), if tp > mode.max_true_peak() { " (over the maximum)" } else { "" });
        out += "\r\nTime           Momentary  Short-term  True peak\r\n               (LUFS)     (LUFS)      (dBTP)\r\n";
        for row in &self.rows {
            out += &format!("{:<14} {:>9}  {:>10}  {:>9}\r\n", hms(a + row.end), v(row.momentary), v(row.short), v(row.peak));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: u32 = 48_000;

    /// `secs` of a stereo sine at `hz`, peak `dbfs`, starting at phase `phase` (radians), both channels alike.
    fn sine(hz: f64, dbfs: f64, secs: f64, phase: f64) -> Vec<f32> {
        let a = 10f64.powf(dbfs / 20.0);
        (0..(secs * R as f64).round() as usize).flat_map(|i| {
            let v = (a * (2.0 * std::f64::consts::PI * hz * i as f64 / R as f64 + phase).sin()) as f32;
            [v, v]
        }).collect()
    }

    fn run(parts: &[(f64, f64)]) -> Meter {
        let mut m = Meter::new(R, false);
        for &(db, secs) in parts {
            m.feed(&sine(1000.0, db, secs, 0.0));
        }
        m
    }

    fn near(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn k_weighting_matches_bs1770_at_48k() {
        let [s, r] = k_weighting(48_000);
        let want_s = ([1.53512485958697, -2.69169618940638, 1.19839281085285], [-1.69065929318241, 0.73248077421585]);
        let want_r = ([1.0, -2.0, 1.0], [-1.99004745483398, 0.99007225036621]);
        for (got, want) in [(s, want_s), (r, want_r)] {
            assert!(got.b.iter().zip(want.0).all(|(g, w)| near(*g, w, 1e-8)), "{got:?}");
            assert!(got.a.iter().zip(want.1).all(|(g, w)| near(*g, w, 1e-8)), "{got:?}");
        }
    }

    /// EBU Tech 3341 cases 1 and 2: a stereo 1 kHz sine at -23 / -33 dBFS reads that loudness, momentary, short-term
    /// and integrated, within 0.1.
    #[test]
    fn tech3341_sines_read_their_level() {
        for db in [-23.0, -33.0] {
            let m = run(&[(db, 20.0)]);
            let r = m.readings();
            for v in [r.momentary, r.short_term, r.integrated, r.max_momentary, r.max_short] {
                assert!(near(v, db, 0.1), "{db}: {r:?}");
            }
            assert!(r.range < 0.1, "{r:?}");
        }
    }

    /// EBU Tech 3341 cases 3, 4 and 5: the absolute and relative gates leave -23.0 ± 0.1 LUFS integrated.
    #[test]
    fn tech3341_gating_cases() {
        let cases: [&[(f64, f64)]; 3] = [
            &[(-36.0, 10.0), (-23.0, 60.0), (-36.0, 10.0)],
            &[(-72.0, 10.0), (-36.0, 10.0), (-23.0, 60.0), (-36.0, 10.0), (-72.0, 10.0)],
            &[(-26.0, 20.0), (-20.0, 20.1), (-26.0, 20.0)],
        ];
        for c in cases {
            let i = run(c).integrated();
            assert!(near(i, -23.0, 0.1), "{c:?}: {i}");
        }
        // Without the relative gate case 3 would read well under -23; silence alone reads nothing.
        assert!(lufs(mean(&run(cases[0]).blocks)) < -24.0);
        assert_eq!(run(&[(-100.0, 5.0)]).integrated(), f64::NEG_INFINITY);
    }

    /// EBU Tech 3342 cases 1-3: two 20 s stretches 10, 5 and 20 dB apart read that loudness range ± 1 LU.
    #[test]
    fn tech3342_loudness_range() {
        for (a, b, lra) in [(-20.0, -30.0, 10.0), (-20.0, -15.0, 5.0), (-40.0, -20.0, 20.0)] {
            let got = run(&[(a, 20.0), (b, 20.0)]).range();
            assert!(near(got, lra, 1.0), "{a}/{b}: {got}");
        }
    }

    /// True peak: a sine whose peaks fall between samples reads its real peak, not its sample peak.
    #[test]
    fn true_peak_finds_inter_sample_peaks() {
        // fs/4 at 45°: every sample is 3 dB under the peak (EBU Tech 3341 allows +0.2 / -0.4 dB here).
        let mut m = Meter::new(R, false);
        m.feed(&sine(12_000.0, -6.0, 1.0, std::f64::consts::FRAC_PI_4));
        let sample_peak = 20.0 * sine(12_000.0, -6.0, 1.0, std::f64::consts::FRAC_PI_4).iter().fold(0.0f32, |a, v| a.max(v.abs())).log10() as f64;
        assert!(near(sample_peak, -9.01, 0.05), "{sample_peak}");
        let tp = m.true_peak();
        assert!(tp.iter().all(|&p| (-6.4..=-5.8).contains(&p)), "{tp:?}");
        // An 8 kHz sine with its peaks half way between samples: within 0.2 dB.
        let mut m = Meter::new(R, false);
        m.feed(&sine(8_000.0, -3.0, 1.0, std::f64::consts::FRAC_PI_6));
        assert!(m.true_peak().iter().all(|&p| near(p, -3.0, 0.2)), "{:?}", m.true_peak());
        // 997 Hz full scale: 0 dBTP.
        let mut m = Meter::new(R, false);
        m.feed(&sine(997.0, 0.0, 1.0, 0.3));
        assert!(m.true_peak().iter().all(|&p| near(p, 0.0, 0.2)), "{:?}", m.true_peak());
    }

    /// The blocking filter takes the larger peak: never under the plain reading.
    #[test]
    fn blocking_filter_never_lowers_the_peak() {
        let mut x = sine(1000.0, -10.0, 1.0, 0.0);
        x.iter_mut().for_each(|v| *v += 0.2); // a DC offset
        let (mut plain, mut blocked) = (Meter::new(R, false), Meter::new(R, true));
        plain.feed(&x);
        blocked.feed(&x);
        assert!(blocked.true_peak()[0] >= plain.true_peak()[0]);
    }

    /// Other rates measure the same (44.1 kHz), and a log has its summary and a row per second.
    #[test]
    fn other_rates_and_the_log() {
        let a = 10f64.powf(-23.0 / 20.0);
        let x: Vec<f32> = (0..44_100 * 10).flat_map(|i| {
            let v = (a * (2.0 * std::f64::consts::PI * 1000.0 * i as f64 / 44_100.0).sin()) as f32;
            [v, v]
        }).collect();
        let mut m = Meter::new(44_100, false);
        m.feed(&x);
        assert!(near(m.integrated(), -23.0, 0.1), "{}", m.integrated());
        let mut log = Log::new(R);
        for piece in sine(1000.0, -23.0, 4.5, 0.0).chunks(2 * 1000) {
            log.feed(piece);
        }
        let text = log.text("p.mnle", "48,000 Hz stereo", Mode::Atsc, (10.0, 14.5));
        assert!(text.contains("Integrated loudness:  -23.0 LUFS") && text.contains("ATSC A/85 (target -24.0 LUFS"), "{text}");
        assert!(text.contains("0:00:11.000") && text.contains("0:00:14.500"), "{text}");
        assert_eq!(text.lines().filter(|l| l.starts_with("0:00:1")).count(), 5, "{text}");
    }
}
