//! Toolbar buttons shared by every bar (§5.2, §5.10, dock toolbars) and the main toolbar row.
use eframe::egui::*;
use crate::history::History;
use crate::project::Project;
use super::icons::{Icon, paint_icon};
use super::keymap::Command;
use super::menus::{MenuAction, save};
use super::theme::{self, UiLook};
use super::ClipRef;

const HINT: &str = "milan_status_hint";

/// Show `text` in the status bar this frame (the hovered command's description, §5.11).
pub fn set_hint(ctx: &Context, text: &str) {
    ctx.data_mut(|d| d.insert_temp(Id::new(HINT), text.to_owned()));
}

/// The hint set this frame, cleared for the next one. Call after every bar has been drawn.
pub fn take_hint(ctx: &Context) -> Option<String> {
    ctx.data_mut(|d| d.remove_temp::<String>(Id::new(HINT)))
}

fn tip(resp: Response, text: &str) -> Response {
    if resp.hovered() {
        set_hint(&resp.ctx, text);
    }
    resp.on_hover_text(text)
}

/// Hover / pressed / active-toggle fill behind a button's 22×22 hit area.
fn paint_state(p: &Painter, hit: Rect, resp: &Response, enabled: bool, active: bool) {
    let t = theme::get();
    let r = CornerRadius::same(t.m.widget_radius);
    if active {
        // VP16: 21×17 square; VP23: 22×18 rounded (§5.10).
        p.rect_filled(Rect::from_center_size(hit.center(), vec2(t.m.toggle_w, t.m.toggle_h)), r, t.button_active_bg);
    } else if enabled && resp.is_pointer_button_down_on() {
        p.rect_filled(hit, r, t.button_active_bg);
    } else if enabled && resp.hovered() {
        p.rect_filled(hit, r, t.button_hover_bg);
    }
}

/// One `button_pitch`-wide cell: 22×22 hit area with a 16 px vector glyph (§5.2).
pub fn tool_button(ui: &mut Ui, icon: Icon, text: &str, enabled: bool, active: bool) -> Response {
    button(ui, icon, text, enabled, enabled, active)
}

/// Drawn lit, as VEGAS shows it in its default state, but Milan Pro has no feature behind it yet: no click,
/// no hover fill, only the tooltip.
pub fn inert_button(ui: &mut Ui, icon: Icon, text: &str, active: bool) -> Response {
    button(ui, icon, text, false, true, active)
}

fn button(ui: &mut Ui, icon: Icon, text: &str, clickable: bool, lit: bool, active: bool) -> Response {
    let t = theme::get();
    let sense = if clickable { Sense::click() } else { Sense::hover() };
    let (cell, resp) = ui.allocate_exact_size(vec2(t.m.button_pitch, t.m.button), sense);
    let hit = Rect::from_center_size(cell.center(), Vec2::splat(t.m.button));
    paint_state(ui.painter(), hit, &resp, clickable, active);
    let tint = (!lit).then_some(t.icon_disabled);
    paint_icon(ui.painter(), Rect::from_center_size(hit.center(), Vec2::splat(t.m.icon)), icon, t.look(), tint);
    tip(resp, text)
}

/// Icon + ▾ split button, 39 px wide with one shared hover box (§5.2). Returns (icon, arrow) responses.
pub fn split_button(ui: &mut Ui, icon: Icon, text: &str, enabled: bool, active: bool) -> (Response, Response) {
    split(ui, icon, text, enabled, enabled, active)
}

/// `split_button` drawn lit with nothing behind it yet (see `inert_button`).
pub fn inert_split(ui: &mut Ui, icon: Icon, text: &str) -> (Response, Response) {
    split(ui, icon, text, false, true, false)
}

fn split(ui: &mut Ui, icon: Icon, text: &str, enabled: bool, lit: bool, active: bool) -> (Response, Response) {
    let t = theme::get();
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    // 39 px (VP16) / 41 px (VP23, light2026: undo 215 -> redo 256).
    let w = if t.look() == UiLook::Vegas16 { 39.0 } else { 41.0 };
    let (cell, _) = ui.allocate_exact_size(vec2(w, t.m.button), Sense::hover());
    let pad = (t.m.button_pitch - t.m.button) * 0.5;
    let main = Rect::from_min_size(cell.min + vec2(pad, 0.0), Vec2::splat(t.m.button));
    let arrow = Rect::from_min_max(pos2(main.max.x, cell.min.y), pos2(cell.max.x - pad - 2.0, cell.max.y));
    let m = ui.interact(main, ui.id().with((text, "main")), sense);
    let a = ui.interact(arrow, ui.id().with((text, "arrow")), sense);
    let p = ui.painter();
    let both = main.union(arrow);
    if active {
        paint_state(p, main, &m, enabled, true);
    } else if enabled && (m.hovered() || a.hovered()) {
        let pressed = m.is_pointer_button_down_on() || a.is_pointer_button_down_on();
        p.rect_filled(both, CornerRadius::same(t.m.widget_radius), if pressed { t.button_active_bg } else { t.button_hover_bg });
    }
    let tint = (!lit).then_some(t.icon_disabled);
    paint_icon(p, Rect::from_center_size(main.center(), Vec2::splat(t.m.icon)), icon, t.look(), tint);
    paint_icon(p, Rect::from_center_size(arrow.center(), Vec2::splat(t.m.icon)), Icon::DropDown, t.look(), tint);
    (tip(m, text), tip(a, text))
}

/// Separator between button groups: 2 px dark/light lines (VP16) or 1 px line (VP23), 16 px tall.
pub fn separator(ui: &mut Ui) {
    let t = theme::get();
    // 7 px (VP16) / 8 px (VP23: light2026 transport separators).
    let w = if t.look() == UiLook::Vegas16 { 7.0 } else { 8.0 };
    let (cell, _) = ui.allocate_exact_size(vec2(w, t.m.button), Sense::hover());
    let x = cell.center().x.floor();
    let y = Rangef::new(cell.center().y - 8.0, cell.center().y + 8.0);
    let p = ui.painter();
    if t.look() == UiLook::Vegas16 {
        p.rect_filled(Rect::from_x_y_ranges(Rangef::new(x - 1.0, x), y), 0.0, t.frame);
        p.rect_filled(Rect::from_x_y_ranges(Rangef::new(x, x + 1.0), y), 0.0, t.button_hover_bg);
    } else {
        p.rect_filled(Rect::from_x_y_ranges(Rangef::new(x, x + 1.0), y), 0.0, t.button_group_sep);
    }
}

/// Popup under a ▾ (or any button) that opens on click; closes when an item is clicked.
pub fn arrow_menu<R>(ui: &Ui, button: &Response, add_contents: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    let id = button.id.with("popup");
    if button.clicked() {
        ui.memory_mut(|m| m.toggle_popup(id));
    }
    popup_below_widget(ui, id, button, PopupCloseBehavior::CloseOnClick, |ui| {
        ui.set_min_width(160.0);
        add_contents(ui)
    })
}

/// Text drop-down in a toolbar ("Preview (Auto) ▾", "100 % ▾"): label, then a ▾ glyph `gap` px after the text;
/// egui menu semantics so items may hold submenus.
pub fn text_menu(ui: &mut Ui, text: &str, gap: f32, tip_text: &str, add_contents: impl FnOnce(&mut Ui)) {
    let t = theme::get();
    let galley = ui.painter().layout_no_wrap(text.to_owned(), t.font(t.font.ui), t.text);
    let tw = galley.size().x;
    // 4 px pad, text, gap, 5 px ▾, 5 px pad.
    let w = 4.0 + tw + gap + 10.0;
    let r = ui.scope(|ui| {
        menu::menu_custom_button(ui, Button::new("").frame(false).min_size(vec2(w, t.m.button)), add_contents)
    });
    let rect = r.inner.response.rect;
    ui.painter().galley(pos2(rect.min.x + 4.0, rect.center().y - galley.size().y * 0.5), galley, t.text);
    // The ▾ glyph's visible part starts 5.5 px into its 16 px cell.
    let arrow = Rect::from_min_size(pos2(rect.min.x + 4.0 + tw + gap - 5.5, rect.center().y - 8.0), Vec2::splat(t.m.icon));
    paint_icon(ui.painter(), arrow, Icon::DropDown, t.look(), None);
    tip(r.inner.response, tip_text);
}

/// History ▾ list: entry k undoes (or redoes) k steps at once.
fn history_popup(ui: &mut Ui, arrow: &Response, steps: usize, verb: &str) -> Option<usize> {
    let id = arrow.id.with("popup");
    if arrow.clicked() {
        ui.memory_mut(|m| m.toggle_popup(id));
    }
    popup_below_widget(ui, id, arrow, PopupCloseBehavior::CloseOnClick, |ui| {
        ui.set_min_width(160.0);
        (1..=steps.min(20)).find(|&k| ui.button(format!("{verb} {k} step{}", if k == 1 { "" } else { "s" })).clicked())
    })
    .flatten()
}

/// Main toolbar (§5.2): `ui` is a left-to-right row over the toolbar body. Returns app-level actions.
pub fn draw_main_toolbar(
    ui: &mut Ui,
    project: &mut Project,
    history: &mut History,
    selected_clip: &mut Option<ClipRef>,
) -> Option<MenuAction> {
    let t = theme::get();
    let v16 = t.look() == UiLook::Vegas16;
    ui.spacing_mut().item_spacing.x = 0.0;
    // First 22 px hit area starts at x = 6 (glyph at x = 9).
    // VP23's first glyph sits 2 px further left (light2026: New at x = 9).
    ui.add_space(if v16 { 6.0 } else { 4.0 } - (t.m.button_pitch - t.m.button) * 0.5);
    let mut action = None;
    let mut act = |resp: Response, a: MenuAction| {
        if resp.clicked() {
            action = Some(a);
        }
    };
    act(tool_button(ui, Icon::New, "New Empty Project (Ctrl+Shift+N)", true, false), MenuAction::New);
    act(tool_button(ui, Icon::Open, "Open (Ctrl+O)", true, false), MenuAction::Open);
    if tool_button(ui, Icon::Save, "Save (Ctrl+S)", true, false).clicked() {
        save(project);
    }
    let render = if v16 { "Render As (Ctrl+M)" } else { "Render (Ctrl+M)" };
    // Greyed with nothing to render, as VEGAS does.
    act(tool_button(ui, Icon::Render, render, crate::render::content_end(project) > 0.0, false), MenuAction::RenderAs);
    act(tool_button(ui, Icon::Properties, "Properties (Alt+Enter)", true, false), MenuAction::Properties);
    let st = super::menus::state(ui.ctx());
    for (icon, tip, cmd) in [
        (Icon::Cut, "Cut (Ctrl+X)", Command::Cut),
        (Icon::Copy, "Copy (Ctrl+C)", Command::Copy),
        (Icon::Paste, "Paste (Ctrl+V)", Command::Paste),
    ] {
        if tool_button(ui, icon, tip, st.enabled(cmd), false).clicked() {
            super::menus::send(ui.ctx(), cmd);
        }
    }
    let (undo_n, redo_n) = history.lens();
    let (m, a) = split_button(ui, Icon::Undo, "Undo (Ctrl+Z)", undo_n > 0, false);
    let undo = if m.clicked() { Some(1) } else { history_popup(ui, &a, undo_n, "Undo") };
    let (m, a) = split_button(ui, Icon::Redo, "Redo (Ctrl+Shift+Z)", redo_n > 0, false);
    let redo = if m.clicked() { Some(1) } else { history_popup(ui, &a, redo_n, "Redo") };
    for _ in 0..undo.unwrap_or(0) {
        history.undo(project);
    }
    for _ in 0..redo.unwrap_or(0) {
        history.redo(project);
    }
    if undo.or(redo).is_some() {
        *selected_clip = None;
    }
    if v16 {
        act(tool_button(ui, Icon::Tutorials, "Interactive Tutorials", true, false), MenuAction::About);
        act(tool_button(ui, Icon::WhatsThis, "What's This? Help (Shift+F1)", true, false), MenuAction::About);
    }
    action
}
