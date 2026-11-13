//! Edit › Paste Repeat... (Ctrl+B): how many copies of the clipboard to paste from the cursor, end to end or one
//! every so often ("Paste every", in the ruler's format).
use eframe::egui::*;

#[derive(Clone)]
pub struct PasteRepeat {
    pub count: u32,
    /// Even spacing (else end to end).
    pub even: bool,
    pub every: String,
}

impl PasteRepeat {
    pub fn new(every: String) -> Self {
        Self { count: 1, even: false, every }
    }

    /// Some(true) on OK, Some(false) on Cancel, × or Esc; `valid` says whether the "Paste every" text is a time.
    pub fn show(&mut self, ctx: &Context, valid: impl Fn(&str) -> bool) -> Option<bool> {
        let (hit, close) = super::modal(ctx, "paste_repeat", "Paste Repeat", vec2(330.0, 190.0), |ui| {
            super::row(ui, "Number of times to paste:", 160.0, true, |ui| ui.add(DragValue::new(&mut self.count).range(1..=999)));
            ui.label("Paste spacing:");
            ui.indent("paste_spacing", |ui| {
                ui.radio_value(&mut self.even, false, "End to end");
                ui.radio_value(&mut self.even, true, "Even spacing");
            });
            let even = self.even;
            super::row(ui, "Paste every:", 160.0, even, |ui| ui.add_enabled(even, TextEdit::singleline(&mut self.every).desired_width(120.0)));
            ui.add_space(8.0);
            super::button_row(ui, &[("OK", !even || valid(&self.every)), ("Cancel", true)])
        });
        match hit {
            Some(0) => Some(true),
            Some(_) => Some(false),
            None => close.then_some(false),
        }
    }
}
