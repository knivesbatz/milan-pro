//! R38: the "Open still image sequence" question. VEGAS has the check box in its own Open dialog; Milan Pro's file
//! dialog is Windows' (rfd), which takes no check box, so the question is asked afterwards, only when a picked or
//! dropped still has numbered siblings. The box remembers its state (Preferences, like VEGAS's dialog) and the
//! sequence plays at the project's frame rate (Project Properties), as VEGAS does.
use std::path::PathBuf;
use eframe::egui::*;
use crate::edit::DropMode;
use crate::ui::widgets;

/// A sequence found among the picked files: the first picked file, (pattern, first number, count), the picked
/// members (`media::sequences_in`).
pub type Seq = (PathBuf, (PathBuf, u32, u32), Vec<PathBuf>);

pub struct SequenceAsk {
    /// Everything that was picked or dropped, and where it goes (a drop: its target; None for File › Import).
    pub paths: Vec<PathBuf>,
    pub drop: Option<Option<(Option<String>, f64, DropMode)>>,
    pub seqs: Vec<Seq>,
    /// The check box.
    pub open_as_sequence: bool,
    /// The project's frame rate, which the sequence will play at.
    pub fps: f64,
}

impl SequenceAsk {
    pub fn new(paths: Vec<PathBuf>, drop: Option<Option<(Option<String>, f64, DropMode)>>, seqs: Vec<Seq>, open_as_sequence: bool, fps: f64) -> Self {
        Self { paths, drop, seqs, open_as_sequence, fps }
    }

    /// `shot_0001.png ... shot_0048.png (48 frames)` for a sequence.
    pub fn describe(seq: &(PathBuf, u32, u32)) -> String {
        let s = crate::project::Sequence { pattern: seq.0.clone(), start: seq.1, count: seq.2, fps: 1.0 };
        format!("{} ... {} ({} frames)", s.file_name(s.start), s.file_name(s.start + s.count - 1), s.count)
    }

    /// Some(true) on OK, Some(false) on Cancel, × or Esc.
    pub fn show(&mut self, ctx: &Context) -> Option<bool> {
        let n = self.seqs.len();
        let (hit, close) = super::modal(ctx, "still_sequence", "Open Still Image Sequence", vec2(440.0, 150.0 + 16.0 * n.min(4) as f32), |ui| {
            let d = crate::ui::theme::get().dialog();
            ui.label(if n == 1 { "The picked image is part of a numbered sequence:".to_string() } else { format!("{n} of the picked images belong to numbered sequences:") });
            for s in self.seqs.iter().take(4) {
                ui.label(RichText::new(Self::describe(&s.1)).color(d.text));
            }
            if n > 4 {
                ui.label(format!("... and {} more", n - 4));
            }
            ui.add_space(6.0);
            widgets::checkbox(ui, &mut self.open_as_sequence, "Open still image sequence");
            ui.label(RichText::new(format!("One event per sequence, playing at the project rate ({:.3} fps). Unchecked, only the picked files open, as stills.", self.fps)).color(d.text_disabled).size(11.0));
            ui.add_space(10.0);
            super::button_row(ui, &[("OK", true), ("Cancel", true)])
        });
        match hit {
            Some(k) => Some(k == 0),
            None => close.then_some(false),
        }
    }

    /// The import list after the answer: each sequence's picked members collapse into its first file, opened as the
    /// sequence (when the box is on), and the other files open as they are.
    pub fn resolve(&self) -> Vec<(PathBuf, crate::media::ImportOpts)> {
        let mut out: Vec<(PathBuf, crate::media::ImportOpts)> = Vec::new();
        for p in &self.paths {
            match self.seqs.iter().find(|s| s.2.contains(p)) {
                Some(s) if self.open_as_sequence => {
                    if !out.iter().any(|o| o.0 == s.0) {
                        out.push((s.0.clone(), crate::media::ImportOpts { sequence: Some(self.fps) }));
                    }
                }
                _ => out.push((p.clone(), crate::media::ImportOpts::default())),
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// On: a sequence's picked files become one entry (its first picked file, opened as the sequence) and the rest stay;
    /// off: everything opens as picked.
    #[test]
    fn resolve_collapses_sequences() {
        let p = |s: &str| PathBuf::from(s);
        let seq = (p("a_0003.png"), (p("a_%04d.png"), 1, 12), vec![p("a_0003.png"), p("a_0001.png")]);
        let mut ask = SequenceAsk::new(vec![p("a_0003.png"), p("clip.mp4"), p("a_0001.png")], None, vec![seq], true, 25.0);
        let r = ask.resolve();
        assert_eq!(r.len(), 2);
        assert_eq!((r[0].0.clone(), r[0].1.sequence, r[1].0.clone(), r[1].1.sequence), (p("a_0003.png"), Some(25.0), p("clip.mp4"), None));
        ask.open_as_sequence = false;
        assert_eq!(ask.resolve().iter().map(|r| (r.0.clone(), r.1.sequence)).collect::<Vec<_>>(), [(p("a_0003.png"), None), (p("clip.mp4"), None), (p("a_0001.png"), None)]);
        assert_eq!(SequenceAsk::describe(&(p("a_%04d.png"), 1, 12)), "a_0001.png ... a_0012.png (12 frames)");
    }
}
