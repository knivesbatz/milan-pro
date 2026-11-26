//! R23: Tools › Selectively Prerender Video (Shift+M) and Clean Up Prerendered Video. The first picks a template and
//! renders the sections of the selection that are more than a plain event (`plan::work_sections`) one after another
//! through `engine::render` into the project's prerender folder, registering each as it finishes
//! (`engine::prerender::add`); the second deletes the project's prerendered files, all of them or the inactive ones.
use std::path::PathBuf;
use std::sync::Arc;
use eframe::egui::*;
use crate::engine::plan::{self, VideoPlan};
use crate::engine::prerender::{self, Entry};
use crate::project::Project;
use crate::render::{Job, Opts, Status, Template};
use crate::settings::Prefs;
use crate::ui::widgets;

enum State {
    Setup,
    /// Rendering section `at` of `sections` into `file` (its entry once done).
    Running { job: Job, at: usize, entry: Entry },
    Done { sections: usize, skipped: bool },
    Nothing,
    Failed(String),
}

pub struct PrerenderDlg {
    template: String,
    loop_only: bool,
    has_loop: bool,
    state: State,
    sections: Vec<(i64, i64)>,
    vp: Option<Arc<VideoPlan>>,
    folder: PathBuf,
    /// The screenshot harness: close by itself once done.
    auto_close: bool,
}

impl PrerenderDlg {
    pub fn new(project: &Project, prefs: &Prefs) -> Self {
        let has_loop = project.timeline.loop_region.is_some_and(|(a, b)| b > a);
        let named = prefs.prerender_template.clone().filter(|n| prerender::templates(prefs).iter().any(|t| &t.name == n));
        Self { template: named.unwrap_or_else(|| prerender::DEFAULT_TEMPLATE.into()), loop_only: has_loop, has_loop, state: State::Setup, sections: Vec::new(), vp: None, folder: prerender::folder().unwrap_or_else(prerender::default_folder), auto_close: false }
    }

    pub fn running(&self) -> bool {
        matches!(self.state, State::Running { .. })
    }

    /// The screenshot harness: start at once with the default template and close when done.
    pub fn start_now(&mut self, ctx: &Context, project: &Project, prefs: &mut Prefs) {
        self.auto_close = true;
        self.start(ctx, project, prefs);
    }

    /// The range to prerender: the loop region when asked for, else the whole timeline.
    fn range(&self, project: &Project) -> (f64, f64) {
        match project.timeline.loop_region.filter(|_| self.loop_only && self.has_loop) {
            Some(lp) => lp,
            None => (0.0, crate::render::content_end(project)),
        }
    }

    fn start(&mut self, ctx: &Context, project: &Project, prefs: &mut Prefs) {
        prefs.prerender_template = Some(self.template.clone());
        let vp = Arc::new(prerender::canonical(project));
        let (a, b) = plan::frame_range(self.range(project), vp.fps);
        self.sections = plan::work_sections(&vp, a, b);
        self.vp = Some(vp);
        if self.sections.is_empty() {
            self.state = State::Nothing;
            return;
        }
        if let Err(e) = std::fs::create_dir_all(&self.folder) {
            self.state = State::Failed(format!("Cannot create {}: {e}", self.folder.display()));
            return;
        }
        self.render_section(ctx, project, prefs, 0);
    }

    /// Render section `at` (a job in the background); the next starts when it is done.
    fn render_section(&mut self, ctx: &Context, project: &Project, prefs: &Prefs, at: usize) {
        let Some(vp) = self.vp.clone() else { return };
        let Some(t) = prerender::templates(prefs).into_iter().find(|t| t.name == self.template) else {
            self.state = State::Failed(format!("The template \"{}\" is gone.", self.template));
            return;
        };
        // Video only, at the project's size and rate: what the preview and a render compose.
        let t = Template { audio: None, ..t };
        let (a, b) = self.sections[at];
        let hash = plan::range_hash(&vp, a, b);
        let file = self.folder.join(format!("{hash:016x}.{}", t.ext()));
        let codec = prerender::codec_name(&t.resolved().0.video.as_ref().map_or(String::new(), |v| v.codec.clone()));
        let entry = Entry { file: file.clone(), a, b, fps: vp.fps, fps_str: vp.fps_str.clone(), w: vp.w, h: vp.h, hash, template: t.name.clone(), codec, made: crate::render::utc(std::time::SystemTime::now()) };
        let range = (crate::project::frame_time(a, vp.fps), crate::project::frame_time(b, vp.fps));
        let opts = Opts { quickfade: prefs.quickfade, stretch: false, rotate: false, markers: false, loudness_log: None };
        let job = crate::engine::render::start(project, &t, file, range, opts, ctx.clone());
        self.state = State::Running { job, at, entry };
    }

    fn poll(&mut self, ctx: &Context, project: &Project, prefs: &Prefs) {
        let State::Running { job, at, entry } = &self.state else { return };
        match job.status() {
            Status::Running => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
            Status::Done => {
                prerender::add(entry.clone());
                let next = at + 1;
                if next < self.sections.len() {
                    self.render_section(ctx, project, prefs, next);
                } else {
                    self.state = State::Done { sections: self.sections.len(), skipped: false };
                }
            }
            Status::Cancelled => self.state = State::Done { sections: *at, skipped: true },
            Status::Failed(e) => self.state = State::Failed(e),
        }
    }

    /// True when closed.
    pub fn show(&mut self, ctx: &Context, project: &Project, prefs: &mut Prefs) -> bool {
        self.poll(ctx, project, prefs);
        if self.auto_close && !self.running() {
            return true;
        }
        let running = self.running();
        let templates = prerender::templates(prefs);
        let (a, b) = self.range(project);
        let (hit, close) = super::modal(ctx, "prerender", "Selectively Prerender Video", vec2(520.0, 300.0), |ui| {
            super::row(ui, "Template:", 90.0, !running, |ui| {
                ui.add_enabled_ui(!running, |ui| {
                    widgets::combo(ui, "prerender_tpl", 320.0, self.template.clone(), |ui| {
                        for t in &templates {
                            ui.selectable_value(&mut self.template, t.name.clone(), &t.name);
                        }
                    });
                });
            });
            if let Some(t) = templates.iter().find(|t| t.name == self.template) {
                ui.label(RichText::new(format!("{}  {}", t.format(), t.video_line(&project.settings, false).unwrap_or_default())).size(11.0));
            }
            ui.add_space(4.0);
            widgets::checkbox_enabled(ui, !running && self.has_loop, &mut self.loop_only, "Render loop region only").on_disabled_hover_text("There is no loop region: the whole timeline is prerendered");
            ui.label(format!(
                "Sections of {} that are more than one plain event (FX, transitions, compositing, Track Motion, generated media) are rendered to {}; the rest plays from its files as it is.",
                if self.loop_only && self.has_loop { format!("the loop region ({:.2} s to {:.2} s)", a, b) } else { "the timeline".to_string() },
                self.folder.display()
            ));
            ui.add_space(6.0);
            match &self.state {
                State::Setup => {}
                State::Running { job, at, .. } => {
                    let (sa, sb) = self.sections[*at];
                    let frac = (*at as f32 + job.fraction()) / self.sections.len() as f32;
                    widgets::progress(ui, frac, None, None);
                    let fps = self.vp.as_ref().map_or(30.0, |v| v.fps);
                    ui.label(format!("Section {} of {}: {:.2} s to {:.2} s ({:.0} %)", at + 1, self.sections.len(), sa as f64 / fps, sb as f64 / fps, job.fraction() * 100.0));
                }
                State::Done { sections, skipped } => {
                    ui.label(if *skipped { format!("Stopped after {sections} section(s); they are prerendered.") } else { format!("{sections} section(s) prerendered.") });
                }
                State::Nothing => {
                    ui.label("Nothing to prerender: every frame of the range is one plain event, which plays from its file.");
                }
                State::Failed(e) => {
                    ui.label(RichText::new(format!("Failed: {e}")).color(crate::ui::theme::get().text_dim));
                }
            }
            ui.add_space(8.0);
            let (ok, label) = match self.state {
                State::Setup => (true, "OK"),
                State::Running { .. } => (false, "OK"),
                _ => (true, "Close"),
            };
            super::button_row(ui, &[(label, ok), ("Cancel", true)])
        });
        match hit {
            Some(0) if matches!(self.state, State::Setup) => {
                self.start(ctx, project, prefs);
                false
            }
            Some(0) => true,
            Some(1) | None if hit.is_some() || close => {
                if let State::Running { job, .. } = &self.state {
                    job.cancel();
                    return false;
                }
                true
            }
            _ => false,
        }
    }
}

/// Clean Up Prerendered Video: all of the project's files, or the inactive ones (an edit changed their frames).
pub struct CleanPrerender {
    all: bool,
    inactive: usize,
    total: usize,
}

impl CleanPrerender {
    pub fn new(project: &Project) -> Self {
        Self { all: false, inactive: prerender::inactive(project).len(), total: prerender::entries().len() }
    }

    /// Some(how many files were deleted) when done; None while open.
    pub fn show(&mut self, ctx: &Context, project: &Project) -> Option<Option<usize>> {
        let (hit, close) = super::modal(ctx, "clean_prerender", "Clean Up Prerendered Video", vec2(440.0, 190.0), |ui| {
            ui.label(format!("This project has {} prerendered file(s) in {}; {} no longer match the project.", self.total, prerender::folder().unwrap_or_default().display(), self.inactive));
            ui.add_space(6.0);
            if widgets::radio(ui, !self.all, "Delete inactive prerendered video files").clicked() {
                self.all = false;
            }
            if widgets::radio(ui, self.all, "Delete all prerendered video files for this project").clicked() {
                self.all = true;
            }
            ui.add_space(8.0);
            super::button_row(ui, &[("OK", self.total > 0), ("Cancel", true)])
        });
        match hit {
            Some(0) => {
                let files: Vec<PathBuf> = if self.all { prerender::entries().into_iter().map(|e| e.file).collect() } else { prerender::inactive(project).into_iter().map(|e| e.file).collect() };
                Some(Some(prerender::remove(&files)))
            }
            Some(_) => Some(None),
            None if close => Some(None),
            None => None,
        }
    }
}
