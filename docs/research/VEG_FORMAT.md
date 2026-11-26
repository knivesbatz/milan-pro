# The `.veg` project format (R27, research only)

**Result: no `.veg` sample was available, the format stays undocumented, and the recommendation is to keep the
script route in both directions (R25 import, R26 export).** Nothing in this note is product code.

## What was looked at

- Every fixed drive of this machine, read-only (`C:\Users`, `C:\ProgramData`, `C:\Program Files`,
  `C:\Program Files (x86)`, other drives whole): **no `.veg`, `.veg.bak` or `.vf` file exists here.** VEGAS Pro 15
  is installed (`C:\Program Files\VEGAS\VEGAS Pro 15.0`) but ships no sample or template project; its `Script Menu`
  folder holds C# and JScript samples only, and `FileIO Plug-Ins` holds media readers, not a project reader. The
  chunk and GUID structure the batch wanted documented therefore could not be observed, and no guess is written down
  as fact. If the user drops a few `.veg` files (saved by VEGAS 13, 15 and 23, with and without an `.sfk`/`.sfvp0`
  pair) into `tests/fixtures/veg/` (never committed: `.veg` is in the no-commit list), the sections below can be
  filled in by hex inspection in a later batch.
- The scripting assembly `ScriptPortal.Vegas.dll` (VEGAS 15, 405 KB), by reflection in a throwaway process that
  loaded the assembly for metadata only (`ReflectionOnlyLoadFrom`; VEGAS itself was never started). It is the public
  surface VEGAS gives scripts, and it says how VEGAS itself wants projects written:
  - `Project.SaveProject(string fileName)`, `Project.BeginWrite(string fileName, ProjectSaveType saveType)` with
    `ProjectSaveType { Project_Default, Project_Reference, Project_AAF, Project_AvidAAF, Project_Archive, Render_Wave,
    Render_Vfw }`, `Project.ArchiveProject(string fileName[, ArchiveProjectFlags])` and `Project.CloneProject`.
    Writing a `.veg` is a VEGAS operation on a live `Project`; the format is not exposed.
  - `Project.ActiveProject`, `Media.CreateInstance`, `Take.CreateInstance`, `TrackEvent.Copy`, `Effect(PlugInNode)`,
    `Envelope(EnvelopeType)`, `Subclip(...)`, `TrackMotion.InsertMotionKeyframe`, `VideoMotionKeyframe(Timecode)`:
    everything a project holds can be *built* through the API, which is what `tools/vegas/Import from Milan Pro.cs`
    does (R26). Both scripts compile against this assembly (`csc` from the .NET Framework 4.8 SDK, read-only
    reference), so the API use is checked without launching VEGAS.
- What the existing media scan (`interchange::scan_veg`, `src/interchange/vegas.rs`) reads, see below.

## What is known without a sample

- A `.veg` is a binary file, not XML and not a RIFF text form; it changes with the VEGAS version (projects saved by
  a newer VEGAS do not open in an older one, and VEGAS itself warns before saving over a project from an older
  version). There is no public specification, no reference implementation and no third-party reader that goes
  beyond media lists; the only supported access is VEGAS's scripting API (above) and VEGAS's own File menu.
- VEGAS stores media paths in the project as UTF-16LE strings. The media scan relies on that alone: it reads the
  file as UTF-16LE at both byte alignments, splits at control characters and the characters Windows forbids in
  paths, and keeps every stretch that starts with a drive letter and `:\` or a UNC `\\` and ends in a media
  extension Milan Pro knows (`media::classify_extension`), without repeats, in file order. Tested on synthetic data
  (`veg_scan_finds_utf16_paths_only`); the paths that exist on disk are added to Project Media, the rest are listed
  in the import report. Relative or substituted paths (a project moved with its media) are not recovered.
- Nothing else is read: no track, event, time or FX data is attempted, because an offset or chunk id guessed from
  one version of one file would misread the next.

## Estimate for a read-only parser

Reverse-engineering the container (chunk ids, versions, GUIDs, string tables) from samples of three VEGAS
generations and then the records inside it (tracks, events, takes, fades, Pan/Crop, envelopes, FX parameter blobs,
which for OFX are the plug-ins' own serialised parameter sets) is weeks of work for a partial result that breaks on
the next VEGAS release, and still could not carry FX settings without each plug-in's private format. A *writer* is
out of the question (the brief forbids fake `.veg` files; a wrong byte would make VEGAS reject or misread the
project).

## Recommendation

Keep the script route: `Export to Milan Pro.cs` (VEGAS to Milan Pro, R25) and `Import from Milan Pro.cs` (Milan Pro
to VEGAS, R26) through VEGAS's own API, with the `.veg` media scan as the no-VEGAS fallback. Revisit this note only if
sample files arrive or a public specification appears. See `docs/INTERCHANGE.md` for what each route carries.
