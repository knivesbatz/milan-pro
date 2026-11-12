//! Help › Unfinished Features: every greyed-out menu row and command, and why (R0), grouped by menu.
use eframe::egui::*;
use super::super::keymap::{self, TABLE};
use super::super::menus::{self, Why};

/// (group, rows): the menu bar's menus, the context menus, then commands only a shortcut reaches.
pub fn groups() -> Vec<(String, Vec<(String, Why)>)> {
    let mut out: Vec<(String, Vec<(String, Why)>)> = Vec::new();
    let mut add = |name: &str, items: &[menus::M]| {
        let name = keymap::mnemonic(name).0;
        let k = out.iter().position(|g| g.0 == name).unwrap_or_else(|| {
            out.push((name, Vec::new()));
            out.len() - 1
        });
        menus::greyed(items, &mut out[k].1);
    };
    let mut in_menus = Vec::new();
    for (name, items) in menus::MENUS.iter().chain(super::super::timeline::CONTEXT_MENUS) {
        add(name, items);
        menus::commands(items, &mut in_menus);
    }
    for items in super::super::media_panel::ITEM_MENUS {
        add("Project Media", items);
    }
    let keys = TABLE
        .iter()
        .filter(|r| !in_menus.contains(&r.0))
        .filter_map(|r| menus::unfinished(r.0).map(|w| (format!("{} ({})", r.1, r.2.first().map_or(String::new(), |k| keymap::fmt_key(k))), w)))
        .collect();
    out.push(("Keyboard shortcuts".into(), keys));
    out.retain(|g| !g.1.is_empty());
    out
}

/// Shows the list while `open`; × / Esc / OK close it.
pub fn unfinished_window(ctx: &Context, open: &mut bool) {
    if !*open {
        return;
    }
    let (ok, close) = super::modal(ctx, "unfinished_features", "Unfinished Features", vec2(760.0, 600.0), |ui| {
        ui.label("Greyed-out items and the batch of docs/REMAINING.md that builds each. Toolbar buttons and dialog settings say the same when hovered.");
        ui.add_space(4.0);
        ScrollArea::vertical().max_height(480.0).auto_shrink(false).show(ui, |ui| {
            for (k, (name, rows)) in groups().into_iter().enumerate() {
                super::group(ui, &name);
                Grid::new(("unfinished_grid", k)).num_columns(2).striped(true).spacing(vec2(16.0, 2.0)).min_col_width(210.0).show(ui, |ui| {
                    for (label, why) in rows {
                        ui.label(label);
                        ui.label(why.text());
                        ui.end_row();
                    }
                });
                ui.add_space(6.0);
            }
        });
        ui.add_space(8.0);
        super::button_row(ui, &[("OK", true)]).is_some()
    });
    if ok || close {
        *open = false;
    }
}
