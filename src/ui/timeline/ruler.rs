//! Band above the lanes (§5.9.1–5.9.2, §5.9.6, §5.9.8): time display, loop bar with yellow flags, marker bar, R18a's
//! CD layout bar and command bar, timecode ruler with grid lines, selection overlay and the per-look playhead.
use eframe::egui::*;
use crate::edit;
use crate::history::History;
use crate::project::*;
use crate::ui::theme::{self, Theme, UiLook};
use super::{Geo, Rename, TlState, SNAP_PX};

/// Time display at the top of the track list (§5.9.1): gripper dots, cursor timecode right-aligned with 8 px padding.
/// `crumbs`: R6b's breadcrumb runs along the top, so the time sits lower.
pub(super) fn draw_time_display(p: &Painter, r: Rect, project: &Project, crumbs: bool) {
    let t = theme::get();
    p.rect_filled(r, 0.0, t.time_display_bg);
    let v16 = t.look() == UiLook::Vegas16;
    if v16 {
        let b = &t.bevel;
        let line = |y: f32, c| p.rect_filled(Rect::from_x_y_ranges(r.x_range(), Rangef::new(y, y + 1.0)), 0.0, c);
        line(r.min.y, b.time_display_1);
        line(r.min.y + 1.0, b.time_display_2);
        line(r.max.y - 1.0, b.time_display_bottom);
    }
    // Gripper: one column of 2 px dots on a 5 px pitch from the top (refs: VP16 ~8 dark dots, dark2026 6 light dots).
    let (dot, n) = if v16 { (t.line_dark, 8) } else { (t.ruler_text, 6) };
    let cy = r.center().y.round();
    for k in 0..n {
        let d = Rect::from_min_size(pos2(r.min.x + 3.0, r.min.y + 4.0 + 5.0 * k as f32), vec2(2.0, 2.0));
        p.rect_filled(d, 0.0, dot);
    }
    let font = if t.font.time_display_semibold { t.semibold(t.font.time_display) } else { t.font(t.font.time_display) };
    let tc = project.settings.ruler_time(project.timeline.cursor_position);
    let y = if crumbs { cy + 7.0 } else { cy };
    p.with_clip_rect(r).text(pos2(r.max.x - 8.0, y), Align2::RIGHT_CENTER, tc, font, t.time_display_text);
}

/// (loop bar, marker bar, ruler) rows of the band: VP16 loop 2–6 · markers 8–21 · ruler 31–46 · line 47;
/// VP23 markers 14 · loop 8, then a dark line and a 14 px ruler standing on the bottom line (dark2026). The ruler stands
/// on the band's bottom, under the CD layout and command bars when they are shown (`band_h`).
pub(super) fn band_rows(t: &Theme, band: Rect) -> (Rect, Rect, Rect) {
    let row = |y0: f32, h: f32| Rect::from_x_y_ranges(band.x_range(), Rangef::new(band.min.y + y0, band.min.y + y0 + h));
    if t.look() == UiLook::Vegas16 {
        (row(2.0, 5.0), row(8.0, 14.0), row(band.height() - 17.0, 16.0))
    } else {
        let h = band.height();
        (row(14.0, 8.0), row(0.0, 14.0), row(h - 15.0, 14.0))
    }
}

/// R18a: the CD layout bar and the command bar: 14 px rows under the marker bar, each shown while the project has CD
/// track regions or indexes / command markers (VEGAS shows them only then).
const BAR_H: f32 = 14.0;
/// VEGAS's snap colours for these tags (VEGAS_UI_SPEC §2.1): CD Track, CD Index, Command Marker.
const CD_TRACK: Color32 = Color32::from_rgb(0xE4, 0x81, 0x81);
const CD_INDEX: Color32 = Color32::from_rgb(0xCE, 0x81, 0xE4);
const COMMAND: Color32 = Color32::from_rgb(0x59, 0x89, 0xFF);

/// The band's height: the look's, plus the CD layout and command bars that are shown.
pub(super) fn band_h(t: &Theme, tl: &Timeline) -> f32 {
    t.m.band_h + BAR_H * (tl.cd_layout() as u8 + !tl.commands.is_empty() as u8) as f32
}

/// (CD layout bar, command bar) rows of the band, when shown.
pub(super) fn bar_rows(band: Rect, tl: &Timeline) -> (Option<Rect>, Option<Rect>) {
    let row = |k: f32| Rect::from_x_y_ranges(band.x_range(), Rangef::new(band.min.y + 22.0 + BAR_H * k, band.min.y + 22.0 + BAR_H * (k + 1.0)));
    let cd = tl.cd_layout();
    (cd.then(|| row(0.0)), (!tl.commands.is_empty()).then(|| row(cd as u8 as f32)))
}

/// A tag on the CD layout bar or the command bar.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(super) enum BarTag {
    Command(usize),
    /// A CD track region's start tag, or (true) its end tag.
    CdTrack(usize, bool),
    CdIndex(usize),
}

impl BarTag {
    /// Where the tag sits.
    pub(super) fn time(self, tl: &Timeline) -> Option<f64> {
        match self {
            BarTag::Command(i) => tl.commands.get(i).map(|c| c.position),
            BarTag::CdTrack(i, end) => tl.cd_tracks.get(i).map(|c| if end { c.end } else { c.start }),
            BarTag::CdIndex(i) => tl.cd_indexes.get(i).copied(),
        }
    }

    /// What a drag of the tag moves: command markers and indexes (a track region is edited in Edit Details).
    fn pos_mut(self, tl: &mut Timeline) -> Option<&mut f64> {
        match self {
            BarTag::Command(i) => tl.commands.get_mut(i).map(|c| &mut c.position),
            BarTag::CdIndex(i) => tl.cd_indexes.get_mut(i),
            BarTag::CdTrack(..) => None,
        }
    }

    /// The number in the tag: a track region's track number (in time order), an index's number within its track
    /// region (VEGAS counts the track's start as index 1); and the label beside it.
    pub(super) fn text(self, tl: &Timeline) -> (String, String) {
        match self {
            BarTag::CdTrack(i, end) => {
                let Some(c) = tl.cd_tracks.get(i) else { return Default::default() };
                let n = tl.cd_tracks.iter().filter(|o| o.start < c.start).count() + 1;
                (n.to_string(), if end { String::new() } else { c.name.clone() })
            }
            BarTag::CdIndex(i) => {
                let at = tl.cd_indexes.get(i).copied().unwrap_or_default();
                let track = tl.cd_tracks.iter().find(|c| c.start <= at && at < c.end);
                let n = track.map_or(String::new(), |c| (tl.cd_indexes.iter().filter(|&&x| x > c.start && x < at).count() + 2).to_string());
                (n, String::new())
            }
            BarTag::Command(i) => {
                let label = tl.commands.get(i).map_or(String::new(), |c| if c.comment.is_empty() { c.command.clone() } else { format!("{}: {}", c.command, c.comment) });
                (String::new(), label)
            }
        }
    }
}

/// Every tag of the CD layout bar and the command bar, with its rect (13×14; a track region's end tag left of its end).
pub(super) fn bar_tags(g: &Geo, tl: &Timeline) -> Vec<(BarTag, Rect)> {
    let (cd, cmd) = bar_rows(g.band, tl);
    let tag = |r: Rect, x: f32| Rect::from_min_size(pos2(x, r.min.y), vec2(13.0, BAR_H));
    let mut v = Vec::new();
    if let Some(r) = cd {
        for (i, c) in tl.cd_tracks.iter().enumerate() {
            v.push((BarTag::CdTrack(i, false), tag(r, g.to_x(c.start).floor())));
            v.push((BarTag::CdTrack(i, true), tag(r, g.to_x(c.end).floor() - 12.0)));
        }
        v.extend(tl.cd_indexes.iter().enumerate().map(|(i, &at)| (BarTag::CdIndex(i), tag(r, g.to_x(at).floor()))));
    }
    if let Some(r) = cmd {
        v.extend(tl.commands.iter().enumerate().map(|(i, c)| (BarTag::Command(i), tag(r, g.to_x(c.position).floor()))));
    }
    v
}

/// Major tick interval: the first of 1/2/5/10/15 frames, 1/2/5/10/15/30 s, 1/2/5/10/15/30 min, 1 h that puts
/// major ticks ≥ 90 px apart.
pub(super) fn major_interval(pps: f64, fps: f64) -> f64 {
    let f = 1.0 / fps.max(1.0);
    let steps = [f, 2.0 * f, 5.0 * f, 10.0 * f, 15.0 * f, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0, 900.0, 1800.0];
    steps.into_iter().find(|s| s * pps >= 90.0).unwrap_or(3600.0)
}

/// Major tick interval in the ruler's format: Measures & Beats ticks on beats and whole measures, the frame formats
/// on their own frames.
pub(super) fn ruler_interval(pps: f64, s: &ProjectSettings) -> f64 {
    match s.ruler.format {
        RulerFormat::MeasuresBeats => {
            let (beat, per) = (s.beat_secs(), s.ruler.beats_per_measure.max(1) as f64);
            let steps = [beat / 4.0, beat / 2.0, beat].into_iter().chain((0..16).map(|k| beat * per * (1u32 << k) as f64));
            steps.clone().find(|st| st * pps >= 90.0).unwrap_or_else(|| steps.last().unwrap_or(beat))
        }
        f @ (RulerFormat::Samples | RulerFormat::Time | RulerFormat::Seconds) => major_interval(pps, f.frame_rate(s.frame_rate)),
        // A whole number of the format's frames, so at 29.97 the 2 s ticks read 00:00:18;00, not 00:00:17;29.
        f => {
            let rate = f.frame_rate(s.frame_rate);
            (major_interval(pps, rate) * rate.round()).round() / rate
        }
    }
}

/// Grid lines as (step, phase): Ruler Marks follow the ruler's major ticks; the other spacings are thinned to whole
/// multiples (1-2-5, or doubling for musical ones) at least 6 px apart. Lines fall where the ruler time (from its
/// start time) is a multiple of the step.
pub fn grid_line(gs: GridSpacing, s: &ProjectSettings, pps: f64) -> (f64, f64) {
    let step = match gs.base(s) {
        None => ruler_interval(pps, s),
        Some(b) => {
            let musical = matches!(gs, GridSpacing::Measures | GridSpacing::Note(..));
            let mults: &[f64] = if musical { &[1.0, 2.0, 4.0, 8.0] } else { &[1.0, 2.0, 5.0] };
            (0..12).flat_map(|e| mults.iter().map(move |m| m * if musical { 16f64.powi(e) } else { 10f64.powi(e) })).map(|k| b * k).find(|st| st * pps >= 6.0).unwrap_or(b)
        }
    };
    (step, (-s.ruler.offset).rem_euclid(step))
}

/// Minor ticks per major interval so they land 6–8 px apart.
fn minor_count(major_px: f64) -> i64 {
    (major_px / 7.0).round().max(1.0) as i64
}

/// Compact timecode for length tags: "00:00:05;12" -> "5;12".
pub(super) fn short_tc(sec: f64, fps: f64) -> String {
    let tc = format_timecode(sec, fps);
    let cut = tc.char_indices().find(|&(i, c)| c != '0' && c != ':' && i < tc.len() - 3).map_or(tc.len() - 3, |(i, _)| i);
    let s = &tc[cut..];
    if s.starts_with([';', ':']) { format!("0{s}") } else { s.to_string() }
}

/// Loop bar drag in progress.
#[derive(Clone, Copy, Debug)]
pub(super) enum LoopDrag {
    /// New selection from this anchor time.
    Create(f64),
    Start,
    End,
    /// Moving the whole region; grab offset from its start.
    Move(f64),
}

/// Apply a loop drag at pointer time `t`.
pub(super) fn loop_drag_to(tl: &mut Timeline, drag: LoopDrag, t: f64) {
    let t = t.max(0.0);
    let (a, b) = tl.loop_region.unwrap_or((t, t));
    tl.loop_region = Some(match drag {
        LoopDrag::Create(anchor) => (anchor.min(t), anchor.max(t)),
        LoopDrag::Start => (t.min(b), t.max(b)),
        LoopDrag::End => (a.min(t), a.max(t)),
        LoopDrag::Move(g) => {
            let s = (t - g).max(0.0);
            (s, s + (b - a))
        }
    });
}

/// Snap `t` to the project's snap targets unless snapping is off (Shift disables it during a drag).
pub(super) fn snapped(ui: &Ui, project: &Project, t: f64, g: &Geo, snapping: bool, st: &mut TlState) -> f64 {
    if !snapping || ui.input(|i| i.modifiers.shift) {
        return t;
    }
    match g.snaps(edit::snap_targets(project, None)).nearest(t, SNAP_PX / g.pps) {
        Some(s) => {
            st.snap = Some(s);
            s
        }
        None => t,
    }
}

/// Loop bar (create / resize / move / double-click = whole project) and marker tags (click, drag, rename).
pub(super) fn interact(ui: &Ui, g: &Geo, project: &mut Project, history: &mut History, st: &mut TlState, snapping: bool) {
    let t = theme::get();
    let (loop_r, marker_r, _) = band_rows(&t, g.band);
    let fps = project.settings.frame_rate;

    let lr = ui.interact(loop_r.expand2(vec2(0.0, 1.0)), ui.id().with("loop_bar"), Sense::click_and_drag());
    if lr.double_clicked() {
        let end = crate::render::content_end(project);
        project.timeline.loop_region = (end > 0.0).then_some((0.0, end));
    }
    if lr.drag_started() {
        if let Some(o) = ui.input(|i| i.pointer.press_origin()) {
            let t0 = g.to_time(o.x).max(0.0);
            st.loop_drag = Some(match project.timeline.loop_region {
                Some((a, _)) if (o.x - g.to_x(a)).abs() <= 5.0 => LoopDrag::Start,
                Some((_, b)) if (o.x - g.to_x(b)).abs() <= 5.0 => LoopDrag::End,
                Some((a, b)) if t0 > a && t0 < b => LoopDrag::Move(t0 - a),
                _ => LoopDrag::Create(snapped(ui, project, t0, g, snapping, st)),
            });
        }
    }
    if let (Some(drag), Some(p)) = (st.loop_drag, lr.interact_pointer_pos().filter(|_| lr.dragged())) {
        let mut tp = g.to_time(p.x);
        if let LoopDrag::Move(gr) = drag {
            // Snap the moved region by its start.
            tp = snapped(ui, project, tp - gr, g, snapping, st) + gr;
        } else {
            tp = snapped(ui, project, tp, g, snapping, st);
        }
        loop_drag_to(&mut project.timeline, drag, tp);
    }
    if lr.drag_stopped() {
        st.loop_drag = None;
        let frame = 1.0 / fps.max(1.0);
        if project.timeline.loop_region.is_some_and(|(a, b)| b - a < frame) {
            project.timeline.loop_region = None;
        }
    }
    if lr.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
    }

    // Marker tags: click moves the cursor there, drag moves the marker, double-click renames.
    for i in 0..project.timeline.markers.len() {
        let x = g.to_x(project.timeline.markers[i].position);
        if x < g.band.min.x - 13.0 || x > g.band.max.x {
            continue;
        }
        let tag = Rect::from_min_size(pos2(x, marker_r.min.y), vec2(13.0, 14.0));
        let r = ui.interact(tag, ui.id().with(("marker", i)), Sense::click_and_drag());
        if r.double_clicked() {
            st.rename = Some((Rename::Marker(i), project.timeline.markers[i].name.clone()));
        } else if r.clicked() {
            project.timeline.cursor_position = project.timeline.markers[i].position;
        }
        if r.drag_started() {
            history.push(project.clone());
            project.mark_modified();
            let grab = r.interact_pointer_pos().map_or(0.0, |p| p.x - x);
            st.marker_drag = Some((i, grab));
        }
        if let (Some((k, grab)), Some(p)) = (st.marker_drag, r.interact_pointer_pos().filter(|_| r.dragged())) {
            if k == i {
                let raw = g.to_time(p.x - grab).max(0.0);
                let own = project.timeline.markers[i].position;
                let mut targets = edit::snap_targets(project, None);
                targets.retain(|v| (v - own).abs() > 1e-9);
                let pos = if snapping && !ui.input(|i| i.modifiers.shift) {
                    g.snaps(targets).nearest(raw, SNAP_PX / g.pps).inspect(|s| st.snap = Some(*s)).unwrap_or(raw)
                } else {
                    raw
                };
                project.timeline.markers[i].position = pos;
            }
        }
        if r.drag_stopped() {
            st.marker_drag = None;
        }
        r.on_hover_text(&project.timeline.markers[i].name);
    }

    // R18a: CD layout bar and command bar tags: a click moves the cursor there; a double-click edits a command marker
    // (Command Properties) or selects a track region's time (VEGAS); command markers and indexes drag like markers.
    for (b, tag) in bar_tags(g, &project.timeline) {
        if tag.max.x < g.band.min.x || tag.min.x > g.band.max.x {
            continue;
        }
        let r = ui.interact(tag, ui.id().with(("bar_tag", b)), Sense::click_and_drag());
        let Some(at) = b.time(&project.timeline) else { continue };
        if r.double_clicked() {
            match b {
                BarTag::Command(i) => super::edit_command(ui.ctx(), i),
                BarTag::CdTrack(i, _) => project.timeline.loop_region = project.timeline.cd_tracks.get(i).map(|c| (c.start, c.end)),
                BarTag::CdIndex(_) => {}
            }
        } else if r.clicked() {
            project.timeline.cursor_position = at;
        }
        if r.drag_started() && b.pos_mut(&mut project.timeline).is_some() {
            history.push(project.clone());
            project.mark_modified();
            st.bar_drag = Some((b, r.interact_pointer_pos().map_or(0.0, |p| p.x - tag.min.x)));
        }
        if let (Some((k, grab)), Some(p)) = (st.bar_drag, r.interact_pointer_pos().filter(|_| r.dragged())) {
            if k == b {
                let raw = g.to_time(p.x - grab).max(0.0);
                let mut targets = edit::snap_targets(project, None);
                targets.retain(|v| (v - at).abs() > 1e-9);
                let pos = if snapping && !ui.input(|i| i.modifiers.shift) {
                    g.snaps(targets).nearest(raw, SNAP_PX / g.pps).inspect(|s| st.snap = Some(*s)).unwrap_or(raw)
                } else {
                    raw
                };
                if let Some(x) = b.pos_mut(&mut project.timeline) {
                    *x = pos;
                }
            }
        }
        if r.drag_stopped() {
            st.bar_drag = None;
        }
        let (_, label) = b.text(&project.timeline);
        if !label.is_empty() {
            r.on_hover_text(label);
        }
    }
}

/// Band background, bevels, ruler ticks/labels, loop flags and marker tags.
pub(super) fn paint_band(p: &Painter, g: &Geo, project: &Project, dragging_loop: bool) {
    let t = theme::get();
    let band = g.band;
    let (loop_r, marker_r, ruler) = band_rows(&t, band);
    let v16 = t.look() == UiLook::Vegas16;
    let fps = project.settings.frame_rate;
    p.rect_filled(band, 0.0, t.band_bg);
    let hline = |x: Rangef, y: f32, c: Color32| p.rect_filled(Rect::from_x_y_ranges(x, Rangef::new(y, y + 1.0)), 0.0, c);
    if v16 {
        let b = &t.bevel;
        hline(band.x_range(), band.min.y, b.band_1);
        hline(band.x_range(), band.min.y + 1.0, b.band_2);
        hline(band.x_range(), ruler.min.y - 2.0, t.band_separator);
        hline(band.x_range(), ruler.min.y - 1.0, b.band_bottom);
        hline(band.x_range(), ruler.max.y, t.line_dark);
    } else {
        hline(band.x_range(), ruler.min.y - 1.0, t.line_dark);
        hline(band.x_range(), band.max.y - 1.0, t.line_dark);
    }

    // Ruler: 1 px major ticks 11 px tall, minor ticks 3 px, labels 4 px right of the tick, baseline 3 px up.
    let p_r = p.with_clip_rect(ruler.intersect(p.clip_rect()));
    // Ticks where the ruler time (timeline time plus the ruler's start time) is a whole number of intervals.
    let s = &project.settings;
    let major = ruler_interval(g.pps, s);
    let n_minor = minor_count(major * g.pps);
    let off = s.ruler.offset;
    let first = ((g.scroll_x + off) / major).floor() as i64;
    let last = ((g.scroll_x + off + band.width() as f64 / g.pps) / major).ceil() as i64;
    let font = t.font(t.font.ruler);
    for i in first..=last {
        let time = i as f64 * major - off;
        let x = g.to_x(time).floor();
        p_r.rect_filled(Rect::from_min_max(pos2(x, ruler.max.y - 11.0), pos2(x + 1.0, ruler.max.y)), 0.0, t.ruler_major_tick);
        let label = s.ruler_time(time);
        p_r.text(pos2(x + 4.0, ruler.max.y - 2.0), Align2::LEFT_BOTTOM, label, font.clone(), t.ruler_text);
        for k in 1..n_minor {
            let mx = g.to_x(time + major * k as f64 / n_minor as f64).floor();
            p_r.rect_filled(Rect::from_min_max(pos2(mx, ruler.max.y - 3.0), pos2(mx + 1.0, ruler.max.y)), 0.0, t.ruler_minor_tick);
            if v16 {
                p_r.rect_filled(Rect::from_min_max(pos2(mx + 1.0, ruler.max.y - 3.0), pos2(mx + 2.0, ruler.max.y)), 0.0, t.ruler_minor_shadow);
            }
        }
    }

    // Loop bar: yellow flags at both ends (overlapping at 0 when there is no selection, as VEGAS draws them).
    let p_b = p.with_clip_rect(band.intersect(p.clip_rect()));
    let (a, b) = project.timeline.loop_region.unwrap_or((0.0, 0.0));
    let (xa, xb) = (g.to_x(a).floor(), g.to_x(b).floor());
    if b > a {
        if v16 {
            let cy = loop_r.center().y.floor();
            p_b.rect_filled(Rect::from_min_max(pos2(xa, cy - 1.0), pos2(xb + 1.0, cy + 2.0)), 0.0, t.loop_sel_line);
        } else {
            p_b.rect_filled(Rect::from_min_max(pos2(xa, loop_r.min.y), pos2(xb + 1.0, loop_r.max.y)), 0.0, t.loop_bar_off);
        }
    }
    let fy = loop_r.min.y;
    let flag = |pts: Vec<Pos2>| p_b.add(Shape::convex_polygon(pts, t.loop_flag, Stroke::NONE));
    flag(vec![pos2(xa, fy), pos2(xa + 6.0, fy), pos2(xa, fy + 6.0)]);
    flag(vec![pos2(xb + 1.0 - 6.0, fy), pos2(xb + 1.0, fy), pos2(xb + 1.0, fy + 6.0)]);
    if v16 && b > a {
        // Length tag: white box, black text, at the right end (§5.9.2).
        let len = if s.ruler.format == RulerFormat::TimeFrames { short_tc(b - a, fps) } else { s.ruler_len(b - a) };
        let text = format!("+{len}");
        let galley = p.layout_no_wrap(text, t.font(10.0), t.length_tag_text);
        let r = Rect::from_min_size(pos2(xb + 3.0, fy - 1.0), vec2(galley.size().x + 6.0, 12.0));
        if dragging_loop || r.max.x < band.max.x {
            p_b.rect_filled(r, 0.0, t.length_tag_bg);
            p_b.galley(pos2(r.min.x + 3.0, r.center().y - galley.size().y * 0.5), galley, t.length_tag_text);
        }
    }

    // Marker tags: 13×14 in marker_flag with the white number, label to the right.
    for (i, m) in project.timeline.markers.iter().enumerate() {
        let x = g.to_x(m.position).floor();
        if x < band.min.x - 200.0 || x > band.max.x {
            continue;
        }
        let tag = Rect::from_min_size(pos2(x, marker_r.min.y), vec2(13.0, 14.0));
        p_b.rect_filled(tag, 0.0, t.marker_flag);
        p_b.text(tag.center(), Align2::CENTER_CENTER, (i + 1).to_string(), t.bold(10.0), t.marker_text);
        p_b.text(pos2(tag.max.x + 3.0, tag.center().y), Align2::LEFT_CENTER, &m.name, t.font(t.font.ui), t.text);
    }

    // R18a: the CD layout bar and the command bar: a separator over each, the track regions' spans as a line (VP16), and
    // their tags in VEGAS's CD Track, CD Index and Command Marker colours with the number inside and the label beside.
    let tl = &project.timeline;
    let (cd, cmd) = bar_rows(band, tl);
    for row in [cd, cmd].into_iter().flatten() {
        hline(band.x_range(), row.min.y - 1.0, if v16 { t.band_separator } else { t.line_dark });
    }
    if let (Some(row), true) = (cd, v16) {
        for c in &tl.cd_tracks {
            hline(Rangef::new(g.to_x(c.start).floor(), g.to_x(c.end).floor()), row.max.y - 1.0, CD_TRACK);
        }
    }
    for (b, tag) in bar_tags(g, tl) {
        if tag.max.x < band.min.x - 200.0 || tag.min.x > band.max.x {
            continue;
        }
        let fill = match b {
            BarTag::Command(_) => COMMAND,
            BarTag::CdTrack(..) => CD_TRACK,
            BarTag::CdIndex(_) => CD_INDEX,
        };
        let (num, label) = b.text(tl);
        p_b.rect_filled(tag, 0.0, fill);
        p_b.text(tag.center(), Align2::CENTER_CENTER, num, t.bold(10.0), Color32::WHITE);
        if !label.is_empty() {
            p_b.text(pos2(tag.max.x + 3.0, tag.center().y), Align2::LEFT_CENTER, label, t.font(t.font.ui), t.text);
        }
    }

    // Region tags: start and end tags with the same number; the start carries the label (VP16: plus a line).
    for (i, r) in project.timeline.regions.iter().enumerate() {
        let (xa, xb) = (g.to_x(r.start).floor(), g.to_x(r.end).floor());
        if xb < band.min.x || xa > band.max.x {
            continue;
        }
        let num = (i + 1).to_string();
        if v16 {
            hline(Rangef::new(xa, xb), marker_r.max.y - 1.0, t.region_line);
        }
        for (k, x) in [xa, xb - 12.0].into_iter().enumerate() {
            let tag = Rect::from_min_size(pos2(x, marker_r.min.y), vec2(13.0, 14.0));
            p_b.rect_filled(tag, 0.0, t.region_flag);
            p_b.rect_filled(Rect::from_x_y_ranges(tag.x_range(), Rangef::new(tag.min.y, tag.min.y + 1.0)), 0.0, t.region_flag_hi);
            p_b.text(tag.center(), Align2::CENTER_CENTER, &num, t.bold(10.0), t.region_text);
            if k == 0 {
                p_b.text(pos2(tag.max.x + 3.0, tag.center().y), Align2::LEFT_CENTER, &r.name, t.font(t.font.ui), t.text);
            }
        }
    }
}

/// Grid lines (Options › Grid Spacing, `Geo::grid`) through the lanes: dashed 2/2 (16), dotted 1/2 (23 dark) or 3/3
/// (23 White). Drawn as one mesh of pixel quads: egui turns 1×1 rects into feathered line segments that vanish.
pub(super) fn paint_grid(p: &Painter, g: &Geo) {
    let t = theme::get();
    let (step, phase) = g.grid;
    let (on, off) = match t.look() {
        UiLook::Vegas16 => (2.0, 2.0),
        _ if t.dark_mode => (1.0, 2.0),
        _ => (3.0, 3.0),
    };
    let first = ((g.scroll_x - phase) / step).floor().max(0.0) as i64;
    let last = ((g.scroll_x - phase + g.lanes.width() as f64 / g.pps) / step).ceil() as i64;
    let mut mesh = Mesh::default();
    for i in first..=last {
        let x = g.to_x(i as f64 * step + phase).floor();
        if x < g.lanes.min.x || x > g.lanes.max.x {
            continue;
        }
        let mut y = g.lanes.min.y.floor();
        while y < g.lanes.max.y {
            mesh.add_colored_rect(Rect::from_min_max(pos2(x, y), pos2(x + 1.0, (y + on).min(g.lanes.max.y))), t.grid_line);
            y += on + off;
        }
    }
    p.add(mesh);
}

/// Selection overlay over the ruler and lanes (§5.9.8).
pub(super) fn paint_selection(p: &Painter, g: &Geo, region: Option<(f64, f64)>) {
    let t = theme::get();
    if let Some((a, b)) = region.filter(|(a, b)| b > a) {
        let (_, _, ruler) = band_rows(&t, g.band);
        let r = Rect::from_x_y_ranges(Rangef::new(g.to_x(a).floor(), g.to_x(b).floor() + 1.0), Rangef::new(ruler.min.y, g.lanes.max.y));
        let clip = Rect::from_x_y_ranges(g.lanes.x_range(), r.y_range());
        p.rect_filled(r.intersect(clip), 0.0, t.selection_overlay);
        if t.look() == UiLook::Vegas23 {
            // Thin olive edge at the selection start (c_ui_tl).
            p.rect_filled(Rect::from_x_y_ranges(Rangef::new(r.min.x, r.min.x + 1.0), r.y_range()).intersect(clip), 0.0, t.loop_sel_edge);
        }
    }
}

/// Cursor line through the ruler and lanes, and the head in the ruler (§5.9.6).
/// `ripple`: Auto Ripple is on, and the cursor line turns red (as the selected events' borders do).
pub(super) fn paint_playhead(p: &Painter, g: &Geo, time: f64, ripple: bool) {
    let t = theme::get();
    let line = if ripple { t.event_selected_border_ripple } else { t.playhead_line };
    let (_, _, ruler) = band_rows(&t, g.band);
    let x = g.to_x(time).floor();
    if x < g.lanes.min.x - 5.0 || x > g.lanes.max.x + 5.0 {
        return;
    }
    let p = p.with_clip_rect(Rect::from_x_y_ranges(g.lanes.x_range(), Rangef::new(ruler.min.y, g.lanes.max.y)).intersect(p.clip_rect()));
    let col = |x0: f32, c: Color32| p.rect_filled(Rect::from_min_max(pos2(x0, ruler.min.y), pos2(x0 + 1.0, g.lanes.max.y)), 0.0, c);
    let rb = ruler.max.y;
    if t.look() == UiLook::Vegas16 {
        // Black line plus a 1 px shadow to its right; white 9×12 flag with two dark ticks, tip on the ruler bottom.
        col(x, line);
        col(x + 1.0, t.playhead_outline);
        let x0 = x - 4.0;
        p.rect_filled(Rect::from_min_max(pos2(x0, rb - 12.0), pos2(x0 + 9.0, rb - 4.0)), 0.0, t.playhead_head);
        p.add(Shape::convex_polygon(vec![pos2(x0, rb - 4.0), pos2(x0 + 9.0, rb - 4.0), pos2(x0 + 4.5, rb)], t.playhead_head, Stroke::NONE));
        for dx in [3.0, 5.0] {
            p.rect_filled(Rect::from_min_size(pos2(x0 + dx, rb - 11.0), vec2(1.0, 4.0)), 0.0, t.fader_thumb_bar);
        }
    } else {
        // Red line with a dark 1 px side (both sides on Dark, right only on White); pentagon head 9×15, dark outline.
        if t.dark_mode {
            col(x - 1.0, t.playhead_outline);
        }
        col(x + 1.0, t.playhead_outline);
        col(x, line);
        let x0 = x - 4.0;
        let pts = vec![pos2(x0, rb - 15.0), pos2(x0 + 9.0, rb - 15.0), pos2(x0 + 9.0, rb - 5.0), pos2(x0 + 4.5, rb), pos2(x0, rb - 5.0)];
        p.add(Shape::convex_polygon(pts, t.playhead_head, Stroke::new(1.0_f32, t.playhead_outline)));
    }
}

/// Marker lines through the lanes, in the marker colour.
pub(super) fn paint_marker_lines(p: &Painter, g: &Geo, project: &Project) {
    let t = theme::get();
    for m in &project.timeline.markers {
        let x = g.to_x(m.position).floor();
        if x >= g.lanes.min.x && x <= g.lanes.max.x {
            p.rect_filled(Rect::from_min_max(pos2(x, g.lanes.min.y), pos2(x + 1.0, g.lanes.max.y)), 0.0, t.marker_flag);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ruler_spacing_and_labels() {
        let fps = 29.97;
        // 80 px/s: 2 s majors (160 px); 1 s would be 80 px < 90.
        assert_eq!(major_interval(80.0, fps), 2.0);
        assert!((major_interval(1000.0, fps) - 5.0 / fps).abs() < 1e-12); // 5 frames
        assert_eq!(major_interval(1.0, fps), 120.0);
        assert_eq!(major_interval(0.001, fps), 3600.0);
        for pps in [3.0, 40.0, 120.0, 900.0] {
            let px = major_interval(pps, fps) * pps;
            let gap = px / minor_count(px) as f64;
            assert!(px >= 90.0 && (5.5..=8.5).contains(&gap), "pps {pps}: major {px} px, minor {gap} px");
        }
        assert_eq!(short_tc(5.0, 29.97), "5;00");
        assert_eq!(short_tc(0.48, 25.0), "0:12");
        assert_eq!(short_tc(75.0, 25.0), "1:15:00");
    }

    /// R1: musical rulers tick on beats and measures; grid spacings thin out to ≥ 6 px; the ruler start time shifts
    /// the lines.
    #[test]
    fn ruler_and_grid_intervals() {
        let mut s = ProjectSettings::default(); // 120 bpm, 4/4: a beat is 0.5 s, a measure 2 s
        s.ruler.format = RulerFormat::MeasuresBeats;
        assert_eq!(ruler_interval(200.0, &s), 0.5);
        assert_eq!(ruler_interval(80.0, &s), 2.0);
        assert_eq!(ruler_interval(20.0, &s), 8.0);
        s.ruler.format = RulerFormat::SmpteEbu;
        assert!((ruler_interval(1000.0, &s) - 5.0 / 25.0).abs() < 1e-12); // 5 EBU frames
        // Time & Frames at 29.97 ticks on whole frames: every 60 frames reads as a whole 2 s.
        s.ruler.format = RulerFormat::TimeFrames;
        let m = ruler_interval(80.0, &s);
        assert!((m - 60.0 * 1001.0 / 30000.0).abs() < 1e-12);
        assert_eq!((s.ruler_time(9.0 * m), s.ruler_time(30.0 * m)), ("00:00:18;00".to_string(), "00:01:00;02".to_string()));
        s.ruler.format = RulerFormat::Seconds;
        assert_eq!(ruler_interval(80.0, &s), 2.0);
        let line = |gs, pps| grid_line(gs, &s, pps);
        assert_eq!(line(GridSpacing::Seconds, 80.0), (1.0, 0.0));
        assert_eq!(line(GridSpacing::Seconds, 2.0).0, 5.0); // 1 s would be 2 px
        assert!((line(GridSpacing::Milliseconds, 80.0).0 - 0.1).abs() < 1e-12);
        assert!((line(GridSpacing::Frames, 400.0).0 - 1001.0 / 30000.0).abs() < 1e-15);
        assert_eq!(line(GridSpacing::Measures, 80.0).0, 2.0);
        assert_eq!(line(GridSpacing::Note(16, false), 80.0).0, 0.125); // 10 px
        assert_eq!(line(GridSpacing::Note(32, false), 80.0).0, 0.125); // 5 px doubles
        assert!((line(GridSpacing::Note(8, true), 80.0).0 - 0.5 / 3.0).abs() < 1e-12);
        assert_eq!(line(GridSpacing::RulerMarks, 80.0), (ruler_interval(80.0, &s), 0.0));
        s.ruler.offset = 0.25;
        assert_eq!(grid_line(GridSpacing::Seconds, &s, 80.0), (1.0, 0.75));
    }

    /// R18a: the CD layout and command bars appear under the marker bar only when there is something on them, and the
    /// ruler moves down by their height; tags are numbered as VEGAS numbers them.
    #[test]
    fn cd_and_command_bars_follow_their_markers() {
        let t = crate::ui::theme::Theme::new(crate::ui::theme::LookChoice::VEGAS16);
        let mut tl = Timeline::new();
        assert_eq!(band_h(&t, &tl), t.m.band_h);
        let band = Rect::from_min_size(Pos2::ZERO, vec2(400.0, band_h(&t, &tl)));
        assert_eq!((bar_rows(band, &tl), band_rows(&t, band).2.min.y), ((None, None), 31.0));
        tl.cd_tracks = vec![CdTrack { start: 4.0, end: 6.0, ..Default::default() }, CdTrack { start: 0.0, end: 4.0, name: "One".into(), ..Default::default() }];
        tl.cd_indexes = vec![1.0, 2.0, 5.0, 9.0];
        tl.commands.push(CommandMarker { command: "URL".into(), ..Default::default() });
        let band = Rect::from_min_size(Pos2::ZERO, vec2(400.0, band_h(&t, &tl)));
        let (cd, cmd) = bar_rows(band, &tl);
        assert_eq!((band.height(), cd.unwrap().min.y, cmd.unwrap().min.y, band_rows(&t, band).2.min.y), (t.m.band_h + 28.0, 22.0, 36.0, 59.0));
        assert!(cmd.unwrap().max.y <= band_rows(&t, band).2.min.y);
        let n = |b: BarTag| b.text(&tl).0;
        assert_eq!((n(BarTag::CdTrack(0, false)), n(BarTag::CdTrack(1, true)), n(BarTag::CdIndex(1)), n(BarTag::CdIndex(2)), n(BarTag::CdIndex(3))), ("2".into(), "1".into(), "3".into(), "2".into(), String::new()));
        assert_eq!(BarTag::CdTrack(1, false).text(&tl).1, "One");
        assert_eq!((BarTag::Command(0).text(&tl).1.as_str(), BarTag::CdTrack(0, true).time(&tl)), ("URL", Some(6.0)));
    }

    #[test]
    fn loop_drags() {
        let mut tl = Timeline::new();
        loop_drag_to(&mut tl, LoopDrag::Create(4.0), 2.0);
        assert_eq!(tl.loop_region, Some((2.0, 4.0)));
        loop_drag_to(&mut tl, LoopDrag::End, 6.0);
        assert_eq!(tl.loop_region, Some((2.0, 6.0)));
        loop_drag_to(&mut tl, LoopDrag::Start, 7.0); // crossing the end swaps
        assert_eq!(tl.loop_region, Some((6.0, 7.0)));
        loop_drag_to(&mut tl, LoopDrag::Move(0.5), 1.0);
        assert_eq!(tl.loop_region, Some((0.5, 1.5)));
        loop_drag_to(&mut tl, LoopDrag::Move(0.5), 0.0); // clamped at 0
        assert_eq!(tl.loop_region, Some((0.0, 1.0)));
    }
}
