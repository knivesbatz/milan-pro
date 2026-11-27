//! File › Render (the VEGAS 23 dialog: a category tree and a template grid, VEGAS_UI_SPEC.md §5.15.3). Templates are
//! the built-in delivery templates and the user's own (kept in the preferences); Customize Template edits a copy. Render puts the render in the render queue
//! (`render::RenderQueue`, R12), which renders a snapshot of the project through the engine and leaves a log. As in
//! VEGAS the dialog shows the progress and blocks editing, unless Render in the background is on: then it is a
//! window, Render closes it and the status bar shows the progress. Also Tools › Render to New Track.
use std::path::{Path, PathBuf};
use eframe::egui::*;
use crate::ui::widgets;
use crate::project::{Project, format_timecode};
use crate::render::{BUILTIN, JobState, Opts, RenderQueue, Template, content_end};
use crate::settings::Prefs;
use super::template::{Closed, Editor};
use super::super::theme;

/// The left tree: (label, depth). Rows 3-10 are `Template::usecase` 0-7.
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

/// Filter: what the template list leaves out.
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
    /// View › Details (a table) instead of Thumbnails.
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
        let title = if self.to_new_track { "Render to New Track" } else { "Render" };
        let size = vec2(960.0, 540.0);
        let bg = self.background;
        let body = |ui: &mut Ui| self.body(ui, project, prefs, queue, &mut changed);
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
            widgets::edit(ui, TextEdit::singleline(&mut self.folder).desired_width(ui.available_width() - 90.0));
            if widgets::push_button(ui, "Browse...").clicked() {
                if let Some(p) = rfd::FileDialog::new().set_directory(&self.folder).pick_folder() {
                    self.folder = p.display().to_string();
                }
            }
        });
        super::row(ui, "Name:", 50.0, !self.running(), |ui| {
            widgets::edit(ui, TextEdit::singleline(&mut self.name).desired_width(ui.available_width() - 90.0));
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
        // R22: a hardware template whose encoder is not available here says it renders in software.
        if let (_, Some(note)) = t.resolved() {
            ui.label(RichText::new(note).color(theme::get().text_dim));
        }
        if let Some(a) = t.audio_line(s) {
            ui.label(format!("Audio:  {a}"));
        }
        let (a, b) = self.range(project);
        let what = if (a, b) == (0.0, content_end(project)) { "whole timeline" } else { "loop region" };
        ui.label(format!("Length: {} ({what})", format_timecode(b - a, s.frame_rate)));
    }

    /// Render Options (the gear menu): stretch, rotation, markers, the loudness log (R16c); multichannel waits for R10a; R12's
    /// Render in the background (off, as VEGAS).
    fn options(&mut self, ui: &mut Ui, project: &Project, prefs: &mut Prefs, changed: &mut bool) {
        let video = self.chosen.video.is_some();
        widgets::checkbox_enabled(ui, video, &mut self.opts.stretch, "Stretch video to fill output frame size (do not letterbox)");
        widgets::checkbox_enabled(ui, video && project.settings.rotation != 0, &mut self.opts.rotate, "Use project output rotation setting")
            .on_disabled_hover_text("The project's Output rotation is 0°.");
        let chapters = self.chosen.container().is_some_and(|c| c.chapters);
        widgets::checkbox_enabled(ui, chapters, &mut self.opts.markers, "Save project markers in media file").on_disabled_hover_text("This format cannot hold markers.");
        // R16c: `<name>_loud.txt` beside the file, measured after the master and before the codec (VEGAS).
        let mut loud = self.opts.loudness_log.is_some();
        if widgets::checkbox_enabled(ui, self.chosen.audio.is_some(), &mut loud, "Save loudness log next to media file").on_disabled_hover_text("This format has no audio.").changed() {
            self.opts.loudness_log = loud.then_some(prefs.loudness.mode);
        }
        // R10a: a stereo project's busses onto the pairs of a surround template (the master on 1-2, Bus A on 3-4, ...).
        let stems = project.settings.layout == crate::project::BusMode::Stereo && !project.timeline.audio.buses.is_empty();
        let wide = self.chosen.audio.as_ref().is_some_and(|a| a.channels > 2);
        widgets::checkbox_enabled(ui, stems && wide, &mut self.opts.multichannel, "Enable multichannel mapping")
            .on_hover_text("The master on channels 1 and 2, then each bus that goes to the master on the next pair (Bus A on 3 and 4, Bus B on 5 and 6, ...), as far as the format has channels.")
            .on_disabled_hover_text("Needs a stereo project with busses and a 5.1 or 7.1 template; a surround project renders its channels as they are.");
        // R39: subtitles from a track's Titles & Text events: burned in (they are video), or also as a file or a stream.
        let tracks = crate::interchange::subtitles::text_tracks(project);
        if !tracks.is_empty() && self.chosen.video.is_some() {
            if !tracks.iter().any(|t| t.0 == self.opts.subs_track as usize) {
                self.opts.subs_track = tracks[0].0 as u16;
            }
            let embeddable = crate::interchange::subtitles::embed_codec(self.chosen.ext()).is_some();
            if self.opts.subs == crate::render::SubsOut::Embedded && !embeddable {
                self.opts.subs = crate::render::SubsOut::BurnedIn;
            }
            ui.horizontal(|ui| {
                ui.label("Subtitles:");
                let cur = self.opts.subs;
                widgets::combo(ui, "render_subs", 250.0, cur.name(), |ui| {
                    for s in crate::render::SubsOut::ALL {
                        let ok = s != crate::render::SubsOut::Embedded || embeddable;
                        ui.add_enabled_ui(ok, |ui| ui.selectable_value(&mut self.opts.subs, s, s.name()))
                            .response
                            .on_disabled_hover_text("MP4, MOV, MKV and WebM hold a subtitle stream; this format does not.");
                    }
                });
                let name = |k: u16| tracks.iter().find(|t| t.0 == k as usize).map_or(String::new(), |t| t.1.clone());
                widgets::combo(ui, "render_subs_track", 170.0, name(self.opts.subs_track), |ui| {
                    for (i, n) in &tracks {
                        ui.selectable_value(&mut self.opts.subs_track, *i as u16, n);
                    }
                })
                .response
                .on_hover_text("The track whose Titles & Text events are the subtitles");
            });
            widgets::checkbox_enabled(ui, self.opts.subs != crate::render::SubsOut::BurnedIn, &mut self.opts.subs_burn, "Burn the subtitles into the picture")
                .on_hover_text("Off: the subtitle track is left out of the picture, so they are only in the file or stream");
        }
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
                        widgets::progress(ui, r.fraction(), None, None);
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
                                if widgets::push_button(ui, "Open Folder").clicked() {
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
            if !self.to_new_track && super::sized_button(ui, "Add to Render Queue", super::BUTTON, ok).on_hover_text("Render a copy of the project as it is now, in the background").clicked() {
                action = Some(true);
            }
            let mut on = self.loop_only && has_loop;
            if widgets::checkbox_enabled(ui, has_loop && !running, &mut on, "Render loop region only").changed() {
                self.loop_only = on;
            }
        });
        action
    }

    fn customize_button(&mut self, ui: &mut Ui, label: &str, size: Vec2) {
        if super::sized_button(ui, label, size, !self.running()).clicked() {
            self.customize();
        }
    }

    fn body(&mut self, ui: &mut Ui, project: &Project, prefs: &mut Prefs, queue: &mut RenderQueue, changed: &mut bool) -> Option<bool> {
        let t = theme::get();
        let d = t.dialog();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Render:").strong());
            ui.label(&project.name);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.menu_button("View ⏷", |ui| {
                    for (details, label) in [(false, "Thumbnails"), (true, "Details")] {
                        if widgets::radio(ui, self.details == details, label).clicked() {
                            self.details = details;
                            ui.close_menu();
                        }
                    }
                });
                ui.menu_button("Filter ⏷", |ui| {
                    widgets::checkbox(ui, &mut self.filter.match_video, "Match project video settings");
                    widgets::checkbox(ui, &mut self.filter.match_audio, "Match project audio settings");
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
            white_box(ui, 4, |ui| {
                ui.set_width(190.0);
                ui.set_height(380.0);
                // Frames inherit the row's horizontal layout; the tree is a column.
                ui.vertical(|ui| self.tree(ui));
            });
            ui.vertical(|ui| {
                white_box(ui, 6, |ui| {
                    ui.set_width(ui.available_width());
                    ui.set_height(170.0);
                    ScrollArea::vertical().id_salt("render_grid_scroll").auto_shrink(false).show(ui, |ui| {
                        if self.details {
                            Grid::new("render_details").num_columns(4).striped(false).show(ui, |ui| {
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
                                    // A raised bevelled tile, pressed in (blue tint) when chosen.
                                    let state = if self.chosen == **tp { widgets::Chrome::Active } else if hit.hovered() { widgets::Chrome::Hover } else { widgets::Chrome::Raised };
                                    widgets::chrome_button(p, r, state, &t.dialog_metal());
                                    let icon = Rect::from_min_size(r.min + vec2(4.0, 8.0), vec2(34.0, 26.0));
                                    p.rect_stroke(icon, 0.0, Stroke::new(1.0_f32, d.text), StrokeKind::Inside);
                                    if tp.video.is_some() {
                                        let c = icon.center();
                                        p.add(Shape::convex_polygon(vec![c + vec2(-4.0, -6.0), c + vec2(6.0, 0.0), c + vec2(-4.0, 6.0)], d.text, Stroke::NONE));
                                    } else {
                                        p.circle_filled(icon.center(), 6.0, d.text);
                                    }
                                    let ink = t.text;
                                    let w = r.max.x - icon.max.x - 10.0;
                                    // R48: a name longer than the tile ends in "…" (the hover has it whole).
                                    let mut job = text::LayoutJob::simple_singleline(tp.name.clone(), t.bold(11.0), d.text);
                                    job.wrap = text::TextWrapping { max_width: w, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
                                    let title = p.layout_job(job);
                                    p.with_clip_rect(r).galley(pos2(icon.max.x + 6.0, r.min.y + 6.0), title, d.text);
                                    let desc = p.layout(tp.notes.clone(), t.font(11.0), ink, w);
                                    p.with_clip_rect(r).galley(pos2(icon.max.x + 6.0, r.min.y + 22.0), desc, ink);
                                    if hit.clicked() {
                                        self.chosen = (*tp).clone();
                                    }
                                    if hit.secondary_clicked() {
                                        star_hit = Some(tp.name.clone());
                                    }
                                    hit.on_hover_text(format!("{}\n{}\nRight-click: add to or remove from Favorites", tp.name, tp.format()));
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
                    white_box(ui, 6, |ui| {
                        ui.set_width(ui.available_width() - 110.0);
                        ui.set_height(100.0);
                        ui.vertical(|ui| self.details(ui, project));
                    });
                    self.customize_button(ui, "Customize...", vec2(100.0, 30.0));
                });
                white_box(ui, 6, |ui| {
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

    fn tree(&mut self, ui: &mut Ui) {
        widgets::edit(ui, TextEdit::singleline(&mut self.search).hint_text("Search").desired_width(180.0));
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
        // The loudness log is written in the Loudness Meters' mode at the time of the render.
        let loudness_log = self.opts.loudness_log.filter(|_| self.chosen.audio.is_some()).map(|_| prefs.loudness.mode);
        let opts = Opts { quickfade: self.quickfade, loudness_log, ..self.opts };
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
/// A white box of the Render dialog: a sunken 3D field box.
fn white_box(ui: &mut Ui, margin: i8, add: impl FnOnce(&mut Ui)) {
    super::field_box(ui, margin, add);
}

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
        assert!(d.listed(&p, &prefs).iter().all(|t| t.format().contains("DNxHR") || t.name.contains("DNxHR")) && d.listed(&p, &prefs).len() == 7, "the DNxHR templates and MXF DNxHD 145 1080 (the same encoder)");
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
