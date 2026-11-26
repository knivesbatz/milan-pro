//! The look: colour tokens, metrics and font sizes (docs/UI_SPEC.md), mapped onto egui::Visuals. Drawing code reads
//! `theme::get()` and never hard-codes colours.
use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Shadow, Stroke, TextStyle, Visuals};
use std::sync::{LazyLock, RwLock};

/// Brushed-metal and 3D-bevel colours (UI_SPEC.md §2).
#[derive(Clone, Copy, Debug)]
pub struct Metal {
    /// Vertical gradient of every bar: top, middle (45 %), bottom.
    pub hi: Color32,
    pub mid: Color32,
    pub lo: Color32,
    /// Classic 3D edges: outer light / inner face on the top-left, inner shadow / outer dark on the bottom-right.
    pub light: Color32,
    pub face: Color32,
    pub shadow: Color32,
    pub dark: Color32,
    /// Opacity of the brushed streak texture over the gradient (0 = plain gradient).
    pub streak: f32,
    /// Blue-grey tint of the track headers (gradient multiplied by it).
    pub header_tint: Color32,
    /// Specular line under a raised button's top edge.
    pub specular: Color32,
    /// Fill of a pressed-in (active) button, over the sunken face.
    pub pressed: Color32,
}

/// Snap-indicator colour per snap target type (VEGAS_UI_SPEC.md §2.1 snap table).
#[derive(Clone, Copy, Debug)]
pub struct SnapColors {
    pub cursor: Color32,
    pub envelope_point: Color32,
    pub loop_: Color32,
    pub drag_origin: Color32,
    pub marker: Color32,
    pub region: Color32,
    pub cd_index: Color32,
    pub cd_track: Color32,
    pub command_marker: Color32,
    pub event: Color32,
    pub event_fade: Color32,
    pub grid: Color32,
}

/// Pixel sizes (UI_SPEC.md §4). Points == px at 100 % scaling.
#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    /// The in-window menu row.
    pub menu_bar_h: f32,
    pub main_toolbar_h: f32,
    pub main_toolbar_bevel: f32,
    pub dock_toolbar_h: f32,
    pub tab_strip_h: f32,
    pub splitter_w: f32,
    /// Outer client gutter on the left/right/bottom.
    pub outer_margin: f32,
    /// Share of the client height taken by the dock area.
    pub dock_frac: f32,
    /// Default dock height (toolbars + content + tab strip) at 1920×1080, measured (§4.1/§4.2).
    pub dock_h: f32,
    pub master_bus_w: f32,
    pub track_list_w: f32,
    /// Gap between the track list and the timeline.
    pub timeline_gap: f32,
    pub color_strip_w: f32,
    pub time_display_h: f32,
    /// Marker bar + loop bar + ruler band, same height as the time display.
    pub band_h: f32,
    pub loop_bar_h: f32,
    pub marker_bar_h: f32,
    pub ruler_h: f32,
    pub video_track_h: f32,
    pub audio_track_h: f32,
    pub event_header_h: f32,
    pub transport_h: f32,
    pub status_h: f32,
    /// Frame-coloured gap above the status bar.
    pub status_gap: f32,
    pub scrollbar_w: f32,
    pub hscroll_h: f32,
    pub button: f32,
    pub button_pitch: f32,
    pub icon: f32,
    pub icon_stroke: f32,
    pub widget_radius: u8,
    pub popup_row_h: f32,
    pub popup_gutter: f32,
}

/// Text sizes (UI_SPEC.md §3), in points.
#[derive(Clone, Copy, Debug)]
pub struct FontSizes {
    pub ui: f32,
    pub menu: f32,
    pub dock_tab: f32,
    pub label: f32,
    pub track_number: f32,
    pub track_number_bold: bool,
    pub mute_solo: f32,
    pub event_name: f32,
    pub ruler: f32,
    pub time_display: f32,
    pub transport_time: f32,
    pub status: f32,
    pub meter: f32,
}

/// Every colour token of UI_SPEC.md §2 under one name set.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub m: Metrics,
    pub font: FontSizes,

    // Menus
    pub menu_text: Color32,
    pub menu_popup_bg: Color32,
    pub menu_popup_border: Color32,
    pub menu_popup_hover: Color32,
    pub menu_popup_text: Color32,
    pub menu_popup_text_disabled: Color32,

    // Frame, toolbars, panels
    pub frame: Color32,
    pub toolbar_bg: Color32,
    pub line_dark: Color32,
    pub panel_bg: Color32,
    pub band_bg: Color32,
    pub lane_bg: Color32,
    pub empty_lane_bg: Color32,

    // Dock tabs
    pub tab_text: Color32,
    pub tab_text_active: Color32,
    pub tab_text_hover: Color32,

    // Text and icons
    pub text: Color32,
    pub text_secondary: Color32,
    pub text_label: Color32,
    pub text_value: Color32,
    pub text_bright: Color32,
    pub text_dim: Color32,
    pub text_disabled: Color32,
    pub icon: Color32,
    pub icon_disabled: Color32,
    pub input_bg: Color32,
    pub input_placeholder: Color32,
    pub tooltip_bg: Color32,
    pub tooltip_text: Color32,
    pub tooltip_border: Color32,

    // Buttons
    pub button_active_bg: Color32,
    pub button_hover_bg: Color32,
    pub accent: Color32,

    // Track headers
    pub track_header_bg: Color32,
    pub track_header_top: Color32,
    pub track_separator: Color32,
    pub track_separator_2: Color32,
    pub strip_divider: Color32,

    // Time display, ruler, scrollbars
    pub time_display_bg: Color32,
    pub time_display_text: Color32,
    pub ruler_text: Color32,
    pub ruler_major_tick: Color32,
    pub ruler_minor_tick: Color32,
    pub grid_line: Color32,
    pub scroll_track: Color32,
    pub scroll_arrow: Color32,

    // Loop bar, selection, markers, regions
    pub loop_sel_line: Color32,
    pub loop_flag: Color32,
    pub loop_flag_shade: Color32,
    pub loop_bar_on: Color32,
    pub selection_overlay: Color32,
    pub length_tag_bg: Color32,
    pub length_tag_text: Color32,
    pub marker_flag: Color32,
    pub marker_text: Color32,
    pub region_flag: Color32,
    pub region_flag_hi: Color32,
    pub region_edge: Color32,
    pub region_text: Color32,
    pub region_line: Color32,

    // Cursor / playhead
    pub playhead_line: Color32,
    /// VP16 inverse-cursor colour where the lane under it is dark-on-light.
    pub playhead_line_inverted: Color32,
    pub playhead_outline: Color32,
    pub playhead_head: Color32,

    // Events
    pub event_selected_border: Color32,
    pub event_selected_border_ripple: Color32,
    /// Event names in the header bar.
    pub event_text: Color32,
    pub fade_curve: Color32,
    pub fade_curve_selected: Color32,
    pub fade_curve_in_xfade: Color32,
    pub fade_region_tint: Color32,
    pub fade_handle: Color32,
    pub fade_handle_2: Color32,
    pub snap_line: Color32,
    pub snap: SnapColors,
    pub checker_a: Color32,
    pub checker_b: Color32,

    // Envelopes
    pub envelope_volume: Color32,
    pub envelope_composite: Color32,
    pub envelope_pan: Color32,
    pub envelope_mute: Color32,
    pub envelope_point: Color32,

    // Pan/Crop and Track Motion (§5.15.2–3): light workspace, keyframe controller
    pub workspace_bg: Color32,
    pub workspace_grid: Color32,
    pub workspace_frame: Color32,
    pub kf_ruler: Color32,
    pub kf_lane: Color32,
    pub kf_lane_alt: Color32,
    pub kf_row_sel: Color32,

    // Meters and faders
    pub meter_bg: Color32,
    pub meter_green: Color32,
    pub meter_yellow: Color32,
    pub meter_red: Color32,
    pub meter_label: Color32,
    pub fader_track: Color32,
    pub fader_thumb: Color32,
    pub fader_thumb_bar: Color32,

    // Preview, status, misc
    pub warning_text: Color32,
    pub preview_bg: Color32,
    pub preview_frame: Color32,

    /// Default track colour palette, assigned to tracks in order, then repeating.
    pub track_colors: [Color32; 8],

    /// Metal and bevel colours.
    pub metal: Metal,
}

const fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

fn hexa(v: u32, alpha: f32) -> Color32 {
    // Gamma-space premultiply: egui 0.31's from_rgba_unmultiplied goes through linear space and
    // roughly doubles low-alpha tints compared with how VEGAS blends.
    hex(v).gamma_multiply(alpha)
}

impl Theme {
    /// Track colour for the n-th track (video tracks first, then audio).
    pub fn track_color(&self, n: usize) -> Color32 {
        self.track_colors[n % self.track_colors.len()]
    }

    /// The text face: Tahoma (Segoe UI falls in when it is missing).
    pub fn family(&self) -> FontFamily {
        if super::fonts::installed() { FontFamily::Name(super::fonts::TAHOMA.into()) } else { FontFamily::Proportional }
    }

    pub fn font(&self, size: f32) -> FontId {
        FontId::new(size, self.family())
    }

    /// The Windows-classic 3D palette of a `#D4D0C8` property sheet (UI_SPEC.md §2.5): the dialogs, their
    /// controls and the floating-window frames paint their bevels with it, through the same `widgets` painters as
    /// the metal bars. Flat (no streaks, no specular).
    pub fn dialog_metal(&self) -> Metal {
        let d = self.dialog();
        Metal {
            hi: d.body,
            mid: d.body,
            lo: d.body,
            light: hex(0xFFFFFF),
            face: d.body,
            shadow: hex(0x808080),
            dark: hex(0x404040),
            streak: 0.0,
            header_tint: hex(0xFFFFFF),
            specular: Color32::TRANSPARENT,
            pressed: self.metal.pressed,
        }
    }

    pub fn bold(&self, size: f32) -> FontId {
        if !super::fonts::installed() {
            return FontId::proportional(size);
        }
        FontId::new(size, FontFamily::Name(super::fonts::TAHOMA_BOLD.into()))
    }

    /// UI_SPEC.md: 2005–2007 NLE chrome (Vegas 6/7 proportions and track headers, Windows 2000/XP classic controls and
    /// menus, brushed-steel bars) on the VEGAS 23 layout. Native title bar, in-window menu row.
    pub fn new() -> Self {
        let m = Metrics {
            menu_bar_h: 20.0,
            main_toolbar_h: 26.0,
            main_toolbar_bevel: 2.0,
            dock_toolbar_h: 26.0,
            tab_strip_h: 21.0,
            splitter_w: 6.0,
            outer_margin: 2.0,
            dock_frac: 0.50,
            dock_h: 534.0,
            master_bus_w: 120.0,
            track_list_w: 250.0,
            timeline_gap: 6.0,
            color_strip_w: 20.0,
            time_display_h: 40.0,
            band_h: 42.0, // loop 6 · markers 14 · ruler 18 · lines
            loop_bar_h: 6.0,
            marker_bar_h: 14.0,
            ruler_h: 18.0,
            video_track_h: 56.0,
            audio_track_h: 56.0,
            event_header_h: 16.0,
            transport_h: 28.0,
            status_h: 20.0,
            status_gap: 2.0,
            scrollbar_w: 16.0,
            hscroll_h: 16.0,
            button: 22.0,
            button_pitch: 23.0,
            icon: 16.0,
            icon_stroke: 1.0,
            widget_radius: 0,
            popup_row_h: 19.0,
            popup_gutter: 24.0,
        };
        // Tahoma 8 pt = 11 px; 7 pt badges and meter labels; 15 pt bold time display.
        let font = FontSizes {
            ui: 11.0,
            menu: 11.0,
            dock_tab: 11.0,
            label: 11.0,
            track_number: 11.0,
            track_number_bold: true,
            mute_solo: 9.0,
            event_name: 11.0,
            ruler: 10.0,
            time_display: 20.0,
            transport_time: 11.0,
            status: 11.0,
            meter: 8.0,
        };
        let metal = Metal {
            hi: hex(0xE4E6E9),
            mid: hex(0xB9BDC3),
            lo: hex(0x8F949B),
            light: hex(0xFFFFFF),
            face: hex(0xD4D6DA),
            shadow: hex(0x6E737A),
            dark: hex(0x2E3136),
            streak: 1.0,
            header_tint: hex(0xC4D0DC),
            specular: hexa(0xFFFFFF, 0.55),
            pressed: hexa(0x00AEEF, 0.35),
        };
        let (black, white, grey, dim) = (hex(0x000000), hex(0xFFFFFF), hex(0x808080), hex(0x5C6168));
        let accent = hex(0x0096E0);
        Self {
            m,
            font,

            menu_text: black,
            // Windows XP (Luna) menus: white, 3D grey border, #316AC5 highlight bar.
            menu_popup_bg: white,
            menu_popup_border: grey,
            menu_popup_hover: hex(0x316AC5),
            menu_popup_text: black,
            menu_popup_text_disabled: grey,

            frame: hex(0x5C6168),
            toolbar_bg: metal.mid,
            line_dark: metal.shadow,
            panel_bg: hex(0xC9CCD1),
            band_bg: hex(0xAEB3BA),
            lane_bg: hex(0x3C4047),
            empty_lane_bg: hex(0x2F3339),

            tab_text: hex(0x2B2E33),
            tab_text_active: black,
            tab_text_hover: black,

            text: black,
            text_secondary: hex(0x1E2226),
            text_label: hex(0x2B2E33),
            text_value: black,
            text_bright: white,
            text_dim: dim,
            text_disabled: grey,
            icon: hex(0x0A0A0A),
            icon_disabled: grey,
            input_bg: white,
            input_placeholder: grey,
            tooltip_bg: hex(0xFFFFE1),
            tooltip_text: black,
            tooltip_border: black,

            button_active_bg: hex(0x9CD3F0),
            button_hover_bg: metal.hi,
            accent,

            track_header_bg: hex(0x9AA6B3),
            track_header_top: metal.light,
            track_separator: hex(0x4A4E55),
            track_separator_2: hex(0x7A7F86),
            strip_divider: metal.dark,

            // Black LCD well with cyan digits (Vegas 5–7 time display).
            time_display_bg: hex(0x14181E),
            time_display_text: hex(0x5CE0FF),
            ruler_text: black,
            ruler_major_tick: black,
            ruler_minor_tick: hex(0x3C4047),
            grid_line: hex(0x50555C),
            scroll_track: hex(0xA9AEB5),
            scroll_arrow: black,

            loop_sel_line: accent,
            loop_flag: hex(0xFFD800),
            loop_flag_shade: hex(0xB09400),
            loop_bar_on: accent,
            selection_overlay: hexa(0x00AEEF, 0.18),
            length_tag_bg: white,
            length_tag_text: black,
            marker_flag: hex(0xE08A2A),
            marker_text: white,
            region_flag: hex(0x3C9A50),
            region_flag_hi: hex(0x6FBF80),
            region_edge: hex(0x2E7A40),
            region_text: white,
            region_line: hex(0x3C9A50),

            playhead_line: hex(0x00C8FF),
            playhead_line_inverted: hex(0x00C8FF),
            playhead_outline: hex(0x00304A),
            playhead_head: white,

            event_selected_border: hex(0x00C8FF),
            event_selected_border_ripple: hex(0xE03030),
            event_text: white,
            fade_curve: hex(0x0000C8),
            fade_curve_selected: white,
            fade_curve_in_xfade: hex(0x0000C8),
            fade_region_tint: hexa(0x000000, 0.30),
            fade_handle: hex(0x0050FF),
            fade_handle_2: hex(0x0030A0),
            snap_line: hex(0x00C8FF),
            snap: SNAP,
            checker_a: hex(0x9A9A9A),
            checker_b: hex(0xC8C8C8),

            envelope_volume: hex(0x0000C8),
            envelope_composite: hex(0x0000C8),
            envelope_pan: hex(0xC00000),
            envelope_mute: hex(0x00C8FF),
            envelope_point: white,

            workspace_bg: hex(0xECECEC),
            workspace_grid: hex(0xC8C8C8),
            workspace_frame: hex(0x505050),
            kf_ruler: hex(0xAEB3BA),
            kf_lane: hex(0xC9CCD1),
            kf_lane_alt: hex(0xBEC2C7),
            kf_row_sel: hex(0xB4CFE4),

            meter_bg: hex(0x14181E),
            meter_green: hex(0x00E020),
            meter_yellow: hex(0xF0E000),
            meter_red: hex(0xFF2020),
            meter_label: black,
            fader_track: metal.shadow,
            fader_thumb: metal.face,
            fader_thumb_bar: metal.dark,

            warning_text: hex(0xA00000),
            preview_bg: hex(0x3C4047),
            preview_frame: black,

            // Vegas 6 event pastels [I]: steel blue, rose, sage, sand, lavender, teal, clay, blue-grey.
            track_colors: [
                hex(0x6E8EC8),
                hex(0xC86E6E),
                hex(0x78B478),
                hex(0xC8B46E),
                hex(0x9678C8),
                hex(0x6EB4B4),
                hex(0xC8966E),
                hex(0x8CA0B4),
            ],
            metal,
        }
    }

    /// The tokens mapped onto egui::Visuals.
    pub fn visuals(&self) -> Visuals {
        let mut v = Visuals::light();
        v.override_text_color = Some(self.text);
        v.panel_fill = self.panel_bg;
        v.window_fill = self.panel_bg;
        v.faint_bg_color = self.band_bg;
        v.extreme_bg_color = self.input_bg;
        v.code_bg_color = self.input_bg;
        v.warn_fg_color = self.warning_text;
        v.error_fg_color = self.warning_text;
        v.hyperlink_color = self.accent;
        v.window_shadow = Shadow::NONE;
        v.popup_shadow = Shadow::NONE;
        let r = CornerRadius::same(self.m.widget_radius);
        v.window_corner_radius = CornerRadius::ZERO;
        v.menu_corner_radius = r;
        v.selection.bg_fill = self.accent;

        let w = &mut v.widgets;
        w.noninteractive.bg_fill = self.panel_bg;
        w.noninteractive.weak_bg_fill = self.panel_bg;
        // Windows-classic controls: 3D grey faces with a shadow outline, black ink, a blue selection.
        let m = &self.metal;
        w.noninteractive.bg_stroke = Stroke::new(1.0_f32, m.shadow);
        w.noninteractive.fg_stroke = Stroke::new(1.0_f32, self.text);
        w.inactive.bg_fill = m.face;
        w.inactive.weak_bg_fill = m.face;
        w.inactive.bg_stroke = Stroke::new(1.0_f32, m.shadow);
        w.inactive.fg_stroke = Stroke::new(1.0_f32, self.text);
        w.hovered.bg_fill = m.hi;
        w.hovered.weak_bg_fill = m.hi;
        w.hovered.bg_stroke = Stroke::new(1.0_f32, m.shadow);
        w.hovered.fg_stroke = Stroke::new(1.0_f32, self.text);
        w.active.bg_fill = self.button_active_bg;
        w.active.weak_bg_fill = self.button_active_bg;
        w.active.bg_stroke = Stroke::NONE;
        w.active.fg_stroke = Stroke::new(1.0_f32, self.text);
        w.open.bg_fill = self.button_hover_bg;
        w.open.weak_bg_fill = self.button_hover_bg;
        w.open.bg_stroke = Stroke::new(1.0_f32, m.shadow);
        w.open.fg_stroke = Stroke::new(1.0_f32, self.text);
        for s in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
            s.corner_radius = r;
            s.expansion = 0.0;
        }
        w.noninteractive.corner_radius = CornerRadius::ZERO;
        v.selection.stroke = Stroke::new(1.0_f32, self.text_bright);
        v.window_stroke = Stroke::new(1.0_f32, m.shadow);
        v
    }

    /// Visuals, spacing and text sizes on the whole context.
    pub fn apply(&self, ctx: &egui::Context) {
        let f = self.font;
        let fam = self.family();
        ctx.style_mut(|s| {
            s.visuals = self.visuals();
            s.text_styles = [
                (TextStyle::Small, FontId::new(9.0, fam.clone())),
                (TextStyle::Body, FontId::new(f.ui, fam.clone())),
                (TextStyle::Button, FontId::new(f.ui, fam.clone())),
                (TextStyle::Heading, FontId::new(f.ui + 2.0, fam.clone())),
                (TextStyle::Monospace, FontId::monospace(f.ui)),
            ]
            .into();
            let sp = &mut s.spacing;
            sp.button_padding = egui::vec2(4.0, 3.0);
            sp.item_spacing = egui::vec2(4.0, 2.0);
            sp.interact_size.y = 20.0;
            sp.menu_margin = egui::Margin::same(2);
            sp.scroll.bar_width = 16.0;
            sp.scroll.floating = false;
            // A grey 3D-face handle on the white rail.
            sp.scroll.foreground_color = false;
        });
    }
}

const SNAP: SnapColors = SnapColors {
    cursor: hex(0xFFE864),
    envelope_point: hex(0xC85AFA),
    loop_: hex(0x4040A0),
    drag_origin: hex(0xFF9B50),
    marker: hex(0xE4AA81),
    region: hex(0x7FB68C),
    cd_index: hex(0xCE81E4),
    cd_track: hex(0xE48181),
    command_marker: hex(0x5989FF),
    event: hex(0x4FC2EE),
    event_fade: hex(0xFC6AFF),
    grid: hex(0xF4FFEE),
};

/// Modal dialog colours: a Windows 2000/XP-classic property sheet.
#[derive(Clone, Copy, Debug)]
pub struct DialogColors {
    pub body: Color32,
    pub page: Color32,
    pub field: Color32,
    pub field_border: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_disabled: Color32,
    pub button: Color32,
    pub button_border: Color32,
    pub button_hover: Color32,
    pub button_hover_border: Color32,
    /// Group heading rule: dark line over a light one.
    pub etch: (Color32, Color32),
    pub accent: Color32,
    pub accent_text: Color32,
    /// Caption bar gradient (left → right) and its text: the Windows 2000 active caption.
    pub caption: (Color32, Color32),
    pub caption_text: Color32,
}

impl Theme {
    pub fn dialog(&self) -> DialogColors {
        // #D4D0C8 face, white fields, #808080 / white etched rules.
        DialogColors {
            body: hex(0xD4D0C8),
            page: hex(0xD4D0C8),
            field: hex(0xFFFFFF),
            field_border: hex(0x808080),
            border: hex(0x808080),
            text: hex(0x000000),
            text_disabled: hex(0x808080),
            button: hex(0xD4D0C8),
            button_border: hex(0x404040),
            button_hover: hex(0xE4E2DC),
            button_hover_border: hex(0x000000),
            etch: (hex(0x808080), hex(0xFFFFFF)),
            accent: hex(0x316AC5),
            accent_text: hex(0xFFFFFF),
            caption: (hex(0x0A246A), hex(0xA6CAF0)),
            caption_text: hex(0xFFFFFF),
        }
    }
}

static CURRENT: LazyLock<RwLock<Theme>> = LazyLock::new(|| RwLock::new(Theme::new()));
/// Track colours edited in Preferences › Display; replaces the default palette when set.
static PALETTE: RwLock<Option<[Color32; 8]>> = RwLock::new(None);

pub fn set_palette(ctx: &egui::Context, palette: Option<[Color32; 8]>) {
    *PALETTE.write().unwrap() = palette;
    apply(ctx);
}

/// The active theme (a copy; cheap enough per panel per frame).
pub fn get() -> Theme {
    *CURRENT.read().unwrap()
}

/// Stores the theme (with the user's track colours) and applies it to `ctx`.
pub fn apply(ctx: &egui::Context) {
    let mut t = Theme::new();
    if let Some(p) = *PALETTE.read().unwrap() {
        t.track_colors = p;
    }
    *CURRENT.write().unwrap() = t;
    t.apply(ctx);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Native title bar, in-window menu, Tahoma 8 pt, nothing rounded, brushed metal, XP menus.
    #[test]
    fn spec_values() {
        let c = Theme::new();
        assert!(c.m.menu_bar_h > 0.0, "in-window menu row");
        assert_eq!((c.font.ui, c.m.icon_stroke, c.m.widget_radius, c.metal.streak), (11.0, 1.0, 0, 1.0));
        assert_eq!((c.menu_popup_bg, c.menu_popup_hover, c.text), (hex(0xFFFFFF), hex(0x316AC5), hex(0x000000)));
        assert!(!c.visuals().dark_mode);
    }
}
