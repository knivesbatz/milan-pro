//! R39: Insert › Subtitles from File... (the file's cues as Titles & Text events, on a new track at the top or a
//! chosen video track, from the project start or the cursor, ASS styles mapped or the Subtitle preset) and File ›
//! Export › Subtitles... (a track's Titles & Text events as SRT or WebVTT).
use std::path::PathBuf;
use eframe::egui::*;
use crate::interchange::subtitles::{self, Format, Subs};
use crate::ui::widgets;

pub struct SubtitleImport {
    pub path: Option<PathBuf>,
    pub subs: Option<Result<Subs, String>>,
    /// 0: a new track at the top; k: video track k - 1.
    pub target: usize,
    pub at_cursor: bool,
    pub styles: bool,
}

impl SubtitleImport {
    /// For `path` (read now), or none yet (Browse...).
    pub fn new(path: Option<PathBuf>) -> Self {
        let mut d = Self { path: None, subs: None, target: 0, at_cursor: false, styles: true };
        if let Some(p) = path {
            d.load(p);
        }
        d
    }

    fn load(&mut self, p: PathBuf) {
        self.subs = Some(subtitles::read(&p));
        self.path = Some(p);
    }

    /// Where the events go.
    pub fn target(&self) -> subtitles::Target {
        if self.target == 0 { subtitles::Target::NewTrack } else { subtitles::Target::Track(self.target - 1) }
    }

    /// Some(true) on Insert, Some(false) on Cancel, × or Esc. `tracks`: the video tracks' names; `time`: a time as the
    /// ruler shows it.
    pub fn show(&mut self, ctx: &Context, tracks: &[String], time: impl Fn(f64) -> String) -> Option<bool> {
        let (hit, close) = super::modal(ctx, "subtitles_import", "Insert Subtitles from File", vec2(560.0, 470.0), |ui| {
            const W: f32 = 90.0;
            super::row(ui, "File:", W, true, |ui| {
                let shown = self.path.as_ref().map_or("(none)".to_string(), |p| p.display().to_string());
                ui.add_sized(vec2(340.0, 20.0), Label::new(shown).truncate());
                if widgets::push_button(ui, "Browse...").clicked() {
                    if let Some(p) = rfd::FileDialog::new().add_filter("Subtitles", &["srt", "vtt", "ass", "ssa"]).pick_file() {
                        self.load(p);
                    }
                }
            });
            let (ok, ass) = match &self.subs {
                Some(Ok(s)) => {
                    let (a, b) = (s.cues.first().map_or(0.0, |c| c.start), s.cues.iter().fold(0.0f64, |m, c| m.max(c.end)));
                    ui.label(format!("{}: {} subtitles, {} to {}{}", s.format.name(), s.cues.len(), time(a), time(b), if s.styles.is_empty() { String::new() } else { format!(", {} styles", s.styles.len()) }));
                    (true, s.format == Format::Ass)
                }
                Some(Err(e)) => {
                    ui.colored_label(crate::ui::theme::get().meter_red, e);
                    (false, false)
                }
                None => {
                    ui.weak("Choose a SubRip (.srt), WebVTT (.vtt) or Advanced SubStation Alpha (.ass, .ssa) file.");
                    (false, false)
                }
            };
            ui.add_space(4.0);
            let list = |ui: &mut Ui| {
                ScrollArea::vertical().max_height(170.0).auto_shrink([false, false]).show(ui, |ui| {
                    if let Some(Ok(s)) = &self.subs {
                        for c in s.cues.iter().take(200) {
                            ui.label(format!("{}   {}", time(c.start), c.text.replace('\n', " / ")));
                        }
                    }
                });
            };
            super::field_box(ui, 2, list);
            ui.add_space(6.0);
            super::group(ui, "Placement");
            widgets::radio_value(ui, &mut self.at_cursor, false, "At the file's times (from the start of the project)");
            widgets::radio_value(ui, &mut self.at_cursor, true, "From the timeline cursor");
            super::row(ui, "Track:", W, true, |ui| {
                let name = |k: usize| if k == 0 { "New track at the top".to_string() } else { tracks.get(k - 1).cloned().unwrap_or_default() };
                widgets::combo(ui, "subs_track", 300.0, name(self.target), |ui| {
                    for k in 0..=tracks.len() {
                        ui.selectable_value(&mut self.target, k, name(k));
                    }
                });
            });
            widgets::checkbox_enabled(ui, ass, &mut self.styles, "Use the file's styles (font, size, colours, outline, shadow, position)")
                .on_disabled_hover_text("SRT and WebVTT cues take the Titles & Text Subtitle preset.");
            ui.add_space(4.0);
            ui.weak("Each subtitle becomes a Titles & Text event. CEA-608/708 closed captions are not read: FFmpeg has no CEA-608/708 encoder, and its decoder only finds them in a video stream's side data.");
            ui.add_space(8.0);
            super::button_row(ui, &[("Insert", ok), ("Cancel", true)])
        });
        match hit {
            Some(k) => Some(k == 0),
            None => close.then_some(false),
        }
    }
}

pub struct SubtitleExport {
    /// An index into the tracks offered (`subtitles::text_tracks`).
    pub track: usize,
    pub format: Format,
}

impl SubtitleExport {
    pub fn new() -> Self {
        Self { track: 0, format: Format::Srt }
    }

    /// Some(true) on Export, Some(false) on Cancel, × or Esc. `tracks`: the video tracks with Titles & Text events.
    pub fn show(&mut self, ctx: &Context, tracks: &[(usize, String)]) -> Option<bool> {
        let (hit, close) = super::modal(ctx, "subtitles_export", "Export Subtitles", vec2(420.0, 200.0), |ui| {
            const W: f32 = 90.0;
            super::row(ui, "Track:", W, true, |ui| {
                let name = |k: usize| tracks.get(k).map_or(String::new(), |t| t.1.clone());
                widgets::combo(ui, "subs_export_track", 280.0, name(self.track), |ui| {
                    for k in 0..tracks.len() {
                        ui.selectable_value(&mut self.track, k, name(k));
                    }
                });
            });
            super::row(ui, "Format:", W, true, |ui| {
                widgets::radio_value(ui, &mut self.format, Format::Srt, "SubRip (.srt)");
                widgets::radio_value(ui, &mut self.format, Format::Vtt, "WebVTT (.vtt)");
            });
            ui.add_space(4.0);
            ui.weak("The track's Titles & Text events, each one a subtitle with its text and timing.");
            ui.add_space(8.0);
            super::button_row(ui, &[("Export...", !tracks.is_empty()), ("Cancel", true)])
        });
        match hit {
            Some(k) => Some(k == 0),
            None => close.then_some(false),
        }
    }
}
