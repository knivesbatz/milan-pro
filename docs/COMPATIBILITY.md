# Milan Pro compatibility

What Milan Pro has been checked against: the VEGAS versions and scripts, every plug-in of the VEGAS Pro 15 install
it was built beside, the plug-ins and plug-in interfaces it hosts, the example VEGAS projects, the VEGAS behaviours
still to confirm, then audio devices, interchange formats and camera formats.

## Tested versions

| What | Version | How it was checked |
|---|---|---|
| VEGAS Pro 15.0 | Build 177 (file version 15.0.0.177) | Read from the version resource of the installed `vegas150.exe`, read-only. VEGAS was never started by the developers: its OFX bundles were run in Milan Pro's own plug-in host (R33c), its plug-in folders and resource files were read in place, and the scripts were compiled against its `ScriptPortal.Vegas.dll` (15.0.0.177). |
| VEGAS Pro 13 and earlier | Not tested | The `(VEGAS 13)` copies of the scripts differ only in their `using Sony.Vegas;` line; no VEGAS 13 was available. |
| VEGAS Pro 16 to 23 | Not tested | Only VEGAS Pro 15 is installed here. VEGAS 16+ projects that use plug-ins VEGAS 15 lacks import those as placeholders. |
| `tools/vegas/Export to Milan Pro.cs` | Writes format `milan-vegas` version 2 (R25, commit `c80bf72`) | Compiled against VEGAS 15's `ScriptPortal.Vegas.dll`; **not run inside VEGAS**. The importer reads versions 1 and 2. |
| `tools/vegas/Import from Milan Pro.cs` | Reads format `milan-vegas` version 2 (R26, commit `ca1dd61`) | Compiled against VEGAS 15's `ScriptPortal.Vegas.dll`; **not run inside VEGAS**. |
| Milan Pro | 0.10.0 | The matrix below and the example projects were checked with this version (`cargo test --release`). |

The acceptance test of both scripts is a pass by the owner in VEGAS 15: export each kind of project in
[Example VEGAS projects](#example-vegas-projects) from VEGAS and compare the import with the example's expected file.

## Plug-in matrix

<!-- compat-matrix:begin: written by interchange::compat_matrix from docs/compat/*.csv, vegas_map and tests/vegas; do not edit by hand -->

Written from `docs/compat/vegas15-inventory.csv`, `docs/compat/ofx-vegas15.csv`, `docs/compat/tested-plugins.csv`, `src/interchange/vegas_map.rs` and `tests/vegas/` by `interchange::compat_matrix`. `cargo test --release compat_matrix` fails when this section is stale or a row claims support without a test that exists; `MILAN_WRITE_COMPAT=1 cargo test --release compat_matrix` rewrites it.

Each row has exactly one status, in its own column:

- **Native equivalent**: a VEGAS plug-in the VEGAS import turns into a Milan Pro built-in with its settings (the `vegas_map` row).
- **Standard interface**: the plug-in runs in Milan Pro's own OFX, VST3, CLAP or AE-format host.
- **Adapter**: the VEGAS import replaces it by name with a Milan Pro built-in, without its settings.
- **Unsupported**: it does not run in Milan Pro; the reason is given.
- **Needs investigation**: what it does in Milan Pro is not known yet; the open question is given.

| Status | VEGAS Pro 15 OFX plug-ins | Named in VEGAS 15's Vfx1.xml but not in its binary | VEGAS DirectX audio plug-ins | VEGAS legacy video plug-ins | Third-party and test plug-ins, and interfaces | All |
|---|---|---|---|---|---|---|
| Native equivalent | 43 | 13 | 0 | 0 | 0 | 56 |
| Standard interface | 0 | 0 | 0 | 0 | 24 | 24 |
| Adapter | 0 | 0 | 16 | 0 | 0 | 16 |
| Unsupported | 36 | 16 | 22 | 7 | 2 | 83 |
| Needs investigation | 0 | 0 | 1 | 0 | 3 | 4 |
| **Rows** | 79 | 29 | 39 | 7 | 29 | 183 |

#### VEGAS Pro 15 OFX plug-ins (installed bundles) (79)

One row per plug-in the 7 bundles export (`docs/compat/ofx-vegas15.csv`). None of them runs in Milan Pro (R33c): a row is Native when `vegas_map` turns the VEGAS plug-in into a Milan built-in on import. The probe results are its second pass, with VEGAS's program folder on the DLL path (`Vfx1.ofx`, `Filters.ofx` and `Stabilize.ofx` do not load at all without it). Indexes the probe did not identify are matched to identifiers by the order of the identifier strings in the binary, which agrees with all 8 it did identify.

| Plug-in | Where | Native equivalent | Standard interface | Adapter | Unsupported (reason) | Needs investigation | Tests |
|---|---|---|---|---|---|---|---|
| VEGAS Checkerboard (`checkerboard`) | `Vfx1.ofx` #0 | `milan.gen.checkerboard` (Checkerboard); vegas_map row `checkerboard` → `milan.gen.checkerboard`, 9 settings carried; checked in `tests/vegas/generated-media.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Noise Texture (`noisetexture`) | `Vfx1.ofx` #1 | `milan.gen.noise` (Noise Texture); vegas_map row `noisetexture` → `milan.gen.noise`, 9 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Solid Color (`solidcolor`) | `Vfx1.ofx` #2 | `milan.gen.solid` (Solid Color); vegas_map row `solidcolor` → `milan.gen.solid`, 4 settings carried; checked in `tests/vegas/generated-media.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Add Noise (`addnoise`) | `Vfx1.ofx` #3 | `milan.fx.add_noise` (Add Noise); vegas_map row `addnoise` → `milan.fx.add_noise`, 4 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Black and White (`blackandwhite`) | `Vfx1.ofx` #4 | `milan.fx.black_white` (Black and White); vegas_map row `blackandwhite` → `milan.fx.black_white`, 1 setting carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Brightness and Contrast (`brightnessandcontrast`) | `Vfx1.ofx` #5 | `milan.fx.brightness_contrast` (Brightness and Contrast); vegas_map row `brightnessandcontrast` → `milan.fx.brightness_contrast`, 3 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Broadcast Colors (`broadcastcolors`) | `Vfx1.ofx` #6 |  |  |  | Runs only in VEGAS: it asks for VEGAS's private suites (`OfxVegas*`) and host key (R33c probe: load yes, describe no, render no). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings |  |  |
| VEGAS Bump Map (`bumpmapfilter`) | `Vfx1.ofx` #7 |  |  |  | Runs only in VEGAS: it asks for VEGAS's private suites (`OfxVegas*`) and host key (R33c probe: load yes, describe no, render no). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings |  |  |
| VEGAS Chroma Keyer (`chromakeyer`) | `Vfx1.ofx` #8 | `milan.fx.chroma_key` (Chroma Keyer); vegas_map row `chromakeyer` → `milan.fx.chroma_key`, 7 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Color Balance (`colorbalance`) | `Vfx1.ofx` #9 |  |  |  | Runs only in VEGAS: it asks for VEGAS's private suites (`OfxVegas*`) and host key (R33c probe: load yes, describe no, render no). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings |  |  |
| VEGAS Color Corrector (`colorcorrector`) | `Vfx1.ofx` #10 | `milan.fx.color_corrector` (Color Corrector); vegas_map row `colorcorrector` → `milan.fx.color_corrector`, 4 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Color Corrector (Secondary) (`colorcorrectorsecondary`) | `Vfx1.ofx` #11 |  |  |  | Runs only in VEGAS: it asks for VEGAS's private suites (`OfxVegas*`) and host key (R33c probe: load yes, describe no, render no). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings |  |  |
| VEGAS Color Curves (`colorcurves`) | `Vfx1.ofx` #12 |  |  |  | Runs only in VEGAS: it asks for VEGAS's private suites (`OfxVegas*`) and host key (R33c probe: load yes, describe no, render no). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings |  |  |
| VEGAS Color Match (`colormatch`) | `Vfx1.ofx` #13 |  |  |  | Runs only in VEGAS: it asks for VEGAS's private suites (`OfxVegas*`) and host key (R33c probe: load yes, describe no, render no). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings |  |  |
| VEGAS Convolution Kernel (`convolutionkernel`) | `Vfx1.ofx` #14 |  |  |  | Runs only in VEGAS: it asks for VEGAS's private suites (`OfxVegas*`) and host key (R33c probe: load yes, describe no, render no). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings |  |  |
| VEGAS Cookie Cutter (`cookiecutter`) | `Vfx1.ofx` #15 | `milan.fx.cookie_cutter` (Cookie Cutter); vegas_map row `cookiecutter` → `milan.fx.cookie_cutter`, 10 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Defocus (`defocusvelvetmatter`) | `Vfx1.ofx` #16 |  |  |  | Runs only in VEGAS: it asks for VEGAS's private suites (`OfxVegas*`) and host key (R33c probe: load yes, describe no, render no). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings |  |  |
| VEGAS Fill Light (`filllightvelvetmatter`) | `Vfx1.ofx` #17 |  |  |  | Runs only in VEGAS: it asks for VEGAS's private suites (`OfxVegas*`) and host key (R33c probe: load yes, describe no, render no). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings |  |  |
| VEGAS Gaussian Blur (`gaussianblur`) | `Vfx1.ofx` #18 | `milan.fx.gaussian_blur` (Gaussian Blur); vegas_map row `gaussianblur` → `milan.fx.gaussian_blur`, 2 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Glow (`glow`) | `Vfx1.ofx` #19 | `milan.fx.glow` (Glow); vegas_map row `glow` → `milan.fx.glow`, 6 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS HSL Adjust (`hsladjust`) | `Vfx1.ofx` #20 | `milan.fx.hsl` (HSL Adjust); vegas_map row `hsladjust` → `milan.fx.hsl`, 3 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Invert (`invert`) | `Vfx1.ofx` #21 | `milan.fx.invert` (Invert); vegas_map row `invert` → `milan.fx.invert`, 1 setting carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS LAB Adjust (`labadjust`) | `Vfx1.ofx` #22 |  |  |  | Runs only in VEGAS: it asks for VEGAS's private suites (`OfxVegas*`) and host key (R33c probe: load yes, describe yes, render no). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings |  |  |
| VEGAS Lens Flare (`lensflare`) | `Vfx1.ofx` #23 |  |  |  | Runs only in VEGAS: it asks for VEGAS's private suites (`OfxVegas*`) and host key (R33c probe: load yes, describe no, render no). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings |  |  |
| VEGAS Levels (`levels`) | `Vfx1.ofx` #24 | `milan.fx.levels` (Levels); vegas_map row `levels` → `milan.fx.levels`, 5 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Linear Blur (`linearblur`) | `Vfx1.ofx` #25 | `milan.fx.linear_blur` (Linear Blur); vegas_map row `linearblur` → `milan.fx.linear_blur`, 2 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS LUT Filter (`lutfilter`) | `Vfx1.ofx` #26 | `milan.color_grading` (Color Grading); vegas_map row `lutfilter` → `milan.color_grading`, 1 setting carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Mask Generator (`maskgenerator`) | `Vfx1.ofx` #27 |  |  |  | Runs only in VEGAS: it asks for VEGAS's private suites (`OfxVegas*`) and host key (R33c probe: load yes, describe no, render no). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings |  |  |
| VEGAS Median (`median`) | `Vfx1.ofx` #28 |  |  |  | Runs only in VEGAS: it asks for VEGAS's private suites (`OfxVegas*`) and host key (R33c probe: load yes, describe no, render no). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings |  |  |
| VEGAS Min and Max (`minandmax`) | `Vfx1.ofx` #29 |  |  |  | Runs only in VEGAS: it asks for VEGAS's private suites (`OfxVegas*`) and host key (R33c probe: load yes, describe no, render no). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings |  |  |
| VEGAS Mirror (`mirror`) | `Vfx1.ofx` #30 | `milan.fx.mirror` (Mirror); vegas_map row `mirror` → `milan.fx.mirror`, 2 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Newsprint (`newsprint`) | `Vfx1.ofx` #31 | `milan.fx.newsprint` (Newsprint); vegas_map row `newsprint` → `milan.fx.newsprint`, 2 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Pixelate (`pixelate`) | `Vfx1.ofx` #32 | `milan.fx.pixelate` (Pixelate); vegas_map row `pixelate` → `milan.fx.pixelate`, 2 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Quick Blur (`quickblur`) | `Vfx1.ofx` #33 |  |  |  | Runs only in VEGAS: it asks for VEGAS's private suites (`OfxVegas*`) and host key (R33c probe: load yes, describe no, render no). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings |  |  |
| VEGAS Radial Blur (`radialblur`) | `Vfx1.ofx` #34 | `milan.fx.radial_blur` (Radial Blur); vegas_map row `radialblur` → `milan.fx.radial_blur`, 4 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Saturation Adjust (`saturationadjust`) | `Vfx1.ofx` #35 |  |  |  | Runs only in VEGAS: it asks for VEGAS's private suites (`OfxVegas*`) and host key (R33c probe: load yes, describe no, render no). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings |  |  |
| VEGAS Sepia (`sepia`) | `Vfx1.ofx` #36 | `milan.fx.sepia` (Sepia); vegas_map row `sepia` → `milan.fx.sepia`, 1 setting carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Sharpen (`sharpen`) | `Vfx1.ofx` #37 | `milan.fx.sharpen` (Sharpen); vegas_map row `sharpen` → `milan.fx.sharpen`, 1 setting carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Spherize (`spherize`) | `Vfx1.ofx` #38 | `milan.fx.spherize` (Spherize); vegas_map row `spherize` → `milan.fx.spherize`, 6 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Starburst (`starburstvelvetmatter`) | `Vfx1.ofx` #39 | `milan.fx.starburst` (Starburst); vegas_map row `starburstvelvetmatter` → `milan.fx.starburst`, 7 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Timecode (`timecode`) | `Vfx1.ofx` #40 |  |  |  | Runs only in VEGAS: it asks for VEGAS's private suites (`OfxVegas*`) and host key (R33c probe: load yes, describe no, render no). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings |  |  |
| VEGAS Unsharp Mask (`unsharpmask`) | `Vfx1.ofx` #41 | `milan.fx.sharpen` (Sharpen); vegas_map row `unsharpmask` → `milan.fx.sharpen`, 3 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS White Balance (`whitebalance`) | `Vfx1.ofx` #42 |  |  |  | Runs only in VEGAS: it asks for VEGAS's private suites (`OfxVegas*`) and host key (R33c probe: load yes, describe no, render no). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings |  |  |
| VEGAS Radial Pixelate (`radialpixelate`) | `Vfx1.ofx` #43 |  |  |  | Runs only in VEGAS: it asks for VEGAS's private suites (`OfxVegas*`) and host key (R33c probe: load yes, describe yes, render no). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings |  |  |
| VEGAS Barn Door (`barndoor`) | `Vfx1.ofx` #44 | `milan.tr.barn_door` (Barn Door); vegas_map row `barndoor` → `milan.tr.barn_door`, 7 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Clock Wipe (`clockwipe`) | `Vfx1.ofx` #45 | `milan.tr.clock_wipe` (Clock Wipe); vegas_map row `clockwipe` → `milan.tr.clock_wipe`, 1 setting carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Cross Effect (`crosseffect`) | `Vfx1.ofx` #46 | `milan.tr.cross_effect` (Cross Effect); vegas_map row `crosseffect` → `milan.tr.cross_effect`, 7 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Dissolve (`dissolve`) | `Vfx1.ofx` #47 | `milan.tr.dissolve` (Dissolve); vegas_map row `dissolve` → `milan.tr.dissolve`, no settings to carry; checked in `tests/vegas/crossfade.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Flash (`flash`) | `Vfx1.ofx` #48 | `milan.tr.flash` (Flash); vegas_map row `flash` → `milan.tr.flash`, 5 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Iris (`iris`) | `Vfx1.ofx` #49 | `milan.tr.iris` (Iris); vegas_map row `iris` → `milan.tr.iris`, 2 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Linear Wipe (`linearwipe`) | `Vfx1.ofx` #50 | `milan.tr.linear_wipe` (Linear Wipe); vegas_map row `linearwipe` → `milan.tr.linear_wipe`, 2 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Page Loop (`pageloop`) | `Vfx1.ofx` #51 |  |  |  | Runs only in VEGAS: it asks for VEGAS's private suites (`OfxVegas*`) and host key (R33c probe: load yes, describe no, render no). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings |  |  |
| VEGAS Page Peel (`pagepeel`) | `Vfx1.ofx` #52 | `milan.tr.page_peel` (Page Peel); vegas_map row `pagepeel` → `milan.tr.page_peel`, 6 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Page Roll (`pageroll`) | `Vfx1.ofx` #53 | `milan.tr.page_roll` (Page Roll); vegas_map row `pageroll` → `milan.tr.page_roll`, 6 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Push (`push`) | `Vfx1.ofx` #54 | `milan.tr.push` (Push); vegas_map row `push` → `milan.tr.push`, 1 setting carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Slide (`slide`) | `Vfx1.ofx` #55 | `milan.tr.slide` (Slide); vegas_map row `slide` → `milan.tr.slide`, 1 setting carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Spiral (`spiral`) | `Vfx1.ofx` #56 | `milan.tr.spiral` (Spiral); vegas_map row `spiral` → `milan.tr.spiral`, 5 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Split (`split`) | `Vfx1.ofx` #57 |  |  |  | Runs only in VEGAS: it asks for VEGAS's private suites (`OfxVegas*`) and host key (R33c probe: load yes, describe no, render no). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings |  |  |
| VEGAS Squeeze (`squeeze`) | `Vfx1.ofx` #58 | `milan.tr.squeeze` (Squeeze); vegas_map row `squeeze` → `milan.tr.squeeze`, 2 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Star Wipe (`starwipe`) | `Vfx1.ofx` #59 | `milan.tr.star_wipe` (Star Wipe); vegas_map row `starwipe` → `milan.tr.star_wipe`, 11 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Swap (`swap`) | `Vfx1.ofx` #60 |  |  |  | Runs only in VEGAS: it asks for VEGAS's private suites (`OfxVegas*`) and host key (R33c probe: load yes, describe no, render no). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings |  |  |
| VEGAS Zoom (`zoom`) | `Vfx1.ofx` #61 | `milan.tr.zoom` (Zoom); vegas_map row `zoom` → `milan.tr.zoom`, no settings to carry; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Bump Map (`bumpmap`) | `Vfx1.ofx` #62 |  |  |  | Runs only in VEGAS: it asks for VEGAS's private suites (`OfxVegas*`) and host key (R33c probe: load yes, describe no, render no). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings |  |  |
| VEGAS Layer Dimensionality (`layerdimensionality`) | `Vfx1.ofx` #63 |  |  |  | Runs only in VEGAS: it asks for VEGAS's private suites (`OfxVegas*`) and host key (R33c probe: load yes, describe yes, render no). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings |  |  |
| VEGAS Vignette (`vignette`) | `Vfx1.ofx` #64 | `milan.fx.vignette` (Vignette); vegas_map row `vignette` → `milan.fx.vignette`, 5 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Picture In Picture (`pictureinpicture`) | `Vfx1.ofx` #65 |  |  |  | Runs only in VEGAS: it asks for VEGAS's private suites (`OfxVegas*`) and host key (R33c probe: load yes, describe yes, render no). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings |  |  |
| VEGAS Crop (`crop`) | `Vfx1.ofx` #66 | `milan.fx.crop` (Crop); vegas_map row `crop` → `milan.fx.crop`, no settings to carry; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Stereoscopic 3D Adjust (`Filters.Stereoscopic3DAdjust`) | `Filters.ofx` #0 |  |  |  | Runs only in VEGAS: it asks for VEGAS's private suites (`OfxVegas*`) and host key (R33c probe: load yes, describe no, render no). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings |  |  |
| VEGAS Stabilize (`Stabilize`) | `Stabilize.ofx` #0 |  |  |  | Runs only in VEGAS: it asks for VEGAS's private suites (`OfxVegas*`) and host key (R33c probe: load yes, describe no, render no). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings |  |  |
| VEGAS Titles & Text (`titlesandtext`) | `TitlesAndText.ofx` #0 | `milan.gen.text` (Titles & Text); vegas_map row `titlesandtext` → `milan.gen.text`, 25 settings carried; checked in `tests/vegas/generated-media.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Smart Zoom (`smart_zoom`) | `spica_cutout.ofx` #0 |  |  |  | Runs only in VEGAS: it asks for VEGAS's OpenCL program suite (R33c probe: load yes, describe no, render no). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings |  |  |
| VEGAS Smart Upscale (`smart_upscale`) | `spica_resizer.ofx` #0 |  |  |  | Runs only in VEGAS: it asks for VEGAS's OpenCL program suite (R33c probe: load yes, describe no, render no). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings |  |  |
| HitFilm Bleach Bypass | `HitFilmVegasBasic.ofx` |  |  |  | Runs only in VEGAS: the VEGAS edition stops in its describe action after reading the host's name (R33c probe: load yes, describe no, render no). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings |  |  |
| HitFilm Light Flares | `HitFilmVegasBasic.ofx` |  |  |  | Runs only in VEGAS: the VEGAS edition stops in its describe action after reading the host's name (R33c probe: load yes, describe no, render no). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings |  |  |
| HitFilm Scan Lines | `HitFilmVegasBasic.ofx` |  |  |  | Runs only in VEGAS: the VEGAS edition stops in its describe action after reading the host's name (R33c probe: load yes, describe no, render no). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings |  |  |
| HitFilm Three Strip Color | `HitFilmVegasBasic.ofx` |  |  |  | Runs only in VEGAS: the VEGAS edition stops in its describe action after reading the host's name (R33c probe: load yes, describe no, render no). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings |  |  |
| HitFilm TV Damage | `HitFilmVegasBasic.ofx` |  |  |  | Runs only in VEGAS: the VEGAS edition stops in its describe action after reading the host's name (R33c probe: load yes, describe no, render no). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings |  |  |
| HitFilm Vibrance | `HitFilmVegasBasic.ofx` |  |  |  | Runs only in VEGAS: the VEGAS edition stops in its describe action after reading the host's name (R33c probe: load yes, describe no, render no). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings |  |  |
| HitFilm Witness Protection | `HitFilmVegasBasic.ofx` |  |  |  | Runs only in VEGAS: the VEGAS edition stops in its describe action after reading the host's name (R33c probe: load yes, describe no, render no). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings |  |  |

#### Named in VEGAS 15's `Vfx1.xml` but not in its binary (29)

`Vfx1.ofx`'s resource file names 96 plug-ins; the binary exports 67. These 29 are in no installed binary; projects from other VEGAS versions can still name them.

| Plug-in | Where | Native equivalent | Standard interface | Adapter | Unsupported (reason) | Needs investigation | Tests |
|---|---|---|---|---|---|---|---|
| VEGAS Color Gradient (`colorgradient`) | `Vfx1.ofx` resource only | `milan.gen.gradient` (Color Gradient); vegas_map row `colorgradient` → `milan.gen.gradient`, 2 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Test Pattern (`testpattern`) | `Vfx1.ofx` resource only | `milan.gen.test_pattern` (Test Pattern); vegas_map row `testpattern` → `milan.gen.test_pattern`, 1 setting carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Black Restore (`blackrestore`) | `Vfx1.ofx` resource only |  |  |  | Not in any VEGAS 15 binary and no Milan built-in; a project naming it imports as a disabled `ofx:` placeholder |  |  |
| VEGAS Border (`border`) | `Vfx1.ofx` resource only | `milan.fx.border` (Border); vegas_map row `border` → `milan.fx.border`, 5 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Channel Blend (`channelblend`) | `Vfx1.ofx` resource only |  |  |  | Not in any VEGAS 15 binary and no Milan built-in; a project naming it imports as a disabled `ofx:` placeholder |  |  |
| VEGAS Chroma Blur (`chromablur`) | `Vfx1.ofx` resource only |  |  |  | Not in any VEGAS 15 binary and no Milan built-in; a project naming it imports as a disabled `ofx:` placeholder |  |  |
| VEGAS Deform (`deform`) | `Vfx1.ofx` resource only |  |  |  | Not in any VEGAS 15 binary and no Milan built-in; a project naming it imports as a disabled `ofx:` placeholder |  |  |
| VEGAS Film Effects (`filmeffects`) | `Vfx1.ofx` resource only | `milan.fx.film_effects` (Film Effects); vegas_map row `filmeffects` → `milan.fx.film_effects`, 9 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Film Grain (`filmgrain`) | `Vfx1.ofx` resource only | `milan.fx.film_grain` (Film Grain); vegas_map row `filmgrain` → `milan.fx.film_grain`, 3 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Glint (`glintvelvetmatter`) | `Vfx1.ofx` resource only |  |  |  | Not in any VEGAS 15 binary and no Milan built-in; a project naming it imports as a disabled `ofx:` placeholder |  |  |
| VEGAS Gradient Map (`gradientmap`) | `Vfx1.ofx` resource only | `milan.fx.gradient_map` (Gradient Map); vegas_map row `gradientmap` → `milan.fx.gradient_map`, 2 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Histogram (`histogram`) | `Vfx1.ofx` resource only |  |  |  | Not in any VEGAS 15 binary and no Milan built-in; a project naming it imports as a disabled `ofx:` placeholder |  |  |
| VEGAS Light Rays (`lightrays`) | `Vfx1.ofx` resource only | `milan.fx.light_rays` (Light Rays); vegas_map row `lightrays` → `milan.fx.light_rays`, 8 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Pinch/Punch (`pinchpunch`) | `Vfx1.ofx` resource only | `milan.fx.pinch_punch` (Pinch/Punch); vegas_map row `pinchpunch` → `milan.fx.pinch_punch`, 6 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Rays (`raysvelvetmatter`) | `Vfx1.ofx` resource only |  |  |  | Not in any VEGAS 15 binary and no Milan built-in; a project naming it imports as a disabled `ofx:` placeholder |  |  |
| VEGAS Soft Contrast (`softcontrastvelvetmatter`) | `Vfx1.ofx` resource only | `milan.fx.soft_contrast` (Soft Contrast); vegas_map row `softcontrastvelvetmatter` → `milan.fx.soft_contrast`, 5 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Swirl (`swirl`) | `Vfx1.ofx` resource only |  |  |  | Not in any VEGAS 15 binary and no Milan built-in; a project naming it imports as a disabled `ofx:` placeholder |  |  |
| VEGAS Threshold (`threshold`) | `Vfx1.ofx` resource only | `milan.fx.threshold` (Threshold); vegas_map row `threshold` → `milan.fx.threshold`, 1 setting carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS TV Simulator (`tvsimulator`) | `Vfx1.ofx` resource only | `milan.fx.tv_simulator` (TV Simulator); vegas_map row `tvsimulator` → `milan.fx.tv_simulator`, 8 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Wave (`wave`) | `Vfx1.ofx` resource only |  |  |  | Not in any VEGAS 15 binary and no Milan built-in; a project naming it imports as a disabled `ofx:` placeholder |  |  |
| VEGAS 3D Blinds (`3dblinds`) | `Vfx1.ofx` resource only |  |  |  | Not in any VEGAS 15 binary and no Milan built-in; a project naming it imports as a disabled `ofx:` placeholder |  |  |
| VEGAS 3D Cascade (`3dcascade`) | `Vfx1.ofx` resource only |  |  |  | Not in any VEGAS 15 binary and no Milan built-in; a project naming it imports as a disabled `ofx:` placeholder |  |  |
| VEGAS 3D Fly In/Out (`3dflyinout`) | `Vfx1.ofx` resource only |  |  |  | Not in any VEGAS 15 binary and no Milan built-in; a project naming it imports as a disabled `ofx:` placeholder |  |  |
| VEGAS 3D Shuffle (`3dshuffle`) | `Vfx1.ofx` resource only |  |  |  | Not in any VEGAS 15 binary and no Milan built-in; a project naming it imports as a disabled `ofx:` placeholder |  |  |
| VEGAS Gradient Wipe (`gradientwipe`) | `Vfx1.ofx` resource only |  |  |  | Not in any VEGAS 15 binary and no Milan built-in; a project naming it imports as a disabled `ofx:` placeholder |  |  |
| VEGAS Portals (`portals`) | `Vfx1.ofx` resource only | `milan.tr.portals` (Portals); vegas_map row `portals` → `milan.tr.portals`, 9 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Venetian Blinds (`venetianblinds`) | `Vfx1.ofx` resource only | `milan.tr.venetian_blinds` (Venetian Blinds); vegas_map row `venetianblinds` → `milan.tr.venetian_blinds`, 3 settings carried; checked in `tests/vegas/video-fx.milan.json` |  |  |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Displacement Map (`displacementmap`) | `Vfx1.ofx` resource only |  |  |  | Not in any VEGAS 15 binary and no Milan built-in; a project naming it imports as a disabled `ofx:` placeholder |  |  |
| VEGAS Height Map (`heightmap`) | `Vfx1.ofx` resource only |  |  |  | Not in any VEGAS 15 binary and no Milan built-in; a project naming it imports as a disabled `ofx:` placeholder |  |  |

#### VEGAS DirectX audio plug-ins (`C:\Program Files (x86)\VEGAS\Shared Plug-Ins\Audio_x64`) (39)

One row per plug-in name in the 11 DLLs (`ErrorReport.dll` there is not a plug-in). Milan Pro hosts no DirectX plug-in (`docs/compat/dx-audio.md`); the VEGAS import replaces a known name with a Milan audio FX.

| Plug-in | Where | Native equivalent | Standard interface | Adapter | Unsupported (reason) | Needs investigation | Tests |
|---|---|---|---|---|---|---|---|
| Wave Hammer Surround | `mchammer_x64.dll` |  |  | VEGAS import by name → `milan.afx.limiter` (Limiter); settings not carried (the script API exposes no DirectX parameters), a Milan preset of the same name is used when there is one; checked in `tests/vegas/audio-fx.milan.json` |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Pan | `sffrgpnv_x64.dll` |  |  |  | No DirectX host in Milan Pro and no stand-in in `vegas_map`; the import leaves it out and says so |  |  |
| VEGAS Volume | `sffrgpnv_x64.dll` |  |  |  | No DirectX host in Milan Pro and no stand-in in `vegas_map`; the import leaves it out and says so |  |  |
| VEGAS Time Stretch | `sfppack1_x64.dll` |  |  |  | No DirectX host in Milan Pro and no stand-in in `vegas_map`; the import leaves it out and says so |  |  |
| VEGAS Pitch Shift | `sfppack1_x64.dll` |  |  |  | No DirectX host in Milan Pro and no stand-in in `vegas_map`; the import leaves it out and says so |  |  |
| VEGAS Simple Delay | `sfppack1_x64.dll` |  |  | VEGAS import by name → `milan.afx.delay` (Simple Delay); settings not carried (the script API exposes no DirectX parameters), a Milan preset of the same name is used when there is one; checked in `tests/vegas/audio-fx.milan.json` |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Multi-Tap Delay | `sfppack1_x64.dll` |  |  | VEGAS import by name → `milan.afx.delay` (Simple Delay); settings not carried (the script API exposes no DirectX parameters), a Milan preset of the same name is used when there is one; checked in `tests/vegas/audio-fx.milan.json` |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Reverb | `sfppack1_x64.dll` |  |  | VEGAS import by name → `milan.afx.reverb` (Reverb); settings not carried (the script API exposes no DirectX parameters), a Milan preset of the same name is used when there is one; checked in `tests/vegas/audio-fx.milan.json` |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Chorus | `sfppack1_x64.dll` |  |  |  | No DirectX host in Milan Pro and no stand-in in `vegas_map`; the import leaves it out and says so |  |  |
| VEGAS Noise Gate | `sfppack2_x64.dll` |  |  | VEGAS import by name → `milan.afx.gate` (Track Noise Gate); settings not carried (the script API exposes no DirectX parameters), a Milan preset of the same name is used when there is one; checked in `tests/vegas/audio-fx.milan.json` |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Graphic Dynamics | `sfppack2_x64.dll` |  |  |  | No DirectX host in Milan Pro and no stand-in in `vegas_map`; the import leaves it out and says so |  |  |
| VEGAS Paragraphic EQ | `sfppack2_x64.dll` |  |  | VEGAS import by name → `milan.afx.eq` (Track EQ); settings not carried (the script API exposes no DirectX parameters), a Milan preset of the same name is used when there is one; checked in `tests/vegas/audio-fx.milan.json` |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Parametric EQ | `sfppack2_x64.dll` |  |  | VEGAS import by name → `milan.afx.eq` (Track EQ); settings not carried (the script API exposes no DirectX parameters), a Milan preset of the same name is used when there is one; checked in `tests/vegas/audio-fx.milan.json` |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Multi-Band Dynamics | `sfppack2_x64.dll` |  |  |  | No DirectX host in Milan Pro and no stand-in in `vegas_map`; the import leaves it out and says so |  |  |
| VEGAS Graphic EQ | `sfppack2_x64.dll` |  |  | VEGAS import by name → `milan.afx.eq` (Track EQ); settings not carried (the script API exposes no DirectX parameters), a Milan preset of the same name is used when there is one; checked in `tests/vegas/audio-fx.milan.json` |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Flange/Wah-Wah | `sfppack3_x64.dll` |  |  |  | No DirectX host in Milan Pro and no stand-in in `vegas_map`; the import leaves it out and says so |  |  |
| VEGAS Gapper/Snipper | `sfppack3_x64.dll` |  |  |  | No DirectX host in Milan Pro and no stand-in in `vegas_map`; the import leaves it out and says so |  |  |
| VEGAS Vibrato | `sfppack3_x64.dll` |  |  |  | No DirectX host in Milan Pro and no stand-in in `vegas_map`; the import leaves it out and says so |  |  |
| VEGAS Smooth/Enhance | `sfppack3_x64.dll` |  |  |  | No DirectX host in Milan Pro and no stand-in in `vegas_map`; the import leaves it out and says so |  |  |
| VEGAS Distortion | `sfppack3_x64.dll` |  |  |  | No DirectX host in Milan Pro and no stand-in in `vegas_map`; the import leaves it out and says so |  |  |
| VEGAS Amplitude Modulation | `sfppack3_x64.dll` |  |  |  | No DirectX host in Milan Pro and no stand-in in `vegas_map`; the import leaves it out and says so |  |  |
| VEGAS Resonant Filter | `sfresfilter_x64.dll` |  |  |  | No DirectX host in Milan Pro and no stand-in in `vegas_map`; the import leaves it out and says so |  |  |
| VEGAS Track Compressor | `sftrkfx1_x64.dll` |  |  | VEGAS import by name → `milan.afx.compressor` (Track Compressor); settings not carried (the script API exposes no DirectX parameters), a Milan preset of the same name is used when there is one; checked in `tests/vegas/audio-fx.milan.json` |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Track EQ | `sftrkfx1_x64.dll` |  |  | VEGAS import by name → `milan.afx.eq` (Track EQ); settings not carried (the script API exposes no DirectX parameters), a Milan preset of the same name is used when there is one; checked in `tests/vegas/audio-fx.milan.json` |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Track Noise Gate | `sftrkfx1_x64.dll` |  |  | VEGAS import by name → `milan.afx.gate` (Track Noise Gate); settings not carried (the script API exposes no DirectX parameters), a Milan preset of the same name is used when there is one; checked in `tests/vegas/audio-fx.milan.json` |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS Dither | `sftrkfx1_x64.dll` |  |  |  |  | Named in the DLL's string table beside the Track FX; whether VEGAS lists it as an audio FX is to be checked in VEGAS's Plug-In Manager |  |
| VEGAS ExpressFX Flange/Wah-Wah | `sfxpfx1_x64.dll` |  |  |  | No DirectX host in Milan Pro and no stand-in in `vegas_map`; the import leaves it out and says so |  |  |
| VEGAS ExpressFX Reverb | `sfxpfx1_x64.dll` |  |  | VEGAS import by name → `milan.afx.reverb` (Reverb); settings not carried (the script API exposes no DirectX parameters), a Milan preset of the same name is used when there is one; checked in `tests/vegas/audio-fx.milan.json` |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS ExpressFX Distortion | `sfxpfx1_x64.dll` |  |  |  | No DirectX host in Milan Pro and no stand-in in `vegas_map`; the import leaves it out and says so |  |  |
| VEGAS ExpressFX Stutter | `sfxpfx1_x64.dll` |  |  |  | No DirectX host in Milan Pro and no stand-in in `vegas_map`; the import leaves it out and says so |  |  |
| VEGAS ExpressFX Delay | `sfxpfx2_x64.dll` |  |  | VEGAS import by name → `milan.afx.delay` (Simple Delay); settings not carried (the script API exposes no DirectX parameters), a Milan preset of the same name is used when there is one; checked in `tests/vegas/audio-fx.milan.json` |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS ExpressFX Equalization | `sfxpfx2_x64.dll` |  |  | VEGAS import by name → `milan.afx.eq` (Track EQ); settings not carried (the script API exposes no DirectX parameters), a Milan preset of the same name is used when there is one; checked in `tests/vegas/audio-fx.milan.json` |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS ExpressFX Chorus | `sfxpfx2_x64.dll` |  |  |  | No DirectX host in Milan Pro and no stand-in in `vegas_map`; the import leaves it out and says so |  |  |
| VEGAS ExpressFX Amplitude Modulation | `sfxpfx2_x64.dll` |  |  |  | No DirectX host in Milan Pro and no stand-in in `vegas_map`; the import leaves it out and says so |  |  |
| VEGAS ExpressFX Dynamics | `sfxpfx3_x64.dll` |  |  | VEGAS import by name → `milan.afx.compressor` (Track Compressor); settings not carried (the script API exposes no DirectX parameters), a Milan preset of the same name is used when there is one; checked in `tests/vegas/audio-fx.milan.json` |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS ExpressFX Graphic EQ | `sfxpfx3_x64.dll` |  |  |  | No DirectX host in Milan Pro and no stand-in in `vegas_map`; the import leaves it out and says so |  |  |
| VEGAS ExpressFX Noise Gate | `sfxpfx3_x64.dll` |  |  | VEGAS import by name → `milan.afx.gate` (Track Noise Gate); settings not carried (the script API exposes no DirectX parameters), a Milan preset of the same name is used when there is one; checked in `tests/vegas/audio-fx.milan.json` |  |  | `interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter`<br>`interchange::vegas_examples::every_example_imports_as_expected` |
| VEGAS ExpressFX Time Stretch | `sfxpfx3_x64.dll` |  |  |  | No DirectX host in Milan Pro and no stand-in in `vegas_map`; the import leaves it out and says so |  |  |
| VEGAS ExpressFX Audio Restoration | `xpvinyl_x64.dll` |  |  |  | No DirectX host in Milan Pro and no stand-in in `vegas_map`; the import leaves it out and says so |  |  |

#### VEGAS legacy video plug-ins (`Video Plug-Ins\`) (7)

The pre-OFX generation, one row per DLL (the effects inside are not listed by any public interface).

| Plug-in | Where | Native equivalent | Standard interface | Adapter | Unsupported (reason) | Needs investigation | Tests |
|---|---|---|---|---|---|---|---|
| VEGAS Color Corrector Plugins | `colorcorrector.dll` |  |  |  | VEGAS-private plug-in interface from before OFX, with no SDK; licensed with VEGAS |  |  |
| VEGAS PluginWrapper Module | `PluginWrapper.dll` |  |  |  | VEGAS-private plug-in interface from before OFX, with no SDK; licensed with VEGAS |  |  |
| VEGAS PagePeel Video Transitions | `sfpagepeel.dll` |  |  |  | VEGAS-private plug-in interface from before OFX, with no SDK; licensed with VEGAS |  |  |
| VEGAS Transition Video Plugins | `sftrans1.dll` |  |  |  | VEGAS-private plug-in interface from before OFX, with no SDK; licensed with VEGAS |  |  |
| VEGAS Plug-Ins | `vfplugs.dll` |  |  |  | VEGAS-private plug-in interface from before OFX, with no SDK; licensed with VEGAS |  |  |
| VEGAS Video Plug-In Pack | `vfx1.dll` |  |  |  | VEGAS-private plug-in interface from before OFX, with no SDK; licensed with VEGAS |  |  |
| VEGAS Video Plugins | `vidpcore.dll` |  |  |  | VEGAS-private plug-in interface from before OFX, with no SDK; licensed with VEGAS |  |  |

#### Third-party and test plug-ins, and interfaces (29)

**Only Milan Pro's own test plug-ins have been run.** They are written against Milan Pro's own reading of each interface, so they prove the hosts consistent with themselves, not that every third-party plug-in works. No third-party OFX, VST3, CLAP or AE-format plug-in has been run yet; those named below are candidates. The tests marked `#[ignore]` in the source need the built plug-in host: `cargo build --release`, then `cargo test --release -- --ignored --test-threads=1 ofx:: aplug:: aex::`.

| Plug-in | Where | Native equivalent | Standard interface | Adapter | Unsupported (reason) | Needs investigation | Tests |
|---|---|---|---|---|---|---|---|
| Milan Test Invert (com.meridiannle.test.invert) | tools/ofx-test-plugin (Milan's own) |  | OFX: Filter; 8-bit and float; matches the built-in Invert |  |  |  | `engine::ofx::tests::describe_through_the_host`<br>`engine::ofx::tests::invert_matches_the_builtin_invert`<br>`engine::ofx::tests::render_matches_preview_with_ofx_plugins` |
| Milan Test Gain (com.meridiannle.test.gain) | tools/ofx-test-plugin (Milan's own) |  | OFX: Filter with a keyframed parameter read at the frame's time |  |  |  | `engine::ofx::tests::gain_is_keyframed_at_t` |
| Milan Test Crash (com.meridiannle.test.crash) | tools/ofx-test-plugin (Milan's own) |  | OFX: Crashes in render on purpose: the host process dies, the FX is bypassed with a message |  |  |  | `engine::ofx::tests::crash_and_hang_are_contained` |
| Milan Test Hang (com.meridiannle.test.hang) | tools/ofx-test-plugin (Milan's own) |  | OFX: Never returns from render on purpose: the watchdog stops it, the FX is bypassed |  |  |  | `engine::ofx::tests::crash_and_hang_are_contained` |
| Milan Test Dissolve (com.meridiannle.test.dissolve) | tools/ofx-test-plugin (Milan's own) |  | OFX: Transition context |  |  |  | `engine::ofx::tests::transition_and_generator_render_through_the_host` |
| Milan Test Solid (com.meridiannle.test.solid) | tools/ofx-test-plugin (Milan's own) |  | OFX: Generator context |  |  |  | `engine::ofx::tests::transition_and_generator_render_through_the_host` |
| Milan Test Params (com.meridiannle.test.params) | tools/ofx-test-plugin (Milan's own) |  | OFX: Push buttons, parameters set by the plug-in, run-time secret/enabled changes, messages |  |  |  | `engine::ofx::tests::plug_in_edits_buttons_state_and_messages` |
| Milan Test Gain | tools/vst3-test-plugin (Milan's own) |  | VST3: Separate controller, 64-sample latency (PDC), program list, state, .vstpreset |  |  |  | `engine::aplug::tests::vst3_and_clap_modules_are_scanned`<br>`engine::aplug::tests::vst3_gain_applies_to_a_sine`<br>`engine::aplug::tests::pdc_aligns_a_latent_plug_in_with_a_dry_track`<br>`engine::aplug::tests::preset_round_trip_restores_parameters`<br>`engine::aplug::tests::render_matches_preview_with_a_vst3_fx` |
| Milan Test Crash | tools/vst3-test-plugin (Milan's own) |  | VST3: Crashes in process on purpose: contained, the FX is bypassed |  |  |  | `engine::aplug::tests::crash_and_hang_are_contained` |
| Milan Test Hang | tools/vst3-test-plugin (Milan's own) |  | VST3: Never returns from process on purpose: stopped by the watchdog, the FX is bypassed |  |  |  | `engine::aplug::tests::crash_and_hang_are_contained` |
| Milan Test CLAP Gain (com.meridiannle.test.clapgain) | tools/clap-test-plugin (Milan's own) |  | CLAP: Gain and mute parameters, 32-sample latency |  |  |  | `engine::aplug::tests::vst3_and_clap_modules_are_scanned`<br>`engine::aplug::tests::clap_plug_in_processes_audio` |
| Test Invert (MLNT Test Invert) | tools/aex-test-plugin (Milan's own) |  | AE-format: Experimental AE-format effects (off by default); equals the built-in invert within 1 LSB |  |  |  | `engine::aex::tests::aex_invert_matches_the_builtin_invert` |
| Test Gain (MLNT Test Gain) | tools/aex-test-plugin (Milan's own) |  | AE-format: Keyframed gain read at the frame's time |  |  |  | `engine::aex::tests::aex_gain_is_keyframed_at_t` |
| Test Params (MLNT Test Params) | tools/aex-test-plugin (Milan's own) |  | AE-format: 12 of 14 parameters get controls (its button and group end do not) |  |  |  | `engine::aex::tests::aex_test_plugin_describes` |
| Test Marker (MLNT Test Marker) | tools/aex-test-plugin (Milan's own) |  | AE-format: Row and channel order |  |  |  | `engine::aex::tests::row_order_and_channel_order_marker` |
| Test Counter (MLNT Test Counter) | tools/aex-test-plugin (Milan's own) |  | AE-format: Sequence data flattened on save and set up again in a new host |  |  |  | `engine::aex::tests::sequence_data_survives_save_and_load` |
| Test Callbacks (MLNT Test Callbacks) | tools/aex-test-plugin (Milan's own) |  | AE-format: Every tier-a callback and suite Milan serves |  |  |  | `engine::aex::tests::aex_callbacks_match_a_rust_reference` |
| Test Previous Frame (MLNT Test Previous Frame) | tools/aex-test-plugin (Milan's own) |  | AE-format: Checkout of the input at another time |  |  |  | `engine::aex::tests::checkout_at_other_time_returns_that_frame` |
| Test Supervisor (MLNT Test Supervisor) | tools/aex-test-plugin (Milan's own) |  | AE-format: A supervised checkbox greys and sets its sibling |  |  |  | `engine::aex::tests::supervised_param_disables_its_sibling` |
| Test Layer (MLNT Test Layer) | tools/aex-test-plugin (Milan's own) |  | AE-format: Layer parameter: None or this event only |  |  |  | `engine::aex::tests::layer_param_none_is_transparent` |
| Test Echo (MLNT Test Echo) | tools/aex-test-plugin (Milan's own) |  | AE-format: Ten nested frame checkouts do not deadlock |  |  |  | `engine::aex::tests::echo_style_plugin_does_not_deadlock` |
| Test Crash (MLNT Test Crash) | tools/aex-test-plugin (Milan's own) |  | AE-format: Crashes in render on purpose: contained and bypassed |  |  |  | `engine::aex::tests::aex_crash_and_hang_are_contained` |
| Test Hang (MLNT Test Hang) | tools/aex-test-plugin (Milan's own) |  | AE-format: Hangs in render on purpose: stopped by the watchdog |  |  |  | `engine::aex::tests::aex_crash_and_hang_are_contained` |
| Test Crash Setup (MLNT Test Crash Setup) | tools/aex-test-plugin (Milan's own) |  | AE-format: Crashes in global setup on purpose: contained; the file's other effects are still described |  |  |  | `engine::aex::tests::a_plugin_that_crashes_in_global_setup_is_contained` |
| Spektrafilm (org.spektrafilm) | third party (not downloaded) |  |  |  |  | OFX: Compared from its source only (docs/compat/ofx-spektrafilm.md): expected to work, never run |  |
| ntsc-rs | third party (not downloaded) |  |  |  |  | OFX and AE-format: The comparison test exists (MILAN_OFX_TEST_DIR and MILAN_AEX_TEST_DIR) and has never been run: no binary was available |  |
| after-effects crate examples (simplest, paramarama, checkout, supervisor) | third party (not built) |  |  |  |  | AE-format: Tests exist (MILAN_AEX_TEST_DIR) and have never been run |  |
| any VST2 plug-in | interface |  |  |  | VST2: Steinberg no longer licenses the VST2 SDK and there is no clean-room binding Milan Pro could ship; use a VST3 or CLAP version |  |  |
| any DirectX audio plug-in | interface |  |  |  | DirectX (DMO): Investigated, not pursued (docs/compat/dx-audio.md): deprecated format whose only interesting plug-ins are VEGAS's own |  |  |

<!-- compat-matrix:end -->

### VST2, DirectX and ASIO

- **VST2 is not supported.** Steinberg no longer licenses the VST2 SDK, and there is no clean-room binding Milan Pro
  could ship. Use a plug-in's VST3 or CLAP version; Milan Pro hosts both (R34a, R34b, R35).
- **DirectX (DMO) audio plug-ins are not hosted.** A DMO does process audio outside its host (Windows's own ParamEq
  did, `docs/compat/dx-audio.md`), but the format is deprecated and the only plug-ins worth hosting are VEGAS's own,
  which are licensed with VEGAS. A VEGAS project's DirectX FX are replaced by name (the Adapter rows above).
- **ASIO is not supported.** cpal's ASIO back end builds against the Steinberg ASIO SDK, whose licence does not allow
  it to be redistributed in an open-source project. Windows Audio (Exclusive Mode) gives low latency instead
  ([Audio devices](#audio-devices)).

## Example VEGAS projects

`tests/vegas/` holds eleven example exports, each `<name>.milan.json` with an `<name>.expected.json` beside it.
**They are written by hand to the export script's schema (format `milan-vegas`, version 2); none was exported from
VEGAS**, and each file says so in its `_note`. Their media paths are fictitious: the test probes them without opening
anything. `interchange::vegas_examples::every_example_imports_as_expected` imports each one and checks the values its
expected file lists (JSON pointers into the imported project, FX chains, counts, shared group ids, warnings).

| Example | What it covers |
|---|---|
| `cuts-and-fades` | Cuts, fades and their curves, event opacity and gain, A/V groups, mute, lock and loop switches, an offline file, snapping to frames |
| `crossfade` | A video crossfade with VEGAS's Dissolve, an audio crossfade with curves |
| `pan-crop` | Pan/Crop keyframes (full frame, a 2x zoom, a held mirror) |
| `track-motion` | Track Motion keyframes, 2D Shadow and Glow, parent/child compositing with Parent Motion |
| `video-fx` | Every `vegas_map` row (33 filters with converted parameters, 17 transitions, 6 generators), a video output FX, a media FX, and two plug-ins that do not map (Radial Pixelate, Page Loop) |
| `generated-media` | Titles & Text (RTF to text and font), Solid Color, Checkerboard |
| `envelopes` | Composite Level, Fade to Color, Volume, Pan and Mute track envelopes, an event Velocity envelope, an envelope Milan does not import |
| `markers-regions` | Markers, regions (one with a negative length) and the time selection |
| `takes` | Inactive takes, a reversed event with a reversed take, an audio take |
| `audio-fx` | Every VEGAS audio FX name `vegas_map` knows, a bypassed one, an event FX with a preset, master bus FX, two names with no stand-in |
| `surround-5-1` | VEGAS's 5.1 master bus (`AudioBusMode.Surround`), 24-bit audio, Wave Hammer Surround |

To check one against VEGAS: build the same project in VEGAS 15, run Export to Milan Pro, and import the result; set
`MILAN_VEGAS_EXAMPLES_DUMP` to a folder to have the test write each imported project there for comparison.

## VEGAS behaviours still to confirm

These ten behaviours were never checked in VEGAS: Milan Pro does what is written in the middle column, which is the
developers' best understanding. **None of them is confirmed.** The owner checks each one in VEGAS Pro 15 and records
the answer here (`docs/FEATURE_PLAN.md` keeps the original list).

| # | Behaviour | Milan Pro today | Question to answer in VEGAS 15 | Status |
|---|---|---|---|---|
| 1 | Video crossfade without a transition | A true dissolve: the outgoing event at full opacity, the incoming one on top with its fade in (decision 0.9) | Overlap two events on one video track: is the overlap a dissolve, or are both fades drawn over black? | Open |
| 2 | Auto Ripple trim of an event's start | Later events move by the trimmed amount (`src/edit.rs`) | With Auto Ripple on, trim the start of an event: do the later events move, and by how much? | Open |
| 3 | VP16 Selection tool rubber band | Selects events only | Does dragging a rubber band with the Selection tool also set the time selection? | Open |
| 4 | Insert Track position | Above the focused track | Does Insert Video/Audio Track add the track above or below the focused one? | Open |
| 5 | Names of Fade to Color, Flash and Additive Dissolve | As `vegas_map` names them (`flash` → Milan's Flash) | What do VEGAS 15's Transitions window and the script's `PlugIn.Name` call these three? | Open |
| 6 | Default interpolation of new Pan/Crop and Track Motion keys | Smooth | Is a new Pan/Crop or Track Motion keyframe Smooth by default? | Open |
| 7 | Track FX and Track Motion order | Track FX first, then Track Motion (`src/engine/plan.rs`) | Does a track's Video FX chain run before or after its Track Motion? | Open |
| 8 | Bypass All Audio FX in a render | Monitoring only: a render keeps every audio FX | Does Options › Bypass All Audio FX also bypass the FX in File › Render As? | Open |
| 9 | Scripting API property names | The scripts use the names in VEGAS 15's `ScriptPortal.Vegas.dll`; they compile, but have not run | Do both scripts run in VEGAS 15 without an error, and does each example above export and import as expected? | Open |
| 10 | `.sfvp0` proxy files | Not read; Milan Pro makes its own proxies (`src/proxy.rs`) | Can FFmpeg decode a `.sfvp0` that VEGAS 15 wrote? | Open |

## Audio devices

Preferences › Audio Device › Audio device type:

| Type | What it is | VEGAS |
|---|---|---|
| Microsoft Sound Mapper (default) | The Windows default playback device, through WASAPI shared mode (cpal) | Same name and behaviour |
| Windows Classic Wave Driver | The playback device you choose, through WASAPI shared mode | Same name; VEGAS uses MME underneath |
| Windows Audio (Exclusive Mode) | The chosen device opened in WASAPI exclusive mode at its smallest period (or the Buffer size), for low-latency playback and input monitoring. Other programs go silent on that device while Milan Pro runs. | Not in VEGAS (Milan Pro's own, off by default) |
| ASIO | Not available | VEGAS offers it |

**ASIO is not supported.** cpal's `asio` feature builds against the Steinberg ASIO SDK, whose licence does not allow
it to be redistributed in an open-source project. Use Windows Audio (Exclusive Mode) for low latency instead.

Other limits:
- Playback is stereo. The rear and center/LFE device choices come with the multichannel engine (R10a).
- Recording and input monitoring use one stereo input (the Default audio recording device). Input monitoring needs
  the recording and playback devices at the same sample rate; otherwise the status bar says so and nothing is heard.
- The latency figures on the Audio Device page are what the devices report. Check them with a loopback cable
  (an output wired to an input) and a recording of a click if you need sample accuracy.

## VEGAS projects

Projects move between VEGAS Pro and Milan Pro through VEGAS's own scripting API, in both directions, never through
`.veg` files (`docs/INTERCHANGE.md`, `docs/research/VEG_FORMAT.md`). The scripts in `tools/vegas` compile against
VEGAS Pro 15's `ScriptPortal.Vegas.dll`; the `(VEGAS 13)` copies use the `Sony.Vegas` namespace of VEGAS Pro 13 and
earlier. They have not been run inside VEGAS by the developers: the user's VEGAS 15 pass is the acceptance test.

## Interchange formats (R28, R29)

What each File > Import / Export format carries is in `docs/INTERCHANGE.md` (the summary table there says, per
feature, accurate / converted / not representable). In short:

| Format | Read | Write | Tested against |
|---|---|---|---|
| OpenTimelineIO (`.otio`) | Yes (`Clip.1` and `Clip.2`, nested stacks flattened, unknown schemas skipped) | Yes, lossless for Milan's own files (`metadata["milan"]`) | Hand-written fixtures to the OTIO JSON schema; the OTIO repository's samples were not downloaded |
| CMX 3600 EDL (`.edl`) | Yes (V, A-A4, C, D, W as D, M2, BL, FROM/TO CLIP NAME, SOURCE FILE) | Yes (DF/NDF by project rate and ruler) | Golden files at 25 and 29.97 DF, a Resolve-style EDL with an hour start and black |
| VEGAS EDL text (`.txt`) | Yes | Yes | Round trip; the column names are VEGAS 15's (not run through VEGAS here) |
| FCP7 XML (`.xml`, Premiere Pro, DaVinci Resolve) | Yes (stereo pairs merged, Time Remap, Opacity, Audio Levels, links) | Yes (xmeml 5) | Round trip, a hand-written Resolve-shaped sequence; not opened in Premiere or Resolve here |
| FCPXML 1.10 (`.fcpxml`) | Yes (spine, lanes, connected storylines, `clip`, `sync-clip`, time maps) | Yes | Round trip, a hand-written FCP-shaped sequence; not opened in Final Cut Pro here |
| AAF | No | No | Route: `otioconvert` with `otio-aaf-adapter` (external, Python) |
| Premiere `.prproj` | No | No | Undocumented gzip format; the Premiere rows use FCP7 XML, which Premiere imports |

### VEGAS plug-ins that become Milan Pro built-ins (`src/interchange/vegas_map.rs`)

| VEGAS plug-in (`com.sonycreativesoftware:`) | Milan Pro | Settings carried |
|---|---|---|
| gaussianblur | Gaussian Blur | ranges (scaled: VEGAS's 0.06 is Milan's 0.012) |
| brightnessandcontrast | Brightness and Contrast | brightness, contrast, centre |
| colorcorrector | Color Corrector | saturation, gamma, gain, offset (the Low/Mid/High wheels are dropped) |
| levels | Levels | input/output start and end, gamma |
| hsladjust | HSL Adjust | hue (turns to degrees), saturation, luminance |
| chromakeyer | Chroma Keyer | colour, thresholds, blur, mask only |
| sharpen, unsharpmask | Sharpen | amount (radius, threshold from Unsharp Mask) |
| sepia | Sepia | blending strength |
| invert, blackandwhite | Invert, Black and White | amount |
| mirror | Mirror | angle to direction, centre to position |
| border | Border | size, colour |
| vignette | Vignette | outer radius, feather, colour |
| crop | Crop | kept rectangle to left/top/right/bottom |
| cookiecutter | Cookie Cutter | shape, size, centre, feather, border, colour, cut-away method |
| lutfilter | Color Grading | the LUT file as the Look LUT |
| solidcolor, colorgradient, checkerboard, noisetexture, testpattern, titlesandtext | the generators of the same names | colours, type, text and font (out of VEGAS's RTF), size, location, outline, shadow |
| dissolve, linearwipe, clockwipe, iris, push, slide, zoom, flash | Dissolve, Linear Wipe, Clock Wipe, Iris, Push, Slide, Zoom, Fade to Color (Flash preset) | angle, feather, shape, direction, tint |

Audio FX travel by name: Track EQ, Track Compressor, Track Noise Gate, Graphic / Paragraphic / Parametric EQ, Reverb,
Simple Delay, Multi-Tap Delay, Wave Hammer and the ExpressFX plug-ins map to Milan Pro's EQ, Compressor, Noise Gate,
Reverb, Delay and Limiter; VEGAS's DirectX plug-ins expose no parameters to scripts, so only the preset name goes
with them. Every other plug-in is a disabled `ofx:<id>` placeholder in Milan Pro, listed in the import report; VEGAS's
own OFX bundles stay placeholders because they run only inside VEGAS (`docs/compat/ofx-vegas15.md`).

## Camera formats (R41)

File › Import › Camera Card Folder (or a folder dropped on Project Media or the timeline) scans a card for its clips.
Verified on FFmpeg-generated stand-ins only (`camera_formats_e2e`, `format_corpus_e2e`); no camera originals were used.

| Format | What Milan Pro does |
|---|---|
| AVCHD (`PRIVATE/AVCHD/BDMV/STREAM/*.MTS`) | Each recording is one clip. Parts of a recording split at the 2 GB limit (the clock runs on from one part to the next) are joined into one item through a `<first part>-span.ffconcat` list written beside the first part; a read-only card imports the parts one by one and says so. Stand-in: an H.264/AC-3 transport stream cut in two. Timecode from the H.264 pic-timing SEI is read when present (the stand-in has none: FFmpeg's encoders cannot write it; the parser is unit-tested). |
| XDCAM HD / HD422 (`Clip/*.MXF`), XDCAM EX (`BPAV/CLPR/*/*.MP4`) | Decodes (MPEG-2 4:2:2 in MXF OP1a stand-in); MXF timecode read. EX spans are not joined (the `.SMI` links are not read). |
| XAVC / XAVC-S (MXF or MP4) | Decodes (H.264 4:2:2 10-bit MOV/MP4 stand-in with make, model and a timecode track); camera and timecode in Media Properties. |
| P2 (`CONTENTS/VIDEO`, `CONTENTS/AUDIO`, MXF OP-Atom) | Picture and sound essence import as separate items (AVC-Intra 100 in MXF OP1a and DNxHD/PCM OP-Atom stand-ins). Group them on the timeline. |
| Canon XF (`CONTENTS/CLIPS001/*/*.MXF`) | Decodes (MPEG-2 4:2:2 MXF); spans are not joined. |
| GoPro (`DCIM/100GOPRO/GX01xxxx.MP4`) | Chapters of one recording join into one item; the GPMF telemetry stream is skipped. |
| ProRes, DNxHD, DNxHR, CineForm | Decode; render templates: MOV ProRes 422 / 422 HQ / 4444 (alpha), DNxHR LB/SQ/HQ/HQX/444, MXF DNxHR HQ, MXF DNxHD 145 1080, CineForm. |
| Blackmagic RAW, RED R3D, ARRIRAW, Canon Cinema RAW Light, Nikon N-RAW | Not supported: they need proprietary SDKs that cannot be redistributed with an MIT program, and FFmpeg has no decoders. The files import as offline items that say why. |
| CinemaDNG | Not supported: FFmpeg's DNG reading has no camera colour matrix, white balance or the common compressions. Single DNG stills import as pictures with that caveat; DNG frames are not offered as a sequence. |

Media Properties shows the camera (make and model, or a camera MXF's identification), the reel and the start
timecode; Project Media's item menu has "Use Media Timecode as Ruler Start", which sets the ruler offset so an event
at the start of the timeline reads its source timecode.

## AE-format effects (experimental)

**AE-format effects (experimental).** Milan Pro can load some effect plug-ins written for the Adobe After Effects
effect API (`.aex` files) that you have installed and licensed yourself. Milan Pro includes no Adobe software and no
third-party plug-ins. It implements only a basic part of the interface, similar to what video editors (rather than
After Effects itself) provide: simple filters with standard controls, in 8-bit. Plug-ins that need After Effects
features such as compositions, cameras, lights, masks, their own drawn controls, GPU rendering, 32-bit float or
SmartFX will not work, and many commercial plug-ins check for an Adobe host or are licensed only for specific hosts;
Milan Pro does not bypass any such check. Each plug-in runs in a separate process, so a failing plug-in cannot crash
the editor; the Plug-In Manager shows what loaded, what did not, and why. If a vendor offers an OpenFX version, use
that instead. Check your plug-in's licence before using it in Milan Pro. Adobe does not support third-party hosts of
its plug-in interface. Adobe, After Effects and Premiere Pro are trademarks of Adobe Inc.; Milan Pro is not
affiliated with, endorsed or sponsored by Adobe.

The feature is off by default (Plug-In Manager › Folders › Experimental: AE-format effects). The tested plug-ins are
in `docs/compat/aex.csv`, one row per plug-in version actually run. **So far that is only Milan Pro's own test
plug-in** (`tools/aex-test-plugin`), which is written against Milan Pro's own declarations: it proves the host
consistent with itself, not that it matches plug-ins built by others. No third-party plug-in has been run yet; every
one shows "Untested" in the Plug-In Manager, and none is claimed to work.

| Part of the interface | In Milan Pro |
|---|---|
| Discovery, PiPL, Blocked (Adobe's own effects), audio effects refused | Yes (A1) |
| Global and parameter setup; slider, float slider, fixed slider, angle, checkbox, colour, point, popup, group, layer parameters | Yes (A2); buttons, custom data, paths: not shown |
| Utility callbacks and the handle, iterate (8-bit), world, fill matte, world transform (copy, blend), ANSI and float parameter suites | Yes (A3); sampling, convolve, 16-bit and float callbacks refused and noted |
| 8-bit render, sequence data with flatten/resetup saved in the project | Yes (A4) |
| Supervision (changed parameters, greyed and hidden parameters), checkout of other frames and of layer parameters (None, this event) | Yes (A5) |
| 32-bit float (Premiere pixel-format route) | No: its suites are not documented publicly (A6 dropped) |
| Transitions (PF Transition Suite) | No: the suite and how progress is passed are not documented publicly (A7 dropped) |
| SmartFX, 16-bit | No: needs the "After Effects dialect", not decided (A8) |
| Custom drawn controls, options dialogs, audio, AEGP, cameras, lights, masks, GPU | No |
