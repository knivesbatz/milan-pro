//! Window frame per look (§1.2, §4.1, §4.2, §4.6, §5.1): band layout of the client area, the VP16 native-style
//! white menu row, the VP23 custom 32 px title bar (inline menu, caption buttons, drag, maximise, edge resize)
//! and the 1 px bevel lines between bands.
use eframe::egui::*;
use super::icons::{Icon, paint_icon};
use super::theme::{self, Theme, UiLook};

/// Client-area bands, in points (== px in the screenshot harness).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Areas {
    /// VP16 menu row, or the VP23 title bar with the inline menu.
    pub menu: Rect,
    /// Main toolbar body (VP16: without its bevel rows and bottom line).
    pub toolbar: Rect,
    /// Dock area: group toolbars, content and tab strips.
    pub dock: Rect,
    /// Drag zone of the dock/timeline splitter.
    pub split: Rect,
    /// Time display + ruler band + track rows.
    pub timeline: Rect,
    /// Track-list column of the transport row (Rate slider).
    pub rate: Rect,
    /// Transport/edit toolbar body.
    pub transport: Rect,
    /// Status bar body.
    pub status: Rect,
}

/// Lay out the client area. `dock_h`: dock height incl. tab strips; `track_w`: track-list width.
pub fn areas(t: &Theme, full: Rect, dock_h: f32, track_w: f32) -> Areas {
    let (x0, x1, w) = (full.min.x, full.max.x, full.width());
    let row = |y0: f32, h: f32, l: f32, r: f32| Rect::from_min_max(pos2(x0 + l, y0), pos2(x1 - r, y0 + h));
    let m = &t.m;
    let g = m.outer_margin;
    let v16 = t.look() == UiLook::Vegas16;
    let (menu, toolbar, dock_top, status, transport_bottom);
    if v16 {
        // menu 20 · 1 px line · 2 px gap · 2 px bevel · toolbar 31 · 1 px line · 2 px gap (§4.1, measured [L]).
        menu = row(full.min.y, m.menu_bar_h, 0.0, 0.0);
        toolbar = row(menu.max.y + 5.0, m.main_toolbar_h, g, g);
        dock_top = toolbar.max.y + 3.0;
        // Bottom: frame row · 2 edge rows · status 17 · 1 px edge.
        status = row(full.max.y - 1.0 - m.status_h, m.status_h, 0.0, 0.0);
        transport_bottom = status.min.y - 3.0;
    } else {
        menu = row(full.min.y, m.title_bar_h, 0.0, 0.0);
        toolbar = row(menu.max.y, m.main_toolbar_h, 0.0, 0.0);
        dock_top = toolbar.max.y + 5.0; // 5 px frame gap (dark2026 y=67-71)
        // Bottom: status 20 · 2 px frame; a 5 px gap above it (§5.11, measured [M]).
        status = row(full.max.y - 2.0 - m.status_h, m.status_h, 0.0, 0.0);
        transport_bottom = status.min.y - m.status_gap;
    }
    // VP16 transport block: 1 px bevel · 30 · 1 px edge; VP23: 32, with a 1 px frame line above.
    let edges = if v16 { 1.0 } else { 0.0 };
    let transport_y = transport_bottom - edges - m.transport_h;
    let timeline_bottom = transport_y - 1.0;
    // Leave at least 120 px for the timeline when the window is small.
    let max_dock = (timeline_bottom - dock_top - 120.0).max(60.0);
    let dock = row(dock_top, dock_h.clamp(60.0, max_dock), g, g);
    // VP16: 2 edge rows + 7 px splitter under the tab strips; VP23: the tab strip touches the timeline.
    let (split, timeline_top) = if v16 {
        let gap = 2.0 + m.splitter_w;
        (row(dock.max.y, gap, g, g), dock.max.y + gap)
    } else {
        (row(dock.max.y - 4.0, 4.0, g, g), dock.max.y)
    };
    let timeline = Rect::from_min_max(pos2(x0 + g, timeline_top), pos2(x1 - g, timeline_bottom));
    let tl = x0 + g + track_w;
    let rate = Rect::from_min_max(pos2(x0 + g, transport_y - edges), pos2(tl, transport_bottom));
    let transport = Rect::from_min_max(pos2(tl + m.timeline_gap, transport_y), pos2(x0 + w - g, transport_y + m.transport_h));
    Areas { menu, toolbar, dock, split, timeline, rate, transport, status }
}

fn hline(p: &Painter, x: Rangef, y: f32, c: Color32) {
    p.rect_filled(Rect::from_x_y_ranges(x, Rangef::new(y, y + 1.0)), 0.0, c);
}

/// Bevel and edge lines between the bands (VP16); VP23 bands are flat on the frame colour.
pub fn paint_bands(p: &Painter, t: &Theme, a: &Areas, full: Rect) {
    p.rect_filled(a.menu, 0.0, t.menu_bar_bg);
    p.rect_filled(a.toolbar, 0.0, t.toolbar_bg);
    p.rect_filled(a.status, 0.0, t.status_bg);
    p.rect_filled(a.transport, 0.0, t.toolbar_bg);
    if t.look() != UiLook::Vegas16 {
        return;
    }
    let all = full.x_range();
    let b = &t.bevel;
    hline(p, all, a.menu.max.y - 1.0, t.menu_bar_bottom);
    hline(p, all, a.menu.max.y, t.menu_bar_line);
    let tb = a.toolbar.x_range();
    hline(p, tb, a.toolbar.min.y - 2.0, t.bevel_hi_1);
    hline(p, tb, a.toolbar.min.y - 1.0, t.bevel_hi_2);
    hline(p, tb, a.toolbar.max.y, t.line_dark);
    // Under the dock tab strips.
    hline(p, a.split.x_range(), a.split.min.y, t.line_dark);
    hline(p, a.split.x_range(), a.split.min.y + 1.0, b.strip_2);
    // Transport row edges.
    let tr = a.transport.x_range();
    hline(p, tr, a.transport.min.y - 1.0, b.transport_top);
    hline(p, tr, a.transport.max.y, b.strip_1);
    // Status edges: frame row, two edge rows above, one below.
    hline(p, all, a.status.min.y - 2.0, t.status_edge_1);
    hline(p, all, a.status.min.y - 1.0, t.status_edge_2);
    hline(p, all, a.status.max.y, b.band_1);
}

/// Popup style for menus: native-light in VP16, dark with an accent hover in VP23 (§5.1).
pub fn menu_style(base: &Style, t: &Theme) -> Style {
    let mut s = base.clone();
    let v = &mut s.visuals;
    v.dark_mode = t.look() != UiLook::Vegas16 && t.dark_mode;
    v.override_text_color = Some(t.menu_popup_text);
    v.window_fill = t.menu_popup_bg;
    v.panel_fill = t.menu_popup_bg;
    v.window_stroke = Stroke::new(1.0_f32, t.menu_popup_border);
    v.menu_corner_radius = CornerRadius::same(t.m.widget_radius);
    let hover_text = if t.look() == UiLook::Vegas16 { t.menu_popup_text } else { t.text_bright };
    let w = &mut v.widgets;
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, t.text_dim.gamma_multiply(0.4));
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, t.menu_popup_text);
    w.inactive.fg_stroke = Stroke::new(1.0_f32, t.menu_popup_text);
    w.inactive.bg_fill = t.menu_popup_bg;
    w.inactive.bg_stroke = Stroke::new(1.0_f32, t.menu_popup_text_disabled);
    for st in [&mut w.hovered, &mut w.active, &mut w.open] {
        st.bg_fill = t.menu_popup_hover;
        st.weak_bg_fill = t.menu_popup_hover;
        st.fg_stroke = Stroke::new(1.0_f32, hover_text);
    }
    v.selection.bg_fill = t.menu_popup_hover;
    v.selection.stroke = Stroke::new(1.0_f32, hover_text);
    s.spacing.interact_size.y = t.m.popup_row_h;
    s.spacing.item_spacing.y = 0.0;
    s.spacing.menu_margin = Margin::same(2);
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

/// VP16: native white menu row; labels 12 px black, hover #E5F3FF with a #CCE8FF border.
pub fn menu_row<R>(ui: &mut Ui, rect: Rect, f: impl FnOnce(&mut Ui) -> R) -> R {
    let mut child = ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::left_to_right(Align::Center)));
    f(&mut child)
}

#[derive(Clone, Copy)]
enum Caption {
    Minimize,
    Maximize,
    Close,
}

/// VP23 title bar: logo, inline menu from x≈32, centred title, 46×32 caption buttons; drag moves the window,
/// double-click maximises.
pub fn title_bar<R>(ui: &mut Ui, rect: Rect, title: &str, menu: impl FnOnce(&mut Ui) -> R) -> R {
    let t = theme::get();
    let ctx = ui.ctx().clone();
    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    let bg = ui.interact(rect, ui.id().with("title_bar"), Sense::click_and_drag());
    if bg.double_clicked() {
        ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
    } else if bg.drag_started_by(PointerButton::Primary) {
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }
    let p = ui.painter().clone();
    p.rect_filled(rect, 0.0, t.title_bg);
    let logo = Rect::from_min_size(pos2(rect.min.x + 5.0, rect.center().y - 10.0), vec2(20.0, 20.0));
    paint_icon(&p, logo, Icon::Logo, t.look(), None);
    p.text(rect.center(), Align2::CENTER_CENTER, title, t.font(t.font.menu), t.menu_text);

    let caps = [Caption::Minimize, Caption::Maximize, Caption::Close];
    for (i, cap) in caps.into_iter().enumerate() {
        let r = Rect::from_min_size(pos2(rect.max.x - 46.0 * (3 - i) as f32, rect.min.y), vec2(46.0, rect.height()));
        let resp = ui.interact(r, ui.id().with(("caption", i)), Sense::click());
        let close = matches!(cap, Caption::Close);
        let mut ink = t.menu_text;
        if resp.hovered() {
            p.rect_filled(r, 0.0, if close { t.caption_close_hover } else { t.caption_hover });
            if close {
                ink = t.text_bright;
            }
        }
        let c = r.center().floor() + vec2(0.5, 0.5);
        let s = Stroke::new(1.0_f32, ink);
        match cap {
            Caption::Minimize => {
                p.line_segment([c + vec2(-5.0, 0.0), c + vec2(5.0, 0.0)], s);
            }
            Caption::Maximize if maximized => {
                p.rect_stroke(Rect::from_min_size(c + vec2(-5.0, -3.0), vec2(8.0, 8.0)), 0.0, s, StrokeKind::Middle);
                p.line_segment([c + vec2(-3.0, -3.0), c + vec2(-3.0, -5.0)], s);
                p.line_segment([c + vec2(-3.0, -5.0), c + vec2(5.0, -5.0)], s);
                p.line_segment([c + vec2(5.0, -5.0), c + vec2(5.0, 3.0)], s);
                p.line_segment([c + vec2(5.0, 3.0), c + vec2(3.0, 3.0)], s);
            }
            Caption::Maximize => {
                p.rect_stroke(Rect::from_min_size(c + vec2(-5.0, -5.0), vec2(10.0, 10.0)), 0.0, s, StrokeKind::Middle);
            }
            Caption::Close => {
                p.line_segment([c + vec2(-5.0, -5.0), c + vec2(5.0, 5.0)], s);
                p.line_segment([c + vec2(5.0, -5.0), c + vec2(-5.0, 5.0)], s);
            }
        }
        let text = ["Minimize", if maximized { "Restore Down" } else { "Maximize" }, "Close"][i];
        if resp.on_hover_text(text).clicked() {
            ctx.send_viewport_cmd(match cap {
                Caption::Minimize => ViewportCommand::Minimized(true),
                Caption::Maximize => ViewportCommand::Maximized(!maximized),
                Caption::Close => ViewportCommand::Close,
            });
        }
    }

    let menu_rect = Rect::from_min_max(pos2(rect.min.x + 30.0, rect.min.y), pos2(rect.center().x - 160.0, rect.max.y));
    let mut child = ui.new_child(UiBuilder::new().max_rect(menu_rect).layout(Layout::left_to_right(Align::Center)));
    menu(&mut child)
}

/// VP23 borderless window: 5 px resize zones on the edges (8 px corners). Call last so they win hit-tests.
pub fn resize_edges(ui: &mut Ui, full: Rect) {
    let ctx = ui.ctx().clone();
    if ctx.input(|i| i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false)) {
        return;
    }
    use ResizeDirection as D;
    let (e, c) = (5.0, 8.0);
    let (x0, x1, y0, y1) = (full.min.x, full.max.x, full.min.y, full.max.y);
    let r = |a: f32, b: f32, cc: f32, d: f32| Rect::from_min_max(pos2(a, b), pos2(cc, d));
    let zones = [
        (D::NorthWest, r(x0, y0, x0 + c, y0 + c), CursorIcon::ResizeNorthWest),
        (D::NorthEast, r(x1 - c, y0, x1, y0 + c), CursorIcon::ResizeNorthEast),
        (D::SouthWest, r(x0, y1 - c, x0 + c, y1), CursorIcon::ResizeSouthWest),
        (D::SouthEast, r(x1 - c, y1 - c, x1, y1), CursorIcon::ResizeSouthEast),
        (D::North, r(x0 + c, y0, x1 - c, y0 + e), CursorIcon::ResizeNorth),
        (D::South, r(x0 + c, y1 - e, x1 - c, y1), CursorIcon::ResizeSouth),
        (D::West, r(x0, y0 + c, x0 + e, y1 - c), CursorIcon::ResizeWest),
        (D::East, r(x1 - e, y0 + c, x1, y1 - c), CursorIcon::ResizeEast),
    ];
    for (i, (dir, rect, cursor)) in zones.into_iter().enumerate() {
        let resp = ui.interact(rect, ui.id().with(("resize_edge", i)), Sense::drag());
        if resp.hovered() || resp.dragged() {
            ctx.set_cursor_icon(cursor);
        }
        if resp.drag_started_by(PointerButton::Primary) {
            ctx.send_viewport_cmd(ViewportCommand::BeginResize(dir));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::LookChoice;

    /// Band positions at 1920×1080 against the measured references (§4.1 minus the 31 px OS title bar; §4.2).
    #[test]
    fn default_bands_match_spec() {
        let full = Rect::from_min_size(Pos2::ZERO, vec2(1920.0, 1080.0));
        let t = Theme::new(LookChoice::VEGAS16);
        let a = areas(&t, full, t.m.dock_h, t.m.track_list_w);
        assert_eq!((a.menu.max.y, a.toolbar.min.y, a.toolbar.max.y, a.dock.min.y), (20.0, 25.0, 56.0, 59.0));
        assert_eq!((a.dock.min.x, a.dock.max.x, a.dock.max.y, a.timeline.min.y), (3.0, 1917.0, 655.0, 664.0));
        assert_eq!((a.status.min.y, a.status.max.y, a.transport.min.y, a.transport.max.y), (1062.0, 1079.0, 1028.0, 1058.0));
        assert_eq!((a.rate.max.x, a.transport.min.x), (254.0, 258.0));

        let t = Theme::new(LookChoice::VEGAS23);
        let a = areas(&t, full, t.m.dock_h, t.m.track_list_w);
        assert_eq!((a.menu.max.y, a.toolbar.max.y, a.dock.min.y, a.dock.max.y), (32.0, 67.0, 72.0, 606.0));
        assert_eq!((a.dock.min.x, a.timeline.min.y, a.timeline.max.y), (5.0, 606.0, 1020.0));
        assert_eq!((a.transport.min.y, a.transport.max.y, a.status.min.y, a.status.max.y), (1021.0, 1053.0, 1058.0, 1078.0));
        assert_eq!((a.rate.max.x, a.transport.min.x), (255.0, 260.0));
    }
}
