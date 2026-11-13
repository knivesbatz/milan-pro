//! File › Render As (VP16, light native dialog with a Formats tree) / File › Render (VP23, dark dialog with a
//! category tree and a template grid), §5.15.1 and §5.15.3. Templates are the built-in delivery templates and the
//! user's own (kept in the preferences); Customize Template edits a copy. Render puts the render in the render queue
//! (`render::RenderQueue`, R12), which renders a snapshot of the project through the engine and leaves a log. As in
//! VEGAS the dialog shows the progress and blocks editing, unless Render in the background is on: then it is a
//! window, Render closes it and the status bar shows the progress. Also Tools › Render to New Track.
use std::path::{Path, PathBuf};
use eframe::egui::*;
use crate::project::{Project, format_timecode};
use crate::render::{BUILTIN, JobState, Opts, RenderQueue, Template, content_end};
use crate::settings::Prefs;
use super::template::{Closed, Editor};
use super::super::theme::{self, UiLook};

/// VP23 left tree: (label, depth). Rows 3-10 are `Template::usecase` 0-7.
const TREE23: [(&str, u8); 14] = [
    ("Favorites", 0),
    ("Recent", 0),
    ("Usecase", 0),
    ("YouTube", 1),
    ("Social Media", 1),
    ("Web & Streaming", 1),
    ("Video VFX & Master", 1),
    ("Image Compositing", 1),
    ("DVD & Blu-ray", 1),
    ("Audio Mix & Master", 1),
    ("Podcast & Audio Streaming", 1),
    ("Video", 0),
    ("Image Sequence", 0),
    ("Audio", 0),
];

/// Filters (VP16) / Filter (VP23): what the template list leaves out.
#[derive(Default, Clone, Copy)]
struct Filter {
    favorites: bool,
    match_video: bool,
    match_audio: bool,
}

pub struct RenderDialog {
    pub open: bool,
    /// Options › Quickfade Audio Edits, set by the app: renders apply it as playback does.
    pub quickfade: bool,
    /// The template Render uses: a copy of a listed one, or one changed in Customize Template.
    chosen: Template,
    /// The render this dialog queued (its id in the render queue).
    job: Option<String>,
    /// That render is waiting or rendering (set each frame from the queue).
    busy: bool,
    /// Render in the background (the preference, read each frame): a window, not a modal dialog.
    background: bool,
    /// Tools › Render to New Track: the file goes on a new track at the start of what was rendered.
    to_new_track: bool,
    /// A Render to New Track render just queued: (its id, where to place the file). The app takes it.
    pub new_track: Option<(String, f64)>,
    /// The queue could not take the render.
    error: Option<String>,
    /// "Render loop region only" (used while the loop region is set).
    loop_only: bool,
    /// Render Options: stretch, rotation, markers.
    opts: Opts,
    folder: String,
    name: String,
    /// The project (path, name) Name and Folder were filled from.
    seeded: Option<(Option<PathBuf>, String)>,
    search: String,
    filter: Filter,
    node: usize,
    /// VP16 tree: the format groups expanded.
    expanded: Vec<String>,
    /// VP23 View › Details (a table) instead of Thumbnails.
    details: bool,
    editor: Option<Editor>,
}

impl Default for RenderDialog {
    fn default() -> Self {
        let folder = std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("Videos")).filter(|p| p.is_dir()).unwrap_or_else(std::env::temp_dir);
        Self {
            open: false,
            quickfade: true,
            chosen: BUILTIN[0].clone(),
            job: None,
            busy: false,
            background: false,
            to_new_track: false,
            new_track: None,
            error: None,
            // VEGAS: on whenever there is a loop region; an untick is kept for the session.
            loop_only: true,
            opts: Opts::default(),
            folder: folder.display().to_string(),
            name: String::new(),
            seeded: None,
            search: String::new(),
            filter: Filter::default(),
            node: 2,
            expanded: vec![BUILTIN[0].format()],
            details: false,
            editor: None,
        }
    }
}

impl RenderDialog {
    /// File › Render As.
    pub fn open(&mut self, project: &Project) {
        self.seed(project);
        if self.to_new_track && self.name == new_track_name(project) {
            self.name = project.name.clone();
        }
        (self.open, self.to_new_track, self.job, self.error) = (true, false, None, None);
    }

    /// Tools › Render to New Track.
    pub fn open_new_track(&mut self, project: &Project) {
        self.seed(project);
        if self.name == project.name {
            self.name = new_track_name(project);
        }
        (self.open, self.to_new_track, self.job, self.error) = (true, true, None, None);
    }

    /// Blocks the app (VEGAS's render): open, unless Render in the background is on.
    pub fn is_modal(&self) -> bool {
        self.open && !self.background
    }

    /// True when the preferences changed (a favourite, a saved or deleted template, the recent list).
    pub fn show(&mut self, ctx: &Context, project: &Project, prefs: &mut Prefs, queue: &mut RenderQueue) -> bool {
        self.background = prefs.render_background;
        if !self.open {
            return false;
        }
        self.seed(project);
        let state = self.job.as_ref().and_then(|id| queue.job(id)).map(|j| j.state.clone());
        self.busy = matches!(state, Some(JobState::Queued | JobState::Running));
        if self.to_new_track && state == Some(JobState::Done) {
            // VEGAS closes the dialog once the file is rendered; the app places it.
            (self.job, self.open) = (None, false);
            return false;
        }
        let mut changed = false;
        let v16 = theme::get().look() == UiLook::Vegas16;
        let title = if self.to_new_track { "Render to New Track" } else if v16 { "Render As" } else { "Render" };
        let size = if v16 { vec2(640.0, 720.0) } else { vec2(960.0, 540.0) };
        let bg = self.background;
        let body = |ui: &mut Ui| if v16 { self.body16(ui, project, prefs, queue, &mut changed) } else { self.body23(ui, project, prefs, queue, &mut changed) };
        let (action, close) = if bg { super::window(ctx, "render_as", title, size, body) } else { super::modal(ctx, "render_as", title, size, body) };
        if let Some(ed) = &mut self.editor {
            match ed.show(ctx, &project.settings, &mut prefs.render_templates, &mut changed) {
                Some(Closed::Ok(t)) => (self.chosen, self.editor) = (t, None),
                Some(Closed::Cancel) => self.editor = None,
                None => {}
            }
        } else if let Some(queued) = action {
            changed |= self.start(project, prefs, queue, queued);
        } else if close {
            // VEGAS: closing the progress dialog cancels the render. In the background it carries on.
            if let (Some(id), true, false) = (&self.job, self.busy, self.background) {
                queue.cancel(id);
            }
            (self.job, self.open) = (None, false);
        }
        changed
    }

    /// Open Customize Template on the chosen template.
    pub fn customize(&mut self) {
        self.editor = Some(Editor::new(&self.chosen));
    }

    fn running(&self) -> bool {
        self.busy
    }

    /// The time span to render: the loop region when "Render loop region only" is on and one is set, else the whole timeline.
    fn range(&self, project: &Project) -> (f64, f64) {
        project.timeline.loop_region.filter(|&(a, b)| self.loop_only && b > a).unwrap_or((0.0, content_end(project)))
    }

    /// Name and Folder from `project` the first time the dialog shows it (and again for another project), never while
    /// they are being edited.
    fn seed(&mut self, project: &Project) {
        let id = (project.path.clone(), project.name.clone());
        if self.seeded.as_ref() != Some(&id) {
            self.name = project.name.clone();
            if let Some(dir) = project.path.as_ref().and_then(|p| p.parent()) {
                self.folder = dir.display().to_string();
            }
            self.seeded = Some(id);
        }
    }

    /// The output file; for an image sequence its first frame, `<name>_000000.<ext>` (VEGAS's numbering).
    fn out_path(&self) -> PathBuf {
        let ext = self.chosen.ext();
        let file = if self.chosen.sequence() { format!("{}_000000.{ext}", self.name) } else { format!("{}.{ext}", self.name) };
        Path::new(&self.folder).join(file)
    }

    /// Why Name and Folder as typed cannot be rendered to, if they cannot. A ':' would write into an NTFS stream of
    /// another file; a relative folder would land in Milan's working directory.
    fn out_error(&self) -> Option<&'static str> {
        if self.name.trim().is_empty() {
            return Some("Type a name for the file.");
        }
        if self.name.contains(['<', '>', ':', '"', '/', '\\', '|', '?', '*']) || self.name.chars().any(char::is_control) {
            return Some("A file name cannot contain < > : \" / \\ | ? *");
        }
        let dir = Path::new(&self.folder);
        if !dir.is_absolute() || !dir.is_dir() {
            return Some("Choose a folder that exists, as a full path (C:\\...).");
        }
        None
    }

    /// The templates the list shows, built-in first, after the search and the filters.
    fn listed<'a>(&self, project: &Project, prefs: &'a Prefs) -> Vec<&'a Template> {
        let q = self.search.to_lowercase();
        let s = &project.settings;
        BUILTIN
            .iter()
            .chain(&prefs.render_templates)
            .filter(|t| q.is_empty() || t.name.to_lowercase().contains(&q) || t.format().to_lowercase().contains(&q) || t.notes.to_lowercase().contains(&q))
            .filter(|t| !self.filter.favorites || prefs.render_favorites.contains(&t.name))
            .filter(|t| !self.filter.match_video || t.matches_video(s))
            .filter(|t| !self.filter.match_audio || t.matches_audio(s))
            .collect()
    }

    fn output_fields(&mut self, ui: &mut Ui) {
        let ext = self.chosen.ext();
        let seq = self.chosen.sequence();
        super::row(ui, "Folder:", 50.0, !self.running(), |ui| {
            ui.add(TextEdit::singleline(&mut self.folder).desired_width(ui.available_width() - 90.0));
            if ui.button("Browse...").clicked() {
                if let Some(p) = rfd::FileDialog::new().set_directory(&self.folder).pick_folder() {
                    self.folder = p.display().to_string();
                }
            }
        });
        super::row(ui, "Name:", 50.0, !self.running(), |ui| {
            ui.add(TextEdit::singleline(&mut self.name).desired_width(ui.available_width() - 90.0));
            ui.label(if seq { format!("_000000.{ext}") } else { format!(".{ext}") });
        });
    }

    fn details(&self, ui: &mut Ui, project: &Project) {
        let t = &self.chosen;
        let s = &project.settings;
        ui.label(RichText::new(&t.name).strong());
        ui.label(format!("Notes:  {}", t.notes));
        if let Some(v) = t.video_line(s, self.opts.rotate) {
            ui.label(format!("Video:  {v}"));
        }
        if let Some(a) = t.audio_line(s) {
            ui.label(format!("Audio:  {a}"));
        }
        let (a, b) = self.range(project);
        let what = if (a, b) == (0.0, content_end(project)) { "whole timeline" } else { "loop region" };
        ui.label(format!("Length: {} ({what})", format_timecode(b - a, s.frame_rate)));
    }

    /// Render Options (VP16 group; VP23's gear menu): stretch, rotation, markers; multichannel waits for R10a; R12's
    /// Render in the background (off, as VEGAS).
    fn options(&mut self, ui: &mut Ui, project: &Project, prefs: &mut Prefs, changed: &mut bool) {
        let video = self.chosen.video.is_some();
        ui.add_enabled(video, Checkbox::new(&mut self.opts.stretch, "Stretch video to fill output frame size (do not letterbox)"));
        ui.add_enabled(video && project.settings.rotation != 0, Checkbox::new(&mut self.opts.rotate, "Use project output rotation setting"))
            .on_disabled_hover_text("The project's Output rotation is 0°.");
        let chapters = self.chosen.container().is_some_and(|c| c.chapters);
        ui.add_enabled(chapters, Checkbox::new(&mut self.opts.markers, "Save project markers in media file")).on_disabled_hover_text("This format cannot hold markers.");
        ui.add_enabled(false, Checkbox::new(&mut false, "Enable multichannel mapping")).on_disabled_hover_text(super::super::menus::Later("R10a").text());
        *changed |= ui
            .checkbox(&mut prefs.render_background, "Render in the background (keep editing; progress in the status bar)")
            .on_hover_text("Renders a copy of the project as it is when Render is clicked: later edits do not change the file.")
            .changed();
    }

    /// Progress / result block and the bottom buttons. Some(false) when Render was clicked, Some(true) for Add to
    /// Render Queue.
    fn footer(&mut self, ui: &mut Ui, project: &Project, queue: &mut RenderQueue, render_label: &str) -> Option<bool> {
        if let Some(j) = self.job.as_ref().and_then(|id| queue.job(id)).cloned() {
            match &j.state {
                JobState::Running => {
                    if let Some(r) = queue.running(&j.id) {
                        ui.add(ProgressBar::new(r.fraction()).show_percentage());
                        ui.label(r.eta().map_or("Estimating time remaining...".to_string(), |e| format!("{e:.0} s remaining")));
                    }
                }
                JobState::Queued => {
                    ui.label(format!("Waiting in the render queue: {} render(s) before this one.", queue.ahead(&j.id)));
                }
                JobState::Paused => {
                    ui.label("Paused in the render queue.");
                }
                st => {
                    ui.horizontal(|ui| {
                        match st {
                            JobState::Done => {
                                ui.label(format!("Rendered {}", j.out.display()));
                                if ui.button("Open Folder").clicked() {
                                    open_folder(&j.out);
                                }
                            }
                            JobState::Failed(_) => {
                                ui.colored_label(theme::get().warning_text, "Render failed:");
                            }
                            _ => {
                                ui.label("Render cancelled.");
                            }
                        }
                        if let Some(log) = &j.log {
                            if ui.link("Show log").on_hover_text(log.display().to_string()).clicked() {
                                open_file(log);
                            }
                        }
                    });
                    if let JobState::Failed(e) = st {
                        ScrollArea::vertical().max_height(80.0).show(ui, |ui| ui.monospace(e));
                    }
                }
            }
        }
        let (a, b) = self.range(project);
        let empty = b <= a;
        let has_loop = project.timeline.loop_region.is_some_and(|(a, b)| b > a);
        let running = self.running();
        let bad = self.out_error().map(String::from).or_else(|| self.chosen.check().err());
        let bad = bad.or_else(|| (self.to_new_track && self.chosen.sequence()).then(|| "Render to New Track needs a video or audio file, not an image sequence.".to_string()));
        if let (Some(e), false) = (bad.as_ref().or(self.error.as_ref()), running) {
            ui.colored_label(theme::get().warning_text, e);
        }
        let mut action = None;
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if super::button(ui, "Cancel", true, false).clicked() {
                match (&self.job, running) {
                    (Some(id), true) => queue.cancel(id),
                    _ => (self.open, self.job) = (false, None),
                }
            }
            let ok = !empty && !running && bad.is_none();
            if super::button(ui, render_label, ok, true).clicked() {
                action = Some(false);
            }
            // R12: rendered while editing goes on (the dialog closes); the Render Queue window lists it.
            if !self.to_new_track && ui.add_enabled(ok, Button::new("Add to Render Queue").min_size(super::BUTTON)).on_hover_text("Render a copy of the project as it is now, in the background").clicked() {
                action = Some(true);
            }
            let mut on = self.loop_only && has_loop;
            if ui.add_enabled(has_loop && !running, Checkbox::new(&mut on, "Render loop region only")).changed() {
                self.loop_only = on;
            }
        });
        action
    }

    fn customize_button(&mut self, ui: &mut Ui, label: &str, size: Vec2) {
        if ui.add_enabled(!self.running(), Button::new(label).min_size(size)).clicked() {
            self.customize();
        }
    }

    fn body16(&mut self, ui: &mut Ui, project: &Project, prefs: &mut Prefs, queue: &mut RenderQueue, changed: &mut bool) -> Option<bool> {
        ui.label("Output File");
        self.output_fields(ui);
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add(TextEdit::singleline(&mut self.search).hint_text("Search render templates").desired_width(300.0));
            ui.menu_button("Filters ⏷", |ui| {
                ui.checkbox(&mut self.filter.favorites, "Show favorites only");
                let mut m = self.filter.match_video && self.filter.match_audio;
                if ui.checkbox(&mut m, "Match project settings").changed() {
                    (self.filter.match_video, self.filter.match_audio) = (m, m);
                }
            });
        });
        let d = theme::get().dialog();
        let listed = self.listed(project, prefs);
        let searching = !self.search.is_empty() || self.filter.favorites || self.filter.match_video || self.filter.match_audio;
        let mut groups: Vec<String> = Vec::new();
        for t in &listed {
            if !groups.contains(&t.format()) {
                groups.push(t.format());
            }
        }
        let mut star_hit = None;
        Frame::NONE.fill(d.field).stroke(Stroke::new(1.0_f32, d.field_border)).inner_margin(4).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ScrollArea::vertical().id_salt("render_tree").max_height(220.0).auto_shrink(false).show(ui, |ui| {
                ui.label(RichText::new("Formats").strong());
                for g in &groups {
                    let open = searching || self.expanded.contains(g);
                    if ui.selectable_label(false, format!("{} {g}", if open { "⏷" } else { "⏵" })).clicked() {
                        match self.expanded.iter().position(|x| x == g) {
                            Some(k) => {
                                self.expanded.remove(k);
                            }
                            None => self.expanded.push(g.clone()),
                        }
                    }
                    if open {
                        for t in listed.iter().filter(|t| &t.format() == g) {
                            ui.horizontal(|ui| {
                                ui.add_space(24.0);
                                if star(ui, prefs.render_favorites.contains(&t.name)) {
                                    star_hit = Some(t.name.clone());
                                }
                                if ui.selectable_label(self.chosen == **t, &t.name).clicked() {
                                    self.chosen = (*t).clone();
                                }
                            });
                        }
                    }
                }
                if groups.is_empty() {
                    ui.label("No templates match.");
                }
            });
        });
        if let Some(n) = star_hit {
            toggle_favorite(prefs, n);
            *changed = true;
        }
        ui.label("Description:");
        Frame::NONE.fill(d.field).stroke(Stroke::new(1.0_f32, d.field_border)).inner_margin(6).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(90.0);
            self.details(ui, project);
        });
        self.customize_button(ui, "Customize Template...", vec2(0.0, 0.0));
        super::group(ui, "Render Options");
        ui.add_enabled_ui(!self.running(), |ui| self.options(ui, project, prefs, changed));
        ui.add_space(4.0);
        self.footer(ui, project, queue, "Render")
    }

    fn body23(&mut self, ui: &mut Ui, project: &Project, prefs: &mut Prefs, queue: &mut RenderQueue, changed: &mut bool) -> Option<bool> {
        let t = theme::get();
        let d = t.dialog();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Render:").strong());
            ui.label(&project.name);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.menu_button("View ⏷", |ui| {
                    for (details, label) in [(false, "Thumbnails"), (true, "Details")] {
                        if ui.radio(self.details == details, label).clicked() {
                            self.details = details;
                            ui.close_menu();
                        }
                    }
                });
                ui.menu_button("Filter ⏷", |ui| {
                    ui.checkbox(&mut self.filter.match_video, "Match project video settings");
                    ui.checkbox(&mut self.filter.match_audio, "Match project audio settings");
                    if ui.button("Show all formats").clicked() {
                        self.filter = Filter::default();
                        ui.close_menu();
                    }
                });
                ui.menu_button("⚙ Render options", |ui| ui.add_enabled_ui(!self.running(), |ui| self.options(ui, project, prefs, changed)));
            });
        });
        let mut render = None;
        let listed = self.listed(project, prefs);
        let shown: Vec<&Template> = listed
            .into_iter()
            .filter(|tp| match self.node {
                0 => prefs.render_favorites.contains(&tp.name),
                1 => prefs.render_recent.contains(&tp.name),
                2 => true,
                3..=10 => tp.usecase() == self.node - 3,
                11 => tp.video.is_some() && !tp.sequence(),
                12 => tp.sequence(),
                _ => tp.video.is_none(),
            })
            .collect();
        let mut star_hit = None;
        ui.horizontal_top(|ui| {
            // Category tree.
            Frame::NONE.fill(d.field).inner_margin(4).show(ui, |ui| {
                ui.set_width(190.0);
                ui.set_height(380.0);
                // Frames inherit the row's horizontal layout; the tree is a column.
                ui.vertical(|ui| self.tree23(ui));
            });
            ui.vertical(|ui| {
                Frame::NONE.fill(d.field).inner_margin(6).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.set_height(170.0);
                    ScrollArea::vertical().id_salt("render_grid_scroll").auto_shrink(false).show(ui, |ui| {
                        if self.details {
                            Grid::new("render_details").num_columns(4).striped(true).show(ui, |ui| {
                                for h in ["", "Name", "Format", "Video"] {
                                    ui.label(RichText::new(h).strong());
                                }
                                ui.end_row();
                                for tp in &shown {
                                    if star(ui, prefs.render_favorites.contains(&tp.name)) {
                                        star_hit = Some(tp.name.clone());
                                    }
                                    if ui.selectable_label(self.chosen == **tp, &tp.name).clicked() {
                                        self.chosen = (*tp).clone();
                                    }
                                    ui.label(tp.format());
                                    ui.label(tp.video_line(&project.settings, self.opts.rotate).unwrap_or_default());
                                    ui.end_row();
                                }
                            });
                        } else {
                            Grid::new("render_grid").num_columns(4).spacing(vec2(10.0, 10.0)).show(ui, |ui| {
                                for (n, tp) in shown.iter().enumerate() {
                                    let (r, hit) = ui.allocate_exact_size(vec2(170.0, 56.0), Sense::click());
                                    let p = ui.painter();
                                    if self.chosen == **tp {
                                        p.rect_filled(r, t.m.widget_radius, t.preset_tile_selected);
                                    } else if hit.hovered() {
                                        p.rect_filled(r, t.m.widget_radius, t.button_hover_bg);
                                    }
                                    let icon = Rect::from_min_size(r.min + vec2(4.0, 8.0), vec2(34.0, 26.0));
                                    p.rect_stroke(icon, 2.0, Stroke::new(1.5_f32, d.text), StrokeKind::Inside);
                                    if tp.video.is_some() {
                                        let c = icon.center();
                                        p.add(Shape::convex_polygon(vec![c + vec2(-4.0, -6.0), c + vec2(6.0, 0.0), c + vec2(-4.0, 6.0)], d.text, Stroke::NONE));
                                    } else {
                                        p.circle_filled(icon.center(), 6.0, d.text);
                                    }
                                    let ink = if self.chosen == **tp { t.text_bright } else { t.text_dim };
                                    let w = r.max.x - icon.max.x - 10.0;
                                    let title = p.layout_no_wrap(tp.name.clone(), t.bold(12.0), d.text);
                                    p.with_clip_rect(r).galley(pos2(icon.max.x + 6.0, r.min.y + 6.0), title, d.text);
                                    let desc = p.layout(tp.notes.clone(), t.font(11.0), ink, w);
                                    p.with_clip_rect(r).galley(pos2(icon.max.x + 6.0, r.min.y + 22.0), desc, ink);
                                    if hit.clicked() {
                                        self.chosen = (*tp).clone();
                                    }
                                    if hit.secondary_clicked() {
                                        star_hit = Some(tp.name.clone());
                                    }
                                    hit.on_hover_text(format!("{}\nRight-click: add to or remove from Favorites", tp.format()));
                                    if n % 4 == 3 {
                                        ui.end_row();
                                    }
                                }
                            });
                        }
                        if shown.is_empty() {
                            ui.label(RichText::new("No templates here.").color(t.text_dim));
                        }
                    });
                });
                ui.horizontal_top(|ui| {
                    Frame::NONE.fill(d.field).inner_margin(6).show(ui, |ui| {
                        ui.set_width(ui.available_width() - 110.0);
                        ui.set_height(100.0);
                        ui.vertical(|ui| self.details(ui, project));
                    });
                    self.customize_button(ui, "Customize...", vec2(100.0, 30.0));
                });
                Frame::NONE.fill(d.field).inner_margin(6).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    self.output_fields(ui);
                });
                render = self.footer(ui, project, queue, "Render...");
            });
        });
        if let Some(n) = star_hit {
            toggle_favorite(prefs, n);
            *changed = true;
        }
        render
    }

    fn tree23(&mut self, ui: &mut Ui) {
        ui.add(TextEdit::singleline(&mut self.search).hint_text("Search").desired_width(180.0));
        for (k, (label, depth)) in TREE23.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.add_space(*depth as f32 * 16.0);
                let text = if k == 0 { format!("★ {label}") } else { label.to_string() };
                if ui.selectable_label(self.node == k, text).clicked() {
                    self.node = k;
                }
            });
        }
    }

    /// Queue the render (after asking before replacing a file); `queued` (Add to Render Queue) closes the dialog, as
    /// Render does in the background. True when the recent list changed.
    fn start(&mut self, project: &Project, prefs: &mut Prefs, queue: &mut RenderQueue, queued: bool) -> bool {
        let out = self.out_path();
        if out.exists()
            && rfd::MessageDialog::new()
                .set_title("Render")
                .set_description(format!("{} already exists. Replace it?", out.display()))
                .set_buttons(rfd::MessageButtons::YesNo)
                .show()
                != rfd::MessageDialogResult::Yes
        {
            return false;
        }
        let opts = Opts { quickfade: self.quickfade, ..self.opts };
        let range = self.range(project);
        match queue.add(project, &self.chosen, out, range, opts) {
            Ok(id) => {
                if self.to_new_track {
                    self.new_track = Some((id.clone(), range.0));
                }
                (self.job, self.error) = (Some(id), None);
                if queued || self.background {
                    (self.job, self.open) = (None, false);
                }
            }
            Err(e) => {
                self.error = Some(e);
                return false;
            }
        }
        let name = self.chosen.name.clone();
        prefs.render_recent.retain(|r| *r != name);
        prefs.render_recent.insert(0, name);
        prefs.render_recent.truncate(10);
        true
    }
}

/// Render to New Track's file name: "<project> (rendered)".
fn new_track_name(project: &Project) -> String {
    format!("{} (rendered)", project.name)
}

fn toggle_favorite(prefs: &mut Prefs, name: String) {
    match prefs.render_favorites.iter().position(|f| *f == name) {
        Some(k) => {
            prefs.render_favorites.remove(k);
        }
        None => prefs.render_favorites.push(name),
    }
}

/// Favourite ☆ that turns gold when set. True when clicked.
fn star(ui: &mut Ui, on: bool) -> bool {
    let t = theme::get();
    let color = if on { t.snap.cursor } else { t.text_dim };
    ui.add(Label::new(RichText::new(if on { "★" } else { "☆" }).color(color)).sense(Sense::click())).clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_and_folder_are_checked_before_rendering() {
        let mut d = RenderDialog { folder: std::env::temp_dir().display().to_string(), name: "Episode 1 - Pilot".into(), ..Default::default() };
        assert_eq!(d.out_error(), None);
        for bad in ["Episode 1: Pilot", "a/b", "what?", "  ", ""] {
            d.name = bad.into();
            assert!(d.out_error().is_some(), "{bad:?}");
        }
        d.name = "ok".into();
        for bad in ["", "renders", r"C:\no\such\folder\milan"] {
            d.folder = bad.into();
            assert!(d.out_error().is_some(), "{bad:?}");
        }
    }

    #[test]
    fn name_and_folder_follow_the_project_not_the_typing() {
        let mut a = Project::new("a");
        a.path = Some(r"C:\A\a.mnle".into());
        let mut d = RenderDialog::default();
        d.seed(&a);
        assert_eq!((d.name.as_str(), d.folder.as_str()), ("a", r"C:\A"));
        // Clearing Name to type a new one keeps it empty and keeps a browsed folder.
        (d.name, d.folder) = (String::new(), r"D:\Exports".into());
        d.seed(&a);
        assert_eq!((d.name.as_str(), d.folder.as_str()), ("", r"D:\Exports"));
        // Another project opened: its own name and folder.
        let mut b = Project::new("b");
        b.path = Some(r"D:\B\b.mnle".into());
        d.seed(&b);
        assert_eq!((d.name.as_str(), d.folder.as_str()), ("b", r"D:\B"));
    }

    /// Search, favourites and Match project settings narrow the list; a sequence is named by its first frame.
    #[test]
    fn the_list_follows_search_and_filters() {
        let p = Project::new("p");
        let mut prefs = Prefs { render_favorites: vec!["Web 720p".into()], ..Default::default() };
        prefs.render_templates.push(Template { name: "My 4K".into(), ..BUILTIN[6].clone() });
        let mut d = RenderDialog { search: "dnxhr".into(), ..Default::default() };
        assert!(d.listed(&p, &prefs).iter().all(|t| t.format().contains("DNxHR") || t.name.contains("DNxHR")) && d.listed(&p, &prefs).len() == 6);
        d.search = "4k".into();
        assert_eq!(d.listed(&p, &prefs).iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["YouTube 2160p", "My 4K"]);
        d.search.clear();
        d.filter.favorites = true;
        assert_eq!(d.listed(&p, &prefs).len(), 1);
        d.filter = Filter { match_video: true, ..Default::default() };
        let names: Vec<&str> = d.listed(&p, &prefs).iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"YouTube 1080p") && !names.contains(&"Web 720p") && !names.contains(&"My 4K"), "{names:?}");
        d.chosen = BUILTIN.iter().find(|t| t.sequence()).unwrap().clone();
        (d.folder, d.name) = (r"C:\v".into(), "Shot".into());
        assert_eq!(d.out_path(), Path::new(r"C:\v\Shot_000000.png"));
    }
}

/// Show `path` selected in Explorer.
pub fn open_folder(path: &Path) {
    let mut cmd = std::process::Command::new("explorer");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // explorer parses its own command line; /select needs the path quoted after the comma.
        cmd.raw_arg(format!("/select,\"{}\"", path.display())).creation_flags(0x08000000);
    }
    #[cfg(not(windows))]
    cmd.arg(path.parent().unwrap_or(path));
    let _ = cmd.spawn();
}

/// Open `path` in its default app (a render log in the text editor).
pub fn open_file(path: &Path) {
    let mut cmd = std::process::Command::new("explorer");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.raw_arg(format!("\"{}\"", path.display())).creation_flags(0x08000000);
    }
    #[cfg(not(windows))]
    cmd.arg(path);
    let _ = cmd.spawn();
}
