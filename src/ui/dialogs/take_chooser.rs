//! Take Chooser (Take › Choose Active... and Take › Delete...): the event's takes, the active one first; OK makes the
//! picked take active or deletes it.
// shortcut: no Play/Stop preview of the picked take as in VEGAS; add one if users ask (the Trimmer can play it).
use eframe::egui::*;
use crate::project::Project;

pub struct TakeChooser {
    pub id: String,
    /// Delete... (else Choose Active...).
    pub delete: bool,
    /// The picked take: 0 the active one, then the inactive takes in order.
    pub pick: usize,
}

impl TakeChooser {
    /// For event `id`; None when it is gone.
    pub fn open(project: &Project, id: &str, delete: bool) -> Option<Self> {
        let c = project.clip(id)?;
        // Delete... starts on the first inactive take, the one most likely to go.
        Some(Self { id: id.to_string(), delete, pick: (delete && !c.takes.is_empty()) as usize })
    }

    /// Some(true) on OK, Some(false) on Cancel, × or Esc (or when the event is gone); None while open.
    pub fn show(&mut self, ctx: &Context, project: &Project) -> Option<bool> {
        let Some(c) = project.clip(&self.id) else { return Some(false) };
        let names: Vec<&str> = std::iter::once(c.name.as_str()).chain(c.takes.iter().map(|t| t.name.as_str())).collect();
        self.pick = self.pick.min(names.len() - 1);
        let title = if self.delete { "Delete Take" } else { "Take Chooser" };
        // The only take cannot be deleted; choosing the active one changes nothing.
        let ok = names.len() > 1 && (self.delete || self.pick > 0);
        let pick = &mut self.pick;
        let (hit, close) = super::modal(ctx, "take_chooser", title, vec2(360.0, 260.0), |ui| {
            ui.label("Takes:");
            Frame::NONE.stroke(Stroke::new(1.0_f32, ui.visuals().widgets.noninteractive.bg_stroke.color)).show(ui, |ui| {
                ScrollArea::vertical().auto_shrink([false; 2]).max_height(150.0).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    for (k, n) in names.iter().enumerate() {
                        let label = if k == 0 { format!("{n} (active)") } else { n.to_string() };
                        ui.selectable_value(pick, k, label);
                    }
                });
            });
            ui.add_space(8.0);
            super::button_row(ui, &[("OK", ok), ("Cancel", true)])
        });
        match hit {
            Some(k) => Some(k == 0),
            None => close.then_some(false),
        }
    }
}
