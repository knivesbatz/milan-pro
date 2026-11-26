# Interchange

What each route into and out of Milan Pro carries, converts or loses. OTIO, EDL, FCP7 XML, FCPXML and the AAF route
come with batches R28 and R29 and will be added here.

## VEGAS Pro, both ways (through VEGAS's scripting API)

Milan Pro never reads or writes `.veg` files (`docs/research/VEG_FORMAT.md` says why). Instead two C# scripts in
`tools/vegas` (installed next to `MilanPro.exe` in `vegas\`; the `(VEGAS 13)` copies for VEGAS Pro 13 and earlier)
move a project through VEGAS's own API, and one JSON file (`<name>.milan.json`, format `milan-vegas` version 2) is
the common form. Both scripts compile against VEGAS 15's `ScriptPortal.Vegas.dll`; running them inside VEGAS has
not been done here (the user's VEGAS 15 pass is the manual acceptance test).

### VEGAS to Milan Pro: `Export to Milan Pro.cs`, then File > Import > VEGAS Project...

| In VEGAS | In Milan Pro |
|---|---|
| Project width, height, frame rate, field order, pixel aspect ratio, full-resolution rendering quality, motion blur type, deinterlace method, sample rate, bit depth | The same (29.97002997 becomes 29.97) |
| Pixel format (32-bit float), master bus mode (5.1) | Reported; Milan Pro composites in 8-bit and mixes to stereo (R40b, R10a) |
| Tracks: name, mute, solo, volume, pan, composite level and mode, parent/child compositing with the Parent Composite Mode and Parent Motion, Track Motion keyframes (position, size, rotation, 3D position and X/Y rotation), 2D Shadow / Glow on | The same; Custom compositing modes fall back to Source Alpha (the compositor is not exported) |
| Track envelopes: Volume, Pan, Mute, Composite Level, Fade to Color | The same (volume in dB, +6 dB ceiling); bus sends and FX automation envelopes are reported |
| Events: start, length, in point, playback rate, reversed subclips, fades and curves, gain/opacity, mute, lock, loop, normalize, groups, takes, stream, Pan/Crop keyframes (as corners, any rotation or flip) | The same; Pan/Crop "Stretch to fill frame" is reported (imported letterboxed) |
| Event envelopes: Velocity | The same (a 0 % hold is a freeze frame); Transition Progress is reported |
| Event, track, media and video output FX: `PlugIn.UniqueID`, bypass, every OFX parameter with its keyframes | Mapped plug-ins become Milan built-ins with their settings converted (`src/interchange/vegas_map.rs`: Gaussian Blur, Brightness and Contrast, Color Corrector (sliders), Levels, HSL Adjust, Chroma Keyer, Sharpen, Unsharp Mask, Sepia, Invert, Black and White, Mirror, Border, Vignette, Crop, Cookie Cutter, LUT Filter). Unmapped ones are kept as **disabled `ofx:<id>` placeholders** holding every parameter; the OFX host plays them once the plug-in is installed (`docs/compat/ofx-vegas15.md`: VEGAS's own bundles need VEGAS, so they stay placeholders). Media FX are put on each event of the file. The import report lists every unmapped plug-in by name |
| Transitions on a fade in or out | Dissolve, Linear Wipe, Clock Wipe, Iris, Push, Slide, Zoom and Flash (as an additive Fade to Color) map; others play as a crossfade until the plug-in is installed |
| Generated media (Solid Color, Color Gradient, Checkerboard, Noise Texture, Test Pattern, Titles & Text) | Milan generators with their colours, text (plain text out of the RTF) and font; others are disabled placeholders |
| Audio FX on tracks, events and the master bus, by plug-in name and preset | Track EQ, Track Compressor, Track Noise Gate, Graphic / Paragraphic / Parametric EQ, Reverb, Simple / Multi-Tap Delay, Wave Hammer and the ExpressFX set map to Milan's audio FX (DirectX plug-ins expose no parameters to scripts, so settings do not travel); others are reported |
| Markers, regions, the time selection | The same |

Picking a `.veg` file in the import dialog instead runs the media scan: the files it names are added to Project
Media and the dialog explains the script route.

### Milan Pro to VEGAS: File > Export > VEGAS Project (via script)..., then `Import from Milan Pro.cs`

The same table in reverse (`src/interchange/vegas_export.rs`, `vegas_map` run backwards): built-ins that stand for a
VEGAS plug-in go out as that plug-in with converted parameters and keyframes, `ofx:` placeholders go back to the
plug-in they came from with every parameter, and Milan's own FX (Exposure, Bezier Masking, Color Grading apart from
its LUT, the extra blend modes, Pan/Crop masks, pitch changes, nested timelines as media, adjustment events) are
listed in the export report and left out. The VEGAS-side script asks whether to build a new project, sets the
project properties, adds media to the pool, and rebuilds tracks, events (reversed ones as reversed subclips),
takes, fades, Pan/Crop, Track Motion, envelopes, groups, markers, regions, the time selection, FX with their
parameters and keyframes, transitions, generated media and audio FX by name; what VEGAS lacks (a plug-in not
installed, a stream a file does not have) is listed when it finishes. Round trip: `vegas_export::tests::round_trip_through_the_vegas_json`
exports a project and reads it back through the import parser to the same timeline.
