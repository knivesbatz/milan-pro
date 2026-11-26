//! Window frame (UI_SPEC.md §4): band layout of the client area under the native title bar, the in-window menu row,
//! and the metal bands with their 3D edges and dotted splitters.
use eframe::egui::*;
use super::theme::Theme;

/// Client-area bands, in points (== px in the screenshot harness).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Areas {
    /// The menu row.
    pub menu: Rect,
    /// Main toolbar body (without its bevel rows and bottom line).
    pub toolbar: Rect,
    /// Docking areas by `dock::Area` (top, bottom, left, right): group toolbars, content and tab strips.
    /// `Rect::NOTHING` when the area is absent.
    pub dock: [Rect; 4],
    /// Drag zone of each area's splitter with the timeline.
    pub split: [Rect; 4],
    /// Time display + ruler band + track rows.
    pub timeline: Rect,
    /// Track-list column of the transport row (Rate slider).
    pub rate: Rect,
    /// Transport/edit toolbar body.
    pub transport: Rect,
    /// Status bar body.
    pub status: Rect,
}

/// Lay out the client area. `docks`: each docking area's height (top, bottom) or width (left, right) incl. tab
/// strips, 0 = none (`Dock::area_sizes`); `track_w`: track-list width. The bottom area sits under the transport bar,
/// the side areas beside the timeline between the top and bottom ones.
pub fn areas(t: &Theme, full: Rect, docks: [f32; 4], track_w: f32) -> Areas {
    let (x0, x1) = (full.min.x, full.max.x);
    let row = |y0: f32, h: f32, l: f32, r: f32| Rect::from_min_max(pos2(x0 + l, y0), pos2(x1 - r, y0 + h));
    let m = &t.m;
    let g = m.outer_margin;
    // Menu row 20 · 1 px shadow · 2 px raised bevel · toolbar 26 · 1 px shadow · 2 px gap; the status bar stands on a
    // 1 px edge with a 2 px gap above it (UI_SPEC.md §4).
    let menu = row(full.min.y, m.menu_bar_h, 0.0, 0.0);
    let toolbar = row(menu.max.y + 3.0, m.main_toolbar_h, 0.0, 0.0);
    let dock_top = toolbar.max.y + 3.0;
    let status = row(full.max.y - 1.0 - m.status_h, m.status_h, 0.0, 0.0);
    let transport_bottom = status.min.y - m.status_gap - 1.0;
    // A 1 px light edge above the transport's metal bar.
    let edges = 1.0;
    let sw = m.splitter_w;
    let [top, bottom, left, right] = docks;
    // The 6 px dotted splitter under the top tab strips.
    let top_gap = sw;
    // Leave at least 120 px for the timeline when the window is small: the top and bottom areas shrink alike.
    let gaps = if top > 0.0 { top_gap } else { 0.0 } + if bottom > 0.0 { sw } else { 0.0 };
    let room = (transport_bottom - dock_top - gaps - edges - m.transport_h - 1.0 - 120.0).max(60.0);
    let k = if top + bottom > room { room / (top + bottom) } else { 1.0 };
    let (mut dock, mut split) = ([Rect::NOTHING; 4], [Rect::NOTHING; 4]);
    let mut timeline_top = dock_top;
    if top > 0.0 {
        dock[0] = row(dock_top, (top * k).floor(), g, g);
        (split[0], timeline_top) = (row(dock[0].max.y, top_gap, g, g), dock[0].max.y + top_gap);
    }
    let mut mid_bottom = transport_bottom;
    if bottom > 0.0 {
        dock[1] = row(transport_bottom - (bottom * k).floor(), (bottom * k).floor(), g, g);
        split[1] = row(dock[1].min.y - sw, sw, g, g);
        mid_bottom = split[1].min.y;
    }
    // Side areas leave the timeline 300 px past the track list.
    let (mut xl, mut xr) = (x0 + g, x1 - g);
    let side_gaps = if left > 0.0 { sw } else { 0.0 } + if right > 0.0 { sw } else { 0.0 };
    let side_room = (xr - xl - side_gaps - track_w - 300.0).max(0.0);
    let ks = if left + right > side_room { side_room / (left + right) } else { 1.0 };
    let col = |a: f32, b: f32| Rect::from_min_max(pos2(a, timeline_top), pos2(b, mid_bottom));
    if left > 0.0 {
        dock[2] = col(xl, xl + (left * ks).floor());
        split[2] = col(dock[2].max.x, dock[2].max.x + sw);
        xl = split[2].max.x;
    }
    if right > 0.0 {
        dock[3] = col(xr - (right * ks).floor(), xr);
        split[3] = col(dock[3].min.x - sw, dock[3].min.x);
        xr = split[3].min.x;
    }
    let transport_y = mid_bottom - edges - m.transport_h;
    let timeline_bottom = transport_y - 1.0;
    let timeline = Rect::from_min_max(pos2(xl, timeline_top), pos2(xr, timeline_bottom));
    let tl = xl + track_w;
    let rate = Rect::from_min_max(pos2(xl, transport_y - edges), pos2(tl, mid_bottom));
    let transport = Rect::from_min_max(pos2(tl + m.timeline_gap, transport_y), pos2(xr, transport_y + m.transport_h));
    Areas { menu, toolbar, dock, split, timeline, rate, transport, status }
}

fn hline(p: &Painter, x: Rangef, y: f32, c: Color32) {
    p.rect_filled(Rect::from_x_y_ranges(x, Rangef::new(y, y + 1.0)), 0.0, c);
}

/// The bands: brushed metal with 3D edges and dotted splitters.
pub fn paint_bands(p: &Painter, t: &Theme, a: &Areas, full: Rect) {
    let m = &t.metal;
    let all = full.x_range();
    // Menu row: metal, shadow line under it.
    super::widgets::metal(p, a.menu, m);
    hline(p, all, a.menu.max.y, m.shadow);
    // Main toolbar: raised 2 px bevel over the body, shadow line under it.
    hline(p, all, a.toolbar.min.y - 2.0, m.light);
    hline(p, all, a.toolbar.min.y - 1.0, m.face);
    super::widgets::metal(p, a.toolbar, m);
    hline(p, all, a.toolbar.max.y, m.shadow);
    // Transport bar: light edge above, metal bar.
    super::widgets::metal_bar(p, a.transport, m);
    // Status bar: light edge above, metal; the panes are sunken by `draw_status_bar`.
    hline(p, all, a.status.min.y - 1.0, m.light);
    super::widgets::metal(p, a.status, m);
    // Dotted splitters between the docking areas and the timeline.
    for (k, s) in a.split.iter().enumerate() {
        if s.is_positive() {
            super::widgets::dotted_splitter(p, *s, k < 2, m);
        }
    }
}

/// Popup style for menus: white with a 3D grey border and the #316AC5 highlight bar (Windows XP).
pub fn menu_style(base: &Style, t: &Theme) -> Style {
    let mut s = base.clone();
    let v = &mut s.visuals;
    v.dark_mode = false;
    v.override_text_color = Some(t.menu_popup_text);
    v.window_fill = t.menu_popup_bg;
    v.panel_fill = t.menu_popup_bg;
    v.window_stroke = Stroke::new(1.0_f32, t.menu_popup_border);
    v.menu_corner_radius = CornerRadius::same(t.m.widget_radius);
    v.popup_shadow = Shadow::NONE;
    let hover_text = t.text_bright;
    let w = &mut v.widgets;
    // Shortcut text and separators in black / grey, as Windows draws them.
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, t.menu_popup_text_disabled);
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, t.menu_popup_text);
    w.inactive.fg_stroke = Stroke::new(1.0_f32, t.menu_popup_text);
    w.inactive.bg_fill = t.menu_popup_bg;
    w.inactive.bg_stroke = Stroke::new(1.0_f32, t.menu_popup_text_disabled);
    for st in [&mut w.hovered, &mut w.active, &mut w.open] {
        st.bg_fill = t.menu_popup_hover;
        st.weak_bg_fill = t.menu_popup_hover;
        st.fg_stroke = Stroke::new(1.0_f32, hover_text);
        st.bg_stroke = Stroke::NONE;
    }
    v.selection.bg_fill = t.menu_popup_hover;
    v.selection.stroke = Stroke::new(1.0_f32, hover_text);
    s.spacing.interact_size.y = t.m.popup_row_h;
    s.spacing.item_spacing.y = 0.0;
    s.spacing.menu_margin = Margin::same(3);
    // Popups size to their rows (egui's default is 400 px wide).
    s.spacing.menu_width = 160.0;
    s
}

/// Run `f` (the menu bar) with the menu popup style on the context, so popups opened inside get it.
pub fn with_menu_style<R>(ctx: &Context, t: &Theme, f: impl FnOnce() -> R) -> R {
    let saved = ctx.style();
    ctx.set_style(menu_style(&saved, t));
    let r = f();
    ctx.set_style(saved);
    r
}

/// The menu row on metal: Tahoma labels, a raised bevel on hover.
pub fn menu_row<R>(ui: &mut Ui, rect: Rect, f: impl FnOnce(&mut Ui) -> R) -> R {
    let mut child = ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::left_to_right(Align::Center)));
    f(&mut child)
}

/// The classic 3D border of a popup (menus, drop-downs): egui paints the fill and a 1 px `shadow` line; this adds the
/// light inner top-left and dark outer bottom-right rows over it.
pub fn classic_popup_border(p: &Painter, r: Rect, t: &Theme) {
    let m = &t.metal;
    let r = Rect::from_min_max(r.min.round(), r.max.round());
    p.rect_filled(Rect::from_min_max(pos2(r.min.x + 1.0, r.min.y + 1.0), pos2(r.max.x - 1.0, r.min.y + 2.0)), 0.0, m.light);
    p.rect_filled(Rect::from_min_max(pos2(r.min.x + 1.0, r.min.y + 1.0), pos2(r.min.x + 2.0, r.max.y - 1.0)), 0.0, m.light);
    p.rect_filled(Rect::from_min_max(pos2(r.min.x, r.max.y - 1.0), r.max), 0.0, m.dark);
    p.rect_filled(Rect::from_min_max(pos2(r.max.x - 1.0, r.min.y), r.max), 0.0, m.dark);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Band positions at 1920×1080 (UI_SPEC.md §4).
    #[test]
    fn default_bands_match_spec() {
        let full = Rect::from_min_size(Pos2::ZERO, vec2(1920.0, 1080.0));
        // Menu row 0–20, toolbar 23–49 (bevel 21–22), dock from 52, a 6 px dotted splitter under it, transport 28 on a
        // 1 px edge, status 20 on a 1 px edge with a 2 px gap above.
        let t = Theme::new();
        let a = areas(&t, full, [t.m.dock_h, 0.0, 0.0, 0.0], t.m.track_list_w);
        assert_eq!((a.menu.max.y, a.toolbar.min.y, a.toolbar.max.y, a.dock[0].min.y, a.dock[0].max.y), (20.0, 23.0, 49.0, 52.0, 586.0));
        assert_eq!((a.split[0].min.y, a.split[0].height(), a.timeline.min.y, a.dock[0].min.x), (586.0, 6.0, 592.0, 2.0));
        assert_eq!((a.status.min.y, a.status.max.y, a.transport.max.y, a.transport.height()), (1059.0, 1079.0, 1055.0, 28.0));
        assert_eq!(a.timeline.max.y, a.transport.min.y - 1.0);
    }

    /// R13: the timeline above a bottom docking area (Timeline Top), a right area beside it, and docks that would leave
    /// the timeline under 120 px shrink alike.
    #[test]
    fn bottom_and_side_areas() {
        let full = Rect::from_min_size(Pos2::ZERO, vec2(1920.0, 1080.0));
        let t = Theme::new();
        let a = areas(&t, full, [0.0, 449.0, 0.0, 300.0], t.m.track_list_w);
        assert_eq!((a.timeline.min.y, a.dock[1].max.y, a.dock[1].height()), (52.0, 1056.0, 449.0));
        assert_eq!(a.transport.max.y, a.split[1].min.y - 1.0);
        assert_eq!((a.dock[3].max.x, a.dock[3].width(), a.timeline.max.x, a.transport.max.x), (1918.0, 300.0, 1612.0, 1612.0));
        assert_eq!((a.dock[3].min.y, a.dock[3].max.y), (52.0, a.split[1].min.y));
        let a = areas(&t, full, [900.0, 900.0, 0.0, 0.0], t.m.track_list_w);
        assert!(a.timeline.height() >= 120.0 && (a.dock[0].height() - a.dock[1].height()).abs() < 1.0, "{a:?}");
    }
}
