# Milan Pro

An open-source nonlinear video editor written in Rust, with a workflow modeled on Sony VEGAS Pro.

> Early development (v0.1). The interface, timeline and project format work; video preview, audio playback and rendering are in progress.

## Features (v0.1)

- VEGAS-style dark interface: media browser, preview monitor, multitrack timeline, properties panel
- Video tracks above audio tracks, with mute / solo / lock state
- Media import for any format FFmpeg can read (metadata via `ffprobe`)
- Clips: add from media, select, split at cursor, delete
- Timeline markers, ruler, playhead, Ctrl+scroll zoom
- Project save/load (`.mnle`, JSON)
- Shortcuts: Space play/pause, Home go to start, Delete remove clip

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

| File | Purpose |
|------|---------|
| `src/main.rs` | Entry point and window setup |
| `src/app.rs` | Application state, panel layout, playback clock, shortcuts |
| `src/project.rs` | Project, timeline, track, clip and media data model |
| `src/media.rs` | Media probing via `ffprobe` |
| `src/ui/mod.rs` | Theme and all panels, including the custom-painted timeline |

## Roadmap

- Video preview frames and audio playback
- Clip drag, trim, snapping, undo/redo
- Render / export via FFmpeg
- Effects and transitions, OFX plugin hosting
- EDL / OTIO interchange

## License

MIT. See [LICENSE](LICENSE). Milan Pro isn't affiliated with or endorsed by VEGAS Creative Software or Sony.
