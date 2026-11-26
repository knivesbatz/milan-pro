//! Small painted widgets shared by the dock windows: tree rows, list rows and headers, the 5 px pane splitter,
//! search fields and preview tiles; and the Classic 2006 brushed-metal / 3D-bevel painters every bar uses
//! (CLASSIC_2006_SPEC.md §2, §4).
use eframe::egui::*;
use super::icons::{Icon, paint_icon};
use super::theme::{self, Metal, Theme, UiLook};

// ---- Classic 2006 metal and bevels ----

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

pub const ROW_H: f32 = 20.0;

/// `r` with its corners on whole pixels.
pub fn px(r: Rect) -> Rect {
    Rect::from_min_max(r.min.round(), r.max.round())
}

/// One tree node. Returns (row response, expander clicked).
/// `expanded`: None = leaf. VP16 draws ⊞/⊟ boxes and a white label box when selected; VP23 ▸/▾ and an accent row.
pub fn tree_row(ui: &mut Ui, depth: usize, expanded: Option<bool>, icon: Option<Icon>, label: &str, selected: bool) -> (Response, bool) {
    let t = theme::get();
    let v16 = t.look() == UiLook::Vegas16;
    let w = ui.available_width().max(60.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, ROW_H), Sense::click_and_drag());
    let p = ui.painter();
    let x0 = rect.min.x + 4.0 + depth as f32 * 16.0;
    let cy = rect.center().y;
    if selected && !v16 {
        p.rect_filled(rect, 0.0, t.accent);
    } else if resp.hovered() && !v16 {
        p.rect_filled(rect, 0.0, t.button_hover_bg);
    }
    let exp_rect = Rect::from_center_size(pos2(x0 + 5.0, cy), vec2(9.0, 9.0));
    let mut toggled = false;
    if let Some(open) = expanded {
        let er = ui.interact(exp_rect.expand(3.0), resp.id.with("exp"), Sense::click());
        toggled = er.clicked();
        let p = ui.painter();
        if v16 {
            p.rect_filled(exp_rect, 0.0, t.text_bright);
            p.rect_stroke(exp_rect, 0.0, Stroke::new(1.0_f32, t.text_dim), StrokeKind::Inside);
            let c = exp_rect.center();
            p.hline(Rangef::new(c.x - 2.5, c.x + 2.5), c.y, Stroke::new(1.0_f32, t.frame));
            if !open {
                p.vline(c.x, Rangef::new(c.y - 2.5, c.y + 2.5), Stroke::new(1.0_f32, t.frame));
            }
        } else {
            let c = exp_rect.center();
            let pts = if open {
                vec![pos2(c.x - 3.5, c.y - 2.0), pos2(c.x + 3.5, c.y - 2.0), pos2(c.x, c.y + 2.5)]
            } else {
                vec![pos2(c.x - 2.0, c.y - 3.5), pos2(c.x + 2.5, c.y), pos2(c.x - 2.0, c.y + 3.5)]
            };
            p.add(Shape::convex_polygon(pts, t.text, Stroke::NONE));
        }
    }
    let mut x = x0 + 14.0;
    if let Some(icon) = icon {
        paint_icon(ui.painter(), Rect::from_min_size(pos2(x, cy - 8.0), vec2(16.0, 16.0)), icon, t.look(), None);
        x += 20.0;
    }
    let p = ui.painter();
    let color = if selected && v16 { t.frame } else if selected { Color32::WHITE } else { t.text };
    let galley = p.layout_no_wrap(label.to_owned(), t.font(t.font.ui), color);
    let text_pos = pos2(x, (cy - galley.size().y * 0.5).round());
    if selected && v16 {
        let box_r = Rect::from_min_size(text_pos - vec2(2.0, 1.0), galley.size() + vec2(4.0, 2.0));
        p.rect_filled(box_r, 0.0, t.text_bright);
        p.rect_stroke(box_r, 0.0, Stroke::new(1.0_f32, t.frame), StrokeKind::Inside);
    }
    p.galley(text_pos, galley, color);
    (resp, toggled)
}

/// Full-width list row (accent fill when selected, hover fill otherwise).
pub fn list_row(ui: &mut Ui, height: f32, selected: bool) -> Response {
    let t = theme::get();
    let w = ui.available_width().max(60.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, height), Sense::click_and_drag());
    if selected {
        ui.painter().rect_filled(rect, 0.0, t.accent);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 0.0, t.button_hover_bg);
    }
    resp
}

/// Column header row (20 px): labels with 1 px separators. `cols`: (title, width). The column clicked, if any.
pub fn header_row(ui: &mut Ui, cols: &[(&str, f32)]) -> Option<usize> {
    let t = theme::get();
    let w = ui.available_width().max(cols.iter().map(|c| c.1).sum());
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 20.0), Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, 0.0, t.toolbar_bg);
    p.hline(rect.x_range(), rect.max.y - 0.5, Stroke::new(1.0_f32, t.frame));
    let mut x = rect.min.x;
    for (title, cw) in cols {
        p.text(pos2(x + 6.0, rect.center().y), Align2::LEFT_CENTER, *title, t.font(t.font.ui), t.text);
        x += cw;
        p.vline(x - 0.5, rect.y_range(), Stroke::new(1.0_f32, t.frame));
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

/// Vertical 5 px splitter at the right edge of a left pane `width` wide inside `area`; drag resizes the pane.
pub fn pane_split(ui: &mut Ui, area: Rect, width: &mut f32, id: &str) -> (Rect, Rect) {
    let t = theme::get();
    // The stored width survives a narrow frame (the first frame, a small dock); only drags change it.
    let w = width.clamp(60.0, (area.width() - 80.0).max(60.0));
    let left = Rect::from_min_max(area.min, pos2(area.min.x + w, area.max.y));
    let split = Rect::from_min_max(pos2(left.max.x, area.min.y), pos2(left.max.x + 5.0, area.max.y));
    let right = Rect::from_min_max(pos2(split.max.x, area.min.y), area.max);
    ui.painter().rect_filled(split, 0.0, t.frame);
    let resp = ui.interact(split, ui.id().with(("pane_split", id)), Sense::drag());
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
    }
    if resp.dragged() {
        *width = w + resp.drag_delta().x;
    }
    (left, right)
}

/// Child ui confined to `rect` with its own scroll area id.
pub fn scroll_pane(ui: &mut Ui, rect: Rect, id: &str, add: impl FnOnce(&mut Ui)) {
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
    ui.add_sized(
        vec2(width, h),
        TextEdit::singleline(text)
            .hint_text(RichText::new(hint).color(t.input_placeholder))
            .background_color(t.input_bg)
            .font(t.font(t.font.ui))
            .margin(vec2(4.0, 2.0)),
    )
}

/// Thumbnail tile: `thumb_rect` image area (letterboxed texture or a painted stand-in), caption centred below,
/// up to two lines. VP16 selection: white tile with grey text (§5.6); VP23: accent outline.
pub fn tile(ui: &Ui, rect: Rect, thumb: Rect, label: &str, selected: bool, hovered: bool) {
    let t = theme::get();
    let p = ui.painter();
    if selected {
        if t.look() == UiLook::Vegas16 {
            p.rect_filled(rect, 0.0, t.preset_tile_selected);
        } else {
            p.rect_stroke(rect, CornerRadius::same(2), Stroke::new(2.0_f32, t.accent), StrokeKind::Inside);
        }
    } else if hovered {
        p.rect_filled(rect, 0.0, t.button_hover_bg);
    }
    let color = if selected && t.look() == UiLook::Vegas16 { t.text_dim } else { t.text };
    let cap = Rect::from_min_max(pos2(rect.min.x + 2.0, thumb.max.y + 3.0), pos2(rect.max.x - 2.0, rect.max.y));
    let mut job = text::LayoutJob::simple(label.to_owned(), t.font(t.font.ui), color, cap.width());
    job.halign = Align::Center;
    job.wrap.max_rows = 2;
    job.wrap.break_anywhere = true;
    let galley = p.layout_job(job);
    p.with_clip_rect(cap.intersect(ui.clip_rect())).galley(pos2(cap.center().x, cap.min.y), galley, color);
}

/// Letterboxed image inside `r` with a 1 px border (VP16 white).
pub fn paint_thumb(p: &Painter, r: Rect, tex: &TextureHandle) {
    let t = theme::get();
    let [w, h] = tex.size();
    let aspect = w as f32 / h.max(1) as f32;
    let (iw, ih) = if r.width() / r.height() > aspect { (r.height() * aspect, r.height()) } else { (r.width(), r.width() / aspect) };
    let ir = px(Rect::from_center_size(r.center(), vec2(iw, ih)));
    p.image(tex.id(), ir, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    if t.look() == UiLook::Vegas16 {
        p.rect_stroke(ir, 0.0, Stroke::new(1.0_f32, Color32::WHITE), StrokeKind::Outside);
    }
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
