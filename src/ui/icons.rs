//! Original vector icons (VEGAS_UI_SPEC.md §8), drawn with egui::Painter on a 16×16 grid: coloured shapes as 2006
//! pixel art, 1 px strokes snapped to the pixel grid, a 1 px dark outline round every fill, colours quantised to a
//! 24-colour period palette (UI_SPEC.md §7).
//! No bitmaps: shapes and meaning match VEGAS, proportions are our own.
use eframe::egui::{Align2, Color32, FontFamily, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, vec2};
use super::fonts;
use super::theme::{self, Theme};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    // §8.1 main toolbar
    New,
    Open,
    Save,
    Render,
    Properties,
    Cut,
    Copy,
    Paste,
    Undo,
    Redo,
    Tutorials,
    WhatsThis,
    // §8.2 transport and edit toolbar
    Record,
    Loop,
    PlayFromStart,
    Play,
    Pause,
    Stop,
    GoToStart,
    GoToEnd,
    PrevFrame,
    NextFrame,
    NormalEdit,
    EnvelopeEdit,
    SelectionEdit,
    ZoomEdit,
    Delete,
    Trim,
    TrimStart,
    TrimEnd,
    Split,
    SmartSplit,
    Lock,
    InsertMarker,
    InsertRegion,
    Snapping,
    AutoCrossfade,
    AutoRipple,
    LockEnvelopes,
    IgnoreGrouping,
    ColorGrading,
    OpenParent,
    OpenNested,
    CreateNested,
    LengthCursor,
    LengthSelEnd,
    LengthLength,
    // §8.3 track header, event and panel glyphs
    More,
    Mute,
    Solo,
    TrackFx,
    CompositingMode,
    PanCrop,
    GeneratedMedia,
    TrackVideo,
    TrackAudio,
    ExpandKeyframes,
    FaderThumb,
    PanThumb,
    Downmix,
    Dim,
    MixingConsole,
    ExternalMonitor,
    SplitScreen,
    Overlays,
    SnapshotCopy,
    SnapshotSave,
    AutoPreview,
    DockMaximize,
    DockClose,
    Logo,
    // helpers used across panels
    DropDown,
    Search,
    Folder,
    // Project Media / Explorer / plug-in chooser toolbars (§5.4–5.6)
    RemoveUnused,
    Capture,
    DeviceMedia,
    WebMedia,
    Views,
    Back,
    Forward,
    Up,
    Refresh,
    Favorite,
    FxBadge,
    // Project Media toolbar and bin tree, Video Preview status row (VP23)
    Import,
    MediaBin,
    StatusPreview,
    StatusFrame,
    StatusDisplay,
    // Pan/Crop and Track Motion toolbar (§5.15.2)
    SizeAboutCenter,
    /// R8b Mixing Console toolbar: Insert Bus (three lines joining into one).
    InsertBus,
}

impl Icon {
    pub const ALL: &[Icon] = {
        use Icon::*;
        &[
            New, Open, Save, Render, Properties, Cut, Copy, Paste, Undo, Redo, Tutorials, WhatsThis, Record, Loop,
            PlayFromStart, Play, Pause, Stop, GoToStart, GoToEnd, PrevFrame, NextFrame, NormalEdit, EnvelopeEdit,
            SelectionEdit, ZoomEdit, Delete, Trim, TrimStart, TrimEnd, Split, SmartSplit, Lock, InsertMarker,
            InsertRegion, Snapping, AutoCrossfade, AutoRipple, LockEnvelopes, IgnoreGrouping, ColorGrading,
            OpenParent, OpenNested, CreateNested, LengthCursor, LengthSelEnd, LengthLength, More, Mute, Solo,
            TrackFx, CompositingMode, PanCrop, GeneratedMedia, TrackVideo, TrackAudio, ExpandKeyframes, FaderThumb,
            PanThumb, Downmix, Dim, MixingConsole, ExternalMonitor, SplitScreen, Overlays, SnapshotCopy,
            SnapshotSave, AutoPreview, DockMaximize, DockClose, Logo, DropDown, Search, Folder, RemoveUnused, Capture,
            DeviceMedia, WebMedia, Views, Back, Forward, Up, Refresh, Favorite, FxBadge, Import, MediaBin, StatusPreview,
            StatusFrame, StatusDisplay, SizeAboutCenter, InsertBus,
        ]
    };
}

/// Paint `icon` centred in `rect` (scaled to its shorter side / 16).
/// `tint`: draw every part in this colour (disabled, or white on an active fill); None = the icon's own colours.
pub fn paint_icon(painter: &Painter, rect: Rect, icon: Icon, tint: Option<Color32>) {
    let t = theme::get();
    let side = rect.width().min(rect.height());
    let s = side / 16.0;
    // R48: the grid sits on physical pixels at any interface scale (`k` pixels per point), so strokes stay 1 px
    // wide and crisp at 125-200 % instead of smearing over two pixels.
    let k = painter.pixels_per_point();
    let o = ((rect.center() - vec2(side, side) * 0.5) * k).round() / k;
    let mirror = matches!(icon, Icon::Redo | Icon::GoToEnd | Icon::NextFrame | Icon::TrimEnd | Icon::Forward);
    let pen = Pen { p: painter, o, s, w: s, k, force: tint, mirror, t: &t };
    draw(&pen, icon);
}

/// Icon palette (UI_SPEC.md §7): the 16 VGA colours less the garish ones, plus the Windows
/// 2000/XP icon tones (folder yellows, document cream, steel blues, leaf greens, two more greys). Every icon colour is
/// snapped to its nearest member.
const PALETTE_2006: [u32; 24] = [
    0x000000, 0x404040, 0x808080, 0xA0A0A0, 0xC0C0C0, 0xE0E0E0, 0xFFFFFF, 0x000080, 0x0000FF, 0x3366CC, 0x6699FF, 0x008080,
    0x00A0C0, 0x008000, 0x339933, 0x99CC66, 0x800000, 0xFF0000, 0xCC6666, 0x808000, 0xFFCC33, 0xCC9933, 0x800080, 0xF0E0C0,
];

fn quantize_2006(c: Color32) -> Color32 {
    let (r, g, b) = (c.r() as i32, c.g() as i32, c.b() as i32);
    let best = PALETTE_2006
        .iter()
        .min_by_key(|&&v| {
            let (pr, pg, pb) = ((v >> 16) as i32 & 255, (v >> 8) as i32 & 255, v as i32 & 255);
            // Weighted RGB distance.
            3 * (r - pr).pow(2) + 4 * (g - pg).pow(2) + 2 * (b - pb).pow(2)
        })
        .copied()
        .unwrap_or(0);
    Color32::from_rgba_premultiplied((best >> 16) as u8, (best >> 8) as u8, best as u8, c.a())
}

#[derive(Clone, Copy)]
struct Pen<'a> {
    p: &'a Painter,
    o: Pos2,
    s: f32,
    w: f32,
    /// Physical pixels per point.
    k: f32,
    force: Option<Color32>,
    mirror: bool,
    t: &'a Theme,
}

type P = (f32, f32);

/// Classic outline round every filled shape.
const OUTLINE_2006: Color32 = Color32::from_rgb(0x20, 0x20, 0x20);

impl Pen<'_> {
    fn at(&self, (x, y): P) -> Pos2 {
        let x = if self.mirror { 16.0 - x } else { x };
        self.o + vec2(x, y) * self.s
    }
    /// A stroke vertex: on pixel centres, so 1 px lines stay crisp.
    fn at_s(&self, p: P) -> Pos2 {
        ((self.at(p) * self.k).floor() + vec2(0.5, 0.5)) / self.k
    }
    /// A fill vertex: on pixel edges.
    fn at_f(&self, p: P) -> Pos2 {
        (self.at(p) * self.k).round() / self.k
    }
    /// The forced tint, else the colour snapped to the palette.
    fn c(&self, c: Color32) -> Color32 {
        self.force.unwrap_or_else(|| quantize_2006(c))
    }
    fn ink(&self) -> Color32 {
        self.c(self.t.icon)
    }
    /// White "paper" of pages, sheets and pointers (the ink is black).
    fn paper(&self) -> Color32 {
        self.c(Color32::WHITE)
    }
    /// The surface the icon sits on (for hollow cut-outs).
    fn bg(&self) -> Color32 {
        self.force.unwrap_or(self.t.metal.face)
    }
    /// Stroke width: whole physical pixels.
    fn sw(&self, k: f32) -> f32 {
        (self.w * k * self.k).round().max(1.0) / self.k
    }
    /// One physical pixel per point, rounded: the 1 px outline at this scale.
    fn px1(&self) -> f32 {
        self.k.round().max(1.0) / self.k
    }
    /// The outline stroke of a fill (none when tinted: a disabled icon is one flat colour).
    fn outline(&self) -> Option<Stroke> {
        self.force.is_none().then(|| Stroke::new(self.px1(), OUTLINE_2006))
    }
    fn line(&self, pts: &[P], c: Color32) {
        self.line_w(pts, c, 1.0);
    }
    fn line_w(&self, pts: &[P], c: Color32, k: f32) {
        let pts: Vec<Pos2> = pts.iter().map(|p| self.at_s(*p)).collect();
        self.p.add(Shape::line(pts, Stroke::new(self.sw(k), self.c(c))));
    }
    fn closed(&self, pts: &[P], c: Color32) {
        let pts: Vec<Pos2> = pts.iter().map(|p| self.at_s(*p)).collect();
        self.p.add(Shape::closed_line(pts, Stroke::new(self.sw(1.0), self.c(c))));
    }
    fn poly(&self, pts: &[P], c: Color32) {
        let pts: Vec<Pos2> = pts.iter().map(|p| self.at_f(*p)).collect();
        if let Some(o) = self.outline() {
            let h = self.px1() * 0.5;
            let edge: Vec<Pos2> = pts.iter().map(|q| *q + vec2(h, h) - (*q - self.at_f((8.0, 8.0))).normalized() * h).collect();
            self.p.add(Shape::convex_polygon(pts, self.c(c), Stroke::NONE));
            self.p.add(Shape::closed_line(edge, o));
            return;
        }
        self.p.add(Shape::convex_polygon(pts, self.c(c), Stroke::NONE));
    }
    fn r(&self, x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_two_pos(self.at_f((x, y)), self.at_f((x + w, y + h)))
    }
    fn fill(&self, x: f32, y: f32, w: f32, h: f32, c: Color32) {
        let r = self.r(x, y, w, h);
        self.p.rect_filled(r, 0.0, self.c(c));
        if let Some(o) = self.outline() {
            if r.width() >= 3.0 * o.width && r.height() >= 3.0 * o.width {
                self.p.rect_stroke(r, 0.0, o, StrokeKind::Inside);
            }
        }
    }
    /// A filled box (the rounded VEGAS shapes, square here).
    fn fill_round(&self, x: f32, y: f32, w: f32, h: f32, _rad: f32, c: Color32) {
        self.p.rect_filled(self.r(x, y, w, h), 0.0, self.c(c));
        if let Some(o) = self.outline() {
            self.p.rect_stroke(self.r(x, y, w, h), 0.0, o, StrokeKind::Inside);
        }
    }
    fn rect(&self, x: f32, y: f32, w: f32, h: f32, c: Color32) {
        let r = Rect::from_two_pos(self.at_s((x, y)), self.at_s((x + w, y + h)));
        self.p.rect_stroke(r, 0.0, Stroke::new(self.px1(), self.c(c)), StrokeKind::Middle);
    }
    /// An outlined box (the rounded VEGAS shapes, square here).
    fn rect_round(&self, x: f32, y: f32, w: f32, h: f32, _rad: f32, c: Color32) {
        self.rect(x, y, w, h, c);
    }
    fn circle(&self, c: P, r: f32, col: Color32) {
        self.p.circle_stroke(self.at_s(c), r * self.s, Stroke::new(self.sw(1.0), self.c(col)));
    }
    fn dot(&self, c: P, r: f32, col: Color32) {
        self.p.circle_filled(self.at_s(c), r * self.s, self.c(col));
        if let Some(o) = self.outline() {
            if r >= 2.0 {
                self.p.circle_stroke(self.at_s(c), r * self.s - self.px1() * 0.5, o);
            }
        }
    }
    /// Arc from `a0` to `a1` degrees (0 = right, 90 = down).
    fn arc(&self, c: P, r: f32, a0: f32, a1: f32, col: Color32, k: f32) {
        let n = 16;
        let pts: Vec<P> = (0..=n)
            .map(|i| {
                let a = (a0 + (a1 - a0) * i as f32 / n as f32).to_radians();
                (c.0 + r * a.cos(), c.1 + r * a.sin())
            })
            .collect();
        self.line_w(&pts, col, k);
    }
    /// Letters in Tahoma Bold.
    fn text(&self, c: P, s: &str, size: f32, col: Color32) {
        let fam = if !fonts::installed() { FontFamily::Proportional } else { FontFamily::Name(fonts::TAHOMA_BOLD.into()) };
        self.p.text(self.at(c), Align2::CENTER_CENTER, s, FontId::new(size * self.s, fam), self.c(col));
    }
    /// Classic arrow pointer with its tip at (x, y), `k` = size factor (1 = 13 px tall).
    fn pointer(&self, x: f32, y: f32, k: f32, fill: Color32) {
        let q = |dx: f32, dy: f32| (x + dx * k, y + dy * k);
        self.poly(&[q(0.0, 0.0), q(0.0, 11.0), q(3.0, 8.5), q(8.0, 8.0)], fill);
        self.poly(&[q(2.6, 8.0), q(4.4, 7.4), q(6.4, 12.0), q(4.6, 12.8)], fill);
    }
    /// Small yellow "?" badge (Tutorials, What's This).
    fn help_badge(&self) {
        self.dot((12.0, 12.0), 3.8, hex(0xF0C800));
        self.text((12.0, 12.2), "?", 7.0, hex(0x1E1E1E));
    }
    fn speaker(&self, x: f32, c: Color32) {
        self.fill(x, 6.0, 3.0, 4.0, c);
        self.poly(&[(x + 3.0, 6.0), (x + 7.0, 2.5), (x + 7.0, 13.5), (x + 3.0, 10.0)], c);
    }
    fn page(&self, x: f32, y: f32, w: f32, h: f32, filled: bool) {
        if filled {
            self.fill(x, y, w, h, self.paper());
            self.rect(x, y, w, h, hex(0x5A5A5A));
        } else {
            self.fill(x, y, w, h, self.bg());
            self.rect(x, y, w, h, self.ink());
        }
    }
    fn floppy(&self) {
        let body = [(2.0, 2.0), (12.0, 2.0), (14.0, 4.0), (14.0, 14.0), (2.0, 14.0)];
        self.poly(&body, hex(0x3C78C8));
        self.fill(5.0, 2.0, 6.0, 4.0, hex(0xD0D0D0));
        self.fill(4.0, 9.0, 8.0, 5.0, hex(0xFFFFFF));
    }
    fn magnifier(&self) {
        self.circle((6.5, 6.5), 4.5, self.ink());
        self.line_w(&[(9.8, 9.8), (14.0, 14.0)], self.ink(), 1.6);
    }
    fn stacked(&self) {
        self.rect(3.0, 2.0, 8.0, 6.0, self.ink());
        self.fill(1.0, 5.0, 8.0, 6.0, self.bg());
        self.rect(1.0, 5.0, 8.0, 6.0, self.ink());
    }
    fn arrow_head(&self, tip: P, dir: P, c: Color32) {
        self.arrow_head_k(tip, dir, c, 1.0);
    }
    fn arrow_head_k(&self, tip: P, dir: P, c: Color32, k: f32) {
        let (dx, dy) = (dir.0 * k, dir.1 * k);
        let (nx, ny) = (-dy, dx);
        self.poly(&[tip, (tip.0 - dx * 3.0 + nx * 2.2, tip.1 - dy * 3.0 + ny * 2.2), (tip.0 - dx * 3.0 - nx * 2.2, tip.1 - dy * 3.0 - ny * 2.2)], c);
    }
    /// Dotted / dashed outline: `on` px dashes with `off` px gaps, drawn as small fills.
    #[allow(clippy::too_many_arguments)]
    fn dashed_rect(&self, x: f32, y: f32, w: f32, h: f32, on: f32, off: f32, c: Color32) {
        let k = 0.5 * self.w / self.s;
        let mut d = 0.0;
        while d < w {
            let l = on.min(w - d);
            self.fill(x + d, y - k, l, 2.0 * k, c);
            self.fill(x + d, y + h - k, l, 2.0 * k, c);
            d += on + off;
        }
        let mut d = 0.0;
        while d < h {
            let l = on.min(h - d);
            self.fill(x - k, y + d, 2.0 * k, l, c);
            self.fill(x + w - k, y + d, 2.0 * k, l, c);
            d += on + off;
        }
    }
    /// VP23 envelope node pair: a box lower-left, a box upper-right, joined by an elbow line (scaled by `k`).
    fn node_pair(&self, k: f32, c: Color32) {
        self.rect(1.5 * k, 10.0 * k, 4.0 * k, 4.0 * k, c);
        self.rect(10.5 * k, 2.0 * k, 4.0 * k, 4.0 * k, c);
        self.line(&[(3.5 * k, 10.0 * k), (3.5 * k, 4.0 * k), (10.5 * k, 4.0 * k)], c);
    }
    /// White sheet with a folded top-right corner (Render As, Copy, Paste, Import).
    fn sheet(&self, x: f32, y: f32, w: f32, h: f32) {
        let pts = [(x, y), (x + w - 3.0, y), (x + w, y + 3.0), (x + w, y + h), (x, y + h)];
        self.poly(&pts, self.paper());
        self.poly(&[(x + w - 3.0, y), (x + w, y + 3.0), (x + w - 3.0, y + 3.0)], hex(0x9A9A9A));
    }
}

const fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

fn draw(p: &Pen, icon: Icon) {
    use Icon::*;
    let ink = p.ink();
    let t = p.t;
    match icon {
        New => {
            let page = [(3.0, 1.0), (10.0, 1.0), (13.0, 4.0), (13.0, 15.0), (3.0, 15.0)];
            p.poly(&page, p.paper());
            p.poly(&[(10.0, 1.0), (13.0, 4.0), (10.0, 4.0)], hex(0x9A9A9A));
        }
        Folder => {
            // Bin folder: tall, bright yellow, darker spine (c_pm).
            p.fill(3.0, 0.5, 10.0, 15.5, hex(0xF8D775));
            p.fill(11.0, 9.0, 2.0, 7.0, hex(0xD8B048));
            p.fill(3.0, 0.5, 1.0, 15.5, hex(0xE8C45E));
        }
        Open => {
            let (body, flap) = (p.c(hex(0xD8B45A)), p.c(hex(0xA88A3C)));
            p.poly(&[(1.0, 3.0), (6.0, 3.0), (7.5, 5.0), (1.0, 5.0)], flap);
            p.fill(1.0, 5.0, 13.0, 9.0, flap);
            p.poly(&[(3.5, 8.0), (15.5, 8.0), (13.5, 14.0), (1.0, 14.0)], body);
        }
        Save | SnapshotSave => p.floppy(),
        Render => {
            // Grey sheet with a darker export arrow leaving it to the upper right (n16_top).
            p.poly(&[(1.5, 2.0), (8.5, 2.0), (11.5, 5.0), (11.5, 15.0), (1.5, 15.0)], hex(0xB4B4B4));
            let a = p.c(hex(0x6E6E6E));
            p.line_w(&[(6.0, 10.5), (11.5, 5.0)], a, 1.8);
            p.arrow_head_k((15.0, 1.5), (0.707, -0.707), a, 1.5);
        }
        Properties => {
            // Solid gear with a dark hub (n16_top, dark2026).
            for k in 0..8 {
                let a = (k as f32 * 45.0 + 22.5).to_radians();
                let (c, s) = (a.cos(), a.sin());
                p.line_w(&[(8.0 + 4.0 * c, 8.0 + 4.0 * s), (8.0 + 6.8 * c, 8.0 + 6.8 * s)], ink, 2.0);
            }
            p.dot((8.0, 8.0), 5.0, ink);
            p.dot((8.0, 8.0), 2.2, p.bg());
        }
        Cut => {
            let blade = p.c(hex(0xC8C8C8));
            p.line(&[(6.3, 10.0), (11.0, 1.5)], blade);
            p.line(&[(9.7, 10.0), (5.0, 1.5)], blade);
            let loops = p.c(hex(0xE05A6E));
            p.circle((4.8, 12.3), 2.5, loops);
            p.circle((11.2, 12.3), 2.5, loops);
        }
        Copy | SnapshotCopy => {
            if icon == Copy {
                // Blue sheet behind a white one.
                p.fill(2.0, 1.5, 8.0, 10.0, hex(0x5DB0E6));
                p.sheet(6.0, 4.5, 8.5, 10.5);
            } else {
                p.page(2.0, 1.5, 8.0, 10.0, true);
                p.page(6.0, 4.5, 8.0, 10.0, true);
            }
        }
        Paste => {
            // Green clipboard outline, white sheet on it.
            let g = hex(0x5AC878);
            p.rect(2.0, 3.0, 9.5, 11.5, g);
            p.fill(4.5, 1.0, 4.5, 3.0, g);
            p.sheet(7.0, 6.5, 7.5, 8.5);
        }
        Undo | Redo => {
            // Hook arrow: head pointing left at the top, the shaft bends round the right side and back
            // (Redo is mirrored).
            let k = 1.5;
            p.line_w(&[(4.5, 5.0), (9.0, 5.0)], ink, k);
            p.arc((9.0, 9.0), 4.0, 270.0, 450.0, ink, k);
            p.line_w(&[(9.0, 13.0), (5.0, 13.0)], ink, k);
            p.arrow_head_k((1.5, 5.0), (-1.0, 0.0), ink, 1.3);
        }
        Tutorials => {
            p.fill_round(4.0, 7.0, 7.0, 6.5, 2.0, p.paper());
            p.fill_round(5.0, 1.5, 2.2, 7.0, 1.0, p.paper());
            p.help_badge();
        }
        WhatsThis => {
            p.pointer(2.0, 1.0, 1.0, p.paper());
            p.help_badge();
        }
        Record => {
            p.fill_round(5.5, 1.0, 5.0, 8.0, 2.5, hex(0xE0505A));
            p.arc((8.0, 6.5), 4.0, 0.0, 180.0, ink, 1.0);
            p.line(&[(8.0, 10.5), (8.0, 14.0)], ink);
            p.line(&[(5.0, 14.5), (11.0, 14.5)], ink);
        }
        Loop => {
            // Circular arrow with a blue tick at the top (c_transport).
            p.arc((8.0, 9.0), 5.0, -60.0, 220.0, ink, 1.2);
            p.arrow_head((3.2, 5.6), (0.64, -0.77), ink);
            p.line_w(&[(8.0, 1.0), (8.0, 5.5)], p.c(hex(0x3C8CDC)), 1.3);
        }
        PlayFromStart => {
            p.fill(3.0, 3.0, 2.0, 10.0, ink);
            p.poly(&[(6.5, 3.0), (6.5, 13.0), (13.0, 8.0)], ink);
        }
        Play => p.poly(&[(4.0, 3.0), (4.0, 13.0), (13.0, 8.0)], ink),
        Pause => {
            p.fill(4.0, 3.0, 3.0, 10.0, ink);
            p.fill(9.0, 3.0, 3.0, 10.0, ink);
        }
        Stop => p.fill(3.5, 3.5, 9.0, 9.0, ink),
        GoToStart | GoToEnd => {
            p.fill(3.0, 3.0, 2.0, 10.0, ink);
            p.poly(&[(13.0, 3.0), (13.0, 13.0), (6.0, 8.0)], ink);
        }
        PrevFrame | NextFrame => {
            p.poly(&[(9.0, 4.0), (9.0, 12.0), (3.5, 8.0)], ink);
            p.fill(10.0, 4.0, 1.2, 8.0, ink);
            p.fill(12.4, 4.0, 1.2, 8.0, ink);
        }
        NormalEdit => {
            p.pointer(2.0, 1.0, 0.95, p.paper());
            p.line(&[(11.0, 10.0), (10.0, 10.0), (10.0, 14.5), (11.0, 14.5)], ink);
            p.line(&[(14.0, 10.0), (15.0, 10.0), (15.0, 14.5), (14.0, 14.5)], ink);
        }
        EnvelopeEdit => {
            p.pointer(1.5, 0.5, 0.85, p.paper());
            let c = p.c(hex(0x3C78C8));
            p.line(&[(7.5, 14.0), (11.0, 10.0), (15.0, 12.5)], c);
            p.fill(10.0, 9.0, 2.0, 2.0, ink);
            p.fill(14.0, 11.5, 2.0, 2.0, ink);
        }
        SelectionEdit => {
            p.pointer(1.5, 0.5, 0.85, p.paper());
            for (a, b) in [((8.0, 9.0), (10.0, 9.0)), ((12.0, 9.0), (14.5, 9.0)), ((14.5, 11.0), (14.5, 12.5)), ((8.0, 14.5), (10.0, 14.5)), ((12.0, 14.5), (14.5, 14.5)), ((8.0, 11.0), (8.0, 12.5))] {
                p.line(&[a, b], ink);
            }
        }
        ZoomEdit => {
            // Magnifier inside a dotted frame (c_transport).
            p.dashed_rect(1.0, 1.0, 11.0, 11.0, 1.0, 1.0, ink);
            p.circle((9.0, 9.0), 3.2, ink);
            p.line_w(&[(11.3, 11.3), (14.5, 14.5)], ink, 1.4);
        }
        Search => p.magnifier(),
        Delete => {
            let c = p.c(hex(0xE04040));
            p.line_w(&[(3.5, 3.5), (12.5, 12.5)], c, 1.4);
            p.line_w(&[(12.5, 3.5), (3.5, 12.5)], c, 1.4);
        }
        Trim => {
            p.fill(4.0, 5.5, 8.0, 5.0, ink);
            p.line(&[(3.0, 3.0), (1.0, 3.0), (1.0, 13.0), (3.0, 13.0)], ink);
            p.line(&[(13.0, 3.0), (15.0, 3.0), (15.0, 13.0), (13.0, 13.0)], ink);
        }
        TrimStart | TrimEnd => {
            let h = p.c(hex(0xD04040));
            p.rect(1.5, 4.0, 6.5, 8.0, h);
            for k in 0..3 {
                let x = 1.5 + k as f32 * 2.5;
                p.line(&[(x, 12.0), ((x + 4.0).min(8.0), 12.0 - (x + 4.0).min(8.0) + x)], h);
            }
            p.fill(9.0, 2.0, 1.5, 12.0, ink);
            p.rect(11.5, 4.0, 3.0, 8.0, ink);
        }
        Split | SmartSplit => {
            let b = hex(0x3C78C8);
            p.fill_round(1.0, 5.0, 6.0, 6.0, 2.0, b);
            p.fill_round(9.0, 5.0, 6.0, 6.0, 2.0, b);
            p.line(&[(8.0, 2.0), (8.0, 14.0)], ink);
            if icon == SmartSplit {
                p.line(&[(13.0, 0.5), (13.0, 4.5)], ink);
                p.line(&[(11.0, 2.5), (15.0, 2.5)], ink);
            }
        }
        Lock => {
            p.arc((8.0, 7.0), 3.0, 180.0, 360.0, ink, 1.0);
            p.line(&[(5.0, 7.0), (5.0, 8.0)], ink);
            p.line(&[(11.0, 7.0), (11.0, 8.0)], ink);
            p.fill(4.0, 8.0, 8.0, 6.0, hex(0xDCDCDC));
        }
        InsertMarker => {
            // Speech-bubble tag: square flag with its tail down the left edge (olive 16, orange 23).
            let c = p.c(hex(0xBDAB5A));
            p.fill(3.0, 2.5, 10.5, 8.5, c);
            p.poly(&[(3.0, 11.0), (6.5, 11.0), (3.0, 14.5)], c);
        }
        InsertRegion => {
            // Green double flag: one block on two legs with a notch between them (c_transport).
            let c = p.c(hex(0x79B27B));
            p.fill(2.0, 2.5, 12.0, 6.5, c);
            p.fill(2.0, 9.0, 2.0, 5.5, c);
            p.fill(12.0, 9.0, 2.0, 5.5, c);
            p.poly(&[(6.5, 9.0), (9.5, 9.0), (8.0, 7.0)], p.bg());
        }
        Snapping => {
            let body = p.c(hex(0xD03030));
            p.arc((8.0, 8.0), 4.5, 180.0, 360.0, body, 1.7);
            p.line_w(&[(3.5, 8.0), (3.5, 11.0)], body, 1.7);
            p.line_w(&[(12.5, 8.0), (12.5, 11.0)], body, 1.7);
            let tip = p.c(hex(0xFFFFFF));
            p.line_w(&[(3.5, 12.0), (3.5, 14.5)], tip, 1.7);
            p.line_w(&[(12.5, 12.0), (12.5, 14.5)], tip, 1.7);
        }
        AutoCrossfade => {
            p.rect(1.0, 3.0, 14.0, 10.0, ink);
            let c = p.c(hex(0x3C78C8));
            p.line(&[(1.0, 13.0), (15.0, 3.0)], c);
            p.line(&[(1.0, 3.0), (15.0, 13.0)], c);
        }
        AutoRipple => {
            for (x, y) in [(1.0, 3.0), (4.5, 7.5), (8.0, 12.0)] {
                p.line(&[(x, y), (x + 5.0, y)], ink);
                p.arrow_head((x + 7.0, y), (1.0, 0.0), ink);
            }
        }
        LockEnvelopes => {
            p.rect(1.0, 8.0, 11.0, 6.0, ink);
            p.line(&[(1.0, 12.5), (5.0, 10.0), (12.0, 11.0)], p.c(hex(0x3C78C8)));
            p.arc((13.0, 3.5), 1.6, 180.0, 360.0, ink, 0.8);
            p.fill(10.8, 3.5, 4.4, 3.5, ink);
        }
        IgnoreGrouping => {
            // Two grouped boxes (one solid, one dotted) and a pointer over them (c_transport, dark2026).
            p.fill(1.0, 2.0, 6.0, 6.0, p.c(hex(0x5DB0E6)));
            p.dashed_rect(8.5, 2.0, 6.0, 6.0, 1.0, 1.0, ink);
            p.pointer(8.0, 6.5, 0.7, p.paper());
        }
        ColorGrading => {
            p.dot((8.0, 4.5), 2.7, hex(0xDC5050));
            p.dot((4.5, 11.0), 2.7, hex(0x50C878));
            p.dot((11.5, 11.0), 2.7, hex(0x3C8CDC));
        }
        OpenParent | OpenNested | CreateNested => {
            p.stacked();
            match icon {
                OpenParent => {
                    p.line(&[(13.5, 14.0), (13.5, 5.0)], ink);
                    p.arrow_head((13.5, 2.5), (0.0, -1.0), ink);
                }
                OpenNested => {
                    p.line(&[(13.5, 2.0), (13.5, 11.0)], ink);
                    p.arrow_head((13.5, 13.5), (0.0, 1.0), ink);
                }
                _ => {
                    p.line(&[(13.5, 9.0), (13.5, 15.0)], ink);
                    p.line(&[(10.5, 12.0), (16.0, 12.0)], ink);
                }
            }
        }
        LengthCursor => {
            p.arc((8.0, 6.0), 3.5, 150.0, 390.0, ink, 1.0);
            p.line(&[(5.0, 7.8), (8.0, 14.0), (11.0, 7.8)], ink);
        }
        LengthSelEnd => {
            p.rect(2.0, 2.0, 12.0, 12.0, ink);
            p.poly(&[(3.0, 8.0), (3.0, 13.0), (8.0, 13.0)], hex(0xF0D000));
        }
        LengthLength => {
            p.rect(2.0, 2.0, 12.0, 12.0, ink);
            let y = hex(0xF0D000);
            p.line(&[(6.0, 8.0), (10.0, 8.0)], y);
            p.arrow_head((4.0, 8.0), (-1.0, 0.0), y);
            p.arrow_head((12.0, 8.0), (1.0, 0.0), y);
        }
        More => {
            for y in [3.0, 7.0, 11.0] {
                p.fill(3.0, y, 10.0, 2.0, ink);
            }
        }
        Mute | Solo => {
            // A bold black letter on the (bevelled) button.
            p.text((8.0, 8.5), if icon == Mute { "M" } else { "S" }, t.font.mute_solo + 1.0, ink);
        }
        TrackFx => {
            p.line(&[(3.5, 14.0), (5.5, 5.0), (6.5, 2.8), (9.0, 2.2)], ink);
            p.line(&[(3.5, 7.0), (8.5, 7.0)], ink);
            p.line(&[(8.0, 9.0), (13.0, 14.0)], ink);
            p.line(&[(13.0, 9.0), (8.0, 14.0)], ink);
        }
        CompositingMode => {
            // Blue square with a dark diagonal arrow, a small white square off its lower-right corner (dark2026).
            p.fill(1.0, 1.0, 10.0, 10.0, t.accent);
            let a = hex(0x141464);
            p.line(&[(3.0, 3.0), (7.5, 7.5)], a);
            p.poly(&[(8.8, 8.8), (8.8, 4.8), (4.8, 8.8)], a);
            p.fill(9.0, 9.0, 6.0, 6.0, hex(0xFFFFFF));
        }
        PanCrop => {
            p.line(&[(4.5, 1.0), (4.5, 11.5), (15.0, 11.5)], ink);
            p.line(&[(1.0, 4.5), (11.5, 4.5), (11.5, 15.0)], ink);
        }
        GeneratedMedia => {
            p.rect(1.0, 3.0, 14.0, 10.0, ink);
            p.circle((8.0, 8.0), 2.5, ink);
        }
        TrackVideo => {
            // Upright film frame with sprocket holes down both sides (c_trackhdr).
            p.fill(3.0, 1.0, 10.0, 14.0, ink);
            for y in [2.5, 6.5, 10.5] {
                for x in [4.0, 10.5] {
                    p.p.rect_filled(p.r(x, y, 1.5, 2.0), 0.0, hex(0x3C3C3C));
                }
            }
        }
        TrackAudio => {
            p.speaker(3.0, ink);
        }
        ExpandKeyframes => {
            p.line(&[(4.5, 4.0), (8.0, 7.0), (11.5, 4.0)], ink);
            p.line(&[(4.5, 8.0), (8.0, 11.0), (11.5, 8.0)], ink);
        }
        FaderThumb => {
            // Chrome handle with a centre line (the track sliders paint theirs with `widgets::thumb`).
            p.fill(3.0, 1.0, 10.0, 14.0, t.metal.face);
            p.line(&[(4.0, 1.5), (12.0, 1.5)], t.metal.light);
            p.line(&[(3.5, 1.5), (3.5, 14.5)], t.metal.light);
            p.fill(7.5, 4.0, 1.0, 8.0, t.metal.dark);
        }
        PanThumb => p.poly(&[(3.0, 4.0), (13.0, 4.0), (8.0, 12.0)], t.fader_thumb),
        Downmix => {
            let a = p.c(hex(0xF0C800));
            p.line(&[(8.0, 2.0), (8.0, 14.0)], ink);
            p.line(&[(1.0, 8.0), (4.5, 8.0)], a);
            p.arrow_head((7.0, 8.0), (1.0, 0.0), a);
            p.line(&[(15.0, 8.0), (11.5, 8.0)], a);
            p.arrow_head((9.0, 8.0), (-1.0, 0.0), a);
        }
        Dim => {
            p.speaker(1.0, ink);
            p.poly(&[(10.0, 9.0), (15.0, 9.0), (12.5, 13.0)], p.c(hex(0x3C78C8)));
        }
        InsertBus => {
            for y in [3.0, 8.0, 13.0] {
                p.line(&[(1.0, y), (6.0, y), (10.0, 8.0)], ink);
            }
            p.line(&[(10.0, 8.0), (15.0, 8.0)], ink);
        }
        MixingConsole => {
            for (x, k) in [(4.0, 10.0), (8.0, 5.0), (12.0, 8.0)] {
                p.line(&[(x, 2.0), (x, 14.0)], ink);
                p.fill(x - 1.5, k, 3.0, 2.0, ink);
            }
        }
        ExternalMonitor => {
            p.rect(1.0, 1.5, 10.0, 7.0, ink);
            p.fill(5.0, 6.0, 10.0, 7.0, p.bg());
            p.rect(5.0, 6.0, 10.0, 7.0, ink);
            p.line(&[(8.0, 15.0), (12.0, 15.0)], ink);
        }
        SplitScreen => {
            p.rect(2.0, 2.0, 12.0, 12.0, ink);
            p.poly(&[(2.0, 2.0), (8.0, 2.0), (8.0, 14.0), (2.0, 14.0)], ink);
            p.line(&[(2.0, 14.0), (14.0, 2.0)], ink);
        }
        Overlays => {
            p.rect(2.0, 2.0, 12.0, 12.0, ink);
            for k in [6.0, 10.0] {
                p.line(&[(k, 2.0), (k, 14.0)], ink);
                p.line(&[(2.0, k), (14.0, k)], ink);
            }
        }
        AutoPreview => {
            // Pointer with a green play disc at its upper right.
            p.pointer(1.0, 3.0, 0.9, p.paper());
            p.dot((11.0, 5.5), 4.5, hex(0x1E1E1E));
            p.dot((11.0, 5.5), 3.8, hex(0x3CC850));
            p.poly(&[(9.8, 3.6), (9.8, 7.4), (13.0, 5.5)], hex(0x1E1E1E));
        }
        DockMaximize => p.rect(3.5, 3.5, 9.0, 9.0, ink),
        DockClose => {
            p.line(&[(4.0, 4.0), (12.0, 12.0)], ink);
            p.line(&[(12.0, 4.0), (4.0, 12.0)], ink);
        }
        Logo => {
            // The brand's metallic chrome M (assets/brand/milan-pro-256.png), tinted when the icon is.
            let r = p.r(0.0, 0.0, 16.0, 16.0);
            let tex = super::widgets::brand_tex(p.p.ctx(), super::widgets::Brand::Mark);
            p.p.image(tex.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), p.force.unwrap_or(Color32::WHITE));
        }
        DropDown => p.poly(&[(5.5, 6.5), (10.5, 6.5), (8.0, 9.5)], ink),
        RemoveUnused => {
            p.page(2.0, 1.5, 9.0, 11.0, true);
            for y in [4.5, 7.0, 9.5] {
                p.line_w(&[(4.0, y), (9.0, y)], hex(0x3C3C3C), 0.7);
            }
            let c = p.c(hex(0xE04040));
            p.line_w(&[(9.0, 9.0), (14.5, 14.5)], c, 1.3);
            p.line_w(&[(14.5, 9.0), (9.0, 14.5)], c, 1.3);
        }
        Capture => {
            // Framed box with a diagonal hatch (drawn greyed: no capture devices).
            p.rect(1.5, 2.5, 13.0, 11.0, ink);
            for k in 0..3 {
                let x = 4.0 + 3.5 * k as f32;
                p.line_w(&[(x, 12.0), (x + 4.0, 4.0)], ink, 0.7);
            }
        }
        DeviceMedia => {
            // White camcorder with a pink arrow down into the lower right (c_pm).
            p.fill(1.0, 2.0, 10.0, 7.5, p.c(hex(0xF0F0F0)));
            p.poly(&[(11.0, 4.0), (14.5, 2.0), (14.5, 9.5), (11.0, 7.5)], p.c(hex(0xF0F0F0)));
            let r = p.c(hex(0xE0506E));
            p.line_w(&[(5.0, 8.5), (10.5, 13.0)], r, 1.6);
            p.arrow_head_k((14.0, 15.5), (0.707, 0.707), r, 1.2);
        }
        WebMedia => {
            // White globe with grey meridians and a red arrow coming down into it.
            p.dot((7.0, 8.5), 6.0, hex(0xF0F0F0));
            let g = hex(0x8A8A8A);
            p.line_w(&[(1.5, 8.5), (12.5, 8.5)], g, 0.7);
            p.p.add(Shape::ellipse_stroke(p.at((7.0, 8.5)), vec2(2.4, 6.0) * p.s, Stroke::new(0.7 * p.w, p.c(g))));
            let r = hex(0xE0506E);
            p.line_w(&[(9.0, 9.0), (12.5, 12.5)], r, 1.6);
            p.arrow_head_k((15.0, 15.0), (0.707, 0.707), r, 1.2);
        }
        Import => {
            // Sheet with a blue arrow coming in from the upper left.
            p.sheet(5.0, 1.5, 10.0, 13.5);
            let a = p.c(hex(0x2E8BE6));
            p.line_w(&[(1.0, 3.0), (6.5, 8.5)], a, 1.8);
            p.arrow_head_k((10.0, 12.0), (0.707, 0.707), a, 1.4);
        }
        MediaBin => {
            // Three stacked frames, the front one with a picture strip.
            p.rect(5.5, 1.5, 9.0, 8.0, ink);
            p.fill(3.5, 3.5, 9.0, 8.0, p.bg());
            p.rect(3.5, 3.5, 9.0, 8.0, ink);
            p.fill(1.5, 5.5, 9.5, 9.0, p.bg());
            p.rect(1.5, 5.5, 9.5, 9.0, ink);
            p.fill(3.5, 8.0, 5.5, 4.0, ink);
        }
        StatusPreview => {
            p.fill(2.0, 3.0, 12.0, 10.0, hex(0x3C8CDC));
            p.rect(2.0, 3.0, 12.0, 10.0, ink);
        }
        StatusFrame => {
            p.rect(2.0, 3.0, 12.0, 10.0, ink);
            for x in [4.0, 7.0, 10.0] {
                p.fill(x, 4.5, 1.5, 1.5, ink);
                p.fill(x, 10.0, 1.5, 1.5, ink);
            }
        }
        SizeAboutCenter => {
            // A dashed box with a centre dot and arrows growing out from it to the corners.
            p.dashed_rect(1.5, 1.5, 13.0, 13.0, 2.0, 1.5, ink);
            p.dot((8.0, 8.0), 1.2, ink);
            for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                p.line(&[(8.0 + dx * 2.0, 8.0 + dy * 2.0), (8.0 + dx * 5.0, 8.0 + dy * 5.0)], ink);
                p.arrow_head_k((8.0 + dx * 5.5, 8.0 + dy * 5.5), (dx * 0.7071, dy * 0.7071), ink, 0.8);
            }
        }
        StatusDisplay => {
            p.rect(3.0, 4.0, 10.0, 8.0, ink);
            for (a, b, c) in [((1.0, 4.0), (1.0, 1.0), (4.0, 1.0)), ((12.0, 1.0), (15.0, 1.0), (15.0, 4.0)), ((1.0, 12.0), (1.0, 15.0), (4.0, 15.0)), ((12.0, 15.0), (15.0, 15.0), (15.0, 12.0))] {
                p.line(&[a, b, c], ink);
            }
        }
        Views => {
            for y in [3.0, 7.0, 11.0] {
                p.fill(2.0, y, 2.5, 2.5, ink);
                p.fill(6.0, y + 0.5, 8.0, 1.5, ink);
            }
        }
        Back | Forward => {
            let c = p.c(hex(0x3CA050));
            p.line_w(&[(13.5, 8.0), (5.0, 8.0)], c, 1.3);
            p.arrow_head((2.5, 8.0), (-1.0, 0.0), c);
        }
        Up => {
            let c = p.c(hex(0x3CA050));
            p.line_w(&[(8.0, 14.0), (8.0, 5.5)], c, 1.3);
            p.arrow_head((8.0, 2.5), (0.0, -1.0), c);
        }
        Refresh => {
            let c = p.c(hex(0x3CA050));
            p.arc((8.0, 8.0), 5.0, 300.0, 600.0, c, 1.1);
            p.arrow_head((12.4, 3.2), (0.6, -0.8), c);
        }
        Favorite => {
            let pts: Vec<P> = (0..10)
                .map(|k| {
                    let a = (-90.0 + 36.0 * k as f32).to_radians();
                    let r = if k % 2 == 0 { 6.5 } else { 2.8 };
                    (8.0 + r * a.cos(), 8.5 + r * a.sin())
                })
                .collect();
            p.closed(&pts, p.c(hex(0xF0C800)));
        }
        FxBadge => {
            p.fill(2.0, 3.0, 12.0, 10.0, hex(0x3C8CDC));
            p.text((8.0, 8.0), "fx", 9.0, hex(0xFFFFFF));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{self, LayerId, pos2};

    #[test]
    fn every_icon_paints() {
        let ctx = egui::Context::default();
        super::super::fonts::install(&ctx);
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let painter = ctx.layer_painter(LayerId::background());
            for (i, &icon) in Icon::ALL.iter().enumerate() {
                let r = Rect::from_min_size(pos2(i as f32 * 20.0, 0.0), vec2(16.0, 16.0));
                paint_icon(&painter, r, icon, None);
                paint_icon(&painter, r, icon, Some(Color32::GRAY));
            }
        });
    }
}
