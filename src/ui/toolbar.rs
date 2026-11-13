//! Toolbar buttons shared by every bar (§5.2, §5.10, dock toolbars) and the main toolbar row.
use eframe::egui::*;
use crate::edit::RippleMode;
use crate::history::History;
use crate::project::Project;
use super::EditTool;
use super::dock::Panel;
use super::icons::{Icon, paint_icon};
use super::keymap::Command;
use super::menus::{Later, NotPlanned, Why, send};
use super::theme::{self, UiLook};
use super::transport::Playback;

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

/// Main toolbar (§5.2): `ui` is a left-to-right row over the toolbar body; `ids` its buttons (`bar_buttons`).
pub fn draw_main_toolbar(ui: &mut Ui, ids: &[&'static str], project: &mut Project, history: &History) {
    let t = theme::get();
    let v16 = t.look() == UiLook::Vegas16;
    ui.spacing_mut().item_spacing.x = 0.0;
    // First 22 px hit area starts at x = 6 (glyph at x = 9).
    // VP23's first glyph sits 2 px further left (light2026: New at x = 9).
    ui.add_space(if v16 { 6.0 } else { 4.0 } - (t.m.button_pitch - t.m.button) * 0.5);
    draw_buttons(ui, ids, project, history, None);
}

// ---- R15: the buttons a toolbar holds (Options › Customize Toolbar, Customize Timeline Toolbar) ----

/// A button a toolbar can hold: its id in the preferences, glyph, name (the tooltip, with the command's key) and the
/// command it sends; `look` limits it to one look, and `inert` draws it lit with nothing behind it yet.
pub struct Btn {
    pub id: &'static str,
    pub icon: Icon,
    pub name: &'static str,
    pub cmd: Option<Command>,
    look: Option<UiLook>,
    inert: Option<Why>,
}

const fn b(id: &'static str, icon: Icon, name: &'static str, cmd: Command) -> Btn {
    Btn { id, icon, name, cmd: Some(cmd), look: None, inert: None }
}

const fn play(id: &'static str, icon: Icon, name: &'static str) -> Btn {
    Btn { id, icon, name, cmd: None, look: None, inert: None }
}

const V23: Option<UiLook> = Some(UiLook::Vegas23);

/// A separator between button groups.
pub const SEP: &str = "|";

/// Every button, in the order Customize Toolbar lists them.
pub const BUTTONS: &[Btn] = &[
    Btn { id: SEP, icon: Icon::More, name: "Separator", cmd: None, look: None, inert: None },
    b("New", Icon::New, "New Empty Project", Command::NewEmpty),
    b("Open", Icon::Open, "Open", Command::Open),
    b("Save", Icon::Save, "Save", Command::Save),
    b("Render", Icon::Render, "Render As", Command::RenderAs),
    b("Properties", Icon::Properties, "Properties", Command::Properties),
    b("Cut", Icon::Cut, "Cut", Command::Cut),
    b("Copy", Icon::Copy, "Copy", Command::Copy),
    b("Paste", Icon::Paste, "Paste", Command::Paste),
    b("Undo", Icon::Undo, "Undo", Command::Undo),
    b("Redo", Icon::Redo, "Redo", Command::Redo),
    Btn { id: "Tutorials", icon: Icon::Tutorials, name: "Interactive Tutorials", cmd: None, look: Some(UiLook::Vegas16), inert: Some(Later("R53")) },
    Btn { look: Some(UiLook::Vegas16), inert: Some(Later("R53")), ..b("WhatsThis", Icon::WhatsThis, "What's This? Help", Command::WhatsThis) },
    play("Record", Icon::Record, "Record"),
    play("Loop", Icon::Loop, "Loop Playback"),
    play("PlayFromStart", Icon::PlayFromStart, "Play From Start"),
    play("Play", Icon::Play, "Play"),
    play("Pause", Icon::Pause, "Pause"),
    play("Stop", Icon::Stop, "Stop"),
    play("GoToStart", Icon::GoToStart, "Go to Start"),
    play("GoToEnd", Icon::GoToEnd, "Go to End"),
    play("PrevFrame", Icon::PrevFrame, "Previous Frame"),
    play("NextFrame", Icon::NextFrame, "Next Frame"),
    b("NormalTool", Icon::NormalEdit, "Normal Edit Tool", Command::NormalTool),
    b("EnvelopeTool", Icon::EnvelopeEdit, "Envelope Edit Tool", Command::Tool(EditTool::Envelope)),
    b("SelectionTool", Icon::SelectionEdit, "Selection Edit Tool", Command::Tool(EditTool::Selection)),
    Btn { id: "ZoomTool", icon: Icon::ZoomEdit, name: "Zoom Edit Tool", cmd: None, look: None, inert: Some(NotPlanned("R58")) },
    b("Delete", Icon::Delete, "Delete", Command::Delete),
    b("Trim", Icon::Trim, "Trim to Time Selection", Command::Trim),
    b("TrimStart", Icon::TrimStart, "Trim Start", Command::TrimStart),
    b("TrimEnd", Icon::TrimEnd, "Trim End", Command::TrimEnd),
    b("Split", Icon::Split, "Split", Command::Split),
    Btn { look: V23, ..b("SmartSplit", Icon::SmartSplit, "Smart Split", Command::SmartSplit) },
    b("Lock", Icon::Lock, "Lock", Command::EventLock),
    b("InsertMarker", Icon::InsertMarker, "Insert Marker", Command::InsertMarker),
    b("InsertRegion", Icon::InsertRegion, "Insert Region", Command::InsertRegion),
    b("Snapping", Icon::Snapping, "Enable Snapping", Command::Snapping),
    b("AutoCrossfade", Icon::AutoCrossfade, "Automatic Crossfades", Command::AutoCrossfade),
    b("AutoRipple", Icon::AutoRipple, "Auto Ripple", Command::AutoRipple),
    b("LockEnvelopes", Icon::LockEnvelopes, "Lock Envelopes to Events", Command::LockEnvelopes),
    b("IgnoreGrouping", Icon::IgnoreGrouping, "Ignore Event Grouping", Command::IgnoreGrouping),
    Btn { look: V23, ..b("ColorGrading", Icon::ColorGrading, "Color Grading", Command::Window(Panel::ColorGrading)) },
    Btn { look: V23, ..b("OpenParent", Icon::OpenParent, "Open Parent Timeline", Command::NestedParent) },
    Btn { look: V23, ..b("OpenNested", Icon::OpenNested, "Open Nested Timeline", Command::NestedOpen) },
    Btn { look: V23, ..b("CreateNested", Icon::CreateNested, "Create Nested Timeline", Command::NestedCreate) },
];

/// The play buttons, in `transport::Cmd::ALL` order: only the timeline toolbar, which has the play state, holds them.
pub const TRANSPORT: [&str; 10] = ["Record", "Loop", "PlayFromStart", "Play", "Pause", "Stop", "GoToStart", "GoToEnd", "PrevFrame", "NextFrame"];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Bar {
    /// The main toolbar under the menu bar.
    Main,
    /// The transport and editing toolbar under the timeline.
    Timeline,
}

impl Bar {
    /// Its key in `Prefs::toolbars` for `look` (VEGAS 23 Dark and White share one).
    pub fn key(self, look: UiLook) -> String {
        crate::settings::layout_key(look, if self == Bar::Main { "toolbar" } else { "timeline_toolbar" })
    }

    /// The buttons VEGAS's bar has in `look` (§5.2, §5.10).
    pub fn defaults(self, look: UiLook) -> Vec<&'static str> {
        let v23 = look == UiLook::Vegas23;
        let mut ids: Vec<&'static str> = match self {
            Bar::Main => vec!["New", "Open", "Save", "Render", "Properties", "Cut", "Copy", "Paste", "Undo", "Redo"],
            Bar::Timeline => TRANSPORT.to_vec(),
        };
        if self == Bar::Main {
            if !v23 {
                ids.extend(["Tutorials", "WhatsThis"]);
            }
            return ids;
        }
        ids.extend([SEP, "NormalTool", "EnvelopeTool", "SelectionTool", "ZoomTool", SEP, "Delete", "Trim", "TrimStart", "TrimEnd", "Split"]);
        if v23 {
            ids.push("SmartSplit");
        }
        ids.extend(["Lock", SEP, "InsertMarker", "InsertRegion", SEP, "Snapping", "AutoCrossfade", "AutoRipple", "LockEnvelopes", "IgnoreGrouping"]);
        if v23 {
            ids.extend([SEP, "ColorGrading", SEP, "OpenParent", "OpenNested", "CreateNested"]);
        }
        ids
    }

    /// The buttons this bar can hold in `look`, the separator first.
    pub fn allowed(self, look: UiLook) -> impl Iterator<Item = &'static Btn> {
        BUTTONS.iter().filter(move |b| b.look.is_none_or(|l| l == look) && (self == Bar::Timeline || !TRANSPORT.contains(&b.id)))
    }

    /// The bar's buttons as saved in `toolbars` (ids it cannot hold dropped), else its defaults.
    pub fn buttons(self, toolbars: &std::collections::BTreeMap<String, Vec<String>>, look: UiLook) -> Vec<&'static str> {
        match toolbars.get(&self.key(look)) {
            Some(ids) => ids.iter().filter_map(|id| self.allowed(look).find(|b| b.id == id).map(|b| b.id)).collect(),
            None => self.defaults(look),
        }
    }
}

pub fn btn(id: &str) -> Option<&'static Btn> {
    BUTTONS.iter().find(|b| b.id == id)
}

/// Draws `ids` left to right; `playback` (the timeline toolbar's) runs the play buttons.
pub fn draw_buttons(ui: &mut Ui, ids: &[&'static str], project: &mut Project, history: &History, mut playback: Option<&mut Playback<'_>>) {
    let look = theme::get().look();
    let st = super::menus::state(ui.ctx());
    let tip = |name: &str, c: Option<Command>| match c.map(|c| super::keymap::shortcut_text(c, look)).filter(|k| !k.is_empty()) {
        Some(k) => format!("{name} ({k})"),
        None => name.to_string(),
    };
    for &id in ids {
        let Some(b) = btn(id) else { continue };
        if id == SEP {
            separator(ui);
            continue;
        }
        if let Some(k) = TRANSPORT.iter().position(|t| *t == id) {
            if let Some(p) = playback.as_deref_mut() {
                super::transport::cmd_button(ui, super::transport::Cmd::ALL[k], project, p);
            }
            continue;
        }
        if let Some(why) = b.inert {
            inert_button(ui, b.icon, &why.tip(&tip(b.name, b.cmd)), false);
            continue;
        }
        let Some(c) = b.cmd else { continue };
        let name = if id == "Render" && look == UiLook::Vegas23 { "Render" } else { b.name };
        let text = tip(name, Some(c));
        match id {
            "Undo" | "Redo" => {
                let n = if id == "Undo" { history.lens().0 } else { history.lens().1 };
                let (m, a) = split_button(ui, b.icon, &text, n > 0, false);
                // One command per step, so the app's Undo/Redo handling (selection, drag state) runs for each.
                let steps = if m.clicked() { Some(1) } else { history_popup(ui, &a, n, id) };
                for _ in 0..steps.unwrap_or(0) {
                    send(ui.ctx(), c);
                }
            }
            // A radio group with the Envelope and Selection tools; ▾ also lists the tools that act on its drags.
            "NormalTool" => {
                let normal = !matches!(st.tool, EditTool::Envelope | EditTool::Selection);
                let (m, a) = split_button(ui, b.icon, &text, true, normal);
                if m.clicked() {
                    send(ui.ctx(), c);
                }
                arrow_menu(ui, &a, |ui| {
                    super::menus::cmd(ui, "&Normal", Command::NormalTool);
                    super::menus::cmd(ui, "Sl&ip", Command::Tool(EditTool::Slip));
                    super::menus::cmd(ui, "Sli&de", Command::Tool(EditTool::Slide));
                    super::menus::cmd(ui, "Time &Stretch/Compress", Command::Tool(EditTool::Stretch));
                });
            }
            "AutoRipple" => {
                let (m, a) = split_button(ui, b.icon, &text, true, st.auto_ripple);
                if m.clicked() {
                    send(ui.ctx(), c);
                }
                arrow_menu(ui, &a, |ui| {
                    super::menus::cmd(ui, "Affected &Tracks", Command::RippleMode(RippleMode::Affected));
                    super::menus::cmd(ui, "Affected Tracks, &Bus Tracks, Markers, and Regions", Command::RippleMode(RippleMode::AffectedMarkers));
                    super::menus::cmd(ui, "&All Tracks, Markers, and Regions", Command::RippleMode(RippleMode::All));
                });
            }
            _ => {
                // Render is greyed with nothing to render, as VEGAS does. Toggles show their state; Color Grading does not.
                let enabled = st.enabled(c) && (c != Command::RenderAs || crate::render::content_end(project) > 0.0);
                let active = st.checked(c) == Some(true) && !matches!(c, Command::Window(_));
                let text = match super::menus::unfinished(c) {
                    Some(why) => why.tip(&text),
                    None => text,
                };
                if tool_button(ui, b.icon, &text, enabled, active).clicked() {
                    send(ui.ctx(), c);
                }
            }
        }
    }
}
