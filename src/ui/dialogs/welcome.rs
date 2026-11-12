//! VP23 Welcome screen (§5.14): 900×600 modal with a left tab rail (New Project / Open Project / Getting
//! Started), aspect tiles, resolution and frame-rate pickers, and "Show Welcome Screen on Startup".
use eframe::egui::*;
use crate::project::ProjectSettings;
use super::super::theme;

const ASPECTS: [(&str, f32, &[(&str, u32, u32)]); 5] = [
    ("Widescreen 16:9", 16.0 / 9.0, &[("FullHD (1920 x 1080)", 1920, 1080), ("HD (1280 x 720)", 1280, 720), ("UHD (3840 x 2160)", 3840, 2160)]),
    ("Fullscreen 4:3", 4.0 / 3.0, &[("HDV (1440 x 1080)", 1440, 1080), ("SD (640 x 480)", 640, 480)]),
    ("Portrait 9:16", 9.0 / 16.0, &[("FullHD (1080 x 1920)", 1080, 1920), ("HD (720 x 1280)", 720, 1280)]),
    ("Square 1:1", 1.0, &[("1080 x 1080", 1080, 1080), ("720 x 720", 720, 720)]),
    ("Scope 2.4:1", 2.4, &[("2592 x 1080", 2592, 1080), ("1920 x 800", 1920, 800)]),
];
const RATES: [(&str, f64); 5] = [("24p", 24.0), ("25p", 25.0), ("30p", 30.0), ("50p", 50.0), ("60p", 60.0)];

pub enum WelcomeResult {
    /// Create a project with these settings; `true` = open Project Properties for more.
    Create(ProjectSettings, bool),
    Open,
    OpenRecent(usize),
    Close,
}

pub struct Welcome {
    page: usize,
    aspect: usize,
    res: usize,
    rate: usize,
    drop: bool,
}

impl Default for Welcome {
    fn default() -> Self {
        Self { page: 0, aspect: 0, res: 0, rate: 2, drop: true }
    }
}

impl Welcome {
    fn fps(&self) -> f64 {
        let f = RATES[self.rate].1;
        // 25/50 have no NTSC variant.
        if self.drop && f != 25.0 && f != 50.0 { f * 1000.0 / 1001.0 } else { f }
    }

    pub fn show(&mut self, ctx: &Context, recent: &[String], show_on_start: &mut bool, sample_rate: u32) -> Option<WelcomeResult> {
        let t = theme::get();
        let d = t.dialog();
        let mut out = None;
        let resp = super::with_style(ctx, || {
            Modal::new(Id::new("welcome"))
                .frame(Frame::NONE.fill(d.body).stroke(Stroke::new(1.0_f32, d.border)))
                .backdrop_color(Color32::from_black_alpha(80))
                .show(ctx, |ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(900.0, 600.0), Sense::hover());
                    let p = ui.painter().clone();
                    // Left rail, 100 px: icon over label.
                    let rail = Rect::from_min_size(rect.min, vec2(100.0, rect.height()));
                    p.rect_filled(rail, 0.0, t.frame);
                    for (k, label) in ["New Project", "Open Project", "Getting Started"].into_iter().enumerate() {
                        let r = Rect::from_min_size(rail.min + vec2(0.0, 12.0 + 84.0 * k as f32), vec2(100.0, 76.0));
                        let hit = ui.interact(r, Id::new(("welcome_tab", k)), Sense::click());
                        let active = self.page == k;
                        if active {
                            p.rect_filled(r.shrink(6.0), t.m.widget_radius, d.accent);
                        } else if hit.hovered() {
                            p.rect_filled(r.shrink(6.0), t.m.widget_radius, t.button_hover_bg);
                        }
                        let ink = if active { d.accent_text } else { d.text };
                        rail_icon(&p, Rect::from_center_size(r.center() - vec2(0.0, 10.0), vec2(26.0, 26.0)), k, ink);
                        p.text(r.center() + vec2(0.0, 18.0), Align2::CENTER_CENTER, label, t.font(11.0), ink);
                        if hit.clicked() {
                            self.page = k;
                        }
                    }
                    // Close ×.
                    let x = Rect::from_min_size(pos2(rect.max.x - 36.0, rect.min.y + 4.0), vec2(32.0, 28.0));
                    let xr = ui.interact(x, Id::new("welcome_close"), Sense::click());
                    if xr.hovered() {
                        p.rect_filled(x, 0.0, t.caption_close_hover);
                    }
                    p.text(x.center(), Align2::CENTER_CENTER, "×", t.font(16.0), d.text);
                    if xr.clicked() {
                        out = Some(WelcomeResult::Close);
                    }

                    let body = Rect::from_min_max(pos2(rail.max.x + 32.0, rect.min.y + 28.0), pos2(rect.max.x - 32.0, rect.max.y - 48.0));
                    let mut ui_body = ui.new_child(UiBuilder::new().max_rect(body).layout(Layout::top_down(Align::Min)));
                    match self.page {
                        0 => self.new_page(&mut ui_body, &mut out, sample_rate),
                        1 => open_page(&mut ui_body, recent, &mut out),
                        _ => getting_started(&mut ui_body),
                    }
                    // Footer.
                    let foot = Rect::from_min_max(pos2(rail.max.x + 32.0, rect.max.y - 40.0), pos2(rect.max.x - 32.0, rect.max.y - 8.0));
                    let mut ui_foot = ui.new_child(UiBuilder::new().max_rect(foot).layout(Layout::left_to_right(Align::Center)));
                    ui_foot.checkbox(show_on_start, "Show Welcome Screen on Startup");
                })
        });
        if resp.should_close() && out.is_none() {
            out = Some(WelcomeResult::Close);
        }
        out
    }

    fn new_page(&mut self, ui: &mut Ui, out: &mut Option<WelcomeResult>, sample_rate: u32) {
        let t = theme::get();
        let d = t.dialog();
        ui.label(RichText::new("New Project").font(t.semibold(20.0)));
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            for (k, (label, ratio, _)) in ASPECTS.iter().enumerate() {
                let (r, hit) = ui.allocate_exact_size(vec2(130.0, 110.0), Sense::click());
                let sel = self.aspect == k;
                let p = ui.painter();
                p.rect(r, t.m.widget_radius, t.band_bg, Stroke::new(if sel { 2.0_f32 } else { 1.0_f32 }, if sel { d.accent } else if hit.hovered() { d.button_hover_border } else { d.field_border }), StrokeKind::Inside);
                // The aspect as a frame inside a 90×56 box.
                let bx = vec2(90.0, 56.0);
                let fr = if *ratio >= bx.x / bx.y { vec2(bx.x, bx.x / ratio) } else { vec2(bx.y * ratio, bx.y) };
                p.rect_stroke(Rect::from_center_size(r.center() - vec2(0.0, 12.0), fr), 2.0, Stroke::new(2.0_f32, if sel { d.accent } else { d.text }), StrokeKind::Inside);
                p.text(pos2(r.center().x, r.max.y - 14.0), Align2::CENTER_CENTER, *label, t.font(11.0), d.text);
                if hit.clicked() && !sel {
                    (self.aspect, self.res) = (k, 0);
                }
            }
        });
        ui.add_space(20.0);
        let res = ASPECTS[self.aspect].2;
        super::row(ui, "Resolution", 110.0, true, |ui| {
            ComboBox::from_id_salt("welcome_res").width(260.0).selected_text(res[self.res].0).show_ui(ui, |ui| {
                for (k, r) in res.iter().enumerate() {
                    ui.selectable_value(&mut self.res, k, r.0);
                }
            });
        });
        let fps = self.fps();
        super::row(ui, "Framerate", 110.0, true, |ui| {
            ComboBox::from_id_salt("welcome_fps").width(160.0).selected_text(format!("{} ({:.3})", RATES[self.rate].0, fps)).show_ui(ui, |ui| {
                for (k, r) in RATES.iter().enumerate() {
                    ui.selectable_value(&mut self.rate, k, r.0);
                }
            });
            ComboBox::from_id_salt("welcome_drop")
                .width(170.0)
                .selected_text(if self.drop { "Drop Frame (1/1.001)" } else { "Integer" })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.drop, true, "Drop Frame (1/1.001)");
                    ui.selectable_value(&mut self.drop, false, "Integer");
                });
        });
        ui.add_space(28.0);
        ui.horizontal(|ui| {
            let (_, w, h) = res[self.res];
            let settings = ProjectSettings { width: w, height: h, frame_rate: (fps * 1000.0).round() / 1000.0, sample_rate };
            if ui.add(Button::new(RichText::new("Create Project").color(d.accent_text)).fill(d.accent).min_size(vec2(140.0, 30.0))).clicked() {
                *out = Some(WelcomeResult::Create(settings.clone(), false));
            }
            if ui.add(Button::new("Advanced Settings...").min_size(vec2(140.0, 30.0))).clicked() {
                *out = Some(WelcomeResult::Create(settings, true));
            }
        });
    }
}

fn open_page(ui: &mut Ui, recent: &[String], out: &mut Option<WelcomeResult>) {
    let t = theme::get();
    ui.label(RichText::new("Open Project").font(t.semibold(20.0)));
    ui.add_space(16.0);
    if ui.add(Button::new("Open...").min_size(vec2(140.0, 30.0))).clicked() {
        *out = Some(WelcomeResult::Open);
    }
    ui.add_space(16.0);
    ui.label(RichText::new("Recent projects").color(t.text_dim));
    for (k, path) in recent.iter().enumerate() {
        if ui.link(path).clicked() {
            *out = Some(WelcomeResult::OpenRecent(k));
        }
    }
    if recent.is_empty() {
        ui.label(RichText::new("No recent projects.").color(t.text_dim));
    }
}

fn getting_started(ui: &mut Ui) {
    let t = theme::get();
    ui.label(RichText::new("Getting Started").font(t.semibold(20.0)));
    ui.add_space(16.0);
    for line in [
        "Import media with File › Import › Media..., or drag files onto Project Media.",
        "Drag media to the timeline; S splits at the cursor, Delete removes the selected event.",
        "Space plays, J / K / L shuttle, M adds a marker, R a region over the time selection.",
        "Render with File › Render...; see Help › Keyboard Shortcuts for every key.",
    ] {
        ui.label(line);
        ui.add_space(4.0);
    }
}

/// Original rail glyphs: page with +, folder, play circle.
fn rail_icon(p: &Painter, r: Rect, k: usize, ink: Color32) {
    let s = Stroke::new(1.5_f32, ink);
    match k {
        0 => {
            p.rect_stroke(r.shrink2(vec2(5.0, 2.0)), 2.0, s, StrokeKind::Inside);
            p.line_segment([r.center() - vec2(5.0, 0.0), r.center() + vec2(5.0, 0.0)], s);
            p.line_segment([r.center() - vec2(0.0, 5.0), r.center() + vec2(0.0, 5.0)], s);
        }
        1 => {
            let b = r.shrink2(vec2(1.0, 5.0));
            p.line_segment([b.left_top(), b.left_top() + vec2(9.0, 0.0)], s);
            p.line_segment([b.left_top() + vec2(9.0, 0.0), b.left_top() + vec2(12.0, 3.0)], s);
            p.rect_stroke(Rect::from_min_max(b.min + vec2(0.0, 3.0), b.max), 1.0, s, StrokeKind::Inside);
        }
        _ => {
            p.circle_stroke(r.center(), 11.0, s);
            let c = r.center();
            p.add(Shape::convex_polygon(vec![c + vec2(-3.0, -5.0), c + vec2(5.0, 0.0), c + vec2(-3.0, 5.0)], ink, Stroke::NONE));
        }
    }
}
