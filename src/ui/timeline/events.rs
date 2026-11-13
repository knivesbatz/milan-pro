//! Events on the lanes (§5.9.4): VP16 header bar + track-colour body, VP23 name label + bottom-right chips;
//! filmstrip thumbnails, waveforms, fade curves and handles, selection outline.
use eframe::egui::*;
use crate::audio::PEAKS_PER_SEC;
use crate::edit;
use crate::project::Clip;
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
    /// Auto Ripple is on: the selected outline is red.
    pub ripple: bool,
    /// Level handle height (gain amplitude or opacity, 0..=1); None when hidden.
    pub level: Option<f32>,
    /// Muted track or event: drawn at 50 % over the lane.
    pub dim: bool,
    /// The event's media is generated: it gets the Generated Media button.
    pub generated: bool,
    pub pps: f64,
    pub fps: f64,
    /// Waveform vertical zoom (1 = full scale).
    pub wave_zoom: f32,
    /// The media length a looping event repeats (`edit::loop_len`): wraps the pictures and waveform, notches the edge.
    pub loop_len: Option<f64>,
    /// Where the media starts in its file (a subclip's start): waveform peaks are the file's.
    pub offset: f64,
    /// An empty event (no media): drawn hatched.
    pub empty: bool,
}

impl Ev<'_> {
    /// Source time at timeline time `t`, wrapped into the media when the event loops.
    fn src(&self, t: f64) -> f64 {
        let s = edit::src_time(self.clip, t);
        self.loop_len.map_or(s, |l| edit::loop_time(s, l))
    }
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
    if ev.empty {
        paint_hatch(p, r, mul(ev.color, 0.7));
    }
    paint_notches(p, ev, lane_bg);
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
    paint_level(p, t, ev);
    if ev.selected {
        let c = if ev.ripple { t.event_selected_border_ripple } else { t.event_selected_border };
        p.rect_stroke(r, 0.0, Stroke::new(2.0_f32, c), StrokeKind::Inside);
    }
    if ev.dim {
        p.rect_filled(r, 0.0, lane_bg.gamma_multiply(0.5));
    }
}

/// Timeline times where a looping event's media wraps (its loop notches).
fn notches(c: &Clip, len: f64) -> Vec<f64> {
    edit::loop_passes(c, Some(len)).into_iter().skip(1).map(|p| p.0).collect()
}

/// Loop notches: a small V cut into the top edge where the media starts again (none closer than 4 px apart).
fn paint_notches(p: &Painter, ev: &Ev, lane_bg: Color32) {
    // shortcut: notches are placed at the plain rate, so an event with a Velocity envelope gets none; integrate the
    // envelope to place them if users ask.
    let Some(len) = ev.loop_len.filter(|l| l / ev.clip.rate.max(1e-9) * ev.pps >= 4.0 && crate::engine::velocity::of(ev.clip).is_none()) else { return };
    let (r, vis) = (ev.rect, p.clip_rect());
    let p = p.with_clip_rect(r.intersect(vis));
    for t in notches(ev.clip, len) {
        let x = (r.min.x as f64 + (t - ev.clip.position) * ev.pps) as f32;
        if x >= vis.min.x - 4.0 && x <= vis.max.x + 4.0 {
            p.add(Shape::convex_polygon(vec![pos2(x - 4.0, r.min.y), pos2(x + 4.0, r.min.y), pos2(x, r.min.y + 5.0)], lane_bg, Stroke::NONE));
        }
    }
}

/// Diagonal hatching over an empty event.
fn paint_hatch(p: &Painter, r: Rect, ink: Color32) {
    let vis = r.intersect(p.clip_rect());
    let p = p.with_clip_rect(vis);
    let h = r.height();
    let mut x = r.min.x + ((vis.min.x - h - r.min.x) / 8.0).floor().max(0.0) * 8.0;
    while x < vis.max.x {
        p.line_segment([pos2(x, r.max.y), pos2(x + h, r.min.y)], Stroke::new(1.0_f32, ink));
        x += 8.0;
    }
}

/// Width of one filmstrip frame in an event `h` px tall (`paint_filmstrip`'s cell).
pub(super) fn thumb_w(t: &Theme, h: f32, aspect: f32) -> f32 {
    let area_h = if t.look() == UiLook::Vegas16 { h - header_h(t, h) } else { h };
    ((area_h - 1.0) * aspect).round().max(4.0)
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
    // Dimmed by the event's opacity.
    let tint = Color32::from_gray((255.0 * ev.clip.opacity().clamp(0.0, 1.0)).round() as u8);
    for k in k0..=k1 {
        let x = area.min.x + step * k as f32;
        let tr = Rect::from_min_size(pos2(x, area.min.y), vec2(w.min(area.max.x - x), h));
        // Frame-quantised source time at the thumbnail's left edge (with the event's rate and direction). Reversed, a
        // frame samples where it ends, as the engine does, so the first tile is not one frame past the source span.
        let t = ev.clip.position + (x - area.min.x) as f64 / ev.pps;
        let src = ev.src(if ev.clip.reverse { t + frame } else { t }).max(0.0);
        let src = (src / frame).floor() * frame;
        if let Some(tex) = thumb(src) {
            let uv = Rect::from_min_max(Pos2::ZERO, pos2((tr.width() / w).min(1.0), 1.0));
            p.image(tex, tr, uv, tint);
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
    // Scaled by the event gain.
    let gain = crate::engine::keyframes::db_to_gain(ev.clip.gain_db) * ev.wave_zoom.max(1.0);
    let src = |x: f32| ev.offset + ev.src(ev.clip.position + (x - area.min.x) as f64 * spp);
    let mut x = vis.min.x.floor();
    while x < vis.max.x {
        // The source span under this pixel column (read backwards when reversed); a column across a loop wrap reads
        // from its start only, not the whole media between.
        let (s0, mut s1) = (src(x), src(x + 1.0));
        if (s1 - s0).abs() > 2.0 * spp * ev.clip.rate {
            s1 = s0;
        }
        let a = (s0.min(s1) * PEAKS_PER_SEC).max(0.0) as usize;
        let z = ((s0.max(s1) * PEAKS_PER_SEC) as usize).max(a + 1).min(peaks.len());
        let pk = peaks.get(a..z).map_or(0.0, |s| s.iter().fold(0.0_f32, |m, v| m.max(*v))) * gain;
        if pk > 0.0 {
            let y = (pk.min(1.0) * half).max(0.5);
            p.rect_filled(Rect::from_min_max(pos2(x, mid - y), pos2(x + 1.0, mid + y)), 0.0, ink);
        }
        x += 1.0;
    }
}

/// Fade curves (in their Fade Type shape) from the bottom corners up to the top at the fade lengths; the area above
/// the curve is darkened. VP16: 1 px blue (white when selected); VP23: 2 px white.
fn paint_fades(p: &Painter, t: &Theme, ev: &Ev, area: Rect) {
    let v16 = t.look() == UiLook::Vegas16;
    let curve = if v16 && !ev.selected { t.fade_curve } else if v16 { t.fade_curve_selected } else { t.fade_curve };
    let stroke = Stroke::new(if v16 { 1.0_f32 } else { 2.0_f32 }, curve);
    let fades = [(ev.clip.fade_in, false, ev.clip.fade_in_curve), (ev.clip.fade_out, true, ev.clip.fade_out_curve)];
    for (len, out, shape) in fades {
        let w = (len * ev.pps) as f32;
        if w < 1.0 {
            continue;
        }
        let x0 = if out { area.max.x - w } else { area.min.x };
        let n = (w / 3.0).clamp(4.0, 48.0) as usize;
        let pts: Vec<Pos2> = (0..=n)
            .map(|i| {
                let f = i as f32 / n as f32;
                // The gain the mixer and compositor use (`engine::plan::fade`): a fade-out runs its curve backwards.
                let g = shape.shape(if out { 1.0 - f as f64 } else { f as f64 }) as f32;
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

/// The event's buttons where they sit in event rect `r`, right to left (⋯, fx, Pan/Crop on video, Generated Media on
/// generated events): VP16 16 px glyphs on an 18 px pitch in the header bar, VP23 20×20 chips at the bottom right.
/// Ones that don't fit are left out.
pub(super) fn buttons(t: &Theme, r: Rect, is_video: bool, generated: bool) -> Vec<(Icon, Rect)> {
    let mut icons = vec![Icon::More, Icon::TrackFx];
    if is_video {
        icons.push(Icon::PanCrop);
    }
    if generated {
        icons.push(Icon::GeneratedMedia);
    }
    if t.look() == UiLook::Vegas16 {
        let hh = header_h(t, r.height());
        let bar = Rect::from_min_max(r.min, pos2(r.max.x, r.min.y + hh));
        icons.truncate(if hh >= 16.0 { ((bar.width() - 28.0) / 18.0).floor().max(0.0) as usize } else { 0 });
        let at = |k: usize| Rect::from_center_size(pos2(bar.max.x - 2.0 - 18.0 * (k + 1) as f32 + 9.0, bar.center().y), vec2(16.0, 16.0));
        icons.into_iter().enumerate().map(|(k, i)| (i, at(k))).collect()
    } else {
        if r.height() < 46.0 || r.width() < 22.0 * icons.len() as f32 + 30.0 {
            icons.truncate(if r.height() >= 24.0 && r.width() >= 60.0 { 1 } else { 0 });
        }
        let at = |k: usize| Rect::from_min_size(pos2(r.max.x - 2.0 - 20.0 - 22.0 * k as f32, r.max.y - 2.0 - 20.0), vec2(20.0, 20.0));
        icons.into_iter().enumerate().map(|(k, i)| (i, at(k))).collect()
    }
}

/// VP16 header bar: name at x+4 (white, 12 px, truncated) and right-aligned glyph buttons on an 18 px pitch.
fn paint_header16(p: &Painter, t: &Theme, ev: &Ev, bar: Rect) {
    let icons = buttons(t, ev.rect, ev.is_video, ev.generated);
    let pi = p.with_clip_rect(bar.intersect(p.clip_rect()));
    for (icon, r) in &icons {
        paint_icon(&pi, *r, *icon, t.look(), None);
        fx_dot(&pi, t, ev, *icon, *r);
    }
    let name_w = bar.max.x - 2.0 - 18.0 * icons.len() as f32 - bar.min.x - 6.0;
    if name_w > 8.0 {
        paint_name(&pi, ev, pos2(bar.min.x + 4.0, bar.center().y), name_w, t.font(t.font.event_name), t.event_text);
    }
}

/// The event's name, with "◄" when reversed and "(rate 150%)" when not at normal speed.
fn label(c: &Clip) -> String {
    let mut s = if c.reverse { format!("◄ {}", c.name) } else { c.name.clone() };
    if (c.rate - 1.0).abs() > 1e-9 {
        s += &format!(" (rate {:.0}%)", c.rate * 100.0);
    }
    s
}

fn paint_name(p: &Painter, ev: &Ev, left_center: Pos2, w: f32, font: FontId, color: Color32) -> Vec2 {
    let mut job = text::LayoutJob::simple_singleline(label(ev.clip), font, color);
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
        let galley = p.layout_no_wrap(label(ev.clip), font, t.event_text);
        let bw = (galley.size().x + 8.0).min(r.width());
        let b = Rect::from_min_size(r.min, vec2(bw, 20.0));
        pr.rect_filled(b, 0.0, t.event_label_bg);
        pr.rect_stroke(b, 0.0, Stroke::new(1.0_f32, t.event_label_border), StrokeKind::Inside);
        pr.with_clip_rect(b.shrink(1.0).intersect(pr.clip_rect())).galley(pos2(b.min.x + 4.0, b.center().y - galley.size().y * 0.5), galley, t.event_text);
    }
    for (icon, c) in buttons(t, r, ev.is_video, ev.generated) {
        pr.rect_filled(c, CornerRadius::same(2), t.event_chip_bg);
        paint_icon(&pr, c.shrink(2.0), icon, t.look(), Some(t.text_bright));
        fx_dot(&pr, t, ev, icon, c);
    }
}

/// The fx button's accent dot while the event has FX.
fn fx_dot(p: &Painter, t: &Theme, ev: &Ev, icon: Icon, r: Rect) {
    if icon == Icon::TrackFx && !ev.clip.fx.is_empty() {
        p.circle_filled(r.right_top() + vec2(-2.5, 2.5), 2.5, t.accent);
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

/// Level line (gain or opacity) across the event while it is hovered or below full level, with the 12×5 pill handle
/// on it while hovered (§5.9.4).
fn paint_level(p: &Painter, t: &Theme, ev: &Ev) {
    let Some(f) = ev.level.filter(|f| ev.hovered || *f < 1.0) else { return };
    let r = ev.rect;
    let y = (r.min.y + (1.0 - f) * r.height()).round().min(r.max.y - 1.0);
    let pr = p.with_clip_rect(r.intersect(p.clip_rect()));
    pr.rect_filled(Rect::from_x_y_ranges(r.x_range(), Rangef::new(y, y + 1.0)), 0.0, t.fade_handle);
    if ev.hovered && r.width() >= 40.0 {
        pr.rect_filled(Rect::from_center_size(pos2(r.center().x, y + 2.0), vec2(12.0, 5.0)), CornerRadius::same(2), t.fade_handle);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// R3a: an 8 s clip extended to 20 s is notched at 8 and 16 s, where its media starts again; reversed, where the
    /// source wraps going backwards.
    #[test]
    fn loop_notches_sit_where_the_media_wraps() {
        let mut c = Clip::new("c", 0.0, 20.0);
        assert_eq!(notches(&c, 8.0), [8.0, 16.0]);
        c.position = 2.0;
        c.in_point = 3.0; // 3..8, then 0..8, then 0..7
        assert_eq!(notches(&c, 8.0), [7.0, 15.0]);
        c.reverse = true; // 7..0, 8..0, 8..3 from the left
        assert_eq!(notches(&c, 8.0), [9.0, 17.0]);
        c.duration = 4.0;
        assert!(notches(&c, 8.0).is_empty());
    }
}
