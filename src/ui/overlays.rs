//! Overlays window (Milan Pro's own; VEGAS has none): the overlay pack library, laid out like Project Media and Media
//! Generators. Toolbar (Import Pack ▾, Create Sample Pack, Remove Pack, Pack Properties, Open Pack Folder, Refresh,
//! Views ▾, search), pack tree (All Overlays, Favorites, Recent, then each pack; a missing folder greyed with
//! Locate...) beside thumbnail tiles that scrub on hover, or a details list. A tile drags onto the timeline like an
//! Explorer file; a double-click adds it at the cursor on a new video track above the current one, with its blend.
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
use eframe::egui::*;
use crate::decode::Thumbs;
use crate::history::History;
use crate::overlays::{self, Blend, Done, Library, Pack, Removal};
use crate::project::{MediaItem, MediaType, Project, format_timecode};
use super::dock::{Layout, Panel};
use super::icons::{Icon, paint_icon};
use super::menus::{self, Mark};
use super::toolbar::{arrow_menu, split_button, tool_button};
use super::widgets::{self, ROW_H};
use super::{MediaDrag, Selection, WinAction, chrome, dialogs, theme};

#[derive(Clone, PartialEq)]
enum Node {
    All,
    Favorites,
    Recent,
    Pack(PathBuf),
}

#[derive(Clone, Copy, PartialEq)]
enum Sort {
    Name,
    Modified,
    Length,
}

/// What a click asked for; carried out after drawing, where the project can be edited.
enum Cmd {
    ImportFolder,
    ImportZip,
    AddExisting,
    Sample,
    Remove(PathBuf),
    Props(PathBuf),
    Rename(PathBuf),
    OpenFolder(PathBuf),
    Locate(PathBuf),
    Refresh,
    AddAtCursor(PathBuf),
    AddToProject(PathBuf),
    Favorite(PathBuf),
    Tags(PathBuf),
    ItemBlend(PathBuf, Option<Blend>),
    Explore(PathBuf),
}

enum Dlg {
    Props { pack: PathBuf, name: String, blend: Option<Blend> },
    Remove { pack: PathBuf, used: usize },
    Rename { pack: PathBuf, text: String },
    Tags { item: PathBuf, text: String },
}

pub struct OverlaysView {
    pub lib: Library,
    node: Node,
    details: bool,
    sort: Sort,
    query: String,
    tree_w: f32,
    sel: Option<PathBuf>,
    dlg: Option<Dlg>,
    /// Toolbar clicks, carried out by the body.
    pending: Vec<Cmd>,
    /// The window has been shown: the library is scanned from then on.
    opened: bool,
}

/// Hover scrubbing shows this many frames across a tile.
const SCRUB: f64 = 8.0;

impl OverlaysView {
    pub fn new(dir: PathBuf, index: Option<PathBuf>) -> Self {
        Self { lib: Library::new(dir, index), node: Node::All, details: false, sort: Sort::Name, query: String::new(), tree_w: 170.0, sel: None, dlg: None, pending: Vec::new(), opened: false }
    }

    /// A dialog of this window is up (shortcuts wait).
    pub fn modal(&self) -> bool {
        self.dlg.is_some()
    }

    /// `MILAN_DIALOG=overlays` (screenshot harness): a library in `dir` that is never saved, holding the sample pack
    /// (made once by FFmpeg) and a linked pack whose folder is missing; the first overlay selected.
    pub fn harness(&mut self, dir: &Path, details: bool) {
        let sample = dir.join(overlays::SAMPLE_PACK);
        let made = if sample.is_dir() { Ok(()) } else { overlays::create_sample(dir, &AtomicU32::new(0)).map(|_| ()) };
        self.lib = Library::new(dir.to_path_buf(), None);
        let ix = &mut self.lib.index;
        ix.packs = vec![
            Pack { name: overlays::SAMPLE_PACK.into(), path: sample.clone(), linked: false, blend: None },
            Pack { name: "Old Dust Pack".into(), path: dir.join("Old Dust Pack (moved)"), linked: true, blend: None },
        ];
        ix.blends.extend(overlays::sample_blends(&sample));
        ix.favorites.push(sample.join("Light Leak.mp4"));
        ix.tags.insert(sample.join("Film Grain.mp4"), vec!["16mm".into(), "subtle".into()]);
        self.lib.message = made.err();
        (self.details, self.sel) = (details, Some(sample.join("Film Grain.mp4")));
    }
}

/// R-overlays: put the Overlays window right after Media Generators in a saved layout (once, for layouts from before
/// it existed). A layout without Media Generators, or with Overlays already, is left as it is.
pub fn place_in_layout(l: &mut Layout) -> bool {
    if l.groups.iter().any(|g| g.tabs.contains(&Panel::Overlays)) || l.floating.iter().any(|f| f.panel == Panel::Overlays) {
        return false;
    }
    let Some(g) = l.groups.iter_mut().find(|g| g.tabs.contains(&Panel::MediaGenerators)) else { return false };
    let k = g.tabs.iter().position(|&p| p == Panel::MediaGenerators).map_or(0, |k| k + 1);
    g.tabs.insert(k, Panel::Overlays);
    if g.active >= k {
        g.active += 1;
    }
    true
}

fn pack<'a>(lib: &'a Library, path: &Path) -> Option<&'a Pack> {
    lib.index.packs.iter().find(|p| p.path == path)
}

/// Source time shown on a tile: a quarter in, as Project Media does.
fn thumb_time(m: &MediaItem) -> f64 {
    (m.duration * 0.25 * 10.0).floor() / 10.0
}

/// The frame shown with the pointer `frac` of the way across a tile: one of `SCRUB` evenly spaced, so they cache.
fn scrub_time(m: &MediaItem, frac: f32) -> f64 {
    let k = (frac.clamp(0.0, 1.0) as f64 * SCRUB).floor().min(SCRUB - 1.0);
    ((k + 0.5) / SCRUB * m.duration * 10.0).floor() / 10.0
}

/// The pack the toolbar acts on: the one selected in the tree, else the selected overlay's.
fn target(v: &OverlaysView) -> Option<PathBuf> {
    match &v.node {
        Node::Pack(p) => Some(p.clone()),
        _ => v.sel.as_ref().and_then(|s| v.lib.items.iter().find(|i| i.path == *s)).map(|i| i.pack.clone()),
    }
}

/// Toolbar row, left to right, as Project Media's.
pub fn draw_overlays_toolbar(ui: &mut Ui, v: &mut OverlaysView) {
    let t = theme::get();
    // Import Pack...: icon + text + ▾ in one hover box, its menu below.
    let (rect, resp) = ui.allocate_exact_size(vec2(112.0, t.m.button), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(t.m.widget_radius), t.button_hover_bg);
    }
    let icon = Rect::from_min_size(pos2(rect.min.x + 3.0, rect.center().y - 8.0), vec2(16.0, 16.0));
    paint_icon(ui.painter(), icon, Icon::Import, t.look(), None);
    ui.painter().text(pos2(icon.max.x + 4.0, rect.center().y), Align2::LEFT_CENTER, "Import Pack...", t.font(t.font.ui), t.text);
    paint_icon(ui.painter(), Rect::from_center_size(pos2(rect.max.x - 9.0, rect.center().y), vec2(16.0, 16.0)), Icon::DropDown, t.look(), None);
    let resp = resp.on_hover_text("Import an overlay pack");
    let mut pick = None;
    chrome::with_menu_style(ui.ctx(), &t, || {
        arrow_menu(ui, &resp, |ui| {
            for (label, c) in [("Import Pack from &Folder...", 0), ("Import Pack from .&zip...", 1), ("&Add Existing Folder as Pack...", 2)] {
                if menus::entry(ui, label, "", true, Mark::None) {
                    pick = Some(c);
                }
            }
        })
    });
    match pick {
        Some(0) => v.pending.push(Cmd::ImportFolder),
        Some(1) => v.pending.push(Cmd::ImportZip),
        Some(_) => v.pending.push(Cmd::AddExisting),
        None => {}
    }
    if tool_button(ui, Icon::GeneratedMedia, "Create Sample Pack (film grain, a light leak and a frame border, made by FFmpeg)", true, false).clicked() {
        v.pending.push(Cmd::Sample);
    }
    let tp = target(v);
    if tool_button(ui, Icon::Delete, "Remove Pack", tp.is_some(), false).clicked() {
        v.pending.extend(tp.clone().map(Cmd::Remove));
    }
    if tool_button(ui, Icon::Properties, "Pack Properties", tp.is_some(), false).clicked() {
        v.pending.extend(tp.clone().map(Cmd::Props));
    }
    let present = tp.as_ref().is_some_and(|p| !v.lib.missing.contains(p));
    if tool_button(ui, Icon::Folder, "Open Pack Folder", present, false).clicked() {
        v.pending.extend(tp.map(Cmd::OpenFolder));
    }
    if tool_button(ui, Icon::Refresh, "Refresh", true, false).clicked() {
        v.pending.push(Cmd::Refresh);
    }
    let (m, a) = split_button(ui, Icon::Views, "Views", true, false);
    let mut views = |ui: &mut Ui| {
        for (details, label) in [(false, "Thumbnails"), (true, "Details")] {
            if ui.radio(v.details == details, label).clicked() {
                v.details = details;
            }
        }
        ui.separator();
        for (sort, label) in [(Sort::Name, "Sort by Name"), (Sort::Modified, "Sort by Date Modified"), (Sort::Length, "Sort by Length")] {
            if ui.radio(v.sort == sort, label).clicked() {
                v.sort = sort;
            }
        }
    };
    if arrow_menu(ui, &a, &mut views).is_none() {
        arrow_menu(ui, &m, &mut views);
    }
    let (icon, _) = ui.allocate_exact_size(vec2(22.0, t.m.button), Sense::hover());
    paint_icon(ui.painter(), Rect::from_center_size(icon.center(), vec2(16.0, 16.0)), Icon::Search, t.look(), None);
    widgets::search_field(ui, &mut v.query, "Search overlays and tags", 150.0);
    for j in &v.lib.jobs {
        ui.add_space(10.0);
        ui.label(RichText::new(format!("{}... {}%", j.what, j.progress.load(Relaxed) / 10)).color(t.text_dim));
    }
}

/// The overlays the selected node shows, searched (name or tag) and sorted: indices into `lib.items`.
fn shown(v: &OverlaysView) -> Vec<usize> {
    let (ix, items) = (&v.lib.index, &v.lib.items);
    let q = v.query.trim().to_lowercase();
    let mut out: Vec<usize> = (0..items.len())
        .filter(|&i| {
            let it = &items[i];
            let in_node = match &v.node {
                Node::All => true,
                Node::Favorites => ix.favorites.contains(&it.path),
                Node::Recent => ix.recent.contains(&it.path),
                Node::Pack(p) => it.pack == *p,
            };
            let tagged = || ix.tags.get(&it.path).is_some_and(|ts| ts.iter().any(|t| t.to_lowercase().contains(&q)));
            in_node && (q.is_empty() || it.name.to_lowercase().contains(&q) || tagged())
        })
        .collect();
    let len = |i: &usize| v.lib.known(&items[*i].path).map_or(0.0, |m| m.duration);
    match v.sort {
        _ if v.node == Node::Recent => out.sort_by_key(|i| ix.recent.iter().position(|p| *p == items[*i].path)),
        Sort::Name => out.sort_by_key(|i| items[*i].name.to_lowercase()),
        Sort::Modified => out.sort_by(|a, b| items[*b].modified.cmp(&items[*a].modified)),
        Sort::Length => out.sort_by(|a, b| len(b).total_cmp(&len(a))),
    }
    out
}

/// Body: pack tree, splitter, and the overlays of the selected node. Clicks are carried out here.
#[allow(clippy::too_many_arguments)]
pub fn draw_overlays(ui: &mut Ui, v: &mut OverlaysView, project: &mut Project, history: &mut History, sel: &mut Selection, thumbs: &mut Thumbs, actions: &mut Vec<WinAction>, still_len: f64) {
    if !v.opened {
        v.opened = true;
        v.lib.refresh();
    }
    let area = ui.max_rect();
    let mut cmds = std::mem::take(&mut v.pending);
    let mut tree_w = v.tree_w;
    let (left, right) = widgets::pane_split(ui, area, &mut tree_w, "overlay_tree");
    v.tree_w = tree_w;
    widgets::scroll_pane(ui, left, "overlay_tree", |ui| tree(ui, v, &mut cmds));
    let items = shown(v);
    let fps = project.settings.frame_rate;
    widgets::scroll_pane(ui, right, "overlay_items", |ui| {
        let t = theme::get();
        let missing = match &v.node {
            Node::Pack(p) if v.lib.missing.contains(p) => Some(p.clone()),
            _ => None,
        };
        if let Some(p) = missing {
            ui.add_space(30.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(format!("The folder of this pack is missing:\n{}", p.display())).color(t.text_dim));
                if ui.button("Locate...").clicked() {
                    cmds.push(Cmd::Locate(p.clone()));
                }
            });
        } else if items.is_empty() {
            let text = if v.lib.index.packs.is_empty() {
                "No overlay packs yet: use Import Pack... or Create Sample Pack."
            } else if v.lib.busy() {
                "Reading the overlay packs..."
            } else {
                "No overlays here."
            };
            ui.add_space(30.0);
            ui.vertical_centered(|ui| ui.label(RichText::new(text).color(t.text_dim)));
        } else if v.details {
            details(ui, v, &items, fps, &mut cmds);
        } else {
            grid(ui, v, &items, thumbs, &mut cmds);
        }
    });
    for c in cmds {
        run(ui.ctx(), v, c, project, history, sel, actions, still_len);
    }
}

fn tree(ui: &mut Ui, v: &mut OverlaysView, cmds: &mut Vec<Cmd>) {
    let t = theme::get();
    let bin = if t.look() == theme::UiLook::Vegas16 { Icon::MediaBin } else { Icon::TrackVideo };
    for (icon, label, n) in [(bin, "All Overlays", Node::All), (Icon::Favorite, "Favorites", Node::Favorites), (bin, "Recent", Node::Recent)] {
        let (r, _) = widgets::tree_row(ui, 0, None, Some(icon), label, v.node == n);
        if r.clicked() {
            v.node = n;
        }
    }
    let packs: Vec<(PathBuf, String, bool)> = v.lib.index.packs.iter().map(|p| (p.path.clone(), p.name.clone(), v.lib.missing.contains(&p.path))).collect();
    for (path, name, missing) in packs {
        let node = Node::Pack(path.clone());
        let (r, _) = widgets::tree_row(ui, 0, None, Some(Icon::Folder), &name, v.node == node);
        if missing {
            // Greyed: the panel colour over the row, half through.
            ui.painter().rect_filled(r.rect, 0.0, t.panel_bg.gamma_multiply(0.55));
        }
        let r = r.on_hover_text(if missing { format!("Missing: {}", path.display()) } else { path.display().to_string() });
        if r.clicked() || r.secondary_clicked() {
            v.node = node;
        }
        menus::context(&r, |ui| {
            let mut item = |ui: &mut Ui, label: &str, c: Cmd, on: bool| {
                if menus::entry(ui, label, "", on, Mark::None) {
                    cmds.push(c);
                }
            };
            if missing {
                item(ui, "&Locate...", Cmd::Locate(path.clone()), true);
                menus::sep(ui);
            }
            item(ui, "Re&name Pack...", Cmd::Rename(path.clone()), true);
            item(ui, "Open Pack &Folder", Cmd::OpenFolder(path.clone()), !missing);
            item(ui, "&Remove Pack...", Cmd::Remove(path.clone()), true);
            menus::sep(ui);
            item(ui, "Re&fresh", Cmd::Refresh, true);
            item(ui, "&Properties...", Cmd::Props(path.clone()), true);
        });
    }
}

/// A small corner label on a tile ("α": the overlay has an alpha channel).
fn badge(p: &Painter, bl: Pos2, text: &str) {
    let r = Rect::from_min_max(bl - vec2(0.0, 14.0), bl + vec2(14.0, 0.0));
    p.rect_filled(r, CornerRadius::same(2), Color32::from_black_alpha(170));
    p.text(r.center(), Align2::CENTER_CENTER, text, FontId::proportional(11.0), Color32::WHITE);
}

/// Tiles as Media Generators': VP16 100×60 thumbnails, VP23 120×75; only those on screen are probed and decoded.
fn grid(ui: &mut Ui, v: &mut OverlaysView, items: &[usize], thumbs: &mut Thumbs, cmds: &mut Vec<Cmd>) {
    let t = theme::get();
    let (thumb, gap) = if t.look() == theme::UiLook::Vegas16 { (vec2(100.0, 60.0), 16.0) } else { (vec2(120.0, 75.0), 12.0) };
    let tile = thumb + vec2(16.0, 36.0);
    let width = ui.available_width().max(tile.x + gap);
    let cols = (((width - gap) / (tile.x + gap)).floor() as usize).max(1);
    let (area, _) = ui.allocate_exact_size(vec2(width, gap + items.len().div_ceil(cols) as f32 * (tile.y + gap)), Sense::hover());
    for (n, &i) in items.iter().enumerate() {
        let min = area.min + vec2(gap + (n % cols) as f32 * (tile.x + gap), gap + (n / cols) as f32 * (tile.y + gap));
        let r = Rect::from_min_size(min, tile);
        let path = v.lib.items[i].path.clone();
        let resp = ui.interact(r, ui.id().with(("overlay", &path)), Sense::click_and_drag());
        let th = Rect::from_min_size(min + vec2(8.0, 6.0), thumb);
        widgets::tile(ui, r, th, &v.lib.items[i].name, v.sel.as_ref() == Some(&path), resp.hovered());
        if ui.clip_rect().intersects(r) {
            let p = ui.painter().with_clip_rect(th.intersect(ui.clip_rect()));
            match v.lib.probe(&path).cloned() {
                Some(m) if m.media_type != MediaType::Audio => {
                    // Hover scrubbing: the frame under the pointer, else (still decoding) the tile's own.
                    let scrub = resp.hover_pos().filter(|_| m.media_type == MediaType::Video && m.duration > 0.0).map(|p| scrub_time(&m, (p.x - th.min.x) / th.width()));
                    let tex = scrub.and_then(|s| thumbs.get(&m, s).cloned()).or_else(|| thumbs.get(&m, thumb_time(&m)).cloned());
                    if let Some(tex) = tex {
                        widgets::paint_thumb(&p, th, &tex);
                    }
                    if m.has_alpha {
                        badge(&p, th.left_bottom() + vec2(2.0, -2.0), "α");
                    }
                }
                _ if v.lib.unreadable(&path) => {
                    p.text(th.center(), Align2::CENTER_CENTER, "Unreadable", t.font(t.font.ui), t.text_dim);
                }
                _ => {}
            }
        }
        // Favourite star, top right: shown when set or hovered.
        let fav = v.lib.index.favorites.contains(&path);
        if fav || resp.hovered() {
            let star = Rect::from_min_size(pos2(th.max.x - 17.0, th.min.y + 1.0), vec2(16.0, 16.0));
            paint_icon(ui.painter(), star, Icon::Favorite, t.look(), Some(if fav { t.accent } else { t.text_dim }));
            if ui.interact(star, resp.id.with("fav"), Sense::click()).on_hover_text(if fav { "Remove from Favorites" } else { "Add to Favorites" }).clicked() {
                cmds.push(Cmd::Favorite(path.clone()));
            }
        }
        item_interaction(ui, v, &resp, &path, cmds);
    }
}

const COLS: [(&str, f32); 8] = [("Name", 200.0), ("Pack", 150.0), ("Length", 90.0), ("Frame Size", 90.0), ("Frame Rate", 80.0), ("Alpha", 50.0), ("Blend", 120.0), ("Tags", 200.0)];

fn details(ui: &mut Ui, v: &mut OverlaysView, items: &[usize], fps: f64, cmds: &mut Vec<Cmd>) {
    let t = theme::get();
    match widgets::header_row(ui, &COLS) {
        Some(0) => v.sort = Sort::Name,
        Some(2) => v.sort = Sort::Length,
        _ => {}
    }
    for &i in items {
        let it = v.lib.items[i].clone();
        let selected = v.sel.as_ref() == Some(&it.path);
        let r = widgets::list_row(ui, ROW_H, selected);
        let m = if ui.clip_rect().intersects(r.rect) { v.lib.probe(&it.path).cloned() } else { v.lib.known(&it.path).cloned() };
        let ix = &v.lib.index;
        let pack = ix.packs.iter().find(|p| p.path == it.pack).map_or(String::new(), |p| p.name.clone());
        let values = match &m {
            Some(m) => [
                it.name.clone(),
                pack,
                if m.media_type == MediaType::Image { "Still".into() } else { format_timecode(m.duration, fps) },
                m.width.zip(m.height).map_or(String::new(), |(w, h)| format!("{w}x{h}")),
                m.frame_rate.filter(|_| m.media_type == MediaType::Video).map_or(String::new(), |f| format!("{f:.3}")),
                if m.has_alpha { "Yes".into() } else { "No".into() },
                ix.blend_for(&it.path, m.has_alpha).map_or(String::new(), |b| b.label().into()),
                ix.tags.get(&it.path).map_or(String::new(), |ts| ts.join(", ")),
            ],
            None => {
                let state = if v.lib.unreadable(&it.path) { "Unreadable" } else { "" };
                [it.name.clone(), pack, state.into(), String::new(), String::new(), String::new(), String::new(), ix.tags.get(&it.path).map_or(String::new(), |ts| ts.join(", "))]
            }
        };
        widgets::row_cells(ui, r.rect, &COLS, &values, if selected { Color32::WHITE } else { t.text });
        if ix.favorites.contains(&it.path) {
            let star = Rect::from_min_size(pos2(r.rect.min.x + COLS[0].1 - 22.0, r.rect.center().y - 8.0), vec2(16.0, 16.0));
            paint_icon(ui.painter(), star, Icon::Favorite, t.look(), Some(if selected { Color32::WHITE } else { t.accent }));
        }
        item_interaction(ui, v, &r, &it.path, cmds);
    }
}

/// Click selects, double-click adds at the cursor, a drag goes to the timeline as an Explorer file does (its probe,
/// ghost, snapping, new tracks and undo), and the right-click menu.
fn item_interaction(ui: &Ui, v: &mut OverlaysView, r: &Response, path: &Path, cmds: &mut Vec<Cmd>) {
    if r.double_clicked() {
        cmds.push(Cmd::AddAtCursor(path.to_path_buf()));
    }
    if r.clicked() || r.secondary_clicked() {
        v.sel = Some(path.to_path_buf());
    }
    if r.drag_started() {
        r.dnd_set_drag_payload(MediaDrag::File(path.to_path_buf()));
    }
    if r.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    }
    let ix = &v.lib.index;
    let fav = ix.favorites.iter().any(|f| f == path);
    let own = ix.blends.get(path).copied();
    let alpha = v.lib.known(path).is_some_and(|m| m.has_alpha);
    let pack = ix.pack_of(path).cloned();
    let default = pack.as_ref().map_or(Blend::Normal, |p| p.blend.unwrap_or_else(|| Blend::suggest(&p.name, alpha)));
    menus::context(r, |ui| {
        let item = |ui: &mut Ui, label: &str| menus::entry(ui, label, "", true, Mark::None);
        let p = path.to_path_buf();
        if item(ui, "Add at &Cursor on New Track") {
            cmds.push(Cmd::AddAtCursor(p.clone()));
        }
        if item(ui, "Add to &Project Media") {
            cmds.push(Cmd::AddToProject(p.clone()));
        }
        menus::sep(ui);
        if item(ui, if fav { "Remove from &Favorites" } else { "Add to &Favorites" }) {
            cmds.push(Cmd::Favorite(p.clone()));
        }
        if item(ui, "&Tags...") {
            cmds.push(Cmd::Tags(p.clone()));
        }
        menus::sub(ui, "&Blend", true, |ui| {
            if menus::entry(ui, &format!("Pack Default ({})", default.label()), "", true, Mark::Radio(own.is_none())) {
                cmds.push(Cmd::ItemBlend(p.clone(), None));
            }
            menus::sep(ui);
            for b in Blend::ALL {
                if menus::entry(ui, b.label(), "", true, Mark::Radio(own == Some(b))) {
                    cmds.push(Cmd::ItemBlend(p.clone(), Some(b)));
                }
            }
        });
        menus::sep(ui);
        if item(ui, "Explore &Containing Folder") {
            cmds.push(Cmd::Explore(p.clone()));
        }
        if let Some(pk) = pack.as_ref().filter(|_| item(ui, "Pack P&roperties...")) {
            cmds.push(Cmd::Props(pk.path.clone()));
        }
    });
}

/// The video track a double-click puts its new track above: the focus track, else the focus event's; None = the top.
fn current_video_track(project: &Project, sel: &Selection) -> Option<usize> {
    let track = sel.tracks.last().and_then(|id| project.find_track(id)).filter(|t| t.0).map(|t| t.1);
    track.or_else(|| sel.clips.last().and_then(|id| project.find_clip(id)).filter(|r| r.is_video).map(|r| r.track_idx))
}

fn folder_name(p: &Path) -> String {
    p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned())
}

#[allow(clippy::too_many_arguments)]
fn run(ctx: &Context, v: &mut OverlaysView, c: Cmd, project: &mut Project, history: &mut History, sel: &mut Selection, actions: &mut Vec<WinAction>, still_len: f64) {
    let lib_dir = v.lib.dir.clone();
    match c {
        Cmd::ImportFolder => {
            if let Some(src) = rfd::FileDialog::new().set_title("Import Pack from Folder").pick_folder() {
                v.lib.start(ctx, format!("Importing '{}'", folder_name(&src)), move |p| overlays::import_folder(&src, &lib_dir, p).map(|f| Done::Pack(f, Vec::new())));
            }
        }
        Cmd::ImportZip => {
            if let Some(zip) = rfd::FileDialog::new().set_title("Import Pack from .zip").add_filter("Zip archives", &["zip"]).pick_file() {
                v.lib.start(ctx, format!("Extracting '{}'", folder_name(&zip)), move |p| overlays::import_zip(&zip, &lib_dir, p).map(|f| Done::Pack(f, Vec::new())));
            }
        }
        Cmd::AddExisting => {
            if let Some(dir) = rfd::FileDialog::new().set_title("Add Existing Folder as Pack").pick_folder() {
                if pack(&v.lib, &dir).is_some() {
                    super::hint(ctx, format!("{} is a pack already.", dir.display()));
                } else {
                    // Linked: it stays where it is (a folder already in the library is one of its own packs).
                    let linked = dir.parent().is_none_or(|p| !(overlays::inside(p, &lib_dir) && overlays::inside(&lib_dir, p)));
                    v.lib.index.packs.push(Pack { name: folder_name(&dir), path: dir.clone(), linked, blend: None });
                    v.lib.save();
                    v.lib.refresh();
                    v.node = Node::Pack(dir);
                }
            }
        }
        Cmd::Sample => v.lib.start(ctx, "Creating the sample pack".into(), move |p| overlays::create_sample(&lib_dir, p).map(|(f, b)| Done::Pack(f, b))),
        Cmd::Remove(p) => {
            let Some(pk) = pack(&v.lib, &p) else { return };
            match overlays::removal(pk, &lib_dir, v.lib.missing.contains(&p), project) {
                Removal::Forget => {
                    super::hint(ctx, format!("Removed the pack '{}' from the library; its folder is kept.", pk.name));
                    v.lib.forget(&p);
                    if v.node == Node::Pack(p) {
                        v.node = Node::All;
                    }
                }
                Removal::Recycle { used } => v.dlg = Some(Dlg::Remove { pack: p, used }),
            }
        }
        Cmd::Props(p) => {
            if let Some(pk) = pack(&v.lib, &p) {
                v.dlg = Some(Dlg::Props { name: pk.name.clone(), blend: pk.blend, pack: p });
            }
        }
        Cmd::Rename(p) => {
            if let Some(pk) = pack(&v.lib, &p) {
                v.dlg = Some(Dlg::Rename { text: pk.name.clone(), pack: p });
            }
        }
        Cmd::OpenFolder(p) => dialogs::open_file(&p),
        Cmd::Explore(p) => dialogs::open_folder(&p),
        Cmd::Locate(p) => {
            let found = rfd::FileDialog::new().set_title("Locate Pack Folder").pick_folder().filter(|d| pack(&v.lib, d).is_none());
            if let Some(dir) = found {
                if let Some(pk) = v.lib.index.packs.iter_mut().find(|k| k.path == p) {
                    // shortcut: favourites, tags and item blends keep the old paths; carry them over if users relocate often.
                    pk.linked = dir.parent().is_none_or(|d| !(overlays::inside(d, &lib_dir) && overlays::inside(&lib_dir, d)));
                    pk.path = dir.clone();
                }
                v.lib.save();
                v.lib.refresh();
                v.node = Node::Pack(dir);
            }
        }
        Cmd::Refresh => v.lib.refresh(),
        Cmd::AddAtCursor(p) => {
            let Some(m) = v.lib.probe(&p).cloned() else {
                let why = if v.lib.unreadable(&p) { "cannot be read" } else { "is still being read; try again in a moment" };
                super::hint(ctx, format!("{} {why}.", folder_name(&p)));
                return;
            };
            let blend = v.lib.index.blend_for(&p, m.has_alpha).unwrap_or(Blend::Normal);
            let (before, at, above) = (project.clone(), project.timeline.cursor_position, current_video_track(project, sel));
            let ids = overlays::add_on_new_track(project, &m, above, at, still_len, blend);
            if !ids.is_empty() {
                history.push(before);
                (sel.clips, sel.list_focus) = (ids, false);
                v.lib.used(&p);
            }
        }
        Cmd::AddToProject(p) => {
            actions.push(WinAction::Import(vec![p.clone()]));
            v.lib.used(&p);
        }
        Cmd::Favorite(p) => {
            let f = &mut v.lib.index.favorites;
            match f.iter().position(|x| *x == p) {
                Some(k) => {
                    f.remove(k);
                }
                None => f.push(p),
            }
            v.lib.save();
        }
        Cmd::Tags(p) => {
            let text = v.lib.index.tags.get(&p).map_or(String::new(), |ts| ts.join(", "));
            v.dlg = Some(Dlg::Tags { item: p, text });
        }
        Cmd::ItemBlend(p, b) => {
            match b {
                Some(b) => v.lib.index.blends.insert(p, b),
                None => v.lib.index.blends.remove(&p),
            };
            v.lib.save();
        }
    }
}

/// Every frame (whether or not the window is shown): the library follows Preferences, takes in finished background
/// work and says what happened in the status bar; this window's dialogs.
pub fn overlay_dialogs(ctx: &Context, v: &mut OverlaysView, dir: PathBuf) {
    v.lib.set_dir(dir);
    v.lib.tick(ctx);
    if let Some(m) = v.lib.message.take() {
        super::hint(ctx, m);
    }
    if !v.lib.jobs.is_empty() {
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }
    let Some(dlg) = v.dlg.as_mut() else { return };
    let done = match dlg {
        Dlg::Props { pack: path, name, blend } => {
            let Some(pk) = pack(&v.lib, path).cloned() else {
                v.dlg = None;
                return;
            };
            let (hit, close) = dialogs::modal(ctx, "overlay_pack_props", "Pack Properties", vec2(440.0, 290.0), |ui| {
                dialogs::row(ui, "Name:", 100.0, true, |ui| ui.add(TextEdit::singleline(name).desired_width(290.0)));
                let suggested = Blend::suggest(&pk.name, false);
                let shown = blend.map_or_else(|| format!("Suggested ({})", suggested.label()), |b| b.label().into());
                dialogs::row(ui, "Default blend:", 100.0, true, |ui| {
                    ComboBox::from_id_salt("overlay_pack_blend").width(200.0).selected_text(shown).show_ui(ui, |ui| {
                        ui.selectable_value(blend, None, format!("Suggested ({})", suggested.label()));
                        for b in Blend::ALL {
                            ui.selectable_value(blend, Some(b), b.label());
                        }
                    });
                });
                ui.add_space(4.0);
                ui.label(if pk.linked { "Linked folder (it stays where it is):" } else { "Copied into the overlay library:" });
                ui.label(pk.path.display().to_string());
                ui.add_space(4.0);
                ui.label("Suggested: Normal for clips with alpha, Screen for light leaks, flares, bokeh, dust and grain. An overlay's own blend (right-click › Blend) comes first.");
                ui.add_space(8.0);
                dialogs::button_row(ui, &[("OK", !name.trim().is_empty()), ("Cancel", true)])
            });
            if hit == Some(0) {
                if let Some(p) = v.lib.index.packs.iter_mut().find(|p| p.path == pk.path) {
                    (p.name, p.blend) = (name.trim().to_string(), *blend);
                }
                v.lib.save();
            }
            hit.is_some() || close
        }
        Dlg::Remove { pack: path, used } => {
            let Some(pk) = pack(&v.lib, path).cloned() else {
                v.dlg = None;
                return;
            };
            let used = *used;
            let (hit, close) = dialogs::modal(ctx, "overlay_pack_remove", "Remove Pack", vec2(460.0, 210.0), |ui| {
                ui.label(format!("Move the folder of the pack '{}' to the Recycle Bin?", pk.name));
                ui.label(RichText::new(pk.path.display().to_string()).weak());
                ui.add_space(4.0);
                ui.label(match used {
                    0 => "The current project uses none of its files.".to_string(),
                    n => format!("The current project uses {n} file{} from this pack: its events stay, and show as offline until the files are back.", if n == 1 { "" } else { "s" }),
                });
                ui.label("You can restore the folder from the Recycle Bin.");
                ui.add_space(8.0);
                dialogs::button_row(ui, &[("Yes", true), ("No", true)])
            });
            if hit == Some(0) {
                let p = pk.path.clone();
                v.lib.start(ctx, format!("Moving '{}' to the Recycle Bin", pk.name), move |_| overlays::recycle(&p).map(|_| Done::Recycled(p)));
                if v.node == Node::Pack(pk.path) {
                    v.node = Node::All;
                }
            }
            hit.is_some() || close
        }
        Dlg::Rename { .. } | Dlg::Tags { .. } => text_dialog(ctx, v),
    };
    if done {
        v.dlg = None;
    }
}

/// (`Dlg::Rename` or `Dlg::Tags`) A one-line text dialog; true when closed.
fn text_dialog(ctx: &Context, v: &mut OverlaysView) -> bool {
    let (rename, mut text, path) = match &v.dlg {
        Some(Dlg::Rename { pack, text }) => (true, text.clone(), pack.clone()),
        Some(Dlg::Tags { item, text }) => (false, text.clone(), item.clone()),
        _ => return true,
    };
    let (title, label) = if rename { ("Rename Pack", "New name for the pack:") } else { ("Overlay Tags", "Tags (separated by commas):") };
    let (hit, close) = dialogs::modal(ctx, "overlay_text", title, vec2(380.0, 140.0), |ui| {
        ui.label(label);
        let r = ui.add(TextEdit::singleline(&mut text).desired_width(350.0));
        if !r.has_focus() && ui.memory(|m| m.focused().is_none()) {
            r.request_focus();
        }
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        ui.add_space(8.0);
        let ok = !rename || !text.trim().is_empty();
        dialogs::button_row(ui, &[("OK", ok), ("Cancel", true)]).or((enter && ok).then_some(0))
    });
    match &mut v.dlg {
        Some(Dlg::Rename { text: t, .. } | Dlg::Tags { text: t, .. }) => *t = text.clone(),
        _ => {}
    }
    if hit == Some(0) {
        let ix = &mut v.lib.index;
        if rename {
            if let Some(p) = ix.packs.iter_mut().find(|p| p.path == path) {
                p.name = text.trim().to_string();
            }
        } else {
            let tags: Vec<String> = text.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
            if tags.is_empty() {
                ix.tags.remove(&path);
            } else {
                ix.tags.insert(path, tags);
            }
        }
        v.lib.save();
    }
    hit.is_some() || close
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A last layout saved before the Overlays window existed gets it once, right after Media Generators (the active
    /// tab kept); one without Media Generators, or with Overlays, is left alone; a built-in layout already has it.
    #[test]
    fn layout_migration_places_overlays_beside_media_generators() {
        let old = r#"{"name":"","groups":[{"tabs":["ProjectMedia","Explorer","Transitions","VideoFx","MediaGenerators","Trimmer"],"active":5,"width":0},{"tabs":["VideoPreview"],"active":0,"width":800}],"height":300,"track_list_w":200}"#;
        let mut l: Layout = serde_json::from_str(old).unwrap();
        assert!(place_in_layout(&mut l));
        assert_eq!(l.groups[0].tabs[5], Panel::Overlays);
        assert_eq!(l.groups[0].tabs[l.groups[0].active], Panel::Trimmer, "the active tab stays active");
        assert!(!place_in_layout(&mut l), "once");
        let mut none: Layout = serde_json::from_str(r#"{"name":"","groups":[{"tabs":["Trimmer"],"active":0,"width":0}],"height":null,"track_list_w":200}"#).unwrap();
        assert!(!place_in_layout(&mut none) && none.groups[0].tabs == [Panel::Trimmer]);
        let t = theme::Theme::new(theme::LookChoice::VEGAS23);
        let mut d = super::super::dock::Dock::new(&t);
        let mut b = d.layout("");
        assert!(!place_in_layout(&mut b));
        // The migrated layout applies (old layouts keep loading).
        d.apply(&l, &t);
        assert!(d.is_open(Panel::Overlays));
    }

    #[test]
    fn scrub_frames_are_few_and_inside() {
        let m = MediaItem { duration: 4.0, media_type: MediaType::Video, ..Default::default() };
        let ts: Vec<f64> = (0..=20).map(|k| scrub_time(&m, k as f32 / 20.0)).collect();
        let mut distinct = ts.clone();
        distinct.dedup();
        assert_eq!(distinct.len(), SCRUB as usize);
        assert!(ts.iter().all(|&t| (0.0..4.0).contains(&t)));
    }
}
