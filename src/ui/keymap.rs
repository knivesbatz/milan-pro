//! Commands and the default keymap (§7). One table holds every default binding for both looks; the menus read
//! their shortcut text from it and the app turns key presses into `Command`s through it.
use eframe::egui::{Context, Event, Key};
use super::dock::Panel;
use super::theme::UiLook;
use crate::decode::Quality;

/// Everything a menu item, context menu or shortcut can ask the app to do. Commands Milan Pro cannot carry out
/// yet still exist so their keys and menu slots are reserved; `CmdState::enabled` greys them out.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Command {
    // File
    New, NewEmpty, Open, Close, Save, SaveAs, SaveVersion, RenderAs, ImportMedia, Properties, Welcome, Exit,
    OpenRecent(usize),
    // Edit
    Undo, Redo, Cut, Copy, Paste, PasteRepeat, PasteInsert, Delete, Trim, TrimStart, TrimEnd, Split, SmartSplit,
    RenameEvent, SelectAll, SelectNone, NormalTool, NextTool, PrevTool, Group, RemoveFromGroup, ClearGroup,
    SelectGroup, IgnoreGrouping, NextTake, PrevTake, RippleAffected, RippleBus, RippleAll, AutoRipple, AutoCrossfade,
    NestedCreate, NestedOpen, NestedParent, RenderToNewTrack, OpenInAudioEditor, Multicam, UndoAll, ClearHistory,
    EventLock,
    // Playback
    PlayStop, PlayPause, PlayFromStart, PlayAnyWindow, Stop, Loop, Record, Arm, ShuttleReverse, ShuttlePause,
    ShuttleForward, PreviewAtCursor, DimMaster, Prerender, RamPreview, ExternalMonitor,
    // Cursor
    GoToStart, GoToEnd, SelOrViewStart, SelOrViewEnd, PrevFrame, NextFrame, PixelLeft, PixelRight, GridLeft,
    GridRight, PrevMarker, NextMarker, SelectToNextMarker, PrevEditPoint, NextEditPoint, SelEventStart, SelEventEnd,
    GoTo, Marker(usize), CenterView, CursorToPlayhead, JogLeft, JogRight,
    // Selection
    SetSelStart, SetSelEnd, SelectLoopRegion, RestoreSelection, SelPrevEvent, SelNextEvent, SelectionToView,
    SelectionToProject, SelectEventStart, SelectEventEnd, ExpandedEdit,
    // Event nudge
    NudgeFrameLeft, NudgeFrameRight, NudgePixelLeft, NudgePixelRight, NudgeTrackUp, NudgeTrackDown, PitchUp,
    PitchDown, PitchCentUp, PitchCentDown, PitchOctUp, PitchOctDown,
    // Markers
    InsertMarker, InsertRegion, InsertCommand, CdTrack, CdIndex,
    // Tracks
    InsertAudioTrack, InsertVideoTrack, InsertAdjustmentTrack, MuteTrack, MuteExclusive, SoloTrack, SoloExclusive,
    GroupMute, GroupSolo, FocusPrevTrack, FocusNextTrack, ToggleListFocus, VolumeEnvelope, PanEnvelope,
    RemoveVolumeEnvelope, RemovePanEnvelope, NextFxEnvelope, PrevFxEnvelope, ShowEnvelopes,
    // Snapping
    Snapping, SnapGrid, SnapMarkers, SnapAllEvents, Quantize,
    // Zoom
    ZoomIn, ZoomOut, ZoomSelection, ZoomOutLarge, ZoomFramePerThumb, WaveZoomIn, WaveZoomOut, TracksTaller,
    TracksShorter, MinimizeTracks, DefaultTrackHeights,
    // View
    BusTracks, VideoBusTrack, MediaMarkers, TakeInfo, Waveforms, EventButtons, FadeLengths, EdgeFrames, EventHeaders,
    NextWindow, PrevWindow, PreviewQuality(Quality),
    // Docking and layouts
    ToggleDocks, TopDock, BottomDock, RightDock, CollapseTrackList, MaximizeTimeline, LayoutDefault, LayoutVp14,
    LayoutAudio, LayoutColor, LayoutSource, LayoutTimelineTop, LayoutUser(usize), SaveLayoutUser(usize),
    // Windows
    FocusTimeline, Window(Panel), EditDetails, ProjectNotes, PluginManager, VideoScopes, SurroundPanner, MixingConsole,
    DeviceExplorer, LoudnessMeters, Dashboard, MotionTracking, TextToSpeech,
    // Misc
    Help, WhatsThis, ContextMenu, RebuildPeaks, MidiTimecode, MidiClock, MidiTrigger, Preferences,
    KeyboardShortcuts, About,
    // Context-menu and preference commands without default keys
    EventOpenInTrimmer, EventSelectInMedia, EventProperties, SpacePlayPause,
}

use Command::*;

/// One row of §7: command, name (Help › Keyboard Shortcuts), VP23 keys, VP16 keys when they differ (`Some(&[])` =
/// none). A key is "Ctrl+Shift+Z"; a chord is two strokes separated by a space ("Alt+D D"); "Num 5" is a numpad
/// key, which egui cannot tell from the digit row, so it is listed but never matched.
type Row = (Command, &'static str, &'static [&'static str], Option<&'static [&'static str]>);

pub const TABLE: &[Row] = &[
    // File
    (New, "New", &["Ctrl+N"], None),
    (NewEmpty, "New, skipping Properties", &["Ctrl+Shift+N"], None),
    (Open, "Open", &["Ctrl+O"], None),
    (Close, "Close", &["Ctrl+F4"], None),
    (Save, "Save", &["Ctrl+S"], None),
    (SaveAs, "Save As", &["Ctrl+Shift+S"], Some(&[])),
    (SaveVersion, "Save Version", &["Ctrl+Alt+S"], Some(&[])),
    (Properties, "Properties", &["Alt+Enter"], None),
    (Exit, "Exit", &["Alt+F4"], None),
    // Edit
    (Undo, "Undo", &["Ctrl+Z", "Alt+Backspace"], None),
    (Redo, "Redo", &["Ctrl+Shift+Z", "Ctrl+Y"], None),
    (Cut, "Cut", &["Ctrl+X", "Shift+Delete"], None),
    (Copy, "Copy", &["Ctrl+C", "Ctrl+Insert"], None),
    (Paste, "Paste", &["Ctrl+V", "Shift+Insert"], None),
    (PasteInsert, "Paste Insert", &["Ctrl+Shift+V"], None),
    (PasteRepeat, "Paste Repeat", &["Ctrl+B"], None),
    (Delete, "Delete", &["Delete"], None),
    (Trim, "Trim", &["Ctrl+T"], None),
    (TrimStart, "Trim Start", &["Alt+["], None),
    (TrimEnd, "Trim End", &["Alt+]"], None),
    (Split, "Split", &["S"], None),
    (SmartSplit, "Smart Split", &["Alt+S"], Some(&[])),
    (RenameEvent, "Rename event", &["F2"], Some(&[])),
    (SelectAll, "Select All", &["Ctrl+A"], None),
    (SelectNone, "Select None", &["Ctrl+Shift+A"], None),
    (NormalTool, "Normal Edit Tool", &["Ctrl+D"], None),
    (NextTool, "Next Tool", &["D"], None),
    (PrevTool, "Previous Tool", &["Shift+D"], None),
    (Group, "Group: Create New", &["G"], None),
    (RemoveFromGroup, "Group: Remove From", &["U"], None),
    (ClearGroup, "Group: Clear", &["Ctrl+U"], None),
    (SelectGroup, "Group: Select All", &["Shift+G"], None),
    (IgnoreGrouping, "Ignore Event Grouping", &["Ctrl+Shift+U"], None),
    (NextTake, "Next Take", &["T"], None),
    (PrevTake, "Previous Take", &["Shift+T"], None),
    (RippleAffected, "Post-Edit Ripple: Affected Tracks", &["F"], None),
    (RippleBus, "Post-Edit Ripple: Tracks, Bus Tracks, Markers, Regions", &["Ctrl+F"], None),
    (RippleAll, "Post-Edit Ripple: All Tracks, Markers, Regions", &["Ctrl+Shift+F"], None),
    (AutoRipple, "Auto Ripple", &["Ctrl+L"], None),
    (AutoCrossfade, "Automatic Crossfades", &["Ctrl+Shift+X"], None),
    (NestedCreate, "Create Nested Timeline", &["Alt+C"], Some(&[])),
    (NestedOpen, "Open Nested Timeline", &["Alt+N"], Some(&[])),
    (NestedParent, "Open Parent Timeline", &["Alt+P"], Some(&[])),
    (RenderToNewTrack, "Render to New Track", &["Ctrl+M"], None),
    (OpenInAudioEditor, "Open in Audio Editor", &["Ctrl+E"], None),
    (Multicam, "Enable Multicamera Editing", &["Ctrl+Shift+D"], None),
    // Playback
    (PlayStop, "Play/Stop (back to the start)", &[], Some(&["Space", "F12"])),
    (PlayPause, "Play/Pause", &["Space", "Enter", "Ctrl+F12"], Some(&["Enter", "Ctrl+F12"])),
    (PlayFromStart, "Play from Start", &["Shift+Space", "Shift+F12"], None),
    (PlayAnyWindow, "Play from any window", &["Ctrl+Space", "F12"], Some(&["Ctrl+Space"])),
    (Stop, "Stop", &["Escape"], None),
    (Loop, "Loop Playback", &["Q", "Ctrl+Shift+L"], None),
    (Record, "Record", &["Ctrl+R"], None),
    (Arm, "Arm track for record", &["Ctrl+Alt+R"], None),
    (ShuttleReverse, "Shuttle reverse (repeat = faster)", &["J"], None),
    (ShuttlePause, "Shuttle pause", &["K"], None),
    (ShuttleForward, "Shuttle forward (repeat = faster)", &["L"], None),
    (PreviewAtCursor, "Preview at cursor", &["Num 0"], None),
    (DimMaster, "Dim master", &["Ctrl+Shift+F12"], None),
    (Prerender, "Selectively Prerender Video", &["Shift+M"], None),
    (RamPreview, "Build Dynamic RAM Preview", &["Shift+B"], None),
    (ExternalMonitor, "Preview on External Monitor", &["Alt+Shift+4"], None),
    // Cursor
    (GoToStart, "Go to Project Start", &["Ctrl+Home", "W"], None),
    (GoToEnd, "Go to Project End", &["Ctrl+End"], None),
    (SelOrViewStart, "Go to Time Selection/View Start", &["Home"], None),
    (SelOrViewEnd, "Go to Time Selection/View End", &["End"], None),
    (PrevFrame, "Move Left by Frame", &["Alt+Left"], None),
    (NextFrame, "Move Right by Frame", &["Alt+Right"], None),
    (PixelLeft, "Move Left by Pixel", &["Left"], None),
    (PixelRight, "Move Right by Pixel", &["Right"], None),
    (GridLeft, "Move Left by Grid Marks", &["PageUp"], None),
    (GridRight, "Move Right by Grid Marks", &["PageDown"], None),
    (PrevMarker, "Move Left by Marker/Region", &["Ctrl+Left"], None),
    (NextMarker, "Move Right by Marker/Region", &["Ctrl+Right"], None),
    (SelectToNextMarker, "Time Select Right by Marker/Region", &["Ctrl+Shift+Right"], None),
    (PrevEditPoint, "Move Left by Event Edit Points", &["Ctrl+Alt+Left"], None),
    (NextEditPoint, "Move Right by Event Edit Points", &["Ctrl+Alt+Right"], None),
    (SelEventStart, "Move Left by Selected Event", &["Ctrl+["], None),
    (SelEventEnd, "Move Right by Selected Event", &["Ctrl+]"], None),
    (GoTo, "Go to Time", &["Ctrl+G"], None),
    (Marker(1), "Go to marker 1", &["1"], None),
    (Marker(2), "Go to marker 2", &["2"], None),
    (Marker(3), "Go to marker 3", &["3"], None),
    (Marker(4), "Go to marker 4", &["4"], None),
    (Marker(5), "Go to marker 5", &["5"], None),
    (Marker(6), "Go to marker 6", &["6"], None),
    (Marker(7), "Go to marker 7", &["7"], None),
    (Marker(8), "Go to marker 8", &["8"], None),
    (Marker(9), "Go to marker 9", &["9"], None),
    (Marker(10), "Go to marker 10", &["0"], None),
    (CenterView, "Center View around Cursor", &["\\"], None),
    (CursorToPlayhead, "Move Cursor to Playhead", &["Alt+Down"], None),
    (JogLeft, "Jog left", &["F3"], None),
    (JogRight, "Jog right", &["F9"], None),
    // Selection
    (SetSelStart, "Set Selection Start", &["I"], None),
    (SetSelEnd, "Set Selection End", &["O"], None),
    (SelectLoopRegion, "Select Loop Region", &["Shift+Q"], None),
    (RestoreSelection, "Restore Previous Time Selection", &["Backspace"], None),
    (SelPrevEvent, "Set Selection to Previous Selected Event", &["Ctrl+Shift+["], None),
    (SelNextEvent, "Set Selection to Next Selected Event", &["Ctrl+Shift+]"], None),
    (SelectEventStart, "Select Event Start", &["["], None),
    (SelectEventEnd, "Select Event End", &["]"], None),
    (ExpandedEdit, "Enter/Exit Expanded Edit Mode", &["Num 5"], None),
    // Event nudge
    (NudgeFrameLeft, "Move selected events left 1 frame", &["Num 1"], None),
    (NudgeFrameRight, "Move selected events right 1 frame", &["Num 3"], None),
    (NudgePixelLeft, "Move selected events left 1 pixel", &["Num 4"], None),
    (NudgePixelRight, "Move selected events right 1 pixel", &["Num 6"], None),
    (NudgeTrackUp, "Move selected events up a track", &["Num 8"], None),
    (NudgeTrackDown, "Move selected events down a track", &["Num 2"], None),
    (PitchUp, "Pitch up a semitone", &["="], None),
    (PitchDown, "Pitch down a semitone", &["-"], None),
    (PitchCentUp, "Pitch up a cent", &["Ctrl+="], None),
    (PitchCentDown, "Pitch down a cent", &["Ctrl+-"], None),
    (PitchOctUp, "Pitch up an octave", &["Shift+="], None),
    (PitchOctDown, "Pitch down an octave", &["Shift+-"], None),
    // Markers
    (InsertMarker, "Insert Marker", &["M"], None),
    (InsertRegion, "Insert Region", &["R"], None),
    (InsertCommand, "Insert Command", &["C"], None),
    (CdTrack, "Insert Audio CD Track Region", &["N"], None),
    (CdIndex, "Insert Audio CD Track Index", &["Shift+N"], None),
    // Tracks
    (InsertAudioTrack, "Insert Audio Track", &["Ctrl+Q"], None),
    (InsertVideoTrack, "Insert Video Track", &["Ctrl+Shift+Q"], None),
    (InsertAdjustmentTrack, "Insert Adjustment Track", &["Ctrl+Shift+Alt+Q"], Some(&[])),
    (MuteTrack, "Mute track", &["Z"], None),
    (MuteExclusive, "Exclusive mute", &["Shift+Z"], None),
    (SoloTrack, "Solo track", &["X"], None),
    (SoloExclusive, "Exclusive solo", &["Shift+X"], None),
    (GroupMute, "Mute track group", &["Ctrl+Alt+Z"], None),
    (GroupSolo, "Solo track group", &["Ctrl+Alt+X"], None),
    (FocusPrevTrack, "Focus previous track", &["Alt+Shift+Up"], None),
    (FocusNextTrack, "Focus next track", &["Alt+Shift+Down"], None),
    (ToggleListFocus, "Track list / timeline focus", &["Tab"], None),
    (VolumeEnvelope, "Volume envelope", &["V"], None),
    (PanEnvelope, "Pan envelope", &["P"], None),
    (RemoveVolumeEnvelope, "Remove volume envelope", &["Shift+V"], None),
    (RemovePanEnvelope, "Remove pan envelope", &["Shift+P"], None),
    (NextFxEnvelope, "Next FX envelope", &["E"], None),
    (PrevFxEnvelope, "Previous FX envelope", &["Shift+E"], None),
    (ShowEnvelopes, "Show Envelopes", &["Ctrl+Shift+E"], None),
    // Snapping
    (Snapping, "Enable Snapping", &["F8"], None),
    (SnapGrid, "Snap to Grid", &["Ctrl+F8"], None),
    (SnapMarkers, "Snap to Markers", &["Shift+F8"], None),
    (SnapAllEvents, "Snap to All Events", &["Ctrl+Shift+F8"], None),
    (Quantize, "Quantize to Frames", &["Alt+F8"], None),
    // Zoom
    (ZoomIn, "Zoom in horizontally", &["Up"], None),
    (ZoomOut, "Zoom out horizontally", &["Down"], None),
    (ZoomSelection, "Zoom Selection / zoom in large", &["Ctrl+Up"], None),
    (ZoomOutLarge, "Zoom out large", &["Ctrl+Down"], None),
    (ZoomFramePerThumb, "One frame per thumbnail", &["Alt+Up"], None),
    (WaveZoomIn, "Waveform zoom in", &["Shift+Up"], None),
    (WaveZoomOut, "Waveform zoom out", &["Shift+Down"], None),
    (TracksTaller, "All tracks taller", &["Ctrl+Shift+Up"], None),
    (TracksShorter, "All tracks shorter", &["Ctrl+Shift+Down"], None),
    (MinimizeTracks, "Minimize/restore all tracks", &["`"], None),
    (DefaultTrackHeights, "Default track heights", &["Ctrl+`"], None),
    // View
    (BusTracks, "Audio Bus Tracks", &["B"], None),
    (VideoBusTrack, "Video Bus Track", &["Ctrl+Shift+B"], None),
    (MediaMarkers, "Show Event Media Markers", &["Ctrl+Shift+K"], None),
    (TakeInfo, "Active Take Information", &["Ctrl+Shift+I"], None),
    (Waveforms, "Audio Event Waveforms", &["Ctrl+Shift+W"], None),
    (EventButtons, "Event Buttons", &["Ctrl+Shift+C"], None),
    (FadeLengths, "Event Fade Lengths", &["Ctrl+Shift+T"], None),
    (EdgeFrames, "Video Event Edge Edit Frames", &["Ctrl+Shift+O"], None),
    (EventHeaders, "Event Headers", &["Ctrl+Alt+M"], Some(&[])),
    (NextWindow, "Next window", &["F6", "Ctrl+Tab"], None),
    (PrevWindow, "Previous window", &["Shift+F6", "Ctrl+Shift+Tab"], None),
    // Docking
    (ToggleDocks, "Minimize/restore all docks", &["F11", "Alt+`"], Some(&["F11"])),
    (TopDock, "Top dock", &["Alt+F11"], Some(&[])),
    (BottomDock, "Bottom dock", &["Ctrl+Alt+F11"], Some(&[])),
    (RightDock, "Right dock", &["Ctrl+Shift+F11"], Some(&[])),
    (CollapseTrackList, "Collapse track headers", &["Ctrl+F11"], Some(&["Shift+F11"])),
    (MaximizeTimeline, "Maximize timeline", &["Ctrl+Alt+Shift+F11"], Some(&["Ctrl+F11"])),
    (LayoutDefault, "Default Layout", &["Alt+D D"], None),
    (LayoutVp14, "Classic Layout", &[], Some(&["Alt+D V"])),
    (LayoutAudio, "Audio Mixing Layout", &["Alt+D A"], None),
    (LayoutColor, "Color Correction Layout", &["Alt+D C"], None),
    (LayoutSource, "Source Window Layout", &["Alt+D S"], Some(&[])),
    (LayoutTimelineTop, "Timeline Top Layout", &["Alt+D T"], Some(&[])),
    (LayoutUser(0), "User layout 0", &["Alt+D 0"], None),
    (LayoutUser(1), "User layout 1", &["Alt+D 1"], None),
    (SaveLayoutUser(0), "Save user layout 0", &["Ctrl+Alt+D 0"], None),
    (SaveLayoutUser(1), "Save user layout 1", &["Ctrl+Alt+D 1"], None),
    // Windows
    (FocusTimeline, "Focus to Timeline", &["Alt+0"], None),
    (Window(Panel::Explorer), "Explorer", &["Alt+1"], None),
    (Window(Panel::Trimmer), "Trimmer", &["Alt+2"], None),
    (Window(Panel::MasterBus), "Master Bus", &["Alt+3"], None),
    (Window(Panel::VideoPreview), "Video Preview", &["Alt+4"], None),
    (Window(Panel::ProjectMedia), "Project Media", &["Alt+5"], None),
    (ProjectNotes, "Project Notes", &["Alt+6"], Some(&[])),
    (EditDetails, "Edit Details", &["Ctrl+Alt+4"], Some(&["Alt+6"])),
    (Window(Panel::Transitions), "Transitions", &["Alt+7"], None),
    (Window(Panel::VideoFx), "Video FX", &["Alt+8"], None),
    (Window(Panel::MediaGenerators), "Media Generators", &["Alt+9"], None),
    (PluginManager, "Plug-In Manager", &["Ctrl+Alt+1"], None),
    (VideoScopes, "Video Scopes", &["Ctrl+Alt+2"], None),
    (SurroundPanner, "Surround Panner", &["Ctrl+Alt+3"], None),
    (MixingConsole, "Mixing Console", &["Ctrl+Alt+5"], Some(&["Ctrl+Alt+6"])),
    (DeviceExplorer, "Device Explorer", &["Ctrl+Alt+6"], Some(&["Ctrl+Alt+7"])),
    (LoudnessMeters, "Loudness Meters", &["Ctrl+Alt+7"], Some(&["Ctrl+Alt+8"])),
    (Dashboard, "Dashboard", &["Ctrl+Alt+8"], Some(&[])),
    (Window(Panel::ColorGrading), "Color Grading", &["Alt+G"], Some(&[])),
    (MotionTracking, "Motion Tracking", &["Alt+M"], Some(&[])),
    (TextToSpeech, "Text to Speech", &["Ctrl+Shift+P"], Some(&[])),
    // Misc
    (Help, "Contents and Index", &["F1"], None),
    (WhatsThis, "What's This?", &["Shift+F1"], None),
    (ContextMenu, "Context menu", &["Shift+F10"], None),
    (RebuildPeaks, "Rebuild Audio Peaks", &["Shift+F5"], None),
    (MidiTimecode, "Generate MIDI Timecode", &["F7"], None),
    (MidiClock, "Generate MIDI Clock", &["Shift+F7"], None),
    (MidiTrigger, "Trigger from MIDI Timecode", &["Ctrl+F7"], None),
];

/// Keys of `cmd` in `look`, as written in the table.
pub fn keys(cmd: Command, look: UiLook) -> &'static [&'static str] {
    TABLE
        .iter()
        .find(|r| r.0 == cmd)
        .map_or(&[], |r| if look == UiLook::Vegas16 { r.3.unwrap_or(r.2) } else { r.2 })
}

/// Shortcut text shown right-aligned in menus: the first binding, chords as "Alt+D, D".
pub fn shortcut_text(cmd: Command, look: UiLook) -> String {
    keys(cmd, look).first().map(|k| fmt_key(k)).unwrap_or_default()
}

/// A table key for display: chords as "Alt+D, D".
pub fn fmt_key(k: &str) -> String {
    if k.starts_with("Num ") { k.to_string() } else { k.replace(' ', ", ") }
}

#[derive(Clone, Copy, PartialEq, Debug)]
struct Stroke {
    ctrl: bool,
    alt: bool,
    shift: bool,
    key: Key,
}

/// None for a numpad key ("Num 5") or a typo; a test checks every table entry parses.
fn parse_stroke(s: &str) -> Option<Stroke> {
    let (mut ctrl, mut alt, mut shift) = (false, false, false);
    let mut parts: Vec<&str> = s.split('+').collect();
    // "Ctrl+=" splits cleanly, but a trailing "+" key would not; the table has none.
    let name = parts.pop()?;
    for m in parts {
        match m {
            "Ctrl" => ctrl = true,
            "Alt" => alt = true,
            "Shift" => shift = true,
            _ => return None,
        }
    }
    Some(Stroke { ctrl, alt, shift, key: Key::from_name(name)? })
}

/// (first stroke of a chord, stroke) for one table key string.
fn parse(k: &str) -> Option<(Option<Stroke>, Stroke)> {
    if k.starts_with("Num ") {
        return None;
    }
    match k.split_once(' ') {
        Some((a, b)) => Some((Some(parse_stroke(a)?), parse_stroke(b)?)),
        None => Some((None, parse_stroke(k)?)),
    }
}

/// Commands that keep firing while their key auto-repeats (cursor moves, zoom, shuttle speed).
fn repeats(cmd: Command) -> bool {
    matches!(
        cmd,
        PrevFrame | NextFrame | PixelLeft | PixelRight | PrevMarker | NextMarker | PrevEditPoint | NextEditPoint
            | ZoomIn | ZoomOut | ZoomSelection | ZoomOutLarge | TracksTaller | TracksShorter | Undo | Redo
            | ShuttleForward | ShuttleReverse
    )
}

/// Chord state between frames (§7: "Alt+D, then X" with a 1.5 s timeout).
#[derive(Default)]
pub struct Keymap {
    chord: Option<(Stroke, f64)>,
}

/// Where keys go this frame.
pub struct Scope {
    /// A text field has focus or a dialog/menu is open: nothing fires.
    pub blocked: bool,
    /// The pointer is over a dock window that handles its own plain keys (Trimmer I/O/A, Project Media Delete),
    /// so plain I, O, A, Delete and digits are left to it.
    pub over_dock: bool,
}

impl Keymap {
    /// Turn this frame's key presses into commands for `look`. Matched presses are removed from the input so
    /// egui widgets do not also act on them.
    pub fn poll(&mut self, ctx: &Context, look: UiLook, scope: Scope) -> Vec<Command> {
        let mut out = Vec::new();
        if scope.blocked {
            self.chord = None;
            return out;
        }
        let now = ctx.input(|i| i.time);
        if self.chord.is_some_and(|(_, t)| now - t > 1.5) {
            self.chord = None;
        }
        ctx.input_mut(|i| {
            i.events.retain(|e| {
                let (key, physical, repeat, m) = match e {
                    Event::Key { key, physical_key, pressed: true, repeat, modifiers } => (*key, *physical_key, *repeat, *modifiers),
                    Event::Cut | Event::Copy | Event::Paste(_) => {
                        out.push(match e {
                            Event::Cut => Cut,
                            Event::Copy => Copy,
                            _ => Paste,
                        });
                        return false;
                    }
                    _ => return true,
                };
                // Physical key: layout-independent and unaffected by Shift ("Ctrl+Shift+[" arrives as "{").
                let stroke = Stroke { ctrl: m.ctrl || m.command, alt: m.alt, shift: m.shift, key: physical.unwrap_or(key) };
                let digit = stroke.key.name().len() == 1 && stroke.key.name().as_bytes()[0].is_ascii_digit();
                let plain_local =
                    !stroke.ctrl && !stroke.alt && !stroke.shift && (digit || matches!(stroke.key, Key::I | Key::O | Key::A | Key::Delete));
                if scope.over_dock && plain_local {
                    return true;
                }
                let first = self.chord.take().map(|(s, _)| s);
                for row in TABLE {
                    let ks = if look == UiLook::Vegas16 { row.3.unwrap_or(row.2) } else { row.2 };
                    for k in ks {
                        let Some((pre, s)) = parse(k) else { continue };
                        if pre == first && s == stroke {
                            if !repeat || repeats(row.0) {
                                out.push(row.0);
                            }
                            return false;
                        }
                        if first.is_none() && pre == Some(stroke) && !repeat {
                            self.chord = Some((stroke, now));
                            return false;
                        }
                    }
                }
                true
            });
        });
        out
    }
}

/// egui's own Ctrl+Plus/Minus UI zoom would steal the pitch keys; VEGAS has no UI zoom.
pub fn install(ctx: &Context) {
    ctx.options_mut(|o| o.zoom_with_keyboard = false);
}

/// Mnemonic helpers for "&File"-style labels: the text without '&' and the index of the underlined char.
pub fn mnemonic(label: &str) -> (String, Option<usize>) {
    let mut text = String::with_capacity(label.len());
    let mut at = None;
    let mut chars = label.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '&' {
            match chars.peek() {
                Some('&') => {
                    text.push('&');
                    chars.next();
                }
                Some(_) if at.is_none() => at = Some(text.len()),
                _ => {}
            }
        } else {
            text.push(c);
        }
    }
    (text, at)
}

/// The mnemonic letter as a key (for Alt+letter and letter-in-open-menu).
pub fn mnemonic_key(label: &str) -> Option<Key> {
    let (text, at) = mnemonic(label);
    let c = text[at?..].chars().next()?;
    Key::from_name(&c.to_ascii_uppercase().to_string())
}

pub fn alt_held(ctx: &Context) -> bool {
    ctx.input(|i| i.modifiers.alt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{self, Modifiers};

    #[test]
    fn table_parses_and_has_no_duplicate_keys() {
        for look in [UiLook::Vegas16, UiLook::Vegas23] {
            let mut seen: Vec<(Option<Stroke>, Stroke, Command)> = Vec::new();
            for row in TABLE {
                for k in keys(row.0, look) {
                    if k.starts_with("Num ") {
                        continue;
                    }
                    let p = parse(k).unwrap_or_else(|| panic!("bad key {k:?}"));
                    if let Some(d) = seen.iter().find(|s| s.0 == p.0 && s.1 == p.1) {
                        panic!("{k} bound to both {:?} and {:?} in {look:?}", d.2, row.0);
                    }
                    seen.push((p.0, p.1, row.0));
                }
            }
        }
        assert_eq!(shortcut_text(PlayStop, UiLook::Vegas16), "Space");
        assert_eq!(shortcut_text(PlayPause, UiLook::Vegas23), "Space");
        assert_eq!(shortcut_text(SaveAs, UiLook::Vegas16), "");
        assert_eq!(shortcut_text(LayoutDefault, UiLook::Vegas23), "Alt+D, D");
        assert_eq!(mnemonic("Time && Fra&mes"), ("Time & Frames".to_string(), Some(10)));
        assert_eq!(mnemonic_key("E&xit"), Some(Key::X));
    }

    #[test]
    fn keys_and_chords_resolve() {
        let ctx = Context::default();
        let mut km = Keymap::default();
        // Press and release, so egui does not report the next press as a repeat.
        let key = |k: Key, m: Modifiers| -> Vec<Event> {
            [true, false].map(|pressed| Event::Key { key: k, physical_key: Some(k), pressed, repeat: false, modifiers: m }).to_vec()
        };
        let mut run = |events: Vec<Event>, look| {
            let mut out = Vec::new();
            ctx.run(egui::RawInput { events, ..Default::default() }, |ctx| {
                out = km.poll(ctx, look, Scope { blocked: false, over_dock: false });
            });
            out
        };
        assert_eq!(run(key(Key::Space, Modifiers::NONE), UiLook::Vegas16), vec![PlayStop]);
        assert_eq!(run(key(Key::Space, Modifiers::NONE), UiLook::Vegas23), vec![PlayPause]);
        assert_eq!(run(key(Key::Z, Modifiers::COMMAND | Modifiers::SHIFT), UiLook::Vegas23), vec![Redo]);
        assert_eq!(run(key(Key::D, Modifiers::ALT), UiLook::Vegas23), vec![]);
        assert_eq!(run(key(Key::D, Modifiers::NONE), UiLook::Vegas23), vec![LayoutDefault]);
        assert_eq!(run(key(Key::D, Modifiers::NONE), UiLook::Vegas23), vec![NextTool]);
        assert_eq!(run(key(Key::Num3, Modifiers::NONE), UiLook::Vegas23), vec![Marker(3)]);
    }
}
