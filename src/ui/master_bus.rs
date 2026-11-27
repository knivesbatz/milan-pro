//! Master Bus window (§5.12): toolbar, strip header, peak readouts, two live meters with a 3 dB scale,
//! and the master fader (one linked thumb) that sets the playback gain. R10c: the strip's automation badge (Automation
//! Settings: Off, Read, Write (Touch), Write (Latch), and Show Automation Controls); with automation controls on, the
//! fader shows and writes the master's Volume envelope.
use eframe::egui::*;
use crate::audio::Master;
use crate::edit;
use crate::history::History;
use crate::project::{AutoMode, EnvKind, Project};
use super::icons::{Icon, paint_icon};
use super::keymap::Command;
use super::menus::send;
use super::mixer::{self, MASTER, RANGE_DB};
use super::theme;
use super::timeline::{fader_gain, fader_pos};
use super::toolbar::tool_button;

/// The master faders: L/R gains (linear); linked moves both. The meters are `mixer::level(MASTER)`.
pub struct MasterView {
    pub gain: [f32; 2],
    pub linked: bool,
}

impl Default for MasterView {
    fn default() -> Self {
        Self { gain: [1.0; 2], linked: true }
    }
}

/// Master Bus toolbar: Master Bus Properties (Project Properties › Audio), Downmix Output (stereo / mono), Dim Output
/// (-20 dB) and the Mixing Console.
pub fn master_bus_toolbar(ui: &mut Ui, master: &Master, console: bool) {
    use std::sync::atomic::Ordering::Relaxed;
    if tool_button(ui, Icon::Properties, "Master Bus Properties", true, false).clicked() {
        send(ui.ctx(), Command::AudioProperties);
    }
    let (mono, dim) = (master.mono.load(Relaxed), master.dim.load(Relaxed));
    if tool_button(ui, Icon::Downmix, if mono { "Downmix Output: Mono" } else { "Downmix Output: Stereo" }, true, mono).clicked() {
        master.mono.store(!mono, Relaxed);
    }
    if tool_button(ui, Icon::Dim, "Dim Output (-20 dB)", true, dim).clicked() {
        send(ui.ctx(), Command::DimMaster);
    }
    if tool_button(ui, Icon::MixingConsole, "Mixing Console", true, console).clicked() {
        send(ui.ctx(), Command::Window(super::dock::Panel::MixingConsole));
    }
}

/// `fx`: the Audio Master FX chain has plug-ins (its fx button is lit; a click opens it). R10c: `project` and `history`
/// for the automation badge and an automation fader; True when the fader is an automation control (its moves are
/// envelope writes, never the master gain).
pub fn master_bus(ui: &mut Ui, view: &mut MasterView, fx: bool, project: &mut Project, history: &mut History) -> bool {
    let t = theme::get();
    let lv = mixer::level(ui.ctx(), MASTER);

    let r = ui.max_rect();
    let p = ui.painter().clone();
    let ink = t.text;
    let head = Rect::from_min_size(r.min + vec2(8.0, 6.0), vec2(10.0, 10.0));
    p.rect_stroke(head, 0.0, Stroke::new(1.0_f32, ink), StrokeKind::Inside);
    p.text(pos2(head.max.x + 6.0, head.center().y), Align2::LEFT_CENTER, "Master", t.font(t.font.ui), ink);
    // Strip row: fx · automation · M · S.
    automation_badge(ui, &p, Rect::from_min_size(pos2(r.min.x + 26.0, r.min.y + 22.0), vec2(16.0, 16.0)), project, history);
    for (i, icon) in [Icon::TrackFx, Icon::Mute, Icon::Solo].into_iter().enumerate() {
        let c = Rect::from_min_size(pos2(r.min.x + 6.0 + 20.0 * (i + usize::from(i > 0)) as f32, r.min.y + 22.0), vec2(16.0, 16.0));
        match icon {
            Icon::TrackFx => {
                paint_icon(&p, c, Icon::TrackFx, None);
                if fx {
                    p.circle_filled(c.right_bottom() - vec2(2.0, 2.0), 2.0, t.accent);
                }
                if ui.interact(c, ui.id().with("master_fx"), Sense::click()).on_hover_text("Audio Master FX").clicked() {
                    super::open_tool(ui.ctx(), super::ToolWin::Fx(crate::edit::FxTarget::AudioMaster));
                }
            }
            // Mute silences playback (Options › Mute All Audio); the render keeps the sound, as a monitoring control.
            Icon::Mute => {
                let on = super::menus::state(ui.ctx()).mute_audio;
                if super::mixer::toggle(ui, &p, c, ui.id().with("master_mute"), Some(Icon::Mute), "", on, "Mute (playback)") {
                    send(ui.ctx(), Command::MuteAllAudio);
                }
            }
            icon => {
                paint_icon(&p, c, icon, None);
                ui.interact(c, ui.id().with("master_solo"), Sense::hover()).on_hover_text(super::menus::NotPlanned("R57").tip("Solo: the master solos against the preview bus"));
            }
        }
    }

    let cx = r.center().x.round();
    let wells = [cx - 26.0, cx + 16.0];
    let font = t.font(t.font.meter);
    // Peak readouts; a click resets them and the clip lamps.
    let ready = r.min.y + 44.0;
    for c in 0..2 {
        p.text(pos2(wells[c] + 5.0, ready), Align2::CENTER_CENTER, lv.readout(c), font.clone(), t.text);
    }
    let read_hit = Rect::from_x_y_ranges(Rangef::new(wells[0] - 14.0, wells[1] + 24.0), Rangef::new(ready - 7.0, ready + 7.0));
    if ui.interact(read_hit, ui.id().with("master_peaks"), Sense::click()).on_hover_text("Click to reset the peaks").clicked() {
        mixer::reset_level(ui.ctx(), MASTER);
    }

    let top = r.min.y + 54.0;
    let bottom = r.max.y - 26.0;
    let auto = super::automation::control_mode(project, edit::MASTER);
    if bottom - top < 40.0 {
        return auto.is_some();
    }
    let to_y = |d: f32| top + (bottom - top) * (-d / RANGE_DB).clamp(0.0, 1.0);
    for (c, &x) in wells.iter().enumerate() {
        let well = Rect::from_min_max(pos2(x, top), pos2(x + 10.0, bottom));
        super::widgets::meter_well(&p, well, &t);
        let lvl = lv.level[c];
        if lvl > -RANGE_DB {
            paint_bar(&p, well, to_y(lvl.min(0.0)), &to_y);
        }
        let (h, _) = lv.hold[c];
        if h > -RANGE_DB {
            let y = to_y(h.min(0.0)).round();
            p.rect_filled(Rect::from_min_max(pos2(x, y), pos2(x + 10.0, y + 1.0)), 0.0, if h >= -6.0 { t.meter_yellow } else { t.meter_green });
        }
        if lv.clip[c] {
            p.rect_filled(Rect::from_min_size(well.min, vec2(10.0, 4.0)), 0.0, t.meter_red);
        }
        if ui.interact(well, ui.id().with(("master_well", c)), Sense::click()).clicked() {
            mixer::reset_level(ui.ctx(), MASTER);
        }
    }
    let step = (bottom - top) / 20.0;
    // A short window labels every 6 or 12 dB so no two labels overlap (the ticks stay at 3 dB).
    let every = [1, 2, 4].into_iter().find(|n| step * *n as f32 >= font.size + 1.0).unwrap_or(4);
    for k in 1..=19 {
        let y = (top + step * k as f32).round();
        let label = (3 * k).to_string();
        if k % every == 0 {
            p.text(pos2(cx, y), Align2::CENTER_CENTER, label, font.clone(), t.meter_label);
        }
        for x in [wells[0] + 11.0, wells[1] - 3.0] {
            p.rect_filled(Rect::from_min_size(pos2(x, y), vec2(2.0, 1.0)), 0.0, t.meter_label);
        }
    }

    // R10c: an automation fader shows the envelope at the cursor (or the value being written).
    let saved = view.gain;
    if auto.is_some() {
        let g = super::automation::gain_of(super::automation::value(ui.ctx(), project, edit::MASTER, EnvKind::Volume));
        view.gain = [g, g];
    }
    faders(ui, view, r, top, bottom, wells, auto, project, history);
    for c in 0..2 {
        let g = 20.0 * view.gain[c].max(1e-6).log10() + 0.0;
        let text = if view.gain[c] <= 1e-5 { "-Inf.".to_owned() } else { format!("{g:.1}") };
        p.text(pos2(wells[c] + 5.0, bottom + 6.0), Align2::CENTER_TOP, text, font.clone(), t.text);
    }
    if auto.is_some() {
        view.gain = saved;
    }
    auto.is_some()
}

/// R10c: the strip's automation badge: a circle with the mode's letter (R, T, L; empty when Off), in the Volume
/// envelope's colour while Show Automation Controls is on. A click drops the Automation Settings menu: Show
/// Automation Controls and the four modes, one undo step each.
fn automation_badge(ui: &mut Ui, p: &Painter, r: Rect, project: &mut Project, history: &mut History) {
    let t = theme::get();
    let (mode, shown) = edit::automation_of(project, edit::MASTER).unwrap_or_default();
    let c = r.center().floor() + vec2(0.5, 0.5);
    let ink = if shown && mode != AutoMode::Off { t.envelope_volume } else { t.text };
    p.circle_stroke(c, 6.5, Stroke::new(1.0_f32, ink));
    let letter = match mode {
        AutoMode::Off => "",
        AutoMode::Read => "R",
        AutoMode::Touch => "T",
        AutoMode::Latch => "L",
    };
    p.text(c, Align2::CENTER_CENTER, letter, t.bold(9.0), ink);
    let resp = ui.interact(r, ui.id().with("master_auto"), Sense::click()).on_hover_text(format!("Automation Settings: {}{}", mode.name(), if shown { ", controls shown" } else { "" }));
    let pid = ui.id().with("master_auto_menu");
    if resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(pid));
    }
    let mut pick = None;
    super::chrome::with_menu_style(&ui.ctx().clone(), &t, || {
        #[allow(deprecated)]
        popup::popup_below_widget(ui, pid, &resp, PopupCloseBehavior::CloseOnClick, |ui| {
            ui.set_min_width(200.0);
            use super::menus::{Mark, entry};
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
    });
    if let Some((m, shown)) = pick {
        let before = project.clone();
        if edit::set_automation(project, &[edit::MASTER.to_string()], m, shown) {
            history.push(before);
        }
    }
}

/// The level as lit LED segments (the 2006 meter) from `y` down.
pub(crate) fn paint_bar(p: &Painter, well: Rect, y: f32, to_y: &dyn Fn(f32) -> f32) {
    super::widgets::led_bar(p, well, y, to_y, &theme::get());
}

/// Faders travel the meter height: one stereo thumb left of the wells. Double-click resets to 0 dB. R10c: as an
/// automation control (`auto`), a move writes the master's Volume envelope (`automation::touch`) instead.
#[allow(clippy::too_many_arguments)]
fn faders(ui: &mut Ui, view: &mut MasterView, r: Rect, top: f32, bottom: f32, wells: [f32; 2], auto: Option<AutoMode>, project: &mut Project, history: &mut History) {
    let t = theme::get();
    let p = ui.painter().clone();
    let xs: Vec<f32> = vec![(r.min.x + wells[0]) * 0.5 - 2.0];
    let to_y = |g: f32| bottom - (bottom - top) * fader_pos(g);
    for (k, &x) in xs.iter().enumerate() {
        let ch = k.min(1);
        super::widgets::groove_v(&p, x.round(), Rangef::new(top, bottom), &t.metal);
        let y = to_y(view.gain[ch]);
        let thumb = Rect::from_center_size(pos2(x.round() + 0.5, y.round()), vec2(25.0, 13.0));
        let hit = Rect::from_x_y_ranges(thumb.expand(3.0).x_range(), Rangef::new(top - 6.0, bottom + 6.0));
        let resp = ui.interact(hit, ui.id().with(("master_fader", k)), Sense::click_and_drag());
        let gain_text = |g: f32| if g <= 1e-5 { "-Inf. dB".to_owned() } else { format!("{:.1} dB", 20.0 * g.log10() + 0.0) };
        let resp = resp.on_hover_text(gain_text(view.gain[ch]));
        let set = |g: f32, view: &mut MasterView| {
            if view.linked {
                let d = g - view.gain[ch];
                for c in 0..2 {
                    view.gain[c] = (view.gain[c] + d).clamp(0.0, fader_gain(1.0));
                }
            } else {
                view.gain[ch] = g;
            }
        };
        if let Some(mode) = auto {
            let moved = if resp.double_clicked() {
                Some(1.0)
            } else {
                resp.interact_pointer_pos().filter(|_| resp.dragged()).map(|pos| fader_gain((bottom - pos.y) / (bottom - top)))
            };
            if moved.is_some() || resp.is_pointer_button_down_on() {
                let v = moved.map(super::automation::db_of);
                super::automation::touch(ui.ctx(), project, history, edit::MASTER, EnvKind::Volume, mode, v);
                if let Some(g) = moved {
                    view.gain = [g, g];
                }
            }
        } else if resp.double_clicked() {
            set(1.0, view);
        } else if let Some(pos) = resp.interact_pointer_pos().filter(|_| resp.dragged()) {
            set(fader_gain((bottom - pos.y) / (bottom - top)), view);
        }
        // The Vegas 5–7 chrome fader handle: a raised block with a dark bar across its middle.
        super::widgets::fader_thumb(&p, thumb, &t.metal, None);
    }
}
