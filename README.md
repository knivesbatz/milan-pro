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

Download `MilanPro-v0.1.0-win64.zip` from [Releases](../../releases), extract it, and either run `MilanPro.exe` directly or run `install.bat` as Administrator to install with Start Menu and Desktop shortcuts.

[FFmpeg](https://ffmpeg.org/) must be on `PATH` for media import (`winget install Gyan.FFmpeg`).

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
- VEGAS project (`.veg`) import, EDL / OTIO interchange
- Auto-updater

## License

MIT. See [LICENSE](LICENSE). Milan Pro isn't affiliated with or endorsed by VEGAS Creative Software or Sony.
