//! Project Media window (§5.4): toolbar, bin tree (All Media / By Type / …) beside a thumbnail, list or
//! details view. Click selects, double-click opens in the Trimmer, drag adds to the timeline.
use eframe::egui::*;
use crate::decode::Thumbs;
use crate::edit;
use crate::history::History;
use crate::media::Importer;
use crate::project::*;
use super::dock::Panel;
use super::icons::{Icon, paint_icon};
use super::menus::{self, M::*, import_media};
use super::toolbar::{arrow_menu, inert_button, split_button, tool_button};
use super::widgets::{self, ROW_H};
use super::{MediaDrag, WinAction, theme};

#[derive(Clone, Copy, PartialEq)]
enum Node {
    All,
    ByType,
    Type(MediaType),
    Tagged,
    Custom,
    Smart,
    Storyboard,
    MainTimeline,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    List,
    Details,
    Thumbnails,
}

pub struct MediaView {
    node: Node,
    by_type_open: bool,
    storyboard_open: bool,
    mode: Mode,
    tree_w: f32,
    /// Some while the search field is shown.
    search: Option<String>,
}

impl Default for MediaView {
    fn default() -> Self {
        Self { node: Node::All, by_type_open: false, storyboard_open: false, mode: Mode::Thumbnails, tree_w: 170.0, search: None }
    }
}

fn use_count(project: &Project, m: &MediaItem) -> usize {
    let tl = &project.timeline;
    tl.video_tracks.iter().chain(&tl.audio_tracks).flat_map(|t| &t.clips).filter(|c| c.media_id.as_deref() == Some(m.id.as_str())).count()
}

impl MediaView {
    fn shows(&self, project: &Project, m: &MediaItem) -> bool {
        let in_node = match self.node {
            Node::All | Node::ByType => true,
            Node::Type(MediaType::Image) => m.media_type == MediaType::Image,
            Node::Type(k) => m.media_type == k,
            Node::MainTimeline => use_count(project, m) > 0,
            Node::Tagged | Node::Custom | Node::Smart | Node::Storyboard => false,
        };
        let q = self.search.as_deref().unwrap_or("").to_lowercase();
        in_node && (q.is_empty() || m.name.to_lowercase().contains(&q))
    }
}

/// Remove media `idxs` and every event that uses it, as one undo step.
fn remove_media(project: &mut Project, history: &mut History, mut idxs: Vec<usize>) {
    // A stale index (from before an undo) removes nothing.
    idxs.retain(|&i| i < project.media_pool.len());
    if idxs.is_empty() {
        return;
    }
    history.push(project.clone());
    idxs.sort_unstable();
    idxs.dedup();
    for i in idxs.into_iter().rev() {
        let m = project.media_pool.remove(i);
        // Clips of removed media would play black/silent.
        let tl = &mut project.timeline;
        for t in tl.video_tracks.iter_mut().chain(&mut tl.audio_tracks) {
            t.clips.retain(|c| c.media_id.as_deref() != Some(m.id.as_str()));
        }
    }
    project.mark_modified();
}

/// Dock toolbar row (§5.4), left to right.
#[allow(clippy::too_many_arguments)]
pub fn draw_media_toolbar(
    ui: &mut Ui,
    project: &mut Project,
    history: &mut History,
    view: &mut MediaView,
    selected: &mut Option<usize>,
    sel_clips: &mut Vec<String>,
    importer: &Importer,
    actions: &mut Vec<WinAction>,
) {
    let t = theme::get();
    let unused: Vec<usize> = (0..project.media_pool.len()).filter(|&i| use_count(project, &project.media_pool[i]) == 0).collect();
    if tool_button(ui, Icon::RemoveUnused, "Remove All Unused Media from Project", true, false).clicked() {
        remove_media(project, history, unused);
        (*selected, *sel_clips) = (None, Vec::new());
    }
    // Import Media…: icon + text + ▾ in one hover box.
    let (rect, resp) = ui.allocate_exact_size(vec2(124.0, t.m.button), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(t.m.widget_radius), t.button_hover_bg);
    }
    let icon = Rect::from_min_size(pos2(rect.min.x + 3.0, rect.center().y - 8.0), vec2(16.0, 16.0));
    paint_icon(ui.painter(), icon, Icon::Import, t.look(), None);
    ui.painter().text(pos2(icon.max.x + 4.0, rect.center().y), Align2::LEFT_CENTER, "Import Media...", t.font(t.font.ui), t.text);
    paint_icon(ui.painter(), Rect::from_center_size(pos2(rect.max.x - 9.0, rect.center().y), vec2(16.0, 16.0)), Icon::DropDown, t.look(), None);
    if resp.on_hover_text("Import Media...").clicked() {
        import_media(ui.ctx(), importer);
    }
    tool_button(ui, Icon::Capture, "Capture Video (not available)", false, false);
    if t.look() == super::theme::UiLook::Vegas16 {
        inert_button(ui, Icon::DeviceMedia, "Import from Device (not available)", false);
    }
    inert_button(ui, Icon::WebMedia, "Get Media from the Web (not available)", false);
    if tool_button(ui, Icon::Delete, "Remove Selected Media from Project", selected.is_some(), false).clicked() {
        remove_media(project, history, selected.take().into_iter().collect());
        sel_clips.clear();
    }
    if tool_button(ui, Icon::Properties, "Media Properties", selected.is_some(), false).clicked() {
        sel_clips.clear();
        actions.push(WinAction::Show(Panel::Properties));
    }
    tool_button(ui, Icon::TrackFx, "Media FX (not available yet)", false, false);
    // Start Preview opens the selected (else first) media in the Trimmer, where it can be played.
    let target = selected.and_then(|i| project.media_pool.get(i)).or(project.media_pool.first());
    if tool_button(ui, Icon::Play, "Start Preview (opens in the Trimmer)", target.is_some(), false).clicked() {
        if let Some(m) = target {
            actions.push(WinAction::OpenTrimmer(m.path.clone()));
        }
    }
    tool_button(ui, Icon::Stop, "Stop Preview", false, false);
    inert_button(ui, Icon::AutoPreview, "Auto Preview (not available yet)", true);
    let (m, a) = split_button(ui, Icon::Views, "Views", true, false);
    let mut pick = |ui: &mut Ui| {
        for mode in [Mode::List, Mode::Details, Mode::Thumbnails] {
            if ui.radio(view.mode == mode, format!("{mode:?}")).clicked() {
                view.mode = mode;
            }
        }
    };
    if arrow_menu(ui, &a, &mut pick).is_none() {
        arrow_menu(ui, &m, &mut pick);
    }
    if tool_button(ui, Icon::Search, "Search", true, view.search.is_some()).clicked() {
        view.search = if view.search.is_some() { None } else { Some(String::new()) };
    }
    if let Some(q) = &mut view.search {
        ui.add_space(4.0);
        widgets::search_field(ui, q, "Search media", 140.0);
    }
}

/// Body: bin tree, splitter, and the items of the selected bin.
#[allow(clippy::too_many_arguments)]
pub fn draw_media_browser(
    ui: &mut Ui,
    project: &mut Project,
    view: &mut MediaView,
    selected: &mut Option<usize>,
    sel_clips: &mut Vec<String>,
    history: &mut History,
    thumbs: &mut Thumbs,
    actions: &mut Vec<WinAction>,
) {
    let area = ui.max_rect();
    let (left, right) = widgets::pane_split(ui, area, &mut view.tree_w, "media_tree");
    widgets::scroll_pane(ui, left, "media_tree", |ui| tree(ui, view));

    let items: Vec<usize> = (0..project.media_pool.len()).filter(|&i| view.shows(project, &project.media_pool[i])).collect();
    let mut cmd: Option<(usize, ItemCmd)> = None;
    widgets::scroll_pane(ui, right, "media_items", |ui| {
        if project.media_pool.is_empty() {
            let t = theme::get();
            ui.add_space(30.0);
            ui.vertical_centered(|ui| ui.label(RichText::new("Drop media here or use Import Media...").color(t.text_dim)));
            return;
        }
        let resps = match view.mode {
            Mode::Thumbnails => thumbnails(ui, project, &items, *selected, thumbs),
            Mode::Details => details(ui, project, &items, *selected),
            Mode::List => list(ui, project, &items, *selected),
        };
        for (i, r) in resps {
            item_interaction(ui, &r, i, &mut cmd);
        }
    });
    if ui.rect_contains_pointer(right) && selected.is_some() && ui.input(|i| i.key_pressed(Key::Delete)) && !ui.ctx().wants_keyboard_input() {
        cmd = selected.map(|i| (i, ItemCmd::Remove));
    }
    match cmd {
        Some((i, ItemCmd::Select)) => {
            *selected = Some(i);
            sel_clips.clear(); // so Properties shows the media
        }
        Some((i, ItemCmd::Trim)) => {
            *selected = Some(i);
            actions.push(WinAction::OpenTrimmer(project.media_pool[i].path.clone()));
        }
        Some((i, ItemCmd::Add(video))) => {
            // Same lock/type checks and linked audio as dropping on the timeline.
            let (before, at) = (project.clone(), project.timeline.cursor_position);
            if edit::drop_media(project, i, video, 0, at) {
                history.push(before);
            }
        }
        Some((i, ItemCmd::Remove)) => {
            remove_media(project, history, vec![i]);
            (*selected, *sel_clips) = (None, Vec::new());
        }
        Some((i, ItemCmd::Explore)) => super::dialogs::open_folder(&project.media_pool[i].path),
        Some((i, ItemCmd::Properties)) => {
            (*selected, *sel_clips) = (Some(i), Vec::new());
            actions.push(WinAction::Show(Panel::Properties));
        }
        None => {}
    }
}

enum ItemCmd {
    Select,
    Trim,
    Add(bool),
    Remove,
    Explore,
    Properties,
}

fn item_interaction(ui: &Ui, r: &Response, i: usize, cmd: &mut Option<(usize, ItemCmd)>) {
    if r.double_clicked() {
        *cmd = Some((i, ItemCmd::Trim));
    } else if r.clicked() || r.secondary_clicked() {
        *cmd = Some((i, ItemCmd::Select));
    }
    // Payload is the media pool index; the timeline drops it on release.
    r.dnd_set_drag_payload(MediaDrag::Pool(i, None));
    if r.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    }
    // Project Media item menu (§6.9); Milan Pro adds "Add to Video/Audio Track".
    menus::context(r, |ui| {
        let mut item = |ui: &mut Ui, text: &str, c: ItemCmd| {
            if menus::entry(ui, text, "", true, menus::Mark::None) {
                *cmd = Some((i, c));
            }
        };
        item(ui, "Open in &Trimmer", ItemCmd::Trim);
        menus::off(ui, "Open Parent Media in &Trimmer");
        item(ui, "Add to &Video Track", ItemCmd::Add(true));
        item(ui, "Add to A&udio Track", ItemCmd::Add(false));
        menus::show(ui, &[X("Add as CD Trac&k"), X("Add at Project &Frame Rate"), X("Se&lect Timeline Events"), X("Match Pr&oject Video Settings"), X("Edit Source Pro&ject")]);
        item(ui, "Explore &Containing Folder", ItemCmd::Explore);
        menus::off(ui, "Create Video Pro&xy");
        menus::sep(ui);
        menus::off(ui, "Remove from Current &Bin");
        item(ui, "&Remove from Project", ItemCmd::Remove);
        menus::show(ui, ITEM_REST);
        item(ui, "&Properties...", ItemCmd::Properties);
        menus::show(ui, &[V23(&S), V23(&X("Tempo Detection")), V23(&X("Detect Scenes and Add to Timeline")), V23(&X("Detect Scenes and Add to Timeline from Cursor")), V23(&X("Detect Scenes and Create Subclips"))]);
    });
}

const ITEM_REST: &[menus::M] = &[
    X("Remove from Project and &Delete File(s)"),
    S,
    X("&Search Media Bins..."),
    S,
    X("Re&name"),
    X("R&eplace..."),
    X("Refres&h"),
    S,
    X("Media &FX..."),
    X("Rotate 90° Co&unterclockwise"),
    X("Rotate 90° Clock&wise"),
    S,
    X("Lay Out Tracks Using Media &Timecode"),
    X("Lay Out Tracks Using Media &Date/Time Stamp"),
    S,
    Sub("V&iew", &[X("Columns")]),
    X("&Auto-Size Columns"),
    S,
    X("File Format Pr&operties..."),
];

fn tree(ui: &mut Ui, view: &mut MediaView) {
    let mut node = |ui: &mut Ui, depth, exp: Option<&mut bool>, icon, label: &str, n: Node| {
        let (r, toggled) = widgets::tree_row(ui, depth, exp.as_ref().map(|e| **e), Some(icon), label, view.node == n);
        if toggled || r.double_clicked() {
            if let Some(e) = exp {
                *e = !*e;
            }
        }
        if r.clicked() {
            view.node = n;
        }
    };
    let (mut by_type, mut story) = (view.by_type_open, view.storyboard_open);
    // Custom Bins has an expander but no bins inside yet.
    let mut custom = false;
    let bin = if theme::get().look() == theme::UiLook::Vegas16 { Icon::MediaBin } else { Icon::TrackVideo };
    node(ui, 0, None, bin, "All Media", Node::All);
    node(ui, 0, Some(&mut by_type), bin, "Media By Type", Node::ByType);
    if by_type {
        node(ui, 1, None, Icon::TrackAudio, "Audio", Node::Type(MediaType::Audio));
        node(ui, 1, None, Icon::TrackVideo, "Video", Node::Type(MediaType::Video));
        node(ui, 1, None, Icon::GeneratedMedia, "Stills", Node::Type(MediaType::Image));
    }
    node(ui, 0, None, bin, "Tagged Media", Node::Tagged);
    node(ui, 0, Some(&mut custom), Icon::Folder, "Custom Bins", Node::Custom);
    node(ui, 0, None, Icon::Folder, "Smart Bins", Node::Smart);
    node(ui, 0, Some(&mut story), Icon::Folder, "Storyboard Bins", Node::Storyboard);
    if story {
        node(ui, 1, None, Icon::Folder, "Main Timeline", Node::MainTimeline);
    }
    (view.by_type_open, view.storyboard_open) = (by_type, story);
}

/// Source time shown on a tile: a quarter in, so the frame is rarely a black lead-in.
fn thumb_time(m: &MediaItem) -> f64 {
    (m.duration * 0.25 * 10.0).floor() / 10.0
}

/// Tiles: ≈100×65 thumbnail, name centred below in up to 2 lines; 28 px / 30 px gaps (§5.4).
fn thumbnails(ui: &mut Ui, project: &Project, items: &[usize], selected: Option<usize>, thumbs: &mut Thumbs) -> Vec<(usize, Response)> {
    let (tw, th, gx, gy, cap) = (100.0, 65.0, 28.0, 30.0, 32.0);
    let width = ui.available_width().max(tw + gx);
    let cols = (((width - 14.0) / (tw + gx)).floor() as usize).max(1);
    let rows = items.len().div_ceil(cols);
    let (area, _) = ui.allocate_exact_size(vec2(width, 10.0 + rows as f32 * (th + cap + gy)), Sense::hover());
    let mut out = Vec::new();
    for (k, &i) in items.iter().enumerate() {
        let (c, r) = (k % cols, k / cols);
        let min = area.min + vec2(14.0 + c as f32 * (tw + gx), 10.0 + r as f32 * (th + cap + gy));
        let tile = Rect::from_min_size(min - vec2(6.0, 4.0), vec2(tw + 12.0, th + cap + 8.0));
        let thumb = Rect::from_min_size(min, vec2(tw, th));
        let resp = ui.interact(tile, ui.id().with(("media_tile", i)), Sense::click_and_drag());
        let m = &project.media_pool[i];
        widgets::tile(ui, tile, thumb, &m.name, selected == Some(i), resp.hovered());
        let p = ui.painter();
        match m.media_type {
            MediaType::Audio | MediaType::Unknown => widgets::paint_audio_page(p, thumb),
            _ => {
                if let Some(tex) = thumbs.get(m, thumb_time(m)) {
                    widgets::paint_thumb(p, thumb, tex);
                }
            }
        }
        out.push((i, resp));
    }
    out
}

const DETAIL_COLS: [(&str, f32); 7] =
    [("Name", 180.0), ("Use Count", 70.0), ("Type", 60.0), ("Length", 90.0), ("Frame Rate", 80.0), ("Frame Size", 90.0), ("Path", 400.0)];

fn details(ui: &mut Ui, project: &Project, items: &[usize], selected: Option<usize>) -> Vec<(usize, Response)> {
    let t = theme::get();
    widgets::header_row(ui, &DETAIL_COLS);
    let fps = project.settings.frame_rate;
    let mut out = Vec::new();
    for &i in items {
        let m = &project.media_pool[i];
        let r = widgets::list_row(ui, ROW_H, selected == Some(i));
        let size = match (m.width, m.height) {
            (Some(w), Some(h)) => format!("{w}x{h}"),
            _ => String::new(),
        };
        let values = [
            m.name.clone(),
            use_count(project, m).to_string(),
            format!("{:?}", m.media_type),
            if m.media_type == MediaType::Image { String::new() } else { format_timecode(m.duration, fps) },
            m.frame_rate.map(|f| format!("{f:.3}")).unwrap_or_default(),
            size,
            m.path.display().to_string(),
        ];
        widgets::row_cells(ui, r.rect, &DETAIL_COLS, &values, if selected == Some(i) { Color32::WHITE } else { t.text });
        out.push((i, r));
    }
    out
}

fn list(ui: &mut Ui, project: &Project, items: &[usize], selected: Option<usize>) -> Vec<(usize, Response)> {
    let t = theme::get();
    let mut out = Vec::new();
    for &i in items {
        let m = &project.media_pool[i];
        let r = widgets::list_row(ui, ROW_H, selected == Some(i));
        let icon = if m.media_type == MediaType::Audio { Icon::TrackAudio } else { Icon::TrackVideo };
        paint_icon(ui.painter(), Rect::from_min_size(pos2(r.rect.min.x + 6.0, r.rect.center().y - 8.0), vec2(16.0, 16.0)), icon, t.look(), None);
        let color = if selected == Some(i) { Color32::WHITE } else { t.text };
        ui.painter().text(pos2(r.rect.min.x + 28.0, r.rect.center().y), Align2::LEFT_CENTER, &m.name, t.font(t.font.ui), color);
        out.push((i, r));
    }
    out
}
