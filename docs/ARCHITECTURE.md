# Milan Pro architecture

Status: R53 (2026-11-26), written against 0.8.0. This is what the code does today. The original engine design, with
its reasoning, is [FEATURE_PLAN.md](FEATURE_PLAN.md) section 1; where the code went further than that plan (GPU
compositing, cpal instead of rodio, plug-in hosting, reliability), this file says so. How VEGAS's concepts map onto
these structures is [REFERENCE_ANALYSIS.md](REFERENCE_ANALYSIS.md) section 11. How to build, test and extend the code
is [DEVELOPING.md](DEVELOPING.md).

Contents:
1. Shape of the program
2. Modules
3. Threads and processes
4. Data flow: project to picture and sound
5. The plug-in host
6. Theming
7. Reliability, updater and file formats
8. The screenshot harness
9. Test strategy
10. Decision records

---

## 1. Shape of the program

One Cargo package, `milan-pro` ([Cargo.toml](../Cargo.toml)), Rust edition 2024, builds two Windows executables:

| Binary | Source | Role |
|---|---|---|
| `milan-pro.exe` (installed as `MilanPro.exe`) | `src/main.rs` | The editor: an eframe/egui 0.31 application painted through wgpu (Vulkan; OpenGL with the `glow` feature). It never loads plug-in code. |
| `milan-plugin-host.exe` | `src/bin/milan-plugin-host/` | Loads and runs OFX, VST3 and CLAP plug-ins in a process of its own, so a plug-in that crashes or hangs cannot take the editor down (section 5). |

Everything media-related that is not Milan's own maths goes through **FFmpeg as child processes** (`ffmpeg.exe`,
`ffprobe.exe` beside the executable, or on `PATH`): probing, decoding, proxies, encoding. No FFmpeg library is linked
(decision record 10.1). Every child is started through `src/proc.rs`, which hides its console and puts it in a Job
Object, so Windows kills it when Milan Pro exits, crashes or is killed.

Three test plug-ins live in `tools/` (`ofx-test-plugin`, `vst3-test-plugin`, `clap-test-plugin`), each its own small
crate built by the tests that use them. `tools/vegas/` holds the C# scripts that run inside VEGAS for the interchange.

## 2. Modules

| Module | What it owns |
|---|---|
| `src/main.rs` | Startup: the single-instance mutex the installer waits on, the logger and crash hooks, the window (off-screen and unfocused under the screenshot harness), then `app::MeridianApp`. |
| `src/app.rs`, `src/app/` | `MeridianApp`, the application state: the open project and its undo history, preferences, the preview, player, recorder, updater, plug-in manager and every open dialog. `commands.rs` executes a `Command` (`exec`), `drop.rs` handles drags from Windows, `multicam.rs`, `nested.rs`, `sync.rs`, `audio_editor.rs`, `reliability.rs` the features of the same names. |
| `src/project.rs`, `src/project/` | The data model, serialised as the `.mnle` JSON: `Project`, `ProjectSettings`, `Timeline`, `Track`, `Clip`, `MediaItem`, `Fx`, `Envelope`, `Keyframes<T>`, markers, regions, busses. `migrate.rs` upgrades older files by `format_version`; `validate.rs` checks a file after it is read. |
| `src/edit.rs`, `src/history.rs` | Every timeline edit as a function on `&mut Project` (trim, split, ripple, paste, track operations, media drops), and undo/redo as whole-project snapshots (100 steps). |
| `src/engine/` | The timeline engine, one plan for preview, playback and render (section 4): `plan.rs`, `worker.rs`, `vdecode.rs`, `composite.rs`, `gpu/`, `vfx.rs`, `transition.rs`, `generate.rs`, `raster.rs`, `mix.rs`, `afx.rs`, `resample.rs`, `render.rs`, the caches, the plug-in registry (`plugins.rs`), OFX (`ofx/`) and VST3/CLAP (`aplug/`) clients, and the measurement code (`scopes.rs`, `loudness.rs`, `sync.rs`, `hw.rs`). |
| `src/audio.rs`, `src/wasapi.rs`, `src/record.rs` | The audio device layer (cpal in WASAPI shared mode, or Milan's own WASAPI exclusive output), the playback clock, waveform peaks, and recording into armed tracks. |
| `src/decode.rs`, `src/media.rs`, `src/proxy.rs` | The preview's handle to the video engine and the Trimmer's single-source decoder; ffprobe import of media; video proxies. |
| `src/render.rs` | Render templates (built-in and the user's), their encoder arguments, and the background `Job` the Render As dialog watches. |
| `src/interchange/` | VEGAS (through the `.milan.json` scripts), OpenTimelineIO, CMX3600 EDL, FCP7 XML, FCPXML, the project archive. |
| `src/settings.rs` | Project Properties and the preferences file. |
| `src/autosave.rs`, `src/logger.rs`, `src/diagnostics.rs`, `src/updater.rs` | Reliability and updates (section 7). |
| `src/overlays.rs` | The Overlays window's pack library. |
| `src/demo.rs` | The screenshot harness and its demo project (section 8). |
| `src/ui/` | Everything drawn: `theme.rs` (the look's tokens), `chrome.rs`, `menus.rs`, `keymap.rs`, `toolbar.rs`, `palette.rs`, `dock.rs` (docking areas, floating windows, layouts), `timeline/`, `preview.rs`, `fx_window.rs`, `mixer.rs`, the other dock windows, and `dialogs/`. |

## 3. Threads and processes

The UI thread runs egui's frame loop (`MeridianApp::update`). Long work never runs on it; it hands work to threads and
polls their results once per frame.

| Thread or process | Count | Job |
|---|---|---|
| UI | 1 | Input, edits, drawing. Hashes the project each frame; when the hash changes it sends a new video plan to the video worker and swaps the audio plan in the mixer. |
| `milan-video` | 1 | Owns the compiled plan, the decoder pool and the frame caches; answers "show frame n" when paused and streams frames while playing (`engine/worker.rs`). Composes on the CPU in row stripes on scoped threads, or on the GPU. |
| `milan-gpu` | 1 at startup | Finds the graphics adapters and opens the GPU compositor's device off the UI thread when it cannot share the UI's (the `glow` build, or another adapter chosen in Preferences). |
| Video decoders | up to 8 streams | Each an `ffmpeg` child plus a reader thread, streaming raw RGBA frames forward (`engine/vdecode.rs`). |
| `milan-mix` | 1 per play session | Mixes the audio plan block by block from persistent `ffmpeg` PCM streams and queues the blocks (`engine/mix.rs`). |
| `milan-audio-device` / `milan-wasapi` | 1 | The device: cpal's callback, or the exclusive-mode event loop. Resamples to the device rate, adds input monitoring, applies the master, keeps the playback clock. Takes no lock and frees nothing. |
| `milan-render` | 1 per render | Composes every frame and writes it to the encoder's stdin; the queue renders one job at a time. |
| `milan-rec`, `milan-rec-wav` | while recording | Capture and the WAV writer. |
| `milan-hw` | 1 at startup | Tests the hardware decoders and encoders once per FFmpeg and driver version (`engine/hw.rs`). |
| `milan-plugin-scan` | while scanning | Describes new or changed plug-in binaries through host processes. |
| `milan-ofx-watchdog`, `milan-ofx-button` | per host | Kills a host that does not answer in time; runs an OFX push button off the UI thread. |
| `milan-scopes`, `milan-loudness`, `milan-sync`, `milan-framecache`, `milan-vegas` | on demand | Video Scopes, Loudness Meters, audio-based synchronisation, the disk frame cache writer, VEGAS import. |
| Thumbnail and waveform workers | a few | Event thumbnails and peaks; they pause while playing. |
| `milan-plugin-host.exe` | 1 per plug-in binary in use | Separate process (section 5); its `milan-plugin-editor` thread pumps a VST3 editor window's messages. |
| `ffmpeg.exe` / `ffprobe.exe` | many, short or persistent | Decoders, the encoder, probes, proxies, the archive's trims, the hardware tests. |

Channels are bounded (`sync_channel`), so a slow consumer slows its producer instead of filling memory. The device
callback hands anything it lets go of back to the UI thread (`Trash` in `audio.rs`) so it never frees memory itself.

## 4. Data flow: project to picture and sound

```
Project (src/project.rs)
   │  hash changed?
   ▼
plan::compile ───────────────► Plan (pure numbers: tracks, layers, sources, FX evaluated per time)
   │                                │
   │ plan::compile_audio            │ plan::frame_plan(n) once per frame
   ▼                                ▼
AudioPlan ──► mix::Mixer ──►    worker ("milan-video")
              ("milan-mix")       ├─ vdecode: ffmpeg streams → RGBA frames (+ proxies, prerendered sections, disk cache)
              afx chains,         ├─ generate / OFX host for generated and plug-in pictures
              aplug (VST3/CLAP)   └─ Compositor: CpuCompositor (composite.rs) or GpuCompositor (gpu/)
                  │                                │
                  ▼                                ▼
            audio::Out (device)            Picture → Video Preview texture (zero-copy on the shared wgpu device)
            playback clock ───────────────► frame n to show

Render (engine/render.rs): the same Plan, frame_plan, Compositor and Mixer, at the template's size and Best quality,
piped into one ffmpeg encoder process (raw RGBA on stdin, f32 audio from a temp file), written to <out>.partial and
renamed when the encoder has finished.
```

**Plan compiler** (`engine/plan.rs`). The only code that interprets the timeline. `compile` runs once per edit and
resolves the project into plain numbers: which source each event reads at which media time (rate, reverse, velocity
envelopes, loops, takes, nested timelines), its geometry (Pan/Crop, Track Motion, parent/child compositing), its FX and
transitions, envelopes and fades. `frame_plan(n)` then gives the layers of one frame, cheaply, and
`compile_audio` gives the mixer its graph (events, tracks, busses, sends, FX chains in dependency order). Because
preview, playback and render all read the same plan, what is seen is what is rendered.

**Decoders** (`engine/vdecode.rs`). Persistent `ffmpeg` processes per source, each streaming raw RGBA over a pipe at
the size the frame needs. A request near a stream's position reads on; anything else starts a new stream. Reverse
playback decodes forward in chunks that end at the wanted frame. Draft and Preview quality read the proxy when there is
one; prerendered sections and the optional disk cache for heavy sources (HEVC, AV1, 4K) stand in for decoding when they
match the plan's hash.

**Compositor** (`engine/composite.rs`, `engine/gpu/`). A `Compositor` trait with two implementations. The CPU one
blends premultiplied 8-bit layers bottom to top in row stripes. The GPU one (wgpu on Vulkan, R21) gives the same
picture (the contract is within 2/255 per channel; it is bit-exact on every plan tested): integer passes for the integer blend modes, and a replay of the CPU's float
operations for the float modes. Video FX, transitions and generators with a GPU port run as WGSL passes; those
without (Bezier Masking, text, OFX plug-ins) run on the CPU and are uploaded. On the shared device the preview shows the
composed texture directly, so playback copies no frame off the GPU; a render, the scopes and snapshots read it back.

**Mixer** (`engine/mix.rs`). Stereo f32 at 48 kHz (`audio::RATE`). Per block it pulls PCM from persistent `ffmpeg`
streams, applies event fades, gain, pan and envelopes, runs the FX chains (`engine/afx.rs`, and VST3/CLAP nodes through
the plug-in host), routes tracks into busses, sends and the master in dependency order, compensates plug-in latency
sample-exactly, and meters every node. A live edit swaps the plan in the running session and is heard within about
150 ms.

**Render** (`engine/render.rs`, `src/render.rs`). A render takes a snapshot of the project, mixes the audio first,
then composes every frame at the template's size and writes it to one `ffmpeg` encoder. Templates are FFmpeg argument
sets; hardware encoders are offered only after they passed the startup test. The render queue (R12) runs jobs in the
background while editing continues. Every render writes a log of its command and FFmpeg's output.

## 5. The plug-in host

`milan-plugin-host.exe` is started by Milan Pro (`engine/ofx/client.rs`) with `--pipe <name>`, inside the same Job
Object as every other child, with no window or console. One host process serves one plug-in binary.

- **Control channel:** a named pipe carrying length-prefixed JSON messages, one reply per request
  (`engine/ofx/proto.rs`, shared by both binaries). OFX requests: `Load`, `Describe`, `Render`, `Button`. Audio
  requests: `AScan`, `AOpen`, `AProcess`, `AGetState`, `ASetState`, `AEditor`, `AClose`.
- **Data channel:** pictures and audio blocks never travel over the pipe. They are written into a shared-memory
  section that both processes map; a request only says where they are.
- **Watchdog:** each request has a deadline (10 s to load a binary, 2 s to describe a plug-in, 30 s for a render).
  A host that misses it is killed; a host that dies is noticed on the pipe. Either way the effect is bypassed for the
  session with a message, and its settings stay in the project.
- **Scanning:** new or changed binaries are described in a host and the results cached in
  `%LOCALAPPDATA%\MilanPro\plugins.json` by path, size and modified time (`engine/ofx/scan.rs`).
- **OFX** (`bin/milan-plugin-host/host.rs`, `suites.rs`, `ofx.rs`): hand-written `#[repr(C)]` bindings to the
  OpenFX 1.4 C API and the property, image effect, parameter, memory, multithread, message, progress and time line
  suites; Filter, General, Transition and Generator contexts at 8-bit, 16-bit, half and float depths. Interact,
  parametric and GPU suites are not offered.
- **VST3** (`vst3.rs`, through the `vst3` crate) and **CLAP** (`clap.rs`, through `clap-sys`): stereo 32-bit float
  effects at the project rate, parameters, state, programs, latency and tail. A VST3 editor opens in a window the host
  owns on its own thread (`editor.rs`); under tests and the harness it is created off every monitor and never
  activated.
- **On Milan's side** `engine/ofx/mod.rs` and `engine/aplug/mod.rs` turn each described plug-in into an entry of the
  same registry the built-ins use (`engine/plugins.rs`), so the choosers, the FX window, keyframes, presets and the
  project file treat `ofx:`, `vst3:` and `clap:` plug-ins like `milan.*` built-ins.

## 6. Theming

Milan Pro has one look, modelled on the Windows video editors of 2005-2007 (bevelled chrome, Tahoma 8 pt, Windows
2000-style dialogs and controls) and laid out like VEGAS Pro: its docking areas, menus, toolbars and keys follow VEGAS.
There is no look switching.

- **Tokens:** `Theme` (`src/ui/theme.rs`) holds every colour, size, font and metric the drawing code uses, from the
  look's specification ([CLASSIC_2006_SPEC.md](CLASSIC_2006_SPEC.md)), and maps them onto egui's `Visuals`
  (`Theme::apply`). Drawing code calls `theme::get()` and never hard-codes a colour.
- **Structure:** the layout, docking areas, menu trees (`menus.rs`), key table (`keymap.rs`), toolbar buttons
  (`toolbar.rs`), built-in layouts (`dock.rs`) and dialog frames (`dialogs/mod.rs`) follow VEGAS Pro's, as measured
  in [VEGAS_UI_SPEC.md](VEGAS_UI_SPEC.md). The window chrome is drawn by `chrome.rs`.

Fonts are read at run time from the Windows fonts folder (`src/ui/fonts.rs`: Tahoma, with Segoe UI and then egui's
bundled fonts as fallbacks); Milan Pro ships no font file. Icons are drawn as vector paths (`src/ui/icons.rs`), not
bitmaps. Window layouts and keyboard overrides are saved in the preferences.

Every command has an entry in `keymap::Command`. Menus, toolbars, the command palette and the keyboard all send the
same `Command` to `MeridianApp::exec`. A command that cannot run yet is greyed with a reason (`menus::unfinished`), and
the reason shows on hover, in Help > Unfinished Features and in the palette (R0).

## 7. Reliability, updater and file formats

- **Project file** `.mnle`: JSON of `Project` with `format_version` (`project::FORMAT_VERSION`). Older files are
  migrated step by step on the JSON before deserialising (`project/migrate.rs`); a file from a newer build opens
  read-only. Each save is atomic (write, then rename) and keeps the previous file as `.mnle.bak`. New fields carry
  `#[serde(default)]`.
- **Autosave and recovery** (`autosave.rs`): timed autosaves into `%APPDATA%\MilanPro\autosave`, never over the user's
  file, and session markers that let the next start offer what a crashed session left.
- **Logs and crashes** (`logger.rs`, `diagnostics.rs`): rotating logs and crash reports with minidumps under
  `%LOCALAPPDATA%\MilanPro`; Help > Create Diagnostic Report gathers them into a folder the user can attach. Nothing is
  sent anywhere by itself.
- **Updater** (`updater.rs`): asks GitHub Releases for the latest release on the chosen channel, downloads the
  installer over HTTPS from GitHub hosts only, checks its SHA-256 (and its minisign signature once a key is compiled
  in), and runs it silently; the installer relaunches Milan Pro. Rollback reinstalls the previous version's installer.
  [SECURITY.md](../SECURITY.md) has the trust model.
- **Other files:** preferences (`prefs.json`), presets, layouts, the plug-in cache, the hardware table and overlay
  index are JSON under `%APPDATA%\MilanPro` or `%LOCALAPPDATA%\MilanPro`. Interchange formats are in
  [INTERCHANGE.md](INTERCHANGE.md).

## 8. The screenshot harness

The harness is how the UI is checked without anyone at the screen (`src/demo.rs`). With `MILAN_SCREENSHOT=<file.png>`
the window is created at 1920×1080 off every monitor, without focus or a taskbar button; Milan Pro waits until the
preview and every pending frame have settled, saves the PNG and exits by itself. `MILAN_DEMO=1` loads a demo project on
generated media and writes nothing to the user's preferences or autosave folder. `MILAN_DIALOG`, `MILAN_PANELS`
 and the other variables listed in [DEVELOPING.md](DEVELOPING.md) choose what is on screen. Tests launch
the built executable the same way (for example the crash-recovery test in `autosave.rs`).

## 9. Test strategy

- **Unit tests beside the code** (`#[cfg(test)] mod tests` in each module, about 650): edit maths, plan compilation,
  keyframe curves, the mixer, migrations, the interchange formats (round trips and the fixtures in `tests/fixtures/`),
  the menu and key tables. They need no FFmpeg, GPU or network, and run with `cargo test`.
- **Rules as tests:** some tests guard project rules instead of behaviour: every greyed control has a reason
  (`menus::tests`), no command waits on a finished tier, the palette's chord collides with no VEGAS key, the CPU and
  GPU compositors agree, and every relative link in the documentation resolves (`tests/doc_links.rs`).
- **Ignored tests** (`#[ignore]`, about 70): need FFmpeg, a GPU, the built plug-in host, the test plug-ins, or a
  long run. They decode and render real media, round-trip renders through ffprobe, crash and hang plug-in hosts on
  purpose, and launch the harness. Run them with `cargo test --release -- --ignored`.
- **Benchmarks:** ignored tests that print `BENCH` lines, run by `tools/bench.ps1`, which writes
  [PERFORMANCE.md](PERFORMANCE.md).
- **Screenshots:** each UI batch is reviewed from harness screenshots of the windows it changes.
- **CI** (`.github/workflows/ci.yml`): `cargo build --release` and `cargo test` on `windows-latest` for every push
  to `main` and every pull request.

## 10. Decision records

Each record gives the choice, what was compared, and why. They were made in [FEATURE_PLAN.md](FEATURE_PLAN.md) and
the batch notes in [REMAINING.md](REMAINING.md); this section writes the comparisons down.

### 10.1 FFmpeg as child processes, not libav bindings or GStreamer

- **FFmpeg CLI (chosen):** one prebuilt, pinned binary set (`installer/fetch-ffmpeg.ps1`) covers every format, the
  hardware decoders and encoders, and filters. Raw frames and PCM come over pipes. A decoder crash, a hang on a broken
  file or a leak ends one child process, never the editor. Builds need no C toolchain, and the GPL build stays a
  separate program next to an MIT one (aggregation, [LICENSES.md](LICENSES.md)). Cost: a pipe copy per frame, a
  process start per new stream, and no access to decoder internals; persistent streams and caches keep the cost down,
  and [PERFORMANCE.md](PERFORMANCE.md) measures it.
- **libav bindings (`ffmpeg-next`, `ffmpeg-sys`):** in-process, no copies, but they tie the build to FFmpeg's headers
  and import libraries of one version, need a C toolchain and bindgen on every machine that builds Milan Pro, put a
  crash in a decoder inside the editor, and make the editor's binary link GPL code when linked to a GPL build.
- **GStreamer (`gstreamer-rs`):** a full pipeline framework with good plug-in coverage, but it adds a large runtime to
  install (about 100 MB of DLLs and plug-ins), its own scheduling model to fit under an NLE's frame-exact seeking, and
  LGPL/GPL plug-in sets to sort through; format coverage on Windows is still FFmpeg's (through `gst-libav`). Nothing it
  offered was missing from the CLI route.

### 10.2 egui (eframe) for the interface, not Qt, Slint or iced

- **egui (chosen):** immediate mode suits a UI that redraws every frame anyway (playback, meters, a scrubbing
  timeline); custom painting is the normal way to draw, which the timeline, envelopes, scopes and the bevelled look
  need; pure Rust, no code generation, MIT/Apache; one `cargo build`. Multiple native windows (floating dock windows)
  through viewports. Cost: no native widgets, so every control is drawn to match the look; accessibility through
  AccessKit only.
- **Qt (through `cxx-qt` or `qmetaobject`):** mature docking and widgets, but a C++ toolchain and Qt's installer in
  every build, LGPL obligations for the shipped DLLs, and a timeline that would be custom-painted anyway.
- **Slint:** declarative and fast, but its licence is GPL-3.0 or a royalty-free licence with conditions, or a paid
  one, which does not fit an MIT project, and its custom painting is less direct than egui's.
- **iced:** Elm-style, MIT, good on wgpu, but it had no multi-window or docking support of the maturity needed when
  the choice was made, and the message-passing model costs more code for a dense editor UI.

### 10.3 wgpu, not glow (OpenGL)

eframe can paint through glow (OpenGL) or wgpu. Milan Pro painted through glow until R21a, then moved to wgpu: the GPU
compositor needs compute-capable, explicit GPU access (WGSL, storage textures), and sharing one wgpu device between
the UI painter and the compositor lets the preview show composed frames without a copy. wgpu is built with its Vulkan
backend (and GL for machines without Vulkan); Direct3D 12 was left out because its backend pulls in `gpu-allocator`
and more crates for no measured gain. The `glow` feature still builds: the compositor then opens its own device and
reads frames back.

### 10.4 cpal, not rodio

The first audio path used rodio, which hides the device behind a sink and gives no latency or timing figure, so the
playhead ran 10-20 ms ahead of the sound. R9 replaced it with cpal (already in the tree under rodio): the device
callback, the buffer size and the output device are Milan's to choose, the playback clock follows the samples the
device has actually consumed, and input monitoring and recording share the layer. WASAPI exclusive mode, which cpal
0.15 lacks, is Milan's own small module (`wasapi.rs`), standing in for ASIO, whose SDK cannot be redistributed.

### 10.5 JSON project files

`.mnle` is `serde_json` of the data model. Readable and diffable, recoverable by hand, readable by other tools without
Milan Pro, and versioned (`format_version` with migrations, R42). The cost is size and parse time on very large
projects, which has not been measured as a problem; a binary container would make the file opaque the way `.veg` is,
which is the problem Milan Pro's interchange has to work around.

### 10.6 Plug-ins out of process

OFX, VST3 and CLAP plug-ins are third-party native code. In process, one crash, hang or heap corruption in any of them
ends the editor with the user's unsaved work. In `milan-plugin-host.exe` a plug-in that fails is killed and bypassed
with a message. The cost is a shared-memory copy per frame or block and a pipe round trip per request: about 10.5 ms for a
1080p float frame (0.7.0 release notes), and one round trip per 1024-frame audio block. Running the host
per binary also isolates one vendor's plug-ins from another's. VEGAS itself ran 32-bit VST plug-ins out of process
(`sfvstserver.exe`), a precedent ([REFERENCE_ANALYSIS.md](REFERENCE_ANALYSIS.md) section 6.2).
