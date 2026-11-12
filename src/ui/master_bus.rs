//! Master Bus window (§5.12): toolbar, strip header, peak readouts, two live meters with a 3 dB scale,
//! and the master faders (VP16 two "88" thumbs, VP23 one linked "II" thumb) that set the playback gain.
use eframe::egui::*;
use crate::audio::Master;
use super::icons::{Icon, paint_icon};
use super::theme::{self, UiLook};
use super::timeline::{fader_gain, fader_pos};
use super::toolbar::inert_button;

/// Meter range: 0 dB at the top of the wells, −60 dB at the bottom.
const RANGE_DB: f32 = 60.0;
/// Fall-back speed of the bars and of a released peak hold.
const FALL_DB_PER_S: f32 = 24.0;
const HOLD_S: f64 = 2.0;

pub struct MasterView {
    /// L/R fader gains (linear); linked moves both.
    pub gain: [f32; 2],
    pub linked: bool,
    /// Displayed bar level and peak hold per channel, in dB.
    level: [f32; 2],
    hold: [(f32, f64); 2],
    /// Highest level since the last reset (the readouts at the top), and the latched clip lamps.
    max: [f32; 2],
    clip: [bool; 2],
}

impl Default for MasterView {
    fn default() -> Self {
        Self { gain: [1.0; 2], linked: true, level: [-f32::INFINITY; 2], hold: [(-f32::INFINITY, 0.0); 2], max: [-f32::INFINITY; 2], clip: [false; 2] }
    }
}

fn db(v: f32) -> f32 {
    if v <= 1e-6 { -f32::INFINITY } else { 20.0 * v.log10() }
}

impl MasterView {
    /// Fold in the peaks played since the last frame (`dt` seconds ago).
    fn feed(&mut self, peaks: [f32; 2], now: f64, dt: f32) {
        for c in 0..2 {
            let d = db(peaks[c]);
            self.level[c] = d.max(self.level[c] - FALL_DB_PER_S * dt);
            let (h, at) = self.hold[c];
            self.hold[c] = if d >= h {
                (d, now)
            } else if now - at > HOLD_S {
                ((h - FALL_DB_PER_S * dt).max(d), at)
            } else {
                (h, at)
            };
            self.max[c] = self.max[c].max(d);
            self.clip[c] |= peaks[c] >= 1.0;
        }
    }

    fn reset(&mut self) {
        self.max = [-f32::INFINITY; 2];
        self.clip = [false; 2];
    }

    fn active(&self) -> bool {
        self.level.iter().chain(self.hold.iter().map(|h| &h.0)).any(|&l| l > -RANGE_DB)
    }
}

/// Master Bus toolbar: properties, downmix, dim, mixing console (not built yet: disabled).
pub fn master_bus_toolbar(ui: &mut Ui) {
    inert_button(ui, Icon::Properties, "Master Bus Properties (not available yet)", false);
    inert_button(ui, Icon::Downmix, "Downmix Output (not available yet)", false);
    inert_button(ui, Icon::Dim, "Dim Output (not available yet)", false);
    inert_button(ui, Icon::MixingConsole, "Mixing Console (not available yet)", false);
}

pub fn master_bus(ui: &mut Ui, view: &mut MasterView, master: &Master) {
    let t = theme::get();
    let v16 = t.look() == UiLook::Vegas16;
    let (now, dt) = ui.input(|i| (i.time, i.stable_dt.min(0.1)));
    view.feed(master.take_peaks(), now, dt);
    if view.active() {
        ui.ctx().request_repaint();
    }

    let r = ui.max_rect();
    let p = ui.painter().clone();
    let ink = t.text;
    let head = Rect::from_min_size(r.min + vec2(8.0, 6.0), vec2(10.0, 10.0));
    p.rect_stroke(head, 0.0, Stroke::new(1.0_f32, ink), StrokeKind::Inside);
    p.text(pos2(head.max.x + 6.0, head.center().y), Align2::LEFT_CENTER, "Master", t.font(t.font.ui), ink);
    // VP16 strip row: fx · automation badge · M · S.
    let strip: &[Option<Icon>] = if v16 { &[Some(Icon::TrackFx), None, Some(Icon::Mute), Some(Icon::Solo)] } else { &[Some(Icon::TrackFx), Some(Icon::Mute), Some(Icon::Solo)] };
    for (i, icon) in strip.iter().enumerate() {
        let c = Rect::from_min_size(pos2(r.min.x + 6.0 + 20.0 * i as f32, r.min.y + 22.0), vec2(16.0, 16.0));
        match icon {
            Some(icon) => paint_icon(&p, c, *icon, t.look(), None),
            None => automation_badge(&p, c),
        }
    }

    let cx = r.center().x.round();
    let wells = [cx - 26.0, cx + 16.0];
    let font = t.font(t.font.meter);
    // Peak readouts; a click resets them and the clip lamps.
    let ready = r.min.y + 44.0;
    for c in 0..2 {
        let text = if view.max[c] > -RANGE_DB { format!("{:.1}", view.max[c]) } else { "-Inf.".into() };
        p.text(pos2(wells[c] + 5.0, ready), Align2::CENTER_CENTER, text, font.clone(), t.text);
    }
    let read_hit = Rect::from_x_y_ranges(Rangef::new(wells[0] - 14.0, wells[1] + 24.0), Rangef::new(ready - 7.0, ready + 7.0));
    if ui.interact(read_hit, ui.id().with("master_peaks"), Sense::click()).on_hover_text("Click to reset the peaks").clicked() {
        view.reset();
    }

    let top = r.min.y + 54.0;
    // VP16 keeps a light native horizontal scrollbar under the strip.
    let sb_h = if v16 { 17.0 } else { 0.0 };
    let bottom = r.max.y - 26.0 - sb_h;
    if v16 {
        native_hscroll(&p, Rect::from_min_max(pos2(r.min.x, r.max.y - sb_h), r.max));
    }
    if bottom - top < 40.0 {
        return;
    }
    let to_y = |d: f32| top + (bottom - top) * (-d / RANGE_DB).clamp(0.0, 1.0);
    for (c, &x) in wells.iter().enumerate() {
        let well = Rect::from_min_max(pos2(x, top), pos2(x + 10.0, bottom));
        if t.meter_unlit > 0.0 {
            paint_bar(&p, well, well.min.y, &to_y, false, t.meter_unlit);
        } else {
            p.rect_filled(well, 0.0, t.meter_bg);
        }
        let lvl = view.level[c];
        if lvl > -RANGE_DB {
            paint_bar(&p, well, to_y(lvl.min(0.0)), &to_y, v16, 1.0);
        }
        let (h, _) = view.hold[c];
        if h > -RANGE_DB {
            let y = to_y(h.min(0.0)).round();
            p.rect_filled(Rect::from_min_max(pos2(x, y), pos2(x + 10.0, y + 1.0)), 0.0, if h >= -6.0 { t.meter_yellow } else { t.meter_green });
        }
        if view.clip[c] {
            p.rect_filled(Rect::from_min_size(well.min, vec2(10.0, 4.0)), 0.0, t.meter_red);
        }
        if ui.interact(well, ui.id().with(("master_well", c)), Sense::click()).clicked() {
            view.reset();
        }
    }
    let step = (bottom - top) / 20.0;
    for k in 1..=19 {
        let y = (top + step * k as f32).round();
        let label = if v16 { format!("-{}", 3 * k) } else { (3 * k).to_string() };
        p.text(pos2(cx, y), Align2::CENTER_CENTER, label, font.clone(), t.meter_label);
        for x in [wells[0] + 11.0, wells[1] - 3.0] {
            p.rect_filled(Rect::from_min_size(pos2(x, y), vec2(2.0, 1.0)), 0.0, t.meter_label);
        }
    }

    faders(ui, view, r, top, bottom, wells);
    for c in 0..2 {
        let g = 20.0 * view.gain[c].max(1e-6).log10() + 0.0;
        let text = if view.gain[c] <= 1e-5 { "-Inf.".to_owned() } else { format!("{g:.1}") };
        p.text(pos2(wells[c] + 5.0, bottom + 6.0), Align2::CENTER_TOP, text, font.clone(), if v16 { t.meter_label } else { t.text });
    }
    master.set_gain(view.gain);
}

/// Solid green with yellow above −6 dB (VP16), or a green → yellow → red gradient by height (VP23), from `y` down.
/// `lit` < 1 blends the colours over `meter_bg` (VP23 unlit wells).
pub(crate) fn paint_bar(p: &Painter, well: Rect, y: f32, to_y: &dyn Fn(f32) -> f32, v16: bool, lit: f32) {
    let t = theme::get();
    let x = well.x_range();
    let seg = |y0: f32, y1: f32, c: Color32| {
        if y1 > y0 {
            p.rect_filled(Rect::from_x_y_ranges(x, Rangef::new(y0, y1)), 0.0, c);
        }
    };
    if v16 {
        let y6 = to_y(-6.0);
        seg(y.max(y6), well.max.y, t.meter_green);
        seg(y, y6.min(well.max.y), t.meter_yellow);
        return;
    }
    // Stops: green to −12 dB, yellow to −3 dB, red at 0 dB.
    let dim = |c: Color32| lerp_color(t.meter_bg, c, lit);
    let stops = [(-RANGE_DB, dim(t.meter_green)), (-12.0, dim(t.meter_green)), (-3.0, dim(t.meter_yellow)), (0.0, dim(t.meter_red))];
    let mut mesh = Mesh::default();
    for w in stops.windows(2) {
        let (ya, yb) = (to_y(w[0].0), to_y(w[1].0));
        let (y0, y1) = (yb.max(y), ya);
        if y1 <= y0 {
            continue;
        }
        let f = |yy: f32| (ya - yy) / (ya - yb).max(1e-3);
        let col = |yy: f32| lerp_color(w[0].1, w[1].1, f(yy));
        let i = mesh.vertices.len() as u32;
        mesh.colored_vertex(pos2(x.min, y1), col(y1));
        mesh.colored_vertex(pos2(x.max, y1), col(y1));
        mesh.colored_vertex(pos2(x.max, y0), col(y0));
        mesh.colored_vertex(pos2(x.min, y0), col(y0));
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i, i + 2, i + 3);
    }
    p.add(mesh);
}

/// Automation-mode badge: white cog ring with an "R" (read).
fn automation_badge(p: &Painter, r: Rect) {
    let t = theme::get();
    let c = r.center();
    for k in 0..8 {
        let a = (k as f32 * 45.0).to_radians();
        p.circle_filled(c + vec2(a.cos(), a.sin()) * 6.0, 1.3, t.icon);
    }
    p.circle_filled(c, 5.5, t.icon);
    p.circle_filled(c, 4.2, Color32::from_rgb(0xD0, 0x60, 0x70));
    p.text(c, Align2::CENTER_CENTER, "R", t.bold(8.0), t.icon);
}

/// Decorative light Windows scrollbar: ‹ › arrows and a thumb (the strip never scrolls).
fn native_hscroll(p: &Painter, r: Rect) {
    let t = theme::get();
    p.rect_filled(r, 0.0, t.native_scroll_bg);
    let thumb = Rect::from_min_max(pos2(r.min.x + 17.0, r.min.y + 2.0), pos2(r.min.x + 17.0 + (r.width() - 34.0) * 0.4, r.max.y - 2.0));
    p.rect_filled(thumb, 0.0, Color32::from_gray(0xCD));
    let ink = Color32::from_gray(0x60);
    let cy = r.center().y;
    for (x, d) in [(r.min.x + 8.0, -1.0), (r.max.x - 8.0, 1.0)] {
        p.add(Shape::line(vec![pos2(x - 1.5 * d, cy - 3.5), pos2(x + 1.5 * d, cy), pos2(x - 1.5 * d, cy + 3.5)], Stroke::new(1.0_f32, ink)));
    }
}

fn lerp_color(a: Color32, b: Color32, f: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * f.clamp(0.0, 1.0)).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// Faders travel the meter height. VP16: one per channel outside the wells (linked unless Ctrl is held);
/// VP23: one stereo thumb left of the wells. Double-click resets to 0 dB.
fn faders(ui: &mut Ui, view: &mut MasterView, r: Rect, top: f32, bottom: f32, wells: [f32; 2]) {
    let t = theme::get();
    let v16 = t.look() == UiLook::Vegas16;
    let p = ui.painter().clone();
    let xs: Vec<f32> = if v16 { vec![r.min.x + 13.0, r.max.x - 13.0] } else { vec![(r.min.x + wells[0]) * 0.5 - 2.0] };
    let to_y = |g: f32| bottom - (bottom - top) * fader_pos(g);
    for (k, &x) in xs.iter().enumerate() {
        let ch = k.min(1);
        p.rect_filled(Rect::from_x_y_ranges(Rangef::new(x.round(), x.round() + 1.0), Rangef::new(top, bottom)), 0.0, t.fader_track);
        let y = to_y(view.gain[ch]);
        let thumb = Rect::from_center_size(pos2(x.round() + 0.5, y.round()), if v16 { vec2(21.0, 12.0) } else { vec2(25.0, 13.0) });
        let hit = Rect::from_x_y_ranges(thumb.expand(3.0).x_range(), Rangef::new(top - 6.0, bottom + 6.0));
        let resp = ui.interact(hit, ui.id().with(("master_fader", k)), Sense::click_and_drag());
        let gain_text = |g: f32| if g <= 1e-5 { "-Inf. dB".to_owned() } else { format!("{:.1} dB", 20.0 * g.log10() + 0.0) };
        let resp = resp.on_hover_text(gain_text(view.gain[ch]));
        let set = |g: f32, view: &mut MasterView| {
            let solo = v16 && ui.input(|i| i.modifiers.ctrl);
            if view.linked && !solo {
                let d = g - view.gain[ch];
                for c in 0..2 {
                    view.gain[c] = (view.gain[c] + d).clamp(0.0, fader_gain(1.0));
                }
            } else {
                view.gain[ch] = g;
            }
        };
        if resp.double_clicked() {
            set(1.0, view);
        } else if let Some(pos) = resp.interact_pointer_pos().filter(|_| resp.dragged()) {
            set(fader_gain((bottom - pos.y) / (bottom - top)), view);
        }
        if v16 {
            // White "88": two solid white squares, each with two dark bars; a lock under the track.
            for dx in [0.0, 11.0] {
                let sq = Rect::from_min_size(pos2(thumb.min.x + dx, thumb.min.y), vec2(10.0, 12.0));
                p.rect_filled(sq, 0.0, t.fader_thumb);
                for dy in [3.0, 7.0] {
                    p.rect_filled(Rect::from_min_size(pos2(sq.min.x + 2.0, sq.min.y + dy), vec2(6.0, 2.0)), 0.0, t.fader_thumb_bar);
                }
            }
            paint_icon(&p, Rect::from_center_size(pos2(x.round() + 0.5, bottom + 12.0), vec2(12.0, 12.0)), Icon::Lock, t.look(), None);
        } else {
            // Twin box 25×13 (light2026): dark 1 px frame and centre divider, two 5 px bars in each half.
            let thumb = Rect::from_min_size(thumb.min.round(), thumb.size());
            p.rect_filled(thumb, 0.0, t.fader_thumb_bar);
            for dx in [1.0, 13.0] {
                let half = Rect::from_min_size(pos2(thumb.min.x + dx, thumb.min.y + 1.0), vec2(11.0, 11.0));
                p.rect_filled(half, 0.0, t.fader_thumb);
                for dy in [4.0, 8.0] {
                    p.rect_filled(Rect::from_min_size(pos2(half.min.x + 3.0, thumb.min.y + dy), vec2(5.0, 1.0)), 0.0, t.fader_thumb_bar);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meter_ballistics_hold_and_clip() {
        let mut m = MasterView::default();
        m.feed([0.5, 1.0], 0.0, 0.016);
        assert!((m.level[0] + 6.02).abs() < 0.01 && m.level[1] == 0.0);
        assert!(m.clip[1] && !m.clip[0]);
        // Silence: the bar falls at FALL_DB_PER_S, the hold stays for HOLD_S.
        m.feed([0.0, 0.0], 1.0, 0.5);
        assert!((m.level[0] - (-6.02 - 12.0)).abs() < 0.01);
        assert!((m.hold[0].0 + 6.02).abs() < 0.01);
        m.feed([0.0, 0.0], 3.0, 0.5);
        assert!(m.hold[0].0 < -6.03);
        assert!((m.max[0] + 6.02).abs() < 0.01);
        m.reset();
        assert!(!m.clip[1] && m.max[1] == -f32::INFINITY);
    }
}
