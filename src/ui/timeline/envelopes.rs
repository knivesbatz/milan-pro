//! Envelopes on the lanes (§5.9.5): track envelopes across the lane, and on video events their Velocity envelope and
//! their transitions' Transition Progress, each over its own span. The line and its points, hit-testing and the value
//! text. The lane mapping and the point edits are in `edit` (`lane_frac`, `env_insert`, ...).
use std::borrow::Cow;
use eframe::egui::*;
use crate::edit::{TRANSITION_OUT, env_fits, lane_frac, transition_span};
use crate::project::*;
use crate::settings::EnvView;
use crate::ui::EnvSel;
use crate::ui::theme::Theme;
use super::{Geo, Row};

/// What the pointer is on: envelope `kind` of `owner` (a track or event id, as `edit::envelope_of` takes it) on the
/// track in `row`, a point on it or else its line, and the point that starts the segment there.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct EnvHit {
    pub is_video: bool,
    pub track: usize,
    pub owner: String,
    pub kind: EnvKind,
    pub point: Option<usize>,
    pub seg: usize,
}

/// An envelope as drawn: key time k sits at timeline time `start + k * scale`, and the line covers `from..to`.
pub(super) struct Shown<'a> {
    pub owner: String,
    pub env: Cow<'a, Envelope>,
    pub start: f64,
    pub scale: f64,
    pub from: f64,
    pub to: f64,
    /// On a locked event: drawn, not edited.
    pub locked: bool,
}

impl Shown<'_> {
    pub fn x_of(&self, g: &Geo, k: f64) -> f32 {
        g.to_x(self.start + k * self.scale)
    }

    /// Key time at timeline time `t`.
    pub fn key_t(&self, t: f64) -> f64 {
        (t - self.start) / self.scale
    }

    /// The value at timeline time `t`.
    pub fn at(&self, t: f64) -> f32 {
        self.env.at(self.key_t(t))
    }
}

/// The envelopes of track `tr` the timeline draws and edits (the ones the engine plays, and not hidden by View ›
/// Envelopes), in drawing order: the track's own, its FX envelopes (R8c), then those of its events that reach into
/// `range` (R8c: an audio event's Volume, Pan and Mute).
pub(super) fn shown(tr: &Track, hidden: u16, range: (f64, f64)) -> Vec<Shown<'_>> {
    let video = tr.kind == TrackKind::Video;
    let visible = |kind| EnvView::of(kind, video).is_none_or(|v| hidden & v.bit() == 0);
    let whole = |owner: String, env| Shown { owner, env, start: 0.0, scale: 1.0, from: f64::NEG_INFINITY, to: f64::INFINITY, locked: false };
    let mut out: Vec<Shown> = tr.envelopes.iter().filter(|e| !e.points.keys.is_empty() && env_fits(e.kind, video) && visible(e.kind)).map(|e| whole(tr.id.clone(), Cow::Borrowed(e))).collect();
    // R8c: FX envelopes; a video track's keyed FX parameters with View › Show OFX Effect Envelopes.
    if !video || hidden & crate::settings::OFX_SHOWN != 0 {
        out.extend(crate::edit::chain_envelopes(&tr.id, &tr.fx, video).into_iter().map(|(o, e)| whole(o, Cow::Owned(e))));
    }
    let reach = |c: &&Clip| c.position <= range.1 && c.position + c.duration >= range.0;
    if !video {
        for c in tr.clips.iter().filter(reach) {
            for e in c.envelopes.iter().filter(|e| matches!(e.kind, EnvKind::Volume | EnvKind::Pan | EnvKind::Mute) && !e.points.keys.is_empty() && visible(e.kind)) {
                out.push(Shown { owner: c.id.clone(), env: Cow::Borrowed(e), start: c.position, scale: 1.0, from: c.position, to: c.position + c.duration, locked: c.locked });
            }
        }
        return out;
    }
    for c in tr.clips.iter().filter(reach) {
        let end = c.position + c.duration;
        if let Some(e) = crate::engine::velocity::of(c).filter(|_| visible(EnvKind::Velocity)) {
            out.push(Shown { owner: c.id.clone(), env: Cow::Borrowed(e), start: c.position, scale: 1.0, from: c.position, to: end, locked: c.locked });
        }
        if !visible(EnvKind::TransitionProgress) {
            continue;
        }
        for (tr_, out_) in [(&c.transition, false), (&c.transition_out, true)] {
            let Some(t) = tr_.as_ref().filter(|t| !t.progress.is_empty()) else { continue };
            let Some((start, len)) = transition_span(&tr.clips, c, out_) else { continue };
            let env = Cow::Owned(Envelope { kind: EnvKind::TransitionProgress, points: t.progress.clone() });
            let owner = if out_ { format!("{}{TRANSITION_OUT}", c.id) } else { c.id.clone() };
            out.push(Shown { owner, env, start, scale: len, from: start, to: start + len, locked: c.locked });
        }
    }
    out
}

/// The lane's usable height: inside the row, clear of the separator.
fn span(row: &Row) -> (f32, f32) {
    (row.top + 2.0, row.top + row.h - 4.0)
}

pub(super) fn y_of(row: &Row, kind: EnvKind, v: f32) -> f32 {
    let (top, bottom) = span(row);
    bottom - lane_frac(kind, v) * (bottom - top)
}

/// Lane height of `dy` pixels, for drags.
pub(super) fn frac_of(row: &Row, dy: f32) -> f32 {
    let (top, bottom) = span(row);
    dy / (bottom - top).max(1.0)
}

/// The time range the lanes show.
fn view(g: &Geo) -> (f64, f64) {
    (g.to_time(g.lanes.min.x), g.to_time(g.lanes.max.x))
}

/// Points within 4 px, then lines within 3 px; the envelope drawn last is on top. Locked events' envelopes are skipped.
pub(super) fn hit(g: &Geo, rows: &[Row], project: &Project, p: Pos2, hidden: u16) -> Option<EnvHit> {
    if !g.lanes.contains(p) {
        return None;
    }
    let row = rows.iter().find(|r| p.y >= r.top && p.y < r.top + r.h)?;
    let tr = &project.timeline.tracks(row.is_video)[row.idx];
    let envs: Vec<Shown> = shown(tr, hidden, view(g)).into_iter().filter(|s| !s.locked).collect();
    let at = |s: &Shown, point: Option<usize>, seg: usize| EnvHit { is_video: row.is_video, track: row.idx, owner: s.owner.clone(), kind: s.env.kind, point, seg };
    for s in envs.iter().rev() {
        if let Some(k) = s.env.points.keys.iter().position(|k| (p.x - s.x_of(g, k.t)).abs() <= 4.0 && (p.y - y_of(row, s.env.kind, k.v)).abs() <= 4.0) {
            return Some(at(s, Some(k), k));
        }
    }
    let t = g.to_time(p.x);
    envs.iter().rev().filter(|s| t >= s.from && t <= s.to).find(|s| (p.y - y_of(row, s.env.kind, s.at(t))).abs() <= 3.0).map(|s| {
        let k = s.key_t(t);
        at(s, None, s.env.points.keys.partition_point(|x| x.t <= k).saturating_sub(1))
    })
}

/// VP16 default envelope colours (spec §2) of the kinds without a theme colour, the same in both looks.
const VELOCITY: Color32 = Color32::from_rgb(0x6A, 0xD3, 0x7F);
const FADE_TO_COLOR: Color32 = Color32::from_rgb(0xFF, 0x45, 0x3F);
// shortcut: VEGAS's FX envelope colour was not measured; amber keeps it apart from the others. Use it once measured.
pub(super) const FX_PARAM: Color32 = Color32::from_rgb(0xD3, 0x9F, 0x3F);

fn color(t: &Theme, kind: EnvKind) -> Color32 {
    match kind {
        EnvKind::Pan => t.envelope_pan,
        EnvKind::Mute => t.envelope_mute,
        EnvKind::CompositeLevel => t.envelope_composite,
        EnvKind::Velocity => VELOCITY,
        EnvKind::FadeToColor => FADE_TO_COLOR,
        EnvKind::TransitionProgress => super::TRANSITION,
        EnvKind::FxParam => FX_PARAM,
        _ => t.envelope_volume,
    }
}

/// Every shown envelope on the visible rows: the line (sampled every 2 px and at each point; 2 px wide when
/// hovered) and 5×5 points in its colour with a white edge, selected ones filled white. Video rows also get the Track
/// Keyframes row: a mark at the bottom of the lane for each Track Motion and track FX key (View › Video Envelopes).
#[allow(clippy::too_many_arguments)]
pub(super) fn paint(p: &Painter, t: &Theme, g: &Geo, rows: &[Row], project: &Project, sel: Option<&EnvSel>, hover: Option<&EnvHit>, hidden: u16) {
    for row in rows.iter().filter(|r| r.top < g.lanes.max.y && r.top + r.h > g.lanes.min.y) {
        let tr = &project.timeline.tracks(row.is_video)[row.idx];
        let lane = Rect::from_x_y_ranges(g.lanes.x_range(), Rangef::new(row.top, row.top + row.h - 2.0)).intersect(g.lanes);
        let p = p.with_clip_rect(lane);
        if row.is_video && hidden & EnvView::TrackKeyframes.bit() == 0 {
            let halos = [&tr.shadow, &tr.glow].into_iter().filter(|h| h.on).flat_map(|h| h.keys.times());
            let keys = tr.motion.keys.iter().chain(&tr.parent_motion.keys).map(|k| k.t).chain(halos).chain(tr.fx.iter().flat_map(|f| f.params.values()).flat_map(|k| k.keys.iter().map(|k| k.t)));
            let y = row.top + row.h - 7.0;
            for x in keys.map(|k| g.to_x(k).round()).filter(|x| *x >= lane.min.x - 4.0 && *x <= lane.max.x + 4.0) {
                let d = [pos2(x, y - 3.0), pos2(x + 3.0, y), pos2(x, y + 3.0), pos2(x - 3.0, y)];
                p.add(Shape::convex_polygon(d.to_vec(), t.text_dim, Stroke::new(1.0_f32, t.envelope_point)));
            }
        }
        for s in shown(tr, hidden, view(g)) {
            let kind = s.env.kind;
            let c = color(t, kind);
            // The selected envelope (E / Shift+E pick an FX envelope) is drawn as a hovered one.
            let hovered = hover.is_some_and(|h| (h.is_video, h.track, &h.owner, h.kind) == (row.is_video, row.idx, &s.owner, kind)) || sel.is_some_and(|e| e.track == s.owner && e.kind == kind);
            let (x0, x1) = (lane.min.x.max(g.to_x(s.from.max(-1e12))), lane.max.x.min(g.to_x(s.to.min(1e12))));
            if x1 < x0 {
                continue;
            }
            let mut xs: Vec<f32> = (0..).map(|k| x0 + 2.0 * k as f32).take_while(|x| *x < x1).chain([x1]).collect();
            xs.extend(s.env.points.keys.iter().map(|k| s.x_of(g, k.t)).filter(|x| *x > x0 && *x < x1));
            xs.sort_by(f32::total_cmp);
            let pts: Vec<Pos2> = xs.iter().map(|&x| pos2(x, y_of(row, kind, s.at(g.to_time(x))))).collect();
            p.add(Shape::line(pts, Stroke::new(if hovered { 2.0_f32 } else { 1.0 }, c)));
            let picked = sel.filter(|e| e.track == s.owner && e.kind == kind);
            for (k, key) in s.env.points.keys.iter().enumerate() {
                let x = s.x_of(g, key.t);
                if x < lane.min.x - 4.0 || x > lane.max.x + 4.0 {
                    continue;
                }
                let r = Rect::from_center_size(pos2(x.round(), y_of(row, kind, key.v).round()), vec2(5.0, 5.0));
                let fill = if picked.is_some_and(|e| e.keys.contains(&k)) { t.envelope_point } else { c };
                p.rect_filled(r, 0.0, fill);
                p.rect_stroke(r, 0.0, Stroke::new(1.0_f32, t.envelope_point), StrokeKind::Outside);
            }
        }
    }
}

/// The tooltip text for a value: "-3.2 dB", "25.0% L", "Composite Level 75.0 %", "Velocity 50.0 %".
pub(super) fn value_text(kind: EnvKind, v: f32) -> String {
    match kind {
        EnvKind::Volume if v <= -120.0 => "-Inf dB".into(),
        EnvKind::Volume => format!("{v:.1} dB"),
        EnvKind::Pan if v.abs() < 0.0005 => "Center".into(),
        EnvKind::Pan => format!("{:.1}% {}", v.abs() * 100.0, if v < 0.0 { "L" } else { "R" }),
        EnvKind::Mute => (if v >= 0.5 { "Unmuted" } else { "Muted" }).into(),
        EnvKind::CompositeLevel => format!("Composite Level {:.1} %", v * 100.0),
        EnvKind::Velocity => format!("Velocity {:.1} %", v * 100.0),
        EnvKind::TransitionProgress => format!("Progress {:.1} %", v * 100.0),
        EnvKind::FadeToColor if v.abs() < 0.0005 => "No Fade".into(),
        EnvKind::FadeToColor => format!("{:.1}% {}", v.abs() * 100.0, if v < 0.0 { "Bottom" } else { "Top" }),
        EnvKind::MotionBlur => format!("Motion Blur {v:.3} s"),
        EnvKind::Supersampling => format!("Supersampling {}", v.round()),
        _ => format!("{:.1} %", v * 100.0),
    }
}

/// R8c: `value_text`, with an FX envelope's value in its parameter's units ("Gain (dB) -6.00", "High-Pass").
pub(super) fn value_label(project: &Project, owner: &str, kind: EnvKind, v: f32) -> String {
    let fx = (kind == EnvKind::FxParam).then(|| crate::edit::fx_value_text(project, owner, v)).flatten();
    fx.unwrap_or_else(|| value_text(kind, v))
}

/// The point menu's preset values (§6.7: two above the default, four below), high to low, evenly spaced in the lane
/// and rounded to a tenth of the unit shown; none for Mute (on or off) or where the default is at the lane's end.
// shortcut: VEGAS's exact preset values are unknown (the spec only counts them); use them if they are measured.
pub(super) fn presets(kind: EnvKind) -> (Vec<f32>, Vec<f32>) {
    if kind == EnvKind::Mute {
        return (Vec::new(), Vec::new());
    }
    let d = lane_frac(kind, crate::edit::env_default(kind));
    let round = |f: f32| {
        let v = crate::edit::lane_value(kind, f);
        if kind == EnvKind::Volume { (v * 10.0).round() / 10.0 } else { (v * 1000.0).round() / 1000.0 }
    };
    let above = if d < 1.0 { [2.0, 1.0].map(|k| round(d + (1.0 - d) * k / 3.0)).to_vec() } else { Vec::new() };
    let below = if d > 0.0 { [4.0, 3.0, 2.0, 1.0].map(|k| round(d * k / 5.0)).to_vec() } else { Vec::new() };
    (above, below)
}

/// A value typed in the units `value_text` shows (dB, % with L or Bottom negative, Composite Level %); None if not a
/// number.
pub(super) fn parse_value(kind: EnvKind, s: &str) -> Option<f32> {
    let s = s.trim().to_ascii_lowercase();
    if kind == EnvKind::Volume && s.contains("inf") {
        return Some(-120.0);
    }
    let left = s.ends_with('l') || s.ends_with("bottom");
    let num = |c: &char| c.is_ascii_digit() || matches!(c, '-' | '+' | '.');
    let n: f32 = s.chars().skip_while(|c| !num(c)).take_while(num).collect::<String>().parse().ok()?;
    Some(match kind {
        EnvKind::Volume => n,
        EnvKind::Pan | EnvKind::FadeToColor => (if left { -n.abs() } else { n }) / 100.0,
        EnvKind::Mute | EnvKind::MotionBlur | EnvKind::Supersampling => n,
        _ => n / 100.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::{env_range, lane_value};

    #[test]
    fn lane_mapping_round_trips() {
        for kind in [EnvKind::Volume, EnvKind::Pan, EnvKind::Mute, EnvKind::CompositeLevel, EnvKind::Velocity, EnvKind::FadeToColor, EnvKind::TransitionProgress] {
            let (lo, hi) = env_range(kind);
            for k in 0..=20 {
                let v = lo + (hi - lo) * k as f32 / 20.0;
                assert!((lane_value(kind, lane_frac(kind, v)) - v).abs() < 1e-4, "{kind:?} {v}");
            }
        }
        assert!((lane_frac(EnvKind::Volume, 0.0) - 0.7).abs() < 1e-6);
        assert!((lane_frac(EnvKind::Volume, 6.0) - 1.0).abs() < 1e-6);
        assert!(lane_frac(EnvKind::Volume, -120.0) < 1e-3);
        assert_eq!(lane_frac(EnvKind::Pan, 0.0), 0.5);
        assert_eq!((lane_frac(EnvKind::CompositeLevel, 1.0), lane_frac(EnvKind::Mute, 0.0)), (1.0, 0.0));
        // Velocity: 100% in the middle, 0% a quarter up, -100% and 1000% at the ends; no fade in the middle.
        assert_eq!([-1.0, 0.0, 1.0, 10.0].map(|v| lane_frac(EnvKind::Velocity, v)), [0.0, 0.25, 0.5, 1.0]);
        assert_eq!(lane_frac(EnvKind::FadeToColor, 0.0), 0.5);
    }

    #[test]
    fn value_text_and_parse_agree() {
        assert_eq!(value_text(EnvKind::Volume, -3.24), "-3.2 dB");
        assert_eq!(value_text(EnvKind::Pan, -0.25), "25.0% L");
        assert_eq!(value_text(EnvKind::CompositeLevel, 0.75), "Composite Level 75.0 %");
        assert_eq!(parse_value(EnvKind::Volume, "-3.5 dB"), Some(-3.5));
        assert_eq!(parse_value(EnvKind::Pan, "25% L"), Some(-0.25));
        assert_eq!(parse_value(EnvKind::Pan, "40"), Some(0.4));
        assert_eq!(parse_value(EnvKind::CompositeLevel, "75 %"), Some(0.75));
        assert_eq!(parse_value(EnvKind::CompositeLevel, &value_text(EnvKind::CompositeLevel, 0.5)), Some(0.5));
        assert_eq!(parse_value(EnvKind::Volume, "-Inf dB"), Some(-120.0));
        assert_eq!(parse_value(EnvKind::Volume, "loud"), None);
        for (kind, v) in [(EnvKind::Velocity, 2.5), (EnvKind::Velocity, -0.5), (EnvKind::FadeToColor, -0.25), (EnvKind::FadeToColor, 0.75), (EnvKind::TransitionProgress, 0.25)] {
            assert_eq!(parse_value(kind, &value_text(kind, v)), Some(v), "{kind:?} {}", value_text(kind, v));
        }
        assert_eq!(value_text(EnvKind::FadeToColor, -0.25), "25.0% Bottom");
    }

    #[test]
    fn point_presets_sit_between_the_ends_and_the_default() {
        assert_eq!(presets(EnvKind::Volume), (vec![4.0, 2.0], vec![-3.9, -8.9, -15.9, -28.0]));
        assert_eq!(presets(EnvKind::CompositeLevel), (vec![], vec![0.8, 0.6, 0.4, 0.2]));
        let (above, below) = presets(EnvKind::Pan);
        assert!(above.len() == 2 && above.iter().all(|v| *v < 0.0) && below.iter().all(|v| *v > 0.0), "left is up");
        assert_eq!(presets(EnvKind::Mute), (vec![], vec![]));
    }

    /// Velocity on events and Transition Progress on transitions are drawn over their own spans and can be hidden.
    #[test]
    fn event_envelopes_show_over_their_spans() {
        let mut tr = Track::new("V", TrackKind::Video);
        let mut c = Clip::new("c", 10.0, 4.0);
        c.envelopes.push(Envelope { kind: EnvKind::Velocity, points: Keyframes { keys: vec![Keyframe { t: 0.0, v: 1.0, curve: Curve::Linear }] } });
        let p = Keyframes { keys: vec![Keyframe { t: 0.0, v: 0.0, curve: Curve::Linear }, Keyframe { t: 1.0, v: 1.0, curve: Curve::Linear }] };
        (c.fade_out, c.transition_out) = (2.0, Some(Transition { plugin: "milan.tr.dissolve".into(), progress: p, ..Default::default() }));
        let id = c.id.clone();
        tr.clips.push(c);
        let all = shown(&tr, 0, (0.0, 100.0));
        let spans: Vec<(String, EnvKind, f64, f64)> = all.iter().map(|s| (s.owner.clone(), s.env.kind, s.from, s.to)).collect();
        assert_eq!(spans, [(id.clone(), EnvKind::Velocity, 10.0, 14.0), (format!("{id}{TRANSITION_OUT}"), EnvKind::TransitionProgress, 12.0, 14.0)]);
        assert!((all[1].at(13.0) - 0.5).abs() < 1e-6, "half way through the 2 s transition");
        assert_eq!(shown(&tr, EnvView::Velocity.bit(), (0.0, 100.0)).len(), 1);
        assert!(shown(&tr, 0, (20.0, 30.0)).is_empty(), "off screen");
    }
}
