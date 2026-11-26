//! R42-R46: the reliability features' app state and dialogs: Save Version, the offer when a project file cannot be
//! read, the notes from opening a file (migration, read-only, repairs), and (R43-R46) crash recovery, the crash-report
//! offer, the diagnostic report and the roll-back offer.
use eframe::egui;
use std::path::PathBuf;
use crate::project::CorruptOffer;
use crate::ui::MenuAction;
use crate::ui::dialogs::{self, CorruptChoice, CrashChoice, DiagAction, RecoverChoice};
use super::MeridianApp;

#[derive(Default)]
pub struct Rel {
    /// File › Save Version, open.
    pub save_version: Option<dialogs::SaveVersion>,
    /// A project file that could not be read, with its backup and autosave.
    pub corrupt: Option<CorruptOffer>,
    /// What `Project::open` had to say about the file just opened (intro, lines).
    pub open_notes: Option<(String, Vec<String>)>,
    /// R43: autosave and session markers are on (off in the harness unless MILAN_APPDATA points at a test folder).
    pub autosave: bool,
    /// R43: the Recover Projects dialog, open.
    pub recover: Option<dialogs::Recover>,
    /// R44: the newest crash report not offered yet, and Help › Create Diagnostic Report, open.
    pub crash_offer: Option<PathBuf>,
    pub diagnostics: Option<dialogs::Diagnostics>,
    /// R46: the Roll Back dialog, open (true: offered because this version failed to start twice), the installer it
    /// would run, and whether the first frame has cleared the start marker.
    pub rollback: Option<bool>,
    pub previous: Option<crate::updater::Previous>,
    pub started: bool,
}

impl Rel {
    /// A modal of this module is open (keys go to it, not the timeline).
    pub fn modal(&self) -> bool {
        self.save_version.is_some() || self.corrupt.is_some() || self.open_notes.is_some() || self.recover.is_some() || self.crash_offer.is_some() || self.diagnostics.is_some() || self.rollback.is_some()
    }
}

impl MeridianApp {
    /// R43, at start: the autosaves of sessions that are not running any more go into the Recover Projects dialog.
    pub(super) fn offer_recovery(&mut self) {
        let Some(dir) = crate::autosave::sessions_dir() else { return };
        let items = crate::autosave::recoverable(crate::autosave::stale_sessions(&dir, crate::autosave::process_running));
        if !items.is_empty() {
            log::info!("{} project(s) to recover from earlier sessions", items.len());
            self.rel.recover = Some(dialogs::Recover::new(items));
        }
    }

    /// R44, at start: the newest crash report, once.
    pub(super) fn offer_crash_report(&mut self) {
        let Some(dir) = crate::logger::crashes_dir() else { return };
        let Some(newest) = crate::logger::crash_reports(&dir).into_iter().next() else { return };
        let name = newest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if self.prefs.crash_seen.as_deref() != Some(name.as_str()) {
            log::info!("offering crash report {name}");
            self.rel.crash_offer = Some(newest);
        }
    }

    pub(super) fn show_reliability(&mut self, ctx: &egui::Context) {
        if !self.rel.started {
            // R46: the first frame: this version starts, so the start marker goes.
            self.rel.started = true;
            if self.rel.autosave {
                if let Some(d) = crate::updater::updates_dir() {
                    crate::updater::start_ok(&d);
                }
            }
        }
        if let Some(failed) = self.rel.rollback {
            match dialogs::rollback_window(ctx, self.rel.previous.as_ref(), failed) {
                Some(true) => {
                    self.rel.rollback = None;
                    if let Some(p) = self.rel.previous.clone() {
                        log::info!("rolling back to {} with {}", p.version, p.exe.display());
                        self.install_on_exit = Some((p.exe, p.sha256));
                        self.request(ctx, MenuAction::Exit);
                    }
                }
                Some(false) => self.rel.rollback = None,
                None => {}
            }
        }
        if let Some(report) = self.rel.crash_offer.clone() {
            if let Some(choice) = dialogs::crash_offer_window(ctx, &report) {
                match choice {
                    CrashChoice::OpenReport => dialogs::open_file(&report),
                    CrashChoice::ReportOnGitHub => dialogs::open_url(&dialogs::issue_url(crate::updater::CURRENT, &report)),
                    CrashChoice::Close => {}
                }
                self.rel.crash_offer = None;
                self.prefs.crash_seen = report.file_name().map(|n| n.to_string_lossy().into_owned());
                self.save_prefs();
            }
        }
        if let Some(choice) = self.rel.diagnostics.as_mut().and_then(|d| d.show(ctx)) {
            if let DiagAction::OpenFolder(dir) = choice {
                dialogs::open_file(&dir);
            }
            self.rel.diagnostics = None;
        }
        if let Some(choice) = self.rel.recover.as_mut().and_then(|r| r.show(ctx)) {
            let r = self.rel.recover.as_mut().expect("the dialog is open");
            match choice {
                RecoverChoice::Open(n) => {
                    let it = r.items[n].clone();
                    match it.open_copy() {
                        Ok(p) => {
                            log::info!("recovered {} from {}", it.name, it.autosave.display());
                            r.settled(n);
                            self.switch_project(p);
                            self.message = Some(format!("Recovered {} from its autosave as an unsaved copy; Save asks where to put it.", it.name));
                        }
                        Err(e) => self.message = Some(format!("Could not open {}: {e}", it.autosave.display())),
                    }
                }
                RecoverChoice::Discard(n) => {
                    r.items[n].delete_files();
                    r.settled(n);
                }
                RecoverChoice::KeepForLater => self.rel.recover = None,
            }
        }
        if self.rel.recover.as_ref().is_some_and(|r| r.items.is_empty()) {
            self.rel.recover = None;
        }
        if let Some(d) = &mut self.rel.save_version {
            match d.show(ctx) {
                Some(true) => {
                    let (path, note) = (d.path.clone(), d.note.trim().to_string());
                    self.rel.save_version = None;
                    self.project.version_note = (!note.is_empty()).then_some(note);
                    let r = self.project.save(&path).map(|_| true).map_err(|e| format!("{}: {e}", path.display()));
                    if self.save_result(r) {
                        log::info!("saved version {}", path.display());
                    }
                }
                Some(false) => self.rel.save_version = None,
                None => {}
            }
        }
        if let Some(offer) = &self.rel.corrupt {
            let file = match dialogs::corrupt_window(ctx, offer) {
                Some(CorruptChoice::Backup) => offer.backup.clone(),
                Some(CorruptChoice::Autosave) => offer.autosave.clone(),
                Some(CorruptChoice::Cancel) => {
                    self.rel.corrupt = None;
                    None
                }
                None => None,
            };
            if let Some(file) = file {
                let offer = self.rel.corrupt.take().expect("the offer is open");
                match offer.open_copy(&file) {
                    Ok(p) => {
                        log::info!("opened {} as an unsaved copy of {}", file.display(), offer.path.display());
                        self.switch_project(p);
                        self.message = Some(format!("Opened {} as an unsaved copy; Save asks where to put it.", file.display()));
                    }
                    Err(e) => self.message = Some(format!("Could not open {}: {e}", file.display())),
                }
            }
        }
        if self.rel.open_notes.as_ref().is_some_and(|(intro, lines)| dialogs::report(ctx, "Open Project", intro, lines)) {
            self.rel.open_notes = None;
        }
    }
}
