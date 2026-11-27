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
- Premiere Pro & Other Hosts [S4] and Basic Host Differences [S18]: https://ae-plugins.docsforadobe.dev/ppro/ppro/ , https://ae-plugins.docsforadobe.dev/ppro/basic-host-differences/ (cited from the plan, not re-read)
- Premiere: bigger differences: https://ae-plugins.docsforadobe.dev/ppro/bigger-differences/
- Premiere: AE transition extensions and PF_TransitionSuite: https://ppro-plugins.docsforadobe.dev/ae-transition-extensions/ae-transition-extensions/ , https://ppro-plugins.docsforadobe.dev/ae-transition-extensions/PF_TransitionSuite/
- Premiere: pixel formats: https://ppro-plugins.docsforadobe.dev/universals/pixel-formats-and-color-spaces/ ; SweetPea suites: https://ppro-plugins.docsforadobe.dev/universals/sweetpea-suites/

Not found on any public page (and therefore not built, or built as class R as said per item): the function order of
`in_data->utils`; suite name strings; numeric values of selectors, parameter types, flags and error codes; the
members of the per-type parameter definitions; the function tables of the Premiere PF Pixel Format, PF Utility and
PF Transition suites; how Premiere passes transition progress.

## Items

Pages are named by their titles in the list above ("PF_InData", "Parameters", ...).

### A1: files and PiPL (`src/engine/aex/pipl.rs`, `src/engine/aex/mod.rs`)

| Item | Value | Class | Source / observation |
|---|---|---|---|
| A plug-in is a DLL with PiPL resources and one exported entry per effect | | G | PiPL resources; Entry point |
| PE headers, section table, resource directory (type / name / language), `.res` file layout | | MS | Microsoft PE and resource-file documentation |
| PiPL resource type | the string `PIPL` (any case), every one enumerated; no id assumed | M | plan §1.1 (the id 16000 is UNVERIFIED, so not relied on) |
| PiPL header | `01 00` (big-endian `00 01`), 4-byte version, 4-byte property count | P | plan §1.1 summary of the `pipl` crate docs [S17] (not re-read for this work) |
| Property | vendor, key, id, length (4 bytes each), data padded to 4 | P | as above |
| Four-character codes | stored as little-endian 32-bit words ('kind' is `dnik` in the file) | P | as above |
| Keys | `kind` (`eFKT` for an effect), `name`, `catg`, `8664` (Win64 entry), `ePVR`, `eSVR`, `eVER`, `eINF`, `eGLO`, `eGL2`, `eMNA`, `aeFL`, `eURL` | P | as above; the property list itself is G (PiPL resources) |
| String encoding | name, category, match name length-prefixed; entry point NUL-terminated; the reader accepts either for any string | R | |
| `eSVR` / `ePVR` | two 16-bit numbers (major, minor) | R | |
| `eVER` packing | major << 19, minor << 15 (display only) | R | |
| Shared plug-in folders | registry `CommonPluginInstallPath` under `SOFTWARE\Adobe\After Effects\7.0` and `SOFTWARE\Adobe\Premiere Pro\10.0`; `Program Files\Adobe\Common\Plug-ins\7.0\MediaCore` and `...\Plugins\...` | G | plan §1.5 [S28][S29] |
| Search rules | 10 levels, skip names ending `)` or starting `~`, no links | G | plan §1.5 [S28] |
| Blocked | match name starting `ADBE `, or a file under `...\Adobe\Adobe <application>\` | M | plan §2.6 (6) |

### A2: commands, structures, constants (`src/bin/milan-plugin-host/aex/abi.rs`)

| Item | Value | Class | Source / observation |
|---|---|---|---|
| Entry point prototype | `(cmd, in_data, out_data, params[], output, extra) -> error` | G | Entry point |
| Calling convention | the C convention (the only one on x64) | R | |
| Command selectors | names and order | G | Command selectors |
| Command values | ABOUT 0, GLOBAL_SETUP 1, GLOBAL_SETDOWN 3, PARAMS_SETUP 4, SEQUENCE_SETUP 5, RESETUP 6, FLATTEN 7, SETDOWN 8, DO_DIALOG 9, FRAME_SETUP 10, RENDER 11, FRAME_SETDOWN 12, USER_CHANGED_PARAM 13, UPDATE_PARAMS_UI 14, EVENT 15, ARBITRARY_CALLBACK 22, SMART_PRE_RENDER 23, SMART_RENDER 24 | R | |
| Error names | NONE, OUT_OF_MEMORY, INTERNAL_STRUCT_DAMAGED, INVALID_INDEX, UNRECOGNIZED_PARAM_TYPE, INVALID_CALLBACK, BAD_CALLBACK_PARAM, INTERRUPT_CANCEL | G | Errors |
| Error values | 0, 4, 512, 513, 514, 515, 516, 517 | R | |
| `PF_InData` field order | inter, utils, effect_ref, quality, version, serial_num, appl_id, num_params, what_cpu, what_fpu, current_time, time_step, total_time, local_time_step, time_scale, field, shutter_angle, width, height, extent_hint, output_origin_x/y, downsample_x/y, pixel_aspect_ratio, in_flags, global/sequence/frame_data, start/dur/total_sampL, src_snd, pica_basicP, pre_effect_source_origin_x/y, shutter_phase | G | PF_InData |
| `PF_InData` types; a reserved 32-bit slot after num_params | | R | |
| `inter` held by value | | G-inf | Interaction callbacks: "the un-macro'd function pointers are in PF_InData" |
| `appl_id` | `'PrMr'` | G | PF_InData: Premiere Pro and other hosts use it |
| API version reported | 13.4 | G | Premiere Pro & Other Hosts [S4] (what Premiere reports); M: Milan reports the same |
| `serial_num` | 0 | M | plan §2.6 (4) |
| `PF_OutData` field order | my_version, name, global_data, num_params, sequence_data, flat_sdata_size, frame_data, width, height, origin, out_flags, return_msg, start_sampL, dur_sampL, dest_snd, out_flags2 | G | PF_OutData |
| `PF_OutData` types; `name` 32 and `return_msg` 256 bytes | | R | |
| `out_flags` bits | bit n = the n-th of the table's 32 rows (two reserved rows included): WIDE_TIME_INPUT 1, SEQUENCE_DATA_NEEDS_FLATTENING 4, I_DO_DIALOG 5, I_EXPAND_BUFFER 9, CUSTOM_UI 15, I_USE_AUDIO 20, DEEP_COLOR_AWARE 25, SEND_UPDATE_PARAMS_UI 26, AUDIO_EFFECT_TOO 30, AUDIO_EFFECT_ONLY 31 | G-inf | PF_OutData lists exactly 32 names, reserved placeholders included |
| `out_flags2` bits | SUPPORTS_QUERY_DYNAMIC_FLAGS 0, I_USE_3D_CAMERA 1, I_USE_3D_LIGHTS 2, SUPPORTS_SMART_RENDER 10, FLOAT_COLOR_AWARE 12, SUPPORTS_GPU_RENDER_F32 25, SUPPORTS_THREADED_RENDERING 27 | R (names G) | the table has gaps, so positions cannot be inferred |
| `PF_EffectWorld` members | world_flags, data, rowbytes, width, height, extent_hint, pix_aspect_ratio, platform_ref, dephault | G | PF_EffectWorld |
| `PF_EffectWorld` reserved slots, their positions, the origin fields | | R | |
| Pixel channel order | A, R, G, B from the low byte | G | PF_EffectWorld |
| Row 0 is the top row; 8-bit worlds hold premultiplied colour | | R | UNVERIFIED (plan §1.1) |
| `rowbytes` may exceed width x 4 | | G | PF_EffectWorld |
| `PF_ParamDef` members and order, and their types | id A_long, change_flags, ui_flags, ui_width A_short, ui_height A_short, param_type, name A_char[32], flags, u | G | PF_ParamDef |
| id and change_flags share a union `uu` | | G-inf | Parameter supervision names `uu.change_flags` |
| A 32-bit unused slot before `u` | | R | |
| Union member names (`ld sd fd fs_d ad bd cd td pd arb_d path_d button_d point3d_d`) and value types | | G | Parameters |
| Members of each definition (ranges, precision, display flags, default, popup count and names pointer, checkbox label pointer, point bounds) and their layout | | R (valid/slider min/max named: G, Parameters) | |
| Parameter type names | | G | Parameters |
| Parameter type values | LAYER 0, SLIDER 1, FIX_SLIDER 2, ANGLE 3, CHECKBOX 4, COLOR 5, POINT 6, POPUP 7, CUSTOM 8, NO_DATA 9, FLOAT_SLIDER 10, ARBITRARY_DATA 11, PATH 12, GROUP_START 13, GROUP_END 14, BUTTON 15, POINT_3D 18 | R | |
| `ui_flags` bits | bit n = the n-th of the ten names: TOPIC 0, CONTROL 1, STD_CONTROL_ONLY 2, NO_ECW_UI 3, ECW_SEPARATOR 4, DISABLED 5, INVISIBLE 9 | G-inf | PF_ParamDef (ten names in table order) |
| `flags` values | CANNOT_TIME_VARY 1, CANNOT_INTERP 2, COLLAPSE_TWIRLY 16 (= START_COLLAPSED), SUPERVISE 32 | R (names G) | |
| `PF_ChangeFlag_CHANGED_VALUE` | 1 | R (name and use G: Parameter supervision) | |
| `PF_ValueDisplayFlag_PERCENT` | bit 0 | G | PF_ParamDef ("the first bit") |
| Popup item separator | the vertical bar | R | the guide's rendering shows a slash (plan §1.1) |
| Popup entry `(-` is a separator | | G | Parameters |
| Fixed point | 16.16 | G | Parameters |
| `PF_UserChangedParamExtra` | the changed parameter's index, its only member | G-inf | Parameter supervision |
| Quality, field, world flag values | LO 0, HI 1; FRAME 0; DEEP 1, WRITEABLE 2 | R (names G) | |
| Suite-not-found error | 'S!Fd' | R | |
| Interaction callback prototypes | | G | Interaction callbacks |
| Interaction callback order: checkout_param, checkin_param, add_param, abort, progress, register_ui, three audio ones, 3 + 10 reserved slots | | R | the page lists the macros in another order |

### A3: suites and utility callbacks (`src/bin/milan-plugin-host/aex/suites.rs`)

| Item | Value | Class | Source / observation |
|---|---|---|---|
| Suites are acquired by name and version through `pica_basicP`; a missing one is an ordinary error | | G | Accessing function suites |
| `SPBasicSuite` member order and prototypes (acquire, release, is_equal, allocate, free, reallocate, undefined) | | R (AcquireSuite named: G) | |
| Suite name strings ("PF Handle Suite", "PF Iterate8 Suite", "PF World Suite", "PF Fill Matte Suite", "PF World Transform Suite", "PF ANSI Suite", "PF ColorParamSuite", "PF PointParamSuite", "PF AngleParamSuite", "PF Param Utils Suite") | | R | no page gives a name string; a wrong one only means a plug-in does not get that suite |
| Suite versions | Handle 1, Iterate8 1, World 2, Fill Matte 2, World Transform 1, ANSI 1, Color/Point/Angle Param 1, Param Utils 3 | G | the version suffix of each suite's name on its page |
| PF_HandleSuite1: functions, order, prototypes | | G | Memory allocation |
| Handle = pointer to the block pointer | | R | |
| PF_Iterate8Suite1: functions, order, prototypes | | G | Iteration suites |
| PF_WorldSuite2: functions, order, prototypes | | G | Image buffer management |
| `PF_PixelFormat_ARGB32` value | 'argb' | R (name G) | |
| PF_FillMatteSuite2: order; fill, premultiply, premultiply_color prototypes | | G | Graphics utility suites |
| PF_WorldTransformSuite1: order and prototypes (copy_hq: "the same parameters" as copy) | | G | Graphics utility suites |
| PF_ANSICallbackSuite1: order; maths return double, radians | | G | Graphics utility suites |
| ANSI argument lists (the C library's) | | R | |
| PF_ColorParamSuite1, PF_PointParamSuite1, PF_AngleParamSuite1: one function each, prototypes | | G | Parameters and floating point values |
| PF_ParamUtilSuite3: functions and prototypes in page order, two marked unsupported in version 3 left out | | G / G-inf | Parameter supervision; R: that page order is table order |
| `in_data->utils` (the utility callbacks): every slot, its order, the embedded ANSI and colour tables | | **R** | no public page. Milan serves copy, fill, blend, premultiply, premultiply_color, iterate, iterate_origin, iterate_lut, iterate_generic, new/dispose world and the handle callbacks at the positions it believes, with the prototypes of the same-named suite functions (G); every other slot notes its name and returns an error |

### A4-A5: behaviour

| Item | Value | Class | Source / observation |
|---|---|---|---|
| Command order: global setup, parameter setup, sequence setup per instance, frame setup / render / frame setdown | | G | Command selectors |
| Flatten when SEQUENCE_DATA_NEEDS_FLATTENING; resetup from the flat copy | | G (names, purpose: Command selectors); M (resetup right after a flatten, plan §4.2) | |
| Calls to one instance serialised; a host may stay single-threaded | | G | Basic host differences [S18] |
| Supervision: USER_CHANGED_PARAM for SUPERVISE parameters; values changed on the original with CHANGED_VALUE; UI changes through PF_UpdateParamUI with a copy during UPDATE_PARAMS_UI | | G | Parameter supervision |
| `checkout_param` of a layer at another time gives that frame; a layer set to none gives zeros | | G | Interaction callbacks [S14] |
| Time: current_time = frame x 1000, time_step 1000, time_scale = fps x 1000 | | M | only the ratio matters (Basic host differences [S18]) |

### Dropped for lack of a public source

| Item | Why |
|---|---|
| A6: PF Pixel Format Suite (`AddSupportedPixelFormat`), PF Utility Suite, Premiere Pixel Format Suite | The pages name the suites and some functions but give no name string, version, function order or prototype, and no numeric pixel-format values (Bigger differences; SweetPea suites; Pixel formats). Without them float frames cannot be offered; `PF_IterateFloatSuite1`'s table is public but useless without float worlds. |
| A7: PF Transition Suite (`RegisterTransitionInputParam`) and transition progress | The pages name the function and say it is called before adding the other clip's layer parameter, but give no prototype, table order, name string or version, and say nothing about how progress reaches the effect. |
| Arbitrary data parameter callbacks | `PF_ArbParamsExtra` is named and its fields only in part; its selector values and layout are not public. ARB parameters keep their defaults and are not flattened. |
