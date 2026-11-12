//! Timeline right-click menus (§6.8 tracks and lanes, §6.9 events, §6.10 marker bar, ruler and time display).
//! One handler sees every right-click in the timeline, including ones a child widget (track header, marker tag,
//! loop bar) took, works out what was clicked and shows that menu. Global actions go to the command bus;
//! actions on the clicked track / marker / region are done here.
use eframe::egui::*;
use crate::history::History;
use crate::project::*;
use super::super::keymap::Command::*;
use super::super::menus::{self, M, M::*, Mark, cmd, entry, off, sep, sub};
use super::super::theme::{self, UiLook};
use super::ruler::band_rows;
use super::{ClipRef, Geo, Rename, Row, TlState};

#[derive(Clone, Copy, PartialEq, Debug)]
enum Target {
    TrackHeader(bool, usize),
    ListEmpty,
    Event,
    Lane(bool, usize),
    Selection,
    MarkerBar,
    Marker(usize),
    RegionStart(usize),
    RegionEnd(usize),
    LoopBar,
    Ruler,
    TimeDisplay,
}

const TAG: Vec2 = vec2(13.0, 14.0);

fn target(g: &Geo, rows: &[Row], project: &Project, sel: &Option<ClipRef>, p: Pos2) -> Option<Target> {
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
        if sel.is_some() {
            return Some(Target::Event);
        }
        let time = g.to_time(p.x);
        if tl.loop_region.is_some_and(|(a, b)| b > a && time >= a && time <= b) {
            return Some(Target::Selection);
        }
        return row.map(|r| Target::Lane(r.is_video, r.idx));
    }
    None
}

/// Open the menu for a right-click anywhere in the timeline and keep showing it while open.
#[allow(clippy::too_many_arguments)]
pub(super) fn menu(
    ui: &Ui,
    response: &Response,
    g: &Geo,
    rows: &[Row],
    project: &mut Project,
    history: &mut History,
    selected_clip: &mut Option<ClipRef>,
    st: &mut TlState,
) {
    let tid = ui.id().with("ctx_target");
    let clicked = ui.input(|i| i.pointer.button_clicked(PointerButton::Secondary).then(|| i.pointer.interact_pos()).flatten());
    let open_at = clicked
        .filter(|p| g.rect.contains(*p) && ui.ctx().layer_id_at(*p) == Some(ui.layer_id()))
        .and_then(|p| target(g, rows, project, selected_clip, p).map(|t| (p, t)));
    if let Some((_, tg)) = open_at {
        ui.data_mut(|d| d.insert_temp(tid, tg));
    }
    let Some(tg) = ui.data(|d| d.get_temp::<Target>(tid)) else { return };
    menus::context_at(response, open_at.map(|o| o.0), |ui| match tg {
        Target::TrackHeader(v, i) => track_header(ui, project, history, selected_clip, st, v, i),
        Target::ListEmpty => {
            cmd(ui, "&Open...", Open);
            cmd(ui, "&Insert Audio Track", InsertAudioTrack);
            cmd(ui, "Insert &Video Track", InsertVideoTrack);
            sep(ui);
            cmd(ui, "&Preferences...", Preferences);
        }
        Target::Event => {
            let is_video = selected_clip.as_ref().is_some_and(|c| c.is_video);
            menus::show(ui, if is_video { VIDEO_EVENT } else { AUDIO_EVENT });
        }
        Target::Lane(v, i) => lane(ui, project, history, selected_clip, st, v, i),
        Target::Selection => {
            off(ui, "&Insert Empty Event");
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
                off(ui, "&Rename");
            }
            sep(ui);
            off(ui, "&Create Subclip");
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
    });
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
        entry(ui, "&Invert Phase", "", false, Mark::Check(false));
    }
    // Milan Pro's own switches.
    toggle(ui, "&Lock", tr.locked, |t| t.locked = !t.locked);
    if v {
        toggle(ui, "Show in &Preview", tr.visible, |t| t.visible = !t.visible);
    }
}

/// Track ≡ / ⋯ button menu: the switches, default height and the button-set entry.
pub(super) fn more_menu(ui: &mut Ui, project: &mut Project, history: &mut History, v: bool, i: usize) {
    if i >= project.timeline.tracks(v).len() {
        return;
    }
    switches(ui, project, history, v, i);
    if entry(ui, "Default Track &Height", "Ctrl+`", project.timeline.tracks(v)[i].height > 0.0, Mark::None) {
        project.timeline.tracks_mut(v)[i].height = 0.0;
    }
    sep(ui);
    off(ui, "Edit Visible Button Set...");
}

fn duplicate_track(project: &mut Project, history: &mut History, v: bool, i: usize) {
    history.push(project.clone());
    let mut tr = project.timeline.tracks(v)[i].clone();
    tr.id = uuid::Uuid::new_v4().to_string();
    for c in &mut tr.clips {
        c.id = uuid::Uuid::new_v4().to_string();
    }
    project.timeline.tracks_mut(v).insert(i + 1, tr);
    project.mark_modified();
}

fn delete_track(project: &mut Project, history: &mut History, sel: &mut Option<ClipRef>, st: &mut TlState, v: bool, i: usize) {
    history.push(project.clone());
    project.timeline.tracks_mut(v).remove(i);
    project.mark_modified();
    // Indices after the removed track shift; drop selections that may point at the wrong track.
    *sel = None;
    st.sel_tracks.clear();
    st.anchor = None;
}

fn track_rows(ui: &mut Ui, project: &mut Project, history: &mut History, sel: &mut Option<ClipRef>, st: &mut TlState, v: bool, i: usize) {
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
        off(ui, "Custom...");
    });
}

fn track_header(ui: &mut Ui, project: &mut Project, history: &mut History, sel: &mut Option<ClipRef>, st: &mut TlState, v: bool, i: usize) {
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
        sub(ui, "Insert/Remove &Envelope", true, |ui| menus::show(ui, &[X("Mute"), X("Composite Level"), X("Fade to Color")]));
        sep(ui);
        sub(ui, "&Switches", true, |ui| switches(ui, project, history, v, i));
        sub(ui, "Fade &Colors", true, |ui| menus::show(ui, &[X("Top..."), X("Bottom...")]));
        entry(ui, "E&xpand Track Layers", "", false, Mark::Check(false));
        sep(ui);
        off(ui, "Set De&fault Track Properties...");
        sep(ui);
        track_group_and_color(ui);
        if v23 {
            off(ui, "Select All After Cursor");
        }
    } else {
        off(ui, "Hide");
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
        sub(ui, "&Switches", true, |ui| switches(ui, project, history, v, i));
        menus::show(ui, AUDIO_HEADER_END);
        track_group_and_color(ui);
        menus::show(ui, AUDIO_HEADER_METERS);
    }
}

fn lane(ui: &mut Ui, project: &mut Project, history: &mut History, sel: &mut Option<ClipRef>, st: &mut TlState, v: bool, i: usize) {
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
    sub(ui, "Insert/Remove Envelope", true, |ui| {
        menus::show(ui, if v { &[X("Mute"), X("Composite Level"), X("Fade to Color")] } else { &[X("Mute"), X("Volume"), X("Pan")] })
    });
    if v {
        sep(ui);
        sub(ui, "Fade Colors", true, |ui| menus::show(ui, &[X("Top..."), X("Bottom...")]));
        menus::show(ui, &[V23(&X("Insert Slideshow"))]);
    } else {
        off(ui, "FX Automation Envelopes");
        sep(ui);
        menus::show(ui, &[K("Use Vertical Meters", false), K("Show Output Meters", true)]);
    }
}

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
    X("Rename Track Group"),
    S,
    X("Group Selected Tracks"),
    X("Ungroup Selected Tracks"),
    X("Collapse"),
    X("Expand"),
    S,
    X("Mute Track Group"),
    X("Solo Track Group"),
];

const AUDIO_HEADER_MID: &[M] = &[
    X("Cu&t"),
    X("&Copy"),
    X("&Paste"),
    X("&Delete"),
    S,
    X("&Arm for Record"),
    Sub("I&nput", &[X("None")]),
    Sub("&Output", &[R("Master", true)]),
    Sub("Au&tomation", &[X("Show Automation Controls"), S, R("Off", true), R("Read", false), R("Write (Touch)", false), R("Write (Latch)", false)]),
    Sub("&Pan Type", &[
        R("Add Channels (0 dB Center)", false),
        R("Balance (0 dB Center)", true),
        R("Balance (-3 dB Center)", false),
        R("Balance (-6 dB Center)", false),
        R("Constant Power", false),
        R("Film", false),
    ]),
    S,
    Sub("Insert/Remove &Envelope", &[X("Mute"), X("Volume"), X("Pan")]),
    X("F&X Automation Envelopes..."),
    S,
];

const AUDIO_HEADER_END: &[M] = &[S, X("Set De&fault Track Properties..."), S];

const AUDIO_HEADER_METERS: &[M] = &[
    S,
    K("Use &Vertical Meters", false),
    K("Show Output &Meters", true),
    S,
    Sub("Channel Width", &[R("Narrow", false), R("Default", true), R("Wide", false)]),
];

const EVENT_SHARED: &[M] = &[
    S,
    C("Cu&t", Cut),
    C("&Copy", Copy),
    X("Paste E&vent Attributes"),
    X("Selectively Paste Event Attributes..."),
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
        K("&Mute", false),
        C("&Lock", EventLock),
        K("L&oop", true),
        S,
        K("Maintain &Aspect Ratio", true),
        V23(&K("Auto Normalize", false)),
        V23(&R("Optical Flow", false)),
    ]),
    Sub("T&ake", &[X("&Rename Active"), X("&Choose Active..."), S, C("&Next Take", NextTake), C("&Previous Take", PrevTake)]),
    Sub("&Group", &[C("Create &New", Group), S, C("&Remove From", RemoveFromGroup), C("&Clear", ClearGroup), C("&Select All", SelectGroup)]),
    Sub("&Stream", &[X("Stream &1")]),
    Sub("&Synchronize", &[X("By Moving"), X("By Slipping")]),
    X("Create Sync &Link with Selected Events"),
    X("Remove Sync Link from Selected Events"),
    S,
    C("&Properties...", EventProperties),
    V23(&S),
    V23(&C("Motion Tracking", MotionTracking)),
    V23(&X("Add Missing Stream for Selected Event")),
    V23(&X("Beat Detection")),
];

const VIDEO_EVENT: &[M] = &[
    C("Open in Tri&mmer", EventOpenInTrimmer),
    X("Open Parent Media in Tri&mmer"),
    C("Select &in Project Media List", EventSelectInMedia),
    X("Edit Source Pro&ject"),
    S,
    X("Media &FX..."),
    S,
    X("Video Event Pan/C&rop..."),
    X("Video &Event FX..."),
    X("Red E&ye Reduction..."),
    Inline(EVENT_SHARED),
    X("Create Su&bclip"),
    X("Rever&se"),
    S,
    X("Select Events to E&nd"),
    Sub("Insert/Remove &Envelope", &[X("Velocity"), X("Freeze Frame at Cursor"), X("Transition Progress")]),
    Inline(EVENT_TAIL),
];

const AUDIO_EVENT: &[M] = &[
    C("Open in Tri&mmer", EventOpenInTrimmer),
    X("Open Parent Media in Tri&mmer"),
    C("Open in Audio Editor", OpenInAudioEditor),
    X("Open Copy in Audio Editor"),
    X("Edit Source Pro&ject"),
    C("Select &in Project Media List", EventSelectInMedia),
    S,
    X("Audio Event FX..."),
    X("Apply Non-Real-Time Event FX..."),
    Inline(EVENT_SHARED),
    X("Create Su&bclip"),
    X("&Reverse"),
    S,
    X("Select Events to E&nd"),
    Sub("Insert/Remove &Envelope", &[X("Volume"), X("Pan"), X("Mute")]),
    Sub("Channe&ls", &[R("&Both", true), R("&Left Only", false), R("&Right Only", false), R("&Combine", false), R("&Swap", false)]),
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
    X("&Clean Up Prerendered Video..."),
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
    R("Time at Cursor", true),
    X("MIDI Timecode In"),
    X("MIDI Timecode Out"),
    X("MIDI Clock Out"),
    S,
    Sub("Time Format", menus::RULER_MENU),
    S,
    Sub("Text Color", &[R("Default", true), X("Custom...")]),
    Sub("Background Color", &[R("Default", true), X("Custom...")]),
];
