//! R12: the Render Queue window, VEGAS's Batch Render and Render Audio Tracks scripts (Tools › Scripting) and the
//! guard shown when Milan Pro is closed or updated while a render runs. Every render goes through
//! `render::RenderQueue`, which renders a snapshot of the project taken when the render was queued.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use eframe::egui::*;
use crate::project::Project;
use crate::render::{BUILTIN, BatchMode, JobState, Opts, RenderQueue, Template, batch, per_track};
use crate::settings::Prefs;
use super::dialogs;

/// Tools › Render Queue...: the queued renders in the order they render.
#[derive(Default)]
pub struct QueueWindow {
    pub open: bool,
    /// Batch Render's jobs: while one waits or renders the window is modal, as the script's renders are (VEGAS shows
    /// each with its progress dialog), unless Render in the background is on.
    pub batch: Vec<String>,
    modal: bool,
    sel: Option<String>,
}

impl QueueWindow {
    pub fn is_modal(&self) -> bool {
        self.open && self.modal
    }

    pub fn show(&mut self, ctx: &Context, queue: &mut RenderQueue) {
        self.modal = self.batch.iter().any(|id| queue.job(id).is_some_and(|j| matches!(j.state, JobState::Queued | JobState::Running)));
        if !self.open {
            return;
        }
        let (size, modal) = (vec2(780.0, 360.0), self.modal);
        let body = |ui: &mut Ui| self.body(ui, queue);
        let (closed, x) = if modal { dialogs::modal(ctx, "render_queue", "Render Queue", size, body) } else { dialogs::window(ctx, "render_queue", "Render Queue", size, body) };
        if closed || x {
            if self.modal {
                // Closing the batch's progress cancels the batch, as the script's Cancel does.
                for id in std::mem::take(&mut self.batch) {
                    queue.cancel(&id);
                }
            }
            self.open = false;
        }
    }

    /// The list and its buttons; true when Close was clicked.
    fn body(&mut self, ui: &mut Ui, queue: &mut RenderQueue) -> bool {
        let t = super::theme::get();
        let d = t.dialog();
        Frame::NONE.fill(d.field).stroke(Stroke::new(1.0_f32, d.field_border)).inner_margin(4).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ScrollArea::vertical().id_salt("queue_list").max_height(230.0).min_scrolled_height(230.0).auto_shrink(false).show(ui, |ui| {
                Grid::new("queue_grid").num_columns(5).striped(true).spacing(vec2(12.0, 4.0)).show(ui, |ui| {
                    for h in ["#", "File", "Template", "Range", "Status"] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for (k, j) in queue.jobs.iter().enumerate() {
                        ui.label((k + 1).to_string());
                        let name = j.out.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned());
                        let on = self.sel.as_deref() == Some(j.id.as_str());
                        if ui.selectable_label(on, name).on_hover_text(format!("{}\nProject: {}", j.out.display(), j.project)).clicked() {
                            self.sel = Some(j.id.clone());
                        }
                        ui.label(&j.template.name);
                        ui.label(format!("{:.2} s to {:.2} s", j.range.0, j.range.1));
                        match queue.running(&j.id) {
                            Some(r) => ui.add(ProgressBar::new(r.fraction()).show_percentage().desired_width(180.0)),
                            None if matches!(j.state, JobState::Failed(_)) => ui.colored_label(t.warning_text, j.state.label()),
                            None => ui.label(j.state.label()),
                        }
                        .on_hover_text(if let JobState::Failed(e) = &j.state { e.clone() } else { j.state.label() });
                        ui.end_row();
                    }
                });
                if queue.jobs.is_empty() {
                    ui.label(RichText::new("No renders are queued. Render As › Add to Render Queue adds one.").color(t.text_dim));
                }
            });
        });
        let job = self.sel.as_ref().and_then(|id| queue.job(id)).cloned();
        let st = job.as_ref().map(|j| j.state.clone());
        let mut close = false;
        ui.horizontal(|ui| {
            let id = job.as_ref().map(|j| j.id.clone()).unwrap_or_default();
            let has = job.is_some();
            if ui.add_enabled(has, Button::new("Move Up")).clicked() {
                queue.shift(&id, -1);
            }
            if ui.add_enabled(has, Button::new("Move Down")).clicked() {
                queue.shift(&id, 1);
            }
            let paused = st == Some(JobState::Paused);
            let can_pause = matches!(st, Some(JobState::Queued | JobState::Paused));
            if ui.add_enabled(can_pause, Button::new(if paused { "Resume" } else { "Pause" })).on_disabled_hover_text("A render that has started can only be cancelled.").clicked() {
                queue.pause(&id);
            }
            if ui.add_enabled(matches!(st, Some(JobState::Queued | JobState::Paused | JobState::Running)), Button::new("Cancel")).clicked() {
                queue.cancel(&id);
            }
            if ui.add_enabled(st.as_ref().is_some_and(JobState::retryable), Button::new("Retry")).clicked() {
                queue.retry(&id);
            }
            if ui.add_enabled(has && st != Some(JobState::Running), Button::new("Remove")).clicked() {
                queue.remove(&id);
                self.sel = None;
            }
            if let Some(log) = job.as_ref().and_then(|j| j.log.clone()) {
                if ui.link("Show log").on_hover_text(log.display().to_string()).clicked() {
                    dialogs::open_file(&log);
                }
            }
            if st == Some(JobState::Done) && ui.button("Open Folder").clicked() {
                dialogs::open_folder(&job.as_ref().map(|j| j.out.clone()).unwrap_or_default());
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                close = dialogs::button(ui, "Close", true, true).clicked();
                if ui.button("Clear Finished").clicked() {
                    queue.clear_finished();
                }
            });
        });
        close
    }
}

/// `<project folder>\<project name>_` (Documents for an unsaved project), the script's default base file name.
fn default_base(project: &Project) -> PathBuf {
    match project.path.as_ref().and_then(|p| p.parent()) {
        Some(dir) => dir.join(format!("{}_", project.name)),
        None => documents().join(format!("{}_", project.name)),
    }
}

fn documents() -> PathBuf {
    std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("Documents")).filter(|p| p.is_dir()).unwrap_or_else(std::env::temp_dir)
}

/// Ask before replacing files, as the scripts do: true to go on.
fn may_overwrite(outs: &[&Path]) -> bool {
    !outs.iter().any(|o| o.exists())
        || rfd::MessageDialog::new()
            .set_title("Overwrite files?")
            .set_description("File(s) exists. Do you want to overwrite them?")
            .set_level(rfd::MessageLevel::Warning)
            .set_buttons(rfd::MessageButtons::OkCancel)
            .show()
            == rfd::MessageDialogResult::Ok
}

/// Tools › Scripting › Batch Render: the project (or its loop region, or each region) with every ticked template.
pub struct BatchRender {
    base: String,
    ticked: BTreeSet<String>,
    mode: BatchMode,
    error: Option<String>,
}

impl BatchRender {
    pub fn new(project: &Project) -> Self {
        Self { base: default_base(project).display().to_string(), ticked: BTreeSet::new(), mode: BatchMode::Project, error: None }
    }

    /// Some(ids queued) once closed (none when cancelled).
    pub fn show(&mut self, ctx: &Context, project: &Project, prefs: &Prefs, queue: &mut RenderQueue) -> Option<Vec<String>> {
        let templates: Vec<&Template> = BUILTIN.iter().chain(&prefs.render_templates).collect();
        let (hit, close) = dialogs::modal(ctx, "batch_render", "Batch Render", vec2(560.0, 520.0), |ui| {
            dialogs::row(ui, "Base File Name:", 100.0, true, |ui| {
                ui.add(TextEdit::singleline(&mut self.base).desired_width(ui.available_width() - 80.0));
                if ui.button("Browse...").clicked() {
                    let p = PathBuf::from(&self.base);
                    let mut fd = rfd::FileDialog::new().set_file_name(p.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned()));
                    if let Some(dir) = p.parent().filter(|d| d.is_dir()) {
                        fd = fd.set_directory(dir);
                    }
                    if let Some(f) = fd.save_file() {
                        self.base = f.with_extension("").display().to_string();
                    }
                }
            });
            ui.label("Render Templates:");
            let d = super::theme::get().dialog();
            Frame::NONE.fill(d.field).stroke(Stroke::new(1.0_f32, d.field_border)).inner_margin(4).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ScrollArea::vertical().id_salt("batch_tree").max_height(300.0).min_scrolled_height(300.0).auto_shrink(false).show(ui, |ui| {
                    let mut formats: Vec<String> = Vec::new();
                    for t in &templates {
                        if !formats.contains(&t.format()) {
                            formats.push(t.format());
                        }
                    }
                    for f in &formats {
                        CollapsingHeader::new(f).id_salt(("batch", f)).show(ui, |ui| {
                            for t in templates.iter().filter(|t| &t.format() == f) {
                                let mut on = self.ticked.contains(&t.name);
                                if ui.checkbox(&mut on, &t.name).changed() {
                                    if on { self.ticked.insert(t.name.clone()) } else { self.ticked.remove(&t.name) };
                                }
                            }
                        });
                    }
                });
            });
            let tl = &project.timeline;
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.mode, BatchMode::Project, "Render Project");
                ui.add_enabled_ui(tl.loop_region.is_some_and(|(a, b)| b > a), |ui| ui.radio_value(&mut self.mode, BatchMode::Selection, "Render Selection"));
                ui.add_enabled_ui(!tl.regions.is_empty(), |ui| ui.radio_value(&mut self.mode, BatchMode::Regions, "Render Regions"));
            });
            if let Some(e) = &self.error {
                ui.colored_label(super::theme::get().warning_text, e);
            }
            dialogs::button_row(ui, &[("OK", true), ("Cancel", true)])
        });
        match (hit, close) {
            (Some(0), _) => {
                let chosen: Vec<&Template> = templates.iter().copied().filter(|t| self.ticked.contains(&t.name)).collect();
                match self.queue(project, &chosen, prefs.quickfade, queue) {
                    Ok(ids) => Some(ids),
                    Err(e) => {
                        self.error = Some(e);
                        None
                    }
                }
            }
            (Some(_), _) | (None, true) => Some(Vec::new()),
            _ => None,
        }
    }

    fn queue(&self, project: &Project, chosen: &[&Template], quickfade: bool, queue: &mut RenderQueue) -> Result<Vec<String>, String> {
        if chosen.is_empty() {
            return Err("No render templates selected.".into());
        }
        let base = PathBuf::from(self.base.trim());
        if !base.parent().is_some_and(|d| d.is_absolute() && d.is_dir()) {
            return Err("The output directory does not exist.".into());
        }
        let jobs = batch(project, chosen, &base, self.mode);
        if let Some((_, out, _)) = jobs.iter().find(|j| j.1.as_os_str().len() > 260) {
            return Err(format!("File name too long: {}", out.display()));
        }
        if let Some(e) = chosen.iter().find_map(|t| t.check().err()) {
            return Err(e);
        }
        if !may_overwrite(&jobs.iter().map(|j| j.1.as_path()).collect::<Vec<_>>()) {
            return Ok(Vec::new());
        }
        jobs.into_iter().map(|(t, out, range)| queue.add(project, &t, out, range, Opts { quickfade, ..Opts::default() })).collect()
    }
}

/// Tools › Scripting › Render Audio Tracks: each audio track soloed, rendered with one template into one folder.
pub struct RenderTracks {
    dir: String,
    template: String,
    loop_only: bool,
    error: Option<String>,
}

impl RenderTracks {
    pub fn new(project: &Project) -> Self {
        let dir = project.path.as_ref().and_then(|p| p.parent()).map_or_else(documents, Path::to_path_buf);
        let first_audio = BUILTIN.iter().find(|t| t.video.is_none()).map_or(String::new(), |t| t.name.clone());
        Self { dir: dir.display().to_string(), template: first_audio, loop_only: true, error: None }
    }

    /// Some(ids queued) once closed (none when cancelled).
    pub fn show(&mut self, ctx: &Context, project: &Project, prefs: &Prefs, queue: &mut RenderQueue) -> Option<Vec<String>> {
        let templates: Vec<&Template> = BUILTIN.iter().chain(&prefs.render_templates).filter(|t| !t.sequence()).collect();
        let can_loop = project.timeline.loop_region.is_some_and(|(a, b)| b > a);
        let (hit, close) = dialogs::modal(ctx, "render_tracks", "Render Audio Tracks", vec2(500.0, 210.0), |ui| {
            dialogs::row(ui, "Output Folder:", 100.0, true, |ui| {
                ui.add(TextEdit::singleline(&mut self.dir).desired_width(ui.available_width() - 70.0));
                if ui.button("Browse").clicked() {
                    if let Some(p) = rfd::FileDialog::new().set_directory(&self.dir).pick_folder() {
                        self.dir = p.display().to_string();
                    }
                }
            });
            dialogs::row(ui, "Template:", 100.0, true, |ui| {
                ComboBox::from_id_salt("tracks_template").width(ui.available_width()).selected_text(self.template.clone()).show_ui(ui, |ui| {
                    for t in &templates {
                        ui.selectable_value(&mut self.template, t.name.clone(), format!("{}  ({})", t.name, t.format()));
                    }
                });
            });
            let mut on = self.loop_only && can_loop;
            if ui.add_enabled(can_loop, Checkbox::new(&mut on, "Render loop region only")).changed() {
                self.loop_only = on;
            }
            if let Some(e) = &self.error {
                ui.colored_label(super::theme::get().warning_text, e);
            }
            dialogs::button_row(ui, &[("OK", true), ("Cancel", true)])
        });
        match (hit, close) {
            (Some(0), _) => {
                let Some(t) = templates.iter().find(|t| t.name == self.template) else {
                    self.error = Some("render template not selected.".into());
                    return None;
                };
                let dir = PathBuf::from(self.dir.trim());
                if !dir.is_absolute() || !dir.is_dir() {
                    self.error = Some("Choose a folder that exists, as a full path (C:\\...).".into());
                    return None;
                }
                let range = project.timeline.loop_region.filter(|_| self.loop_only && can_loop).unwrap_or((0.0, crate::render::content_end(project)));
                let jobs = per_track(project, t, &dir);
                if !may_overwrite(&jobs.iter().map(|j| j.1.as_path()).collect::<Vec<_>>()) {
                    return Some(Vec::new());
                }
                let ids: Result<Vec<String>, String> = jobs.iter().map(|(p, out)| queue.add(p, t, out.clone(), range, Opts { quickfade: prefs.quickfade, ..Opts::default() })).collect();
                ids.map_err(|e| self.error = Some(e)).ok()
            }
            (Some(_), _) | (None, true) => Some(Vec::new()),
            _ => None,
        }
    }
}

/// What the render guard asked: Exit (or Install and restart) while a render runs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Guard {
    /// Close (or install) once the render queue has finished.
    Wait,
    /// Cancel the renders and close.
    CancelRender,
    /// Close now; the renders start again at the next start.
    Keep,
    /// Neither: stay open (no update is installed).
    Back,
}

/// The guard's buttons. An update never interrupts a render: it can only wait for the queue, or not install now.
pub fn guard_choices(install: bool) -> &'static [(&'static str, Guard)] {
    if install {
        &[("Wait", Guard::Wait), ("Not Now", Guard::Back)]
    } else {
        &[("Wait", Guard::Wait), ("Cancel Render", Guard::CancelRender), ("Keep in Queue", Guard::Keep), ("Back", Guard::Back)]
    }
}

/// "A render is running: wait, cancel, or keep it in the queue for the next start" (`install`: Install and restart).
pub fn guard(ctx: &Context, install: bool) -> Option<Guard> {
    let text = if install {
        "A render is running. Install and restart waits until the render queue has finished, then installs the update."
    } else {
        "A render is running. Wait for the render queue to finish and then close, cancel the render and close, or keep it in the render queue for the next start?"
    };
    let choices = guard_choices(install);
    let (hit, close) = dialogs::modal(ctx, "render_guard", "Milan Pro", vec2(470.0, 150.0), |ui| {
        ui.add(Label::new(text).wrap());
        ui.add_space(14.0);
        dialogs::button_row(ui, &choices.iter().map(|(l, _)| (*l, true)).collect::<Vec<_>>())
    });
    hit.map(|k| choices[k].1).or(close.then_some(Guard::Back))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R12: Exit while a render runs offers wait, cancel or keep for next start; Install and restart only waits (or
    /// does not install now): an update never interrupts a render.
    #[test]
    fn exit_and_update_guards() {
        let exit: Vec<Guard> = guard_choices(false).iter().map(|c| c.1).collect();
        assert_eq!(exit, [Guard::Wait, Guard::CancelRender, Guard::Keep, Guard::Back]);
        let install: Vec<Guard> = guard_choices(true).iter().map(|c| c.1).collect();
        assert!(install.contains(&Guard::Wait) && !install.contains(&Guard::CancelRender) && !install.contains(&Guard::Keep));
    }

    #[test]
    fn script_defaults() {
        let mut p = Project::new("Show");
        p.path = Some(r"C:\Work\Show.mnle".into());
        assert_eq!(default_base(&p), Path::new(r"C:\Work\Show_"));
        let b = BatchRender::new(&p);
        let mut q = RenderQueue::default();
        assert_eq!(b.queue(&p, &[], true, &mut q), Err("No render templates selected.".into()));
        let b = BatchRender { base: r"C:\no\such\folder\x_".into(), ..BatchRender::new(&p) };
        assert_eq!(b.queue(&p, &[&BUILTIN[0]], true, &mut q), Err("The output directory does not exist.".into()));
        assert!(q.jobs.is_empty());
        assert_eq!(RenderTracks::new(&p).template, "WAV audio only");
    }
}
