//! Help › Keyboard Shortcuts: the keys of the current look, the user's (Customize Keyboard) where set.
use eframe::egui::*;
use super::super::keymap::{self, TABLE};
use super::super::{menus, theme};

/// Shows the list while `open`; × / Esc / OK close it.
pub fn shortcuts_window(ctx: &Context, open: &mut bool) {
    if !*open {
        return;
    }
    let look = theme::get().look();
    let binds = keymap::active(look);
    // Commands the user gave keys that the table has no row for go by their menu path.
    let named = if binds.len() > TABLE.len() { menus::named_commands(look) } else { Vec::new() };
    let classic = theme::get().classic();
    let (ok, close) = super::modal(ctx, "keyboard_shortcuts", "Keyboard Shortcuts", vec2(520.0, 600.0), |ui| {
        let list = |ui: &mut Ui| {
            ScrollArea::vertical().max_height(if classic { 486.0 } else { 490.0 }).auto_shrink(false).show(ui, |ui| {
                Grid::new("shortcut_grid").num_columns(2).striped(!classic).spacing(vec2(16.0, 2.0)).show(ui, |ui| {
                    for (k, (cmd, keys)) in binds.iter().enumerate() {
                        if keys.is_empty() {
                            continue;
                        }
                        ui.label(TABLE.get(k).map(|r| r.1.to_string()).or_else(|| named.iter().find(|n| n.1 == *cmd).map(|n| n.0.clone())).unwrap_or_default());
                        ui.label(keys.iter().map(|k| keymap::fmt_key(k)).collect::<Vec<_>>().join("  or  "));
                        ui.end_row();
                    }
                });
            });
        };
        // Classic 2006: the list in a sunken white box (no stripes).
        if classic {
            super::field_box(ui, 0, list);
        } else {
            list(ui);
        }
        ui.add_space(4.0);
        ui.label("Num keys are on the numeric keypad, which Milan Pro cannot tell from the top row: use the other key listed, or the menu.");
        ui.add_space(4.0);
        super::button_row(ui, &[("OK", true)]).is_some()
    });
    if ok || close {
        *open = false;
    }
}
