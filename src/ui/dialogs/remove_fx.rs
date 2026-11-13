//! Tools › Audio › Remove Assignable FX...: tick the assignable FX chains to remove.
use eframe::egui::*;

pub struct RemoveAssignableFx {
    /// (id, name, ticked) per chain.
    chains: Vec<(String, String, bool)>,
}

impl RemoveAssignableFx {
    pub fn new(p: &crate::project::Project) -> Self {
        Self { chains: p.timeline.audio.fx.iter().map(|b| (b.id.clone(), b.name.clone(), false)).collect() }
    }

    /// The chains ticked.
    pub fn ids(&self) -> Vec<String> {
        self.chains.iter().filter(|c| c.2).map(|c| c.0.clone()).collect()
    }

    /// Some(true) on OK, Some(false) on Cancel, × or Esc; None while open.
    pub fn show(&mut self, ctx: &Context) -> Option<bool> {
        let chains = &mut self.chains;
        let (hit, close) = super::modal(ctx, "remove_assignable_fx", "Remove Assignable FX", vec2(320.0, 260.0), |ui| {
            ui.label("Select the assignable FX chains to remove:");
            ui.add_space(4.0);
            ScrollArea::vertical().max_height(150.0).show(ui, |ui| {
                for (_, name, on) in chains.iter_mut() {
                    ui.checkbox(on, name.as_str());
                }
            });
            ui.add_space(8.0);
            super::button_row(ui, &[("OK", chains.iter().any(|c| c.2)), ("Cancel", true)])
        });
        match hit {
            Some(k) => Some(k == 0),
            None => close.then_some(false),
        }
    }
}
