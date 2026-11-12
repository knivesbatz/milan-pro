//! Transitions / Video FX / Media Generators plug-in choosers (§5.6). UI only for now: search, plug-in tree
//! (VP16) or category chips and rows (VP23), preset tiles with procedurally painted thumbnails, description strip.
use eframe::egui::*;
use std::collections::HashSet;
use super::icons::{Icon, paint_icon};
use super::theme::{self, UiLook};
use super::widgets;

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Transitions,
    VideoFx,
    Generators,
}

impl Kind {
    /// (plug-in name, VP23 category, short description). Generic effect names, no vendor branding.
    fn plugins(self) -> &'static [(&'static str, &'static str, &'static str)] {
        match self {
            Kind::Transitions => &[
                ("Barn Door", "Creative", "Opens or closes like a pair of doors"),
                ("Clock Wipe", "Creative", "Sweeps around the centre like a clock hand"),
                ("Cross Dissolve", "Utility", "Blends the outgoing and incoming clips"),
                ("Flash", "Light", "Fades through a bright flash"),
                ("Iris", "Creative", "Opens or closes a shaped iris"),
                ("Linear Wipe", "Utility", "Wipes across at an angle"),
                ("Page Peel", "Creative", "Peels the outgoing clip away like a page"),
                ("Push", "Utility", "Pushes the outgoing clip out of frame"),
                ("Slide", "Utility", "Slides the incoming clip in over the outgoing one"),
                ("Spiral", "Creative", "Reveals in a spiral"),
                ("Squeeze", "Creative", "Squeezes the outgoing clip away"),
                ("Zoom", "Blur", "Zooms through the cut with motion blur"),
            ],
            Kind::VideoFx => &[
                ("Add Noise", "Creative", "Adds film-style grain"),
                ("Black and White", "Color", "Removes colour"),
                ("Border", "Utility", "Draws a border around the frame"),
                ("Brightness and Contrast", "Color", "Adjusts brightness and contrast"),
                ("Chroma Keyer", "Utility", "Keys out a colour"),
                ("Color Balance", "Color", "Shifts shadows, midtones and highlights"),
                ("Color Curves", "Color", "Adjusts tone with curves"),
                ("Gaussian Blur", "Blur", "Softens the image"),
                ("Glow", "Light", "Adds a soft glow to bright areas"),
                ("Levels", "Color", "Sets input and output levels"),
                ("Mirror", "Creative", "Mirrors the frame"),
                ("Picture in Picture", "Utility", "Scales and places the frame"),
                ("Sharpen", "Utility", "Sharpens detail"),
                ("Vignette", "Light", "Darkens the corners"),
            ],
            Kind::Generators => &[
                ("Checkerboard", "Utility", "Generates a checkerboard"),
                ("Color Gradient", "Color", "Generates a linear or radial gradient"),
                ("Credit Roll", "Creative", "Generates scrolling credits"),
                ("Noise Texture", "Creative", "Generates a noise texture"),
                ("Solid Color", "Color", "Generates a solid colour"),
                ("Test Pattern", "Utility", "Generates colour bars and test patterns"),
                ("Titles & Text", "Creative", "Generates titles"),
            ],
        }
    }

    fn presets(self) -> &'static [&'static str] {
        match self {
            Kind::Transitions => &["Default", "Left to Right", "Right to Left", "Top Down", "Bottom Up", "Soft Edge"],
            Kind::VideoFx => &["Default", "Light", "Medium", "Strong", "Reset to None"],
            Kind::Generators => &["Default", "Black", "White", "Blue", "Warm", "Cool"],
        }
    }

    fn group(self) -> &'static str {
        match self {
            Kind::Transitions => "Transitions",
            Kind::VideoFx => "Filters",
            Kind::Generators => "Generators",
        }
    }
}

const CHIPS: [&str; 11] = ["All Plug-ins", "Favorites", "AI/ML", "Creative", "Color", "Utility", "Blur", "Light", "360°", "Third Party", "Filter Packages"];

pub struct Chooser {
    kind: Kind,
    query: String,
    /// None = the "All" node.
    plugin: Option<usize>,
    preset: Option<usize>,
    chip: usize,
    favorites: HashSet<usize>,
    tree_w: f32,
}

impl Chooser {
    pub fn new(kind: Kind) -> Self {
        Self { kind, query: String::new(), plugin: Some(0), preset: None, chip: 0, favorites: HashSet::new(), tree_w: 166.0 }
    }

    fn visible(&self, i: usize) -> bool {
        let (name, cat, _) = self.kind.plugins()[i];
        let q = self.query.to_lowercase();
        let chip_ok = match self.chip {
            0 => true,
            1 => self.favorites.contains(&i),
            k => CHIPS[k] == cat,
        };
        chip_ok && (q.is_empty() || name.to_lowercase().contains(&q))
    }
}

/// Toolbar row: VP16 search field + bold "Preset:"; VP23 search + category chips.
pub fn draw_chooser_toolbar(ui: &mut Ui, c: &mut Chooser) {
    let t = theme::get();
    let (icon, _) = ui.allocate_exact_size(vec2(22.0, t.m.button), Sense::hover());
    paint_icon(ui.painter(), Rect::from_center_size(icon.center(), vec2(16.0, 16.0)), Icon::Search, t.look(), None);
    widgets::search_field(ui, &mut c.query, "Search plug-ins", 135.0);
    if t.look() == UiLook::Vegas16 {
        ui.add_space(14.0);
        ui.label(RichText::new("Preset:").font(t.bold(t.font.ui)).color(t.text));
        if let Some(name) = c.plugin.zip(c.preset).map(|(_, k)| c.kind.presets()[k]) {
            ui.add_space(6.0);
            ui.label(RichText::new(name).font(t.font(t.font.ui)).color(t.text));
        }
        return;
    }
    ui.add_space(8.0);
    for (k, chip) in CHIPS.iter().enumerate() {
        let font = t.font(t.font.ui);
        let w = ui.painter().layout_no_wrap(chip.to_string(), font.clone(), t.text).size().x + 24.0;
        let (r, resp) = ui.allocate_exact_size(vec2(w, 24.0), Sense::click());
        let active = c.chip == k;
        let p = ui.painter();
        if active {
            p.rect_filled(r, CornerRadius::same(2), t.accent);
        } else if resp.hovered() {
            p.rect_filled(r, CornerRadius::same(2), t.button_hover_bg);
        }
        p.text(r.center(), Align2::CENTER_CENTER, *chip, font, if active { Color32::WHITE } else { t.text });
        p.vline(r.max.x + 0.5, r.y_range(), Stroke::new(1.0_f32, t.title_bg));
        if resp.clicked() {
            c.chip = k;
        }
    }
}

/// Body: plug-in tree/list | 5 px splitter | preset tiles; description strip at the bottom.
pub fn draw_chooser(ui: &mut Ui, c: &mut Chooser) {
    let t = theme::get();
    let v16 = t.look() == UiLook::Vegas16;
    let area = ui.max_rect();
    let desc_h = 26.0;
    let body = Rect::from_min_max(area.min, pos2(area.max.x, area.max.y - desc_h));
    let mut tree_w = c.tree_w;
    let (left, right) = widgets::pane_split(ui, body, &mut tree_w, "chooser_tree");
    c.tree_w = tree_w;
    let plugins = c.kind.plugins();

    widgets::scroll_pane(ui, left, "chooser_list", |ui| {
        if v16 {
            let (r, _) = widgets::tree_row(ui, 0, Some(true), Some(Icon::Folder), "All", c.plugin.is_none());
            if r.clicked() {
                (c.plugin, c.preset) = (None, None);
            }
        }
        let shown: Vec<usize> = (0..plugins.len()).filter(|&i| c.visible(i)).collect();
        for i in shown {
            let sel = c.plugin == Some(i);
            let r = if v16 {
                widgets::tree_row(ui, 1, None, Some(Icon::FxBadge), plugins[i].0, sel).0
            } else {
                let r = widgets::list_row(ui, 26.0, sel);
                let p = ui.painter();
                paint_icon(p, Rect::from_min_size(pos2(r.rect.min.x + 8.0, r.rect.center().y - 8.0), vec2(16.0, 16.0)), Icon::FxBadge, t.look(), sel.then_some(Color32::WHITE));
                p.text(pos2(r.rect.min.x + 32.0, r.rect.center().y), Align2::LEFT_CENTER, plugins[i].0, t.font(t.font.ui), if sel { Color32::WHITE } else { t.text });
                let fav = c.favorites.contains(&i);
                if fav || r.hovered() {
                    let star = Rect::from_center_size(pos2(r.rect.max.x - 14.0, r.rect.center().y), vec2(16.0, 16.0));
                    paint_icon(p, star, Icon::Favorite, t.look(), Some(if fav { t.accent } else { t.text_dim }));
                    if ui.interact(star, r.id.with("fav"), Sense::click()).clicked() && !c.favorites.remove(&i) {
                        c.favorites.insert(i);
                    }
                }
                r
            };
            if r.clicked() {
                (c.plugin, c.preset) = (Some(i), None);
            }
        }
    });

    widgets::scroll_pane(ui, right, "chooser_presets", |ui| preset_grid(ui, c));

    // Description strip.
    let strip = Rect::from_min_max(pos2(area.min.x, body.max.y), area.max);
    let p = ui.painter();
    p.rect_filled(strip, 0.0, t.toolbar_bg);
    if let Some(i) = c.plugin {
        let (name, cat, desc) = plugins[i];
        let text = format!("{name}: {desc}, OFX, 32-bit floating point, Grouping: Milan\\{}\\{cat}, Version 1.0", c.kind.group());
        p.with_clip_rect(strip.intersect(ui.clip_rect())).text(pos2(strip.min.x + 6.0, strip.center().y), Align2::LEFT_CENTER, text, t.font(t.font.ui), t.text);
    }
}

/// Preset tiles: VP16 116×96 (100×60 thumbnail + 2-line label), VP23 ≈120×75 thumbnails with 12 px gaps.
fn preset_grid(ui: &mut Ui, c: &mut Chooser) {
    let t = theme::get();
    let v16 = t.look() == UiLook::Vegas16;
    let (thumb, gap) = if v16 { (vec2(100.0, 60.0), 16.0) } else { (vec2(120.0, 75.0), 12.0) };
    let tile = thumb + vec2(16.0, 36.0);
    let plugins = c.kind.plugins();
    // The "All" node shows each plug-in's default preset.
    let tiles: Vec<(usize, usize)> = match c.plugin {
        Some(i) => (0..c.kind.presets().len()).map(|k| (i, k)).collect(),
        None => (0..plugins.len()).filter(|&i| c.visible(i)).map(|i| (i, 0)).collect(),
    };
    let width = ui.available_width().max(tile.x + gap);
    let cols = (((width - gap) / (tile.x + gap)).floor() as usize).max(1);
    let rows = tiles.len().div_ceil(cols);
    let (area, _) = ui.allocate_exact_size(vec2(width, gap + rows as f32 * (tile.y + gap)), Sense::hover());
    for (n, &(i, k)) in tiles.iter().enumerate() {
        let min = area.min + vec2(gap + (n % cols) as f32 * (tile.x + gap), gap + (n / cols) as f32 * (tile.y + gap));
        let r = Rect::from_min_size(min, tile);
        let resp = ui.interact(r, ui.id().with(("preset", i, k)), Sense::click());
        let sel = c.plugin == Some(i) && c.preset == Some(k);
        let th = Rect::from_min_size(min + vec2(8.0, 6.0), thumb);
        let label = if c.plugin.is_some() { c.kind.presets()[k] } else { plugins[i].0 };
        widgets::tile(ui, r, th, label, sel, resp.hovered());
        paint_preset(ui.painter(), th, c.kind, i, k, resp.hovered().then(|| ui.input(|inp| inp.time)));
        if resp.hovered() && c.kind == Kind::Transitions {
            ui.ctx().request_repaint();
        }
        if resp.clicked() {
            (c.plugin, c.preset) = (Some(i), Some(k));
        }
        resp.on_hover_text(format!("{} - {}", plugins[i].0, c.kind.presets()[k]));
    }
}

/// Original procedural thumbnails: transitions show A→B (animated while hovered), FX a painted sample scene
/// with the effect suggested, generators the generated image.
fn paint_preset(p: &Painter, r: Rect, kind: Kind, plugin: usize, preset: usize, time: Option<f64>) {
    let hue = |k: usize| -> Color32 { ecolor::Hsva::new(((plugin * 37 + k * 53) % 100) as f32 / 100.0, 0.55, 0.85, 1.0).into() };
    let p = p.with_clip_rect(r.intersect(p.clip_rect()));
    match kind {
        Kind::Transitions => {
            // B wipes in over A in the preset's direction (half-way when idle, animated on hover).
            let f = time.map_or(0.5, |t| (t * 0.6).fract() as f32);
            let (w, h) = (r.width() * f, r.height() * f);
            let b = match preset {
                2 => Rect::from_min_max(pos2(r.max.x - w, r.min.y), r.max),
                3 => Rect::from_min_max(r.min, pos2(r.max.x, r.min.y + h)),
                4 => Rect::from_min_max(pos2(r.min.x, r.max.y - h), r.max),
                _ => Rect::from_min_max(r.min, pos2(r.min.x + w, r.max.y)),
            };
            let a = match preset {
                2 => Rect::from_min_max(r.min, pos2(b.min.x, r.max.y)),
                3 => Rect::from_min_max(pos2(r.min.x, b.max.y), r.max),
                4 => Rect::from_min_max(r.min, pos2(r.max.x, b.min.y)),
                _ => Rect::from_min_max(pos2(b.max.x, r.min.y), r.max),
            };
            let font = FontId::proportional(18.0);
            p.rect_filled(r, 0.0, Color32::from_rgb(40, 90, 170));
            p.with_clip_rect(a.intersect(p.clip_rect())).text(a.center(), Align2::CENTER_CENTER, "A", font.clone(), Color32::from_white_alpha(170));
            p.rect_filled(b, 0.0, Color32::from_rgb(210, 120, 40));
            p.with_clip_rect(b.intersect(p.clip_rect())).text(b.center(), Align2::CENTER_CENTER, "B", font, Color32::from_white_alpha(200));
        }
        Kind::VideoFx => {
            // Sky, sun and hills, then a per-preset strength wash in the plug-in's colour.
            let mut sky = Mesh::default();
            sky.colored_vertex(r.left_top(), Color32::from_rgb(70, 130, 200));
            sky.colored_vertex(r.right_top(), Color32::from_rgb(70, 130, 200));
            sky.colored_vertex(r.right_bottom(), Color32::from_rgb(230, 190, 140));
            sky.colored_vertex(r.left_bottom(), Color32::from_rgb(230, 190, 140));
            sky.add_triangle(0, 1, 2);
            sky.add_triangle(0, 2, 3);
            p.add(sky);
            p.circle_filled(r.min + vec2(r.width() * 0.7, r.height() * 0.35), r.height() * 0.16, Color32::from_rgb(255, 230, 120));
            let hill = |x: f32, w: f32, h: f32, c| {
                p.add(Shape::convex_polygon(vec![pos2(x - w, r.max.y), pos2(x, r.max.y - h), pos2(x + w, r.max.y)], c, Stroke::NONE))
            };
            hill(r.min.x + r.width() * 0.3, r.width() * 0.45, r.height() * 0.45, Color32::from_rgb(50, 110, 60));
            hill(r.min.x + r.width() * 0.85, r.width() * 0.35, r.height() * 0.3, Color32::from_rgb(40, 90, 50));
            let strength = [0.0, 0.25, 0.45, 0.7, 0.0][preset.min(4)];
            if strength > 0.0 || preset == 0 {
                p.rect_filled(r, 0.0, hue(0).gamma_multiply(if preset == 0 { 0.3 } else { strength }));
            }
        }
        Kind::Generators => match plugin {
            0 => {
                let n = 6;
                let s = r.width() / n as f32;
                for y in 0..((r.height() / s).ceil() as usize) {
                    for x in 0..n {
                        let c = if (x + y) % 2 == 0 { Color32::WHITE } else { Color32::BLACK };
                        p.rect_filled(Rect::from_min_size(r.min + vec2(x as f32 * s, y as f32 * s), vec2(s, s)), 0.0, c);
                    }
                }
            }
            6 | 2 => {
                p.rect_filled(r, 0.0, Color32::BLACK);
                let txt = if plugin == 2 { "Credits\nName" } else { "Title" };
                p.text(r.center(), Align2::CENTER_CENTER, txt, FontId::proportional(14.0), Color32::WHITE);
            }
            5 => {
                let bars = [(192, 192, 192), (192, 192, 0), (0, 192, 192), (0, 192, 0), (192, 0, 192), (192, 0, 0), (0, 0, 192)];
                let w = r.width() / bars.len() as f32;
                for (k, (rr, g, b)) in bars.into_iter().enumerate() {
                    p.rect_filled(Rect::from_min_size(r.min + vec2(k as f32 * w, 0.0), vec2(w + 0.5, r.height())), 0.0, Color32::from_rgb(rr, g, b));
                }
            }
            _ => {
                let mut m = Mesh::default();
                let (a, b): (Color32, Color32) = (hue(preset), hue(preset + 3));
                let flat = plugin == 4;
                m.colored_vertex(r.left_top(), a);
                m.colored_vertex(r.right_top(), if flat { a } else { b });
                m.colored_vertex(r.right_bottom(), if flat { a } else { b });
                m.colored_vertex(r.left_bottom(), a);
                m.add_triangle(0, 1, 2);
                m.add_triangle(0, 2, 3);
                p.add(m);
            }
        },
    }
}
