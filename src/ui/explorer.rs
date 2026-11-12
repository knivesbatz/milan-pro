//! Explorer window (§5.5): a real file-system browser. Toolbar with back / forward / up and an address field,
//! folder tree (drives and folders, expanded lazily) beside the folder's subfolders and media files.
//! Double-click a media file to import it and open it in the Trimmer; drag it onto the timeline to add it.
use eframe::egui::*;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use super::icons::{Icon, paint_icon};
use super::toolbar::{arrow_menu, split_button, tool_button};
use super::widgets::{self, ROW_H};
use super::{MediaDrag, WinAction, theme};

struct Entry {
    path: PathBuf,
    name: String,
    is_dir: bool,
    size: u64,
}

pub struct Explorer {
    /// None = the drive list ("Computer").
    dir: Option<PathBuf>,
    back: Vec<Option<PathBuf>>,
    fwd: Vec<Option<PathBuf>>,
    entries: Option<Result<Vec<Entry>, String>>,
    address: String,
    tree_w: f32,
    open: HashSet<PathBuf>,
    subdirs: HashMap<PathBuf, Vec<PathBuf>>,
    selected: Option<PathBuf>,
    details: bool,
    /// Drive roots, probed at start and on Refresh (a disconnected drive can be slow to probe).
    drives: Vec<PathBuf>,
}

impl Default for Explorer {
    fn default() -> Self {
        let start = std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("Videos")).filter(|p| p.is_dir());
        let mut e = Self {
            dir: None,
            back: Vec::new(),
            fwd: Vec::new(),
            entries: None,
            address: String::new(),
            tree_w: 180.0,
            open: HashSet::new(),
            subdirs: HashMap::new(),
            selected: None,
            details: false,
            drives: drives(),
        };
        e.set_dir(start);
        e
    }
}

pub fn is_media(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| crate::media::classify_extension(&e.to_ascii_lowercase()) != crate::project::MediaType::Unknown)
}

/// Existing drive roots, A:\ … Z:\.
fn drives() -> Vec<PathBuf> {
    (b'A'..=b'Z').map(|c| PathBuf::from(format!("{}:\\", c as char))).filter(|p| p.is_dir()).collect()
}

/// Subfolders and media files of `dir` (hidden dot-folders skipped), folders first, by name.
fn list_dir(dir: &Path) -> Result<Vec<Entry>, String> {
    let rd = std::fs::read_dir(dir).map_err(|e| format!("Cannot read {}: {e}", dir.display()))?;
    let mut v: Vec<Entry> = rd
        .flatten()
        .filter_map(|d| {
            let path = d.path();
            let md = d.metadata().ok()?;
            let name = d.file_name().to_string_lossy().into_owned();
            let is_dir = md.is_dir();
            (!name.starts_with('.') && (is_dir || is_media(&path))).then_some(Entry { path, name, is_dir, size: md.len() })
        })
        .collect();
    v.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(v)
}

fn display_name(p: &Path) -> String {
    p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned())
}

impl Explorer {
    pub fn at(dir: PathBuf) -> Self {
        let mut e = Self::default();
        e.set_dir(Some(dir));
        e
    }

    fn set_dir(&mut self, dir: Option<PathBuf>) {
        self.address = dir.as_ref().map_or("Computer".into(), |d| d.display().to_string());
        self.entries = dir.as_deref().map(list_dir);
        self.dir = dir;
        self.selected = None;
    }

    fn go(&mut self, dir: Option<PathBuf>) {
        if dir != self.dir {
            self.back.push(self.dir.clone());
            self.fwd.clear();
            self.set_dir(dir);
        }
    }

    fn refresh(&mut self) {
        self.subdirs.clear();
        self.drives = drives();
        self.set_dir(self.dir.clone());
    }

    fn children(&mut self, p: &Path) -> &[PathBuf] {
        self.subdirs.entry(p.to_path_buf()).or_insert_with(|| {
            list_dir(p).map(|v| v.into_iter().filter(|e| e.is_dir).map(|e| e.path).collect()).unwrap_or_default()
        })
    }
}

pub fn draw_explorer_toolbar(ui: &mut Ui, ex: &mut Explorer) {
    if tool_button(ui, Icon::Back, "Back", !ex.back.is_empty(), false).clicked() {
        if let Some(d) = ex.back.pop() {
            ex.fwd.push(ex.dir.clone());
            ex.set_dir(d);
        }
    }
    if tool_button(ui, Icon::Forward, "Forward", !ex.fwd.is_empty(), false).clicked() {
        if let Some(d) = ex.fwd.pop() {
            ex.back.push(ex.dir.clone());
            ex.set_dir(d);
        }
    }
    if tool_button(ui, Icon::Up, "Up One Level", ex.dir.is_some(), false).clicked() {
        let up = ex.dir.as_deref().and_then(Path::parent).map(Path::to_path_buf);
        ex.go(up);
    }
    ui.add_space(3.0);
    // Address combo: type a path and press Enter, or pick a drive from ▾.
    let resp = widgets::search_field(ui, &mut ex.address, "Path", 220.0);
    if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
        let p = PathBuf::from(ex.address.trim());
        if p.is_dir() {
            ex.go(Some(p));
        } else {
            ex.address = ex.dir.as_ref().map_or("Computer".into(), |d| d.display().to_string());
        }
    }
    let arrow = tool_button(ui, Icon::DropDown, "Drives", true, false);
    if let Some(Some(d)) = arrow_menu(ui, &arrow, |ui| {
        let mut pick = None;
        if ui.button("Computer").clicked() {
            pick = Some(None);
        }
        for d in ex.drives.clone() {
            if ui.button(d.display().to_string()).clicked() {
                pick = Some(Some(d));
            }
        }
        pick
    }) {
        ex.go(d);
    }
    if tool_button(ui, Icon::Refresh, "Refresh", true, false).clicked() {
        ex.refresh();
    }
    tool_button(ui, Icon::Delete, "Delete (disabled: Milan Pro never deletes your files)", false, false);
    tool_button(ui, Icon::Favorite, "Add to Favorites (not available yet)", false, false);
    tool_button(ui, Icon::Play, "Start Preview (not available yet)", false, false);
    tool_button(ui, Icon::Stop, "Stop Preview (not available yet)", false, false);
    tool_button(ui, Icon::AutoPreview, "Auto Preview (not available yet)", false, true);
    let (m, a) = split_button(ui, Icon::Views, "Views", true, false);
    let mut pick = |ui: &mut Ui| {
        if ui.radio(!ex.details, "List").clicked() {
            ex.details = false;
        }
        if ui.radio(ex.details, "Details").clicked() {
            ex.details = true;
        }
    };
    if arrow_menu(ui, &a, &mut pick).is_none() {
        arrow_menu(ui, &m, &mut pick);
    }
}

pub fn draw_explorer(ui: &mut Ui, ex: &mut Explorer, actions: &mut Vec<WinAction>) {
    let area = ui.max_rect();
    let mut tree_w = ex.tree_w;
    let (left, right) = widgets::pane_split(ui, area, &mut tree_w, "explorer_tree");
    ex.tree_w = tree_w;
    let mut nav = None;
    widgets::scroll_pane(ui, left, "explorer_tree", |ui| {
        let (r, _) = widgets::tree_row(ui, 0, Some(true), Some(Icon::ExternalMonitor), "Computer", ex.dir.is_none());
        if r.clicked() {
            nav = Some(None);
        }
        for d in ex.drives.clone() {
            tree_node(ui, ex, d, 1, &mut nav);
        }
    });
    if let Some(d) = nav {
        ex.go(d);
    }

    let mut open_dir = None;
    widgets::scroll_pane(ui, right, "explorer_files", |ui| {
        let t = theme::get();
        let rows: Vec<(PathBuf, String, bool, u64)> = match &ex.entries {
            None => ex.drives.iter().map(|d| (d.clone(), d.display().to_string(), true, 0)).collect(),
            Some(Ok(v)) => v.iter().map(|e| (e.path.clone(), e.name.clone(), e.is_dir, e.size)).collect(),
            Some(Err(e)) => {
                ui.add_space(8.0);
                ui.label(RichText::new(e).color(t.warning_text));
                return;
            }
        };
        let cols = [("Name", 260.0), ("Size", 90.0), ("Type", 90.0)];
        if ex.details {
            widgets::header_row(ui, &cols);
        }
        for (path, name, is_dir, size) in rows {
            let sel = ex.selected.as_ref() == Some(&path);
            let r = widgets::list_row(ui, ROW_H, sel);
            let icon = if is_dir { Icon::Folder } else if is_audio(&path) { Icon::TrackAudio } else { Icon::TrackVideo };
            paint_icon(ui.painter(), Rect::from_min_size(pos2(r.rect.min.x + 6.0, r.rect.center().y - 8.0), vec2(16.0, 16.0)), icon, t.look(), None);
            let color = if sel { Color32::WHITE } else { t.text };
            if ex.details {
                let kind = if is_dir { "Folder".to_owned() } else { path.extension().map(|e| e.to_string_lossy().to_uppercase()).unwrap_or_default() };
                let size = if is_dir { String::new() } else { format!("{} KB", size.div_ceil(1024)) };
                let rest = Rect::from_min_max(pos2(r.rect.min.x + 22.0, r.rect.min.y), r.rect.max);
                widgets::row_cells(ui, rest, &cols, &[name, size, kind], color);
            } else {
                ui.painter().text(pos2(r.rect.min.x + 28.0, r.rect.center().y), Align2::LEFT_CENTER, &name, t.font(t.font.ui), color);
            }
            if r.clicked() {
                ex.selected = Some(path.clone());
            }
            if is_dir {
                if r.double_clicked() {
                    open_dir = Some(path);
                }
                continue;
            }
            if r.double_clicked() {
                actions.push(WinAction::Import(vec![path.clone()]));
                actions.push(WinAction::OpenTrimmer(path.clone()));
            }
            r.dnd_set_drag_payload(MediaDrag::File(path.clone()));
            if r.dragged() {
                ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
            }
            super::menus::context(&r, |ui| {
                if ui.button("Add to Project Media").clicked() {
                    actions.push(WinAction::Import(vec![path.clone()]));
                    ui.close_menu();
                }
                if ui.button("Open in Trimmer").clicked() {
                    actions.push(WinAction::Import(vec![path.clone()]));
                    actions.push(WinAction::OpenTrimmer(path.clone()));
                    ui.close_menu();
                }
            });
        }
    });
    if let Some(d) = open_dir {
        ex.go(Some(d));
    }
}

fn is_audio(p: &Path) -> bool {
    let e = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    ["mp3", "wav", "flac", "aac", "ogg", "wma", "aiff", "opus"].contains(&e.as_str())
}

/// One folder of the tree and, when expanded, its subfolders (listed on first expand).
fn tree_node(ui: &mut Ui, ex: &mut Explorer, path: PathBuf, depth: usize, nav: &mut Option<Option<PathBuf>>) {
    let open = ex.open.contains(&path);
    let sel = ex.dir.as_ref() == Some(&path);
    let (r, toggled) = widgets::tree_row(ui, depth, Some(open), Some(Icon::Folder), &display_name(&path), sel);
    if toggled {
        if open {
            ex.open.remove(&path);
        } else {
            ex.open.insert(path.clone());
        }
    } else if r.clicked() {
        *nav = Some(Some(path.clone()));
    }
    if open && depth < 24 {
        for c in ex.children(&path).to_vec() {
            tree_node(ui, ex, c, depth + 1, nav);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_folders_first_and_only_media() {
        let dir = std::env::temp_dir().join(format!("milan-explorer-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("b_sub")).unwrap();
        for f in ["z.mp4", "a.WAV", "notes.txt"] {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        let names: Vec<String> = list_dir(&dir).unwrap().into_iter().map(|e| e.name).collect();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(names, ["b_sub", "a.WAV", "z.mp4"]);
    }

    #[test]
    fn camera_and_phone_formats_are_media() {
        for f in ["00001.MTS", "a.m2ts", "memo.m4a", "clip.m4v", "x.3gp", "scan.tif", "t.tga", "s.ac3"] {
            assert!(is_media(Path::new(f)), "{f}");
        }
        assert!(!is_media(Path::new("notes.txt")));
    }
}
