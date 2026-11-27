//! R49: the track header's Edit Visible Button Set... (VEGAS 19 on): which buttons a video or an audio track header
//! shows; the ones turned off stay in the header's More (≡) menu. Kept in the preferences
//! (`settings::HeaderButtons`), for every track of the kind.
use eframe::egui::*;
use crate::settings::HeaderButtons;
use crate::ui::widgets;

/// The buttons a header can show, (key, label), in header order. Arm for Record shows on armed audio tracks only.
pub const VIDEO: [(&str, &str); 4] = [("comp", "Compositing Mode"), ("fx", "Track FX"), ("mute", "Mute"), ("solo", "Solo")];
pub const AUDIO: [(&str, &str); 4] = [("arm", "Arm for Record"), ("fx", "Track FX"), ("mute", "Mute"), ("solo", "Solo")];

pub struct ButtonSet {
    video: bool,
    /// Shown, per entry of `VIDEO` or `AUDIO`.
    shown: Vec<bool>,
}

impl ButtonSet {
    pub fn new(video: bool, set: &HeaderButtons) -> Self {
        let hidden = set.hidden(video);
        Self { video, shown: Self::list(video).iter().map(|(k, _)| !hidden.iter().any(|h| h == k)).collect() }
    }

    fn list(video: bool) -> &'static [(&'static str, &'static str)] {
        if video { &VIDEO } else { &AUDIO }
    }

    /// The set as `set` with this kind's buttons as chosen.
    pub fn apply(&self, set: &HeaderButtons) -> HeaderButtons {
        let hidden: Vec<String> = Self::list(self.video).iter().zip(&self.shown).filter(|(_, on)| !**on).map(|((k, _), _)| k.to_string()).collect();
        let mut out = set.clone();
        if self.video { out.video_hidden = hidden } else { out.audio_hidden = hidden }
        out
    }

    /// Some(true) on OK, Some(false) on Cancel, × or Esc; None while open.
    pub fn show(&mut self, ctx: &Context) -> Option<bool> {
        let (video, shown) = (self.video, &mut self.shown);
        let title = if video { "Edit Visible Button Set (Video Tracks)" } else { "Edit Visible Button Set (Audio Tracks)" };
        let (hit, close) = super::modal(ctx, "button_set", title, vec2(340.0, 236.0), |ui| {
            ui.label(format!("Buttons shown on {} track headers (the others stay in the header's More menu):", if video { "video" } else { "audio" }));
            ui.add_space(4.0);
            super::field_box(ui, 4, |ui| {
                ui.set_width(ui.available_width());
                for ((_, label), on) in Self::list(video).iter().zip(shown.iter_mut()) {
                    widgets::checkbox(ui, on, *label);
                }
            });
            if !video {
                ui.label(RichText::new("Arm for Record shows on armed tracks only.").small());
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if super::button(ui, "Reset", true, false).on_hover_text("Show every button (VEGAS's default set)").clicked() {
                    shown.iter_mut().for_each(|s| *s = true);
                }
            });
            ui.add_space(4.0);
            super::button_row(ui, &[("OK", true), ("Cancel", true)])
        });
        match hit {
            Some(k) => Some(k == 0),
            None => close.then_some(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Turning a button off hides it for that kind only; Reset shows them all again.
    #[test]
    fn button_set_hides_per_kind() {
        let set = HeaderButtons::default();
        let mut d = ButtonSet::new(true, &set);
        d.shown[2] = false; // Mute
        let out = d.apply(&set);
        assert_eq!((out.video_hidden.as_slice(), out.audio_hidden.is_empty()), (["mute".to_string()].as_slice(), true));
        assert!(out.hides(true, "mute") && !out.hides(false, "mute") && !out.hides(true, "fx"));
        let back = ButtonSet::new(true, &out);
        assert_eq!(back.shown, [true, true, false, true]);
    }
}
