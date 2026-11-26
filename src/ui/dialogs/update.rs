//! Help › Check for Updates and the startup check: a non-modal window showing the updater's state. R46: the
//! verification note (SHA-256 alone until a signing key is compiled in), the roll-back note, the recording guard,
//! and the Roll Back dialog (Help › Roll Back, or the offer after a version failed to start twice).
use eframe::egui::*;
use crate::ui::widgets;
use std::path::PathBuf;
use crate::updater::{CURRENT, Previous, State, Updater, verification_note};

pub enum UpdateResult {
    /// "Skip this version": remember it so the automatic check stays quiet about it.
    Skip(String),
    /// "Install and restart" with the verified installer and its SHA-256.
    Install(PathBuf, String),
}

/// Draw the window for `up`'s state (nothing when idle). `auto_check` is the "Check for updates automatically"
/// preference, editable here too. `blocked`: why "Install and restart" may not run now (`updater::install_guard`);
/// the click then shows that instead.
pub fn update_window(ctx: &Context, up: &mut Updater, auto_check: &mut bool, blocked: Option<&str>) -> Option<UpdateResult> {
    let size = if matches!(up.state, State::Available(_)) { vec2(480.0, 460.0) } else { vec2(420.0, 170.0) };
    let note = up.note.clone();
    let (hit, close) = match &up.state {
        State::Idle => return None,
        state => super::window(ctx, "milan_update", "Milan Pro Update", size, |ui| body(ui, state, auto_check, note.as_deref())),
    };
    let out = match (&up.state, hit) {
        (State::Available(_), Some(0)) => {
            up.download(ctx);
            return None;
        }
        (State::Available(r), Some(2)) => Some(UpdateResult::Skip(r.version.clone())),
        (State::Ready(..), Some(0)) if blocked.is_some() => {
            up.state = State::Message(blocked.unwrap_or_default().to_string());
            return None;
        }
        (State::Ready(r, exe), Some(0)) => Some(UpdateResult::Install(exe.clone(), r.digest.clone().unwrap_or_default())),
        _ if hit.is_none() && !close => return None,
        _ => None,
    };
    // Any other button (Remind me later, Cancel, Later, OK) or × closes the window.
    up.dismiss();
    out
}

fn body(ui: &mut Ui, state: &State, auto_check: &mut bool, note: Option<&str>) -> Option<usize> {
    match state {
        State::Idle => None,
        State::Checking => {
            ui.label("Checking for updates...");
            ui.add_space(66.0);
            super::button_row(ui, &[("Cancel", true)])
        }
        State::Available(r) => {
            ui.label(RichText::new(format!("Milan Pro {} is available{}.", r.version, if r.prerelease { " (preview)" } else { "" })).strong());
            ui.label(format!("You have version {CURRENT}."));
            ui.add_space(4.0);
            ui.label("Release notes:");
            super::field_box(ui, 4, |ui| {
                ScrollArea::vertical().max_height(250.0).auto_shrink(false).show(ui, |ui| {
                    ui.label(if r.notes.trim().is_empty() { "(No release notes.)" } else { r.notes.as_str() });
                });
            });
            widgets::checkbox(ui, auto_check, "Check for updates automatically");
            ui.label(RichText::new(verification_note()).small());
            ui.add_space(6.0);
            super::button_row(ui, &[("Download and install", true), ("Remind me later", true), ("Skip this version", true)])
        }
        State::Downloading(r, done) => {
            ui.label(format!("Downloading {}...", r.name));
            let mb = |b: u64| b as f64 / 1_048_576.0;
            let frac = if r.size > 0 { (*done as f32 / r.size as f32).min(1.0) } else { 0.0 };
            widgets::progress(ui, frac, Some(format!("{:.1} of {:.1} MB", mb(*done), mb(r.size))), None);
            ui.add_space(38.0);
            super::button_row(ui, &[("Cancel", true)])
        }
        State::Ready(r, _) => {
            ui.label(format!("Milan Pro {} was downloaded and verified.", r.version));
            ui.label(RichText::new(verification_note()).small());
            match note {
                Some(n) => ui.label(RichText::new(n).small()),
                None => ui.label("Milan Pro will close, install the update and start again."),
            };
            ui.add_space(if note.is_some() { 8.0 } else { 26.0 });
            super::button_row(ui, &[("Install and restart", true), ("Later", true)])
        }
        State::Message(m) => {
            ui.label(m.as_str());
            ui.add_space(66.0);
            super::button_row(ui, &[("OK", true)])
        }
    }
}

/// R46: Help › Roll Back to Previous Version (`failed_start` false), or the offer when this version failed to start
/// twice since it was updated. Some(true): roll back to `previous`; Some(false): keep this version (close).
pub fn rollback_window(ctx: &Context, previous: Option<&Previous>, failed_start: bool) -> Option<bool> {
    let (hit, close) = super::modal(ctx, "rollback", "Roll Back", vec2(480.0, 210.0), |ui| {
        if failed_start {
            ui.label(RichText::new(format!("Milan Pro {CURRENT} failed to start twice since it was updated.")).strong());
        } else {
            ui.label(RichText::new("Go back to the version installed before the last update?").strong());
        }
        ui.add_space(4.0);
        match previous {
            Some(p) => {
                ui.label(format!("Milan Pro will close, install version {} from the copy it kept and start again. Projects and preferences are kept.", p.version));
                ui.label(RichText::new(p.exe.display().to_string()).small());
            }
            None => {
                ui.label("No earlier installer is kept, so there is nothing to roll back to.");
                ui.label("Turn on Preferences › Update › \"Keep the previous version's installer\" before the next update, or install an earlier release from GitHub by hand.");
            }
        }
        ui.add_space(12.0);
        super::button_row(ui, &[("Roll back", previous.is_some()), (if failed_start { "Keep this version" } else { "Cancel" }, true)])
    });
    match hit {
        Some(0) => Some(true),
        Some(_) => Some(false),
        None if close => Some(false),
        None => None,
    }
}
