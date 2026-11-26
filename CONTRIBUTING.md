# Contributing to Milan Pro

Thank you for helping. Milan Pro is an open-source video editor modelled on VEGAS Pro's workflow; bug reports,
plug-in and format compatibility reports, documentation and code are all welcome. Everyone taking part follows the
[Code of Conduct](CODE_OF_CONDUCT.md).

## Reporting

- **Bugs:** open an issue with the *Bug report* form. Say which version (Help > About), your Windows build and GPU, and
  the steps. A diagnostic report (Help > Create Diagnostic Report) helps most; read it before you attach it
  (Preferences > Update can leave file names and paths out of it).
- **Plug-ins and media files:** the *Plug-in compatibility* and *Media format* forms ask for exactly what is needed to
  reproduce a result (plug-in name, vendor, format, version; or the file's `ffprobe` output).
- **Features:** the *Feature request* form. Say what VEGAS (or another editor) does, if it does, and what you need.
- **Security problems:** not in a public issue. See [SECURITY.md](SECURITY.md).

## Building and testing

[docs/DEVELOPING.md](docs/DEVELOPING.md) has the toolchain, the build, the tests (including the ignored ones that need
FFmpeg or a GPU), the screenshot harness and how to add commands, dialogs and plug-ins.
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) explains how the parts fit.

## How work is organised

Work is planned as **batches** in [docs/REMAINING.md](docs/REMAINING.md) (section 5): each has a scope, the files it
touches and acceptance tests. Pick an open batch, or open an issue first for anything bigger than a fix, so two people
do not build the same thing.

The batch rules (from [docs/FEATURE_PLAN.md](docs/FEATURE_PLAN.md) and `REMAINING.md` section 5.0):

- **One batch, from start to finish.** A batch ends with `cargo build --release` free of warnings, `cargo test` green,
  the feature wired into the UI, and nothing half-wired. Its status and evidence go into `REMAINING.md`.
- **No silent placeholders.** A control that does not work yet is greyed and says on hover which batch builds it
  (`menus::unfinished` or `X(label, Later("R.."))`). The batch that builds the feature removes that label.
- **Persistent data:** every new field in the project or preferences carries `#[serde(default)]`; a project format
  change bumps `project::FORMAT_VERSION` and adds a migration step with a test.
- **Child processes** go through `proc::spawn` / `proc::output` (no console window, killed with Milan Pro).
- **Known limits** are written where they are, as `// shortcut: <limit>, <when to upgrade>`, so they can be found
  with a grep.
- **New crates** only when nothing in `Cargo.lock` does the job, with a permissive licence (MIT, Apache-2.0, BSD, ISC,
  Zlib, MPL-2.0 and the like), named in the commit message. Run `tools/licenses.ps1` and commit its output.
- **Plug-in code never runs in `milan-pro.exe`**: it runs in `milan-plugin-host.exe`.

## Branches, commits and pull requests

- Branch from `main`; one branch per batch or fix (`wip-<topic>` is the habit).
- Commit subjects start with the batch: `R53: architecture and developer guide`. Fixes outside a batch say what they
  fix: `Fix: ...`. Write the body in plain sentences: what changed and why, and what was tested.
- Keep the existing style: lines up to about 150 columns (there is no `rustfmt.toml` yet; R51 adds one), comments that explain why, names that follow VEGAS's where a VEGAS concept
  exists. No reformatting of code you did not change.
- **Tests are required.** New behaviour comes with a unit test; anything that needs FFmpeg, a GPU, the plug-in host or
  a long run is an `#[ignore]` test that you have run (say so in the pull request). UI changes come with harness
  screenshots of the windows they affect.
- Documentation changes keep every relative link working (`cargo test --release --test doc_links`).
- Fill in the pull request template. A maintainer merges into `main`.

## What may go into the repository

- **Your own work, under the MIT licence.** By contributing you agree that your contribution is licensed under the
  project's [MIT licence](LICENSE), and you confirm that you have the right to contribute it.
- **No VEGAS material.** No screenshots, icons, logos, bitmaps, help text or other copied text from VEGAS Pro or its
  plug-ins. Describe VEGAS behaviour in your own words; measure it, do not copy it.
- **No proprietary SDK material.** No headers, samples or code from SDKs whose licences forbid redistribution or
  translation (the ASIO SDK, camera RAW SDKs, Adobe's SDKs). Bindings to C APIs are written by hand from public
  documentation, with a note of where each item comes from.
- **Third-party code** with a compatible licence is fine when it keeps its licence notice and is listed in
  [docs/LICENSES.md](docs/LICENSES.md).
- No binaries, media files over a few hundred kilobytes, or generated output other than the documented generated files
  (`docs/LICENSES.md`, `THIRD-PARTY-NOTICES.txt`, `docs/PERFORMANCE.md`, the screenshots in `docs/images/`).

## AE-format plug-ins: no Adobe SDK material

Milan Pro's After Effects / Premiere Pro effect plug-in hosting ([docs/AE_PREMIERE_PLUGINS.md](docs/AE_PREMIERE_PLUGINS.md))
is built without the Adobe After Effects or Premiere Pro SDKs, whose terms forbid redistributing, porting or
translating them.

- **No Adobe headers, samples, shipped documentation, PiPL tools or copyright strings in the tree**, and no
  translation of any of them. No dependency on crates that vendor or generate from them (`after-effects-sys`,
  `premiere-sys`).
- **The AE ABI bindings are hand-written** from public documentation and from behaviour observed with Milan Pro's own
  test plug-in. Each item carries a provenance note (the public page it comes from, or "observed with
  tools/aex-test-plugin").
- **Contributors to the AE ABI bindings confirm in their pull request** that they have not accepted the Adobe
  Developer Terms (or the SDK's licence) and did not consult the Adobe SDK while writing the code.
- A CI check (added with batch A0 of that plan) fails on Adobe SDK copyright strings and header names in the tree.
- Milan Pro ships no AE-format plug-ins, downloads none and does not work around a plug-in's own licensing: a plug-in
  that refuses the host is reported as refused.

## Licence of this document

This file is part of Milan Pro and MIT-licensed like the rest of the repository.
