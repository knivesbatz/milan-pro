//! Menu bar (§5.1, §6.1–6.7): the full File…Help trees per look, with mnemonics (underlined while Alt is held or
//! after Alt+letter / F10 opened a menu), right-aligned shortcuts from the keymap, check and radio marks, submenus
//! and greyed-out items for what Milan Pro cannot do yet. Menus and context menus send `Command`s on a per-frame
//! bus that the app drains; they read enabled/checked state from the `CmdState` the app publishes each frame.
use std::sync::Arc;
use eframe::egui::{self, *};
use eframe::egui::menu::{BarState, MenuRoot};
use eframe::egui::text::{LayoutJob, TextFormat};
use crate::decode::Quality;
use crate::history::History;
use crate::media::Importer;
use crate::project::*;
use crate::edit::{RippleMode, Switch};
use crate::settings::EnvView;
use super::chrome;
use super::EditTool;
use super::dock::Panel;
use super::keymap::{self, Command, Command::*};
use super::theme::{self, UiLook};

/// Menu items handled by the app (dialogs, and actions that may discard unsaved changes). Kept for the
/// toolbars, which predate `Command`.
#[derive(Clone, Copy, PartialEq)]
pub enum MenuAction {
    RenderAs,
    Properties,
    New,
    Open,
    Exit,
    /// Replace the project with an imported VEGAS project (after the save prompt).
    ImportVegas,
}

impl From<MenuAction> for Command {
    fn from(a: MenuAction) -> Self {
        match a {
            MenuAction::RenderAs => RenderAs,
            MenuAction::Properties => Properties,
            MenuAction::New => NewEmpty,
            MenuAction::Open => Open,
            MenuAction::Exit => Exit,
            MenuAction::ImportVegas => ImportVegas,
        }
    }
}

/// What menus need to know to grey out and check items; published by the app every frame.
#[derive(Clone, Default)]
pub struct CmdState {
    pub look: UiLook,
    pub can_undo: bool,
    pub can_redo: bool,
    /// An event is selected (and whether the focus event is locked); `n_sel` events are.
    pub clip: bool,
    pub clip_locked: bool,
    /// The focus event's Switches › Mute and Loop.
    pub clip_muted: bool,
    pub clip_looping: bool,
    /// A video event is selected (Pan/Crop, Video Event FX); an audio event is (Audio Event FX). A grouped A/V pair
    /// sets both.
    pub video_clip: bool,
    pub audio_clip: bool,
    /// A selected event has inactive takes.
    pub takes: bool,
    pub n_sel: usize,
    /// Tracks exist / are selected; the track list had the last click.
    pub any_tracks: bool,
    pub tracks_sel: bool,
    /// A selected track is in a track group.
    pub track_group: bool,
    pub list_focus: bool,
    pub quantize: bool,
    /// An event is on the clipboard.
    pub clipboard: bool,
    /// A time selection exists.
    pub selection: bool,
    pub looping: bool,
    pub snapping: bool,
    /// Options › Snap to Grid, Snap to All Events, Grid Spacing and Ruler Format.
    pub snap_grid: bool,
    pub snap_all: bool,
    pub grid: GridSpacing,
    pub ruler: RulerFormat,
    pub auto_crossfade: bool,
    pub open_panels: Vec<Panel>,
    pub quality: Option<Quality>,
    pub recent: Vec<String>,
    /// VP16 preference: Space and F12 Play/Pause instead of Play/Stop.
    pub space_pauses: bool,
    pub tool: super::EditTool,
    /// A selected event is in a group.
    pub in_group: bool,
    /// An edit Post-Edit Ripple can apply is waiting.
    pub last_edit: bool,
    pub auto_ripple: bool,
    pub ripple_mode: RippleMode,
    pub ignore_group: bool,
    /// View › Show Envelopes, Options › Lock Envelopes to Events.
    pub show_env: bool,
    pub lock_env: bool,
    /// Envelope points are selected (Delete removes them first).
    pub env_sel: bool,
    /// An audio / a video track is selected, and the envelopes the focus track of each kind has.
    pub audio_track_focus: bool,
    pub video_track_focus: bool,
    pub focus_envs: Vec<EnvKind>,
    pub focus_venvs: Vec<EnvKind>,
    /// View › Audio / Video Envelopes rows turned off (`EnvView::bit`).
    pub hidden_envs: u16,
    /// The focus video event has a Velocity envelope; a transition (into or out of it); a Transition Progress one.
    pub velocity: bool,
    pub transition: bool,
    /// R8c: the envelopes the focus audio event has; View › Show OFX Effect Envelopes.
    pub event_envs: Vec<EnvKind>,
    pub ofx_envelopes: bool,
    pub progress: bool,
    /// Options › Bypass All Audio FX.
    pub bypass_audio_fx: bool,
    /// The focus audio track is armed for recording; any track is; a recording is running.
    pub armed: bool,
    pub any_armed: bool,
    pub recording: bool,
    /// Live input peak (linear) shown in armed tracks' meters; None while no input is open.
    pub input_level: Option<f32>,
    /// Names of the saved user layouts 0-9 of this look (None = <EMPTY>).
    pub layouts: [Option<String>; 10],
    /// The user layout last loaded or saved (Save Layout saves over it); None = the default layout.
    pub layout_slot: Option<usize>,
    /// Edge-trimming mode is on (an event edge is selected with [ or ]).
    pub edge_mode: bool,
    /// Options › Mute All Audio / Mute All Video.
    pub mute_audio: bool,
    pub mute_video: bool,
    /// The R3b switches that are on: audio ones of the focus audio event, video ones of the focus video event; the
    /// focus video event's resample radio and the focus audio event's Channels.
    pub switches: Vec<crate::edit::Switch>,
    pub resample: Option<Resample>,
    pub channels: ChannelMap,
    /// Stream ▸: the focus event's stream, and how many of its kind its media has.
    pub stream: u32,
    pub streams: u32,
    /// Options › Quickfade Audio Edits.
    pub quickfade: bool,
    /// Options › Audio Follows Video Velocity (the project's).
    pub audio_follows_velocity: bool,
    /// View › Video Bus Track; Preferences › Video › extra blend modes.
    pub video_bus: bool,
    pub extra_modes: bool,
    /// The project has an assignable FX chain (Remove Assignable FX).
    pub assignable_fx: bool,
    /// R8b: View › Audio Bus Tracks; Dim Output is on; the track header's Show Output Meters, Use Vertical Meters and
    /// Channel Width (0 narrow, 1 default, 2 wide).
    pub bus_tracks: bool,
    pub dim: bool,
    pub output_meters: bool,
    pub vertical_meters: bool,
    pub meter_width: u8,
}

impl CmdState {
    pub fn enabled(&self, c: Command) -> bool {
        let v23 = self.look == UiLook::Vegas23;
        match c {
            New | NewEmpty | Open | Close | Save | SaveAs | ImportMedia | ImportVegas | Properties | Exit | RenderAs | Preferences
            | About | KeyboardShortcuts | UnfinishedFeatures | CheckForUpdates | NormalTool | LayoutDefault | SaveLayout | SaveLayoutAs
            | SaveLayoutUser(_) => true,
            LayoutUser(n) => self.layouts.get(n).is_some_and(Option::is_some),
            Welcome => v23,
            OpenRecent(i) => i < self.recent.len(),
            Undo | UndoAll => self.can_undo,
            Redo => self.can_redo,
            ClearHistory => self.can_undo || self.can_redo,
            TrimStart | TrimEnd | EventLock | SelEventStart | SelEventEnd | EventOpenInTrimmer | EventSelectInMedia
            | EventProperties | EventReverse | EventMute | RenameEvent | EventLoop | CreateSubclip => self.clip,
            PasteAttributes | PasteAttributesSelect => self.clipboard && self.n_sel > 0,
            InsertEmptyEvent => self.selection,
            GroupCut | GroupCopy | GroupDelete => self.in_group,
            NextTake | PrevTake | ChooseActiveTake | DeleteActiveTake | DeleteTake => self.takes,
            // With nothing selected, Split splits every event under the cursor.
            Split | SelectAll | SelectNone | Quantize | MinimizeTracks => true,
            // With Auto Ripple, Delete with only a time selection deletes that time.
            Delete => self.env_sel || self.n_sel > 0 || (self.list_focus && self.tracks_sel) || (self.auto_ripple && self.selection),
            ShowEnvelopes | LockEnvelopes => true,
            VolumeEnvelope | PanEnvelope | RemoveVolumeEnvelope | RemovePanEnvelope => self.audio_track_focus,
            ToggleEnvelope(_) => self.audio_track_focus,
            ToggleVideoEnvelope(_) => self.video_track_focus,
            ViewEnvelope(_) => true,
            EventVelocity | FreezeFrame => self.video_clip,
            EventEnvelope(_) => self.audio_clip,
            OfxEnvelopes => true,
            NextFxEnvelope | PrevFxEnvelope => self.audio_track_focus || self.video_track_focus,
            TransitionProgress => self.video_clip && self.transition,
            EventPanCrop | VideoEventFx => self.video_clip,
            TrackMotion | TrackFx => self.video_track_focus,
            VideoOutputFx | BypassAudioFx => true,
            AudioEventFx => self.audio_clip,
            AudioTrackFx | DefaultTrackFx => self.audio_track_focus,
            // Ctrl+R starts with armed tracks and stops a running recording; arming waits until it stops.
            Record => self.recording || self.any_armed,
            Arm => self.audio_track_focus && !self.recording,
            Trim => self.clip && self.selection,
            Copy => self.n_sel > 0,
            Cut => self.n_sel > 0 && !self.clip_locked,
            Paste | PasteInsert => self.clipboard,
            NextTool | PrevTool | AutoRipple | RippleMode(_) | IgnoreGrouping | Tool(_) => true,
            Group => self.n_sel >= 2,
            RemoveFromGroup | ClearGroup | SelectGroup => self.in_group,
            RippleAffected | RippleBus | RippleMarkers | RippleAll => self.last_edit,
            MuteTrack | SoloTrack => self.tracks_sel,
            FocusPrevTrack | FocusNextTrack => self.any_tracks,
            InsertRegion | ZoomSelection => self.selection,
            PlayStop | PlayPause | PlayFromStart | PlayAnyWindow | Stop | Loop | ShuttleReverse | ShuttlePause
            | ShuttleForward | SpacePlayPause => true,
            GoToStart | GoToEnd | SelOrViewStart | SelOrViewEnd | PrevFrame | NextFrame | PixelLeft | PixelRight
            | PrevMarker | NextMarker | PrevEditPoint | NextEditPoint | Marker(_) | CenterView => true,
            SetSelStart | SetSelEnd | SelectionToView | SelectionToProject | InsertMarker | InsertAudioTrack
            | InsertVideoTrack | Snapping | AutoCrossfade | ZoomIn | ZoomOut | ZoomOutLarge | TracksTaller
            | TracksShorter | DefaultTrackHeights | PreviewQuality(_) => true,
            GoTo | GridLeft | GridRight | SnapGrid | SnapAllEvents | SetGrid(_) | SetRuler(_) | SetTimeAtCursor | ProjectTempo => true,
            CursorToPlayhead | JogLeft | JogRight | SelectToNextMarker | SelectLoopRegion | RestoreSelection | SelectAfterCursor
            | ZoomFramePerThumb | WaveZoomIn | WaveZoomOut | PreviewAtCursor | MuteAllAudio | MuteAllVideo => true,
            SelectEventStart | SelectEventEnd | ExpandedEdit | SelectEventsToEnd => self.clip,
            ExitEditMode => self.edge_mode,
            SelPrevEvent | SelNextEvent | NudgeFrameLeft | NudgeFrameRight | NudgePixelLeft | NudgePixelRight | NudgeTrackUp
            | NudgeTrackDown => self.n_sel > 0,
            PasteRepeat => self.clipboard,
            MuteExclusive | SoloExclusive => self.tracks_sel,
            GroupMute | GroupSolo => self.track_group,
            ToggleListFocus => self.any_tracks,
            EventSwitch(s) => if s.video() { self.video_clip } else { self.audio_clip },
            SetResample(_) => self.video_clip,
            SetChannels(_) => self.audio_clip,
            PitchUp | PitchDown | PitchCentUp | PitchCentDown | PitchOctUp | PitchOctDown => self.audio_clip,
            SetStream(n) => self.clip && n < self.streams,
            Quickfade | AudioFollowsVelocity | VideoBusTrack | InsertAudioBus | InsertAssignableFx | InsertInputBus => true,
            RemoveAssignableFx => self.assignable_fx,
            BusTracks | DimMaster | AudioProperties | OutputMeters | VerticalMeters | MeterWidth(_) => true,
            Window(p) | ShowWindow(p) => p.in_look(self.look),
            _ => false,
        }
    }

    /// Some(on) for check and radio items.
    pub fn checked(&self, c: Command) -> Option<bool> {
        Some(match c {
            Loop => self.looping,
            Snapping => self.snapping,
            SnapGrid => self.snap_grid,
            SnapAllEvents => self.snap_all,
            SetGrid(g) => self.grid == g,
            SetRuler(r) => self.ruler == r,
            AutoCrossfade => self.auto_crossfade,
            Window(p) => self.open_panels.contains(&p),
            PreviewQuality(q) => self.quality == Some(q),
            NormalTool => self.tool == EditTool::Normal,
            Tool(t) => self.tool == t,
            AutoRipple => self.auto_ripple,
            RippleMode(m) => self.ripple_mode == m,
            IgnoreGrouping => self.ignore_group,
            EventLock => self.clip_locked,
            EventMute => self.clip_muted,
            EventLoop => self.clip_looping,
            SpacePlayPause => self.space_pauses,
            Quantize => self.quantize,
            ShowEnvelopes => self.show_env,
            LockEnvelopes => self.lock_env,
            BypassAudioFx => self.bypass_audio_fx,
            MuteAllAudio => self.mute_audio,
            MuteAllVideo => self.mute_video,
            EventSwitch(s) => self.switches.contains(&s),
            SetResample(r) => self.resample == r,
            SetChannels(ch) => self.channels == ch,
            SetStream(n) => self.stream == n,
            Quickfade => self.quickfade,
            AudioFollowsVelocity => self.audio_follows_velocity,
            VideoBusTrack => self.video_bus,
            BusTracks => self.bus_tracks,
            DimMaster => self.dim,
            OutputMeters => self.output_meters,
            VerticalMeters => self.vertical_meters,
            MeterWidth(w) => self.meter_width == w,
            Arm => self.armed,
            ToggleEnvelope(k) => self.focus_envs.contains(&k),
            ToggleVideoEnvelope(k) => self.focus_venvs.contains(&k),
            ViewEnvelope(v) => self.hidden_envs & v.bit() == 0,
            EventVelocity => self.velocity,
            EventEnvelope(k) => self.event_envs.contains(&k),
            OfxEnvelopes => self.ofx_envelopes,
            TransitionProgress => self.progress,
            LayoutUser(n) => self.layout_slot == Some(n),
            LayoutDefault => self.layout_slot.is_none(),
            // Always-on behaviour Milan Pro has no switch for.
            SnapMarkers | Waveforms | EventButtons | FadeLengths | EdgeFrames | TakeInfo => true,
            EventHeaders => self.look == UiLook::Vegas16,
            MediaMarkers | Multicam | ExternalMonitor => false,
            _ => return None,
        })
    }
}

// ---- Command bus ----

fn bus_id() -> Id {
    Id::new("milan_command_bus")
}

/// Queue `cmd` for the app (runs after this frame's UI).
pub fn send(ctx: &Context, cmd: Command) {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Vec<Command>>(bus_id()).push(cmd));
}

pub fn drain(ctx: &Context) -> Vec<Command> {
    ctx.data_mut(|d| d.remove_temp::<Vec<Command>>(bus_id())).unwrap_or_default()
}

pub fn publish_state(ctx: &Context, st: CmdState) {
    ctx.data_mut(|d| d.insert_temp(Id::new("milan_cmd_state"), Arc::new(st)));
}

pub fn state(ctx: &Context) -> Arc<CmdState> {
    ctx.data(|d| d.get_temp::<Arc<CmdState>>(Id::new("milan_cmd_state"))).unwrap_or_default()
}

// ---- Why a control is greyed out (R0: nothing is greyed without saying why) ----

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Why {
    /// Not built yet; the batch of docs/REMAINING.md that builds it.
    Later(&'static str),
    /// VEGAS long tail (R57-R60): no decision to build it.
    NotPlanned(&'static str),
    /// Anything else, as a lowercase phrase: a behaviour Milan Pro has without a switch, a choice fixed by the look...
    Fixed(&'static str),
}

pub use Why::*;

/// A VEGAS switch for something Milan Pro always does.
pub const ALWAYS_ON: Why = Fixed("always on in Milan Pro (there is no switch)");

impl Why {
    /// "not available yet: batch R8b", to go inside a sentence.
    pub fn phrase(self) -> String {
        match self {
            Later(b) => format!("not available yet: batch {b}"),
            NotPlanned(b) => format!("not planned: VEGAS long tail {b}"),
            Fixed(s) => s.to_string(),
        }
    }

    /// Hover text for a greyed menu row or dialog control.
    pub fn text(self) -> String {
        let p = self.phrase(); // starts with an ASCII letter
        p[..1].to_uppercase() + &p[1..]
    }

    /// Tooltip for a toolbar button: "Mixing Console (not available yet: batch R8b)".
    pub fn tip(self, label: &str) -> String {
        format!("{label} ({})", self.phrase())
    }
}

/// Why `c` is greyed out whatever is selected; None for commands that work (they may still need a selection).
pub fn unfinished(c: Command) -> Option<Why> {
    Some(match c {
        SaveVersion => Later("R42"),
        NestedCreate | NestedOpen | NestedParent => Later("R6b"),
        SurroundPanner => Later("R10b"),
        RenderToNewTrack => Later("R12"),
        ToggleDocks | TopDock | BottomDock | RightDock | CollapseTrackList | MaximizeTimeline | LayoutVp14 | LayoutAudio | LayoutColor
        | LayoutSource | LayoutTimelineTop | NextWindow | PrevWindow | FocusTimeline => Later("R13"),
        ExternalMonitor => Later("R14"),
        EditDetails | ProjectNotes => Later("R16a"),
        VideoScopes => Later("R16b"),
        LoudnessMeters => Later("R16c"),
        Multicam => Later("R17b"),
        SmartSplit | OpenInAudioEditor | InsertCommand | CdTrack | CdIndex | MediaMarkers | RebuildPeaks | ContextMenu => Later("R18a"),
        InsertAdjustmentTrack => Later("R18b"),
        Prerender | RamPreview => Later("R23"),
        PluginManager => Later("R31"),
        Help | WhatsThis => Later("R53"),
        DeviceExplorer | Dashboard | MotionTracking | TextToSpeech | MidiTimecode | MidiClock | MidiTrigger => NotPlanned("R59"),
        SnapMarkers | Waveforms | EventButtons | FadeLengths | EdgeFrames | TakeInfo => ALWAYS_ON,
        EventHeaders => Fixed("set by the look: on in VEGAS 16, off in VEGAS 23"),
        _ => return None,
    })
}

// ---- Rows ----

#[derive(Clone, Copy, PartialEq)]
pub enum Mark {
    None,
    Check(bool),
    Radio(bool),
}

fn kb_id() -> Id {
    Id::new("milan_menu_kb")
}

/// Underline mnemonics: Alt held, or the menu was opened from the keyboard.
fn show_mnemonics(ctx: &Context) -> bool {
    keymap::alt_held(ctx) || ctx.data(|d| d.get_temp::<bool>(kb_id())).unwrap_or(false)
}

fn label_job(ui: &Ui, label: &str, color: Color32, gutter: f32) -> LayoutJob {
    let t = theme::get();
    let (text, at) = keymap::mnemonic(label);
    let font = t.font(t.font.menu);
    let mut job = LayoutJob::default();
    let fmt = TextFormat { font_id: font, color, ..Default::default() };
    let under = TextFormat { underline: Stroke::new(1.0_f32, color), ..fmt.clone() };
    match at.filter(|_| show_mnemonics(ui.ctx())) {
        Some(i) => {
            let n = text[i..].chars().next().map_or(0, char::len_utf8);
            job.append(&text[..i], gutter, fmt.clone());
            job.append(&text[i..i + n], 0.0, under);
            job.append(&text[i + n..], 0.0, fmt);
        }
        None => job.append(&text, gutter, fmt),
    }
    job
}

fn row_colors(enabled: bool) -> (Color32, Color32) {
    let t = theme::get();
    let text = if enabled { t.menu_popup_text } else { t.menu_popup_text_disabled };
    let short = if !enabled { t.menu_popup_text_disabled } else if t.look() == UiLook::Vegas16 { t.menu_popup_text } else { t.text_dim };
    (text, short)
}

/// Keyboard-opened menus activate an item by its mnemonic letter.
fn mnemonic_hit(ui: &Ui, label: &str) -> bool {
    let kb = ui.ctx().data(|d| d.get_temp::<bool>(kb_id())).unwrap_or(false);
    kb && keymap::mnemonic_key(label).is_some_and(|k| ui.ctx().input_mut(|i| i.consume_key(Modifiers::NONE, k)))
}

/// One popup row: 28 px gutter (✓ / ●), label with mnemonic, right-aligned shortcut. True when chosen; the menu
/// then closes.
pub fn entry(ui: &mut Ui, label: &str, shortcut: &str, enabled: bool, mark: Mark) -> bool {
    row(ui, label, shortcut, enabled, mark, None)
}

/// `entry` that, greyed out, says `why` on hover.
fn row(ui: &mut Ui, label: &str, shortcut: &str, enabled: bool, mark: Mark, why: Option<Why>) -> bool {
    let (text, short) = row_colors(enabled);
    let mut b = Button::new(label_job(ui, label, text, 26.0)).shortcut_text(RichText::new(shortcut).color(short));
    if !enabled {
        // Same padding as live rows, no hover box.
        b = b.sense(Sense::hover()).fill(Color32::TRANSPARENT).stroke(Stroke::NONE);
    }
    let mut r = ui.add(b);
    if let Some(w) = why.filter(|_| !enabled) {
        r = r.on_hover_text(w.text());
    }
    let c = pos2(r.rect.min.x + 14.0, r.rect.center().y);
    match mark {
        Mark::Check(true) => {
            let s = Stroke::new(1.5_f32, text);
            ui.painter().line_segment([c + vec2(-4.0, 0.0), c + vec2(-1.5, 3.0)], s);
            ui.painter().line_segment([c + vec2(-1.5, 3.0), c + vec2(4.0, -3.5)], s);
        }
        Mark::Radio(true) => {
            ui.painter().circle_filled(c, 3.0, text);
        }
        _ => {}
    }
    let hit = r.clicked() || (enabled && mnemonic_hit(ui, label));
    if hit {
        ui.close_menu();
    }
    hit
}

/// A row for `cmd`: state from the published `CmdState`, shortcut from the keymap; sends it when chosen.
pub fn cmd(ui: &mut Ui, label: &str, c: Command) {
    let st = state(ui.ctx());
    let radio = matches!(
        c,
        PreviewQuality(_) | NormalTool | Tool(_) | RippleMode(_) | MeterWidth(_) | LayoutUser(_) | LayoutDefault | SetGrid(_) | SetRuler(_) | SetResample(_) | SetChannels(_) | SetStream(_)
    );
    let mark = match st.checked(c) {
        Some(on) if radio => Mark::Radio(on),
        Some(on) => Mark::Check(on),
        None => Mark::None,
    };
    if row(ui, label, &keymap::shortcut_text(c, st.look), st.enabled(c), mark, unfinished(c)) {
        send(ui.ctx(), c);
    }
}

/// Greyed-out row for something Milan Pro does not have, saying why on hover.
pub fn off(ui: &mut Ui, label: &str, why: Why) {
    inert(ui, label, Mark::None, why);
}

/// Greyed-out row with a fixed check or radio mark.
pub fn inert(ui: &mut Ui, label: &str, mark: Mark, why: Why) {
    row(ui, label, "", false, mark, Some(why));
}

pub fn sep(ui: &mut Ui) {
    let t = theme::get();
    let (_, r) = ui.allocate_space(vec2(ui.available_width(), 9.0));
    let y = r.center().y.floor() + 0.5;
    ui.painter().hline(r.x_range().shrink(4.0), y, Stroke::new(1.0_f32, t.text_dim.gamma_multiply(0.4)));
}

/// Submenu row with ▸ (egui draws the arrow and opens it on hover).
pub fn sub(ui: &mut Ui, label: &str, enabled: bool, add: impl FnOnce(&mut Ui)) {
    let (text, _) = row_colors(enabled);
    ui.menu_button(label_job(ui, label, text, 26.0), |ui| {
        ui.set_min_width(180.0);
        add(ui)
    });
}

// ---- Static trees ----

/// A menu tree entry. `V16` / `V23` limit an entry to one look.
pub enum M {
    C(&'static str, Command),
    /// Not available: greyed out, saying why on hover.
    X(&'static str, Why),
    /// Not available, shown with a fixed check / radio state.
    K(&'static str, bool, Why),
    R(&'static str, bool, Why),
    S,
    Sub(&'static str, &'static [M]),
    /// Items shown in place (shared blocks).
    Inline(&'static [M]),
    V16(&'static M),
    V23(&'static M),
    Recent,
    /// View › Window Layouts: user slots 0-9 with their names.
    UserLayouts,
    /// Stream ▸: a radio per stream of the focus event's media (Stream 1, Stream 2...).
    Streams,
}

use M::*;

pub fn show(ui: &mut Ui, items: &[M]) {
    let look = theme::get().look();
    for item in items {
        show_item(ui, item, look);
    }
}

fn show_item(ui: &mut Ui, item: &M, look: UiLook) {
    match item {
        C(l, c) => cmd(ui, l, *c),
        X(l, w) => off(ui, l, *w),
        K(l, on, w) => inert(ui, l, Mark::Check(*on), *w),
        R(l, on, w) => inert(ui, l, Mark::Radio(*on), *w),
        S => sep(ui),
        Sub(l, items) => sub(ui, l, true, |ui| show(ui, items)),
        Inline(items) => show(ui, items),
        V16(i) if look == UiLook::Vegas16 => show_item(ui, i, look),
        V23(i) if look == UiLook::Vegas23 => show_item(ui, i, look),
        V16(_) | V23(_) => {}
        Recent => {
            let st = state(ui.ctx());
            for (i, p) in st.recent.iter().enumerate() {
                cmd(ui, &format!("&{} {}", i + 1, p.replace('&', "&&")), OpenRecent(i));
            }
            if st.recent.is_empty() {
                off(ui, "Recent projects", Fixed("no recent projects yet"));
            }
        }
        UserLayouts => {
            let st = state(ui.ctx());
            for (n, name) in st.layouts.iter().enumerate() {
                cmd(ui, &format!("&{n} {}", name.as_ref().map_or("<EMPTY>".into(), |s| s.replace('&', "&&"))), LayoutUser(n));
            }
        }
        Streams => {
            for n in 0..state(ui.ctx()).streams.max(1) {
                cmd(ui, &format!("Stream &{}", n + 1), SetStream(n));
            }
        }
    }
}

const IMPORT: &[M] = &[
    C("&Media...", ImportMedia),
    C("&VEGAS Project...", ImportVegas),
    X("Media from &Project...", Later("R30")),
    V16(&X("&AAF...", AAF)),
    X("P&remiere/After Effects (*.prproj)...", Later("R29")),
    X("&Final Cut Pro 7/DaVinci Resolve (*.xml)...", Later("R29")),
    X("Final Cut Pro &X (*.fcpxml)...", Later("R29")),
    X("&EDL Text File (*.txt)...", Later("R28")),
    X("&Broadcast Wave Format...", NotPlanned("R57")),
    X("&Closed Captioning...", Later("R39")),
];

const AAF: Why = Fixed("not feasible: no Rust AAF library (OTIO route in batch R29)");

const EXPORT: &[M] = &[
    X("&Milan Pro Project Archive...", Later("R30")),
    X("Premiere/&After Effects (*.prproj)...", Later("R29")),
    X("&Final Cut Pro 7/DaVinci Resolve (*.xml)...", Later("R29")),
    X("Final Cut Pro &X (*.fcpxml)...", Later("R29")),
    X("&EDL Text File (*.txt)...", Later("R28")),
];

pub const FILE: &[M] = &[
    C("&New...", New),
    C("New Empty Pro&ject", NewEmpty),
    C("&Open...", Open),
    C("&Close", Close),
    S,
    C("&Save", Save),
    C("Save &As...", SaveAs),
    V23(&C("Save Version", SaveVersion)),
    S,
    V16(&C("&Render As...", RenderAs)),
    V23(&C("&Render...", RenderAs)),
    X("Real-Ti&me Render...", Later("R12")),
    S,
    Sub("&Import", IMPORT),
    Sub("E&xport", EXPORT),
    V16(&X("Capture &Video...", NotPlanned("R59"))),
    X("&Extract Audio from CD...", NotPlanned("R59")),
    S,
    V23(&C("Welcome Screen", Welcome)),
    C("&Properties...", Properties),
    S,
    Recent,
    S,
    C("E&xit", Exit),
];

const NAVIGATE: &[M] = &[
    C("&Go to Time...", GoTo),
    C("Go to Project &Start", GoToStart),
    C("Go to Project &End", GoToEnd),
    C("Go to &Time Selection/View Start", SelOrViewStart),
    C("Go to Time Selection/&View End", SelOrViewEnd),
    C("M&ove Left by Grid Marks", GridLeft),
    C("Move Right by Grid &Marks", GridRight),
    C("Move Left by &Pixel", PixelLeft),
    C("Move Right by P&ixel", PixelRight),
    C("Move Left by &Frame", PrevFrame),
    C("Move Right by F&rame", NextFrame),
    S,
    C("Move Left by M&arker/Region", PrevMarker),
    C("Move Rig&ht by Marker/Region", NextMarker),
    C("Time Sele&ct Right by Marker/Region", SelectToNextMarker),
    S,
    C("Move &Left by Event Edit Points", PrevEditPoint),
    C("Move Right by Event E&dit Points", NextEditPoint),
    C("Move Left &by Selected Event", SelEventStart),
    C("Move Right b&y Selected Event", SelEventEnd),
    S,
    C("Move Cursor to Play&head", CursorToPlayhead),
    C("Center View aro&und Cursor", CenterView),
];

const RIPPLE: &[M] = &[
    C("Affected &Tracks", RippleAffected),
    C("Affected Tracks, &Bus Tracks, Markers, and Regions", RippleBus),
    C("Affected Tracks, &Markers, and Regions", RippleMarkers),
    C("&All Tracks, Markers, and Regions", RippleAll),
];

const SELECT: &[M] = &[
    C("Select &All", SelectAll),
    V23(&C("Select All After Cursor", SelectAfterCursor)),
    C("Select Event Sta&rt", SelectEventStart),
    C("Select Event &End", SelectEventEnd),
    C("Enter Expanded Edit &Mode", ExpandedEdit),
    V16(&C("Exit E&dit Mode", ExitEditMode)),
    V23(&C("Exit Expanded E&dit Mode", ExitEditMode)),
    S,
    C("&Loop Playback", Loop),
    S,
    C("&Zoom Selection", ZoomSelection),
    C("Set Selection to &View", SelectionToView),
    C("Set Selection to &Project", SelectionToProject),
    C("&Select Loop Region", SelectLoopRegion),
    S,
    C("Set Selection S&tart", SetSelStart),
    C("Set Selection E&nd", SetSelEnd),
    C("Set Selection to Prev&ious Selected Event", SelPrevEvent),
    C("Set Selection to Ne&xt Selected Event", SelNextEvent),
    C("Rest&ore Previous Time Selection", RestoreSelection),
];

const TOOLS_SUB: &[M] = &[
    C("&Normal Edit Tool", NormalTool),
    R("S&huffle Tool", false, NotPlanned("R58")),
    C("Sl&ip Tool", Tool(EditTool::Slip)),
    C("Sli&de Tool", Tool(EditTool::Slide)),
    C("Time Stretch/&Compress Tool", Tool(EditTool::Stretch)),
    R("Split &Trim Tool", false, NotPlanned("R58")),
    S,
    C("&Envelope", Tool(EditTool::Envelope)),
    C("&Selection", Tool(EditTool::Selection)),
    R("&Zoom", false, NotPlanned("R58")),
    S,
    C("Ne&xt Tool", NextTool),
    C("&Previous Tool", PrevTool),
];

const SWITCHES: &[M] = &[
    C("&Mute", EventMute),
    C("&Lock", EventLock),
    C("L&oop", EventLoop),
    S,
    Inline(AUDIO_SWITCHES),
    S,
    Inline(VIDEO_SWITCHES),
];

/// The audio event switches (Edit › Switches and the audio event menu).
pub const AUDIO_SWITCHES: &[M] = &[
    C("&Invert Phase", EventSwitch(Switch::Invert)),
    C("&Normalize", EventSwitch(Switch::Normalize)),
    V23(&C("Auto Normalize", EventSwitch(Switch::AutoNormalize))),
];

/// The video event switches (Edit › Switches and the video event menu).
pub const VIDEO_SWITCHES: &[M] = &[
    C("Maintain &Aspect Ratio", EventSwitch(Switch::KeepAspect)),
    C("Reduce Interlace &Flicker", EventSwitch(Switch::ReduceFlicker)),
    S,
    C("Use P&roject Resample Mode", SetResample(None)),
    C("Force Re&sample", SetResample(Some(Resample::Force))),
    C("&Disable Resample", SetResample(Some(Resample::Disable))),
    V23(&C("Optical Flow", SetResample(Some(Resample::OpticalFlow)))),
];

/// Edit › Take and the event menu's Take ▸; Choose Active... and Delete... open the Take Chooser.
pub const TAKE: &[M] = &[
    C("&Rename Active", RenameEvent),
    C("&Choose Active...", ChooseActiveTake),
    S,
    C("&Next Take", NextTake),
    C("&Previous Take", PrevTake),
    S,
    C("Delete &Active", DeleteActiveTake),
    C("&Delete...", DeleteTake),
];

/// Edit › Group and the event menu's Group ▸.
pub const GROUP: &[M] = &[
    C("Create &New", Group),
    S,
    C("&Remove From", RemoveFromGroup),
    C("&Clear", ClearGroup),
    C("&Select All", SelectGroup),
    S,
    C("Cut &All", GroupCut),
    C("Cop&y All", GroupCopy),
    C("&Delete All", GroupDelete),
];

pub const CHANNELS: &[M] = &[
    C("&Both", SetChannels(ChannelMap::Both)),
    C("&Left Only", SetChannels(ChannelMap::LeftOnly)),
    C("&Right Only", SetChannels(ChannelMap::RightOnly)),
    C("&Combine", SetChannels(ChannelMap::Combine)),
    C("&Swap", SetChannels(ChannelMap::Swap)),
];

pub const STREAM: &[M] = &[Streams];

pub const EDIT: &[M] = &[
    C("&Undo", Undo),
    C("&Redo", Redo),
    S,
    C("Cu&t", Cut),
    C("&Copy", Copy),
    C("&Paste", Paste),
    C("P&aste Repeat...", PasteRepeat),
    C("Pa&ste Insert", PasteInsert),
    C("Paste E&vent Attributes", PasteAttributes),
    C("Selectively Paste Event Attributes...", PasteAttributesSelect),
    S,
    C("&Delete", Delete),
    C("Tri&m", Trim),
    C("Trim &Start", TrimStart),
    C("Trim &End", TrimEnd),
    C("Spl&it", Split),
    V23(&C("Smart Split", SmartSplit)),
    S,
    Sub("Na&vigate", NAVIGATE),
    Sub("P&ost-Edit Ripple", RIPPLE),
    Sub("Se&lect", SELECT),
    Sub("&Editing Tool", TOOLS_SUB),
    S,
    Sub("S&witches", SWITCHES),
    Sub("Ta&ke", TAKE),
    Sub("&Group", GROUP),
    Sub("&Stream", STREAM),
    Sub("C&hannels", CHANNELS),
    S,
    C("U&ndo All", UndoAll),
    C("Clear Edit Histor&y...", ClearHistory),
];

const LAYOUTS: &[M] = &[
    X("&Organize Layouts...", Later("R13")),
    C("&Save Layout", SaveLayout),
    C("Save Layout &As...", SaveLayoutAs),
    X("R&eload Selected Layout", Later("R13")),
    S,
    C("&Default Layout", LayoutDefault),
    V16(&C("Classic Default Layout", LayoutVp14)),
    C("Audio Mixing", LayoutAudio),
    C("Color Correction", LayoutColor),
    V23(&C("Source Window Layout", LayoutSource)),
    V23(&C("Timeline Top Layout", LayoutTimelineTop)),
    S,
    UserLayouts,
];

const WINDOWS: &[M] = &[
    C("E&xplorer", Window(Panel::Explorer)),
    C("T&rimmer", Window(Panel::Trimmer)),
    C("&Master Bus", Window(Panel::MasterBus)),
    C("&Video Preview", Window(Panel::VideoPreview)),
    C("Pro&ject Media", Window(Panel::ProjectMedia)),
    C("Edit &Details", EditDetails),
    V23(&C("Project Notes", ProjectNotes)),
    C("&Transitions", Window(Panel::Transitions)),
    C("V&ideo FX", Window(Panel::VideoFx)),
    C("Media &Generators", Window(Panel::MediaGenerators)),
    C("Compositors", Window(Panel::Compositors)),
    C("&Plug-In Manager", PluginManager),
    C("Video &Scopes", VideoScopes),
    C("S&urround Panner", SurroundPanner),
    C("Mixing Console", Window(Panel::MixingConsole)),
    C("Device Exp&lorer", DeviceExplorer),
    C("Loudness Meters", LoudnessMeters),
    V23(&C("Dashboard", Dashboard)),
    V23(&C("Color Grading", Window(Panel::ColorGrading))),
    S,
    // Milan Pro's own window (not in VEGAS).
    C("Pr&operties", Window(Panel::Properties)),
];

const AUDIO_ENV_VIEW: &[M] = &[
    C("Track &Mute", ViewEnvelope(EnvView::AudioMute)),
    C("Track &Volume", ViewEnvelope(EnvView::AudioVolume)),
    C("Track &Pan", ViewEnvelope(EnvView::AudioPan)),
];

const VIDEO_ENV_VIEW: &[M] = &[
    C("Track &Mute", ViewEnvelope(EnvView::VideoMute)),
    C("Track &Composite Level", ViewEnvelope(EnvView::CompositeLevel)),
    C("Track &Fade to Color", ViewEnvelope(EnvView::FadeToColor)),
    C("Track &Keyframes", ViewEnvelope(EnvView::TrackKeyframes)),
    S,
    C("Event &Velocity", ViewEnvelope(EnvView::Velocity)),
    C("Transition &Progress", ViewEnvelope(EnvView::TransitionProgress)),
    S,
    C("Motion &Blur Amount", ViewEnvelope(EnvView::MotionBlur)),
    C("Video &Supersampling", ViewEnvelope(EnvView::Supersampling)),
];

pub const VIEW: &[M] = &[
    K("&Toolbar", true, ALWAYS_ON),
    K("Status &Bar", true, ALWAYS_ON),
    S,
    Sub("Window La&youts", LAYOUTS),
    S,
    C("&Focus to Timeline", FocusTimeline),
    Sub("Window", WINDOWS),
    S,
    // The preview bus it shows plays the Explorer's previews (R57); Milan Pro has none yet.
    K("Mixer Previe&w Fader", false, NotPlanned("R57")),
    S,
    C("Audio Bus Tra&cks", BusTracks),
    C("Vide&o Bus Track", VideoBusTrack),
    S,
    Sub("Eve&nt Media Markers", &[C("&Show Markers", MediaMarkers), K("Show Marker &Labels", false, Later("R18a"))]),
    C("Active Take &Information", TakeInfo),
    C("Audio Event &Waveforms", Waveforms),
    C("Event &Buttons", EventButtons),
    C("Event Fade &Lengths", FadeLengths),
    C("Video &Event Edge Edit Frames", EdgeFrames),
    C("Event Headers", EventHeaders),
    V23(&K("Event Length", false, Later("R18a"))),
    V23(&C("Show OFX Effect Envelopes (Beta)", OfxEnvelopes)),
    S,
    Sub("&Audio Envelopes", AUDIO_ENV_VIEW),
    Sub("Video &Envelopes", VIDEO_ENV_VIEW),
    C("S&how Envelopes", ShowEnvelopes),
    S,
    Sub("Preview &Quality", &[
        C("&Draft", PreviewQuality(Quality::Draft)),
        C("&Preview", PreviewQuality(Quality::Preview)),
        C("&Good", PreviewQuality(Quality::Good)),
        C("&Best", PreviewQuality(Quality::Best)),
    ]),
    S,
    C("Minimi&ze All Tracks", MinimizeTracks),
    C("Rebuild Audio Pea&ks", RebuildPeaks),
];

pub const INSERT: &[M] = &[
    // Insert/Remove: checked while the focus track has it (V and P only insert; Shift+V and Shift+P remove).
    Sub("A&udio Envelopes", &[C("&Mute", ToggleEnvelope(EnvKind::Mute)), C("&Volume", ToggleEnvelope(EnvKind::Volume)), C("&Pan", ToggleEnvelope(EnvKind::Pan))]),
    Sub("V&ideo Envelopes", &[
        C("Track &Mute", ToggleVideoEnvelope(EnvKind::Mute)),
        C("Track &Composite Level", ToggleVideoEnvelope(EnvKind::CompositeLevel)),
        C("Track &Fade to Color", ToggleVideoEnvelope(EnvKind::FadeToColor)),
        S,
        C("Event &Velocity", EventVelocity),
        C("Transition &Progress", TransitionProgress),
    ]),
    S,
    C("&Audio Track", InsertAudioTrack),
    C("&Video Track", InsertVideoTrack),
    V23(&C("Adjustment Track", InsertAdjustmentTrack)),
    S,
    C("Audio &Bus", InsertAudioBus),
    C("Audio In&put Bus", InsertInputBus),
    C("Audio Assignable &FX...", InsertAssignableFx),
    S,
    C("&Empty Event", InsertEmptyEvent),
    V23(&X("Adjustment Event", Later("R18b"))),
    C("&Generated Media...", ShowWindow(Panel::MediaGenerators)), // opens, never closes, and has no check
    V23(&X("&Slideshow...", Later("R18b"))),
    V23(&X("Insert Subtitles from Files...", Later("R39"))),
    V23(&X("Insert Subtitles from Regions...", Later("R39"))),
    S,
    X("Audio Tra&nsition...", Later("R18b")),
    S,
    X("&Time...", Later("R18b")),
    S,
    C("&Marker", InsertMarker),
    C("&Region", InsertRegion),
    S,
    C("Audio CD Trac&k Region", CdTrack),
    C("Audio CD Track Inde&x", CdIndex),
    S,
    C("&Command...", InsertCommand),
];

pub const TOOLS: &[M] = &[
    C("&Open in Audio Editor", OpenInAudioEditor),
    X("Open &Copy in Audio Editor", Later("R18a")),
    X("Edit Source Pro&ject", Later("R6b")),
    Sub("&Audio", &[
        X("&Open in Audio Editor", Later("R18a")),
        X("Open &Copy in Audio Editor", Later("R18a")),
        X("Edit Source Pro&ject", Later("R6b")),
        S,
        C("Audio &Event FX...", AudioEventFx),
        X("Apply Non-Real-Time Event &FX...", NotPlanned("R58")),
        S,
        C("&Track FX...", AudioTrackFx),
        S,
        C("&Remove Assignable FX...", RemoveAssignableFx),
    ]),
    Sub("&Video", &[
        X("&Media FX...", NotPlanned("R57")),
        S,
        C("Video Event &Pan/Crop...", EventPanCrop),
        C("Video Event &FX...", VideoEventFx),
        S,
        C("Video Track F&X...", TrackFx),
        C("&Track Motion...", TrackMotion),
        S,
        C("Vi&deo Output FX...", VideoOutputFx),
        V23(&C("Motion Tracking", MotionTracking)),
    ]),
    S,
    C("&Render to New Track...", RenderToNewTrack),
    C("Preview on E&xternal Monitor", ExternalMonitor),
    C("&Selectively Prerender Video...", Prerender),
    X("Clean &Up Prerendered Video...", Later("R23")),
    C("Build Dynamic RAM Previe&w", RamPreview),
    V23(&X("Clear Dynamic RAM Preview", Later("R23"))),
    S,
    X("&Lay Out Audio CD from Events", NotPlanned("R59")),
    Sub("&Burn Disc", &[
        X("&Track-at-Once Audio CD...", NotPlanned("R59")),
        X("&Disc-at-Once Audio CD...", NotPlanned("R59")),
        X("&DVD...", NotPlanned("R59")),
        X("&Blu-ray Disc...", Fixed("not feasible: needs AACS licensing (VEGAS long tail R59)")),
    ]),
    X("Generate Loudness Log", Later("R16c")),
    V23(&X("Advanced Save...", Later("R30"))),
    V23(&X("Speech to Text...", NotPlanned("R59"))),
    V23(&C("Text to Speech...", TextToSpeech)),
    S,
    Sub("Scrip&ting", &[X("Ru&n Script...", NotPlanned("R60")), X("Res&can Script Menu Folder", NotPlanned("R60"))]),
    Sub("&Multicamera", &[
        X("Synchronize Audio to A&lign Events", Later("R17a")),
        X("Synchronize Media &Timecode to Align Events", Later("R17a")),
        X("Synchronize &Date/Time Stamp to Align Events", Later("R17a")),
        X("&Create Multicamera Track", Later("R17b")),
        X("E&xpand to Multiple Tracks...", Later("R17b")),
        S,
        C("&Enable Multicamera Editing", Multicam),
        K("Edit Multicamera &Audio with Video", false, Later("R17b")),
        K("Show Full Frame in External &Monitor", false, Later("R17b")),
    ]),
    S,
    X("&Clean Project Media", Later("R45b")),
    X("Rebuild Offline &Nested Audio", Later("R6a")),
];

pub const GRID_SPACING: &[M] = {
    use GridSpacing::*;
    &[
        C("&Ruler Marks", SetGrid(RulerMarks)),
        S,
        C("Mi&nutes", SetGrid(Minutes)),
        C("&Seconds", SetGrid(GridSpacing::Seconds)),
        C("&Milliseconds", SetGrid(Milliseconds)),
        C("&Frames", SetGrid(Frames)),
        S,
        C("Me&asures", SetGrid(Measures)),
        C("&Quarter Notes", SetGrid(Note(4, false))),
        C("Q&uarter Note Triplets", SetGrid(Note(4, true))),
        C("&8th Notes", SetGrid(Note(8, false))),
        C("8th Note &Triplets", SetGrid(Note(8, true))),
        C("&16th Notes", SetGrid(Note(16, false))),
        C("16t&h Note Triplets", SetGrid(Note(16, true))),
        C("&32nd Notes", SetGrid(Note(32, false))),
        C("32nd Note Tr&iplets", SetGrid(Note(32, true))),
    ]
};

/// The Ruler Format radios: Options › Ruler Format, the time ruler's and the time display's right-click lists.
pub const RULER_FORMATS: &[M] = {
    use RulerFormat::*;
    &[
        C("&Samples", SetRuler(Samples)),
        C("&Time", SetRuler(Time)),
        C("Se&conds", SetRuler(RulerFormat::Seconds)),
        C("Time && Fra&mes", SetRuler(TimeFrames)),
        C("Absol&ute Frames", SetRuler(AbsFrames)),
        C("Measures && &Beats", SetRuler(MeasuresBeats)),
        C("Feet and Frames &16mm (40 fpf)", SetRuler(Feet16)),
        C("Feet and Frames &35mm (16 fpf)", SetRuler(Feet35)),
        C("SMPTE Fi&lm Sync IVTC (23.976 fps, Video)", SetRuler(SmpteFilmIvtc)),
        C("SMPTE &Film Sync (24 fps, Film)", SetRuler(SmpteFilm)),
        C("SMPTE &EBU (25 fps, Video)", SetRuler(SmpteEbu)),
        C("SMPTE &Non-Drop (29.97 fps, Video)", SetRuler(SmpteNonDrop)),
        C("SMPTE &Drop (29.97 fps, Video)", SetRuler(SmpteDrop)),
        C("SM&PTE 30 (30 fps, Audio)", SetRuler(Smpte30)),
        C("&Audio CD Time", SetRuler(AudioCd)),
    ]
};

/// Options › Ruler Format, also the time ruler's right-click menu (§6.6, §6.10).
pub const RULER_MENU: &[M] = &[
    Inline(RULER_FORMATS),
    S,
    C("Set T&ime at Cursor", SetTimeAtCursor),
    V23(&C("Set Project Tempo", ProjectTempo)),
];

pub const OPTIONS: &[M] = &[
    C("&Quantize to Frames", Quantize),
    S,
    C("&Enable Snapping", Snapping),
    C("&Snap to Grid", SnapGrid),
    C("S&nap to Markers", SnapMarkers),
    C("Snap to All Even&ts", SnapAllEvents),
    S,
    Sub("Gr&id Spacing", GRID_SPACING),
    Sub("Ruler &Format", RULER_MENU),
    S,
    C("&Automatic Crossfades", AutoCrossfade),
    C("Q&uickfade Audio Edits", Quickfade),
    C("Audio Follows Video Velocit&y", AudioFollowsVelocity),
    C("Auto &Ripple", AutoRipple),
    Sub("&Ripple", &[
        C("Auto &Ripple", AutoRipple),
        S,
        C("Affected &Tracks", RippleMode(RippleMode::Affected)),
        C("Affected Tracks, &Bus Tracks, Markers, and Regions", RippleMode(RippleMode::AffectedMarkers)),
        C("Affected Tracks, &Markers, and Regions", RippleMode(RippleMode::AffectedMarkersOnly)),
        C("&All Tracks, Markers, and Regions", RippleMode(RippleMode::All)),
    ]),
    C("Lock En&velopes to Events", LockEnvelopes),
    C("I&gnore Event Grouping", IgnoreGrouping),
    K("Slip All Ta&kes", false, NotPlanned("R58")),
    S,
    C("&Loop Playback", Loop),
    K("Metr&onome", false, Later("R18a")),
    Sub("&Loudness Meters", &[
        X("&Reset Clip", Later("R16c")),
        X("Reset Meterin&g Engine", Later("R16c")),
        S,
        R("EB&U R 128 Mode", true, Later("R16c")),
        R("&ATSC A 85 Mode", false, Later("R16c")),
        S,
        K("True Peak &Blocking Filter", false, Later("R16c")),
        Sub("&True Peak Range", &[
            R("-12 to 0 dB", false, Later("R16c")),
            R("-24 to 0 dB", false, Later("R16c")),
            R("-42 to 0 dB", false, Later("R16c")),
            R("-60 to 0 dB", true, Later("R16c")),
            R("-78 to 0 dB", false, Later("R16c")),
            R("-90 to 0 dB", false, Later("R16c")),
            R("-120 to 0 dB", false, Later("R16c")),
            R("-138 to 0 dB", false, Later("R16c")),
        ]),
        Sub("Loudness S&cale", &[
            R("&EBU +9", true, Later("R16c")),
            R("E&BU +18", false, Later("R16c")),
            S,
            R("&Absolute (-23 LUFS)", false, Later("R16c")),
        ]),
        S,
        K("Show True P&eak Meter", false, Later("R16c")),
        K("&Show Labels", true, Later("R16c")),
        K("Hold Pea&ks", false, Later("R16c")),
        K("Hold &Valleys", false, Later("R16c")),
    ]),
    S,
    C("&Bypass All Audio FX", BypassAudioFx),
    S,
    C("&Mute All Audio", MuteAllAudio),
    C("Mute All Vi&deo", MuteAllVideo),
    S,
    Sub("Time&code", &[
        C("&Generate MIDI Timecode", MidiTimecode),
        C("Generate &MIDI Clock", MidiClock),
        C("&Trigger from MIDI Timecode", MidiTrigger),
    ]),
    K("External Control", false, NotPlanned("R59")),
    S,
    X("Customize &Toolbar...", Later("R15")),
    X("Customize Timeline Toolbar...", Later("R15")),
    X("Customize &Keyboard...", Later("R15")),
    C("&Preferences...", Preferences),
    V23(&X("Export Preferences...", Later("R49"))),
    V23(&X("Import Preferences...", Later("R49"))),
];

pub const HELP: &[M] = &[
    C("&Contents and Index", Help),
    C("What's &This?", WhatsThis),
    C("&Keyboard Shortcuts", KeyboardShortcuts),
    C("Unfinished &Features", UnfinishedFeatures),
    X("&Show Me How...", Later("R53")),
    V16(&X("Interactive Tutorial&s...", Later("R53"))),
    V16(&X("&Welcome...", Fixed("only in the VEGAS 23 look (File > Welcome Screen)"))),
    S,
    C("Check for &Updates...", CheckForUpdates),
    V23(&X("Download User Manual", Later("R53"))),
    S,
    C("About Milan Pro...", About),
];

/// The menu bar's menus, for the Unfinished Features list and the tests.
pub const MENUS: [(&str, &[M]); 7] =
    [("&File", FILE), ("&Edit", EDIT), ("&View", VIEW), ("&Insert", INSERT), ("&Tools", TOOLS), ("&Options", OPTIONS), ("&Help", HELP)];

/// The commands of the `C` rows of `items`, submenus and both looks included.
pub fn commands(items: &[M], out: &mut Vec<Command>) {
    for item in items {
        match item {
            C(_, c) => out.push(*c),
            Sub(_, items) | Inline(items) => commands(items, out),
            V16(i) | V23(i) => commands(std::slice::from_ref(*i), out),
            _ => {}
        }
    }
}

/// Every greyed row of `items` (submenus included, rows of both looks) as (label without '&', why), in menu order;
/// `C` rows whose command `unfinished` names. A label shown twice with the same reason is listed once.
pub fn greyed(items: &[M], out: &mut Vec<(String, Why)>) {
    for item in items {
        let row = match item {
            C(l, c) => unfinished(*c).map(|w| (*l, w)),
            X(l, w) | K(l, _, w) | R(l, _, w) => Some((*l, *w)),
            Sub(_, items) | Inline(items) => {
                greyed(items, out);
                None
            }
            V16(i) | V23(i) => {
                greyed(std::slice::from_ref(*i), out);
                None
            }
            S | Recent | UserLayouts | Streams => None,
        };
        if let Some((l, w)) = row {
            let l = keymap::mnemonic(l).0;
            if !out.iter().any(|(o, ow)| *o == l && *ow == w) {
                out.push((l, w));
            }
        }
    }
}

fn bar_id() -> Id {
    Id::new("milan_menu_bar")
}

/// A bar menu or a context menu is open (keys then go to the menu, not the keymap).
pub fn any_open(ctx: &Context) -> bool {
    BarState::load(ctx, bar_id()).is_some() || ctx.is_context_menu_open()
}

/// Open top-level menu `k` (0 = File) on the next frame, as if by keyboard.
pub fn request_open(ctx: &Context, k: usize) {
    ctx.data_mut(|d| d.insert_temp(Id::new("milan_menu_request"), k));
}

/// The menu bar. Alt+letter opens a menu, F10 opens File, Left/Right move between open menus.
pub fn draw_menu_bar(ui: &mut Ui) {
    let t = theme::get();
    let ctx = ui.ctx().clone();
    let mut bar = BarState::load(&ctx, bar_id());
    let open_idx = bar.as_ref().and_then(|r| MENUS.iter().position(|(l, _)| bar_id().with(*l) == r.id));
    let mut want: Option<usize> = None;
    let kb = ctx.data(|d| d.get_temp::<bool>(kb_id())).unwrap_or(false);
    // No other Context calls inside: the input lock is the context's lock.
    ctx.input_mut(|i| {
        let alt_only = i.modifiers.alt && !i.modifiers.ctrl && !i.modifiers.shift;
        for (k, (label, _)) in MENUS.iter().enumerate() {
            if alt_only && keymap::mnemonic_key(label).is_some_and(|key| i.consume_key(Modifiers::ALT, key)) {
                want = Some(k);
            }
        }
        if i.consume_key(Modifiers::NONE, Key::F10) {
            want = Some(0);
        }
        if let Some(k) = open_idx.filter(|_| kb) {
            if i.consume_key(Modifiers::NONE, Key::ArrowRight) {
                want = Some((k + 1) % MENUS.len());
            } else if i.consume_key(Modifiers::NONE, Key::ArrowLeft) {
                want = Some((k + MENUS.len() - 1) % MENUS.len());
            }
        }
    });
    if let Some(k) = ctx.data_mut(|d| d.remove_temp::<usize>(Id::new("milan_menu_request"))) {
        want = Some(k.min(MENUS.len() - 1));
    }
    if want.is_some() {
        ctx.data_mut(|d| d.insert_temp(kb_id(), true));
    }

    egui::menu::bar(ui, |ui| {
        // Bar labels: native white row in VP16, dark title-bar row in VP23 (§5.1).
        let v = ui.visuals_mut();
        v.override_text_color = Some(t.menu_text);
        v.widgets.hovered.weak_bg_fill = t.menu_bar_hover;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, t.menu_bar_hover_border);
        v.widgets.open.weak_bg_fill = t.menu_bar_open;
        v.widgets.active.weak_bg_fill = t.menu_bar_open;
        // VP23 labels sit ~2 px further apart (light2026: File 39, Edit 73, View 109, Insert 150).
        ui.spacing_mut().button_padding = vec2(if t.look() == UiLook::Vegas16 { 7.0 } else { 8.0 }, 1.0);
        ui.spacing_mut().item_spacing.x = 0.0;
        for (k, (label, items)) in MENUS.iter().enumerate() {
            let id = bar_id().with(*label);
            let mut b = Button::new(label_job(ui, label, t.menu_text, 0.0));
            if bar.as_ref().is_some_and(|r| r.id == id) {
                b = b.fill(t.menu_bar_open).stroke(Stroke::new(1.0_f32, t.menu_bar_hover_border));
            }
            let mut resp = ui.add(b);
            resp.id = id; // a fixed id, so Alt+letter can open the menu and `draw_menu_bar` can tell which is open
            if want == Some(k) {
                **bar = Some(MenuRoot::new(resp.rect.left_bottom(), id));
            }
            bar.bar_menu(&resp, |ui| {
                ui.set_min_width(220.0);
                show(ui, items);
            });
        }
    });
    if bar.is_none() {
        ctx.data_mut(|d| d.remove::<bool>(kb_id()));
    }
    bar.store(&ctx, bar_id());
}

/// Show `add` as `resp`'s right-click menu in the look's popup style (native light in VP16).
pub fn context(resp: &Response, add: impl FnOnce(&mut Ui)) {
    let t = theme::get();
    chrome::with_menu_style(&resp.ctx, &t, || {
        resp.context_menu(|ui| {
            ui.set_min_width(200.0);
            add(ui)
        })
    });
}

/// Open the context menu owned by `owner` at `pos` (for right-clicks a child widget swallowed), and show it.
pub fn context_at(owner: &Response, open_at: Option<Pos2>, add: impl FnOnce(&mut Ui)) {
    let ctx = &owner.ctx;
    let id = Id::new("__egui::context_menu");
    let mut bs = BarState::load(ctx, id);
    if let Some(p) = open_at {
        **bs = Some(MenuRoot::new(p, owner.id));
    } else if let Some(root) = bs.as_ref().filter(|r| r.id == owner.id) {
        // Same outside-click rule as egui's own context menus.
        let outside = ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !root.menu_state.read().area_contains(p)));
        if outside {
            **bs = None;
        }
    }
    let t = theme::get();
    chrome::with_menu_style(ctx, &t, || {
        bs.show(owner, |ui| {
            ui.set_min_width(200.0);
            add(ui)
        })
    });
    bs.store(ctx, id);
}

/// Save to the current path, or Save As if the project has none. Ok(false): Save As was cancelled.
pub fn save(project: &mut Project) -> Result<bool, String> {
    match project.path.clone() {
        Some(path) => save_to(project, &path),
        None => save_as(project),
    }
}

pub fn save_as(project: &mut Project) -> Result<bool, String> {
    let picked = rfd::FileDialog::new().add_filter("Milan Pro Project", &["mnle"]).set_file_name(format!("{}.mnle", project.name)).save_file();
    picked.map_or(Ok(false), |path| save_to(project, &path))
}

fn save_to(project: &mut Project, path: &std::path::PathBuf) -> Result<bool, String> {
    project.save(path).map(|_| true).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn add_marker(project: &mut Project, history: &mut History) {
    history.push(project.clone());
    let c = theme::get().marker_flag;
    project.timeline.markers.push(crate::project::Marker {
        position: project.timeline.cursor_position,
        name: format!("Marker {}", project.timeline.markers.len() + 1),
        color: [c.r(), c.g(), c.b()],
    });
    project.mark_modified();
}

/// Pick files and probe them in the background; the app adds the results when they arrive.
pub fn import_media(ctx: &Context, importer: &Importer) {
    use crate::media::{AUDIO_EXTS, IMAGE_EXTS, VIDEO_EXTS};
    if let Some(paths) = rfd::FileDialog::new()
        .add_filter("All Media", &[VIDEO_EXTS, AUDIO_EXTS, IMAGE_EXTS].concat())
        .add_filter("Video", VIDEO_EXTS)
        .add_filter("Audio", AUDIO_EXTS)
        .add_filter("Images", IMAGE_EXTS)
        .add_filter("All files", &["*"])
        .pick_files()
    {
        importer.import(ctx, paths);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_ripple_and_tool_items_follow_the_state() {
        let mut st = CmdState { n_sel: 2, ..Default::default() };
        assert!(st.enabled(Group) && !st.enabled(RemoveFromGroup) && !st.enabled(SelectGroup));
        st.in_group = true;
        assert!(st.enabled(RemoveFromGroup) && st.enabled(ClearGroup));
        assert!(!st.enabled(RippleAffected) && !st.enabled(RippleAll));
        st.last_edit = true;
        assert!(st.enabled(RippleAffected) && st.enabled(RippleBus));
        assert_eq!(st.checked(AutoRipple), Some(false));
        st.auto_ripple = true;
        assert_eq!(st.checked(AutoRipple), Some(true));
        assert_eq!(st.checked(RippleMode(RippleMode::Affected)), Some(true));
        assert_eq!(st.checked(NormalTool), Some(true));
        st.tool = EditTool::Selection;
        assert_eq!((st.checked(NormalTool), st.checked(Tool(EditTool::Selection))), (Some(false), Some(true)));
        assert!(st.enabled(Tool(EditTool::Envelope)) && st.enabled(Tool(EditTool::Slip)) && st.enabled(Tool(EditTool::Stretch)));
    }

    #[test]
    fn event_switches_follow_the_focus_event() {
        let mut st = CmdState::default();
        assert!(!st.enabled(EventReverse) && !st.enabled(EventMute));
        st.clip = true;
        assert!(st.enabled(EventReverse) && st.enabled(EventMute));
        assert_eq!(st.checked(EventMute), Some(false));
        st.clip_muted = true;
        assert_eq!((st.checked(EventMute), st.checked(EventReverse)), (Some(true), None));
        // Take items: Rename and Properties need an event, the rest an event with takes.
        assert!(st.enabled(RenameEvent) && st.enabled(EventProperties) && !st.enabled(NextTake) && !st.enabled(DeleteActiveTake) && !st.enabled(DeleteTake));
        st.takes = true;
        assert!(st.enabled(NextTake) && st.enabled(PrevTake) && st.enabled(ChooseActiveTake) && st.enabled(DeleteActiveTake) && st.enabled(DeleteTake));
        // Pan/Crop and Event FX need a video event, Track Motion and Track FX a video track; Video Output FX nothing.
        assert!(!st.enabled(EventPanCrop) && !st.enabled(TrackMotion) && !st.enabled(VideoEventFx) && !st.enabled(TrackFx));
        assert!(st.enabled(VideoOutputFx));
        (st.video_clip, st.video_track_focus) = (true, true);
        assert!(st.enabled(EventPanCrop) && st.enabled(TrackMotion) && st.enabled(VideoEventFx) && st.enabled(TrackFx));
        // Audio Event FX needs an audio event (also beside a video one: a grouped pair); Track FX and the default
        // chain an audio track.
        assert!(!st.enabled(AudioEventFx) && !st.enabled(AudioTrackFx) && !st.enabled(DefaultTrackFx));
        (st.audio_clip, st.audio_track_focus) = (true, true);
        assert!(st.enabled(AudioEventFx) && st.enabled(AudioTrackFx) && st.enabled(DefaultTrackFx));
        assert_eq!(st.checked(BypassAudioFx), Some(false));
        st.bypass_audio_fx = true;
        assert!(st.enabled(BypassAudioFx) && st.checked(BypassAudioFx) == Some(true));
    }

    /// R1: the snapping switches and the Grid Spacing / Ruler Format radios follow the state.
    #[test]
    fn grid_and_ruler_items_follow_the_state() {
        let mut st = CmdState::default();
        assert_eq!((st.checked(SnapGrid), st.checked(SnapAllEvents)), (Some(false), Some(false)));
        assert_eq!(st.checked(SetGrid(GridSpacing::RulerMarks)), Some(true));
        assert_eq!(st.checked(SetRuler(RulerFormat::TimeFrames)), Some(true));
        (st.snap_grid, st.grid, st.ruler) = (true, GridSpacing::Note(8, true), RulerFormat::MeasuresBeats);
        assert_eq!((st.checked(SnapGrid), st.checked(SetGrid(GridSpacing::Note(8, true))), st.checked(SetGrid(GridSpacing::Note(8, false)))), (Some(true), Some(true), Some(false)));
        assert_eq!((st.checked(SetRuler(RulerFormat::MeasuresBeats)), st.checked(SetRuler(RulerFormat::TimeFrames))), (Some(true), Some(false)));
        for c in [GoTo, GridLeft, GridRight, SnapGrid, SnapAllEvents, SetTimeAtCursor, ProjectTempo, SetGrid(GridSpacing::Frames)] {
            assert!(st.enabled(c) && unfinished(c).is_none(), "{c:?}");
        }
        let mut cmds = Vec::new();
        commands(GRID_SPACING, &mut cmds);
        commands(RULER_FORMATS, &mut cmds);
        assert_eq!(cmds.len(), 14 + 15);
    }

    /// R2: the navigation, selection, nudge and zoom commands work; the ones that act on events, tracks or the clipboard
    /// wait for them, and Exit Edit Mode for edge-trimming mode.
    #[test]
    fn navigation_selection_nudge_and_zoom_commands_work() {
        let r2 = [
            CursorToPlayhead, JogLeft, JogRight, SelectToNextMarker, SelectLoopRegion, RestoreSelection, SelPrevEvent, SelNextEvent,
            SelectEventStart, SelectEventEnd, ExpandedEdit, ExitEditMode, PasteRepeat, MuteExclusive, SoloExclusive, ToggleListFocus,
            ZoomFramePerThumb, WaveZoomIn, WaveZoomOut, PreviewAtCursor, NudgeFrameLeft, NudgeFrameRight, NudgePixelLeft,
            NudgePixelRight, NudgeTrackUp, NudgeTrackDown, SelectAfterCursor, SelectEventsToEnd, MuteAllAudio, MuteAllVideo,
        ];
        let mut st = CmdState { any_tracks: true, ..Default::default() };
        for c in r2 {
            assert!(unfinished(c).is_none(), "{c:?}");
        }
        for c in [NudgeFrameLeft, SelNextEvent, SelectEventStart, ExpandedEdit, SelectEventsToEnd, PasteRepeat, MuteExclusive, ExitEditMode] {
            assert!(!st.enabled(c), "{c:?} needs a selection");
        }
        (st.n_sel, st.clip, st.clipboard, st.tracks_sel, st.edge_mode) = (1, true, true, true, true);
        assert!(r2.iter().all(|c| st.enabled(*c)));
        assert_eq!((st.checked(MuteAllAudio), st.checked(MuteAllVideo)), (Some(false), Some(false)));
        st.mute_video = true;
        assert_eq!(st.checked(MuteAllVideo), Some(true));
        let mut rows = Vec::new();
        for (_, items) in MENUS.iter().chain(crate::ui::timeline::CONTEXT_MENUS) {
            greyed(items, &mut rows);
        }
        assert!(!rows.iter().any(|(_, w)| *w == Later("R2")), "{rows:?}");
    }

    /// R5: Mute / Solo Track Group work once a selected track is in a group; nothing in the menus waits on R5 any more.
    #[test]
    fn track_group_commands_work() {
        let mut st = CmdState { tracks_sel: true, ..Default::default() };
        for c in [GroupMute, GroupSolo] {
            assert!(unfinished(c).is_none() && !st.enabled(c), "{c:?}");
        }
        st.track_group = true;
        assert!(st.enabled(GroupMute) && st.enabled(GroupSolo));
        let mut rows = Vec::new();
        for (_, items) in MENUS.iter().chain(crate::ui::timeline::CONTEXT_MENUS) {
            greyed(items, &mut rows);
        }
        assert!(!rows.iter().any(|(_, w)| *w == Later("R5")), "{rows:?}");
    }

    /// R8a: Insert › Audio Bus and Audio Assignable FX work; Remove Assignable FX once there is a chain; nothing in the
    /// menus waits on R8a any more (Audio Input Bus is input monitoring: R9).
    #[test]
    fn bus_commands_work() {
        let mut st = CmdState::default();
        for c in [InsertAudioBus, InsertAssignableFx, RemoveAssignableFx] {
            assert!(unfinished(c).is_none(), "{c:?}");
        }
        assert!(st.enabled(InsertAudioBus) && st.enabled(InsertAssignableFx) && !st.enabled(RemoveAssignableFx));
        st.assignable_fx = true;
        assert!(st.enabled(RemoveAssignableFx));
        let mut rows = Vec::new();
        for (_, items) in MENUS.iter().chain(crate::ui::timeline::CONTEXT_MENUS) {
            greyed(items, &mut rows);
        }
        assert!(!rows.iter().any(|(_, w)| *w == Later("R8a")), "{rows:?}");
    }

    /// R9: Insert › Audio Input Bus works, and nothing in the menus waits on R9 any more.
    #[test]
    fn input_bus_command_works() {
        assert!(unfinished(InsertInputBus).is_none() && CmdState::default().enabled(InsertInputBus));
        let mut rows = Vec::new();
        for (_, items) in MENUS.iter().chain(crate::ui::timeline::CONTEXT_MENUS) {
            greyed(items, &mut rows);
        }
        assert!(!rows.iter().any(|(_, w)| *w == Later("R9")), "{rows:?}");
    }

    /// R8b: the Mixing Console, Audio Bus Tracks, Dim master, Audio Properties (Master Bus Properties) and the fourth
    /// ripple mode work; nothing in the menus waits on R8b any more.
    #[test]
    fn mixer_bus_track_and_dim_commands_work() {
        let mut st = CmdState { look: UiLook::Vegas16, ..Default::default() };
        for c in [Window(Panel::MixingConsole), BusTracks, DimMaster, AudioProperties, RippleMode(RippleMode::AffectedMarkersOnly), OutputMeters, VerticalMeters, MeterWidth(2)] {
            assert!(unfinished(c).is_none() && st.enabled(c), "{c:?}");
        }
        assert!(unfinished(RippleMarkers).is_none() && !st.enabled(RippleMarkers));
        st.last_edit = true;
        assert!(st.enabled(RippleMarkers));
        assert_eq!((st.checked(BusTracks), st.checked(DimMaster), st.checked(Window(Panel::MixingConsole))), (Some(false), Some(false), Some(false)));
        assert_eq!((st.checked(OutputMeters), st.checked(MeterWidth(0)), st.checked(MeterWidth(1))), (Some(false), Some(true), Some(false)));
        (st.bus_tracks, st.dim, st.open_panels) = (true, true, vec![Panel::MixingConsole]);
        assert_eq!((st.checked(BusTracks), st.checked(DimMaster), st.checked(Window(Panel::MixingConsole))), (Some(true), Some(true), Some(true)));
        let mut rows = Vec::new();
        for (_, items) in MENUS.iter().chain(crate::ui::timeline::CONTEXT_MENUS) {
            greyed(items, &mut rows);
        }
        assert!(!rows.iter().any(|(_, w)| *w == Later("R8b")), "{rows:?}");
    }

    /// R3a: the Loop switch, empty events, subclips, attribute paste and the group commands work, each once there is
    /// something to act on; nothing in the menus waits on R3a any more.
    #[test]
    fn loop_empty_subclip_attribute_and_group_commands_work() {
        let r3a = [EventLoop, InsertEmptyEvent, CreateSubclip, PasteAttributes, PasteAttributesSelect, GroupCut, GroupCopy, GroupDelete];
        let mut st = CmdState::default();
        for c in r3a {
            assert!(unfinished(c).is_none() && !st.enabled(c), "{c:?}");
        }
        st.clip = true;
        assert!(st.enabled(EventLoop) && st.enabled(CreateSubclip) && !st.enabled(PasteAttributes));
        assert_eq!(st.checked(EventLoop), Some(false));
        st.clip_looping = true;
        assert_eq!(st.checked(EventLoop), Some(true));
        (st.clipboard, st.n_sel, st.selection, st.in_group) = (true, 1, true, true);
        assert!(r3a.iter().all(|c| st.enabled(*c)));
        let mut rows = Vec::new();
        for (_, items) in MENUS.iter().chain(crate::ui::timeline::CONTEXT_MENUS) {
            greyed(items, &mut rows);
        }
        assert!(!rows.iter().any(|(_, w)| *w == Later("R3a")), "{rows:?}");
    }

    /// R3b: the audio and video event switches, resample radios, Channels and Stream follow the focus events and wait
    /// for an event of their kind; Quickfade is an option; nothing in the menus waits on R3b any more.
    #[test]
    fn event_switches_channels_streams_and_quickfade_work() {
        let r3b = [
            EventSwitch(Switch::Invert),
            EventSwitch(Switch::Normalize),
            EventSwitch(Switch::AutoNormalize),
            EventSwitch(Switch::KeepAspect),
            EventSwitch(Switch::ReduceFlicker),
            SetResample(None),
            SetResample(Some(Resample::OpticalFlow)),
            SetChannels(ChannelMap::Swap),
            SetStream(0),
            Quickfade,
        ];
        let mut st = CmdState::default();
        for c in r3b {
            assert!(unfinished(c).is_none(), "{c:?}");
            assert_eq!(st.enabled(c), c == Quickfade, "{c:?}");
        }
        st.audio_clip = true;
        assert!(st.enabled(EventSwitch(Switch::Normalize)) && st.enabled(SetChannels(ChannelMap::Both)) && !st.enabled(EventSwitch(Switch::KeepAspect)));
        (st.video_clip, st.clip, st.streams) = (true, true, 2);
        assert!(r3b.iter().all(|c| st.enabled(*c)) && !st.enabled(SetStream(2)));
        (st.switches, st.resample, st.channels, st.stream, st.quickfade) = (vec![Switch::KeepAspect], Some(Resample::Force), ChannelMap::LeftOnly, 1, true);
        assert_eq!((st.checked(EventSwitch(Switch::KeepAspect)), st.checked(EventSwitch(Switch::Invert))), (Some(true), Some(false)));
        assert_eq!((st.checked(SetResample(None)), st.checked(SetResample(Some(Resample::Force)))), (Some(false), Some(true)));
        assert_eq!((st.checked(SetChannels(ChannelMap::LeftOnly)), st.checked(SetStream(1)), st.checked(Quickfade)), (Some(true), Some(true), Some(true)));
        // R4b: Audio Follows Video Velocity is an option, off until the project turns it on.
        assert!(st.enabled(AudioFollowsVelocity) && st.checked(AudioFollowsVelocity) == Some(false));
        st.audio_follows_velocity = true;
        assert_eq!(st.checked(AudioFollowsVelocity), Some(true));
        let mut rows = Vec::new();
        for (_, items) in MENUS.iter().chain(crate::ui::timeline::CONTEXT_MENUS) {
            greyed(items, &mut rows);
        }
        assert!(!rows.iter().any(|(_, w)| *w == Later("R3b")), "{rows:?}");
    }

    /// R4a: the video envelope rows, the event Velocity, Freeze Frame and Transition Progress items and the View ›
    /// Envelopes filters work; nothing in the menus waits on R4a any more.
    #[test]
    fn velocity_progress_fade_and_envelope_view_items_work() {
        let r4a = [ToggleVideoEnvelope(EnvKind::Mute), ToggleVideoEnvelope(EnvKind::FadeToColor), EventVelocity, FreezeFrame, TransitionProgress];
        let mut st = CmdState::default();
        for c in r4a {
            assert!(unfinished(c).is_none() && !st.enabled(c), "{c:?}");
        }
        assert!(st.enabled(ViewEnvelope(EnvView::Velocity)) && st.checked(ViewEnvelope(EnvView::Velocity)) == Some(true));
        st.hidden_envs = EnvView::bits(&[EnvView::Velocity, EnvView::AudioMute]);
        assert_eq!((st.checked(ViewEnvelope(EnvView::Velocity)), st.checked(ViewEnvelope(EnvView::VideoMute))), (Some(false), Some(true)));
        // Audio Mute and video Track Mute are told apart.
        (st.audio_track_focus, st.focus_envs) = (true, vec![EnvKind::Mute]);
        assert!(st.enabled(ToggleEnvelope(EnvKind::Mute)) && !st.enabled(ToggleVideoEnvelope(EnvKind::Mute)));
        assert_eq!((st.checked(ToggleEnvelope(EnvKind::Mute)), st.checked(ToggleVideoEnvelope(EnvKind::Mute))), (Some(true), Some(false)));
        (st.video_track_focus, st.video_clip, st.velocity) = (true, true, true);
        assert!(st.enabled(ToggleVideoEnvelope(EnvKind::FadeToColor)) && st.enabled(FreezeFrame) && !st.enabled(TransitionProgress));
        assert_eq!(st.checked(EventVelocity), Some(true));
        st.transition = true;
        assert!(st.enabled(TransitionProgress) && st.checked(TransitionProgress) == Some(false));
        let mut rows = Vec::new();
        for (_, items) in MENUS.iter().chain(crate::ui::timeline::CONTEXT_MENUS) {
            greyed(items, &mut rows);
        }
        assert!(!rows.iter().any(|(_, w)| *w == Later("R4a")), "{rows:?}");
    }

    #[test]
    fn record_and_arm_follow_the_armed_tracks() {
        let mut st = CmdState::default();
        assert!(!st.enabled(Record) && !st.enabled(Arm));
        st.audio_track_focus = true;
        assert!(st.enabled(Arm) && st.checked(Arm) == Some(false) && !st.enabled(Record));
        (st.armed, st.any_armed) = (true, true);
        assert!(st.enabled(Record) && st.checked(Arm) == Some(true));
        // While recording, Ctrl+R stops it and arming is fixed.
        (st.recording, st.any_armed) = (true, false);
        assert!(st.enabled(Record) && !st.enabled(Arm));
    }

    #[test]
    fn user_layout_slots_follow_the_saved_ones() {
        let mut st = CmdState::default();
        assert!(!st.enabled(LayoutUser(3)) && st.enabled(SaveLayoutUser(3)) && st.enabled(SaveLayout) && st.enabled(SaveLayoutAs));
        st.layouts[3] = Some("Edit".into());
        assert!(st.enabled(LayoutUser(3)) && !st.enabled(LayoutUser(4)) && !st.enabled(LayoutUser(10)));
        // The bullet follows the current layout: Default until a slot is loaded or saved.
        assert_eq!((st.checked(LayoutDefault), st.checked(LayoutUser(3))), (Some(true), Some(false)));
        st.layout_slot = Some(3);
        assert_eq!((st.checked(LayoutDefault), st.checked(LayoutUser(3)), st.checked(LayoutUser(4))), (Some(false), Some(true), Some(false)));
        assert!(!st.enabled(Window(Panel::ColorGrading)) && !st.enabled(LayoutTimelineTop));
    }

    /// R0: a command that stays greyed out with everything selected says why, and a command labelled unfinished is
    /// never live (the batch that builds it removes the label).
    #[test]
    fn every_greyed_command_says_why() {
        let everything = |look| CmdState {
            look,
            can_undo: true,
            can_redo: true,
            clip: true,
            video_clip: true,
            audio_clip: true,
            takes: true,
            n_sel: 2,
            any_tracks: true,
            tracks_sel: true,
            track_group: true,
            list_focus: true,
            clipboard: true,
            selection: true,
            in_group: true,
            last_edit: true,
            auto_ripple: true,
            audio_track_focus: true,
            video_track_focus: true,
            any_armed: true,
            edge_mode: true,
            transition: true,
            assignable_fx: true,
            recent: vec!["p.mnle".into(); 10],
            layouts: std::array::from_fn(|_| Some("L".into())),
            ..Default::default()
        };
        let mut cmds: Vec<Command> = keymap::TABLE.iter().map(|r| r.0).collect();
        for (_, items) in MENUS.iter().chain(crate::ui::timeline::CONTEXT_MENUS) {
            commands(items, &mut cmds);
        }
        assert!(cmds.len() > 250);
        for c in cmds {
            let live = [UiLook::Vegas16, UiLook::Vegas23].into_iter().any(|look| everything(look).enabled(c));
            assert_eq!(live, unfinished(c).is_none(), "{c:?}: live {live}, reason {:?}", unfinished(c));
        }
        // Every greyed menu row has a reason by construction (`X`, `K`, `R` carry one); the list has them all.
        let mut rows = Vec::new();
        greyed(MENUS[5].1, &mut rows);
        assert!(rows.iter().any(|(l, w)| l == "Metronome" && *w == Later("R18a")), "{rows:?}");
        // R1 built snapping, the grid and the ruler formats: nothing in the menus waits on it any more.
        assert!(!rows.iter().any(|(l, w)| l.contains("Grid") || l.contains("SMPTE") || *w == Later("R1")), "{rows:?}");
    }

    /// Every `Later` and `NotPlanned` in the source names a batch heading of docs/REMAINING.md (`NotPlanned` only the
    /// R57-R60 long tail), and `Fixed` texts are lowercase phrases.
    #[test]
    fn every_reason_names_a_batch_of_remaining_md() {
        let doc = include_str!("../../docs/REMAINING.md");
        let mut files = vec![std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/src"))];
        let mut found = 0;
        while let Some(p) = files.pop() {
            if p.is_dir() {
                files.extend(std::fs::read_dir(&p).unwrap().map(|e| e.unwrap().path()));
                continue;
            }
            if p.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let src = std::fs::read_to_string(&p).unwrap();
            for kind in ["Later", "NotPlanned", "Fixed"] {
                let open = format!("{kind}(\"");
                for (at, _) in src.match_indices(&open) {
                    let arg = &src[at + open.len()..];
                    let arg = &arg[..arg.find('"').unwrap()];
                    let long_tail = ["R57", "R58", "R59", "R60"].contains(&arg);
                    match kind {
                        "Later" => assert!(!long_tail && doc.contains(&format!("\n### {arg}:")), "{}: Later({arg})", p.display()),
                        "NotPlanned" => assert!(long_tail, "{}: NotPlanned({arg})", p.display()),
                        _ => assert!(arg.starts_with(|c: char| c.is_ascii_lowercase()), "{}: Fixed({arg})", p.display()),
                    }
                    found += 1;
                }
            }
        }
        assert!(found > 200, "{found}");
    }

    /// A save that cannot write (missing or read-only folder) comes back as an error for the status bar, and the
    /// project stays modified so the save prompt keeps its New/Open/Exit from discarding it.
    #[test]
    fn failed_save_is_an_error() {
        let mut p = crate::project::Project::new("t");
        p.mark_modified();
        p.path = Some(std::env::temp_dir().join("milan-no-such-dir-7f3a").join("p.mnle"));
        let e = super::save(&mut p).unwrap_err();
        assert!(e.contains("p.mnle") && p.modified, "{e}");
    }
}
