//! R30: File › Export › Milan Pro Project Archive...: the folder, the trim option with its handles, the media the
//! archive will hold with their sizes, then progress while a worker thread copies, and the result with Open Folder.
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use eframe::egui::*;
use crate::interchange::archive::{self, Item, Options, Progress, Report};
use crate::project::Project;
use crate::ui::{theme, widgets};

enum Msg {
    Progress(Progress),
    Done(Result<Report, String>),
}

enum State {
    Idle,
    Running { rx: Receiver<Msg>, cancel: Arc<AtomicBool>, done: usize, total: usize, current: String },
    Done(Report),
    Failed(String),
}

pub struct ArchiveDlg {
    pub dir: String,
    pub opts: Options,
    items: Vec<Item>,
    state: State,
}

/// Bytes as the dialog shows them.
pub fn size_text(bytes: u64) -> String {
    let mb = bytes as f64 / 1_048_576.0;
    if mb >= 1024.0 { format!("{:.2} GB", mb / 1024.0) } else if mb >= 10.0 { format!("{mb:.0} MB") } else if mb >= 1.0 { format!("{mb:.1} MB") } else { format!("{:.0} KB", bytes as f64 / 1024.0) }
}

impl ArchiveDlg {
    /// The folder defaults to "<name> Archive" next to the project (or in Documents for an unsaved one).
    pub fn new(project: &Project) -> Self {
        let base = project.path.as_ref().and_then(|p| p.parent()).map(Path::to_path_buf).unwrap_or_else(documents);
        let name = if project.name.is_empty() { "Project".to_string() } else { project.name.clone() };
        Self { dir: base.join(format!("{name} Archive")).display().to_string(), opts: Options::default(), items: archive::plan(project), state: State::Idle }
    }

    fn running(&self) -> bool {
        matches!(self.state, State::Running { .. })
    }

    fn poll(&mut self, ctx: &Context) {
        let State::Running { rx, done, total, current, .. } = &mut self.state else { return };
        let mut finished = None;
        while let Ok(m) = rx.try_recv() {
            match m {
                Msg::Progress(p) => (*done, *total, *current) = (p.done, p.total, p.current),
                Msg::Done(r) => finished = Some(r),
            }
        }
        match finished {
            Some(Ok(r)) => self.state = State::Done(r),
            Some(Err(e)) => self.state = State::Failed(e),
            None => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
        }
    }

    fn start(&mut self, ctx: &Context, project: &Project) {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let (project, dest, opts, stop, ctx2) = (project.clone(), PathBuf::from(self.dir.trim()), self.opts.clone(), cancel.clone(), ctx.clone());
        std::thread::spawn(move || {
            let progress = |p: Progress| {
                let _ = tx.send(Msg::Progress(p));
                ctx2.request_repaint();
            };
            let r = archive::run(project, &dest, &opts, &progress, &stop).map_err(|e| e.to_string());
            let _ = tx.send(Msg::Done(r));
            ctx2.request_repaint();
        });
        self.state = State::Running { rx, cancel, done: 0, total: self.items.len(), current: String::new() };
    }

    /// True when closed.
    pub fn show(&mut self, ctx: &Context, project: &Project) -> bool {
        self.poll(ctx);
        let running = self.running();
        let online = self.items.iter().filter(|i| i.online).count();
        let total: u64 = self.items.iter().map(|i| i.size).sum();
        let estimate = archive::estimate(&self.items, &self.opts);
        let (hit, close) = super::modal(ctx, "archive_project", "Archive Project", vec2(600.0, 460.0), |ui| {
            super::row(ui, "Archive folder:", 110.0, !running, |ui| {
                widgets::edit_enabled(ui, !running, TextEdit::singleline(&mut self.dir).desired_width(ui.available_width() - 80.0));
                if widgets::push_button_enabled(ui, !running, "Browse...").clicked() {
                    let start = Path::new(&self.dir).parent().filter(|p| p.is_dir()).map(Path::to_path_buf).unwrap_or_else(documents);
                    if let Some(p) = rfd::FileDialog::new().set_title("Archive folder").set_directory(start).pick_folder() {
                        self.dir = p.display().to_string();
                    }
                }
            });
            ui.add_space(4.0);
            widgets::checkbox_enabled(ui, !running, &mut self.opts.trim, "Trim media to the ranges used on the timeline (video is re-encoded; audio is cut losslessly)");
            super::row(ui, "Handles:", 110.0, !running && self.opts.trim, |ui| {
                widgets::spin_enabled(ui, !running && self.opts.trim, &mut self.opts.handles, 0.5, |d| d.range(0.0..=60.0).speed(0.1).suffix(" s").fixed_decimals(1));
                ui.label("extra on either side of each used range");
            });
            ui.add_space(6.0);
            let about = if self.opts.trim && estimate < total { format!("; about {} after trimming", size_text(estimate)) } else { String::new() };
            ui.label(format!("Media: {} file(s), {}{about}{}", self.items.len(), size_text(total), if online < self.items.len() { format!("; {} offline (left as they are)", self.items.len() - online) } else { String::new() }));
            super::field_box(ui, 4, |ui| {
                ui.set_width(ui.available_width());
                ScrollArea::vertical().max_height(150.0).auto_shrink([false, false]).show(ui, |ui| {
                    for it in &self.items {
                        let used = match (it.online, it.used, archive::trim_range(it, self.opts.handles).filter(|_| self.opts.trim)) {
                            (false, ..) => "offline".to_string(),
                            (_, _, Some((a, b))) => format!("trimmed to {a:.1} s – {b:.1} s"),
                            (_, Some((a, b)), None) => format!("uses {a:.1} s – {b:.1} s"),
                            (_, None, None) => "not on the timeline".to_string(),
                        };
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&it.name).strong());
                            ui.label(format!("{}  ({used})", size_text(it.size)));
                        });
                    }
                    if self.items.is_empty() {
                        ui.label("No media files: the archive holds the project alone.");
                    }
                });
            });
            ui.add_space(6.0);
            match &self.state {
                State::Idle => {}
                State::Running { done, total, current, .. } => {
                    widgets::progress(ui, if *total == 0 { 1.0 } else { *done as f32 / *total as f32 }, None, None);
                    ui.label(if current.is_empty() { "Saving the project...".to_string() } else { format!("Copying {current}...") });
                }
                State::Done(r) => {
                    ui.horizontal(|ui| {
                        ui.label(format!("Archived to {} ({} copied, {} trimmed).", r.project.display(), r.copied, r.trimmed));
                        if widgets::push_button(ui, "Open Folder").clicked() {
                            super::open_folder(&r.project);
                        }
                    });
                    for s in r.skipped.iter().map(|s| format!("Offline, left as it was: {s}")).chain(r.notes.iter().cloned()) {
                        ui.label(s);
                    }
                }
                State::Failed(e) => {
                    ui.colored_label(theme::get().warning_text, if e == "cancelled" { "Archiving cancelled; the folder holds what was copied so far.".to_string() } else { format!("Archiving failed: {e}") });
                }
            }
            ui.add_space(8.0);
            let dir_ok = !self.dir.trim().is_empty();
            let finished = matches!(self.state, State::Done(_) | State::Failed(_));
            let buttons: &[(&str, bool)] = if finished { &[("Close", true)] } else { &[("Archive", dir_ok && !running), ("Cancel", true)] };
            super::button_row(ui, buttons)
        });
        let finished = matches!(self.state, State::Done(_) | State::Failed(_));
        match hit {
            Some(0) if finished => true,
            Some(0) => {
                self.start(ctx, project);
                false
            }
            Some(_) | None if close || hit.is_some() => {
                if let State::Running { cancel, .. } = &self.state {
                    // Esc or Cancel while copying: the worker stops between files; the dialog says so when it has.
                    cancel.store(true, Ordering::Relaxed);
                    return false;
                }
                true
            }
            _ => false,
        }
    }
}

fn documents() -> PathBuf {
    std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("Documents")).filter(|p| p.is_dir()).unwrap_or_else(std::env::temp_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_well() {
        assert_eq!(size_text(500), "0 KB");
        assert_eq!(size_text(300 * 1024), "300 KB");
        assert_eq!(size_text(3 * 1_048_576 + 524_288), "3.5 MB");
        assert_eq!(size_text(250 * 1_048_576), "250 MB");
        assert_eq!(size_text(3 * 1024 * 1_048_576), "3.00 GB");
    }

    #[test]
    fn dialog_defaults_next_to_the_project() {
        let mut p = Project::new("Show");
        p.path = Some(PathBuf::from(r"D:\work\Show.mnle"));
        let d = ArchiveDlg::new(&p);
        assert_eq!((d.dir.as_str(), d.items.len(), d.running()), (r"D:\work\Show Archive", 0, false));
        assert_eq!(d.opts, Options::default());
    }
}
