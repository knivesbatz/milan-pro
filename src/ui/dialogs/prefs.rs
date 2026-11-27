//! Options › Preferences (VEGAS_UI_SPEC.md §5.15.1): three rows of tabs. Every page is live or says why a control is
//! not (R49): General, Video, Preview Device, Audio, Audio Device, VST Effects, Editing, Display and Milan Pro's Update;
//! File I/O and Deprecated Features hold VEGAS switches for readers and plug-ins Milan Pro does not have; MIDI, CD
//! Settings, Sync and External Control & Automation are not planned (R59).
use eframe::egui::{self, *};
use crate::ui::widgets;
use crate::settings::Prefs;
use super::super::menus::{ALWAYS_ON, Fixed, NotPlanned, Why};
use super::super::theme;

// "Update" is Milan Pro's own page (R44/R46): the updater, the log and crash-report options.
const ROWS: &[&[&str]] = &[
    &["General", "Video", "Preview Device", "Audio", "Audio Device", "MIDI", "VST Effects"],
    &["Editing", "Display", "CD Settings", "Sync", "External Control & Automation"],
    &["File I/O", "Deprecated Features", "Update"],
];

/// The Update page's index in `ROWS` (it is last).
pub fn update_tab() -> usize {
    ROWS.iter().flat_map(|r| r.iter()).position(|n| *n == "Update").unwrap_or(0)
}
const GENERAL: usize = 0;
pub const VIDEO: usize = 1;
pub const PREVIEW_DEVICE: usize = 2;
pub const AUDIO: usize = 3;
pub const AUDIO_DEVICE: usize = 4;
pub const VST_EFFECTS: usize = 6;
const EDITING: usize = 7;
pub const DISPLAY: usize = 8;
const FILE_IO: usize = 12;
const DEPRECATED: usize = 13;

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
    /// R49: the Display page's Envelope Type and Snap Type choices.
    env_kind: usize,
    snap_kind: usize,
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
    pub fn show(&mut self, ctx: &Context, current: &Prefs, latency: Option<(f64, f64, f64)>) -> Option<Prefs> {
        let d = self.draft.as_mut()?;
        // Layouts change behind the dialog (another window saves its own); OK must not hand back stale ones.
        if d.layouts != current.layouts {
            d.layouts.clone_from(&current.layouts);
        }
        let rows = ROWS;
        let changed = *d != *current;
        let (tab, track, inputs, outputs) = (&mut self.tab, &mut self.track, &mut self.inputs, &mut self.outputs);
        let monitors = &mut self.monitors;
        let kinds = (&mut self.env_kind, &mut self.snap_kind);
        let (hit, close) = super::modal(ctx, "preferences", "Preferences", vec2(433.0, 670.0), |ui| {
            super::tabs(ui, rows, tab);
            super::page(ui, 540.0, |ui| match *tab {
                GENERAL => general(ui, d),
                VIDEO => video(ui, d),
                PREVIEW_DEVICE => preview_device(ui, d, monitors.get_or_insert_with(crate::ui::monitors::list)),
                AUDIO => audio(ui, d),
                AUDIO_DEVICE => audio_device(ui, d, inputs.get_or_insert_with(crate::record::input_devices), outputs.get_or_insert_with(crate::audio::output_devices), latency),
                VST_EFFECTS => vst_effects(ui, d),
                EDITING => editing(ui, d),
                DISPLAY => display(ui, d, track, kinds),
                FILE_IO => file_io(ui),
                DEPRECATED => deprecated(ui),
                k if k == update_tab() => update(ui, d),
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
fn page_why(_page: &str) -> Why {
    // MIDI, CD Settings, Sync, External Control & Automation.
    NotPlanned("R59")
}

fn general(ui: &mut Ui, d: &mut Prefs) {
    ui.label("General preferences:");
    super::field_box(ui, 4, |ui| {
        ui.set_width(ui.available_width());
        // R49: as many rows as there are options (VEGAS's list is longer), so the page fits at 150 %.
        ui.set_height(16.0 * 12.0);
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.spacing_mut().interact_size.y = 16.0;
        widgets::checkbox(ui, &mut d.open_last, "Automatically open last project on startup");
        widgets::checkbox(ui, &mut d.show_splash, "Show logo splash screen on startup");
        // Space always plays and pauses (the VEGAS 23 keys).
        ui.add_enabled(false, Checkbox::new(&mut true, "Make spacebar and F12 Play/Pause instead of Play/Stop")).on_disabled_hover_text(ALWAYS_ON.text());
        widgets::checkbox(ui, &mut d.show_welcome, "Show Welcome Screen on startup");
        widgets::checkbox(ui, &mut d.auto_update, "Check for updates automatically");
        widgets::checkbox(ui, &mut d.nested_breadcrumb, "Show the parent timelines of a nested timeline above the track list");
    });
    ui.horizontal(|ui| {
        let mut on = true;
        widgets::checkbox_enabled(ui, false, &mut on, "Recently used project list").on_disabled_hover_text(ALWAYS_ON.text());
        widgets::spin(ui, &mut d.recent_max, 1.0, |d| d.range(1..=9));
    });
    // R43: autosave (a backup copy in AppData, never the project file) and how many copies are kept per project.
    ui.horizontal(|ui| {
        widgets::checkbox(ui, &mut d.autosave_on, "Save a backup copy of the project every");
        widgets::spin_enabled(ui, d.autosave_on, &mut d.autosave_minutes, 1.0, |s| s.range(1..=60));
        ui.label("minutes");
    });
    ui.horizontal(|ui| {
        ui.label("Backup copies kept per project:");
        widgets::spin_enabled(ui, d.autosave_on, &mut d.autosave_keep, 1.0, |s| s.range(1..=20));
    });
    // R49: where Milan Pro and FFmpeg write temporary files (renders' audio, intermediate files); from the next start.
    ui.label("Temporary files folder:");
    let shown = d.temp_folder.clone().unwrap_or_else(crate::settings::system_temp);
    ui.horizontal(|ui| {
        let mut path = shown.display().to_string();
        if widgets::edit(ui, TextEdit::singleline(&mut path).desired_width(300.0)).on_hover_text("Takes effect the next time Milan Pro starts").changed() {
            d.temp_folder = (!path.trim().is_empty()).then(|| path.trim().into()).filter(|p| *p != crate::settings::system_temp());
        }
        if widgets::push_button(ui, "Browse...").clicked() {
            if let Some(p) = rfd::FileDialog::new().set_title("Temporary files folder").pick_folder() {
                d.temp_folder = Some(p);
            }
        }
    });
    ui.label(match crate::ui::status::free_space(&shown) {
        Some(b) => format!("Free storage space in selected folder: {:.1} Gigabytes", b as f64 / 1e9),
        None => "Free storage space in selected folder: unknown (the folder is not there)".into(),
    });
    // Milan Pro's own: where the Overlays window keeps the packs it imports.
    ui.label("Overlay pack library folder:");
    ui.horizontal(|ui| {
        let mut path = d.overlay_library.clone().unwrap_or_else(crate::overlays::default_library).display().to_string();
        if widgets::edit(ui, TextEdit::singleline(&mut path).desired_width(300.0)).changed() {
            d.overlay_library = (!path.trim().is_empty()).then(|| path.trim().into());
        }
        if widgets::push_button(ui, "Browse...").clicked() {
            if let Some(p) = rfd::FileDialog::new().set_title("Overlay pack library folder").pick_folder() {
                d.overlay_library = Some(p);
            }
        }
    });
    if default_all(ui) {
        (d.show_welcome, d.show_splash, d.recent_max, d.auto_update, d.nested_breadcrumb) = (true, true, 4, true, false);
        (d.autosave_on, d.autosave_minutes, d.autosave_keep) = (true, 2, 5);
        (d.overlay_library, d.open_last, d.temp_folder) = (None, false, None);
    }
}

/// R44/R46: Milan Pro's own page: the updater's settings and the log and crash-report options.
fn update(ui: &mut Ui, d: &mut Prefs) {
    ui.label("Updates:");
    super::field_box(ui, 4, |ui| {
        use crate::updater::Channel;
        ui.set_width(ui.available_width());
        widgets::checkbox(ui, &mut d.auto_update, "Check for updates automatically at startup");
        ui.horizontal(|ui| {
            ui.label("Update channel:");
            widgets::radio_value(ui, &mut d.update_channel, Channel::Stable, "Stable");
            widgets::radio_value(ui, &mut d.update_channel, Channel::Preview, "Preview (pre-releases too)");
        });
        ui.horizontal(|ui| {
            match &d.skipped_update {
                Some(v) => ui.label(format!("Skipped version: {v}")),
                None => ui.label("Skipped version: none"),
            };
            if widgets::push_button_enabled(ui, d.skipped_update.is_some(), "Clear").clicked() {
                d.skipped_update = None;
            }
        });
        widgets::checkbox(ui, &mut d.keep_previous, "Keep the previous version's installer (Help › Roll Back to Previous Version)");
        ui.label(RichText::new(format!("Kept in {}", crate::updater::previous_dir().map(|p| p.display().to_string()).unwrap_or_default())).small());
    });
    ui.add_space(8.0);
    ui.label("Logging and crash reports:");
    super::field_box(ui, 4, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label("Log level:");
            let current = d.log_level.clone();
            widgets::combo(ui, "log_level", 100.0, current.as_str(), |ui| {
                for l in crate::logger::LEVEL_NAMES {
                    if ui.selectable_label(d.log_level == l, l).clicked() {
                        d.log_level = l.into();
                    }
                }
            });
        });
        widgets::checkbox(ui, &mut d.crash_privacy, "Crash reports and diagnostic reports leave out file names and paths");
        ui.horizontal(|ui| {
            if widgets::push_button(ui, "Open Log Folder").clicked() {
                if let Some(dir) = crate::logger::logs_dir() {
                    let _ = std::fs::create_dir_all(&dir);
                    super::open_file(&dir);
                }
            }
            ui.label(crate::logger::logs_dir().map(|p| p.display().to_string()).unwrap_or_default());
        });
        ui.label("A crash writes crash-<time>.txt (and a minidump) next to the logs; the next start offers to open it or report it on GitHub. Nothing is sent by itself.");
    });
    if default_all(ui) {
        (d.auto_update, d.log_level, d.crash_privacy) = (true, "info".into(), false);
        (d.update_channel, d.keep_previous) = (Default::default(), false);
    }
}

/// Audio (R49, VEGAS's Audio tab): Normalize peak level, R18a's preferred audio editor (Tools › Open in Audio Editor) and
/// R16c's loudness log option; the switches Milan Pro has no counterpart for say why.
fn audio(ui: &mut Ui, d: &mut Prefs) {
    super::row(ui, "Normalize peak level (dB):", 230.0, true, |ui| {
        widgets::spin(ui, &mut d.normalize_peak_db, 0.1, |s| s.range(-20.0..=0.0).speed(0.05).fixed_decimals(2))
            .on_hover_text("The peak Switches › Normalize brings an event to (measured again when changed)")
    });
    super::row(ui, "Quick fade length for audio events (ms):", 230.0, false, |ui| {
        super::fixed_combo(ui, "pa_qf", "10", 80.0, Fixed("the fade of Options > Quickfade Audio Edits is 10 ms, in playback and render alike"));
    });
    super::row(ui, "Dither:", 230.0, false, |ui| {
        super::fixed_combo(ui, "pa_dither", "None", 120.0, Fixed("renders at 8, 16 and 24 bits round to the nearest step; no dither is added"));
    });
    widgets::checkbox_enabled(ui, false, &mut true, "Import audio at project sample rate")
        .on_disabled_hover_text(Fixed("every file plays at the project rate through Milan Pro's resampler; no converted copy is made").text());
    widgets::checkbox_enabled(ui, false, &mut true, "Use default metronome sounds").on_disabled_hover_text(Fixed("the clicks of Options > Metronome are Milan Pro's own sounds").text());
    ui.add_space(8.0);
    ui.label("Preferred audio editor:");
    ui.horizontal(|ui| {
        let mut path = d.audio_editor.as_ref().map_or(String::new(), |p| p.display().to_string());
        if widgets::edit(ui, TextEdit::singleline(&mut path).desired_width(320.0)).changed() {
            d.audio_editor = (!path.trim().is_empty()).then(|| path.trim().into());
        }
        if widgets::push_button(ui, "Browse...").clicked() {
            if let Some(p) = rfd::FileDialog::new().add_filter("Programs", &["exe"]).pick_file() {
                d.audio_editor = Some(p);
            }
        }
    });
    ui.add_space(8.0);
    widgets::checkbox(ui, &mut d.loudness.open_editor, "Open editor when new loudness log is generated");
    if default_all(ui) {
        (d.normalize_peak_db, d.audio_editor, d.loudness.open_editor) = (crate::edit::NORMALIZE_PEAK_DB, None, true);
    }
}

/// R49: File I/O (VEGAS 23): VEGAS's legacy and compound file readers, which FFmpeg's single reader replaces.
fn file_io(ui: &mut Ui) {
    let why = Fixed("every format is read by FFmpeg itself; VEGAS's legacy AVC/HEVC readers and the so4 compound reader have no counterpart");
    for label in ["Enable legacy AVC decoding", "Enable legacy HEVC decoding", "Enable so4 compound reader for AVC/M2TS"] {
        widgets::checkbox_enabled(ui, false, &mut false, label).on_disabled_hover_text(why.text());
    }
    ui.add_space(8.0);
    ui.label("Hardware decoding and the encode device are on the Video page; proxies on the Video page and in Project Media.");
}

/// R49: Deprecated Features (VEGAS 23): switches for plug-in kinds VEGAS keeps for old projects.
fn deprecated(ui: &mut Ui) {
    let qt = Fixed("files in QuickTime formats are read by FFmpeg; no QuickTime plug-in is needed");
    widgets::checkbox_enabled(ui, false, &mut false, "Enable QuickTime plug-in").on_disabled_hover_text(qt.text());
    let dx = Fixed("only VST3 and CLAP audio effects and OFX video effects are hosted, not DirectX plug-ins");
    widgets::checkbox_enabled(ui, false, &mut false, "Enable DirectX plug-in hosting").on_disabled_hover_text(dx.text());
    ui.add_space(8.0);
    ui.label("Milan Pro has no deprecated features: its readers and plug-in hosts are current.");
}

fn video(ui: &mut Ui, d: &mut Prefs) {
    // VEGAS creates proxies for 4K video automatically; Milan Pro makes that opt-in (off by default, per the plan),
    // besides Project Media › Create Video Proxy.
    widgets::checkbox(ui, &mut d.auto_proxy, "Automatically create video proxies for 4K, HEVC and AV1 media on import");
    // Milan Pro's own, off by default: compositing modes VEGAS does not have.
    widgets::checkbox(ui, &mut d.extra_blend_modes, "Offer extra blend modes (Soft Light, Exclusion, Hard Mix, Hue, Saturation, Color, Luminosity)")
        .on_hover_text("Not VEGAS modes: a project using them looks different in VEGAS");
    // R21a: VEGAS's "GPU acceleration of video processing": Off or an adapter (Milan Pro adds Automatic, the default).
    use crate::engine::gpu;
    ui.add_space(6.0);
    let adapters = gpu::adapters();
    if adapters.is_none() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(250)); // the list arrives from a background thread
    }
    let auto = "Automatic (the best adapter)";
    let shown = match d.gpu.as_str() {
        "" => auto.to_string(),
        n if n == gpu::OFF || adapters.as_ref().is_none_or(|a| a.iter().any(|x| x == n)) => n.to_string(),
        n => format!("{n} (not found)"),
    };
    super::row(ui, "GPU acceleration of video processing:", 230.0, true, |ui| {
        ComboBox::from_id_salt("pv_gpu").width(180.0).selected_text(shown).show_ui(ui, |ui| {
            ui.selectable_value(&mut d.gpu, gpu::OFF.to_string(), gpu::OFF);
            ui.selectable_value(&mut d.gpu, String::new(), auto);
            for n in adapters.iter().flatten() {
                ui.selectable_value(&mut d.gpu, n.clone(), n.as_str());
            }
        })
        .response
        .on_hover_text("Compositing on the graphics card (Vulkan). Without one, or Off, the CPU composites; the picture is the same either way.")
    });
    ui.label(gpu::status());
    // R22: hardware decoding and the encode device, from the capability table (`engine::hw`).
    use crate::engine::hw;
    ui.add_space(6.0);
    let table = hw::table();
    if table.is_none() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(500)); // the probe runs on a background thread
    }
    let auto_dec = "Automatic (heavy sources, when faster)";
    let shown = match d.hw_decode.as_str() {
        "" => auto_dec.to_string(),
        n => n.to_string(),
    };
    super::row(ui, "Video decode acceleration:", 230.0, true, |ui| {
        ComboBox::from_id_salt("pv_hwdec").width(180.0).selected_text(shown).show_ui(ui, |ui| {
            ui.selectable_value(&mut d.hw_decode, hw::OFF.to_string(), hw::OFF);
            ui.selectable_value(&mut d.hw_decode, String::new(), auto_dec);
            for h in hw::HWACCELS {
                let passed = table.as_ref().is_none_or(|t| t.decode.iter().any(|x| x.ok && x.hwaccel == h));
                ui.add_enabled(passed, SelectableLabel::new(d.hw_decode == h, h)).on_disabled_hover_text("Decoded nothing on this computer").clicked().then(|| d.hw_decode = h.to_string());
            }
        })
        .response
        .on_hover_text("Which FFmpeg hardware decoder path HEVC, AV1 and 4K sources decode through during playback. Automatic takes the one that measured faster than software, source by source.")
    });
    let auto_enc = "Automatic (the first that passed)";
    let shown = match d.hw_encode.as_str() {
        "" => auto_enc.to_string(),
        n => hw::device_label(n).to_string(),
    };
    super::row(ui, "Encode device:", 230.0, true, |ui| {
        ComboBox::from_id_salt("pv_hwenc").width(180.0).selected_text(shown).show_ui(ui, |ui| {
            ui.selectable_value(&mut d.hw_encode, hw::OFF.to_string(), hw::OFF);
            ui.selectable_value(&mut d.hw_encode, String::new(), auto_enc);
            for (dev, label) in hw::DEVICES {
                let passed = table.as_ref().is_none_or(|t| t.encode.iter().any(|x| x.ok && x.encoder.ends_with(&format!("_{dev}"))));
                ui.add_enabled(passed, SelectableLabel::new(d.hw_encode == dev, label)).on_disabled_hover_text("No encoder of this device passed on this computer").clicked().then(|| d.hw_encode = dev.to_string());
            }
        })
        .response
        .on_hover_text("The graphics card encoder the hardware render templates use (Render As > MP4 H.264 (hardware)...). A template whose encoder did not pass renders in software and says so.")
    });
    ui.horizontal(|ui| {
        ui.label("Hardware acceleration test:");
        if widgets::push_button_enabled(ui, table.is_some(), "Test Again").on_hover_text("Measure every decoder path and encoder again (about 10 s)").clicked() {
            hw::start(true, true);
        }
    });
    super::field_box(ui, 4, |ui| {
        ui.set_width(ui.available_width());
        ui.set_height(7.0 * 16.0);
        ui.spacing_mut().item_spacing.y = 0.0;
        for line in hw::summary() {
            ui.label(RichText::new(line).size(11.0));
        }
    });
    // R23: the caches.
    ui.add_space(6.0);
    super::row(ui, "Dynamic RAM Preview max (MB):", 230.0, true, |ui| {
        widgets::spin(ui, &mut d.ram_preview_mb, 16.0, |s| s.range(16..=65536)).on_hover_text("How much memory Build Dynamic RAM Preview (Shift+B) may fill with frames of the loop region")
    });
    super::row(ui, "Disk cache for decoded HEVC, AV1 and 4K frames (MB):", 230.0, true, |ui| {
        widgets::spin(ui, &mut d.disk_cache_mb, 256.0, |s| s.range(0..=1_048_576)).on_hover_text("Milan Pro's own (not in VEGAS), off at 0: decoded frames of heavy sources are kept on disk, so seeking back to them needs no decode");
        if widgets::push_button(ui, "Clear").on_hover_text("Delete the cached frames").clicked() {
            crate::engine::diskcache::clear();
        }
    });
    ui.label("Prerendered files folder:");
    ui.horizontal(|ui| {
        let mut path = d.prerender_folder.clone().unwrap_or_else(crate::engine::prerender::default_folder).display().to_string();
        if widgets::edit(ui, TextEdit::singleline(&mut path).desired_width(300.0)).on_hover_text("Selectively Prerender Video writes each project's files to a folder of its own in here").changed() {
            d.prerender_folder = (!path.trim().is_empty()).then(|| path.trim().into());
        }
        if widgets::push_button(ui, "Browse...").clicked() {
            if let Some(p) = rfd::FileDialog::new().set_title("Prerendered files folder").pick_folder() {
                d.prerender_folder = Some(p);
            }
        }
    });
    if default_all(ui) {
        (d.auto_proxy, d.extra_blend_modes, d.gpu) = (false, false, String::new());
        (d.hw_decode, d.hw_encode) = (String::new(), String::new());
        (d.ram_preview_mb, d.disk_cache_mb, d.prerender_folder) = (200, 0, None);
    }
}

/// R14: Preview Device, VEGAS's Windows Graphics Card device (a secondary display): the display Preview on External
/// Monitor uses and how the picture goes on it. VEGAS's other devices (IEEE 1394, SDI cards) are not planned.
fn preview_device(ui: &mut Ui, d: &mut Prefs, monitors: &[crate::ui::monitors::Monitor]) {
    const W: f32 = 230.0;
    super::row(ui, "Device:", 110.0, true, |ui| {
        widgets::combo(ui, "ppd_device", W, "Windows Graphics Card", |ui| {
            let _ = ui.selectable_label(true, "Windows Graphics Card");
            ui.add_enabled(false, SelectableLabel::new(false, "IEEE 1394/OHCI Compatible Device")).on_disabled_hover_text(NotPlanned("R59").text());
        });
    });
    ui.add_space(6.0);
    super::group(ui, "Details");
    let auto = "Automatic (the first secondary display)";
    let shown = d.ext_display.as_deref().map_or(auto.to_string(), |n| monitors.iter().find(|m| m.name == n).map_or(format!("{n} (not connected)"), |m| m.label()));
    super::row(ui, "Display adapter:", 110.0, true, |ui| {
        widgets::combo(ui, "ppd_display", W, shown, |ui| {
            ui.selectable_value(&mut d.ext_display, None, auto);
            for m in monitors {
                ui.selectable_value(&mut d.ext_display, Some(m.name.clone()), m.label());
            }
        });
    });
    if monitors.is_empty() {
        ui.label("No displays were found.");
    }
    // R49: each display shows its number for a few seconds (as VEGAS's Identify Displays).
    if widgets::push_button(ui, "Identify Displays").on_hover_text("Show each display's number on it").clicked() {
        crate::ui::monitors::identify(ui.ctx(), monitors);
    }
    ui.add_space(6.0);
    widgets::checkbox(ui, &mut d.ext_fit, "Scale output to fit display").on_hover_text("Off: one project pixel per display pixel, centred");
    widgets::checkbox_enabled(ui, false, &mut false, "Apply deinterlace filter")
        .on_disabled_hover_text(Fixed("frames are progressive already: interlaced sources are deinterlaced as they are decoded (Project Properties > Deinterlace method)").text());
    widgets::checkbox_enabled(ui, false, &mut false, "Adjust levels from studio RGB to computer RGB")
        .on_disabled_hover_text(Fixed("the timeline is computer RGB (0-255) end to end; sources are expanded from studio levels as they are decoded").text());
    widgets::checkbox_enabled(ui, false, &mut true, "Wait for vertical sync").on_disabled_hover_text(ALWAYS_ON.text());
    widgets::checkbox_enabled(ui, false, &mut true, "Optimize GPU display performance").on_disabled_hover_text(ALWAYS_ON.text());
    widgets::checkbox_enabled(ui, false, &mut false, "Recompress edited frames").on_disabled_hover_text(NotPlanned("R59").text());
    widgets::checkbox(ui, &mut d.ext_in_window, "Display frames in Video Preview window during playback");
    widgets::checkbox(ui, &mut d.ext_rotate, "Use project output rotation setting");
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
        widgets::combo(ui, "pad_type", COMBO_W, d.device_type.name(), |ui| {
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
            ui.add_enabled_ui(false, |ui| widgets::combo(ui, "pad_out", COMBO_W, "Microsoft Sound Mapper", |_| {}).response)
                .inner
                .on_disabled_hover_text(Fixed("the Microsoft Sound Mapper plays on the Windows default device").text());
            return;
        }
        let shown = d.playback_device.clone().unwrap_or_else(|| DEFAULT.into());
        widgets::combo(ui, "pad_out", COMBO_W, shown, |ui| {
            ui.selectable_value(&mut d.playback_device, None, DEFAULT);
            for n in outputs {
                ui.selectable_value(&mut d.playback_device, Some(n.clone()), n.as_str());
            }
        });
    });
    // R10a: Milan Pro monitors a surround project as its stereo fold-down on the main device (the render has the channels).
    const FOLD: Why = Fixed("surround projects are monitored as a stereo fold-down on the stereo and front device; separate rear and center/LFE devices are not offered");
    super::row(ui, "Default rear playback device:", LABEL_W, false, |ui| super::fixed_combo(ui, "pad_rear", "None", COMBO_W, FOLD));
    super::row(ui, "Default center and LFE playback device:", LABEL_W, false, |ui| super::fixed_combo(ui, "pad_lfe", "None", COMBO_W, FOLD));
    super::row(ui, "Buffer size:", LABEL_W, true, |ui| {
        widgets::combo(ui, "pad_buf", COMBO_W, buffer_text(d.buffer_frames), |ui| {
                for n in BUFFERS {
                    ui.selectable_value(&mut d.buffer_frames, n, buffer_text(n));
                }
            })
            .response
            .on_hover_text("Smaller buffers lower the latency of playback and input monitoring; too small for this computer, and the sound breaks up");
    });
    super::row(ui, "Default audio recording device:", LABEL_W, true, |ui| {
        let shown = d.record_input.clone().unwrap_or_else(|| DEFAULT.into());
        widgets::combo(ui, "pad_rec", COMBO_W, shown, |ui| {
            ui.selectable_value(&mut d.record_input, None, DEFAULT);
            for n in inputs {
                ui.selectable_value(&mut d.record_input, Some(n.clone()), n.as_str());
            }
        });
    });
    super::row(ui, "Recording latency offset (ms):", LABEL_W, true, |ui| {
        widgets::spin(ui, &mut d.record_offset_ms, 0.5, |d| d.range(-1000.0..=1000.0).speed(0.5).fixed_decimals(1))
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

/// R34a: the VST3 and CLAP folders searched (the standard ones, read-only, and the user's), as VEGAS's VST Effects page.
fn vst_effects(ui: &mut Ui, d: &mut Prefs) {
    ui.label("Milan Pro searches these folders for VST3 and CLAP audio effects (and the OFX folders of the Plug-In Manager):");
    ui.add_space(4.0);
    super::field_box(ui, 4, |ui| {
        ui.set_width(ui.available_width());
        ui.set_height(8.0 * 24.0);
        let standard = crate::engine::ofx::scan::audio_folders(&crate::engine::ofx::OfxPrefs::default());
        for f in standard {
            ui.horizontal(|ui| {
                ui.add_space(4.0);
                ui.label(RichText::new(f.display().to_string()).color(theme::get().text_dim)).on_hover_text("A standard folder, always searched");
            });
        }
        let mut remove = None;
        for (i, f) in d.ofx.vst_folders.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.add_space(4.0);
                ui.label(f.display().to_string());
                if widgets::small_push(ui, "×").on_hover_text("Stop searching this folder").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            d.ofx.vst_folders.remove(i);
        }
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if widgets::push_button(ui, "Add Folder...").clicked() {
            if let Some(f) = rfd::FileDialog::new().set_title("Add a VST3 or CLAP plug-in folder").pick_folder() {
                if !d.ofx.vst_folders.contains(&f) {
                    d.ofx.vst_folders.push(f);
                }
            }
        }
        ui.label(RichText::new("Plug-ins are listed and rescanned in View › Window › Plug-In Manager.").color(theme::get().text_dim));
    });
    ui.add_space(8.0);
    let (a, ok) = {
        let e = crate::engine::aplug::entries();
        (e.len(), e.iter().filter(|x| x.status == crate::engine::ofx::Status::Ok).count())
    };
    ui.label(format!("{a} VST3/CLAP effects found, {ok} OK. VST2 plug-ins are not supported (no licence to host them)."));
}

fn editing(ui: &mut Ui, d: &mut Prefs) {
    ui.horizontal(|ui| {
        ui.label("New still image length (seconds):");
        widgets::spin(ui, &mut d.still_len, 0.1, |d| d.range(0.1..=3600.0).speed(0.1).fixed_decimals(3));
    });
    ui.horizontal(|ui| {
        ui.label("Cut-to-overlap conversion (seconds):").on_hover_text("The crossfade a transition dropped on a cut makes, centred on the cut");
        widgets::spin(ui, &mut d.cut_overlap, 0.05, |d| d.range(0.0..=60.0).speed(0.05).fixed_decimals(3));
    });
    ui.horizontal(|ui| {
        ui.label("Cursor preview duration (seconds):").on_hover_text("What Preview at Cursor (Num 0, Ctrl+Shift+Space) plays, centred on the cursor");
        widgets::spin(ui, &mut d.cursor_preview, 0.1, |d| d.range(0.1..=60.0).speed(0.1).fixed_decimals(3));
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

fn display(ui: &mut Ui, d: &mut Prefs, track: &mut usize, (env_k, snap_k): (&mut usize, &mut usize)) {
    let t = theme::get();
    // The default palette for this preview, whatever the current override.
    let base = theme::Theme::new().track_colors;
    let mut palette = d.palette().unwrap_or(base);

    super::group(ui, "Track Colors");
    let changed = super::row(ui, "      Track:", 120.0, true, |ui| {
        widgets::combo(ui, "pref_track", 200.0, COLOR_NAMES[*track], |ui| {
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
    // R49: the colour of each envelope and snap type (the defaults are the look's; a change shows on OK / Apply).
    let mut base = theme::Theme::new();
    let _ = t;
    super::group(ui, "Envelope Colors");
    super::row(ui, "      Envelope Type:", 120.0, true, |ui| {
        widgets::combo(ui, "pref_env", 200.0, theme::ENVELOPE_TYPES[*env_k], |ui| {
            for (k, n) in theme::ENVELOPE_TYPES.iter().enumerate() {
                ui.selectable_value(env_k, k, *n);
            }
        });
        let name = theme::ENVELOPE_TYPES[*env_k];
        user_color(ui, &mut d.envelope_colors, name, base.envelope_slot(name).copied().unwrap_or_default());
    });
    super::group(ui, "Snap Colors");
    super::row(ui, "      Snap Type:", 120.0, true, |ui| {
        widgets::combo(ui, "pref_snap", 200.0, theme::SNAP_TYPES[*snap_k], |ui| {
            for (k, n) in theme::SNAP_TYPES.iter().enumerate() {
                ui.selectable_value(snap_k, k, *n);
            }
        });
        let name = theme::SNAP_TYPES[*snap_k];
        user_color(ui, &mut d.snap_colors, name, base.snap_slot(name).copied().unwrap_or_default());
    });
    ui.add_space(4.0);
    super::row(ui, "Icon color strength:", 120.0, true, |ui| {
        let mut v = f32::from(d.icon_strength);
        if widgets::slider(ui, &mut v, 0.0..=100.0, widgets::Slide { no_value: true, ..Default::default() }).changed() {
            d.icon_strength = v.round().clamp(0.0, 100.0) as u8;
        }
        ui.label(format!("{} %", d.icon_strength));
    });
    ui.add_space(4.0);
    // The docking options (§4.3).
    widgets::checkbox(ui, &mut d.auto_hide_docks, "Automatically hide docking area");
    widgets::checkbox(ui, &mut d.tabs_on_top, "Position tabs at top of docked windows");
    let mut float_dock = d.float_dock();
    if widgets::checkbox(ui, &mut float_dock, "Allow floating windows to dock (hold Ctrl to override)").changed() {
        d.float_dock = Some(float_dock);
    }
    // R48: Milan Pro's own (also View › Interface Scale); Auto follows Windows, as VEGAS does.
    ui.add_space(4.0);
    super::row(ui, "Interface scale:", 120.0, true, |ui| {
        let name = |s: Option<u16>| s.map_or("Auto (Windows setting)".to_string(), |p| format!("{p} %"));
        widgets::combo(ui, "pref_scale", 200.0, name(d.ui_scale), |ui| {
            ui.selectable_value(&mut d.ui_scale, None, name(None));
            for p in theme::SCALES {
                ui.selectable_value(&mut d.ui_scale, Some(p), name(Some(p)));
            }
        })
        .response
        .on_hover_text("The size of text, icons and bevels. Whole-pixel edges stay sharp at every scale.");
    });
    if default_all(ui) {
        d.track_colors = None;
        (d.auto_hide_docks, d.tabs_on_top, d.float_dock, d.ui_scale) = (false, false, None, None);
        (d.envelope_colors, d.snap_colors, d.icon_strength) = (Default::default(), Default::default(), 75);
    }
}

/// A Display page colour button: `name`'s colour in `map` (the user's), else `default`; back to the default drops it.
fn user_color(ui: &mut Ui, map: &mut std::collections::BTreeMap<String, [u8; 3]>, name: &str, default: Color32) {
    ui.spacing_mut().interact_size = vec2(60.0, 23.0);
    let mut c = map.get(name).map_or(default, |[r, g, b]| Color32::from_rgb(*r, *g, *b));
    if egui::color_picker::color_edit_button_srgba(ui, &mut c, egui::color_picker::Alpha::Opaque).changed() {
        if c == default {
            map.remove(name);
        } else {
            map.insert(name.to_string(), [c.r(), c.g(), c.b()]);
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    /// Another window saves its last layout while the dialog is open, then OK: the dialog hands that layout back (it
    /// does not undo it) and Apply goes off again.
    #[test]
    fn ok_after_apply_keeps_the_layouts_saved_meanwhile() {
        let mut dlg = PrefsDialog::default();
        let mut current = Prefs::default();
        dlg.open(&current);
        let l = crate::ui::dock::Dock::new(&theme::Theme::new()).layout("");
        current.layouts.insert(crate::settings::layout_key("last"), l);
        let ctx = Context::default();
        let _ = ctx.run(RawInput::default(), |ctx| assert!(dlg.show(ctx, &current, None).is_none()));
        assert_eq!(dlg.draft.as_ref(), Some(&current));
    }
}
