# Interchange

What each route into and out of Milan Pro carries, converts or loses. Every import ends in the same report dialog
(what did not carry over, line by line); every export's report lists what the file has no place for. The format is
picked by the file's extension (`.milan.json` / `.veg`, `.otio`, `.edl` / `.txt`, `.xml`, `.fcpxml`;
`src/interchange/mod.rs::Format`).

## Summary: accurate, converted, not representable

| | VEGAS (script) | OTIO | CMX 3600 EDL | VEGAS EDL text | FCP7 XML | FCPXML 1.10 |
|---|---|---|---|---|---|---|
| Project size, frame rate, sample rate | accurate | accurate | rate only (DF/NDF) | rate from the open project | accurate | accurate |
| Tracks (any number), names, mute, lock | accurate | accurate | V + A-A4, no names | accurate, no names | accurate (stereo as two linked tracks) | accurate (spine + lanes) |
| Event timing, in point, speed, reverse | accurate | accurate | accurate (speed to 0.1 fps) | accurate | accurate | accurate (frame-aligned) |
| Transitions | mapped by plug-in; others crossfade | dissolve (Milan kind in metadata) | dissolve | converted to an overlap (crossfade) | Cross Dissolve | Cross Dissolve |
| Fades | accurate (curves too) | Milan metadata only | not representable | accurate | converted: dissolves from/to black | converted: opacity keyframes / fadeIn, fadeOut |
| Opacity, gain | accurate (envelopes too) | metadata (static and envelopes) | not representable | static | static (Opacity, Audio Levels) | static (adjust-blend, adjust-volume) |
| Event / track FX, Pan/Crop, Track Motion, envelopes | mapped or placeholders | metadata only (FX named as `Effect.1`) | not representable (listed) | not representable (listed) | not representable (listed; others' filters listed on import) | not representable (listed) |
| Takes | accurate | accurate (media references) | active take only | active take only | active take only | active take only |
| Groups | accurate | metadata | not representable | not representable | `<link>` | video+audio of one clip |
| Markers, regions | accurate | accurate | not representable | not representable | accurate | accurate (on the covering element) |
| Generated media, adjustment events | accurate | missing reference | left out | left out | offline clip / left out | asset without media / left out |

## OpenTimelineIO (R28): File > Import / Export > OpenTimelineIO...

`src/interchange/otio.rs`, hand-written serde structs for `Timeline.1`, `Stack.1`, `Track.1`, `Clip.2` (`Clip.1`
read too), `Gap.1`, `Transition.1`, `Marker.2`, `ExternalReference.1`, `MissingReference.1`, `RationalTime.1`,
`TimeRange.1`, `LinearTimeWarp.1`, `FreezeFrame.1` and `Effect.1`; other schemas are skipped with a warning.

**Milan to Milan is lossless.** Every object goes out twice: as the OTIO object other tools read, and as the Milan
struct itself under `metadata["milan"]` (the project without its tracks on the Timeline, each Track without its
events, each event, each transition). Reading a file with that metadata rebuilds the project exactly (test
`lossless_round_trip`: transitions, takes, markers, regions, envelopes, FX, a speed change, generated media,
groups come back as the same JSON).

What other tools see in a Milan export:

| In Milan Pro | In the OTIO file |
|---|---|
| Project frame rate | Every `RationalTime` rate (29.97 as 30000/1001) |
| Video tracks (top first), audio tracks | `Track.1` of kind Video (bottom first, as OTIO stacks) and Audio |
| Events, their media, in point, length, mute | `Clip.2` with an `ExternalReference.1` (`file:///` URL, `available_range` from the file's start timecode) and `source_range`; `enabled` |
| Takes | Extra `media_references` (`TAKE_1`, ...) on the clip |
| An overlap (transition or automatic crossfade) | An `SMPTE_Dissolve` `Transition.1` centred on the overlap: `in_offset` and `out_offset` are its halves, the clips' ranges meet at its middle |
| Playback rate, reversed events | `LinearTimeWarp.1` (`time_scalar` negative for reverse) |
| A Velocity envelope that is all 0 % | `FreezeFrame.1` |
| Event FX | `Effect.1` entries named after the plug-in (other tools do not play them) |
| Markers, regions | `Marker.2` on the stack (regions with a duration) |
| Generated media, adjustment events | `MissingReference.1` |
| Fades, Pan/Crop, envelopes, track FX, Track Motion, compositing modes, Velocity ramps | Milan metadata only |

Reading a file from another tool: clips at their running position (gaps advance it), a transition extends the
outgoing clip by its `out_offset` and starts the incoming one `in_offset` earlier (using media before its in point)
and becomes a Dissolve on it; a transition against a gap is a fade; time warps become the event's rate, freeze frames a
0 % Velocity point; extra media references become takes; clip markers become timeline markers; a stack nested in a
track is flattened (its first track in line, trimmed to the stack's range; the others become tracks of their own).
Effects other than time warps are listed and dropped. The hand-written fixtures `tests/fixtures/simple_cut.otio` and
`nested_example.otio` cover this (the OTIO repository's samples were not downloaded).

## EDL (R28): File > Import / Export > EDL Text File...

`src/interchange/edl.rs`, two kinds told apart by the file (`.edl` writes CMX 3600, `.txt` writes VEGAS's table;
import reads either by content):

**CMX 3600** (what Resolve, Premiere and Avid read): `TITLE:`, `FCM: DROP FRAME` / `NON-DROP FRAME` (drop frame
at 29.97 / 59.94 unless the project ruler is SMPTE Non-Drop), one video track (`V`) and up to four audio tracks
(`A`, `A2`, `A3`, `A4`; more are listed and left out), cuts, dissolves as the from-source line then the to-source
line with `D` and the length in frames (every Milan transition goes out as a dissolve; the EDL's wipes `W001`...
come in as dissolves), `M2` speed lines (frames per second, negative for reverse, one decimal), reel names from the
file names (eight capitals and digits, unique), `* FROM CLIP NAME:` / `* TO CLIP NAME:` with the file name and
`* SOURCE FILE:` with the full path, `* COMMENT:` with the event name. Source timecodes include the file's start
timecode; the reader takes it off again once the probe knows it (and strips whole hours from files that have none).
Record timecodes starting on an hour are brought back to zero. `BL` (black) dissolves are fades. Golden files at 25
and 29.97 DF: `tests/fixtures/golden_25.edl`, `golden_2997df.edl`. Lost: fades (a note per event), markers, levels,
FX, envelopes, Pan/Crop, takes, generated media.

**VEGAS EDL text** (`"ID";"Track";"StartTime";...`, VEGAS's own File > Import/Export > EDL Text File): every track,
times in milliseconds, play rate (negative for reverse), fades, levels (opacity / gain), locked; no transitions
(an overlap is VEGAS's automatic crossfade, as in Milan).

## Final Cut Pro 7 XML (R29): File > Import / Export > Final Cut Pro 7/DaVinci Resolve (*.xml)... and the Premiere rows

`src/interchange/xmeml.rs` (`xmeml` version 5, read with quick-xml 0.41 through `src/interchange/xml.rs`). This is
the sequence XML Premiere Pro (File > Import, File > Export > Final Cut Pro XML) and DaVinci Resolve (File > Import
Timeline / Export > Timeline, "FCP 7 XML") read and write; the Premiere/After Effects rows of VEGAS's menu point at
it because `.prproj` is an undocumented gzip format nothing but Premiere reads.

| In Milan Pro | In the XML |
|---|---|
| Frame rate, size, sample rate | `<rate>` (timebase + `ntsc`), video and audio `<format>` |
| Video tracks | `<track>`s in order |
| Audio tracks | Two `<track>`s each (channel 1 and 2 of every event, `<sourcetrack>`, linked), as Premiere and Resolve write stereo; a pair like that merges back into one stereo event on import |
| Events: file, in point, length, mute | `<clipitem>` with `<file>` (`pathurl`, duration, start timecode) once and `<file id/>` after |
| Overlaps (transitions, automatic crossfades) | `<transitionitem>` Cross Dissolve between the clip items (they overlap it, as FCP7 lays them out); every Milan transition kind goes out as a dissolve |
| Fades | Converted: dissolves from black (`start-black`) and to black (`end-black`); Cross Fade (+3dB) on audio |
| Playback rate, reverse | `Time Remap` filter (`speed` in percent, `reverse`); a variable ramp on import uses its average |
| Opacity, gain | `Opacity` and `Audio Levels` filters (static) |
| Groups | `<link>` on every member |
| Markers, regions | Sequence `<marker>`s (`out` -1 for a marker) |
| Event / track FX, Pan/Crop, Track Motion, envelopes, takes, generated media, compositing modes | Not representable; listed in the export report. Filters in a Premiere / Resolve XML (other than Time Remap, Opacity, Audio Levels) are listed and dropped on import |

Test: `xmeml::tests::round_trip`, and `tests/fixtures/resolve_sample.xml`, hand-written to the DTD in the shape
Resolve exports (stereo pair, timecode 01:00:00:00, Time Remap, Opacity, a filter Milan lacks, an end-black fade,
markers).

## Final Cut Pro X XML, FCPXML 1.10 (R29): File > Import / Export > Final Cut Pro X (*.fcpxml)...

`src/interchange/fcpxml.rs`. Out: `<resources>` (one `<format>`, one `<asset>` per file with its `<media-rep>`,
the Cross Dissolve `<effect>`), a library / event / project / `<sequence>` whose primary storyline is Milan's bottom
video track; the other video tracks are connected storylines (`<spine lane="1">`, `lane="2"`, ...) and the audio
tracks lanes -1, -2, ..., all anchored to the first spine element. Times are rationals on the frame ("3003/30000s").

| In Milan Pro | In the FCPXML |
|---|---|
| Frame rate, size, sample rate | `<format frameDuration width height>`, `audioRate` |
| Events | `<asset-clip ref offset start duration>`; `srcEnable="video"` / `"audio"` picks the stream (a Milan video event and its audio event are two clips) |
| Overlaps | `<transition>` straddling a cut on a frame; the clips abut at it. On import the outgoing clip plays on to the transition's end and the incoming one starts at its start |
| Playback rate, reverse | Linear `<timeMap>` (two time points); a ramp on import uses its average |
| Opacity, video fades | `<adjust-blend amount>` with opacity keyframes for the fades |
| Gain, audio fades | `<adjust-volume amount="-6dB">` with `<fadeIn>` / `<fadeOut>` |
| Mute | `enabled="0"` |
| Markers, regions | `<marker start duration value>` on the spine element that covers them (a region has a duration) |
| Everything else (FX, Pan/Crop, Track Motion, envelopes, takes, groups beyond a clip's own audio, generated media) | Not representable; listed. Titles, generators, compound clips (`ref-clip`, read as offline events), `filter-video` / `filter-audio` effects are listed on import |

Test: `fcpxml::tests::round_trip` (spine, a connected storyline, an audio lane), and `tests/fixtures/fcpx_sample.fcpxml`,
hand-written to the 1.10 DTD in the shape Final Cut Pro exports (a transition, a time map, a muted connected clip, a
music lane, markers, a title, an effect).

## AAF (external route)

There is no maintained Rust AAF library, and the reference AAF SDK is a large C++/COM structured-storage codebase
(`docs/REMAINING.md` 9.6), so Milan Pro neither reads nor writes AAF. The route is OpenTimelineIO's AAF adapter,
which is Python and runs outside Milan Pro:

```
pip install opentimelineio otio-aaf-adapter
otioconvert -i sequence.aaf -o sequence.otio      # AAF from Avid, Premiere, Resolve -> File > Import > OpenTimelineIO
otioconvert -i sequence.otio -o sequence.aaf      # File > Export > OpenTimelineIO -> AAF for Avid
```

What the AAF adapter carries is what the OTIO table above says (clips, cuts, dissolves, markers, linear speed), and
Milan's metadata rides along untouched through `otioconvert` when the file comes straight back. The AAF row in VEGAS
16's Import menu stays greyed with this note.

## VEGAS Pro, both ways (through VEGAS's scripting API)

Below: the two C# scripts that move a project through VEGAS's own API (both ways), and the `.veg` media scan.

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

## Project archive and media from another project (R30)

File > Export > Milan Pro Project Archive... copies the project and every media file it uses into a folder (`Media\`
beside the `.mnle`), saved with relative paths so the folder opens from another drive or machine. "Trim media to
the ranges used" keeps only the stretches the timeline plays plus handles on either side: audio files are cut
losslessly (PCM re-written sample-accurately, compressed audio stream-copied to the packet), video is re-encoded
(H.264 CRF 16 + AAC, as `.mp4`) so the cut is frame-accurate, and every event's in point moves with the cut. Nested
projects and subclip parents are copied whole; offline files are left as they were and listed. File > Import >
Media from Project... lists another `.mnle`'s media pool and adds the checked items to Project Media as they were
probed there.
