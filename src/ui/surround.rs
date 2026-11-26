//! R10b: the Surround Panner window (VEGAS's, opened from an audio track's surround pan control in a 5.1 / 7.1
//! project). The room seen from above, the front at the top: a speaker icon per channel of the master bus mode (a
//! click switches one off or on), the pan point (drag it; a double-click returns it to the front centre), the pan
//! type (Smooth: the track's own Pan Type; Constant Power), Smoothness and the LFE-only switch. Under the room the
//! keyframe controller: with keyframes the pan point is keyed at the cursor (the track's Surround X / Y envelopes,
//! the same keyframes the timeline shows and the mixer plays); without, it is the track's static pan.
use eframe::egui::*;
use crate::engine::surround;
use crate::history::History;
use crate::project::{BusMode, Curve, EnvKind, Envelope, Keyframe, Keyframes, PanType, Project, SurroundPan};
use crate::ui::widgets;
use super::dialogs;
use super::keyframes::{self, KfState};
use super::theme;

pub const ID: &str = "surround_panner";
const SIZE: Vec2 = vec2(520.0, 560.0);

/// Where each speaker stands in the room (x left to right, y back to front, -1..1) for a layout of `ch` channels.
pub fn speakers(ch: usize) -> Vec<(usize, Pos2)> {
    use surround::*;
    let mut v = vec![(FL, pos2(-0.8, 0.9)), (FC, pos2(0.0, 1.0)), (FR, pos2(0.8, 0.9)), (LFE, pos2(0.35, 1.0))];
    if ch > SR {
        v.extend([(SL, pos2(-1.0, 0.0)), (SR, pos2(1.0, 0.0)), (BL, pos2(-0.6, -1.0)), (BR, pos2(0.6, -1.0))]);
    } else {
        v.extend([(BL, pos2(-0.9, -0.9)), (BR, pos2(0.9, -0.9))]);
    }
    v
}

/// The gain each speaker gives a centred mono signal (the track's L and R at 1) at pan `sp`, in channel order.
pub fn speaker_gains(pt: PanType, sp: &SurroundPan, ch: usize) -> Vec<f32> {
    let g = surround::pan_matrix(pt, sp, 0.0, ch);
    (0..ch).map(|c| 0.5 * (g[0][c] + g[1][c])).collect()
}

pub struct SurroundWin {
    /// The audio track's id.
    pub track: String,
    kf: KfState,
    gesture: bool,
}

/// The value of the Surround X or Y envelope `kind` in `envs` at `t`, else `fallback`.
fn env_at(envs: &[Envelope], kind: EnvKind, t: f64, fallback: f32) -> f32 {
    envs.iter().find(|e| e.kind == kind && !e.points.keys.is_empty()).map_or(fallback, |e| e.at(t))
}

/// Key both Surround envelopes at `t` to `(x, y)`; an envelope made now starts with the static pan at 0.
pub fn key_pan(envs: &mut Vec<Envelope>, sp: &SurroundPan, t: f64, (x, y): (f32, f32), fps: f64) {
    for (kind, v, start) in [(EnvKind::SurroundX, x, sp.x), (EnvKind::SurroundY, y, sp.y)] {
        let i = match envs.iter().position(|e| e.kind == kind) {
            Some(i) => i,
            None => {
                envs.push(Envelope { kind, points: Keyframes::default() });
                envs.len() - 1
            }
        };
        let keys = &mut envs[i].points;
        if keys.keys.is_empty() && t.abs() >= 0.5 / fps.max(1.0) {
            keys.keys.push(Keyframe { t: 0.0, v: start, curve: Curve::Smooth });
        }
        keyframes::set_at(keys, t, v, fps);
    }
}

impl SurroundWin {
    /// For audio track `id`; None when it is gone or a video track.
    pub fn open(project: &Project, id: &str) -> Option<Self> {
        matches!(project.find_track(id), Some((false, _))).then(|| Self { track: id.to_string(), kf: KfState::default(), gesture: false })
    }

    /// Draw the window and apply its edits (one undo step per gesture). False once it is closed.
    pub fn show(&mut self, ctx: &Context, project: &mut Project, history: &mut History) -> bool {
        let Some((false, ti)) = project.find_track(&self.track) else { return false };
        let tr = &project.timeline.audio_tracks[ti];
        let editable = !tr.locked;
        let fps = project.settings.frame_rate;
        let mode = project.settings.layout;
        // A stereo project shows the 5.1 room (the pan is kept for when the master bus mode is surround).
        let ch = if mode == BusMode::Stereo { 6 } else { mode.channels() };
        let at = self.kf.cursor(project.timeline.cursor_position).max(0.0);
        let (mut sp, mut envs, pt) = (tr.surround, tr.envelopes.clone(), tr.pan_type);
        let keyed = envs.iter().any(|e| matches!(e.kind, EnvKind::SurroundX | EnvKind::SurroundY) && !e.points.keys.is_empty());
        let cur = (env_at(&envs, EnvKind::SurroundX, at, sp.x), env_at(&envs, EnvKind::SurroundY, at, sp.y));
        let span = crate::render::content_end(project).max(at).max(1.0);
        let title = format!("Surround Panner (Track {})", ti + 1);
        let t = theme::get();
        let mut moved: Option<(f32, f32)> = None;
        let mut seek = None;

        let (_, close) = dialogs::tool_window(ctx, ID, &title, SIZE, |ui| {
            let full = ui.available_rect_before_wrap();
            let bar = full.with_max_y(full.min.y + 26.0);
            let kf_r = full.with_min_y(full.max.y - keyframes::HEIGHT);
            let info = Rect::from_min_max(pos2(full.min.x, kf_r.min.y - 22.0), pos2(full.max.x, kf_r.min.y - 4.0));
            let area = Rect::from_min_max(pos2(full.min.x + 8.0, bar.max.y + 8.0), pos2(full.max.x - 8.0, info.min.y - 4.0));
            let side = area.width().min(area.height());
            let room = Rect::from_center_size(area.center(), vec2(side, side));
            widgets::metal_bar(ui.painter(), bar, &t.metal);
            let mut row = ui.new_child(UiBuilder::new().max_rect(bar.shrink2(vec2(6.0, 0.0))).layout(Layout::left_to_right(Align::Center)));
            row.add_enabled_ui(editable, |ui| {
                ui.label("Pan type:");
                let name = if sp.constant_power { "Constant Power" } else { "Smooth" };
                widgets::combo(ui, "sp_type", 120.0, name, |ui| {
                    ui.selectable_value(&mut sp.constant_power, false, "Smooth");
                    ui.selectable_value(&mut sp.constant_power, true, "Constant Power");
                })
                .response
                .on_hover_text("Smooth pans by the track's Pan Type; Constant Power keeps the loudness the same anywhere in the room");
                ui.add_space(8.0);
                ui.label("Smoothness:");
                let mut pc = sp.smooth * 100.0;
                if ui.add(DragValue::new(&mut pc).range(0.0..=100.0).speed(0.5).suffix(" %")).on_hover_text("Pulls the sound towards the middle of the room: at 100% it is heard everywhere alike").changed() {
                    sp.smooth = pc / 100.0;
                }
                ui.add_space(8.0);
                widgets::checkbox(ui, &mut sp.lfe, "LFE only").on_hover_text("The track goes to the LFE channel alone");
            });

            // The room: a square seen from above, the listener in the middle.
            let p = ui.painter().with_clip_rect(room.expand(12.0));
            p.rect_filled(room, 0.0, t.workspace_bg);
            p.rect_stroke(room, 0.0, Stroke::new(1.0_f32, t.workspace_frame), StrokeKind::Outside);
            for k in 1..4 {
                let f = k as f32 / 4.0;
                let x = room.min.x + room.width() * f;
                let y = room.min.y + room.height() * f;
                p.vline(x, room.y_range(), Stroke::new(1.0_f32, t.workspace_grid));
                p.hline(room.x_range(), y, Stroke::new(1.0_f32, t.workspace_grid));
            }
            p.circle_stroke(room.center(), 7.0, Stroke::new(1.0_f32, t.workspace_frame));
            p.text(room.center() + vec2(0.0, 12.0), Align2::CENTER_TOP, "Listener", t.font(t.font.meter), t.workspace_frame);
            let to_px = |q: Pos2| pos2(room.center().x + q.x * room.width() * 0.45, room.center().y - q.y * room.height() * 0.45);
            let from_px = |q: Pos2| ((q.x - room.center().x) / (room.width() * 0.45), (room.center().y - q.y) / (room.height() * 0.45));
            let live = SurroundPan { x: cur.0, y: cur.1, ..sp };
            let gains = speaker_gains(pt, &live, ch);
            let names = BusMode::of_channels(ch).unwrap_or(BusMode::Surround51).channel_names();
            for (c, at_) in speakers(ch) {
                let centre = to_px(at_);
                let r = Rect::from_center_size(centre, vec2(30.0, 22.0));
                let on = sp.speaker_on(c);
                let lit = gains.get(c).copied().unwrap_or(0.0);
                let fill = if !on { t.workspace_grid } else if lit > 1e-4 { t.meter_green.lerp_to_gamma(Color32::WHITE, 1.0 - lit.min(1.0)) } else { Color32::WHITE };
                p.rect_filled(r, 3.0, fill);
                p.rect_stroke(r, 3.0, Stroke::new(1.0_f32, if on { t.workspace_frame } else { t.workspace_grid }), StrokeKind::Inside);
                // The cone facing the listener.
                let dir = (room.center() - centre).normalized();
                let tip = centre + dir * 15.0;
                let n = vec2(-dir.y, dir.x) * 6.0;
                p.add(Shape::convex_polygon(vec![centre + dir * 11.0 + n, centre + dir * 11.0 - n, tip], if on { t.workspace_frame } else { t.workspace_grid }, Stroke::NONE));
                p.text(centre, Align2::CENTER_CENTER, names.get(c).copied().unwrap_or("?"), t.font(t.font.meter), if on { Color32::BLACK } else { t.workspace_frame });
                let db = if lit > 1e-5 { format!("{:.1}", 20.0 * lit.log10()) } else { "-Inf".into() };
                p.text(centre + vec2(0.0, 13.0), Align2::CENTER_TOP, db, t.font(t.font.meter), t.workspace_frame);
                let resp = ui.interact(r, ui.id().with(("speaker", c)), Sense::click()).on_hover_text(format!("{}: click to switch it {}", names.get(c).copied().unwrap_or("?"), if on { "off" } else { "on" }));
                if resp.clicked() && editable {
                    sp.off ^= 1 << c;
                }
            }
            // The pan point.
            let puck = to_px(pos2(cur.0, cur.1));
            let resp = ui.interact(Rect::from_center_size(puck, vec2(22.0, 22.0)), ui.id().with("puck"), Sense::click_and_drag());
            let room_resp = ui.interact(room, ui.id().with("room"), Sense::click());
            if editable {
                if resp.double_clicked() {
                    moved = Some((0.0, 1.0));
                } else if let Some(q) = resp.interact_pointer_pos().filter(|_| resp.dragged()) {
                    let (x, y) = from_px(q);
                    moved = Some((x.clamp(-1.0, 1.0), y.clamp(-1.0, 1.0)));
                } else if let Some(q) = room_resp.interact_pointer_pos().filter(|_| room_resp.clicked()) {
                    let (x, y) = from_px(q);
                    moved = Some((x.clamp(-1.0, 1.0), y.clamp(-1.0, 1.0)));
                }
            }
            let ink = if keyed { t.envelope_pan } else { t.envelope_volume };
            p.line_segment([room.center(), puck], Stroke::new(1.0_f32, ink.gamma_multiply(0.5)));
            p.circle_filled(puck, 7.0, ink);
            p.circle_stroke(puck, 7.0, Stroke::new(1.5_f32, Color32::WHITE));
            resp.on_hover_text("The pan point: drag it; a double-click returns it to the front centre");
            let txt = format!(
                "{}   Left/Right {:+.0}%   Front/Rear {:+.0}%   {}",
                if mode == BusMode::Stereo { "The master bus mode is Stereo: Project Properties › Audio sets 5.1 or 7.1." } else { mode.name() },
                cur.0 * 100.0,
                cur.1 * 100.0,
                if keyed { "Keyframed" } else { "" }
            );
            ui.painter().text(info.left_center() + vec2(6.0, 0.0), Align2::LEFT_CENTER, txt, t.font(t.font.ui), t.text);

            // Keyframes: the Surround X envelope's keys (both envelopes are keyed together).
            let lanes: Vec<(f64, Curve)> = envs.iter().find(|e| e.kind == EnvKind::SurroundX).map_or(Vec::new(), |e| e.points.keys.iter().map(|k| (k.t, k.curve)).collect());
            if let Some(r) = keyframes::controller(ui, kf_r, &mut self.kf, "Position", &lanes, span, f64::INFINITY, at, fps) {
                if lanes.is_empty() && r == keyframes::KfResponse::Add {
                    key_pan(&mut envs, &sp, at, cur, fps);
                } else {
                    for e in envs.iter_mut().filter(|e| matches!(e.kind, EnvKind::SurroundX | EnvKind::SurroundY)) {
                        seek = keyframes::apply(r, &mut e.points, at, fps).or(seek);
                    }
                }
            }
            ui.allocate_rect(full, Sense::hover());
        });

        if let Some(xy) = moved {
            if keyed {
                key_pan(&mut envs, &sp, at, xy, fps);
            } else {
                (sp.x, sp.y) = xy;
            }
        }
        envs.retain(|e| !(matches!(e.kind, EnvKind::SurroundX | EnvKind::SurroundY) && e.points.keys.is_empty()));
        let tr = &project.timeline.audio_tracks[ti];
        let changed = editable && (tr.surround != sp || tr.envelopes != envs);
        let down = ctx.input(|i| i.pointer.any_down());
        super::workspace::commit(project, history, &mut self.gesture, down, changed, |p| {
            let tr = &mut p.timeline.audio_tracks[ti];
            (tr.surround, tr.envelopes) = (sp, envs);
        });
        if let Some(s) = seek.and_then(|s| self.kf.seek(s.max(0.0))) {
            project.timeline.cursor_position = s;
        }
        !close
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R10b acceptance: the pan point at the front left gives FL a gain of 1 and every other speaker 0.
    #[test]
    fn front_left_puck_gives_fl_unity_and_nothing_else() {
        let sp = SurroundPan { x: -1.0, y: 1.0, ..Default::default() };
        let g = surround::pan_matrix(PanType::Balance0, &sp, 0.0, 6);
        assert_eq!(g[0][surround::FL], 1.0);
        assert!((1..6).all(|c| g[0][c] == 0.0 && g[1][c] == 0.0), "{g:?}");
        // Constant Power at the middle of the left wall shares the power between front and back.
        let mid = SurroundPan { x: -1.0, y: 0.0, constant_power: true, off: 1 << surround::FC, ..Default::default() };
        let gains = speaker_gains(PanType::Balance0, &mid, 6);
        assert!((gains[surround::FL] - gains[surround::BL]).abs() < 1e-6 && gains[surround::FL] > 0.0);
        assert_eq!(speakers(6).len(), 6);
        assert_eq!(speakers(8).len(), 8);
    }

    /// Keying the pan point makes both Surround envelopes, the first key at 0 holding the static pan, and the plan
    /// plays them.
    #[test]
    fn keyed_pan_point_moves_through_the_envelopes() {
        let mut envs = Vec::new();
        let sp = SurroundPan::default();
        key_pan(&mut envs, &sp, 2.0, (-1.0, -1.0), 30.0);
        assert_eq!(envs.len(), 2);
        let x = envs.iter().find(|e| e.kind == EnvKind::SurroundX).unwrap();
        let y = envs.iter().find(|e| e.kind == EnvKind::SurroundY).unwrap();
        assert_eq!((x.at(0.0), y.at(0.0), x.at(2.0), y.at(2.0)), (0.0, 1.0, -1.0, -1.0));
        let mut p = Project::new("t");
        p.settings.layout = BusMode::Surround51;
        p.timeline.audio_tracks[0].envelopes = envs;
        let m = crate::project::MediaItem { id: "a".into(), path: "a.wav".into(), media_type: crate::project::MediaType::Audio, duration: 10.0, sample_rate: Some(48_000), ..Default::default() };
        p.timeline.audio_tracks[0].clips.push(crate::project::Clip::from_media(&m, 0.0));
        p.media_pool.push(m);
        let plan = crate::engine::plan::compile_audio(&p);
        assert!(plan.tracks[0].x_env.is_some() && plan.tracks[0].y_env.is_some());
    }
}
