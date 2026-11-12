//! Event Properties (event menu › Properties...): the event page edits a copy of the event, applied in one undo step
//! on OK; the media page shows the active take's media. A field only changes the value when its text is edited, so
//! focusing it does not round an off-frame value to the frame or rate shown.
use eframe::egui::*;
use crate::edit;
use crate::project::{Clip, Curve, MediaItem, Project, format_timecode};
use super::super::menus::Later;

pub struct EventProps {
    pub id: String,
    pub draft: Clip,
    is_video: bool,
    /// On a locked track only the event's Lock switch can change (`edit::apply_props`).
    track_locked: bool,
    tab: usize,
}

const W: f32 = 120.0;

impl EventProps {
    /// For event `id`; None when it is gone.
    pub fn open(project: &Project, id: &str) -> Option<Self> {
        let r = project.find_clip(id)?;
        let track_locked = project.timeline.tracks(r.is_video)[r.track_idx].locked;
        Some(Self { id: id.to_string(), draft: project.clip(id)?.clone(), is_video: r.is_video, track_locked, tab: 0 })
    }

    /// Open on the Media page (screenshot harness).
    pub fn media_tab(self) -> Self {
        Self { tab: 1, ..self }
    }

    /// Some(true) on OK, Some(false) on Cancel, × or Esc; None while open.
    pub fn show(&mut self, ctx: &Context, project: &Project) -> Option<bool> {
        let fps = project.settings.frame_rate;
        let (is_video, track_locked, tab, d) = (self.is_video, self.track_locked, &mut self.tab, &mut self.draft);
        let (hit, close) = super::modal(ctx, "event_props", "Properties", vec2(460.0, 520.0), |ui| {
            let row: &[&str] = &[if is_video { "Video Event" } else { "Audio Event" }, "Media"];
            super::tabs(ui, &[row], tab);
            super::page(ui, 410.0, |ui| {
                if *tab == 0 {
                    event_page(ui, d, is_video, track_locked, fps);
                } else {
                    let m = d.media_id.as_deref().and_then(|id| project.media_pool.iter().find(|m| m.id == id));
                    media_page(ui, m, fps);
                }
            });
            ui.add_space(8.0);
            super::button_row(ui, &[("OK", true), ("Cancel", true)])
        });
        match hit {
            Some(k) => Some(k == 0),
            None => close.then_some(false),
        }
    }
}

fn event_page(ui: &mut Ui, d: &mut Clip, is_video: bool, track_locked: bool, fps: f64) {
    // A locked event, or any event on a locked track, keeps everything but its Lock switch.
    let on = !d.locked && !track_locked;
    if track_locked {
        ui.label("The track is locked: only the event's Lock switch can change.");
    }
    super::row(ui, "Active take:", W, on && !d.takes.is_empty(), |ui| {
        let mut k = None;
        ComboBox::from_id_salt("ev_take").width(250.0).selected_text(d.name.as_str()).show_ui(ui, |ui| {
            let _ = ui.selectable_label(true, d.name.as_str()); // the active take
            for (i, t) in d.takes.iter().enumerate() {
                if ui.selectable_label(false, t.name.as_str()).clicked() {
                    k = Some(i);
                }
            }
        });
        if let Some(k) = k {
            edit::choose_take(d, k);
        }
    });
    super::row(ui, "Take name:", W, on, |ui| ui.add(TextEdit::singleline(&mut d.name).desired_width(250.0)));
    super::group(ui, "Time");
    super::row(ui, "Position:", W, on, |ui| tc_edit(ui, "ev_pos", &mut d.position, fps));
    super::row(ui, "Length:", W, on, |ui| tc_edit(ui, "ev_len", &mut d.duration, fps));
    super::row(ui, "In point:", W, on, |ui| tc_edit(ui, "ev_in", &mut d.in_point, fps));
    super::row(ui, "Playback rate:", W, on, |ui| {
        let mut rate = d.rate;
        if num_edit(ui, &mut rate, edit::MIN_RATE..=edit::MAX_RATE, 0.01, 3, "") {
            edit::apply_rate(d, rate);
        }
    });
    super::group(ui, "Level and fades");
    if is_video {
        // Hidden on the timeline too when an opacity envelope has several points.
        let single = d.envelopes.iter().find(|e| e.kind == crate::project::EnvKind::Opacity).is_none_or(|e| e.points.keys.len() <= 1);
        super::row(ui, "Opacity:", W, on && single, |ui| {
            let mut pct = d.opacity() * 100.0;
            if num_edit(ui, &mut pct, 0.0..=100.0, 0.5, 1, " %") {
                d.set_opacity(pct / 100.0);
            }
        });
    } else {
        super::row(ui, "Gain:", W, on, |ui| num_edit(ui, &mut d.gain_db, -120.0..=0.0, 0.1, 1, " dB"));
    }
    for (label, len, curve, id) in [("Fade in:", &mut d.fade_in, &mut d.fade_in_curve, "ev_fi"), ("Fade out:", &mut d.fade_out, &mut d.fade_out_curve, "ev_fo")] {
        super::row(ui, label, W, on, |ui| {
            tc_edit(ui, id, len, fps);
            curve_combo(ui, id, curve);
        });
    }
    super::group(ui, "Switches");
    ui.horizontal(|ui| {
        ui.add_enabled(on, Checkbox::new(&mut d.reverse, "Reverse"));
        ui.add_enabled(on, Checkbox::new(&mut d.muted, "Mute"));
        ui.checkbox(&mut d.locked, "Lock");
        ui.add_enabled(false, Checkbox::new(&mut true, "Loop")).on_disabled_hover_text(Later("R3a").text());
    });
    // VEGAS switches Milan Pro does not have.
    ui.add_enabled_ui(false, |ui| {
        ui.horizontal(|ui| {
            let r3b = Later("R3b").text();
            if is_video {
                ui.checkbox(&mut true, "Maintain aspect ratio").on_disabled_hover_text(&r3b);
                ui.checkbox(&mut false, "Undersample rate").on_disabled_hover_text(&r3b);
            } else {
                ui.checkbox(&mut false, "Normalize").on_disabled_hover_text(&r3b);
                ui.checkbox(&mut false, "Pitch change").on_disabled_hover_text(Later("R4b").text());
            }
        });
    });
}

fn curve_combo(ui: &mut Ui, id: &str, c: &mut Curve) {
    const CURVES: [(&str, Curve); 5] = [("Linear", Curve::Linear), ("Fast", Curve::Fast), ("Slow", Curve::Slow), ("Smooth", Curve::Smooth), ("Sharp", Curve::Sharp)];
    let name = CURVES.iter().find(|k| k.1 == *c).map_or("Hold", |k| k.0);
    ComboBox::from_id_salt((id, "curve")).width(90.0).selected_text(name).show_ui(ui, |ui| {
        for (n, k) in CURVES {
            ui.selectable_value(c, k, n);
        }
    });
}

/// A number field (DragValue): true when `v` changed by more than the `decimals` shown, so clicking in and out of
/// the field keeps the exact value.
fn num_edit<T: emath::Numeric + std::fmt::Display>(ui: &mut Ui, v: &mut T, range: std::ops::RangeInclusive<T>, speed: f64, decimals: usize, suffix: &str) -> bool {
    let mut x = *v;
    let changed = ui.add(DragValue::new(&mut x).range(range).speed(speed).fixed_decimals(decimals).suffix(suffix)).changed();
    let edited = changed && format!("{x:.decimals$}") != format!("{:.decimals$}", *v);
    if edited {
        *v = x;
    }
    edited
}

/// A timecode field: shows `v` and sets it from what was typed when the field loses focus (unparsable text is dropped;
/// text left as shown keeps the exact value, which may lie between frames).
fn tc_edit(ui: &mut Ui, id: &str, v: &mut f64, fps: f64) {
    let id = ui.id().with(id);
    let buf_id = id.with("buf");
    let shown = format_timecode(*v, fps);
    let mut buf = ui.data_mut(|d| d.get_temp::<String>(buf_id)).unwrap_or_else(|| shown.clone());
    let r = ui.add(TextEdit::singleline(&mut buf).id(id).desired_width(110.0));
    if r.lost_focus() {
        if let Some(t) = parse_timecode(&buf, fps).filter(|_| buf.trim() != shown) {
            *v = t;
        }

        ui.data_mut(|d| d.remove::<String>(buf_id));
    } else if r.has_focus() {
        ui.data_mut(|d| d.insert_temp(buf_id, buf));
    }
}

fn media_page(ui: &mut Ui, m: Option<&MediaItem>, fps: f64) {
    let Some(m) = m else {
        ui.label("This event has no media.");
        return;
    };
    let opt = |v: Option<String>| v.unwrap_or_else(|| "-".into());
    let yes = |b: bool| if b { "Yes" } else { "No" }.to_string();
    let rows = [
        ("File:", if m.generator.is_some() { "(generated media)".into() } else { m.path.display().to_string() }),
        ("Name:", m.name.clone()),
        ("Type:", format!("{:?}", m.media_type)),
        ("Length:", format_timecode(m.duration, fps)),
        ("Frame size:", opt(m.width.zip(m.height).map(|(w, h)| format!("{w}x{h}")))),
        ("Frame rate:", opt(m.frame_rate.map(|f| format!("{f:.3} fps")))),
        ("Sample rate:", opt(m.sample_rate.map(|s| format!("{s} Hz")))),
        ("Codec:", opt(m.codec.clone())),
        ("Rotation:", format!("{} degrees", m.rotation)),
        ("Alpha channel:", yes(m.has_alpha)),
        ("Interlaced:", yes(m.interlaced)),
        ("Color matrix:", opt(m.color_matrix.clone())),
    ];
    Grid::new("ev_media").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
        for (k, v) in rows {
            ui.label(k);
            ui.add(Label::new(v).wrap());
            ui.end_row();
        }
    });
}

/// Inverse of `format_timecode`: "hh:mm:ss:ff" (drop-frame "hh:mm:ss;ff" at 29.97 and 59.94, as shown), the
/// shorter "mm:ss:ff" and "ss:ff", or plain seconds ("1.5"). None when it is not a time.
pub fn parse_timecode(s: &str, fps: f64) -> Option<f64> {
    let s = s.trim();
    if !s.contains([':', ';']) {
        return s.parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0);
    }
    let nominal = fps.round() as i64;
    let parts: Vec<i64> = s.split([':', ';']).map(|p| p.trim().parse().ok()).collect::<Option<_>>()?;
    if nominal <= 0 || parts.len() > 4 || parts.iter().any(|&p| p < 0) {
        return None;
    }
    let mut p = [0; 4];
    p[4 - parts.len()..].copy_from_slice(&parts);
    let [h, m, sec, ff] = p;
    let mins = h * 60 + m;
    let mut frames = (mins * 60 + sec) * nominal + ff;
    if fps.fract() != 0.0 && (nominal == 30 || nominal == 60) {
        // Frame numbers 0..k are skipped at each minute except every tenth (SMPTE 12M).
        frames -= nominal / 15 * (mins - mins / 10);
    }
    Some(frames as f64 / fps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timecode_parses_what_format_shows() {
        for fps in [29.97, 25.0, 59.94, 23.976, 30.0] {
            for n in [0u32, 1, 29, 1799, 1800, 1801, 17982, 107892, 123456] {
                let t = n as f64 / fps;
                let back = parse_timecode(&format_timecode(t, fps), fps).unwrap();
                assert!((back - t).abs() < 1e-9, "{fps} fps frame {n}: {} -> {back}", format_timecode(t, fps));
            }
        }
        assert_eq!(parse_timecode("1.5", 25.0), Some(1.5));
        assert_eq!(parse_timecode("02:10", 25.0), Some(2.4)); // ss:ff
        assert_eq!(parse_timecode("1:00:00", 25.0), Some(60.0)); // mm:ss:ff
        assert!(parse_timecode("abc", 25.0).is_none() && parse_timecode("1:-2", 25.0).is_none() && parse_timecode("-1", 25.0).is_none());
    }
}
