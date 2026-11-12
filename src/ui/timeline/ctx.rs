//! Timeline right-click menus (§6.8 tracks and lanes, §6.9 events, §6.10 marker bar, ruler and time display).
//! One handler sees every right-click in the timeline, including ones a child widget (track header, marker tag,
//! loop bar) took, works out what was clicked and shows that menu. Global actions go to the command bus;
//! actions on the clicked track / marker / region are done here.
use eframe::egui::*;
use crate::edit;
use crate::engine::plugins::{self, PluginKind};
use crate::history::History;
use crate::project::*;
use crate::ui::{EnvSel, Selection};
use super::super::keymap::Command::*;
use super::super::menus::{self, Fixed, Later, M, M::*, Mark, NotPlanned, cmd, entry, inert, off, sep, sub};
use super::super::theme::{self, UiLook};
use super::ruler::band_rows;
use super::{FADE_PX, Geo, Rename, Row, TlState, envelopes};

#[derive(Clone, PartialEq, Debug)]
enum Target {
    TrackHeader(bool, usize),
    ListEmpty,
    /// The overlap of two events on a track: (outgoing, incoming) ids.
    Crossfade(String, String),
    /// An event's fade handle: (id, fade in).
    Fade(String, bool),
    /// A track envelope's point, or its line at a time (with the point starting that segment): track id, kind.
    EnvPoint(String, EnvKind, usize),
    EnvLine(String, EnvKind, f64, usize),
    /// The selected events; with the incoming event's id and the cut time when the click was on a cut's edge.
    Event(Option<(String, f64)>),
    Lane(bool, usize),
    Selection,
    MarkerBar,
    Marker(usize),
    RegionStart(usize),
    RegionEnd(usize),
    LoopBar,
    Ruler,
    TimeDisplay,
    /// The menu after a right-drag of media onto the lanes.
    Drop(DropReq),
}

/// A right-drag media drop waiting on its menu: where it lands, what it adds and how.
#[derive(Clone, PartialEq, Debug)]
pub(super) struct DropReq {
    pub target: Option<(bool, usize)>,
    pub time: f64,
    /// Media pool indices and their source range.
    pub idxs: Vec<usize>,
    /// Or files from the Explorer window, imported and placed by the app once a choice is made.
    pub files: Vec<std::path::PathBuf>,
    pub range: Option<(f64, f64)>,
    pub still_len: f64,
    pub xfade: bool,
}

const TAG: Vec2 = vec2(13.0, 14.0);
/// How near a cut a right-click offers Transition ▸ for it.
const CUT_PX: f32 = 8.0;

fn target(g: &Geo, rows: &[Row], project: &Project, sel: &Selection, p: Pos2, env_on: bool) -> Option<Target> {
    let t = theme::get();
    let tl = &project.timeline;
    if Rect::from_min_size(g.rect.min, vec2(g.header_w, t.m.band_h)).contains(p) {
        return Some(Target::TimeDisplay);
    }
    if g.band.contains(p) {
        let (loop_r, marker_r, _) = band_rows(&t, g.band);
        if marker_r.contains(p) {
            let tag = |x: f32| Rect::from_min_size(pos2(x, marker_r.min.y), TAG).contains(p);
            if let Some(i) = tl.markers.iter().position(|m| tag(g.to_x(m.position).floor())) {
                return Some(Target::Marker(i));
            }
            if let Some(i) = tl.regions.iter().position(|r| tag(g.to_x(r.start).floor())) {
                return Some(Target::RegionStart(i));
            }
            if let Some(i) = tl.regions.iter().position(|r| tag(g.to_x(r.end).floor() - 12.0)) {
                return Some(Target::RegionEnd(i));
            }
            return Some(Target::MarkerBar);
        }
        return Some(if loop_r.expand2(vec2(0.0, 1.0)).contains(p) { Target::LoopBar } else { Target::Ruler });
    }
    let row = rows.iter().find(|r| p.y >= r.top && p.y < r.top + r.h);
    if g.list.contains(p) {
        return Some(row.map_or(Target::ListEmpty, |r| Target::TrackHeader(r.is_video, r.idx)));
    }
    if g.lanes.contains(p) {
        let time = g.to_time(p.x);
        // A fade handle: the top corner zone `hit_clip` drags the fade from.
        let fade = row.filter(|r| p.y >= r.top + 1.0 && p.y - r.top - 1.0 <= FADE_PX).and_then(|r| {
            tl.tracks(r.is_video)[r.idx].clips.iter().rev().find_map(|c| {
                let (x0, x1) = (g.to_x(c.position), g.to_x(c.position + c.duration));
                let (fi, fo) = (x0 + (c.fade_in * g.pps) as f32, x1 - (c.fade_out * g.pps) as f32);
                if p.x < x0 || p.x > x1 {
                    None
                } else if (p.x - fi).abs() <= FADE_PX && p.x - x0 < x1 - p.x {
                    Some(Target::Fade(c.id.clone(), true))
                } else {
                    ((p.x - fo).abs() <= FADE_PX).then(|| Target::Fade(c.id.clone(), false))
                }
            })
        });
        // An envelope point, else its line unless a fade handle is there.
        if let Some(h) = envelopes::hit(g, rows, project, p).filter(|h| env_on && (h.point.is_some() || fade.is_none())) {
            let tr = &tl.tracks(h.is_video)[h.track];
            let (id, kind) = (tr.id.clone(), tr.envelopes[h.env].kind);
            if !tr.locked {
                return Some(match h.point {
                    Some(k) => Target::EnvPoint(id, kind, k),
                    None => Target::EnvLine(id, kind, time, h.seg),
                });
            }
        }
        let mut cut = None;
        if let Some(r) = row {
            let clips = &tl.tracks(r.is_video)[r.idx].clips;
            let xfade = clips.iter().enumerate().find_map(|(i, a)| {
                let b = clips[i + 1..].iter().find(|b| b.position < a.position + a.duration && b.position >= a.position)?;
                (time >= b.position && time <= a.position + a.duration).then(|| Target::Crossfade(a.id.clone(), b.id.clone()))
            });
            if xfade.is_some() {
                return xfade;
            }
            if fade.is_some() {
                return fade;
            }
            // Near a video cut (two events butting): the cut can become a transition.
            let tol = 0.5 / project.settings.frame_rate.max(1.0);
            let end = |c: &Clip| c.position + c.duration;
            cut = clips
                .iter()
                .filter(|b| r.is_video && (p.x - g.to_x(b.position)).abs() <= CUT_PX)
                .find(|b| clips.iter().any(|a| a.position < b.position && (end(a) - b.position).abs() <= tol))
                .map(|b| (b.id.clone(), b.position));
        }
        if !sel.clips.is_empty() {
            return Some(Target::Event(cut));
        }
        if tl.loop_region.is_some_and(|(a, b)| b > a && time >= a && time <= b) {
            return Some(Target::Selection);
        }
        return row.map(|r| Target::Lane(r.is_video, r.idx));
    }
    None
}

/// Open the menu for a right-click anywhere in the timeline (or `drop`, a right-drag of media released at a point)
/// and keep showing it while open.
#[allow(clippy::too_many_arguments)]
pub(super) fn menu(
    ui: &Ui,
    response: &Response,
    g: &Geo,
    rows: &[Row],
    project: &mut Project,
    history: &mut History,
    sel: &mut Selection,
    st: &mut TlState,
    env_on: bool,
    xf: (f64, bool),
    drop: Option<(Pos2, DropReq)>,
    drops: &mut crate::ui::FileDrag,
) {
    let tid = ui.id().with("ctx_target");
    let clicked = ui.input(|i| i.pointer.button_clicked(PointerButton::Secondary).then(|| i.pointer.interact_pos()).flatten());
    let open_at = drop.map(|(p, r)| (p, Target::Drop(r))).or_else(|| {
        clicked
            .filter(|p| g.rect.contains(*p) && ui.ctx().layer_id_at(*p) == Some(ui.layer_id()))
            .and_then(|p| target(g, rows, project, sel, p, env_on).map(|t| (p, t)))
    });
    if let Some((_, tg)) = &open_at {
        ui.data_mut(|d| d.insert_temp(tid, tg.clone()));
        st.env_text = None;
    }
    let Some(tg) = ui.data(|d| d.get_temp::<Target>(tid)) else { return };
    menus::context_at(response, open_at.map(|o| o.0), |ui| match tg {
        Target::TrackHeader(v, i) => track_header(ui, project, history, &sel.tracks, st, v, i),
        Target::ListEmpty => {
            cmd(ui, "&Open...", Open);
            cmd(ui, "&Insert Audio Track", InsertAudioTrack);
            cmd(ui, "Insert &Video Track", InsertVideoTrack);
            sep(ui);
            cmd(ui, "&Preferences...", Preferences);
        }
        Target::Crossfade(ref a, ref b) => {
            // Inside the overlap, so a transition lands on it.
            let mid = project.clip(a).zip(project.clip(b)).map_or(0.0, |(a, b)| (b.position + a.position + a.duration) * 0.5);
            transition_items(ui, project, history, b, mid, xf);
            sub(ui, "&Fade Type", true, |ui| {
                // A curve pair: the outgoing event's fade-out, then the incoming one's fade-in.
                const CURVES: [(&str, Curve); 5] = [("Linear", Curve::Linear), ("Fast", Curve::Fast), ("Slow", Curve::Slow), ("Smooth", Curve::Smooth), ("Sharp", Curve::Sharp)];
                let cur = project.clip(a).map(|c| c.fade_out_curve).zip(project.clip(b).map(|c| c.fade_in_curve));
                for (out_name, out) in CURVES {
                    sub(ui, &format!("{out_name} Out"), true, |ui| {
                        for (in_name, inc) in CURVES {
                            if entry(ui, &format!("{out_name} Out, {in_name} In"), "", true, Mark::Radio(cur == Some((out, inc)))) {
                                let before = project.clone();
                                if edit::set_crossfade_curve(project, a, b, out, inc) {
                                    history.push(before);
                                }
                            }
                        }
                    });
                }
            });
        }
        Target::Fade(ref id, is_in) => {
            sub(ui, if is_in { "Fade &In" } else { "Fade &Out" }, true, |ui| {
                let dir = if is_in { "in from" } else { "out to" };
                for db in ["6.0 dB", "0 dB", "-Inf dB"] {
                    off(ui, &format!("Fade {dir} {db}"), NotPlanned("R58"));
                }
                sep(ui);
                let cur = project.clip(id).map(|c| if is_in { c.fade_in_curve } else { c.fade_out_curve });
                for (name, curve) in [("&Linear", Curve::Linear), ("S&mooth", Curve::Smooth), ("&Fast", Curve::Fast), ("&Slow", Curve::Slow), ("S&harp", Curve::Sharp)] {
                    if entry(ui, name, "", true, Mark::Radio(cur == Some(curve))) {
                        let before = project.clone();
                        if edit::set_fade_curve(project, id, is_in, curve) {
                            history.push(before);
                        }
                    }
                }
                sep(ui);
                off(ui, "Reset ASR", NotPlanned("R58"));
            });
        }
        Target::EnvPoint(ref track, kind, k) => env_point(ui, project, history, sel, st, track, kind, k),
        Target::EnvLine(ref track, kind, t, seg) => env_line(ui, project, history, sel, track, kind, t, seg),
        Target::Event(ref cut) => {
            let is_video = sel.clips.last().and_then(|id| project.find_clip(id)).is_some_and(|c| c.is_video);
            menus::show(ui, if is_video { VIDEO_EVENT } else { AUDIO_EVENT });
            if let Some((b, at)) = cut {
                sep(ui);
                transition_items(ui, project, history, b, *at, xf);
            } else if let Some(id) = sel.clips.last().filter(|id| project.clip(id).is_some_and(|c| c.transition.is_some())).cloned() {
                // A transition with nothing before it (a fade-in) is removed from its event's menu.
                sep(ui);
                transition_tail(ui, project, history, &id, true);
            }
        }
        Target::Lane(v, i) => lane(ui, project, history, &sel.tracks, st, v, i),
        Target::Selection => {
            off(ui, "&Insert Empty Event", Later("R3a"));
            cmd(ui, "&Zoom Selection", ZoomSelection);
        }
        Target::MarkerBar => marker_bar(ui, project, history),
        Target::Marker(i) => {
            let Some(m) = project.timeline.markers.get(i).cloned() else { return };
            if entry(ui, "&Go To", "", true, Mark::None) {
                project.timeline.cursor_position = m.position;
            }
            if entry(ui, "&Rename", "", true, Mark::None) {
                st.rename = Some((Rename::Marker(i), m.name));
            }
            sep(ui);
            if entry(ui, "&Delete", "", true, Mark::None) {
                history.push(project.clone());
                project.timeline.markers.remove(i);
                project.mark_modified();
            }
        }
        Target::RegionStart(i) | Target::RegionEnd(i) => {
            let Some(r) = project.timeline.regions.get(i).cloned() else { return };
            let start = matches!(tg, Target::RegionStart(_));
            if entry(ui, "&Go To", "", true, Mark::None) {
                project.timeline.cursor_position = if start { r.start } else { r.end };
            }
            let (other, at) = if start { ("Go to &End", r.end) } else { ("Go to S&tart", r.start) };
            if entry(ui, other, "", true, Mark::None) {
                project.timeline.cursor_position = at;
            }
            if entry(ui, "&Select Region", "", true, Mark::None) {
                project.timeline.loop_region = Some((r.start, r.end));
            }
            if start {
                off(ui, "&Rename", Later("R16a"));
            }
            sep(ui);
            off(ui, "&Create Subclip", Later("R3a"));
            sep(ui);
            if entry(ui, "&Delete", "", true, Mark::None) {
                history.push(project.clone());
                project.timeline.regions.remove(i);
                project.mark_modified();
            }
        }
        Target::LoopBar => menus::show(ui, LOOP_BAR),
        Target::Ruler => menus::show(ui, menus::RULER_MENU),
        Target::TimeDisplay => menus::show(ui, TIME_DISPLAY),
        Target::Drop(ref req) => drop_menu(ui, project, history, sel, drops, req),
    });
}

/// Right-drag drop menu: the media is added only once a choice is made (one undo step; the new events selected).
/// Add as Takes over an event adds the media to it as takes; elsewhere it makes a new event with them. Explorer
/// files go to the app (`dropped_at`), which places them the same way once probed.
fn drop_menu(ui: &mut Ui, project: &mut Project, history: &mut History, sel: &mut Selection, drops: &mut crate::ui::FileDrag, req: &DropReq) {
    use edit::DropMode::*;
    for (label, mode) in [("Add Across &Time", AcrossTime), ("Add Across T&racks", AcrossTracks), ("Add as Ta&kes", AsTakes)] {
        if entry(ui, label, "", true, Mark::None) {
            if !req.files.is_empty() {
                let track = req.target.and_then(|(v, i)| project.timeline.tracks(v).get(i)).map(|t| t.id.clone());
                drops.dropped_at = Some((req.files.clone(), track, req.time, mode));
                continue;
            }
            let before = project.clone();
            let ids = edit::drop_media_as(project, &req.idxs, req.range, req.target, req.time, req.still_len, req.xfade, mode);
            if !ids.is_empty() {

                history.push(before);
                (sel.clips, sel.list_focus, sel.env) = (ids, false, None);
            }
        }
    }
    sep(ui);
    entry(ui, "&Cancel", "", true, Mark::None);
}

/// Transition ▸ (the transitions, the one into event `b` checked; one picked goes on at time `at`, a cut converted
/// with `xf` = (cut-to-overlap, Automatic Crossfades)) and Remove Transition. Audio crossfades have no transitions.
fn transition_items(ui: &mut Ui, project: &mut Project, history: &mut History, b: &str, at: f64, xf: (f64, bool)) {
    let Some(r) = project.find_clip(b) else { return };
    let track = project.timeline.tracks(r.is_video)[r.track_idx].id.clone();
    let cur = project.clip(b).and_then(|c| c.transition.as_ref()).map(|t| t.plugin.clone());
    if !r.is_video {
        return off(ui, "&Transition", Fixed("audio crossfades have no transitions (use Fade Type)"));
    }
    sub(ui, "&Transition", true, |ui| {
        for info in plugins::catalog(PluginKind::Transition) {
            if entry(ui, info.name, "", true, Mark::Radio(cur.as_deref() == Some(info.id))) {
                let tr = edit::transition_of(&plugins::new_fx(info, info.presets.first()));
                let before = project.clone();
                if edit::set_transition(project, &track, at, tr, xf.0, xf.1) {
                    history.push(before);
                } else if !xf.1 && cur.is_none() {
                    crate::ui::hint(ui.ctx(), "Turn on Automatic Crossfades to convert a cut to a transition.");
                }
            }
        }
    });
    transition_tail(ui, project, history, b, cur.is_some());
}

/// Transition Properties... (its FX window) and Remove Transition, for the transition into event `id`.
fn transition_tail(ui: &mut Ui, project: &mut Project, history: &mut History, id: &str, has: bool) {
    if entry(ui, "Transition &Properties...", "", has, Mark::None) {
        crate::ui::open_tool(ui.ctx(), crate::ui::ToolWin::Fx(edit::FxTarget::Transition(id.to_string())));
    }
    if entry(ui, "&Remove Transition", "", has, Mark::None) {
        let before = project.clone();
        if edit::remove_transition(project, id) {
            history.push(before);
        }
    }
}

/// Mute / Solo / Lock (and Show in Preview on video) for one track.
fn switches(ui: &mut Ui, project: &mut Project, history: &mut History, v: bool, i: usize) {
    let tr = project.timeline.tracks(v)[i].clone();
    let mut toggle = |ui: &mut Ui, label: &str, on: bool, f: fn(&mut Track)| {
        if entry(ui, label, "", true, Mark::Check(on)) {
            history.push(project.clone());
            f(&mut project.timeline.tracks_mut(v)[i]);
            project.mark_modified();
        }
    };
    toggle(ui, "&Mute", tr.muted, |t| t.muted = !t.muted);
    toggle(ui, "&Solo", tr.solo, |t| t.solo = !t.solo);
    if !v {
        inert(ui, "&Invert Phase", Mark::Check(false), Later("R3b"));
    }
    // Milan Pro's own switches.
    toggle(ui, "&Lock", tr.locked, |t| t.locked = !t.locked);
    if v {
        toggle(ui, "Show in &Preview", tr.visible, |t| t.visible = !t.visible);
    }
}

/// Insert/Remove Envelope ▸ for one track: a check per envelope Milan Pro plays on it, the rest greyed out.
fn env_toggles(ui: &mut Ui, project: &mut Project, history: &mut History, v: bool, i: usize) {
    let items = if v {
        [("&Mute", None), ("&Composite Level", Some(EnvKind::CompositeLevel)), ("&Fade to Color", None)]
    } else {
        [("&Mute", Some(EnvKind::Mute)), ("&Volume", Some(EnvKind::Volume)), ("&Pan", Some(EnvKind::Pan))]
    };
    for (label, kind) in items {
        let (Some(kind), Some(tr)) = (kind, project.timeline.tracks(v).get(i)) else {
            off(ui, label, Later("R4a")); // video track Mute and Fade to Color
            continue;
        };
        let (id, has) = (tr.id.clone(), tr.envelopes.iter().any(|e| e.kind == kind));
        if entry(ui, label, "", !tr.locked, Mark::Check(has)) {
            let before = project.clone();
            if edit::set_envelope(project, &[id], kind, !has) {
                history.push(before);
            }
        }
    }
}

const ENV_CURVES: [(&str, Curve); 6] = [
    ("&Linear Fade", Curve::Linear),
    ("&Fast Fade", Curve::Fast),
    ("&Slow Fade", Curve::Slow),
    ("S&mooth Fade", Curve::Smooth),
    ("S&harp Fade", Curve::Sharp),
    ("H&old", Curve::Hold),
];

fn env_of<'a>(project: &'a Project, track: &str, kind: EnvKind) -> Option<&'a Envelope> {
    let (v, i) = project.find_track(track)?;
    project.timeline.tracks(v)[i].envelopes.iter().find(|e| e.kind == kind)
}

/// One undo step for an edit of envelope `kind` on track `track`.
fn env_edit(project: &mut Project, history: &mut History, track: &str, kind: EnvKind, f: impl FnOnce(&mut Envelope) -> bool) {
    let before = project.clone();
    if edit::edit_envelope(project, track, kind, f) {
        history.push(before);
    }
}

/// The curve items, checked by point `k`'s curve; they set the curve after each of points `keys`.
fn env_curves(ui: &mut Ui, project: &mut Project, history: &mut History, track: &str, kind: EnvKind, k: usize, keys: &[usize]) {
    let cur = env_of(project, track, kind).and_then(|e| e.points.keys.get(k)).map(|x| x.curve);
    for (name, curve) in ENV_CURVES {
        if entry(ui, name, "", true, Mark::Radio(cur == Some(curve))) {
            env_edit(project, history, track, kind, |e| edit::env_set_curve(e, keys, curve));
        }
    }
}

/// Envelope point menu (§6.7). It acts on the selected points when the clicked one is among them.
#[allow(clippy::too_many_arguments)]
fn env_point(ui: &mut Ui, project: &mut Project, history: &mut History, sel: &mut Selection, st: &mut TlState, track: &str, kind: EnvKind, k: usize) {
    let keys = sel.env.as_ref().filter(|s| s.track == track && s.kind == kind && s.keys.contains(&k)).map_or(vec![k], |s| s.keys.clone());
    let lane_end = |f: f32| edit::env_clamp(kind, edit::lane_value(kind, f));
    let (above, below) = envelopes::presets(kind);
    let named = |v: f32| (format!("Set to {}", envelopes::value_text(kind, v)), v);
    let items = std::iter::once(("Set to Ma&ximum".to_string(), lane_end(1.0)))
        .chain(above.into_iter().map(named))
        .chain([("Set to D&efault".to_string(), edit::env_default(kind))])
        .chain(below.into_iter().map(named))
        .chain([("Set to M&inimum".to_string(), lane_end(0.0))]);
    for (label, v) in items {
        if entry(ui, &label, "", true, Mark::None) {
            env_edit(project, history, track, kind, |e| edit::env_set_values(e, &keys, v));
        }
    }
    sep(ui);
    sub(ui, "Set &To", true, |ui| {
        let now = env_of(project, track, kind).and_then(|e| e.points.keys.get(k)).map_or(0.0, |x| x.v);
        let text = st.env_text.get_or_insert_with(|| envelopes::value_text(kind, now));
        let r = ui.add(TextEdit::singleline(text).desired_width(140.0));
        if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            if let Some(v) = envelopes::parse_value(kind, text) {
                env_edit(project, history, track, kind, |e| edit::env_set_values(e, &keys, v));
                ui.close_menu();
            }
        }
    });
    sep(ui);
    if entry(ui, "&Delete", "", keys.iter().any(|&i| i > 0), Mark::None) {
        env_edit(project, history, track, kind, |e| edit::env_delete(e, &keys));
        sel.env = None; // the indices after the deleted points have moved
    }
    sep(ui);
    env_curves(ui, project, history, track, kind, k, &keys);
    sep(ui);
    off(ui, "&Properties", Later("R8c"));
}

/// Envelope line menu (§6.7): `t` is where it was clicked, `seg` the point starting that segment.
#[allow(clippy::too_many_arguments)]
fn env_line(ui: &mut Ui, project: &mut Project, history: &mut History, sel: &mut Selection, track: &str, kind: EnvKind, t: f64, seg: usize) {
    if entry(ui, "Add &Point", "", true, Mark::None) {
        let mut k = None;
        env_edit(project, history, track, kind, |e| {
            k = Some(edit::env_insert(e, t, e.at(t)));
            true
        });
        if let Some(k) = k {
            sel.env = Some(EnvSel { track: track.to_string(), kind, keys: vec![k] });
        }
    }
    sep(ui);
    env_curves(ui, project, history, track, kind, seg, &[seg]);
    sep(ui);
    let all: [(&str, fn(&mut Envelope) -> bool); 3] = [("Fl&ip All Points", edit::env_flip), ("Thi&n All Points", edit::env_thin), ("&Reset All", edit::env_reset)];
    for (label, f) in all {
        if entry(ui, label, "", true, Mark::None) {
            env_edit(project, history, track, kind, f);
            sel.env = None; // Thin moves the indices
        }
    }
    sep(ui);
    off(ui, "&Properties", Later("R8c"));
}

/// Track ≡ / ⋯ button menu: the switches, default height and the button-set entry.
pub(super) fn more_menu(ui: &mut Ui, project: &mut Project, history: &mut History, sel: &mut Selection, v: bool, i: usize) {
    if i >= project.timeline.tracks(v).len() {
        return;
    }
    if v {
        // The hidden video buttons (§5.9.3): Track Motion and, in VP16, Track FX and Compositing Mode.
        let tr = &project.timeline.video_tracks[i];
        let (id, cur, locked) = (tr.id.clone(), tr.composite, tr.locked);
        if entry(ui, "Track &Motion...", "", true, Mark::None) {
            crate::ui::open_tool(ui.ctx(), crate::ui::ToolWin::TrackMotion(id.clone()));
        }
        // VP23 shows the fx and Comp buttons on the header, so only VP16 lists them here.
        if theme::get().look() == UiLook::Vegas16 {
            if entry(ui, "Track &FX...", "", true, Mark::None) {
                crate::ui::open_tool(ui.ctx(), crate::ui::ToolWin::Fx(edit::FxTarget::Track(id.clone())));
            }
            sub(ui, "&Compositing Mode", true, |ui| {
                if let Some(m) = crate::ui::trackmotion::composite_menu(ui, cur, !locked) {
                    let before = project.clone();
                    if edit::set_composite(project, &id, m) {
                        history.push(before);
                    }
                }
            });
        }
        sep(ui);
    } else {
        // Arm for Record leads the hidden audio buttons. Through the app (it owns the input device): this track
        // becomes the focus, then Arm toggles it, as the header's Arm button does.
        let (id, armed) = (project.timeline.audio_tracks[i].id.clone(), project.timeline.audio_tracks[i].armed);
        let st = menus::state(ui.ctx());
        if entry(ui, "&Arm for Record", &super::super::keymap::shortcut_text(Arm, st.look), !st.recording, Mark::Check(armed)) {
            (sel.tracks, sel.list_focus, sel.env) = (vec![id.clone()], true, None);
            menus::send(ui.ctx(), Arm);
        }
        // VP16 keeps the audio Track FX button hidden too (VP23 shows it on the header).
        if theme::get().look() == UiLook::Vegas16 && entry(ui, "Track &FX...", "", true, Mark::None) {
            crate::ui::open_tool(ui.ctx(), crate::ui::ToolWin::Fx(edit::FxTarget::Track(id)));
        }
        sep(ui);
    }
    switches(ui, project, history, v, i);
    if entry(ui, "Default Track &Height", "Ctrl+`", project.timeline.tracks(v)[i].height > 0.0, Mark::None) {
        project.timeline.tracks_mut(v)[i].height = 0.0;
        project.mark_modified(); // a view change: no undo step
    }
    sep(ui);
    off(ui, "Edit Visible Button Set...", Later("R15"));
}

fn duplicate_track(project: &mut Project, history: &mut History, v: bool, i: usize) {
    history.push(project.clone());
    let mut tr = project.timeline.tracks(v)[i].clone();
    tr.id = uuid::Uuid::new_v4().to_string();
    // The copies form groups of their own, not with the originals' mates.
    let mut groups = std::collections::HashMap::new();
    for c in &mut tr.clips {
        c.id = uuid::Uuid::new_v4().to_string();
        c.group = c.group.take().map(|g| groups.entry(g).or_insert_with(|| uuid::Uuid::new_v4().to_string()).clone());
    }
    project.timeline.tracks_mut(v).insert(i + 1, tr);
    edit::dissolve_lone_groups(&mut project.timeline); // a copy whose mates were on other tracks is alone
    project.mark_modified();
}

/// Delete track `i`, or every selected track when it is one of them.
fn delete_track(project: &mut Project, history: &mut History, sel: &[String], st: &mut TlState, v: bool, i: usize) {
    let before = project.clone();
    let id = project.timeline.tracks(v)[i].id.clone();
    let ids = if sel.contains(&id) { sel.to_vec() } else { vec![id] };
    if edit::delete_tracks(project, &ids) {
        history.push(before);
    }
    // Row numbers after the removed track shift (selections are ids and drop the gone ones themselves).
    st.anchor = None;
}

fn track_rows(ui: &mut Ui, project: &mut Project, history: &mut History, sel: &[String], st: &mut TlState, v: bool, i: usize) {
    if entry(ui, "D&uplicate Track", "", true, Mark::None) {
        duplicate_track(project, history, v, i);
    }
    if entry(ui, "&Delete Track", "", true, Mark::None) {
        delete_track(project, history, sel, st, v, i);
    }
}

fn track_group_and_color(ui: &mut Ui) {
    sub(ui, "Track &Group", true, |ui| menus::show(ui, TRACK_GROUP));
    sub(ui, "Trac&k Display Color", true, |ui| {
        let t = theme::get();
        for (k, name) in ["Color A", "Color B", "Color C", "Color D", "Color E", "Color F", "Color G", "Color H"].into_iter().enumerate() {
            let r = ui.add(Button::new(format!("        {name}")).sense(Sense::hover())).rect;
            ui.painter().rect_filled(Rect::from_min_size(r.left_center() + vec2(8.0, -6.0), vec2(16.0, 12.0)), 0.0, t.track_colors[k]);
        }
        sep(ui);
        off(ui, "Custom...", NotPlanned("R58"));
    });
}

fn track_header(ui: &mut Ui, project: &mut Project, history: &mut History, sel: &[String], st: &mut TlState, v: bool, i: usize) {
    if i >= project.timeline.tracks(v).len() {
        return;
    }
    let v23 = theme::get().look() == UiLook::Vegas23;
    let rename = |ui: &mut Ui, project: &Project, st: &mut TlState| {
        if entry(ui, "&Rename", "F2", true, Mark::None) {
            st.rename = Some((Rename::Track(v, i), project.timeline.tracks(v)[i].name.clone()));
        }
    };
    if v {
        rename(ui, project, st);
        sep(ui);
        cmd(ui, "Insert &Video Track", InsertVideoTrack);
        track_rows(ui, project, history, sel, st, v, i);
        if i >= project.timeline.tracks(v).len() {
            return;
        }
        sep(ui);
        sub(ui, "Insert/Remove &Envelope", true, |ui| env_toggles(ui, project, history, v, i));
        sep(ui);
        sub(ui, "&Switches", true, |ui| switches(ui, project, history, v, i));
        sub(ui, "Fade &Colors", true, |ui| menus::show(ui, FADE_COLORS));
        inert(ui, "E&xpand Track Layers", Mark::Check(false), NotPlanned("R58"));
        sep(ui);
        off(ui, "Set De&fault Track Properties...", Later("R18b"));
        sep(ui);
        track_group_and_color(ui);
        if v23 {
            off(ui, "Select All After Cursor", Later("R2"));
        }
    } else {
        off(ui, "Hide", Later("R5"));
        sep(ui);
        rename(ui, project, st);
        sep(ui);
        cmd(ui, "&Insert Audio Track", InsertAudioTrack);
        track_rows(ui, project, history, sel, st, v, i);
        if i >= project.timeline.tracks(v).len() {
            return;
        }
        sep(ui);
        menus::show(ui, AUDIO_HEADER_MID);
        sub(ui, "Insert/Remove &Envelope", true, |ui| env_toggles(ui, project, history, v, i));
        menus::show(ui, &[X("F&X Automation Envelopes...", Later("R8c")), S]);
        sub(ui, "&Switches", true, |ui| switches(ui, project, history, v, i));
        menus::show(ui, AUDIO_HEADER_END);
        track_group_and_color(ui);
        menus::show(ui, AUDIO_HEADER_METERS);
    }
}

fn lane(ui: &mut Ui, project: &mut Project, history: &mut History, sel: &[String], st: &mut TlState, v: bool, i: usize) {
    if i >= project.timeline.tracks(v).len() {
        return;
    }
    cmd(ui, "&Paste", Paste);
    sep(ui);
    menus::show(ui, &[C("&Delete", Delete), C("Tri&m", Trim), C("Trim &Start", TrimStart), C("Trim &End", TrimEnd), C("Spl&it", Split)]);
    sep(ui);
    if v {
        cmd(ui, "Insert &Video Track", InsertVideoTrack);
    } else {
        cmd(ui, "Insert &Audio Track", InsertAudioTrack);
    }
    track_rows(ui, project, history, sel, st, v, i);
    sep(ui);
    sub(ui, "Insert/Remove Envelope", true, |ui| env_toggles(ui, project, history, v, i));
    if v {
        sep(ui);
        sub(ui, "Fade Colors", true, |ui| menus::show(ui, FADE_COLORS));
        menus::show(ui, &[V23(&X("Insert Slideshow", Later("R18b")))]);
    } else {
        off(ui, "FX Automation Envelopes", Later("R8c"));
        sep(ui);
        menus::show(ui, &[K("Use Vertical Meters", false, Later("R8b")), K("Show Output Meters", true, Later("R8b"))]);
    }
}

const FADE_COLORS: &[M] = &[X("Top...", Later("R4a")), X("Bottom...", Later("R4a"))];

fn marker_bar(ui: &mut Ui, project: &mut Project, history: &mut History) {
    menus::show(ui, MARKER_BAR_TOP);
    sub(ui, "&Markers/Regions", true, |ui| {
        cmd(ui, "Insert Marker", InsertMarker);
        cmd(ui, "Insert Region", InsertRegion);
        sep(ui);
        let tl = &project.timeline;
        let any = !tl.markers.is_empty() || !tl.regions.is_empty();
        if entry(ui, "Delete All", "", any, Mark::None) {
            history.push(project.clone());
            project.timeline.markers.clear();
            project.timeline.regions.clear();
            project.mark_modified();
        }
        let sel = project.timeline.loop_region.filter(|(a, b)| b > a);
        if entry(ui, "Delete All in Selection", "", any && sel.is_some(), Mark::None) {
            if let Some((a, b)) = sel {
                history.push(project.clone());
                let tl = &mut project.timeline;
                tl.markers.retain(|m| m.position < a || m.position > b);
                tl.regions.retain(|r| r.end < a || r.start > b);
                project.mark_modified();
            }
        }
    });
    menus::show(ui, MARKER_BAR_BOTTOM);
}

const TRACK_GROUP: &[M] = &[
    X("Rename Track Group", Later("R5")),
    S,
    X("Group Selected Tracks", Later("R5")),
    X("Ungroup Selected Tracks", Later("R5")),
    X("Collapse", Later("R5")),
    X("Expand", Later("R5")),
    S,
    X("Mute Track Group", Later("R5")),
    X("Solo Track Group", Later("R5")),
];

const AUDIO_HEADER_MID: &[M] = &[
    X("Cu&t", Later("R5")),
    X("&Copy", Later("R5")),
    X("&Paste", Later("R5")),
    X("&Delete", Later("R5")),
    S,
    C("&Arm for Record", Arm),
    Sub("I&nput", &[X("None", Later("R9"))]),
    Sub("&Output", &[R("Master", true, Later("R8a"))]),
    Sub("Au&tomation", &[
        X("Show Automation Controls", Later("R8c")),
        S,
        R("Off", true, Later("R8c")),
        R("Read", false, Later("R8c")),
        R("Write (Touch)", false, Later("R8c")),
        R("Write (Latch)", false, Later("R8c")),
    ]),
    Sub("&Pan Type", &[
        R("Add Channels (0 dB Center)", false, Later("R8a")),
        R("Balance (0 dB Center)", true, Later("R8a")),
        R("Balance (-3 dB Center)", false, Later("R8a")),
        R("Balance (-6 dB Center)", false, Later("R8a")),
        R("Constant Power", false, Later("R8a")),
        R("Film", false, Later("R8a")),
    ]),
    S,
];

const AUDIO_HEADER_END: &[M] = &[S, C("Set De&fault Track Properties...", DefaultTrackFx), S];

const AUDIO_HEADER_METERS: &[M] = &[
    S,
    K("Use &Vertical Meters", false, Later("R8b")),
    K("Show Output &Meters", true, Later("R8b")),
    S,
    Sub("Channel Width", &[R("Narrow", false, Later("R8b")), R("Default", true, Later("R8b")), R("Wide", false, Later("R8b"))]),
];

const EVENT_SHARED: &[M] = &[
    S,
    C("Cu&t", Cut),
    C("&Copy", Copy),
    X("Paste E&vent Attributes", Later("R3a")),
    X("Selectively Paste Event Attributes...", Later("R3a")),
    S,
    C("&Delete", Delete),
    C("Tri&m", Trim),
    C("Trim &Start", TrimStart),
    C("Trim &End", TrimEnd),
    C("Spl&it", Split),
    S,
];

const EVENT_TAIL: &[M] = &[
    S,
    Sub("Switc&hes", &[
        C("&Mute", EventMute),
        C("&Lock", EventLock),
        K("L&oop", true, Later("R3a")),
        S,
        K("Maintain &Aspect Ratio", true, Later("R3b")),
        V23(&K("Auto Normalize", false, Later("R3b"))),
        V23(&R("Optical Flow", false, Later("R3b"))),
    ]),
    Sub("T&ake", menus::TAKE),
    Sub("&Group", &[C("Create &New", Group), S, C("&Remove From", RemoveFromGroup), C("&Clear", ClearGroup), C("&Select All", SelectGroup)]),
    Sub("&Stream", menus::STREAM),
    Sub("&Synchronize", &[X("By Moving", NotPlanned("R58")), X("By Slipping", NotPlanned("R58"))]),
    X("Create Sync &Link with Selected Events", NotPlanned("R58")),
    X("Remove Sync Link from Selected Events", NotPlanned("R58")),
    S,
    C("&Properties...", EventProperties),
    V23(&S),
    V23(&C("Motion Tracking", MotionTracking)),
    V23(&X("Add Missing Stream for Selected Event", NotPlanned("R58"))),
    V23(&X("Beat Detection", NotPlanned("R57"))),
];

const VIDEO_EVENT: &[M] = &[
    C("Open in Tri&mmer", EventOpenInTrimmer),
    X("Open Parent Media in Tri&mmer", NotPlanned("R57")),
    C("Select &in Project Media List", EventSelectInMedia),
    X("Edit Source Pro&ject", Later("R6b")),
    S,
    X("Media &FX...", NotPlanned("R57")),
    S,
    C("Video Event Pan/C&rop...", EventPanCrop),
    C("Video &Event FX...", VideoEventFx),
    X("Red E&ye Reduction...", NotPlanned("R58")),
    Inline(EVENT_SHARED),
    X("Create Su&bclip", Later("R3a")),
    C("Rever&se", EventReverse),
    S,
    X("Select Events to E&nd", Later("R2")),
    Sub("Insert/Remove &Envelope", &[
        X("Velocity", Later("R4a")),
        X("Freeze Frame at Cursor", Later("R4a")),
        X("Transition Progress", Later("R4a")),
    ]),
    Inline(EVENT_TAIL),
];

const AUDIO_EVENT: &[M] = &[
    C("Open in Tri&mmer", EventOpenInTrimmer),
    X("Open Parent Media in Tri&mmer", NotPlanned("R57")),
    C("Open in Audio Editor", OpenInAudioEditor),
    X("Open Copy in Audio Editor", Later("R18a")),
    X("Edit Source Pro&ject", Later("R6b")),
    C("Select &in Project Media List", EventSelectInMedia),
    S,
    C("Audio Event &FX...", AudioEventFx),
    X("Apply Non-Real-Time Event FX...", NotPlanned("R58")),
    Inline(EVENT_SHARED),
    X("Create Su&bclip", Later("R3a")),
    C("&Reverse", EventReverse),
    S,
    X("Select Events to E&nd", Later("R2")),
    Sub("Insert/Remove &Envelope", &[X("Volume", Later("R8c")), X("Pan", Later("R8c")), X("Mute", Later("R8c"))]),
    Sub("Channe&ls", menus::CHANNELS),
    Inline(EVENT_TAIL),
];

const MARKER_BAR_TOP: &[M] = &[
    C("&Loop Playback", Loop),
    S,
    C("Set Selection to &View", SelectionToView),
    C("Set Selection to &Project", SelectionToProject),
    C("Set Selection to &All", SelectionToProject),
    C("&Select Loop Region", SelectLoopRegion),
    S,
];

const MARKER_BAR_BOTTOM: &[M] = &[
    S,
    C("&Quantize to Frames", Quantize),
    S,
    C("&Enable Snapping", Snapping),
    C("&Snap to Grid", SnapGrid),
    C("S&nap to Markers", SnapMarkers),
    C("Snap to All Even&ts", SnapAllEvents),
    S,
    Sub("Gr&id Spacing", menus::GRID_SPACING),
    S,
    C("Selectively Pre&render Video...", Prerender),
    X("&Clean Up Prerendered Video...", Later("R23")),
];

const LOOP_BAR: &[M] = &[
    C("Insert &Region", InsertRegion),
    S,
    C("&Loop Playback", Loop),
    S,
    C("&Zoom Selection", ZoomSelection),
    C("Set Selection to &View", SelectionToView),
    C("Set Selection to &Project", SelectionToProject),
    C("&Select Loop Region", SelectLoopRegion),
    S,
    C("Render to New &Track...", RenderToNewTrack),
    C("Selectively Prerender V&ideo...", Prerender),
];

const TIME_DISPLAY: &[M] = &[
    R("Time at Cursor", true, Fixed("the only mode until MIDI timecode (VEGAS long tail R59)")),
    X("MIDI Timecode In", NotPlanned("R59")),
    X("MIDI Timecode Out", NotPlanned("R59")),
    X("MIDI Clock Out", NotPlanned("R59")),
    S,
    Sub("Time Format", menus::RULER_FORMATS),
    S,
    Sub("Text Color", &[R("Default", true, NotPlanned("R58")), X("Custom...", NotPlanned("R58"))]),
    Sub("Background Color", &[R("Default", true, NotPlanned("R58")), X("Custom...", NotPlanned("R58"))]),
];

/// The tables of the timeline's right-click menus, for the Unfinished Features list.
pub(crate) const MENUS: &[(&str, &[M])] = &[
    ("Track header", TRACK_GROUP),
    ("Track header", AUDIO_HEADER_MID),
    ("Track header", AUDIO_HEADER_METERS),
    ("Track header", FADE_COLORS),
    ("Video event", VIDEO_EVENT),
    ("Audio event", AUDIO_EVENT),
    ("Marker bar", MARKER_BAR_TOP),
    ("Marker bar", MARKER_BAR_BOTTOM),
    ("Time selection", LOOP_BAR),
    ("Time display", TIME_DISPLAY),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// A right-click on a fade handle or an envelope gets their menus; with envelopes hidden, the lane's.
    #[test]
    fn right_click_targets_fades_and_envelopes() {
        let mut p = Project::new("t");
        let mut c = Clip::new("v", 2.0, 4.0); // x 20..60, fade-in handle at 30
        c.fade_in = 1.0;
        let id = c.id.clone();
        p.timeline.video_tracks[0].clips.push(c);
        edit::toggle_envelope(&mut p.timeline.audio_tracks[0], EnvKind::Pan); // centre: y 74 on the row 50..100
        let a1 = p.timeline.audio_tracks[0].id.clone();
        let lanes = Rect::from_min_max(Pos2::ZERO, pos2(1000.0, 300.0));
        let g = Geo { rect: lanes, header_w: 0.0, list: Rect::ZERO, band: Rect::ZERO, lanes, vscroll: Rect::ZERO, hscroll: Rect::ZERO, pps: 10.0, scroll_x: 0.0, grid: (1.0, 0.0), grid_snap: false };
        let rows = [Row { is_video: true, idx: 0, num: 0, top: 0.0, h: 50.0 }, Row { is_video: false, idx: 0, num: 1, top: 50.0, h: 50.0 }];
        let sel = Selection::default();
        assert_eq!(target(&g, &rows, &p, &sel, pos2(30.0, 3.0), true), Some(Target::Fade(id.clone(), true)));
        assert_eq!(target(&g, &rows, &p, &sel, pos2(59.0, 3.0), true), Some(Target::Fade(id.clone(), false)));
        assert_eq!(target(&g, &rows, &p, &sel, pos2(2.0, 74.0), true), Some(Target::EnvPoint(a1.clone(), EnvKind::Pan, 0)));
        assert_eq!(target(&g, &rows, &p, &sel, pos2(500.0, 75.0), true), Some(Target::EnvLine(a1, EnvKind::Pan, 50.0, 0)));
        assert_eq!(target(&g, &rows, &p, &sel, pos2(500.0, 75.0), false), Some(Target::Lane(false, 0)));
        // A Composite Level envelope at 100% runs along the top (y 2): the fade handles still win over its line.
        edit::toggle_envelope(&mut p.timeline.video_tracks[0], EnvKind::CompositeLevel);
        let v1 = p.timeline.video_tracks[0].id.clone();
        assert_eq!(target(&g, &rows, &p, &sel, pos2(30.0, 3.0), true), Some(Target::Fade(id, true)));
        assert_eq!(target(&g, &rows, &p, &sel, pos2(500.0, 3.0), true), Some(Target::EnvLine(v1, EnvKind::CompositeLevel, 50.0, 0)));
    }
}
