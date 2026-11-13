//! Set Default Track Properties (track header right-click): saves properties of the track as the ones new tracks of
//! its kind start with, or restores the built-in defaults. Audio: Track FX (gate, EQ and compressor built in); video
//! (R18b, VEGAS's list): Height, Composite Mode and Track FX.
// shortcut: audio offers only Track FX (VEGAS also has Volume, Pan Type, Input monitor, Height and Automation mode);
// add them if users ask.
use eframe::egui::*;

pub struct DefaultTrackProps {
    /// The track the command was given on, and its kind.
    pub track: String,
    pub video: bool,
    pub track_fx: bool,
    pub height: bool,
    pub composite: bool,
    pub restore: bool,
}

impl DefaultTrackProps {
    pub fn new(track: String, video: bool) -> Self {
        Self { track, video, track_fx: true, height: video, composite: video, restore: false }
    }

    /// Some(true) on OK, Some(false) on Cancel, × or Esc; None while open.
    pub fn show(&mut self, ctx: &Context) -> Option<bool> {
        let video = self.video;
        let (fx, height, composite, restore) = (&mut self.track_fx, &mut self.height, &mut self.composite, &mut self.restore);
        let size = vec2(380.0, if video { 236.0 } else { 190.0 });
        let (hit, close) = super::modal(ctx, "default_track", "Set Default Track Properties", size, |ui| {
            ui.label(format!("Save these properties of the selected track as the default for new {} tracks:", if video { "video" } else { "audio" }));
            ui.add_space(4.0);
            if video {
                ui.add_enabled(!*restore, Checkbox::new(height, "Height"));
                ui.add_enabled(!*restore, Checkbox::new(composite, "Composite Mode"));
            }
            ui.add_enabled(!*restore, Checkbox::new(fx, "Track FX"));
            ui.checkbox(restore, "Restore original defaults");
            ui.add_space(8.0);
            let any = *restore || *fx || (video && (*height || *composite));
            super::button_row(ui, &[("OK", any), ("Cancel", true)])
        });
        match hit {
            Some(k) => Some(k == 0),
            None => close.then_some(false),
        }
    }
}
