//! Timeline editing math: trim, snap, split, delete, move, paste, tracks, media drop.
use std::collections::HashMap;
use crate::project::*;

/// Source length a clip may not exceed; None = unlimited (images, unknown or unprobed media).
pub fn media_limit(project: &Project, clip: &Clip) -> Option<f64> {
    let id = clip.media_id.as_deref()?;
    let m = project.media_pool.iter().find(|m| m.id == id)?;
    matches!(m.media_type, MediaType::Video | MediaType::Audio)
        .then_some(m.duration)
        .filter(|d| *d > 0.0)
}

/// Move the clip's left edge to `new_start`, keeping its right edge fixed.
pub fn trim_left(clip: &mut Clip, orig: &Clip, new_start: f64, frame: f64, limit: Option<f64>) {
    let end = orig.position + orig.duration;
    // Limited media can't reveal frames before in_point 0; images may extend freely.
    let min_start = if limit.is_some() { (orig.position - orig.in_point).max(0.0) } else { 0.0 };
    let start = new_start.min(end - frame).max(min_start);
    clip.position = start;
    clip.in_point = (orig.in_point + start - orig.position).max(0.0);
    clip.duration = end - start;
    clip.out_point = orig.out_point;
    (clip.fade_in, clip.fade_out) = (orig.fade_in, orig.fade_out);
    clip.clamp_fades();
}

/// Move the clip's right edge to `new_end`.
pub fn trim_right(clip: &mut Clip, orig: &Clip, new_end: f64, frame: f64, limit: Option<f64>) {
    let max_dur = limit.map_or(f64::INFINITY, |l| l - orig.in_point);
    let dur = (new_end - orig.position).min(max_dur).max(frame);
    clip.duration = dur;
    clip.out_point = orig.in_point + dur;
    (clip.fade_in, clip.fade_out) = (orig.fade_in, orig.fade_out);
    clip.clamp_fades();
}

/// Nearest target within `threshold` of `t`.
pub fn snap(t: f64, targets: &[f64], threshold: f64) -> Option<f64> {
    targets
        .iter()
        .copied()
        .filter(|x| (x - t).abs() <= threshold)
        .min_by(|a, b| (a - t).abs().total_cmp(&(b - t).abs()))
}

/// Snap a span [start, start+len] by whichever edge is closest to a target.
pub fn snap_span(start: f64, len: f64, targets: &[f64], threshold: f64) -> f64 {
    let a = snap(start, targets, threshold).map(|s| s - start);
    let b = snap(start + len, targets, threshold).map(|s| s - start - len);
    start + [a, b].into_iter().flatten().min_by(|x, y| x.abs().total_cmp(&y.abs())).unwrap_or(0.0)
}

/// 0, playhead, markers, loop ends and every clip edge except the clip with `exclude_id`.
pub fn snap_targets(project: &Project, exclude_id: Option<&str>) -> Vec<f64> {
    snap_targets_except(project, exclude_id.as_slice())
}

/// `snap_targets` without the edges of the events in `ids` (the ones being dragged).
pub fn snap_targets_except(project: &Project, ids: &[&str]) -> Vec<f64> {
    let tl = &project.timeline;
    let mut v = vec![0.0, tl.cursor_position];
    v.extend(tl.markers.iter().map(|m| m.position));
    v.extend(tl.loop_region.iter().flat_map(|&(a, b)| [a, b]));
    for c in tl.video_tracks.iter().chain(&tl.audio_tracks).flat_map(|t| &t.clips) {
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
    first.out_point = clip.in_point + first_dur;
    first.fade_out = 0.0; // each half keeps only its outer fade
    first.clamp_fades();
    let mut second = clip.clone();
    second.id = uuid::Uuid::new_v4().to_string();
    second.position = at;
    second.duration -= first_dur;
    second.in_point += first_dur;
    second.fade_in = 0.0;
    second.clamp_fades();
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
    let mut done = false;
    for t in tracks_mut(&mut project.timeline).filter(|t| !t.locked) {
        let mut j = 0;
        while j < t.clips.len() {
            let c = &t.clips[j];
            if !c.locked && (ids.is_empty() || ids.contains(&c.id)) {
                if let Some((a, b)) = split_clip(c, at) {
                    t.clips[j] = a;
                    t.clips.insert(j + 1, b);
                    j += 1;
                    done = true;
                }
            }
            j += 1;
        }
    }
    changed(project, done)
}

/// Move events `ids` by `dt` seconds and `dtrack` tracks (the same index offset within each kind). The earliest
/// stays at or after 0. A hop onto a missing or locked track refuses the whole move; locked events stay where they
/// are. Touched tracks are stable-sorted by position.
pub fn move_clips(project: &mut Project, ids: &[String], dt: f64, dtrack: isize) -> bool {
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
    changed(project, true)
}

/// Snap `t` to the project frame grid (Options › Quantize to Frames).
pub fn quantize(t: f64, fps: f64) -> f64 {
    let fps = fps.max(1.0);
    (t * fps).round() / fps
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
        tl.tracks_mut(*v)[i].clips.push(c);
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
        spans.push((m, a, b, t));
        t += b - a;
    }
    let paired = |m: &MediaItem| m.media_type == MediaType::Video && m.sample_rate.is_some();
    if let Some(tr) = tl.audio_tracks.get(at) {
        let overlaps = |pos: f64, len: f64| tr.clips.iter().any(|c| c.position < pos + len && pos < c.position + c.duration);
        if spans.iter().any(|&(m, a, b, pos)| paired(m) && (tr.locked || overlaps(pos, b - a))) {
            at = tl.audio_tracks.len();
        }
    }
    let mut events = Vec::new();
    for (m, a, b, pos) in spans {
        let mut c = Clip::from_media(m, pos);
        (c.in_point, c.out_point, c.duration) = (a, b, b - a);
        if m.media_type == MediaType::Audio {
            events.push((false, at, c));
            continue;
        }
        if paired(m) {
            c.group = Some(uuid::Uuid::new_v4().to_string());
            let mut sound = c.clone();
            sound.id = uuid::Uuid::new_v4().to_string();
            sound.color = [100, 180, 100];
            events.push((true, vt, c));
            events.push((false, at, sound));
        } else {
            events.push((true, vt, c));
        }
    }
    let locked = |v: bool, i: usize| tl.tracks(v).get(i).is_some_and(|t| t.locked);
    let refused = target.is_some_and(|(v, i)| locked(v, i)) || events.iter().any(|(v, i, _)| locked(*v, *i));
    DropPlan { events, refused }
}

/// Add the events of a `plan_drop`, creating the tracks it needs. Returns the new event ids (none when refused).
pub fn apply_drop(project: &mut Project, plan: DropPlan) -> Vec<String> {
    if plan.refused {
        return Vec::new();
    }
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
    let done = !ids.is_empty();
    changed(project, done);
    ids
}

/// Drop pool media `idxs` (each with source range `range`) on `target` at `time`: `plan_drop` + `apply_drop`.
pub fn drop_media(project: &mut Project, idxs: &[usize], range: Option<(f64, f64)>, target: Option<(bool, usize)>, time: f64, still_len: f64) -> Vec<String> {
    let items: Vec<_> = idxs.iter().filter_map(|&i| project.media_pool.get(i)).map(|m| (m, range)).collect();
    let plan = plan_drop(&project.timeline, &items, target, time, still_len);
    apply_drop(project, plan)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!((snap_span(4.95, 2.0, &t, 0.1) - 5.0).abs() < 1e-9); // start snaps
        assert!((snap_span(2.98, 7.0, &t, 0.1) - 3.0).abs() < 1e-9); // end snaps to 10
        assert_eq!(snap_span(3.0, 1.0, &t, 0.1), 3.0);
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
        assert!(!move_clips(&mut p, &id, 1.0, 0));
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
        assert!(move_clips(&mut p, &ids, -5.0, 0));
        assert_eq!(p.clip(&ids[0]).unwrap().position, 0.0);
        assert_eq!(p.clip(&ids[1]).unwrap().position, 3.0);
        // A hop of +1: V1 → V2 is fine, but V2 → V3 and A1 → A2 are missing, so nothing moves.
        assert!(!move_clips(&mut p, &ids, 0.0, 1));
        assert!(move_clips(&mut p, &ids[..1], 0.5, 1));
        assert_eq!(p.find_clip(&ids[0]).map(|r| r.track_idx), Some(1));
        // Sorted by position on the target track: the moved event (0.5) comes before V2's (3.0).
        assert_eq!(p.timeline.video_tracks[1].clips[0].id, ids[0]);
        // A hop onto a locked track is refused for all events.
        p.timeline.video_tracks[0].locked = true;
        assert!(!move_clips(&mut p, &ids[..2], 1.0, -1));
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
        assert_eq!(drop_media(&mut p, &[1], None, Some((true, 0)), 1.0, 5.0).len(), 1);
        assert_eq!(laid_out(&p, true), [(0, 1.0, 5.0, "i".into())]);
        assert!(p.modified);
    }

    #[test]
    fn wrong_kind_rows_redirect_to_the_first_track_of_the_right_kind() {
        let mut p = media_project();
        p.timeline.add_video_track();
        p.timeline.add_audio_track();
        // Audio on Video 2 goes to Audio 1; a still on Audio 2 goes to Video 1.
        drop_media(&mut p, &[2], None, Some((true, 1)), 0.0, 5.0);
        drop_media(&mut p, &[1], None, Some((false, 1)), 0.0, 5.0);
        assert_eq!(laid_out(&p, false), [(0, 0.0, 4.0, "a".into())]);
        assert_eq!(laid_out(&p, true), [(0, 0.0, 5.0, "i".into())]);
        // No audio track at all: one is created.
        p.timeline.audio_tracks.clear();
        drop_media(&mut p, &[2], None, Some((true, 0)), 9.0, 5.0);
        assert_eq!(laid_out(&p, false), [(0, 9.0, 4.0, "a".into())]);
    }

    #[test]
    fn drop_below_the_last_track_creates_tracks() {
        let mut p = media_project();
        let ids = drop_media(&mut p, &[0], None, None, 2.0, 5.0);
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
        drop_media(&mut p, &[2], None, None, 0.0, 5.0);
        assert_eq!((p.timeline.video_tracks.len(), p.timeline.audio_tracks.len()), (0, 1));
    }

    #[test]
    fn paired_audio_takes_a_new_track_when_the_first_is_occupied() {
        let mut p = media_project();
        p.timeline.audio_tracks[0].clips.push(clip(8.0, 4.0, 0.0));
        drop_media(&mut p, &[0], None, Some((true, 0)), 0.0, 5.0); // 0..10 overlaps 8..12
        assert_eq!(laid_out(&p, false), [(0, 8.0, 4.0, String::new()), (1, 0.0, 10.0, "v".into())]);
        // Free over the event's range: the first audio track.
        drop_media(&mut p, &[0], Some((1.0, 3.0)), Some((true, 0)), 20.0, 5.0);
        assert_eq!(laid_out(&p, false)[1], (0, 20.0, 2.0, "v".into()));
        // A locked first audio track is not free either.
        p.timeline.audio_tracks[0].locked = true;
        drop_media(&mut p, &[0], None, Some((true, 0)), 40.0, 5.0);
        assert_eq!(p.timeline.audio_tracks.len(), 3);
    }

    #[test]
    fn several_items_go_across_time_and_stills_use_the_still_length() {
        let mut p = media_project();
        let ids = drop_media(&mut p, &[0, 1, 2], None, Some((true, 0)), 1.0, 3.0);
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
        assert!(drop_media(&mut p, &[0], None, Some((true, 0)), 0.0, 5.0).is_empty());
        // Pointing at a locked row refuses even when the media would go elsewhere.
        assert!(drop_media(&mut p, &[2], None, Some((true, 0)), 0.0, 5.0).is_empty());
        assert!(laid_out(&p, true).is_empty() && laid_out(&p, false).is_empty() && !p.modified);
        // Below the last track, the new tracks take it.
        assert_eq!(drop_media(&mut p, &[1], None, None, 0.0, 5.0).len(), 1);
    }

    #[test]
    fn quantize_lands_on_frames() {
        let q = quantize(1.0, 29.97);
        assert!((q - 30.0 / 29.97).abs() < 1e-12);
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
        assert_eq!(drop_media(&mut p, &[0], Some((2.0, 3.5)), Some((true, 0)), 5.0, 5.0).len(), 2);
        let v = &p.timeline.video_tracks[0].clips[0];
        assert_eq!((v.position, v.in_point, v.out_point, v.duration), (5.0, 2.0, 3.5, 1.5));
        assert_eq!(p.timeline.audio_tracks[0].clips[0].in_point, 2.0); // linked audio gets the same range
        m.media_type = MediaType::Unknown;
        p.media_pool[0] = m;
        assert!(drop_media(&mut p, &[0], None, Some((true, 0)), 0.0, 5.0).is_empty()); // unreadable media adds nothing
    }
}
