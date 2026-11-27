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

## AE-format (.aex) effect hosting: the clean-room rule

Milan Pro can host effect plug-ins written for the Adobe After Effects effect API (`docs/AE_PREMIERE_PLUGINS.md`).
On 2026-11-26 the project owner approved building this with **Milan's own bindings only**. For every change that
touches `src/bin/milan-plugin-host/aex/`, `src/engine/aex/`, `tools/aex-test-plugin/` or
`docs/compat/aex-provenance.md`:

1. **No Adobe SDK material, of any kind, in the repository, the build or a release**: no SDK header, sample, shipped
   document, PiPL tool or resource script, no translation or port of any of them (to Rust or anything else), nothing
   generated from SDK headers, and no dependency on the `after-effects`, `after-effects-sys` or `premiere-sys` crates.
   `cargo test` fails when a file carries the SDK's copyright line, its confidentiality banner or the name of its main
   effect header (`tests/no_adobe_material.rs`).
2. **Every ABI item has a provenance entry** in `docs/compat/aex-provenance.md`: the public documentation page it comes
   from, or how it was observed (with `tools/aex-test-plugin` or an open-source plug-in run by its authors' build).
   An item with no public source is marked as such and stays UNVERIFIED until an open-source plug-in proves it.
3. **Say in the pull request** that you have not accepted the Adobe Developer Terms of Use and did not consult the
   Adobe SDK (headers, samples or its shipped documentation) while writing the change. Someone who has must not edit
   `src/bin/milan-plugin-host/aex/abi.rs`; they may build SDK samples on their own machine to cross-check, and report
   only pass/fail results, never code or numbers copied from the SDK.
4. **No plug-in binaries in the tree.** Open-source test plug-ins (`docs/AE_PREMIERE_PLUGINS.md` §3.2) are found
   through `MILAN_AEX_TEST_DIR` by `#[ignore]` tests; never vendor them (some of their repositories contain SDK files).
5. **No circumvention**: Milan reports itself as a third-party host (`'PrMr'`), keeps the serial number 0, never
   patches, hooks or fakes a licence check, and blocks Adobe's own effects (match names starting `ADBE `, files under
   an Adobe application folder).
6. **Trademarks, nominative use only.** Say "AE-format effect" or "effect plug-ins written for the Adobe After Effects
   effect API"; never "After Effects compatible" or "Adobe compatible", no Adobe logos or product icons. Where the
   feature is described, carry the notice: "Adobe, After Effects and Premiere Pro are trademarks of Adobe Inc. Milan
   Pro is not affiliated with, endorsed or sponsored by Adobe."

## Licence of this document

This file is part of Milan Pro and MIT-licensed like the rest of the repository.
