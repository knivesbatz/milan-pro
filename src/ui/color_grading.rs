//! Color Grading panel (§5.13, VP23): Lift / Gamma / Gain / Offset wheels, curves and LUTs on the first Color Grading
//! effect of the selected video event, the focus video track or the Video Output. The effect is added to that chain
//! on the first edit; each gesture is one undo step. VP16 users reach the same effect through the Video FX window.
use eframe::egui::*;
use std::f32::consts::TAU;
use std::path::Path;
use crate::edit::{self, FxTarget};
use crate::engine::plugins::{self, FxEval, GRADING, ParamKind};
use crate::engine::{lut, vfx};
use crate::history::History;
use crate::project::{Fx, Project};
use super::Selection;
use super::fx_window::{animated, field_style, key_time, set_num};
use super::theme;
use super::workspace;

#[derive(Default)]
pub struct ColorGrading {
    /// 0 the selected event, 1 the focus video track, 2 the Video Output.
    target: usize,
    gesture: bool,
    /// Curves channel: 0 master, 1 R, 2 G, 3 B.
    channel: usize,
    /// The curve point being dragged.
    drag: Option<usize>,
}

const TARGETS: [&str; 3] = ["Event FX:", "Track FX:", "Video Output FX"];
/// (title, key prefix, neutral value, R/G/B change with the puck at the rim).
const WHEELS: [(&str, &str, f32, f32); 4] = [("Lift", "lift", 0.0, 0.25), ("Gamma", "gamma", 1.0, 0.5), ("Gain", "gain", 1.0, 0.5), ("Offset", "offset", 0.0, 0.25)];
const S3: f32 = 0.866_025_4;
/// Screen directions of R, G and B on a wheel: red up, green lower left, blue lower right.
const AXES: [Vec2; 3] = [vec2(0.0, -1.0), vec2(-S3, 0.5), vec2(S3, 0.5)];

/// R, G and B with the puck at `puck` (the unit disc): `mean` plus the puck's pull along each colour's axis.
pub fn rgb_of(puck: Vec2, mean: f32, scale: f32) -> [f32; 3] {
    std::array::from_fn(|i| mean + scale * puck.dot(AXES[i]))
}

/// Where the puck sits for `rgb` (the inverse of `rgb_of`; the common part is the Y slider's business).
pub fn puck_of(rgb: [f32; 3], scale: f32) -> Vec2 {
    AXES.iter().zip(rgb).fold(Vec2::ZERO, |s, (a, v)| s + *a * v) * (2.0 / (3.0 * scale))
}

/// Curve points as the Color Grading effect stores them ("x:y;x:y").
pub fn points_text(pts: &[(f32, f32)]) -> String {
    pts.iter().map(|(x, y)| format!("{x:.4}:{y:.4}")).collect::<Vec<_>>().join(";")
}

fn target(st: &ColorGrading, p: &Project, sel: &Selection) -> Option<FxTarget> {
    match st.target {
        0 => sel.clips.last().filter(|id| p.find_clip(id).is_some_and(|r| r.is_video)).map(|id| FxTarget::Event(id.clone())),
        1 => sel.tracks.iter().rev().find(|id| matches!(p.find_track(id), Some((true, _)))).map(|id| FxTarget::Track(id.clone())),
        _ => Some(FxTarget::VideoOutput),
    }
}

/// Toolbar: which chain is graded, and its name.
pub fn color_grading_toolbar(ui: &mut Ui, st: &mut ColorGrading, p: &Project, sel: &Selection) {
    ComboBox::from_id_salt("cg_target").width(120.0).selected_text(TARGETS[st.target]).show_ui(ui, |ui| {
        for (k, n) in TARGETS.iter().enumerate() {
            ui.selectable_value(&mut st.target, k, *n);
        }
    });
    let name = match target(st, p, sel) {
        Some(FxTarget::Event(id)) => p.clip(&id).map_or(String::new(), |c| c.name.clone()),
        Some(FxTarget::Track(id)) => format!("Track {}", p.find_track(&id).map_or(0, |t| t.1 + 1)),
        Some(_) => String::new(),
        None => "(none selected)".into(),
    };
    ui.label(name);
}

/// The panel body; a status message (LUT export).
pub fn draw_color_grading(ui: &mut Ui, st: &mut ColorGrading, project: &mut Project, history: &mut History, sel: &Selection) -> Option<String> {
    let t = theme::get();
    let Some(target) = target(st, project, sel) else {
        let what = if st.target == 0 { "a video event" } else { "a video track" };
        ui.painter().text(ui.max_rect().center(), Align2::CENTER_CENTER, format!("Select {what} to grade."), t.font(t.font.ui), t.text_dim);
        return None;
    };
    let (mut list, editable) = edit::fx_list(project, &target)?;
    let info = plugins::find(GRADING).expect("built in");
    let k = list.iter().position(|f| f.plugin == GRADING);
    let mut fx = k.map_or_else(|| plugins::new_fx(info, None), |k| list[k].clone());
    let fps = project.settings.frame_rate;
    let at = key_time(project, &target).map_or(0.0, |k| k.0);
    // An effect animated elsewhere is keyed at the cursor; otherwise the panel sets plain values.
    let key_at = animated(&fx).then_some(at);
    let e = plugins::eval(&fx, at).expect("built in");
    let before = fx.clone();
    let mut msg = None;

    ScrollArea::both().id_salt("cg_scroll").auto_shrink([false, false]).show(ui, |ui| {
        field_style(ui);
        ui.add_enabled_ui(editable, |ui| {
            ui.horizontal_top(|ui| {
                for (title, prefix, neutral, scale) in WHEELS {
                    ui.vertical(|ui| wheel(ui, &mut fx, &e, title, prefix, neutral, scale, key_at, fps));
                    ui.add_space(10.0);
                }
            });
            ui.separator();
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| curves(ui, st, &mut fx, &e));
                ui.add_space(16.0);
                ui.vertical(|ui| {
                    for (label, key) in [("Input LUT:", "input_lut"), ("Look LUT:", "look_lut")] {
                        let path = e.t(key).trim().to_string();
                        ui.horizontal(|ui| {
                            ui.label(label);
                            ui.label(Path::new(&path).file_name().map_or("(none)".into(), |n| n.to_string_lossy().into_owned()));
                            if ui.button("Browse...").clicked() {
                                if let Some(p) = rfd::FileDialog::new().add_filter("Cube LUT", &["cube"]).pick_file() {
                                    fx.text.insert(key.into(), p.display().to_string());
                                }
                            }
                            if ui.add_enabled(!path.is_empty(), Button::new("Clear")).clicked() {
                                fx.text.remove(key);
                            }
                        });
                        if let Some(err) = Some(&path).filter(|p| !p.is_empty()).and_then(|p| lut::load(Path::new(p)).err()) {
                            ui.colored_label(t.warning_text, err);
                        }
                    }
                    ui.add_space(8.0);
                    for key in ["saturation", "temperature", "tint"] {
                        let spec = info.params.iter().find(|s| s.key == key).expect("grading parameter");
                        if let ParamKind::Float { min, max, .. } = spec.kind {
                            let mut v = e.f(key);
                            if ui.add(Slider::new(&mut v, min..=max).text(spec.name)).changed() {
                                set_num(&mut fx, key, v, e.f(key), key_at, fps);
                            }
                        }
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Export LUT...").clicked() {
                            if let Some(path) = rfd::FileDialog::new().add_filter("Cube LUT", &["cube"]).set_file_name("Grade.cube").save_file() {
                                msg = Some(match lut::export_cube(&e, &path, 33) {
                                    Ok(()) => format!("LUT exported to {}", path.display()),
                                    Err(err) => format!("Could not export the LUT: {err}"),
                                });
                            }
                        }
                        if ui.button("Reset All").clicked() {
                            fx.params.clear();
                            fx.text.clear();
                        }
                    });
                });
            });
        });
    });

    let changed = editable && fx != before;
    if changed {
        match k {
            Some(k) => list[k] = fx,
            None => list.push(fx),
        }
    }
    let active = ui.input(|i| i.pointer.any_down()) || ui.ctx().wants_keyboard_input();
    workspace::commit(project, history, &mut st.gesture, active, changed, |p| {
        edit::set_fx_list(p, &target, list);
    });
    msg
}

/// One wheel: title (with * when changed) and reset, the hue ring with its puck, a Y slider and R / G / B / Y fields.
#[allow(clippy::too_many_arguments)]
fn wheel(ui: &mut Ui, fx: &mut Fx, e: &FxEval, title: &str, prefix: &str, neutral: f32, scale: f32, key_at: Option<f64>, fps: f64) {
    let t = theme::get();
    let keys = ["r", "g", "b", "y"].map(|c| format!("{prefix}.{c}"));
    let vals = keys.each_ref().map(|k| e.f(k));
    let changed = vals.iter().any(|v| *v != neutral);
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{title}{}", if changed { " *" } else { "" })).strong());
        if ui.add_enabled(changed, Button::new("↺").small()).on_hover_text("Reset").clicked() {
            for k in &keys {
                fx.params.remove(k);
            }
        }
    });
    let size = 120.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click_and_drag());
    let (c, outer) = (rect.center(), size * 0.5 - 2.0);
    let inner = outer - 8.0;
    let p = ui.painter();
    // Hue ring: red up, then blue and green clockwise, as `AXES`.
    let mut mesh = Mesh::default();
    let n = 60;
    for i in 0..=n {
        let a = TAU * i as f32 / n as f32;
        let dir = Vec2::angled(a);
        let hue = (-(a.to_degrees() + 90.0) / 360.0).rem_euclid(1.0);
        let col: Color32 = ecolor::Hsva::new(hue, 0.75, 0.9, 1.0).into();
        mesh.colored_vertex(c + dir * inner, col);
        mesh.colored_vertex(c + dir * outer, col);
        if i > 0 {
            let k = 2 * i as u32;
            mesh.add_triangle(k - 2, k - 1, k);
            mesh.add_triangle(k - 1, k, k + 1);
        }
    }
    p.add(mesh);
    p.circle_filled(c, inner, t.panel_bg.gamma_multiply(0.7));
    p.hline(Rangef::new(c.x - 4.0, c.x + 4.0), c.y, Stroke::new(1.0_f32, t.text_dim));
    p.vline(c.x, Rangef::new(c.y - 4.0, c.y + 4.0), Stroke::new(1.0_f32, t.text_dim));
    let rgb = [vals[0], vals[1], vals[2]];
    let puck = puck_of(rgb, scale);
    let at = c + puck * inner;
    p.circle(at, 4.0, Color32::WHITE, Stroke::new(1.0_f32, Color32::BLACK));
    let mean = (rgb[0] + rgb[1] + rgb[2]) / 3.0;
    let new = if resp.double_clicked() {
        Some(Vec2::ZERO)
    } else if resp.dragged() || resp.clicked() {
        resp.interact_pointer_pos().map(|q| {
            let v = (q - c) / inner;
            if v.length() > 1.0 { v.normalized() } else { v }
        })
    } else {
        None
    };
    if let Some(v) = new {
        for (i, x) in rgb_of(v, mean, scale).into_iter().enumerate() {
            set_num(fx, &keys[i], x, vals[i], key_at, fps);
        }
    }
    // The Y slider and the four fields, in each parameter's range.
    let range = |k: &str| match plugins::find(GRADING).and_then(|i| i.params.iter().find(|s| s.key == k)).map(|s| s.kind) {
        Some(ParamKind::Float { min, max, .. }) => min..=max,
        _ => 0.0..=1.0,
    };
    let mut y = vals[3];
    if ui.add(Slider::new(&mut y, range(&keys[3])).text("Y").show_value(false)).changed() {
        set_num(fx, &keys[3], y, vals[3], key_at, fps);
    }
    ui.horizontal(|ui| {
        for (i, k) in keys.iter().enumerate() {
            let mut v = vals[i];
            let dv = DragValue::new(&mut v).speed(0.005).range(range(k)).fixed_decimals(3).prefix(["R ", "G ", "B ", "Y "][i]).update_while_editing(false);
            if ui.add(dv).changed() {
                set_num(fx, k, v, vals[i], key_at, fps);
            }
        }
    });
}

/// The curves editor: a click adds a point, a drag moves one (between its neighbours), a right-click removes one.
fn curves(ui: &mut Ui, st: &mut ColorGrading, fx: &mut Fx, e: &FxEval) {
    let t = theme::get();
    ui.horizontal(|ui| {
        ui.label(RichText::new("Curves").strong());
        for (k, name) in ["Master", "R", "G", "B"].into_iter().enumerate() {
            if ui.selectable_label(st.channel == k, name).clicked() {
                (st.channel, st.drag) = (k, None);
            }
        }
    });
    let key = ["curve.m", "curve.r", "curve.g", "curve.b"][st.channel];
    let mut pts = vfx::parse_points(e.t(key));
    let (rect, resp) = ui.allocate_exact_size(vec2(200.0, 200.0), Sense::click_and_drag());
    let to_screen = |(x, y): (f32, f32)| pos2(rect.min.x + x * rect.width(), rect.max.y - y * rect.height());
    let from_screen = |q: Pos2| (((q.x - rect.min.x) / rect.width()).clamp(0.0, 1.0), ((rect.max.y - q.y) / rect.height()).clamp(0.0, 1.0));
    let hit = |pts: &[(f32, f32)], q: Pos2| pts.iter().position(|&p| to_screen(p).distance(q) <= 7.0);
    let mut edited = false;
    // egui clears press_origin on release, the frame `clicked` is true in.
    let q = if resp.drag_started() { ui.input(|i| i.pointer.press_origin()) } else if resp.clicked() { resp.interact_pointer_pos() } else { None };
    if let Some(q) = q {
        st.drag = hit(&pts, q);
        if st.drag.is_none() {
            // The first point keeps the curve straight between the corners.
            if pts.is_empty() {
                pts = vec![(0.0, 0.0), (1.0, 1.0)];
            }
            let p = from_screen(q);
            pts.retain(|o| o.0 != p.0);
            pts.push(p);
            pts.sort_by(|a, b| a.0.total_cmp(&b.0));
            st.drag = pts.iter().position(|o| *o == p);
            edited = true;
        }
    }
    if let (Some(i), Some(q)) = (st.drag.filter(|&i| i < pts.len()), resp.interact_pointer_pos().filter(|_| resp.dragged())) {
        let (x, y) = from_screen(q);
        let lo = if i > 0 { pts[i - 1].0 + 0.005 } else { 0.0 };
        let hi = pts.get(i + 1).map_or(1.0, |n| n.0 - 0.005);
        pts[i] = (x.clamp(lo, hi.max(lo)), y);
        edited = true;
    }
    if resp.drag_stopped() || resp.clicked() {
        st.drag = None;
    }
    if let Some(i) = resp.interact_pointer_pos().filter(|_| resp.secondary_clicked()).and_then(|q| hit(&pts, q)) {
        pts.remove(i);
        edited = true;
    }
    if edited {
        fx.text.insert(key.into(), points_text(&pts));
    }
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, t.panel_bg.gamma_multiply(0.7));
    for k in 1..4 {
        let f = k as f32 / 4.0;
        p.vline(rect.min.x + f * rect.width(), rect.y_range(), Stroke::new(1.0_f32, t.text_dim.gamma_multiply(0.3)));
        p.hline(rect.x_range(), rect.min.y + f * rect.height(), Stroke::new(1.0_f32, t.text_dim.gamma_multiply(0.3)));
    }
    let ink = [Color32::WHITE, Color32::from_rgb(0xDC, 0x50, 0x50), Color32::from_rgb(0x50, 0xC8, 0x78), Color32::from_rgb(0x3C, 0x8C, 0xDC)][st.channel];
    let line: Vec<Pos2> = (0..=64).map(|i| i as f32 / 64.0).map(|x| to_screen((x, vfx::monotone(&pts, x).clamp(0.0, 1.0)))).collect();
    p.add(Shape::line(line, Stroke::new(1.5_f32, ink)));
    for &q in &pts {
        p.circle(to_screen(q), 3.5, ink, Stroke::new(1.0_f32, Color32::BLACK));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_puck_and_rgb_round_trip() {
        for (v, mean, scale) in [(vec2(0.3, -0.4), 0.0, 0.25), (vec2(-0.7, 0.1), 1.2, 0.5), (Vec2::ZERO, 1.0, 0.5)] {
            let rgb = rgb_of(v, mean, scale);
            assert!(((rgb[0] + rgb[1] + rgb[2]) / 3.0 - mean).abs() < 1e-5, "the puck keeps the mean");
            assert!((puck_of(rgb, scale) - v).length() < 1e-4);
        }
        // Up is red.
        let up = rgb_of(vec2(0.0, -1.0), 0.0, 0.25);
        assert!(up[0] > 0.0 && up[1] < 0.0 && up[2] < 0.0);
    }

    #[test]
    fn curve_points_round_trip_through_the_stored_text() {
        let pts = vec![(0.0, 0.0), (0.25, 0.4), (1.0, 1.0)];
        assert_eq!(vfx::parse_points(&points_text(&pts)), pts);
        assert_eq!(points_text(&[]), "");
    }
}
