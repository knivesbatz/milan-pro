//! All panels. Colours, sizes and fonts come from `theme::get()` (never hard-coded).
pub mod automation;
pub mod chrome;
mod choosers;
mod color_grading;
pub mod dialogs;
pub mod dock;
mod edit_details;
mod explorer;
pub mod fonts;
mod fx_window;
#[allow(dead_code)] // toolkit for the look stages; not every icon is placed yet
pub mod icons;
mod keyframes;
pub mod keymap;
mod master_bus;
mod media_panel;
pub mod menus;
pub mod monitors;
pub mod render_queue;
mod mixer;
mod pancrop;
mod preview;
mod project_notes;
mod properties;
mod status;
#[allow(dead_code)] // full token set from the spec; later stages use the rest
pub mod theme;
mod timeline;
mod toolbar;
mod trackmotion;
mod transport;
mod trimmer;
mod widgets;
mod workspace;

use std::path::PathBuf;
use crate::project::Clip;

pub use choosers::{Chooser, draw_chooser, draw_chooser_toolbar};
pub use edit_details::{EditDetails, edit_details, edit_details_toolbar};
pub use project_notes::project_notes;
pub use color_grading::{ColorGrading, color_grading_toolbar, draw_color_grading};
pub use fx_window::FxWindow;
pub use explorer::{Explorer, draw_explorer, draw_explorer_toolbar, is_media};
pub use master_bus::{MasterView, master_bus, master_bus_toolbar};
pub use mixer::{ConsolePrefs, ConsoleView, feed_levels, mixer_toolbar, mixing_console, show_level, MASTER as MASTER_LEVEL};
pub use media_panel::{MediaView, draw_media_browser, draw_media_toolbar};
pub use menus::{MenuAction, add_marker, draw_menu_bar, import_media};
pub use pancrop::PanCropWin;
pub use preview::{PreviewView, draw_external, draw_preview, draw_preview_toolbar};
pub use properties::draw_properties;
pub use status::draw_status_bar;
pub use timeline::{BASE_PPS, MIN_TRACK_H, draw_timeline, frame_per_thumb_zoom, grid_line, rename_event};
pub use toolbar::{Bar, draw_main_toolbar};
pub use trackmotion::TrackMotionWin;
pub use transport::{Cmd, Playback, TransportState, add_region, draw_rate, draw_transport, run as run_cmd};
pub use trimmer::{EditSide, Trimmer, draw_trimmer, draw_trimmer_toolbar};

/// Requests from dock windows that the app carries out after the dock has been drawn.
pub enum WinAction {
    OpenTrimmer(PathBuf),
    Show(dock::Panel),
    /// Import files (skipping ones already in the project).
    Import(Vec<PathBuf>),
    /// Project Media › Create Video Proxy for these media.
    CreateProxy(Vec<crate::project::MediaItem>),
}

/// A Pan/Crop (event id), Track Motion (track id) or FX window asked for by a timeline button, a drop or a menu; the
/// app opens it.
#[derive(Clone, Debug, PartialEq)]
pub enum ToolWin {
    PanCrop(String),
    TrackMotion(String),
    /// A compositing parent's Parent Motion.
    ParentMotion(String),
    Fx(crate::edit::FxTarget),
}

/// A plug-in tile dragged from a chooser: a ready instance of its preset (a transition's values as keys at 0).
#[derive(Clone)]
pub struct PluginDrag {
    pub kind: crate::engine::plugins::PluginKind,
    pub name: String,
    pub fx: crate::project::Fx,
}

/// A plug-in tile released over `resp` this frame. Other drags' payloads are left alone.
pub fn plugin_dropped(resp: &eframe::egui::Response) -> Option<std::sync::Arc<PluginDrag>> {
    eframe::egui::DragAndDrop::has_payload_of_type::<PluginDrag>(&resp.ctx).then(|| resp.dnd_release_payload::<PluginDrag>()).flatten()
}

fn hint_id() -> eframe::egui::Id {
    // Not toolbar's "milan_status_hint": the status bar takes that one as a hover hint before the app reads this.
    eframe::egui::Id::new("milan_status_message")
}

/// A status-bar message from a window without its own (a refused plug-in drop); the app shows it.
pub fn hint(ctx: &eframe::egui::Context, msg: impl Into<String>) {
    ctx.data_mut(|d| d.insert_temp(hint_id(), msg.into()));
}

pub fn take_hint(ctx: &eframe::egui::Context) -> Option<String> {
    ctx.data_mut(|d| d.remove_temp::<String>(hint_id()))
}

fn keys_id() -> eframe::egui::Id {
    eframe::egui::Id::new("milan_plain_key_areas")
}

/// An area of a dock window that reads plain keys itself (Trimmer I/O/A, Project Media Delete): while the pointer
/// is over it the keymap leaves those keys alone. Claimed while drawing, read by the next frame's keymap.
pub fn claim_plain_keys(ctx: &eframe::egui::Context, rect: eframe::egui::Rect) {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Vec<eframe::egui::Rect>>(keys_id()).push(rect));
}

/// The pointer is over an area claimed last frame (the claims are cleared for this frame's).
pub fn over_plain_key_area(ctx: &eframe::egui::Context) -> bool {
    let areas = ctx.data_mut(|d| d.remove_temp::<Vec<eframe::egui::Rect>>(keys_id())).unwrap_or_default();
    ctx.pointer_hover_pos().is_some_and(|p| areas.iter().any(|r| r.contains(p)))
}

fn tool_id() -> eframe::egui::Id {
    eframe::egui::Id::new("milan_open_tool")
}

pub fn open_tool(ctx: &eframe::egui::Context, w: ToolWin) {
    ctx.data_mut(|d| d.insert_temp(tool_id(), Some(w)));
}

pub fn take_tool(ctx: &eframe::egui::Context) -> Option<ToolWin> {
    ctx.data_mut(|d| d.remove_temp::<Option<ToolWin>>(tool_id())).flatten()
}

/// The tool windows' layers: plain keys over them (Delete) are theirs, as over a dock window.
pub fn over_tool_window(ctx: &eframe::egui::Context) -> bool {
    let layer = ctx.pointer_hover_pos().and_then(|p| ctx.layer_id_at(p));
    [pancrop::ID, trackmotion::ID, fx_window::ID].iter().any(|id| layer == Some(dialogs::tool_layer(id)))
}

/// Drag-and-drop payload for media headed to the timeline.
#[derive(Clone)]
pub enum MediaDrag {
    /// Media pool indices (added across time, in this order), optionally with a source range (the Trimmer's in/out).
    Pool(Vec<usize>, Option<(f64, f64)>),
    /// A file from the Explorer, imported on drop.
    File(PathBuf),
}

/// Files headed for the timeline from Windows or the Explorer window. The app fills it before the timeline is drawn
/// and finishes a drop the timeline reports in `dropped_at`.
#[derive(Default)]
pub struct FileDrag {
    /// Files being dragged over the window (or just dropped from Windows).
    pub files: Vec<PathBuf>,
    /// Their media once every probe is done (unreadable files left out).
    pub items: Option<Vec<crate::project::MediaItem>>,
    /// Pointer during a drag from Windows, which egui does not track. Some only for those drags.
    pub os_pos: Option<eframe::egui::Pos2>,
    /// The files from Windows were dropped this frame; the timeline clears it when the drop is its own.
    pub os_dropped: bool,
    /// Preferences › Editing: new still image length.
    pub still_len: f64,
    /// Set by the timeline: (files, target track id or None for new tracks, time, how the drop menu adds them).
    pub dropped_at: Option<(Vec<PathBuf>, Option<String>, f64, crate::edit::DropMode)>,
}

pub use crate::project::ClipRef;

/// Event and track selection by id, in click order; the last of each is the focus.
#[derive(Clone, Default)]
pub struct Selection {
    pub clips: Vec<String>,
    pub tracks: Vec<String>,
    /// The track list had the last click: Delete removes the selected tracks when no event is selected.
    pub list_focus: bool,
    /// Selected envelope points; Delete removes them before any event.
    pub env: Option<EnvSel>,
    /// Edge-trimming mode: the event edge the nudge keys trim, marked with a red bracket.
    pub edge: Option<EdgeSel>,
}

/// An event edge selected with [ or ] (edge-trimming mode); `expanded` is Expanded Edit Mode, where the Trimmer shows
/// the edit point's outgoing and incoming frames.
#[derive(Clone, Debug, PartialEq)]
pub struct EdgeSel {
    pub clip: String,
    pub end: bool,
    pub expanded: bool,
}

/// Selected points of one track envelope: the track's id, the envelope kind and the point indices.
#[derive(Clone, Debug, PartialEq)]
pub struct EnvSel {
    pub track: String,
    pub kind: crate::project::EnvKind,
    pub keys: Vec<usize>,
}

/// An envelope point (or segment) drag: one undo step on release.
pub struct EnvDrag {
    pub track: String,
    pub kind: crate::project::EnvKind,
    /// The points it moves, and the one under the pointer (it snaps).
    pub keys: Vec<usize>,
    pub grab: usize,
    /// A segment moves only up and down.
    pub segment: bool,
    /// The envelope at the press, and the press point's time and y.
    pub orig: crate::project::Envelope,
    pub press_t: f64,
    pub press_y: f32,
    /// Project before the drag; pushed to history on release if anything changed.
    pub before: crate::project::Project,
    pub changed: bool,
}

/// Edit › Editing Tool (not saved). Slip, Slide and Stretch make event drags slip, slide or stretch without modifiers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditTool {
    #[default]
    Normal,
    Envelope,
    Selection,
    Slip,
    Slide,
    Stretch,
}

impl EditTool {
    /// Next Tool (D) / Previous Tool (Shift+D): Normal → Envelope → Selection → Normal (Slip, Slide and Stretch count
    /// as Normal).
    pub fn cycle(self, forward: bool) -> Self {
        const RING: [EditTool; 3] = [EditTool::Normal, EditTool::Envelope, EditTool::Selection];
        let k = RING.iter().position(|t| *t == self).unwrap_or(0);
        RING[if forward { (k + 1) % 3 } else { (k + 2) % 3 }]
    }
}

/// Editing options the timeline follows (from the app's preferences and toggles).
#[derive(Clone, Copy, Default)]
pub struct EditOpts {
    pub snapping: bool,
    /// Options › Snap to Grid, Snap to All Events and Grid Spacing (the grid is drawn whether or not it snaps).
    pub snap_grid: bool,
    pub snap_all: bool,
    pub grid: crate::project::GridSpacing,
    pub quantize: bool,
    pub auto_crossfade: bool,
    /// Some(mode) while Auto Ripple is on.
    pub ripple: Option<crate::edit::RippleMode>,
    /// A click selects (and a drag moves) the event's whole group unless this is on.
    pub ignore_grouping: bool,
    pub tool: EditTool,
    /// View › Show Envelopes and Options › Lock Envelopes to Events.
    pub show_envelopes: bool,
    pub lock_envelopes: bool,
    /// View › Audio / Video Envelopes rows turned off (`settings::EnvView::bit`).
    pub hidden_envs: u16,
    /// Preferences › Editing: the crossfade a transition dropped on a cut makes (seconds).
    pub cut_overlap: f64,
    /// Waveform vertical zoom (Shift+Up / Shift+Down), 1 = full scale.
    pub wave_zoom: f32,
    /// View › Video Bus Track; R8b: View › Audio Bus Tracks.
    pub video_bus: bool,
    pub bus_tracks: bool,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DragMode {
    Move,
    TrimLeft,
    TrimRight,
    /// Dragging a top corner sets the fade length.
    FadeIn,
    FadeOut,
    /// The media moves under the event (Alt+drag inside, or the Slip tool).
    Slip,
    /// An edge trims and its frame moves with it (Alt+drag an edge, or the Slip tool).
    SlipTrimLeft,
    SlipTrimRight,
    /// The event moves over its media (Ctrl+Alt+drag inside, or the Slide tool).
    Slide,
    /// The edit point with the adjacent event moves (Ctrl+Alt+drag an edge, or the Slide tool).
    RollLeft,
    RollRight,
    /// An edge changes the playback rate (Ctrl+drag an edge, or the Time Stretch tool).
    StretchLeft,
    StretchRight,
    /// The top edge sets the event's gain (audio) or opacity (video).
    Level,
}

impl DragMode {
    /// The drag a hit zone (`self`: Move, TrimLeft or TrimRight) makes with the tool and the held modifiers.
    pub fn with_tool(self, tool: EditTool, ctrl: bool, alt: bool) -> Self {
        use DragMode::*;
        let slip = tool == EditTool::Slip || (alt && !ctrl);
        let slide = tool == EditTool::Slide || (alt && ctrl);
        let stretch = tool == EditTool::Stretch || (ctrl && !alt);
        match self {
            Move if slip => Slip,
            Move if slide => Slide,
            TrimLeft if slip => SlipTrimLeft,
            TrimLeft if slide => RollLeft,
            TrimLeft if stretch => StretchLeft,
            TrimRight if slip => SlipTrimRight,
            TrimRight if slide => RollRight,
            TrimRight if stretch => StretchRight,
            m => m,
        }
    }

    /// The drag moves the event's right edge.
    pub fn right_edge(self) -> bool {
        matches!(self, DragMode::TrimRight | DragMode::SlipTrimRight | DragMode::RollRight | DragMode::StretchRight)
    }
}

pub struct DragState {
    /// The grabbed event.
    pub id: String,
    /// Events a Move drag moves (the selection); the grabbed one snaps and the rest follow.
    pub ids: Vec<String>,
    pub mode: DragMode,
    /// Pointer time minus the grabbed edge's time at drag start.
    pub grab_offset: f64,
    pub original: Clip,
    pub limit: Option<f64>,
    /// A trim's group mates with an edge on the trimmed one (`edit::edge_mates`): originals and media limits.
    pub others: Vec<(Clip, Option<f64>)>,
    /// The ripple this drag makes (or would make, for Post-Edit Ripple), as of the last frame.
    pub op: Vec<crate::edit::RippleOp>,
    /// The tracks (by id) the dragged events are on, and the earliest start among them, at drag start.
    pub tracks: Vec<String>,
    pub from: f64,
    pub snaps: crate::edit::Snaps,
    /// Project before the drag; pushed to history on release if anything changed.
    pub before: crate::project::Project,
    pub changed: bool,
}

#[cfg(test)]
mod tests {
    use super::EditTool::*;

    #[test]
    fn d_cycles_normal_envelope_selection() {
        assert_eq!([Normal, Envelope, Selection, Slip].map(|t| t.cycle(true)), [Envelope, Selection, Normal, Envelope]);
        assert_eq!([Normal, Envelope, Selection].map(|t| t.cycle(false)), [Selection, Normal, Envelope]);
    }

    #[test]
    fn drag_modes_follow_modifiers_and_tools() {
        use super::DragMode as D;
        let m = |hit: D, tool, ctrl, alt| hit.with_tool(tool, ctrl, alt);
        let zones = [D::Move, D::TrimLeft, D::TrimRight];
        assert_eq!(zones.map(|h| m(h, Normal, false, true)), [D::Slip, D::SlipTrimLeft, D::SlipTrimRight]);
        assert_eq!(zones.map(|h| m(h, Normal, true, true)), [D::Slide, D::RollLeft, D::RollRight]);
        assert_eq!(zones.map(|h| m(h, Normal, true, false)), [D::Move, D::StretchLeft, D::StretchRight]);
        assert_eq!([D::Move, D::TrimRight, D::FadeIn, D::Level].map(|h| m(h, Normal, false, false)), [D::Move, D::TrimRight, D::FadeIn, D::Level]);
        assert_eq!([m(D::Move, Slip, false, false), m(D::TrimRight, Slide, false, false), m(D::TrimLeft, Stretch, false, false)], [D::Slip, D::RollRight, D::StretchLeft]);
        assert_eq!(m(D::Move, Stretch, false, false), D::Move);
    }
}
