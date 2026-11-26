# Milan Pro: hosting After Effects / Premiere Pro effect plug-ins

Research and implementation plan. Written 2026-11-13 from web reading only: no SDK, installer or binary was
downloaded or run. Anything not confirmed by a cited page is marked **UNVERIFIED**. This is not legal advice.

Adobe, After Effects and Premiere Pro are trademarks of Adobe Inc. Milan Pro is not affiliated with or endorsed by Adobe.

Contents: 1 Technical background · 2 Legal position · 3 Feasibility tiers · 4 Architecture · 5 Batches · 6 User text and matrix · 7 Sources

---

## 0. Summary

- The only realistic target is the **After Effects effect API** (`.aex`, the `PF_` API) in the dialect **Premiere Pro**
  hosts: 8-bit ARGB (optionally 32-bit float through Premiere's pixel-format suite), no SmartFX, no AEGP, no 16-bit.
  Adobe's own guide describes that subset and says other hosts identify as `'PrMr'` [S4][S9].
- Adobe's position, verbatim: "Adobe neither supports nor recommends the creation of Adobe-compatible third-party
  hosts" [S10]. They do not say it is forbidden, and commercial hosts have done it (EDIUS, HitFilm Pro 14) [S20][S22].
- The Adobe SDK (headers, samples, shipped docs) may not be redistributed, modified, ported or translated, and may
  not be combined with open source in a way that forces its disclosure [S1 §4.1, §6.1, §6.6]. So: **no SDK file, and
  no translation of an SDK file, in the repo.** Bindings are written by hand from public documentation.
- Honest expectation: simple CPU filters work; plug-ins with custom UI work with generic controls only; SmartFX-only,
  AEGP/camera-dependent, GPU-only and host-locked plug-ins (most of the famous commercial ones) do not.
- 9 batches, A1-A9; A1-A5 give a useful, testable result with open-source plug-ins only.

---

## 1. What "AE / Premiere plug-ins" are

### 1.1 After Effects effect plug-ins (`.aex`)

A Windows `.aex` is a DLL. It contains a **PiPL** resource and one exported entry point.

**Entry point** [S3]:

```c
PF_Err main(PF_Cmd cmd, PF_InData *in_data, PF_OutData *out_data,
            PF_ParamDef *params[], PF_LayerDef *output, void *extra);
```

The name is whatever the PiPL says (commonly `EffectMain`). `params[0]` is the input image. `extra` depends on the
command (events, SmartFX, arbitrary data, supervision). Newer SDKs also export `PluginDataEntryFunction`
(/`…2`), which registers name, match name and category by callback [S13]; its exact signature is **UNVERIFIED**
(the public page describes it but does not give the prototype).

**PiPL** (Plug-In Property List): tells the host about the plug-in "without executing the plug-in" [S2]. Properties:
Kind (`AEEffect`), Name, Category, entry point per platform, `AE_PiPL_Version`, `AE_Effect_Spec_Version`,
`AE_Effect_Version`, `AE_Effect_Info_Flags`, `AE_Effect_Global_OutFlags`, `AE_Effect_Global_OutFlags_2`,
`AE_Effect_Match_Name` (the stable id), `AE_Reserved_Info`, `AE_Effect_Support_URL` (23.5+) [S2].
Binary layout, from the open-source `pipl` crate (MIT/Apache) [S17]: on Windows two bytes `01 00`, a 4-byte version
(0), a 4-byte property count; then per property vendor fourcc (`8BIM`), key fourcc, 4 bytes of zero, 4-byte length,
contents; little-endian with the fourcc bytes reversed. Keys: `kind` (value `eFKT` for AE effects), `name`, `catg`,
`8664` (Win64 entry-point name), `ePVR`, `eSVR`, `eVER`, `eINF`, `eGLO`, `eGL2`, `eMNA`, `aeFL`, `eURL`.
Premiere kinds: `SPFX`, `VFlt`, `AFlt`, `IMPT`, `CMPM`, `RECM`, `PLYM`, `DevC`, `ExpM`, `ExpD` [S17].
Reading it without running the DLL: map the file as data (`LoadLibraryExW(…, LOAD_LIBRARY_AS_DATAFILE)` +
`FindResourceW`, or parse the PE `.rsrc` section; the unmerged `r31-plugin-manager` branch has a PE reader).
The resource type name `PiPL` and id 16000 are what SDK-built plug-ins use as far as I know: **UNVERIFIED**, so the
parser must enumerate every resource whose type is the string `PIPL` rather than hard-code the id.
A plug-in may hold several PiPLs (several effects per file); Premiere does not support that [S7].

**PF_Cmd selectors** [S5]:

| Group | Selectors |
|---|---|
| Global | `PF_Cmd_ABOUT`, `GLOBAL_SETUP`, `GLOBAL_SETDOWN`, `PARAM_SETUP` (the page spells it `PF_Cmd_PARAM_SETUP`; the task's `PARAMS_SETUP` is the SDK header spelling: **UNVERIFIED** which is canonical) |
| Sequence (per instance) | `SEQUENCE_SETUP`, `SEQUENCE_RESETUP` (after load/copy: unflatten), `SEQUENCE_FLATTEN` (before save), `SEQUENCE_SETDOWN` |
| Frame | `FRAME_SETUP` (may resize output), `RENDER` (8/16-bit), `FRAME_SETDOWN`; audio: `AUDIO_SETUP`/`AUDIO_RENDER`/`AUDIO_SETDOWN`; SmartFX: `SMART_PRE_RENDER`, `SMART_RENDER` |
| Messaging | `EVENT` (custom UI), `USER_CHANGED_PARAM` (needs `PF_ParamFlag_SUPERVISE`), `UPDATE_PARAMS_UI`, `DO_DIALOG`, `ARBITRARY_CALLBACK`, `GET_EXTERNAL_DEPENDENCIES`, `COMPLETELY_GENERAL`, `QUERY_DYNAMIC_FLAGS`, `GPU_DEVICE_SETUP`, `GPU_DEVICE_SETDOWN`, `SMART_RENDER_GPU` |

Order: GLOBAL_SETUP, PARAM_SETUP, then SEQUENCE_SETUP per instance, then per frame FRAME_SETUP → RENDER →
FRAME_SETDOWN (or one or more SMART_PRE_RENDER then at most one SMART_RENDER). "Only the first few selectors are
predictable" [S5].

**PF_InData** [S6]: `inter` (interaction callbacks), `utils` (graphics/maths callbacks, "always defined"), `effect_ref`,
`quality`, `version`, `serial_num`, `appl_id` ("After Effects uses 'FXTC' and Premiere Pro and other hosts use
'PrMr'"), `num_params`, `current_time`/`time_step`/`total_time`/`local_time_step` in units of `time_scale` per second
(time_step may be 0 or negative), `field`, `shutter_angle`/`shutter_phase`, `width`/`height` (source size, not
downsampled), `extent_hint`, `output_origin_x/y`, `downsample_x/y`, `pixel_aspect_ratio`, `global_data`,
`sequence_data`, `frame_data`, audio fields, `pica_basicP` (the `SPBasicSuite`), `pre_effect_source_origin_x/y`.
**PF_OutData**: `out_flags`/`out_flags2` (must match the PiPL), `my_version`, `num_params`, `global_data`,
`sequence_data`, `frame_data`, `width`/`height`/`origin` (when the effect grows the output), `return_msg`.
The exact struct layouts (field order, reserved fields, packing) are **not** fully given by the public guide: see 2.4.

**Parameters** [S8]: `PF_Param_LAYER` (another layer; params[0] is one), `SLIDER` (obsolete), `FIX_SLIDER`
(deprecated, 16.16 fixed), `FLOAT_SLIDER` (recommended, double), `ANGLE` (fixed-point degrees, may exceed 360),
`CHECKBOX`, `COLOR` (8-bit `PF_Pixel`, RGB only; float via `PF_ColorParamSuite1`), `POINT` (fixed, layer space, origin
upper left; float via `PF_PointParamSuite1`), `POPUP` (1-based index, item names in one separated string: the
separator is `|` to my knowledge, the fetched page rendered it as "/": **UNVERIFIED**), `ARBITRARY_DATA` (opaque
handle managed through `PF_Cmd_ARBITRARY_CALLBACK`: new, dispose, copy, flatten, unflatten, interpolate, compare,
print, scan), `PATH` (a mask on the layer), `GROUP_START`/`GROUP_END`, `BUTTON`, `POINT_3D` (not in Premiere).
`PF_ParamDef` has `id`, `change_flags`, `ui_flags` (`PF_PUI_TOPIC`, `CONTROL`, `STD_CONTROL_ONLY`, `NO_ECW_UI`,
`DISABLED`, `INVISIBLE`, …), `ui_width/height`, `param_type`, `name[32]`, `flags` (`CANNOT_TIME_VARY`,
`CANNOT_INTERP`, `SUPERVISE`, `START_COLLAPSED`, …) and the union [S8b].

**Pixels** [S11]: `PF_EffectWorld` (= `PF_LayerDef`): `world_flags` (`PF_WorldFlag_DEEP` = 16-bit), `data`,
`rowbytes` (use it, never assume `width*4`; input and output may differ), `width`, `height`, `extent_hint`,
`pix_aspect_ratio`. Channel order A, R, G, B. 8-bit, 16-bit (0..32768) and 32-bit float; float only through SmartFX
in After Effects [S12]. Row 0 is the top row and alpha is straight/premultiplied: **UNVERIFIED** from the public
pages; A4 settles both with a test plug-in and an open-source plug-in before any conversion is trusted.

**Interaction callbacks** [S14]: `PF_ADD_PARAM` (during PARAM_SETUP), `PF_ABORT`, `PF_PROGRESS`,
`PF_CHECKOUT_PARAM(index, what_time, step, time_scale, &param)` / `PF_CHECKIN_PARAM` (a parameter value or another
layer's frame at any time; a layer set to none returns zeros; re-entrant renders can result), `PF_REGISTER_UI`,
`PF_CHECKOUT_LAYER_AUDIO` / `PF_GET_AUDIO_DATA`.

**SmartFX** [S12]: `PF_OutFlag2_SUPPORTS_SMART_RENDER`; pre-render gets a request rect and bit depth and must return
`result_rect`, `max_result_rect`, and check out every input (`checkout_layer` with a unique id); render calls
`checkout_layer_pixels`, `checkout_output`. It is "the way to implement 32-bit per channel support in After
Effects". "As of this writing, no third party hosts support SmartFX, or our AEGP functions" [S9].

**Custom UI** [S15]: `PF_OutFlag_CUSTOM_UI`, `PF_REGISTER_UI`, `PF_Cmd_EVENT` with `PF_Event_NEW_CONTEXT`, `ACTIVATE`,
`DO_CLICK`, `DRAG`, `DRAW`, `DEACTIVATE`, `CLOSE_CONTEXT`, `IDLE`, `ADJUST_CURSOR`, `KEYDOWN`, `MOUSE_EXITED`; drawing
goes through the host's Drawbot suites, in the Effect Controls window or as an overlay on the comp/layer view.

**Multi-frame rendering** [S16]: `PF_OutFlag2_SUPPORTS_THREADED_RENDERING`; sequence data is const during render
(`PF_EffectSequenceDataSuite1`); global data written only in GLOBAL_SETUP/SETDOWN. A host may always stay
single-threaded per instance, which is what Premiere does ("a critical section serializes calls to each instance")
[S18].

### 1.2 Suites

`in_data->pica_basicP` is an `SPBasicSuite`; plug-ins call `AcquireSuite(name, version, &ptr)` / `ReleaseSuite`.
A missing suite is an ordinary error the plug-in is told to handle ("try the latest version and fall back… warn the
user and return an error") [S19]. Suites an ordinary effect asks for (names from the guide's chapter titles and the
Premiere pages [S18][S19][S7]; version numbers and function tables are **UNVERIFIED** until read from a public source
in A2):

| Suite | Need | Plan |
|---|---|---|
| Callbacks in `in_data->utils` (copy, fill, blend, convolve, iterate, sampling) and `in_data->inter` | Almost every effect | A3 |
| PF Iterate8 / Iterate16 / IterateFloat | Very common | A3 (8), A6 (16/float) |
| PF World Suite, PF Pixel Data Suite, PF Handle Suite, memory (host handles) | Very common | A3 |
| PF ANSI callbacks (maths, sprintf, strcpy) | Common in older effects | A3 |
| PF Param Utils (`UpdateParamUI`, changed-param queries), PF App Suite (colours, cursors, version) | Supervised / custom-UI effects | A5 |
| PF Color Callbacks / Colour-space conversion, PF ColorParam, PF PointParam, PF AngleParam | Common | A3 |
| PF World Transform, PF Fill Matte, PF Sampling 8/16/Float | Common; absent in Premiere, which offers `PF_COPY`/`PF_FILL` macros instead [S18] | A3 (transform/fill), sampling A6 |
| PF Effect UI, Effect Custom UI, Drawbot suites | Custom UI | Not provided (tier b) |
| PF Path Query / Path Data | Mask-driven effects | Not provided |
| PF AE Channel Suite (depth, normals…) | 3D-pass effects; absent in Premiere [S7] | Not provided |
| AEGP suites (`AEGP_PFInterfaceSuite`, Comp, Layer, Stream, Camera, Light, `AEGP_RegisterWithAEGP`) | Camera/light/comp-aware effects [S21] | Not provided (tier d) |
| Premiere: PF Pixel Format Suite (`AddSupportedPixelFormat`), PF Utility Suite (source frame rate, field type, clip speed, `EffectWantsCheckedOutFramesToMatchRenderPixelFormat`), PF Transition Suite (`RegisterTransitionInputParam`), App Info Suite | Premiere-aware effects [S23][S24] | A6, A7 |
| PF GPU Device Suite, PrGPU suites | GPU effects | Not provided (tier e) |

### 1.3 AEGP plug-ins (out of scope)

AEGPs ("general plug-ins") script and extend After Effects itself: menus, panels, project/comp/layer access, render
queue, I/O (AEIO), 3D renderers (Artisans). They talk to After Effects' project model, which Milan does not have, and
"No AEGP calls work in Premiere" either [S18]. Out of scope entirely; effects that need AEGP suites are tier d.

### 1.4 Premiere Pro's own APIs

- Premiere "support[s] the After Effects effect API", the "key omissions being 3D-related calls, plus 16-bit and
  SmartFX support" and the AEGP utility functions; `appl_id` is `'PrMr'`; `PF_InData.version` is 13.4 for CC-CC 2019
  [S4]. Unsupported: `extent_hint`, multiple PiPLs, AEGP, audio (a plug-in setting `PF_OutFlag_I_USE_AUDIO` is not
  loaded), `PF_ParamFlag_COLLAPSE_TWIRLY`, PF World Transform Suite, PF AE Channel Suite, AE's high-bit-depth path,
  SmartFX, 3D, `PF_SUBPIXEL_SAMPLE()`, `PF_GET_PIXEL_DATA16()` [S7].
- Differences a host in that dialect may copy [S18]: time_scale/time_step differ from AE (only the ratio matters); a
  low-quality render then a full one; one RENDER per field with `height` halved and `rowbytes` doubled; frames out of
  order; calls to one instance serialised.
- Pixel formats [S23][S25]: an 8-bit ARGB-only effect sees AE-like behaviour. Others register formats with
  `AddSupportedPixelFormat()` in GLOBAL_SETUP: `BGRA_4444_8u` (Premiere's native), `VUYA_4444_8u(_709)`,
  `ARGB_4444_8u`, `BGRA_4444_32f`, `VUYA_4444_32f(_709)`, premultiplied families (`BGRP`, `VUYP`, `PRGB`), packed YUV.
  BGRA formats are not premultiplied; uncompressed PPix buffers have a lower-left origin [S25].
- GPU: a `PrGPUFilter` entry point added to an AE-API effect; the software effect still defines parameters and UI and
  is the fallback; frames stay on the GPU under the Mercury Playback Engine (CUDA; OpenCL/Metal; DirectX 12 in
  development) [S26].
- Transitions: AE-API effects that call `PF_TransitionSuite::RegisterTransitionInputParam()` before `PF_ADD_PARAM`
  for the layer parameter that is the other clip [S24].
- Premiere-only plug-ins (`.prm` on Windows: extension **UNVERIFIED**, the public pages say only "DLLs"):
  importers, exporters, export controllers, transmitters, legacy video filters ("we strongly recommend using the After
  Effects SDK to develop effects plugins"), control surfaces [S27]. Audio effects are VST3 (VST 2.4 before CC) [S27].

### 1.5 Where plug-ins are installed on Windows

- Shared folder: registry `HKLM\SOFTWARE\Adobe\After Effects\[version]\CommonPluginInstallPath` (version "locked at
  7.0 for all CC versions") [S28] and `HKLM\Software\Adobe\Premiere Pro\10.0\CommonPluginInstallPath`, example value
  `C:\Program Files\Adobe\Common\Plugins\7.0\MediaCore\` (older installs: `…\Plug-ins\7.0\MediaCore`; Premiere 22
  renamed `Plug-ins` to `Plugins` and still loads both) [S29].
- After Effects only: `HKLM\SOFTWARE\Adobe\After Effects\(version)\PluginInstallPath` (the app's `Plug-ins` folder) [S28].
- After Effects recurses 10 levels, does not follow shortcuts, skips folders whose names end in parentheses or start
  with `~` [S28]. Premiere reads `Blocklist.txt` files in the folder [S29].

---

## 2. Legal and licensing

Not legal advice. The project owner should have a lawyer read this section before A1 is merged.

### 2.1 Adobe's SDK terms (Adobe Developer Terms of Use, last updated 18 June 2024 [S1]; whether a newer version exists is UNVERIFIED)

- §2.23: "Developer Tools" are provided "for the purpose of accessing, interoperating with, or adding functionality
  to other Adobe Software and Adobe Services" and include SDK files, "header … files", "Sample Code" and "related
  documentation, technical specifications".
- §4.1(A): licence to "use and reproduce the Developer Tools for the development and testing of your Developer
  Software" and to distribute them "solely in and with your approved Developer Software in object code form only".
  "Developer Software" (§2.22) is software "intended to access, function or interoperate with … Adobe Software". A
  competing host is not that, so the licence does not cover using the SDK to build one.
- §6.1: no "modify, port, adapt, or translate any portion of any Developer Tools"; no reverse engineering, "including
  … monitoring or tracking the inputs and outputs flowing through a system or an application in order to recreate
  that system".
- §6.3 no unbundling/repackaging; §6.6 no combining with software in a way that would require the Developer Tools to
  be disclosed in source form or redistributed at no charge ("GPL … or other open-source licenses"); §6.8 no
  sublicensing.
- An Adobe employee on Adobe's forum: developers are "free to distribute plugins built using SDK headers and source
  files", but may not distribute the headers and source files themselves [S30].

Answers to the questions asked:

| Question | Answer |
|---|---|
| Can SDK headers go into an MIT repo? | **No.** §4.1 allows object code only, inside Developer Software; §6.3/§6.6/§6.8 forbid the rest. |
| Can a contributor who accepted the terms translate the headers to Rust? | **No** for that person: §6.1 ("port, adapt, or translate"). Such a file would also be a derivative of Adobe's copyright work. |
| Can a third party implement a compatible host at all? | Nothing found forbids it for someone who has **not** accepted the Developer Terms, and Adobe's guide acknowledges such hosts exist [S9][S10]. Adobe "neither supports nor recommends" it. Risk is not zero: see 2.4. |
| Is clean-room re-declaration needed? | **Yes**, and the people writing the bindings should not have accepted the Developer Terms or have the SDK open. |

### 2.2 Precedent (each verified or marked)

| Host | AE-API plug-ins? | Evidence |
|---|---|---|
| Adobe Premiere Pro / Elements | Yes (first party) | [S4] |
| Grass Valley EDIUS | **Yes**: "After Effects Plug-in Bridge", user registers a folder; "Only 64-bit After Effects plug-ins are supported. Operations of all After Effects plug-ins are not guaranteed." (EDIUS 7 manual; current versions UNVERIFIED) | [S20] |
| FXhome HitFilm Pro 14 (Nov 2019) | **Yes, six named plug-ins only** (Video Copilot Element 3D, Saber, Orb, Optical Flares, Heat Distortion; Red Giant Trapcode Particular); manual tells users to install After Effects (a trial is enough) and then the plug-ins. Whether FXhome had agreements with the vendors or Adobe: UNVERIFIED. A 2021 version history lists the feature for removal: UNVERIFIED. | [S22] |
| eyeon Fusion 5.x | Probably yes, historically: only forum posts found (an "AEPlugins" folder in Fusion 5.3; "moving forward we will no longer support the AE plugin set" when Fusion went 64-bit). **UNVERIFIED** from a primary source. | [S31] |
| Blackmagic Fusion 8+ / DaVinci Resolve | No evidence of AE-API hosting; they are OpenFX hosts. | [S31][S32] |
| Natron | No: OpenFX only. | [S32] |
| VEGAS Pro | No evidence; OpenFX (and its own older formats). | [S32] |
| Boris FX products (RED, Continuum) | Boris ships plug-ins *for* AE/Premiere/Avid/OFX hosts. Boris RED hosting third-party AE filters: **UNVERIFIED** (not found). | [S33] |
| Autodesk Combustion, Apple Final Cut Pro 1-7 / Motion, Avid (via Elastic Gasket) | Widely remembered as AE-plug-in hosts or adapters; **UNVERIFIED**, no source found in this pass. Do not cite as precedent. | none |
| aexlo.js / aexlo-rs (open source, MIT) | An "Emulator of After Effects SDK" that loads `.aex`; archived May 2025; its README says "These may run into some kind of copyright issue." Whether it contains Adobe headers: UNVERIFIED. | [S34] |

Precedent shows it has been done commercially without public legal action that I could find; it does not show it is
safe. Absence of evidence of enforcement is all that was verified.

### 2.3 Plug-in vendors' licences

- **Maxon / Red Giant EULA** [S35]: no clause restricting hosts was found; §12.2 "The User may not remove or
  circumvent the protective measures"; §10.3 no decompiling/reverse engineering.
- **Video Copilot**: "our Plug-ins are only compatible with Adobe After Effects" (Optical Flares also for Nuke) [S36].
  That is a compatibility statement; the EULA text was not read: UNVERIFIED.
- **Boris FX, RE:Vision Effects, Sapphire**: licences are sold per host family (Adobe / Avid / OFX …) [S33]; EULA
  clauses not read: **UNVERIFIED**. These vendors sell OpenFX builds; Milan users should use those through R33.
- **Adobe's own bundled effects** (match names beginning `ADBE `, files inside the After Effects / Premiere program
  folders) are part of Adobe's applications and licensed with them. Milan must not load them.
- Practical point: many commercial plug-ins check the host (`appl_id`, host version, AEGP presence, the process
  name) or their licensing component expects an Adobe host. If such a check fails, Milan reports it and stops.

### 2.4 Interoperability law in broad terms

- US: *Google v. Oracle* (2021) held that copying API declarations to let programmers use a familiar interface was
  fair use on those facts [S37]. It is a fair-use ruling, fact-specific, not a rule that interfaces are free.
- EU: *SAS Institute v World Programming* (C-406/10): functionality, programming languages and data file formats are
  not protected as such [S38]; Directive 2009/24/EC art. 5(3) and 6 allow observation and, under conditions,
  decompilation for interoperability [S39].
- Contracts are separate from copyright: a person who accepted the Developer Terms is bound by §6.1 regardless.
- The weak point of any "from documentation" approach: the public guide [S40] documents selectors, fields and
  behaviour but not every struct layout, constant value or suite version. Three possible sources, in order of
  safety: (1) the public guide; (2) permissively licensed third-party source that uses the API (the `after-effects`
  crate ships "pre-generated bindings", licensed Apache-2.0/BSD-3/MIT/Zlib by its author [S41]; they were generated
  from Adobe headers, so the author's licence cannot remove Adobe's rights in whatever is protectable in them:
  treat as a reference for facts such as offsets and numeric values, never copy text, names of local variables or
  comments, and never add the crate as a dependency of the host); (3) measuring an open-source plug-in built by its
  authors (field offsets observed at run time). Record the source of every constant in a provenance table.

### 2.5 Trademarks

Nominative use only. Say "loads effect plug-ins written for the Adobe After Effects effect API" or "After
Effects-format (.aex) effect plug-ins". Do not say "After Effects compatible", "Adobe compatible", "works with
Premiere plug-ins" as a headline claim, do not use Adobe logos or product icons, do not put Adobe product names in
Milan's name, menus' top level, or installer. Carry the notice "Adobe, After Effects and Premiere Pro are trademarks
of Adobe Inc. Milan Pro is not affiliated with, endorsed or sponsored by Adobe." in README, About and the Plug-In
Manager pane. In the UI the plug-in kind is labelled "AE-format effect".

### 2.6 Recommended position

1. **No Adobe material in the repo or in release artefacts**: no SDK header, sample, PiPL tool, shipped doc, or
   translation of one; no `after-effects-sys`/`premiere-sys` dependency in Milan or its host.
2. **Own `#[repr(C)]` bindings**, hand-written, each item carrying a provenance note (public guide URL, or
   "observed with tools/aex-test-plugin"). Contributors to `aex/abi.rs` confirm in the PR that they have not accepted
   the Adobe Developer Terms and did not consult the SDK. A `CONTRIBUTING` rule and a CI grep for Adobe copyright
   strings enforce it.
3. **The user supplies plug-ins** they have licensed. Milan ships none, downloads none, and never copies a plug-in.
4. **No circumvention**: no patching, no emulation of licensing libraries, no faked serial numbers
   (`in_data->serial_num` stays 0), no hooking of plug-in code, no spoofed process name. If a plug-in refuses the
   host, the report says "refused by the plug-in" and Milan stops.
5. **Host identity is honest but interoperable.** The API has two dialects selected by `appl_id`; the guide says
   "Premiere Pro and other hosts use 'PrMr'" [S6]. Default: the documented third-party-host convention `'PrMr'`
   with Milan's own suites; the About/compat text states this plainly. Milan never reports `'FXTC'` by default.
   **Decision for the owner (with counsel):** whether an opt-in per-plug-in "After Effects dialect" (needed for
   SmartFX-only effects, tier c) is acceptable. Until decided, A8 stays unbuilt.
6. **Adobe's own effects are blocked** (match name prefix `ADBE `, or a path under an Adobe application folder),
   status "Blocked: licensed for Adobe applications".
7. **Compatibility is per plug-in and evidence-based** (2.10.26 "no fake compatibility claims"): untested plug-ins
   show "Untested"; nothing is called supported without a recorded test.
8. **Feature is off by default** in the first release ("Experimental: AE-format effects" in Plug-In Manager ›
   Folders), with the trademark notice and the "Adobe does not support third-party hosts" sentence next to the switch.
9. Prefer the vendor's **OpenFX** build whenever one exists; the Plug-In Manager says so when it sees both.

Residual risks to state openly: Adobe may object regardless; a vendor may object to their plug-in running in an
unlisted host; the provenance of layout facts (2.4) is the part a lawyer should look at hardest.

---

## 3. Feasibility tiers

| Tier | What | Expectation | Why |
|---|---|---|---|
| a | CPU effects using `utils`/`inter` callbacks, iterate, world, handle, ANSI, colour suites; 8-bit ARGB; plain parameters | **Feasible** (A1-A5). The Premiere dialect is exactly this. | Small, documented surface; hosts have done it [S20]. |
| b | Custom ECW/comp UI, ARB parameters, sequence data, supervision | **Partly.** Sequence data and flatten/unflatten: yes. ARB data: stored and passed back opaque, default value only unless the plug-in's own UI can edit it. Custom-drawn controls: not shown; standard parameters still get generic controls. Supervision (`USER_CHANGED_PARAM`, `UPDATE_PARAMS_UI`): yes for values, enable/disable and visibility. | Drawbot and the event model are a large undocumented-in-detail surface; an out-of-process plug-in cannot draw into Milan's egui window. |
| c | SmartFX, 16-bit and 32-bit float | **Float via the Premiere route** (PF Pixel Format Suite, `BGRA_4444_32f` in `PF_Cmd_RENDER`): feasible (A6) for plug-ins that implement it. **SmartFX** (A8): technically feasible for single-input effects (pre-render/render with rect bookkeeping), but it means presenting the After Effects dialect: blocked on the 2.6(5) decision. 16-bit: only with SmartFX/AE dialect. | "No third party hosts support SmartFX" [S9]; many modern AE-only effects implement SmartFX only and have no `PF_Cmd_RENDER` path. |
| d | Effects needing AEGP, comp, camera, lights, other layers' transforms, masks/paths, 3D channels | **Infeasible** in any honest form. Stub = `AcquireSuite` fails; well-behaved plug-ins then degrade or refuse. A fake camera/comp would produce wrong pictures and be a "fake compatibility claim". | Milan has no comp/camera/light model; "No AEGP calls work in Premiere" either [S18]. |
| e | GPU effects (`PrGPUFilter`, `PF_Cmd_SMART_RENDER_GPU`) | **Not hosted.** Their CPU fallback is used when it exists in tier a/c. Plug-ins that create their own GL/Vulkan/D3D context off-screen (e.g. shader runners) may work in the host process: per-plug-in luck. | Would need Mercury-style device sharing and the PrGPU suites; out of proportion. |
| f | Premiere-only `.prm` importers, exporters, transmitters, legacy video filters, device control | **Out of scope.** Importers/exporters duplicate FFmpeg; they are licensed for Adobe hosts and bind to Premiere's media core. Legacy `.prm` video filters are obsolete [S27]. AE-API transitions (A7) are the only Premiere-specific item worth doing. Audio: VST3 through R34. | |

### 3.1 Popular plug-ins: likely outcome (predictions, all UNVERIFIED until tested)

| Plug-in | Likely | Reason |
|---|---|---|
| Video Copilot Element 3D, Optical Flares (lights), Saber, Orb | Not work | AE-only per vendor [S36]; camera/lights/AEGP (tier d), own licensing, custom UI. |
| Maxon Red Giant Trapcode Particular / Form | Not work | AE comp camera, AEGP, custom Designer UI. |
| Red Giant Magic Bullet Looks / Colorista, Universe | Unlikely | Premiere builds exist, but GPU pipelines, custom UI windows and Maxon App licensing. Use the OFX builds where sold. |
| Boris Continuum, Sapphire, Mocha Pro | Do not attempt | OpenFX builds exist; licence is per host family. Use R33. |
| RE:Vision Twixtor, ReelSmart Motion Blur | Unlikely | Need neighbouring frames (possible, A5) but per-host licensing; OFX builds exist. |
| Neat Video | Do not attempt | Licensed per host; OFX build exists. |
| Plexus, Stardust, Newton | Not work | AE comp model (tier d). |
| Deep Glow, simple glows/blurs/keyers from small vendors | Mixed | SmartFX-only → tier c; some have licence checks through AEGP. |
| Older "classic" 8-bit effects that also run in Premiere | Best chance | Tier a by construction. |
| ntsc-rs (open source) | Good | Runs in AE and Premiere, plain parameters [S42]. |
| Gyroflow plug-in (open source) | Uncertain | Zero-copy GPU path and file parameters [S41][S43]; has an OFX build that R33 should run instead. |
| F's Plugins (open source, many effects) | Mixed | Windows-only AE effects, mostly simple pixel filters; AE-dialect assumptions likely [S44]. |

### 3.2 Open-source AE-API plug-ins usable as legal test cases

Licences as stated on the project pages; re-check each LICENSE file in A1 before use. None is copied into the repo:
tests locate them in a folder named by `MILAN_AEX_TEST_DIR` and are `#[ignore]` otherwise.

| Project | Licence (as stated) | Notes |
|---|---|---|
| ntsc-rs, github.com/valadaptive/ntsc-rs | LICENSE-MIT, LICENSE-ISC, LICENSE-APACHE-2.0 files (how they combine: UNVERIFIED) | "can be used as an After Effects, Premiere, or OpenFX plugin"; whether releases contain a Windows `.aex`: UNVERIFIED [S42]. Also an OFX build → output can be compared between R33 and the AE path. |
| after-effects crate examples (virtualritz/after-effects) | Apache-2.0 OR BSD-3-Clause OR MIT OR Zlib [S41] | 20 examples mirroring the classic samples (`simplest`, `sdk_noise`, `paramarama`, `checkout`, `supervisor`, `persisto`, `resizer`, `gamma_table`, `convolutrix`, `custom_ecw_ui`, …). Must be built by the tester (`cargo`, no Adobe SDK needed: "Pre-generated bindings are included"). Best tier-a/b conformance set. Build output stays outside the repo. |
| F's Plugins (bryful/F-s-PluginsProjects) | MIT (README) [S44] | Prebuilt Windows `.aex` in releases. Its "Legacy" folder includes Adobe SDK files: never vendor the repo. |
| ISF4AE, AE-GLSL (baku89) | MIT [S45] | GLSL runners; OpenGL context inside the plug-in: a tier-e probe. Prebuilt Windows binaries: UNVERIFIED. |
| Vulkanator (Wunkolo) | MIT [S45] | Vulkan sample effect; tier-e probe. |
| Gyroflow plug-ins | GPL-3.0 with App Store exception [S43] | Running a GPL plug-in is fine (separate work, separate process); do not copy code. |
| PyShiftAE | AGPL-3.0 [S45] | AEGP-style; not useful. |
| obs | n/a | No AE plug-in found. |

---

## 4. Architecture in Milan

Reuse what R31-R33b built: `milan-plugin-host.exe` (named pipe + shared memory, Job Object, watchdog, silent
crashes), `engine/ofx/client.rs` (`Conn`, `Shm`), the scan cache, the Plug-In Manager, `ParamSpec` registry entries,
`vfx::apply`. The AE host is a second personality of the same host binary.

```
MilanPro.exe                                   milan-plugin-host.exe  (one per .aex)
  engine/aex/mod.rs   registry entries, apply    aex/abi.rs     own #[repr(C)] types + provenance
  engine/aex/scan.rs  folders, PiPL, cache       aex/pipl.rs    PiPL reader (shared with scan via #[path])
  engine/ofx/client   Conn/Shm/watchdog (reuse)  aex/host.rs    lifecycle, PF_Cmd flow, instances
  engine/ofx/proto    + Aex requests             aex/suites.rs  SPBasicSuite + PF suites
  ui/plugin_manager   rows, report               aex/convert.rs RGBA<->ARGB, rowbytes, fields
```

### 4.1 Discovery and PiPL

- Folders (all read-only, off until the user enables the feature): the two `CommonPluginInstallPath` registry values
  (1.5), `C:\Program Files\Adobe\Common\Plug-ins\7.0\MediaCore` and `…\Plugins\7.0\MediaCore` if present, plus user
  folders (`prefs.aex.folders`). Not scanned by default: Adobe application folders. Same recursion rules as AE (10
  levels, skip `(...)` and `~` folders, no shortcuts).
- For each `*.aex`: check PE machine (x64 only), read PiPLs **in Milan's process as data** (no code runs), keep kind
  `eFKT` only. Result: name, category, match name, entry-point name, out_flags/out_flags2, spec version.
- Static pre-classification from the flags, before anything is loaded (flag bit values come from the public guide /
  provenance table): uses audio → Unsupported; SmartFX without a `RENDER` path → "Needs SmartFX"; custom UI →
  "Generic controls only"; `ADBE ` match name or Adobe path → Blocked.
- Describe (GLOBAL_SETUP + PARAM_SETUP) runs in a host process with the R32 time-outs; result cached in
  `plugins.json` by path, size, mtime (the existing cache, new source `Aex { path, match_name }`).

### 4.2 Lifecycle in the host

| Milan event | Host actions |
|---|---|
| `Load` | `LoadLibraryExW` with the plug-in's folder on the DLL search path; resolve the PiPL entry point. |
| `Describe` | `GLOBAL_SETUP` (keep `global_data`, out_flags; compare with PiPL and warn on mismatch), `PARAM_SETUP` (collect `PF_ADD_PARAM` calls). |
| First render of an instance | `SEQUENCE_SETUP`, or `SEQUENCE_RESETUP` with the stored flat blob when the project has one. |
| Frame | set params at t → [`USER_CHANGED_PARAM` for changed supervised params] → `FRAME_SETUP` → `RENDER` → `FRAME_SETDOWN`. |
| Project save / instance copy | `SEQUENCE_FLATTEN` when `PF_OutFlag_SEQUENCE_DATA_NEEDS_FLATTENING`, return the handle bytes; then `SEQUENCE_RESETUP` to keep running. |
| Instance removed / host exit | `SEQUENCE_SETDOWN`; `GLOBAL_SETDOWN` on `Quit` (best effort; the host exits regardless, as today). |

All calls to one plug-in are serialised on the host's single thread (what Premiere guarantees [S18]). Global and
sequence data are host-allocated handles (PF Handle Suite) so their size is known for flatten.

### 4.3 Parameters → `ParamSpec`, keyframes, storage

Instances are stored as `aex:<match name>` with keys `p<id>` (the plug-in's stable `PF_ParamDef.id`, so parameter
reordering between plug-in versions keeps projects valid).

| PF type | Milan | Storage | Notes |
|---|---|---|---|
| FLOAT_SLIDER, FIX_SLIDER, SLIDER | `Float{min,max,default}` (slider range, not valid range) | one key | percent display flag → name suffix |
| ANGLE | `Float` in degrees, -3600..3600 | one key | converted to fixed 16.16 in the host |
| CHECKBOX | `Bool` | 0/1 | |
| POPUP | `Choice` | index 0-based in Milan, +1 for the plug-in | items split on the separator; `(-` entries dropped with index remap |
| COLOR | `Color` | `key.r/.g/.b/.a` | 8-bit to the plug-in; alpha ignored |
| POINT | two `Float`s, 0..100 % of frame | `key.x`, `key.y` | host converts to layer pixels × downsample, fixed point; POINT_3D adds `.z` (AE dialect only) |
| GROUP_START/END | section heading | none | nesting flattened |
| BUTTON | not shown (same as OFX push buttons today) | none | later: FX-window button → `USER_CHANGED_PARAM` |
| LAYER (index > 0) | `Choice`: None / the event's own source / "Track below" / "Track N" | one key | host answers `PF_CHECKOUT_PARAM`; see 4.5. First version: None and Self only. |
| ARBITRARY_DATA | not shown | `Fx.blob["p<id>"]` | default from the plug-in; flatten/unflatten through `ARBITRARY_CALLBACK`; not keyframed |
| PATH, CUSTOM, NO_DATA | not shown | none | PATH checks out "no mask" |

`PF_ParamFlag_CANNOT_TIME_VARY` → no keyframes; `PF_PUI_INVISIBLE`/`NO_ECW_UI` → hidden; `PF_PUI_DISABLED` → shown
disabled when the host reports it after supervision (needs a small "dynamic param state" reply; the OFX side has
the same gap, `kOfxParamPropEnabled`, so build it once for both).
Keyframes, presets and automation come free, as for OFX: values are evaluated by Milan at t and sent with the frame.
`PF_CHECKOUT_PARAM` for a *value* at another time needs the curve: send the instance's keyframes with the render
request when the plug-in set `PF_OutFlag_WIDE_TIME_INPUT` or has supervised params (also closes the OFX "parameter
curves at other times" gap).

New persistent field (ground rule 5.0): `Fx.blob: BTreeMap<String, String>` (base64 of flat sequence data under
`seq`, ARB values under `p<id>`), `#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]`, old-project test
extended. Blobs are capped (16 MB each) and treated as untrusted on load: they go only to the isolated host.

### 4.4 Frames

Milan gives the host a premultiplied RGBA frame, bottom row first (the OFX convention already in `Render`).
The AE side of the host converts in place in the shared memory:

- channel order RGBA → ARGB (8-bit) or BGRA/ARGB 32f; row order flipped by handing the plug-in a pointer to the last
  row and a **positive** `rowbytes` over a flipped copy (do not rely on negative rowbytes: UNVERIFIED that plug-ins
  tolerate it). The lazy first version copies; add `// shortcut:` for the extra copy.
- alpha: convert to whatever A4's test proves the API expects (straight vs premultiplied), and back, clamping as the
  OFX path does.
- `rowbytes` = width × bytes, padded to 16; input and output worlds separate; `PF_WorldFlag_WRITEABLE` on output only.
- `in_data`: `width`/`height` = full-quality size; `downsample_x/y` = rational render scale (Draft/Preview);
  `pixel_aspect_ratio` rational from project PAR; `output_origin` 0; `extent_hint` = whole frame;
  `quality` = HI for Best/Good and render, LO otherwise; `field` = none (progressive frames only in v1; field
  rendering — one RENDER per field, half height, doubled rowbytes [S18] — is a later item, flagged in the report for
  interlaced projects).
- time: `time_scale` = fps numerator (e.g. 30000), `time_step` = fps denominator (1001), `current_time` = frame ×
  time_step, event-relative; `total_time` = event length. Only the ratio matters to plug-ins [S18].
- `FRAME_SETUP` asking for a larger output (`PF_OutFlag_I_EXPAND_BUFFER`): allocate it, then crop back to the frame
  around the origin (v1), report "output cropped".
- 32f: only when the plug-in registered a float format through the PF Pixel Format Suite (A6); else 8-bit, and the
  Plug-In Manager shows "8-bit only" (a float project is quantised through this effect; say so in the FX window).

### 4.5 Checkout of other frames and layers

`PF_CHECKOUT_PARAM(index, time, …)`:
- value parameter → evaluate the sent curve at that time.
- layer parameter 0 (the input) at the current time → the source world; at another time → a nested request from the
  host to Milan over the pipe (`Reply::NeedFrame { layer, time }` → Milan renders that source frame into a second
  shared-memory region → host continues). This makes the protocol two-way during a render; the watchdog covers the
  whole exchange. Milan answers from the event's media only (the effect's own input chain is not re-run in v1).
- other layer parameters → "None" gives a zero-filled world [S14]; "Track N" uses the same NeedFrame path.
Temporal checkout is also what the OFX path lists as left ("temporal clip access"); build the NeedFrame message once.

### 4.6 Custom UI strategy

v1: **generic controls only.** `PF_REGISTER_UI` succeeds and is recorded; no `PF_Cmd_EVENT` is ever sent; parameters
with `PF_PUI_CONTROL` and a standard type still get the generic control; ARB/CUSTOM parameters are hidden and the
FX window shows "This plug-in has custom controls that Milan Pro cannot show".
Later, only if demand justifies it (not scheduled): a host-owned top-level window per instance (the R34b pattern for
VST3 editors) that implements Drawbot over GDI+/Direct2D and forwards mouse events. `PF_Cmd_DO_DIALOG` (options
dialog; the plug-in opens its own Win32 window) can be offered earlier as an "Options…" button since it needs no
Drawbot, but it breaks the "host shows no windows" rule and needs a modal bridge: A9, optional.
`PF_Cmd_ABOUT`: not sent; `return_msg` strings from any command go to the log and the FX window, never a dialog.

### 4.7 Plug-In Manager, report, watchdog

- New kind "AE-format effect" with statuses OK / Failed (reason) / Disabled / **Blocked** / **Untested**, and a
  capability line per plug-in from PiPL flags and describe: bit depth, SmartFX, custom UI, layer params, ARB,
  requested suites that were refused (the host logs every `AcquireSuite` name+version it could not serve: the single
  most useful compatibility datum).
- "Export compatibility report" (the R31 left-over) gains these columns; feeds `docs/compat/` and R55.
- Watchdog as R32: load 10 s, describe 2 s, render 30 s, plus 5 s for flatten. A crash, hang or `PF_Err` bypasses the
  instance for the session. One host process per `.aex` (statics in plug-ins make sharing unsafe).
- Extra containment for this format: the host sets the DLL directory to the plug-in's folder only; never adds Adobe
  application folders to the search path; message boxes from plug-ins cannot be prevented in general, so the host
  runs on a hidden desktop-less window station if R32 testing shows dialogs appearing (UNVERIFIED need; decide in A2).

---

## 5. Batches

Sizes as in `docs/REMAINING.md` (S/M/L/XL). Ground rules 5.0 apply. Every batch: `cargo build`, `cargo test` green,
nothing half-wired, new `Fx` fields `#[serde(default)]`.

**Test plug-ins.** (1) `tools/aex-test-plugin/` (pattern of `tools/ofx-test-plugin/`): a Rust `cdylib` written
against Milan's **own** ABI declarations (shared `abi.rs`), renamed `.aex`, its PiPL written by Milan's own PiPL
writer as a `.res` linked by `build.rs`. It is written without Adobe SDK headers. (2) Open-source plug-ins from 3.2,
found through `MILAN_AEX_TEST_DIR`, tests `#[ignore]`. (3) A developer who has the Adobe SDK may build SDK samples
locally to cross-check, but nothing built from or containing SDK code is ever committed, and such a developer does
not edit `abi.rs` (2.6).
Caveat to keep in every report: a test plug-in built on our own ABI proves self-consistency, not conformance. Only
the plug-ins of (2) prove the ABI is right, so A2-A4 are not "done" until at least one of them passes.

### A0: Legal sign-off and provenance scaffold (S, no code)

- **Scope:** owner (ideally with counsel) confirms 2.6, especially points 2 and 5. Add `docs/compat/aex-provenance.md`
  (table: item, value, source URL / observation), the CONTRIBUTING rule, a CI grep that fails on "Adobe Systems
  Incorporated" / "ADOBE CONFIDENTIAL" / `AE_Effect.h` in the tree, the trademark notice text.
- **Files:** `docs/compat/aex-provenance.md`, `CONTRIBUTING.md`, CI script.
- **Risks:** the answer may be "do not build this"; then stop here.
- **Acceptance:** the grep job fails on a fixture file containing the strings and passes on the tree.

### A1: PiPL reader, discovery, Plug-In Manager rows (M)

- **Scope:** PE resource walk + PiPL parser and writer (writer is for the test plug-in and fixtures); folder scan
  (4.1) behind `prefs.aex.enabled` (default off); `PluginSource::Aex`; static classification; Blocked rule; rows and
  statuses in the Plug-In Manager; nothing is loaded or run.
- **Files:** `engine/aex/{mod.rs,scan.rs,pipl.rs}` (new), `engine/ofx/scan.rs` (cache source), `ui/plugin_manager.rs`,
  `settings.rs`, `tools/aex-test-plugin/` (PiPL only at this stage).
- **Risks:** resource type/id assumptions (enumerate, do not hard-code); multiple PiPLs per file; big-endian PiPLs in
  old files; malformed lengths (parser must be bounds-checked: it runs in Milan's process on untrusted files).
- **Acceptance:** `pipl_round_trips` (writer → parser); `pipl_rejects_truncated_and_oversized` (fuzz-style table of
  bad inputs, no panic); `aex_scan_lists_the_test_plugin`; `adobe_match_names_are_blocked`; `scan_is_off_by_default`;
  `#[ignore]` `pipl_of_open_source_plugins_parses` over `MILAN_AEX_TEST_DIR` printing name/match name/flags.

### A2: ABI bindings, load, GLOBAL_SETUP and PARAM_SETUP (L)

- **Scope:** `aex/abi.rs` with provenance for every item; `SPBasicSuite` returning "missing" for everything while
  logging requests; `in_data->inter.add_param`; handle/memory callbacks; `Req::AexLoad`/`AexDescribe` (or a format
  field on the existing requests); describe result → `PluginDesc`-like struct → `ParamSpec` (4.3) → registry entries
  `aex:<match name>`; choosers list them under "Third Party"; capability line; compile-time `size_of`/`offset_of`
  assertions for every struct.
- **Files:** `src/bin/milan-plugin-host/aex/{abi.rs,host.rs,suites.rs}` (new), `main.rs`, `engine/ofx/proto.rs`,
  `engine/aex/mod.rs`, `ui/choosers.rs`.
- **Risks:** the biggest in the plan: wrong struct layout = memory corruption (contained by the host process, but
  silent wrong behaviour is possible). Layout facts not in the public guide (2.4). Plug-ins that call a NULL callback
  → fill every slot of `utils`/`inter` with a logging stub, never NULL.
- **Acceptance:** `aex_test_plugin_describes` (all parameter types round-trip into `ParamSpec`);
  `parameters_map_both_ways_aex`; `a_plugin_that_crashes_in_global_setup_is_contained`; `#[ignore]`
  `open_source_plugins_describe` (ntsc-rs and the `after-effects` crate examples `simplest`, `paramarama`: parameter
  names and counts equal those in their source); the unserved-suite log is non-empty and exported.

### A3: Core suites and callbacks (L)

- **Scope:** the tier-a set of 1.2: `utils` callbacks (copy, fill, blend, premultiply, iterate 8-bit, convolve,
  sampling as far as documented), PF Iterate8, World, Pixel Data, Handle, ANSI, Color/ColorParam/PointParam/AngleParam,
  World Transform `copy`, Fill Matte `fill`, abort/progress. Each suite only as far as a test plug-in or an
  open-source plug-in exercises it; unimplemented members are logging stubs returning an error.
- **Files:** `aex/suites.rs`, `aex/abi.rs`, provenance doc.
- **Risks:** suite version sprawl (plug-ins ask newest-first; serve only versions whose table is known); iterate
  callbacks' threading (run single-threaded first, `// shortcut:`).
- **Acceptance:** host unit tests per suite (`iterate8_visits_every_pixel_once`, `handle_suite_resize_keeps_data`,
  `copy_and_fill_respect_rects`); the test plug-in's "uses every tier-a callback" mode returns a checksum equal to a
  Rust reference.

### A4: Render path, sequence data, project storage (L)

- **Scope:** 4.2 frame flow, 4.4 conversion (settle row order and alpha with tests first), `vfx::apply` dispatch for
  `aex:`; sequence setup/resetup/flatten/setdown; `Fx.blob`; save → flatten, load → resetup; preview and render share
  the path; bypass on error; FX-window messages.
- **Files:** `aex/{host.rs,convert.rs}`, `engine/aex/mod.rs`, `engine/vfx.rs`, `project.rs` (`Fx.blob`), project
  save/load hook, `ui/fx_window.rs`.
- **Risks:** alpha/row-order errors look "almost right"; flatten called while a render is queued; blob growth;
  plug-ins that resize output.
- **Acceptance:** `aex_invert_matches_the_builtin_invert` (≤ 1 LSB); `aex_gain_is_keyframed_at_t`;
  `row_order_and_channel_order_marker` (test plug-in paints a known asymmetrical pattern); `sequence_data_survives_save_and_load`
  (test plug-in counts in sequence data, flattens a pointer-holding structure); `a_project_with_a_missing_aex_plugin_keeps_its_parameters`;
  old-project test extended for `Fx.blob`; `render_matches_preview_with_aex_plugins`; `#[ignore]`
  `ntsc_rs_aex_matches_ntsc_rs_ofx` (same settings through R33 and through A4, within a stated tolerance) — this is
  the test that proves the ABI against a third-party binary.

### A5: Supervision, checkout, layer parameters (L)

- **Scope:** `USER_CHANGED_PARAM`, `UPDATE_PARAMS_UI`, PF Param Utils subset, dynamic enabled/hidden state back to
  the FX window (shared with OFX); curves sent for wide-time plug-ins; `PF_CHECKOUT_PARAM` for values and for the
  input at other times via `NeedFrame` (4.5); layer params "None / Self / Track N"; plug-in-initiated parameter
  changes written back to the project as one undo step.
- **Files:** `aex/host.rs`, `engine/ofx/{proto.rs,client.rs}` (two-way exchange), `engine/aex/mod.rs`, `engine/vfx.rs`,
  `ui/fx_window.rs`.
- **Risks:** re-entrancy (a checkout that triggers another render of the same instance: refuse with a zero frame and
  log); deadlock between Milan's render thread and the host (single in-flight request per host; time-out covers it);
  cost of temporal effects in preview.
- **Acceptance:** `checkout_at_other_time_returns_that_frame` (test plug-in outputs frame t-1: equals the previous
  source frame); `layer_param_none_is_transparent`; `supervised_param_disables_its_sibling`;
  `echo_style_plugin_does_not_deadlock` (10 nested checkouts, inside the time-out); `#[ignore]` the crate examples
  `checkout` and `supervisor`.

### A6: 32-bit float through the Premiere pixel-format route (M)

- **Scope:** PF Pixel Format Suite (`AddSupportedPixelFormat`, clear), the Premiere pixel-format suite's black/white
  query, PF Utility Suite subset (frame rate, field type, checkout-format opt-in); host picks `BGRA_4444_32f` when
  registered, else 8-bit; IterateFloat. VUYA formats are never offered.
- **Files:** `aex/{suites.rs,convert.rs,abi.rs}`, `engine/aex/mod.rs`.
- **Risks:** the Premiere suites are documented only in outline publicly (names and purpose [S23]); function tables
  need a public source or stay unbuilt. If none is found, A6 is dropped and float projects quantise (stated in UI).
- **Acceptance:** `float_frames_round_trip_above_1_0` (test plug-in doubles values; overbrights survive);
  `eight_bit_only_plugin_is_marked`; `#[ignore]` crate example `sdk_noise` renders in float.

### A7: Transitions (M)

- **Scope:** PF Transition Suite `RegisterTransitionInputParam`; such plug-ins register as `PluginKind::Transition`;
  A and B frames in shared memory; progress from Milan's transition progress envelope mapped to the plug-in's
  completion parameter (how Premiere passes progress is not on the public page: UNVERIFIED; resolve before starting,
  else drop).
- **Files:** `aex/host.rs`, `engine/aex/mod.rs`, `engine/composite.rs`, `ui/choosers.rs`.
- **Risks:** thin public documentation; few open-source test subjects (none found).
- **Acceptance:** test plug-in cross-dissolve equals the built-in at p = 0.25/0.5/0.75 (≤ 1 LSB).

### A8: SmartFX (XL, blocked on decision 2.6(5))

- **Scope:** per-plug-in opt-in "After Effects dialect"; `SMART_PRE_RENDER`/`SMART_RENDER` with `checkout_layer`,
  `checkout_layer_pixels`, `checkout_output`, rect bookkeeping (request = full frame; honour `result_rect` and
  `max_result_rect`), 16-bit and float worlds, PF World Suite pixel-format query, Sampling suites; AEGP suites still
  refused.
- **Files:** `aex/{smart.rs (new),suites.rs,abi.rs}`, `engine/aex/mod.rs`, `ui/plugin_manager.rs`.
- **Risks:** legal (presenting the AE dialect); many SmartFX plug-ins also expect AEGP and will fail anyway; large
  ABI surface from thin public documentation.
- **Acceptance:** test plug-in SmartFX blur with an expanded `result_rect` matches a Rust reference; `#[ignore]` an
  open-source SmartFX effect (crate example `transformer` or an F's Plugins effect) renders; a plug-in that asks for an
  AEGP suite reports "needs After Effects (AEGP)" and is bypassed.

### A9: Compatibility run, report, docs; optional options dialog (M, RESEARCH plus data)

- **Scope:** run describe + render for every open-source plug-in of 3.2 and, with the owner's own licences, any
  commercial plug-in the owner chooses; fill `docs/compat/aex.csv` (template 6.2), feed R55; COMPATIBILITY.md and
  README sections (6.1); in-app text. Optional: `PF_Cmd_DO_DIALOG` behind an "Options…" button.
- **Files:** `docs/compat/aex.csv`, `docs/COMPATIBILITY.md`, `README.md`, `ui/plugin_manager.rs`.
- **Risks:** temptation to list untested plug-ins; vendor objections to being listed (list only what was run, with
  version and date).
- **Acceptance:** no row says "unknown"; nothing in the docs or UI claims support for a plug-in without a passing
  row; trademark notice present in the three places of 2.5.

Order and dependencies: A0 → A1 → A2 → A3 → A4 → A5; A6, A7 after A4; A8 after A5 and the decision; A9 last.
All need R31/R32 (done for OFX). Suggested tier: after R33c, alongside tier 5 "Plug-in compatibility".
Stopping after A5 is a coherent product: "simple 8-bit AE-format effects, generic controls".

---

## 6. What we will tell users

### 6.1 Paragraph

> **AE-format effects (experimental).** Milan Pro can load some effect plug-ins written for the Adobe After Effects
> effect API (`.aex` files) that you have installed and licensed yourself. Milan Pro includes no Adobe software and
> no third-party plug-ins. It implements only a basic part of the interface, similar to what video editors (rather
> than After Effects itself) provide: simple filters with standard controls, in 8-bit or, where the plug-in offers
> it, 32-bit float. Plug-ins that need After Effects features such as compositions, cameras, lights, masks, their
> own drawn controls, GPU rendering or SmartFX will not work, and many commercial plug-ins check for an Adobe host or
> are licensed only for specific hosts; Milan Pro does not bypass any such check. Each plug-in runs in a separate
> process, so a failing plug-in cannot crash the editor; the Plug-In Manager shows what loaded, what did not, and why.
> If a vendor offers an OpenFX version, use that instead. Check your plug-in's licence before using it in Milan Pro.
> Adobe does not support third-party hosts of its plug-in interface. Adobe, After Effects and Premiere Pro are
> trademarks of Adobe Inc.; Milan Pro is not affiliated with or endorsed by Adobe.

### 6.2 Compatibility-matrix template (`docs/compat/aex.csv`, one row per plug-in version tested)

| Plug-in | Vendor | Version | Plug-in licence | Source of binary | Match name | Tier (a-f) | PiPL parsed | Global/param setup | Params mapped / total | Renders 8-bit | Renders 32f | Sequence data save/load | Custom UI | Suites refused | Host check / licence prompt | Result | Notes | Milan build | Date |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| example: ntsc-rs | valadaptive | 0.9.7 | MIT/ISC/Apache (verify) | project release | (from PiPL) | a | yes/no | ok/fail(reason) | n / m | ok / diff vs OFX | n/a | ok | none | list | none | Works / Works with limits / Fails / Refused by plug-in / Blocked / Untested | free text | commit | yyyy-mm-dd |

Result vocabulary is fixed to those six values; "Works" requires a recorded render comparison, not just loading.

---

## 7. Sources

- [S1] Adobe Developer Terms of Use, last updated 18 June 2024: https://wwwimages2.adobe.com/content/dam/cc/en/legal/servicetou/Developer-Terms-en_US-20240618.pdf
- [S2] PiPL Resources: https://ae-plugins.docsforadobe.dev/intro/pipl-resources/
- [S3] Entry Point: https://ae-plugins.docsforadobe.dev/effect-basics/entry-point/
- [S4] Premiere Pro & Other Hosts: https://ae-plugins.docsforadobe.dev/ppro/ppro/
- [S5] Command Selectors: https://ae-plugins.docsforadobe.dev/effect-basics/command-selectors/
- [S6] PF_InData: https://ae-plugins.docsforadobe.dev/effect-basics/PF_InData/
- [S7] Unsupported Features (Premiere): https://ae-plugins.docsforadobe.dev/ppro/unsupported-features/
- [S8] Parameters: https://ae-plugins.docsforadobe.dev/effect-basics/parameters/ ; [S8b] PF_ParamDef: https://ae-plugins.docsforadobe.dev/effect-basics/PF_ParamDef/
- [S9] Third-party Plug-in Hosts?: https://ae-plugins.docsforadobe.dev/intro/third-party-plug-in-hosts/
- [S10] Other Hosts: https://ae-plugins.docsforadobe.dev/ppro/other-hosts/
- [S11] PF_EffectWorld / PF_LayerDef: https://ae-plugins.docsforadobe.dev/effect-basics/PF_EffectWorld/
- [S12] SmartFX: https://ae-plugins.docsforadobe.dev/smartfx/smartfx/
- [S13] Exporting Symbols in Effects: https://ae-plugins.docsforadobe.dev/intro/symbol-export/
- [S14] Interaction Callback Functions: https://ae-plugins.docsforadobe.dev/effect-details/interaction-callback-functions/
- [S15] Effect UI & Events: https://ae-plugins.docsforadobe.dev/effect-ui-events/effect-ui-events/
- [S16] Multi-Frame Rendering in AE: https://ae-plugins.docsforadobe.dev/effect-details/multi-frame-rendering-in-ae/
- [S17] `pipl` crate (MIT OR Apache-2.0): https://docs.rs/pipl/latest/pipl/ , source https://docs.rs/crate/pipl/latest/source/src/lib.rs
- [S18] Basic Host Differences: https://ae-plugins.docsforadobe.dev/ppro/basic-host-differences/
- [S19] Accessing the After Effects Function Suites: https://ae-plugins.docsforadobe.dev/effect-details/accessing-function-suites/
- [S20] EDIUS 7 manual, After Effects Plug-in Bridge: https://wwwapps.grassvalley.com/manuals/EDIUS7_USER_EN/l06/l0_6_l1_15_l2_3.html
- [S21] Cheating Effect Usage of AEGP Suites: https://ae-plugins.docsforadobe.dev/aegps/cheating-effect-usage-of-aegp-suites/
- [S22] HitFilm Pro 14 and AE plug-ins: https://www.provideocoalition.com/hitfilm-pro-14-launched-with-support-for-select-after-effects-plugins/ ; manual: https://www.manula.com/manuals/fxhome/hitfilm/14/en/topic/openfx ; version history: https://videohelp.com/software/HitFilm/version-history
- [S23] Bigger Differences (pixel formats): https://ae-plugins.docsforadobe.dev/ppro/bigger-differences/
- [S24] AE Transition Extensions / PF_TransitionSuite: https://ppro-plugins.docsforadobe.dev/ae-transition-extensions/ae-transition-extensions/ , https://ppro-plugins.docsforadobe.dev/ae-transition-extensions/PF_TransitionSuite/
- [S25] Premiere pixel formats: https://ppro-plugins.docsforadobe.dev/universals/pixel-formats-and-color-spaces/
- [S26] GPU Effects & Transitions: https://ppro-plugins.docsforadobe.dev/gpu-effects-transitions/gpu-effects-transitions/
- [S27] Premiere Pro plug-in types: https://ppro-plugins.docsforadobe.dev/intro/premiere-pro-plugin-types/
- [S28] Where Installers Should Put Plug-ins: https://ae-plugins.docsforadobe.dev/intro/where-installers-should-put-plug-ins/
- [S29] Premiere plug-in installation: https://ppro-plugins.docsforadobe.dev/intro/plugin-installation/
- [S30] Adobe forum, "After Effects SDK license": https://community.adobe.com/questions-529/after-effects-sdk-license-54330
- [S31] Blackmagic forum threads on Fusion and AE plug-ins: https://forum.blackmagicdesign.com/viewtopic.php?p=196027 , https://forum.blackmagicdesign.com/viewtopic.php?p=308339
- [S32] OpenFX hosts: https://en.wikipedia.org/wiki/OpenFX_(API)
- [S33] Boris FX multi-host listings (example): https://www.bhphotovideo.com/c/product/1449529-REG/boris_fx_bccmultiavx_continuum_2019_multi_host_license.html ; Digital Anarchy host-specific serials: https://digitalanarchy.com/?p=1685
- [S34] aexlo.js: https://github.com/potistudio/aexlo.js
- [S35] Maxon EULA: https://www.maxon.net/en/legal/eula
- [S36] Video Copilot compatibility: https://www.videocopilot.net/support/category/compatibility
- [S37] Google LLC v. Oracle America, Inc., 593 U.S. ___ (2021): https://www.supremecourt.gov/opinions/20pdf/18-956_d18f.pdf (cited from memory, not re-read in this pass)
- [S38] CJEU C-406/10 SAS Institute v World Programming: https://eur-lex.europa.eu/legal-content/EN/TXT/?uri=CELEX:62010CJ0406 (cited from memory)
- [S39] Directive 2009/24/EC: https://eur-lex.europa.eu/legal-content/EN/TXT/?uri=CELEX:32009L0024 (cited from memory)
- [S40] After Effects C++ SDK Guide (community-hosted copy of Adobe's guide; its own licence and whether reading it engages §2.23(G) for someone who never accepted the terms: UNVERIFIED): https://ae-plugins.docsforadobe.dev/
- [S41] `after-effects` Rust crate: https://github.com/virtualritz/after-effects
- [S42] ntsc-rs: https://github.com/valadaptive/ntsc-rs
- [S43] Gyroflow plug-ins: https://github.com/gyroflow/gyroflow-plugins
- [S44] F's Plugins: https://github.com/bryful/F-s-PluginsProjects
- [S45] ISF4AE https://github.com/baku89/ISF4AE , AE-GLSL https://github.com/baku89/AE-GLSL , Vulkanator https://github.com/Wunkolo/Vulkanator (URL from search listing, not opened), PyShiftAE https://github.com/Trentonom0r3/PyShiftAE (URL from search listing, not opened)

Local context read (read-only): `C:\MeridianNLE\docs\REMAINING.md` (5.0, R31-R35, 2.10), `src\engine\plugins.rs`,
`src\engine\ofx\{mod,proto}.rs`, `src\bin\milan-plugin-host\main.rs`, `src\project.rs` (`Fx`), `tools\ofx-test-plugin`.
