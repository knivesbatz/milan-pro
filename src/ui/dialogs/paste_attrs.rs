//! Edit › Selectively Paste Event Attributes...: which attributes of the copied event go onto the selected events.
use eframe::egui::*;
use crate::edit::Attrs;

pub struct PasteAttrs {
    pub attrs: Attrs,
}

impl PasteAttrs {
    pub fn new() -> Self {
        Self { attrs: Attrs::ALL }
    }

    /// Some(true) on OK, Some(false) on Cancel, × or Esc.
    pub fn show(&mut self, ctx: &Context) -> Option<bool> {
        let a = &mut self.attrs;
        let (hit, close) = super::modal(ctx, "paste_attrs", "Selectively Paste Event Attributes", vec2(330.0, 250.0), |ui| {
            ui.label("Paste these attributes of the copied event:");
            ui.indent("paste_attrs_list", |ui| {
                ui.checkbox(&mut a.fx, "Event FX");
                ui.checkbox(&mut a.pan_crop, "Pan/Crop (video)");
                ui.checkbox(&mut a.envelopes, "Envelopes");
                ui.checkbox(&mut a.gain, "Gain (audio)");
                ui.checkbox(&mut a.rate, "Playback rate");
                ui.checkbox(&mut a.fades, "Fades");
            });
            ui.add_space(8.0);
            super::button_row(ui, &[("OK", true), ("Cancel", true)])
        });
        match hit {
            Some(k) => Some(k == 0),
            None => close.then_some(false),
        }
    }
}
