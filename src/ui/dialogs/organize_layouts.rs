//! View › Window Layouts › Organize Layouts... (R13): this look's user slots 0-9 (Alt+D, 0-9) as a list, to rename,
//! move up or down, delete and activate. VEGAS keeps layouts as files and assigns them to the ten menu slots; Milan
//! Pro keeps them in the slots themselves, so Delete is VEGAS's Clear and Delete Layout in one.
use eframe::egui::*;
use crate::ui::dock::Layout;
use crate::ui::keymap::{Command, shortcut_text};
use crate::ui::theme::{self, UiLook};

pub struct OrganizeLayouts {
    /// The slots as arranged so far: each layout with the slot it was saved in.
    pub slots: [Option<(Layout, usize)>; 10],
    sel: usize,
    name: String,
}

pub enum Organize {
    Ok,
    Cancel,
    /// Load this layout now (it was saved in that slot); the dialog stays.
    Activate(Layout, usize),
}

impl OrganizeLayouts {
    pub fn new(slots: [Option<Layout>; 10]) -> Self {
        let mut n = 0;
        let slots = slots.map(|s| {
            n += 1;
            s.map(|l| (l, n - 1))
        });
        let sel = slots.iter().position(Option::is_some).unwrap_or(0);
        let mut d = Self { slots, sel, name: String::new() };
        d.select(sel);
        d
    }

    fn select(&mut self, n: usize) {
        self.sel = n;
        self.name = self.slots[n].as_ref().map(|s| s.0.name.clone()).unwrap_or_default();
    }

    /// Swap the selected slot with the one above (or below); the selection moves with it.
    pub fn move_by(&mut self, up: bool) {
        let to = if up { self.sel.checked_sub(1) } else { Some(self.sel + 1).filter(|&n| n < 10) };
        if let Some(to) = to {
            self.slots.swap(self.sel, to);
            self.sel = to;
        }
    }

    pub fn rename(&mut self) {
        let name = self.name.trim();
        if let Some(s) = self.slots[self.sel].as_mut().filter(|_| !name.is_empty()) {
            s.0.name = name.to_string();
        }
    }

    pub fn delete(&mut self) {
        self.slots[self.sel] = None;
        self.name.clear();
    }

    pub fn show(&mut self, ctx: &Context, look: UiLook) -> Option<Organize> {
        let mut out = None;
        let (hit, close) = super::modal(ctx, "organize_layouts", "Organize Layouts", vec2(420.0, 330.0), |ui| {
            let d = theme::get().dialog();
            ui.label("Layouts in the View › Window Layouts menu:");
            ui.horizontal_top(|ui| {
                Frame::NONE.fill(d.field).stroke(Stroke::new(1.0_f32, d.field_border)).inner_margin(2).show(ui, |ui| {
                    ui.set_width(280.0);
                    ui.with_layout(eframe::egui::Layout::top_down_justified(Align::LEFT), |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.spacing_mut().interact_size.y = 20.0;
                        for n in 0..10 {
                            let name = self.slots[n].as_ref().map_or("<EMPTY>", |s| s.0.name.as_str());
                            let label = format!("{}    {name}", shortcut_text(Command::LayoutUser(n), look));
                            let r = ui.selectable_label(self.sel == n, label);
                            if r.clicked() {
                                self.select(n);
                            }
                            if r.double_clicked() {
                                if let Some(s) = &self.slots[n] {
                                    out = Some(Organize::Activate(s.0.clone(), s.1));
                                }
                            }
                        }
                    });
                });
                ui.vertical(|ui| {
                    ui.set_width(80.0);
                    let full = self.slots[self.sel].is_some();
                    if super::button(ui, "Move Up", self.sel > 0, false).clicked() {
                        self.move_by(true);
                    }
                    if super::button(ui, "Move Down", self.sel < 9, false).clicked() {
                        self.move_by(false);
                    }
                    if super::button(ui, "Activate", full, false).clicked() {
                        out = self.slots[self.sel].as_ref().map(|s| Organize::Activate(s.0.clone(), s.1));
                    }
                    if super::button(ui, "Delete", full, false).clicked() {
                        self.delete();
                    }
                });
            });
            ui.add_space(6.0);
            let full = self.slots[self.sel].is_some();
            super::row(ui, "Name:", 50.0, full, |ui| {
                ui.add(TextEdit::singleline(&mut self.name).desired_width(226.0));
                if super::button(ui, "Rename", full && !self.name.trim().is_empty(), false).clicked() {
                    self.rename();
                }
            });
            ui.add_space(10.0);
            super::button_row(ui, &[("OK", true), ("Cancel", true)])
        });
        match hit {
            Some(0) => Some(Organize::Ok),
            Some(_) => Some(Organize::Cancel),
            None if close => Some(Organize::Cancel),
            None => out,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::dock::Dock;
    use crate::ui::theme::{LookChoice, Theme};

    #[test]
    fn rename_move_and_delete_keep_where_each_came_from() {
        let t = Theme::new(LookChoice::VEGAS16);
        let l = |n: &str| Dock::new(&t).layout(n);
        let mut d = OrganizeLayouts::new(std::array::from_fn(|n| match n {
            2 => Some(l("Two")),
            3 => Some(l("Three")),
            _ => None,
        }));
        assert_eq!((d.sel, d.name.as_str()), (2, "Two"));
        d.move_by(false); // Two goes to slot 3, Three to slot 2
        d.name = "  Edit  ".into();
        d.rename();
        let names = |d: &OrganizeLayouts| d.slots.iter().map(|s| s.as_ref().map(|s| (s.0.name.clone(), s.1))).collect::<Vec<_>>();
        assert_eq!(names(&d)[2..4], [Some(("Three".into(), 3)), Some(("Edit".into(), 2))]);
        d.select(2);
        d.delete();
        d.name = "x".into();
        d.rename(); // nothing there to rename
        assert_eq!(names(&d)[2..4], [None, Some(("Edit".into(), 2))]);
        d.select(0);
        d.move_by(true);
        d.select(9);
        d.move_by(false);
        assert_eq!(names(&d).iter().filter(|s| s.is_some()).count(), 1);
    }
}
