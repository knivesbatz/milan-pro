# Milan Pro: implementation plan

Status: plan only. Nothing here is built yet; `src/engine/` does not exist (checked 2026-11-11).
Sources: the engine verdict (design A plus grafts from B and C), the three feature designs (editing, video plug-ins, audio/other) and the three code maps. Where those documents disagreed, section 0 says which way this plan goes.

User priorities, in order: editing functionality > VEGAS workflow familiarity > preview performance > compatibility > plugins > media formats > reliability > modern UI > open source.

How to read this file:
- Section 0: decisions that settle conflicts between the source designs.
- Section 1: the engine design.
- Section 2: the batches, in build order. Each has scope, files, design and a test plan.
- Section 3: deferred items and why.

Rules for every batch:
- One agent implements a batch from start to finish.
- The batch must end with `cargo build` and `cargo test` passing and the app working. Nothing may be left half-wired.
- A batch owns only the files it lists. Files marked **(UI-shared)** are ones the UI agent edits. Do not start a batch that touches them until the UI agent has handed over, or coordinate the edit with it.
- Every child process goes through `proc::spawn` (batch 1 onwards). The one exception is the updater's installer launch (batch 18).
- Each new persistent field gets `#[serde(default)]`, and the old-project test at `project.rs:391` (`old_projects_load_without_fades_or_loop`) is extended to cover it.
- Each edit is a pure function in `edit.rs` that returns `bool` for "changed". It gets exactly one History push per user gesture.
- Disabled menu, key and transport items are enabled in the same batch as the feature behind them, not in a separate pass. The list for each batch is under "Enables".
- A known limit is marked in code as `// shortcut: <limit>, <when to upgrade>`.

---

## 0. Decisions that reconcile the source designs

1. **Engine stages S5 are folded into S2 and S3.**
   - Batch 1 lands the whole data model.
   - The plan compiler therefore reads every field from day one: fade curves, clip gain, velocity, reverse, envelopes, Pan/Crop, Track Motion and composite modes.
   - There is no later retrofit pass.
2. **The "audio hang + loop-past-end" quick fix is not shipped separately.** It is part of S3 (batch 3), with the same watchdog, the same releasable device thread and the same `effective_end`.
3. **The interim one-line event-gain change in `audio.rs` is dropped.** The batch-3 mixer reads `gain_db` before the gain handle (batch 7) exists.
4. **One plug-in registry.** `engine/plugins.rs` holds video FX, transitions, generators and audio FX:
   - `PluginKind { VideoFx, Transition, Generator, AudioFx }`
   - one shared `ParamSpec`
   - one `eval(fx, t)`
   - `afx.rs` registers its DSP constructors by plug-in id and does not keep its own `AfxInfo`/`ParamSpec` copy.
5. **One FX window.** `ui/fx_window.rs` serves video Event/Track/Output FX, audio Event/Track/Master FX, generator media and transitions.
   - Targets are stable ids: `FxTarget { Event(clip_id), Track(track_id), VideoOutput, AudioMaster, Media(media_id), Transition(clip_id) }`.
   - The window chooses the registry kind from the target, plus the track kind for Event and Track targets.
   - There is no separate `ui/dialogs/audio_fx.rs`.
6. **Generated media is `MediaItem.generator: Option<Fx>`**, with plug-in ids such as `milan.gen.solid` and `milan.gen.text`.
   - The engine design's separate `Generator` enum is dropped. Generators reuse the registry, the parameter grid, presets and keyframes.
   - `Src::Gen` in the plan carries an `FxEval`.
7. **Spatial FX parameters are relative to frame height** (blur 0.01 means 1% of the height). Plug-ins take no `scale = out_w / project_w` argument.
8. **The `Composite` enum lives in `project.rs`** because it is data, and `composite.rs` uses it. (The engine design had it defined in `composite.rs` and re-exported.)
9. **Video crossfade without a transition** (same-track overlap):
   - The outgoing event is drawn at full alpha through the overlap.
   - The incoming event is drawn on top with its fade-in alpha.
   - The result is a true dissolve, A·(1−p) + B·p. Drawing both events with their own fades would give a dip toward black.
   - **Verify against VEGAS.**
10. **Prefs fields arrive with the batch that uses them.** Batch 1 changes only `project.rs` and `MediaItem`. `ProjectSettings` gets no new fields, because `ui/dialogs/welcome.rs:154` builds it as a struct literal.
11. **Keys inside an event are event-relative, and stay put on the timeline when the event is trimmed or split.**
    - This covers FX parameter keys, Pan/Crop keys and event envelopes.
    - One helper, `edit::shift_clip_keys`, does this. `trim_left` and `split_clip` call it.
12. **A transition is stored on the incoming event** (`Clip.transition`). A transition on a fade-out with nothing after it is not supported (shortcut).
13. **The render graph goes through a file as an interim fix** (`-/filter_complex <file>`) in batch 1. Batch 4 deletes it together with `build_args`. It is about ten lines and removes a hard failure on timelines with 100 or more events until then.
14. **`Clip` derives `PartialEq`** from batch 1 on. The Event Properties dialog needs it, and every new type derives it as well.

---

## 1. Engine design (final)

### 1.1 Facts the design rests on (checked by reading the code)

**Rendering and pixel layout**
- eframe 0.31 with default features, so the UI renders through glow. All compositing is on the CPU.
- In epaint 0.31.1, `ColorImage { size: [usize; 2], pixels: Vec<Color32> }`. A composited `Vec<Color32>` moves into it with no copy.

**Bundled tools and crates**
- Bundled ffmpeg is `9.0.2-full_build-www.gyan.dev`, a shared build (avcodec-63).
- Already in Cargo.lock, so using them downloads nothing:
  - cpal 0.15.3 (also re-exported by rodio 0.20.1 as `rodio::cpal`)
  - ab_glyph 0.2.32
  - windows-sys 0.59.0 and 0.61.2
  - bytemuck 1.25.2
  - owned_ttf_parser
  - libloading 0.8.9
- rayon is **not** in the lock. The design uses `std::thread::scope` instead.

**Struct literals that break when fields are added**
- `MediaItem { .. }` literals: `audio.rs:434`, `decode.rs:356`, `edit.rs:300`, `media.rs:22`, `render.rs:288`.
- `ProjectSettings` literal: `ui/dialogs/welcome.rs:154`. Therefore no new fields go on `ProjectSettings`.
- `Track` and `Clip` are built only through `Track::new` and `Clip::new`.

**What the UI depends on in the engine**
- `crate::decode::{Preview, Quality, Size}` with these fields and methods: `texture`, `image`, `error`, `quality`, `size`, `display_w`, `width()`, `busy()`, `show()`, `new()`.
- `audio::Master` (`take_peaks`, `set_gain`).
- The render dialog at `ui/dialogs/render.rs:353-354` calls `build_args` and then `Job::start(args, content_end, out, ctx)`.
- The engine therefore sits behind `decode::Preview` and `audio::Player`/`audio::Master` and keeps their public surfaces.

**Current bugs this design removes**
- `frame_source` returns only one layer (`decode.rs:74`).
- Every preview frame spawns a new ffmpeg (`decode.rs:103`).
- Every Play leaks a thread: the device thread parks and the mix thread blocks on `sync_channel(2)`.
- The audio session ends at `content_end`, so a loop region past the end hangs.
- The render graph is one command-line argument.
- Video fades are linear, while audio fades use an S-curve.
- The pad bars are opaque.
- The probe uses `-v quiet` and `r_frame_rate`.

### 1.2 Principles

1. **One plan for every consumer.**
   - `engine::plan` turns a `Project` into frame jobs and audio plans.
   - Preview, playback and render all use the same compositor and the same mixer.
   - No other code interprets the timeline.
2. **ffmpeg only at the two ends.**
   - Long-lived decoder child processes stream raw RGBA video or f32 PCM over pipes.
   - One encoder child takes finished frames on stdin.
   - There are no libav bindings, so a crashing decoder cannot crash Milan.
3. **Pixels are `Color32`.**
   - Premultiplied, gamma-space, 8-bit RGBA, which is egui's own layout.
   - Row stripes run under `std::thread::scope`.
   - One texture upload per frame.
4. **Audio is the master clock.** It is made robust by:
   - a stall watchdog
   - block timestamps, so loop wraps are seamless
   - live plan swaps, so edits are heard without a restart.
5. **No UI-file changes until a stage needs one.** Each needed UI edit is named in its batch.

### 1.3 Modules

**`src/proc.rs` (batch 1)**
- `ffmpeg() -> Command` and `ffprobe() -> Command`: set `-nostdin`, null stdin and `creation_flags(0x08000000)`.
- `spawn(&mut Command) -> io::Result<Child>`:
  - spawns the child, then calls `AssignProcessToJobObject(job(), handle)`;
  - if the assignment fails, it logs and still returns the child.
- `static JOB: OnceLock<HANDLE>`:
  - created with `CreateJobObjectW` plus `SetInformationJobObject(JobObjectExtendedLimitInformation, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE)`;
  - the handle is never closed, so when Milan exits, crashes or is killed, Windows kills every child.
- `tool_error(e, tool) -> String` returns `"<tool> not found next to Milan or on PATH"`.
- Cargo: `[target.'cfg(windows)'.dependencies] windows-sys = { version = "0.59", features = ["Win32_Foundation", "Win32_System_JobObjects", "Win32_System_Threading"] }`. This version is already locked.

**`src/project.rs`: data model (batch 1, see 1.7)**

**`src/engine/mod.rs`**
- `pub struct VideoEngine`, with fields:
  - `tx: Sender<VCmd>`
  - `latest: Arc<Mutex<Option<(i64, Arc<ColorImage>)>>>`
  - `play_rx: Option<Receiver<(i64, Arc<ColorImage>)>>`
  - `clock_frame: Arc<AtomicI64>`
  - `pub stats: Arc<Stats>`
  - `sent_hash: u64`
  - `want: Option<(i64, u32, Quality)>`
- `enum VCmd`:
  - `Plan(Arc<VideoPlan>)`
  - `Show { n, w, h, q }`
  - `Play { from_n, w, h, q, loop_n: Option<(i64, i64)>, tx: SyncSender<(i64, Arc<ColorImage>)> }`
  - `Stop`
- `content_hash(&Project) -> u64`:
  - `serde_json::to_writer` into `struct HashWriter(DefaultHasher): io::Write`, over `(&settings, &timeline, &media_pool)`;
  - cursor, zoom and scroll are `serde(skip)`, so they never trigger a replan;
  - shortcut: hashes the whole project every UI frame (about 1 ms on big projects); switch to a generation counter in `mark_modified` if a profile shows it.
- `Stats { fps_x100: AtomicU32, dropped: AtomicU64, live_decoders: AtomicU32 }`.

**`src/engine/keyframes.rs` (batch 1, pure)**
- `Curve::shape(x)`: `Smooth` reuses `project::smooth_fade`; `Hold` is a step.
- `Keyframes<T: Lerp + Default>::at(t)`: an empty list gives the default; values hold before the first key and after the last.
- `trait Lerp` for `f32`, `[f32; N]`, `PanCrop` and `Motion`.
- `Envelope::at(t)`.
- `ramp(env, t0, t1) -> (f32, f32)`.
- `db_to_gain`.
- Edit operations (batch 10 adds the UI on top):
  - `set_key(kf, t, v, curve)` replaces a key within 0.5/fps, otherwise inserts sorted;
  - `remove_key`;
  - `move_key -> usize` (keeps the list sorted);
  - `times`;
  - `shift`.

**`src/engine/plan.rs` (pure, fully unit-tested)**

`compile(&Project) -> (VideoPlan, AudioPlan)`.

`VideoPlan`:
- `{ w, h, fps: f64, fps_str, tracks: Vec<VTrack>, out_fx: Vec<Fx>, end }`.
- Tracks run bottom to top, which is `shown_video_tracks().rev()`.
- `VTrack { composite, level: f32, level_env, motion: Keyframes<Motion>, fx, events: Vec<VEvent> }`.
- `events` is a stable sort by position; on ties the later Vec entry is on top.
- `VEvent { clip_id: Arc<str>, src: Src, pos, dur, in_point, rate, reverse, fade_in, fade_out, fade_in_curve, fade_out_curve, opacity_env, pan_crop, fx, transition, muted }`.
- `enum Src { Media(Arc<MediaRef>), Still(Arc<MediaRef>), Gen(Fx) }`.
- `MediaRef { id, path, w, h, rotation, has_alpha, matrix: Option<&'static str>, codec, interlaced, proxy: Option<PathBuf> }`. `proxy` is filled from batch 12.

`frame_plan(&VideoPlan, n) -> FramePlan`:
- Everything is resolved to plain numbers at t = n / fps.
- `FramePlan { layers: Vec<TrackPlan>, out_fx: Vec<FxEval> }`.
- `TrackPlan { mode, opacity, motion: Affine, fx: Vec<FxEval>, items: Vec<Item> }`.
- `enum Item { Event(EventPlan), Mix { a, b, kind: FxEval, p } }`.
- `EventPlan { src: SrcAt, place: Affine /* PanCrop, then fit to the project frame */, fx: Vec<FxEval>, alpha }`.
- `SrcAt` is one of `Media { m, sf }`, `Still(m)` or `Gen(FxEval, t_local)`.
- `sf = round(src_time(event, t) * fps)`, on the project-fps grid.
- `src_time` is shared with `edit.rs`:
  - forward: `in + (t − pos) × rate`
  - reverse: `in + (pos + dur − t) × rate`
- `alpha` = fade gain (with its curve) × opacity envelope.
- Overlap handling on one track:
  - incoming event has a transition: `Item::Mix`;
  - lone fade-in with a transition: `Mix` with a transparent A;
  - otherwise: the decision 0.9 dissolve rule.
- Muted events (`Clip.muted`) are skipped.
- `FramePlan: Hash`, with f32 hashed through `to_bits`.

Other plan functions:
- `upcoming(&VideoPlan, t0, t1) -> Vec<(Arc<MediaRef>, i64)>` pre-warms decoders.
- `AudioPlan { tracks: Vec<ATrack>, master: MasterBus, bypass_fx: bool, end }`.
- `ATrack { id, gain: [f32; 2], vol_env, pan_env, mute_env, fx, events }`.
- `AEvent { clip_id, path, pos, dur, in_point, rate, reverse, gain, fade_in, fade_out, curves, vol_env, fx }`.
- `effective_end(p, looping) = max(content_end, loop end)` when looping. This fixes the loop-past-end hang.

**`src/engine/vdecode.rs`**

`DecSpec { path, sf, fps_str, fps, w, h, scaler, hw: bool, matrix, interlaced }`.

`args(&DecSpec) -> Vec<String>` is pure and tested. The argv is:

```
-v error -threads 2 [-hwaccel auto] -ss {sf/fps:.6} -i PATH -map 0:v:0 -an -sn -dn
-vf "[yadif=deint=interlaced,]fps={fps_str}:round=near,scale={w}:{h}:flags={scaler}:in_color_matrix={matrix|auto}:out_range=full,format=rgba"
-f rawvideo -
```

- `-hwaccel` is used only for HEVC or AV1 sources, or widths of 2560 or more, and only in streams, never for scrub. Measured cost: 0.3-0.7 s per spawn.
- Untagged sources with height ≥ 720 get `bt709`.
- ffmpeg autorotates, and the probe has already swapped w and h.

`VideoStream { media, w, h, next_sf, rx: Receiver<Img>, child, err, used: Instant }`:
- One reader thread per stream does `read_exact(w*h*4)` into `Vec<Color32>`.
  - It uses a bytemuck cast when ecolor's bytemuck feature is on, otherwise a byte loop (about 1 ms per frame).
  - It premultiplies only when `has_alpha`.
  - It sends on `sync_channel(4)`.
- A stderr drain thread keeps the last line as the error text.

`DecoderPool { streams, hw_failed: HashSet<Arc<str>> }`:
- `frame(m, sf, w, h, q, deadline: Option<Instant>) -> Pull { Exact | Held | Err }`.
- Reuse rule: a stream with the same (media, w, h) and `sf ∈ [next_sf, next_sf + 2·fps]` is read forward, dropping frames, and the drops are counted. Otherwise a new stream is spawned.
- At most 8 live streams, evicted LRU.
- A stream idle for 5 s is killed. `Drop` kills the child.
- If a hwaccel stream produces no frame, it is retried without hwaccel and the media goes into `hw_failed`.
- Stills use the one-frame command and are pinned in the cache.
- Reversed events: sf decreases, so the pool respawns per frame, about 5 fps.
  - Built in R20: a picture read backwards (reversed events, J shuttle, dragging left) comes from 1 s chunks decoded forward (`-frames:v`) that end at the frame asked for, with two chunks before it decoding ahead; all chunks stay under 256 MB. Paused seeks and chunks of source media decode with up to 4 threads. See `REMAINING.md` R20.

**`src/engine/cache.rs`**
- `ByteLru<K, V>`, about 40 lines, no crate.
- `SourceCache: (Arc<str>, i64, u32, bool proxy) -> Arc<Img>`, 192 MB.
- `CompositeCache: (u64 plan_hash, u32 w) -> Arc<ColorImage>`, 256 MB. A light Dynamic RAM Preview; it needs no invalidation on edits.

**`src/engine/composite.rs`**
- `Img { w, h, px: Vec<Color32> }`.
- `Affine { a, b, c, d, tx, ty }` with `then`, `inverse`, and `fit(sw, sh, dw, dh)` (keeps aspect; outside the image is transparent).

`compose(&FramePlan, &mut dyn Sources, w, h) -> Img`:
1. Start from opaque black.
2. For each track, bottom to top:
   - **Fast path:** the track has no FX, uses SourceAlpha, and its events do not overlap at Level < 1. Draw each event straight into the accumulator with `place × motion` folded into one affine, so each pixel is sampled once.
   - **Otherwise:** draw into a transparent scratch layer, then track FX, then motion, then `blend(mode, opacity)`.
3. Apply the Video Output FX.

FX order: event FX on a clone of the source (`Arc::make_mut`) before `draw`, then track FX on the scratch layer, then output FX last.

`draw(dst, src, inv, opacity, mode, bilinear)`:
- fast path for an integer translate at scale 1;
- otherwise 16.16 fixed-point inverse mapping;
- `mul255(x, y) = (t + (t >> 8)) >> 8`, with `t = x·y + 128`;
- K = clamp(available_parallelism / 2, 1, 6) row stripes.

`trait Sources { fn get(&mut self, s: &SrcAt, w, h) -> Result<Arc<Img>, String> }`:
- the real implementation is the pool plus the caches plus `generate::render`;
- tests use a fake.

**`src/engine/worker.rs`** (thread "milan-video")
- Owns the plan, the pool and both caches.
- `Show`:
  1. drain to the newest command;
  2. `fp = frame_plan(n)`;
  3. check the composite cache, otherwise compose;
  4. turn the result into a `ColorImage` (a move, or one clone if it is also cached);
  5. store it in `latest` and call `request_repaint`.
- `Play`:
  - Loop with `n = max(next, clock_frame + 1)`, wrapping at `loop_n.1`.
  - Pull layers with a deadline of now + 25 ms. A late layer holds its last frame.
  - Send on `sync_channel(4)`, which blocks once it is 4 frames ahead.
  - Every 0.5 s: call `upcoming(t, t + 1.5 s)` and update `Stats`.
- `Plan` swaps the plan. Live decoders survive if the sf they are asked for is still reachable.

**`src/engine/mix.rs` (audio)**

`AudioStream { clip, next_frame, rx: Receiver<Vec<f32>> /* sync_channel(8), 0.25 s chunks */, buf, pos, child }`:
- Spawned with `-ss SRC -i PATH -vn -map 0:a:0 -ac 2 -ar 48000 [-af atempo chain | areverse] -f f32le -`.
- `atempo` is chained in [0.5, 2] steps, which keeps pitch (the VEGAS default).
- Reverse uses `-t dur×rate … -af areverse`.
  - shortcut: areverse buffers the whole event (about 23 MB per stereo minute); render a reversed temp file past roughly 10 min.

`Mixer { plan: Arc<AudioPlan>, streams, ev_fx: HashMap<clip_id, Chain>, tr_fx: HashMap<track_id, Chain>, master_fx: Chain }`.

`block(t0, out)`, with `BLOCK = 1024` frames:
1. Per event: PCM, then the event FX chain at `t0 − pos`, then × gain × fade(curve) × volume envelope, into the track buffer.
2. Per track:
   - run the track FX chain, even when no event is under the block, so tails continue;
   - then volume, pan and mute envelopes (ramped across the block) × `track_gain`;
   - add into the master.
3. Master FX, then (render only) master gain.

Stream lifecycle:
- A stream opens 1 s before its event enters the window.
- It reopens when the needed frame is not `next_frame`.
- Chains `sync()` when the plan Arc changes.
- Chains reset on a session start, but not on a loop wrap.

**`src/engine/render.rs`** (batch 4): see 1.6.

**Plug-in modules** (batches 14 and 16): `plugins.rs`, `vfx.rs`, `transition.rs`, `generate.rs`, `fonts.rs`, `lut.rs`, `thumbs.rs`, `afx.rs`. Section 2 has the designs.

**Existing modules after the engine lands**
- `decode.rs` keeps:
  - `Quality`, `Size`, `preview_width`;
  - `decode_size` (with rotated dimensions);
  - the single-media `Preview::show` path for the Trimmer;
  - `Thumbs` (generator media goes to `generate::render`).
  - `frame_source` is deleted in batch 2.
- `audio.rs` keeps `Master`, `Feed`, `Player` (new internals) and `Waveforms`.
- `render.rs` keeps:
  - `PRESETS`, plus a new `muxer: &'static str` field;
  - `content_end`;
  - `Job`.
  - `build_args` is deleted in batch 4.

### 1.4 Threads and channels (steady state)

| Thread | Count | Inputs | Outputs |
|---|---|---|---|
| UI (`MeridianApp::update`) | 1 | — | Hashes the project each frame. When the hash changes, sends `Plan` and swaps the audio plan. Stores `clock_frame`, takes frames and makes one `texture.set` per frame. |
| milan-video | 1 | mpsc `VCmd` | the `latest` slot, plus a play `sync_channel(4)` |
| compositor stripes | K scoped threads per frame | — | — |
| video decoder | at most 8 (child + reader + stderr drain each) | — | `sync_channel(4)` of `Img` |
| Trimmer | 1 | — | the old single-frame path, unchanged |
| milan-mix | 1 per play session | `Arc<Mutex<Arc<AudioPlan>>>`, read once per block | `(t0, Vec<f32>)` on `sync_channel(6)` (about 128 ms) |
| audio decoder | child + reader per audible event in the window | — | `sync_channel(8)` |
| rodio device | 1, rebuilt on reopen | holds the `OutputStream` and blocks on `release_rx.recv()` | dropping `release_tx` frees it |
| render | job + writer `sync_channel(3)` + stderr drain + its own pool | — | — |
| thumbs (2) and waveform (1) workers | — | check `Arc<AtomicBool> paused`, set while playing | — |

### 1.5 Preview and playback path

1. **Construction.** `decode::Preview` gains `engine: Option<VideoEngine>` and `visible: bool`.
   - `Preview::new(ctx)` keeps the single-source path, which the Trimmer uses.
   - `Preview::timeline(ctx)` creates the engine. `app.rs:132` switches to it.
   - `update(ctx, project)` routes to the engine.
   - `ui/preview.rs` does not change.
2. **Each UI frame:**
   - `h = content_hash`. If it changed, send `Plan(Arc::new(compile(p).0))`.
   - When paused, if `(n = floor(cursor·fps + 1e-6), w, h, quality)` changed and `visible` is set, send `Show`.
   - Take `latest` if its `n` is still wanted. Call `texture.set(ImageData::Color(arc), LINEAR)` on the one `"preview_frame"` texture and set `image`.
   - On an error, keep the last texture and set `error` (no black flash).
   - An empty timeline composes to black.
3. **Quality levels.**

   | Level | Width | Decode scaler | Compose sampling |
   |---|---|---|---|
   | Draft | half | fast_bilinear | nearest |
   | Preview | full | fast_bilinear | bilinear |
   | Good | full | bilinear | bilinear |
   | Best | full | lanczos | bilinear |

   - Geometry is identical at every level.
   - Decode width per layer = min(source width, preview width × Pan/Crop zoom).
   - From batch 12, Draft and Preview use the proxy when one exists.
4. **Playing.**
   - `preview.play(from_n, loop_n)` sends `Play`.
   - Each UI frame, `clock_frame.store(floor(cursor·fps))`. Pop frames while `front.n <= clock_frame` and show the newest.
   - Late frames are dropped and early ones held, so A/V stays locked.
   - The second pass over a loop that fits in 256 MB plays from `CompositeCache`.
   - `stop()` sends `Stop`.
5. **`busy()`** = a frame is wanted but not yet shown. This keeps the screenshot harness working.

### 1.6 Audio path (rodio kept)

1. **Starting a session.** `Player::start(ctx, project, from, end, loop_)`, where `end = effective_end`, spawns milan-mix:
   - `loop { plan = slot.lock().clone(); mixer.block(t, buf); send_or_quit((t, buf)) }`.
   - `send_or_quit` is a `try_send` loop that sleeps 10 ms between tries and returns false once `quit` is set or the receiver is gone. It never blocks forever.
   - Looping: when `t >= b`, set `t = a`; streams for `a` were pre-warmed 1 s earlier.
   - Otherwise stop at `end`. Between `content_end` and the loop end, the mixer outputs silence.
2. **Clock.**
   - `Feed` stores `block_t0` and `block_played0` in atomics on each block switch.
   - `position() = block_t0 + (played − block_played0) / 2 / RATE`, so the clock follows loop wraps exactly.
   - On underrun, `Feed` outputs silence and the clock does not advance.
3. **Live edits.**
   - `Player::set_plan(Arc<AudioPlan>)` swaps the slot when the hash changes; the edit is heard in about 150 ms or less.
   - Delete app.rs's `end != play_end` restart and its device-pending restart.
   - A session restarts only on play/stop, on a seek (`cursor != clock_pos`), or on a loop toggle.
4. **Watchdog.**
   - `Feed.calls: AtomicU64` is incremented on every `next()`, including underruns, so a slow first decode is not mistaken for a dead device.
   - The pure `stalled(watch, calls, now)` returns true after 400 ms with no change.
   - While stalled, `position()` returns `None`, so the existing dt branch keeps the playhead moving.
   - `app.rs` then calls `stop()` and `reopen()`. `reopen()` drops `release_tx`, which frees the old stream, and spawns a new device thread. It is rate-limited to once every 2 s.
   - When the device is back, the session restarts at the cursor.
5. **Unchanged.**
   - Master gain and peaks stay in `Feed`; `ui/master_bus.rs` is unchanged.
   - Shuttle and Rate ≠ 1 stay silent, as today (R4b has since made them audible: `docs/REMAINING.md`).
6. **Known limit.** rodio gives no latency figure, so the playhead runs about 10-20 ms ahead of what is heard. The fix (cpal timestamps) is deferred.

### 1.7 Render path

**Batch 1 interim**
- `Job::run` writes the graph to `%TEMP%\milan-render-<pid>-<n>.txt` and passes `-/filter_complex <file>`.
- On Cancelled or Failed it deletes `out` (the dialog already confirmed any overwrite at `:343`) and the graph file.
- The child goes through `proc::spawn`.

**Batch 4, engine render**

`engine::render::start(project, preset, out, range, master_gain, ctx) -> Job` reuses `Job`, `Status`, `fraction`, `eta`, `cancel` and `Drop`. It works on a snapshot of the project.

Audio pass (0-5% of progress):
- A `Mixer` at the project sample rate, with master FX and master gain on.
- Writes f32le stereo to `%TEMP%\milan-render-<pid>-<n>.f32`.
- Samples are clamped to [−1, 1] unless a master limiter is present.

Video pass, encoder command:

```
ffmpeg -hide_banner -v error -f rawvideo -pix_fmt rgba -s WxH -framerate <fps_str> -i -
  -f f32le -ar SR -ac 2 -i TMP -map 0:v -map 1:a
  -vf scale=out_color_matrix=bt709:out_range=tv -colorspace bt709 -color_primaries bt709 -color_trc bt709
  <preset video> <preset audio> -ar SR -f <preset.muxer> OUT.partial
```

- Audio-only presets drop the video input.
- For n in `round(a·fps) .. round(b·fps)`: `compose(frame_plan(n))` at full size and Best quality, through a private pool.
  - No deadlines, so no frames are dropped.
  - Decoders read strictly forward.
  - No composite cache, and never proxies.
  - The writer thread (`sync_channel(3)`) calls `stdin.write_all`.
- Progress = frames done / total. Cancel is checked every frame.

Finishing:
- **Done:** remove `OUT` if it exists, rename `.partial` to `OUT`, delete the temp file.
- **Cancel or Fail:** kill the encoder, delete `.partial` and the temp file.

`range` comes from the "Render loop region only" checkbox, which batch 4 enables.

### 1.8 Data model (all lands in batch 1)

All new fields carry serde defaults. Old `.mnle` files load and behave as today.

**New types in `project.rs`.** All derive `Clone, Debug, PartialEq, Serialize, Deserialize`.

| Type | Definition | Notes |
|---|---|---|
| `Curve` | `enum { Linear, Fast, Slow, #[default] Smooth, Sharp, Hold }` | |
| `Keyframe<T>` (not `Key<T>`: that clashes with `egui::Key` in UI files that glob-import both) | `{ t: f64, v: T, #[serde(default = "Curve::linear")] curve: Curve }` | the curve shapes the segment after the key |
| `Keyframes<T>` | `#[serde(default)] { keys: Vec<Keyframe<T>> }` | empty = default value |
| `EnvKind` | `enum { Volume /*dB, floor −120*/, Pan /*−1..1*/, Mute /*1 on, 0 muted, Hold*/, CompositeLevel /*0..1*/, Opacity, Velocity /*reserved*/, #[serde(other)] Unknown /*a later version's kind; ignored*/ }` | |
| `Envelope` | `{ kind: EnvKind, points: Keyframes<f32> }` | |
| `Fx` | `{ id: String /*uuid; keys DSP state*/, plugin: String, #[serde(default = "yes")] enabled: bool, #[serde(default)] params: BTreeMap<String, Keyframes<f32>>, #[serde(default)] text: BTreeMap<String, String> }` | |
| `Transition` | `{ plugin: String, #[serde(default)] params: BTreeMap<String, f32> }` | |
| `PanCrop` | `{ cx, cy, w, h, rot }` | normalised to the source; default `{0.5, 0.5, 1, 1, 0}`; negative w/h = flip |
| `Motion` | `{ x, y, w, h, rot }` | normalised to the project frame; identity `{0, 0, 1, 1, 0}` |
| `Composite` | `enum { #[default] SourceAlpha, Add, Subtract, Multiply, Screen, Overlay, Darken, Lighten, Difference }` | |
| `Take` | `{ media_id, in_point, name }` | |
| `MasterBus` | `{ gain: [f32; 2], audio_fx: Vec<Fx>, video_fx: Vec<Fx> }` | manual `Default` with gain = [1, 1] |

**New fields on existing structs.** Each is `#[serde(default)]` unless noted.

| Struct | New fields | Notes |
|---|---|---|
| `Clip` | `gain_db: f32`, `fade_in_curve`, `fade_out_curve` (default Smooth, today's curve), `#[serde(default = "one")] rate: f64`, `reverse`, `muted`, `pan_crop: Keyframes<PanCrop>`, `fx: Vec<Fx>`, `transition: Option<Transition>`, `envelopes: Vec<Envelope>`, `group: Option<String>`, `takes: Vec<Take>` | `takes` holds the inactive takes. `Clip::new` sets them all. Derives `PartialEq`. |
| `Track` | `composite`, `motion: Keyframes<Motion>`, `fx`, `envelopes` | `track.volume` stays the static Level/Volume, multiplied with the envelope |
| `Timeline` | `master: MasterBus` | |
| `MediaItem` | `has_alpha`, `rotation: i32`, `color_matrix: Option<String>`, `interlaced`, `generator: Option<Fx>` | Derives `Default` (`MediaType: Default = Unknown`). The five literals gain `..Default::default()`. Generated media is `Image`, duration 86400, empty path. Thumbs, Trimmer, `media_limit` and waveforms check `generator` first. |

**Probe changes** (`media.rs`, batch 1):
- `-v error` instead of `-v quiet`.
- Classify by stream type first, falling back to the extension.
- Skip `disposition.attached_pic` streams.
- `avg_frame_rate`, falling back to `r_frame_rate`.
- Rotation from `side_data_list`; swap w and h at 90 and 270.
- An alpha `pix_fmt` sets `has_alpha`.
- `color_space` → `color_matrix`.
- `field_order` other than `progressive` sets `interlaced`.

**Undo:** unchanged. Snapshot history stores all of this as plain data.

### 1.9 Performance budget

Target: a 6-core machine, a 1080p29.97 project, Preview (Auto) at 960×540.

**Per frame with 4 layers**
- decode: out of process, about 3-5 ms CPU per layer, in parallel
- pipe read: about 0.4 ms per layer
- compose: ≤ 4 ms on 4 stripes (1 ms on the fast path)
- texture upload: ≤ 2 ms
- engine time on the UI thread: < 3 ms
- **Gate:** 4 × 1080p H.264 at 29.97 fps with < 1% dropped frames.

**Scrub**
- cache hit: < 16 ms
- forward step on a live stream: < 15 ms
- respawn: about 200 ms (1 layer, GOP 250)

**Other targets**
- Play start ≤ 250 ms.
- A mixer edit is heard in ≤ 150 ms.
- Memory: source cache 192 MB, composite cache 256 MB, plus at most 8 decoders at about 30-50 MB each outside our heap.
- Render: x264 is the bottleneck; compose is about 6 ms per frame for 3 layers at 1080p. The temp audio is about 1.4 GB per hour.

### 1.10 Engine risks

- **Long-GOP or phone HEVC:** scrub respawns cost 200-800 ms. Proxies (batch 12) are the fix.
- **VFR and open-GOP sources:** `-ss` can land a frame off. The frame-accuracy e2e test is the gate.
- **8-bit gamma-space compositing:** bands under heavy grading. There is no float mode.
- **4K Full/Best with blur:** drops frames on the CPU. `compose()` takes a frame in and gives one out, so a GPU backend can replace it later.
- **Not verified:**
  - C's spike timings
  - `-/filter_complex` on the bundled build
  - pipe throughput on Windows
  - whether ecolor's bytemuck feature is on
  - `CREATE_NO_WINDOW` together with Job Object assignment
  - dropping a rodio `OutputStream` after its device is gone.

---

## 2. Batches (build in this order)

| # | Batch | Size | UI-shared files? |
|---|---|---|---|
| 1 | Engine foundation: process hardening, probe, data model, atomic save | M | no |
| 2 | Video engine: multi-layer compositor, persistent decoders, playback | XL | app.rs only |
| 3 | Audio engine: streaming mixer, live edits, watchdog, loop fix | L | app.rs only |
| 4 | Render through the engine | M | app.rs, dialogs/render.rs (2 lines) |
| 5 | Editing core: save/import fixes, ids, multi-select, tracks, quantize | L | yes |
| 6 | Edit tools, grouping, automatic crossfades, ripple | L | yes |
| 7 | Rate/reverse, slip/slide/roll/stretch, event gain and opacity handle | M | yes |
| 8 | Envelopes, Envelope tool, curve menus, Lock Envelopes | L | yes |
| 9 | Takes and Event Properties | M | yes |
| 10 | Keyframe controller, Event Pan/Crop, Track Motion | L | yes |
| 11 | Dock window layouts | M | yes |
| 12 | Video proxies | M | yes (media panel, status) |
| 13 | VEGAS project import (script plus .veg media scan) | M | yes (menu, modal) |
| 14 | Video plug-in engine: registry, FX, transitions, generators, text, grading maths | XL | no |
| 15 | Video plug-in UI: choosers, FX window, transitions, generators, Color Grading panel | XL | yes |
| 16 | Audio FX: DSP, chains, FX window audio targets, master FX | L | yes |
| 17 | Audio recording | L | yes |
| 18 | Updater polish | S | no |

Why this order:
- Batches 1-4 are the engine. They are mostly outside `src/ui`, so they can start while the UI agent is still working, and every later feature becomes visible and audible on them.
- Batches 5-9 cover editing functionality.
- Batches 10-11 cover VEGAS workflow familiarity.
- Batch 12 is preview performance.
- Batch 13 is compatibility.
- Batches 14-16 are plug-ins.
- Batches 17-18 are the rest.
- Batch 14 is pure engine code and may run in parallel with any of batches 5-13, since it shares no files with them except `engine/mod.rs` (one `pub mod` line each).

---

### Batch 1: Engine foundation (S0 + S1)

**Scope**
- `proc.rs` with the Job Object, used at every spawn site.
- The render graph through a file, with partial-output cleanup.
- The probe fixes.
- The full data model (1.8) and `engine/keyframes.rs`.
- Atomic `Project::save`.
- Waveforms retry a failed decode once.
- No visible change in the app.

**Files owned**
- `Cargo.toml`, `Cargo.lock` (windows-sys features; nothing is downloaded)
- `src/main.rs` (`mod proc; mod engine;`)
- `src/proc.rs` (new)
- `src/engine/mod.rs` (new, stub)
- `src/engine/keyframes.rs` (new)
- `src/project.rs`
- `src/media.rs`
- `src/decode.rs`, `src/audio.rs`, `src/render.rs`, `src/demo.rs`, `src/edit.rs` (spawn sites and MediaItem literals only)

**Design notes**
- **Spawn sites.**
  - Every `Command::new("ffmpeg"/"ffprobe")` becomes `proc::ffmpeg()` / `proc::ffprobe()`.
  - Every `.spawn()` becomes `proc::spawn`.
  - `.output()` calls stay synchronous; a crash orphans them for under a second.
  - Error text uses `tool_error`.
- **Atomic save.** `Project::save` keeps its signature `(&mut self, &PathBuf) -> anyhow::Result<()>`:
  1. serialize to a String;
  2. write `path.with_extension("mnle.tmp")`;
  3. `std::fs::rename` (which replaces an existing file);
  4. only then set `name`, `path` and `modified = false`.
  - On error, remove the tmp file and leave name, path and modified unchanged.
  - The caller-side reporting of errors is batch 5.
- **Waveform retry.** A failed peaks decode is retried once before the empty result is cached.

**Test plan**
- **proc:**
  - `tool_error` text for `NotFound`.
  - `#[ignore]` Job Object e2e:
    1. The test re-runs its own exe with `MILAN_JOB_CHILD=1`.
    2. The helper `proc::spawn`s `ffmpeg -f lavfi -i nullsrc -f null -`, prints the PID and sleeps.
    3. The parent kills the helper, then asserts with `OpenProcess`/`WaitForSingleObject(0)` that the ffmpeg PID has exited.
- **render interim:** an argv test that the graph is a file argument. Each event is still its own `-i` input until batch 4, so with realistic 80-character media paths 250 events fit under Windows' 32,767-character limit and 500 events give a clear "too many events" error (the 1000-character gate moves to batch 4).
- **render e2e (`#[ignore]`):** cancel removes the partial output and the graph file.
- **probe:** feed `parse_ffprobe` canned JSON for:
  - cover art (attached_pic is skipped)
  - rotation 90 (w and h swapped)
  - yuva420p (`has_alpha`)
  - tt field order (`interlaced`)
  - VFR (`avg_frame_rate` is used)
  - `.mts` with a video stream (classified as Video)
- **data model:**
  - Extend `old_projects_load_without_fades_or_loop`: an old file gives rate 1, Smooth curves, no group, no takes, reverse and muted false, an empty fx list, gain 0 dB, master gain [1, 1], composite SourceAlpha.
  - A round-trip test with every new field set.
  - `MediaItem::default()` compiles in all five literal sites.
- **keyframes:**
  - every `Curve` has shape(0) = 0 and shape(1) = 1 and is monotonic;
  - Hold is a step;
  - an empty list gives the default;
  - values hold outside the keys;
  - `ramp` endpoints;
  - `set_key` replaces within half a frame and inserts otherwise;
  - `move_key` keeps the list sorted;
  - `db_to_gain(−6.02) ≈ 0.5`.
- **save:**
  - saving into a missing directory returns Err and leaves name, path and modified unchanged;
  - saving over an existing file leaves no `.mnle.tmp` behind;
  - save then load round-trips.

---

### Batch 2: Video engine (S2 + video half of S5)

**Scope**
- Multi-layer preview with:
  - track Level and the Composite Level envelope
  - fade curves and opacity
  - the crossfade dissolve
  - transparent letterbox bars
  - Pan/Crop and Track Motion affines
  - composite modes
  - velocity and reverse
- Persistent decoder streams, read-ahead, the RAM caches and streaming playback.
- Not in this batch:
  - FX and transitions: `Item::Mix` is planned, and compose falls back to a dissolve until batch 14 registers kinds.
  - Generators: render transparent until batch 14.

**Files owned**
- `src/engine/plan.rs` (video half, plus `src_time` and `effective_end`)
- `src/engine/vdecode.rs`, `src/engine/cache.rs`, `src/engine/composite.rs`, `src/engine/worker.rs`, `src/engine/mod.rs`
- `src/decode.rs`: `Preview::timeline`, `play`, `stop`, `visible`; delete `frame_source`; pause flag on Thumbs
- `src/app.rs` **(UI-shared, small)**:
  - construct with `timeline()`
  - `play`/`stop` on play edges
  - store `clock_frame`
  - set `preview.visible` from the dock
  - pause thumbs and waveforms while playing

**Enables:** nothing in the menus. No UI-file change besides `app.rs`.

**Test plan**
- **plan** (port the `frame_source` cases):
  - track order; hidden, muted and solo tracks
  - a later event wins
  - an overlap with a transition gives p = 0 at its start and 1 at its end
  - `sf` rounding at 29.97 and 23.976 with an off-grid `in_point`
  - rate 2 doubles the source advance
  - reverse maps the event end to `in_point`
  - `effective_end` with a loop past the end
  - `FramePlan` hash is stable and changes with any parameter
  - a muted event is skipped
  - the dissolve rule: the outgoing event has alpha 1 inside the overlap
- **composite:**
  - `mul255` matches `(a·b + 127) / 255` for all inputs
  - each mode matches an f64 reference within 1 LSB
  - the fast path equals the general path
  - striped output equals single-threaded output, bit for bit
  - `fit` leaves letterbox bars transparent
  - two layers at Level 0.5 give the closed-form value
  - default PanCrop of a 4:3 source in 16:9 gives pillarbox bars of the expected width
  - PanCrop w = 0.5 gives a 2× zoom
  - negative w flips
  - Motion w = h = 0.5 at x = 0.25 centres the track at 75% of the width
  - rot 90 maps (1, 0) to (0, 1)
- **cache:** stays under its byte budget; pinned stills survive.
- **vdecode::args:** exact argv for sample specs; hwaccel only for HEVC/AV1 or width ≥ 2560; yadif only when interlaced.
- **`content_hash`:** changes on any edit; does not change on cursor, zoom or scroll.
- **`#[ignore]` e2e:**
  - Frame accuracy: lavfi `nullsrc=s=64x64:r=30000/1001,geq=lum='mod(N,256)'`. Stream reads from several seek points equal single-frame decodes within ±1 code value. Repeat with a 25 fps source in a 29.97 project.
  - `bench_playback`: 4 × 1080p H.264 at Preview (Auto) reaches ≥ 29.5 fps with < 1% drops.
  - `bench_scrub`: report p50 and p95.
- **Manual (user):** MILAN_SCREENSHOT shots before and after look the same.

---

### Batch 3: Audio engine (S3 + audio half of S5)

**Scope**
- A streaming `Mixer` with:
  - clip gain and fade curves
  - volume, pan and mute envelopes
  - velocity (atempo) and reverse (areverse)
  - track gain
- `Player` internals:
  - block-t0 clock
  - live plan swap
  - loop handled inside the mixer
  - watchdog and reopen
  - releasable device thread
  - `send_or_quit`
- Simpler restart logic in `app.rs`.
- FX chains are present as empty slots, filled in batch 16.

**Files owned**
- `src/engine/mix.rs` (new)
- `src/engine/plan.rs` (audio half)
- `src/audio.rs`: `Player`, `Feed`; `render()` and `decode_pcm` are deleted after their tests are ported
- `src/app.rs` **(UI-shared, small)**: restart conditions (1.6 step 3) and the stall branch

**Test plan**
- **mix** (with a fake stream):
  - pan, volume, solo and fades (ported from `audio.rs`)
  - Smooth vs Linear fade-curve shapes
  - `gain_db −6` halves the amplitude
  - a volume envelope ramps across a block
  - two 1024-frame blocks equal one 2048-frame mix
  - a plan swap is heard on the next block
  - the loop wrap jumps `t0` from b to a
  - silence past `content_end` up to the loop end
- **atempo:** the argv chains steps correctly for rates 0.25, 0.5, 1.5 and 4.
- **Feed clock:** `position()` follows a `t0` jump.
- **watchdog:** `stalled()` is false while calls advance, true 400 ms after they stop, and resets when calls change.
- **send_or_quit:** a full `sync_channel(0)` with quit set returns false within 50 ms; a disconnected receiver returns false.
- **Manual (user):**
  - unplug headphones during playback: the playhead keeps moving and sound resumes
  - a loop region past the end wraps
  - mute, solo and volume changes are heard while playing

---

### Batch 4: Render through the engine (S4)

**Scope**
- `engine::render::start`.
- `Preset.muxer`.
- Render loop region only.
- The master gain lives in `timeline.master.gain`, mirrored from MasterView by `app.rs`.
- Delete `build_args`, its graph tests and the batch-1 graph-file interim.

**Files owned**
- `src/engine/render.rs` (new)
- `src/render.rs`
- `src/app.rs` **(UI-shared, 1 line)**
- `src/ui/dialogs/render.rs` **(UI-shared)**:
  - lines 353-354 become `self.job = Some(engine::render::start(project, preset, out, range, master_gain, ctx.clone()))`
  - the checkbox at :173 is enabled

**Enables:** the "Render loop region only" checkbox.

**Test plan**
- **argv**, for every preset:
  - contains `-f rawvideo -i -`, the muxer, the bt709 tags and `.partial`
  - stays under 1000 characters with 500 events
  - audio-only presets have no rawvideo input
- **`#[ignore]` e2e:**
  - every preset renders a 3-layer project
  - a loop-region render has the expected duration
  - cancel removes `.partial` and the temp audio
  - parity: decoded output frame k against `compose(frame_plan(k))` at full size has mean absolute error ≤ 2/255
  - master gain 0.5 halves the rendered audio

---

### Batch 5: Editing core

**Scope**
- Bug fixes:
  - B1: silent save errors (UI side)
  - B2: imports landing in the wrong project
  - B3: duplicate imports
  - B4: stale `selected_media`
  - B6: inconsistent track-height dirty state
  - B7: quantize
  - B13: deleting the last track
- Stable event ids and multi-selection.
- Track insert, delete, reorder and selection.
- Track heights and minimize.

**Files owned** (all **UI-shared** except edit.rs, project.rs, media.rs, settings.rs)
- `src/edit.rs`, `src/project.rs` (find helpers), `src/media.rs` (`merge_new`), `src/settings.rs` (`quantize`)
- `src/app.rs`, `src/app/commands.rs`
- `src/ui/menus.rs`, `src/ui/toolbar.rs`, `src/ui/media_panel.rs`, `src/ui/mod.rs`, `src/ui/transport.rs`, `src/ui/properties.rs`, `src/ui/trimmer.rs`
- `src/ui/timeline/mod.rs`, `events.rs`, `ctx.rs`, `tracks.rs`, `scroll.rs`

**Design**

1. **Save errors (B1).**
   - `menus::save` and `save_as` return `Result<bool, String>`, where `Ok(false)` means the user cancelled Save As.
   - In `exec`, Save and SaveAs set `self.message = "Save failed: {e}"` on Err and call `remember()` only on `Ok(true)`.
   - The save-before-New/Open/Exit prompt aborts the pending action on Err, so unsaved work is never thrown away after a failed save.
   - The toolbar Save becomes `menus::send(ctx, Command::Save)`.
   - The toolbar Undo and Redo become `Command::Undo` and `Command::Redo`, so all of that logic lives in `exec`.
2. **Imports (B2, B3, B4).**
   - `run()` (New/Open) resets `self.importer = Importer::new()` and `selected_media = None`. The stale batch's `send` then fails and is dropped.
   - `media::merge_new(pool, items) -> bool` skips paths already in the pool or earlier in the same batch. `app.rs:397-411` uses it and pushes history only when it returns true.
   - Undo, Redo and UndoAll clear `selected_media`.
   - `media_panel.rs:123-136` uses `.get(i)`.
3. **Ids and multi-selection.**
   - `MeridianApp.sel: Vec<String>` holds clip ids in click order; the last one is the focus.
   - `selected_clip` is derived each frame with `project.find_clip(sel.last())`, so the existing readers keep working.
   - After an undo, `sel` drops ids that no longer exist.
   - `project.rs` gains `find_clip`, `clip`, `clip_mut` and `find_track`.
   - New id-based ops in `edit.rs` (the ClipRef versions stay as thin wrappers until their callers move over):
     - `delete_clips(p, ids)` skips locked clips and tracks.
     - `split_ids(p, ids, at)`: an empty `ids` means every event under `at`.
     - `move_clips(p, ids, dt, dtrack)`:
       - clamps so the minimum position stays ≥ 0;
       - a track hop applies the same index offset within each kind, and is refused entirely if any target track is missing or locked;
       - touched tracks are stable-sorted by position afterwards.
   - Clipboard:
     - `Vec<(bool is_video, usize track_offset, Clip)>`, with positions relative to the earliest clip;
     - paste lays the clips out from the focus track at the cursor, with fresh uuids, and group ids remapped to fresh uuids.
   - Timeline clicks:
     - plain click replaces the selection;
     - Ctrl toggles;
     - Shift selects the range on the same track between the focus and the clicked clip.
   - `DragState.others: Vec<(String, Clip)>` holds the originals of the selected events. The grabbed clip snaps, and one delta applies to all of them, recomputed from `before` every frame.
   - Paint: `selected = sel.contains(&clip.id)`.
   - `CmdState.n_sel`. Delete, Cut, Copy and Split act on the whole selection.
4. **Tracks.**
   - `sel_tracks` moves from `TlState` to `MeridianApp` (track ids).
   - `edit::insert_track(p, is_video, after)`, `delete_tracks(p, ids)`, `move_tracks(p, is_video, ids, to)`.
   - `drop_media_range` creates missing video tracks the way it already creates audio tracks. This fixes B13 at the root, so deleting every track is allowed, as in VEGAS.
   - Header reorder drag:
     - a vertical drag of more than 4 px that starts on the number or colour strip;
     - a 2 px accent insertion line;
     - one undo step on release;
     - the bottom-edge height zone keeps priority.
   - Insert Track goes after the focused track of that kind (verify above or below against VEGAS).
   - Delete removes tracks when the track list has keyboard focus and no events are selected.
5. **Heights (B6).**
   - Every height change calls `mark_modified()` and never pushes history (header drag, scroll ±, TracksTaller, TracksShorter, Default Track Heights, the ctx Default Track Height).
   - `MinimizeTracks` uses `MeridianApp.minimized: Option<Vec<(String, f32)>>` to save heights and restore them.
   - A Ctrl-drag of a bottom edge resizes all selected tracks.
6. **Quantize (B7).**
   - `prefs.quantize` (default true).
   - `edit::quantize(t, fps) = (t·fps).round() / fps`.
   - Applied to moves (after snapping), trims and paste positions.
   - `Command::Quantize` becomes a live toggle.

**Enables:**
- SelectAll (Ctrl+A) and SelectNone
- MuteTrack (Z) and SoloTrack (X) on `sel_tracks`
- FocusPrevTrack and FocusNextTrack
- MinimizeTracks (`)
- Quantize
- View > Minimize All Tracks
- Options > Quantize to Frames

**Test plan**
- **merge_new:**
  - a path already in the pool is skipped;
  - a path twice in one batch is added once;
  - returns false when nothing was added.
- **edit:**
  - `delete_clips` skips locked clips and tracks;
  - `split_ids` with empty ids splits only the events under the cursor;
  - `move_clips` clamps at 0, refuses a hop onto a locked track for all clips, and sorts afterwards;
  - paste across two tracks keeps offsets and gives fresh ids;
  - `move_tracks([1, 3] → 0)` gives the order 1, 3, 0, 2, and a target past the end is clamped;
  - `delete_tracks` removes their clips;
  - dropping onto video track 0 with no video tracks creates one;
  - `insert_track(after = Some(0))` places the track at index 1;
  - `quantize` at 29.97 lands on n/29.97, leaves an on-grid value unchanged, and is idempotent;
  - `toggle_minimize` twice restores the heights.
- **project:** `find_clip` survives a reorder.
- **Manual (user):**
  - save to a read-only folder: the status bar shows the error;
  - start a 50-file import and press Ctrl+N: the new project stays empty;
  - Ctrl+click three events, drag them: one undo step.

---

### Batch 5b: Drag media to the timeline (user priority)

The user asked for this explicitly. Today a drag from Project Media, the Explorer tab or the Trimmer onto an existing track of the right kind works (`ui/timeline/mod.rs` `dnd_release_payload::<MediaDrag>`, `edit::drop_media_range`). Everything below is missing and must work the way VEGAS Pro does it.

1. **Files from Windows File Explorer onto the timeline.** `app.rs` imports `raw.dropped_files` into Project Media only. Instead:
   - track where the OS drag is hovering (`raw.hovered_files` and the pointer position) and show the same drop indicator and ghost as an in-app drag;
   - on drop over the timeline, import the files (reuse the importer and `merge_new`; a file already in the pool is reused, not imported twice), then place each one when its probe finishes, at the drop time and track that were captured at drop time (store a pending placement keyed by path; it is cancelled by New/Open, and is one undo step per placed batch);
   - several files are added across time (one after another from the drop point, VEGAS's default "Add Across Time");
   - a drop anywhere else (media panel, preview, empty window) keeps today's behaviour: import only;
   - files that ffprobe rejects show the existing error and place nothing.
2. **Drop below the last track, or on an empty timeline, creates tracks** of the right kind (video track for video/image media, audio track for audio), as VEGAS does. A video file with audio creates a video track and an audio track.
3. **Wrong-kind rows:** dropping audio on a video row places it on the first audio track (creating one if none), and video/image on an audio row places it on the first video track (creating one), instead of silently failing.
4. **Paired audio placement:** a video with audio puts its audio on the audio track that VEGAS would use: the first audio track if it is free over the event's time range, otherwise a new audio track (no overlap onto existing audio). Both events share the group id once batch 6 lands; leave a `group` hook if batch 6 is not in yet.
5. **Ghost while dragging:** draw a translucent event rectangle (event colour, name, real length at the current zoom) for each event that will be created, on the target rows, snapped like the drop itself, plus the existing indicator line. Rows that would be created are shown as a dashed ghost row below the last track.
6. **Multi-select drag from Project Media:** dragging with several media items selected adds all of them across time, in list order.
7. **Stills** get the image length from prefs (default 5 s, VEGAS's "New still image length"), using the existing default if one exists.
8. **Locked tracks** refuse the drop with the "no" cursor; the ghost turns red.
9. Every drop is one undo step and marks the project modified; the dropped events become the selection.

**Test plan**
- `edit` unit tests: wrong-kind row redirect; drop below last track creates tracks; paired audio goes to a new audio track when the first one is occupied; across-time placement of three items; still length; locked track refused.
- A simulated-input UI test (like the existing drag tests) that drags a Project Media item onto a track and below the last track.
- A test for the pending OS-file placement: a fake probe result arriving after New is discarded; one arriving normally lands at the captured time and track.
- Harness screenshot is not enough for the ghost; add a debug flag or test that renders a frame mid-drag if practical, otherwise describe what was checked.

---

### Batch 6: Edit tools, grouping, automatic crossfades, ripple

**Scope**
- `EditTool` state and the Selection-tool rubber band.
- Event grouping (fixes B8: video and its audio now stay linked).
- Automatic crossfades on overlap, plus the crossfade Fade Type menu.
- Auto Ripple modes and Post-Edit Ripple.

**Files owned** (**UI-shared** except edit.rs and settings.rs)
- `src/edit.rs`, `src/settings.rs`
- `src/app.rs`, `src/app/commands.rs`
- `src/ui/mod.rs`, `src/ui/keymap.rs`, `src/ui/menus.rs`, `src/ui/transport.rs`, `src/ui/trimmer.rs`
- `src/ui/timeline/mod.rs`, `ctx.rs`, `events.rs`

**Design**

1. **Tools.**
   - `enum EditTool { #[default] Normal, Envelope, Selection, Slip, Slide, Stretch }` in `ui/mod.rs`, held in `MeridianApp.tool` and not saved.
   - NextTool and PrevTool (D, Shift+D) cycle Normal → Envelope → Selection. There is no Zoom tool.
   - New `Command::Tool(EditTool)` for the Edit > Editing Tool submenu and the transport radios.
   - Transport: the Normal split button gets ▾ Normal / Slip / Slide / Time Stretch; the Envelope and Selection buttons become live radios; Zoom stays inert.
   - Rubber band:
     - `TlState.rubber: Option<(Pos2, bool additive)>`;
     - starts on an empty-lane drag when the tool is Selection, or when the look is VP23 and the tool is Normal;
     - the VP16 Normal empty-lane drag still makes a time selection;
     - painted as a dashed 1 px accent rectangle;
     - on release, selects the events whose rects intersect it (union with the old selection when Ctrl is held);
     - with the Selection tool, also sets `loop_region` to the rectangle's span (verify against VEGAS);
     - VP23 right-drag on an empty lane makes a time selection.
   - In the Selection tool, clicking an event selects it, but events cannot be dragged and edges are not trim zones.
   - Pure helper: `clips_in_rect(rows, geo, rect) -> Vec<String>`.
2. **Grouping.**
   - `edit::group`, `ungroup` (any one-member group left behind is dissolved), `clear_groups`, `expand_groups`, `edge_mates(p, id, left, tol = 1 frame)`.
   - `drop_media_range` and the Trimmer add give the video clip and its audio clip the same group id.
   - A click selects the whole group, unless `prefs.ignore_grouping` is set or the tool is Slip.
   - A trim drag puts the `edge_mates` into `DragState.others`, each clamped by its own `media_limit`.
   - `split_ids` maps each old group to a new uuid for the right halves.
   - The Trimmer add pushes its own history step.
3. **Automatic crossfades.**
   - `prefs.auto_crossfade` (default true) replaces `MeridianApp.auto_crossfade`.
   - `edit::auto_crossfade(track, before, ids)` runs after move, trim, paste and drop:
     1. sort by position;
     2. for each consecutive pair touching `ids`, compute `o = a.end − b.pos`;
     3. if `o > 0` and b is not fully covered, set `a.fade_out = b.fade_in = o`;
     4. if the pair overlapped in `before` with fades equal to the old overlap, but no longer overlaps, reset those fades to 0. A fade the user set by hand is kept.
   - `ctx::Target::Crossfade(a, b)` offers Fade Type ▸ Linear / Fast / Slow / Smooth / Sharp, which sets the paired curves.
4. **Ripple.**
   - `RippleMode { Affected, AffectedMarkers, All }` and `RippleOp { from, delta, tracks }` in `edit.rs`.
   - `ripple(p, op, mode, exclude)`:
     1. delta is clamped to ≥ −(minimum position of the affected clips);
     2. clips at or after `from` move by delta;
     3. track envelope keys at or after `from` move, and keys inside a closed gap are deleted first;
     4. modes other than Affected also shift markers, regions (a region that straddles `from` stretches) and the loop region.
   - `delete_range(p, a, b, tracks, mode)` splits at a and b, deletes the pieces inside, then ripples by −(b − a).
   - Hooks while Auto Ripple is on:
     - Delete or Cut: ripple each removed span, right to left.
     - Paste: ripple from the cursor by the pasted length, before inserting.
     - Trim right: ripple from the old end by the change.
     - Trim left: the event start stays fixed, its content shifts, and later material moves left (verify against VEGAS).
     - A same-track move ripples from the old position, excluding the dragged set. Cross-track moves do not ripple.
     - Delete with a time selection and nothing selected: `delete_range` over the selected tracks, or every track in All mode.
   - During a drag, recompute from `DragState.before` every frame, so ripple is never applied on top of itself.
   - Post-edit:
     - every length-changing edit stores `MeridianApp.last_edit: Option<RippleOp>`;
     - F, Ctrl+F and Ctrl+Shift+F apply it with their mode as one undo step.
   - `prefs.auto_ripple` and `prefs.ripple_mode`.
   - The selected-event outline is red (#DC3C3C) while Auto Ripple is on.
   - Bus tracks do not exist in Milan, so mode 2 means affected tracks plus markers and regions.

**Enables:**
- NormalTool, NextTool, PrevTool, `Tool(_)`
- Group (G), RemoveFromGroup (U), ClearGroup (Ctrl+U), SelectGroup (Shift+G), IgnoreGrouping (Ctrl+Shift+U, checked)
- AutoRipple (Ctrl+L, checked), `RippleMode(_)` (radio)
- RippleAffected (F), RippleBus (Ctrl+F) and RippleAll (Ctrl+Shift+F), enabled when `last_edit` is set
- PasteInsert
- transport Normal ▾, Envelope and Selection radios, Auto Ripple ▾, Ignore Grouping
- the Edit > Editing Tool items (Normal, Envelope, Selection, Slip, Slide, Time Stretch)
- Post-Edit Ripple, the Group submenu, Options > Auto Ripple and the Ripple submenu
- event ctx Group
- `CmdState` gains `tool`, `in_group`, `last_edit`, `auto_ripple`, `ripple_mode`, `ignore_group`.

**Test plan**
- **clips_in_rect:**
  - an event partly inside is selected;
  - an event on another row is not;
  - a zero-width rect selects nothing.
- **group:**
  - drop_media of video with audio gives two clips with the same group;
  - ungrouping one of a pair leaves no one-member group;
  - `expand_groups`;
  - a split of a grouped pair gives two groups;
  - `edge_mates` honours the tolerance;
  - `delete_clips(expand_groups(..))` removes the A/V pair.
- **auto_crossfade:**
  - moving B over A by 1 s gives fades of 1;
  - moving further gives larger fades;
  - pulling apart gives 0;
  - a hand-set 0.5 s fade on a non-overlapping pair is kept;
  - a fully covered event gets no crossfade;
  - with three clips, only the pairs involving the moved clip change;
  - trimming A's right edge into B makes a crossfade.
- **ripple:**
  - Affected leaves other tracks and the markers alone;
  - AffectedMarkers moves markers and stretches a straddling region;
  - All moves every track;
  - a negative delta is clamped;
  - envelope keys in the gap are deleted and later keys shift;
  - excluded ids do not move;
  - `delete_range`: a straddling clip is split, the tail moves by −(b − a), and the length shrinks by b − a;
  - post-edit ripple equals the auto-ripple result for the same edit.
- **menus:**
  - default `CmdState` plus `n_sel = 2`: Group is enabled, RemoveFromGroup is disabled;
  - RippleAffected is disabled without `last_edit`;
  - `checked(AutoRipple)` follows the state.
- **Manual (user):** D cycles tools and the transport radio follows.

---

### Batch 7: Rate, reverse, slip/slide/roll/stretch, event level handle

**Scope**
- Rate-aware edit maths and the Reverse command.
- Slip, slip-trim, slide, roll and time-stretch drags, and the VP23 tools that force them.
- The event gain/opacity handle.
- Playback already honours all of these (batches 2-3).

**Files owned**
- `src/edit.rs`, `src/project.rs` (`Clip::opacity`, `set_opacity`)
- **UI-shared:** `src/app/commands.rs`, `src/ui/mod.rs`, `src/ui/keymap.rs`, `src/ui/menus.rs`, `src/ui/timeline/mod.rs`, `events.rs`, `ctx.rs`

**Design**
1. **Rate and reverse.**
   - `edit::src_time` is shared with the plan.
   - `media_limit` trims: `max_dur = (limit − in) / rate`.
   - `trim_left` (forward): `in += (start − pos) × rate`. On a reversed event a left trim leaves `in_point` unchanged and a right trim shifts it.
   - `split_clip`: forward, the right half gets `in += first_dur × rate`; reversed, the first half gets `in + second_dur × rate`.
   - `Command::EventReverse` toggles reverse on the selection as one undo step.
   - The timeline draws thumbnails at `src_time`, reads the waveform mirrored when reversed, and puts "◄" in the label.
2. **Drags.**
   - `DragMode` gains Slip, SlipTrimLeft, SlipTrimRight, Slide, RollLeft, RollRight, StretchLeft and StretchRight.
   - The mode comes from the hit zone plus modifiers (Alt, Ctrl+Alt, Ctrl), or from the tool forcing one.
   - Each drag is a pure function `(orig, delta, frame, limit) -> Clip`:
     - `slip`: `in = orig.in − d × rate`, clamped to [0, limit − dur × rate].
     - `slip_trim_right`: the left edge is fixed and `in = orig.in + (orig.dur − dur) × rate`, so the frame at the dragged edge stays the same. The left edge mirrors it.
     - `slide`: `pos += d` and `in += d × rate`, clamped by the media limits.
     - Roll: the neighbour whose edge is within 1 frame gets the opposite trim, each within its own limit.
     - `stretch_right`: `rate = orig.rate × orig.dur / new_dur`, clamped to [0.25, 4]; `dur = orig.dur × orig.rate / rate`. The left edge moves `pos` instead.
   - Slip acts on the event under the cursor only.
   - The tooltip shows the rate %, and the label shows "(rate 150%)".
   - Every result is quantised when `prefs.quantize` is on.
3. **Level handle.**
   - `DragMode::Level`, hit in the top 6 px between the fade zones.
   - Audio: `gain_db = 20·log10(frac)`, with frac = 1 − dy / event height; floor −120, max 0.
   - Video: opacity stored as a single-key `EnvKind::Opacity` envelope through `Clip::set_opacity`. The handle is hidden if that envelope has more than one key.
   - Paint: a level line, the waveform scaled by gain, thumbnails dimmed by (1 − opacity), and a tooltip ("-3.2 dB" or "48.1%").

**Enables:** EventReverse, EventMute (Switches > Mute), the event ctx Reverse and Switches > Mute, and the Slip, Slide and Time Stretch tool entries.

**Test plan**
- **src_time:** forward and reverse at both event edges.
- **trims:** a rate-2 `trim_right` is limited to (limit − in) / 2.
- **reversed split:** the halves' source ranges join without a gap and play in decreasing order.
- **Reverse:** toggled twice is the identity.
- **drags:**
  - `slip` keeps position and duration and clamps `in`;
  - `slip_trim_right` keeps the edge frame;
  - `slide` keeps the timeline-to-source mapping (src at 5 s unchanged);
  - roll keeps A.end == B.pos within both limits;
  - stretch keeps rate × dur constant and clamps at 0.25 and 4;
  - every result is quantised.
- **gain mapping:** frac 1 → 0 dB; 0.5 → about −6.02 dB; 0 → −120.
- **opacity:** `set_opacity` then `opacity()` round-trips; an empty envelope gives 1.0.

---

### Batch 8: Envelopes, Envelope tool, curve menus, Lock Envelopes

**Scope**
- Track envelopes:
  - audio: Volume, Pan, Mute
  - video: Composite Level
- Event envelopes, rebased on split and trim.
- Drawing, hit-testing and point add/move/delete with context menus.
- The Envelope Edit tool.
- Fade-curve menus.
- Lock Envelopes to Events.
- Everything is audible and visible already (batches 2-3).

**Files owned**
- `src/edit.rs`, `src/settings.rs` (`show_envelopes`, `lock_envelopes`)
- `src/ui/timeline/envelopes.rs` (new)
- **UI-shared:** `src/app.rs`, `src/app/commands.rs`, `src/ui/keymap.rs`, `src/ui/menus.rs`, `src/ui/transport.rs`, `src/ui/timeline/mod.rs`, `ctx.rs`, `events.rs`

**Design**
1. **edit.rs helpers:**
   - `env_default`, `env_range`
   - `toggle_envelope(track, kind)`: adds an envelope with one key at 0 holding the default, or removes it
   - `env_insert` (the new key copies its left neighbour's curve)
   - `env_move` (t clamped strictly between the neighbours, v clamped to the range)
   - `env_delete` (always keeps at least one key)
   - `env_set_curve`
   - `env_flip` (1 − frac in lane space)
   - `env_thin` (tolerance 0.005 of the lane)
   - `env_reset`
   - `keys_in_span(env, a, b)`, inclusive at both ends
   - `shift_clip_keys(clip, dt)`: shifts every keyframe list on the clip (fx params, pan_crop, envelopes). It drops keys before 0 but keeps the value at 0 by inserting a boundary key. `trim_left` and `split_clip` call it.
   - `carry_envelope_points(track, old_span, delta)`:
     - removes keys inside the destination span;
     - shifts keys inside the old span;
     - inserts boundary keys holding the pre-move values;
     - `move_clips` calls it when `prefs.lock_envelopes` is set;
     - cross-track moves carry points only when the destination has an envelope of the same kind (shortcut, with a code comment).
2. **`envelopes.rs`:**
   - `lane_frac` and `lane_value`:
     - Volume above 0 dB: 0.7 + 0.3 × dB / 6;
     - Volume below 0 dB: 0.7 × 10^(dB / 40);
     - other kinds: linear over their range.
   - `paint`: samples `Keyframes::at` every 2 px into one polyline, 1 px wide (2 px when hovered), with 5×5 point squares (a selected point is filled white). Drawn after the events.
   - `hit`: 4 px tolerance for points, 3 px for the line. Tested before `hit_clip`.
3. **Interaction** (Normal or Envelope tool, while `show_envelopes` is on):
   - double-click on the line adds a point;
   - drag a point (Ctrl = vertical motion / 10; time snaps unless Shift is held);
   - drag a segment to move both of its keys;
   - `MeridianApp.env_drag` gives one history step per gesture;
   - `TlState.env_sel` holds selected points, and Delete removes them before event Delete is considered;
   - tooltips in dB, L/R % or Composite Level %.
   - Context targets:
     - `EnvPoint`: Set to Max / Default / Min, Set To…, Delete, curve types;
     - `EnvLine`: Add Point, curve types, Flip All, Thin All, Reset All.
4. **Envelope tool:**
   - `hit_clip` is skipped;
   - a click on the line adds a point;
   - an empty-lane drag selects keys in that time span on the topmost visible envelope;
   - dragging a selected point moves the whole selection, each key clamped between its unselected neighbours.
5. **Fades:** `ctx::Target::Fade(clip_id, is_in)` within `FADE_PX` gives Fade In ▸ / Fade Out ▸ curves. `paint_fades` uses `Curve::shape`.

**Enables:**
- VolumeEnvelope (V), PanEnvelope (P), RemoveVolumeEnvelope, RemovePanEnvelope, `Envelope(kind)`
- ShowEnvelopes (Ctrl+Shift+E)
- LockEnvelopes (transport and Options)
- Insert > Audio and Video Envelopes (Mute, Volume, Pan, Composite Level)
- track and lane ctx Insert/Remove Envelope
- View > Show Envelopes
- `CmdState` gains `show_env`, `lock_env`, `env_sel`, `audio_track_focus`, `video_track_focus`.

**Test plan**
- **env helpers:**
  - `env_insert` keeps the keys sorted;
  - `env_move` clamps t and v;
  - `env_delete` never empties the envelope;
  - `env_thin` removes a collinear midpoint and keeps a corner;
  - `env_flip` twice is the identity;
  - `toggle_envelope` adds then removes;
  - a multi-key move is clamped by an unselected neighbour;
  - `env_set_curve` on a segment changes only its left key;
  - `keys_in_span` is inclusive.
- **lane mapping:**
  - `lane_frac` / `lane_value` round-trip within 1e-4 for every kind;
  - 0 dB → 0.7, +6 dB → 1.0, −120 dB → about 0, Pan 0 → 0.5.
- **shift_clip_keys:**
  - a split at 2 s of a clip with a key at 3 s leaves the right half's key at 1 s, and the value at the old timeline time is unchanged on both halves;
  - `trim_left` by +1 s shifts keys by −1 s.
- **carry_envelope_points:**
  - keys inside a moved event move with it;
  - keys outside stay;
  - the value just outside the old span is unchanged;
  - with the lock off, nothing moves.
- **Manual (user):** a volume envelope is heard during playback, and a Composite Level envelope is seen.

---

### Batch 9: Takes and Event Properties

**Scope**
- Next and Previous Take.
- The Take submenu.
- Add as Takes through the right-drag drop popup.
- The Event Properties dialog.

**Files owned**
- `src/edit.rs`
- `src/ui/dialogs/event_props.rs` (new)
- **UI-shared:** `src/ui/dialogs/mod.rs`, `src/app.rs`, `src/app/commands.rs`, `src/ui/menus.rs`, `src/ui/timeline/mod.rs`, `ctx.rs`, `events.rs`

**Design**
1. **Takes.**
   - `edit::next_take(p, ids, forward)` rotates the active take (media_id, in_point, name) through `takes`; duration is kept.
     - shortcut: a take shorter than the event holds its last frame and goes silent past its end (VEGAS loops).
   - `add_as_take`.
   - `delete_active_take` is refused when there are no takes.
   - `delete_inactive_takes`.
   - Right-drag drop: read `secondary_released()` at release time and show a popup with Add Across Time / Add Across Tracks / Add as Takes / Cancel.
   - The VP23 label shows the active take's name.
2. **Event Properties.** `EventProps { id, draft: Clip }`, opened by `Command::EventProperties`. Its Event page has:
   - name
   - active take combo
   - position, length and in point as timecode
   - rate 0.25-4.00 (length recomputed as source span / rate)
   - reverse, mute, lock
   - gain dB (audio) or opacity % (video)
   - fade lengths and curves
   - The VEGAS-only switches are shown greyed: Undersample, Maintain aspect, Loop, Normalize, Pitch.

   The Media page shows read-only rows from the MediaItem.

   OK: if `*c != draft`, assign the draft, call `clamp_fades`, push one history step and mark modified. The dialog is registered in `modal_open()`.

**Enables:** NextTake (T), PrevTake (Shift+T), the event ctx Take submenu and Properties.

**Test plan**
- **takes:**
  - `next_take` then previous take is the identity;
  - position and duration are unchanged;
  - `add_as_take` makes the new media active and keeps the old in `takes`;
  - `delete_active_take` with no takes is refused;
  - a split keeps the takes on both halves.
- **properties:**
  - `apply_rate` keeps dur × rate and clamps the rate;
  - an unchanged draft pushes no history.
- **Manual (user):** a change to name, rate and curves is undone with one Ctrl+Z.

---

### Batch 10: Keyframe controller, Event Pan/Crop, Track Motion

**Scope**
- The shared keyframe controller widget (F5).
- The Pan/Crop window (F10).
- The Track Motion window (F11).
- The VP23 Comp button (composite mode menu).
- The preview updates live through the content hash.

**Files owned**
- `src/ui/keyframes.rs` (new), `src/ui/workspace.rs` (new), `src/ui/pancrop.rs` (new), `src/ui/trackmotion.rs` (new)
- **UI-shared:** `src/app.rs`, `src/app/commands.rs`, `src/ui/keymap.rs`, `src/ui/menus.rs`, `src/ui/timeline/events.rs` (Pan/Crop chip), `tracks.rs` (Track Motion and Comp buttons)

**Design**
1. **Keyframe controller.**
   - `controller(ui, lanes, span, cursor, fps) -> KfResponse`, where `KfResponse` is one of Seek, Add, Delete, Move, SetCurve, Begin or End. The caller applies the intent.
   - Painting: 15 px ruler, 12 px lanes, grey diamonds, a white cursor flag and a checker past the event end.
   - Sync Cursor, on by default.
   - Right-clicking a key sets its curve.
2. **Workspace helpers** (shared): box handles, grid, view zoom; resize with lock aspect or about the centre; rotate; pixel ↔ normalised conversion.
3. **Pan/Crop window.**
   - Toolbar: Normal/Zoom, Snapping, Lock Aspect, Size About Center.
   - The grid shows source pixels.
   - Light #ECECEC workspace with the source thumbnail (`Thumbs::get`), a dashed box, 8 handles and a rotation circle.
   - Edits go through `set_key(.., Curve::Smooth)`, with one history step per drag.
   - Right-click: Restore, Center, Flip Horizontal, Flip Vertical, Match Output Aspect.
   - The keyframe lane is "Position".
   - Keys are relative to the event start.
4. **Track Motion window.**
   - The box marks the track output inside the project frame.
   - The grid shows X/Y offset from the centre plus Width/Height in pixels.
   - A composite-mode combo.
   - Absolute-time keys.
   - Out of scope: 2D Shadow, 2D Glow, 3D, parenting (section 3).

**Enables:** `EventPanCrop`, `TrackMotion` (new commands), Tools > Video > Video Event Pan/Crop and Track Motion, the event ctx Video Event Pan/Crop, the Pan/Crop chip, the Track Motion button and the VP23 Comp button.

**Test plan**
- **workspace:**
  - a lock-aspect resize keeps the pixel aspect;
  - resize about the centre keeps cx/cy;
  - rotate gives the drag vector's angle;
  - pixel ↔ normalised round-trips.
- **controller intents** (pure helper `kf_hit`): the click position maps to the right key, or to a Seek.
- **content hash:** composite mode is stored, and the hash changes with it.
- **Manual (user):** drag the box and see the preview update; add two keys and play.

---

### Batch 11: Dock window layouts

**Scope**
- Restore the last layout per look at the next launch.
- User layout slots 0-9 with names.
- Optional: a tab-overflow scroller.

**Files owned**
- `src/settings.rs` (`Prefs.layouts: BTreeMap<String, Layout>`)
- **UI-shared:** `src/ui/dock.rs`, `src/app.rs`, `src/app/commands.rs`, `src/ui/menus.rs`, `src/ui/keymap.rs`

**Design**
- `Panel` and `Group` derive serde.
- `Layout { name, groups, height: Option<f32>, track_list_w }`.
- `Dock::layout()` and `Dock::apply(&Layout, look)`. `apply` sanitises the layout:
  - drops duplicate panels;
  - drops panels the look lacks (ColorGrading in VP16);
  - drops empty groups;
  - clamps widths to at least `MIN_W`;
  - fixes `active`;
  - falls back to `Dock::new` when nothing is left.
- Keys are `"vp16:last"` and `"vp16:0"`…`"vp16:9"`, and the same for vp23.
- Store `last` on Exit and on a look change. Apply it after `Dock::new`.
- `LayoutUser(n)` and `SaveLayoutUser(n)` for n in 0-9: add TABLE rows for 2-9 (Alt+D n, Ctrl+Alt+D n).
- Save Layout As opens a modal with a name and a slot.
- View > Window Layouts lists the saved slots.

**Enables:** the Window Layouts items except Organize, Reload, the preset layouts and Timeline Top.

**Test plan**
- `Layout` serde round-trip.
- `apply()` removes duplicates, ColorGrading under VP16, empty groups and widths below the minimum, and keeps every other panel exactly once.
- `SaveLayoutUser(3)`, then a move, then `LayoutUser(3)` restores the groups.
- A look change keeps each look's own layout.
- Corrupt `layouts` falls back to the default.
- Keymap: Alt+D 5 → `LayoutUser(5)` and Ctrl+Alt+D 5 → `SaveLayoutUser(5)`.

---

### Batch 12: Video proxies

**Scope**
- Create Video Proxy, run as a background queue.
- Draft and Preview quality use the proxy through the engine; Good, Best and render never do.
- Thumbs always use the proxy when one exists.
- A preference to proxy 4K, HEVC and AV1 automatically on import.

**Files owned**
- `src/proxy.rs` (new), `src/main.rs`, `src/settings.rs` (`auto_proxy`)
- `src/engine/plan.rs`, `src/engine/vdecode.rs` / `worker.rs` (proxy choice, cache key), `src/decode.rs` (Thumbs)
- **UI-shared:** `src/app.rs`, `src/app/commands.rs`, `src/ui/menus.rs`, `src/ui/media_panel.rs`, `src/ui/status.rs`

**Design**
- Proxy paths:
  - `proxy_path(src)` = `<src>.milanproxy.mp4`;
  - fallback = `%LOCALAPPDATA%\MilanPro\proxies\<hash>.mp4`, for read-only source folders.
- `find(src)` checks milanproxy, then the fallback, then VEGAS's `<src>.sfvp0`:
  - the candidate must be newer than the source;
  - a `.sfvp0` is used only if ffprobe sees video with a duration within one frame of the source.
- Encode args:

  ```
  -hide_banner -v error -i SRC -map 0:v:0 -an -sn -dn
  -vf scale='min(1280,iw)':-2:flags=bicubic,format=yuv420p
  -c:v libx264 -preset veryfast -crf 23 -g 15 -bf 0 -tune fastdecode
  -fps_mode passthrough -movflags +faststart -progress pipe:1 -y OUT.partial.mp4
  ```

- `ProxyJobs`:
  - one job at a time;
  - the child goes through `proc::spawn` at below-normal priority;
  - `.partial` is renamed on success and deleted on failure or cancel;
  - jobs survive New/Open (proxies belong to files, not projects).
- `READY` registry plus `GENERATION`:
  - shortcut: a global, so the `Thumbs`/`Preview` signatures do not change.
- `plan::compile` fills `MediaRef.proxy`.
- The worker hash XORs `GENERATION`.
- `SourceCache` keys gain a proxy bool.
- Media panel: Create Video Proxy is enabled for video, and items with a proxy show a P badge.
- Status bar: "Creating video proxy 2 of 5: 43%".

**Test plan**
- **unit:**
  - naming of both paths;
  - `find` ignores a stale proxy and prefers milanproxy over sfvp0;
  - exact argv;
  - the queue removes duplicates;
  - `poll` renames on success and deletes on failure (fake child exit codes).
- **`#[ignore]` e2e:**
  - a 1080p source with the frame number encoded in luma: frames 0, 37 and 300 decoded from the proxy and the source match;
  - a cancel leaves no `.partial`.

---

### Batch 13: VEGAS project import

**Scope**
- A VEGAS script that exports to JSON (ScriptPortal.Vegas, plus a Sony.Vegas copy for VEGAS 13 and earlier).
- A Milan importer for it.
- A no-VEGAS fallback that scans a `.veg` for media paths.

**Files owned**
- `tools/vegas/Export to Milan Pro.cs` (new), `tools/vegas/Export to Milan Pro (VEGAS 13).cs` (new)
- `src/interchange.rs` (new), `src/main.rs`
- `installer/milan-pro.iss` (installs to `{app}\vegas\`)
- **UI-shared:** `src/app/commands.rs` (`ImportVegas`, applied like Open with the save prompt), `src/ui/menus.rs` (File > Import > VEGAS Project...), `src/ui/dialogs/mod.rs` (warnings modal)

**Design**
- The script writes `<project>.milan.json` as hand-built JSON:
  - `format: "milan-vegas"`, `version: 1`
  - `project { w, h, fps, sample_rate }`
  - `tracks` with kind, mute, solo, volume, pan, composite level and mode, envelopes, and events (start, length, offset, rate, path, stream, fades, curves, mute, locked)
  - `markers`, `regions`, `loop`
  - `unsupported[]`: FX, inactive takes, groups and generators are listed here.
- `read_vegas_json` uses serde with every field defaulted; a wrong `format` is an error.
- `to_project(e, probed, fps)` maps fields 1:1 onto the batch-1 model and snaps times to frames.
- The work runs on a thread: parse, probe each unique path, build the project.
- Missing media becomes an offline item plus a warning.
- `.veg` fallback (about 40 lines):
  - scan for UTF-16LE absolute paths with media extensions;
  - import the ones that exist into Project Media;
  - tell the user the layout needs the script.
- Confirm the script property names against `ScriptPortal.Vegas.dll` and the bundled "Export EDL.js".

**Test plan**
- **Hand-written fixture JSON:**
  - tracks, in-points, fades, curves, volume, pan, markers, regions and loop map correctly;
  - times snap to 29.97 frames;
  - a missing path gives an offline item plus a warning;
  - unknown fields are ignored;
  - a wrong `format` is an Err.
- **.veg scan:** finds the UTF-16LE paths in a synthetic buffer and ignores non-path strings.
- **Manual (user, VEGAS Pro 15):** run the script, import the JSON and compare.

---

### Batch 14: Video plug-in engine (F1, F2, F6, F8 render, F9, F12 maths)

**Scope**
- Pure engine code with no UI.
- When it lands, FX, transitions and generators already stored in a project render in the preview and in the final render.

**Files owned**
- `src/engine/plugins.rs`, `vfx.rs`, `transition.rs`, `generate.rs`, `fonts.rs`, `lut.rs`, `thumbs.rs` (all new)
- `src/engine/composite.rs` and `plan.rs` (call sites)
- `src/decode.rs` (Thumbs and Trimmer render generator media)
- `src/audio.rs` (waveforms skip generators)
- `src/edit.rs` (`media_limit` returns infinity for generators)
- `Cargo.toml` (`ab_glyph = "0.2"`, already in the lock)

**Design**

1. **Registry** (`plugins.rs`):
   - `PluginInfo { id, name /* VEGAS name without "VEGAS " */, kind, group, category, desc, params: &[ParamSpec], presets: &[Preset] }`.
   - `ParamKind` is one of Float{min, max, default}, Color{default}, Choice, Bool, Text{multiline}, Font.
   - Storage:
     - a Color is stored as four floats, `key.r`, `key.g`, `key.b`, `key.a`;
     - Choice and Bool are floats (index, 0/1);
     - Text and Font go in `Fx.text`;
     - a missing key means the default.
   - `find`, `catalog(kind)`, `new_fx(info, preset)`.
   - `eval(fx, t) -> Option<FxEval>`:
     - an unknown id gives None, and the plug-in is bypassed;
     - `FxEval` implements Hash.
   - User presets live in `%APPDATA%\MilanPro\presets.json`, through `load_user_presets` and `save_user_preset -> io::Result`.
   - Audio FX register here too (batch 16).
2. **Video FX** (`vfx::apply(img, e)`):
   - One shared `ColorOp { lut: [[u8; 256]; 3], m: Option<[[f32; 3]; 3]>, mix }`:
     - unpremultiplies only when a < 255;
     - applies the matrix, then the LUT, then mixes, then re-premultiplies;
     - striped.
   - On ColorOp: Brightness and Contrast, Levels, Invert, Black and White, Sepia, HSL Adjust and Color Corrector.
     - shortcut: HSL hue uses a YIQ rotation matrix, so it is not exact HSL.
   - Spatial:
     - Gaussian Blur: three separable box passes, radius = range × h.
     - Sharpen: unsharp mask with a threshold.
     - Vignette.
     - Mirror.
     - Crop with feather.
     - Border.
     - Chroma Keyer: CbCr distance, smoothstep between low and high, an alpha blur and mask-only mode. No spill suppression.
3. **Transitions** (`transition::mix(kind, a, b, p, out)`):
   - dissolve (Additive preset), fade_to_color (Dip to Black, Dip to White, Flash), linear_wipe, push, slide (8 directions), iris (circle, rectangle, diamond), clock_wipe, zoom.
   - Inside a Mix, fade alphas are ignored.
   - Audio crossfades are unchanged.
4. **Generators** (`generate::render(e, w, h, t)`):
   - Solid.
   - Gradient: linear, radial or rectangle; stops are stored in `text["stops"]` as `"0:#000000ff;1:#ffffffff"`; a 256-entry LUT.
   - Checkerboard.
   - Noise: value-noise fBm with a seed and an animate speed.
     - shortcut: value noise only; add Perlin or marble types if presets need them.
   - Text (`milan.gen.text`) through ab_glyph:
     - line layout with kerning;
     - an f32 coverage mask;
     - the outline is a square max-dilate (shortcut: switch to a round kernel if corners look boxy);
     - the shadow is an offset, box-blurred mask;
     - drawn in the order shadow → outline → fill → background.
     - `fonts::list()` scans `C:\Windows\Fonts` and the per-user fonts folder. Names come from file stems (shortcut: parse the name table with owned_ttf_parser later).
     - A missing font falls back to egui's bundled font.
     - Presets: Default, Lower Third, Title, Subtitle.
5. **Color grading maths:**
   - `milan.color_grading`, applied in this order:
     1. input LUT
     2. offset
     3. lift
     4. gain
     5. gamma
     6. curves (monotone Fritsch-Carlson cubic)
     7. saturation, temperature and tint matrix
     8. look LUT
   - Steps 2-6 compile into one `ColorOp`.
   - `lut::load` parses `.cube` (TITLE, 1D/3D size, DOMAIN, comments), cached by path and mtime. An error passes the image through and reports the error.
   - `export_cube(fx, path, 33)`.
6. **Thumbnails:** `thumbs::preset_thumb(kind, info, preset, p) -> 100×60` on a procedural sample image.

**Test plan**
- **registry:**
  - ids are unique;
  - every preset key exists in its plug-in's params;
  - every default lies within [min, max];
  - `eval` with no params gives the defaults;
  - an unknown id gives None;
  - keyframed params evaluate;
  - a user preset round-trips.
- **vfx:**
  - identity parameters are a no-op (bit-exact for the LUT effects);
  - Invert twice is the identity;
  - Black and White at blend 1 gives r = g = b;
  - blur with radius 0 is the identity, a constant image stays constant, and energy is kept within 1%;
  - Crop zeroes the outside;
  - Mirror is symmetric about its position;
  - a chroma key of the exact key colour gives alpha 0, and a far colour gives 255;
  - `color_op` on a half-transparent pixel is within 1 LSB of an f64 reference;
  - striped output equals single-threaded output.
- **transitions:**
  - p = 0 is exactly A and p = 1 is exactly B, for every transition;
  - dissolve at 0.5 matches the closed form;
  - fade_to_color at 0.5 is solid C;
  - wipe pixels are A, B or a blend inside the feather band;
  - the plan builds a Mix with a transparent A for a lone fade-in.
- **generators:**
  - Solid fills every pixel;
  - a gradient's ends equal its stops;
  - the checkerboard alternates at the cell size;
  - noise is deterministic per seed and in range, and the animated version differs between times.
- **text:**
  - "A" gives coverage only near the anchor;
  - outline 0 equals fill-only;
  - the shadow centroid moves by (dx, dy) within 1 px;
  - left, centre and right alignment put the box at the expected x;
  - an unknown font falls back;
  - multiline text gives separate line bands;
  - empty text gives a transparent frame.
- **grading:**
  - defaults are the identity (bit-exact);
  - gain 2 doubles mid-grey;
  - lift 0.1 moves black to 0.1 and keeps white at 1;
  - gamma follows the closed form;
  - identity `.cube` at sizes 2 and 17 is within 1 LSB;
  - a DOMAIN, comments and a 1D size parse;
  - garbage gives Err with a line number;
  - `export_cube` at defaults reads back as the identity;
  - the monotone curve passes through its points.
- **thumbs:** every catalogue entry renders 100×60 without panicking, and a transition at p = 0 differs from p = 1.

---

### Batch 15: Video plug-in UI (F3, F4, F7, F8 UI, F12 panel)

**Scope**
- The real Video FX, Transitions and Media Generators choosers, with rendered thumbnails and drag-to-apply.
- The FX window.
- Transition drop and cut-to-overlap conversion.
- Inserting generated media.
- The Color Grading panel.

**Files owned**
- `src/edit.rs`: `FxTarget`, `fx_chain_mut`, `add_fx`, `remove_fx`, `move_fx`, `set_fx_enabled`, `set_transition`, `remove_transition`, `insert_generated`
- `src/settings.rs`: `fx_favorites`, `cut_overlap` (default 1.0)
- `src/ui/fx_window.rs` (new), `src/ui/color_grading.rs` (new)
- **UI-shared:**
  - `src/ui/choosers.rs`, `src/ui/preview.rs` (drop target, Video Output FX button)
  - `src/ui/timeline/mod.rs`, `events.rs`, `tracks.rs`, `ctx.rs`
  - `src/ui/dialogs/prefs.rs` (one Cut-to-overlap row)
  - `src/ui/keymap.rs`, `src/ui/menus.rs`, `src/ui/panels.rs`
  - `src/app.rs`, `src/app/commands.rs`

**Design**

1. **Choosers.**
   - Built from `catalog(kind)`, its presets and the user presets.
   - Description strip: `"{name}: {desc}, Grouping: Milan\{group}, Version 1.0"`. The false OFX claim is removed.
   - Thumbnails are built lazily, only for visible tiles. Transitions animate at 15 fps on hover.
   - Tile drag payload: `PluginDrag { id, preset: PresetRef }`.
   - Drop targets:
     - an event → `add_fx(Event)`;
     - a video track header → `Track`;
     - the Video Preview → `VideoOutput`.
     - Each drop is one history step and then opens the FX window.
   - Double-clicking a tile applies it to the selected event.
   - VP23 favourites are stored in `prefs.fx_favorites`.
2. **FX window** (one window, `Option<FxWindow>` on `MeridianApp`):
   - The target is resolved every frame; if it is gone, the window closes.
   - Chain pills: bypass checkbox, click to select, drag to reorder, fx+ (chooser popup), fx×.
   - Preset row: built-in and user presets, a save floppy, and a delete X (user presets only).
   - Parameter grid from `ParamSpec`, grouped by section:
     - Float: DragValue + slider
     - Color: swatch
     - Choice: ComboBox
     - Bool: checkbox
     - Text: TextEdit
     - Font: ComboBox
     - Each row shows a changed asterisk and resets to the default on double-click.
   - Animate per plug-in: an edit sets a key at the cursor; without Animate it sets a single key at t = 0.
   - The keyframe controller is the batch-10 widget, with one lane per plug-in.
   - Undo: one push per gesture (snapshot on drag start or focus gain, push on release or focus loss if anything changed).
   - The event fx chip is clickable and shows an accent dot when FX are present.
3. **Transitions.**
   - `set_transition(p, track, at, tr, cut_overlap, auto_xfade)`:
     - **on an overlap:** just set the field;
     - **on a cut, with auto crossfade on:** extend each side by `cut_overlap / 2` within its media limit (the other side takes the whole overlap if one has no handle), set the fades, then set the transition;
     - **on a lone start edge:** set the transition, and `fade_in = cut_overlap` if it was 0;
     - locked events or tracks return false.
   - A drop on a fade-out with nothing after it shows a hint instead.
   - The overlap is painted #BF7FD3 with the transition name.
   - ctx Remove Transition.
   - The FX window's `Transition` target edits `Transition.params` (no keyframe lane).
4. **Generators.**
   - `insert_generated(p, track, at, fx, name, len = 5 s)` adds a MediaItem and a clip as one step. Inserting with no video track creates one.
   - A double-click inserts on the selected video track; a drag drops at the pointer time.
   - The Generated Media chip and Project Media > Properties open `FxTarget::Media`.
5. **Color Grading panel** (replaces the placeholder; VP23 only, while VP16 users get the same effect from the Video FX tab):
   - Target combo: event, track or Video Output. The panel finds or creates the first `milan.color_grading` effect in that chain.
   - Lift, gamma, gain and offset wheels with a puck, a Y slider, R/G/B/Y DragValues, a reset button and a `*` when changed.
   - Curves editor with master, R, G and B.
   - LUT rows with Browse, Clear and the error text.
   - Export LUT… and Reset All.
   - One history step per drag.

**Enables:**
- VideoEventFx, TrackFx, VideoOutputFx (new commands)
- Tools > Video > Video Event FX / Track FX / Video Output FX
- the event ctx Video Event FX
- the preview's Video Output FX button
- the VP23 Track Fx header button (video tracks)

**Test plan**
- **edit:**
  - `add_fx` on a locked track or event is refused and changes nothing;
  - `add_fx` then `remove_fx` restores the project;
  - `move_fx` reorders;
  - `fx_chain_mut(VideoOutput)` returns `master.video_fx`;
  - `set_transition` with 5 s handles gives a 1.0 s centred overlap with matching fades;
  - with no outgoing handle, the incoming event takes the full 1.0 s;
  - with auto crossfade off at a cut, it returns false;
  - on an overlap, only the field changes;
  - a locked track returns false;
  - one undo restores positions, trims and fades;
  - `insert_generated` adds one item and one clip, and one undo removes both.
- **window:**
  - a fake parameter gesture pushes exactly one history step;
  - the window closes when its event is deleted.
- **Manual (user):**
  - open the FX window from all entry points and confirm undo restores parameters;
  - drag a preset onto an event and see it in the preview and in a render.

---

### Batch 16: Audio FX

**Scope**
- The DSP library (`afx.rs`).
- Chains in the mixer and the render, for events, tracks and the master.
- The FX window's audio targets.
- Master FX.
- Bypass All Audio FX.
- Default track chain and Set Default Track Properties.

**Files owned**
- `src/engine/afx.rs` (new), `src/engine/plugins.rs` (entries), `src/engine/mix.rs`, `src/engine/plan.rs`
- `src/project.rs` (new audio tracks get the default chain)
- `src/settings.rs` (`default_track_fx`)
- **UI-shared:**
  - `src/ui/fx_window.rs` (audio targets), `src/ui/master_bus.rs` (fx icon)
  - `src/ui/timeline/tracks.rs` (VP16 FX button, VP23 Fx button), `ctx.rs`, `events.rs`
  - `src/ui/menus.rs`, `src/app.rs`, `src/app/commands.rs`

**Design**
- **DSP interface.**
  - `trait AudioFx: Send { fn process(&mut self, p: &[f32], buf: &mut [f32]); fn reset(&mut self); fn neutral(&self, p: &[f32]) -> bool }`.
  - `Chain { slots: Vec<(fx_id, plugin_id, Box<dyn AudioFx>)> }`:
    - `sync(&[Fx])` keeps the state of slots whose id and plugin still match;
    - `run(fx, t, buf)` skips disabled, unknown and neutral slots.
- **Plug-ins:**
  - Track Noise Gate (threshold default −96, which is off).
  - Track EQ: 4 RBJ biquad bands, TDF2 with f64 state; band 1 is low shelf or HP, bands 2-3 are peaks, band 4 is high shelf or LP.
  - Track Compressor (ratio default 1, which is neutral; feed-forward, stereo-linked).
  - Reverb (Freeverb).
  - Simple Delay (5 s ring buffer).
  - Limiter (1.5 ms lookahead, brickwall).
  - Coefficients are recomputed only when values change. Tiny recursive states are flushed to 0 to avoid denormals.
  - shortcut: parameters are read once per 1024-frame block; add per-sample smoothing if zipper noise is heard.
- **Default chain.**
  - New audio tracks get gate + EQ + compressor at neutral settings (`default_track_chain()`, or `prefs.default_track_fx` with regenerated ids).
  - Old projects keep empty chains.
- **Mixer** (as in 1.3): event FX are cut at the event end, while track FX run past it so tails continue.
- **Bypass All Audio FX.**
  - `MeridianApp.bypass_audio_fx` → `AudioPlan.bypass_fx`.
  - Playback only; the render forces it off (not verified against VEGAS).
- **Render.** Master FX run before master gain, using the same `Mixer`.

**Enables:** Options > Bypass All Audio FX, the audio event ctx Audio Event FX..., the track header Set Default Track Properties, the master strip fx icon, and the track Fx buttons on audio tracks.

**Test plan**
- **EQ:**
  - at 0 dB it is neutral and bit-exact;
  - a +6 dB peak at 1 kHz measures +6 ±0.1 dB;
  - the low-shelf gain at DC equals its gain;
  - a 100 Hz high-pass gives −3 ±0.2 dB at 100 Hz.
- **Compressor:** −10 dBFS in, threshold −20, ratio 4 gives −17.5 ±0.3 dB; ratio 1 is neutral.
- **Gate:** a signal below the threshold is ≤ −60 dB after the release.
- **Delay:** an impulse reappears at the delay, and feedback decays geometrically.
- **Reverb:** output stays finite, and the tail decays after the input stops.
- **Limiter:** a +12 dB sine never exceeds the ceiling.
- **Chain:** `sync` keeps state for the same id and creates fresh state for a new id; disabled slots pass the signal through.
- **Mixer:**
  - event FX run before the fade;
  - a track reverb tail continues after the event, while an event reverb tail is cut;
  - `bypass_fx` equals a plan with no FX;
  - two mixers on the same plan give identical blocks;
  - 2 × 1024 equals 2048 with FX.
- **Master:**
  - a limiter at −1 dB in a render never exceeds −1 dBFS;
  - master gain 0.5 with neutral FX gives mix × 0.5;
  - an empty master chain is bit-identical to batch 3.
- **Data:** a new audio track gets the default chain; an old project has an empty chain; `default_track_fx` round-trips.

---

### Batch 17: Audio recording

**Scope**
- Arm audio tracks and record with Ctrl+R into a WAV file.
- The recording is placed as events at the record start.
- A Recorded Files dialog.
- Input device and latency offset preferences.
- A live input meter while armed.

**Files owned**
- `src/record.rs` (new), `src/main.rs`
- `src/project.rs` (`Track.armed` with `serde(skip)`, `Timeline.record_dir`)
- `src/edit.rs` (`place_recording`)
- `src/settings.rs` (`record_input`, `record_offset_ms`)
- **UI-shared:** `src/app.rs`, `src/app/commands.rs`, `src/ui/menus.rs`, `src/ui/transport.rs`, `src/ui/timeline/tracks.rs`, `ctx.rs`, `src/ui/dialogs/prefs.rs` (Audio Device page), `src/ui/dialogs/mod.rs`, `src/ui/status.rs`

**Design**
- Capture goes through `rodio::cpal`, so there is no new crate:
  - the "milan-rec" device thread converts F32, I16 and U16 samples to f32;
  - it blocks on `stop_rx`;
  - the callback uses `try_send` into `sync_channel(64)` and counts drops;
  - the error callback keeps the file written so far.
- The writer thread writes an IEEE-float WAV and patches the RIFF and data sizes every second, so a crash leaves a readable file.
- File name: `<record_dir or project folder>\<project> <track> Rec NNN.wav`, never overwriting. An unsaved project prompts for a folder on the first arm.
- Starting: the recorder starts, then playback from the cursor with looping off.
- Stopping: `place_recording` adds one MediaItem and one clip per armed, unlocked track, at start − offset, as one history step.
- Recorded Files dialog: Delete (asks first, then removes through history and deletes the file), Rename, Done.
- Shortcuts:
  - one input device;
  - no takes;
  - no input monitoring;
  - no punch-in;
  - the latency offset is manual.

**Enables:** Record (Ctrl+R), Arm (Ctrl+Alt+R), the transport Record button, the audio header Arm for Record, and Preferences > Audio Device.

**Test plan**
- **WavWriter:**
  - the header bytes for 2 ch / 48 kHz f32 are correct;
  - the patched sizes are correct after 1000 frames;
  - samples round-trip.
- **Conversion:** i16 −32768 → −1.0 and u16 32768 → 0.
- **place_recording:**
  - clips only on armed, unlocked tracks;
  - exactly one history push;
  - media duration = frames / rate;
  - the offset is applied;
  - returns false with no armed tracks.
- **Naming:** never overwrites.
- **Serde:** `Track.armed` is not saved; an old project loads with `record_dir` None.
- **Manual (user):** record 10 s over playback and check the alignment against a click track.

---

### Batch 18: Updater polish

**Scope**
- Clean up old downloads.
- Make the installer wait for Milan to exit.
- Keep the installer launch out of the Job Object.

**Files owned**
- `src/updater.rs`, `src/main.rs` (named mutex "MilanProRunning" through windows-sys)
- `installer/milan-pro.iss` (`[Code] InitializeSetup` polls `CheckForMutexes` for up to 20 s; do not use the AppMutex directive, which aborts silent setup)
- `src/app.rs` (one call to `clean_downloads` at startup; **UI-shared**, 1 line)

**Design**
- `clean_downloads(dir)` removes `*.part` files and `MilanPro-*-Setup.exe` files whose version is ≤ the current one.
- `launch_installer` stays a plain `Command` and never uses `proc::spawn`, because KILL_ON_JOB_CLOSE would kill the installer as Milan exits. Add a comment saying so.

**Test plan**
- The existing updater tests stay.
- `clean_downloads` in a temp dir removes older versions and `.part` files, and keeps newer versions and unrelated files.
- A test that `launch_installer` does not go through `proc`.
- **Manual (user):** with an all-users install, a silent update shows UAC.

---

## 3. Deferred items and reasons

| Item | Reason |
|---|---|
| Velocity envelope (variable speed, freeze frame) | Needs a varispeed resampler in the mixer, backwards decoding, and per-frame integral caching. `EnvKind::Velocity` is reserved in batch 1 so files written later still load. Do it after batch 8. |
| Reverse-video read-back (decode 1 s chunks backwards into SourceCache) | Built in R20 (`engine/vdecode.rs` chunks). |
| cpal output with latency timestamps | rodio is kept to avoid a large rewrite. The cost is a playhead about 10-20 ms ahead of the sound. Revisit if users notice. |
| Waveform peak files on disk; yuv420p pipe transfer | Performance work with no measured need yet. yuv420p only if Full/Best stutters. |
| Pan/Crop Bezier masks, Track Motion 2D Shadow/Glow, 3D, parenting | Each needs extra rasteriser or blur stages. They can reuse the batch-14 blur. Not requested in this pass. |
| Credit Roll and Test Pattern generators | Small, but not requested. Test Pattern is about 30 lines in `generate.rs` and can be added any time. |
| Transition Progress envelope | Needs `Keyframes<f32>` on `Transition`. Progress is linear today. |
| Transition on a fade-out with no following event | Would need `Clip.transition_out`. Add it if users ask. |
| Exact per-pixel HSL; round outline kernel; font name-table parsing | Quality upgrades to batch-14 shortcuts, done when someone notices. |
| Native `.veg` parsing | Undocumented, version-specific GUID chunks with no maintained parser anywhere. XL work with high breakage risk. The batch-13 script reads the same data through VEGAS's supported API. |
| VST3 / CLAP hosting | 2-3k lines of unsafe FFI. In-process plug-ins can crash Milan, so a safe version needs an out-of-process host. The user's VST3 folder is empty. If pursued after batch 16: `vst3` bindings plus libloading, effects only, behind a "may crash" preference. VST2 is not possible (no license). |
| OFX hosting | 4-6k lines of C ABI, with no Rust host crate. VEGAS's own OFX bundles are tied to VEGAS's license and host checks, and the user has no third-party OFX installed. `Fx.plugin` ids stay open to an `ofx:` prefix. |
| Not requested, skipped | Ctrl+drag copy; right-drag Move/Copy Here; Shuffle, Split Trim and Zoom tools; Fade to Color envelope; event Loop switch; Slip All Takes; keyboard nudge (egui cannot tell numpad keys apart); Synchronize/Sync Link; Paste Event Attributes; floating dock windows; Organize Layouts; per-track record inputs; punch-in; input monitoring; plug-in presets for audio FX; an EQ curve graph. |
| GPU compositing | `compose()` is frame-in, frame-out, so a GPU backend can replace it if 4K Best with heavy FX drops frames. |

**VEGAS behaviours to confirm in VEGAS before or while building:**
- The video crossfade dissolve rule (decision 0.9).
- An Auto Ripple trim of an event start.
- Whether the VP16 Selection tool rubber band also sets the time selection.
- Whether Insert Track goes above or below the focused track.
- The names Fade to Color, Flash and Additive Dissolve.
- Whether new Pan/Crop and Track Motion keys default to Smooth.
- Whether Track FX run before or after Track Motion.
- Whether Bypass All Audio FX affects render.
- The script API property names.
- Whether ffmpeg can decode `.sfvp0` proxies.
