//! Small painted widgets shared by the dock windows: tree rows, list rows and headers, the 5 px pane splitter,
//! search fields and preview tiles; and the brushed-metal / 3D-bevel painters every bar uses (UI_SPEC.md §2, §4).
use eframe::egui::*;
use super::icons::{Icon, paint_icon};
use super::theme::{self, Metal, Theme};

// ---- Metal and bevels ----

/// Side of the brushed streak texture; it repeats over every bar.
const STREAK: usize = 64;

/// The brushed-steel streak texture, made once per context (a 64×64 tile of lighter and darker horizontal streaks,
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
    // roughened only slightly along it; at most ~7 % lighter or darker than the gradient.
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

/// Brushed-metal fill: vertical gradient `hi` → `mid` (45 %) → `lo`, multiplied by `tint`, with the streak texture
/// over it. Two triangles' worth of mesh and one textured quad.
pub fn metal_tinted(p: &Painter, r: Rect, m: &Metal, tint: Color32) {
    let r = px(r);
    if !r.is_positive() {
        return;
    }
    let mul = |c: Color32| {
        let f = |a: u8, b: u8| ((a as u32 * b as u32 + 127) / 255) as u8;
        Color32::from_rgb(f(c.r(), tint.r()), f(c.g(), tint.g()), f(c.b(), tint.b()))
    };
    let mut mesh = Mesh::default();
    let mid_y = (r.min.y + r.height() * 0.45).round();
    for (y, c) in [(r.min.y, mul(m.hi)), (mid_y, mul(m.mid)), (r.max.y, mul(m.lo))] {
        mesh.colored_vertex(pos2(r.min.x, y), c);
        mesh.colored_vertex(pos2(r.max.x, y), c);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    mesh.add_triangle(2, 3, 4);
    mesh.add_triangle(3, 5, 4);
    p.add(mesh);
    if m.streak > 0.0 {
        let uv = Rect::from_min_max(Pos2::ZERO, pos2(r.width() / STREAK as f32, r.height() / STREAK as f32));
        p.image(streak_tex(p.ctx()), r, uv, Color32::WHITE.gamma_multiply(m.streak));
    }
}

pub fn metal(p: &Painter, r: Rect, m: &Metal) {
    metal_tinted(p, r, m, Color32::WHITE);
}

/// A metal band with its light top row and dark bottom row (toolbars, transport, status bar, ruler band).
pub fn metal_bar(p: &Painter, r: Rect, m: &Metal) {
    let r = px(r);
    metal(p, r, m);
    p.rect_filled(Rect::from_min_max(r.min, pos2(r.max.x, r.min.y + 1.0)), 0.0, m.light);
    p.rect_filled(Rect::from_min_max(pos2(r.min.x, r.max.y - 1.0), r.max), 0.0, m.shadow);
}

/// Classic 2 px 3D edge inside `r`: raised = light/face on the top-left, shadow/dark on the bottom-right; sunken
/// swaps them (Windows 2000 button / field edges).
pub fn bevel(p: &Painter, r: Rect, raised: bool, m: &Metal) {
    let r = px(r);
    let (tl1, tl2, br1, br2) = if raised { (m.light, m.face, m.dark, m.shadow) } else { (m.shadow, m.dark, m.light, m.face) };
    let line = |a: Pos2, b: Pos2, c: Color32| p.rect_filled(Rect::from_two_pos(a, b), 0.0, c);
    // Outer ring.
    line(r.min, pos2(r.max.x - 1.0, r.min.y + 1.0), tl1);
    line(r.min, pos2(r.min.x + 1.0, r.max.y - 1.0), tl1);
    line(pos2(r.min.x, r.max.y - 1.0), r.max, br1);
    line(pos2(r.max.x - 1.0, r.min.y), r.max, br1);
    // Inner ring.
    let i = r.shrink(1.0);
    if i.width() > 2.0 && i.height() > 2.0 {
        line(i.min, pos2(i.max.x - 1.0, i.min.y + 1.0), tl2);
        line(i.min, pos2(i.min.x + 1.0, i.max.y - 1.0), tl2);
        line(pos2(i.min.x, i.max.y - 1.0), i.max, br2);
        line(pos2(i.max.x - 1.0, i.min.y), i.max, br2);
    }
}

/// Thin 1 px 3D edge (light top-left, shadow bottom-right; swapped when sunken): tabs, grooves, panes.
pub fn edge(p: &Painter, r: Rect, raised: bool, m: &Metal) {
    let r = px(r);
    let (tl, br) = if raised { (m.light, m.shadow) } else { (m.shadow, m.light) };
    p.rect_filled(Rect::from_min_max(r.min, pos2(r.max.x - 1.0, r.min.y + 1.0)), 0.0, tl);
    p.rect_filled(Rect::from_min_max(r.min, pos2(r.min.x + 1.0, r.max.y - 1.0)), 0.0, tl);
    p.rect_filled(Rect::from_min_max(pos2(r.min.x, r.max.y - 1.0), r.max), 0.0, br);
    p.rect_filled(Rect::from_min_max(pos2(r.max.x - 1.0, r.min.y), r.max), 0.0, br);
}

/// How a chrome button is drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Chrome {
    /// Raised metal with a specular top line.
    Raised,
    /// Raised and brighter.
    Hover,
    /// Pressed in: sunken, darker face.
    Pressed,
    /// A toggle that is on: sunken with the electric-blue tint.
    Active,
}

/// Bevelled chrome-style button over `r`: metal face, 2 px 3D edge, specular highlight under the top edge.
pub fn chrome_button(p: &Painter, r: Rect, state: Chrome, m: &Metal) {
    let r = px(r);
    let tint = match state {
        Chrome::Raised => Color32::WHITE,
        Chrome::Hover => Color32::from_rgb(0xFF, 0xFF, 0xFF),
        Chrome::Pressed | Chrome::Active => Color32::from_rgb(0xC8, 0xCC, 0xD2),
    };
    metal_tinted(p, r, m, tint);
    if state == Chrome::Hover {
        p.rect_filled(r, 0.0, Color32::from_white_alpha(40));
    }
    let raised = matches!(state, Chrome::Raised | Chrome::Hover);
    bevel(p, r, raised, m);
    if raised && r.height() > 6.0 {
        p.rect_filled(Rect::from_min_max(pos2(r.min.x + 2.0, r.min.y + 2.0), pos2(r.max.x - 2.0, r.min.y + 3.0)), 0.0, m.specular);
    }
    if state == Chrome::Active {
        p.rect_filled(r.shrink(2.0), 0.0, m.pressed);
    }
}

/// Sunken pane (status panes, list boxes, the time display well): 3D edge in, `fill` inside.
pub fn sunken(p: &Painter, r: Rect, fill: Color32, m: &Metal) {
    let r = px(r);
    p.rect_filled(r, 0.0, fill);
    bevel(p, r, false, m);
}

/// Etched separator: a shadow line with a light line beside it (vertical over `y` at `x`).
pub fn etched_v(p: &Painter, x: f32, y: Rangef, m: &Metal) {
    let x = x.floor();
    p.rect_filled(Rect::from_x_y_ranges(Rangef::new(x, x + 1.0), y), 0.0, m.shadow);
    p.rect_filled(Rect::from_x_y_ranges(Rangef::new(x + 1.0, x + 2.0), y), 0.0, m.light);
}

pub fn etched_h(p: &Painter, x: Rangef, y: f32, m: &Metal) {
    let y = y.floor();
    p.rect_filled(Rect::from_x_y_ranges(x, Rangef::new(y, y + 1.0)), 0.0, m.shadow);
    p.rect_filled(Rect::from_x_y_ranges(x, Rangef::new(y + 1.0, y + 2.0)), 0.0, m.light);
}

/// Toolbar grip: two raised 2 px bars (light + shadow) down the bar's left end, as Office 2000 / Vegas 6 toolbars.
pub fn grip(p: &Painter, r: Rect, m: &Metal) {
    let y = Rangef::new(r.min.y + 3.0, r.max.y - 3.0);
    for k in 0..2 {
        let x = r.min.x.floor() + 2.0 + 3.0 * k as f32;
        p.rect_filled(Rect::from_x_y_ranges(Rangef::new(x, x + 1.0), y), 0.0, m.light);
        p.rect_filled(Rect::from_x_y_ranges(Rangef::new(x + 1.0, x + 2.0), y), 0.0, m.shadow);
    }
}

/// Splitter between bands or panes: metal with a dotted centre line (Vegas 6), `across` = a horizontal bar.
pub fn dotted_splitter(p: &Painter, r: Rect, across: bool, m: &Metal) {
    let r = px(r);
    metal(p, r, m);
    let c = r.center().floor();
    let mut mesh = Mesh::default();
    if across {
        let mut x = r.min.x + 2.0;
        while x < r.max.x - 1.0 {
            mesh.add_colored_rect(Rect::from_min_size(pos2(x, c.y), vec2(1.0, 1.0)), m.dark);
            mesh.add_colored_rect(Rect::from_min_size(pos2(x + 1.0, c.y + 1.0), vec2(1.0, 1.0)), m.light);
            x += 3.0;
        }
    } else {
        let mut y = r.min.y + 2.0;
        while y < r.max.y - 1.0 {
            mesh.add_colored_rect(Rect::from_min_size(pos2(c.x, y), vec2(1.0, 1.0)), m.dark);
            mesh.add_colored_rect(Rect::from_min_size(pos2(c.x + 1.0, y + 1.0), vec2(1.0, 1.0)), m.light);
            y += 3.0;
        }
    }
    p.add(mesh);
}

/// Sunken 3 px slider groove over `x` centred on `cy`.
pub fn groove(p: &Painter, x: Rangef, cy: f32, m: &Metal) {
    let cy = cy.floor();
    p.rect_filled(Rect::from_x_y_ranges(x, Rangef::new(cy - 1.0, cy)), 0.0, m.shadow);
    p.rect_filled(Rect::from_x_y_ranges(x, Rangef::new(cy, cy + 1.0)), 0.0, m.dark);
    p.rect_filled(Rect::from_x_y_ranges(x, Rangef::new(cy + 1.0, cy + 2.0)), 0.0, m.light);
}

/// Metallic slider thumb: raised chrome block with a dark centre line (Vegas 5–7 fader handle).
pub fn thumb(p: &Painter, r: Rect, m: &Metal, tint: Option<Color32>) {
    let r = px(r);
    chrome_button(p, r, Chrome::Raised, m);
    let c = r.center().floor();
    p.rect_filled(Rect::from_min_max(pos2(c.x, r.min.y + 3.0), pos2(c.x + 1.0, r.max.y - 3.0)), 0.0, tint.unwrap_or(m.dark));
    if let Some(t) = tint {
        p.rect_filled(r.shrink(2.0), 0.0, t.gamma_multiply(0.25));
    }
}

/// Sunken 3 px vertical slider groove down `y` centred on `cx` (fader tracks).
pub fn groove_v(p: &Painter, cx: f32, y: Rangef, m: &Metal) {
    let cx = cx.floor();
    p.rect_filled(Rect::from_x_y_ranges(Rangef::new(cx - 1.0, cx), y), 0.0, m.shadow);
    p.rect_filled(Rect::from_x_y_ranges(Rangef::new(cx, cx + 1.0), y), 0.0, m.dark);
    p.rect_filled(Rect::from_x_y_ranges(Rangef::new(cx + 1.0, cx + 2.0), y), 0.0, m.light);
}

/// Thumb of a vertical fader: `thumb` with the centre line across it (the Vegas 5–7 fader handle, dark bar
/// across its middle).
pub fn fader_thumb(p: &Painter, r: Rect, m: &Metal, tint: Option<Color32>) {
    let r = px(r);
    chrome_button(p, r, Chrome::Raised, m);
    let c = r.center().floor();
    p.rect_filled(Rect::from_min_max(pos2(r.min.x + 3.0, c.y), pos2(r.max.x - 3.0, c.y + 1.0)), 0.0, tint.unwrap_or(m.dark));
    if let Some(t) = tint {
        p.rect_filled(r.shrink(2.0), 0.0, t.gamma_multiply(0.25));
    }
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

/// Classic LED meter: 2 px segments on a 3 px pitch from the well's bottom up to `y`, green below −12 dB, yellow to
/// −3 dB, red above (`to_y` maps dB to y). The well's dark fill shows between the segments.
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

/// Sunken meter well (3D edge round the dark well).
pub fn meter_well(p: &Painter, well: Rect, t: &Theme) {
    sunken(p, well.expand(2.0), t.meter_bg, &t.metal);
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
    let handle = ctx.load_texture(format!("milan_brand_{which:?}"), image, TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, handle.clone()));
    handle
}

/// Black triangle of a classic scrollbar / spinner arrow in `r`; `dir` = (dx, dy) it points to.
pub fn arrow(p: &Painter, r: Rect, dir: (f32, f32), ink: Color32) {
    let c = r.center().floor() + vec2(0.5, 0.5);
    let (dx, dy) = dir;
    let (nx, ny) = (-dy, dx);
    let tip = c + vec2(dx, dy) * 2.5;
    let base = c - vec2(dx, dy) * 1.5;
    let pts = vec![tip, base + vec2(nx, ny) * 4.0, base - vec2(nx, ny) * 4.0];
    p.add(Shape::convex_polygon(pts, ink, Stroke::NONE));
}

/// Metal header with a blue-grey tint (track headers).
pub fn header_metal(p: &Painter, r: Rect, t: &Theme, selected: bool) {
    let tint = if selected { Color32::from_rgb(0xBE, 0xDC, 0xF2) } else { t.metal.header_tint };
    metal_tinted(p, r, &t.metal, tint);
}

// ---- Windows 2000 skins over egui's controls (UI_SPEC.md §8.6) ----
//
// egui draws its check boxes, radio buttons, combo boxes, drag values, sliders, progress bars, text fields and push
// buttons flat. The wrappers below let egui lay out and run the control (clicks, drags, keyboard, the combo list) and
// paint the Windows 2000 control over it through one set of painters: `check_box`, `radio_mark`, `combo_frame`,
// `spin_buttons`, `progress_blocks`, `classic_push` and the sunken `bevel` of a field.

/// The 3D palette the controls on this `ui` are drawn with: the `#D4D0C8` property-sheet one inside a dialog
/// (`dialogs::style` fills the panel with the dialog page colour), the metal one in the dock windows.
pub fn skin(ui: &Ui, t: &Theme) -> Metal {
    if ui.visuals().panel_fill == t.dialog().page { t.dialog_metal() } else { t.metal }
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

/// Windows-classic check box in `r` (14×14): a sunken 2 px edge round a white box (face-grey when disabled) and a
/// 7×7 pixel check mark in `ink`.
pub fn check_box(p: &Painter, r: Rect, checked: bool, enabled: bool, ink: Color32, m: &Metal) {
    let r = px(r);
    p.rect_filled(r, 0.0, if enabled { Color32::WHITE } else { m.face });
    bevel(p, r, false, m);
    if checked {
        // Rows of the check from the top: (first x, one past the last x), two runs on the rows the strokes cross.
        const ROWS: [&[(f32, f32)]; 7] = [&[(6.0, 7.0)], &[(5.0, 7.0)], &[(0.0, 1.0), (4.0, 7.0)], &[(0.0, 2.0), (3.0, 6.0)], &[(0.0, 5.0)], &[(1.0, 4.0)], &[(2.0, 3.0)]];
        let o = (r.min + vec2((r.width() - 7.0) * 0.5, (r.height() - 7.0) * 0.5)).floor();
        let mut mesh = Mesh::default();
        for (y, runs) in ROWS.iter().enumerate() {
            for (x0, x1) in runs.iter() {
                mesh.add_colored_rect(Rect::from_min_max(pos2(o.x + x0, o.y + y as f32), pos2(o.x + x1, o.y + y as f32 + 1.0)), ink);
            }
        }
        p.add(mesh);
    }
}

/// Windows-classic radio button in `r`: a white disc (face-grey when disabled) with a 3D ring (shadow / dark on the
/// upper left, light / face on the lower right) and a 4 px dot in `ink` when on.
pub fn radio_mark(p: &Painter, r: Rect, on: bool, enabled: bool, ink: Color32, m: &Metal) {
    use std::f32::consts::PI;
    let c = r.center().floor() + vec2(0.5, 0.5);
    let rad = (r.width().min(r.height()) * 0.5 - 1.0).floor();
    p.circle_filled(c, rad, if enabled { Color32::WHITE } else { m.face });
    let arc = |radius: f32, a0: f32, a1: f32, col: Color32| {
        let n = 14;
        let pts: Vec<Pos2> = (0..=n).map(|i| c + Vec2::angled(a0 + (a1 - a0) * i as f32 / n as f32) * radius).collect();
        p.add(Shape::line(pts, Stroke::new(1.0_f32, col)));
    };
    arc(rad, 0.75 * PI, 1.75 * PI, m.shadow);
    arc(rad - 1.0, 0.75 * PI, 1.75 * PI, m.dark);
    arc(rad, -0.25 * PI, 0.75 * PI, m.light);
    arc(rad - 1.0, -0.25 * PI, 0.75 * PI, m.face);
    if on {
        p.circle_filled(c, 2.0, ink);
    }
}

/// Width of a combo box's arrow button.
const COMBO_ARROW_W: f32 = 20.0;

/// Windows-classic combo box over `r`: a sunken 2 px edge round the field and a raised arrow button at its right end
/// holding a black ▾, pressed in while the list is open. The field itself is egui's (white; face-grey disabled).
pub fn combo_frame(p: &Painter, r: Rect, open: bool, ink: Color32, m: &Metal) {
    let r = px(r);
    bevel(p, r, false, m);
    let b = Rect::from_min_max(pos2(r.max.x - 2.0 - COMBO_ARROW_W, r.min.y + 2.0), r.max - vec2(2.0, 2.0));
    p.rect_filled(b, 0.0, m.face);
    if open {
        p.rect_stroke(b, 0.0, Stroke::new(1.0_f32, m.shadow), StrokeKind::Inside);
    } else {
        bevel(p, b, true, m);
    }
    let shift = if open { vec2(1.0, 1.0) } else { Vec2::ZERO };
    tri(p, b.translate(shift), (0.0, 1.0), 3.5, 4.0, ink);
}

/// Width of a spinner's up / down buttons.
const SPIN_W: f32 = 13.0;

/// The up / down buttons of a Windows-classic spinner in `r` (at the right end of its field): two raised faces with
/// small black triangles; `pressed` = 0 (up) / 1 (down) sinks that one.
pub fn spin_buttons(p: &Painter, r: Rect, pressed: Option<usize>, ink: Color32, m: &Metal) {
    let r = px(r);
    let mid = (r.min.y + r.height() * 0.5).floor();
    let halves = [Rect::from_min_max(r.min, pos2(r.max.x, mid)), Rect::from_min_max(pos2(r.min.x, mid), r.max)];
    for (k, b) in halves.into_iter().enumerate() {
        p.rect_filled(b, 0.0, m.face);
        let down = pressed == Some(k);
        if down {
            p.rect_stroke(b, 0.0, Stroke::new(1.0_f32, m.shadow), StrokeKind::Inside);
        } else {
            bevel(p, b, true, m);
        }
        let shift = if down { vec2(1.0, 1.0) } else { Vec2::ZERO };
        tri(p, b.translate(shift), (0.0, if k == 0 { -1.0 } else { 1.0 }), 2.5, 3.0, ink);
    }
}

/// Windows 2000 progress bar in `r`: a sunken face-grey well with `accent` blocks (two thirds as wide as they are
/// tall, 2 px apart) for `frac` of its length.
pub fn progress_blocks(p: &Painter, r: Rect, frac: f32, accent: Color32, m: &Metal) {
    let r = px(r);
    p.rect_filled(r, 0.0, m.face);
    edge(p, r, false, m);
    let inner = r.shrink(3.0);
    let bw = (inner.height() * 2.0 / 3.0).round().max(2.0);
    let n = ((inner.width() + 2.0) / (bw + 2.0)).floor();
    let lit = (n * frac.clamp(0.0, 1.0)).round() as usize;
    let mut mesh = Mesh::default();
    for k in 0..lit {
        let x = inner.min.x + k as f32 * (bw + 2.0);
        mesh.add_colored_rect(Rect::from_min_max(pos2(x, inner.min.y), pos2((x + bw).min(inner.max.x), inner.max.y)), accent);
    }
    p.add(mesh);
}

/// Windows 2000 push button of `size`: `m.face` with the 2 px 3D edge (sunken while pressed, the label shifted),
/// the default button's black ring outside it, the label in Tahoma (`ink`; `disabled_ink` with a white emboss).
#[allow(clippy::too_many_arguments)]
pub fn classic_push(ui: &mut Ui, label: &str, size: Vec2, enabled: bool, default: bool, ink: Color32, disabled_ink: Color32, m: &Metal) -> Response {
    let t = theme::get();
    // Never narrower than its label (egui's `min_size` buttons grow the same way).
    let size = vec2(size.x.max(ui.painter().layout_no_wrap(label.to_owned(), t.font(11.0), ink).size().x + 16.0), size.y);
    let (r, resp) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    let r = px(r);
    let p = ui.painter();
    p.rect_filled(r, 0.0, m.face);
    let face = if default && enabled {
        p.rect_stroke(r, 0.0, Stroke::new(1.0_f32, Color32::BLACK), StrokeKind::Inside);
        r.shrink(1.0)
    } else {
        r
    };
    let pressed = enabled && resp.is_pointer_button_down_on();
    if pressed {
        p.rect_stroke(face, 0.0, Stroke::new(1.0_f32, m.shadow), StrokeKind::Inside);
    } else {
        bevel(p, face, true, m);
    }
    let c = face.center() + if pressed { vec2(1.0, 1.0) } else { Vec2::ZERO };
    let font = t.font(11.0);
    if enabled {
        p.text(c, Align2::CENTER_CENTER, label, font, ink);
    } else {
        p.text(c + vec2(1.0, 1.0), Align2::CENTER_CENTER, label, font.clone(), m.light);
        p.text(c, Align2::CENTER_CENTER, label, font, disabled_ink);
    }
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
/// `combo_frame` painted over its button (the field white, face-grey when disabled).
pub fn combo<R>(ui: &mut Ui, id: impl std::hash::Hash, width: f32, text: impl Into<WidgetText>, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<Option<R>> {
    let t = theme::get();
    let m = skin(ui, &t);
    let fill = if ui.is_enabled() { t.input_bg } else { m.face };
    let out = ui
        .scope(|ui| {
            flat_field_visuals(ui, fill);
            ComboBox::from_id_salt(id).width(width).selected_text(text).show_ui(ui, add)
        })
        .inner;
    combo_frame(ui.painter(), out.response.rect, out.inner.is_some(), t.text, &m);
    out
}

/// egui's widget visuals with a flat `fill` and no edge, under the classic painters.
fn flat_field_visuals(ui: &mut Ui, fill: Color32) {
    let w = &mut ui.visuals_mut().widgets;
    for s in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        s.bg_fill = fill;
        s.weak_bg_fill = fill;
        s.bg_stroke = Stroke::NONE;
        s.expansion = 0.0;
    }
}

/// egui's drag value (`ui.add(build(DragValue::new(v)))`) drawn as a spinner: a sunken white field
/// with `spin_buttons` at its right end that step the value by `step`.
pub fn spin<N: emath::Numeric>(ui: &mut Ui, v: &mut N, step: f64, build: impl for<'a> FnOnce(DragValue<'a>) -> DragValue<'a>) -> Response {
    let t = theme::get();
    let m = skin(ui, &t);
    let enabled = ui.is_enabled();
    let fill = if enabled { t.input_bg } else { m.face };
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
    bevel(&p, r, false, &m);
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

/// A horizontal slider drawn as the Vegas fader: a sunken `groove` with the chrome `thumb`, then
/// the value as a spinner and the label.
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
        let track = Rangef::new(rect.min.x + 6.0, rect.max.x - 6.0);
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
        groove(p, track, rect.center().y, &m);
        let x = track.min + norm(v.clamp(lo, hi)).clamp(0.0, 1.0) * track.span();
        thumb(p, Rect::from_center_size(pos2(x.round(), rect.center().y), vec2(11.0, 15.0)), &m, None);
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

/// A progress bar (`text` shown, else the percentage; `width` or the available width): `progress_blocks` with the text in black over them.
pub fn progress(ui: &mut Ui, frac: f32, text: Option<String>, width: Option<f32>) -> Response {
    let t = theme::get();
    let w = width.unwrap_or_else(|| ui.available_size_before_wrap().x.max(96.0));
    let (r, resp) = ui.allocate_exact_size(vec2(w, ui.spacing().interact_size.y), Sense::hover());
    let p = ui.painter();
    progress_blocks(p, r, frac, t.dialog().accent, &skin(ui, &t));
    p.text(r.center(), Align2::CENTER_CENTER, text.unwrap_or_else(|| format!("{:.0} %", frac * 100.0)), t.font(t.font.ui), t.text);
    resp
}

/// egui's text field (`ui.add(te)`) with the sunken 2 px edge painted round it.
pub fn edit(ui: &mut Ui, te: TextEdit<'_>) -> Response {
    edit_enabled(ui, true, te)
}

pub fn edit_enabled(ui: &mut Ui, enabled: bool, te: TextEdit<'_>) -> Response {
    let t = theme::get();
    // Room for the 2 px edge round the text; egui's response rect is the inner one (inside the margin).
    let resp = ui.add_enabled(enabled, te.margin(Margin::symmetric(5, 3)));
    bevel(ui.painter(), resp.rect.expand2(vec2(5.0, 3.0)), false, &skin(ui, &t));
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
    Rect::from_min_max(r.min.round(), r.max.round())
}

/// One tree node. Returns (row response, expander clicked).
/// `expanded`: None = leaf. ⊞/⊟ boxes, dotted tree lines, and the highlight bar on the label when selected.
pub fn tree_row(ui: &mut Ui, depth: usize, expanded: Option<bool>, icon: Option<Icon>, label: &str, selected: bool) -> (Response, bool) {
    let t = theme::get();
    let w = ui.available_width().max(60.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, ROW_H), Sense::click_and_drag());
    let p = ui.painter();
    let x0 = rect.min.x + 4.0 + depth as f32 * 16.0;
    let cy = rect.center().y;
    let exp_rect = Rect::from_center_size(pos2(x0 + 5.0, cy), vec2(9.0, 9.0));
    // TreeView lines: from the node to its icon, from the parent's column to the node, and down that column from the
    // last connector at this depth (the parent's box, or the previous sibling's row) to this row's, so the line ends at
    // the last child. Rows come in order, each drawing the column above itself; the connectors are kept per list
    // (`ui.id()`) for this pass.
    let (ex, ey) = (exp_rect.center().x.floor(), cy.floor());
    let ink = t.metal.shadow;
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
        // ⊞ / ⊟: a white 9×9 box with a grey edge and a black + or −, on whole pixels.
        let b = px(exp_rect);
        p.rect_filled(b, 0.0, t.input_bg);
        p.rect_stroke(b, 0.0, Stroke::new(1.0_f32, t.metal.shadow), StrokeKind::Inside);
        let c = b.center().floor();
        p.rect_filled(Rect::from_min_max(pos2(c.x - 2.0, c.y), pos2(c.x + 3.0, c.y + 1.0)), 0.0, t.text);
        if !open {
            p.rect_filled(Rect::from_min_max(pos2(c.x, c.y - 2.0), pos2(c.x + 1.0, c.y + 3.0)), 0.0, t.text);
        }
    }
    let mut x = x0 + 14.0;
    if let Some(icon) = icon {
        paint_icon(ui.painter(), Rect::from_min_size(pos2(x, cy - 8.0), vec2(16.0, 16.0)), icon, None);
        x += 20.0;
    }
    let p = ui.painter();
    let color = if selected { Color32::WHITE } else { t.text };
    let galley = p.layout_no_wrap(label.to_owned(), t.font(t.font.ui), color);
    let text_pos = pos2(x, (cy - galley.size().y * 0.5).round());
    if selected {
        // The highlight bar covers the label only, with a dotted focus rectangle.
        let box_r = px(Rect::from_min_size(text_pos - vec2(2.0, 1.0), galley.size() + vec2(4.0, 2.0)));
        p.rect_filled(box_r, 0.0, t.menu_popup_hover);
        let (a, b) = (box_r.min, box_r.max - vec2(1.0, 1.0));
        dotted(p, a, pos2(b.x, a.y), t.text_bright);
        dotted(p, pos2(a.x, b.y), b, t.text_bright);
        dotted(p, a, pos2(a.x, b.y), t.text_bright);
        dotted(p, pos2(b.x, a.y), b, t.text_bright);
    }
    p.galley(text_pos, galley, color);
    (resp, toggled)
}

/// Full-width list row: the `#316AC5` highlight bar over a white list box when selected, no hover fill.
pub fn list_row(ui: &mut Ui, height: f32, selected: bool) -> Response {
    let t = theme::get();
    let w = ui.available_width().max(60.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, height), Sense::click_and_drag());
    if selected {
        ui.painter().rect_filled(rect, 0.0, t.menu_popup_hover);
    }
    resp
}

/// Column header row (20 px): labels with 1 px separators. `cols`: (title, width). The column clicked, if any.
pub fn header_row(ui: &mut Ui, cols: &[(&str, f32)]) -> Option<usize> {
    header_row_sorted(ui, cols, None)
}

/// `header_row` with the sorted column marked: raised bevelled header buttons with a ▲ / ▼ sort arrow on `sort` =
/// (column, ascending).
pub fn header_row_sorted(ui: &mut Ui, cols: &[(&str, f32)], sort: Option<(usize, bool)>) -> Option<usize> {
    let t = theme::get();
    let w = ui.available_width().max(cols.iter().map(|c| c.1).sum());
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 20.0), Sense::click());
    let p = ui.painter();
    // Windows header control: one raised button per column, a blank one filling the rest of the row.
    let m = &t.metal;
    let mut x = rect.min.x;
    let ends: Vec<f32> = cols.iter().map(|c| c.1).chain([f32::INFINITY]).collect();
    for (k, cw) in ends.iter().enumerate() {
        let x1 = (x + cw).min(rect.max.x);
        if x1 <= x {
            break;
        }
        let r = Rect::from_min_max(pos2(x, rect.min.y), pos2(x1, rect.max.y));
        p.rect_filled(r, 0.0, m.face);
        bevel(p, r, true, m);
        if let Some((title, _)) = cols.get(k) {
            p.with_clip_rect(r.shrink(2.0).intersect(ui.clip_rect())).text(pos2(x + 6.0, rect.center().y), Align2::LEFT_CENTER, *title, t.font(t.font.ui), t.text);
            if let Some((_, asc)) = sort.filter(|s| s.0 == k) {
                let ar = Rect::from_center_size(pos2(x1 - 10.0, rect.center().y), vec2(8.0, 8.0));
                arrow(p, ar, (0.0, if asc { -1.0 } else { 1.0 }), m.shadow);
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

/// Grid lines of a details list (Edit Details): a light rule under the row and one between the columns.
pub fn grid_lines(p: &Painter, rect: Rect, cols: &[(&str, f32)]) {
    let c = Color32::from_rgb(0xC0, 0xC0, 0xC0);
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
    dotted_splitter(ui.painter(), split, false, &t.metal);
    let resp = ui.interact(split, ui.id().with(("pane_split", id)), Sense::drag());
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
    }
    if resp.dragged() {
        *width = w + resp.drag_delta().x;
    }
    (left, right)
}

/// Child ui confined to `rect` with its own scroll area id: a sunken white list box (the Windows
/// list / tree view) with the contents inside its 2 px edge.
pub fn scroll_pane(ui: &mut Ui, rect: Rect, id: &str, add: impl FnOnce(&mut Ui)) {
    let t = theme::get();
    sunken(ui.painter(), rect, t.input_bg, &t.metal);
    let rect = rect.shrink(2.0);
    let mut child = ui.new_child(UiBuilder::new().id_salt(("pane", id)).max_rect(rect).layout(Layout::top_down(Align::Min)));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    ScrollArea::both().id_salt(id).auto_shrink([false, false]).show(&mut child, |ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        add(ui)
    });
}

/// Search field in `input_bg` with a placeholder.
pub fn search_field(ui: &mut Ui, text: &mut String, hint: &str, width: f32) -> Response {
    let t = theme::get();
    let h = (t.m.button - 2.0).max(18.0);
    let resp = ui.add_sized(
        vec2(width, h),
        TextEdit::singleline(text)
            .hint_text(RichText::new(hint).color(t.input_placeholder))
            .background_color(t.input_bg)
            .font(t.font(t.font.ui))
            .margin(vec2(4.0, 2.0)),
    );
    // egui's response rect is the inner one (inside the 4 / 2 px margin).
    bevel(ui.painter(), resp.rect.expand2(vec2(4.0, 2.0)), false, &t.metal);
    resp
}

/// Thumbnail tile: `thumb_rect` image area (letterboxed texture or a painted stand-in), caption centred below,
/// up to two lines, on a raised chrome tile (pressed in when selected).
pub fn tile(ui: &Ui, rect: Rect, thumb: Rect, label: &str, selected: bool, hovered: bool) {
    let t = theme::get();
    let p = ui.painter();
    // A raised chrome preset tile, pressed in (blue tint) when selected.
    let state = if selected { Chrome::Active } else if hovered { Chrome::Hover } else { Chrome::Raised };
    chrome_button(p, rect, state, &t.metal);
    let color = t.text;
    let cap = Rect::from_min_max(pos2(rect.min.x + 2.0, thumb.max.y + 3.0), pos2(rect.max.x - 2.0, rect.max.y));
    let mut job = text::LayoutJob::simple(label.to_owned(), t.font(t.font.ui), color, cap.width());
    job.halign = Align::Center;
    job.wrap.max_rows = 2;
    job.wrap.break_anywhere = true;
    let galley = p.layout_job(job);
    p.with_clip_rect(cap.intersect(ui.clip_rect())).galley(pos2(cap.center().x, cap.min.y), galley, color);
}

/// The black picture well a preset thumbnail sits in: sunken, so a title or a generator with transparent areas reads
/// against black (as VEGAS shows them) instead of against the tile's brushed metal.
pub fn thumb_well(p: &Painter, r: Rect) {
    let r = px(r);
    p.rect_filled(r, 0.0, Color32::BLACK);
    // Sunken 1 px frame: dark top-left, light bottom-right, outside the picture.
    let o = r.expand(1.0);
    let (dark, light) = (Color32::from_rgb(0x40, 0x40, 0x40), Color32::from_rgb(0xFF, 0xFF, 0xFF));
    p.line_segment([o.left_top(), o.right_top()], Stroke::new(1.0_f32, dark));
    p.line_segment([o.left_top(), o.left_bottom()], Stroke::new(1.0_f32, dark));
    p.line_segment([o.left_bottom(), o.right_bottom()], Stroke::new(1.0_f32, light));
    p.line_segment([o.right_top(), o.right_bottom()], Stroke::new(1.0_f32, light));
}

/// Letterboxed image inside `r` with a 1 px dark frame and a grey mat.
pub fn paint_thumb(p: &Painter, r: Rect, tex: &TextureHandle) {
    let [w, h] = tex.size();
    let aspect = w as f32 / h.max(1) as f32;
    let (iw, ih) = if r.width() / r.height() > aspect { (r.height() * aspect, r.height()) } else { (r.width(), r.width() / aspect) };
    let ir = px(Rect::from_center_size(r.center(), vec2(iw, ih)));
    p.image(tex.id(), ir, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    // 1 px dark frame and a thin grey mat round it.
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
