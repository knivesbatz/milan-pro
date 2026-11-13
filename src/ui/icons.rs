//! Original vector icons (spec §8), drawn with egui::Painter on a 16×16 grid.
//! VP16 = coloured, slightly raised (1 px dark shade lower-right); VP23 = flat monochrome lines.
//! No bitmaps: shapes and meaning match VEGAS, proportions are our own.
use eframe::egui::{Align2, Color32, CornerRadius, FontFamily, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, vec2};
use super::fonts;
use super::theme::{self, Theme, UiLook};

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
/// `tint`: draw every part in this colour (disabled, or white on an active fill); None = look colours.
pub fn paint_icon(painter: &Painter, rect: Rect, icon: Icon, look: UiLook, tint: Option<Color32>) {
    let t = theme::get();
    let side = rect.width().min(rect.height());
    let s = side / 16.0;
    let o = (rect.center() - vec2(side, side) * 0.5).round();
    let v16 = look == UiLook::Vegas16;
    let stroke = if v16 { 1.5 } else { 1.25 };
    let mirror = matches!(icon, Icon::Redo | Icon::GoToEnd | Icon::NextFrame | Icon::TrimEnd | Icon::Forward);
    let pen = Pen { p: painter, o, s, w: stroke * s, force: tint, strength: t.icon_color_strength, mirror, t: &t, v16 };
    // VP16 raised look: the same shape in a dark shade 1 px down-right, then the icon on top.
    if v16 && tint.is_none() && t.icon_shadow.a() > 0 && !matches!(icon, Icon::Logo | Icon::FaderThumb) {
        let shadow = Pen { o: o + vec2(1.0, 1.0), force: Some(t.icon_shadow.gamma_multiply(0.8)), ..pen };
        draw(&shadow, icon);
    }
    draw(&pen, icon);
}

#[derive(Clone, Copy)]
struct Pen<'a> {
    p: &'a Painter,
    o: Pos2,
    s: f32,
    w: f32,
    force: Option<Color32>,
    strength: f32,
    mirror: bool,
    t: &'a Theme,
    v16: bool,
}

type P = (f32, f32);

impl Pen<'_> {
    fn at(&self, (x, y): P) -> Pos2 {
        let x = if self.mirror { 16.0 - x } else { x };
        self.o + vec2(x, y) * self.s
    }
    /// Look colour: forced tint, else VP16 colours toned down by "Icon color strength".
    fn c(&self, c: Color32) -> Color32 {
        if let Some(f) = self.force {
            return f;
        }
        if self.strength >= 1.0 {
            return c;
        }
        let l = (0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32) as u8;
        Color32::from_gray(l).lerp_to_gamma(c, self.strength)
    }
    /// Coloured in VP16, plain ink in VP23.
    fn c16(&self, c: Color32) -> Color32 {
        if self.v16 { self.c(c) } else { self.ink() }
    }
    fn ink(&self) -> Color32 {
        self.c(self.t.icon)
    }
    /// The surface the icon sits on (for hollow cut-outs).
    fn bg(&self) -> Color32 {
        self.force.map_or(self.t.toolbar_bg, |_| self.c(self.t.toolbar_bg))
    }
    fn line(&self, pts: &[P], c: Color32) {
        self.line_w(pts, c, 1.0);
    }
    fn line_w(&self, pts: &[P], c: Color32, k: f32) {
        let pts: Vec<Pos2> = pts.iter().map(|p| self.at(*p)).collect();
        self.p.add(Shape::line(pts, Stroke::new(self.w * k, self.c(c))));
    }
    fn closed(&self, pts: &[P], c: Color32) {
        let pts: Vec<Pos2> = pts.iter().map(|p| self.at(*p)).collect();
        self.p.add(Shape::closed_line(pts, Stroke::new(self.w, self.c(c))));
    }
    fn poly(&self, pts: &[P], c: Color32) {
        let pts: Vec<Pos2> = pts.iter().map(|p| self.at(*p)).collect();
        self.p.add(Shape::convex_polygon(pts, self.c(c), Stroke::NONE));
    }
    fn r(&self, x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_two_pos(self.at((x, y)), self.at((x + w, y + h)))
    }
    fn fill(&self, x: f32, y: f32, w: f32, h: f32, c: Color32) {
        self.p.rect_filled(self.r(x, y, w, h), 0.0, self.c(c));
    }
    fn fill_round(&self, x: f32, y: f32, w: f32, h: f32, rad: f32, c: Color32) {
        self.p.rect_filled(self.r(x, y, w, h), CornerRadius::same((rad * self.s) as u8), self.c(c));
    }
    fn rect(&self, x: f32, y: f32, w: f32, h: f32, c: Color32) {
        self.p.rect_stroke(self.r(x, y, w, h), 0.0, Stroke::new(self.w, self.c(c)), StrokeKind::Middle);
    }
    fn rect_round(&self, x: f32, y: f32, w: f32, h: f32, rad: f32, c: Color32) {
        let cr = CornerRadius::same((rad * self.s) as u8);
        self.p.rect_stroke(self.r(x, y, w, h), cr, Stroke::new(self.w, self.c(c)), StrokeKind::Middle);
    }
    fn circle(&self, c: P, r: f32, col: Color32) {
        self.p.circle_stroke(self.at(c), r * self.s, Stroke::new(self.w, self.c(col)));
    }
    fn dot(&self, c: P, r: f32, col: Color32) {
        self.p.circle_filled(self.at(c), r * self.s, self.c(col));
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
    fn text(&self, c: P, s: &str, size: f32, family: &str, col: Color32) {
        let fam = if family.is_empty() { FontFamily::Proportional } else { FontFamily::Name(family.into()) };
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
        self.text((12.0, 12.2), "?", 7.0, fonts::BOLD, hex(0x1E1E1E));
    }
    fn speaker(&self, x: f32, c: Color32) {
        self.fill(x, 6.0, 3.0, 4.0, c);
        self.poly(&[(x + 3.0, 6.0), (x + 7.0, 2.5), (x + 7.0, 13.5), (x + 3.0, 10.0)], c);
    }
    fn page(&self, x: f32, y: f32, w: f32, h: f32, filled: bool) {
        if filled {
            self.fill(x, y, w, h, self.t.icon);
            self.rect(x, y, w, h, hex(0x5A5A5A));
        } else {
            self.fill(x, y, w, h, self.bg());
            self.rect(x, y, w, h, self.ink());
        }
    }
    fn floppy(&self) {
        let body = [(2.0, 2.0), (12.0, 2.0), (14.0, 4.0), (14.0, 14.0), (2.0, 14.0)];
        if self.v16 {
            self.poly(&body, hex(0x3C78C8));
            self.fill(5.0, 2.0, 6.0, 4.0, hex(0xD0D0D0));
            self.fill(4.0, 9.0, 8.0, 5.0, hex(0xFFFFFF));
        } else {
            // Solid disk: shutter notch at the top, dark hub in the middle (dark2026).
            self.poly(&body, self.ink());
            self.fill(5.0, 2.0, 5.5, 3.0, self.bg());
            self.dot((8.0, 9.5), 2.4, self.bg());
        }
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
        if self.v16 {
            self.poly(&pts, self.t.icon);
            self.poly(&[(x + w - 3.0, y), (x + w, y + 3.0), (x + w - 3.0, y + 3.0)], hex(0x9A9A9A));
        } else {
            self.closed(&pts, self.ink());
        }
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
            if p.v16 {
                p.poly(&page, t.icon);
                p.poly(&[(10.0, 1.0), (13.0, 4.0), (10.0, 4.0)], hex(0x9A9A9A));
            } else {
                // Solid page, the folded corner cut out (dark2026).
                p.poly(&page, ink);
                p.line(&[(10.0, 1.5), (10.0, 4.0), (12.5, 4.0)], p.bg());
            }
        }
        Folder if p.v16 => {
            // VP16 bin folder: tall, bright yellow, darker spine (c_pm).
            p.fill(3.0, 0.5, 10.0, 15.5, hex(0xF8D775));
            p.fill(11.0, 9.0, 2.0, 7.0, hex(0xD8B048));
            p.fill(3.0, 0.5, 1.0, 15.5, hex(0xE8C45E));
        }
        Folder if !p.v16 => {
            // VP23 bin node: outline folder (spec 5.4).
            p.closed(&[(1.5, 3.0), (6.0, 3.0), (7.5, 5.0), (14.5, 5.0), (14.5, 13.5), (1.5, 13.5)], ink);
        }
        Open | Folder => {
            let (body, flap) = (p.c16(hex(0xD8B45A)), p.c16(hex(0xA88A3C)));
            p.poly(&[(1.0, 3.0), (6.0, 3.0), (7.5, 5.0), (1.0, 5.0)], flap);
            if icon == Folder || !p.v16 {
                p.fill(1.0, 5.0, 14.0, 9.0, body);
            } else if p.v16 {
                p.fill(1.0, 5.0, 13.0, 9.0, flap);
                p.poly(&[(3.5, 8.0), (15.5, 8.0), (13.5, 14.0), (1.0, 14.0)], body);
            }
        }
        Save | SnapshotSave => p.floppy(),
        Render => {
            if p.v16 {
                // Grey sheet with a darker export arrow leaving it to the upper right (n16_top).
                p.poly(&[(1.5, 2.0), (8.5, 2.0), (11.5, 5.0), (11.5, 15.0), (1.5, 15.0)], hex(0xB4B4B4));
                let a = p.c(hex(0x6E6E6E));
                p.line_w(&[(6.0, 10.5), (11.5, 5.0)], a, 1.8);
                p.arrow_head_k((15.0, 1.5), (0.707, -0.707), a, 1.5);
            } else {
                // Open box with an out-arrow (open chevron head).
                p.line(&[(7.5, 3.0), (2.5, 3.0), (2.5, 13.5), (13.0, 13.5), (13.0, 8.5)], ink);
                p.line(&[(7.0, 9.0), (14.0, 2.0)], ink);
                p.line(&[(9.5, 2.0), (14.0, 2.0), (14.0, 6.5)], ink);
            }
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
            let blade = p.c16(hex(0xC8C8C8));
            p.line(&[(6.3, 10.0), (11.0, 1.5)], blade);
            p.line(&[(9.7, 10.0), (5.0, 1.5)], blade);
            let loops = p.c16(hex(0xE05A6E));
            p.circle((4.8, 12.3), 2.5, loops);
            p.circle((11.2, 12.3), 2.5, loops);
        }
        Copy | SnapshotCopy => {
            if p.v16 && icon == Copy {
                // Blue sheet behind a white one.
                p.fill(2.0, 1.5, 8.0, 10.0, hex(0x5DB0E6));
                p.sheet(6.0, 4.5, 8.5, 10.5);
            } else if p.v16 {
                p.page(2.0, 1.5, 8.0, 10.0, true);
                p.page(6.0, 4.5, 8.0, 10.0, true);
            } else {
                // Two solid pages, the front one set off by a background gap.
                p.fill(2.0, 1.5, 8.0, 10.0, ink);
                p.fill(5.0, 3.5, 10.0, 12.0, p.bg());
                p.poly(&[(6.0, 4.5), (11.5, 4.5), (14.0, 7.0), (14.0, 15.0), (6.0, 15.0)], ink);
            }
        }
        Paste => {
            if p.v16 {
                // Green clipboard outline, white sheet on it.
                let g = hex(0x5AC878);
                p.rect(2.0, 3.0, 9.5, 11.5, g);
                p.fill(4.5, 1.0, 4.5, 3.0, g);
                p.sheet(7.0, 6.5, 7.5, 8.5);
            } else {
                // Clipboard: board outline, solid clip, three text lines.
                p.rect(2.5, 2.5, 11.0, 12.5, ink);
                p.fill(5.0, 1.0, 6.0, 3.0, ink);
                for y in [7.0, 9.5, 12.0] {
                    p.line(&[(5.0, y), (11.0, y)], ink);
                }
            }
        }
        Undo | Redo => {
            // Hook arrow: head pointing left at the top, the shaft bends round the right side and back
            // (Redo is mirrored). VP16 draws it bold, VP23 as a thin line.
            let k = if p.v16 { 1.5 } else { 1.0 };
            p.line_w(&[(4.5, 5.0), (9.0, 5.0)], ink, k);
            p.arc((9.0, 9.0), 4.0, 270.0, 450.0, ink, k);
            p.line_w(&[(9.0, 13.0), (5.0, 13.0)], ink, k);
            p.arrow_head_k((1.5, 5.0), (-1.0, 0.0), ink, if p.v16 { 1.3 } else { 1.1 });
        }
        Tutorials => {
            p.fill_round(4.0, 7.0, 7.0, 6.5, 2.0, t.icon);
            p.fill_round(5.0, 1.5, 2.2, 7.0, 1.0, t.icon);
            p.help_badge();
        }
        WhatsThis => {
            p.pointer(2.0, 1.0, 1.0, t.icon);
            p.help_badge();
        }
        Record => {
            if p.v16 {
                p.fill_round(5.5, 1.0, 5.0, 8.0, 2.5, hex(0xE0505A));
                p.arc((8.0, 6.5), 4.0, 0.0, 180.0, ink, 1.0);
                p.line(&[(8.0, 10.5), (8.0, 14.0)], ink);
                p.line(&[(5.0, 14.5), (11.0, 14.5)], ink);
            } else {
                p.dot((8.0, 8.0), 5.0, hex(0xFF7B5F));
            }
        }
        Loop if p.v16 => {
            // Circular arrow with a blue tick at the top (c_transport).
            p.arc((8.0, 9.0), 5.0, -60.0, 220.0, ink, 1.2);
            p.arrow_head((3.2, 5.6), (0.64, -0.77), ink);
            p.line_w(&[(8.0, 1.0), (8.0, 5.5)], p.c(hex(0x3C8CDC)), 1.3);
        }
        Loop => {
            p.line(&[(9.0, 4.0), (4.5, 4.0)], ink);
            p.arc((4.5, 8.0), 4.0, 270.0, 90.0, ink, 1.0);
            p.line(&[(7.0, 12.0), (11.5, 12.0)], ink);
            p.arc((11.5, 8.0), 4.0, 90.0, -90.0, ink, 1.0);
            p.arrow_head((12.0, 4.0), (1.0, 0.0), ink);
            p.arrow_head((4.0, 12.0), (-1.0, 0.0), ink);
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
        NormalEdit if !p.v16 => {
            // Plain outlined arrow (dark2026).
            p.closed(&[(3.5, 1.5), (3.5, 13.5), (6.5, 10.5), (9.0, 15.0), (10.8, 14.2), (8.5, 9.8), (12.5, 9.8)], ink);
        }
        EnvelopeEdit if !p.v16 => p.node_pair(1.0, ink),
        SelectionEdit if !p.v16 => p.dashed_rect(2.0, 2.0, 12.0, 12.0, 2.0, 1.5, ink),
        ZoomEdit if !p.v16 => {
            p.circle((7.0, 7.0), 5.0, ink);
            p.line(&[(10.6, 10.6), (14.5, 14.5)], ink);
        }
        NormalEdit => {
            p.pointer(2.0, 1.0, 0.95, t.icon);
            p.line(&[(11.0, 10.0), (10.0, 10.0), (10.0, 14.5), (11.0, 14.5)], ink);
            p.line(&[(14.0, 10.0), (15.0, 10.0), (15.0, 14.5), (14.0, 14.5)], ink);
        }
        EnvelopeEdit => {
            p.pointer(1.5, 0.5, 0.85, t.icon);
            let c = p.c16(hex(0x3C78C8));
            p.line(&[(7.5, 14.0), (11.0, 10.0), (15.0, 12.5)], c);
            p.fill(10.0, 9.0, 2.0, 2.0, ink);
            p.fill(14.0, 11.5, 2.0, 2.0, ink);
        }
        SelectionEdit => {
            p.pointer(1.5, 0.5, 0.85, t.icon);
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
            let c = p.c16(hex(0xE04040));
            p.line_w(&[(3.5, 3.5), (12.5, 12.5)], c, 1.4);
            p.line_w(&[(12.5, 3.5), (3.5, 12.5)], c, 1.4);
        }
        Trim => {
            p.fill(4.0, 5.5, 8.0, 5.0, ink);
            p.line(&[(3.0, 3.0), (1.0, 3.0), (1.0, 13.0), (3.0, 13.0)], ink);
            p.line(&[(13.0, 3.0), (15.0, 3.0), (15.0, 13.0), (13.0, 13.0)], ink);
        }
        TrimStart | TrimEnd => {
            let h = p.c16(hex(0xD04040));
            p.rect(1.5, 4.0, 6.5, 8.0, h);
            for k in 0..3 {
                let x = 1.5 + k as f32 * 2.5;
                p.line(&[(x, 12.0), ((x + 4.0).min(8.0), 12.0 - (x + 4.0).min(8.0) + x)], h);
            }
            p.fill(9.0, 2.0, 1.5, 12.0, ink);
            p.rect(11.5, 4.0, 3.0, 8.0, ink);
        }
        Split | SmartSplit => {
            if p.v16 {
                let b = hex(0x3C78C8);
                p.fill_round(1.0, 5.0, 6.0, 6.0, 2.0, b);
                p.fill_round(9.0, 5.0, 6.0, 6.0, 2.0, b);
            } else {
                p.rect_round(1.0, 5.0, 5.5, 6.0, 2.0, ink);
                p.rect_round(9.5, 5.0, 5.5, 6.0, 2.0, ink);
            }
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
            if p.v16 {
                p.fill(4.0, 8.0, 8.0, 6.0, hex(0xDCDCDC));
            } else {
                p.rect(4.0, 8.0, 8.0, 6.0, ink);
            }
        }
        InsertMarker => {
            // Speech-bubble tag: square flag with its tail down the left edge (olive 16, orange 23).
            let c = p.c(if p.v16 { hex(0xBDAB5A) } else { hex(0xF08C28) });
            p.fill(3.0, 2.5, 10.5, 8.5, c);
            p.poly(&[(3.0, 11.0), (6.5, 11.0), (3.0, 14.5)], c);
        }
        InsertRegion if p.v16 => {
            // Green double flag: one block on two legs with a notch between them (c_transport).
            let c = p.c(hex(0x79B27B));
            p.fill(2.0, 2.5, 12.0, 6.5, c);
            p.fill(2.0, 9.0, 2.0, 5.5, c);
            p.fill(12.0, 9.0, 2.0, 5.5, c);
            p.poly(&[(6.5, 9.0), (9.5, 9.0), (8.0, 7.0)], p.bg());
        }
        InsertRegion => {
            // Two small green flags on short poles (dark2026).
            let c = p.c(hex(0x28DC64));
            p.fill(1.5, 5.0, 5.5, 4.5, c);
            p.fill(1.5, 5.0, 1.0, 6.5, c);
            p.fill(9.0, 5.0, 5.5, 4.5, c);
            p.fill(13.5, 5.0, 1.0, 6.5, c);
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
        AutoCrossfade if !p.v16 => {
            // Bow tie: solid left triangle, outlined right one meeting at the centre.
            p.poly(&[(1.5, 2.5), (8.0, 8.0), (1.5, 13.5)], ink);
            p.closed(&[(14.5, 2.5), (8.0, 8.0), (14.5, 13.5)], ink);
            p.line(&[(1.5, 2.5), (14.5, 13.5)], ink);
        }
        AutoCrossfade => {
            p.rect(1.0, 3.0, 14.0, 10.0, ink);
            let c = p.c16(hex(0x3C78C8));
            p.line(&[(1.0, 13.0), (15.0, 3.0)], c);
            p.line(&[(1.0, 3.0), (15.0, 13.0)], c);
        }
        AutoRipple => {
            for (x, y) in [(1.0, 3.0), (4.5, 7.5), (8.0, 12.0)] {
                p.line(&[(x, y), (x + 5.0, y)], ink);
                p.arrow_head((x + 7.0, y), (1.0, 0.0), ink);
            }
        }
        LockEnvelopes if !p.v16 => {
            // Node pair with a padlock at the lower right.
            p.node_pair(0.8, ink);
            p.arc((12.0, 9.5), 1.8, 180.0, 360.0, ink, 0.9);
            p.fill(9.5, 9.5, 5.0, 5.0, ink);
        }
        LockEnvelopes => {
            p.rect(1.0, 8.0, 11.0, 6.0, ink);
            p.line(&[(1.0, 12.5), (5.0, 10.0), (12.0, 11.0)], p.c16(hex(0x3C78C8)));
            p.arc((13.0, 3.5), 1.6, 180.0, 360.0, ink, 0.8);
            p.fill(10.8, 3.5, 4.4, 3.5, ink);
        }
        IgnoreGrouping => {
            // Two grouped boxes (one solid, one dotted) and a pointer over them (c_transport, dark2026).
            p.fill(1.0, 2.0, 6.0, 6.0, p.c16(hex(0x5DB0E6)));
            p.dashed_rect(8.5, 2.0, 6.0, 6.0, 1.0, 1.0, ink);
            p.pointer(8.0, 6.5, 0.7, t.icon);
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
            if p.v16 {
                for y in [3.0, 7.0, 11.0] {
                    p.fill(3.0, y, 10.0, 2.0, ink);
                }
            } else {
                for x in [3.0, 7.0, 11.0] {
                    p.fill(x, 7.0, 2.0, 2.0, ink);
                }
            }
        }
        Mute | Solo => {
            let size = t.font.mute_solo;
            if p.v16 {
                let mute = icon == Mute;
                p.dot((8.0, 8.0), 7.5, if mute { t.mute_badge } else { t.solo_badge });
                let fg = if mute { hex(0xFFFFFF) } else { hex(0x1E1E1E) };
                p.text((8.0, 8.0), if mute { "M" } else { "S!" }, size, fonts::BOLD, fg);
                if mute {
                    // Small circle-slash on the top-right of the badge (c_trackhdr).
                    p.dot((12.5, 3.5), 3.2, hex(0xFFFFFF));
                    p.circle((12.5, 3.5), 2.2, hex(0x3C3C3C));
                    p.line_w(&[(11.0, 5.0), (14.0, 2.0)], hex(0x3C3C3C), 0.6);
                }
            } else {
                p.text((8.0, 8.0), if icon == Mute { "M" } else { "S" }, size, fonts::BOLD, ink);
            }
        }
        TrackFx if !p.v16 => p.text((8.0, 8.0), "fx", 13.0, fonts::BOLD, ink),
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
            if p.v16 {
                // Upright film frame with sprocket holes down both sides (c_trackhdr).
                p.fill(3.0, 1.0, 10.0, 14.0, ink);
                for y in [2.5, 6.5, 10.5] {
                    for x in [4.0, 10.5] {
                        p.p.rect_filled(p.r(x, y, 1.5, 2.0), 0.0, hex(0x3C3C3C));
                    }
                }
            } else {
                p.rect(2.0, 2.0, 12.0, 12.0, ink);
                p.line(&[(8.0, 2.0), (8.0, 14.0)], ink);
                p.line(&[(2.0, 8.0), (14.0, 8.0)], ink);
            }
        }
        TrackAudio => {
            if p.v16 {
                p.speaker(3.0, ink);
            } else {
                p.dot((5.5, 12.0), 2.5, ink);
                p.line(&[(7.8, 12.0), (7.8, 2.0), (12.5, 4.5), (12.5, 6.5)], ink);
            }
        }
        ExpandKeyframes => {
            p.line(&[(4.5, 4.0), (8.0, 7.0), (11.5, 4.0)], ink);
            p.line(&[(4.5, 8.0), (8.0, 11.0), (11.5, 8.0)], ink);
        }
        FaderThumb => {
            let (x, w) = if p.v16 { (0.0, 16.0) } else { (2.0, 12.0) };
            p.fill(x, 2.0, w, 12.0, t.fader_thumb);
            p.fill(6.5, 5.0, 1.0, 6.0, t.fader_thumb_bar);
            p.fill(8.5, 5.0, 1.0, 6.0, t.fader_thumb_bar);
        }
        PanThumb => p.poly(&[(3.0, 4.0), (13.0, 4.0), (8.0, 12.0)], t.fader_thumb),
        Downmix => {
            let a = p.c16(hex(0xF0C800));
            p.line(&[(8.0, 2.0), (8.0, 14.0)], ink);
            p.line(&[(1.0, 8.0), (4.5, 8.0)], a);
            p.arrow_head((7.0, 8.0), (1.0, 0.0), a);
            p.line(&[(15.0, 8.0), (11.5, 8.0)], a);
            p.arrow_head((9.0, 8.0), (-1.0, 0.0), a);
        }
        Dim => {
            p.speaker(1.0, ink);
            p.poly(&[(10.0, 9.0), (15.0, 9.0), (12.5, 13.0)], p.c16(hex(0x3C78C8)));
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
            p.pointer(1.0, 3.0, 0.9, t.icon);
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
            // Original Milan Pro monogram (never the VEGAS "V").
            let r = p.r(0.0, 0.0, 16.0, 16.0);
            p.p.rect_filled(r, CornerRadius::same((3.0 * p.s) as u8), p.c(t.logo_bg));
            p.p.rect_stroke(r, CornerRadius::same((3.0 * p.s) as u8), Stroke::new(1.0_f32, p.c(t.logo_border)), StrokeKind::Inside);
            p.line_w(&[(4.0, 12.0), (4.0, 4.5), (8.0, 9.5), (12.0, 4.5), (12.0, 12.0)], t.logo_text, 1.1);
        }
        DropDown => p.poly(&[(5.5, 6.5), (10.5, 6.5), (8.0, 9.5)], ink),
        RemoveUnused => {
            p.page(2.0, 1.5, 9.0, 11.0, p.v16);
            for y in [4.5, 7.0, 9.5] {
                p.line_w(&[(4.0, y), (9.0, y)], if p.v16 { hex(0x3C3C3C) } else { ink }, 0.7);
            }
            let c = p.c16(hex(0xE04040));
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
            p.fill(1.0, 2.0, 10.0, 7.5, p.c16(hex(0xF0F0F0)));
            p.poly(&[(11.0, 4.0), (14.5, 2.0), (14.5, 9.5), (11.0, 7.5)], p.c16(hex(0xF0F0F0)));
            let r = p.c16(hex(0xE0506E));
            p.line_w(&[(5.0, 8.5), (10.5, 13.0)], r, 1.6);
            p.arrow_head_k((14.0, 15.5), (0.707, 0.707), r, 1.2);
        }
        WebMedia => {
            if p.v16 {
                // White globe with grey meridians and a red arrow coming down into it.
                p.dot((7.0, 8.5), 6.0, hex(0xF0F0F0));
                let g = hex(0x8A8A8A);
                p.line_w(&[(1.5, 8.5), (12.5, 8.5)], g, 0.7);
                p.p.add(Shape::ellipse_stroke(p.at((7.0, 8.5)), vec2(2.4, 6.0) * p.s, Stroke::new(0.7 * p.w, p.c(g))));
                let r = hex(0xE0506E);
                p.line_w(&[(9.0, 9.0), (12.5, 12.5)], r, 1.6);
                p.arrow_head_k((15.0, 15.0), (0.707, 0.707), r, 1.2);
            } else {
                p.circle((8.0, 8.0), 6.0, ink);
                p.line(&[(2.0, 8.0), (14.0, 8.0)], ink);
                p.p.add(Shape::ellipse_stroke(p.at((8.0, 8.0)), vec2(2.6, 6.0) * p.s, Stroke::new(p.w, p.c(ink))));
            }
        }
        Import => {
            // Sheet with a blue arrow coming in from the upper left.
            p.sheet(5.0, 1.5, 10.0, 13.5);
            let a = p.c16(hex(0x2E8BE6));
            p.line_w(&[(1.0, 3.0), (6.5, 8.5)], a, if p.v16 { 1.8 } else { 1.0 });
            p.arrow_head_k((10.0, 12.0), (0.707, 0.707), a, if p.v16 { 1.4 } else { 1.0 });
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
            let c = p.c16(hex(0x3CA050));
            p.line_w(&[(13.5, 8.0), (5.0, 8.0)], c, 1.3);
            p.arrow_head((2.5, 8.0), (-1.0, 0.0), c);
        }
        Up => {
            let c = p.c16(hex(0x3CA050));
            p.line_w(&[(8.0, 14.0), (8.0, 5.5)], c, 1.3);
            p.arrow_head((8.0, 2.5), (0.0, -1.0), c);
        }
        Refresh => {
            let c = p.c16(hex(0x3CA050));
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
            p.closed(&pts, p.c16(hex(0xF0C800)));
        }
        FxBadge => {
            if p.v16 {
                p.fill(2.0, 3.0, 12.0, 10.0, hex(0x3C8CDC));
                p.text((8.0, 8.0), "fx", 9.0, fonts::BOLD, hex(0xFFFFFF));
            } else {
                p.text((8.0, 8.0), "fx", 11.0, fonts::BOLD, ink);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{self, LayerId, pos2};

    #[test]
    fn every_icon_paints_in_every_look() {
        let ctx = egui::Context::default();
        super::super::fonts::install(&ctx);
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let painter = ctx.layer_painter(LayerId::background());
            for look in [UiLook::Vegas16, UiLook::Vegas23] {
                for (i, &icon) in Icon::ALL.iter().enumerate() {
                    let r = Rect::from_min_size(pos2(i as f32 * 20.0, 0.0), vec2(16.0, 16.0));
                    paint_icon(&painter, r, icon, look, None);
                    paint_icon(&painter, r, icon, look, Some(Color32::GRAY));
                }
            }
        });
    }
}
