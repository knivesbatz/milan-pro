//! Transport and editing toolbar under the timeline (§5.10), the Rate area left of it, and the transport
//! commands shared with the Video Preview bar and its More menu.
use eframe::egui::*;
use crate::edit;
use crate::history::History;
use crate::project::*;
use crate::render::content_end;
use super::icons::{Icon, paint_icon};
use super::menus::add_marker;
use super::theme::{self, UiLook};
use super::toolbar::{inert_button, inert_split, separator, split_button, tool_button};
use super::ClipRef;

/// What the bar asks the app to do beyond editing the project.
pub enum TransportAction {
    ToggleColorGrading,
}

/// Playback commands: the first ten buttons of the bar, and the preview's More menu (§5.8).
#[derive(Clone, Copy, PartialEq)]
pub enum Cmd {
    Record,
    Loop,
    PlayFromStart,
    Play,
    Pause,
    Stop,
    GoToStart,
    GoToEnd,
    PrevFrame,
    NextFrame,
}

impl Cmd {
    pub const ALL: [Cmd; 10] =
        [Cmd::Record, Cmd::Loop, Cmd::PlayFromStart, Cmd::Play, Cmd::Pause, Cmd::Stop, Cmd::GoToStart, Cmd::GoToEnd, Cmd::PrevFrame, Cmd::NextFrame];

    pub fn icon(self) -> Icon {
        match self {
            Cmd::Record => Icon::Record,
            Cmd::Loop => Icon::Loop,
            Cmd::PlayFromStart => Icon::PlayFromStart,
            Cmd::Play => Icon::Play,
            Cmd::Pause => Icon::Pause,
            Cmd::Stop => Icon::Stop,
            Cmd::GoToStart => Icon::GoToStart,
            Cmd::GoToEnd => Icon::GoToEnd,
            Cmd::PrevFrame => Icon::PrevFrame,
            Cmd::NextFrame => Icon::NextFrame,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Cmd::Record => "Record (Ctrl+R)",
            Cmd::Loop => "Loop Playback (Q)",
            Cmd::PlayFromStart => "Play From Start (Shift+Space)",
            Cmd::Play => "Play (Space)",
            Cmd::Pause => "Pause (Enter)",
            Cmd::Stop => "Stop (Esc)",
            Cmd::GoToStart => "Go to Start (Ctrl+Home)",
            Cmd::GoToEnd => "Go to End (Ctrl+End)",
            Cmd::PrevFrame => "Previous Frame (Alt+Left Arrow)",
            Cmd::NextFrame => "Next Frame (Alt+Right Arrow)",
        }
    }

    /// Recording needs capture devices, which Milan Pro does not drive.
    pub fn enabled(self) -> bool {
        self != Cmd::Record
    }

    /// Active-toggle fill: Play while playing, Loop while loop playback is on.
    pub fn active(self, p: &Playback) -> bool {
        match self {
            Cmd::Play => *p.playing,
            Cmd::Loop => *p.looping,
            _ => false,
        }
    }
}

/// Play state the commands act on (owned by the app).
pub struct Playback<'a> {
    pub playing: &'a mut bool,
    pub looping: &'a mut bool,
    /// Cursor where the current playback began; Stop returns there, as in VEGAS.
    pub play_start: f64,
}

pub fn run(cmd: Cmd, project: &mut Project, p: &mut Playback) {
    let frame = 1.0 / project.settings.frame_rate.max(1.0);
    let end = content_end(project);
    let tl = &mut project.timeline;
    match cmd {
        Cmd::Record => {}
        Cmd::Loop => *p.looping = !*p.looping,
        Cmd::PlayFromStart => {
            tl.cursor_position = 0.0;
            *p.playing = true;
        }
        Cmd::Play => *p.playing = true,
        Cmd::Pause => *p.playing = false,
        Cmd::Stop => {
            if *p.playing {
                *p.playing = false;
                tl.cursor_position = p.play_start;
            }
        }
        Cmd::GoToStart => tl.cursor_position = 0.0,
        Cmd::GoToEnd => tl.cursor_position = end,
        Cmd::PrevFrame => tl.cursor_position = (tl.cursor_position - frame).max(0.0),
        Cmd::NextFrame => tl.cursor_position += frame,
    }
}

/// One transport command as a toolbar button; runs it when clicked.
pub fn cmd_button(ui: &mut Ui, cmd: Cmd, project: &mut Project, p: &mut Playback) {
    if !cmd.enabled() {
        // Record: lit as in VEGAS, nothing to record from.
        inert_button(ui, cmd.icon(), "Record (no capture devices)", false);
    } else if tool_button(ui, cmd.icon(), cmd.label(), cmd.enabled(), cmd.active(p)).clicked() {
        run(cmd, project, p);
    }
}

/// The full command list as menu items (the ≡ / ⋯ More menu).
pub fn cmd_menu(ui: &mut Ui, project: &mut Project, p: &mut Playback) {
    for cmd in Cmd::ALL {
        let text = cmd.label();
        let resp = if cmd == Cmd::Loop {
            ui.add(Button::new(text).selected(*p.looping))
        } else {
            ui.add_enabled(cmd.enabled(), Button::new(text))
        };
        if resp.clicked() {
            run(cmd, project, p);
            ui.close_menu();
        }
    }
}

pub struct TransportState<'a> {
    pub project: &'a mut Project,
    pub history: &'a mut History,
    pub selected_clip: &'a mut Option<ClipRef>,
    pub playback: Playback<'a>,
    pub snapping: &'a mut bool,
    /// Automatic Crossfades: a stored flag only; overlaps do not crossfade yet.
    pub auto_crossfade: &'a mut bool,
}

/// Buttons whose feature does not exist yet are drawn disabled rather than left out, to keep VEGAS's layout.
pub fn draw_transport(ui: &mut Ui, rect: Rect, mut s: TransportState) -> Option<TransportAction> {
    let t = theme::get();
    let v23 = t.look() == UiLook::Vegas23;
    let mut row = ui.new_child(UiBuilder::new().max_rect(rect.shrink2(vec2(1.0, 0.0))).layout(Layout::left_to_right(Align::Center)));
    row.set_clip_rect(rect.intersect(ui.clip_rect()));
    let ui = &mut row;
    ui.spacing_mut().item_spacing.x = 0.0;
    let mut action = None;

    for cmd in Cmd::ALL {
        cmd_button(ui, cmd, s.project, &mut s.playback);
    }
    separator(ui);
    // Normal is the only edit tool; the radio group keeps its place.
    split_button(ui, Icon::NormalEdit, "Normal Edit Tool (Ctrl+D)", true, true);
    inert_button(ui, Icon::EnvelopeEdit, "Envelope Edit Tool (not available yet)", false);
    inert_button(ui, Icon::SelectionEdit, "Selection Edit Tool (not available yet)", false);
    inert_button(ui, Icon::ZoomEdit, "Zoom Edit Tool (not available yet)", false);
    separator(ui);

    let sel = s.selected_clip.clone();
    let tl = &s.project.timeline;
    let clip = sel.as_ref().and_then(|cr| tl.tracks(cr.is_video).get(cr.track_idx)?.clips.get(cr.clip_idx));
    let has_sel = clip.is_some();
    let locked = clip.is_some_and(|c| c.locked);
    let region = tl.loop_region.filter(|(a, b)| b > a);
    let cursor = tl.cursor_position;
    // Each edit is one undo step.
    let edit = |s: &mut TransportState, f: &dyn Fn(&mut Project, &ClipRef) -> bool| {
        if let Some(cr) = &sel {
            let before = s.project.clone();
            if f(s.project, cr) {
                s.history.push(before);
            }
        }
    };
    if tool_button(ui, Icon::Delete, "Delete (Delete)", has_sel && !locked, false).clicked() {
        edit(&mut s, &|p, cr| edit::delete_clip(p, cr));
        *s.selected_clip = None;
    }
    if tool_button(ui, Icon::Trim, "Trim to Time Selection (Ctrl+T)", has_sel && region.is_some(), false).clicked() {
        let (a, b) = region.unwrap_or_default();
        edit(&mut s, &|p, cr| edit::trim_to(p, cr, Some(a), Some(b)));
    }
    if tool_button(ui, Icon::TrimStart, "Trim Start (Alt+[)", has_sel, false).clicked() {
        edit(&mut s, &|p, cr| edit::trim_to(p, cr, Some(cursor), None));
    }
    if tool_button(ui, Icon::TrimEnd, "Trim End (Alt+])", has_sel, false).clicked() {
        edit(&mut s, &|p, cr| edit::trim_to(p, cr, None, Some(cursor)));
    }
    if tool_button(ui, Icon::Split, "Split (S)", has_sel, false).clicked() {
        edit(&mut s, &|p, cr| edit::split_at(p, cr, cursor));
    }
    if v23 {
        tool_button(ui, Icon::SmartSplit, "Smart Split (Alt+S) (not available yet)", false, false);
    }
    if tool_button(ui, Icon::Lock, "Lock", has_sel, locked).clicked() {
        edit(&mut s, &|p, cr| edit::toggle_clip_lock(p, cr));
    }
    separator(ui);
    if tool_button(ui, Icon::InsertMarker, "Insert Marker (M)", true, false).clicked() {
        add_marker(s.project, s.history);
    }
    if tool_button(ui, Icon::InsertRegion, "Insert Region (R)", region.is_some(), false).clicked() {
        add_region(s.project, s.history);
    }
    separator(ui);
    if tool_button(ui, Icon::Snapping, "Enable Snapping (F8)", true, *s.snapping).clicked() {
        *s.snapping = !*s.snapping;
    }
    if tool_button(ui, Icon::AutoCrossfade, "Automatic Crossfades (Ctrl+Shift+X)", true, *s.auto_crossfade).clicked() {
        *s.auto_crossfade = !*s.auto_crossfade;
    }
    inert_split(ui, Icon::AutoRipple, "Auto Ripple (Ctrl+L) (not available yet)");
    // Default on in VEGAS; shown with its active fill although envelopes do not exist yet.
    inert_button(ui, Icon::LockEnvelopes, "Lock Envelopes to Events (not available yet)", true);
    inert_button(ui, Icon::IgnoreGrouping, "Ignore Event Grouping (Ctrl+Shift+U) (not available yet)", false);
    if v23 {
        separator(ui);
        if tool_button(ui, Icon::ColorGrading, "Color Grading (Alt+G)", true, false).clicked() {
            action = Some(TransportAction::ToggleColorGrading);
        }
        separator(ui);
        tool_button(ui, Icon::OpenParent, "Open Parent Timeline (Alt+P) (not available yet)", false, false);
        tool_button(ui, Icon::OpenNested, "Open Nested Timeline (Alt+N) (not available yet)", false, false);
        tool_button(ui, Icon::CreateNested, "Create Nested Timeline (Alt+C) (not available yet)", false, false);
    }
    length_boxes(ui, rect, s.project);
    action
}

/// Region over the time selection, numbered after the markers' scheme.
pub fn add_region(project: &mut Project, history: &mut History) {
    let Some((a, b)) = project.timeline.loop_region.filter(|(a, b)| b > a) else { return };
    history.push(project.clone());
    let n = project.timeline.regions.len() + 1;
    project.timeline.regions.push(Region { start: a, end: b, name: format!("Region {n}") });
    project.mark_modified();
}

/// Cursor-or-selection-start / Selection End / Selection Length boxes at the right end: 14 px icon + timecode.
/// End and Length are blank without a time selection (§5.10).
fn length_boxes(ui: &Ui, rect: Rect, project: &Project) {
    let t = theme::get();
    let p = ui.painter();
    let font = t.font(t.font.transport_time);
    let fps = project.settings.frame_rate;
    let tc = |s: f64| format_timecode(s, fps);
    let tl = &project.timeline;
    let texts = match tl.loop_region.filter(|(a, b)| b > a) {
        Some((a, b)) => [tc(a), tc(b), tc(b - a)],
        None => [tc(tl.cursor_position), String::new(), String::new()],
    };
    let w = 95.0;
    let boxes = [Icon::LengthCursor, Icon::LengthSelEnd, Icon::LengthLength];
    for (k, (icon, text)) in boxes.into_iter().zip(texts).enumerate() {
        let x = rect.max.x - 8.0 - w * (3 - k) as f32;
        let r = Rect::from_min_size(pos2(x, rect.center().y - 7.0), vec2(14.0, 14.0));
        p.rect_filled(Rect::from_min_max(pos2(x - 2.0, rect.min.y), pos2(x + w - 4.0, rect.max.y)), 0.0, t.toolbar_bg);
        paint_icon(p, r, icon, t.look(), None);
        p.text(pos2(r.max.x + 6.0, rect.center().y), Align2::LEFT_CENTER, text, font.clone(), t.text);
    }
}

/// Rate area in the track-list column (§5.9.3): "Rate: 0.00" at x=6 and a −4…+4 shuttle slider from x=90 to
/// x=240 with a white ▲ under 1.0. Dragging the thumb shuttles the cursor at that rate; it springs back to 0.
pub fn draw_rate(ui: &Ui, rect: Rect, playing: bool, project: &mut Project) {
    let t = theme::get();
    let p = ui.painter();
    p.rect_filled(rect, 0.0, if t.look() == UiLook::Vegas16 { t.band_bg } else { t.toolbar_bg });
    if t.look() == UiLook::Vegas16 {
        let x = rect.x_range();
        let line = |y: f32, c| p.rect_filled(Rect::from_x_y_ranges(x, Rangef::new(y, y + 1.0)), 0.0, c);
        line(rect.min.y, t.tab_separator);
        line(rect.min.y + 1.0, t.bevel.band_2);
        line(rect.max.y - 1.0, t.bevel.band_1);
    }
    let cy = rect.center().y.round();
    let (x0, x1) = (rect.min.x + 90.0, (rect.min.x + 240.0).min(rect.max.x - 8.0));
    let mut rate = 0.0_f32;
    if x1 - x0 >= 30.0 {
        let to_x = |r: f32| (x0 + (r + 4.0) / 8.0 * (x1 - x0)).round();
        let hit = Rect::from_x_y_ranges(Rangef::new(x0 - 8.0, x1 + 8.0), Rangef::new(cy - 9.0, cy + 9.0));
        let resp = ui.interact(hit, ui.id().with("rate_slider"), Sense::drag()).on_hover_text("Rate");
        if let Some(pos) = resp.interact_pointer_pos().filter(|_| resp.dragged()) {
            rate = ((pos.x - x0) / (x1 - x0) * 8.0 - 4.0).clamp(-4.0, 4.0);
            if !playing && rate.abs() > 0.01 {
                let tl = &mut project.timeline;
                let dt = ui.input(|i| i.stable_dt).min(0.1) as f64;
                tl.cursor_position = (tl.cursor_position + rate as f64 * dt).max(0.0);
                ui.ctx().request_repaint();
            }
        }
        p.rect_filled(Rect::from_min_max(pos2(x0, cy), pos2(x1, cy + 1.0)), 0.0, t.fader_track);
        let normal = to_x(1.0);
        p.add(Shape::convex_polygon(vec![pos2(normal, cy + 6.0), pos2(normal + 4.0, cy + 10.0), pos2(normal - 4.0, cy + 10.0)], t.text, Stroke::NONE));
        let thumb = Rect::from_center_size(pos2(to_x(rate), cy), vec2(16.0, 16.0));
        paint_icon(p, thumb, Icon::FaderThumb, t.look(), None);
    }
    let shown = if rate != 0.0 { rate } else if playing { 1.0 } else { 0.0 };
    p.text(pos2(rect.min.x + 6.0, cy), Align2::LEFT_CENTER, format!("Rate: {shown:.2}"), t.font(t.font.ui), t.text);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_move_cursor_and_play_state() {
        let mut p = Project::new("t");
        p.timeline.video_tracks[0].clips.push(Clip::new("c", 0.0, 10.0));
        let (mut playing, mut looping) = (false, false);
        let mut pb = Playback { playing: &mut playing, looping: &mut looping, play_start: 2.0 };
        run(Cmd::GoToEnd, &mut p, &mut pb);
        assert_eq!(p.timeline.cursor_position, 10.0);
        run(Cmd::PrevFrame, &mut p, &mut pb);
        assert!(p.timeline.cursor_position < 10.0);
        run(Cmd::Play, &mut p, &mut pb);
        run(Cmd::Stop, &mut p, &mut pb);
        assert_eq!(p.timeline.cursor_position, 2.0); // Stop returns to where playback began
        run(Cmd::Loop, &mut p, &mut pb);
        run(Cmd::PlayFromStart, &mut p, &mut pb);
        assert!(*pb.playing && *pb.looping && p.timeline.cursor_position == 0.0);
    }
}
