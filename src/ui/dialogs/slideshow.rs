//! R18b: Insert › Slideshow... (VEGAS 23's Slideshow Creator, also a video lane's right-click Insert Slideshow). The
//! Pictures tab lists the pictures in slideshow order (Add Pictures, Delete, Move Selected -1 / +1, Sort A to Z / Z to
//! A, New to Old / Old to New, Shuffle); the Settings tab places the slideshow (at the cursor or a typed time), sets
//! each picture's length or the whole slideshow's (the pictures share it), the transition (Effects) and the overlap.
// shortcut: no Rotate buttons and no pan/zoom styles (VEGAS's Effects with Ken Burns moves); the transitions are
// Milan Pro's, used in turn or at random. Add them if users ask.
use std::path::PathBuf;
use eframe::egui::*;
use crate::engine::plugins::{self, PluginKind};

pub struct Slideshow {
    pub pictures: Vec<PathBuf>,
    selected: Option<usize>,
    tab: usize,
    /// Settings: start at the cursor (else at `at`), each picture's length (else the whole slideshow's), the effect
    /// (0 Crossfade, then the transitions, then Random) and the overlap, as typed in the ruler's format.
    pub at_cursor: bool,
    pub at: String,
    pub per_picture: bool,
    pub len: String,
    pub total: String,
    pub effect: usize,
    pub overlap: String,
}

/// The pictures share `total` seconds (overlaps included): each one's length; `per` when it is given instead.
pub fn picture_len(n: usize, per: Option<f64>, total: f64, overlap: f64) -> f64 {
    per.unwrap_or_else(|| (total + n.saturating_sub(1) as f64 * overlap) / n.max(1) as f64)
}

impl Slideshow {
    /// `at`, `len` and `overlap` as the ruler shows the cursor, the still length and the overlap.
    pub fn new(at: String, len: String, overlap: String) -> Self {
        Self { pictures: Vec::new(), selected: None, tab: 0, at_cursor: true, at, per_picture: true, total: len.clone(), len, effect: 0, overlap }
    }

    /// The Effects list: Crossfade, the transitions, Random.
    pub fn effects() -> Vec<&'static str> {
        std::iter::once("Crossfade").chain(plugins::catalog(PluginKind::Transition).map(|i| i.name)).chain(["Random"]).collect()
    }

    /// The transitions effect `k` uses in turn (none: a plain crossfade), and whether at random.
    pub fn transitions(k: usize) -> (Vec<crate::project::Transition>, bool) {
        let all: Vec<_> = plugins::catalog(PluginKind::Transition).map(|i| crate::edit::transition_of(&plugins::new_fx(i, i.presets.first()))).collect();
        match k {
            0 => (Vec::new(), false),
            k if k <= all.len() => (vec![all[k - 1].clone()], false),
            _ => (all, true),
        }
    }

    /// Some(true) on Create, Some(false) on Cancel, × or Esc; `parse_len` and `valid_time` read the typed fields.
    pub fn show(&mut self, ctx: &Context, parse_len: impl Fn(&str) -> Option<f64>, valid_time: impl Fn(&str) -> bool) -> Option<bool> {
        let (hit, close) = super::modal(ctx, "slideshow", "Slideshow Creator", vec2(460.0, 400.0), |ui| {
            super::tabs(ui, &[&["Pictures", "Settings"]], &mut self.tab);
            super::page(ui, 290.0, |ui| if self.tab == 0 { self.pictures_tab(ui) } else { self.settings_tab(ui) });
            ui.add_space(8.0);
            let ok = !self.pictures.is_empty()
                && (self.at_cursor || valid_time(&self.at))
                && parse_len(if self.per_picture { &self.len } else { &self.total }).is_some_and(|v| v > 0.0)
                && parse_len(&self.overlap).is_some_and(|v| v >= 0.0);
            super::button_row(ui, &[("Create", ok), ("Cancel", true)])
        });
        match hit {
            Some(k) => Some(k == 0),
            None => close.then_some(false),
        }
    }

    fn pictures_tab(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui.button("Add Pictures...").clicked() {
                let exts = crate::media::IMAGE_EXTS;
                if let Some(paths) = rfd::FileDialog::new().add_filter("Pictures", exts).pick_files() {
                    for p in paths {
                        if !self.pictures.contains(&p) {
                            self.pictures.push(p);
                        }
                    }
                }
            }
            let sel = self.selected.filter(|&i| i < self.pictures.len());
            if ui.add_enabled(sel.is_some(), Button::new("Delete")).clicked() {
                if let Some(i) = sel {
                    self.pictures.remove(i);
                    self.selected = None;
                }
            }
            for (label, d) in [("Move Selected -1", -1i64), ("Move Selected +1", 1)] {
                let to = sel.map(|i| i as i64 + d).filter(|&j| j >= 0 && (j as usize) < self.pictures.len());
                if ui.add_enabled(to.is_some(), Button::new(label)).clicked() {
                    if let (Some(i), Some(j)) = (sel, to) {
                        self.pictures.swap(i, j as usize);
                        self.selected = Some(j as usize);
                    }
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            let name = |p: &PathBuf| p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            let modified = |p: &PathBuf| std::fs::metadata(p).and_then(|m| m.modified()).ok();
            let any = self.pictures.len() > 1;
            if ui.add_enabled(any, Button::new("Sort A to Z")).clicked() {
                self.pictures.sort_by_key(name);
            }
            if ui.add_enabled(any, Button::new("Sort Z to A")).clicked() {
                self.pictures.sort_by_key(name);
                self.pictures.reverse();
            }
            if ui.add_enabled(any, Button::new("Sort New to Old")).clicked() {
                self.pictures.sort_by_key(|p| std::cmp::Reverse(modified(p)));
            }
            if ui.add_enabled(any, Button::new("Sort Old to New")).clicked() {
                self.pictures.sort_by_key(modified);
            }
            if ui.add_enabled(any, Button::new("Shuffle")).clicked() {
                self.pictures.sort_by_cached_key(|_| uuid::Uuid::new_v4());
            }
        });
        ui.add_space(4.0);
        ScrollArea::vertical().max_height(200.0).auto_shrink([false, false]).show(ui, |ui| {
            if self.pictures.is_empty() {
                ui.weak("Add Pictures to begin.");
            }
            for (i, p) in self.pictures.iter().enumerate() {
                let label = format!("{}  {}", i + 1, p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned()));
                if ui.selectable_label(self.selected == Some(i), label).clicked() {
                    self.selected = Some(i);
                }
            }
        });
    }

    fn settings_tab(&mut self, ui: &mut Ui) {
        const W: f32 = 150.0;
        super::group(ui, "Slideshow Placement");
        ui.radio_value(&mut self.at_cursor, true, "Start at the timeline cursor");
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.at_cursor, false, "Start at:");
            ui.add_enabled(!self.at_cursor, TextEdit::singleline(&mut self.at).desired_width(120.0));
        });
        super::group(ui, "Duration");
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.per_picture, true, "Each picture:");
            ui.add_enabled(self.per_picture, TextEdit::singleline(&mut self.len).desired_width(120.0));
        });
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.per_picture, false, "Whole slideshow:");
            ui.add_enabled(!self.per_picture, TextEdit::singleline(&mut self.total).desired_width(120.0));
        });
        super::group(ui, "Transitions");
        let names = Self::effects();
        super::row(ui, "Effects:", W - 70.0, true, |ui| {
            ComboBox::from_id_salt("slideshow_effect").width(200.0).selected_text(names.get(self.effect).copied().unwrap_or("")).show_ui(ui, |ui| {
                for (k, n) in names.iter().enumerate() {
                    ui.selectable_value(&mut self.effect, k, *n);
                }
            });
        });
        super::row(ui, "Overlap:", W - 70.0, true, |ui| ui.add(TextEdit::singleline(&mut self.overlap).desired_width(120.0)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three pictures over 10 s with 1 s overlaps last 4 s each (0-4, 3-7, 6-10); a given length is used as it is.
    #[test]
    fn a_total_length_is_shared() {
        assert_eq!(picture_len(3, None, 10.0, 1.0), 4.0);
        assert_eq!(picture_len(3, Some(2.5), 10.0, 1.0), 2.5);
        assert_eq!(picture_len(1, None, 6.0, 1.0), 6.0);
        // Crossfade first, Random last; one transition used alone, Random draws from all of them.
        let n = Slideshow::effects().len();
        assert!(Slideshow::transitions(0).0.is_empty() && Slideshow::transitions(1).0.len() == 1 && !Slideshow::transitions(1).1);
        let (all, random) = Slideshow::transitions(n - 1);
        assert!(random && all.len() == n - 2);
    }
}
