//! Options › Preferences (§5.15.1): two (VP23: three) rows of tabs; General, Video, Audio Device, Editing and Display
//! are live.
use eframe::egui::{self, *};
use crate::settings::Prefs;
use super::super::menus::{ALWAYS_ON, Fixed, Later, NotPlanned, Why};
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
const VIDEO: usize = 1;
const AUDIO_DEVICE: usize = 4;
const EDITING: usize = 7;
const DISPLAY: usize = 8;

#[derive(Default)]
pub struct PrefsDialog {
    draft: Option<Prefs>,
    tab: usize,
    track: usize,
    /// Input device names, listed once per opening of the Audio Device page (listing devices is slow).
    inputs: Option<Vec<String>>,
}

impl PrefsDialog {
    pub fn open(&mut self, prefs: &Prefs) {
        self.draft = Some(prefs.clone());
        self.inputs = None;
    }

    /// Open on the Display tab (screenshot harness).
    pub fn open_display(&mut self, prefs: &Prefs) {
        self.open(prefs);
        self.tab = DISPLAY;
    }

    /// Open on the Audio Device tab (screenshot harness).
    pub fn open_audio_device(&mut self, prefs: &Prefs) {
        self.open(prefs);
        self.tab = AUDIO_DEVICE;
    }

    pub fn is_open(&self) -> bool {
        self.draft.is_some()
    }

    /// Show the dialog; returns the preferences to apply on OK / Apply.
    pub fn show(&mut self, ctx: &Context, current: &Prefs) -> Option<Prefs> {
        let d = self.draft.as_mut()?;
        // Layouts change behind the dialog (Apply on a new look saves the old look's); OK must not hand back stale ones.
        if d.layouts != current.layouts {
            d.layouts.clone_from(&current.layouts);
        }
        let t = theme::get();
        let rows = if t.look() == UiLook::Vegas16 { ROWS16 } else { ROWS23 };
        let (tab, track, inputs) = (&mut self.tab, &mut self.track, &mut self.inputs);
        let changed = d != current;
        let (hit, close) = super::modal(ctx, "preferences", "Preferences", vec2(433.0, 670.0), |ui| {
            super::tabs(ui, rows, tab);
            super::page(ui, 540.0, |ui| match *tab {
                GENERAL => general(ui, d),
                VIDEO => video(ui, d),
                AUDIO_DEVICE => audio_device(ui, d, inputs.get_or_insert_with(crate::record::input_devices)),
                EDITING => editing(ui, d),
                DISPLAY => display(ui, d, track),
                k => super::empty_page(ui, page_why(rows.iter().flat_map(|r| r.iter()).nth(k).copied().unwrap_or(""))),
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

/// Why a Preferences page has no settings yet.
fn page_why(page: &str) -> Why {
    match page {
        "Preview Device" => Later("R14"),
        "Audio" => Later("R9"),
        "VST Effects" => Later("R31"),
        "MIDI" | "CD Settings" | "Sync" | "External Control & Automation" => NotPlanned("R59"),
        _ => Later("R49"), // File I/O, Deprecated Features
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
        let r49 = Later("R49").text();
        ui.add_enabled(false, Checkbox::new(&mut false, "Automatically open last project on startup")).on_disabled_hover_text(&r49);
        ui.add_enabled(false, Checkbox::new(&mut true, "Show logo splash screen on startup")).on_disabled_hover_text(&r49);
        let mut pause = d.space_pauses || !v16;
        if ui
            .add_enabled(v16, Checkbox::new(&mut pause, "Make spacebar and F12 Play/Pause instead of Play/Stop"))
            .on_disabled_hover_text(Fixed("always on in the VEGAS 23 look").text())
            .changed()
        {
            d.space_pauses = pause;
        }
        ui.add_enabled(!v16, Checkbox::new(&mut d.show_welcome, "Show Welcome Screen on startup"))
            .on_disabled_hover_text(Fixed("the Welcome Screen belongs to the VEGAS 23 look").text());
        ui.checkbox(&mut d.auto_update, "Check for updates automatically");
    });
    ui.horizontal(|ui| {
        let mut on = true;
        ui.add_enabled(false, Checkbox::new(&mut on, "Recently used project list")).on_disabled_hover_text(ALWAYS_ON.text());
        ui.add(DragValue::new(&mut d.recent_max).range(1..=9));
    });
    ui.add_enabled_ui(false, |ui| {
        let r49 = Later("R49").text();
        ui.label("Temporary files folder:").on_disabled_hover_text(&r49);
        ui.horizontal(|ui| {
            ui.add(TextEdit::singleline(&mut std::env::temp_dir().display().to_string()).desired_width(300.0)).on_disabled_hover_text(&r49);
            ui.button("Browse...").on_disabled_hover_text(&r49);
        });
    });
    if default_all(ui) {
        (d.space_pauses, d.show_welcome, d.recent_max, d.auto_update) = (false, true, 4, true);
    }
}

fn video(ui: &mut Ui, d: &mut Prefs) {
    // VEGAS creates proxies for 4K video automatically; Milan Pro makes that opt-in (off by default, per the plan),
    // besides Project Media › Create Video Proxy.
    ui.checkbox(&mut d.auto_proxy, "Automatically create video proxies for 4K, HEVC and AV1 media on import");
    if default_all(ui) {
        d.auto_proxy = false;
    }
}

/// Audio Device: playback goes to the Windows default device; recording takes the chosen input and its latency offset.
fn audio_device(ui: &mut Ui, d: &mut Prefs, inputs: &[String]) {
    const DEFAULT: &str = "Windows default device";
    super::row(ui, "Audio device type:", 175.0, false, |ui| super::fixed_combo(ui, "pad_type", "Windows Classic Wave Driver", 200.0, Later("R9")));
    super::row(ui, "Default playback device:", 175.0, false, |ui| super::fixed_combo(ui, "pad_out", DEFAULT, 200.0, Later("R9")));
    super::row(ui, "Default record device:", 175.0, true, |ui| {
        let shown = d.record_input.clone().unwrap_or_else(|| DEFAULT.into());
        ComboBox::from_id_salt("pad_rec").width(200.0).selected_text(shown).show_ui(ui, |ui| {
            ui.selectable_value(&mut d.record_input, None, DEFAULT);
            for n in inputs {
                ui.selectable_value(&mut d.record_input, Some(n.clone()), n.as_str());
            }
        });
    });
    super::row(ui, "Recording latency offset (ms):", 175.0, true, |ui| {
        ui.add(DragValue::new(&mut d.record_offset_ms).range(-1000.0..=1000.0).speed(0.5).fixed_decimals(1))
            .on_hover_text("Recorded events are placed this much earlier, to line up with what was playing")
    });
    if inputs.is_empty() {
        ui.label("No recording devices were found.");
    }
    if default_all(ui) {
        (d.record_input, d.record_offset_ms) = (None, 0.0);
    }
}

fn editing(ui: &mut Ui, d: &mut Prefs) {
    ui.horizontal(|ui| {
        ui.label("New still image length (seconds):");
        ui.add(DragValue::new(&mut d.still_len).range(0.1..=3600.0).speed(0.1).fixed_decimals(3));
    });
    ui.horizontal(|ui| {
        ui.label("Cut-to-overlap conversion (seconds):").on_hover_text("The crossfade a transition dropped on a cut makes, centred on the cut");
        ui.add(DragValue::new(&mut d.cut_overlap).range(0.0..=60.0).speed(0.05).fixed_decimals(3));
    });
    if default_all(ui) {
        (d.still_len, d.cut_overlap) = (5.0, 1.0);
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
    let r49 = Later("R49").text();
    super::row(ui, "      Envelope Type:", 120.0, false, |ui| {
        super::fixed_combo(ui, "pref_env", "Volume", 200.0, Later("R49"));
        super::swatch(ui, t.envelope_volume, false).on_disabled_hover_text(&r49);
    });
    super::group(ui, "Snap Colors");
    super::row(ui, "      Snap Type:", 120.0, false, |ui| {
        super::fixed_combo(ui, "pref_snap", "Cursor", 200.0, Later("R49"));
        super::swatch(ui, t.snap.cursor, false).on_disabled_hover_text(&r49);
    });
    ui.add_space(4.0);
    super::row(ui, "Icon color strength:", 120.0, false, |ui| {
        let mut v = 75.0_f32;
        ui.add(Slider::new(&mut v, 0.0..=100.0).show_value(false)).on_disabled_hover_text(&r49);
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
        let why = if variant.is_some() { Fixed("the VEGAS 16 look has the Dark type only") } else { Later("R49") };
        if ui.add_enabled(enabled, RadioButton::new(on, label)).on_disabled_hover_text(why.text()).clicked() {
            if let Some(v) = variant {
                d.look.variant = v;
            }
        }
    }
    ui.add_space(4.0);
    ui.add_enabled_ui(false, |ui| {
        let (r13, r14) = (Later("R13").text(), Later("R14").text());
        ui.checkbox(&mut false, "Automatically hide docking area").on_disabled_hover_text(&r13);
        ui.checkbox(&mut true, "Display timeline at bottom of main window").on_disabled_hover_text(&r13);
        ui.checkbox(&mut false, "Position tabs at top of docked windows").on_disabled_hover_text(&r13);
        ui.checkbox(&mut !v16, "Allow floating windows to dock (hold Ctrl to override)").on_disabled_hover_text(&r14);
    });
    if default_all(ui) {
        d.track_colors = None;
        d.look.variant = Variant::Dark;
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    /// Apply a new look, the app saves the old look's last layout, then OK: the dialog hands that layout back
    /// (it does not undo it) and Apply goes off again.
    #[test]
    fn ok_after_apply_keeps_the_layouts_saved_meanwhile() {
        let mut dlg = PrefsDialog::default();
        let mut current = Prefs::default();
        dlg.open(&current);
        let l = crate::ui::dock::Dock::new(&theme::Theme::new(LookChoice::VEGAS16)).layout("");
        current.layouts.insert(crate::settings::layout_key(UiLook::Vegas16, "last"), l);
        let ctx = Context::default();
        let _ = ctx.run(RawInput::default(), |ctx| assert!(dlg.show(ctx, &current).is_none()));
        assert_eq!(dlg.draft.as_ref(), Some(&current));
    }
}
