//! Dock windows that have no editor behind them yet.
use eframe::egui::*;
use super::dock::Panel;
use super::theme;

/// Empty window with an honest note; its toolbar stays blank.
pub fn placeholder(ui: &mut Ui, panel: Panel) {
    let t = theme::get();
    let r = ui.max_rect();
    let text = format!("{} is not available in Milan Pro yet.", panel.title(t.look()));
    ui.painter().text(r.center(), Align2::CENTER_CENTER, text, t.font(t.font.ui), t.text_dim);
}
