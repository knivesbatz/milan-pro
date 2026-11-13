//! Help › Keyboard Shortcuts: the keymap table for the current look.
use eframe::egui::*;
use super::super::keymap::{self, TABLE};
use super::super::theme;

/// Shows the list while `open`; × / Esc / OK close it.
pub fn shortcuts_window(ctx: &Context, open: &mut bool) {
    if !*open {
        return;
    }
    let look = theme::get().look();
    let (ok, close) = super::modal(ctx, "keyboard_shortcuts", "Keyboard Shortcuts", vec2(520.0, 600.0), |ui| {
        ScrollArea::vertical().max_height(490.0).auto_shrink(false).show(ui, |ui| {
            Grid::new("shortcut_grid").num_columns(2).striped(true).spacing(vec2(16.0, 2.0)).show(ui, |ui| {
                for row in TABLE {
                    let keys = keymap::keys(row.0, look);
                    if keys.is_empty() {
                        continue;
                    }
                    ui.label(row.1);
                    ui.label(keys.iter().map(|k| keymap::fmt_key(k)).collect::<Vec<_>>().join("  or  "));
                    ui.end_row();
                }
            });
        });
        ui.add_space(4.0);
        ui.label("Num keys are on the numeric keypad, which Milan Pro cannot tell from the top row: use the other key listed, or the menu.");
        ui.add_space(4.0);
        super::button_row(ui, &[("OK", true)]).is_some()
    });
    if ok || close {
        *open = false;
    }
}
