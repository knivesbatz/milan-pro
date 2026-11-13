//! R18a: Insert › Command... (C) and a command marker's Edit: VEGAS's Command Properties: the metadata command (one of
//! VEGAS's, or typed), its parameter, a comment and the position (in the ruler's format; the cursor for a new one).
use eframe::egui::*;
use crate::project::CommandMarker;

/// VEGAS's predefined metadata commands.
pub const COMMANDS: &[&str] = &["URL", "Text", "WMClosedCaption", "WMTextBodyText", "WMTextHeadline", "Scott EOM", "Scott Cue In", "608CC1", "608CC3"];

pub struct CommandProps {
    /// The command marker being edited; None for a new one.
    pub index: Option<usize>,
    pub marker: CommandMarker,
    /// The Position box, as typed.
    pub position: String,
}

impl CommandProps {
    pub fn new(index: Option<usize>, marker: CommandMarker, position: String) -> Self {
        Self { index, marker, position }
    }

    /// Some(true) on OK, Some(false) on Cancel, × or Esc; `valid` says whether the Position text is a time.
    pub fn show(&mut self, ctx: &Context, valid: impl Fn(&str) -> bool) -> Option<bool> {
        let (hit, close) = super::modal(ctx, "command_props", "Command Properties", vec2(440.0, 228.0), |ui| {
            super::row(ui, "Command:", 90.0, true, |ui| {
                ui.add(TextEdit::singleline(&mut self.marker.command).desired_width(250.0));
                ComboBox::from_id_salt("command_kind").width(20.0).selected_text("").show_ui(ui, |ui| {
                    for c in COMMANDS {
                        if ui.selectable_label(self.marker.command == *c, *c).clicked() {
                            self.marker.command = c.to_string();
                        }
                    }
                });
            });
            super::row(ui, "Parameter:", 90.0, true, |ui| ui.add(TextEdit::singleline(&mut self.marker.parameter).desired_width(290.0)));
            super::row(ui, "Comment:", 90.0, true, |ui| ui.add(TextEdit::multiline(&mut self.marker.comment).desired_rows(3).desired_width(290.0)));
            super::row(ui, "Position:", 90.0, true, |ui| ui.add(TextEdit::singleline(&mut self.position).desired_width(140.0)));
            ui.add_space(8.0);
            let ok = valid(&self.position) && !self.marker.command.trim().is_empty();
            super::button_row(ui, &[("OK", ok), ("Cancel", true)])
        });
        match hit {
            Some(0) => Some(true),
            Some(_) => Some(false),
            None => close.then_some(false),
        }
    }
}
