//! R10a: surround arithmetic, pure. The channel order is FFmpeg's: FL FR | FC LFE | BL BR | SL SR (`BusMode`). The
//! mixer's tracks are stereo (as VEGAS's are); in a 5.1 / 7.1 project each track's stereo output is placed in the
//! room by its surround pan (`pan_matrix`), busses and the master carry the layout's channels, and stereo monitoring
//! and stereo renders fold the master down with ITU-R BS.775 (`downmix`: L = FL + 0.707 C + 0.707 Ls, LFE dropped),
//! the same matrix FFmpeg's `-ac 2` uses for float samples.
use crate::project::{BusMode, PanType, SurroundPan};

/// Channel indices.
pub const FL: usize = 0;
pub const FR: usize = 1;
pub const FC: usize = 2;
pub const LFE: usize = 3;
pub const BL: usize = 4;
pub const BR: usize = 5;
pub const SL: usize = 6;
pub const SR: usize = 7;

/// At most this many channels anywhere (7.1).
pub const MAX: usize = 8;

/// BS.775's surround and centre level, -3 dB.
pub const SURROUND_LEVEL: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// The stereo pair (L, R) of one `ch`-channel frame: itself for stereo, the BS.775 fold-down of 5.1 / 7.1 (side and
/// back channels both at -3 dB, as FFmpeg's default matrix has them), the sum for anything else.
#[inline]
pub fn downmix_frame(f: &[f32]) -> [f32; 2] {
    match f.len() {
        0 => [0.0; 2],
        1 => [f[0], f[0]],
        2 => [f[0], f[1]],
        n => {
            let c = if n > FC { f[FC] * SURROUND_LEVEL } else { 0.0 };
            let (mut l, mut r) = (f[FL] + c, f[FR] + c);
            if n > BR {
                l += f[BL] * SURROUND_LEVEL;
                r += f[BR] * SURROUND_LEVEL;
            }
            if n > SR {
                l += f[SL] * SURROUND_LEVEL;
                r += f[SR] * SURROUND_LEVEL;
            }
            [l, r]
        }
    }
}

/// Interleaved `ch`-channel `buf` as interleaved stereo in `out` (replaced).
pub fn downmix(buf: &[f32], ch: usize, out: &mut Vec<f32>) {
    out.clear();
    out.reserve(buf.len() / ch.max(1) * 2);
    for f in buf.chunks_exact(ch.max(1)) {
        out.extend_from_slice(&downmix_frame(f));
    }
}

/// Interleaved `from` channels as interleaved `to` channels in `out` (replaced): 1 is the mean of the stereo fold,
/// stereo into a surround layout fills the front pair, 5.1 into 7.1 leaves the sides silent, 7.1 into 5.1 folds the
/// sides into the backs at unity (FFmpeg's matrix).
pub fn convert(buf: &[f32], from: usize, to: usize, out: &mut Vec<f32>) {
    let (from, to) = (from.max(1), to.max(1));
    out.clear();
    if from == to {
        out.extend_from_slice(buf);
        return;
    }
    out.reserve(buf.len() / from * to);
    for f in buf.chunks_exact(from) {
        match to {
            1 => {
                let [l, r] = downmix_frame(f);
                out.push((l + r) * 0.5);
            }
            2 => out.extend_from_slice(&downmix_frame(f)),
            _ => {
                let mut o = [0.0f32; MAX];
                if from <= 2 {
                    let [l, r] = downmix_frame(f);
                    (o[FL], o[FR]) = (l, r);
                } else {
                    for (c, v) in f.iter().enumerate().take(to.min(from)) {
                        o[c] = *v;
                    }
                    if from > SR && to <= SL {
                        o[BL] += f[SL];
                        o[BR] += f[SR];
                    }
                }
                out.extend_from_slice(&o[..to.min(MAX)]);
            }
        }
    }
}

/// A mono signal between two speakers at `p` (-1..1, by the pan type's law): (the near one's gain, the far one's).
/// Balance (0 dB) and Add Channels keep the near one at unity; Balance -3 / -6 dB start both at the centre level;
/// Constant Power and Film keep l² + r² at 1.
fn one_d(pt: PanType, p: f32) -> (f32, f32) {
    let a = p.abs().min(1.0);
    match pt {
        PanType::Balance0 | PanType::AddChannels => (1.0, 1.0 - a),
        PanType::Balance3 | PanType::Balance6 => {
            let c = if pt == PanType::Balance3 { 10f32.powf(-3.0 / 20.0) } else { 10f32.powf(-6.0 / 20.0) };
            (c + (1.0 - c) * a, c * (1.0 - a))
        }
        PanType::ConstantPower | PanType::Film => {
            let th = (1.0 - a) * std::f32::consts::FRAC_PI_4;
            (th.cos(), th.sin())
        }
    }
}

/// The gains of a stereo track into the `ch` channels of the room: `[from L, from R]`, each `MAX` wide (channels past
/// `ch` are 0). `x_trim` is the Pan slider and envelope, added to the pan point's x. Left speakers take the left side
/// of the pan type's stereo matrix at x and right speakers the right side; the front, side and back rows take the 1-D
/// law along y; the centre speaker takes the mono sum, fading out as the point leaves the middle. Speakers switched
/// off take nothing; with `lfe` the whole track goes to the LFE channel alone (the mono sum at 0 dB). Smoothness pulls
/// the point towards the middle of the room (at 1 nothing pans).
pub fn pan_matrix(pt: PanType, sp: &SurroundPan, x_trim: f32, ch: usize) -> [[f32; MAX]; 2] {
    let mut g = [[0.0f32; MAX]; 2];
    let ch = ch.min(MAX);
    if ch <= 2 {
        let m = pt.matrix((sp.x + x_trim).clamp(-1.0, 1.0));
        (g[0][FL], g[1][FL], g[0][FR], g[1][FR]) = (m[0][0], m[0][1], m[1][0], m[1][1]);
        return g;
    }
    if sp.lfe {
        (g[0][LFE], g[1][LFE]) = (0.5, 0.5);
        return g;
    }
    // R10b: the Surround Panner's Constant Power pan type, whatever the track's.
    let pt = if sp.constant_power { PanType::ConstantPower } else { pt };
    let s = 1.0 - sp.smooth.clamp(0.0, 1.0);
    let x = (sp.x + x_trim).clamp(-1.0, 1.0) * s;
    let y = sp.y.clamp(-1.0, 1.0) * s;
    let m = pt.matrix(x);
    // Row gains: front, side (7.1), back.
    let (front, side, back) = if ch > SR {
        let (near, far) = one_d(pt, y);
        if y >= 0.0 { (near, far, 0.0) } else { (0.0, far, near) }
    } else {
        let (near, far) = one_d(pt, y);
        if y >= 0.0 { (near, 0.0, far) } else { (far, 0.0, near) }
    };
    let rows = [(FL, FR, front), (BL, BR, back), (SL, SR, side)];
    for (l, r, gain) in rows {
        if r < ch && gain > 0.0 {
            (g[0][l], g[1][l]) = (m[0][0] * gain, m[0][1] * gain);
            (g[0][r], g[1][r]) = (m[1][0] * gain, m[1][1] * gain);
        }
    }
    // The centre: the mono sum, by the far-speaker law of |x| (full in the middle, nothing at a side), on the front row.
    let c = one_d(pt, x).1 * front * 0.5;
    (g[0][FC], g[1][FC]) = (c, c);
    for k in 0..ch {
        if !sp.speaker_on(k) {
            (g[0][k], g[1][k]) = (0.0, 0.0);
        }
    }
    g
}

/// ITU-R BS.1770-4 channel weights for loudness: 1 for the fronts and sides-as-fronts, 1.41 (+1.5 dB) for the
/// surround channels, 0 for LFE (left out of the measure). Stereo: both 1.
pub fn loudness_weights(ch: usize) -> [f64; MAX] {
    let mut w = [1.0f64; MAX];
    if ch > 2 {
        w[LFE] = 0.0;
        w[BL] = 1.41;
        w[BR] = 1.41;
        w[SL] = 1.41;
        w[SR] = 1.41;
    }
    for v in w.iter_mut().skip(ch) {
        *v = 0.0;
    }
    w
}

/// The layout of a buffer of `ch` interleaved channels (stereo for anything that is not 6 or 8).
pub fn layout_of(ch: usize) -> BusMode {
    BusMode::of_channels(ch).unwrap_or(BusMode::Stereo)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sp(x: f32, y: f32) -> SurroundPan {
        SurroundPan { x, y, ..Default::default() }
    }

    fn place(pt: PanType, sp: &SurroundPan, l: f32, r: f32) -> Vec<f32> {
        let g = pan_matrix(pt, sp, 0.0, 6);
        (0..6).map(|c| g[0][c] * l + g[1][c] * r).collect()
    }

    /// R10b acceptance: the puck at front-left gives an FL gain of 1 and nothing elsewhere.
    #[test]
    fn front_left_puck_feeds_the_front_left_speaker_alone() {
        let out = place(PanType::Balance0, &sp(-1.0, 1.0), 1.0, 1.0);
        assert_eq!(out, [1.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        // Add Channels: the right side moves into the left speaker too.
        assert_eq!(place(PanType::AddChannels, &sp(-1.0, 1.0), 0.3, 0.5), [0.8, 0.0, 0.0, 0.0, 0.0, 0.0]);
    }

    /// The front centre keeps L and R where they are and gives the centre speaker the mono sum (Balance 0 dB); with the
    /// centre off, nothing goes there; the rear puts them in the backs; the LFE switch sends the mono sum to LFE alone.
    #[test]
    fn front_rear_centre_and_lfe() {
        assert_eq!(place(PanType::Balance0, &sp(0.0, 1.0), 0.2, 0.4), [0.2, 0.4, 0.3, 0.0, 0.0, 0.0]);
        let no_c = SurroundPan { off: 1 << FC, ..sp(0.0, 1.0) };
        assert_eq!(place(PanType::Balance0, &no_c, 0.2, 0.4), [0.2, 0.4, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(place(PanType::Balance0, &sp(0.0, -1.0), 0.2, 0.4), [0.0, 0.0, 0.0, 0.0, 0.2, 0.4]);
        let lfe = SurroundPan { lfe: true, ..sp(0.7, -0.2) };
        assert_eq!(place(PanType::Balance0, &lfe, 0.2, 0.4), [0.0, 0.0, 0.0, 0.3, 0.0, 0.0]);
        // Centre only (the others off): the mono sum on C, as a centre dialogue track.
        let only_c = SurroundPan { off: !(1 << FC), ..sp(0.0, 1.0) };
        assert_eq!(place(PanType::Balance0, &only_c, 0.2, 0.4), [0.0, 0.0, 0.3, 0.0, 0.0, 0.0]);
    }

    /// Constant Power keeps the power of a mono signal at 1 across the front-back axis and a side; the middle of the
    /// room spreads it over front and back at -3 dB each; smoothness 1 pans nothing.
    #[test]
    fn constant_power_and_smoothness() {
        for y in [-1.0f32, -0.5, 0.0, 0.5, 1.0] {
            let no_c = SurroundPan { off: 1 << FC, ..sp(-1.0, y) };
            let out = place(PanType::ConstantPower, &no_c, 1.0, 1.0);
            let power: f32 = out.iter().map(|v| v * v).sum();
            assert!((power - 1.0).abs() < 1e-5, "y {y}: {out:?}");
        }
        let mid = place(PanType::ConstantPower, &SurroundPan { off: 1 << FC, ..sp(0.0, 0.0) }, 1.0, 0.0);
        assert!((mid[FL] - 0.5).abs() < 1e-6 && (mid[BL] - 0.5).abs() < 1e-6 && mid[FR] == 0.0, "{mid:?}");
        let flat = place(PanType::Balance0, &SurroundPan { smooth: 1.0, ..sp(1.0, -1.0) }, 1.0, 1.0);
        assert_eq!(flat, [1.0, 1.0, 1.0, 0.0, 1.0, 1.0]);
        // 7.1: the middle row is the sides.
        let g = pan_matrix(PanType::Balance0, &sp(0.0, 0.0), 0.0, 8);
        assert_eq!((g[0][FL], g[0][SL], g[0][BL], g[1][SR]), (1.0, 1.0, 0.0, 1.0));
        let g = pan_matrix(PanType::Balance0, &sp(0.0, -1.0), 0.0, 8);
        assert_eq!((g[0][FL], g[0][SL], g[0][BL], g[1][BR]), (0.0, 0.0, 1.0, 1.0));
        // Stereo: the pan type's own matrix, the Pan trim included.
        let g = pan_matrix(PanType::Balance0, &sp(0.0, 1.0), 0.5, 2);
        assert_eq!((g[0][FL], g[1][FR], g[0][FR]), (0.5, 1.0, 0.0));
    }

    /// R10a acceptance: the downmix matrix sums as BS.775 says, and the conversions go both ways.
    #[test]
    fn downmix_and_convert() {
        let f = [0.1, 0.2, 0.3, 0.9, 0.5, 0.6];
        let [l, r] = downmix_frame(&f);
        assert!((l - (0.1 + 0.3 * SURROUND_LEVEL + 0.5 * SURROUND_LEVEL)).abs() < 1e-6);
        assert!((r - (0.2 + 0.3 * SURROUND_LEVEL + 0.6 * SURROUND_LEVEL)).abs() < 1e-6);
        let mut out = Vec::new();
        downmix(&[f, f].concat(), 6, &mut out);
        assert_eq!(out.len(), 4);
        convert(&[0.3, 0.4], 2, 6, &mut out);
        assert_eq!(out, [0.3, 0.4, 0.0, 0.0, 0.0, 0.0]);
        convert(&[0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8], 8, 6, &mut out);
        assert!(out.iter().zip([0.1, 0.2, 0.3, 0.4, 1.2, 1.4]).all(|(a, b)| (a - b).abs() < 1e-6), "{out:?}");
        convert(&f, 6, 8, &mut out);
        assert_eq!(out, [0.1, 0.2, 0.3, 0.9, 0.5, 0.6, 0.0, 0.0]);
        convert(&[0.5, 0.25], 2, 1, &mut out);
        assert_eq!(out, [0.375]);
        convert(&f, 6, 6, &mut out);
        assert_eq!(out, f);
        assert_eq!(loudness_weights(6), [1.0, 1.0, 1.0, 0.0, 1.41, 1.41, 0.0, 0.0]);
        assert_eq!(loudness_weights(2), [1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    }
}
