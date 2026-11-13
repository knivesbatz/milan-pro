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
use super::menus::{self, Fixed, Later, M::*, NotPlanned, import_media};
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
    /// Selected media (pool indices, click order); the app's `selected_media` is the focus, the last one.
    sel: Vec<usize>,
    /// Where Shift+click ranges start.
    anchor: Option<usize>,
}

impl Default for MediaView {
    fn default() -> Self {
        Self { node: Node::All, by_type_open: false, storyboard_open: false, mode: Mode::Thumbnails, tree_w: 170.0, search: None, sel: Vec::new(), anchor: None }
    }
}

/// Keep the multi-selection in step with the focus, which the app changes too (Undo and New clear it, Select in
/// Project Media sets it).
fn sync_sel(view: &mut MediaView, focus: Option<usize>, n: usize) {
    view.sel.retain(|&i| i < n);
    if focus.is_none_or(|f| view.sel.last() != Some(&f)) {
        view.sel = focus.filter(|&f| f < n).into_iter().collect();
    }
}

/// `i` and the rest of the selection in list order when `i` is selected, else `i` alone (what a drag or an item
/// command acts on).
fn targets(view: &MediaView, items: &[usize], i: usize) -> Vec<usize> {
    if view.sel.contains(&i) { items.iter().copied().filter(|k| view.sel.contains(k)).collect() } else { vec![i] }
}

/// Click on item `i` (`items` in list order): selects it alone, Ctrl toggles it, Shift selects the run from the
/// anchor (the last plain or Ctrl click, kept by Shift+clicks as in Windows lists; the focus when it is gone).
fn click(view: &mut MediaView, items: &[usize], focus: Option<usize>, i: usize, ctrl: bool, shift: bool) {
    let pos = |k: usize| items.iter().position(|&x| x == k);
    let anchor = view.anchor.filter(|a| view.sel.contains(a)).or(focus);
    if ctrl {
        if let Some(k) = view.sel.iter().position(|&s| s == i) {
            view.sel.remove(k);
        } else {
            view.sel.push(i);
        }
    } else if let Some((a, b)) = anchor.and_then(pos).zip(pos(i)).filter(|_| shift) {
        view.sel = items[a.min(b)..=a.max(b)].iter().copied().filter(|&k| k != i).collect();
        view.sel.push(i);
        return;
    } else {
        view.sel = vec![i];
    }
    view.anchor = Some(i);
}

fn use_count(project: &Project, m: &MediaItem) -> usize {
    let tl = &project.timeline;
    // Inactive takes count: their media must stay in the project.
    tl.video_tracks.iter().chain(&tl.audio_tracks).flat_map(|t| &t.clips).filter(|c| edit::uses_media(c, &m.id)).count()
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

/// Remove media `idxs`, the takes that use it and the events left with no take (`edit::forget_media`), as one undo
/// step.
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
        edit::forget_media(&mut project.timeline, &m.id);

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
    sync_sel(view, *selected, project.media_pool.len());
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
    tool_button(ui, Icon::Capture, &NotPlanned("R59").tip("Capture Video"), false, false);
    if t.look() == super::theme::UiLook::Vegas16 {
        inert_button(ui, Icon::DeviceMedia, &NotPlanned("R59").tip("Import from Device"), false);
    }
    inert_button(ui, Icon::WebMedia, &NotPlanned("R59").tip("Get Media from the Web"), false);
    if tool_button(ui, Icon::Delete, "Remove Selected Media from Project", selected.is_some(), false).clicked() {
        remove_media(project, history, std::mem::take(&mut view.sel));
        (*selected, *sel_clips) = (None, Vec::new());
    }
    if tool_button(ui, Icon::Properties, "Media Properties", selected.is_some(), false).clicked() {
        sel_clips.clear();
        properties(ui.ctx(), selected.and_then(|i| project.media_pool.get(i)), actions);
    }
    tool_button(ui, Icon::TrackFx, &NotPlanned("R57").tip("Media FX"), false, false);
    // Start Preview opens the selected (else first) media in the Trimmer, where it can be played.
    let target = selected.and_then(|i| project.media_pool.get(i)).or(project.media_pool.first());
    if tool_button(ui, Icon::Play, "Start Preview (opens in the Trimmer)", target.is_some(), false).clicked() {
        if let Some(m) = target {
            actions.push(WinAction::OpenTrimmer(m.path.clone()));
        }
    }
    // Previews play in the Trimmer, which has its own Stop.
    tool_button(ui, Icon::Stop, &NotPlanned("R57").tip("Stop Preview"), false, false);
    inert_button(ui, Icon::AutoPreview, &NotPlanned("R57").tip("Auto Preview"), true);
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
    still_len: f64,
    xfade: bool,
) {
    let area = ui.max_rect();
    let (left, right) = widgets::pane_split(ui, area, &mut view.tree_w, "media_tree");
    widgets::scroll_pane(ui, left, "media_tree", |ui| tree(ui, view));

    sync_sel(view, *selected, project.media_pool.len());
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
            Mode::Thumbnails => thumbnails(ui, project, &items, &view.sel, thumbs),
            Mode::Details => details(ui, project, &items, &view.sel),
            Mode::List => list(ui, project, &items, &view.sel),
        };
        for (i, r) in resps {
            let (video, nested) = (crate::proxy::is_video(&project.media_pool[i]), project.media_pool[i].nested);
            item_interaction(ui, &r, i, view.sel.contains(&i), video, nested, &mut cmd);
            // The payload is every selected item when the grabbed one is selected, so they are added across time.
            if r.drag_started() {
                r.dnd_set_drag_payload(MediaDrag::Pool(targets(view, &items, i), None));
            }
        }
    });
    if selected.is_some() {
        super::claim_plain_keys(ui.ctx(), right);
    }
    if ui.rect_contains_pointer(right) && selected.is_some() && ui.input(|i| i.key_pressed(Key::Delete)) && !ui.ctx().wants_keyboard_input() {
        cmd = selected.map(|i| (i, ItemCmd::Remove));
    }
    match cmd {
        Some((i, ItemCmd::Select(m))) => {
            click(view, &items, *selected, i, m.command, m.shift);
            *selected = view.sel.last().copied();
            sel_clips.clear(); // so Properties shows the media
        }
        Some((i, ItemCmd::Trim)) => {
            (*selected, view.sel) = (Some(i), vec![i]);
            actions.push(WinAction::OpenTrimmer(project.media_pool[i].path.clone()));
        }
        Some((i, ItemCmd::Add(video))) => {
            // Same lock/type checks, linked audio and across-time layout as dropping on the timeline.
            let (before, at) = (project.clone(), project.timeline.cursor_position);
            let ids = edit::drop_media(project, &targets(view, &items, i), None, Some((video, 0)), at, still_len, xfade);
            if !ids.is_empty() {
                history.push(before);
            }
        }
        Some((i, ItemCmd::Remove)) => {
            remove_media(project, history, targets(view, &items, i));
            (*selected, *sel_clips) = (None, Vec::new());
        }
        Some((i, ItemCmd::EditSource)) => actions.push(WinAction::OpenNested(project.media_pool[i].path.clone())),
        Some((i, ItemCmd::Explore)) => super::dialogs::open_folder(&project.media_pool[i].path),
        Some((i, ItemCmd::Proxy)) => actions.push(WinAction::CreateProxy(targets(view, &items, i).into_iter().map(|k| project.media_pool[k].clone()).collect())),
        // Replace...: the item (and every event using it) takes another file, probed in the background like an import.
        Some((i, ItemCmd::Replace)) if project.media_pool[i].generator.is_none() => {
            let file = rfd::FileDialog::new().pick_file().filter(|p| !project.media_pool.iter().any(|m| m.path == *p));
            if let Some(path) = file {
                history.push(project.clone());
                let m = &mut project.media_pool[i];
                let name = path.file_name().map_or(m.name.clone(), |n| n.to_string_lossy().into_owned());
                *m = crate::project::MediaItem { id: std::mem::take(&mut m.id), path: path.clone(), name, media_type: m.media_type, ..Default::default() };
                project.mark_modified();
                actions.push(WinAction::Import(vec![path]));
            }
        }
        Some((_, ItemCmd::Replace)) => {}
        Some((i, ItemCmd::Properties)) => {
            (*selected, *sel_clips) = (Some(i), Vec::new());
            properties(ui.ctx(), project.media_pool.get(i), actions);
        }
        None => {}
    }
}

/// Properties: generated media opens its generator in the FX window, other media shows in the Properties window.
fn properties(ctx: &Context, m: Option<&crate::project::MediaItem>, actions: &mut Vec<WinAction>) {
    match m.filter(|m| m.generator.is_some()) {
        Some(m) => super::open_tool(ctx, super::ToolWin::Fx(edit::FxTarget::Media(m.id.clone()))),
        None => actions.push(WinAction::Show(Panel::Properties)),
    }
}

enum ItemCmd {
    /// With the click's modifiers.
    Select(Modifiers),
    Trim,
    Add(bool),
    Remove,
    Explore,
    Proxy,
    Replace,
    Properties,
    /// R6b: Edit Source Project (a nested timeline).
    EditSource,
}

/// `selected`: a right-click on a selected item keeps the selection, so its menu acts on all of it.
/// `video`: the item is file-based video without alpha, which Create Video Proxy needs; `nested`: a nested timeline.
#[allow(clippy::too_many_arguments)]
fn item_interaction(ui: &Ui, r: &Response, i: usize, selected: bool, video: bool, nested: bool, cmd: &mut Option<(usize, ItemCmd)>) {
    if r.double_clicked() {
        *cmd = Some((i, ItemCmd::Trim));
    } else if r.clicked() || (r.secondary_clicked() && !selected) {
        *cmd = Some((i, ItemCmd::Select(if r.clicked() { ui.input(|i| i.modifiers) } else { Modifiers::NONE })));
    }
    if r.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    }
    // Project Media item menu (§6.9); Milan Pro adds "Add to Video/Audio Track".
    menus::context(r, |ui| {
        let mut item = |ui: &mut Ui, text: &str, c: ItemCmd, on: bool| {
            if menus::entry(ui, text, "", on, menus::Mark::None) {
                *cmd = Some((i, c));
            }
        };
        item(ui, "Open in &Trimmer", ItemCmd::Trim, true);
        menus::show(ui, ITEM_TOP);
        item(ui, "Add to &Video Track", ItemCmd::Add(true), true);
        item(ui, "Add to A&udio Track", ItemCmd::Add(false), true);
        menus::show(ui, ITEM_ADD);
        item(ui, "Edit Source Pro&ject", ItemCmd::EditSource, nested);
        item(ui, "Explore &Containing Folder", ItemCmd::Explore, true);
        item(ui, "Create Video Pro&xy", ItemCmd::Proxy, video);
        menus::sep(ui);
        menus::show(ui, ITEM_BIN);
        item(ui, "&Remove from Project", ItemCmd::Remove, true);
        menus::show(ui, ITEM_REST);
        item(ui, "R&eplace...", ItemCmd::Replace, true);
        menus::show(ui, ITEM_TAIL);
        item(ui, "&Properties...", ItemCmd::Properties, true);
        menus::show(ui, ITEM_V23);
    });
}

const ITEM_TOP: &[menus::M] = &[X("Open Parent Media in &Trimmer", NotPlanned("R57"))];

const ITEM_ADD: &[menus::M] = &[
    X("Add as CD Trac&k", NotPlanned("R59")),
    X("Add at Project &Frame Rate", NotPlanned("R57")),
    X("Se&lect Timeline Events", NotPlanned("R57")),
    X("Match Pr&oject Video Settings", NotPlanned("R57")),
];

const ITEM_BIN: &[menus::M] = &[X("Remove from Current &Bin", Later("R45a"))];

const ITEM_REST: &[menus::M] = &[
    X("Remove from Project and &Delete File(s)", Fixed("disabled on purpose: Milan Pro never deletes your files")),
    S,
    X("&Search Media Bins...", Later("R45a")),
    S,
    X("Re&name", NotPlanned("R57")),
];

const ITEM_TAIL: &[menus::M] = &[
    X("Refres&h", NotPlanned("R57")),
    S,
    X("Media &FX...", NotPlanned("R57")),
    X("Rotate 90° Co&unterclockwise", NotPlanned("R57")),
    X("Rotate 90° Clock&wise", NotPlanned("R57")),
    S,
    X("Lay Out Tracks Using Media &Timecode", Later("R17a")),
    X("Lay Out Tracks Using Media &Date/Time Stamp", Later("R17a")),
    S,
    Sub("V&iew", &[X("Columns", NotPlanned("R57"))]),
    X("&Auto-Size Columns", NotPlanned("R57")),
    S,
    X("File Format Pr&operties...", Later("R45b")),
];

const ITEM_V23: &[menus::M] = &[
    V23(&S),
    V23(&X("Tempo Detection", NotPlanned("R57"))),
    V23(&X("Detect Scenes and Add to Timeline", NotPlanned("R57"))),
    V23(&X("Detect Scenes and Add to Timeline from Cursor", NotPlanned("R57"))),
    V23(&X("Detect Scenes and Create Subclips", NotPlanned("R57"))),
];

/// The Project Media item menu's tables, for the Unfinished Features list.
pub const ITEM_MENUS: &[&[menus::M]] = &[ITEM_TOP, ITEM_ADD, ITEM_BIN, ITEM_REST, ITEM_TAIL, ITEM_V23];

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
fn thumbnails(ui: &mut Ui, project: &Project, items: &[usize], sel: &[usize], thumbs: &mut Thumbs) -> Vec<(usize, Response)> {
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
        widgets::tile(ui, tile, thumb, &m.name, sel.contains(&i), resp.hovered());
        let p = ui.painter();
        match m.media_type {
            MediaType::Audio | MediaType::Unknown => widgets::paint_audio_page(p, thumb),
            _ => {
                if let Some(tex) = thumbs.get(m, thumb_time(m)) {
                    widgets::paint_thumb(p, thumb, tex);
                }
                if crate::proxy::ready(&m.path).is_some() {
                    proxy_badge(p, thumb.right_top() + vec2(-2.0, 2.0));
                }
            }
        }
        out.push((i, resp));
    }
    out
}

/// "P" on media that has a video proxy, its top-right corner at `tr`.
fn proxy_badge(p: &Painter, tr: Pos2) {
    let r = Rect::from_min_max(tr - vec2(13.0, 0.0), tr + vec2(0.0, 13.0));
    p.rect_filled(r, CornerRadius::same(2), theme::get().accent);
    p.text(r.center(), Align2::CENTER_CENTER, "P", FontId::proportional(10.0), Color32::WHITE);
}

const DETAIL_COLS: [(&str, f32); 7] =
    [("Name", 180.0), ("Use Count", 70.0), ("Type", 60.0), ("Length", 90.0), ("Frame Rate", 80.0), ("Frame Size", 90.0), ("Path", 400.0)];

fn details(ui: &mut Ui, project: &Project, items: &[usize], sel: &[usize]) -> Vec<(usize, Response)> {
    let t = theme::get();
    widgets::header_row(ui, &DETAIL_COLS);
    let fps = project.settings.frame_rate;
    let mut out = Vec::new();
    for &i in items {
        let m = &project.media_pool[i];
        let r = widgets::list_row(ui, ROW_H, sel.contains(&i));
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
        widgets::row_cells(ui, r.rect, &DETAIL_COLS, &values, if sel.contains(&i) { Color32::WHITE } else { t.text });
        if crate::proxy::ready(&m.path).is_some() {
            proxy_badge(ui.painter(), pos2(r.rect.min.x + DETAIL_COLS[0].1 - 6.0, r.rect.min.y + 2.0));
        }
        out.push((i, r));
    }
    out
}

fn list(ui: &mut Ui, project: &Project, items: &[usize], sel: &[usize]) -> Vec<(usize, Response)> {
    let t = theme::get();
    let mut out = Vec::new();
    for &i in items {
        let m = &project.media_pool[i];
        let r = widgets::list_row(ui, ROW_H, sel.contains(&i));
        let icon = if m.media_type == MediaType::Audio { Icon::TrackAudio } else { Icon::TrackVideo };
        paint_icon(ui.painter(), Rect::from_min_size(pos2(r.rect.min.x + 6.0, r.rect.center().y - 8.0), vec2(16.0, 16.0)), icon, t.look(), None);
        let color = if sel.contains(&i) { Color32::WHITE } else { t.text };
        let name = ui.painter().text(pos2(r.rect.min.x + 28.0, r.rect.center().y), Align2::LEFT_CENTER, &m.name, t.font(t.font.ui), color);
        if crate::proxy::ready(&m.path).is_some() {
            proxy_badge(ui.painter(), pos2(name.max.x + 18.0, r.rect.min.y + 2.0));
        }
        out.push((i, r));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_click_keeps_the_anchor() {
        let (mut v, items) = (MediaView::default(), [0, 1, 2, 3, 4]);
        click(&mut v, &items, None, 0, false, false); // A
        click(&mut v, &items, Some(0), 3, false, true); // Shift D: A..D
        assert_eq!(v.sel, [0, 1, 2, 3]);
        click(&mut v, &items, Some(3), 1, false, true); // Shift B: A..B, not B..D
        assert_eq!(v.sel, [0, 1]);
        click(&mut v, &items, Some(1), 4, true, false); // Ctrl E: E is the new anchor
        click(&mut v, &items, Some(4), 2, false, true);
        assert_eq!(v.sel, [3, 4, 2]);
    }
}
