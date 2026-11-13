//! R17b: the multicamera preview. With Tools › Multicamera › Enable Multicamera Editing on, the Video Preview shows
//! every take of the topmost multicamera event at the cursor in a grid (VEGAS: with overlapping video the topmost one
//! shows; single-take events such as titles do not). The grid is a project of its own (`grid`) that the video engine
//! composes like any other, so it plays from the same decoders and caches: one track per take, each moved into its
//! tile by Track Motion, over black. As in VEGAS, video FX, envelopes and motion are left out while it shows.
use crate::edit;
use crate::project::{Clip, Keyframe, Keyframes, MediaItem, MediaType, Motion, Project, Track, TrackKind};

/// Columns and rows of a grid of `n` takes: as square as it goes, as wide as tall or wider.
pub fn grid_size(n: usize) -> (usize, usize) {
    let cols = (n.max(1) as f64).sqrt().ceil() as usize;
    (cols, n.max(1).div_ceil(cols))
}

/// Where take `k` of `n` shows, as [x0, y0, x1, y1] of the frame (0-1): tiles keep the frame's shape, rows centred.
pub fn tile(n: usize, k: usize) -> [f32; 4] {
    let (cols, rows) = grid_size(n);
    let s = 1.0 / cols as f32;
    let top = (1.0 - rows as f32 * s) * 0.5;
    let (x, y) = ((k % cols) as f32 * s, top + (k / cols) as f32 * s);
    [x, y, x + s, y + s]
}

/// The multicamera events on video tracks in the preview.
fn multicam_events(p: &Project) -> impl Iterator<Item = &Clip> {
    p.timeline.shown_video_tracks().flat_map(|t| &t.clips).filter(|c| edit::is_multicam(c) && !c.muted)
}

/// The takes a grid has: the most any multicamera event in the preview has (0 without one). Read every frame.
pub fn takes_in(p: &Project) -> usize {
    multicam_events(p).map(|c| c.takes.len() + 1).max().unwrap_or(0)
}

/// The multicamera events the grid shows, by time: the topmost where they overlap.
fn shown(p: &Project) -> Vec<&Clip> {
    let mut out: Vec<&Clip> = Vec::new();
    for c in multicam_events(p) {
        let i = out.partition_point(|s| s.position < c.position);
        let free = (i == 0 || out[i - 1].position + out[i - 1].duration <= c.position) && out.get(i).is_none_or(|s| c.position + c.duration <= s.position);
        if free {
            out.insert(i, c);
        }
    }
    out
}

/// The generated black under the grid.
const BLACK: &str = "milan.multicam.black";

/// `p` as the multicamera preview shows it: over each shown multicamera event, its takes in their tiles on black
/// (empty takes stay black); elsewhere the timeline as it is, without video FX.
pub fn grid(p: &Project) -> Project {
    let mut q = super::without_video_fx(p);
    q.timeline.master.video_envelopes.clear();
    let shown = shown(p);
    let n = takes_in(p);
    if n == 0 {
        return q;
    }
    // Nothing that is wholly under the grid needs decoding. Solo has picked the tracks that show: the grid's own
    // tracks show over them.
    let under = |c: &Clip| {
        let i = shown.partition_point(|s| s.position <= c.position + 1e-9);
        i > 0 && c.position + c.duration <= shown[i - 1].position + shown[i - 1].duration + 1e-9
    };
    let any_solo = p.timeline.any_solo(true);
    for t in &mut q.timeline.video_tracks {
        if !p.timeline.plays(t, any_solo) {
            t.clips.clear();
        }
        t.clips.retain(|c| !under(c));
        t.solo = false;
    }
    q.timeline.track_groups.iter_mut().for_each(|g| g.solo = false);
    use crate::engine::plugins::{find, new_fx};
    let black = MediaItem { id: BLACK.into(), name: "Black".into(), media_type: MediaType::Image, duration: 86400.0, generator: find("milan.gen.solid").map(|s| new_fx(s, None)), ..Default::default() };
    let mut bg = Track::new("Multicamera", TrackKind::Video);
    let mut tracks: Vec<Track> = (0..n)
        .map(|k| {
            let [x0, y0, x1, y1] = tile(n, k);
            let m = Motion { x: (x0 + x1) * 0.5 - 0.5, y: (y0 + y1) * 0.5 - 0.5, w: x1 - x0, h: y1 - y0, ..Default::default() };
            let mut t = Track::new(&format!("Take {}", k + 1), TrackKind::Video);
            t.motion = Keyframes { keys: vec![Keyframe { t: 0.0, v: m, curve: Default::default() }] };
            t
        })
        .collect();
    for s in shown {
        bg.clips.push(Clip { position: s.position, duration: s.duration, ..Clip::from_media(&black, 0.0) });
        for (k, t) in edit::take_list(s).0.into_iter().enumerate().filter(|(_, t)| !t.media_id.is_empty()) {
            tracks[k].clips.push(Clip {
                media_id: Some(t.media_id),
                in_point: t.in_point,
                takes: Vec::new(),
                take_no: 0,
                fx: Vec::new(),
                envelopes: Vec::new(),
                pan_crop: Default::default(),
                masks: Vec::new(),
                transition: None,
                transition_out: None,
                fade_in: 0.0,
                fade_out: 0.0,
                ..s.clone()
            });
        }
    }
    q.media_pool.push(black);
    tracks.push(bg);
    tracks.append(&mut q.timeline.video_tracks);
    q.timeline.video_tracks = tracks;
    q
}

/// The grid at a time: how many tiles, the active take and each take's name (None for an empty take).
#[derive(Debug, PartialEq)]
pub struct At {
    pub n: usize,
    pub active: usize,
    pub names: Vec<Option<String>>,
}

/// What the grid shows at `t`: the topmost multicamera event there (as `edit::multicam_at` finds it, the one a take
/// choice edits). None where no multicamera event plays.
pub fn at(p: &Project, t: f64) -> Option<At> {
    let (ti, j) = edit::multicam_at(&p.timeline, true, t)?;
    let (list, active) = edit::take_list(&p.timeline.video_tracks[ti].clips[j]);
    Some(At { n: takes_in(p).max(list.len()), active, names: list.into_iter().map(|t| (!t.media_id.is_empty()).then_some(t.name)).collect() })
}

/// The take under frame point `pos` (0-1) of a grid of `n` tiles, of the first `takes` (the event there has).
pub fn hit(n: usize, takes: usize, pos: [f32; 2]) -> Option<usize> {
    (0..takes.min(n)).find(|&k| {
        let [x0, y0, x1, y1] = tile(n, k);
        (x0..x1).contains(&pos[0]) && (y0..y1).contains(&pos[1])
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::Quality;
    use crate::engine::plan;
    use crate::engine::worker::Worker;
    use crate::project::Take;
    use eframe::egui::Color32;
    use std::sync::Arc;

    #[test]
    fn grid_layout() {
        assert_eq!([2, 3, 4, 5, 9, 10].map(grid_size), [(2, 1), (2, 2), (2, 2), (3, 2), (3, 3), (4, 3)]);
        assert_eq!(tile(4, 3), [0.5, 0.5, 1.0, 1.0]);
        // Two takes side by side at half size, centred vertically.
        assert_eq!(tile(2, 1), [0.5, 0.25, 1.0, 0.75]);
        assert_eq!((hit(4, 4, [0.7, 0.2]), hit(4, 3, [0.7, 0.7]), hit(2, 2, [0.2, 0.1])), (Some(1), None, None));
    }

    /// Solid colour media for tests: (media, its colour).
    fn solid(id: &str, preset: &str) -> MediaItem {
        use crate::engine::plugins::{find, new_fx};
        let s = find("milan.gen.solid").unwrap();
        MediaItem { id: id.into(), name: id.into(), media_type: MediaType::Image, duration: 86400.0, generator: Some(new_fx(s, s.presets.iter().find(|p| p.name == preset))), ..Default::default() }
    }

    fn frame(p: &Project, t: f64) -> Vec<Color32> {
        let mut wk = Worker::new();
        let vp = plan::compile(p);
        let n = (t * vp.fps).round() as i64;
        wk.set_plan(Arc::new(vp));
        wk.frame(n, 64, 36, Quality::Good, None).0.pixels.clone()
    }

    /// The grid shows each take in its tile, the empty one black, and leaves the timeline as it is elsewhere.
    #[test]
    fn grid_shows_every_take_in_its_tile() {
        let mut p = Project::new("g");
        p.settings.frame_rate = 30.0;
        let (red, blue, green) = (solid("r", "Red"), solid("b", "Blue"), solid("g", "Green"));
        let mut c = Clip::from_media(&red, 0.0);
        c.duration = 4.0;
        c.takes = vec![Take { media_id: "b".into(), in_point: 0.0, name: "b".into() }, Take { media_id: String::new(), in_point: 0.0, name: String::new() }];
        let mut after = Clip::from_media(&green, 4.0);
        after.duration = 2.0;
        p.timeline.video_tracks[0].clips = vec![c, after];
        p.media_pool = vec![red, blue, green];
        let q = grid(&p);
        let px = |img: &[Color32], x: usize, y: usize| img[y * 64 + x];
        let img = frame(&q, 1.0);
        assert_eq!((px(&img, 16, 9), px(&img, 48, 9), px(&img, 16, 27), px(&img, 48, 27)), (Color32::RED, Color32::BLUE, Color32::BLACK, Color32::BLACK));
        assert_eq!(px(&frame(&q, 5.0), 32, 18), Color32::GREEN, "no multicamera event there: the timeline");
        let a = at(&p, 1.0).unwrap();
        assert_eq!((a.n, a.active, a.names), (3, 0, vec![Some("r".into()), Some("b".into()), None]));
        assert!(at(&p, 5.0).is_none());
    }

    /// Acceptance: Expand to Multiple Tracks gives each camera's track back: two tracks made one multicamera track,
    /// a take changed at 2 s, expanded keeping the unused takes, show (unmuted, each on its own) the frames the original
    /// tracks did.
    #[test]
    fn expand_restores_the_original_tracks() {
        let mut p = Project::new("x");
        p.settings.frame_rate = 30.0;
        let (red, blue) = (solid("r", "Red"), solid("b", "Blue"));
        let mut a = Clip::from_media(&red, 0.0);
        a.duration = 4.0;
        let mut b = Clip::from_media(&blue, 1.0);
        b.duration = 4.0;
        let ids = vec![a.id.clone(), b.id.clone()];
        p.timeline.video_tracks[0].clips = vec![a];
        p.timeline.add_video_track();
        p.timeline.video_tracks[1].clips = vec![b];
        p.media_pool = vec![red, blue];
        let only = |p: &Project, k: usize| {
            let mut q = p.clone();
            for (i, t) in q.timeline.video_tracks.iter_mut().enumerate() {
                t.visible = i == k;
                t.clips.iter_mut().for_each(|c| c.muted = false);
            }
            q
        };
        let times = [0.5, 1.5, 2.5, 3.5, 4.5, 5.5];
        let before: Vec<Vec<Vec<Color32>>> = (0..2).map(|k| times.iter().map(|&t| frame(&only(&p, k), t)).collect()).collect();
        let mut m = p.clone();
        edit::create_multicam(&mut m, &ids).unwrap();
        assert_eq!(m.timeline.video_tracks.len(), 1, "the emptied camera track goes");
        assert!(edit::switch_take(&mut m, true, 2.0, None, 1));
        let id = m.timeline.video_tracks[0].id.clone();
        assert!(edit::expand_multicam(&mut m, &[id], true));
        assert_eq!(m.timeline.video_tracks.len(), 2);
        for k in 0..2 {
            let after: Vec<Vec<Color32>> = times.iter().map(|&t| frame(&only(&m, k), t)).collect();
            assert!(after == before[k], "track {k}");
        }
        // The cut shows as it was made: red to 2 s, blue after.
        assert_eq!((frame(&m, 1.5)[0], frame(&m, 2.5)[0], frame(&m, 4.5)[0]), (Color32::RED, Color32::BLUE, Color32::BLUE));
    }
}
