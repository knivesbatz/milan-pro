//! R16c: the Loudness Meters window (View › Window › Loudness Meters), as VEGAS has it: the statistics on the left
//! (Momentary, Short, Integrated, LRA, True Peak, with over-target lamps), the M, S and I meters on the Loudness Scale
//! and, with Show True Peak Meter, the true-peak meters. Options › Loudness Meters (also its right-click menu) sets
//! them. The measuring runs on a "milan-loudness" thread from the master output's tap (`audio::Player::loud`): the
//! mix and output threads only copy samples into a lock-free ring. Playback starting (or a seek) starts it again.
use eframe::egui::*;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use crate::audio::Ring;
use crate::engine::loudness::{Meter, Mode, Readings};
use super::menus;
use super::theme::{self, UiLook};

/// Options › Loudness Meters › True Peak Range: the bottom of the true-peak scale (to 0 dB).
pub const TP_RANGES: [f64; 8] = [-12.0, -24.0, -42.0, -60.0, -78.0, -90.0, -120.0, -138.0];

/// Options › Loudness Meters (and the window's right-click menu).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LoudCmd {
    ResetClip,
    ResetEngine,
    Mode(Mode),
    BlockingFilter,
    /// True Peak Range, an index into `TP_RANGES`.
    Range(u8),
    /// Loudness Scale › EBU +9 (false) / EBU +18 (true).
    Wide(bool),
    Absolute,
    ShowTruePeak,
    Labels,
    HoldPeaks,
    HoldValleys,
}

/// The Loudness Meters options, kept in the preferences, with VEGAS's defaults; and Preferences › Audio › "Open editor
/// when new loudness log is generated".
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LoudPrefs {
    pub mode: Mode,
    pub blocking: bool,
    pub range: u8,
    pub wide: bool,
    pub absolute: bool,
    pub show_true_peak: bool,
    pub labels: bool,
    pub hold_peaks: bool,
    pub hold_valleys: bool,
    pub open_editor: bool,
}

impl Default for LoudPrefs {
    fn default() -> Self {
        Self { mode: Mode::Ebu, blocking: false, range: 3, wide: false, absolute: false, show_true_peak: false, labels: true, hold_peaks: false, hold_valleys: false, open_editor: true }
    }
}

impl LoudPrefs {
    /// The menu row's mark (None: not a check or radio row).
    pub fn checked(&self, c: LoudCmd) -> Option<bool> {
        Some(match c {
            LoudCmd::Mode(m) => self.mode == m,
            LoudCmd::BlockingFilter => self.blocking,
            LoudCmd::Range(i) => self.range == i,
            LoudCmd::Wide(w) => self.wide == w,
            LoudCmd::Absolute => self.absolute,
            LoudCmd::ShowTruePeak => self.show_true_peak,
            LoudCmd::Labels => self.labels,
            LoudCmd::HoldPeaks => self.hold_peaks,
            LoudCmd::HoldValleys => self.hold_valleys,
            LoudCmd::ResetClip | LoudCmd::ResetEngine => return None,
        })
    }

    /// Apply an option (the resets are the meter's: `Loudness::command`).
    pub fn apply(&mut self, c: LoudCmd) {
        match c {
            LoudCmd::Mode(m) => self.mode = m,
            LoudCmd::BlockingFilter => self.blocking = !self.blocking,
            LoudCmd::Range(i) => self.range = i.min(TP_RANGES.len() as u8 - 1),
            LoudCmd::Wide(w) => self.wide = w,
            LoudCmd::Absolute => self.absolute = !self.absolute,
            LoudCmd::ShowTruePeak => self.show_true_peak = !self.show_true_peak,
            LoudCmd::Labels => self.labels = !self.labels,
            LoudCmd::HoldPeaks => self.hold_peaks = !self.hold_peaks,
            LoudCmd::HoldValleys => self.hold_valleys = !self.hold_valleys,
            LoudCmd::ResetClip | LoudCmd::ResetEngine => {}
        }
    }

    /// The Loudness Scale in LU from the target: EBU +9 (-18 to +9) or EBU +18 (-36 to +18).
    fn scale(&self) -> (f64, f64) {
        if self.wide { (-36.0, 18.0) } else { (-18.0, 9.0) }
    }
}

/// What the window and the measuring thread share. The thread holds the meter; the UI asks for resets.
#[derive(Default)]
struct Shared {
    reset: AtomicBool,
    reset_clip: AtomicBool,
    blocking: AtomicBool,
    readings: Mutex<Readings>,
}

/// The window's state: the measuring thread, started when the window first draws.
#[derive(Default)]
pub struct Loudness {
    shared: Option<Arc<Shared>>,
}

impl Loudness {
    /// Reset Clip (the true peaks and their lamp) or Reset Metering Engine (everything).
    pub fn command(&self, c: LoudCmd) {
        if let Some(s) = &self.shared {
            match c {
                LoudCmd::ResetEngine => s.reset.store(true, Relaxed),
                LoudCmd::ResetClip => s.reset_clip.store(true, Relaxed),
                _ => {}
            }
        }
    }

    fn start(ctx: &Context, ring: Arc<Ring>, blocking: bool) -> Arc<Shared> {
        let shared = Arc::new(Shared::default());
        shared.blocking.store(blocking, Relaxed);
        let (s, ctx) = (shared.clone(), ctx.clone());
        let _ = std::thread::Builder::new().name("milan-loudness".into()).spawn(move || {
            let mut buf = Vec::new();
            ring.take(&mut buf); // what played before the window opened
            let (mut rate, mut blocking) = (ring.rate.load(Relaxed), s.blocking.load(Relaxed));
            let mut m = Meter::new(rate, blocking);
            // New readings at most 10 times a second (integrated and range go over the whole programme).
            let (mut dirty, mut shown) = (false, std::time::Instant::now());
            // Ends with the window's state (Milan Pro closing). Drained every 20 ms, so the ring (16384 frames) never
            // fills, even from a 192 kHz device.
            while Arc::strong_count(&s) > 1 {
                std::thread::sleep(std::time::Duration::from_millis(20));
                let (r, b) = (ring.rate.load(Relaxed), s.blocking.load(Relaxed));
                let mut fresh = s.reset.swap(false, Relaxed) || (r, b) != (rate, blocking);
                if fresh {
                    (rate, blocking) = (r, b);
                    m = Meter::new(rate, blocking);
                }
                if s.reset_clip.swap(false, Relaxed) {
                    m.reset_peaks();
                    fresh = true;
                }
                buf.clear();
                ring.take(&mut buf);
                // A NaN frame starts a playback session: the meter starts again there.
                let from = buf.chunks_exact(2).rposition(|f| f[0].is_nan()).map_or(0, |i| {
                    m = Meter::new(rate, blocking);
                    fresh = true;
                    2 * i + 2
                });
                m.feed(&buf[from..]);
                dirty |= from < buf.len();
                if fresh || (dirty && shown.elapsed() >= std::time::Duration::from_millis(100)) {
                    *s.readings.lock().unwrap_or_else(|p| p.into_inner()) = m.readings();
                    (dirty, shown) = (false, std::time::Instant::now());
                    ctx.request_repaint();
                }
            }
        });
        shared
    }
}

/// The window: statistics on the left, the meters on the right; its right-click menu is Options › Loudness Meters.
pub fn loudness_meters(ui: &mut Ui, l: &mut Loudness, ring: &Arc<Ring>, p: &LoudPrefs) {
    let s = l.shared.get_or_insert_with(|| Loudness::start(ui.ctx(), ring.clone(), p.blocking));
    s.blocking.store(p.blocking, Relaxed);
    let r = *s.readings.lock().unwrap_or_else(|e| e.into_inner());
    let t = theme::get();
    let rect = ui.max_rect();
    let resp = ui.interact(rect, ui.id().with("loudness_meters"), Sense::click());
    menus::context(&resp, |ui| menus::show(ui, menus::LOUDNESS));
    let painter = ui.painter_at(rect);
    let (font, small) = (t.font(t.font.ui), t.font(t.font.meter));
    let target = p.mode.target();
    // Loudness in the scale's units: LU from the target, or LUFS with Absolute.
    let unit = if p.absolute { "LUFS" } else { "LU" };
    let shown = |v: f64| if p.absolute { v } else { v - target };
    let num = |v: f64| if v.is_finite() { format!("{v:.1}") } else { "-Inf.".to_string() };
    let tp = r.true_peak[0].max(r.true_peak[1]);
    let over_i = r.integrated.is_finite() && r.integrated > target + 0.05;
    let over_tp = tp > p.mode.max_true_peak();

    // Statistics.
    let stats_w = 150.0f32.min(rect.width() * 0.45);
    let rows: [(&str, String, &str, Option<bool>); 5] = [
        ("Momentary", num(shown(r.momentary)), unit, None),
        ("Short", num(shown(r.short_term)), unit, None),
        ("Integrated", num(shown(r.integrated)), unit, Some(over_i)),
        ("LRA", num(r.range), "LU", None),
        ("True Peak", num(tp), "dB FS", Some(over_tp)),
    ];
    for (k, (label, value, unit, lamp)) in rows.iter().enumerate() {
        let top = rect.min.y + 6.0 + 38.0 * k as f32;
        if top + 34.0 > rect.max.y {
            break;
        }
        painter.text(pos2(rect.min.x + 8.0, top), Align2::LEFT_TOP, *label, small.clone(), t.text_dim);
        let bx = Rect::from_min_size(pos2(rect.min.x + 8.0, top + 13.0), vec2(stats_w - 16.0, 20.0));
        painter.rect_filled(bx, 2.0, t.meter_bg);
        painter.text(bx.right_center() - vec2(38.0, 0.0), Align2::RIGHT_CENTER, value, font.clone(), t.text);
        painter.text(bx.right_center() - vec2(34.0, 0.0), Align2::LEFT_CENTER, *unit, small.clone(), t.text_dim);
        if let Some(on) = lamp {
            let c = pos2(bx.min.x + 9.0, bx.center().y);
            painter.circle_filled(c, 4.0, if *on { t.meter_red } else { t.text_dim.gamma_multiply(0.35) });
        }
    }

    // Meters.
    let area = Rect::from_min_max(pos2(rect.min.x + stats_w + 8.0, rect.min.y + 8.0), pos2(rect.max.x - 8.0, rect.max.y - if p.labels { 20.0 } else { 6.0 }));
    if area.width() < 40.0 || area.height() < 40.0 {
        return;
    }
    let (lo, hi) = p.scale();
    let ly = |lu: f64| area.max.y - area.height() * ((lu - lo) / (hi - lo)).clamp(0.0, 1.0) as f32;
    let bar_w = 14.0;
    let label_w = if p.labels { 30.0 } else { 0.0 };
    let x0 = area.min.x + label_w;
    let bars = [("M", r.momentary, Some((r.max_momentary, r.min_momentary))), ("S", r.short_term, Some((r.max_short, r.min_short))), ("I", r.integrated, None)];
    for (k, (name, v, holds)) in bars.iter().enumerate() {
        let well = Rect::from_min_size(pos2(x0 + k as f32 * (bar_w + 8.0), area.min.y), vec2(bar_w, area.height()));
        painter.rect_filled(well, 0.0, t.meter_bg);
        if v.is_finite() {
            let lu = v - target;
            let y = ly(lu);
            let y0 = ly(0.0);
            // Green up to the target, yellow above it.
            painter.rect_filled(Rect::from_x_y_ranges(well.x_range(), Rangef::new(y.max(y0), well.max.y)), 0.0, t.meter_green);
            if y < y0 {
                painter.rect_filled(Rect::from_x_y_ranges(well.x_range(), Rangef::new(y, y0)), 0.0, if lu > 3.0 { t.meter_red } else { t.meter_yellow });
            }
        }
        if let Some((max, min)) = holds {
            for (on, h) in [(p.hold_peaks, *max), (p.hold_valleys, *min)] {
                if on && h.is_finite() && (lo..=hi).contains(&(h - target)) {
                    let y = ly(h - target).round();
                    painter.rect_filled(Rect::from_min_max(pos2(well.min.x, y), pos2(well.max.x, y + 2.0)), 0.0, t.text);
                }
            }
        }
        if p.labels {
            painter.text(pos2(well.center().x, area.max.y + 4.0), Align2::CENTER_TOP, *name, small.clone(), t.meter_label);
        }
    }
    // The target across the meters.
    let bars_end = x0 + 3.0 * (bar_w + 8.0) - 8.0;
    painter.hline(Rangef::new(x0 - 3.0, bars_end + 3.0), ly(0.0).round() + 0.5, Stroke::new(1.0_f32, t.accent));
    if p.labels {
        let step = if p.wide { 6.0 } else { 3.0 };
        let mut lu = lo;
        while lu <= hi + 1e-9 {
            let y = ly(lu);
            let text = if p.absolute { format!("{:.0}", lu + target) } else { format!("{lu:+.0}").replace("+0", "0") };
            painter.text(pos2(x0 - 4.0, y), Align2::RIGHT_CENTER, text, small.clone(), t.meter_label);
            lu += step;
        }
    }
    if !p.show_true_peak {
        return;
    }
    // True peaks: L and R on the True Peak Range, a lamp over the mode's maximum.
    let bottom = TP_RANGES[(p.range as usize).min(TP_RANGES.len() - 1)];
    let ty = |db: f64| area.max.y - area.height() * ((db - bottom) / -bottom).clamp(0.0, 1.0) as f32;
    let tx = bars_end + 24.0 + if p.labels { 24.0 } else { 0.0 };
    let v16 = t.look() == UiLook::Vegas16;
    for (c, name) in ["L", "R"].iter().enumerate() {
        let well = Rect::from_min_size(pos2(tx + c as f32 * (bar_w + 4.0), area.min.y), vec2(bar_w, area.height()));
        if well.max.x > rect.max.x {
            break;
        }
        painter.rect_filled(well, 0.0, t.meter_bg);
        let v = r.true_peak[c];
        if v.is_finite() && v > bottom {
            super::master_bus::paint_bar(&painter, well, ty(v.min(0.0)), &|d: f32| ty(d as f64), v16, 1.0);
        }
        if v > p.mode.max_true_peak() {
            painter.rect_filled(Rect::from_min_size(well.min, vec2(bar_w, 4.0)), 0.0, t.meter_red);
        }
        if p.labels {
            painter.text(pos2(well.center().x, area.max.y + 4.0), Align2::CENTER_TOP, *name, small.clone(), t.meter_label);
        }
    }
    if p.labels {
        let step = (-bottom / 6.0).round().max(1.0);
        let mut db = 0.0;
        while db >= bottom - 1e-9 {
            painter.text(pos2(tx - 4.0, ty(db)), Align2::RIGHT_CENTER, format!("{db:.0}"), small.clone(), t.meter_label);
            db -= step;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Old preferences load with VEGAS's defaults; every option toggles and its menu mark follows.
    #[test]
    fn options_follow_the_menu_and_old_prefs_load() {
        let p: LoudPrefs = serde_json::from_str("{}").unwrap();
        assert_eq!(p, LoudPrefs::default());
        assert_eq!((p.checked(LoudCmd::Mode(Mode::Ebu)), p.checked(LoudCmd::Range(3)), p.checked(LoudCmd::Labels)), (Some(true), Some(true), Some(true)));
        assert_eq!(p.checked(LoudCmd::ResetClip), None);
        let mut q = p;
        for c in [LoudCmd::Mode(Mode::Atsc), LoudCmd::BlockingFilter, LoudCmd::Range(7), LoudCmd::Wide(true), LoudCmd::Absolute, LoudCmd::ShowTruePeak, LoudCmd::Labels, LoudCmd::HoldPeaks, LoudCmd::HoldValleys] {
            q.apply(c);
            assert_eq!(q.checked(c), Some(c != LoudCmd::Labels), "{c:?}");
        }
        assert_eq!((q.mode.target(), q.scale(), TP_RANGES[q.range as usize]), (-24.0, (-36.0, 18.0), -138.0));
        q.apply(LoudCmd::Range(99));
        assert_eq!(q.range, 7);
    }
}
