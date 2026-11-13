//! Custom-painted timeline (§5.9): time display, loop/marker/ruler band, track headers, lanes with events,
//! playhead, snap indicators and scrollbars. Interaction runs first and painting after, so edits show this frame.
mod ctx;
mod envelopes;
mod events;
mod ruler;
mod scroll;
mod tracks;

pub(super) use tracks::{fader_gain, fader_pos};
pub(super) use ctx::MENUS as CONTEXT_MENUS;
pub use ruler::grid_line;

use eframe::egui::*;
use crate::audio::Waveforms;
use crate::decode::Thumbs;
use crate::edit;
use crate::history::History;
use crate::project::*;
use super::theme::{self, Theme, UiLook};
use super::{ClipRef, DragMode, DragState, EditOpts, EditTool, EnvDrag, EnvSel, Selection};
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
    /// Grid lines (step, phase) at this zoom, and whether drags snap to them (Snap to Grid).
    grid: (f64, f64),
    grid_snap: bool,
}

impl Geo {
    /// Snap targets: `points`, plus the grid with Snap to Grid.
    fn snaps(&self, points: Vec<f64>) -> edit::Snaps {
        edit::Snaps { points, grid: self.grid_snap.then_some(self.grid) }
    }
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

#[derive(Clone, PartialEq)]
enum Rename {
    Track(bool, usize),
    Marker(usize),
    /// An event's active take (Take › Rename Active), by event id.
    Event(String),
}

fn rename_id() -> Id {
    Id::new("milan_rename_event")
}

/// Take › Rename Active: the timeline opens a rename box on event `id` next frame.
pub fn rename_event(ctx: &Context, id: &str) {
    ctx.data_mut(|d| d.insert_temp(rename_id(), id.to_string()));
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
    /// Rubber-band event selection: where it started, and whether it adds to the selection (Ctrl).
    rubber: Option<(Pos2, bool)>,
    /// Envelope tool drag on an empty lane: where it started (it selects the points in its time span).
    env_span: Option<Pos2>,
    /// The envelope point menu's Set To value being typed.
    env_text: Option<String>,
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

/// The zoom at which one filmstrip frame of video track `tr` (frames of `aspect`) is one frame long (Alt+Up).
pub fn frame_per_thumb_zoom(tr: &Track, aspect: f32, fps: f64) -> f64 {
    let t = theme::get();
    events::thumb_w(&t, track_h(tr, &t) - 3.0, aspect) as f64 * exact_fps(fps.max(1.0)) / BASE_PPS
}

/// Edge-trimming mode: a red bracket on the selected edge, its serifs pointing into the event.
fn paint_edge_bracket(p: &Painter, g: &Geo, rows: &[Row], project: &Project, edge: Option<&super::EdgeSel>) {
    let Some(e) = edge else { return };
    let (Some(r), Some(c)) = (project.find_clip(&e.clip), project.clip(&e.clip)) else { return };
    let Some(row) = rows.iter().find(|w| w.is_video == r.is_video && w.idx == r.track_idx) else { return };
    let x = g.to_x(if e.end { c.position + c.duration } else { c.position }).round() + if e.end { -1.0 } else { 1.0 };
    let (y0, y1, dx) = (row.top + 2.0, row.top + row.h - 3.0, if e.end { -6.0 } else { 6.0 });
    let s = Stroke::new(2.0_f32, Color32::from_rgb(0xFF, 0x20, 0x20));
    p.with_clip_rect(g.lanes).line(vec![pos2(x + dx, y0), pos2(x, y0), pos2(x, y1), pos2(x + dx, y1)], s);
}

fn set_zoom(tl: &mut Timeline, zoom: f64) {
    tl.zoom = zoom.clamp(0.0005, 400.0);
}

/// Put back what an event drag edits (tracks, markers, regions, time selection) as it was in `b`.
fn restore_edits(tl: &mut Timeline, b: &Timeline) {
    tl.video_tracks.clone_from(&b.video_tracks);
    tl.audio_tracks.clone_from(&b.audio_tracks);
    tl.markers.clone_from(&b.markers);
    tl.regions.clone_from(&b.regions);
    tl.loop_region = b.loop_region;
}

/// Returns the ripple of an event drag released this frame that changed something (for Post-Edit Ripple).
#[allow(clippy::too_many_arguments)]
pub fn draw_timeline(
    ui: &mut Ui,
    project: &mut Project,
    sel: &mut Selection,
    drag_state: &mut Option<DragState>,
    env_drag: &mut Option<EnvDrag>,
    scroll_y: &mut f32,
    history: &mut History,
    opts: EditOpts,
    waves: &mut Waveforms,
    thumbs: &mut Thumbs,
    playing: bool,
    track_w: &mut f32,
    drops: &mut super::FileDrag,
) -> Option<Vec<edit::RippleOp>> {
    let t = theme::get();
    let v16 = t.look() == UiLook::Vegas16;
    let rect = ui.max_rect();
    if rect.width() < 80.0 || rect.height() < t.m.band_h + 40.0 {
        return None;
    }
    let (snapping, quantize, tool) = (opts.snapping, opts.quantize, opts.tool);
    // The Envelope and Selection tools leave events where they are; Slip clicks ignore grouping (as in VEGAS), but a
    // slip-trim still takes the group mates along.
    let moves_events = !matches!(tool, EditTool::Envelope | EditTool::Selection);
    let grouping = !opts.ignore_grouping;
    let click_groups = grouping && tool != EditTool::Slip;
    let mut released_op = None;
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
    let make_geo = |project: &Project| {
        let tl = &project.timeline;
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
            grid: ruler::grid_line(opts.grid, &project.settings, tl.zoom * BASE_PPS),
            grid_snap: opts.snap_grid,
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
    let mut g = make_geo(project);
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
    g = make_geo(project);

    ruler::interact(ui, &g, project, history, &mut st, snapping);
    scroll::interact(ui, &g, &rows, project, &mut st, scroll_y);
    g = make_geo(project);
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
            // The level line: at the top edge at full level, lower as the level drops.
            let on_level = level_of(c, row.is_video).is_some_and(|f| {
                let ly = top + (1.0 - f) * (row.h - 3.0);
                p.y >= ly - 3.0 && p.y <= ly + FADE_PX
            });
            let mode = if near_top && (p.x - fi).abs() <= FADE_PX && p.x - x0 < x1 - p.x {
                DragMode::FadeIn
            } else if near_top && (p.x - fo).abs() <= FADE_PX {
                DragMode::FadeOut
            } else if p.x - x0 <= EDGE_PX {
                DragMode::TrimLeft
            } else if x1 - p.x <= EDGE_PX {
                DragMode::TrimRight
            } else if on_level {
                DragMode::Level
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

    // Envelopes (View › Show Envelopes; every tool but Selection) are hit before events, on unlocked tracks, but an
    // event's fade handle beats an envelope line through it. The Envelope tool never hits events.
    let env_on = opts.show_envelopes && tool != EditTool::Selection;
    let env_hit = |project: &Project, p: Pos2| {
        let on_fade = || tool != EditTool::Envelope && hit_clip(project, p).is_some_and(|(_, m)| matches!(m, DragMode::FadeIn | DragMode::FadeOut));
        env_on
            .then(|| envelopes::hit(&g, &rows, project, p))
            .flatten()
            .filter(|h| !project.timeline.tracks(h.is_video)[h.track].locked && (h.point.is_some() || !on_fade()))
    };
    let idle = drag_state.is_none() && env_drag.is_none();
    let hovered_env = response.hover_pos().filter(|_| idle).and_then(|p| env_hit(project, p));
    let hovered_clip = response.hover_pos().filter(|_| hovered_env.is_none() && tool != EditTool::Envelope).and_then(|p| hit_clip(project, p));
    let env_cursor = |segment: bool| if segment { CursorIcon::ResizeVertical } else { CursorIcon::PointingHand };
    if let Some(ed) = env_drag.as_ref() {
        ui.ctx().set_cursor_icon(env_cursor(ed.segment));
    } else if let Some(h) = hovered_env {
        ui.ctx().set_cursor_icon(env_cursor(h.point.is_none()));
    }

    // Trim / fade / level cursor on event edges, corners and the level line, kept for the whole drag.
    let drag_cursor = |mode: DragMode| match mode {
        DragMode::Move => None,
        DragMode::Level => Some(CursorIcon::ResizeVertical),
        _ => Some(CursorIcon::ResizeHorizontal),
    };
    if let Some(ds) = drag_state.as_ref() {
        if let Some(c) = drag_cursor(ds.mode) {
            ui.ctx().set_cursor_icon(c);
        }
    } else if let Some((cr, mode)) = &hovered_clip {
        if let Some(c) = drag_cursor(*mode).filter(|_| moves_events && !is_locked(project, cr)) {
            ui.ctx().set_cursor_icon(c);
        }
    }

    // Event drag: move / trim / fade. On an empty lane, VP16 drags out a time selection and VP23 a rubber band (its
    // right-drag makes the time selection); the Selection tool rubber-bands anywhere.
    let press = ui.input(|i| i.pointer.press_origin()).filter(|p| g.lanes.contains(*p));
    // An envelope point (a selected one takes the selection along) or segment; the Envelope tool's drag on an empty
    // lane selects the points in its time span.
    let env_press = press.and_then(|p| env_hit(project, p));
    if response.drag_started_by(PointerButton::Primary) && (env_press.is_some() || tool == EditTool::Envelope) {
        if let (Some(p), Some(h)) = (press, env_press) {
            let tr = &project.timeline.tracks(h.is_video)[h.track];
            let e = &tr.envelopes[h.env];
            let picked = sel.env.as_ref().filter(|s| s.track == tr.id && s.kind == e.kind && h.point.is_some_and(|k| s.keys.contains(&k)));
            let keys = match (picked, h.point) {
                (Some(s), _) => s.keys.clone(),
                (None, Some(k)) => vec![k],
                (None, None) => [h.seg, h.seg + 1].into_iter().filter(|&k| k < e.points.keys.len()).collect(),
            };
            if h.point.is_some() {
                sel.env = Some(EnvSel { track: tr.id.clone(), kind: e.kind, keys: keys.clone() });
            }
            let (track, kind, orig) = (tr.id.clone(), e.kind, e.clone());
            *env_drag = Some(EnvDrag { track, kind, keys, grab: h.seg, segment: h.point.is_none(), orig, press_t: g.to_time(p.x), press_y: p.y, before: project.clone(), changed: false });
        } else if let Some(p) = press {
            st.env_span = Some(p);
        }
    } else if response.drag_started_by(PointerButton::Primary) {
        if let Some(p) = press {
            let hit = hit_clip(project, p);
            // Slip, Slide and Time Stretch are Normal-tool variants: only their event drags differ.
            let normal = matches!(tool, EditTool::Normal | EditTool::Slip | EditTool::Slide | EditTool::Stretch);
            if tool == EditTool::Selection || (hit.is_none() && !v16 && normal) {
                st.rubber = Some((p, ui.input(|i| i.modifiers.command || i.modifiers.shift)));
            } else if let Some((cr, mode)) = hit.clone().filter(|(cr, _)| moves_events && !is_locked(project, cr)) {
                let clip = project.timeline.tracks(cr.is_video)[cr.track_idx].clips[cr.clip_idx].clone();
                let m = ui.input(|i| i.modifiers);
                let mut mode = mode.with_tool(tool, m.command, m.alt);
                // The other side of a roll at this edit point, unlocked.
                let partner = |id: &str, right: bool| {
                    edit::adjacent(project, id, right, frame).filter(|n| project.find_clip(n).is_some_and(|r| !is_locked(project, &r)))
                };
                // A slide inside a crossfade slides the crossfade: a roll of the incoming event's start.
                let t_press = g.to_time(p.x);
                let fade_from = partner(&clip.id, false).and_then(|n| project.clip(&n).map(|a| a.position + a.duration));
                if mode == DragMode::Slide && fade_from.is_some_and(|e| e > clip.position + frame && t_press < e) {
                    mode = DragMode::RollLeft;
                }
                // A roll with nothing to roll against is a plain trim (group mates and ripple included).
                match mode {
                    DragMode::RollLeft if partner(&clip.id, false).is_none() => mode = DragMode::TrimLeft,
                    DragMode::RollRight if partner(&clip.id, true).is_none() => mode = DragMode::TrimRight,
                    _ => {}
                }
                let right = mode.right_edge();
                let edge = match mode {
                    _ if right => clip.position + clip.duration,
                    DragMode::FadeIn => clip.position + clip.fade_in,
                    DragMode::FadeOut => clip.position + clip.duration - clip.fade_out,
                    _ => clip.position,
                };
                // Dragging an unselected event selects it (and its group) alone; a selected one moves the whole selection.
                if !sel.clips.contains(&clip.id) {
                    sel.clips = if click_groups { edit::expand_groups(project, &[clip.id.clone()]) } else { vec![clip.id.clone()] };
                }
                sel.tracks = vec![project.timeline.tracks(cr.is_video)[cr.track_idx].id.clone()];
                (sel.list_focus, sel.env) = (false, None);
                // A trim, slip-trim, stretch or slide takes the group mates whose edge lines up with it along, each
                // within its own media. A roll takes pairs: the event its edge meets, then each such group mate and
                // the event that one's edge meets.
                let mates = match mode {
                    DragMode::RollLeft | DragMode::RollRight => {
                        let mut v: Vec<String> = partner(&clip.id, right).into_iter().collect();
                        // shortcut: a group mate with no unlocked neighbour of its own stays put; trim it if asked.
                        for mate in edit::edge_mates(project, &clip.id, !right, frame).into_iter().filter(|_| grouping) {
                            let free = project.find_clip(&mate).is_some_and(|r| !is_locked(project, &r));
                            if let Some(n) = partner(&mate, right).filter(|_| free) {
                                v.extend([mate, n]);
                            }
                        }
                        v
                    }
                    _ if !grouping || matches!(mode, DragMode::Move | DragMode::Slip | DragMode::Level | DragMode::FadeIn | DragMode::FadeOut) => Vec::new(),
                    _ => edit::edge_mates(project, &clip.id, !right, frame),
                };
                let others: Vec<(Clip, Option<f64>)> = mates
                    .iter()
                    .filter_map(|id| project.find_clip(id).filter(|r| !is_locked(project, r)).and(project.clip(id)))
                    .map(|c| (c.clone(), edit::media_limit(project, c)))
                    .collect();
                let ids = if mode == DragMode::Move { sel.clips.clone() } else { std::iter::once(clip.id.clone()).chain(others.iter().map(|o| o.0.id.clone())).collect() };
                let moving: Vec<&str> = ids.iter().map(String::as_str).collect();
                let refs = project.find_clips(&ids);
                let mut tracks: Vec<String> = refs.iter().map(|r| project.timeline.tracks(r.is_video)[r.track_idx].id.clone()).collect();
                tracks.sort();
                tracks.dedup();
                let from = refs.iter().map(|r| project.timeline.tracks(r.is_video)[r.track_idx].clips[r.clip_idx].position).fold(f64::INFINITY, f64::min);
                *drag_state = Some(DragState {
                    id: clip.id.clone(),
                    mode,
                    grab_offset: g.to_time(p.x) - edge,
                    limit: edit::media_limit(project, &clip),
                    // Snap to All Events off: only the edges on the dragged events' own tracks.
                    snaps: g.snaps(edit::snap_targets_except(project, &moving, (!opts.snap_all).then_some(&tracks[..]))),
                    ids,
                    original: clip,
                    others,
                    op: Vec::new(),
                    tracks,
                    from,
                    before: project.clone(),
                    changed: false,
                });
            } else if v16 && hit.is_none() {
                let t0 = ruler::snapped(ui, project, g.to_time(p.x).max(0.0), &g, snapping, &mut st);
                st.loop_drag = Some(LoopDrag::Create(t0));
            }
        }
    }
    if response.drag_started_by(PointerButton::Secondary) && !v16 {
        if let Some(p) = press.filter(|p| hit_clip(project, *p).is_none()) {
            let t0 = ruler::snapped(ui, project, g.to_time(p.x).max(0.0), &g, snapping, &mut st);
            st.loop_drag = Some(LoopDrag::Create(t0));
        }
    }
    if let Some(ds) = drag_state.as_mut() {
        if let Some(p) = response.interact_pointer_pos().filter(|_| response.dragged()) {
            let thr = SNAP_PX / g.pps;
            let edge = g.to_time(p.x) - ds.grab_offset;
            let snap = if snap_on { ds.snaps.nearest(edge, thr) } else { None };
            let snapped_edge = snap.unwrap_or(edge);
            let q = |t: f64| if quantize { edit::quantize(t, fps) } else { t };
            let trim_edge = snap.unwrap_or_else(|| q(edge));
            // Every frame starts from the project as it was before the drag, so ripples and crossfades never stack.
            restore_edits(&mut project.timeline, &ds.before.timeline);
            let o = ds.original.clone();
            let tracks = ds.tracks.clone();
            let mut op = Vec::new();
            // Where a move or slide puts the grabbed event: it snaps, or else quantizes (a snapped edge stays on its
            // target even off the frame grid).
            let mut span_start = || {
                let mut pos = edge.max(0.0);
                let mut hit = None;
                if snap_on {
                    pos = ds.snaps.span(pos, o.duration, thr).max(0.0);
                    hit = [pos, pos + o.duration].into_iter().find(|v| ds.snaps.hits(*v));
                    st.snap = hit;
                }
                if hit.is_some() { pos } else { q(pos) }
            };

            if ds.mode == DragMode::Move {
                // Every selected event gets the grabbed one's delta.
                let dt = span_start() - o.position;
                let from = ds.before.find_clip(&ds.id);
                let dtrack = row_at(p.y).zip(from).filter(|(r, f)| r.is_video == f.is_video).map_or(0, |(r, f)| r.idx as isize - f.track_idx as isize);
                // A hop onto a missing or locked track still moves in time.
                // Lock Envelopes to Events: points go with the events (a ripple already moves them along their tracks).
                let lock = opts.lock_envelopes;
                let hopped = dtrack != 0 && edit::move_clips(project, &ds.ids, dt, dtrack, lock);
                ds.changed = hopped || edit::move_clips(project, &ds.ids, dt, 0, lock && opts.ripple.is_none());
                // A move along its tracks ripples from where the events were; one to other tracks does not.
                let moved = project.clip(&ds.id).map_or(0.0, |c| c.position - o.position);
                if !hopped && moved != 0.0 {
                    op = vec![edit::RippleOp { from: ds.from, delta: moved, tracks, exclude: ds.ids.clone(), keys_from: None }];
                }
            } else {
                use DragMode::*;
                let mode = ds.mode;
                // Slip and slide move by the pointer's travel (whole frames with Quantize to Frames); a slide snaps the
                // event like a move. Edges snap or quantize as trims do.
                let d = match mode {
                    Slide => span_start() - o.position,
                    _ => q(edge - o.position),
                };
                if !matches!(mode, Slip | Slide | Level) {
                    st.snap = snap;
                }
                let press_y = ui.input(|i| i.pointer.press_origin()).map_or(p.y, |s| s.y);
                let put = |project: &mut Project, c: Clip| {
                    if let Some(r) = project.find_clip(&c.id) {
                        project.timeline.tracks_mut(r.is_video)[r.track_idx].clips[r.clip_idx] = c;
                    }
                };
                if matches!(mode, RollLeft | RollRight) {
                    // Each edit point (the grabbed one, then its group mates'): the left event and the right one,
                    // each with its media limit. The incoming event starts at the dragged edge (a right edge only
                    // meets its neighbour, so there is no crossfade to keep).
                    let all: Vec<(&Clip, Option<f64>)> = std::iter::once((&ds.original, ds.limit)).chain(ds.others.iter().map(|(c, l)| (c, *l))).collect();
                    for pair in all.chunks_exact(2) {
                        let ((oa, la), (ob, lb)) = if mode == RollRight { (pair[0], pair[1]) } else { (pair[1], pair[0]) };
                        let (mut a, mut b) = (oa.clone(), ob.clone());
                        edit::roll(&mut a, oa, la, &mut b, ob, lb, trim_edge, frame);
                        put(project, a);
                        put(project, b);
                    }
                } else {
                    for (orig, limit) in std::iter::once((&ds.original, ds.limit)).chain(ds.others.iter().map(|(c, l)| (c, *l))) {
                        let Some(cr) = project.find_clip(&orig.id) else { continue };
                        let lane_h = rows.iter().find(|r| r.is_video == cr.is_video && r.idx == cr.track_idx).map_or(1.0, |r| (r.h - 3.0).max(1.0));
                        let clip = &mut project.timeline.tracks_mut(cr.is_video)[cr.track_idx].clips[cr.clip_idx];
                        match mode {
                            Move => {}
                            TrimLeft | RollLeft => edit::trim_left(clip, orig, trim_edge, frame, limit),
                            TrimRight | RollRight => edit::trim_right(clip, orig, trim_edge, frame, limit),
                            SlipTrimLeft => edit::slip_trim_left(clip, orig, trim_edge, frame, limit),
                            SlipTrimRight => edit::slip_trim_right(clip, orig, trim_edge, frame, limit),
                            // A stretch or slide stopped by a limit is pulled back onto the frame grid, inside the limit.
                            StretchLeft | StretchRight => {
                                let left = mode == StretchLeft;
                                edit::stretch(clip, orig, trim_edge, left, frame);
                                if quantize && (clip.rate == edit::MIN_RATE || clip.rate == edit::MAX_RATE) {
                                    let fixed = if left { orig.position + orig.duration } else { orig.position };
                                    let e = if left { clip.position } else { clip.position + clip.duration };
                                    // At the top rate the event may only grow; at the bottom one only shrink.
                                    let toward = if clip.rate == edit::MAX_RATE { 2.0 * e - fixed } else { fixed };
                                    edit::stretch(clip, orig, edit::quantize_toward(e, fps, toward), left, frame);
                                }
                            }
                            Slip => edit::slip(clip, orig, d, limit),
                            Slide => {
                                edit::slide(clip, orig, d, limit);
                                if quantize && (clip.position - orig.position - d).abs() > 1e-9 {
                                    let to = edit::quantize_toward(clip.position, fps, orig.position) - orig.position;
                                    edit::slide(clip, orig, if to * d > 0.0 { to } else { 0.0 }, limit);
                                }
                            }
                            FadeIn => {
                                let len = ((snapped_edge - clip.position) / frame).round() * frame;
                                clip.fade_in = len.clamp(0.0, (clip.duration - clip.fade_out).max(0.0));
                            }
                            FadeOut => {
                                let len = ((clip.position + clip.duration - snapped_edge) / frame).round() * frame;
                                clip.fade_out = len.clamp(0.0, (clip.duration - clip.fade_in).max(0.0));
                            }
                            Level => {
                                let f = (level_of(orig, cr.is_video).unwrap_or(1.0) - (p.y - press_y) / lane_h).clamp(0.0, 1.0);
                                if cr.is_video {
                                    clip.set_opacity(f);
                                } else {
                                    clip.gain_db = edit::level_db(f);
                                }
                            }
                        }
                    }
                }
                if let Some(c) = project.clip(&ds.id) {
                    ds.changed = *c != o;
                    if matches!(mode, TrimLeft | TrimRight | SlipTrimLeft | SlipTrimRight | StretchLeft | StretchRight) {
                        op = edit::trim_ripple(&ds.before, &o, c, &ds.ids, &tracks);
                    }
                }
            }
            if let Some(mode) = opts.ripple {
                edit::ripple_all(project, &op, mode);
            }
            // A fade handle on a crossfade sets that fade by hand: it is not reset to the overlap. A slip or a level
            // change leaves the events where they are.
            if opts.auto_crossfade && !matches!(ds.mode, DragMode::FadeIn | DragMode::FadeOut | DragMode::Slip | DragMode::Level) {
                edit::auto_crossfade(project, &ds.before, &ds.ids);
            }
            ds.op = op;
            project.modified = ds.before.modified; // marked on release, if anything changed
        }
    }
    // Envelope drag: Ctrl fine-adjusts values (a tenth as fast) and keeps the time; a point's time snaps (Shift: off)
    // or lands on a frame.
    if let Some(ed) = env_drag.as_mut() {
        if let Some(p) = response.interact_pointer_pos().filter(|_| response.dragged()) {
            let row = project.find_track(&ed.track).and_then(|(v, i)| rows.iter().find(|r| r.is_video == v && r.idx == i));
            let mut df = row.map_or(0.0, |r| -envelopes::frac_of(r, p.y - ed.press_y));
            let fine = ui.input(|i| i.modifiers.command);
            if fine {
                df /= 10.0;
            }
            let mut dt = 0.0;
            if !ed.segment && !fine {
                let k0 = ed.orig.points.keys.get(ed.grab).map_or(0.0, |k| k.t);
                let want = k0 + g.to_time(p.x) - ed.press_t;
                let snap = if snap_on { g.snaps(edit::snap_targets(project, None)).nearest(want, SNAP_PX / g.pps) } else { None };
                st.snap = snap;
                dt = snap.unwrap_or(if quantize && !shift { edit::quantize(want.max(0.0), fps) } else { want }) - k0;
            }
            if let Some((v, i)) = project.find_track(&ed.track) {
                if let Some(e) = project.timeline.tracks_mut(v)[i].envelopes.iter_mut().find(|e| e.kind == ed.kind) {
                    ed.changed = edit::env_move(e, &ed.orig, &ed.keys, dt, df);
                }
            }
        }
    }
    // Empty-lane time selection (VP16), or scrubbing a drag that started on the ruler.
    if drag_state.is_none() && env_drag.is_none() && response.dragged() {
        let origin = ui.input(|i| i.pointer.press_origin());
        if let (Some(o), Some(p)) = (origin, response.interact_pointer_pos()) {
            if let Some(d) = st.loop_drag {
                let tp = ruler::snapped(ui, project, g.to_time(p.x), &g, snapping, &mut st);
                ruler::loop_drag_to(&mut project.timeline, d, tp);
            } else if g.band.contains(o) {
                project.timeline.cursor_position = cursor_at(g.to_time(p.x.clamp(g.lanes.min.x, g.lanes.max.x)));
            }
        }
    }
    if response.drag_stopped() {
        if let Some(ed) = env_drag.take() {
            if ed.changed {
                history.push(ed.before);
                project.mark_modified();
            }
        }
        // Envelope tool span: the points of the track's top envelope in its time span become the selection.
        if let (Some(o), Some(p)) = (st.env_span.take(), ui.input(|i| i.pointer.latest_pos())) {
            let (a, b) = (g.to_time(o.x.min(p.x)), g.to_time(o.x.max(p.x)));
            let tr = row_at(o.y).map(|r| &project.timeline.tracks(r.is_video)[r.idx]).filter(|t| !t.locked);
            let top = tr.and_then(|t| envelopes::shown(t).last().map(|(_, e)| EnvSel { track: t.id.clone(), kind: e.kind, keys: edit::keys_in_span(e, a, b) }));
            sel.env = top.filter(|s| !s.keys.is_empty());
        }
        if let Some(ds) = drag_state.take() {
            if ds.changed {
                history.push(ds.before);
                project.mark_modified();
                released_op = Some(ds.op).filter(|op| !op.is_empty());
            } else {
                // Nothing moved in the end: crossfades made on the way are dropped too.
                restore_edits(&mut project.timeline, &ds.before.timeline);
            }
        }
        // Rubber band: the events it touches become the selection (Ctrl adds them); the Selection tool also makes
        // its time span the time selection.
        if let (Some((o, add)), Some(p)) = (st.rubber.take(), ui.input(|i| i.pointer.latest_pos())) {
            let r = Rect::from_two_pos(o, p);
            let mut hits = clips_in_rect(project, &rows, &g, r);
            if click_groups {
                hits = edit::expand_groups(project, &hits);
            }
            if add {
                hits.retain(|h| !sel.clips.contains(h));
                sel.clips.extend(hits);
            } else {
                sel.clips = hits;
            }
            (sel.list_focus, sel.env) = (false, None);
            if tool == EditTool::Selection && r.width() >= 1.0 {
                project.timeline.loop_region = Some((g.to_time(r.min.x).max(0.0), g.to_time(r.max.x).max(0.0)));
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
        let s = if snapping { g.snaps(edit::snap_targets(project, None)).nearest(t, SNAP_PX / g.pps) } else { None };
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
    // A plug-in tile dropped on the lanes: video FX on the video event under it, a transition on the edge or overlap
    // there, generated media at the drop time. One undo step; the FX window then opens on what it made.
    if let Some(d) = super::plugin_dropped(&response) {
        if let Some(p) = ui.input(|i| i.pointer.latest_pos()).filter(|p| g.lanes.contains(*p)) {
            use crate::engine::plugins::PluginKind;
            let (before, row) = (project.clone(), row_at(p.y).filter(|r| r.is_video));
            let track = row.map(|r| project.timeline.video_tracks[r.idx].id.clone());
            let (target, refused) = match d.kind {
                PluginKind::VideoFx => {
                    let ev = hit_clip(project, p).filter(|(cr, _)| cr.is_video).map(|(cr, _)| edit::FxTarget::Event(project.timeline.video_tracks[cr.track_idx].clips[cr.clip_idx].id.clone()));
                    (ev.filter(|t| edit::add_fx(project, t, d.fx.clone())), "Drop video FX on an unlocked video event, a video track header or the Video Preview.")
                }
                PluginKind::Transition => {
                    let tr = edit::transition_of(&d.fx);
                    let done = track.as_ref().is_some_and(|t| edit::set_transition(project, t, g.to_time(p.x).max(0.0), tr.clone(), opts.cut_overlap, opts.auto_crossfade));
                    // The event that took it: the last one whose transition is now this one and that changed.
                    let tl = &project.timeline;
                    let into = row.filter(|_| done).and_then(|r| tl.video_tracks[r.idx].clips.iter().rev().find(|c| c.transition.as_ref() == Some(&tr) && before.clip(&c.id) != Some(c)));
                    let why = "A transition goes on an event's start, a cut (with Automatic Crossfades on) or a crossfade on an unlocked video track; Milan Pro has none on a fade-out.";
                    (into.map(|c| edit::FxTarget::Transition(c.id.clone())), why)
                }
                PluginKind::Generator => {
                    let at = drop_time(project, p.x);
                    // Over a video track: on it; below the last track: on a new one; over an audio track: refused.
                    let track = match row_at(p.y) {
                        Some(r) if !r.is_video => None,
                        r => Some(r.map_or(project.timeline.video_tracks.len(), |r| r.idx)),
                    };
                    let id = track.and_then(|i| edit::insert_generated(project, Some(i), at, d.fx.clone(), &d.name, drops.still_len));
                    if let Some(id) = &id {
                        (sel.clips, sel.list_focus, sel.env) = (vec![id.clone()], false, None);
                    }
                    let why = if track.is_some() { "The track is locked." } else { "Drop generated media on a video track." };
                    (id.and_then(|id| project.clip(&id)?.media_id.clone()).map(edit::FxTarget::Media), why)
                }
                PluginKind::AudioFx => (None, "Audio plug-ins are added with fx+ in an FX window."),
            };
            match target {
                Some(t) => {
                    history.push(before);
                    super::open_tool(ui.ctx(), super::ToolWin::Fx(t));
                }
                None => super::hint(ui.ctx(), refused),
            }
        }
    }
    let released = response.dnd_release_payload::<super::MediaDrag>();
    let right_drop = ui.input(|i| i.pointer.button_released(PointerButton::Secondary));
    let mut drop_menu = None;
    if released.is_some() || drops.os_dropped {
        if let Some((target, time, plan)) = ghost.take() {
            drops.os_dropped = false;
            match released.as_deref() {
                // A right-drag asks how to add the media first (VEGAS's drop menu).
                Some(drag @ (super::MediaDrag::Pool(..) | super::MediaDrag::File(_))) if right_drop && !plan.refused => {
                    let at = pointer.unwrap_or(g.lanes.center());
                    let (still_len, xfade) = (drops.still_len, opts.auto_crossfade);
                    let (idxs, range, files) = match drag {
                        super::MediaDrag::Pool(idxs, range) => (idxs.clone(), *range, Vec::new()),
                        super::MediaDrag::File(_) => (Vec::new(), None, drops.files.clone()),
                    };
                    drop_menu = Some((at, ctx::DropReq { target, time, idxs, range, files, still_len, xfade }));
                }
                Some(super::MediaDrag::Pool(..)) => {
                    let before = project.clone();
                    let ids = edit::apply_drop(project, plan, opts.auto_crossfade);
                    if !ids.is_empty() {
                        history.push(before);
                        (sel.clips, sel.list_focus, sel.env) = (ids, false, None);
                    }
                }
                // Files are placed by the app once probed, on the track (by id) and at the time of the drop.
                _ if !plan.refused => {
                    let track = target.map(|(v, i)| project.timeline.tracks(v)[i].id.clone());
                    drops.dropped_at = Some((drops.files.clone(), track, time, edit::DropMode::AcrossTime));
                }
                _ => {}
            }
        }
    }

    // Click: select the event under the pointer (Ctrl toggles, Shift adds the range on its track) and put the
    // cursor there, as VEGAS does; the clicked track gets the focus. A right-click on an unselected event selects
    // it, so the context menu acts on the clicked event.
    // A click on an envelope point selects it (Ctrl toggles it; a right-click keeps a selection it is in). A
    // double-click on the line (a click, with the Envelope tool) adds a point there. Events and the cursor stay.
    let clicks = response.clicked() || response.secondary_clicked() || response.double_clicked();
    let env_click = response.interact_pointer_pos().filter(|_| clicks).and_then(|p| env_hit(project, p).map(|h| (p, h)));
    if let Some((p, h)) = env_click {
        let tr = &project.timeline.tracks(h.is_video)[h.track];
        let (id, kind) = (tr.id.clone(), tr.envelopes[h.env].kind);
        let mut keys = sel.env.take().filter(|s| s.track == id && s.kind == kind).map_or(Vec::new(), |s| s.keys);
        if let Some(k) = h.point {
            if response.clicked() && ui.input(|i| i.modifiers.command) {
                if let Some(j) = keys.iter().position(|x| *x == k) {
                    keys.remove(j);
                } else {
                    keys.push(k);
                }
            } else if !(response.secondary_clicked() && keys.contains(&k)) {
                keys = vec![k];
            }
            sel.env = Some(EnvSel { track: id, kind, keys }).filter(|s| !s.keys.is_empty());
        } else if response.double_clicked() || (tool == EditTool::Envelope && response.clicked()) {
            let t = cursor_at(g.to_time(p.x));
            let (before, mut k) = (project.clone(), 0);
            if edit::edit_envelope(project, &id, kind, |e| {
                k = edit::env_insert(e, t, e.at(t));
                true
            }) {
                history.push(before);
                sel.env = Some(EnvSel { track: id, kind, keys: vec![k] });
            }
        }
    } else if response.clicked() || response.secondary_clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if g.lanes.contains(pos) || (response.secondary_clicked() && !g.band.contains(pos)) {
                let hit = hit_clip(project, pos).map(|(cr, _)| cr).filter(|_| tool != EditTool::Envelope);
                let m = ui.input(|i| i.modifiers);
                if g.lanes.contains(pos) {
                    // Any click in the lanes (also below the last track) takes the focus from the track list and
                    // drops the envelope point selection.
                    sel.list_focus = false;
                    sel.env = None;
                    if let Some(row) = row_at(pos.y) {
                        sel.tracks = vec![project.timeline.tracks(row.is_video)[row.idx].id.clone()];
                    }
                }
                match &hit {
                    Some(cr) if response.secondary_clicked() => {
                        let id = &project.timeline.tracks(cr.is_video)[cr.track_idx].clips[cr.clip_idx].id;
                        // A selected event stays selected and becomes the focus (last), so the menu acts on it.
                        if let Some(k) = sel.clips.iter().position(|c| c == id) {
                            let f = sel.clips.remove(k);
                            sel.clips.push(f);
                        } else {
                            sel.clips = vec![id.clone()];
                        }
                    }
                    Some(cr) => click_select(project, sel, cr, m.command, m.shift, click_groups),
                    None if !(m.command || m.shift) => sel.clips.clear(),
                    None => {}
                }
                // An event's fx button and a video event's Pan/Crop and Generated Media buttons open their windows.
                if let (Some(cr), Some(row), true) = (&hit, row_at(pos.y), response.clicked()) {
                    use super::icons::Icon;
                    let c = &project.timeline.tracks(cr.is_video)[cr.track_idx].clips[cr.clip_idx];
                    let (x0, x1) = (g.to_x(c.position).round(), g.to_x(c.position + c.duration).round());
                    let r = Rect::from_min_max(pos2(x0, row.top + 1.0), pos2(x1.max(x0 + 1.0), row.top + row.h - 2.0));
                    let generated = generated_media(project, c);
                    let win = events::buttons(&t, r, cr.is_video, generated.is_some()).into_iter().find(|(_, b)| b.contains(pos)).and_then(|(icon, _)| match icon {
                        Icon::PanCrop => Some(super::ToolWin::PanCrop(c.id.clone())),
                        Icon::TrackFx => Some(super::ToolWin::Fx(edit::FxTarget::Event(c.id.clone()))),
                        Icon::GeneratedMedia => generated.map(|m| super::ToolWin::Fx(edit::FxTarget::Media(m))),
                        _ => None,
                    });
                    if let Some(w) = win {
                        super::open_tool(ui.ctx(), w);
                    }
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
    ctx::menu(ui, &response, &g, &rows, project, history, sel, &mut st, env_on, (opts.cut_overlap, opts.auto_crossfade), drop_menu, drops);

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
    ruler::paint_grid(&pl, &g);

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
                ripple: opts.ripple.is_some(),
                level: level_of(clip, row.is_video),
                dim: track.muted || clip.muted,
                generated: media.is_some_and(|m| m.generator.is_some()),
                pps: g.pps,
                fps,
                wave_zoom: opts.wave_zoom,
            };
            let mut thumb = |src: f64| pic.and_then(|m| thumbs.get(m, src)).map(|tex| tex.id());
            paint_event(&pe, &t, &ev, t.lane_bg, wave, &mut thumb);
        }
        if row.is_video {
            paint_transitions(&pe, &t, &g, row, &track.clips);
        }
    }
    paint_edge_bracket(&pl, &g, &rows, project, sel.edge.as_ref());
    if opts.show_envelopes {
        envelopes::paint(&pl, &t, &g, &rows, project, sel.env.as_ref(), hovered_env);
        // The value of the point being dragged (or hovered).
        let tip = env_drag.as_ref().and_then(|ed| {
            let (v, i) = project.find_track(&ed.track)?;
            let k = project.timeline.tracks(v)[i].envelopes.iter().find(|e| e.kind == ed.kind)?.points.keys.get(ed.grab)?;
            Some((v, i, ed.kind, k.t, k.v))
        });
        let tip = tip.or_else(|| {
            let h = hovered_env?;
            // The context menu may have removed it this frame.
            let e = project.timeline.tracks(h.is_video).get(h.track)?.envelopes.get(h.env)?;
            let k = e.points.keys.get(h.point?)?;
            Some((h.is_video, h.track, e.kind, k.t, k.v))
        });
        if let Some((v, i, kind, kt, kv)) = tip {
            if let Some(row) = rows.iter().find(|r| r.is_video == v && r.idx == i) {
                paint_fade_tag(&pl, &t, pos2(g.to_x(kt) + 6.0, envelopes::y_of(row, kind, kv) - 18.0), envelopes::value_text(kind, kv));
            }
        }
    }
    if let (Some(o), Some(p), Some(r)) = (st.env_span, ui.input(|i| i.pointer.latest_pos()), st.env_span.and_then(|o| row_at(o.y))) {
        let x = Rangef::new(o.x.min(p.x), o.x.max(p.x));
        pl.rect_filled(Rect::from_x_y_ranges(x, Rangef::new(r.top, r.top + r.h - 2.0)), 0.0, t.accent.gamma_multiply(0.25));
    }
    ruler::paint_marker_lines(&pl, &g, project);
    if let (Some((o, _)), Some(p)) = (st.rubber, ui.input(|i| i.pointer.latest_pos())) {
        let r = Rect::from_two_pos(o, p);
        let pts = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
        pl.extend(Shape::dashed_line(&pts, Stroke::new(1.0_f32, t.accent), 4.0, 3.0));
    }
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
    ruler::paint_playhead(&painter, &g, project.timeline.cursor_position, opts.ripple.is_some());
    // Length tag while dragging a fade; the level ("-3.2 dB", "48.1%") or the rate while dragging those.
    if let Some(ds) = drag_state.as_ref() {
        if let Some((cr, c)) = project.find_clip(&ds.id).zip(project.clip(&ds.id)) {
            let mid = g.to_x(c.position + c.duration * 0.5);
            let tag = match ds.mode {
                DragMode::FadeIn => Some((ruler::short_tc(c.fade_in, fps), g.to_x(c.position + c.fade_in) + 4.0)),
                DragMode::FadeOut => Some((ruler::short_tc(c.fade_out, fps), g.to_x(c.position + c.duration - c.fade_out) - 44.0)),
                DragMode::Level if cr.is_video => Some((format!("{:.1}%", c.opacity() * 100.0), mid)),
                DragMode::Level if c.gain_db <= -120.0 => Some(("-Inf dB".into(), mid)),
                DragMode::Level => Some((format!("{:.1} dB", c.gain_db), mid)),
                DragMode::StretchLeft | DragMode::StretchRight => Some((format!("Rate {:.1}%", c.rate * 100.0), mid)),
                _ => None,
            };
            if let (Some((text, x)), Some(row)) = (tag, rows.iter().find(|r| r.is_video == cr.is_video && r.idx == cr.track_idx)) {
                paint_fade_tag(&pl, &t, pos2(x, row.top + row.h * 0.5 - 7.0), text);
            }
        }
    }
    scroll::paint(&painter, &g, &rows, project, *scroll_y);

    rename_box(ui, &g, &rows, project, history, &mut st);
    ui.data_mut(|d| d.insert_temp(sid, st));
    released_op
}

/// The id of event `c`'s media when it is generated.
fn generated_media(project: &Project, c: &Clip) -> Option<String> {
    let id = c.media_id.as_ref()?;
    project.media_pool.iter().find(|m| m.id == *id && m.generator.is_some()).map(|m| m.id.clone())
}

/// The Transition Progress colour (spec §2 envelope colours), the same in both looks.
const TRANSITION: Color32 = Color32::from_rgb(0xBF, 0x7F, 0xD3);

/// Each transition on a video row: its overlap (or a lone one's fade-in) tinted, with the plug-in's name.
fn paint_transitions(p: &Painter, t: &Theme, g: &Geo, row: &Row, clips: &[Clip]) {
    let (t0, t1) = (g.to_time(p.clip_rect().min.x), g.to_time(p.clip_rect().max.x));
    // Off-screen events are skipped before the overlap search, which scans the track.
    for c in clips.iter().filter(|c| c.position <= t1 && c.position + c.duration >= t0) {
        let Some(tr) = &c.transition else { continue };
        let from = clips.iter().filter(|a| a.position < c.position && a.position + a.duration > c.position).map(|a| a.position + a.duration).reduce(f64::max);
        let end = from.map_or(c.position + c.fade_in, |e| e.min(c.position + c.duration));
        if end <= c.position {
            continue;
        }
        let r = Rect::from_min_max(pos2(g.to_x(c.position), row.top + 1.0), pos2(g.to_x(end), row.top + row.h - 2.0));
        if !r.intersects(p.clip_rect()) {
            continue;
        }
        p.rect_filled(r, 0.0, TRANSITION.gamma_multiply(0.45));
        p.rect_stroke(r, 0.0, Stroke::new(1.0_f32, TRANSITION), StrokeKind::Inside);
        let name = crate::engine::plugins::find(&tr.plugin).map_or(tr.plugin.as_str(), |i| i.name);
        p.with_clip_rect(r.intersect(p.clip_rect())).text(r.center(), Align2::CENTER_CENTER, name, t.font(t.font.ui), t.text_bright);
    }
}

/// The level handle's height (0..=1): gain as an amplitude (audio) or opacity (video). None when an opacity envelope
/// with several keys hides it.
fn level_of(c: &Clip, video: bool) -> Option<f32> {
    if !video {
        return Some(crate::engine::keyframes::db_to_gain(c.gain_db).min(1.0));
    }
    let keys = c.envelopes.iter().find(|e| e.kind == EnvKind::Opacity).map_or(0, |e| e.points.keys.len());
    (keys <= 1).then(|| c.opacity())
}

/// Left-click on event `cr`: replace the selection, Ctrl toggles it, Shift selects every event on its track between
/// the focus event and it (a plain click when the focus is on another track). The clicked event becomes the focus.
/// With `group`, the clicked events' groups come along.
fn click_select(project: &Project, sel: &mut Selection, cr: &ClipRef, ctrl: bool, shift: bool, group: bool) {
    let track = &project.timeline.tracks(cr.is_video)[cr.track_idx];
    let id = track.clips[cr.clip_idx].id.clone();
    let with_mates = |ids: Vec<String>| if group { edit::expand_groups(project, &ids) } else { ids };
    let focus = sel.clips.last().and_then(|f| track.clips.iter().find(|c| c.id == *f));
    if ctrl {
        let (members, was) = (with_mates(vec![id.clone()]), sel.clips.contains(&id));
        sel.clips.retain(|s| !members.contains(s));
        if !was {
            sel.clips.extend(members);
        }
    } else if let Some(f) = focus.filter(|_| shift) {
        let c = &track.clips[cr.clip_idx];
        let (a, b) = (f.position.min(c.position), f.position.max(c.position));
        let mut range: Vec<String> = track.clips.iter().filter(|e| e.position >= a && e.position <= b && e.id != id).map(|e| e.id.clone()).collect();
        range.push(id);
        sel.clips = with_mates(range);
    } else {
        sel.clips = with_mates(vec![id]);
    }
}

/// Events whose rectangles meet `rect` (the rubber band) on the rows shown; an empty rectangle meets none.
fn clips_in_rect(project: &Project, rows: &[Row], g: &Geo, rect: Rect) -> Vec<String> {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for row in rows {
        for c in &project.timeline.tracks(row.is_video)[row.idx].clips {
            let ev = Rect::from_min_max(pos2(g.to_x(c.position), row.top + 1.0), pos2(g.to_x(c.position + c.duration), row.top + row.h - 2.0));
            if ev.intersects(rect) {
                out.push(c.id.clone());
            }
        }
    }
    out
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

/// Inline rename box for a track name (double-click the header's top-left), a marker (double-click its tag) or an
/// event's active take (over its label).
fn rename_box(ui: &mut Ui, g: &Geo, rows: &[Row], project: &mut Project, history: &mut History, st: &mut TlState) {
    if let Some(c) = ui.data_mut(|d| d.remove_temp::<String>(rename_id())).and_then(|id| project.clip(&id)) {
        (st.rename, st.rename_focused) = (Some((Rename::Event(c.id.clone()), c.name.clone())), false);
    }
    let Some((target, mut buf)) = st.rename.take() else {
        st.rename_focused = false;
        return;
    };
    let t = theme::get();
    let at = match &target {
        &Rename::Track(v, i) => rows.iter().find(|r| r.is_video == v && r.idx == i).map(|r| {
            let x = g.rect.min.x + t.m.color_strip_w + 4.0;
            Rect::from_min_size(pos2(x, r.top + 2.0), vec2((g.header_w - t.m.color_strip_w - 80.0).max(60.0), 18.0))
        }),
        &Rename::Marker(i) => project.timeline.markers.get(i).map(|m| {
            let (_, marker_r, _) = ruler::band_rows(&t, g.band);
            Rect::from_min_size(pos2(g.to_x(m.position) + 14.0, marker_r.min.y - 2.0), vec2(160.0, 18.0))
        }),
        Rename::Event(id) => project.find_clip(id).zip(project.clip(id)).and_then(|(r, c)| {
            let row = rows.iter().find(|w| w.is_video == r.is_video && w.idx == r.track_idx)?;
            let x = g.to_x(c.position).max(g.lanes.min.x) + 2.0;
            Some(Rect::from_min_size(pos2(x, row.top + 2.0), vec2(200.0, 18.0)))
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
        let clip = |id: &str| project.find_clip(id).map(|r| (r.is_video, r.track_idx, r.clip_idx));
        let old = match &target {
            &Rename::Track(v, i) => project.timeline.tracks(v).get(i).map(|t| &t.name),
            &Rename::Marker(i) => project.timeline.markers.get(i).map(|m| &m.name),
            Rename::Event(id) => project.clip(id).map(|c| &c.name),
        };
        // An unchanged name is no edit (no undo step).
        if !ui.input(|i| i.key_pressed(Key::Escape)) && old.is_some_and(|o| *o != buf) {
            let at = if let Rename::Event(id) = &target { clip(id) } else { None };
            history.push(project.clone());
            match target {
                Rename::Track(v, i) => project.timeline.tracks_mut(v)[i].name = buf,
                Rename::Marker(i) => project.timeline.markers[i].name = buf,
                Rename::Event(_) => {
                    if let Some((v, i, j)) = at {
                        project.timeline.tracks_mut(v)[i].clips[j].name = buf;
                    }
                }
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
        env_drag: Option<EnvDrag>,
        scroll_y: f32,
        history: History,
        waves: Waveforms,
        thumbs: Thumbs,
        track_w: f32,
        quantize: bool,
        drops: crate::ui::FileDrag,
        /// Cursor the last frame asked for.
        cursor: CursorIcon,
        /// Tool, grouping, crossfade and ripple options (snapping is on; `quantize` overrides).
        opts: EditOpts,
        /// The ripple of the last drag released.
        released: Option<Vec<edit::RippleOp>>,
    }

    impl Rig {
        fn new() -> Self {
            let ctx = Context::default();
            crate::ui::fonts::install(&ctx);
            let mut project = Project::new("t");
            project.timeline.video_tracks[0].clips.push(Clip::new("c", 2.0, 4.0));
            let (waves, thumbs) = (Waveforms::new(&ctx), Thumbs::new(&ctx));
            let drops = crate::ui::FileDrag { still_len: 5.0, ..Default::default() };
            let opts = EditOpts { auto_crossfade: true, ..Default::default() };
            Rig { ctx, project, sel: Selection::default(), drag: None, env_drag: None, scroll_y: 0.0, history: History::default(), waves, thumbs, track_w: 251.0, quantize: false, drops, cursor: CursorIcon::Default, opts, released: None }
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
                    let opts = EditOpts { snapping: true, quantize: r.quantize, ..r.opts };
                    r.released = draw_timeline(
                        &mut child, &mut r.project, &mut r.sel, &mut r.drag, &mut r.env_drag, &mut r.scroll_y, &mut r.history, opts,
                        &mut r.waves, &mut r.thumbs, false, &mut r.track_w, &mut r.drops,
                    )
                    .or(r.released.take());
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

    #[test]
    fn clips_in_rect_picks_events_the_band_touches() {
        let mut p = Project::new("t");
        p.timeline.video_tracks[0].clips.push(Clip::new("v", 2.0, 4.0)); // x 20..60, row y 0..50
        p.timeline.audio_tracks[0].clips.push(Clip::new("a", 2.0, 4.0)); // row y 50..100
        let lanes = Rect::from_min_max(Pos2::ZERO, pos2(1000.0, 300.0));
        let g = Geo { rect: lanes, header_w: 0.0, list: Rect::ZERO, band: Rect::ZERO, lanes, vscroll: Rect::ZERO, hscroll: Rect::ZERO, pps: 10.0, scroll_x: 0.0, grid: (1.0, 0.0), grid_snap: false };
        let rows = [Row { is_video: true, idx: 0, num: 0, top: 0.0, h: 50.0 }, Row { is_video: false, idx: 0, num: 1, top: 50.0, h: 50.0 }];
        let v = p.timeline.video_tracks[0].clips[0].id.clone();
        // Partly inside on the video row; the audio row below is not touched.
        assert_eq!(clips_in_rect(&p, &rows, &g, Rect::from_min_max(pos2(50.0, 10.0), pos2(100.0, 30.0))), [v]);
        assert!(clips_in_rect(&p, &rows, &g, Rect::from_min_max(pos2(30.0, 10.0), pos2(30.0, 80.0))).is_empty());
        assert_eq!(clips_in_rect(&p, &rows, &g, Rect::from_min_max(pos2(30.0, 10.0), pos2(40.0, 80.0))).len(), 2);
    }

    /// The Selection tool: a drag (even over an event) rubber-bands the events it touches and sets the time selection
    /// to its span; nothing moves.
    #[test]
    fn selection_tool_rubber_band() {
        let mut r = Rig::new();
        r.opts.tool = EditTool::Selection;
        r.project.timeline.audio_tracks[0].clips.push(Clip::new("a", 8.0, 1.0));
        r.drag(pos2(300.0, 60.0), pos2(495.0, 100.0));
        assert_eq!(r.sel.clips, [r.clip().id.clone()]);
        assert_eq!(r.clip().position, 2.0);
        let (a, b) = r.project.timeline.loop_region.expect("time selection");
        assert!((a - 45.0 / 80.0).abs() < 1e-6 && (b - 3.0).abs() < 1e-6, "{a}..{b}");
        assert_eq!(r.history.lens().0, 0);
    }

    /// A click selects the event's group; a trim takes the grouped event whose edge lines up with it along.
    #[test]
    fn group_click_and_trim() {
        let mut r = Rig::new();
        let mut a = Clip::new("a", 2.0, 4.0);
        a.group = Some("g".into());
        r.project.timeline.video_tracks[0].clips[0].group = Some("g".into());
        r.project.timeline.audio_tracks[0].clips.push(a);
        r.click(pos2(575.0, 90.0), Modifiers::NONE);
        assert_eq!(r.sel.clips.len(), 2);
        assert_eq!(r.sel.clips.last(), Some(&r.clip().id), "the clicked event is the focus");
        r.drag(pos2(734.0, 90.0), pos2(654.0, 90.0));
        let d = [&r.project.timeline.video_tracks[0], &r.project.timeline.audio_tracks[0]].map(|t| t.clips[0].duration);
        assert!(d.iter().all(|d| (d - 3.0).abs() < 1e-6), "{d:?}");
        assert_eq!(r.history.lens().0, 1);
        // Ignore Event Grouping: a click takes the event alone.
        r.opts.ignore_grouping = true;
        r.click(pos2(575.0, 90.0), Modifiers::NONE);
        assert_eq!(r.sel.clips.len(), 1);
    }

    /// Auto Ripple: trimming an event's right edge moves what follows on its track, and the drag reports its ripple.
    #[test]
    fn auto_ripple_trim_moves_later_events() {
        let mut r = Rig::new();
        r.opts.ripple = Some(edit::RippleMode::Affected);
        r.project.timeline.video_tracks[0].clips.push(Clip::new("d", 7.0, 1.0));
        r.drag(pos2(734.0, 90.0), pos2(814.0, 90.0));
        let tr = &r.project.timeline.video_tracks[0];
        assert!((tr.clips[0].duration - 5.0).abs() < 1e-6 && (tr.clips[1].position - 8.0).abs() < 1e-6, "{:?}", starts(tr));
        let op = r.released.take().expect("ripple");
        assert!(op.len() == 1 && (op[0].from - 2.0).abs() < 1e-9 && (op[0].delta - 1.0).abs() < 1e-6, "{op:?}");
    }

    /// Auto Ripple: a move along its track moves what follows by as much; extending a left edge keeps the event's
    /// start and pushes what follows.
    #[test]
    fn auto_ripple_move_and_left_trim() {
        let mut r = Rig::new();
        r.opts.ripple = Some(edit::RippleMode::Affected);
        r.project.timeline.video_tracks[0].clips[0].in_point = 2.0;
        r.project.timeline.video_tracks[0].clips.push(Clip::new("d", 7.0, 1.0));
        r.drag(pos2(575.0, 90.0), pos2(655.0, 90.0)); // the 2..6 event right by 1 s
        assert_eq!(starts(&r.project.timeline.video_tracks[0]), [3.0, 8.0]);
        r.drag(pos2(496.0, 90.0), pos2(416.0, 90.0)); // its left edge out by 1 s
        let tr = &r.project.timeline.video_tracks[0];
        assert_eq!((starts(tr), tr.clips[0].duration), (vec![3.0, 9.0], 5.0));
        assert_eq!(r.history.lens().0, 2);
    }

    /// A drag out and back to where it started leaves no crossfade, no undo step and no change behind.
    #[test]
    fn zero_net_drag_changes_nothing() {
        let mut r = Rig::new();
        r.project.timeline.video_tracks[0].clips.push(Clip::new("d", 5.0, 3.0)); // x 655..895, over 2..6 without fades
        let p = pos2(850.0, 90.0);
        let press = |pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        r.frame(vec![Event::PointerMoved(p)]);
        r.frame(vec![press(true)]);
        for x in [900.0, 1010.0, 900.0, 850.0] {
            r.frame(vec![Event::PointerMoved(pos2(x, 90.0))]);
        }
        r.frame(vec![press(false)]);
        r.frame(vec![]);
        let tr = &r.project.timeline.video_tracks[0];
        assert_eq!((starts(tr), tr.clips[0].fade_out, tr.clips[1].fade_in, r.history.lens().0), (vec![2.0, 5.0], 0.0, 0.0, 0));
        assert!(!r.project.modified);
    }

    /// Automatic Crossfades: dragging one event onto another crossfades them for the overlap.
    #[test]
    fn dragging_onto_an_event_crossfades() {
        let mut r = Rig::new();
        r.project.timeline.video_tracks[0].clips.push(Clip::new("d", 7.0, 2.0));
        r.drag(pos2(895.0, 90.0), pos2(735.0, 90.0)); // d to 5..7, over 2..6
        let tr = &r.project.timeline.video_tracks[0];
        assert!((tr.clips[0].fade_out - 1.0).abs() < 1e-6 && (tr.clips[1].fade_in - 1.0).abs() < 1e-6);
        assert_eq!(r.history.lens().0, 1);
    }

    /// Alt+drag inside slips, Ctrl+Alt+drag of an edit point rolls, Ctrl+drag of an edge stretches (on the frame grid
    /// with Quantize to Frames), each in one undo step. The event (2..6 s) is at x 415..735 on Video 1 (y 48..112).
    #[test]
    fn slip_roll_and_stretch_drags() {
        let alt = Modifiers { alt: true, ..Default::default() };
        let ctrl = Modifiers { ctrl: true, command: true, ..Default::default() };
        let mut r = Rig::new();
        r.project.timeline.video_tracks[0].clips[0].in_point = 3.0;
        r.drag_mod(pos2(575.0, 90.0), pos2(655.0, 90.0), alt);
        assert!(r.clip().position == 2.0 && (r.clip().in_point - 2.0).abs() < 1e-6, "slip: in {}", r.clip().in_point);

        r.project.timeline.video_tracks[0].clips.push(Clip::new("d", 6.0, 2.0));
        r.drag_mod(pos2(735.0, 90.0), pos2(815.0, 90.0), ctrl | alt);
        let tr = &r.project.timeline.video_tracks[0];
        let (a, b) = (&tr.clips[0], &tr.clips[1]);
        assert!((a.duration - 5.0).abs() < 1e-6 && a.position + a.duration == b.position && (b.duration - 1.0).abs() < 1e-6, "roll {a:?} {b:?}");

        r.quantize = true;
        r.drag_mod(pos2(416.0, 90.0), pos2(335.0, 90.0), ctrl); // the left edge (x 415) out by ~1 s
        let c = r.clip();
        assert!((c.position - edit::quantize(c.position, 29.97)).abs() < 1e-9 && (c.position + c.duration - 7.0).abs() < 1e-6, "{c:?}");
        assert!((c.rate * c.duration - 5.0).abs() < 1e-6 && c.rate < 1.0);
        assert_eq!(r.history.lens().0, 3);
    }

    /// With Quantize to Frames, a slip moves the media by whole frames and slip-trim, slide and roll edges land on the
    /// frame grid; a slide stopped by the media start (in_point off the grid) is pulled back onto it.
    #[test]
    fn quantized_drags_land_on_frames() {
        let fps = 30000.0 / 1001.0; // the project's 29.97, exactly
        let on_grid = |t: f64| (t * fps - (t * fps).round()).abs() < 1e-6;
        let alt = Modifiers { alt: true, ..Default::default() };
        let ctrl_alt = Modifiers { ctrl: true, command: true, alt: true, ..Default::default() };
        let mut r = Rig::new();
        r.quantize = true;
        r.project.timeline.video_tracks[0].clips[0].in_point = 3.0;
        r.drag_mod(pos2(575.0, 90.0), pos2(658.0, 90.0), alt);
        assert!(on_grid(3.0 - r.clip().in_point) && r.clip().in_point < 3.0, "slip: in {}", r.clip().in_point);
        r.drag_mod(pos2(734.0, 90.0), pos2(693.0, 90.0), alt);
        let c = r.clip();
        assert!(on_grid(c.position + c.duration) && c.duration < 4.0, "slip-trim {c:?}");
        r.drag_mod(pos2(575.0, 90.0), pos2(640.0, 90.0), ctrl_alt);
        assert!(on_grid(r.clip().position) && r.clip().position > 2.5, "slide {:?}", r.clip());

        let end = r.clip().position + r.clip().duration;
        r.project.timeline.video_tracks[0].clips.push(Clip::new("d", end, 2.0));
        let x = 255.0 + end as f32 * 80.0;
        r.drag_mod(pos2(x - 1.0, 90.0), pos2(x + 36.0, 90.0), ctrl_alt);
        let tr = &r.project.timeline.video_tracks[0];
        let (a, b) = (&tr.clips[0], &tr.clips[1]);
        assert!(a.position + a.duration == b.position && on_grid(b.position) && b.position > end, "roll {a:?} {b:?}");

        r.project.media_pool.push(MediaItem { id: "m".into(), media_type: MediaType::Audio, duration: 10.0, ..Default::default() });
        let mut au = Clip::new("a", 90.0 / fps, 4.0); // Audio 1, y 112..180
        (au.media_id, au.in_point) = (Some("m".into()), 1.234);
        r.project.timeline.audio_tracks[0].clips.push(au);
        let x = 255.0 + (90.0 / fps as f32 + 1.0) * 80.0;
        r.drag_mod(pos2(x, 160.0), pos2(x - 240.0, 160.0), ctrl_alt);
        let au = &r.project.timeline.audio_tracks[0].clips[0];
        assert!(on_grid(au.position) && au.in_point >= 0.0 && au.in_point < 1.0 / fps, "clamped slide {au:?}");
        assert_eq!(r.history.lens().0, 5);
    }

    /// The Slide tool on an edge with no neighbour trims like the Normal tool: the grouped audio comes along and Auto
    /// Ripple moves what follows.
    #[test]
    fn roll_without_a_neighbour_trims_the_group_and_ripples() {
        let mut r = Rig::new();
        r.opts.tool = EditTool::Slide;
        r.opts.ripple = Some(edit::RippleMode::Affected);
        let mut a = Clip::new("a", 2.0, 4.0);
        a.group = Some("g".into());
        r.project.timeline.video_tracks[0].clips[0].group = Some("g".into());
        r.project.timeline.audio_tracks[0].clips.push(a);
        r.project.timeline.video_tracks[0].clips.push(Clip::new("d", 8.0, 1.0));
        r.drag(pos2(734.0, 90.0), pos2(654.0, 90.0));
        let tl = &r.project.timeline;
        let d = [tl.video_tracks[0].clips[0].duration, tl.audio_tracks[0].clips[0].duration];
        assert!(d.iter().all(|d| (d - 3.0).abs() < 1e-6), "{d:?}");
        assert!((tl.video_tracks[0].clips[1].position - 7.0).abs() < 1e-6, "{:?}", starts(&tl.video_tracks[0]));
    }

    /// Ctrl+Alt on a grouped A/V edit point rolls both cuts; Ctrl+Alt inside a crossfade slides it, keeping its length.
    #[test]
    fn group_roll_and_crossfade_slide() {
        let ctrl_alt = Modifiers { ctrl: true, command: true, alt: true, ..Default::default() };
        let grouped = |name: &str, pos, dur, g: &str| {
            let mut c = Clip::new(name, pos, dur);
            c.group = Some(g.into());
            c
        };
        let mut r = Rig::new();
        let tl = &mut r.project.timeline;
        tl.video_tracks[0].clips[0].group = Some("g1".into());
        tl.video_tracks[0].clips.push(grouped("e", 6.0, 3.0, "g2"));
        tl.audio_tracks[0].clips.extend([grouped("a", 2.0, 4.0, "g1"), grouped("b", 6.0, 3.0, "g2")]);
        r.drag_mod(pos2(735.0, 90.0), pos2(815.0, 90.0), ctrl_alt); // the cut at 6 s to 7 s
        for t in [&r.project.timeline.video_tracks[0], &r.project.timeline.audio_tracks[0]] {
            let (a, b) = (&t.clips[0], &t.clips[1]);
            assert!((a.duration - 5.0).abs() < 1e-6 && a.position + a.duration == b.position && (b.duration - 2.0).abs() < 1e-6, "{a:?} {b:?}");
        }

        let mut r = Rig::new();
        r.project.timeline.video_tracks[0].clips.push(Clip::new("e", 5.0, 4.0)); // over 2..6 from 5 s
        r.drag_mod(pos2(695.0, 90.0), pos2(735.0, 90.0), ctrl_alt); // inside the overlap, right by 0.5 s
        let tr = &r.project.timeline.video_tracks[0];
        let (a, b) = (&tr.clips[0], &tr.clips[1]);
        assert!((b.position - 5.5).abs() < 1e-6 && (b.position + b.duration - 9.0).abs() < 1e-6, "{b:?}");
        assert!((a.position + a.duration - 6.5).abs() < 1e-6 && (a.fade_out - 1.0).abs() < 1e-6, "{a:?}");
    }

    /// The level line at an event's top edge: dragging it halfway down gives an audio event -6 dB and a video event
    /// 50 % opacity.
    #[test]
    fn level_handle_sets_gain_and_opacity() {
        let mut r = Rig::new();
        r.project.timeline.audio_tracks[0].clips.push(Clip::new("a", 2.0, 4.0)); // Audio 1 y 112..180
        r.drag(pos2(575.0, 115.0), pos2(575.0, 147.5));
        let a = &r.project.timeline.audio_tracks[0].clips[0];
        assert!((a.gain_db + 6.0206).abs() < 0.01 && a.position == 2.0, "gain {}", a.gain_db);
        r.drag(pos2(575.0, 51.0), pos2(575.0, 81.5));
        assert!((r.clip().opacity() - 0.5).abs() < 0.01 && r.clip().position == 2.0, "opacity {}", r.clip().opacity());
        assert_eq!(r.history.lens().0, 2);
        // With several opacity keys there is no handle: the drag moves the event.
        let env = &mut r.project.timeline.video_tracks[0].clips[0].envelopes[0];
        env.points.keys.push(Keyframe { t: 1.0, v: 1.0, curve: Curve::Linear });
        r.drag(pos2(575.0, 51.0), pos2(655.0, 51.0));
        assert_eq!(r.clip().position, 3.0);
    }

    /// Audio 1 (y 112..180; its envelope lane 114..176) with a volume envelope 0 dB at 0 and a point at 4 s (x 575):
    /// the point drags down in one undo step, an Envelope tool click on the line adds a point, and with Lock
    /// Envelopes to Events a moved event takes its points along.
    #[test]
    fn envelope_points_drag_add_and_follow_events() {
        let mut r = Rig::new();
        (r.opts.show_envelopes, r.opts.lock_envelopes) = (true, true);
        let a = &mut r.project.timeline.audio_tracks[0];
        edit::toggle_envelope(a, EnvKind::Volume);
        a.envelopes[0].points.keys.push(Keyframe { t: 4.0, v: 0.0, curve: Curve::Linear });
        let y = |db: f32| 176.0 - edit::lane_frac(EnvKind::Volume, db) * 62.0;
        let key = |r: &Rig, k: usize| r.project.timeline.audio_tracks[0].envelopes[0].points.keys[k].clone();
        r.drag(pos2(575.0, y(0.0)), pos2(575.0, y(0.0) + 0.35 * 62.0));
        assert!(key(&r, 1).t == 4.0 && (key(&r, 1).v + 12.04).abs() < 0.3, "{:?}", key(&r, 1));
        assert_eq!(r.history.lens().0, 1);
        assert_eq!(r.sel.env.as_ref().map(|s| s.keys.clone()), Some(vec![1]));

        r.opts.tool = EditTool::Envelope;
        let mid = r.project.timeline.audio_tracks[0].envelopes[0].at(2.0);
        r.click(pos2(415.0, y(mid)), Modifiers::NONE);
        assert_eq!(r.project.timeline.audio_tracks[0].envelopes[0].points.keys.len(), 3);
        assert!((key(&r, 1).t - 2.0).abs() < 1e-6 && (key(&r, 1).v - mid).abs() < 1e-4, "{:?}", key(&r, 1));
        assert_eq!(r.history.lens().0, 2);

        r.opts.tool = EditTool::Normal;
        let low = key(&r, 2).v;
        r.project.timeline.audio_tracks[0].clips.push(Clip::new("a", 3.0, 2.0)); // x 495..655, under the 4 s point
        r.drag(pos2(520.0, 172.0), pos2(600.0, 172.0));
        assert_eq!(r.project.timeline.audio_tracks[0].clips[0].position, 4.0);
        let e = &r.project.timeline.audio_tracks[0].envelopes[0];
        assert!(e.points.keys.iter().any(|k| k.t == 5.0 && k.v == low) && (e.at(2.0) - mid).abs() < 1e-4, "{e:?}");
        assert_eq!(r.history.lens().0, 3);
        assert!(r.sel.env.is_none(), "an event drag drops the point selection, so Delete takes the events");
    }

    fn starts(t: &Track) -> Vec<f64> {
        t.clips.iter().map(|c| c.position).collect()
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

    /// A right-drag from Project Media asks how to add the media: the drop menu opens and nothing is added yet.
    #[test]
    fn right_drag_drop_opens_the_drop_menu() {
        let mut r = Rig::new();
        r.project.media_pool = pool();
        let at = pos2(255.0 + 8.0 * 80.0, 80.0);
        r.frame(vec![Event::PointerMoved(at)]);
        DragAndDrop::set_payload(&r.ctx, crate::ui::MediaDrag::Pool(vec![0], None));
        r.frame(vec![Event::PointerMoved(at)]);
        r.frame(vec![Event::PointerButton { pos: at, button: PointerButton::Secondary, pressed: false, modifiers: Modifiers::NONE }]);
        r.frame(vec![]);
        assert!(r.ctx.is_context_menu_open(), "drop menu");
        assert_eq!((events(&r.project, true).len(), r.history.lens().0), (1, 0));
        // A right-drag from the Explorer asks too; nothing goes to the app until a choice is made.
        let mut r = Rig::new();
        (r.drops.files, r.drops.items) = (vec!["v.mp4".into()], Some(pool()[..1].to_vec()));
        r.frame(vec![Event::PointerMoved(at)]);
        DragAndDrop::set_payload(&r.ctx, crate::ui::MediaDrag::File("v.mp4".into()));
        r.frame(vec![Event::PointerMoved(at)]);
        r.frame(vec![Event::PointerButton { pos: at, button: PointerButton::Secondary, pressed: false, modifiers: Modifiers::NONE }]);
        r.frame(vec![]);
        assert!(r.ctx.is_context_menu_open() && r.drops.dropped_at.is_none(), "drop menu for files");
    }


    /// Take › Rename Active opens a rename box on the event; typing a name and Enter renames it in one undo step.
    #[test]
    fn rename_active_take() {
        let mut r = Rig::new();
        let id = r.clip().id.clone();
        rename_event(&r.ctx, &id);
        r.frame(vec![]);
        r.frame(vec![Event::Key { key: Key::Backspace, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]); // the old name, "c"
        r.frame(vec![Event::Text("Take 2".into())]);
        r.frame(vec![Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
        r.frame(vec![]);
        assert_eq!((r.clip().name.as_str(), r.history.lens().0), ("Take 2", 1));
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
        assert_eq!(r.drops.dropped_at.take(), Some((vec!["v.mp4".into()], Some(a1), 8.0, edit::DropMode::AcrossTime)));
        assert!(!r.drops.os_dropped);
        r.drops.os_pos = Some(pos2(100.0, 150.0)); // over the track list
        r.drops.os_dropped = true;
        r.frame(vec![]);
        assert!(r.drops.dropped_at.is_none() && r.drops.os_dropped);
    }
}
