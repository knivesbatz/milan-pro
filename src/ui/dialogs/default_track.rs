//! Set Default Track Properties (audio track header right-click): saves the track's FX chain as the one new audio
//! tracks start with, or restores the built-in gate, EQ and compressor.
// shortcut: only Track FX is offered (VEGAS also has Volume, Pan Type and Height); add them if users ask.
use eframe::egui::*;

pub struct DefaultTrackProps {
    /// The audio track the command was given on.
    pub track: String,
    pub track_fx: bool,
    pub restore: bool,
}

impl DefaultTrackProps {
    pub fn new(track: String) -> Self {
        Self { track, track_fx: true, restore: false }
    }

    /// Some(true) on OK, Some(false) on Cancel, × or Esc; None while open.
    pub fn show(&mut self, ctx: &Context) -> Option<bool> {
        let (fx, restore) = (&mut self.track_fx, &mut self.restore);
        let (hit, close) = super::modal(ctx, "default_track", "Set Default Track Properties", vec2(380.0, 190.0), |ui| {
            ui.label("Save these properties of the selected track as the default for new audio tracks:");
            ui.add_space(4.0);
            ui.add_enabled(!*restore, Checkbox::new(fx, "Track FX"));
            ui.checkbox(restore, "Restore original defaults");
            ui.add_space(8.0);
            super::button_row(ui, &[("OK", *restore || *fx), ("Cancel", true)])
        });
        match hit {
            Some(k) => Some(k == 0),
            None => close.then_some(false),
        }
    }
}
