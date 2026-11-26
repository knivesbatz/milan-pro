//! R42: a structural check of a project after it is read: ids that clash or point nowhere, times that are NaN,
//! infinite or negative, takes that duplicate each other. What is safe to fix is fixed in memory and listed; what is
//! not is only listed. The file on disk is untouched until the user saves (and then `.mnle.bak` keeps the original).
use std::collections::HashSet;
use super::Project;

/// Repair and report. Each line names the place and what was done ("set to 0", "removed", "kept").
pub fn validate(p: &mut Project) -> Vec<String> {
    let mut out = Vec::new();
    settings(p, &mut out);
    ids(p, &mut out);
    times(p, &mut out);
    takes(p, &mut out);
    out
}

fn bad(x: f64) -> bool {
    !x.is_finite() || x < 0.0
}

/// Replace a NaN, infinite or negative time with `to`, with a report line.
fn fix(what: &str, x: &mut f64, to: f64, out: &mut Vec<String>) {
    if bad(*x) {
        out.push(format!("{what} was {}, set to {to}", if x.is_nan() { "NaN".to_string() } else { x.to_string() }));
        *x = to;
    }
}

fn settings(p: &mut Project, out: &mut Vec<String>) {
    let s = &mut p.settings;
    let d = super::ProjectSettings::default();
    if !s.frame_rate.is_finite() || s.frame_rate <= 0.0 {
        out.push(format!("Project frame rate was {}, set to {}", s.frame_rate, d.frame_rate));
        s.frame_rate = d.frame_rate;
    }
    if s.width == 0 || s.height == 0 {
        out.push(format!("Project frame size was {}x{}, set to {}x{}", s.width, s.height, d.width, d.height));
        (s.width, s.height) = (d.width, d.height);
    }
    if s.sample_rate == 0 {
        out.push(format!("Project sample rate was 0, set to {}", d.sample_rate));
        s.sample_rate = d.sample_rate;
    }
}

/// Duplicate media, track and event ids get a new id (the first keeps its own, so events still find their media);
/// events and takes whose media id is unknown are reported (the event plays offline; the take is removed).
fn ids(p: &mut Project, out: &mut Vec<String>) {
    let mut seen = HashSet::new();
    for m in &mut p.media_pool {
        if !seen.insert(m.id.clone()) {
            let old = std::mem::replace(&mut m.id, uuid::Uuid::new_v4().to_string());
            out.push(format!("Media '{}' had the same id as another item ({old}); it got a new one", m.name));
        }
    }
    let media: HashSet<String> = p.media_pool.iter().map(|m| m.id.clone()).collect();
    let mut tracks = HashSet::new();
    let mut events = HashSet::new();
    let tl = &mut p.timeline;
    for t in tl.video_tracks.iter_mut().chain(&mut tl.audio_tracks) {
        if !tracks.insert(t.id.clone()) {
            let old = std::mem::replace(&mut t.id, uuid::Uuid::new_v4().to_string());
            out.push(format!("Track '{}' had the same id as another track ({old}); it got a new one", t.name));
        }
        for c in &mut t.clips {
            if !events.insert(c.id.clone()) {
                let old = std::mem::replace(&mut c.id, uuid::Uuid::new_v4().to_string());
                out.push(format!("Event '{}' on {} had the same id as another event ({old}); it got a new one", c.name, t.name));
            }
            if let Some(id) = c.media_id.as_ref().filter(|id| !media.contains(*id)) {
                out.push(format!("Event '{}' on {} refers to media id {id}, which is not in the project; it is offline", c.name, t.name));
            }
            let before = c.takes.len();
            c.takes.retain(|k| media.contains(&k.media_id));
            if c.takes.len() != before {
                out.push(format!("Event '{}' on {}: {} take(s) referred to media not in the project and were removed", c.name, t.name, before - c.takes.len()));
            }
        }
    }
}

fn times(p: &mut Project, out: &mut Vec<String>) {
    let frame = 1.0 / p.settings.frame_rate.max(1.0);
    for m in &mut p.media_pool {
        fix(&format!("Media '{}' duration", m.name), &mut m.duration, 0.0, out);
    }
    let tl = &mut p.timeline;
    for t in tl.video_tracks.iter_mut().chain(&mut tl.audio_tracks) {
        for c in &mut t.clips {
            let at = format!("Event '{}' on {}", c.name, t.name);
            fix(&format!("{at}: position"), &mut c.position, 0.0, out);
            fix(&format!("{at}: in point"), &mut c.in_point, 0.0, out);
            if !c.duration.is_finite() || c.duration <= 0.0 {
                out.push(format!("{at}: length was {}, set to one frame", c.duration));
                c.duration = frame;
            }
            if bad(c.out_point) || c.out_point < c.in_point {
                out.push(format!("{at}: out point was {}, set from the in point and length", c.out_point));
                c.out_point = c.in_point + c.duration;
            }
            if !c.rate.is_finite() || c.rate <= 0.0 {
                out.push(format!("{at}: playback rate was {}, set to 1", c.rate));
                c.rate = 1.0;
            }
            fix(&format!("{at}: fade in"), &mut c.fade_in, 0.0, out);
            fix(&format!("{at}: fade out"), &mut c.fade_out, 0.0, out);
            for k in &mut c.takes {
                fix(&format!("{at}: take '{}' in point", k.name), &mut k.in_point, 0.0, out);
            }
        }
    }
    let n = tl.markers.len();
    tl.markers.retain(|m| !bad(m.position));
    if tl.markers.len() != n {
        out.push(format!("{} marker(s) had no valid position and were removed", n - tl.markers.len()));
    }
    let n = tl.regions.len();
    tl.regions.retain(|r| !bad(r.start) && !bad(r.end) && r.end >= r.start);
    if tl.regions.len() != n {
        out.push(format!("{} region(s) had no valid range and were removed", n - tl.regions.len()));
    }
    if tl.loop_region.is_some_and(|(a, b)| bad(a) || bad(b)) {
        tl.loop_region = None;
        out.push("The time selection had no valid range and was cleared".into());
    }
    if bad(tl.cursor_position) {
        tl.cursor_position = 0.0;
    }
    for n in &mut p.notes {
        fix("A project note's position", &mut n.at, 0.0, out);
    }
}

/// Takes that overlap: the same media at the same in point twice, or a take that is the active media itself.
fn takes(p: &mut Project, out: &mut Vec<String>) {
    let tl = &mut p.timeline;
    for t in tl.video_tracks.iter_mut().chain(&mut tl.audio_tracks) {
        for c in &mut t.clips {
            let mut seen: HashSet<(String, u64)> = HashSet::new();
            if let Some(m) = &c.media_id {
                seen.insert((m.clone(), c.in_point.to_bits()));
            }
            let before = c.takes.len();
            c.takes.retain(|k| seen.insert((k.media_id.clone(), k.in_point.to_bits())));
            if c.takes.len() != before {
                out.push(format!("Event '{}' on {}: {} take(s) duplicated another take or the active media and were removed", c.name, t.name, before - c.takes.len()));
            }
            if c.take_no as usize > c.takes.len() {
                c.take_no = 0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Clip, MediaItem, Take};

    fn project() -> Project {
        let mut p = Project::new("t");
        p.media_pool.push(MediaItem { id: "m1".into(), name: "a.mp4".into(), duration: 10.0, ..Default::default() });
        let mut c = Clip::new("ev", 2.0, 3.0);
        c.media_id = Some("m1".into());
        p.timeline.video_tracks[0].clips.push(c);
        p
    }

    #[test]
    fn a_nan_position_is_repaired_and_listed() {
        let mut p = project();
        p.timeline.video_tracks[0].clips[0].position = f64::NAN;
        let notes = validate(&mut p);
        assert_eq!(p.timeline.video_tracks[0].clips[0].position, 0.0);
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].contains("position was NaN, set to 0"), "{notes:?}");
        // A sound project reports nothing and is left alone.
        let mut ok = project();
        let before = ok.clone();
        assert!(validate(&mut ok).is_empty());
        assert_eq!(ok.timeline.video_tracks[0].clips, before.timeline.video_tracks[0].clips);
    }

    #[test]
    fn negative_and_infinite_times_and_bad_settings() {
        let mut p = project();
        {
            let c = &mut p.timeline.video_tracks[0].clips[0];
            (c.duration, c.fade_in, c.rate, c.out_point) = (-1.0, f64::INFINITY, 0.0, f64::NAN);
        }
        p.settings.frame_rate = f64::NAN;
        p.timeline.markers.push(crate::project::Marker { position: f64::NAN, name: "x".into(), color: [0; 3] });
        p.timeline.loop_region = Some((0.0, f64::NAN));
        let notes = validate(&mut p);
        let c = &p.timeline.video_tracks[0].clips[0];
        assert!(c.duration > 0.0 && c.fade_in == 0.0 && c.rate == 1.0 && c.out_point == c.in_point + c.duration);
        assert_eq!(p.settings.frame_rate, 29.97);
        assert!(p.timeline.markers.is_empty() && p.timeline.loop_region.is_none());
        assert!(notes.len() >= 7, "{notes:?}");
    }

    #[test]
    fn duplicate_ids_unknown_media_and_overlapping_takes() {
        let mut p = project();
        let dup = p.media_pool[0].clone();
        p.media_pool.push(dup);
        let mut c2 = p.timeline.video_tracks[0].clips[0].clone(); // same event id
        c2.media_id = Some("nope".into());
        c2.takes = vec![
            Take { media_id: "m1".into(), in_point: 0.0, name: "t1".into() },
            Take { media_id: "m1".into(), in_point: 0.0, name: "t1 again".into() },
            Take { media_id: "gone".into(), in_point: 0.0, name: "t2".into() },
        ];
        p.timeline.video_tracks[0].clips.push(c2);
        let t2 = p.timeline.video_tracks[0].clone(); // same track id
        p.timeline.video_tracks.push(t2);
        let notes = validate(&mut p);
        assert_ne!(p.media_pool[0].id, p.media_pool[1].id);
        assert_ne!(p.timeline.video_tracks[0].id, p.timeline.video_tracks[1].id);
        let clips = &p.timeline.video_tracks[0].clips;
        assert_ne!(clips[0].id, clips[1].id);
        assert_eq!(clips[1].takes.len(), 1, "{:?}", clips[1].takes);
        assert!(notes.iter().any(|n| n.contains("refers to media id nope")), "{notes:?}");
        assert!(notes.iter().any(|n| n.contains("same id as another item")), "{notes:?}");
        assert!(notes.iter().any(|n| n.contains("duplicated another take")), "{notes:?}");
    }
}
