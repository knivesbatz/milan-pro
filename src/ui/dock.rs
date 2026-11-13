//! Dock area (§4.3–4.5, §5.3): columns of tabbed window groups with tab strips at the bottom, draggable
//! splitters, maximise-in-dock, close (hide), and moving tabs between groups by drag or context menu.
use eframe::egui::*;
use serde::{Deserialize, Serialize};
use super::icons::{Icon, paint_icon};
use super::theme::{self, Theme, UiLook};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Serialize, Deserialize)]
pub enum Panel {
    Explorer,
    ProjectMedia,
    MediaGenerators,
    Transitions,
    VideoFx,
    ColorGrading,
    Properties,
    VideoPreview,
    Trimmer,
    MasterBus,
    /// View › Window › Compositors (not in a default layout, as in VEGAS).
    Compositors,
    /// R8b: View › Window › Mixing Console (not in a default layout).
    MixingConsole,
}

impl Panel {
    pub const ALL: [Panel; 12] = [
        Panel::Explorer,
        Panel::ProjectMedia,
        Panel::MediaGenerators,
        Panel::Transitions,
        Panel::VideoFx,
        Panel::ColorGrading,
        Panel::Properties,
        Panel::VideoPreview,
        Panel::Trimmer,
        Panel::MasterBus,
        Panel::Compositors,
        Panel::MixingConsole,
    ];

    pub fn title(self, look: UiLook) -> &'static str {
        match self {
            Panel::Explorer => "Explorer",
            Panel::ProjectMedia => "Project Media",
            Panel::MediaGenerators if look == UiLook::Vegas23 => "Media Generator",
            Panel::MediaGenerators => "Media Generators",
            Panel::Transitions => "Transitions",
            Panel::VideoFx => "Video FX",
            Panel::ColorGrading => "Color Grading",
            Panel::Properties => "Properties",
            Panel::VideoPreview => "Video Preview",
            Panel::Trimmer => "Trimmer",
            Panel::MasterBus => "Master Bus",
            Panel::Compositors => "Compositors",
            Panel::MixingConsole => "Mixing Console",
        }
    }

    /// Color Grading is a VP23 window.
    pub fn in_look(self, look: UiLook) -> bool {
        self != Panel::ColorGrading || look == UiLook::Vegas23
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub tabs: Vec<Panel>,
    pub active: usize,
    /// Column width in px; the first column takes whatever the others leave.
    pub width: f32,
}

pub struct Dock {
    pub groups: Vec<Group>,
    /// Dock height incl. tab strips; None = the look's default.
    pub height: Option<f32>,
    pub track_list_w: f32,
    /// Group maximised within the dock (□ on the active tab, or double-click).
    pub maximized: Option<usize>,
    dragging: Option<Panel>,
}

/// A saved window layout (View › Window Layouts): the dock's columns, its height and the track list width.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    pub name: String,
    pub groups: Vec<Group>,
    pub height: Option<f32>,
    pub track_list_w: f32,
}

const MIN_W: f32 = 80.0;

impl Dock {
    /// Default layout at 1920×1080 (§4.1 / §4.2): [plug-in & media group] [Video Preview, Trimmer] [Master Bus].
    pub fn new(t: &Theme) -> Self {
        use Panel::*;
        let (left, active, mid_w) = match t.look() {
            UiLook::Vegas16 => (vec![ProjectMedia, Explorer, Transitions, VideoFx, MediaGenerators], 0, 835.0),
            // Hub tabs dropped (no online service); Project Media active on first launch [I].
            UiLook::Vegas23 => (vec![Explorer, ProjectMedia, MediaGenerators, Transitions, VideoFx, ColorGrading], 1, 750.0),
        };
        Self {
            groups: vec![
                Group { tabs: left, active, width: 0.0 },
                Group { tabs: vec![VideoPreview, Trimmer], active: 0, width: mid_w },
                Group { tabs: vec![MasterBus], active: 0, width: t.m.master_bus_w },
            ],
            height: None,
            track_list_w: t.m.track_list_w,
            maximized: None,
            dragging: None,
        }
    }

    pub fn layout(&self, name: &str) -> Layout {
        Layout { name: name.to_string(), groups: self.groups.clone(), height: self.height, track_list_w: self.track_list_w }
    }

    /// Take on `l`, made safe for `t`'s look: each panel once and only if the look has it, no empty groups, widths
    /// of at least `MIN_W` (but the first), a valid active tab. A layout with nothing left gives the default dock.
    pub fn apply(&mut self, l: &Layout, t: &Theme) {
        let mut seen: Vec<Panel> = Vec::new();
        let groups: Vec<Group> = l
            .groups
            .iter()
            .filter_map(|g| {
                let active = g.tabs.get(g.active).copied();
                // The first column takes what the others leave: its width is unused, kept as saved (a true round trip).
                let width = match seen.is_empty() {
                    true if g.width.is_finite() => g.width,
                    true => 0.0,
                    false => g.width.max(MIN_W),
                };
                let mut tabs: Vec<Panel> = Vec::new();
                for &p in &g.tabs {
                    if p.in_look(t.look()) && !seen.contains(&p) {
                        seen.push(p);
                        tabs.push(p);
                    }
                }
                let active = active.and_then(|a| tabs.iter().position(|&p| p == a)).unwrap_or(0);
                (!tabs.is_empty()).then(|| Group { tabs, active, width })
            })
            .collect();
        *self = Dock::new(t);
        if !groups.is_empty() {
            self.groups = groups;
            self.height = l.height.filter(|h| h.is_finite());
            if l.track_list_w.is_finite() && l.track_list_w >= 0.0 {
                self.track_list_w = l.track_list_w;
            }
        }
    }

    fn find(&self, p: Panel) -> Option<(usize, usize)> {
        self.groups.iter().enumerate().find_map(|(g, gr)| gr.tabs.iter().position(|&x| x == p).map(|i| (g, i)))
    }

    pub fn is_open(&self, p: Panel) -> bool {
        self.find(p).is_some()
    }

    /// Show and activate `p` (into the first group if hidden), or hide it if it is the active tab already.
    pub fn toggle(&mut self, p: Panel) {
        match self.find(p) {
            Some((g, i)) if self.groups[g].active == i => self.close(g, i),
            Some((g, i)) => self.groups[g].active = i,
            None => self.open(p),
        }
    }

    /// Show and activate `p` (into the first group if hidden).
    pub fn open(&mut self, p: Panel) {
        if let Some((g, i)) = self.find(p) {
            self.groups[g].active = i;
            return;
        }
        if self.groups.is_empty() {
            self.groups.push(Group { tabs: vec![], active: 0, width: 0.0 });
        }
        let g = &mut self.groups[0];
        g.tabs.push(p);
        g.active = g.tabs.len() - 1;
    }

    /// Closing only hides the window; View › Window reopens it (§4.3).
    fn close(&mut self, g: usize, i: usize) {
        self.take(g, i);
    }

    /// Remove tab `i` of group `g`, dropping the group if it empties. Returns the panel.
    fn take(&mut self, g: usize, i: usize) -> Panel {
        let gr = &mut self.groups[g];
        let p = gr.tabs.remove(i);
        if gr.active > i || gr.active >= gr.tabs.len() {
            gr.active = gr.active.saturating_sub(1);
        }
        if gr.tabs.is_empty() {
            self.groups.remove(g);
            self.maximized = None;
        }
        p
    }

    /// Move `p` into group `to` (`None` = a new column right of its current one).
    fn move_to(&mut self, p: Panel, to: Option<usize>) {
        let Some((g, i)) = self.find(p) else { return };
        if to == Some(g) {
            return;
        }
        let target = to.map(|to| self.groups.get(to).map(|gr| gr.tabs[0]));
        let moved = self.take(g, i);
        match target {
            Some(Some(anchor)) => {
                if let Some((tg, _)) = self.find(anchor) {
                    let gr = &mut self.groups[tg];
                    gr.tabs.push(moved);
                    gr.active = gr.tabs.len() - 1;
                }
            }
            _ => {
                let at = (g + 1).min(self.groups.len());
                self.groups.insert(at, Group { tabs: vec![moved], active: 0, width: 320.0 });
            }
        }
        self.maximized = None;
    }

    /// Paint and run the dock in `rect`. `content(panel, toolbar_ui, body_ui)` fills the active window of each group.
    pub fn show(&mut self, ui: &mut Ui, rect: Rect, mut content: impl FnMut(Panel, &mut Ui, &mut Ui)) {
        let t = theme::get();
        let sw = t.m.splitter_w;
        if self.groups.is_empty() {
            return;
        }
        let visible: Vec<usize> = match self.maximized {
            Some(g) if g < self.groups.len() => vec![g],
            _ => (0..self.groups.len()).collect(),
        };
        // Column x ranges: later columns keep their width, the first takes the rest. A dock too narrow for those
        // widths shows the later columns narrower alike (their widths are kept), so the first still gets MIN_W.
        let later: f32 = visible[1..].iter().map(|&g| self.groups[g].width).sum();
        let room = rect.width() - MIN_W - sw * visible[1..].len() as f32;
        let scale = if later > room && later > 0.0 { (room / later).max(0.0) } else { 1.0 };
        let rest: f32 = visible[1..].iter().map(|&g| self.groups[g].width * scale + sw).sum();
        let mut x = rect.min.x;
        let mut cols = Vec::with_capacity(visible.len());
        for (k, &g) in visible.iter().enumerate() {
            let w = if k == 0 { (rect.width() - rest).max(MIN_W) } else { self.groups[g].width * scale };
            let w = w.min(rect.max.x - x).max(0.0).round();
            cols.push((g, Rect::from_min_max(pos2(x, rect.min.y), pos2(x + w, rect.max.y))));
            x += w + sw;
        }
        for &(g, col) in &cols {
            self.group_ui(ui, &t, g, col, &mut content);
        }
        // Splitters between columns (§4.5): no grip glyph, resize cursor.
        for k in 0..cols.len().saturating_sub(1) {
            let (g, col) = cols[k];
            let split = Rect::from_min_max(pos2(col.max.x, rect.min.y), pos2(col.max.x + sw, rect.max.y));
            let resp = ui.interact(split, ui.id().with(("dock_split", k)), Sense::drag());
            if resp.hovered() || resp.dragged() {
                ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
            }
            let dx = resp.drag_delta().x;
            if dx != 0.0 {
                let next = cols[k + 1].0;
                let room = |w: f32| (w - MIN_W).max(0.0);
                if k == 0 {
                    let dx = dx.clamp(-room(col.width()), room(self.groups[next].width));
                    self.groups[next].width -= dx;
                } else {
                    let dx = dx.clamp(-room(self.groups[g].width), room(self.groups[next].width));
                    self.groups[g].width += dx;
                    self.groups[next].width -= dx;
                }
            }
        }
        self.drop_target(ui, &t, &cols);
    }

    /// Dragging a tab outlines the group under the pointer; releasing over another group moves the tab there.
    fn drop_target(&mut self, ui: &mut Ui, t: &Theme, cols: &[(usize, Rect)]) {
        let Some(p) = self.dragging else { return };
        let (pos, released) = ui.input(|i| (i.pointer.latest_pos(), i.pointer.any_released() || !i.pointer.any_down()));
        let target = pos.and_then(|pos| cols.iter().find(|(_, r)| r.contains(pos)));
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        if let Some(&(g, r)) = target {
            ui.painter().rect_stroke(r.shrink(1.0), 0.0, Stroke::new(2.0_f32, t.text_dim), StrokeKind::Inside);
            if released {
                self.move_to(p, Some(g));
            }
        }
        if let Some(pos) = pos {
            let layer = ui.ctx().layer_painter(LayerId::new(Order::Tooltip, Id::new("dock_drag_label")));
            layer.text(pos + vec2(12.0, 4.0), Align2::LEFT_TOP, p.title(t.look()), t.font(t.font.dock_tab), t.text_bright);
        }
        if released {
            self.dragging = None;
        }
    }

    fn group_ui(&mut self, ui: &mut Ui, t: &Theme, g: usize, col: Rect, content: &mut impl FnMut(Panel, &mut Ui, &mut Ui)) {
        let v16 = t.look() == UiLook::Vegas16;
        let p = ui.painter().clone();
        let strip = Rect::from_min_max(pos2(col.min.x, col.max.y - t.m.tab_strip_h), col.max);
        let mut top = col.min.y;
        if v16 {
            // 2 px top bevel (§5.3).
            hline(&p, col.x_range(), top, t.dock_bevel_1);
            hline(&p, col.x_range(), top + 1.0, t.dock_bevel_2);
            top += 2.0;
        }
        let bar = Rect::from_min_max(pos2(col.min.x, top), pos2(col.max.x, top + t.m.dock_toolbar_h));
        p.rect_filled(bar, 0.0, t.toolbar_bg);
        hline(&p, col.x_range(), bar.max.y, if v16 { t.dock_divider } else { t.frame });
        let body = Rect::from_min_max(pos2(col.min.x, bar.max.y + 1.0), pos2(col.max.x, strip.min.y));
        p.rect_filled(body, 0.0, t.panel_bg);

        let gr = &self.groups[g];
        let panel = gr.tabs[gr.active.min(gr.tabs.len() - 1)];
        let mut bar_ui = ui.new_child(
            UiBuilder::new().id_salt(("dock_bar", panel)).max_rect(bar.shrink2(vec2(2.0, 0.0))).layout(eframe::egui::Layout::left_to_right(Align::Center)),
        );
        bar_ui.set_clip_rect(bar.intersect(ui.clip_rect()));
        bar_ui.spacing_mut().item_spacing.x = 0.0;
        let mut body_ui = ui.new_child(UiBuilder::new().id_salt(("dock_body", panel)).max_rect(body).layout(eframe::egui::Layout::top_down(Align::Min)));
        body_ui.set_clip_rect(body.intersect(ui.clip_rect()));
        content(panel, &mut bar_ui, &mut body_ui);
        self.tab_strip(ui, t, g, strip);
    }

    fn tab_strip(&mut self, ui: &mut Ui, t: &Theme, g: usize, strip: Rect) {
        let v16 = t.look() == UiLook::Vegas16;
        let look = t.look();
        let p = ui.painter().with_clip_rect(strip.intersect(ui.clip_rect()));
        let b = &t.bevel;
        let single = self.groups[g].tabs.len() == 1;
        if v16 {
            // Right of the last tab: two edge rows over the frame colour (#202020 for a single-window group).
            p.rect_filled(strip, 0.0, if single { t.tab_strip_single_bg } else { t.frame });
            hline(&p, strip.x_range(), strip.min.y, b.strip_1);
            hline(&p, strip.x_range(), strip.min.y + 1.0, b.strip_2);
        } else {
            p.rect_filled(strip, 0.0, t.tab_strip_bg);
        }
        let mut x = strip.min.x;
        let mut clicked = None;
        let mut action: Option<TabAction> = None;
        let tabs = self.groups[g].tabs.clone();
        let others: Vec<(usize, Panel)> =
            self.groups.iter().enumerate().filter(|&(k, _)| k != g).map(|(k, gr)| (k, gr.tabs[gr.active.min(gr.tabs.len() - 1)])).collect();
        let maximized = self.maximized == Some(g);
        for (i, &panel) in tabs.iter().enumerate() {
            let active = i == self.groups[g].active;
            let font = if active && !v16 { t.bold(t.font.dock_tab) } else { t.font(t.font.dock_tab) };
            let label = panel.title(look);
            let galley = p.layout_no_wrap(label.to_owned(), font, t.tab_text);
            // VP16 tabs reserve room for □ × even when inactive: 8 + label + 38 (measured [L]: label + 44 at GDI widths).
            let w = if v16 { 8.0 + galley.size().x + 38.0 } else { 9.0 + galley.size().x + 28.0 }.round();
            let r = Rect::from_min_size(pos2(x, strip.min.y), vec2(w, strip.height()));
            if r.min.x >= strip.max.x {
                break; // shortcut: overflow tabs are cut off (no ◂ ▸ scroller yet)
            }
            let resp = ui.interact(r.intersect(strip), ui.id().with(("dock_tab", panel)), Sense::click_and_drag());
            let hovered = resp.hovered();
            if v16 {
                if active {
                    p.rect_filled(r, 0.0, t.tab_active_bg);
                    vline(&p, r.min.x, strip.y_range(), t.tab_separator);
                    vline(&p, r.min.x + 1.0, strip.y_range(), b.tab_side_hi);
                    vline(&p, r.max.x - 1.0, strip.y_range(), t.line_dark);
                } else {
                    p.rect_filled(r, 0.0, t.tab_strip_bg);
                    hline(&p, r.x_range(), r.min.y, t.tab_separator);
                    hline(&p, r.x_range(), r.min.y + 1.0, b.tab_top_2);
                    vline(&p, r.max.x, strip.y_range(), t.tab_separator);
                }
            }
            let color = if hovered && !active {
                t.tab_text_hover
            } else if active {
                t.tab_text_active
            } else {
                t.tab_text
            };
            let tx = r.min.x + if v16 { 8.0 } else { 9.0 };
            // VP23 labels sit 2 px above centre (measured [M]).
            let ty = (r.center().y - galley.size().y * 0.5).round() - if v16 { 0.0 } else { 2.0 };
            if !v16 && active {
                // 1 px line on the content-facing (top) edge, across the label plus its 9 px padding.
                let line = vec2(galley.size().x + 18.0, t.m.tab_active_line_w);
                p.rect_filled(Rect::from_min_size(r.min, line), 0.0, t.tab_active_line);
            }
            p.galley(pos2(tx, ty), galley, color);
            if v16 && active {
                // □ maximise in dock and × close, each in a 13 px cell (§4.4).
                for (k, icon) in [Icon::DockMaximize, Icon::DockClose].into_iter().enumerate() {
                    let cell = Rect::from_min_size(pos2(r.max.x - 34.0 + 17.0 * k as f32, r.min.y), vec2(13.0, r.height()));
                    let br = ui.interact(cell, ui.id().with(("dock_tab_btn", panel, k)), Sense::click());
                    let tint = if br.hovered() { t.text_bright } else { t.tab_text };
                    paint_icon(&p, Rect::from_center_size(cell.center(), vec2(16.0, 16.0)), icon, look, Some(tint));
                    let tip = if k == 0 { "Maximize in Dock" } else { "Close" };
                    if br.on_hover_text(tip).clicked() {
                        action = Some(if k == 0 { TabAction::Maximize } else { TabAction::Close(i) });
                    }
                }
            }
            if resp.double_clicked() {
                action = Some(TabAction::Maximize);
            } else if resp.clicked() {
                clicked = Some(i);
            }
            if resp.drag_started_by(PointerButton::Primary) {
                self.dragging = Some(panel);
            }
            super::menus::context(&resp, |ui| {
                for &(k, anchor) in &others {
                    if ui.button(format!("Dock with {}", anchor.title(look))).clicked() {
                        action = Some(TabAction::Move(panel, Some(k)));
                        ui.close_menu();
                    }
                }
                if tabs.len() > 1 && ui.button("Dock in New Column").clicked() {
                    action = Some(TabAction::Move(panel, None));
                    ui.close_menu();
                }
                ui.separator();
                if ui.button(if maximized { "Restore in Dock" } else { "Maximize in Dock" }).clicked() {
                    action = Some(TabAction::Maximize);
                    ui.close_menu();
                }
                if ui.button("Close").clicked() {
                    action = Some(TabAction::Close(i));
                    ui.close_menu();
                }
            });
            x += w + if v16 && !active { 1.0 } else { 0.0 };
        }
        if let Some(i) = clicked {
            self.groups[g].active = i;
        }
        match action {
            Some(TabAction::Maximize) => self.maximized = if self.maximized == Some(g) { None } else { Some(g) },
            Some(TabAction::Close(i)) => self.close(g, i),
            Some(TabAction::Move(p, to)) => self.move_to(p, to),
            None => {}
        }
    }
}

enum TabAction {
    Maximize,
    Close(usize),
    Move(Panel, Option<usize>),
}

fn hline(p: &Painter, x: Rangef, y: f32, c: Color32) {
    p.rect_filled(Rect::from_x_y_ranges(x, Rangef::new(y, y + 1.0)), 0.0, c);
}

fn vline(p: &Painter, x: f32, y: Rangef, c: Color32) {
    p.rect_filled(Rect::from_x_y_ranges(Rangef::new(x, x + 1.0), y), 0.0, c);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::LookChoice;

    #[test]
    fn hide_show_and_move_tabs() {
        let mut d = Dock::new(&Theme::new(LookChoice::VEGAS16));
        assert_eq!(d.groups.len(), 3);
        assert_eq!(d.groups[0].tabs[d.groups[0].active], Panel::ProjectMedia);
        // Closing the only tab of a group removes the group; opening brings it back into the first group.
        d.toggle(Panel::MasterBus);
        assert!(!d.is_open(Panel::MasterBus) && d.groups.len() == 2);
        d.toggle(Panel::MasterBus);
        assert_eq!(d.find(Panel::MasterBus), Some((0, 5)));
        // Moving keeps every panel exactly once and activates it in the target group.
        d.move_to(Panel::Trimmer, Some(0));
        assert_eq!(d.find(Panel::Trimmer), Some((0, 6)));
        assert_eq!(d.groups[0].active, 6);
        d.move_to(Panel::Explorer, None);
        assert_eq!(d.groups[1].tabs, vec![Panel::Explorer]);
        let n: usize = d.groups.iter().map(|g| g.tabs.len()).sum();
        assert_eq!(n, 8); // Color Grading is VP23-only; Properties opens on demand
    }

    #[test]
    fn layout_roundtrips_and_apply_sanitises() {
        use Panel::*;
        let (t16, t23) = (Theme::new(LookChoice::VEGAS16), Theme::new(LookChoice::VEGAS23));
        let mut d = Dock::new(&t23);
        d.move_to(Trimmer, None);
        d.height = Some(400.0);
        let l = d.layout("Mine");
        assert_eq!(serde_json::from_str::<Layout>(&serde_json::to_string(&l).unwrap()).unwrap(), l);
        let mut e = Dock::new(&t23);
        e.apply(&l, &t23);
        assert_eq!((e.groups, e.height, e.track_list_w), (d.groups.clone(), Some(400.0), d.track_list_w));

        let g = |tabs: Vec<Panel>, active, width| Group { tabs, active, width };
        let bad = Layout {
            name: String::new(),
            groups: vec![
                g(vec![ColorGrading, Explorer, Explorer], 0, 0.0),
                g(vec![], 0, 300.0),
                g(vec![Explorer, ColorGrading], 1, 300.0),
                g(vec![Trimmer, VideoPreview], 9, 10.0),
            ],
            height: Some(f32::NAN),
            track_list_w: f32::INFINITY,
        };
        let mut v16 = Dock::new(&t16);
        v16.apply(&bad, &t16);
        assert_eq!(v16.groups, vec![g(vec![Explorer], 0, 0.0), g(vec![Trimmer, VideoPreview], 0, MIN_W)]);
        assert_eq!((v16.height, v16.track_list_w), (None, t16.m.track_list_w));
        // VP23 keeps Color Grading, once, as the active tab it was.
        let mut v23 = Dock::new(&t23);
        v23.apply(&bad, &t23);
        assert_eq!(v23.groups[0], g(vec![ColorGrading, Explorer], 0, 0.0));
        assert_eq!(v23.groups.len(), 2);
        // Nothing usable left: the default dock.
        let mut z = Dock::new(&t16);
        z.move_to(Explorer, None);
        z.apply(&Layout { groups: vec![g(vec![ColorGrading], 0, 300.0)], ..bad }, &t16);
        assert_eq!(z.groups, Dock::new(&t16).groups);
    }

    /// Simulated pointer input: a click on the tab strip switches tabs, dragging a splitter resizes columns.
    #[test]
    fn click_tab_and_drag_splitter() {
        let ctx = Context::default();
        let t = theme::get();
        let mut d = Dock::new(&t);
        let rect = Rect::from_min_size(pos2(3.0, 0.0), vec2(1914.0, 596.0));
        let frame = |events: Vec<Event>, d: &mut Dock| {
            let raw = RawInput { events, screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1920.0, 700.0))), ..Default::default() };
            let _ = ctx.run(raw, |ctx| {
                CentralPanel::default().frame(Frame::NONE).show(ctx, |ui| d.show(ui, rect, |_, _, _| {}));
            });
        };
        let press = |p: Pos2, pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        let tab = pos2(400.0, rect.max.y - 8.0);
        frame(vec![Event::PointerMoved(tab)], &mut d);
        frame(vec![press(tab, true)], &mut d);
        frame(vec![press(tab, false)], &mut d);
        assert_ne!(d.groups[0].active, 0, "click on a tab activates it");

        let first_split = rect.min.x + rect.width() - 125.0 - 7.0 - 835.0 - 7.0 + 3.0;
        let mut y = pos2(first_split, 300.0);
        frame(vec![Event::PointerMoved(y)], &mut d);
        frame(vec![press(y, true)], &mut d);
        for _ in 0..4 {
            y.x -= 10.0;
            frame(vec![Event::PointerMoved(y)], &mut d);
        }
        frame(vec![press(y, false)], &mut d);
        assert!(d.groups[1].width > 835.0 && d.groups[1].width <= 875.0, "width {}", d.groups[1].width);
    }
}
