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
use crate::project::{Clip, Project};

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
    /// Media pool index, optionally with a source range (the Trimmer's in/out).
    Pool(usize, Option<(f64, f64)>),
    /// A file from the Explorer, imported on drop.
    File(PathBuf),
}

/// Drop a dragged item on `track` (is video, index) at `time`; None (below the last track) adds a track of the
/// media's kind. A file is probed now (shortcut: blocks the UI for one ffprobe run, ~0.1 s; move to the Importer
/// if drops of big batches are added) and added to the pool first.
pub fn drop_media_drag(project: &mut Project, drag: &MediaDrag, track: Option<(bool, usize)>, time: f64) -> bool {
    let (idx, range, added) = match drag {
        MediaDrag::Pool(i, range) => (*i, *range, false),
        MediaDrag::File(path) => match project.media_pool.iter().position(|m| m.path == *path) {
            Some(i) => (i, None, false),
            None => match crate::media::probe_media(path) {
                Ok(m) => {
                    project.media_pool.push(m);
                    project.mark_modified();
                    (project.media_pool.len() - 1, None, true)
                }
                Err(_) => return false,
            },
        },
    };
    let (is_video, track_idx) = track.unwrap_or_else(|| {
        let v = project.media_pool.get(idx).is_some_and(|m| m.media_type != crate::project::MediaType::Audio);
        (v, project.timeline.tracks(v).len())
    });
    // The import alone is still a change worth an undo step.
    crate::edit::drop_media_range(project, idx, is_video, track_idx, time, range) || added
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
