//! Dock areas (§4.3–4.5, §5.3): columns of tabbed window groups with tab strips at the bottom (or the top), draggable
//! splitters, maximise-in-dock, close (hide), moving tabs between groups by drag or context menu, the tab overflow
//! scroller, and the areas themselves: VP16 has one (above the timeline, or below it), VP23 four (top, bottom, left,
//! right), each one minimised on its own (F11, Alt+F11...). R13: the built-in window layouts.
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

/// A docking area around the timeline (§4.3). Old layouts have every group in the top one.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Area {
    #[default]
    Top,
    Bottom,
    Left,
    Right,
}

impl Area {
    pub const ALL: [Area; 4] = [Area::Top, Area::Bottom, Area::Left, Area::Right];

    /// Top and bottom span the window's width; left and right sit beside the timeline.
    pub fn across(self) -> bool {
        matches!(self, Area::Top | Area::Bottom)
    }

    fn name(self) -> &'static str {
        ["Top", "Bottom", "Left", "Right"][self as usize]
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub tabs: Vec<Panel>,
    pub active: usize,
    /// Column width in px; the first column of an area takes whatever the others leave.
    pub width: f32,
    #[serde(default)]
    pub area: Area,
}

/// The built-in window layouts (View › Window Layouts).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Builtin {
    Default,
    /// VP16: VEGAS Pro 14 Default Layout, the timeline above the dock.
    Classic,
    AudioMixing,
    ColorCorrection,
    /// VP23: Source Window Layout.
    Source,
    /// VP23: Timeline Top Layout.
    TimelineTop,
}

/// The layout last loaded or saved: Reload Selected Layout goes back to it, Save Layout saves over a user slot.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayoutRef {
    Builtin(Builtin),
    User(usize),
}

pub struct Dock {
    pub groups: Vec<Group>,
    /// Top area height incl. tab strips; None = the look's default.
    pub height: Option<f32>,
    /// Bottom area height, left and right area widths; None = the default.
    pub bottom_h: Option<f32>,
    pub left_w: Option<f32>,
    pub right_w: Option<f32>,
    /// Areas minimised (F11, Alt+F11, Ctrl+Alt+F11, Ctrl+Shift+F11): not shown, their windows kept.
    pub hidden: Vec<Area>,
    pub track_list_w: f32,
    /// The track list width Collapse Track Headers goes back to.
    track_restore: Option<f32>,
    /// Maximize Timeline: the minimised areas and track list width before it.
    timeline_max: Option<(Vec<Area>, f32)>,
    /// Group maximised within its area (□ on the active tab, or double-click).
    pub maximized: Option<usize>,
    /// The window Next/Previous Window (F6, Ctrl+Tab) reached or last clicked; None = the timeline.
    pub focus: Option<Panel>,
    /// Preferences › Display › Position tabs at top of docked windows.
    pub tabs_top: bool,
    /// Where each area was drawn last frame (auto-hide keeps an area open while the pointer is over it).
    shown: [Rect; 4],
    dragging: Option<Panel>,
    /// The tabs drawn whole last frame.
    #[cfg(test)]
    drawn: Vec<Panel>,
}

/// A saved window layout (View › Window Layouts): the groups, the areas' sizes and which are minimised, and the
/// track list width.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    pub name: String,
    pub groups: Vec<Group>,
    pub height: Option<f32>,
    pub track_list_w: f32,
    #[serde(default)]
    pub bottom_h: Option<f32>,
    #[serde(default)]
    pub left_w: Option<f32>,
    #[serde(default)]
    pub right_w: Option<f32>,
    #[serde(default)]
    pub hidden: Vec<Area>,
}

const MIN_W: f32 = 80.0;
/// Smallest area a splitter leaves.
pub const MIN_AREA: f32 = 60.0;
/// An auto-hidden area keeps this edge to hover.
const PEEK: f32 = 6.0;
/// The ◂ ▸ tab scroller cells (§4.4).
const ARROW_W: f32 = 13.0;

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
                Group { tabs: left, active, width: 0.0, area: Area::Top },
                Group { tabs: vec![VideoPreview, Trimmer], active: 0, width: mid_w, area: Area::Top },
                Group { tabs: vec![MasterBus], active: 0, width: t.m.master_bus_w, area: Area::Top },
            ],
            height: None,
            bottom_h: None,
            left_w: None,
            right_w: None,
            hidden: Vec::new(),
            track_list_w: t.m.track_list_w,
            track_restore: None,
            timeline_max: None,
            maximized: None,
            focus: None,
            tabs_top: false,
            shown: [Rect::NOTHING; 4],
            dragging: None,
            #[cfg(test)]
            drawn: Vec::new(),
        }
    }

    /// A built-in layout. Classic, Audio Mixing and Color Correction are decoded from VEGAS 15's
    /// `Standard Layouts\*.VegasWindowLayout` (window ids, tab order, active tab and column widths at 1920×1080,
    /// §4.1); Source Window and Timeline Top (VP23) are built from the VP23 default [I].
    pub fn builtin(b: Builtin, t: &Theme) -> Self {
        use Panel::*;
        let mut d = Dock::new(t);
        let left = d.groups[0].clone();
        let col = |tabs: Vec<Panel>, width: f32| Group { tabs, active: 0, width, area: Area::Top };
        match b {
            Builtin::Default => {}
            Builtin::Classic => {
                d.groups = vec![left, col(vec![Trimmer], 610.0), col(vec![VideoPreview], 613.0), col(vec![MasterBus], 125.0)];
                d.groups.iter_mut().for_each(|g| g.area = Area::Bottom);
                d.bottom_h = Some(449.0);
            }
            Builtin::AudioMixing => {
                // The Mixing Console first and active, then the default group; no Trimmer or Master Bus.
                let tabs = [vec![MixingConsole], left.tabs].concat();
                d.groups = vec![col(tabs, 0.0), col(vec![VideoPreview], 511.0)];
                d.height = Some(438.0);
            }
            Builtin::ColorCorrection => {
                // VEGAS puts Video Scopes (539 px) between the group and Video Preview: R16b adds it here. VP23 brings
                // its Color Grading window to the front [I].
                let front = if t.look() == UiLook::Vegas23 { ColorGrading } else { ProjectMedia };
                let active = left.tabs.iter().position(|&p| p == front).unwrap_or(0);
                d.groups = vec![Group { active, ..left }, col(vec![VideoPreview], 539.0), col(vec![MasterBus], 127.0)];
                d.height = Some(438.0);
            }
            Builtin::Source => {
                // Source (Trimmer) and program (Video Preview) monitors side by side.
                let w = d.groups[1].width;
                d.groups[1].tabs = vec![VideoPreview];
                d.groups.insert(1, col(vec![Trimmer], w));
            }
            Builtin::TimelineTop => d.groups.iter_mut().for_each(|g| g.area = Area::Bottom),
        }
        d
    }

    pub fn layout(&self, name: &str) -> Layout {
        Layout {
            name: name.to_string(),
            groups: self.groups.clone(),
            height: self.height,
            track_list_w: self.track_list_w,
            bottom_h: self.bottom_h,
            left_w: self.left_w,
            right_w: self.right_w,
            hidden: self.hidden.clone(),
        }
    }

    /// Take on `l`, made safe for `t`'s look: each panel once and only if the look has it, no empty groups, widths
    /// of at least `MIN_W` (but each area's first), a valid active tab, and in VP16 one area (above the timeline or
    /// below it). A layout with nothing left gives the default dock.
    pub fn apply(&mut self, l: &Layout, t: &Theme) {
        let mut seen: Vec<Panel> = Vec::new();
        let mut areas: Vec<Area> = Vec::new();
        let groups: Vec<Group> = l
            .groups
            .iter()
            .filter_map(|g| {
                let active = g.tabs.get(g.active).copied();
                let mut tabs: Vec<Panel> = Vec::new();
                for &p in &g.tabs {
                    if p.in_look(t.look()) && !seen.contains(&p) {
                        seen.push(p);
                        tabs.push(p);
                    }
                }
                let active = active.and_then(|a| tabs.iter().position(|&p| p == a)).unwrap_or(0);
                // VP16 has one docking area: where the first group is (the top, unless it is the bottom).
                let area = match (t.look(), areas.first()) {
                    (UiLook::Vegas16, Some(&a)) => a,
                    (UiLook::Vegas16, None) if g.area != Area::Bottom => Area::Top,
                    _ => g.area,
                };
                if tabs.is_empty() {
                    return None;
                }
                // An area's first column takes what the others leave: its width is unused, kept as saved (a round trip).
                let width = match areas.contains(&area) {
                    false if g.width.is_finite() => g.width,
                    false => 0.0,
                    true => g.width.max(MIN_W),
                };
                areas.push(area);
                Some(Group { tabs, active, width, area })
            })
            .collect();
        *self = Dock::new(t);
        if !groups.is_empty() {
            self.groups = groups;
            let ok = |s: Option<f32>| s.filter(|s| s.is_finite());
            (self.height, self.bottom_h, self.left_w, self.right_w) = (ok(l.height), ok(l.bottom_h), ok(l.left_w), ok(l.right_w));
            for &a in &l.hidden {
                if !self.hidden.contains(&a) {
                    self.hidden.push(a);
                }
            }
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

    /// Show and activate `p` (into the first group if hidden), or hide it if it is the active tab of a shown area.
    pub fn toggle(&mut self, p: Panel) {
        match self.find(p) {
            Some((g, i)) if self.groups[g].active == i && !self.hidden.contains(&self.groups[g].area) => self.close(g, i),
            _ => self.open(p),
        }
    }

    /// Show and activate `p` (into the first group if hidden), restoring its area if it is minimised.
    pub fn open(&mut self, p: Panel) {
        let g = match self.find(p) {
            Some((g, i)) => {
                self.groups[g].active = i;
                g
            }
            None => {
                if self.groups.is_empty() {
                    self.groups.push(Group { tabs: vec![], active: 0, width: 0.0, area: Area::Top });
                }
                let g = &mut self.groups[0];
                g.tabs.push(p);
                g.active = g.tabs.len() - 1;
                0
            }
        };
        let area = self.groups[g].area;
        self.hidden.retain(|&a| a != area);
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
        let area = self.groups[g].area;
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
                self.groups.insert(at, Group { tabs: vec![moved], active: 0, width: 320.0, area });
            }
        }
        self.maximized = None;
    }

    /// Move `p` into docking area `area` (VP23), as a new last column there; the area is shown.
    pub fn move_to_area(&mut self, p: Panel, area: Area) {
        let Some((g, i)) = self.find(p) else { return };
        let moved = self.take(g, i);
        self.groups.push(Group { tabs: vec![moved], active: 0, width: 320.0, area });
        self.hidden.retain(|&a| a != area);
        self.maximized = None;
    }

    fn used(&self) -> Vec<Area> {
        Area::ALL.into_iter().filter(|a| self.groups.iter().any(|g| g.area == *a)).collect()
    }

    /// F11: minimise every docking area that has windows, or restore them all when none is shown.
    pub fn toggle_all(&mut self) {
        let used = self.used();
        if used.iter().any(|a| !self.hidden.contains(a)) {
            let add: Vec<Area> = used.into_iter().filter(|a| !self.hidden.contains(a)).collect();
            self.hidden.extend(add);
        } else {
            self.hidden.retain(|a| !used.contains(a));
        }
    }

    /// Alt+F11 / Ctrl+Alt+F11 / Ctrl+Shift+F11 (VP23): minimise or restore one area.
    pub fn toggle_area(&mut self, a: Area) {
        match self.hidden.iter().position(|&h| h == a) {
            Some(k) => {
                self.hidden.remove(k);
            }
            None => self.hidden.push(a),
        }
    }

    /// Collapse the track list to its colour strips, or expand it again (§4.5).
    pub fn toggle_track_list(&mut self, t: &Theme) {
        let strip = t.m.color_strip_w;
        if self.track_list_w > strip + 1.0 {
            self.track_restore = Some(self.track_list_w);
            self.track_list_w = strip;
        } else {
            self.track_list_w = self.track_restore.take().unwrap_or(t.m.track_list_w);
        }
    }

    /// Maximize Timeline: minimise the docking areas and collapse the track list; again puts both back.
    pub fn toggle_maximize(&mut self, t: &Theme) {
        match self.timeline_max.take() {
            Some((hidden, w)) => (self.hidden, self.track_list_w) = (hidden, w),
            None => {
                self.timeline_max = Some((self.hidden.clone(), self.track_list_w));
                self.hidden = Area::ALL.to_vec();
                if self.track_list_w > t.m.color_strip_w + 1.0 {
                    self.toggle_track_list(t);
                }
            }
        }
    }

    /// VP16 Display preference "Display timeline at bottom of main window": the dock area is above the timeline.
    pub fn timeline_top(&self) -> bool {
        self.groups.first().is_some_and(|g| g.area == Area::Bottom)
    }

    pub fn set_timeline_top(&mut self, top: bool) {
        let area = if top { Area::Bottom } else { Area::Top };
        self.groups.iter_mut().for_each(|g| g.area = area);
        self.hidden.clear();
    }

    /// Next Window (F6, Ctrl+Tab) or Previous Window: the timeline, then every window of the shown areas in turn;
    /// a window reached comes to the front of its group.
    pub fn cycle(&mut self, back: bool) {
        let mut ring: Vec<Option<Panel>> = vec![None];
        for g in self.groups.iter().filter(|g| !self.hidden.contains(&g.area)) {
            ring.extend(g.tabs.iter().copied().map(Some));
        }
        let at = ring.iter().position(|&f| f == self.focus).unwrap_or(0);
        let n = ring.len();
        self.focus = ring[if back { (at + n - 1) % n } else { (at + 1) % n }];
        if let Some((g, i)) = self.focus.and_then(|p| self.find(p)) {
            self.groups[g].active = i;
            if self.maximized.is_some_and(|m| m != g) {
                self.maximized = None;
            }
        }
    }

    pub fn size(&self, a: Area) -> Option<f32> {
        [self.height, self.bottom_h, self.left_w, self.right_w][a as usize]
    }

    pub fn size_mut(&mut self, a: Area) -> &mut Option<f32> {
        match a {
            Area::Top => &mut self.height,
            Area::Bottom => &mut self.bottom_h,
            Area::Left => &mut self.left_w,
            Area::Right => &mut self.right_w,
        }
    }

    /// Each area's size this frame (`chrome::areas`): 0 when it has no windows or is minimised. With auto-hide on
    /// (Preferences › Display) an area is a thin edge until the pointer is over it, a tab is dragged or `keep_open`
    /// (a menu is open).
    pub fn area_sizes(&self, t: &Theme, full: Rect, auto_hide: bool, hover: Option<Pos2>, keep_open: bool) -> [f32; 4] {
        let used = self.used();
        Area::ALL.map(|a| {
            if self.hidden.contains(&a) || !used.contains(&a) {
                return 0.0;
            }
            let open = !auto_hide || keep_open || self.dragging.is_some() || hover.is_some_and(|p| self.shown[a as usize].expand(4.0).contains(p));
            if !open {
                return PEEK;
            }
            let def = if a.across() { t.m.dock_h.min(full.height() * t.m.dock_frac) } else { 320.0 };
            self.size(a).filter(|s| s.is_finite()).unwrap_or(def).max(MIN_AREA)
        })
    }

    /// Paint and run the docking areas in `rects` (by `Area`; empty or thin ones draw nothing). `content(panel,
    /// toolbar_ui, body_ui)` fills the active window of each group.
    pub fn show(&mut self, ui: &mut Ui, rects: [Rect; 4], mut content: impl FnMut(Panel, &mut Ui, &mut Ui)) {
        let t = theme::get();
        let sw = t.m.splitter_w;
        self.shown = rects;
        #[cfg(test)]
        self.drawn.clear();
        let min_h = t.m.tab_strip_h + t.m.dock_toolbar_h + 4.0;
        let mut cols: Vec<(usize, Rect)> = Vec::new();
        for (k, area) in Area::ALL.into_iter().enumerate() {
            let rect = rects[k];
            if !rect.is_positive() || rect.height() < min_h || rect.width() < MIN_W {
                continue;
            }
            let in_area: Vec<usize> = (0..self.groups.len()).filter(|&g| self.groups[g].area == area).collect();
            let visible = match self.maximized {
                Some(g) if in_area.contains(&g) => vec![g],
                _ => in_area,
            };
            if visible.is_empty() {
                continue;
            }
            // Column x ranges: later columns keep their width, the first takes the rest. An area too narrow for those
            // widths shows the later columns narrower alike (their widths are kept), so the first still gets MIN_W.
            // shortcut: left and right areas lay their groups out as columns too; stacked groups come with R14's
            // drag docking.
            let later: f32 = visible[1..].iter().map(|&g| self.groups[g].width).sum();
            let room = rect.width() - MIN_W - sw * visible[1..].len() as f32;
            let scale = if later > room && later > 0.0 { (room / later).max(0.0) } else { 1.0 };
            let rest: f32 = visible[1..].iter().map(|&g| self.groups[g].width * scale + sw).sum();
            let mut x = rect.min.x;
            let start = cols.len();
            for (j, &g) in visible.iter().enumerate() {
                let w = if j == 0 { (rect.width() - rest).max(MIN_W) } else { self.groups[g].width * scale };
                let w = w.min(rect.max.x - x).max(0.0).round();
                cols.push((g, Rect::from_min_max(pos2(x, rect.min.y), pos2(x + w, rect.max.y))));
                x += w + sw;
            }
            // Splitters between columns (§4.5): no grip glyph, resize cursor.
            let area_cols = cols[start..].to_vec();
            for j in 0..area_cols.len().saturating_sub(1) {
                let (g, col) = area_cols[j];
                let split = Rect::from_min_max(pos2(col.max.x, rect.min.y), pos2(col.max.x + sw, rect.max.y));
                let resp = ui.interact(split, ui.id().with(("dock_split", k, j)), Sense::drag());
                if resp.hovered() || resp.dragged() {
                    ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
                }
                let dx = resp.drag_delta().x;
                if dx != 0.0 {
                    let next = area_cols[j + 1].0;
                    let room = |w: f32| (w - MIN_W).max(0.0);
                    if j == 0 {
                        let dx = dx.clamp(-room(col.width()), room(self.groups[next].width));
                        self.groups[next].width -= dx;
                    } else {
                        let dx = dx.clamp(-room(self.groups[g].width), room(self.groups[next].width));
                        self.groups[g].width += dx;
                        self.groups[next].width -= dx;
                    }
                }
            }
        }
        // Tab actions that change the groups wait until every group is drawn (indices stay valid meanwhile).
        let mut action = None;
        for &(g, col) in &cols {
            action = self.group_ui(ui, &t, g, col, &mut content).or(action);
        }
        // A press in a window gives it the focus (Next/Previous Window go on from there).
        if let Some(p) = ui.input(|i| i.pointer.press_origin().filter(|_| i.pointer.any_pressed())) {
            if let Some(&(g, _)) = cols.iter().find(|(_, r)| r.contains(p)) {
                let gr = &self.groups[g];
                self.focus = Some(gr.tabs[gr.active.min(gr.tabs.len() - 1)]);
            }
        }
        match action {
            Some(TabAction::Maximize(g)) => self.maximized = if self.maximized == Some(g) { None } else { Some(g) },
            Some(TabAction::Close(g, i)) => self.close(g, i),
            Some(TabAction::Move(p, to)) => self.move_to(p, to),
            Some(TabAction::ToArea(p, a)) => self.move_to_area(p, a),
            None => {}
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

    fn group_ui(&mut self, ui: &mut Ui, t: &Theme, g: usize, col: Rect, content: &mut impl FnMut(Panel, &mut Ui, &mut Ui)) -> Option<TabAction> {
        let v16 = t.look() == UiLook::Vegas16;
        let p = ui.painter().clone();
        let h = t.m.tab_strip_h;
        // Tabs at the bottom (§4.3), or at the top (Preferences › Display).
        let (strip, mut top, bottom) = if self.tabs_top {
            (Rect::from_min_size(col.min, vec2(col.width(), h)), col.min.y + h, col.max.y)
        } else {
            (Rect::from_min_max(pos2(col.min.x, col.max.y - h), col.max), col.min.y, col.max.y - h)
        };
        if v16 {
            // 2 px top bevel (§5.3).
            hline(&p, col.x_range(), top, t.dock_bevel_1);
            hline(&p, col.x_range(), top + 1.0, t.dock_bevel_2);
            top += 2.0;
        }
        let bar = Rect::from_min_max(pos2(col.min.x, top), pos2(col.max.x, top + t.m.dock_toolbar_h));
        p.rect_filled(bar, 0.0, t.toolbar_bg);
        hline(&p, col.x_range(), bar.max.y, if v16 { t.dock_divider } else { t.frame });
        let body = Rect::from_min_max(pos2(col.min.x, bar.max.y + 1.0), pos2(col.max.x, bottom));
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
        self.tab_strip(ui, t, g, strip)
    }

    fn tab_strip(&mut self, ui: &mut Ui, t: &Theme, g: usize, strip: Rect) -> Option<TabAction> {
        let v16 = t.look() == UiLook::Vegas16;
        let look = t.look();
        let clip = strip.intersect(ui.clip_rect());
        let p = ui.painter().with_clip_rect(clip);
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
        let mut clicked = None;
        let mut action: Option<TabAction> = None;
        let tabs = self.groups[g].tabs.clone();
        let active = self.groups[g].active;
        let area = self.groups[g].area;
        let others: Vec<(usize, Panel)> =
            self.groups.iter().enumerate().filter(|&(k, _)| k != g).map(|(k, gr)| (k, gr.tabs[gr.active.min(gr.tabs.len() - 1)])).collect();
        let maximized = self.maximized == Some(g);
        let galleys: Vec<_> = tabs
            .iter()
            .enumerate()
            .map(|(i, &panel)| {
                let font = if i == active && !v16 { t.bold(t.font.dock_tab) } else { t.font(t.font.dock_tab) };
                p.layout_no_wrap(panel.title(look).to_owned(), font, t.tab_text)
            })
            .collect();
        // VP16 tabs reserve room for □ × even when inactive: 8 + label + 38 (measured [L]: label + 44 at GDI widths);
        // an inactive VP16 tab is followed by a 1 px separator.
        let widths: Vec<f32> = galleys
            .iter()
            .enumerate()
            .map(|(i, gl)| if v16 { 8.0 + gl.size().x + 38.0 + if i == active { 0.0 } else { 1.0 } } else { 9.0 + gl.size().x + 28.0 }.round())
            .collect();
        // Overflow (§4.4): ◂ ▸ at the right end scroll the tabs; a tab activated elsewhere is scrolled into view.
        let overflow = widths.iter().sum::<f32>() > strip.width();
        let avail = strip.width() - if overflow { 2.0 * ARROW_W } else { 0.0 };
        let mem = ui.id().with(("dock_tab_first", tabs[0]));
        let (mut first, last_active) = ui.data(|d| d.get_temp::<(usize, usize)>(mem)).unwrap_or((0, usize::MAX));
        if !overflow {
            first = 0;
        } else if active != last_active {
            first = scroll_to(&widths, avail, first, active);
        }
        first = first.min(tabs.len() - 1);
        let tabs_clip = Rect::from_min_max(strip.min, pos2(strip.min.x + avail, strip.max.y)).intersect(clip);
        let p = p.with_clip_rect(tabs_clip);
        let mut x = strip.min.x;
        for i in first..tab_end(&widths, first, avail) {
            let panel = tabs[i];
            #[cfg(test)]
            if x + widths[i] <= strip.min.x + avail {
                self.drawn.push(panel);
            }
            let is_active = i == active;
            let galley = galleys[i].clone();
            let w = widths[i] - if v16 && !is_active { 1.0 } else { 0.0 };
            let r = Rect::from_min_size(pos2(x, strip.min.y), vec2(w, strip.height()));
            let resp = ui.interact(r.intersect(tabs_clip), ui.id().with(("dock_tab", panel)), Sense::click_and_drag());
            let hovered = resp.hovered();
            if v16 {
                if is_active {
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
            let color = if hovered && !is_active {
                t.tab_text_hover
            } else if is_active {
                t.tab_text_active
            } else {
                t.tab_text
            };
            let tx = r.min.x + if v16 { 8.0 } else { 9.0 };
            // VP23 labels sit 2 px off centre, away from the content (measured [M]).
            let shift = if v16 { 0.0 } else if self.tabs_top { -2.0 } else { 2.0 };
            let ty = (r.center().y - galley.size().y * 0.5).round() - shift;
            if !v16 && is_active {
                // 1 px line on the content-facing edge, across the label plus its 9 px padding.
                let line = vec2(galley.size().x + 18.0, t.m.tab_active_line_w);
                let y = if self.tabs_top { r.max.y - line.y } else { r.min.y };
                p.rect_filled(Rect::from_min_size(pos2(r.min.x, y), line), 0.0, t.tab_active_line);
            }
            p.galley(pos2(tx, ty), galley, color);
            if v16 && is_active {
                // □ maximise in dock and × close, each in a 13 px cell (§4.4).
                for (k, icon) in [Icon::DockMaximize, Icon::DockClose].into_iter().enumerate() {
                    let cell = Rect::from_min_size(pos2(r.max.x - 34.0 + 17.0 * k as f32, r.min.y), vec2(13.0, r.height()));
                    let br = ui.interact(cell.intersect(tabs_clip), ui.id().with(("dock_tab_btn", panel, k)), Sense::click());
                    let tint = if br.hovered() { t.text_bright } else { t.tab_text };
                    paint_icon(&p, Rect::from_center_size(cell.center(), vec2(16.0, 16.0)), icon, look, Some(tint));
                    let tip = if k == 0 { "Maximize in Dock" } else { "Close" };
                    if br.on_hover_text(tip).clicked() {
                        action = Some(if k == 0 { TabAction::Maximize(g) } else { TabAction::Close(g, i) });
                    }
                }
            }
            if resp.double_clicked() {
                action = Some(TabAction::Maximize(g));
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
                // VP23 docks on all four sides (§4.3); dragging to an edge comes with R14.
                if look == UiLook::Vegas23 {
                    for a in Area::ALL.into_iter().filter(|&a| a != area) {
                        if ui.button(format!("Dock in {} Docking Area", a.name())).clicked() {
                            action = Some(TabAction::ToArea(panel, a));
                            ui.close_menu();
                        }
                    }
                }
                ui.separator();
                if ui.button(if maximized { "Restore in Dock" } else { "Maximize in Dock" }).clicked() {
                    action = Some(TabAction::Maximize(g));
                    ui.close_menu();
                }
                if ui.button("Close").clicked() {
                    action = Some(TabAction::Close(g, i));
                    ui.close_menu();
                }
            });
            x += widths[i];
        }
        if overflow {
            let more = widths[first..].iter().sum::<f32>() > avail;
            for (k, (enabled, left)) in [(first > 0, true), (more, false)].into_iter().enumerate() {
                let cell = Rect::from_min_size(pos2(strip.max.x - ARROW_W * (2 - k) as f32, strip.min.y), vec2(ARROW_W, strip.height()));
                let resp = ui.interact(cell.intersect(clip), ui.id().with(("dock_tab_scroll", tabs[0], k)), Sense::click());
                let ink = if !enabled { t.text_dim.gamma_multiply(0.5) } else if resp.hovered() { t.text_bright } else { t.tab_text };
                let c = cell.center();
                let s = if left { -1.0 } else { 1.0 };
                let tri = vec![c + vec2(3.0 * s, 0.0), c + vec2(-2.0 * s, -4.0), c + vec2(-2.0 * s, 4.0)];
                ui.painter().with_clip_rect(clip).add(Shape::convex_polygon(tri, ink, Stroke::NONE));
                if enabled && resp.on_hover_text(if left { "Scroll tabs left" } else { "Scroll tabs right" }).clicked() {
                    first = if left { first - 1 } else { first + 1 };
                }
            }
        }
        if let Some(i) = clicked {
            self.groups[g].active = i;
            self.focus = Some(tabs[i]);
        }
        let now = clicked.unwrap_or(active);
        ui.data_mut(|d| d.insert_temp(mem, (first, now)));
        action
    }
}

/// The tabs drawn from `first` in `avail` px end before this index (a last tab that does not fit is cut).
fn tab_end(widths: &[f32], first: usize, avail: f32) -> usize {
    let (mut x, mut i) = (0.0, first);
    while i < widths.len() && x < avail {
        x += widths[i];
        i += 1;
    }
    i
}

/// The first tab to draw so that tab `active` shows whole, starting from `first` when it already does.
fn scroll_to(widths: &[f32], avail: f32, first: usize, active: usize) -> usize {
    if active < first {
        return active;
    }
    let mut f = first;
    while f < active && widths[f..=active].iter().sum::<f32>() > avail {
        f += 1;
    }
    f
}

enum TabAction {
    Maximize(usize),
    Close(usize, usize),
    Move(Panel, Option<usize>),
    ToArea(Panel, Area),
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
        d.move_to_area(Explorer, Area::Right);
        d.height = Some(400.0);
        d.right_w = Some(250.0);
        d.toggle_area(Area::Right);
        let l = d.layout("Mine");
        assert_eq!(serde_json::from_str::<Layout>(&serde_json::to_string(&l).unwrap()).unwrap(), l);
        let mut e = Dock::new(&t23);
        e.apply(&l, &t23);
        assert_eq!((e.groups, e.height, e.right_w, e.hidden, e.track_list_w), (d.groups.clone(), Some(400.0), Some(250.0), vec![Area::Right], d.track_list_w));

        let g = |tabs: Vec<Panel>, active, width| Group { tabs, active, width, area: Area::Top };
        let bad = Layout {
            name: String::new(),
            groups: vec![
                g(vec![ColorGrading, Explorer, Explorer], 0, 0.0),
                g(vec![], 0, 300.0),
                g(vec![Explorer, ColorGrading], 1, 300.0),
                Group { area: Area::Right, ..g(vec![Trimmer, VideoPreview], 9, 10.0) },
            ],
            height: Some(f32::NAN),
            track_list_w: f32::INFINITY,
            bottom_h: Some(f32::INFINITY),
            left_w: None,
            right_w: None,
            hidden: vec![Area::Left, Area::Left],
        };
        let mut v16 = Dock::new(&t16);
        v16.apply(&bad, &t16);
        // VP16 has one docking area: the right-hand group joins the top one.
        assert_eq!(v16.groups, vec![g(vec![Explorer], 0, 0.0), g(vec![Trimmer, VideoPreview], 0, MIN_W)]);
        assert_eq!((v16.height, v16.bottom_h, v16.track_list_w, v16.hidden), (None, None, t16.m.track_list_w, vec![Area::Left]));
        // VP23 keeps Color Grading, once, as the active tab it was, and the right area (its first column there).
        let mut v23 = Dock::new(&t23);
        v23.apply(&bad, &t23);
        assert_eq!(v23.groups[0], g(vec![ColorGrading, Explorer], 0, 0.0));
        assert_eq!(v23.groups[1], Group { area: Area::Right, ..g(vec![Trimmer, VideoPreview], 0, 10.0) });
        // Nothing usable left: the default dock.
        let mut z = Dock::new(&t16);
        z.move_to(Explorer, None);
        z.apply(&Layout { groups: vec![g(vec![ColorGrading], 0, 300.0)], ..bad.clone() }, &t16);
        assert_eq!(z.groups, Dock::new(&t16).groups);
        // A VP16 layout below the timeline stays there; a layout from before R13 (no areas) is all at the top.
        let mut c = Dock::new(&t16);
        c.apply(&Dock::builtin(Builtin::Classic, &t16).layout("C"), &t16);
        assert!(c.timeline_top() && c.groups.iter().all(|g| g.area == Area::Bottom));
        let old: Layout = serde_json::from_str(r#"{"name":"Old","groups":[{"tabs":["Trimmer"],"active":0,"width":0}],"height":300,"track_list_w":200}"#).unwrap();
        assert_eq!((old.groups[0].area, old.bottom_h, old.hidden.len()), (Area::Top, None, 0));
    }

    /// R13: each built-in layout opens the windows VEGAS's layout files show (Video Scopes waits on R16b).
    #[test]
    fn builtin_layouts_open_the_vegas_panels() {
        use Panel::*;
        let (t16, t23) = (Theme::new(LookChoice::VEGAS16), Theme::new(LookChoice::VEGAS23));
        let cols = |d: &Dock| d.groups.iter().map(|g| (g.tabs.clone(), g.tabs[g.active], g.area)).collect::<Vec<_>>();
        let left16 = vec![ProjectMedia, Explorer, Transitions, VideoFx, MediaGenerators];
        let left23 = vec![Explorer, ProjectMedia, MediaGenerators, Transitions, VideoFx, ColorGrading];
        use Area::{Bottom, Top};
        let table: Vec<(Builtin, &Theme, Vec<(Vec<Panel>, Panel, Area)>, Option<f32>)> = vec![
            (Builtin::Classic, &t16, vec![(left16.clone(), ProjectMedia, Bottom), (vec![Trimmer], Trimmer, Bottom), (vec![VideoPreview], VideoPreview, Bottom), (vec![MasterBus], MasterBus, Bottom)], Some(449.0)),
            (Builtin::AudioMixing, &t16, vec![([vec![MixingConsole], left16.clone()].concat(), MixingConsole, Top), (vec![VideoPreview], VideoPreview, Top)], Some(438.0)),
            (Builtin::ColorCorrection, &t16, vec![(left16.clone(), ProjectMedia, Top), (vec![VideoPreview], VideoPreview, Top), (vec![MasterBus], MasterBus, Top)], Some(438.0)),
            (Builtin::ColorCorrection, &t23, vec![(left23.clone(), ColorGrading, Top), (vec![VideoPreview], VideoPreview, Top), (vec![MasterBus], MasterBus, Top)], Some(438.0)),
            (Builtin::Source, &t23, vec![(left23.clone(), ProjectMedia, Top), (vec![Trimmer], Trimmer, Top), (vec![VideoPreview], VideoPreview, Top), (vec![MasterBus], MasterBus, Top)], None),
            (Builtin::TimelineTop, &t23, vec![(left23.clone(), ProjectMedia, Bottom), (vec![VideoPreview, Trimmer], VideoPreview, Bottom), (vec![MasterBus], MasterBus, Bottom)], None),
        ];
        for (b, t, want, h) in table {
            let d = Dock::builtin(b, t);
            assert_eq!(cols(&d), want, "{b:?} {:?}", t.look());
            assert_eq!(if d.timeline_top() { d.bottom_h } else { d.height }, h, "{b:?}");
        }
        assert_eq!(Dock::builtin(Builtin::AudioMixing, &t16).groups[1].width, 511.0);
        assert_eq!(Dock::builtin(Builtin::Default, &t23).groups, Dock::new(&t23).groups);
    }

    /// F11 twice gives the layout back; one area alone; Maximize Timeline and Collapse Track Headers toggle back.
    #[test]
    fn dock_commands_toggle_back() {
        let t = Theme::new(LookChoice::VEGAS23);
        let full = Rect::from_min_size(Pos2::ZERO, vec2(1920.0, 1080.0));
        let mut d = Dock::builtin(Builtin::TimelineTop, &t);
        d.move_to_area(Panel::MasterBus, Area::Right);
        let before = (d.layout(""), d.area_sizes(&t, full, false, None, false));
        assert!(before.1[0] == 0.0 && before.1[1] > 0.0 && before.1[3] > 0.0);
        d.toggle_all();
        assert_eq!(d.area_sizes(&t, full, false, None, false), [0.0; 4]);
        d.toggle_all();
        assert_eq!((d.layout(""), d.area_sizes(&t, full, false, None, false)), before);
        d.toggle_area(Area::Right);
        assert_eq!(d.area_sizes(&t, full, false, None, false)[3], 0.0);
        d.toggle_all(); // the bottom area was still shown: everything goes
        d.toggle_all();
        assert_eq!(d.layout(""), before.0);
        // Opening a window of a minimised area shows the area again.
        d.toggle_area(Area::Right);
        d.open(Panel::MasterBus);
        assert!(d.hidden.is_empty());

        let w = d.track_list_w;
        d.toggle_track_list(&t);
        assert_eq!(d.track_list_w, t.m.color_strip_w);
        d.toggle_track_list(&t);
        assert_eq!(d.track_list_w, w);
        d.toggle_maximize(&t);
        assert_eq!((d.area_sizes(&t, full, false, None, false), d.track_list_w), ([0.0; 4], t.m.color_strip_w));
        d.toggle_maximize(&t);
        assert_eq!(d.layout(""), before.0);
        // Auto-hide: a thin edge until the pointer is over the area.
        let shown = d.area_sizes(&t, full, true, None, false);
        assert_eq!(shown[1], PEEK);
        assert_eq!(d.area_sizes(&t, full, true, None, true)[1], before.1[1]);
        // VP16's Display preference moves the one dock area.
        let mut v = Dock::new(&Theme::new(LookChoice::VEGAS16));
        v.set_timeline_top(true);
        assert!(v.timeline_top());
        v.set_timeline_top(false);
        assert_eq!(v.groups, Dock::new(&Theme::new(LookChoice::VEGAS16)).groups);
    }

    /// Next Window goes through the timeline and every window of the shown areas, bringing each to the front.
    #[test]
    fn next_window_cycles_and_activates() {
        let t = Theme::new(LookChoice::VEGAS16);
        let mut d = Dock::new(&t);
        let n: usize = d.groups.iter().map(|g| g.tabs.len()).sum();
        d.cycle(false);
        assert_eq!((d.focus, d.groups[0].active), (Some(Panel::ProjectMedia), 0));
        d.cycle(false);
        assert_eq!((d.focus, d.groups[0].active), (Some(Panel::Explorer), 1));
        d.cycle(true);
        d.cycle(true);
        assert_eq!(d.focus, None);
        d.cycle(true);
        assert_eq!(d.focus, Some(Panel::MasterBus));
        for _ in 0..=n {
            d.cycle(false);
        }
        assert_eq!(d.focus, Some(Panel::MasterBus)); // the full ring is n windows plus the timeline
        d.toggle_all();
        d.cycle(false);
        assert_eq!(d.focus, None); // only the timeline is left
    }

    #[test]
    fn tab_scroll_window() {
        let w = [50.0, 60.0, 70.0, 80.0];
        assert_eq!(tab_end(&w, 0, 100.0), 2); // the second one cut
        assert_eq!(tab_end(&w, 2, 1000.0), 4);
        assert_eq!(scroll_to(&w, 150.0, 0, 3), 2);
        assert_eq!(scroll_to(&w, 140.0, 0, 3), 3);
        assert_eq!(scroll_to(&w, 140.0, 3, 1), 1);
        assert_eq!(scroll_to(&w, 140.0, 1, 2), 1);
    }

    fn frame(ctx: &Context, d: &mut Dock, rect: Rect, events: Vec<Event>) {
        // Frames 0.15 s apart: a press and release is a click (under 0.8 s), clicks three frames apart are no
        // double-click (over 0.3 s).
        let time = Some(ctx.input(|i| i.time) + 0.15);
        let raw = RawInput { events, time, screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1920.0, 700.0))), ..Default::default() };
        let _ = ctx.run(raw, |ctx| {
            CentralPanel::default().frame(Frame::NONE).show(ctx, |ui| d.show(ui, [rect, Rect::NOTHING, Rect::NOTHING, Rect::NOTHING], |_, _, _| {}));
        });
    }

    fn click(ctx: &Context, d: &mut Dock, rect: Rect, at: Pos2) {
        let press = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        frame(ctx, d, rect, vec![Event::PointerMoved(at)]);
        frame(ctx, d, rect, vec![press(true)]);
        frame(ctx, d, rect, vec![press(false)]);
    }

    /// Simulated pointer input: a click on the tab strip switches tabs, dragging a splitter resizes columns.
    #[test]
    fn click_tab_and_drag_splitter() {
        let ctx = Context::default();
        let t = theme::get();
        let mut d = Dock::new(&t);
        let rect = Rect::from_min_size(pos2(3.0, 0.0), vec2(1914.0, 596.0));
        click(&ctx, &mut d, rect, pos2(400.0, rect.max.y - 8.0));
        assert_ne!(d.groups[0].active, 0, "click on a tab activates it");
        assert_eq!(d.focus, Some(d.groups[0].tabs[d.groups[0].active]), "and focuses that window");

        let press = |p: Pos2, pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        let first_split = rect.min.x + rect.width() - 125.0 - 7.0 - 835.0 - 7.0 + 3.0;
        let mut y = pos2(first_split, 300.0);
        frame(&ctx, &mut d, rect, vec![Event::PointerMoved(y)]);
        frame(&ctx, &mut d, rect, vec![press(y, true)]);
        for _ in 0..4 {
            y.x -= 10.0;
            frame(&ctx, &mut d, rect, vec![Event::PointerMoved(y)]);
        }
        frame(&ctx, &mut d, rect, vec![press(y, false)]);
        assert!(d.groups[1].width > 835.0 && d.groups[1].width <= 875.0, "width {}", d.groups[1].width);
    }

    /// R13: a group with more tabs than its strip shows (every window, 400 px) scrolls with ◂ ▸, so every tab can be
    /// reached.
    #[test]
    fn overflowing_tabs_scroll() {
        let ctx = Context::default();
        let t = theme::get();
        let mut d = Dock::new(&t);
        let all: Vec<Panel> = Panel::ALL.into_iter().filter(|p| p.in_look(t.look())).collect();
        d.groups = vec![Group { tabs: all.clone(), active: 0, width: 0.0, area: Area::Top }];
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 300.0));
        let y = rect.max.y - t.m.tab_strip_h * 0.5;
        let (next, prev) = (pos2(rect.max.x - ARROW_W * 0.5, y), pos2(rect.max.x - ARROW_W * 1.5, y));
        let last = all[all.len() - 1];
        frame(&ctx, &mut d, rect, vec![]);
        assert!(d.drawn.contains(&all[0]) && !d.drawn.contains(&last), "the last tab starts out of view: {:?}", d.drawn);
        for _ in 0..all.len() {
            click(&ctx, &mut d, rect, next);
        }
        assert!(d.drawn.contains(&last) && !d.drawn.contains(&all[0]), "▸ brings the last tab into view: {:?}", d.drawn);
        for _ in 0..all.len() {
            click(&ctx, &mut d, rect, prev);
        }
        assert!(d.drawn.contains(&all[0]) && !d.drawn.contains(&last), "◂ goes back: {:?}", d.drawn);
        // A window activated from elsewhere (View › Window) is scrolled into view, and can be clicked there.
        d.open(last);
        frame(&ctx, &mut d, rect, vec![]);
        assert!(d.drawn.contains(&last), "{:?}", d.drawn);
        assert_eq!(d.groups.len(), 1);
    }
}
