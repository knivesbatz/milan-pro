# Spektrafilm OFX in Milan Pro

Spektrafilm (https://github.com/chaert-s/spektrafilm-ofx) is a film-emulation OFX plug-in written for DaVinci Resolve and Nuke. This page compares what its source asks of a host with what Milan Pro's OFX host provides (Milan Pro 0.5, `milan-plugin-host.exe`).

**Status: expected to work, not verified with the real binary.** The comparison below comes from reading Spektrafilm's source (`src/SpektraFilmPlugin.cpp`, `src/SpektraVulkanRenderer.cpp`, `src/SpektraParameters.h` on the main branch, November 2026). Milan Pro's host was tested with its own test plug-in (`tools/ofx-test-plugin`), not with Spektrafilm's release binaries, which were neither downloaded nor run while building this.

## What Spektrafilm needs, and what Milan Pro provides

| Spektrafilm needs | Milan Pro | Notes |
|---|---|---|
| `OfxSetHost` (OFX 1.5 export), `OfxGetNumberOfPlugins` (1), `OfxGetPlugin` | Yes | `OfxSetHost` is called when exported, before the plug-ins are listed. |
| Suites: Image Effect v1, Property v1, Parameter v1 (its load fails without them), Message v1 | Yes | Also offered: Memory, MultiThread, Progress v1/v2, TimeLine, Message v2. |
| Context: Filter only | Yes | Filter and General are run. |
| Pixel depths: 16-bit half and 32-bit float (no 8-bit) | Float | Milan Pro hands it 32-bit float RGBA. Half is not offered and not needed. |
| Components: RGBA on Source and Output | Yes | Premultiplied, bottom row first, as OFX defines. |
| Tiles off, temporal access off, multiple clip depths off | Yes | Milan Pro always gives the whole frame at one depth. |
| Host name checks (Resolve, Nuke) | None found | Its source does not check the host's name. |
| GPU render suites (CUDA, OpenCL, Metal, OpenGL) | Not provided | Not needed on Windows: Spektrafilm takes the host's CPU float buffers and runs its own Vulkan 1.1 compute pipeline inside the plug-in. Its Metal buffer path is macOS-only and off by default. |
| A Vulkan 1.1 device with a compute queue and its driver | Up to your PC | Spektrafilm has no CPU fallback: without Vulkan its render fails ("No Vulkan physical device is available." and similar). Milan Pro then bypasses it and shows the plug-in's message. |
| Shader files from its bundle (`Contents\Resources\shaders\*.spv`, found from the DLL's own path) | Yes | The bundle is loaded in place from `C:\Program Files\Common Files\OFX\Plugins`. |
| OFX 1.5 colour management (`kOfxImageEffectColourManagementCore`) | Not provided | Milan Pro does not implement OFX colour management. Spektrafilm's own Input and Output colour space parameters decide. |
| Parameters: Group, Choice, Double, Integer, Boolean, RGB, Double2D, Double3D, String, PushButton | All but PushButton | Groups become headings in the FX window; values are keyframable and saved in the project. Push buttons are not shown (see below). |
| `paramGetValueAtTime` at the render time | Yes | Milan Pro sends the values at the frame being rendered. Other times read the same values. |
| `instanceChanged` with `paramSetValue`, `paramEditBegin/End`, `propReset` and `propSetStringN` on choice options | Yes | Changes the plug-in makes itself stay in the plug-in host. They are not written back to the project, and the FX window shows the choice options from when the plug-in was described. |
| `kOfxImageEffectActionEndSequenceRender` and `PurgeCaches` free its GPU resources | Partly | End Sequence Render is sent when an instance is closed (at most 16 instances live per host). Purge Caches is never sent. |
| Message suite for errors | Yes | Messages are logged and added to the failure text, never shown as dialogs. |

## What you should know

- **Input colour space.** Spektrafilm's default input colour space is ARRI LogC4. Milan Pro's frames are display-referred Rec.709/sRGB pictures (0 to 1). Set Spektrafilm's **Input colour space** to **sRGB** (or **Rec709 Gamma 2.4**, whichever matches your footage) and the **Output colour space** to Rec709 Gamma 2.4 or sRGB. Otherwise the look will be far off. Milan Pro cannot set this for you, because it does not implement OFX colour management.
- **Push buttons are not available:** Save Preset, Load Preset, Export LUT, Copy Params, Save/Reset Defaults and Open Manual do nothing in Milan Pro. Use Milan Pro's own FX presets to save settings.
- **Hidden parameters may show.** Spektrafilm hides some parameters by turning them off (`kOfxParamPropEnabled = 0`) and changes which ones show at run time. Milan Pro hides only parameters marked secret, so a few extra controls may appear.
- **Draft and Preview quality.** Spektrafilm does not read the render scale in the code that was read. In Draft or Preview quality it gets a smaller picture, so grain and halation can look coarser in the preview than in the full-quality render. The render itself runs at full size.
- **Speed.** Every frame goes through shared memory to the plug-in host and back as 32-bit float. That costs a few milliseconds at 1080p on top of Spektrafilm's own GPU time.
- **If it fails**, Milan Pro bypasses Spektrafilm for the rest of the session, says why in the status bar and the FX window, and keeps your settings. Fix the cause (for example a GPU driver), then press **Rescan** in View > Window > Plug-In Manager.

## Installing

Run Spektrafilm's own installer (`install.bat` copies the bundles to `C:\Program Files\Common Files\OFX\Plugins`), then start Milan Pro or press **Rescan** in the Plug-In Manager. Spektrafilm appears in the Video FX window under its grouping (`spektrafilm OFX` in its source).
