//! R17a: lining events up by their sound, media timecode or date/time stamp (Tools › Multicamera › Synchronize Audio /
//! Media Timecode / Date/Time Stamp to Align Events; Project Media's Lay Out Tracks Using Media Timecode / Date/Time
//! Stamp). The stamps are read at import (`media.rs`); the sound is matched here, on a thread of its own (`AudioJob`).
//!
//! Sound: each event is read as the mixer reads it and reduced to a 1 kHz envelope (mean |sample| per ms). The
//! envelopes are cross-correlated at every lag at once (FFT), each lag scored by its correlation over the overlap, and
//! the best lag is refined sample by sample on up to 2 s of the raw sound where both are loudest. A low score, or a
//! second lag scoring nearly as well, is refused: nothing moves on a guess.
use std::io::Read;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};
use std::sync::{Arc, mpsc};
use eframe::egui;
use crate::audio::RATE;
use crate::engine::plan::ASrc;
use crate::project::{MediaItem, exact_fps};

/// What Synchronize (and Lay Out Tracks) matches events by.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum By {
    Audio,
    Timecode,
    Date,
}

// ---- Stamps ----

/// Media timecode `tag` ("01:00:00:00", drop-frame "01:00:00;00" or "01:00:00.00") of a file at `fps`, as seconds of
/// real time since 00:00:00:00. Drop-frame follows the tag's separator, at 29.97 and 59.94 only (SMPTE 12M).
pub fn parse_timecode(tag: &str, fps: f64) -> Option<f64> {
    let tag = tag.trim();
    let nominal = fps.round() as u64;
    let parts: Vec<u64> = tag.split([':', ';', '.', ',']).map(|p| p.parse().ok()).collect::<Option<_>>()?;
    let [_, m, s, f] = parts[..] else { return None };
    if !(fps > 0.0) || m >= 60 || s >= 60 || f >= nominal {
        return None;
    }
    let drop = tag.contains([';', '.', ',']) && fps.fract() != 0.0 && nominal % 30 == 0;
    crate::project::parse_smpte(&tag.replace(['.', ','], ";"), exact_fps(fps), nominal, drop)
}

/// An ffprobe `creation_time` ("2024-05-01T12:34:56.000000Z", also with a space, no fraction or a "+02:00" offset) as
/// Unix seconds. Dates before 1971 are cameras' "never set" clocks: None.
pub fn parse_date(s: &str) -> Option<f64> {
    let s = s.trim();
    let (date, rest) = s.split_at_checked(10)?;
    let d: Vec<i64> = date.split('-').map(|p| p.parse().ok()).collect::<Option<_>>()?;
    let [y, mo, da] = d[..] else { return None };
    let rest = rest.strip_prefix(['T', 't', ' '])?;
    let end = rest.find(['Z', 'z', '+', '-']).unwrap_or(rest.len());
    let (time, zone) = rest.split_at(end);
    let t: Vec<&str> = time.split(':').collect();
    let [h, mi, sec] = t[..] else { return None };
    let (h, mi, sec): (i64, i64, f64) = (h.parse().ok()?, mi.parse().ok()?, sec.parse().ok()?);
    let offset = match zone {
        "" | "Z" | "z" => 0,
        z => {
            let sign = if z.starts_with('-') { -1 } else { 1 };
            let (oh, om) = z[1..].split_once(':').unwrap_or((&z[1..], "0"));
            sign * (oh.parse::<i64>().ok()? * 3600 + om.parse::<i64>().ok()? * 60)
        }
    };
    if y < 1971 || !(1..=12).contains(&mo) || !(1..=31).contains(&da) || h > 23 || mi > 59 || !(0.0..61.0).contains(&sec) {
        return None;
    }
    Some(((days_from_civil(y, mo, da) * 86_400 + h * 3600 + mi * 60 - offset) as f64) + sec)
}

/// Days from 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

/// When media `m`'s file began recording, in Unix seconds: its `creation_time` tag, else its file's modified time less
/// its length (a camera writes the file until the recording stops). None for generated media and missing files.
pub fn recorded(m: &MediaItem) -> Option<f64> {
    if m.generator.is_some() || m.nested {
        return None;
    }
    m.created.or_else(|| {
        let t = std::fs::metadata(&m.path).and_then(|f| f.modified()).ok()?;
        // shortcut: a subclip is taken to run to its file's end (the parent's length is not at hand here).
        Some(t.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs_f64() - m.file_time(m.duration))
    })
}

/// Media `m`'s stamp of kind `by` (seconds; Audio has none).
pub fn stamp(m: &MediaItem, by: By) -> Option<f64> {
    match by {
        By::Timecode => m.timecode,
        By::Date => recorded(m),
        By::Audio => None,
    }
}

// ---- Matching sound ----

/// Envelope rate: one value per this many samples (1 ms at 48 kHz) unless the events are too long for one FFT.
const PER: usize = RATE as usize / 1000;
/// The largest FFT (complex f32): 32 MB a buffer.
const MAX_FFT: usize = 1 << 22;
/// A match needs at least this score (correlation of the envelopes over the overlap)...
const MIN_SCORE: f32 = 0.5;
/// ...and no other peak scoring more than this share of it.
const MAX_RIVAL: f32 = 0.75;
/// The sample-exact step reads up to this many seconds of both sounds.
const FINE_SECS: f64 = 2.0;

/// The best lag of envelope `b` in envelope `a` (b[i] goes with a[i + lag]) among lags where they overlap by at least
/// `min` values, its score, and the best score of any other peak.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Peak {
    pub lag: i64,
    pub score: f32,
    pub rival: f32,
}

impl Peak {
    /// High enough and unambiguous.
    pub fn confident(&self) -> bool {
        self.score >= MIN_SCORE && self.rival <= MAX_RIVAL * self.score
    }
}

type C = [f32; 2];

/// In-place radix-2 FFT (`x.len()` a power of two); the inverse is not scaled.
fn fft(x: &mut [C], inverse: bool) {
    let n = x.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            x.swap(i, j);
        }
    }
    let sign = if inverse { 1.0 } else { -1.0 };
    let tw: Vec<C> = (0..n / 2).map(|k| std::f64::consts::TAU * sign * k as f64 / n as f64).map(|a| [a.cos() as f32, a.sin() as f32]).collect();
    let mut len = 2;
    while len <= n {
        let (half, step) = (len / 2, n / len);
        for s in (0..n).step_by(len) {
            for k in 0..half {
                let (w, u, v) = (tw[k * step], x[s + k], x[s + k + half]);
                let t = [v[0] * w[0] - v[1] * w[1], v[0] * w[1] + v[1] * w[0]];
                x[s + k] = [u[0] + t[0], u[1] + t[1]];
                x[s + k + half] = [u[0] - t[0], u[1] - t[1]];
            }
        }
        len <<= 1;
    }
}

fn prefix(v: &[f32]) -> Vec<f64> {
    std::iter::once(0.0).chain(v.iter().scan(0.0, |s, x| {
        *s += (*x as f64) * (*x as f64);
        Some(*s)
    })).collect()
}

/// Where envelope `b` best fits in envelope `a` (see `Peak`). None when too short.
pub fn coarse(a: &[f32], b: &[f32], min: usize) -> Option<Peak> {
    let score = scores(a, b, min)?;
    let nb = b.len();
    let (best, &top) = score.iter().enumerate().max_by(|x, y| x.1.total_cmp(y.1))?;
    if !top.is_finite() {
        return None;
    }
    // The peak's own lobe (down to half its score) is not a rival.
    let (mut lo, mut hi) = (best, best);
    while lo > 0 && score[lo - 1] > top * 0.5 {
        lo -= 1;
    }
    while hi + 1 < score.len() && score[hi + 1] > top * 0.5 {
        hi += 1;
    }
    let rival = score[..lo].iter().chain(&score[hi + 1..]).copied().filter(|s| s.is_finite()).fold(0.0f32, f32::max);
    Some(Peak { lag: best as i64 - (nb as i64 - 1), score: top, rival })
}

/// The score of every lag d of `b` in `a` (b[i] with a[i + d]), at index d + b.len() - 1: both envelopes centred,
/// cross-correlated by FFT, and each lag's sum divided by their energy over that lag's overlap (-inf where they
/// overlap by fewer than `min` values, 0 where either is silent).
fn scores(a: &[f32], b: &[f32], min: usize) -> Option<Vec<f32>> {
    let (na, nb) = (a.len(), b.len());
    let min = min.min(na).min(nb).max(2);
    if na < 2 || nb < 2 || na + nb > MAX_FFT {
        return None;
    }
    let centre = |v: &[f32]| {
        let m = v.iter().map(|x| *x as f64).sum::<f64>() / v.len() as f64;
        v.iter().map(|x| (*x as f64 - m) as f32).collect::<Vec<f32>>()
    };
    let (a, b) = (centre(a), centre(b));
    let n = (na + nb).next_power_of_two();
    let mut fa: Vec<C> = a.iter().map(|&v| [v, 0.0]).chain(std::iter::repeat([0.0; 2])).take(n).collect();
    let mut fb: Vec<C> = b.iter().map(|&v| [v, 0.0]).chain(std::iter::repeat([0.0; 2])).take(n).collect();
    fft(&mut fa, false);
    fft(&mut fb, false);
    for (x, y) in fa.iter_mut().zip(&fb) {
        *x = [x[0] * y[0] + x[1] * y[1], x[1] * y[0] - x[0] * y[1]]; // x · conj(y)
    }
    drop(fb);
    fft(&mut fa, true);
    let (pa, pb) = (prefix(&a), prefix(&b));
    let score = (-(nb as i64 - 1)..na as i64).map(|d| {
        let (i0, i1) = ((-d).max(0) as usize, (nb as i64).min(na as i64 - d) as usize);
        if i1 < i0 + min {
            return f32::NEG_INFINITY;
        }
        let (ea, eb) = (pa[(i1 as i64 + d) as usize] - pa[(i0 as i64 + d) as usize], pb[i1] - pb[i0]);
        let c = fa[d.rem_euclid(n as i64) as usize][0] as f64 / n as f64;
        if ea > 1e-12 && eb > 1e-12 { (c / (ea * eb).sqrt()) as f32 } else { 0.0 }
    });
    Some(score.collect())
}

/// The shift `e` in `-w..=w` at which `b[j + w - e]` best matches `a[j]` sample for sample (`b` is `a.len() + 2w`
/// long), and that match's |normalised correlation| (a microphone wired in reverse polarity still matches).
pub fn fine(a: &[f32], b: &[f32], w: usize) -> (i64, f32) {
    let l = a.len();
    let ea: f64 = a.iter().map(|x| (*x as f64).powi(2)).sum();
    let pb = prefix(b);
    let mut best = (0, 0.0f32);
    for off in 0..=2 * w {
        if off + l > b.len() {
            break;
        }
        let dot: f64 = a.iter().zip(&b[off..off + l]).map(|(x, y)| (*x * *y) as f64).sum();
        let eb = pb[off + l] - pb[off];
        let r = if ea > 1e-12 && eb > 1e-12 { (dot / (ea * eb).sqrt()).abs() as f32 } else { 0.0 };
        if r > best.1 {
            best = (w as i64 - off as i64, r);
        }
    }
    best
}

/// One event's sound to match: as the mixer reads it (`src`, which is never reversed), where the event starts on the
/// timeline and how long it is.
#[derive(Clone, Debug)]
pub struct Event {
    pub src: ASrc,
    pub pos: f64,
    pub len: f64,
}

/// `len` seconds of `src`'s sound from `from` seconds into its event, mixed to mono at RATE, handed to `each` in chunks.
/// Err when it cannot be read, or "cancelled" once `stop` is set.
fn read(src: &ASrc, from: f64, len: f64, stop: &AtomicBool, mut each: impl FnMut(&[f32])) -> Result<(), String> {
    let mut a = crate::engine::mix::args(src, (from * RATE as f64).round() as i64, false);
    a.splice(4..4, ["-t".to_string(), format!("{:.6}", len * src.rate)]); // after "-ss <time>": source seconds
    let mut cmd = crate::proc::ffmpeg();
    cmd.args(a).stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = crate::proc::spawn(&mut cmd).map_err(|e| crate::proc::tool_error(&e, "ffmpeg"))?;
    let mut out = child.stdout.take().expect("piped stdout");
    let (mut bytes, mut mono) = (vec![0u8; 8 * 8192], Vec::with_capacity(8192));
    loop {
        if stop.load(Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("cancelled".into());
        }
        let mut n = 0;
        while n < bytes.len() {
            match out.read(&mut bytes[n..]) {
                Ok(0) | Err(_) => break,
                Ok(k) => n += k,
            }
        }
        mono.clear();
        mono.extend(bytes[..n - n % 8].chunks_exact(8).map(|f| {
            let v = (f32::from_le_bytes([f[0], f[1], f[2], f[3]]) + f32::from_le_bytes([f[4], f[5], f[6], f[7]])) * 0.5;
            if v.is_finite() { v } else { 0.0 }
        }));
        each(&mono);
        if n < bytes.len() {
            break;
        }
    }
    drop(out);
    if child.wait().is_ok_and(|s| s.success()) { Ok(()) } else { Err("its audio could not be read".into()) }
}

/// `len` samples from `from` seconds into `e` (which may be before the event: silence there) as one buffer.
fn window(e: &Event, from: f64, len: usize, stop: &AtomicBool) -> Result<Vec<f32>, String> {
    let lead = ((-from * RATE as f64).round().max(0.0) as usize).min(len);
    let mut v = vec![0.0; lead];
    if lead < len {
        read(&e.src, from.max(0.0), (len - lead) as f64 / RATE as f64, stop, |c| v.extend_from_slice(c))?;
    }
    v.resize(len, 0.0);
    Ok(v)
}

/// The envelope of `len` seconds of `e` from `from` seconds in, `per` samples a value; `tick` gets the share read.
fn env_of(e: &Event, from: f64, len: f64, per: usize, stop: &AtomicBool, mut tick: impl FnMut(f64)) -> Result<Vec<f32>, String> {
    let (mut env, mut acc, mut k) = (Vec::with_capacity((len * RATE as f64) as usize / per + 1), 0.0f32, 0);
    let total = (len * RATE as f64).max(1.0);
    let mut seen = 0usize;
    read(&e.src, from, len, stop, |c| {
        for v in c {
            acc += v.abs();
            k += 1;
            if k == per {
                env.push(acc / per as f32);
                (acc, k) = (0.0, 0);
            }
        }
        seen += c.len();
        tick(seen as f64 / total);
    })?;
    if k > 0 {
        env.push(acc / k as f32);
    }
    Ok(env)
}

/// Where each of `others` matches the references (`refs`, read only inside `range`, the time selection, where it
/// covers them): the timeline position its start goes to, or why it has none. `progress` counts to 1000.
pub fn match_audio(refs: &[Event], others: &[Event], range: Option<(f64, f64)>, stop: &AtomicBool, progress: &dyn Fn(u32)) -> Vec<Result<f64, String>> {
    // Each reference's stretch read: the time selection where it covers part of the event, else all of it.
    let span = |e: &Event| match range {
        Some((a, b)) if a.max(e.pos) < b.min(e.pos + e.len) => (a.max(e.pos), b.min(e.pos + e.len)),
        _ => (e.pos, e.pos + e.len),
    };
    let longest = |v: &[Event], cut: bool| v.iter().map(|e| if cut { span(e).1 - span(e).0 } else { e.len }).fold(0.0, f64::max);
    let total_samples = (longest(refs, true) + longest(others, false)) * RATE as f64;
    let mut per = PER;
    while total_samples / per as f64 + 2.0 > MAX_FFT as f64 / 2.0 {
        per *= 2; // shortcut: very long events are matched on a coarser envelope; the fine step still finds the sample
    }
    let steps = (refs.len() + 2 * others.len()).max(1) as f64;
    let (mut step, last) = (0.0, std::cell::Cell::new(0));
    let tick = |within: f64, step: f64| {
        let p = ((step + within.clamp(0.0, 1.0)) / steps * 1000.0) as u32;
        if p != last.get() {
            last.set(p);
            progress(p);
        }
    };
    let ref_env: Vec<Option<(f64, Vec<f32>)>> = refs
        .iter()
        .map(|e| {
            let (a, b) = span(e);
            let env = env_of(e, a - e.pos, b - a, per, stop, |f| tick(f, step)).ok().map(|v| (a, v));
            step += 1.0;
            env
        })
        .collect();
    let mut out = Vec::new();
    for o in others {
        if stop.load(Relaxed) {
            break;
        }
        let env = env_of(o, 0.0, o.len, per, stop, |f| tick(f, step));
        step += 1.0;
        let r = env.and_then(|env| {
            let found = place(&ref_env, refs, o, &env, per, stop);
            tick(1.0, step);
            found
        });
        step += 1.0;
        out.push(r);
    }
    out
}

/// The match of `o` (envelope `env`) against the best-fitting reference, refined to the sample: `o`'s new start.
fn place(ref_env: &[Option<(f64, Vec<f32>)>], refs: &[Event], o: &Event, env: &[f32], per: usize, stop: &AtomicBool) -> Result<f64, String> {
    let min_overlap = |n: usize| (5 * RATE as usize / per).max(n / 10);
    let best = ref_env
        .iter()
        .enumerate()
        .filter_map(|(k, r)| {
            let (start, renv) = r.as_ref()?;
            Some((k, *start, coarse(renv, env, min_overlap(renv.len().min(env.len())))?))
        })
        .max_by(|x, y| x.2.score.total_cmp(&y.2.score));
    let Some((k, start, p)) = best else {
        return Err(if ref_env.iter().all(Option::is_none) { "the reference audio could not be read".into() } else { "no audio to compare".into() });
    };
    if !p.confident() {
        let why = if p.score < MIN_SCORE { "no confident audio match" } else { "the audio matches equally well at another place" };
        return Err(format!("{why}: score {:.2}, next best {:.2}", p.score, p.rival));
    }
    // Fine: where both are loudest together, up to FINE_SECS, compared sample by sample around the envelope's lag.
    let renv = &ref_env[k].as_ref().expect("matched").1;
    let (lo, hi) = (p.lag.max(0) as usize, (renv.len() as i64).min(p.lag + env.len() as i64) as usize);
    let win = ((FINE_SECS * RATE as f64) as usize / per).min(hi - lo).max(1);
    let both: Vec<f32> = (lo..hi).map(|i| renv[i] * env[(i as i64 - p.lag) as usize]).collect();
    let sums: Vec<f32> = both.windows(win).map(|w| w.iter().sum()).collect();
    let at = lo + sums.iter().enumerate().max_by(|x, y| x.1.total_cmp(y.1)).map_or(0, |m| m.0);
    let (lag, w, len) = (p.lag * per as i64, (RATE as usize / 100).max(3 * per), win * per);
    let r = &refs[k];
    let t0 = at * per; // in the reference's read stretch
    let a = window(r, start - r.pos + t0 as f64 / RATE as f64, len, stop)?;
    let b = window(o, (t0 as i64 - lag - w as i64) as f64 / RATE as f64, len + 2 * w, stop)?;
    let (e, score) = fine(&a, &b, w);
    // A weak sample match (different microphones, heavy noise) keeps the envelope's lag, good to a millisecond.
    let lag = if score >= 0.2 && e.unsigned_abs() < w as u64 { lag + e } else { lag };
    Ok(start + lag as f64 / RATE as f64)
}

/// Synchronize Audio to Align Events analysing in the background: `match_audio` on a "milan-sync" thread, with its
/// progress and a Cancel.
pub struct AudioJob {
    stop: Arc<AtomicBool>,
    done: Arc<AtomicU32>,
    rx: mpsc::Receiver<Vec<Result<f64, String>>>,
}

impl AudioJob {
    pub fn start(refs: Vec<Event>, others: Vec<Event>, range: Option<(f64, f64)>, ctx: egui::Context) -> Self {
        let (stop, done) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicU32::new(0)));
        let (tx, rx) = mpsc::channel();
        let (s, d) = (stop.clone(), done.clone());
        let spawned = std::thread::Builder::new().name("milan-sync".into()).spawn(move || {
            let progress = |p: u32| {
                d.store(p, Relaxed);
                ctx.request_repaint();
            };
            let _ = tx.send(match_audio(&refs, &others, range, &s, &progress));
            ctx.request_repaint();
        });
        if spawned.is_err() {
            stop.store(true, Relaxed); // the receiver then reports it as cancelled
        }
        Self { stop, done, rx }
    }

    /// 0..1.
    pub fn progress(&self) -> f32 {
        self.done.load(Relaxed) as f32 / 1000.0
    }

    pub fn cancel(&self) {
        self.stop.store(true, Relaxed);
    }

    pub fn cancelled(&self) -> bool {
        self.stop.load(Relaxed)
    }

    /// The finished result (Some once): each other event's new start, or why it has none. A cancelled job gives
    /// Some(empty) once its thread has stopped.
    pub fn poll(&self) -> Option<Vec<Result<f64, String>>> {
        match self.rx.try_recv() {
            Ok(r) if !self.cancelled() => Some(r),
            Ok(_) | Err(mpsc::TryRecvError::Disconnected) => Some(Vec::new()),
            Err(mpsc::TryRecvError::Empty) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mean |sample| per `per` samples, as `env_of` makes it while reading.
    fn envelope(pcm: &[f32], per: usize) -> Vec<f32> {
        pcm.chunks(per).map(|c| c.iter().map(|v| v.abs()).sum::<f32>() / c.len() as f32).collect()
    }

    /// A reproducible noise generator (xorshift), -1..1.
    fn noise(seed: u64, n: usize) -> Vec<f32> {
        let mut s = seed.max(1);
        (0..n)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                (s >> 40) as f32 / (1u64 << 23) as f32 - 1.0
            })
            .collect()
    }

    /// Noise in bursts of 40-300 ms with gaps, like speech or claps: `secs` long.
    fn bursts(seed: u64, secs: f64) -> Vec<f32> {
        let n = (secs * RATE as f64) as usize;
        let (mut v, gate) = (noise(seed, n), noise(seed ^ 0x5555, 64));
        let mut on = 0;
        let mut k = 0;
        let mut i = 0;
        while i < n {
            let len = ((gate[k % 64].abs() * 0.26 + 0.04) * RATE as f32) as usize;
            if on % 2 == 1 {
                v[i..(i + len).min(n)].iter_mut().for_each(|x| *x *= 0.02);
            }
            (i, k, on) = (i + len, k + 1, on + 1);
        }
        v
    }

    /// `b` against `a` as `match_audio` does it, from samples: coarse on the envelopes, then the fine step.
    fn lag_of(a: &[f32], b: &[f32]) -> Result<i64, Peak> {
        let p = coarse(&envelope(a, PER), &envelope(b, PER), 5 * RATE as usize / PER).unwrap();
        if !p.confident() {
            return Err(p);
        }
        let (lag, w) = (p.lag * PER as i64, RATE as usize / 100);
        let t0 = (lag.max(0) as usize) + RATE as usize / 2;
        let len = 2 * RATE as usize;
        let a = &a[t0..t0 + len];
        let s0 = t0 as i64 - lag - w as i64;
        let b: Vec<f32> = (s0..s0 + (len + 2 * w) as i64).map(|i| if i < 0 { 0.0 } else { b.get(i as usize).copied().unwrap_or(0.0) }).collect();
        let (e, r) = fine(a, &b, w);
        assert!(r > 0.2, "{r}");
        Ok(lag + e)
    }

    /// Acceptance: two copies of a noise burst track, the second starting 1234 samples later, sync to exactly 1234;
    /// also the other way round, and with the copy buried in other noise and turned upside down (polarity).
    #[test]
    fn a_copy_offset_by_1234_samples_syncs_to_exactly_1234() {
        let a = bursts(7, 12.0);
        let mut b = vec![0.0; 1234];
        b.extend_from_slice(&a[..a.len() - 2000]);
        assert_eq!(lag_of(&a, &b), Ok(-1234), "b's sample i is a's i - 1234: it goes 1234 samples later");
        assert_eq!(lag_of(&b, &a), Ok(1234));
        // 1.234 s (the batch's case) with independent noise at a quarter of the level, inverted.
        let shift = 59_232;
        let hiss = noise(99, a.len());
        let c: Vec<f32> = (0..a.len()).map(|i| -a.get(i + shift).copied().unwrap_or(0.0) + 0.25 * hiss[i]).collect();
        assert_eq!(lag_of(&a, &c), Ok(shift as i64));
    }

    /// Acceptance: unrelated sounds are refused, never moved on the best of bad lags; so is a sound that repeats.
    #[test]
    fn a_weak_or_ambiguous_match_is_refused() {
        let (a, b) = (bursts(1, 10.0), bursts(2, 10.0));
        let p = lag_of(&a, &b).unwrap_err();
        assert!(!p.confident(), "{p:?}");
        // The same 2 s phrase three times: it fits three places equally well.
        let phrase = bursts(3, 2.0);
        let loop3: Vec<f32> = phrase.iter().chain(&phrase).chain(&phrase).copied().collect();
        let p = coarse(&envelope(&loop3, PER), &envelope(&phrase, PER), 1000).unwrap();
        assert!(!p.confident() && p.rival > MAX_RIVAL * p.score, "{p:?}");
        // Silence matches nothing.
        assert!(coarse(&vec![0.0; 6000], &envelope(&a, PER), 1000).is_none_or(|p| !p.confident()));
    }

    /// The FFT scores are the direct sums: every lag of two short envelopes against a brute-force correlation.
    #[test]
    fn coarse_scores_every_lag_like_a_direct_sum() {
        let (a, b) = (envelope(&bursts(5, 3.0), PER), envelope(&bursts(5, 3.0)[RATE as usize..], PER));
        let p = coarse(&a, &b, 1500).unwrap();
        assert_eq!(p.lag, 1000);
        assert!(p.score > 0.99 && p.confident(), "{p:?}");
        let centre = |v: &[f32]| {
            let m = v.iter().map(|x| *x as f64).sum::<f64>() / v.len() as f64;
            v.iter().map(|x| *x as f64 - m).collect::<Vec<_>>()
        };
        let (ca, cb) = (centre(&a), centre(&b));
        let direct = |d: i64| {
            let pairs = (0..cb.len()).filter_map(|i| Some((*ca.get(usize::try_from(i as i64 + d).ok()?)?, cb[i])));
            let (mut s, mut ea, mut eb, mut n) = (0.0, 0.0, 0.0, 0);
            for (x, y) in pairs {
                (s, ea, eb, n) = (s + x * y, ea + x * x, eb + y * y, n + 1);
            }
            if n < 1500 { f64::NEG_INFINITY } else { s / (ea * eb).sqrt() }
        };
        let fast = scores(&a, &b, 1500).unwrap();
        for (k, s) in fast.iter().enumerate() {
            let d = direct(k as i64 - (b.len() as i64 - 1));
            assert!(d == f64::NEG_INFINITY && *s == f32::NEG_INFINITY || (d - *s as f64).abs() < 1e-3, "lag {k}: {d} vs {s}");
        }
        // The fine step finds a shift within its window and reports polarity-free scores.
        let s = noise(4, 4000);
        let a2 = &s[100..1100];
        assert_eq!(fine(a2, &s[100 - 20..1100 + 20], 20), (0, 1.0));
        let inv: Vec<f32> = s.iter().map(|v| -v).collect();
        let (e, r) = fine(a2, &inv[100 - 20 + 7..1100 + 20 + 7], 20);
        assert_eq!(e, 7);
        assert!((r - 1.0).abs() < 1e-4);
    }

    /// The timecode parser handles drop-frame and non-drop-frame tags; dates parse too.
    #[test]
    fn timecode_tags_drop_and_non_drop() {
        let close = |tag: &str, fps: f64, want: f64| {
            let got = parse_timecode(tag, fps).unwrap_or(f64::NAN);
            assert!((got - want).abs() < 1e-9, "{tag} at {fps}: {got} vs {want}");
        };
        close("01:00:00:00", 25.0, 3600.0);
        close("00:00:10:12", 25.0, 10.48);
        // Non-drop at 29.97: 108000 frames of 1001/30000 s.
        close("01:00:00:00", 29.97, 108_000.0 * 1001.0 / 30_000.0);
        // Drop-frame: 01:00:00;00 is 107892 frames, real time 3599.9964 s; "." is drop-frame too.
        let df = 107_892.0 * 1001.0 / 30_000.0;
        close("01:00:00;00", 29.97, df);
        close("01:00:00.00", 29.97, df);
        close("00:01:00;02", 29.97, 1800.0 * 1001.0 / 30_000.0);
        close("10:00:00;00", 59.94, 2_157_840.0 * 1001.0 / 60_000.0);
        // Non-NTSC rates have no drop-frame, whatever the separator.
        close("00:00:01;00", 25.0, 1.0);
        close("00:00:01:00", 23.976, 24.0 * 1001.0 / 24_000.0);
        for bad in ["", "01:00:00", "01:00:00:25", "01:60:00:00", "aa:00:00:00", "-1:00:00:00"] {
            assert_eq!(parse_timecode(bad, 25.0), None, "{bad}");
        }
        assert_eq!(parse_timecode("01:00:00:00", 0.0), None);
        assert_eq!(parse_date("2024-05-01T12:34:56.000000Z"), Some(1_714_566_896.0));
        assert_eq!(parse_date("2024-05-01 12:34:56"), Some(1_714_566_896.0));
        assert_eq!(parse_date("2024-05-01T14:34:56.5+02:00"), Some(1_714_566_896.5));
        assert_eq!(parse_date("2000-02-29T00:00:00Z"), Some(951_782_400.0));
        assert_eq!(parse_date("1904-01-01T00:00:00.000000Z"), None, "an unset camera clock");
        assert_eq!(parse_date("garbage"), None);
    }

    /// Needs ffmpeg. `cargo test --release -- --ignored ffmpeg_syncs_two_recordings`
    /// Two WAV recordings of one burst track, the second starting 1.234 s into it, sync to the sample through ffmpeg,
    /// as Synchronize reads them (one of them trimmed, so its in point counts); a cancelled job stops.
    #[test]
    #[ignore]
    fn ffmpeg_syncs_two_recordings() {
        let dir = std::env::temp_dir().join(format!("milan-sync-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let write = |name: &str, pcm: &[f32]| {
            let path = dir.join(name);
            let raw = dir.join(format!("{name}.f32"));
            std::fs::write(&raw, pcm.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>()).unwrap();
            let ok = crate::proc::ffmpeg().args(["-v", "error", "-y", "-f", "f32le", "-ar", "48000", "-ac", "1", "-i"]).arg(&raw).args(["-c:a", "pcm_f32le"]).arg(&path).status().unwrap().success();
            assert!(ok);
            std::fs::remove_file(&raw).unwrap();
            path
        };
        let a = bursts(11, 14.0);
        let shift = 59_232; // 1.234 s
        let pa = write("a.wav", &a);
        let pb = write("b.wav", &a[shift..]);
        let ev = |path: &std::path::Path, pos: f64, len: f64, in_point: f64| Event {
            src: ASrc { path: path.to_path_buf(), pos: 0.0, dur: len, in_point, rate: 1.0, reverse: false, stream: 0, pitch: 0.0, nested: None },
            pos,
            len,
        };
        let never = AtomicBool::new(false);
        // A at 10 s on the timeline (its first 0.5 s trimmed off); B at 3 s: B belongs at 10 + 1.234 - 0.5.
        let r = match_audio(&[ev(&pa, 10.0, 13.0, 0.5)], &[ev(&pb, 3.0, 12.0, 0.0)], None, &never, &|_| {});
        let at = r[0].clone().unwrap();
        assert!((at - (10.0 + 1.234 - 0.5)).abs() < 0.5 / RATE as f64, "{at}");
        // Read only within a time selection, it lands in the same place.
        let r = match_audio(&[ev(&pa, 10.0, 13.0, 0.5)], &[ev(&pb, 3.0, 12.0, 0.0)], Some((14.0, 21.0)), &never, &|_| {});
        assert!((r[0].clone().unwrap() - (10.0 + 1.234 - 0.5)).abs() < 0.5 / RATE as f64);
        let job = AudioJob::start(vec![ev(&pa, 0.0, 14.0, 0.0)], vec![ev(&pb, 0.0, 12.0, 0.0)], None, egui::Context::default());
        job.cancel();
        let t = std::time::Instant::now();
        let got = loop {
            if let Some(r) = job.poll() {
                break r;
            }
            assert!(t.elapsed().as_secs() < 30);
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        assert!(got.is_empty() && job.cancelled());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
