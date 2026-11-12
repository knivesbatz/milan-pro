//! Events on the lanes (§5.9.4): VP16 header bar + track-colour body, VP23 name label + bottom-right chips;
//! filmstrip thumbnails, waveforms, fade curves and handles, selection outline.
use eframe::egui::*;
use crate::audio::PEAKS_PER_SEC;
use crate::project::{Clip, smooth_fade};
use crate::ui::icons::{Icon, paint_icon};
use crate::ui::theme::{Theme, UiLook};

pub(super) struct Ev<'a> {
    /// Whole event, [start, end] × [lane top + 1, lane bottom − 2]; may extend past the visible lanes.
    pub rect: Rect,
    pub clip: &'a Clip,
    pub color: Color32,
    pub is_video: bool,
    pub still: bool,
    /// Source width / height for the filmstrip.
    pub aspect: f32,
    pub selected: bool,
    pub hovered: bool,
    /// Muted track: drawn at 50 % over the lane.
    pub dim: bool,
    pub pps: f64,
    pub fps: f64,
}

fn mul(c: Color32, k: f32) -> Color32 {
    let f = |v: u8| (v as f32 * k).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(f(c.r()), f(c.g()), f(c.b()))
}

/// VP16 header bar height: 22 px, 16 px under 40 px events, none under 24 px.
fn header_h(t: &Theme, h: f32) -> f32 {
    if t.look() != UiLook::Vegas16 {
        0.0
    } else if h >= 40.0 {
        t.m.event_header_h
    } else if h >= 24.0 {
        16.0
    } else {
        0.0
    }
}

/// `wave`: peaks of the source (PEAKS_PER_SEC per second). `thumb(source time)`: a decoded thumbnail, if ready.
pub(super) fn paint_event(
    p: &Painter,
    t: &Theme,
    ev: &Ev,
    lane_bg: Color32,
    wave: Option<&[Vec<f32>]>,
    thumb: &mut dyn FnMut(f64) -> Option<TextureId>,
) {
    let v16 = t.look() == UiLook::Vegas16;
    let r = ev.rect;
    let vis = r.intersect(p.clip_rect());
    if vis.width() <= 0.0 || vis.height() <= 0.0 {
        return;
    }
    let hh = header_h(t, r.height());
    let body = r.with_min_y(r.min.y + hh);
    let base = if !v16 && !ev.is_video { mul(ev.color, 0.5) } else { ev.color };
    p.rect_filled(r, 0.0, base);

    if ev.is_video {
        paint_filmstrip(p, ev, v16, if v16 { body } else { r }, thumb);
    } else if let Some(chans) = wave {
        let ink = if v16 { ev.color.lerp_to_gamma(Color32::WHITE, 0.55) } else { ev.color };
        // One lane per channel, stacked, each around its own centre line.
        let area = body.shrink2(vec2(0.0, 2.0));
        let lane_h = area.height() / chans.len() as f32;
        for (k, peaks) in chans.iter().enumerate() {
            let lane = Rect::from_x_y_ranges(area.x_range(), Rangef::new(area.min.y + lane_h * k as f32, area.min.y + lane_h * (k + 1) as f32));
            if chans.len() > 1 {
                let line = Rect::from_x_y_ranges(lane.x_range(), Rangef::new(lane.center().y.round(), lane.center().y.round() + 1.0));
                p.rect_filled(line.intersect(p.clip_rect()), 0.0, mul(ev.color, if v16 { 0.75 } else { 0.8 }));
            }
            paint_wave(p, ev, lane.shrink2(vec2(0.0, 1.0)), peaks, ink);
        }
    }
    if r.width() >= 3.0 {
        p.rect_filled(Rect::from_x_y_ranges(r.x_range(), Rangef::new(r.max.y - 1.0, r.max.y)), 0.0, mul(ev.color, 0.73));
    }
    paint_fades(p, t, ev, if v16 { body } else { r });

    if v16 {
        if hh > 0.0 {
            paint_header16(p, t, ev, Rect::from_min_max(r.min, pos2(r.max.x, r.min.y + hh)));
        }
    } else {
        paint_label23(p, t, ev);
    }
    if ev.hovered {
        paint_handles(p, t, ev, if v16 { body } else { r });
    }
    if ev.selected {
        p.rect_stroke(r, 0.0, Stroke::new(2.0_f32, t.event_selected_border), StrokeKind::Inside);
    }
    if ev.dim {
        p.rect_filled(r, 0.0, lane_bg.gamma_multiply(0.5));
    }
}

/// Filmstrip: as many unstretched frames as fit, abutting from the start; the leftover at the end shows the track
/// colour. Stills show one image at the start.
fn paint_filmstrip(p: &Painter, ev: &Ev, v16: bool, area: Rect, thumb: &mut dyn FnMut(f64) -> Option<TextureId>) {
    let h = area.height() - 1.0;
    if h < 8.0 {
        return;
    }
    let w = (h * ev.aspect).round().max(4.0);
    let ew = area.width();
    // VP16: contiguous whole frames from the start, the leftover shows the track colour, stills show one image.
    // VP23: frames (stills too) repeat across the full width, the last one cropped.
    let n = match (v16, ev.still) {
        (true, true) => 1,
        (true, false) => ((ew / w).floor() as i64).max(1),
        (false, _) => ((ew / w).ceil() as i64).max(1),
    };
    let step = w;
    let vis = p.clip_rect();
    let (k0, k1) = if n > 1 {
        (((vis.min.x - w - area.min.x) / step).floor().max(0.0) as i64, (((vis.max.x - area.min.x) / step).ceil() as i64).min(n - 1))
    } else {
        (0, 0)
    };
    let frame = 1.0 / ev.fps.max(1.0);
    let p = p.with_clip_rect(area.intersect(vis));
    for k in k0..=k1 {
        let x = area.min.x + step * k as f32;
        let tr = Rect::from_min_size(pos2(x, area.min.y), vec2(w.min(area.max.x - x), h));
        // Frame-quantised source time at the thumbnail's left edge.
        let src = ev.clip.in_point + (x - area.min.x) as f64 / ev.pps;
        let src = (src / frame).floor() * frame;
        if let Some(tex) = thumb(src) {
            let uv = Rect::from_min_max(Pos2::ZERO, pos2((tr.width() / w).min(1.0), 1.0));
            p.image(tex, tr, uv, Color32::WHITE);
        }
    }
}

fn paint_wave(p: &Painter, ev: &Ev, area: Rect, peaks: &[f32], ink: Color32) {
    let vis = area.intersect(p.clip_rect());
    if vis.width() <= 0.0 || area.height() < 4.0 {
        return;
    }
    let (mid, half) = (area.center().y, area.height() * 0.5);
    let spp = 1.0 / ev.pps;
    let mut x = vis.min.x.floor();
    while x < vis.max.x {
        let t = ev.clip.in_point + (x - area.min.x) as f64 * spp;
        let a = (t * PEAKS_PER_SEC).max(0.0) as usize;
        let z = (((t + spp) * PEAKS_PER_SEC) as usize).max(a + 1).min(peaks.len());
        let pk = peaks.get(a..z).map_or(0.0, |s| s.iter().fold(0.0_f32, |m, v| m.max(*v)));
        if pk > 0.0 {
            let y = (pk * half).max(0.5);
            p.rect_filled(Rect::from_min_max(pos2(x, mid - y), pos2(x + 1.0, mid + y)), 0.0, ink);
        }
        x += 1.0;
    }
}

/// Fade curves (smooth S) from the bottom corners up to the top at the fade lengths; the area above the curve is
/// darkened. VP16: 1 px blue (white when selected); VP23: 2 px white.
fn paint_fades(p: &Painter, t: &Theme, ev: &Ev, area: Rect) {
    let v16 = t.look() == UiLook::Vegas16;
    let curve = if v16 && !ev.selected { t.fade_curve } else if v16 { t.fade_curve_selected } else { t.fade_curve };
    let stroke = Stroke::new(if v16 { 1.0_f32 } else { 2.0_f32 }, curve);
    let fades = [(ev.clip.fade_in, false), (ev.clip.fade_out, true)];
    for (len, out) in fades {
        let w = (len * ev.pps) as f32;
        if w < 1.0 {
            continue;
        }
        let x0 = if out { area.max.x - w } else { area.min.x };
        let n = (w / 3.0).clamp(4.0, 48.0) as usize;
        let pts: Vec<Pos2> = (0..=n)
            .map(|i| {
                let f = i as f32 / n as f32;
                let g = smooth_fade(f as f64) as f32;
                let g = if out { 1.0 - g } else { g };
                pos2(x0 + f * w, area.max.y - g * area.height())
            })
            .collect();
        let mut mesh = Mesh::default();
        for (i, q) in pts.iter().enumerate() {
            mesh.colored_vertex(pos2(q.x, area.min.y), t.fade_region_tint);
            mesh.colored_vertex(*q, t.fade_region_tint);
            if i > 0 {
                let k = (i * 2) as u32;
                mesh.add_triangle(k - 2, k - 1, k);
                mesh.add_triangle(k - 1, k, k + 1);
            }
        }
        p.add(mesh);
        p.add(Shape::line(pts, stroke));
    }
}

/// VP16 header bar: name at x+4 (white, 12 px, truncated) and right-aligned glyph buttons on an 18 px pitch.
fn paint_header16(p: &Painter, t: &Theme, ev: &Ev, bar: Rect) {
    let mut icons = vec![Icon::More, Icon::TrackFx];
    if ev.is_video {
        icons.push(Icon::PanCrop);
    }
    let fit = (((bar.width() - 28.0) / 18.0).floor().max(0.0) as usize).min(icons.len());
    let icons = &icons[..if bar.height() >= 16.0 { fit } else { 0 }];
    let mut x = bar.max.x - 2.0;
    let pi = p.with_clip_rect(bar.intersect(p.clip_rect()));
    for icon in icons {
        x -= 18.0;
        let r = Rect::from_center_size(pos2(x + 9.0, bar.center().y), vec2(16.0, 16.0));
        paint_icon(&pi, r, *icon, t.look(), None);
    }
    let name_w = x - bar.min.x - 6.0;
    if name_w > 8.0 {
        paint_name(&pi, ev, pos2(bar.min.x + 4.0, bar.center().y), name_w, t.font(t.font.event_name), t.event_text);
    }
}

fn paint_name(p: &Painter, ev: &Ev, left_center: Pos2, w: f32, font: FontId, color: Color32) -> Vec2 {
    let mut job = text::LayoutJob::simple_singleline(ev.clip.name.clone(), font, color);
    job.wrap = text::TextWrapping::truncate_at_width(w);
    let galley = p.layout_job(job);
    let size = galley.size();
    p.galley(pos2(left_center.x, left_center.y - size.y * 0.5), galley, color);
    size
}

/// VP23: name label box at the top-left and the 20×20 chips at the bottom-right.
fn paint_label23(p: &Painter, t: &Theme, ev: &Ev) {
    let r = ev.rect;
    let pr = p.with_clip_rect(r.intersect(p.clip_rect()));
    if r.height() >= 20.0 && r.width() >= 16.0 {
        let font = t.font(t.font.event_name);
        let galley = p.layout_no_wrap(ev.clip.name.clone(), font, t.event_text);
        let bw = (galley.size().x + 8.0).min(r.width());
        let b = Rect::from_min_size(r.min, vec2(bw, 20.0));
        pr.rect_filled(b, 0.0, t.event_label_bg);
        pr.rect_stroke(b, 0.0, Stroke::new(1.0_f32, t.event_label_border), StrokeKind::Inside);
        pr.with_clip_rect(b.shrink(1.0).intersect(pr.clip_rect())).galley(pos2(b.min.x + 4.0, b.center().y - galley.size().y * 0.5), galley, t.event_text);
    }
    let mut chips = vec![Icon::More, Icon::TrackFx];
    if ev.is_video {
        chips.push(Icon::PanCrop);
    }
    if r.height() < 46.0 || r.width() < 22.0 * chips.len() as f32 + 30.0 {
        chips.truncate(if r.height() >= 24.0 && r.width() >= 60.0 { 1 } else { 0 });
    }
    for (k, icon) in chips.iter().enumerate() {
        let c = Rect::from_min_size(pos2(r.max.x - 2.0 - 20.0 - 22.0 * k as f32, r.max.y - 2.0 - 20.0), vec2(20.0, 20.0));
        pr.rect_filled(c, CornerRadius::same(2), t.event_chip_bg);
        paint_icon(&pr, c.shrink(2.0), *icon, t.look(), Some(t.text_bright));
    }
}

/// Hover handles: fade handles at the top corners (VP16 blue quarter-arcs, VP23 cyan 7×7 triangles) and the
/// VP23 "II" trim grips at mid-height.
fn paint_handles(p: &Painter, t: &Theme, ev: &Ev, area: Rect) {
    let r = ev.rect;
    let pr = p.with_clip_rect(r.intersect(p.clip_rect()));
    let xi = (area.min.x + (ev.clip.fade_in * ev.pps) as f32).min(r.max.x);
    let xo = (area.max.x - (ev.clip.fade_out * ev.pps) as f32).max(r.min.x);
    if t.look() == UiLook::Vegas16 {
        let arc = |c: Pos2, a0: f32, a1: f32| {
            let pts: Vec<Pos2> = (0..=8).map(|i| {
                let a = (a0 + (a1 - a0) * i as f32 / 8.0).to_radians();
                c + vec2(a.cos(), a.sin()) * 5.0
            }).collect();
            pr.add(Shape::line(pts, Stroke::new(2.0_f32, t.fade_handle)));
        };
        arc(pos2(xi, r.min.y), 0.0, 90.0);
        arc(pos2(xo, r.min.y), 90.0, 180.0);
    } else {
        let y = r.min.y;
        pr.add(Shape::convex_polygon(vec![pos2(xi, y), pos2(xi + 7.0, y), pos2(xi, y + 7.0)], t.fade_handle, Stroke::NONE));
        pr.add(Shape::convex_polygon(vec![pos2(xo - 7.0, y), pos2(xo, y), pos2(xo, y + 7.0)], t.fade_handle, Stroke::NONE));
        if r.height() >= 18.0 {
            for x in [r.min.x + 1.0, r.max.x - 7.0] {
                let g = Rect::from_min_size(pos2(x, r.center().y - 7.0), vec2(6.0, 14.0));
                pr.rect_filled(g, 0.0, t.event_chip_bg);
                for dx in [1.5, 3.5] {
                    pr.rect_filled(Rect::from_min_size(pos2(g.min.x + dx, g.min.y + 3.0), vec2(1.0, 8.0)), 0.0, t.text_bright);
                }
            }
        }
    }
}

/// Length tooltip shown while dragging a fade: white box, dark text (§5.9.4).
pub(super) fn paint_fade_tag(p: &Painter, t: &Theme, at: Pos2, text: String) {
    let galley = p.layout_no_wrap(text, t.font(11.0), t.length_tag_text);
    let r = Rect::from_min_size(at, galley.size() + vec2(6.0, 2.0));
    p.rect_filled(r, 0.0, t.length_tag_bg);
    p.rect_stroke(r, 0.0, Stroke::new(1.0_f32, t.length_tag_text), StrokeKind::Inside);
    p.galley(r.min + vec2(3.0, 1.0), galley, t.length_tag_text);
}
