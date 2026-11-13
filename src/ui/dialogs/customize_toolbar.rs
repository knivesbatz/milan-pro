//! Options › Customize Toolbar and Customize Timeline Toolbar (R15), as VEGAS's (the Windows toolbar dialog): the
//! buttons the bar can take and the ones it has, with Add, Remove, Move Up, Move Down and Reset. The bar changes as
//! the dialog goes; Close ends it.
use eframe::egui::*;
use crate::ui::icons::paint_icon;
use crate::ui::theme::{self, UiLook};
use crate::ui::toolbar::{Bar, SEP, btn};

pub struct CustomizeToolbar {
    pub bar: Bar,
    pub look: UiLook,
    /// The bar's buttons as arranged so far.
    pub list: Vec<&'static str>,
    avail: Option<&'static str>,
    cur: Option<usize>,
}

impl CustomizeToolbar {
    pub fn new(bar: Bar, list: Vec<&'static str>, look: UiLook) -> Self {
        Self { bar, look, list, avail: None, cur: None }
    }

    /// The buttons that can be added: the separator, then those the bar lacks.
    pub fn available(&self) -> Vec<&'static str> {
        self.bar.allowed(self.look).map(|b| b.id).filter(|id| *id == SEP || !self.list.contains(id)).collect()
    }

    /// Add the chosen available button before the selected one (at the end with none), and select it.
    pub fn add(&mut self) {
        let Some(id) = self.avail.filter(|id| self.available().contains(id)) else { return };
        let at = self.cur.unwrap_or(self.list.len()).min(self.list.len());
        self.list.insert(at, id);
        self.cur = Some(at);
        if id != SEP {
            self.avail = None;
        }
    }

    pub fn remove(&mut self) {
        let Some(k) = self.cur.filter(|&k| k < self.list.len()) else { return };
        self.list.remove(k);
        self.cur = (!self.list.is_empty()).then(|| k.min(self.list.len() - 1));
    }

    pub fn move_by(&mut self, up: bool) {
        let Some(k) = self.cur.filter(|&k| k < self.list.len()) else { return };
        let to = if up { k.checked_sub(1) } else { Some(k + 1).filter(|&n| n < self.list.len()) };
        if let Some(to) = to {
            self.list.swap(k, to);
            self.cur = Some(to);
        }
    }

    /// VEGAS's buttons for this bar and look.
    pub fn reset(&mut self) {
        self.list = self.bar.defaults(self.look);
        self.cur = None;
    }

    /// (the bar changed, the dialog closed).
    pub fn show(&mut self, ctx: &Context) -> (bool, bool) {
        let before = self.list.clone();
        let title = if self.bar == Bar::Main { "Customize Toolbar" } else { "Customize Timeline Toolbar" };
        let (close, x) = super::modal(ctx, "customize_toolbar", title, vec2(620.0, 420.0), |ui| {
            let d = theme::get().dialog();
            let frame = Frame::NONE.fill(d.field).stroke(Stroke::new(1.0_f32, d.field_border)).inner_margin(2);
            let (mut close, look) = (false, self.look);
            ui.horizontal_top(|ui| {
                // One list: rows of glyph and name; the selected row's index.
                let list = |ui: &mut Ui, salt: &str, ids: &[&'static str], sel: Option<usize>| -> Option<usize> {
                    let mut hit = None;
                    frame.show(ui, |ui| {
                        ui.set_width(190.0);
                        ScrollArea::vertical().id_salt(salt).max_height(320.0).auto_shrink(false).show(ui, |ui| {
                            ui.with_layout(Layout::top_down_justified(Align::LEFT), |ui| {
                                ui.spacing_mut().item_spacing.y = 0.0;
                                for (k, id) in ids.iter().enumerate() {
                                    let Some(b) = btn(id) else { continue };
                                    let r = ui.selectable_label(sel == Some(k), format!("       {}", b.name));
                                    if *id != SEP {
                                        let g = Rect::from_center_size(pos2(r.rect.min.x + 12.0, r.rect.center().y), Vec2::splat(16.0));
                                        paint_icon(ui.painter(), g, b.icon, look, None);
                                    }
                                    if r.clicked() {
                                        hit = Some(k);
                                    }
                                }
                            });
                        });
                    });
                    hit
                };
                let avail = self.available();
                ui.vertical(|ui| {
                    ui.label("Available toolbar buttons:");
                    if let Some(k) = list(ui, "tb_avail", &avail, self.avail.and_then(|a| avail.iter().position(|x| *x == a))) {
                        self.avail = Some(avail[k]);
                    }
                });
                ui.vertical(|ui| {
                    ui.add_space(60.0);
                    if super::button(ui, "Add ->", self.avail.is_some(), false).clicked() {
                        self.add();
                    }
                    if super::button(ui, "<- Remove", self.cur.is_some(), false).clicked() {
                        self.remove();
                    }
                });
                let cur = self.list.clone();
                ui.vertical(|ui| {
                    ui.label("Current toolbar buttons:");
                    if let Some(k) = list(ui, "tb_current", &cur, self.cur) {
                        self.cur = Some(k);
                    }
                });
                ui.vertical(|ui| {
                    ui.add_space(18.0);
                    close = super::button(ui, "Close", true, true).clicked();
                    if super::button(ui, "Reset", true, false).clicked() {
                        self.reset();
                    }
                    ui.add_space(12.0);
                    let k = self.cur.unwrap_or(usize::MAX);
                    if super::button(ui, "Move Up", k > 0 && k < self.list.len(), false).clicked() {
                        self.move_by(true);
                    }
                    if super::button(ui, "Move Down", k + 1 < self.list.len(), false).clicked() {
                        self.move_by(false);
                    }
                });
            });
            close
        });
        (self.list != before, close || x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Add, remove, reorder and reset, and the bar reads back what was saved (without ids it cannot hold).
    #[test]
    fn toolbar_add_remove_reorder_and_reset() {
        let mut d = CustomizeToolbar::new(Bar::Main, Bar::Main.defaults(UiLook::Vegas23), UiLook::Vegas23);
        assert_eq!(d.list, ["New", "Open", "Save", "Render", "Properties", "Cut", "Copy", "Paste", "Undo", "Redo"]);
        let avail = d.available();
        assert!(avail[0] == SEP && avail.contains(&"Split") && !avail.contains(&"Save"));
        assert!(!avail.contains(&"Play") && !avail.contains(&"Tutorials"), "no play buttons on the main bar; Tutorials is VEGAS 16's");
        d.cur = Some(2);
        d.avail = Some("Split");
        d.add();
        assert_eq!((&d.list[1..4], d.cur), (&["Open", "Split", "Save"][..], Some(2)));
        d.avail = Some(SEP);
        d.cur = None;
        d.add();
        d.add();
        assert_eq!(&d.list[d.list.len() - 3..], ["Redo", SEP, SEP], "separators can be added again and again");
        d.remove();
        d.cur = Some(0);
        d.move_by(true);
        d.move_by(false);
        d.move_by(false);
        assert_eq!(&d.list[..3], ["Open", "Split", "New"]);
        d.cur = Some(0);
        d.remove();
        assert_eq!((d.list[0], d.cur), ("Split", Some(0)));
        let saved = BTreeMap::from([(Bar::Main.key(UiLook::Vegas23), d.list.iter().map(|s| s.to_string()).chain(["Nope".into(), "Play".into()]).collect())]);
        assert_eq!(Bar::Main.buttons(&saved, UiLook::Vegas23), d.list);
        assert_eq!(Bar::Main.buttons(&saved, UiLook::Vegas16), Bar::Main.defaults(UiLook::Vegas16), "each look has its own bar");
        d.reset();
        assert_eq!(d.list, Bar::Main.defaults(UiLook::Vegas23));
        // The timeline bar holds the play buttons; VEGAS 16's has no Smart Split or Color Grading.
        let t16 = Bar::Timeline.defaults(UiLook::Vegas16);
        assert!(t16.starts_with(&["Record", "Loop"]) && !t16.contains(&"SmartSplit") && *t16.last().unwrap() == "IgnoreGrouping");
        assert!(Bar::Timeline.defaults(UiLook::Vegas23).ends_with(&["OpenParent", "OpenNested", "CreateNested"]));
        for look in [UiLook::Vegas16, UiLook::Vegas23] {
            for bar in [Bar::Main, Bar::Timeline] {
                assert!(bar.defaults(look).iter().all(|id| bar.allowed(look).any(|b| b.id == *id)), "{bar:?} {look:?}");
            }
        }
    }
}
