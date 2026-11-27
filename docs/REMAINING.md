# Milan Pro: what is left before release

Status: gap analysis of `main` at 44c268a (v0.4.0 plus two bug-fix commits), 2026-11-12.
Source of the requirements: the original project brief (sections 1 to 18, the deliverables list and the priority order).
The user's instruction: **do not release until every item below is DONE, or is NOT FEASIBLE with its honest deliverable shipped.**

How this was checked:
- Every bullet of the brief was looked up in `src/`, `.github/`, `installer/`, `tools/` and `docs/` (grep and reading, no build).
- The installed reference application was inventoried read-only: `C:\Program Files\VEGAS\VEGAS Pro 15.0` and `C:\Program Files (x86)\VEGAS\Shared Plug-Ins` (section 4).
- The bundled FFmpeg (`installer/ffmpeg/ffmpeg.exe`, 9.0.2 gyan full build) was queried with `-encoders`, `-hwaccels` and `-filters`.
- `cargo tree` was used for dependency facts.

Status words:
- **DONE**: works and is wired into the UI. Evidence is a file.
- **PARTIAL**: some of it works. The cell says what is missing.
- **MISSING**: nothing is there, or only a greyed-out menu row.
- **NOT FEASIBLE**: cannot be built as stated, for a reason given. The cell names the honest deliverable that replaces it, and that deliverable is scheduled in a batch.

Sizes: **S** under ~300 changed lines, **M** 300-1000, **L** 1000-2500. Anything larger is split, so every batch fits one agent in one session.

Contents:
1. Summary
2. Requirement table
3. Disabled and placeholder controls
4. Reference inventory (VEGAS Pro 15.0 install and bundled FFmpeg)
5. Implementation batches R0-R60
6. Not feasible items and their deliverables
7. Release gate

---

## 1. Summary

Counts cover the rows of section 2 (roll-up rows in 2.19 are not counted).

| Status | Rows |
|---|---|
| DONE | 366 |
| PARTIAL | 40 |
| MISSING | 13 |
| NOT FEASIBLE | 5 (plus 4 partial infeasibilities inside other rows; section 6 lists all 9) |
| **Total** | **424** |

What matters most:
- **Editing core works.** Snap to grid, the 15 ruler formats and an exact 30000/1001 timebase are in (R1); the navigation, selection, nudge, edge-trimming and zoom commands are in (R2). Event looping, empty events, subclips, attribute paste and the group commands are in (R3a). The event switches (Invert Phase, Normalize, Maintain Aspect Ratio, Reduce Interlace Flicker, resampling with frame blend and Optical Flow, Undersample rate), Channels, Stream and Quickfade Audio Edits are in (R3b). Video event velocity, freeze frame, Transition Progress, transitions on a lone fade-out, and video track Mute and Fade to Color envelopes are in (R4a). The audio of an A/V pair can follow its velocity (an option, off as in VEGAS), J / L shuttle and the Rate slider are heard, and audio events take a pitch change (R4b). Track groups (collapsible, with group Mute / Solo) and Hide are in (R5). Masks, all of VEGAS's compositing modes, parent/child compositing, the Compositors window, Exposure, spill suppression, 2D Shadow / Glow, 3D Track Motion and the Video Bus Track with motion blur are in (R7). Audio busses (0-26), assignable FX chains, track Output, pre/post sends with the multipurpose slider, the six Pan Types and per-track/bus meter data are in (R8a). The Mixing Console, live track/bus/master meters (peak, and VU as an option), Audio Bus Tracks with their envelope lanes, Dim and Downmix are in (R8b). Automation Settings (Off / Read / Write (Touch) / Write (Latch), Show Automation Controls), FX automation envelopes on audio tracks and busses (and video track FX keys as envelopes, VP23), audio event Volume / Pan / Mute envelopes and audio FX presets are in (R8c). Playback runs on cpal (or WASAPI exclusive) with a choice of device and buffer size, the playhead follows what is heard, and armed tracks and input busses monitor their input live (R9). A `.mnle` project is media, a nested timeline played and rendered through the engine, with a cycle guard (R6a); Create Nested Timeline, Open Nested / Open Parent Timeline and Edit Source Project work in the window, and nested events have thumbnails, waveforms and a Trimmer view made by the engine (R6b). Events line up by their sound (refusing weak matches), media timecode or date/time stamp, and Project Media lays out tracks by either (R17a). Multicamera editing works: Create Multicamera Track, the take grid in the Video Preview with takes chosen by click or number key (also while playing, each one undoable), Expand to Multiple Tracks, Edit Multicamera Audio with Video and Show Full Frame in External Monitor (R17b). Smart Split, command markers, Audio CD track regions and indexes (on their own bars, and in Edit Details), Open (Copy) in Audio Editor with reload on save, Rebuild Audio Peaks, event media markers, View › Event Length, the Metronome, the Credit Roll and Test Pattern generators and Shift+F10 are in (R18a). Adjustment Tracks and Adjustment Events, Insert Slideshow, Insert Time, Insert Audio Transition, Cut / Copy over a time selection and Paste Insert's split, Resize Generated Media and video track Set Default Track Properties are in (R18b), which finishes tier 2: no control waits on a batch of tiers 0-2 any more (test `no_control_waits_on_tiers_0_to_2`). What is still greyed belongs to later batches or the R57-R60 long tail.
- **Preview is complete to the plan.** Scrubbing follows the cursor, seeks are frame-exact and reversed events show every frame (R20); the compositor runs on the GPU (R21a-c); hardware decoding and encoding are tested at startup and used where they pass and measure faster (R22); Build Dynamic RAM Preview, Selectively Prerender Video with Clean Up, and a disk cache for decoded frames of heavy sources are in (R23). Adjust Size and Quality for Optimal Playback, the measured frame rate and playback statistics, and the benchmark report `docs/PERFORMANCE.md` (written by `tools/bench.ps1`) are in (R19).
- **Render has templates (R11).** 38 built-in delivery templates plus user templates, Customize Template (size, rate, field order, pixel aspect ratio, codec, rate control, pixel format, audio), letterbox or stretch, output rotation, chapters from markers, and a log per render; every built-in is rendered and checked with ffprobe. A render queue renders a snapshot of the project taken when each render is queued, so editing goes on (Render in the background, off as in VEGAS); Batch Render, Render Audio Tracks and Render to New Track are in, and Exit or an update asks before stopping a render (R12). Hardware encoders (NVENC, Quick Sync, AMF, Media Foundation) are tested at startup and offered when they pass (R22).
- **Interchange is VEGAS-script import only.** OTIO, EDL, FCP XML, interchange report and a reverse path into VEGAS are all missing.
- **Plug-ins:** there is no external hosting. The installed VEGAS has 7 OFX bundles (102 effects; 7 of them third-party HitFilm) and 11 DirectX audio plug-in DLLs. The VST3 folder is empty.
- **Reliability gaps (closed by R42-R44):** schema version and migration, `.mnle.bak` on save, crash recovery, configurable autosave, a file logger and crash reports, the diagnostic report.
- **Documentation (R52-R54, R56):** [REFERENCE_ANALYSIS.md](REFERENCE_ANALYSIS.md), [ARCHITECTURE.md](ARCHITECTURE.md), [DEVELOPING.md](DEVELOPING.md), [LICENSES.md](LICENSES.md), `CONTRIBUTING.md`, the Code of Conduct, issue forms and the changelog are written. Still open: the compatibility matrix (R55) and the format report (R37).
- **The no-silent-placeholder rule holds (R0, DONE).** Every greyed menu row, toggle, combo, checkbox and inert toolbar button says on hover which batch builds it, or "not planned" for the R57-R60 long tail; Help > Unfinished Features lists them all, and two unit tests keep it that way.

---

## 2. Requirement table

### 2.1 Section 1: inspect the VEGAS installation

| # | Requirement | Status | Evidence / what is missing |
|---|---|---|---|
| 1.1 | Locate VEGAS, exact version and components | DONE | (R52) [`docs/REFERENCE_ANALYSIS.md`](REFERENCE_ANALYSIS.md) section 1: VEGAS Pro 15.0 build 177, the program folder and the shared plug-in folder, component by component. |
| 1.2 | Study interface, layout, menus, toolbars, timeline, preview, media, audio, FX, transitions, render | DONE | `docs/VEGAS_UI_SPEC.md` (2522 lines; measured [L] from the VP15 install plus VP16/VP23 references) |
| 1.3 | Examine settings and supported project, media and plug-in formats | DONE | (R52) [`docs/REFERENCE_ANALYSIS.md`](REFERENCE_ANALYSIS.md) sections 3 (project files), 4 (media formats per FileIO plug-in), 5 (render formats) and 9 (settings pages). |
| 1.4 | Identify installed video and audio plug-ins and their formats | DONE | (R52, R31) [`docs/REFERENCE_ANALYSIS.md`](REFERENCE_ANALYSIS.md) section 6: the 7 OFX bundles (HitFilm Vegas Basic is FXHOME's, third party), the DirectX audio plug-ins, the legacy video plug-ins; `FEATURE_PLAN.md` corrected. |
| 1.5 | Research integration points, SDKs, plug-in standards, interop | DONE | (R52) [`docs/REFERENCE_ANALYSIS.md`](REFERENCE_ANALYSIS.md) sections 6-8: OFX with VEGAS's extensions, VST through `sfvstwrap`/`sfvstserver.exe`, DirectX audio, the .NET script API, the FileIO SDK, external control drivers, AAF, EDL, XML. |
| 1.6 | Detailed reference document | DONE | (R52) [`docs/REFERENCE_ANALYSIS.md`](REFERENCE_ANALYSIS.md), with the UI spec for the interface. |
| 1.7 | `REFERENCE_ANALYSIS.md` | DONE | (R52) [`docs/REFERENCE_ANALYSIS.md`](REFERENCE_ANALYSIS.md) |
| 1.8 | Observe the installed app's real behaviour | PARTIAL | The UI spec measured the VP15 exe resources and pixels. `FEATURE_PLAN.md:1817-1827` lists 10 behaviours still to confirm in VEGAS. → R55 |

### 2.2 Section 2: repository

| # | Requirement | Status | Evidence / what is missing |
|---|---|---|---|
| 2.1 | Public GitHub repository | DONE | knivesbatz/milan-pro; `src/updater.rs:17` |
| 2.2 | Distinctive name, checked | DONE | "Milan Pro". The name check is not recorded in docs (R53 notes it). |
| 2.3 | Professional README | DONE | (R53) `README.md`: a harness screenshot, a feature status table for 0.8.0, system requirements, every document linked, the roadmap. |
| 2.4 | Project overview and feature roadmap | DONE | (R53) README "Roadmap" lists the open batches and links section 5 of this file, which is the roadmap. |
| 2.5 | Installation instructions | DONE | `README.md` "Install (Windows)" |
| 2.6 | Developer documentation | DONE | (R53) [`docs/DEVELOPING.md`](DEVELOPING.md): toolchain, FFmpeg, tests including `--ignored`, the harness variables, benches, licences, adding commands/dialogs/plug-ins, conventions, releases and versioning. |
| 2.7 | Architecture documentation | DONE | (R53) [`docs/ARCHITECTURE.md`](ARCHITECTURE.md): modules, threads, data flow, the plug-in host IPC, theming, reliability and file formats, the harness, the test strategy and decision records; `FEATURE_PLAN.md`'s header fixed. |
| 2.8 | Contribution guidelines | DONE | (R54) [`CONTRIBUTING.md`](../CONTRIBUTING.md): reporting, build, batch rules, `shortcut:` convention, tests, branches and commits, what may go into the repository, the AE-format "no Adobe SDK material" rule. |
| 2.9 | License | DONE | `LICENSE` (MIT) |
| 2.10 | Issue templates | DONE | (R54) `.github/ISSUE_TEMPLATE/`: `bug_report.yml`, `feature_request.yml`, `plugin_compat.yml`, `format_support.yml`, `config.yml`; `.github/PULL_REQUEST_TEMPLATE.md`. Rendering on GitHub to be checked by the user. |
| 2.11 | Bug-report template | DONE | (R54) `.github/ISSUE_TEMPLATE/bug_report.yml` (version, Windows build, GPU, steps, expected/actual, media, diagnostic report). |
| 2.12 | Feature-request template | DONE | (R54) `.github/ISSUE_TEMPLATE/feature_request.yml`. |
| 2.13 | GitHub Actions CI | DONE | `.github/workflows/ci.yml` (build plus test on windows-latest) |
| 2.14 | Automated build workflows | DONE | `ci.yml` uploads `milan-pro.exe` as an artifact |
| 2.15 | Release workflows | DONE | `.github/workflows/release.yml` (Inno Setup, FFmpeg fetch, SHA-256, draft-then-publish) |
| 2.16 | Version tagging | DONE | `release.yml` triggers on `v*` and checks that the tag matches `Cargo.toml` |
| 2.17 | Organized source tree | DONE | `src/{engine,ui,app}`, `installer/`, `tools/vegas/`, `docs/` |
| 2.18 | Permissive licence after a dependency licence check, third-party restrictions documented | DONE | (R56) [`docs/LICENSES.md`](LICENSES.md), generated by `tools/licenses.ps1` from `Cargo.lock` (474 crates, none copyleft-only, none unknown), with FFmpeg's GPL v3 position, codec patents, OpenFX, VST3, CLAP, ASIO, camera SDKs, fonts and Inno Setup; `THIRD-PARTY-NOTICES.txt` in the installer. |
| 2.19 | Keep the repository public | DONE | |
| 2.20 | Regular meaningful commits | DONE | `git log` shows one commit per batch |

### 2.3 Section 3: technology stack

| # | Requirement | Status | Evidence / what is missing |
|---|---|---|---|
| 3.1 | Rust for core functionality | DONE | 36k lines of Rust; FFmpeg only as child processes |
| 3.2 | FFmpeg for decode, encode, filters | DONE | `src/engine/vdecode.rs`, `src/engine/mix.rs`, `src/engine/render.rs`, `src/proc.rs` |
| 3.3 | GStreamer evaluated | DONE | (R53) [`docs/ARCHITECTURE.md`](ARCHITECTURE.md) 10.1: FFmpeg CLI against libav bindings and GStreamer. |
| 3.4 | GPU-accelerated rendering and compositing | DONE | (R21a-c) `engine/gpu`: the compositor runs on the GPU through wgpu on Vulkan (`GpuCompositor`, a `composite::Compositor` beside `CpuCompositor`), for the preview and renders alike; draws, every blend mode, Track Motion, the video FX, transitions and generators in WGSL, bit-exact against the CPU on 3000 random plans and at three parameter sets per plug-in (the clock wipe within 1). A preview frame stays on the GPU (`Picture`, shown as an egui native texture); a render reads back. Preferences › Video › GPU acceleration of video processing: Automatic, Off or an adapter. |
| 3.5 | Vulkan / Direct3D or another graphics API evaluated | DONE | (R21a) The UI paints through wgpu on Vulkan (eframe feature `wgpu`, the default; OpenGL where there is no Vulkan), and the compositor shares its device. Direct3D 12 was not taken: wgpu's dx12 backend would add gpu-allocator and its MIT/Apache tree, and VEGAS's own compositor runs on OpenCL/Vulkan-class hardware either way. Cargo feature `glow` keeps the OpenGL painter (the compositor then opens its own device). → R53 for the docs |
| 3.6 | Hardware video decoding and encoding | DONE | (R22) `engine/hw.rs` capability table (timed d3d11va/cuda/qsv decodes, nvenc/qsv/amf/mf encodes, cached per FFmpeg and driver); Preferences › Video decode acceleration and encode device; hardware Render As templates that fall back to software with a note. |
| 3.7 | Suitable GUI framework, evaluated | DONE | (R53) [`docs/ARCHITECTURE.md`](ARCHITECTURE.md) 10.2: egui against Qt, Slint and iced. |
| 3.8 | Professional audio engine | DONE | Streaming mixer (`engine/mix.rs`), (R8a) with busses, assignable FX chains and sends mixed in dependency order (`AudioPlan::buses`, `Mixer::block`) and per-node peak/RMS meters (`mix::Meter`); (R9) on cpal (WASAPI shared) or WASAPI exclusive with an output device choice, buffer size and latency-compensated playhead (`audio.rs`, `wasapi.rs`). R10a: multichannel: mono / stereo / 5.1 / 7.1 tracks and busses, a Stereo / 5.1 / 7.1 master (`AudioLayout`, `BusMode`, `mix::route`, `engine/surround.rs`); tests `a_5_1_source_plays_each_channel_to_its_position`, `surround_renders_match_ffmpeg`, `stereo_mix_is_bit_identical_to_the_pre_r10a_engine` |
| 3.9 | OpenFX and VST hosting where compatible | DONE | OFX video (R31-R33c), VST3 and CLAP audio effects (R34a-b, R35) in the out-of-process host; VST2 not feasible (10.11), DX/DMO investigated and not pursued (`docs/compat/dx-audio.md`). |
| 3.10 | Frameworks evaluated for performance, extensibility, maintainability | DONE | (R53) [`docs/ARCHITECTURE.md`](ARCHITECTURE.md) section 10: FFmpeg, GUI, glow/wgpu, rodio/cpal, JSON projects, out-of-process plug-ins. |

### 2.4 Section 4: interface

| # | Requirement | Status | Evidence / what is missing |
|---|---|---|---|
| 4.1 | Feels familiar to VEGAS users | DONE | The VEGAS 23 window layout, menus and keymap under Milan Pro's one look (`ui/theme.rs`, `ui/menus.rs`, `ui/keymap.rs`; `docs/UI_SPEC.md`) |
| 4.2 | Media / project explorer | DONE | `ui/explorer.rs`, `ui/media_panel.rs` |
| 4.3 | Preview monitor | DONE | `ui/preview.rs`, `decode.rs`, `engine/worker.rs` |
| 4.4 | Multitrack timeline | DONE | `ui/timeline/` |
| 4.5 | Track controls | DONE | `ui/timeline/tracks.rs` (mute, solo, lock, FX, level/vol/pan, arm) |
| 4.6 | Audio mixer | DONE | (R8b) Master Bus window (`ui/master_bus.rs`) with working Properties, Downmix, Dim and Mixing Console buttons; the Mixing Console window (`ui/mixer.rs`: strips for audio tracks, (R9) input busses, busses, assignable FX chains and the master, with Insert FX, Sends, I/O (input and output), VU Meters, Peak Meters and Faders regions); live meters in the track headers. |
| 4.7 | Effects and transitions browser | DONE | `ui/choosers.rs` (search, categories, presets, animated transition thumbnails) |
| 4.8 | Video effects controls | DONE | `ui/fx_window.rs`, `ui/color_grading.rs`, `ui/keyframes.rs` |
| 4.9 | Media properties | DONE | `ui/properties.rs` shows the basics; R45b: File Format Properties... lists every ffprobe format, stream, chapter and program field and tag in a tree, with Copy as Text (`ui/dialogs/file_props.rs`). |
| 4.10 | Transport controls | DONE | `ui/transport.rs` |
| 4.11 | Project settings | DONE | `settings.rs`: width, height, frame rate, sample rate, (R1) the whole Ruler page (`ruler_tab`: start time, format, tempo, beats per measure, beat note) and (R3b) Resample mode (`ProjectSettings.resample`, named as VEGAS 23 names it) are editable, and (R8a) Number of stereo busses (`audio_tab`, `edit::set_bus_count`). R11: Field order, Pixel aspect ratio (media keep their shape: `plan::media_ref`), Output rotation (the preview turns, `preview::rotated_image`), Full-resolution rendering quality, Deinterlace method (None / Blend fields / Interpolate fields, `Deinterlace::filter`), Adjust source media and audio Bit depth (PCM/FLAC/ALAC renders that follow the project) are editable; an old project keeps Best and yadif (`old_projects_load_without_fades_or_loop`). (R16a) The Summary page (Title, Artist, Engineer, Copyright, Comments: `settings.rs` `summary_tab`, `ProjectSettings.summary`, `notes_and_summary_survive_save_and_load`) is editable. (R40a) Color management (`ProjectSettings.color_management`) replaces the view transform row. (R10a) Master bus mode (Stereo / 5.1 Surround / 7.1 Surround) and Resample and stretch quality are editable. (R40b) Pixel format, HDR Mode, Compositing gamma, ACES version, ACES color space and View transform (`ProjectSettings.pixel`, `project/pixel.rs`) are editable; Look modification transform is greyed with its reason (a look LUT goes on the Video Output FX). |
| 4.12 | Render / export controls | DONE | R11, `ui/dialogs/render.rs`: 38 built-in templates plus the user's (`Prefs.render_templates`), the Usecase tree, Filter, View (Thumbnails / Details), favourites and Recent kept in the preferences; Customize Template (`ui/dialogs/template.rs`); Render Options stretch / rotation / markers; Show log. (R10a) Enable multichannel mapping and the 5.1 / 7.1 templates. R12: Add to Render Queue, Render in the background (a window instead of a modal dialog), Render to New Track |
| 4.13 | Toolbar and contextual menus | DONE | `ui/toolbar.rs`, `ui/timeline/ctx.rs`. Many rows inside them are greyed (see section 3). |
| 4.14 | Workspace resizable and customizable | DONE | Splitters, tab moves and saved layouts work; (R14) windows float on any monitor and dock again; (R15) Options › Customize Toolbar and Customize Timeline Toolbar add, remove and reorder buttons and separators, saved in prefs (`toolbar.rs` `Bar`, `dialogs/customize_toolbar.rs`); (R49) the track header's Edit Visible Button Set (`dialogs/button_set.rs`, `Prefs.header_buttons`): hidden buttons move to the header's More menu. |
| 4.15 | Cleaner typography | DONE | `ui/fonts.rs`, `ui/theme.rs` |
| 4.16 | Consistent icons | DONE | `ui/icons.rs` (967 lines of vector icons) |
| 4.17 | Better spacing and alignment | DONE | `ui/theme.rs` metrics (`docs/UI_SPEC.md` §4) |
| 4.18 | Modern dark theme | DONE | VP16 and VP23 dark (`theme.rs`) |
| 4.19 | Optional light theme | DONE | VP23 `Variant::White` (`theme.rs:17-21`) |
| 4.20 | Smooth but restrained animations | DONE | (R48) One policy: `theme::anim(ms)` gives every transition its length, 0 when Windows' "Show animations in Windows" (`SPI_GETCLIENTAREAANIMATION`) is off; egui's own transitions and scrolling and the chooser's hovered-transition preview follow it (test `animations_off_gives_zero_duration_transitions`). |
| 4.21 | Resizable and dockable panels | DONE | `ui/dock.rs`: dock columns and tab moves; (R13) docking areas on four sides in VP23 (one, above or below the timeline, in VP16: `dock::Area`, `chrome::areas`), each minimised on its own (F11, Alt+F11, Ctrl+Alt+F11, Ctrl+Shift+F11), Maximize Timeline, Collapse Track Headers, Next/Previous Window, Focus to Timeline, the ◂ ▸ tab overflow scroller, and the Display page's auto-hide, timeline-at-bottom (VP16) and tabs-at-top options; (R14) floating windows (`Dock::show_floating`, one egui viewport each; Undock, or drag a tab off the docking areas), dragging a tab to the main window's edge docks it in that area, a floating window moved over the dock docks under "Allow floating windows to dock" (Ctrl inverts it). Tests: `drop_targets`, `drag_tab_out_floats_it`, `floating_windows_open_close_dock_and_cycle`. |
| 4.22 | Adjustable workspace layouts | DONE | User slots 0-9 (`settings.rs`, batch 11; 0.8.0's per-look slots carry over, `settings::migrate_look_keys`); (R13) the built-in VEGAS layouts Audio Mixing, Color Correction, Source Window and Timeline Top (`Dock::builtin`, decoded from VEGAS 15's `Standard Layouts`), Organize Layouts (`ui/dialogs/organize_layouts.rs`) and Reload Selected Layout. Tests: `builtin_layouts_open_the_vegas_panels`, `dock_commands_toggle_back`, `overflowing_tabs_scroll`. (R16b) Color Correction has VEGAS's Video Scopes column (539 px) between the group and Video Preview. |
| 4.23 | Useful contextual menus | DONE | `ui/timeline/ctx.rs`, `ui/media_panel.rs` |
| 4.24 | Searchable commands | MISSING | No command palette. → R47 |
| 4.25 | Searchable effects | DONE | `ui/choosers.rs:85` search field |
| 4.26 | Custom keyboard shortcuts | DONE | (R15) Options › Customize Keyboard: searchable command list named by menu path, key capture, "currently used by" with the key taken from the other command, Remove, Reset, [Default]; per-look keys in prefs override the table (`keymap::set_overrides`); keyboard maps saved, loaded and deleted as .ini files in %APPDATA%\MilanPro\Keyboard; menus and toolbar tooltips show the user's keys. Tests: `keymap::overrides_replace_defaults_and_clashes_are_found`, `customize_keys::rebind_conflict_and_reset`, `keyboard_maps_save_load_and_delete`, `menus::menu_shortcut_text_follows_the_overrides`. |
| 4.27 | High-DPI display support | DONE | (R48) View › Interface Scale and Preferences › Display (Auto = Windows' scale, 100-200 %); bevels, rules and icon strokes are whole physical pixels on pixel edges (`theme::one`, `snap`; icons drawn on the physical grid), egui's anti-aliasing feather is off at 125/150/175 % so 1-point edges never blur, brand bitmaps are drawn at whole multiples of their pixels; dialogs fit the window and their pages scroll; the harness renders any scale (`MILAN_SCALE`, `MILAN_SHOT_SIZE`) and checks that text fits (`demo::overflow`). |
| 4.28 | Proper multi-monitor support | DONE | (R14) Floating windows on any display, saved in layouts with their rect (physical pixels) and display, reopened on the primary display when theirs is gone (`ui/monitors.rs`: `EnumDisplayMonitors`, `place`); each window takes its own display's scaling and is placed in it (`monitors::settle`). Preview on External Monitor (Alt+Shift+4, Video Preview toolbar) full screen on the Preferences › Preview Device display (`App::show_external`, `preview::draw_external`). Tests: `layout_serde_round_trip_with_floating_windows`, `floating_window_on_a_removed_monitor_reopens_on_the_primary`, `external_monitor_frame_geometry`. Not checked by hand on two monitors or a 150 %/100 % mix yet (the harness never opens a window on a monitor). |
| 4.29 | No excessive visual effects; serious professional feel | DONE | |
| 4.30 | Save and restore workspace layouts | DONE | `Prefs.layouts` with `merge_layouts` (`settings.rs:61-75`) |

### 2.5 Section 5: timeline editing engine

| # | Requirement | Status | Evidence / what is missing |
|---|---|---|---|
| 5.1 | Proper multitrack engine | DONE | `engine/plan.rs`, `edit.rs` (3402 lines), `history.rs` |
| 5.2 | Unlimited practical video tracks | DONE | `Vec<Track>` |
| 5.3 | Video track ordering | DONE | `edit::move_tracks` (`edit.rs:831`), header drag (`tracks.rs:146`) |
| 5.4 | Video track visibility | DONE | Switches › Show in Preview (`Track.visible`, `Timeline::shown_video_tracks`) takes a video track out of preview and render, and its events are drawn dimmed like a muted track's; the audio track header's Hide takes a row off the timeline (`Track.hidden`, it still plays; the track list's empty-area menu has Show Hidden Tracks) (R5) |
| 5.5 | Track locking | DONE | `Track.locked` |
| 5.6 | Track muting | DONE | |
| 5.7 | Compositing and blending | DONE | R7: all 15 VEGAS modes (3D Source Alpha, Cut, Hard Light, Dodge, Burn, Difference Squared added; `composite::blend`), Custom with the Compositors window (Bump Map, Displacement Map, Height Map), Make Compositing Parent / Child with Parent Composite Mode and Parent Motion (`composite::compose`). Soft Light, Exclusion, Hard Mix, Hue, Saturation, Color and Luminosity are Milan Pro extras behind Preferences › Video (off). |
| 5.8 | Video track-level effects | DONE | Track FX (`FxTarget::Track`) |
| 5.9 | Track opacity | DONE | Composite level and envelope |
| 5.10 | Track grouping | DONE | `Timeline.track_groups`, `edit::group_tracks` / `ungroup_tracks` / `toggle_track_groups` / `tidy_track_groups`; group header rows with ▾, name, M / S (`ui/timeline/tracks.rs`); Track Group ▸ menu (`ctx.rs::track_group`) (R5) |
| 5.11 | Unlimited practical audio tracks | DONE | |
| 5.12 | Track volume | DONE | |
| 5.13 | Track panning | DONE | |
| 5.14 | Track mute and solo | DONE | Mute, Solo, and (R2) exclusive mute/solo Shift+Z / Shift+X (`edit::exclusive_tracks`); Options › Mute All Audio / Mute All Video (playback only) |
| 5.15 | Track automation | DONE | Volume/pan/mute envelopes; (R10c) the master's Volume and Mute envelopes on a Master bus track, with Automation Settings from the Master Bus window's badge; (R8c) Automation Settings per audio track and bus (`Track/Bus.automation`, `auto_controls`; Off leaves the envelopes and FX automation out of the plan), automation controls in the track header, bus tracks and Mixing Console that follow and write the envelopes (`ui/automation.rs`: Touch while held, Latch until stop, a point per 20 ms and 0.5 dB at most), FX automation envelopes (`Fx.auto`, FX Automation chooser, E / Shift+E, envelope Properties), audio event envelopes. Tests `a_touch_write_is_thinned_and_stays_inside_the_hold`, `a_threshold_envelope_changes_the_gain_reduction`, `render_e2e` (automation renders as it plays). |
| 5.16 | Waveform rendering | DONE | `audio.rs` Waveforms |
| 5.17 | Audio track-level effects | DONE | Batch 16 |
| 5.18 | Audio routing | DONE | (R8a) Track header › Output (Master or a bus, `ctx.rs::routing`, `edit::set_track_out`); sends to busses (pre-volume and pre-mute) and assignable FX chains (post-volume) on the multipurpose slider (`tracks.rs`, Pre/Post Volume in its label list); busses route to the master or another bus with loops refused (`edit::set_bus_out`); `engine/mix.rs` tests `a_track_through_a_bus_at_minus_6_db_is_halved`, `a_pre_volume_send_feeds_the_bus_with_the_fader_down`, `a_bus_loop_is_rejected`, `busses_play_and_render_the_same` |
| 5.19 | Drag-and-drop media placement | DONE | `app/drop.rs`, batch 5b |
| 5.20 | Move clips | DONE | |
| 5.21 | Trim clips | DONE | |
| 5.22 | Split clips | DONE | `edit::split_ids` |
| 5.23 | Ripple editing | DONE | Auto Ripple plus post-edit ripple modes |
| 5.24 | Slip editing | DONE | Batch 7 |
| 5.25 | Slide editing | DONE | Batch 7 |
| 5.26 | Looping (events) | DONE | R3a: Switches › Loop (on by default, `Clip.looping`; event menu, Edit › Switches, Event Properties). A looping event drags past its media end (`edit::media_limit` is unbounded on the right) and repeats the whole file (or subclip): video wraps in `plan::frame_plan` (`edit::loop_time`), audio reads one stream per pass (`edit::loop_passes`, `AEvent.passes`, `mix::Mixer::block`), so preview and render match; notches mark each wrap (`ui/timeline/events.rs`). Takes shorter than the event loop. Tests `a_looping_event_repeats_its_media`, `a_looping_event_restarts_its_media_at_each_pass`, `loop_notches_sit_where_the_media_wraps`, `loop_passes_split_at_the_media_end` |
| 5.27 | Playback rate adjustment | DONE | `Clip.rate`, Event Properties |
| 5.28 | Reverse playback (events) | DONE | `Clip.reverse` renders correctly. R20: the preview reads it from chunks decoded forward and read backwards (`engine/vdecode.rs`), started before the event plays (`Worker::warm_back`): 301 of 301 frames exact at 1x on 1080p GOP-250 media, where 1 of 301 was before (ffmpeg-backed `reverse_playback_shows_every_frame`, `docs/PERFORMANCE.md`) |
| 5.29 | Clip grouping | DONE | Create/remove/clear/select, and (R3a) Group › Cut All / Copy All / Delete All in Edit › Group and the event menu: the whole group of every selected event, whatever Ignore Event Grouping says (`app/commands.rs`). Test `groups_expand_split_and_dissolve` |
| 5.30 | Event envelopes | DONE | Volume, pan, mute and opacity work. R4a: event Velocity (-100%..1000%, `engine/velocity.rs`, `edit::src_time`), Freeze Frame at Cursor (`edit::freeze_frame`) and Transition Progress (`Transition.progress`), inserted from the event menu and Insert › Video Envelopes, drawn and edited on the event (`ui/timeline/envelopes.rs`); video track Mute and Fade to Color envelopes with Fade Colors. Tests `velocity_drives_the_source_time`, `track_mute_fade_to_color_and_velocity`, `progress_fade_out_transition_and_fade_to_color`, `render_e2e` (parity). R4b: with Options › Audio Follows Video Velocity on (off by default, as VEGAS), the audio of an A/V pair (grouped, same place and length) plays a copy of its video event's Velocity envelope, varispeed (`edit::sync_velocity`, `mix::Mixer::varispeed`, `engine/resample.rs`); tests `audio_follows_its_video_velocity`, `velocity_plays_audio_varispeed` |
| 5.31 | Fade-in and fade-out | DONE | Fade curves and crossfades |
| 5.32 | Audio and video synchronization | DONE | Grouped A/V, audio master clock (`FEATURE_PLAN.md` 1.6) |
| 5.33 | Precise frame-level editing | DONE | Quantize to Frames |
| 5.34 | Snap-to-grid | DONE | R1: Options › Snap to Grid (Ctrl+F8) and the 14 Grid Spacing radios (`Prefs::snap_grid`, `Prefs::grid`); `edit::Snaps` snaps to grid lines from `ui/timeline/ruler.rs::grid_line`, which also draws them. Move Left/Right by Grid Marks (PageUp/PageDown, `edit::grid_step`). Tests `snap_to_a_one_second_grid_lands_on_whole_seconds`, `ruler_and_grid_intervals` |
| 5.35 | Snapping to clips and markers | DONE | `edit::snap_targets_except`. Snap to All Events (Ctrl+Shift+F8, R1): off, a dragged event snaps to edges on its own tracks only, as in VEGAS; test `snap_to_all_events_reaches_other_tracks` |
| 5.36 | Timeline zooming and scrolling | DONE | |
| 5.37 | Markers and regions | DONE | `Timeline.markers`, `regions` |
| 5.38 | Undo and redo | DONE | `history.rs`, Undo All, history list on the split button |
| 5.39 | Accurate timeline mathematics and timecode | DONE | R1: All 15 VEGAS ruler formats (`project::format_time` / `parse_time`, `RulerFormat`), used by the ruler, time display, transport length boxes and Go to Time (Ctrl+G, `ui/dialogs/goto.rs`); ruler start time and Set Time at Cursor; Measures & Beats from the project tempo. Test `every_ruler_format_round_trips` (every format at 23.976, 25, 29.97 DF, 59.94 DF, 48 kHz) |
| 5.40 | Different frame rates, timebases, resolutions and sample rates without drift | DONE | R1: `project::rate_ratio` makes 29.97 exactly 30000/1001; the video plan (`frame_time`), render frame count (`plan::frame_range`), Quantize to Frames and timecode all use it, and `fps_str` is built from it. Test `exact_rates_do_not_drift`: frame n at 3 h lands on n, 1 h at 29.97 renders 107892 frames |

### 2.6 Section 6: preview player

| # | Requirement | Status | Evidence / what is missing |
|---|---|---|---|
| 6.1 | No whole-video-in-memory player; dedicated pipeline | DONE | Persistent decoder pool plus worker (`engine/vdecode.rs`, `engine/worker.rs`) |
| 6.2 | Low-latency playback | DONE | Play start ≤ 250 ms target (`FEATURE_PLAN.md` 1.9) |
| 6.3 | Accurate frame timing | DONE | Frames are popped against the audio clock (`FEATURE_PLAN.md` 1.5.4) |
| 6.4 | Audio/video synchronization | DONE | Locked to the audio clock; (R9) the playhead is what is heard: the clock less the output latency the device reports, carried on between callbacks (`audio::Stamp`, test `the_playhead_follows_what_is_heard`). The manual click-track check (under 5 ms) is the user's. |
| 6.5 | GPU-accelerated compositing | DONE | (R21a-c) See 3.4. `docs/PERFORMANCE.md`: four full 1080p layers compose in 4.7 ms on the GPU (22 ms on the CPU), four picture-in-pictures in 1.2 ms (13 ms), four 4K layers in 30 ms (103 ms), uploads of the decoded pictures included. |
| 6.6 | Hardware decoding when available | DONE | (R22) The probe measures each hwaccel against the software decoder; Automatic uses the faster one for heavy sources, a chosen one for every codec it passed. |
| 6.7 | Efficient frame caching | DONE | `engine/cache.rs` (SourceCache 192 MB, CompositeCache 256 MB) |
| 6.8 | Asynchronous media decoding | DONE | Decoder child processes and reader threads |
| 6.9 | Background thumbnail generation | DONE | `engine/thumbs.rs`, `decode.rs` Thumbs |
| 6.10 | Background waveform generation | DONE | `audio.rs` Waveforms worker |
| 6.11 | Intelligent caching | DONE | (R23) Build Dynamic RAM Preview (Shift+B, the loop region or from the cursor, capped by Preferences › Video, frames pinned until an edit changes them; the green bar on the ruler), Selectively Prerender Video (Shift+M: the sections with FX, transitions, compositing or motion rendered to the project's prerender folder, played and rendered from the files while their frames' hash matches; the blue bar; Clean Up Prerendered Video) and a disk frame cache for decoded HEVC/AV1/4K frames (LRU under a cap, off by default). |
| 6.12 | Efficient seeking | DONE | R20: frame-exact (200 random seeks on GOP-250 media and on its proxy, ffmpeg-backed `seeks_are_frame_exact_on_long_gop`); a paused seek of source media decodes with up to 4 threads (`vdecode::seek_threads`) and a step back reads a short chunk instead of respawning. Random seek p95 about 126 ms on a proxy; on GOP-250 1080p originals 300-450 ms with machine load (582 ms before, measured with the same test) (`docs/PERFORMANCE.md`) |
| 6.13 | Scrubbing | DONE | R20: the jump was the Video Preview showing only the answer to the newest Show, so a drag showed nothing until it slowed, and each step back respawned a decoder. Any newer answer now shows and steps back read chunks decoded forward. Dragging at 2x the picture trails the cursor about 0.05 s forwards and 0.1 s backwards on proxies; scrub p95 98.9 ms on proxies (target under 150 ms), steps back p95 8 ms (ffmpeg-backed `scrub_drag_follows_the_cursor`, `bench_scrub`, `docs/PERFORMANCE.md`) |
| 6.14 | Frame-by-frame navigation | DONE | `PrevFrame`/`NextFrame` |
| 6.15 | Variable playback speed | DONE | R4b: J / K / L shuttle (1x to 8x each way) and the Rate slider play the timeline's sound at their speed, varispeed and backwards below 0 (`mix::shuttle`, `engine/resample.rs`, a 32-tap windowed sinc); the device clock moves the cursor by the rate (`audio::Feed`, `Player::set_rate`). Tests `shuttle_plays_the_timeline_at_its_rate`, `shuttle_clock_advances_by_the_rate`, `snr_above_90_db_at_ratio_0_9` |
| 6.16 | Preview quality settings | DONE | Draft/Preview/Good/Best (`decode.rs:13`) |
| 6.17 | Proxy media | DONE | `proxy.rs`, batch 12 |
| 6.18 | Optimized timeline playback | DONE | Fast-path compositor, pre-warm (`plan::upcoming`) |
| 6.19 | Multitrack preview compositing | DONE | `engine/composite.rs` |
| 6.20 | Full resolution | DONE | `Size::Full` |
| 6.21 | Half resolution | DONE | `Size::Half` |
| 6.22 | Quarter resolution | DONE | `Size::Quarter` |
| 6.23 | Draft | DONE | `Quality::Draft` |
| 6.24 | Automatic adaptive quality | DONE | R19: the Video Preview's right-click Adjust Size and Quality for Optimal Playback (on, as in VEGAS; `decode::Adapt`, `decode::ladder`): while playing, more than 2 % of frames dropped over 2 s steps the preview down (Full, Half, Quarter, then Draft), 10 s without a drop steps it back up, a failed step up doubles the next wait (hysteresis, up to 160 s), and counting waits for the decoders to spin up after each change. Stopped, the preview is at the chosen quality again; the render never uses it. Tests `adaptive_quality_steps_down_then_up_without_oscillating`, `adaptive_quality_restores_when_playback_keeps_up`, ffmpeg-backed `bench_adaptive_quality` (`docs/PERFORMANCE.md`) |
| 6.25 | Expensive work cannot freeze the UI | DONE | Worker threads; render on its own thread |
| 6.26 | Performance benchmarks; measured stability | DONE | R19: `tools/bench.ps1` makes the synthetic media with ffmpeg lavfi, runs the ignored benches one at a time in release and writes `docs/PERFORMANCE.md` (machine, method, 1-4 layers: fps, dropped %, play start, A/V offset; scrub p50/p95; Adjust Size and Quality off and on). The Video Preview shows the frame rate it plays at in its Display value (`warning_text` when frames drop, as VEGAS), and Show Playback Statistics (right-click, off by default) adds a line: frame rate (target), dropped frames, live decoders, cache hits and the adjusted setting (`engine::Shows`, `engine::Stats`) |

### 2.7 Section 7: formats

| # | Requirement | Status | Evidence / what is missing |
|---|---|---|---|
| 7.1 | Comprehensive media backend | DONE | FFmpeg 9.0.2 full build bundled (`installer/`) |
| 7.2 | Codec availability, hardware acceleration, licensing and distribution researched | DONE | R37: `engine/caps.rs` asks the bundled FFmpeg for its demuxers, decoders, encoders, muxers, pixel formats and filters (cached per FFmpeg version) and `docs/FORMAT_SUPPORT.md` has the capability table, the tested decode matrix with a licence column per format and the render table; hardware acceleration is R22. (R56) Licensing and distribution: [`docs/LICENSES.md`](LICENSES.md) (FFmpeg GPL v3 build, libx264/libx265, no libfdk-aac, codec patents). |
| 7.3 | H.264 / AVC | DONE | Decode and encode (libx264) work. R37: decode checked by `format_corpus_e2e` (generated file, frame 0 and the middle frame against the bars, 1 s of sound by RMS), `docs/FORMAT_SUPPORT.md`. |
| 7.4 | H.265 / HEVC | DONE | Decode and encode (libx265) work. R37: decode checked by `format_corpus_e2e` (generated file, frame 0 and the middle frame against the bars, 1 s of sound by RMS), `docs/FORMAT_SUPPORT.md`. |
| 7.5 | AV1 | DONE | Decodes. R11: renders with libsvtav1 ("AV1 (SVT)" template, MP4 or WebM), ffprobe-checked by `render_templates_e2e`, `docs/FORMAT_SUPPORT.md`. Decode not covered by a format test. R37: decode checked by `format_corpus_e2e` (generated file, frame 0 and the middle frame against the bars, 1 s of sound by RMS), `docs/FORMAT_SUPPORT.md`. |
| 7.6 | VP8 | DONE | Decodes. R11: "VP8 WebM" template (libvpx), ffprobe-checked by `render_templates_e2e`, `docs/FORMAT_SUPPORT.md`. R37: decode checked by `format_corpus_e2e` (generated file, frame 0 and the middle frame against the bars, 1 s of sound by RMS), `docs/FORMAT_SUPPORT.md`. |
| 7.7 | VP9 | DONE | Decodes. R11: "VP9 WebM" template (libvpx-vp9), ffprobe-checked by `render_templates_e2e`, `docs/FORMAT_SUPPORT.md`. R37: decode checked by `format_corpus_e2e` (generated file, frame 0 and the middle frame against the bars, 1 s of sound by RMS), `docs/FORMAT_SUPPORT.md`. |
| 7.8 | MPEG-1 | DONE | Decodes. R37: decode checked by `format_corpus_e2e` (generated file, frame 0 and the middle frame against the bars, 1 s of sound by RMS), `docs/FORMAT_SUPPORT.md`. Decode only: no MPEG-1 render template (VEGAS has none either). |
| 7.9 | MPEG-2 | DONE | Decodes. R11: "MPEG-2 TS" and "MPEG-2 Program Stream" templates (also interlaced: upper field first, checked with `idet`), ffprobe-checked by `render_templates_e2e`, `docs/FORMAT_SUPPORT.md`. R37: decode checked by `format_corpus_e2e` (generated file, frame 0 and the middle frame against the bars, 1 s of sound by RMS), `docs/FORMAT_SUPPORT.md`. |
| 7.10 | MPEG-4 Part 2 | DONE | Decodes. R37: decode checked by `format_corpus_e2e` (generated file, frame 0 and the middle frame against the bars, 1 s of sound by RMS), `docs/FORMAT_SUPPORT.md`. |
| 7.11 | ProRes | DONE | Decodes. R11: renders every profile (Proxy, LT, 422, HQ, 4444, 4444 XQ); "ProRes 4444 (alpha)" keeps transparency where nothing covers the frame (`VideoPlan.alpha`), ffprobe-checked by `render_templates_e2e`, `docs/FORMAT_SUPPORT.md`. R37: decode checked by `format_corpus_e2e` (generated file, frame 0 and the middle frame against the bars, 1 s of sound by RMS), `docs/FORMAT_SUPPORT.md`. |
| 7.12 | DNxHD | DONE | Decodes. R11: DNxHD is a Customize profile (constant bit rate; its fixed sizes and rates are FFmpeg's); no built-in template, DNxHR has them. R37: decode checked by `format_corpus_e2e` (generated file, frame 0 and the middle frame against the bars, 1 s of sound by RMS), `docs/FORMAT_SUPPORT.md`. R41: "MXF DNxHD 145 1080" template (1920x1080, 145 Mbps 8-bit, MXF OP1a), ffprobe-checked by `render_templates_e2e`. |
| 7.13 | DNxHR | DONE | Decodes. R11: DNxHR LB/SQ/HQ/HQX/444 templates in MOV and DNxHR HQ in MXF, ffprobe-checked by `render_templates_e2e`, `docs/FORMAT_SUPPORT.md`. R37: decode checked by `format_corpus_e2e` (generated file, frame 0 and the middle frame against the bars, 1 s of sound by RMS), `docs/FORMAT_SUPPORT.md`. |
| 7.14 | CineForm | DONE | Decodes. R11: "CineForm" template (cfhd, quality presets), ffprobe-checked by `render_templates_e2e`, `docs/FORMAT_SUPPORT.md`. R37: decode checked by `format_corpus_e2e` (generated file, frame 0 and the middle frame against the bars, 1 s of sound by RMS), `docs/FORMAT_SUPPORT.md`. |
| 7.15 | MJPEG | DONE | Decodes. R37: decode checked by `format_corpus_e2e` (generated file, frame 0 and the middle frame against the bars, 1 s of sound by RMS), `docs/FORMAT_SUPPORT.md`. |
| 7.16 | Theora | DONE | Decodes. R37: decode checked by `format_corpus_e2e` (generated file, frame 0 and the middle frame against the bars, 1 s of sound by RMS), `docs/FORMAT_SUPPORT.md`. |
| 7.17 | FFV1 | DONE | Decodes. R11: "FFV1 MKV archive" template (lossless RGB, FLAC), ffprobe-checked by `render_templates_e2e`, `docs/FORMAT_SUPPORT.md`. R37: decode checked by `format_corpus_e2e` (generated file, frame 0 and the middle frame against the bars, 1 s of sound by RMS), `docs/FORMAT_SUPPORT.md`. |
| 7.18 | Animation codecs (QuickTime Animation/qtrle, PNG-in-MOV, UT Video) | DONE | Decode (alpha is handled, `media.rs:131`). R37: decode checked by `format_corpus_e2e` (generated file, frame 0 and the middle frame against the bars, 1 s of sound by RMS), `docs/FORMAT_SUPPORT.md`. |
| 7.19 | RAW video formats | NOT FEASIBLE | R3D, BRAW, ARRIRAW, Canon Cinema RAW Light and Nikon N-RAW need proprietary SDKs that an MIT project cannot redistribute; FFmpeg has no decoders for them. Deliverable shipped (R41): they import as offline items that give the reason (`camera::RAW`), listed in `docs/FORMAT_SUPPORT.md` and `docs/COMPATIBILITY.md`; CinemaDNG is listed unsupported too (FFmpeg reads some DNG stills without the camera colour pipeline; DNG frames are not offered as a sequence). Not built: the optional runtime-loaded BRAW SDK study. |
| 7.20 | WAV | DONE | Works. R37: decode checked by `format_corpus_e2e` (generated file, frame 0 and the middle frame against the bars, 1 s of sound by RMS), `docs/FORMAT_SUPPORT.md`. |
| 7.21 | AIFF | DONE | Decodes. R37: decode checked by `format_corpus_e2e` (generated file, frame 0 and the middle frame against the bars, 1 s of sound by RMS), `docs/FORMAT_SUPPORT.md`. |
| 7.22 | MP3 | DONE | Decode and encode. R37: decode checked by `format_corpus_e2e` (generated file, frame 0 and the middle frame against the bars, 1 s of sound by RMS), `docs/FORMAT_SUPPORT.md`. |
| 7.23 | AAC | DONE | Decode and encode. R37: decode checked by `format_corpus_e2e` (generated file, frame 0 and the middle frame against the bars, 1 s of sound by RMS), `docs/FORMAT_SUPPORT.md`. |
| 7.24 | FLAC | DONE | Decodes. R11: "FLAC audio" template (16/24-bit), FLAC in MKV, ffprobe-checked by `render_templates_e2e`, `docs/FORMAT_SUPPORT.md`. R37: decode checked by `format_corpus_e2e` (generated file, frame 0 and the middle frame against the bars, 1 s of sound by RMS), `docs/FORMAT_SUPPORT.md`. |
| 7.25 | ALAC | DONE | Decodes. R11: "ALAC audio" template (.m4a), ffprobe-checked by `render_templates_e2e`, `docs/FORMAT_SUPPORT.md`. R37: decode checked by `format_corpus_e2e` (generated file, frame 0 and the middle frame against the bars, 1 s of sound by RMS), `docs/FORMAT_SUPPORT.md`. |
| 7.26 | OGG Vorbis | DONE | Decodes. R11: "Ogg Vorbis audio" and VP8 WebM templates, ffprobe-checked by `render_templates_e2e`, `docs/FORMAT_SUPPORT.md`. R37: decode checked by `format_corpus_e2e` (generated file, frame 0 and the middle frame against the bars, 1 s of sound by RMS), `docs/FORMAT_SUPPORT.md`. |
| 7.27 | Opus | DONE | Decodes. R11: "Opus audio" and VP9 WebM templates (48 kHz), ffprobe-checked by `render_templates_e2e`, `docs/FORMAT_SUPPORT.md`. R37: decode checked by `format_corpus_e2e` (generated file, frame 0 and the middle frame against the bars, 1 s of sound by RMS), `docs/FORMAT_SUPPORT.md`. |
| 7.28 | WMA | DONE | Decodes (wmav2; WMA Pro and Lossless partly). R37: decode checked by `format_corpus_e2e` (generated file, frame 0 and the middle frame against the bars, 1 s of sound by RMS), `docs/FORMAT_SUPPORT.md`. |
| 7.29 | PCM | DONE | WAV preset, pcm_s16le in the ProRes MOV |
| 7.30 | AC-3 | DONE | Decodes. R11: "AC-3 audio" template and AC-3 in the MPEG-2 templates, ffprobe-checked by `render_templates_e2e`, `docs/FORMAT_SUPPORT.md`. R37: decode checked by `format_corpus_e2e` (generated file, frame 0 and the middle frame against the bars, 1 s of sound by RMS), `docs/FORMAT_SUPPORT.md`. |
| 7.31 | E-AC-3 | DONE | Decodes. R11: "E-AC-3 audio" template, ffprobe-checked by `render_templates_e2e`, `docs/FORMAT_SUPPORT.md`. R37: decode checked by `format_corpus_e2e` (generated file, frame 0 and the middle frame against the bars, 1 s of sound by RMS), `docs/FORMAT_SUPPORT.md`. |
| 7.32 | DTS where supportable | DONE | Decodes (dca). The encoder is experimental and core-only and no template offers it. R37: decode checked by `format_corpus_e2e` (generated file, frame 0 and the middle frame against the bars, 1 s of sound by RMS), `docs/FORMAT_SUPPORT.md`. Licensing is in [`docs/LICENSES.md`](LICENSES.md) (R56). |
| 7.33 | Multichannel audio (formats) | DONE | R10a: a 5.1 / 7.1 track decodes its media to its layout (`-ac 6` / `-ac 8`, channel for channel when it already is); a stereo track takes the BS.775 downmix or (Channels ▸, R10b) one stereo pair; renders to 5.1 / 7.1 WAV, FLAC, AAC, AC-3, E-AC-3 and MP4 / MPEG-2 TS templates with `-channel_layout`. Checked against FFmpeg's own output within 1e-4 (`surround_renders_match_ffmpeg`) |
| 7.34 | MP4 | DONE | Import and render |
| 7.35 | MOV | DONE | Import and render |
| 7.36 | MKV | DONE | Import; R11 render ("FFV1 MKV archive", any listed codec through Customize Template), ffprobe-checked by `render_templates_e2e`, `docs/FORMAT_SUPPORT.md` |
| 7.37 | AVI | DONE | Import; R11 render ("AVI (MJPEG)"), ffprobe-checked by `render_templates_e2e`, `docs/FORMAT_SUPPORT.md` |
| 7.38 | MPEG-TS | DONE | Import (`.ts`, `.m2ts`, `.mts`); R11 render ("MPEG-2 TS", `.m2ts`), ffprobe-checked by `render_templates_e2e`, `docs/FORMAT_SUPPORT.md` |
| 7.39 | MPEG-PS | DONE | Import (`.mpg`, `.vob`); R11 render ("MPEG-2 Program Stream", `.mpg`), ffprobe-checked by `render_templates_e2e`, `docs/FORMAT_SUPPORT.md` |
| 7.40 | WebM | DONE | Import; R11 render ("VP9 WebM", "VP8 WebM"), ffprobe-checked by `render_templates_e2e`, `docs/FORMAT_SUPPORT.md` |
| 7.41 | MXF | DONE | Import: MXF OP1a (XDCAM HD422 MPEG-2, DNxHD, AVC-Intra 100) and OP-Atom (DNxHD, PCM) stand-ins decode with their timecode (`format_corpus_e2e`, `camera_formats_e2e`); P2 and XDCAM folders import from File › Import › Camera Card Folder (R41). R11: "MXF DNxHR HQ" render, R41: "MXF DNxHD 145 1080", ffprobe-checked by `render_templates_e2e`, `docs/FORMAT_SUPPORT.md`. |
| 7.42 | WAV container | DONE | |
| 7.43 | AIFF container | DONE | Import; R11 render ("AIFF audio", 8/16/24/32-bit big-endian PCM), ffprobe-checked by `render_templates_e2e`, `docs/FORMAT_SUPPORT.md` |
| 7.44 | PNG | DONE | `media.rs:108` |
| 7.45 | JPEG | DONE | |
| 7.46 | TIFF | DONE | Imported; R40b: in a 32-bit floating point project 16-bit TIFF decodes at 16 bits and float TIFF in float (an 8-bit project squeezes them to 8-bit, as before). |
| 7.47 | BMP | DONE | |
| 7.48 | WebP | DONE | R38: stills, and animated WebP imports as video (`animated_gif_e2e`: 10 distinct frames). |
| 7.49 | GIF | DONE | R38: a GIF with several frames imports as video, one frame stays a picture (`animated_pictures_are_video`, `animated_gif_e2e`: a 10-frame GIF plays 10 distinct frames). |
| 7.50 | EXR | DONE | R38: single files and sequences decode; an 8-bit project shows linear float through a simple sRGB transfer (values above 1.0 clip). R40b: a 32-bit floating point project decodes it in float (zimg scaling, highlights kept), composes it as linear light, and the OpenEXR sequence template writes linear float (`linear_exr_keeps_its_highlights`). |
| 7.51 | OpenEXR sequences | DONE | R38: an EXR sequence opens as one item like any still sequence; R40b: in float in a 32-bit floating point project (see 7.50). |
| 7.52 | Image sequences | DONE | R38: a picked or dropped numbered still asks "Open still image sequence" (`ui/dialogs/sequence.rs`, the answer remembered); checked, the run from its lowest number to the first gap is one video item at the project rate read through image2 (`MediaItem::sequence`, `plan::source_input`). `image_sequence_e2e`: 48 PNGs at 24 fps last 2.0 s, the 10th frame is file 0010, the gap stops the run, and a PNG sequence render gives each file back pixel for pixel. Archive Project copies every frame. |
| 7.53 | Other production image formats (DPX, TGA, PSD, HDR, JPEG 2000) | DONE | R38: DPX, JPEG 2000, Radiance HDR (tone-mapped), DNG, SVG (offline with the reason when FFmpeg lacks librsvg) and JPEG XL join TGA and PSD in `IMAGE_EXTS`; each FFmpeg can write is decoded in `image_sequence_e2e`. |
| 7.54 | Subtitles | DONE | R39 (`interchange/subtitles.rs`, `ui/dialogs/subtitles.rs`): Insert › Subtitles from File... reads SRT, WebVTT and ASS / SSA into Titles & Text events (the Subtitle preset, or the ASS styles: font, size, colours, outline, shadow, alignment, margins, `{\anN}`) on a new top track or a chosen one, at the file's times or from the cursor; Insert › Subtitles from Regions; File › Export › Subtitles... writes a track's Titles & Text events as SRT or WebVTT; Render As burns them in (they are video) and can also write an SRT / WebVTT file beside the render or embed a soft stream (`mov_text` in MP4 / MOV, SubRip in MKV, WebVTT in WebM), with the burn-in optional. Tests `srt_round_trip`, `webvtt_reads_and_writes`, `ass_imports_with_styles_mapped`, `srt_import_then_export_is_exact`, `subtitles_render_as_a_sidecar_and_a_soft_stream` (ignored, FFmpeg) |
| 7.55 | Closed captions | NOT FEASIBLE | CEA-608/708: the bundled FFmpeg has no CEA-608/708 encoder, and its decoder finds them only in a video stream's side data (A53), which Milan Pro's decode does not keep. Import › Closed Captioning says so; captions are made as subtitles (7.54) and delivered as SRT / WebVTT sidecars or soft streams |
| 7.56 | Alpha channels | DONE | `has_alpha` probe and premultiply (`media.rs:131`, `vdecode.rs`) |
| 7.57 | High bit-depth footage | DONE | R40b: a 32-bit floating point project decodes 10-bit and wider sources at 16 bits (`rgba64le`) and float ones in float, composes in float and renders 16 bits to the encoder: a 10-bit source round-trips through a 10-bit render within 1 code value (`ten_bit_source_round_trips_through_a_ten_bit_render`). An 8-bit project is as before. |
| 7.58 | HDR media | DONE | R40a: transfer, primaries, matrix, bit depth and HDR10 mastering display / content light level are probed (stream or first-frame side data) and shown in Media Properties, Project Properties lists the HDR media and holds Color management (tone-mapped by default, clip, off). PQ and HLG reach the Rec.709 timeline through a zimg view transform in the decode filter shared by preview, thumbnails, proxies and render; BT.2020 SDR is converted; Project Media badges "HDR". `hdr_view_transform_e2e`: PQ and HLG clips within 2 levels of FFmpeg's own tone map and within 5 % of the SDR original on the greys; SDR decodes bit-identically with the transform on or off. R40b: HDR10 and HLG projects (ACES view transforms) and render templates. |
| 7.59 | Variable frame rate media | DONE | `avg_frame_rate` rule plus the `fps` filter conform (`media.rs:166-171`, `vdecode.rs:48`) |
| 7.60 | Interlaced video | DONE | `field_order` probe plus yadif (`media.rs:176`, `vdecode.rs:49`) |
| 7.61 | Multichannel audio (media) | DONE | R3b: Stream ▸ plays any audio (or video) stream of a multi-stream file (`Clip.stream`, `-map 0:a:N` in `engine/mix.rs`, `0:v:N` in `engine/vdecode.rs`; streams counted at import, `media.rs`); Channels ▸ Both / Left Only / Right Only / Combine / Swap (`mix::map_channels`). R10a: a 5.1 / 7.1 track plays every channel of the media. R10b: Channels ▸ picks a stereo pair of a multichannel stream (or its downmix), Stream ▸ Add All Streams as Tracks, a drop of multi-stream media says how many streams it has, Event Properties › Media shows the streams and channels. Tests `a_three_stream_mov_adds_three_audio_events_when_asked`, `ffmpeg_second_stream_and_peak` |
| 7.62 | Professional camera formats | DONE | R41: File › Import › Camera Card Folder (or a dropped folder) finds AVCHD, XDCAM EX/HD, XAVC, P2, Canon XF and GoPro clips (`camera.rs`); spanned AVCHD parts and GoPro chapters join into one item through a `.ffconcat` list; transport streams and lists seek exactly (`plan::PREROLL`); camera make/model, reel and timecode (tags, tmcd, MXF, H.264 SEI) in Media Properties, "Use Media Timecode as Ruler Start". Checked on FFmpeg-made stand-ins only (`camera_formats_e2e`): a spanned AVCHD pair imports as one 4 s item that plays its second part, XAVC-style MP4, XDCAM HD422 / DNxHD / AVC-Intra MXF, a P2-style OP-Atom card. `docs/COMPATIBILITY.md` camera section. |
| 7.63 | Format compatibility system; every supported format tested | DONE | R37: capability table (`engine/caps.rs`), the generated legal corpus (`tools/make-test-media.ps1`, `format_corpus_e2e`: 35 files over the codecs, containers, rates, sizes and channel layouts of the spec), the decode matrix in `docs/FORMAT_SUPPORT.md`, and the verdict (decode, proxy, render as same) in Media Properties. |
| 7.64 | Limitations, external dependencies and licensing documented | DONE | R37: limitations per format and a licence column in `docs/FORMAT_SUPPORT.md`; RAW and CinemaDNG reasons (R41). (R56) Licensing and external dependencies: [`docs/LICENSES.md`](LICENSES.md). |
| 7.65 | Never silently substitute or misinterpret an unsupported codec | DONE | R37: a codec the bundled FFmpeg cannot decode imports as an offline item that says so (`caps::unsupported`, "!" badge, Media Properties), never decoded as something else; R40a: HDR is no longer misinterpreted. (R10b) Extra audio streams are announced on a drop and can be added as tracks; extra channels play on a 5.1 / 7.1 track or are picked by pair. |

### 2.8 Section 8: VEGAS project import

| # | Requirement | Status | Evidence / what is missing |
|---|---|---|---|
| 8.1 | Native `.veg` projects | NOT FEASIBLE | Undocumented, version-specific binary chunk format with no public parser (`FEATURE_PLAN.md:1811`). Deliverables, all done: the script route both ways (`tools/vegas/Export to Milan Pro.cs` R25, `Import from Milan Pro.cs` R26), the `.veg` media scan (`interchange/vegas.rs::scan_veg`) and the research note `docs/research/VEG_FORMAT.md` (R27: no sample `.veg` exists on this machine; the recommendation is the script route). |
| 8.2 | Project metadata | DONE | `VegasExport.name`, `project` (`interchange.rs:13-37`) |
| 8.3 | Media references | DONE | Including offline items with warnings (`interchange.rs:226-236`) |
| 8.4 | Track organization | DONE | |
| 8.5 | Video events | DONE | |
| 8.6 | Audio events | DONE | |
| 8.7 | Timeline positions | DONE | |
| 8.8 | Event lengths | DONE | |
| 8.9 | Playback rates | DONE | |
| 8.10 | Transitions | DONE | R25: `Fade.Transition` on both fades with every OFX parameter; Dissolve, Linear Wipe, Clock Wipe, Iris, Push, Slide, Zoom and Flash map (`interchange/vegas_map.rs`), the rest play as a crossfade and are reported. Test `v2_transitions_generators_and_envelopes`. |
| 8.11 | Effects | DONE | R25: event, track, media and video output FX with `PlugIn.UniqueID`, bypass and every OFX parameter with keyframes; 17 VEGAS plug-ins map onto built-ins with per-parameter scale/offset/choice tables, the rest are disabled `ofx:<id>@1` placeholders that keep every parameter (the OFX host plays them once installed) and are listed in the report. Audio FX by name (Track EQ, Compressor, Noise Gate, Reverb, delays, Wave Hammer, ExpressFX). Tests `v2_fx_map_or_become_placeholders`, `every_row_lands_on_a_built_in_parameter`. |
| 8.12 | Track properties | DONE | R25: track FX, Track Motion keyframes (position, size, rotation, Z and X/Y rotation), 2D Shadow / Glow on, parent/child compositing with Parent Composite Mode and Parent Motion. Track colour is not in VEGAS's scripting API. Test `v2_track_motion_compositing_settings_and_audio_fx`. |
| 8.13 | Compositing | DONE | R7: every VEGAS mode name maps (`interchange::vegas::composite`) but Custom, whose compositor the export does not carry (reported). R25: 3D Track Motion values and parent/child nesting come across. |
| 8.14 | Keyframes | DONE | R25: Pan/Crop, FX parameter (with VEGAS's interpolation names) and Track Motion keyframes. |
| 8.15 | Envelopes | DONE | Track envelopes come across, including (R4a) video track Mute and Fade to Color (`interchange::vegas::envelope`); R25: event Velocity envelopes (a 0 % hold is a freeze frame); other event envelopes (Transition Progress) are reported. |
| 8.16 | Markers | DONE | |
| 8.17 | Regions | DONE | |
| 8.18 | Project settings | DONE | R25: field order, pixel aspect ratio, rendering quality, motion blur type, deinterlace method, bit depth; master bus mode is read and reported when Milan Pro cannot follow it; R40b: the pixel format (VEGAS's 32-bit floating point video levels or full range) is imported and exported. |
| 8.19 | Audio configuration | DONE | Sample rate, bit depth (R25), the number of busses (R8a); R10a: Master bus mode Stereo / 5.1 Surround (VEGAS) / 7.1 Surround, Resample and stretch quality (Preview / Good / Best: 16 / 32 / 64 taps), each track's and bus's channel layout |

### 2.9 Section 8b: export and interchange

| # | Requirement | Status | Evidence / what is missing |
|---|---|---|---|
| 9.1 | Investigate reliable VEGAS-compatible project export | DONE | R26: File › Export › VEGAS Project (via script)... writes `<name>.milan.json` (`interchange/vegas_export.rs`, `vegas_map` in reverse); `tools/vegas/Import from Milan Pro.cs` (and the VEGAS 13 copy) rebuilds it through the scripting API (new or open project, settings, media, tracks, events, reversed subclips, takes, fades, Pan/Crop, Track Motion, envelopes, groups, FX with parameters and keyframes, transitions, generated media, audio FX by name, markers, regions, selection). Both scripts compile against VEGAS 15's `ScriptPortal.Vegas.dll`; the script's JSON reader was run on Milan Pro's output. Round trip `round_trip_through_the_vegas_json`. Not run inside VEGAS here (manual: user). |
| 9.2 | Never write fake `.veg` files | DONE | Nothing writes `.veg` |
| 9.3 | OpenTimelineIO | DONE | R28: `interchange/otio.rs`, File › Import / Export › OpenTimelineIO. Milan's structs under `metadata["milan"]`: `lossless_round_trip` gives the same project JSON back. Hand-written fixtures `tests/fixtures/simple_cut.otio`, `nested_example.otio` (nested stacks flatten). |
| 9.4 | EDL | DONE | R28: `interchange/edl.rs`: CMX 3600 (`.edl`: V, A-A4, cuts, dissolves, M2 speed, reel names, DF/NDF, `* FROM CLIP NAME` / `* SOURCE FILE`) and VEGAS's own EDL text table (`.txt`), both ways; golden files `tests/fixtures/golden_25.edl`, `golden_2997df.edl`. |
| 9.5 | XML editing timelines (FCP7 xmeml, FCPXML) | DONE | R29: `interchange/xmeml.rs` (xmeml 5: Premiere, Resolve; stereo pairs, Time Remap, Opacity, Audio Levels, links, markers) and `interchange/fcpxml.rs` (1.10: spine, lanes, connected storylines, time maps, adjust-blend/volume, markers), both ways; the Premiere rows point at FCP7 XML. Hand-written fixtures `tests/fixtures/resolve_sample.xml`, `fcpx_sample.fcpxml`. |
| 9.6 | AAF where practical | NOT FEASIBLE | No maintained Rust AAF library. The reference AAF SDK is large C++/COM structured storage. R29: the route through OTIO's AAF adapter (`otioconvert`, Python, external) is in `docs/INTERCHANGE.md`; the AAF menu row's hover text names it. |
| 9.7 | Reliable import/export for suitable formats | DONE | R28/R29: OTIO, CMX 3600 and VEGAS EDL, FCP7 XML, FCPXML, each with a round-trip test and a hand-written fixture; none opened in the other applications here (manual: user). |
| 9.8 | Compatibility report: accurate / needs conversion / not representable | DONE | R29: the summary table and per-format tables in `docs/INTERCHANGE.md`; `docs/COMPATIBILITY.md` lists what each format was tested against. |

### 2.10 Section 9: VEGAS plug-in compatibility

| # | Requirement | Status | Evidence / what is missing |
|---|---|---|---|
| 10.1 | Inventory video effect plug-ins | DONE | (R52) [`docs/REFERENCE_ANALYSIS.md`](REFERENCE_ANALYSIS.md) section 6.1, and `docs/compat/ofx-vegas15.md` per plug-in. |
| 10.2 | Inventory audio effect plug-ins | DONE | (R52) [`docs/REFERENCE_ANALYSIS.md`](REFERENCE_ANALYSIS.md) sections 1.2 and 6.3, and `docs/compat/dx-audio.md`. |
| 10.3 | Inventory transitions | DONE | (R52) [`docs/REFERENCE_ANALYSIS.md`](REFERENCE_ANALYSIS.md) section 6.1 (the transitions bundle). |
| 10.4 | Inventory generators | DONE | (R52) [`docs/REFERENCE_ANALYSIS.md`](REFERENCE_ANALYSIS.md) section 6.1 (the generators). |
| 10.5 | Third-party plug-in formats | DONE | (R52) [`docs/REFERENCE_ANALYSIS.md`](REFERENCE_ANALYSIS.md) sections 6.1-6.5 (OFX: HitFilm; VST folders empty). |
| 10.6 | Plug-in SDKs | DONE | (R52) [`docs/REFERENCE_ANALYSIS.md`](REFERENCE_ANALYSIS.md) section 7 (the .NET script API, the FileIO SDK, the OFX SDK, external control). |
| 10.7 | Documented host interfaces | DONE | (R52) [`docs/REFERENCE_ANALYSIS.md`](REFERENCE_ANALYSIS.md) sections 6 and 7. |
| 10.8 | Plug-in dependencies | DONE | (R52, R33c) [`docs/REFERENCE_ANALYSIS.md`](REFERENCE_ANALYSIS.md) section 6.1 (proDAD Mercalli for Stabilize, the HitFilm licence folder, VEGAS's private suites), tested in `docs/compat/ofx-vegas15.md`. |
| 10.9 | Research the actual plug-in standards VEGAS 15 uses | DONE | (R52) [`docs/REFERENCE_ANALYSIS.md`](REFERENCE_ANALYSIS.md) section 6; `FEATURE_PLAN.md:1813` corrected. |
| 10.10 | OpenFX | MISSING | → R33a-c |
| 10.11 | VST2 | NOT FEASIBLE | Steinberg stopped licensing the VST2 SDK (2018); new hosts cannot get a licence, and clean-room headers carry legal risk. Deliverable: stated in COMPATIBILITY.md with the VST3/CLAP alternative (R55). |
| 10.12 | VST3 | DONE | (R34a-b) `engine/aplug`, `src/bin/milan-plugin-host/vst3.rs`: scan (IPluginFactory, moduleinfo.json cross-check), IComponent/IAudioProcessor at 48 kHz float with 1024-frame blocks, IEditController parameters as `ParamSpec`s with keyframes and automation, program lists, latency → PDC in the mixer, tail, IPlugView editor windows, `.vstpreset`. Tests `aplug::tests::*`. |
| 10.13 | Other documented standards (CLAP, DirectX/DMO) | DONE | (R35) CLAP effects through the same host (`clap.rs`: params, state, latency, tail, process; no editor yet). DX/DMO: investigated in the isolated host and not pursued (`docs/compat/dx-audio.md`). |
| 10.14 | Modular plug-in hosting architecture | PARTIAL | One registry for built-ins (`engine/plugins.rs`). `Fx.plugin` ids are open to an `ofx:` prefix. No external sources. → R31, R32 |
| 10.15 | Plug-in scanning | MISSING | → R31 |
| 10.16 | Plug-in discovery | MISSING | → R31 |
| 10.17 | Plug-in categorization | PARTIAL | Built-ins have categories (`choosers.rs`). → R31 |
| 10.18 | Plug-in search | PARTIAL | Built-ins only. → R31 |
| 10.19 | Plug-in enable/disable | MISSING | → R31 |
| 10.20 | Plug-in compatibility reporting | MISSING | → R31, R55 |
| 10.21 | Plug-in error handling | MISSING | No external plug-ins to fail yet. → R32 |
| 10.22 | Plug-in presets | PARTIAL | Video FX presets work; (R8c) audio FX presets too: built-in ones for each audio plug-in and user presets saved and loaded in the FX window (test `audio_preset_round_trips`); (R34b) `.vstpreset` load and save for VST3 effects (`aplug::preset`, test `preset_round_trip_restores_parameters`). OFX presets are missing. |
| 10.23 | Effect chains | DONE | `Vec<Fx>` on event, track, output, master and media; FX window (`ui/fx_window.rs`) |
| 10.24 | Plug-in UI integration | PARTIAL | The generic parameter grid for every plug-in; (R34b) a VST3 plug-in's own editor (IPlugView) in a host-owned window from the FX window's Show plug-in UI, resizable, its edits written to the project (automation while recording). CLAP editors are left. |
| 10.25 | Compatibility matrix (native / standard / adapter / unsupported / investigate) | MISSING | → R55 |
| 10.26 | No fake compatibility claims | DONE | Nothing claims external plug-in support |
| 10.27 | A plug-in crash must not bring down the editor | PARTIAL | FFmpeg runs out of process (`proc.rs` Job Object). A plug-in host process is still to be built. → R32 |

### 2.11 Section 10: effects, transitions and compositing

| # | Requirement | Status | Evidence / what is missing |
|---|---|---|---|
| 11.1 | Video effects | DONE | 15 built-in FX (`engine/vfx.rs`, `plugins.rs`) |
| 11.2 | Audio effects | DONE | EQ, compressor, gate, limiter, reverb, delay (`engine/afx.rs`) |
| 11.3 | Color correction | DONE | `milan.fx.color_corrector`, Levels, HSL |
| 11.4 | Color grading | DONE | `ui/color_grading.rs` (lift/gamma/gain, curves, .cube LUTs); (R16b) the Video Scopes window (`engine/scopes.rs`, `ui/scopes.rs`: VEGAS's Vectorscope with the 75 % targets, Waveform (Luminance / Composite, IRE), Histogram (Luminance, R, G, B, Alpha, Luminance/R/G/B, Lightness/a/b, with Mean and Std Dev), RGB Parade and their combinations; Settings 7.5 IRE Setup and Studio RGB; Update Scopes while Playing) and, in the Video Preview, Split-Screen View (FX Bypassed / Clipboard, a drawn or half region) and the Isolate channel overlays. Tests: `white_peaks_at_100_ire_and_255`, `colour_bars_land_on_the_vectorscope_targets`, `histogram_counts`, `throttle_allows_ten_a_second`, `split_regions_follow_the_rotation`, `fx_bypassed_plan_has_no_video_fx`. |
| 11.5 | Brightness and contrast | DONE | `milan.fx.brightness_contrast` |
| 11.6 | Saturation | DONE | HSL FX |
| 11.7 | Exposure | DONE | R7: Exposure FX, stops in linear light plus offset and gamma (`milan.fx.exposure`; +1 doubles linear values, tested). |
| 11.8 | Curves | DONE | Color Grading curves |
| 11.9 | Levels | DONE | `milan.fx.levels` |
| 11.10 | Sharpening | DONE | `milan.fx.sharpen` |
| 11.11 | Blur | DONE | `milan.fx.gaussian_blur` |
| 11.12 | Chroma key | DONE | `milan.fx.chroma_key` |
| 11.13 | Green screen | DONE | Chroma Keyer with Show mask only (matte view) and, R7, Spill suppression (`vfx::despill`; Milan Pro's own, 0 = off by default as VEGAS's keyer has none). |
| 11.14 | Masks | DONE | R7: Event Pan/Crop Mask (keyframed Bezier paths, Positive / Negative / Disabled, opacity, feather; `engine/raster.rs`, `ui/pancrop.rs`), Bezier Masking FX and Cookie Cutter FX. VEGAS's In/Out feather types are not built (feather is centred on the edge). |
| 11.15 | Transform controls | DONE | Pan/Crop, Track Motion (`ui/pancrop.rs`, `ui/trackmotion.rs`) |
| 11.16 | Crop | DONE | `milan.fx.crop`, Pan/Crop |
| 11.17 | Pan and zoom | DONE | Pan/Crop keyframes |
| 11.18 | Opacity | DONE | |
| 11.19 | Blending modes | DONE | R7: VEGAS's 15 plus Custom; 7 extra W3C modes behind a preference (off). |
| 11.20 | Compositing | DONE | `engine/composite.rs` |
| 11.21 | Motion effects | DONE | R7: Track Motion 2D Shadow and 2D Glow, 3D Source Alpha (Z position, X / Y rotation; camera one frame width away, an estimate), Parent Motion, Video Bus Track Motion Blur Amount and Video Supersampling envelopes with Project Properties › Motion blur type, Bypass Motion Blur (`plan::blur_samples`, `worker::frame`). |
| 11.22 | Keyframe animation | DONE | `engine/keyframes.rs`, `ui/keyframes.rs` |
| 11.23 | Transitions | DONE | 8 transitions (`milan.tr.*`) |
| 11.24 | Generators | DONE | Solid, gradient, checkerboard, noise and text; Credit Roll and Test Pattern (R18a, `engine/generate.rs`). |
| 11.25 | Non-destructive | DONE | |
| 11.26 | Applied to clips, tracks and project | DONE | Event, track and Video Output FX. Media FX (FX on a file in Project Media) is greyed (`media_panel.rs:174`). → R57 |
| 11.27 | Parameter keyframes | DONE | |
| 11.28 | Editable effect chains | DONE | |

### 2.12 Section 11: audio

| # | Requirement | Status | Evidence / what is missing |
|---|---|---|---|
| 12.1 | Multitrack audio editing | DONE | |
| 12.2 | Accurate waveform visualization | DONE | `audio.rs` Waveforms; View › Rebuild Audio Peaks (R18a) |
| 12.3 | Volume envelopes | DONE | |
| 12.4 | Pan envelopes | DONE | |
| 12.5 | Automation | DONE | See 5.15 (R8c) |
| 12.6 | Fade curves | DONE | Linear, fast, slow, smooth, sharp, hold |
| 12.7 | Audio routing | DONE | See 5.18 (R8a) |
| 12.8 | Master bus | DONE | `ui/master_bus.rs`, master FX |
| 12.9 | Track buses | DONE | (R8a) Insert › Audio Bus, Insert › Audio Assignable FX... (opens the chain's FX window), Tools › Audio › Remove Assignable FX...; bus FX, volume, pan, mute and Volume/Pan/Mute envelopes play (`ABus`). (R8b) A bus's fader, pan, mute, FX and output on its Mixing Console strip (`edit::set_bus_out` refuses loops), and View › Audio Bus Tracks (B): a lane per bus and chain with its Volume/Pan/Mute envelopes and Mute/Vol/Pan/FX/meter in its header (`ui/timeline/bus.rs`). No bus Solo and no Master bus track yet. |
| 12.10 | Audio meters | DONE | (R8b) Track header meters show each track's post-fader level (L/R, 2 s peak hold, clip lamps, VP16 peak readout; Show Output Meters, Use Vertical Meters, Channel Width), as do the bus track headers, the console strips and the Master Bus window, all from one per-frame fold of the lock-free `mix::Meter`s (`mixer::feed_levels`). (R16c) The Loudness Meters window (`ui/loudness.rs`, `engine/loudness.rs`): ITU-R BS.1770-4 / EBU R 128 momentary, short-term and integrated loudness, loudness range and 4x true peak, with Options › Loudness Meters, Tools › Generate Loudness Log and Render Options › Save loudness log next to media file (tests `tech3341_sines_read_their_level`, `tech3341_gating_cases`, `tech3342_loudness_range`, `true_peak_finds_inter_sample_peaks`). |
| 12.11 | Peak meters | DONE | `master_bus.rs` (peak hold, clip lamps) |
| 12.12 | VU-style meters | DONE | (R8b) The Mixing Console's VU Meters region (off by default, as VEGAS shows peak meters): 300 ms ballistics, 0 VU = -20 dBFS (`MeterView::vu_db`, test `vu_step_reaches_99_percent_in_300_ms`). |
| 12.13 | Multichannel audio | DONE | R10a: multichannel tracks, busses, master, renders and loudness (BS.1770 channel weights); R10b: Surround Panner and per-channel meters |
| 12.14 | Sample-rate conversion | DONE | FFmpeg `-ar` in decode (`mix.rs:70`); render at project rate |
| 12.15 | Audio effects chains | DONE | |
| 12.16 | Real-time audio processing | DONE | Block mixer with FX and live plan swaps |
| 12.17 | Low-latency monitoring | DONE | (R9) Input Monitoring Off / On / Auto per armed track (track header › Input, console Record Input) and Insert › Audio Input Bus: the input goes through a lock-free ring into the output callback, the track's or bus's FX, volume, pan and routing, then the master (`audio::MonPlan`, `MonState`; tests `the_monitor_plays_armed_tracks_and_input_busses`, `the_input_is_heard_through_the_output_callback`). The round trip is printed in Preferences › Audio Device. |
| 12.18 | VST hosting investigated | DONE | Built (R34a-b): see 10.12. |
| 12.19 | Audio-device compatibility | DONE | (R9) Preferences › Audio Device: Microsoft Sound Mapper, Windows Classic Wave Driver (a chosen device) and Windows Audio (Exclusive Mode, Milan Pro's, off by default, `wasapi.rs`), buffer size, recording device; device loss reopens (watchdog). ASIO is not feasible (section 6; `docs/COMPATIBILITY.md`). |
| 12.20 | Precise A/V sync through playback and render | DONE | Same mixer for both; render audio pass (`engine/render.rs`) |

### 2.13 Section 12: rendering and export

| # | Requirement | Status | Evidence / what is missing |
|---|---|---|---|
| 13.1 | Dedicated rendering engine | DONE | `engine/render.rs` (same plan, compositor and mixer as preview) |
| 13.2 | Render presets | DONE | R11: `render::Template` (serde), 38 built-ins in `render::BUILTIN`, user templates in the preferences; `encoder_argv_for_every_template` |
| 13.3 | Custom resolutions | DONE | R11: Customize Template › Frame size (the project's or any 16-8192), letterboxed or stretched (`render::fit`), turned by Output rotation; `interlace_rotation_and_letterbox_filters`; letterbox, stretch and rotation checked in `render_templates_e2e` |
| 13.4 | Custom frame rates | DONE | R11: the plan is compiled at the template's rate (`engine/render.rs::encode`); interlaced output composes one frame per field and weaves them; a 30p project rendered at 29.97i in `render_templates_e2e` |
| 13.5 | Custom bitrates | DONE | R11: constant quality / VBR (average, maximum) / CBR per codec (`Template::video_args`); audio bit rates per codec |
| 13.6 | Audio export configuration | DONE | R11: audio codec, bit rate, sample rate (the project's or fixed, raised or capped to what the codec takes: `pick_rate`), mono or stereo, bit depth (`Template::audio_args`). Multichannel → 13.11 |
| 13.7 | Codec selection | DONE | R11: Customize Template › Format and Codec (`render::CONTAINERS`, `VCODECS`, `ACODECS`); combinations a format cannot hold are refused (`Template::check`: VP9 in MOV) |
| 13.8 | Hardware encoding | DONE | (R22) `VCODECS` holds the 12 device encoders and three `<codec>_hw` placeholders; the built-in "MP4 H.264/HEVC/AV1 (hardware)" templates render on the encode device that passed, else in software with a note (`Template::resolved`); ignored `render_with_hardware_encoder` renders through h264_amf here. |
| 13.9 | Software encoding | DONE | libx264, libx265, prores_ks, pcm, libmp3lame |
| 13.10 | Quality settings | DONE | R11: Project Properties › Full-resolution rendering quality (Good for a new project, as VEGAS) and the template's Video rendering quality; encoder speed presets; renders never decode proxies (`VideoPlan::without_proxies`) |
| 13.11 | Multi-channel audio export | DONE | R10a: 5.1 / 7.1 templates (WAV, FLAC, AAC, AC-3, E-AC-3, MP4 H.264 with AAC 5.1, MPEG-2 TS with AC-3 5.1) and Customize Template's 5.1 / 7.1 channels for the codecs that hold them (`render::max_channels`); Enable multichannel mapping puts a stereo project's master on channels 1-2 and each bus that goes to the master on the next pair |
| 13.12 | Batch rendering | DONE | R12: Tools › Scripting › Batch Render (VEGAS's `Batch Render.cs`: per ticked template, over the project, the loop region or each region, `<base><format>_<template>[n].<ext>`) and Render Audio Tracks (`Render Audio Tracks.cs`: each audio track soloed, `track NN (name).<ext>`), queued (`render::batch`, `render::per_track`, `ui/render_queue.rs`); `batch_render_jobs_and_names` |
| 13.13 | Render queue | DONE | R12: `render::RenderQueue`, Tools › Render Queue... (move, pause / resume a waiting job, cancel, retry, remove, clear finished, show log), one render at a time in list order, kept in `%APPDATA%\MilanPro\queue.json` with a project snapshot per job; `queue_persists_and_marks_interrupted_jobs`, ignored `render_queue_e2e` (two renders checked with ffprobe, a cancelled queued job never rendered) |
| 13.14 | Render progress | DONE | `dialogs/render.rs:170` |
| 13.15 | Estimated completion | DONE | `j.eta()` |
| 13.16 | Cancellation | DONE | `j.cancel()`; `.partial` deleted |
| 13.17 | Failure recovery | DONE | A failed render cleans up its temp files and shows the FFmpeg error tail. R12: Retry re-renders a failed, cancelled or interrupted job from its snapshot; a job rendering when Milan Pro crashed comes back Interrupted, waiting jobs resume at the next start (`RenderQueue::load`) |
| 13.18 | Export logs | DONE | R11: every render writes `%LOCALAPPDATA%\MilanPro\logs\render\<date>-<time>-<n> <file>.log` (command, FFmpeg version, timings, frames, status, all FFmpeg output; `engine/render.rs::write_log`); Show log in the dialog |
| 13.19 | Configurable presets for common delivery formats | DONE | R11: YouTube 1080p/2160p, Vimeo, Web 720p, Social Vertical, ProRes 422 HQ/4444 (alpha), DNxHR LB/SQ/HQ/HQX/444, CineForm, FFV1 MKV archive, VP9/VP8 WebM, AV1, MPEG-2 TS/PS, AVI MJPEG, PNG/TIFF/EXR/DPX sequences, FLAC, ALAC, AAC, Opus, Vorbis, AC-3, E-AC-3, AIFF; user templates saved and deleted in Customize Template |
| 13.20 | Background rendering while editing continues | DONE | R12: Render Options › Render in the background (off by default, as VEGAS's progress dialog is modal) and Add to Render Queue: the dialog closes, the status bar shows the progress with Cancel, and the render is of the snapshot taken when it was queued, so edits never reach it (`queue_persists_and_marks_interrupted_jobs`, `render_queue_e2e`) |
| 13.21 | Renderer reproduces timeline FX, transitions, compositing, audio | DONE | One `plan::compile` for preview and render |

### 2.14 Section 13: media management

| # | Requirement | Status | Evidence / what is missing |
|---|---|---|---|
| 14.1 | Media browser | DONE | `ui/explorer.rs`. Its preview buttons are inert with labels (`explorer.rs:168-171`) → R57 |
| 14.2 | Project media bin | DONE | `ui/media_panel.rs` |
| 14.3 | Folder organization | DONE | R45a: Custom Bins (nested; drag media or bins onto a bin; an item in several bins; Remove from Current Bin) and Smart Bins (saved queries) in `Project::bins` (`library.rs`), with VEGAS's bin tree menu (Create New Bin, Save as Bin, Save as Smart Bin, Delete, Rename, Edit Search). Undoable. |
| 14.4 | Tags | DONE | R45a: `MediaItem::tags`, edited with the item menu's Media Tags...; Tagged Media lists every tag with its count; the list background's Media Tags shows them under the tiles; a Tags column in Details. |
| 14.5 | Search | DONE | R45a: the search field takes a query over name, path, codec, tags and frame size, with `type:`, `codec:`, `tag:`, `res>=`, `fps:`, `length<`, `is:used/unused/offline` and `-` (`library::Query`); Search Media Bins... builds one from criteria rows. |
| 14.6 | Metadata inspection | DONE | R45b: File Format Properties (see 4.9). |
| 14.7 | Thumbnail generation | DONE | |
| 14.8 | Waveform generation | DONE | |
| 14.9 | Proxy generation | DONE | `proxy.rs` |
| 14.10 | Relinking missing media | DONE | Per-item Replace..., the automatic search relative to the project and next to it on Open, and R45b's Media Offline dialog: Search Folder... (recursive, by name and size), per-item Browse..., Relink all found as one undo step, or Leave Offline (`ui/dialogs/missing_media.rs`, `library::{find_in_folder, relink}`). |
| 14.11 | Duplicate media detection | DONE | Same path, and R45b: on Import Media the worker compares each new file with the pool by size, then SHA-256 of its size and first and last 4 MB (`library::fingerprint`); a match asks "Use Existing / Add Anyway". |
| 14.12 | Media properties | DONE | See 4.9 |
| 14.13 | Import progress | DONE | R45b: "Importing 3 of 40" in the status bar with Cancel (the files read so far are added), for imports and drops (`media::Progress`). |
| 14.14 | Background media analysis | PARTIAL | Probe, thumbnails, peaks and (R45b) the duplicate fingerprint on the import worker. No loudness or scene analysis. → R57 |
| 14.15 | Large libraries without slowing the editor | DONE | R45a: the shown items are evaluated only when the history, pool, node or search change, and the list, details and thumbnail views draw only the rows on screen. Bench `large_library_frame_time_stays_under_4ms` (10,000 items). |
| 14.16 | Drag-and-drop importing | DONE | From Windows, Explorer and Project Media (`app/drop.rs`) |

### 2.15 Section 14: reliability

| # | Requirement | Status | Evidence / what is missing |
|---|---|---|---|
| 15.1 | Automatic project saving | DONE | `autosave.rs` (backup copy in `%APPDATA%\MilanPro\autosave`, atomic) |
| 15.2 | Configurable autosave interval | DONE | R43: Preferences › General "Save a backup copy of the project every N minutes" (off, or 1-60; VEGAS's default of 2) (`Prefs.autosave_on/autosave_minutes`) |
| 15.3 | Project backups | DONE | R42: Save keeps the file it writes over as `<name>.mnle.bak` (`project::backup_path`, as VEGAS keeps `.veg.bak`). R43: "Backup copies kept per project" 1-20 (default 5): `<name>-autosave.mnle`, `.1.mnle`, `.2.mnle`, ... (`autosave::rotate`); the parents waiting under a nested timeline are autosaved too |
| 15.4 | Recovery after crashes | DONE | R43: `%APPDATA%\MilanPro\sessions\<pid>.json` lists the open projects and their autosaves, removed on a clean exit; at start, markers of pids no longer running (`OpenProcess`) whose autosaves are newer than their projects open the Recover Projects dialog: Open (as an unsaved copy), Discard, Keep for later (`autosave::{stale_sessions, recoverable}`, `dialogs::Recover`) |
| 15.5 | Undo/redo history | DONE | `history.rs`, history list on the Undo split button |
| 15.6 | Non-destructive editing | DONE | |
| 15.7 | Project versioning | DONE | R42: File › Save Version (Ctrl+Alt+S, as VEGAS 23) saves `name_01.mnle`, `name_02.mnle`, ... beside the file with an optional note (`Project.version_note`) and goes on editing in the new file (`dialogs/save_version.rs`) |
| 15.8 | Missing-media detection | DONE | On Open a missing file marks its item offline (`MediaItem::missing`, never stored), the status bar says so and the Media Offline dialog opens; offline events are drawn hatched with "Media Offline", offline tiles likewise with a red "!". |
| 15.9 | Media relinking | DONE | See 14.10 |
| 15.10 | Corrupt project detection | DONE | R42: a file that does not parse offers its `.mnle.bak` and latest autosave, opened as an unsaved copy (`project::CorruptOffer`, `dialogs::corrupt_window`); one that parses goes through `project/validate.rs` (duplicate or unknown ids, NaN/negative/infinite times, duplicate takes, bad settings): repaired where safe, every finding listed in the Open Project report, nothing written until the user saves |
| 15.11 | Error logging | DONE | R44: `src/logger.rs`, Milan Pro's own file logger behind the `log` crate: `%LOCALAPPDATA%\MilanPro\logs\milan-<date>.log`, 2 MB × 5 files, level in Preferences › Update; every former stderr print logs (grep test `logger::tests::no_eprintln_left_in_the_tree`); updater, autosave, render-queue, plug-in and open/save events are logged |
| 15.12 | Diagnostic reports | DONE | R44: Help › Create Diagnostic Report writes `%LOCALAPPDATA%\MilanPro\diagnostics\report-<time>\` with `system.txt` (Windows build, CPU, memory, GPUs and drivers through DXGI, displays), `ffmpeg.txt` (`-version`, `-hwaccels`), `prefs.json`, the logs and the crash files (`src/diagnostics.rs`, `dialogs/diagnostics.rs`); a panic writes `crashes\crash-<time>.txt` (version, backtrace, project summary, last 500 log lines) and a native crash a minidump beside it (`SetUnhandledExceptionFilter` → `MiniDumpWriteDump`); the next start offers Open crash report / Report on GitHub (the issue page with the version prefilled; nothing uploaded) |
| 15.13 | Proper project format | DONE | `.mnle` JSON (`project.rs`) |
| 15.14 | Stores structure, media, tracks, FX, transitions, keyframes, audio settings, workspace | DONE | All editing data is stored, (R8a) busses, assignable FX chains, routing, sends and Pan Type included (`Timeline.audio`, `Track.out/sends/pan_type`); (R10a) the master bus mode, resample quality, track and bus layouts and surround pans (`ProjectSettings.layout`, `Track.layout`, `Track.surround`, `Bus.layout`, skipped when stereo so old projects are written as before). The workspace layout lives in prefs, not the project (acceptable) |
| 15.15 | Versioned schema with safe migration | DONE | R42: `Project.format_version` (`FORMAT_VERSION` = 1; absent = 0), `project/migrate.rs` steps run on the JSON before deserializing (0→1 tested); a file from a newer build opens read-only ("(read-only)" in the title, Save becomes Save As), never down-saved in place |
| 15.16 | Never change a project destructively without preserving the original | DONE | R42: saves are atomic and keep the previous file as `.mnle.bak`; validator repairs stay in memory until a save; backups and autosaves open as unsaved copies. VEGAS import opens as a new unsaved project |

### 2.16 Section 15: updater

| # | Requirement | Status | Evidence / what is missing |
|---|---|---|---|
| 16.1 | GitHub Releases as source | DONE | `updater.rs:17` |
| 16.2 | Automatic update check | DONE | `app.rs:172-174` (5 s after start) |
| 16.3 | Manual update check | DONE | Help > Check for Updates |
| 16.4 | Display current version | DONE | `dialogs/update.rs:47` |
| 16.5 | Display available version | DONE | |
| 16.6 | Release notes | DONE | |
| 16.7 | Download progress | DONE | `State::Downloading` |
| 16.8 | Update verification | DONE | SHA-256 against the API digest or the `.sha256` asset (`updater.rs:72-83, 255-270`); re-hashed before launch (`updater.rs:278`) |
| 16.9 | Signed release artifacts where practical | PARTIAL | R46: `release.yml` signs the `.sha256` with minisign when the `MINISIGN_SECRET_KEY` secret exists and uploads the `.minisig`; the app verifies it with `updater::MINISIGN_PUBLIC_KEY` (empty until the maintainer generates the key pair, SECURITY.md; then SHA-256 only, said in the dialog). Authenticode: `signtool` steps gated on the certificate secrets; certificate pending (user action) |
| 16.10 | Integrity verification | DONE | |
| 16.11 | Safe installation | DONE | Silent Inno Setup waits on the `MilanProRunning` mutex; a failed or aborted Setup brings the old Milan Pro back (`DeinitializeSetup`). R46: the relaunch passes `--after-update` and a version that fails to start twice offers the roll back |
| 16.12 | Rollback / recovery from failed updates | DONE | R46: with "Keep the previous version's installer" on, the current version's installer is fetched from its own release before an update (verified like an update) into `%LOCALAPPDATA%\MilanPro\updates\previous\` with its `.sha256`; Help › Roll Back to Previous Version runs it silently (`updater::{keep_current, rollback_target}`); `updates\starting.json` counts the starts after an update and the third try offers the roll back (`start_marker`, cleared by the first frame) |
| 16.13 | Update settings | DONE | R46: Preferences › Update: channel, auto-check, skipped version with Clear, keep previous version; the log and crash-report options (R44) share the page |
| 16.14 | Stable and optional preview channels | DONE | R46: `updater::Channel`: Stable reads `/releases/latest`; Preview reads `/releases` and takes pre-releases and pre-release tags (`Version` orders them as semver does); `release.yml` publishes a `-` tag as a pre-release |
| 16.15 | Updates cannot corrupt an open project | DONE | Install goes through the save prompt (`commands.rs:1345-1349`) |
| 16.16 | Updates cannot interrupt an ongoing export | DONE | R12: Install and restart (and Exit, and closing the window) while a render runs or waits asks first (`ui/render_queue.rs::guard`): an update only waits for the queue to finish or is not installed now; Exit may also cancel the renders or keep them for the next start (`exit_and_update_guards`). R46's recording guard is separate |
| 16.17 | Separate, maintainable component | PARTIAL | A self-contained module, `src/updater.rs`, not a crate. → R50 |
| 16.18 | Never install unverified packages | PARTIAL | Hash-verified always; authenticated by minisign once the key is compiled in (16.9). A release whose signature is missing or wrong is then refused, and the API digest is no longer trusted alone |
| 16.19 | Best implementation researched for this framework and deployment | DONE | `README.md` "Updates", `FEATURE_PLAN.md` batch 18 |

### 2.17 Section 16: performance and hardware

| # | Requirement | Status | Evidence / what is missing |
|---|---|---|---|
| 17.1 | CPU thread scheduling | DONE | Row stripes at clamp(cores/2, 1, 6) (`FEATURE_PLAN.md` 1.3), `-threads 2` per decoder. R12: a render's encoder runs at BELOW_NORMAL priority (`proc::below_normal`, checked in `render_queue_e2e`). R19: the render thread and its compose stripes run below normal too (`proc::below_normal_thread`, `composite::par_rows`; test `render_threads_run_below_normal`), so the preview and UI come first |
| 17.2 | GPU acceleration | DONE | (R21a) Preferences › Video › "GPU acceleration of video processing" as VEGAS has it (Off or an adapter), plus Automatic (the best Vulkan adapter, the default). The page says what composites now. |
| 17.3 | Hardware video decoding | DONE | (R22) See 3.6. |
| 17.4 | Hardware encoding | DONE | (R22) See 13.8. |
| 17.5 | GPU compositing | DONE | (R21a-c) See 3.4 and 6.5. |
| 17.6 | Memory management | DONE | Byte-capped LRU caches (`engine/cache.rs`), decoder cap of 8 |
| 17.7 | Cache management | DONE | (R23) Preferences › Video: Dynamic RAM Preview max (MB), the disk frame cache's size with Clear, the prerendered files folder; Clear Dynamic RAM Preview and Clean Up Prerendered Video (all or inactive files). The source and composite caches keep their fixed sizes (192 and 256 MB). |
| 17.8 | SSD storage (cache/prerender location) | DONE | (R23) The prerendered files folder is a preference (each project gets a folder of its own inside it; Project Properties shows the project's), the disk frame cache lives under `%LOCALAPPDATA%\MilanPro\framecache`. (R49) The temporary files folder is a preference too (TMP and TEMP for Milan Pro and FFmpeg, from the next start), with the free space shown. |
| 17.9 | High-resolution displays | PARTIAL | Preview Auto size fits the window. Not verified at 4K/200%. → R48 |
| 17.10 | Multiple monitors | DONE | R14: floating windows and the external preview on any display (see 4.28) |
| 17.11 | High-refresh-rate displays | DONE | (R48) Paused, nothing repaints unasked: the window title was sent every frame, and every viewport command asks for another frame, so the idle window painted at the display's refresh rate (144 frames a second at 144 Hz); now it is sent when it changes. The harness's `MILAN_IDLE_CHECK` counts the frames of 2 idle seconds (test `paused_preview_paints_no_extra_frames`). Playback still paints each display refresh, as it must to move the cursor. |
| 17.12 | High-DPI scaling | DONE | (R48) See 4.27. |
| 17.13 | Direct3D / Vulkan / vendor acceleration paths | DONE | (R22) d3d11va, cuda and qsv are tested and chosen explicitly (Preferences › Video); `auto` only until the table is built. |
| 17.14 | No assumption that every machine has the same acceleration | DONE | `hw_failed` fallback (`FEATURE_PLAN.md` 1.3 vdecode) |
| 17.15 | Fallback paths for unsupported hardware | DONE | CPU compositor; software decode fallback |
| 17.16 | Usable without a powerful GPU | DONE | Everything is CPU today |

### 2.18 Sections 17 and 18: architecture, standards and testing

| # | Requirement | Status | Evidence / what is missing |
|---|---|---|---|
| 18.1 | Modular application with separated concerns | PARTIAL | UI, timeline (`edit.rs`), media (`media.rs`), decode/encode (`engine/vdecode.rs`, `mix.rs`, `render.rs`), preview (`engine/worker.rs`), audio, effects (`engine/vfx.rs`, `afx.rs`), serialization, interchange, settings and updater are separate modules. There is no plug-in host module yet, and the media browser is UI-only. → R31, R32 |
| 18.2 | Rust workspace with independently testable modules | PARTIAL | One crate, `milan-pro`; unit tests per module. → R50 |
| 18.3 | Timeline not coupled to one codec library or rendering API | DONE | `trait Sources`, `compose()` frame-in/frame-out, FFmpeg behind `vdecode`/`mix` |
| 18.4 | Meaningful error handling | DONE | `anyhow` plus user-facing messages throughout |
| 18.5 | Logging | DONE | R44: see 15.11 |
| 18.6 | Automated tests | DONE | 319 unit tests |
| 18.7 | Integration tests | PARTIAL | 17 `#[ignore]` end-to-end tests need FFmpeg (`grep '#\[ignore'`); there is no `tests/` folder and CI never runs them. → R51 |
| 18.8 | Performance tests | PARTIAL | R19: the benches run locally through `tools/bench.ps1` into `docs/PERFORMANCE.md`; CI never runs them → R51 |
| 18.9 | Memory-safety-conscious design | DONE | `unsafe` only in `windows-sys` calls (`main.rs`, `proc.rs`) |
| 18.10 | Documentation | PARTIAL | (R52, R53, R54, R56 DONE) Reference analysis, architecture, developer guide, community files and licences are written. → R55 (compatibility matrix) |
| 18.11 | Dependency licensing review | DONE | (R56) [`docs/LICENSES.md`](LICENSES.md); `tools/licenses.ps1 -Check` fails when it is stale. |
| 18.12 | CI builds | DONE | |
| 18.13 | Code formatting and linting | MISSING | No `rustfmt.toml`; rustfmt is not even installed locally; no clippy or fmt in CI. → R51 |
| 18.14 | No placeholder implementations disguised as complete | DONE | R0: the unlit track meters and every greyed Project Properties combo say on hover that they are not available yet and which batch builds them. |
| 18.15 | Never leave non-functional controls without marking them unfinished | DONE | R0: `menus::Why` on every greyed row, combo, checkbox and inert button; Help > Unfinished Features (`ui/dialogs/unfinished.rs`); tests `every_greyed_command_says_why` and `every_reason_names_a_batch_of_remaining_md` (`ui/menus.rs`). |
| 19.1 | Test media import | PARTIAL | `media.rs` unit tests; real files only in ignored tests → R37 |
| 19.2 | Test media decoding | PARTIAL | `vdecode.rs` ignored tests → R37 |
| 19.3 | Test playback | DONE | R19: playback benches at 1-4 layers, with scopes, the multicam grid and through the Video Preview with adaptive quality (`worker.rs`, `engine/mod.rs`), run by `tools/bench.ps1`; `engine_show_and_play`, `jump_cut_plays_exact_frames` |
| 19.4 | Test audio synchronization | DONE | Mixer clock unit tests; (R9) the latency-compensation test with a fake timestamp; (R19) every playback bench measures how far each frame shown is behind the audio clock (p50 / p95 / max in `docs/PERFORMANCE.md`) |
| 19.5 | Test timeline editing | DONE | `edit.rs` tests |
| 19.6 | Test multiple video tracks | DONE | Compositor and plan tests |
| 19.7 | Test multiple audio tracks | DONE | `mix.rs` tests |
| 19.8 | Test transitions | DONE | `engine/transition.rs` tests |
| 19.9 | Test effects | DONE | `vfx.rs`, `afx.rs` tests |
| 19.10 | Test keyframes | DONE | `engine/keyframes.rs` tests |
| 19.11 | Test plug-in loading | DONE | OFX: the test bundle (`tools/ofx-test-plugin`) and the VEGAS 15 probe (R33c); VST3 and CLAP: Milan's own test modules (`tools/vst3-test-plugin`, `tools/clap-test-plugin`) scanned, processed, crashed, hung, preset-loaded and shown through the host (`aplug::tests`). |
| 19.12 | Test project saving | DONE | `project.rs` tests (atomic save, failed save) |
| 19.13 | Test project reopening | DONE | `old_projects_load_*` tests |
| 19.14 | Test project interchange | DONE | VEGAS JSON, OTIO (lossless round trip, two hand-written fixtures), EDL (round trips, goldens at 25 and 29.97 DF, a Resolve-style EDL), FCP7 XML and FCPXML (round trips, hand-written fixtures) tests |
| 19.15 | Test rendering | PARTIAL | `engine/render.rs:329` ignored e2e → R51 |
| 19.16 | Test hardware acceleration | DONE | (R22) `hw_capability_table` (ignored: the probe on this machine) and `render_with_hardware_encoder`; parser and choice unit tests on captured output. |
| 19.17 | Test recovery after crashes | DONE | R43: `autosave::tests::crash_recovery_e2e` (ignored): the app runs off-screen, autosaves, is killed by pid, and the next start lists the project and screenshots the dialog; unit tests for the stale-marker check, rotation and the unsaved copy |
| 19.18 | Legally usable test media collection (resolutions, rates, codecs, containers, bit depths, audio configs) | MISSING | → R37 |
| 19.19 | Automated tests for serialization and timeline accuracy | DONE | `project.rs`, `plan.rs`, `edit.rs` |
| 19.20 | VEGAS example projects; exact VEGAS versions and capabilities tested | MISSING | → R55 |

### 2.19 Final deliverables (roll-up, not counted)

| Deliverable | Status | Blocking batches |
|---|---|---|
| Public GitHub repository | DONE | |
| Fully functional Rust desktop editor | PARTIAL | R1-R12 |
| VEGAS-inspired refined interface | DONE (minus greyed items of later batches, all labelled) | (R13, R14, R15, R16a, R16b, R16c, R17a, R17b, R18a, R18b DONE) |
| Real multitrack timeline engine | DONE | |
| Responsive preview player | DONE | (R19-R23) |
| Comprehensive media format support | PARTIAL | R37-R41, R10 |
| Professional audio editing system | PARTIAL | R8-R10 |
| Professional rendering engine | DONE | (R11, R12, R22) |
| Modular effects and transitions system | DONE | Masks in R7 |
| Plug-in compatibility architecture | MISSING | R31-R35 |
| VEGAS project import/interchange | DONE | R25, R26, R27, R28, R29, R30 |
| Automatic GitHub updater | DONE | R46 (minisign key and certificate pending) |
| Windows installer | DONE | |
| Automated tests | DONE | R51 for CI coverage |
| Complete technical documentation | PARTIAL | R52, R53, R54, R56 done; R55 open |
| Public compatibility matrix | MISSING | R55 |
| Development roadmap | DONE | this file (section 5), linked from the README roadmap (R53) |
| Repository stays buildable | DONE | |

---

## 3. Disabled and placeholder controls

The brief says: "Never leave nonfunctional buttons or controls without clearly identifying them as unfinished."

**Status: DONE (R0).** Every item below now carries a `menus::Why`: `Later("Rn")` (shown as "Not available yet: batch Rn"), `NotPlanned("R57".."R60")` ("Not planned: VEGAS long tail Rn") or `Fixed("...")` (always on, set by the look, not feasible). Menu rows (`X`, `K`, `R`, `off`, `inert`) show it on hover; `cmd` rows show `menus::unfinished(command)`; combos (`fixed_combo`), checkboxes and buttons use `on_disabled_hover_text`; inert toolbar buttons name the batch in their tooltip; empty Preferences and Project Properties pages name theirs. Help > Unfinished Features lists every greyed menu row and keyboard-only command. The tests `every_greyed_command_says_why` and `every_reason_names_a_batch_of_remaining_md` (`ui/menus.rs`) fail if a greyed command has no reason, if a labelled command works, or if a reason names a batch missing from this file. A batch that builds an item removes its `Why` in the same change (rule 5.0).

The inventory as found before R0:

| Kind | Count | Where |
|---|---|---|
| Greyed menu rows (`X(...)`) | 156 | `ui/menus.rs` (File/Edit/View/Insert/Tools/Options/Help), `ui/timeline/ctx.rs`, `ui/media_panel.rs` |
| Greyed rows via `off(...)` | 16 | `ui/timeline/ctx.rs`, `ui/media_panel.rs`, menus |
| Inert check/radio rows (`K`/`R`) | 121 | Switches, Channels, Grid Spacing, Ruler Format (14), Loudness Meters, Video Envelopes view, Mixer Preview Fader, Metronome, Mute All, External Control... |
| `Command`s that `CmdState::enabled` always refuses (`_ => false`, `menus.rs:162`) | ~85 | Listed below |
| Disabled combos `fixed_combo` | 17 | Project Properties video/audio tabs (`settings.rs:309-363`) |
| `add_enabled(false, ...)` controls | 16 | Prefs General (`prefs.rs:97-114`), Project Properties (`settings.rs:343-350`), Render options (`dialogs/render.rs:260-272`), Event Properties Loop (`event_props.rs:112`), Preview menu (`preview.rs:70, 161, 166`) |
| Controls that look live but are not | 2 | Track meters during playback (`tracks.rs:528`); VP23 preview HDR/360° buttons (labelled "not applicable", which is fine) |

**Commands that are always disabled** (from `keymap.rs:11-74` minus the arms of `menus.rs:113-161`):

| Group | Commands | Batch |
|---|---|---|
| File | SaveVersion | R42 (DONE) |
| Edit | PasteRepeat, SmartSplit, NestedCreate, NestedOpen, NestedParent, RenderToNewTrack, OpenInAudioEditor, Multicam | R2 (DONE), R18a (DONE), R6b (DONE), R12 (DONE), R18a (DONE), R17b (DONE) |
| Playback | PreviewAtCursor, DimMaster, Prerender, RamPreview, ExternalMonitor | R2 (DONE), R8b (DONE), R23 (DONE), R23 (DONE), R14 (DONE) |
| Cursor | GridLeft, GridRight, SelectToNextMarker, GoTo, CursorToPlayhead, JogLeft, JogRight | R1 (GridLeft, GridRight, GoTo: DONE), R2 (DONE) |
| Selection | SelectLoopRegion, RestoreSelection, SelPrevEvent, SelNextEvent, SelectEventStart, SelectEventEnd, ExpandedEdit | R2 (DONE) |
| Nudge and pitch | NudgeFrameLeft/Right, NudgePixelLeft/Right, NudgeTrackUp/Down, PitchUp/Down, PitchCentUp/Down, PitchOctUp/Down | R2 (nudges: DONE), R4b (DONE) |
| Markers | InsertCommand, CdTrack, CdIndex | R18a (DONE) |
| Tracks | InsertAdjustmentTrack, MuteExclusive, SoloExclusive, GroupMute, GroupSolo, ToggleListFocus, NextFxEnvelope, PrevFxEnvelope | R18b (DONE), R2 (DONE), R5 (DONE), R8c (DONE) |
| Snapping | SnapGrid, SnapAllEvents | R1 (DONE) |
| Zoom | ZoomFramePerThumb, WaveZoomIn, WaveZoomOut | R2 (DONE) |
| View | BusTracks, VideoBusTrack, MediaMarkers, NextWindow, PrevWindow | R8b (DONE), R7 (DONE), R18a (DONE), R13 (DONE) |
| Docking | ToggleDocks, TopDock, BottomDock, RightDock, CollapseTrackList, MaximizeTimeline, LayoutVp14, LayoutAudio, LayoutColor, LayoutSource, LayoutTimelineTop | R13 (DONE) |
| Windows | FocusTimeline, EditDetails, ProjectNotes, PluginManager, VideoScopes, SurroundPanner, MixingConsole, DeviceExplorer, LoudnessMeters, Dashboard, MotionTracking, TextToSpeech | R13 (DONE), R16a (DONE), R16b (DONE), R16c (DONE), R31, R10b (DONE), R8b (DONE), R59 |
| Misc | Help, WhatsThis, ContextMenu, RebuildPeaks, MidiTimecode, MidiClock, MidiTrigger | R53 (DONE), R18a (DONE), R59 |
| Always on, shown checked but disabled (labelled "always on" by R0) | SnapMarkers, Waveforms, EventButtons, FadeLengths, EdgeFrames, TakeInfo | DONE |

**Toolbar buttons already labelled before R0** (R0 added the batch to each tooltip; they stay until their feature lands):
- `explorer.rs:168-171`: Favorites, Start/Stop Preview, Auto Preview.
- `media_panel.rs:161-183`: Capture, Import from Device, Get Media from the Web, Media FX, Auto Preview.
- `master_bus.rs:69-72`: Master Bus Properties, Downmix, Dim, Mixing Console (R8b: DONE, they work).
- `preview.rs:36-40`: External Monitor, Split-Screen (R14, R16b: DONE, they work).
- `transport.rs:201-245`: Zoom Edit Tool, Smart Split, the three Nested Timeline buttons (R6b: DONE, they work).
- `toolbar.rs:207-208`: Tutorials, What's This.
- `trimmer.rs:98`: Show Video in Preview.

---

## 4. Reference inventory

### 4.1 VEGAS Pro 15.0 (build 177), `C:\Program Files\VEGAS\VEGAS Pro 15.0`

Read-only listing; VEGAS was not launched.

Note: the installed reference is **VEGAS Pro 15**. The "VEGAS 16" look was modelled on it ("VP16 kept the VP15 interface", `VEGAS_UI_SPEC.md:11`); the "VEGAS 23" look comes from documentation and screenshots only. REFERENCE_ANALYSIS.md must say so.

| Area | What is installed |
|---|---|
| Main exe | `vegas150.exe` (+ `vegas150k.dll`, `.udat`, `.zip` resources), `vidcap60.exe` (capture) |
| Scripting | `ScriptPortal.Vegas.dll` (.NET API), `Script Menu\` with 17 scripts: Batch Render, Export EDL (.js), Export Regions as Subtitles, Export Closed Captioning for DVD Architect/QuickTime/WMP/YouTube, Export Chapters, Render Audio Tracks, Group Video and Audio Events, Promote Media Markers/Closed Captioning, Stereo to Mono, Resize Generated Media, Remove Letterboxing, Add/Remove Timecode |
| Project interchange | `ProjectInterchange.dll`, `ProjectInterchangeHelper.dll`, `AAFCOAPI.dll` (AAF import/export in VEGAS) |
| FileIO plug-ins (31) | ac3studioplug, aifplug, atracplug, aviplug, compoundplug, flacplug, gifplug, mcmp4plug2 (MainConcept AVC, CPU/CUDA/OpenCL encoders), mcmp4xavcs, mcplug2, mp3plug2, mp4plug3, mvcplug, mxavcaacplug, mxfhdcamsrplug, mxfp2, mxfplug, mxfplug3, mxfxavc, mxhevcplug, oggplug, qt7plug, redplug (RED R3D), sctplug, sflgaplg, sfpaplug, so4compoundplug, stl2plg, vduplug, wavplug, wicplug, wmfplug4 |
| Intel Media SDK | `libmfxsw64.dll`, HEVC decode and encode plug-ins in two GUID folders |
| OFX video plug-ins | 7 bundles in `OFX Video Plug-Ins\` (4.2) |
| Legacy video plug-ins | `Video Plug-Ins\`: PluginWrapper, colorcorrector, sfpagepeel, sftrans1, vfplugs, vfx1, vidpcore (DirectX-era, VEGAS-private) |
| Third-party bundled | `proDADMercalli20.dll` (stabiliser), `ffplugsk64.dll`, gnsdk_* (Gracenote) |
| VST support | `sfvstwrap.dll`, `SfVstProxyStubx64.dll`, `x86\sfvstserver.exe` (32-bit VST bridge, an out-of-process precedent) |
| Audio hardware | `Audio Hardware Drivers\`: sfasio.dll (ASIO), sfdsound.dll (DirectSound), extvid_drv.dll |
| Video hardware | AJA, Decklink, AVC/HDV (FireWire), Monitor3D, extviddev |
| Control surfaces | FaderPort, Mackie, TranzPort, generic, networkXML; ShuttlePRO/Xpress `.pref` profiles |
| Colour | `OpenColorIO.dll` + `OpenColorIO\` configs (ACES) |
| Error reporting | ErrorReport*.exe/.dll, `CreateMinidumpx64.exe` (precedent for R44) |
| Layouts | `Standard Layouts\`: Audio Mixing, Color Correction, Default Expanded Edit Mode, Vegas 14 Default (`.VegasWindowLayout`; inputs for R13) |
| Disc | sfdvd, bdmux, ProDiscAPI, sfcd* (CD/DVD/Blu-ray burning) |

### 4.2 Plug-ins

**OFX bundles** (`Contents\Win64\*.ofx`; ids from `Contents\Resources\*.xml`):

| Bundle | Size | Plug-ins | Vendor |
|---|---|---|---|
| `Vfx1.ofx` | 14.3 MB | 96. **Generators (5):** checkerboard, colorgradient, noisetexture, solidcolor, testpattern. **Transitions (28):** 3dblinds, 3dcascade, 3dflyinout, 3dshuffle, barndoor, clockwipe, crosseffect, dissolve, flash, gradientwipe, iris, linearwipe, pageloop, pagepeel, pageroll, portals, push, slide, spiral, split, squeeze, starwipe, swap, venetianblinds, zoom, plus bumpmap/displacementmap/heightmap compositors. **Filters (63):** addnoise, blackandwhite, blackrestore, border, brightnessandcontrast, broadcastcolors, bumpmapfilter, channelblend, chromablur, chromakeyer, colorbalance, colorcorrector, colorcorrectorsecondary, colorcurves, colormatch, convolutionkernel, cookiecutter, defocus/fill-light/glint/rays/soft-contrast/starburst velvetmatter, deform, filmeffects, filmgrain, gaussianblur, glow, gradientmap, histogram, hsladjust, invert, labadjust, layerdimensionality, lensflare, levels, lightrays, linearblur, lutfilter, maskgenerator, median, minandmax, mirror, newsprint, pinchpunch, pixelate, quickblur, radialblur, radialpixelate, saturationadjust, sepia, sharpen, spherize, swirl, threshold, timecode, tvsimulator, unsharpmask, vignette, wave, whitebalance, pictureinpicture, crop | `com.sonycreativesoftware:*` |
| `Filters.ofx` | 8.5 MB | Stereoscopic3DAdjust | sonycreativesoftware |
| `Stabilize.ofx` | 7.7 MB | Stabilize | sonycreativesoftware |
| `TitlesAndText.ofx` | 0.4 MB | titlesandtext (generator, uses `ScriptPortal.MediaSoftware.TextGen.OFXInterop.dll`, a .NET dependency) | sonycreativesoftware |
| `spica_cutout.ofx` | 3.7 MB | smart_zoom | sonycreativesoftware |
| `spica_resizer.ofx` | 3.7 MB | smart_upscale | sonycreativesoftware |
| `HitFilmVegasBasic.ofx` | 1.5 MB | 7: BleachBypass, LightFlares, ScanLines, ThreeStripColor, TVDamage, Vibrance, WitnessProtection | **FXHOME (third party)** |

**DirectX audio plug-ins** (`C:\Program Files (x86)\VEGAS\Shared Plug-Ins\Audio_x64`, 11 DLLs plus `.chm` help): mchammer_x64 (Multi-band dynamics), sffrgpnv_x64, sfppack1/2/3_x64 (plug-in packs: EQ, compressor, reverb...), sfresfilter_x64, sftrkfx1_x64 (track EQ/compressor/noise gate), sfxpfx1/2/3_x64 (XP FX), xpvinyl_x64.

**VST:** `C:\Program Files\Common Files\VST3` and `C:\Program Files\Steinberg\VSTPlugins` exist and are empty. No CLAP folder; no `Common Files\OFX`.

R33c note: the counts above come from the `Resources\*.xml` files; the binaries export 79 plug-ins (`Vfx1.ofx` 67, not 96), and none runs outside VEGAS (`docs/compat/ofx-vegas15.md`).

**Consequences for the plug-in batches:**
- Every OFX bundle except HitFilm is first-party VEGAS code. It is licensed with VEGAS and may check the host. R33c must test whether each loads in a foreign host, and record the licence position, before anything is claimed.
- The user's VST3/CLAP folders are empty. R34 and R35 are verified against free MIT/GPL test plug-ins that are not bundled (for example Surge XT, Airwindows Consolidated, the VST3 SDK "again" sample), downloaded by the user.
- The DX audio plug-ins are COM DMOs. Hosting them is technically documented by Microsoft, but they are VEGAS-licensed. R35 is research only.

### 4.3 Bundled FFmpeg capabilities (9.0.2 gyan full)

- **Hardware accelerators:** cuda, vaapi, dxva2, qsv, d3d11va, opencl, vulkan, d3d12va, amf.
- **Hardware encoders:** h264/hevc/av1 × {nvenc, qsv, amf, mf}; mpeg2_qsv, mjpeg_qsv, vp9_qsv.
- **Software encoders of interest:** libx264, libx265, libsvtav1, libaom-av1, librav1e, libvpx (VP8), libvpx-vp9, prores/prores_ks/prores_aw, dnxhd (DNxHD/HR), cfhd (CineForm), ffv1, png, tiff, exr, alac, flac, ac3, eac3, dca (experimental), aac, libmp3lame, libopus, libvorbis, pcm_*.
- **Filters of interest:** subtitles and ass (libass), zscale, tonemap, tonemap_opencl, libplacebo.

---

## 5. Implementation batches

### 5.0 Ground rules (carried over from `FEATURE_PLAN.md`)

- One agent does one batch, from start to finish. Each batch ends with `cargo build` and `cargo test` green, the app working, and nothing half-wired.
- Each new persistent field gets `#[serde(default)]` and extends the old-project test. From R42 onwards, a schema change also bumps `format_version` and adds a migration step with a test.
- A greyed item is enabled in the batch that builds its feature, and its "not available yet" label is removed in that same batch.
- Child processes go through `proc::spawn`. Known limits are marked `// shortcut: <limit>, <when to upgrade>`.
- No new crate when one in `Cargo.lock` does the job. New crates carry their licence in the batch notes, and R56 must list them.

### 5.1 Order

Batches follow the user's priority order, adjusted for dependencies. R0 goes first because it enforces a hard rule of the brief and is small.

| Tier | Priority | Batches |
|---|---|---|
| 0 | Hard rule (no silent placeholders) | R0 |
| 1 | Editing functionality | R1, R2, R3a, R3b, R4a, R4b, R5, R7, R8a, R8b, R8c, R9, R11, R12 |
| 2 | VEGAS familiarity | R13, R14, R15, R16a, R16b, R16c, R6a, R6b, R17a, R17b, R18a, R18b |
| 3 | Preview performance | R19, R20, R21a, R21b, R21c, R22, R23 |
| 4 | Project compatibility | R25, R26, R27, R28, R29, R30 |
| 5 | Plug-in compatibility | R31, R32, R33a, R33b, R33c, R34a, R34b, R35; AE-format effects A0-A9 (`docs/AE_PREMIERE_PLUGINS.md`) |
| 6 | Media support | R37, R38, R39, R40a, R40b, R10a, R10b, R10c, R41 |
| 7 | Reliability | R42, R43, R44, R45a, R45b, R46 |
| 8 | Modern interface | R47, R48, R49 |
| 9 | Open-source project | R50, R51, R52, R53, R54, R55, R56 |
| 10 | VEGAS long tail (outside the brief, but greyed in the UI) | R57, R58, R59, R60 |

Dependency notes:
- R12 needs R11.
- R8b and R8c need R8a.
- R10a needs R8a; R10c needs R10a.
- R21b and R21c need R21a.
- R40b needs R21a.
- R33 and R34 need R31 and R32.
- R55 needs R33c, R34a, R37, R28 and R29.
- R46's export guard needs R12.
- R53 follows R50.
- R6 (nested timelines) is placed in tier 2 because it is a VEGAS 23 workflow feature.
- R24 is unused: its scope (cpal latency) was merged into R9.
- R36 is unused: the plug-in matrix lives in R55.

---

### R0: Label every unfinished control (S)

**Status: DONE.** Built as designed below, except: the reasons live on the rows (`X(label, Why)`, `off(ui, label, Why)`, `fixed_combo(..., Why)`) instead of a label-keyed `UNFINISHED` table, because the same label means different things in different menus; Help > Unfinished Features builds its list by walking the menu tables (`menus::greyed`) plus keyboard-only commands; rows greyed only for lack of a selection get no hover text (they work).

Items no batch listed, assigned by R0 (each later batch takes these on with its scope):
- R2: Options > Mute All Audio / Mute All Video.
- R3b: the resample switches (Use Project Resample Mode, Force Resample, Disable Resample, Optical Flow), Project Properties Resample mode, Event Properties Undersample rate, Options > Quickfade Audio Edits (not implemented today).
- R4a: video track Mute envelope, View > Video Envelopes filters (Track Mute, Fade to Color, Keyframes, Velocity, Transition Progress).
- R5: track header Cut/Copy/Paste/Delete.
- R7 (DONE): Motion Blur Amount and Video Supersampling envelopes, Project Properties Motion blur type, the compositing modes 3D Source Alpha, Cut and Difference Squared.
- R8b: the ripple mode "Affected Tracks, Markers, and Regions" (the same as the mode above it until bus tracks exist).
- R8c (DONE): audio event Volume/Pan/Mute envelopes in the event menu, envelope point Properties.
- R11: Project Properties Field order, Pixel aspect ratio, Full-resolution rendering quality, Deinterlace method, Adjust source media, audio Bit depth; preview Simulate Device Aspect Ratio.
- R12: File > Real-Time Render (R12 moved it to R59: it exists for external hardware FX, which Milan Pro does not host; a normal render gives the same file).
- R13/R14: the Display page docking options (R13 DONE: Automatically hide docking area, Display timeline at bottom of main window (VP16), Position tabs at top of docked windows; R14 DONE: Allow floating windows to dock).
- R16a (DONE): region Rename (marker bar), Project Properties Summary page.
- R16b (DONE): preview Split-Screen View and the Isolate channel overlays.
- R18a (DONE): View > Event Length (VP23).
- R18b (DONE): video track Set Default Track Properties.
- R10a (re-assigned from R4b by R18b): Project Properties › Audio › Resample and stretch quality.
- R49 (re-assigned from R15 by R18b, done): the track header's Edit Visible Button Set.
- R30 (done): Tools > Advanced Save (VP23) opens Archive Project.
- R49 (done): Display page envelope/snap colours, icon colour strength (Milan Pro has one look, so no Interface Type radios); File I/O, Deprecated Features and Project Properties Advanced pages (labelled).
- R53 (done): Show Me How, Interactive Tutorials, Download User Manual (open the docs).
- R57/R58/R59 (not planned): Explorer Favorites, Trimmer Loop Playback and Fit to Fill, fade start levels and Reset ASR, Red Eye Reduction, Expand Track Layers, track display and time display custom colours.

**Scope**
- Every greyed control says why it is greyed.
- Add a Help > "Unfinished Features" list.
- Remove the misleading live look from track meters.

**Design**
- `ui/menus.rs`:
  - In `entry`, when `!enabled`, add `.on_hover_text(reason)`.
  - Add `pub fn reason(label) -> &'static str`, which looks the label up in a new `const UNFINISHED: &[(&str, Plan)]`.
  - `Plan` is `Planned(batch)` or `NotPlanned(why)` or `AlwaysOn`. Unknown labels default to "Not available yet".
  - For `cmd()` rows disabled by `CmdState::enabled`, use the command's own reason: "needs a selection" when it is enabled in principle, else the unfinished text.
- `ui/dialogs/mod.rs::fixed_combo` and the `add_enabled(false, ...)` sites take `.on_disabled_hover_text("Not available yet")`. This is egui 0.31 API, so no new code path is needed.
- `ui/timeline/tracks.rs::paint_meter`: while playing and the track is not armed, draw the unlit meter with a hover "Per-track playback meters are not available yet".
- New `ui/dialogs/unfinished.rs`: a modal listing `UNFINISHED` grouped by menu, with the batch id or "not planned" reason. It opens from Help > Unfinished Features.

**Files:** `ui/menus.rs`, `ui/dialogs/mod.rs`, `ui/dialogs/unfinished.rs` (new), `ui/dialogs/prefs.rs`, `settings.rs`, `ui/dialogs/render.rs`, `ui/dialogs/event_props.rs`, `ui/preview.rs`, `ui/timeline/tracks.rs`, `app/commands.rs` (open the dialog).

**Acceptance tests**
- A unit test walks every menu table (`FILE`...`HELP`, `ctx.rs` and `media_panel.rs` tables) and asserts that each `X`, `K` and `R` row and each always-disabled `Command` has a non-empty reason.
- Every entry in `UNFINISHED` names a batch id that exists in this file, or a not-planned reason.
- Manual: hovering a greyed row in each menu shows the text.

---

### R1: Grid, snapping and ruler formats (M)

**Status: DONE.** Built as designed below, except:
- The ruler settings (format, start time, tempo, beats per measure, beat note) live in `ProjectSettings.ruler`, not `Timeline.tempo` and `Prefs.ruler`: VEGAS keeps them with the project on the Project Properties Ruler page, which R1 also builds. Changing the format is an undo step, like any project setting. Grid spacing, Snap to Grid and Snap to All Events are in `Prefs`.
- `rate_ratio`, `exact_fps` and `frame_time` are in `project.rs`; `render::fps_str` is built from them, and Quantize to Frames uses the same exact grid.
- Also built: Set Time at Cursor (the ruler start time), VP23 Set Project Tempo (opens Project Properties at the Ruler page), and the time display's Time Format list. The time display shares the ruler's format (VEGAS can set it separately).
- Snap to All Events off means a dragged event snaps to event edges on its own tracks only. The cursor, markers and the time selection always count.
- With a frame-based format, ruler ticks fall on whole frames of that format, so at 29.97 the labels read 00:00:18;00 rather than 00:00:17;29.
- Grid lines are thinned at low zoom to at least 6 px apart (1-2-5 steps, or doubling for musical spacings). Snapping uses the lines that are drawn.

**Scope**
- Snap to Grid, Snap to All Events, the Grid Spacing radios and all 15 Ruler Format radios.
- Go to Time (Ctrl+G), Move Left/Right by Grid Marks.
- A long-duration timebase test.

**Design**
- New `Prefs` fields: `grid: GridSpacing { RulerMarks, Minutes, Seconds, Milliseconds, Frames, Measures, Note(n, triplet) }`, `ruler: RulerFormat { Samples, Time, Seconds, TimeFrames, AbsFrames, MeasuresBeats, Feet16, Feet35, Smpte(…) }`, `snap_grid`, `snap_all`.
- Tempo goes in `Timeline.tempo: f64` with a default of 120 and `#[serde(default)]`. It is needed for measures.
- `project.rs`: add `format_time(t, fmt, fps, sr, tempo)` and `parse_time(s, fmt, ...)`, both pure.
- `edit::snap_points` adds grid lines when `snap_grid` is on. `snap_all` snaps across all tracks (today only the edited track's neighbours count).
- Ruler (`ui/timeline/ruler.rs`) and time display (`ui/transport.rs`) use `format_time`.
- `GoTo` is a modal with one text field parsed by `parse_time`.
- Exact rates: add `fn rate_ratio(fps) -> (u32, u32)` beside `fps_str`. `plan::frame_plan` and `render` compute `t = n * den / num` (instead of `n / fps`), so preview, render and audio share one exact grid.

**Files:** `project.rs`, `settings.rs`, `edit.rs`, `engine/plan.rs`, `engine/render.rs`, `ui/timeline/ruler.rs`, `ui/transport.rs`, `ui/menus.rs`, `app/commands.rs`, `ui/dialogs/goto.rs` (new).

**Acceptance tests**
- `format_time` and `parse_time` round-trip for every format at 23.976, 25, 29.97 DF and 59.94 DF, 48 kHz.
- Snap to a 1 s grid lands on whole seconds.
- Frame n of a 29.97 project at 3 h: `t(n)` × 30000/1001 equals n exactly. Render frame count for 1 h at 29.97 = 107892.
- Old projects load with tempo 120.

---

### R2: Navigation, selection, nudge and zoom commands (M)

**Status: DONE.** Built as designed below, except:
- Edge-trimming mode follows VEGAS: [ / ] (Num 7 / Num 9) select the focus event's start / end edge (red bracket), then walk to the previous / next edge on its track; the frame and pixel nudge keys and F3 / F9 then trim that edge (one undo step each, rippled with Auto Ripple, crossfaded with Automatic Crossfades). Esc, Exit Edit Mode or selecting another event leaves it. Enter Expanded Edit Mode (Num 5) turns the Trimmer into a two-up view of the outgoing event's last frame and the incoming event's first frame at the edge (`trimmer.rs`, second `Preview`). Not built: entering it by double-clicking an edge, the expanded A/B view of the track itself, and the Ctrl/Alt/Shift numpad trim variants (stretch, slip, L/J, slide), which need numpad keys egui cannot read.
- Non-numpad keys: Alt+, / Alt+. frame nudge, Alt+Shift+, / Alt+Shift+. pixel nudge, Alt+PageUp / Alt+PageDown track nudge, Ctrl+Shift+Space Preview at Cursor (Milan's own). Help › Keyboard Shortcuts says why.
- Preview at Cursor plays Preferences › Editing "Cursor preview duration" (default 3 s, new `Prefs.cursor_preview`) centred on the cursor as a temporary time selection, looping with Loop Playback, then puts the cursor and selection back.
- Restore Previous Time Selection keeps five (`edit::SelHistory`), as VEGAS does. Milan has no loop region apart from the time selection, so Select Loop Region brings back the last time selection after it was cleared.
- Move Cursor to Playhead keeps the cursor where playback is when it stops (Milan's cursor is the playhead while playing). Jog (F3 / F9) steps one frame while stopped.
- Mute All Audio / Mute All Video are playback-only session switches (silent master, empty Video Preview), like Bypass All Audio FX; renders are not affected.
- One frame per thumbnail (Alt+Up) uses the focus (else first) video track's height and the project aspect. Waveform zoom (Shift+Up / Down) is 1x to 64x, for this session.
- Also built: Select All After Cursor (VP23 Edit › Select and video track header), Select Events to End (event menu: the event and later ones on its track, plus groups), Exclusive mute / solo, Tab track list focus, Ctrl+Shift+Right time select to the next marker, Ctrl+Shift+[ / ] time select the previous / next selected event. Nudges are one undo step each and repeat while held.
- Paste Repeat does not ripple (paste with Paste Insert for that).

**Scope:** enable CursorToPlayhead, JogLeft/Right, SelectToNextMarker, SelectLoopRegion, RestoreSelection (a selection history), SelPrevEvent/SelNextEvent, SelectEventStart/End, ExpandedEdit (trim-mode view of the edit point, as in VEGAS: a two-up Trimmer of outgoing and incoming events), PasteRepeat (dialog: count and spacing), MuteExclusive, SoloExclusive, ToggleListFocus, ZoomFramePerThumb, WaveZoomIn/Out, PreviewAtCursor, the Nudge commands, and "Select All After Cursor" and "Select Events to End".

**Design**
- Mostly `app/commands.rs` arms calling small pure helpers in `edit.rs` (`select_after(t)`, `selection_history`, `paste_repeat`).
- Numpad: egui cannot tell numpad keys apart (`keymap.rs` note). Give the Nudge commands VEGAS's alternative non-numpad keys (Alt+, and Alt+. for frame nudge). Leave the Num rows listed, and say so in the keyboard dialog.
- ExpandedEdit reuses `ui/trimmer.rs` with two sources.

**Files:** `app/commands.rs`, `edit.rs`, `ui/menus.rs`, `ui/keymap.rs`, `ui/trimmer.rs`, `ui/dialogs/paste_repeat.rs` (new).

**Acceptance tests**
- Unit tests per helper: paste repeat ×3 every 2 s; restore selection after 3 changes; select-after picks events starting at or after t.
- Every listed command is in `CmdState::enabled`; `R0`'s test list shrinks.

---

### R3a: Event looping, empty events, subclips, attribute paste, group operations (M)

**Status: DONE.** Built as designed below, except:
- Loop repeats the whole file (or subclip) from its start, as VEGAS does, not from the in point: source time wraps modulo the media length (`edit::loop_time`), and the audio plan cuts a looping event into passes (`edit::loop_passes`, one mixer stream each). Only the right edge runs on past the media; the in point stays at or after 0. Loop off keeps Milan's media-end limit for drags; an event already past the end then holds its last frame and is silent there. Media under 50 ms does not loop (`edit::MIN_LOOP`).
- `MediaItem.subclip` is `Subclip { parent, start }` with `duration` the subclip's length and `path` the parent's file; `MediaItem::file_time` adds the start for the plan, mixer, thumbnails, preview and waveforms. Create Subclip comes from the Trimmer's More menu (in/out range, or the whole media) and the event menu (the event then uses the subclip; a span that wraps past the media end only adds it to Project Media). Not built: the naming dialog (subclips are named "<media> Subclip n"; rename them in Project Media), and opening a subclip in the Trimmer shows its whole file. The timeline region tags' Create Subclip says why it is greyed (regions are not media).
- Insert Empty Event uses the selected tracks, else the focus event's track. Empty events are hatched.
- Paste Event Attributes takes the first copied event of each kind (video onto video, audio onto audio); Selectively Paste offers Event FX, Pan/Crop, Envelopes, Gain, Playback rate and Fades. A pasted rate keeps the event's length.
- The event menu's Group ▸ now has Cut All / Copy All / Delete All too, as Edit › Group does.

**Scope**
- The event Loop switch and events extended past media end (looped, with notch marks).
- Insert Empty Event; Create Subclip.
- Paste Event Attributes and Selectively Paste Event Attributes.
- Group > Cut/Copy/Delete All.
- Takes shorter than the event loop (`edit.rs:1049` shortcut).

**Design**
- `Clip.looping: bool` (default true, as in VEGAS).
- `media_limit` returns None when `looping`.
- `src_time` wraps modulo `(media_len - in)` in `plan.rs` for both video and audio. The mixer reopens its stream at the wrap; one `AudioStream` per loop pass.
- Empty event: `Clip { media_id: None }`, drawn hatched.
- Subclip: `MediaItem.subclip: Option<(parent_id, in, out)>`.
- Attribute paste copies `fx`, `pan_crop`, `envelopes`, `gain_db`, `rate` and fades by checkbox.

**Files:** `project.rs`, `edit.rs`, `engine/plan.rs`, `engine/mix.rs`, `ui/timeline/events.rs`, `ui/timeline/ctx.rs`, `ui/menus.rs`, `app/commands.rs`, `ui/dialogs/paste_attrs.rs` (new).

**Acceptance tests**
- An 8 s clip extended to 20 s plays source times 0..8, 0..8, 0..4 (plan and mixer unit tests).
- A loop notch is drawn at 8 and 16.
- Attribute paste of FX only leaves fades untouched.
- Group Delete All removes the grouped audio as well.

---

### R3b: Audio event switches and channels (M)

**Status: DONE.** Built as designed below, plus the items R0 assigned (resample switches, Project Properties Resample mode, Undersample rate, Quickfade Audio Edits), except:
- Normalize is `Clip.normalize` / `auto_normalize` plus `Clip.normalized: Option<Normalized>` (gain and the span it was measured on). The peak is read by decoding the event's audio as the mixer does, through its channel map (`mix::peak`), on one background thread (`audio::Normalizer`), not `astats`; the gain takes the peak to -0.1 dBFS (VEGAS's default "Normalize peak level", not a preference yet). Setting the measured gain is not an undo step of its own. Plain Normalize keeps its gain through trims and splits (Event Properties › Recalculate measures again), as in VEGAS; VP23 Auto Normalize measures again whenever the event's media, stream, channels, in point, length or rate change. A render started before a measurement finishes plays that event without the gain.
- Resampling blends the two source frames around each frame's time, read on the source's own frame grid (`plan::event_source`, `EventPlan.next`, `composite::blend_frames`). Smart (VP23 "Frame blend") acts when the source rate differs from the project's or the event plays off speed; a source that lands on whole frames (60p at normal speed in 30p) is read as before. Optical Flow decodes through ffmpeg `minterpolate` (mci) on its own grid: correct but a few 1080p frames a second (shortcut in `vdecode.rs`). New projects resample Smart (VP16), the VP23 Welcome screen's projects start with resampling off, and a project saved before R3b opens with Disable so its renders do not change. Event Properties has no resample control (the Switches radios are it).
- Maintain Aspect Ratio off stretches the crop to the frame (`plan::place`); Reduce Interlace Flicker is a vertical 1-2-1 blur of the source before the event FX (`composite::reduce_flicker`); Undersample rate holds each frame for 1/r project frames (Event Properties, 0.05-0.99).
- Channels and Invert Phase act on the PCM as it is read, before the event FX; Combine is the mean of both sides. The audio track header's Switches › Invert Phase is built too (`Track.invert`).
- Stream ▸ lists the focus event's streams (`MediaItem.audio_streams` / `video_streams`, counted at import; media probed before R3b counts as one stream until imported again) and sets them on the selected events of that kind. Another video stream, or Optical Flow, is decoded under its own source id; proxies are only used for stream 1. Waveforms still show the first audio stream (shortcut in `audio.rs`). R10b added the import notice and Stream ▸ Add All Streams as Tracks.
- Options › Quickfade Audio Edits (on by default, a preference): a 10 ms linear fade at each audio event edge that has no fade of its own, in playback and render alike (`AudioPlan.quickfade`).

**Scope**
- Invert Phase, Normalize (and VP23 Auto Normalize), Maintain Aspect Ratio and Reduce Interlace Flicker switches.
- Edit > Channels: Both, Left Only, Right Only, Combine, Swap.
- Edit > Stream: pick the audio or video stream of a multi-stream file.

**Design**
- `Clip.invert`, `Clip.normalize_db: Option<f32>` (peak found by a background `-af astats` scan, stored), `Clip.channels: ChannelMap`, `Clip.stream: u32` (the `-map 0:a:N` index in `mix.rs`/`vdecode.rs`).
- Aspect and flicker flags feed `plan.rs` (fit vs stretch; a vertical 1-2-1 blur for flicker).

**Files:** `project.rs`, `edit.rs`, `engine/mix.rs`, `engine/plan.rs`, `engine/vdecode.rs`, `media.rs` (stream count probe), `ui/menus.rs`, `ui/timeline/ctx.rs`.

**Acceptance tests**
- Mixer unit tests: invert gives -x; Left Only puts L on both channels; Swap swaps.
- Normalize to -0.1 dBFS scales the peak to 0.9886.
- A 2-stream test file (ffmpeg lavfi) plays stream 2 when it is chosen.

---

### R4a: Velocity, freeze frame, transition progress, fade to colour (M-L)

**Status: DONE.** Built as designed below, plus the items R0 assigned (video track Mute envelope, the View › Audio / Video Envelopes filters) and R5's Track Fade Colors, except:
- `engine/velocity.rs` integrates the envelope exactly (`Curve::area`, the closed-form integral of each curve), so there is no 1 ms table or cache. Velocity is a video event envelope (as in VEGAS); in R4b its audio can follow (an option, off by default). A forward trim and a split keep every frame where it was on the timeline; reversed trims, slips and stretches of an event with a velocity envelope still count its span at the plain rate (shortcut in `edit::trim_left_edge`), and such an event shows no loop notches.
- Freeze Frame at Cursor keeps the speed up to the cursor (a point there holding its value) and steps to a 0% Hold point, so the frame at the cursor holds until the next point or the event's end. Velocity makes an event "off speed" for Smart resampling.
- `Transition.progress` keys are fractions of the transition's length (0..1), so the envelope follows the overlap when an edge moves; none = linear. Transition Progress toggles on the focus event's transitions, in and out.
- A transition dropped on an event's end with nothing after it fades the event out over the cut-to-overlap length into transparent (`Clip.transition_out`); its menu, FX window (`FxTarget::Transition("<id>/out")`) and Remove Transition work like the incoming one's. An end that a later event overlaps is a crossfade (refused there).
- Fade to Color fades the track's picture (after its FX and Track Motion) towards the top colour above 0 and the bottom colour below, at the picture's own alpha (empty areas stay empty). Fade Colors ▸ Top / Bottom pick in place (one undo step per pick), white over black by default.
- Video track Mute hides the track where it is 0. Audio and video Mute are separate commands (Insert › Audio Envelopes and Insert › Video Envelopes act on the selected tracks of their kind).
- View › Audio / Video Envelopes hide kinds from the timeline (`Prefs.hidden_envelopes`; playback is unchanged). Track Keyframes shows a mark at the bottom of the video lane for each Track Motion and track FX key (VEGAS's separate keyframe row under the track is not built).
- VEGAS project import now brings Fade to Color and video track Mute envelopes.

**Scope (video and plan side)**
- Velocity envelope (-100%..1000%, negative means reverse), Freeze Frame at Cursor.
- Transition Progress envelope.
- Track Fade to Color envelopes and colours.
- Transition on a fade-out with nothing after it (`Clip.transition_out`).

**Design**
- `EnvKind::Velocity` already exists.
- New `engine/velocity.rs`: `integral(env, t) -> source offset`. It integrates the envelope piecewise with `Curve::shape`, numerically at 1 ms, and caches per event in `Arc<[f64]>`. The cache is keyed by the envelope hash.
- `edit::src_time` gains a velocity path. A freeze frame is a Hold key at 0%.
- `Transition.progress: Keyframes<f32>`.
- `Track.fade_colors: [Color; 2]` plus `EnvKind::FadeToColor` (VEGAS name; add the variant with `#[serde(other)]` safety intact).

**Files:** `project.rs`, `edit.rs`, `engine/velocity.rs` (new), `engine/plan.rs`, `engine/transition.rs`, `ui/timeline/envelopes.rs`, `ui/timeline/ctx.rs`, `ui/menus.rs`.

**Acceptance tests**
- Constant 200% velocity gives `src_time` = 2t.
- A ramp 100→0% over 1 s gives an offset of 0.5 s.
- A freeze holds frame n.
- A progress envelope at 0.25 renders the dissolve 25% mixed.
- A Fade to Color envelope at -1 draws the bottom colour.

---

### R4b: Audio varispeed, shuttle audio, pitch commands (L)

**Status: DONE.** Built as designed below, except:
- VEGAS leaves audio at normal speed under a video Velocity envelope, and so does Milan Pro by default (imported VEGAS projects sound the same). Options › Audio Follows Video Velocity (per project, `Timeline.audio_follows_velocity`, off by default, one undo step) keeps an A/V pair in step instead: the audio events grouped with a video event at its place and length get a copy of its envelope whenever it is inserted, frozen, edited or removed (`edit::sync_velocity`); the copy is not drawn. Turning the option on copies every video Velocity envelope to its pair, turning it off removes the copies (`edit::set_audio_follows_velocity`). Audio moved off its pair keeps the copy it has.
- Velocity audio is varispeed (the pitch follows rate x velocity, as a tape's does), Loop included; below 1% speed (a freeze) it is silent. The decoder reads the media at rate 1 from the earliest time the rest of the event reaches and keeps it (`mix::VLive`), so reverse and freeze need no areverse; the resampler reads it at each frame's exact time (`plan::Varispeed`), so preview and render match.
- Pitch is `Clip.pitch_cents`, within ±2 octaves: = / - a semitone, Ctrl a cent, Shift an octave (VEGAS's keys) on the selected audio events, and Event Properties › Pitch change (semitones). It is ffmpeg `asetrate` put back to length by `atempo` in the decoder, so there is no `engine/pitch.rs`. Not built: VEGAS's Ctrl+Shift+= reset, the Time stretch/pitch shift method list and the pitch shown on the event.
- The shuttle mixes the timeline at 1x and resamples the mix at the rate, read again each block, so the Rate slider and J / L change speed without restarting; backwards it mixes a quarter second of play at a time before what was heard (new decoders each chunk). No loop while shuttling.

**Scope**
- Audio follows the velocity envelope.
- J/L shuttle and rates other than 1 are audible.
- Pitch Up/Down by semitone, cent and octave (`Clip.pitch_cents`, as VEGAS "Pitch shift" for audio events; `-af rubberband` is not in the build, so use asetrate plus atempo, or the internal resampler).

**Design**
- New `engine/resample.rs`: a windowed-sinc polyphase resampler, 32 taps, f32, with a per-block ratio.
- `AudioStream` reads at rate 1 and the resampler consumes at the envelope ratio. Reverse uses the existing areverse buffer.
- Pitch without tempo change: phase-vocoder `engine/pitch.rs` (WSOLA is simpler: about 200 lines).
- Shuttle: `Player::start` gains a rate argument; the mixer clock advances by the rate.

**Files:** `engine/mix.rs`, `engine/resample.rs` (new), `engine/pitch.rs` (new), `audio.rs`, `app/commands.rs` (tick_shuttle), `ui/menus.rs`.

**Acceptance tests**
- A 1 kHz sine at 2× velocity measures 2 kHz (zero-crossing count).
- Pitch +12 semitones measures 2 kHz at the same duration.
- Shuttle 2× for 1 s advances the clock 2 s.
- Resampler SNR above 90 dB on a 1 kHz sine at ratio 0.9.

---

### R5: Track groups and track visibility (M)

**Status: DONE.** Built as designed below, plus the item R0 assigned (the audio track header's Cut / Copy / Paste / Delete: the Edit commands, as in VEGAS, so Delete there deletes the selected tracks), except:
- A group is a header row (22 px: ▾ / ▸, name, M and S) above each run of its tracks; it may hold video and audio tracks. A click on it selects the group's tracks, a double-click on the name renames it, ▾ collapses it. Group Selected Tracks gathers the tracks of each kind under the first of them (or just after a group that one would split) and names the group "Track Group n". A track inserted, duplicated or dragged between two tracks of a group joins it; one dragged away from the rest of its group leaves it; Ungroup moves a track from the middle of its group to just below it (`edit::tidy_track_groups`, `move_tracks`).
- Group Mute and Solo are the group's own switches, as in VEGAS: a muted group silences its tracks over their own Solo, a soloed group plays them over their own Mute (`Timeline::switches`, used by the video plan, the audio plan and the dimmed events). Mute / Solo Track Group (Ctrl+Alt+Z / Ctrl+Alt+X) toggle the groups of the selected tracks, one undo step. Collapse, Expand and Hide are view changes (no undo step), like track heights.
- Hide is on the audio track header, where VEGAS's menu has it; it hides the clicked (or every selected) track's row, which still plays. The way back, Show Hidden Tracks (n) in the track list's empty-area menu, is Milan Pro's own. A hidden track leaves the track selection.
- Not built: dragging a group by its header, the 10 px indent of member headers (VP23) and events drawn on a collapsed group's row (VEGAS 15 shows none either).

**Scope**
- Group Selected Tracks, Ungroup, Rename Track Group, Collapse/Expand, Mute/Solo Track Group (GroupMute, GroupSolo).
- Track Hide/Show.
- Track Fade Colors (with R4a; DONE there).

**Design**
- `Timeline.track_groups: Vec<TrackGroup { id, name, collapsed, tracks: Vec<String> }>`.
- The header draws a group bar (`ui/timeline/tracks.rs`). A collapsed group hides member rows; mute and solo apply to members. `visible` is exposed in the context menu.

**Files:** `project.rs`, `edit.rs`, `ui/timeline/tracks.rs`, `ui/timeline/mod.rs`, `ui/timeline/ctx.rs`, `app/commands.rs`.

**Acceptance tests**
- Group mute mutes 3 tracks in one undo step.
- A collapsed group still renders.
- Delete track removes it from its group; an empty group is dropped.

---

### R6a: Nested timelines, engine (L)

**Status: DONE.** Built as designed below (`engine/nested.rs`), except where VEGAS differs:
- A `.mnle` imports (Import Media, drag and drop, the Explorer) as a nested timeline, as VEGAS imports a `.veg`: `MediaItem.nested`, probed by loading the child (`nested::probe`); its length, frame size, rate and whether it has sound follow the child's edits (`nested::as_media`). A child with sound only is an audio item.
- The picture is the child's own frame (`plan::Nested::Frame`, at its frame rate, with its own motion blur), composed at child resolution (or the 1/2-1/8 the parent needs) through the same sources and caches, transparent where the child shows nothing (VEGAS's nested projects are). The parent's motion blur leaves it alone, as it does footage.
- The sound is the child's master bus (master FX, then master gain), as in VEGAS. VEGAS renders it to an audio proxy (`.sfap0`) that Tools › Rebuild Offline Nested Audio re-renders; Milan Pro mixes it live (`mix::spawn_nested`: a child mixer on its own thread, through ffmpeg when the event plays at another rate or pitch, or reversed), so it is never stale; Rebuild Offline Nested Audio (enabled) reads every nested timeline again (`nested::reload`). Normalize measures the mix.
- Children are cached by path and the projects they are inside, valid while every project file under them keeps its modified time; `engine::content_hash` includes those times (`nested::stamp`), so saving a child (here or elsewhere) replans the parent's preview and playback.
- A nested timeline more than 8 levels below the project, or one of the projects it is inside (A→B→A, or a project nesting its own file), is offline: it draws nothing and the preview says "<file>: nested timeline refers to itself" (or "nested more than 8 levels deep"), as for a missing file.
- Shortcuts: nested frames are never resampled to another rate (frame blend, Optical Flow); a child always quickfades its own bare event edges and ignores Bypass All Audio FX. (R6b added thumbnails, waveforms and the Trimmer view of nested events.)
- Tests: `nested::tests` (`a_nested_red_solid_renders_red`, `a_project_never_contains_itself`, `saving_the_child_reaches_the_parent`, `nested_sound_is_the_childs_master_bus`), `menus::tests::rebuild_nested_audio_works`, the old-project test, and `render::tests::nested_render_matches_flat` (ignored, ffmpeg: ProRes frames and WAV samples of a nested parent equal its flat equivalent's, a reversed nested event included).

**Scope:** a `.mnle` file as media. It is rendered through the engine, recursively, with a cycle guard.

**Design**
- `MediaItem.nested: bool` (path to `.mnle`).
- `plan::compile` loads the child project (cached by path and mtime) and compiles it as `Src::Nested(Arc<VideoPlan>)`, with a matching nested AudioPlan in the mixer.
- `compose` renders the child frame into a layer at child resolution.
- Depth limit 8; a self-reference is reported as an offline item, never a crash.

**Files:** `project.rs`, `engine/plan.rs`, `engine/worker.rs`, `engine/mix.rs`, `engine/composite.rs`, `media.rs`.

**Acceptance tests**
- A parent with a child containing a red solid renders red.
- A→B→A gives an offline item with the reason "nested timeline refers to itself".
- Editing and saving the child invalidates the parent cache (mtime).

### R6b: Nested timelines, UI and commands (L)

**Status: DONE.** Built as designed below, except where VEGAS differs:
- Create Nested Timeline (Alt+C, the V23 timeline toolbar button) works as in VEGAS 23: a parent never saved is saved first; the nested timeline is saved where asked, the name proposed being "<parent>_Untitled Timeline.mnle" beside the parent (numbered when taken); the selected events become one grouped video/audio event pair over the span they covered (one undo step); the new nested timeline opens. `edit::create_nested` (with `extract_to_project`) writes the child atomically before the parent changes, so a failed write leaves the parent as it was; a project never nests one it is inside. The child keeps the events' timeline positions (the nested event starts that far in), the project settings, busses and the Video Bus Track's motion blur; the master bus, Fade to Color and markers stay in the parent. The picture and sound stay the same: events of a kind on one track with nothing else over their span put the nested event on that track (its copy in the child is plain); otherwise it goes on a new plain track above them and the copies keep their own settings. A nested event's edge where none of the child's events plays across it gets no second quickfade (`NestedMix::sounds_across`), so the sound is sample-identical.
- Open Nested Timeline (Alt+N, toolbar, the V23 event menu) and Edit Source Project (event menus, Tools, Tools › Audio, Project Media's item menu) open the nested timeline in this window at the moment under the cursor; Open Parent Timeline (Alt+P, toolbar) goes back, asking to save a modified nested timeline (Yes / No / Cancel). VEGAS 23 switches between nested and parent in one instance like this; its Edit Source Project opens a second VEGAS window instead (not built: Milan Pro opens it in place). The parents wait on a stack with their undo history, selection and scroll; New, Open, Exit (and closing the window) inside a nested timeline go up first, each project asking to save. The title bar names the nested timeline, as VEGAS's does; the breadcrumb (parent › child, a click goes up) is Preferences › General "Show the parent timelines of a nested timeline above the track list", off by default (`Prefs.nested_breadcrumb`).
- R6a's gaps closed: nested events have thumbnails (composed by the engine, `nested::frame`, through the thumbnail threads) and waveforms (peaks of the child's master bus mix, `nested::peaks`), never ffmpeg on the `.mnle`, both made again once the child is saved; the Trimmer shows a nested item's picture, filmstrip and waveform. Still not built: frame-rate resampling of nested frames, and the parent's Quickfade / Bypass All Audio FX options inside a child.
- R43 closed the shortcut: the parents waiting on the stack are autosaved with the project shown and listed in the session marker, so each is recovered on its own.
- Tests: `nested::tests::create_nested_keeps_the_picture_and_undoes` (one-track and two-track nests are pixel-identical, one undo step restores, a failed write or a self-nest changes nothing), `edges_and_pictures_of_a_nested_timeline`, `app::nested::tests::open_parent_asks_to_save_a_modified_child`, `menus::tests::nested_timeline_commands_work`, and `render::tests::create_nested_renders_the_same` (ignored, ffmpeg: ProRes frames and WAV samples equal before and after Create Nested Timeline, also checks the nested waveform and picture).

**Scope:** Create Nested Timeline (Alt+C) saves the selected events into a new `.mnle` and replaces them with one nested event. Open Nested (Alt+N) and Open Parent (Alt+P) use a project stack with a breadcrumb in the timeline header. Transport buttons are enabled.

**Files:** `app/commands.rs`, `app.rs` (project stack), `ui/transport.rs`, `ui/timeline/mod.rs`, `edit.rs` (`extract_to_project`).

**Acceptance tests**
- Create Nested keeps the rendered output identical (frame hash before and after).
- Undo restores the events.
- Open Parent prompts to save a modified child.

---

### R7: Masks, blend modes, exposure, keying polish, motion extras (L)

**Status: DONE.** Built as designed below, plus the items R0 assigned (3D Source Alpha, Cut, Difference Squared, the Motion Blur Amount and Video Supersampling envelopes, Project Properties › Motion blur type), except where VEGAS differs:
- Compositing modes are VEGAS's 15 in its menu order, plus Custom… (opens the Compositors window). Soft Light, Exclusion, Hard Mix, Hue, Saturation, Color and Luminosity are not VEGAS modes: they are listed only with Preferences › Video › "Offer extra blend modes" (off by default), and always played. Formulas are W3C's (Hard Mix is Photoshop's, Difference Squared VEGAS's (Cb - Cs)²); Cut takes the source's alpha out of what is below. The opaque-backdrop integer path of the 9 old modes is unchanged; the rest use a float path.
- Masks follow VEGAS: each Pan/Crop path is Positive, Negative or Disabled (not add/subtract/intersect); positive paths add, negative ones cut out, only-negative paths keep the outside. `Clip.masks: Vec<Mask { path: Keyframes<Vec<[f32; 6]>>, mode, opacity, feather }>`, keyed event-relative (they move with trims and splits and paste with Pan/Crop). The Pan/Crop window's Mask row edits them: a click adds a point, a drag from it pulls out its curve, Ctrl+drag on a point pulls its handles out, the right-click menu deletes points and paths. Coverage is exact across each row on 4 sub-scanlines (`engine/raster.rs`); feather is the FX blur, centred on the edge (VEGAS's In / Out feather types are not built). Bezier Masking is an FX with up to 8 keyframable points (smooth or straight) edited in its parameter grid, not on the preview; Cookie Cutter has Circle, Square, Diamond and Star, feather, a border and Invert.
- Parent/child (VEGAS's way): Make Compositing Child joins a track to the nearest non-child track above, Make Compositing Parent takes it out (the track's ≡ menu). The children are composited together, then the parent over them with its own mode (Multiply (Mask) there masks by the parent's luminance times its alpha; a parent with no event at that time masks nothing), then the group goes through Parent Motion onto the tracks below with the Parent Composite Mode. One level of nesting (VEGAS allows more); a child shows a bar on its header instead of VEGAS's indent.
- Custom mode: compositor plug-ins (`PluginKind::Compositor`: Bump Map, Displacement Map, Height Map), with the layer above as the map; the View › Window › Compositors chooser drops them on a track header (or double-click for the focus track) and the FX window edits them (`FxTarget::Compositor`). VEGAS's Mask Generator compositor is not built.
- Exposure FX: stops in linear light (sRGB curves), plus offset and gamma. Chroma Keyer › Spill suppression is Milan Pro's own (0, off, by default); Show mask only was already there.
- Track Motion window rows Position, 2D Shadow and 2D Glow, each with its check box and keys (`Track.shadow` / `glow: Halo2D`): offset (shadow), blur, intensity, colour. 3D Source Alpha adds Z, X angle and Y angle (`Motion.z/rx/ry`), drawn through a perspective camera one frame width in front (an estimate: VEGAS's is not documented).
- Video Bus Track (View › Video Bus Track, Ctrl+Shift+B): a row docked under the tracks with the Video Output FX button and the Fade to Color, Motion Blur Amount (seconds) and Video Supersampling (0-8) envelopes (`MasterBus.video_envelopes`); its Fade Colors stay white over black (no picker yet). Motion blur blends sub-frames of what moves in the project (Track Motion, Pan/Crop, envelopes, FX, generated media) over the blur's length with the Motion blur type's weights (symmetric or trailing), not footage's own motion, as in VEGAS; Bypass Motion Blur (track ≡ menu) keeps a track still. Preview and render share `Worker::frame`, so they match (render_e2e checks a mask, Hard Light, a 2D Glow and motion blur).

**Scope**
- Pan/Crop Mask mode: Bezier paths with keyframes, feather, invert, add/subtract/intersect.
- A Bezier Masking FX and Cookie Cutter.
- Blend modes Hard Light, Soft Light, Dodge, Burn, Exclusion, Hard Mix, Hue, Saturation, Color, Luminosity.
- Parent/child compositing (VEGAS "Make Compositing Child").
- An Exposure FX (EV stops in linear light) and chroma key spill suppression and matte view.
- Track Motion 2D Shadow and 2D Glow.
- The Compositors window and the Video Bus Track (output FX lane).

**Design**
- `PanCrop` gains `masks: Vec<Mask { points: Keyframes<Vec<[f32; 6]>>, mode, feather, invert }>`.
- `engine/raster.rs`: a scanline polygon fill with 4×4 supersample coverage plus the batch-14 blur for feather.
- `composite::blend` gets the new modes (formulas from the W3C compositing spec).
- `Track.parent: bool` composites a child group into a scratch layer, then onto the parent with the parent's mode.

**Files:** `project.rs`, `engine/raster.rs` (new), `engine/composite.rs`, `engine/vfx.rs`, `engine/plugins.rs`, `ui/pancrop.rs`, `ui/trackmotion.rs`, `ui/timeline/tracks.rs`, `interchange.rs` (map VEGAS modes).

**Acceptance tests**
- Mask raster area of a unit square equals 1 ± 0.5%.
- Feather 0 gives a hard edge; invert flips coverage.
- Each blend mode matches a hand-computed pixel.
- Exposure +1 doubles linear values.
- Old projects load with no masks.

---

### R8a: Audio buses and routing, engine (L)

**Status: DONE.** Built as designed below, plus the items R0 assigned (track header › Output and Pan Type), except where VEGAS differs:
- Busses and assignable FX chains are one type (`project::Bus`) in `Timeline.audio: AudioSettings { buses, fx }` (both skipped when empty, so old projects are written as before). `Track.out: Out { Master | Bus(id) }`, `Track.sends: Vec<AudioSend { to, level, pre }>` (no entry = -inf) and `Track.pan_type`. A track goes to the master or a bus, never to an FX chain (tracks send to those, as in VEGAS); a bus or chain goes to the master or a bus.
- Sends follow VEGAS: a new bus send is pre-volume and pre-mute, a new FX send post-volume; sends are centre-panned by the track's Pan Type (VEGAS without Link to Main Track Pan). A muted or un-soloed track stays in the plan only for its pre-volume sends.
- The plan's busses are ordered by their longest path to the master, so each mixes after everything that feeds it (`plan::bus_order`); a loop in a hand-edited project is cut there. `edit::set_bus_out` refuses a loop with a message ("Bus B cannot be routed to Bus A: that would make a loop").
- Meters: `mix::Meter` per track and bus (post-fader peak since read, RMS over 50 ms), in an id map the UI thread rebuilds with each plan (`Player::meters`), so the mix thread takes no lock; a render has none. R8b reads them.
- Pan Types (Add Channels, Balance 0/-3/-6 dB, Constant Power, Film = Constant Power in stereo) are `PanType::matrix`; Balance (0 dB Center) is the old law, bit for bit.
- UI in this batch: Project Properties › Audio › Number of stereo busses (0-26; fewer removes the last ones, and what went to them goes to the master), Insert › Audio Bus / Audio Assignable FX... (the new chain opens in the FX window, where plug-ins are added: VEGAS opens the Plug-In Chooser first), Tools › Audio › Remove Assignable FX... (a tick list), track header › Output / Pan Type, and the multipurpose slider (third header row when the project has a bus or chain; its label picks the target and Pre / Post Volume). A bus's own fader, pan, mute, FX button and Output (with `set_bus_out`) and its envelope lanes are R8b's Mixing Console and Audio Bus Tracks. Master bus mode stays Stereo (R10a); Audio Input Bus moved to R9.

**Scope**
- Project audio settings: master bus mode (stereo now; 5.1 in R10a) and the number of busses (0-26).
- Bus tracks; a track output assignment (Master or Bus X); bus FX, volume, pan and mute envelopes.
- Assignable FX (sends) with pre/post fader.
- Per-track and per-bus peak/RMS meter data.

**Design**
- `Timeline.buses: Vec<Bus { id, name, gain, fx, envelopes, out: Out }>` and `Track.out: Out { Master | Bus(id) }`, plus `Track.sends: Vec<Send { to: id, level, pre }>`.
- `AudioPlan` becomes a DAG evaluated in topological order, with a cycle check at compile.
- The mixer keeps one buffer per node. Meters: `Arc<[AtomicU32; 2]>` per node, peak and RMS over a 50 ms window, read by the UI.
- `ProjectSettings` must not gain fields (the `welcome.rs:154` literal). Put bus settings on `Timeline.audio: AudioSettings`.

**Files:** `project.rs`, `engine/plan.rs`, `engine/mix.rs`, `audio.rs`, `edit.rs`, `settings.rs` (audio tab).

**Acceptance tests**
- Track→Bus A (-6 dB)→Master halves the amplitude.
- A send at 0 dB pre-fader with the track fader at -inf still feeds the bus.
- A cycle Bus A→B→A is rejected with a message.
- Render equals preview (the same mixer).

### R8b: Mixing Console, track meters, master buttons (L)

**Status: DONE.** Built as designed below, plus the items R0 assigned, except where VEGAS differs:
- Meters: once a frame the app folds every `mix::Meter` and the master tap into a `MeterView` per id (`mixer::feed_levels`, in egui memory); track headers, bus track headers, console strips and the Master Bus window read it, so the mix thread still only writes atomics. Peak bars fall at 24 dB/s, peak holds stay 2 s, clip lamps and readouts latch until clicked. The master tap now also gives a 50 ms RMS for its VU. Track and bus meters lead the sound by the device queue (about 128 ms; shortcut noted in `feed_levels`).
- VU: VEGAS has a separate VU Meters control region in the console (Properties and Layout › Show Control Regions), so the VU ballistics are that region, off by default; the peak meters stay the default everywhere.
- Mixing Console (`ui/mixer.rs`, `Panel::MixingConsole`, View › Window › Mixing Console, Ctrl+Alt+5 / VP23 Ctrl+Alt+6): VEGAS's toolbar (Views › Channel List; Properties and Layout › Audio Properties..., Show Channels, Show Control Regions, Channel Width; Downmix Output; Dim Output; Insert Audio Track, Insert Assignable FX, Insert Bus; Insert Input Bus is R9), the Channel List (strip check boxes, for the session), and strips for audio tracks, busses, assignable FX chains and the master: Insert FX (click opens the chain), Sends (a fader per bus and chain, right-click Pre / Post Volume), I/O (output: Master or a bus; a bus loop is refused with `set_bus_out`'s message), VU Meters, Peak Meters, Faders (Arm, Mute, Solo, Invert Phase on tracks; Mute on busses; the master's Mute is Mute All Audio; Pan; the fader; a meter beside it when Peak Meters is hidden), and the label (click selects, double-click renames). Every control edit is one undo step. Layout is saved in the preferences (`Prefs.console`).
- Master Bus window: Master Bus Properties opens Project Properties › Audio (`Command::AudioProperties`), Downmix Output plays (L+R)/2 on both sides, Dim Output (and `DimMaster`, Ctrl+Shift+F12) lowers the output 20 dB, both gliding, playback only (never rendered); Mixing Console opens the window. Its Mute is Mute All Audio; its Solo and automation badge say why they do nothing (R57 preview bus; R10a, moved there by R8c).
- Audio Bus Tracks (View › Audio Bus Tracks, B; `Prefs.bus_tracks`): one 64 px row per bus and chain under the tracks (with the Video Bus Track), at most half the timeline (whole rows; shortcut: no scrolling of that area). Header: name, Mute, FX, Vol, Pan, meter; lane: Volume / Pan / Mute envelopes (right-click › Insert/Remove Envelope, point drags, double-click adds, point menu deletes). No Master bus track (it would need master envelopes in the engine; R10c added both).
- Track header (R0 items): Show Output Meters, Use Vertical Meters (off draws L/R bars across the top row) and Channel Width (meter width) work and are saved in the preferences. Ripple: VEGAS's fourth mode, Affected Tracks, Markers, and Regions, is `RippleMode::AffectedMarkersOnly` (also Post-Edit Ripple); the bus-track modes (Affected Tracks, Bus Tracks, Markers, and Regions; All) also ripple the bus tracks' envelope keys.
- View › Mixer Preview Fader is relabelled R57: the preview bus it shows plays the Explorer's previews, which Milan Pro does not have.
- Not done: bus Solo, a Master bus track, the strips' recording input selector (R9: DONE), the console keys (F2, D/N/W, F/I/H/M/S: they clash with global keys while the pointer is over the console).

**Scope**
- Mixing Console window (strips: input/FX/sends/pan/fader/meter, mute, solo, arm).
- Live track meters in track headers (unlit-meter label removed).
- VU ballistics option (300 ms integration, -20 dBFS = 0 VU reference) beside peak.
- Audio Bus Tracks view (bus lanes in the timeline for envelopes).
- Master Bus buttons: Properties, Downmix (mono check), Dim (-20 dB), Mixing Console.
- DimMaster command.

**Files:** `ui/mixer.rs` (new), `ui/master_bus.rs`, `ui/timeline/tracks.rs`, `ui/timeline/mod.rs`, `ui/dock.rs` (Panel::MixingConsole), `ui/menus.rs`, `app/commands.rs`.

**Acceptance tests**
- VU ballistics unit test: a 1 kHz step reaches 99% in 300 ms ±10%.
- Peak hold 2 s.
- Every master-bus button works and its label is gone.
- Screenshot harness of the Mixing Console.

### R8c: Automation write and FX parameter envelopes (M-L)

**Status: DONE.** Built as designed below, plus the items R0 assigned, except where VEGAS differs:
- Model: `Track.automation` / `Bus.automation` (`AutoMode`: Off, Read (the default, so old projects play as before), Touch, Latch) and `auto_controls` (Show Automation Controls); FX automation in `Fx.auto` (parameter key → keys in the parameter's units, timeline time), which `plugins::eval` plays over the parameter's own value. Instead of `EnvKind::Fx { fx_id, param }` the FX envelope is `EnvKind::FxParam` lent out by an owner string (`{track or bus}/fx/{plug-in}/{parameter}`, `edit::fx_owner`) as 0..=1 of the parameter's range (`plugins::to_unit`, log for wide ranges): the kind stays `Copy`, every envelope edit (drag, point menu, Thin, Flip, ripple) works on it unchanged, and an older build reads nothing it does not know. All new fields are skipped when default.
- Modes follow VEGAS: Off leaves the track's (bus's) envelopes and FX automation out of the plan and its controls trim; Read plays them and an automation control follows them without editing; Write (Touch) writes while the control is held, Write (Latch) from the first move until playback stops (`ui/automation.rs`). Stopped, a move sets the envelope at the cursor (adding the envelope if there is none, as VEGAS). A pass is one undo step, punched in and out with a step, so the envelope outside it is untouched; a point at most every 20 ms and 0.5 dB (half a percent of the lane for the others). Writes are UI-thread edits heard through the plan swap; the mix thread is unchanged.
- Automation controls: with Show Automation Controls on and a mode other than Off, the Vol and Pan sliders of the audio track header, the bus track and the Mixing Console strips show and write the Volume / Pan envelope (their thumbs take the envelope's colour). An automated FX parameter's control in the FX window follows the mode alone (Read shows it greyed).
- FX Automation Envelopes... (track header, lane, bus track, and the FX window's Automation... button) opens the FX Automation chooser: each plug-in's numbers, choices and switches with a check box each. E / Shift+E select the focus track's next / previous FX envelope (drawn as hovered, its name in the status bar). The envelope menus' Properties opens an FX envelope's plug-in; the other kinds have none (says so on hover).
- Video track FX are keyframed in VEGAS, so their "envelopes" are View › Show OFX Effect Envelopes (Beta, VP23, off by default): a video track's keyed FX numbers drawn and edited as envelopes. Automation modes are on audio tracks and busses only (as the VEGAS track menu).
- Audio event Insert/Remove Envelope › Volume / Pan / Mute (event-relative; Pan is Balance 0 dB) play in the mixer and render.
- Audio FX presets: built-in presets for each audio plug-in (named in VEGAS's style; VEGAS's own lists live inside its DirectX DLLs and were not read out, shortcut noted in `plugins.rs`); user presets already worked. A preset pick keeps the plug-in's FX envelopes.
- The Master Bus window's automation badge moved to R10a (the master has no envelopes or bus track yet), and from there to R10c (done).
- Not done: "Smooth and thin automation data after recording" (the writes are thinned as they go), "Set controls to default values when automation is turned off" (Off keeps the static values, VEGAS's default behaviour for FX), automation of sends and of Mute, Lock Envelopes to Events for FX automation.

**Scope**
- FX parameter automation envelopes on audio and video tracks and buses (`NextFxEnvelope`, `PrevFxEnvelope`, "FX Automation Envelopes...").
- Automation modes Off/Read/Write (Touch)/Write (Latch): fader, pan and FX knob moves during playback write envelope points.
- Audio FX presets (skipped in batch 16).

**Design**
- `EnvKind::Fx { fx_id, param }`, serialised as a tagged variant (keep the `#[serde(other)]` fallback).
- The plan evaluates FX params from the envelope when one exists.
- Touch writes points while the control is held (thinned to one per 20 ms with a 0.5 dB tolerance). Latch keeps writing until stop.

**Files:** `project.rs`, `engine/plan.rs`, `engine/afx.rs`, `ui/fx_window.rs`, `ui/timeline/envelopes.rs`, `ui/timeline/ctx.rs`, `ui/timeline/tracks.rs`, `settings.rs` (afx presets).

**Acceptance tests**
- A compressor threshold envelope from -10 to -30 dB changes gain reduction over time (mixer test).
- A touch write of a linear fader move gives at most 60 points per second, with no point outside the hold.
- An audio preset save/load round-trip.

---

### R9: Audio device layer on cpal; latency compensation; input monitoring (L)

**Status: DONE.** Built as designed below, except where VEGAS differs:
- Output: rodio is gone. `audio.rs` opens the device with cpal (WASAPI shared) on a "milan-audio-device" thread, or with `wasapi.rs` (WASAPI exclusive, event driven, MMCSS "Pro Audio", 48 kHz or the mix rate, f32/32/24/16-bit). One callback (`Out::render`) serves both: it takes the session's blocks (`Feed`), resamples 48 kHz to the device rate (linear, as rodio did; bit exact at 48 kHz), adds the input monitor, applies the master (gain glide, Dim, Downmix, meters) and maps stereo onto the device's channels. It takes no lock and frees nothing: spent blocks, feeds and monitor plans go back to the UI thread (`Trash`). The stream stays open while stopped (as rodio's did), so monitoring works without playback; a lost device (error callback) or one that stops calling back is reopened every 2 s, playing or not.
- Preferences › Audio Device follows VEGAS's tab: Audio device type (Microsoft Sound Mapper = the Windows default, the default as in VEGAS; Windows Classic Wave Driver = a chosen device; Windows Audio (Exclusive Mode), Milan Pro's, not VEGAS's; ASIO greyed as not feasible), Default stereo and front playback device, rear and center/LFE (greyed: R10a monitors a surround project as its stereo fold-down), Buffer size (VEGAS's Advanced › Buffer size: Default, 64-4096 samples), Default audio recording device, Recording latency offset, and the output, input and monitoring round-trip latency the devices report (for the user's loopback check). Changes apply at once (a recording keeps its device until it stops). Old preferences keep the Windows default device at its default buffer. The Audio tab (VEGAS's Audio preferences) is relabelled R49, where its scope already was.
- Latency: the playhead is the feed clock less cpal's reported `playback - callback` (WASAPI exclusive: the buffer plus `GetStreamLatency`), carried on between callbacks from a seqlock stamp (`Stamp`, `Heard`) and held on underruns; at the start it waits at the cursor until the first sound is heard. Track and bus meters still lead by the mixer queue (R8b's note).
- Input monitoring (VEGAS's Record Input › Input Monitoring Off / On / Auto, Off by default; Auto = while stopped and while recording): the track header's Input ▸ and the console track strip's Record Input button set it (one undo step, `Track.monitor`). The recording input feeds a lock-free SPSC ring (`Ring`) from its callback; the output callback runs it through each armed, monitored track's FX (values evaluated on the UI thread, automation bypassed as VEGAS does), volume, pan (Pan Type), phase and output gain, and adds it before the master. Plug-in state survives value changes (`Chain::run_with`). A dry signal is recorded, as in VEGAS. Monitoring needs the input and output at the same rate (the status bar says why otherwise).
- Insert › Audio Input Bus (and the console's Insert Input Bus): up to 26 input busses (`AudioSettings.inputs`, "Input Bus A"...), console strips with Insert FX (pre-fader), Record Input, Output (Master or a bus), meters, Mute, Pan and fader, Show Channels › Input Busses, label › Delete. They are heard live through the monitor while the project has one (the input opens for them), never rendered.
- Harness: the screenshot harness opens no audio device (`Player::new(false, ..)`, no capture); `MILAN_DIALOG=input-bus` shows an input bus and an armed Auto track in the console.
- Not done: per-track record inputs (mono/left/right, input busses as a track's record input, recording a wet input bus), "Automatically detect and offset for hardware recording latency", the busses' FX and pan on monitored inputs (they run ahead in the mix thread; shortcut noted in `audio::route_gain`), following a change of the Windows default device while running. Not verified here: anything on a real device (the user's PC was not to make sound), so the exclusive path, the latency figures and the loopback and click-track checks are the user's.

**Scope**
- Output device choice (Preferences > Audio Device), buffer size, WASAPI shared/exclusive.
- Insert › Audio Input Bus (assigned by R8a: a hardware input routed through the mixer is input monitoring).
- The playhead compensated by the reported output latency (removes the 10-20 ms lead).
- Low-latency input monitoring through track FX for armed tracks, with a per-track monitor toggle.
- ASIO stays a documented limitation: the cpal `asio` feature needs the Steinberg ASIO SDK, whose licence cannot be redistributed. Record this in FORMAT_SUPPORT/COMPATIBILITY.

**Design**
- Replace rodio's `OutputStream` with a direct cpal 0.15.3 stream (already in the lock via rodio; add `cpal = "0.15"` as a direct dependency).
- `Feed` keeps its clock; `position()` subtracts `OutputCallbackInfo::timestamp().playback - callback`.
- Monitoring: the input callback goes to an SPSC ring (std `Mutex<VecDeque>` is fine at 256 frames), then the armed track chain, then the output. The target round trip is under 20 ms at 128-frame buffers.
- Keep rodio only if something still needs it; otherwise drop the dependency.

**Files:** `audio.rs`, `record.rs`, `engine/mix.rs`, `ui/dialogs/prefs.rs`, `settings.rs`, `Cargo.toml`.

**Acceptance tests**
- `stalled()` watchdog tests stay green.
- Clock compensation unit test with a fake timestamp.
- Manual (user): a loopback cable measures round-trip latency, which is printed in Preferences.
- Manual: a click-track A/V offset under 5 ms.

---

### R11: Render templates, custom settings, more formats, render logs (L)

**Status: DONE.** Built as designed below, except:
- Hardware encoders are not offered: R22 owns their runtime test, and R11 lists no encoder that a test has not passed. The codec lists are tested software encoders (`render::VCODECS`, `ACODECS`); every codec × pixel format and format × audio codec pair they allow was encoded with the bundled FFmpeg.
- Image sequences render as `<name>_000000.<ext>` files, staged in a `.partial` folder and moved in when complete. DNxHD (fixed rasters) is a Customize profile, not a built-in. DNxHR cannot be interlaced (FFmpeg 9 refuses).
- R0's Project Properties items are built: Field order, Pixel aspect ratio, Output rotation, Full-resolution rendering quality, Deinterlace method, Adjust source media, audio Bit depth, and the preview's Simulate Device Aspect Ratio. Bit depth applies to renders that follow the project; recordings stay 32-bit float (a superset).
- VP23 Filter offers Match project video / audio settings and Show all formats; View offers Thumbnails and Details; the Render options are a gear menu. Favorites and Recent are kept in the preferences.
- `docs/FORMAT_SUPPORT.md` is created with the render table; R37 adds the decode half.

**Scope**
- Customize Template dialog: resolution, frame rate, PAR, field order, codec (from the R22 capability list or FFmpeg's encoder list), rate control (CRF/CQ/VBR/CBR with bitrate), preset/speed, pixel format/bit depth, audio codec/bitrate/sample rate/channels.
- User templates saved in prefs; built-in delivery templates: YouTube 1080p/2160p, Vimeo, Web 720p, ProRes 422 HQ/4444 (alpha), DNxHR LB/SQ/HQ/HQX/444, CineForm, FFV1 MKV archive, VP9 WebM, AV1 (SVT), MPEG-2 TS, AVI (MJPEG), image sequences PNG/TIFF/EXR/DPX, FLAC, ALAC, AAC, Opus, Vorbis, AC-3/E-AC-3.
- Render options: stretch versus letterbox, markers into the file (MP4 chapters via `-f ffmetadata`), project output rotation.
- A per-render log: the full FFmpeg command, version, timings and stderr, in `%LOCALAPPDATA%\MilanPro\logs\render\`, with a "Show log" link.

**Design**
- `render::Preset` becomes `Template { name, container, video: Option<VideoEnc>, audio: Option<AudioEnc>, w, h, fps, ... }`, serde-able. Built-ins live in a `const` table.
- `engine/render.rs::start` takes a Template and scales and conforms the composite to its size and rate (`plan` at template fps).
- Validation: codec × container × pixel format is checked against the R22 capability table before Render is enabled.

**Files:** `render.rs`, `engine/render.rs`, `ui/dialogs/render.rs`, `ui/dialogs/template.rs` (new), `settings.rs`.

**Acceptance tests**
- An args unit test per template.
- An ignored e2e renders 2 s with each built-in template, ffprobes the output (codec, size, fps, channels, duration ±1 frame), and writes the results into `FORMAT_SUPPORT.md`'s render table.
- An invalid combination (VP9 in MOV) is refused with a message.

### R12: Background render, render queue, batch render, Render to New Track (L)

**Status: DONE.** Built as designed below, except:
- As in VEGAS the Render dialog stays modal with its progress by default; Render Options › Render in the background (a preference, off) makes it a window that Render closes, and Add to Render Queue does that for one render. Batch Render and Render Audio Tracks (Tools › Scripting, as VEGAS's bundled scripts) show their renders in the Render Queue window, modal unless Render in the background is on; closing it cancels the batch, as the script's Cancel does. Tools › Render Queue... is the extra window.
- Pause holds a waiting job; a render that has started can only be cancelled (holding the encoder mid-stream is not done). Keep in Queue at Exit stops the running render; it starts again from its beginning at the next start.
- Every render (the Render dialog's too) goes through the queue, whose snapshot is the project serialised to `%APPDATA%\MilanPro\queue\<id>.mnle`. One window owns `queue.json` at a time (`queue.lock`); a second window keeps its queue in memory.
- The guard's choices apply when Milan Pro really closes, after the save prompt (a cancelled save prompt leaves the renders running). Install and restart offers only Wait or Not Now.
- Render to New Track places the file on a new first video track and a new first audio track (grouped), at the start of what was rendered, in the project it was rendered from; image sequences are refused for it.
- R0's File > Real-Time Render moved to R59 (external hardware FX).
- Thread priority: the encoder process only (`proc::below_normal`); the render's composing threads stay normal (R19).

**Scope**
- The render dialog becomes non-modal: closing it leaves the job running. Status-bar progress uses `status.rs`'s progress and Cancel link.
- Render queue window: add the current settings, reorder, pause or cancel per job, sequential execution. The queue persists in `%APPDATA%\MilanPro\queue.json`; after a crash, unfinished jobs show "interrupted" with Retry.
- Batch Render: one job per region, per selected template, or per track (like VEGAS's `Batch Render.cs` / `Render Audio Tracks.cs`).
- Render to New Track (Ctrl+M).
- Encoder process priority BELOW_NORMAL (`windows-sys SetPriorityClass`).
- Updater, exit and close guards: "A render is running: wait, cancel, or keep it running in the queue for next start".

**Design**
- `RenderQueue { jobs: Vec<QueuedJob { template, out, range, project_snapshot_path, state }> }` in `app.rs`. The snapshot is the project serialised to a temp `.mnle`, so later edits do not change a queued job.

**Files:** `app.rs`, `app/commands.rs`, `render.rs`, `engine/render.rs`, `ui/dialogs/render.rs`, `ui/render_queue.rs` (new), `ui/status.rs`, `proc.rs` (priority), `ui/menus.rs`.

**Acceptance tests**
- Queue persistence round-trip; a job marked interrupted after a simulated kill.
- Batch by regions gives N jobs with region ranges and names.
- Exit with a running job shows the guard.
- Render to New Track places the file at the selection start on a new track.

---

### R13: Built-in layouts and dock commands (M)

**Status: DONE.** Built as designed below, except:
- The layout files are binary (base64 in XML); their window records were decoded by window id (32219 Explorer ... 32229 Video Scopes, 32238 Mixing Console), tab order, active tab and column rectangles. Classic is the VEGAS Pro 14 layout: the dock area below the timeline, [Project Media group] · Trimmer 610 · Video Preview 613 · Master Bus 125, height 449. Audio Mixing: [Mixing Console + the default group] · Video Preview 511, height 438 (an unidentified second tab behind Video Preview, id 32231, is left out). Color Correction: [group] · Video Scopes 539 · Video Preview 539 · Master Bus 127, height 438 (R16b added the Video Scopes column); VP23 brings Color Grading to the front [I]. Source Window (VP23) puts the Trimmer and Video Preview side by side; Timeline Top (VP23) puts the default windows in the bottom area [I: no reference shows them].
- The dock model gains areas (`dock::Area`, `Group.area`, default Top so old layouts load as before): VP23 has four, VP16 one that sits above the timeline or, with "Display timeline at bottom of main window" off (the Classic layout), below it. Layouts save each area's size and which are minimised. Windows move to another area through the tab menu (Dock in Top/Bottom/Left/Right Docking Area, VP23); dragging to an edge comes with R14's drag-out. Groups in the left and right areas are columns too.
- F11 minimises every area that has windows (again restores them); Maximize Timeline also collapses the track list. Next/Previous Window cycle the timeline and every window of the shown areas, bringing each to the front; a press in a window or the timeline moves that focus. Focus to Timeline also takes the keyboard from a text field.
- Organize Layouts works on the ten slots themselves (Milan Pro has no layout files): Move Up/Down, Rename, Delete (VEGAS's Clear and Delete Layout in one), Activate; OK writes them. Reload Selected Layout goes back to the built-in or user layout last loaded or saved.
- The overflow test uses every window (12) in a 400 px group: there are not 30 distinct windows.

**Scope**
- Layouts Audio Mixing, Color Correction, Classic (VP14) Default, Source Window and Timeline Top. Reproduce them from VEGAS's `Standard Layouts\*.VegasWindowLayout`; if those files are binary, use the VEGAS_UI_SPEC descriptions.
- Organize Layouts (rename, delete, reorder slots); Reload Selected Layout.
- ToggleDocks (F11), Top/Bottom/Right dock, CollapseTrackList, MaximizeTimeline, Next/Prev Window (Ctrl+Tab), FocusTimeline (Alt+0).
- The dock tab overflow scroller (`dock.rs:357` shortcut).

**Files:** `ui/dock.rs`, `ui/workspace.rs`, `settings.rs`, `ui/menus.rs`, `app/commands.rs`, `ui/dialogs/organize_layouts.rs` (new).

**Acceptance tests**
- Each built-in layout opens the panels VEGAS shows (table test).
- ToggleDocks twice restores the layout.
- 30 tabs in one group scroll.

### R14: Floating windows, multi-monitor, external preview (L)

**Status: DONE.** Built as designed below, except:
- Floating windows are immediate viewports (`show_viewport_immediate`), not deferred ones: they draw with the app's state like docked windows. Each floating window holds one window (VEGAS lets a floating window take tabs). It has a native title bar and the dock toolbar and tab; its tab moves the window as the title bar does (the OS's own move). It stays above the main window while Milan Pro is the active application (VEGAS's floating windows are owned windows) and has no taskbar button. Keys pressed in it reach the keymap. Closed (×, View › Window, Close), a floating window reopens docked.
- Undock is in the tab menu (spec §5.3); a tab dropped anywhere but a group or an edge band floats there. The edge bands (32 px along the main window's client area) dock in that side's area: all four in VP23, the one area in VP16. A floating window let go over a group or an edge band after a mouse move docks under Preferences › Display › "Allow floating windows to dock" (default off in VP16, on in VP23, as the installs show it; Ctrl inverts it). No outline is drawn while the OS moves a floating window.
- `FloatWin { panel, rect: [x, y, w, h] in physical pixels, monitor: "\\.\DISPLAYn" }`. Windows are placed in their display's own scaling and corrected for a few frames (Windows creates a window in the primary display's scaling first), so a 150 %/100 % mix keeps both position and size; egui scales the content per window.
- Preview on External Monitor is a borderless, never-activated viewport that goes full screen on the Preview Device display once it is there; Esc on it (as in VEGAS) or Alt+Shift+4 turns it off; not kept between sessions (VEGAS asks again each start). The Preview Device page has the Windows Graphics Card device with Display adapter (Automatic = the first secondary display), Scale output to fit display (off: 1:1), Display frames in Video Preview window during playback and Use project output rotation setting; Identify Displays, deinterlace and studio-to-computer RGB stay greyed (R49), vertical sync and GPU display are always on, IEEE 1394 and Recompress edited frames are not planned (R59).
- The screenshot harness opens every floating window and the external preview off-screen (`main::offscreen`), never active and never full screen: `MILAN_DIALOG=float`, `external-preview`, `prefs-preview`.
- Not checked by hand on two monitors or a 150 %/100 % mix (needs the user).

**Scope**
- Drag any dock tab out to a floating native window (egui viewports), dock it back, and save floating rectangles and monitors in layouts.
- Preview on External Monitor: a borderless full-screen viewport on the chosen monitor, from the Preferences > Preview Device page (monitor list, scaling).
- Per-monitor DPI correctness when moving windows.

**Design**
- eframe 0.31 `Context::show_viewport_deferred` works with the glow backend on Windows.
- `Layout` gains `floating: Vec<FloatWin { panel, rect, monitor }>`.
- Enumerate monitors with windows-sys `EnumDisplayMonitors` and `GetMonitorInfoW` (add feature `Win32_Graphics_Gdi`).

**Files:** `ui/dock.rs`, `ui/workspace.rs`, `app.rs`, `settings.rs`, `ui/preview.rs`, `ui/dialogs/prefs.rs`, `Cargo.toml` (windows-sys feature).

**Acceptance tests**
- Layout serde round-trip with floating windows.
- A floating window on a removed monitor reopens on the primary one (unit test on the rect clamp).
- Manual (user, two monitors): external preview full screen; 150%/100% DPI mix.

### R15: Customize Keyboard and Customize Toolbar (M)

**Status: DONE.** Built as designed below, except:
- Keyboard maps follow VEGAS rather than JSON import/export: Save As writes the keys as a named .ini map in %APPDATA%\MilanPro\Keyboard, the Keyboard map list loads one (a map copied into that folder is listed), Delete removes one, and [Default] restores the table. A map holds the changed keys (maps saved by 0.8.0 keep their VEGAS 23 section). The command name saved is the `Command` variant's derived `Debug` text, not a hand-written `name()`.
- VEGAS's per-window contexts (Global, Timeline, Trimmer...) are not built: Milan Pro's keymap is global. Keys are single strokes; the table's Alt+D chords can be removed or taken, not recorded.
- The Shortcut keys box says which command a pressed key belongs to ("Shortcut currently used by"); Add then takes it from that command, so no key does two things. Reset gives the selected command its default keys back, taking them from whichever commands had them.
- The toolbar dialogs are VEGAS's (the Windows toolbar dialog): changes show at once, Reset restores VEGAS's buttons, Close ends it. The main toolbar can take the edit buttons of the timeline toolbar; the play buttons stay on the timeline toolbar, which has the play state. Toolbar tooltips show the user's keys; the play buttons' tooltips keep their fixed text.

**Scope**
- Options > Customize Keyboard: a command list (searchable), key capture, conflict warning, reset one/all, per-look profiles, import/export (JSON).
- Options > Customize Toolbar and Customize Timeline Toolbar: add, remove and reorder buttons.

**Design**
- `Prefs.keys: BTreeMap<String /*look*/, BTreeMap<String /*Command name*/, Vec<String>>>` overrides `keymap::TABLE`.
- `keymap::keys(c, look)` checks the overrides first.
- `Command` gets a stable string name (`#[derive(strum-like)]` by a hand-written `fn name()`, with no new crate).

**Files:** `ui/keymap.rs`, `settings.rs`, `ui/dialogs/customize_keys.rs` (new), `ui/dialogs/customize_toolbar.rs` (new), `ui/toolbar.rs`, `ui/transport.rs`, `ui/menus.rs`.

**Acceptance tests**
- An override replaces a default; a conflict is detected; reset restores.
- Unknown command names in prefs are ignored, not fatal.
- Menu shortcut text follows the overrides.

### R16a: Edit Details and Project Notes (M)

**Status: DONE.** Built as VEGAS has it, which differs from the design below in places:
- Edit Details (`ui/edit_details.rs`, View › Window, Alt+6 in VEGAS 16 and Ctrl+Alt+4 in VEGAS 23) shows VEGAS's six views: Audio CD Track List, Commands, Events, Markers, Regions and Selected Events (VEGAS has no Media view; Project Media lists media). Events have Track, Start, End, Length, Number of Takes, Active Take Name, Take Start, File Path and the Select, Mute, Loop, Lock and Normalize checks. A heading click sorts (again: descending), a double-click edits a cell, Enter or a click away applies it, Esc cancels. Start moves the event (and its group) as a Move drag, End and Length trim the right edge as a trim drag, Track moves it to another track of its kind; locks, Ignore Event Grouping, Auto Ripple (or Post-Edit Ripple later) and auto crossfades apply as on the timeline, one undo step each. The Audio CD Track List and Commands views list the markers R18a inserts.
- Project Notes (`ui/project_notes.rs`, VEGAS 23, Alt+6) is VEGAS's list of notes, not one text field: Add New Note (lower left) adds a note at the cursor; each note has its position (a click goes there), Resolved and its text. Notes are saved with the project (`Project.notes`) and are not undone by Undo, so nothing typed is lost to an undo.
- Also built: region Rename (region start tag menu) and the Project Properties › Summary page (Title, Artist, Engineer, Copyright, Comments; `ProjectSettings.summary`).
- Not built (VEGAS extras): column drag to reorder or hide, view presets, the right-click menu, copying rows, and the Timecode In / Out, Tape Name and Snap Offset columns (Milan Pro keeps no tape names or snap offsets; media timecode is kept since R17a, so Timecode In / Out can be added).
- Tests: `editing_start_moves_the_event_in_one_undo_step`, `sort_by_length`, `notes_and_summary_survive_save_and_load`, `locks_groups_tracks_and_lengths_follow_the_timeline`, `markers_and_regions_edit`, `lengths_round_trip_in_every_ruler_format`, `undo_keeps_track_heights_and_the_name`, `old_projects_load_without_fades_or_loop`.

**Scope**
- Edit Details window: sortable, editable tables of Events, Markers, Regions, Media and Commands, with VEGAS columns (Track, Start, Length, Take, Active, ...).
- Project Notes (VP23): a free text field stored in the project.

**Files:** `ui/edit_details.rs` (new), `project.rs` (`Project.notes`), `ui/dock.rs`, `ui/menus.rs`, `app/commands.rs`.

**Acceptance tests**
- Editing Start in the table moves the event (one undo step).
- Sort by length.
- Notes survive save and load.

### R16b: Video Scopes (M)

**Status: DONE.** Built as VEGAS 15 has it (its resource strings were read from `vegas150.exe`), which goes beyond the design below in places:
- Video Scopes (`ui/scopes.rs`, View › Window, Ctrl+Alt+2) lists VEGAS's views: Vectorscope, Waveform, Histogram, RGB Parade, Vectorscope/Waveform, Vectorscope/Waveform/Histogram, Vectorscope/RGB Parade and All (two by two). Each scope has VEGAS's own setting above it: Vectorscope Normal / Magnified (2×), Waveform Luminance / Composite (luma ± chroma), Histogram Luminance, Red, Green, Blue, Alpha, Luminance/R/G/B and Lightness/a/b with "Mean" and "Std Dev" under each graph. The toolbar has Settings... (Video Scopes Settings: 7.5 IRE Setup and Studio RGB (16 to 235), both off, so a 100 % white frame is 100 IRE) and Update Scopes while Playing (off, as in VEGAS: the scopes keep the last frame while playing). Hovering reads IRE, hue angle and saturation, the parade value or a histogram bin. The choices are kept in the preferences (`Prefs.scopes`; an old file gets the defaults).
- `engine/scopes.rs` measures in one pass over the Video Preview's composited frame (the engine's `Preview.image`, never decoded again): Rec. 601 luma, NTSC U/V for the vectorscope and its 75 % targets (R, Mg, B, Cy, G, Yl), code values for the histogram and parade (the settings change only the waveform and vectorscope, as in VEGAS). Frames over 0.25 MP are read every 2nd or 3rd pixel. It runs on a "milan-scopes" thread started when the window first draws, at most 10 times a second (`Throttle`); a closed or background-tab window costs nothing. With the Video Preview closed, the open scopes keep the engine making the frame at the cursor.
- R0 gave this batch the Video Preview's Split-Screen View and the Isolate channel overlays: Split-Screen View (button and ▾: FX Bypassed / Clipboard, Select Left Half / Right Half / All; a drag in the picture draws the region, a double-click takes the whole frame; not kept between sessions, as in VEGAS). FX Bypassed composes the frame a second time without event, track and Video Output FX on a second engine that runs only while it is on (`VideoEngine::without_fx`); Clipboard shows the last Copy Snapshot to Clipboard frame, and the scopes measure that split as VEGAS's do. Overlays › Red, Green, Blue, Red / Green / Blue / Alpha as Grayscale (VEGAS 15's rows) show one channel.
- Not built: VEGAS 2026's HDR (ST.2084) scope, skin tone line and parade Combined mode (later VEGAS versions); the composite waveform marks the two peaks of each pixel's signal, not the swing between (shortcut); overlays and split are not applied to the Trimmer.
- Benchmarks (release, this PC): `bench_scopes` all four scopes 5.1 ms at 480x270, 5.3 ms at 960x540, 10.1 ms at 1920x1080 (Lightness/a/b alone 6.6-11.6 ms); `bench_playback` 30.09/30.09/30.08/30.08 fps for 1-4 layers, 0 skipped (before: 30.09 each, 0 skipped); `bench_playback_with_scopes` (all four scopes of a 960x540 frame 10 times a second alongside) 30.08-30.09 fps, 0 skipped.
- Tests: `white_peaks_at_100_ire_and_255`, `colour_bars_land_on_the_vectorscope_targets`, `histogram_counts`, `composite_waveform_shows_chroma`, `splice_takes_the_region_from_the_other_frame`, `throttle_allows_ten_a_second`, `views_want_their_scopes_and_old_prefs_load`, `a_frame_is_measured_once`, `isolate_shows_one_channel`, `split_regions_follow_the_rotation`, `fx_bypassed_plan_has_no_video_fx`, `builtin_layouts_open_the_vegas_panels`, `a_bad_field_falls_back_alone`.

**Scope:** waveform (luma/RGB parade), vectorscope with 75% targets, histogram and RGB parade. They are computed from the displayed composite on a worker at most 10 times a second, and work on the paused frame. Add the Video Scopes column (539 px, between the group and Video Preview) to the Color Correction layout (`Dock::builtin`, R13).

**Files:** `engine/scopes.rs` (new, pure), `ui/scopes.rs` (new), `ui/dock.rs`, `ui/menus.rs`.

**Acceptance tests**
- 75% colour bars land on the vectorscope targets within ±2%.
- A black frame histogram is all at 0.
- A test of the 10/s throttle.

### R16c: Loudness meters and loudness log (M)

**Status: DONE.** Built as VEGAS has it (its help pages on loudness metering and logging), which goes beyond the design below in places:
- Measuring (`engine/loudness.rs`, pure) follows ITU-R BS.1770-4: the K-weighting shelf and RLB high-pass (from their analogue prototypes; at 48 kHz they are the tabled coefficients to 1e-8), 100 ms sub-blocks, momentary (400 ms) and short-term (3 s) loudness every 100 ms, integrated loudness over 400 ms gating blocks with 75 % overlap and the -70 LUFS absolute and -10 LU relative gates, loudness range after EBU Tech 3342 (3 s windows, -70 LUFS and -20 LU gates, 10th to 95th percentile), and true peak with the Annex 2 48-tap 4-phase interpolating filter. Stereo, both channels weighted 1; R10a: 5.1 / 7.1 with BS.1770-4's channel weights.
- Loudness Meters window (`ui/loudness.rs`, View › Window › Loudness Meters, Ctrl+Alt+8 in VEGAS 16 and Ctrl+Alt+7 in VEGAS 23; not in a default layout): the statistics on the left (Momentary, Short, Integrated with its over-target lamp, LRA, True Peak with its over-maximum lamp), the M, S and I meters with the target line and, with Show True Peak Meter, L and R true-peak meters. Its right-click menu is Options › Loudness Meters, now live: Reset Clip (the true peaks), Reset Metering Engine, EBU R 128 Mode (-23 LUFS, -1 dBTP) / ATSC A 85 Mode (-24 LUFS, -2 dBTP), True Peak Blocking Filter (the larger of the peaks with and without a 10 Hz DC blocker, VEGAS's wording), True Peak Range (-12 ... -138 to 0 dB), Loudness Scale EBU +9 (-18 to +9 LU) / EBU +18 (-36 to +18 LU) and Absolute (-23 LUFS) (a check: LUFS instead of LU from the target, as VEGAS's help reads it), Show True Peak Meter, Show Labels, Hold Peaks and Hold Valleys. The choices are kept in the preferences (`Prefs.loudness`; an old file gets VEGAS's defaults).
- Real time: the output callback copies the master (after the master fader, before Dim and Downmix, which only monitor) into a lock-free ring (`audio::Ring`, `Player::loud`) while a session plays, with a NaN frame at each session's start; a "milan-loudness" thread, started when the window first draws, drains it every 20 ms and measures. Neither the mix thread nor the callback takes a lock or does more than a copy. As in VEGAS the statistics start again when playback starts (or seeks) and keep their values when it stops. Shortcut: a device not at 48 kHz is metered after the playback resampler.
- Tools › Generate Loudness Log: the time selection (else the whole timeline) mixed as a render mixes it, in the background, into `<project>_loud.txt` beside the project (an unsaved project asks where); Preferences › Audio › "Open editor when new loudness log is generated" (on) opens it. Render Options › "Save loudness log next to media file" writes `<media>_loud.txt` from the mix the encoder gets (after the master FX, before the codec, as VEGAS). The log has the file, format, mode and range, the integrated loudness, loudness range, maximum momentary, short-term and true peak, and a row per second (momentary, short-term, true peak). VEGAS's own log layout is not documented, so this one is Milan Pro's.
- Not built: the loudness of media on import (R45b). (5.1 channel weighting: R10a.)
- Tests: `k_weighting_matches_bs1770_at_48k`, `tech3341_sines_read_their_level` (cases 1 and 2), `tech3341_gating_cases` (cases 3-5), `tech3342_loudness_range` (cases 1-3), `true_peak_finds_inter_sample_peaks`, `blocking_filter_never_lowers_the_peak`, `other_rates_and_the_log`, `loudness_tap_is_the_master_before_dim_and_downmix`, `options_follow_the_menu_and_old_prefs_load`, `loudness_meter_commands_work`, `a_bad_field_falls_back_alone`; `render_e2e` checks the render's log against the rendered file and Generate Loudness Log against it.

**Scope**
- EBU R128 / ATSC A/85: momentary, short-term and integrated LUFS, LRA, and true peak (4× oversampling). Scale options from the existing Options > Loudness Meters menu.
- Generate Loudness Log (a text report for a render or the loop region).

**Files:** `engine/loudness.rs` (new, pure: K-weighting biquads, gating), `ui/loudness.rs` (new), `engine/render.rs` (optional log), `ui/menus.rs`.

**Acceptance tests**
- EBU Tech 3341 test signals give the expected values: a 1 kHz sine at -23 dBFS reads -23.0 ± 0.1 LUFS; gating case 3/4 ± 0.1.
- A true-peak test signal is within ±0.2 dB.

### R17a: Synchronize events by audio, timecode or date (M)

**Status: DONE.** Built as VEGAS has it, which differs from the design below in places:
- Tools › Multicamera › Synchronize Audio to Align Events, Synchronize Media Timecode to Align Events and Synchronize Date/Time Stamp to Align Events (`app/sync.rs`; `edit::sync_units`, `plan_sync`, `apply_sync`; live with two or more events selected): as in VEGAS, the selected events on the topmost track that has one stay, and the other selected events move to match them. An event moves with its group mates (with Ignore Event Grouping, only the selected events of its group); a selected picture is matched by its grouped sound. The moves are one undo step and follow a Move drag: locked events and events on locked tracks stay (and are named), Auto Ripple (or Post-Edit Ripple later), Lock Envelopes to Events and automatic crossfades apply, and a ripple never moves a synchronized event. If a match would start an event before 0, everything moves later instead, the references too (refused when a reference is locked). The status bar says how many events were aligned and names each one not moved, with the reason. VEGAS reports in a message box.
- Audio (`engine/sync.rs`): each event is read as the mixer plays it (in point, rate, stream), mixed to mono at 48 kHz. Its 1 ms envelope (coarser when the events together run past about 35 minutes) is cross-correlated with the reference's by FFT (a small radix-2 FFT, no new crate). Each lag is scored by the correlation over the overlap, which must be at least 5 s or a tenth of the shorter event. The best lag is then refined sample by sample (±10 ms) on up to 2 s where both are loudest; the absolute correlation is used, so a reversed-polarity microphone still matches. A match scoring under 0.5, or with another peak above 75 % of it, is refused ("no confident audio match" / "matches equally well at another place"), and that event stays where it is. The analysis runs on a "milan-sync" thread with its progress and Cancel in the status bar; an event edited meanwhile is left alone. With a time selection, only that stretch of the reference is read (VEGAS's own advice for quicker, surer syncs).
- Timecode is read at import (`MediaItem.timecode`, seconds): the container's or a stream's `timecode` tag (QuickTime `tmcd`, MXF) at the picture's rate, drop-frame when the tag says so (`;` or `.`) at 29.97 and 59.94, else a BWF `time_reference`. The date is `MediaItem.created` (`creation_time`), else the file's modified time less its length, which also works for media imported before R17a. Older media gets its timecode when it is imported again. Old projects load (both fields are optional and not written when empty).
- Project Media › Lay Out Tracks Using Media Timecode / Date/Time Stamp: the media acted on are added on new tracks, lined up by their stamps with the earliest at the cursor, in one undo step. Media without a stamp, and stills, are skipped, and the status bar counts them. The Timecode item is greyed when none of the media has timecode. VEGAS puts each Tape Name on one track; Milan Pro keeps no tape names, so each file gets its own tracks.
- For R17b: `edit::sync_units` and `engine::sync` are what Create Multicamera Track will sync with. The other Multicamera items stay greyed for R17b.
- Not built: reversed events and nested timelines are not read (they are reported as having no audio), and Velocity envelopes are ignored when matching. A date mixes tag and file times as they are, so a camera that writes local time as UTC is hours off.
- Tests: `a_copy_offset_by_1234_samples_syncs_to_exactly_1234` (also 1.234 s under noise with reversed polarity), `a_weak_or_ambiguous_match_is_refused`, `coarse_scores_every_lag_like_a_direct_sum`, `timecode_tags_drop_and_non_drop`, `timecode_and_creation_time_are_read`, `synchronize_respects_groups_locks_and_ripple_in_one_undo_step`, `lay_out_tracks_by_timecode` (three clips with known start timecodes), `synchronize_commands_work`, `old_projects_load_without_fades_or_loop`, `every_new_field_round_trips`. ffmpeg-backed (ignored): `ffmpeg_syncs_two_recordings` (two WAV files 1.234 s apart sync to within half a sample, also inside a time selection; Cancel stops the job) and `ffmpeg_reads_media_timecode` (a drop-frame QuickTime, an MXF and a BWF WAV).

**Scope:** Tools > Multicamera > Synchronize Audio to Align Events, Synchronize Media Timecode, Synchronize Date/Time Stamp; Lay Out Tracks Using Media Timecode or Date/Time.

**Design**
- Audio: cross-correlate 1 kHz envelope-decimated audio (peaks from `Waveforms`), then refine at 48 kHz in a ±50 ms window with a direct correlation over 2 s (no FFT crate needed).
- Timecode: ffprobe `tags.timecode` or the `tmcd` track (add to probe).
- Date: `creation_time` tag or the file mtime.

**Files:** `engine/sync.rs` (new), `media.rs`, `project.rs` (`MediaItem.timecode`, `created`), `edit.rs`, `ui/menus.rs`.

**Acceptance tests:** a synthetic pair offset by 1.234 s aligns within 1 sample at 48 kHz; the timecode parser handles DF/NDF.

### R17b: Multicamera editing (L)

**Status: DONE.** Built as VEGAS has it (its help page on editing multicamera video), which differs from the design below in places:
- Takes keep a fixed order now (`Clip.take_no`, the active take's number; old projects load with 0 and it is not written then): Take 1, 2, ... are the cameras, and Next / Previous Take, the Take Chooser and removing media keep it (`edit::take_list`, `set_take`).
- Tools › Multicamera › Create Multicamera Track (`edit::create_multicam`; live with events selected on two or more tracks of one kind): of each kind, the selected events become one track of multicamera events whose takes are their tracks in track order, split wherever one of them starts or ends; a track with nothing there gives an empty take, "(no camera)", that cannot be chosen. The topmost track keeps them, camera tracks left empty go, and the active take is the topmost camera there. As in VEGAS, the events are used as they are placed: line them up first with Synchronize (R17a). One undo step; a locked event or track refuses it.
- Tools › Multicamera › Enable Multicamera Editing (Ctrl+Shift+D, this session only): the Video Preview shows every take of the topmost multicamera event at the cursor in a grid, each with its number and name and the active one in a coloured border; video FX, envelopes and motion are left out (VEGAS). The grid is a project of its own (`engine::multicam::grid`: a track per take moved into its tile by Track Motion, over black) that the video engine plays like any other, so it reuses its decoders and caches. Clicking a take, or 1-9 (instead of going to markers), chooses it at the cursor, also while playing: the event is cut there and the take plays from there on, and neighbours left playing one take across a cut join again (`edit::switch_take`). Ctrl+click and Ctrl+1-9 crossfade into it over Preferences › Editing › Cut-to-overlap conversion. Stopped with a time selection, the selection gets the take where it is available. Each choice is one undo step.
- Edit Multicamera Audio with Video (off, as in VEGAS): a take choice also switches the topmost multicamera audio track's take. While it is on in multicamera editing, playback decodes the other audio takes along (`AEvent::standby`, `mix::Mixer::standby`), so a switch goes on from a running decoder, and a switch heard after its cut (the mixer runs about 130 ms ahead) crossfades over the quickfade instead of clicking.
- Show Full Frame in External Monitor (off): with Preview on External Monitor, the external display shows the timeline's own picture (a second engine) while the Video Preview shows the grid.
- Tools › Multicamera › Expand to Multiple Tracks... (live with a multicamera track selected; `edit::expand_multicam`): asks, as VEGAS does, whether to keep the unused takes as muted events (Yes, so the track can be made again), delete them (No) or Cancel. Take 1 stays on the track, the others go on new tracks below it named after their takes.
- Benchmark (`bench_playback_multicam_grid`, four 1080p H.264 cameras in the grid at Preview, 960x540): 30.09 fps against the 29.97 fps clock, 0 frames skipped, 0 late layer frames; `bench_playback` with 4 blended layers on the same machine: 30.08 fps.
- Not built: Preferences › Video › Display take names / numbers and the Active take indicator color (the grid always shows both, in Milan Pro's blue), per-take event settings (a take plays at its event's rate, Pan/Crop and FX, the topmost camera's when the track is made), a crossfade in the audio for Ctrl (the sound cuts), and the overlay drawn turned for Project Properties › Output rotation.
- Tests: `create_multicamera_track_makes_takes_of_the_tracks`, `choosing_take_2_at_3_s_cuts_there` (the acceptance test), `a_take_over_a_time_selection`, `take_numbers_stay_with_their_takes`, `expand_without_the_unused_takes`, `grid_layout`, `grid_shows_every_take_in_its_tile`, `expand_restores_the_original_tracks` (the acceptance test: frame by frame, each expanded track shows what its camera track did), `a_take_switch_while_playing_goes_on_from_the_standby_decoder`, `multicamera_commands_work`, `old_projects_load_without_fades_or_loop`, `every_new_field_round_trips`, `prefs_roundtrip_and_tolerate_missing_fields`. ffmpeg-backed (ignored): `multicam_render_cuts_between_cameras` (two cameras cut at 1 s render red then blue, 440 Hz then 880 Hz) and `bench_playback_multicam_grid`.

**Scope**
- Create Multicamera Track: tracks become one track whose events carry takes.
- Enable Multicamera Editing (Ctrl+Shift+D): the preview shows a take grid; clicking a take or pressing 1-9 during playback switches the take at the cursor (split plus take change).
- Expand to Multiple Tracks; Edit Multicamera Audio with Video; Show Full Frame in External Monitor.

**Files:** `edit.rs`, `ui/preview.rs`, `engine/worker.rs` (grid compose), `app/commands.rs`, `ui/menus.rs`.

**Acceptance tests**
- Pressing 2 at 3.0 s splits the event and sets take 2 from 3.0 s.
- Expand restores the original tracks (frame hash).

### R18a: Remaining VEGAS commands, part 1 (M)

**Status: DONE.** Built as VEGAS has it (its help pages on Smart Split and command markers), which differs from the design below in places:
- Edit › Smart Split (VEGAS 23, Alt+S, the toolbar button; `edit::smart_split`): with events selected (and their group, unless Ignore Event Grouping) and a time selection inside them, each loses the selected stretch: the part after moves back to overlap the part before by five frames with a crossfade, and the two are grouped. Locked events and tracks stay; what follows ripples as a delete does (Auto Ripple, or Post-Edit Ripple later); one undo step.
- Insert › Command... (C): VEGAS's Command Properties (`ui/dialogs/command_props.rs`: Command, from VEGAS's list (URL, Text, WMClosedCaption, ...) or typed, Parameter, Comment, Position). Insert › Audio CD Track Region (N, over the time selection) and Audio CD Track Index (Shift+N, at the cursor). They are kept in `Timeline.commands`, `cd_tracks` (Name, ISRC, copy protection, emphasis) and `cd_indexes`, and shown, as in VEGAS only while there are some, on a CD layout bar and a command bar under the marker bar (VEGAS's CD Track, CD Index and Command Marker colours; track numbers, and index numbers counted from 2 within their track). A click on a tag moves the cursor there, a double-click edits a command marker or selects a track region's time, command markers and indexes drag (snapping) like markers, and the right-click menu has Go To, Edit... / Select Track and Delete. They ripple with the markers and regions, are snap targets, and Edit Details' Audio CD Track List and Commands views list and edit them (one undo step each).
- Tools › Open in Audio Editor (Ctrl+E), Open Copy in Audio Editor (also in Tools › Audio and the event menu; `app/audio_editor.rs`): the focus audio event's file opens in Preferences › Audio › "Preferred audio editor" (asked for when not set). Open Copy copies the file beside it ("Take Copy.wav") and makes the copy the event's active take (one undo step). Each file sent is watched: when its modified time changes, it is probed again (on a thread), its media takes the new length, rate and markers (a subclip keeps its span), Normalize measures again and the peaks are rebuilt. The editor is started directly, not in Milan Pro's kill-on-exit job, so closing Milan Pro never closes it with unsaved work.
- View › Rebuild Audio Peaks (Shift+F5): every waveform is read again (`Waveforms::rebuild`; peaks still being read for the old generation are dropped).
- View › Event Media Markers › Show Markers (Ctrl+Shift+K) and Show Marker Labels: media markers are the file's chapters, read at import (`ffprobe -show_chapters`, `MediaItem.markers`; media imported before R18a has none until imported again), drawn as lines in the events where they play (in point, rate, reverse and loop passes), with their names when labels are on. View › Event Length (VEGAS 23): the event's length in the ruler's format at its top right. All off, as in VEGAS (`Prefs`).
- Options › Metronome: a click on every beat at the project tempo (Project Properties › Ruler), 1.5 kHz on each measure's first beat, 1 kHz on the others, added by the playback mixer (`mix::Metronome`, `AudioPlan.metronome`), so it is heard while playing and recording and never rendered (VEGAS).
- Insert › Generated Media: Credit Roll (`milan.gen.credits`: VEGAS's paste rule, a line starting with a space is a header and a line with a tab two columns; scrolls up or down over the generated media's length, which is the first event's) and Test Pattern (SMPTE Bars (EG 1-1990 in computer RGB with 7.5 % setup), Color Bars, Grayscale Ramp, Grid).
- Shift+F10 (Context menu) opens the focus event's right-click menu, else the one under the pointer.
- Not built: VEGAS's Warp Flow morph across Smart Split's overlap (a crossfade, i.e. a dissolve, instead; shortcut in `edit::smart_split`), command templates (`cmdtemp.xml`), Project Properties › Audio CD (first track number, the 2 s offset VEGAS adds before the first track) and burning (R59), media regions and WAV cue points (ffmpeg reads no cue chunks), the metronome's sound files and level, the Credit Roll's Timed Sequence effect and per-line styles, the audio editor's name in the menu label.
- Tests: `smart_split_cuts_the_selection_out_and_overlaps_five_frames`, `cd_and_command_markers_ripple_with_markers`, `command_and_cd_markers_insert_in_order`, `cd_and_command_bars_follow_their_markers`, `audio_cd_tracks_and_commands_show_and_edit`, `media_markers_follow_the_event`, `chapters_are_media_markers`, `rebuild_forgets_peaks`, `copies_and_reloads`, `r18a_commands_work`, `every_greyed_command_says_why`, `old_projects_load_without_fades_or_loop`, `every_new_field_round_trips`, `prefs_roundtrip_and_tolerate_missing_fields`; the acceptance tests `test_pattern_smpte_bars`, `credit_roll_scroll_position_at_t`, `metronome_clicks_land_on_beats_at_120_bpm` and (ffmpeg-backed, ignored) `ffmpeg_reads_media_markers` (chapters from an ffmetadata file).

**Scope**
- Smart Split (VP23).
- Insert Command markers (metadata: URL/Text/Caption) and Audio CD Track Region and Index (markers only; burning is R59).
- Open in Audio Editor (preference path to an external editor; on its exit, or on an mtime change, reload the media and rebuild peaks).
- Rebuild Audio Peaks.
- Event Media Markers (read chapters and cue points with ffprobe `-show_chapters`) and Show Marker Labels.
- Metronome (a click in the mixer at the timeline tempo, from R1).
- Credit Roll and Test Pattern generators.

**Files:** `edit.rs`, `project.rs`, `engine/generate.rs`, `engine/plugins.rs`, `engine/mix.rs`, `media.rs`, `ui/menus.rs`, `app/commands.rs`, `settings.rs`.

**Acceptance tests**
- Test Pattern SMPTE bars pixel values.
- A credit roll scroll position at t.
- The metronome click lands on beats at 120 BPM (sample index).
- Media markers parsed from an ffmetadata test file.

### R18b: Remaining VEGAS commands, part 2 (M)

**Status: DONE.** Built as VEGAS has it (its help pages on adjustment tracks and events, the Slideshow Creator, cutting, copying and pasting events, setting default track properties, and its Resize Generated Media script), which differs from the design below in places:
- Insert › Adjustment Track (VEGAS 23, Ctrl+Shift+Alt+Q; `Track.adjustment`, `edit::insert_adjustment_track`): a video track, above the focus video track, whose Track FX run on the composite of everything below it (`plan::TrackPlan::adjust`, `composite::adjust`), mixed in at its Level and Composite Level envelope (VEGAS's strength), switched by its Mute envelope. Made a compositing parent, it adjusts only its children. Its Track Motion, compositing mode and Fade to Color do not apply to the adjustment.
- Insert › Adjustment Event (VEGAS 23; `Clip.adjustment`, no media, drawn with a light shading): on the focus video track at the cursor, as long as Preferences › Editing › New still image length; its Event FX adjust the tracks below while it plays, through its fades and Opacity envelope (overlapping ones crossfade, a transition between them acting as a dissolve). Trim, split, cut, copy, paste, Mute and Lock work as for any event.
- Insert › Slideshow... (VEGAS 23, also a video lane's Insert Slideshow; `ui/dialogs/slideshow.rs`, `edit::insert_slideshow`): the Slideshow Creator's Pictures tab (Add Pictures, Delete, Move Selected -1 / +1, Sort A to Z / Z to A / New to Old / Old to New, Shuffle) and Settings tab (start at the cursor or a typed time; each picture's length or the whole slideshow's; Effects: Crossfade, one of the transitions, or Random; Overlap). Create imports the pictures (probed in the background, as a drop) and lays them out on the focus video track as one undo step; a locked track refuses it.
- Insert › Time... (`edit::insert_time`): a length (in the ruler's format; the time selection's, else 1 s) of empty time at the cursor: what starts after it moves on the selected tracks, or every track when none is selected (VEGAS), with the markers, regions, command and CD markers and the time selection; an event across the cursor and locked events stay. One undo step.
- Insert › Audio Transition... (`ui/dialogs/audio_transition.rs`, `edit::audio_transition`): a crossfade preset (fade-out and fade-in curves) and length (Preferences › Editing › Cut-to-overlap conversion) for the cut, crossfade or lone event edge at the cursor on the tracks of the selected audio events (else the selected audio tracks): a cut becomes an overlap as a transition dropped on a video cut does (`edit::widen_cut`, shared with `set_transition`). VEGAS documents no dialog for this item, so its layout is Milan Pro's.
- Paste Insert variants (VEGAS's help on cutting, copying and pasting): with a time selection, Cut and Copy take the parts of the selected events inside it, or of every track's events when none is selected (`edit::split_range`, `copy_range`); Cut closes the gap with Auto Ripple (or Post-Edit Ripple later). Paste Insert splits an event under the cursor so its second half moves after what is pasted (`edit::split_for_insert`).
- Tools › Scripting › Resize Generated Media (`edit::resize_generated`, as VEGAS's script): generated media now keeps the frame size it was made at (`MediaItem.width/height`; media made before R18b has none and follows the project), and the script sets the selected video events' (else every video event's) generated media to the project's size. One undo step.
- Video track Set Default Track Properties (`VideoDefaults`, `Prefs.default_video_track`): Height, Composite Mode and Track FX of the track become what new video tracks start with (Restore original defaults clears them); audio keeps Track FX.
- Re-assigned (no tier 0-2 batch is left on a control): Project Properties › Audio › Resample and stretch quality (R4b) to R10a, the track header's Edit Visible Button Set (R15) to R49.
- Not built: the Slideshow Creator's Rotate buttons and pan/zoom Effects styles, VEGAS refusing media on an adjustment track (a media event there draws normally over the adjustment), Track Motion on an adjustment track, and the audio defaults' Volume, Pan Type, Input monitor, Height and Automation mode.
- Tests: `adjustment_track_blur_equals_video_output_blur` (the acceptance test, plus a track above, Level 50 %, a compositing child and an Adjustment Event at Opacity 50 %), `insert_time_moves_markers_and_regions_after_the_cursor` (the acceptance test), `time_selection_cut_copy_and_paste_insert_split`, `audio_transition_makes_a_crossfade_preset`, `slideshow_adjustments_and_generated_resize`, `a_total_length_is_shared`, `new_audio_tracks_get_the_default_chain`, `r18b_commands_work`, `no_control_waits_on_tiers_0_to_2`, `old_projects_load_without_fades_or_loop`, `every_new_field_round_trips`, `prefs_roundtrip_and_tolerate_missing_fields`; `render_e2e` and `nested_render_matches_flat` (ffmpeg-backed) pass.

**Scope**
- Adjustment Track and Adjustment Event (VP23: the track's FX apply to the composite of the tracks below).
- Insert Slideshow (VP23: images to events at a fixed length with a transition).
- Insert Time (shift everything after the cursor).
- Paste Insert variants.
- Insert Audio Transition (crossfade preset).
- Generated media Resize (as the VEGAS script).

**Files:** `project.rs` (`Track.adjustment`), `engine/plan.rs`, `engine/composite.rs`, `edit.rs`, `ui/dialogs/slideshow.rs` (new), `ui/menus.rs`, `app/commands.rs`.

**Acceptance tests**
- A blur adjustment track over two tracks equals Video Output blur applied to the same frame.
- Insert Time of 2 s moves markers and regions after the cursor.

---

### R19: Performance harness, stats and adaptive quality (M)

**Status: DONE.** Built as VEGAS has it where VEGAS has the feature, which differs from the design below in places:
- Adjust Size and Quality for Optimal Playback is the Video Preview's right-click toggle, on by default as in VEGAS (`Preview.adjust`, not saved, like the preview quality). The ladder is the design's (`decode::ladder`): the size halves to Quarter, then Draft (Quarter); Auto starts from the size it picks. The controller (`decode::Adapt`) steps down on more than 2 % dropped over 2 s and up after 10 s without a drop; for hysteresis a step up that drops again within 10 s doubles the next wait (up to 160 s), and after each change or seek counting waits until the new session has spun up (1 s and 15 frames, or 4 s), because a size change reopens the decoders and on GOP-250 media that stalls 1-3 s. The level resets when playback stops or the chosen setting changes; paused frames and the render always use the chosen setting.
- The frame rate readout is VEGAS's: the Display value of the status block ends with the rate shown while playing (`warning_text` when frames drop; VP23 appends it while playing), measured as the project rate times the share of frames shown (`engine::Shows`). The design's status line (VEGAS's "Frame rate: x (target y)" plus dropped frames, live decoders, cache hits and the adjusted setting, which is the "badge") is an extra, so it is the right-click option Show Playback Statistics, off by default. The Preview value turns `warning_text` while the setting is adjusted.
- `tools/bench.ps1` writes `docs/PERFORMANCE.md` with 1-4 layers (not only 1/2/4), the A/V offset of the preview (rows 19.3, 19.4) and an off/on run of the adaptive quality through `decode::Preview` (`bench_adaptive_quality`: 37.7 % dropped off, 19.5 % on over 60 s, 0 % over the last 10 s, on the machine in that file). Benches read `MILAN_BENCH_DIR` for their media.
- Row 17.1: the render thread and its compose stripes run below normal priority (`proc::below_normal_thread`).
- Not built: proxies in the bench (R20 measures scrub on proxies), CI runs of the benches (R51).
- Tests: `adaptive_quality_steps_down_then_up_without_oscillating` and `adaptive_quality_restores_when_playback_keeps_up` (the acceptance tests, against a simulated slow worker, including a 2.5 s spin-up stall per change), `adjust_ladder`, `shows_count_drops_and_rate`, `render_threads_run_below_normal`, `stays_under_budget_least_recent_first` (cache hit counting); ffmpeg-backed `bench_playback`, `bench_scrub`, `bench_playback_with_scopes`, `bench_playback_multicam_grid` and `bench_adaptive_quality` pass through `tools/bench.ps1`.

**Scope**
- A preview status line (VEGAS shows "Frame rate: x.xx (target y)"): fps, dropped frames, live decoders, cache hits. This removes the `engine/mod.rs:54` shortcut.
- `tools/bench.ps1` generates synthetic media with ffmpeg lavfi and runs the ignored benches with `--release`. It writes `docs/PERFORMANCE.md`: machine, preset, 1/2/4 layers, fps, drop percentage, scrub p50/p95, play-start time.
- Adaptive quality ("Adjust Size and Quality for Optimal Playback"): if more than 2% of frames drop over 2 s, step Size down (Full→Half→Quarter, then Draft). After 10 s without drops, step back up. A badge shows the reduction.

**Files:** `engine/mod.rs`, `engine/worker.rs`, `decode.rs`, `ui/preview.rs`, `tools/bench.ps1` (new), `docs/PERFORMANCE.md` (new, generated).

**Acceptance tests**
- A unit test of the adaptive controller with scripted drop counts (down after 2 s of drops, up after 10 s clean, no oscillation).
- The bench script runs locally and fills the table.

### R20: Accurate scrubbing, seeking and reverse playback (L)

**Status: DONE.** Built as below; it differs from the design in places:
- Root cause of the scrub jump (traced first with `scrub_drag_follows_the_cursor`, which reads the burnt-in frame number of the picture on screen while the cursor is dragged through `VideoEngine` at 60 Hz): the Video Preview showed only the answer to the newest Show, and while the cursor moves every UI frame no answer is the newest, so the picture stood still until the drag slowed (dragging at 2x it trailed the cursor by 2.3 s forwards and never moved backwards); on top of that every step back respawned a decoder. Now any answer newer than the picture on screen shows (`VideoEngine::update`). Dragging at 2x, the picture trails the cursor by about 0.05 s forwards and 0.1 s backwards on a proxy (0.4-0.8 s backwards on GOP-250 originals, depending on machine load). Scrub requests were already coalesced by the worker (newest Show wins).
- Reverse: a picture read backwards (reversed events, J shuttle, dragging left, Left arrow; detected when the frame read last is less than 2 s after the one asked for) is read from chunks decoded forward (`-frames:v`) that end at the frame asked for, kept in `DecoderPool` (`engine/vdecode.rs`), not in the source cache: 1 s each (30 frames at 960x540, 10 at 1080p), with the two before it decoding ahead while playing, shuttling or dragging; a single paused jump back reads an 8-frame chunk. Chunks read past close. **Memory bound:** all chunks together reserve at most 256 MB (`BACK_BYTES`; only 8K pictures, at one frame per chunk, pass it) and at most 8 are open, on top of the source (192 MB) and composite (256 MB) caches; idle chunks close after 5 s like streams. `Worker::warm_back` starts a reversed event's chunks before it plays and at play start, so its first frames are not held. Played forward again from frames read backwards, the stream after the chunk starts at once. The per-frame "chase" stream is gone. Renders of reversed events read chunks too.
- Seeks were already frame-exact (ffmpeg's `-ss` before `-i` decodes from the keyframe before the target; `seeks_are_frame_exact_on_long_gop` confirms 200 of 200 on GOP-250 media and on its proxy). A paused seek of source media (a Show) and the chunks now decode with up to 4 threads (`vdecode::seek_threads`, a quarter of the logical cores): a seek 240 frames past a keyframe of 1080p H.264 takes 0.4 s with 4 threads instead of 0.6 s with 2; playback and render streams and proxies keep 2, so forward playback is unchanged (measured A/B against the previous build: play start, fps and unpaced fps equal within noise).
- Not built: the on-disk keyframe index. ffmpeg's seek already starts at the keyframe before the target, chunks need no GOP alignment to be exact, and the index would only refine the 2 s reuse-or-respawn window by tens of milliseconds on long-GOP media (a respawn costs about 100 ms plus decoding from the keyframe). Proxies (a keyframe every 15 frames) stay the fix for long-GOP scrubbing, and meet the target.
- Measured (`docs/PERFORMANCE.md`; this machine was under heavier load than in R19, see the A/B): scrub p95 98.9 ms on proxies (target under 150 ms: pass) and 177.4 ms on GOP-250 originals (R19: 232.6 ms; A/B in the same conditions: 242-255 ms before, 190-197 ms after); 60 steps back p95 8.2 ms (before: a respawn each); random seeks p95 125.9 ms on proxies; reverse playback at 1x 301 of 301 frames exact (before: 1 of 301, the rest held). No menu item waited on R20.
- Tests: `chunks_are_a_second_within_the_budget`, `decoder_argv` (threads, `-frames:v`); ffmpeg-backed `seeks_are_frame_exact_on_long_gop`, `reverse_playback_shows_every_frame` (also renders a reversed event frame-exact), `scrub_drag_follows_the_cursor`, `reverse_playback_reads_every_frame_from_chunks` (replaces `reverse_playback_starts_one_stream_at_a_time`; also bounded chunks over random steps back, playing forward out of a chunk, two layers of one file stepped forward), `bench_scrub` (now also steps back and proxies). `tools/bench.ps1` runs the new ones and reports them.

**Scope**
- Fix the scrub jump of 0.7-1.2 s.
- Frame-exact random seek.
- Smooth reverse playback.

**Design**
- Keyframe index per media: `ffprobe -select_streams v:0 -show_entries packet=pts_time,flags -of csv`, run on the thumbs worker and cached on disk next to proxies, keyed by path, size and mtime.
- A seek spawns at the nearest keyframe at or before the target (exact `-ss` before `-i` with `-noaccurate_seek` off) and reads forward. A scrub request is coalesced: while a seek is pending, only the newest target is kept, and the stream is reused if the target is ahead within the GOP.
- Root-cause check: the jump probably comes from the reuse window `[next_sf, next_sf + 2·fps]` dropping frames and `Held` results being shown. Verify with a trace before changing the code.
- Reverse: `SourceCache` read-back. Decode one GOP-aligned 1 s chunk ending at the target into the cache, then play it backwards; prefetch the previous chunk.

**Files:** `engine/vdecode.rs`, `engine/worker.rs`, `engine/cache.rs`, `proxy.rs` (cache dir), `media.rs`.

**Acceptance tests**
- An ignored e2e on a GOP-250 H.264 synthetic with burnt-in frame numbers (lavfi `drawtext`): 200 random seeks each show the exact frame number (OCR-free: encode the frame number as a colour patch and read the pixel).
- Reverse play at 1× shows at least 95% of frames (counted with the colour patch).
- Scrub p95 under 150 ms on proxies.

### R21a: GPU compositor core on wgpu (L)

**Status: DONE.** Built as designed below, except:
- Textures are `Rgba8Uint`, not `Rgba8Unorm`: the passes replay the CPU compositor's integer maths on the bytes as they are, and the float blend modes replay its f32 operations in order through `engine/gpu/math.wgsl` (every operation pinned against fusing, division and square root corrected to the rounded result), so the parity is bit-exact rather than within 2: 3000 random plans of 1-4 layers in every blend mode (`gpu_matches_cpu_on_random_plans`, `gpu_parity_histogram`), Hard Mix included. The output for the preview is an `Rgba8Unorm` texture with an sRGB view (R21c).
- The UI paints through wgpu (eframe feature `wgpu`, default) on Vulkan, and the compositor shares its device (`gpu::attach`); the `glow` feature keeps the OpenGL painter, and a preference naming another adapter opens it on a device of its own. The screenshot harness runs on both painters; against the glow screenshots the VEGAS 23 look differs by 1-2 in 255 at antialiased edges and through textures (egui-wgpu's sRGB conversion in the shader against egui_glow's; 6.6 % of the pixels) and nowhere else but the Record Time readout, which counts free disk space.
- Found while reviewing the paused WIP: `floor(x + 0.5)` rounded up just under a half, and ClipColor divided 0 by 0 on a uniform out-of-range grey (a NaN the CPU cast to black); both fixed on both sides (`composite::set_lum`).
- Two threads recording on one device at once (a render beside the preview) lost draws on wgpu 24's Vulkan backend (the parity tests run in parallel reproduced it); each recording step takes the compositor's `record` lock, which the parity tests now pass in parallel 10 times over.
- Bench (`bench_compose`, in `tools/bench.ps1` and `docs/PERFORMANCE.md`): four full 1080p layers 4.7 ms on the GPU against 22 ms on the CPU (about three quarters of it uploading the four decoded 8 MB pictures), four picture-in-pictures 1.2 ms against 13 ms. The 4 ms gate holds for the picture-in-pictures and not for four full frames on this integrated Radeon (a Z1 Extreme), where the upload of 32 MB a frame is the cost.

**Scope**
- Switch eframe to the `wgpu` renderer: feature `wgpu`, which uses wgpu 24.0.5 and egui-wgpu 0.31.1 already in `Cargo.lock`. Keep glow as a fallback feature.
- `trait Compositor` with `CpuCompositor` (today's code) and `GpuCompositor`: per-layer textures, affine draw, opacity, the 9 (plus R7) blend modes and track motion in WGSL.
- Automatic fallback to the CPU when no adapter, or the "Use GPU" preference is off.

**Design**
- Frames are uploaded as `Rgba8Unorm` textures, and the output is read back for the existing `ColorImage` path (zero-copy comes in R21c).
- Parity: the GPU output must match the CPU within 2 LSB.

**Files:** `Cargo.toml`, `main.rs` (renderer choice), `engine/composite.rs` (trait), `engine/gpu/mod.rs`, `engine/gpu/*.wgsl` (new), `engine/worker.rs`, `engine/render.rs`, `settings.rs`.

**Risk:** the UI's own painting moves to wgpu too. Rerun the screenshot harness.

**Acceptance tests**
- Parity tests run when an adapter exists, else they skip: random plans with 1-4 layers and each blend mode, max abs diff ≤ 2.
- 4-layer 1080p compose under 4 ms on a mid-range GPU (bench).

### R21b: GPU effects, transitions and generators (L)

**Status: DONE.** Built as designed below, except:
- `PluginInfo::gpu: Option<GpuPort>` names the port; the 15 video FX plus Color Grading, the 8 transitions and the solid, gradient, checkerboard and noise generators have one (`engine/gpu/fx.rs`, `fx.wgsl`). Cookie Cutter, Bezier Masking (rasterised masks), the text generators and OFX plug-ins have none: a picture, track, transition, adjustment or output chain with one is made by the CPU code and uploaded, so mixed plans compose on the GPU with those pieces exact.
- Levels and curves are the CPU's own 256-entry LUTs (`vfx::color_op_for`, `Grade::steps`), uploaded as a storage buffer; a .cube is a storage buffer with the CPU's trilinear maths rather than a sampled 3D texture, so it is bit-exact. The blur is a compute shader running the CPU's sliding window, one invocation per line, exact at any radius. Resampling, Reduce Interlace Flicker, Adjustment mixes and the opaque output step are passes too.
- Parity (`gpu_video_fx_match_cpu`, `gpu_transitions_match_cpu`, `gpu_generators_match_cpu`, three parameter sets each): every plug-in bit-exact, the clock wipe within 1 (the GPU's `atan2`); a clock wipe with no feather can differ on the pixels exactly at its edge.

**Scope:** WGSL ports of the 15 video FX (separable blur, colour ops, levels/curves via 1D LUT texture, .cube via 3D texture, chroma key, sharpen, vignette, mirror, border, crop), the 8 transitions and the noise/gradient/checker generators. Text stays CPU-rendered and uploaded.

**Files:** `engine/gpu/fx/*.wgsl`, `engine/gpu/mod.rs`, `engine/plugins.rs` (a `gpu: Option<…>` per plug-in).

**Acceptance tests:** a parity test per plug-in at three parameter sets (≤ 2 LSB; blur ≤ 3).

### R21c: Zero-copy preview and float pipeline hook (M)

**Status: DONE.** Built as designed below, except:
- `engine::picture::Picture` carries a frame: pixels (the CPU), or the GPU's output texture with the pixels read back once if asked (`image`; a render, the scopes, a snapshot, the Overlays channel view). `decode::PreviewTex::Native` shows it through `egui_wgpu::Renderer::register_native_texture` (an `Rgba8Unorm` texture through its sRGB view, as egui's own textures), freed when the next frame replaces it. `preview_frames_stay_on_the_gpu_and_renders_read_back`: the compositor's readback counter stays at 0 through a preview frame and moves once when its pixels are asked for as a render asks; the harness prints the count with `MILAN_SHOT_TRACE`.
- `GpuCompositor::set_intermediate(Intermediate::Rgba16Float)` keeps the output in linear-light 16-bit float (the preview samples it, a readback converts back within 1 in 255: `float_intermediate_reads_back_within_one`); the passes still compose in 8-bit until R40b.
- 4K Best bench: four full 3840x2160 layers 30 ms on the GPU against 103 ms on the CPU (uploads of 133 MB a frame included); with the readback 49 ms.

**Scope**
- Register the GPU output as an egui native texture (`egui_wgpu::RenderState::renderer.register_native_texture`), so preview playback needs no readback.
- Add an `Rgba16Float` intermediate format switch, used by R40b.

**Files:** `decode.rs` (Preview texture path), `engine/gpu/mod.rs`, `ui/preview.rs`.

**Acceptance tests**
- Preview shows GPU frames with no CPU readback (a counter stays 0).
- Render still reads back.
- A 4K Best 4-layer bench is recorded in PERFORMANCE.md.

### R22: Hardware acceleration detection and preferences (M)

**Status: DONE.** `engine/hw.rs`: at startup a background thread lists `ffmpeg -hwaccels` and the hardware encoders of `-encoders`, then times each decoder path (d3d11va, cuda, qsv) on a 5 s 1080p H.264, HEVC and AV1 clip against the software decoder, and runs each hardware encoder (h264/hevc/av1 × nvenc/qsv/amf/mf) on the same picture; the table is kept in `%LOCALAPPDATA%\MilanPro\hw.json` per FFmpeg version and Vulkan driver string. Preferences › Video: "Video decode acceleration" (Automatic, Off, or a hwaccel that passed), "Encode device" (Automatic, Off, or a device that passed), Test Again, and the measured table. The decoder (`vdecode::wants_hw` → `hw::decode_hwaccel`) uses the chosen hwaccel for every codec it passed, or with Automatic the one that measured faster than software for heavy sources (HEVC, AV1, 2560 px and up); until the table is built, heavy sources get `-hwaccel auto` as before. Render As: three "(hardware)" templates (`h264_hw`, `hevc_hw`, `av1_hw` in `VCODECS`) resolve to the device's encoder (`Template::resolved`, with each device's own rate control in `video_args`) or to the software encoder with a note in the details and the render log; Customize Template lists a device's encoder once it passed. Tests: `parses_hwaccels`, `parses_hardware_encoders` (captured output), `decode_choice_follows_the_table_and_the_preference`, `encoder_choice_follows_the_table_and_the_device`, `hardware_templates_fall_back_to_software_with_a_note` (forced unavailable), ignored `hw_capability_table` (the probe, printed for `tools/bench.ps1`) and `render_with_hardware_encoder` (this machine: h264_amf). Measured here (Ryzen Z1 Extreme iGPU): d3d11va decodes all three codecs but, with the frames downloaded to system memory, only HEVC comes out faster than the 2-thread software decoder (385 vs 412 ms for 150 frames), so Automatic uses it for HEVC alone; AMF encodes H.264, HEVC and AV1, Media Foundation H.264. Not built: a hardware decode that keeps frames on the GPU (`-hwaccel_output_format`), which would need the decoder's output to go to the compositor without the RGBA pipe.

**Scope**
- At startup, on a background thread, build a capability table: hwaccels from `ffmpeg -hwaccels`, and a 1-frame test decode and encode per device and codec (d3d11va, cuda, qsv; h264/hevc/av1 × nvenc/qsv/amf/mf). Cache it per FFmpeg version and GPU driver.
- Preferences > Video: decode acceleration (Auto/Off/specific) and encode device.
- The decoder uses the chosen hwaccel for heavy sources and, when measured faster, H.264.
- R11 templates offer the hardware encoders that passed.

**Files:** `engine/hw.rs` (new), `engine/vdecode.rs`, `render.rs`, `settings.rs`, `ui/dialogs/prefs.rs`.

**Acceptance tests**
- Parser unit tests on captured `-hwaccels` and `-encoders` output.
- With hardware unavailable (forced), templates fall back to software with a note.
- Ignored e2e: an h264_nvenc render when present.

### R23: RAM preview, selective prerender, disk cache (L)

**Status: DONE** (peak files not built, see below). Build Dynamic RAM Preview (Tools, Shift+B) composes the loop region (or from the cursor to the end) at the preview's size and quality into frames the worker pins (`Worker::ram`), up to Preferences › Video › "Dynamic RAM Preview max (MB)" (200, VEGAS's default); the status bar shows the build and Esc stops it; playback and paused frames inside come straight from it (`ram_preview_plays_with_no_decoder`: no decoder opens); an edit drops only the frames whose `plan::frame_key` changed (`ram_preview_keeps_the_frames_an_edit_did_not_touch`); Clear Dynamic RAM Preview (Tools, VEGAS 23). Selectively Prerender Video (Tools and the ruler's menus, Shift+M): a dialog with the template (video templates at the project's size and rate; last used kept) and "Render loop region only"; the sections that are more than a plain event (`plan::needs_work`, `work_sections`) render one by one through `engine::render` to `<prerendered files folder>\<project>-<hash>\<frames hash>.<ext>` and into the folder's `index.json` (`engine/prerender.rs`); the worker plays a section's frames from its file while `plan::range_hash` of the plan without proxies still matches (`Entry::valid`), a render too (`Worker::for_render`); an edit inside a section makes it inactive, outside leaves it (`prerendered_sections_serve_playback_and_render_until_edited`, with the source file taken away); Clean Up Prerendered Video deletes the project's files, all or the inactive ones, never anything the index does not list. The ruler draws the RAM preview (green) and the active prerendered sections (blue) along the top of the time ruler (`engine::preview_bars`). The disk frame cache (`engine/diskcache.rs`, Milan Pro's own, off by default: Preferences › Video "Disk cache for decoded HEVC, AV1 and 4K frames (MB)" with Clear) keeps decoded frames of heavy sources that paused seeks asked for as raw RGBA files under `%LOCALAPPDATA%\MilanPro\framecache` with an index, least recently used first under the cap, written on a thread (`lru_on_disk`); `bench_disk_cache` and `bench_ram_preview` are in `tools/bench.ps1` and `docs/PERFORMANCE.md`. Not built: waveform peak files on disk (`FEATURE_PLAN.md` lists them as performance work with no measured need; the waveform worker is unchanged), and a prerender's folder per project file (VEGAS's Project Properties field; Milan Pro keeps one preference with a subfolder per project, so `project.rs` stays untouched).

**Scope**
- Build Dynamic RAM Preview (Shift+B) over the selection, with progress and a size cap preference; Clear Dynamic RAM Preview.
- Selectively Prerender Video (Shift+M): encode the selection to intermediate files (FFV1 or ProRes) in a prerender folder (preference, SSD). The plan uses a prerendered segment when its `FramePlan` hash range matches. Clean Up Prerendered Video.
- Cache size preferences.
- Waveform peak files on disk (`.mpk` beside proxies).

**Files:** `engine/cache.rs`, `engine/worker.rs`, `engine/plan.rs`, `engine/prerender.rs` (new), `audio.rs`, `settings.rs`, `ui/dialogs/prefs.rs`, `ui/timeline/ruler.rs` (prerendered bar), `ui/menus.rs`.

**Acceptance tests**
- A RAM preview of 5 s plays with 0 decoder spawns.
- Prerender invalidation: changing one FX parameter in the range marks it stale.
- Peak files reload with identical waveforms.

---

### R25: VEGAS import v2 (FX, transitions, generators, Track Motion, settings) (L)

**Status: DONE.** `interchange.rs` is `interchange/{mod,vegas,vegas_map}.rs`. The scripts (version 2) export every OFX parameter with keyframes (typed: Double, 2D/3D, Integer, Boolean, Choice, RGB/RGBA, String, Custom), transitions, generated media, Track Motion and Parent Motion keyframes, parent/child compositing, event envelopes, audio FX by name and preset, media FX per file, video output and master bus FX, field order, PAR, quality, motion blur, deinterlace, pixel format, bit depth and master bus mode; they compile against VEGAS 15's `ScriptPortal.Vegas.dll` (read-only; VEGAS never launched). `vegas_map.rs` holds 32 rows (section 4.2's list plus the CSV's Vignette, Crop and LUT Filter) with parameter names, choice indexes and ranges read from the plug-ins' own preset and resource XML in the VEGAS install; unmapped plug-ins become disabled `ofx:<id>@1` placeholders holding every parameter, listed in the report. Tests: `v2_fx_map_or_become_placeholders` (save/load keeps the placeholder), `v2_transitions_generators_and_envelopes`, `v2_track_motion_compositing_settings_and_audio_fx`, `rtf_becomes_plain_text`, `every_row_lands_on_a_built_in_parameter`, `scales_and_tables_invert`. Left: the manual VEGAS 15 pass (user); the Track Motion Z sign and rotation sense are taken from the API's y-up frame, not measured (`vegas::motion`).

**Scope:** extend `tools/vegas/Export to Milan Pro.cs` and its VEGAS 13 copy, and `interchange.rs`, to carry:
- Event, track, output and media video FX (OFX `PlugIn.UniqueID`, e.g. `com.sonycreativesoftware:gaussianblur`, plus every `OFXEffect.Parameters` value and keyframes).
- Transitions (`Fade.Transition`).
- Generated media (solid colour, gradients, checkerboard, noise, test pattern, Titles & Text text and font).
- Track Motion keyframes; track and event envelopes, including velocity (R4a).
- Parent/child compositing.
- Project settings (field order, PAR, pixel format, bus mode, audio channels).
- Audio FX by name.
- Stream indices (R3b).

**Design**
- New `src/interchange/vegas_map.rs`: a table mapping OFX id → milan plug-in id, with per-parameter name, scale and offset. Seed it from section 4.2: gaussianblur, brightnessandcontrast, colorcorrector, levels, hsladjust, chromakeyer, sharpen, sepia, invert, mirror, border, vignette, blackandwhite, crop, solidcolor, colorgradient, checkerboard, noisetexture, testpattern, titlesandtext, and the transitions dissolve, linearwipe, clockwipe, iris, push, slide, zoom, flash→fade-to-colour.
- Anything unmapped is listed in the import report with its VEGAS name, and its event keeps a disabled placeholder `Fx { plugin: "ofx:<id>", enabled: false }`. That placeholder is reusable once R33 can load the real plug-in.

**Files:** `tools/vegas/*.cs`, `interchange.rs` (becomes `interchange/mod.rs` plus `interchange/vegas.rs` and `vegas_map.rs`).

**Acceptance tests**
- JSON fixtures (hand-written from the script's schema) for each feature import correctly.
- An unmapped plug-in survives a save/load as a disabled `ofx:` FX.
- Manual (user, VEGAS 15): a test project with 10 FX and 3 transitions imports, and the report lists the rest.

### R26: Send a project back to VEGAS (M)

**Status: DONE (compile-checked, not run in VEGAS).** `interchange/vegas_export.rs` writes the version 2 JSON from a Milan project (`vegas_map` in reverse, placeholders back to their plug-in, colours completed from the built-in's defaults, Titles & Text back to RTF, Pan/Crop back to corners, Track Motion back to pixels); File › Export › VEGAS Project (via script)... writes it and reports what has no VEGAS counterpart. `tools/vegas/Import from Milan Pro.cs` (+ VEGAS 13 copy, with its own JSON reader) rebuilds the project through the API. Both scripts were compiled with the .NET Framework `csc` against the installed `ScriptPortal.Vegas.dll` (clean) and the reader was run on a Milan export; running the script inside VEGAS is the user's manual test. Tests `round_trip_through_the_vegas_json`, `the_file_names_vegas_plugins`.

**Scope:** `tools/vegas/Import from Milan Pro.cs` (VEGAS 14+ and 13 copies) reads a `.milan.json` written by File > Export > "VEGAS Project (via script)...". It rebuilds tracks, events, takes, fades, Pan/Crop, envelopes, markers, regions, and mapped FX and transitions through the scripting API. Milan never writes `.veg`.

**Files:** `tools/vegas/Import from Milan Pro*.cs` (new), `interchange/vegas_export.rs` (new; reuses `vegas_map` in reverse), `ui/menus.rs`, `app/commands.rs`.

**Acceptance tests**
- A serialisation round-trip: Milan → JSON → the import parser of R25 gives the same timeline.
- Manual (user): open in VEGAS 15 and compare.

### R27: `.veg` format research (M, RESEARCH-ONLY)

**Status: DONE.** `docs/research/VEG_FORMAT.md`: no `.veg` exists on this machine (every fixed drive searched read-only), so the container could not be observed and nothing is guessed; the note records what the media scan reads, what the scripting assembly reveals about saving (`SaveProject`, `BeginWrite(ProjectSaveType)`, `ArchiveProject`), the estimate for a reverse-engineered reader and the recommendation (the script route, now both ways). Linked from `docs/INTERCHANGE.md`; the `.veg`-picked explanation in Import › VEGAS Project is verified (`poll_vegas`).

**Deliverable:** `docs/research/VEG_FORMAT.md`. It documents:
- the chunk/GUID container structure observed in sample `.veg` files the user provides (VEGAS 15);
- what the existing media scan reads;
- an estimate for a read-only parser;
- the recommendation (keep the script route).

No product code unless the research finds a stable, documented structure. If it does, open a new batch.

**Acceptance:** the document exists, is linked from INTERCHANGE.md, and Import > VEGAS Project explains the script route when a `.veg` is picked (already done; verify).

### R28: OpenTimelineIO and EDL import/export (L)

**Scope**
- OTIO: hand-written serde structs for `Timeline.1`, `Stack.1`, `Track.1`, `Clip.2`, `Gap.1`, `Transition.1`, `Marker.2`, `ExternalReference.1`, `MissingReference.1`, `RationalTime.1`, `TimeRange.1`, `LinearTimeWarp.1`, `FreezeFrame.1`. Milan-specific data goes under `metadata["milan"]`, so a Milan→OTIO→Milan round-trip is lossless.
- EDL: CMX3600 import/export (V and A1/A2 tracks, cuts, dissolves `D`, M2 speed lines, reel names from file names with a `* FROM CLIP NAME:` comment, drop/non-drop).
- Wire the File > Import/Export EDL rows.

**Files:** `interchange/otio.rs` (new), `interchange/edl.rs` (new), `ui/menus.rs`, `app/commands.rs`.

**Acceptance tests**
- Round-trip tests for both.
- Import of the OTIO sample files (`simple_cut.otio`, `nested_example.otio` from the OTIO repo, Apache-2.0, stored in `tests/fixtures/`).
- EDL golden files at 25 and 29.97 DF.

### R29: FCP7 XML, FCPXML, AAF route, interchange report (L)

**Status: DONE.** `interchange/xml.rs` (quick-xml 0.41 element tree and an indenting writer), `interchange/xmeml.rs` (xmeml 5 both ways: rate with the NTSC flag, every video track, two linked audio tracks per Milan audio track as Premiere and Resolve write stereo and merged back on import, `<file>` with path URL / duration / timecode, Cross Dissolve for overlaps, fades as dissolves from and to black, Time Remap, Opacity, Audio Levels, `<link>` groups, sequence markers; other filters listed and dropped), `interchange/fcpxml.rs` (1.10 both ways: resources, the bottom video track as the spine, other tracks as connected storylines on lanes, `asset-clip` with `srcEnable`, transitions straddling a frame-aligned cut, linear `timeMap`, `adjust-blend` with fade keyframes, `adjust-volume` with fadeIn/fadeOut, markers on the covering element; `clip`, `sync-clip`, `ref-clip`, auditions, titles read or listed). Menu rows wired; the Premiere/After Effects rows read "via Final Cut Pro 7 XML" and run the xmeml route; the AAF row's hover text names the `otioconvert` route. `docs/INTERCHANGE.md` has the summary table (accurate / converted / not representable) and a table per format plus the AAF and VEGAS routes; `docs/COMPATIBILITY.md` says what each was tested against. Fixtures are hand-written to the DTDs (`tests/fixtures/resolve_sample.xml`, `fcpx_sample.fcpxml`); nothing was opened in Premiere, Resolve or Final Cut Pro here.

**Scope**
- FCP7 `xmeml` v5 import/export (also what Premiere and Resolve read). FCPXML 1.10 import/export.
- quick-xml: `quick-xml = "0.41"` reuses the version already in `Cargo.lock`; MIT.
- The Premiere `.prproj` menu rows are relabelled to point at FCP7 XML.
- Write `docs/INTERCHANGE.md`: for each format, what translates exactly, what is converted (fades → transitions, FX dropped with a list), and what is lost. Include the AAF route (`otioconvert` with the `otio-aaf-adapter` Python package, external) and the VEGAS script route.

**Files:** `interchange/xmeml.rs`, `interchange/fcpxml.rs` (new), `Cargo.toml`, `ui/menus.rs`, `docs/INTERCHANGE.md` (new).

**Acceptance tests:** round-trip tests; Resolve-exported sample XMLs import (fixtures made by the user, or hand-written to the DTD).

### R30: Project archive and import media from project (M)

**Status: DONE.** `interchange/archive.rs` (`plan`, `trim_range`, `estimate`, `run`, `media_of`), `ui/dialogs/archive.rs` (Archive Project: folder, trim with handles, the media list with sizes and used ranges, progress on a worker thread with Cancel, the result with Open Folder; also Tools › Advanced Save in VP23), `ui/dialogs/media_from_project.rs` (check boxes, items already in the project greyed, one undo step). Trimming: PCM re-written sample-accurately, compressed audio stream-copied, video re-encoded (H.264 CRF 16 + AAC) so the cut is frame-accurate; in points move with the cut; subclip parents and nested projects are copied whole. Tests `plan_finds_used_ranges_and_dedupes_files`, `names_stay_unique`, `sizes_read_well`, ignored `archive_copies_trims_and_reopens_from_another_drive` (two files, trimmed with handles, the saved project rewritten to a dead drive letter and reopened through its relative paths). Screenshot `%TEMP%\milan-review\r25\archive-classic.png`.

**Scope**
- File > Export > Milan Pro Project Archive: copy the project and all media (option: trim media to used ranges plus handles, re-encoding with stream copy where possible) into a folder, with relative paths.
- Import > Media from Project: add another `.mnle`'s media to the pool.

**Files:** `interchange/archive.rs` (new), `ui/menus.rs`, `app/commands.rs`.

**Acceptance tests:** an archive opens from a new drive letter with no offline media; trimmed media keeps event timing (frame hash).

---

### R31: Plug-in manager and registry sources (M-L)

**Status: DONE for OFX; VST3/CLAP sources DONE in R34a/R35 (the VST folders live in Preferences › VST Effects and Plug-In Manager › Folders; `Binary::audio` in the same cache; the tree by kind/vendor, search by vendor and the compatibility report are still left).** Built (`engine/ofx/scan.rs`, `ui/plugin_manager.rs`): OFX bundles in `%CommonProgramFiles%\OFX\Plugins`, `OFX_PLUGIN_PATH` and user folders (Plug-In Manager › Folders, `prefs.ofx.folders`) are described in plug-in host processes and cached in `%LOCALAPPDATA%\MilanPro\plugins.json` by path, size and mtime; the Plug-In Manager (View › Window, Ctrl+Alt+1) lists built-ins and OFX plug-ins with OK / Failed (reason) / Disabled, a check box per OFX plug-in (`prefs.ofx.disabled`) and Rescan; the Video FX chooser lists enabled OFX plug-ins in a folder per grouping (VP16) and under Third Party (VP23); missing, disabled or failed plug-ins keep their saved settings and are bypassed, the FX window saying why. Tests: `scan_cache_invalidates_on_mtime_change`, `a_project_with_a_missing_ofx_plugin_keeps_its_parameters`, `plugin_manager_works`. Left: `PluginSource` for VST3/CLAP and the VST folders (Preferences › VST Effects, now labelled R34a), the tree by kind/vendor, search by vendor, "Export compatibility report" (the unmerged WIP branch `r31-plugin-manager` has a PE/moduleinfo reader and the report; reuse it with R34a).

**Scope**
- `PluginSource { Builtin, Ofx { bundle, index }, Vst3 { path, class_id }, Clap { path, id } }`.
- Scan cache at `%LOCALAPPDATA%\MilanPro\plugins.json` (path, size, mtime, status, error).
- Folders preference (the "VST Effects" Preferences page becomes live): default OFX paths `C:\Program Files\Common Files\OFX\Plugins` plus user folders, and VST3 `C:\Program Files\Common Files\VST3`.
- Plug-In Manager window: a tree by kind, vendor and category, search, enable/disable, status (OK / Failed / Blocked / Untested) with error text, Rescan, and "Export compatibility report" (Markdown/CSV).
- Choosers list enabled external plug-ins.

**Files:** `engine/plugins.rs`, `engine/plugin_scan.rs` (new), `ui/plugin_manager.rs` (new), `ui/choosers.rs`, `ui/dialogs/prefs.rs`, `settings.rs`.

**Acceptance tests:** scan cache invalidation on mtime change; a disabled plug-in is hidden from the choosers but a project using it still loads (FX shown as "disabled plug-in"); report export format.

### R32: Out-of-process plug-in host (L)

**Status: DONE for OFX video.** `milan-plugin-host.exe` (`src/bin/milan-plugin-host/`, a second `[[bin]]`, installed beside MilanPro.exe) serves a named pipe (length-prefixed JSON, `engine/ofx/proto.rs`, shared by both binaries); frames travel in a paging-file mapping (`CreateFileMappingW`, inputs then output, single-buffered). One host per binary, started on demand in the Job Object with `CREATE_NO_WINDOW`, no console, `SetErrorMode` + `WerSetFlags(NO_UI)` so a crash shows no dialog; the Job Object sets no UI limits, so a plug-in can make its own Vulkan/D3D device (`the_host_can_make_a_vulkan_device`: a Vulkan 1.1 instance and a compute queue from inside the host). Watchdog (`engine/ofx/client.rs`): load 10 s, describe 2 s per plug-in, render 30 s, none while a push button runs (the user has a Stop button); a crash, hang or plug-in error bypasses that plug-in for the session with a status-bar message, the FX window and Plug-In Manager show the reason (the plug-in's own message text verbatim), the event's fx button gets a red "!" badge in every look, Rescan retries. R33c probes add a hidden desktop, the background flag and a window watcher (`main.rs::probe_mode`). Tests: `crash_and_hang_are_contained`, `a_panicking_host_is_reported`, `the_host_can_make_a_vulkan_device` (all `#[ignore]`, need the built host). Left: audio blocks, double buffering, "isolate each instance" mode.

**Scope**
- `milan-plugin-host.exe`: a second `[[bin]]`, or a workspace member after R50.
- Milan talks to it over a named pipe (std plus windows-sys `Win32_System_Pipes`): length-prefixed JSON control messages. Frames and audio blocks travel in shared memory (`CreateFileMappingW` / `MapViewOfFile`, feature `Win32_System_Memory`), double-buffered.
- One host per plug-in format, or per plug-in instance in "isolate each" mode. The host is in the existing Job Object.
- Watchdog: no reply in 2 s (render: 30 s) → kill, mark the instance crashed, bypass it, show a red FX badge and log.
- Scanning also runs in a host, so a crashing scan never takes Milan down.

**Files:** `src/bin/milan-plugin-host.rs` (new), `engine/host_ipc.rs` (new), `proc.rs`, `Cargo.toml`.

**Acceptance tests**
- An IPC round-trip unit test with a fake in-process transport.
- A test host binary mode that panics on command: Milan survives, marks the plug-in failed and keeps playing.
- A timeout test.

### R33a: OFX host, describe and parameters (L)

**Status: DONE.** Hand-written OpenFX 1.4 bindings (`src/bin/milan-plugin-host/ofx.rs`, attribution in README › OFX plug-ins) and the host suites (`suites.rs`): property, image effect, parameter (Double, Integer, Boolean, Choice, RGB/RGBA, 2D/3D Double and Integer, String, Custom, Group, Page, PushButton; the variadic value functions read the Win64 argument slots), memory, multithread (real threads, recursive mutexes), message (logged and shown as the plug-in worded them, printf arguments formatted; no dialogs; questions answer No), progress, time line and a stub interact suite; parametric, dialog and GPU suites return NULL and are noted in the error ("suites asked for and not provided"). Describe runs load, describe and describeInContext (Filter, else General, Transition, Generator). Every parameter with a value is a `ParamSpec` and is stored (`ofx:<identifier>@<major>` with OFX parameter names as keys, so keyframes, presets and automation work as for built-ins); what the FX window shows follows the plug-in at run time (`engine/ofx::layout`: pages, groups that open and close, `kOfxParamPropSecret` hidden, `kOfxParamPropEnabled` greyed, labels and choice options as last set); push buttons send `kOfxActionInstanceChanged` to the instance in the host on a thread without a time limit; what the plug-in sets itself (`paramSetValue` in create instance, instance changed, a button or the render) comes back with the reply and the app writes it into the project (`Project::fx_by_id_mut`, `ofx::apply_edits`: a button is one undo step, starting values and render-time edits ride on the frame). A one-time hint in the FX window when an input colour space choice is at a camera-log default (`colour_hint`, dismissed per plug-in into `prefs.ofx.hints_seen`). Tests: `parameters_map_both_ways`, `layout_follows_pages_groups_and_state`, `describe_through_the_host`, `plug_in_edits_buttons_state_and_messages`, host `property_suite` / `variadic_parameter_values` / `messages_are_formatted` / `recursive_mutex_and_memory`. Left: parameter curves for `paramGetValueAtTime` at other times (the frame's values are returned), `kOfxChangePluginEdited` echoes, OFX 1.5 colour management, custom interacts, parametric parameters.

**Scope**
- Load `.ofx` with libloading 0.8 (in the lock; ISC) inside the host.
- Implement `OfxGetNumberOfPlugins` / `OfxGetPlugin`, the property suite, parameter suite (double, int, bool, choice, RGB/RGBA, 2D, string, group, page, pushbutton), image effect suite (describe, describeInContext for Filter, Transition and Generator contexts), memory, message and multithread suites.
- Map parameters to `ParamSpec` so OFX plug-ins use the existing FX window, keyframes and presets.
- Hand-written `#[repr(C)]` bindings from the OpenFX 1.4 headers (BSD-3; record the attribution in R56).

**Files:** `plugin_host/ofx/*.rs` (new), `engine/plugins.rs`.

**Acceptance tests:** the OpenFX sample plug-ins (BSD; build them with MSVC in a test step or fetch the release artifacts) describe correctly; the parameter mapping unit tests.

### R33b: OFX render path (L)

**Status: DONE.** `vfx::apply` sends `ofx:` effects to their host (`engine/ofx::apply`), `transition::mix` OFX transitions (`ofx::transition`: SourceFrom, SourceTo and the plug-in's `Transition` parameter set by the host) and `generate::render` OFX generators (`ofx::generate`, an instance per generated media at its media time): Milan's premultiplied frame goes to shared memory bottom row first as RGBA 8-bit, 16-bit, half or 32-bit float (the plug-in's best depth, float first), with time in frames, render scale (picture height over the full-quality height: event FX use the media's, track and output FX, transitions and generators the project's; the render window in that scale, RoDs in canonical coordinates), project PAR, field none; the host runs instance changed for parameters Milan changed, begin sequence render (per render scale; end when it changes or the instance closes), region of definition and of interest, is identity, then render, and the output comes back clamped to valid premultiplied colours. Preview and render share the path (`plan` → `composite` → `vfx`). Tests (`#[ignore]`, need the host and the test plug-in): `invert_matches_the_builtin_invert` (≤ 1 LSB, float and 8-bit), `gain_is_keyframed_at_t`, `transition_and_generator_render_through_the_host` (dissolve at p = 0.5, 16-bit solid colour), `render_matches_preview_with_ofx_plugins` (FFV1 render equals the compositor's frames, needs ffmpeg), `bench_ofx_roundtrip_1080p` (`docs/PERFORMANCE.md`). Left: temporal clip access (`clipGetImage` at other times returns the current frame), tiles, a CPU-side cache of OFX output, a transition plug-in's own edits (transitions have no instance id in the project).

**Scope**
- `kOfxImageEffectActionRender` for Filter, Transition and Generator with 8-bit RGBA, plus 16f/32f after R40b.
- Clip images via `clipGetImage` from shared memory; render scale for Draft/Preview; keyframed parameters at t; instance changed and begin/end sequence render.
- Wire into `composite.rs` as an `Fx` whose eval goes through IPC.

**Files:** `plugin_host/ofx/render.rs`, `engine/host_ipc.rs`, `engine/composite.rs`, `engine/worker.rs`, `engine/render.rs`.

**Acceptance tests:** an OpenFX sample "invert" matches the built-in invert (≤ 1 LSB); a transition sample at p = 0.5; the render path through IPC at 1080p under 10 ms overhead per frame (bench).

### R33c: OFX compatibility testing against installed bundles (M, RESEARCH plus data)

**Status: DONE (data: nothing runs).** `vegas15_compat_report` (`#[ignore]`, `src/engine/ofx/tests.rs`) runs every plug-in of the 7 VEGAS Pro 15 bundles in probe hosts (hidden desktop, background flag, window watcher, watchdog; `client::PROBE_LOG`, `main.rs::probe_mode`): load, describe, describeInContext and one 96x54 frame each, read-only, VEGAS never launched. The binaries export 79 plug-ins, not the 102 the XML resources suggested. Result in `docs/compat/ofx-vegas15.csv` and `docs/compat/ofx-vegas15.md`: 0 of 79 render. Every first-party plug-in asks for VEGAS's private suites (`OfxVegasEffectSuite`, `OfxVegasKeyframeSuite`, `OfxVegasProgressSuite v2`, `OfxVegasStereoscopicImageEffectSuite`; Spica: `OfxOpenCLProgramSuite`) and stops with `kOfxStatErrMissingHostFeature`; `Vfx1`, `Filters` and `Stabilize` also import VEGAS's own DLLs (`ErrorReport.dll`, `OpenColorIO.dll`, `proDADMercalli20.dll`) and do not even load from the bundle folder; HitFilm Vegas Basic (FXHOME, VEGAS edition) reads the host's name and fails its describe action; the first-party plug-ins also read a VEGAS-private host key (`OfxPropVegasSpikeKey`), which Milan does not fake. No dialog, no hang. The licence position is recorded; the VEGAS folder is not added to the default scan; nothing claims support. Host fixes from the run: load errors carry the OS error, action errors name the suites and host properties asked for in vain, a stub interact suite. Left: the `vegas_map` seed (R25 has not built the map), VEGAS's private suites (not a goal).

**Scope**
- Run describe and render for every plug-in in the 7 installed bundles (section 4.2), in an isolated host.
- Record per plug-in: loads? host check? licence prompt? renders? parameter count? crashes?
- Record the licence position (first-party VEGAS plug-ins are licensed for use with VEGAS; HitFilm Basic under FXHOME terms).
- Produce `docs/compat/ofx-vegas15.csv`, which feeds R55.
- Seed the `vegas_map` (R25) so a VEGAS import can use the real plug-in when it loads. (Done in R25: `interchange/vegas_map.rs` names the CSV's Vignette, Crop and LUT Filter; the rest become `ofx:` placeholders that load when a plug-in runs.)

**Acceptance:** the CSV has 102 rows, none of them "unknown"; nothing ships that claims support for a plug-in that failed.

### R34a: VST3 host, scan, process and parameters (L)

**Status: DONE.** `milan-plugin-host.exe` loads a `.vst3` module through the coupler-rs `vst3` bindings (`src/bin/milan-plugin-host/vst3.rs`): the factory's "Audio Module Class" effects (Fx sub categories; PClassInfo2 vendor/version/SDK), each set up once at scan to read its parameters (IEditController, IUnitInfo program lists), latency, tail and whether it has an editor; `ofx::scan` finds `.vst3` bundles (`Contents\x86_64-win`) and single files in `%CommonProgramFiles%\VST3`, `%LOCALAPPDATA%\Programs\Common\VST3` and the Preferences › VST Effects folders, cross-checks `moduleinfo.json`, and caches them in `plugins.json` (`Binary::audio`). Instances: IComponent + IAudioProcessor (stereo main buses, 32-bit float, the project rate, 1024-frame blocks) with their IEditController (a separate class or the component itself) connected directly, a host IHostApplication with IMessage/IAttributeList, IComponentHandler for edits and restarts, memory IBStreams, parameter changes per block (and a 0-sample flush before a state read). Milan side (`engine/aplug`): `vst3:<class id>` registry entries with `p<id>` controls (normalized values; stepped parameters as choices with the plug-in's step names, program change parameters with their program list), the chooser under the vendor, `aplug::Node` as an `afx::AudioFx` in the mixer's chains (blocks in shared memory, a host per module), a crash or hang bypassing the effect with a message exactly as OFX. Plug-in delay compensation (`mix::Lat`): every path from an event to the master is delayed to the longest latency and the mix runs that far ahead, pre-rolling after a seek, sample-exact (`pdc_aligns_a_latent_plug_in_with_a_dry_track`). Preview and render share the mixer (`render_matches_preview_with_a_vst3_fx`). Overhead per 1024-frame block: 0.012 ms over a built-in (`bench_vst3_block_overhead`, `docs/PERFORMANCE.md`). Tests (`#[ignore]`, with `tools/vst3-test-plugin`): `vst3_and_clap_modules_are_scanned`, `vst3_gain_applies_to_a_sine`, `crash_and_hang_are_contained`. Left: mono and surround bus layouts, sample-accurate parameter ramps within a block, MIDI/event buses, instruments.

**Scope**
- In the host process: `IPluginFactory` scan, `IComponent` / `IAudioProcessor` setup (32-bit float, the project rate, max block 1024), `IEditController` parameters → `ParamSpec` and automation (R8c), bypass, latency reporting → plug-in delay compensation in the mixer, tail.
- Bindings: the `vst3` crate (coupler-rs, MIT OR Apache-2.0; check the current version on crates.io before adding). Avoid `vst3-sys` (GPL-3.0).
- Record in R56 that the VST3 SDK moved to the MIT licence with VST 3.8 (verify when implementing).

**Files:** `plugin_host/vst3/*.rs` (new), `engine/mix.rs` (PDC), `engine/afx.rs` (external FX node), `Cargo.toml`.

**Acceptance tests:** a free test plug-in (the user installs, for example, Airwindows Consolidated VST3, MIT) processes a sine as expected; PDC aligns a 64-sample-latency plug-in with a dry track (sample-exact); a crashing plug-in is contained (R32).

### R34b: VST3 editor windows and presets (L)

**Status: DONE.** IPlugView in a top-level window the host owns (`editor.rs`: a thread with its own message loop; IPlugFrame resize requests and WM_SIZE both way; off every monitor and never activated in tests and the harness), opened and closed from the FX window's "Show plug-in UI" and polled ten times a second for the plug-in's edits (IComponentHandler::performEdit/beginEdit/endEdit → `aplug::take_edits`): the app writes them as FX automation keys at the cursor while the owning track or bus records automation (Write Touch/Latch) and plays, else as values, a gesture an undo step; the window closes with the FX window, the project and Milan (and dies with its host in the Job Object). `.vstpreset` load/save (`aplug::preset`: header, Comp/Cont/Info chunks and the chunk list, as `vstpresetfile.cpp` writes them) through `ASetState`/`AGetState`; the plug-in's state also travels in `Fx.text["state"]` (and `"cstate"`), refreshed when the editor closes. Program lists: IUnitInfo program names on the kIsProgramChange parameter, shown as a Program choice. Tests: `preset_round_trip_restores_parameters`, `editor_opens_off_screen_and_closes_with_the_host`. Left: IPlugViewContentScaleSupport (DPI), keyboard forwarding, CLAP editors.

**Scope:** `IPlugView` attached to a host-owned top-level HWND, opened from the FX window's "Show plug-in UI" with resize support; `.vstpreset` load/save; program lists; parameter changes from the GUI recorded as automation (Touch).

**Files:** `plugin_host/vst3/editor.rs`, `ui/fx_window.rs`, `engine/host_ipc.rs`.

**Acceptance tests:** a preset round-trip restores parameters; closing Milan closes plug-in windows; manual GUI check.

### R35: CLAP host and DirectX/DMO feasibility (M)

**Status: DONE.** CLAP (`clap.rs`, `clap-sys` 0.6): `clap_entry`, the plug-in factory, "audio-effect" descriptors, instances with the params (plain values; stepped and enum parameters with their texts), state, latency, tail and audio-ports extensions (stereo main ports), process with param value events in and out (edits and gestures back to Milan), a host object answering params/latency/state/log/thread-check/tail/audio-ports; `clap:<id>` entries in the same registry and mixer node, PDC alike (`clap_plug_in_processes_audio`, the CLAP track in the PDC test). DX/DMO: a read-only probe in the isolated host (`dmo.rs`: DMOEnum, CoCreateInstance(IMediaObject), float stereo types, one block) found no VEGAS `Audio_x64` DMOs on this machine (the folder is not installed) and ran Windows' own ParamEq DMO outside its host; report and licence position in `docs/compat/dx-audio.md`; no DX path ships.

**Scope**
- CLAP audio effects through `clap-sys` (MIT OR Apache-2.0) in the same host, reusing the R34a mixer node.
- DX/DMO: a research spike only. Instantiate one of the VEGAS `Audio_x64` DMOs through COM (`CoCreateInstance`, `IMediaObject`) in the isolated host, record whether it works outside VEGAS, and record the licence position.

**Deliverables:** a working CLAP path; `docs/compat/dx-audio.md` with results.

**Acceptance tests:** a CLAP test plug-in (for example Surge XT effects, GPL, user-installed) processes audio; the DMO report is written.

---

### A0-A9: AE-format (.aex) effect hosting (plan: `docs/AE_PREMIERE_PLUGINS.md`)

**Status: A0-A5 and A9 DONE (2026-11-26, experimental, off by default); A6 and A7 DROPPED for lack of a public source; A8 BLOCKED on decision 2.6(5).** Owner's decision of 2026-11-26: Milan's own declarations from public documentation, no Adobe SDK material (`CONTRIBUTING.md`, `tests/no_adobe_material.rs`, `docs/compat/aex-provenance.md`). A1: PiPL reader and writer (bounds-checked, in Milan's process as data), folders and search rules, Blocked/unsupported classification, Plug-In Manager rows and the experimental switch with the trademark notice. A2-A5: the host's `aex` personality (`src/bin/milan-plugin-host/aex/`): global and parameter setup into the OFX registry (`aex:<match name>`, keys `p<id>`), the tier-a callbacks and suites, 8-bit render through shared memory, sequence setup/resetup/flatten/setdown with `Fx.blob` saved in the project, supervision (changed values back to the project, greyed/hidden state to the FX window), checkout of other frames and layer parameters over a nested pipe request (`Reply::NeedFrame`). Tests: `engine::aex::tests` (`--ignored --test-threads=1`), host unit tests in `aex/{suites,convert,mod}.rs`. **Caveat: the declarations are proven self-consistent only** (Milan's own test plug-in); the tests over open-source plug-ins (`MILAN_AEX_TEST_DIR`) have not been run, and many layout facts are class R (not on any public page) in the provenance table. Left: run the §3.2 open-source plug-ins; ARB parameter callbacks; parameter curves for checkouts of values at other times; field rendering; `PF_Cmd_DO_DIALOG`; float (needs a public description of Premiere's pixel-format suite); transitions (needs one of the transition suite).

---

### R37: Format capability system, test corpus, format report (L)

**Status: DONE.** `engine/caps.rs`: the bundled FFmpeg's demuxers, decoders, encoders, muxers, pixel formats and filters (`-hwaccels` is R22's), cached per FFmpeg version in `%LOCALAPPDATA%\MilanPro\caps.json` (the harness and tests never write it). A stream whose codec has no decoder imports as an offline item with "unsupported codec <name>: ..." (`MediaItem::unsupported`; a "!" badge in Project Media, the reason in the import message and Media Properties; never decoded, proxied or drawn). Media Properties shows the verdict: decode (software, or the R22 hardware path), proxy, and the render templates that write the same codec. `tools/make-test-media.ps1` runs the ignored `engine::formats_e2e::format_corpus_e2e`, which generates the legal corpus from lavfi (35 files: H.264 8/10-bit 4:2:0/4:2:2, HEVC 8/10 incl. 2160p, AV1, VP8, VP9, MPEG-1/2/4, XDCAM HD422 and DNxHD in MXF OP1a, ProRes 422/4444 alpha, DNxHR, CineForm, MJPEG, Theora, FFV1, qtrle, PNG-in-MOV, Ut Video, VFR; WAV 16/24/32f, AIFF, MP3, AAC, FLAC, ALAC, Vorbis, Opus, WMA, AC-3 5.1, E-AC-3 7.1, DTS 5.1; 23.976 to 59.94 and 29.97i; mono to 7.1) into `MILAN_TESTMEDIA` (default `target/testmedia`), imports each file, checks frame 0 and the middle frame against the SMPTE bars through the engine's decoder, mixes 1 s and checks its RMS, and rewrites the decode, capability and camera tables of `docs/FORMAT_SUPPORT.md` (with a licence column). The spec's `tests/formats.rs` is a module of the binary instead (Milan Pro has no library crate). Tests: `capability_parser` (captured output), `verdict_rows`, `unsupported_codec_is_an_offline_item`, ignored `bundled_ffmpeg_capabilities`, `format_corpus_e2e`.

**Scope**
- `engine/caps.rs`: parse `ffmpeg -decoders -encoders -demuxers -muxers -hwaccels` of the bundled FFmpeg at first run (cached).
- On import, a stream whose codec has no decoder becomes an offline item: "unsupported codec <name>". Never substituted.
- `tools/make-test-media.ps1` generates a legal corpus with lavfi (`testsrc2`, `smptehdbars`, `sine`, `anoisesrc`) covering: H.264 (8/10-bit, 4:2:0/4:2:2), HEVC (8/10), AV1, VP8, VP9, MPEG-1/2/4, ProRes 422/4444 alpha, DNxHD/HR, CineForm, MJPEG, Theora, FFV1, qtrle, PNG-in-MOV, UT Video; containers MP4, MOV, MKV, AVI, TS, PS, WebM, MXF (OP1a), WAV, AIFF; audio AAC, MP3, FLAC, ALAC, Vorbis, Opus, WMA, PCM 16/24/32f, AC-3, E-AC-3, DTS; rates 23.976/25/29.97i/50/59.94 and VFR; resolutions 640×360 to 3840×2160; mono, stereo, 5.1, 7.1. Nothing is committed; it is generated into `target/testmedia`.
- An ignored integration test imports, probes, decodes frame 0 and the middle frame (pixel check against the known test pattern), plays 1 s of audio (RMS check) and writes `docs/FORMAT_SUPPORT.md` (format, decode, encode, notes, licence).

**Files:** `engine/caps.rs` (new), `media.rs`, `tools/make-test-media.ps1` (new), `tests/formats.rs` (new integration test), `docs/FORMAT_SUPPORT.md` (new, generated).

**Acceptance tests:** a capability parser unit test on captured output; the unsupported-codec path gives an offline item with the reason; the corpus test passes for every row marked supported.

### R38: Image sequences and still formats (M)

**Status: DONE.** A picked or dropped numbered still asks "Open still image sequence" (`ui/dialogs/sequence.rs`, VEGAS's check box, remembered in Preferences); checked, the run from its lowest number to the first gap is one video item at the project's frame rate (`MediaItem::sequence`, pattern beside its first file so it moves with the project; image2 in every reader; no proxy; Archive Project copies every frame). DPX, JPEG 2000, Radiance HDR, DNG, SVG and JPEG XL joined `IMAGE_EXTS` (SVG imports offline with the reason: the bundled FFmpeg has no librsvg; DNG frames are not offered as a sequence, CinemaDNG is listed unsupported). Animated GIF, WebP and APNG are video, read from their start (a seek lands between dependent frames). Linear stills (EXR, Radiance HDR) go through a simple sRGB transfer, with Radiance HDR tone-mapped first, in thumbnails, the timeline and renders, noted in Media Properties ("linear EXR shown with a simple sRGB transfer" until R40b). Found on the way: ffmpeg 9 decodes nothing for `-ss 0` on a JPEG or WebP, so a zero seek is left out. Render to PNG, TIFF, OpenEXR and DPX sequences was R11. Tests: `numbered_sequences_are_detected`, `resolve_collapses_sequences`, `animated_pictures_are_video`, ignored `image_sequence_e2e` (48 PNGs at 24 fps last 2.0 s, the 10th frame is file 0010, a gap stops the run, a PNG sequence render gives every file back pixel for pixel, the still formats FFmpeg writes decode, alpha survives) and `animated_gif_e2e` (a 10-frame GIF and WebP play 10 distinct frames).

**Scope**
- Detect numbered sequences on import and drop (`name_0001.ext` with consecutive numbers). Ask with "Open as image sequence" in a Milan modal (rfd cannot add a checkbox).
- `MediaItem.sequence: Option<Seq { pattern, start, count, fps }>` decoded with `-f image2 -start_number N -framerate R -i pattern`.
- Add DPX, JP2, Radiance HDR and DNG to `IMAGE_EXTS`.
- Animated GIF and WebP become Video.
- EXR sequences, with linear→sRGB handling from R40b when it exists (until then, an 8-bit sRGB transform with the note "linear EXR shown with a simple sRGB transfer").

**Files:** `media.rs`, `project.rs`, `engine/vdecode.rs`, `app/drop.rs`, `ui/dialogs/sequence.rs` (new).

**Acceptance tests:** a 48-frame PNG sequence at 24 fps lasts 2.0 s and frame 10 equals file 0010; a gap in the numbering stops the sequence; a GIF with 10 frames plays 10 distinct frames.

### R39: Subtitles and closed captions (M-L)

**Status: DONE**, closed captions NOT FEASIBLE (below). As VEGAS has it, a subtitle is a Titles & Text event: there is no subtitle track type.
- `interchange/subtitles.rs` reads SubRip (.srt; `<i>`, `<b>`, `<u>`, `<font>` and `{\anN}` tags left out), WebVTT (.vtt; NOTE / STYLE / REGION blocks skipped, tags left out, entities decoded) and Advanced SubStation Alpha (.ass, .ssa: PlayRes, [V4+ Styles] / [V4 Styles], Dialogue lines; override blocks left out, \N line breaks), UTF-8 with or without a BOM (other bytes as Latin-1), and writes SRT and WebVTT. Times are kept to the millisecond, so SRT import → export is text- and time-exact.
- Insert › Subtitles from File... (`ui/dialogs/subtitles.rs`): the file's cues listed, at the file's times or from the cursor, on a new "Subtitles" video track at the top or a chosen one; each cue is a Titles & Text event with media of its own, the Subtitle preset or (ASS, on by default) its style: font, size (of PlayResY), primary / outline / back colours, outline width, shadow, the numpad alignment and margins. Bold, italic, karaoke, drawings and per-character overrides are not drawn (Titles & Text has one font and style per event); an ASS opaque box (BorderStyle 3) is drawn as an outline.
- Insert › Subtitles from Regions: each named region becomes a subtitle on a new track.
- File › Export › Subtitles...: a track's Titles & Text events (muted ones left out) as SRT or WebVTT.
- Render As › Subtitles: Burned in only (default: the events are video), or also an SRT / WebVTT file beside the render, or a soft subtitle stream (stream copy remux after the render: `mov_text` in MP4 / MOV, SubRip in MKV, WebVTT in WebM); "Burn the subtitles into the picture" can be turned off, which leaves the track out of the picture. Cues are clipped to the rendered range and counted from its start.
- Closed captions are NOT FEASIBLE with the bundled FFmpeg: it has no CEA-608/708 encoder, and its decoder reads them only from a video stream's A53 side data, which Milan Pro's frame decode does not keep (`movie=...[out+subcc]` needs a lavfi source per file outside the decode path, and no test sample is available without downloading one). Import › Closed Captioning... says so.
- Tests: `srt_round_trip`, `webvtt_reads_and_writes`, `ass_imports_with_styles_mapped`, `srt_import_then_export_is_exact` (pure); `subtitles_render_as_a_sidecar_and_a_soft_stream` (ignored, FFmpeg: an MP4 probes with a `mov_text` stream, an MKV with SubRip, the sidecar SRT equals the export).

**Scope**
- Import SRT, VTT and ASS (Insert > Subtitles from Files) as text events on a subtitle video track (`milan.gen.text` "Subtitle" preset) or as regions.
- Insert Subtitles from Regions.
- Export SRT/VTT from the subtitle track or regions (like VEGAS's `Export Regions as Subtitles.cs`).
- Render options: burn in (it is already video), or embed a soft subtitle stream (`-c:s mov_text` for MP4, `webvtt` for WebM, `srt` for MKV).
- Closed captions: Import > Closed Captioning extracts CEA-608 from media through `movie=FILE[out+subcc]` lavfi → SRT → caption track.
- Embedding 608/708 in output is NOT FEASIBLE with this FFmpeg (no CEA-608 encoder). The deliverable is SCC/SRT sidecar export, documented in FORMAT_SUPPORT.md.

**Files:** `interchange/subtitles.rs` (new), `engine/render.rs`, `render.rs`, `edit.rs`, `ui/menus.rs`, `app/commands.rs`.

**Acceptance tests:** an SRT parse/write round-trip with UTF-8, multi-line text and overlapping cues; an MP4 with mov_text subtitles probes with a subtitle stream; a 608 test file (lavfi cannot make one; use a user-provided broadcast sample or a fixture from the FFmpeg FATE suite, LGPL data) imports its captions.

### R40a: HDR detection and correct SDR display (M)

**Status: DONE.** Probed: `color_transfer`, `color_primaries`, matrix, bit depth (`bits_per_raw_sample`, else the pix_fmt) and HDR10 mastering display / content light level (stream side data, else the first frame's SEI). Media Properties shows them with the colour space as VEGAS names it and how the source reaches the timeline; Project Properties › Video lists the project's HDR and wide-gamut media and has Color management (VEGAS's View transform row): SDR Rec.709 with HDR tone-mapped (default), SDR with HDR clipped, or Off. `engine/color.rs` builds the view transform (zimg) that `plan::media_ref` puts ahead of the scaler, so the preview, Trimmer, thumbnails, proxies (made through it) and render all see the same picture: PQ is taken to linear with reference white 203 cd/m² (BT.2408) as 1.0, HLG decoded for a 1000 cd/m² display and exposed to the same white, both converted to Rec.709 primaries and rolled off with Möbius above 50 % up to the mastering peak (1000 cd/m² without metadata); BT.2020 SDR is converted to Rec.709; SDR sources get nothing. Differences from the spec: Möbius at 203 cd/m² instead of Hable at 100 (Hable at 100 darkens SDR-graded content by about a stop; Möbius keeps everything under the knee exact), and the badge reads "HDR" in Project Media. Tests: `view_transform_per_transfer` (args per transfer), `hdr_tags_are_read`, ignored `hdr_view_transform_e2e` (generated PQ and HLG clips within 2 levels of FFmpeg's own tone map of the same file and within 5 % of the SDR original on the 40 % and 75 % greys; an SDR clip bit-identical with the transform on and off).

**Scope**
- Probe `color_transfer`, `color_primaries`, bit depth (`bits_per_raw_sample` / pix_fmt) and mastering metadata.
- HDR (PQ or HLG, BT.2020) sources are tone-mapped to BT.709 SDR in the decode filter (`zscale=t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,tonemap=hable:desat=0,zscale=t=bt709:m=bt709:r=pc,format=rgba`) and badged "HDR → SDR".
- BT.2020 SDR is converted to 709.
- This ends the silent misinterpretation (7.58 and 7.65).

**Files:** `media.rs`, `project.rs` (`MediaItem.transfer`, `primaries`, `bit_depth`), `engine/vdecode.rs`, `ui/media_panel.rs` (badge), `ui/properties.rs`.

**Acceptance tests:** an args unit test per transfer; a generated PQ test clip (lavfi with `zscale` tagging) shows mid-grey within ±5% of the SDR reference.

### R40b: High-bit-depth and HDR pipeline (L, after R21a and R21c)

**Status: DONE.** Built as designed below, where VEGAS or the measurements differ:
- Project Properties › Video has VEGAS 18-23's rows (`project/pixel.rs`, `ProjectSettings.pixel`, saved only when not 8-bit): Pixel format (8-bit, 32-bit floating point (video levels), 32-bit floating point (full range); full range switches Compositing gamma to 1.000 as VEGAS does), HDR Mode (Off, HDR10, HLG: full range, linear, the Rec.2020 view), Compositing gamma (2.222 (Video), 1.000 (Linear)), ACES version (1.0, 1.1: the same transforms), ACES color space (ACES2065-1, ACEScg, ACEScc, ACEScct) and View transform (Off, sRGB (ACES), Rec.709 (ACES), Rec.2020 ST 2084 1000 nits (ACES), Rec.2020 HLG 1000 nits (ACES), Custom LUT (.cube)). A row a format does not use is greyed with the reason; Look modification transform stays greyed (a look LUT goes on the Video Output FX). R40a's Color management still says how HDR reaches an SDR working space; with an ACES view HDR comes in as scene light instead. VEGAS project import and export carry the pixel format.
- `engine/wide`: a float project's working space (`Working`: video-coded or linear Rec.709, or the ACES color space with an ACES view) and pictures (`WImage`, premultiplied f32). Sources decode at 16 bits (`rgba64le`, swscale with exact rounding) or, OpenEXR and float TIFF, in float (`gbrapf32le` through zimg, which keeps values above 1.0); an 8-bit source keeps its 8-bit decode. Each source's `Input` (in `MediaRef::wide`) takes it into the working space: as coded, the 2.222 power, PQ or HLG (BT.2408: 203 cd/m² is 1.0), the inverse of an ACES SDR view (so SDR media shows as coded), and the primaries matrix. Frames ride the existing decoders and caches as `ColorImage`s 2 or 4 times as wide, under an id of their own.
- Compositing: `wide/compose.rs` is `composite.rs` in f32 (same order, same 16.16 sample positions); `gpu/wide.rs` runs the draws, every blend mode, opacity, Fade to Color and compositing groups on `Rgba16Float` (sources uploaded as `Rgba32Float`), each event's picture, a track with track FX, 3D motion, halos or a Custom compositor, and a transition with a kind made by the CPU's float code; Adjustment FX, Video Output FX and a LUT view leave the frame to the CPU. The GPU also renders the view into the 8-bit texture the preview shows without a copy. Blends clamp to 0..1 in a video-coded space (the 8-bit look) and keep highlights in linear light.
- FX see the working values, as VEGAS hands them to its FX. In float: every `ColorOp` colour FX (its LUT read between entries), Color Grading (`Grade::rgb`), Gaussian Blur, masks, dissolves and OFX plug-ins (at their float depth); every other FX, transition and generator, the MVFX plug-ins included, runs on an 8-bit copy (`via8`) that keeps exactly the pixels it left alone and the sub-8-bit part of the rest, as VEGAS runs an 8-bit-only plug-in in a float project. Generated media are 8-bit SDR brought in.
- Views: Off (the inverse of the input), the ACES SDR views as Stephen Hill's fit of the RRT and ODT (not the reference CTL: OpenColorIO is C++; an exact transform goes in as a .cube LUT), and HDR10 / HLG for a 1000 cd/m² display (exact up to 750 cd/m², Möbius above). An HDR project's preview, scopes and snapshots are tone-mapped to SDR as R40a does.
- Render (`engine/render.rs` `Pipe`, `Feed`): an 8-bit project into an SDR template is fed exactly as before; a float project feeds 16-bit RGBA through its view (exact rounding into YUV), an OpenEXR render linear float planes; an HDR template (`VideoEnc.color`) gets Rec.2020 tags and, HEVC, x265's HDR10 SEI (`master-display`, `max-cll=1000,400`); an SDR project into it puts SDR white at 203 cd/m². New templates: MP4 HEVC Main10 (10-bit), MP4 HEVC HDR10 (PQ), MP4 HEVC HLG, ProRes 4444 XQ, ProRes 4444 XQ HDR10 (PQ) (tags only: FFmpeg's MOV muxer gets no mastering metadata from raw frames).
- 8-bit stays bit-identical: `engine/bitexact.rs` hashes 200 random frame plans, every built-in plug-in at each preset, and an FFV1 render of `render_e2e`'s project as the base engine made them.
- Spec deviation: "unique values ≥ 900 of 1024" cannot hold for video-range luma (877 codes): `ten_bit_gradient_renders_without_banding` asks 850 of the 877 (the 8-bit project's render: at most 256).
- Tests: `transfers_and_primaries_round_trip`, `sdr_media_round_trips_through_every_working_space`, `float_compose_follows_the_8bit_compose`, `gpu_float_matches_cpu_float`, `fx_keep_float_precision`, `pixel_settings_save_and_load`, `vegas_switches_and_old_files`, the template arguments in `encoder_argv_for_every_template`; ignored, with FFmpeg: `ten_bit_source_round_trips_through_a_ten_bit_render`, `ten_bit_gradient_renders_without_banding`, `hdr_projects_render_hdr10_and_hlg` (ffprobe: `smpte2084`, `bt2020`, the mastering display and content light level side data; HLG `arib-std-b67`), `linear_exr_keeps_its_highlights`, `eight_bit_render_is_bit_identical_to_the_base_engine`. GPU parity (the Z1 Extreme's integrated Radeon): channels over 2e-3 of the CPU's: 0.010 % video-coded, 0.015 % linear, 0.18 % ACEScg (scene light far over 1 through cancelling blend formulas); half floats round toward zero on it, so alpha is computed as `da + sa (1 - da)` to keep an opaque backdrop exactly opaque on both. Bench (`bench_compose`'s `compose_float` rows, `tools/bench.ps1`; measured while other builds ran, so not yet in PERFORMANCE.md): four 1080p layers 38 ms on the CPU and 50 ms on the GPU (the four pictures uploaded as `Rgba32Float`, 133 MB), picture-in-pictures 26 / 12 ms, four 4K layers 233 / 502 ms; 8-bit in the same run 5.5 ms GPU at 1080p. The GPU float path is upload-bound on this integrated GPU; uploading half floats is the next step if it matters.

**Scope**
- An optional 16-bit float compositing mode on the GPU path (Project Properties "Pixel format: 32-bit floating point (video levels/full range)", as in VEGAS): decode as `rgba64le` or `gbrpf32le` → `Rgba16Float` textures.
- Linear EXR handling.
- HDR render templates: HEVC Main10 PQ and HLG with mastering metadata (`-x265-params hdr10=1:master-display=…:max-cll=…`) and ProRes 4444 XQ.
- An HDR preview is shown tone-mapped (egui renders SDR).
- ACES/OpenColorIO is NOT FEASIBLE in pure Rust (OCIO is C++). The deliverable is LUT-based input/output transforms (.cube) and the note in FORMAT_SUPPORT.md.

**Files:** `engine/gpu/*`, `engine/vdecode.rs`, `engine/render.rs`, `render.rs`, `settings.rs` (pixel format combo live).

**Acceptance tests:** a 10-bit gradient renders with no banding (unique values ≥ 900 of 1024 in a ProRes 10-bit output); a PQ render's ffprobe shows `smpte2084` and the mastering side data.

### R10a: Multichannel audio engine and export (L, after R8a)

**Status: DONE**, except the master bus envelopes, moved to R10c. Built as designed below, where VEGAS differs:
- Layouts: Project Properties › Audio › Master bus mode is Stereo or 5.1 Surround (VEGAS) or 7.1 Surround (Milan Pro's). Each audio track has a Channel Layout (track header menu, Mixing Console label menu): Stereo (VEGAS's track, the default), Mono (its events summed to one channel heard on both sides), 5.1 or 7.1 (`project::AudioLayout`). A bus has one too, or follows the master (the default, as VEGAS's busses do). Channel order is FFmpeg's: L R C LFE Ls Rs (5.1), L R C LFE Lb Rb Ls Rs (7.1).
- Decode: a stereo track decodes stereo as before (a 5.1 source folded down by FFmpeg's `-ac 2`, its BS.775 matrix); a 5.1 / 7.1 track decodes to its layout (`-ac 6` / `-ac 8`: channel for channel when the media already is, stereo into the front pair, mono into the centre, as FFmpeg upmixes). A nested timeline's mix is spread the same way.
- Routing (`mix::route`, `engine/surround.rs`): a stereo track into a 5.1 / 7.1 bus or master is placed in the room by its surround pan (`surround::pan_matrix`: the track's Pan Type left to right, front to back by the same law, the centre speaker taking the mono sum near the middle; speakers can be switched off; LFE only; Smoothness), which R10b's Surround Panner edits and keys. A node of the same layout goes channel for channel; another layout is converted (`surround::convert`: BS.775 fold-down to stereo with the LFE dropped, 7.1 sides into 5.1 backs, 5.1 into 7.1 with silent sides). A 5.1 / 7.1 node's Pan is a left-right balance (the centre and LFE keep their level). Event, track, bus and master FX run per stereo pair of a wider node (a chain each; external VST3 / CLAP plug-ins on the front pair only: they are stereo instances). Delay compensation, varispeed (`Resampler::read_n`), multicamera standby takes and quickfades work at any width.
- Stereo projects mix bit for bit as before (`stereo_mix_is_bit_identical_to_the_pre_r10a_engine`, hash taken on the base engine).
- Playback: a 5.1 / 7.1 master is metered per channel and fed to the Loudness Meters by the mix thread, then folded down (BS.775) for the stereo device; Preferences › Audio Device's rear and center/LFE devices are explained as not offered (the render has the channels).
- Render: the mix goes to FFmpeg at the template's channels with `-channel_layout`; 8 new templates (WAV / FLAC 5.1, WAV 7.1, AAC / AC-3 / E-AC-3 5.1, MP4 H.264 with AAC 5.1, MPEG-2 TS with AC-3 5.1); Customize Template offers 5.1 / 7.1 where the codec takes them (AC-3 and E-AC-3 up to 5.1, MP3 / MP2 stereo). A stereo project into a surround template fills the front pair, or with Enable multichannel mapping puts each bus that goes to the master on the next pair. Loudness logs and the Loudness Meters weigh 5.1 / 7.1 channels as BS.1770-4 does (surrounds +1.5 dB, LFE left out).
- Project Properties › Audio › Resample and stretch quality: Preview / Good / Best are 16 / 32 / 64 taps of the varispeed and shuttle resampler (Good is what every project had).
- Tests: `a_5_1_source_plays_each_channel_to_its_position`, `track_and_bus_layouts_route_by_width`, `downmix_and_convert`, `front_rear_centre_and_lfe`, `constant_power_and_smoothness` (pure); `surround_renders_match_ffmpeg` (ignored, FFmpeg: a 5.1 and a 7.1 source render to WAV equal to the source within 1e-4, the mixer's fold-down equals FFmpeg's `-ac 2` within 1e-4, a 5.1 AC-3 render probes as 6 channels with each sine in its own channel by Goertzel, a stereo source at the front-left pan point lands on L alone).

**Scope**
- Master bus mode 5.1 (and 7.1).
- The mixer is N-channel (interleaved `Vec<f32>` with `channels` in the plan).
- Sources decode to their native layout (no `-ac 2`) with a channel map.
- Stereo-to-5.1 pan law; downmix for stereo monitoring (ITU-R BS.775).
- Multichannel render (AAC 5.1, AC-3, E-AC-3, PCM, FLAC) with `-channel_layout`; "Enable multichannel mapping" (tracks to channels).
- From R8c: master bus envelopes (a Master bus track) and the Master Bus window's Automation Settings badge (done in R10c).
- From R4b (re-assigned by R18b): Project Properties › Audio › Resample and stretch quality (Milan Pro resamples with one 32-tap windowed sinc today).

**Files:** `engine/mix.rs`, `engine/plan.rs`, `audio.rs`, `engine/render.rs`, `render.rs`, `project.rs` (`AudioSettings.layout`).

**Acceptance tests:** a 5.1 source plays each channel to its position (a mixer test with 6 distinct sines); the downmix matrix sums correctly; a 5.1 AC-3 render probes as 6 channels with the sines in the right channels (FFT-free: Goertzel at each frequency).

### R10c: Master bus envelopes (S-M, after R10a)

**Status: DONE**, except the per-channel envelopes of a 5.1 / 7.1 master (one Volume envelope gains every channel). Built as designed below:
- Model: `MasterBus.audio_envelopes` (Volume and Mute, timeline time), `automation` and `auto_controls`, skipped when default, so old projects load and save as before. The master is an envelope and automation owner like a bus (`edit::MASTER`, "master"): `envelope_of`, `with_envelope`, `ensure_envelope`, `toggle_bus_envelope`, `automation_of` and `set_automation` take it, so automation writes, point edits and undo work unchanged.
- Engine: `AudioPlan.master_vol_env` / `master_mute_env` (left out with automation Off), applied by the mixer after the master FX and the plug-in delay lines, ramped across each block as a track's envelopes are, at the timeline time the block is heard; playback and the render share the plan, so they hear the same envelope. The master fader stays after it (device or render).
- Master bus track: View › Audio Bus Tracks shows a Master row last, with the Audio Master FX button, Vol (the master gain; with Show Automation Controls and a mode other than Off, the Volume envelope at the cursor, written in Touch / Latch), the master meter, and the Volume / Mute envelope lane (Insert/Remove Envelope, drags, double-click adds, point menu deletes, Automation ▸ in the header menu).
- Master Bus window: the strip's automation badge (between fx and M) shows the mode's letter (R, T, L; empty for Off), in the Volume envelope colour while the controls are shown; a click drops Show Automation Controls and the four modes. With automation controls the window's fader shows and writes the Volume envelope instead of the master gain.
- Tests: `master_volume_envelope_matches_a_track_envelope` (the same ramp on the master and on every track gives the same samples within 1e-5; Mute silences; Off ignores; playback's and the render's plans mix it the same; the project file keeps it), `master_automation_modes_and_envelopes` (the badge's four modes and controls, an automation write adding the envelope, Volume and Mute only).
- Not done: per-channel master envelopes in 5.1 / 7.1; the Mixing Console's master strip fader does not follow the automation (the bus track and the Master Bus window do).

**Scope:** moved from R10a (where R8c had put it): master bus Volume (and in 5.1 / 7.1 per-channel) envelopes on a Master bus track, played and rendered, and the Master Bus window's Automation Settings badge.

**Files:** `ui/master_bus.rs`, `ui/timeline/bus.rs`, `engine/render.rs` (`mix_range`'s master gain), `audio.rs`.

**Acceptance tests:** a master Volume envelope ramp plays and renders the same; the badge sets Off / Read / Touch / Latch.

### R10b: Surround Panner and stream selection UI (M)

**Status: DONE.** As designed below, where VEGAS differs:
- The Surround Panner (`ui/surround.rs`; View › Window › Surround Panner, Ctrl+Alt+3, on the focus audio track; the track header menu; a click on the track header's room): the room from above with a speaker icon per channel of the master bus mode (each shows its gain; a click switches it off or on), the pan point (drag; a click in the room moves it there; a double-click returns it to the front centre), Pan type Smooth (the track's own Pan Type) or Constant Power, Smoothness, LFE only, and the keyframe controller. Keyframes are the track's Surround X / Y envelopes (`EnvKind::SurroundX/SurroundY`, shown as envelopes on the track and played by the mixer); without keyframes the pan point is the track's static surround pan. A stereo project shows the 5.1 room and says the master bus mode is Stereo.
- Track headers: in a 5.1 / 7.1 project a stereo track's Pan row shows the room in miniature with its pan point (VEGAS's surround pan control); a click opens the Surround Panner.
- Mixing Console: a 5.1 / 7.1 node's peak meter has a bar per channel (named under it) and one readout; track headers and VU meters show the left and right sides of it (`MeterView::stereo`). The master of a 5.1 / 7.1 project is metered by the mix thread before the fold-down.
- Channels ▸ of a multichannel stream adds Channels 1 and 2, 3 and 4, ... and All Channels (Downmix) (`Clip.pair`); Stream ▸ Add All Streams as Tracks puts the event's other streams on new tracks under it; a drop of media with several audio streams says so in the status bar (VEGAS asks; Milan Pro does not interrupt the drop). Event Properties › Media lists the audio streams and the channels.
- The window is drawn like the application's other windows (`docs/UI_SPEC.md`).

**Scope:** the Surround Panner window (a 2D puck per track with keyframes → pan envelopes per channel); the Stream submenu itself was built in R3b; a "Media has N audio streams" warning on import with an option to add all streams as separate tracks (as VEGAS does).

**Files:** `ui/surround.rs` (new), `ui/timeline/ctx.rs`, `ui/menus.rs`, `app/drop.rs`, `edit.rs`.

**Acceptance tests:** a panner puck at front-left gives an FL gain of 1 and others 0; a 3-stream MOV imports as 3 audio events when asked.

### R41: Professional camera formats (M)

**Status: DONE** (on FFmpeg-made stand-ins; no camera originals). File › Import › Camera Card Folder, or a folder dropped on Project Media or the timeline, scans AVCHD, XDCAM EX/HD, XAVC, P2, Canon XF and GoPro layouts (`camera.rs`); AVCHD parts whose clock runs on and GoPro chapters join into one item through a `<first>-span.ffconcat` list beside the first part (a read-only card imports the parts one by one and says so). Found and fixed: ffmpeg seeks MPEG transport and program streams and concat lists to the next keyframe, so every TS/MTS source showed the wrong frame after a seek; the readers now open those `plan::PREROLL` (3 s) early and trim to the target. Camera make/model (QuickTime tags, a camera MXF's identification; FFmpeg's own is ignored), reel and the start timecode (tags, tmcd, MXF, H.264 SEI) show in Media Properties; "Use Media Timecode as Ruler Start" in the item menu sets the ruler offset from it. RAW formats (BRAW, R3D, ARRIRAW, Canon RAW Light, N-RAW) import as offline items with the reason; CinemaDNG is listed unsupported. New template "MXF DNxHD 145 1080". Not built: XDCAM EX and Canon XF span joining (needs their XML), P2 picture and sound as one item, the optional BRAW SDK study. The timecode SEI path is unit-tested only (FFmpeg cannot write H.264 timecode SEI for a stand-in). Tests: `spans_by_timestamps_and_gopro_chapters`, `card_layouts_are_found`, `camera_tags_and_sei_timecode`, ignored `camera_formats_e2e` (a spanned AVCHD pair is one item of the summed length whose every frame, through fresh seeks, equals FFmpeg reading the list straight through; XAVC-style MP4, XDCAM HD422 / DNxHD / AVC-Intra 100 MXF with timecode; a P2-style OP-Atom card; a .braw offline with the reason).

**Scope**
- Test and fix XAVC-S/XAVC (MXF), XDCAM HD422, P2 (AVC-Intra MXF), AVCHD, Canon XF, GoPro (multi-stream).
- Join spanned clips (AVCHD `.MTS` series, P2/XDCAM folder structures) into one media item through the concat demuxer.
- Read embedded timecode (done in R17a: `MediaItem.timecode`; check it on AVCHD and XAVC files here).
- RAW: document NOT FEASIBLE for R3D/BRAW/ARRIRAW; keep an optional research note on runtime-loading the free BRAW SDK if the user installs it (licence forbids redistribution).

**Files:** `media.rs`, `engine/vdecode.rs`, `project.rs`, `docs/FORMAT_SUPPORT.md`.

**Acceptance tests:** a synthetic spanned AVCHD pair (two TS files with continuous PTS) imports as one item of summed duration; the timecode tag is parsed.

---

### R42: Project schema versioning, backups, validation, Save Version (M)

**Status: DONE.** Built as designed below. `format_version` 1 is the first versioned format (0.7.0 and earlier files are 0 and migrate on open: `migrate::v0_to_v1` drops the `null` optionals the old builds wrote); a newer file opens read-only with a note and Save becomes Save As. `project/validate.rs` repairs duplicate ids, unknown take media, NaN/negative/infinite times, duplicate takes and zero settings, and lists unknown event media (left offline); the notes show in an "Open Project" report. Save Version follows VEGAS 23 (the menu item is in that look only; Ctrl+Alt+S works in all) and keeps editing in the new numbered file; a project never saved is saved as first. Tests: `project::migrate::tests`, `project::validate::tests`, `project::tests::{newer_format_opens_read_only_and_v0_migrates, save_keeps_the_previous_file_as_bak, a_truncated_file_offers_the_backup}`, `dialogs::save_version::tests`.

**Scope**
- `Project.format_version: u32`, where 0 means pre-versioning; current = 1.
- `migrate(value: serde_json::Value, from) -> Value` steps, each with a test.
- A file with a newer version opens read-only with a warning, and Save becomes Save As. Never silently down-save.
- On save, keep the previous file as `<name>.mnle.bak` (VEGAS writes `.veg.bak`).
- A validation pass after load: unknown media ids, duplicate ids, NaN or negative times and durations, overlapping takes. It repairs where safe, reports the rest, and never touches the original file until the user saves.
- A corrupt JSON file offers "Open the backup (.bak) / latest autosave".
- File > Save Version (Ctrl+Alt+S): `name_01.mnle`, `name_02.mnle`... with an optional note.

**Files:** `project.rs`, `project/migrate.rs` (new), `project/validate.rs` (new), `app/commands.rs`, `ui/menus.rs`.

**Acceptance tests**
- A v0 file migrates to v1.
- A future-version file opens read-only.
- Save twice leaves the `.bak` equal to the previous content.
- A truncated JSON file offers the backup.
- The validator repairs a NaN position and lists it.

### R43: Configurable autosave and crash recovery (M)

**Status: DONE.** Built as designed below. The interval is a checkbox plus 1-60 minutes (off = unchecked) and the kept count 1-20 (default 5, rotated as `<name>-autosave.mnle`, `.1.mnle`, ...). The session marker is written when the set of open projects changes (shown project first, then the parents of a nested timeline, each with its autosave file) and removed by `on_exit`. A marker with nothing newer than its projects is dropped at start; the rest open the Recover Projects dialog, where Open loads the autosave as an unsaved copy (the original is never written), Discard deletes the autosave copies, and Keep for later leaves everything for the next start. Harness: `MILAN_DIALOG=recover` (made-up sessions), `MILAN_APPDATA`, `MILAN_HOLD`, `MILAN_AUTOSAVE_NOW` (`demo.rs`). Tests: `autosave::tests::{stale_marker_is_detected, rotation_keeps_n_copies, recovered_project_opens_unsaved_with_the_original_untouched, crash_recovery_e2e (ignored; kills its own instance by pid)}`.

**Scope**
- Preferences > General: autosave interval (Off, 1-60 min, default 2) and backups kept (1-20, rotation).
- A session marker `%APPDATA%\MilanPro\sessions\<pid>.json` (project path, autosave path, start time), removed on clean exit.
- At start, markers whose pid is not running (`OpenProcess` check) open a "Recover projects" dialog listing autosaves newer than their projects, with Open (as an unsaved copy) / Discard / Keep for later.
- From R6b: while a nested timeline is open, the parents waiting on the stack (and their unsaved edits) must be autosaved too, and recovered with it.

**Files:** `autosave.rs`, `app.rs`, `main.rs`, `settings.rs`, `ui/dialogs/recover.rs` (new), `ui/dialogs/prefs.rs`.

**Acceptance tests:** a stale marker is detected (fake pid); rotation keeps N; a recovered project opens unsaved with the original untouched; ignored e2e: kill the app (`taskkill /F`), restart, and the dialog lists the project.

### R44: Logging, crash reports, diagnostic report (M-L)

**Status: DONE.** Built as designed below, with these differences: the module is `src/logger.rs` (a module named `log` would shadow the `log` crate inside the tree); the level and the privacy option live on Preferences › Update (Milan Pro's own page, shared with R46) rather than General; crash files go to `%LOCALAPPDATA%\MilanPro\crashes\`. The exception filter writes the minidump with `MiniDumpWithIndirectlyReferencedMemory | MiniDumpWithThreadInfo`; release builds are stripped, so a panic's backtrace is addresses only (the minidump carries the context a debugger needs). The plug-in host's one stderr print now goes to its `MILAN_HOST_LOG` file (its stderr is a null handle). Harness: `MILAN_DIALOG=crash-offer`, `MILAN_DIALOG=prefs-update`; the harness logs to stderr only and writes no crash files unless `MILAN_APPDATA` is set. Tests: `logger::tests::{log_rotation_keeps_n_files_and_the_ring_the_last_lines, panic_hook_writes_a_crash_file (a child process panics), no_eprintln_left_in_the_tree}`, `diagnostics::tests::{diagnostic_report_has_every_section, gpu_list_names_an_adapter}`, `dialogs::diagnostics::tests::issue_url_carries_the_version_and_nothing_else`.

**Scope**
- Add `log = "0.4"` (already in the lock; MIT/Apache).
- A small own file logger: `%LOCALAPPDATA%\MilanPro\logs\milan-<date>.log`, 5 files × 2 MB rotation, with level set in Preferences.
- Replace every `eprintln!`. Log FFmpeg stderr tails, updater, render and plug-in events.
- A panic hook writes `crash-<time>.txt` (version, backtrace, the last 500 log lines, project summary without paths when the "privacy" option is on).
- `SetUnhandledExceptionFilter` → `MiniDumpWriteDump` (windows-sys features `Win32_System_Diagnostics_Debug`, `Win32_System_Kernel`) to `crash-<time>.dmp`.
- On the next start, offer "Open crash report / Report on GitHub", which opens the bug template URL with the version prefilled. Nothing is uploaded automatically.
- Help > Create Diagnostic Report: a folder with system info (Windows build, CPU, RAM, GPU and driver via DXGI `Win32_Graphics_Dxgi`), `ffmpeg -version`, the capability table (R37/R22), prefs, the last logs and crash files.

**Files:** `log.rs` (new), `main.rs`, `app.rs`, every `eprintln!` site, `ui/menus.rs`, `ui/dialogs/diagnostics.rs` (new), `Cargo.toml`.

**Acceptance tests:** log rotation; the panic hook writes a file (test via `std::panic::catch_unwind` in a child process); the diagnostic report contains each section; no `eprintln!` left (grep test).

### R45a: Bins, tags, smart bins, metadata search, large libraries (M-L)

**Status: DONE.** `src/library.rs` holds the bins (`Project::bins`, `#[serde(default)]`: old projects load unchanged, no format bump), the tag helpers and the query language; `ui/media_panel.rs` draws them. Bins nest and are moved by dragging one bin onto another (or onto Custom Bins); media are dragged from the item views onto a bin. Smart bins are saved queries, shown with a magnifier, edited with Edit Search. Search Media Bins... (item, background and tree menus) turns criteria rows (field, operator, value) into the query text; Search puts it in the search field, Save as Smart Bin keeps it. Tags are edited in a small Media Tags dialog (VEGAS edits them in the tile strip; Milan Pro's tiles are drawn, not widgets). Removing media takes it out of every bin. The bin operations are functions in `library.rs` rather than `edit.rs` (they touch `Project::bins` and tags only). Harness: `MILAN_DIALOG=media-bins`, `media-tags`, `media-smart`, `media-search`. Tests: `library::tests::{the_query_parser_reads_fields_ops_quotes_and_negation, queries_evaluate_over_every_field, bin_moves_are_undoable, tags_are_set_and_counted, old_projects_load_without_bins_or_tags}`, `media_panel::tests::{search_rows_round_trip_through_the_query, large_library_frame_time_stays_under_4ms}` (10,000 items: each view's frame under 4 ms in release; search, smart-bin and tag evaluation over 5,000 and 10,000 items under 40 ms (about 12, 2.4 and 1.4 ms at 10,000 here); prints `BENCH` lines).

**Scope**
- Custom bins: nested folders; drag items into bins; an item in several bins.
- Tags: `MediaItem.tags`, a tag editor, and the Tagged Media node lists tags.
- Smart bins: saved queries over type, codec, resolution, fps, duration, tag, used/unused and offline.
- Search over name, path, codec, tags and resolution.
- A virtualised list and thumbnail grid (`ScrollArea::show_rows`) tested with 10,000 items.

**Files:** `project.rs` (`Project.bins: Vec<Bin>`), `ui/media_panel.rs`, `edit.rs`.

**Acceptance tests:** the query parser and evaluation; bin moves are undoable; with 10k synthetic items the media panel frame time stays under 4 ms (bench).

### R45b: Metadata inspector, duplicates, relink, offline marking, import progress (M-L)

**Status: DONE.** Built as designed below, except: the duplicate check runs for Import Media (and File › Import), not for files dropped straight onto the timeline, whose probes start while they hover; Clean Project Media is VEGAS's Remove All Unused Media (toolbar, and now the list background menu), which also takes the media out of their bins. `MediaItem::file_size` (`#[serde(default)]`) is recorded on import so the folder search can tell a moved file from another of the same name; media imported earlier is matched by name. An item is offline when `MediaItem::offline()`: never probed, an unsupported codec, or `missing` (found again on every Open and by Project Media › Find Missing Media). The status bar's import counter covers Import Media and drops; Cancel keeps what was read. Harness: `MILAN_DIALOG=missing-media` (two demo paths pointed at a folder that does not exist, then a folder search of the demo media; nothing on disk moves), `offline-events`, `file-props`. Tests: `library::tests::{the_hash_is_stable_across_copies_and_sees_the_last_byte, relink_by_folder_finds_a_moved_file_of_the_same_size}`, `timeline::events::tests::offline_events_render_with_the_marker`, `library::tests::moved_media_opens_offline_and_relinks_by_folder` (five files moved within the temp folder after a save: the project opens with each offline, one folder search relinks every item, same-named files told apart by size), `media::tests::the_import_counter_reaches_n_of_n`, `dialogs::file_props::tests::every_field_and_tag_is_listed`.

**Scope**
- File Format Properties: every ffprobe format and stream field and tag in a tree; copy as text.
- Duplicate detection on import: size plus SHA-256 of the first and last 4 MB (sha2, already a dependency) on the import worker; prompt "already in the project as X: add anyway / use existing".
- Missing Media dialog (on Open and from Project Media): list offline items, "Search folder..." recursively by name and size, relink all matches, per-item Browse, and leave offline.
- Offline events drawn hatched with an "offline" label on the timeline.
- Import progress "Importing 3 of 40" in the status bar with Cancel.
- Clean Project Media (remove unused).

**Files:** `media.rs`, `ui/media_panel.rs`, `ui/dialogs/missing_media.rs` (new), `ui/dialogs/file_props.rs` (new), `ui/timeline/events.rs`, `ui/status.rs`, `app/commands.rs`, `app/drop.rs`.

**Acceptance tests:** the hash is stable across copies and differs on a 1-byte change at the end; relink-by-folder finds a moved file of the same size; offline events render with the marker; the import counter reaches N of N.

### R46: Updater hardening (M-L)

**Status: DONE** (two maintainer actions outstanding: generate the minisign key pair and obtain a code-signing certificate, both in SECURITY.md). Built as designed below. `MINISIGN_PUBLIC_KEY` is empty in this tree, so updates are checked by SHA-256 alone and the dialog says so; with a key, the `.sha256` must carry a verifying `.minisig` and the API digest is ignored. The roll-back copy is fetched after the update's own download (a failure only adds a note to the Ready state). The failed-start offer and Help › Roll Back share one dialog; with no kept installer it explains the preference instead. The recording guard is in the update dialog (`install_guard`); a render keeps R12's wait/cancel guard. `release.yml` signs with minisign and signtool only when the secrets exist and publishes `-` tags as pre-releases. Harness: the Update page is `MILAN_DIALOG=prefs-update`. Tests: `updater::tests::{prerelease_versions_order, release_list_per_channel, bad_signature_is_rejected_when_a_key_is_configured (minisign's test vector; a release without a signature is refused before any download), install_guard_blocks_a_running_job, rollback_marker_counts_failed_starts}`.

**Scope**
- Channels: Preferences > General, "Update channel: Stable / Preview". Preview reads `/releases` (not `/latest`) and accepts `prerelease: true`.
- Authenticity: `minisign-verify` (MIT, no dependencies).
  - `release.yml` signs `MilanPro-x.y.z-Setup.exe.sha256` with a minisign key held in a repository secret, and uploads `.minisig`.
  - The public key is compiled into Milan, and the app refuses an update whose signature is missing or wrong.
  - Key rotation is documented in SECURITY.md.
- Authenticode: a `signtool` step in `release.yml`, gated on certificate secrets.
  - This needs a certificate the user must obtain: SignPath Foundation (free for OSS) or Azure Trusted Signing (paid).
  - Until then, record "Code signing: not yet (certificate pending)" in README. This is a user action.
- Rollback:
  - Before launching Setup, keep the current version's installer in `%LOCALAPPDATA%\MilanPro\updates\previous\`. On first update, download the current version's Setup from its release, verified the same way.
  - Help > "Roll back to version X" runs it with `/SILENT`.
  - If the new version fails to start twice, a marker written by Setup's relaunch and cleared by a successful first frame triggers an offer to roll back.
- Guards: no install while a render job (R12) or recording is running.
- Update settings page: channel, auto-check, skipped version (clear), "Keep previous version for rollback".

**Files:** `updater.rs`, `app/commands.rs`, `ui/dialogs/update.rs`, `ui/dialogs/prefs.rs`, `settings.rs`, `.github/workflows/release.yml`, `installer/milan-pro.iss`, `Cargo.toml`, `SECURITY.md` (R54).

**Acceptance tests:** a release-list parse with the prerelease filter per channel; a bad signature is rejected (fixture signed with a test key); the guard blocks during a fake running job; the rollback marker logic is unit-tested.

---

### R47: Command palette (S-M)

**Scope**
- A searchable command palette over every `Command` (name from `keymap::TABLE` plus menu labels with `&` stripped), plug-ins, layouts and recent projects.
- Fuzzy subsequence match with ranking.
- It shows shortcuts and greys unavailable commands with their R0 reason.
- Default key: pick an unbound chord, checked against `TABLE` by a test (for example Ctrl+Shift+P); it can be rebound through R15.

**Files:** `ui/palette.rs` (new), `ui/keymap.rs`, `app/commands.rs`.

**Acceptance tests:** ranking unit tests ("tm" → Track Motion first); the default key does not collide with any `TABLE` key.

### R48: High-DPI, high-refresh and animation policy (M)

**Status: DONE.** As designed below, with these choices:
- Crisp at 125-200 %: the look's chrome is drawn in whole physical pixels (`theme::one()`: 1 px at 100 and 125 %, 2 px from 150 %; `theme::snap`, `widgets::px` on physical pixel edges), icons on the physical pixel grid with whole-pixel strokes and outlines, and egui's anti-aliasing feather is off at fractional scales (a 1-point line was 1.5 px of half-transparent smear); text keeps its own anti-aliasing. Brand bitmaps (splash, About, Start) are drawn at a whole multiple of their pixels (1× up to 175 %, 2× at 200 %, nearest-neighbour), never stretched. No 2× bitmap art is needed: every icon is vector.
- View › Interface Scale (Auto, 100, 125, 150, 175, 200 %) and the same choice in Preferences › Display; Auto follows Windows as VEGAS does.
- The harness: `MILAN_SCALE` (1-3) and `MILAN_SHOT_SIZE` (physical window size, e.g. 3840x2160 for 200 % on 4K); each shot prints `overflow:` lines for windows off the screen and dialog text cut off across (`demo::overflow`, the egui layout check), and `MILAN_IDLE_CHECK=1` prints the frames painted in 2 idle seconds. Dialogs never grow past the window: their page scrolls (`dialogs::fit`, `page`).
- 144 Hz: the idle repaint loop found by the check (the title sent every frame) is gone; a paused, idle window paints nothing.
- Animation policy: `theme::anim(ms)`; Windows' client-area animation setting off makes every transition instant (egui's animation and scroll times, the chooser's hover preview).
- Tests: `animations_off_gives_zero_duration_transitions`, `chrome_pixels_are_whole_physical_pixels`, and the ignored `harness_fits_at_every_scale` (main window, FX window, Render As, Preferences and Pan/Crop at 1, 1.25, 1.5 and 2) and `paused_preview_paints_no_extra_frames`.

**Scope**
- The screenshot harness renders key windows (main, FX window, render dialog, preferences, Pan/Crop) at zoom 1.0, 1.25, 1.5 and 2.0, and checks that text bounding boxes stay inside their containers (an egui layout check, not pixels).
- View > Interface Scale (Auto, 100-200%) preference.
- The preview present path is checked at 144 Hz (no extra repaints while paused).
- Animation policy: one `theme::anim(ms)` helper; respect the Windows `SPI_GETCLIENTAREAANIMATION` setting (windows-sys `Win32_UI_WindowsAndMessaging`, already enabled).

**Files:** `demo.rs`, `ui/theme.rs`, `settings.rs`, `main.rs`, `ui/choosers.rs`.

**Acceptance tests:** harness runs at the 4 scales with no overflow; animations off gives zero-duration transitions.

### R49: Remaining Preferences pages and preference export (M)

**Status: DONE.** Every Preferences page is live or says why a control is not:
- General: "Automatically open last project on startup" (off, as VEGAS; the Start dialog is skipped when it opens one; `Prefs::startup_project`), "Show logo splash screen on startup", the temporary files folder (TMP and TEMP for Milan Pro and its FFmpeg children, set at the next start before any thread; the free space of its drive shown). The option list is as long as its options, so the page fits at 150 %.
- Audio (VEGAS's Audio tab): Normalize peak level (dB) (`edit::set_normalize_peak`, VEGAS's -0.1), the preferred audio editor and the loudness log option; Quick fade length (10 ms), Dither, Import audio at project sample rate and the metronome sounds are greyed with the reason.
- Preview Device: Identify Displays shows each display's number for 3 s (`monitors::identify`, borderless windows that never take the focus); deinterlace and studio-to-computer RGB are greyed with the reason (frames are progressive and computer RGB as they are decoded).
- Display: Envelope Colors (Volume, Pan, Mute, Composite Level) and Snap Colors (VEGAS's 12 types) with colour buttons, Icon color strength (75 % = the icons as drawn), applied through `theme::set_user_colors`.
- File I/O and Deprecated Features (VEGAS 23): VEGAS's legacy readers, QuickTime and DirectX switches, greyed with the reason. MIDI, CD Settings, Sync and External Control & Automation: not planned (R59). Project Properties › Advanced: Master display (R40b), 360 output (not planned, R59).
- Project Properties › "Start all new projects with these settings" (`Prefs.new_project`): File › New and the first Untitled project take them; unchecking goes back to the built-in defaults.
- Options › Export Preferences... / Import Preferences... (VEGAS 23's .vegprefs; Milan Pro's `.milanprefs`, JSON with a marker): a file the user picks, asking before replacing one ("... already exists. Replace it?") and before replacing the preferences on import; the import takes effect at once (keys, toolbars, layouts, colours, track defaults; `app/prefs_io.rs`).
- Edit Visible Button Set... (track header menu, VEGAS 19): per kind, which of Compositing Mode, Track FX, Mute, Solo (video) or Arm for Record, Track FX, Mute, Solo (audio) the headers show; hidden ones are rows of the header's More menu.
- `MILAN_APPDATA` now also moves prefs.json, so tests and the harness never touch the user's preferences.
- Tests: `prefs_export_import_round_trip_restores_every_field` (every field set away from its default, checked), `open_last_project_flag`, `button_set_hides_per_kind`, `display_colours_and_icon_strength`.

**Scope**
- Make the Preview Device page live (from R14) and the Audio page (from R9/R8a).
- Export/Import Preferences (VP23).
- "Automatically open last project on startup", "Show logo splash", temporary-files folder and "Start all new projects with these settings": make each live, or label it.
- MIDI, CD Settings, Sync and External Control pages: label them "not planned" with the R59 reference.
- From R15 (re-assigned by R18b): the track header's Edit Visible Button Set (VEGAS 19 on: which buttons a video or audio track header shows, the rest staying in its More menu), kept in the preferences.

**Files:** `ui/dialogs/prefs.rs`, `settings.rs`, `app.rs`, `ui/timeline/tracks.rs`, `ui/timeline/ctx.rs`.

**Acceptance tests:** a prefs export/import round-trip; the open-last-project flag works.

---

### R50: Cargo workspace split (L)

**Scope:** convert to a workspace with these crates:
- `milan-core` (project, edit, history, keyframes; no egui)
- `milan-engine` (plan, composite, decode, mix, render, built-in plug-ins, caps, hw)
- `milan-interchange`
- `milan-updater`
- `milan-plugin-host` (bin)
- `milan-pro` (app and UI)

Each crate gets its own tests. `egui` types in the engine (`ColorImage`, `Color32`) are replaced by `epaint` re-exports or a plain `Rgba8` type, so the engine builds without the UI.

**Acceptance tests:** `cargo test --workspace` passes the same number of tests; `cargo build -p milan-engine` works without eframe; the release workflow still produces the installer.

### R51: CI quality gates (M)

**Scope**
- Add `rustfmt.toml` matching the existing style (`max_width = 150` or measured) and run `cargo fmt` once in its own commit.
- `cargo clippy --workspace -- -D warnings` (fix or allow with reasons).
- `cargo deny check licenses advisories bans` with `deny.toml`.
- A Windows CI job that runs `installer/fetch-ffmpeg.ps1` and then `cargo test -- --include-ignored` (excluding the hardware and live-network tests through a name filter).
- An MSRV 1.85 job.
- A weekly scheduled run of `tools/bench.ps1`, uploading `PERFORMANCE.md` as an artifact.

**Files:** `.github/workflows/ci.yml`, `rustfmt.toml`, `deny.toml`, `clippy.toml` (new), tests named for filtering.

**Acceptance:** CI is green with all gates; a deliberately unformatted file fails CI.

### R52: REFERENCE_ANALYSIS.md (M)

**Status: DONE.** [`docs/REFERENCE_ANALYSIS.md`](REFERENCE_ANALYSIS.md): the installation (program folder and shared plug-ins), what the interface was measured from, project files, media formats per FileIO plug-in, render formats, plug-in standards, SDKs and integration points, interoperability (with R28/R29 now built), settings pages, the open behaviours, and section 11, which maps VEGAS's model (tracks, events, takes, envelopes, busses, FX chains, Pan/Crop, Track Motion, the project and render pipeline, windows and docking) onto Milan Pro's data model and engine, with the deliberate differences. Section 12 lists the corrections; `FEATURE_PLAN.md:1813` (HitFilm) is corrected. Written from files and documentation only: VEGAS was not launched, and nothing of it is reproduced.

**Scope:** the reference document the brief asked for first. It covers:
- the VEGAS Pro 15.0 build 177 install (section 4.1 here, expanded);
- the relationship to the VEGAS 16 and VEGAS 23 references Milan Pro's layout follows;
- project formats (`.veg`, `.vf`, `.veg.bak`, `.sfk` peaks, `.sfvp0` proxies);
- media formats per FileIO plug-in;
- render formats;
- plug-in standards (OFX 1.x with VEGAS extensions, VST2/VST3 via `sfvstwrap`, DirectX/DMO audio, legacy video plug-ins);
- SDKs and integration points (the .NET Script API, `ScriptPortal.Vegas.dll` classes used by the export script, the FileIO SDK, the external-control drivers);
- interoperability (AAF via `AAFCOAPI.dll`, EDL script, XML via scripts);
- settings pages;
- the list of behaviours still to confirm (`FEATURE_PLAN.md:1817`).

It also corrects `FEATURE_PLAN.md:1813` (HitFilm is third-party OFX).

**Files:** `docs/REFERENCE_ANALYSIS.md` (new), `docs/FEATURE_PLAN.md` (correction).

**Acceptance:** every section 1 row of this file can be marked DONE with a link to it.

### R53: ARCHITECTURE.md, developer guide, README, roadmap (M)

**Status: DONE.** [`docs/ARCHITECTURE.md`](ARCHITECTURE.md) (with the six decision records), [`docs/DEVELOPING.md`](DEVELOPING.md), the README refresh (screenshots from the harness in `docs/images/`, the 0.8.0 feature table, system requirements, the documentation index and the roadmap), and `FEATURE_PLAN.md`'s header. Help > Contents and Index opens the docs folder on GitHub and Help > Download User Manual the README (`dialogs::DOCS_URL`, `MANUAL_URL`); What's This?, Show Me How and Interactive Tutorials stay greyed with a final reason (hover text and the docs stand in for them) instead of a batch. Link check: `tests/doc_links.rs` resolves every relative link (inline, reference and HTML) of every Markdown file and runs with `cargo test`, so CI checks it. Not done: the "name check" note of row 2.2.

**Scope**
- `docs/ARCHITECTURE.md`: crates and modules, threads, data flow (plan → compose/mix → preview/render), the plug-in host, the updater, file formats, and decision records:
  - FFmpeg CLI versus libav bindings versus GStreamer;
  - egui versus Qt, Slint and iced;
  - glow versus wgpu;
  - rodio versus cpal;
  - JSON project format;
  - out-of-process plug-ins.
- `docs/DEVELOPING.md`: toolchain, FFmpeg fetch, tests including ignored ones, screenshot harness environment variables (`MILAN_SCREENSHOT`, `MILAN_DEMO`, `MILAN_UPDATE_API`, `MILAN_UPDATE_AUTO`), benches, release process and versioning.
- Fix the stale header of `FEATURE_PLAN.md`.
- README: feature status table, screenshots (from the harness), system requirements, links to all docs, and a roadmap linking to this file.
- Help > Contents opens the docs on GitHub (enables the `Help` command).

**Acceptance:** links resolve (a markdown link check in CI); Help > Contents works.

### R54: Community files and templates (S)

**Status: DONE.** [`CONTRIBUTING.md`](../CONTRIBUTING.md), [`CODE_OF_CONDUCT.md`](../CODE_OF_CONDUCT.md) (Contributor Covenant 2.1, CC BY 4.0, attribution kept; reports go to the maintainers through the repository's private reporting form), the issue forms (`bug_report`, `feature_request`, `plugin_compat` in place of `plugin_compatibility`, `format_support`, `config.yml` with blank issues off and links to security reporting and the docs instead of Discussions, which the repository does not use), the pull request template, and [`CHANGELOG.md`](../CHANGELOG.md) from the 0.2.0-0.7.0 release notes and the 0.8.0 batches. `SECURITY.md` already covered reporting and the updater keys and is unchanged. The labels the forms name (`bug`, `enhancement`, `plug-in compatibility`, `format`) are applied only once they exist on the repository. Acceptance (GitHub renders the forms) is for the user to check on the repo page.

**Scope**
- `CONTRIBUTING.md` (build, branch and commit style, batch rules from FEATURE_PLAN, `shortcut:` convention, tests required).
- `CODE_OF_CONDUCT.md` (Contributor Covenant 2.1).
- `SECURITY.md` (reporting, updater keys).
- `.github/ISSUE_TEMPLATE/`: `bug_report.yml` (version, Windows build, GPU, steps, expected/actual, diagnostic-report upload); `feature_request.yml`; `plugin_compatibility.yml` (plug-in name, vendor, format, version, result); `format_support.yml` (file details, ffprobe output); `config.yml` (blank issues off, link to discussions).
- `.github/PULL_REQUEST_TEMPLATE.md`.

**Acceptance:** GitHub renders the templates (checked by the user on the repo page).

### R55: Compatibility matrix, tested VEGAS versions, example projects (M)

**Scope**
- `docs/COMPATIBILITY.md`, the public matrix. Rows: every installed VEGAS plug-in (102 OFX, 11 DX audio, legacy video plug-ins) plus tested third-party OFX/VST3/CLAP. Columns: Native equivalent (milan id) / Standard interface (OFX, VST3, CLAP) / Adapter (VEGAS import mapping) / Unsupported (reason) / Needs investigation. Data comes from R33c, R34a, R35 and `vegas_map`. Include the VST2 and ASIO statements.
- A tested-versions table (VEGAS 15.0 b177; others when tested) and the script versions.
- `tests/vegas/`: small example `.milan.json` exports (from the user's VEGAS) with expected results, plus a test that imports each.
- Confirm the 10 open VEGAS behaviours (`FEATURE_PLAN.md:1817-1827`) with the user and record them.

**Acceptance:** each matrix row has a status from the five categories; no "supported" row without a passing test reference.

### R56: Dependency and codec licensing review (S-M)

**Status: DONE.** [`docs/LICENSES.md`](LICENSES.md) (named so, not `LICENSING.md`) and `THIRD-PARTY-NOTICES.txt` (not `THIRD_PARTY_LICENSES.txt`; with every crate's licence text, each distinct text once) are written by `tools/licenses.ps1` from `Cargo.lock` and the crates in the local Cargo registry (nothing downloaded; the hand-written notes are `tools/licenses-notes.md`). `cargo deny` is not in the toolchain, so the acceptance check is `tools/licenses.ps1 -Check`, which fails when either file is stale; `deny.toml` is R51's. Result: 474 crates, all under permissive licences; no GPL, LGPL-only, AGPL or unknown licence; `r-efi` offers LGPL-2.1 only as a third choice beside MIT and Apache-2.0. Not crates: FFmpeg 9.0.2 (GPL v3, separate processes, bundled with `FFMPEG-SOURCE.txt` as the source offer), the OpenFX header attribution (BSD-3), VST3 (`vst3` crate MIT/Apache; no Steinberg SDK code shipped; the SDK 3.8 MIT licence is still to be verified against Steinberg's own text), CLAP (MIT), the Contributor Covenant (CC BY 4.0), fonts (none shipped; egui's fallback fonts under OFL-1.1, UFL-1.0 and MIT), Inno Setup. `installer/milan-pro.iss` installs `THIRD-PARTY-NOTICES.txt` (checked with ISCC).

**Scope**
- `docs/LICENSING.md`: the `cargo deny` licence list (all crates; MIT/Apache/BSD/ISC/Zlib/Unicode/MPL), new crates from the batches (minisign-verify, vst3, clap-sys, quick-xml).
- FFmpeg: GPL v3 build, shipped as separate executables (aggregation), with the source offer `FFMPEG-SOURCE.txt`. libx264/libx265 GPL; libfdk-aac absent.
- Codec patent notes: H.264, HEVC, AAC, AC-3/E-AC-3, MPEG-2. The distributor and user responsibility is stated plainly, not legal advice.
- The VST3 SDK licence (MIT from 3.8; verify), OpenFX (BSD-3), the ASIO SDK (not redistributable), BRAW/RED SDKs (proprietary), and VEGAS's bundled plug-ins (licensed with VEGAS).
- The installer bundles `THIRD_PARTY_LICENSES.txt` generated from cargo metadata.

**Acceptance:** `cargo deny check licenses` passes; the installer contains the licence file.

---

### R57-R60: VEGAS long tail (greyed in the UI, outside the brief)

These are VEGAS menu items, not requirements of the brief. The brief only requires that they be labelled (R0). They are listed so "every single thing" has a decision. **The user should choose: build, or keep labelled "Not planned".**

| Batch | Scope | Size | Feasibility |
|---|---|---|---|
| R57 | Media ops: Media FX on files, Rotate 90°, Broadcast Wave import (bext time reference → placement), Add at Project Frame Rate, Match Project Video Settings, Select Timeline Events, Open Parent Media in Trimmer, Rename, Refresh, Columns/Auto-size, Scene Detection (FFmpeg `scdet`) to events/subclips, Tempo/Beat Detection (onset envelope autocorrelation), Explorer preview buttons, Trimmer "Show Video in Preview" | L (split 2 × M) | Feasible |
| R58 | Event long tail: Synchronize by Moving/Slipping, Sync Links, Add Missing Stream, Apply Non-Real-Time Event FX (render the event with FX to a new take), Select Events to End, Split Trim / Shuffle / Zoom tools, Expanded Edit polish, Slip All Takes, Fade Colors dialogs, track Text/Background Color | L (split 2 × M) | Feasible |
| R59 | Hardware and services: CD/DVD burn (Windows IMAPI2 COM: feasible for audio CD and data); Blu-ray authoring (NOT FEASIBLE: AACS/BD licensing); Extract Audio from CD (feasible via IOCTL_CDROM_RAW_READ, M); DV/HDV capture (obsolete FireWire: not planned); Import from Device (WPD/MTP, M); Get Media from the Web (not planned); MIDI timecode, clock and trigger (`midir` crate, MIT, M); File > Real-Time Render (renders through external hardware FX in real time; needs hardware FX hosting, not planned); External Control surfaces (Mackie/FaderPort over MIDI, L); Text to Speech (SAPI COM, S-M); Speech to Text (whisper.cpp MIT plus model download, L, opt-in); Motion Tracking (KLT tracker, L-XL, research); Dashboard (VP23 system stats, S); Stereoscopic 3D (not planned) | per item | Mixed, as noted |
| R60 | Scripting: VEGAS .NET script compatibility is NOT FEASIBLE (it needs the ScriptPortal API surface on .NET). Honest alternative: Milan scripting with Rhai 1.x (MIT/Apache) exposing the edit API; Tools > Scripting > Run Script, Rescan Script Menu; port the 17 bundled VEGAS script behaviours that matter (batch render → R12; EDL → R28; subtitles → R39) | L | Alternative feasible |

---

## 6. Not feasible items and their honest deliverables

| Item | Reason | Deliverable (batch) |
|---|---|---|
| Native `.veg` read/write (8.1) | Undocumented, versioned binary format with no public spec or parser; writing would produce files VEGAS might reject (the brief forbids fake `.veg`) | Script import (R25, done), script export back into VEGAS (R26, done), research note (R27, done: `docs/research/VEG_FORMAT.md`), media scan (done) |
| AAF (9.6) | No Rust AAF library; the AAF SDK is a large C++/COM structured-storage codebase | OTIO plus the external `otio-aaf-adapter` route documented in INTERCHANGE.md (R29) |
| VST2 (10.11) | Steinberg no longer licenses the VST2 SDK | VST3 (R34) and CLAP (R35) hosting; stated in COMPATIBILITY.md (R55) |
| RAW video (7.19) | R3D, BRAW and ARRIRAW decoders are proprietary SDKs, not redistributable with an MIT project; FFmpeg has none | Documented in FORMAT_SUPPORT.md; optional user-installed BRAW SDK study (R41) |
| CEA-608/708 captions (7.55) | The bundled FFmpeg has no 608/708 encoder; its decoder reads them only from a video stream's A53 side data, which the decode does not keep | Subtitles as Titles & Text events with SRT / WebVTT export, sidecars and soft streams (R39, DONE) |
| ACES/OpenColorIO pipeline (part of 7.57/7.58) | OCIO is C++ with no Rust port | Done in R40b: the ACES color spaces (AP0/AP1, ACEScc, ACEScct) exactly, the ACES SDR output transforms as a fitted curve, HDR10/HLG views, and a .cube LUT view for an OCIO transform baked into one |
| ASIO output (part of 12.19) | The ASIO SDK licence forbids redistribution | WASAPI exclusive low-latency path (R9, DONE: `wasapi.rs`); documented in `docs/COMPATIBILITY.md` |
| Signed artifacts without a certificate (16.9) | Authenticode needs a CA-issued certificate that only the user can obtain | R46: minisign verification is built and waits on the maintainer's key pair (SECURITY.md); the Authenticode step is wired and waits on the certificate secrets |
| VEGAS-licensed plug-ins (OFX first-party, DX audio) in Milan | Licence ties them to VEGAS; host checks are likely | Tested and reported per plug-in (R33c, R35), never claimed without a passing test (R55) |

---

## 7. Release gate

Do not tag a release until all of the following hold:
1. Every row in section 2 is DONE, or NOT FEASIBLE with its deliverable merged.
2. R0's test proves no greyed control lacks a reason, and R57-R60 items each have a recorded build or "not planned" decision from the user.
3. CI (R51) is green with fmt, clippy, deny, unit tests and the FFmpeg-backed integration tests. PERFORMANCE.md (R19) shows the 4×1080p gate (< 1% drops) and scrub p95 < 150 ms on proxies.
4. FORMAT_SUPPORT.md, COMPATIBILITY.md, INTERCHANGE.md, LICENSING.md, REFERENCE_ANALYSIS.md and ARCHITECTURE.md exist and are linked from README.
5. The updater verifies a minisign signature (R46). Authenticode is either in place or explicitly waived by the user.
6. A manual pass by the user in VEGAS 15 for R25/R26/R33c/R55 is recorded.

Effort: 71 sessions for R0-R56 (R24 and R36 are unused; R3, R4, R6, R8, R10, R16, R17, R18, R21, R33, R34, R40 and R45 are split), plus R57-R60 if the user chooses to build the VEGAS long tail (about 10 more).

---

## 8. Extras requested by the user

Features VEGAS does not have, built because the user asked for them. They follow the same looks and rules as the rest.

- **Overlays window (overlay packs): DONE.** View › Window › Overlays, docked after Media Generators in the built-in layouts (saved last layouts get it once; user layout slots are left alone). Library folder (Preferences › General, default `Documents\Milan Pro\Overlay Packs`) with one subfolder per pack, index in `%APPDATA%\MilanPro\overlays.json`. Import Pack from Folder / from .zip (Windows `tar.exe`; entries that would leave the pack folder refuse the whole zip), Add Existing Folder as Pack (linked), Rename, Remove (linked: forgotten; copied: Recycle Bin after a confirm that counts the project's media in it), Open Pack Folder, Refresh, Locate... for a missing folder, Create Sample Pack (FFmpeg-made film grain, light leak and frame border). Tree (All Overlays, Favorites, Recent, packs), thumbnails with hover scrubbing, Details view, search over names and tags, sort, favourites, tags. Drag to the timeline, double-click (new video track above the current one at the cursor) or Add to Project Media; a new track gets the overlay's blend (Normal, Screen, Add, Multiply, Overlay, or the Chroma Keyer's green/blue preset) in the same undo step. Code: `src/overlays.rs`, `src/ui/overlays.rs`.
  - Left: image sequences are listed as single stills (Milan Pro has no sequence reader on import); favourites, tags and item blends stay on the old paths after Locate...; the Recycle Bin move is not exercised by tests (they never touch the real Recycle Bin).
- **Music-video FX and transitions of 2001-2009 (MVFX): DONE.** Research in `docs/research/MUSIC_VIDEO_2000S.md` (emo / pop-punk and hip-hop videos, their looks, optics, cutting and transitions, and how editors of the time made them). 32 video FX and 19 transitions in `src/engine/mvfx/` (registry rows, CPU kernels as the reference, recipes of kernel / blur / draw steps) with GPU twins (`src/engine/gpu/mvfx.rs`, `mvfx_*.wgsl`, `GpuPort::Mvfx`) written in the same f32 operations (portable sine-free maths in `pm.rs`), every parameter keyframable, a Default preset and era presets. VEGAS names and parameters for its built-ins (Glow, Film Effects, Film Grain, Spherize, Pinch/Punch, Radial Blur, Linear Blur, Soft Contrast, Starburst, Light Rays, TV Simulator, Threshold, Newsprint, Add Noise, Pixelate, Gradient Map; Flash, Cross Effect, Page Peel, Page Roll, Barn Door, Venetian Blinds, Spiral, Squeeze, Star Wipe, Portals), which now import from VEGAS projects as Milan built-ins (`vegas_map.rs`). Temporal FX are pure functions of the frame: the plan adds the time to their values (`mvfx::frame_vals`), and Echo, Stutter and Datamosh read earlier source frames through the frame's `Sources` (`mvfx::event_fx`, the GPU's `mv_event`). FX Packages: 15 built in, the user's saved from the FX window (Save As...) to `%APPDATA%\MilanPro\fx_packages.json`, listed under the Video FX window's Filter Packages chip, dropped or double-clicked like an FX. Tests: registry, striped-equals-single, GPU parity at three parameter sets and at later frames, determinism (same frame twice, seek against play, CPU and GPU), source readers, package round trip and loading, VEGAS rows; review contact sheets through the render path (`--ignored mvfx_contact_sheets`).
  - Left: Echo, Stutter and Datamosh read the event's own earlier source frames, so as track, adjustment or Video Output FX (no source) they pass the picture on; earlier frames are taken on the source-frame grid (a velocity-ramped event's echoes follow its source, not the timeline); the repeat (P-frame duplication) datamosh and moshing across an edit between two events are not built (the Glitch transition stands in); transitions keep Milan's unkeyed transition settings (their progress is the keyframe); Lens Flare, Kaleidoscope, Defocus, Wave, Deform, Swirl and Gradient Wipe are out of scope (no source ties them to these scenes).
