//! Project Media window (§5.4): toolbar, bin tree (All Media / By Type / Tagged Media / Custom Bins / Smart Bins /
//! Storyboard Bins) beside a thumbnail, list or details view. Click selects, double-click opens in the Trimmer, drag
//! adds to the timeline or (R45a) into a custom bin.
//!
//! R45a: the shown items are evaluated once per change (history generation, pool size, node, search) and the views
//! draw only the rows on screen, so a library of 10,000 items scrolls like a small one.
use std::collections::HashSet;
use eframe::egui::*;
use crate::decode::Thumbs;
use crate::edit;
use crate::engine::sync::{self, By};
use crate::history::History;
use crate::library::{self, Field, Op, Query, Term};
use crate::project::*;
use super::dock::Panel;
use super::icons::{Icon, paint_icon};
use super::menus::{self, Fixed, M::*, NotPlanned, pick_media};
use super::toolbar::{arrow_menu, inert_button, split_button, tool_button};
use super::widgets::{self, ROW_H};
use super::{MediaDrag, WinAction, theme};

#[derive(Clone, PartialEq, Debug)]
enum Node {
    All,
    ByType,
    Type(MediaType),
    /// Every tagged item; `Tag` the items with one tag.
    Tagged,
    Tag(String),
    /// Every item in a custom bin; `Bin` one custom or smart bin (by id).
    Custom,
    Smart,
    Bin(String),
    Storyboard,
    MainTimeline,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    List,
    Details,
    Thumbnails,
}

/// A custom bin dragged in the tree (onto another bin, or Custom Bins to move it to the top).
#[derive(Clone)]
struct BinDrag(String);

/// The shown items, evaluated when their key changes.
struct Cache {
    /// (history generation, pool size, bin count, node, search)
    key: (u64, usize, usize, Node, String),
    items: Vec<usize>,
    tags: Vec<(String, usize)>,
}

pub struct MediaView {
    node: Node,
    by_type_open: bool,
    storyboard_open: bool,
    tagged_open: bool,
    custom_open: bool,
    smart_open: bool,
    /// Custom bins whose bins inside are shown.
    open_bins: HashSet<String>,
    mode: Mode,
    tree_w: f32,
    /// Some while the search field is shown.
    search: Option<String>,
    /// Selected media (pool indices, click order); the app's `selected_media` is the focus, the last one.
    sel: Vec<usize>,
    /// Where Shift+click ranges start.
    anchor: Option<usize>,
    /// R45a: the list background's Media Tags: each tile shows its tags.
    show_tags: bool,
    cache: Option<Cache>,
    dlg: Option<Dlg>,
}

impl Default for MediaView {
    fn default() -> Self {
        Self {
            node: Node::All,
            by_type_open: false,
            storyboard_open: false,
            tagged_open: true,
            custom_open: true,
            smart_open: true,
            open_bins: HashSet::new(),
            mode: Mode::Thumbnails,
            tree_w: 170.0,
            search: None,
            sel: Vec::new(),
            anchor: None,
            show_tags: false,
            cache: None,
            dlg: None,
        }
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

/// The pool indices `node` and `search` show, in pool order.
fn evaluate(project: &Project, node: &Node, search: &str) -> Vec<usize> {
    let cx = library::Ctx::of(project);
    let q = Query::parse(search);
    let pool = &project.media_pool;
    let in_bins = |ids: &mut dyn Iterator<Item = &String>| -> HashSet<String> { ids.cloned().collect() };
    let member: Option<HashSet<String>> = match node {
        Node::Custom => Some(in_bins(&mut project.bins.iter().filter(|b| !b.is_smart()).flat_map(|b| &b.media))),
        Node::Bin(id) => match library::bin(project, id) {
            Some(b) if !b.is_smart() => Some(in_bins(&mut b.media.iter())),
            _ => None,
        },
        _ => None,
    };
    let smart = match node {
        Node::Bin(id) => library::bin(project, id).and_then(|b| b.query.as_deref()).map(Query::parse),
        _ => None,
    };
    let shows = |m: &MediaItem| -> bool {
        let in_node = match node {
            Node::All | Node::ByType => true,
            Node::Type(k) => m.media_type == *k,
            Node::MainTimeline => cx.used.contains(&m.id),
            Node::Tagged => !m.tags.is_empty(),
            Node::Tag(t) => m.tags.iter().any(|x| x.eq_ignore_ascii_case(t)),
            Node::Custom => member.as_ref().is_some_and(|s| s.contains(&m.id)),
            Node::Bin(_) => match (&member, &smart) {
                (Some(s), _) => s.contains(&m.id),
                (None, Some(q)) => q.matches(m, &cx),
                (None, None) => false,
            },
            Node::Smart | Node::Storyboard => false,
        };
        in_node && q.matches(m, &cx)
    };
    // A custom bin lists its media in the order they were added.
    if let Node::Bin(id) = node {
        if let Some(b) = library::bin(project, id).filter(|b| !b.is_smart()) {
            let at: std::collections::HashMap<&str, usize> = pool.iter().enumerate().map(|(i, m)| (m.id.as_str(), i)).collect();
            return b.media.iter().filter_map(|id| at.get(id.as_str()).copied()).filter(|&i| q.matches(&pool[i], &cx)).collect();
        }
    }
    (0..pool.len()).filter(|&i| shows(&pool[i])).collect()
}

impl MediaView {
    /// The shown items, from the cache while nothing they depend on changed.
    fn items(&mut self, project: &Project, generation: u64) -> Vec<usize> {
        let search = self.search.clone().unwrap_or_default();
        let key = (generation, project.media_pool.len(), project.bins.len(), self.node.clone(), search);
        if self.cache.as_ref().is_none_or(|c| c.key != key) {
            let items = evaluate(project, &key.3, &key.4);
            let tags = library::all_tags(&project.media_pool);
            self.cache = Some(Cache { key, items, tags });
        }
        self.cache.as_ref().map(|c| c.items.clone()).unwrap_or_default()
    }

    fn tags(&self) -> &[(String, usize)] {
        self.cache.as_ref().map_or(&[], |c| &c.tags)
    }

    /// The custom bin shown, if any (Remove from Current Bin acts on it).
    fn current_bin<'a>(&self, project: &'a Project) -> Option<&'a library::Bin> {
        match &self.node {
            Node::Bin(id) => library::bin(project, id).filter(|b| !b.is_smart()),
            _ => None,
        }
    }

    /// R45b review: select a node by name for the harness ("Interviews", "Tagged: interview", "Smart: 4K").
    pub fn harness_select(&mut self, project: &Project, what: &str) {
        self.mode = Mode::Thumbnails;
        if let Some(tag) = what.strip_prefix("Tagged: ") {
            self.node = Node::Tag(tag.to_string());
        } else if let Some(b) = project.bins.iter().find(|b| b.name == what.trim_start_matches("Smart: ")) {
            self.node = Node::Bin(b.id.clone());
            self.open_bins.extend(b.parent.iter().cloned().chain([b.id.clone()]));
        }
        self.show_tags = true;
    }

    /// R45a review: Search Media Bins open on `query`.
    pub fn harness_search(&mut self, query: &str) {
        self.dlg = Some(Dlg::search(query, None));
    }
}

/// Remove media `idxs`, the takes that use it and the events left with no take (`edit::forget_media`), as one undo
/// step. R45a: they leave their bins too.
fn remove_media(project: &mut Project, history: &mut History, mut idxs: Vec<usize>) {
    // A stale index (from before an undo) removes nothing.
    idxs.retain(|&i| i < project.media_pool.len());
    if idxs.is_empty() {
        return;
    }
    history.push(project.clone());
    idxs.sort_unstable();
    idxs.dedup();
    let mut gone = HashSet::new();
    for i in idxs.into_iter().rev() {
        let m = project.media_pool.remove(i);
        edit::forget_media(&mut project.timeline, &m.id);
        gone.insert(m.id);
    }
    library::forget_media(project, &gone);
    project.mark_modified();
}

/// Remove All Unused Media from Project (VEGAS's "clean" of Project Media), as one undo step.
fn remove_unused(project: &mut Project, history: &mut History) {
    let cx = library::Ctx::of(project);
    let unused: Vec<usize> = (0..project.media_pool.len()).filter(|&i| !cx.used.contains(&project.media_pool[i].id)).collect();
    remove_media(project, history, unused);
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
    actions: &mut Vec<WinAction>,
) {
    let t = theme::get();
    sync_sel(view, *selected, project.media_pool.len());
    if tool_button(ui, Icon::RemoveUnused, "Remove All Unused Media from Project", true, false).clicked() {
        remove_unused(project, history);
        (*selected, *sel_clips) = (None, Vec::new());
    }
    // Import Media…: icon + text + ▾ in one hover box.
    let (rect, resp) = ui.allocate_exact_size(vec2(124.0, t.m.button), Sense::click());
    use super::widgets::Chrome;
    widgets::chrome_button(ui.painter(), rect, if resp.hovered() { Chrome::Hover } else { Chrome::Raised }, &t.metal);
    let icon = Rect::from_min_size(pos2(rect.min.x + 3.0, rect.center().y - 8.0), vec2(16.0, 16.0));
    paint_icon(ui.painter(), icon, Icon::Import, None);
    ui.painter().text(pos2(icon.max.x + 4.0, rect.center().y), Align2::LEFT_CENTER, "Import Media...", t.font(t.font.ui), t.text);
    paint_icon(ui.painter(), Rect::from_center_size(pos2(rect.max.x - 9.0, rect.center().y), vec2(16.0, 16.0)), Icon::DropDown, None);
    if resp.on_hover_text("Import Media...").clicked() {
        if let Some(paths) = pick_media() {
            actions.push(WinAction::Import(paths));
        }
    }
    tool_button(ui, Icon::Capture, &NotPlanned("R59").tip("Capture Video"), false, false);
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
    if tool_button(ui, Icon::Search, "Search (name, path, codec, tags, frame size; type:, tag:, res>=, fps:, length<, is:unused)", true, view.search.is_some()).clicked() {
        view.search = if view.search.is_some() { None } else { Some(String::new()) };
    }
    if let Some(q) = &mut view.search {
        ui.add_space(4.0);
        widgets::search_field(ui, q, "Search media", 160.0);
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
    sync_sel(view, *selected, project.media_pool.len());
    let items = view.items(project, history.generation());
    let mut tree_cmd = None;
    widgets::scroll_pane(ui, left, "media_tree", |ui| tree_cmd = tree(ui, view, project));

    let mut cmd: Option<(usize, ItemCmd)> = None;
    let mut bg: Option<BgCmd> = None;
    widgets::scroll_pane(ui, right, "media_items", |ui| {
        if project.media_pool.is_empty() {
            let t = theme::get();
            ui.add_space(30.0);
            ui.vertical_centered(|ui| ui.label(RichText::new("Drop media here or use Import Media...").color(t.lcd.text)));
            return;
        }
        // The list background (below and between the items): VEGAS's view menu. Registered first, so the items
        // above it take their own clicks.
        let rest = ui.interact(ui.clip_rect(), ui.id().with("media_bg"), Sense::click());
        let resps = match view.mode {
            Mode::Thumbnails => thumbnails(ui, project, &items, &view.sel, thumbs, view.show_tags),
            Mode::Details => details(ui, project, &items, &view.sel),
            Mode::List => list(ui, project, &items, &view.sel),
        };
        let in_bin = view.current_bin(project).is_some();
        for (i, r) in resps {
            let (video, nested) = (crate::proxy::is_video(&project.media_pool[i]), project.media_pool[i].nested);
            let timecode = || targets(view, &items, i).iter().any(|&k| project.media_pool[k].timecode.is_some());
            let file = !project.media_pool[i].path.as_os_str().is_empty() && project.media_pool[i].generator.is_none();
            let caps = Caps { selected: view.sel.contains(&i), video, nested, in_bin, file };
            item_interaction(ui, &r, i, caps, &timecode, &mut cmd);
            // The payload is every selected item when the grabbed one is selected, so they are added across time.
            if r.drag_started() {
                r.dnd_set_drag_payload(MediaDrag::Pool(targets(view, &items, i), None));
            }
        }
        menus::context(&rest, |ui| {
            for (mode, label) in [(Mode::List, "&List"), (Mode::Details, "&Details"), (Mode::Thumbnails, "&Thumbnail")] {
                if menus::entry(ui, label, "", true, menus::Mark::Radio(view.mode == mode)) {
                    bg = Some(BgCmd::Mode(mode));
                }
            }
            menus::sep(ui);
            if menus::entry(ui, "&Media Tags", "", true, menus::Mark::Check(view.show_tags)) {
                bg = Some(BgCmd::Tags);
            }
            menus::sep(ui);
            if menus::entry(ui, "&Search Media Bins...", "", true, menus::Mark::None) {
                bg = Some(BgCmd::Search);
            }
            if menus::entry(ui, "Find M&issing Media...", "", true, menus::Mark::None) {
                bg = Some(BgCmd::Missing);
            }
            if menus::entry(ui, "Remove All &Unused Media", "", true, menus::Mark::None) {
                bg = Some(BgCmd::Unused);
            }
        });
    });
    if selected.is_some() {
        super::claim_plain_keys(ui.ctx(), right);
    }
    if ui.rect_contains_pointer(right) && selected.is_some() && ui.input(|i| i.key_pressed(Key::Delete)) && !ui.ctx().wants_keyboard_input() && view.dlg.is_none() {
        cmd = selected.map(|i| (i, ItemCmd::Remove));
    }
    match bg {
        Some(BgCmd::Mode(m)) => view.mode = m,
        Some(BgCmd::Tags) => view.show_tags = !view.show_tags,
        Some(BgCmd::Search) => view.dlg = Some(Dlg::search(view.search.as_deref().unwrap_or(""), None)),
        Some(BgCmd::Missing) => actions.push(WinAction::MissingMedia),
        Some(BgCmd::Unused) => {
            remove_unused(project, history);
            (*selected, *sel_clips) = (None, Vec::new());
        }
        None => {}
    }
    if let Some(c) = tree_cmd {
        tree_command(project, history, view, c, &items);
    }
    let ids = |view: &MediaView, i: usize| -> Vec<String> { targets(view, &items, i).into_iter().filter_map(|k| project.media_pool.get(k).map(|m| m.id.clone())).collect() };
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
        Some((i, ItemCmd::LayOut(by))) => {
            // One undo step, the earliest stamp at the cursor; media without one is skipped and said so (as VEGAS does).
            let (before, at) = (project.clone(), project.timeline.cursor_position);
            let (ids, skipped) = edit::lay_out_tracks(project, &targets(view, &items, i), at, |m| sync::stamp(m, by));
            if !ids.is_empty() {
                history.push(before);
            }
            if skipped > 0 {
                let what = if by == By::Timecode { "media timecode" } else { "a date/time stamp" };
                super::hint(ui.ctx(), format!("Lay Out Tracks: {skipped} of the media had no {what} and were skipped."));
            }
        }
        Some((i, ItemCmd::Remove)) => {
            remove_media(project, history, targets(view, &items, i));
            (*selected, *sel_clips) = (None, Vec::new());
        }
        Some((i, ItemCmd::RemoveFromBin)) => {
            let ids = ids(view, i);
            if let Some(b) = view.current_bin(project).map(|b| b.id.clone()) {
                let before = project.clone();
                if library::remove_from_bin(project, &b, &ids) {
                    history.push(before);
                }
            }
        }
        Some((i, ItemCmd::Tags)) => {
            let ids = ids(view, i);
            let text = project.media_pool[i].tags.join(", ");
            view.dlg = Some(Dlg::Tags { ids, text });
        }
        Some((_, ItemCmd::Search)) => view.dlg = Some(Dlg::search(view.search.as_deref().unwrap_or(""), None)),
        Some((i, ItemCmd::FileProps)) => actions.push(WinAction::FileProps(project.media_pool[i].id.clone())),
        Some((i, ItemCmd::RulerTimecode)) => {
            if let Some(tc) = project.media_pool[i].timecode {
                history.push(project.clone());
                project.settings.ruler.offset = tc;
                project.mark_modified();
            }
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
                let tags = std::mem::take(&mut m.tags);
                *m = crate::project::MediaItem { id: std::mem::take(&mut m.id), path: path.clone(), name, media_type: m.media_type, tags, ..Default::default() };
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
    show_dialog(ui.ctx(), project, history, view);
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
    /// R17a: Lay Out Tracks Using Media Timecode / Date/Time Stamp.
    LayOut(By),
    /// R41: the ruler starts at this media's timecode (an event at 0 then reads its source timecode on the ruler).
    RulerTimecode,
    /// R45a: Remove from Current Bin, the item's Media Tags, Search Media Bins.
    RemoveFromBin,
    Tags,
    Search,
    /// R45b: File Format Properties.
    FileProps,
}

/// The list background's menu.
enum BgCmd {
    Mode(Mode),
    Tags,
    Search,
    Missing,
    Unused,
}

/// What an item's menu offers. `selected`: a right-click on a selected item keeps the selection, so its menu acts on
/// all of it. `video`: file-based video without alpha, which Create Video Proxy needs; `nested`: a nested timeline;
/// `in_bin`: a custom bin is shown; `file`: it has a file (File Format Properties).
struct Caps {
    selected: bool,
    video: bool,
    nested: bool,
    in_bin: bool,
    file: bool,
}

/// `timecode`: the item (or another item the menu acts on) has media timecode, asked only while the menu is open.
fn item_interaction(ui: &Ui, r: &Response, i: usize, caps: Caps, timecode: &dyn Fn() -> bool, cmd: &mut Option<(usize, ItemCmd)>) {
    if r.double_clicked() {
        *cmd = Some((i, ItemCmd::Trim));
    } else if r.clicked() || (r.secondary_clicked() && !caps.selected) {
        *cmd = Some((i, ItemCmd::Select(if r.clicked() { ui.input(|i| i.modifiers) } else { Modifiers::NONE })));
    }
    if r.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    }
    // Project Media item menu (§6.9); Milan Pro adds "Add to Video/Audio Track" and "Media Tags...".
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
        item(ui, "Edit Source Pro&ject", ItemCmd::EditSource, caps.nested);
        item(ui, "Explore &Containing Folder", ItemCmd::Explore, true);
        item(ui, "Create Video Pro&xy", ItemCmd::Proxy, caps.video);
        menus::sep(ui);
        item(ui, "Remove from Current &Bin", ItemCmd::RemoveFromBin, caps.in_bin);
        item(ui, "&Remove from Project", ItemCmd::Remove, true);
        menus::show(ui, ITEM_DELETE);
        menus::sep(ui);
        item(ui, "&Search Media Bins...", ItemCmd::Search, true);
        menus::sep(ui);
        menus::show(ui, ITEM_RENAME);
        item(ui, "Media Ta&gs...", ItemCmd::Tags, true);
        item(ui, "R&eplace...", ItemCmd::Replace, true);
        menus::show(ui, ITEM_TAIL);
        // R17a: greyed, as in VEGAS, when none of the media has timecode.
        item(ui, "Lay Out Tracks Using Media &Timecode", ItemCmd::LayOut(By::Timecode), timecode());
        item(ui, "Lay Out Tracks Using Media &Date/Time Stamp", ItemCmd::LayOut(By::Date), true);
        item(ui, "Use Media Timecode as Ruler Sta&rt", ItemCmd::RulerTimecode, timecode());
        menus::show(ui, ITEM_VIEW);
        item(ui, "File Format Pr&operties...", ItemCmd::FileProps, caps.file);
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

const ITEM_DELETE: &[menus::M] = &[X("Remove from Project and &Delete File(s)", Fixed("disabled on purpose: Milan Pro never deletes your files"))];

const ITEM_RENAME: &[menus::M] = &[X("Re&name", NotPlanned("R57"))];

const ITEM_TAIL: &[menus::M] = &[
    X("Refres&h", NotPlanned("R57")),
    S,
    X("Media &FX...", NotPlanned("R57")),
    X("Rotate 90° Co&unterclockwise", NotPlanned("R57")),
    X("Rotate 90° Clock&wise", NotPlanned("R57")),
    S,
];

const ITEM_VIEW: &[menus::M] = &[S, Sub("V&iew", &[X("Columns", NotPlanned("R57"))]), X("&Auto-Size Columns", NotPlanned("R57")), S];

const ITEM_V23: &[menus::M] = &[
    S,
    X("Tempo Detection", NotPlanned("R57")),
    X("Detect Scenes and Add to Timeline", NotPlanned("R57")),
    X("Detect Scenes and Add to Timeline from Cursor", NotPlanned("R57")),
    X("Detect Scenes and Create Subclips", NotPlanned("R57")),
];

/// The Project Media item menu's tables, for the Unfinished Features list.
pub const ITEM_MENUS: &[&[menus::M]] = &[ITEM_TOP, ITEM_ADD, ITEM_DELETE, ITEM_RENAME, ITEM_TAIL, ITEM_VIEW, ITEM_V23];

/// A bin tree menu command or drop.
enum TreeCmd {
    NewBin(Option<String>),
    SaveAsBin,
    SaveAsSmart,
    Delete(String),
    Rename(String),
    EditSearch(String),
    Search,
    /// Media (pool indices) dropped on a custom bin.
    AddTo(String, Vec<usize>),
    /// A bin dropped on another (None: on Custom Bins).
    Move(String, Option<String>),
}

fn tree(ui: &mut Ui, view: &mut MediaView, project: &Project) -> Option<TreeCmd> {
    let mut out = None;
    let t = theme::get();
    let bin = Icon::TrackVideo;
    let node = |ui: &mut Ui, view: &mut MediaView, depth, exp: Option<bool>, icon, label: &str, n: Node| -> (Response, bool) {
        let (r, toggled) = widgets::tree_row(ui, depth, exp, Some(icon), label, view.node == n);
        if r.clicked() {
            view.node = n;
        }
        let toggle = toggled || (exp.is_some() && r.double_clicked());
        (r, toggle)
    };
    // Drops on a custom bin row (media, or another bin); a box round the row while something hovers over it.
    let drop_target = |r: &Response, bin: Option<&str>, out: &mut Option<TreeCmd>| {
        let media = r.dnd_hover_payload::<MediaDrag>().is_some_and(|d| matches!(*d, MediaDrag::Pool(..)) && bin.is_some());
        if media || r.dnd_hover_payload::<BinDrag>().is_some() {
            r.ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("bin_drop"))).rect_stroke(r.rect.shrink(1.0), 0.0, Stroke::new(1.0_f32, t.accent), StrokeKind::Inside);
        }
        if let Some(d) = r.dnd_release_payload::<MediaDrag>() {
            if let (MediaDrag::Pool(idxs, _), Some(b)) = (&*d, bin) {
                *out = Some(TreeCmd::AddTo(b.to_string(), idxs.clone()));
            }
        }
        if let Some(d) = r.dnd_release_payload::<BinDrag>() {
            if bin != Some(d.0.as_str()) {
                *out = Some(TreeCmd::Move(d.0.clone(), bin.map(str::to_string)));
            }
        }
    };
    let tree_menu = |r: &Response, on: Option<&library::Bin>, out: &mut Option<TreeCmd>| {
        menus::context(r, |ui| {
            let mut e = |ui: &mut Ui, label: &str, on: bool, c: TreeCmd| {
                if menus::entry(ui, label, "", on, menus::Mark::None) {
                    *out = Some(c);
                }
            };
            let parent = on.filter(|b| !b.is_smart()).map(|b| b.id.clone());
            e(ui, "Create &New Bin", true, TreeCmd::NewBin(parent));
            e(ui, "&Save as Bin", true, TreeCmd::SaveAsBin);
            e(ui, "Save as S&mart Bin", true, TreeCmd::SaveAsSmart);
            menus::sep(ui);
            let id = on.map(|b| b.id.clone()).unwrap_or_default();
            e(ui, "&Delete", on.is_some(), TreeCmd::Delete(id.clone()));
            e(ui, "&Rename", on.is_some(), TreeCmd::Rename(id.clone()));
            e(ui, "&Edit Search", on.is_some_and(library::Bin::is_smart), TreeCmd::EditSearch(id));
            menus::sep(ui);
            e(ui, "&Search Media Bins...", true, TreeCmd::Search);
        });
    };

    let (r, tg) = node(ui, view, 0, None, bin, "All Media", Node::All);
    tree_menu(&r, None, &mut out);
    let _ = tg;
    let open = view.by_type_open;
    let (r, tg) = node(ui, view, 0, Some(open), bin, "Media By Type", Node::ByType);
    tree_menu(&r, None, &mut out);
    view.by_type_open ^= tg;
    if view.by_type_open {
        node(ui, view, 1, None, Icon::TrackAudio, "Audio", Node::Type(MediaType::Audio));
        node(ui, view, 1, None, Icon::TrackVideo, "Video", Node::Type(MediaType::Video));
        node(ui, view, 1, None, Icon::GeneratedMedia, "Stills", Node::Type(MediaType::Image));
    }
    // Tagged Media lists the tags in use.
    let tags: Vec<(String, usize)> = view.tags().to_vec();
    let open = (!tags.is_empty()).then_some(view.tagged_open);
    let (r, tg) = node(ui, view, 0, open, bin, "Tagged Media", Node::Tagged);
    tree_menu(&r, None, &mut out);
    view.tagged_open ^= tg;
    if view.tagged_open {
        for (tag, n) in &tags {
            node(ui, view, 1, None, Icon::Folder, &format!("{tag} ({n})"), Node::Tag(tag.clone()));
        }
    }
    // Custom Bins: nested folders, in the order they were made.
    let custom: Vec<&library::Bin> = project.bins.iter().filter(|b| !b.is_smart()).collect();
    let open = (!custom.is_empty()).then_some(view.custom_open);
    let (r, tg) = node(ui, view, 0, open, Icon::Folder, "Custom Bins", Node::Custom);
    tree_menu(&r, None, &mut out);
    drop_target(&r, None, &mut out);
    view.custom_open ^= tg;
    if view.custom_open {
        let mut stack: Vec<(&library::Bin, usize)> = custom.iter().rev().filter(|b| b.parent.is_none()).map(|b| (*b, 1)).collect();
        while let Some((b, depth)) = stack.pop() {
            let kids: Vec<&library::Bin> = custom.iter().filter(|c| c.parent.as_deref() == Some(b.id.as_str())).copied().collect();
            let open = view.open_bins.contains(&b.id);
            let (r, tg) = node(ui, view, depth, (!kids.is_empty()).then_some(open), Icon::Folder, &format!("{} ({})", b.name, b.media.len()), Node::Bin(b.id.clone()));
            if tg && !view.open_bins.remove(&b.id) {
                view.open_bins.insert(b.id.clone());
            }
            tree_menu(&r, Some(b), &mut out);
            drop_target(&r, Some(&b.id), &mut out);
            if r.drag_started() {
                r.dnd_set_drag_payload(BinDrag(b.id.clone()));
            }
            if open {
                stack.extend(kids.iter().rev().map(|k| (*k, depth + 1)));
            }
        }
    }
    let smart: Vec<&library::Bin> = project.bins.iter().filter(|b| b.is_smart()).collect();
    let open = (!smart.is_empty()).then_some(view.smart_open);
    let (r, tg) = node(ui, view, 0, open, Icon::Folder, "Smart Bins", Node::Smart);
    tree_menu(&r, None, &mut out);
    view.smart_open ^= tg;
    if view.smart_open {
        for b in smart {
            let (r, _) = node(ui, view, 1, None, Icon::Search, &b.name, Node::Bin(b.id.clone()));
            let r = r.on_hover_text(b.query.as_deref().unwrap_or(""));
            tree_menu(&r, Some(b), &mut out);
        }
    }
    let open = view.storyboard_open;
    let (r, tg) = node(ui, view, 0, Some(open), Icon::Folder, "Storyboard Bins", Node::Storyboard);
    tree_menu(&r, None, &mut out);
    view.storyboard_open ^= tg;
    if view.storyboard_open {
        node(ui, view, 1, None, Icon::Folder, "Main Timeline", Node::MainTimeline);
    }
    out
}

fn tree_command(project: &mut Project, history: &mut History, view: &mut MediaView, c: TreeCmd, items: &[usize]) {
    let before = project.clone();
    let changed = match c {
        TreeCmd::NewBin(parent) => {
            view.dlg = Some(Dlg::Name { rename: None, parent, media: Vec::new(), name: "New Bin".into() });
            false
        }
        TreeCmd::SaveAsBin => {
            let media = items.iter().filter_map(|&i| project.media_pool.get(i).map(|m| m.id.clone())).collect();
            view.dlg = Some(Dlg::Name { rename: None, parent: None, media, name: "New Bin".into() });
            false
        }
        TreeCmd::SaveAsSmart => {
            view.dlg = Some(Dlg::search(view.search.as_deref().unwrap_or(""), None));
            false
        }
        TreeCmd::Search => {
            view.dlg = Some(Dlg::search(view.search.as_deref().unwrap_or(""), None));
            false
        }
        TreeCmd::EditSearch(id) => {
            let q = library::bin(project, &id).and_then(|b| b.query.clone()).unwrap_or_default();
            view.dlg = Some(Dlg::search(&q, Some(id)));
            false
        }
        TreeCmd::Rename(id) => {
            let name = library::bin(project, &id).map(|b| b.name.clone()).unwrap_or_default();
            view.dlg = Some(Dlg::Name { rename: Some(id), parent: None, media: Vec::new(), name });
            false
        }
        TreeCmd::Delete(id) => {
            let gone = library::subtree(project, &id);
            if matches!(&view.node, Node::Bin(b) if gone.contains(b)) {
                view.node = Node::All;
            }
            library::delete_bin(project, &id)
        }
        TreeCmd::AddTo(bin, idxs) => {
            let ids: Vec<String> = idxs.iter().filter_map(|&i| project.media_pool.get(i).map(|m| m.id.clone())).collect();
            library::add_to_bin(project, &bin, &ids)
        }
        TreeCmd::Move(bin, parent) => {
            if let Some(p) = &parent {
                view.open_bins.insert(p.clone());
            }
            library::move_bin(project, &bin, parent.as_deref())
        }
    };
    if changed {
        history.push(before);
    }
}

// ---- Dialogs ----

/// The Project Media dialogs: a bin's name, Search Media Bins (criteria rows that make a query; Save as Smart Bin),
/// and Media Tags.
enum Dlg {
    /// New bin (`rename` None; inside `parent`, holding `media` for Save as Bin) or Rename.
    Name { rename: Option<String>, parent: Option<String>, media: Vec<String>, name: String },
    Search { rows: Vec<Row>, edit: Option<String>, name: String },
    Tags { ids: Vec<String>, text: String },
}

/// One criterion of Search Media Bins: a field, an operator and a value.
#[derive(Clone, Debug, PartialEq)]
struct Row {
    field: usize,
    op: usize,
    value: String,
}

/// The criteria fields: (label, field, kind). Kinds: 0 text, 1 type, 2 number, 3 status.
const FIELDS: &[(&str, Field, u8)] = &[
    ("Any text", Field::Any, 0),
    ("Name", Field::Name, 0),
    ("Path", Field::Path, 0),
    ("Codec", Field::Codec, 0),
    ("Tag", Field::Tag, 0),
    ("Type", Field::Type, 1),
    ("Frame size", Field::Res, 2),
    ("Width", Field::Width, 2),
    ("Height", Field::Height, 2),
    ("Frame rate", Field::Fps, 2),
    ("Length", Field::Length, 2),
    ("Status", Field::Is, 3),
];

/// Operators per kind: (label, op, negated).
fn ops(kind: u8) -> &'static [(&'static str, Op, bool)] {
    match kind {
        0 => &[("contains", Op::Has, false), ("does not contain", Op::Has, true), ("is", Op::Eq, false)],
        1 | 3 => &[("is", Op::Has, false), ("is not", Op::Has, true)],
        2 => &[("=", Op::Eq, false), (">", Op::Gt, false), (">=", Op::Ge, false), ("<", Op::Lt, false), ("<=", Op::Le, false)],
        _ => &[("=", Op::Eq, false)],
    }
}

const TYPES: &[&str] = &["video", "audio", "still"];
const STATUS: &[&str] = &["used", "unused", "offline", "online", "tagged"];

impl Row {
    fn kind(&self) -> u8 {
        FIELDS[self.field.min(FIELDS.len() - 1)].2
    }

    fn term(&self) -> Option<Term> {
        let (_, field, kind) = &FIELDS[self.field.min(FIELDS.len() - 1)];
        let (_, op, not) = ops(*kind)[self.op.min(ops(*kind).len() - 1)];
        let v = self.value.trim();
        (!v.is_empty()).then(|| Term::new(not, field.clone(), op, v))
    }

    fn of(t: &Term) -> Row {
        let field = FIELDS.iter().position(|f| f.1 == t.field).unwrap_or(0);
        let op = ops(FIELDS[field].2).iter().position(|o| o.1 == t.op && o.2 == t.not).unwrap_or(0);
        Row { field, op, value: t.value.clone() }
    }
}

/// The query criteria rows make, every one of them to match.
fn rows_query(rows: &[Row]) -> String {
    Query { terms: rows.iter().filter_map(Row::term).collect() }.text()
}

impl Dlg {
    fn search(query: &str, edit: Option<String>) -> Dlg {
        let mut rows: Vec<Row> = Query::parse(query).terms.iter().map(Row::of).collect();
        if rows.is_empty() {
            rows.push(Row { field: 0, op: 0, value: String::new() });
        }
        Dlg::Search { rows, edit, name: "New Smart Bin".into() }
    }
}

fn show_dialog(ctx: &Context, project: &mut Project, history: &mut History, view: &mut MediaView) {
    let Some(dlg) = &mut view.dlg else { return };
    let done: bool;
    let mut select = None;
    let before = project.clone();
    let mut changed = false;
    match dlg {
        Dlg::Name { rename, parent, media, name } => {
            let title = if rename.is_some() { "Rename Bin" } else { "Create New Bin" };
            let (hit, close) = super::dialogs::modal(ctx, "media_bin_name", title, vec2(320.0, 130.0), |ui| {
                ui.label("Bin name:");
                let r = widgets::edit(ui, TextEdit::singleline(name).desired_width(f32::INFINITY));
                r.request_focus();
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                ui.add_space(12.0);
                super::dialogs::button_row(ui, &[("OK", !name.trim().is_empty()), ("Cancel", true)]).or(enter.then_some(0))
            });
            match hit {
                Some(0) if !name.trim().is_empty() => {
                    match rename {
                        Some(id) => changed = library::rename_bin(project, id, name),
                        None => {
                            let id = library::new_bin(project, name, parent.as_deref());
                            library::add_to_bin(project, &id, media);
                            if let Some(p) = parent {
                                view.open_bins.insert(p.clone());
                            }
                            select = Some(Node::Bin(id));
                            changed = true;
                        }
                    }
                    done = true;
                }
                Some(_) => done = true,
                None => done = close,
            }
        }
        Dlg::Search { rows, edit, name } => {
            let title = if edit.is_some() { "Edit Smart Bin Search" } else { "Search Media Bins" };
            let h = 150.0 + rows.len() as f32 * 26.0 + if edit.is_none() { 30.0 } else { 0.0 };
            let (hit, close) = super::dialogs::modal(ctx, "media_search", title, vec2(560.0, h), |ui| {
                ui.label("Find media that match every criterion:");
                ui.add_space(4.0);
                let mut remove = None;
                for (k, row) in rows.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        let before = row.field;
                        widgets::combo(ui, ("crit_field", k), 110.0, FIELDS[row.field].0, |ui| {
                            for (j, f) in FIELDS.iter().enumerate() {
                                ui.selectable_value(&mut row.field, j, f.0);
                            }
                        });
                        if row.field != before {
                            row.op = 0;
                            if FIELDS[row.field].2 != FIELDS[before].2 {
                                row.value.clear();
                            }
                        }
                        let kind = row.kind();
                        let os = ops(kind);
                        widgets::combo(ui, ("crit_op", k), 110.0, os[row.op.min(os.len() - 1)].0, |ui| {
                            for (j, o) in os.iter().enumerate() {
                                ui.selectable_value(&mut row.op, j, o.0);
                            }
                        });
                        match kind {
                            1 | 3 => {
                                let list = if kind == 1 { TYPES } else { STATUS };
                                if !list.contains(&row.value.as_str()) {
                                    row.value = list[0].to_string();
                                }
                                widgets::combo(ui, ("crit_val", k), 150.0, row.value.clone(), |ui| {
                                    for v in list {
                                        ui.selectable_value(&mut row.value, v.to_string(), *v);
                                    }
                                });
                            }
                            _ => {
                                let hint = match FIELDS[row.field].1 {
                                    Field::Res => "1920x1080 or 1080",
                                    Field::Fps => "25",
                                    Field::Length => "seconds or m:ss",
                                    _ => "",
                                };
                                widgets::edit(ui, TextEdit::singleline(&mut row.value).desired_width(150.0).hint_text(hint));
                            }
                        }
                        if super::dialogs::classic_button(ui, "-", vec2(23.0, 21.0), true, false).on_hover_text("Remove this criterion").clicked() {
                            remove = Some(k);
                        }
                    });
                }
                if let Some(k) = remove.filter(|_| rows.len() > 1) {
                    rows.remove(k);
                }
                if super::dialogs::classic_button(ui, "Add Criterion", vec2(100.0, 23.0), true, false).clicked() {
                    rows.push(Row { field: 0, op: 0, value: String::new() });
                }
                ui.add_space(4.0);
                let q = rows_query(rows);
                ui.label(RichText::new(if q.is_empty() { "(every item)".to_string() } else { q }).color(theme::get().dialog().text_disabled));
                if edit.is_none() {
                    ui.horizontal(|ui| {
                        ui.label("Smart bin name:");
                        widgets::edit(ui, TextEdit::singleline(name).desired_width(200.0));
                    });
                }
                ui.add_space(8.0);
                if edit.is_some() {
                    super::dialogs::button_row(ui, &[("OK", true), ("Cancel", true)]).map(|k| if k == 0 { 2 } else { 3 })
                } else {
                    super::dialogs::button_row(ui, &[("Search", true), ("Save as Smart Bin", !name.trim().is_empty()), ("Cancel", true)])
                }
            });
            let q = rows_query(rows);
            match hit {
                // Search: the query goes into the search field over the node shown.
                Some(0) => {
                    view.search = Some(q);
                    done = true;
                }
                Some(1) => {
                    let id = library::new_smart_bin(project, name, &q);
                    (select, changed, done) = (Some(Node::Bin(id)), true, true);
                }
                Some(2) => {
                    if let Some(id) = edit {
                        changed = library::set_query(project, id, &q);
                    }
                    done = true;
                }
                Some(_) => done = true,
                None => done = close,
            }
        }
        Dlg::Tags { ids, text } => {
            let known = view.cache.as_ref().map(|c| c.tags.iter().map(|t| t.0.clone()).collect::<Vec<_>>()).unwrap_or_default();
            let title = if ids.len() > 1 { format!("Media Tags ({} items)", ids.len()) } else { "Media Tags".to_string() };
            let (hit, close) = super::dialogs::modal(ctx, "media_tags", &title, vec2(380.0, 170.0), |ui| {
                ui.label("Tags, separated by commas:");
                let r = widgets::edit(ui, TextEdit::singleline(text).desired_width(f32::INFINITY).hint_text("interview, b roll, day 1"));
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                if !known.is_empty() {
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label("In use:");
                        for k in &known {
                            let have = library::parse_tags(text).iter().any(|t| t.eq_ignore_ascii_case(k));
                            if ui.selectable_label(have, k).clicked() {
                                let mut tags = library::parse_tags(text);
                                if have {
                                    tags.retain(|t| !t.eq_ignore_ascii_case(k));
                                } else {
                                    tags.push(k.clone());
                                }
                                *text = tags.join(", ");
                            }
                        }
                    });
                }
                ui.add_space(10.0);
                super::dialogs::button_row(ui, &[("OK", true), ("Cancel", true)]).or(enter.then_some(0))
            });
            match hit {
                Some(0) => {
                    changed = library::set_tags(project, ids, &library::parse_tags(text));
                    done = true;
                }
                Some(_) => done = true,
                None => done = close,
            }
        }
    }
    if changed {
        history.push(before);
    }
    if let Some(n) = select {
        view.node = n;
    }
    if done {
        view.dlg = None;
    }
}

// ---- Views ----

/// Source time shown on a tile: a quarter in, so the frame is rarely a black lead-in.
fn thumb_time(m: &MediaItem) -> f64 {
    (m.duration * 0.25 * 10.0).floor() / 10.0
}

/// The rows of `n` rows `row_h` tall from `top` that are on screen (the scroll area's clip).
fn visible(ui: &Ui, top: f32, row_h: f32, n: usize) -> std::ops::Range<usize> {
    let clip = ui.clip_rect();
    let first = (((clip.min.y - top) / row_h).floor().max(0.0) as usize).min(n);
    let last = ((((clip.max.y - top) / row_h).ceil().max(0.0)) as usize).min(n);
    first..last.max(first)
}

/// Tiles: ≈100×65 thumbnail, name centred below in up to 2 lines; 28 px / 30 px gaps (§5.4). With Media Tags on, a
/// line of tags under the name.
fn thumbnails(ui: &mut Ui, project: &Project, items: &[usize], sel: &[usize], thumbs: &mut Thumbs, show_tags: bool) -> Vec<(usize, Response)> {
    let t = theme::get();
    let (tw, th, gx, gy) = (100.0, 65.0, 28.0, 30.0);
    let cap = if show_tags { 46.0 } else { 32.0 };
    let width = ui.available_width().max(tw + gx);
    let cols = (((width - 14.0) / (tw + gx)).floor() as usize).max(1);
    let rows = items.len().div_ceil(cols);
    let pitch = th + cap + gy;
    let (area, _) = ui.allocate_exact_size(vec2(width, 10.0 + rows as f32 * pitch), Sense::hover());
    let mut out = Vec::new();
    for r in visible(ui, area.min.y + 6.0, pitch, rows) {
        for c in 0..cols {
            let Some(&i) = items.get(r * cols + c) else { break };
            let min = area.min + vec2(14.0 + c as f32 * (tw + gx), 10.0 + r as f32 * pitch);
            let tile = Rect::from_min_size(min - vec2(6.0, 4.0), vec2(tw + 12.0, th + cap + 8.0));
            let thumb = Rect::from_min_size(min, vec2(tw, th));
            let resp = ui.interact(tile, ui.id().with(("media_tile", i)), Sense::click_and_drag());
            let m = &project.media_pool[i];
            widgets::tile(ui, tile, thumb, &m.name, sel.contains(&i), resp.hovered());
            let p = ui.painter();
            match m.media_type {
                MediaType::Audio | MediaType::Unknown => widgets::paint_audio_page(p, thumb),
                _ => {
                    if !m.offline() {
                        if let Some(tex) = thumbs.get(m, thumb_time(m)) {
                            widgets::paint_thumb(p, thumb, tex);
                        }
                    }
                }
            }
            if m.missing {
                paint_offline(p, thumb);
            }
            badges(p, m, project, thumb.right_top() + vec2(-2.0, 2.0), -1.0);
            if show_tags && !m.tags.is_empty() {
                let at = Rect::from_min_size(pos2(tile.min.x + 3.0, tile.max.y - 16.0), vec2(tile.width() - 6.0, 13.0));
                p.with_clip_rect(at.intersect(ui.clip_rect())).text(at.center(), Align2::CENTER_CENTER, m.tags.join(", "), t.font(t.font.ui), t.ink.glyph);
            }
            out.push((i, resp));
        }
    }
    out
}

/// R45b: an offline (missing) item's thumbnail: hatched, "Media Offline".
fn paint_offline(p: &Painter, r: Rect) {
    let t = theme::get();
    p.rect_filled(r, 0.0, Color32::from_gray(40));
    let vis = r.intersect(p.clip_rect());
    let pc = p.with_clip_rect(vis);
    let mut x = r.min.x - r.height();
    while x < r.max.x {
        pc.line_segment([pos2(x, r.max.y), pos2(x + r.height(), r.min.y)], Stroke::new(1.0_f32, Color32::from_gray(80)));
        x += 8.0;
    }
    pc.text(r.center(), Align2::CENTER_CENTER, "Media Offline", t.font(10.0), Color32::from_rgb(240, 200, 60));
}

/// A small lettered badge with its top-right corner at `tr`; returns its width.
fn badge(p: &Painter, tr: Pos2, text: &str, fill: Color32) -> f32 {
    let t = theme::get();
    let w = (text.len() as f32 * 7.0 + 4.0).max(13.0);
    let r = Rect::from_min_max(tr - vec2(w, 0.0), tr + vec2(0.0, 13.0));
    p.rect_filled(r, 0.0, fill);
    p.text(r.center(), Align2::CENTER_CENTER, text, t.font(10.0), Color32::WHITE);
    w
}

/// The badges of `m` from `at` on: "P" (a video proxy), R40a "HDR" (an HDR source shown through the view transform;
/// Media Properties says how), R37/R45b "!" (offline: a codec this FFmpeg build cannot decode, or the file is
/// missing). `dir` is -1 to lay them out leftwards from a right corner, +1 rightwards from a left one.
fn badges(p: &Painter, m: &MediaItem, project: &Project, at: Pos2, dir: f32) {
    let mut x = at.x;
    let mut put = |text: &str, fill: Color32| {
        let tr = if dir < 0.0 { pos2(x, at.y) } else { pos2(x + (text.len() as f32 * 7.0 + 4.0).max(13.0), at.y) };
        let w = badge(p, tr, text, fill);
        x += dir * (w + 2.0);
    };
    if m.unsupported.is_some() || m.missing {
        put("!", Color32::from_rgb(200, 40, 40));
    }
    if m.media_type == MediaType::Video && !m.missing && crate::proxy::ready(&m.path).is_some() {
        put("P", theme::get().accent);
    }
    if matches!(crate::engine::color::badge(m, project.settings.color_management), Some("HDR → SDR")) {
        put("HDR", Color32::from_rgb(170, 110, 20));
    }
}

const DETAIL_COLS: [(&str, f32); 8] =
    [("Name", 180.0), ("Use Count", 70.0), ("Type", 60.0), ("Length", 90.0), ("Frame Rate", 80.0), ("Frame Size", 90.0), ("Tags", 140.0), ("Path", 400.0)];

/// A row of the list or details view at row `k` below `top`: its response, the selection bar when selected.
fn row_at(ui: &Ui, top: Pos2, width: f32, k: usize, i: usize, selected: bool) -> Response {
    let t = theme::get();
    let rect = Rect::from_min_size(top + vec2(0.0, k as f32 * ROW_H), vec2(width, ROW_H));
    if selected {
        ui.painter().rect_filled(rect, 0.0, t.menu_popup_hover);
    }
    ui.interact(rect, ui.id().with(("media_row", i)), Sense::click_and_drag())
}

fn details(ui: &mut Ui, project: &Project, items: &[usize], sel: &[usize]) -> Vec<(usize, Response)> {
    widgets::header_row(ui, &DETAIL_COLS);
    let fps = project.settings.frame_rate;
    let width = ui.available_width().max(DETAIL_COLS.iter().map(|c| c.1).sum());
    let (area, _) = ui.allocate_exact_size(vec2(width, items.len() as f32 * ROW_H), Sense::hover());
    let mut out = Vec::new();
    for k in visible(ui, area.min.y, ROW_H, items.len()) {
        let i = items[k];
        let m = &project.media_pool[i];
        let r = row_at(ui, area.min, width, k, i, sel.contains(&i));
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
            m.tags.join(", "),
            if m.missing { format!("{} (offline)", m.path.display()) } else { m.path.display().to_string() },
        ];
        widgets::row_cells(ui, r.rect, &DETAIL_COLS, &values, widgets::list_ink(ui, sel.contains(&i)));
        badges(ui.painter(), m, project, pos2(r.rect.min.x + DETAIL_COLS[0].1 - 6.0, r.rect.min.y + 2.0), -1.0);
        out.push((i, r));
    }
    out
}

fn list(ui: &mut Ui, project: &Project, items: &[usize], sel: &[usize]) -> Vec<(usize, Response)> {
    let t = theme::get();
    let width = ui.available_width().max(60.0);
    let (area, _) = ui.allocate_exact_size(vec2(width, items.len() as f32 * ROW_H), Sense::hover());
    let mut out = Vec::new();
    for k in visible(ui, area.min.y, ROW_H, items.len()) {
        let i = items[k];
        let m = &project.media_pool[i];
        let r = row_at(ui, area.min, width, k, i, sel.contains(&i));
        let icon = if m.media_type == MediaType::Audio { Icon::TrackAudio } else { Icon::TrackVideo };
        paint_icon(ui.painter(), Rect::from_min_size(pos2(r.rect.min.x + 6.0, r.rect.center().y - 8.0), vec2(16.0, 16.0)), icon, None);
        let color = widgets::list_ink(ui, sel.contains(&i));
        let name = ui.painter().text(pos2(r.rect.min.x + 28.0, r.rect.center().y), Align2::LEFT_CENTER, &m.name, t.font(t.font.ui), color);
        badges(ui.painter(), m, project, pos2(name.max.x + 5.0, r.rect.min.y + 2.0), 1.0);
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

    /// The criteria rows make the query, and a query reads back into the same rows.
    #[test]
    fn search_rows_round_trip_through_the_query() {
        let q = "interview -tag:\"b roll\" res>=1920x1080 type:video -is:offline length<90";
        let Dlg::Search { rows, .. } = Dlg::search(q, None) else { panic!() };
        assert_eq!(rows.len(), 6);
        assert_eq!((FIELDS[rows[1].field].0, ops(0)[rows[1].op].0, rows[1].value.as_str()), ("Tag", "does not contain", "b roll"));
        assert_eq!(rows_query(&rows), q);
        assert_eq!(rows_query(&[Row { field: 0, op: 0, value: " ".into() }]), "", "an empty criterion is left out");
    }

    pub(crate) fn synthetic(n: usize) -> Project {
        let mut p = Project::new("big");
        for k in 0..n {
            let (t, codec) = match k % 4 {
                0 | 1 => (MediaType::Video, "h264"),
                2 => (MediaType::Audio, "aac"),
                _ => (MediaType::Image, "png"),
            };
            let mut m = MediaItem { id: format!("m{k}"), name: format!("Clip {k:05}.mp4"), path: format!(r"D:\Shoot\Day {}\Clip {k:05}.mp4", k % 7).into(), media_type: t, codec: Some(codec.into()), ..Default::default() };
            if t != MediaType::Audio {
                (m.width, m.height) = if k % 3 == 0 { (Some(3840), Some(2160)) } else { (Some(1920), Some(1080)) };
                m.frame_rate = Some(if k % 2 == 0 { 25.0 } else { 29.97 });
            } else {
                m.sample_rate = Some(48_000);
            }
            m.duration = (k % 600) as f64 + 1.0;
            if k % 5 == 0 {
                m.tags = vec!["interview".into(), format!("day {}", k % 7)];
            }
            p.media_pool.push(m);
        }
        let b = library::new_bin(&mut p, "Selects", None);
        let ids: Vec<String> = (0..n).step_by(3).map(|k| format!("m{k}")).collect();
        library::add_to_bin(&mut p, &b, &ids);
        library::new_smart_bin(&mut p, "4K unused", "res>=3840x2160 is:unused type:video");
        p
    }

    /// R45a acceptance (bench): 10,000 items, each view drawn headless; after the first frame (which evaluates the
    /// node) a frame of the Project Media body stays under 4 ms. Search and smart-bin evaluation over 5,000 and
    /// 10,000 items are timed too. `cargo test --release large_library -- --nocapture` prints BENCH lines.
    #[test]
    fn large_library_frame_time_stays_under_4ms() {
        let mut p = synthetic(10_000);
        let ctx = Context::default();
        let mut thumbs = Thumbs::new(&ctx);
        thumbs.set_paused(true); // no decodes: the frame cost is the view's own
        let mut history = History::default();
        let (mut sel, mut clips, mut actions) = (None, Vec::new(), Vec::new());
        let smart = p.bins.iter().find(|b| b.is_smart()).map(|b| b.id.clone()).unwrap();
        let budget = if cfg!(debug_assertions) { 40.0 } else { 4.0 };
        for (mode, node, search) in [
            (Mode::Thumbnails, Node::All, None),
            (Mode::Details, Node::All, None),
            (Mode::List, Node::All, Some("interview".to_string())),
            (Mode::Thumbnails, Node::Bin(smart.clone()), None),
            (Mode::Details, Node::Tag("interview".into()), Some("res>=1920x1080 fps:25".to_string())),
        ] {
            let mut view = MediaView { mode, node: node.clone(), search, ..Default::default() };
            let mut frame = |scroll: f32| {
                let mut input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 700.0))), ..Default::default() };
                if scroll != 0.0 {
                    input.events.push(Event::PointerMoved(pos2(700.0, 300.0)));
                    input.events.push(Event::MouseWheel { unit: MouseWheelUnit::Point, delta: vec2(0.0, -scroll), modifiers: Modifiers::NONE });
                }
                let t0 = std::time::Instant::now();
                let _ = ctx.run(input, |ctx| {
                    CentralPanel::default().show(ctx, |ui| {
                        draw_media_browser(ui, &mut p, &mut view, &mut sel, &mut clips, &mut history, &mut thumbs, &mut actions, 5.0, false);
                    });
                });
                t0.elapsed().as_secs_f64() * 1000.0
            };
            let first = frame(0.0);
            let mut times: Vec<f64> = (0..30).map(|k| frame(if k % 3 == 0 { 2000.0 } else { 0.0 })).collect();
            times.sort_by(f64::total_cmp);
            let median = times[times.len() / 2];
            println!("BENCH media_panel {mode:?} {node:?}: first {first:.2} ms, median {median:.3} ms, max {:.3} ms", times[times.len() - 1]);
            assert!(median < budget, "{mode:?} {node:?}: {median:.3} ms per frame");
        }
        for n in [5_000, 10_000] {
            let p = synthetic(n);
            for (what, node, search) in [("search", Node::All, "clip 0 day"), ("smart bin", Node::Bin(p.bins[1].id.clone()), ""), ("tag", Node::Tag("interview".into()), "")] {
                let t0 = std::time::Instant::now();
                let hits = evaluate(&p, &node, search).len();
                let ms = t0.elapsed().as_secs_f64() * 1000.0;
                println!("BENCH library {what} over {n}: {ms:.2} ms ({hits} hits)");
                assert!(hits > 0 && ms < if cfg!(debug_assertions) { 400.0 } else { 40.0 }, "{what} over {n}: {ms:.2} ms");
            }
        }
    }
}
