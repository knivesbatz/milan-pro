//! R8b: the Mixing Console window (View › Window › Mixing Console) and the level meters every window shares.
//!
//! Meters: once a frame the app folds what each `mix::Meter` (tracks and busses) and the `audio::Master` tap measured
//! into one `MeterView` per id (`feed_levels`), kept in egui memory. The track headers, the Master Bus window and the
//! console only read them, and the mix thread only writes atomics: metering takes no lock and costs playback nothing.
use std::collections::{HashMap, HashSet};
use eframe::egui::*;
use serde::{Deserialize, Serialize};
use crate::audio::Master;
use crate::edit::{self, FxTarget};
use crate::engine::mix::Meters;
use crate::history::History;
use crate::project::*;
use super::Selection;
use super::icons::{Icon, paint_icon};
use super::keymap::Command;
use super::menus::{self, Later, Mark, entry, send};
use super::theme::{self, Theme, UiLook};
use super::timeline::{fader_gain, fader_pos};
use super::toolbar::{inert_button, tool_button};

/// Meter range: 0 dB at the top of a well, -60 dB at the bottom.
pub const RANGE_DB: f32 = 60.0;
/// Fall-back speed of the bars and of a released peak hold.
const FALL_DB_PER_S: f32 = 24.0;
/// How long a peak hold stays before it falls.
pub const HOLD_S: f64 = 2.0;
/// VU ballistics: a step reaches 99 % in 300 ms (first order: 1 - e^(-0.3 / tau) = 0.99, tau = 0.3 / ln 100).
const VU_TAU: f32 = 0.3 / 4.605_17;
/// 0 VU is -20 dBFS RMS.
const VU_REF_DB: f32 = -20.0;
/// The VU scale: -20 VU at the bottom, +3 VU at the top.
const VU_RANGE: (f32, f32) = (-20.0, 3.0);
/// The Master bus's id among the levels (track and bus ids are UUIDs).
pub const MASTER: &str = "master";

pub fn db(v: f32) -> f32 {
    if v <= 1e-6 { -f32::INFINITY } else { 20.0 * v.log10() }
}

/// What one meter shows, per channel: the bar (dB, falling back), the 2 s peak hold, the highest peak since the last
/// reset (the readout), the latched clip lamp and the VU level (linear RMS through the VU ballistics).
#[derive(Clone, Copy, Debug)]
pub struct MeterView {
    pub level: [f32; 2],
    pub hold: [(f32, f64); 2],
    pub max: [f32; 2],
    pub clip: [bool; 2],
    pub vu: [f32; 2],
}

impl Default for MeterView {
    fn default() -> Self {
        Self { level: [-f32::INFINITY; 2], hold: [(-f32::INFINITY, 0.0); 2], max: [-f32::INFINITY; 2], clip: [false; 2], vu: [0.0; 2] }
    }
}

impl MeterView {
    /// Fold in the peaks played since the last frame (`dt` seconds ago) and the RMS of the last 50 ms.
    pub fn feed(&mut self, peaks: [f32; 2], rms: [f32; 2], now: f64, dt: f32) {
        let k = 1.0 - (-dt / VU_TAU).exp();
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
            self.vu[c] += (rms[c] - self.vu[c]) * k;
        }
    }

    /// Click on the readouts or a well: the readouts and clip lamps start again.
    pub fn reset(&mut self) {
        self.max = [-f32::INFINITY; 2];
        self.clip = [false; 2];
    }

    pub fn active(&self) -> bool {
        self.level.iter().chain(self.hold.iter().map(|h| &h.0)).any(|&l| l > -RANGE_DB) || self.vu.iter().any(|&v| v > 1e-4)
    }

    /// The VU reading of channel `c` (0 VU = -20 dBFS).
    pub fn vu_db(&self, c: usize) -> f32 {
        db(self.vu[c]) - VU_REF_DB
    }

    /// The peak readout of channel `c`: "-4.3", or "-Inf." before any sound.
    pub fn readout(&self, c: usize) -> String {
        peak_text(self.max[c])
    }

    /// One readout for both channels (a track header's): the higher peak.
    pub fn peak_text(&self) -> String {
        peak_text(self.max[0].max(self.max[1]))
    }
}

fn peak_text(d: f32) -> String {
    if d > -RANGE_DB { format!("{d:.1}") } else { "-Inf.".into() }
}

#[derive(Clone, Default)]
struct Levels(HashMap<String, MeterView>);

fn levels_id() -> Id {
    Id::new("milan_levels")
}

/// Each frame, before the windows draw: fold in what every meter measured since the last frame. `live`: a playback
/// session runs (stopped, the last RMS is stale and the meters fall). Meters that left the plan fall to silence too.
// shortcut: track and bus meters are fed as the mixer makes a block, up to the device queue (about 128 ms) before it is
// heard; the master meter is exact. Delay them by the queue if users notice.
pub fn feed_levels(ctx: &Context, meters: &Meters, master: &Master, live: bool) {
    let (now, dt) = ctx.input(|i| (i.time, i.stable_dt.min(0.1)));
    let rms = |r: [f32; 2]| if live { r } else { [0.0; 2] };
    let active = ctx.data_mut(|d| {
        let lv = &mut d.get_temp_mut_or_default::<Levels>(levels_id()).0;
        for id in meters.keys() {
            if !lv.contains_key(id) {
                lv.insert(id.clone(), MeterView::default());
            }
        }
        lv.entry(MASTER.to_string()).or_default();
        for (id, v) in lv.iter_mut() {
            let (p, r) = match (id.as_str(), meters.get(id)) {
                (MASTER, _) => (master.take_peaks(), master.rms()),
                (_, Some(m)) => (m.take_peaks(), m.rms()),
                _ => ([0.0; 2], [0.0; 2]),
            };
            v.feed(p, rms(r), now, dt);
        }
        lv.retain(|id, v| id == MASTER || meters.contains_key(id) || v.active());
        lv.values().any(MeterView::active)
    });
    if active {
        ctx.request_repaint();
    }
}

/// The meter of track or bus `id` (or `MASTER`); silent when it has none.
pub fn level(ctx: &Context, id: &str) -> MeterView {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Levels>(levels_id()).0.get(id).copied()).unwrap_or_default()
}

/// Screenshot harness: meter `id` shows `db` (L, R) peaks, held, and its VU that RMS.
pub fn show_level(ctx: &Context, id: &str, db: [f32; 2]) {
    ctx.data_mut(|d| {
        let v = d.get_temp_mut_or_default::<Levels>(levels_id()).0.entry(id.to_string()).or_default();
        let lin = db.map(|x| 10f32.powf(x / 20.0));
        (v.level, v.max, v.vu, v.clip) = (db, db, lin, lin.map(|x| x >= 1.0));
        v.hold = db.map(|x| (x, f64::INFINITY)); // held for good
    });
}

pub fn reset_level(ctx: &Context, id: &str) {
    ctx.data_mut(|d| {
        if let Some(v) = d.get_temp_mut_or_default::<Levels>(levels_id()).0.get_mut(id) {
            v.reset();
        }
    });
}

/// The two channel bars of `lv` in `well` (split in two with a 1 px gap): the level, the peak hold line and the clip
/// lamps. The caller paints the empty well.
pub fn paint_levels(p: &Painter, well: Rect, lv: &MeterView, v16: bool) {
    let t = theme::get();
    let to_y = |d: f32| well.min.y + well.height() * (-d / RANGE_DB).clamp(0.0, 1.0);
    let half = ((well.width() - 1.0) * 0.5).floor().max(1.0);
    for c in 0..2 {
        let x0 = well.min.x + c as f32 * (well.width() - half);
        let bar = Rect::from_x_y_ranges(Rangef::new(x0, x0 + half), well.y_range());
        if lv.level[c] > -RANGE_DB {
            super::master_bus::paint_bar(p, bar, to_y(lv.level[c].min(0.0)), &to_y, v16, 1.0);
        }
        let h = lv.hold[c].0;
        if h > -RANGE_DB {
            let y = to_y(h.min(0.0)).round();
            p.rect_filled(Rect::from_x_y_ranges(bar.x_range(), Rangef::new(y, y + 1.0)), 0.0, if h >= -6.0 { t.meter_yellow } else { t.meter_green });
        }
        if lv.clip[c] {
            p.rect_filled(Rect::from_x_y_ranges(bar.x_range(), Rangef::new(well.min.y, well.min.y + 4.0)), 0.0, t.meter_red);
        }
    }
}

/// An empty meter well (the VP23 unlit gradient, or `meter_bg`).
pub fn paint_well(p: &Painter, well: Rect) {
    let t = theme::get();
    if t.meter_unlit > 0.0 {
        let to_y = |d: f32| well.min.y + well.height() * (-d / RANGE_DB).clamp(0.0, 1.0);
        super::master_bus::paint_bar(p, well, well.min.y, &to_y, false, t.meter_unlit);
    } else {
        p.rect_filled(well, 0.0, t.meter_bg);
    }
}

/// The VU pair: green up to 0 VU, red above, on a -20..+3 VU scale.
fn paint_vu(p: &Painter, well: Rect, lv: &MeterView) {
    let t = theme::get();
    p.rect_filled(well, 0.0, t.meter_bg);
    let to_y = |v: f32| well.max.y - well.height() * ((v - VU_RANGE.0) / (VU_RANGE.1 - VU_RANGE.0)).clamp(0.0, 1.0);
    let half = ((well.width() - 1.0) * 0.5).floor().max(1.0);
    for c in 0..2 {
        let v = lv.vu_db(c);
        if v <= VU_RANGE.0 {
            continue;
        }
        let x = Rangef::new(well.min.x + c as f32 * (well.width() - half), well.min.x + c as f32 * (well.width() - half) + half);
        let (top, zero) = (to_y(v), to_y(0.0));
        p.rect_filled(Rect::from_x_y_ranges(x, Rangef::new(top.max(zero), well.max.y)), 0.0, t.meter_green);
        if top < zero {
            p.rect_filled(Rect::from_x_y_ranges(x, Rangef::new(top, zero)), 0.0, t.meter_red);
        }
    }
}

// ---- Console layout (saved in the preferences) ----

/// The control regions, in the order the strips stack them (top down).
const REGION_NAMES: [&str; 6] = ["Insert FX", "Sends", "I/O", "VU Meters", "Peak Meters", "Faders"];
const FX_R: usize = 0;
const SENDS_R: usize = 1;
const IO_R: usize = 2;
const VU_R: usize = 3;
const METERS_R: usize = 4;
const FADERS_R: usize = 5;

/// Mixing Console view settings: Views › Channel List; Properties and Layout › Show Channels (audio tracks, audio
/// busses, assignable FX, master bus), Show Control Regions (`REGION_NAMES` order; VU Meters off, as the peak meters
/// are VEGAS's default) and Channel Width (0 narrow, 1 default, 2 wide).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConsolePrefs {
    pub channel_list: bool,
    pub channels: [bool; 4],
    pub regions: [bool; 6],
    pub width: u8,
}

impl Default for ConsolePrefs {
    fn default() -> Self {
        Self { channel_list: true, channels: [true; 4], regions: [true, true, true, false, true, true], width: 1 }
    }
}

impl ConsolePrefs {
    fn strip_w(&self) -> f32 {
        [60.0, 84.0, 116.0][self.width.min(2) as usize]
    }
}

/// A strip: an audio track or a bus / assignable FX chain (by id), or the master bus.
#[derive(Clone, PartialEq, Debug)]
enum Strip {
    Track(String),
    Bus(String),
    Master,
}

impl Strip {
    fn key(&self) -> &str {
        match self {
            Strip::Track(id) | Strip::Bus(id) => id,
            Strip::Master => MASTER,
        }
    }
}

/// Console state that is not saved: strips unticked in the Channel List and a rename under way.
// shortcut: unticked strips last for the session only; save them with the project if users ask for it.
#[derive(Default)]
pub struct ConsoleView {
    hidden: HashSet<String>,
    rename: Option<(String, String, bool)>,
}

/// The strips the project has, in console order (audio tracks, busses, assignable FX chains, master), with their label
/// and colour.
fn strips(project: &Project, t: &Theme, kinds: [bool; 4]) -> Vec<(Strip, String, Color32)> {
    let tl = &project.timeline;
    let mut v = Vec::new();
    if kinds[0] {
        let n0 = tl.video_tracks.len();
        for (i, tr) in tl.audio_tracks.iter().enumerate() {
            let name = if tr.name.is_empty() { format!("{}", n0 + i + 1) } else { format!("{}: {}", n0 + i + 1, tr.name) };
            v.push((Strip::Track(tr.id.clone()), name, t.track_color(n0 + i)));
        }
    }
    for (k, list) in [(1, &tl.audio.buses), (2, &tl.audio.fx)] {
        if kinds[k] {
            v.extend(list.iter().map(|b| (Strip::Bus(b.id.clone()), b.name.clone(), t.accent)));
        }
    }
    if kinds[3] {
        v.push((Strip::Master, "Master".into(), t.text_dim));
    }
    v
}

/// A popup menu below an icon button with egui menu semantics (submenus work); ▾ beside the icon.
fn icon_menu(ui: &mut Ui, icon: Icon, tip: &str, add: impl FnOnce(&mut Ui)) {
    let t = theme::get();
    let r = ui.scope(|ui| menu::menu_custom_button(ui, Button::new("").frame(false).min_size(vec2(t.m.button_pitch + 10.0, t.m.button)), add));
    let rect = r.inner.response.rect;
    paint_icon(ui.painter(), Rect::from_center_size(rect.center() - vec2(5.0, 0.0), Vec2::splat(t.m.icon)), icon, t.look(), None);
    paint_icon(ui.painter(), Rect::from_center_size(pos2(rect.max.x - 7.0, rect.center().y), Vec2::splat(t.m.icon)), Icon::DropDown, t.look(), None);
    r.inner.response.on_hover_text(tip);
}

/// The Mixing Console toolbar: Views, Properties and Layout, Downmix Output, Dim Output, Insert Audio Track, Insert
/// Assignable FX, Insert Bus (and Insert Input Bus, R9). True when the layout changed (save the preferences).
pub fn mixer_toolbar(ui: &mut Ui, prefs: &mut ConsolePrefs, master: &Master) -> bool {
    let before = prefs.clone();
    icon_menu(ui, Icon::Views, "Views", |ui| {
        if entry(ui, "&Channel List", "", true, Mark::Check(prefs.channel_list)) {
            prefs.channel_list = !prefs.channel_list;
        }
    });
    icon_menu(ui, Icon::Properties, "Properties and Layout", |ui| {
        if entry(ui, "&Audio Properties...", "", true, Mark::None) {
            send(ui.ctx(), Command::AudioProperties);
        }
        menus::sep(ui);
        menus::sub(ui, "Show &Channels", true, |ui| {
            if entry(ui, "Show &All Channels", "", true, Mark::None) {
                prefs.channels = [true; 4];
            }
            menus::sep(ui);
            for (k, label) in ["Audio &Tracks", "Audio &Busses", "Assignable &FX", "&Master Bus"].into_iter().enumerate() {
                if entry(ui, label, "", true, Mark::Check(prefs.channels[k])) {
                    prefs.channels[k] = !prefs.channels[k];
                }
            }
        });
        menus::sub(ui, "Show Control &Regions", true, |ui| {
            if entry(ui, "Show &All Control Regions", "", true, Mark::None) {
                prefs.regions = [true; 6];
            }
            menus::sep(ui);
            // VEGAS's order: Faders, Peak Meters, VU Meters, I/O, Sends, Insert FX.
            for k in [FADERS_R, METERS_R, VU_R, IO_R, SENDS_R, FX_R] {
                if entry(ui, &format!("{} Control Region", REGION_NAMES[k]), "", true, Mark::Check(prefs.regions[k])) {
                    prefs.regions[k] = !prefs.regions[k];
                }
            }
        });
        menus::sub(ui, "Channel &Width", true, |ui| {
            for (k, label) in ["&Narrow", "&Default", "&Wide"].into_iter().enumerate() {
                if entry(ui, label, "", true, Mark::Radio(prefs.width as usize == k)) {
                    prefs.width = k as u8;
                }
            }
        });
    });
    let (mono, dim) = (master.mono.load(std::sync::atomic::Ordering::Relaxed), master.dim.load(std::sync::atomic::Ordering::Relaxed));
    if tool_button(ui, Icon::Downmix, if mono { "Downmix Output: Mono" } else { "Downmix Output: Stereo" }, true, mono).clicked() {
        master.mono.store(!mono, std::sync::atomic::Ordering::Relaxed);
    }
    if tool_button(ui, Icon::Dim, "Dim Output (-20 dB)", true, dim).clicked() {
        send(ui.ctx(), Command::DimMaster);
    }
    super::toolbar::separator(ui);
    for (icon, tip, c) in [(Icon::TrackAudio, "Insert Audio Track", Command::InsertAudioTrack), (Icon::TrackFx, "Insert Assignable FX", Command::InsertAssignableFx), (Icon::InsertBus, "Insert Bus", Command::InsertAudioBus)] {
        if tool_button(ui, icon, tip, true, false).clicked() {
            send(ui.ctx(), c);
        }
    }
    inert_button(ui, Icon::InsertBus, &Later("R9").tip("Insert Input Bus"), false);
    *prefs != before
}

/// The y ranges of the control regions (None when hidden) and of the label, for strips `h` tall.
fn regions(prefs: &ConsolePrefs, sends: usize, h: f32) -> ([Option<Rangef>; 6], Rangef) {
    let on = prefs.regions;
    let fixed = [64.0, if sends > 0 { 4.0 + 26.0 * sends as f32 } else { 0.0 }, 26.0, 100.0];
    let mut out = [None; 6];
    let mut y = 0.0;
    for k in [FX_R, SENDS_R, IO_R, VU_R] {
        if on[k] && fixed[k] > 0.0 {
            out[k] = Some(Rangef::new(y, y + fixed[k]));
            y += fixed[k];
        }
    }
    let label = Rangef::new(h - 22.0, h);
    let rest = (label.min - y).max(0.0);
    let meters = match (on[METERS_R], on[FADERS_R]) {
        (true, true) => (rest * 0.4).max(70.0).min(rest),
        (true, false) => rest,
        _ => 0.0,
    };
    if meters > 0.0 {
        out[METERS_R] = Some(Rangef::new(y, y + meters));
        y += meters;
    }
    if on[FADERS_R] {
        out[FADERS_R] = Some(Rangef::new(y, label.min));
    }
    (out, label)
}

/// Slider gesture on a control at `pos` (0..1): a double-click resets to `reset`, a press away from the thumb jumps
/// there (`at` maps the pointer), a drag moves by `delta` (Ctrl: fine), the wheel nudges. Some((push an undo step, the new
/// position)).
pub(super) fn slider(ui: &Ui, resp: &Response, pos: f32, reset: f32, at: impl Fn(Pos2) -> f32, delta: impl Fn(Vec2) -> f32) -> Option<(bool, f32)> {
    if resp.double_clicked() {
        return Some((true, reset));
    }
    if resp.drag_started() || resp.clicked() {
        let p = resp.interact_pointer_pos().map(at).filter(|p| (p - pos).abs() > 0.05).unwrap_or(pos);
        return Some((true, p));
    }
    if resp.dragged() {
        let fine = if ui.input(|i| i.modifiers.command) { 0.1 } else { 1.0 };
        return Some((false, pos + delta(resp.drag_delta()) * fine));
    }
    if resp.hovered() {
        let d = ui.input(|i| i.raw_scroll_delta.y);
        if d != 0.0 {
            ui.ctx().input_mut(|i| {
                i.raw_scroll_delta = Vec2::ZERO;
                i.smooth_scroll_delta = Vec2::ZERO;
            });
            return Some((true, pos + d.signum() * 0.01));
        }
    }
    None
}

pub(super) fn gain_text(g: f32) -> String {
    if g <= 1e-5 { "-Inf.".into() } else { format!("{:.1}", 20.0 * g.log10() + 0.0) }
}

pub(super) fn pan_text(pan: f32) -> String {
    let p = (pan * 100.0).round();
    if p == 0.0 { "Center".into() } else if p < 0.0 { format!("{:.0}% L", -p) } else { format!("{p:.0}% R") }
}

/// A strip toggle (Mute, Solo, Arm...): lit like the track header's. True when clicked.
pub(super) fn toggle(ui: &Ui, p: &Painter, r: Rect, id: Id, icon: Option<Icon>, text: &str, on: bool, tip: &str) -> bool {
    let t = theme::get();
    let resp = ui.interact(r, id, Sense::click()).on_hover_text(tip);
    if on {
        p.rect_filled(r, CornerRadius::same(2), if t.look() == UiLook::Vegas16 { t.button_active_bg } else { t.accent.gamma_multiply(0.3) });
    } else if resp.hovered() {
        p.rect_filled(r, CornerRadius::same(2), t.button_hover_bg);
    }
    // An unarmed Arm button is grey (its icon is red).
    let tint = if on && t.look() != UiLook::Vegas16 { Some(t.accent) } else if !on && icon == Some(Icon::Record) { Some(t.text_dim) } else { None };
    match icon {
        Some(i) => paint_icon(p, Rect::from_center_size(r.center(), vec2(16.0, 16.0)), i, t.look(), tint),
        None => {
            p.text(r.center(), Align2::CENTER_CENTER, text, t.font(t.font.ui), tint.unwrap_or(t.text));
        }
    }
    resp.clicked()
}

/// What a strip's controls edit, read once per frame.
struct Ctl {
    volume: f32,
    pan: f32,
    muted: bool,
    fx: Vec<Fx>,
    out: Option<Out>,
}

fn ctl(project: &Project, s: &Strip) -> Option<Ctl> {
    let tl = &project.timeline;
    Some(match s {
        Strip::Track(id) => {
            let tr = tl.audio_tracks.iter().find(|t| t.id == *id)?;
            Ctl { volume: tr.volume, pan: tr.pan, muted: tr.muted, fx: tr.fx.clone(), out: Some(tr.out.clone()) }
        }
        Strip::Bus(id) => {
            let b = tl.audio.bus(id)?;
            Ctl { volume: b.volume, pan: b.pan, muted: b.muted, fx: b.fx.clone(), out: Some(b.out.clone()) }
        }
        Strip::Master => Ctl { volume: tl.master.gain[0], pan: 0.0, muted: false, fx: tl.master.audio_fx.clone(), out: None },
    })
}

fn track_mut<'a>(project: &'a mut Project, id: &str) -> Option<&'a mut Track> {
    project.timeline.audio_tracks.iter_mut().find(|t| t.id == id)
}

fn bus_mut<'a>(project: &'a mut Project, id: &str) -> Option<&'a mut Bus> {
    let a = &mut project.timeline.audio;
    a.buses.iter_mut().chain(&mut a.fx).find(|b| b.id == id)
}

/// Set strip `s`'s volume (the master's both channels, linked) or pan.
fn set_level(project: &mut Project, s: &Strip, pan: bool, v: f32) {
    match s {
        Strip::Track(id) => {
            if let Some(t) = track_mut(project, id) {
                if pan { t.pan = v } else { t.volume = v }
            }
        }
        Strip::Bus(id) => {
            if let Some(b) = bus_mut(project, id) {
                if pan { b.pan = v } else { b.volume = v }
            }
        }
        Strip::Master if !pan => {
            let g = &mut project.timeline.master.gain;
            let d = v - g[0];
            g.iter_mut().for_each(|x| *x = (*x + d).clamp(0.0, fader_gain(1.0)));
        }
        Strip::Master => {}
    }
    project.mark_modified();
}

/// The Mixing Console body: the Channel List (strip check boxes) and the channel strips, each with (top down) Insert
/// FX, Sends, I/O, VU Meters, Peak Meters, Faders and its label. Every control edit is one undo step.
pub fn mixing_console(ui: &mut Ui, view: &mut ConsoleView, prefs: &ConsolePrefs, project: &mut Project, history: &mut History, sel: &mut Selection) {
    let t = theme::get();
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0.0, t.panel_bg);
    let all = strips(project, &t, prefs.channels);
    let mut area = rect;
    if prefs.channel_list && rect.width() > 260.0 {
        let list = rect.with_max_x(rect.min.x + 130.0);
        area = rect.with_min_x(list.max.x + 1.0);
        ui.painter().rect_filled(Rect::from_x_y_ranges(Rangef::new(list.max.x, list.max.x + 1.0), rect.y_range()), 0.0, t.frame);
        let mut child = ui.new_child(UiBuilder::new().max_rect(list.shrink(4.0)));
        ScrollArea::vertical().id_salt("console_list").auto_shrink(false).show(&mut child, |ui| {
            for (s, name, _) in &all {
                let mut on = !view.hidden.contains(s.key());
                if ui.checkbox(&mut on, RichText::new(name).color(t.text)).changed() {
                    if on { view.hidden.remove(s.key()) } else { view.hidden.insert(s.key().to_string()) };
                }
            }
        });
    }
    let shown: Vec<&(Strip, String, Color32)> = all.iter().filter(|s| !view.hidden.contains(s.0.key())).collect();
    let a = &project.timeline.audio;
    let targets: Vec<(String, String)> = a.buses.iter().chain(&a.fx).map(|b| (b.id.clone(), b.name.clone())).collect();
    let w = prefs.strip_w();
    // Tall enough for every region (the Sends grow with the busses); the area scrolls.
    let h = (area.height() - 2.0).max(330.0 + 26.0 * targets.len() as f32 + if prefs.regions[VU_R] { 100.0 } else { 0.0 });
    let mut child = ui.new_child(UiBuilder::new().max_rect(area));
    ScrollArea::both().id_salt("console_strips").auto_shrink(false).show(&mut child, |ui| {
        let (r, _) = ui.allocate_exact_size(vec2(w * shown.len() as f32, h), Sense::hover());
        let (regs, label) = regions(prefs, targets.len(), h);
        for (k, (s, name, color)) in shown.iter().enumerate() {
            let x = Rangef::new(r.min.x + w * k as f32, r.min.x + w * (k + 1) as f32 - 1.0);
            if x.max < ui.clip_rect().min.x || x.min > ui.clip_rect().max.x {
                continue;
            }
            let ry = |y: Rangef| Rect::from_x_y_ranges(x, Rangef::new(r.min.y + y.min, r.min.y + y.max));
            strip(ui, view, project, history, sel, s, name, *color, &targets, regs.map(|o| o.map(ry)), ry(label), !prefs.regions[METERS_R]);
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn strip(ui: &mut Ui, view: &mut ConsoleView, project: &mut Project, history: &mut History, sel: &mut Selection, s: &Strip, name: &str, color: Color32, targets: &[(String, String)], regs: [Option<Rect>; 6], label: Rect, fader_meter: bool) {
    let t = theme::get();
    let v16 = t.look() == UiLook::Vegas16;
    let Some(c) = ctl(project, s) else { return };
    let id = ui.id().with(("strip", s.key()));
    let p = ui.painter().clone();
    let all = regs.iter().flatten().fold(label, |a, r| a.union(*r));
    let selected = matches!(s, Strip::Track(tid) if sel.tracks.contains(tid));
    p.rect_filled(all, 0.0, if selected { t.track_header_sel_bg } else { t.track_header_bg });
    p.rect_filled(Rect::from_x_y_ranges(Rangef::new(all.max.x, all.max.x + 1.0), all.y_range()), 0.0, t.track_separator);
    for r in regs.iter().flatten() {
        p.hline(r.x_range(), r.max.y - 0.5, Stroke::new(1.0_f32, t.track_separator));
    }
    let small = t.font(t.font.meter + 1.0);
    let lv = level(ui.ctx(), s.key());
    let target = match s {
        Strip::Track(id) => FxTarget::Track(id.clone()),
        Strip::Bus(id) => FxTarget::Bus(id.clone()),
        Strip::Master => FxTarget::AudioMaster,
    };

    // Insert FX: the chain's plug-ins; a click opens the chain in the FX window.
    if let Some(r) = regs[FX_R] {
        let resp = ui.interact(r, id.with("fx"), Sense::click()).on_hover_text("Insert FX: click to edit the chain");
        if resp.hovered() {
            p.rect_filled(r.shrink(1.0), 0.0, t.button_hover_bg);
        }
        if resp.clicked() {
            super::open_tool(ui.ctx(), super::ToolWin::Fx(target.clone()));
        }
        let pc = p.with_clip_rect(r.shrink(2.0));
        paint_icon(&pc, Rect::from_min_size(r.min + vec2(3.0, 3.0), vec2(14.0, 14.0)), Icon::TrackFx, t.look(), None);
        for (k, f) in c.fx.iter().take(3).enumerate() {
            let n = crate::engine::plugins::find(&f.plugin).map_or(f.plugin.clone(), |i| i.name.to_string());
            pc.text(pos2(r.min.x + 4.0, r.min.y + 20.0 + 14.0 * k as f32), Align2::LEFT_TOP, n, small.clone(), if f.enabled { t.text } else { t.text_dim });
        }
        if c.fx.len() > 3 {
            pc.text(pos2(r.max.x - 4.0, r.min.y + 4.0), Align2::RIGHT_TOP, format!("+{}", c.fx.len() - 3), small.clone(), t.text_dim);
        }
    }

    // Sends (audio tracks): one fader per bus and assignable FX chain; right-click for Pre / Post Volume.
    if let (Some(r), Strip::Track(tid)) = (regs[SENDS_R], s) {
        for (k, (to, tname)) in targets.iter().enumerate() {
            let row = Rect::from_min_size(pos2(r.min.x, r.min.y + 2.0 + 26.0 * k as f32), vec2(r.width(), 26.0));
            let Some(tr) = project.timeline.audio_tracks.iter().find(|x| x.id == *tid) else { return };
            let snd = tr.send(to).cloned();
            let pre = snd.as_ref().map_or(edit::send_pre_default(&project.timeline, to), |s| s.pre);
            let lvl = snd.map_or(0.0, |s| s.level);
            let pc = p.with_clip_rect(row);
            pc.text(pos2(row.min.x + 4.0, row.min.y + 1.0), Align2::LEFT_TOP, tname, small.clone(), t.text_label);
            pc.text(pos2(row.max.x - 4.0, row.min.y + 1.0), Align2::RIGHT_TOP, gain_text(lvl), small.clone(), t.text_value);
            let groove = Rangef::new(row.min.x + 8.0, row.max.x - 8.0);
            let cy = row.min.y + 19.0;
            p.rect_filled(Rect::from_x_y_ranges(groove, Rangef::new(cy - 1.0, cy + 1.0)), 0.0, t.fader_track);
            let pos = fader_pos(lvl);
            let tx = groove.min + pos * groove.span();
            p.rect_filled(Rect::from_center_size(pos2(tx.round(), cy), vec2(6.0, 10.0)), 1.0, t.fader_thumb);
            let tip = format!("{tname}: {} dB ({})", gain_text(lvl), if pre { "Pre Volume" } else { "Post Volume" });
            let resp = ui.interact(row.shrink2(vec2(2.0, 0.0)), id.with(("send", k)), Sense::click_and_drag()).on_hover_text(tip);
            let span = groove.span().max(1.0);
            if let Some((undo, np)) = slider(ui, &resp, pos, fader_pos(1.0), |q| (q.x - groove.min) / span, |d| d.x / span) {
                if undo {
                    history.push(project.clone());
                }
                if let Some(tr) = track_mut(project, tid) {
                    edit::set_send(tr, to, fader_gain(np.clamp(0.0, 1.0)), pre);
                }
                project.mark_modified();
            }
            let mut pick = None;
            menus::context(&resp, |ui| {
                for (label, on) in [("Pre Volume", true), ("Post Volume", false)] {
                    if entry(ui, label, "", true, Mark::Radio(pre == on)) {
                        pick = Some(on);
                    }
                }
            });
            if let Some(on) = pick {
                let before = project.clone();
                if track_mut(project, tid).is_some_and(|tr| edit::set_send_pre(tr, to, on)) {
                    history.push(before);
                    project.mark_modified();
                }
            }
        }
    }

    // I/O: the output (Master or a bus); the master goes to the output device.
    if let Some(r) = regs[IO_R] {
        let b = Rect::from_min_max(pos2(r.min.x + 3.0, r.min.y + 4.0), pos2(r.max.x - 3.0, r.max.y - 4.0));
        p.rect_filled(b, 2.0, t.input_bg);
        let a = &project.timeline.audio;
        let text = match &c.out {
            None => "Default".to_string(),
            Some(Out::Master) => "Master".to_string(),
            Some(Out::Bus(o)) => a.bus(o).map_or("Master".into(), |x| x.name.clone()),
        };
        p.with_clip_rect(b).text(b.left_center() + vec2(4.0, 0.0), Align2::LEFT_CENTER, text, small.clone(), if c.out.is_some() { t.text } else { t.text_dim });
        if let Some(out) = &c.out {
            let resp = ui.interact(b, id.with("out"), Sense::click()).on_hover_text("Output");
            let pid = id.with("out_popup");
            if resp.clicked() {
                ui.memory_mut(|m| m.toggle_popup(pid));
            }
            // Busses a track or bus can go to (never an assignable FX chain, nor the bus itself).
            let mut choices = vec![(Out::Master, "Master".to_string())];
            choices.extend(a.buses.iter().filter(|x| x.id != s.key()).map(|x| (Out::Bus(x.id.clone()), x.name.clone())));
            let mut picked = None;
            super::chrome::with_menu_style(&ui.ctx().clone(), &t, || {
                #[allow(deprecated)]
                popup::popup_below_widget(ui, pid, &resp, PopupCloseBehavior::CloseOnClick, |ui| {
                    ui.set_min_width(120.0);
                    for (o, n) in &choices {
                        if entry(ui, &n.replace('&', "&&"), "", true, Mark::Radio(o == out)) {
                            picked = Some(o.clone());
                        }
                    }
                });
            });
            if let Some(o) = picked {
                let before = project.clone();
                let done = match s {
                    Strip::Track(tid) => Ok(edit::set_track_out(project, std::slice::from_ref(tid), &o)),
                    Strip::Bus(bid) => edit::set_bus_out(project, bid, &o),
                    Strip::Master => Ok(false),
                };
                match done {
                    Ok(true) => history.push(before),
                    Ok(false) => {}
                    Err(e) => super::hint(ui.ctx(), e),
                }
            }
        } else {
            ui.interact(b, id.with("out"), Sense::hover()).on_hover_text("The default output device (a device choice comes with batch R9)");
        }
    }

    // VU meters (VU ballistics, 0 VU = -20 dBFS).
    if let Some(r) = regs[VU_R] {
        let well = Rect::from_min_max(pos2(r.center().x - 9.0, r.min.y + 6.0), pos2(r.center().x + 9.0, r.max.y - 6.0));
        paint_vu(&p, well, &lv);
        if r.width() > 70.0 {
            let to_y = |v: f32| well.max.y - well.height() * ((v - VU_RANGE.0) / (VU_RANGE.1 - VU_RANGE.0));
            for v in [3.0, 0.0, -3.0, -7.0, -10.0, -20.0] {
                p.text(pos2(well.min.x - 3.0, to_y(v)), Align2::RIGHT_CENTER, format!("{v:+.0}").replace("+0", "0"), t.font(t.font.meter), t.meter_label);
            }
        }
        ui.interact(well, id.with("vu"), Sense::hover()).on_hover_text("VU (0 VU = -20 dBFS)");
    }

    // Peak meters: readouts (a click resets them and the clip lamps) over the two wells, with a dB scale.
    if let Some(r) = regs[METERS_R] {
        let font = t.font(t.font.meter);
        let read = Rect::from_min_max(r.min + vec2(2.0, 2.0), pos2(r.max.x - 2.0, r.min.y + 14.0));
        for ch in 0..2 {
            let at = if ch == 0 { read.left_center() + vec2(2.0, 0.0) } else { read.right_center() - vec2(2.0, 0.0) };
            p.text(at, if ch == 0 { Align2::LEFT_CENTER } else { Align2::RIGHT_CENTER }, lv.readout(ch), font.clone(), if lv.clip[ch] { t.meter_red } else { t.text });
        }
        let well = Rect::from_min_max(pos2(r.center().x - 9.0, read.max.y + 3.0), pos2(r.center().x + 9.0, r.max.y - 4.0));
        if well.height() > 10.0 {
            paint_well(&p, well);
            paint_levels(&p, well, &lv, v16);
            if r.width() > 70.0 {
                for d in (6..60).step_by(6) {
                    let y = well.min.y + well.height() * d as f32 / RANGE_DB;
                    p.text(pos2(well.min.x - 3.0, y), Align2::RIGHT_CENTER, d.to_string(), font.clone(), t.meter_label);
                }
            }
        }
        if ui.interact(read.union(well), id.with("peaks"), Sense::click()).on_hover_text("Click to reset the peaks").clicked() {
            reset_level(ui.ctx(), s.key());
        }
    }

    // Faders: Arm / Mute / Solo / Phase, then Pan, then the fader and its gain.
    if let Some(r) = regs[FADERS_R] {
        let mut x = r.min.x + 3.0;
        let mut btn = |n: f32| {
            let b = Rect::from_min_size(pos2(x, r.min.y + 3.0), vec2(n, 18.0));
            x += n + 2.0;
            b
        };
        match s {
            Strip::Track(tid) => {
                let tr = project.timeline.audio_tracks.iter().find(|x| x.id == *tid).map(|t| (t.armed, t.muted, t.solo, t.invert));
                if let Some((armed, muted, solo, invert)) = tr {
                    if toggle(ui, &p, btn(18.0), id.with("arm"), Some(Icon::Record), "", armed, "Arm for Record") {
                        (sel.tracks, sel.list_focus, sel.env) = (vec![tid.clone()], true, None);
                        send(ui.ctx(), Command::Arm);
                    }
                    for (k, (icon, text, on, tip)) in [(Some(Icon::Mute), "", muted, "Mute"), (Some(Icon::Solo), "", solo, "Solo"), (None, "Ø", invert, "Invert Track Phase")].into_iter().enumerate() {
                        if toggle(ui, &p, btn(18.0), id.with(("tbtn", k)), icon, text, on, tip) {
                            history.push(project.clone());
                            if let Some(tr) = track_mut(project, tid) {
                                match k {
                                    0 => tr.muted = !tr.muted,
                                    1 => tr.solo = !tr.solo,
                                    _ => tr.invert = !tr.invert,
                                }
                            }
                            project.mark_modified();
                        }
                    }
                }
            }
            Strip::Bus(bid) => {
                if toggle(ui, &p, btn(18.0), id.with("mute"), Some(Icon::Mute), "", c.muted, "Mute") {
                    history.push(project.clone());
                    if let Some(b) = bus_mut(project, bid) {
                        b.muted = !b.muted;
                    }
                    project.mark_modified();
                }
            }
            // The master's Mute silences playback (Options › Mute All Audio), as on the Master Bus window.
            Strip::Master => {
                if toggle(ui, &p, btn(18.0), id.with("mute"), Some(Icon::Mute), "", menus::state(ui.ctx()).mute_audio, "Mute (playback)") {
                    send(ui.ctx(), Command::MuteAllAudio);
                }
            }
        }
        // R8c: with Show Automation Controls, a track's or bus's Pan and fader show (and write) its envelopes.
        let sid = match s {
            Strip::Track(id) | Strip::Bus(id) => Some(id.clone()),
            Strip::Master => None,
        };
        let auto = sid.as_deref().and_then(|id| Some((id, super::automation::control_mode(project, id)?)));
        let c = match auto {
            Some((id, _)) => {
                let v = |kind| super::automation::value(ui.ctx(), project, id, kind);
                Ctl { pan: v(EnvKind::Pan), volume: super::automation::gain_of(v(EnvKind::Volume)), ..c }
            }
            None => c,
        };
        let tint = |kind| auto.map(|_| if kind == EnvKind::Volume { t.envelope_volume } else { t.envelope_pan });
        // Pan (not on the master).
        let mut top = r.min.y + 24.0;
        if *s != Strip::Master {
            let groove = Rangef::new(r.min.x + 8.0, r.max.x - 8.0);
            let cy = top + 8.0;
            p.rect_filled(Rect::from_x_y_ranges(groove, Rangef::new(cy - 1.0, cy + 1.0)), 0.0, t.fader_track);
            p.rect_filled(Rect::from_center_size(pos2(groove.center().round(), cy), vec2(1.0, 7.0)), 0.0, t.fader_track);
            let pos = (c.pan.clamp(-1.0, 1.0) + 1.0) * 0.5;
            let tx = (groove.min + pos * groove.span()).round();
            p.add(Shape::convex_polygon(vec![pos2(tx - 5.0, cy - 5.0), pos2(tx + 5.0, cy - 5.0), pos2(tx, cy + 4.0)], tint(EnvKind::Pan).unwrap_or(t.fader_thumb), Stroke::new(1.0_f32, t.fader_thumb_bar)));
            let hit = Rect::from_x_y_ranges(Rangef::new(groove.min - 6.0, groove.max + 6.0), Rangef::new(top, top + 16.0));
            let resp = ui.interact(hit, id.with("pan"), Sense::click_and_drag()).on_hover_text(format!("Pan: {}", pan_text(c.pan)));
            let span = groove.span().max(1.0);
            if let Some((aid, mode)) = auto {
                super::automation::slider(ui, &resp, project, history, aid, EnvKind::Pan, mode, pos, 0.5, |q| (q.x - groove.min) / span, |d| d.x / span, |np| np * 2.0 - 1.0);
            } else if let Some((undo, np)) = slider(ui, &resp, pos, 0.5, |q| (q.x - groove.min) / span, |d| d.x / span) {
                if undo {
                    history.push(project.clone());
                }
                set_level(project, s, true, np.clamp(0.0, 1.0) * 2.0 - 1.0);
            }
            top += 20.0;
        }
        // The fader travels the rest, over the gain readout; a thin meter beside it when the meter region is hidden.
        let (y0, y1) = (top + 6.0, r.max.y - 18.0);
        if y1 - y0 > 20.0 {
            let fx = r.center().x.round() - if fader_meter { 8.0 } else { 0.0 };
            p.rect_filled(Rect::from_x_y_ranges(Rangef::new(fx, fx + 1.0), Rangef::new(y0, y1)), 0.0, t.fader_track);
            let pos = fader_pos(c.volume);
            let ty = (y1 - (y1 - y0) * pos).round();
            let thumb = Rect::from_center_size(pos2(fx + 0.5, ty), if v16 { vec2(21.0, 12.0) } else { vec2(25.0, 13.0) });
            p.rect_filled(thumb, 1.0, tint(EnvKind::Volume).unwrap_or(t.fader_thumb));
            p.rect_stroke(thumb, 1.0, Stroke::new(1.0_f32, t.fader_thumb_bar), StrokeKind::Inside);
            p.rect_filled(Rect::from_center_size(thumb.center(), vec2(thumb.width() - 8.0, 1.0)), 0.0, t.fader_thumb_bar);
            if fader_meter {
                let well = Rect::from_min_max(pos2(fx + 16.0, y0), pos2(fx + 26.0, y1));
                paint_well(&p, well);
                paint_levels(&p, well, &lv, v16);
            }
            let hit = Rect::from_x_y_ranges(Rangef::new(fx - 14.0, fx + 15.0), Rangef::new(y0 - 7.0, y1 + 7.0));
            let what = if *s == Strip::Master { "Master" } else { "Volume" };
            let resp = ui.interact(hit, id.with("fader"), Sense::click_and_drag()).on_hover_text(format!("{what}: {} dB", gain_text(c.volume)));
            let span = (y1 - y0).max(1.0);
            if let Some((aid, mode)) = auto {
                let to_v = |np: f32| super::automation::db_of(fader_gain(np));
                super::automation::slider(ui, &resp, project, history, aid, EnvKind::Volume, mode, pos, fader_pos(1.0), |q| (y1 - q.y) / span, |d| -d.y / span, to_v);
            } else if let Some((undo, np)) = slider(ui, &resp, pos, fader_pos(1.0), |q| (y1 - q.y) / span, |d| -d.y / span) {
                if undo {
                    history.push(project.clone());
                }
                set_level(project, s, false, fader_gain(np.clamp(0.0, 1.0)));
            }
            p.text(pos2(fx + 0.5, r.max.y - 9.0), Align2::CENTER_CENTER, gain_text(c.volume), small.clone(), t.text_value);
        }
    }

    // Label: colour bar and name; a click selects the track, a double-click renames.
    p.rect_filled(Rect::from_x_y_ranges(label.x_range(), Rangef::new(label.min.y, label.min.y + 3.0)), 0.0, color);
    let lr = ui.interact(label, id.with("label"), Sense::click());
    if lr.clicked() {
        if let Strip::Track(tid) = s {
            (sel.tracks, sel.list_focus, sel.env) = (vec![tid.clone()], true, None);
        }
    }
    let editing = view.rename.as_ref().is_some_and(|r| r.0 == s.key());
    if lr.double_clicked() && *s != Strip::Master && !editing {
        let old = match s {
            Strip::Track(tid) => project.timeline.audio_tracks.iter().find(|x| x.id == *tid).map(|x| x.name.clone()),
            _ => project.timeline.audio.bus(s.key()).map(|b| b.name.clone()),
        };
        view.rename = old.map(|o| (s.key().to_string(), o, false));
    }
    if editing {
        rename(ui, view, project, history, s, label);
    } else {
        p.with_clip_rect(label).text(label.center() + vec2(0.0, 1.5), Align2::CENTER_CENTER, name, t.font(t.font.ui), t.text_bright);
    }
}

/// The label's rename box: Enter or a click away keeps the name (one undo step), Escape drops it.
fn rename(ui: &mut Ui, view: &mut ConsoleView, project: &mut Project, history: &mut History, s: &Strip, at: Rect) {
    let t = theme::get();
    let Some((key, mut buf, focused)) = view.rename.take() else { return };
    let resp = ui.put(at.shrink2(vec2(2.0, 2.0)), TextEdit::singleline(&mut buf).font(t.font(t.font.ui)).margin(vec2(2.0, 1.0)));
    if !focused {
        resp.request_focus();
    }
    if !resp.lost_focus() {
        view.rename = Some((key, buf, true));
        return;
    }
    if ui.input(|i| i.key_pressed(Key::Escape)) {
        return;
    }
    let before = project.clone();
    let name = match s {
        Strip::Track(id) => track_mut(project, id).map(|t| &mut t.name),
        Strip::Bus(id) => bus_mut(project, id).map(|b| &mut b.name),
        Strip::Master => None,
    };
    if let Some(n) = name.filter(|n| **n != buf) {
        *n = buf;
        history.push(before);
        project.mark_modified();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meter_ballistics_hold_and_clip() {
        let mut m = MeterView::default();
        m.feed([0.5, 1.0], [0.0; 2], 0.0, 0.016);
        assert!((m.level[0] + 6.02).abs() < 0.01 && m.level[1] == 0.0);
        assert!(m.clip[1] && !m.clip[0]);
        // Silence: the bar falls at FALL_DB_PER_S, the hold stays for HOLD_S.
        m.feed([0.0, 0.0], [0.0; 2], 1.0, 0.5);
        assert!((m.level[0] - (-6.02 - 12.0)).abs() < 0.01);
        assert!((m.hold[0].0 + 6.02).abs() < 0.01);
        m.feed([0.0, 0.0], [0.0; 2], 3.0, 0.5);
        assert!(m.hold[0].0 < -6.03);
        assert!((m.max[0] + 6.02).abs() < 0.01);
        m.reset();
        assert!(!m.clip[1] && m.max[1] == -f32::INFINITY);
    }

    /// R8b acceptance: a peak hold stays put for 2 s, then falls.
    #[test]
    fn peak_hold_lasts_two_seconds() {
        let mut m = MeterView::default();
        let dt = 1.0 / 60.0;
        m.feed([0.5, 0.5], [0.0; 2], 0.0, dt);
        let held = m.hold[0].0;
        let mut now = 0.0;
        while now < HOLD_S - 0.02 {
            now += dt as f64;
            m.feed([0.01, 0.01], [0.0; 2], now, dt);
            assert_eq!(m.hold[0].0, held, "fell at {now:.3} s");
        }
        for _ in 0..4 {
            now += dt as f64;
            m.feed([0.01, 0.01], [0.0; 2], now, dt);
        }
        assert!(m.hold[0].0 < held, "still held at {now:.3} s");
    }

    /// R8b acceptance: VU ballistics. A 1 kHz sine step at 0 VU (-20 dBFS RMS), through the 50 ms meter RMS window and the
    /// UI's frames, reaches 99 % of its level in 300 ms ±10 %; 0 VU reads 0.
    #[test]
    fn vu_step_reaches_99_percent_in_300_ms() {
        let rate = crate::audio::RATE as f64;
        let amp = 10f32.powf(-20.0 / 20.0) * std::f32::consts::SQRT_2;
        let win = crate::engine::mix::METER_WINDOW;
        let (mut m, mut sq, mut t) = (MeterView::default(), std::collections::VecDeque::new(), 0.0f64);
        let dt = 1.0 / 60.0;
        let mut reached = None;
        let mut n = 0usize;
        while t < 0.6 {
            // One UI frame of samples into the sliding 50 ms window, then the frame's feed.
            let frames = (dt as f64 * rate) as usize;
            for _ in 0..frames {
                let v = amp * (2.0 * std::f32::consts::PI * 1000.0 * (n as f64 / rate) as f32).sin();
                sq.push_back(v * v);
                if sq.len() > win {
                    sq.pop_front();
                }
                n += 1;
            }
            let rms = (sq.iter().sum::<f32>() / win as f32).sqrt();
            t += dt as f64;
            m.feed([0.0; 2], [rms; 2], t, dt);
            if reached.is_none() && m.vu[0] >= 0.99 * 0.1 {
                reached = Some(t);
            }
        }
        let at = reached.expect("never reached 99 %");
        assert!((at - 0.3).abs() <= 0.03, "99 % at {at:.3} s");
        assert!(m.vu_db(0).abs() < 0.1, "{}", m.vu_db(0));
    }

    #[test]
    fn regions_stack_and_hide() {
        let p = ConsolePrefs::default();
        let (r, label) = regions(&p, 2, 500.0);
        assert!(r[VU_R].is_none(), "VU meters are off by default");
        let order: Vec<Rangef> = [FX_R, SENDS_R, IO_R, METERS_R, FADERS_R].iter().map(|&k| r[k].unwrap()).collect();
        assert!(order.windows(2).all(|w| w[0].max == w[1].min));
        assert_eq!((r[SENDS_R].unwrap().span(), r[FADERS_R].unwrap().max, label.max), (56.0, 478.0, 500.0));
        let q = ConsolePrefs { regions: [false, true, false, true, false, true], ..p };
        let (r, _) = regions(&q, 0, 500.0);
        assert!(r[SENDS_R].is_none() && r[VU_R].unwrap().min == 0.0 && r[FADERS_R].unwrap().min == 100.0);
    }
}
