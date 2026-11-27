//! Toolbar buttons shared by every bar (§5.2, §5.10, dock toolbars) and the main toolbar row.
use eframe::egui::*;
use crate::edit::RippleMode;
use crate::history::History;
use crate::project::Project;
use super::EditTool;
use super::dock::Panel;
use super::icons::{Icon, paint_icon};
use super::keymap::Command;
use super::menus::{NotPlanned, Why, send};
use super::theme;
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

/// The chrome behind a button's 22×22 hit area (UI_SPEC.md §5 Toolbar button): every button is raised chrome; hover
/// lifts it, a press inverts it, an active toggle lights it.
fn paint_state(p: &Painter, hit: Rect, resp: &Response, enabled: bool, active: bool) {
    paint_state_r(p, hit, resp, enabled, active, None);
}

/// Diameter of a round transport button, and the cell it takes (UI_SPEC.md §5 Transport button).
pub const ROUND: f32 = 26.0;
pub const ROUND_PITCH: f32 = 27.0;

/// The width a button for `icon` takes in a row: the round transport ones are larger.
pub fn pitch(icon: Icon) -> f32 {
    if round(icon) { ROUND_PITCH } else { theme::get().m.button_pitch }
}

/// The transport's playback buttons are round (UI_SPEC.md §5 Transport button).
pub fn round(icon: Icon) -> bool {
    matches!(icon, Icon::Record | Icon::Loop | Icon::PlayFromStart | Icon::Play | Icon::Pause | Icon::Stop | Icon::GoToStart | Icon::GoToEnd | Icon::PrevFrame | Icon::NextFrame)
}

fn paint_state_r(p: &Painter, hit: Rect, resp: &Response, enabled: bool, active: bool, radius: Option<f32>) {
    let t = theme::get();
    let state = if active || (enabled && resp.is_pointer_button_down_on()) {
        if active { super::widgets::Chrome::Active } else { super::widgets::Chrome::Pressed }
    } else if enabled && resp.hovered() {
        super::widgets::Chrome::Hover
    } else {
        super::widgets::Chrome::Raised
    };
    match radius {
        Some(r) => super::widgets::chrome(p, hit, r, state),
        None => super::widgets::chrome_button(p, hit, state, &t.metal),
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
    if round(icon) {
        // A large round chrome button with the glyph in the chrome blue (Record keeps its red), centred on the row
        // and allowed to overhang the 22 px cell into the bar's own height.
        let (cell, resp) = ui.allocate_exact_size(vec2(ROUND_PITCH, t.m.button), sense);
        let hit = Rect::from_center_size(cell.center(), Vec2::splat(ROUND));
        paint_state_r(ui.painter(), hit, &resp, clickable, active, Some(ROUND * 0.5));
        let tint = if !lit { Some(t.icon_disabled) } else if icon == Icon::Record { None } else { Some(t.ink.glyph) };
        paint_icon(ui.painter(), Rect::from_center_size(hit.center(), Vec2::splat(t.m.icon)), icon, tint);
        return tip(resp, text);
    }
    let (cell, resp) = ui.allocate_exact_size(vec2(t.m.button_pitch, t.m.button), sense);
    let hit = Rect::from_center_size(cell.center(), Vec2::splat(t.m.button));
    paint_state_r(ui.painter(), hit, &resp, clickable, active, None);
    let tint = (!lit).then_some(t.icon_disabled);
    paint_icon(ui.painter(), Rect::from_center_size(hit.center(), Vec2::splat(t.m.icon)), icon, tint);
    tip(resp, text)
}

/// Icon + ▾ split button, 39 px wide with one shared hover box (§5.2). Returns (icon, arrow) responses.
pub fn split_button(ui: &mut Ui, icon: Icon, text: &str, enabled: bool, active: bool) -> (Response, Response) {
    split(ui, icon, text, enabled, enabled, active)
}

fn split(ui: &mut Ui, icon: Icon, text: &str, enabled: bool, lit: bool, active: bool) -> (Response, Response) {
    let t = theme::get();
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    // 41 px (VEGAS 23, light2026: undo 215 -> redo 256).
    let w = 41.0;
    let (cell, _) = ui.allocate_exact_size(vec2(w, t.m.button), Sense::hover());
    let pad = (t.m.button_pitch - t.m.button) * 0.5;
    let main = Rect::from_min_size(cell.min + vec2(pad, 0.0), Vec2::splat(t.m.button));
    let arrow = Rect::from_min_max(pos2(main.max.x, cell.min.y), pos2(cell.max.x - pad - 2.0, cell.max.y));
    let m = ui.interact(main, ui.id().with((text, "main")), sense);
    let a = ui.interact(arrow, ui.id().with((text, "arrow")), sense);
    let p = ui.painter();
    // One chrome button for the icon, a narrower one for the ▾.
    paint_state(p, main, &m, enabled, active);
    paint_state(p, Rect::from_min_max(pos2(main.max.x - 1.0, main.min.y), pos2(arrow.max.x + 1.0, main.max.y)), &a, enabled, false);
    let tint = (!lit).then_some(t.icon_disabled);
    paint_icon(p, Rect::from_center_size(main.center(), Vec2::splat(t.m.icon)), icon, tint);
    paint_icon(p, Rect::from_center_size(arrow.center(), Vec2::splat(t.m.icon)), Icon::DropDown, tint);
    (tip(m, text), tip(a, text))
}

/// Separator between button groups: an etched line, 16 px tall.
pub fn separator(ui: &mut Ui) {
    let t = theme::get();
    // 8 px (VEGAS 23: light2026 transport separators).
    let w = 8.0;
    let (cell, _) = ui.allocate_exact_size(vec2(w, t.m.button), Sense::hover());
    let x = cell.center().x.floor();
    let p = ui.painter();
    // Etched: shadow line, light line, over the bar's height less 2 px.
    super::widgets::etched_v(p, x - 1.0, Rangef::new(cell.min.y + 1.0, cell.max.y - 1.0), &t.metal);
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
    paint_icon(ui.painter(), arrow, Icon::DropDown, None);
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
    ui.spacing_mut().item_spacing.x = 0.0;
    // Toolbar grip at the left end, then the buttons on a 23 px pitch.
    super::widgets::grip(ui.painter(), ui.max_rect(), &t.metal);
    ui.add_space(10.0);
    draw_buttons(ui, ids, project, history, None);
}

// ---- R15: the buttons a toolbar holds (Options › Customize Toolbar, Customize Timeline Toolbar) ----

/// A button a toolbar can hold: its id in the preferences, glyph, name (the tooltip, with the command's key) and the
/// command it sends; `inert` draws it lit with nothing behind it yet.
pub struct Btn {
    pub id: &'static str,
    pub icon: Icon,
    pub name: &'static str,
    pub cmd: Option<Command>,
    inert: Option<Why>,
}

const fn b(id: &'static str, icon: Icon, name: &'static str, cmd: Command) -> Btn {
    Btn { id, icon, name, cmd: Some(cmd), inert: None }
}

const fn play(id: &'static str, icon: Icon, name: &'static str) -> Btn {
    Btn { id, icon, name, cmd: None, inert: None }
}

/// A separator between button groups.
pub const SEP: &str = "|";

/// Every button, in the order Customize Toolbar lists them.
pub const BUTTONS: &[Btn] = &[
    Btn { id: SEP, icon: Icon::More, name: "Separator", cmd: None, inert: None },
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
    Btn { id: "ZoomTool", icon: Icon::ZoomEdit, name: "Zoom Edit Tool", cmd: None, inert: Some(NotPlanned("R58")) },
    b("Delete", Icon::Delete, "Delete", Command::Delete),
    b("Trim", Icon::Trim, "Trim to Time Selection", Command::Trim),
    b("TrimStart", Icon::TrimStart, "Trim Start", Command::TrimStart),
    b("TrimEnd", Icon::TrimEnd, "Trim End", Command::TrimEnd),
    b("Split", Icon::Split, "Split", Command::Split),
    b("SmartSplit", Icon::SmartSplit, "Smart Split", Command::SmartSplit),
    b("Lock", Icon::Lock, "Lock", Command::EventLock),
    b("InsertMarker", Icon::InsertMarker, "Insert Marker", Command::InsertMarker),
    b("InsertRegion", Icon::InsertRegion, "Insert Region", Command::InsertRegion),
    b("Snapping", Icon::Snapping, "Enable Snapping", Command::Snapping),
    b("AutoCrossfade", Icon::AutoCrossfade, "Automatic Crossfades", Command::AutoCrossfade),
    b("AutoRipple", Icon::AutoRipple, "Auto Ripple", Command::AutoRipple),
    b("LockEnvelopes", Icon::LockEnvelopes, "Lock Envelopes to Events", Command::LockEnvelopes),
    b("IgnoreGrouping", Icon::IgnoreGrouping, "Ignore Event Grouping", Command::IgnoreGrouping),
    b("ColorGrading", Icon::ColorGrading, "Color Grading", Command::Window(Panel::ColorGrading)),
    b("OpenParent", Icon::OpenParent, "Open Parent Timeline", Command::NestedParent),
    b("OpenNested", Icon::OpenNested, "Open Nested Timeline", Command::NestedOpen),
    b("CreateNested", Icon::CreateNested, "Create Nested Timeline", Command::NestedCreate),
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
    /// Its key in `Prefs::toolbars`.
    pub fn key(self) -> String {
        crate::settings::layout_key(if self == Bar::Main { "toolbar" } else { "timeline_toolbar" })
    }

    /// The buttons VEGAS 23's bar has (VEGAS_UI_SPEC.md §5.2, §5.10).
    pub fn defaults(self) -> Vec<&'static str> {
        let mut ids: Vec<&'static str> = match self {
            Bar::Main => vec!["New", "Open", "Save", "Render", "Properties", "Cut", "Copy", "Paste", "Undo", "Redo"],
            Bar::Timeline => TRANSPORT.to_vec(),
        };
        if self == Bar::Main {
            return ids;
        }
        ids.extend([SEP, "NormalTool", "EnvelopeTool", "SelectionTool", "ZoomTool", SEP, "Delete", "Trim", "TrimStart", "TrimEnd", "Split"]);
        ids.extend(["SmartSplit", "Lock", SEP, "InsertMarker", "InsertRegion", SEP, "Snapping", "AutoCrossfade", "AutoRipple", "LockEnvelopes", "IgnoreGrouping"]);
        ids.extend([SEP, "ColorGrading", SEP, "OpenParent", "OpenNested", "CreateNested"]);
        ids
    }

    /// The buttons this bar can hold, the separator first.
    pub fn allowed(self) -> impl Iterator<Item = &'static Btn> {
        BUTTONS.iter().filter(move |b| self == Bar::Timeline || !TRANSPORT.contains(&b.id))
    }

    /// The bar's buttons as saved in `toolbars` (ids it cannot hold dropped), else its defaults.
    pub fn buttons(self, toolbars: &std::collections::BTreeMap<String, Vec<String>>) -> Vec<&'static str> {
        match toolbars.get(&self.key()) {
            Some(ids) => ids.iter().filter_map(|id| self.allowed().find(|b| b.id == id).map(|b| b.id)).collect(),
            None => self.defaults(),
        }
    }
}

pub fn btn(id: &str) -> Option<&'static Btn> {
    BUTTONS.iter().find(|b| b.id == id)
}

/// Draws `ids` left to right; `playback` (the timeline toolbar's) runs the play buttons.
pub fn draw_buttons(ui: &mut Ui, ids: &[&'static str], project: &mut Project, history: &History, mut playback: Option<&mut Playback<'_>>) {
    let st = super::menus::state(ui.ctx());
    let tip = |name: &str, c: Option<Command>| match c.map(super::keymap::shortcut_text).filter(|k| !k.is_empty()) {
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
        // The VEGAS 23 tooltip; Customize Toolbar lists it by its command, Render As.
        let name = if id == "Render" { "Render" } else { b.name };
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
