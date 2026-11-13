//! R16b: the Video Scopes window (View › Window › Video Scopes, Ctrl+Alt+2), as VEGAS has it: the scope list
//! (Vectorscope, Waveform, Histogram, RGB Parade and their combinations), Settings... (7.5 IRE Setup, Studio RGB),
//! Update Scopes while Playing, and each scope's own settings above it. It measures the Video Preview's composited
//! frame (never decoding again) on its own thread, at most 10 times a second; closed, it costs nothing.
use eframe::egui::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use crate::engine::scopes::{self as sc, Channel, Hist, HistMode, Levels, Want};
use super::icons::Icon;
use super::theme;
use super::toolbar::{text_menu, tool_button};

/// The scope list (VEGAS's order).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum ScopeView {
    #[default]
    Vectorscope,
    Waveform,
    Histogram,
    Parade,
    VectorWave,
    VectorWaveHist,
    VectorParade,
    All,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Pane {
    Vector,
    Wave,
    Hist,
    Parade,
}

impl ScopeView {
    const ALL: [ScopeView; 8] = [
        ScopeView::Vectorscope,
        ScopeView::Waveform,
        ScopeView::Histogram,
        ScopeView::Parade,
        ScopeView::VectorWave,
        ScopeView::VectorWaveHist,
        ScopeView::VectorParade,
        ScopeView::All,
    ];

    fn label(self) -> &'static str {
        ["Vectorscope", "Waveform", "Histogram", "RGB Parade", "Vectorscope/Waveform", "Vectorscope/Waveform/Histogram", "Vectorscope/RGB Parade", "All"][self as usize]
    }

    fn panes(self) -> &'static [Pane] {
        use Pane::*;
        match self {
            ScopeView::Vectorscope => &[Vector],
            ScopeView::Waveform => &[Wave],
            ScopeView::Histogram => &[Hist],
            ScopeView::Parade => &[Parade],
            ScopeView::VectorWave => &[Vector, Wave],
            ScopeView::VectorWaveHist => &[Vector, Wave, Hist],
            ScopeView::VectorParade => &[Vector, Parade],
            ScopeView::All => &[Vector, Wave, Hist, Parade],
        }
    }
}

/// What the Video Scopes window shows and how, kept in the preferences (VEGAS keeps them between sessions). The
/// defaults are VEGAS's: the vectorscope at Normal, the waveform's Luminance, the histogram's Luminance, both settings
/// off and no updates while playing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScopePrefs {
    pub view: ScopeView,
    pub levels: Levels,
    pub composite: bool,
    pub magnified: bool,
    pub hist: HistMode,
    pub while_playing: bool,
}

impl ScopePrefs {
    fn want(&self) -> Want {
        let has = |p| self.view.panes().contains(&p);
        Want {
            wave: has(Pane::Wave),
            vector: has(Pane::Vector),
            parade: has(Pane::Parade),
            hist: has(Pane::Hist).then_some(self.hist),
            levels: self.levels,
            composite: self.composite,
            magnified: self.magnified,
        }
    }
}

/// A frame to measure: the preview's, with Split-Screen View › Clipboard's region taken from the snapshot.
struct Job {
    img: Arc<ColorImage>,
    split: Option<(Arc<ColorImage>, [f32; 4])>,
    want: Want,
}

impl Job {
    fn same(&self, img: &Arc<ColorImage>, split: Option<(&Arc<ColorImage>, [f32; 4])>, want: &Want) -> bool {
        let split_same = match (&self.split, split) {
            (None, None) => true,
            (Some((a, r)), Some((b, s))) => Arc::ptr_eq(a, b) && *r == s,
            _ => false,
        };
        Arc::ptr_eq(&self.img, img) && split_same && self.want == *want
    }
}

/// The plots drawn on the scope thread, and the histograms.
struct Done {
    wave: Option<ColorImage>,
    vector: Option<ColorImage>,
    parade: Option<ColorImage>,
    hist: Vec<(Channel, Hist)>,
}

fn work(job: &Job) -> Done {
    let spliced = job.split.as_ref().map(|(o, r)| sc::splice(&job.img, o, *r));
    let s = sc::analyze(spliced.as_ref().unwrap_or(&job.img), &job.want);
    let trace = Color32::from_gray(235);
    // A composite waveform marks two points per pixel; the parade three, one in each channel's third.
    let wave_n = s.samples * if job.want.composite { 2 } else { 1 };
    let rgb = [Channel::Red, Channel::Green, Channel::Blue];
    Done {
        wave: s.wave.map(|g| g.image(wave_n, |_| trace)),
        vector: s.vector.map(|g| g.image(s.samples, |_| trace)),
        parade: s.parade.map(|g| g.image(3 * s.samples, |x| rgb[(x / sc::PARADE_W).min(2)].color())),
        hist: s.hist,
    }
}

/// The window's state: the scope thread (started when first needed), the last request and the plots.
#[derive(Default)]
pub struct Scopes {
    chan: Option<(Sender<Job>, Receiver<Done>)>,
    throttle: sc::Throttle,
    asked: Option<Job>,
    wave: Option<TextureHandle>,
    vector: Option<TextureHandle>,
    parade: Option<TextureHandle>,
    hist: Vec<(Channel, Hist)>,
    /// Video Scopes Settings, open: the settings being edited.
    pub settings: Option<Levels>,
}

impl Scopes {
    fn send(&mut self, ctx: &Context, job: Job) {
        let (tx, _) = self.chan.get_or_insert_with(|| {
            let (tx, jobs) = mpsc::channel::<Job>();
            let (done_tx, rx) = mpsc::channel();
            let ctx = ctx.clone();
            let _ = std::thread::Builder::new().name("milan-scopes".into()).spawn(move || {
                while let Ok(mut job) = jobs.recv() {
                    while let Ok(newer) = jobs.try_recv() {
                        job = newer;
                    }
                    if done_tx.send(work(&job)).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            });
            (tx, rx)
        });
        let _ = tx.send(Job { img: job.img.clone(), split: job.split.clone(), want: job.want });
        self.asked = Some(job);
    }

    fn receive(&mut self, ctx: &Context) {
        let Some((_, rx)) = &self.chan else { return };
        let Some(d) = rx.try_iter().last() else { return };
        let put = |slot: &mut Option<TextureHandle>, img: Option<ColorImage>, name: &str| {
            *slot = img.map(|img| match slot.take() {
                Some(mut t) => {
                    t.set(img, TextureOptions::LINEAR);
                    t
                }
                None => ctx.load_texture(name, img, TextureOptions::LINEAR),
            });
        };
        put(&mut self.wave, d.wave, "scope_waveform");
        put(&mut self.vector, d.vector, "scope_vectorscope");
        put(&mut self.parade, d.parade, "scope_parade");
        self.hist = d.hist;
    }
}

/// The toolbar: the scope list, Settings... and Update Scopes while Playing. True when the preferences changed.
pub fn scopes_toolbar(ui: &mut Ui, s: &mut Scopes, p: &mut ScopePrefs) -> bool {
    let before = *p;
    ui.add_space(4.0);
    text_menu(ui, p.view.label(), 7.0, "Scope", |ui| {
        for v in ScopeView::ALL {
            if ui.radio(p.view == v, v.label()).clicked() {
                p.view = v;
                ui.close_menu();
            }
        }
    });
    if tool_button(ui, Icon::Properties, "Settings...", true, false).clicked() {
        s.settings = Some(p.levels);
    }
    if tool_button(ui, Icon::Refresh, "Update Scopes while Playing", true, p.while_playing).clicked() {
        p.while_playing = !p.while_playing;
    }
    *p != before
}

/// The scopes of `frame` (the Video Preview's composited frame; `split`: Split-Screen View › Clipboard's snapshot and
/// region). While `playing` they follow the frame only with Update Scopes while Playing on. True when the preferences
/// changed.
pub fn draw_scopes(ui: &mut Ui, s: &mut Scopes, p: &mut ScopePrefs, frame: Option<&Arc<ColorImage>>, split: Option<(&Arc<ColorImage>, [f32; 4])>, playing: bool) -> bool {
    let before = *p;
    let ctx = ui.ctx().clone();
    s.receive(&ctx);
    let want = p.want();
    if let Some(img) = frame.filter(|_| !playing || p.while_playing) {
        if !s.asked.as_ref().is_some_and(|j| j.same(img, split, &want)) {
            let wait = s.throttle.wait(ui.input(|i| i.time));
            if wait > 0.0 {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
            } else {
                s.send(&ctx, Job { img: img.clone(), split: split.map(|(o, r)| (o.clone(), r)), want });
            }
        }
    }

    let t = theme::get();
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0.0, Color32::BLACK);
    let panes = p.view.panes();
    let cells = cells(rect, panes.len());
    for (&pane, &cell) in panes.iter().zip(&cells) {
        let mut row = ui.new_child(UiBuilder::new().max_rect(Rect::from_min_size(cell.min, vec2(cell.width(), t.m.button))).layout(Layout::left_to_right(Align::Center)));
        row.add_space(2.0);
        let plot = cell.with_min_y(cell.min.y + t.m.button).shrink(6.0);
        match pane {
            Pane::Vector => {
                setting(&mut row, "Vectorscope Settings", &mut p.magnified, ["Normal", "Magnified"]);
                vectorscope(ui, plot, s.vector.as_ref(), p.magnified);
            }
            Pane::Wave => {
                setting(&mut row, "Waveform Settings", &mut p.composite, ["Luminance", "Composite"]);
                waveform(ui, plot, s.wave.as_ref(), p.composite, p.levels);
            }
            Pane::Hist => {
                text_menu(&mut row, p.hist.label(), 7.0, "Histogram Settings", |ui| {
                    for m in HistMode::ALL {
                        if ui.radio(p.hist == m, m.label()).clicked() {
                            p.hist = m;
                            ui.close_menu();
                        }
                    }
                });
                histograms(ui, plot, &s.hist);
            }
            Pane::Parade => parade(ui, plot, s.parade.as_ref()),
        }
    }

    if let Some(mut lv) = s.settings {
        let (hit, close) = super::dialogs::modal(&ctx, "video_scopes_settings", "Video Scopes Settings", vec2(300.0, 150.0), |ui| {
            ui.checkbox(&mut lv.setup, "7.5 IRE Setup");
            ui.checkbox(&mut lv.studio, "Studio RGB (16 to 235)");
            ui.add_space(12.0);
            super::dialogs::button_row(ui, &[("OK", true), ("Cancel", true)])
        });
        s.settings = Some(lv);
        if hit == Some(0) {
            p.levels = lv;
        }
        if hit.is_some() || close {
            s.settings = None;
        }
    }
    *p != before
}

/// A scope's own two-way setting, as a drop-down above it.
fn setting(ui: &mut Ui, tip: &str, on: &mut bool, labels: [&str; 2]) {
    text_menu(ui, labels[*on as usize], 7.0, tip, |ui| {
        for (k, l) in labels.into_iter().enumerate() {
            if ui.radio(*on == (k == 1), l).clicked() {
                *on = k == 1;
                ui.close_menu();
            }
        }
    });
}

/// `n` panes in `r`: side by side (stacked when `r` is tall), All as two by two.
fn cells(r: Rect, n: usize) -> Vec<Rect> {
    if n == 4 {
        let (top, bottom) = r.split_top_bottom_at_fraction(0.5);
        let ((a, b), (c, d)) = (top.split_left_right_at_fraction(0.5), bottom.split_left_right_at_fraction(0.5));
        return vec![a, b, c, d];
    }
    let k = n.max(1) as f32;
    (0..n)
        .map(|i| {
            let (f0, f1) = (i as f32 / k, (i + 1) as f32 / k);
            if r.width() >= r.height() {
                Rect::from_x_y_ranges(r.min.x + f0 * r.width()..=r.min.x + f1 * r.width(), r.y_range())
            } else {
                Rect::from_x_y_ranges(r.x_range(), r.min.y + f0 * r.height()..=r.min.y + f1 * r.height())
            }
        })
        .collect()
}

fn line_color() -> Color32 {
    Color32::from_gray(70)
}

fn label(p: &Painter, at: Pos2, align: Align2, text: impl ToString) {
    p.text(at, align, text.to_string(), FontId::proportional(10.0), Color32::from_gray(150));
}

/// A graticule label sits above its line, or below it on the top line (where above is off the plot).
fn above_or_below(top: bool) -> Align2 {
    if top { Align2::LEFT_TOP } else { Align2::LEFT_BOTTOM }
}

/// The plot texture over `r`.
fn plot_image(p: &Painter, r: Rect, tex: Option<&TextureHandle>) {
    if let Some(t) = tex {
        p.image(t.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    }
}

/// Graticule: axes, the 100 % circle and a target box for each 75 % bar (R, Mg, B, Cy, G, Yl); hover reads the hue
/// angle and the saturation against 100 % bars.
fn vectorscope(ui: &mut Ui, area: Rect, tex: Option<&TextureHandle>, magnified: bool) {
    let side = area.width().min(area.height()).max(1.0);
    let r = Rect::from_center_size(area.center(), Vec2::splat(side));
    let p = ui.painter().with_clip_rect(r.intersect(ui.clip_rect()));
    let c = r.center();
    let at = |u: f32, v: f32| {
        let (x, y) = sc::vec_pos(u, v, magnified);
        c + vec2(x, -y) * side * 0.5
    };
    let stroke = Stroke::new(1.0_f32, line_color());
    p.hline(r.x_range(), c.y, stroke);
    p.vline(c.x, r.y_range(), stroke);
    let (_, ru, rv) = sc::targets()[0];
    let full = (at(ru, rv) - c).length() / 0.75;
    p.circle_stroke(c, full, stroke);
    plot_image(&p, r, tex);
    let box_w = side * 0.04;
    for (name, u, v) in sc::targets() {
        let q = at(u, v);
        p.rect_stroke(Rect::from_center_size(q, Vec2::splat(box_w)), 0.0, Stroke::new(1.0_f32, Color32::from_gray(150)), StrokeKind::Middle);
        let out = (q - c).normalized() * (box_w + 8.0);
        label(&p, q + out, Align2::CENTER_CENTER, name);
    }
    let resp = ui.interact(r, ui.id().with("vectorscope"), Sense::hover());
    if let Some(h) = resp.hover_pos() {
        let d = h - c;
        let angle = (-d.y).atan2(d.x).to_degrees().rem_euclid(360.0);
        resp.on_hover_text_at_pointer(format!("{angle:.0}°, {:.0} %", d.length() / full * 100.0));
    }
}

/// Graticule every 10 IRE, labelled every 20, with 7.5 IRE marked under setup; hover reads IRE.
fn waveform(ui: &mut Ui, r: Rect, tex: Option<&TextureHandle>, composite: bool, levels: Levels) {
    let p = ui.painter().with_clip_rect(r.intersect(ui.clip_rect()));
    let (lo, hi) = sc::wave_range(composite);
    let y_of = |ire: f32| r.max.y - (ire - lo) / (hi - lo) * r.height();
    let mut ire = lo;
    while ire <= hi + 0.1 {
        p.hline(r.x_range(), y_of(ire).round() + 0.5, Stroke::new(1.0_f32, if ire == 0.0 || ire == 100.0 { Color32::from_gray(110) } else { line_color() }));
        if (ire as i32).rem_euclid(20) == 0 {
            label(&p, pos2(r.min.x + 2.0, y_of(ire)), above_or_below(ire >= hi - 1.0), ire as i32);
        }
        ire += 10.0;
    }
    if levels.setup {
        p.hline(r.x_range(), y_of(7.5).round() + 0.5, Stroke::new(1.0_f32, Color32::from_rgb(140, 120, 60)));
    }
    plot_image(&p, r, tex);
    let resp = ui.interact(r, ui.id().with("waveform"), Sense::hover());
    if let Some(h) = resp.hover_pos() {
        resp.on_hover_text_at_pointer(format!("{:.1} IRE", lo + (r.max.y - h.y) / r.height() * (hi - lo)));
    }
}

/// Red, green and blue waveforms side by side on a 0-255 scale; hover reads the value.
fn parade(ui: &mut Ui, r: Rect, tex: Option<&TextureHandle>) {
    let p = ui.painter().with_clip_rect(r.intersect(ui.clip_rect()));
    for v in [0, 64, 128, 191, 255] {
        let y = (r.max.y - v as f32 / 255.0 * r.height()).round() + 0.5;
        p.hline(r.x_range(), y, Stroke::new(1.0_f32, line_color()));
        label(&p, pos2(r.min.x + 2.0, y), above_or_below(v == 255), v);
    }
    for k in 1..3 {
        p.vline((r.min.x + r.width() * k as f32 / 3.0).round() + 0.5, r.y_range(), Stroke::new(1.0_f32, Color32::from_gray(90)));
    }
    plot_image(&p, r, tex);
    let resp = ui.interact(r, ui.id().with("parade"), Sense::hover());
    if let Some(h) = resp.hover_pos() {
        resp.on_hover_text_at_pointer(format!("{:.0}", ((r.max.y - h.y) / r.height() * 255.0).clamp(0.0, 255.0)));
    }
}

/// One graph per channel, stacked, each with its Mean and Std Dev under it (VEGAS's "Mean: x Std Dev: y").
fn histograms(ui: &mut Ui, r: Rect, hist: &[(Channel, Hist)]) {
    let n = hist.len().max(1) as f32;
    let text_h = 14.0;
    for (i, (ch, h)) in hist.iter().enumerate() {
        let cell = Rect::from_x_y_ranges(r.x_range(), r.min.y + r.height() * i as f32 / n..=r.min.y + r.height() * (i + 1) as f32 / n);
        let g = cell.with_max_y(cell.max.y - text_h);
        let p = ui.painter().with_clip_rect(cell.intersect(ui.clip_rect()));
        p.rect_stroke(g, 0.0, Stroke::new(1.0_f32, line_color()), StrokeKind::Inside);
        let max = h.bins.iter().copied().max().unwrap_or(0).max(1) as f32;
        let bw = g.width() / 256.0;
        let mut mesh = epaint::Mesh::default();
        for (k, &c) in h.bins.iter().enumerate() {
            if c > 0 {
                let x = g.min.x + k as f32 * bw;
                mesh.add_colored_rect(Rect::from_min_max(pos2(x, g.max.y - c as f32 / max * g.height()), pos2(x + bw.max(1.0), g.max.y)), ch.color());
            }
        }
        p.add(mesh);
        label(&p, pos2(cell.min.x + 2.0, cell.max.y), Align2::LEFT_BOTTOM, format!("Mean: {:.2}    Std Dev: {:.2}", h.mean, h.std));
        let resp = ui.interact(g, ui.id().with(("histogram", i)), Sense::hover());
        if let Some(pos) = resp.hover_pos() {
            let k = (((pos.x - g.min.x) / bw) as usize).min(255);
            resp.on_hover_text_at_pointer(format!("{:.0}: {} pixels", ch.value(k as f32), h.bins[k]));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The window asks for the scopes its view shows, and an old preferences file keeps VEGAS's defaults.
    #[test]
    fn views_want_their_scopes_and_old_prefs_load() {
        let p = ScopePrefs { view: ScopeView::VectorParade, ..Default::default() };
        let w = p.want();
        assert!(w.vector && w.parade && !w.wave && w.hist.is_none());
        let all = ScopePrefs { view: ScopeView::All, ..Default::default() }.want();
        assert!(all.vector && all.wave && all.parade && all.hist == Some(HistMode::Luminance));
        let old: ScopePrefs = serde_json::from_str(r#"{"view":"Waveform"}"#).unwrap();
        assert_eq!(old, ScopePrefs { view: ScopeView::Waveform, ..Default::default() });
        assert!(!old.while_playing && !old.levels.setup && !old.levels.studio);
        assert_eq!(cells(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 300.0)), 3)[1], Rect::from_min_max(pos2(200.0, 0.0), pos2(400.0, 300.0)));
    }

    /// A new frame is measured once; the same frame, view and settings again are not; Split-Screen View's snapshot
    /// region is part of what is measured.
    #[test]
    fn a_frame_is_measured_once() {
        let img = Arc::new(ColorImage::new([4, 4], Color32::WHITE));
        let snap = Arc::new(ColorImage::new([4, 4], Color32::BLACK));
        let want = ScopePrefs::default().want();
        let job = Job { img: img.clone(), split: None, want };
        assert!(job.same(&img, None, &want));
        assert!(!job.same(&Arc::new((*img).clone()), None, &want));
        assert!(!job.same(&img, Some((&snap, [0.5, 0.0, 1.0, 1.0])), &want));
        assert!(!job.same(&img, None, &Want { magnified: true, ..want }));
        let d = work(&Job { img, split: Some((snap, [0.0, 0.0, 1.0, 1.0])), want: Want { hist: Some(HistMode::Luminance), ..want } });
        assert_eq!(d.hist[0].1.bins[0], 16, "the snapshot fills the frame");
        assert!(d.vector.is_some() && d.wave.is_none());
    }
}
