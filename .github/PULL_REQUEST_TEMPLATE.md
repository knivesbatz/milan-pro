<!-- Thank you. CONTRIBUTING.md has the rules this checklist refers to. -->

## What and why

<!-- What this changes, and why. Link the issue or the batch in docs/REMAINING.md (for example R39). -->

## How it was tested

<!-- The tests you added or ran. Name any #[ignore] tests you ran (they need FFmpeg, a GPU or the plug-in host). -->

## Screenshots

<!-- For UI changes: harness screenshots of the windows the change affects. -->

## Checklist

- [ ] `cargo build --release` has no warnings and `cargo test --release` passes.
- [ ] New behaviour has tests; heavy tests are `#[ignore]` and I ran them.
- [ ] Controls that do not work yet are greyed with a reason; the labels for what this builds are removed.
- [ ] New persistent fields have `#[serde(default)]`; a project format change bumps `FORMAT_VERSION` with a migration test.
- [ ] `docs/REMAINING.md` and the other docs are updated; relative links still resolve.
- [ ] New crates, if any, are permissively licensed, and `tools/licenses.ps1` was run.
- [ ] Nothing is copied from VEGAS Pro, its plug-ins or proprietary SDKs; the code is mine to license under MIT.
- [ ] If this touches the After Effects / Premiere ABI bindings: I have not accepted the Adobe Developer Terms and did not consult the Adobe SDK.
