# Changelog

What changed in each Milan Pro release. Entries 0.2.0 to 0.7.0 are taken from the GitHub release notes; 0.8.0 from the
batches it merged (its release notes are written when it is published). Batch numbers (R..) refer to
[docs/REMAINING.md](docs/REMAINING.md). Versions follow [semantic versioning](https://semver.org) while below 1.0:
the minor number for feature releases, the patch number for fixes.

## Unreleased

### Added
- Documentation for contributors and users: [docs/REFERENCE_ANALYSIS.md](docs/REFERENCE_ANALYSIS.md) (R52),
  [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) and [docs/DEVELOPING.md](docs/DEVELOPING.md) (R53), a refreshed README
  with screenshots, a feature table, system requirements and the roadmap (R53).
- Help > Contents and Index opens the documentation on GitHub; Help > Download User Manual opens the README
  (R53).
- [CONTRIBUTING.md](CONTRIBUTING.md), [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md), issue forms for bugs, feature requests,
  plug-in compatibility and media formats, and a pull request template (R54).
- [docs/LICENSES.md](docs/LICENSES.md), generated from `Cargo.lock` by `tools/licenses.ps1`, with the FFmpeg, codec
  patent, plug-in SDK, font and installer notes; the installer now ships `THIRD-PARTY-NOTICES.txt` (R56).
- **Format capability system:** the bundled FFmpeg's demuxers, decoders, encoders, muxers, pixel formats and filters
  are read once per FFmpeg version; a codec it cannot decode imports offline with the reason (a red "!" badge), and
  Media Properties says what decodes, proxies and renders each file. A generated, legal test corpus
  (`tools/make-test-media.ps1`) checks decoding of every listed format; [docs/FORMAT_SUPPORT.md](docs/FORMAT_SUPPORT.md)
  has the results (R37).
- **Image sequences and stills:** numbered stills open as one clip at the project rate (with VEGAS's "Open still
  image sequence" question), animated GIF/WebP play as video, more still formats (16-bit TIFF, TGA, DPX, JPEG 2000,
  PSD, OpenEXR, Radiance HDR), and PNG, TIFF, OpenEXR and DPX sequence renders (R38).
- **HDR detection:** PQ and HLG sources are detected (transfer, primaries, bit depth, HDR10 metadata) and shown on the
  SDR timeline through a view transform that the preview, proxies and renders share, chosen in Project Properties >
  Color management; BT.2020 SDR is converted to Rec.709; an "HDR" badge in Project Media (R40a).
- **High bit depth and HDR:** Project Properties > Video's Pixel format (8-bit, 32-bit floating point video levels
  or full range), HDR Mode (HDR10, HLG), Compositing gamma (2.222, 1.000), ACES version, ACES color space
  (ACES2065-1, ACEScg, ACEScc, ACEScct) and View transform (sRGB, Rec.709, Rec.2020 PQ and HLG 1000 nits, or a .cube
  LUT) as VEGAS has them. A float project decodes 10-bit and wider sources at 16 bits (OpenEXR and float TIFF in
  float, highlights above 1.0 kept), composes in float on the CPU or the GPU (`Rgba16Float`), runs its colour FX,
  Color Grading, Gaussian Blur, masks, dissolves and OFX plug-ins in float, and renders 16 bits to the encoder: a
  10-bit source comes back within one code value from a 10-bit render. New templates: MP4 HEVC Main10, HEVC HDR10
  (PQ, with mastering display and content light level metadata), HEVC HLG, ProRes 4444 XQ and ProRes 4444 XQ HDR10;
  an HDR project's preview is tone-mapped to SDR. 8-bit projects compose and render bit for bit as before (R40b).
- **Camera cards:** File > Import > Camera Card Folder (or a dropped folder) imports AVCHD, XDCAM, XAVC, P2, Canon XF
  and GoPro clips, joins spanned recordings into one clip and seeks transport streams exactly; camera, reel and
  timecode in Media Properties; an MXF DNxHD render template; camera RAW formats import offline with the reason (R41).
- **Multichannel audio:** mono, stereo, 5.1 and 7.1 tracks and busses (Track > Channel Layout), a Stereo, 5.1 or
  7.1 master bus (Project Properties > Audio, monitored as its stereo fold-down), Resample and stretch quality, and
  5.1 / 7.1 renders (WAV, FLAC, AAC, AC-3, E-AC-3); loudness measured with the BS.1770 channel weights (R10a).
- **Surround Panner** (View > Window) with keyframes, a surround pan control in the track headers, per-channel
  meters in the Mixing Console, Channels > pair selection for multichannel streams, Stream > Add All Streams as
  Tracks, and a note on dropping media with several audio streams (R10b).
- **Subtitles:** Insert > Subtitles from File (SRT, WebVTT, ASS with its styles) and from Regions as Titles & Text
  events, File > Export > Subtitles (SRT, WebVTT), and Render As burning them in, writing them beside the render or
  as a soft stream (MP4, MOV, MKV, WebM). CEA-608/708 closed captions are not feasible with the bundled FFmpeg (R39).
- **Music-video FX (2001-2009):** 32 video FX and 19 transitions after the looks, lens tricks, temporal effects and
  cuts of emo and hip-hop videos of the time (researched in `docs/research/MUSIC_VIDEO_2000S.md`), each on the CPU
  and the GPU with the same picture; VEGAS's built-ins of the same purpose import onto them with their parameters.
- **FX Packages:** the Video FX window's Filter Packages chip lists 15 built-in looks, applied in one step by drag or
  double-click; Save As... in the FX window's chain bar saves your own chain as a package.

### Changed
- Help > What's This?, Show Me How and Interactive Tutorials are no longer marked as waiting for a batch: Milan Pro
  answers them with hover text and the documentation instead (R53).

## 0.9.0

### Changed
- New branding in the style of 2002-2006 Windows software: app icon, startup splash (now with the version number),
  About banner and installer art, rendered from `tools/brand`.
- One interface look: the VEGAS 16 and VEGAS 23 looks and the Interface Look preference are removed. Window layouts,
  toolbars and keyboard shortcuts carry over from whichever look was used.

### Removed
- The VEGAS 14 "Classic" layout command, the "timeline at bottom" option and the Space/F12 play setting (VEGAS 16
  look only).

## 0.8.0 (2026-11-26)

### Added
- **Interchange with other editors:** OpenTimelineIO import and export (lossless for Milan Pro's own projects), CMX
  3600 and VEGAS-style EDLs (R28); Final Cut Pro 7 XML for Premiere Pro and DaVinci Resolve, FCPXML 1.10, AAF through
  OpenTimelineIO's adapter, and an interchange report of what each format could not carry (R29).
- **VEGAS round trip:** VEGAS import with FX, transitions, generators, Track Motion and project settings (R25); sending
  a project back to VEGAS through its scripting API (R26); a research note on the `.veg` format (R27).
- **Project archive** (File > Export > Milan Pro Project Archive, optionally trimmed) and File > Import > Media from
  Project (R30).
- **GPU compositing** on wgpu/Vulkan, matching the CPU compositor's picture, with the video FX, transitions and
  generators ported to WGSL and a zero-copy preview (R21a-c).
- **Hardware acceleration table:** hardware decoders and encoders are tested at startup and chosen in Preferences
  (R22).
- **Dynamic RAM Preview, Selectively Prerender Video** with Clean Up, and a disk cache for decoded frames of heavy
  sources (R23).
- **Reliability:** project format versions with migrations, `.mnle.bak`, validation and Save Version (R42);
  configurable autosave and Recover Projects (R43); a file logger, crash reports with minidumps and a diagnostic
  report (R44).
- **Updates:** Stable and Preview channels, minisign verification (once the key exists), rollback to the previous
  version, and the Update preferences page (R46).
- **Command palette** over every command, plug-in and layout (R47).

### Fixed
- The GPU stack's per-frame driver warnings no longer fill the log (R44).

## 0.7.0 (2026-11-26)

### Added
- **VST3 and CLAP audio plug-ins** from the standard and per-user folders and folders added in Preferences > VST
  Effects, on events, tracks, busses and the master, in preview and render, with keyframes and automation. VST3
  editor windows (changes recorded as automation while the track records), `.vstpreset` files and program lists.
  Plug-in latency is compensated sample-exactly. Not yet: CLAP editor windows, mono or surround plug-ins; VST2 is not
  possible (no licence) and DirectX audio plug-ins were investigated and not pursued.
- **OFX:** push buttons (Save Preset, Export LUT and the like) run in the plug-in host and their changes are saved; the
  FX window follows the plug-in's pages, groups and dynamic labels; OFX transitions and generators render; 16-bit and
  half-float plug-ins; a red "!" badge on events whose plug-in failed; a hint for plug-ins whose input defaults to a log
  curve. A 1080p float round trip went from 31 ms to 10.5 ms.
- **Classic 2006 look**, the new default: brushed metal, 3D bevels, Tahoma 8 pt, Windows 2000 dialogs and a black LCD
  time display on the VEGAS 23 layout. New branding: icon, splash, About banner and installer art.

### Known limits
- VEGAS Pro's own OFX plug-ins do not run in Milan Pro or any other host: they need VEGAS's private suites and host
  key ([docs/compat/ofx-vegas15.md](docs/compat/ofx-vegas15.md)).

## 0.6.0 (2026-11-13)

### Added
- **OFX plug-ins** from `C:\Program Files\Common Files\OFX\Plugins`, `OFX_PLUGIN_PATH` and added folders, in the Video
  FX window under their own group, on events, tracks and the video output, with keyframes, in preview and render. The
  Plug-In Manager (View > Window, Ctrl+Alt+1) lists them with their status, turns them off and rescans. Plug-ins run in
  a separate process: a crash or hang bypasses the effect, and missing plug-ins keep their settings.
- **Overlays** window for overlay packs (light leaks, grain, dust, frames, keyed elements), imported from folders or
  `.zip` files and dropped on the timeline with the right blend.
- Smooth scrubbing, complete reverse playback, adaptive preview quality.
- Multicamera editing, nested timelines, Video Scopes, Loudness Meters, Edit Details, floating windows, customizable
  keyboard and toolbars, adjustment tracks, slideshows, Smart Split.
- App icon and a branded installer.

### Known limits
- Spektrafilm needs its input colour space set to sRGB or Rec.709 and a Vulkan 1.1 driver; OFX transitions,
  generators, push buttons and custom on-screen controls were not yet supported (transitions, generators and buttons
  came in 0.7.0).

## 0.5.0 (2026-11-13)

### Fixed
- Milan Pro runs on any Windows PC: 0.2.0-0.4.0 needed the Visual C++ runtime (VCRUNTIME140.dll); it is now built in.
- About 50 fixes from a full bug hunt: shortcuts on German and French keyboards, paste, ripple editing of envelopes
  and markers, transparent media edges, anamorphic footage, 60p slow motion, mono levels, audio dropouts and fader
  clicks, damaged MP3 renders, `:` in render names, MP3/AAC sample rates.
- Moved media is found again, Project Media's Replace works, and `.MTS`, `.M2TS`, `.m4a`, `.3gp` and `.tif` import.

### Added
- Timeline: grid snapping, the 15 VEGAS ruler formats, Go To Time, exact 30000/1001 timing.
- Editing: edge trimming (`[` / `]`), Expanded Edit, Paste Repeat, nudges, selection history, event looping, empty
  events, subclips, Paste Event Attributes.
- Speed: velocity envelopes, Freeze Frame at Cursor, audible J/L shuttle, pitch shift, frame resampling (blend and
  optical flow).
- Compositing: all VEGAS compositing modes, parent/child compositing, Bezier masks, Exposure, 2D shadow and glow, 3D
  Track Motion, motion blur, a video bus track.
- Tracks: groups with group mute and solo, hidden tracks, Fade to Color and Mute envelopes.
- Audio: busses, assignable FX chains, sends, the six VEGAS pan types, the Mixing Console with live meters, automation
  (Off, Read, Touch, Latch), FX parameter envelopes, input busses and monitoring, a choice of audio device and buffer.
- Help > Unfinished Features lists everything still being built; every greyed control says which update adds it.

## 0.4.0 (2026-11-12)

### Added
- Video FX (Brightness and Contrast, Color Corrector, Levels, HSL Adjust, Black and White, Sepia, Invert, Gaussian
  Blur, Sharpen, Vignette, Mirror, Crop, Border, Chroma Keyer) on events, tracks and the video output.
- Transitions (Crossfade, Dip to Black/White, Linear Wipe, Push, Slide, Iris, Clock Wipe, Zoom).
- Media generators (Solid Color, Color Gradient, Checkerboard, Noise Texture, Titles & Text).
- Color Grading: lift/gamma/gain wheels, curves and `.cube` LUTs.
- The FX window with presets, parameter sections and keyframes.
- Audio FX (parametric EQ, compressor, noise gate, reverb, delay) on events, tracks and the master; Set Default Track
  Properties.
- Audio recording into armed tracks.
- Updater: old downloads are cleaned up and Setup waits for Milan Pro to close.

## 0.3.0 (2026-11-12)

### Added
- The real editing engine: all video tracks composited in real time with persistent decoders and frame caches.
- Drag to the timeline from Project Media, the Explorer, the Trimmer or Windows File Explorer, with a ghost of each
  event.
- Editing: multi-select and rubber band, track insert/delete/reorder, grouping, automatic crossfades, Auto Ripple and
  Post-Edit Ripple, slip/slide/roll/stretch, playback rate and reverse, event mute and level, quantize to frames.
- Volume, pan, composite level and mute envelopes; takes; Event Properties; Event Pan/Crop and Track Motion with
  keyframes.
- Window layouts per look with Alt+D shortcuts; video proxies; import from VEGAS Pro through an export script.
- A streaming audio mixer: editing during playback, seamless loops, recovery when the audio device changes. Render
  through the preview's engine.

### Fixed
- Save errors are shown and saves are atomic; imports land in the right project after New/Open; FFmpeg processes
  cannot outlive Milan Pro.

## 0.2.0 (2026-11-12)

First public build.

### Added
- VEGAS 16 and VEGAS 23 (dark and white) looks with VEGAS menus, keyboard shortcuts, context menus and the transport
  bar.
- Multitrack timeline editing: move, trim, split, snapping, fades, markers, regions, loop region, cut/copy/paste,
  undo/redo.
- Video preview from Draft to Best quality; audio playback with waveforms and meters.
- Render As through FFmpeg with progress and cancel.
- Autosave every 2 minutes and safe saving.
- The auto-updater: checks GitHub, verifies the download's SHA-256, installs and restarts.
- The installer bundles FFmpeg 9.0.2 (GPL v3; source links in `FFMPEG-SOURCE.txt`).
