//! Options › Preferences (§5.15.1): two (VP23: three) rows of tabs; General, Video, Audio Device, Editing and Display
//! are live, and Audio has the preferred audio editor (R18a) and the loudness log option (R16c).
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
pub const PREVIEW_DEVICE: usize = 2;
pub const AUDIO: usize = 3;
pub const AUDIO_DEVICE: usize = 4;
const EDITING: usize = 7;
pub const DISPLAY: usize = 8;

#[derive(Default)]
pub struct PrefsDialog {
    draft: Option<Prefs>,
    tab: usize,
    track: usize,
    /// Input and output device names, listed once per opening of the Audio Device page (listing devices is slow).
    inputs: Option<Vec<String>>,
    outputs: Option<Vec<String>>,
    /// R14: the displays, listed once per opening of the Preview Device page.
    monitors: Option<Vec<crate::ui::monitors::Monitor>>,
    /// R13 (VP16) Display: "Display timeline at bottom of main window", which the app reads from and applies to the
    /// dock (the place of its one area is part of the window layout).
    pub timeline_bottom: bool,
}

impl PrefsDialog {
    pub fn open(&mut self, prefs: &Prefs) {
        self.draft = Some(prefs.clone());
        (self.inputs, self.outputs, self.monitors) = (None, None, None);
    }

    /// Open on page `tab` (`DISPLAY`, `AUDIO_DEVICE`, `PREVIEW_DEVICE`; screenshot harness).
    pub fn open_on(&mut self, prefs: &Prefs, tab: usize) {
        self.open(prefs);
        self.tab = tab;
    }

    pub fn is_open(&self) -> bool {
        self.draft.is_some()
    }

    /// Show the dialog; returns the preferences to apply on OK / Apply. `latency`: the audio device's output, input and
    /// monitoring round-trip latency in ms (`audio::Player::latency_ms`), shown on the Audio Device page.
    /// `timeline_bottom_now`: where the VP16 dock area is (so a change of it enables Apply).
    pub fn show(&mut self, ctx: &Context, current: &Prefs, latency: Option<(f64, f64, f64)>, timeline_bottom_now: bool) -> Option<Prefs> {
        let d = self.draft.as_mut()?;
        // Layouts change behind the dialog (Apply on a new look saves the old look's); OK must not hand back stale ones.
        if d.layouts != current.layouts {
            d.layouts.clone_from(&current.layouts);
        }
        let t = theme::get();
        let rows = if t.look() == UiLook::Vegas16 { ROWS16 } else { ROWS23 };
        let changed = *d != *current || self.timeline_bottom != timeline_bottom_now;
        let (tab, track, inputs, outputs, bottom) = (&mut self.tab, &mut self.track, &mut self.inputs, &mut self.outputs, &mut self.timeline_bottom);
        let monitors = &mut self.monitors;
        let (hit, close) = super::modal(ctx, "preferences", "Preferences", vec2(433.0, 670.0), |ui| {
            super::tabs(ui, rows, tab);
            super::page(ui, 540.0, |ui| match *tab {
                GENERAL => general(ui, d),
                VIDEO => video(ui, d),
                PREVIEW_DEVICE => preview_device(ui, d, monitors.get_or_insert_with(crate::ui::monitors::list)),
                AUDIO => audio(ui, d),
                AUDIO_DEVICE => audio_device(ui, d, inputs.get_or_insert_with(crate::record::input_devices), outputs.get_or_insert_with(crate::audio::output_devices), latency),
                EDITING => editing(ui, d),
                DISPLAY => display(ui, d, track, bottom),
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
        // R31 built the OFX folders (Plug-In Manager › Folders); VST folders come with the VST3 host.
        "VST Effects" => Later("R34a"),
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
        ui.checkbox(&mut d.nested_breadcrumb, "Show the parent timelines of a nested timeline above the track list");
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
        (d.space_pauses, d.show_welcome, d.recent_max, d.auto_update, d.nested_breadcrumb) = (false, true, 4, true, false);
    }
}

/// Audio: R18a's preferred audio editor (Tools › Open in Audio Editor) and R16c's loudness log option; the page's other
/// settings wait for R49.
fn audio(ui: &mut Ui, d: &mut Prefs) {
    ui.label("Preferred audio editor:");
    ui.horizontal(|ui| {
        let mut path = d.audio_editor.as_ref().map_or(String::new(), |p| p.display().to_string());
        if ui.add(TextEdit::singleline(&mut path).desired_width(320.0)).changed() {
            d.audio_editor = (!path.trim().is_empty()).then(|| path.trim().into());
        }
        if ui.button("Browse...").clicked() {
            if let Some(p) = rfd::FileDialog::new().add_filter("Programs", &["exe"]).pick_file() {
                d.audio_editor = Some(p);
            }
        }
    });
    ui.add_space(8.0);
    ui.checkbox(&mut d.loudness.open_editor, "Open editor when new loudness log is generated");
    ui.add_space(8.0);
    ui.label(format!("Milan Pro has no other settings on this page ({}).", Later("R49").phrase()));
}

fn video(ui: &mut Ui, d: &mut Prefs) {
    // VEGAS creates proxies for 4K video automatically; Milan Pro makes that opt-in (off by default, per the plan),
    // besides Project Media › Create Video Proxy.
    ui.checkbox(&mut d.auto_proxy, "Automatically create video proxies for 4K, HEVC and AV1 media on import");
    // Milan Pro's own, off by default: compositing modes VEGAS does not have.
    ui.checkbox(&mut d.extra_blend_modes, "Offer extra blend modes (Soft Light, Exclusion, Hard Mix, Hue, Saturation, Color, Luminosity)")
        .on_hover_text("Not VEGAS modes: a project using them looks different in VEGAS");
    if default_all(ui) {
        (d.auto_proxy, d.extra_blend_modes) = (false, false);
    }
}

/// R14: Preview Device, VEGAS's Windows Graphics Card device (a secondary display): the display Preview on External
/// Monitor uses and how the picture goes on it. VEGAS's other devices (IEEE 1394, SDI cards) are not planned.
fn preview_device(ui: &mut Ui, d: &mut Prefs, monitors: &[crate::ui::monitors::Monitor]) {
    const W: f32 = 230.0;
    super::row(ui, "Device:", 110.0, true, |ui| {
        ComboBox::from_id_salt("ppd_device").width(W).selected_text("Windows Graphics Card").show_ui(ui, |ui| {
            let _ = ui.selectable_label(true, "Windows Graphics Card");
            ui.add_enabled(false, SelectableLabel::new(false, "IEEE 1394/OHCI Compatible Device")).on_disabled_hover_text(NotPlanned("R59").text());
        });
    });
    ui.add_space(6.0);
    super::group(ui, "Details");
    let auto = "Automatic (the first secondary display)";
    let shown = d.ext_display.as_deref().map_or(auto.to_string(), |n| monitors.iter().find(|m| m.name == n).map_or(format!("{n} (not connected)"), |m| m.label()));
    super::row(ui, "Display adapter:", 110.0, true, |ui| {
        ComboBox::from_id_salt("ppd_display").width(W).selected_text(shown).show_ui(ui, |ui| {
            ui.selectable_value(&mut d.ext_display, None, auto);
            for m in monitors {
                ui.selectable_value(&mut d.ext_display, Some(m.name.clone()), m.label());
            }
        });
    });
    if monitors.is_empty() {
        ui.label("No displays were found.");
    }
    ui.add_enabled(false, Button::new("Identify Displays")).on_disabled_hover_text(Later("R49").text());
    ui.add_space(6.0);
    ui.checkbox(&mut d.ext_fit, "Scale output to fit display").on_hover_text("Off: one project pixel per display pixel, centred");
    let r49 = Later("R49").text();
    ui.add_enabled(false, Checkbox::new(&mut false, "Apply deinterlace filter")).on_disabled_hover_text(&r49);
    ui.add_enabled(false, Checkbox::new(&mut false, "Adjust levels from studio RGB to computer RGB")).on_disabled_hover_text(&r49);
    ui.add_enabled(false, Checkbox::new(&mut true, "Wait for vertical sync")).on_disabled_hover_text(ALWAYS_ON.text());
    ui.add_enabled(false, Checkbox::new(&mut true, "Optimize GPU display performance")).on_disabled_hover_text(ALWAYS_ON.text());
    ui.add_enabled(false, Checkbox::new(&mut false, "Recompress edited frames")).on_disabled_hover_text(NotPlanned("R59").text());
    ui.checkbox(&mut d.ext_in_window, "Display frames in Video Preview window during playback");
    ui.checkbox(&mut d.ext_rotate, "Use project output rotation setting");
    if default_all(ui) {
        (d.ext_display, d.ext_fit, d.ext_in_window, d.ext_rotate) = (None, false, true, true);
    }
}

/// The Buffer size choices (frames; 0 the device's default), VEGAS's Advanced Audio Configuration › Buffer size.
const BUFFERS: [u32; 8] = [0, 64, 128, 256, 512, 1024, 2048, 4096];

/// The Audio Device page's label and combo widths (VEGAS's long device labels).
const LABEL_W: f32 = 225.0;
const COMBO_W: f32 = 185.0;

fn buffer_text(n: u32) -> String {
    if n == 0 { "Default".into() } else { format!("{n} samples") }
}

/// Audio Device (R9): the device type (VEGAS's Microsoft Sound Mapper and Windows Classic Wave Driver, and Milan Pro's
/// exclusive mode), the playback device, the buffer size, the recording input and its latency offset, and the latency
/// the device reports.
fn audio_device(ui: &mut Ui, d: &mut Prefs, inputs: &[String], outputs: &[String], latency: Option<(f64, f64, f64)>) {
    use crate::audio::DeviceType;
    const DEFAULT: &str = "Windows default device";
    super::row(ui, "Audio device type:", LABEL_W, true, |ui| {
        ComboBox::from_id_salt("pad_type").width(COMBO_W).selected_text(d.device_type.name()).show_ui(ui, |ui| {
            for k in DeviceType::ALL {
                if !cfg!(windows) && k == DeviceType::Exclusive {
                    continue;
                }
                let r = ui.selectable_value(&mut d.device_type, k, k.name());
                if k == DeviceType::Exclusive {
                    r.on_hover_text("Milan Pro's low-latency output (not in VEGAS): takes the device for itself, so other programs go silent while Milan Pro runs");
                }
            }
            ui.add_enabled(false, SelectableLabel::new(false, "ASIO")).on_disabled_hover_text(Fixed("not feasible: the ASIO SDK licence forbids shipping it (use exclusive mode for low latency)").text());
        });
    });
    let mapper = d.device_type == DeviceType::SoundMapper;
    super::row(ui, "Default stereo and front playback device:", LABEL_W, !mapper, |ui| {
        if mapper {
            ui.add_enabled_ui(false, |ui| ComboBox::from_id_salt("pad_out").width(COMBO_W).selected_text("Microsoft Sound Mapper").show_ui(ui, |_| {}).response)
                .inner
                .on_disabled_hover_text(Fixed("the Microsoft Sound Mapper plays on the Windows default device").text());
            return;
        }
        let shown = d.playback_device.clone().unwrap_or_else(|| DEFAULT.into());
        ComboBox::from_id_salt("pad_out").width(COMBO_W).selected_text(shown).show_ui(ui, |ui| {
            ui.selectable_value(&mut d.playback_device, None, DEFAULT);
            for n in outputs {
                ui.selectable_value(&mut d.playback_device, Some(n.clone()), n.as_str());
            }
        });
    });
    super::row(ui, "Default rear playback device:", LABEL_W, false, |ui| super::fixed_combo(ui, "pad_rear", "None", COMBO_W, Later("R10a")));
    super::row(ui, "Default center and LFE playback device:", LABEL_W, false, |ui| super::fixed_combo(ui, "pad_lfe", "None", COMBO_W, Later("R10a")));
    super::row(ui, "Buffer size:", LABEL_W, true, |ui| {
        ComboBox::from_id_salt("pad_buf")
            .width(COMBO_W)
            .selected_text(buffer_text(d.buffer_frames))
            .show_ui(ui, |ui| {
                for n in BUFFERS {
                    ui.selectable_value(&mut d.buffer_frames, n, buffer_text(n));
                }
            })
            .response
            .on_hover_text("Smaller buffers lower the latency of playback and input monitoring; too small for this computer, and the sound breaks up");
    });
    super::row(ui, "Default audio recording device:", LABEL_W, true, |ui| {
        let shown = d.record_input.clone().unwrap_or_else(|| DEFAULT.into());
        ComboBox::from_id_salt("pad_rec").width(COMBO_W).selected_text(shown).show_ui(ui, |ui| {
            ui.selectable_value(&mut d.record_input, None, DEFAULT);
            for n in inputs {
                ui.selectable_value(&mut d.record_input, Some(n.clone()), n.as_str());
            }
        });
    });
    super::row(ui, "Recording latency offset (ms):", LABEL_W, true, |ui| {
        ui.add(DragValue::new(&mut d.record_offset_ms).range(-1000.0..=1000.0).speed(0.5).fixed_decimals(1))
            .on_hover_text("Recorded events are placed this much earlier, to line up with what was playing")
    });
    if inputs.is_empty() {
        ui.label("No recording devices were found.");
    }
    // What the device reports, for checking against a loopback cable (output to input).
    ui.add_space(6.0);
    ui.label(match latency {
        Some((out, inp, round)) => format!("Output latency: {out:.1} ms    Input latency: {inp:.1} ms\nInput monitoring round trip: {round:.1} ms"),
        None => "Latency: the audio device is not open.".into(),
    });
    if default_all(ui) {
        (d.record_input, d.record_offset_ms) = (None, 0.0);
        (d.device_type, d.playback_device, d.buffer_frames) = (DeviceType::SoundMapper, None, 0);
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
    ui.horizontal(|ui| {
        ui.label("Cursor preview duration (seconds):").on_hover_text("What Preview at Cursor (Num 0, Ctrl+Shift+Space) plays, centred on the cursor");
        ui.add(DragValue::new(&mut d.cursor_preview).range(0.1..=60.0).speed(0.1).fixed_decimals(3));
    });
    if default_all(ui) {
        (d.still_len, d.cut_overlap, d.cursor_preview) = (5.0, 1.0, 3.0);
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

fn display(ui: &mut Ui, d: &mut Prefs, track: &mut usize, timeline_bottom: &mut bool) {
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
    // The docking options (§4.3); VP23 dropped "Display timeline at bottom" (its Timeline Top layout does that).
    ui.checkbox(&mut d.auto_hide_docks, "Automatically hide docking area");
    if v16 {
        ui.checkbox(timeline_bottom, "Display timeline at bottom of main window");
    }
    ui.checkbox(&mut d.tabs_on_top, "Position tabs at top of docked windows");
    let mut float_dock = d.float_dock(d.look.look);
    if ui.checkbox(&mut float_dock, "Allow floating windows to dock (hold Ctrl to override)").changed() {
        d.float_dock = Some(float_dock);
    }
    if default_all(ui) {
        d.track_colors = None;
        d.look.variant = Variant::Dark;
        (d.auto_hide_docks, d.tabs_on_top, *timeline_bottom, d.float_dock) = (false, false, true, None);
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
        let _ = ctx.run(RawInput::default(), |ctx| assert!(dlg.show(ctx, &current, None, true).is_none()));
        assert_eq!(dlg.draft.as_ref(), Some(&current));
    }
}
