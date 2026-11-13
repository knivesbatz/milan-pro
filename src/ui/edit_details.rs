//! R16a: the Edit Details window (View › Window › Edit Details): the project as tables, chosen with Show (VEGAS's
//! views: Audio CD Track List, Commands, Events, Markers, Regions, Selected Events). A heading click sorts by that
//! column (again: the other way), a double-click edits a cell, a click toggles a check cell. Edits go through the same
//! edit functions as the timeline (`apply`), so locks, grouping, Auto Ripple and auto crossfades behave the same, and
//! each is one undo step.
use eframe::egui::*;
use crate::edit::{self, RippleOp};
use crate::history::History;
use crate::project::{Clip, Project, exact_fps};
use super::{EditOpts, Selection, theme, widgets};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Show {
    AudioCd,
    Commands,
    Events,
    Markers,
    Regions,
    SelectedEvents,
}

impl Show {
    const ALL: [Show; 6] = [Show::AudioCd, Show::Commands, Show::Events, Show::Markers, Show::Regions, Show::SelectedEvents];

    fn name(self) -> &'static str {
        ["Audio CD Track List", "Commands", "Events", "Markers", "Regions", "Selected Events"][self as usize]
    }

    fn cols(self) -> &'static [Col] {
        use Col::*;
        match self {
            Show::AudioCd => &[CdTrack, Start, End, Length, Name, Isrc, Prot, Emph],
            Show::Commands => &[Position, Command, Parameter, Comment],
            // shortcut: VEGAS's Timecode In / Out, Tape Name and Snap Offset are left out: Milan Pro keeps no tape names or
            // snap offsets; Timecode In / Out could come from `MediaItem.timecode` (R17a) when they are wanted.
            Show::Events | Show::SelectedEvents => &[Track, Start, End, Length, Takes, TakeName, TakeStart, FilePath, Select, Mute, Loop, Lock, Normalize],
            Show::Markers => &[Position, Name],
            Show::Regions => &[Start, End, Length, Name],
        }
    }
}

/// A column of some view.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Col {
    Track,
    Start,
    End,
    Length,
    Takes,
    TakeName,
    TakeStart,
    FilePath,
    Select,
    Mute,
    Loop,
    Lock,
    Normalize,
    Position,
    Name,
    Command,
    Parameter,
    Comment,
    CdTrack,
    Isrc,
    Prot,
    Emph,
}

impl Col {
    fn title(self) -> &'static str {
        use Col::*;
        match self {
            Track | CdTrack => "Track",
            Start => "Start",
            End => "End",
            Length => "Length",
            Takes => "Number of Takes",
            TakeName => "Active Take Name",
            TakeStart => "Take Start",
            FilePath => "File Path",
            Select => "Select",
            Mute => "Mute",
            Loop => "Loop",
            Lock => "Lock",
            Normalize => "Normalize",
            Position => "Position",
            Name => "Name",
            Command => "Command",
            Parameter => "Parameter",
            Comment => "Comment",
            Isrc => "ISRC",
            Prot => "Prot.",
            Emph => "Emph.",
        }
    }

    fn width(self) -> f32 {
        use Col::*;
        match self {
            Track | CdTrack | Prot | Emph => 50.0,
            Select | Mute | Loop | Lock => 52.0,
            Normalize => 72.0,
            Takes => 112.0,
            TakeName | Name | Parameter | Comment => 170.0,
            FilePath => 280.0,
            Isrc | Command => 110.0,
            _ => 100.0,
        }
    }
}

/// What a row shows: an event (by id), a marker or a region (by index).
#[derive(Clone, PartialEq, Debug)]
pub enum Item {
    Event(String),
    Marker(usize),
    Region(usize),
}

/// How a cell is edited.
#[derive(Clone, Copy, PartialEq)]
enum Edit {
    No,
    Text,
    Check(bool),
}

/// A cell's sort value.
#[derive(Clone, PartialEq)]
enum Sort {
    N(f64),
    T(String),
}

impl Sort {
    fn cmp(&self, o: &Sort) -> std::cmp::Ordering {
        match (self, o) {
            (Sort::N(a), Sort::N(b)) => a.total_cmp(b),
            (Sort::T(a), Sort::T(b)) => a.to_lowercase().cmp(&b.to_lowercase()),
            (Sort::N(_), Sort::T(_)) => std::cmp::Ordering::Less,
            (Sort::T(_), Sort::N(_)) => std::cmp::Ordering::Greater,
        }
    }
}

struct Row {
    key: Item,
    cells: Vec<(String, Sort, Edit)>,
}

/// A cell edit: typed text, or a check cell clicked.
pub enum Input {
    Text(String),
    Toggle,
}

/// The window's state (not saved).
pub struct EditDetails {
    show: Show,
    /// The column sorted by, descending when true.
    sort: Option<(Col, bool)>,
    /// The cell being typed in, its text, and whether it has had the focus yet.
    editing: Option<(Item, Col, String, bool)>,
    /// The row clicked last (highlighted).
    row: Option<Item>,
}

impl Default for EditDetails {
    fn default() -> Self {
        Self { show: Show::Events, sort: None, editing: None, row: None }
    }
}

const ROW_H: f32 = 20.0;

/// The toolbar: Show ▾.
pub fn edit_details_toolbar(ui: &mut Ui, v: &mut EditDetails) {
    ui.add_space(6.0);
    ui.label("Show:");
    let before = v.show;
    ComboBox::from_id_salt("ed_show").width(170.0).selected_text(v.show.name()).show_ui(ui, |ui| {
        for s in Show::ALL {
            ui.selectable_value(&mut v.show, s, s.name());
        }
    });
    if v.show != before {
        (v.sort, v.editing, v.row) = (None, None, None);
    }
}

/// The table. Some(ripple) after an edit (one undo step): what Post-Edit Ripple may still apply (empty when Auto
/// Ripple already did, or for an edit that does not ripple).
pub fn edit_details(ui: &mut Ui, v: &mut EditDetails, project: &mut Project, history: &mut History, sel: &mut Selection, opts: &EditOpts) -> Option<Vec<RippleOp>> {
    let t = theme::get();
    let cols = v.show.cols();
    let mut rows = rows(project, &sel.clips, v.show);
    if let Some(k) = v.sort.and_then(|(c, _)| cols.iter().position(|x| *x == c)) {
        let desc = v.sort.is_some_and(|s| s.1);
        rows.sort_by(|a, b| if desc { b.cells[k].1.cmp(&a.cells[k].1) } else { a.cells[k].1.cmp(&b.cells[k].1) });
    }
    let mut pending: Option<(Item, Col, Input)> = None;
    ScrollArea::both().id_salt("edit_details").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        let titles: Vec<String> = cols
            .iter()
            .map(|c| match v.sort {
                Some((s, desc)) if s == *c => format!("{} {}", c.title(), if desc { '▼' } else { '▲' }),
                _ => c.title().to_string(),
            })
            .collect();
        let head: Vec<(&str, f32)> = titles.iter().zip(cols).map(|(s, c)| (s.as_str(), c.width())).collect();
        if let Some(k) = widgets::header_row(ui, &head) {
            v.sort = Some((cols[k], v.sort == Some((cols[k], false))));
        }
        if rows.is_empty() && matches!(v.show, Show::AudioCd | Show::Commands) {
            // Their markers cannot be inserted yet (Insert › Audio CD Track Region / Command are greyed).
            ui.add_space(6.0);
            ui.label(RichText::new("No markers of this kind: inserting them is not available yet (batch R18a).").color(t.text_dim));
        }
        for row in &rows {
            let selected = v.row.as_ref() == Some(&row.key);
            let r = widgets::list_row(ui, ROW_H, selected);
            if r.clicked() {
                v.row = Some(row.key.clone());
            }
            let color = if selected { Color32::WHITE } else { t.text };
            let mut x = r.rect.min.x;
            for (col, (text, _, how)) in cols.iter().zip(&row.cells) {
                let cell = Rect::from_min_size(pos2(x, r.rect.min.y), vec2(col.width(), ROW_H));
                x += col.width();
                match *how {
                    Edit::Check(on) => {
                        let mut b = on;
                        let at = Rect::from_center_size(cell.center(), vec2(18.0, 18.0));
                        if ui.put(at, Checkbox::without_text(&mut b)).clicked() {
                            pending = Some((row.key.clone(), *col, Input::Toggle));
                        }
                        continue;
                    }
                    Edit::Text if r.double_clicked() && r.interact_pointer_pos().is_some_and(|p| cell.contains(p)) => {
                        v.editing = Some((row.key.clone(), *col, text.clone(), false));
                    }
                    _ => {}
                }
                match &mut v.editing {
                    Some((k, c, buf, focused)) if *k == row.key && c == col => {
                        let resp = ui.put(cell.shrink2(vec2(1.0, 0.0)), TextEdit::singleline(buf).font(t.font(t.font.ui)).margin(vec2(4.0, 1.0)));
                        if !*focused {
                            resp.request_focus();
                            *focused = true;
                        } else if resp.lost_focus() {
                            if !ui.input(|i| i.key_pressed(Key::Escape)) && *buf != *text {
                                pending = Some((row.key.clone(), *col, Input::Text(buf.clone())));
                            }
                            v.editing = None;
                        }
                    }
                    _ => {
                        let clip = cell.shrink2(vec2(2.0, 0.0)).intersect(ui.clip_rect());
                        ui.painter().with_clip_rect(clip).text(pos2(cell.min.x + 6.0, cell.center().y), Align2::LEFT_CENTER, text, t.font(t.font.ui), color);
                    }
                }
            }
        }
    });
    let (key, col, input) = pending?;
    if col == Col::Select {
        if let Item::Event(id) = key {
            match sel.clips.iter().position(|c| *c == id) {
                Some(i) => {
                    sel.clips.remove(i);
                }
                None => sel.clips.push(id),
            }
            sel.env = None;
        }
        return None;
    }
    let before = project.clone();
    let ops = apply(project, &key, col, input, opts)?;
    history.push(before);
    Some(ops)
}

/// The rows of `show` in timeline order: events by track, then by start; markers and regions as kept.
fn rows(p: &Project, sel: &[String], show: Show) -> Vec<Row> {
    let s = &p.settings;
    let tl = &p.timeline;
    let time = |t: f64| (s.ruler_time(t), Sort::N(t), Edit::Text);
    let len = |d: f64| (s.ruler_len(d), Sort::N(d), Edit::Text);
    let text = |x: &str, how: Edit| (x.to_string(), Sort::T(x.to_string()), how);
    let check = |on: bool| (String::new(), Sort::N(on as u8 as f64), Edit::Check(on));
    match show {
        Show::AudioCd | Show::Commands => Vec::new(),
        Show::Markers => tl.markers.iter().enumerate().map(|(i, m)| Row { key: Item::Marker(i), cells: vec![time(m.position), text(&m.name, Edit::Text)] }).collect(),
        Show::Regions => tl
            .regions
            .iter()
            .enumerate()
            .map(|(i, r)| Row { key: Item::Region(i), cells: vec![time(r.start), time(r.end), len(r.end - r.start), text(&r.name, Edit::Text)] })
            .collect(),
        Show::Events | Show::SelectedEvents => {
            let mut out = Vec::new();
            for (n, (is_video, c)) in tl.video_tracks.iter().map(|t| (true, t)).chain(tl.audio_tracks.iter().map(|t| (false, t))).enumerate().flat_map(|(n, (v, t))| t.clips.iter().map(move |c| (n, (v, c)))) {
                if show == Show::SelectedEvents && !sel.contains(&c.id) {
                    continue;
                }
                let file = edit::media_of(p, c).map_or(String::new(), |m| if m.generator.is_some() { String::new() } else { m.path.display().to_string() });
                let takes = c.takes.len() + c.media_id.is_some() as usize;
                let cells = vec![
                    ((n + 1).to_string(), Sort::N((n + 1) as f64), Edit::Text),
                    time(c.position),
                    time(c.position + c.duration),
                    len(c.duration),
                    (takes.to_string(), Sort::N(takes as f64), Edit::No),
                    text(&c.name, Edit::Text),
                    len(c.in_point),
                    text(&file, Edit::No),
                    check(sel.contains(&c.id)),
                    check(c.muted),
                    check(c.looping),
                    check(c.locked),
                    if is_video { text("", Edit::No) } else { check(c.normalize) },
                ];
                out.push(Row { key: Item::Event(c.id.clone()), cells });
            }
            out
        }
    }
}

/// Carry out one cell edit as the timeline would. Some(ripple) when something changed (see `edit_details`).
pub fn apply(p: &mut Project, key: &Item, col: Col, input: Input, opts: &EditOpts) -> Option<Vec<RippleOp>> {
    let s = p.settings.clone();
    let text = match &input {
        Input::Text(t) => Some(t.trim()),
        Input::Toggle => None,
    };
    match key {
        Item::Marker(i) => {
            let m = p.timeline.markers.get_mut(*i)?;
            match col {
                Col::Position => {
                    let at = s.parse_ruler_time(text?)?.max(0.0);
                    (m.position != at).then(|| m.position = at)?;
                }
                Col::Name => (m.name != text?).then(|| m.name = text.unwrap_or_default().to_string())?,
                _ => return None,
            }
            p.mark_modified();
            Some(Vec::new())
        }
        Item::Region(i) => {
            let r = p.timeline.regions.get_mut(*i)?;
            let (a, b) = match col {
                // A new start moves the region (its length stays), as Start does to an event.
                Col::Start => {
                    let a = s.parse_ruler_time(text?)?.max(0.0);
                    (a, a + r.end - r.start)
                }
                Col::End => (r.start, s.parse_ruler_time(text?)?),
                Col::Length => (r.start, r.start + s.parse_ruler_len(text?)?),
                Col::Name => {
                    (r.name != text?).then(|| r.name = text.unwrap_or_default().to_string())?;
                    p.mark_modified();
                    return Some(Vec::new());
                }
                _ => return None,
            };
            ((b > a) && (a, b) != (r.start, r.end)).then(|| (r.start, r.end) = (a, b))?;
            p.mark_modified();
            Some(Vec::new())
        }
        Item::Event(id) => {
            let r = p.find_clip(id)?;
            let c = p.clip(id)?.clone();
            // A locked event, or any event on a locked track, keeps everything but its Lock switch (as on the timeline).
            if col != Col::Lock && (c.locked || p.timeline.tracks(r.is_video)[r.track_idx].locked) {
                return None;
            }
            match col {
                Col::Start => move_event(p, id, &c, s.parse_ruler_time(text?)?.max(0.0), opts),
                Col::End => trim_end(p, id, &c, s.parse_ruler_time(text?)?, opts),
                Col::Length => trim_end(p, id, &c, c.position + s.parse_ruler_len(text?)?, opts),
                Col::Track => {
                    // Track numbers count the video tracks, then the audio tracks, from 1; an event stays its kind.
                    let n: usize = text?.parse().ok()?;
                    let nv = p.timeline.video_tracks.len();
                    let to = if r.is_video { n.checked_sub(1).filter(|&i| i < nv)? } else { n.checked_sub(nv + 1).filter(|&i| i < p.timeline.audio_tracks.len())? };
                    let ids = with_group(p, id, opts);
                    let before = p.clone();
                    edit::move_clips(p, &ids, 0.0, to as isize - r.track_idx as isize, opts.lock_envelopes).then_some(())?;
                    Some(finish(p, &before, &ids, Vec::new(), opts))
                }
                _ => {
                    let mut d = c.clone();
                    match col {
                        Col::TakeName => d.name = text?.to_string(),
                        Col::TakeStart => d.in_point = s.parse_ruler_len(text?)?,
                        Col::Mute => d.muted = !d.muted,
                        Col::Loop => d.looping = !d.looping,
                        Col::Lock => d.locked = !d.locked,
                        Col::Normalize if !r.is_video => d.normalize = !d.normalize,
                        _ => return None,
                    }
                    edit::apply_props(p, id, d).then(Vec::new)
                }
            }
        }
    }
}

/// `id` and, unless Ignore Event Grouping is on, its group mates.
fn with_group(p: &Project, id: &str, opts: &EditOpts) -> Vec<String> {
    if opts.ignore_grouping { vec![id.to_string()] } else { edit::expand_groups(p, &[id.to_string()]) }
}

/// The ids' tracks (ids, deduplicated) in `p`.
fn tracks_of(p: &Project, ids: &[String]) -> Vec<String> {
    let mut t: Vec<String> = p.find_clips(ids).iter().map(|r| p.timeline.tracks(r.is_video)[r.track_idx].id.clone()).collect();
    t.sort();
    t.dedup();
    t
}

/// Start: the event (and its group) moves to `to`, rippling from where they were, as a Move drag.
fn move_event(p: &mut Project, id: &str, o: &Clip, to: f64, opts: &EditOpts) -> Option<Vec<RippleOp>> {
    let ids = with_group(p, id, opts);
    let before = p.clone();
    let from = before.find_clips(&ids).iter().map(|r| before.timeline.tracks(r.is_video)[r.track_idx].clips[r.clip_idx].position).fold(f64::INFINITY, f64::min);
    edit::move_clips(p, &ids, to - o.position, 0, opts.lock_envelopes && opts.ripple.is_none()).then_some(())?;
    let moved = p.clip(id).map_or(0.0, |c| c.position - o.position);
    let ops = if moved != 0.0 { vec![RippleOp { from, delta: moved, tracks: tracks_of(&before, &ids), exclude: ids.clone(), keys_from: None }] } else { Vec::new() };
    Some(finish(p, &before, &ids, ops, opts))
}

/// End or Length: the right edge (and its group mates' that line up with it) moves to `end`, as a trim drag.
fn trim_end(p: &mut Project, id: &str, o: &Clip, end: f64, opts: &EditOpts) -> Option<Vec<RippleOp>> {
    let frame = 1.0 / exact_fps(p.settings.frame_rate.max(1.0));
    let d = end - (o.position + o.duration);
    let mut ids = vec![id.to_string()];
    if !opts.ignore_grouping {
        ids.extend(edit::edge_mates(p, id, false, frame));
    }
    let before = p.clone();
    let done = ids.iter().fold(false, |done, k| edit::trim_edge(p, k, true, d) | done);
    done.then_some(())?;
    let ops = edit::trim_ripple(&before, o, p.clip(id).unwrap_or(o), &ids, &tracks_of(&before, &ids));
    Some(finish(p, &before, &ids, ops, opts))
}

/// Auto Ripple and auto crossfades after a move or trim; the ripple left for Post-Edit Ripple.
fn finish(p: &mut Project, before: &Project, ids: &[String], ops: Vec<RippleOp>, opts: &EditOpts) -> Vec<RippleOp> {
    if let Some(m) = opts.ripple {
        edit::ripple_all(p, &ops, m);
    }
    if opts.auto_crossfade {
        edit::auto_crossfade(p, before, ids);
    }
    if opts.ripple.is_some() { Vec::new() } else { ops }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::RippleMode;
    use crate::project::Marker;

    /// Shown in Seconds, so typed times are exact.
    fn new() -> Project {
        let mut p = Project::new("t");
        p.settings.ruler.format = crate::project::RulerFormat::Seconds;
        p
    }

    fn project() -> (Project, String, String) {
        let mut p = new();
        let (a, b) = (Clip::new("a", 0.0, 2.0), Clip::new("b", 2.0, 3.0));
        let ids = (a.id.clone(), b.id.clone());
        p.timeline.video_tracks[0].clips = vec![a, b];
        (p, ids.0, ids.1)
    }

    fn t(p: &Project, secs: f64) -> Input {
        Input::Text(p.settings.ruler_time(secs))
    }

    /// Acceptance: editing Start in the table moves the event, one undo step; with Auto Ripple what followed moves too.
    #[test]
    fn editing_start_moves_the_event_in_one_undo_step() {
        let (mut p, a, b) = project();
        let mut h = History::default();
        let before = p.clone();
        let input = t(&p, 4.0);
        assert!(apply(&mut p, &Item::Event(b.clone()), Col::Start, input, &EditOpts::default()).is_some());
        h.push(before);
        assert_eq!((p.clip(&b).unwrap().position, p.clip(&a).unwrap().position), (4.0, 0.0));
        assert!(p.modified && h.lens() == (1, 0));
        assert!(h.undo(&mut p));
        assert_eq!(p.clip(&b).unwrap().position, 2.0);

        // Auto Ripple: moving the first event 1 s later pushes the next one along.
        let opts = EditOpts { ripple: Some(RippleMode::Affected), ..Default::default() };
        let input = t(&p, 1.0);
        assert_eq!(apply(&mut p, &Item::Event(a.clone()), Col::Start, input, &opts), Some(Vec::new()));
        assert_eq!((p.clip(&a).unwrap().position, p.clip(&b).unwrap().position), (1.0, 3.0));
        // Without it, the ripple is left for Post-Edit Ripple.
        let input = t(&p, 1.5);
        let ops = apply(&mut p, &Item::Event(a.clone()), Col::Start, input, &EditOpts::default()).unwrap();
        assert_eq!((ops.len(), ops[0].delta), (1, 0.5));
    }

    #[test]
    fn locks_groups_tracks_and_lengths_follow_the_timeline() {
        let (mut p, a, b) = project();
        let o = EditOpts::default();
        // A locked event keeps its place; only its Lock switch changes.
        p.timeline.video_tracks[0].clips[0].locked = true;
        let input = t(&p, 5.0);
        assert!(apply(&mut p, &Item::Event(a.clone()), Col::Start, input, &o).is_none());
        assert!(apply(&mut p, &Item::Event(a.clone()), Col::Lock, Input::Toggle, &o).is_some());
        assert!(!p.clip(&a).unwrap().locked);
        // Grouped events move together, unless grouping is ignored.
        assert!(edit::group(&mut p, &[a.clone(), b.clone()]));
        let input = t(&p, 1.0);
        apply(&mut p, &Item::Event(a.clone()), Col::Start, input, &o).unwrap();
        assert_eq!(p.clip(&b).unwrap().position, 3.0);
        let input = t(&p, 0.0);
        apply(&mut p, &Item::Event(a.clone()), Col::Start, input, &EditOpts { ignore_grouping: true, ..o }).unwrap();
        assert_eq!((p.clip(&a).unwrap().position, p.clip(&b).unwrap().position), (0.0, 3.0));
        // Length trims the right edge; Track moves to another track of the same kind only (audio is track 3 here).
        let input = Input::Text(p.settings.ruler_len(1.0));
        apply(&mut p, &Item::Event(b.clone()), Col::Length, input, &EditOpts { ignore_grouping: true, ..o }).unwrap();
        assert_eq!(p.clip(&b).unwrap().duration, 1.0);
        p.timeline.add_video_track();
        assert!(apply(&mut p, &Item::Event(b.clone()), Col::Track, Input::Text("3".into()), &o).is_none(), "not onto an audio track");
        apply(&mut p, &Item::Event(b.clone()), Col::Track, Input::Text("2".into()), &EditOpts { ignore_grouping: true, ..o }).unwrap();
        assert_eq!(p.find_clip(&b).unwrap().track_idx, 1);
        assert_eq!(apply(&mut p, &Item::Event(b.clone()), Col::TakeName, Input::Text("Take 2".into()), &o), Some(Vec::new()));
        assert_eq!(p.clip(&b).unwrap().name, "Take 2");
    }

    #[test]
    fn markers_and_regions_edit() {
        let mut p = new();
        p.timeline.markers.push(Marker { position: 1.0, name: "a".into(), color: [0; 3] });
        p.timeline.regions.push(crate::project::Region { start: 1.0, end: 3.0, name: String::new() });
        let o = EditOpts::default();
        let input = t(&p, 2.0);
        apply(&mut p, &Item::Marker(0), Col::Position, input, &o).unwrap();
        let input = t(&p, 5.0);
        apply(&mut p, &Item::Region(0), Col::Start, input, &o).unwrap();
        assert_eq!((p.timeline.markers[0].position, p.timeline.regions[0].start, p.timeline.regions[0].end), (2.0, 5.0, 7.0));
        let input = t(&p, 4.0);
        assert!(apply(&mut p, &Item::Region(0), Col::End, input, &o).is_none(), "an end before the start");
        apply(&mut p, &Item::Region(0), Col::Name, Input::Text("Intro".into()), &o).unwrap();
        assert!(apply(&mut p, &Item::Region(0), Col::Name, Input::Text("Intro".into()), &o).is_none(), "no change, no undo step");
        assert_eq!(p.timeline.regions[0].name, "Intro");
    }

    /// Lengths are typed as shown: in Measures & Beats they count from 0.0.000 (a position from 1.1.000).
    #[test]
    fn lengths_round_trip_in_every_ruler_format() {
        let mut s = crate::project::ProjectSettings::default();
        for f in crate::project::RulerFormat::ALL {
            s.ruler.format = f;
            let back = s.parse_ruler_len(&s.ruler_len(3.25)).unwrap();
            assert!((back - 3.25).abs() < 0.025, "{f:?}: {back}"); // within half a frame
        }
        s.ruler.format = crate::project::RulerFormat::MeasuresBeats;
        assert_eq!((s.ruler_len(3.25).as_str(), s.parse_ruler_len("2")), ("1.2.384", Some(4.0)));
    }

    /// Acceptance: sort by length.
    #[test]
    fn sort_by_length() {
        let (mut p, a, b) = project();
        p.timeline.video_tracks[0].clips[1].duration = 1.0;
        let mut r = rows(&p, &[], Show::Events);
        let k = Show::Events.cols().iter().position(|c| *c == Col::Length).unwrap();
        r.sort_by(|x, y| x.cells[k].1.cmp(&y.cells[k].1));
        assert_eq!(r.iter().map(|r| r.key.clone()).collect::<Vec<_>>(), [Item::Event(b.clone()), Item::Event(a.clone())]);
        assert_eq!(rows(&p, &[a.clone()], Show::SelectedEvents).len(), 1);
    }
}
