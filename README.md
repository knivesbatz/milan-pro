<p align="center"><img src="assets/brand/readme-banner-960x260.png" width="720" alt="Milan Pro: Nonlinear Editing System"></p>

# Milan Pro

An open-source nonlinear video editor written in Rust, with a workflow modeled on Sony VEGAS Pro.

> Early development (v0.7). Editing, preview, effects, transitions, titles, audio FX, OFX video plug-ins, VST3 and CLAP audio plug-ins, recording and rendering work.

## Features

- Three interface looks: Classic 2006, and two modelled on VEGAS Pro 16 and VEGAS Pro 23 (dark and white), with VEGAS menus, shortcuts and window layouts
- Media import for any format FFmpeg can read; drag from Project Media, the Explorer tab or Windows File Explorer onto the timeline
- Multitrack editing: move, trim, split, snapping, multi-select, grouping, automatic crossfades, ripple editing, slip/slide/roll/stretch, playback rate and reverse, takes, Event Properties
- Volume, pan, composite level and mute envelopes; Event Pan/Crop and Track Motion with keyframes
- Real-time multi-layer preview with persistent decoders, frame caches and video proxies; Build Dynamic RAM Preview (Shift+B), Selectively Prerender Video (Shift+M) with Clean Up, and an optional disk cache for decoded HEVC/AV1/4K frames
- Video FX, transitions, media generators, Titles & Text and Color Grading (lift/gamma/gain, curves, .cube LUTs), with presets and keyframes
- Streaming audio mixer with waveforms and meters; audio FX (EQ, compressor, gate, reverb, delay) on events, tracks and the master; recording into armed tracks
- Render As through the same engine as the preview (FFmpeg encoders); hardware encoders (NVENC, Quick Sync, AMF, Media Foundation) and hardware decoding (d3d11va, cuda, qsv) are tested at startup and used when they pass and measure faster
- Autosave, safe saves, undo/redo, and an auto-updater
- Project files are `.mnle` (JSON)

## Looks

Milan Pro starts in the **Classic 2006** look: the brushed-metal, 3D-bevelled chrome of the 2005–2007 Windows video editors (Tahoma 8 pt, raised toolbars, sunken white lists, Windows 2000 dialogs and controls, a black LCD time display) on the VEGAS 23 window layout, with every menu, shortcut and docking area working as before. **Options > Preferences > Display > Interface Look** switches to the VEGAS 16 or VEGAS 23 (dark or white) looks, which are drawn as those applications are; the choice is kept in the preferences, and window layouts are saved per look. `docs/CLASSIC_2006_SPEC.md` and `docs/VEGAS_UI_SPEC.md` describe each look pixel by pixel.

## Overlay packs

**View > Window > Overlays** keeps your overlay video packs (light leaks, film grain, dust and scratches, bokeh, flares, frames, keyed elements) in a library folder, by default `Documents\Milan Pro\Overlay Packs` (change it in **Options > Preferences > General**). Each pack is a subfolder: **Import Pack...** copies a folder or extracts a `.zip` into the library, or links an existing folder where it is. Browse by pack, favourites, recent or search (names and tags), with thumbnails that scrub as you hover. Drag an overlay onto the timeline, or double-click it to add it at the cursor on a new track; the new track gets the overlay's blend (Normal, Screen, Add, Multiply, Overlay, or a green/blue screen key), set per pack in **Pack Properties** or per overlay from its right-click menu. Removing a copied pack moves its folder to the Recycle Bin; nothing is deleted for good. Milan Pro ships no third-party overlays; **Create Sample Pack** makes a few with FFmpeg.

## Install (Windows)

Download `MilanPro-<version>-Setup.exe` from [Releases](../../releases) and run it. It installs per-user by default (no admin needed), adds a Start Menu shortcut, and can be removed from Settings > Apps.

The installer bundles [FFmpeg](https://ffmpeg.org/) 9.0.2 (GPL v3, build by Gyan Doshi), installed next to `MilanPro.exe`. Source links are in `FFMPEG-SOURCE.txt` in the install folder. When building from source, run `installer/fetch-ffmpeg.ps1` or have FFmpeg on `PATH`.

## Updates

A few seconds after startup Milan Pro asks GitHub for the latest release (it sends only a `MilanPro-Updater/<version>` user agent). When a newer version exists it shows the release notes with **Download and install**, **Remind me later** and **Skip this version**. The installer is downloaded over HTTPS, checked against its SHA-256 (`MilanPro-<version>-Setup.exe.sha256`, or the release's asset digest), and run silently; Milan Pro asks to save your project, closes, updates and starts again. Check by hand with **Help > Check for Updates...**, or turn the startup check off in **Options > Preferences > Update**.

The Update page also has the channel (**Stable**, or **Preview** for pre-releases), the skipped version, and **Keep the previous version's installer**: with it on, the current version's own installer is fetched from its release (verified the same way) before an update, and **Help > Roll Back to Previous Version** reinstalls it. A version that fails to start twice after an update offers the roll back by itself. Updates are never installed while a render or a recording is running.

Verification: the download's SHA-256 must match the release. Once the maintainer's minisign key pair exists, the checksum file is signed (`.minisig`) and the public key is compiled into Milan Pro, which then refuses unsigned updates; until then the update dialog says "SHA-256 only". Code signing: not yet (certificate pending). See [SECURITY.md](SECURITY.md).

## Import from VEGAS Pro

Milan Pro cannot read `.veg` files directly (`docs/research/VEG_FORMAT.md`). In VEGAS, choose **Tools > Scripting > Run Script...** and pick `Export to Milan Pro.cs` from the `vegas` folder of the Milan Pro install (`tools/vegas` in the source; use the `(VEGAS 13)` copy in VEGAS Pro 13 and earlier). It writes `<project>.milan.json`; open that in Milan Pro with **File > Import > VEGAS Project...**. Tracks, events (Pan/Crop, subclips, reversed events, velocity), fades, takes, groups, envelopes, Track Motion, parent/child compositing, transitions, generated media, event/track/media/output FX with their keyframes, audio FX by name, markers, regions and the project settings come across; VEGAS plug-ins that have a Milan Pro counterpart become it with their settings converted, the rest are kept as disabled OFX placeholders (they play once the plug-in is installed) and Milan Pro lists them. Picking a `.veg` there instead only adds the media it uses to Project Media.

The other way, **File > Export > VEGAS Project (via script)...** writes the same kind of file, and `Import from Milan Pro.cs` (same folder) rebuilds the project inside VEGAS through its scripting API; Milan Pro never writes `.veg` files. **File > Export > Milan Pro Project Archive...** copies a project with its media into one folder (optionally trimmed to the used ranges), and **File > Import > Media from Project...** pulls another project's media in. `docs/INTERCHANGE.md` has the details.

## OFX plug-ins

Milan Pro runs third-party OFX video effects, transitions and generators (Filter, General, Transition and Generator contexts; 8-bit, 16-bit, half or 32-bit float) in **milan-plugin-host.exe**, a separate process installed next to `MilanPro.exe`: a plug-in that crashes or hangs is stopped and bypassed with a message, and Milan Pro keeps running. Its settings stay in the project, so nothing is lost.

- **Where Milan Pro looks:** `C:\Program Files\Common Files\OFX\Plugins` (the standard folder, where installers put `*.ofx.bundle` folders), every folder in the `OFX_PLUGIN_PATH` environment variable (separated by `;`), and folders you add in **View > Window > Plug-In Manager > Folders > Add Folder...**.
- **Using them:** found effects appear in the Video FX window under their own grouping (for example "spektrafilm OFX"), transitions in the Transitions window and generators in Media Generators, and apply like the built-in ones, with their parameters and keyframes in the FX window. The FX window lays out an OFX plug-in's controls as the plug-in defines them (pages, groups, hidden and greyed parameters, labels and choice options it changes at run time) and runs its push buttons (Save Preset, Export LUT and the like) in the plug-in host; what the plug-in changes itself is written to the project. Projects store them as `ofx:<identifier>@<major version>`; a project that uses a plug-in you do not have keeps its settings and shows the FX as missing, with a red "!" on the event's fx button.
- **Plug-In Manager** (View > Window, Ctrl+Alt+1): every plug-in with its status (OK, Failed with the reason, Disabled), a check box to turn one off, and **Rescan** (reads every plug-in again and tries failed ones again). Results are cached in `%LOCALAPPDATA%\MilanPro\plugins.json`.
- **Limits:** Milan Pro does not provide GPU render suites (CUDA, OpenCL, Metal, OpenGL), overlays/on-screen controls, custom parameter UIs, parametric (curve) parameters, OFX 1.5 colour management or other frames than the current one (temporal access). Plug-ins that need these, or that only run in a particular host (DaVinci Resolve-only plug-ins; the VEGAS Pro OFX bundles, which need VEGAS's private suites and DLLs: see `docs/compat/ofx-vegas15.md`), fail with the reason shown. See `docs/compat/ofx-spektrafilm.md` for Spektrafilm.

The OFX C API bindings in `src/bin/milan-plugin-host/ofx.rs` were written from the OpenFX 1.4 headers, Copyright (c) 2003-2015 The Open Effects Association Ltd, used under the BSD 3-Clause licence.

## VST3 and CLAP plug-ins

Milan Pro runs third-party **VST3** and **CLAP** audio effects (stereo, 32-bit float, at the project's 48 kHz) in the same **milan-plugin-host.exe** process as OFX: a plug-in that crashes or hangs is stopped and bypassed with a message, and Milan Pro keeps running. Its settings stay in the project.

- **Where Milan Pro looks:** `C:\Program Files\Common Files\VST3` and `%LOCALAPPDATA%\Programs\Common\VST3` (`.vst3` bundles or single files), `C:\Program Files\Common Files\CLAP` and `%LOCALAPPDATA%\Programs\Common\CLAP` (`.clap` files), and folders you add in **Options > Preferences > VST Effects** (or Plug-In Manager > Folders). A VST3 bundle's `moduleinfo.json` is cross-checked against what its factory reports.
- **Using them:** found effects appear in the Audio FX menu (fx+ in the FX window) under their vendor and apply to events, tracks, busses and the master like the built-in audio FX, in preview and in renders, with their parameters, keyframes and FX automation envelopes in the FX window. A plug-in's latency is compensated in the mixer (plug-in delay compensation, sample-exact: every other path is delayed to match, and the mix runs that far ahead). **Show plug-in UI** opens the plug-in's own editor in a window of the plug-in host (resizable; it closes with Milan Pro); what you change there is written to the project, as FX automation keys while the track records automation (Write Touch/Latch) and plays, else as values. **Load/Save .vstpreset** read and write Steinberg's preset file format; program lists appear as a Program parameter. Projects store them as `vst3:<class id>` or `clap:<id>` with the parameter values and the plug-in's own state; a project that uses a plug-in you do not have keeps its settings and shows the FX as missing, with a red "!" on the event's fx button.
- **Plug-In Manager** (View > Window, Ctrl+Alt+1): VST3 and CLAP effects with their status, a check box to turn one off, and **Rescan**; results are cached in `%LOCALAPPDATA%\MilanPro\plugins.json` with the OFX plug-ins.
- **Limits:** stereo effects only (instruments, MIDI effects, mono- or surround-only plug-ins are listed as unsupported); CLAP plug-ins have no editor window yet; parameter changes are sent once per block (1024 frames); the editor's controls are not shown in Milan's generic parameter grid beyond the plug-in's own parameter list. **VST2** is not supported: Steinberg no longer licenses its SDK. **DirectX (DMO)** audio plug-ins were investigated and not pursued (`docs/compat/dx-audio.md`).

VST3 is hosted through the `vst3` crate (coupler-rs, MIT OR Apache-2.0; the VST 3 SDK itself is MIT-licensed since VST 3.8) and CLAP through `clap-sys` (MIT OR Apache-2.0). The test plug-ins in `tools/vst3-test-plugin` and `tools/clap-test-plugin` are Milan Pro's own.

## AE-format effects (experimental)

**AE-format effects (experimental).** Milan Pro can load some effect plug-ins written for the Adobe After Effects effect API (`.aex` files) that you have installed and licensed yourself. Milan Pro includes no Adobe software and no third-party plug-ins. It implements only a basic part of the interface, similar to what video editors (rather than After Effects itself) provide: simple filters with standard controls, in 8-bit. Plug-ins that need After Effects features such as compositions, cameras, lights, masks, their own drawn controls, GPU rendering, 32-bit float or SmartFX will not work, and many commercial plug-ins check for an Adobe host or are licensed only for specific hosts; Milan Pro does not bypass any such check. Each plug-in runs in a separate process, so a failing plug-in cannot crash the editor; the Plug-In Manager shows what loaded, what did not, and why. If a vendor offers an OpenFX version, use that instead. Check your plug-in's licence before using it in Milan Pro. Adobe does not support third-party hosts of its plug-in interface.

- **Off by default.** Turn it on in **View > Window > Plug-In Manager > Folders > Experimental: AE-format effects**. Milan Pro then searches the shared plug-in folders After Effects and Premiere Pro name in the registry, `C:\Program Files\Adobe\Common\Plug-ins\7.0\MediaCore` (and `...\Plugins\...`), and folders you add there (**Add AE-format Folder...**). Adobe application folders are never searched, and Adobe's own effects are listed as Blocked.
- **What runs:** effects whose PiPL says they are video effects; they appear in the Video FX window under their own category, with generic controls for sliders, angles, checkboxes, colours, points, popups, groups and layer parameters (None or this event), keyframes and presets. Effects that read other frames of their input get them. Projects store them as `aex:<match name>`, each parameter under `p<its id>`, and their flattened sequence data in the project; a project that uses an effect you do not have keeps its settings and shows it as missing.
- **Status:** every effect is **Untested** until a test of it is recorded in `docs/compat/aex.csv` (so far only Milan Pro's own test plug-in). Milan Pro reports itself to plug-ins as a third-party host (`'PrMr'`) with no serial number.
- **Limits:** 8-bit only (Premiere's float pixel-format suite is not documented publicly); no transitions (the transition suite is not documented publicly); no SmartFX, audio, custom drawn controls, options dialogs, masks, cameras or AEGP suites. The interface declarations are Milan Pro's own, written from public documentation (`docs/compat/aex-provenance.md`); they are proven only against Milan Pro's own test plug-in so far, not yet against a third-party plug-in.

Adobe, After Effects and Premiere Pro are trademarks of Adobe Inc. Milan Pro is not affiliated with, endorsed or sponsored by Adobe.

## Build from source

```bash
cargo build --release
```

Requires Rust 1.85+ (edition 2024). The binaries are written to `target/release/milan-pro.exe` and `target/release/milan-plugin-host.exe` (the OFX / VST3 / CLAP plug-in host, which must sit beside it).

## Project layout

| Path | Purpose |
|------|---------|
| `src/app.rs`, `src/app/` | Application state, commands, drops, playback |
| `src/project.rs` | Project, timeline, track, event and media data model |
| `src/edit.rs`, `src/history.rs` | Editing operations and undo |
| `src/engine/` | Render plan, decoders, compositor, audio mixer, render |
| `src/ui/` | Looks, menus, dock windows and the custom-painted timeline |
| `docs/FEATURE_PLAN.md` | Engine design and the feature batches |

## Roadmap

- CLAP editor windows; OFX GPU render
- Masks, freely dockable panels, direct `.veg` reading
- EDL / OTIO interchange

## License

MIT. See [LICENSE](LICENSE). Milan Pro isn't affiliated with or endorsed by VEGAS Creative Software or Sony.
