//! Transitions / Video FX / Media Generators plug-in choosers (§5.6), built from the plug-in registry: search, category
//! chips and rows, preset tiles (built-in and user presets) with thumbnails rendered by
//! the engine, and the description strip. A tile drags onto an event, a track header or the preview; a double-click
//! asks the app to apply it to the selection.
use eframe::egui::*;
use std::collections::HashMap;
use crate::engine::plugins::{self, PluginInfo, PluginKind, UserPreset};
use crate::engine::mvfx::packages::{self, Package};
use crate::engine::{ofx, thumbs};
use super::icons::{Icon, paint_icon};
use super::theme;
use super::{PluginDrag, widgets};

const CHIPS: [&str; 11] = ["All Plug-ins", "Favorites", "AI/ML", "Creative", "Color", "Utility", "Blur", "Light", "360°", "Third Party", "Filter Packages"];
/// The "Filter Packages" chip: the Video FX window lists FX Packages there.
const PACKAGES: usize = 10;
/// New thumbnails rendered per frame; the rest come on the next frames.
const THUMBS_PER_FRAME: usize = 4;
/// Transitions animate at this rate while hovered.
const HOVER_FPS: f64 = 15.0;

/// One tile's preset: a built-in one (index into `info.presets`) or a user preset.
#[derive(Clone)]
enum Tile<'a> {
    Builtin(usize),
    User(&'a UserPreset),
}

impl Tile<'_> {
    /// (user preset, name): a user preset may share a built-in one's name.
    fn key(&self, info: &PluginInfo) -> (bool, String) {
        match self {
            Tile::Builtin(k) => (false, info.presets[*k].name.to_string()),
            Tile::User(u) => (true, u.name.clone()),
        }
    }

    fn instance(&self, info: &PluginInfo) -> crate::project::Fx {
        match self {
            Tile::Builtin(k) => plugins::new_fx(info, info.presets.get(*k)),
            Tile::User(u) => plugins::new_user_fx(info, u),
        }
    }
}

pub struct Chooser {
    kind: PluginKind,
    /// The built-ins, then the OFX plug-ins found (R33a) as of registry change `generation`.
    plugins: Vec<&'static PluginInfo>,
    generation: u64,
    query: String,
    /// None = the "All" node.
    plugin: Option<usize>,
    /// Selected tile: (user preset, name).
    preset: Option<(bool, String)>,
    chip: usize,
    tree_w: f32,
    /// Rendered thumbnails by plug-in id and tile key.
    thumbs: HashMap<ThumbKey, TextureHandle>,
    /// The hovered transition's animation: its key, texture and frame number.
    hover: Option<(ThumbKey, TextureHandle, i64)>,
    /// The user presets the thumbnails were drawn from; a save or delete redraws theirs.
    users: Vec<UserPreset>,
    /// FX Packages (built in, then the user's) as of `packages::generation` `pkg_gen`, their thumbnails and the one
    /// selected.
    packages: Vec<Package>,
    pkg_gen: Option<u64>,
    pkg_thumbs: HashMap<String, TextureHandle>,
    pkg_sel: Option<String>,
}

type ThumbKey = (&'static str, (bool, String));

impl Chooser {
    pub fn new(kind: PluginKind) -> Self {
        let generation = ofx::generation();
        let plugins: Vec<_> = plugins::catalog(kind).collect();
        Self { kind, plugins, generation, query: String::new(), plugin: Some(0), preset: None, chip: 0, tree_w: 166.0, thumbs: HashMap::new(), hover: None, users: Vec::new(), packages: Vec::new(), pkg_gen: None, pkg_thumbs: HashMap::new(), pkg_sel: None }
    }

    /// The Video FX window on its FX Packages (the "Filter Packages" chip).
    fn packages_shown(&self) -> bool {
        self.kind == PluginKind::VideoFx && self.chip == PACKAGES
    }

    /// The screenshot harness: plug-in `id` selected on its category's chip with its presets showing, or (None) the
    /// FX Packages.
    pub fn show_for_harness(&mut self, id: Option<&str>) {
        match id {
            Some(id) => {
                // On its category's chip, so the list shows its neighbours too.
                let at = self.plugins.iter().position(|p| p.id == id);
                let chip = at.and_then(|i| CHIPS.iter().position(|c| *c == self.plugins[i].category)).unwrap_or(0);
                (self.chip, self.plugin, self.preset) = (chip, at, None);
            }
            None => self.chip = PACKAGES,
        }
    }

    /// R33a: after a plug-in scan, or a plug-in turned on or off, list the plug-ins again (keeping the one selected).
    fn refresh(&mut self) {
        if self.generation == ofx::generation() {
            return;
        }
        self.generation = ofx::generation();
        let was = self.plugin.and_then(|i| self.plugins.get(i)).map(|p| p.id);
        self.plugins = plugins::catalog(self.kind).collect();
        if let Some(id) = was {
            let at = self.plugins.iter().position(|p| p.id == id);
            if at.is_none() {
                self.preset = None;
            }
            self.plugin = at.or(Some(0));
        }
    }

    fn visible(&self, i: usize, favorites: &[String]) -> bool {
        let p = self.plugins[i];
        let q = self.query.to_lowercase();
        let chip_ok = match self.chip {
            0 => true,
            1 => favorites.iter().any(|f| f == p.id),
            k => CHIPS[k] == p.category,
        };
        chip_ok && (q.is_empty() || p.name.to_lowercase().contains(&q))
    }
}

/// Toolbar row: search field and category chips.
pub fn draw_chooser_toolbar(ui: &mut Ui, c: &mut Chooser) {
    let t = theme::get();
    let (icon, _) = ui.allocate_exact_size(vec2(22.0, t.m.button), Sense::hover());
    paint_icon(ui.painter(), Rect::from_center_size(icon.center(), vec2(16.0, 16.0)), Icon::Search, None);
    widgets::search_field(ui, &mut c.query, "Search plug-ins", 135.0);
    ui.add_space(8.0);
    for (k, chip) in CHIPS.iter().enumerate() {
        let font = t.font(t.font.ui);
        let w = ui.painter().layout_no_wrap(chip.to_string(), font.clone(), t.text).size().x + 24.0;
        let (r, resp) = ui.allocate_exact_size(vec2(w, 24.0), Sense::click());
        let active = c.chip == k;
        // Toggle pills (UI_SPEC.md §5), the active category lit.
        let _ = font;
        widgets::pill_paint(ui.painter(), Rect::from_center_size(r.center(), vec2(r.width() - 2.0, 18.0)), chip, active, resp.hovered(), true);
        if resp.clicked() {
            c.chip = k;
        }
    }
}

/// What the chooser asks of the app this frame.
#[derive(Default)]
pub struct ChooserOut {
    /// A tile was double-clicked: apply it to the selection.
    pub apply: Option<PluginDrag>,
    /// The favourites changed (save the preferences).
    pub favorites: bool,
}

/// Body: plug-in tree/list | 5 px splitter | preset tiles; description strip at the bottom.
pub fn draw_chooser(ui: &mut Ui, c: &mut Chooser, users: &[UserPreset], favorites: &mut Vec<String>) -> ChooserOut {
    let t = theme::get();
    let area = ui.max_rect();
    let desc_h = 26.0;
    let body = Rect::from_min_max(area.min, pos2(area.max.x, area.max.y - desc_h));
    let mut tree_w = c.tree_w;
    let (left, right) = widgets::pane_split(ui, body, &mut tree_w, "chooser_tree");
    c.tree_w = tree_w;
    let mut out = ChooserOut::default();
    c.refresh();
    if c.packages_shown() && c.pkg_gen != Some(packages::generation()) {
        (c.packages, c.pkg_gen) = (packages::all(), Some(packages::generation()));
        c.pkg_thumbs.clear();
    }
    if c.users != users {
        c.thumbs.retain(|k, _| !k.1.0);
        c.hover = None;
        c.users = users.to_vec();
    }

    widgets::scroll_pane(ui, left, "chooser_list", |ui| {
        if c.packages_shown() {
            let q = c.query.to_lowercase();
            for pk in c.packages.iter().filter(|p| q.is_empty() || p.name.to_lowercase().contains(&q)) {
                let sel = c.pkg_sel.as_deref() == Some(pk.name.as_str());
                let r = widgets::list_row(ui, 26.0, sel);
                let p = ui.painter();
                paint_icon(p, Rect::from_min_size(pos2(r.rect.min.x + 8.0, r.rect.center().y - 8.0), vec2(16.0, 16.0)), Icon::FxBadge, None);
                p.text(pos2(r.rect.min.x + 32.0, r.rect.center().y), Align2::LEFT_CENTER, &pk.name, t.font(t.font.ui), widgets::list_ink(ui, sel));
                if r.clicked() {
                    c.pkg_sel = Some(pk.name.clone());
                }
            }
            return;
        }
        let shown: Vec<usize> = (0..c.plugins.len()).filter(|&i| c.visible(i, favorites)).collect();
        for i in shown {
            let (id, name) = (c.plugins[i].id, c.plugins[i].name);
            let sel = c.plugin == Some(i);
            let r = {
                let r = widgets::list_row(ui, 26.0, sel);
                let p = ui.painter();
                paint_icon(p, Rect::from_min_size(pos2(r.rect.min.x + 8.0, r.rect.center().y - 8.0), vec2(16.0, 16.0)), Icon::FxBadge, None);
                p.text(pos2(r.rect.min.x + 32.0, r.rect.center().y), Align2::LEFT_CENTER, name, t.font(t.font.ui), widgets::list_ink(ui, sel));
                let fav = favorites.iter().any(|f| f == id);
                if fav || r.hovered() {
                    let star = Rect::from_center_size(pos2(r.rect.max.x - 14.0, r.rect.center().y), vec2(16.0, 16.0));
                    paint_icon(p, star, Icon::Favorite, Some(if fav { t.amber.base } else { t.lcd.text_dim }));
                    if ui.interact(star, r.id.with("fav"), Sense::click()).clicked() {
                        if fav {
                            favorites.retain(|f| f != id);
                        } else {
                            favorites.push(id.to_string());
                        }
                        out.favorites = true;
                    }
                }
                r
            };
            if r.clicked() {
                (c.plugin, c.preset) = (Some(i), None);
            }
        }
    });

    widgets::scroll_pane(ui, right, "chooser_presets", |ui| if c.packages_shown() { package_grid(ui, c, &mut out) } else { preset_grid(ui, c, users, favorites, &mut out) });

    // Description strip.
    let strip = Rect::from_min_max(pos2(area.min.x, body.max.y), area.max);
    let p = ui.painter();
    widgets::sunken(p, strip.shrink2(vec2(0.0, 1.0)), t.input_bg, &t.metal);
    let package = c.packages_shown().then(|| c.pkg_sel.as_ref().and_then(|n| c.packages.iter().find(|p| &p.name == n))).flatten();
    if let Some(pk) = package {
        let what = if pk.builtin { "FX Package" } else { "FX Package (yours)" };
        let text = format!("{}: {}{}{}, {what}", pk.name, pk.desc, if pk.desc.is_empty() { "" } else { " - " }, packages::summary(pk));
        p.with_clip_rect(strip.intersect(ui.clip_rect())).text(pos2(strip.min.x + 6.0, strip.center().y), Align2::LEFT_CENTER, text, t.font(t.font.ui), t.text);
    } else if let Some(info) = c.plugin.and_then(|i| c.plugins.get(i)).filter(|_| !c.packages_shown()) {
        let text = if info.id.starts_with(crate::engine::aex::PREFIX) {
            format!("{}, Grouping: {}, {}", info.name, info.group, crate::engine::aex::KIND)
        } else if info.id.starts_with(ofx::PREFIX) {
            let v = info.id.rsplit_once('@').map_or("", |v| v.1);
            format!("{}{}, Grouping: {}, OFX plug-in version {v}", info.name, if info.desc.is_empty() { String::new() } else { format!(": {}", info.desc) }, info.group)
        } else {
            format!("{}: {}, Grouping: Milan\\{}, Version 1.0", info.name, info.desc, info.group)
        };
        p.with_clip_rect(strip.intersect(ui.clip_rect())).text(pos2(strip.min.x + 6.0, strip.center().y), Align2::LEFT_CENTER, text, t.font(t.font.ui), t.text);
    }
    out
}

/// Preset tiles: ≈120×75 thumbnails with 12 px gaps.
fn preset_grid(ui: &mut Ui, c: &mut Chooser, users: &[UserPreset], favorites: &[String], out: &mut ChooserOut) {
    let (thumb, gap) = (vec2(120.0, 75.0), 12.0);
    let tile = thumb + vec2(16.0, 36.0);
    // The "All" node shows each plug-in's first preset.
    let tiles: Vec<(&'static PluginInfo, Tile)> = match c.plugin.and_then(|i| c.plugins.get(i)) {
        Some(&info) => (0..info.presets.len()).map(|k| (info, Tile::Builtin(k))).chain(users.iter().filter(|u| u.plugin == info.id).map(|u| (info, Tile::User(u)))).collect(),
        None => (0..c.plugins.len()).filter(|&i| c.visible(i, favorites)).map(|i| (c.plugins[i], Tile::Builtin(0))).collect(),
    };
    let width = ui.available_width().max(tile.x + gap);
    let cols = (((width - gap) / (tile.x + gap)).floor() as usize).max(1);
    let rows = tiles.len().div_ceil(cols);
    let (area, _) = ui.allocate_exact_size(vec2(width, gap + rows as f32 * (tile.y + gap)), Sense::hover());
    let now = ui.input(|i| i.time);
    let mut budget = THUMBS_PER_FRAME;
    for (n, (info, preset)) in tiles.iter().enumerate() {
        let min = area.min + vec2(gap + (n % cols) as f32 * (tile.x + gap), gap + (n / cols) as f32 * (tile.y + gap));
        let r = Rect::from_min_size(min, tile);
        let tk = preset.key(info);
        let name = tk.1.clone();
        let resp = ui.interact(r, ui.id().with(("preset", info.id, &tk)), Sense::click_and_drag());
        let sel = c.plugin.is_some() && c.preset.as_ref() == Some(&tk);
        let th = Rect::from_min_size(min + vec2(8.0, 6.0), thumb);
        let label = if c.plugin.is_some() { name.clone() } else { info.name.to_string() };
        widgets::tile(ui, r, th, &label, sel, resp.hovered());
        widgets::thumb_well(ui.painter(), th);
        // Only tiles on screen are rendered.
        if ui.clip_rect().intersects(r) {
            let key = (info.id, tk.clone());
            let animate = resp.hovered() && info.kind == PluginKind::Transition;
            let tex = if animate {
                Some(hover_frame(ui, c, key, info, preset, now))
            } else if let Some(h) = c.thumbs.get(&key) {
                Some(h.id())
            } else if budget > 0 {
                budget -= 1;
                // A third of the way: a dissolve from the sample to its negative is flat grey at one half.
                let img = thumb_image(info, preset, 0.35);
                let h = ui.ctx().load_texture(format!("chooser_{}_{}_{name}", info.id, tk.0), img, TextureOptions::LINEAR);
                let id = h.id();
                c.thumbs.insert(key, h);
                Some(id)
            } else {
                ui.ctx().request_repaint();
                None
            };
            if let Some(tex) = tex {
                ui.painter().with_clip_rect(th.intersect(ui.clip_rect())).image(tex, th, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
            }
        }
        let drag = || PluginDrag { kind: info.kind, name: info.name.to_string(), fx: preset.instance(info), package: Vec::new() };
        if resp.drag_started() {
            DragAndDrop::set_payload(ui.ctx(), drag());
        }
        if resp.clicked() || resp.double_clicked() {
            c.plugin = c.plugins.iter().position(|p| p.id == info.id);
            c.preset = Some(tk.clone());
        }
        if resp.double_clicked() {
            out.apply = Some(drag());
        }
        resp.on_hover_text(format!("{} - {name}", info.name));
    }
}

/// FX Package tiles: the chain on the sample picture; a tile drags onto an event like an FX, a double-click applies
/// it to the selection, and a user package's context menu deletes it.
fn package_grid(ui: &mut Ui, c: &mut Chooser, out: &mut ChooserOut) {
    let (thumb, gap) = (vec2(120.0, 75.0), 12.0);
    let tile = thumb + vec2(16.0, 36.0);
    let q = c.query.to_lowercase();
    let shown: Vec<Package> = c.packages.iter().filter(|p| q.is_empty() || p.name.to_lowercase().contains(&q)).cloned().collect();
    let width = ui.available_width().max(tile.x + gap);
    let cols = (((width - gap) / (tile.x + gap)).floor() as usize).max(1);
    let rows = shown.len().div_ceil(cols);
    let (area, _) = ui.allocate_exact_size(vec2(width, gap + rows as f32 * (tile.y + gap)), Sense::hover());
    let mut budget = THUMBS_PER_FRAME;
    for (n, pk) in shown.iter().enumerate() {
        let min = area.min + vec2(gap + (n % cols) as f32 * (tile.x + gap), gap + (n / cols) as f32 * (tile.y + gap));
        let r = Rect::from_min_size(min, tile);
        let resp = ui.interact(r, ui.id().with(("package", &pk.name)), Sense::click_and_drag());
        let sel = c.pkg_sel.as_deref() == Some(pk.name.as_str());
        let th = Rect::from_min_size(min + vec2(8.0, 6.0), thumb);
        widgets::tile(ui, r, th, &pk.name, sel, resp.hovered());
        widgets::thumb_well(ui.painter(), th);
        if ui.clip_rect().intersects(r) {
            let tex = match c.pkg_thumbs.get(&pk.name) {
                Some(h) => Some(h.id()),
                None if budget > 0 => {
                    budget -= 1;
                    let h = ui.ctx().load_texture(format!("package_{}", pk.name), packages::thumb(pk), TextureOptions::LINEAR);
                    let id = h.id();
                    c.pkg_thumbs.insert(pk.name.clone(), h);
                    Some(id)
                }
                None => {
                    ui.ctx().request_repaint();
                    None
                }
            };
            if let Some(tex) = tex {
                ui.painter().with_clip_rect(th.intersect(ui.clip_rect())).image(tex, th, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
            }
        }
        let drag = || {
            let mut chain = packages::instantiate(pk);
            let first = chain.remove(0);
            PluginDrag { kind: PluginKind::VideoFx, name: pk.name.clone(), fx: first, package: chain }
        };
        if resp.drag_started() && !pk.fx.is_empty() {
            DragAndDrop::set_payload(ui.ctx(), drag());
        }
        if resp.clicked() || resp.double_clicked() {
            c.pkg_sel = Some(pk.name.clone());
        }
        if resp.double_clicked() && !pk.fx.is_empty() {
            out.apply = Some(drag());
        }
        let resp = resp.on_hover_text(format!("{} - {}", pk.name, packages::summary(pk)));
        if !pk.builtin {
            resp.context_menu(|ui| {
                if ui.button("Delete FX Package").clicked() {
                    if let Err(e) = packages::delete_user(&pk.name) {
                        log::warn!("could not delete the FX Package {}: {e}", pk.name);
                    }
                    ui.close_menu();
                }
            });
        }
    }
}

fn thumb_image(info: &'static PluginInfo, preset: &Tile, p: f32) -> ColorImage {
    // An OFX plug-in's tile is the sample as it is: tiles are drawn on the UI thread, plug-ins run in their host.
    if ofx::is_hosted(info.id) {
        return thumbs::sample();
    }
    match preset {
        Tile::Builtin(k) => thumbs::preset_thumb(info, info.presets.get(*k), p),
        Tile::User(u) => plugins::eval(&plugins::new_user_fx(info, u), 0.0).map_or_else(|| ColorImage::new([thumbs::W, thumbs::H], Color32::BLACK), |e| thumbs::eval_thumb(info.kind, &e, p)),
    }
}

/// The hovered transition at its current animation frame (A to B over 1.6 s), re-rendered at 15 fps.
fn hover_frame(ui: &Ui, c: &mut Chooser, key: ThumbKey, info: &'static PluginInfo, preset: &Tile, now: f64) -> TextureId {
    const FRAMES: i64 = 24;
    let n = (now * HOVER_FPS) as i64 % FRAMES;
    let p = n as f32 / (FRAMES - 1) as f32;
    ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(1.0 / HOVER_FPS));
    match &mut c.hover {
        Some((k, h, at)) if *k == key => {
            if *at != n {
                h.set(thumb_image(info, preset, p), TextureOptions::LINEAR);
                *at = n;
            }
            h.id()
        }
        slot => {
            let h = ui.ctx().load_texture("chooser_hover", thumb_image(info, preset, p), TextureOptions::LINEAR);
            let id = h.id();
            *slot = Some((key, h, n));
            id
        }
    }
}
