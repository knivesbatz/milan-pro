//! Custom-painted timeline (§5.9): time display, loop/marker/ruler band, track headers, lanes with events,
//! playhead, snap indicators and scrollbars. Interaction runs first and painting after, so edits show this frame.
mod ctx;
mod events;
mod ruler;
mod scroll;
mod tracks;

pub(super) use tracks::{fader_gain, fader_pos};

use eframe::egui::*;
use crate::audio::Waveforms;
use crate::decode::Thumbs;
use crate::edit;
use crate::history::History;
use crate::project::*;
use super::theme::{self, Theme, UiLook};
use super::{ClipRef, DragMode, DragState};
use events::{Ev, paint_event, paint_fade_tag};
use ruler::LoopDrag;

/// Trim zone at each event edge (±4 px, §5.9.4).
const EDGE_PX: f32 = 4.0;
/// Fade handle hit size at the top corners.
const FADE_PX: f32 = 6.0;
const SNAP_PX: f64 = 8.0;
/// Pixels per second at zoom 1.
pub const BASE_PPS: f64 = 80.0;
const MIN_TRACK_H: f32 = 22.0;

/// This frame's layout.
#[derive(Clone, Copy)]
struct Geo {
    /// Whole timeline area (time display + band + rows + horizontal scrollbar row).
    rect: Rect,
    header_w: f32,
    /// Track list column, rows part.
    list: Rect,
    /// Loop / marker / ruler band right of the time display.
    band: Rect,
    lanes: Rect,
    /// Vertical scrollbar column right of the band and lanes.
    vscroll: Rect,
    /// Horizontal scrollbar row under the lanes (incl. the zoom buttons).
    hscroll: Rect,
    pps: f64,
    scroll_x: f64,
}

impl Geo {
    fn to_x(&self, t: f64) -> f32 {
        ((t - self.scroll_x) * self.pps) as f32 + self.lanes.min.x
    }
    fn to_time(&self, x: f32) -> f64 {
        (x - self.lanes.min.x) as f64 / self.pps + self.scroll_x
    }
}

/// One track row on screen. `num`: position in the list (video tracks first), used for the number and colour.
#[derive(Clone, Copy)]
struct Row {
    is_video: bool,
    idx: usize,
    num: usize,
    top: f32,
    h: f32,
}

#[derive(Clone, Copy, PartialEq)]
enum Rename {
    Track(bool, usize),
    Marker(usize),
}

/// Timeline UI state kept in egui memory between frames.
#[derive(Clone, Default)]
struct TlState {
    sel_tracks: Vec<usize>,
    anchor: Option<usize>,
    loop_drag: Option<LoopDrag>,
    /// (marker index, grab offset in px)
    marker_drag: Option<(usize, f32)>,
    /// Snap target hit by this frame's drag, for the indicator line.
    snap: Option<f64>,
    rename: Option<(Rename, String)>,
    rename_focused: bool,
    hdrag: Option<scroll::HDrag>,
    vdrag: Option<f32>,
    height_drag: Option<f32>,
}

/// Track height; 0 in the project means the look's default for new tracks (§5.9.3: 64/68 vs 52 px).
fn track_h(tr: &Track, t: &Theme) -> f32 {
    if tr.height > 0.0 {
        tr.height
    } else if tr.kind == TrackKind::Video {
        t.m.video_track_h
    } else {
        t.m.audio_track_h
    }
}

fn set_zoom(tl: &mut Timeline, zoom: f64) {
    tl.zoom = zoom.clamp(0.0005, 400.0);
}

#[allow(clippy::too_many_arguments)]
pub fn draw_timeline(
    ui: &mut Ui,
    project: &mut Project,
    selected_clip: &mut Option<ClipRef>,
    drag_state: &mut Option<DragState>,
    scroll_y: &mut f32,
    history: &mut History,
    snapping: bool,
    waves: &mut Waveforms,
    thumbs: &mut Thumbs,
    playing: bool,
    track_w: &mut f32,
) {
    let t = theme::get();
    let v16 = t.look() == UiLook::Vegas16;
    let rect = ui.max_rect();
    if rect.width() < 80.0 || rect.height() < t.m.band_h + 40.0 {
        return;
    }
    let sid = ui.id().with("timeline_state");
    let mut st: TlState = ui.data_mut(|d| d.get_temp(sid)).unwrap_or_default();
    st.snap = None;

    // Track list | splitter | lanes. Dragging the splitter resizes the list; double-click toggles colour strips only (§4.5).
    let gap = t.m.timeline_gap;
    let max_w = (rect.width() - 200.0).max(t.m.color_strip_w);
    *track_w = track_w.clamp(t.m.color_strip_w, max_w);
    let header_w = *track_w;
    let response = ui.interact(rect, ui.id().with("timeline"), Sense::click_and_drag());
    // Registered after the timeline so it wins the hit test; the new width applies next frame.
    let split = Rect::from_min_size(pos2(rect.min.x + header_w, rect.min.y), vec2(gap, rect.height() - t.m.hscroll_h));
    let split_resp = ui.interact(split, ui.id().with("track_list_split"), Sense::click_and_drag());
    if split_resp.hovered() || split_resp.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
    }
    if split_resp.double_clicked() {
        *track_w = if *track_w > t.m.color_strip_w + 1.0 { t.m.color_strip_w } else { t.m.track_list_w };
    } else if split_resp.dragged() {
        *track_w = (*track_w + split_resp.drag_delta().x).clamp(t.m.color_strip_w, max_w);
    }

    let fps = project.settings.frame_rate;
    let frame = 1.0 / fps.max(1.0);
    let make_geo = |tl: &Timeline| {
        let clips_left = rect.min.x + header_w + gap;
        let rows_top = rect.min.y + t.m.band_h;
        let lanes = Rect::from_min_max(pos2(clips_left, rows_top), pos2(rect.max.x - t.m.scrollbar_w, rect.max.y - t.m.hscroll_h));
        Geo {
            rect,
            header_w,
            list: Rect::from_min_max(pos2(rect.min.x, rows_top), pos2(rect.min.x + header_w, lanes.max.y)),
            band: Rect::from_min_max(pos2(clips_left, rect.min.y), pos2(lanes.max.x, rows_top)),
            lanes,
            vscroll: Rect::from_min_max(pos2(lanes.max.x, rect.min.y), pos2(rect.max.x, lanes.max.y)),
            hscroll: Rect::from_min_max(pos2(clips_left, lanes.max.y), rect.max),
            pps: tl.zoom * BASE_PPS,
            scroll_x: tl.scroll_x,
        }
    };
    let rows_of = |project: &Project, g: &Geo, scroll_y: &mut f32| -> Vec<Row> {
        let tl = &project.timeline;
        let content: f32 = tl.video_tracks.iter().chain(&tl.audio_tracks).map(|tr| track_h(tr, &t)).sum();
        *scroll_y = scroll_y.clamp(0.0, (content - g.lanes.height()).max(0.0));
        let mut rows = Vec::new();
        let mut y = g.lanes.min.y - *scroll_y;
        for is_video in [true, false] {
            for (idx, tr) in tl.tracks(is_video).iter().enumerate() {
                let h = track_h(tr, &t);
                rows.push(Row { is_video, idx, num: rows.len(), top: y, h });
                y += h;
            }
        }
        rows
    };
    let mut g = make_geo(&project.timeline);
    let rows = rows_of(project, &g, scroll_y);

    // ---- Interaction ----

    tracks::interact(ui, &g, &rows, project, history, &mut st);

    // Wheel (§5.9.9): zooms around the mouse; Ctrl+wheel (or wheel over the track list) scrolls tracks;
    // Shift+wheel scrolls time.
    if ui.rect_contains_pointer(rect) {
        let (d, m, ptr) = ui.input(|i| (i.raw_scroll_delta, i.modifiers, i.pointer.hover_pos()));
        if let Some(ptr) = ptr.filter(|_| d != Vec2::ZERO) {
            let tl = &mut project.timeline;
            if m.command || ptr.x < g.lanes.min.x {
                *scroll_y -= d.y;
            } else if d.x != 0.0 {
                tl.scroll_x = (tl.scroll_x - d.x as f64 / g.pps).max(0.0);
            } else {
                let px = (ptr.x - g.lanes.min.x).clamp(0.0, g.lanes.width()) as f64;
                let anchor = g.scroll_x + px / g.pps;
                set_zoom(tl, tl.zoom * (d.y as f64 * 0.004).exp());
                tl.scroll_x = (anchor - px / (tl.zoom * BASE_PPS)).max(0.0);
            }
        }
    }
    // Page-follow the playhead during playback.
    if playing {
        let tl = &mut project.timeline;
        let view = g.lanes.width() as f64 / (tl.zoom * BASE_PPS);
        if tl.cursor_position < tl.scroll_x || tl.cursor_position > tl.scroll_x + view {
            tl.scroll_x = tl.cursor_position;
        }
    }
    g = make_geo(&project.timeline);

    ruler::interact(ui, &g, project, history, &mut st, snapping);
    scroll::interact(ui, &g, &rows, project, &mut st, scroll_y);
    g = make_geo(&project.timeline);
    let rows = rows_of(project, &g, scroll_y);

    let row_at = |py: f32| rows.iter().find(|r| py >= r.top.max(g.lanes.min.y) && py < r.top + r.h && py < g.lanes.max.y).copied();
    let hit_clip = |project: &Project, p: Pos2| -> Option<(ClipRef, DragMode)> {
        if !g.lanes.contains(p) {
            return None;
        }
        let row = row_at(p.y)?;
        let top = row.top + 1.0;
        if p.y < top || p.y >= row.top + row.h - 2.0 {
            return None;
        }
        let clips = &project.timeline.tracks(row.is_video)[row.idx].clips;
        // Later clips are drawn on top, so test them first.
        clips.iter().enumerate().rev().find_map(|(j, c)| {
            let (x0, x1) = (g.to_x(c.position), g.to_x(c.position + c.duration));
            if p.x < x0 || p.x > x1 {
                return None;
            }
            let (fi, fo) = (x0 + (c.fade_in * g.pps) as f32, x1 - (c.fade_out * g.pps) as f32);
            let near_top = p.y - top <= FADE_PX;
            let mode = if near_top && (p.x - fi).abs() <= FADE_PX && p.x - x0 < x1 - p.x {
                DragMode::FadeIn
            } else if near_top && (p.x - fo).abs() <= FADE_PX {
                DragMode::FadeOut
            } else if p.x - x0 <= EDGE_PX {
                DragMode::TrimLeft
            } else if x1 - p.x <= EDGE_PX {
                DragMode::TrimRight
            } else {
                DragMode::Move
            };
            Some((ClipRef { is_video: row.is_video, track_idx: row.idx, clip_idx: j }, mode))
        })
    };
    let is_locked = |project: &Project, cr: &ClipRef| {
        let t = &project.timeline.tracks(cr.is_video)[cr.track_idx];
        t.locked || t.clips.get(cr.clip_idx).is_some_and(|c| c.locked)
    };
    let shift = ui.input(|i| i.modifiers.shift);
    let snap_on = snapping && !shift;

    // Trim / fade cursor on event edges and corners, kept for the whole drag.
    let hovered_clip = response.hover_pos().and_then(|p| hit_clip(project, p));
    if let Some(ds) = drag_state.as_ref() {
        if ds.mode != DragMode::Move {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
        }
    } else if let Some((cr, mode)) = &hovered_clip {
        if *mode != DragMode::Move && !is_locked(project, cr) {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
        }
    }

    // Event drag: move / trim / fade. On an empty lane, VP16 drags out a time selection.
    if response.drag_started() {
        if let Some(p) = ui.input(|i| i.pointer.press_origin()).filter(|p| g.lanes.contains(*p)) {
            if let Some((cr, mode)) = hit_clip(project, p).filter(|(cr, _)| !is_locked(project, cr)) {
                let clip = project.timeline.tracks(cr.is_video)[cr.track_idx].clips[cr.clip_idx].clone();
                let edge = match mode {
                    DragMode::TrimRight => clip.position + clip.duration,
                    DragMode::FadeIn => clip.position + clip.fade_in,
                    DragMode::FadeOut => clip.position + clip.duration - clip.fade_out,
                    _ => clip.position,
                };
                *selected_clip = Some(cr.clone());
                *drag_state = Some(DragState {
                    clip_ref: cr,
                    original_position: clip.position,
                    mode,
                    grab_offset: g.to_time(p.x) - edge,
                    limit: edit::media_limit(project, &clip),
                    snap_targets: edit::snap_targets(project, Some(&clip.id)),
                    original: clip,
                    before: project.clone(),
                    changed: false,
                });
            } else if v16 && hit_clip(project, p).is_none() {
                let t0 = ruler::snapped(ui, project, g.to_time(p.x).max(0.0), g.pps, snapping, &mut st);
                st.loop_drag = Some(LoopDrag::Create(t0));
            }
        }
    }
    if let Some(ds) = drag_state.as_mut() {
        if let Some(p) = response.interact_pointer_pos().filter(|_| response.dragged()) {
            let thr = SNAP_PX / g.pps;
            let edge = g.to_time(p.x) - ds.grab_offset;
            let snap = if snap_on { edit::snap(edge, &ds.snap_targets, thr) } else { None };
            let snapped_edge = snap.unwrap_or(edge);

            // Vertical move to another unlocked track of the same kind
            if ds.mode == DragMode::Move {
                if let Some(row) = row_at(p.y) {
                    let cr = &mut ds.clip_ref;
                    let tracks = project.timeline.tracks_mut(row.is_video);
                    let ti = row.idx;
                    if row.is_video == cr.is_video && ti != cr.track_idx && !tracks[ti].locked && cr.clip_idx < tracks[cr.track_idx].clips.len() {
                        let c = tracks[cr.track_idx].clips.remove(cr.clip_idx);
                        tracks[ti].clips.push(c);
                        cr.track_idx = ti;
                        cr.clip_idx = tracks[ti].clips.len() - 1;
                        ds.changed = true;
                    }
                }
            }

            let cr = ds.clip_ref.clone();
            let clip = project.timeline.tracks_mut(cr.is_video).get_mut(cr.track_idx).and_then(|t| t.clips.get_mut(cr.clip_idx));
            if let Some(clip) = clip {
                match ds.mode {
                    DragMode::Move => {
                        let mut pos = edge.max(0.0);
                        if snap_on {
                            pos = edit::snap_span(pos, clip.duration, &ds.snap_targets, thr).max(0.0);
                            st.snap = [pos, pos + clip.duration].into_iter().find(|v| ds.snap_targets.iter().any(|x| (x - v).abs() < 1e-9));
                        }
                        clip.position = pos;
                    }
                    DragMode::TrimLeft => edit::trim_left(clip, &ds.original, snapped_edge, frame, ds.limit),
                    DragMode::TrimRight => edit::trim_right(clip, &ds.original, snapped_edge, frame, ds.limit),
                    DragMode::FadeIn => {
                        let len = ((snapped_edge - clip.position) / frame).round() * frame;
                        clip.fade_in = len.clamp(0.0, (clip.duration - clip.fade_out).max(0.0));
                    }
                    DragMode::FadeOut => {
                        let len = ((clip.position + clip.duration - snapped_edge) / frame).round() * frame;
                        clip.fade_out = len.clamp(0.0, (clip.duration - clip.fade_in).max(0.0));
                    }
                }
                if ds.mode != DragMode::Move {
                    st.snap = snap;
                }
                let o = &ds.original;
                ds.changed |= clip.position != ds.original_position
                    || clip.duration != o.duration
                    || clip.fade_in != o.fade_in
                    || clip.fade_out != o.fade_out;
                *selected_clip = Some(cr);
            }
        }
    }
    // Empty-lane time selection (VP16), or scrubbing a drag that started on the ruler.
    if drag_state.is_none() && response.dragged() {
        let origin = ui.input(|i| i.pointer.press_origin());
        if let (Some(o), Some(p)) = (origin, response.interact_pointer_pos()) {
            if let Some(d) = st.loop_drag {
                let tp = ruler::snapped(ui, project, g.to_time(p.x), g.pps, snapping, &mut st);
                ruler::loop_drag_to(&mut project.timeline, d, tp);
            } else if g.band.contains(o) {
                project.timeline.cursor_position = g.to_time(p.x.clamp(g.lanes.min.x, g.lanes.max.x)).max(0.0);
            }
        }
    }
    if response.drag_stopped() {
        if let Some(ds) = drag_state.take() {
            if ds.changed {
                history.push(ds.before);
                project.mark_modified();
            }
        }
        if st.loop_drag.take().is_some() && project.timeline.loop_region.is_some_and(|(a, b)| b - a < frame) {
            project.timeline.loop_region = None;
        }
    }

    // Drop media from the browser; snapped once so the indicator line shows exactly where the clip will land.
    let drop_time = |project: &Project, x: f32| {
        let t = g.to_time(x).max(0.0);
        if snapping { edit::snap(t, &edit::snap_targets(project, None), SNAP_PX / g.pps).unwrap_or(t) } else { t }
    };
    let drop_x = response
        .dnd_hover_payload::<super::MediaDrag>()
        .and_then(|_| ui.input(|i| i.pointer.latest_pos()))
        .filter(|p| g.lanes.contains(*p))
        .map(|p| g.to_x(drop_time(project, p.x)));
    if let Some(drag) = response.dnd_release_payload::<super::MediaDrag>() {
        if let Some(p) = ui.input(|i| i.pointer.latest_pos()).filter(|p| g.lanes.contains(*p)) {
            if let Some(row) = row_at(p.y) {
                let time = drop_time(project, p.x);
                let before = project.clone();
                if super::drop_media_drag(project, &drag, row.is_video, row.idx, time) {
                    history.push(before);
                }
            }
        }
    }

    // Click: select the event under the pointer (or none) and put the cursor there, as VEGAS does.
    // A right-click selects too, so the context menu acts on the clicked event.
    if response.clicked() || response.secondary_clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if g.lanes.contains(pos) || (response.secondary_clicked() && !g.band.contains(pos)) {
                *selected_clip = hit_clip(project, pos).map(|(cr, _)| cr);
                if response.clicked() {
                    project.timeline.cursor_position = g.to_time(pos.x).max(0.0);
                    if v16 && selected_clip.is_none() {
                        project.timeline.loop_region = None; // a plain click deselects the time
                    }
                }
            } else if response.clicked() && g.band.contains(pos) {
                project.timeline.cursor_position = g.to_time(pos.x).max(0.0);
            } else if response.clicked() && g.list.contains(pos) {
                st.sel_tracks.clear();
            }
        }
    }
    // 1–9, 0 jump to marker N through the keymap (§7).

    // Right-click menus for every part of the timeline (§6.8–6.10).
    ctx::menu(ui, &response, &g, &rows, project, history, selected_clip, &mut st);
    let rows = rows_of(project, &g, scroll_y);

    // ---- Painting ----

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, t.frame);
    let pl = painter.with_clip_rect(g.lanes);
    pl.rect_filled(g.lanes, 0.0, if v16 { t.frame } else { t.empty_lane_bg });
    for row in &rows {
        let lane = Rect::from_x_y_ranges(g.lanes.x_range(), Rangef::new(row.top, row.top + row.h - 2.0));
        pl.rect_filled(lane, 0.0, t.lane_bg);
        pl.rect_filled(Rect::from_x_y_ranges(g.lanes.x_range(), Rangef::new(lane.max.y, lane.max.y + 1.0)), 0.0, t.track_separator);
        pl.rect_filled(Rect::from_x_y_ranges(g.lanes.x_range(), Rangef::new(lane.max.y + 1.0, lane.max.y + 2.0)), 0.0, t.track_separator_2);
    }
    ruler::paint_grid(&pl, &g, fps);

    let hover_ref = hovered_clip.map(|(cr, _)| cr).or_else(|| drag_state.as_ref().map(|d| d.clip_ref.clone()));
    for row in &rows {
        if row.top > g.lanes.max.y || row.top + row.h < g.lanes.min.y {
            continue;
        }
        let track = &project.timeline.tracks(row.is_video)[row.idx];
        let lane = Rect::from_x_y_ranges(g.lanes.x_range(), Rangef::new(row.top, row.top + row.h - 2.0)).intersect(g.lanes);
        let pe = pl.with_clip_rect(lane);
        for (j, clip) in track.clips.iter().enumerate() {
            let (x0, x1) = (g.to_x(clip.position), g.to_x(clip.position + clip.duration));
            if x1 < g.lanes.min.x || x0 > g.lanes.max.x {
                continue;
            }
            let same = |c: &ClipRef| c.is_video == row.is_video && c.track_idx == row.idx && c.clip_idx == j;
            let media = clip.media_id.as_deref().and_then(|id| project.media_pool.iter().find(|m| m.id == id));
            let wave = media.filter(|m| !row.is_video && m.sample_rate.is_some()).and_then(|m| waves.get(m));
            let pic = media.filter(|m| matches!(m.media_type, MediaType::Video | MediaType::Image));
            let aspect = pic.and_then(|m| Some(m.width? as f32 / m.height?.max(1) as f32)).unwrap_or(16.0 / 9.0);
            let ev = Ev {
                rect: Rect::from_min_max(pos2(x0.round(), row.top + 1.0), pos2(x1.round().max(x0.round() + 1.0), row.top + row.h - 2.0)),
                clip,
                color: t.track_color(row.num),
                is_video: row.is_video,
                still: pic.is_some_and(|m| m.media_type == MediaType::Image),
                aspect,
                selected: selected_clip.as_ref().is_some_and(same),
                hovered: hover_ref.as_ref().is_some_and(same),
                dim: track.muted,
                pps: g.pps,
                fps,
            };
            let mut thumb = |src: f64| pic.and_then(|m| thumbs.get(m, src)).map(|tex| tex.id());
            paint_event(&pe, &t, &ev, t.lane_bg, wave, &mut thumb);
        }
    }
    ruler::paint_marker_lines(&pl, &g, project);
    if let Some(s) = st.snap {
        paint_snap(&painter, &g, &rows, project, s, drag_state.as_ref().map(|d| d.original.id.as_str()));
    }
    if let Some(x) = drop_x {
        pl.rect_filled(Rect::from_min_max(pos2(x.floor() - 1.0, g.lanes.min.y), pos2(x.floor() + 1.0, g.lanes.max.y)), 0.0, t.accent);
    }

    tracks::paint(&painter, &g, &rows, project, &st);
    let display = Rect::from_min_size(rect.min, vec2(header_w, t.m.time_display_h.min(t.m.band_h)));
    ruler::draw_time_display(&painter, display, project);
    ruler::paint_band(&painter, &g, project, st.loop_drag.is_some());
    ruler::paint_selection(&painter, &g, project.timeline.loop_region);
    ruler::paint_playhead(&painter, &g, project.timeline.cursor_position);
    // Fade length tag while dragging a fade.
    if let Some(ds) = drag_state.as_ref().filter(|d| matches!(d.mode, DragMode::FadeIn | DragMode::FadeOut)) {
        let cr = &ds.clip_ref;
        if let Some(c) = project.timeline.tracks(cr.is_video).get(cr.track_idx).and_then(|tr| tr.clips.get(cr.clip_idx)) {
            let (len, x) = if ds.mode == DragMode::FadeIn {
                (c.fade_in, g.to_x(c.position + c.fade_in) + 4.0)
            } else {
                (c.fade_out, g.to_x(c.position + c.duration - c.fade_out) - 44.0)
            };
            if let Some(row) = rows.iter().find(|r| r.is_video == cr.is_video && r.idx == cr.track_idx) {
                paint_fade_tag(&pl, &t, pos2(x, row.top + row.h * 0.5 - 7.0), ruler::short_tc(len, fps));
            }
        }
    }
    scroll::paint(&painter, &g, &rows, project, *scroll_y);

    rename_box(ui, &g, &rows, project, history, &mut st);
    ui.data_mut(|d| d.insert_temp(sid, st));
}

/// Vertical snap indicator in the target type's colour: across the target track for event edges, the full
/// height for the cursor, markers, loop ends and time 0 (§5.9.7). VP23 adds a soft glow.
fn paint_snap(p: &Painter, g: &Geo, rows: &[Row], project: &Project, s: f64, exclude: Option<&str>) {
    let t = theme::get();
    let tl = &project.timeline;
    let near = |v: f64| (v - s).abs() < 1e-9;
    let full = Rangef::new(g.band.max.y - t.m.ruler_h, g.lanes.max.y);
    let (color, spans): (Color32, Vec<Rangef>) = if near(tl.cursor_position) {
        (t.snap.cursor, vec![full])
    } else if tl.markers.iter().any(|m| near(m.position)) {
        (t.snap.marker, vec![full])
    } else if tl.loop_region.is_some_and(|(a, b)| near(a) || near(b)) {
        (t.snap.loop_, vec![full])
    } else {
        let spans: Vec<Rangef> = rows
            .iter()
            .filter(|r| {
                tl.tracks(r.is_video)[r.idx].clips.iter().any(|c| Some(c.id.as_str()) != exclude && (near(c.position) || near(c.position + c.duration)))
            })
            .map(|r| Rangef::new(r.top, r.top + r.h - 2.0))
            .collect();
        if spans.is_empty() { (t.snap.grid, vec![full]) } else { (t.snap.event, spans) }
    };
    let x = g.to_x(s).floor();
    let p = p.with_clip_rect(Rect::from_x_y_ranges(g.lanes.x_range(), full).intersect(p.clip_rect()));
    for span in spans {
        if t.look() != UiLook::Vegas16 {
            p.rect_filled(Rect::from_x_y_ranges(Rangef::new(x - 1.5, x + 2.5), span), 0.0, color.gamma_multiply(0.35));
        }
        p.rect_filled(Rect::from_x_y_ranges(Rangef::new(x, x + 1.0), span), 0.0, color);
    }
}

/// Inline rename box for a track name (double-click the header's top-left) or a marker (double-click its tag).
fn rename_box(ui: &mut Ui, g: &Geo, rows: &[Row], project: &mut Project, history: &mut History, st: &mut TlState) {
    let Some((target, mut buf)) = st.rename.take() else {
        st.rename_focused = false;
        return;
    };
    let t = theme::get();
    let at = match target {
        Rename::Track(v, i) => rows.iter().find(|r| r.is_video == v && r.idx == i).map(|r| {
            let x = g.rect.min.x + t.m.color_strip_w + 4.0;
            Rect::from_min_size(pos2(x, r.top + 2.0), vec2((g.header_w - t.m.color_strip_w - 80.0).max(60.0), 18.0))
        }),
        Rename::Marker(i) => project.timeline.markers.get(i).map(|m| {
            let (_, marker_r, _) = ruler::band_rows(&t, g.band);
            Rect::from_min_size(pos2(g.to_x(m.position) + 14.0, marker_r.min.y - 2.0), vec2(160.0, 18.0))
        }),
    };
    let Some(at) = at else { return };
    let resp = ui.put(at, TextEdit::singleline(&mut buf).font(t.font(t.font.ui)).margin(vec2(2.0, 1.0)));
    if !st.rename_focused {
        resp.request_focus();
        st.rename_focused = true;
    }
    if resp.lost_focus() {
        st.rename_focused = false;
        if !ui.input(|i| i.key_pressed(Key::Escape)) {
            history.push(project.clone());
            match target {
                Rename::Track(v, i) => project.timeline.tracks_mut(v)[i].name = buf,
                Rename::Marker(i) => project.timeline.markers[i].name = buf,
            }
            project.mark_modified();
        }
    } else {
        st.rename = Some((target, buf));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Rig {
        ctx: Context,
        project: Project,
        sel: Option<ClipRef>,
        drag: Option<DragState>,
        scroll_y: f32,
        history: History,
        waves: Waveforms,
        thumbs: Thumbs,
        track_w: f32,
    }

    impl Rig {
        fn new() -> Self {
            let ctx = Context::default();
            crate::ui::fonts::install(&ctx);
            let mut project = Project::new("t");
            project.timeline.video_tracks[0].clips.push(Clip::new("c", 2.0, 4.0));
            let (waves, thumbs) = (Waveforms::new(&ctx), Thumbs::new(&ctx));
            Rig { ctx, project, sel: None, drag: None, scroll_y: 0.0, history: History::default(), waves, thumbs, track_w: 251.0 }
        }
        fn frame(&mut self, events: Vec<Event>) {
            let raw = RawInput { events, screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 400.0))), ..Default::default() };
            let ctx = self.ctx.clone();
            let _ = ctx.run(raw, |ctx| {
                CentralPanel::default().frame(Frame::NONE).show(ctx, |ui| {
                    let mut child = ui.new_child(UiBuilder::new().max_rect(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 400.0))));
                    let r = &mut *self;
                    draw_timeline(
                        &mut child, &mut r.project, &mut r.sel, &mut r.drag, &mut r.scroll_y, &mut r.history, true,
                        &mut r.waves, &mut r.thumbs, false, &mut r.track_w,
                    );
                });
            });
        }
        fn drag(&mut self, from: Pos2, to: Pos2) {
            let press = |p: Pos2, pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
            self.frame(vec![Event::PointerMoved(from)]);
            self.frame(vec![press(from, true)]);
            for k in 1..=4 {
                self.frame(vec![Event::PointerMoved(from + (to - from) * k as f32 / 4.0)]);
            }
            self.frame(vec![press(to, false)]);
            self.frame(vec![]);
        }
        fn clip(&self) -> &Clip {
            &self.project.timeline.video_tracks[0].clips[0]
        }
        fn right_click(&mut self, p: Pos2) {
            let press = |pressed| Event::PointerButton { pos: p, button: PointerButton::Secondary, pressed, modifiers: Modifiers::NONE };
            self.frame(vec![Event::PointerMoved(p)]);
            self.frame(vec![press(true)]);
            self.frame(vec![press(false)]);
            self.frame(vec![]);
        }
    }

    /// Right-clicks open the matching context menu (§6.8–6.10), including on child widgets like headers.
    #[test]
    fn right_click_menus() {
        let mut r = Rig::new();
        r.right_click(pos2(575.0, 90.0));
        assert!(r.ctx.is_context_menu_open(), "event menu");
        assert!(r.sel.is_some(), "right-click selects the event");
        r.right_click(pos2(100.0, 80.0));
        assert!(r.ctx.is_context_menu_open(), "track header menu");
        r.right_click(pos2(400.0, 15.0));
        assert!(r.ctx.is_context_menu_open(), "marker bar menu");
    }

    /// Simulated mouse input against the VP16 layout: lanes start at x = 251 + 4, 80 px/s, Video 1 at y 48..112.
    #[test]
    fn move_fade_trim_and_loop_drags() {
        let mut r = Rig::new();
        r.drag(pos2(575.0, 90.0), pos2(655.0, 90.0));
        assert!((r.clip().position - 3.0).abs() < 1e-6, "moved to {}", r.clip().position);
        assert_eq!(r.history.lens().0, 1);
        assert!(r.sel.is_some());

        // Top-left corner of the event (now at x 495) sets the fade-in length, frame-quantised.
        r.drag(pos2(497.0, 51.0), pos2(577.0, 51.0));
        assert!((r.clip().fade_in - 1.0).abs() < 0.02, "fade in {}", r.clip().fade_in);
        assert_eq!(r.clip().position, 3.0);

        // Right edge (x 815) trims, and the fade stays inside the event.
        r.drag(pos2(814.0, 90.0), pos2(734.0, 90.0));
        assert!((r.clip().duration - 3.0).abs() < 1e-6, "duration {}", r.clip().duration);
        assert_eq!(r.history.lens().0, 3);

        // Loop bar (VP16 rows 2..6 of the band) drags out a selection.
        r.drag(pos2(300.0, 4.0), pos2(460.0, 4.0));
        let (a, b) = r.project.timeline.loop_region.expect("loop region");
        assert!((a - 0.5625).abs() < 1e-6 && (b - 2.5625).abs() < 1e-6, "loop {a}..{b}");
    }
}
