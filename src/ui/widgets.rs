//! The design system's painters (UI_SPEC.md §4-§5): chrome, recesses, LCD wells, segmented digits, selection bars,
//! pills and section tabs, the controls painted over egui's, and the shared dock-window pieces (tree rows, list rows
//! and headers, the pane splitter, search fields and preset tiles). Every surface draws through these.
use eframe::egui::*;
use super::icons::{Icon, paint_icon};
use super::theme::{self, Metal, Theme};

// ---- Primitives ----

/// One line of chrome (an outline, a lip, a shade) in points: R48's whole physical pixel (`theme::one()`), so every
/// painter's edges stay crisp at 125, 150 and 200 %.
pub fn hair() -> f32 {
    theme::one()
}

/// `a` → `b` by `f` in gamma space (as the period skins blended).
fn mix(a: Color32, b: Color32, f: f32) -> Color32 {
    let f = f.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * f).round() as u8;
    Color32::from_rgba_premultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}

/// The colour of vertical gradient `stops` ((position 0..1, colour), ascending) at `f`.
fn sample(stops: &[(f32, Color32)], f: f32) -> Color32 {
    let Some(&(p0, c0)) = stops.first() else { return Color32::TRANSPARENT };
    if f <= p0 {
        return c0;
    }
    for w in stops.windows(2) {
        let ((a, ca), (b, cb)) = (w[0], w[1]);
        if f <= b {
            return mix(ca, cb, if b > a { (f - a) / (b - a) } else { 1.0 });
        }
    }
    stops[stops.len() - 1].1
}

/// A vertical gradient filling `r` with corners of `radius`: horizontal strips whose ends follow the corner arcs, one
/// row per gradient stop and per arc step, so the gradient is exact (piecewise linear) at any radius. The outline drawn
/// over it anti-aliases the corners.
pub fn grad(p: &Painter, r: Rect, radius: f32, stops: &[(f32, Color32)]) {
    if !r.is_positive() || stops.is_empty() {
        return;
    }
    let rad = radius.min(r.width() * 0.5).min(r.height() * 0.5).max(0.0);
    let h = r.height();
    let steps = if rad >= 1.0 { (rad.ceil() as usize).clamp(2, 6) } else { 0 };
    let mut ys: Vec<f32> = Vec::with_capacity(2 * steps + stops.len() + 4);
    ys.extend([r.min.y, r.max.y]);
    for i in 0..=steps {
        let k = i as f32 / steps.max(1) as f32;
        ys.extend([r.min.y + rad * k, r.max.y - rad * k]);
    }
    ys.extend(stops.iter().map(|(f, _)| r.min.y + f.clamp(0.0, 1.0) * h));
    ys.sort_by(f32::total_cmp);
    ys.dedup_by(|a, b| (*a - *b).abs() < 0.01);
    let inset = |y: f32| {
        let dy = (r.min.y + rad - y).max(y - (r.max.y - rad)).max(0.0);
        if rad > 0.0 { rad - (rad * rad - dy * dy).max(0.0).sqrt() } else { 0.0 }
    };
    let mut mesh = Mesh::default();
    for (i, &y) in ys.iter().enumerate() {
        let c = sample(stops, (y - r.min.y) / h);
        let d = inset(y);
        mesh.colored_vertex(pos2(r.min.x + d, y), c);
        mesh.colored_vertex(pos2(r.max.x - d, y), c);
        if i > 0 {
            let k = (2 * i) as u32;
            mesh.add_triangle(k - 2, k - 1, k);
            mesh.add_triangle(k - 1, k + 1, k);
        }
    }
    p.add(mesh);
}

/// A 1 px horizontal line from `x0` to `x1` at `y` (its top edge).
fn hl(p: &Painter, x0: f32, x1: f32, y: f32, c: Color32) {
    if x1 > x0 {
        p.rect_filled(Rect::from_min_max(pos2(x0, y), pos2(x1, y + hair())), 0.0, c);
    }
}

/// A 1 px vertical line from `y0` to `y1` at `x` (its left edge).
fn vl(p: &Painter, x: f32, y0: f32, y1: f32, c: Color32) {
    if y1 > y0 {
        p.rect_filled(Rect::from_min_max(pos2(x, y0), pos2(x + hair(), y1)), 0.0, c);
    }
}

/// The radius a control of this size takes: `r_small` under 20 px, `r_ctl` above.
pub fn ctl_radius(r: Rect, t: &Theme) -> f32 {
    if r.width().min(r.height()) < 20.0 { t.m.r_small as f32 } else { t.m.r_ctl as f32 }
}

// ---- Metal bars ----

/// Side of the brushed streak texture; it repeats over every bar.
const STREAK: usize = 64;

/// The brushed streak texture, made once per context (a 64×64 tile of lighter and darker horizontal streaks,
/// `TextureWrapMode::Repeat`), drawn over the gradient with its UVs scaled to the bar: no per-frame allocation.
fn streak_tex(ctx: &Context) -> TextureId {
    let id = Id::new("milan_metal_streaks");
    if let Some(h) = ctx.data(|d| d.get_temp::<TextureHandle>(id)) {
        return h.id();
    }
    let mut seed: u32 = 0x9E37_79B9;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed >> 8) as f32 / (1u32 << 24) as f32
    };
    let mut pixels = vec![Color32::TRANSPARENT; STREAK * STREAK];
    // Each row is one fine streak: its own brightness (smoothed with the row above, so no two adjacent rows jump),
    // roughened only slightly along it; at most 18/255 lighter or darker at full opacity (`Metal::streak` scales it).
    let mut prev = 0.0_f32;
    for y in 0..STREAK {
        let row = (rnd() * 2.0 - 1.0) * 0.6 + prev * 0.4;
        prev = row;
        for x in 0..STREAK {
            let v = row * 0.85 + (rnd() * 2.0 - 1.0) * 0.15;
            let a = (v.abs() * 18.0) as u8;
            pixels[y * STREAK + x] = if v >= 0.0 { Color32::from_white_alpha(a) } else { Color32::from_black_alpha(a) };
        }
    }
    let image = ColorImage { size: [STREAK, STREAK], pixels };
    let opts = TextureOptions { magnification: TextureFilter::Nearest, minification: TextureFilter::Nearest, wrap_mode: TextureWrapMode::Repeat, mipmap_mode: None };
    let handle = ctx.load_texture("milan_metal_streaks", image, opts);
    let tid = handle.id();
    ctx.data_mut(|d| d.insert_temp(id, handle));
    tid
}

/// The LCD dot grid: a 2×2 tile with one lit dot (`lcd.rule`), repeated over a read-out well.
fn dots_tex(ctx: &Context) -> TextureId {
    let id = Id::new("milan_lcd_dots");
    if let Some(h) = ctx.data(|d| d.get_temp::<TextureHandle>(id)) {
        return h.id();
    }
    let c = theme::get().lcd.rule;
    let pixels = vec![c, Color32::TRANSPARENT, Color32::TRANSPARENT, Color32::TRANSPARENT];
    let opts = TextureOptions { magnification: TextureFilter::Nearest, minification: TextureFilter::Nearest, wrap_mode: TextureWrapMode::Repeat, mipmap_mode: None };
    let handle = ctx.load_texture("milan_lcd_dots", ColorImage { size: [2, 2], pixels }, opts);
    let tid = handle.id();
    ctx.data_mut(|d| d.insert_temp(id, handle));
    tid
}

/// Bar fill: glossy chrome (`Theme::gloss`: a bright upper half, a clear mid-line, a darker lower half with a
/// bounce of light at the bottom), multiplied by `tint`, with the faint streak texture over it (bars only:
/// UI_SPEC.md §5 Bar). Square: bars run edge to edge.
pub fn metal_tinted(p: &Painter, r: Rect, m: &Metal, tint: Color32) {
    let r = px(r);
    if !r.is_positive() {
        return;
    }
    let mul = |c: Color32| {
        let f = |a: u8, b: u8| ((a as u32 * b as u32 + 127) / 255) as u8;
        Color32::from_rgb(f(c.r(), tint.r()), f(c.g(), tint.g()), f(c.b(), tint.b()))
    };
    let stops = theme::get().gloss_bar.stops(0.5).map(|(f, c)| (f, mul(c)));
    grad(p, r, 0.0, &stops);
    if m.streak > 0.0 {
        let uv = Rect::from_min_max(Pos2::ZERO, pos2(r.width() / STREAK as f32, r.height() / STREAK as f32));
        p.image(streak_tex(p.ctx()), r, uv, Color32::WHITE.gamma_multiply(m.streak));
    }
}

pub fn metal(p: &Painter, r: Rect, m: &Metal) {
    metal_tinted(p, r, m, Color32::WHITE);
}

/// A bar (menu row, toolbars, transport, status, ruler band): glossy chrome with a 2 px light lip along the top
/// and a 2 px dark edge along the bottom (UI_SPEC.md §4.1, thick bevels).
pub fn metal_bar(p: &Painter, r: Rect, m: &Metal) {
    let r = px(r);
    metal(p, r, m);
    let o = hair();
    hl(p, r.min.x, r.max.x, r.min.y, m.light);
    hl(p, r.min.x, r.max.x, r.min.y + o, m.light.gamma_multiply(0.55));
    hl(p, r.min.x, r.max.x, r.max.y - 2.0 * o, m.shadow);
    hl(p, r.min.x, r.max.x, r.max.y - o, m.dark);
}

/// The thick rounded rim inside `r` (UI_SPEC.md §4.1): two rings, light on the upper half and dark on the lower half
/// (swapped when `inverted`, a pressed face), each one hairline wide.
fn rim(p: &Painter, r: Rect, radius: f32, light: [Color32; 2], dark: [Color32; 2], inverted: bool) {
    let o = hair();
    let mid = r.center().y.round();
    let (top, bottom) = (r.with_max_y(mid).intersect(p.clip_rect()), r.with_min_y(mid).intersect(p.clip_rect()));
    let (upper, lower) = if inverted { (dark, light) } else { (light, dark) };
    for k in 0..2 {
        let rr = r.shrink(o * (k + 1) as f32);
        if !rr.is_positive() || rr.height() < 2.0 * o {
            break;
        }
        let rad = (radius - o * (k + 1) as f32).max(0.0);
        p.with_clip_rect(top).rect_stroke(rr, rad, Stroke::new(o, upper[k]), StrokeKind::Inside);
        p.with_clip_rect(bottom).rect_stroke(rr, rad, Stroke::new(o, lower[k]), StrokeKind::Inside);
    }
}

// ---- Bevels and recesses ----

/// A square 2 px edge inside `r` (UI_SPEC.md §4.1): raised = `s0` lip on the top / left inside an `s6` outline whose
/// bottom / right rows are shaded `s5`; sunken = the recess (an `s5` outline, a dark inner top row, an `s0` lower lip).
pub fn bevel(p: &Painter, r: Rect, raised: bool, m: &Metal) {
    let r = px(r);
    let o = hair();
    if raised {
        p.rect_stroke(r, 0.0, Stroke::new(o, m.dark), StrokeKind::Inside);
        let i = r.shrink(o);
        hl(p, i.min.x, i.max.x, i.min.y, m.light);
        vl(p, i.min.x, i.min.y, i.max.y, m.light);
        hl(p, i.min.x, i.max.x, i.max.y - o, m.shadow);
        vl(p, i.max.x - o, i.min.y, i.max.y, m.shadow);
    } else {
        recess_edge(p, r, 0.0, m);
    }
}

/// The recess edge (UI_SPEC.md §4.1) round `r` with corners of `radius`: an `s5` outline, a 2 px inner shadow under
/// the top (black 30 % then 12 %), the light lower lip along the bottom inside.
pub fn recess_edge(p: &Painter, r: Rect, radius: f32, m: &Metal) {
    recess_edge_in(p, r, radius, m.shadow, m.light.gamma_multiply(0.8));
}

fn recess_edge_in(p: &Painter, r: Rect, radius: f32, outline: Color32, lip: Color32) {
    let r = px(r);
    let o = hair();
    let inset = radius.max(o);
    hl(p, r.min.x + inset, r.max.x - inset, r.min.y + o, Color32::from_black_alpha(77));
    hl(p, r.min.x + inset, r.max.x - inset, r.min.y + 2.0 * o, Color32::from_black_alpha(31));
    hl(p, r.min.x + inset, r.max.x - inset, r.max.y - 2.0 * o, lip);
    p.rect_stroke(r, radius, Stroke::new(o, outline), StrokeKind::Inside);
}

/// Thin 1 px edge (lip top-left, shade bottom-right; swapped when sunken): grooves and panes.
pub fn edge(p: &Painter, r: Rect, raised: bool, m: &Metal) {
    let r = px(r);
    let o = hair();
    let (tl, br) = if raised { (m.light, m.shadow) } else { (m.shadow, m.light) };
    hl(p, r.min.x, r.max.x - o, r.min.y, tl);
    vl(p, r.min.x, r.min.y, r.max.y - o, tl);
    hl(p, r.min.x, r.max.x, r.max.y - o, br);
    vl(p, r.max.x - o, r.min.y, r.max.y, br);
}

/// How a chrome control is drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Chrome {
    /// Raised chrome.
    Raised,
    /// Raised, the gradient lifted toward `s0`, a `lit.outline` outline.
    Hover,
    /// Pressed in: the gradient inverted and one step darker.
    Pressed,
    /// Lit: a toggle that is on (sky-blue chrome, `lit.outline`).
    Active,
    /// A slider or fader knob (`gloss_knob`; the same as Raised on Silver).
    Knob,
}

/// Chrome over `r` with corners of `radius` (UI_SPEC.md §4.1 bevel recipe): the state's glossy gradient (bright
/// upper half, a clear mid-line, darker lower half), a thick rounded rim (two light rings over the upper half, two
/// dark rings over the lower; inverted when pressed) and the outline round it. Faces under 12 px get a thin rim.
pub fn chrome(p: &Painter, r: Rect, radius: f32, state: Chrome) {
    let t = theme::get();
    let (s, sel) = (t.silver, t.sel);
    let r = px(r);
    if !r.is_positive() {
        return;
    }
    let o = hair();
    let (gloss, outline) = match state {
        Chrome::Raised => (t.gloss, s.s6),
        Chrome::Knob => (t.gloss_knob, s.s6),
        Chrome::Hover => (theme::Gloss { upper: s.s0, line: t.gloss.upper, low: t.gloss.line, bounce: t.gloss.upper, ..t.gloss }, sel.lit_outline),
        Chrome::Pressed => (t.gloss_pressed, s.s6),
        Chrome::Active => (t.gloss_lit, sel.lit_outline),
    };
    grad(p, r, radius, &gloss.stops(0.5));
    let thin = r.height().min(r.width()) < 12.0;
    let white = t.bevel.lip;
    let (light, dark) = match state {
        Chrome::Active => ([white.gamma_multiply(0.9), white.gamma_multiply(0.4)], [sel.lit_outline.gamma_multiply(0.55), sel.lit_outline.gamma_multiply(0.2)]),
        _ => ([white, white.gamma_multiply(0.45)], [s.s5, s.s5.gamma_multiply(0.35)]),
    };
    if thin {
        rim(p, r, radius, [light[0], Color32::TRANSPARENT], [dark[1], Color32::TRANSPARENT], state == Chrome::Pressed);
    } else {
        rim(p, r, radius, light, dark, state == Chrome::Pressed);
    }
    p.rect_stroke(r, radius, Stroke::new(o, outline), StrokeKind::Inside);
}

/// A chrome button over `r` (toolbars, header buttons, chooser chips): `chrome` at the size's radius.
pub fn chrome_button(p: &Painter, r: Rect, state: Chrome, _m: &Metal) {
    let t = theme::get();
    chrome(p, r, ctl_radius(r, &t), state);
}

/// A recessed well over `r` (fields, panes, list boxes): `fill` with the recess edge, corners `r_ctl`. A dark `fill`
/// (navy) takes the LCD outline.
pub fn sunken(p: &Painter, r: Rect, fill: Color32, m: &Metal) {
    let t = theme::get();
    well(p, r, t.m.r_ctl as f32, fill, m);
}

pub fn well(p: &Painter, r: Rect, radius: f32, fill: Color32, m: &Metal) {
    let r = px(r);
    p.rect_filled(r, radius, fill);
    let dark = fill.r() as u32 + fill.g() as u32 + fill.b() as u32 <= 3 * 96;
    if dark {
        let t = theme::get();
        recess_edge_in(p, r, radius, t.lcd.outline, Color32::from_white_alpha(14));
    } else {
        recess_edge(p, r, radius, m);
    }
}

/// An LCD well (UI_SPEC.md §5): `lcd.deep` for read-outs (with the faint dot grid when `dots`), `lcd.well` for lists.
pub fn lcd_well(p: &Painter, r: Rect, deep: bool, dots: bool) {
    let t = theme::get();
    let r = px(r);
    let radius = t.m.r_ctl as f32;
    p.rect_filled(r, radius, if deep { t.lcd.deep } else { t.lcd.well });
    if dots && r.width() > 8.0 && r.height() > 8.0 {
        let inner = r.shrink(2.0);
        let uv = Rect::from_min_max(pos2(inner.min.x / 2.0, inner.min.y / 2.0), pos2(inner.max.x / 2.0, inner.max.y / 2.0));
        p.image(dots_tex(p.ctx()), inner, uv, Color32::WHITE.gamma_multiply(0.35));
    }
    recess_edge_in(p, r, radius, t.lcd.outline, Color32::from_white_alpha(14));
}

/// The edge of a navy well whose fill egui painted (a `Frame`): the LCD outline, inner shadow and lip.
pub fn lcd_well_edge(p: &Painter, r: Rect) {
    let t = theme::get();
    recess_edge_in(p, r, t.m.r_ctl as f32, t.lcd.outline, Color32::from_white_alpha(14));
}

/// Groove: an `s5` line with an `s0` line beside it (vertical over `y` at `x`): separators.
pub fn etched_v(p: &Painter, x: f32, y: Rangef, m: &Metal) {
    let x = theme::snap(x.floor());
    vl(p, x, y.min, y.max, m.shadow);
    vl(p, x + hair(), y.min, y.max, m.light);
}

pub fn etched_h(p: &Painter, x: Rangef, y: f32, m: &Metal) {
    let y = theme::snap(y.floor());
    hl(p, x.min, x.max, y, m.shadow);
    hl(p, x.min, x.max, y + hair(), m.light);
}

/// One grip dimple at `at` (its top-left): a 2×2 `s5` dot with an `s0` pixel at its lower right.
fn dimple(p: &Painter, at: Pos2, m: &Metal) {
    let o = hair();
    p.rect_filled(Rect::from_min_size(at + vec2(o, o), vec2(2.0 * o, 2.0 * o)), 0.0, m.light);
    p.rect_filled(Rect::from_min_size(at, vec2(2.0 * o, 2.0 * o)), 0.0, m.shadow);
}

/// Toolbar grip: a column of dimples down the bar's left end.
pub fn grip(p: &Painter, r: Rect, m: &Metal) {
    let x = r.min.x.floor() + 3.0;
    let mut y = r.min.y.floor() + 5.0;
    while y + 3.0 <= r.max.y - 4.0 {
        dimple(p, pos2(x, y), m);
        y += 4.0;
    }
}

/// Splitter between bands or panes: `slate.frame` with three dimples at its middle; `across` = a horizontal bar.
pub fn dotted_splitter(p: &Painter, r: Rect, across: bool, m: &Metal) {
    let t = theme::get();
    let r = px(r);
    p.rect_filled(r, 0.0, t.slate.frame);
    let c = r.center().floor();
    let dm = Metal { light: t.silver.s3, shadow: t.lcd.outline, ..*m };
    for k in -1..=1 {
        let d = 5.0 * k as f32;
        let at = if across { pos2(c.x + d - 1.0, c.y - 1.0) } else { pos2(c.x - 1.0, c.y + d - 1.0) };
        dimple(p, at, &dm);
    }
}

/// Slider track over `x` centred on `cy`: a 4 px recessed pill in `s4` with an `s5` top edge and an `s0` lower lip.
pub fn groove(p: &Painter, x: Rangef, cy: f32, m: &Metal) {
    groove_value(p, x, cy, None, m);
}

/// `groove` with the part left of `value_x` filled with the selection blue (the value fill).
pub fn groove_value(p: &Painter, x: Rangef, cy: f32, value_x: Option<f32>, m: &Metal) {
    let t = theme::get();
    let cy = theme::snap(cy.floor());
    let o = hair();
    // A dark chrome slot (UI_SPEC.md §5 Slider): slate inside, a black inner shadow under its top, a light lip under
    // its bottom on the chrome, the value part in the selection blue.
    let r = px(Rect::from_x_y_ranges(x, Rangef::new(cy - 2.0 * o - 1.0, cy + 2.0 * o + 1.0)));
    let rad = r.height() * 0.5;
    p.rect_filled(r, rad, t.slate.lo);
    if let Some(vx) = value_x {
        let f = Rect::from_min_max(r.min, pos2(vx.clamp(r.min.x, r.max.x), r.max.y));
        if f.width() >= r.height() {
            grad(p, f, rad, &[(0.0, t.sel.top), (1.0, t.sel.bottom)]);
        }
    }
    hl(p, r.min.x + rad, r.max.x - rad, r.min.y + o, Color32::from_black_alpha(90));
    hl(p, r.min.x + rad, r.max.x - rad, r.max.y, m.light);
    p.rect_stroke(r, rad, Stroke::new(o, m.dark), StrokeKind::Inside);
}

/// Vertical slider track down `y` centred on `cx` (fader tracks): the dark chrome slot standing up, its value part
/// below `value_y` in the selection blue.
pub fn groove_v(p: &Painter, cx: f32, y: Rangef, m: &Metal) {
    groove_v_value(p, cx, y, None, m);
}

pub fn groove_v_value(p: &Painter, cx: f32, y: Rangef, value_y: Option<f32>, m: &Metal) {
    let t = theme::get();
    let cx = theme::snap(cx.floor());
    let o = hair();
    let r = px(Rect::from_x_y_ranges(Rangef::new(cx - 2.0 * o - 1.0, cx + 2.0 * o + 1.0), y));
    let rad = r.width() * 0.5;
    p.rect_filled(r, rad, t.slate.lo);
    if let Some(vy) = value_y {
        let f = Rect::from_min_max(pos2(r.min.x, vy.clamp(r.min.y, r.max.y)), r.max);
        if f.height() >= r.width() {
            grad(p, f, rad, &[(0.0, t.sel.top), (1.0, t.sel.bottom)]);
        }
    }
    vl(p, r.min.x + o, r.min.y + rad, r.max.y - rad, Color32::from_black_alpha(90));
    vl(p, r.max.x, r.min.y + rad, r.max.y - rad, m.light);
    p.rect_stroke(r, rad, Stroke::new(o, m.dark), StrokeKind::Inside);
}

/// Slider knob centred in `r`: a lozenge of raised chrome at least 15 × 9 (pill-shaped, wider than tall) with a dark
/// centre notch; `tint` (automation) colours the notch and the knob faintly.
pub fn thumb(p: &Painter, r: Rect, m: &Metal, tint: Option<Color32>) {
    let k = px(Rect::from_center_size(r.center(), vec2(r.width().max(17.0), r.height().min(11.0).max(9.0))));
    chrome(p, k, k.height() * 0.5, Chrome::Knob);
    if let Some(t) = tint {
        p.rect_filled(k.shrink(2.0), k.height() * 0.5 - 2.0, t.gamma_multiply(0.22));
    }
    let c = k.center().floor();
    let w = if tint.is_some() { 2.0 } else { 1.0 };
    p.rect_filled(Rect::from_min_max(pos2(c.x, k.min.y + 2.0), pos2(c.x + w, k.max.y - 2.0)), 0.0, tint.unwrap_or(m.dark));
}

/// Knob of a vertical fader in `r`: a pill of raised chrome with the notch across it.
pub fn fader_thumb(p: &Painter, r: Rect, m: &Metal, tint: Option<Color32>) {
    let r = px(r);
    let rad = r.width().min(r.height()) * 0.5;
    chrome(p, r, rad, Chrome::Knob);
    if let Some(t) = tint {
        p.rect_filled(r.shrink(2.0), rad - 2.0, t.gamma_multiply(0.22));
    }
    let c = r.center().floor();
    p.rect_filled(Rect::from_min_max(pos2(r.min.x + 3.0, c.y), pos2(r.max.x - 3.0, c.y + 1.0)), 0.0, tint.unwrap_or(m.dark));
}

/// Dotted 1 px line (a pixel every 2 px) from `a` to `b`, horizontal or vertical: tree lines, focus rectangles.
pub fn dotted(p: &Painter, a: Pos2, b: Pos2, c: Color32) {
    let (a, b) = (a.floor(), b.floor());
    let mut mesh = Mesh::default();
    // Dots on even pixels, so segments that meet keep one phase.
    let even = |v: f32| if v.rem_euclid(2.0) == 0.0 { v } else { v + 1.0 };
    if (a.y - b.y).abs() < 0.5 {
        let mut x = even(a.x.min(b.x));
        while x <= a.x.max(b.x) {
            mesh.add_colored_rect(Rect::from_min_size(pos2(x, a.y), vec2(1.0, 1.0)), c);
            x += 2.0;
        }
    } else {
        let mut y = even(a.y.min(b.y));
        while y <= a.y.max(b.y) {
            mesh.add_colored_rect(Rect::from_min_size(pos2(a.x, y), vec2(1.0, 1.0)), c);
            y += 2.0;
        }
    }
    p.add(mesh);
}

/// LED meter: 2 px segments on a 3 px pitch from the well's bottom up to `y`, green below −12 dB, yellow to −3 dB, red
/// above (`to_y` maps dB to y). The navy well shows between the segments.
pub fn led_bar(p: &Painter, well: Rect, y: f32, to_y: &dyn Fn(f32) -> f32, t: &Theme) {
    let (y12, y3) = (to_y(-12.0), to_y(-3.0));
    let mut mesh = Mesh::default();
    let mut top = well.max.y.floor() - 3.0;
    while top + 1.0 >= y.max(well.min.y) && top >= well.min.y - 1.0 {
        let c = if top < y3 { t.meter_red } else if top < y12 { t.meter_yellow } else { t.meter_green };
        mesh.add_colored_rect(Rect::from_x_y_ranges(well.x_range(), Rangef::new(top.max(well.min.y), top + 2.0)), c);
        top -= 3.0;
    }
    p.add(mesh);
}

/// Meter well: an LCD well (`lcd.deep`) round the meter, 2 px outside it.
pub fn meter_well(p: &Painter, well: Rect, t: &Theme) {
    let r = px(well.expand(2.0));
    p.rect_filled(r, t.m.r_small as f32, t.meter_bg);
    recess_edge_in(p, r, t.m.r_small as f32, t.lcd.outline, Color32::from_white_alpha(14));
}

// ---- The design system's own pieces ----

/// The selection bar over `r` (UI_SPEC.md §5 Selection): `sel.top` → `sel.bottom` with the `sel.lip` line on top.
pub fn selection_bar(p: &Painter, r: Rect, radius: f32) {
    let t = theme::get();
    let r = px(r);
    grad(p, r, radius, &[(0.0, t.sel.top), (1.0, t.sel.bottom)]);
    if r.height() >= 6.0 {
        hl(p, r.min.x + radius.max(1.0), r.max.x - radius.max(1.0), r.min.y, t.sel.lip);
    }
}

/// The amber lamp along `r` (2 px: `amber` with an `amber.hi` line over it): the active dock tab, record armed.
pub fn lamp(p: &Painter, r: Rect) {
    let t = theme::get();
    let r = px(r);
    p.rect_filled(r, 1.0, t.amber.base);
    hl(p, r.min.x + 1.0, r.max.x - 1.0, r.min.y, t.amber.hi);
    p.rect_stroke(r, 1.0, Stroke::new(hair(), t.amber.lo.gamma_multiply(0.6)), StrokeKind::Inside);
}

/// The rounded edge of a framed region `r` already painted square (a dock group, a floating window): the corners
/// outside a `rad` arc filled with `outer` (the slate behind the frame), then the `s6` outline round it.
pub fn frame_edge(p: &Painter, r: Rect, rad: f32, outer: Color32) {
    use std::f32::consts::{FRAC_PI_2, PI};
    let t = theme::get();
    let r = px(r);
    let n = 6;
    let mut mesh = Mesh::default();
    let corners = [
        (r.left_top(), r.left_top() + vec2(rad, rad), PI),
        (r.right_top(), r.right_top() + vec2(-rad, rad), 1.5 * PI),
        (r.right_bottom(), r.right_bottom() + vec2(-rad, -rad), 0.0),
        (r.left_bottom(), r.left_bottom() + vec2(rad, -rad), 0.5 * PI),
    ];
    for (corner, center, a0) in corners {
        let base = mesh.vertices.len() as u32;
        mesh.colored_vertex(corner, outer);
        for i in 0..=n {
            mesh.colored_vertex(center + Vec2::angled(a0 + FRAC_PI_2 * i as f32 / n as f32) * rad, outer);
        }
        for i in 0..n as u32 {
            mesh.add_triangle(base, base + 1 + i, base + 2 + i);
        }
    }
    p.add(mesh);
    // The panel's chrome rim: a light ring over its upper half and a dark one over its lower half, inside the
    // outline (the content starts 2 px in).
    rim(p, r, rad, [t.bevel.lip, Color32::TRANSPARENT], [t.silver.s5, Color32::TRANSPARENT], false);
    p.rect_stroke(r, rad, Stroke::new(hair(), t.slate.lo), StrokeKind::Inside);
}

/// A grooved title band with a pill title tab (UI_SPEC.md §5 Panel frame), as the period skins frame a panel: twin
/// grooves along `band` from its left end to a 16 px chrome pill holding `title` at its right end. Nothing when the
/// band is too short for the pill and 24 px of groove.
pub fn title_band(p: &Painter, band: Rect, title: &str) {
    let t = theme::get();
    let font = t.bold(t.font.ui);
    let tw = p.layout_no_wrap(title.to_owned(), font.clone(), t.ink.text).size().x + 18.0;
    if band.width() < tw + 24.0 {
        return;
    }
    let cy = theme::snap(band.center().y.round());
    let pill = px(Rect::from_min_size(pos2(band.max.x - tw, cy - 8.0), vec2(tw, 16.0)));
    let m = &t.metal;
    let x = Rangef::new(band.min.x, pill.min.x - 6.0);
    etched_h(p, x, cy - 3.0, m);
    etched_h(p, x, cy + 1.0, m);
    chrome(p, pill, 8.0, Chrome::Raised);
    p.text(pill.center(), Align2::CENTER_CENTER, title, font, t.ink.text);
}

/// A tab over `r` (UI_SPEC.md §5 Dock tabs): chrome with `r_ctl` corners away from the content and square ones at
/// it; `on_content` = the content is below (tabs standing on it) rather than above (tabs hanging under it). Inactive
/// tabs are a step darker (lifted when hovered); the active one joins the content and, with `lamp`, carries the amber
/// lamp along its outer edge.
pub fn tab_face(p: &Painter, r: Rect, active: bool, hovered: bool, on_content: bool, lamp_on: bool) {
    let t = theme::get();
    let s = &t.silver;
    let r = px(r);
    let rad = t.m.r_ctl as f32;
    // Painted longer toward the content and clipped, so only the outer corners are rounded; an inactive tab keeps
    // its edge at the content.
    let grow = rad + 2.0;
    let full = if on_content { r.with_max_y(r.max.y + grow) } else { r.with_min_y(r.min.y - grow) };
    let clip = if active { r } else if on_content { r.with_max_y(r.max.y - 1.0) } else { r.with_min_y(r.min.y + 1.0) };
    let pc = p.with_clip_rect(clip.intersect(p.clip_rect()));
    if active {
        chrome(&pc, full, rad, Chrome::Raised);
        if lamp_on {
            let y = if on_content { r.min.y + 2.0 } else { r.max.y - 4.0 };
            lamp(&pc, Rect::from_min_max(pos2(r.min.x + rad + 2.0, y), pos2(r.max.x - rad - 2.0, y + 2.0)));
        }
    } else if hovered {
        chrome(&pc, full, rad, Chrome::Hover);
    } else {
        grad(&pc, full, rad, &[(0.0, s.s2), (1.0, s.s4)]);
        pc.rect_stroke(full, rad, Stroke::new(hair(), s.s6), StrokeKind::Inside);
        hl(&pc, r.min.x + rad, r.max.x - rad, if on_content { r.min.y + 1.0 } else { r.max.y - 2.0 }, s.s1.gamma_multiply(0.7));
    }
}

/// Toggle pill over `r` (UI_SPEC.md §5): pill-shaped chrome with an `ink.button` label; on = the selection gradient
/// with a white label (the lit "ON" pill); hovered = lifted chrome.
pub fn pill_paint(p: &Painter, r: Rect, label: &str, on: bool, hovered: bool, enabled: bool) {
    let t = theme::get();
    let r = px(r);
    let rad = r.height() * 0.5;
    let ink = if on {
        selection_bar(p, r, rad);
        p.rect_stroke(r, rad, Stroke::new(hair(), t.lcd.outline), StrokeKind::Inside);
        t.sel.text
    } else {
        chrome(p, r, rad, if hovered && enabled { Chrome::Hover } else { Chrome::Raised });
        if enabled { t.ink.button } else { t.ink.disabled }
    };
    p.with_clip_rect(r.intersect(p.clip_rect())).text(r.center(), Align2::CENTER_CENTER, label, t.font(t.font.ui), ink);
}

/// A section title tab (UI_SPEC.md §5): a 16 px pill of raised chrome holding `text` in Tahoma Bold, set at the left
/// of `row`, with a groove running from it to the row's right end. Returns the pill's rect.
pub fn section_tab(p: &Painter, row: Rect, text: &str) -> Rect {
    let t = theme::get();
    let font = t.bold(t.font.ui);
    let w = p.layout_no_wrap(text.to_owned(), font.clone(), t.ink.text).size().x + 16.0;
    let cy = row.center().y.round();
    let r = px(Rect::from_min_size(pos2(row.min.x, cy - 8.0), vec2(w, 16.0)));
    etched_h(p, Rangef::new(r.max.x + 2.0, row.max.x), cy, &t.metal);
    chrome(p, r, 8.0, Chrome::Raised);
    p.text(r.center(), Align2::CENTER_CENTER, text, font, t.ink.text);
    r
}

// ---- Segmented LCD digits (UI_SPEC.md §3) ----

/// Segments of a digit: a top, b top right, c bottom right, d bottom, e bottom left, f top left, g middle.
fn segments(ch: char) -> Option<u8> {
    Some(match ch {
        '0' => 0b0111111,
        '1' => 0b0000110,
        '2' => 0b1011011,
        '3' => 0b1001111,
        '4' => 0b1100110,
        '5' => 0b1101101,
        '6' => 0b1111101,
        '7' => 0b0000111,
        '8' => 0b1111111,
        '9' => 0b1101111,
        '-' => 0b1000000,
        _ => return None,
    })
}

/// Advance of `ch` at digit height `h`.
fn lcd_advance(ch: char, h: f32) -> f32 {
    match ch {
        ':' | ';' | '.' | ',' => h * 0.28,
        ' ' => h * 0.3,
        _ if segments(ch).is_some() => h * 0.6,
        _ => h * 0.62,
    }
}

/// Width of `text` as `lcd_digits` paints it at height `h`.
pub fn lcd_width(text: &str, h: f32) -> f32 {
    text.chars().map(|c| lcd_advance(c, h)).sum()
}

/// `text` as seven-segment LCD digits `h` px tall, slanted 8 %, anchored by `align` at `at` (UI_SPEC.md §3): solid
/// bevelled segments (anti-aliased hexagons, a lighter core over the lit colour) with a soft glow round them, the
/// unlit ones of each digit in `ghost` (as a real LCD shows them). `:` `;` `.` `,` are round dots; other characters
/// fall back to Tahoma Bold. Returns the painted rect.
pub fn lcd_digits(p: &Painter, at: Pos2, align: Align2, text: &str, h: f32, lit: Color32, ghost: Color32) -> Rect {
    let t = theme::get();
    let w = lcd_width(text, h);
    let r = align.anchor_size(at, vec2(w, h));
    let th = (h * 0.19).max(2.0);
    let slant = 0.08;
    // x shifts right as y goes up.
    let sk = |pt: Pos2| pos2(pt.x + (r.max.y - pt.y) * slant, pt.y);
    let edge = mix(lit, t.lcd.text, 0.35);
    let glow = t.lcd.text.gamma_multiply(0.16);
    let mut ghosts: Vec<Shape> = Vec::new();
    let mut glows: Vec<Shape> = Vec::new();
    let mut lits: Vec<Shape> = Vec::new();
    let poly = |pts: [Pos2; 6], c: Color32, grow: f32| {
        let mid = pts.iter().fold(Vec2::ZERO, |a, q| a + q.to_vec2()) / 6.0;
        let pts: Vec<Pos2> = pts.iter().map(|q| sk(*q + (q.to_vec2() - mid).normalized() * grow)).collect();
        Shape::convex_polygon(pts, c, Stroke::NONE)
    };
    let mut x = r.min.x;
    for ch in text.chars() {
        let adv = lcd_advance(ch, h);
        if let Some(bits) = segments(ch) {
            let dw = adv - h * 0.14;
            let (x0, x1) = (x + h * 0.04, x + h * 0.04 + dw);
            let (y0, ym, y1) = (r.min.y, r.min.y + h * 0.5, r.max.y);
            let g = th * 0.5;
            // Each segment runs between the centre lines of its neighbours, less a hairline gap, with pointed ends.
            let gap = (h * 0.008).max(0.25);
            let hseg = |y: f32| {
                let (xa, xb) = (x0 + g + gap, x1 - g - gap);
                [pos2(xa, y), pos2(xa + g, y - g), pos2(xb - g, y - g), pos2(xb, y), pos2(xb - g, y + g), pos2(xa + g, y + g)]
            };
            let vseg = |xx: f32, ya: f32, yb: f32| {
                let (ya, yb) = (ya + gap, yb - gap);
                [pos2(xx, ya), pos2(xx + g, ya + g), pos2(xx + g, yb - g), pos2(xx, yb), pos2(xx - g, yb - g), pos2(xx - g, ya + g)]
            };
            let segs = [
                hseg(y0 + g),
                vseg(x1 - g, y0 + g, ym),
                vseg(x1 - g, ym, y1 - g),
                hseg(y1 - g),
                vseg(x0 + g, ym, y1 - g),
                vseg(x0 + g, y0 + g, ym),
                hseg(ym),
            ];
            for (k, s) in segs.into_iter().enumerate() {
                if bits & (1 << k) != 0 {
                    glows.push(poly(s, glow, (h * 0.07).max(1.2)));
                    // Solid: the lit colour over the whole segment, a lighter sheen down its middle.
                    lits.push(poly(s, edge, 0.25));
                    lits.push(poly(s, lit, -(th * 0.12).max(0.35)));
                } else {
                    ghosts.push(poly(s, mix(ghost, t.lcd.deep, 0.35), 0.0));
                }
            }
        } else if matches!(ch, ':' | ';' | '.' | ',') {
            let d = th * 0.62;
            let cx = x + adv * 0.5;
            let mut put = |y: f32| {
                let c = sk(pos2(cx, y));
                glows.push(Shape::circle_filled(c, d + (h * 0.07).max(1.2), glow));
                lits.push(Shape::circle_filled(c, d, lit));
            };
            if matches!(ch, ':' | ';') {
                put(r.min.y + h * 0.3);
            }
            put(r.max.y - h * 0.2);
            if matches!(ch, ';' | ',') {
                let c = sk(pos2(cx - d * 0.5, r.max.y - h * 0.2 + d * 1.6));
                lits.push(Shape::circle_filled(c, d * 0.55, lit));
            }
        } else if ch != ' ' {
            p.text(pos2(x + adv * 0.5, r.center().y), Align2::CENTER_CENTER, ch, t.bold((h * 0.8).round()), lit);
        }
        x += adv;
    }
    p.extend(ghosts);
    p.extend(glows);
    p.extend(lits);
    r
}

/// A read-out value on an LCD: segmented digits `h` px tall when `text` is all digits, else Tahoma Bold at the size
/// that matches (`lcd.digits` lit, `lcd.ghost` unlit). Returns the painted rect.
pub fn lcd_value(p: &Painter, at: Pos2, align: Align2, text: &str, h: f32) -> Rect {
    let t = theme::get();
    if lcd_paintable(text) {
        lcd_digits(p, at, align, text, h, t.lcd.digits, t.lcd.ghost)
    } else {
        p.text(at, align, text, t.bold((h * 0.8).round().max(11.0)), t.lcd.digits)
    }
}

/// True when `text` paints entirely as segments (digits and their separators).
pub fn lcd_paintable(text: &str) -> bool {
    text.chars().all(|c| segments(c).is_some() || matches!(c, ':' | ';' | '.' | ',' | ' '))
}

/// The brand bitmaps (assets/brand), decoded once per context.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Brand {
    /// milan-pro-256.png: the metallic chrome M.
    Mark,
    /// splash-520x320.png.
    Splash,
    /// about-banner-480x120.png.
    AboutBanner,
}

pub fn brand_tex(ctx: &Context, which: Brand) -> TextureHandle {
    let id = Id::new(("milan_brand", which));
    if let Some(h) = ctx.data(|d| d.get_temp::<TextureHandle>(id)) {
        return h;
    }
    let png: &[u8] = match which {
        Brand::Mark => include_bytes!("../../assets/brand/milan-pro-256.png"),
        Brand::Splash => include_bytes!("../../assets/brand/splash-520x320.png"),
        Brand::AboutBanner => include_bytes!("../../assets/brand/about-banner-480x120.png"),
    };
    let data = eframe::icon_data::from_png_bytes(png).expect("assets/brand PNGs are valid");
    let image = ColorImage::from_rgba_unmultiplied([data.width as usize, data.height as usize], &data.rgba);
    // Nearest when enlarged: the bitmaps are only ever drawn at whole multiples of their pixels (`brand_size`).
    let opts = TextureOptions { magnification: TextureFilter::Nearest, ..TextureOptions::LINEAR };
    let handle = ctx.load_texture(format!("milan_brand_{which:?}"), image, opts);
    ctx.data_mut(|d| d.insert_temp(id, handle.clone()));
    handle
}

/// R48: the size in points a brand bitmap of `px` pixels is drawn at: a whole number of physical pixels per bitmap
/// pixel (1 at 100-175 %, 2 at 200 %), never a blurred 1.5; at 125-175 % it is that much smaller in points.
pub fn brand_size(px: Vec2) -> Vec2 {
    let k = theme::ppp();
    px * k.floor().max(1.0) / k
}

/// The brand bitmap `which` of `px` pixels, centred in `slot`, at `brand_size`, on physical pixel edges.
pub fn brand_image(p: &Painter, slot: Rect, which: Brand, px_size: Vec2) {
    let tex = brand_tex(p.ctx(), which);
    let r = px(Rect::from_center_size(slot.center(), brand_size(px_size)));
    let r = Rect::from_min_size(r.min, brand_size(px_size));
    p.image(tex.id(), r, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
}

/// Filled triangle of a scrollbar / spinner arrow in `r`; `dir` = (dx, dy) it points to.
pub fn arrow(p: &Painter, r: Rect, dir: (f32, f32), ink: Color32) {
    let c = r.center().floor() + vec2(0.5, 0.5);
    let (dx, dy) = dir;
    let (nx, ny) = (-dy, dx);
    let tip = c + vec2(dx, dy) * 2.5;
    let base = c - vec2(dx, dy) * 1.5;
    let pts = vec![tip, base + vec2(nx, ny) * 4.0, base - vec2(nx, ny) * 4.0];
    p.add(Shape::convex_polygon(pts, ink, Stroke::NONE));
}

/// A Mixing Console strip's card: a brushed chrome face (`s1` → `s3`; lit when selected) with `r_frame` corners,
/// the thick rim and the outline.
pub fn strip_card(p: &Painter, r: Rect, selected: bool) {
    let t = theme::get();
    let r = px(r);
    let rad = t.m.r_frame as f32;
    let (s, g) = (t.silver, t.gloss_lit);
    let stops = if selected { [(0.0, g.top), (0.4, g.upper), (1.0, g.low)] } else { [(0.0, s.s1), (0.35, s.s2), (1.0, s.s3)] };
    grad(p, r, rad, &stops);
    rim(p, r, rad, [t.bevel.lip, t.bevel.lip.gamma_multiply(0.4)], [s.s5, s.s5.gamma_multiply(0.35)], false);
    p.rect_stroke(r, rad, Stroke::new(hair(), s.s6), StrokeKind::Inside);
}

/// A track header's card (UI_SPEC.md §5 Track headers): raised chrome, lit when the track is selected.
pub fn header_metal(p: &Painter, r: Rect, t: &Theme, selected: bool) {
    let r = px(r);
    // A softer gloss than a button's (the card is 56 px tall): bright upper half, a mid-line, a darker lower half.
    let g = if selected { t.gloss_lit } else { t.gloss };
    let soft = |c: Color32| mix(c, t.bevel.soften, 0.3);
    grad(p, r, 0.0, &[(0.0, g.top), (0.47, soft(g.upper)), (0.53, soft(g.line)), (1.0, soft(g.low))]);
    let o = hair();
    hl(p, r.min.x, r.max.x, r.min.y + o, t.bevel.lip.gamma_multiply(0.6));
    vl(p, r.min.x, r.min.y, r.max.y, t.bevel.lip.gamma_multiply(0.8));
    vl(p, r.max.x - o, r.min.y, r.max.y, t.silver.s5);
}

// ---- Controls painted over egui's (UI_SPEC.md §5) ----
//
// egui draws its check boxes, radio buttons, combo boxes, drag values, sliders, progress bars, text fields and push
// buttons flat. The wrappers below let egui lay out and run the control (clicks, drags, keyboard, the combo list) and
// paint the design system's control over it: `check_box`, `radio_mark`, `combo_frame`, `spin_buttons`,
// `progress_blocks`, `classic_push` and the recessed field edge.

/// The chrome palette the controls on this `ui` are drawn with (one silver everywhere; no streaks on controls).
pub fn skin(_ui: &Ui, t: &Theme) -> Metal {
    t.dialog_metal()
}

/// Filled triangle pointing `dir` ((dx, dy)) in `r`: `half` px each side of the centre line, `h` px tall.
fn tri(p: &Painter, r: Rect, dir: (f32, f32), half: f32, h: f32, ink: Color32) {
    let c = r.center().floor() + vec2(0.5, 0.5);
    let (dx, dy) = dir;
    let (nx, ny) = (-dy, dx);
    let tip = c + vec2(dx, dy) * (h * 0.5 + 0.5);
    let base = c - vec2(dx, dy) * (h * 0.5 - 0.5);
    p.add(Shape::convex_polygon(vec![tip, base + vec2(nx, ny) * half, base - vec2(nx, ny) * half], ink, Stroke::NONE));
}

/// Check box in `r` (13 × 13 centred): `paper` with the recess edge and `r_small`, a 2 px anti-aliased tick in
/// `ink.glyph` (`ink.disabled` on `paper.disabled` when disabled). `_ink` is kept for callers of the old painter.
pub fn check_box(p: &Painter, r: Rect, checked: bool, enabled: bool, _ink: Color32, m: &Metal) {
    let t = theme::get();
    let b = px(Rect::from_center_size(r.center(), vec2(13.0, 13.0)));
    let rad = t.m.r_small as f32;
    p.rect_filled(b, rad, if enabled { t.ink.paper } else { t.ink.paper_disabled });
    recess_edge(p, b, rad, &Metal { shadow: t.bevel.field_edge, ..*m });
    if checked {
        let c = if enabled { t.ink.glyph } else { t.ink.disabled };
        let o = b.min;
        let pts = vec![o + vec2(3.0, 6.5), o + vec2(5.5, 9.0), o + vec2(10.0, 3.8)];
        p.add(Shape::line(pts, Stroke::new(2.0_f32, c)));
    }
}

/// Radio button in `r` (13 px disc centred): `paper`, an `s5` outline, a dark inner top arc; on = a 5 px dot of the
/// selection blue with a white glint.
pub fn radio_mark(p: &Painter, r: Rect, on: bool, enabled: bool, _ink: Color32, m: &Metal) {
    use std::f32::consts::PI;
    let t = theme::get();
    let c = r.center().floor() + vec2(0.5, 0.5);
    let rad = 6.0;
    p.circle_filled(c, rad, if enabled { t.ink.paper } else { t.ink.paper_disabled });
    let pts: Vec<Pos2> = (0..=12).map(|i| c + Vec2::angled(PI * 1.1 + PI * 0.8 * i as f32 / 12.0) * (rad - 1.2)).collect();
    p.add(Shape::line(pts, Stroke::new(1.0_f32, Color32::from_black_alpha(60))));
    let _ = m;
    p.circle_stroke(c, rad - 0.5, Stroke::new(1.0_f32, t.bevel.field_edge));
    if on {
        let dot = if enabled { t.sel.mid } else { t.ink.disabled };
        p.circle_filled(c, 3.0, dot);
        p.circle_filled(c + vec2(-0.8, -0.8), 1.0, Color32::from_white_alpha(170));
    }
}

/// Width of a combo box's arrow button.
const COMBO_ARROW_W: f32 = 18.0;

/// Combo box over `r`: the recess edge round egui's paper field and a chrome arrow button inside its right end
/// holding an `ink.glyph` ▾, pressed while the list is open.
pub fn combo_frame(p: &Painter, r: Rect, open: bool, _ink: Color32, m: &Metal) {
    let t = theme::get();
    let r = px(r);
    recess_edge(p, r, t.m.r_ctl as f32, m);
    let b = Rect::from_min_max(pos2(r.max.x - 2.0 - COMBO_ARROW_W, r.min.y + 2.0), r.max - vec2(2.0, 2.0));
    chrome(p, b, t.m.r_small as f32, if open { Chrome::Pressed } else { Chrome::Raised });
    let shift = if open { vec2(1.0, 1.0) } else { Vec2::ZERO };
    tri(p, b.translate(shift), (0.0, 1.0), 3.5, 4.0, t.ink.glyph);
}

/// Width of a spinner's up / down buttons.
const SPIN_W: f32 = 13.0;

/// The up / down buttons of a spinner in `r` (at the right end of its field): two chrome half-buttons with `ink.glyph`
/// triangles; `pressed` = 0 (up) / 1 (down) presses that one.
pub fn spin_buttons(p: &Painter, r: Rect, pressed: Option<usize>, _ink: Color32, _m: &Metal) {
    let t = theme::get();
    let r = px(r);
    let mid = (r.min.y + r.height() * 0.5).floor();
    let halves = [Rect::from_min_max(r.min, pos2(r.max.x, mid)), Rect::from_min_max(pos2(r.min.x, mid), r.max)];
    for (k, b) in halves.into_iter().enumerate() {
        let down = pressed == Some(k);
        chrome(p, b, t.m.r_small as f32, if down { Chrome::Pressed } else { Chrome::Raised });
        let shift = if down { vec2(1.0, 1.0) } else { Vec2::ZERO };
        tri(p, b.translate(shift), (0.0, if k == 0 { -1.0 } else { 1.0 }), 2.5, 3.0, t.ink.glyph);
    }
}

/// Progress bar in `r` (UI_SPEC.md §5): a recessed `lcd.deep` pill, the done part a selection-blue pill with its lip.
/// `_accent` is kept for callers of the old painter.
pub fn progress_blocks(p: &Painter, r: Rect, frac: f32, _accent: Color32, _m: &Metal) {
    let t = theme::get();
    let r = px(r);
    let rad = (r.height() * 0.5).min(t.m.r_ctl as f32 + 3.0);
    p.rect_filled(r, rad, t.lcd.deep);
    let inner = r.shrink(2.0);
    let w = (inner.width() * frac.clamp(0.0, 1.0)).round();
    if w >= 2.0 {
        selection_bar(p, Rect::from_min_size(inner.min, vec2(w, inner.height())), rad - 2.0);
    }
    recess_edge_in(p, r, rad, t.lcd.outline, Color32::from_white_alpha(20));
}

/// Push button of `size` (UI_SPEC.md §5): raised chrome with `r_ctl` corners and an `ink.button` label (`ink`);
/// hovered = lifted with the `lit.outline`; pressed = inverted, the label 1 px down-right; the default button has a
/// 2 px `lit.outline` ring inside its outline and a bold label; disabled = flat `s2`, an `s4` outline and
/// `disabled_ink`.
#[allow(clippy::too_many_arguments)]
pub fn classic_push(ui: &mut Ui, label: &str, size: Vec2, enabled: bool, default: bool, ink: Color32, disabled_ink: Color32, _m: &Metal) -> Response {
    let t = theme::get();
    let font = if default && enabled { t.bold(11.0) } else { t.font(11.0) };
    // Never narrower than its label (egui's `min_size` buttons grow the same way).
    let size = vec2(size.x.max(ui.painter().layout_no_wrap(label.to_owned(), font.clone(), ink).size().x + 16.0), size.y);
    let (r, resp) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    let r = px(r);
    let p = ui.painter();
    let rad = t.m.r_ctl as f32;
    let pressed = enabled && resp.is_pointer_button_down_on();
    if enabled {
        chrome(p, r, rad, if pressed { Chrome::Pressed } else if resp.hovered() { Chrome::Hover } else { Chrome::Raised });
        if default {
            p.rect_stroke(r.shrink(hair()), rad - 1.0, Stroke::new(2.0_f32, t.sel.lit_outline), StrokeKind::Inside);
        }
    } else {
        p.rect_filled(r, rad, t.silver.s2);
        p.rect_stroke(r, rad, Stroke::new(hair(), t.silver.s4), StrokeKind::Inside);
    }
    let c = r.center() + if pressed { vec2(1.0, 1.0) } else { Vec2::ZERO };
    let ink = if !enabled { disabled_ink } else if ink == t.text || ink == t.dialog().text { t.ink.button } else { ink };
    p.text(c, Align2::CENTER_CENTER, label, font, ink);
    resp
}

/// egui's check box with `check_box` painted over its box.
pub fn checkbox(ui: &mut Ui, checked: &mut bool, text: impl Into<WidgetText>) -> Response {
    checkbox_enabled(ui, true, checked, text)
}

pub fn checkbox_enabled(ui: &mut Ui, enabled: bool, checked: &mut bool, text: impl Into<WidgetText>) -> Response {
    let resp = ui.add_enabled(enabled, Checkbox::new(checked, text));
    let t = theme::get();
    let (_, big) = ui.spacing().icon_rectangles(resp.rect);
    check_box(ui.painter(), big, *checked, enabled && ui.is_enabled(), t.text, &skin(ui, &t));
    resp
}

/// egui's radio button with `radio_mark` painted over its circle.
pub fn radio(ui: &mut Ui, on: bool, text: impl Into<WidgetText>) -> Response {
    radio_enabled(ui, true, on, text)
}

pub fn radio_enabled(ui: &mut Ui, enabled: bool, on: bool, text: impl Into<WidgetText>) -> Response {
    let resp = ui.add_enabled(enabled, RadioButton::new(on, text));
    let t = theme::get();
    let (_, big) = ui.spacing().icon_rectangles(resp.rect);
    radio_mark(ui.painter(), big, on, enabled && ui.is_enabled(), t.text, &skin(ui, &t));
    resp
}

/// `Ui::radio_value` through `radio`.
pub fn radio_value<T: PartialEq>(ui: &mut Ui, current: &mut T, value: T, text: impl Into<WidgetText>) -> Response {
    let mut resp = radio(ui, *current == value, text);
    if resp.clicked() && *current != value {
        *current = value;
        resp.mark_changed();
    }
    resp
}

/// egui's combo box (`ComboBox::from_id_salt(id).width(width).selected_text(text).show_ui(ui, add)`) with
/// `combo_frame` painted over its button (the field paper, `paper.disabled` when disabled).
pub fn combo<R>(ui: &mut Ui, id: impl std::hash::Hash, width: f32, text: impl Into<WidgetText>, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<Option<R>> {
    let t = theme::get();
    let m = skin(ui, &t);
    let fill = if ui.is_enabled() { t.input_bg } else { t.ink.paper_disabled };
    let out = ui
        .scope(|ui| {
            flat_field_visuals(ui, fill);
            ComboBox::from_id_salt(id).width(width).selected_text(text).show_ui(ui, add)
        })
        .inner;
    combo_frame(ui.painter(), out.response.rect, out.inner.is_some(), t.text, &m);
    out
}

/// egui's widget visuals with a flat `fill` (rounded `r_ctl`) and no edge, under the painters.
fn flat_field_visuals(ui: &mut Ui, fill: Color32) {
    let t = theme::get();
    let w = &mut ui.visuals_mut().widgets;
    for s in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        s.bg_fill = fill;
        s.weak_bg_fill = fill;
        s.bg_stroke = Stroke::NONE;
        s.expansion = 0.0;
        s.corner_radius = CornerRadius::same(t.m.r_ctl);
    }
}

/// egui's drag value (`ui.add(build(DragValue::new(v)))`) drawn as a spinner: a recessed paper field with
/// `spin_buttons` at its right end that step the value by `step`.
pub fn spin<N: emath::Numeric>(ui: &mut Ui, v: &mut N, step: f64, build: impl for<'a> FnOnce(DragValue<'a>) -> DragValue<'a>) -> Response {
    let t = theme::get();
    let m = skin(ui, &t);
    let enabled = ui.is_enabled();
    let fill = if enabled { t.input_bg } else { t.ink.paper_disabled };
    let mut resp = ui
        .scope(|ui| {
            flat_field_visuals(ui, fill);
            // Room for the buttons at the right: the text is centred, so it needs the same on the left.
            ui.spacing_mut().button_padding.x = SPIN_W + 2.0;
            ui.add(build(DragValue::new(v)))
        })
        .inner;
    let r = px(resp.rect);
    let p = ui.painter().clone();
    recess_edge(&p, r, t.m.r_ctl as f32, &m);
    let br = Rect::from_min_max(pos2(r.max.x - 2.0 - SPIN_W, r.min.y + 2.0), r.max - vec2(2.0, 2.0));
    let mid = (br.min.y + br.height() * 0.5).floor();
    let up = ui.interact(Rect::from_min_max(br.min, pos2(br.max.x, mid)), resp.id.with("spin_up"), Sense::click());
    let down = ui.interact(Rect::from_min_max(pos2(br.min.x, mid), br.max), resp.id.with("spin_down"), Sense::click());
    let pressed = if up.is_pointer_button_down_on() { Some(0) } else if down.is_pointer_button_down_on() { Some(1) } else { None };
    spin_buttons(&p, br, pressed, t.text, &m);
    let delta = if up.clicked() { step } else if down.clicked() { -step } else { 0.0 };
    if delta != 0.0 && enabled {
        *v = N::from_f64(v.to_f64() + delta);
        resp.mark_changed();
    }
    resp
}

/// `spin` through `Ui::add_enabled`.
pub fn spin_enabled<N: emath::Numeric>(ui: &mut Ui, enabled: bool, v: &mut N, step: f64, build: impl for<'a> FnOnce(DragValue<'a>) -> DragValue<'a>) -> Response {
    ui.add_enabled_ui(enabled, |ui| spin(ui, v, step, build)).inner
}

/// How a `slider` is built (egui's `Slider` defaults).
#[derive(Clone, Copy, Default)]
pub struct Slide<'a> {
    /// Label after the value.
    pub text: &'a str,
    /// Hide the value field.
    pub no_value: bool,
    pub log: bool,
    /// `SliderClamping::Always`.
    pub clamp: bool,
}

/// A horizontal slider (UI_SPEC.md §5): the recessed track with its value fill and the chrome lozenge knob, then the
/// value as a spinner and the label.
pub fn slider(ui: &mut Ui, v: &mut f32, range: std::ops::RangeInclusive<f32>, s: Slide) -> Response {
    let t = theme::get();
    let m = skin(ui, &t);
    let (lo, hi) = (*range.start(), *range.end());
    if s.clamp {
        *v = v.clamp(lo, hi);
    }
    let log = s.log && lo > 0.0 && hi > lo;
    let out = ui.horizontal(|ui| {
        let thickness = ui.text_style_height(&TextStyle::Body).max(ui.spacing().interact_size.y);
        let (rect, mut resp) = ui.allocate_exact_size(vec2(ui.spacing().slider_width, thickness), Sense::click_and_drag());
        let track = Rangef::new(rect.min.x + 8.0, rect.max.x - 8.0);
        let norm = |x: f32| if log { (x / lo).ln() / (hi / lo).ln() } else { (x - lo) / (hi - lo).max(1e-9) };
        let denorm = |n: f32| if log { lo * (hi / lo).powf(n) } else { lo + n * (hi - lo) };
        if let Some(pos) = resp.interact_pointer_pos().filter(|_| resp.dragged() || resp.clicked()) {
            let nv = denorm(((pos.x - track.min) / track.span().max(1.0)).clamp(0.0, 1.0));
            if nv != *v {
                *v = nv;
                resp.mark_changed();
            }
        }
        let p = ui.painter();
        let x = track.min + norm(v.clamp(lo, hi)).clamp(0.0, 1.0) * track.span();
        groove_value(p, Rangef::new(track.min - 4.0, track.max + 4.0), rect.center().y, Some(x), &m);
        thumb(p, Rect::from_center_size(pos2(x.round(), rect.center().y), vec2(15.0, 11.0)), &m, None);
        if !s.no_value {
            let span = (hi - lo) as f64;
            let step = if span >= 100.0 { 1.0 } else { span / 100.0 };
            if spin(ui, v, step, |d| d.range(lo..=hi).speed(span / 200.0).max_decimals(3)).changed() {
                resp.mark_changed();
            }
        }
        if !s.text.is_empty() {
            ui.label(s.text);
        }
        resp
    });
    out.inner | out.response
}

/// A progress bar (`text` shown, else the percentage; `width` or the available width): `progress_blocks` with the
/// text in `sel.text` over it (AA on both the groove and the fill).
pub fn progress(ui: &mut Ui, frac: f32, text: Option<String>, width: Option<f32>) -> Response {
    let t = theme::get();
    let w = width.unwrap_or_else(|| ui.available_size_before_wrap().x.max(96.0));
    let (r, resp) = ui.allocate_exact_size(vec2(w, ui.spacing().interact_size.y), Sense::hover());
    let p = ui.painter();
    progress_blocks(p, r, frac, t.sel.mid, &skin(ui, &t));
    p.text(r.center(), Align2::CENTER_CENTER, text.unwrap_or_else(|| format!("{:.0} %", frac * 100.0)), t.font(t.font.ui), t.sel.text);
    resp
}

/// egui's text field (`ui.add(te)`) with the recess edge painted round it.
pub fn edit(ui: &mut Ui, te: TextEdit<'_>) -> Response {
    edit_enabled(ui, true, te)
}

pub fn edit_enabled(ui: &mut Ui, enabled: bool, te: TextEdit<'_>) -> Response {
    let t = theme::get();
    // Room for the edge round the text; egui's response rect is the inner one (inside the margin).
    let resp = ui.scope(|ui| {
        let w = &mut ui.visuals_mut().widgets;
        for s in [&mut w.inactive, &mut w.hovered, &mut w.active] {
            s.bg_stroke = Stroke::NONE;
        }
        ui.add_enabled(enabled, te.margin(Margin::symmetric(5, 3)))
    });
    let resp = resp.inner;
    let outer = resp.rect.expand2(vec2(5.0, 3.0));
    recess_edge(ui.painter(), outer, t.m.r_ctl as f32, &skin(ui, &t));
    if resp.has_focus() {
        ui.painter().rect_stroke(px(outer), t.m.r_ctl as f32, Stroke::new(hair(), t.sel.lit_outline), StrokeKind::Inside);
    }
    resp
}

/// A push button (`ui.button(text)`): `classic_push` sized to the label.
pub fn push_button(ui: &mut Ui, text: impl Into<WidgetText>) -> Response {
    push_button_enabled(ui, true, text)
}

pub fn push_button_enabled(ui: &mut Ui, enabled: bool, text: impl Into<WidgetText>) -> Response {
    let t = theme::get();
    let text = text.into();
    let label = text.text().to_owned();
    let w = ui.painter().layout_no_wrap(label.clone(), t.font(11.0), t.text).size().x + 16.0;
    let h = ui.spacing().interact_size.y.max(21.0);
    classic_push(ui, &label, vec2(w, h), enabled && ui.is_enabled(), false, t.text, t.text_disabled, &skin(ui, &t))
}

/// A small button (`ui.small_button(text)`, the × of a row): a 16×16 `classic_push`.
pub fn small_push(ui: &mut Ui, text: &str) -> Response {
    let t = theme::get();
    classic_push(ui, text, vec2(16.0, 16.0), ui.is_enabled(), false, t.text, t.text_disabled, &skin(ui, &t))
}

/// `small_push` through `Ui::add_enabled` (`Button::new(text).small()`).
pub fn small_push_enabled(ui: &mut Ui, enabled: bool, text: &str) -> Response {
    let t = theme::get();
    classic_push(ui, text, vec2(16.0, 16.0), enabled && ui.is_enabled(), false, t.text, t.text_disabled, &skin(ui, &t))
}

pub const ROW_H: f32 = 20.0;

/// `r` with its corners on whole pixels.
pub fn px(r: Rect) -> Rect {
    // R48: on physical pixel edges at any interface scale (whole points at 100 %).
    let s = |p: Pos2| pos2(theme::snap(p.x.round()), theme::snap(p.y.round()));
    Rect::from_min_max(s(r.min), s(r.max))
}

/// True inside a navy list well (`scroll_pane` sets its text colour).
pub fn in_well(ui: &Ui) -> bool {
    ui.visuals().override_text_color == Some(theme::get().lcd.text)
}

/// The text colour of a row on this `ui`: `sel.text` on the selection, `lcd.text` in a navy well, `ink` elsewhere.
pub fn list_ink(ui: &Ui, selected: bool) -> Color32 {
    let t = theme::get();
    if selected {
        t.sel.text
    } else if in_well(ui) {
        t.lcd.text
    } else {
        t.text
    }
}

/// One tree node. Returns (row response, expander clicked).
/// `expanded`: None = leaf. ⊞/⊟ boxes, dotted tree lines, and the selection bar on the label when selected.
pub fn tree_row(ui: &mut Ui, depth: usize, expanded: Option<bool>, icon: Option<Icon>, label: &str, selected: bool) -> (Response, bool) {
    let t = theme::get();
    let well = in_well(ui);
    let w = ui.available_width().max(60.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, ROW_H), Sense::click_and_drag());
    let p = ui.painter();
    if well && resp.hovered() && !selected {
        p.rect_filled(rect, 0.0, t.lcd.hover);
    }
    let x0 = rect.min.x + 4.0 + depth as f32 * 16.0;
    let cy = rect.center().y;
    let exp_rect = Rect::from_center_size(pos2(x0 + 5.0, cy), vec2(9.0, 9.0));
    // TreeView lines: from the node to its icon, from the parent's column to the node, and down that column from the
    // last connector at this depth (the parent's box, or the previous sibling's row) to this row's, so the line ends at
    // the last child. Rows come in order, each drawing the column above itself; the connectors are kept per list
    // (`ui.id()`) for this pass.
    let (ex, ey) = (exp_rect.center().x.floor(), cy.floor());
    let ink = if well { t.lcd.rule } else { t.metal.shadow };
    dotted(p, pos2(if expanded.is_some() { exp_rect.max.x.floor() } else { ex }, ey), pos2(x0 + 13.0, ey), ink);
    let key = ui.id().with("tree_lines");
    let pass = ui.ctx().cumulative_pass_nr();
    let mut st: (u64, Vec<f32>) = ui.data(|d| d.get_temp(key)).filter(|s: &(u64, Vec<f32>)| s.0 == pass).unwrap_or((pass, Vec::new()));
    if depth > 0 {
        let col = ex - 16.0;
        dotted(p, pos2(col, ey), pos2(if expanded.is_some() { exp_rect.min.x.floor() } else { ex }, ey), ink);
        if let Some(&from) = st.1.get(depth).filter(|y| y.is_finite()) {
            dotted(p, pos2(col, from), pos2(col, ey), ink);
        }
    }
    st.1.truncate(depth + 1);
    st.1.resize(depth + 1, f32::NAN);
    st.1[depth] = ey;
    if expanded == Some(true) {
        st.1.push(exp_rect.max.y.floor());
    }
    ui.data_mut(|d| d.insert_temp(key, st));
    let mut toggled = false;
    if let Some(open) = expanded {
        let er = ui.interact(exp_rect.expand(3.0), resp.id.with("exp"), Sense::click());
        toggled = er.clicked();
        let p = ui.painter();
        // ⊞ / ⊟: a 9×9 box with a + or −, on whole pixels: navy with `lcd.text` in a well, paper and ink elsewhere.
        let b = px(exp_rect);
        let (fill, edge, mark) = if well { (t.lcd.deep, t.lcd.text, t.lcd.text_hi) } else { (t.input_bg, t.metal.shadow, t.ink.glyph) };
        p.rect_filled(b, 1.0, fill);
        p.rect_stroke(b, 1.0, Stroke::new(1.0_f32, edge), StrokeKind::Inside);
        let c = b.center().floor();
        p.rect_filled(Rect::from_min_max(pos2(c.x - 2.0, c.y), pos2(c.x + 3.0, c.y + 1.0)), 0.0, mark);
        if !open {
            p.rect_filled(Rect::from_min_max(pos2(c.x, c.y - 2.0), pos2(c.x + 1.0, c.y + 3.0)), 0.0, mark);
        }
    }
    let mut x = x0 + 14.0;
    if let Some(icon) = icon {
        paint_icon(ui.painter(), Rect::from_min_size(pos2(x, cy - 8.0), vec2(16.0, 16.0)), icon, None);
        x += 20.0;
    }
    let p = ui.painter();
    let color = list_ink(ui, selected);
    let galley = p.layout_no_wrap(label.to_owned(), t.font(t.font.ui), color);
    let text_pos = pos2(x, (cy - galley.size().y * 0.5).round());
    if selected {
        // The selection bar covers the label only, as a TreeView's.
        let box_r = px(Rect::from_min_size(text_pos - vec2(3.0, 2.0), galley.size() + vec2(6.0, 4.0)));
        selection_bar(p, box_r, t.m.r_small as f32);
    }
    p.galley(text_pos, galley, color);
    (resp, toggled)
}

/// Full-width list row: the selection bar when selected; `lcd.hover` under the pointer in a navy well.
pub fn list_row(ui: &mut Ui, height: f32, selected: bool) -> Response {
    let t = theme::get();
    let w = ui.available_width().max(60.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, height), Sense::click_and_drag());
    if selected {
        selection_bar(ui.painter(), rect, 0.0);
    } else if in_well(ui) && resp.hovered() {
        ui.painter().rect_filled(rect, 0.0, t.lcd.hover);
    }
    resp
}

/// Column header row (20 px): labels with 1 px separators. `cols`: (title, width). The column clicked, if any.
pub fn header_row(ui: &mut Ui, cols: &[(&str, f32)]) -> Option<usize> {
    header_row_sorted(ui, cols, None)
}

/// `header_row` with the sorted column marked: a chrome bar of raised segments with `ink` labels, a ▲ / ▼ in
/// `ink.glyph` on `sort` = (column, ascending).
pub fn header_row_sorted(ui: &mut Ui, cols: &[(&str, f32)], sort: Option<(usize, bool)>) -> Option<usize> {
    let t = theme::get();
    let w = ui.available_width().max(cols.iter().map(|c| c.1).sum());
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 20.0), Sense::click());
    let p = ui.painter();
    let m = &t.metal;
    let mut x = rect.min.x;
    let ends: Vec<f32> = cols.iter().map(|c| c.1).chain([f32::INFINITY]).collect();
    for (k, cw) in ends.iter().enumerate() {
        let x1 = (x + cw).min(rect.max.x);
        if x1 <= x {
            break;
        }
        let r = px(Rect::from_min_max(pos2(x, rect.min.y), pos2(x1, rect.max.y)));
        grad(p, r, 0.0, &[(0.0, t.silver.s1), (0.45, t.silver.s2), (1.0, t.silver.s3)]);
        hl(p, r.min.x, r.max.x, r.min.y, m.light);
        hl(p, r.min.x, r.max.x, r.max.y - hair(), m.dark);
        if x1 < rect.max.x {
            etched_v(p, r.max.x - 2.0, Rangef::new(r.min.y + 3.0, r.max.y - 3.0), m);
        }
        if let Some((title, _)) = cols.get(k) {
            p.with_clip_rect(r.shrink(2.0).intersect(ui.clip_rect())).text(pos2(x + 6.0, rect.center().y), Align2::LEFT_CENTER, *title, t.font(t.font.ui), t.text);
            if let Some((_, asc)) = sort.filter(|s| s.0 == k) {
                let ar = Rect::from_center_size(pos2(x1 - 10.0, rect.center().y), vec2(8.0, 8.0));
                arrow(p, ar, (0.0, if asc { -1.0 } else { 1.0 }), t.ink.glyph);
            }
        }
        x = x1;
    }
    let px = resp.clicked().then(|| resp.interact_pointer_pos()).flatten()?.x - rect.min.x;
    cols.iter().scan(0.0, |end, c| {
        *end += c.1;
        Some(*end)
    }).position(|end| px < end)
}

/// Text cells of one row laid over `rect`, clipped per column.
pub fn row_cells(ui: &Ui, rect: Rect, cols: &[(&str, f32)], values: &[String], color: Color32) {
    let t = theme::get();
    let mut x = rect.min.x;
    for ((_, cw), v) in cols.iter().zip(values) {
        let cell = Rect::from_x_y_ranges(Rangef::new(x, x + cw - 4.0), rect.y_range());
        ui.painter().with_clip_rect(cell.intersect(ui.clip_rect())).text(
            pos2(x + 6.0, rect.center().y),
            Align2::LEFT_CENTER,
            v,
            t.font(t.font.ui),
            color,
        );
        x += cw;
    }
}

/// Grid lines of a details list (Edit Details, on the panel): an `s4` rule under the row and one between the columns.
pub fn grid_lines(p: &Painter, rect: Rect, cols: &[(&str, f32)]) {
    let c = theme::get().silver.s4;
    let y = rect.max.y.floor() - 1.0;
    p.rect_filled(Rect::from_x_y_ranges(rect.x_range(), Rangef::new(y, y + 1.0)), 0.0, c);
    let mut x = rect.min.x;
    for (_, cw) in cols {
        x += cw;
        p.rect_filled(Rect::from_x_y_ranges(Rangef::new(x.floor() - 1.0, x.floor()), rect.y_range()), 0.0, c);
    }
}

/// Vertical 5 px splitter at the right edge of a left pane `width` wide inside `area`; drag resizes the pane.
pub fn pane_split(ui: &mut Ui, area: Rect, width: &mut f32, id: &str) -> (Rect, Rect) {
    let t = theme::get();
    // The stored width survives a narrow frame (the first frame, a small dock); only drags change it.
    let w = width.clamp(60.0, (area.width() - 80.0).max(60.0));
    let left = Rect::from_min_max(area.min, pos2(area.min.x + w, area.max.y));
    let split = Rect::from_min_max(pos2(left.max.x, area.min.y), pos2(left.max.x + 5.0, area.max.y));
    let right = Rect::from_min_max(pos2(split.max.x, area.min.y), area.max);
    // Inside a panel the split is panel silver with the dimples (the slate is for the window's own splitters).
    let p = ui.painter();
    p.rect_filled(split, 0.0, t.panel_bg);
    let c = split.center().floor();
    for k in -1..=1 {
        dimple(p, pos2(c.x - 1.0, c.y + 5.0 * k as f32 - 1.0), &t.metal);
    }
    let resp = ui.interact(split, ui.id().with(("pane_split", id)), Sense::drag());
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
    }
    if resp.dragged() {
        *width = w + resp.drag_delta().x;
    }
    (left, right)
}

/// Child ui confined to `rect` with its own scroll area id: a navy list well (UI_SPEC.md §5 List / tree in a well)
/// with the contents inside its edge, their text in `lcd.text`.
pub fn scroll_pane(ui: &mut Ui, rect: Rect, id: &str, add: impl FnOnce(&mut Ui)) {
    let t = theme::get();
    lcd_well(ui.painter(), rect, false, false);
    let rect = rect.shrink(2.0);
    let mut child = ui.new_child(UiBuilder::new().id_salt(("pane", id)).max_rect(rect).layout(Layout::top_down(Align::Min)));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    let v = child.visuals_mut();
    v.override_text_color = Some(t.lcd.text);
    v.extreme_bg_color = t.lcd.deep;
    v.widgets.noninteractive.fg_stroke.color = t.lcd.text;
    v.widgets.inactive.fg_stroke.color = t.lcd.text;
    v.widgets.hovered.fg_stroke.color = t.lcd.text_hi;
    v.widgets.noninteractive.bg_stroke.color = t.lcd.rule;
    // egui's scroll bars in the well: a silver handle on the deep navy rail.
    v.widgets.inactive.bg_fill = t.silver.s4;
    v.widgets.hovered.bg_fill = t.silver.s3;
    v.widgets.active.bg_fill = t.silver.s2;
    ScrollArea::both().id_salt(id).auto_shrink([false, false]).show(&mut child, |ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        add(ui)
    });
}

/// Search field: a recessed paper field with a placeholder.
pub fn search_field(ui: &mut Ui, text: &mut String, hint: &str, width: f32) -> Response {
    let t = theme::get();
    let h = (t.m.button - 2.0).max(18.0);
    let resp = ui.scope(|ui| {
        let w = &mut ui.visuals_mut().widgets;
        for s in [&mut w.inactive, &mut w.hovered, &mut w.active] {
            s.bg_stroke = Stroke::NONE;
            s.corner_radius = CornerRadius::same(t.m.r_ctl);
        }
        ui.add_sized(
            vec2(width, h),
            TextEdit::singleline(text)
                .hint_text(RichText::new(hint).color(t.input_placeholder))
                .background_color(t.input_bg)
                .text_color(t.text)
                .font(t.font(t.font.ui))
                .margin(vec2(4.0, 2.0)),
        )
    });
    let resp = resp.inner;
    // egui's response rect is the inner one (inside the 4 / 2 px margin).
    let outer = resp.rect.expand2(vec2(4.0, 2.0));
    recess_edge(ui.painter(), outer, t.m.r_ctl as f32, &t.metal);
    if resp.has_focus() {
        ui.painter().rect_stroke(px(outer), t.m.r_ctl as f32, Stroke::new(hair(), t.sel.lit_outline), StrokeKind::Inside);
    }
    resp
}

/// Thumbnail tile (UI_SPEC.md §5 preset tiles): raised chrome with `r_frame` corners (lifted when hovered, lit with a
/// 2 px `lit.outline` ring when selected), the picture area `thumb_rect` above, the caption in `ink` centred below,
/// up to two lines.
pub fn tile(ui: &Ui, rect: Rect, thumb: Rect, label: &str, selected: bool, hovered: bool) {
    let t = theme::get();
    let p = ui.painter();
    let rad = t.m.r_frame as f32;
    let state = if selected { Chrome::Active } else if hovered { Chrome::Hover } else { Chrome::Raised };
    chrome(p, rect, rad, state);
    if selected {
        p.rect_stroke(px(rect), rad, Stroke::new(2.0_f32, t.sel.lit_outline), StrokeKind::Inside);
    }
    let color = t.text;
    let cap = Rect::from_min_max(pos2(rect.min.x + 3.0, thumb.max.y + 3.0), pos2(rect.max.x - 3.0, rect.max.y));
    let mut job = text::LayoutJob::simple(label.to_owned(), t.font(t.font.ui), color, cap.width());
    job.halign = Align::Center;
    job.wrap.max_rows = 2;
    job.wrap.break_anywhere = true;
    let galley = p.layout_job(job);
    p.with_clip_rect(cap.intersect(ui.clip_rect())).galley(pos2(cap.center().x, cap.min.y), galley, color);
}

/// The black picture well a thumbnail sits in (UI_SPEC.md §1, neutral for pictures): black with a 1 px `s5` rim
/// outside the picture, so a title or a generator with transparent areas reads against black.
pub fn thumb_well(p: &Painter, r: Rect) {
    let t = theme::get();
    let r = px(r);
    p.rect_filled(r, 0.0, Color32::BLACK);
    p.rect_stroke(r.expand(1.0), 1.0, Stroke::new(1.0_f32, t.silver.s5), StrokeKind::Inside);
}

/// Letterboxed image inside `r` with a 1 px dark frame and a grey mat.
pub fn paint_thumb(p: &Painter, r: Rect, tex: &TextureHandle) {
    let [w, h] = tex.size();
    let aspect = w as f32 / h.max(1) as f32;
    let (iw, ih) = if r.width() / r.height() > aspect { (r.height() * aspect, r.height()) } else { (r.width(), r.width() / aspect) };
    let ir = px(Rect::from_center_size(r.center(), vec2(iw, ih)));
    p.image(tex.id(), ir, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    // 1 px dark frame and a thin neutral mat round it.
    p.rect_stroke(ir, 0.0, Stroke::new(1.0_f32, Color32::from_rgb(0x20, 0x20, 0x20)), StrokeKind::Outside);
    p.rect_stroke(ir.expand(1.0), 0.0, Stroke::new(1.0_f32, Color32::from_rgb(0xA0, 0xA0, 0xA0)), StrokeKind::Outside);
}

/// Audio file stand-in: a white page with a blue circular ♪ mark (§5.4).
pub fn paint_audio_page(p: &Painter, r: Rect) {
    let page = px(Rect::from_center_size(r.center(), vec2(r.height() * 0.62, r.height() * 0.86)));
    p.rect_filled(page, 0.0, Color32::WHITE);
    let c = page.center() + vec2(0.0, 2.0);
    let blue = Color32::from_rgb(0x2E, 0x7C, 0xD6);
    p.circle_filled(c, page.width() * 0.32, blue);
    // ♪: head and stem with a small flag, in white.
    let s = page.width() * 0.08;
    p.circle_filled(c + vec2(-1.5 * s, 2.0 * s), 1.6 * s, Color32::WHITE);
    p.line_segment([c + vec2(-0.1 * s, 2.0 * s), c + vec2(-0.1 * s, -3.5 * s)], Stroke::new(s.max(1.0), Color32::WHITE));
    p.line_segment([c + vec2(-0.1 * s, -3.5 * s), c + vec2(2.0 * s, -2.0 * s)], Stroke::new(s.max(1.0), Color32::WHITE));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The gradient sampler hits its stops and blends between them.
    #[test]
    fn gradient_stops() {
        let (a, b) = (Color32::from_rgb(0, 0, 0), Color32::from_rgb(200, 100, 50));
        let stops = [(0.0, a), (1.0, b)];
        assert_eq!(sample(&stops, 0.0), a);
        assert_eq!(sample(&stops, 1.0), b);
        assert_eq!(sample(&stops, 0.5), Color32::from_rgb(100, 50, 25));
        assert_eq!(sample(&stops, -1.0), a);
    }

    /// Every timecode character paints as segments; widths add up.
    #[test]
    fn lcd_digits_cover_timecodes() {
        assert!(lcd_paintable("00:00:05;12") && lcd_paintable("1.1.000") && lcd_paintable("-12.5"));
        assert!(!lcd_paintable("1920x1080"));
        assert!((lcd_width("00", 20.0) - 2.0 * lcd_advance('0', 20.0)).abs() < 1e-4);
    }
}
