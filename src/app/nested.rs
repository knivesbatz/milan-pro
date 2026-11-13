//! R6b: nested timelines in the window, as VEGAS 23 has them: Create Nested Timeline, Open Nested Timeline (also Edit
//! Source Project) and Open Parent Timeline, through a stack of the projects a nested timeline was opened from
//! (`MeridianApp::parents`; the title bar names the one shown).
use std::path::{Path, PathBuf};
use crate::edit;
use crate::engine::nested;
use crate::project::Project;
use crate::ui::MenuAction;
use super::{MeridianApp, Parent};

impl MeridianApp {
    /// The files of the projects open around the timeline shown, outermost first: none of them may be nested in it.
    fn inside(&self) -> Vec<PathBuf> {
        self.parents.iter().filter_map(|p| p.project.path.clone()).collect()
    }

    /// The names of the projects from the top one to the one shown (the timeline's breadcrumb).
    pub(super) fn crumbs(&self) -> Vec<String> {
        if self.parents.is_empty() {
            return Vec::new();
        }
        self.parents.iter().map(|p| &p.project.name).chain([&self.project.name]).cloned().collect()
    }

    /// Edit › Create Nested Timeline (Alt+C): as in VEGAS, a parent never saved is saved first, the nested timeline is
    /// saved under the name asked for (proposed: "<parent>_Untitled Timeline.mnle" beside it), the selected events become
    /// one nested event pair (one undo step; `edit::create_nested`), and the new nested timeline opens.
    pub(super) fn create_nested(&mut self) {
        let ids = self.sel.clips.clone();
        if ids.is_empty() {
            return;
        }
        if self.project.path.is_none() {
            let r = crate::ui::menus::save_as(&mut self.project);
            if !self.save_result(r) {
                return;
            }
        }
        let Some(parent) = self.project.path.clone() else { return };
        let proposed = edit::nested_file_name(&parent);
        let mut dialog = rfd::FileDialog::new().set_title("Save Nested Timeline").add_filter("Milan Pro Project", &["mnle"]);
        if let (Some(dir), Some(name)) = (proposed.parent(), proposed.file_name()) {
            dialog = dialog.set_directory(dir).set_file_name(name.to_string_lossy());
        }
        let Some(path) = dialog.save_file() else { return };
        let path = with_mnle(path);
        let (before, inside) = (self.project.clone(), self.inside());
        match edit::create_nested(&mut self.project, &ids, &path, &inside) {
            Ok(new) => {
                self.history.push(before);
                (self.sel.clips, self.sel.list_focus, self.sel.env) = (new, false, None);
                // Its events kept their times, so the cursor shows the same moment there.
                let at = self.project.timeline.cursor_position;
                self.open_nested(&path, at);
            }
            Err(e) => self.message = Some(format!("Create Nested Timeline: {e}")),
        }
    }

    /// Open Nested Timeline (Alt+N) and Edit Source Project on the focus event: its nested timeline at the moment the
    /// cursor is over in it (its start when the cursor is outside the event).
    pub(super) fn open_focus_nested(&mut self) {
        let Some(c) = self.sel.clips.last().and_then(|id| self.project.clip(id)) else { return };
        let Some(m) = edit::media_of(&self.project, c).filter(|m| m.nested) else { return };
        let local = (self.project.timeline.cursor_position - c.position).clamp(0.0, c.duration);
        let local = if c.reverse { c.duration - local } else { local };
        let path = m.path.clone();
        self.open_nested(&path, m.file_time(c.in_point + local * c.rate));
    }

    /// The nested timeline at `path` replaces the timeline shown, which waits with its undo history and selection for
    /// Open Parent Timeline; its cursor goes to `at`. A project the shown one is inside is refused, as the engine shows
    /// it offline.
    pub(super) fn open_nested(&mut self, path: &Path, at: f64) {
        let me = nested::canon(path);
        if self.inside().iter().chain(&self.project.path).any(|p| nested::canon(p) == me) {
            self.message = Some(format!("{}: {}", path.display(), nested::SELF_REF));
            return;
        }
        let mut child = match Project::load(&path.to_path_buf()) {
            Ok(p) => p,
            Err(e) => return self.message = Some(format!("Could not open {}: {e}", path.display())),
        };
        child.timeline.cursor_position = at.max(0.0);
        // shortcut: autosave backs up only the project shown, so a waiting parent's unsaved edits are not backed up
        // until it is shown again; give `Autosave` one timer per project if crash recovery of parents is asked for.
        let parent = Parent {
            project: std::mem::replace(&mut self.project, Project::new("")),
            history: std::mem::take(&mut self.history),
            sel: std::mem::take(&mut self.sel),
            scroll_y: self.timeline_scroll_y,
        };
        self.parents.push(parent);
        self.switch_project(child);
        self.timeline_scroll_y = 0.0;
    }

    /// Open Parent Timeline, once the save prompt for the nested timeline is answered: the parent comes back as it was
    /// left (it shows the nested timeline as now saved). Then on up, or the New/Open/Exit that is waiting.
    pub(super) fn back_to_parent(&mut self, ctx: &eframe::egui::Context) {
        let Some(p) = self.parents.pop() else { return };
        self.switch_project(p.project);
        (self.history, self.sel, self.timeline_scroll_y) = (p.history, p.sel, p.scroll_y);
        if let Some((depth, then)) = self.unwind.take() {
            if self.parents.len() > depth {
                self.unwind = Some((depth, then));
                self.request(ctx, MenuAction::OpenParent);
            } else if let Some(a) = then {
                self.request(ctx, a);
            }
        }
    }

    /// A breadcrumb click: up to the project at `level` (0 = the top one).
    pub(super) fn up_to(&mut self, ctx: &eframe::egui::Context, level: usize) {
        if level < self.parents.len() {
            self.unwind = Some((level, None));
            self.request(ctx, MenuAction::OpenParent);
        }
    }
}

/// What New, Open, Exit, an imported project or Open Parent Timeline does first (`MeridianApp::request`).
#[derive(Debug, PartialEq)]
pub(super) enum Step {
    /// Inside a nested timeline: Open Parent first, up through every project (each asking to save), then this.
    Up,
    /// Ask "Do you want to save the changes?" first.
    Ask,
    Run,
    /// Open Parent with no parent.
    Nothing,
}

/// `action` with the project shown `modified`, `parents` levels down.
pub(super) fn step(action: MenuAction, modified: bool, parents: usize) -> Step {
    match (action == MenuAction::OpenParent, parents) {
        (false, 1..) => Step::Up,
        (true, 0) => Step::Nothing,
        _ if modified => Step::Ask,
        _ => Step::Run,
    }
}

/// `path` with the project extension, which a typed name may lack.
fn with_mnle(path: PathBuf) -> PathBuf {
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("mnle")) { path } else { PathBuf::from(format!("{}.mnle", path.display())) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Open Parent asks to save a modified nested timeline (and goes straight up from a saved one); New, Open and Exit
    /// inside a nested timeline go up first, so every project above asks too; without a parent, nothing happens.
    #[test]
    fn open_parent_asks_to_save_a_modified_child() {
        assert_eq!(step(MenuAction::OpenParent, true, 1), Step::Ask);
        assert_eq!(step(MenuAction::OpenParent, false, 2), Step::Run);
        assert_eq!(step(MenuAction::OpenParent, true, 0), Step::Nothing);
        for a in [MenuAction::New, MenuAction::Open, MenuAction::Exit, MenuAction::ImportVegas] {
            assert_eq!(step(a, false, 1), Step::Up);
            assert_eq!((step(a, true, 0), step(a, false, 0)), (Step::Ask, Step::Run));
        }
    }

    #[test]
    fn a_typed_name_gets_the_project_extension() {
        assert_eq!(super::with_mnle(PathBuf::from(r"C:\a\Show_Untitled Timeline")), PathBuf::from(r"C:\a\Show_Untitled Timeline.mnle"));
        assert_eq!(super::with_mnle(PathBuf::from(r"C:\a\b.MNLE")), PathBuf::from(r"C:\a\b.MNLE"));
        assert_eq!(super::with_mnle(PathBuf::from(r"C:\a\v1.2")), PathBuf::from(r"C:\a\v1.2.mnle"));
    }
}
