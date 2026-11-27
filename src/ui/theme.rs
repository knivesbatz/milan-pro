//! The look: the design system's colour tokens, metrics and font sizes (docs/UI_SPEC.md §2-§4), mapped onto
//! egui::Visuals. Drawing code reads `theme::get()` and never hard-codes colours.
use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Shadow, Stroke, TextStyle, Visuals};
use std::sync::{LazyLock, RwLock};

/// The silver ramp (UI_SPEC.md §2.1): the brand's aluminium, darkened in steps.
#[derive(Clone, Copy, Debug)]
pub struct Silver {
    /// Specular lip inside the top of raised chrome.
    pub s0: Color32,
    /// Chrome top; menu and tooltip body.
    pub s1: Color32,
    /// Chrome middle; panel body.
    pub s2: Color32,
    /// Chrome bottom.
    pub s3: Color32,
    /// Recess: slider and scroll tracks, inactive tabs, tab strips.
    pub s4: Color32,
    /// Shadow edge, grooves.
    pub s5: Color32,
    /// Outline round every control and panel.
    pub s6: Color32,
}

/// Window frames and captions (UI_SPEC.md §2.2).
#[derive(Clone, Copy, Debug)]
pub struct Slate {
    pub hi: Color32,
    pub lo: Color32,
    /// Gutters, splitters and the outer frame of dialogs.
    pub frame: Color32,
    /// The grooved ridges beside a caption's title: light over dark.
    pub ridge: (Color32, Color32),
    pub text: Color32,
}

/// The navy LCD (UI_SPEC.md §2.3): read-outs, meters, lists and trees.
#[derive(Clone, Copy, Debug)]
pub struct Lcd {
    /// Read-out wells (time, meters, status panes).
    pub deep: Color32,
    /// List and tree wells.
    pub well: Color32,
    pub hover: Color32,
    /// Tree lines, grid rules, the dot grid.
    pub rule: Color32,
    /// Unlit segments.
    pub ghost: Color32,
    pub outline: Color32,
    pub text: Color32,
    pub text_hi: Color32,
    /// Secondary text, only on `deep`.
    pub text_dim: Color32,
    pub digits: Color32,
}

/// Selection gradient and the lit (toggle on) chrome (UI_SPEC.md §2.4).
#[derive(Clone, Copy, Debug)]
pub struct Sel {
    pub top: Color32,
    pub bottom: Color32,
    /// The flat selection (egui-drawn highlights).
    pub mid: Color32,
    /// Light line along the top of a selection bar.
    pub lip: Color32,
    pub text: Color32,
    pub lit_top: Color32,
    pub lit_bottom: Color32,
    /// Outline of lit chrome; the focus ring.
    pub lit_outline: Color32,
}

/// The one accent (UI_SPEC.md §2.5).
#[derive(Clone, Copy, Debug)]
pub struct Amber {
    pub base: Color32,
    pub hi: Color32,
    pub lo: Color32,
    /// Amber text on navy.
    pub text: Color32,
}

/// Text and fields (UI_SPEC.md §2.6).
#[derive(Clone, Copy, Debug)]
pub struct Ink {
    /// Text on silver and paper.
    pub text: Color32,
    pub label: Color32,
    /// Push button and pill labels.
    pub button: Color32,
    /// Monochrome glyphs drawn on silver (arrows, ×, check marks).
    pub glyph: Color32,
    pub disabled: Color32,
    pub paper: Color32,
    pub paper_disabled: Color32,
}

/// The chrome palette every bevel painter takes (`Theme::metal`, the silver ramp under the names of its roles).
#[derive(Clone, Copy, Debug)]
pub struct Metal {
    /// Vertical gradient of raised chrome: top, middle (45 %), bottom.
    pub hi: Color32,
    pub mid: Color32,
    pub lo: Color32,
    /// Lip (top-left of raised), face, shade (bottom-right of raised) and outline.
    pub light: Color32,
    pub face: Color32,
    pub shadow: Color32,
    pub dark: Color32,
    /// Opacity of the brushed streak texture over the gradient (0 = plain gradient).
    pub streak: f32,
    /// Tint of the track headers (gradient multiplied by it).
    pub header_tint: Color32,
    /// Specular lip under a raised control's top edge.
    pub specular: Color32,
    /// Flat fill of a pressed or lit control where no gradient is drawn.
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
    /// Corner radius of egui's widgets: `r_ctl`.
    pub widget_radius: u8,
    /// Outer corners of windows, dock groups and preset tiles.
    pub r_frame: u8,
    /// Buttons, tabs, wells, fields, menus.
    pub r_ctl: u8,
    /// Check boxes, small icon buttons.
    pub r_small: u8,
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

    // The design system's tokens (UI_SPEC.md §2); the named roles below are drawn from them.
    pub silver: Silver,
    pub slate: Slate,
    pub lcd: Lcd,
    pub sel: Sel,
    pub amber: Amber,
    pub ink: Ink,

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
    /// Colour-critical wells (wheels, curves, scopes): neutral, never navy (UI_SPEC.md §1).
    pub neutral_well: Color32,

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

    /// The chrome palette inside dialogs: the same silver as the dock windows (one look), without the streaks (the
    /// streak texture is for bars only).
    pub fn dialog_metal(&self) -> Metal {
        Metal { streak: 0.0, ..self.metal }
    }

    pub fn bold(&self, size: f32) -> FontId {
        if !super::fonts::installed() {
            return FontId::proportional(size);
        }
        FontId::new(size, FontFamily::Name(super::fonts::TAHOMA_BOLD.into()))
    }

    /// UI_SPEC.md: the silver chrome, navy LCD and amber design system on the VEGAS 23 layout. Native title bar,
    /// in-window menu row.
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
            widget_radius: 3,
            r_frame: 5,
            r_ctl: 3,
            r_small: 2,
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
        // UI_SPEC.md §2: the tokens.
        let silver = Silver {
            s0: hex(0xFFFFFF),
            s1: hex(0xF3F5F8),
            s2: hex(0xDDE1E7),
            s3: hex(0xC4CAD3),
            s4: hex(0xA6AEBB),
            s5: hex(0x7A8494),
            s6: hex(0x4A5468),
        };
        let slate = Slate { hi: hex(0x5B6C8E), lo: hex(0x34425E), frame: hex(0x36425C), ridge: (hex(0x7D8DAA), hex(0x1E2840)), text: hex(0xF3F5F8) };
        let lcd = Lcd {
            deep: hex(0x00225F),
            well: hex(0x1C3D7D),
            hover: hex(0x24488C),
            rule: hex(0x3A5A9A),
            ghost: hex(0x0E3477),
            outline: hex(0x0B1F4F),
            text: hex(0xA8C8FF),
            text_hi: hex(0xE6F0FF),
            text_dim: hex(0x8DB0EE),
            digits: hex(0xDCEBFF),
        };
        let sel = Sel {
            top: hex(0x3A68CE),
            bottom: hex(0x2450B4),
            mid: hex(0x2F5CC1),
            lip: hex(0x7FA2E8),
            text: hex(0xFFFFFF),
            lit_top: hex(0xE3EDFF),
            lit_bottom: hex(0x9DBDF2),
            lit_outline: hex(0x2F5CC1),
        };
        let amber = Amber { base: hex(0xFFA424), hi: hex(0xFFDC90), lo: hex(0xD86000), text: hex(0xFFB43C) };
        let ink = Ink {
            text: hex(0x0F1C3A),
            label: hex(0x2E3A55),
            button: hex(0x0C2A78),
            glyph: hex(0x1A49B8),
            disabled: hex(0x4C566B),
            paper: hex(0xFBFCFE),
            paper_disabled: hex(0xE8EBF0),
        };
        // Bars run from `s1` to the recess silver `s4`: the reference's deep top-to-bottom sweep.
        let metal = Metal {
            hi: silver.s1,
            mid: silver.s2,
            lo: silver.s4,
            light: silver.s0,
            face: silver.s2,
            shadow: silver.s5,
            dark: silver.s6,
            // Faint: at most ±2 % luminance, on bars only (UI_SPEC.md §1: text never on texture).
            streak: 0.25,
            header_tint: hex(0xFFFFFF),
            specular: silver.s0,
            pressed: sel.lit_bottom,
        };
        let (black, white, grey, dim) = (ink.text, hex(0xFFFFFF), ink.disabled, ink.label);
        let accent = sel.mid;
        Self {
            m,
            font,
            silver,
            slate,
            lcd,
            sel,
            amber,
            ink,

            menu_text: black,
            // Menus: silver popups with an outline and the selection blue.
            menu_popup_bg: silver.s1,
            menu_popup_border: silver.s6,
            menu_popup_hover: sel.mid,
            menu_popup_text: black,
            menu_popup_text_disabled: grey,

            frame: slate.frame,
            toolbar_bg: metal.mid,
            line_dark: metal.shadow,
            panel_bg: silver.s2,
            band_bg: silver.s3,
            // Blue-slate lanes: the navy family, desaturated so the pastel events keep their colour (§2.7).
            lane_bg: hex(0x2B3550),
            empty_lane_bg: hex(0x222A40),

            tab_text: ink.label,
            tab_text_active: black,
            tab_text_hover: black,

            text: black,
            text_secondary: ink.label,
            text_label: ink.label,
            text_value: black,
            text_bright: white,
            text_dim: dim,
            text_disabled: grey,
            icon: hex(0x0A0A0A),
            icon_disabled: grey,
            input_bg: ink.paper,
            input_placeholder: grey,
            tooltip_bg: silver.s1,
            tooltip_text: black,
            tooltip_border: silver.s6,

            button_active_bg: sel.lit_bottom,
            button_hover_bg: metal.hi,
            accent,

            track_header_bg: silver.s2,
            track_header_top: metal.light,
            track_separator: slate.frame,
            track_separator_2: silver.s5,
            strip_divider: metal.dark,

            // The navy LCD with segmented digits.
            time_display_bg: lcd.deep,
            time_display_text: lcd.digits,
            ruler_text: black,
            ruler_major_tick: black,
            ruler_minor_tick: silver.s6,
            grid_line: hex(0x3D4A68),
            scroll_track: silver.s4,
            scroll_arrow: ink.glyph,

            loop_sel_line: accent,
            loop_flag: hex(0xFFD800),
            loop_flag_shade: hex(0xB09400),
            loop_bar_on: accent,
            selection_overlay: hexa(0x00AEEF, 0.18),
            length_tag_bg: silver.s1,
            length_tag_text: black,
            marker_flag: hex(0xE08A2A),
            // Ink on the tags: white fails AA on the orange and green (UI_SPEC.md §2.8).
            marker_text: black,
            region_flag: hex(0x3C9A50),
            region_flag_hi: hex(0x6FBF80),
            region_edge: hex(0x2E7A40),
            region_text: black,
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
            kf_ruler: silver.s3,
            kf_lane: silver.s2,
            kf_lane_alt: hex(0xD2D7DE),
            kf_row_sel: sel.lit_top,

            meter_bg: lcd.deep,
            meter_green: hex(0x00E020),
            meter_yellow: hex(0xF0E000),
            meter_red: hex(0xFF2020),
            meter_label: black,
            fader_track: metal.shadow,
            fader_thumb: metal.face,
            fader_thumb_bar: metal.dark,

            warning_text: hex(0xA00000),
            // Video sits on black (UI_SPEC.md §1: neutral for pictures).
            preview_bg: hex(0x000000),
            preview_frame: hex(0x000000),
            neutral_well: hex(0x1C1C1E),

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

    /// The tokens mapped onto egui::Visuals: silver faces, paper fields, `r_ctl` corners, the selection blue; popups
    /// and tooltips on `s1` with an `s6` outline (UI_SPEC.md §5).
    pub fn visuals(&self) -> Visuals {
        let mut v = Visuals::light();
        v.override_text_color = Some(self.text);
        v.panel_fill = self.panel_bg;
        v.window_fill = self.silver.s1;
        v.faint_bg_color = self.band_bg;
        v.extreme_bg_color = self.input_bg;
        v.code_bg_color = self.input_bg;
        v.warn_fg_color = self.warning_text;
        v.error_fg_color = self.warning_text;
        v.hyperlink_color = self.accent;
        v.window_shadow = Shadow::NONE;
        v.popup_shadow = Shadow::NONE;
        let r = CornerRadius::same(self.m.r_ctl);
        v.window_corner_radius = CornerRadius::same(self.m.r_frame);
        v.menu_corner_radius = r;
        v.selection.bg_fill = self.sel.mid;

        let (s, ink) = (&self.silver, &self.ink);
        let w = &mut v.widgets;
        w.noninteractive.bg_fill = self.panel_bg;
        w.noninteractive.weak_bg_fill = self.panel_bg;
        // Silver faces with an outline, navy ink, paper fields (`bg_fill`: check boxes, slider rails).
        w.noninteractive.bg_stroke = Stroke::new(1.0_f32, s.s5);
        w.noninteractive.fg_stroke = Stroke::new(1.0_f32, ink.text);
        w.inactive.bg_fill = ink.paper;
        w.inactive.weak_bg_fill = s.s2;
        w.inactive.bg_stroke = Stroke::new(1.0_f32, s.s5);
        w.inactive.fg_stroke = Stroke::new(1.0_f32, ink.text);
        w.hovered.bg_fill = ink.paper;
        w.hovered.weak_bg_fill = s.s1;
        w.hovered.bg_stroke = Stroke::new(1.0_f32, self.sel.lit_outline);
        w.hovered.fg_stroke = Stroke::new(1.0_f32, ink.text);
        w.active.bg_fill = ink.paper;
        w.active.weak_bg_fill = self.sel.lit_bottom;
        w.active.bg_stroke = Stroke::new(1.0_f32, self.sel.lit_outline);
        w.active.fg_stroke = Stroke::new(1.0_f32, ink.text);
        w.open.bg_fill = ink.paper;
        w.open.weak_bg_fill = self.sel.lit_top;
        w.open.bg_stroke = Stroke::new(1.0_f32, self.sel.lit_outline);
        w.open.fg_stroke = Stroke::new(1.0_f32, ink.text);
        for st in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
            st.corner_radius = r;
            st.expansion = 0.0;
        }
        w.noninteractive.corner_radius = CornerRadius::ZERO;
        v.selection.stroke = Stroke::new(1.0_f32, self.sel.text);
        v.window_stroke = Stroke::new(1.0_f32, s.s6);
        v.text_cursor.stroke = Stroke::new(1.0_f32, ink.text);
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
            // An `s5` handle on the rail (UI_SPEC.md §5 scrollbar).
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

/// Dialog colours: the design system's panel, paper, ink and caption tokens under the dialog roles.
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
    /// Caption bar gradient (top → bottom) and its text: the slate caption.
    pub caption: (Color32, Color32),
    pub caption_text: Color32,
}

impl Theme {
    pub fn dialog(&self) -> DialogColors {
        let (s, ink) = (&self.silver, &self.ink);
        DialogColors {
            body: s.s2,
            page: s.s2,
            field: ink.paper,
            field_border: s.s5,
            border: s.s6,
            text: ink.text,
            text_disabled: ink.disabled,
            button: s.s2,
            button_border: s.s6,
            button_hover: s.s1,
            button_hover_border: self.sel.lit_outline,
            etch: (s.s5, s.s0),
            accent: self.sel.mid,
            accent_text: self.sel.text,
            caption: (self.slate.hi, self.slate.lo),
            caption_text: self.slate.text,
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

    /// Native title bar, in-window menu, Tahoma 8 pt, the one radius family, faint streaks, silver menus with the
    /// selection blue, navy ink (UI_SPEC.md §2-§4).
    #[test]
    fn spec_values() {
        let c = Theme::new();
        assert!(c.m.menu_bar_h > 0.0, "in-window menu row");
        assert_eq!((c.font.ui, c.m.icon_stroke, c.m.widget_radius, c.metal.streak), (11.0, 1.0, 3, 0.25));
        assert_eq!((c.m.r_frame, c.m.r_ctl, c.m.r_small), (5, 3, 2));
        assert_eq!((c.menu_popup_bg, c.menu_popup_hover, c.text), (hex(0xF3F5F8), hex(0x2F5CC1), hex(0x0F1C3A)));
        assert_eq!((c.lcd.deep, c.lcd.well, c.amber.base), (hex(0x00225F), hex(0x1C3D7D), hex(0xFFA424)));
        // One silver: the dialogs paint with the dock windows' palette.
        assert_eq!((c.dialog().page, c.dialog_metal().face), (c.panel_bg, c.metal.face));
        assert!(!c.visuals().dark_mode);
    }

    /// WCAG 2.1 relative luminance of an sRGB colour.
    fn luminance(c: Color32) -> f64 {
        let lin = |v: u8| {
            let v = v as f64 / 255.0;
            if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
    }

    fn contrast(a: Color32, b: Color32) -> f64 {
        let (la, lb) = (luminance(a), luminance(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    /// `c` darkened by `f` (0..1): the streak texture's darkest pixel over a bar.
    fn darker(c: Color32, f: f32) -> Color32 {
        let k = |v: u8| (v as f32 * (1.0 - f)).round() as u8;
        Color32::from_rgb(k(c.r()), k(c.g()), k(c.b()))
    }

    /// UI_SPEC.md §2.8: every text / background pair of the design system meets WCAG AA for normal text (4.5:1),
    /// on the worst stop of each background a label can sit on.
    #[test]
    fn contrast_aa() {
        let t = Theme::new();
        let (s, l, sel, ink, d) = (t.silver, t.lcd, t.sel, t.ink, t.dialog());
        // The streak texture over a bar: at most 18/255 per unit, at `streak` opacity, darker.
        let streak = 18.0 / 255.0 * t.metal.streak;
        let pairs = [
            ("ink on chrome bottom", ink.text, s.s3),
            ("ink on streaked bar bottom", ink.text, darker(t.metal.lo, streak)),
            ("ink on recess", ink.text, s.s4),
            ("ink on panel", ink.text, s.s2),
            ("ink on paper", ink.text, ink.paper),
            ("label on chrome bottom", ink.label, s.s3),
            ("button label on chrome bottom", ink.button, s.s3),
            ("button label on lit chrome", ink.button, sel.lit_bottom),
            ("ink on lit chrome top", ink.text, sel.lit_top),
            ("ink on lit chrome bottom", ink.text, sel.lit_bottom),
            ("disabled on chrome bottom", ink.disabled, s.s3),
            ("disabled on panel", ink.disabled, s.s2),
            ("disabled on disabled field", ink.disabled, ink.paper_disabled),
            ("placeholder on paper", t.input_placeholder, ink.paper),
            ("caption on caption top", t.slate.text, t.slate.hi),
            ("caption on caption bottom", t.slate.text, t.slate.lo),
            ("lcd text on list well", l.text, l.well),
            ("lcd text on hovered row", l.text, l.hover),
            ("lcd text on read-out well", l.text, l.deep),
            ("lcd dim text on read-out well", l.text_dim, l.deep),
            ("lcd digits on read-out well", l.digits, l.deep),
            ("lcd bright text on list well", l.text_hi, l.well),
            ("amber text on list well", t.amber.text, l.well),
            ("amber text on read-out well", t.amber.text, l.deep),
            ("selection text on selection top", sel.text, sel.top),
            ("selection text on selection bottom", sel.text, sel.bottom),
            ("selection text on flat selection", sel.text, sel.mid),
            ("menu text on menu", t.menu_popup_text, t.menu_popup_bg),
            ("menu disabled on menu", t.menu_popup_text_disabled, t.menu_popup_bg),
            ("tooltip", t.tooltip_text, t.tooltip_bg),
            ("dialog text on page", d.text, d.page),
            ("dialog disabled on page", d.text_disabled, d.page),
            ("dialog text on field", d.text, d.field),
            ("progress label on its groove", sel.text, l.deep),
            ("tab label on inactive tab", t.tab_text, s.s4),
            ("ruler text on band", t.ruler_text, s.s3),
            ("meter label on silver", t.meter_label, s.s3),
            ("track value on header", t.text_value, s.s3),
            ("white text on lanes", t.text_bright, t.lane_bg),
            ("marker number on its tag", t.marker_text, t.marker_flag),
            ("warning text on chrome bottom", t.warning_text, s.s3),
            ("region number on its tag", t.region_text, t.region_flag),
        ];
        let fails: Vec<String> = pairs
            .iter()
            .map(|&(name, fg, bg)| (name, contrast(fg, bg)))
            .filter(|&(_, r)| r < 4.5)
            .map(|(name, r)| format!("{name}: {r:.2}"))
            .collect();
        assert!(fails.is_empty(), "below WCAG AA 4.5:1: {fails:?}");
        // The streaks stay faint: ±2 % at most.
        assert!(streak <= 0.02, "{streak}");
    }
}
