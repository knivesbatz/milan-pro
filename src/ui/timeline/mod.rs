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
use super::{ClipRef, DragMode, DragState, Selection};
use events::{Ev, paint_event, paint_fade_tag};
use ruler::LoopDrag;

/// Trim zone at each event edge (±4 px, §5.9.4).
const EDGE_PX: f32 = 4.0;
/// Fade handle hit size at the top corners.
const FADE_PX: f32 = 6.0;
const SNAP_PX: f64 = 8.0;
/// Pixels per second at zoom 1.
pub const BASE_PPS: f64 = 80.0;
pub const MIN_TRACK_H: f32 = 22.0;

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
    /// Row number Shift+click on a header extends from.
    anchor: Option<usize>,
    /// Header reorder drag in progress.
    track_drag: Option<tracks::TrackDrag>,
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
    sel: &mut Selection,
    drag_state: &mut Option<DragState>,
    scroll_y: &mut f32,
    history: &mut History,
    snapping: bool,
    quantize: bool,
    waves: &mut Waveforms,
    thumbs: &mut Thumbs,
    playing: bool,
    track_w: &mut f32,
    drops: &mut super::FileDrag,
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
    // Quantize to Frames also puts a clicked cursor on a frame, as in VEGAS.
    let cursor_at = |t: f64| if quantize { edit::quantize(t.max(0.0), fps) } else { t.max(0.0) };
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

    tracks::interact(ui, &g, &rows, project, history, sel, &mut st);

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
                // Dragging an unselected event selects it alone; a selected one moves the whole selection.
                if !sel.clips.contains(&clip.id) {
                    sel.clips = vec![clip.id.clone()];
                }
                sel.tracks = vec![project.timeline.tracks(cr.is_video)[cr.track_idx].id.clone()];
                sel.list_focus = false;
                let ids = if mode == DragMode::Move { sel.clips.clone() } else { vec![clip.id.clone()] };
                let moving: Vec<&str> = ids.iter().map(String::as_str).collect();
                *drag_state = Some(DragState {
                    id: clip.id.clone(),
                    mode,
                    grab_offset: g.to_time(p.x) - edge,
                    limit: edit::media_limit(project, &clip),
                    snap_targets: edit::snap_targets_except(project, &moving),
                    ids,
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
            let q = |t: f64| if quantize { edit::quantize(t, fps) } else { t };
            let trim_edge = snap.unwrap_or_else(|| q(edge));

            if ds.mode == DragMode::Move {
                // The grabbed event snaps, or else quantizes (a snapped edge stays on its target even off the frame
                // grid); every selected event gets the same delta, applied to the tracks as they were before the
                // drag, so each frame starts from scratch.
                let o = &ds.original;
                let mut pos = edge.max(0.0);
                let mut hit = None;
                if snap_on {
                    pos = edit::snap_span(pos, o.duration, &ds.snap_targets, thr).max(0.0);
                    hit = [pos, pos + o.duration].into_iter().find(|v| ds.snap_targets.iter().any(|x| (x - v).abs() < 1e-9));
                    st.snap = hit;
                }
                let dt = if hit.is_some() { pos } else { q(pos) } - o.position;
                let from = ds.before.find_clip(&ds.id);
                let dtrack = row_at(p.y).zip(from).filter(|(r, f)| r.is_video == f.is_video).map_or(0, |(r, f)| r.idx as isize - f.track_idx as isize);
                let tl = &mut project.timeline;
                tl.video_tracks.clone_from(&ds.before.timeline.video_tracks);
                tl.audio_tracks.clone_from(&ds.before.timeline.audio_tracks);
                // A hop onto a missing or locked track still moves in time.
                ds.changed = edit::move_clips(project, &ds.ids, dt, dtrack) || (dtrack != 0 && edit::move_clips(project, &ds.ids, dt, 0));
                project.modified = ds.before.modified; // marked on release, if anything changed
            }

            let cr = project.find_clip(&ds.id).filter(|_| ds.mode != DragMode::Move);
            let clip = cr.and_then(|cr| project.timeline.tracks_mut(cr.is_video).get_mut(cr.track_idx)?.clips.get_mut(cr.clip_idx));
            if let Some(clip) = clip {
                match ds.mode {
                    DragMode::Move => {}
                    DragMode::TrimLeft => edit::trim_left(clip, &ds.original, trim_edge, frame, ds.limit),
                    DragMode::TrimRight => edit::trim_right(clip, &ds.original, trim_edge, frame, ds.limit),
                    DragMode::FadeIn => {
                        let len = ((snapped_edge - clip.position) / frame).round() * frame;
                        clip.fade_in = len.clamp(0.0, (clip.duration - clip.fade_out).max(0.0));
                    }
                    DragMode::FadeOut => {
                        let len = ((clip.position + clip.duration - snapped_edge) / frame).round() * frame;
                        clip.fade_out = len.clamp(0.0, (clip.duration - clip.fade_in).max(0.0));
                    }
                }
                st.snap = snap;
                let o = &ds.original;
                ds.changed |= clip.position != o.position || clip.duration != o.duration || clip.fade_in != o.fade_in || clip.fade_out != o.fade_out;
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
                project.timeline.cursor_position = cursor_at(g.to_time(p.x.clamp(g.lanes.min.x, g.lanes.max.x)));
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

    // Media dragged from Project Media, the Trimmer, the Explorer window or Windows: the events the drop will add are
    // planned every frame for the ghost, at the start time snapped (or quantized) once, as the drop itself is.
    let drop_time = |project: &Project, x: f32| {
        let t = g.to_time(x).max(0.0);
        let s = if snapping { edit::snap(t, &edit::snap_targets(project, None), SNAP_PX / g.pps) } else { None };
        s.unwrap_or(if quantize { edit::quantize(t, fps) } else { t })
    };
    let payload = response.dnd_hover_payload::<super::MediaDrag>();
    let pointer = if payload.is_some() { ui.input(|i| i.pointer.latest_pos()) } else { drops.os_pos };
    let mut ghost = pointer.filter(|p| g.lanes.contains(*p)).map(|p| {
        let target = row_at(p.y).map(|row| (row.is_video, row.idx));
        let time = drop_time(project, p.x);
        let items: Vec<(&MediaItem, Option<(f64, f64)>)> = match payload.as_deref() {
            Some(super::MediaDrag::Pool(idxs, range)) => idxs.iter().filter_map(|&i| project.media_pool.get(i)).map(|m| (m, *range)).collect(),
            // Files show once probed; until then only the indicator line.
            _ => drops.items.iter().flatten().map(|m| (m, None)).collect(),
        };
        (target, time, edit::plan_drop(&project.timeline, &items, target, time, drops.still_len))
    });
    if ghost.as_ref().is_some_and(|gh| gh.2.refused) && payload.is_some() {
        ui.ctx().set_cursor_icon(CursorIcon::NotAllowed);
    }
    let released = response.dnd_release_payload::<super::MediaDrag>();
    if released.is_some() || drops.os_dropped {
        if let Some((target, time, plan)) = ghost.take() {
            drops.os_dropped = false;
            match released.as_deref() {
                Some(super::MediaDrag::Pool(..)) => {
                    let before = project.clone();
                    let ids = edit::apply_drop(project, plan);
                    if !ids.is_empty() {
                        history.push(before);
                        (sel.clips, sel.list_focus) = (ids, false);
                    }
                }
                // Files are placed by the app once probed, on the track (by id) and at the time of the drop.
                _ if !plan.refused => {
                    let track = target.map(|(v, i)| project.timeline.tracks(v)[i].id.clone());
                    drops.dropped_at = Some((drops.files.clone(), track, time));
                }
                _ => {}
            }
        }
    }

    // Click: select the event under the pointer (Ctrl toggles, Shift adds the range on its track) and put the
    // cursor there, as VEGAS does; the clicked track gets the focus. A right-click on an unselected event selects
    // it, so the context menu acts on the clicked event.
    if response.clicked() || response.secondary_clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if g.lanes.contains(pos) || (response.secondary_clicked() && !g.band.contains(pos)) {
                let hit = hit_clip(project, pos).map(|(cr, _)| cr);
                let m = ui.input(|i| i.modifiers);
                if g.lanes.contains(pos) {
                    // Any click in the lanes (also below the last track) takes the focus from the track list.
                    sel.list_focus = false;
                    if let Some(row) = row_at(pos.y) {
                        sel.tracks = vec![project.timeline.tracks(row.is_video)[row.idx].id.clone()];
                    }
                }
                match &hit {
                    Some(cr) if response.secondary_clicked() => {
                        let id = &project.timeline.tracks(cr.is_video)[cr.track_idx].clips[cr.clip_idx].id;
                        if !sel.clips.contains(id) {
                            sel.clips = vec![id.clone()];
                        }
                    }
                    Some(cr) => click_select(project, sel, cr, m.command, m.shift),
                    None if !(m.command || m.shift) => sel.clips.clear(),
                    None => {}
                }
                if response.clicked() {
                    project.timeline.cursor_position = cursor_at(g.to_time(pos.x));
                    if v16 && hit.is_none() {
                        project.timeline.loop_region = None; // a plain click deselects the time
                    }
                }
            } else if response.clicked() && g.band.contains(pos) {
                project.timeline.cursor_position = cursor_at(g.to_time(pos.x));
            } else if response.clicked() && g.list.contains(pos) {
                sel.tracks.clear();
            }
        }
    }
    // 1–9, 0 jump to marker N through the keymap (§7).

    // Right-click menus for every part of the timeline (§6.8–6.10).
    ctx::menu(ui, &response, &g, &rows, project, history, sel, &mut st);
    let mut rows = rows_of(project, &g, scroll_y);
    // A drop that adds video tracks: the audio rows move down to show where they will go.
    if let Some((_, _, plan)) = &ghost {
        let dy = added_tracks(&project.timeline, plan, true) as f32 * t.m.video_track_h;
        rows.iter_mut().filter(|r| !r.is_video).for_each(|r| r.top += dy);
    }

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

    let hover_ref = hovered_clip.map(|(cr, _)| cr).or_else(|| drag_state.as_ref().and_then(|d| project.find_clip(&d.id)));
    let selected: std::collections::HashSet<&str> = sel.clips.iter().map(String::as_str).collect();
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
                selected: selected.contains(clip.id.as_str()),
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
        paint_snap(&painter, &g, &rows, project, s, drag_state.as_ref().map_or(&[], |d| d.ids.as_slice()));
    }
    if let Some((target, time, plan)) = &ghost {
        paint_ghost(&pl, &g, &rows, project, *target, plan);
        let x = g.to_x(*time).floor();
        pl.rect_filled(Rect::from_min_max(pos2(x - 1.0, g.lanes.min.y), pos2(x + 1.0, g.lanes.max.y)), 0.0, t.accent);
    }

    tracks::paint(&painter, &g, &rows, project, sel);
    tracks::paint_reorder(&painter, &g, &rows, &st);
    let display = Rect::from_min_size(rect.min, vec2(header_w, t.m.time_display_h.min(t.m.band_h)));
    ruler::draw_time_display(&painter, display, project);
    ruler::paint_band(&painter, &g, project, st.loop_drag.is_some());
    ruler::paint_selection(&painter, &g, project.timeline.loop_region);
    ruler::paint_playhead(&painter, &g, project.timeline.cursor_position);
    // Fade length tag while dragging a fade.
    if let Some(ds) = drag_state.as_ref().filter(|d| matches!(d.mode, DragMode::FadeIn | DragMode::FadeOut)) {
        if let Some((cr, c)) = project.find_clip(&ds.id).zip(project.clip(&ds.id)) {
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

/// Left-click on event `cr`: replace the selection, Ctrl toggles it, Shift selects every event on its track between
/// the focus event and it (a plain click when the focus is on another track). The clicked event becomes the focus.
fn click_select(project: &Project, sel: &mut Selection, cr: &ClipRef, ctrl: bool, shift: bool) {
    let track = &project.timeline.tracks(cr.is_video)[cr.track_idx];
    let id = track.clips[cr.clip_idx].id.clone();
    let focus = sel.clips.last().and_then(|f| track.clips.iter().find(|c| c.id == *f));
    if ctrl {
        if let Some(k) = sel.clips.iter().position(|s| *s == id) {
            sel.clips.remove(k);
        } else {
            sel.clips.push(id);
        }
    } else if let Some(f) = focus.filter(|_| shift) {
        let c = &track.clips[cr.clip_idx];
        let (a, b) = (f.position.min(c.position), f.position.max(c.position));
        sel.clips = track.clips.iter().filter(|e| e.position >= a && e.position <= b && e.id != id).map(|e| e.id.clone()).collect();
        sel.clips.push(id);
    } else {
        sel.clips = vec![id];
    }
}

/// Vertical snap indicator in the target type's colour: across the target track for event edges, the full
/// height for the cursor, markers, loop ends and time 0 (§5.9.7). VP23 adds a soft glow.
fn paint_snap(p: &Painter, g: &Geo, rows: &[Row], project: &Project, s: f64, exclude: &[String]) {
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
                tl.tracks(r.is_video)[r.idx].clips.iter().any(|c| !exclude.contains(&c.id) && (near(c.position) || near(c.position + c.duration)))
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

/// How many tracks of kind `v` (video when true) the drop `plan` adds.
fn added_tracks(tl: &Timeline, plan: &edit::DropPlan, v: bool) -> usize {
    plan.events.iter().filter(|e| e.0 == v).map(|e| e.1 + 1).max().unwrap_or(0).saturating_sub(tl.tracks(v).len())
}

/// Red for a drop that a locked track refuses (no theme token: VEGAS itself shows only the "no" cursor).
const REFUSED: Color32 = Color32::from_rgb(0xD0, 0x40, 0x38);

/// Ghost of a media drop: a translucent event (track colour, name, real length) on each row it lands on and a
/// dashed row below the last track for each track it adds; red, with the target row tinted, when a locked track
/// refuses it.
fn paint_ghost(p: &Painter, g: &Geo, rows: &[Row], project: &Project, target: Option<(bool, usize)>, plan: &edit::DropPlan) {
    let t = theme::get();
    let tl = &project.timeline;
    // New video tracks go after the last video track (the audio rows were moved down for them), new audio tracks
    // after the last row; `num` is the row number each will have, for its colour.
    let nv = added_tracks(tl, plan, true);
    let bottom = |v: bool| rows.iter().rev().find(|r| r.is_video == v).map(|r| r.top + r.h);
    let video_end = bottom(true).or(rows.first().map(|r| r.top - nv as f32 * t.m.video_track_h)).unwrap_or(g.lanes.min.y);
    let audio_end = bottom(false).unwrap_or(video_end + nv as f32 * t.m.video_track_h);
    let mut extra: Vec<Row> = Vec::new();
    for (v, mut y, num, h) in [(true, video_end, tl.video_tracks.len(), t.m.video_track_h), (false, audio_end, rows.len() + nv, t.m.audio_track_h)] {
        for k in 0..added_tracks(tl, plan, v) {
            extra.push(Row { is_video: v, idx: tl.tracks(v).len() + k, num: num + k, top: y, h });
            y += h;
        }
    }
    let lane = |r: &Row| Rect::from_x_y_ranges(g.lanes.x_range(), Rangef::new(r.top, r.top + r.h - 2.0));
    let stroke = Stroke::new(1.0_f32, if plan.refused { REFUSED } else { t.text_dim });
    for r in &extra {
        let b = lane(r).shrink(1.0);
        p.extend(Shape::dashed_line(&[b.left_top(), b.right_top(), b.right_bottom(), b.left_bottom(), b.left_top()], stroke, 6.0, 4.0));
    }
    if let Some(r) = target.filter(|_| plan.refused).and_then(|(v, i)| rows.iter().find(|r| r.is_video == v && r.idx == i)) {
        p.rect_filled(lane(r), 0.0, REFUSED.gamma_multiply(0.25));
    }
    for (v, i, c) in &plan.events {
        let Some(r) = rows.iter().chain(&extra).find(|r| r.is_video == *v && r.idx == *i) else { continue };
        let color = if plan.refused { REFUSED } else { t.track_color(r.num) };
        let x0 = g.to_x(c.position);
        let ev = Rect::from_min_max(pos2(x0, r.top + 1.0), pos2(g.to_x(c.position + c.duration).max(x0 + 1.0), r.top + r.h - 2.0));
        p.rect_filled(ev, 0.0, color.gamma_multiply(0.5));
        p.rect_stroke(ev, 0.0, Stroke::new(1.0_f32, color), StrokeKind::Inside);
        p.with_clip_rect(ev.intersect(p.clip_rect())).text(ev.min + vec2(4.0, 3.0), Align2::LEFT_TOP, &c.name, t.font(t.font.ui), t.text_bright);
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
        let old = match target {
            Rename::Track(v, i) => project.timeline.tracks(v).get(i).map(|t| &t.name),
            Rename::Marker(i) => project.timeline.markers.get(i).map(|m| &m.name),
        };
        // An unchanged name is no edit (no undo step).
        if !ui.input(|i| i.key_pressed(Key::Escape)) && old.is_some_and(|o| *o != buf) {
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
        sel: Selection,
        drag: Option<DragState>,
        scroll_y: f32,
        history: History,
        waves: Waveforms,
        thumbs: Thumbs,
        track_w: f32,
        quantize: bool,
        drops: crate::ui::FileDrag,
        /// Cursor the last frame asked for.
        cursor: CursorIcon,
    }

    impl Rig {
        fn new() -> Self {
            let ctx = Context::default();
            crate::ui::fonts::install(&ctx);
            let mut project = Project::new("t");
            project.timeline.video_tracks[0].clips.push(Clip::new("c", 2.0, 4.0));
            let (waves, thumbs) = (Waveforms::new(&ctx), Thumbs::new(&ctx));
            let drops = crate::ui::FileDrag { still_len: 5.0, ..Default::default() };
            Rig { ctx, project, sel: Selection::default(), drag: None, scroll_y: 0.0, history: History::default(), waves, thumbs, track_w: 251.0, quantize: false, drops, cursor: CursorIcon::Default }
        }
        fn frame(&mut self, events: Vec<Event>) {
            self.frame_mod(events, Modifiers::NONE);
        }
        fn frame_mod(&mut self, events: Vec<Event>, modifiers: Modifiers) {
            let screen_rect = Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 400.0)));
            let raw = RawInput { events, screen_rect, modifiers, ..Default::default() };
            let ctx = self.ctx.clone();
            let out = ctx.run(raw, |ctx| {
                CentralPanel::default().frame(Frame::NONE).show(ctx, |ui| {
                    let mut child = ui.new_child(UiBuilder::new().max_rect(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 400.0))));
                    let r = &mut *self;
                    draw_timeline(
                        &mut child, &mut r.project, &mut r.sel, &mut r.drag, &mut r.scroll_y, &mut r.history, true, r.quantize,
                        &mut r.waves, &mut r.thumbs, false, &mut r.track_w, &mut r.drops,
                    );
                });
            });
            self.cursor = out.platform_output.cursor_icon;
        }
        /// A Project Media drag of pool items `idxs` held over `at`, then released there; returns the cursor while
        /// it hovered.
        fn media_drop(&mut self, idxs: Vec<usize>, at: Pos2) -> CursorIcon {
            self.frame(vec![Event::PointerMoved(at)]);
            DragAndDrop::set_payload(&self.ctx, crate::ui::MediaDrag::Pool(idxs, None));
            self.frame(vec![Event::PointerMoved(at)]);
            let hover = self.cursor;
            self.frame(vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }]);
            self.frame(vec![]);
            hover
        }
        fn drag(&mut self, from: Pos2, to: Pos2) {
            self.drag_mod(from, to, Modifiers::NONE);
        }
        fn drag_mod(&mut self, from: Pos2, to: Pos2, modifiers: Modifiers) {
            let press = |p: Pos2, pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers };
            self.frame_mod(vec![Event::PointerMoved(from)], modifiers);
            self.frame_mod(vec![press(from, true)], modifiers);
            for k in 1..=4 {
                self.frame_mod(vec![Event::PointerMoved(from + (to - from) * k as f32 / 4.0)], modifiers);
            }
            self.frame_mod(vec![press(to, false)], modifiers);
            self.frame(vec![]);
        }
        fn clip(&self) -> &Clip {
            &self.project.timeline.video_tracks[0].clips[0]
        }
        fn click(&mut self, p: Pos2, modifiers: Modifiers) {
            let press = |pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers };
            self.frame_mod(vec![Event::PointerMoved(p)], modifiers);
            self.frame_mod(vec![press(true)], modifiers);
            self.frame_mod(vec![press(false)], modifiers);
            self.frame(vec![]);
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
        assert_eq!(r.sel.clips.len(), 1, "right-click selects the event");
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
        assert_eq!(r.sel.clips, [r.clip().id.clone()]);

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

    /// Ctrl+click three events on two tracks, drag one: all three move by the same delta, in one undo step.
    #[test]
    fn ctrl_click_select_and_group_move() {
        let mut r = Rig::new();
        r.project.timeline.video_tracks[0].clips.push(Clip::new("c2", 8.0, 2.0));
        r.project.timeline.audio_tracks[0].clips.push(Clip::new("a", 3.0, 2.0));
        let ctrl = Modifiers { ctrl: true, command: true, ..Default::default() };
        // Lanes start at x 255, 80 px/s; Video 1 at y 48..112, Audio 1 below it.
        for p in [pos2(575.0, 90.0), pos2(935.0, 90.0), pos2(575.0, 150.0)] {
            r.click(p, ctrl);
        }
        assert_eq!(r.sel.clips.len(), 3);
        r.click(pos2(935.0, 90.0), ctrl); // Ctrl+click again deselects
        assert_eq!(r.sel.clips.len(), 2);
        r.click(pos2(935.0, 90.0), ctrl);
        r.drag(pos2(575.0, 90.0), pos2(655.0, 90.0));
        let tl = &r.project.timeline;
        let pos: Vec<f64> = tl.video_tracks[0].clips.iter().chain(&tl.audio_tracks[0].clips).map(|c| c.position).collect();
        assert!(pos.iter().zip([3.0, 9.0, 4.0]).all(|(a, b)| (a - b).abs() < 1e-6), "{pos:?}");
        assert_eq!(r.history.lens().0, 1);
        // A plain click replaces the selection.
        r.click(pos2(575.0, 150.0), Modifiers::NONE);
        assert_eq!(r.sel.clips, [r.project.timeline.audio_tracks[0].clips[0].id.clone()]);
    }

    /// Header clicks give the track list the focus (Delete then removes tracks); any lane click takes it back, also
    /// below the last track. Ctrl+drag of a bottom edge sizes every selected track without an undo step; a drag
    /// from the colour strip reorders in one undo step.
    #[test]
    fn track_focus_heights_and_reorder() {
        let mut r = Rig::new();
        r.project.timeline.add_video_track(); // V1 48..112, V2 112..176, A1 176..244
        let ids: Vec<String> = r.project.timeline.video_tracks.iter().map(|t| t.id.clone()).collect();
        let ctrl = Modifiers { ctrl: true, command: true, ..Default::default() };
        // Header clicks below the name row (sliders at +27..+41), so the quick clicks don't open a rename.
        r.click(pos2(575.0, 90.0), Modifiers::NONE); // the event
        r.click(pos2(100.0, 100.0), Modifiers::NONE);
        r.click(pos2(100.0, 165.0), ctrl);
        assert!(r.sel.list_focus && r.sel.clips.len() == 1);
        assert_eq!(r.sel.tracks, ids);
        r.click(pos2(600.0, 300.0), Modifiers::NONE); // empty space below the tracks
        assert!(!r.sel.list_focus && r.sel.clips.is_empty());
        r.click(pos2(100.0, 100.0), Modifiers::NONE);
        r.click(pos2(100.0, 165.0), ctrl);
        assert_eq!(r.history.lens().0, 0);

        r.drag_mod(pos2(100.0, 111.0), pos2(100.0, 131.0), ctrl);
        let h: Vec<f32> = r.project.timeline.video_tracks.iter().chain(&r.project.timeline.audio_tracks).map(|t| t.height).collect();
        assert_eq!(h, [84.0, 84.0, 0.0]);
        assert!(r.project.modified && r.history.lens().0 == 0);

        // V1 is now 48..132 and V2 132..216: select V1 alone and drag its strip past V2's middle.
        r.click(pos2(100.0, 120.0), Modifiers::NONE);
        r.drag(pos2(12.0, 90.0), pos2(12.0, 190.0));
        let order: Vec<String> = r.project.timeline.video_tracks.iter().map(|t| t.id.clone()).collect();
        assert_eq!(order, [ids[1].clone(), ids[0].clone()]);
        assert_eq!(r.history.lens().0, 1);
    }

    /// With Quantize to Frames on, a move whose edge snaps to an off-grid event edge stays on that edge.
    #[test]
    fn snapped_move_is_not_quantized_off_its_target() {
        let mut r = Rig::new();
        r.quantize = true;
        r.project.timeline.video_tracks[0].clips.push(Clip::new("b", 10.0213, 2.0));
        // Drag the 2..6 s event right by ~4 s: its right edge (now ~10) snaps to 10.0213.
        r.drag(pos2(575.0, 90.0), pos2(575.0 + 4.0 * 80.0 + 1.0, 90.0));
        let c = r.clip();
        assert!((c.position + c.duration - 10.0213).abs() < 1e-9, "end {}", c.position + c.duration);
    }

    /// Video with audio (2 s) and a still.
    fn pool() -> Vec<MediaItem> {
        let m = |id: &str, media_type, sample_rate| MediaItem { id: id.into(), path: id.into(), name: id.into(), media_type, duration: 2.0, sample_rate, ..Default::default() };
        vec![m("v.mp4", MediaType::Video, Some(48_000)), m("s.png", MediaType::Image, None)]
    }

    /// (track index, position, duration) of every event on the tracks of one kind.
    fn events(p: &Project, video: bool) -> Vec<(usize, f64, f64)> {
        p.timeline.tracks(video).iter().enumerate().flat_map(|(i, t)| t.clips.iter().map(move |c| (i, c.position, c.duration))).collect()
    }

    /// Drags from Project Media: onto Video 1 (its audio on the free Audio 1), several items below the last track (new
    /// tracks, across time), and onto a locked track (the "no" cursor, nothing added). Lanes start at x 255, 80 px/s;
    /// Video 1 is y 48..112 and Audio 1 112..180.
    #[test]
    fn media_drops_onto_a_track_and_below_the_last_track() {
        let mut r = Rig::new();
        r.project.media_pool = pool();
        assert_ne!(r.media_drop(vec![0], pos2(255.0 + 8.0 * 80.0, 80.0)), CursorIcon::NotAllowed);
        assert_eq!(events(&r.project, true), [(0, 2.0, 4.0), (0, 8.0, 2.0)]);
        assert_eq!(events(&r.project, false), [(0, 8.0, 2.0)]);
        assert_eq!((r.history.lens().0, r.sel.clips.len()), (1, 2), "one undo step; the new events are selected");

        r.media_drop(vec![0, 1], pos2(255.0 + 80.0, 300.0));
        assert_eq!(events(&r.project, true)[2..], [(1, 1.0, 2.0), (1, 3.0, 5.0)]);
        assert_eq!(events(&r.project, false)[1..], [(1, 1.0, 2.0)]);
        assert_eq!((r.history.lens().0, r.sel.clips.len()), (2, 3));

        r.project.timeline.video_tracks[0].locked = true;
        assert_eq!(r.media_drop(vec![1], pos2(255.0 + 10.0 * 80.0, 80.0)), CursorIcon::NotAllowed);
        assert_eq!((events(&r.project, true).len(), r.history.lens().0), (4, 2));
    }

    /// Files from Windows: the timeline reports where they were dropped (by track id) for the app to place once probed;
    /// a drop outside the lanes is left to the app (import only).
    #[test]
    fn file_drops_report_track_and_time() {
        let mut r = Rig::new();
        r.drops.files = vec!["v.mp4".into()];
        r.drops.items = Some(pool()[..1].to_vec());
        r.drops.os_pos = Some(pos2(255.0 + 8.0 * 80.0, 150.0)); // over Audio 1
        r.frame(vec![]);
        assert!(r.drops.dropped_at.is_none() && events(&r.project, false).is_empty());
        r.drops.os_dropped = true;
        r.frame(vec![]);
        let a1 = r.project.timeline.audio_tracks[0].id.clone();
        assert_eq!(r.drops.dropped_at.take(), Some((vec!["v.mp4".into()], Some(a1), 8.0)));
        assert!(!r.drops.os_dropped);
        r.drops.os_pos = Some(pos2(100.0, 150.0)); // over the track list
        r.drops.os_dropped = true;
        r.frame(vec![]);
        assert!(r.drops.dropped_at.is_none() && r.drops.os_dropped);
    }
}
