//! Scrollbars and zoom (§5.9.9): horizontal bar under the lanes with ◂ ▸, a thumb whose ends zoom, + / − zoom
//! buttons and the magnifier; vertical bar right of the lanes with the marker corner button and + / − heights.
use eframe::egui::*;
use crate::project::*;
use crate::render::content_end;
use crate::ui::icons::{Icon, paint_icon};
use crate::ui::theme;
use super::{BASE_PPS, Geo, MIN_TRACK_H, TlState, set_zoom, track_h};

#[derive(Clone, Copy, Debug)]
pub(super) enum HDrag {
    /// Thumb body: scroll; (scroll_x at press, scroll range at press).
    Thumb(f64, f64),
    /// Thumb ends: zoom with the other end fixed; (fixed end time, scroll range at press).
    Left(f64, f64),
    Right(f64, f64),
}

struct HParts {
    left: Rect,
    right: Rect,
    track: Rect,
    thumb: Rect,
    plus: Rect,
    minus: Rect,
    zoom: Rect,
    total: f64,
}

/// Scroll range: the project plus some room, and never less than what is in view.
fn h_total(g: &Geo, project: &Project) -> f64 {
    let view = g.lanes.width() as f64 / g.pps;
    (content_end(project).max(30.0) * 1.1).max(g.scroll_x + view)
}

fn h_parts(g: &Geo, total: f64) -> HParts {
    let r = g.hscroll;
    let cell = |x: f32, w: f32| Rect::from_min_size(pos2(x, r.min.y), vec2(w, r.height()));
    let zoom = cell(r.max.x - g.vscroll.width(), g.vscroll.width());
    let minus = cell(zoom.min.x - 19.0, 19.0);
    let plus = cell(minus.min.x - 19.0, 19.0);
    let right = cell(plus.min.x - 16.0, 16.0);
    let left = cell(r.min.x, 16.0);
    // The thumb takes the bar's full 16 px.
    let track = Rect::from_min_max(pos2(left.max.x, r.min.y), pos2(right.min.x, r.max.y));
    let view = g.lanes.width() as f64 / g.pps;
    let tw = track.width() as f64;
    let x0 = track.min.x + (g.scroll_x / total * tw) as f32;
    let w = ((view / total * tw) as f32).max(8.0);
    let thumb = Rect::from_min_max(pos2(x0, track.min.y), pos2((x0 + w).min(track.max.x), track.max.y));
    HParts { left, right, track, thumb, plus, minus, zoom, total }
}

/// Zoom so `view` seconds fill the lanes, starting at `start`.
fn show_span(tl: &mut Timeline, g: &Geo, start: f64, view: f64) {
    set_zoom(tl, g.lanes.width() as f64 / (view.max(0.01) * BASE_PPS));
    tl.scroll_x = start.max(0.0);
}

/// `content`: the height of the track rows and group headers.
pub(super) fn interact(ui: &Ui, g: &Geo, content: f32, project: &mut Project, st: &mut TlState, scroll_y: &mut f32) {
    let h = h_parts(g, h_total(g, project));
    let end = content_end(project);
    let view = g.lanes.width() as f64 / g.pps;
    let id = ui.id().with("hscroll");
    let click = |r: Rect, k: &str| ui.interact(r, id.with(k), Sense::click());
    let tl = &mut project.timeline;
    if click(h.left, "left").clicked() {
        tl.scroll_x = (tl.scroll_x - view * 0.1).max(0.0);
    }
    if click(h.right, "right").clicked() {
        tl.scroll_x += view * 0.1;
    }
    let centre = tl.scroll_x + view * 0.5;
    if click(h.plus, "plus").on_hover_text("Zoom In").clicked() {
        show_span(tl, g, centre - view / 3.0, view / 1.5);
    }
    if click(h.minus, "minus").on_hover_text("Zoom Out").clicked() {
        show_span(tl, g, centre - view * 0.75, view * 1.5);
    }
    let _ = click(h.zoom, "zoom_tool").on_hover_text("Zoom Tool");
    let tr = ui.interact(h.track, id.with("track"), Sense::click());
    if tr.clicked() {
        if let Some(p) = tr.interact_pointer_pos() {
            tl.scroll_x = if p.x < h.thumb.min.x { (tl.scroll_x - view).max(0.0) } else { tl.scroll_x + view };
        }
    }
    let th = ui.interact(h.thumb.expand2(vec2(2.0, 0.0)), id.with("thumb"), Sense::click_and_drag());
    let at_end = |x: f32| (x - h.thumb.min.x).abs() <= 3.0 || (x - h.thumb.max.x).abs() <= 3.0;
    if th.hover_pos().is_some_and(|p| at_end(p.x)) || matches!(st.hdrag, Some(HDrag::Left(..) | HDrag::Right(..))) {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
    }
    if th.double_clicked() {
        show_span(tl, g, 0.0, end.max(1.0));
    }
    if th.drag_started() {
        if let Some(o) = ui.input(|i| i.pointer.press_origin()) {
            st.hdrag = Some(if (o.x - h.thumb.min.x).abs() <= 3.0 {
                HDrag::Left(tl.scroll_x + view, h.total)
            } else if (o.x - h.thumb.max.x).abs() <= 3.0 {
                HDrag::Right(tl.scroll_x, h.total)
            } else {
                HDrag::Thumb(tl.scroll_x, h.total)
            });
        }
    }
    let to_t = |x: f32, total: f64| ((x - h.track.min.x) as f64 / h.track.width().max(1.0) as f64 * total).max(0.0);
    if let (Some(d), Some(p), Some(o)) = (st.hdrag, th.interact_pointer_pos().filter(|_| th.dragged()), ui.input(|i| i.pointer.press_origin())) {
        match d {
            HDrag::Thumb(s0, total) => tl.scroll_x = (s0 + (p.x - o.x) as f64 / h.track.width().max(1.0) as f64 * total).max(0.0),
            HDrag::Left(end, total) => {
                let start = to_t(p.x, total).min(end - 0.05);
                show_span(tl, g, start, end - start);
            }
            HDrag::Right(start, total) => {
                let end = to_t(p.x, total).max(start + 0.05);
                show_span(tl, g, start, end - start);
            }
        }
    }
    if th.drag_stopped() {
        st.hdrag = None;
    }

    // Vertical: thumb drag scrolls, + / − step every track's height.
    let v = v_parts(g, content, *scroll_y);
    let vid = ui.id().with("vscroll");
    let vt = ui.interact(v.thumb, vid.with("thumb"), Sense::drag());
    if vt.drag_started() {
        st.vdrag = Some(*scroll_y);
    }
    if let (Some(s0), true) = (st.vdrag, vt.dragged()) {
        let dy = ui.input(|i| i.pointer.press_origin()).zip(vt.interact_pointer_pos()).map_or(0.0, |(o, p)| p.y - o.y);
        *scroll_y = s0 + dy / v.track.height().max(1.0) * v.content;
    }
    if vt.drag_stopped() {
        st.vdrag = None;
    }
    let t = theme::get();
    for (r, k, f) in [(v.plus, "plus", 1.25_f32), (v.minus, "minus", 0.8)] {
        if ui.interact(r, vid.with(k), Sense::click()).on_hover_text(if f > 1.0 { "Increase Track Height" } else { "Decrease Track Height" }).clicked() {
            let max = g.lanes.height().max(MIN_TRACK_H);
            for tr in project.timeline.video_tracks.iter_mut().chain(project.timeline.audio_tracks.iter_mut()) {
                tr.height = (track_h(tr, &t) * f).round().clamp(MIN_TRACK_H, max);
            }
            project.mark_modified();
        }
    }
    let _ = ui.interact(v.corner, vid.with("marker_tool"), Sense::click()).on_hover_text("Marker Tool");
}

struct VParts {
    corner: Rect,
    track: Rect,
    thumb: Rect,
    plus: Rect,
    minus: Rect,
    content: f32,
}

fn v_parts(g: &Geo, content: f32, scroll_y: f32) -> VParts {
    let c = g.vscroll;
    let corner = Rect::from_min_size(c.min, vec2(c.width(), c.width()));
    let minus = Rect::from_min_size(pos2(c.min.x, g.lanes.max.y - 16.0), vec2(c.width(), 16.0));
    let plus = minus.translate(vec2(0.0, -16.0));
    let track = Rect::from_min_max(pos2(c.min.x + 2.0, g.lanes.min.y), pos2(c.max.x - 2.0, plus.min.y - 1.0));
    let content = content.max(g.lanes.height());
    let th = (g.lanes.height() / content * track.height()).max(10.0).min(track.height());
    let y0 = track.min.y + scroll_y / content * track.height();
    let thumb = Rect::from_min_size(pos2(track.min.x, y0.min(track.max.y - th)), vec2(track.width(), th));
    VParts { corner, track, thumb, plus, minus, content }
}

pub(super) fn paint(p: &Painter, g: &Geo, content: f32, project: &Project, scroll_y: f32) {
    let t = theme::get();
    let h = h_parts(g, h_total(g, project));
    paint_bars(p, &t, g, &h, &v_parts(g, content, scroll_y));
}

/// The scrollbars (Windows 2000 / Vegas 6): flat grey tracks, raised chrome arrow buttons with black
/// triangles, a chrome thumb with two grip lines, chrome + / − / magnifier buttons.
fn paint_bars(p: &Painter, t: &crate::ui::theme::Theme, g: &Geo, h: &HParts, v: &VParts) {
    use crate::ui::widgets::{Chrome, arrow, chrome_button};
    let (m, ink) = (&t.metal, t.scroll_arrow);
    let btn = |r: Rect| chrome_button(p, r, Chrome::Raised, m);
    let plus = |r: Rect| {
        let c = r.center().floor();
        p.rect_filled(Rect::from_min_size(pos2(c.x - 3.0, c.y), vec2(7.0, 1.0)), 0.0, ink);
        p.rect_filled(Rect::from_min_size(pos2(c.x, c.y - 3.0), vec2(1.0, 7.0)), 0.0, ink);
    };
    let minus = |r: Rect| {
        let c = r.center().floor();
        p.rect_filled(Rect::from_min_size(pos2(c.x - 3.0, c.y), vec2(7.0, 1.0)), 0.0, ink);
    };
    // Horizontal bar.
    p.rect_filled(g.hscroll, 0.0, t.scroll_track);
    btn(h.left);
    arrow(p, h.left, (-1.0, 0.0), ink);
    btn(h.right);
    arrow(p, h.right, (1.0, 0.0), ink);
    // Thumbs: chrome pills with three grip ridges (UI_SPEC.md §5 Scrollbar).
    let pill = |r: Rect| crate::ui::widgets::chrome(p, r.shrink(1.0), (r.width().min(r.height()) * 0.5 - 1.0).max(0.0), Chrome::Raised);
    pill(h.thumb);
    if h.thumb.width() >= 18.0 {
        let c = h.thumb.center().floor();
        for dx in [-4.0, -1.0, 2.0] {
            p.rect_filled(Rect::from_min_size(pos2(c.x + dx, h.thumb.min.y + 4.0), vec2(1.0, h.thumb.height() - 8.0)), 0.0, m.shadow);
            p.rect_filled(Rect::from_min_size(pos2(c.x + dx + 1.0, h.thumb.min.y + 4.0), vec2(1.0, h.thumb.height() - 8.0)), 0.0, m.light);
        }
    }
    btn(h.plus);
    plus(h.plus);
    btn(h.minus);
    minus(h.minus);
    btn(h.zoom);
    paint_icon(p, h.zoom.shrink(2.0), Icon::Search, Some(ink));

    // Vertical bar.
    crate::ui::widgets::metal(p, g.vscroll, m);
    p.rect_filled(Rect::from_min_max(pos2(g.vscroll.min.x, g.lanes.min.y), pos2(g.vscroll.max.x, g.lanes.max.y)), 0.0, t.scroll_track);
    btn(v.corner);
    paint_icon(p, v.corner.shrink(2.0), Icon::InsertMarker, None);
    pill(v.thumb);
    if v.thumb.height() >= 18.0 {
        let c = v.thumb.center().floor();
        for dy in [-4.0, -1.0, 2.0] {
            p.rect_filled(Rect::from_min_size(pos2(v.thumb.min.x + 4.0, c.y + dy), vec2(v.thumb.width() - 8.0, 1.0)), 0.0, m.shadow);
            p.rect_filled(Rect::from_min_size(pos2(v.thumb.min.x + 4.0, c.y + dy + 1.0), vec2(v.thumb.width() - 8.0, 1.0)), 0.0, m.light);
        }
    }
    btn(v.plus);
    plus(v.plus);
    btn(v.minus);
    minus(v.minus);
}
