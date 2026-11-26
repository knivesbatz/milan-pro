# VEGAS Pro 15 OFX plug-ins in Milan Pro (R33c)

**Result: none of the 79 plug-ins in the 7 bundles installed with VEGAS Pro 15 runs in Milan Pro.** Every first-party VEGAS plug-in asks the host for VEGAS's private suites and stops without them; the two Spica plug-ins need VEGAS's OpenCL program suite; HitFilm Vegas Basic (the VEGAS edition of FXHOME's plug-ins) stops in its describe action without asking for anything Milan Pro lacks. Milan Pro claims support for none of them, and the VEGAS folder is **not** added to the default plug-in scan (nothing in it would work; the results below are the reason).

The data is `docs/compat/ofx-vegas15.csv`, one row per plug-in the binaries export (plus one per binary that could not be loaded), written by the probe described below. Rerun it with `cargo test --release -- --ignored --test-threads=1 --exact engine::ofx::tests::vegas15_compat_report --nocapture` (set `MILAN_COMPAT_DLL_DIRS="C:\Program Files\VEGAS\VEGAS Pro 15.0"` for the second pass).

## Licence position

The six `com.sonycreativesoftware` bundles are VEGAS Pro's own code, licensed to the user for use with VEGAS Pro under the VEGAS licence agreement. `HitFilmVegasBasic.ofx` is FXHOME's, bundled with VEGAS under FXHOME's terms and keyed to VEGAS (its `HFPL` licence folder sits in the bundle). Milan Pro does not bundle, copy or redistribute any of these files, loads them only from where VEGAS installed them and only when asked to, and never works around a host or licence check: the probe below ran each binary exactly as any OFX host would, with Milan Pro's own host name, and recorded what happened. Adding `C:\Program Files\VEGAS\VEGAS Pro 15.0` to the DLL search path for the second pass was done only to see how far the plug-ins get; Milan Pro itself never does that.

## How the probe ran

`engine::ofx::tests::vegas15_compat_report` (`src/engine/ofx/tests.rs`) scans `C:\Program Files\VEGAS\VEGAS Pro 15.0\OFX Video Plug-Ins` read-only and runs every binary in `milan-plugin-host.exe` as Milan Pro does (load, each plug-in's load and describe actions, describeInContext in the context Milan Pro would use, then one 96x54 frame: a filter on a gradient, a transition at p = 0.5, a generator), with these probe settings (`client::PROBE_LOG`, `main.rs::probe_mode`):

- the host is started with `CREATE_NO_WINDOW` and no console, as always, and in the Job Object so it dies with the test;
- its main thread moves to a hidden desktop of its own (`CreateDesktopW("milan-ofx-probe")` + `SetThreadDesktop`) before any plug-in code runs, so a licence or registration dialog cannot appear on the user's screen or take focus;
- `kOfxImageEffectHostPropIsBackground` is 1 (plug-ins are told nobody is watching);
- a watcher thread notes every visible window the plug-in opens on that thread in `host-dialogs.log` ("dialog: <title>"), so the report can tell a dialog from a hang;
- the watchdog kills the host after 10 s (load), 10 s (describe) or 20 s (render) and the row says so.

Two passes: first with the bundle's own folder as the only extra DLL search path (what Milan Pro does), then with the VEGAS program folder on `PATH` for the probe hosts (`MILAN_COMPAT_DLL_DIRS`), because `Vfx1.ofx`, `Filters.ofx` and `Stabilize.ofx` import VEGAS's `ErrorReport.dll`, `OpenColorIO.dll` and `proDADMercalli20.dll` from there (read from their PE import tables) and cannot load without them. No dialog was seen and nothing hung in either pass; each pass took under a second.

## Results per bundle

| Bundle | Plug-ins exported | Loads | Describes | Renders | Why not |
|---|---|---|---|---|---|
| `Vfx1.ofx` | 67 (the `Vfx1.xml` resource names 96 effects; 29 of them are not in this binary) | Pass 1: no (`LoadLibraryExW` fails: it imports `ErrorReport.dll` and `OpenColorIO.dll` from the VEGAS folder). Pass 2: yes, 66 of 67 (plug-in 0's load action fails) | 8 of 67 (LAB Adjust, LUT Filter, Radial Pixelate, Star Wipe, Layer Dimensionality, Vignette, Picture In Picture, Crop); the other 58 fail describeInContext with `kOfxStatErrMissingHostFeature` | 0: the 8 fail their render action with `kOfxStatErrMissingHostFeature` | They ask for `OfxVegasEffectSuite v1`, `OfxVegasKeyframeSuite v1`, `OfxVegasProgressSuite v2` and `OfxVegasStereoscopicImageEffectSuite v1` (VEGAS's private suites) and read the host properties `OfxPropName` and `OfxPropVegasSpikeKey`, a VEGAS-private host key Milan Pro does not have and will not fake. Needs VEGAS. |
| `Filters.ofx` (Stereoscopic 3D Adjust) | 1 | Pass 1: no (`ErrorReport.dll`). Pass 2: the DLL loads; the plug-in's load action fails | no | no | The same four VEGAS suites. Needs VEGAS. |
| `Stabilize.ofx` | 1 | Pass 1: no (`proDADMercalli20.dll`, `ErrorReport.dll`). Pass 2: the DLL loads; the load action fails | no | no | The same four VEGAS suites. Needs VEGAS. |
| `TitlesAndText.ofx` | 1 | DLL yes; load action fails | no | no | The same four VEGAS suites (and `ScriptPortal.MediaSoftware.TextGen.OFXInterop.dll`, .NET, after that). Needs VEGAS. |
| `spica_cutout.ofx` (Smart Zoom) | 1 | DLL yes; load action fails | no | no | Asks for `OfxOpenCLProgramSuite v1` (VEGAS's OpenCL suite). Needs VEGAS. |
| `spica_resizer.ofx` (Smart Upscale) | 1 | DLL yes; load action fails | no | no | `OfxOpenCLProgramSuite v1`. Needs VEGAS. |
| `HitFilmVegasBasic.ofx` (7: Bleach Bypass, Light Flares, Scan Lines, Three Strip Color, TV Damage, Vibrance, Witness Protection) | 7 | yes (load actions pass) | no: every describe action returns `kOfxStatErrMissingHostFeature` | no | It fetched no suite and asked for no host property that Milan Pro lacks (the interact suite it fetches is provided as a stub); the only host property it read before stopping is `OfxPropName`, the host's name. HitFilm Vegas Basic is the VEGAS edition, with its licence folder in the bundle: it runs in VEGAS only, by its own check of the host's name. Needs VEGAS. |

Totals: 79 plug-ins exported by 7 binaries; 73 pass their load action (pass 2), 8 describe, 0 render. Section 4.2 of `docs/REMAINING.md` counted 102 from the XML resources; the binaries export 79.

Every first-party plug-in reads `OfxPropVegasSpikeKey` from the host. Whatever that key is (the name suggests a host verification value), providing or guessing it would be working around VEGAS's check, which Milan Pro does not do.

## What the probe fixed in the host

Running these binaries found and fixed three host gaps, which help other plug-ins:

- a load failure now carries the OS error (so "a DLL it imports is missing" is visible instead of a bare `LoadLibraryExW failed`);
- an error from a plug-in action names every suite (and host property) the plug-in asked for and did not get, so the Plug-In Manager says what a plug-in needs instead of only `kOfxStatErrMissingHostFeature`;
- the interact suite (overlays) is offered as a stub, because plug-ins that fetch it in their describe action otherwise stop there even though Milan Pro draws no overlays.

## Not done

- VEGAS's private suites (`OfxVegas*`, `OfxOpenCLProgramSuite`) are not implemented: they are undocumented outside VEGAS's SDK, and implementing them to run VEGAS-licensed plug-ins outside VEGAS is not a goal. Should the VEGAS Pro OFX SDK headers be published under terms that allow it, the keyframe, progress and effect suites are small; the stereoscopic and OpenCL ones are not.
- The R25 `vegas_map` (VEGAS effect → Milan built-in) does not exist yet; nothing was seeded.
- HitFilm's check is of the host's name (`OfxPropName`); no host identity was faked to confirm what it accepts, and none will be.
