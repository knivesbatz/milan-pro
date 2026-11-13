//! Timeline editing math: trim, snap, split, delete, move, paste, tracks, media drop.
use std::collections::{HashMap, HashSet};
use serde::{Deserialize, Serialize};
use crate::project::*;

/// Source length a clip may not exceed; None = unlimited (images, generated, unknown or unprobed media). A looping
/// event's is infinite: it may run on past its media's end (which repeats), while its in point stays at or after 0.
pub fn media_limit(project: &Project, clip: &Clip) -> Option<f64> {
    let m = media_of(project, clip)?;
    if loop_len(clip, m).is_some() {
        return Some(f64::INFINITY);
    }
    (m.generator.is_none() && matches!(m.media_type, MediaType::Video | MediaType::Audio))
        .then_some(m.duration)
        .filter(|d| *d > 0.0)
}

/// The event's active take's media.
pub fn media_of<'a>(project: &'a Project, clip: &Clip) -> Option<&'a MediaItem> {
    let id = clip.media_id.as_deref()?;
    project.media_pool.iter().find(|m| m.id == id)
}

/// Media shorter than this does not loop.
// shortcut: each pass is a decoder start, so very short media holds its last frame and is silent past its end (as
// with Loop off); loop it from a cached buffer if users ask.
pub const MIN_LOOP: f64 = 0.05;

/// The length of media `m` that event `c` repeats past its end (Switches › Loop; VEGAS repeats the whole file, or the
/// whole subclip); None when the event does not loop or the media has no length (stills, generated media).
pub fn loop_len(c: &Clip, m: &MediaItem) -> Option<f64> {
    let timed = m.generator.is_none() && matches!(m.media_type, MediaType::Video | MediaType::Audio);
    (c.looping && timed && m.duration >= MIN_LOOP).then_some(m.duration)
}

/// Source time `s` of an event looping over media `len` seconds long, wrapped into [0, len).
pub fn loop_time(s: f64, len: f64) -> f64 {
    // A time a hair under a wrap (float noise in a frame time) is on it.
    (s - len * ((s + 1e-6) / len).floor()).max(0.0)
}

/// An event's passes through its media, in timeline order: (position, length, in point) of each stretch that plays the
/// media once without wrapping. Without `len` (no loop) the event is one pass. The loop notches sit where passes meet.
pub fn loop_passes(c: &Clip, len: Option<f64>) -> Vec<(f64, f64, f64)> {
    let Some(len) = len.filter(|l| *l > 0.0 && c.rate > 0.0) else { return vec![(c.position, c.duration, c.in_point)] };
    // The source span the event covers, unwrapped, cut at each multiple of the media length.
    let (u0, u1) = (c.in_point, c.in_point + c.duration * c.rate);
    let mut v = Vec::new();
    let mut u = u0;
    while u < u1 - 1e-9 {
        let k = ((u + 1e-6) / len).floor();
        let end = ((k + 1.0) * len).min(u1);
        // Reversed, the source runs from the event's end back to its start.
        let pos = c.position + if c.reverse { u1 - end } else { u - u0 } / c.rate;
        v.push((pos, (end - u) / c.rate, (u - k * len).max(0.0)));
        u = end;
    }
    if v.len() <= 1 {
        // No wrap inside: the event exactly (its in point wrapped, after a slip past the end).
        return vec![(c.position, c.duration, v.first().map_or(c.in_point, |p| p.2))];
    }
    if c.reverse {
        v.reverse();
    }
    v
}

/// Source time shown at timeline time `t` (shared with the engine plan). Forward: in + (t - pos) x rate; reversed:
/// in + (pos + dur - t) x rate. Either way the event covers the source span [in, in + dur x rate]. With a Velocity
/// envelope the media played so far is its integral (`engine::velocity`), times the rate; reversed, mirrored.
pub fn src_time(c: &Clip, t: f64) -> f64 {
    use crate::engine::velocity;
    match velocity::of(c) {
        None => c.in_point + if c.reverse { c.position + c.duration - t } else { t - c.position } * c.rate,
        Some(v) => {
            let s = velocity::integral(v, t - c.position);
            c.in_point + c.rate * if c.reverse { velocity::integral(v, c.duration) - s } else { s }
        }
    }
}

/// `out_point` is the end of the source span the event covers.
fn sync_out(c: &mut Clip) {
    c.out_point = c.in_point + c.duration * c.rate;
}

fn keep_fades(clip: &mut Clip, orig: &Clip) {
    (clip.fade_in, clip.fade_out) = (orig.fade_in, orig.fade_out);
    clip.clamp_fades();
    sync_out(clip);
}

/// Move the clip's left edge to `new_start`, keeping its right edge fixed (and the frame shown there).
pub fn trim_left(clip: &mut Clip, orig: &Clip, new_start: f64, frame: f64, limit: Option<f64>) {
    trim_left_edge(clip, orig, new_start, frame, limit, false);
}

/// Alt+drag of the left edge (slip-trim): the right edge stays put and the frame at the dragged edge moves with it.
pub fn slip_trim_left(clip: &mut Clip, orig: &Clip, new_start: f64, frame: f64, limit: Option<f64>) {
    trim_left_edge(clip, orig, new_start, frame, limit, true);
}

fn trim_left_edge(clip: &mut Clip, orig: &Clip, new_start: f64, frame: f64, limit: Option<f64>, slip: bool) {
    let end = orig.position + orig.duration;
    // in_point follows the edge when the left edge shows it moving: a forward trim or a reversed slip-trim.
    let shift = orig.reverse == slip;
    // Limited media can't reveal frames outside [0, limit]; images may extend freely.
    let room = limit.map(|l| if shift { orig.in_point } else { l - orig.in_point - orig.duration * orig.rate } / orig.rate);
    let min_start = room.map_or(0.0, |r| (orig.position - r.max(0.0)).max(0.0));
    let start = new_start.min(end - frame).max(min_start);
    clip.position = start;
    clip.duration = end - start;
    // A forward trim keeps the frame at each point of the timeline, also through a Velocity envelope.
    // shortcut: other edits of an event with a Velocity envelope (reversed trims, slips, stretches) count its
    // span at the plain rate; integrate them too if users ask.
    clip.in_point = match shift {
        true if !orig.reverse => src_time(orig, start).max(0.0),
        true => (orig.in_point + (start - orig.position) * orig.rate).max(0.0),
        false => orig.in_point,
    };
    // Inactive takes move as a plain trim moves the media, also under a slip-trim (only the active take slips).
    shift_takes(clip, orig, if orig.reverse { 0.0 } else { (start - orig.position) * orig.rate });
    rebase_keys(clip, orig, orig.position - start);
    keep_fades(clip, orig);
}

/// Inactive takes stay lined up with the active one: their in points move by `d` as the event's does in a trim, split
/// or slide (a slip moves the active take alone, VEGAS's default with Slip All Takes off).
fn shift_takes(clip: &mut Clip, orig: &Clip, d: f64) {
    for (t, o) in clip.takes.iter_mut().zip(&orig.takes) {
        t.in_point = (o.in_point + d).max(0.0);
    }
}

/// Move the clip's right edge to `new_end`, keeping the frame at its left edge.
pub fn trim_right(clip: &mut Clip, orig: &Clip, new_end: f64, frame: f64, limit: Option<f64>) {
    trim_right_edge(clip, orig, new_end, frame, limit, false);
}

/// Alt+drag of the right edge (slip-trim): the left edge stays put and the frame at the dragged edge moves with it.
pub fn slip_trim_right(clip: &mut Clip, orig: &Clip, new_end: f64, frame: f64, limit: Option<f64>) {
    trim_right_edge(clip, orig, new_end, frame, limit, true);
}

fn trim_right_edge(clip: &mut Clip, orig: &Clip, new_end: f64, frame: f64, limit: Option<f64>, slip: bool) {
    // in_point follows the edge when the right edge shows it: a reversed trim or a forward slip-trim.
    let shift = orig.reverse != slip;
    let max_dur = limit.map_or(f64::INFINITY, |l| if shift { orig.duration + orig.in_point / orig.rate } else { (l - orig.in_point) / orig.rate });
    let dur = (new_end - orig.position).min(max_dur).max(frame);
    clip.duration = dur;
    clip.in_point = if shift { (orig.in_point + (orig.duration - dur) * orig.rate).max(0.0) } else { orig.in_point };
    shift_takes(clip, orig, if orig.reverse { (orig.duration - dur) * orig.rate } else { 0.0 });
    keep_fades(clip, orig);
}

/// Highest in_point that keeps the event inside its media.
fn max_in(orig: &Clip, limit: Option<f64>) -> f64 {
    limit.map_or(f64::INFINITY, |l| (l - orig.duration * orig.rate).max(0.0))
}

/// Alt+drag inside an event (slip): the media moves `d` seconds under the event, which stays put.
pub fn slip(clip: &mut Clip, orig: &Clip, d: f64, limit: Option<f64>) {
    let dir = if orig.reverse { 1.0 } else { -1.0 };
    clip.in_point = (orig.in_point + dir * d * orig.rate).min(max_in(orig, limit)).max(0.0);
    keep_fades(clip, orig);
}

/// Ctrl+Alt+drag inside an event (slide): the event moves `d` seconds while its media stays where it was on the
/// timeline, within the media and not before 0.
pub fn slide(clip: &mut Clip, orig: &Clip, d: f64, limit: Option<f64>) {
    let dir = if orig.reverse { -1.0 } else { 1.0 };
    // Range of dir x d that keeps in_point in [0, max_in]; images are not limited.
    let (lo, hi) = match limit {
        Some(_) => (-orig.in_point / orig.rate, (max_in(orig, limit) - orig.in_point) / orig.rate),
        None => (f64::NEG_INFINITY, f64::INFINITY),
    };
    let d = (dir * (dir * d).min(hi).max(lo)).max(-orig.position);
    clip.position = orig.position + d;
    clip.in_point = (orig.in_point + dir * d * orig.rate).max(0.0);
    shift_takes(clip, orig, clip.in_point - orig.in_point);
    keep_fades(clip, orig);
}

/// Playback rate limits of a time stretch (VEGAS's 0.25x to 4x).
pub const MIN_RATE: f64 = 0.25;
pub const MAX_RATE: f64 = 4.0;

/// Ctrl+drag of an edge (time stretch): the other edge stays put and the same source span plays at a new rate, so
/// rate x duration is constant.
pub fn stretch(clip: &mut Clip, orig: &Clip, new_edge: f64, left: bool, frame: f64) {
    let end = orig.position + orig.duration;
    let want = if left { (end - new_edge).min(end) } else { new_edge - orig.position }.max(frame);
    let span = orig.duration * orig.rate;
    clip.rate = (span / want).clamp(MIN_RATE, MAX_RATE);
    clip.duration = span / clip.rate;
    clip.position = if left { end - clip.duration } else { orig.position };
    clip.in_point = orig.in_point;
    keep_fades(clip, orig);
}

/// Ctrl+Alt+drag of the edit point between `a` and `b`, rolled so b starts at `start`: a's end and b's start move
/// together, each within its own media. An edit point within a frame closes up; a crossfade (b starting more than a
/// frame before a ends) keeps its length, so the crossfade slides.
#[allow(clippy::too_many_arguments)]
pub fn roll(a: &mut Clip, oa: &Clip, la: Option<f64>, b: &mut Clip, ob: &Clip, lb: Option<f64>, start: f64, frame: f64) {
    let ov = Some(oa.position + oa.duration - ob.position).filter(|o| *o > frame + 1e-9).unwrap_or(0.0);
    trim_left(b, ob, start, frame, lb);
    trim_right(a, oa, b.position + ov, frame, la);
    let e = a.position + a.duration - ov;
    if b.position != e {
        trim_left(b, ob, e, frame, lb);
        trim_right(a, oa, b.position + ov, frame, la);
    }
}

/// The other side of a roll at `id`'s right (or left) edge, on its track: the event whose opposite edge meets it
/// within `tol`, or else, at the left edge, the earlier event that crossfades into it (ends inside it).
pub fn adjacent(project: &Project, id: &str, right: bool, tol: f64) -> Option<String> {
    let r = project.find_clip(id)?;
    let clips = &project.timeline.tracks(r.is_video)[r.track_idx].clips;
    let c = &clips[r.clip_idx];
    let e = if right { c.position + c.duration } else { c.position };
    let other = |k: &Clip| if right { k.position } else { k.position + k.duration };
    let fades_in = |k: &Clip| !right && k.position < c.position && other(k) > e + tol + 1e-9 && other(k) < c.position + c.duration;
    let meets = clips.iter().find(|k| k.id != id && (other(k) - e).abs() <= tol + 1e-9);
    meets.or_else(|| clips.iter().find(|k| k.id != id && fades_in(k))).map(|k| k.id.clone())
}

/// The event's level handle as gain in dB: `frac` is the handle's height (1 at the top, 0 at the bottom), as an
/// amplitude; 0 dB at most, -120 dB (silence) at the bottom.
pub fn level_db(frac: f32) -> f32 {
    (20.0 * frac.log10()).clamp(-120.0, 0.0)
}

/// Reverse the unlocked events `ids` (each flips; the source span stays the same).
pub fn reverse_clips(project: &mut Project, ids: &[String]) -> bool {
    let mut done = false;
    for c in tracks_mut(&mut project.timeline).filter(|t| !t.locked).flat_map(|t| &mut t.clips).filter(|c| !c.locked && ids.contains(&c.id)) {
        c.reverse = !c.reverse;
        done = true;
    }
    changed(project, done)
}

/// Switches › Mute: events `ids` all take the opposite of the focus (last) event's state.
pub fn mute_clips(project: &mut Project, ids: &[String]) -> bool {
    switch_clips(project, ids, |c| &mut c.muted)
}

/// Switches › Loop, the same way.
pub fn loop_clips(project: &mut Project, ids: &[String]) -> bool {
    switch_clips(project, ids, |c| &mut c.looping)
}

/// The R3b event switches of Edit › Switches and the event menus; each acts on events of its kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Switch {
    Invert,
    Normalize,
    AutoNormalize,
    KeepAspect,
    ReduceFlicker,
}

impl Switch {
    /// A video event switch (else an audio one).
    pub fn video(self) -> bool {
        matches!(self, Switch::KeepAspect | Switch::ReduceFlicker)
    }

    pub fn get(self, c: &Clip) -> bool {
        match self {
            Switch::Invert => c.invert,
            Switch::Normalize => c.normalize,
            Switch::AutoNormalize => c.auto_normalize,
            Switch::KeepAspect => c.keep_aspect,
            Switch::ReduceFlicker => c.reduce_flicker,
        }
    }

    pub fn of(self, c: &mut Clip) -> &mut bool {
        match self {
            Switch::Invert => &mut c.invert,
            Switch::Normalize => &mut c.normalize,
            Switch::AutoNormalize => &mut c.auto_normalize,
            Switch::KeepAspect => &mut c.keep_aspect,
            Switch::ReduceFlicker => &mut c.reduce_flicker,
        }
    }
}

/// The unlocked events `ids` of one kind on unlocked tracks, in timeline order, and the focus one's index among them:
/// the last listed of the kind, or (that one locked) the last of them.
fn events_of<'a>(project: &'a mut Project, ids: &[String], video: bool) -> (Vec<&'a mut Clip>, Option<usize>) {
    let focus = ids.iter().rev().find(|id| project.find_clip(id).is_some_and(|r| r.is_video == video)).cloned();
    let v: Vec<&mut Clip> = project.timeline.tracks_mut(video).iter_mut().filter(|t| !t.locked).flat_map(|t| &mut t.clips).filter(|c| !c.locked && ids.contains(&c.id)).collect();
    let at = focus.and_then(|f| v.iter().position(|c| c.id == f)).or(v.len().checked_sub(1));
    (v, at)
}

/// Edit › Switches (R3b): the events of the switch's kind take the opposite of the focus event's state. Turning
/// Normalize or Auto Normalize on measures the peak again.
pub fn set_switch(project: &mut Project, ids: &[String], s: Switch) -> bool {
    let (mut v, Some(at)) = events_of(project, ids, s.video()) else { return false };
    let on = !s.get(v[at]);
    for c in &mut v {
        *s.of(c) = on;
        if on && matches!(s, Switch::Normalize | Switch::AutoNormalize) {
            c.normalized = None;
        }
    }
    changed(project, true)
}

/// Switches › resample radios on the video events `ids`.
pub fn set_resample(project: &mut Project, ids: &[String], r: Option<Resample>) -> bool {
    let (mut v, _) = events_of(project, ids, true);
    let done = v.iter().any(|c| c.resample != r);
    v.iter_mut().for_each(|c| c.resample = r);
    changed(project, done)
}

/// Channels ▸ on the audio events `ids`.
pub fn set_channels(project: &mut Project, ids: &[String], ch: ChannelMap) -> bool {
    let (mut v, _) = events_of(project, ids, false);
    let done = v.iter().any(|c| c.channels != ch);
    v.iter_mut().for_each(|c| c.channels = ch);
    changed(project, done)
}

/// Stream ▸: stream `n` of their media for the events `ids` of one kind that have it.
pub fn set_stream(project: &mut Project, ids: &[String], video: bool, n: u32) -> bool {
    let counts: HashMap<String, u32> = project.media_pool.iter().map(|m| (m.id.clone(), m.streams(video))).collect();
    let (mut v, _) = events_of(project, ids, video);
    let mut done = false;
    for c in v.iter_mut().filter(|c| c.stream != n && c.media_id.as_ref().and_then(|m| counts.get(m)).is_some_and(|&k| n < k)) {
        c.stream = n;
        done = true;
    }
    changed(project, done)
}

/// Preferences › Audio "Normalize peak level" (VEGAS's default).
pub const NORMALIZE_PEAK_DB: f32 = -0.1;

/// The Normalize gain (dB) that takes a measured `peak` (linear) to `NORMALIZE_PEAK_DB`; silence gets none.
pub fn normalize_db(peak: f32) -> f32 {
    if peak > 1e-6 { NORMALIZE_PEAK_DB - 20.0 * peak.log10() } else { 0.0 }
}

/// The audio event `c` plays, as its Normalize gain is measured on.
pub fn norm_span(c: &Clip) -> NormSpan {
    NormSpan { media: c.media_id.clone(), stream: c.stream, channels: c.channels, in_point: c.in_point, duration: c.duration, rate: c.rate }
}

/// Normalize or Auto Normalize is on and the gain is not measured yet, or (Auto Normalize) was measured on audio the
/// event no longer plays. Plain Normalize keeps its gain through trims, as in VEGAS (Event Properties › Recalculate).
pub fn needs_normalize(c: &Clip) -> bool {
    (c.normalize || c.auto_normalize)
        && c.media_id.is_some()
        && c.normalized.as_ref().is_none_or(|n| c.auto_normalize && n.span != norm_span(c))
}

/// The Normalize gain the event plays with (dB), 0 when Normalize is off or not measured yet.
pub fn normalize_gain(c: &Clip) -> f32 {
    c.normalized.as_ref().filter(|_| c.normalize || c.auto_normalize).map_or(0.0, |n| n.db)
}

fn switch_clips(project: &mut Project, ids: &[String], switch: impl Fn(&mut Clip) -> &mut bool) -> bool {
    let Some(on) = ids.last().and_then(|id| project.clip(id)).cloned().map(|mut c| !*switch(&mut c)) else { return false };
    for c in tracks_mut(&mut project.timeline).flat_map(|t| &mut t.clips).filter(|c| ids.contains(&c.id)) {
        *switch(c) = on;
    }
    changed(project, true)
}

/// Nearest target within `threshold` of `t`.
pub fn snap(t: f64, targets: &[f64], threshold: f64) -> Option<f64> {
    targets
        .iter()
        .copied()
        .filter(|x| (x - t).abs() <= threshold)
        .min_by(|a, b| (a - t).abs().total_cmp(&(b - t).abs()))
}

/// What a drag snaps to: `points`, and with Snap to Grid the grid lines `step` seconds apart through `phase`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snaps {
    pub points: Vec<f64>,
    /// (step, phase)
    pub grid: Option<(f64, f64)>,
}

impl Snaps {
    /// The grid line nearest `t`.
    fn line(&self, t: f64) -> Option<f64> {
        self.grid.filter(|(step, _)| *step > 0.0).map(|(step, ph)| ((t - ph) / step).round() * step + ph).filter(|l| *l >= -1e-9)
    }

    /// Nearest point or grid line within `threshold` of `t`.
    pub fn nearest(&self, t: f64, threshold: f64) -> Option<f64> {
        let line = self.line(t).filter(|l| (l - t).abs() <= threshold);
        match (snap(t, &self.points, threshold), line) {
            (Some(p), Some(l)) => Some(if (l - t).abs() < (p - t).abs() { l } else { p }),
            (p, l) => p.or(l),
        }
    }

    /// Snap a span [start, start+len] by whichever edge is closest to a target.
    pub fn span(&self, start: f64, len: f64, threshold: f64) -> f64 {
        let a = self.nearest(start, threshold).map(|s| s - start);
        let b = self.nearest(start + len, threshold).map(|s| s - start - len);
        start + [a, b].into_iter().flatten().min_by(|x, y| x.abs().total_cmp(&y.abs())).unwrap_or(0.0)
    }

    /// `v` lies on a point or a grid line.
    pub fn hits(&self, v: f64) -> bool {
        self.points.iter().any(|x| (x - v).abs() < 1e-9) || self.line(v).is_some_and(|l| (l - v).abs() < 1e-9)
    }
}

/// The grid line after `t` (`forward`) or before it: Move Right/Left by Grid Marks. Never before 0.
pub fn grid_step(t: f64, (step, phase): (f64, f64), forward: bool) -> f64 {
    let k = (t - phase) / step;
    // A cursor on a line (to rounding) moves a whole step.
    let k = if forward { (k + 1e-6).floor() + 1.0 } else { (k - 1e-6).ceil() - 1.0 };
    (k * step + phase).max(0.0)
}

/// 0, playhead, markers, loop ends and every clip edge except the clip with `exclude_id`.
pub fn snap_targets(project: &Project, exclude_id: Option<&str>) -> Vec<f64> {
    snap_targets_except(project, exclude_id.as_slice(), None)
}

/// `snap_targets` without the edges of the events in `ids` (the ones being dragged); with `tracks` (Snap to All
/// Events off), only the edges of events on those tracks (by id).
pub fn snap_targets_except(project: &Project, ids: &[&str], tracks: Option<&[String]>) -> Vec<f64> {
    let tl = &project.timeline;
    let mut v = vec![0.0, tl.cursor_position];
    v.extend(tl.markers.iter().map(|m| m.position));
    v.extend(tl.loop_region.iter().flat_map(|&(a, b)| [a, b]));
    // R18a: VEGAS snaps to command markers, CD indexes and CD track regions too.
    v.extend(tl.commands.iter().map(|c| c.position).chain(tl.cd_indexes.iter().copied()).chain(tl.cd_tracks.iter().flat_map(|c| [c.start, c.end])));
    let on = |t: &Track| tracks.is_none_or(|ts| ts.contains(&t.id));
    for c in tl.video_tracks.iter().chain(&tl.audio_tracks).filter(|t| on(t)).flat_map(|t| &t.clips) {
        if !ids.contains(&c.id.as_str()) {
            v.push(c.position);
            v.push(c.position + c.duration);
        }
    }
    v
}

/// Split `clip` at timeline time `at`; None if `at` is not strictly inside it.
pub fn split_clip(clip: &Clip, at: f64) -> Option<(Clip, Clip)> {
    if at <= clip.position || at >= clip.position + clip.duration {
        return None;
    }
    let first_dur = at - clip.position;
    let mut first = clip.clone();
    first.duration = first_dur;
    first.fade_out = 0.0; // each half keeps only its outer fade
    let mut second = clip.clone();
    second.id = uuid::Uuid::new_v4().to_string();
    second.position = at;
    second.duration -= first_dur;
    second.fade_in = 0.0;
    second.transition = None; // the transition belongs to the event's start, the one out to its end
    first.transition_out = None;
    shift_clip_keys(&mut second, -first_dur);
    // The half that plays first in source order starts at in_point; reversed, that is the second half.
    if clip.reverse {
        first.in_point += second.duration * clip.rate;
    } else {
        second.in_point = src_time(clip, at);
    }
    for c in [&mut first, &mut second] {
        shift_takes(c, clip, c.in_point - clip.in_point);
        c.clamp_fades();
        sync_out(c);
    }
    Some((first, second))
}

fn unlocked_track<'a>(project: &'a mut Project, sel: &ClipRef) -> Option<&'a mut Track> {
    project
        .timeline
        .tracks_mut(sel.is_video)
        .get_mut(sel.track_idx)
        .filter(|t| !t.locked && t.clips.get(sel.clip_idx).is_some_and(|c| !c.locked))
}

/// Trim the selected event so it covers only `[start, end]` (either side may be None to leave it).
/// Trim Start / Trim End use the cursor, Trim uses the time selection (§5.10).
pub fn trim_to(project: &mut Project, sel: &ClipRef, start: Option<f64>, end: Option<f64>) -> bool {
    let frame = 1.0 / project.settings.frame_rate.max(1.0);
    let Some(c) = project.timeline.tracks(sel.is_video).get(sel.track_idx).and_then(|t| t.clips.get(sel.clip_idx)) else { return false };
    let (orig, limit) = (c.clone(), media_limit(project, c));
    let (a, b) = (orig.position, orig.position + orig.duration);
    let start = start.filter(|&s| s > a && s < b);
    let end = end.filter(|&e| e > a && e < b && start.is_none_or(|s| e > s));
    if start.is_none() && end.is_none() {
        return false;
    }
    let Some(track) = unlocked_track(project, sel) else { return false };
    let clip = &mut track.clips[sel.clip_idx];
    if let Some(e) = end {
        trim_right(clip, &orig, e, frame, limit);
    }
    if let Some(s) = start {
        let o = clip.clone();
        trim_left(clip, &o, s, frame, limit);
    }
    project.mark_modified();
    true
}

/// Toggle the lock flag of the selected event (allowed on a locked track too, as in VEGAS).
pub fn toggle_clip_lock(project: &mut Project, sel: &ClipRef) -> bool {
    let tracks = project.timeline.tracks_mut(sel.is_video);
    let Some(c) = tracks.get_mut(sel.track_idx).and_then(|t| t.clips.get_mut(sel.clip_idx)) else { return false };
    c.locked = !c.locked;
    project.mark_modified();
    true
}

fn changed(project: &mut Project, done: bool) -> bool {
    if done {
        project.mark_modified();
    }
    done
}

fn tracks_mut(tl: &mut Timeline) -> impl Iterator<Item = &mut Track> {
    tl.video_tracks.iter_mut().chain(tl.audio_tracks.iter_mut())
}

/// Delete events `ids`; locked events and events on locked tracks stay.
pub fn delete_clips(project: &mut Project, ids: &[String]) -> bool {
    let mut done = false;
    for t in tracks_mut(&mut project.timeline).filter(|t| !t.locked) {
        let n = t.clips.len();
        t.clips.retain(|c| c.locked || !ids.contains(&c.id));
        done |= t.clips.len() != n;
    }
    changed(project, done)
}

/// Split events `ids` at `at`; an empty `ids` splits every (unlocked) event under `at`, as VEGAS does with nothing
/// selected. The first half keeps the id.
pub fn split_ids(project: &mut Project, ids: &[String], at: f64) -> bool {
    split_where(project, at, |_, c| ids.is_empty() || ids.contains(&c.id))
}

/// Split the unlocked events `pick` chooses at `at`. The right halves of a group form a new group of their own.
fn split_where(project: &mut Project, at: f64, pick: impl Fn(&Track, &Clip) -> bool) -> bool {
    let mut groups: HashMap<String, String> = HashMap::new();
    let mut done = false;
    for t in tracks_mut(&mut project.timeline).filter(|t| !t.locked) {
        let mut j = 0;
        while j < t.clips.len() {
            let c = &t.clips[j];
            if !c.locked && pick(t, c) {
                if let Some((a, mut b)) = split_clip(c, at) {
                    b.group = b.group.map(|g| groups.entry(g).or_insert_with(|| uuid::Uuid::new_v4().to_string()).clone());
                    t.clips[j] = a;
                    t.clips.insert(j + 1, b);
                    j += 1;
                    done = true;
                }
            }
            j += 1;
        }
    }
    if done {
        dissolve_lone_groups(&mut project.timeline);
    }
    changed(project, done)
}

/// R18a: Edit › Smart Split (VEGAS 23). Each of events `ids` that holds the time selection `a..b` inside it loses that
/// stretch: it is split at both ends, the middle goes, and the part after moves back to overlap the part before by
/// `overlap` (VEGAS: five frames), crossfading there; the two parts are grouped (with the event's group, if any).
/// Locked events, events on locked tracks and events whose parts would be shorter than the overlap stay as they are.
/// Returns the ripple that closes up what follows on their tracks (Auto Ripple, or Post-Edit Ripple later); None when
/// nothing was split.
// shortcut: VEGAS morphs across the overlap with its Warp Flow (optical flow) transition; this crossfades (a dissolve on
// video). Add a morph transition with the GPU effects (R21b) if wanted.
pub fn smart_split(p: &mut Project, ids: &[String], (a, b): (f64, f64), overlap: f64) -> Option<RippleOp> {
    const EPS: f64 = 1e-9;
    let shift = b - a + overlap;
    let (mut tracks, mut moved) = (Vec::new(), Vec::new());
    for t in tracks_mut(&mut p.timeline).filter(|t| !t.locked) {
        let mut j = 0;
        while j < t.clips.len() {
            let c = &t.clips[j];
            let fits = !c.locked && ids.contains(&c.id) && a - c.position > overlap + EPS && c.position + c.duration - b > overlap + EPS;
            let parts = fits.then(|| split_clip(c, a)).flatten().and_then(|(first, rest)| Some((first, split_clip(&rest, b)?.1)));
            if let Some((mut first, mut second)) = parts {
                let g = first.group.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
                (first.group, first.fade_out) = (Some(g.clone()), overlap);
                (second.group, second.fade_in) = (Some(g), overlap);
                second.position -= shift;
                moved.push(second.id.clone());
                t.clips[j] = first;
                t.clips.insert(j + 1, second);
                if !tracks.contains(&t.id) {
                    tracks.push(t.id.clone());
                }
                j += 1;
            }
            j += 1;
        }
    }
    changed(p, !moved.is_empty()).then(|| RippleOp { from: b, delta: -shift, tracks, exclude: moved, keys_from: Some(b) })
}

/// R18a: Insert › Command (or a command marker's Edit, `index`): command marker `m`; the markers stay in time order.
/// False when nothing changed.
pub fn set_command(p: &mut Project, index: Option<usize>, m: CommandMarker) -> bool {
    let cs = &mut p.timeline.commands;
    match index {
        Some(i) if cs.get(i) == Some(&m) => return false,
        Some(i) if i < cs.len() => cs[i] = m,
        Some(_) => return false,
        None => cs.push(m),
    }
    cs.sort_by(|a, b| a.position.total_cmp(&b.position));
    changed(p, true)
}

/// R18a: Insert › Audio CD Track Region over the time selection `a..b`, and Audio CD Track Index at `at` (not twice at
/// one place); both kept in time order.
pub fn insert_cd_track(p: &mut Project, (a, b): (f64, f64)) -> bool {
    let tl = &mut p.timeline;
    if b <= a || tl.cd_tracks.iter().any(|c| (c.start, c.end) == (a, b)) {
        return false;
    }
    tl.cd_tracks.push(CdTrack { start: a, end: b, ..Default::default() });
    tl.cd_tracks.sort_by(|x, y| x.start.total_cmp(&y.start));
    changed(p, true)
}

pub fn insert_cd_index(p: &mut Project, at: f64) -> bool {
    let tl = &mut p.timeline;
    if tl.cd_indexes.iter().any(|&x| (x - at).abs() < 1e-9) {
        return false;
    }
    tl.cd_indexes.push(at.max(0.0));
    tl.cd_indexes.sort_by(f64::total_cmp);
    changed(p, true)
}

// ---- Nested timelines (R6b) ----

/// R6b: the file Create Nested Timeline proposes, as VEGAS does: the parent's name plus "_Untitled Timeline", beside
/// the parent, numbered when that name is taken.
pub fn nested_file_name(parent: &std::path::Path) -> std::path::PathBuf {
    let dir = parent.parent().unwrap_or(std::path::Path::new("."));
    let stem = parent.file_stem().map_or("Untitled".into(), |s| s.to_string_lossy().into_owned());
    let name = |k: u32| if k == 1 { format!("{stem}_Untitled Timeline.mnle") } else { format!("{stem}_Untitled Timeline {k}.mnle") };
    (1..).map(|k| dir.join(name(k))).find(|p| !p.exists()).expect("a free name")
}

/// R6b: the project Create Nested Timeline saves for the events `picked` covering `start..end` of `p`, the tracks they
/// were on (`plain`: the tracks whose copy is plain, top first) and the media they use. Their timeline positions,
/// the project settings and busses stay, so it plays as they did; the master bus and markers stay in `p`.
fn extract_to_project(p: &Project, picked: &HashSet<String>, plain: [Option<usize>; 2]) -> Project {
    let tl = &p.timeline;
    let tracks = |video: bool| -> Vec<Track> {
        let any_solo = tl.any_solo(video);
        let mut out = Vec::new();
        for (i, t) in tl.tracks(video).iter().enumerate() {
            let clips: Vec<Clip> = t.clips.iter().filter(|c| picked.contains(&c.id)).cloned().collect();
            if clips.is_empty() {
                continue;
            }
            out.push(if plain[video as usize] == Some(i) {
                // Its Track Motion, FX, volume... act on the nested event in `p`, as they did on these events.
                Track { clips, height: t.height, fx: Vec::new(), ..Track::new(&t.name, t.kind) }
            } else {
                // Unheard (muted, un-soloed or in a muted group) here stays unheard there.
                Track { clips, muted: !tl.plays(t, any_solo), solo: false, armed: false, monitor: Monitor::Off, hidden: false, ..t.clone() }
            });
        }
        out
    };
    let clips = tl.video_tracks.iter().chain(&tl.audio_tracks).flat_map(|t| &t.clips).filter(|c| picked.contains(&c.id));
    let mut used: HashSet<&str> = clips.flat_map(|c| c.media_id.as_deref().into_iter().chain(c.takes.iter().map(|t| t.media_id.as_str()))).collect();
    // A subclip's parent item comes along.
    used.extend(p.media_pool.iter().filter(|m| used.contains(m.id.as_str())).filter_map(|m| m.subclip.as_ref().map(|s| s.parent.as_str())).collect::<Vec<_>>());
    // The Video Bus Track's motion blur and supersampling act inside a nested timeline (Fade to Color stays outside).
    let blur = tl.master.video_envelopes.iter().filter(|e| matches!(e.kind, EnvKind::MotionBlur | EnvKind::Supersampling)).cloned().collect();
    let timeline = Timeline {
        video_tracks: tracks(true),
        audio_tracks: tracks(false),
        master: MasterBus { video_envelopes: blur, ..MasterBus::default() },
        audio: AudioSettings { buses: tl.audio.buses.clone(), fx: tl.audio.fx.clone(), inputs: Vec::new() },
        audio_follows_velocity: tl.audio_follows_velocity,
        cursor_position: tl.cursor_position,
        ..Timeline::new()
    };
    let media_pool = p.media_pool.iter().filter(|m| used.contains(m.id.as_str())).cloned().collect();
    Project { settings: p.settings.clone(), timeline, media_pool, ..Project::new("") }
}

/// R6b: Edit › Create Nested Timeline (Alt+C): events `ids` move into a new project saved at `path` (written
/// atomically, before anything in `p` changes) and are replaced by it, a nested timeline, as a grouped video and audio
/// event pair over the span they covered (VEGAS). The picture and sound stay the same: when the events of a kind are
/// on one track with no other event over that span, the nested event goes on it (its copy in the nested timeline is
/// plain); otherwise on a new plain track above the topmost of them (the copies keep their own settings). `inside`:
/// the projects open around `p`, which it must not nest. The new event ids (video last); Err leaves `p` as it was.
pub fn create_nested(p: &mut Project, ids: &[String], path: &std::path::Path, inside: &[std::path::PathBuf]) -> Result<Vec<String>, String> {
    use crate::engine::nested;
    let target = nested::canon(path);
    if p.path.iter().map(|q| nested::canon(q)).chain(inside.iter().map(|q| nested::canon(q))).any(|q| q == target) {
        return Err(format!("{}: {}", path.display(), nested::SELF_REF));
    }
    let tl = &p.timeline;
    let refs = p.find_clips(ids);
    let clip = |r: &ClipRef| &tl.tracks(r.is_video)[r.track_idx].clips[r.clip_idx];
    if refs.is_empty() {
        return Err("no events are selected".into());
    }
    if refs.iter().any(|r| tl.tracks(r.is_video)[r.track_idx].locked || clip(r).locked) {
        return Err("locked events cannot be nested".into());
    }
    let picked: HashSet<String> = refs.iter().map(|r| clip(r).id.clone()).collect();
    let start = refs.iter().map(|r| clip(r).position).fold(f64::INFINITY, f64::min);
    let end = refs.iter().map(|r| clip(r).position + clip(r).duration).fold(f64::NEG_INFINITY, f64::max);
    if end <= start {
        return Err("the selected events have no length".into());
    }
    // Per kind (audio, video): where the nested event goes, Ok(that track) or Err(a new track inserted there).
    let dest = |video: bool| -> Option<Result<usize, usize>> {
        let ts = tl.tracks(video);
        let used: Vec<usize> = (0..ts.len()).filter(|&i| ts[i].clips.iter().any(|c| picked.contains(&c.id))).collect();
        let top = *used.first()?;
        let free = ts[top].clips.iter().all(|c| picked.contains(&c.id) || c.position + c.duration <= start || c.position >= end);
        Some(if used.len() == 1 && free { Ok(top) } else { Err(top) })
    };
    let dests = [dest(false), dest(true)];
    let mut child = extract_to_project(p, &picked, dests.map(|d| d.and_then(Result::ok)));
    child.save(&path.to_path_buf()).map_err(|e| format!("{}: {e}", path.display()))?;
    let item = nested::probe(path).map_err(|e| format!("{}: {e}", path.display()))?;
    // Nothing above changed `p`.
    for t in tracks_mut(&mut p.timeline) {
        t.clips.retain(|c| !picked.contains(&c.id));
    }
    let pair = dests.iter().flatten().count() == 2;
    let group = pair.then(|| uuid::Uuid::new_v4().to_string());
    let mut out = Vec::new();
    for (video, d) in [false, true].into_iter().zip(dests) {
        let Some(d) = d else { continue };
        let i = d.unwrap_or_else(|at| {
            let kind = if video { TrackKind::Video } else { TrackKind::Audio };
            // Plain: a new audio track's default FX chain would touch the sound.
            p.timeline.tracks_mut(video).insert(at, Track { fx: Vec::new(), ..Track::new(&item.name, kind) });
            at
        });
        let c = Clip { in_point: start, out_point: end, duration: end - start, group: group.clone(), ..Clip::from_media(&item, start) };
        out.push(c.id.clone());
        let t = &mut p.timeline.tracks_mut(video)[i];
        t.clips.push(c);
        t.clips.sort_by(|a, b| a.position.total_cmp(&b.position));
    }
    p.media_pool.push(item);
    dissolve_lone_groups(&mut p.timeline);
    tidy_track_groups(&mut p.timeline);
    changed(p, true);
    Ok(out)
}

// ---- Grouping ----

/// A group with one event left in it is no group.
pub fn dissolve_lone_groups(tl: &mut Timeline) {
    let mut count: HashMap<String, usize> = HashMap::new();
    for g in tl.video_tracks.iter().chain(&tl.audio_tracks).flat_map(|t| &t.clips).filter_map(|c| c.group.clone()) {
        *count.entry(g).or_default() += 1;
    }
    for c in tracks_mut(tl).flat_map(|t| &mut t.clips) {
        if c.group.as_ref().is_some_and(|g| count[g] < 2) {
            c.group = None;
        }
    }
}

fn all_clips(tl: &Timeline) -> impl Iterator<Item = &Clip> {
    tl.video_tracks.iter().chain(&tl.audio_tracks).flat_map(|t| &t.clips)
}

/// `ids` with every event grouped with them: the group mates first, then `ids` in their order (the focus stays last).
pub fn expand_groups(project: &Project, ids: &[String]) -> Vec<String> {
    let tl = &project.timeline;
    let groups: Vec<&String> = all_clips(tl).filter(|c| ids.contains(&c.id)).filter_map(|c| c.group.as_ref()).collect();
    let mut out: Vec<String> = all_clips(tl).filter(|c| !ids.contains(&c.id) && c.group.as_ref().is_some_and(|g| groups.contains(&g))).map(|c| c.id.clone()).collect();
    out.extend(ids.iter().cloned());
    out
}

/// Edit › Group › Create New: events `ids` (two or more) become one new group, leaving their old groups.
pub fn group(project: &mut Project, ids: &[String]) -> bool {
    if project.find_clips(ids).len() < 2 {
        return false;
    }
    let g = uuid::Uuid::new_v4().to_string();
    for c in tracks_mut(&mut project.timeline).flat_map(|t| &mut t.clips).filter(|c| ids.contains(&c.id)) {
        c.group = Some(g.clone());
    }
    dissolve_lone_groups(&mut project.timeline);
    changed(project, true)
}

/// Group › Remove From: events `ids` leave their groups.
pub fn ungroup(project: &mut Project, ids: &[String]) -> bool {
    let mut done = false;
    for c in tracks_mut(&mut project.timeline).flat_map(|t| &mut t.clips).filter(|c| ids.contains(&c.id)) {
        done |= c.group.take().is_some();
    }
    dissolve_lone_groups(&mut project.timeline);
    changed(project, done)
}

/// Group › Clear: the groups of events `ids` are dissolved.
pub fn clear_groups(project: &mut Project, ids: &[String]) -> bool {
    let all = expand_groups(project, ids);
    ungroup(project, &all)
}

/// Group mates of event `id` whose left (or right) edge is within `tol` of its own: they trim along with it.
pub fn edge_mates(project: &Project, id: &str, left: bool, tol: f64) -> Vec<String> {
    let Some(c) = project.clip(id) else { return Vec::new() };
    let edge = |c: &Clip| if left { c.position } else { c.position + c.duration };
    let (g, e) = (c.group.as_ref(), edge(c));
    all_clips(&project.timeline)
        .filter(|m| m.id != id && g.is_some() && m.group.as_ref() == g && (edge(m) - e).abs() <= tol + 1e-9)
        .map(|m| m.id.clone())
        .collect()
}

// ---- Automatic crossfades ----

/// Options › Automatic Crossfades, after an edit of events `ids` (`before`: the project before it). On each track,
/// two neighbours that overlap get a fade-out and a fade-in as long as the overlap (unless one covers the other).
/// A crossfade the edit pulls apart loses its fades; fades set by hand, not equal to an old overlap, are kept.
pub fn auto_crossfade(project: &mut Project, before: &Project, ids: &[String]) -> bool {
    const EPS: f64 = 1e-9;
    let ids: HashSet<&str> = ids.iter().map(String::as_str).collect();
    let touched = |c: &Clip| ids.contains(c.id.as_str());
    let sorted = |t: &Track| {
        let mut k: Vec<usize> = (0..t.clips.len()).collect();
        k.sort_by(|&a, &b| t.clips[a].position.total_cmp(&t.clips[b].position));
        k
    };
    let overlap = |a: &Clip, b: &Clip| a.position + a.duration - b.position;
    let mut done = false;
    let btl = &before.timeline;
    for t in btl.video_tracks.iter().chain(&btl.audio_tracks) {
        for w in sorted(t).windows(2) {
            let (a, b) = (&t.clips[w[0]], &t.clips[w[1]]);
            let o = overlap(a, b);
            if t.locked || a.locked || b.locked || !(touched(a) || touched(b)) || o <= EPS || (a.fade_out - o).abs() > EPS || (b.fade_in - o).abs() > EPS {
                continue;
            }
            let (ra, rb) = (project.find_clip(&a.id), project.find_clip(&b.id));
            let still = ra.as_ref().zip(rb.as_ref()).is_some_and(|(ra, rb)| {
                let (na, nb) = (project.clip(&a.id).unwrap(), project.clip(&b.id).unwrap());
                (ra.is_video, ra.track_idx) == (rb.is_video, rb.track_idx) && na.position < nb.position + nb.duration && nb.position < na.position + na.duration
            });
            if still {
                continue;
            }
            let tl = &mut project.timeline;
            if let Some(r) = ra {
                tl.tracks_mut(r.is_video)[r.track_idx].clips[r.clip_idx].fade_out = 0.0;
            }
            if let Some(r) = rb {
                tl.tracks_mut(r.is_video)[r.track_idx].clips[r.clip_idx].fade_in = 0.0;
            }
            done = true;
        }
    }
    // The pair's overlap before the edit, when it was a pair on one track then.
    let was = |a: &Clip, b: &Clip| -> Option<f64> {
        let (ra, rb) = (before.find_clip(&a.id)?, before.find_clip(&b.id)?);
        if (ra.is_video, ra.track_idx) != (rb.is_video, rb.track_idx) {
            return None;
        }
        Some(overlap(before.clip(&a.id)?, before.clip(&b.id)?))
    };
    for t in tracks_mut(&mut project.timeline).filter(|t| !t.locked && t.clips.iter().any(touched)) {
        for w in sorted(t).windows(2) {
            let (a, b) = (&t.clips[w[0]], &t.clips[w[1]]);
            let o = overlap(a, b);
            let covered = b.position + b.duration <= a.position + a.duration + EPS || o >= a.duration - EPS;
            // An overlap the edit left as it was keeps the fades it has (hand-set ones too).
            let unchanged = (a.fade_out > EPS || b.fade_in > EPS) && was(a, b).is_some_and(|w| (w - o).abs() <= EPS);
            if a.locked || b.locked || unchanged || !(touched(a) || touched(b)) || o <= EPS || covered || (a.fade_out == o && b.fade_in == o) {
                continue;
            }
            for (k, out) in [(w[0], true), (w[1], false)] {
                let c = &mut t.clips[k];
                *(if out { &mut c.fade_out } else { &mut c.fade_in }) = o;
                c.clamp_fades();
            }
            done = true;
        }
    }
    changed(project, done)
}

/// `auto_crossfade` for the events a ripple moved (`before`: the project before the ripple): overlaps it made get
/// crossfades, and crossfades it pulled apart lose their fades.
pub fn crossfade_moved(project: &mut Project, before: &Project) -> bool {
    let was: HashMap<&str, f64> = all_clips(&before.timeline).map(|c| (c.id.as_str(), c.position)).collect();
    let ids: Vec<String> = all_clips(&project.timeline).filter(|c| was.get(c.id.as_str()).is_some_and(|p| *p != c.position)).map(|c| c.id.clone()).collect();
    !ids.is_empty() && auto_crossfade(project, before, &ids)
}

/// Fade Type of the crossfade between `a` (outgoing, curve `out`) and `b` (incoming, curve `inc`).
pub fn set_crossfade_curve(project: &mut Project, a: &str, b: &str, out: Curve, inc: Curve) -> bool {
    edit_clips(project, &[a.to_string(), b.to_string()], |c| {
        let (f, curve) = if c.id == a { (&mut c.fade_out_curve, out) } else { (&mut c.fade_in_curve, inc) };
        std::mem::replace(f, curve) != curve
    })
}

// ---- Ripple ----

/// What a ripple moves besides the events on the affected tracks (Options › Ripple).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RippleMode {
    #[default]
    Affected,
    /// Affected Tracks, Bus Tracks, Markers, and Regions.
    AffectedMarkers,
    /// R8b: Affected Tracks, Markers, and Regions: the bus tracks' envelopes stay.
    AffectedMarkersOnly,
    All,
}

/// Material at or after `from` on `tracks` (track ids) moves by `delta`; events in `exclude` stay put.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RippleOp {
    pub from: f64,
    pub delta: f64,
    pub tracks: Vec<String>,
    pub exclude: Vec<String>,
    /// Where the time actually opens or closes, for envelope keys, markers and regions, when that is not `from`
    /// (a deleted gap's end, a trimmed edge): a closing ripple deletes keys in `keys_from + delta .. keys_from`.
    pub keys_from: Option<f64>,
}

/// Ripple: unlocked events at or after `from` move by `delta` (clamped so none goes before 0), with their tracks'
/// envelope keys at or after `keys_from` (keys in a gap being closed are deleted). Modes other than Affected also move
/// markers, regions and the time selection (one that straddles `keys_from` stretches); All moves every track. All and
/// AffectedMarkers also move the bus tracks' envelope keys (audio busses, assignable FX chains, the Video Bus Track).
pub fn ripple(project: &mut Project, op: &RippleOp, mode: RippleMode) -> bool {
    const EPS: f64 = 1e-9;
    let all = mode == RippleMode::All;
    let exclude: HashSet<&str> = op.exclude.iter().map(String::as_str).collect();
    let hit = |t: &Track| !t.locked && (all || op.tracks.contains(&t.id));
    let moves = |c: &Clip| !c.locked && c.position >= op.from - EPS && !exclude.contains(c.id.as_str());
    let tl = &mut project.timeline;
    let min = tl.video_tracks.iter().chain(&tl.audio_tracks).filter(|t| hit(t)).flat_map(|t| &t.clips).filter(|c| moves(c)).map(|c| c.position).fold(f64::INFINITY, f64::min);
    let d = op.delta.max(-if min.is_finite() { min } else { op.from.max(0.0) });
    if d == 0.0 {
        return false;
    }
    let from = op.keys_from.unwrap_or(op.from);
    let at_or_after = |t: f64| t >= from - EPS;
    let shift = |e: &mut Envelope| {
        let keys = &mut e.points.keys;
        if d < 0.0 {
            keys.retain(|k| k.t < from + d - EPS || at_or_after(k.t));
        }
        let mut done = false;
        for k in keys.iter_mut().filter(|k| at_or_after(k.t)) {
            k.t = (k.t + d).max(0.0);
            done = true;
        }
        done
    };
    let mut done = false;
    for t in tracks_mut(tl).filter(|t| hit(t)) {
        for c in t.clips.iter_mut().filter(|c| moves(c)) {
            c.position += d;
            done = true;
        }
        t.clips.sort_by(|a, b| a.position.total_cmp(&b.position));
        for e in &mut t.envelopes {
            done |= shift(e);
        }
        // R8c: FX automation rides with the track's envelopes.
        for kf in t.fx.iter_mut().flat_map(|f| f.auto.values_mut()) {
            done |= ripple_keys(kf, &shift);
        }
    }
    if matches!(mode, RippleMode::AffectedMarkers | RippleMode::All) {
        let a = &mut tl.audio;
        for e in a.buses.iter_mut().chain(&mut a.fx).flat_map(|b| &mut b.envelopes).chain(&mut tl.master.video_envelopes) {
            done |= shift(e);
        }
        for kf in a.buses.iter_mut().chain(&mut a.fx).flat_map(|b| &mut b.fx).flat_map(|f| f.auto.values_mut()) {
            done |= ripple_keys(kf, &shift);
        }
    }
    if mode != RippleMode::Affected {
        // R18a: command markers and CD indexes ride with the markers, CD track regions with the regions.
        let points = tl.markers.iter_mut().map(|m| &mut m.position).chain(tl.commands.iter_mut().map(|c| &mut c.position)).chain(&mut tl.cd_indexes);
        for m in points.filter(|m| at_or_after(**m)) {
            *m = (*m + d).max(0.0);
            done = true;
        }
        let shift = |(a, b): (f64, f64)| -> (f64, f64) {
            if at_or_after(a) {
                ((a + d).max(0.0), (b + d).max(0.0))
            } else if at_or_after(b) {
                (a, (b + d).max(a))
            } else {
                (a, b)
            }
        };
        for (s, e) in tl.regions.iter_mut().map(|r| (&mut r.start, &mut r.end)).chain(tl.cd_tracks.iter_mut().map(|r| (&mut r.start, &mut r.end))) {
            let new = shift((*s, *e));
            done |= new != (*s, *e);
            (*s, *e) = new;
        }
        let new = tl.loop_region.map(shift);
        done |= new != tl.loop_region;
        tl.loop_region = new;
    }
    changed(project, done)
}

/// `ripple` each op in turn (they come right to left from `gaps`).
pub fn ripple_all(project: &mut Project, ops: &[RippleOp], mode: RippleMode) -> bool {
    ops.iter().fold(false, |done, op| ripple(project, op, mode) | done)
}

/// The gaps deleting events `ids` leaves (locked ones stay), as ripple ops closing them, right to left. Overlapping
/// spans on one track are one gap, and only their empty time closes: a kept event covering part of a span (the other
/// half of a crossfade) keeps it. A gap at the same place on several tracks (a video and its audio) is one op.
pub fn gaps(project: &Project, ids: &[String]) -> Vec<RippleOp> {
    const EPS: f64 = 1e-9;
    let tl = &project.timeline;
    let gone: HashSet<&str> = ids.iter().map(String::as_str).collect();
    let mut spans: Vec<(f64, f64, String)> = Vec::new();
    for t in tl.video_tracks.iter().chain(&tl.audio_tracks).filter(|t| !t.locked) {
        let (del, kept): (Vec<&Clip>, Vec<&Clip>) = t.clips.iter().partition(|c| !c.locked && gone.contains(c.id.as_str()));
        let mut s: Vec<(f64, f64)> = del.iter().map(|c| (c.position, c.position + c.duration)).collect();
        s.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut merged: Vec<(f64, f64)> = Vec::new();
        for (a, b) in s {
            match merged.last_mut() {
                Some(m) if a <= m.1 => m.1 = m.1.max(b),
                _ => merged.push((a, b)),
            }
        }
        for (a, b) in merged {
            let mut s = a;
            while s < b - EPS {
                // Past a kept event that covers `s`, else up to the next one that starts inside the span.
                if let Some(e) = kept.iter().filter(|k| k.position <= s + EPS && k.position + k.duration > s + EPS).map(|k| k.position + k.duration).reduce(f64::max) {
                    s = e;
                    continue;
                }
                let e = kept.iter().map(|k| k.position).filter(|&k| k > s + EPS && k < b).fold(b, f64::min);
                spans.push((s, e, t.id.clone()));
                s = e;
            }
        }
    }
    let mut ops: Vec<RippleOp> = Vec::new();
    for (a, b, track) in spans {
        match ops.iter_mut().find(|o| o.from == a && o.delta == a - b) {
            Some(o) => o.tracks.push(track),
            None => ops.push(RippleOp { from: a, delta: a - b, tracks: vec![track], exclude: Vec::new(), keys_from: Some(b) }),
        }
    }
    ops.sort_by(|x, y| y.from.total_cmp(&x.from));
    ops
}

/// Delete the time range `a..b` on tracks `tracks` (every track in All mode): events are split at both ends, the
/// pieces inside removed, and everything after closes up by `b - a`.
pub fn delete_range(project: &mut Project, a: f64, b: f64, tracks: &[String], mode: RippleMode) -> bool {
    if b <= a {
        return false;
    }
    let on = |t: &Track| mode == RippleMode::All || tracks.contains(&t.id);
    let mut done = split_where(project, a, |t, _| on(t)) | split_where(project, b, |t, _| on(t));
    for t in tracks_mut(&mut project.timeline).filter(|t| !t.locked && on(t)) {
        let n = t.clips.len();
        t.clips.retain(|c| c.locked || c.position < a - 1e-9 || c.position + c.duration > b + 1e-9);
        done |= t.clips.len() != n;
    }
    dissolve_lone_groups(&mut project.timeline);
    let ids: Vec<String> = project.timeline.video_tracks.iter().chain(&project.timeline.audio_tracks).filter(|t| on(t)).map(|t| t.id.clone()).collect();
    done |= ripple(project, &RippleOp { from: b, delta: a - b, tracks: ids, ..Default::default() }, mode);
    changed(project, done)
}

/// The ripple a paste of `clips` at `at` makes room with (Paste Insert, or Paste with Auto Ripple): its tracks as
/// `paste_clips` picks them, from `at` by the pasted length.
pub fn paste_ripple(project: &Project, clips: &[(bool, usize, Clip)], at: f64, focus: Option<(bool, usize)>) -> RippleOp {
    let base = |v: bool| focus.filter(|f| f.0 == v).map_or(0, |f| f.1);
    let mut tracks = Vec::new();
    for (v, o, _) in clips {
        if let Some(t) = project.timeline.tracks(*v).get(base(*v) + o).filter(|t| !tracks.contains(&t.id)) {
            tracks.push(t.id.clone());
        }
    }
    let len = clips.iter().map(|(_, _, c)| c.position + c.duration).fold(0.0, f64::max);
    RippleOp { from: at, delta: len, tracks, ..Default::default() }
}

/// The ripple of trimming event `o` to `c` (with its trimmed group mates, all in `ids`) on `tracks`, `before`
/// being the project before the trim. What starts after `o`'s start (also a crossfaded neighbour) follows the right
/// edge's change; a changed left edge keeps the event's start (its content shifts) and moves what follows along.
pub fn trim_ripple(before: &Project, o: &Clip, c: &Clip, ids: &[String], tracks: &[String]) -> Vec<RippleOp> {
    const EPS: f64 = 1e-9;
    let mut ops = Vec::new();
    let end = (c.position + c.duration) - (o.position + o.duration);
    if end.abs() > EPS {
        ops.push(RippleOp { from: o.position, delta: end, tracks: tracks.to_vec(), exclude: ids.to_vec(), keys_from: Some(o.position + o.duration) });
    }
    let start = o.position - c.position;
    if start.abs() > EPS {
        let from = c.position.min(o.position);
        // An extended left edge leaves alone what starts between its new and its old start.
        let between = all_clips(&before.timeline).filter(|k| k.position >= from - EPS && k.position < o.position - EPS && !ids.contains(&k.id));
        // Keys: a shrunk left edge closes old start..new start; an extended one opens at the old start.
        let keys_from = Some(c.position.max(o.position));
        ops.push(RippleOp { from, delta: start, tracks: tracks.to_vec(), exclude: between.map(|k| k.id.clone()).collect(), keys_from });
    }
    ops
}

/// Move events `ids` by `dt` seconds and `dtrack` tracks (the same index offset within each kind). The earliest
/// stays at or after 0. A hop onto a missing or locked track refuses the whole move; locked events stay where they
/// are. Touched tracks are stable-sorted by position. With `lock_env` (Lock Envelopes to Events) the tracks'
/// envelope points under the moved events go with them (`carry_envelope_points`).
pub fn move_clips(project: &mut Project, ids: &[String], dt: f64, dtrack: isize, lock_env: bool) -> bool {
    let tl = &project.timeline;
    let refs: Vec<ClipRef> = project
        .find_clips(ids)
        .into_iter()
        .filter(|r| {
            let t = &tl.tracks(r.is_video)[r.track_idx];
            !t.locked && !t.clips[r.clip_idx].locked
        })
        .collect();
    let at = |r: &ClipRef| &tl.tracks(r.is_video)[r.track_idx].clips[r.clip_idx];
    let Some(min) = refs.iter().map(|r| at(r).position).reduce(f64::min) else { return false };
    let dt = dt.max(-min);
    let target = |r: &ClipRef| r.track_idx.checked_add_signed(dtrack).filter(|&i| tl.tracks(r.is_video).get(i).is_some_and(|t| !t.locked));
    let Some(targets) = refs.iter().map(target).collect::<Option<Vec<usize>>>() else { return false };
    if dt == 0.0 && dtrack == 0 {
        return false;
    }
    let spans: Vec<_> = refs.iter().zip(&targets).map(|(r, &to)| (r.is_video, r.track_idx, to, at(r).position, at(r).position + at(r).duration)).collect();
    // Take them out from the highest index down so the other indices stay valid, then place them.
    let mut order: Vec<usize> = (0..refs.len()).collect();
    order.sort_by_key(|&k| std::cmp::Reverse((refs[k].is_video, refs[k].track_idx, refs[k].clip_idx)));
    let tl = &mut project.timeline;
    let mut moved = Vec::new();
    for k in order {
        let r = &refs[k];
        let mut c = tl.tracks_mut(r.is_video)[r.track_idx].clips.remove(r.clip_idx);
        c.position += dt;
        moved.push((k, c));
    }
    moved.sort_by_key(|(k, _)| *k);
    let mut touched: Vec<(bool, usize)> = Vec::new();
    for (k, c) in moved {
        let v = refs[k].is_video;
        tl.tracks_mut(v)[targets[k]].clips.push(c);
        touched.extend([(v, refs[k].track_idx), (v, targets[k])]);
    }
    for (v, i) in touched {
        tl.tracks_mut(v)[i].clips.sort_by(|a, b| a.position.total_cmp(&b.position));
    }
    if lock_env {
        carry_spans(tl, spans, dt);
    }
    changed(project, true)
}

/// Snap `t` to the project frame grid (Options › Quantize to Frames).
pub fn quantize(t: f64, fps: f64) -> f64 {
    let fps = exact_fps(fps.max(1.0)); // the render's grid: 30000/1001 at 29.97
    (t * fps).round() / fps
}

/// The frame-grid time nearest `t` on the side of `toward` (`t`'s own when it is on the grid): an edit stopped off
/// the grid by a limit is pulled back onto it, inside the limit.
pub fn quantize_toward(t: f64, fps: f64, toward: f64) -> f64 {
    let q = quantize(t, fps);
    match (q - t) * (toward - t) {
        _ if (q - t).abs() < 1e-9 => q,
        s if s > 0.0 => q,
        _ if toward == t => t,
        _ => q + (toward - t).signum() / exact_fps(fps.max(1.0)),
    }
}

/// Clipboard copy of events `ids`: (is video, track offset from the topmost copied track of that kind, event with
/// its position relative to the earliest copied event).
pub fn copy_clips(project: &Project, ids: &[String]) -> Vec<(bool, usize, Clip)> {
    let refs = project.find_clips(ids);
    let tl = &project.timeline;
    let clip = |r: &ClipRef| &tl.tracks(r.is_video)[r.track_idx].clips[r.clip_idx];
    let t0 = refs.iter().map(|r| clip(r).position).reduce(f64::min).unwrap_or(0.0);
    let top = |v: bool| refs.iter().filter(|r| r.is_video == v).map(|r| r.track_idx).min().unwrap_or(0);
    refs.iter()
        .map(|r| {
            let mut c = clip(r).clone();
            c.position -= t0;
            (r.is_video, r.track_idx - top(r.is_video), c)
        })
        .collect()
}

/// Paste `clips` (from `copy_clips`) at `at`. Track offsets count from the focus track for its kind and from the
/// first track for the other kind; missing tracks are created, and a locked target track refuses the paste. Events
/// get fresh ids, and group ids map to fresh ones. Returns the new ids.
pub fn paste_clips(project: &mut Project, clips: &[(bool, usize, Clip)], at: f64, focus: Option<(bool, usize)>) -> Vec<String> {
    let base = |v: bool| focus.filter(|f| f.0 == v).map_or(0, |f| f.1);
    let tl = &mut project.timeline;
    if clips.iter().any(|(v, o, _)| tl.tracks(*v).get(base(*v) + o).is_some_and(|t| t.locked)) {
        return Vec::new();
    }
    let mut groups: HashMap<String, String> = HashMap::new();
    let mut ids = Vec::new();
    for (v, o, c) in clips {
        let i = base(*v) + o;
        while tl.tracks(*v).len() <= i {
            tl.add_track(*v);
        }
        let mut c = c.clone();
        c.id = uuid::Uuid::new_v4().to_string();
        c.position += at;
        c.locked = false;
        c.group = c.group.map(|g| groups.entry(g).or_insert_with(|| uuid::Uuid::new_v4().to_string()).clone());
        ids.push(c.id.clone());
        let clips = &mut tl.tracks_mut(*v)[i].clips;
        clips.push(c);
        clips.sort_by(|a, b| a.position.total_cmp(&b.position));
    }
    let done = !ids.is_empty();
    changed(project, done);
    ids
}

/// Paste Repeat (Ctrl+B): `clips` pasted `n` times from `at`, end to end (`every` None: each copy starts where the
/// last ends) or with a copy starting every `every` seconds. Returns all the new ids; empty if a locked track refuses.
pub fn paste_repeat(project: &mut Project, clips: &[(bool, usize, Clip)], at: f64, focus: Option<(bool, usize)>, n: usize, every: Option<f64>) -> Vec<String> {
    let len = clips.iter().map(|(_, _, c)| c.position + c.duration).fold(0.0, f64::max);
    let step = every.filter(|e| *e > 0.0).unwrap_or(len);
    let mut ids = Vec::new();
    for k in 0..n {
        let new = paste_clips(project, clips, at + k as f64 * step, focus);
        if new.is_empty() {
            break;
        }
        ids.extend(new);
    }
    ids
}

/// Select All After Cursor (VP23): every event that starts at or after `t`.
pub fn select_after(project: &Project, t: f64) -> Vec<String> {
    all_clips(&project.timeline).filter(|c| c.position >= t - 1e-9).map(|c| c.id.clone()).collect()
}

/// Select Events to End: event `id` and the events after it on its track (they start at or after it).
pub fn events_to_end(project: &Project, id: &str) -> Vec<String> {
    let (Some(r), Some(c)) = (project.find_clip(id), project.clip(id)) else { return Vec::new() };
    let start = c.position;
    project.timeline.tracks(r.is_video)[r.track_idx].clips.iter().filter(|k| k.position >= start - 1e-9).map(|k| k.id.clone()).collect()
}

/// Set Selection to Previous / Next Selected Event (Ctrl+Shift+[ / ]): the span of the selected event that starts
/// last before `cursor` (or first after it).
pub fn selected_event_span(project: &Project, ids: &[String], cursor: f64, next: bool) -> Option<(f64, f64)> {
    let spans = ids.iter().filter_map(|id| project.clip(id)).map(|c| (c.position, c.position + c.duration));
    if next {
        spans.filter(|s| s.0 > cursor + 1e-9).min_by(|a, b| a.0.total_cmp(&b.0))
    } else {
        spans.filter(|s| s.0 < cursor - 1e-9).max_by(|a, b| a.0.total_cmp(&b.0))
    }
}

/// Time Select Right by Marker/Region: the time selection grown from the cursor to `to`. A cursor at one end of the
/// selection `sel` keeps the other end as the anchor (repeating the command extends it further).
pub fn extend_selection(sel: Option<(f64, f64)>, cursor: f64, to: f64) -> (f64, f64) {
    let anchor = match sel {
        Some((a, b)) if (cursor - b).abs() < 1e-9 => a,
        Some((a, b)) if (cursor - a).abs() < 1e-9 => b,
        _ => cursor,
    };
    (anchor.min(to), anchor.max(to))
}

/// Edge-trimming mode: the event edge before (or after) the edge `end` of event `id`, on its track, in time order
/// (where two events meet, the outgoing end comes before the incoming start). Returns (event, end, time).
pub fn next_edge(project: &Project, id: &str, end: bool, forward: bool) -> Option<(String, bool, f64)> {
    let r = project.find_clip(id)?;
    let mut edges: Vec<(f64, bool, &str)> =
        project.timeline.tracks(r.is_video)[r.track_idx].clips.iter().flat_map(|c| [(c.position, false, c.id.as_str()), (c.position + c.duration, true, c.id.as_str())]).collect();
    edges.sort_by(|a, b| a.0.total_cmp(&b.0).then(b.1.cmp(&a.1)));
    let k = edges.iter().position(|e| e.2 == id && e.1 == end)?;
    let e = if forward { edges.get(k + 1) } else { k.checked_sub(1).and_then(|k| edges.get(k)) }?;
    Some((e.2.to_string(), e.1, e.0))
}

/// Edge-trimming mode: move edge `end` of event `id` by `d` seconds (a plain trim, within its media and at least a
/// frame long). False when locked or nothing moved.
pub fn trim_edge(project: &mut Project, id: &str, end: bool, d: f64) -> bool {
    let frame = 1.0 / exact_fps(project.settings.frame_rate.max(1.0));
    let (Some(r), Some(orig)) = (project.find_clip(id), project.clip(id).cloned()) else { return false };
    let limit = media_limit(project, &orig);
    let Some(track) = unlocked_track(project, &r) else { return false };
    let c = &mut track.clips[r.clip_idx];
    if end {
        trim_right(c, &orig, orig.position + orig.duration + d, frame, limit);
    } else {
        trim_left(c, &orig, orig.position + d, frame, limit);
    }
    let done = (c.position, c.duration) != (orig.position, orig.duration);
    track.clips.sort_by(|a, b| a.position.total_cmp(&b.position));
    changed(project, done)
}

/// Exclusive mute / solo (Shift+Z / Shift+X): tracks `ids` are muted (soloed) and every other track is not; done
/// again on that state, it clears them.
pub fn exclusive_tracks(project: &mut Project, ids: &[String], solo: bool) -> bool {
    if ids.is_empty() {
        return false;
    }
    let flag = |t: &Track| if solo { t.solo } else { t.muted };
    let only = tracks_mut(&mut project.timeline).all(|t| flag(t) == ids.contains(&t.id));
    for t in tracks_mut(&mut project.timeline) {
        let on = !only && ids.contains(&t.id);
        if solo { t.solo = on } else { t.muted = on }
    }
    changed(project, true)
}

/// Restore Previous Time Selection (Backspace) keeps the last five time selections, as VEGAS does; `loop_region` is
/// the last time selection made, which Select Loop Region (Shift+Q) brings back after it was cleared.
#[derive(Clone, Debug, Default)]
pub struct SelHistory {
    current: Option<(f64, f64)>,
    prev: Vec<(f64, f64)>,
    pub loop_region: Option<(f64, f64)>,
}

impl SelHistory {
    /// Note the time selection `sel` (call it once a selection drag has ended).
    pub fn observe(&mut self, sel: Option<(f64, f64)>) {
        let sel = sel.filter(|(a, b)| b > a);
        if sel == self.current {
            return;
        }
        if let Some(c) = self.current.take() {
            self.prev.push(c);
            if self.prev.len() > 5 {
                self.prev.remove(0);
            }
        }
        self.current = sel;
        self.loop_region = sel.or(self.loop_region);
    }

    /// The selection before the current one, which becomes current (Backspace again goes further back).
    pub fn restore(&mut self) -> Option<(f64, f64)> {
        let p = self.prev.pop()?;
        (self.current, self.loop_region) = (Some(p), Some(p));
        Some(p)
    }
}

/// Add an empty track of that kind above track `before`, as VEGAS's header menu does; without one, a video track
/// goes first and an audio track last (VEGAS's Ctrl+Shift+Q / Ctrl+Q). Returns its index.
pub fn insert_track(project: &mut Project, is_video: bool, before: Option<usize>) -> usize {
    let tracks = project.timeline.tracks_mut(is_video);
    let at = before.map_or(if is_video { 0 } else { tracks.len() }, |b| b.min(tracks.len()));
    let (name, kind) = if is_video { ("Video", TrackKind::Video) } else { ("Audio", TrackKind::Audio) };
    tracks.insert(at, crate::project::inserted_track(&format!("{name} {}", tracks.len() + 1), kind));
    tidy_track_groups(&mut project.timeline); // inserted inside a track group, it joins it
    project.mark_modified();
    at
}

/// R18b: Insert › Adjustment Track (Ctrl+Shift+Alt+Q, VEGAS 23): a video track whose Track FX adjust the tracks below,
/// above video track `before` (else at the top). Returns its index.
pub fn insert_adjustment_track(project: &mut Project, before: Option<usize>) -> usize {
    let tracks = &mut project.timeline.video_tracks;
    let at = before.map_or(0, |b| b.min(tracks.len()));
    let n = tracks.iter().filter(|t| t.adjustment).count() + 1;
    tracks.insert(at, Track { adjustment: true, ..Track::new(&format!("Adjustment Track {n}"), TrackKind::Video) });
    tidy_track_groups(&mut project.timeline);
    project.mark_modified();
    at
}

/// Delete tracks `ids` with their events. Deleting every track is allowed, as in VEGAS.
pub fn delete_tracks(project: &mut Project, ids: &[String]) -> bool {
    let tl = &mut project.timeline;
    let n = tl.video_tracks.len() + tl.audio_tracks.len();
    tl.video_tracks.retain(|t| !ids.contains(&t.id));
    tl.audio_tracks.retain(|t| !ids.contains(&t.id));
    let done = tl.video_tracks.len() + tl.audio_tracks.len() != n;
    tidy_track_groups(tl);
    changed(project, done)
}

/// Move tracks `ids` of one kind, keeping their order, into the gap before track `to` (past the end = last). Moved
/// away from the rest of their track group they leave it; moved between two tracks of a group they join it.
pub fn move_tracks(project: &mut Project, is_video: bool, ids: &[String], to: usize) -> bool {
    let tracks = project.timeline.tracks_mut(is_video);
    let old: Vec<String> = tracks.iter().map(|t| t.id.clone()).collect();
    let at = tracks[..to.min(tracks.len())].iter().filter(|t| !ids.contains(&t.id)).count();
    let (moved, mut rest): (Vec<Track>, Vec<Track>) = std::mem::take(tracks).into_iter().partition(|t| ids.contains(&t.id));
    let n = moved.len();
    rest.splice(at..at, moved);
    *tracks = rest;
    let done = tracks.iter().map(|t| &t.id).ne(old.iter());
    let order: Vec<String> = tracks.iter().map(|t| t.id.clone()).collect();
    for g in &mut project.timeline.track_groups {
        // The block of moved tracks stays in its group while it touches the group's other tracks of its kind.
        let others: Vec<usize> = order.iter().enumerate().filter(|(_, id)| g.tracks.contains(id) && !ids.contains(id)).map(|(k, _)| k).collect();
        let (Some(&lo), Some(&hi)) = (others.first(), others.last()) else { continue };
        if at > hi + 1 || at + n < lo {
            g.tracks.retain(|t| !ids.contains(t));
        }
    }
    tidy_track_groups(&mut project.timeline);
    changed(project, done)
}

/// Keep the track groups consistent after tracks are added, removed or moved: ids of deleted tracks and empty groups
/// go, and an ungrouped track between two tracks of one group (of its kind) joins that group.
pub fn tidy_track_groups(tl: &mut Timeline) {
    let order: Vec<(bool, String)> = [true, false].into_iter().flat_map(|v| tl.tracks(v).iter().map(move |t| (v, t.id.clone()))).collect();
    for g in &mut tl.track_groups {
        g.tracks.retain(|id| order.iter().any(|(_, o)| o == id));
    }
    tl.track_groups.retain(|g| !g.tracks.is_empty());
    for (k, (v, id)) in order.iter().enumerate() {
        if tl.group_of(id).is_some() {
            continue;
        }
        let kind = |r: &[(bool, String)], g: &TrackGroup| r.iter().any(|(w, o)| w == v && g.tracks.contains(o));
        if let Some(g) = tl.track_groups.iter_mut().find(|g| kind(&order[..k], g) && kind(&order[k + 1..], g)) {
            g.tracks.push(id.clone());
        }
    }
}

/// Track Group › Group Selected Tracks: tracks `ids` leave their groups and form a new group, gathered (per kind)
/// under the first of them, or just after a group that first one would split.
pub fn group_tracks(project: &mut Project, ids: &[String]) -> bool {
    let tl = &mut project.timeline;
    let ids: Vec<String> = [true, false].into_iter().flat_map(|v| tl.tracks(v).iter().map(|t| t.id.clone()).collect::<Vec<_>>()).filter(|t| ids.contains(t)).collect();
    if ids.is_empty() {
        return false;
    }
    for g in &mut tl.track_groups {
        g.tracks.retain(|t| !ids.contains(t));
    }
    tl.track_groups.retain(|g| !g.tracks.is_empty());
    let n = (1..).find(|n| !tl.track_groups.iter().any(|g| g.name == format!("Track Group {n}"))).unwrap_or(1);
    let gid = uuid::Uuid::new_v4().to_string();
    // In place before the gathering moves, so no other group takes these tracks in on the way.
    tl.track_groups.push(TrackGroup { id: gid.clone(), name: format!("Track Group {n}"), tracks: ids.clone(), collapsed: false, muted: false, solo: false });
    for v in [true, false] {
        let list: Vec<&String> = project.timeline.tracks(v).iter().map(|t| &t.id).collect();
        let Some(mut to) = list.iter().position(|t| ids.contains(t)) else { continue };
        for g in project.timeline.track_groups.iter().filter(|g| g.id != gid) {
            let at: Vec<usize> = list.iter().enumerate().filter(|(_, t)| g.tracks.contains(t)).map(|(k, _)| k).collect();
            if let (Some(&lo), Some(&hi)) = (at.first(), at.last()) {
                if lo < to && to < hi {
                    to = hi + 1;
                }
            }
        }
        let mine: Vec<String> = list.iter().filter(|t| ids.contains(t)).map(|t| (*t).clone()).collect();
        move_tracks(project, v, &mine, to);
    }
    changed(project, true)
}

/// Track Group › Ungroup Selected Tracks: tracks `ids` leave their groups; one that sat inside its group moves to just
/// after it, so the group stays in one piece. An emptied group is dropped.
pub fn ungroup_tracks(project: &mut Project, ids: &[String]) -> bool {
    let groups: Vec<TrackGroup> = project.timeline.track_groups.clone();
    if !groups.iter().any(|g| g.tracks.iter().any(|t| ids.contains(t))) {
        return false;
    }
    for g in &mut project.timeline.track_groups {
        g.tracks.retain(|t| !ids.contains(t));
    }
    for g in &groups {
        for v in [true, false] {
            let list: Vec<String> = project.timeline.tracks(v).iter().map(|t| t.id.clone()).collect();
            let Some(hi) = list.iter().rposition(|t| g.tracks.contains(t) && !ids.contains(t)) else { continue };
            let lo = list.iter().position(|t| g.tracks.contains(t) && !ids.contains(t)).unwrap_or(hi);
            let inside: Vec<String> = list[lo..hi].iter().filter(|t| g.tracks.contains(t) && ids.contains(t)).cloned().collect();
            if !inside.is_empty() {
                move_tracks(project, v, &inside, hi + 1);
            }
        }
    }
    tidy_track_groups(&mut project.timeline);
    changed(project, true)
}

/// Mute (or Solo) Track Group: toggles it on the groups of tracks `ids`, all set to the opposite of the focus (last)
/// track's group. One undo step for all of their tracks.
pub fn toggle_track_groups(project: &mut Project, ids: &[String], solo: bool) -> bool {
    let tl = &mut project.timeline;
    let Some(on) = ids.iter().rev().find_map(|id| tl.group_of(id)).map(|g| !if solo { g.solo } else { g.muted }) else { return false };
    for g in tl.track_groups.iter_mut().filter(|g| g.tracks.iter().any(|t| ids.contains(t))) {
        if solo { g.solo = on } else { g.muted = on }
    }
    changed(project, true)
}

/// Mute (or solo) tracks `ids`: all set to the opposite of the focus (last) track's state.
pub fn toggle_tracks(project: &mut Project, ids: &[String], solo: bool) -> bool {
    fn flag(t: &mut Track, solo: bool) -> &mut bool {
        if solo { &mut t.solo } else { &mut t.muted }
    }
    let Some(on) = ids.last().and_then(|id| tracks_mut(&mut project.timeline).find(|t| t.id == *id)).map(|t| !*flag(t, solo)) else { return false };
    for t in tracks_mut(&mut project.timeline).filter(|t| ids.contains(&t.id)) {
        *flag(t, solo) = on;
    }
    changed(project, true)
}

/// View › Minimize All Tracks: set every track to `min_h`, keeping the heights in `saved`; the next call restores
/// them. Heights are view state: marked modified, never an undo step.
pub fn toggle_minimize(project: &mut Project, saved: &mut Option<Vec<(String, f32)>>, min_h: f32) {
    let all = tracks_mut(&mut project.timeline);
    match saved.take() {
        Some(s) => {
            for t in all {
                if let Some((_, h)) = s.iter().find(|(id, _)| *id == t.id) {
                    t.height = *h;
                }
            }
        }
        None => *saved = Some(all.map(|t| (t.id.clone(), std::mem::replace(&mut t.height, min_h))).collect()),
    }
    project.mark_modified();
}

/// Where a media drop lands: each new event with its (is video, track index). An index past the last track of its
/// kind is a track the drop adds at the end. `refused`: a locked track is in the way, so nothing is added.
#[derive(Debug, Default)]
pub struct DropPlan {
    pub events: Vec<(bool, usize, Clip)>,
    pub refused: bool,
}

/// Lay `items` (media with an optional source range) out one after another from `time`, as VEGAS's default Add
/// Across Time does. `target` is the row under the pointer; None (below the last track, or no tracks) adds tracks.
/// Pictures go on the target video track (the first one from an audio row), audio on the target audio track (the
/// first one from a video row). A video's own audio takes that audio track only where it is free and unlocked,
/// otherwise a new one, and shares a group id with its video event. Stills last `still_len`.
pub fn plan_drop(tl: &Timeline, items: &[(&MediaItem, Option<(f64, f64)>)], target: Option<(bool, usize)>, time: f64, still_len: f64) -> DropPlan {
    plan_drop_as(tl, items, target, time, still_len, DropMode::AcrossTime)
}

/// The right-drag drop menu's choices (VEGAS's Add Across Time / Add Across Tracks / Add as Takes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DropMode {
    AcrossTime,
    /// Every item at the drop time, each on the next track down (video and audio counted apart).
    AcrossTracks,
    /// One event per kind, as long as the first item: the first item is the active take, the rest inactive takes.
    AsTakes,
}

/// `plan_drop` with a drop mode.
pub fn plan_drop_as(tl: &Timeline, items: &[(&MediaItem, Option<(f64, f64)>)], target: Option<(bool, usize)>, time: f64, still_len: f64, mode: DropMode) -> DropPlan {
    let target = target.filter(|&(v, i)| i < tl.tracks(v).len());
    let pick = |v: bool| match target {
        Some((tv, i)) if tv == v => i,
        Some(_) => 0,
        None => tl.tracks(v).len(),
    };
    let (vt, mut at) = (pick(true), pick(false));
    // (media, source in, source out, position)
    let mut spans = Vec::new();
    let mut t = time;
    for &(m, range) in items {
        let (a, b) = match m.media_type {
            MediaType::Unknown => continue,
            MediaType::Image => (0.0, still_len),
            _ => range.filter(|(a, b)| b > a).unwrap_or((0.0, m.duration)),
        };
        if b <= a {
            continue; // offline media of unknown length: an event would be invisible
        }
        spans.push((m, a, b, t));
        if mode == DropMode::AcrossTime {
            t += b - a;
        }
    }
    let paired = |m: &MediaItem| m.media_type == MediaType::Video && m.sample_rate.is_some();
    if let Some(tr) = tl.audio_tracks.get(at) {
        let overlaps = |pos: f64, len: f64| tr.clips.iter().any(|c| c.position < pos + len && pos < c.position + c.duration);
        if spans.iter().any(|&(m, a, b, pos)| paired(m) && (tr.locked || overlaps(pos, b - a))) {
            at = tl.audio_tracks.len();
        }
    }
    let mut events = Vec::new();
    // Across Tracks: the next item of each kind goes one track further down.
    let step = (mode == DropMode::AcrossTracks) as usize;
    let (mut kv, mut ka) = (0, 0);
    for (m, a, b, pos) in spans {
        let mut c = Clip::from_media(m, pos);
        (c.in_point, c.out_point, c.duration) = (a, b, b - a);
        if m.media_type == MediaType::Audio {
            events.push((false, at + ka, c));
            ka += step;
            continue;
        }
        if paired(m) {
            c.group = Some(uuid::Uuid::new_v4().to_string());
            let mut sound = c.clone();
            sound.id = uuid::Uuid::new_v4().to_string();
            sound.color = [100, 180, 100];
            events.push((true, vt + kv, c));
            events.push((false, at + ka, sound));
            ka += step;
        } else {
            events.push((true, vt + kv, c));
        }
        kv += step;
    }
    if mode == DropMode::AsTakes {
        // The kept video and audio events stay grouped only when every item brought both (in the same order), so
        // their take lists match and T moves picture and sound together.
        let groups = |v: bool| events.iter().filter(|e| e.0 == v).map(|e| e.2.group.clone()).collect::<Vec<_>>();
        let paired = groups(true) == groups(false) && groups(true).iter().all(Option::is_some);
        events = as_takes(events, paired);
    }
    let locked = |v: bool, i: usize| tl.tracks(v).get(i).is_some_and(|t| t.locked);
    let refused = target.is_some_and(|(v, i)| locked(v, i)) || events.iter().any(|(v, i, _)| locked(*v, *i));
    DropPlan { events, refused }
}

/// Add as Takes: the first event of each kind stays, and the later ones of that kind become its inactive takes.
fn as_takes(events: Vec<(bool, usize, Clip)>, paired: bool) -> Vec<(bool, usize, Clip)> {
    let mut kept: Vec<(bool, usize, Clip)> = Vec::new();
    for (v, i, c) in events {
        match kept.iter_mut().find(|e| e.0 == v) {
            Some(e) => e.2.takes.push(Take { media_id: c.media_id.unwrap_or_default(), in_point: c.in_point, name: c.name }),
            None => kept.push((v, i, c)),
        }
    }
    if !paired {
        kept.iter_mut().for_each(|e| e.2.group = None);
    }
    kept
}

/// Add the events of a `plan_drop`, creating the tracks it needs, with `auto_crossfade` when `xfade`. Returns the
/// new event ids (none when refused).
pub fn apply_drop(project: &mut Project, plan: DropPlan, xfade: bool) -> Vec<String> {
    if plan.refused {
        return Vec::new();
    }
    let before = xfade.then(|| project.clone());
    let tl = &mut project.timeline;
    let mut ids = Vec::new();
    for (v, i, c) in plan.events {
        while tl.tracks(v).len() <= i {
            tl.add_track(v);
        }
        ids.push(c.id.clone());
        let clips = &mut tl.tracks_mut(v)[i].clips;
        clips.push(c);
        clips.sort_by(|a, b| a.position.total_cmp(&b.position));
    }
    if let Some(b) = before {
        auto_crossfade(project, &b, &ids);
    }
    let done = !ids.is_empty();
    changed(project, done);
    ids
}

/// Drop pool media `idxs` (each with source range `range`) on `target` at `time`: `plan_drop` + `apply_drop`.
pub fn drop_media(project: &mut Project, idxs: &[usize], range: Option<(f64, f64)>, target: Option<(bool, usize)>, time: f64, still_len: f64, xfade: bool) -> Vec<String> {
    drop_media_as(project, idxs, range, target, time, still_len, xfade, DropMode::AcrossTime)
}

/// `drop_media` as the drop menu's `mode` says: Add as Takes over an event adds the media to it (`add_takes_to`),
/// anything else is `plan_drop_as` + `apply_drop`. The new or changed event ids (none when refused).
#[allow(clippy::too_many_arguments)]
pub fn drop_media_as(project: &mut Project, idxs: &[usize], range: Option<(f64, f64)>, target: Option<(bool, usize)>, time: f64, still_len: f64, xfade: bool, mode: DropMode) -> Vec<String> {

    let pool = project.media_pool.clone();
    let items: Vec<_> = idxs.iter().filter_map(|&i| pool.get(i)).map(|m| (m, range)).collect();
    if let Some(id) = target.filter(|_| mode == DropMode::AsTakes).and_then(|t| add_takes_to(project, t, time, &items)) {
        return vec![id];
    }
    let plan = plan_drop_as(&project.timeline, &items, target, time, still_len, mode);
    apply_drop(project, plan, xfade)
}

// ---- R17a: Synchronize and Lay Out Tracks ----

/// Selected events as Tools › Multicamera › Synchronize ... moves them.
#[derive(Clone, Debug, PartialEq)]
pub struct SyncUnit {
    /// The events that move together: a selected event with its group mates, or with Ignore Event Grouping only the
    /// selected events of its group.
    pub ids: Vec<String>,
    /// The event whose media is read (the first `usable` one, selected ones first; it may be an unselected group mate,
    /// the sound of a selected video event).
    pub key: Option<String>,
    /// It has a selected event on the topmost track that has one: it stays, and the others are matched to it.
    pub reference: bool,
}

/// The selection `sel` as units, top track first (VEGAS aligns to the topmost track's events). `usable(event, is
/// video)` says whether an event's media can be matched.
pub fn sync_units(p: &Project, sel: &[String], ignore_grouping: bool, usable: impl Fn(&Clip, bool) -> bool) -> Vec<SyncUnit> {
    let nv = p.timeline.video_tracks.len();
    let row = |r: &ClipRef| if r.is_video { r.track_idx } else { nv + r.track_idx };
    let clip = |r: &ClipRef| &p.timeline.tracks(r.is_video)[r.track_idx].clips[r.clip_idx];
    let mut refs = p.find_clips(sel);
    refs.sort_by(|a, b| row(a).cmp(&row(b)).then(clip(a).position.total_cmp(&clip(b).position)));
    let Some(top) = refs.first().map(row) else { return Vec::new() };
    let mut units: Vec<SyncUnit> = Vec::new();
    for r in &refs {
        let id = &clip(r).id;
        if units.iter().any(|u| u.ids.contains(id)) {
            continue;
        }
        let mates = expand_groups(p, std::slice::from_ref(id));
        let ids: Vec<String> = if ignore_grouping { mates.iter().filter(|m| sel.contains(m)).cloned().collect() } else { mates.clone() };
        let (picked, rest): (Vec<&String>, Vec<&String>) = mates.iter().partition(|m| sel.contains(m));
        let key = picked.into_iter().chain(rest).find(|k| p.find_clip(k).is_some_and(|r| usable(clip(&r), r.is_video))).cloned();
        let reference = refs.iter().any(|q| row(q) == top && ids.contains(&clip(q).id));
        units.push(SyncUnit { ids, key, reference });
    }
    units
}

/// Any of events `ids` is locked or on a locked track (Synchronize leaves the unit where it is).
pub fn sync_locked(p: &Project, ids: &[String]) -> bool {
    p.find_clips(ids).iter().any(|r| {
        let t = &p.timeline.tracks(r.is_video)[r.track_idx];
        t.locked || t.clips[r.clip_idx].locked
    })
}

/// What Synchronize does with what it found.
#[derive(Debug, Default, PartialEq)]
pub struct SyncPlan {
    /// (unit ids, seconds) to move.
    pub moves: Vec<(Vec<String>, f64)>,
    /// Each unit left where it is: its key event's name and why.
    pub refused: Vec<String>,
    /// Units now aligned (moved, or already in place).
    pub aligned: usize,
}

/// Synchronize's plan from the non-reference `units`' offsets (`results`, in order, or why there is none). Locked
/// units stay. One that would start before the timeline's start moves everything later instead, the references too;
/// with a reference locked it is refused.
pub fn plan_sync(p: &Project, units: &[SyncUnit], results: Vec<Result<f64, String>>) -> SyncPlan {
    let name = |u: &SyncUnit| u.key.iter().chain(&u.ids).find_map(|id| p.clip(id)).map_or_else(String::new, |c| format!("\"{}\"", c.name));
    let start = |ids: &[String]| p.find_clips(ids).iter().map(|r| p.timeline.tracks(r.is_video)[r.track_idx].clips[r.clip_idx].position).fold(f64::INFINITY, f64::min);
    let (refs, others): (Vec<&SyncUnit>, Vec<&SyncUnit>) = units.iter().partition(|u| u.reference);
    let mut plan = SyncPlan::default();
    let mut moves = Vec::new();
    for (u, r) in others.iter().zip(results) {
        match r {
            Err(e) => plan.refused.push(format!("{} ({e})", name(u))),
            Ok(_) if sync_locked(p, &u.ids) => plan.refused.push(format!("{} (locked)", name(u))),
            Ok(d) => {
                plan.aligned += 1;
                if d.abs() > 1e-9 {
                    moves.push((u.ids.clone(), d, name(u)));
                }
            }
        }
    }
    let early = moves.iter().map(|(ids, d, _)| -(start(ids) + d)).fold(0.0, f64::max);
    if early > 1e-9 && refs.iter().any(|u| sync_locked(p, &u.ids)) {
        moves.retain(|(ids, d, n)| {
            let fits = start(ids) + d >= -1e-9;
            if !fits {
                plan.refused.push(format!("{n} (it would start before the timeline does)"));
                plan.aligned -= 1;
            }
            fits
        });
    } else if early > 1e-9 {
        moves.iter_mut().for_each(|m| m.1 += early);
        moves.extend(refs.iter().map(|u| (u.ids.clone(), early, String::new())));
    }
    plan.moves = moves.into_iter().map(|(ids, d, _)| (ids, d)).collect();
    plan
}

/// Synchronize's `moves` as one edit: each unit moves as a Move drag of it would (`lock_env`: Lock Envelopes to
/// Events, when nothing ripples), with automatic crossfades when `xfade`. Each move leaves a ripple (its tracks from
/// where it was, by what it moved), applied with `mode` (Auto Ripple) or returned for Post-Edit Ripple; it never moves
/// an event of `units`, so a ripple cannot undo the alignment. None when nothing moved.
pub fn apply_sync(p: &mut Project, units: &[SyncUnit], moves: &[(Vec<String>, f64)], mode: Option<RippleMode>, lock_env: bool, xfade: bool) -> Option<Vec<RippleOp>> {
    let before = p.clone();
    let keep: Vec<String> = units.iter().flat_map(|u| u.ids.clone()).collect();
    let mut ops = Vec::new();
    for (ids, dt) in moves {
        let refs = p.find_clips(ids);
        let from = refs.iter().map(|r| p.timeline.tracks(r.is_video)[r.track_idx].clips[r.clip_idx].position).fold(f64::INFINITY, f64::min);
        let mut tracks: Vec<String> = refs.iter().map(|r| p.timeline.tracks(r.is_video)[r.track_idx].id.clone()).collect();
        tracks.sort();
        tracks.dedup();
        if move_clips(p, ids, *dt, 0, lock_env && mode.is_none()) {
            ops.push(RippleOp { from, delta: *dt, tracks, exclude: keep.clone(), keys_from: None });
        }
    }
    if ops.is_empty() {
        return None;
    }
    if let Some(m) = mode {
        ripple_all(p, &ops, m);
    }
    if xfade {
        let moved: Vec<String> = moves.iter().flat_map(|m| m.0.clone()).collect();
        auto_crossfade(p, &before, &moved);
    }
    Some(ops)
}

/// Project Media › Lay Out Tracks Using Media Timecode / Date/Time Stamp: media `idxs` added at once, each on new
/// tracks of its own (VEGAS gives each tape its tracks; Milan Pro keeps no tape names, so each file gets its own),
/// placed so their `stamp`s line up, the earliest at `at`. Media without a stamp (and stills) are skipped. Returns
/// the new event ids and how many were skipped.
pub fn lay_out_tracks(project: &mut Project, idxs: &[usize], at: f64, stamp: impl Fn(&MediaItem) -> Option<f64>) -> (Vec<String>, usize) {
    let pool = project.media_pool.clone();
    let picked: Vec<(&MediaItem, f64)> = idxs.iter().filter_map(|&i| pool.get(i)).filter(|m| matches!(m.media_type, MediaType::Video | MediaType::Audio)).filter_map(|m| Some((m, stamp(m)?))).collect();
    let skipped = idxs.len() - picked.len();
    let Some(t0) = picked.iter().map(|p| p.1).reduce(f64::min) else { return (Vec::new(), skipped) };
    let items: Vec<_> = picked.iter().map(|(m, _)| (*m, None)).collect();
    let mut plan = plan_drop_as(&project.timeline, &items, None, at, 0.0, DropMode::AcrossTracks);
    for (_, _, c) in &mut plan.events {
        c.position += picked.iter().find(|(m, _)| c.media_id.as_ref() == Some(&m.id)).map_or(0.0, |p| p.1 - t0);
    }
    (apply_drop(project, plan, false), skipped)
}

/// Fade In ▸ / Fade Out ▸ curve of event `id`.
pub fn set_fade_curve(project: &mut Project, id: &str, is_in: bool, curve: Curve) -> bool {
    edit_clips(project, &[id.to_string()], |c| {
        let f = if is_in { &mut c.fade_in_curve } else { &mut c.fade_out_curve };
        std::mem::replace(f, curve) != curve
    })
}

// ---- Takes ----

/// Make take `t` the event's own media, in point and name; returns the take it replaced.
fn make_active(c: &mut Clip, t: Take) -> Take {
    let old = Take { media_id: c.media_id.take().unwrap_or_default(), in_point: c.in_point, name: std::mem::take(&mut c.name) };
    c.media_id = Some(t.media_id).filter(|m| !m.is_empty());
    (c.in_point, c.name) = (t.in_point, t.name);
    sync_out(c);
    old
}

/// Next Take / Previous Take on one event: the active take and `takes` form a ring in list order. The event keeps
/// its place and length. False without takes.
/// A take shorter than the event loops (with Loop off it holds its last frame and is silent past its end).
pub fn cycle_take(c: &mut Clip, forward: bool) -> bool {
    let t = match (forward, c.takes.is_empty()) {
        (_, true) => return false,
        (true, _) => c.takes.remove(0),
        (false, _) => c.takes.pop().expect("takes"),
    };
    let old = make_active(c, t);
    if forward { c.takes.push(old) } else { c.takes.insert(0, old) }
    let n = c.takes.len() as u32 + 1;
    c.take_no = (c.take_no % n + if forward { 1 } else { n - 1 }) % n;
    true
}

/// R17b: the event's takes in their fixed order (Take 1, 2, ...: the cameras of a multicamera event), and the active
/// one's place in it. An empty take ("(no camera)") has no media.
pub fn take_list(c: &Clip) -> (Vec<Take>, usize) {
    let k = c.take_no as usize % (c.takes.len() + 1);
    let mut ring = vec![Take { media_id: c.media_id.clone().unwrap_or_default(), in_point: c.in_point, name: c.name.clone() }];
    ring.extend(c.takes.iter().cloned());
    ring.rotate_right(k);
    (ring, k)
}

/// R17b: the event's takes become `list` (fixed order), take `k` active.
fn set_take_list(c: &mut Clip, mut list: Vec<Take>, k: usize) {
    list.rotate_left(k);
    let first = list.remove(0);
    make_active(c, first);
    (c.takes, c.take_no) = (list, k as u32);
}

/// R17b: take `k` (fixed order) becomes active; false when it is already, or there is no such take.
pub fn set_take(c: &mut Clip, k: usize) -> bool {
    let (list, now) = take_list(c);
    if k >= list.len() || k == now {
        return false;
    }
    set_take_list(c, list, k);
    true
}

/// Make inactive take `k` active, keeping the ring order (Event Properties' active take list).
pub fn choose_take(c: &mut Clip, k: usize) {
    if k < c.takes.len() {
        for _ in 0..=k {
            cycle_take(c, true);
        }
    }
}

/// Media `m` from `in_point` becomes the event's active take; the old one stays as an inactive take, next in the
/// ring (VEGAS appends the new take and makes it active).
pub fn add_as_take(c: &mut Clip, m: &MediaItem, in_point: f64) {
    let old = make_active(c, Take { media_id: m.id.clone(), in_point, name: m.name.clone() });
    c.takes.insert(0, old);
}

/// Add as Takes dropped onto an event: the event under `time` on track `(v, i)`, and its group mates under `time`
/// (the other half of an A/V pair), get each item that has a stream for all of them as their new active take, in
/// turn (the last one ends up active), so their takes stay matched. The event id; None when no unlocked event is
/// there, a mate is locked or no item fits, and the drop then makes a new event.
pub fn add_takes_to(project: &mut Project, (v, i): (bool, usize), time: f64, items: &[(&MediaItem, Option<(f64, f64)>)]) -> Option<String> {
    let covers = |c: &Clip| time >= c.position && time < c.position + c.duration;
    let tl = &project.timeline;
    let tr = tl.tracks(v).get(i).filter(|t| !t.locked)?;
    let c = tr.clips.iter().rev().find(|c| !c.locked && covers(c))?;
    let id = c.id.clone();
    // (kind, event id, locked) of the events that get the takes.
    let mut mates = vec![(v, id.clone(), false)];
    if let Some(g) = &c.group {
        for kind in [true, false] {
            for t in tl.tracks(kind) {
                let mine = t.clips.iter().filter(|m| m.group.as_ref() == Some(g) && m.id != id && covers(m));
                mates.extend(mine.map(|m| (kind, m.id.clone(), t.locked || m.locked)));
            }
        }
    }
    let fits = |m: &MediaItem, v: bool| if v { matches!(m.media_type, MediaType::Video | MediaType::Image) } else { m.sample_rate.is_some() };
    let items: Vec<_> = items.iter().filter(|(m, _)| mates.iter().all(|k| fits(m, k.0))).collect();
    if items.is_empty() || mates.iter().any(|m| m.2) {
        return None;
    }
    for (_, mid, _) in &mates {
        let r = project.find_clip(mid)?;
        let c = &mut project.timeline.tracks_mut(r.is_video)[r.track_idx].clips[r.clip_idx];
        for (m, range) in &items {
            add_as_take(c, m, range.map_or(0.0, |r| r.0));
        }
    }
    changed(project, true).then_some(id)
}

/// `f` on the unlocked events `ids` on unlocked tracks; true (and modified) when it changed any.
fn edit_clips(project: &mut Project, ids: &[String], mut f: impl FnMut(&mut Clip) -> bool) -> bool {
    let mut done = false;
    for c in tracks_mut(&mut project.timeline).filter(|t| !t.locked).flat_map(|t| &mut t.clips).filter(|c| !c.locked && ids.contains(&c.id)) {
        done |= f(c);
    }
    changed(project, done)
}

/// Next Take (T) / Previous Take (Shift+T) on events `ids`.
pub fn next_take(project: &mut Project, ids: &[String], forward: bool) -> bool {
    edit_clips(project, ids, |c| cycle_take(c, forward))
}

/// Take › Delete Active: the next take becomes active; refused (false) on an event without other takes.
pub fn delete_active_take(project: &mut Project, ids: &[String]) -> bool {
    edit_clips(project, ids, |c| {
        let any = !c.takes.is_empty();
        if any {
            let t = c.takes.remove(0);
            make_active(c, t);
            c.take_no %= c.takes.len() as u32 + 1;
        }
        any
    })
}

/// Take Chooser OK on event `id`: take `k` (0 the active take, then the inactive takes in ring order) becomes active
/// (Choose Active...) or is deleted (Delete...; deleting the active take makes the next one active, and an event's
/// only take cannot go).
pub fn take_chooser(project: &mut Project, id: &str, k: usize, delete: bool) -> bool {
    edit_clips(project, &[id.to_string()], |c| {
        if k > c.takes.len() || (k == 0 && (!delete || c.takes.is_empty())) {
            return false;
        }
        match (k, delete) {
            (0, _) => {
                let t = c.takes.remove(0);
                make_active(c, t);
                c.take_no %= c.takes.len() as u32 + 1;
            }
            (_, false) => choose_take(c, k - 1),
            (_, true) => {
                // The fixed order loses the take `k` steps on from the active one.
                let (n, no) = (c.takes.len() as u32 + 1, c.take_no % (c.takes.len() as u32 + 1));
                c.takes.remove(k - 1);
                c.take_no = if (no + k as u32) % n < no { no - 1 } else { no };
            }
        }
        true
    })
}

/// Event `c` plays media `id`, as its active take or an inactive one.
pub fn uses_media(c: &Clip, id: &str) -> bool {
    c.media_id.as_deref() == Some(id) || c.takes.iter().any(|t| t.media_id == id)
}

/// Media `id` leaves the pool: inactive takes of it go, an event whose active take it is moves on to its next take,
/// and an event with no other take is deleted (it would play black or silence).
pub fn forget_media(tl: &mut Timeline, id: &str) {
    for tr in tracks_mut(tl) {
        tr.clips.retain_mut(|c| {
            if !uses_media(c, id) {
                return true;
            }
            // In the fixed take order: the active take stays, or the next one left after it takes over.
            let (list, k) = take_list(c);
            let k = list.iter().take(k).filter(|t| t.media_id != id).count();
            let left: Vec<Take> = list.into_iter().filter(|t| t.media_id != id).collect();
            if left.is_empty() {
                return false;
            }
            let k = k % left.len();
            set_take_list(c, left, k);
            true
        });
    }
}

// ---- Multicamera (R17b) ----

/// A multicamera event (VEGAS keeps the cameras as takes): one with more than one take.
pub fn is_multicam(c: &Clip) -> bool {
    !c.takes.is_empty()
}

/// Tracks of kind `video` whose unmuted multicamera events overlap [a, b) (a point when b <= a), topmost first, with
/// the last such event's index. Video tracks out of the preview (hidden, muted or not soloed) are skipped.
fn multicam_tracks(tl: &Timeline, video: bool, a: f64, b: f64) -> impl Iterator<Item = (usize, usize)> + '_ {
    let hit = move |c: &Clip| !c.muted && is_multicam(c) && c.position + c.duration > a + 1e-9 && if b > a { c.position < b - 1e-9 } else { c.position <= a + 1e-9 };
    let any_solo = tl.any_solo(true);
    tl.tracks(video).iter().enumerate().filter(move |(_, t)| !video || (t.visible && tl.plays(t, any_solo))).filter_map(move |(i, t)| t.clips.iter().rposition(hit).map(|j| (i, j)))
}

/// The topmost multicamera event of kind `video` playing at `t` (track, event index): with overlapping video it is
/// the one the multicamera preview shows, and the one a take choice edits (VEGAS).
pub fn multicam_at(tl: &Timeline, video: bool, t: f64) -> Option<(usize, usize)> {
    multicam_tracks(tl, video, t, t).next()
}

/// Two events next to each other that are one event split (same takes, active take and settings, each take's source
/// running on across the cut, no fade or transition between them): a take choice joins them back.
fn joins(x: &Clip, y: &Clip) -> bool {
    let bare = |c: &Clip| Clip {
        id: String::new(),
        media_id: None,
        position: 0.0,
        duration: 0.0,
        in_point: 0.0,
        out_point: 0.0,
        name: String::new(),
        fade_in: 0.0,
        fade_out: 0.0,
        transition: None,
        transition_out: None,
        takes: Vec::new(),
        take_no: 0,
        normalized: None,
        ..c.clone()
    };
    let ((lx, kx), (ly, ky)) = (take_list(x), take_list(y));
    let runs_on = |a: &Take, b: &Take| a.media_id == b.media_id && (a.in_point + x.duration * x.rate - b.in_point).abs() < 1e-6;
    (x.position + x.duration - y.position).abs() < 1e-9
        && (kx, lx.len()) == (ky, ly.len())
        && lx.iter().zip(&ly).all(|(a, b)| runs_on(a, b))
        && !x.reverse
        && !x.locked
        && x.fade_out == 0.0
        && y.fade_in == 0.0
        && x.transition_out.is_none()
        && y.transition.is_none()
        && x.envelopes.is_empty()
        && x.pan_crop.keys.len() <= 1
        && x.masks.is_empty()
        && bare(x) == bare(y)
}

/// Join every pair of events on `t` that `joins`, meeting in [a, b].
fn join_split(t: &mut Track, a: f64, b: f64) {
    t.clips.sort_by(|x, y| x.position.total_cmp(&y.position));
    let mut i = 0;
    while i + 1 < t.clips.len() {
        let at = t.clips[i + 1].position;
        if at >= a - 1e-9 && at <= b + 1e-9 && joins(&t.clips[i], &t.clips[i + 1]) {
            let y = t.clips.remove(i + 1);
            let x = &mut t.clips[i];
            x.duration += y.duration;
            (x.fade_out, x.fade_out_curve, x.transition_out) = (y.fade_out, y.fade_out_curve, y.transition_out);
            sync_out(x);
        } else {
            i += 1;
        }
    }
}

/// Multicamera editing (Tools › Multicamera › Enable Multicamera Editing): take `k` (from 0) plays from `t` in the
/// topmost multicamera event of kind `video` there, split at `t` (VEGAS: choosing a take cuts at the cursor, also
/// while playing). With a time selection `range`, every multicamera event in it on the topmost multicamera track
/// there plays take `k`, split at the selection's edges; events without that take (or where it is empty, "(no
/// camera)") keep theirs. Events left playing one take on across a cut join again. False when nothing changed.
pub fn switch_take(p: &mut Project, video: bool, t: f64, range: Option<(f64, f64)>, k: usize) -> bool {
    let (a, b) = range.unwrap_or((t, t));
    let Some((ti, j)) = multicam_tracks(&p.timeline, video, a, b).next() else { return false };
    let mut tr = p.timeline.tracks(video)[ti].clone();
    if tr.locked {
        return false;
    }
    let cut = |tr: &mut Track, i: usize, e: f64| {
        if let Some((l, r)) = split_clip(&tr.clips[i], e).filter(|_| !tr.clips[i].locked) {
            tr.clips[i] = l;
            tr.clips.insert(i + 1, r);
        }
    };
    // A point choice cuts the event under it and runs to its end.
    let (a, b) = match range {
        Some(r) => {
            for e in [r.0, r.1] {
                if let Some(i) = tr.clips.iter().position(|c| is_multicam(c) && split_clip(c, e).is_some()) {
                    cut(&mut tr, i, e);
                }
            }
            r
        }
        None => {
            let end = tr.clips[j].position + tr.clips[j].duration;
            cut(&mut tr, j, t);
            (t.max(tr.clips[j].position), end)
        }
    };
    let mut done = false;
    for c in tr.clips.iter_mut().filter(|c| is_multicam(c) && !c.locked && !c.muted && c.position >= a - 1e-9 && c.position + c.duration <= b + 1e-9) {
        if take_list(c).0.get(k).is_some_and(|t| !t.media_id.is_empty()) {
            done |= set_take(c, k);
        }
    }
    if !done {
        return false;
    }
    join_split(&mut tr, a, b);
    p.timeline.tracks_mut(video)[ti] = tr;
    changed(p, true)
}

/// Tools › Multicamera › Create Multicamera Track: of each kind, the selected events `ids` on two or more tracks
/// become one track of multicamera events, the tracks their takes in track order (Take 1 the topmost), split wherever
/// one of them starts or ends; a track with nothing there gives an empty take, "(no camera)". The topmost track gets
/// them, the others lose the events (a track left empty goes), and the active take is the topmost one with something
/// there. The new events' ids; Err says why nothing changed.
// shortcut: a take plays at its event's rate, pan/crop and FX, the topmost camera's there; cameras edited differently
// (other rates, reversed) lose that. Keep per-take settings if VEGAS-style per-take attributes are asked for.
pub fn create_multicam(p: &mut Project, ids: &[String]) -> Result<Vec<String>, String> {
    let before = p.clone();
    let made = make_multicam(p, ids);
    if made.is_err() {
        *p = before; // a locked audio event found after the video was made
    }
    made
}

fn make_multicam(p: &mut Project, ids: &[String]) -> Result<Vec<String>, String> {
    let mut made = Vec::new();
    let mut gone = Vec::new();
    for video in [true, false] {
        let tl = &p.timeline;
        let cams: Vec<usize> = (0..tl.tracks(video).len()).filter(|&i| tl.tracks(video)[i].clips.iter().any(|c| ids.contains(&c.id))).collect();
        if cams.len() < 2 {
            continue;
        }
        let picked = |i: usize| tl.tracks(video)[i].clips.iter().filter(|c| ids.contains(&c.id));
        if let Some(c) = cams.iter().flat_map(|&i| picked(i)).find(|c| c.locked) {
            return Err(format!("\"{}\" is locked", c.name));
        }
        if let Some(&i) = cams.iter().find(|&&i| tl.tracks(video)[i].locked) {
            return Err(format!("track \"{}\" is locked", tl.tracks(video)[i].name));
        }
        let mut cuts: Vec<f64> = cams.iter().flat_map(|&i| picked(i)).flat_map(|c| [c.position, c.position + c.duration]).collect();
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|x, y| (*x - *y).abs() < 1e-9);
        // Each camera's events cut at every edge: each piece then covers one stretch between two edges.
        let pieces: Vec<Vec<Clip>> = cams
            .iter()
            .map(|&i| {
                let mut out = Vec::new();
                for c in picked(i) {
                    let mut rest = c.clone();
                    for &e in &cuts {
                        if let Some((l, r)) = split_clip(&rest, e) {
                            out.push(l);
                            rest = r;
                        }
                    }
                    out.push(rest);
                }
                out
            })
            .collect();
        let mut events = Vec::new();
        for w in cuts.windows(2) {
            let mid = (w[0] + w[1]) * 0.5;
            let at: Vec<Option<&Clip>> = pieces.iter().map(|ps| ps.iter().rev().find(|c| c.position <= mid && mid < c.position + c.duration)).collect();
            let Some(k) = at.iter().position(Option::is_some) else { continue };
            let list = at.iter().map(|c| c.map_or(Take { media_id: String::new(), in_point: 0.0, name: String::new() }, |c| Take { media_id: c.media_id.clone().unwrap_or_default(), in_point: c.in_point, name: c.name.clone() })).collect();
            let mut e = Clip { group: None, takes: Vec::new(), take_no: 0, ..at[k].expect("found").clone() };
            set_take_list(&mut e, list, k);
            made.push(e.id.clone());
            events.push(e);
        }
        let tracks = p.timeline.tracks_mut(video);
        for &i in &cams {
            tracks[i].clips.retain(|c| !ids.contains(&c.id));
        }
        let top = &mut tracks[cams[0]];
        top.clips.extend(events);
        top.clips.sort_by(|x, y| x.position.total_cmp(&y.position));
        gone.extend(cams[1..].iter().map(|&i| &tracks[i]).filter(|t| t.clips.is_empty()).map(|t| t.id.clone()));
    }
    if made.is_empty() {
        return Err("select events on two or more tracks of one kind (one track per camera)".into());
    }
    delete_tracks(p, &gone);
    p.mark_modified();
    Ok(made)
}

/// Tools › Multicamera › Expand to Multiple Tracks on the multicamera tracks `ids`: take `k` of each multicamera event
/// goes to the `k`th track (Take 1 stays on the multicamera track, the others go on new tracks below it, named after
/// their takes). The active takes play; with `keep_unused` the others are kept as muted events (so the multicamera
/// track can be made again), else they go, as VEGAS asks. False when none of the tracks (unlocked) has a multicamera
/// event.
pub fn expand_multicam(p: &mut Project, ids: &[String], keep_unused: bool) -> bool {
    let mut done = false;
    for id in ids {
        let Some((video, ti)) = p.find_track(id) else { continue };
        let tr = &p.timeline.tracks(video)[ti];
        let n = tr.clips.iter().filter(|c| is_multicam(c)).map(|c| c.takes.len() + 1).max().unwrap_or(0);
        if tr.locked || n < 2 {
            continue;
        }
        let mut lanes: Vec<Track> = (0..n).map(|_| Track { id: uuid::Uuid::new_v4().to_string(), clips: Vec::new(), armed: false, ..tr.clone() }).collect();
        (lanes[0].id, lanes[0].name) = (tr.id.clone(), tr.name.clone());
        for c in &tr.clips {
            if !is_multicam(c) {
                lanes[0].clips.push(c.clone());
                continue;
            }
            let (list, active) = take_list(c);
            for (k, t) in list.into_iter().enumerate().filter(|(_, t)| !t.media_id.is_empty()) {
                if k > 0 && lanes[k].name == tr.name {
                    lanes[k].name = t.name.clone();
                }
                if k == active || keep_unused {
                    let mut e = Clip { id: if k == active { c.id.clone() } else { uuid::Uuid::new_v4().to_string() }, muted: c.muted || k != active, ..c.clone() };
                    set_take_list(&mut e, vec![t], 0);
                    lanes[k].clips.push(e);
                }
            }
        }
        let tracks = p.timeline.tracks_mut(video);
        tracks.splice(ti..=ti, lanes);
        done = true;
    }
    if done {
        tidy_track_groups(&mut p.timeline);
    }
    changed(p, done)
}

// ---- Empty events, subclips, attribute paste (R3a) ----

/// Insert › Empty Event: an event with no media over the time selection `(a, b)` on each unlocked track `tracks` (by
/// id). Returns the new events' ids.
pub fn insert_empty(project: &mut Project, tracks: &[String], (a, b): (f64, f64)) -> Vec<String> {
    let mut ids = Vec::new();
    if b <= a {
        return ids;
    }
    for t in tracks_mut(&mut project.timeline).filter(|t| !t.locked && tracks.contains(&t.id)) {
        let c = Clip::new("", a, b - a);
        ids.push(c.id.clone());
        t.clips.push(c);
        t.clips.sort_by(|x, y| x.position.total_cmp(&y.position));
    }
    changed(project, !ids.is_empty());
    ids
}

/// A subclip of media `m`: `len` seconds of its file from `start` (in `m`'s own time), added to Project Media and
/// named after it. A subclip of a subclip is a subclip of the same file. Its id; None for a stretch under a frame long
/// or media without a timed file.
pub fn create_subclip(project: &mut Project, media_id: &str, start: f64, len: f64) -> Option<String> {
    let m = project.media_pool.iter().find(|m| m.id == media_id)?;
    let ok = m.generator.is_none() && matches!(m.media_type, MediaType::Video | MediaType::Audio) && !m.is_offline();
    let start = start.clamp(0.0, m.duration);
    let len = len.min(m.duration - start);
    if !ok || len < 1.0 / 1000.0 {
        return None;
    }
    let n = project.media_pool.iter().filter(|s| s.subclip.is_some() && s.path == m.path).count() + 1;
    let sub = MediaItem {
        id: uuid::Uuid::new_v4().to_string(),
        name: format!("{} Subclip {n}", m.name),
        duration: len,
        subclip: Some(Subclip { parent: m.subclip.as_ref().map_or(m.id.clone(), |s| s.parent.clone()), start: m.file_time(start) }),
        rel_path: None,
        ..m.clone()
    };
    let id = sub.id.clone();
    project.media_pool.push(sub);
    project.mark_modified();
    Some(id)
}

/// The event menu's Create Subclip: a subclip of the source span event `id` plays, which the event then uses (it
/// looks and sounds the same). A span that wraps past the media end (a looped event) makes a subclip of the part
/// inside the media and the event keeps its media. The subclip's id.
pub fn subclip_event(project: &mut Project, id: &str) -> Option<String> {
    let r = project.find_clip(id)?;
    let c = project.clip(id)?.clone();
    let (mid, len) = media_of(project, &c).map(|m| (m.id.clone(), m.duration))?;
    let (a, b) = (c.in_point, c.in_point + c.duration * c.rate);
    let fits = b <= len + 1e-6;
    let sub = create_subclip(project, &mid, a, b - a)?;
    let tr = &mut project.timeline.tracks_mut(r.is_video)[r.track_idx];
    if fits && !tr.locked && !c.locked {
        let e = &mut tr.clips[r.clip_idx];
        (e.media_id, e.in_point) = (Some(sub.clone()), 0.0);
        sync_out(e);
    }
    Some(sub)
}

/// What Paste Event Attributes carries over (Selectively Paste Event Attributes picks).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Attrs {
    pub fx: bool,
    pub pan_crop: bool,
    pub envelopes: bool,
    pub gain: bool,
    pub rate: bool,
    pub fades: bool,
}

impl Attrs {
    pub const ALL: Attrs = Attrs { fx: true, pan_crop: true, envelopes: true, gain: true, rate: true, fades: true };
}

/// Paste Event Attributes: the chosen attributes of the first copied event of each kind (video, audio) go onto the
/// unlocked events `ids` of that kind; their media, place and in point stay. FX get new instance ids; a new playback
/// rate keeps the event's length (within its media, unless it loops).
pub fn paste_attrs(project: &mut Project, from: &[(bool, usize, Clip)], ids: &[String], a: Attrs) -> bool {
    let mut done = false;
    for video in [true, false] {
        let Some(src) = from.iter().find(|c| c.0 == video).map(|c| &c.2) else { continue };
        let limits: HashMap<String, Option<f64>> =
            project.timeline.tracks(video).iter().flat_map(|t| &t.clips).filter(|c| ids.contains(&c.id)).map(|c| (c.id.clone(), media_limit(project, c))).collect();
        for t in project.timeline.tracks_mut(video).iter_mut().filter(|t| !t.locked) {
            for c in t.clips.iter_mut().filter(|c| !c.locked && ids.contains(&c.id)) {
                let before = c.clone();
                if a.fx {
                    c.fx = src.fx.iter().map(|f| Fx { id: uuid::Uuid::new_v4().to_string(), ..f.clone() }).collect();
                }
                if a.pan_crop && video {
                    c.pan_crop = src.pan_crop.clone();
                    c.masks = src.masks.clone(); // the Pan/Crop window's Mask goes with its Position
                }
                if a.envelopes {
                    c.envelopes = src.envelopes.clone();
                }
                if a.gain {
                    c.gain_db = src.gain_db;
                }
                if a.rate {
                    c.rate = src.rate;
                    if let Some(l) = limits.get(&c.id).copied().flatten() {
                        c.duration = c.duration.min((l - c.in_point) / c.rate);
                    }
                }
                if a.fades {
                    (c.fade_in, c.fade_out, c.fade_in_curve, c.fade_out_curve) = (src.fade_in, src.fade_out, src.fade_in_curve, src.fade_out_curve);
                }
                c.clamp_fades();
                sync_out(c);
                // FX always get new ids: compare without them.
                let same_fx = c.fx.len() == before.fx.len() && c.fx.iter().zip(&before.fx).all(|(x, y)| Fx { id: y.id.clone(), ..x.clone() } == *y);
                done |= !same_fx || Clip { fx: before.fx.clone(), ..c.clone() } != before;
            }
        }
    }
    changed(project, done)
}

// ---- Event Properties ----

/// Event Properties' rate: the same source span plays at `rate` (0.25..=4), so rate x length stays the same.
pub fn apply_rate(c: &mut Clip, rate: f64) {
    let span = c.duration * c.rate;
    c.rate = rate.clamp(MIN_RATE, MAX_RATE);
    c.duration = span / c.rate;
    c.clamp_fades();
    sync_out(c);
}

/// Event Properties OK: event `id` becomes `draft`, kept inside its media with its fades inside it. On a locked track
/// only the event's Lock switch changes (as `toggle_clip_lock`). False when nothing changes (no undo step then).
pub fn apply_props(project: &mut Project, id: &str, mut draft: Clip) -> bool {
    let Some(r) = project.find_clip(id) else { return false };
    let tr = &mut project.timeline.tracks_mut(r.is_video)[r.track_idx];
    let (locked, cur) = (tr.locked, &mut tr.clips[r.clip_idx]);
    if locked {
        let done = cur.locked != draft.locked;
        cur.locked = draft.locked;
        return changed(project, done);
    }
    if *cur == draft {
        return false;
    }
    // Normalize turned on is measured again, as from the Switches menu.
    if (draft.normalize && !cur.normalize) || (draft.auto_normalize && !cur.auto_normalize) {
        draft.normalized = None;
    }

    let frame = 1.0 / project.settings.frame_rate.max(1.0);
    if let Some(l) = media_limit(project, &draft) {
        draft.in_point = draft.in_point.min(l - frame * draft.rate).max(0.0);
        // An event already past its media end (Loop turned off) keeps its length; it may not grow further.
        let had = project.clip(id).map_or(0.0, |c| c.duration);
        draft.duration = draft.duration.min(((l - draft.in_point) / draft.rate).max(had));
    }
    (draft.position, draft.duration) = (draft.position.max(0.0), draft.duration.max(frame));
    draft.clamp_fades();
    sync_out(&mut draft);
    let clips = &mut project.timeline.tracks_mut(r.is_video)[r.track_idx].clips;
    clips[r.clip_idx] = draft;
    clips.sort_by(|a, b| a.position.total_cmp(&b.position));
    changed(project, true)
}

// ---- Envelopes ----

/// Two points closer than this in time are one place.
const KEY_EPS: f64 = 1e-9;
/// The least time between a dragged point and its neighbours.
const KEY_GAP: f64 = 1e-6;

/// The value of a new envelope's point (Set to Default, Reset All).
pub fn env_default(kind: EnvKind) -> f32 {
    kind.neutral()
}

/// An envelope's values: dB for Volume (+6 at the lane top, -120 is silence), -1 (left) ..= 1 (right) for Pan, -1
/// (bottom colour) ..= 1 (top colour) for Fade to Color, -1 ..= 10 (-100% ..= 1000%) for Velocity, 0..=1 for the rest.
pub fn env_range(kind: EnvKind) -> (f32, f32) {
    match kind {
        EnvKind::Volume => (-120.0, 6.0),
        EnvKind::Pan | EnvKind::FadeToColor => (-1.0, 1.0),
        EnvKind::Velocity => (crate::engine::velocity::MIN, crate::engine::velocity::MAX),
        EnvKind::Supersampling => (0.0, 8.0),
        _ => (0.0, 1.0),
    }
}

/// `v` inside the range; a Mute point is on (1) or off (0).
pub fn env_clamp(kind: EnvKind, v: f32) -> f32 {
    let (lo, hi) = env_range(kind);
    let v = v.clamp(lo, hi);
    if kind == EnvKind::Mute { v.round() } else { v }
}

/// The track envelopes Milan Pro plays: Volume, Pan and Mute on audio tracks, Mute, Composite Level and Fade to
/// Color on video tracks.
pub fn env_fits(kind: EnvKind, video: bool) -> bool {
    matches!((kind, video), (EnvKind::Volume | EnvKind::Pan | EnvKind::Mute, false) | (EnvKind::Mute | EnvKind::CompositeLevel | EnvKind::FadeToColor, true))
}

/// Height of value `v` in the envelope's lane, 0 (bottom) ..= 1 (top). Volume: 0 dB at 0.7, linear up to +6 dB at the
/// top and 0.7 x 10^(dB / 40) below; Pan: left at the top; Velocity: -100% ..= 100% over the lower half, 100% ..=
/// 1000% over the upper; the rest linear over their range.
pub fn lane_frac(kind: EnvKind, v: f32) -> f32 {
    let (lo, hi) = env_range(kind);
    match kind {
        EnvKind::Volume if v >= 0.0 => 0.7 + 0.3 * v.min(hi) / hi,
        EnvKind::Volume => 0.7 * 10f32.powf(v.max(lo) / 40.0),
        EnvKind::Pan => (1.0 - v.clamp(lo, hi)) / 2.0,
        EnvKind::Velocity if v >= 1.0 => 0.5 + 0.5 * (v.min(hi) - 1.0) / (hi - 1.0),
        EnvKind::Velocity => (v.max(lo) - lo) / (1.0 - lo) * 0.5,
        _ => ((v - lo) / (hi - lo)).clamp(0.0, 1.0),
    }
}

/// The value at lane height `f` (the inverse of `lane_frac`).
pub fn lane_value(kind: EnvKind, f: f32) -> f32 {
    let f = f.clamp(0.0, 1.0);
    let (lo, hi) = env_range(kind);
    match kind {
        EnvKind::Volume if f >= 0.7 => (f - 0.7) / 0.3 * hi,
        EnvKind::Volume => (40.0 * (f / 0.7).log10()).max(lo),
        EnvKind::Pan => 1.0 - 2.0 * f,
        EnvKind::Velocity if f >= 0.5 => 1.0 + (f - 0.5) * 2.0 * (hi - 1.0),
        EnvKind::Velocity => lo + f * 2.0 * (1.0 - lo),
        _ => lo + f * (hi - lo),
    }
}

/// A new envelope of `kind`: one point at 0 holding the default (a Mute point switches).
fn new_envelope(kind: EnvKind) -> Envelope {
    let curve = if kind == EnvKind::Mute { Curve::Hold } else { Curve::Linear };
    Envelope { kind, points: Keyframes { keys: vec![Keyframe { t: 0.0, v: env_default(kind), curve }] } }
}

/// Insert/Remove Envelope: add envelope `kind` to the track (one point at 0 holding the default), or remove it.
pub fn toggle_envelope(track: &mut Track, kind: EnvKind) -> bool {
    let n = track.envelopes.len();
    track.envelopes.retain(|e| e.kind != kind);
    if track.envelopes.len() == n {
        track.envelopes.push(new_envelope(kind));
    }
    true
}

/// The video events `ids` can be edited: unlocked, on unlocked tracks.
fn free_video_events<'a>(project: &'a mut Project, ids: &[String]) -> impl Iterator<Item = &'a mut Clip> {
    project.timeline.video_tracks.iter_mut().filter(|t| !t.locked).flat_map(|t| &mut t.clips).filter(move |c| !c.locked && ids.contains(&c.id))
}

/// R4b: the audio events that follow video event `id`'s Velocity envelope: unlocked, in its group, at its place and
/// length (the audio of an A/V pair). They play a copy of it (`sync_velocity`), which is not drawn.
fn velocity_followers(project: &Project, id: &str) -> Vec<String> {
    let Some(c) = project.clip(id) else { return Vec::new() };
    let Some(g) = &c.group else { return Vec::new() };
    let same = |a: &Clip| a.group.as_ref() == Some(g) && !a.locked && (a.position - c.position).abs() < 1e-6 && (a.duration - c.duration).abs() < 1e-6;
    project.timeline.audio_tracks.iter().filter(|t| !t.locked).flat_map(|t| &t.clips).filter(|a| same(a)).map(|a| a.id.clone()).collect()
}

/// R4b: with Options › Audio Follows Video Velocity on, the audio of video event `id` follows its speed: its followers
/// get a copy of its Velocity envelope (or lose theirs with it). Off (VEGAS: the audio stays at normal speed), they lose
/// any copy.
fn sync_velocity(project: &mut Project, id: &str) {
    let ids = velocity_followers(project, id);
    let follow = project.timeline.audio_follows_velocity;
    let env = project.clip(id).filter(|_| follow).and_then(|c| c.envelopes.iter().find(|e| e.kind == EnvKind::Velocity)).cloned();
    for a in project.timeline.audio_tracks.iter_mut().flat_map(|t| &mut t.clips).filter(|a| ids.contains(&a.id)) {
        a.envelopes.retain(|e| e.kind != EnvKind::Velocity);
        a.envelopes.extend(env.clone());
    }
}

/// Options › Audio Follows Video Velocity: set it `on` and copy (or remove) every video Velocity envelope's audio copy.
pub fn set_audio_follows_velocity(project: &mut Project, on: bool) -> bool {
    let done = project.timeline.audio_follows_velocity != on;
    project.timeline.audio_follows_velocity = on;
    let ids: Vec<String> = project.timeline.video_tracks.iter().flat_map(|t| &t.clips).filter(|c| c.envelopes.iter().any(|e| e.kind == EnvKind::Velocity)).map(|c| c.id.clone()).collect();
    for id in &ids {
        sync_velocity(project, id);
    }
    changed(project, done)
}

/// Insert/Remove Envelope › Velocity on video events `ids`: all take the opposite of the focus (last) event's state.
/// A new one runs at 100%.
pub fn toggle_velocity(project: &mut Project, ids: &[String]) -> bool {
    let Some(on) = ids.last().and_then(|id| project.clip(id)).map(|c| crate::engine::velocity::of(c).is_none()) else { return false };
    let mut done = Vec::new();
    // Events that already have one keep it (their points are the user's).
    for c in free_video_events(project, ids).filter(|c| crate::engine::velocity::of(c).is_some() != on) {
        c.envelopes.retain(|e| e.kind != EnvKind::Velocity);
        if on {
            c.envelopes.push(new_envelope(EnvKind::Velocity));
        }
        done.push(c.id.clone());
    }
    for id in &done {
        sync_velocity(project, id);
    }
    changed(project, !done.is_empty())
}

/// Event Properties › Pitch change, by `cents` (the Pitch Up / Down keys), on the unlocked audio events `ids`, within
/// two octaves either way.
pub fn shift_pitch(project: &mut Project, ids: &[String], cents: f32) -> bool {
    let mut done = false;
    for c in project.timeline.audio_tracks.iter_mut().filter(|t| !t.locked).flat_map(|t| &mut t.clips).filter(|c| !c.locked && ids.contains(&c.id)) {
        let p = (c.pitch_cents + cents).clamp(-PITCH_MAX, PITCH_MAX).round();
        done |= p != c.pitch_cents;
        c.pitch_cents = p;
    }
    changed(project, done)
}

/// The largest pitch change, in cents.
pub const PITCH_MAX: f32 = 2400.0;

/// Freeze Frame at Cursor: a Velocity envelope (added at 100% if there is none) that steps to a 0% Hold point at the
/// cursor (`t`, timeline time): the speed up to the cursor is kept (a point there holds its value), and the frame at
/// the cursor holds until the next point, or to the event's end.
pub fn freeze_frame(project: &mut Project, ids: &[String], t: f64) -> bool {
    let mut done = Vec::new();
    for c in free_video_events(project, ids).filter(|c| t > c.position && t < c.position + c.duration) {
        let local = t - c.position;
        if !c.envelopes.iter().any(|e| e.kind == EnvKind::Velocity) {
            c.envelopes.push(new_envelope(EnvKind::Velocity));
        }
        let Some(e) = c.envelopes.iter_mut().find(|e| e.kind == EnvKind::Velocity) else { continue };
        let v = e.points.at(local);
        let keys = &mut e.points.keys;
        // Points already at the cursor make way for the step.
        keys.retain(|k| (k.t - local).abs() > KEY_EPS);
        let i = keys.partition_point(|k| k.t < local);
        keys.splice(i..i, [Keyframe { t: local, v, curve: Curve::Hold }, Keyframe { t: local, v: 0.0, curve: Curve::Hold }]);
        done.push(c.id.clone());
    }
    for id in &done {
        sync_velocity(project, id);
    }
    changed(project, !done.is_empty())
}

/// Insert/Remove Envelope › Transition Progress on the transitions of video events `ids` (into and out of each):
/// added (0% to 100% over the transition) when the focus event's have none, else removed.
pub fn toggle_progress(project: &mut Project, ids: &[String]) -> bool {
    let has = |c: &Clip| [&c.transition, &c.transition_out].into_iter().flatten().any(|tr| !tr.progress.is_empty());
    let Some(on) = ids.last().and_then(|id| project.clip(id)).map(|c| !has(c)) else { return false };
    let linear = Keyframes { keys: vec![Keyframe { t: 0.0, v: 0.0, curve: Curve::Linear }, Keyframe { t: 1.0, v: 1.0, curve: Curve::Linear }] };
    let mut done = false;
    for c in free_video_events(project, ids) {
        // Transitions that already have one keep it.
        for tr in [&mut c.transition, &mut c.transition_out].into_iter().flatten().filter(|tr| tr.progress.is_empty() == on) {
            tr.progress = if on { linear.clone() } else { Keyframes::default() };
            done = true;
        }
    }
    changed(project, done)
}

/// Whose envelope an envelope edit is on: a track (by id), a video event's Velocity (by event id), or the Transition
/// Progress of the transition into an event (event id) or out of it (event id + `TRANSITION_OUT`).
pub const TRANSITION_OUT: &str = "/out";

/// The start and length of an event's transition (into it, or `out` of it): the overlap with the event before it, or
/// its fade-in (fade-out).
pub fn transition_span(clips: &[Clip], c: &Clip, out: bool) -> Option<(f64, f64)> {
    let end = c.position + c.duration;
    let (a, b) = if out {
        (end - c.fade_out, end)
    } else {
        let from = clips.iter().filter(|a| a.position < c.position && a.position + a.duration > c.position).map(|a| a.position + a.duration).reduce(f64::max);
        (c.position, from.map_or(c.position + c.fade_in, |e| e.min(end)))
    };
    (b > a).then_some((a, b - a))
}

/// Envelope `kind` of `owner` (see `TRANSITION_OUT`), with how its key times map to the timeline: timeline time = start
/// + key time x scale. A transition's progress is lent out as an envelope.
pub fn envelope_of(project: &Project, owner: &str, kind: EnvKind) -> Option<(Envelope, f64, f64)> {
    // R8c: an FX envelope (`FX_ENV`), and an audio bus's own envelopes.
    if kind == EnvKind::FxParam {
        return fx_envelope(project, owner).map(|e| (e, 0.0, 1.0));
    }
    if let Some(b) = project.timeline.audio.bus(owner) {
        return b.envelopes.iter().find(|e| e.kind == kind).map(|e| (e.clone(), 0.0, 1.0));
    }
    if let Some((v, i)) = project.find_track(owner) {
        return project.timeline.tracks(v)[i].envelopes.iter().find(|e| e.kind == kind).map(|e| (e.clone(), 0.0, 1.0));
    }
    let (id, out) = split_owner(owner);
    let r = project.find_clip(id)?;
    let clips = &project.timeline.tracks(r.is_video)[r.track_idx].clips;
    let c = &clips[r.clip_idx];
    if kind == EnvKind::TransitionProgress {
        let tr = if out { &c.transition_out } else { &c.transition };
        let points = tr.as_ref().map(|t| t.progress.clone()).filter(|p| !p.is_empty())?;
        let (start, len) = transition_span(clips, c, out)?;
        return Some((Envelope { kind, points }, start, len));
    }
    c.envelopes.iter().find(|e| e.kind == kind && !out).map(|e| (e.clone(), c.position, 1.0))
}

/// Insert (`on`) or remove envelope `kind` on the unlocked tracks `ids` it fits (`env_fits`).
pub fn set_envelope(project: &mut Project, ids: &[String], kind: EnvKind, on: bool) -> bool {
    let mut done = false;
    for t in tracks_mut(&mut project.timeline).filter(|t| ids.contains(&t.id) && !t.locked && env_fits(kind, t.kind == TrackKind::Video)) {
        if t.envelopes.iter().any(|e| e.kind == kind) != on {
            done |= toggle_envelope(t, kind);
        }
    }
    changed(project, done)
}

/// Run `f` on envelope `kind` of `owner`, a track or an event (`envelope_of`), unless it is locked.
pub fn edit_envelope(project: &mut Project, owner: &str, kind: EnvKind, f: impl FnOnce(&mut Envelope) -> bool) -> bool {
    let done = with_envelope(project, owner, kind, f);
    changed(project, done)
}

/// `edit_envelope` without marking the project modified (a drag marks it on release).
pub fn with_envelope(project: &mut Project, owner: &str, kind: EnvKind, f: impl FnOnce(&mut Envelope) -> bool) -> bool {
    if kind == EnvKind::FxParam {
        return with_fx_envelope(project, owner, f);
    }
    let a = &mut project.timeline.audio;
    if let Some(b) = a.buses.iter_mut().chain(&mut a.fx).find(|b| b.id == owner) {
        return b.envelopes.iter_mut().find(|e| e.kind == kind).is_some_and(f);
    }
    if let Some((v, i)) = project.find_track(owner) {
        let t = &mut project.timeline.tracks_mut(v)[i];
        return !t.locked && t.envelopes.iter_mut().find(|e| e.kind == kind).is_some_and(f);
    }
    let (id, out) = split_owner(owner);
    let Some(r) = project.find_clip(id) else { return false };
    let t = &mut project.timeline.tracks_mut(r.is_video)[r.track_idx];
    let c = &mut t.clips[r.clip_idx];
    if t.locked || c.locked {
        return false;
    }
    if kind != EnvKind::TransitionProgress {
        let done = !out && c.envelopes.iter_mut().find(|e| e.kind == kind).is_some_and(f);
        if done && kind == EnvKind::Velocity && r.is_video {
            sync_velocity(project, id);
        }
        return done;
    }
    let Some(tr) = (if out { c.transition_out.as_mut() } else { c.transition.as_mut() }).filter(|t| !t.progress.is_empty()) else { return false };
    let mut e = Envelope { kind, points: std::mem::take(&mut tr.progress) };
    let done = f(&mut e);
    // Progress keys stay inside the transition.
    e.points.keys.iter_mut().for_each(|k| k.t = k.t.min(1.0));
    tr.progress = e.points;
    done
}

/// The compositing mode of video track `track` (unless it is locked).
pub fn set_composite(project: &mut Project, track: &str, mode: Composite) -> bool {
    let Some((true, i)) = project.find_track(track) else { return false };
    let t = &mut project.timeline.video_tracks[i];
    let done = !t.locked && t.composite != mode;
    if done {
        t.composite = mode;
    }
    changed(project, done)
}

/// A compositor from the Compositors window on the unlocked video tracks `ids`: their mode becomes Custom.
pub fn set_compositor(project: &mut Project, ids: &[String], fx: &Fx) -> bool {
    let mut done = false;
    for t in project.timeline.video_tracks.iter_mut().filter(|t| ids.contains(&t.id) && !t.locked) {
        (t.composite, t.compositor) = (Composite::Custom, Some(Fx { id: uuid::Uuid::new_v4().to_string(), ..fx.clone() }));
        done = true;
    }
    changed(project, done)
}

/// Make Compositing Child (`child`) or Make Compositing Parent (not `child`) on the unlocked video tracks `ids`. The
/// top track has no parent above it, so it cannot be a child.
pub fn set_child(project: &mut Project, ids: &[String], child: bool) -> bool {
    let mut done = false;
    for (i, t) in project.timeline.video_tracks.iter_mut().enumerate() {
        if ids.contains(&t.id) && !t.locked && t.child != child && (i > 0 || !child) {
            t.child = child;
            done = true;
        }
    }
    changed(project, done)
}

/// The Parent Composite Mode of video track `track` (unless it is locked).
pub fn set_parent_mode(project: &mut Project, track: &str, mode: Composite) -> bool {
    let Some((true, i)) = project.find_track(track) else { return false };
    let t = &mut project.timeline.video_tracks[i];
    let done = !t.locked && t.parent_mode != mode;
    if done {
        t.parent_mode = mode;
    }
    changed(project, done)
}

/// Video track `i` is a compositing parent: the track below it is its child.
pub fn is_parent(tl: &Timeline, i: usize) -> bool {
    !tl.video_tracks.get(i).is_none_or(|t| t.child) && tl.video_tracks.get(i + 1).is_some_and(|t| t.child)
}

/// Insert/Remove Envelope on a bus track: the Video Bus Track's (`bus` None: Fade to Color, Motion Blur Amount or Video
/// Supersampling) or, R8b, an audio bus's or assignable FX chain's (Volume, Pan or Mute).
pub fn toggle_bus_envelope(project: &mut Project, bus: Option<&str>, kind: EnvKind) -> bool {
    let a = &mut project.timeline.audio;
    let envs = match bus {
        None => &mut project.timeline.master.video_envelopes,
        Some(id) => match a.buses.iter_mut().chain(&mut a.fx).find(|b| b.id == id) {
            Some(b) => &mut b.envelopes,
            None => return false,
        },
    };
    let n = envs.len();
    envs.retain(|e| e.kind != kind);
    if envs.len() == n {
        envs.push(new_envelope(kind));
    }
    changed(project, true)
}

// ---- R8c: FX envelopes, automation modes, automation writes ----

/// The owner of an FX envelope, as `envelope_of` takes it: `{track or bus id}/fx/{plug-in id}/{parameter key}`. Audio
/// track and bus plug-ins keep their envelopes in `Fx::auto`; a video track plug-in's are its keys (View › Show OFX
/// Effect Envelopes). They are lent out as `EnvKind::FxParam`, 0..=1 of the parameter's range (`plugins::to_unit`).
pub const FX_ENV: &str = "/fx/";

pub fn fx_owner(parent: &str, fx: &str, param: &str) -> String {
    format!("{parent}{FX_ENV}{fx}/{param}")
}

/// (track or bus id, plug-in id, parameter key) of an FX envelope owner.
pub fn fx_owner_parts(owner: &str) -> Option<(&str, &str, &str)> {
    let (parent, rest) = owner.split_once(FX_ENV)?;
    let (fx, param) = rest.split_once('/')?;
    Some((parent, fx, param))
}

/// The track or bus id an envelope owner belongs to (an event id stays as it is).
pub fn owner_base(owner: &str) -> &str {
    fx_owner_parts(owner).map_or(owner, |p| p.0)
}

/// Run envelope edit `f` on keys `kf` (FX automation).
fn ripple_keys(kf: &mut Keyframes<f32>, f: &impl Fn(&mut Envelope) -> bool) -> bool {
    let mut e = Envelope { kind: EnvKind::FxParam, points: std::mem::take(kf) };
    let done = f(&mut e);
    *kf = e.points;
    done
}

/// The plug-in chain of track or bus `parent`: (chain, a video track's (keys are its envelopes), locked).
fn chain_of<'a>(project: &'a Project, parent: &str) -> Option<(&'a [Fx], bool, bool)> {
    match project.find_track(parent) {
        Some((v, i)) => {
            let t = &project.timeline.tracks(v)[i];
            Some((&t.fx, v, t.locked))
        }
        None => project.timeline.audio.bus(parent).map(|b| (&b.fx[..], false, false)),
    }
}

fn chain_of_mut<'a>(project: &'a mut Project, parent: &str) -> Option<(&'a mut Vec<Fx>, bool, bool)> {
    match project.find_track(parent) {
        Some((v, i)) => {
            let t = &mut project.timeline.tracks_mut(v)[i];
            Some((&mut t.fx, v, t.locked))
        }
        None => {
            let a = &mut project.timeline.audio;
            a.buses.iter_mut().chain(&mut a.fx).find(|b| b.id == parent).map(|b| (&mut b.fx, false, false))
        }
    }
}

/// Keys in a parameter's units as envelope points (0..=1), and back.
fn unit_keys(kf: &Keyframes<f32>, kind: crate::engine::plugins::ParamKind, to: bool) -> Keyframes<f32> {
    use crate::engine::plugins::{from_unit, to_unit};
    Keyframes { keys: kf.keys.iter().map(|k| Keyframe { v: if to { to_unit(kind, k.v) } else { from_unit(kind, k.v) }, ..k.clone() }).collect() }
}

/// FX envelope `owner` lent out (points 0..=1); None when it has none.
fn fx_envelope(project: &Project, owner: &str) -> Option<Envelope> {
    let (parent, fid, key) = fx_owner_parts(owner)?;
    let (chain, video, _) = chain_of(project, parent)?;
    let fx = chain.iter().find(|f| f.id == fid)?;
    let spec = crate::engine::plugins::automatable(fx, key)?;
    let kf = if video { fx.params.get(key) } else { fx.auto.get(key) }.filter(|k| !k.keys.is_empty())?;
    Some(Envelope { kind: EnvKind::FxParam, points: unit_keys(kf, spec.kind, true) })
}

fn with_fx_envelope(project: &mut Project, owner: &str, f: impl FnOnce(&mut Envelope) -> bool) -> bool {
    let Some((parent, fid, key)) = fx_owner_parts(owner) else { return false };
    let Some((chain, video, locked)) = chain_of_mut(project, parent) else { return false };
    let Some(fx) = chain.iter_mut().find(|f| f.id == fid).filter(|_| !locked) else { return false };
    let Some(spec) = crate::engine::plugins::automatable(fx, key) else { return false };
    let Some(kf) = if video { fx.params.get_mut(key) } else { fx.auto.get_mut(key) }.filter(|k| !k.keys.is_empty()) else { return false };
    let mut e = Envelope { kind: EnvKind::FxParam, points: unit_keys(kf, spec.kind, true) };
    let done = f(&mut e);
    if done {
        *kf = unit_keys(&e.points, spec.kind, false);
    }
    done
}

/// FX Automation Envelopes: add (one point at 0 holding the parameter's value) or remove the envelope of parameter `key`
/// of plug-in `fx` on audio track or bus `parent`. False on a locked track or a parameter no envelope can automate.
pub fn toggle_fx_envelope(project: &mut Project, parent: &str, fx: &str, key: &str) -> bool {
    let Some((chain, false, false)) = chain_of_mut(project, parent) else { return false };
    let Some(f) = chain.iter_mut().find(|f| f.id == fx) else { return false };
    if crate::engine::plugins::automatable(f, key).is_none() {
        return false;
    }
    if f.auto.remove(key).is_none() {
        let v = crate::engine::plugins::eval(&Fx { auto: Default::default(), ..f.clone() }, 0.0).map_or(0.0, |e| e.f(key));
        f.auto.insert(key.to_string(), Keyframes { keys: vec![Keyframe { t: 0.0, v, curve: Curve::Linear }] });
    }
    changed(project, true)
}

/// Every FX envelope of the chain `chain` of track or bus `parent`, lent out with its owner, in chain and parameter
/// order: an audio chain's automated parameters, a `video` track's keyed (animated) numbers.
pub fn chain_envelopes(parent: &str, chain: &[Fx], video: bool) -> Vec<(String, Envelope)> {
    let mut out = Vec::new();
    for f in chain {
        let Some(info) = crate::engine::plugins::find(&f.plugin) else { continue };
        for p in info.params.iter().filter(|p| crate::engine::plugins::automatable(f, p.key).is_some()) {
            let kf = if video { f.params.get(p.key).filter(|k| k.keys.len() > 1 || k.keys.iter().any(|k| k.t != 0.0)) } else { f.auto.get(p.key) };
            if let Some(kf) = kf.filter(|k| !k.keys.is_empty()) {
                out.push((fx_owner(parent, &f.id, p.key), Envelope { kind: EnvKind::FxParam, points: unit_keys(kf, p.kind, true) }));
            }
        }
    }
    out
}

/// The FX envelope owners of track or bus `parent` (`chain_envelopes`); a video track's only with `video_keys`.
pub fn fx_envelopes(project: &Project, parent: &str, video_keys: bool) -> Vec<String> {
    let Some((chain, video, _)) = chain_of(project, parent) else { return Vec::new() };
    if video && !video_keys {
        return Vec::new();
    }
    chain_envelopes(parent, chain, video).into_iter().map(|e| e.0).collect()
}

/// "Track EQ: Band 1 Gain (dB)": the plug-in and parameter an FX envelope owner names.
pub fn fx_envelope_name(project: &Project, owner: &str) -> Option<String> {
    let (parent, fid, key) = fx_owner_parts(owner)?;
    let fx = chain_of(project, parent)?.0.iter().find(|f| f.id == fid)?;
    let info = crate::engine::plugins::find(&fx.plugin)?;
    let spec = info.params.iter().find(|p| p.key == key)?;
    // A parameter without a section heading is in the last one above it.
    let section = info.params.iter().take_while(|p| p.key != key).chain([spec]).filter(|p| !p.section.is_empty()).last().map(|p| p.section);
    Some(match section {
        Some(s) => format!("{}: {s} {}", info.name, spec.name),
        None => format!("{}: {}", info.name, spec.name),
    })
}

/// A value of FX envelope `owner` (0..=1) in the parameter's units: "-12.0", "High-Pass", "On".
pub fn fx_value_text(project: &Project, owner: &str, u: f32) -> Option<String> {
    use crate::engine::plugins::{ParamKind, from_unit};
    let (parent, fid, key) = fx_owner_parts(owner)?;
    let fx = chain_of(project, parent)?.0.iter().find(|f| f.id == fid)?;
    let spec = crate::engine::plugins::automatable(fx, key)?;
    let v = from_unit(spec.kind, u);
    Some(match spec.kind {
        ParamKind::Choice { options, .. } => options.get(v as usize).copied().unwrap_or_default().to_string(),
        ParamKind::Bool { .. } => (if v >= 0.5 { "On" } else { "Off" }).to_string(),
        _ => format!("{} {}", spec.name, if v.abs() >= 100.0 { format!("{v:.0}") } else { format!("{v:.2}") }),
    })
}

/// The default value of FX envelope `owner`'s parameter, 0..=1 (the point menu's Set to Default).
pub fn fx_default_unit(project: &Project, owner: &str) -> Option<f32> {
    use crate::engine::plugins::{ParamKind, to_unit};
    let (parent, fid, key) = fx_owner_parts(owner)?;
    let fx = chain_of(project, parent)?.0.iter().find(|f| f.id == fid)?;
    let spec = crate::engine::plugins::automatable(fx, key)?;
    Some(match spec.kind {
        ParamKind::Float { default, .. } => to_unit(spec.kind, default),
        ParamKind::Choice { default, .. } => to_unit(spec.kind, default as f32),
        ParamKind::Bool { default } => default as u8 as f32,
        _ => return None,
    })
}

/// R8c: the Automation Settings and Show Automation Controls of the audio track or bus `id`.
pub fn automation_of(project: &Project, id: &str) -> Option<(AutoMode, bool)> {
    match project.find_track(id) {
        Some((false, i)) => {
            let t = &project.timeline.audio_tracks[i];
            Some((t.automation, t.auto_controls))
        }
        Some(_) => None,
        None => project.timeline.audio.bus(id).map(|b| (b.automation, b.auto_controls)),
    }
}

/// Automation ▸ Off / Read / Write (Touch) / Write (Latch) (`mode`) or Show Automation Controls (`controls`) on the
/// unlocked audio tracks and busses `ids`.
pub fn set_automation(project: &mut Project, ids: &[String], mode: Option<AutoMode>, controls: Option<bool>) -> bool {
    let mut done = false;
    let mut set = |m: &mut AutoMode, c: &mut bool| {
        if let Some(mode) = mode.filter(|x| x != m) {
            (*m, done) = (mode, true);
        }
        if let Some(on) = controls.filter(|x| x != c) {
            (*c, done) = (on, true);
        }
    };
    let tl = &mut project.timeline;
    for t in tl.audio_tracks.iter_mut().filter(|t| ids.contains(&t.id) && !t.locked) {
        set(&mut t.automation, &mut t.auto_controls);
    }
    for b in tl.audio.buses.iter_mut().chain(&mut tl.audio.fx).filter(|b| ids.contains(&b.id)) {
        set(&mut b.automation, &mut b.auto_controls);
    }
    changed(project, done)
}

/// An automation write needs envelope `kind` of `owner` (an audio track, a bus, or an FX envelope owner): add it when
/// it has none. False when it cannot have one (locked, gone).
pub fn ensure_envelope(project: &mut Project, owner: &str, kind: EnvKind) -> bool {
    if project.find_track(owner_base(owner)).is_some_and(|(v, i)| project.timeline.tracks(v)[i].locked) {
        return false;
    }
    if envelope_of(project, owner, kind).is_some() {
        return true;
    }
    if let Some((parent, fx, key)) = fx_owner_parts(owner) {
        return toggle_fx_envelope(project, parent, fx, key);
    }
    match project.find_track(owner) {
        Some((false, i)) if !project.timeline.audio_tracks[i].locked && env_fits(kind, false) => project.timeline.audio_tracks[i].envelopes.push(new_envelope(kind)),
        Some(_) => return false,
        None if project.timeline.audio.bus(owner).is_some() => return toggle_bus_envelope(project, Some(owner), kind),
        None => return false,
    }
    changed(project, true)
}

/// Insert/Remove Envelope › Volume, Pan or Mute on audio events `ids`: all take the opposite of the focus (last)
/// event's state. Locked events and events on locked tracks are left alone.
pub fn toggle_event_envelope(project: &mut Project, ids: &[String], kind: EnvKind) -> bool {
    let Some(on) = ids.last().and_then(|id| project.clip(id)).map(|c| !c.envelopes.iter().any(|e| e.kind == kind)) else { return false };
    let mut done = false;
    for c in project.timeline.audio_tracks.iter_mut().filter(|t| !t.locked).flat_map(|t| &mut t.clips).filter(|c| !c.locked && ids.contains(&c.id)) {
        // Events that already have one keep it (their points are the user's).
        if c.envelopes.iter().any(|e| e.kind == kind) != on {
            c.envelopes.retain(|e| e.kind != kind);
            if on {
                c.envelopes.push(new_envelope(kind));
            }
            done = true;
        }
    }
    changed(project, done)
}

/// The least time between two points an automation write adds, and the least change: 0.5 dB on Volume, half a percent
/// of the lane on the others.
pub const WRITE_GAP: f64 = 0.020;

/// Add value `v` at key time `t` to the points `pts` of an automation write, unless it is within `WRITE_GAP` of the last
/// one or changes it by less than the least change.
pub fn write_point(kind: EnvKind, pts: &mut Vec<(f64, f32)>, t: f64, v: f32) {
    let differs = |a: f32| if kind == EnvKind::Volume { (v - a).abs() >= 0.5 } else { (lane_frac(kind, v) - lane_frac(kind, a)).abs() >= 0.005 };
    if pts.last().is_none_or(|&(lt, lv)| t - lt >= WRITE_GAP - 1e-9 && differs(lv)) {
        pts.push((t, env_clamp(kind, v)));
    }
}

/// An automation write over envelope `orig` (key times): punched in at `t0` with points `pts`, the control at `v` at
/// `t` now. `orig` stays as it was before `t0` and after `t`; it steps to the written values at `t0` and back at `t`.
pub fn env_written(orig: &Envelope, t0: f64, pts: &[(f64, f32)], t: f64, v: f32) -> Envelope {
    let key = |t, v, curve| Keyframe { t, v, curve };
    let ks = &orig.points.keys;
    let mut keys: Vec<Keyframe<f32>> = ks.iter().filter(|k| k.t < t0 - KEY_EPS).cloned().collect();
    keys.push(key(t0, orig.at(t0), Curve::Linear));
    keys.extend(pts.iter().map(|&(t, v)| key(t, v, Curve::Linear)));
    keys.push(key(t, env_clamp(orig.kind, v), Curve::Linear));
    // The original curve on from `t`: its value there, with the shape of the segment it is in.
    let after = ks.partition_point(|k| k.t <= t + KEY_EPS);
    let curve = after.checked_sub(1).map_or(Curve::Linear, |i| ks[i].curve);
    keys.push(key(t, orig.at(t), curve));
    keys.extend(ks[after..].iter().cloned());
    Envelope { kind: orig.kind, points: Keyframes { keys } }
}

/// Add a point at `t` holding `v`; it takes its left neighbour's curve (Mute points hold). Returns its index.
pub fn env_insert(env: &mut Envelope, t: f64, v: f32) -> usize {
    let keys = &mut env.points.keys;
    let t = t.max(0.0);
    let i = keys.partition_point(|k| k.t <= t);
    let curve = if env.kind == EnvKind::Mute { Curve::Hold } else { i.checked_sub(1).map_or(Curve::Linear, |j| keys[j].curve) };
    keys.insert(i, Keyframe { t, v: env_clamp(env.kind, v), curve });
    i
}

/// Drag of points `sel` as they are in `orig` by `dt` seconds and `df` of the lane height: they move together, strictly
/// between their unselected neighbours (the first point keeps its time), with values in the range. True when `env`
/// now differs from `orig`.
pub fn env_move(env: &mut Envelope, orig: &Envelope, sel: &[usize], dt: f64, df: f32) -> bool {
    let keys = &orig.points.keys;
    let sel: Vec<usize> = sel.iter().copied().filter(|&i| i < keys.len()).collect();
    let mut dt = if sel.contains(&0) { 0.0 } else { dt };
    for &i in &sel {
        if let Some(p) = (0..i).rev().find(|j| !sel.contains(j)) {
            dt = dt.max(keys[p].t + KEY_GAP - keys[i].t);
        }
        if let Some(n) = (i + 1..keys.len()).find(|j| !sel.contains(j)) {
            dt = dt.min(keys[n].t - KEY_GAP - keys[i].t);
        }
    }
    let mut out = orig.clone();
    for &i in &sel {
        let k = &mut out.points.keys[i];
        k.t = (k.t + dt).max(0.0);
        if df != 0.0 {
            k.v = env_clamp(orig.kind, lane_value(orig.kind, lane_frac(orig.kind, k.v) + df));
        }
    }
    *env = out;
    env != orig
}

/// Delete points `sel`; the first point stays, so the envelope is never empty.
pub fn env_delete(env: &mut Envelope, sel: &[usize]) -> bool {
    let n = env.points.keys.len();
    let mut i = 0;
    env.points.keys.retain(|_| {
        i += 1;
        i == 1 || !sel.contains(&(i - 1))
    });
    env.points.keys.len() != n
}

/// Points `sel` take value `v` (Set to Maximum / Default / Minimum, Set To...).
pub fn env_set_values(env: &mut Envelope, sel: &[usize], v: f32) -> bool {
    let v = env_clamp(env.kind, v);
    let mut done = false;
    for (_, k) in env.points.keys.iter_mut().enumerate().filter(|(i, _)| sel.contains(i)) {
        done |= k.v != v;
        k.v = v;
    }
    done
}

/// The curve of the segment after each of points `sel`.
pub fn env_set_curve(env: &mut Envelope, sel: &[usize], curve: Curve) -> bool {
    let mut done = false;
    for (_, k) in env.points.keys.iter_mut().enumerate().filter(|(i, _)| sel.contains(i)) {
        done |= k.curve != curve;
        k.curve = curve;
    }
    done
}

/// Flip All Points: each point mirrored about the middle of the lane.
pub fn env_flip(env: &mut Envelope) -> bool {
    let kind = env.kind;
    let mut done = false;
    for k in &mut env.points.keys {
        let v = env_clamp(kind, lane_value(kind, 1.0 - lane_frac(kind, k.v)));
        done |= v != k.v;
        k.v = v;
    }
    done
}

/// Thin All Points: drop points the envelope would pass within 0.005 of the lane height of without them.
pub fn env_thin(env: &mut Envelope) -> bool {
    let (kind, n) = (env.kind, env.points.keys.len());
    let keys = &mut env.points.keys;
    let mut i = 1;
    while i + 1 < keys.len() {
        let (a, k, b) = (&keys[i - 1], &keys[i], &keys[i + 1]);
        let s = a.curve.shape((k.t - a.t) / (b.t - a.t).max(KEY_EPS)) as f32;
        let without = a.v + (b.v - a.v) * s;
        if (lane_frac(kind, without) - lane_frac(kind, k.v)).abs() <= 0.005 && k.curve == a.curve {
            keys.remove(i);
        } else {
            i += 1;
        }
    }
    keys.len() != n
}

/// Reset All: every point to the default value.
pub fn env_reset(env: &mut Envelope) -> bool {
    let all: Vec<usize> = (0..env.points.keys.len()).collect();
    env_set_values(env, &all, env_default(env.kind))
}

/// Indices of the points in `a..=b`.
pub fn keys_in_span(env: &Envelope, a: f64, b: f64) -> Vec<usize> {
    env.points.keys.iter().enumerate().filter(|(_, k)| k.t >= a - KEY_EPS && k.t <= b + KEY_EPS).map(|(i, _)| i).collect()
}

/// Keys are event-relative and stay put on the timeline: when an event's start moves by `-dt` (a left trim, the
/// right half of a split), every keyframe list on it (envelopes, Pan/Crop, FX parameters) shifts by `dt`. Keys that
/// land before 0 go, and a key at 0 keeps the value there.
pub fn shift_clip_keys(clip: &mut Clip, dt: f64) {
    for e in &mut clip.envelopes {
        shift_keys(&mut e.points, dt);
    }
    shift_keys(&mut clip.pan_crop, dt);
    for m in &mut clip.masks {
        shift_keys(&mut m.path, dt);
    }
    for k in clip.fx.iter_mut().flat_map(|f| f.params.values_mut()) {
        shift_keys(k, dt);
    }
}

// shortcut: the boundary key keeps the cut segment's curve, so only linear segments stay exact; split curves if needed.
fn shift_keys<T: crate::engine::keyframes::Lerp + Default>(k: &mut Keyframes<T>, dt: f64) {
    if k.keys.is_empty() || dt == 0.0 {
        return;
    }
    let at0 = k.at(-dt);
    k.shift(dt);
    let gone = k.keys.partition_point(|x| x.t < -KEY_EPS);
    if gone > 0 {
        let curve = k.keys[gone - 1].curve;
        k.keys.drain(..gone);
        if k.keys.first().is_none_or(|x| x.t > KEY_EPS) {
            k.keys.insert(0, Keyframe { t: 0.0, v: at0, curve });
        }
    }
}

/// `clip`'s keyframe lists as `orig` had them, shifted by `dt` (a trim recomputes from the original every frame).
fn rebase_keys(clip: &mut Clip, orig: &Clip, dt: f64) {
    clip.envelopes.clone_from(&orig.envelopes);
    clip.pan_crop.clone_from(&orig.pan_crop);
    clip.fx.clone_from(&orig.fx);
    shift_clip_keys(clip, dt);
}

/// A point at `t` holding the envelope's value there, with the curve of the segment it lands on.
fn key_at(k: &Keyframes<f32>, t: f64) -> Keyframe<f32> {
    let i = k.keys.partition_point(|x| x.t <= t);
    let curve = k.keys.get(i.saturating_sub(1)).map_or(Curve::Linear, |x| x.curve);
    Keyframe { t, v: k.at(t), curve }
}

/// `keys` (each flagged true when added by a carry, not the user's) in time order, without points that change
/// nothing: a run of points at one time keeps its first value and its last value and curve, and an added point
/// goes where the envelope is flat or a straight line through it.
fn sorted_keys(mut keys: Vec<(Keyframe<f32>, bool)>) -> Keyframes<f32> {
    keys.sort_by(|a, b| a.0.t.total_cmp(&b.0.t)); // stable: equal times keep their order (a step)
    let same = |a: &Keyframe<f32>, b: &Keyframe<f32>| (a.t - b.t).abs() <= KEY_EPS;
    let mut out: Vec<(Keyframe<f32>, bool)> = Vec::new();
    for k in keys {
        let n = out.len();
        if n >= 2 && same(&out[n - 2].0, &k.0) && same(&out[n - 1].0, &k.0) {
            out.pop(); // a point between two at its time changes nothing
        }
        match out.last_mut() {
            Some(l) if same(&l.0, &k.0) && (l.0.v - k.0.v).abs() <= 1e-5 => *l = (k.0, l.1 && k.1),
            _ => out.push(k),
        }
    }
    let mut i = 1;
    while i < out.len() {
        let (p, k) = (&out[i - 1].0, &out[i].0);
        let flat = |a: f32, b: f32| (a - b).abs() <= 1e-5;
        let redundant = out[i].1
            && match out.get(i + 1).map(|n| &n.0) {
                None => flat(p.v, k.v),
                Some(n) => {
                    let on_line = p.curve == Curve::Linear && k.curve == Curve::Linear && n.t - p.t > KEY_EPS && flat(p.v + (n.v - p.v) * ((k.t - p.t) / (n.t - p.t)) as f32, k.v);
                    (flat(p.v, k.v) && flat(k.v, n.v)) || on_line
                }
            };
        if redundant {
            out.remove(i);
        } else {
            i += 1;
        }
    }
    Keyframes { keys: out.into_iter().map(|k| k.0).collect() }
}

fn user(k: &Keyframe<f32>) -> (Keyframe<f32>, bool) {
    (k.clone(), false)
}

/// `k` with its points in `a..=b` taken out and points at `a` and `b` holding its values there.
fn cut_keys(k: &Keyframes<f32>, a: f64, b: f64) -> Keyframes<f32> {
    let mut keys: Vec<_> = k.keys.iter().filter(|x| x.t < a - KEY_EPS || x.t > b + KEY_EPS).map(user).collect();
    keys.extend([(key_at(k, a), true), (key_at(k, b), true)]);
    sorted_keys(keys)
}

/// `into` with `from`'s points in `a..=b` moved by `d`, replacing the points in the destination span. The values
/// just outside it stay (a point at each edge where `edge(t)` allows), and the moved span starts and ends with
/// `from`'s values at `a` and `b`.
fn paste_keys(into: &Keyframes<f32>, from: &Keyframes<f32>, (a, b): (f64, f64), d: f64, edge: impl Fn(f64) -> bool) -> Keyframes<f32> {
    let (c, e) = (a + d, b + d);
    let inside = |t: f64, lo: f64, hi: f64| t >= lo - KEY_EPS && t <= hi + KEY_EPS;
    let has = |t: f64| from.keys.iter().any(|x| (x.t - t).abs() <= KEY_EPS);
    let mut keys: Vec<_> = into.keys.iter().filter(|x| !inside(x.t, c, e)).map(user).collect();
    if edge(c) {
        keys.push((key_at(into, c), true));
    }
    if !has(a) {
        keys.push((Keyframe { t: c, ..key_at(from, a) }, true));
    }
    keys.extend(from.keys.iter().filter(|x| inside(x.t, a, b)).map(|x| (Keyframe { t: x.t + d, ..x.clone() }, false)));
    if !has(b) {
        keys.push((Keyframe { t: e, ..key_at(from, b) }, true));
    }
    if edge(e) {
        keys.push((key_at(into, e), true));
    }
    sorted_keys(keys)
}

/// Lock Envelopes to Events: events covering `span` on `track` moved by `delta`, and its envelope points go with
/// them. Points in the destination span go; points just outside both spans keep their values.
pub fn carry_envelope_points(track: &mut Track, span: (f64, f64), delta: f64) -> bool {
    let (a, b) = span;
    let mut done = false;
    for e in track.envelopes.iter_mut().filter(|e| !e.points.keys.is_empty() && delta != 0.0) {
        // An edge of the destination inside the vacated span gets no point: the vacated part holds the edge value.
        let new = paste_keys(&cut_keys(&e.points, a, b), &e.points, span, delta, |t| !(t > a + KEY_EPS && t < b - KEY_EPS));
        done |= new != e.points;
        e.points = new;
    }
    done
}

/// Carry the envelope points of moved events: `spans` are (is video, from track, to track, start, end) before a move
/// by `d`. Spans that meet on one track pair move as one; on one track in the direction of the move, so none lands on
/// points another has still to take. Across tracks every source is cut first and the points pasted come from the
/// envelopes as they were, so a track that is both a source and a destination keeps its own points moving.
// shortcut: points go only to an envelope of the same kind the destination already has; add one there if users ask.
fn carry_spans(tl: &mut Timeline, mut spans: Vec<(bool, usize, usize, f64, f64)>, d: f64) {
    spans.sort_by(|x, y| (x.0, x.1, x.2).cmp(&(y.0, y.1, y.2)).then(x.3.total_cmp(&y.3)));
    let mut merged: Vec<(bool, usize, usize, f64, f64)> = Vec::new();
    for s in spans {
        match merged.last_mut() {
            Some(m) if (m.0, m.1, m.2) == (s.0, s.1, s.2) && s.3 <= m.4 + KEY_EPS => m.4 = m.4.max(s.4),
            _ => merged.push(s),
        }
    }
    if d > 0.0 {
        merged.reverse();
    }
    let before: [Vec<Vec<Envelope>>; 2] = [false, true].map(|v| tl.tracks(v).iter().map(|t| t.envelopes.clone()).collect());
    let goes = |src: &Envelope, dst: &[Envelope]| !src.points.keys.is_empty() && dst.iter().any(|e| e.kind == src.kind && !e.points.keys.is_empty());
    for &(v, from, to, a, b) in &merged {
        let tracks = tl.tracks_mut(v);
        if from == to {
            carry_envelope_points(&mut tracks[from], (a, b), d);
            continue;
        }
        for k in 0..tracks[from].envelopes.len() {
            if goes(&tracks[from].envelopes[k], &before[v as usize][to]) {
                tracks[from].envelopes[k].points = cut_keys(&tracks[from].envelopes[k].points, a, b);
            }
        }
    }
    for &(v, from, to, a, b) in merged.iter().filter(|s| s.1 != s.2) {
        for src in before[v as usize][from].iter().filter(|s| goes(s, &before[v as usize][to])) {
            if let Some(e) = tl.tracks_mut(v)[to].envelopes.iter_mut().find(|e| e.kind == src.kind && !e.points.keys.is_empty()) {
                e.points = paste_keys(&e.points, &src.points, (a, b), d, |_| true);
            }
        }
    }
}

// ---- Busses and routing (R8a) ----

/// VEGAS's limit on assignable FX chains.
pub const MAX_FX_CHAINS: usize = 32;

/// Project Properties › Number of stereo busses: busses are added (Bus A, Bus B...) or the last ones removed, up to 26.
/// What went to a removed bus goes to the master, and sends to it go. False when the count is already `n`.
pub fn set_bus_count(project: &mut Project, n: usize) -> bool {
    let a = &mut project.timeline.audio;
    let n = n.min(AudioSettings::MAX_BUSES);
    if a.buses.len() == n {
        return false;
    }
    while a.buses.len() < n {
        let name = format!("Bus {}", (b'A' + a.buses.len() as u8) as char);
        a.buses.push(Bus::new(name, Vec::new()));
    }
    let gone: Vec<String> = a.buses.drain(n..).map(|b| b.id).collect();
    unroute(&mut project.timeline, &gone);
    changed(project, true)
}

/// Insert › Audio Bus: one more bus (false at 26).
pub fn insert_bus(project: &mut Project) -> bool {
    let n = project.timeline.audio.buses.len();
    n < AudioSettings::MAX_BUSES && set_bus_count(project, n + 1)
}

/// Insert › Audio Assignable FX: a new chain ("FX 1", the first free number) holding `fx`, routed to the master. Its id,
/// or None at VEGAS's 32 chains.
pub fn insert_assignable_fx(project: &mut Project, fx: Vec<Fx>) -> Option<String> {
    let a = &mut project.timeline.audio;
    if a.fx.len() >= MAX_FX_CHAINS {
        return None;
    }
    let n = (1..).find(|n| !a.fx.iter().any(|b| b.name == format!("FX {n}"))).unwrap_or(1);
    let b = Bus::new(format!("FX {n}"), fx);
    let id = b.id.clone();
    a.fx.push(b);
    project.mark_modified();
    Some(id)
}

/// R9: Insert › Audio Input Bus: one more input bus ("Input Bus A"..., the first free letter) to the master. Its id, or
/// None at VEGAS's 26.
pub fn insert_input_bus(project: &mut Project) -> Option<String> {
    let a = &mut project.timeline.audio;
    let name = (b'A'..=b'Z').map(|c| format!("Input Bus {}", c as char)).find(|n| !a.inputs.iter().any(|b| b.name == *n))?;
    let b = Bus::new(name, Vec::new());
    let id = b.id.clone();
    a.inputs.push(b);
    project.mark_modified();
    Some(id)
}

/// R9: the input bus strip's Delete. False when there is no such input bus.
pub fn delete_input_bus(project: &mut Project, id: &str) -> bool {
    let a = &mut project.timeline.audio;
    let n = a.inputs.len();
    a.inputs.retain(|b| b.id != id);
    let done = a.inputs.len() != n;
    changed(project, done)
}

/// Tools › Audio › Remove Assignable FX: the chains `ids` go, and every send to them.
pub fn remove_assignable_fx(project: &mut Project, ids: &[String]) -> bool {
    let a = &mut project.timeline.audio;
    let n = a.fx.len();
    a.fx.retain(|b| !ids.contains(&b.id));
    let done = a.fx.len() != n;
    unroute(&mut project.timeline, ids);
    changed(project, done)
}

/// Busses `gone` were removed: what went to them goes to the master, and sends to them go.
fn unroute(tl: &mut Timeline, gone: &[String]) {
    let lost = |o: &Out| matches!(o, Out::Bus(id) if gone.contains(id));
    for t in &mut tl.audio_tracks {
        if lost(&t.out) {
            t.out = Out::Master;
        }
        t.sends.retain(|s| !gone.contains(&s.to));
        if t.send_view.as_ref().is_some_and(|v| gone.contains(v)) {
            t.send_view = None;
        }
    }
    for b in tl.audio.buses.iter_mut().chain(&mut tl.audio.fx).chain(&mut tl.audio.inputs).filter(|b| lost(&b.out)) {
        b.out = Out::Master;
    }
}

/// Track header › Output: the audio tracks `ids` go to `out` (the master or a bus; never an assignable FX chain, which
/// tracks only send to). False when nothing changed or `out` names no bus.
pub fn set_track_out(project: &mut Project, ids: &[String], out: &Out) -> bool {
    if matches!(out, Out::Bus(b) if !project.timeline.audio.buses.iter().any(|x| x.id == *b)) {
        return false;
    }
    let mut done = false;
    for t in project.timeline.audio_tracks.iter_mut().filter(|t| ids.contains(&t.id) && t.out != *out) {
        t.out = out.clone();
        done = true;
    }
    changed(project, done)
}

/// R9: Input › Input Monitoring for the audio tracks `ids`. False when nothing changed.
pub fn set_monitor(project: &mut Project, ids: &[String], m: Monitor) -> bool {
    let mut done = false;
    for t in project.timeline.audio_tracks.iter_mut().filter(|t| ids.contains(&t.id) && t.monitor != m) {
        t.monitor = m;
        done = true;
    }
    changed(project, done)
}

/// A bus's or assignable FX chain's output: the master or a bus. Err (with the message) when that would make a loop,
/// as in Bus A → Bus B → Bus A; Ok(false) when nothing changed.
pub fn set_bus_out(project: &mut Project, id: &str, out: &Out) -> Result<bool, String> {
    let a = &project.timeline.audio;
    let name = |id: &str| a.bus(id).map_or(String::new(), |b| b.name.clone());
    if let Out::Bus(to) = out {
        if !a.buses.iter().any(|b| b.id == *to) {
            return Ok(false);
        }
        // Follow the outputs on from `to`: reaching `id` is a loop.
        let mut at = Some(to.clone());
        for _ in 0..=a.buses.len() {
            match at {
                Some(b) if b == id => return Err(format!("{} cannot be routed to {}: that would make a loop", name(id), name(to))),
                Some(b) => at = a.bus(&b).and_then(|b| if let Out::Bus(n) = &b.out { Some(n.clone()) } else { None }),
                None => break,
            }
        }
    }
    let Some(b) = project.timeline.audio.bus_mut(id).filter(|b| b.out != *out) else { return Ok(false) };
    b.out = out.clone();
    Ok(changed(project, true))
}

/// A new send to `to` is pre-volume to a bus and post-volume to an assignable FX chain, as in VEGAS.
pub fn send_pre_default(tl: &Timeline, to: &str) -> bool {
    tl.audio.buses.iter().any(|b| b.id == to)
}

/// The multipurpose slider: track `t`'s send to bus or chain `to` at `level` (linear); a new one is pre-volume when
/// `pre` (`send_pre_default`).
pub fn set_send(t: &mut Track, to: &str, level: f32, pre: bool) {
    let level = level.max(0.0);
    match t.sends.iter_mut().find(|s| s.to == to) {
        Some(s) => s.level = level,
        None => t.sends.push(AudioSend { to: to.to_string(), level, pre }),
    }
}

/// Pre Volume / Post Volume for track `t`'s send to `to` (a send at -inf is made first). False when it already was.
pub fn set_send_pre(t: &mut Track, to: &str, pre: bool) -> bool {
    if t.send(to).is_none() {
        set_send(t, to, 0.0, !pre);
    }
    let s = t.sends.iter_mut().find(|s| s.to == to).expect("just made");
    let done = s.pre != pre;
    s.pre = pre;
    done
}

/// Track header › Pan Type on the audio tracks `ids`.
pub fn set_pan_type(project: &mut Project, ids: &[String], pt: PanType) -> bool {
    let mut done = false;
    for t in project.timeline.audio_tracks.iter_mut().filter(|t| ids.contains(&t.id) && t.pan_type != pt) {
        t.pan_type = pt;
        done = true;
    }
    changed(project, done)
}

// ---- Plug-ins ----

/// What an FX window, a plug-in drop or the Color Grading panel edits (decision 0.5). Ids, so it survives edits.
#[derive(Clone, Debug, PartialEq)]
pub enum FxTarget {
    /// A video or audio event's or track's chain (the track kind says which).
    Event(String),
    Track(String),
    VideoOutput,
    AudioMaster,
    /// Generated media: its generator.
    Media(String),
    /// The transition into this event.
    Transition(String),
    /// A video track's Custom compositing mode: its compositor.
    Compositor(String),
    /// R8a: an audio bus's or assignable FX chain's FX.
    Bus(String),
}

/// The FX chain `target` names; None when it is gone, locked, or not a chain (Media, Transition).
pub fn fx_chain_mut<'a>(p: &'a mut Project, target: &FxTarget) -> Option<&'a mut Vec<Fx>> {
    match target {
        FxTarget::Event(id) => {
            let r = p.find_clip(id)?;
            let t = &mut p.timeline.tracks_mut(r.is_video)[r.track_idx];
            let free = !t.locked;
            let c = &mut t.clips[r.clip_idx];
            (free && !c.locked).then_some(&mut c.fx)
        }
        FxTarget::Track(id) => {
            let (v, i) = p.find_track(id)?;
            let t = &mut p.timeline.tracks_mut(v)[i];
            (!t.locked).then_some(&mut t.fx)
        }
        FxTarget::VideoOutput => Some(&mut p.timeline.master.video_fx),
        FxTarget::AudioMaster => Some(&mut p.timeline.master.audio_fx),
        FxTarget::Bus(id) => p.timeline.audio.bus_mut(id).map(|b| &mut b.fx),
        FxTarget::Media(_) | FxTarget::Transition(_) | FxTarget::Compositor(_) => None,
    }
}

/// `target` holds audio plug-ins: an audio event's or track's chain, or the Audio Master's.
pub fn fx_is_audio(p: &Project, target: &FxTarget) -> bool {
    match target {
        FxTarget::Event(id) => p.find_clip(id).is_some_and(|r| !r.is_video),
        FxTarget::Track(id) => p.find_track(id).is_some_and(|(v, _)| !v),
        FxTarget::AudioMaster | FxTarget::Bus(_) => true,
        _ => false,
    }
}

/// The plug-ins `target` holds as the FX window shows them (a transition as one plug-in keyed at 0, generated media as
/// its generator), and whether they may be changed. None when the target is gone.
pub fn fx_list(p: &Project, target: &FxTarget) -> Option<(Vec<Fx>, bool)> {
    let clip = |id: &str| {
        let r = p.find_clip(id)?;
        let t = &p.timeline.tracks(r.is_video)[r.track_idx];
        let c = &t.clips[r.clip_idx];
        Some((c, !t.locked && !c.locked))
    };
    match target {
        FxTarget::Event(id) => clip(id).map(|(c, free)| (c.fx.clone(), free)),
        FxTarget::Track(id) => p.find_track(id).map(|(v, i)| (p.timeline.tracks(v)[i].fx.clone(), !p.timeline.tracks(v)[i].locked)),
        FxTarget::VideoOutput => Some((p.timeline.master.video_fx.clone(), true)),
        FxTarget::AudioMaster => Some((p.timeline.master.audio_fx.clone(), true)),
        FxTarget::Bus(id) => p.timeline.audio.bus(id).map(|b| (b.fx.clone(), true)),
        FxTarget::Media(id) => p.media_pool.iter().find(|m| m.id == *id)?.generator.clone().map(|g| (vec![g], true)),
        FxTarget::Compositor(id) => p.find_track(id).and_then(|(_, i)| {
            let t = p.timeline.video_tracks.get(i)?;
            Some((vec![t.compositor.clone()?], !t.locked))
        }),
        FxTarget::Transition(owner) => {
            let (id, out) = split_owner(owner);
            clip(id).and_then(|(c, free)| Some((vec![fx_of_transition(if out { &c.transition_out } else { &c.transition }.as_ref()?)], free)))
        }
    }
}

/// An event id, and whether `owner` names the transition out of it (`TRANSITION_OUT`).
pub fn split_owner(owner: &str) -> (&str, bool) {
    owner.strip_suffix(TRANSITION_OUT).map_or((owner, false), |id| (id, true))
}

/// Put back what `fx_list` gave, edited. False when nothing changed or the target is gone or locked.
pub fn set_fx_list(p: &mut Project, target: &FxTarget, list: Vec<Fx>) -> bool {
    let done = match target {
        FxTarget::Media(id) => match (p.media_pool.iter_mut().find(|m| m.id == *id), list.into_iter().next()) {
            (Some(m), Some(g)) if m.generator.is_some() && m.generator.as_ref() != Some(&g) => {
                m.generator = Some(g);
                true
            }
            _ => false,
        },
        FxTarget::Compositor(id) => match (p.find_track(id), list.into_iter().next()) {
            (Some((true, i)), Some(f)) => {
                let t = &mut p.timeline.video_tracks[i];
                let ok = !t.locked && t.compositor.is_some() && t.compositor.as_ref() != Some(&f);
                if ok {
                    t.compositor = Some(f);
                }
                ok
            }
            _ => false,
        },
        FxTarget::Transition(owner) => {
            let (id, out) = split_owner(owner);
            let tr = list.first().map(transition_of);
            p.find_clip(id).is_some_and(|r| {
                let t = &mut p.timeline.tracks_mut(r.is_video)[r.track_idx];
                let free = !t.locked;
                let c = &mut t.clips[r.clip_idx];
                let slot = if out { &mut c.transition_out } else { &mut c.transition };
                // The FX window edits the plug-in and its settings; the Transition Progress envelope stays.
                let tr = tr.map(|tr| Transition { progress: slot.as_ref().map(|s| s.progress.clone()).unwrap_or_default(), ..tr });
                let ok = free && !c.locked && slot.is_some() && tr.is_some() && *slot != tr;
                if ok {
                    *slot = tr;
                }
                ok
            })
        }
        _ => fx_chain_mut(p, target).is_some_and(|c| {
            let ok = *c != list;
            *c = list;
            ok
        }),
    };
    changed(p, done)
}

/// A transition's settings as a plug-in instance (each value one key at 0), for the FX window.
pub fn fx_of_transition(tr: &Transition) -> Fx {
    let key = |v| Keyframes { keys: vec![Keyframe { t: 0.0, v, curve: Curve::Linear }] };
    Fx { id: "transition".into(), plugin: tr.plugin.clone(), enabled: true, params: tr.params.iter().map(|(k, &v)| (k.clone(), key(v))).collect(), text: Default::default(), auto: Default::default() }
}

/// A plug-in instance as a transition (transitions are not keyframed: each value at 0).
pub fn transition_of(fx: &Fx) -> Transition {
    let params = fx.params.iter().filter(|(_, k)| !k.keys.is_empty()).map(|(k, kf)| (k.clone(), kf.at(0.0))).collect();
    Transition { plugin: fx.plugin.clone(), params, progress: Keyframes::default() }
}

/// Append `fx` to `target`'s chain; refused (false) when it is locked or gone.
pub fn add_fx(p: &mut Project, target: &FxTarget, fx: Fx) -> bool {
    let done = fx_chain_mut(p, target).map(|c| c.push(fx)).is_some();
    changed(p, done)
}

pub fn remove_fx(p: &mut Project, target: &FxTarget, i: usize) -> bool {
    let done = fx_chain_mut(p, target).filter(|c| i < c.len()).map(|c| c.remove(i)).is_some();
    changed(p, done)
}

/// Move plug-in `from` so it ends up at index `to` (clamped to the chain).
pub fn move_fx(p: &mut Project, target: &FxTarget, from: usize, to: usize) -> bool {
    let done = fx_chain_mut(p, target).filter(|c| from < c.len() && from != to.min(c.len() - 1)).map(|c| {
        let f = c.remove(from);
        c.insert(to.min(c.len()), f);
    });
    changed(p, done.is_some())
}

/// The chain pill's bypass box.
pub fn set_fx_enabled(p: &mut Project, target: &FxTarget, i: usize, on: bool) -> bool {
    let done = fx_chain_mut(p, target).and_then(|c| c.get_mut(i)).filter(|f| f.enabled != on).map(|f| f.enabled = on).is_some();
    changed(p, done)
}

/// Where a transition dropped at `at` on a track goes, as clip indices.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Edge {
    /// Outgoing, incoming.
    Overlap(usize, usize),
    Cut(usize, usize),
    /// An event's start with nothing before it, or its end with nothing after it.
    Start(usize),
    End(usize),
}

/// Overlap: two events live at `at`. Otherwise the half of the event under `at` picks its start or its end, each a
/// cut when an event meets it there.
fn transition_edge(clips: &[Clip], at: f64, tol: f64) -> Option<Edge> {
    let end = |c: &Clip| c.position + c.duration;
    let live: Vec<usize> = (0..clips.len()).filter(|&k| clips[k].position <= at && at < end(&clips[k])).collect();
    let b = live.iter().copied().max_by(|&x, &y| clips[x].position.total_cmp(&clips[y].position))?;
    // An overlap within `tol` (float error, an unsnapped drag) is a cut.
    if let Some(a) = live.iter().copied().filter(|&k| clips[k].position < clips[b].position && end(&clips[k]) - clips[b].position > tol).max_by(|&x, &y| end(&clips[x]).total_cmp(&end(&clips[y]))) {
        return Some(Edge::Overlap(a, b));
    }
    let e = &clips[b];
    if at < e.position + e.duration * 0.5 {
        let prev = (0..clips.len()).find(|&k| k != b && clips[k].position < e.position && (end(&clips[k]) - e.position).abs() <= tol);
        Some(prev.map_or(Edge::Start(b), |a| Edge::Cut(a, b)))
    } else {
        // An event starting at the end is a cut; one starting inside the event crossfades with it (no lone fade-out).
        let next = (0..clips.len()).find(|&k| k != b && clips[k].position > e.position && clips[k].position <= end(e) + tol);
        match next {
            Some(n) if (clips[n].position - end(e)).abs() <= tol => Some(Edge::Cut(b, n)),
            Some(_) => None,
            None => Some(Edge::End(b)),
        }
    }
}

/// Drop transition `tr` on video track `track` at time `at`. On an overlap it becomes the incoming event's. At a cut,
/// with Automatic Crossfades on, the outgoing event is extended right and the incoming one left by `cut_overlap / 2`
/// each, within their media (one with no handle leaves the whole overlap to the other), and both fade over it. On an
/// event's start with nothing before it (its end with nothing after it), the event fades in (out) over `cut_overlap`
/// unless it already does, and the transition goes on that fade (`Clip::transition_out` for a fade-out). False
/// (nothing changed) when an event or the track is locked, or at a cut that cannot overlap.
pub fn set_transition(p: &mut Project, track: &str, at: f64, tr: Transition, cut_overlap: f64, auto_xfade: bool) -> bool {
    let Some((true, ti)) = p.find_track(track) else { return false };
    let fps = p.settings.frame_rate.max(1.0);
    let t = &p.timeline.video_tracks[ti];
    let Some(edge) = transition_edge(&t.clips, at, 0.5 / fps).filter(|_| !t.locked) else { return false };
    let free = |k: usize| !t.clips[k].locked;
    match edge {
        Edge::Overlap(a, b) | Edge::Cut(a, b) if !free(a) || !free(b) => return false,
        Edge::Start(b) | Edge::End(b) if !free(b) => return false,
        Edge::Cut(..) if !auto_xfade => return false,
        _ => {}
    }
    if let Edge::End(b) = edge {
        let c = &mut p.timeline.video_tracks[ti].clips[b];
        let mut done = c.fade_out == 0.0;
        if done {
            c.fade_out = cut_overlap;
            c.clamp_fades();
        }
        done |= c.transition_out.as_ref() != Some(&tr);
        c.transition_out = Some(tr);
        return changed(p, done);
    }
    let (b, na) = match edge {
        Edge::Overlap(_, b) | Edge::Start(b) | Edge::End(b) => (b, None),
        Edge::Cut(a, b) => {
            let Some((na, nb)) = widen_cut(p, &t.clips, a, b, cut_overlap) else { return false };
            p.timeline.video_tracks[ti].clips[b] = nb;
            (b, Some((a, na)))
        }
    };
    let clips = &mut p.timeline.video_tracks[ti].clips;
    let mut done = na.is_some();
    if let Some((a, na)) = na {
        clips[a] = na;
    }
    let c = &mut clips[b];
    if matches!(edge, Edge::Start(_)) && c.fade_in == 0.0 {
        c.fade_in = cut_overlap;
        c.clamp_fades();
        done = true;
    }
    done |= c.transition.as_ref() != Some(&tr);
    c.transition = Some(tr);
    changed(p, done)
}

/// The cut between `clips[a]` and `clips[b]` (`a` ends where `b` starts) made an overlap of `cut_overlap`: `a` extended
/// right and `b` left by half each, within their media; what one side cannot give, the other takes. Neither passes the
/// other's far edge (less a frame) or runs into the event beyond it. Both fade over the overlap; None when there is
/// no room at all.
fn widen_cut(p: &Project, clips: &[Clip], a: usize, b: usize, cut_overlap: f64) -> Option<(Clip, Clip)> {
    let (oa, ob) = (clips[a].clone(), clips[b].clone());
    let (la, lb) = (media_limit(p, &oa), media_limit(p, &ob));
    let (a_end, frame) = (oa.position + oa.duration, 1.0 / p.settings.frame_rate.max(1.0));
    let others = || clips.iter().enumerate().filter(|&(k, _)| k != a && k != b).map(|(_, c)| c);
    let next = others().filter(|c| c.position >= ob.position).map(|c| c.position).fold(f64::INFINITY, f64::min);
    let prev = others().filter(|c| c.position < oa.position).map(|c| c.position + c.duration).fold(f64::NEG_INFINITY, f64::max);
    let max_a = (ob.position + ob.duration - frame).min(next) - a_end;
    let max_b = ob.position - (oa.position + frame).max(prev);
    let ext_a = |w: f64| {
        let mut c = oa.clone();
        trim_right(&mut c, &oa, a_end + w.min(max_a).max(0.0), frame, la);
        c
    };
    let ext_b = |w: f64| {
        let mut c = ob.clone();
        trim_left(&mut c, &ob, ob.position - w.min(max_b).max(0.0), frame, lb);
        c
    };
    let ga = ext_a(cut_overlap * 0.5).duration - oa.duration;
    let mut nb = ext_b(cut_overlap - ga);
    let mut na = ext_a(cut_overlap - (ob.position - nb.position));
    let ov = na.position + na.duration - nb.position;
    if ov <= 1e-9 {
        return None;
    }
    (na.fade_out, nb.fade_in) = (ov, ov);
    na.clamp_fades();
    nb.clamp_fades();
    Some((na, nb))
}

/// R18b: Insert › Audio Transition...: the cut, crossfade or lone event edge at `at` on audio track `track` gets the
/// crossfade preset `(out, inc)` (the outgoing fade's curve, the incoming one's): a cut is made an overlap of `len`
/// (as a transition dropped on a video cut is), an existing crossfade takes the curves, and an event start (end) with
/// nothing beside it fades in (out) over `len` unless it already does. False (nothing changed) when an event or the
/// track is locked, or nothing is there.
pub fn audio_transition(p: &mut Project, track: &str, at: f64, len: f64, (out, inc): (Curve, Curve)) -> bool {
    let Some((false, ti)) = p.find_track(track) else { return false };
    let fps = p.settings.frame_rate.max(1.0);
    let t = &p.timeline.audio_tracks[ti];
    let Some(edge) = transition_edge(&t.clips, at, 0.5 / fps).filter(|_| !t.locked) else { return false };
    let locked = |k: usize| t.clips[k].locked;
    let (a, b) = match edge {
        Edge::Overlap(a, b) | Edge::Cut(a, b) => (Some(a), Some(b)),
        Edge::Start(b) => (None, Some(b)),
        Edge::End(a) => (Some(a), None),
    };
    if a.into_iter().chain(b).any(locked) {
        return false;
    }
    let widened = match edge {
        Edge::Cut(a, b) => Some(widen_cut(p, &t.clips, a, b, len).ok_or(()).map(|n| (a, b, n))),
        _ => None,
    };
    let clips = &mut p.timeline.audio_tracks[ti].clips;
    let before = (a.map(|k| clips[k].clone()), b.map(|k| clips[k].clone()));
    match widened {
        Some(Err(())) => return false,
        Some(Ok((a, b, (na, nb)))) => (clips[a], clips[b]) = (na, nb),
        None => {}
    }
    if let Some(a) = a {
        let c = &mut clips[a];
        if b.is_none() && c.fade_out == 0.0 {
            c.fade_out = len;
            c.clamp_fades();
        }
        c.fade_out_curve = out;
    }
    if let Some(b) = b {
        let c = &mut clips[b];
        if a.is_none() && c.fade_in == 0.0 {
            c.fade_in = len;
            c.clamp_fades();
        }
        c.fade_in_curve = inc;
    }
    let done = (a.map(|k| clips[k].clone()), b.map(|k| clips[k].clone())) != before;
    changed(p, done)
}

/// Event context menu › Remove Transition: the transition into event `id`, or `out` of it.
pub fn remove_transition(p: &mut Project, id: &str, out: bool) -> bool {
    let done = p.find_clip(id).is_some_and(|r| {
        let t = &mut p.timeline.tracks_mut(r.is_video)[r.track_idx];
        let free = !t.locked;
        let c = &mut t.clips[r.clip_idx];
        free && !c.locked && if out { c.transition_out.take() } else { c.transition.take() }.is_some()
    });
    changed(p, done)
}

/// Insert › Generated Media: a new media item drawn by `fx`, named `name`, and a `len`-second event of it at `at` on
/// video track `track` (the first video track when None; a new one when there is none or `track` is past the last), as
/// one edit. The event's id; None when the track is locked.
pub fn insert_generated(p: &mut Project, track: Option<usize>, at: f64, fx: Fx, name: &str, len: f64) -> Option<String> {
    let tl = &mut p.timeline;
    let i = track.unwrap_or(0).min(tl.video_tracks.len());
    if tl.video_tracks.get(i).is_some_and(|t| t.locked) {
        return None;
    }
    if i == tl.video_tracks.len() {
        tl.add_video_track();
    }
    // R18a: a Credit Roll is as long as its first event: it scrolls over that length (VEGAS's generated media length).
    let duration = if fx.plugin == crate::engine::generate::CREDITS { len } else { 86400.0 };
    // R18b: made at the project's frame size, which it keeps (VEGAS; Resize Generated Media matches it to the project).
    let (width, height) = (Some(p.settings.width), Some(p.settings.height));
    let m = MediaItem { id: uuid::Uuid::new_v4().to_string(), name: name.into(), media_type: MediaType::Image, duration, generator: Some(fx), width, height, ..Default::default() };
    let mut c = Clip::from_media(&m, at.max(0.0));
    (c.duration, c.out_point) = (len, len);
    let id = c.id.clone();
    let clips = &mut tl.video_tracks[i].clips;
    clips.push(c);
    clips.sort_by(|a, b| a.position.total_cmp(&b.position));
    p.media_pool.push(m);
    changed(p, true).then_some(id)
}

/// R18b: Insert › Adjustment Event (VEGAS 23): an Adjustment Event over `a..b` on video track `track` (no media; its
/// Event FX adjust the tracks below while it plays). Its id; None when the track is locked or not there.
pub fn insert_adjustment_event(p: &mut Project, track: usize, (a, b): (f64, f64)) -> Option<String> {
    let t = p.timeline.video_tracks.get_mut(track).filter(|t| !t.locked && b > a)?;
    let mut c = Clip::new("Adjustment Event", a.max(0.0), b - a.max(0.0));
    (c.adjustment, c.color) = (true, ADJUSTMENT_COLOR);
    let id = c.id.clone();
    t.clips.push(c);
    t.clips.sort_by(|x, y| x.position.total_cmp(&y.position));
    changed(p, true).then_some(id)
}

/// The colour an Adjustment Event is drawn in.
pub const ADJUSTMENT_COLOR: [u8; 3] = [128, 96, 168];

/// R18b: Tools › Scripting › Resize Generated Media (VEGAS's script): the generated media of the selected video events'
/// active takes (of every video event when none is selected) takes the project's frame size. False when every one
/// already has it.
pub fn resize_generated(p: &mut Project, sel: &[String]) -> bool {
    let tl = &p.timeline;
    let picked: Vec<&Clip> = tl.video_tracks.iter().flat_map(|t| &t.clips).filter(|c| sel.contains(&c.id)).collect();
    let events = if picked.is_empty() { tl.video_tracks.iter().flat_map(|t| &t.clips).collect() } else { picked };
    let ids: HashSet<String> = events.iter().filter_map(|c| c.media_id.clone()).collect();
    let size = (Some(p.settings.width), Some(p.settings.height));
    let mut done = false;
    for m in p.media_pool.iter_mut().filter(|m| m.generator.is_some() && ids.contains(&m.id)) {
        done |= (m.width, m.height) != size;
        (m.width, m.height) = size;
    }
    changed(p, done)
}

/// R18b: Insert › Time...: `len` seconds of empty time at `at`. What starts at or after it moves later on `tracks`
/// (every track when empty, as VEGAS does with no track selected), with the markers, regions, command markers and the
/// time selection after it; an event across `at` stays. Locked tracks and events stay.
pub fn insert_time(p: &mut Project, at: f64, len: f64, tracks: &[String]) -> bool {
    if len <= 0.0 {
        return false;
    }
    let mode = if tracks.is_empty() { RippleMode::All } else { RippleMode::AffectedMarkers };
    ripple(p, &RippleOp { from: at, delta: len, tracks: tracks.to_vec(), ..Default::default() }, mode)
}

/// R18b: the pieces of events inside the time selection `a..b`, as VEGAS's Cut and Copy take them: events `ids` (every
/// event, when empty) are split at both ends, and the ids of the parts inside are returned. Locked events and tracks
/// are left whole and out.
pub fn split_range(p: &mut Project, ids: &[String], (a, b): (f64, f64)) -> Vec<String> {
    const EPS: f64 = 1e-9;
    let old: HashSet<String> = all_clips(&p.timeline).map(|c| c.id.clone()).collect();
    // A piece split off a picked event has a new id.
    let pick = |c: &Clip| ids.is_empty() || ids.contains(&c.id) || !old.contains(&c.id);
    split_where(p, a, |_, c| pick(c));
    split_where(p, b, |_, c| pick(c));
    let tl = &p.timeline;
    tl.video_tracks
        .iter()
        .chain(&tl.audio_tracks)
        .filter(|t| !t.locked)
        .flat_map(|t| &t.clips)
        .filter(|c| !c.locked && pick(c) && c.position >= a - EPS && c.position + c.duration <= b + EPS)
        .map(|c| c.id.clone())
        .collect()
}

/// R18b: Copy with a time selection: the parts of events `ids` (every event when empty) inside `a..b`, placed as they
/// sit from `a` (`copy_clips` counts from the earliest).
pub fn copy_range(p: &Project, ids: &[String], (a, b): (f64, f64)) -> Vec<(bool, usize, Clip)> {
    let mut tmp = p.clone();
    let inside = split_range(&mut tmp, ids, (a, b));
    let t0 = inside.iter().filter_map(|id| tmp.clip(id)).map(|c| c.position).fold(b, f64::min);
    let mut out = copy_clips(&tmp, &inside);
    for (_, _, c) in &mut out {
        c.position += t0 - a;
    }
    out
}

/// R18b: Paste Insert splits the events under the cursor `at` on `tracks` (every track when `all`), so their second
/// halves make room with what follows (VEGAS).
pub fn split_for_insert(p: &mut Project, at: f64, tracks: &[String], all: bool) -> bool {
    split_where(p, at, |t, _| all || tracks.contains(&t.id))
}

/// R18b: Insert › Slideshow (VEGAS 23's Slideshow Creator): how the pictures are laid out.
#[derive(Clone, Debug, PartialEq)]
pub struct Slideshow {
    /// Each picture's length, and how much neighbours overlap (the transition's length).
    pub len: f64,
    pub overlap: f64,
    /// The transitions between pictures, used in turn (or at random); none: a plain crossfade.
    pub transitions: Vec<Transition>,
    pub random: bool,
}

/// R18b: the pictures among pool media `idxs` (in that order) as a slideshow from `at` on video track `track` (the
/// first one when None; a new one when there is none): each `len` long, overlapping the one before by `overlap` with a
/// crossfade or the next transition. The new ids; empty (nothing changed) when the track is locked or no picture is
/// usable.
pub fn insert_slideshow(p: &mut Project, idxs: &[usize], track: Option<usize>, at: f64, s: &Slideshow) -> Vec<String> {
    let pics: Vec<MediaItem> = idxs.iter().filter_map(|&i| p.media_pool.get(i)).filter(|m| m.media_type == MediaType::Image && m.generator.is_none()).cloned().collect();
    let tl = &mut p.timeline;
    let i = track.unwrap_or(0).min(tl.video_tracks.len());
    if pics.is_empty() || s.len <= 0.0 || tl.video_tracks.get(i).is_some_and(|t| t.locked) {
        return Vec::new();
    }
    if i == tl.video_tracks.len() {
        tl.add_video_track();
    }
    let overlap = s.overlap.clamp(0.0, s.len * 0.5);
    let mut ids = Vec::new();
    for (k, m) in pics.iter().enumerate() {
        let mut c = Clip::from_media(m, at.max(0.0) + k as f64 * (s.len - overlap));
        (c.duration, c.out_point) = (s.len, s.len);
        if overlap > 0.0 && k + 1 < pics.len() {
            c.fade_out = overlap;
        }
        if overlap > 0.0 && k > 0 {
            c.fade_in = overlap;
            let n = s.transitions.len();
            let pick = if s.random { (uuid::Uuid::new_v4().as_u128() % n.max(1) as u128) as usize } else { (k - 1) % n.max(1) };
            c.transition = s.transitions.get(pick).cloned();
        }
        ids.push(c.id.clone());
        tl.video_tracks[i].clips.push(c);
    }
    tl.video_tracks[i].clips.sort_by(|x, y| x.position.total_cmp(&y.position));
    changed(p, true);
    ids
}

/// A stopped recording: the WAV at `path` (`frames` at `rate` Hz) goes into Project Media and, as one event, onto every
/// armed, unlocked audio track. Its first sample belongs at timeline time `at`; what falls before `from` (where
/// recording started) or 0 is trimmed off the event's start. False, and nothing changed, when no track takes it.
pub fn place_recording(p: &mut Project, path: &std::path::Path, frames: u64, rate: u32, at: f64, from: f64) -> bool {
    let tracks: Vec<usize> = p.timeline.audio_tracks.iter().enumerate().filter(|(_, t)| t.armed && !t.locked).map(|(i, _)| i).collect();
    let len = frames as f64 / rate.max(1) as f64;
    let cut = (from.max(0.0) - at).max(0.0);
    if tracks.is_empty() || len <= cut {
        return false;
    }
    let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let m = MediaItem {
        id: uuid::Uuid::new_v4().to_string(),
        path: path.to_path_buf(),
        name,
        media_type: MediaType::Audio,
        duration: len,
        sample_rate: Some(rate),
        codec: Some("pcm_f32le".into()),
        ..Default::default()
    };
    for i in tracks {
        let mut c = Clip::from_media(&m, at + cut);
        (c.in_point, c.duration) = (cut, len - cut);
        let clips = &mut p.timeline.audio_tracks[i].clips;
        clips.push(c);
        clips.sort_by(|a, b| a.position.total_cmp(&b.position));
    }
    p.media_pool.push(m);
    changed(p, true)
}

/// Tools › Render to New Track: the rendered file `media_pool[idx]` on new tracks at the top (a new first video track
/// for its picture, a new first audio track for its sound, grouped), starting at `at`. The new events' ids.
pub fn place_on_new_tracks(p: &mut Project, idx: usize, at: f64) -> Vec<String> {
    let Some(m) = p.media_pool.get(idx) else { return Vec::new() };
    let video = matches!(m.media_type, MediaType::Video | MediaType::Image);
    let audio = m.media_type == MediaType::Audio || (video && m.sample_rate.is_some());
    if video {
        insert_track(p, true, Some(0));
    }
    if audio {
        insert_track(p, false, Some(0));
    }
    drop_media_as(p, &[idx], None, Some((video, 0)), at, 5.0, false, DropMode::AcrossTime)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R12: Render to New Track puts the file at the start of what was rendered, on new tracks above the others.
    #[test]
    fn rendered_file_goes_on_new_tracks() {
        let mut p = Project::new("t");
        p.timeline.audio_tracks[0].clips.push(Clip::new("old", 0.0, 9.0));
        let av = MediaItem { id: "r".into(), name: "r.mp4".into(), media_type: MediaType::Video, duration: 4.0, sample_rate: Some(48_000), ..Default::default() };
        let wav = MediaItem { id: "w".into(), name: "w.wav".into(), media_type: MediaType::Audio, duration: 2.0, sample_rate: Some(48_000), ..Default::default() };
        p.media_pool = vec![av, wav];
        let ids = place_on_new_tracks(&mut p, 0, 3.0);
        let tl = &p.timeline;
        assert_eq!((ids.len(), tl.video_tracks.len(), tl.audio_tracks.len()), (2, 2, 2));
        let (v, a) = (&tl.video_tracks[0].clips[0], &tl.audio_tracks[0].clips[0]);
        assert_eq!((v.position, v.duration, a.position, v.media_id.as_deref()), (3.0, 4.0, 3.0, Some("r")));
        assert!(v.group.is_some() && v.group == a.group && tl.video_tracks[1].clips.is_empty() && tl.audio_tracks[1].clips[0].name == "old");
        let ids = place_on_new_tracks(&mut p, 1, 1.5);
        assert_eq!((ids.len(), p.timeline.video_tracks.len(), p.timeline.audio_tracks.len()), (1, 2, 3));
        assert_eq!(p.timeline.audio_tracks[0].clips[0].position, 1.5);
        assert!(p.modified);
    }

    #[test]
    fn place_recording_on_armed_unlocked_tracks() {
        let mut p = Project::new("t");
        let path = std::path::Path::new(r"C:\Shows\t Audio 1 Rec 001.wav");
        assert!(!place_recording(&mut p, path, 48_000, 48_000, 2.0, 0.0), "no armed track");
        p.timeline.add_audio_track();
        p.timeline.add_audio_track();
        let tl = &mut p.timeline.audio_tracks;
        (tl[0].armed, tl[1].armed, tl[1].locked) = (true, true, true);
        let mut h = crate::history::History::default();
        let before = p.clone();
        assert!(place_recording(&mut p, path, 96_000, 48_000, 2.3, 2.0), "capture started 0.3 s after the playhead");
        h.push(before);
        let m = &p.media_pool[0];
        assert_eq!((p.media_pool.len(), m.duration, m.media_type, m.path.as_path()), (1, 2.0, MediaType::Audio, path));
        let counts: Vec<usize> = p.timeline.audio_tracks.iter().map(|t| t.clips.len()).collect();
        assert_eq!(counts, [1, 0, 0], "only the armed, unlocked track");
        let c = &p.timeline.audio_tracks[0].clips[0];
        assert!((c.position - 2.3).abs() < 1e-9 && c.duration == 2.0 && c.in_point == 0.0);
        assert_eq!(c.media_id.as_deref(), Some(m.id.as_str()));
        // One undo step takes it all back.
        assert!(h.undo(&mut p) && !h.can_undo());
        assert!(p.media_pool.is_empty() && p.timeline.audio_tracks[0].clips.is_empty());
        // Audio from before the recording started (capture ahead of playback, or the latency offset) is trimmed off
        // the head, as is anything before 0.
        p.timeline.audio_tracks[0].armed = true;
        assert!(place_recording(&mut p, path, 48_000, 48_000, 1.6, 2.0));
        let c = &p.timeline.audio_tracks[0].clips[0];
        assert!(c.position == 2.0 && (c.in_point - 0.4).abs() < 1e-9 && (c.duration - 0.6).abs() < 1e-9);
        p.timeline.audio_tracks[0].clips.clear();
        assert!(place_recording(&mut p, path, 48_000, 48_000, -0.2, -0.1));
        let c = &p.timeline.audio_tracks[0].clips[0];
        assert!(c.position == 0.0 && (c.in_point - 0.2).abs() < 1e-9 && (c.duration - 0.8).abs() < 1e-9);
        assert!(!place_recording(&mut p, path, 48_000, 48_000, 0.0, 1.0), "nothing left after the start");
    }

    #[test]
    fn media_limit_is_unlimited_for_generated_media() {
        let mut p = Project::new("t");
        let mut m = MediaItem { id: "v".into(), media_type: MediaType::Video, duration: 8.0, ..Default::default() };
        p.media_pool.push(m.clone());
        let mut c = Clip::from_media(&m, 0.0);
        c.looping = false;
        assert_eq!(media_limit(&p, &c), Some(8.0));
        m.generator = Some(Fx { id: "f".into(), plugin: "milan.gen.solid".into(), enabled: true, params: Default::default(), text: Default::default(), auto: Default::default() });
        p.media_pool[0] = m;
        assert_eq!(media_limit(&p, &c), None);
    }

    /// R3a: a looping event may be dragged past its media end (only the media start bounds it); with Loop off it stops
    /// there. Too-short media and stills never loop.
    #[test]
    fn a_looping_event_extends_past_its_media_end() {
        let mut p = Project::new("t");
        let m = MediaItem { id: "v".into(), media_type: MediaType::Video, duration: 8.0, ..Default::default() };
        p.media_pool.push(m.clone());
        let o = Clip::from_media(&m, 0.0);
        assert!(o.looping, "events loop by default, as in VEGAS");
        let limit = media_limit(&p, &o);
        assert_eq!(limit, Some(f64::INFINITY));
        let mut c = o.clone();
        trim_right(&mut c, &o, 20.0, F, limit);
        assert_eq!((c.duration, c.out_point), (20.0, 20.0));
        trim_left(&mut c, &o, -5.0, F, limit); // the left edge stops at the media start
        assert_eq!((c.position, c.in_point), (0.0, 0.0));
        let mut off = o.clone();
        off.looping = false;
        let mut c = off.clone();
        trim_right(&mut c, &off, 20.0, F, media_limit(&p, &off));
        assert_eq!(c.duration, 8.0);
        assert_eq!(loop_len(&o, &m), Some(8.0));
        assert_eq!(loop_len(&off, &m), None);
        assert_eq!(loop_len(&o, &MediaItem { duration: 0.01, ..m.clone() }), None);
        assert_eq!(loop_len(&o, &MediaItem { media_type: MediaType::Image, ..m.clone() }), None);
        // Wrapping: a hair under the end is the start again; a negative time never comes out.
        assert_eq!((loop_time(9.0, 8.0), loop_time(16.0, 8.0), loop_time(8.0 - 1e-9, 8.0)), (1.0, 0.0, 0.0));
    }

    /// R3a: an 8 s clip extended to 20 s plays source 0..8, 0..8, 0..4; slipped, from its in point; reversed, back to front.
    #[test]
    fn loop_passes_split_at_the_media_end() {
        let mut c = Clip::new("c", 1.0, 20.0);
        assert_eq!(loop_passes(&c, Some(8.0)), [(1.0, 8.0, 0.0), (9.0, 8.0, 0.0), (17.0, 4.0, 0.0)]);
        assert_eq!(loop_passes(&c, None), [(1.0, 20.0, 0.0)]);
        c.rate = 2.0; // 40 s of source in 20 s: five passes of 4 s
        assert_eq!(loop_passes(&c, Some(8.0)).len(), 5);
        (c.rate, c.in_point, c.duration) = (1.0, 6.0, 4.0); // 6..8 then 0..2
        assert_eq!(loop_passes(&c, Some(8.0)), [(1.0, 2.0, 6.0), (3.0, 2.0, 0.0)]);
        c.reverse = true; // 2..0 then 8..6, from the left
        assert_eq!(loop_passes(&c, Some(8.0)), [(1.0, 2.0, 0.0), (3.0, 2.0, 6.0)]);
        (c.reverse, c.in_point) = (false, 10.0); // slipped past the end: inside the second pass
        assert_eq!(loop_passes(&c, Some(8.0)), [(1.0, 4.0, 2.0)]);
    }

    /// R3a: Insert › Empty Event fills the time selection on the chosen unlocked tracks.
    #[test]
    fn empty_events_fill_the_selection() {
        let mut p = Project::new("t");
        p.timeline.audio_tracks[0].locked = true;
        let tracks: Vec<String> = [&p.timeline.video_tracks[0], &p.timeline.audio_tracks[0]].map(|t| t.id.clone()).to_vec();
        let ids = insert_empty(&mut p, &tracks, (2.0, 5.0));
        assert_eq!(ids.len(), 1, "not on the locked track");
        let c = p.clip(&ids[0]).unwrap();
        assert_eq!((c.position, c.duration, c.media_id.as_ref()), (2.0, 3.0, None));
        assert!(insert_empty(&mut p, &tracks, (5.0, 5.0)).is_empty());
        // No media: nothing to draw or hear.
        assert!(crate::engine::plan::frame_plan(&crate::engine::plan::compile(&p), 90).layers.is_empty());
    }

    /// R3a: a subclip is a stretch of its file used as a whole file: events made from it, and an event turned into
    /// one, play the same source frames.
    #[test]
    fn subclips_play_their_stretch_of_the_file() {
        let mut p = Project::new("t");
        p.settings.frame_rate = 30.0;
        let m = MediaItem { id: "v".into(), path: "v.mp4".into(), name: "v.mp4".into(), media_type: MediaType::Video, duration: 10.0, width: Some(64), height: Some(64), sample_rate: Some(48_000), ..Default::default() };
        p.media_pool.push(m.clone());
        let sid = create_subclip(&mut p, "v", 2.0, 3.0).unwrap();
        let s = p.media_pool[1].clone();
        assert_eq!((s.name.as_str(), s.duration, s.path.as_path(), s.subclip.as_ref().map(|x| (x.parent.as_str(), x.start))), ("v.mp4 Subclip 1", 3.0, std::path::Path::new("v.mp4"), Some(("v", 2.0))));
        // A subclip of the subclip is of the same file, its start counted in the file.
        let s2 = create_subclip(&mut p, &sid, 1.0, 5.0).unwrap();
        let s2 = p.media_pool.iter().find(|x| x.id == s2).unwrap();
        assert_eq!((s2.duration, s2.subclip.as_ref().map(|x| (x.parent.as_str(), x.start))), (2.0, Some(("v", 3.0))));
        assert!(create_subclip(&mut p, "v", 10.0, 1.0).is_none(), "nothing left past the end");
        // An event from the subclip shows file frame 60 + k at its frame k (2 s in, at 30 fps), and loops over 3 s.
        let mut c = Clip::from_media(&s, 0.0);
        c.duration = 4.0;
        p.timeline.video_tracks[0].clips.push(c);
        let sf = |p: &Project, n: i64| match &crate::engine::plan::frame_plan(&crate::engine::plan::compile(p), n).layers[0].items[0] {
            crate::engine::plan::Item::Event(e) => e.src.sf,
            other => panic!("{other:?}"),
        };
        assert_eq!((sf(&p, 0), sf(&p, 45), sf(&p, 95)), (Some(60), Some(105), Some(65)));
        // Its sound is read from the same stretch of the file, one stream per pass.
        let mut c = Clip::from_media(&s, 0.0);
        c.duration = 4.0;
        p.timeline.audio_tracks[0].clips.push(c);
        let a = crate::engine::plan::compile_audio(&p);
        let passes: Vec<(f64, f64, f64)> = a.tracks[0].events[0].passes.iter().map(|s| (s.pos, s.dur, s.in_point)).collect();
        assert_eq!(passes, [(0.0, 3.0, 2.0), (3.0, 1.0, 2.0)]);
        // An event of the file turned into a subclip keeps showing the same frames.
        let mut q = Project::new("t");
        q.settings.frame_rate = 30.0;
        q.media_pool.push(m.clone());
        let mut e = Clip::from_media(&m, 1.0);
        (e.in_point, e.duration) = (4.0, 2.0);
        let id = e.id.clone();
        q.timeline.video_tracks[0].clips.push(e);
        let before: Vec<_> = (30..90).map(|n| sf(&q, n)).collect();
        let sub = subclip_event(&mut q, &id).unwrap();
        let e = q.clip(&id).unwrap();
        assert_eq!((e.media_id.as_deref(), e.in_point), (Some(sub.as_str()), 0.0));
        assert_eq!((30..90).map(|n| sf(&q, n)).collect::<Vec<_>>(), before);
    }

    /// R3a: Paste Event Attributes copies the chosen attributes of the copied event; FX only leaves fades alone.
    #[test]
    fn attribute_paste_copies_only_what_is_chosen() {
        let mut p = Project::new("t");
        let mut src = Clip::new("src", 0.0, 4.0);
        src.fx = vec![Fx { id: "fx1".into(), plugin: "milan.fx.invert".into(), enabled: true, params: Default::default(), text: Default::default(), auto: Default::default() }];
        (src.fade_in, src.fade_out, src.gain_db, src.rate) = (1.0, 1.0, -6.0, 2.0);
        src.pan_crop = Keyframes { keys: vec![Keyframe { t: 0.0, v: PanCrop { w: 0.5, ..Default::default() }, curve: Curve::Linear }] };
        let mut dst = Clip::new("dst", 10.0, 4.0);
        dst.fade_in = 0.5;
        let id = dst.id.clone();
        p.timeline.video_tracks[0].clips.push(dst);
        let board = vec![(true, 0, src.clone())];
        let fx_only = Attrs { fx: true, pan_crop: false, envelopes: false, gain: false, rate: false, fades: false };
        assert!(paste_attrs(&mut p, &board, &[id.clone()], fx_only));
        let c = p.clip(&id).unwrap();
        assert_eq!((c.fx.len(), c.fx[0].plugin.as_str(), c.fade_in, c.fade_out, c.rate), (1, "milan.fx.invert", 0.5, 0.0, 1.0));
        assert_ne!(c.fx[0].id, "fx1", "a new FX instance");
        assert!(c.pan_crop.keys.is_empty());
        assert!(paste_attrs(&mut p, &board, &[id.clone()], Attrs::ALL));
        let c = p.clip(&id).unwrap();
        assert_eq!((c.fade_in, c.fade_out, c.rate, c.gain_db, c.pan_crop.keys.len(), c.duration, c.position), (1.0, 1.0, 2.0, -6.0, 1, 4.0, 10.0));
        assert!(!paste_attrs(&mut p, &board, &[id.clone()], Attrs::ALL), "nothing new the second time");
        // A copied video event pastes nothing onto audio events.
        let a = Clip::new("a", 0.0, 2.0);
        let aid = a.id.clone();
        p.timeline.audio_tracks[0].clips.push(a);
        assert!(!paste_attrs(&mut p, &board, &[aid], Attrs::ALL));
    }

    const F: f64 = 1.0 / 30.0;

    fn clip(pos: f64, dur: f64, inp: f64) -> Clip {
        let mut c = Clip::new("c", pos, dur);
        c.in_point = inp;
        c.out_point = inp + dur;
        c
    }

    #[test]
    fn trim_left_clamps_in_point_and_min_duration() {
        let o = clip(10.0, 5.0, 2.0);
        let mut c = o.clone();
        trim_left(&mut c, &o, 0.0, F, Some(20.0)); // can only reveal 2s of head
        assert_eq!((c.position, c.in_point, c.duration), (8.0, 0.0, 7.0));
        trim_left(&mut c, &o, 100.0, F, Some(20.0)); // can't pass the right edge
        assert!((c.duration - F).abs() < 1e-9 && (c.position + c.duration - 15.0).abs() < 1e-9);
        assert_eq!(c.out_point, o.out_point);
        trim_left(&mut c, &o, 11.0, F, Some(20.0));
        assert_eq!((c.position, c.in_point, c.duration), (11.0, 3.0, 4.0));
    }

    #[test]
    fn trim_right_clamps_to_media_and_frame() {
        let o = clip(10.0, 5.0, 2.0);
        let mut c = o.clone();
        trim_right(&mut c, &o, 100.0, F, Some(10.0)); // 8s of source left after in_point 2
        assert_eq!((c.duration, c.out_point), (8.0, 10.0));
        trim_right(&mut c, &o, 0.0, F, Some(10.0));
        assert!((c.duration - F).abs() < 1e-9);
        trim_right(&mut c, &o, 100.0, F, None); // images unlimited
        assert_eq!(c.duration, 90.0);
    }

    #[test]
    fn snapping_picks_nearest_edge_within_threshold() {
        let t = [0.0, 5.0, 10.0];
        assert_eq!(snap(5.05, &t, 0.1), Some(5.0));
        assert_eq!(snap(5.5, &t, 0.1), None);
        let s = Snaps { points: t.to_vec(), grid: None };
        assert!((s.span(4.95, 2.0, 0.1) - 5.0).abs() < 1e-9); // start snaps
        assert!((s.span(2.98, 7.0, 0.1) - 3.0).abs() < 1e-9); // end snaps to 10
        assert_eq!(s.span(3.0, 1.0, 0.1), 3.0);
    }

    /// R1: with Snap to Grid on a 1 s grid, times land on whole seconds; the nearer of a point and a line wins.
    #[test]
    fn snap_to_a_one_second_grid_lands_on_whole_seconds() {
        let mut s = Snaps { points: vec![7.03], grid: Some((1.0, 0.0)) };
        for (t, want) in [(3.04, Some(3.0)), (11.97, Some(12.0)), (0.02, Some(0.0)), (5.5, None), (7.06, Some(7.03)), (6.98, Some(7.0))] {
            assert_eq!(s.nearest(t, 0.08), want, "{t}");
        }
        assert!((s.span(2.96, 1.5, 0.08) - 3.0).abs() < 1e-12 && s.hits(3.0) && s.hits(7.03) && !s.hits(3.5));
        // Through a phase (a ruler start time of 0.25 s): lines at .25.
        s.grid = Some((1.0, 0.25));
        assert_eq!(s.nearest(4.3, 0.08), Some(4.25));
        // Move Left/Right by Grid Marks: the next line each way, a whole step from a line, never before 0.
        let g = (1.0, 0.0);
        assert_eq!((grid_step(3.4, g, true), grid_step(3.4, g, false), grid_step(3.0, g, true), grid_step(3.0, g, false)), (4.0, 3.0, 4.0, 2.0));
        assert_eq!(grid_step(0.5, g, false), 0.0);
        assert_eq!(grid_step(0.0, g, false), 0.0);
        let f = 1001.0 / 30000.0; // a frame grid at 29.97 a long way in
        assert!((grid_step(100_000.0 * f, (f, 0.0), true) - 100_001.0 * f).abs() < 1e-9);
    }

    /// R1: Snap to All Events off snaps an event only to edges on its own tracks.
    #[test]
    fn snap_to_all_events_reaches_other_tracks() {
        let mut p = Project::new("t");
        p.timeline.video_tracks[0].clips.push(Clip::new("v", 2.0, 3.0));
        p.timeline.audio_tracks[0].clips.push(Clip::new("a", 4.0, 3.0));
        let v1 = vec![p.timeline.video_tracks[0].id.clone()];
        let all = snap_targets_except(&p, &[], None);
        let own = snap_targets_except(&p, &[], Some(&v1));
        assert!(all.contains(&7.0) && !own.contains(&7.0) && own.contains(&5.0) && own.contains(&0.0));
    }

    #[test]
    fn split_math() {
        let c = clip(10.0, 6.0, 1.0);
        let (a, b) = split_clip(&c, 12.0).unwrap();
        assert_eq!((a.position, a.duration, a.in_point, a.out_point), (10.0, 2.0, 1.0, 3.0));
        assert_eq!((b.position, b.duration, b.in_point, b.out_point), (12.0, 4.0, 3.0, 7.0));
        assert_ne!(a.id, b.id);
        assert!(split_clip(&c, 10.0).is_none() && split_clip(&c, 16.0).is_none());
        let mut f = c.clone();
        (f.fade_in, f.fade_out) = (1.0, 3.0);
        let (a, b) = split_clip(&f, 12.0).unwrap();
        assert_eq!((a.fade_in, a.fade_out, b.fade_in, b.fade_out), (1.0, 0.0, 0.0, 3.0));
    }

    #[test]
    fn split_keeps_the_transition_on_the_first_half_only() {
        let mut c = clip(10.0, 6.0, 1.0);
        (c.fade_in, c.transition) = (1.0, Some(Transition { plugin: "dissolve".into(), ..Default::default() }));
        (c.fade_out, c.transition_out) = (1.0, c.transition.clone());
        let (a, b) = split_clip(&c, 12.0).unwrap();
        assert!(a.transition.is_some() && b.transition.is_none());
        assert!(a.transition_out.is_none() && b.transition_out.is_some(), "the transition out stays with the end");
    }

    /// R4a: Velocity. Constant 200% plays 2 s of media a second; a ramp 100% -> 0% over 1 s plays half a second; a
    /// freeze holds the frame at the cursor; a forward trim and a split keep each frame where it was.
    #[test]
    fn velocity_drives_the_source_time() {
        let key = |t, v, curve| Keyframe { t, v, curve };
        let mut c = clip(10.0, 6.0, 1.0);
        c.envelopes.push(Envelope { kind: EnvKind::Velocity, points: Keyframes { keys: vec![key(0.0, 2.0, Curve::Linear)] } });
        assert_eq!([src_time(&c, 10.0), src_time(&c, 11.0), src_time(&c, 13.0)], [1.0, 3.0, 7.0]);
        c.envelopes[0].points.keys = vec![key(0.0, 1.0, Curve::Linear), key(1.0, 0.0, Curve::Linear)];
        assert!((src_time(&c, 11.0) - 1.5).abs() < 1e-12 && (src_time(&c, 15.0) - 1.5).abs() < 1e-12);
        // Reversed: the same span, played from its end.
        c.reverse = true;
        assert!((src_time(&c, 10.0) - 1.5).abs() < 1e-12 && (src_time(&c, 16.0) - 1.0).abs() < 1e-12);
        c.reverse = false;
        // Freeze Frame at Cursor on a plain event: 100% up to the cursor, then the frame there holds.
        let mut p = Project::new("t");
        let mut e = clip(10.0, 6.0, 1.0);
        let id = e.id.clone();
        e.envelopes.clear();
        p.timeline.video_tracks[0].clips.push(e);
        assert!(freeze_frame(&mut p, &[id.clone()], 12.0));
        let e = p.clip(&id).unwrap();
        assert_eq!([src_time(e, 11.0), src_time(e, 12.0), src_time(e, 13.0), src_time(e, 15.9)], [2.0, 3.0, 3.0, 3.0]);
        let fps = 30.0;
        let frame = |p: &Project, t: f64| crate::engine::plan::frame_at(src_time(p.clip(&id).unwrap(), t), fps);
        assert!((360..480).all(|n| frame(&p, n as f64 / fps) == 90), "frame 90 (3 s) holds");
        // A second freeze earlier keeps the speed before it and holds from there on (the first freeze point stays).
        assert!(freeze_frame(&mut p, &[id.clone()], 11.0) && src_time(p.clip(&id).unwrap(), 14.0) == 2.0);
        // Trim the start 1 s later and split: the frames stay where they were on the timeline.
        let before = p.clip(&id).unwrap().clone();
        let mut t = before.clone();
        trim_left(&mut t, &before, 10.5, F, None);
        for x in [10.5, 10.9, 11.0, 12.0, 15.0] {
            assert!((src_time(&t, x) - src_time(&before, x)).abs() < 1e-9, "{x}");
        }
        let (a, b) = split_clip(&before, 10.5).unwrap();
        assert!([10.0, 10.25].iter().all(|&x| (src_time(&a, x) - src_time(&before, x)).abs() < 1e-9));
        assert!([10.5, 10.75, 11.0, 14.0].iter().all(|&x| (src_time(&b, x) - src_time(&before, x)).abs() < 1e-9));
        // Remove: Insert/Remove Envelope › Velocity toggles.
        assert!(toggle_velocity(&mut p, &[id.clone()]) && crate::engine::velocity::of(p.clip(&id).unwrap()).is_none());
        assert!(toggle_velocity(&mut p, &[id.clone()]) && p.clip(&id).unwrap().envelopes[0].points.keys[0].v == 1.0);
    }

    /// R4b: with Options › Audio Follows Video Velocity on, the audio of an A/V pair (grouped, same place and length)
    /// follows its video event's Velocity: inserted, frozen, edited and removed with it. Audio elsewhere, or moved off,
    /// keeps its own speed. Off (the default, VEGAS) it never gets one, and turning it off removes the copies.
    #[test]
    fn audio_follows_its_video_velocity() {
        let mut p = Project::new("t");
        assert!(!p.timeline.audio_follows_velocity, "VEGAS: audio keeps normal speed");
        assert!(set_audio_follows_velocity(&mut p, true) && !set_audio_follows_velocity(&mut p, true));
        let (mut v, mut a, mut other) = (clip(10.0, 6.0, 1.0), clip(10.0, 6.0, 1.0), clip(10.0, 5.0, 1.0));
        for c in [&mut v, &mut a, &mut other] {
            (c.envelopes, c.group) = (Vec::new(), Some("g".into()));
        }
        let (vid, aid, oid) = (v.id.clone(), a.id.clone(), other.id.clone());
        p.timeline.video_tracks[0].clips.push(v);
        p.timeline.audio_tracks[0].clips.extend([a, other]);
        let vel = |p: &Project, id: &str| crate::engine::velocity::of(p.clip(id).unwrap()).map(|e| e.points.keys.clone());
        assert!(toggle_velocity(&mut p, &[vid.clone()]));
        assert!(vel(&p, &aid).is_some() && vel(&p, &oid).is_none(), "the other length is not its pair");
        assert!(freeze_frame(&mut p, &[vid.clone()], 12.0));
        assert!(edit_envelope(&mut p, &vid, EnvKind::Velocity, |e| {
            e.points.keys[0].v = 2.0;
            true
        }));
        assert_eq!(vel(&p, &aid), vel(&p, &vid));
        assert_eq!(src_time(p.clip(&aid).unwrap(), 11.0), src_time(p.clip(&vid).unwrap(), 11.0));
        // Off: the copy goes, later edits leave the audio alone; back on, it copies again.
        assert!(set_audio_follows_velocity(&mut p, false) && vel(&p, &aid).is_none() && vel(&p, &vid).is_some());
        assert!(freeze_frame(&mut p, &[vid.clone()], 14.0) && vel(&p, &aid).is_none());
        assert!(set_audio_follows_velocity(&mut p, true) && vel(&p, &aid) == vel(&p, &vid));
        assert!(toggle_velocity(&mut p, &[vid.clone()]) && vel(&p, &aid).is_none());
        p.timeline.audio_follows_velocity = false;
        assert!(toggle_velocity(&mut p, &[vid.clone()]) && vel(&p, &vid).is_some() && vel(&p, &aid).is_none());
    }

    /// R4b: Pitch Up / Down move the selected audio events' pitch by semitones, cents and octaves, within two octaves;
    /// video events have none.
    #[test]
    fn pitch_keys_shift_audio_events() {
        let mut p = Project::new("t");
        let (a, v) = (clip(0.0, 4.0, 0.0), clip(0.0, 4.0, 0.0));
        let ids = [a.id.clone(), v.id.clone()];
        p.timeline.audio_tracks[0].clips.push(a);
        p.timeline.video_tracks[0].clips.push(v);
        let pitch = |p: &Project| (p.clip(&ids[0]).unwrap().pitch_cents, p.clip(&ids[1]).unwrap().pitch_cents);
        assert!(shift_pitch(&mut p, &ids, 100.0) && shift_pitch(&mut p, &ids, -1.0));
        assert_eq!(pitch(&p), (99.0, 0.0));
        for _ in 0..3 {
            shift_pitch(&mut p, &ids, 1200.0);
        }
        assert_eq!(pitch(&p).0, 2400.0);
        assert!(!shift_pitch(&mut p, &ids, 1.0), "at the limit nothing changes");
    }

    #[test]
    fn trims_keep_fades_inside_the_clip() {
        let mut o = clip(10.0, 6.0, 0.0);
        (o.fade_in, o.fade_out) = (2.0, 3.0);
        let mut c = o.clone();
        trim_right(&mut c, &o, 13.0, F, None);
        assert_eq!((c.fade_in, c.fade_out), (2.0, 1.0));
        trim_left(&mut c, &o, 15.0, F, None);
        assert_eq!((c.duration, c.fade_in, c.fade_out), (1.0, 1.0, 0.0));
        trim_right(&mut c, &o, 20.0, F, None); // lengthening restores the original fades
        assert_eq!((c.fade_in, c.fade_out), (2.0, 3.0));
    }

    #[test]
    fn locked_track_blocks_edits() {
        let mut p = Project::new("t");
        p.timeline.video_tracks[0].clips.push(clip(0.0, 4.0, 0.0));
        p.timeline.video_tracks[0].locked = true;
        let sel = ClipRef { is_video: true, track_idx: 0, clip_idx: 0 };
        let id = vec![p.timeline.video_tracks[0].clips[0].id.clone()];
        assert!(!split_ids(&mut p, &id, 2.0));
        assert!(!delete_clips(&mut p, &id));
        assert!(!move_clips(&mut p, &id, 1.0, 0, false));
        p.timeline.video_tracks[0].locked = false;
        assert!(split_ids(&mut p, &id, 2.0));
        assert_eq!(p.timeline.video_tracks[0].clips.len(), 2);
        // A locked event blocks edits on an unlocked track.
        assert!(toggle_clip_lock(&mut p, &sel));
        assert!(!delete_clips(&mut p, &id) && !trim_to(&mut p, &sel, Some(1.0), None));
        assert_eq!(p.timeline.video_tracks[0].clips.len(), 2);
    }

    /// Two video tracks and one audio track, each with one event; returns the event ids.
    fn three_events() -> (Project, Vec<String>) {
        let mut p = Project::new("t");
        p.timeline.add_video_track();
        p.timeline.video_tracks[0].clips.push(clip(1.0, 2.0, 0.0));
        p.timeline.video_tracks[1].clips.push(clip(4.0, 2.0, 0.0));
        p.timeline.audio_tracks[0].clips.push(clip(2.0, 2.0, 0.0));
        let ids = [&p.timeline.video_tracks[0], &p.timeline.video_tracks[1], &p.timeline.audio_tracks[0]].map(|t| t.clips[0].id.clone());
        (p, ids.to_vec())
    }

    #[test]
    fn delete_clips_skips_locked_events_and_tracks() {
        let (mut p, ids) = three_events();
        p.timeline.video_tracks[0].clips[0].locked = true;
        p.timeline.audio_tracks[0].locked = true;
        assert!(delete_clips(&mut p, &ids));
        let tl = &p.timeline;
        assert_eq!((tl.video_tracks[0].clips.len(), tl.video_tracks[1].clips.len(), tl.audio_tracks[0].clips.len()), (1, 0, 1));
        assert!(!delete_clips(&mut p, &ids));
    }

    #[test]
    fn split_ids_without_ids_splits_only_events_under_the_cursor() {
        let (mut p, ids) = three_events();
        assert!(split_ids(&mut p, &[], 1.5)); // only V1's event spans 1.5
        let tl = &p.timeline;
        assert_eq!((tl.video_tracks[0].clips.len(), tl.video_tracks[1].clips.len(), tl.audio_tracks[0].clips.len()), (2, 1, 1));
        assert_eq!(tl.video_tracks[0].clips[0].id, ids[0]);
        assert!(split_ids(&mut p, &ids[2..], 3.0)); // the audio event only
        assert_eq!(p.timeline.audio_tracks[0].clips.len(), 2);
        assert!(!split_ids(&mut p, &ids[1..2], 3.0)); // V2's event is not under 3.0
    }

    #[test]
    fn move_clips_clamps_hops_and_sorts() {
        let (mut p, ids) = three_events();
        // Clamped so the earliest (at 1.0) lands on 0; the others keep their spacing.
        assert!(move_clips(&mut p, &ids, -5.0, 0, false));
        assert_eq!(p.clip(&ids[0]).unwrap().position, 0.0);
        assert_eq!(p.clip(&ids[1]).unwrap().position, 3.0);
        // A hop of +1: V1 → V2 is fine, but V2 → V3 and A1 → A2 are missing, so nothing moves.
        assert!(!move_clips(&mut p, &ids, 0.0, 1, false));
        assert!(move_clips(&mut p, &ids[..1], 0.5, 1, false));
        assert_eq!(p.find_clip(&ids[0]).map(|r| r.track_idx), Some(1));
        // Sorted by position on the target track: the moved event (0.5) comes before V2's (3.0).
        assert_eq!(p.timeline.video_tracks[1].clips[0].id, ids[0]);
        // A hop onto a locked track is refused for all events.
        p.timeline.video_tracks[0].locked = true;
        assert!(!move_clips(&mut p, &ids[..2], 1.0, -1, false));
        assert_eq!(p.timeline.video_tracks[1].clips.len(), 2);
    }

    #[test]
    fn paste_across_two_tracks_keeps_offsets_and_gives_fresh_ids() {
        let (mut p, ids) = three_events();
        p.timeline.video_tracks[0].clips[0].group = Some("g".into());
        p.timeline.video_tracks[1].clips[0].group = Some("g".into());
        let clips = copy_clips(&p, &ids[..2]);
        assert_eq!(clips.iter().map(|(v, o, c)| (*v, *o, c.position)).collect::<Vec<_>>(), [(true, 0, 0.0), (true, 1, 3.0)]);
        // Focus on V2: the copy lands on V2 and a new V3, from the cursor.
        let new = paste_clips(&mut p, &clips, 10.0, Some((true, 1)));
        assert_eq!(p.timeline.video_tracks.len(), 3);
        let (a, b) = (p.find_clip(&new[0]).unwrap(), p.find_clip(&new[1]).unwrap());
        assert_eq!((a.track_idx, b.track_idx), (1, 2));
        assert_eq!((p.clip(&new[0]).unwrap().position, p.clip(&new[1]).unwrap().position), (10.0, 13.0));
        assert!(new.iter().all(|id| !ids.contains(id)));
        let g = p.clip(&new[0]).unwrap().group.clone();
        assert!(g.is_some() && g != Some("g".into()) && g == p.clip(&new[1]).unwrap().group);
        // A locked target refuses the whole paste.
        p.timeline.video_tracks[2].locked = true;
        assert!(paste_clips(&mut p, &clips, 20.0, Some((true, 1))).is_empty());
    }

    #[test]
    fn track_insert_delete_and_move() {
        let mut p = Project::new("t");
        for _ in 0..3 {
            p.timeline.add_video_track();
        }
        let id = |p: &Project, i: usize| p.timeline.video_tracks[i].id.clone();
        let order = |p: &Project| p.timeline.video_tracks.iter().map(|t| t.id.clone()).collect::<Vec<_>>();
        let t: Vec<String> = order(&p);
        assert!(move_tracks(&mut p, true, &[t[1].clone(), t[3].clone()], 0));
        assert_eq!(order(&p), [&t[1], &t[3], &t[0], &t[2]].map(Clone::clone));
        assert!(move_tracks(&mut p, true, &[t[1].clone()], 99)); // past the end: last
        assert_eq!(order(&p), [&t[3], &t[0], &t[2], &t[1]].map(Clone::clone));
        assert!(!move_tracks(&mut p, true, &[t[1].clone()], 4));

        assert_eq!(insert_track(&mut p, true, Some(1)), 1); // above the focus track
        assert_eq!(p.timeline.video_tracks.len(), 5);
        assert_eq!((id(&p, 0), id(&p, 2)), (t[3].clone(), t[0].clone()));
        assert_eq!(insert_track(&mut p, true, None), 0); // no focus: video first
        assert_eq!(id(&p, 1), t[3]);
        assert_eq!(insert_track(&mut p, false, None), 1); // audio last

        p.timeline.video_tracks[0].clips.push(clip(0.0, 1.0, 0.0));
        let all: Vec<String> = order(&p).into_iter().chain(p.timeline.audio_tracks.iter().map(|t| t.id.clone())).collect();
        assert!(delete_tracks(&mut p, &all[..1]));
        assert!(p.timeline.video_tracks.iter().all(|t| t.clips.is_empty()));
        assert!(delete_tracks(&mut p, &all));
        assert!(p.timeline.video_tracks.is_empty() && p.timeline.audio_tracks.is_empty());
    }

    /// R5: grouping gathers the tracks, group Mute / Solo override the members' own, and the groups follow track
    /// moves, inserts, ungrouping and deletes.
    #[test]
    fn track_groups() {
        let mut p = Project::new("t");
        for _ in 0..4 {
            p.timeline.add_video_track(); // V0..V4
        }
        p.timeline.add_audio_track(); // A0, A1
        let t: Vec<String> = p.timeline.video_tracks.iter().map(|t| t.id.clone()).collect();
        let a: Vec<String> = p.timeline.audio_tracks.iter().map(|t| t.id.clone()).collect();
        let order = |p: &Project| p.timeline.video_tracks.iter().map(|t| t.id.clone()).collect::<Vec<_>>();
        let members = |p: &Project| {
            let mut m = p.timeline.track_groups.first().map_or(Vec::new(), |g| g.tracks.clone());
            m.sort();
            m
        };
        let sorted = |mut v: Vec<String>| {
            v.sort();
            v
        };

        // V1, V3 and A1 group: V3 is gathered under V1; the audio track stays where it is.
        assert!(group_tracks(&mut p, &[t[3].clone(), t[1].clone(), a[1].clone()]));
        assert_eq!(order(&p), [&t[0], &t[1], &t[3], &t[2], &t[4]].map(Clone::clone));
        assert_eq!(members(&p), sorted(vec![t[1].clone(), t[3].clone(), a[1].clone()]));
        assert_eq!(p.timeline.track_groups[0].name, "Track Group 1");

        // Group mute: one change silences all three, over their own Solo; group solo plays them over their own Mute.
        let mut h = crate::history::History::default();
        let before = p.clone();
        p.timeline.video_tracks[1].solo = true;
        assert!(toggle_track_groups(&mut p, &[t[3].clone()], false));
        h.push(before);
        let tl = &p.timeline;
        let plays = |id: &str| [true, false].into_iter().find_map(|v| tl.tracks(v).iter().find(|x| x.id == id).map(|x| tl.plays(x, tl.any_solo(v)))).unwrap();
        assert!(!plays(&t[1]) && !plays(&t[3]) && !plays(&a[1]) && plays(&t[0]) && plays(&a[0]));
        assert!(!tl.any_solo(true), "a muted group's own soloed track solos nothing");
        let mut undone = p.clone();
        assert!(h.undo(&mut undone) && !h.can_undo());
        assert!(!undone.timeline.track_groups[0].muted, "one undo step");
        assert!(toggle_track_groups(&mut p, &[t[1].clone()], false), "toggles back");
        p.timeline.video_tracks[2].muted = true; // V3 (gathered to index 2)
        assert!(toggle_track_groups(&mut p, &[a[1].clone()], true));
        let tl = &p.timeline;
        let plays = |id: &str| [true, false].into_iter().find_map(|v| tl.tracks(v).iter().find(|x| x.id == id).map(|x| tl.plays(x, tl.any_solo(v)))).unwrap();
        assert!(plays(&t[3]) && plays(&a[1]) && !plays(&t[0]) && !plays(&a[0]));
        assert!(!toggle_track_groups(&mut p, &[t[0].clone()], true), "V0 is in no group");

        // A track inserted inside the group joins it; one moved into it joins; one moved away leaves.
        let i = insert_track(&mut p, true, Some(2)); // between V1 and V3
        let new = p.timeline.video_tracks[i].id.clone();
        assert!(p.timeline.group_of(&new).is_some());
        assert!(move_tracks(&mut p, true, &[t[4].clone()], 2)); // V4 between V1 and the new track
        assert!(p.timeline.group_of(&t[4]).is_some());
        assert!(move_tracks(&mut p, true, &[t[4].clone()], 99)); // to the end: out
        assert!(p.timeline.group_of(&t[4]).is_none());
        assert!(move_tracks(&mut p, true, &[t[3].clone()], 1)); // just above the group: still in it
        assert!(p.timeline.group_of(&t[3]).is_some());

        // Ungroup the middle track: it moves below the group, which stays in one piece.
        let mid = order(&p)[2].clone();
        assert!(ungroup_tracks(&mut p, std::slice::from_ref(&mid)));
        let o = order(&p);
        let k = o.iter().position(|x| *x == mid).unwrap();
        assert!(o[..k].iter().filter(|x| p.timeline.group_of(x).is_some()).count() == 2 && p.timeline.group_of(&mid).is_none());
        assert!(o[k + 1..].iter().all(|x| p.timeline.group_of(x).is_none()));
        assert!(!ungroup_tracks(&mut p, std::slice::from_ref(&mid)));

        // Deleting tracks takes them out of their group; an emptied group is dropped.
        let left = p.timeline.track_groups[0].tracks.clone();
        assert!(delete_tracks(&mut p, &left[..1]));
        assert_eq!(p.timeline.track_groups[0].tracks.len(), left.len() - 1);
        assert!(delete_tracks(&mut p, &left[1..]));
        assert!(p.timeline.track_groups.is_empty());

        // A second group gets the next free name, and grouping takes tracks out of the group they were in.
        let v: Vec<String> = order(&p);
        assert!(group_tracks(&mut p, &v[..2]) && group_tracks(&mut p, &v[1..2]));
        assert_eq!(p.timeline.track_groups.iter().map(|g| (g.name.as_str(), g.tracks.len())).collect::<Vec<_>>(), [("Track Group 1", 1), ("Track Group 2", 1)]);
        assert!(!group_tracks(&mut p, &[]));
    }

    /// Pool: 0 video with audio (10 s), 1 still, 2 audio (4 s).
    fn media_project() -> Project {
        let mut p = Project::new("t");
        let m = |id: &str, media_type, duration, sample_rate| MediaItem { id: id.into(), path: id.into(), name: id.into(), media_type, duration, sample_rate, ..Default::default() };
        p.media_pool = vec![m("v", MediaType::Video, 10.0, Some(48_000)), m("i", MediaType::Image, 0.04, None), m("a", MediaType::Audio, 4.0, Some(48_000))];
        p
    }

    /// (track index, position, duration, media id) of each event on the tracks of one kind.
    fn laid_out(p: &Project, video: bool) -> Vec<(usize, f64, f64, String)> {
        let tracks = p.timeline.tracks(video).iter().enumerate();
        tracks.flat_map(|(i, t)| t.clips.iter().map(move |c| (i, c.position, c.duration, c.media_id.clone().unwrap_or_default()))).collect()
    }

    #[test]
    fn drop_creates_a_video_track_when_there_is_none() {
        let mut p = media_project();
        p.timeline.video_tracks.clear();
        assert_eq!(drop_media(&mut p, &[1], None, Some((true, 0)), 1.0, 5.0, false).len(), 1);
        assert_eq!(laid_out(&p, true), [(0, 1.0, 5.0, "i".into())]);
        assert!(p.modified);
    }

    #[test]
    fn wrong_kind_rows_redirect_to_the_first_track_of_the_right_kind() {
        let mut p = media_project();
        p.timeline.add_video_track();
        p.timeline.add_audio_track();
        // Audio on Video 2 goes to Audio 1; a still on Audio 2 goes to Video 1.
        drop_media(&mut p, &[2], None, Some((true, 1)), 0.0, 5.0, false);
        drop_media(&mut p, &[1], None, Some((false, 1)), 0.0, 5.0, false);
        assert_eq!(laid_out(&p, false), [(0, 0.0, 4.0, "a".into())]);
        assert_eq!(laid_out(&p, true), [(0, 0.0, 5.0, "i".into())]);
        // No audio track at all: one is created.
        p.timeline.audio_tracks.clear();
        drop_media(&mut p, &[2], None, Some((true, 0)), 9.0, 5.0, false);
        assert_eq!(laid_out(&p, false), [(0, 9.0, 4.0, "a".into())]);
    }

    #[test]
    fn drop_below_the_last_track_creates_tracks() {
        let mut p = media_project();
        let ids = drop_media(&mut p, &[0], None, None, 2.0, 5.0, false);
        let tl = &p.timeline;
        assert_eq!((tl.video_tracks.len(), tl.audio_tracks.len()), (2, 2));
        assert_eq!(laid_out(&p, true), [(1, 2.0, 10.0, "v".into())]);
        assert_eq!(laid_out(&p, false), [(1, 2.0, 10.0, "v".into())]);
        // The video and its audio are one group, and both are returned (they become the selection).
        let (v, a) = (p.clip(&ids[0]).unwrap(), p.clip(&ids[1]).unwrap());
        assert!(v.group.is_some() && v.group == a.group && ids.len() == 2);
        // An empty timeline gets a track of each kind it needs.
        let mut p = media_project();
        p.timeline.video_tracks.clear();
        p.timeline.audio_tracks.clear();
        drop_media(&mut p, &[2], None, None, 0.0, 5.0, false);
        assert_eq!((p.timeline.video_tracks.len(), p.timeline.audio_tracks.len()), (0, 1));
    }

    #[test]
    fn paired_audio_takes_a_new_track_when_the_first_is_occupied() {
        let mut p = media_project();
        p.timeline.audio_tracks[0].clips.push(clip(8.0, 4.0, 0.0));
        drop_media(&mut p, &[0], None, Some((true, 0)), 0.0, 5.0, false); // 0..10 overlaps 8..12
        assert_eq!(laid_out(&p, false), [(0, 8.0, 4.0, String::new()), (1, 0.0, 10.0, "v".into())]);
        // Free over the event's range: the first audio track.
        drop_media(&mut p, &[0], Some((1.0, 3.0)), Some((true, 0)), 20.0, 5.0, false);
        assert_eq!(laid_out(&p, false)[1], (0, 20.0, 2.0, "v".into()));
        // A locked first audio track is not free either.
        p.timeline.audio_tracks[0].locked = true;
        drop_media(&mut p, &[0], None, Some((true, 0)), 40.0, 5.0, false);
        assert_eq!(p.timeline.audio_tracks.len(), 3);
    }

    #[test]
    fn several_items_go_across_time_and_stills_use_the_still_length() {
        let mut p = media_project();
        let ids = drop_media(&mut p, &[0, 1, 2], None, Some((true, 0)), 1.0, 3.0, false);
        assert_eq!(ids.len(), 4);
        assert_eq!(laid_out(&p, true), [(0, 1.0, 10.0, "v".into()), (0, 11.0, 3.0, "i".into())]);
        assert_eq!(laid_out(&p, false), [(0, 1.0, 10.0, "v".into()), (0, 14.0, 4.0, "a".into())]);
        let i = p.clip(&ids[2]).unwrap();
        assert_eq!((i.in_point, i.out_point), (0.0, 3.0));
    }

    #[test]
    fn a_locked_track_refuses_the_drop() {
        let mut p = media_project();
        p.timeline.video_tracks[0].locked = true;
        let plan = plan_drop(&p.timeline, &[(&p.media_pool[0], None)], Some((true, 0)), 0.0, 5.0);
        assert!(plan.refused && plan.events.len() == 2); // still laid out, for the red ghost
        assert!(drop_media(&mut p, &[0], None, Some((true, 0)), 0.0, 5.0, false).is_empty());
        // Pointing at a locked row refuses even when the media would go elsewhere.
        assert!(drop_media(&mut p, &[2], None, Some((true, 0)), 0.0, 5.0, false).is_empty());
        assert!(laid_out(&p, true).is_empty() && laid_out(&p, false).is_empty() && !p.modified);
        // Below the last track, the new tracks take it.
        assert_eq!(drop_media(&mut p, &[1], None, None, 0.0, 5.0, false).len(), 1);
    }

    #[test]
    fn quantize_lands_on_frames() {
        let q = quantize(1.0, 29.97);
        assert!((q - 30.0 * 1001.0 / 30000.0).abs() < 1e-12); // frame 30 of the exact 30000/1001 grid
        assert_eq!(quantize(q, 29.97), q);
        assert_eq!(quantize(2.0, 25.0), 2.0);
        assert!((quantize(2.03, 25.0) - 2.04).abs() < 1e-12);
    }

    #[test]
    fn mute_solo_follow_the_focus_track() {
        let (mut p, _) = three_events();
        let ids: Vec<String> = p.timeline.video_tracks.iter().map(|t| t.id.clone()).collect();
        p.timeline.video_tracks[0].muted = true;
        assert!(toggle_tracks(&mut p, &ids, false)); // focus V2 was unmuted: both mute
        assert!(p.timeline.video_tracks.iter().all(|t| t.muted));
        assert!(toggle_tracks(&mut p, &ids[..1], true));
        assert!(p.timeline.video_tracks[0].solo && !p.timeline.video_tracks[1].solo);
        assert!(!toggle_tracks(&mut p, &[], true));
    }

    #[test]
    fn toggle_minimize_twice_restores_the_heights() {
        let mut p = Project::new("t");
        p.timeline.video_tracks[0].height = 90.0;
        let mut saved = None;
        toggle_minimize(&mut p, &mut saved, 22.0);
        assert_eq!((p.timeline.video_tracks[0].height, p.timeline.audio_tracks[0].height), (22.0, 22.0));
        toggle_minimize(&mut p, &mut saved, 22.0);
        assert_eq!((p.timeline.video_tracks[0].height, p.timeline.audio_tracks[0].height), (90.0, 0.0));
        assert!(saved.is_none() && p.modified);
    }

    #[test]
    fn trim_to_cursor_and_selection() {
        let mut p = Project::new("t");
        p.timeline.video_tracks[0].clips.push(clip(10.0, 6.0, 1.0));
        let sel = ClipRef { is_video: true, track_idx: 0, clip_idx: 0 };
        assert!(!trim_to(&mut p, &sel, Some(20.0), None)); // outside the event: nothing
        assert!(trim_to(&mut p, &sel, None, Some(15.0)));
        assert!(trim_to(&mut p, &sel, Some(11.0), None));
        let c = &p.timeline.video_tracks[0].clips[0];
        assert!((c.position - 11.0).abs() < 1e-9 && (c.duration - 4.0).abs() < 1e-9 && (c.in_point - 2.0).abs() < 1e-9);
        assert!(trim_to(&mut p, &sel, Some(12.0), Some(14.0)));
        let c = &p.timeline.video_tracks[0].clips[0];
        assert!((c.position - 12.0).abs() < 1e-9 && (c.duration - 2.0).abs() < 1e-9);
    }

    #[test]
    fn drop_with_source_range() {
        let mut p = Project::new("t");
        let mut m = MediaItem {
            id: "m".into(), path: "m.mp4".into(), name: "m".into(), media_type: MediaType::Video, duration: 10.0,
            sample_rate: Some(48_000), ..Default::default()
        };
        p.media_pool.push(m.clone());
        assert_eq!(drop_media(&mut p, &[0], Some((2.0, 3.5)), Some((true, 0)), 5.0, 5.0, false).len(), 2);
        let v = &p.timeline.video_tracks[0].clips[0];
        assert_eq!((v.position, v.in_point, v.out_point, v.duration), (5.0, 2.0, 3.5, 1.5));
        assert_eq!(p.timeline.audio_tracks[0].clips[0].in_point, 2.0); // linked audio gets the same range
        m.media_type = MediaType::Unknown;
        p.media_pool[0] = m;
        assert!(drop_media(&mut p, &[0], None, Some((true, 0)), 0.0, 5.0, false).is_empty()); // unreadable media adds nothing
    }

    /// The video with audio (10 s) dropped at 0: (project, video id, audio id).
    fn av_pair() -> (Project, String, String) {
        let mut p = media_project();
        let ids = drop_media(&mut p, &[0], None, Some((true, 0)), 0.0, 5.0, false);
        (p, ids[0].clone(), ids[1].clone())
    }

    fn group_of(p: &Project, id: &str) -> Option<String> {
        p.clip(id).unwrap().group.clone()
    }

    #[test]
    fn groups_expand_split_and_dissolve() {
        let (mut p, v, a) = av_pair();
        assert!(group_of(&p, &v).is_some() && group_of(&p, &v) == group_of(&p, &a));
        // The mates come first, so the focus stays last.
        assert_eq!(expand_groups(&p, &[a.clone()]), [v.clone(), a.clone()]);
        // A split gives two groups: the left halves and the right halves.
        assert!(split_ids(&mut p, &[v.clone(), a.clone()], 4.0));
        let right: Vec<String> = [true, false].map(|vid| p.timeline.tracks(vid)[0].clips[1].id.clone()).to_vec();
        assert!(group_of(&p, &right[0]).is_some() && group_of(&p, &right[0]) == group_of(&p, &right[1]));
        assert_ne!(group_of(&p, &right[0]), group_of(&p, &v));
        // Splitting only one event of a pair leaves its right half in no group.
        assert!(split_ids(&mut p, &[v.clone()], 2.0));
        assert_eq!(group_of(&p, &p.timeline.video_tracks[0].clips[1].id), None);
        // Ungrouping one of a pair leaves no one-member group.
        assert!(ungroup(&mut p, &[v.clone()]));
        assert!(group_of(&p, &v).is_none() && group_of(&p, &a).is_none());
        assert!(group(&mut p, &[v.clone(), a.clone()]) && group_of(&p, &v).is_some());
        assert!(!group(&mut p, &[v.clone()]), "one event is no group");
        assert!(clear_groups(&mut p, &[a.clone()]) && group_of(&p, &v).is_none());
        // Group › Delete All (R3a) deletes the expanded selection: the grouped audio goes with the video.
        let (mut p, v, _) = av_pair();
        let all = expand_groups(&p, &[v]);
        assert!(delete_clips(&mut p, &all));
        assert!(p.timeline.video_tracks[0].clips.is_empty() && p.timeline.audio_tracks[0].clips.is_empty());
    }

    #[test]
    fn edge_mates_honour_the_tolerance() {
        let (mut p, v, a) = av_pair();
        assert_eq!(edge_mates(&p, &v, false, F), [a.clone()]);
        p.timeline.audio_tracks[0].clips[0].duration = 10.0 + F * 0.5;
        assert_eq!(edge_mates(&p, &v, false, F), [a.clone()]);
        p.timeline.audio_tracks[0].clips[0].duration = 10.0 + F * 2.0;
        assert!(edge_mates(&p, &v, false, F).is_empty());
        assert_eq!(edge_mates(&p, &v, true, F), [a]); // left edges still line up
    }

    /// One track: events "a" 0..5 and "b" 6..10 (ids are their names).
    fn two_on_a_track() -> Project {
        let mut p = Project::new("t");
        for (id, pos, dur) in [("a", 0.0, 5.0), ("b", 6.0, 4.0)] {
            let mut c = clip(pos, dur, 0.0);
            c.id = id.into();
            p.timeline.video_tracks[0].clips.push(c);
        }
        p
    }

    /// Locked events and an overlap the edit did not change keep their fades.
    #[test]
    fn auto_crossfade_leaves_locked_and_unchanged_pairs() {
        let mut p = two_on_a_track();
        p.timeline.video_tracks[0].clips[1].position = 4.0; // 1 s overlap, no fades
        p.timeline.video_tracks[0].locked = true;
        let before = p.clone();
        assert!(!auto_crossfade(&mut p, &before, &["a".into(), "b".into()]));
        assert_eq!(fades(&p), [(0.0, 0.0), (0.0, 0.0)]);
        // Fade types refuse locked tracks too.
        assert!(!set_fade_curve(&mut p, "a", true, Curve::Fast) && !set_crossfade_curve(&mut p, "a", "b", Curve::Fast, Curve::Slow));
        // A hand-set 0.5 s crossfade fade, both events moved together.
        p.timeline.video_tracks[0].locked = false;
        (p.timeline.video_tracks[0].clips[0].fade_out, p.timeline.video_tracks[0].clips[1].fade_in) = (0.5, 0.5);
        let (before, ids) = (p.clone(), vec!["a".to_string(), "b".to_string()]);
        move_clips(&mut p, &ids, 2.0, 0, false);
        auto_crossfade(&mut p, &before, &ids);
        assert_eq!(fades(&p), [(0.0, 0.5), (0.5, 0.0)]);
    }

    fn xfade_move(p: &mut Project, id: &str, dt: f64) {
        let (before, ids) = (p.clone(), vec![id.to_string()]);
        move_clips(p, &ids, dt, 0, false);
        auto_crossfade(p, &before, &ids);
    }

    fn fades(p: &Project) -> Vec<(f64, f64)> {
        p.timeline.video_tracks[0].clips.iter().map(|c| (c.fade_in, c.fade_out)).collect()
    }

    #[test]
    fn auto_crossfade_follows_the_overlap() {
        let mut p = two_on_a_track();
        xfade_move(&mut p, "b", -2.0); // b 4..8 over a 0..5
        assert_eq!(fades(&p), [(0.0, 1.0), (1.0, 0.0)]);
        xfade_move(&mut p, "b", -1.0);
        assert_eq!(fades(&p), [(0.0, 2.0), (2.0, 0.0)]);
        xfade_move(&mut p, "b", 5.0); // pulled apart
        assert_eq!(fades(&p), [(0.0, 0.0), (0.0, 0.0)]);
        // A fade set by hand on a pair that does not overlap is kept.
        p.timeline.video_tracks[0].clips[0].fade_out = 0.5;
        xfade_move(&mut p, "b", 1.0);
        assert_eq!(fades(&p)[0], (0.0, 0.5));
        // A fully covered event gets no crossfade.
        let mut p = two_on_a_track();
        p.timeline.video_tracks[0].clips[1].duration = 1.0;
        xfade_move(&mut p, "b", -5.0); // b 1..2 inside a
        assert_eq!(fades(&p), [(0.0, 0.0), (0.0, 0.0)]);
    }

    #[test]
    fn auto_crossfade_changes_only_pairs_with_the_edited_event() {
        let mut p = two_on_a_track();
        p.timeline.video_tracks[0].clips[1].position = 4.0; // a and b overlap, without fades
        let mut c = clip(12.0, 2.0, 0.0);
        c.id = "c".into();
        p.timeline.video_tracks[0].clips.push(c);
        xfade_move(&mut p, "c", -4.5); // c 7.5..9.5 over b 4..8
        assert_eq!(fades(&p), [(0.0, 0.0), (0.0, 0.5), (0.5, 0.0)]);
        // Trimming a's right edge into b makes a crossfade too.
        let mut p = two_on_a_track();
        let before = p.clone();
        let o = p.timeline.video_tracks[0].clips[0].clone();
        trim_right(&mut p.timeline.video_tracks[0].clips[0], &o, 7.0, F, None);
        assert!(auto_crossfade(&mut p, &before, &["a".into()]));
        assert_eq!(fades(&p), [(0.0, 1.0), (1.0, 0.0)]);
        assert!(set_crossfade_curve(&mut p, "a", "b", Curve::Fast, Curve::Slow));
        let tr = &p.timeline.video_tracks[0];
        assert_eq!((tr.clips[0].fade_out_curve, tr.clips[1].fade_in_curve, tr.clips[0].fade_in_curve), (Curve::Fast, Curve::Slow, Curve::Smooth));
    }

    /// V1: 0..2, 3..5, 8..9; V2: 4..6; A1: 5..6. Markers at 1 and 4, a region 2..6. Returns the track ids.
    fn ripple_project() -> (Project, Vec<String>) {
        let mut p = Project::new("t");
        p.timeline.add_video_track();
        for (pos, dur) in [(0.0, 2.0), (3.0, 2.0), (8.0, 1.0)] {
            p.timeline.video_tracks[0].clips.push(clip(pos, dur, 0.0));
        }
        p.timeline.video_tracks[1].clips.push(clip(4.0, 2.0, 0.0));
        p.timeline.audio_tracks[0].clips.push(clip(5.0, 1.0, 0.0));
        for pos in [1.0, 4.0] {
            p.timeline.markers.push(Marker { position: pos, name: String::new(), color: [0; 3] });
        }
        p.timeline.regions.push(Region { start: 2.0, end: 6.0, name: String::new() });
        let ids = [&p.timeline.video_tracks[0], &p.timeline.video_tracks[1], &p.timeline.audio_tracks[0]].map(|t| t.id.clone()).to_vec();
        (p, ids)
    }

    fn starts(t: &Track) -> Vec<f64> {
        t.clips.iter().map(|c| c.position).collect()
    }

    #[test]
    fn ripple_modes() {
        let (mut p, tr) = ripple_project();
        let op = RippleOp { from: 3.0, delta: 2.0, tracks: vec![tr[0].clone()], ..Default::default() };
        assert!(ripple(&mut p, &op, RippleMode::Affected));
        let tl = &p.timeline;
        assert_eq!((starts(&tl.video_tracks[0]), starts(&tl.video_tracks[1]), starts(&tl.audio_tracks[0])), (vec![0.0, 5.0, 10.0], vec![4.0], vec![5.0]));
        assert_eq!((tl.markers[1].position, tl.regions[0].end), (4.0, 6.0));

        let (mut p, _) = ripple_project();
        assert!(ripple(&mut p, &op, RippleMode::AffectedMarkers));
        let tl = &p.timeline;
        assert_eq!((tl.markers[0].position, tl.markers[1].position), (1.0, 6.0));
        assert_eq!((tl.regions[0].start, tl.regions[0].end), (2.0, 8.0)); // straddles 3: stretches
        assert_eq!(starts(&tl.video_tracks[1]), [4.0]);

        let (mut p, _) = ripple_project();
        assert!(ripple(&mut p, &op, RippleMode::All));
        let tl = &p.timeline;
        assert_eq!((starts(&tl.video_tracks[1]), starts(&tl.audio_tracks[0])), (vec![6.0], vec![7.0]));

        // R8b: bus tracks. A Bus A Volume envelope point and a Video Bus Track point at 4 s move with the bus-track
        // modes; Affected Tracks, Markers, and Regions moves the markers but leaves them.
        let bus_project = || {
            let (mut p, tr) = ripple_project();
            let op = RippleOp { from: 3.0, delta: 2.0, tracks: vec![tr[0].clone()], ..Default::default() };
            set_bus_count(&mut p, 1);
            let id = p.timeline.audio.buses[0].id.clone();
            toggle_bus_envelope(&mut p, Some(&id), EnvKind::Volume);
            toggle_bus_envelope(&mut p, None, EnvKind::FadeToColor);
            let tl = &mut p.timeline;
            for e in tl.audio.buses[0].envelopes.iter_mut().chain(&mut tl.master.video_envelopes) {
                env_insert(e, 4.0, 0.0);
            }
            (p, op)
        };
        let bus_keys = |p: &Project| (p.timeline.audio.buses[0].envelopes[0].points.times(), p.timeline.master.video_envelopes[0].points.times());
        for (mode, at) in [(RippleMode::AffectedMarkers, 6.0), (RippleMode::All, 6.0), (RippleMode::AffectedMarkersOnly, 4.0), (RippleMode::Affected, 4.0)] {
            let (mut p, op) = bus_project();
            assert!(ripple(&mut p, &op, mode), "{mode:?}");
            assert_eq!(bus_keys(&p), (vec![0.0, at], vec![0.0, at]), "{mode:?}");
            assert_eq!(p.timeline.markers[1].position, if mode == RippleMode::Affected { 4.0 } else { 6.0 }, "{mode:?}");
        }

        // A negative delta stops where the first moved event reaches 0; excluded events stay.
        let (mut p, tr) = ripple_project();
        let id = p.timeline.video_tracks[0].clips[2].id.clone();
        ripple(&mut p, &RippleOp { from: 3.0, delta: -10.0, tracks: vec![tr[0].clone()], exclude: vec![id], ..Default::default() }, RippleMode::Affected);
        assert_eq!(starts(&p.timeline.video_tracks[0]), [0.0, 0.0, 8.0]);
    }

    #[test]
    fn ripple_closes_envelope_gaps() {
        let (mut p, tr) = ripple_project();
        let keys = [1.0, 2.0, 2.5, 3.0, 4.0].map(|t| Keyframe { t, v: 0.0_f32, curve: Curve::Linear }).to_vec();
        p.timeline.video_tracks[0].envelopes.push(Envelope { kind: EnvKind::Volume, points: Keyframes { keys } });
        ripple(&mut p, &RippleOp { from: 3.0, delta: -1.0, tracks: vec![tr[0].clone()], ..Default::default() }, RippleMode::Affected);
        let t: Vec<f64> = p.timeline.video_tracks[0].envelopes[0].points.keys.iter().map(|k| k.t).collect();
        assert_eq!(t, [1.0, 2.0, 3.0]);
    }

    #[test]
    fn delete_range_splits_and_closes_up() {
        let mut p = Project::new("t");
        p.timeline.video_tracks[0].clips.push(clip(0.0, 10.0, 0.0));
        let tr = vec![p.timeline.video_tracks[0].id.clone()];
        assert!(delete_range(&mut p, 2.0, 5.0, &tr, RippleMode::Affected));
        let c: Vec<(f64, f64, f64)> = p.timeline.video_tracks[0].clips.iter().map(|c| (c.position, c.duration, c.in_point)).collect();
        assert_eq!(c, [(0.0, 2.0, 0.0), (2.0, 5.0, 5.0)]);
        assert!(!delete_range(&mut Project::new("t"), 1.0, 2.0, &[], RippleMode::All), "nothing there: no edit");
        // Deleting events closes their gaps, a video and its audio in one op.
        let (mut p, v, a) = av_pair();
        p.timeline.video_tracks[0].clips.push(clip(12.0, 1.0, 0.0));
        let ops = gaps(&p, &[v.clone(), a.clone()]);
        assert_eq!((ops.len(), ops[0].from, ops[0].delta, ops[0].tracks.len()), (1, 0.0, -10.0, 2));
        delete_clips(&mut p, &[v, a]);
        ripple_all(&mut p, &ops, RippleMode::Affected);
        assert_eq!(starts(&p.timeline.video_tracks[0]), [2.0]);
    }

    /// One track: "a" 0..5 and "b" 4..9 crossfaded by 1 s, then "c" 9..12.
    fn crossfaded() -> (Project, Vec<String>) {
        let mut p = Project::new("t");
        for (id, pos, dur) in [("a", 0.0, 5.0), ("b", 4.0, 5.0), ("c", 9.0, 3.0)] {
            let mut c = clip(pos, dur, 0.0);
            c.id = id.into();
            p.timeline.video_tracks[0].clips.push(c);
        }
        let tl = &mut p.timeline.video_tracks[0].clips;
        (tl[0].fade_out, tl[1].fade_in) = (1.0, 1.0);
        let tr = vec![p.timeline.video_tracks[0].id.clone()];
        (p, tr)
    }

    fn delete_rippled(p: &mut Project, id: &str) {
        let ids = vec![id.to_string()];
        let ops = gaps(p, &ids);
        delete_clips(p, &ids);
        ripple_all(p, &ops, RippleMode::Affected);
    }

    /// A ripple delete closes only the empty time: the other half of a crossfade stays, and what follows lands
    /// against it.
    #[test]
    fn ripple_delete_keeps_crossfades_together() {
        let (mut p, _) = crossfaded();
        delete_rippled(&mut p, "a");
        assert_eq!(starts(&p.timeline.video_tracks[0]), [0.0, 5.0]); // b 0..5, c 5..8
        let (mut p, _) = crossfaded();
        delete_rippled(&mut p, "b");
        assert_eq!(starts(&p.timeline.video_tracks[0]), [0.0, 5.0]); // c against a's end
    }

    /// Ripple trims: a right edge takes its crossfaded neighbour along; an extended left edge keeps the event's start
    /// and leaves alone what starts before it.
    #[test]
    fn ripple_trims_move_what_follows() {
        let (mut p, tr) = crossfaded();
        let o = p.timeline.video_tracks[0].clips[0].clone();
        trim_right(&mut p.timeline.video_tracks[0].clips[0], &o, 3.0, F, None);
        let c = p.timeline.video_tracks[0].clips[0].clone();
        let ops = trim_ripple(&p, &o, &c, &["a".into()], &tr);
        ripple_all(&mut p, &ops, RippleMode::Affected);
        assert_eq!(starts(&p.timeline.video_tracks[0]), [0.0, 2.0, 7.0]); // still a 1 s overlap

        // x 3..4, then b 4..8 with 2 s of head: dragging b's left edge to 2.5 keeps b at 4 and x where it is.
        let mut p = Project::new("t");
        let tr = vec![p.timeline.video_tracks[0].id.clone()];
        let mut x = clip(3.0, 1.0, 0.0);
        x.id = "x".into();
        let mut b = clip(4.0, 4.0, 2.0);
        b.id = "b".into();
        p.timeline.video_tracks[0].clips.extend([x, b.clone()]);
        let before = p.clone();
        trim_left(&mut p.timeline.video_tracks[0].clips[1], &b, 2.5, F, None);
        let c = p.timeline.video_tracks[0].clips[1].clone();
        let ops = trim_ripple(&before, &b, &c, &["b".into()], &tr);
        ripple_all(&mut p, &ops, RippleMode::Affected);
        let t = &p.timeline.video_tracks[0];
        assert_eq!((starts(t), t.clips[1].duration), (vec![3.0, 4.0], 5.5));
    }

    /// A ripple delete or trim deletes only the keys under the closed time and moves only what follows it; markers
    /// before the gap's end stay.
    #[test]
    fn ripple_delete_and_trim_keep_keys_before_the_gap() {
        let keys = |p: &Project| p.timeline.video_tracks[0].envelopes[0].points.keys.iter().map(|k| (k.t, k.v)).collect::<Vec<_>>();
        let mut p = Project::new("t");
        for (id, pos) in [("a", 0.0), ("b", 5.0), ("c", 10.0)] {
            let mut c = clip(pos, 5.0, 0.0);
            c.id = id.into();
            p.timeline.video_tracks[0].clips.push(c);
        }
        let ks = [(0.0, 0.0), (3.0, -10.0), (7.0, -30.0), (12.0, 0.0)].map(|(t, v)| Keyframe { t, v, curve: Curve::Linear }).to_vec();
        p.timeline.video_tracks[0].envelopes.push(Envelope { kind: EnvKind::Volume, points: Keyframes { keys: ks } });
        p.timeline.markers.push(Marker { position: 7.0, name: String::new(), color: [0; 3] });
        let tr = vec![p.timeline.video_tracks[0].id.clone()];

        let mut d = p.clone();
        let ids = vec!["b".to_string()];
        let ops = gaps(&d, &ids);
        delete_clips(&mut d, &ids);
        ripple_all(&mut d, &ops, RippleMode::AffectedMarkers);
        assert_eq!(keys(&d), [(0.0, 0.0), (3.0, -10.0), (7.0, 0.0)]);
        assert_eq!(d.timeline.markers[0].position, 7.0);

        // Right-trim c 10..15 to 13: the key at 12 is on the kept part.
        let mut t = p.clone();
        let o = t.timeline.video_tracks[0].clips[2].clone();
        trim_right(&mut t.timeline.video_tracks[0].clips[2], &o, 13.0, F, None);
        let c = t.timeline.video_tracks[0].clips[2].clone();
        let ops = trim_ripple(&p, &o, &c, &["c".into()], &tr);
        ripple_all(&mut t, &ops, RippleMode::Affected);
        assert_eq!(keys(&t), keys(&p));

        // Left-trim b 5..10 to start at 6: the key at 7 is past the cut second and moves to 6.
        let mut t = p.clone();
        let o = t.timeline.video_tracks[0].clips[1].clone();
        trim_left(&mut t.timeline.video_tracks[0].clips[1], &o, 6.0, F, None);
        let c = t.timeline.video_tracks[0].clips[1].clone();
        let ops = trim_ripple(&p, &o, &c, &["b".into()], &tr);
        ripple_all(&mut t, &ops, RippleMode::Affected);
        assert_eq!(keys(&t), [(0.0, 0.0), (3.0, -10.0), (6.0, -30.0), (11.0, 0.0)]);
    }

    /// After a ripple, crossfades it pulled apart lose their fades.
    #[test]
    fn crossfade_moved_follows_the_ripple() {
        let (mut p, tr) = crossfaded();
        let before = p.clone();
        ripple(&mut p, &RippleOp { from: 4.0, delta: 1.0, tracks: tr, ..Default::default() }, RippleMode::Affected);
        assert!(crossfade_moved(&mut p, &before));
        assert_eq!(fades(&p), [(0.0, 0.0), (0.0, 0.0), (0.0, 0.0)]);
    }

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn src_time_forward_and_reversed_at_both_edges() {
        let mut c = clip(10.0, 4.0, 2.0);
        c.rate = 1.5;
        assert!(near(src_time(&c, 10.0), 2.0) && near(src_time(&c, 14.0), 8.0));
        c.reverse = true;
        assert!(near(src_time(&c, 10.0), 8.0) && near(src_time(&c, 14.0), 2.0));
    }

    #[test]
    fn trims_follow_the_rate_and_direction() {
        // Rate 2: 10 s of media after in_point 2 last 4 s on the timeline.
        let mut o = clip(10.0, 2.0, 2.0);
        o.rate = 2.0;
        let mut c = o.clone();
        trim_right(&mut c, &o, 100.0, F, Some(10.0));
        assert!(near(c.duration, 4.0) && near(c.out_point, 10.0) && c.in_point == 2.0);
        trim_left(&mut c, &o, 0.0, F, Some(10.0)); // 2 s of head at rate 2 is 1 s
        assert!(near(c.position, 9.0) && near(c.in_point, 0.0) && near(c.duration, 3.0));
        // Reversed, the left edge shows the source end: a left trim keeps in_point, a right trim moves it.
        o.reverse = true;
        let mut c = o.clone();
        trim_left(&mut c, &o, 0.0, F, Some(10.0)); // source end 6 may grow to 10: 2 s more
        assert!(near(c.position, 8.0) && c.in_point == 2.0 && near(c.out_point, 10.0));
        let mut c = o.clone();
        trim_right(&mut c, &o, 100.0, F, Some(10.0)); // in_point 2 may drop to 0: 1 s more
        assert!(near(c.duration, 3.0) && near(c.in_point, 0.0) && near(src_time(&c, 10.0), 6.0), "the left edge frame stays");
    }

    #[test]
    fn reversed_split_halves_join_and_play_backwards() {
        let mut c = clip(10.0, 6.0, 1.0);
        (c.rate, c.reverse) = (0.5, true);
        let (a, b) = split_clip(&c, 12.0).unwrap();
        // Every timeline time shows the same source as before the split.
        for t in [10.0, 11.0, 11.999] {
            assert!(near(src_time(&a, t), src_time(&c, t)));
        }
        for t in [12.0, 14.0, 16.0] {
            assert!(near(src_time(&b, t), src_time(&c, t)));
        }
        // Source ranges [b.in, b.out] then [a.in, a.out] meet, and the first half plays the later source.
        assert!(near(b.out_point, a.in_point) && near(b.in_point, 1.0) && near(a.out_point, 4.0));
        assert!(src_time(&a, 10.0) > src_time(&b, 12.0));
    }

    #[test]
    fn reverse_twice_is_the_identity_and_mute_follows_the_focus() {
        let (mut p, ids) = three_events();
        let before = p.timeline.video_tracks[0].clips[0].clone();
        assert!(reverse_clips(&mut p, &ids[..1]) && p.clip(&ids[0]).unwrap().reverse);
        assert!(reverse_clips(&mut p, &ids[..1]));
        assert_eq!(p.timeline.video_tracks[0].clips[0], before);
        p.timeline.video_tracks[0].clips[0].locked = true;
        assert!(!reverse_clips(&mut p, &ids[..1]), "a locked event stays");
        p.timeline.video_tracks[1].clips[0].muted = true;
        assert!(mute_clips(&mut p, &ids)); // the focus (audio) was not muted: all mute
        assert!(ids.iter().all(|id| p.clip(id).unwrap().muted));
        assert!(mute_clips(&mut p, &ids[..1]) && !p.clip(&ids[0]).unwrap().muted);
        assert!(!mute_clips(&mut p, &[]));
    }

    #[test]
    fn slip_keeps_the_event_and_clamps_the_media() {
        let o = clip(10.0, 4.0, 3.0);
        let mut c = o.clone();
        slip(&mut c, &o, 1.0, Some(10.0)); // the media moves right: an earlier frame shows
        assert!((c.position, c.duration) == (10.0, 4.0) && near(c.in_point, 2.0) && near(c.out_point, 6.0));
        slip(&mut c, &o, 5.0, Some(10.0));
        assert_eq!(c.in_point, 0.0);
        slip(&mut c, &o, -50.0, Some(10.0));
        assert!(near(c.in_point, 6.0)); // 10 - 4
        let mut r = o.clone();
        (r.reverse, r.rate) = (true, 2.0);
        let mut c = r.clone();
        slip(&mut c, &r, 0.5, Some(20.0)); // reversed, the source runs the other way
        assert!(near(src_time(&c, 11.0), src_time(&r, 10.5)));
    }

    #[test]
    fn slip_trim_keeps_the_frame_at_the_dragged_edge() {
        let o = clip(10.0, 4.0, 3.0);
        let mut c = o.clone();
        slip_trim_right(&mut c, &o, 13.0, F, Some(10.0));
        assert!(c.position == 10.0 && near(c.duration, 3.0) && near(src_time(&c, 13.0), src_time(&o, 14.0)));
        slip_trim_right(&mut c, &o, 30.0, F, Some(10.0)); // in_point 3 runs out after 3 s more
        assert!(near(c.duration, 7.0) && c.in_point == 0.0);
        slip_trim_left(&mut c, &o, 12.0, F, Some(10.0));
        assert!(near(c.position, 12.0) && c.in_point == 3.0 && near(c.position + c.duration, 14.0));
        slip_trim_left(&mut c, &o, 0.0, F, Some(10.0)); // the source end 7 may grow to 10
        assert!(near(c.position, 7.0) && near(c.out_point, 10.0));
    }

    #[test]
    fn slide_keeps_the_timeline_to_source_mapping() {
        let o = clip(4.0, 2.0, 3.0);
        let mut c = o.clone();
        slide(&mut c, &o, 0.5, Some(10.0));
        assert!(near(c.position, 4.5) && near(src_time(&c, 5.0), src_time(&o, 5.0)));
        slide(&mut c, &o, -10.0, Some(10.0)); // in_point 3 allows 3 s left
        assert!(near(c.position, 1.0) && c.in_point == 0.0);
        slide(&mut c, &o, 10.0, Some(10.0)); // the media end allows 5 s right
        assert!(near(c.position, 9.0) && near(c.out_point, 10.0));
        let mut r = o.clone();
        (r.reverse, r.rate) = (true, 2.0);
        let mut c = r.clone();
        slide(&mut c, &r, 0.25, Some(20.0));
        assert!(near(src_time(&c, 5.0), src_time(&r, 5.0)));
        let mut c = o.clone();
        slide(&mut c, &o, -10.0, None); // images slide freely, but not before 0
        assert_eq!(c.position, 0.0);
    }

    #[test]
    fn roll_moves_the_edit_point_within_both_media() {
        let (a, mut b) = (clip(0.0, 4.0, 1.0), clip(4.0, 3.0, 2.0));
        b.id = "b".into();
        let (mut x, mut y) = (a.clone(), b.clone());
        roll(&mut x, &a, Some(10.0), &mut y, &b, Some(6.0), 5.0, F);
        assert!(near(x.position + x.duration, 5.0) && x.position + x.duration == y.position && near(y.in_point, 3.0));
        roll(&mut x, &a, Some(10.0), &mut y, &b, Some(6.0), 1.0, F); // b's in_point 2 lets it reach back to 2
        assert!(near(y.position, 2.0) && x.position + x.duration == y.position && y.in_point == 0.0);
        roll(&mut x, &a, Some(5.0), &mut y, &b, Some(6.0), 6.5, F); // a's media (5 - 1) ends at 4
        assert!(near(x.duration, 4.0) && x.position + x.duration == y.position);
        let mut p = Project::new("t");
        p.timeline.video_tracks[0].clips.extend([a.clone(), b.clone()]);
        assert_eq!(adjacent(&p, &a.id, true, F), Some("b".into()));
        assert_eq!((adjacent(&p, "b", false, F), adjacent(&p, "b", true, F)), (Some(a.id.clone()), None));
        // A crossfade (b from 3, a to 4) slides: b's start and a's end move together and the 1 s overlap stays.
        let mut b2 = b.clone();
        (b2.position, b2.duration, b2.in_point) = (3.0, 4.0, 2.0);
        p.timeline.video_tracks[0].clips[1] = b2.clone();
        assert_eq!((adjacent(&p, "b", false, F), adjacent(&p, &a.id, true, F)), (Some(a.id.clone()), None));
        let (mut x, mut y) = (a.clone(), b2.clone());
        roll(&mut x, &a, Some(10.0), &mut y, &b2, Some(10.0), 3.5, F);
        assert!(near(y.position, 3.5) && near(x.position + x.duration, 4.5) && near(y.position + y.duration, 7.0) && near(y.in_point, 2.5));
        roll(&mut x, &a, Some(10.0), &mut y, &b2, Some(10.0), 0.0, F); // b's in_point 2 stops it at 1
        assert!(near(y.position, 1.0) && near(x.duration, 2.0));
    }

    #[test]
    fn quantize_toward_stays_inside_the_limit() {
        let fps = 10.0;
        assert!(near(quantize_toward(1.23, fps, 5.0), 1.3) && near(quantize_toward(1.27, fps, 5.0), 1.3));
        assert!(near(quantize_toward(1.27, fps, 0.0), 1.2) && near(quantize_toward(1.23, fps, 0.0), 1.2));
        assert_eq!((quantize_toward(1.2, fps, 0.0), quantize_toward(1.23, fps, 1.23)), (1.2, 1.23));
    }

    #[test]
    fn stretch_keeps_the_source_span_and_clamps_the_rate() {
        let o = clip(10.0, 4.0, 1.0);
        let mut c = o.clone();
        stretch(&mut c, &o, 18.0, false, F);
        assert!(c.position == 10.0 && near(c.duration, 8.0) && near(c.rate, 0.5) && near(c.rate * c.duration, 4.0));
        stretch(&mut c, &o, 100.0, false, F);
        assert!(c.rate == MIN_RATE && near(c.duration, 16.0));
        stretch(&mut c, &o, 13.9, true, F); // the left edge: the right one stays at 14
        assert!(c.rate == MAX_RATE && near(c.duration, 1.0) && near(c.position, 13.0) && c.in_point == 1.0);
        stretch(&mut c, &o, 12.0, true, F);
        assert!(near(c.rate, 2.0) && near(c.position, 12.0) && near(c.out_point, 5.0));
    }

    #[test]
    fn level_handle_gain_mapping() {
        assert_eq!(level_db(1.0), 0.0);
        assert!((level_db(0.5) + 6.0206).abs() < 1e-3);
        assert_eq!(level_db(0.0), -120.0);
    }

    fn env(kind: EnvKind, keys: &[(f64, f32)]) -> Envelope {
        Envelope { kind, points: Keyframes { keys: keys.iter().map(|&(t, v)| Keyframe { t, v, curve: Curve::Linear }).collect() } }
    }

    fn times(e: &Envelope) -> Vec<f64> {
        e.points.keys.iter().map(|k| k.t).collect()
    }

    #[test]
    fn env_insert_keeps_order_and_takes_the_left_curve() {
        let mut e = env(EnvKind::Volume, &[(0.0, 0.0), (4.0, -6.0)]);
        e.points.keys[0].curve = Curve::Fast;
        assert_eq!(env_insert(&mut e, 2.0, 99.0), 1);
        assert_eq!((times(&e), e.points.keys[1].v, e.points.keys[1].curve), (vec![0.0, 2.0, 4.0], 6.0, Curve::Fast));
        assert_eq!(env_insert(&mut e, 9.0, -3.0), 3);
        let mut m = env(EnvKind::Mute, &[(0.0, 1.0)]);
        env_insert(&mut m, 1.0, 0.3);
        assert_eq!((m.points.keys[1].v, m.points.keys[1].curve), (0.0, Curve::Hold));
    }

    #[test]
    fn env_move_clamps_time_and_value() {
        let o = env(EnvKind::CompositeLevel, &[(0.0, 1.0), (2.0, 0.5), (4.0, 0.5)]);
        let mut e = o.clone();
        assert!(env_move(&mut e, &o, &[1], 10.0, 0.9));
        assert!(e.points.keys[1].t < 4.0 && e.points.keys[1].t > 3.99 && e.points.keys[1].v == 1.0);
        assert!(env_move(&mut e, &o, &[1], -10.0, -0.9));
        assert!(e.points.keys[1].t > 0.0 && e.points.keys[1].t < 0.01 && e.points.keys[1].v == 0.0);
        assert!(env_move(&mut e, &o, &[0], 3.0, -0.25) && e.points.keys[0].t == 0.0, "the first point keeps its time");
        assert!(!env_move(&mut e, &o, &[1], 0.0, 0.0) && e == o);
        // Two points move together, stopped by the unselected one after them.
        let o = env(EnvKind::CompositeLevel, &[(0.0, 1.0), (1.0, 0.5), (2.0, 0.5), (3.0, 0.5)]);
        let mut e = o.clone();
        env_move(&mut e, &o, &[1, 2], 5.0, 0.0);
        let t = times(&e);
        assert!((t[2] - 3.0).abs() < 1e-5 && t[2] < 3.0 && (t[2] - t[1] - 1.0).abs() < 1e-9, "{t:?}");
    }

    #[test]
    fn env_delete_set_curve_and_span() {
        let mut e = env(EnvKind::Volume, &[(0.0, 0.0), (1.0, -3.0), (2.0, -6.0)]);
        assert!(keys_in_span(&e, 1.0, 2.0) == [1, 2] && keys_in_span(&e, 0.5, 0.9).is_empty());
        assert!(env_set_curve(&mut e, &[1], Curve::Smooth));
        assert_eq!(e.points.keys.iter().map(|k| k.curve).collect::<Vec<_>>(), [Curve::Linear, Curve::Smooth, Curve::Linear]);
        assert!(env_delete(&mut e, &[0, 1, 2]));
        assert_eq!(times(&e), [0.0], "the first point stays");
        assert!(!env_delete(&mut e, &[0]));
    }

    #[test]
    fn env_thin_flip_reset_and_toggle() {
        let mut e = env(EnvKind::CompositeLevel, &[(0.0, 0.0), (1.0, 0.5), (2.0, 1.0), (3.0, 0.0)]);
        assert!(env_thin(&mut e));
        assert_eq!(times(&e), [0.0, 2.0, 3.0], "the collinear midpoint goes, the corner at 2 stays");
        assert!(!env_thin(&mut e));
        let o = env(EnvKind::Volume, &[(0.0, -120.0), (1.0, -6.0), (2.0, 4.0)]);
        let mut f = o.clone();
        assert!(env_flip(&mut f) && f != o);
        env_flip(&mut f);
        assert!(f.points.keys.iter().zip(&o.points.keys).all(|(a, b)| (a.v - b.v).abs() < 1e-2), "{f:?}");
        assert!(env_reset(&mut f) && f.points.keys.iter().all(|k| k.v == 0.0));
        let mut t = Track::new("A", TrackKind::Audio);
        assert!(toggle_envelope(&mut t, EnvKind::Pan));
        assert_eq!((t.envelopes[0].points.keys.len(), t.envelopes[0].at(5.0)), (1, 0.0));
        assert!(toggle_envelope(&mut t, EnvKind::Pan) && t.envelopes.is_empty());
        let mut p = Project::new("t");
        let ids = vec![p.timeline.video_tracks[0].id.clone(), p.timeline.audio_tracks[0].id.clone()];
        assert!(set_envelope(&mut p, &ids, EnvKind::Volume, true) && p.timeline.video_tracks[0].envelopes.is_empty());
        assert!(!set_envelope(&mut p, &ids, EnvKind::Volume, true), "already there");
    }

    #[test]
    fn split_and_left_trim_keep_event_keys_on_the_timeline() {
        let mut c = clip(0.0, 5.0, 0.0);
        c.envelopes.push(env(EnvKind::Opacity, &[(0.0, 0.0), (3.0, 1.0)]));
        let (a, b) = split_clip(&c, 2.0).unwrap();
        assert_eq!(times(&b.envelopes[0]), [0.0, 1.0]);
        for t in [2.0, 2.5, 3.0, 4.0] {
            assert!((b.envelopes[0].at(t - 2.0) - c.envelopes[0].at(t)).abs() < 1e-6, "{t}");
        }
        assert!((a.envelopes[0].at(1.5) - c.envelopes[0].at(1.5)).abs() < 1e-6);
        let mut d = c.clone();
        trim_left(&mut d, &c, 1.0, F, None);
        assert_eq!(times(&d.envelopes[0]), [0.0, 2.0]);
        assert!((d.envelopes[0].at(0.0) - c.envelopes[0].at(1.0)).abs() < 1e-6);
        trim_left(&mut d, &c, 0.0, F, None); // recomputed from the original, not stacked
        assert_eq!(d.envelopes[0], c.envelopes[0]);
    }

    #[test]
    fn locked_envelopes_follow_moved_events() {
        let (mut p, ids) = three_events(); // the audio event is 2..4
        let vol = env(EnvKind::Volume, &[(0.0, 0.0), (3.0, -12.0), (8.0, 0.0)]);
        p.timeline.audio_tracks[0].envelopes.push(vol.clone());
        let o = p.clone();
        assert!(move_clips(&mut p, &ids[2..], 3.0, 0, true)); // to 5..7
        let e = &p.timeline.audio_tracks[0].envelopes[0];
        assert!(times(e).contains(&6.0) && !times(e).contains(&3.0) && times(e).contains(&8.0), "{:?}", times(e));
        assert!((e.at(6.0) + 12.0).abs() < 1e-6, "the point moved with its event");
        for t in [1.0, 1.99, 7.01, 7.5, 9.0] {
            assert!((e.at(t) - vol.at(t)).abs() < 1e-4, "{t}: {} vs {}", e.at(t), vol.at(t));
        }
        // An overlapping move keeps the vacated part flat at the old edge value.
        let mut q = o.clone();
        move_clips(&mut q, &ids[2..], 1.0, 0, true); // to 3..5
        let e = &q.timeline.audio_tracks[0].envelopes[0];
        assert!((e.at(4.0) + 12.0).abs() < 1e-6 && (e.at(2.5) - vol.at(2.0)).abs() < 1e-6 && (e.at(1.0) - vol.at(1.0)).abs() < 1e-6);
        let mut q = o.clone();
        move_clips(&mut q, &ids[2..], 3.0, 0, false);
        assert_eq!(q.timeline.audio_tracks[0].envelopes[0], vol, "lock off: nothing moves");
        // To another track: only into an envelope of the same kind.
        let mut q = o.clone();
        q.timeline.add_audio_track();
        q.timeline.audio_tracks[1].envelopes.push(env(EnvKind::Volume, &[(0.0, -6.0)]));
        assert!(move_clips(&mut q, &ids[2..], 0.0, 1, true));
        let (a0, a1) = (&q.timeline.audio_tracks[0].envelopes[0], &q.timeline.audio_tracks[1].envelopes[0]);
        assert!((a1.at(3.0) + 12.0).abs() < 1e-6 && (a1.at(1.0) + 6.0).abs() < 1e-6 && (a1.at(5.0) + 6.0).abs() < 1e-6);
        assert!(!times(a0).contains(&3.0) && (a0.at(1.0) - vol.at(1.0)).abs() < 1e-6);
    }

    #[test]
    fn locked_moves_add_no_redundant_points() {
        let (mut p, ids) = three_events(); // the audio event is 2..4
        p.timeline.audio_tracks[0].envelopes.push(env(EnvKind::Volume, &[(0.0, 0.0)]));
        let (o, flat) = (p.clone(), p.timeline.audio_tracks[0].envelopes[0].clone());
        for d in [3.0, -3.0, 3.0] {
            assert!(move_clips(&mut p, &ids[2..], d, 0, true));
            assert_eq!(p.timeline.audio_tracks[0].envelopes[0], flat, "a flat envelope stays one point");
        }
        // A ramp: moves there and back add no stacked copies, and the count stops growing.
        let mut p = o;
        p.timeline.audio_tracks[0].envelopes[0] = env(EnvKind::Volume, &[(0.0, 0.0), (10.0, -20.0)]);
        let mut counts = Vec::new();
        for d in [3.0, -3.0, 3.0, -3.0, 3.0, -3.0] {
            move_clips(&mut p, &ids[2..], d, 0, true);
            let k = &p.timeline.audio_tracks[0].envelopes[0].points.keys;
            assert!(k.windows(2).all(|w| w[1].t - w[0].t > KEY_EPS || w[1].v != w[0].v), "{k:?}");
            assert!(k.windows(3).all(|w| w[2].t - w[0].t > KEY_EPS), "at most two points (a step) at one time: {k:?}");
            counts.push(k.len());
        }
        assert!(counts[5] <= counts[1] + 2, "{counts:?}");
    }

    #[test]
    fn locked_move_down_two_tracks_keeps_each_tracks_points() {
        let (mut p, _) = three_events();
        p.timeline.add_audio_track();
        p.timeline.add_audio_track();
        p.timeline.audio_tracks[1].clips.push(clip(2.0, 2.0, 0.0));
        let ids = [0, 1].map(|i| p.timeline.audio_tracks[i].clips[0].id.clone());
        for (i, v) in [-12.0, -24.0, 0.0].into_iter().enumerate() {
            p.timeline.audio_tracks[i].envelopes.push(env(EnvKind::Volume, &[(0.0, 0.0), (3.0, v)]));
        }
        assert!(move_clips(&mut p, &ids, 0.0, 1, true));
        let at = |i: usize| p.timeline.audio_tracks[i].envelopes[0].at(3.0);
        assert!((at(1) + 12.0).abs() < 1e-6 && (at(2) + 24.0).abs() < 1e-6, "{} {}", at(1), at(2));
    }

    /// Post-Edit Ripple after a paste ends where Auto Ripple would have: the pasted events stay where they landed.
    #[test]
    fn post_edit_ripple_matches_auto_ripple() {
        let (p0, _) = ripple_project();
        let copied = copy_clips(&p0, &[p0.timeline.video_tracks[0].clips[0].id.clone()]);
        let mut auto = p0.clone();
        let op = paste_ripple(&auto, &copied, 3.0, Some((true, 0)));
        assert_eq!((op.delta, op.tracks.len()), (2.0, 1));
        ripple(&mut auto, &op, RippleMode::Affected);
        paste_clips(&mut auto, &copied, 3.0, Some((true, 0)));
        let mut post = p0.clone();
        let ids = paste_clips(&mut post, &copied, 3.0, Some((true, 0)));
        ripple(&mut post, &RippleOp { exclude: ids, ..op }, RippleMode::Affected);
        assert_eq!(starts(&auto.timeline.video_tracks[0]), [0.0, 3.0, 5.0, 10.0]);
        assert_eq!(starts(&post.timeline.video_tracks[0]), starts(&auto.timeline.video_tracks[0]));
    }

    /// An audio event on "a" at 2..6 s from 1 s, with "v" (from 5 s) added as a take.
    fn take_project() -> (Project, String) {
        let mut p = media_project();
        let mut c = Clip::from_media(&p.media_pool[2], 2.0);
        (c.in_point, c.duration) = (1.0, 3.0);
        sync_out(&mut c);
        add_as_take(&mut c, &p.media_pool[0], 5.0);
        let id = c.id.clone();
        p.timeline.audio_tracks[0].clips.push(c);
        (p, id)
    }

    fn active(p: &Project, id: &str) -> (String, f64, String) {
        let c = p.clip(id).unwrap();
        (c.media_id.clone().unwrap_or_default(), c.in_point, c.name.clone())
    }

    #[test]
    fn takes_cycle_add_and_delete() {
        let (mut p, id) = take_project();
        let ids = vec![id.clone()];
        // Add as Take: the new media is active, the old one kept.
        assert_eq!(active(&p, &id), ("v".into(), 5.0, "v".into()));
        assert_eq!(p.clip(&id).unwrap().takes, [Take { media_id: "a".into(), in_point: 1.0, name: "a".into() }]);
        let c0 = p.clip(&id).unwrap().clone();
        assert!(next_take(&mut p, &ids, true));
        let c = p.clip(&id).unwrap();
        assert_eq!((active(&p, &id), c.position, c.duration, c.out_point), (("a".into(), 1.0, "a".into()), 2.0, 3.0, 4.0));
        assert!(next_take(&mut p, &ids, false));
        assert_eq!(*p.clip(&id).unwrap(), c0, "next then previous is the identity");
        // A third take: the ring goes v -> a -> i -> v; choosing inactive take 1 is two steps on.
        let mut c = c0.clone();
        c.takes.push(Take { media_id: "i".into(), in_point: 0.0, name: "i".into() });
        choose_take(&mut c, 1);
        assert_eq!((c.name.as_str(), c.takes.iter().map(|t| t.name.as_str()).collect::<Vec<_>>()), ("i", vec!["v", "a"]));
        // Delete Active: the next take takes over; with none left it is refused.
        assert!(delete_active_take(&mut p, &ids));
        assert_eq!((active(&p, &id).0, p.clip(&id).unwrap().takes.len()), ("a".into(), 0));
        p.modified = false;
        assert!(!delete_active_take(&mut p, &ids) && !next_take(&mut p, &ids, true) && !p.modified);
        assert_eq!(active(&p, &id).0, "a");
        // Take Chooser: Choose Active picks a take in the ring; Delete... removes only the picked take.
        let (mut p, id) = take_project();
        p.timeline.audio_tracks[0].clips[0].takes.push(Take { media_id: "i".into(), in_point: 0.0, name: "i".into() });
        assert!(!take_chooser(&mut p, &id, 0, false) && !take_chooser(&mut p, &id, 3, true), "the active take, or none");
        assert!(take_chooser(&mut p, &id, 2, false));
        let names = |p: &Project| (p.clip(&id).unwrap().name.clone(), p.clip(&id).unwrap().takes.iter().map(|t| t.name.clone()).collect::<Vec<_>>());
        assert_eq!(names(&p), ("i".into(), vec!["v".into(), "a".into()]));
        assert!(take_chooser(&mut p, &id, 2, true));
        assert_eq!(names(&p), ("i".into(), vec!["v".into()]));
        assert!(take_chooser(&mut p, &id, 0, true));
        assert_eq!(names(&p), ("v".into(), vec![]));
        assert!(!take_chooser(&mut p, &id, 0, true), "the only take stays");
    }

    /// Media used only by an inactive take is in use; removing media keeps the event while it has another take.
    #[test]
    fn removed_media_leaves_the_other_takes() {
        let (mut p, id) = take_project(); // active v, take a
        let c = p.clip(&id).unwrap();
        assert!(uses_media(c, "a") && uses_media(c, "v") && !uses_media(c, "i"));
        let mut q = p.clone();
        forget_media(&mut q.timeline, "a");
        assert_eq!((active(&q, &id).0, q.clip(&id).unwrap().takes.len()), ("v".into(), 0));
        forget_media(&mut p.timeline, "v");
        assert_eq!((active(&p, &id), p.clip(&id).unwrap().takes.len()), (("a".into(), 1.0, "a".into()), 0));
        forget_media(&mut p.timeline, "a");
        assert!(p.clip(&id).is_none(), "no take left");
    }

    #[test]
    fn splits_and_trims_keep_takes_lined_up() {
        let (p, id) = take_project();
        let c = p.clip(&id).unwrap(); // v from 5 at 2..5 s; take a from 1
        let (a, b) = split_clip(c, 3.0).unwrap();
        assert_eq!((a.takes[0].in_point, b.in_point, b.takes[0].in_point), (1.0, 6.0, 2.0));
        let mut t = c.clone();
        trim_left(&mut t, c, 2.5, F, None);
        assert_eq!((t.in_point, t.takes[0].in_point), (5.5, 1.5));
        let mut s = c.clone();
        slip(&mut s, c, 1.0, None); // a slip moves the active take alone
        assert_eq!((s.in_point, s.takes[0].in_point), (4.0, 1.0));
        // Slip-trims too: the inactive take keeps showing the same source at each timeline time.
        let mut s = c.clone();
        slip_trim_left(&mut s, c, 3.0, F, None);
        assert_eq!((s.position, s.in_point, s.takes[0].in_point), (3.0, 5.0, 2.0));
        let mut s = c.clone();
        slip_trim_right(&mut s, c, 4.0, F, None);
        assert_eq!((s.duration, s.in_point, s.takes[0].in_point), (2.0, 6.0, 1.0));
    }

    #[test]
    fn drop_as_takes_and_across_tracks() {
        let p = media_project();
        let items: Vec<_> = [2, 0].iter().map(|&i| (&p.media_pool[i], None)).collect();
        let plan = plan_drop_as(&p.timeline, &items, Some((false, 0)), 1.0, 5.0, DropMode::AsTakes);
        // Audio "a" (4 s) is the audio event with v's sound as its take; v's picture is the video event.
        let ev: Vec<_> = plan.events.iter().map(|(v, i, c)| (*v, *i, c.position, c.duration, c.takes.len(), c.group.is_some())).collect();
        assert_eq!(ev, [(false, 0, 1.0, 4.0, 1, false), (true, 0, 1.0, 10.0, 0, false)]);
        assert_eq!(plan.events[0].2.takes[0].media_id, "v");
        let plan = plan_drop_as(&p.timeline, &items, Some((false, 0)), 1.0, 5.0, DropMode::AcrossTracks);
        let ev: Vec<_> = plan.events.iter().map(|(v, i, c)| (*v, *i, c.position)).collect();
        assert_eq!(ev, [(false, 0, 1.0), (true, 0, 1.0), (false, 1, 1.0)]);

        // Onto an audio event: "a" and v's sound become takes (v last, so active); the still has no sound.
        let (mut p, id) = take_project(); // 2..5 s on Audio 1, active v, take a
        let pool = p.media_pool.clone();
        let items: Vec<_> = [1, 2, 0].iter().map(|&i| (&pool[i], Some((0.5, 1.0)))).collect();
        assert_eq!(add_takes_to(&mut p, (false, 0), 3.0, &items), Some(id.clone()));
        let c = p.clip(&id).unwrap();
        assert_eq!((c.name.as_str(), c.in_point, c.duration), ("v", 0.5, 3.0));
        assert_eq!(c.takes.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["a", "v", "a"]);
        assert!(p.modified && add_takes_to(&mut p, (false, 0), 6.0, &items).is_none(), "no event there");
    }

    /// A/V takes stay matched: picture and sound of each take come from the same item, also after T.
    #[test]
    fn av_takes_stay_matched() {
        let p = media_project();
        let plan_of = |idx: &[usize]| {
            let items: Vec<_> = idx.iter().map(|&i| (&p.media_pool[i], None)).collect();
            plan_drop_as(&p.timeline, &items, Some((true, 0)), 0.0, 5.0, DropMode::AsTakes)
        };
        // v, still, v: the video has two takes and the audio one, so they are not grouped.
        let plan = plan_of(&[0, 1, 0]);
        let ev: Vec<_> = plan.events.iter().map(|(v, _, c)| (*v, c.takes.len(), c.group.is_some())).collect();
        assert_eq!(ev, [(true, 2, false), (false, 1, false)]);
        assert!(plan_of(&[0, 0]).events.iter().all(|e| e.2.group.is_some() && e.2.takes.len() == 1));

        // Onto the video half of a grouped A/V event: only items with picture and sound go, to both halves.
        let mut p = media_project();
        let ids = drop_media(&mut p, &[0], None, Some((true, 0)), 0.0, 5.0, false);
        let ids2 = drop_media_as(&mut p, &[2, 0, 1], Some((2.0, 3.0)), Some((true, 0)), 1.0, 5.0, false, DropMode::AsTakes);
        assert_eq!(ids2, [ids[0].clone()]);
        let src = |p: &Project| ids.iter().map(|id| active(p, id)).collect::<Vec<_>>();
        let v: (String, f64, String) = ("v".into(), 2.0, "v".into());
        assert_eq!(src(&p), [v.clone(), v]);
        for _ in 0..2 {
            assert!(next_take(&mut p, &ids, true));
            let s = src(&p);
            assert_eq!(s[0], s[1]);
        }
        // A locked half refuses the takes.
        let r = p.find_clip(&ids[1]).unwrap();
        p.timeline.audio_tracks[r.track_idx].clips[r.clip_idx].locked = true;
        let pool = p.media_pool.clone();
        assert!(add_takes_to(&mut p, (true, 0), 1.0, &[(&pool[0], None)]).is_none());
    }

    #[test]
    fn properties_rate_and_unchanged_draft() {
        let mut c = clip(0.0, 4.0, 0.0);
        c.fade_in = 3.0;
        apply_rate(&mut c, 2.0);
        assert_eq!((c.rate, c.duration, c.fade_in, c.out_point), (2.0, 2.0, 2.0, 4.0));
        apply_rate(&mut c, 10.0);
        assert_eq!((c.rate, c.duration), (4.0, 1.0));
        apply_rate(&mut c, 0.0);
        assert_eq!((c.rate, c.duration), (0.25, 16.0));

        let (mut p, id) = take_project();
        let draft = p.clip(&id).unwrap().clone();
        assert!(!apply_props(&mut p, &id, draft.clone()) && !p.modified, "an unchanged draft is no edit");
        // Media v is 10 s: from in 5 the event can last 5 s at most with Loop off; it moves after the other event.
        p.timeline.audio_tracks[0].clips.push(clip(4.0, 1.0, 0.0));
        let mut d = draft;
        (d.duration, d.position, d.name) = (8.0, 6.0, "Take B".into());
        assert!(apply_props(&mut p, &id, d.clone()));
        assert_eq!(p.clip(&id).unwrap().duration, 8.0, "a looping event runs on past its media");
        d.looping = false;
        assert!(apply_props(&mut p, &id, d.clone()));
        assert_eq!(p.clip(&id).unwrap().duration, 8.0, "turning Loop off keeps the length it had");
        d.duration = 4.0;
        assert!(apply_props(&mut p, &id, d.clone()));
        d.duration = 8.0;
        assert!(apply_props(&mut p, &id, d));
        let c = &p.timeline.audio_tracks[0].clips[1];
        assert_eq!((c.id.as_str(), c.position, c.duration, c.name.as_str()), (id.as_str(), 6.0, 5.0, "Take B"));

        // Name, rate and curves together: one apply (one undo step) does all of them.
        let mut d = p.clip(&id).unwrap().clone();
        d.name = "Take C".into();
        apply_rate(&mut d, 2.0);
        (d.fade_in, d.fade_in_curve, d.fade_out_curve) = (0.5, Curve::Fast, Curve::Sharp);
        assert!(apply_props(&mut p, &id, d.clone()));
        let c = p.clip(&id).unwrap();
        assert_eq!((c.name.as_str(), c.rate, c.duration, c.fade_in_curve, c.fade_out_curve), ("Take C", 2.0, 2.5, Curve::Fast, Curve::Sharp));

        // On a locked track only Lock changes.
        p.timeline.audio_tracks[0].locked = true;
        let mut d = p.clip(&id).unwrap().clone();
        (d.locked, d.name) = (true, "ignored".into());
        assert!(apply_props(&mut p, &id, d.clone()));
        assert_eq!((p.clip(&id).unwrap().locked, p.clip(&id).unwrap().name.as_str()), (true, "Take C"));
        assert!(!apply_props(&mut p, &id, d), "nothing else to change");
    }

    #[test]
    fn composite_mode_is_stored_and_replans() {
        let mut p = Project::new("t");
        let id = p.timeline.video_tracks[0].id.clone();
        let h = crate::engine::content_hash(&p);
        assert!(set_composite(&mut p, &id, Composite::Screen));
        assert_eq!(p.timeline.video_tracks[0].composite, Composite::Screen);
        assert_ne!(crate::engine::content_hash(&p), h);
        assert!(!set_composite(&mut p, &id, Composite::Screen), "unchanged");
        let audio = p.timeline.audio_tracks[0].id.clone();
        assert!(!set_composite(&mut p, &audio, Composite::Add), "audio tracks have no mode");
        p.timeline.video_tracks[0].locked = true;
        assert!(!set_composite(&mut p, &id, Composite::Add), "locked");
        // Saved and loaded with the project.
        let back: Project = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back.timeline.video_tracks[0].composite, Composite::Screen);
    }

    /// R7: Make Compositing Child / Parent, the Parent Composite Mode, a compositor from the Compositors window and its
    /// FX window target, the Video Bus Track's envelopes, and mask keys that stay put through a split.
    #[test]
    fn compositing_parent_child_compositor_bus_and_masks() {
        let mut p = Project::new("t");
        p.timeline.add_video_track();
        let (top, below) = (p.timeline.video_tracks[0].id.clone(), p.timeline.video_tracks[1].id.clone());
        assert!(!set_child(&mut p, std::slice::from_ref(&top), true), "the top track has no parent");
        assert!(set_child(&mut p, std::slice::from_ref(&below), true) && is_parent(&p.timeline, 0) && !is_parent(&p.timeline, 1));
        assert!(set_parent_mode(&mut p, &top, Composite::Add) && p.timeline.video_tracks[0].parent_mode == Composite::Add);
        assert!(set_child(&mut p, std::slice::from_ref(&below), false) && !is_parent(&p.timeline, 0));
        let bump = fx("milan.comp.bump_map");
        assert!(set_compositor(&mut p, std::slice::from_ref(&top), &bump));
        let t = &p.timeline.video_tracks[0];
        assert!(t.composite == Composite::Custom && t.compositor.as_ref().is_some_and(|c| c.plugin == bump.plugin && c.id != bump.id));
        let target = FxTarget::Compositor(top.clone());
        let (mut list, free) = fx_list(&p, &target).unwrap();
        list[0].params.insert("height".into(), Keyframes { keys: vec![Keyframe { t: 0.0, v: 0.9, curve: Curve::Linear }] });
        assert!(free && set_fx_list(&mut p, &target, list.clone()) && !set_fx_list(&mut p, &target, list));
        assert!(toggle_bus_envelope(&mut p, None, EnvKind::MotionBlur) && p.timeline.master.video_envelopes[0].kind == EnvKind::MotionBlur);
        toggle_bus_envelope(&mut p, None, EnvKind::MotionBlur);
        assert!(!toggle_bus_envelope(&mut p, Some("no such bus"), EnvKind::Volume));
        assert!(p.timeline.master.video_envelopes.is_empty());
        // A mask keyed at 1 s and 3 s of an event split at 2 s: the right half's keys move 2 s earlier.
        let mut c = Clip::new("c", 0.0, 4.0);
        let path = |x: f32| vec![[x, 0.0, 0.0, 0.0, 0.0, 0.0]; 3];
        let key = |t, x| Keyframe { t, v: path(x), curve: Curve::Linear };
        c.masks = vec![crate::project::Mask { path: Keyframes { keys: vec![key(1.0, 0.0), key(3.0, 1.0)] }, mode: crate::project::MaskMode::Positive, opacity: 1.0, feather: 0.0 }];
        shift_clip_keys(&mut c, -2.0);
        assert_eq!(c.masks[0].path.times(), vec![0.0, 1.0]);
        assert!((c.masks[0].path.at(0.0)[0][0] - 0.5).abs() < 1e-6);
    }

    fn fx(plugin: &str) -> Fx {
        crate::engine::plugins::new_fx(crate::engine::plugins::find(plugin).unwrap(), None)
    }

    fn json(p: &Project) -> serde_json::Value {
        serde_json::to_value(p).unwrap()
    }

    #[test]
    fn fx_chains_add_remove_move_and_respect_locks() {
        let mut p = Project::new("t");
        p.timeline.video_tracks[0].clips.push(Clip::new("c", 0.0, 4.0));
        let ev = FxTarget::Event(p.timeline.video_tracks[0].clips[0].id.clone());
        let tr = FxTarget::Track(p.timeline.video_tracks[0].id.clone());
        let start = json(&p);
        assert!(add_fx(&mut p, &ev, fx("milan.fx.invert")));
        assert!(remove_fx(&mut p, &ev, 0));
        assert_eq!(json(&p), start, "add then remove restores the project");
        // Locked event, locked track: refused, nothing changes.
        p.timeline.video_tracks[0].clips[0].locked = true;
        assert!(!add_fx(&mut p, &ev, fx("milan.fx.invert")));
        p.timeline.video_tracks[0].clips[0].locked = false;
        p.timeline.video_tracks[0].locked = true;
        assert!(!add_fx(&mut p, &ev, fx("milan.fx.invert")) && !add_fx(&mut p, &tr, fx("milan.fx.invert")));
        p.timeline.video_tracks[0].locked = false;
        assert_eq!(json(&p), start);
        assert!(!add_fx(&mut p, &FxTarget::Event("gone".into()), fx("milan.fx.invert")));
        // Reorder and bypass.
        for id in ["milan.fx.invert", "milan.fx.sepia", "milan.fx.crop"] {
            assert!(add_fx(&mut p, &tr, fx(id)));
        }
        assert!(move_fx(&mut p, &tr, 0, 2));
        let order = |p: &Project| p.timeline.video_tracks[0].fx.iter().map(|f| f.plugin.clone()).collect::<Vec<_>>();
        assert_eq!(order(&p), ["milan.fx.sepia", "milan.fx.crop", "milan.fx.invert"]);
        assert!(move_fx(&mut p, &tr, 2, 0) && !move_fx(&mut p, &tr, 1, 1) && !move_fx(&mut p, &tr, 5, 0));
        assert_eq!(order(&p), ["milan.fx.invert", "milan.fx.sepia", "milan.fx.crop"]);
        assert!(set_fx_enabled(&mut p, &tr, 1, false) && !set_fx_enabled(&mut p, &tr, 1, false));
        assert!(!p.timeline.video_tracks[0].fx[1].enabled);
        // Video Output FX is the master bus's video chain.
        let chain: *const Vec<Fx> = fx_chain_mut(&mut p, &FxTarget::VideoOutput).unwrap();
        assert!(std::ptr::eq(chain, &p.timeline.master.video_fx));
        assert!(add_fx(&mut p, &FxTarget::VideoOutput, fx("milan.fx.invert")));
        assert_eq!(p.timeline.master.video_fx.len(), 1);
        // Audio Master FX is its audio chain; audio events and tracks are audio targets.
        assert!(add_fx(&mut p, &FxTarget::AudioMaster, fx("milan.afx.limiter")));
        assert_eq!((p.timeline.master.audio_fx.len(), fx_list(&p, &FxTarget::AudioMaster).unwrap().0.len()), (1, 1));
        p.timeline.audio_tracks[0].clips.push(Clip::new("a", 0.0, 1.0));
        let (ae, at) = (FxTarget::Event(p.timeline.audio_tracks[0].clips[0].id.clone()), FxTarget::Track(p.timeline.audio_tracks[0].id.clone()));
        assert!([&ae, &at, &FxTarget::AudioMaster].iter().all(|t| fx_is_audio(&p, t)));
        assert!([&ev, &tr, &FxTarget::VideoOutput].iter().all(|t| !fx_is_audio(&p, t)));
    }

    /// Two events meeting at 5 s on video track 1, each with `handle` seconds of media past the cut.
    fn cut(handle_a: f64, handle_b: f64) -> (Project, String) {
        let mut p = Project::new("t");
        for (id, dur) in [("a", 5.0 + handle_a), ("b", 5.0 + handle_b)] {
            p.media_pool.push(MediaItem { id: id.into(), media_type: MediaType::Video, duration: dur, ..Default::default() });
        }
        let mut a = Clip::from_media(&p.media_pool[0], 0.0);
        (a.duration, a.out_point) = (5.0, 5.0);
        let mut b = Clip::from_media(&p.media_pool[1], 5.0);
        (b.in_point, b.duration) = (handle_b, 5.0);
        b.out_point = b.in_point + 5.0;
        // Loop off: past its handle an event has no more media (a looping one would repeat it).
        (a.looping, b.looping) = (false, false);
        p.timeline.video_tracks[0].clips = vec![a, b];
        let t = p.timeline.video_tracks[0].id.clone();
        (p, t)
    }

    fn dissolve() -> Transition {
        Transition { plugin: "milan.tr.dissolve".into(), ..Default::default() }
    }

    #[test]
    fn a_transition_on_a_cut_makes_a_centred_crossfade() {
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
        let (mut p, t) = cut(5.0, 5.0);
        let mut h = crate::history::History::default();
        let before = p.clone();
        assert!(set_transition(&mut p, &t, 4.9, dissolve(), 1.0, true));
        h.push(before.clone());
        let c = &p.timeline.video_tracks[0].clips;
        assert!(close(c[0].position + c[0].duration, 5.5) && close(c[1].position, 4.5) && close(c[1].in_point, 4.5));
        assert!(close(c[0].fade_out, 1.0) && close(c[1].fade_in, 1.0) && c[1].transition == Some(dissolve()));
        // One undo puts back positions, trims and fades.
        assert!(h.undo(&mut p));
        assert_eq!(p.timeline.video_tracks[0].clips, before.timeline.video_tracks[0].clips);
        // No outgoing handle: the incoming event takes the whole overlap.
        let (mut p, t) = cut(0.0, 5.0);
        assert!(set_transition(&mut p, &t, 5.1, dissolve(), 1.0, true));
        let c = &p.timeline.video_tracks[0].clips;
        assert!(close(c[0].duration, 5.0) && close(c[1].position, 4.0) && close(c[1].fade_in, 1.0) && close(c[0].fade_out, 1.0));
        // Automatic Crossfades off: a cut is refused.
        let (mut p, t) = cut(5.0, 5.0);
        let start = json(&p);
        assert!(!set_transition(&mut p, &t, 4.9, dissolve(), 1.0, false));
        assert_eq!(json(&p), start);
        // Locked track, or a locked event: refused.
        p.timeline.video_tracks[0].locked = true;
        let start = json(&p);
        assert!(!set_transition(&mut p, &t, 4.9, dissolve(), 1.0, true));
        assert_eq!(json(&p), start);
        (p.timeline.video_tracks[0].locked, p.timeline.video_tracks[0].clips[1].locked) = (false, true);
        assert!(!set_transition(&mut p, &t, 4.9, dissolve(), 1.0, true));
        // A sub-frame overlap (float error) is still a cut: it converts, at the incoming event's start.
        let (mut p, t) = cut(5.0, 5.0);
        p.timeline.video_tracks[0].clips[0].duration += 1e-9;
        assert!(set_transition(&mut p, &t, 5.0, dissolve(), 1.0, true));
        let c = &p.timeline.video_tracks[0].clips;
        assert!((c[1].position - 4.5).abs() < 1e-6 && (c[1].fade_in - 1.0).abs() < 1e-6);
    }

    /// The conversion never runs one event past the other or into the event beyond it.
    #[test]
    fn a_cut_to_overlap_stops_at_the_short_event_and_its_neighbour() {
        let frame = 1.0 / 30.0;
        let image = |p: &mut Project, at: f64, len: f64| {
            let m = MediaItem { id: format!("img{at}"), media_type: MediaType::Image, ..Default::default() };
            let mut c = Clip::from_media(&m, at);
            (c.duration, c.out_point) = (len, len);
            p.media_pool.push(m);
            c
        };
        // A (0-5, 5 s tail handle) | B (5.0-5.3, no head handle) | D at 5.3.
        let (mut p, t) = cut(5.0, 0.0);
        p.settings.frame_rate = 30.0;
        let tr = &mut p.timeline.video_tracks[0];
        (tr.clips[1].duration, tr.clips[1].out_point) = (0.3, 0.3);
        let d = image(&mut p, 5.3, 2.0);
        p.timeline.video_tracks[0].clips.push(d.clone());
        assert!(set_transition(&mut p, &t, 4.9, dissolve(), 1.0, true));
        let c = &p.timeline.video_tracks[0].clips;
        assert!(c[0].position + c[0].duration <= 5.3 - frame + 1e-9, "A stops a frame short of B's end");
        assert_eq!((c[1].position, &c[2]), (5.0, &d), "B stays, D is untouched");
        // Mirror: P (0-4) | A (4.0-4.3, no tail handle) | B (4.3, 5 s head handle).
        let mut p = Project::new("t");
        p.settings.frame_rate = 30.0;
        let pc = image(&mut p, 0.0, 4.0);
        for (id, dur) in [("a", 0.3), ("b", 10.0)] {
            p.media_pool.push(MediaItem { id: id.into(), media_type: MediaType::Video, duration: dur, ..Default::default() });
        }
        let mut a = Clip::from_media(&p.media_pool[1], 4.0);
        (a.duration, a.out_point, a.looping) = (0.3, 0.3, false);
        let mut b = Clip::from_media(&p.media_pool[2], 4.3);
        (b.in_point, b.duration, b.out_point) = (5.0, 5.0, 10.0);
        p.timeline.video_tracks[0].clips = vec![pc.clone(), a, b];
        let t = p.timeline.video_tracks[0].id.clone();
        assert!(set_transition(&mut p, &t, 4.31, dissolve(), 1.0, true));
        let c = &p.timeline.video_tracks[0].clips;
        assert!(c[2].position >= 4.0 + frame - 1e-9, "B stops a frame after A's start");
        assert_eq!((&c[0], c[1].position, c[1].duration), (&pc, 4.0, 0.3), "P and A are untouched");
    }

    #[test]
    fn a_transition_on_an_overlap_or_a_lone_start_or_end() {
        let (mut p, t) = cut(5.0, 5.0);
        p.timeline.video_tracks[0].clips[1].position = 4.0; // a 1 s overlap
        let mut want = p.clone();
        want.timeline.video_tracks[0].clips[1].transition = Some(dissolve());
        assert!(set_transition(&mut p, &t, 4.5, dissolve(), 1.0, true));
        assert_eq!(json(&p), json(&want), "on an overlap only the field changes");
        assert!(!set_transition(&mut p, &t, 4.5, dissolve(), 1.0, true), "already there");
        // A lone start fades in over the cut-to-overlap length; R4a: the end of the last event fades out to
        // transparent through it (`transition_out`).
        let (mut p, t) = cut(5.0, 5.0);
        p.timeline.video_tracks[0].clips.remove(0);
        assert!(set_transition(&mut p, &t, 5.2, dissolve(), 0.5, false));
        assert_eq!(p.timeline.video_tracks[0].clips[0].fade_in, 0.5);
        assert!(set_transition(&mut p, &t, 9.5, dissolve(), 0.5, true));
        let c = &p.timeline.video_tracks[0].clips[0];
        assert_eq!((c.fade_out, c.transition_out.as_ref()), (0.5, Some(&dissolve())));
        assert!(!set_transition(&mut p, &t, 9.5, dissolve(), 0.5, true), "already there");
        let id = p.timeline.video_tracks[0].clips[0].id.clone();
        assert!(remove_transition(&mut p, &id, false) && !remove_transition(&mut p, &id, false));
        assert!(p.timeline.video_tracks[0].clips[0].transition.is_none() && p.timeline.video_tracks[0].clips[0].transition_out.is_some());
        assert!(remove_transition(&mut p, &id, true) && p.timeline.video_tracks[0].clips[0].transition_out.is_none());
        // An end that a later event overlaps is a crossfade, not a lone fade-out.
        let (mut p, t) = cut(5.0, 5.0);
        p.timeline.video_tracks[0].clips[1].position = 4.0;
        assert!(!set_transition(&mut p, &t, 3.5, dissolve(), 0.5, true));
        // The FX window edits a transition as a plug-in and puts it back, keeping its progress envelope.
        let tr = Transition { plugin: "milan.tr.linear_wipe".into(), params: [("angle".to_string(), 90.0)].into(), ..Default::default() };
        assert_eq!(transition_of(&fx_of_transition(&tr)), tr);
        let (mut p, t) = cut(5.0, 5.0);
        assert!(set_transition(&mut p, &t, 9.5, dissolve(), 0.5, true));
        let id = p.timeline.video_tracks[0].clips[1].id.clone();
        assert!(toggle_progress(&mut p, &[id.clone()]));
        let target = FxTarget::Transition(format!("{id}{TRANSITION_OUT}"));
        let (mut list, _) = fx_list(&p, &target).unwrap();
        list[0].plugin = "milan.tr.linear_wipe".into();
        assert!(set_fx_list(&mut p, &target, list));
        let out = p.clip(&id).unwrap().transition_out.clone().unwrap();
        assert_eq!((out.plugin.as_str(), out.progress.keys.len()), ("milan.tr.linear_wipe", 2));
    }

    #[test]
    fn generated_media_inserts_as_one_undo_step() {
        let mut p = Project::new("t");
        let mut h = crate::history::History::default();
        let before = p.clone();
        let id = insert_generated(&mut p, None, 2.0, fx("milan.gen.solid"), "Solid Color", 5.0).unwrap();
        h.push(before);
        assert_eq!((p.media_pool.len(), p.timeline.video_tracks[0].clips.len()), (1, 1));
        let c = p.clip(&id).unwrap();
        assert_eq!((c.position, c.duration, c.media_id.as_ref()), (2.0, 5.0, Some(&p.media_pool[0].id)));
        assert!(p.media_pool[0].generator.is_some());
        assert!(h.undo(&mut p));
        assert!(p.media_pool.is_empty() && p.timeline.video_tracks[0].clips.is_empty());
        // No video track: one is made. A locked one refuses.
        p.timeline.video_tracks.clear();
        assert!(insert_generated(&mut p, Some(3), 0.0, fx("milan.gen.solid"), "S", 5.0).is_some());
        assert_eq!(p.timeline.video_tracks.len(), 1);
        p.timeline.video_tracks[0].locked = true;
        assert!(insert_generated(&mut p, None, 0.0, fx("milan.gen.solid"), "S", 5.0).is_none());
        // Past the last track (a drop below the tracks): a new track.
        assert!(insert_generated(&mut p, Some(1), 0.0, fx("milan.gen.solid"), "S", 5.0).is_some());
        assert_eq!(p.timeline.video_tracks.len(), 2);
        // The generator is edited through the FX window's list.
        let m = FxTarget::Media(p.media_pool[0].id.clone());
        let (mut list, free) = fx_list(&p, &m).unwrap();
        list[0].text.insert("x".into(), "y".into());
        assert!(free && set_fx_list(&mut p, &m, list.clone()) && !set_fx_list(&mut p, &m, list));
    }

    /// R2: Paste Repeat ×3 every 2 s, and end to end.
    #[test]
    fn paste_repeat_every_and_end_to_end() {
        let (mut p, ids) = three_events();
        let clips = copy_clips(&p, &ids[..1]); // a 2 s event
        let new = paste_repeat(&mut p, &clips, 10.0, Some((true, 0)), 3, Some(2.5));
        let pos: Vec<f64> = new.iter().map(|id| p.clip(id).unwrap().position).collect();
        assert_eq!(pos, [10.0, 12.5, 15.0]);
        let new = paste_repeat(&mut p, &clips, 20.0, Some((true, 0)), 3, None);
        let pos: Vec<f64> = new.iter().map(|id| p.clip(id).unwrap().position).collect();
        assert_eq!(pos, [20.0, 22.0, 24.0]);
        p.timeline.video_tracks[0].locked = true;
        assert!(paste_repeat(&mut p, &clips, 30.0, Some((true, 0)), 3, None).is_empty());
    }

    /// R2: Backspace restores the selections before the current one, newest first, five at most.
    #[test]
    fn restore_selection_after_three_changes() {
        let mut h = SelHistory::default();
        for s in [Some((1.0, 2.0)), Some((3.0, 4.0)), None, Some((5.0, 6.0))] {
            h.observe(s);
            h.observe(s); // seen again on the next frame: no change
        }
        assert_eq!(h.restore(), Some((3.0, 4.0)), "the cleared selection is skipped");
        assert_eq!(h.restore(), Some((1.0, 2.0)));
        assert_eq!(h.restore(), None);
        h.observe(None);
        assert_eq!(h.loop_region, Some((1.0, 2.0)), "Select Loop Region brings back the last selection");
        for k in 0..8 {
            h.observe(Some((k as f64, k as f64 + 1.0)));
        }
        let mut n = 0;
        while h.restore().is_some() {
            n += 1;
        }
        assert_eq!(n, 5);
    }

    /// R2: Select All After Cursor takes events starting at or after the cursor; Select Events to End stays on the
    /// event's track.
    #[test]
    fn select_after_and_events_to_end() {
        let (mut p, ids) = three_events(); // V1 at 1, V2 at 4, A1 at 2
        assert_eq!(select_after(&p, 2.0), [ids[1].clone(), ids[2].clone()]);
        assert_eq!(select_after(&p, 0.0).len(), 3);
        p.timeline.video_tracks[0].clips.push(clip(5.0, 1.0, 0.0));
        let late = p.timeline.video_tracks[0].clips[1].id.clone();
        assert_eq!(events_to_end(&p, &ids[0]), [ids[0].clone(), late.clone()]);
        assert_eq!(events_to_end(&p, &late), [late]);
    }

    /// R2: time selections from selected events, and Ctrl+Shift+Right growing the selection.
    #[test]
    fn selection_from_events_and_markers() {
        let (p, ids) = three_events();
        assert_eq!(selected_event_span(&p, &ids, 0.0, true), Some((1.0, 3.0)));
        assert_eq!(selected_event_span(&p, &ids, 1.0, true), Some((2.0, 4.0)));
        assert_eq!(selected_event_span(&p, &ids, 3.0, false), Some((2.0, 4.0)));
        assert_eq!(selected_event_span(&p, &ids, 1.0, false), None);
        assert_eq!(extend_selection(None, 2.0, 5.0), (2.0, 5.0));
        assert_eq!(extend_selection(Some((2.0, 5.0)), 5.0, 8.0), (2.0, 8.0));
        assert_eq!(extend_selection(Some((2.0, 5.0)), 3.0, 8.0), (3.0, 8.0));
    }

    /// R2: [ and ] walk the edges of a track in time order; the nudge keys trim the selected edge.
    #[test]
    fn edge_walk_and_trim() {
        let mut p = Project::new("t");
        p.timeline.video_tracks[0].clips = vec![clip(0.0, 2.0, 1.0), clip(2.0, 2.0, 0.0)];
        let (a, b) = (p.timeline.video_tracks[0].clips[0].id.clone(), p.timeline.video_tracks[0].clips[1].id.clone());
        assert_eq!(next_edge(&p, &a, true, true), Some((b.clone(), false, 2.0)), "at a cut the incoming start is next");
        assert_eq!(next_edge(&p, &b, false, false), Some((a.clone(), true, 2.0)));
        assert_eq!(next_edge(&p, &a, false, false), None);
        assert_eq!(next_edge(&p, &b, true, true), None);
        let f = 1.0 / 30.0;
        p.settings.frame_rate = 30.0;
        assert!(trim_edge(&mut p, &a, false, f));
        let c = p.clip(&a).unwrap();
        assert!((c.position - f).abs() < 1e-9 && (c.in_point - 1.0 - f).abs() < 1e-9 && (c.duration - 2.0 + f).abs() < 1e-9);
        assert!(trim_edge(&mut p, &a, false, -2.0 * f), "not before 0");
        assert_eq!(p.clip(&a).unwrap().position, 0.0);
        assert!(!trim_edge(&mut p, &a, false, -f));
        assert!(trim_edge(&mut p, &b, true, f));
        assert!((p.clip(&b).unwrap().duration - (2.0 + f)).abs() < 1e-9);
        p.timeline.video_tracks[0].clips[1].locked = true;
        assert!(!trim_edge(&mut p, &b, true, f));
    }

    /// R2: exclusive mute leaves only the selected tracks muted; again, it clears them.
    #[test]
    fn exclusive_mute_and_solo() {
        let (mut p, _) = three_events();
        let tl = &mut p.timeline;
        (tl.video_tracks[1].muted, tl.audio_tracks[0].solo) = (true, true);
        let v1 = vec![p.timeline.video_tracks[0].id.clone()];
        assert!(exclusive_tracks(&mut p, &v1, false));
        let muted = |p: &Project| p.timeline.video_tracks.iter().chain(&p.timeline.audio_tracks).map(|t| t.muted).collect::<Vec<_>>();
        assert_eq!(muted(&p), [true, false, false]);
        assert!(p.timeline.audio_tracks[0].solo, "mute leaves solo alone");
        assert!(exclusive_tracks(&mut p, &v1, false));
        assert_eq!(muted(&p), [false, false, false]);
        assert!(exclusive_tracks(&mut p, &v1, true));
        assert!(p.timeline.video_tracks[0].solo && !p.timeline.audio_tracks[0].solo);
    }

    /// R3b: each switch acts on events of its kind and follows the focus event; Normalize turned on is measured again;
    /// Stream ▸ only picks streams the media has; locked events keep theirs.
    #[test]
    fn event_switches_channels_and_streams() {
        let (mut p, ids) = three_events(); // video, video, audio (focus)
        assert!(set_switch(&mut p, &ids, Switch::Invert));
        let tl = &p.timeline;
        assert!(tl.audio_tracks[0].clips[0].invert && !tl.video_tracks[0].clips[0].invert);
        assert!(set_switch(&mut p, &ids, Switch::KeepAspect)); // the focus video event keeps aspect: all turn it off
        assert!(p.timeline.video_tracks.iter().all(|t| !t.clips[0].keep_aspect));
        p.timeline.video_tracks[1].clips[0].locked = true;
        assert!(set_switch(&mut p, &ids, Switch::ReduceFlicker));
        assert!(p.timeline.video_tracks[0].clips[0].reduce_flicker && !p.timeline.video_tracks[1].clips[0].reduce_flicker);
        assert!(set_resample(&mut p, &ids, Some(Resample::Force)) && !set_resample(&mut p, &ids, Some(Resample::Force)));
        assert!(set_channels(&mut p, &ids, ChannelMap::Swap));
        assert_eq!(p.timeline.audio_tracks[0].clips[0].channels, ChannelMap::Swap);
        // Normalize: measured once, kept through a trim; Auto Normalize measures the new span again.
        let a = &mut p.timeline.audio_tracks[0].clips[0];
        a.media_id = Some("m".into());
        a.normalized = Some(Normalized { db: 3.0, span: norm_span(a) });
        assert!(!needs_normalize(a) && normalize_gain(a) == 0.0, "off: not measured, not applied");
        assert!(set_switch(&mut p, &ids, Switch::Normalize));
        let a = &mut p.timeline.audio_tracks[0].clips[0];
        assert!(a.normalized.is_none() && needs_normalize(a), "turned on: measured again");
        a.normalized = Some(Normalized { db: 3.0, span: norm_span(a) });
        a.duration = 1.0;
        assert!(!needs_normalize(a) && normalize_gain(a) == 3.0);
        a.auto_normalize = true;
        assert!(needs_normalize(a));
        assert_eq!(normalize_db(1.0), NORMALIZE_PEAK_DB);
        assert!((crate::engine::keyframes::db_to_gain(normalize_db(0.5)) * 0.5 - 0.98855).abs() < 1e-4);
        // Stream ▸: the media has two audio streams.
        p.media_pool.push(MediaItem { id: "m".into(), audio_streams: 2, ..Default::default() });
        assert!(!set_stream(&mut p, &ids, false, 2) && set_stream(&mut p, &ids, false, 1));
        assert_eq!(p.timeline.audio_tracks[0].clips[0].stream, 1);
        assert!(!set_stream(&mut p, &ids, true, 1), "the video events have no media with a second stream");
    }

    /// R8a: the bus count adds Bus A, B... and removes the last ones, rerouting what went to them to the master; chains
    /// are named FX 1, FX 2... and removed with their sends; Output and Pan Type apply to the given tracks.
    #[test]
    fn busses_chains_and_routing() {
        let mut p = Project::new("t");
        p.timeline.add_audio_track();
        assert!(set_bus_count(&mut p, 3) && !set_bus_count(&mut p, 3));
        let names: Vec<&str> = p.timeline.audio.buses.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, ["Bus A", "Bus B", "Bus C"]);
        assert!(insert_bus(&mut p) && p.timeline.audio.buses[3].name == "Bus D");
        set_bus_count(&mut p, 40);
        assert_eq!(p.timeline.audio.buses.len(), 26);
        assert_eq!(p.timeline.audio.buses[25].name, "Bus Z");
        assert!(!insert_bus(&mut p), "26 at most");
        let (c, ids) = (p.timeline.audio.buses[2].id.clone(), vec![p.timeline.audio_tracks[0].id.clone()]);
        assert!(set_track_out(&mut p, &ids, &Out::Bus(c.clone())) && !set_track_out(&mut p, &ids, &Out::Bus(c.clone())));
        assert!(!set_track_out(&mut p, &ids, &Out::Bus("gone".into())));
        let d = p.timeline.audio.buses[3].id.clone();
        assert_eq!(set_bus_out(&mut p, &d, &Out::Bus(c.clone())), Ok(true));
        let t = &mut p.timeline.audio_tracks[0];
        set_send(t, &c, 0.5, true);
        t.send_view = Some(c.clone());
        // Two busses: C and D go; the track and D's output go to the master, the send to C goes.
        assert!(set_bus_count(&mut p, 2));
        let t = &p.timeline.audio_tracks[0];
        assert!(t.out == Out::Master && t.sends.is_empty() && t.send_view.is_none());
        // Assignable FX: FX 1, FX 2; FX 1 removed with its sends; the free name is used again.
        let f1 = insert_assignable_fx(&mut p, Vec::new()).unwrap();
        let f2 = insert_assignable_fx(&mut p, Vec::new()).unwrap();
        assert_eq!(p.timeline.audio.bus(&f2).unwrap().name, "FX 2");
        let pre = send_pre_default(&p.timeline, &f1);
        set_send(&mut p.timeline.audio_tracks[1], &f1, 1.0, pre);
        assert!(!p.timeline.audio_tracks[1].sends[0].pre, "an FX send is post-volume, as in VEGAS");
        assert!(send_pre_default(&p.timeline, &p.timeline.audio.buses[0].id.clone()), "a bus send is pre-volume");
        assert!(remove_assignable_fx(&mut p, std::slice::from_ref(&f1)) && p.timeline.audio_tracks[1].sends.is_empty());
        let f3 = insert_assignable_fx(&mut p, Vec::new()).unwrap();
        assert_eq!(p.timeline.audio.bus(&f3).unwrap().name, "FX 1");
        for _ in 0..40 {
            insert_assignable_fx(&mut p, Vec::new());
        }
        assert_eq!(p.timeline.audio.fx.len(), MAX_FX_CHAINS);
        // Bus FX are edited as a chain.
        let t = FxTarget::Bus(f2.clone());
        assert!(fx_is_audio(&p, &t) && add_fx(&mut p, &t, Fx { id: "x".into(), plugin: "milan.afx.eq".into(), enabled: true, params: Default::default(), text: Default::default(), auto: Default::default() }));
        assert_eq!(fx_list(&p, &t).unwrap().0.len(), 1);
        // Pan Type on the tracks given.
        let all: Vec<String> = p.timeline.audio_tracks.iter().map(|t| t.id.clone()).collect();
        assert!(set_pan_type(&mut p, &all, PanType::Film) && !set_pan_type(&mut p, &all, PanType::Film));
        assert!(p.timeline.audio_tracks.iter().all(|t| t.pan_type == PanType::Film));
    }

    /// R8c: an audio track's FX envelope is added holding the parameter's value, lent out 0..=1 of its range and
    /// written back in its units, rides a ripple, and goes again; Automation and event envelopes on the ids given.
    #[test]
    fn fx_envelopes_automation_and_event_envelopes() {
        let mut p = Project::new("t");
        let mut c = Clip::new("c", 1.0, 4.0);
        c.media_id = Some("m".into());
        let cid = c.id.clone();
        p.timeline.audio_tracks[0].clips.push(c);
        let tr = &p.timeline.audio_tracks[0];
        let (tid, comp) = (tr.id.clone(), tr.fx.iter().find(|f| f.plugin == "milan.afx.compressor").unwrap().id.clone());
        let owner = fx_owner(&tid, &comp, "threshold");
        assert_eq!(fx_owner_parts(&owner), Some((tid.as_str(), comp.as_str(), "threshold")));
        assert_eq!(owner_base(&owner), tid);
        assert!(envelope_of(&p, &owner, EnvKind::FxParam).is_none());
        assert!(toggle_fx_envelope(&mut p, &tid, &comp, "threshold"));
        // -12 dB of -60..0 is 0.8 of the lane.
        let (e, start, scale) = envelope_of(&p, &owner, EnvKind::FxParam).unwrap();
        assert!((e.points.keys[0].v - 0.8).abs() < 1e-6 && (start, scale) == (0.0, 1.0));
        assert!(edit_envelope(&mut p, &owner, EnvKind::FxParam, |e| {
            env_insert(e, 3.0, 0.5);
            true
        }));
        let f = |p: &Project| p.timeline.audio_tracks[0].fx.iter().find(|f| f.id == comp).unwrap().clone();
        assert!((f(&p).auto["threshold"].keys[1].v - -30.0).abs() < 1e-4, "stored in dB");
        assert_eq!(fx_envelopes(&p, &tid, false), [owner.clone()]);
        assert_eq!(fx_envelope_name(&p, &owner).as_deref(), Some("Track Compressor: Threshold (dB)"));
        assert_eq!(fx_value_text(&p, &owner, 0.5).as_deref(), Some("Threshold (dB) -30.00"));
        // A ripple moves the FX automation with the track's envelopes.
        let op = RippleOp { from: 2.0, delta: 1.0, tracks: vec![tid.clone()], exclude: Vec::new(), keys_from: None };
        assert!(ripple(&mut p, &op, RippleMode::Affected));
        assert_eq!(f(&p).auto["threshold"].keys[1].t, 4.0);
        assert!(toggle_fx_envelope(&mut p, &tid, &comp, "threshold") && f(&p).auto.is_empty());
        // A locked track takes no new FX envelope; a video track's are its keys (not toggled here).
        p.timeline.audio_tracks[0].locked = true;
        assert!(!toggle_fx_envelope(&mut p, &tid, &comp, "threshold") && !ensure_envelope(&mut p, &tid, EnvKind::Volume));
        p.timeline.audio_tracks[0].locked = false;
        // Automation ▸ and Show Automation Controls on tracks and busses.
        set_bus_count(&mut p, 1);
        let bid = p.timeline.audio.buses[0].id.clone();
        assert!(set_automation(&mut p, &[tid.clone(), bid.clone()], Some(AutoMode::Latch), Some(true)));
        assert_eq!((automation_of(&p, &tid), automation_of(&p, &bid)), (Some((AutoMode::Latch, true)), Some((AutoMode::Latch, true))));
        assert!(!set_automation(&mut p, &[tid.clone()], Some(AutoMode::Latch), None), "no change");
        assert!(automation_of(&p, &p.timeline.video_tracks[0].id.clone()).is_none());
        // A write adds the envelope it needs: the track's, the bus's.
        assert!(ensure_envelope(&mut p, &tid, EnvKind::Volume) && ensure_envelope(&mut p, &bid, EnvKind::Pan));
        assert!(envelope_of(&p, &bid, EnvKind::Pan).is_some() && with_envelope(&mut p, &bid, EnvKind::Pan, |e| env_set_values(e, &[0], 0.5)));
        // Audio event envelopes: on, then off with the focus event.
        assert!(toggle_event_envelope(&mut p, std::slice::from_ref(&cid), EnvKind::Pan));
        assert_eq!(envelope_of(&p, &cid, EnvKind::Pan).map(|e| e.1), Some(1.0));
        assert!(toggle_event_envelope(&mut p, &[cid], EnvKind::Pan) && p.timeline.audio_tracks[0].clips[0].envelopes.is_empty());
    }

    /// R8c: an automation write keeps the envelope outside the pass and thins what it writes.
    #[test]
    fn env_written_keeps_the_rest_and_write_point_thins() {
        let key = |t, v| Keyframe { t, v, curve: Curve::Linear };
        let orig = Envelope { kind: EnvKind::Volume, points: Keyframes { keys: vec![key(0.0, 0.0), key(10.0, -10.0)] } };
        let mut pts = Vec::new();
        for k in 0..=100 {
            write_point(EnvKind::Volume, &mut pts, 2.0 + k as f64 * 0.001, -20.0 + k as f32 * 0.1);
        }
        assert_eq!(pts.len(), 6, "a 10 dB move in 100 ms: a point every 20 ms: {pts:?}");
        let e = env_written(&orig, 2.0, &pts, 2.1, -10.0);
        assert_eq!((e.at(1.0), e.at(2.05)), (-1.0, -15.0));
        assert!((e.at(6.0) - -6.0).abs() < 1e-5 && e.points.keys.last().unwrap().t == 10.0);
        assert!(e.points.keys.iter().all(|k| k.t <= 2.1 + 1e-9 || k.t == 10.0));
    }

    /// R17a: two cameras and a recorder: camera A's grouped pair on the top tracks, camera B's pair, a locked recorder.
    fn sync_project() -> (Project, [String; 6]) {
        let mut p = Project::new("t");
        p.timeline.add_track(true);
        p.timeline.add_track(false);
        p.timeline.add_track(false);
        let ev = |name: &str, at: f64, len: f64, group: Option<&str>| {
            let mut c = Clip::new(name, at, len);
            c.group = group.map(String::from);
            c
        };
        let (v1, a1, v2, a2, x, a3) = (ev("v1", 0.0, 10.0, Some("g1")), ev("a1", 0.0, 10.0, Some("g1")), ev("v2", 5.0, 10.0, Some("g2")), ev("a2", 5.0, 10.0, Some("g2")), ev("x", 20.0, 2.0, None), ev("a3", 1.0, 4.0, None));
        let ids = [&v1, &a1, &v2, &a2, &x, &a3].map(|c| c.id.clone());
        let tl = &mut p.timeline;
        (tl.video_tracks[0].clips, tl.video_tracks[1].clips) = (vec![v1], vec![v2, x]);
        (tl.audio_tracks[0].clips, tl.audio_tracks[1].clips, tl.audio_tracks[2].clips) = (vec![a1], vec![a2], vec![a3]);
        tl.audio_tracks[2].clips[0].locked = true;
        (p, ids)
    }

    /// R17a: the selection as Synchronize units (group mates move along, the topmost track's stay), the plan (locked
    /// units stay, none goes before 0) and the move: one undo step, rippled as Auto Ripple says, never rippling a
    /// synchronized event.
    #[test]
    fn synchronize_respects_groups_locks_and_ripple_in_one_undo_step() {
        let (p, [v1, a1, v2, a2, x, a3]) = sync_project();
        let sound = |_: &Clip, video: bool| !video;
        let sel = [v1.clone(), a2.clone(), a3.clone()];
        let units = sync_units(&p, &sel, false, sound);
        let want = |ids: &[&String], key: &String, reference| SyncUnit { ids: ids.iter().map(|s| s.to_string()).collect(), key: Some(key.clone()), reference };
        assert_eq!(units, [want(&[&a1, &v1], &a1, true), want(&[&v2, &a2], &a2, false), want(&[&a3], &a3, false)], "a selected picture is matched by its sound");
        // Ignore Event Grouping: only selected events move; the picture's unselected sound is still what is read.
        let loose = sync_units(&p, &sel, true, sound);
        assert_eq!(loose, [want(&[&v1], &a1, true), want(&[&a2], &a2, false), want(&[&a3], &a3, false)]);

        let plan = plan_sync(&p, &units, vec![Ok(-3.0), Ok(2.0)]);
        assert_eq!((plan.moves.clone(), plan.aligned), (vec![(vec![v2.clone(), a2.clone()], -3.0)], 1));
        assert_eq!(plan.refused, ["\"a3\" (locked)"]);
        let mut h = crate::history::History::default();
        let mut q = p.clone();
        let ops = apply_sync(&mut q, &units, &plan.moves, None, false, false).unwrap();
        h.push(p.clone());
        let at = |p: &Project, id: &str| p.clip(id).unwrap().position;
        assert_eq!([at(&q, &v2), at(&q, &a2), at(&q, &x), at(&q, &v1), at(&q, &a3)], [2.0, 2.0, 20.0, 0.0, 1.0]);
        assert_eq!(ops.len(), 1);
        assert!(ops[0].from == 5.0 && ops[0].delta == -3.0 && ops[0].exclude.contains(&a1), "kept for Post-Edit Ripple: {ops:?}");
        assert!(q.modified && h.lens() == (1, 0));
        h.undo(&mut q);
        assert_eq!([at(&q, &v2), at(&q, &a2)], [5.0, 5.0], "one step undoes it all");
        // Auto Ripple (Affected Tracks): what followed on camera B's tracks closes up with it; nothing else moves.
        let mut q = p.clone();
        apply_sync(&mut q, &units, &plan.moves, Some(RippleMode::Affected), false, false).unwrap();
        assert_eq!([at(&q, &v2), at(&q, &x), at(&q, &v1), at(&q, &a3)], [2.0, 17.0, 0.0, 1.0]);
        // Ripple All moves every track but never a synchronized event.
        let mut q = p.clone();
        apply_sync(&mut q, &units, &plan.moves, Some(RippleMode::All), false, false).unwrap();
        assert_eq!([at(&q, &v2), at(&q, &x), at(&q, &v1), at(&q, &a1)], [2.0, 17.0, 0.0, 0.0]);

        // Camera B belongs 1 s before the timeline's start: everything moves 1 s later instead, the reference too.
        let plan = plan_sync(&p, &units, vec![Ok(-6.0), Err("no confident audio match".into())]);
        assert_eq!(plan.moves, [(vec![v2.clone(), a2.clone()], -5.0), (vec![a1.clone(), v1.clone()], 1.0)]);
        assert_eq!((plan.aligned, plan.refused.clone()), (1, vec!["\"a3\" (no confident audio match)".to_string()]));
        let mut q = p.clone();
        apply_sync(&mut q, &units, &plan.moves, None, false, false).unwrap();
        assert_eq!([at(&q, &v2), at(&q, &v1)], [0.0, 1.0]);
        // ...unless the reference is locked: then it is refused.
        let mut locked = p.clone();
        locked.timeline.video_tracks[0].locked = true;
        let plan = plan_sync(&locked, &units, vec![Ok(-6.0), Ok(0.0)]);
        assert!(plan.moves.is_empty() && plan.aligned == 0, "{plan:?}");
        assert_eq!(plan.refused.len(), 2);
        // Nothing to move: no edit.
        assert!(apply_sync(&mut p.clone(), &units, &[], None, false, false).is_none());
    }

    /// R17a acceptance: three clips with known start timecodes laid out on tracks of their own, lined up by timecode,
    /// the earliest at the cursor; a still and a clip without timecode are skipped.
    #[test]
    fn lay_out_tracks_by_timecode() {
        let mut p = media_project();
        let tc = |s: &str| crate::engine::sync::parse_timecode(s, 25.0);
        p.media_pool[0].timecode = tc("01:00:02:00");
        p.media_pool[2].timecode = tc("00:59:59:12");
        let w = MediaItem { id: "w".into(), path: "w".into(), name: "w".into(), media_type: MediaType::Video, duration: 3.0, timecode: tc("01:00:10:06"), ..Default::default() };
        let none = MediaItem { id: "n".into(), path: "n".into(), name: "n".into(), media_type: MediaType::Video, duration: 3.0, ..Default::default() };
        p.media_pool.extend([w, none]);
        let (ids, skipped) = lay_out_tracks(&mut p, &[0, 1, 2, 3, 4], 1.0, |m| m.timecode);
        assert_eq!((ids.len(), skipped), (4, 2));
        let close = |got: Vec<(usize, f64, f64, String)>, want: &[(usize, f64, &str)]| {
            assert_eq!(got.len(), want.len(), "{got:?}");
            for (g, w) in got.iter().zip(want) {
                assert!(g.0 == w.0 && (g.1 - w.1).abs() < 1e-9 && g.3 == w.2, "{got:?}");
            }
        };
        // v (01:00:02:00) 2.52 s after a (00:59:59:12), w (01:00:10:06) 10.76 s after; each on new tracks.
        close(laid_out(&p, true), &[(1, 3.52, "v"), (2, 11.76, "w")]);
        close(laid_out(&p, false), &[(1, 3.52, "v"), (2, 1.0, "a")]);
        assert!(p.modified);
        let (ids, skipped) = lay_out_tracks(&mut p, &[4], 0.0, |m| m.timecode);
        assert!(ids.is_empty() && skipped == 1);
    }

    /// R17b: three cameras on Video 1-3 (a 0..6 s, b 1..8 s, c 2..6 s, each from 10 s in) and their sound on Audio
    /// 1-2 (a and b).
    fn cameras() -> (Project, Vec<String>) {
        let mut p = Project::new("mc");
        let media = |id: &str, video: bool| MediaItem {
            id: id.into(),
            path: format!("{id}.mp4").into(),
            name: id.into(),
            media_type: if video { MediaType::Video } else { MediaType::Audio },
            duration: 60.0,
            sample_rate: Some(48_000),
            ..Default::default()
        };
        let ev = |id: &str, pos: f64, end: f64| Clip { in_point: 10.0, duration: end - pos, ..Clip::from_media(&media(id, true), pos) };
        p.timeline.video_tracks = (1..=3).map(|n| Track::new(&format!("Video {n}"), TrackKind::Video)).collect();
        p.timeline.audio_tracks = (1..=2).map(|n| Track::new(&format!("Audio {n}"), TrackKind::Audio)).collect();
        for (k, (id, a, b)) in [("a", 0.0, 6.0), ("b", 1.0, 8.0), ("c", 2.0, 6.0)].into_iter().enumerate() {
            p.timeline.video_tracks[k].clips.push(ev(id, a, b));
            if k < 2 {
                p.timeline.audio_tracks[k].clips.push(ev(id, a, b));
            }
            p.media_pool.push(media(id, true));
        }
        let ids = p.timeline.video_tracks.iter().chain(&p.timeline.audio_tracks).flat_map(|t| t.clips.iter().map(|c| c.id.clone())).collect();
        (p, ids)
    }

    /// (position, end, active take, each take's (media, in point)) of a track's events.
    fn takes_of(t: &Track) -> Vec<(f64, f64, usize, Vec<(String, f64)>)> {
        t.clips.iter().map(|c| {
            let (list, k) = take_list(c);
            (c.position, c.position + c.duration, k, list.into_iter().map(|t| (t.media_id, t.in_point)).collect())
        }).collect()
    }

    #[test]
    fn create_multicamera_track_makes_takes_of_the_tracks() {
        let (mut p, ids) = cameras();
        let made = create_multicam(&mut p, &ids).unwrap();
        // The camera tracks left empty go; Video 1 and Audio 1 hold the cameras as takes, split at every edge.
        assert_eq!((p.timeline.video_tracks.len(), p.timeline.audio_tracks.len(), made.len()), (1, 1, 4 + 3));
        let s = |m: &str, i: f64| (m.to_string(), i);
        let none = (String::new(), 0.0);
        assert_eq!(takes_of(&p.timeline.video_tracks[0]), vec![
            (0.0, 1.0, 0, vec![s("a", 10.0), none.clone(), none.clone()]),
            (1.0, 2.0, 0, vec![s("a", 11.0), s("b", 10.0), none.clone()]),
            (2.0, 6.0, 0, vec![s("a", 12.0), s("b", 11.0), s("c", 10.0)]),
            // Take 1 has nothing here: the first camera that has something plays.
            (6.0, 8.0, 1, vec![none.clone(), s("b", 15.0), none.clone()]),
        ]);
        assert_eq!(takes_of(&p.timeline.audio_tracks[0]).len(), 3);
        assert!(p.modified && p.timeline.video_tracks[0].clips.iter().all(|c| c.group.is_none()));
        // One track selected, a locked event or a locked camera track: nothing changes.
        let (mut q, ids) = cameras();
        assert!(create_multicam(&mut q, &ids[..1]).is_err());
        q.timeline.audio_tracks[1].locked = true;
        let before = serde_json::to_string(&q).unwrap();
        assert!(create_multicam(&mut q, &ids).unwrap_err().contains("Audio 2"));
        assert_eq!(serde_json::to_string(&q).unwrap(), before, "the video made first is put back");
    }

    /// Acceptance: pressing 2 at 3.0 s splits the multicamera event there and plays take 2 from 3.0 s.
    #[test]
    fn choosing_take_2_at_3_s_cuts_there() {
        let (mut p, ids) = cameras();
        create_multicam(&mut p, &ids).unwrap();
        assert!(switch_take(&mut p, true, 3.0, None, 1));
        let v = takes_of(&p.timeline.video_tracks[0]);
        assert_eq!(v[2..4].iter().map(|e| (e.0, e.1, e.2)).collect::<Vec<_>>(), [(2.0, 3.0, 0), (3.0, 6.0, 1)]);
        // Take 2's source runs on across the cut; the audio stays, as Edit Multicamera Audio with Video is off.
        assert_eq!(v[3].3[1], ("b".to_string(), 12.0));
        assert_eq!(takes_of(&p.timeline.audio_tracks[0]).len(), 3);
        // The same take again, an empty take or no such take: nothing changes.
        assert!(!switch_take(&mut p, true, 4.0, None, 1) && !switch_take(&mut p, true, 0.5, None, 2) && !switch_take(&mut p, true, 3.5, None, 7));
        // Back to take 1 at 3.0 s: the two halves are one event again.
        assert!(switch_take(&mut p, true, 3.0, None, 0));
        assert_eq!(takes_of(&p.timeline.video_tracks[0])[2].1, 6.0);
        // The audio's takes, as Edit Multicamera Audio with Video switches them.
        assert!(switch_take(&mut p, false, 3.0, None, 1));
        let a = &takes_of(&p.timeline.audio_tracks[0])[2];
        assert_eq!((a.0, a.1, a.2), (3.0, 6.0, 1));
    }

    /// A time selection gets the take in every multicamera event in it that has it, cut at its edges.
    #[test]
    fn a_take_over_a_time_selection() {
        let (mut p, ids) = cameras();
        create_multicam(&mut p, &ids).unwrap();
        assert!(switch_take(&mut p, true, 0.0, Some((0.5, 4.0)), 1));
        let v: Vec<(f64, f64, usize)> = takes_of(&p.timeline.video_tracks[0]).into_iter().map(|e| (e.0, e.1, e.2)).collect();
        // 0.5..1 has no take 2 and keeps take 1 (joined back up); 1..2 and 2..4 take it; the rest is as it was.
        assert_eq!(v, [(0.0, 1.0, 0), (1.0, 2.0, 1), (2.0, 4.0, 1), (4.0, 6.0, 0), (6.0, 8.0, 1)]);
        // The track locked: refused.
        p.timeline.video_tracks[0].locked = true;
        assert!(!switch_take(&mut p, true, 5.0, None, 2));
    }

    /// The fixed take order stays as takes are cycled, chosen, deleted or lose their media.
    #[test]
    fn take_numbers_stay_with_their_takes() {
        let (mut p, _) = take_project(); // active v, take a
        let id = p.timeline.audio_tracks[0].clips[0].id.clone();
        p.timeline.audio_tracks[0].clips[0].takes.push(Take { media_id: "i".into(), in_point: 0.0, name: "i".into() });
        let order = |p: &Project| {
            let (l, k) = take_list(p.clip(&id).unwrap());
            (l.into_iter().map(|t| t.name).collect::<Vec<_>>(), k)
        };
        let names = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(order(&p), (names(&["v", "a", "i"]), 0));
        next_take(&mut p, std::slice::from_ref(&id), true);
        assert_eq!(order(&p), (names(&["v", "a", "i"]), 1));
        next_take(&mut p, std::slice::from_ref(&id), false);
        next_take(&mut p, std::slice::from_ref(&id), false);
        assert_eq!(order(&p), (names(&["v", "a", "i"]), 2));
        assert!(take_chooser(&mut p, &id, 1, true)); // deletes v, one step on from i
        assert_eq!(order(&p), (names(&["a", "i"]), 1));
        let mut c = p.clip(&id).unwrap().clone();
        assert!(set_take(&mut c, 0) && c.name == "a" && !set_take(&mut c, 0) && !set_take(&mut c, 2));
        forget_media(&mut p.timeline, "i"); // the active take goes: the next one plays
        assert_eq!(order(&p), (names(&["a"]), 0));
    }

    /// Expand to Multiple Tracks without the unused takes: only what plays is kept, each take on its own track.
    #[test]
    fn expand_without_the_unused_takes() {
        let (mut p, ids) = cameras();
        create_multicam(&mut p, &ids).unwrap();
        switch_take(&mut p, true, 3.0, None, 2);
        let id = p.timeline.video_tracks[0].id.clone();
        assert!(expand_multicam(&mut p, std::slice::from_ref(&id), false));
        let tl = &p.timeline;
        assert_eq!(tl.video_tracks.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["Video 1", "b", "c"]);
        let spans = |t: &Track| t.clips.iter().map(|c| (c.position, c.position + c.duration, c.muted, c.takes.len())).collect::<Vec<_>>();
        assert_eq!(spans(&tl.video_tracks[0]), [(0.0, 1.0, false, 0), (1.0, 2.0, false, 0), (2.0, 3.0, false, 0)]);
        assert_eq!(spans(&tl.video_tracks[1]), [(6.0, 8.0, false, 0)]);
        assert_eq!(spans(&tl.video_tracks[2]), [(3.0, 6.0, false, 0)]);
        assert_eq!(tl.video_tracks[2].clips[0].in_point, 11.0);
        // No multicamera event left: nothing to expand.
        assert!(!expand_multicam(&mut p, &[id], true));
    }

    /// R18a: Smart Split cuts the selection out of the event, the rest overlapping by five frames with a crossfade, the
    /// parts grouped; what follows ripples (one undo step, as the caller pushes one), locked events stay.
    #[test]
    fn smart_split_cuts_the_selection_out_and_overlaps_five_frames() {
        let mut p = Project::new("t");
        let (c, d) = (Clip::new("c", 0.0, 10.0), Clip::new("d", 10.0, 2.0));
        let (id, next) = (c.id.clone(), d.id.clone());
        p.timeline.video_tracks[0].clips = vec![c, d];
        let o = 5.0 / 25.0;
        assert!(smart_split(&mut p, &[id.clone()], (0.1, 5.0), o).is_none(), "the first part would be shorter than the overlap");
        let op = smart_split(&mut p, &[id.clone()], (3.0, 5.0), o).unwrap();
        let cl = &p.timeline.video_tracks[0].clips;
        let (first, second) = (&cl[0], &cl[1]);
        assert_eq!((first.id.as_str(), first.duration, first.fade_out), (id.as_str(), 3.0, o));
        assert!((second.position - (3.0 - o)).abs() < 1e-12 && second.duration == 5.0 && second.in_point == 5.0 && second.fade_in == o);
        assert!(first.group.is_some() && first.group == second.group);
        assert_eq!((op.from, op.delta, op.exclude.clone()), (5.0, -(2.0 + o), vec![second.id.clone()]));
        assert!(ripple(&mut p, &op, RippleMode::Affected));
        let d = p.clip(&next).unwrap();
        assert!((d.position - (10.0 - 2.0 - o)).abs() < 1e-12);
        // A locked event is left alone.
        let mut q = Project::new("t");
        let mut c = Clip::new("c", 0.0, 10.0);
        c.locked = true;
        let id = c.id.clone();
        q.timeline.video_tracks[0].clips = vec![c];
        assert!(smart_split(&mut q, &[id], (3.0, 5.0), o).is_none() && !q.modified);
    }

    /// R18a: Insert › Command, Audio CD Track Region and Index keep their lists in time order and refuse no-ops.
    #[test]
    fn command_and_cd_markers_insert_in_order() {
        let mut p = Project::new("t");
        let m = |at: f64, c: &str| CommandMarker { position: at, command: c.into(), ..Default::default() };
        assert!(set_command(&mut p, None, m(5.0, "URL")) && set_command(&mut p, None, m(2.0, "Text")));
        assert_eq!(p.timeline.commands.iter().map(|c| c.command.as_str()).collect::<Vec<_>>(), ["Text", "URL"]);
        assert!(!set_command(&mut p, Some(0), m(2.0, "Text")) && !set_command(&mut p, Some(9), m(1.0, "X")));
        assert!(set_command(&mut p, Some(0), m(7.0, "Text")));
        assert_eq!(p.timeline.commands[1].position, 7.0);
        assert!(insert_cd_track(&mut p, (4.0, 8.0)) && insert_cd_track(&mut p, (0.0, 4.0)) && !insert_cd_track(&mut p, (0.0, 4.0)) && !insert_cd_track(&mut p, (3.0, 3.0)));
        assert_eq!(p.timeline.cd_tracks[0].start, 0.0);
        assert!(insert_cd_index(&mut p, 6.0) && insert_cd_index(&mut p, 5.0) && !insert_cd_index(&mut p, 5.0));
        assert_eq!(p.timeline.cd_indexes, [5.0, 6.0]);
        assert!(p.timeline.cd_layout());
    }

    /// R18a: command markers, CD indexes and CD track regions ripple with the markers and regions.
    #[test]
    fn cd_and_command_markers_ripple_with_markers() {
        let mut p = Project::new("t");
        p.timeline.commands.push(CommandMarker { position: 4.0, ..Default::default() });
        p.timeline.cd_indexes.push(5.0);
        p.timeline.cd_tracks.push(CdTrack { start: 1.0, end: 6.0, ..Default::default() });
        let op = RippleOp { from: 3.0, delta: 2.0, tracks: Vec::new(), ..Default::default() };
        assert!(ripple(&mut p, &op, RippleMode::AffectedMarkersOnly));
        let tl = &p.timeline;
        assert_eq!((tl.commands[0].position, tl.cd_indexes[0], tl.cd_tracks[0].start, tl.cd_tracks[0].end), (6.0, 7.0, 1.0, 8.0));
        assert!(snap_targets(&p, None).contains(&7.0));
    }

    /// R18b acceptance: Insert Time of 2 s at 3 s moves what starts after the cursor, the markers and regions after it
    /// (a region across it stretches) and the time selection; an event across it and locked events stay. With tracks
    /// selected, only theirs move (the markers still do).
    #[test]
    fn insert_time_moves_markers_and_regions_after_the_cursor() {
        let mut p = Project::new("t");
        let (a, b, mut l) = (Clip::new("a", 1.0, 4.0), Clip::new("b", 6.0, 1.0), Clip::new("l", 8.0, 1.0));
        l.locked = true;
        p.timeline.video_tracks[0].clips = vec![a, b, l];
        p.timeline.audio_tracks[0].clips = vec![Clip::new("s", 4.0, 1.0)];
        p.timeline.markers = vec![Marker { position: 2.0, name: "m1".into(), color: [0; 3] }, Marker { position: 3.5, name: "m2".into(), color: [0; 3] }];
        p.timeline.regions = vec![Region { start: 4.0, end: 5.0, name: "r1".into() }, Region { start: 2.0, end: 4.0, name: "r2".into() }];
        p.timeline.loop_region = Some((3.5, 4.5));
        assert!(!insert_time(&mut p, 3.0, 0.0, &[]));
        assert!(insert_time(&mut p, 3.0, 2.0, &[]));
        let tl = &p.timeline;
        let starts = |t: &Track| t.clips.iter().map(|c| c.position).collect::<Vec<_>>();
        assert_eq!((starts(&tl.video_tracks[0]), starts(&tl.audio_tracks[0])), (vec![1.0, 8.0, 8.0], vec![6.0]));
        assert_eq!(tl.markers.iter().map(|m| m.position).collect::<Vec<_>>(), [2.0, 5.5]);
        assert_eq!(tl.regions.iter().map(|r| (r.start, r.end)).collect::<Vec<_>>(), [(6.0, 7.0), (2.0, 6.0)]);
        assert_eq!(tl.loop_region, Some((5.5, 6.5)));
        // Only the selected video track moves; the markers do too.
        let v = p.timeline.video_tracks[0].id.clone();
        assert!(insert_time(&mut p, 0.5, 1.0, &[v]));
        let tl = &p.timeline;
        assert_eq!((starts(&tl.video_tracks[0]), starts(&tl.audio_tracks[0]), tl.markers[0].position), (vec![2.0, 8.0, 9.0], vec![6.0], 3.0));
    }

    /// R18b: Cut and Copy with a time selection take the parts inside it, of the selected events or of every event; Paste
    /// Insert's split puts an event's second half after what is pasted.
    #[test]
    fn time_selection_cut_copy_and_paste_insert_split() {
        let mut p = Project::new("t");
        let (a, b) = (Clip::new("a", 0.0, 4.0), Clip::new("b", 5.0, 4.0));
        let (ida, idb) = (a.id.clone(), b.id.clone());
        p.timeline.video_tracks[0].clips = vec![a, b];
        p.timeline.audio_tracks[0].clips = vec![Clip::new("s", 0.0, 10.0)];
        // Copy of 3..6 with nothing selected: a's 3..4, b's 5..6 and the sound's 3..6, placed from 3.
        let all = copy_range(&p, &[], (3.0, 6.0));
        let mut got: Vec<(bool, f64, f64, f64)> = all.iter().map(|(v, _, c)| (*v, c.position, c.duration, c.in_point)).collect();
        got.sort_by(|x, y| x.partial_cmp(y).unwrap());
        assert_eq!(got, [(false, 0.0, 3.0, 3.0), (true, 0.0, 1.0, 3.0), (true, 2.0, 1.0, 0.0)]);
        assert_eq!(p.timeline.video_tracks[0].clips.len(), 2, "copying changes nothing");
        // Only b selected: its 5..6, at 2 s from the selection's start.
        let sel = copy_range(&p, &[idb.clone()], (3.0, 6.0));
        assert_eq!(sel.iter().map(|(_, _, c)| (c.position, c.duration)).collect::<Vec<_>>(), [(2.0, 1.0)]);
        assert!(copy_range(&p, &[ida.clone()], (5.0, 6.0)).is_empty());
        // Cut's split: b's 5..6 is the piece inside (b keeps its id), the rest a new event from 6 s.
        let inside = split_range(&mut p, &[idb.clone()], (3.0, 6.0));
        assert_eq!(inside, [idb.clone()]);
        assert_eq!(p.clip(&idb).map(|c| (c.position, c.duration)), Some((5.0, 1.0)));
        assert!(p.timeline.video_tracks[0].clips.iter().any(|c| c.position == 6.0 && c.in_point == 1.0 && c.duration == 3.0));
        // Paste Insert at 2 s splits a there (on its track only); with the ripple the second half follows.
        let v = p.timeline.video_tracks[0].id.clone();
        assert!(split_for_insert(&mut p, 2.0, &[v.clone()], false));
        assert!(ripple(&mut p, &RippleOp { from: 2.0, delta: 1.5, tracks: vec![v], ..Default::default() }, RippleMode::Affected));
        let starts: Vec<(f64, f64)> = p.timeline.video_tracks[0].clips.iter().map(|c| (c.position, c.duration)).collect();
        assert_eq!(starts[..2], [(0.0, 2.0), (3.5, 2.0)]);
        assert_eq!(p.timeline.audio_tracks[0].clips.len(), 1, "the other track is not split");
    }

    /// R18b: Insert › Audio Transition turns a cut into an overlap of its length with the preset's curves, sets them on
    /// an existing crossfade, fades a lone edge, and leaves locked events alone.
    #[test]
    fn audio_transition_makes_a_crossfade_preset() {
        let mut p = Project::new("t");
        p.settings.frame_rate = 25.0;
        let t = p.timeline.audio_tracks[0].id.clone();
        p.timeline.audio_tracks[0].clips = vec![Clip::new("a", 0.0, 4.0), Clip::new("b", 4.0, 4.0)];
        assert!(audio_transition(&mut p, &t, 4.0, 1.0, (Curve::Fast, Curve::Slow)));
        let c = &p.timeline.audio_tracks[0].clips;
        assert!((c[0].position + c[0].duration - c[1].position - 1.0).abs() < 1e-9 && c[0].fade_out == c[1].fade_in);
        assert_eq!((c[0].fade_out_curve, c[1].fade_in_curve), (Curve::Fast, Curve::Slow));
        // On the crossfade now: only the curves change.
        assert!(audio_transition(&mut p, &t, 4.0, 1.0, (Curve::Linear, Curve::Sharp)));
        assert!(!audio_transition(&mut p, &t, 4.0, 1.0, (Curve::Linear, Curve::Sharp)), "nothing left to change");
        // The last event's end: a fade-out over the length.
        let end = p.timeline.audio_tracks[0].clips[1].position + p.timeline.audio_tracks[0].clips[1].duration;
        assert!(audio_transition(&mut p, &t, end - 0.1, 0.5, (Curve::Smooth, Curve::Smooth)));
        assert_eq!(p.timeline.audio_tracks[0].clips[1].fade_out, 0.5);
        p.timeline.audio_tracks[0].clips[0].locked = true;
        assert!(!audio_transition(&mut p, &t, 3.5, 1.0, (Curve::Fast, Curve::Fast)));
        let v = p.timeline.video_tracks[0].id.clone();
        assert!(!audio_transition(&mut p, &v, 0.0, 1.0, (Curve::Fast, Curve::Fast)), "audio tracks only");
    }

    /// R18b: Insert › Slideshow lays the pictures out at a fixed length, each overlapping the one before with the next
    /// transition; a locked track refuses it. Adjustment tracks and events insert; Resize Generated Media matches the
    /// project's size.
    #[test]
    fn slideshow_adjustments_and_generated_resize() {
        let mut p = Project::new("t");
        for (i, (kind, name)) in [(MediaType::Image, "a.png"), (MediaType::Audio, "s.wav"), (MediaType::Image, "b.png"), (MediaType::Image, "c.png")].into_iter().enumerate() {
            p.media_pool.push(MediaItem { id: format!("m{i}"), path: name.into(), name: name.into(), media_type: kind, ..Default::default() });
        }
        let wipe = Transition { plugin: "milan.tr.linear_wipe".into(), ..Default::default() };
        let s = Slideshow { len: 4.0, overlap: 1.0, transitions: vec![wipe.clone()], random: false };
        let ids = insert_slideshow(&mut p, &[0, 1, 2, 3], None, 2.0, &s);
        assert_eq!(ids.len(), 3, "the sound is not a picture");
        let c = &p.timeline.video_tracks[0].clips;
        assert_eq!(c.iter().map(|c| (c.position, c.duration, c.fade_in, c.fade_out)).collect::<Vec<_>>(), [(2.0, 4.0, 0.0, 1.0), (5.0, 4.0, 1.0, 1.0), (8.0, 4.0, 1.0, 0.0)]);
        assert!(c[0].transition.is_none() && c[1].transition == Some(wipe.clone()) && c[2].transition == Some(wipe));
        p.timeline.video_tracks[0].locked = true;
        assert!(insert_slideshow(&mut p, &[0], Some(0), 0.0, &s).is_empty());
        // An adjustment track goes on top; an adjustment event refuses a locked track.
        assert_eq!(insert_adjustment_track(&mut p, None), 0);
        assert!(p.timeline.video_tracks[0].adjustment && p.timeline.video_tracks[0].name == "Adjustment Track 1");
        let id = insert_adjustment_event(&mut p, 0, (1.0, 3.0)).unwrap();
        assert!(p.clip(&id).is_some_and(|c| c.adjustment && c.media_id.is_none() && c.duration == 2.0));
        assert!(insert_adjustment_event(&mut p, 1, (1.0, 3.0)).is_none());
        // Generated media keeps the size it was made at; Resize Generated Media sets the project's.
        p.timeline.video_tracks[1].locked = false;
        let fx = crate::engine::plugins::new_fx(crate::engine::plugins::find("milan.gen.solid").unwrap(), None);
        let g = insert_generated(&mut p, Some(1), 0.0, fx, "Solid", 5.0).unwrap();
        let gm = p.clip(&g).and_then(|c| c.media_id.clone()).unwrap();
        let size = |p: &Project| p.media_pool.iter().find(|m| m.id == gm).map(|m| (m.width, m.height)).unwrap();
        assert_eq!(size(&p), (Some(1920), Some(1080)));
        (p.settings.width, p.settings.height) = (1280, 720);
        assert!(!resize_generated(&mut p, &[ids[0].clone()]), "a picture is not generated media");
        assert!(resize_generated(&mut p, &[]) && size(&p) == (Some(1280), Some(720)) && !resize_generated(&mut p, &[]));
    }
}
