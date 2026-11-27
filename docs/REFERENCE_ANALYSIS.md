# Reference analysis: VEGAS Pro and Milan Pro

Status: R52 (2026-11-26). The reference document the project brief asked for first: what the installed VEGAS Pro is
made of, which formats, plug-in standards and integration points it has, and how its model maps onto Milan Pro's.
Everything here was gathered by reading files and documentation; VEGAS was never launched, nothing in its folders
was executed except through the read-only OFX and DMO probes described in [compat/ofx-vegas15.md](compat/ofx-vegas15.md)
and [compat/dx-audio.md](compat/dx-audio.md), and no text, bitmap or logo of VEGAS is reproduced. Where something could
not be confirmed, the section says so.

Companion documents: [VEGAS_UI_SPEC.md](VEGAS_UI_SPEC.md) (VEGAS's interface, measured), [UI_SPEC.md](UI_SPEC.md)
(how Milan Pro's interface is drawn), [FEATURE_PLAN.md](FEATURE_PLAN.md) (the engine design), [REMAINING.md](REMAINING.md) (requirement
status and batches; its section 4 is the raw inventory this file expands), [INTERCHANGE.md](INTERCHANGE.md),
[COMPATIBILITY.md](COMPATIBILITY.md), [FORMAT_SUPPORT.md](FORMAT_SUPPORT.md) and [research/VEG_FORMAT.md](research/VEG_FORMAT.md).

Contents:
1. The reference installation
2. What Milan Pro's interface was measured from
3. Project files
4. Media formats: the FileIO plug-ins
5. Render formats
6. Plug-in standards
7. SDKs and integration points
8. Interoperability
9. Settings pages
10. Behaviours still to confirm in VEGAS
11. How VEGAS's model maps onto Milan Pro's
12. Corrections to earlier documents

---

## 1. The reference installation

**VEGAS Pro 15.0, build 177**, 64-bit, at `C:\Program Files\VEGAS\VEGAS Pro 15.0`, with its shared plug-in folder at
`C:\Program Files (x86)\VEGAS\Shared Plug-Ins`. It is the install `VEGAS_UI_SPEC.md` measured its VEGAS 15/16
interface on (section 2). The listing was taken read-only on 2026-11-12 and
re-checked on 2026-11-26.

### 1.1 Program folder

| Area | Files | What it tells us |
|---|---|---|
| Application | `vegas150.exe`, `vegas150k.dll`, `vegas150.udat`, `vegas150.oemdat`, `vegas150.zip` (resources), `vegas150.exe.config`, `vegasdeu/esp/fra/jpn.tut` (interactive tutorials), `vidcap60.exe` + `VIDCAP60.hlp` (the separate DV/HDV capture program) | One native executable with a .NET layer on top (the `ScriptPortal.*` assemblies and `log4net.dll`). Capture is a second program, which is why Milan Pro treats capture as a separate, not-planned item (R59). |
| .NET UI framework | `ControlLibrary.dll`, `CoreUI.dll`, `CoreGraphics.Native.dll`, `CorePrimitives.dll`, `WidgetLibrary.dll`, `BurgerMenuConfigDialog.dll`, `RenderAsDialog.dll`, `ScriptPortal.MediaSoftware.Skins.dll`, `Microsoft.WindowsAPICodePack*.dll` | The dockable windows, the Render As dialog and the "burger" (hamburger) menu configuration are managed code; the skin assembly is where the dark theme lives. Milan Pro draws all of this itself in egui from the measurements in `VEGAS_UI_SPEC.md`. |
| Scripting | `ScriptPortal.Vegas.dll` (the public API, 405 KB), `Script Menu\` (17 sample scripts, section 7.1), `ScriptPortal.Vegas.MobileSync.dll`, `ScriptPortal.Vegas.PublishOFA.dll`, `ScriptPortal.Vegas.RelinkSonyWirelessAdapterMedia.dll` | The only supported programmatic access to a project. Milan Pro's whole VEGAS interchange runs through it (section 8.1). |
| Project interchange | `ProjectInterchange.dll`, `ProjectInterchangeHelper.dll`, `AAFCOAPI.dll` | VEGAS's AAF import/export (section 8.2). |
| Media readers and writers | `FileIO Plug-Ins\` (31 folders, section 4), `FileIOProxyStubx64.dll`, `Release-x64.fio2007-config`, `x86\FileIOSurrogate.exe` + `x86\FileIOProxyStubx86.dll` | A plug-in model for file formats, with a 32-bit surrogate process for 32-bit readers. The `fio2007` name dates the interface. |
| Hardware codecs | `libmfxsw64.dll` (Intel Media SDK software fallback) and two GUID-named folders (`15dd936825ad475ea34e35f3f54217a6`, `2fca99749fdb49aeb121a5b63ef568f7`) holding the Intel HEVC decode/encode plug-ins; `mc_cuda` / `mc_open_cl` / `mc_cpu` subfolders inside the MainConcept AVC plug-ins; `sgcudme.dll`, `sgocldme.dll` (Sony AVC CUDA/OpenCL motion estimation) | VEGAS accelerates encoding through vendor SDKs per codec. Milan Pro reaches the same hardware through FFmpeg's NVENC, Quick Sync, AMF and Media Foundation encoders, tested at startup (R22). |
| Video plug-ins | `OFX Video Plug-Ins\` (7 bundles, section 6.1), `Video Plug-Ins\` (7 pre-OFX DLLs, section 6.4) | Two generations of video effects side by side. |
| Audio plug-ins | `sfvstwrap.dll`, `SfVstProxyStubx64.dll`, `x86\sfvstwrap.dll`, `x86\sfvstserver.exe`, `x86\sfvstproxystubx86.dll` (VST bridge, section 6.2); the DirectX plug-ins live in the shared folder (section 6.3) | VEGAS already ran 32-bit VST plug-ins out of process in 2017: a precedent for Milan Pro's `milan-plugin-host.exe`. |
| Audio hardware | `Audio Hardware Drivers\sfasio.dll`, `sfdsound.dll`, `extvid_drv.dll` | ASIO and DirectSound back ends. Milan Pro uses WASAPI through cpal and offers no ASIO ([COMPATIBILITY.md](COMPATIBILITY.md) says why). |
| Video hardware | `Video Hardware Drivers\AjaVideoDevice.dll`, `DecklinkVideoDevice.dll`, `AVCDevices.dll` (FireWire DV/HDV), `Monitor3D.dll`, `extviddev.dll`, `DVIEffect.fx/.fxo`; `AjaVideoProperties.dll`, `DecklinkVideoProperties.dll`, `ScriptPortal.MediaSoftware.ExternalVideoDevice.dll` | Preview on SDI/HDMI cards and FireWire. Milan Pro's external preview is a second window on another display (R14); SDI cards are not planned. |
| Control surfaces | `External Control Drivers\faderport.dll`, `spmackiectrlopt.dll`, `tranzport.dll`, `spgenctrlopt.dll`, `spconsoleopt.dll`, `networkXML.dll`; `Joystick Profiles\`; six `*.pref` files for Contour ShuttlePRO / ShuttleXpress | MIDI control surfaces and jog/shuttle devices (section 7.4). Not planned in Milan Pro (R59). |
| Colour management | `OpenColorIO.dll`, `OpenColorIO\` (empty on this machine: the ACES configs are fetched or installed separately) | VEGAS 15 ships OCIO for its ACES mode. Milan Pro composites in sRGB 8-bit (32-bit float is R40b) and has no OCIO. |
| Error reporting | `ErrorReport.dll`, `ErrorReportClient.exe`, `ErrorReportLauncher.exe`, `ErrorReportConfig.xml`, `CreateMinidumpx64.exe`, `dbghelp.dll` | A crash-report pipeline with minidumps. Milan Pro's R44 diagnostic report and crash offer follow the same shape (a report file the user can read before sending). |
| Layouts | `Standard Layouts\Audio Mixing.VegasWindowLayout`, `Color Correction.VegasWindowLayout`, `Default Expanded Edit Mode.VegasWindowLayout`, `Vegas 14 Default.VegasWindowLayout` | The four built-in window layouts VEGAS 15 ships. Milan Pro's built-ins (`src/ui/dock.rs`, `Builtin`) add VEGAS 23's Source and Timeline Top layouts. |
| Disc authoring | `sfdvd.dll`, `bdmux\`, `ProDiscAPI.dll`, `sfcd.cdd.dll`, `sfcdfs.dll`, `sfcdix.dll`, `sfcdsim.cdd.dll`, `sfld.ldd.dll`, `sfldsim.ldd.dll`, `sfscsi.dll`, `sfspti.dll`, `discdrv.dll`, `ScriptPortal.MediaSoftware.SfBdMuxCom.dll` | Audio CD, DVD and Blu-ray burning. Not planned in Milan Pro except an audio/data CD path if asked (R59). |
| Online services | `Online\` (MAGIX account, YouTube, Vimeo, Facebook upload DLLs, OAuth), `sfmarket2.dll` | Upload from the Render dialog. Not in Milan Pro. |
| Music identification | `gnsdk_*.dll` + `gnsdk_license.txt` (Gracenote) | Used by the Audio CD features. Not in Milan Pro. |
| Stabiliser | `proDADMercalli20.dll` | The third-party library behind the Stabilize OFX plug-in; the plug-in cannot load without it (section 6.1). |
| Miscellaneous | `MuCaAudioSync_x64.dll` (multicamera audio sync), `mxmetamux_x64.dll`, `vdlxipp61_x64.dll` (Intel IPP), `sonymvd2pro_xp.dll` (MPEG-2 video decoder), `sftutor*.dll` + `vegas.tut` (Show Me How), `ApplicationRegistration.exe`, `PRSConfig.exe`, `NGenTool.exe`, `eula\`, `Language\`, `de`/`es`/`fr` resource folders, `Protein\`, `install.cfg`, `readme` | The multicamera sync library confirms VEGAS aligns by audio in native code; Milan Pro's R17a does the same in `src/engine/sync.rs`. |

### 1.2 Shared plug-in folder

`C:\Program Files (x86)\VEGAS\Shared Plug-Ins` holds `Audio_x64\` and `Help Files\`. Section 4.2 of `REMAINING.md`
inventoried `Audio_x64` as 11 DirectX audio plug-in DLLs (Track EQ / Compressor / Noise Gate in `sftrkfx1_x64`, the
plug-in packs, the ExpressFX set, the multiband dynamics `mchammer_x64`, the vinyl restoration tool) with their `.chm`
help. The R35 probe on 2026-11-26 found no DMO from a VEGAS DLL registered on this machine (section 6.3), so the folder's
contents as listed in November may have been removed or never registered since; this document does not re-count them.
R55 re-read the folder in place (2026-11-27): the 11 DLLs are there, unregistered; their 39 plug-in names are rows of
the matrix in [COMPATIBILITY.md](COMPATIBILITY.md).

---

## 2. What Milan Pro's interface was measured from

Milan Pro has one look. Its drawing is modelled on the Windows video editors of 2005 to 2007 (Vegas 5 to 7,
Premiere Pro 2.0, Avid Xpress Pro 5, Pinnacle, Ulead, Edius, Final Cut Pro 5) and the Windows 2000/XP classic
controls: conventions, not measurements, and nothing copied ([UI_SPEC.md](UI_SPEC.md)). Its
structure (window layout, docking areas, menus, commands and keys) follows VEGAS Pro, as recorded in
[VEGAS_UI_SPEC.md](VEGAS_UI_SPEC.md) from two sources:

| VEGAS interface | Evidence | Notes |
|---|---|---|
| VEGAS Pro 15 to 18 (2017 to 2020) | Measured on the local VEGAS 15 install: pixel sizes, colours, fonts, the Preferences swatches, the keyboard table. `VEGAS_UI_SPEC.md` tags these [L]. | VEGAS 16 kept VEGAS 15's interface; the few 16-only items come from the VEGAS 16 manual. |
| VEGAS Pro 23's "modernized" interface (2025) | Official help images, release notes and forum threads ([M] and [C] in the spec). Nothing was measured locally: no VEGAS 23 is installed here. | The layout Milan Pro follows: docking on four sides, the menus, Space = Play/Pause. |

Consequences: any behaviour tagged [L] can be checked against the installed VEGAS; VEGAS 23 behaviours rest on
documentation, and where the documentation contradicts itself (Space in VEGAS 23: the release notes say Play/Pause,
the help's shortcut table still says Play/Stop) the spec records the conflict and picks the release notes. The list of
behaviours no document settles is in section 10.

---

## 3. Project files

| Extension | What it is | Milan Pro |
|---|---|---|
| `.veg` | The project. A binary, version-specific container with no public specification; projects saved by a newer VEGAS do not open in older ones. No `.veg` exists on this machine (VEGAS 15 ships no sample projects), so its chunk structure could not be observed; `research/VEG_FORMAT.md` records what was tried. | Never written. Read only for the media paths it contains (UTF-16LE strings, `interchange::scan_veg` in `src/interchange/vegas.rs`), as a fallback when the script route is not available. Milan's own project is `.mnle`, JSON (`src/project.rs`, with `format_version` and migrations in `src/project/migrate.rs`). |
| `.veg.bak` | The copy VEGAS keeps of the previous save. | `.mnle.bak` on save (R42), plus timed autosave and crash recovery (R43). |
| `.vf` | A VEGAS project template / the Movie Studio project extension (same container family). Not seen here. | Not read. Milan Pro's templates are plain `.mnle` files (`src/ui/dialogs/template.rs`). |
| `.sfk` | Peak data VEGAS writes beside each audio file, so waveforms draw without decoding again. | Milan Pro keeps peaks in its own cache (`src/engine/caches.rs`), never beside the media; Rebuild Audio Peaks (R18a) rebuilds them. `.sfk` files are not read. |
| `.sfvp0` | VEGAS's video proxy beside the source (Project Media › Create Video Proxy). Whether FFmpeg can decode one is unconfirmed (section 10). | Milan Pro makes its own proxies (`src/proxy.rs`: 1280 wide, a keyframe every 15 frames). `.sfvp0` files are ignored. |
| `.VegasWindowLayout` | A saved window layout (section 1.1). | Milan Pro layouts are JSON under the preferences folder (`src/ui/dock.rs`, `Layout`). The VEGAS files were used as the list of built-in layouts to provide, not parsed. |
| `.vegprefs` | Exported preferences (VEGAS 23). | Export/Import Preferences is R49. |
| `.milan.json` | Not a VEGAS file: the interchange document the scripts in `tools/vegas` write and read (format `milan-vegas`, version 2). | The whole VEGAS interchange ([INTERCHANGE.md](INTERCHANGE.md)). |

Also used by VEGAS: keyboard maps as `.ini` files (Milan Pro's Customize Keyboard writes the same shape,
`src/ui/dialogs/customize_keys.rs`), `.sfpreset` effect presets (Milan Pro keeps user presets in its preferences),
and `.vstpreset` for VST3 (read by Milan Pro's VST3 host, R34b).

---

## 4. Media formats: the FileIO plug-ins

Each folder in `FileIO Plug-Ins\` is one reader and/or writer. The format each covers is taken from its name, its
`.chm` help file name and the vendor libraries beside it; where the name alone is the evidence, the row says
"by name".

| Folder | Format | Direction | Evidence beside it |
|---|---|---|---|
| `aviplug` | AVI (Video for Windows codecs) | read, write | also in `Vidcap Plug-Ins` (capture target) |
| `wavplug` | WAV / Broadcast Wave | read, write | |
| `aifplug` | AIFF | read, write | |
| `flacplug` | FLAC | read, write | |
| `oggplug` | Ogg Vorbis | read, write | |
| `mp3plug2` | MP3 | read, write | |
| `ac3studioplug` | Dolby Digital AC-3 (the Studio encoder) | write (read through MainConcept `mc_dec_dd`) | |
| `atracplug` | Sony ATRAC | read, write (by name) | |
| `sfpaplug` | Perfect Clarity Audio / Sony Pictures Digital audio (by name) | read | |
| `sflgaplg` | A Sony legacy audio format (by name; unconfirmed) | read | |
| `wmfplug4` | Windows Media (WMV, WMA, ASF) | read, write | |
| `qt7plug` | QuickTime MOV (through the QuickTime 7 runtime, if installed) | read, write | |
| `mp4plug3` | MP4 with Sony AVC/MVC encoder (`savce.dll`), CUDA/OpenCL motion estimation | write | `sgcudme.dll`, `sgocldme.dll` |
| `mcmp4plug2` | MainConcept AVC/AAC MP4 (CPU, CUDA and OpenCL encoder builds) | read, write | `mc_cpu`, `mc_cuda`, `mc_open_cl` |
| `mcmp4xavcs` | XAVC S (MainConcept) | read, write | same three subfolders |
| `mcplug2` | MainConcept MPEG-2 (program/transport streams, DVD/Blu-ray profiles), MPEG audio, MXF mux | read, write | `mc_dec_mp2v`, `mc_enc_mp2v`, `mc_mux_mp2`, `mc_mux_mxf` |
| `compoundplug` | MPEG-4/H.264/AAC through Media Foundation import | read | `mc_mfimport.dll`, `mc_demux_mp4` |
| `so4compoundplug` | Sony XAVC / XDCAM "So4" container reader, Sony RAW | read | `So4Reader.dll`, `SonyRawDev.dll` |
| `mxhevcplug` | HEVC (MainConcept with OpenCL) and Sony So4 | read | `mp4ffsdk.dll`, `opencl.dll` |
| `mxfplug`, `mxfplug3` | MXF (XDCAM HD, IMX, DV) through the Sony MXF SDK (`SMDK` 4.0) | read, write | |
| `mxfp2` | Panasonic P2 MXF (DVCPRO HD, AVC-Intra) | read, write | `mc_dec_dv100`, `mc_bc_dec_avc` |
| `mxfxavc` | XAVC MXF (Sony MXF SDK 4.8) | read, write | |
| `mxfhdcamsrplug` | HDCAM SR MXF (MPEG-4 SStP) | read, write | `mp4decoder_dll.dll`, `mp4encoder_dll.dll` |
| `mxavcaacplug` | AVC/AAC MP4 (MainConcept, another profile set) | write | |
| `mvcplug` | MVC (stereoscopic AVC) | read | `sonyjvtd.dll` |
| `redplug` | RED R3D (CUDA and OpenCL debayer) | read | `REDR3D-x64.dll`, `REDCuda`, `REDOpenCL` |
| `wicplug` | Still images through Windows Imaging Component (PNG, JPEG, TIFF, BMP, GIF, HEIF if the codec is installed) | read, write | |
| `gifplug` | Animated GIF | read, write (by name) | |
| `stl2plg` | Still image sequences / legacy still reader (by name; unconfirmed); also in `Vidcap Plug-Ins` | read | |
| `sctplug` | SCT (Scitex CT) stills or a Sony capture format (by name; unconfirmed) | read | |
| `vduplug` | Video for Windows device / uncompressed (by name; unconfirmed) | read | |

What is not here: no ProRes writer (VEGAS 15 reads ProRes through QuickTime only), no DNxHD/HR, no WebM/VP9, no AV1,
no CineForm, no image-sequence EXR. VEGAS 16 to 23 added some of these through later FileIO plug-ins.

**Milan Pro** reads and writes everything through one bundled FFmpeg (`installer/fetch-ffmpeg.ps1` fetches the
`gyan.dev` full build; `src/decode.rs`, `src/media.rs`, `src/engine/vdecode.rs`), so its format list is FFmpeg's
(`REMAINING.md` section 4.3). The practical differences: Milan Pro decodes RED only if FFmpeg's build does (it does
not; R3D needs the RED SDK, which is proprietary), has no XDCAM/P2/XAVC MXF *writer* profiles beyond DNxHR MXF, and
cannot read Sony RAW or MVC; in return it reads and writes ProRes, DNxHR, CineForm, FFV1, VP8/VP9, AV1, Opus and FLAC
in Matroska, which VEGAS 15 cannot. [FORMAT_SUPPORT.md](FORMAT_SUPPORT.md) has the measured render table.

---

## 5. Render formats

VEGAS 15's Render As dialog lists one entry per writer plug-in, each with templates. From the plug-ins above, the
formats offered are: AVI, Windows Media (WMV/WMA), QuickTime (if QuickTime 7 is installed), MP4 as MainConcept AVC/AAC
and Sony AVC/MVC, XAVC S, XAVC Intra MXF, MPEG-2 (program and transport streams, DVD Architect and Blu-ray presets),
MXF (XDCAM HD/IMX, P2, HDCAM SR), Dolby Digital AC-3 Studio, WAV, AIFF, FLAC, Ogg Vorbis, MP3, ATRAC, image sequences
through WIC (PNG, JPEG, TIFF, BMP) and animated GIF. Hardware encoding goes through the vendor SDK of each plug-in
(MainConcept CUDA/OpenCL, Intel Media SDK for HEVC, Sony's CUDA/OpenCL motion estimation), chosen per template.

Milan Pro's render path (`src/engine/render.rs`, `src/render.rs`) composes every frame through the same plan,
compositor and mixer as the preview and pipes raw frames into one FFmpeg encoder; the 38 built-in templates and the
Customize Template dialog cover MP4 H.264/HEVC/AV1, QuickTime ProRes/DNxHR/CineForm, MXF DNxHR, WebM VP8/VP9, Matroska
FFV1, MPEG-2 TS/PS with AC-3, AVI MJPEG, WAV, MP3, FLAC and image sequences; NVENC, Quick Sync, AMF and Media Foundation
encoders are offered when the startup test passes (R22). Every template is rendered and checked with ffprobe by the
ignored test that writes `FORMAT_SUPPORT.md`. What VEGAS has and Milan Pro lacks: XAVC/XDCAM/P2/HDCAM SR MXF profiles,
AC-3 Studio's metadata panel, Windows Media, ATRAC, the disc-authoring presets.

---

## 6. Plug-in standards

### 6.1 OFX 1.x with VEGAS's extensions

VEGAS's video effects, transitions and generators are OpenFX bundles under `OFX Video Plug-Ins\`
(`<name>.ofx.bundle\Contents\Win64\<name>.ofx`, with `Resources\*.xml` describing the plug-ins and `Presets\`).
Seven bundles are installed:

| Bundle | Plug-ins (binary exports) | Vendor |
|---|---|---|
| `Vfx1.ofx` | 67 (the XML names 96): the generators, transitions and filters VEGAS shows as its own | VEGAS (`com.sonycreativesoftware:*`) |
| `Filters.ofx` | Stereoscopic 3D Adjust | VEGAS |
| `Stabilize.ofx` | Stabilize (proDAD Mercalli inside) | VEGAS |
| `TitlesAndText.ofx` | Titles & Text (a .NET text engine behind it) | VEGAS |
| `spica_cutout.ofx` | Smart Zoom | VEGAS |
| `spica_resizer.ofx` | Smart Upscale | VEGAS |
| `HitFilmVegasBasic.ofx` | 7 effects (Bleach Bypass, Light Flares, Scan Lines, Three Strip Color, TV Damage, Vibrance, Witness Protection) | **FXHOME, third party**, the VEGAS edition with its licence folder in the bundle |

The standard parts are OFX 1.3/1.4 image effects: `OfxImageEffectSuite`, `OfxParameterSuite`, `OfxPropertySuite`,
`OfxMemorySuite`, `OfxMultiThreadSuite`, the interact suite for overlays, and the usual contexts (filter, transition,
generator). On top of them VEGAS defines private suites that every first-party bundle requires and stops without:
an effect suite, a keyframe suite, a progress suite, a stereoscopic image-effect suite and an OpenCL program suite,
plus a host property that looks like a host verification key. These are documented only in the VEGAS Pro OFX SDK,
which is MAGIX's to license. The R33c probe ([compat/ofx-vegas15.md](compat/ofx-vegas15.md), raw data in
[compat/ofx-vegas15.csv](compat/ofx-vegas15.csv)) ran all 79 plug-ins in Milan Pro's host exactly as any OFX host
would: 73 load, 8 describe, none renders, and HitFilm Vegas Basic stops in its describe action after reading the
host's name. **No VEGAS-installed OFX plug-in runs in Milan Pro, and Milan Pro does not fake a host identity or the
private suites to change that.**

Milan Pro's OFX host (`src/engine/ofx`, `src/bin/milan-plugin-host`) implements the standard suites and runs
plug-ins out of process over a named pipe with frames in shared memory. It is verified against the project's own test
bundle (`tools/ofx-test-plugin`) and third-party plug-ins the user installs ([compat/ofx-spektrafilm.md](compat/ofx-spektrafilm.md)
is one such report). The VEGAS folder is not scanned by default.

### 6.2 VST2 and VST3 through `sfvstwrap`

VEGAS 15 hosts VST2 (and, from VEGAS 15, VST3) audio effects through its own wrapper (`sfvstwrap.dll`), with a 32-bit
server process (`x86\sfvstserver.exe`, with COM proxy/stub DLLs for both bitnesses) so 32-bit plug-ins run inside the
64-bit application. The Preferences › VST Effects page lists the search folders. On this machine
`C:\Program Files\Common Files\VST3` and `C:\Program Files\Steinberg\VSTPlugins` exist and are empty, so no VST plug-in
could be tried against VEGAS's wrapper or Milan Pro's host.

Milan Pro hosts VST3 (R34a/b) and CLAP (R35) in `milan-plugin-host.exe`, through the `vst3` and `clap-sys` bindings,
with plug-in delay compensation, editor windows and `.vstpreset` files. **VST2 is not possible**: Steinberg no longer
licenses the VST2 SDK, and there is no clean-room binding Milan Pro could ship. The project's test plug-ins
(`tools/vst3-test-plugin`, `tools/clap-test-plugin`) and free third-party effects are the verification set; VEGAS has
nothing to contribute here beyond the precedent of running plug-ins in a separate process.

### 6.3 DirectX / DMO audio plug-ins

VEGAS's own audio effects (Track EQ, Track Compressor, Track Noise Gate, the ExpressFX set, the plug-in packs) are
DirectX Media Objects: COM objects implementing `IMediaObject`, registered as audio effects, with parameters through
`IMediaParams`. The R35 research spike ([compat/dx-audio.md](compat/dx-audio.md)) showed that a DMO processes audio
outside its host (Windows's own ParamEq did), that the VEGAS DMOs were not registered on this machine when the probe
ran, and that they are licensed as part of VEGAS. **Milan Pro ships no DMO host**: the format is deprecated, its only
interesting plug-ins are VEGAS's, and VST3/CLAP cover third-party effects. VEGAS's audio FX travel across the
interchange by *name* (Milan Pro's built-in EQ, Compressor, Noise Gate, Reverb, Delay and Limiter stand in for them,
[COMPATIBILITY.md](COMPATIBILITY.md)); their settings do not, because the scripting API exposes no DMO parameters.

### 6.4 Legacy video plug-ins

`Video Plug-Ins\` holds the pre-OFX generation: `vfx1.dll`, `vfplugs.dll`, `colorcorrector.dll`, `sfpagepeel.dll`,
`sftrans1.dll`, `vidpcore.dll` and `PluginWrapper.dll`. They use a VEGAS-private COM interface from the Sony Vegas
era; `PluginWrapper.dll` presents them to the OFX-based pipeline. There is no SDK for them, and VEGAS 15 still loads
them for projects that reference the old effect ids. Milan Pro has no equivalent and needs none: the interchange
scripts see effects through `Effect.PlugIn.UniqueID`, and `vegas_map` (`src/interchange/vegas_map.rs`) maps the OFX ids.

### 6.5 Other plug-in points

| VEGAS | Mechanism | Milan Pro |
|---|---|---|
| FileIO plug-ins | A VEGAS-private C++ SDK (`fio2007`), partner-only | FFmpeg out of process; no plug-in interface for formats |
| Video capture plug-ins (`Vidcap Plug-Ins\aviplug`, `stl2plg`) | Capture targets for `vidcap60.exe` | Not planned (DV/HDV capture, R59) |
| External video devices | AJA, Decklink, DV, 3D monitor driver DLLs | A second preview window (R14) |
| Control surfaces | The driver DLLs in section 1.1 | Not planned (R59) |
| Scripts | .NET assemblies loaded in process (section 7.1) | Not feasible as such; a Rhai alternative is proposed (R60) |

---

## 7. SDKs and integration points

### 7.1 The .NET scripting API

`ScriptPortal.Vegas.dll` is the public object model VEGAS gives scripts (C#, JScript, VB.NET, compiled or run from
source through Tools › Scripting). VEGAS 13 and earlier shipped the same API under the `Sony.Vegas` namespace, which
is why `tools/vegas` carries a `(VEGAS 13)` copy of each script differing only in its `using` line. The assembly was
inspected by reflection only (`research/VEG_FORMAT.md`); VEGAS was never run.

What the export script ([tools/vegas/Export to Milan Pro.cs](../tools/vegas/Export%20to%20Milan%20Pro.cs)) reads, by
class: `Vegas` (the application, entry point `FromVegas(Vegas)`), `Project` (file path, video and audio properties,
tracks, markers, regions, the media pool), `VideoTrack` / `AudioTrack` (name, mute, solo, volume, pan, composite mode
and level, parent/child compositing), `TrackMotion` / `VideoMotionKeyframe` (Track Motion keys, 2D shadow and glow
flags), `Envelope` / `EnvelopePoint` (track and event envelopes by type), `TrackEvent` / `VideoEvent` / `AudioEvent`
(start, length, playback rate, fades with their curve types, gain, mute, lock, loop, normalize, groups, the active
take), `Take` / `Media` / `MediaStream` / `VideoStream` (the file, the stream index, subclip and reverse flags),
`VideoMotion` (Pan/Crop keys as corner bounds), `Effect` / `OFXEffect` / `OFXParameter` (every FX on events, tracks,
media and the video output, by plug-in unique id, with each parameter's keyframes), `Transition`, `Generator` (generated
media and its parameters), `Fade`, `Marker`, `Region` and the time selection. The import script
([tools/vegas/Import from Milan Pro.cs](../tools/vegas/Import%20from%20Milan%20Pro.cs)) uses the constructing side of
the same classes: `Media`, `Subclip`, `TrackEventGroup`, `Envelope`, `EnvelopePoint`, `Effect(PlugInNode)`,
`VideoMotionKeyframe` / `VideoMotionBounds`, `TrackMotion`, `Marker`, `Region`, `Timecode.FromSeconds`, the
`CompositeMode`, `EnvelopeType` and `VideoFieldOrder` enumerations and `OFXColor` parameters. Both scripts compile
against the VEGAS 15 assembly with the .NET Framework C# compiler; running them inside VEGAS is the user's acceptance
test and has not been done by the developers.

Also in the API, not used by Milan Pro: `Project.SaveProject`, `Project.BeginWrite` with the AAF and archive save
types, `Project.ArchiveProject`, renderers and render templates (`Renderer`, `RenderTemplate`, `RenderArgs`), the
capture and device-explorer assemblies, and the per-window state VEGAS exposes to scripts.

The 17 scripts VEGAS 15 ships in `Script Menu\` show what the API was meant for; their behaviours map onto Milan Pro
as follows:

| VEGAS script | Milan Pro |
|---|---|
| Batch Render, Render Audio Tracks | Tools › Scripting › Batch Render and Render Audio Tracks (R12, built in) |
| Export EDL (JScript) | File › Export › EDL (R28) |
| Export Regions as Subtitles; Export Closed Captioning for DVD Architect / QuickTime / Windows Media Player / YouTube; Promote Media Closed Captioning | Subtitle export (R39) |
| Export Chapters | Chapters from markers in the render templates (R11) |
| Group Video and Audio Events | Edit › Group (R3a) covers it |
| Promote Media Markers | Project Media › media markers to timeline markers (R57) |
| Stereo to Mono | Event Channels ▸ (R3b) |
| Resize Generated Media | Tools › Scripting › Resize Generated Media (R18b, built in) |
| Remove Letterboxing | Pan/Crop presets (R57 candidate) |
| Add / Remove Timecode to all Media | The Timecode effect as a media FX (R57) |

### 7.2 The FileIO SDK

The reader/writer interface behind `FileIO Plug-Ins\` is private to VEGAS and its codec partners (MainConcept, Sony,
Intel, RED); nothing public documents it, and its `fio2007` configuration file name is the only version marker here.
Milan Pro does not implement or emulate it.

### 7.3 The OFX SDK

Standard OpenFX headers are BSD-licensed and public; Milan Pro's host is written against them (`src/engine/ofx`).
VEGAS's extension headers come with the VEGAS Pro OFX SDK, which is not public and whose terms are MAGIX's; Milan Pro
does not use them (section 6.1).

### 7.4 External control and device drivers

Control surfaces (Mackie Control, PreSonus FaderPort, Frontier TranzPort, a generic MIDI controller, an XML-over-network
driver) plug into VEGAS through the DLLs in `External Control Drivers\`, configured on the Preferences › External
Control & Automation page; Contour ShuttlePRO/Xpress profiles are `.pref` files for the Contour driver. Video devices
(AJA, Decklink, FireWire DV) and audio devices (ASIO, DirectSound) are driver DLLs of the same kind. None of these
interfaces is public. Milan Pro's position: control surfaces and MIDI are the R59 long tail (a `midir`-based path is
feasible if asked), ASIO is not shipped for licensing reasons, SDI output is not planned.

---

## 8. Interoperability

### 8.1 VEGAS projects: the script route

Milan Pro never parses or writes `.veg`. Projects cross in both directions through the two scripts and one JSON
document; [INTERCHANGE.md](INTERCHANGE.md) lists, field by field, what carries, what converts and what is reported
as lost. The import side is `src/interchange/vegas.rs` (parsing, media probing, project building) and
`src/interchange/vegas_map.rs` (VEGAS plug-in ids and parameters to Milan built-ins, with scaling); the export side is
`src/interchange/vegas_export.rs` (the same map run backwards, plus an export report of what VEGAS lacks). A round-trip
test exports a project and reads it back through the import parser. The media scan of a `.veg` is the no-VEGAS
fallback.

### 8.2 AAF

VEGAS imports and exports AAF through `AAFCOAPI.dll` (the AAF Association's reference implementation, COM-based) and
`ProjectInterchange.dll`; the scripting API exposes it as the `Project_AAF` and `Project_AvidAAF` save types. There is
no maintained Rust AAF library and the reference SDK is large C++ structured storage, so Milan Pro neither reads nor
writes AAF: the route is OpenTimelineIO's AAF adapter (external, Python, `otioconvert`) on either side of Milan Pro's
OTIO import and export, documented in [INTERCHANGE.md](INTERCHANGE.md) (R29). `REMAINING.md` row 9.6 records AAF as
not feasible as stated, with that route as its deliverable.

### 8.3 EDL

VEGAS's EDL export is a bundled JScript (`Export EDL.js`) that writes a CMX-style list from the timeline; there is no
EDL importer in VEGAS 15. Milan Pro reads and writes CMX3600 (V and A1/A2, cuts, dissolves, speed lines, reel names
from file names, drop and non-drop frame) in `src/interchange/edl.rs` (R28).

### 8.4 XML and other editors

VEGAS has no FCP XML or FCPXML path of its own; third-party scripts do it through the scripting API. Milan Pro
reads and writes FCP7 `xmeml` (`src/interchange/xmeml.rs`), FCPXML 1.10 (`src/interchange/fcpxml.rs`) and
OpenTimelineIO (`src/interchange/otio.rs`) directly (R28, R29), which also reach Premiere and Resolve. Milan Pro's own
`.mnle` is JSON with a schema version, so other tools can read it without Milan Pro.

### 8.5 Closed captions and subtitles

Through the bundled scripts VEGAS turns regions and media captions into DVD Architect, QuickTime, Windows Media and
YouTube caption files. Milan Pro's subtitle export is R39.

---

## 9. Settings pages

**Options › Preferences** in VEGAS 15/16 is a Win32 property sheet of about 433×670 px with two rows of tabs,
measured locally (`VEGAS_UI_SPEC.md` §5.15): General, Video, Preview Device, Audio, Audio Device, MIDI, VST Effects in
the front row; Editing, Display, CD Settings, Sync, External Control & Automation behind. VEGAS 23 adds File I/O and
Deprecated Features, and Options › Export / Import Preferences.

| Page | VEGAS content | Milan Pro (`src/ui/dialogs/prefs.rs`, `src/settings.rs`) |
|---|---|---|
| General | A scrolling checklist (about 24 visible rows, autosave, warnings, recently-used list length, the temporary files folder, free-space line) | The same page shape; each option is live or labelled with its batch (R49 finishes the remaining ones) |
| Video | Dynamic RAM preview size, thumbnails, GPU acceleration, external preview, 32-bit float options | Live: RAM preview (R23), GPU compositing (R21), extra blend modes, the disk frame cache; 32-bit float is R40b |
| Preview Device | The external monitor device (Windows secondary display, AJA, Decklink, DV) and its options | The second-window external preview (R14); the device list is R49 |
| Audio | Metering, recording defaults, downmix | R8/R9 options; the page goes live in R49 |
| Audio Device | Device type (Sound Mapper, Classic Wave, ASIO), playback and record devices, buffering | Live, with WASAPI Exclusive as Milan's own type and no ASIO ([COMPATIBILITY.md](COMPATIBILITY.md)) |
| MIDI | MIDI in/out devices | Labelled "not planned" (R59) |
| VST Effects | Search folders, rescan | Milan Pro's VST3/CLAP folders and rescan live on the Plug-In Manager and the VST page (R34, R35) |
| Editing | Cut-to-overlap, quick fade lengths, still image length, Auto Crossfade and ripple defaults, JKL speeds | Live (R1 to R4) |
| Display | Track, envelope and snap colours, icon colour strength, button highlight colour, interface type, docking options | Live: the colours are the look's tokens; the docking checkboxes drive `src/ui/dock.rs` |
| CD Settings | Burn drive and speed | Labelled "not planned" (R59) |
| Sync | MIDI timecode and clock generation / triggering | Labelled "not planned" (R59) |
| External Control & Automation | Control surfaces, automation recording options | The automation options are live (R8c); control surfaces are R59 |
| File I/O (23) | Decoder choices per format, proxy settings | Decoder choices are FFmpeg's; proxies in Project Media |
| Deprecated Features (23) | Re-enable features VEGAS removed | No equivalent |

**Project Properties** (Alt+Enter) has Video, Audio, Ruler, Summary and Audio CD tabs; Milan Pro's dialog has the same
tabs (`src/ui/dialogs` and `src/project.rs`, `ProjectSettings`, `AudioSettings`, `Ruler`), with the 15 ruler formats
and the exact 30000/1001 timebase (R1), master bus mode and the audio properties (R8b).

---

## 10. Behaviours still to confirm in VEGAS

`FEATURE_PLAN.md` section 3 ends with the list of VEGAS behaviours the engine design could not settle from
documentation. They stand, with the current Milan Pro choice beside each; R55 records the user's answers.

| Question | Milan Pro today |
|---|---|
| The video crossfade rule for a same-track overlap without a transition (true dissolve, or both fades drawn) | A true dissolve: outgoing at full alpha, incoming on top with its fade-in (decision 0.9) |
| What an Auto Ripple trim of an event's *start* moves | The later events move by the trimmed amount (`src/edit.rs`) |
| Whether the VP16 Selection tool's rubber band also sets the time selection | It selects events only |
| Whether Insert Track goes above or below the focused track | Above |
| The exact names of the Fade to Color, Flash and Additive Dissolve transitions | As `vegas_map` names them |
| Whether new Pan/Crop and Track Motion keys default to Smooth | Smooth |
| Whether Track FX run before or after Track Motion | Track FX first, then Track Motion (`src/engine/plan.rs`) |
| Whether Bypass All Audio FX affects a render | It does not: it is a monitoring switch |
| The scripting API property names used by the export and import scripts | Compiled against the assembly, not run |
| Whether FFmpeg can decode `.sfvp0` proxy files | Unknown; Milan Pro makes its own proxies |

Added by this analysis: whether the `Audio_x64` DirectX plug-ins were ever registered on this machine (section 1.2),
and which formats `sflgaplg`, `stl2plg`, `sctplug` and `vduplug` really cover (section 4).

---

## 11. How VEGAS's model maps onto Milan Pro's

Milan Pro's data model is `src/project.rs` (one `Project`: `ProjectSettings`, a `Timeline` of video and audio
`Track`s holding `Clip`s, a media pool of `MediaItem`s, `Marker`s, `Region`s, `AudioSettings` with `Bus`es, and a
`MasterBus`). The engine (`src/engine`) turns it into plans: `plan.rs` compiles the timeline into a video plan (one
`frame_plan` per frame) and an audio plan, `composite.rs` (CPU) and `gpu/` (wgpu) compose a frame plan, `mix.rs` mixes
the audio plan block by block, and `render.rs` runs both into one FFmpeg encoder. The table says, concept by concept,
what VEGAS has, what Milan Pro has, and where they part on purpose.

### 11.1 Tracks

| VEGAS | Milan Pro | Deliberate differences |
|---|---|---|
| Video and audio tracks in one list, each with a header (name, mute, solo, level/volume, pan, FX, compositing mode, Track Motion), a colour from the Preferences swatches, and a height | `Track` with `kind: TrackKind`, `clips`, `muted`, `solo`, `locked`, `visible`, `hidden`, `height`, `volume`, `pan`, `composite`, `motion`, `fx`, `envelopes`, `fade_colors`, the compositing fields and the R8 audio routing fields | Milan Pro adds `locked` (VEGAS locks events, not tracks) and `hidden` (R5) and keeps `armed` and `send_view` as session state. Track colours are the look's palette, chosen per track as in VEGAS 23. |
| Parent/child compositing (Make Compositing Child, Parent Composite Mode, Parent Motion) | `child`, `parent_mode`, `parent_motion`: a child composites into the nearest non-child track above, and the parent's group goes onto the tracks below through `parent_mode` | Same model, including the Custom mode's compositor plug-in (`compositor: Option<Fx>`) |
| Adjustment Tracks (VEGAS 23) | `adjustment: bool`: the track's FX apply to the composite below it, as strong as its level envelope | Same |
| Track groups (collapsible, group mute/solo) | `TrackGroup` in the timeline, with `group_of` lookups | Same (R5) |
| Audio track routing: output to master or a bus, sends, pan type, input monitoring, automation mode | `out: Out`, `sends: Vec<AudioSend>`, `pan_type`, `monitor`, `automation`, `auto_controls` | The six VEGAS pan types are implemented (`src/engine/mix.rs`); stereo output only until R10a |

### 11.2 Events, takes and media

| VEGAS | Milan Pro | Deliberate differences |
|---|---|---|
| An event is a span of a media stream on a track: start, length, offset into the media, playback rate, reverse, fades with curve types, gain or opacity, mute, lock, loop, normalize, maintain aspect, interlace flicker reduction, resample mode, undersample rate, channels, stream, pitch | `Clip` with `position`, `duration`, `in_point`, `out_point`, `rate`, `reverse`, `fade_in/out` + curves, `gain_db`, `muted`, `locked`, `looping`, `normalize`/`auto_normalize`, `keep_aspect`, `reduce_flicker`, `resample`, `undersample`, `channels`, `stream`, `pitch_cents` | The same switch set. Opacity for video and gain for audio share `gain_db`. `adjustment` marks a VEGAS 23 Adjustment Event. |
| Takes: a list of media per event, one active; VEGAS 15 keeps the alternatives in the event | `takes: Vec<Take>` holds the *inactive* takes (media, in point, name); the active take is the event's own `media_id`, and `take_no` keeps the numbering stable for multicamera | Same behaviour (T / Shift+T, the Take Chooser); storage differs but is invisible |
| Subclips (Project Media, with reverse) | `MediaItem` + `Subclip` ranges | Same |
| Media pool: files with streams, media FX, tags, bins | `MediaItem` (`MediaType`, streams, probe data, `generator: Option<Fx>` for generated media, media FX) | Generated media is a media item with a generator plug-in, not a separate class (decision 0.6) |
| Event groups (A/V pairs and user groups) | `group: Option<String>` on each clip | Same |
| Event FX chain; media FX on the file | `fx: Vec<Fx>` on the clip; media FX on the `MediaItem` | Same |
| Event Pan/Crop with keyframes and Bezier masks | `pan_crop: Keyframes<PanCrop>` (centre, size, rotation, flip by negative size, normalised to the source) and `masks: Vec<Mask>` (positive/negative Bezier paths) | Keys are event-relative and stay with the picture when the event is trimmed (decision 0.11) |
| Velocity envelope, Transition Progress envelope | `envelopes` on the clip (`EnvKind::Velocity`); `Transition.progress` | Same, including freeze frames as a 0 % hold |
| Transitions between overlapping events, with a plug-in and parameters | `transition: Option<Transition>` stored on the *incoming* event; `transition_out` for a fade-out into nothing | VEGAS attaches the transition to the overlap; Milan Pro stores it on the incoming event (decision 0.12), which the interchange converts both ways |

### 11.3 Envelopes and keyframes

| VEGAS | Milan Pro | Deliberate differences |
|---|---|---|
| Track envelopes (Volume, Pan, Mute, Composite Level, Fade to Color, bus sends, FX automation), event envelopes (Velocity, event Volume/Pan/Mute), bus envelopes; points with Linear, Fast, Slow, Smooth, Sharp, Hold curves | `Envelope { kind: EnvKind, points: Keyframes<f32> }` on tracks, clips and buses; `Keyframe<T> { t, v, curve: Curve }`; FX automation in `Fx.auto` per parameter; automation modes Off / Read / Write (Touch) / Write (Latch) | The same curve set. Milan Pro keeps every keyed value (Pan/Crop, Track Motion, FX parameters, envelopes) in one generic `Keyframes<T>` (`src/engine/keyframes.rs`), so the keyframe controller and the envelope editing share code. |
| Lock Envelopes to Events | `lock_envelopes` preference; `edit::shift_clip_keys` | Same |

### 11.4 Busses, FX chains and the master

| VEGAS | Milan Pro | Deliberate differences |
|---|---|---|
| Audio busses (up to 26), assignable FX chains, input busses, the master bus; each with FX, volume, pan, mute, envelopes, output routing | `AudioSettings { buses: Vec<Bus>, fx: Vec<Bus> (assignable chains), ... }`; `Bus` has `fx`, `volume`, `pan`, `muted`, `envelopes`, `out`, `automation`; `MasterBus` has `gain`, `audio_fx`, `video_fx`, the Video Bus Track's envelopes | Same topology (R8a/b). The master bus is stereo until R10a. |
| Video Output FX (the Video Bus) with Fade to Color, Motion Blur Amount and Video Supersampling envelopes | `MasterBus.video_fx` and `video_envelopes`, applied in `plan.rs` after the last track | Same |
| FX chains anywhere: a list of plug-in instances with bypass, parameters, presets | `Fx { id, plugin, enabled, params: BTreeMap<key, Keyframes<f32>>, text, auto }`; one registry for video FX, transitions, generators, audio FX and compositors (`src/engine/plugins.rs`) | VEGAS has separate plug-in models per kind; Milan Pro has one (decision 0.4) and one FX window for every target (decision 0.5). Plug-in ids are `milan.*` for built-ins, `ofx:`, `vst3:` and `clap:` for hosted plug-ins. |

### 11.5 Pan/Crop, Track Motion and compositing

VEGAS composites bottom to top: each track's events go through their Pan/Crop (a crop and transform of the source
into the frame), event FX, then the track's Track FX, Track Motion (position, size, rotation, with 3D position and
X/Y rotation in the 3D Source Alpha mode, plus 2D shadow and glow), and the track's compositing mode and level onto
the result of the tracks below; parent/child groups composite as a unit; the video output FX and the Video Bus Track's
envelopes finish the frame. Milan Pro's `plan.rs` produces exactly that order per frame (`TrackPlan` / `EventPlan` with
an `Affine` per stage, `MaskPlan` for Pan/Crop masks, `Halo` for shadow and glow, `Composite` per track), and
`composite.rs` or the GPU compositor executes it. The compositing modes are VEGAS's list (`Composite` in
`src/project.rs`) plus Milan's own extra blend modes behind a preference, which the interchange reports rather than
converts. Whether Track FX precede Track Motion is on the confirm list (section 10); the plan's order is the one
VEGAS's FX window layout implies (FX on the track's picture, then the motion).

### 11.6 Project and render pipeline

| VEGAS | Milan Pro | Deliberate differences |
|---|---|---|
| Project properties: size, frame rate, field order, pixel aspect, full-resolution rendering quality, motion blur type, deinterlace method, pixel format (8-bit / 32-bit float), sample rate, bit depth, master bus mode, ruler format and tempo | `ProjectSettings` and `AudioSettings` with the same fields; `Ruler` with 15 formats and the 30000/1001 timebase | 8-bit sRGB compositing until R40b; stereo until R10a |
| Preview: a frame cache, Dynamic RAM Preview, Selectively Prerender, proxies, preview quality Draft/Preview/Good/Best × Auto/Full/Half/Quarter, Adjust Size and Quality for Optimal Playback | The "milan-video" worker composes frame plans from persistent FFmpeg decoder processes (`src/engine/worker.rs`, `src/engine/vdecode.rs`); RAM preview, prerender and a disk frame cache (R23); proxies (`src/proxy.rs`); the same quality grid; adaptive quality (R19) | Decoders are child processes, not in-process codecs: a decoder crash cannot take the editor down, at the cost of a pipe per stream |
| Playback: audio-clocked, with JKL shuttle and a rate slider | `src/audio.rs` (cpal / WASAPI) drives the clock; the mixer runs on its own thread from the audio plan; the video worker follows the clock frame (`FEATURE_PLAN.md` §1.4 to 1.6) | Same model |
| Render: per-plug-in encoders with templates, Customize Template, a render queue (VEGAS 23), Batch Render script | `src/engine/render.rs` composes every frame at the template's size and pipes raw video and f32 audio into one FFmpeg process; templates in `src/render.rs`; the queue, Batch Render and Render Audio Tracks (R12) | Everything goes through FFmpeg, so the format list is FFmpeg's (section 5); a render works on a snapshot of the project so editing continues |
| Nested projects (`.veg` as media) | `.mnle` as media, played and rendered through the engine with a cycle guard (R6) | Same idea |

### 11.7 Windows and docking

| VEGAS | Milan Pro | Deliberate differences |
|---|---|---|
| Dockable windows (Explorer, Project Media, Trimmer, Video Preview, Master Bus, Mixing Console, Transitions, Video FX, Media Generators, Compositors, Plug-In Manager, Video Scopes, Edit Details, Loudness Meters, Color Grading, Device Explorer...) in one docking area above or below the timeline (VEGAS 15/16) or four areas (VEGAS 23), plus floating windows; tabs at the bottom of each group; layouts saved per user with Alt+D chords | `Panel` (18 windows), `Dock`, `Area` (top, bottom, left, right), `Group`, `FloatWin`, `Layout` and `Builtin` in `src/ui/dock.rs`; four docking areas as in VEGAS 23; floating windows are real OS windows (egui viewports) that keep working with the keyboard | Layouts are JSON files, not `.VegasWindowLayout`. Device Explorer, Dashboard and Motion Tracking windows exist as greyed commands only (R59). |
| Modal dialogs as Win32 property sheets (light, native) in 15/16; dark in 23 | `src/ui/dialogs/mod.rs` draws every dialog frame in the look's Windows 2000-style chrome | The same dialogs and tabs, drawn in Milan Pro's own look |
| Menus, keyboard shortcuts, the toolbar sets, mnemonics | `src/ui/menus.rs` (the trees of §6 of the UI spec), `src/ui/keymap.rs` (the §7 table, with user overrides), `src/ui/toolbar.rs` | Milan Pro adds a command palette (R47) on a chord VEGAS does not use; every VEGAS command Milan Pro cannot run is greyed with the batch that builds it (R0), which VEGAS of course has no need for |

### 11.8 Where Milan Pro differs on purpose, in one list

- **Project file**: JSON (`.mnle`) with a schema version and migrations, instead of an undocumented binary; readable by other tools, diffable, and recoverable by hand.
- **Decoding, encoding and plug-ins in separate processes**: FFmpeg decoders and the encoder are child processes; OFX, VST3 and CLAP plug-ins run in `milan-plugin-host.exe`. VEGAS runs codecs in process and only 32-bit VSTs out of process.
- **One FFmpeg instead of 31 FileIO plug-ins**: fewer formats on the professional MXF side, more on the open side (section 4), and no vendor SDK licences to carry.
- **No VEGAS plug-ins**: VEGAS's OFX bundles, DirectX effects and legacy video plug-ins are licensed with VEGAS and check their host; Milan Pro maps the common ones to built-ins and hosts standard third-party OFX, VST3 and CLAP instead (section 6).
- **One plug-in registry, one keyframe type, one FX window** (decisions 0.4, 0.5 and §11.3), where VEGAS has a model per kind.
- **Transitions on the incoming event** and **event-relative keys** (decisions 0.11, 0.12): the interchange converts both.
- **Milan-only features** that the VEGAS export leaves out and reports: extra blend modes, Exposure, Bezier Masking as a plug-in, Color Grading beyond its LUT, pitch changes, nested `.mnle` media, adjustment events in VEGAS 15 (VEGAS 23 has them), WASAPI Exclusive mode, the command palette, the mid-2000s look.
- **Not built, by decision** (R57 to R60 in `REMAINING.md`): disc burning, capture, control surfaces, MIDI sync, the .NET script API (a Rhai scripting alternative is proposed), stereoscopic 3D, Blu-ray authoring.

---

## 12. Corrections to earlier documents

- `FEATURE_PLAN.md` section 3 ("OFX hosting") said the user had no third-party OFX installed. The install has one:
  FXHOME's HitFilm Vegas Basic bundle, which is third-party code in the VEGAS edition keyed to VEGAS; it does not run
  in Milan Pro (section 6.1). The line is corrected in this batch.
- `REMAINING.md` section 4.2 counts 102 OFX plug-ins from the bundles' XML resources; the binaries export 79
  (`compat/ofx-vegas15.md`). Both numbers are right for what they count.
- `REMAINING.md` section 4.2 lists 11 DirectX audio DLLs in `Audio_x64`; the R35 probe found no VEGAS DMO registered
  on 2026-11-26 (section 1.2). The listing is kept as recorded; the registration state is noted as unconfirmed.
