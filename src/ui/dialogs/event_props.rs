//! Event Properties (event menu › Properties...): the event page edits a copy of the event, applied in one undo step
//! on OK; the media page shows the active take's media. A field only changes the value when its text is edited, so
//! focusing it does not round an off-frame value to the frame or rate shown.
use eframe::egui::*;
use crate::edit;
use crate::project::{Clip, Curve, MediaItem, Project, format_timecode, parse_timecode};

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
        ui.add_enabled(on, Checkbox::new(&mut d.looping, "Loop"));
    });
    ui.horizontal(|ui| {
        if is_video {
            ui.add_enabled(on, Checkbox::new(&mut d.keep_aspect, "Maintain aspect ratio"));
            ui.add_enabled(on, Checkbox::new(&mut d.reduce_flicker, "Reduce interlace flicker"));
        } else {
            ui.add_enabled(on, Checkbox::new(&mut d.invert, "Invert phase"));
            ui.add_enabled(on, Checkbox::new(&mut d.normalize, "Normalize"));
            if crate::ui::theme::get().look() == crate::ui::theme::UiLook::Vegas23 {
                ui.add_enabled(on, Checkbox::new(&mut d.auto_normalize, "Auto normalize"));
            }
        }
    });
    if is_video {
        // Undersample rate: the event plays at this share of the project frame rate.
        ui.add_enabled_ui(on, |ui| ui.horizontal(|ui| {
            let mut under = d.undersample.is_some();
            if ui.checkbox(&mut under, "Undersample rate").changed() {
                d.undersample = under.then_some(0.5);
            }
            if let Some(r) = &mut d.undersample {
                ui.add(DragValue::new(r).range(0.05..=0.99).speed(0.01).fixed_decimals(2));
            }
        }));
    } else {
        super::row(ui, "Normalize gain:", W, on && (d.normalize || d.auto_normalize), |ui| {
            ui.label(match &d.normalized {
                Some(n) => format!("{:.2} dB", n.db),
                None => "-".into(),
            });
            // Measured again (in the background) after OK.
            if ui.button("Recalculate").clicked() {
                d.normalized = None;
            }
        });
        // VEGAS's Pitch change, in semitones: the pitch moves, the length does not.
        super::row(ui, "Pitch change:", W, on, |ui| {
            let max = crate::edit::PITCH_MAX / 100.0;
            let mut st = d.pitch_cents / 100.0;
            if num_edit(ui, &mut st, -max..=max, 0.05, 2, " semitones") {
                d.pitch_cents = (st * 100.0).round();
            }
        });
    }
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
    super::time_edit(ui, id, v, |t| format_timecode(t, fps), |s| parse_timecode(s, fps));
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

