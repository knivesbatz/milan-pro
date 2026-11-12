# Milan Pro

An open-source nonlinear video editor written in Rust, with a workflow modeled on Sony VEGAS Pro.

> Early development (v0.4). Editing, preview, effects, transitions, titles, audio FX, recording and rendering work; plug-in hosting is next.

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

## Install (Windows)

Download `MilanPro-<version>-Setup.exe` from [Releases](../../releases) and run it. It installs per-user by default (no admin needed), adds a Start Menu shortcut, and can be removed from Settings > Apps.

The installer bundles [FFmpeg](https://ffmpeg.org/) 9.0.2 (GPL v3, build by Gyan Doshi), installed next to `MilanPro.exe`. Source links are in `FFMPEG-SOURCE.txt` in the install folder. When building from source, run `installer/fetch-ffmpeg.ps1` or have FFmpeg on `PATH`.

## Updates

A few seconds after startup Milan Pro asks GitHub for the latest release (it sends only a `MilanPro-Updater/<version>` user agent). When a newer version exists it shows the release notes with **Download and install**, **Remind me later** and **Skip this version**. The installer is downloaded over HTTPS, checked against its SHA-256 (the release's asset digest or `MilanPro-<version>-Setup.exe.sha256`), and run silently; Milan Pro asks to save your project, closes, updates and starts again. Check by hand with **Help > Check for Updates...**, or turn the startup check off in **Options > Preferences > General**.

## Import from VEGAS Pro

Milan Pro cannot read `.veg` files directly. In VEGAS, choose **Tools > Scripting > Run Script...** and pick `Export to Milan Pro.cs` from the `vegas` folder of the Milan Pro install (`tools/vegas` in the source; use the `(VEGAS 13)` copy in VEGAS Pro 13 and earlier). It writes `<project>.milan.json`; open that in Milan Pro with **File > Import > VEGAS Project...**. Tracks, events (including Pan/Crop, subclips and reversed events), fades, takes, groups, track envelopes, markers and regions come across; Milan Pro lists what does not (FX, generated media, transitions and Track Motion). Picking a `.veg` there instead only adds the media it uses to Project Media.

## Build from source

```bash
cargo build --release
```

Requires Rust 1.85+ (edition 2024). The binary is written to `target/release/milan-pro.exe`.

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

- OFX / VST plug-in hosting
- Masks, freely dockable panels, direct `.veg` reading
- EDL / OTIO interchange

## License

MIT. See [LICENSE](LICENSE). Milan Pro isn't affiliated with or endorsed by VEGAS Creative Software or Sony.
