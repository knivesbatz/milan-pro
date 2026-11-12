//! Help › Check for Updates and the startup check: a non-modal window showing the updater's state.
use eframe::egui::*;
use std::path::PathBuf;
use crate::updater::{CURRENT, State, Updater};
use super::super::theme;

pub enum UpdateResult {
    /// "Skip this version": remember it so the automatic check stays quiet about it.
    Skip(String),
    /// "Install and restart" with the verified installer and its SHA-256.
    Install(PathBuf, String),
}

/// Draw the window for `up`'s state (nothing when idle). `auto_check` is the "Check for updates automatically"
/// preference, editable here too.
pub fn update_window(ctx: &Context, up: &mut Updater, auto_check: &mut bool) -> Option<UpdateResult> {
    let size = if matches!(up.state, State::Available(_)) { vec2(480.0, 440.0) } else { vec2(420.0, 150.0) };
    let (hit, close) = match &up.state {
        State::Idle => return None,
        state => super::window(ctx, "milan_update", "Milan Pro Update", size, |ui| body(ui, state, auto_check)),
    };
    let out = match (&up.state, hit) {
        (State::Available(_), Some(0)) => {
            up.download(ctx);
            return None;
        }
        (State::Available(r), Some(2)) => Some(UpdateResult::Skip(r.version.clone())),
        (State::Ready(r, exe), Some(0)) => Some(UpdateResult::Install(exe.clone(), r.digest.clone().unwrap_or_default())),
        _ if hit.is_none() && !close => return None,
        _ => None,
    };
    // Any other button (Remind me later, Cancel, Later, OK) or × closes the window.
    up.dismiss();
    out
}

fn body(ui: &mut Ui, state: &State, auto_check: &mut bool) -> Option<usize> {
    match state {
        State::Idle => None,
        State::Checking => {
            ui.label("Checking for updates...");
            ui.add_space(46.0);
            super::button_row(ui, &[("Cancel", true)])
        }
        State::Available(r) => {
            ui.label(RichText::new(format!("Milan Pro {} is available.", r.version)).strong());
            ui.label(format!("You have version {CURRENT}."));
            ui.add_space(4.0);
            ui.label("Release notes:");
            let d = theme::get().dialog();
            Frame::NONE.fill(d.field).stroke(Stroke::new(1.0_f32, d.field_border)).inner_margin(4).show(ui, |ui| {
                ScrollArea::vertical().max_height(250.0).auto_shrink(false).show(ui, |ui| {
                    ui.label(if r.notes.trim().is_empty() { "(No release notes.)" } else { r.notes.as_str() });
                });
            });
            ui.checkbox(auto_check, "Check for updates automatically");
            ui.add_space(6.0);
            super::button_row(ui, &[("Download and install", true), ("Remind me later", true), ("Skip this version", true)])
        }
        State::Downloading(r, done) => {
            ui.label(format!("Downloading {}...", r.name));
            let mb = |b: u64| b as f64 / 1_048_576.0;
            let frac = if r.size > 0 { (*done as f32 / r.size as f32).min(1.0) } else { 0.0 };
            ui.add(ProgressBar::new(frac).text(format!("{:.1} of {:.1} MB", mb(*done), mb(r.size))));
            ui.add_space(18.0);
            super::button_row(ui, &[("Cancel", true)])
        }
        State::Ready(r, _) => {
            ui.label(format!("Milan Pro {} was downloaded and verified.", r.version));
            ui.label("Milan Pro will close, install the update and start again.");
            ui.add_space(26.0);
            super::button_row(ui, &[("Install and restart", true), ("Later", true)])
        }
        State::Message(m) => {
            ui.label(m.as_str());
            ui.add_space(46.0);
            super::button_row(ui, &[("OK", true)])
        }
    }
}
