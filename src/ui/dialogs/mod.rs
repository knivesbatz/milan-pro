//! Modal dialogs (UI_SPEC.md §5 Dialogs, §8.3): the shared frame (slate caption with ? and ×, chrome tabs, section
//! title tabs, 75×23 push buttons) and the dialogs themselves.
mod about;
pub mod button_set;
mod archive;
mod audio_transition;
pub mod command_props;
mod customize_keys;
mod customize_toolbar;
mod default_track;
pub mod diagnostics;
mod event_props;
pub mod file_props;
mod goto;
mod media_from_project;
pub mod missing_media;
mod organize_layouts;
mod paste_attrs;
mod paste_repeat;
pub mod prefs;
pub mod prerender;
mod recorded;
pub mod recover;
mod remove_fx;
mod render;
mod save_version;
pub mod sequence;
mod shortcuts;
pub mod slideshow;
pub mod subtitles;
mod take_chooser;
mod template;
mod unfinished;
mod update;
mod welcome;

pub use about::about;
pub use button_set::ButtonSet;
pub use archive::ArchiveDlg;
pub use audio_transition::AudioTransition;
pub use command_props::CommandProps;
pub use customize_keys::CustomizeKeys;
pub use customize_toolbar::CustomizeToolbar;
pub use default_track::DefaultTrackProps;
pub use diagnostics::{CrashChoice, DOCS_URL, DiagAction, Diagnostics, MANUAL_URL, crash_offer_window, issue_url, open_url};
pub use event_props::EventProps;
pub use goto::{TimeEntry, TimeMode};
pub use media_from_project::MediaFromProject;
pub use file_props::FileProps;
pub use missing_media::{DupChoice, DuplicateAsk, MissingMedia, MissingResult};
pub use slideshow::Slideshow;
pub use subtitles::{SubtitleExport, SubtitleImport};
pub use organize_layouts::{Organize, OrganizeLayouts};
pub use paste_attrs::PasteAttrs;
pub use paste_repeat::PasteRepeat;
pub use prefs::PrefsDialog;
pub use prerender::{CleanPrerender, PrerenderDlg};
pub use recorded::{Recorded, RecordedFiles};
pub use recover::{CorruptChoice, Recover, RecoverChoice, corrupt_window};
pub use save_version::SaveVersion;
pub use sequence::SequenceAsk;
pub use remove_fx::RemoveAssignableFx;
pub use render::{RenderDialog, open_file, open_folder};
pub use shortcuts::shortcuts_window;
pub use take_chooser::TakeChooser;
pub use unfinished::unfinished_window;
pub use update::{UpdateResult, rollback_window, update_window};
pub use welcome::{Welcome, WelcomeResult};

use eframe::egui::*;
use super::menus::Why;
use super::theme::{self, Theme};
use super::widgets;

pub const BUTTON: Vec2 = vec2(75.0, 23.0);
/// The title row: a 1 px frame, a 20 px slate caption, 3 px to the body.
const TITLE_H: f32 = 24.0;

/// The title row's height.
pub fn title_h() -> f32 {
    TITLE_H
}

/// Widget style inside dialogs: the design system's controls on the panel silver.
pub fn style(base: &Style, t: &Theme) -> Style {
    let d = t.dialog();
    let mut s = base.clone();
    let v = &mut s.visuals;
    v.dark_mode = t.kind == theme::ThemeKind::Dark;
    v.override_text_color = Some(d.text);
    v.panel_fill = d.page;
    v.window_fill = d.field;
    v.extreme_bg_color = d.field;
    v.window_stroke = Stroke::new(1.0_f32, d.border);
    v.window_corner_radius = CornerRadius::same(t.m.r_ctl);
    v.menu_corner_radius = CornerRadius::same(t.m.r_ctl);
    v.selection.bg_fill = d.accent;
    v.selection.stroke = Stroke::new(1.0_f32, d.accent_text);
    v.text_cursor.stroke = Stroke::new(1.0_f32, d.text);
    let r = CornerRadius::same(t.m.widget_radius);
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
    // Paper fields with an `s5` edge, silver button faces with an `s6` edge, the lit outline on hover, the selection
    // blue; combo popups with an `s6` outline (the paper `window_fill` below for their lists).
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
    let size = fit(ctx, size);
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
    let size = fit(ctx, size);
    with_style(ctx, || area(ctx, id, size, UiKind::Window).show(ctx, |ui| dialog_frame().show(ui, |ui| framed(ui, id, title, size, body)).inner).inner)
}

/// A modeless tool window (Pan/Crop, Track Motion): the title row of `window`, with the body on the panel colour.
/// Its layer is `tool_layer(id)`.
pub fn tool_window<R>(ctx: &Context, id: &str, title: &str, size: Vec2, body: impl FnOnce(&mut Ui) -> R) -> (R, bool) {
    let t = theme::get();
    let frame = Frame::NONE.fill(t.panel_bg).corner_radius(t.m.r_frame);
    area(ctx, id, size, UiKind::Window).show(ctx, |ui| frame.show(ui, |ui| framed(ui, id, title, size, body)).inner).inner
}

pub fn tool_layer(id: &str) -> LayerId {
    LayerId::new(Order::Foreground, Id::new(id))
}

/// R48: a dialog never larger than the window (a small display at 150-200 %); its page scrolls instead (`page`).
fn fit(ctx: &Context, size: Vec2) -> Vec2 {
    let room = ctx.screen_rect().size() - vec2(8.0, 8.0);
    vec2(size.x.min(room.x), size.y.min(room.y)).max(vec2(200.0, 150.0))
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
    // The frame and caption are painted in `framed`.
    Frame::NONE.fill(d.body).corner_radius(t.m.r_frame)
}

/// Title row with "?" and × above `body`; whether × was clicked (`classic_caption`, shared with the tool windows and
/// floating windows).
fn framed<R>(ui: &mut Ui, id: &str, title: &str, size: Vec2, body: impl FnOnce(&mut Ui) -> R) -> (R, bool) {
    ui.set_min_size(size);
    ui.set_max_size(size);
    let top = ui.min_rect().min;
    let th = title_h();
    let close = classic_caption(ui, Rect::from_min_size(top, size), title, id, true);
    ui.allocate_space(vec2(size.x, th));
    let inner = Frame::NONE.inner_margin(Margin { left: 10, right: 10, top: 6, bottom: 10 }).show(ui, |ui| {
        ui.set_width(size.x - 20.0);
        ui.set_min_height(size.y - th - 16.0);
        body(ui)
    });
    (inner.inner, close)
}

/// The window chrome over `win` (UI_SPEC.md §5 Dialogs): a `slate.frame` outline with `r_frame` corners, the 20 px
/// slate caption with the bold title centred between two grooved ridges and, if `help`, a "?" button beside the ×
/// button (16 × 14 chrome, `ink.glyph`). True when × is clicked. The body starts `TITLE_H` below `win.min`.
pub fn classic_caption(ui: &mut Ui, win: Rect, title: &str, id: &str, help: bool) -> bool {
    let t = theme::get();
    let d = t.dialog();
    let p = ui.painter().clone();
    let win = widgets::px(win);
    let rad = t.m.r_frame as f32;
    let cap = Rect::from_min_size(win.min + vec2(1.0, 1.0), vec2(win.width() - 2.0, 20.0));
    // The body: a brushed-panel sweep from `s1` to `s3` (no streaks: text sits on it), rounded at the bottom only.
    let body = win.with_min_y(cap.max.y);
    let s = &t.silver;
    widgets::grad(&p.with_clip_rect(body.intersect(p.clip_rect())), body.with_min_y(body.min.y - rad), rad, &[(0.0, s.s1), (0.3, s.s2), (1.0, s.s3)]);
    // Rounded at the top only: painted taller and clipped at the caption's bottom.
    let pc = p.with_clip_rect(cap.intersect(p.clip_rect()));
    widgets::grad(&pc, cap.with_max_y(cap.max.y + rad), rad - 1.0, &[(0.0, d.caption.0), (1.0, d.caption.1)]);
    pc.rect_filled(Rect::from_min_max(pos2(cap.min.x + rad, cap.min.y), pos2(cap.max.x - rad, cap.min.y + 1.0)), 0.0, t.slate.ridge.0);
    p.rect_filled(Rect::from_min_max(pos2(cap.min.x, cap.max.y), pos2(cap.max.x, cap.max.y + 1.0)), 0.0, t.slate.ridge.1);
    let mut close = false;
    let buttons: &[(&str, bool)] = if help { &[("?", false), ("×", true)] } else { &[("×", true)] };
    let first_button = cap.max.x - 3.0 - 18.0 * buttons.len() as f32;
    // The title centred over the caption, between two grooved ridges (light over dark, twice).
    let g = p.layout_no_wrap(title.to_owned(), t.bold(11.0), d.caption_text);
    let room = Rangef::new(cap.min.x + 8.0, first_button - 6.0);
    let tw = g.size().x.min(room.span());
    let tx = (cap.center().x - tw * 0.5).clamp(room.min, (room.max - tw).max(room.min)).round();
    let cy = cap.center().y.round();
    let ridge = |x0: f32, x1: f32| {
        if x1 - x0 > 8.0 {
            for (dy, c) in [(-3.0, t.slate.ridge.0), (-2.0, t.slate.ridge.1), (1.0, t.slate.ridge.0), (2.0, t.slate.ridge.1)] {
                p.rect_filled(Rect::from_min_max(pos2(x0, cy + dy), pos2(x1, cy + dy + 1.0)), 0.0, c);
            }
        }
    };
    ridge(room.min, tx - 8.0);
    ridge(tx + tw + 8.0, room.max);
    p.with_clip_rect(Rect::from_x_y_ranges(room, cap.y_range()).intersect(p.clip_rect())).galley(pos2(tx, (cy - g.size().y * 0.5).round()), g, d.caption_text);
    for (k, (glyph, is_close)) in buttons.iter().enumerate() {
        let r = Rect::from_min_size(pos2(first_button + 18.0 * k as f32, cap.min.y + 3.0), vec2(16.0, 14.0));
        let hit = ui.interact(r, Id::new(id).with(("cap", k)), Sense::click());
        let pressed = hit.is_pointer_button_down_on();
        let state = if pressed { widgets::Chrome::Pressed } else if hit.hovered() { widgets::Chrome::Hover } else { widgets::Chrome::Raised };
        widgets::chrome(&p, r, t.m.r_small as f32, state);
        let c = r.center() + if pressed { vec2(1.0, 1.0) } else { Vec2::ZERO };
        if *is_close {
            let s = Stroke::new(1.6_f32, t.ink.glyph);
            p.line_segment([c + vec2(-3.0, -3.0), c + vec2(3.0, 3.0)], s);
            p.line_segment([c + vec2(-3.0, 3.0), c + vec2(3.0, -3.0)], s);
        } else {
            p.text(c, Align2::CENTER_CENTER, *glyph, t.bold(11.0), t.ink.glyph);
        }
        close |= *is_close && hit.clicked();
    }
    // A lip under the caption on the body, then the outline over the caption's corners.
    p.rect_filled(Rect::from_min_max(pos2(win.min.x + 1.0, cap.max.y + 1.0), pos2(win.max.x - 1.0, cap.max.y + 2.0)), 0.0, t.silver.s0);
    p.rect_stroke(win, rad, Stroke::new(theme::one(), t.slate.frame), StrokeKind::Inside);
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

/// Tab control (UI_SPEC.md §5 Dialogs: chrome tabs standing on the page); `rows` front row first. The row holding
/// the selected tab is drawn next to the page, as Win32 multi-row tabs do.
pub fn tabs(ui: &mut Ui, rows: &[&[&str]], sel: &mut usize) {
    let t = theme::get();
    let d = t.dialog();
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
    let font = t.font(11.0);
    for (slot, &ri) in order.iter().enumerate() {
        let row = rows[ri];
        let y = rect.min.y + slot as f32 * row_h;
        // The selected tab's label is bold: measured so.
        let widths: Vec<f32> = row
            .iter()
            .enumerate()
            .map(|(k, l)| p.layout_no_wrap(l.to_string(), if firsts[ri] + k == *sel { t.bold(11.0) } else { font.clone() }, d.text).size().x + 12.0)
            .collect();
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
            // Chrome tabs standing on the page; the selected one joins it (its label bold).
            let rp = widgets::px(r);
            widgets::tab_face(&p, rp, selected, resp.hovered(), true, false);
            if selected {
                ui.data_mut(|dd| dd.insert_temp(Id::new("milan_dialog_tab_sel"), rp));
                p.text(r.center(), Align2::CENTER_CENTER, *label, t.bold(11.0), d.text);
            } else {
                p.text(r.center(), Align2::CENTER_CENTER, *label, font.clone(), t.ink.label);
            }
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
    let frame = Frame::NONE.fill(d.page);
    // R48: what is left for the page above the dialog's button row; a dialog squeezed by a small window scrolls its
    // page (at 100 % on a 1080-line display nothing changes).
    let room = (ui.ctx().screen_rect().max.y - ui.cursor().min.y - 16.0 - 56.0).floor().max(120.0);
    let out = frame.inner_margin(Margin::symmetric(10, 8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        if room >= height {
            ui.set_min_height(height);
            ui.set_max_height(height);
            return add(ui);
        }
        ui.set_min_height(room);
        ui.set_max_height(room);
        ScrollArea::vertical().id_salt("dialog_page").max_height(room).auto_shrink([false, false]).show(ui, |ui| {
            ui.set_max_height(height);
            add(ui)
        })
        .inner
    });
    // The page is a panel (an `s6` outline, the `s0` lip inside its top and left) that the selected tab joins: the
    // outline is opened under that tab.
    let r = widgets::px(out.response.rect);
    let p = ui.painter();
    let rad = t.m.r_ctl as f32;
    p.rect_filled(Rect::from_min_max(pos2(r.min.x + 1.0, r.min.y + 1.0), pos2(r.max.x - rad, r.min.y + 2.0)), 0.0, t.silver.s0);
    p.rect_filled(Rect::from_min_max(pos2(r.min.x + 1.0, r.min.y + 1.0), pos2(r.min.x + 2.0, r.max.y - rad)), 0.0, t.silver.s0);
    p.rect_stroke(r, CornerRadius { nw: 0, ne: t.m.r_ctl, sw: t.m.r_ctl, se: t.m.r_ctl }, Stroke::new(theme::one(), t.silver.s6), StrokeKind::Inside);
    if let Some(tab) = ui.data(|dd| dd.get_temp::<Rect>(Id::new("milan_dialog_tab_sel"))).filter(|tab| (tab.max.y - r.min.y).abs() <= 3.0 && tab.min.x >= r.min.x - 1.0) {
        p.rect_filled(Rect::from_min_max(pos2(tab.min.x + 1.0, r.min.y), pos2(tab.max.x - 1.0, r.min.y + 2.0)), 0.0, t.silver.s2);
    }
    out.inner
}

/// A recessed paper well (a list or a text area; UI_SPEC.md §1, paper for typing): the frame the Preferences option
/// lists, the Render Queue and other dialog lists sit in.
pub fn field_box<R>(ui: &mut Ui, margin: i8, add: impl FnOnce(&mut Ui) -> R) -> R {
    let t = theme::get();
    let d = t.dialog();
    let out = Frame::NONE.fill(d.field).corner_radius(t.m.r_ctl).inner_margin(margin + 2).show(ui, add);
    widgets::recess_edge(ui.painter(), out.response.rect, t.m.r_ctl as f32, &t.dialog_metal());
    out.inner
}

/// Group heading: a section title tab (UI_SPEC.md §5) with its groove to the right edge.
pub fn group(ui: &mut Ui, text: &str) {
    let w = (ui.max_rect().max.x - ui.cursor().min.x).max(40.0);
    let (r, _) = ui.allocate_exact_size(vec2(w, 18.0), Sense::hover());
    widgets::section_tab(ui.painter(), r, text);
}

/// 75×23 push button; `default` gets the black ring (OK, Render…).
pub fn button(ui: &mut Ui, label: &str, enabled: bool, default: bool) -> Response {
    classic_button(ui, label, BUTTON, enabled, default)
}

/// Push button (`widgets::classic_push`, UI_SPEC.md §5): raised chrome, pressed inverted, the default one ringed in
/// the lit outline with a bold label, disabled flat with `ink.disabled`.
pub fn classic_button(ui: &mut Ui, label: &str, size: Vec2, enabled: bool, default: bool) -> Response {
    let t = theme::get();
    let d = t.dialog();
    widgets::classic_push(ui, label, size, enabled, default, d.text, d.text_disabled, &t.dialog_metal())
}

/// A push button of `size` (the Render dialog's wide ones).
pub fn sized_button(ui: &mut Ui, label: &str, size: Vec2, enabled: bool) -> Response {
    classic_button(ui, label, size, enabled, false)
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

/// Label | control grid row helper: fixed-width label then the control.
pub fn row<R>(ui: &mut Ui, label: &str, label_w: f32, enabled: bool, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        let t = theme::get();
        let d = t.dialog();
        let (r, _) = ui.allocate_exact_size(vec2(label_w, 21.0), Sense::hover());
        ui.painter().text(r.left_center(), Align2::LEFT_CENTER, label, t.font(11.0), if enabled { d.text } else { d.text_disabled });
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
