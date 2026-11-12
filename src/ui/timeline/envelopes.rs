//! Track envelopes on the lanes (§5.9.5): the line and its points, hit-testing and the value text. The lane mapping
//! and the point edits are in `edit` (`lane_frac`, `env_insert`, ...).
use eframe::egui::*;
use crate::edit::{env_fits, lane_frac};
use crate::project::*;
use crate::ui::EnvSel;
use crate::ui::theme::Theme;
use super::{Geo, Row};

/// What the pointer is on: envelope `env` (an index into the track's envelopes) of the track in `row`, a point on it
/// or else its line, and the point that starts the segment there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct EnvHit {
    pub is_video: bool,
    pub track: usize,
    pub env: usize,
    pub point: Option<usize>,
    pub seg: usize,
}

/// The track's envelopes drawn and edited (the ones the engine plays), in drawing order.
pub(super) fn shown(tr: &Track) -> impl Iterator<Item = (usize, &Envelope)> {
    let video = tr.kind == TrackKind::Video;
    tr.envelopes.iter().enumerate().filter(move |(_, e)| !e.points.keys.is_empty() && env_fits(e.kind, video))
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

/// Points within 4 px, then lines within 3 px; the envelope drawn last is on top.
pub(super) fn hit(g: &Geo, rows: &[Row], project: &Project, p: Pos2) -> Option<EnvHit> {
    if !g.lanes.contains(p) {
        return None;
    }
    let row = rows.iter().find(|r| p.y >= r.top && p.y < r.top + r.h)?;
    let tr = &project.timeline.tracks(row.is_video)[row.idx];
    let envs: Vec<(usize, &Envelope)> = shown(tr).collect();
    let at = |env: usize, point: Option<usize>, seg: usize| EnvHit { is_video: row.is_video, track: row.idx, env, point, seg };
    for &(i, e) in envs.iter().rev() {
        if let Some(k) = e.points.keys.iter().position(|k| (p.x - g.to_x(k.t)).abs() <= 4.0 && (p.y - y_of(row, e.kind, k.v)).abs() <= 4.0) {
            return Some(at(i, Some(k), k));
        }
    }
    let t = g.to_time(p.x);
    envs.iter().rev().find(|(_, e)| (p.y - y_of(row, e.kind, e.at(t))).abs() <= 3.0).map(|&(i, e)| at(i, None, e.points.keys.partition_point(|k| k.t <= t).saturating_sub(1)))
}

fn color(t: &Theme, kind: EnvKind) -> Color32 {
    match kind {
        EnvKind::Pan => t.envelope_pan,
        EnvKind::Mute => t.envelope_mute,
        EnvKind::CompositeLevel => t.envelope_composite,
        _ => t.envelope_volume,
    }
}

/// Every shown envelope on the visible rows: the line (sampled every 2 px and at each point; 2 px wide when
/// hovered) and 5×5 points in its colour with a white edge, selected ones filled white.
pub(super) fn paint(p: &Painter, t: &Theme, g: &Geo, rows: &[Row], project: &Project, sel: Option<&EnvSel>, hover: Option<EnvHit>) {
    for row in rows.iter().filter(|r| r.top < g.lanes.max.y && r.top + r.h > g.lanes.min.y) {
        let tr = &project.timeline.tracks(row.is_video)[row.idx];
        let lane = Rect::from_x_y_ranges(g.lanes.x_range(), Rangef::new(row.top, row.top + row.h - 2.0)).intersect(g.lanes);
        let p = p.with_clip_rect(lane);
        for (i, e) in shown(tr) {
            let c = color(t, e.kind);
            let hovered = hover.is_some_and(|h| (h.is_video, h.track, h.env) == (row.is_video, row.idx, i));
            let mut xs: Vec<f32> = (0..).map(|k| lane.min.x + 2.0 * k as f32).take_while(|x| *x <= lane.max.x + 2.0).collect();
            xs.extend(e.points.keys.iter().map(|k| g.to_x(k.t)).filter(|x| *x > lane.min.x && *x < lane.max.x));
            xs.sort_by(f32::total_cmp);
            let pts: Vec<Pos2> = xs.iter().map(|&x| pos2(x, y_of(row, e.kind, e.at(g.to_time(x))))).collect();
            p.add(Shape::line(pts, Stroke::new(if hovered { 2.0_f32 } else { 1.0 }, c)));
            let picked = sel.filter(|s| s.track == tr.id && s.kind == e.kind);
            for (k, key) in e.points.keys.iter().enumerate() {
                let x = g.to_x(key.t);
                if x < lane.min.x - 4.0 || x > lane.max.x + 4.0 {
                    continue;
                }
                let r = Rect::from_center_size(pos2(x.round(), y_of(row, e.kind, key.v).round()), vec2(5.0, 5.0));
                let fill = if picked.is_some_and(|s| s.keys.contains(&k)) { t.envelope_point } else { c };
                p.rect_filled(r, 0.0, fill);
                p.rect_stroke(r, 0.0, Stroke::new(1.0_f32, t.envelope_point), StrokeKind::Outside);
            }
        }
    }
}

/// The tooltip text for a value: "-3.2 dB", "25.0% L", "Composite Level 75.0 %".
pub(super) fn value_text(kind: EnvKind, v: f32) -> String {
    match kind {
        EnvKind::Volume if v <= -120.0 => "-Inf dB".into(),
        EnvKind::Volume => format!("{v:.1} dB"),
        EnvKind::Pan if v.abs() < 0.0005 => "Center".into(),
        EnvKind::Pan => format!("{:.1}% {}", v.abs() * 100.0, if v < 0.0 { "L" } else { "R" }),
        EnvKind::Mute => (if v >= 0.5 { "Unmuted" } else { "Muted" }).into(),
        EnvKind::CompositeLevel => format!("Composite Level {:.1} %", v * 100.0),
        _ => format!("{:.1} %", v * 100.0),
    }
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

/// A value typed in the units `value_text` shows (dB, % with L negative, Composite Level %); None if not a number.
pub(super) fn parse_value(kind: EnvKind, s: &str) -> Option<f32> {
    let s = s.trim().to_ascii_lowercase();
    if kind == EnvKind::Volume && s.contains("inf") {
        return Some(-120.0);
    }
    let left = s.ends_with('l');
    let num = |c: &char| c.is_ascii_digit() || matches!(c, '-' | '+' | '.');
    let n: f32 = s.chars().skip_while(|c| !num(c)).take_while(num).collect::<String>().parse().ok()?;
    Some(match kind {
        EnvKind::Volume => n,
        EnvKind::Pan => (if left { -n.abs() } else { n }) / 100.0,
        EnvKind::Mute => n,
        _ => n / 100.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::{env_range, lane_value};

    #[test]
    fn lane_mapping_round_trips() {
        for kind in [EnvKind::Volume, EnvKind::Pan, EnvKind::Mute, EnvKind::CompositeLevel] {
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
    }

    #[test]
    fn point_presets_sit_between_the_ends_and_the_default() {
        assert_eq!(presets(EnvKind::Volume), (vec![4.0, 2.0], vec![-3.9, -8.9, -15.9, -28.0]));
        assert_eq!(presets(EnvKind::CompositeLevel), (vec![], vec![0.8, 0.6, 0.4, 0.2]));
        let (above, below) = presets(EnvKind::Pan);
        assert!(above.len() == 2 && above.iter().all(|v| *v < 0.0) && below.iter().all(|v| *v > 0.0), "left is up");
        assert_eq!(presets(EnvKind::Mute), (vec![], vec![]));
    }
}
