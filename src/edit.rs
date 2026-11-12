//! Timeline editing math: trim, snap, split, delete, media drop.
use crate::project::*;
use crate::ui::ClipRef;

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
    let tl = &project.timeline;
    let mut v = vec![0.0, tl.cursor_position];
    v.extend(tl.markers.iter().map(|m| m.position));
    v.extend(tl.loop_region.iter().flat_map(|&(a, b)| [a, b]));
    for c in tl.video_tracks.iter().chain(&tl.audio_tracks).flat_map(|t| &t.clips) {
        if Some(c.id.as_str()) != exclude_id {
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

pub fn split_at(project: &mut Project, sel: &ClipRef, at: f64) -> bool {
    let Some(track) = unlocked_track(project, sel) else { return false };
    let Some((a, b)) = split_clip(&track.clips[sel.clip_idx], at) else { return false };
    track.clips[sel.clip_idx] = a;
    track.clips.insert(sel.clip_idx + 1, b);
    project.mark_modified();
    true
}

pub fn delete_clip(project: &mut Project, sel: &ClipRef) -> bool {
    let Some(track) = unlocked_track(project, sel) else { return false };
    track.clips.remove(sel.clip_idx);
    project.mark_modified();
    true
}

/// Drop media `idx` on a track at `time`. Video with audio also lands on the audio
/// track of the same index (created if missing). Returns false if nothing was added.
pub fn drop_media(project: &mut Project, idx: usize, is_video: bool, track_idx: usize, time: f64) -> bool {
    drop_media_range(project, idx, is_video, track_idx, time, None)
}

/// `drop_media` with an optional source range `(in, out)` (the Trimmer's selection).
pub fn drop_media_range(project: &mut Project, idx: usize, is_video: bool, track_idx: usize, time: f64, range: Option<(f64, f64)>) -> bool {
    let Some(media) = project.media_pool.get(idx).cloned() else { return false };
    let clip = |color: Option<[u8; 3]>| {
        let mut c = Clip::from_media(&media, time);
        if let Some((a, b)) = range.filter(|(a, b)| b > a) {
            (c.in_point, c.out_point, c.duration) = (a, b, b - a);
        }
        if let Some(col) = color {
            c.color = col;
        }
        c
    };
    let fits = match media.media_type {
        MediaType::Video => true,
        MediaType::Image => is_video,
        MediaType::Audio => !is_video,
        MediaType::Unknown => false,
    };
    let tl = &mut project.timeline;
    match tl.tracks_mut(is_video).get_mut(track_idx) {
        Some(t) if fits && !t.locked => t.clips.push(clip(None)),
        _ => return false,
    }
    if is_video && media.media_type == MediaType::Video && media.sample_rate.is_some() {
        while tl.audio_tracks.len() <= track_idx {
            tl.add_audio_track();
        }
        let at = &mut tl.audio_tracks[track_idx];
        if !at.locked {
            at.clips.push(clip(Some([100, 180, 100])));
        }
    }
    project.mark_modified();
    true
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
        assert!(!split_at(&mut p, &sel, 2.0));
        assert!(!delete_clip(&mut p, &sel));
        p.timeline.video_tracks[0].locked = false;
        assert!(split_at(&mut p, &sel, 2.0));
        assert_eq!(p.timeline.video_tracks[0].clips.len(), 2);
        // A locked event blocks edits on an unlocked track.
        assert!(toggle_clip_lock(&mut p, &sel));
        assert!(!delete_clip(&mut p, &sel) && !trim_to(&mut p, &sel, Some(1.0), None));
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
        assert!(drop_media_range(&mut p, 0, true, 0, 5.0, Some((2.0, 3.5))));
        let v = &p.timeline.video_tracks[0].clips[0];
        assert_eq!((v.position, v.in_point, v.out_point, v.duration), (5.0, 2.0, 3.5, 1.5));
        assert_eq!(p.timeline.audio_tracks[0].clips[0].in_point, 2.0); // linked audio gets the same range
        m.media_type = MediaType::Audio;
        p.media_pool[0] = m;
        assert!(!drop_media_range(&mut p, 0, true, 0, 0.0, None)); // audio can't go on a video track
    }
}
