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
`.veg` files (`docs/INTERCHANGE.md`). The scripts in `tools/vegas` compile against
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
