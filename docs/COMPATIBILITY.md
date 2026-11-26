# Milan Pro compatibility

The plug-in and VEGAS-version matrix comes with batch R55 (`docs/REMAINING.md`). This file starts with the audio
devices (batch R9).

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
