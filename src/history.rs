use crate::project::Project;

const CAP: usize = 100;

/// Snapshot-based undo/redo. Snapshots hold the whole serializable project;
/// view state (path, cursor, zoom, scroll) is carried over from the live project on restore.
#[derive(Default)]
pub struct History {
    undo: Vec<Project>,
    redo: Vec<Project>,
    /// Changes with every push, undo, redo and clear.
    generation: u64,
}

impl History {
    /// Record the state *before* a mutation.
    pub fn push(&mut self, before: Project) {
        self.undo.push(before);
        if self.undo.len() > CAP {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.generation += 1;
    }

    pub fn undo(&mut self, project: &mut Project) -> bool {
        let Some(prev) = self.undo.pop() else { return false };
        self.redo.push(restore(project, prev));
        self.generation += 1;
        true
    }

    pub fn redo(&mut self, project: &mut Project) -> bool {
        let Some(next) = self.redo.pop() else { return false };
        self.undo.push(restore(project, next));
        self.generation += 1;
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Steps available to undo / redo (for the toolbar's ▾ history lists).
    pub fn lens(&self) -> (usize, usize) {
        (self.undo.len(), self.redo.len())
    }

    /// A number that changes whenever the history does: an edit, undo or redo since it was read is any change.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.generation += 1;
    }
}

/// Swap `snap` into `project`, keeping view state; returns the replaced project.
fn restore(project: &mut Project, mut snap: Project) -> Project {
    snap.path = project.path.clone();
    snap.name = project.name.clone(); // derived from the path
    // Track heights are view state too.
    let live = &project.timeline;
    let heights: std::collections::HashMap<&str, _> = live.video_tracks.iter().chain(&live.audio_tracks).map(|t| (t.id.as_str(), t.height)).collect();
    for t in snap.timeline.video_tracks.iter_mut().chain(&mut snap.timeline.audio_tracks) {
        if let Some(&h) = heights.get(t.id.as_str()) {
            t.height = h;
        }
    }
    snap.timeline.cursor_position = project.timeline.cursor_position;
    snap.timeline.zoom = project.timeline.zoom;
    snap.timeline.scroll_x = project.timeline.scroll_x;
    snap.modified = true;
    std::mem::replace(project, snap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_redo_roundtrip_keeps_view_state() {
        let mut h = History::default();
        let mut p = Project::new("t");
        h.push(p.clone());
        p.timeline.add_video_track();
        assert_eq!(p.timeline.video_tracks.len(), 2);

        p.timeline.cursor_position = 7.0;
        assert!(h.undo(&mut p));
        assert_eq!(p.timeline.video_tracks.len(), 1);
        assert_eq!(p.timeline.cursor_position, 7.0);
        assert!(!h.can_undo() && h.can_redo());

        assert!(h.redo(&mut p));
        assert_eq!(p.timeline.video_tracks.len(), 2);
        assert!(!h.redo(&mut p));

        // a new edit clears redo
        h.undo(&mut p);
        h.push(p.clone());
        assert!(!h.can_redo());
    }

    #[test]
    fn undo_keeps_track_heights_and_the_name() {
        let mut h = History::default();
        let mut p = Project::new("Untitled");
        h.push(p.clone());
        p.timeline.markers.push(crate::project::Marker { position: 1.0, name: String::new(), color: [0; 3] });
        p.timeline.video_tracks[0].height += 40.0;
        p.name = "Show".into();
        let height = p.timeline.video_tracks[0].height;
        assert!(h.undo(&mut p));
        assert!(p.timeline.markers.is_empty());
        assert_eq!((p.timeline.video_tracks[0].height, p.name.as_str()), (height, "Show"));
    }

    #[test]
    fn cap_is_enforced() {
        let mut h = History::default();
        let p = Project::new("t");
        for _ in 0..CAP + 10 {
            h.push(p.clone());
        }
        assert_eq!(h.undo.len(), CAP);
    }
}
