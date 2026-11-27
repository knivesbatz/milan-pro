# Developing Milan Pro

Status: R53 (2026-11-26). How to build, run, test and extend Milan Pro. Read [ARCHITECTURE.md](ARCHITECTURE.md) first
for what the parts are; [CONTRIBUTING.md](../CONTRIBUTING.md) has the rules for pull requests.

## Toolchain

- **Windows 10 or 11, x64.** Milan Pro is a Windows application today (WASAPI, Job Objects, DXGI, the installer). The
  engine and most tests are portable Rust, but nothing else is built or tested.
- **Rust 1.85 or later** (edition 2024), stable, from [rustup](https://rustup.rs). The MSVC toolchain
  (`x86_64-pc-windows-msvc`) with the Visual Studio Build Tools' C++ workload, for the linker.
- **FFmpeg** for running the app and for the ignored tests: `installer/fetch-ffmpeg.ps1` downloads the pinned build
  (9.0.2, checked against its SHA-256) into `installer/ffmpeg`, which is ignored by git. Put that folder on `PATH`, or
  copy `ffmpeg.exe`, `ffprobe.exe` and the DLLs next to `target/release/milan-pro.exe`; Milan Pro looks beside itself
  first.
- **A GPU with Vulkan** for the GPU compositor (OpenGL-only machines fall back to the CPU compositor).
- Optional: **Inno Setup 6** to build the installer; **Python 3 with Pillow** for `tools/make-icons.py`.

## Build and run

```
cargo build --release
target\release\milan-pro.exe
```

`cargo build` builds both binaries: `milan-pro.exe` and `milan-plugin-host.exe`, which must stay in the same folder.
Release builds use the Windows subsystem (no console); debug builds keep the console for `log` output.

`cargo build --release --no-default-features --features glow` builds the OpenGL painter instead of wgpu; the GPU
compositor then opens its own device and reads frames back. Keep both building.

## Tests

```
cargo test --release                       # the unit tests: no FFmpeg, GPU or network needed
cargo test --release -- --ignored          # the heavy ones: need FFmpeg on PATH, a GPU, the built host
cargo test --release some_name -- --ignored --nocapture   # one of them, with its output
```

The ignored tests decode and render real media made with FFmpeg's `lavfi` sources, compare the GPU compositor with the
CPU one, crash and hang plug-in hosts on purpose, build and host the test plug-ins in `tools/`, and launch the
screenshot harness. Run `cargo build --release` before them: they start the built executables. Some of them read
extra folders, given by environment variables (`MILAN_OFX_DIR`, `MILAN_AUDIO_PLUGIN_DIR`, `MILAN_VEGAS_OFX`,
`MILAN_COMPAT_OUT`, `MILAN_R26_SAMPLE_DIR`); without them they skip. Temporary files go under `%TEMP%`.

`tests/doc_links.rs` checks every relative link in the Markdown files of the repository; run it after editing docs.

**Benchmarks:** `powershell -ExecutionPolicy Bypass -File tools\bench.ps1` makes the bench media, runs the benchmark
tests one by one in a release build and rewrites [PERFORMANCE.md](PERFORMANCE.md). It takes about five minutes; keep
the machine otherwise idle.

**Licences:** `powershell -ExecutionPolicy Bypass -File tools\licenses.ps1` rewrites [LICENSES.md](LICENSES.md) and
`THIRD-PARTY-NOTICES.txt` from `Cargo.lock`; `-Check` fails when the committed files are stale. Run it whenever
`Cargo.lock` changes, and commit its output.

## The screenshot harness

The harness runs the real application without a person at the screen and without disturbing one: the window opens at
1920×1080 off every monitor, never takes focus and has no taskbar button; Milan Pro saves a PNG once the picture has
settled and exits by itself.

```
set MILAN_DEMO=1
set MILAN_SCREENSHOT=%TEMP%\shot.png
target\release\milan-pro.exe
```

| Variable | Effect |
|---|---|
| `MILAN_SCREENSHOT=<file.png>` | Take the screenshot and exit. Without it the window opens normally. |
| `MILAN_DEMO=1` | Load the demo project on generated media (cached in `%TEMP%\milan-demo`). Nothing is written to the user's preferences, autosave or plug-in cache. |
| `MILAN_DIALOG=<name>` | Open a dialog, window or menu first: `menu-edit`, `prefs`, `prefs-page-<n>` (R49, n in tab order), `prefs-update`, `button-set`, `master-env`, `recover`, `crash-offer`, `scopes`, `loudness`, `edit-details`, `plugin-manager`, `overlays` and the others in `open_from_env` (`src/app/commands.rs`). |
| `MILAN_PANELS=explorer,trimmer,...` | Dock windows to bring to the front. |
| `MILAN_DROP=x,y` | With `MILAN_DEMO`: the demo media hover at that point as if dragged from Windows. |
| `MILAN_OFX_DIR`, `MILAN_AUDIO_PLUGIN_DIR` | The only plug-in folders scanned for this run. |
| `MILAN_OVERLAY_LIBRARY=<folder>` | With `MILAN_DIALOG=overlays`: where the harness makes its overlay library. |
| `MILAN_HW_TABLE=<file.json>` | Show this hardware acceleration table in Preferences > Video instead of probing. |
| `MILAN_APPDATA=<folder>` | Move the preferences (prefs.json), autosave, sessions, logs and crash files to a test folder. |
| `MILAN_AUTOSAVE_NOW=1` | Autosave at the first tick (recovery tests). |
| `MILAN_HOLD=1` | Never take the screenshot or exit: stay open off-screen until killed (the crash-recovery test). |
| `MILAN_UPDATE_API=http://127.0.0.1:<port>/...` | Point the updater at a local test server (any other value is ignored). |
| `MILAN_UPDATE_AUTO=1` | With a local `MILAN_UPDATE_API`: check, download and install without clicks (the end-to-end updater test). |
| `MILAN_SHOT_TRACE=1` | Print what the screenshot is waiting for, when a run does not end. |
| `MILAN_SCALE=1.5` | Render at this interface scale (1 to 3) whatever the display's; prints `overflow:` lines for text that does not fit. |
| `MILAN_SHOT_SIZE=3840x2160` | The window's physical size (default 1920x1080), e.g. a 4K display at `MILAN_SCALE=2`. |
| `MILAN_IDLE_CHECK=1` | After the shot, wait 2 s asking for nothing and print `idle frames: n`, the frames painted meanwhile. |

The plug-in host has its own probe variables (`MILAN_HOST_DESKTOP`, `MILAN_HOST_LOG`, `MILAN_HOST_PROBE`) used by the
compatibility reports in `docs/compat/`.

Rules for anyone automating Milan Pro: launch it only through the harness, never in a way that shows a window, takes
focus or plays sound on someone's desktop, and keep test output in `%TEMP%`.

## Adding things

### A command

1. Add a variant to `Command` in `src/ui/keymap.rs`, with a doc comment saying what it is (and the VEGAS version it
   comes from).
2. If it has a default key, add a row to `TABLE` there (check that the chord is free).
3. Put it in the menu tables in `src/ui/menus.rs` (`C(label, Command)`), and decide
   when it is enabled in `CmdState::enabled` (and `checked` for a toggle).
4. Execute it in `MeridianApp::exec_one` (`src/app/commands.rs`). Edits go through `src/edit.rs` functions and push
   undo history.
5. Until it works, leave it greyed with a reason: `X(label, Later("R..."))` in the menu table, or an entry in
   `menus::unfinished`. A greyed control without a reason fails `menus::tests`.

The command then appears in the command palette and Customize Keyboard by itself.

### A dialog

Dialogs live in `src/ui/dialogs/`, one file each, re-exported from `dialogs/mod.rs`. A dialog is a struct held in an
`Option` on `MeridianApp`, opened by a command and drawn each frame with the frame helper in `dialogs/mod.rs`, which
draws the look's Windows 2000-style chrome. It returns what
the user chose; the app applies it through `edit.rs` or `settings.rs` so undo works. Add a `MILAN_DIALOG` name in
`open_from_env` so the harness can screenshot it.

### A built-in plug-in (video FX, transition, generator, audio FX)

1. Add a `PluginInfo` to `PLUGINS` in `src/engine/plugins.rs`: a `milan.*` id (never renamed: projects store it), the
   VEGAS-style name, kind, chooser group and category, `ParamSpec`s and presets.
2. Implement it: video FX in `engine/vfx.rs`, transitions in `engine/transition.rs`, generators in
   `engine/generate.rs`, audio FX in `engine/afx.rs`. Spatial sizes are fractions of the picture height.
3. For the GPU compositor, give it a `GpuPort` and a pass in `engine/gpu/fx.wgsl`, with a test that it matches the
   CPU; without a port the CPU code runs and the picture is uploaded.
4. If VEGAS has the same effect, map it in `src/interchange/vegas_map.rs` so imports convert it.

Hosting a new third-party plug-in format follows the OFX/VST3/CLAP pattern: describe and run it in
`milan-plugin-host.exe` (`src/bin/milan-plugin-host/`), add its requests to `engine/ofx/proto.rs`, and register what
it describes in the same registry with its own id prefix. Plug-in code never runs in `milan-pro.exe`.

### The look

Colours, sizes, fonts and metrics are tokens of `Theme` in `src/ui/theme.rs`, taken from the look's specification
([UI_SPEC.md](UI_SPEC.md)); drawing code reads them through `theme::get()` and never hard-codes a
colour. Change the specification and the tokens together, and screenshot the windows that change.

## Conventions

- **One batch, one commit:** work is planned as batches in [REMAINING.md](REMAINING.md); a commit's subject starts
  with its batch (`R53: ...`). Each batch ends with `cargo build` and `cargo test` green, the feature wired into the
  UI, and its status written in `REMAINING.md`.
- **No warnings** in `cargo build --release`.
- **Persistent fields** get `#[serde(default)]`; a change to the project format bumps `project::FORMAT_VERSION` and
  adds a migration step with a test (`src/project/migrate.rs`).
- **Child processes** go through `proc::spawn` / `proc::output`, never `Command::output` directly.
- **Known limits** are marked in the code as `// shortcut: <limit>, <when to upgrade>`.
- **No silent placeholders:** a control that does nothing yet is greyed and says why.
- **New crates:** only when nothing in `Cargo.lock` does the job; permissive licences only; say the licence in the
  commit, and rerun `tools/licenses.ps1`.
- **Comments say why**, not what; names follow VEGAS's where a VEGAS concept exists.

## Releases and versioning

Versions follow semantic versioning while below 1.0: the minor number goes up with each feature release, the patch
number for fixes. `Cargo.toml`'s `version` is the release version, and the updater compares releases against it.

1. Bump `version` in `Cargo.toml` (and `Cargo.lock` through `cargo build`) and the version line in `README.md`; add the
   release to [CHANGELOG.md](../CHANGELOG.md). Commit as `Release X.Y.Z`.
2. Tag `vX.Y.Z` and push the tag. A tag with a hyphen (`v0.9.0-beta.1`) becomes a pre-release, offered only on the
   Preview update channel.
3. `.github/workflows/release.yml` checks that the tag matches `Cargo.toml`, builds, fetches FFmpeg, signs when the
   secrets exist ([SECURITY.md](../SECURITY.md)), builds the installer with Inno Setup, writes the `.sha256` (and its
   `.minisig`), and publishes the release only after every file is uploaded.

Local installer build (for checking the script; the output goes to `release/`, which git ignores):

```
"%LOCALAPPDATA%\Programs\Inno Setup 6\ISCC.exe" /Q /DAppVersion=0.0.0 installer\milan-pro.iss
```

It needs `installer/ffmpeg` (from `fetch-ffmpeg.ps1`) and a release build.
