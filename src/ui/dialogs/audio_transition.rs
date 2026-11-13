//! R18b: Insert › Audio Transition...: a crossfade preset (the outgoing event's fade curve and the incoming one's, as
//! the crossfade's Fade Type list pairs them) and the length a cut is overlapped by, for the selected audio at the
//! cursor.
use eframe::egui::*;
use crate::project::Curve;

pub const CURVES: [(&str, Curve); 5] = [("Linear", Curve::Linear), ("Fast", Curve::Fast), ("Slow", Curve::Slow), ("Smooth", Curve::Smooth), ("Sharp", Curve::Sharp)];

pub struct AudioTransition {
    pub out: Curve,
    pub inc: Curve,
    /// The overlap a cut becomes, in the ruler's format.
    pub len: String,
}

impl AudioTransition {
    pub fn new(len: String) -> Self {
        Self { out: Curve::Smooth, inc: Curve::Smooth, len }
    }

    /// Some(true) on OK, Some(false) on Cancel, × or Esc; `valid` says whether the length is one.
    pub fn show(&mut self, ctx: &Context, valid: impl Fn(&str) -> bool) -> Option<bool> {
        let (hit, close) = super::modal(ctx, "audio_transition", "Audio Transition", vec2(330.0, 176.0), |ui| {
            for (label, id, curve) in [("Fade out:", "at_out", &mut self.out), ("Fade in:", "at_in", &mut self.inc)] {
                super::row(ui, label, 90.0, true, |ui| {
                    let name = CURVES.iter().find(|c| c.1 == *curve).map_or("", |c| c.0);
                    ComboBox::from_id_salt(id).width(180.0).selected_text(name).show_ui(ui, |ui| {
                        for (n, c) in CURVES {
                            ui.selectable_value(curve, c, n);
                        }
                    });
                });
            }
            super::row(ui, "Length:", 90.0, true, |ui| ui.add(TextEdit::singleline(&mut self.len).desired_width(120.0)));
            ui.add_space(8.0);
            super::button_row(ui, &[("OK", valid(&self.len)), ("Cancel", true)])
        });
        match hit {
            Some(k) => Some(k == 0),
            None => close.then_some(false),
        }
    }
}
