//! Timeline editing math: trim, snap, split, delete, move, paste, tracks, media drop.
use std::collections::{HashMap, HashSet};
use serde::{Deserialize, Serialize};
use crate::project::*;

/// Source length a clip may not exceed; None = unlimited (images, generated, unknown or unprobed media).
pub fn media_limit(project: &Project, clip: &Clip) -> Option<f64> {
    let id = clip.media_id.as_deref()?;
    let m = project.media_pool.iter().find(|m| m.id == id)?;
    (m.generator.is_none() && matches!(m.media_type, MediaType::Video | MediaType::Audio))
        .then_some(m.duration)
        .filter(|d| *d > 0.0)
}

/// Source time shown at timeline time `t` (shared with the engine plan). Forward: in + (t - pos) x rate; reversed:
/// in + (pos + dur - t) x rate. Either way the event covers the source span [in, in + dur x rate].
pub fn src_time(c: &Clip, t: f64) -> f64 {
    let local = if c.reverse { c.position + c.duration - t } else { t - c.position };
    c.in_point + local * c.rate
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
    clip.in_point = if shift { (orig.in_point + (start - orig.position) * orig.rate).max(0.0) } else { orig.in_point };
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
    let Some(on) = ids.last().and_then(|id| project.clip(id)).map(|c| !c.muted) else { return false };
    for c in tracks_mut(&mut project.timeline).flat_map(|t| &mut t.clips).filter(|c| ids.contains(&c.id)) {
        c.muted = on;
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
    second.transition = None; // the transition belongs to the event's start
    shift_clip_keys(&mut second, -first_dur);
    // The half that plays first in source order starts at in_point; reversed, that is the second half.
    if clip.reverse {
        first.in_point += second.duration * clip.rate;
    } else {
        second.in_point += first_dur * clip.rate;
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

/// What a ripple moves besides the events on the affected tracks (Options › Ripple). Milan Pro has no bus tracks,
/// so VEGAS's "Affected Tracks, Bus Tracks, Markers, and Regions" is `AffectedMarkers`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RippleMode {
    #[default]
    Affected,
    AffectedMarkers,
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
/// markers, regions and the time selection (one that straddles `keys_from` stretches); All moves every track.
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
    let mut done = false;
    for t in tracks_mut(tl).filter(|t| hit(t)) {
        for c in t.clips.iter_mut().filter(|c| moves(c)) {
            c.position += d;
            done = true;
        }
        t.clips.sort_by(|a, b| a.position.total_cmp(&b.position));
        for e in &mut t.envelopes {
            let keys = &mut e.points.keys;
            if d < 0.0 {
                keys.retain(|k| k.t < from + d - EPS || at_or_after(k.t));
            }
            for k in keys.iter_mut().filter(|k| at_or_after(k.t)) {
                k.t = (k.t + d).max(0.0);
                done = true;
            }
        }
    }
    if mode != RippleMode::Affected {
        for m in tl.markers.iter_mut().filter(|m| at_or_after(m.position)) {
            m.position = (m.position + d).max(0.0);
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
        for r in &mut tl.regions {
            let new = shift((r.start, r.end));
            done |= new != (r.start, r.end);
            (r.start, r.end) = new;
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

/// Add an empty track of that kind above track `before`, as VEGAS's header menu does; without one, a video track
/// goes first and an audio track last (VEGAS's Ctrl+Shift+Q / Ctrl+Q). Returns its index.
pub fn insert_track(project: &mut Project, is_video: bool, before: Option<usize>) -> usize {
    let tracks = project.timeline.tracks_mut(is_video);
    let at = before.map_or(if is_video { 0 } else { tracks.len() }, |b| b.min(tracks.len()));
    let (name, kind) = if is_video { ("Video", TrackKind::Video) } else { ("Audio", TrackKind::Audio) };
    tracks.insert(at, Track::new(&format!("{name} {}", tracks.len() + 1), kind));
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
    changed(project, done)
}

/// Move tracks `ids` of one kind, keeping their order, into the gap before track `to` (past the end = last).
pub fn move_tracks(project: &mut Project, is_video: bool, ids: &[String], to: usize) -> bool {
    let tracks = project.timeline.tracks_mut(is_video);
    let old: Vec<String> = tracks.iter().map(|t| t.id.clone()).collect();
    let at = tracks[..to.min(tracks.len())].iter().filter(|t| !ids.contains(&t.id)).count();
    let (moved, mut rest): (Vec<Track>, Vec<Track>) = std::mem::take(tracks).into_iter().partition(|t| ids.contains(&t.id));
    rest.splice(at..at, moved);
    *tracks = rest;
    let done = tracks.iter().map(|t| &t.id).ne(old.iter());
    changed(project, done)
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
// shortcut: a take shorter than the event holds its last frame and is silent past its end (VEGAS loops it); loop it in the plan if users ask.
pub fn cycle_take(c: &mut Clip, forward: bool) -> bool {
    let t = match (forward, c.takes.is_empty()) {
        (_, true) => return false,
        (true, _) => c.takes.remove(0),
        (false, _) => c.takes.pop().expect("takes"),
    };
    let old = make_active(c, t);
    if forward { c.takes.push(old) } else { c.takes.insert(0, old) }
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
            }
            (_, false) => choose_take(c, k - 1),
            (_, true) => {
                c.takes.remove(k - 1);
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
            c.takes.retain(|t| t.media_id != id);
            if c.media_id.as_deref() != Some(id) {
                return true;
            }
            let next = (!c.takes.is_empty()).then(|| c.takes.remove(0));
            next.map(|t| make_active(c, t)).is_some()
        });
    }
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

    let frame = 1.0 / project.settings.frame_rate.max(1.0);
    if let Some(l) = media_limit(project, &draft) {
        draft.in_point = draft.in_point.min(l - frame * draft.rate).max(0.0);
        draft.duration = draft.duration.min((l - draft.in_point) / draft.rate);
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

/// An envelope's values: dB for Volume (+6 at the lane top, -120 is silence), -1 (left) ..= 1 (right) for Pan, 0..=1
/// for the rest.
pub fn env_range(kind: EnvKind) -> (f32, f32) {
    match kind {
        EnvKind::Volume => (-120.0, 6.0),
        EnvKind::Pan => (-1.0, 1.0),
        _ => (0.0, 1.0),
    }
}

/// `v` inside the range; a Mute point is on (1) or off (0).
pub fn env_clamp(kind: EnvKind, v: f32) -> f32 {
    let (lo, hi) = env_range(kind);
    let v = v.clamp(lo, hi);
    if kind == EnvKind::Mute { v.round() } else { v }
}

/// The track envelopes Milan Pro plays: Volume, Pan and Mute on audio tracks, Composite Level on video tracks.
pub fn env_fits(kind: EnvKind, video: bool) -> bool {
    matches!((kind, video), (EnvKind::Volume | EnvKind::Pan | EnvKind::Mute, false) | (EnvKind::CompositeLevel, true))
}

/// Height of value `v` in the envelope's lane, 0 (bottom) ..= 1 (top). Volume: 0 dB at 0.7, linear up to +6 dB at the
/// top and 0.7 x 10^(dB / 40) below; Pan: left at the top; the rest linear over their range.
pub fn lane_frac(kind: EnvKind, v: f32) -> f32 {
    let (lo, hi) = env_range(kind);
    match kind {
        EnvKind::Volume if v >= 0.0 => 0.7 + 0.3 * v.min(hi) / hi,
        EnvKind::Volume => 0.7 * 10f32.powf(v.max(lo) / 40.0),
        EnvKind::Pan => (1.0 - v.clamp(lo, hi)) / 2.0,
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
        _ => lo + f * (hi - lo),
    }
}

/// Insert/Remove Envelope: add envelope `kind` to the track (one point at 0 holding the default), or remove it.
pub fn toggle_envelope(track: &mut Track, kind: EnvKind) -> bool {
    let n = track.envelopes.len();
    track.envelopes.retain(|e| e.kind != kind);
    if track.envelopes.len() == n {
        let curve = if kind == EnvKind::Mute { Curve::Hold } else { Curve::Linear };
        track.envelopes.push(Envelope { kind, points: Keyframes { keys: vec![Keyframe { t: 0.0, v: env_default(kind), curve }] } });
    }
    true
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

/// Run `f` on envelope `kind` of track `track` (unless the track is locked).
pub fn edit_envelope(project: &mut Project, track: &str, kind: EnvKind, f: impl FnOnce(&mut Envelope) -> bool) -> bool {
    let Some((v, i)) = project.find_track(track) else { return false };
    let t = &mut project.timeline.tracks_mut(v)[i];
    let done = !t.locked && t.envelopes.iter_mut().find(|e| e.kind == kind).is_some_and(f);
    changed(project, done)
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
        FxTarget::Media(_) | FxTarget::Transition(_) => None,
    }
}

/// `target` holds audio plug-ins: an audio event's or track's chain, or the Audio Master's.
pub fn fx_is_audio(p: &Project, target: &FxTarget) -> bool {
    match target {
        FxTarget::Event(id) => p.find_clip(id).is_some_and(|r| !r.is_video),
        FxTarget::Track(id) => p.find_track(id).is_some_and(|(v, _)| !v),
        FxTarget::AudioMaster => true,
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
        FxTarget::Media(id) => p.media_pool.iter().find(|m| m.id == *id)?.generator.clone().map(|g| (vec![g], true)),
        FxTarget::Transition(id) => clip(id).and_then(|(c, free)| Some((vec![fx_of_transition(c.transition.as_ref()?)], free))),
    }
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
        FxTarget::Transition(id) => {
            let tr = list.first().map(transition_of);
            p.find_clip(id).is_some_and(|r| {
                let t = &mut p.timeline.tracks_mut(r.is_video)[r.track_idx];
                let free = !t.locked;
                let c = &mut t.clips[r.clip_idx];
                let ok = free && !c.locked && c.transition.is_some() && tr.is_some() && c.transition != tr;
                if ok {
                    c.transition = tr;
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
    Fx { id: "transition".into(), plugin: tr.plugin.clone(), enabled: true, params: tr.params.iter().map(|(k, &v)| (k.clone(), key(v))).collect(), text: Default::default() }
}

/// A plug-in instance as a transition (transitions are not keyframed: each value at 0).
pub fn transition_of(fx: &Fx) -> Transition {
    Transition { plugin: fx.plugin.clone(), params: fx.params.iter().filter(|(_, k)| !k.keys.is_empty()).map(|(k, kf)| (k.clone(), kf.at(0.0))).collect() }
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
    /// An event's start with nothing before it.
    Start(usize),
}

/// Overlap: two events live at `at`. Otherwise the half of the event under `at` picks its start or its end; an end
/// counts only when an event starts there (Milan Pro has no transition on a lone fade-out).
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
        let next = (0..clips.len()).find(|&k| k != b && clips[k].position > e.position && (clips[k].position - end(e)).abs() <= tol)?;
        Some(Edge::Cut(b, next))
    }
}

/// Drop transition `tr` on video track `track` at time `at`. On an overlap it becomes the incoming event's. At a cut,
/// with Automatic Crossfades on, the outgoing event is extended right and the incoming one left by `cut_overlap / 2`
/// each, within their media (one with no handle leaves the whole overlap to the other), and both fade over it. On an
/// event's start with nothing before it, the event fades in over `cut_overlap` unless it already fades in. False
/// (nothing changed) when an event or the track is locked, at an end with nothing after it, or at a cut that cannot
/// overlap.
pub fn set_transition(p: &mut Project, track: &str, at: f64, tr: Transition, cut_overlap: f64, auto_xfade: bool) -> bool {
    let Some((true, ti)) = p.find_track(track) else { return false };
    let fps = p.settings.frame_rate.max(1.0);
    let t = &p.timeline.video_tracks[ti];
    let Some(edge) = transition_edge(&t.clips, at, 0.5 / fps).filter(|_| !t.locked) else { return false };
    let free = |k: usize| !t.clips[k].locked;
    match edge {
        Edge::Overlap(a, b) | Edge::Cut(a, b) if !free(a) || !free(b) => return false,
        Edge::Start(b) if !free(b) => return false,
        Edge::Cut(..) if !auto_xfade => return false,
        _ => {}
    }
    let (b, na) = match edge {
        Edge::Overlap(_, b) | Edge::Start(b) => (b, None),
        Edge::Cut(a, b) => {
            let (oa, ob) = (t.clips[a].clone(), t.clips[b].clone());
            let (la, lb) = (media_limit(p, &oa), media_limit(p, &ob));
            // Neither side may pass the other's far edge (less a frame) or run into the event beyond it.
            let (a_end, frame) = (oa.position + oa.duration, 1.0 / fps);
            let others = || t.clips.iter().enumerate().filter(|&(k, _)| k != a && k != b).map(|(_, c)| c);
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
            // Half each; what one side cannot give, the other takes.
            let ga = ext_a(cut_overlap * 0.5).duration - oa.duration;
            let mut nb = ext_b(cut_overlap - ga);
            let mut na = ext_a(cut_overlap - (ob.position - nb.position));
            let ov = na.position + na.duration - nb.position;
            if ov <= 1e-9 {
                return false;
            }
            (na.fade_out, nb.fade_in) = (ov, ov);
            na.clamp_fades();
            nb.clamp_fades();
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

/// Event context menu › Remove Transition.
pub fn remove_transition(p: &mut Project, id: &str) -> bool {
    let done = p.find_clip(id).is_some_and(|r| {
        let t = &mut p.timeline.tracks_mut(r.is_video)[r.track_idx];
        let free = !t.locked;
        let c = &mut t.clips[r.clip_idx];
        free && !c.locked && c.transition.take().is_some()
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
    let m = MediaItem { id: uuid::Uuid::new_v4().to_string(), name: name.into(), media_type: MediaType::Image, duration: 86400.0, generator: Some(fx), ..Default::default() };
    let mut c = Clip::from_media(&m, at.max(0.0));
    (c.duration, c.out_point) = (len, len);
    let id = c.id.clone();
    let clips = &mut tl.video_tracks[i].clips;
    clips.push(c);
    clips.sort_by(|a, b| a.position.total_cmp(&b.position));
    p.media_pool.push(m);
    changed(p, true).then_some(id)
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let c = Clip::from_media(&m, 0.0);
        assert_eq!(media_limit(&p, &c), Some(8.0));
        m.generator = Some(Fx { id: "f".into(), plugin: "milan.gen.solid".into(), enabled: true, params: Default::default(), text: Default::default() });
        p.media_pool[0] = m;
        assert_eq!(media_limit(&p, &c), None);
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
        (c.fade_in, c.transition) = (1.0, Some(Transition { plugin: "dissolve".into(), params: Default::default() }));
        let (a, b) = split_clip(&c, 12.0).unwrap();
        assert!(a.transition.is_some() && b.transition.is_none());
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
        // Deleting the expanded selection removes the whole pair.
        let (mut p, _, a) = av_pair();
        let all = expand_groups(&p, &[a]);
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
        // Media v is 10 s: from in 5 the event can last 5 s at most; it moves after the other event.
        p.timeline.audio_tracks[0].clips.push(clip(4.0, 1.0, 0.0));
        let mut d = draft;
        (d.duration, d.position, d.name) = (8.0, 6.0, "Take B".into());
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
        p.timeline.video_tracks[0].clips = vec![a, b];
        let t = p.timeline.video_tracks[0].id.clone();
        (p, t)
    }

    fn dissolve() -> Transition {
        Transition { plugin: "milan.tr.dissolve".into(), params: Default::default() }
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
        (a.duration, a.out_point) = (0.3, 0.3);
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
        // A lone start fades in over the cut-to-overlap length; the end of the last event takes none.
        let (mut p, t) = cut(5.0, 5.0);
        p.timeline.video_tracks[0].clips.remove(0);
        assert!(set_transition(&mut p, &t, 5.2, dissolve(), 0.5, false));
        assert_eq!(p.timeline.video_tracks[0].clips[0].fade_in, 0.5);
        assert!(!set_transition(&mut p, &t, 9.5, dissolve(), 0.5, true));
        let id = p.timeline.video_tracks[0].clips[0].id.clone();
        assert!(remove_transition(&mut p, &id) && !remove_transition(&mut p, &id));
        assert!(p.timeline.video_tracks[0].clips[0].transition.is_none());
        // The FX window edits a transition as a plug-in and puts it back.
        let tr = Transition { plugin: "milan.tr.linear_wipe".into(), params: [("angle".to_string(), 90.0)].into() };
        assert_eq!(transition_of(&fx_of_transition(&tr)), tr);
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
}
