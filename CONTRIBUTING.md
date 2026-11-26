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
