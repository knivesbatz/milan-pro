//! Trimmer window (§5.7): media history combo, monitor, waveform and filmstrip strips with the in/out range,
//! its own small transport and the In / Out / Length row. A / Shift+A (or drag) add the range to the timeline.
use eframe::egui::*;
use std::path::{Path, PathBuf};
use crate::audio::{PEAKS_PER_SEC, Waveforms};
use crate::decode::{Preview, Thumbs};
use crate::edit;
use crate::history::History;
use crate::project::*;
use super::icons::{Icon, paint_icon};
use super::menus::{NotPlanned, Why};
use super::toolbar::{arrow_menu, text_menu, tool_button};
use super::{MediaDrag, theme};

/// In/out shading and flags (§5.7).
const RANGE_FILL: Color32 = Color32::from_rgba_premultiplied(0x3D, 0x48, 0x64, 0x80);
const FLAG: Color32 = Color32::from_rgb(0xFE, 0xF5, 0x0C);

pub struct Trimmer {
    history: Vec<PathBuf>,
    current: Option<PathBuf>,
    cursor: f64,
    mark_in: Option<f64>,
    mark_out: Option<f64>,
    playing: bool,
    preview: Preview,
    /// Expanded Edit Mode's second monitor (the incoming event).
    preview_b: Preview,
}

impl Trimmer {
    pub fn new(ctx: &Context) -> Self {
        Self { history: Vec::new(), current: None, cursor: 0.0, mark_in: None, mark_out: None, playing: false, preview: Preview::new(ctx), preview_b: Preview::new(ctx) }
    }

    /// Show `path` (most recent first in the history).
    pub fn open(&mut self, path: PathBuf) {
        self.history.retain(|p| *p != path);
        self.history.insert(0, path.clone());
        if self.current.as_ref() != Some(&path) {
            (self.cursor, self.mark_in, self.mark_out, self.playing) = (0.0, None, None, false);
        }
        self.current = Some(path);
    }

    /// In/out marks in source seconds (demo harness).
    pub fn set_marks(&mut self, a: f64, b: f64) {
        (self.mark_in, self.mark_out, self.cursor) = (Some(a), Some(b), a);
    }

    pub fn busy(&self) -> bool {
        self.preview.busy() || self.preview_b.busy()
    }

    /// The selected range, or the whole media without marks.
    fn range(&self, m: &MediaItem) -> (f64, f64) {
        let a = self.mark_in.unwrap_or(0.0);
        let b = self.mark_out.unwrap_or(m.duration).max(a);
        (a, b)
    }
}

fn media_index(project: &Project, path: &Path) -> Option<usize> {
    // The whole file (a subclip's parent) when it is in the pool.
    // shortcut: the Trimmer keeps files, not items, so a subclip opens as its whole file; key it by item to trim subclips.
    let at = |whole: bool| project.media_pool.iter().position(|m| m.path == path && (!whole || m.subclip.is_none()));
    at(true).or_else(|| at(false))
}

/// Add the range at the timeline cursor (A) or ending at it (Shift+A); the cursor moves past an added event.
fn add_to_timeline(project: &mut Project, history: &mut History, idx: usize, range: (f64, f64), up_to_cursor: bool, still_len: f64, xfade: bool) {
    let m = &project.media_pool[idx];
    let video = m.media_type != MediaType::Audio;
    let len = if m.media_type == MediaType::Image { still_len } else { range.1 - range.0 };
    let cursor = project.timeline.cursor_position;
    let at = if up_to_cursor { (cursor - len).max(0.0) } else { cursor };
    let before = project.clone();
    if !edit::drop_media(project, &[idx], Some(range), Some((video, 0)), at, still_len, xfade).is_empty() {
        history.push(before);
        if !up_to_cursor {
            project.timeline.cursor_position = at + len;
        }
    }
}

pub fn draw_trimmer_toolbar(ui: &mut Ui, tr: &mut Trimmer) {
    let label = tr.current.as_ref().map_or("(no media)".to_owned(), |p| {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let dir = p.parent().map(|d| d.display().to_string()).unwrap_or_default();
        format!("{name} [{dir}]")
    });
    let mut pick = None;
    text_menu(ui, &label, 4.0, "Trimmer history", |ui| {
        for p in &tr.history {
            if ui.button(p.display().to_string()).clicked() {
                pick = Some(p.clone());
                ui.close_menu();
            }
        }
    });
    if let Some(p) = pick {
        tr.open(p);
    }
    tool_button(ui, Icon::ExternalMonitor, &NotPlanned("R57").tip("Show Video in Preview"), false, true);
    if tool_button(ui, Icon::RemoveUnused, "Clear History", !tr.history.is_empty(), false).clicked() {
        tr.history.clear();
        tr.current = None;
    }
    if tool_button(ui, Icon::Delete, "Remove from History", tr.current.is_some(), false).clicked() {
        let cur = tr.current.take();
        tr.history.retain(|p| Some(p) != cur.as_ref());
        tr.current = tr.history.first().cloned();
    }
}

/// One side of an edit point in Expanded Edit Mode: the event's media and the source time of its frame there.
pub type EditSide = Option<(MediaItem, f64)>;

/// `edit_point` (Expanded Edit Mode): the Trimmer shows the outgoing event's last frame and the incoming event's first
/// frame at the selected edit point, side by side, instead of its media.
#[allow(clippy::too_many_arguments)]
pub fn draw_trimmer(ui: &mut Ui, tr: &mut Trimmer, project: &mut Project, history: &mut History, waves: &mut Waveforms, thumbs: &mut Thumbs, still_len: f64, xfade: bool, edit_point: Option<[EditSide; 2]>) {
    let t = theme::get();
    let rect = ui.max_rect();
    let p = ui.painter().clone();
    p.rect_filled(rect, 0.0, t.preview_bg);
    if let Some(sides) = edit_point {
        let fps = project.settings.frame_rate;
        let half = rect.width() * 0.5;
        let previews = [&mut tr.preview, &mut tr.preview_b];
        for (k, (side, pv)) in sides.iter().zip(previews).enumerate() {
            let cell = Rect::from_min_size(pos2(rect.min.x + half * k as f32, rect.min.y), vec2(half, rect.height())).shrink(4.0);
            let label = Rect::from_min_size(pos2(cell.min.x, cell.max.y - 18.0), vec2(cell.width(), 18.0));
            let monitor = cell.with_max_y(label.min.y - 2.0);
            let name = if k == 0 { "Outgoing (last frame)" } else { "Incoming (first frame)" };
            let Some((m, src)) = side else {
                p.text(monitor.center(), Align2::CENTER_CENTER, "No event", t.font(t.font.ui), t.text_dim);
                p.text(label.center(), Align2::CENTER_CENTER, name, t.font(t.font.ui), t.text_dim);
                continue;
            };
            pv.display_w = monitor.width().max(2.0) as u32;
            let video = m.media_type != MediaType::Audio;
            pv.show(ui.ctx(), video.then_some((m, if m.media_type == MediaType::Image { 0.0 } else { *src })), fps, m.width.unwrap_or(1920));
            let aspect = m.width.zip(m.height).map_or(16.0 / 9.0, |(w, h)| w as f32 / h.max(1) as f32);
            let (w, h) = if monitor.width() / monitor.height().max(1.0) > aspect { (monitor.height() * aspect, monitor.height()) } else { (monitor.width(), monitor.width() / aspect) };
            let frame = super::widgets::px(Rect::from_center_size(monitor.center(), vec2(w, h).max(Vec2::ZERO)));
            p.rect_filled(frame, 0.0, t.preview_frame);
            match &pv.texture {
                Some(tex) if video => {
                    p.image(tex.id(), frame, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                }
                _ => super::widgets::paint_audio_page(&p, Rect::from_center_size(frame.center(), vec2(80.0, 60.0))),
            }
            let mfps = m.frame_rate.filter(|f| *f > 0.0).unwrap_or(fps);
            p.text(label.center(), Align2::CENTER_CENTER, format!("{name}  {}", format_timecode(*src, mfps)), t.font(t.font.ui), t.text);
        }
        return;
    }
    let Some(path) = tr.current.clone() else {
        p.text(rect.center(), Align2::CENTER_CENTER, "Double-click media in Project Media or Explorer to trim it.", t.font(t.font.ui), t.text_dim);
        return;
    };
    let Some(idx) = media_index(project, &path) else {
        p.text(rect.center(), Align2::CENTER_CENTER, format!("Importing {}...", path.display()), t.font(t.font.ui), t.text_dim);
        return;
    };
    let m = project.media_pool[idx].clone();
    let dur = m.duration.max(0.001);
    let fps = m.frame_rate.filter(|f| *f > 0.0).unwrap_or(project.settings.frame_rate);

    // Playback: video only, on the frame clock.
    if tr.playing {
        tr.cursor += ui.input(|i| i.stable_dt).min(0.1) as f64;
        if tr.cursor >= dur {
            (tr.cursor, tr.playing) = (dur, false);
        }
        ui.ctx().request_repaint();
    }

    let has_audio = m.sample_rate.is_some();
    let bottom_h = 22.0;
    let transport_h = 28.0;
    let film_h = 30.0;
    let wave_h = if has_audio { 36.0 } else { 0.0 };
    let film = Rect::from_min_max(pos2(rect.min.x + 4.0, rect.max.y - bottom_h - transport_h - film_h), pos2(rect.max.x - 4.0, rect.max.y - bottom_h - transport_h));
    let wave = Rect::from_min_max(pos2(film.min.x, film.min.y - wave_h), pos2(film.max.x, film.min.y));
    let monitor = Rect::from_min_max(rect.min, pos2(rect.max.x, wave.min.y - 4.0));

    // Monitor: letterboxed frame from the trimmer's own decoder.
    let pw = m.width.unwrap_or(1920);
    tr.preview.display_w = monitor.width().max(2.0) as u32;
    let src = (m.media_type != MediaType::Audio).then_some((&m, if m.media_type == MediaType::Image { 0.0 } else { tr.cursor.min(dur - 0.001) }));
    tr.preview.show(ui.ctx(), src, fps, pw);
    let aspect = m.width.zip(m.height).map_or(16.0 / 9.0, |(w, h)| w as f32 / h.max(1) as f32);
    let (w, h) = if monitor.width() / monitor.height().max(1.0) > aspect { (monitor.height() * aspect, monitor.height()) } else { (monitor.width(), monitor.width() / aspect) };
    let frame = super::widgets::px(Rect::from_center_size(monitor.center(), vec2(w, h).max(Vec2::ZERO)));
    p.rect_filled(frame, 0.0, t.preview_frame);
    if let Some(tex) = &tr.preview.texture {
        p.image(tex.id(), frame, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    } else if m.media_type == MediaType::Audio {
        super::widgets::paint_audio_page(&p, Rect::from_center_size(frame.center(), vec2(80.0, 60.0)));
    }

    let to_x = |s: f64| film.min.x + (s / dur) as f32 * film.width();
    let to_s = |x: f32| (((x - film.min.x) / film.width()) as f64 * dur).clamp(0.0, dur);

    if has_audio {
        p.rect_filled(wave, 0.0, t.lane_bg);
        if let Some(peaks) = waves.get(&m).map(|c| &c[0]) {
            let n = wave.width() as usize;
            let cy = wave.center().y;
            let at = |f: f64| (m.file_time(f * dur) * PEAKS_PER_SEC) as usize; // peaks are the file's
            for k in 0..n {
                let s0 = at(k as f64 / n as f64);
                let s1 = at((k + 1) as f64 / n as f64).max(s0 + 1);
                let v = peaks.get(s0..s1.min(peaks.len())).and_then(|s| s.iter().copied().reduce(f32::max)).unwrap_or(0.0);
                let hh = v * wave.height() * 0.45;
                let x = wave.min.x + k as f32 + 0.5;
                p.vline(x, Rangef::new(cy - hh, cy + hh.max(0.5)), Stroke::new(1.0_f32, t.text_dim));
            }
        }
    }

    // Filmstrip: frames across the strip, the in/out range shaded, yellow flags above the strip.
    p.rect_filled(film, 0.0, t.frame);
    if m.media_type != MediaType::Audio {
        let cell = (film.height() * 16.0 / 9.0).max(8.0);
        let n = (film.width() / cell).ceil() as usize;
        for k in 0..n {
            let x = film.min.x + k as f32 * cell;
            let s = ((to_s(x + cell * 0.5) * 2.0).floor() / 2.0).min(dur);
            let r = Rect::from_min_size(pos2(x + 1.0, film.min.y + 2.0), vec2(cell - 2.0, film.height() - 4.0)).intersect(film);
            if let Some(tex) = thumbs.get(&m, s) {
                p.image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
        }
    }
    let (a, b) = tr.range(&m);
    if tr.mark_in.is_some() || tr.mark_out.is_some() {
        p.rect_filled(Rect::from_x_y_ranges(Rangef::new(to_x(a), to_x(b)), film.y_range()), 0.0, RANGE_FILL);
        for (x, left) in [(to_x(a), true), (to_x(b), false)] {
            let y = film.min.y;
            let pts = if left { vec![pos2(x, y - 7.0), pos2(x + 7.0, y - 7.0), pos2(x, y)] } else { vec![pos2(x - 7.0, y - 7.0), pos2(x, y - 7.0), pos2(x, y)] };
            p.add(Shape::convex_polygon(pts, FLAG, Stroke::NONE));
            p.vline(x, film.y_range(), Stroke::new(1.0_f32, Color32::from_rgb(0x20, 0x30, 0x90)));
        }
    }
    let cx = to_x(tr.cursor).round();
    p.vline(cx + 0.5, Rangef::new(wave.min.y, film.max.y), Stroke::new(1.0_f32, t.text_bright));

    // Click/drag the strips to scrub; drag the monitor to add the range to the timeline.
    let strips = wave.union(film);
    let scrub = ui.interact(strips, ui.id().with("trim_scrub"), Sense::click_and_drag());
    if let Some(pos) = scrub.interact_pointer_pos().filter(|_| scrub.dragged() || scrub.clicked()) {
        tr.cursor = to_s(pos.x);
        tr.playing = false;
    }
    let drag = ui.interact(frame, ui.id().with("trim_drag"), Sense::drag()).on_hover_text("Drag to the timeline to add the selected range");
    drag.dnd_set_drag_payload(MediaDrag::Pool(vec![idx], Some((a, b))));

    // Keys while the pointer is over the Trimmer: I / O set in / out, A / Shift+A add to the timeline.
    super::claim_plain_keys(ui.ctx(), rect);
    if ui.rect_contains_pointer(rect) && !ui.ctx().wants_keyboard_input() {
        let (i, o, add, shift) = ui.input(|i| (i.key_pressed(Key::I), i.key_pressed(Key::O), i.key_pressed(Key::A), i.modifiers.shift));
        if i {
            tr.mark_in = Some(tr.cursor);
            tr.mark_out = tr.mark_out.filter(|&o| o > tr.cursor);
        }
        if o {
            tr.mark_out = Some(tr.cursor);
            tr.mark_in = tr.mark_in.filter(|&i| i < tr.cursor);
        }
        if add {
            add_to_timeline(project, history, idx, tr.range(&m), shift, still_len, xfade);
        }
    }

    transport(ui, tr, project, history, idx, &m, Rect::from_min_max(pos2(rect.min.x, film.max.y), pos2(rect.max.x, film.max.y + transport_h)), still_len, xfade);

    // In ◤ · Out ◥ · Length.
    let (a, b) = tr.range(&m);
    let y = rect.max.y - bottom_h * 0.5;
    let font = t.font(t.font.transport_time);
    let items = [(Icon::LengthCursor, a), (Icon::LengthSelEnd, b), (Icon::LengthLength, b - a)];
    let x0 = rect.center().x - 1.5 * 130.0;
    for (k, (icon, v)) in items.into_iter().enumerate() {
        let x = x0 + k as f32 * 130.0;
        paint_icon(&p, Rect::from_min_size(pos2(x, y - 7.0), vec2(14.0, 14.0)), icon, t.look(), None);
        p.text(pos2(x + 20.0, y), Align2::LEFT_CENTER, format_timecode(v, fps), font.clone(), t.text);
    }
}

/// VP16 ▶ ❚❚ ■ · Add to Timeline · ≡ (full set behind More).
fn transport(ui: &mut Ui, tr: &mut Trimmer, project: &mut Project, history: &mut History, idx: usize, m: &MediaItem, row: Rect, still_len: f64, xfade: bool) {
    let t = theme::get();
    let r = Rect::from_center_size(row.center(), vec2(5.0 * t.m.button_pitch, t.m.button));
    let mut ui = ui.new_child(UiBuilder::new().max_rect(r).layout(Layout::left_to_right(Align::Center)));
    ui.spacing_mut().item_spacing.x = 0.0;
    let dur = m.duration;
    let frame = 1.0 / m.frame_rate.filter(|f| *f > 0.0).unwrap_or(project.settings.frame_rate).max(1.0);
    if tool_button(&mut ui, Icon::Play, "Play", true, tr.playing).clicked() {
        if tr.cursor >= dur {
            tr.cursor = tr.mark_in.unwrap_or(0.0);
        }
        tr.playing = true;
    }
    if tool_button(&mut ui, Icon::Pause, "Pause", true, false).clicked() {
        tr.playing = false;
    }
    if tool_button(&mut ui, Icon::Stop, "Stop", true, false).clicked() {
        (tr.playing, tr.cursor) = (false, tr.mark_in.unwrap_or(0.0));
    }
    if tool_button(&mut ui, Icon::Render, "Add to Timeline from Cursor (A)", true, false).clicked() {
        add_to_timeline(project, history, idx, tr.range(m), false, still_len, xfade);
    }
    let more = tool_button(&mut ui, Icon::More, "More", true, false);
    arrow_menu(&ui, &more, |ui| {
        let item = |ui: &mut Ui, text: &str, enabled: bool| ui.add_enabled(enabled, Button::new(text)).clicked();
        let off = |ui: &mut Ui, text: &str, why: Why| ui.add_enabled(false, Button::new(text)).on_disabled_hover_text(why.text());
        off(ui, "Loop Playback", NotPlanned("R57"));
        if item(ui, "Play From Start", true) {
            (tr.cursor, tr.playing) = (0.0, true);
        }
        if item(ui, "Go to Start", true) {
            tr.cursor = 0.0;
        }
        if item(ui, "Go to End", true) {
            tr.cursor = dur;
        }
        if item(ui, "Previous Frame", true) {
            tr.cursor = (tr.cursor - frame).max(0.0);
        }
        if item(ui, "Next Frame", true) {
            tr.cursor = (tr.cursor + frame).min(dur);
        }
        ui.separator();
        if item(ui, "Add to Timeline from Cursor (A)", true) {
            add_to_timeline(project, history, idx, tr.range(m), false, still_len, xfade);
        }
        if item(ui, "Add to Timeline up to Cursor (Shift+A)", true) {
            add_to_timeline(project, history, idx, tr.range(m), true, still_len, xfade);
        }
        off(ui, "Fit to Fill", NotPlanned("R58"));
        // The in/out range (or the whole media) becomes a new item in Project Media; one undo step.
        if item(ui, "Create Subclip", m.media_type != MediaType::Image && m.generator.is_none()) {
            let (before, (a, b)) = (project.clone(), tr.range(m));
            if edit::create_subclip(project, &m.id, a, b - a).is_some() {
                history.push(before);
            }
        }
        ui.separator();
        if item(ui, "Set In (I)", true) {
            tr.mark_in = Some(tr.cursor);
        }
        if item(ui, "Set Out (O)", true) {
            tr.mark_out = Some(tr.cursor);
        }
        if item(ui, "Clear In/Out", tr.mark_in.is_some() || tr.mark_out.is_some()) {
            (tr.mark_in, tr.mark_out) = (None, None);
        }
    });
}
