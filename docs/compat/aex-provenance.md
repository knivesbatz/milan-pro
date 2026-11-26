# AE-format effect host: provenance of every ABI item

Milan Pro's host for effect plug-ins written for the Adobe After Effects effect API uses its own hand-written
`#[repr(C)]` declarations (`src/bin/milan-plugin-host/aex/abi.rs`). No Adobe SDK file, sample, PiPL tool or anything
generated from SDK headers was used or is in the tree (owner's decision of 2026-11-26, `docs/AE_PREMIERE_PLUGINS.md`
§2.6; rule in `CONTRIBUTING.md`; enforced by `tests/no_adobe_material.rs`). This file says where each item comes from.

Adobe, After Effects and Premiere Pro are trademarks of Adobe Inc. Milan Pro is not affiliated with, endorsed or
sponsored by Adobe.

## Source classes

| Class | Meaning |
|---|---|
| **G** | Stated on a public page of the After Effects C++ SDK Guide as hosted at ae-plugins.docsforadobe.dev, or of the Premiere Pro guide at ppro-plugins.docsforadobe.dev (URL given). Names, field order, prototypes. |
| **G-inf** | Inferred from such a page, with the reasoning given (for example a flag's bit from its position in a table that lists every bit, reserved ones included). |
| **P** | A permissively licensed, hand-written third-party description of a data format (the PiPL property layout of the `pipl` crate's documentation, MIT OR Apache-2.0, cited by the plan as [S17]); facts only, no text copied. |
| **MS** | Microsoft's public documentation of Windows (PE resource directory, `.res` file format, Win64 calling convention). |
| **R** | Not on any public page found. The value is the implementer's own understanding, written without the SDK open; it is **UNVERIFIED** and must be confirmed by observation of an open-source plug-in (plan §2.4 (3)) before any plug-in is called supported. Every R item is listed below so that it can be reviewed or removed as one set. |
| **M** | Milan's own choice (a host policy, not an interface fact). |

Caveat that applies to all of it: `tools/aex-test-plugin` is built on these same declarations, so its passing tests
prove the host is **self-consistent**, not that it matches the API. Only an open-source plug-in built by its own
authors (plan §3.2, found through `MILAN_AEX_TEST_DIR`) can prove the layout; none was available when this was
written, so those tests are `#[ignore]` and unverified.

## Pages consulted (all read 2026-11-26)

- PF_InData: https://ae-plugins.docsforadobe.dev/effect-basics/PF_InData/
- PF_OutData: https://ae-plugins.docsforadobe.dev/effect-basics/PF_OutData/
- Entry point: https://ae-plugins.docsforadobe.dev/effect-basics/entry-point/
- Command selectors: https://ae-plugins.docsforadobe.dev/effect-basics/command-selectors/
- PF_EffectWorld: https://ae-plugins.docsforadobe.dev/effect-basics/PF_EffectWorld/
- PF_ParamDef: https://ae-plugins.docsforadobe.dev/effect-basics/PF_ParamDef/
- Parameters: https://ae-plugins.docsforadobe.dev/effect-basics/parameters/
- Errors: https://ae-plugins.docsforadobe.dev/effect-basics/errors/
- Interaction callbacks: https://ae-plugins.docsforadobe.dev/effect-details/interaction-callback-functions/
- Accessing function suites: https://ae-plugins.docsforadobe.dev/effect-details/accessing-function-suites/
- Iteration suites: https://ae-plugins.docsforadobe.dev/effect-details/iteration-suites/
- Memory allocation: https://ae-plugins.docsforadobe.dev/effect-details/memory-allocation/
- Image buffer management: https://ae-plugins.docsforadobe.dev/effect-details/image-buffer-management-functions/
- Graphics utility suites: https://ae-plugins.docsforadobe.dev/effect-details/graphics-utility-suites/
- Parameters and floating point values: https://ae-plugins.docsforadobe.dev/effect-details/parameters-floating-point-values/
- Parameter supervision: https://ae-plugins.docsforadobe.dev/effect-details/parameter-supervision/
- Arbitrary data parameters: https://ae-plugins.docsforadobe.dev/effect-details/arbitrary-data-parameters/
- PiPL resources: https://ae-plugins.docsforadobe.dev/intro/pipl-resources/
- Premiere: bigger differences: https://ae-plugins.docsforadobe.dev/ppro/bigger-differences/
- Premiere: AE transition extensions and PF_TransitionSuite: https://ppro-plugins.docsforadobe.dev/ae-transition-extensions/ae-transition-extensions/ , https://ppro-plugins.docsforadobe.dev/ae-transition-extensions/PF_TransitionSuite/
- Premiere: pixel formats: https://ppro-plugins.docsforadobe.dev/universals/pixel-formats-and-color-spaces/ ; SweetPea suites: https://ppro-plugins.docsforadobe.dev/universals/sweetpea-suites/

Not found on any public page (and therefore not built, or built as class R as said per item): the function order of
`in_data->utils`; suite name strings; numeric values of selectors, parameter types, flags and error codes; the
members of the per-type parameter definitions; the function tables of the Premiere PF Pixel Format, PF Utility and
PF Transition suites; how Premiere passes transition progress.

## Items

The table is filled in batch by batch (A1 PiPL, A2 structures and selectors, A3 suites).
