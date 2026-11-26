//! Modal dialogs (§5.15): VP16 draws them as native light Win32 property sheets, VP23 dark. This module holds
//! the shared frame (title row with ? and ×, tabs, group headings, 75×23 buttons) and the dialogs themselves.
mod about;
mod archive;
mod audio_transition;
pub mod command_props;
mod customize_keys;
mod customize_toolbar;
mod default_track;
pub mod diagnostics;
mod event_props;
mod goto;
mod media_from_project;
mod organize_layouts;
mod paste_attrs;
mod paste_repeat;
pub mod prefs;
mod recorded;
pub mod recover;
mod remove_fx;
mod render;
mod save_version;
mod shortcuts;
pub mod slideshow;
mod take_chooser;
mod template;
mod unfinished;
mod update;
mod welcome;

pub use about::about;
pub use archive::ArchiveDlg;
pub use audio_transition::AudioTransition;
pub use command_props::CommandProps;
pub use customize_keys::CustomizeKeys;
pub use customize_toolbar::CustomizeToolbar;
pub use default_track::DefaultTrackProps;
pub use diagnostics::{CrashChoice, DiagAction, Diagnostics, crash_offer_window, issue_url, open_url};
pub use event_props::EventProps;
pub use goto::{TimeEntry, TimeMode};
pub use media_from_project::MediaFromProject;
pub use slideshow::Slideshow;
pub use organize_layouts::{Organize, OrganizeLayouts};
pub use paste_attrs::PasteAttrs;
pub use paste_repeat::PasteRepeat;
pub use prefs::PrefsDialog;
pub use recorded::{Recorded, RecordedFiles};
pub use recover::{CorruptChoice, Recover, RecoverChoice, corrupt_window};
pub use save_version::SaveVersion;
pub use remove_fx::RemoveAssignableFx;
pub use render::{RenderDialog, open_file, open_folder};
pub use shortcuts::shortcuts_window;
pub use take_chooser::TakeChooser;
pub use unfinished::unfinished_window;
pub use update::{UpdateResult, rollback_window, update_window};
pub use welcome::{Welcome, WelcomeResult};

use eframe::egui::*;
use super::menus::Why;
use super::theme::{self, Theme, UiLook};
use super::widgets;

pub const BUTTON: Vec2 = vec2(75.0, 23.0);
const TITLE_H: f32 = 31.0;
/// Classic 2006: 3 px 3D frame, an 18 px Windows 2000 caption, 3 px to the body.
const TITLE_H_CLASSIC: f32 = 24.0;

/// The title row's height in the current look.
pub fn title_h() -> f32 {
    if theme::get().classic() { TITLE_H_CLASSIC } else { TITLE_H }
}

/// Widget style inside dialogs: native light controls in VP16, the look's dark controls in VP23.
pub fn style(base: &Style, t: &Theme) -> Style {
    let d = t.dialog();
    let mut s = base.clone();
    let v = &mut s.visuals;
    v.dark_mode = t.look() != UiLook::Vegas16 && t.dark_mode;
    v.override_text_color = Some(d.text);
    v.panel_fill = d.page;
    v.window_fill = d.field;
    v.extreme_bg_color = d.field;
    v.window_stroke = Stroke::new(1.0_f32, d.border);
    v.window_corner_radius = CornerRadius::ZERO;
    v.menu_corner_radius = CornerRadius::ZERO;
    v.selection.bg_fill = d.accent;
    v.selection.stroke = Stroke::new(1.0_f32, d.accent_text);
    v.text_cursor.stroke = Stroke::new(1.0_f32, d.text);
    let r = CornerRadius::same(if t.look() == UiLook::Vegas16 { 4 } else { t.m.widget_radius });
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = d.page;
    w.noninteractive.weak_bg_fill = d.page;
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, d.etch.0);
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, d.text);
    w.inactive.bg_fill = d.field;
    w.inactive.weak_bg_fill = d.button;
    w.inactive.bg_stroke = Stroke::new(1.0_f32, d.button_border);
    w.inactive.fg_stroke = Stroke::new(1.0_f32, d.text);
    w.hovered.bg_fill = d.button_hover;
    w.hovered.weak_bg_fill = d.button_hover;
    w.hovered.bg_stroke = Stroke::new(1.0_f32, d.button_hover_border);
    w.hovered.fg_stroke = Stroke::new(1.0_f32, d.text);
    w.active.bg_fill = d.button_hover;
    w.active.weak_bg_fill = d.button_hover;
    w.active.bg_stroke = Stroke::new(1.0_f32, d.button_hover_border);
    w.active.fg_stroke = Stroke::new(1.0_f32, d.text);
    w.open = w.hovered;
    for st in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        st.corner_radius = r;
        st.expansion = 0.0;
    }
    if t.classic() {
        // Windows 2000 controls: white sunken fields with a grey edge, #D4D0C8 button faces with a dark edge, the
        // #316AC5 highlight; combo popups white with a 1 px dark border.
        let dm = t.dialog_metal();
        w.inactive.bg_stroke = Stroke::new(1.0_f32, dm.shadow);
        w.inactive.weak_bg_fill = d.button;
        w.hovered.bg_fill = d.field;
        w.hovered.weak_bg_fill = d.button;
        w.hovered.bg_stroke = Stroke::new(1.0_f32, dm.dark);
        w.active.bg_fill = d.field;
        w.active.weak_bg_fill = dm.face;
        w.active.bg_stroke = Stroke::new(1.0_f32, dm.dark);
        w.open = w.hovered;
        v.window_fill = d.field;
        v.window_stroke = Stroke::new(1.0_f32, dm.dark);
        v.popup_shadow = Shadow::NONE;
        v.window_shadow = Shadow::NONE;
    }
    s.spacing.item_spacing = vec2(6.0, 6.0);
    s.spacing.interact_size.y = 21.0;
    s.spacing.button_padding = vec2(8.0, 3.0);
    s.spacing.menu_margin = Margin::same(1);
    s
}

/// Run `f` with the dialog style on the context, so windows and combo popups opened inside get it.
pub fn with_style<R>(ctx: &Context, f: impl FnOnce() -> R) -> R {
    let t = theme::get();
    let saved = ctx.style();
    ctx.set_style(style(&saved, &t));
    let r = f();
    ctx.set_style(saved);
    r
}

/// A modal, movable dialog of `size` (title row included) with a native-style title row: title text, a greyed
/// "?" and ×. Returns the body's result and whether × or Esc asked to close.
pub fn modal<R>(ctx: &Context, id: &str, title: &str, size: Vec2, body: impl FnOnce(&mut Ui) -> R) -> (R, bool) {
    with_style(ctx, || {
        let (area, frame) = (area(ctx, id, size, UiKind::Modal), dialog_frame());
        let resp = Modal::new(Id::new(id)).area(area).frame(frame).backdrop_color(Color32::TRANSPARENT).show(ctx, |ui| framed(ui, id, title, size, body));
        let esc = ctx.input(|i| i.key_pressed(Key::Escape));
        let (r, close) = resp.inner;
        (r, close || esc)
    })
}

/// `modal` without the modality: the app stays usable behind it, and Esc does not close it.
pub fn window<R>(ctx: &Context, id: &str, title: &str, size: Vec2, body: impl FnOnce(&mut Ui) -> R) -> (R, bool) {
    with_style(ctx, || area(ctx, id, size, UiKind::Window).show(ctx, |ui| dialog_frame().show(ui, |ui| framed(ui, id, title, size, body)).inner).inner)
}

/// A modeless tool window (Pan/Crop, Track Motion): the native-style title row of `window`, with the body in the
/// look's own dark style in both looks (§5.15.2). Its layer is `tool_layer(id)`.
pub fn tool_window<R>(ctx: &Context, id: &str, title: &str, size: Vec2, body: impl FnOnce(&mut Ui) -> R) -> (R, bool) {
    let t = theme::get();
    let frame = if t.classic() { Frame::NONE.fill(t.panel_bg) } else { Frame::NONE.fill(t.panel_bg).stroke(Stroke::new(1.0_f32, t.dialog().border)) };
    area(ctx, id, size, UiKind::Window).show(ctx, |ui| frame.show(ui, |ui| framed(ui, id, title, size, body)).inner).inner
}

pub fn tool_layer(id: &str) -> LayerId {
    LayerId::new(Order::Foreground, Id::new(id))
}

fn area(ctx: &Context, id: &str, size: Vec2, kind: UiKind) -> Area {
    Area::new(Id::new(id))
        .kind(kind)
        .order(Order::Foreground)
        .interactable(true)
        .movable(true)
        .default_pos(ctx.screen_rect().center() - size * 0.5)
        .constrain(true)
}

fn dialog_frame() -> Frame {
    let t = theme::get();
    let d = t.dialog();
    // Classic 2006 paints its 3D frame in `framed`.
    if t.classic() { Frame::NONE.fill(d.body) } else { Frame::NONE.fill(d.body).stroke(Stroke::new(1.0_f32, d.border)) }
}

/// Title row with "?" and × above `body`; whether × was clicked. Classic 2006: the Windows 2000 window: a raised 3D
/// frame round the whole dialog, the navy → light-blue caption gradient with bold white Tahoma and bevelled ? / ×
/// caption buttons (`classic_caption`, shared with the tool windows and floating windows).
fn framed<R>(ui: &mut Ui, id: &str, title: &str, size: Vec2, body: impl FnOnce(&mut Ui) -> R) -> (R, bool) {
    let t = theme::get();
    let d = t.dialog();
    ui.set_min_size(size);
    ui.set_max_size(size);
    let top = ui.min_rect().min;
    let th = title_h();
    let title_r = Rect::from_min_size(top, vec2(size.x, th));
    let mut close = false;
    if t.classic() {
        close = classic_caption(ui, Rect::from_min_size(top, size), title, id, true);
    } else {
        ui.painter().rect_filled(title_r, 0.0, d.title_bg);
        ui.painter().text(title_r.left_center() + vec2(12.0, 0.0), Align2::LEFT_CENTER, title, t.font(12.0), d.title_text);
        for (k, glyph) in ["?", "×"].into_iter().enumerate() {
            let r = Rect::from_min_size(pos2(title_r.max.x - 46.0 * (2 - k) as f32, title_r.min.y), vec2(46.0, th));
            let hit = ui.interact(r, Id::new(id).with(("cap", k)), Sense::click());
            if k == 1 && hit.hovered() {
                ui.painter().rect_filled(r, 0.0, t.caption_close_hover);
            }
            let ink = if k == 0 { d.text_disabled } else if hit.hovered() { Color32::WHITE } else { d.title_text };
            ui.painter().text(r.center(), Align2::CENTER_CENTER, glyph, t.font(if k == 0 { 12.0 } else { 16.0 }), ink);
            close |= k == 1 && hit.clicked();
        }
    }
    ui.allocate_space(vec2(size.x, th));
    let inner = Frame::NONE.inner_margin(Margin { left: 10, right: 10, top: 6, bottom: 10 }).show(ui, |ui| {
        ui.set_width(size.x - 20.0);
        ui.set_min_height(size.y - th - 16.0);
        body(ui)
    });
    (inner.inner, close)
}

/// The Classic 2006 window chrome over `win`: a raised 3D frame (`dialog_metal`), the Windows 2000 caption
/// gradient inset 3 px with the title in bold white Tahoma and, if `help`, a "?" button beside the × button (both
/// 16×14 bevelled). True when × is clicked. The body starts `TITLE_H_CLASSIC` below `win.min`.
pub fn classic_caption(ui: &mut Ui, win: Rect, title: &str, id: &str, help: bool) -> bool {
    let t = theme::get();
    let d = t.dialog();
    let dm = t.dialog_metal();
    let p = ui.painter().clone();
    let win = widgets::px(win);
    widgets::bevel(&p, win, true, &dm);
    p.rect_stroke(win.shrink(2.0), 0.0, Stroke::new(1.0_f32, dm.face), StrokeKind::Inside);
    let cap = Rect::from_min_size(win.min + vec2(3.0, 3.0), vec2(win.width() - 6.0, 18.0));
    let mut mesh = Mesh::default();
    mesh.colored_vertex(cap.left_top(), d.caption.0);
    mesh.colored_vertex(cap.right_top(), d.caption.1);
    mesh.colored_vertex(cap.right_bottom(), d.caption.1);
    mesh.colored_vertex(cap.left_bottom(), d.caption.0);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    p.add(mesh);
    p.with_clip_rect(cap.shrink2(vec2(2.0, 0.0))).text(pos2(cap.min.x + 5.0, cap.center().y), Align2::LEFT_CENTER, title, t.bold(11.0), d.caption_text);
    let mut close = false;
    let buttons: &[(&str, bool)] = if help { &[("?", false), ("×", true)] } else { &[("×", true)] };
    for (k, (glyph, is_close)) in buttons.iter().enumerate() {
        let r = Rect::from_min_size(pos2(cap.max.x - 2.0 - 16.0 * (buttons.len() - k) as f32 - if *is_close { 0.0 } else { 2.0 }, cap.min.y + 2.0), vec2(16.0, 14.0));
        let hit = ui.interact(r, Id::new(id).with(("cap", k)), Sense::click());
        let pressed = hit.is_pointer_button_down_on();
        p.rect_filled(r, 0.0, dm.face);
        widgets::bevel(&p, r, !pressed, &dm);
        let c = r.center().floor() + if pressed { vec2(1.0, 1.0) } else { Vec2::ZERO };
        if *is_close {
            // A 7×7 pixel × in black.
            let mut m = Mesh::default();
            for i in -3..=3 {
                let i = i as f32;
                m.add_colored_rect(Rect::from_min_size(pos2(c.x + i - 0.5, c.y + i - 0.5), vec2(1.0, 1.0)), t.text);
                m.add_colored_rect(Rect::from_min_size(pos2(c.x + i - 0.5, c.y - i - 0.5), vec2(1.0, 1.0)), t.text);
                m.add_colored_rect(Rect::from_min_size(pos2(c.x + i + 0.5, c.y + i - 0.5), vec2(1.0, 1.0)), t.text);
                m.add_colored_rect(Rect::from_min_size(pos2(c.x + i + 0.5, c.y - i - 0.5), vec2(1.0, 1.0)), t.text);
            }
            p.add(m);
        } else {
            p.text(c, Align2::CENTER_CENTER, *glyph, t.bold(11.0), t.text);
        }
        close |= *is_close && hit.clicked();
    }
    close
}

/// A report with an OK button: `intro`, then `lines` in a scrolling list. True when closed.
pub fn report(ctx: &Context, title: &str, intro: &str, lines: &[String]) -> bool {
    let (ok, close) = modal(ctx, "report", title, vec2(560.0, 340.0), |ui| {
        ui.label(intro);
        ui.add_space(4.0);
        ScrollArea::vertical().max_height(220.0).auto_shrink([false, false]).show(ui, |ui| {
            for l in lines {
                ui.label(l);
            }
        });
        ui.add_space(10.0);
        button_row(ui, &[("OK", true)]).is_some()
    });
    ok || close
}

/// Native tab control; `rows` front row first. The row holding the selected tab is drawn next to the page,
/// as Win32 multi-row tabs do.
pub fn tabs(ui: &mut Ui, rows: &[&[&str]], sel: &mut usize) {
    let t = theme::get();
    let d = t.dialog();
    let font = t.font(12.0);
    let w = ui.available_width();
    let row_h = 18.0;
    let (rect, _) = ui.allocate_exact_size(vec2(w, row_h * rows.len() as f32 + 2.0), Sense::hover());
    let mut firsts = Vec::new();
    let mut acc = 0;
    for r in rows {
        firsts.push(acc);
        acc += r.len();
    }
    let sel_row = (0..rows.len()).find(|&ri| *sel >= firsts[ri] && *sel < firsts[ri] + rows[ri].len()).unwrap_or(0);
    // Back rows above, the selected row at the bottom.
    let mut order: Vec<usize> = (0..rows.len()).filter(|&r| r != sel_row).collect();
    order.push(sel_row);
    let p = ui.painter().clone();
    let classic = t.classic();
    let dm = t.dialog_metal();
    let font = if classic { t.font(11.0) } else { font };
    for (slot, &ri) in order.iter().enumerate() {
        let row = rows[ri];
        let y = rect.min.y + slot as f32 * row_h;
        let widths: Vec<f32> = row.iter().map(|l| p.layout_no_wrap(l.to_string(), font.clone(), d.text).size().x + 12.0).collect();
        // Rows stretch to the full width like Win32 multi-row tabs.
        let extra = ((w - widths.iter().sum::<f32>()) / row.len() as f32).max(0.0);
        let mut x = rect.min.x;
        for (k, label) in row.iter().enumerate() {
            let i = firsts[ri] + k;
            let tw = widths[k] + extra;
            let selected = i == *sel;
            let r = Rect::from_min_size(pos2(x, y), vec2(tw, row_h));
            let r = if selected { r.expand2(vec2(2.0, 2.0)) } else { r };
            let resp = ui.interact(r, ui.id().with(("tab", i)), Sense::click());
            if classic {
                // Windows tab control: a raised tab (white left/top, dark right), the selected one joined to the page.
                let r = widgets::px(r);
                p.rect_filled(r, 0.0, dm.face);
                let line = |a: Pos2, b: Pos2, c: Color32| p.rect_filled(Rect::from_two_pos(a, b), 0.0, c);
                line(r.min, pos2(r.min.x + 1.0, r.max.y), dm.light);
                line(r.min + vec2(1.0, 0.0), pos2(r.max.x - 1.0, r.min.y + 1.0), dm.light);
                line(pos2(r.max.x - 1.0, r.min.y + 1.0), r.max, dm.dark);
                line(pos2(r.max.x - 2.0, r.min.y + 1.0), pos2(r.max.x - 1.0, r.max.y), dm.shadow);
                if !selected {
                    line(pos2(r.min.x, r.max.y - 1.0), r.max, dm.light);
                }
            } else {
                p.rect_filled(r, 0.0, if selected { d.page } else if resp.hovered() { d.button_hover } else { d.body });
                p.rect_stroke(r, 0.0, Stroke::new(1.0_f32, d.tab_border), StrokeKind::Inside);
                if selected {
                    p.hline(r.x_range().shrink(1.0), r.max.y, Stroke::new(2.0_f32, d.page));
                }
            }
            p.text(r.center(), Align2::CENTER_CENTER, *label, font.clone(), d.text);
            if resp.clicked() {
                *sel = i;
            }
            x += tw;
        }
    }
}

/// The page under a tab control: page fill and border; `add` draws the page's controls.
pub fn page<R>(ui: &mut Ui, height: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let t = theme::get();
    let d = t.dialog();
    let frame = if t.classic() { Frame::NONE.fill(d.page) } else { Frame::NONE.fill(d.page).stroke(Stroke::new(1.0_f32, d.tab_border)) };
    let out = frame.inner_margin(Margin::symmetric(10, 8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.set_min_height(height);
        ui.set_max_height(height);
        add(ui)
    });
    if t.classic() {
        // The page is a raised panel (white left/top, dark right/bottom) that the selected tab joins.
        let dm = t.dialog_metal();
        let r = widgets::px(out.response.rect);
        let p = ui.painter();
        p.rect_filled(Rect::from_min_max(r.min, pos2(r.max.x - 1.0, r.min.y + 1.0)), 0.0, dm.light);
        p.rect_filled(Rect::from_min_max(r.min, pos2(r.min.x + 1.0, r.max.y - 1.0)), 0.0, dm.light);
        p.rect_filled(Rect::from_min_max(pos2(r.min.x, r.max.y - 1.0), r.max), 0.0, dm.dark);
        p.rect_filled(Rect::from_min_max(pos2(r.max.x - 1.0, r.min.y), r.max), 0.0, dm.dark);
        p.rect_filled(Rect::from_min_max(pos2(r.min.x + 1.0, r.max.y - 2.0), r.max - vec2(1.0, 1.0)), 0.0, dm.shadow);
        p.rect_filled(Rect::from_min_max(pos2(r.max.x - 2.0, r.min.y + 1.0), r.max - vec2(1.0, 1.0)), 0.0, dm.shadow);
    }
    out.inner
}

/// A sunken white box (a list or a text area) in Classic 2006, the look's flat field box elsewhere: the frame the
/// Preferences option lists, the Render Queue and other dialog lists sit in.
pub fn field_box<R>(ui: &mut Ui, margin: i8, add: impl FnOnce(&mut Ui) -> R) -> R {
    let t = theme::get();
    let d = t.dialog();
    if !t.classic() {
        return Frame::NONE.fill(d.field).stroke(Stroke::new(1.0_f32, d.field_border)).inner_margin(margin).show(ui, add).inner;
    }
    let out = Frame::NONE.fill(d.field).inner_margin(margin + 2).show(ui, add);
    widgets::bevel(ui.painter(), out.response.rect, false, &t.dialog_metal());
    out.inner
}

/// Group heading: text then a 2 px etched line to the right edge (§5.15.1).
pub fn group(ui: &mut Ui, text: &str) {
    let d = theme::get().dialog();
    let r = ui.label(text).rect;
    let y = r.center().y.floor() + 0.5;
    let x0 = r.max.x + 6.0;
    let x1 = ui.max_rect().max.x;
    ui.painter().hline(x0..=x1, y, Stroke::new(1.0_f32, d.etch.0));
    ui.painter().hline(x0..=x1, y + 1.0, Stroke::new(1.0_f32, d.etch.1));
}

/// 75×23 push button; `default` gets the accent border (OK) or, in VP23, the accent fill (Render…).
pub fn button(ui: &mut Ui, label: &str, enabled: bool, default: bool) -> Response {
    let t = theme::get();
    let d = t.dialog();
    if t.classic() {
        return classic_button(ui, label, BUTTON, enabled, default);
    }
    let mut b = Button::new(RichText::new(label).color(if enabled { d.text } else { d.text_disabled })).min_size(BUTTON);
    if default && t.look().v23() {
        b = b.fill(d.accent);
    } else if default && enabled {
        b = b.stroke(Stroke::new(1.0_f32, d.accent));
    }
    ui.add_enabled(enabled, b)
}

/// Windows 2000 push button (`widgets::classic_push` in the property-sheet palette): #D4D0C8 face with the 2 px 3D
/// edge (sunken while pressed), the default button's black ring outside it, the label in Tahoma (greyed with a white
/// emboss when disabled).
pub fn classic_button(ui: &mut Ui, label: &str, size: Vec2, enabled: bool, default: bool) -> Response {
    let t = theme::get();
    let d = t.dialog();
    widgets::classic_push(ui, label, size, enabled, default, d.text, d.text_disabled, &t.dialog_metal())
}

/// A push button of `size` (the Render dialog's wide ones): egui's in the VEGAS looks, `classic_button` here.
pub fn sized_button(ui: &mut Ui, label: &str, size: Vec2, enabled: bool) -> Response {
    if theme::get().classic() {
        return classic_button(ui, label, size, enabled, false);
    }
    ui.add_enabled(enabled, Button::new(label).min_size(size))
}

/// Right-aligned button row (OK, Cancel, Apply…) with 6 px gaps; the index of the clicked one.
pub fn button_row(ui: &mut Ui, buttons: &[(&str, bool)]) -> Option<usize> {
    let mut hit = None;
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (k, (label, enabled)) in buttons.iter().enumerate().rev() {
            if button(ui, label, *enabled, k == 0).clicked() {
                hit = Some(k);
            }
        }
    });
    hit
}

/// 60×23 colour swatch button (the colour shows even when the button is greyed).
pub fn swatch(ui: &mut Ui, color: Color32, enabled: bool) -> Response {
    let t = theme::get();
    let d = t.dialog();
    let (r, resp) = ui.allocate_exact_size(vec2(60.0, 23.0), if enabled { Sense::click() } else { Sense::hover() });
    if t.classic() {
        let r = widgets::px(r);
        ui.painter().rect_filled(r, 0.0, d.button);
        widgets::bevel(ui.painter(), r, true, &t.dialog_metal());
    } else {
        let border = if resp.hovered() && enabled { d.button_hover_border } else { d.button_border };
        ui.painter().rect(r, 3.0, d.button, Stroke::new(1.0_f32, border), StrokeKind::Inside);
    }
    ui.painter().rect_filled(r.shrink2(vec2(7.0, 6.0)), 0.0, color);
    resp
}

/// Label | control grid row helper: fixed-width label then the control.
pub fn row<R>(ui: &mut Ui, label: &str, label_w: f32, enabled: bool, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        let t = theme::get();
        let d = t.dialog();
        let (r, _) = ui.allocate_exact_size(vec2(label_w, 21.0), Sense::hover());
        ui.painter().text(r.left_center(), Align2::LEFT_CENTER, label, t.font(if t.classic() { 11.0 } else { 12.0 }), if enabled { d.text } else { d.text_disabled });
        ui.add_enabled_ui(enabled, add).inner
    })
    .inner
}

/// Greyed combo box showing a fixed value (settings Milan Pro does not have), saying `why` on hover.
pub fn fixed_combo(ui: &mut Ui, id: &str, value: &str, width: f32, why: Why) {
    ui.add_enabled_ui(false, |ui| {
        widgets::combo(ui, id, width, value, |_| {}).response.on_disabled_hover_text(why.text());
    });
}

/// A time field: shows `v` through `show` and sets it from what was typed (through `parse`) when the field loses
/// focus. Unparsable text is dropped; text left as shown keeps the exact value, which may lie between frames.
pub fn time_edit(ui: &mut Ui, id: &str, v: &mut f64, show: impl Fn(f64) -> String, parse: impl Fn(&str) -> Option<f64>) {
    let id = ui.id().with(id);
    let buf_id = id.with("buf");
    let shown = show(*v);
    let mut buf = ui.data_mut(|d| d.get_temp::<String>(buf_id)).unwrap_or_else(|| shown.clone());
    let r = widgets::edit(ui, TextEdit::singleline(&mut buf).id(id).desired_width(110.0));
    if r.lost_focus() {
        if let Some(t) = parse(&buf).filter(|_| buf.trim() != shown) {
            *v = t;
        }
        ui.data_mut(|d| d.remove::<String>(buf_id));
    } else if r.has_focus() {
        ui.data_mut(|d| d.insert_temp(buf_id, buf));
    }
}

/// The note on a Preferences or Project Properties page Milan Pro has no settings for.
pub fn empty_page(ui: &mut Ui, why: Why) {
    ui.add_space(8.0);
    ui.label(format!("Milan Pro has no settings on this page ({}).", why.phrase()));
}
