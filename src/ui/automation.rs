//! R8c: automation (VEGAS Automation Settings). With Show Automation Controls on and a mode other than Off, the Vol and
//! Pan controls of an audio track or bus (track header, bus track, Mixing Console) show its envelope at the cursor, as
//! does an automated FX parameter's control in the FX window (by the mode alone). Read only shows it. Write (Touch)
//! writes points while the control is held; Write (Latch) writes from the first move until playback stops. Stopped, a
//! move sets the envelope at the cursor (adding the envelope if there is none). Each pass is one undo step. Writes are
//! UI-thread edits; the mix hears them through its plan swap, as any edit while playing.
//! Also the FX Automation chooser (FX Automation Envelopes...).
use eframe::egui::*;
use crate::edit;
use crate::history::History;
use crate::project::{AutoMode, EnvKind, Envelope, Project};

#[derive(Clone)]
struct Rec {
    /// The envelope as the recording found it (key times), and where it punched in.
    orig: Envelope,
    t0: f64,
    pts: Vec<(f64, f32)>,
    last: f64,
}

/// One control writing one envelope.
#[derive(Clone)]
struct Pass {
    owner: String,
    kind: EnvKind,
    latch: bool,
    /// Touched this frame: a Touch pass that is not ends.
    fed: bool,
    v: f32,
    rec: Option<Rec>,
}

#[derive(Clone, Default)]
struct State {
    playing: bool,
    passes: Vec<Pass>,
    /// FX Automation Envelopes...: the track or bus whose chooser is open.
    chooser: Option<String>,
}

fn id() -> Id {
    Id::new("milan_automation")
}

fn with<R>(ctx: &Context, f: impl FnOnce(&mut State) -> R) -> R {
    ctx.data_mut(|d| f(d.get_temp_mut_or_default::<State>(id())))
}

/// The play state this frame; the app calls it before the UI draws.
pub fn begin_frame(ctx: &Context, playing: bool) {
    with(ctx, |s| s.playing = playing);
}

/// What the Vol and Pan controls of audio track or bus `id` do: None, they trim; Some(mode), they are automation
/// controls (Show Automation Controls on, and the mode is not Off).
pub fn control_mode(project: &Project, id: &str) -> Option<AutoMode> {
    edit::automation_of(project, id).filter(|(m, shown)| *shown && m.plays()).map(|(m, _)| m)
}

/// What an automation control of envelope `kind` of `owner` shows: the value it is writing, else the envelope at the
/// cursor (or the value of no envelope). While writing, the envelope just after the cursor is still the old one.
pub fn value(ctx: &Context, project: &Project, owner: &str, kind: EnvKind) -> f32 {
    if let Some(v) = with(ctx, |s| s.passes.iter().find(|p| p.owner == owner && p.kind == kind).map(|p| p.v)) {
        return v;
    }
    let cur = project.timeline.cursor_position;
    edit::envelope_of(project, owner, kind).map_or(kind.neutral(), |(e, start, scale)| e.at((cur - start) / scale))
}

/// The key time of the cursor on envelope `kind` of `owner`, with the envelope.
fn at_cursor(project: &Project, owner: &str, kind: EnvKind) -> Option<(Envelope, f64, f64, f64)> {
    let (e, start, scale) = edit::envelope_of(project, owner, kind)?;
    Some((e, start, scale, ((project.timeline.cursor_position - start) / scale).max(0.0)))
}

/// An automation control of envelope `kind` of `owner` (its track's or bus's mode `mode`) moved to `v` (envelope
/// units), or is held (`None`: it keeps its value) this frame.
pub fn touch(ctx: &Context, project: &mut Project, history: &mut History, owner: &str, kind: EnvKind, mode: AutoMode, v: Option<f32>) {
    if !mode.writes() {
        if v.is_some() {
            crate::ui::hint(ctx, "Automation is set to Read: choose Write (Touch) or Write (Latch) to change the envelope.");
        }
        return;
    }
    let mut st = with(ctx, std::mem::take);
    let k = match st.passes.iter().position(|p| p.owner == owner && p.kind == kind) {
        Some(k) => k,
        None => {
            let Some(v) = v else { return with(ctx, |s| *s = st) };
            let before = project.clone();
            if !edit::ensure_envelope(project, owner, kind) {
                return with(ctx, |s| *s = st);
            }
            history.push(before);
            st.passes.push(Pass { owner: owner.to_string(), kind, latch: mode == AutoMode::Latch, fed: true, v, rec: None });
            st.passes.len() - 1
        }
    };
    let pass = &mut st.passes[k];
    pass.fed = true;
    if let Some(v) = v {
        pass.v = v;
    }
    if st.playing {
        step(project, pass);
    } else if v.is_some() {
        pass.rec = None;
        set_at_cursor(project, owner, kind, pass.v);
    }
    with(ctx, |s| *s = st);
}

/// Stopped: the envelope takes `v` at the cursor (a point there is moved, else one is added).
fn set_at_cursor(project: &mut Project, owner: &str, kind: EnvKind, v: f32) {
    let Some((_, _, scale, k)) = at_cursor(project, owner, kind) else { return };
    let tol = 0.5 / project.settings.frame_rate.max(1.0) / scale;
    if edit::with_envelope(project, owner, kind, |e| {
        match e.points.keys.iter().position(|p| (p.t - k).abs() < tol) {
            Some(i) => e.points.keys[i].v = edit::env_clamp(kind, v),
            None => {
                edit::env_insert(e, k, v);
            }
        }
        true
    }) {
        project.mark_modified();
    }
}

/// Playing: write the pass's value at the cursor. A cursor behind the last write (a loop wrap, a seek back) or far past
/// it (a seek on) punches in again there.
fn step(project: &mut Project, pass: &mut Pass) {
    let Some((e, _, _, k)) = at_cursor(project, &pass.owner, pass.kind) else { return };
    if pass.rec.as_ref().is_none_or(|r| k < r.last - 1e-6 || k > r.last + 1.0) {
        pass.rec = Some(Rec { orig: e, t0: k, pts: Vec::new(), last: k });
    }
    let Some(r) = pass.rec.as_mut() else { return };
    edit::write_point(pass.kind, &mut r.pts, k, pass.v);
    r.last = k;
    let new = edit::env_written(&r.orig, r.t0, &r.pts, k, pass.v);
    if edit::with_envelope(project, &pass.owner, pass.kind, |e| {
        *e = new;
        true
    }) {
        project.mark_modified();
    }
}

/// After the UI: a Latch pass writes on until playback stops; a pass whose control was let go ends (a Latch one only
/// when stopped).
pub fn end_frame(ctx: &Context, project: &mut Project) {
    let mut st = with(ctx, std::mem::take);
    let playing = st.playing;
    st.passes.retain_mut(|p| {
        let keep = p.fed || (p.latch && playing && p.rec.is_some());
        if keep && !p.fed && playing {
            step(project, p);
        }
        p.fed = false;
        keep
    });
    with(ctx, |s| *s = st);
}

/// An automation slider over envelope `kind` of `owner`: `resp` on a control at `pos` (0..1) whose value `to_v` maps
/// (reset to `reset`); gestures as `mixer::slider`, and a held press keeps a Touch pass going.
#[allow(clippy::too_many_arguments)]
pub fn slider(
    ui: &Ui,
    resp: &Response,
    project: &mut Project,
    history: &mut History,
    owner: &str,
    kind: EnvKind,
    mode: AutoMode,
    pos: f32,
    reset: f32,
    at: impl Fn(Pos2) -> f32,
    delta: impl Fn(Vec2) -> f32,
    to_v: impl Fn(f32) -> f32,
) {
    match super::mixer::slider(ui, resp, pos, reset, at, delta) {
        Some((_, np)) => touch(ui.ctx(), project, history, owner, kind, mode, Some(to_v(np.clamp(0.0, 1.0)))),
        None if resp.is_pointer_button_down_on() => touch(ui.ctx(), project, history, owner, kind, mode, None),
        None => {}
    }
}

/// Volume envelope dB as a fader gain, and back.
pub fn gain_of(db: f32) -> f32 {
    crate::engine::keyframes::db_to_gain(db)
}

pub fn db_of(gain: f32) -> f32 {
    if gain <= 1e-6 { -120.0 } else { (20.0 * gain.log10()).max(-120.0) }
}

/// Automation ▸ (audio track header, bus track): Show Automation Controls and the four modes, checked by the first of
/// `ids`; a pick is one undo step on all of `ids`.
pub fn menu(ui: &mut Ui, project: &mut Project, history: &mut History, ids: &[String]) {
    use super::menus::{Mark, entry};
    let Some((mode, shown)) = ids.first().and_then(|id| edit::automation_of(project, id)) else { return };
    let mut pick = None;
    super::menus::sub(ui, "Au&tomation", true, |ui| {
        if entry(ui, "Show Automation Controls", "", true, Mark::Check(shown)) {
            pick = Some((None, Some(!shown)));
        }
        super::menus::sep(ui);
        for m in AutoMode::ALL {
            if entry(ui, m.name(), "", true, Mark::Radio(m == mode)) {
                pick = Some((Some(m), None));
            }
        }
    });
    if let Some((m, c)) = pick {
        let before = project.clone();
        if edit::set_automation(project, ids, m, c) {
            history.push(before);
        }
    }
}

/// FX Automation Envelopes... on audio track or bus `id`: the chooser opens.
pub fn open_chooser(ctx: &Context, id: &str) {
    with(ctx, |s| s.chooser = Some(id.to_string()));
}

/// The FX Automation chooser: each plug-in of the track's (or bus's) chain and its parameters an envelope can
/// automate, checked while it has one. A click adds or removes that envelope (one undo step).
pub fn chooser(ctx: &Context, project: &mut Project, history: &mut History) {
    let Some(owner) = with(ctx, |s| s.chooser.clone()) else { return };
    let chain: Vec<crate::project::Fx> = match project.find_track(&owner) {
        Some((false, i)) => project.timeline.audio_tracks[i].fx.clone(),
        Some(_) => Vec::new(),
        None => project.timeline.audio.bus(&owner).map_or(Vec::new(), |b| b.fx.clone()),
    };
    let name = match project.find_track(&owner) {
        Some((_, i)) => format!("Track {}", i + 1),
        None => project.timeline.audio.bus(&owner).map_or(String::new(), |b| b.name.clone()),
    };
    if edit::automation_of(project, &owner).is_none() {
        return with(ctx, |s| s.chooser = None);
    }
    let mut toggle = None;
    let (_, close) = super::dialogs::tool_window(ctx, "fx_automation", &format!("FX Automation: {name}"), vec2(340.0, 380.0), |ui| {
        super::fx_window::field_style(ui);
        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if chain.is_empty() {
                ui.label("No plug-ins. Add some with the Track FX button.");
            }
            for f in &chain {
                let Some(info) = crate::engine::plugins::find(&f.plugin) else { continue };
                ui.label(RichText::new(info.name).strong());
                let mut section = "";
                for p in info.params.iter().filter(|p| crate::engine::plugins::automatable(f, p.key).is_some()) {
                    section = if p.section.is_empty() { section } else { p.section };
                    let mut on = f.auto.contains_key(p.key);
                    let label = if section.is_empty() { p.name.to_string() } else { format!("{section}: {}", p.name) };
                    if ui.checkbox(&mut on, label).changed() {
                        toggle = Some((f.id.clone(), p.key.to_string()));
                    }
                }
                ui.add_space(6.0);
            }
        });
    });
    if let Some((fx, key)) = toggle {
        let before = project.clone();
        if edit::toggle_fx_envelope(project, &owner, &fx, &key) {
            history.push(before);
        }
    }
    if close {
        with(ctx, |s| s.chooser = None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> (Project, String) {
        let mut p = Project::new("t");
        let tr = &mut p.timeline.audio_tracks[0];
        (tr.automation, tr.auto_controls) = (AutoMode::Touch, true);
        let id = tr.id.clone();
        (p, id)
    }

    /// A frame of the app: the cursor at `t`, the control at `v` (None: let go).
    fn frame(ctx: &Context, p: &mut Project, h: &mut History, id: &str, t: f64, v: Option<f32>) {
        begin_frame(ctx, true);
        p.timeline.cursor_position = t;
        if let Some(v) = v {
            touch(ctx, p, h, id, EnvKind::Volume, control_mode(p, id).unwrap(), Some(v));
        }
        end_frame(ctx, p);
    }

    /// R8c acceptance: a touch write of a linear fader move gives at most 60 points per second, none outside the hold,
    /// and the envelope as it was outside it; the whole pass is one undo step.
    #[test]
    fn a_touch_write_is_thinned_and_stays_inside_the_hold() {
        let (ctx, (mut p, id), mut h) = (Context::default(), project(), History::default());
        edit::ensure_envelope(&mut p, &id, EnvKind::Volume);
        edit::with_envelope(&mut p, &id, EnvKind::Volume, |e| {
            edit::env_insert(e, 10.0, -6.0);
            true
        });
        // 1 ms frames: idle 1..2 s, a linear move -20 dB -> 0 dB held 2..4 s, idle 4..5 s.
        for ms in 1000..5000 {
            let t = ms as f64 / 1000.0;
            let v = (2.0..4.0).contains(&t).then(|| -20.0 + 10.0 * (t - 2.0) as f32);
            frame(&ctx, &mut p, &mut h, &id, t, v);
            if ms == 3500 {
                // The control shows the value it writes (just after the cursor the envelope is still the old one).
                assert_eq!(value(&ctx, &p, &id, EnvKind::Volume), -5.0);
            }
        }
        assert!(value(&ctx, &p, &id, EnvKind::Volume) > -3.5, "let go: the envelope at the cursor again");
        let (e, _, _) = edit::envelope_of(&p, &id, EnvKind::Volume).unwrap();
        let inside: Vec<f64> = e.points.keys.iter().map(|k| k.t).filter(|t| *t > 1e-9 && *t < 10.0).collect();
        assert!(inside.iter().all(|t| (2.0..4.0).contains(t)), "{inside:?}");
        assert!(inside.len() as f64 <= 60.0 * 2.0 + 4.0, "{} points", inside.len());
        assert!(inside.len() >= 20, "the move is drawn: {} points", inside.len());
        assert!((e.at(3.0) - -10.0).abs() < 0.6, "{}", e.at(3.0));
        // Before and after the hold the original line, 0 dB at 0 to -6 dB at 10 s.
        assert!((e.at(1.5) - -0.9).abs() < 1e-3 && (e.at(6.0) - -3.6).abs() < 1e-3, "{} {}", e.at(1.5), e.at(6.0));
        assert_eq!(h.lens().0, 1, "one undo step");
        // Read writes nothing.
        p.timeline.audio_tracks[0].automation = AutoMode::Read;
        frame(&ctx, &mut p, &mut h, &id, 4.5, Some(5.0));
        assert_eq!(edit::envelope_of(&p, &id, EnvKind::Volume).unwrap().0, e);
    }

    /// Latch writes on after the control is let go, until playback stops; stopped, a move sets the cursor's point.
    #[test]
    fn latch_writes_until_stop_and_a_stopped_move_sets_the_cursor_point() {
        let (ctx, (mut p, id), mut h) = (Context::default(), project(), History::default());
        p.timeline.audio_tracks[0].automation = AutoMode::Latch;
        frame(&ctx, &mut p, &mut h, &id, 1.0, Some(-12.0));
        for ms in 1001..3000 {
            frame(&ctx, &mut p, &mut h, &id, ms as f64 / 1000.0, None);
        }
        let e = edit::envelope_of(&p, &id, EnvKind::Volume).unwrap().0;
        assert!((e.at(2.9) - -12.0).abs() < 1e-4 && e.at(0.5).abs() < 1e-4);
        // Stopped: the pass ends; a move at 5 s adds a point there.
        begin_frame(&ctx, false);
        end_frame(&ctx, &mut p);
        p.timeline.cursor_position = 5.0;
        begin_frame(&ctx, false);
        touch(&ctx, &mut p, &mut h, &id, EnvKind::Volume, AutoMode::Latch, Some(-3.0));
        end_frame(&ctx, &mut p);
        let e = edit::envelope_of(&p, &id, EnvKind::Volume).unwrap().0;
        assert!(e.points.keys.iter().any(|k| (k.t - 5.0).abs() < 1e-9 && k.v == -3.0));
        assert_eq!(h.lens().0, 2);
    }
}
