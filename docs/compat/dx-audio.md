# DirectX (DMO) audio effects: feasibility (R35 research spike)

**Question.** VEGAS Pro's own audio effects (Track EQ, Track Compressor, Noise Gate, ExpressFX, the Sony/MAGIX `Audio_x64` plug-ins) are DirectX Media Objects (DMOs): COM objects implementing `IMediaObject`, registered under `DMOCATEGORY_AUDIO_EFFECT`. Could Milan Pro host them, and may it?

**Method.** `milan-plugin-host.exe` has a read-only probe (`Req::DmoProbe`, `src/bin/milan-plugin-host/dmo.rs`, run by the ignored test `aplug::tests::dmo_probe_runs_in_the_host`): `DMOEnum(DMOCATEGORY_AUDIO_EFFECT, DMO_ENUMF_INCLUDE_KEYED)` lists the registered audio effect DMOs; the first one is made with `CoCreateInstance(IMediaObject)` in the isolated host, asked for 32-bit float stereo 48 kHz in and out (`SetInputType` / `SetOutputType`, `WAVE_FORMAT_IEEE_FLOAT`), and one 1024-frame block of a -6 dBFS 1 kHz sine goes through `ProcessInput` / `ProcessOutput` with host-provided `IMediaBuffer`s. Nothing is registered, written or shown; VEGAS is never started. `IEnumDMO`, `IMediaObject` and `IMediaBuffer` are written out in the host from `mediaobj.h` (windows-sys 0.59 has `dmo.h`'s functions and constants but not those interfaces).

**Machine (2026-11-26).** Windows 11 Home 10.0.26200, VEGAS Pro 15.0 installed at `C:\Program Files\VEGAS\VEGAS Pro 15.0`.

## Findings

1. **The VEGAS `Audio_x64` plug-ins are not present on this machine.** `C:\Program Files\VEGAS\VEGAS Pro 15.0` has no `Audio_x64` folder (nor `Audio Plug-Ins`), and no DMO registered under `HKCR\DirectShow\MediaObjects` is served from a VEGAS, Sony or MAGIX DLL. The VEGAS 15 install here carries `sfvstwrap.dll` (its VST wrapper) and the OFX video plug-ins (see `ofx-vegas15.md`), but its DirectX audio plug-in set was not installed or was removed. So the VEGAS DMOs could not be tried; the probe tried a Microsoft DMO instead.
2. **The DMO path itself works outside VEGAS.** `DMOEnum` found the DirectSound effect DMOs Windows ships (`dsdmo.dll`: ParamEq, Chorus, Compressor, Distortion, Echo, Flanger, Gargle, I3DL2Reverb, WavesReverb). The first, **ParamEq** (`{120CED89-3BF4-4173-A132-3CB406CF3231}`): `CoCreateInstance(IMediaObject)` OK; 1 input stream, 1 output stream; `SetInputType` and `SetOutputType` (float stereo 48 kHz) OK; `AllocateStreamingResources` OK; `ProcessInput` OK; `ProcessOutput` OK, 1024 frames out, input RMS 0.354, output RMS 0.354 (a flat EQ at its default). **Result: the DMO processes audio outside its host.** A DMO host in Milan Pro is therefore technically feasible: ~600 lines (the three interfaces, `IMediaParams` for parameters, `IPersistStream` for presets), in the isolated host like the other formats.
3. **Licence position.** The first-party VEGAS audio plug-ins are licensed as part of VEGAS Pro, for use with VEGAS Pro (and, historically, other Sony Creative Software hosts): the EULA licenses "the Software" as a whole to run as installed, not its components for use in other programs. Hosting them from Milan Pro would run VEGAS's binaries outside VEGAS, which the licence does not grant, and (as with the OFX bundles, `ofx-vegas15.md`) they may check their host. Microsoft's `dsdmo.dll` effects are part of Windows and may be used by any application through DirectSound/DMO, but they are elementary (a parametric EQ, a compressor, an echo) and Milan Pro's built-in audio FX already cover them.

## Decision

**Milan Pro ships no DX/DMO path.** The format is 25 years old, deprecated by Microsoft (Media Foundation Transforms superseded DMOs; VEGAS itself moved its effects to VST3 in VEGAS Pro 20+), the only interesting plug-ins (VEGAS's) are not licensed for other hosts and were not even present here, and VST3 (R34a/R34b) and CLAP (R35) cover third-party audio effects with current, licensable SDKs. Rows 10.13 (DirectX/DMO) of `docs/REMAINING.md` is closed as "investigated, not pursued"; the probe stays in the host as the record of how it was measured, and runs again with `cargo test --release -- --ignored aplug::tests::dmo_probe_runs_in_the_host`.

## Raw probe output

```
DMO audio effects registered (DMOCATEGORY_AUDIO_EFFECT): 9
  {120CED89-3BF4-4173-A132-3CB406CF3231} ParamEq
  ... (the DirectSound effects of dsdmo.dll)
Trying "ParamEq" outside its host:
  CoCreateInstance(IMediaObject): OK
  streams: 1 in, 1 out
  SetInputType(float stereo 48 kHz): OK; SetOutputType: OK
  AllocateStreamingResources: OK
  ProcessInput: OK; ProcessOutput: OK (1024 frames out, input RMS 0.354, output RMS 0.354)
  Result: the DMO processes audio outside its host.
```
