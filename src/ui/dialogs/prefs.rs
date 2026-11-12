//! Options › Preferences (§5.15.1): two (VP23: three) rows of tabs; General and Display are live.
use eframe::egui::{self, *};
use crate::settings::Prefs;
use super::super::theme::{self, LookChoice, UiLook, Variant};

const ROWS16: &[&[&str]] = &[
    &["General", "Video", "Preview Device", "Audio", "Audio Device", "MIDI", "VST Effects"],
    &["Editing", "Display", "CD Settings", "Sync", "External Control & Automation"],
];
const ROWS23: &[&[&str]] = &[
    &["General", "Video", "Preview Device", "Audio", "Audio Device", "MIDI", "VST Effects"],
    &["Editing", "Display", "CD Settings", "Sync", "External Control & Automation"],
    &["File I/O", "Deprecated Features"],
];
const GENERAL: usize = 0;
const DISPLAY: usize = 8;

#[derive(Default)]
pub struct PrefsDialog {
    draft: Option<Prefs>,
    tab: usize,
    track: usize,
}

impl PrefsDialog {
    pub fn open(&mut self, prefs: &Prefs) {
        self.draft = Some(prefs.clone());
    }

    /// Open on the Display tab (screenshot harness).
    pub fn open_display(&mut self, prefs: &Prefs) {
        self.open(prefs);
        self.tab = DISPLAY;
    }

    pub fn is_open(&self) -> bool {
        self.draft.is_some()
    }

    /// Show the dialog; returns the preferences to apply on OK / Apply.
    pub fn show(&mut self, ctx: &Context, current: &Prefs) -> Option<Prefs> {
        let d = self.draft.as_mut()?;
        let t = theme::get();
        let rows = if t.look() == UiLook::Vegas16 { ROWS16 } else { ROWS23 };
        let (tab, track) = (&mut self.tab, &mut self.track);
        let changed = d != current;
        let (hit, close) = super::modal(ctx, "preferences", "Preferences", vec2(433.0, 670.0), |ui| {
            super::tabs(ui, rows, tab);
            super::page(ui, 540.0, |ui| match *tab {
                GENERAL => general(ui, d),
                DISPLAY => display(ui, d, track),
                _ => {
                    ui.add_space(8.0);
                    ui.label("Milan Pro has no settings on this page yet.");
                }
            });
            ui.add_space(8.0);
            super::button_row(ui, &[("OK", true), ("Cancel", true), ("Apply", changed)])
        });
        let out = matches!(hit, Some(0 | 2)).then(|| d.clone());
        if matches!(hit, Some(0 | 1)) || close {
            self.draft = None;
        }
        out
    }
}

fn general(ui: &mut Ui, d: &mut Prefs) {
    let v16 = theme::get().look() == UiLook::Vegas16;
    ui.label("General preferences:");
    let dd = theme::get().dialog();
    Frame::NONE.fill(dd.field).stroke(Stroke::new(1.0_f32, dd.field_border)).inner_margin(4).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.set_height(16.0 * 24.0);
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.spacing_mut().interact_size.y = 16.0;
        ui.add_enabled(false, Checkbox::new(&mut false, "Automatically open last project on startup"));
        ui.add_enabled(false, Checkbox::new(&mut true, "Show logo splash screen on startup"));
        let mut pause = d.space_pauses || !v16;
        if ui.add_enabled(v16, Checkbox::new(&mut pause, "Make spacebar and F12 Play/Pause instead of Play/Stop")).changed() {
            d.space_pauses = pause;
        }
        ui.add_enabled(!v16, Checkbox::new(&mut d.show_welcome, "Show Welcome Screen on startup"));
        ui.checkbox(&mut d.auto_update, "Check for updates automatically");
    });
    ui.horizontal(|ui| {
        let mut on = true;
        ui.add_enabled(false, Checkbox::new(&mut on, "Recently used project list"));
        ui.add(DragValue::new(&mut d.recent_max).range(1..=9));
    });
    ui.add_enabled_ui(false, |ui| {
        ui.label("Temporary files folder:");
        ui.horizontal(|ui| {
            ui.add(TextEdit::singleline(&mut std::env::temp_dir().display().to_string()).desired_width(300.0));
            let _ = ui.button("Browse...");
        });
    });
    if default_all(ui) {
        (d.space_pauses, d.show_welcome, d.recent_max, d.auto_update) = (false, true, 4, true);
    }
}

/// "Default All" at the bottom right of the page, under an etched rule.
fn default_all(ui: &mut Ui) -> bool {
    let left = (ui.max_rect().max.y - ui.min_rect().max.y - 40.0).max(4.0);
    ui.add_space(left);
    ui.separator();
    ui.with_layout(Layout::right_to_left(Align::Min), |ui| super::button(ui, "Default All", true, false).clicked()).inner
}

const COLOR_NAMES: [&str; 8] = ["Color A", "Color B", "Color C", "Color D", "Color E", "Color F", "Color G", "Color H"];

fn display(ui: &mut Ui, d: &mut Prefs, track: &mut usize) {
    let t = theme::get();
    let v16 = t.look() == UiLook::Vegas16;
    // The look's own palette for this preview, whatever the current override.
    let base = theme::Theme::new(d.look).track_colors;
    let mut palette = d.palette().unwrap_or(base);

    super::group(ui, "Track Colors");
    let changed = super::row(ui, "      Track:", 120.0, true, |ui| {
        ComboBox::from_id_salt("pref_track").width(200.0).selected_text(COLOR_NAMES[*track]).show_ui(ui, |ui| {
            for (k, n) in COLOR_NAMES.iter().enumerate() {
                ui.selectable_value(track, k, *n);
            }
        });
        ui.spacing_mut().interact_size = vec2(60.0, 23.0);
        egui::color_picker::color_edit_button_srgba(ui, &mut palette[*track], egui::color_picker::Alpha::Opaque).changed()
    });
    if changed {
        d.track_colors = Some(palette.map(|c| [c.r(), c.g(), c.b()]));
    }
    super::group(ui, "Envelope Colors");
    super::row(ui, "      Envelope Type:", 120.0, false, |ui| {
        super::fixed_combo(ui, "pref_env", "Volume", 200.0);
        super::swatch(ui, t.envelope_volume, false);
    });
    super::group(ui, "Snap Colors");
    super::row(ui, "      Snap Type:", 120.0, false, |ui| {
        super::fixed_combo(ui, "pref_snap", "Cursor", 200.0);
        super::swatch(ui, t.snap.cursor, false);
    });
    ui.add_space(4.0);
    super::row(ui, "Icon color strength:", 120.0, false, |ui| {
        let mut v = 75.0_f32;
        ui.add(Slider::new(&mut v, 0.0..=100.0).show_value(false));
        ui.label("75 %");
    });

    // Milan Pro: which VEGAS generation the interface follows.
    super::group(ui, "Interface Look");
    ui.horizontal(|ui| {
        for (look, label) in [(UiLook::Vegas16, "VEGAS 16"), (UiLook::Vegas23, "VEGAS 23")] {
            if ui.radio(d.look.look == look, label).clicked() {
                d.look = if look == UiLook::Vegas16 { LookChoice::VEGAS16 } else { LookChoice { look, variant: d.look.variant } };
            }
        }
    });
    super::group(ui, "Interface Type");
    for (variant, label, enabled) in [
        (Some(Variant::Dark), "Dark", true),
        (None, "Medium", false),
        (None, "Light", false),
        (Some(Variant::White), "White", d.look.look == UiLook::Vegas23),
    ] {
        let on = variant == Some(d.look.variant);
        if ui.add_enabled(enabled, RadioButton::new(on, label)).clicked() {
            if let Some(v) = variant {
                d.look.variant = v;
            }
        }
    }
    ui.add_space(4.0);
    ui.add_enabled_ui(false, |ui| {
        ui.checkbox(&mut false, "Automatically hide docking area");
        ui.checkbox(&mut true, "Display timeline at bottom of main window");
        ui.checkbox(&mut false, "Position tabs at top of docked windows");
        ui.checkbox(&mut !v16, "Allow floating windows to dock (hold Ctrl to override)");
    });
    if default_all(ui) {
        d.track_colors = None;
        d.look.variant = Variant::Dark;
    }
}
