<p align="center"><img src="assets/brand/milan-pro-256.png" width="128" height="128" alt="Milan Pro icon"></p>

# Milan Pro

An open-source nonlinear video editor written in Rust, with a workflow modeled on Sony VEGAS Pro.

> Early development (v0.6). Editing, preview, effects, transitions, titles, audio FX, OFX video plug-ins, recording and rendering work; VST hosting is next.

## Features

- Two interface looks modelled on VEGAS Pro 16 and VEGAS Pro 23 (dark and white), with VEGAS menus, shortcuts and window layouts
- Media import for any format FFmpeg can read; drag from Project Media, the Explorer tab or Windows File Explorer onto the timeline
- Multitrack editing: move, trim, split, snapping, multi-select, grouping, automatic crossfades, ripple editing, slip/slide/roll/stretch, playback rate and reverse, takes, Event Properties
- Volume, pan, composite level and mute envelopes; Event Pan/Crop and Track Motion with keyframes
- Real-time multi-layer preview with persistent decoders, frame caches and video proxies
- Video FX, transitions, media generators, Titles & Text and Color Grading (lift/gamma/gain, curves, .cube LUTs), with presets and keyframes
- Streaming audio mixer with waveforms and meters; audio FX (EQ, compressor, gate, reverb, delay) on events, tracks and the master; recording into armed tracks
- Render As through the same engine as the preview (FFmpeg encoders)
- Autosave, safe saves, undo/redo, and an auto-updater
- Project files are `.mnle` (JSON)

## Overlay packs

**View > Window > Overlays** keeps your overlay video packs (light leaks, film grain, dust and scratches, bokeh, flares, frames, keyed elements) in a library folder, by default `Documents\Milan Pro\Overlay Packs` (change it in **Options > Preferences > General**). Each pack is a subfolder: **Import Pack...** copies a folder or extracts a `.zip` into the library, or links an existing folder where it is. Browse by pack, favourites, recent or search (names and tags), with thumbnails that scrub as you hover. Drag an overlay onto the timeline, or double-click it to add it at the cursor on a new track; the new track gets the overlay's blend (Normal, Screen, Add, Multiply, Overlay, or a green/blue screen key), set per pack in **Pack Properties** or per overlay from its right-click menu. Removing a copied pack moves its folder to the Recycle Bin; nothing is deleted for good. Milan Pro ships no third-party overlays; **Create Sample Pack** makes a few with FFmpeg.

## Install (Windows)

Download `MilanPro-<version>-Setup.exe` from [Releases](../../releases) and run it. It installs per-user by default (no admin needed), adds a Start Menu shortcut, and can be removed from Settings > Apps.

The installer bundles [FFmpeg](https://ffmpeg.org/) 9.0.2 (GPL v3, build by Gyan Doshi), installed next to `MilanPro.exe`. Source links are in `FFMPEG-SOURCE.txt` in the install folder. When building from source, run `installer/fetch-ffmpeg.ps1` or have FFmpeg on `PATH`.

## Updates

A few seconds after startup Milan Pro asks GitHub for the latest release (it sends only a `MilanPro-Updater/<version>` user agent). When a newer version exists it shows the release notes with **Download and install**, **Remind me later** and **Skip this version**. The installer is downloaded over HTTPS, checked against its SHA-256 (the release's asset digest or `MilanPro-<version>-Setup.exe.sha256`), and run silently; Milan Pro asks to save your project, closes, updates and starts again. Check by hand with **Help > Check for Updates...**, or turn the startup check off in **Options > Preferences > General**.

## Import from VEGAS Pro

Milan Pro cannot read `.veg` files directly. In VEGAS, choose **Tools > Scripting > Run Script...** and pick `Export to Milan Pro.cs` from the `vegas` folder of the Milan Pro install (`tools/vegas` in the source; use the `(VEGAS 13)` copy in VEGAS Pro 13 and earlier). It writes `<project>.milan.json`; open that in Milan Pro with **File > Import > VEGAS Project...**. Tracks, events (including Pan/Crop, subclips and reversed events), fades, takes, groups, track envelopes, markers and regions come across; Milan Pro lists what does not (FX, generated media, transitions and Track Motion). Picking a `.veg` there instead only adds the media it uses to Project Media.

## OFX plug-ins

Milan Pro runs third-party OFX video effects (Filter and General effects, 8-bit or 32-bit float) in **milan-plugin-host.exe**, a separate process installed next to `MilanPro.exe`: a plug-in that crashes or hangs is stopped and bypassed with a message, and Milan Pro keeps running. Its settings stay in the project, so nothing is lost.

- **Where Milan Pro looks:** `C:\Program Files\Common Files\OFX\Plugins` (the standard folder, where installers put `*.ofx.bundle` folders), every folder in the `OFX_PLUGIN_PATH` environment variable (separated by `;`), and folders you add in **View > Window > Plug-In Manager > Folders > Add Folder...**.
- **Using them:** found effects appear in the Video FX window under their own grouping (for example "spektrafilm OFX"), and apply to events, tracks and Video Output like the built-in FX, with their parameters and keyframes in the FX window. Projects store them as `ofx:<identifier>@<major version>`; a project that uses a plug-in you do not have keeps its settings and shows the FX as missing.
- **Plug-In Manager** (View > Window, Ctrl+Alt+1): every plug-in with its status (OK, Failed with the reason, Disabled), a check box to turn one off, and **Rescan** (reads every plug-in again and tries failed ones again). Results are cached in `%LOCALAPPDATA%\MilanPro\plugins.json`.
- **Limits:** Milan Pro does not provide GPU render suites (CUDA, OpenCL, Metal, OpenGL), overlays/on-screen controls, custom parameter UIs, push buttons, parametric (curve) parameters or other frames than the current one (temporal access). Plug-ins that need these, or that only run in a particular host (for example DaVinci Resolve-only plug-ins), may fail or not show. Generator and Transition plug-ins are listed but not run yet. See `docs/compat/ofx-spektrafilm.md` for Spektrafilm.

The OFX C API bindings in `src/bin/milan-plugin-host/ofx.rs` were written from the OpenFX 1.4 headers, Copyright (c) 2003-2015 The Open Effects Association Ltd, used under the BSD 3-Clause licence.

## Build from source

```bash
cargo build --release
```

Requires Rust 1.85+ (edition 2024). The binaries are written to `target/release/milan-pro.exe` and `target/release/milan-plugin-host.exe` (the OFX plug-in host, which must sit beside it).

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

- VST plug-in hosting; OFX transitions, generators and GPU render
- Masks, freely dockable panels, direct `.veg` reading
- EDL / OTIO interchange

## License

MIT. See [LICENSE](LICENSE). Milan Pro isn't affiliated with or endorsed by VEGAS Creative Software or Sony.
