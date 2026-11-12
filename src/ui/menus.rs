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
use crate::edit::RippleMode;
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
    About,
}

impl From<MenuAction> for Command {
    fn from(a: MenuAction) -> Self {
        match a {
            MenuAction::RenderAs => RenderAs,
            MenuAction::Properties => Properties,
            MenuAction::New => NewEmpty,
            MenuAction::Open => Open,
            MenuAction::Exit => Exit,
            MenuAction::About => About,
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
    /// The focus event's Switches › Mute.
    pub clip_muted: bool,
    /// The focus event is a video event (Pan/Crop).
    pub video_clip: bool,
    /// A selected event has inactive takes.
    pub takes: bool,
    pub n_sel: usize,
    /// Tracks exist / are selected; the track list had the last click.
    pub any_tracks: bool,
    pub tracks_sel: bool,
    pub list_focus: bool,
    pub quantize: bool,
    /// An event is on the clipboard.
    pub clipboard: bool,
    /// A time selection exists.
    pub selection: bool,
    pub looping: bool,
    pub snapping: bool,
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
    /// Names of the saved user layouts 0-9 of this look (None = <EMPTY>).
    pub layouts: [Option<String>; 10],
    /// The user layout last loaded or saved (Save Layout saves over it); None = the default layout.
    pub layout_slot: Option<usize>,
}

impl CmdState {
    pub fn enabled(&self, c: Command) -> bool {
        let v23 = self.look == UiLook::Vegas23;
        match c {
            New | NewEmpty | Open | Close | Save | SaveAs | ImportMedia | Properties | Exit | RenderAs | Preferences
            | About | KeyboardShortcuts | CheckForUpdates | NormalTool | LayoutDefault | SaveLayout | SaveLayoutAs
            | SaveLayoutUser(_) => true,
            LayoutUser(n) => self.layouts.get(n).is_some_and(Option::is_some),
            Welcome => v23,
            OpenRecent(i) => i < self.recent.len(),
            Undo | UndoAll => self.can_undo,
            Redo => self.can_redo,
            ClearHistory => self.can_undo || self.can_redo,
            TrimStart | TrimEnd | EventLock | SelEventStart | SelEventEnd | EventOpenInTrimmer | EventSelectInMedia
            | EventProperties | EventReverse | EventMute | RenameEvent => self.clip,
            NextTake | PrevTake | ChooseActiveTake | DeleteActiveTake | DeleteTake => self.takes,
            // With nothing selected, Split splits every event under the cursor.
            Split | SelectAll | SelectNone | Quantize | MinimizeTracks => true,
            // With Auto Ripple, Delete with only a time selection deletes that time.
            Delete => self.env_sel || self.n_sel > 0 || (self.list_focus && self.tracks_sel) || (self.auto_ripple && self.selection),
            ShowEnvelopes | LockEnvelopes => true,
            VolumeEnvelope | PanEnvelope | RemoveVolumeEnvelope | RemovePanEnvelope => self.audio_track_focus,
            ToggleEnvelope(k) => if crate::edit::env_fits(k, true) { self.video_track_focus } else { self.audio_track_focus },
            EventPanCrop => self.video_clip,
            TrackMotion => self.video_track_focus,
            Trim => self.clip && self.selection,
            Copy => self.n_sel > 0,
            Cut => self.n_sel > 0 && !self.clip_locked,
            Paste | PasteInsert => self.clipboard,
            NextTool | PrevTool | AutoRipple | RippleMode(_) | IgnoreGrouping | Tool(_) => true,
            Group => self.n_sel >= 2,
            RemoveFromGroup | ClearGroup | SelectGroup => self.in_group,
            RippleAffected | RippleBus | RippleAll => self.last_edit,
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
            Window(p) => p.in_look(self.look),
            _ => false,
        }
    }

    /// Some(on) for check and radio items.
    pub fn checked(&self, c: Command) -> Option<bool> {
        Some(match c {
            Loop => self.looping,
            Snapping => self.snapping,
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
            SpacePlayPause => self.space_pauses,
            Quantize => self.quantize,
            ShowEnvelopes => self.show_env,
            LockEnvelopes => self.lock_env,
            ToggleEnvelope(k) => self.focus_envs.contains(&k),
            LayoutUser(n) => self.layout_slot == Some(n),
            LayoutDefault => self.layout_slot.is_none(),
            // Always-on behaviour Milan Pro has no switch for.
            SnapMarkers | Waveforms | EventButtons | FadeLengths | EdgeFrames | TakeInfo => true,
            EventHeaders => self.look == UiLook::Vegas16,
            SnapGrid | SnapAllEvents | BusTracks | VideoBusTrack | MediaMarkers | Multicam
            | ExternalMonitor => false,
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
    let (text, short) = row_colors(enabled);
    let mut b = Button::new(label_job(ui, label, text, 26.0)).shortcut_text(RichText::new(shortcut).color(short));
    if !enabled {
        // Same padding as live rows, no hover box.
        b = b.sense(Sense::hover()).fill(Color32::TRANSPARENT).stroke(Stroke::NONE);
    }
    let r = ui.add(b);
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
    let radio = matches!(c, PreviewQuality(_) | NormalTool | Tool(_) | RippleMode(_) | LayoutUser(_) | LayoutDefault);
    let mark = match st.checked(c) {
        Some(on) if radio => Mark::Radio(on),
        Some(on) => Mark::Check(on),
        None => Mark::None,
    };
    if entry(ui, label, &keymap::shortcut_text(c, st.look), st.enabled(c), mark) {
        send(ui.ctx(), c);
    }
}

/// Greyed-out row for something Milan Pro does not have.
pub fn off(ui: &mut Ui, label: &str) {
    entry(ui, label, "", false, Mark::None);
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
    /// Not available: greyed out.
    X(&'static str),
    /// Not available, shown with a fixed check / radio state.
    K(&'static str, bool),
    R(&'static str, bool),
    S,
    Sub(&'static str, &'static [M]),
    /// Items shown in place (shared blocks).
    Inline(&'static [M]),
    V16(&'static M),
    V23(&'static M),
    Recent,
    /// View › Window Layouts: user slots 0-9 with their names.
    UserLayouts,
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
        X(l) => off(ui, l),
        K(l, on) => {
            entry(ui, l, "", false, Mark::Check(*on));
        }
        R(l, on) => {
            entry(ui, l, "", false, Mark::Radio(*on));
        }
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
                off(ui, "Recent projects");
            }
        }
        UserLayouts => {
            let st = state(ui.ctx());
            for (n, name) in st.layouts.iter().enumerate() {
                cmd(ui, &format!("&{n} {}", name.as_ref().map_or("<EMPTY>".into(), |s| s.replace('&', "&&"))), LayoutUser(n));
            }
        }
    }
}

const IMPORT: &[M] = &[
    C("&Media...", ImportMedia),
    X("Media from &Project..."),
    V16(&X("&AAF...")),
    X("P&remiere/After Effects (*.prproj)..."),
    X("&Final Cut Pro 7/DaVinci Resolve (*.xml)..."),
    X("Final Cut Pro &X (*.fcpxml)..."),
    X("&EDL Text File (*.txt)..."),
    X("&Broadcast Wave Format..."),
    X("&Closed Captioning..."),
];

const EXPORT: &[M] = &[
    X("&Milan Pro Project Archive..."),
    X("Premiere/&After Effects (*.prproj)..."),
    X("&Final Cut Pro 7/DaVinci Resolve (*.xml)..."),
    X("Final Cut Pro &X (*.fcpxml)..."),
    X("&EDL Text File (*.txt)..."),
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
    X("Real-Ti&me Render..."),
    S,
    Sub("&Import", IMPORT),
    Sub("E&xport", EXPORT),
    V16(&X("Capture &Video...")),
    X("&Extract Audio from CD..."),
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
    X("Affected Tracks, &Markers, and Regions"),
    C("&All Tracks, Markers, and Regions", RippleAll),
];

const SELECT: &[M] = &[
    C("Select &All", SelectAll),
    V23(&X("Select All After Cursor")),
    C("Select Event Sta&rt", SelectEventStart),
    C("Select Event &End", SelectEventEnd),
    C("Enter Expanded Edit &Mode", ExpandedEdit),
    V16(&X("Exit E&dit Mode")),
    V23(&X("Exit Expanded E&dit Mode")),
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
    R("S&huffle Tool", false),
    C("Sl&ip Tool", Tool(EditTool::Slip)),
    C("Sli&de Tool", Tool(EditTool::Slide)),
    C("Time Stretch/&Compress Tool", Tool(EditTool::Stretch)),
    R("Split &Trim Tool", false),
    S,
    C("&Envelope", Tool(EditTool::Envelope)),
    C("&Selection", Tool(EditTool::Selection)),
    R("&Zoom", false),
    S,
    C("Ne&xt Tool", NextTool),
    C("&Previous Tool", PrevTool),
];

const SWITCHES: &[M] = &[
    C("&Mute", EventMute),
    C("&Lock", EventLock),
    K("L&oop", true),
    S,
    K("&Invert Phase", false),
    K("&Normalize", false),
    V23(&K("Auto Normalize", false)),
    S,
    K("Maintain &Aspect Ratio", true),
    K("Reduce Interlace &Flicker", false),
    S,
    R("Use P&roject Resample Mode", true),
    R("Force Re&sample", false),
    R("&Disable Resample", false),
    V23(&R("Optical Flow", false)),
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

const GROUP: &[M] = &[
    C("Create &New", Group),
    S,
    C("&Remove From", RemoveFromGroup),
    C("&Clear", ClearGroup),
    C("&Select All", SelectGroup),
    S,
    X("Cut &All"),
    X("Cop&y All"),
    X("&Delete All"),
];

const CHANNELS: &[M] = &[R("&Both", true), R("&Left Only", false), R("&Right Only", false), R("&Combine", false), R("&Swap", false)];

pub const EDIT: &[M] = &[
    C("&Undo", Undo),
    C("&Redo", Redo),
    S,
    C("Cu&t", Cut),
    C("&Copy", Copy),
    C("&Paste", Paste),
    C("P&aste Repeat...", PasteRepeat),
    C("Pa&ste Insert", PasteInsert),
    X("Paste E&vent Attributes"),
    X("Selectively Paste Event Attributes..."),
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
    Sub("&Stream", &[X("Stream &1")]),
    Sub("C&hannels", CHANNELS),
    S,
    C("U&ndo All", UndoAll),
    C("Clear Edit Histor&y...", ClearHistory),
];

const LAYOUTS: &[M] = &[
    X("&Organize Layouts..."),
    C("&Save Layout", SaveLayout),
    C("Save Layout &As...", SaveLayoutAs),
    X("R&eload Selected Layout"),
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
    X("Compositors"),
    C("&Plug-In Manager", PluginManager),
    C("Video &Scopes", VideoScopes),
    C("S&urround Panner", SurroundPanner),
    C("Mixing Console", MixingConsole),
    C("Device Exp&lorer", DeviceExplorer),
    C("Loudness Meters", LoudnessMeters),
    V23(&C("Dashboard", Dashboard)),
    V23(&C("Color Grading", Window(Panel::ColorGrading))),
    S,
    // Milan Pro's own window (not in VEGAS).
    C("Pr&operties", Window(Panel::Properties)),
];

const AUDIO_ENV_VIEW: &[M] = &[K("Track &Mute", true), K("Track &Volume", true), K("Track &Pan", true)];

const VIDEO_ENV_VIEW: &[M] = &[
    K("Track &Mute", true),
    K("Track &Composite Level", true),
    K("Track &Fade to Color", true),
    K("Track &Keyframes", true),
    S,
    K("Event &Velocity", true),
    K("Transition &Progress", true),
    S,
    K("Motion &Blur Amount", true),
    K("Video &Supersampling", true),
];

pub const VIEW: &[M] = &[
    K("&Toolbar", true),
    K("Status &Bar", true),
    S,
    Sub("Window La&youts", LAYOUTS),
    S,
    C("&Focus to Timeline", FocusTimeline),
    Sub("Window", WINDOWS),
    S,
    K("Mixer Previe&w Fader", false),
    S,
    C("Audio Bus Tra&cks", BusTracks),
    C("Vide&o Bus Track", VideoBusTrack),
    S,
    Sub("Eve&nt Media Markers", &[C("&Show Markers", MediaMarkers), K("Show Marker &Labels", false)]),
    C("Active Take &Information", TakeInfo),
    C("Audio Event &Waveforms", Waveforms),
    C("Event &Buttons", EventButtons),
    C("Event Fade &Lengths", FadeLengths),
    C("Video &Event Edge Edit Frames", EdgeFrames),
    C("Event Headers", EventHeaders),
    V23(&K("Event Length", false)),
    V23(&K("Show OFX Effect Envelopes (Beta)", false)),
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
        X("Track &Mute"),
        C("Track &Composite Level", ToggleEnvelope(EnvKind::CompositeLevel)),
        X("Track &Fade to Color"),
        S,
        X("Event &Velocity"),
        X("Transition &Progress"),
    ]),
    S,
    C("&Audio Track", InsertAudioTrack),
    C("&Video Track", InsertVideoTrack),
    V23(&C("Adjustment Track", InsertAdjustmentTrack)),
    S,
    X("Audio &Bus"),
    X("Audio In&put Bus"),
    X("Audio Assignable &FX..."),
    S,
    X("&Empty Event"),
    V23(&X("Adjustment Event")),
    C("&Generated Media...", Window(Panel::MediaGenerators)),
    V23(&X("&Slideshow...")),
    V23(&X("Insert Subtitles from Files...")),
    V23(&X("Insert Subtitles from Regions...")),
    S,
    X("Audio Tra&nsition..."),
    S,
    X("&Time..."),
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
    X("Open &Copy in Audio Editor"),
    X("Edit Source Pro&ject"),
    Sub("&Audio", &[
        X("&Open in Audio Editor"),
        X("Open &Copy in Audio Editor"),
        X("Edit Source Pro&ject"),
        S,
        X("Audio &Event FX..."),
        X("Apply Non-Real-Time Event &FX..."),
        S,
        X("&Track FX..."),
        S,
        X("&Remove Assignable FX..."),
    ]),
    Sub("&Video", &[
        X("&Media FX..."),
        S,
        C("Video Event &Pan/Crop...", EventPanCrop),
        X("Video Event &FX..."),
        S,
        X("Video Track F&X..."),
        C("&Track Motion...", TrackMotion),
        S,
        X("Vi&deo Output FX..."),
        V23(&C("Motion Tracking", MotionTracking)),
    ]),
    S,
    C("&Render to New Track...", RenderToNewTrack),
    C("Preview on E&xternal Monitor", ExternalMonitor),
    C("&Selectively Prerender Video...", Prerender),
    X("Clean &Up Prerendered Video..."),
    C("Build Dynamic RAM Previe&w", RamPreview),
    V23(&X("Clear Dynamic RAM Preview")),
    S,
    X("&Lay Out Audio CD from Events"),
    Sub("&Burn Disc", &[X("&Track-at-Once Audio CD..."), X("&Disc-at-Once Audio CD..."), X("&DVD..."), X("&Blu-ray Disc...")]),
    X("Generate Loudness Log"),
    V23(&X("Advanced Save...")),
    V23(&X("Speech to Text...")),
    V23(&C("Text to Speech...", TextToSpeech)),
    S,
    Sub("Scrip&ting", &[X("Ru&n Script..."), X("Res&can Script Menu Folder")]),
    Sub("&Multicamera", &[
        X("Synchronize Audio to A&lign Events"),
        X("Synchronize Media &Timecode to Align Events"),
        X("Synchronize &Date/Time Stamp to Align Events"),
        X("&Create Multicamera Track"),
        X("E&xpand to Multiple Tracks..."),
        S,
        C("&Enable Multicamera Editing", Multicam),
        K("Edit Multicamera &Audio with Video", false),
        K("Show Full Frame in External &Monitor", false),
    ]),
    S,
    X("&Clean Project Media"),
    X("Rebuild Offline &Nested Audio"),
];

pub const GRID_SPACING: &[M] = &[
    R("&Ruler Marks", true),
    S,
    R("Mi&nutes", false),
    R("&Seconds", false),
    R("&Milliseconds", false),
    R("&Frames", false),
    S,
    R("Me&asures", false),
    R("&Quarter Notes", false),
    R("Q&uarter Note Triplets", false),
    R("&8th Notes", false),
    R("8th Note &Triplets", false),
    R("&16th Notes", false),
    R("16t&h Note Triplets", false),
    R("&32nd Notes", false),
    R("32nd Note Tr&iplets", false),
];

/// Options › Ruler Format, also the time ruler's and time display's right-click list (§6.6, §6.10).
pub const RULER_MENU: &[M] = &[
    R("&Samples", false),
    R("&Time", false),
    R("Se&conds", false),
    R("Time && Fra&mes", true),
    R("Absol&ute Frames", false),
    R("Measures && &Beats", false),
    R("Feet and Frames &16mm (40 fpf)", false),
    R("Feet and Frames &35mm (16 fpf)", false),
    R("SMPTE Fi&lm Sync IVTC (23.976 fps, Video)", false),
    R("SMPTE &Film Sync (24 fps, Film)", false),
    R("SMPTE &EBU (25 fps, Video)", false),
    R("SMPTE &Non-Drop (29.97 fps, Video)", false),
    R("SMPTE &Drop (29.97 fps, Video)", false),
    R("SM&PTE 30 (30 fps, Audio)", false),
    R("&Audio CD Time", false),
    S,
    X("Set T&ime at Cursor"),
    V23(&X("Set Project Tempo")),
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
    K("Q&uickfade Audio Edits", true),
    C("Auto &Ripple", AutoRipple),
    Sub("&Ripple", &[
        C("Auto &Ripple", AutoRipple),
        S,
        C("Affected &Tracks", RippleMode(RippleMode::Affected)),
        C("Affected Tracks, &Bus Tracks, Markers, and Regions", RippleMode(RippleMode::AffectedMarkers)),
        // Without bus tracks this is the item above.
        R("Affected Tracks, &Markers, and Regions", false),
        C("&All Tracks, Markers, and Regions", RippleMode(RippleMode::All)),
    ]),
    C("Lock En&velopes to Events", LockEnvelopes),
    C("I&gnore Event Grouping", IgnoreGrouping),
    K("Slip All Ta&kes", false),
    S,
    C("&Loop Playback", Loop),
    K("Metr&onome", false),
    Sub("&Loudness Meters", &[
        X("&Reset Clip"),
        X("Reset Meterin&g Engine"),
        S,
        R("EB&U R 128 Mode", true),
        R("&ATSC A 85 Mode", false),
        S,
        K("True Peak &Blocking Filter", false),
        Sub("&True Peak Range", &[
            R("-12 to 0 dB", false),
            R("-24 to 0 dB", false),
            R("-42 to 0 dB", false),
            R("-60 to 0 dB", true),
            R("-78 to 0 dB", false),
            R("-90 to 0 dB", false),
            R("-120 to 0 dB", false),
            R("-138 to 0 dB", false),
        ]),
        Sub("Loudness S&cale", &[R("&EBU +9", true), R("E&BU +18", false), S, R("&Absolute (-23 LUFS)", false)]),
        S,
        K("Show True P&eak Meter", false),
        K("&Show Labels", true),
        K("Hold Pea&ks", false),
        K("Hold &Valleys", false),
    ]),
    S,
    K("&Bypass All Audio FX", false),
    S,
    K("&Mute All Audio", false),
    K("Mute All Vi&deo", false),
    S,
    Sub("Time&code", &[
        C("&Generate MIDI Timecode", MidiTimecode),
        C("Generate &MIDI Clock", MidiClock),
        C("&Trigger from MIDI Timecode", MidiTrigger),
    ]),
    K("External Control", false),
    S,
    X("Customize &Toolbar..."),
    X("Customize Timeline Toolbar..."),
    X("Customize &Keyboard..."),
    C("&Preferences...", Preferences),
    V23(&X("Export Preferences...")),
    V23(&X("Import Preferences...")),
];

pub const HELP: &[M] = &[
    C("&Contents and Index", Help),
    C("What's &This?", WhatsThis),
    C("&Keyboard Shortcuts", KeyboardShortcuts),
    X("&Show Me How..."),
    V16(&X("Interactive Tutorial&s...")),
    V16(&X("&Welcome...")),
    S,
    C("Check for &Updates...", CheckForUpdates),
    V23(&X("Download User Manual")),
    S,
    C("About Milan Pro...", About),
];

const MENUS: [(&str, &[M]); 7] =
    [("&File", FILE), ("&Edit", EDIT), ("&View", VIEW), ("&Insert", INSERT), ("&Tools", TOOLS), ("&Options", OPTIONS), ("&Help", HELP)];

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

/// File extensions Milan Pro imports (lower case).
pub const MEDIA_EXTS: &[&str] = &[
    "mp4", "mov", "avi", "mkv", "wmv", "flv", "webm", "mxf", "mpg", "mpeg", "ts",
    "mp3", "wav", "flac", "aac", "ogg", "wma", "aiff", "opus",
    "png", "jpg", "jpeg", "tiff", "bmp", "webp", "gif", "exr",
];

/// Pick files and probe them in the background; the app adds the results when they arrive.
pub fn import_media(ctx: &Context, importer: &Importer) {
    if let Some(paths) = rfd::FileDialog::new()
        .add_filter("All Media", MEDIA_EXTS)
        .add_filter("Video", &["mp4", "mov", "avi", "mkv", "wmv", "flv", "webm", "mxf"])
        .add_filter("Audio", &["mp3", "wav", "flac", "aac", "ogg", "wma", "aiff", "opus"])
        .add_filter("Images", &["png", "jpg", "jpeg", "tiff", "bmp", "webp", "gif", "exr"])
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
        // Pan/Crop needs a video event, Track Motion a video track.
        assert!(!st.enabled(EventPanCrop) && !st.enabled(TrackMotion));
        (st.video_clip, st.video_track_focus) = (true, true);
        assert!(st.enabled(EventPanCrop) && st.enabled(TrackMotion));
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
