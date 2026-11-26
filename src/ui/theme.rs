//! Look switch ("Classic 2006" / "VEGAS 16" / "VEGAS 23" / "VEGAS 23 White"): colour tokens, metrics and font
//! sizes from docs/VEGAS_UI_SPEC.md §1.3, §2, §3 and §4 and docs/CLASSIC_2006_SPEC.md, mapped onto egui::Visuals
//! (§2.6). Drawing code reads `theme::get()` and never hard-codes colours.
use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Shadow, Stroke, TextStyle, Visuals};
use serde::{Deserialize, Serialize};
use std::sync::{LazyLock, RwLock};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum UiLook {
    Vegas16,
    Vegas23,
    /// 2005–2007 NLE chrome (CLASSIC_2006_SPEC.md) on the VEGAS 23 layout: the default for new installs.
    #[default]
    Classic2006,
}

impl UiLook {
    /// Follows the VEGAS 23 layout, docking areas, menus, toolbars and keys (Classic 2006 only changes the drawing).
    pub fn v23(self) -> bool {
        matches!(self, UiLook::Vegas23 | UiLook::Classic2006)
    }

    pub fn classic(self) -> bool {
        self == UiLook::Classic2006
    }

    /// The VEGAS look whose layout, menus, toolbar sets and keys this look follows.
    pub fn base(self) -> UiLook {
        if self.v23() { UiLook::Vegas23 } else { UiLook::Vegas16 }
    }
}

/// Interface type within a look. Only VP23 White is measured besides Dark (§2.3).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Variant {
    #[default]
    Dark,
    White,
}

/// What the user picks in View > Interface; persisted in prefs.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct LookChoice {
    pub look: UiLook,
    pub variant: Variant,
}

impl LookChoice {
    pub const VEGAS16: Self = Self { look: UiLook::Vegas16, variant: Variant::Dark };
    pub const VEGAS23: Self = Self { look: UiLook::Vegas23, variant: Variant::Dark };
    pub const VEGAS23_WHITE: Self = Self { look: UiLook::Vegas23, variant: Variant::White };
    pub const CLASSIC2006: Self = Self { look: UiLook::Classic2006, variant: Variant::Dark };
    /// Menu order and labels for View > Interface.
    pub const ALL: [(Self, &'static str); 4] =
        [(Self::CLASSIC2006, "Classic 2006"), (Self::VEGAS16, "VEGAS 16"), (Self::VEGAS23, "VEGAS 23"), (Self::VEGAS23_WHITE, "VEGAS 23 White")];

    /// `MILAN_LOOK` values: classic2006 | vegas16 | vegas23 | vegas23-white.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "classic2006" | "classic" | "2006" | "c2006" => Some(Self::CLASSIC2006),
            "vegas16" | "16" => Some(Self::VEGAS16),
            "vegas23" | "23" => Some(Self::VEGAS23),
            "vegas23-white" | "vegas23white" | "23-white" => Some(Self::VEGAS23_WHITE),
            _ => None,
        }
    }
}

/// Classic 2006 brushed-metal and 3D-bevel colours (CLASSIC_2006_SPEC.md §2); the VEGAS looks fill them with their
/// flat toolbar colour and never paint them.
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

/// Snap-indicator colour per snap target type (§2.1 snap table; VP23 reuses it).
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

/// Pixel sizes per look (§1.3, §4, §5). Points == px at 100 % scaling.
#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    /// Custom title bar with inline menu (VP23); 0 = native OS title bar (VP16).
    pub title_bar_h: f32,
    /// Separate menu row (VP16); 0 when the menu is inline in the title bar.
    pub menu_bar_h: f32,
    pub main_toolbar_h: f32,
    pub main_toolbar_bevel: f32,
    pub dock_toolbar_h: f32,
    pub tab_strip_h: f32,
    pub tab_active_line_w: f32,
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
    pub toggle_w: f32,
    pub toggle_h: f32,
    pub widget_radius: u8,
    pub popup_row_h: f32,
    pub popup_gutter: f32,
}

/// Text sizes per look (§3), in points.
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
    pub time_display_semibold: bool,
    pub transport_time: f32,
    pub status: f32,
    pub meter: f32,
}

/// Every colour token of §2 under one name set, so drawing code stays look-agnostic.
/// Tokens a look does not have are filled with its nearest equivalent.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub choice: LookChoice,
    pub dark_mode: bool,
    pub m: Metrics,
    pub font: FontSizes,

    // Title and menu bar
    pub title_bg: Color32,
    pub menu_bar_bg: Color32,
    pub menu_bar_bottom: Color32,
    pub menu_bar_line: Color32,
    pub menu_text: Color32,
    pub menu_bar_hover: Color32,
    pub menu_bar_hover_border: Color32,
    pub menu_bar_open: Color32,
    pub menu_popup_bg: Color32,
    pub menu_popup_border: Color32,
    pub menu_popup_hover: Color32,
    pub menu_popup_text: Color32,
    pub menu_popup_text_disabled: Color32,
    pub caption_hover: Color32,
    pub caption_close_hover: Color32,

    // Frame, toolbars, panels
    pub frame: Color32,
    pub toolbar_bg: Color32,
    pub bevel_hi_1: Color32,
    pub bevel_hi_2: Color32,
    pub dock_bevel_1: Color32,
    pub dock_bevel_2: Color32,
    pub line_dark: Color32,
    pub dock_divider: Color32,
    pub panel_bg: Color32,
    pub band_bg: Color32,
    pub lane_bg: Color32,
    pub empty_lane_bg: Color32,

    // Dock tabs
    pub tab_strip_bg: Color32,
    pub tab_strip_single_bg: Color32,
    pub tab_active_bg: Color32,
    pub tab_separator: Color32,
    pub tab_text: Color32,
    pub tab_text_active: Color32,
    pub tab_text_hover: Color32,
    pub tab_active_line: Color32,

    // Text and icons
    pub text: Color32,
    pub text_secondary: Color32,
    pub text_label: Color32,
    pub text_value: Color32,
    pub text_bright: Color32,
    pub text_dim: Color32,
    pub text_disabled: Color32,
    pub icon: Color32,
    pub icon_shadow: Color32,
    pub icon_disabled: Color32,
    /// VP16 "Icon color strength" (0.75): coloured icon pixels blend toward grey by 1 - this.
    pub icon_color_strength: f32,
    pub input_bg: Color32,
    pub input_placeholder: Color32,
    pub tooltip_bg: Color32,
    pub tooltip_text: Color32,
    pub tooltip_border: Color32,

    // Buttons
    pub button_active_bg: Color32,
    pub button_hover_bg: Color32,
    pub button_group_sep: Color32,
    pub accent: Color32,

    // Track headers
    pub track_header_bg: Color32,
    pub track_header_sel_bg: Color32,
    pub track_header_sel_top: Color32,
    pub track_header_sel_bottom: Color32,
    pub track_header_top: Color32,
    pub track_separator: Color32,
    pub track_separator_2: Color32,
    pub strip_divider: Color32,
    pub mute_badge: Color32,
    pub solo_badge: Color32,

    // Time display, ruler, scrollbars
    pub time_display_bg: Color32,
    pub time_display_text: Color32,
    pub ruler_text: Color32,
    pub ruler_major_tick: Color32,
    pub ruler_minor_tick: Color32,
    pub ruler_minor_shadow: Color32,
    pub band_separator: Color32,
    pub grid_line: Color32,
    pub scroll_track: Color32,
    pub scroll_thumb: Color32,
    pub scroll_arrow: Color32,
    pub native_scroll_bg: Color32,

    // Loop bar, selection, markers, regions
    pub loop_sel_line: Color32,
    pub loop_sel_edge: Color32,
    pub loop_flag: Color32,
    pub loop_flag_shade: Color32,
    pub loop_bar_off: Color32,
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

    // Cursor / playhead (VP16 cursor_line / cursor_shadow / cursor_head)
    pub playhead_line: Color32,
    /// VP16 inverse-cursor colour where the lane under it is dark-on-light.
    pub playhead_line_inverted: Color32,
    pub playhead_outline: Color32,
    pub playhead_head: Color32,

    // Events
    pub event_selected_border: Color32,
    pub event_selected_border_ripple: Color32,
    pub event_label_bg: Color32,
    pub event_label_border: Color32,
    pub event_chip_bg: Color32,
    /// Event names: VP16 header bar text, VP23 name label text.
    pub event_text: Color32,
    pub fade_curve: Color32,
    pub fade_curve_selected: Color32,
    pub fade_curve_in_xfade: Color32,
    pub fade_region_tint: Color32,
    pub fade_handle: Color32,
    pub fade_handle_2: Color32,
    pub snap_line: Color32,
    pub snap: SnapColors,
    /// VP16 1 px bevel lines around dock tabs, the time display, ruler band and transport bar.
    pub bevel: Bevels16,
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
    /// VP23 unlit wells: the meter gradient blended this far over `meter_bg` (0 = flat `meter_bg`).
    pub meter_unlit: f32,
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
    pub preset_tile_selected: Color32,
    pub status_bg: Color32,
    pub status_edge_1: Color32,
    pub status_edge_2: Color32,
    pub logo_bg: Color32,
    pub logo_border: Color32,
    pub logo_text: Color32,

    /// Default track colour palette, assigned to tracks in order, then repeating.
    pub track_colors: [Color32; 8],

    /// Classic 2006 metal and bevel colours (flat stand-ins in the VEGAS looks).
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
    pub fn new(choice: LookChoice) -> Self {
        match (choice.look, choice.variant) {
            (UiLook::Vegas16, _) => Self::vegas16_dark(),
            (UiLook::Vegas23, Variant::Dark) => Self::vegas23_dark(),
            (UiLook::Vegas23, Variant::White) => Self::vegas23_white(),
            (UiLook::Classic2006, _) => Self::classic2006(),
        }
    }

    pub fn look(&self) -> UiLook {
        self.choice.look
    }

    /// Classic 2006: the drawing switch (the layout switch is `look().v23()`).
    pub fn classic(&self) -> bool {
        self.look().classic()
    }

    /// Track colour for the n-th track (video tracks first, then audio).
    pub fn track_color(&self, n: usize) -> Color32 {
        self.track_colors[n % self.track_colors.len()]
    }

    /// The look's text face: Tahoma in Classic 2006 (Segoe UI falls in when it is missing), Segoe UI otherwise.
    pub fn family(&self) -> FontFamily {
        if self.classic() && super::fonts::installed() { FontFamily::Name(super::fonts::TAHOMA.into()) } else { FontFamily::Proportional }
    }

    pub fn font(&self, size: f32) -> FontId {
        FontId::new(size, self.family())
    }

    /// Corner radius of a small rounded highlight (hover boxes, chips, badges): none in Classic 2006, `v` otherwise.
    pub fn radius(&self, v: u8) -> CornerRadius {
        if self.classic() { CornerRadius::ZERO } else { CornerRadius::same(v) }
    }

    /// The Windows-classic 3D palette of a `#D4D0C8` property sheet (CLASSIC_2006_SPEC.md §2.5): the dialogs, their
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

    pub fn semibold(&self, size: f32) -> FontId {
        if self.classic() {
            return self.bold(size);
        }
        FontId::new(size, FontFamily::Name(super::fonts::SEMIBOLD.into()))
    }

    pub fn bold(&self, size: f32) -> FontId {
        if !super::fonts::installed() {
            return FontId::proportional(size);
        }
        let fam = if self.classic() { super::fonts::TAHOMA_BOLD } else { super::fonts::BOLD };
        FontId::new(size, FontFamily::Name(fam.into()))
    }

    /// §2.1, measured from a local VP15 install (VP16 kept the VP15 interface).
    pub fn vegas16_dark() -> Self {
        Self {
            choice: LookChoice::VEGAS16,
            dark_mode: true,
            m: Metrics {
                title_bar_h: 0.0,
                menu_bar_h: 20.0,
                main_toolbar_h: 31.0,
                main_toolbar_bevel: 2.0,
                dock_toolbar_h: 28.0,
                tab_strip_h: 17.0,
                tab_active_line_w: 0.0,
                splitter_w: 7.0,
                outer_margin: 3.0,
                dock_frac: 0.56,
                dock_h: 596.0,
                master_bus_w: 125.0,
                track_list_w: 251.0,
                timeline_gap: 4.0,
                color_strip_w: 24.0,
                time_display_h: 46.0,
                band_h: 48.0,
                loop_bar_h: 5.0,
                marker_bar_h: 14.0,
                ruler_h: 16.0,
                video_track_h: 64.0,
                audio_track_h: 68.0,
                event_header_h: 22.0,
                transport_h: 30.0,
                status_h: 17.0,
                status_gap: 0.0,
                scrollbar_w: 17.0,
                hscroll_h: 16.0,
                button: 22.0,
                button_pitch: 24.0,
                icon: 16.0,
                icon_stroke: 1.5,
                toggle_w: 21.0,
                toggle_h: 17.0,
                widget_radius: 0,
                popup_row_h: 22.0,
                popup_gutter: 28.0,
            },
            font: FontSizes {
                ui: 12.0,
                menu: 12.0,
                dock_tab: 12.0,
                label: 12.0,
                track_number: 14.0,
                track_number_bold: true,
                mute_solo: 9.0,
                event_name: 12.0,
                ruler: 10.0,
                time_display: 24.0,
                time_display_semibold: false,
                transport_time: 11.0,
                status: 12.0,
                meter: 9.0,
            },

            title_bg: hex(0xFFFFFF),
            menu_bar_bg: hex(0xFFFFFF),
            menu_bar_bottom: hex(0xF1F1F1),
            menu_bar_line: hex(0x7D7D7D),
            menu_text: hex(0x000000),
            menu_bar_hover: hex(0xE5F3FF),
            menu_bar_hover_border: hex(0xCCE8FF),
            menu_bar_open: hex(0xCCE8FF),
            menu_popup_bg: hex(0xF2F2F2),
            menu_popup_border: hex(0xCCCCCC),
            menu_popup_hover: hex(0x91C9F7),
            menu_popup_text: hex(0x000000),
            menu_popup_text_disabled: hex(0x6D6D6D),
            caption_hover: hex(0xE5E5E5),
            caption_close_hover: hex(0xC42B1C),

            frame: hex(0x232323),
            toolbar_bg: hex(0x464646),
            bevel_hi_1: hex(0x5C5C5C),
            bevel_hi_2: hex(0x606060),
            dock_bevel_1: hex(0x575757),
            dock_bevel_2: hex(0x616161),
            line_dark: hex(0x333333),
            dock_divider: hex(0x393939),
            panel_bg: hex(0x2D2D2D),
            band_bg: hex(0x373737),
            lane_bg: hex(0x2D2D2D),
            empty_lane_bg: hex(0x232323),

            tab_strip_bg: hex(0x373737),
            tab_strip_single_bg: hex(0x202020),
            tab_active_bg: hex(0x464646),
            tab_separator: hex(0x2D2D2D),
            tab_text: hex(0xDCDCDC),
            tab_text_active: hex(0xDCDCDC),
            tab_text_hover: hex(0xFFFFFF),
            tab_active_line: hex(0x464646),

            text: hex(0xDCDCDC),
            text_secondary: hex(0xC2C6C5),
            text_label: hex(0x959594),
            text_value: hex(0xC4C7C2),
            text_bright: hex(0xFFFFFF),
            text_dim: hex(0x959594),
            text_disabled: hex(0x6A6A6A),
            icon: hex(0xF0F0F0),
            icon_shadow: hex(0x1E1E1E),
            icon_disabled: hex(0x6A6A6A),
            icon_color_strength: 0.75,
            input_bg: hex(0x383838),
            input_placeholder: hex(0x4F5559),
            tooltip_bg: hex(0xFFFFFF),
            tooltip_text: hex(0x000000),
            tooltip_border: hex(0x767676),

            button_active_bg: hex(0x12B1FF),
            button_hover_bg: hex(0x555555),
            button_group_sep: hex(0x333333),
            accent: hex(0x12B1FF),

            track_header_bg: hex(0x2D2D2D),
            track_header_sel_bg: hex(0x738C9A),
            track_header_sel_top: hex(0x434D53),
            track_header_sel_bottom: hex(0x55656E),
            track_header_top: hex(0x272727),
            track_separator: hex(0x292929),
            track_separator_2: hex(0x272727),
            strip_divider: hex(0x242425),
            mute_badge: hex(0xE07080),
            solo_badge: hex(0xC8B060),

            time_display_bg: hex(0x464646),
            time_display_text: hex(0xDCDCDC),
            ruler_text: hex(0xB4B4B4),
            ruler_major_tick: hex(0x9B9B9B),
            ruler_minor_tick: hex(0x626262),
            ruler_minor_shadow: hex(0x414141),
            band_separator: hex(0x2E2E2E),
            grid_line: hex(0x525252),
            scroll_track: hex(0x2E2E2E),
            scroll_thumb: hex(0x606060),
            scroll_arrow: hex(0xDCDCDC),
            native_scroll_bg: hex(0xF0F0F0),

            loop_sel_line: hex(0x7BB5FE),
            loop_sel_edge: hex(0x567192),
            loop_flag: hex(0xFEF50C),
            loop_flag_shade: hex(0xC8C00A),
            loop_bar_off: hex(0x373737),
            loop_bar_on: hex(0x7BB5FE),
            selection_overlay: hexa(0x7BB5FE, 0.22),
            length_tag_bg: hex(0xFFFFFF),
            length_tag_text: hex(0x000000),
            marker_flag: hex(0xDB8E57),
            marker_text: hex(0xFFFFFF),
            region_flag: hex(0x549E65),
            region_flag_hi: hex(0x73AF81),
            region_edge: hex(0x5E7463),
            region_text: hex(0xD2F3E3),
            region_line: hex(0x4E8F5D),

            playhead_line: hex(0x060606),
            playhead_line_inverted: hex(0xD3D3D3),
            playhead_outline: hex(0x272727),
            playhead_head: hex(0xFFFFFF),

            event_selected_border: hex(0xCEE500),
            event_selected_border_ripple: hex(0xDC3C3C),
            event_label_bg: hexa(0x000000, 0.68),
            event_label_border: hex(0x141414),
            event_chip_bg: hexa(0x000000, 0.60),
            event_text: hex(0xFFFFFF),
            fade_curve: hex(0x0000F8),
            fade_curve_selected: hex(0xFFFFFF),
            fade_curve_in_xfade: hex(0x0000F8),
            fade_region_tint: hexa(0x000000, 0.35),
            fade_handle: hex(0x0202F3),
            fade_handle_2: hex(0x1212AD),
            snap_line: hex(0x4FC2EE),
            snap: SNAP16,
            bevel: BEVEL16,
            checker_a: hex(0x9A9A9A),
            checker_b: hex(0xC8C8C8),

            envelope_volume: hex(0x0F009B),
            envelope_composite: hex(0x0F009B),
            envelope_pan: hex(0xBC0400),
            envelope_mute: hex(0x32F0FF),
            envelope_point: hex(0xFFFFFF),

            workspace_bg: hex(0xECECEC),
            workspace_grid: hex(0xC8C8C8),
            workspace_frame: hex(0x505050),
            kf_ruler: hex(0x373737),
            kf_lane: hex(0x383838),
            kf_lane_alt: hex(0x2B2B2B),
            kf_row_sel: hex(0x828282),

            meter_bg: hex(0x232323),
            meter_unlit: 0.0,
            meter_green: hex(0x00FF57),
            meter_yellow: hex(0xFFFF00),
            meter_red: hex(0xFF2020),
            meter_label: hex(0x999A98),
            fader_track: hex(0x6A6A6A),
            fader_thumb: hex(0xFFFFFF),
            fader_thumb_bar: hex(0x232323),

            warning_text: hex(0xEF8F7C),
            preview_bg: hex(0x2D2D2D),
            preview_frame: hex(0x000000),
            preset_tile_selected: hex(0xFFFFFF),
            status_bg: hex(0x373737),
            status_edge_1: hex(0x2D2D2D),
            status_edge_2: hex(0x424242),
            logo_bg: hex(0x001751),
            logo_border: hex(0x258EE4),
            logo_text: hex(0xFFFFFF),

            track_colors: [
                hex(0x5973A8),
                hex(0x7C3441),
                hex(0x599CA8),
                hex(0x5D4181),
                hex(0x33897C),
                hex(0x7C7634),
                hex(0x415E81),
                hex(0x59A869),
            ],
            metal: FLAT_METAL,
        }
    }

    /// §2.2, measured from the official VP23 help images.
    pub fn vegas23_dark() -> Self {
        let mut m = Self::vegas16_dark().m;
        m.title_bar_h = 32.0;
        m.menu_bar_h = 0.0;
        m.main_toolbar_h = 35.0;
        m.main_toolbar_bevel = 0.0;
        m.dock_toolbar_h = 30.0;
        m.tab_strip_h = 29.0;
        m.tab_active_line_w = 1.0;
        m.splitter_w = 5.0;
        m.outer_margin = 5.0;
        m.dock_frac = 0.50;
        m.dock_h = 534.0; // timeline still starts at y=606 below the 5 px toolbar gap
        m.master_bus_w = 120.0;
        m.track_list_w = 250.0;
        m.timeline_gap = 5.0;
        m.time_display_h = 45.0;
        m.band_h = 46.0; // dark2026: rows +0..+29, line +30, 14 px ruler, bottom line +45
        m.loop_bar_h = 8.0;
        m.marker_bar_h = 14.0;
        m.ruler_h = 23.0;
        m.video_track_h = 52.0;
        m.audio_track_h = 52.0;
        m.event_header_h = 0.0;
        m.transport_h = 32.0;
        m.status_h = 20.0;
        m.status_gap = 5.0;
        m.button_pitch = 26.0;
        m.icon_stroke = 1.25;
        m.toggle_w = 22.0;
        m.toggle_h = 18.0;
        m.widget_radius = 2;
        m.popup_row_h = 24.0;

        let mut font = Self::vegas16_dark().font;
        font.track_number = 12.0;
        font.track_number_bold = false;
        font.mute_solo = 16.0;
        font.time_display = 22.0;
        font.time_display_semibold = true;
        font.transport_time = 12.0;

        let (white, dim) = (hex(0xF0F0F0), hex(0x787878));
        Self {
            choice: LookChoice::VEGAS23,
            dark_mode: true,
            m,
            font,

            title_bg: hex(0x141414),
            menu_bar_bg: hex(0x141414),
            menu_bar_bottom: hex(0x141414),
            menu_bar_line: hex(0x141414),
            menu_text: white,
            menu_bar_hover: hex(0x3C3C3C),
            menu_bar_hover_border: hex(0x3C3C3C),
            menu_bar_open: hex(0x3C3C3C),
            menu_popup_bg: hex(0x282828),
            menu_popup_border: hex(0x141414),
            menu_popup_hover: hex(0x3C8CDC),
            menu_popup_text: white,
            menu_popup_text_disabled: dim,
            caption_hover: hex(0x2D2D2D),
            caption_close_hover: hex(0xC42B1C),

            frame: hex(0x141414),
            toolbar_bg: hex(0x282828),
            bevel_hi_1: hex(0x282828),
            bevel_hi_2: hex(0x282828),
            dock_bevel_1: hex(0x282828),
            dock_bevel_2: hex(0x282828),
            line_dark: hex(0x141414),
            dock_divider: hex(0x141414),
            panel_bg: hex(0x282828),
            band_bg: hex(0x282828),
            lane_bg: hex(0x282828),
            empty_lane_bg: hex(0x1E1E1E),

            tab_strip_bg: hex(0x141414),
            tab_strip_single_bg: hex(0x141414),
            tab_active_bg: hex(0x141414),
            tab_separator: hex(0x141414),
            tab_text: dim,
            tab_text_active: white,
            tab_text_hover: hex(0xC8C8C8),
            tab_active_line: white,

            text: white,
            text_secondary: white,
            text_label: white,
            text_value: white,
            text_bright: hex(0xFFFFFF),
            text_dim: dim,
            text_disabled: dim,
            icon: white,
            icon_shadow: Color32::TRANSPARENT,
            icon_disabled: hex(0x4B4B4B),
            icon_color_strength: 1.0,
            input_bg: hex(0x1E1E1E),
            input_placeholder: dim,
            tooltip_bg: hex(0x282828),
            tooltip_text: white,
            tooltip_border: hex(0x141414),

            button_active_bg: hex(0x3C8CDC),
            button_hover_bg: hex(0x3A3A3A),
            button_group_sep: hex(0x505050),
            accent: hex(0x3C8CDC),

            track_header_bg: hex(0x282828),
            track_header_sel_bg: hex(0x505050),
            track_header_sel_top: hex(0x505050),
            track_header_sel_bottom: hex(0x505050),
            track_header_top: hex(0x141414),
            track_separator: hex(0x141414),
            track_separator_2: hex(0x141414),
            strip_divider: hex(0x141414),
            mute_badge: hex(0x3C8CDC),
            solo_badge: hex(0x3C8CDC),

            time_display_bg: hex(0x282828),
            time_display_text: white,
            ruler_text: hex(0xB0B0B0),
            ruler_major_tick: hex(0x909090),
            ruler_minor_tick: hex(0x909090),
            ruler_minor_shadow: Color32::TRANSPARENT,
            band_separator: hex(0x141414),
            grid_line: hex(0x505050),
            scroll_track: hex(0x2D2D2D),
            scroll_thumb: hex(0x464646),
            scroll_arrow: white,
            native_scroll_bg: hex(0x2D2D2D),

            loop_sel_line: hex(0x3C8CDC),
            loop_sel_edge: hex(0x6B5C28),
            loop_flag: hex(0xF9F400),
            loop_flag_shade: hex(0x9D9800),
            loop_bar_off: hex(0x3E3E3E),
            loop_bar_on: hex(0x3C8CDC),
            // Official VP23 image (c_ui_tl): a neutral lift #282828 -> #3C3C3C, not the §2.2 blue.
            selection_overlay: hexa(0xFFFFFF, 0.093),
            length_tag_bg: hex(0xFFFFFF),
            length_tag_text: hex(0x000000),
            marker_flag: hex(0xF38E28),
            marker_text: hex(0xFFFFFF),
            region_flag: hex(0x26CB5C),
            region_flag_hi: hex(0x26CB5C),
            region_edge: hex(0x26CB5C),
            region_text: hex(0xFFFFFF),
            region_line: hex(0x26CB5C),

            playhead_line: hex(0xEE5544),
            playhead_line_inverted: hex(0xEE5544),
            playhead_outline: hex(0x141414),
            playhead_head: white,

            event_selected_border: hex(0xC8C240),
            event_selected_border_ripple: hex(0xDC3C3C),
            event_label_bg: hexa(0x000000, 0.68),
            event_label_border: hex(0x141414),
            event_chip_bg: hexa(0x000000, 0.60),
            event_text: white,
            fade_curve: hex(0xFFFFFF),
            fade_curve_selected: hex(0xFFFFFF),
            fade_curve_in_xfade: hex(0x3C8CDC),
            fade_region_tint: hexa(0x000000, 0.35),
            fade_handle: hex(0x64C8DC),
            fade_handle_2: hex(0x64C8DC),
            snap_line: hex(0x5A9FC3),
            snap: SNAP16,
            bevel: BEVEL16,
            checker_a: hex(0x9A9A9A),
            checker_b: hex(0xC8C8C8),

            envelope_volume: hex(0x493FAB),
            envelope_composite: hex(0x493FAB),
            envelope_pan: hex(0xDC6450),
            envelope_mute: hex(0x32F0FF),
            envelope_point: hex(0xFFFFFF),

            workspace_bg: hex(0xECECEC),
            workspace_grid: hex(0xC8C8C8),
            workspace_frame: hex(0x505050),
            kf_ruler: hex(0x333333),
            kf_lane: hex(0x282828),
            kf_lane_alt: hex(0x2E2E2E),
            kf_row_sel: hex(0x505050),

            meter_bg: hex(0x141414),
            meter_unlit: 0.117, // green end -> #16291C (dark2026)
            meter_green: hex(0x3CC864),
            meter_yellow: hex(0xDCB43C),
            meter_red: hex(0xDC3C3C),
            meter_label: hex(0xB0B0B0),
            fader_track: hex(0x505050),
            fader_thumb: white,
            fader_thumb_bar: hex(0x282828),

            warning_text: hex(0xE0643C),
            preview_bg: hex(0x282828),
            preview_frame: hex(0x000000),
            preset_tile_selected: hex(0x3C8CDC),
            status_bg: hex(0x282828),
            status_edge_1: hex(0x141414),
            status_edge_2: hex(0x141414),
            logo_bg: hex(0x001751),
            logo_border: hex(0x258EE4),
            logo_text: hex(0xFFFFFF),

            track_colors: [
                hex(0x3C8CDC),
                hex(0x64C8DC),
                hex(0x50C878),
                hex(0xDCB43C),
                hex(0xDC8C3C),
                hex(0xDC6450),
                hex(0xC878DC),
                hex(0x783CF0),
            ],
            metal: FLAT_METAL,
        }
    }

    /// CLASSIC_2006_SPEC.md: 2005–2007 NLE chrome (Vegas 6/7 proportions and track headers, Windows 2000/XP classic
    /// controls and menus, brushed-steel bars) on the VEGAS 23 layout. Native title bar, in-window menu row.
    pub fn classic2006() -> Self {
        let m = Metrics {
            title_bar_h: 0.0,
            menu_bar_h: 20.0,
            main_toolbar_h: 26.0,
            main_toolbar_bevel: 2.0,
            dock_toolbar_h: 26.0,
            tab_strip_h: 21.0,
            tab_active_line_w: 0.0,
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
            toggle_w: 22.0,
            toggle_h: 20.0,
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
            time_display_semibold: true,
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
            choice: LookChoice::CLASSIC2006,
            dark_mode: false,
            m,
            font,

            title_bg: metal.mid,
            menu_bar_bg: metal.mid,
            menu_bar_bottom: metal.lo,
            menu_bar_line: metal.shadow,
            menu_text: black,
            menu_bar_hover: metal.hi,
            menu_bar_hover_border: metal.shadow,
            menu_bar_open: metal.face,
            // Windows XP (Luna) menus: white, 3D grey border, #316AC5 highlight bar.
            menu_popup_bg: white,
            menu_popup_border: grey,
            menu_popup_hover: hex(0x316AC5),
            menu_popup_text: black,
            menu_popup_text_disabled: grey,
            caption_hover: metal.hi,
            caption_close_hover: hex(0xC42B1C),

            frame: hex(0x5C6168),
            toolbar_bg: metal.mid,
            bevel_hi_1: metal.light,
            bevel_hi_2: metal.face,
            dock_bevel_1: metal.light,
            dock_bevel_2: metal.face,
            line_dark: metal.shadow,
            dock_divider: metal.shadow,
            panel_bg: hex(0xC9CCD1),
            band_bg: hex(0xAEB3BA),
            lane_bg: hex(0x3C4047),
            empty_lane_bg: hex(0x2F3339),

            tab_strip_bg: hex(0x9EA3AA),
            tab_strip_single_bg: hex(0x9EA3AA),
            tab_active_bg: metal.face,
            tab_separator: metal.shadow,
            tab_text: hex(0x2B2E33),
            tab_text_active: black,
            tab_text_hover: black,
            tab_active_line: metal.light,

            text: black,
            text_secondary: hex(0x1E2226),
            text_label: hex(0x2B2E33),
            text_value: black,
            text_bright: white,
            text_dim: dim,
            text_disabled: grey,
            icon: hex(0x0A0A0A),
            icon_shadow: Color32::TRANSPARENT,
            icon_disabled: grey,
            icon_color_strength: 1.0,
            input_bg: white,
            input_placeholder: grey,
            tooltip_bg: hex(0xFFFFE1),
            tooltip_text: black,
            tooltip_border: black,

            button_active_bg: hex(0x9CD3F0),
            button_hover_bg: metal.hi,
            button_group_sep: metal.shadow,
            accent,

            track_header_bg: hex(0x9AA6B3),
            track_header_sel_bg: hex(0xB4CFE4),
            track_header_sel_top: hex(0xDCEAF5),
            track_header_sel_bottom: hex(0x6E8CA6),
            track_header_top: metal.light,
            track_separator: hex(0x4A4E55),
            track_separator_2: hex(0x7A7F86),
            strip_divider: metal.dark,
            mute_badge: hex(0xD04040),
            solo_badge: hex(0xE0C040),

            // Black LCD well with cyan digits (Vegas 5–7 time display).
            time_display_bg: hex(0x14181E),
            time_display_text: hex(0x5CE0FF),
            ruler_text: black,
            ruler_major_tick: black,
            ruler_minor_tick: hex(0x3C4047),
            ruler_minor_shadow: Color32::TRANSPARENT,
            band_separator: metal.shadow,
            grid_line: hex(0x50555C),
            scroll_track: hex(0xA9AEB5),
            scroll_thumb: metal.face,
            scroll_arrow: black,
            native_scroll_bg: hex(0xD4D0C8),

            loop_sel_line: accent,
            loop_sel_edge: hex(0x005A90),
            loop_flag: hex(0xFFD800),
            loop_flag_shade: hex(0xB09400),
            loop_bar_off: hex(0x8E939A),
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
            event_label_bg: hexa(0x000000, 0.68),
            event_label_border: metal.dark,
            event_chip_bg: hexa(0x000000, 0.60),
            event_text: white,
            fade_curve: hex(0x0000C8),
            fade_curve_selected: white,
            fade_curve_in_xfade: hex(0x0000C8),
            fade_region_tint: hexa(0x000000, 0.30),
            fade_handle: hex(0x0050FF),
            fade_handle_2: hex(0x0030A0),
            snap_line: hex(0x00C8FF),
            snap: SNAP16,
            bevel: BEVEL16,
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
            meter_unlit: 0.0,
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
            preset_tile_selected: hex(0xB4CFE4),
            status_bg: metal.mid,
            status_edge_1: metal.shadow,
            status_edge_2: metal.light,
            logo_bg: hex(0x1C3A5E),
            logo_border: hex(0x5CA8E0),
            logo_text: white,

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

    /// §2.3: tokens not listed there stay as Dark; [I] values are marked in the spec.
    pub fn vegas23_white() -> Self {
        let mut t = Self::vegas23_dark();
        t.choice = LookChoice::VEGAS23_WHITE;
        t.dark_mode = false;
        t.m.tab_active_line_w = 2.0;
        let (edge, body, lane, ink, dim) = (hex(0xB4B4B4), hex(0xC8C8C8), hex(0xBEBEBE), hex(0x1E1E1E), hex(0x6E6E6E));
        for c in [
            &mut t.title_bg, &mut t.menu_bar_bg, &mut t.menu_bar_bottom, &mut t.menu_bar_line, &mut t.frame,
            &mut t.tab_strip_bg, &mut t.tab_strip_single_bg, &mut t.tab_active_bg, &mut t.tab_separator,
            &mut t.line_dark, &mut t.dock_divider, &mut t.track_separator, &mut t.track_separator_2,
            &mut t.track_header_top, &mut t.strip_divider, &mut t.band_separator, &mut t.status_edge_1,
            &mut t.status_edge_2, &mut t.event_label_border, &mut t.tooltip_border,
        ] {
            *c = edge;
        }
        for c in [
            &mut t.toolbar_bg, &mut t.bevel_hi_1, &mut t.bevel_hi_2, &mut t.dock_bevel_1, &mut t.dock_bevel_2,
            &mut t.panel_bg, &mut t.band_bg, &mut t.time_display_bg, &mut t.status_bg, &mut t.preview_bg,
            &mut t.track_header_bg, &mut t.tooltip_bg,
        ] {
            *c = body;
        }
        t.lane_bg = lane;
        t.empty_lane_bg = lane;
        for c in [
            &mut t.text, &mut t.text_secondary, &mut t.text_label, &mut t.text_value, &mut t.menu_text,
            &mut t.menu_popup_text, &mut t.time_display_text, &mut t.tab_text_active, &mut t.tab_active_line,
            &mut t.icon, &mut t.scroll_arrow, &mut t.tooltip_text,
        ] {
            *c = ink;
        }
        t.text_dim = dim;
        t.tab_text = dim;
        t.input_placeholder = dim;
        t.tab_text_hover = hex(0x3C3C3C); // [I]
        t.text_disabled = hex(0xA0A0A0); // [I]
        t.menu_popup_text_disabled = hex(0xA0A0A0); // [I]
        t.icon_disabled = hex(0xA0A0A0); // [I]
        t.scroll_track = hex(0xF2F2F2);
        t.native_scroll_bg = hex(0xF2F2F2);
        t.scroll_thumb = hex(0xA0A0A0); // [I]
        t.track_header_sel_bg = hex(0xE6E6E6); // [I]
        t.track_header_sel_top = hex(0xE6E6E6);
        t.track_header_sel_bottom = hex(0xE6E6E6);
        t.menu_popup_bg = hex(0xDCDCDC); // [I]
        t.menu_bar_hover = hex(0xD2D2D2); // [I]
        t.menu_bar_hover_border = hex(0xD2D2D2);
        t.menu_bar_open = hex(0xD2D2D2);
        t.caption_hover = hex(0xD2D2D2); // [I]
        t.button_hover_bg = hex(0xD8D8D8); // [I]
        t.button_group_sep = hex(0xA0A0A0); // [I]
        t.input_bg = hex(0xF2F2F2); // [I]
        t.warning_text = hex(0x78321E);
        // Not measured for White: dark-grey stand-ins readable on #C8C8C8. [I]
        t.ruler_text = hex(0x3C3C3C);
        t.ruler_major_tick = dim;
        t.ruler_minor_tick = dim;
        t.grid_line = hex(0xF0F0F0); // light2026 x=395
        t.loop_bar_off = hex(0xAAAAAA);
        t.meter_label = hex(0x3C3C3C);
        // Pale unlit columns, green end #A5B8AC (light2026).
        t.meter_bg = hex(0xBBBBBB);
        t.meter_unlit = 0.175;
        t.fader_track = hex(0x8C8C8C);
        t.fader_thumb = hex(0xFFFFFF);
        t
    }

    /// §2.6 mapping onto egui::Visuals.
    pub fn visuals(&self) -> Visuals {
        let mut v = if self.dark_mode { Visuals::dark() } else { Visuals::light() };
        v.dark_mode = self.dark_mode;
        v.override_text_color = Some(self.text);
        v.panel_fill = self.panel_bg;
        v.window_fill = self.panel_bg;
        v.faint_bg_color = self.band_bg;
        v.extreme_bg_color = match self.look() {
            UiLook::Vegas16 | UiLook::Classic2006 => self.input_bg,
            UiLook::Vegas23 => self.scroll_track,
        };
        v.code_bg_color = self.input_bg;
        v.warn_fg_color = self.warning_text;
        v.error_fg_color = self.warning_text;
        v.hyperlink_color = self.accent;
        v.window_stroke = Stroke::new(1.0_f32, self.frame);
        v.window_shadow = Shadow::NONE;
        v.popup_shadow = match self.look() {
            UiLook::Vegas16 | UiLook::Classic2006 => Shadow::NONE,
            UiLook::Vegas23 => Shadow { offset: [0, 2], blur: 6, spread: 0, color: Color32::from_black_alpha(90) },
        };
        let r = CornerRadius::same(self.m.widget_radius);
        v.window_corner_radius = CornerRadius::ZERO;
        v.menu_corner_radius = r;
        v.selection.bg_fill = self.accent;
        v.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);

        let w = &mut v.widgets;
        w.noninteractive.bg_fill = self.panel_bg;
        w.noninteractive.weak_bg_fill = self.panel_bg;
        w.noninteractive.bg_stroke = Stroke::new(1.0_f32, self.line_dark);
        w.noninteractive.fg_stroke = Stroke::new(1.0_f32, self.text);
        w.inactive.bg_fill = self.toolbar_bg;
        w.inactive.weak_bg_fill = self.toolbar_bg;
        w.inactive.bg_stroke = Stroke::NONE;
        w.inactive.fg_stroke = Stroke::new(1.0_f32, self.text);
        w.hovered.bg_fill = self.button_hover_bg;
        w.hovered.weak_bg_fill = self.button_hover_bg;
        w.hovered.bg_stroke = Stroke::NONE;
        w.hovered.fg_stroke = Stroke::new(1.5_f32, self.text);
        w.active.bg_fill = self.button_active_bg;
        w.active.weak_bg_fill = self.button_active_bg;
        w.active.bg_stroke = Stroke::NONE;
        w.active.fg_stroke = Stroke::new(1.5_f32, self.text_bright);
        w.open.bg_fill = self.button_hover_bg;
        w.open.weak_bg_fill = self.button_hover_bg;
        w.open.bg_stroke = Stroke::NONE;
        w.open.fg_stroke = Stroke::new(1.0_f32, self.text);
        for s in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
            s.corner_radius = r;
            s.expansion = 0.0;
        }
        w.noninteractive.corner_radius = CornerRadius::ZERO;
        if self.classic() {
            // Windows-classic controls: 3D grey faces with a shadow outline, black ink, a blue selection.
            let m = &self.metal;
            w.inactive.bg_fill = m.face;
            w.inactive.weak_bg_fill = m.face;
            w.inactive.bg_stroke = Stroke::new(1.0_f32, m.shadow);
            w.hovered.bg_fill = m.hi;
            w.hovered.weak_bg_fill = m.hi;
            w.hovered.bg_stroke = Stroke::new(1.0_f32, m.shadow);
            w.hovered.fg_stroke = Stroke::new(1.0_f32, self.text);
            w.active.fg_stroke = Stroke::new(1.0_f32, self.text);
            w.open.bg_stroke = Stroke::new(1.0_f32, m.shadow);
            w.noninteractive.bg_stroke = Stroke::new(1.0_f32, m.shadow);
            v.selection.stroke = Stroke::new(1.0_f32, self.text_bright);
            v.window_stroke = Stroke::new(1.0_f32, m.shadow);
        }
        v
    }

    /// Visuals, spacing and per-look text sizes on the whole context.
    pub fn apply(&self, ctx: &egui::Context) {
        let f = self.font;
        let fam = self.family();
        ctx.style_mut(|s| {
            s.visuals = self.visuals();
            s.text_styles = [
                (TextStyle::Small, FontId::new(if self.classic() { 9.0 } else { 10.0 }, fam.clone())),
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
            // Classic 2006: a grey 3D-face handle on the white rail (the VEGAS looks keep egui's light foreground handle).
            sp.scroll.foreground_color = !self.classic();
        });
    }
}

/// VP16 bevel lines measured from the native VP15 window [L]; VP23 draws none.
#[derive(Clone, Copy, Debug)]
pub struct Bevels16 {
    /// Second top row of an inactive tab (first is `tab_separator`).
    pub tab_top_2: Color32,
    /// Top rows of the strip right of the last tab, and the rows under the strip.
    pub strip_1: Color32,
    pub strip_2: Color32,
    pub tab_side_hi: Color32,
    pub time_display_1: Color32,
    pub time_display_2: Color32,
    pub time_display_bottom: Color32,
    pub band_1: Color32,
    pub band_2: Color32,
    pub band_bottom: Color32,
    pub transport_top: Color32,
}

const BEVEL16: Bevels16 = Bevels16 {
    tab_top_2: hex(0x313131),
    strip_1: hex(0x3C3C3C),
    strip_2: hex(0x292929),
    tab_side_hi: hex(0x444444),
    time_display_1: hex(0x484848),
    time_display_2: hex(0x6B6B6B),
    time_display_bottom: hex(0x393939),
    band_1: hex(0x313131),
    band_2: hex(0x404040),
    band_bottom: hex(0x303030),
    transport_top: hex(0x6F6F6F),
};

/// The VEGAS looks never paint metal: flat stand-ins the size of their toolbar colour.
const FLAT_METAL: Metal = Metal {
    hi: hex(0x464646),
    mid: hex(0x464646),
    lo: hex(0x464646),
    light: hex(0x5C5C5C),
    face: hex(0x464646),
    shadow: hex(0x333333),
    dark: hex(0x232323),
    streak: 0.0,
    header_tint: hex(0xFFFFFF),
    specular: Color32::TRANSPARENT,
    pressed: Color32::TRANSPARENT,
};

const SNAP16: SnapColors = SnapColors {
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

/// Modal dialog colours (§5.15): VP16 dialogs are native light Win11 property sheets [L]; VP23 dialogs use the
/// dark palette with a native (white) title bar [M].
#[derive(Clone, Copy, Debug)]
pub struct DialogColors {
    pub body: Color32,
    pub page: Color32,
    pub field: Color32,
    pub field_border: Color32,
    pub title_bg: Color32,
    pub title_text: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_disabled: Color32,
    pub button: Color32,
    pub button_border: Color32,
    pub button_hover: Color32,
    pub button_hover_border: Color32,
    pub tab_border: Color32,
    /// Group heading rule: dark line over a light one.
    pub etch: (Color32, Color32),
    pub accent: Color32,
    pub accent_text: Color32,
    /// Caption bar gradient (left → right) and its text: the Windows 2000 active caption in Classic 2006, the flat
    /// title row elsewhere.
    pub caption: (Color32, Color32),
    pub caption_text: Color32,
}

impl Theme {
    pub fn dialog(&self) -> DialogColors {
        match self.look() {
            // Windows 2000/XP-classic property sheet: #D4D0C8 face, white fields, #808080 / white etched rules.
            UiLook::Classic2006 => DialogColors {
                body: hex(0xD4D0C8),
                page: hex(0xD4D0C8),
                field: hex(0xFFFFFF),
                field_border: hex(0x808080),
                title_bg: hex(0xFFFFFF),
                title_text: hex(0x000000),
                border: hex(0x808080),
                text: hex(0x000000),
                text_disabled: hex(0x808080),
                button: hex(0xD4D0C8),
                button_border: hex(0x404040),
                button_hover: hex(0xE4E2DC),
                button_hover_border: hex(0x000000),
                tab_border: hex(0x808080),
                etch: (hex(0x808080), hex(0xFFFFFF)),
                accent: hex(0x316AC5),
                accent_text: hex(0xFFFFFF),
                caption: (hex(0x0A246A), hex(0xA6CAF0)),
                caption_text: hex(0xFFFFFF),
            },
            UiLook::Vegas16 => DialogColors {
                body: hex(0xF0F0F0),
                page: hex(0xF9F9F9),
                field: hex(0xFFFFFF),
                field_border: hex(0xBCBCBC),
                title_bg: hex(0xF3F3F3),
                title_text: hex(0x000000),
                border: hex(0xAAAAAA),
                text: hex(0x000000),
                text_disabled: hex(0x9D9D9D),
                button: hex(0xFDFDFD),
                button_border: hex(0xD0D0D0),
                button_hover: hex(0xE0EEF9),
                button_hover_border: hex(0x0078D4),
                tab_border: hex(0xD9D9D9),
                etch: (hex(0xA0A0A0), hex(0xFFFFFF)),
                accent: hex(0x0067C0),
                accent_text: hex(0xFFFFFF),
                caption: (hex(0xF3F3F3), hex(0xF3F3F3)),
                caption_text: hex(0x000000),
            },
            UiLook::Vegas23 => DialogColors {
                body: self.panel_bg,
                page: self.panel_bg,
                field: self.input_bg,
                field_border: self.line_dark,
                title_bg: hex(0xFFFFFF),
                title_text: hex(0x000000),
                border: self.frame,
                text: self.text,
                text_disabled: self.text_disabled,
                button: self.toolbar_bg,
                button_border: self.line_dark,
                button_hover: self.button_hover_bg,
                button_hover_border: self.accent,
                tab_border: self.line_dark,
                etch: (self.line_dark, self.band_bg),
                accent: self.accent,
                accent_text: self.text_bright,
                caption: (hex(0xFFFFFF), hex(0xFFFFFF)),
                caption_text: hex(0x000000),
            },
        }
    }
}

/// Before `set` (the app sets the chosen look at startup): the VEGAS 16 theme, which the unit tests draw with.
static CURRENT: LazyLock<RwLock<Theme>> = LazyLock::new(|| RwLock::new(Theme::vegas16_dark()));
/// Track colours edited in Preferences › Display; replaces the look's palette when set.
static PALETTE: RwLock<Option<[Color32; 8]>> = RwLock::new(None);

pub fn set_palette(ctx: &egui::Context, palette: Option<[Color32; 8]>) {
    *PALETTE.write().unwrap() = palette;
    let choice = get().choice;
    set(ctx, choice);
}

/// The active theme (a copy; cheap enough per panel per frame).
pub fn get() -> Theme {
    *CURRENT.read().unwrap()
}

/// Switch looks live: stores the theme and applies it to `ctx`.
pub fn set(ctx: &egui::Context, choice: LookChoice) {
    let mut t = Theme::new(choice);
    if let Some(p) = *PALETTE.read().unwrap() {
        t.track_colors = p;
    }
    *CURRENT.write().unwrap() = t;
    t.apply(ctx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn look_parse_and_spec_values() {
        assert_eq!(LookChoice::parse("VEGAS23-white"), Some(LookChoice::VEGAS23_WHITE));
        assert_eq!(LookChoice::parse("vegas16"), Some(LookChoice::VEGAS16));
        assert_eq!(LookChoice::parse("classic2006"), Some(LookChoice::CLASSIC2006));
        assert_eq!(LookChoice::parse("nope"), None);
        let (a, b, w) = (Theme::new(LookChoice::VEGAS16), Theme::new(LookChoice::VEGAS23), Theme::vegas23_white());
        assert_eq!((a.frame, a.toolbar_bg, a.m.splitter_w), (hex(0x232323), hex(0x464646), 7.0));
        assert_eq!((b.frame, b.toolbar_bg, b.m.splitter_w), (hex(0x141414), hex(0x282828), 5.0));
        assert_eq!((w.frame, w.panel_bg, w.text), (hex(0xB4B4B4), hex(0xC8C8C8), hex(0x1E1E1E)));
        assert_eq!(w.accent, hex(0x3C8CDC)); // unlisted White tokens stay Dark
    }

    /// Three looks: Classic 2006 is the default for new installs, follows the VEGAS 23 layout family, keeps the VEGAS
    /// looks' tokens untouched, and old preferences naming a VEGAS look still load.
    #[test]
    fn classic_2006_is_default_and_follows_vp23() {
        assert_eq!(LookChoice::default(), LookChoice::CLASSIC2006);
        assert_eq!(UiLook::default(), UiLook::Classic2006);
        assert_eq!(LookChoice::ALL.len(), 3 + 1);
        assert!(UiLook::Classic2006.v23() && UiLook::Vegas23.v23() && !UiLook::Vegas16.v23());
        assert_eq!((UiLook::Classic2006.base(), UiLook::Vegas16.base()), (UiLook::Vegas23, UiLook::Vegas16));
        let c = Theme::new(LookChoice::CLASSIC2006);
        assert!(c.classic() && !c.dark_mode && c.m.title_bar_h == 0.0 && c.m.menu_bar_h > 0.0, "native title bar, in-window menu");
        assert_eq!((c.font.ui, c.m.icon_stroke, c.m.widget_radius, c.metal.streak), (11.0, 1.0, 0, 1.0));
        assert_eq!((c.menu_popup_bg, c.menu_popup_hover, c.text), (hex(0xFFFFFF), hex(0x316AC5), hex(0x000000)));
        assert_eq!(Theme::new(LookChoice::VEGAS23).toolbar_bg, hex(0x282828));
        assert_eq!(Theme::new(LookChoice::VEGAS16).metal.streak, 0.0);
        let old: LookChoice = serde_json::from_str(r#"{"look":"Vegas16","variant":"Dark"}"#).unwrap();
        assert_eq!(old, LookChoice::VEGAS16);
        let v23: LookChoice = serde_json::from_str(r#"{"look":"Vegas23","variant":"White"}"#).unwrap();
        assert_eq!(v23, LookChoice::VEGAS23_WHITE);
    }
}
