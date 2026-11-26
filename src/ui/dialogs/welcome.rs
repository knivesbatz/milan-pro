//! The Start dialog (File › Welcome, and at startup): the splash art over a Start group (New Project, Advanced
//! Settings..., Open Project...), the recent projects in a sunken list, "Show this dialog at startup" and Close.
use eframe::egui::*;
use crate::project::ProjectSettings;
use super::super::theme;

pub enum WelcomeResult {
    /// Create a project with these settings; `true` = open Project Properties for more.
    Create(ProjectSettings, bool),
    Open,
    OpenRecent(usize),
    Close,
}

#[derive(Default)]
pub struct Welcome;

impl Welcome {
    /// A new project takes the VEGAS 23 Welcome defaults: FullHD at 29.970p (30 / 1.001), resampling off.
    pub fn show(&mut self, ctx: &Context, recent: &[String], show_on_start: &mut bool, sample_rate: u32) -> Option<WelcomeResult> {
        use super::super::widgets::{self, Brand};
        let t = theme::get();
        let d = t.dialog();
        let mut out = None;
        let (_, close) = super::modal(ctx, "welcome", "Milan Pro", vec2(540.0, 522.0), |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            let (r, _) = ui.allocate_exact_size(vec2(520.0, 320.0), Sense::hover());
            let tex = widgets::brand_tex(ui.ctx(), Brand::Splash);
            ui.painter().image(tex.id(), widgets::px(r), Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
            ui.add_space(2.0);
            super::group(ui, "Start");
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                let fps = 30.0 * 1000.0 / 1001.0;
                let settings = ProjectSettings { width: 1920, height: 1080, frame_rate: (fps * 1000.0_f64).round() / 1000.0, sample_rate, resample: crate::project::Resample::Disable, ..Default::default() };
                if super::classic_button(ui, "New Project", vec2(120.0, 23.0), true, true).clicked() {
                    out = Some(WelcomeResult::Create(settings.clone(), false));
                }
                if super::classic_button(ui, "Advanced Settings...", vec2(130.0, 23.0), true, false).clicked() {
                    out = Some(WelcomeResult::Create(settings, true));
                }
                if super::classic_button(ui, "Open Project...", vec2(120.0, 23.0), true, false).clicked() {
                    out = Some(WelcomeResult::Open);
                }
            });
            super::group(ui, "Recent projects");
            super::field_box(ui, 4, |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(56.0);
                ui.spacing_mut().item_spacing.y = 2.0;
                for (k, path) in recent.iter().enumerate() {
                    if ui.link(RichText::new(path).color(d.accent)).clicked() {
                        out = Some(WelcomeResult::OpenRecent(k));
                    }
                }
                if recent.is_empty() {
                    ui.label(RichText::new("No recent projects.").color(d.text_disabled));
                }
            });
            ui.horizontal(|ui| {
                widgets::checkbox(ui, show_on_start, "Show this dialog at startup");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if super::button(ui, "Close", true, false).clicked() {
                        out = Some(WelcomeResult::Close);
                    }
                });
            });
        });
        if close && out.is_none() {
            out = Some(WelcomeResult::Close);
        }
        out
    }
}
