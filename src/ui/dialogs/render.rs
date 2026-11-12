//! File › Render As (VP16, light native dialog with a Formats tree) / File › Render (VP23, dark dialog with a
//! category tree and a template grid), §5.15.1 and §5.15.3. Templates are Milan Pro's ffmpeg presets; the render runs
//! through the engine (`engine::render`).
use std::path::{Path, PathBuf};
use eframe::egui::*;
use crate::project::{Project, format_timecode};
use crate::render::{Job, PRESETS, Status, content_end};
use super::super::menus::Later;
use super::super::theme::{self, UiLook};

/// Format group and description per preset (same order as `PRESETS`).
const INFO: [(&str, &str); 5] = [
    ("MP4 (H.264 / AAC)", "Use this setting to create a high-quality file at the project size, suited to uploading and playback."),
    ("MP4 (HEVC / AAC)", "High-efficiency video at the project size: smaller files for the same quality."),
    ("QuickTime (ProRes 422)", "Intermediate-quality master for further editing and grading."),
    ("Wave (PCM)", "Uncompressed 16-bit stereo audio mix."),
    ("MP3 Audio", "Compressed 320 kbps stereo audio mix."),
];
/// VP23 left tree: (label, depth, filter). Filter: None = every template.
const TREE23: [(&str, u8, Option<bool>); 14] = [
    ("Favorites", 0, None),
    ("Recent", 0, None),
    ("Usecase", 0, None),
    ("YouTube", 1, Some(true)),
    ("Social Media", 1, Some(true)),
    ("Web & Streaming", 1, Some(true)),
    ("Video VFX & Master", 1, Some(true)),
    ("Image Compositing", 1, Some(true)),
    ("DVD & Blu-ray", 1, Some(true)),
    ("Audio Mix & Master", 1, Some(false)),
    ("Podcast & Audio Streaming", 1, Some(false)),
    ("Video", 0, Some(true)),
    ("Image Sequence", 0, Some(true)),
    ("Audio", 0, Some(false)),
];

pub struct RenderDialog {
    pub open: bool,
    preset: usize,
    job: Option<Job>,
    /// "Render loop region only" (used while the loop region is set).
    loop_only: bool,
    folder: String,
    name: String,
    /// The project (path, name) Name and Folder were filled from.
    seeded: Option<(Option<PathBuf>, String)>,
    search: String,
    favorites: [bool; 5],
    node: usize,
    /// VP16 tree: which format groups are expanded.
    expanded: [bool; 5],
}

impl Default for RenderDialog {
    fn default() -> Self {
        let folder = std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("Videos")).filter(|p| p.is_dir()).unwrap_or_else(std::env::temp_dir);
        Self {
            open: false,
            preset: 0,
            job: None,
            // VEGAS: on whenever there is a loop region; an untick is kept for the session.
            loop_only: true,
            folder: folder.display().to_string(),
            name: String::new(),
            seeded: None,
            search: String::new(),
            favorites: [false; 5],
            node: 2,
            expanded: [true, false, false, false, false],
        }
    }
}

impl RenderDialog {
    pub fn show(&mut self, ctx: &Context, project: &Project) {
        if !self.open {
            return;
        }
        self.seed(project);
        let v16 = theme::get().look() == UiLook::Vegas16;
        let (title, size) = if v16 { ("Render As", vec2(640.0, 640.0)) } else { ("Render", vec2(960.0, 470.0)) };
        let (render, close) = super::modal(ctx, "render_as", title, size, |ui| if v16 { self.body16(ui, project) } else { self.body23(ui, project) });
        if render {
            self.start(ctx, project);
        }
        if close {
            // Closing the window abandons the render rather than leaving an invisible ffmpeg running.
            if let Some(j) = &self.job {
                j.cancel();
            }
            (self.job, self.open) = (None, false);
        }
    }

    fn running(&self) -> bool {
        self.job.as_ref().is_some_and(|j| j.status() == Status::Running)
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

    fn out_path(&self) -> PathBuf {
        Path::new(&self.folder).join(format!("{}.{}", self.name, PRESETS[self.preset].ext))
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

    fn output_fields(&mut self, ui: &mut Ui) {
        let ext = PRESETS[self.preset].ext;
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
            ui.label(format!(".{ext}"));
        });
    }

    fn details(&self, ui: &mut Ui, project: &Project) {
        let p = &PRESETS[self.preset];
        let s = &project.settings;
        ui.label(RichText::new(p.name).strong());
        ui.label(format!("Notes:  {}", INFO[self.preset].1));
        if p.video.is_some() {
            ui.label(format!("Video:  {:.3} fps; {}x{}; Progressive", s.frame_rate, s.width, s.height));
        }
        ui.label(format!("Audio:  {} Hz; Stereo", p.sample_rate(s.sample_rate)));
        let (a, b) = self.range(project);
        let what = if (a, b) == (0.0, content_end(project)) { "whole timeline" } else { "loop region" };
        ui.label(format!("Length: {} ({what})", format_timecode(b - a, s.frame_rate)));
    }

    /// Progress / result block and the bottom buttons. True when Render was clicked.
    fn footer(&mut self, ui: &mut Ui, project: &Project, render_label: &str) -> bool {
        match self.job.as_ref().map(|j| (j, j.status())) {
            Some((j, Status::Running)) => {
                ui.add(ProgressBar::new(j.fraction()).show_percentage());
                ui.label(j.eta().map_or("Estimating time remaining...".to_string(), |e| format!("{e:.0} s remaining")));
            }
            Some((j, Status::Done)) => {
                ui.horizontal(|ui| {
                    ui.label(format!("Rendered {}", j.out.display()));
                    if ui.button("Open Folder").clicked() {
                        open_folder(&j.out);
                    }
                });
            }
            Some((_, Status::Failed(e))) => {
                let warn = theme::get().warning_text;
                ui.colored_label(warn, "Render failed:");
                ScrollArea::vertical().max_height(80.0).show(ui, |ui| ui.monospace(e));
            }
            Some((_, Status::Cancelled)) => {
                ui.label("Render cancelled.");
            }
            None => {}
        }
        let (a, b) = self.range(project);
        let empty = b <= a;
        let has_loop = project.timeline.loop_region.is_some_and(|(a, b)| b > a);
        let running = self.running();
        let bad = self.out_error();
        if let (Some(e), false) = (bad, running) {
            ui.colored_label(theme::get().warning_text, e);
        }
        let mut render = false;
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if super::button(ui, "Cancel", true, false).clicked() {
                if running {
                    if let Some(j) = &self.job {
                        j.cancel();
                    }
                } else {
                    (self.open, self.job) = (false, None);
                }
            }
            render = super::button(ui, render_label, !empty && !running && bad.is_none(), true).clicked();
            let mut on = self.loop_only && has_loop;
            if ui.add_enabled(has_loop && !running, Checkbox::new(&mut on, "Render loop region only")).changed() {
                self.loop_only = on;
            }
        });
        render
    }

    fn body16(&mut self, ui: &mut Ui, project: &Project) -> bool {
        ui.label("Output File");
        self.output_fields(ui);
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add(TextEdit::singleline(&mut self.search).hint_text("Search render templates").desired_width(300.0));
            super::fixed_combo(ui, "render_filter", "Filters", 120.0, Later("R11"));
        });
        let d = theme::get().dialog();
        let q = self.search.to_lowercase();
        Frame::NONE.fill(d.field).stroke(Stroke::new(1.0_f32, d.field_border)).inner_margin(4).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ScrollArea::vertical().id_salt("render_tree").max_height(220.0).auto_shrink(false).show(ui, |ui| {
                ui.label(RichText::new("Formats").strong());
                for (k, p) in PRESETS.iter().enumerate() {
                    if !q.is_empty() && !p.name.to_lowercase().contains(&q) && !INFO[k].0.to_lowercase().contains(&q) {
                        continue;
                    }
                    let arrow = if self.expanded[k] { "⏷" } else { "⏵" };
                    if ui.selectable_label(false, format!("{arrow} {}", INFO[k].0)).clicked() {
                        self.expanded[k] = !self.expanded[k];
                    }
                    if self.expanded[k] || !q.is_empty() {
                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            star(ui, &mut self.favorites[k]);
                            if ui.selectable_label(self.preset == k, format!("Project settings ({})", p.name)).clicked() {
                                self.preset = k;
                            }
                        });
                    }
                }
            });
        });
        ui.label("Description:");
        Frame::NONE.fill(d.field).stroke(Stroke::new(1.0_f32, d.field_border)).inner_margin(6).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(90.0);
            self.details(ui, project);
        });
        ui.add_enabled_ui(false, |ui| {
            ui.button("Customize Template...").on_disabled_hover_text(Later("R11").text());
        });
        super::group(ui, "Render Options");
        ui.add_enabled_ui(false, |ui| {
            for (o, why) in [
                ("Stretch video to fill output frame size (do not letterbox)", Later("R11")),
                ("Use project output rotation setting", Later("R11")),
                ("Save project markers in media file", Later("R11")),
                ("Enable multichannel mapping", Later("R10a")),
            ] {
                ui.checkbox(&mut false, o).on_disabled_hover_text(why.text());
            }
        });
        ui.add_space(4.0);
        self.footer(ui, project, "Render")
    }

    fn body23(&mut self, ui: &mut Ui, project: &Project) -> bool {
        let t = theme::get();
        let d = t.dialog();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Render:").strong());
            ui.label(&project.name);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                super::fixed_combo(ui, "render_view", "View", 70.0, Later("R11"));
                super::fixed_combo(ui, "render_filt", "Filter", 70.0, Later("R11"));
            });
        });
        let filter = TREE23[self.node].2;
        let mut render = false;
        ui.horizontal_top(|ui| {
            // Category tree.
            Frame::NONE.fill(d.field).inner_margin(4).show(ui, |ui| {
                ui.set_width(190.0);
                ui.set_height(330.0);
                // Frames inherit the row's horizontal layout; the tree is a column.
                ui.vertical(|ui| self.tree23(ui));
            });
            ui.vertical(|ui| {
                // Template grid: 4 columns of icon + bold title + 2-line description.
                let q = self.search.to_lowercase();
                let shown: Vec<usize> = (0..PRESETS.len())
                    .filter(|&k| match (self.node, filter) {
                        (0, _) => self.favorites[k],
                        (_, Some(video)) => PRESETS[k].video.is_some() == video,
                        _ => true,
                    })
                    .filter(|&k| q.is_empty() || PRESETS[k].name.to_lowercase().contains(&q))
                    .collect();
                Frame::NONE.fill(d.field).inner_margin(6).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.set_height(150.0);
                    Grid::new("render_grid").num_columns(4).spacing(vec2(10.0, 10.0)).show(ui, |ui| {
                        for (n, &k) in shown.iter().enumerate() {
                            let (r, hit) = ui.allocate_exact_size(vec2(170.0, 56.0), Sense::click());
                            let p = ui.painter();
                            if self.preset == k {
                                p.rect_filled(r, t.m.widget_radius, t.preset_tile_selected);
                            } else if hit.hovered() {
                                p.rect_filled(r, t.m.widget_radius, t.button_hover_bg);
                            }
                            let icon = Rect::from_min_size(r.min + vec2(4.0, 8.0), vec2(34.0, 26.0));
                            p.rect_stroke(icon, 2.0, Stroke::new(1.5_f32, d.text), StrokeKind::Inside);
                            if PRESETS[k].video.is_some() {
                                let c = icon.center();
                                p.add(Shape::convex_polygon(vec![c + vec2(-4.0, -6.0), c + vec2(6.0, 0.0), c + vec2(-4.0, 6.0)], d.text, Stroke::NONE));
                            } else {
                                p.circle_filled(icon.center(), 6.0, d.text);
                            }
                            p.text(pos2(icon.max.x + 6.0, r.min.y + 6.0), Align2::LEFT_TOP, INFO[k].0, t.bold(12.0), d.text);
                            let ink = if self.preset == k { t.text_bright } else { t.text_dim };
                            let desc = p.layout(INFO[k].1.to_string(), t.font(11.0), ink, r.max.x - icon.max.x - 10.0);
                            p.with_clip_rect(r).galley(pos2(icon.max.x + 6.0, r.min.y + 22.0), desc, ink);
                            if hit.clicked() {
                                self.preset = k;
                            }
                            if n % 4 == 3 {
                                ui.end_row();
                            }
                        }
                    });
                    if shown.is_empty() {
                        ui.label(RichText::new("No templates here.").color(t.text_dim));
                    }
                });
                ui.horizontal_top(|ui| {
                    Frame::NONE.fill(d.field).inner_margin(6).show(ui, |ui| {
                        ui.set_width(ui.available_width() - 110.0);
                        ui.set_height(88.0);
                        ui.vertical(|ui| self.details(ui, project));
                    });
                    ui.add_enabled_ui(false, |ui| {
                        ui.add(Button::new("Customize...").min_size(vec2(100.0, 30.0))).on_disabled_hover_text(Later("R11").text());
                    });
                });
                Frame::NONE.fill(d.field).inner_margin(6).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    self.output_fields(ui);
                });
                render = self.footer(ui, project, "Render...");
            });
        });
        render
    }

    fn tree23(&mut self, ui: &mut Ui) {
        ui.add(TextEdit::singleline(&mut self.search).hint_text("Search").desired_width(180.0));
        for (k, (label, depth, _)) in TREE23.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.add_space(*depth as f32 * 16.0);
                let text = if k == 0 { format!("★ {label}") } else { label.to_string() };
                if ui.selectable_label(self.node == k, text).clicked() {
                    self.node = k;
                }
            });
        }
    }

    fn start(&mut self, ctx: &Context, project: &Project) {
        let out = self.out_path();
        if out.exists()
            && rfd::MessageDialog::new()
                .set_title("Render")
                .set_description(format!("{} already exists. Replace it?", out.display()))
                .set_buttons(rfd::MessageButtons::YesNo)
                .show()
                != rfd::MessageDialogResult::Yes
        {
            return;
        }
        self.job = Some(crate::engine::render::start(project, &PRESETS[self.preset], out, self.range(project), ctx.clone()));
    }
}

/// Favourite ☆ that turns gold when set.
fn star(ui: &mut Ui, on: &mut bool) -> bool {
    let t = theme::get();
    let color = if *on { t.snap.cursor } else { t.text_dim };
    let r = ui.add(Label::new(RichText::new(if *on { "★" } else { "☆" }).color(color)).sense(Sense::click()));
    if r.clicked() {
        *on = !*on;
    }
    r.clicked()
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
