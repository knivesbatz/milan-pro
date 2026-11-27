## Notes

Everything below is a description of what Milan Pro ships and under which terms, written by the project. It is not
legal advice. Anyone who distributes Milan Pro, or products made with it, is responsible for checking that these
terms and the patent position in their country fit what they do.

### Milan Pro

Milan Pro's own code, the three test plug-ins in `tools/` (OFX, VST3, CLAP; each with its own `Cargo.lock`, whose
only direct dependencies are `clap-sys` and `vst3`, listed below) and the VEGAS scripts in `tools/vegas/` are MIT
(`LICENSE`). Contributions are accepted under the same terms (`CONTRIBUTING.md`). "VEGAS" is a trademark of its
owner; Milan Pro is not affiliated with or endorsed by VEGAS Creative Software or Sony.

### How the acceptance check works

`cargo deny` is not part of the toolchain this project pins (it is a separate install), so the licence check is
`tools/licenses.ps1 -Check`: it reads every crate's `Cargo.toml` from the local registry, compares every licence
expression with the permissive list in the script, and fails when the committed `docs/LICENSES.md` or
`THIRD-PARTY-NOTICES.txt` is stale. A crate with a licence outside that list shows up in the Flagged section above
and is a review item, not a build break. The script also flags git and path dependencies and crates missing from the
local registry (run `cargo build` first). `Cargo.lock` lists the crates of every target platform (macOS, Linux and
Wayland crates among them), so the tables are a superset of what the Windows build compiles.

`THIRD-PARTY-NOTICES.txt` adds, after these notes, the licence files of the crates themselves (for a choice of
licences the MIT file when MIT is offered, else the Apache-2.0 one, and every NOTICE file), each distinct text once
with the crates that carry it, so the copyright and permission notices travel with the binaries.

### FFmpeg (GPL v3)

Milan Pro decodes, encodes and probes media by running `ffmpeg.exe` and `ffprobe.exe` as separate processes
(`src/decode.rs`, `src/render.rs`, `src/media.rs`); it never links FFmpeg's libraries. The installer bundles the
build pinned in `installer/fetch-ffmpeg.ps1`: **FFmpeg 9.0.2, `ffmpeg-9.0.2-full_build-shared`, built by Gyan
Doshi** (https://github.com/GyanD/codexffmpeg/releases/tag/9.0.2). The build's own `README.txt` (installed as
`FFMPEG-README.txt`) states "License: GPL v3", and `fetch-ffmpeg.ps1` writes the same into `FFMPEG-SOURCE.txt`: the
binaries are **GPL v3**. Among the external libraries that README lists are **libx264** and **libx265** (both GPL) and
the other GPL components of Gyan's full build. Milan Pro treats this as aggregation: the MIT program and the GPL programs
sit side by side in one installer and talk through pipes, and the GPL applies to FFmpeg and its source, not to Milan
Pro. The install folder carries `FFMPEG-LICENSE.txt` and `FFMPEG-README.txt` from the build and `FFMPEG-SOURCE.txt`,
which gives the FFmpeg source tarball (https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz) and the build's source
repository, as the GPL's written offer. The build contains **no libfdk-aac** (a "nonfree" component Gyan's builds do
not include; AAC comes from FFmpeg's native `aac` encoder, see the encoder list in `docs/REMAINING.md` section 4.3).
Anyone who replaces the bundled FFmpeg with their own build takes on that build's terms.

### Codec patents

FFmpeg implements H.264/AVC, H.265/HEVC, AAC, AC-3 and E-AC-3, MPEG-2, DTS and others. Some of these are covered by
patents in some countries, and patent pools (for example for H.264, HEVC and AAC) licence encoders and decoders.
Milan Pro and its contributors hold no codec patent licences and grant none; the MIT licence covers copyright only.
Whether a given use of these codecs needs a licence depends on the country, the use (personal, commercial,
distribution) and the volume, and is the responsibility of the person distributing or using the software. The
built-in render templates (`docs/FORMAT_SUPPORT.md`) use H.264, HEVC, AAC, AV1, ProRes, DNxHD, FLAC, MP3 and PCM;
DTS encoding is FFmpeg's experimental core-only `dca` encoder and is not offered by a template. Plain-text summary:
Milan Pro gives you the tools; the codec licensing position is yours to check.

### OpenFX (BSD 3-Clause)

`src/bin/milan-plugin-host/ofx.rs` holds hand-written Rust `#[repr(C)]` declarations of the parts of the OpenFX 1.4 C
API that Milan Pro hosts. The names, constants and string values come from the OpenFX headers (`ofxCore.h`,
`ofxProperty.h`, `ofxImageEffect.h`, `ofxParam.h` and others), Copyright (c) 2003-2015 The Open Effects Association
Ltd, under the BSD 3-Clause licence. No header file is vendored; the attribution is in that file and in `README.md`.

### VST 3

VST 3 plug-ins are hosted through the `vst3` crate (coupler-rs, 0.3.0, **MIT OR Apache-2.0**), which ships Rust
bindings pre-generated from the VST 3 API headers (`src/bindings.rs` in the crate; the build runs no generator and
needs no SDK). Before version 3.8, Steinberg licensed the VST 3 SDK under its proprietary VST 3 licence or the GPL v3;
Steinberg announced the move of SDK 3.8 to the **MIT licence** in 2025, which `README.md` repeats. This review did not
download Steinberg's licence text (the `vst3` crate's changelog only records that 0.3.0 builds against SDK 3.8.0), so
that statement is **to verify** against Steinberg's own licence file before it is relied on. Either way, Milan Pro
ships no code from the Steinberg SDK: no SDK sources, headers or binaries are in this repository or the installer,
only the crate's bindings. "VST" is a trademark of
Steinberg Media Technologies GmbH. **VST 2** is not supported, as Steinberg no longer licenses that SDK.

### CLAP (MIT)

CLAP plug-ins are hosted through the `clap-sys` crate (0.6.0, MIT/Apache-2.0), the Rust translation of the CLAP C
headers; the CLAP specification itself is MIT.

### ASIO (not used)

The Steinberg ASIO SDK may not be redistributed, so Milan Pro does not use it: audio runs through WASAPI, via `cpal`
(Apache-2.0, its `asio` feature off: `Cargo.toml` enables none) and the project's own WASAPI exclusive-mode output
(`src/wasapi.rs`). Preferences offer no ASIO device type (`docs/COMPATIBILITY.md`, Audio devices).

### Camera RAW SDKs (not used)

RED R3D, Blackmagic RAW and ARRIRAW decoding need proprietary SDKs whose licences do not allow redistribution with an
MIT project, and FFmpeg has no decoders for them. Milan Pro ships none and reads none of those formats
(`docs/REMAINING.md`, requirement 7.19 and section 6).

### VEGAS's bundled plug-ins (not shipped)

VEGAS Pro's OFX, DirectX audio and FileIO plug-ins are licensed to VEGAS's users with VEGAS. Milan Pro ships none of
them and copies nothing from them. A user who adds VEGAS's OFX folder in the Plug-In Manager has them tried in the
plug-in host under their own VEGAS licence; they do not run outside VEGAS, which they check for
(`docs/compat/ofx-vegas15.md`).

### Contributor Covenant (CC BY 4.0)

`CODE_OF_CONDUCT.md` is the Contributor Covenant, version 2.1 (https://www.contributor-covenant.org), used under the
Creative Commons Attribution 4.0 licence, with the attribution the Covenant asks for kept in the file.

### Fonts

Milan Pro bundles no font files of its own. The interface uses Tahoma, with Segoe UI as its fallback, both
read at run time from the Windows fonts folder on the user's machine (`src/ui/fonts.rs`); they are not redistributed.
When a face is missing, egui's own bundled fonts stand in: these ship inside the `epaint_default_fonts` crate
(licence expression `(MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0`), namely Ubuntu-Light (Ubuntu Font Licence
1.0), NotoEmoji-Regular (SIL Open Font Licence 1.1), Hack-Regular (MIT, with Bitstream Vera and DejaVu notices) and
emoji-icon-font (MIT, John Slegers). Their licence texts are in that crate's `fonts/` folder.

### TLS roots (`webpki-roots`, CDLA-Permissive-2.0)

The updater's HTTPS client (`ureq` with `rustls`) trusts the Mozilla CA bundle compiled into the `webpki-roots` crate,
which is distributed under the Community Data License Agreement - Permissive 2.0. `rustls` itself is Apache-2.0 OR
ISC OR MIT.

### Inno Setup

The Windows installer is built with Inno Setup 6 (Jordan Russell, Martijn Laan), under the Inno Setup licence: a
permissive licence that allows commercial use and redistribution of the generated installers while asking that the
copyright notices and web addresses in the installer itself stay in place. Milan Pro's installer changes none of
them. Inno Setup is a build tool: none of its source is in this repository, and the generated `Setup.exe` is the only
part users receive.

### What the installer ships

`MilanPro.exe` and `milan-plugin-host.exe` (MIT, built from this repository with the crates above), `LICENSE.txt`
(this project's MIT licence), `THIRD-PARTY-NOTICES.txt` (this file's plain-text form with the licence texts), the FFmpeg programs, DLLs and
their licence, readme and source files, and the VEGAS export scripts. Nothing else.
