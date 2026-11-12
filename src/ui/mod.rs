//! All panels. Colours, sizes and fonts come from `theme::get()` (never hard-coded).
pub mod chrome;
mod choosers;
pub mod dialogs;
pub mod dock;
mod explorer;
pub mod fonts;
#[allow(dead_code)] // toolkit for the look stages; not every icon is placed yet
pub mod icons;
pub mod keymap;
mod master_bus;
mod media_panel;
pub mod menus;
mod panels;
mod preview;
mod properties;
mod status;
#[allow(dead_code)] // full token set from the spec; later stages use the rest
pub mod theme;
mod timeline;
mod toolbar;
mod transport;
mod trimmer;
mod widgets;

use std::path::PathBuf;
use crate::project::Clip;

pub use choosers::{Chooser, Kind as ChooserKind, draw_chooser, draw_chooser_toolbar};
pub use explorer::{Explorer, draw_explorer, draw_explorer_toolbar, is_media};
pub use master_bus::{MasterView, master_bus, master_bus_toolbar};
pub use media_panel::{MediaView, draw_media_browser, draw_media_toolbar};
pub use menus::{MenuAction, add_marker, draw_menu_bar, import_media};
pub use panels::placeholder;
pub use preview::{PreviewView, draw_preview, draw_preview_toolbar};
pub use properties::draw_properties;
pub use status::draw_status_bar;
pub use timeline::{BASE_PPS, MIN_TRACK_H, draw_timeline};
pub use toolbar::draw_main_toolbar;
pub use transport::{Cmd, Playback, TransportAction, TransportState, add_region, draw_rate, draw_transport, run as run_cmd};
pub use trimmer::{Trimmer, draw_trimmer, draw_trimmer_toolbar};

/// Requests from dock windows that the app carries out after the dock has been drawn.
pub enum WinAction {
    OpenTrimmer(PathBuf),
    Show(dock::Panel),
    /// Import files (skipping ones already in the project).
    Import(Vec<PathBuf>),
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
    /// Set by the timeline: (files, target track id or None for new tracks, time).
    pub dropped_at: Option<(Vec<PathBuf>, Option<String>, f64)>,
}

pub use crate::project::ClipRef;

/// Event and track selection by id, in click order; the last of each is the focus.
#[derive(Clone, Default)]
pub struct Selection {
    pub clips: Vec<String>,
    pub tracks: Vec<String>,
    /// The track list had the last click: Delete removes the selected tracks when no event is selected.
    pub list_focus: bool,
}

#[derive(Clone, Copy, PartialEq)]
pub enum DragMode {
    Move,
    TrimLeft,
    TrimRight,
    /// Dragging a top corner sets the fade length.
    FadeIn,
    FadeOut,
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
    pub snap_targets: Vec<f64>,
    /// Project before the drag; pushed to history on release if anything changed.
    pub before: crate::project::Project,
    pub changed: bool,
}
