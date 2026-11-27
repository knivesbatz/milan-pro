//! R45b: the Missing Media dialog (VEGAS's "Media offline" on Open, and Project Media › Find Missing Media): the
//! offline items with where they were, "Search Folder..." (recursively, by name and size, on a worker), per-item
//! "Browse...", then "Relink" for every item found, or "Leave Offline". Also the question asked when an imported
//! file is already in the project under another name.
use eframe::egui::*;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Receiver;
use crate::library::{self, Wanted};
use crate::project::Project;
use crate::ui::widgets;

pub struct MissingRow {
    pub id: String,
    pub name: String,
    pub was: PathBuf,
    pub wanted: Wanted,
    /// Where it was found (by the folder search or Browse).
    pub found: Option<PathBuf>,
}

pub struct MissingMedia {
    pub rows: Vec<MissingRow>,
    sel: Option<usize>,
    /// A folder search running: its folder, the rows it looks for, the result and its Cancel.
    search: Option<(PathBuf, Vec<usize>, Receiver<Vec<Option<PathBuf>>>, Arc<AtomicBool>)>,
    pub note: String,
}

/// What the dialog asks the app to do.
pub enum MissingResult {
    /// Point each media id at its file (one undo step).
    Relink(Vec<(String, PathBuf)>),
    Close,
}

impl MissingMedia {
    /// The offline items of `project` whose file is gone; None when there are none.
    pub fn new(project: &Project) -> Option<Self> {
        let rows: Vec<MissingRow> = project
            .media_pool
            .iter()
            .filter(|m| m.missing && m.subclip.is_none())
            .map(|m| MissingRow { id: m.id.clone(), name: m.name.clone(), was: m.path.clone(), wanted: Wanted::of(m), found: None })
            .collect();
        (!rows.is_empty()).then(|| Self { rows, sel: Some(0), search: None, note: String::new() })
    }

    /// Search `dir` for the rows not found yet, on a worker thread.
    pub fn search_folder(&mut self, ctx: &Context, dir: PathBuf) {
        let todo: Vec<usize> = (0..self.rows.len()).filter(|&k| self.rows[k].found.is_none()).collect();
        let wanted: Vec<Wanted> = todo.iter().map(|&k| self.rows[k].wanted.clone()).collect();
        let (tx, rx) = std::sync::mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let (stop, ctx, d) = (cancel.clone(), ctx.clone(), dir.clone());
        std::thread::Builder::new()
            .name("milan-relink".into())
            .spawn(move || {
                let _ = tx.send(library::find_in_folder(&d, &wanted, &stop));
                ctx.request_repaint();
            })
            .ok();
        self.note = format!("Searching {}...", dir.display());
        self.search = Some((dir, todo, rx, cancel));
    }

    /// The harness: the search done at once.
    pub fn search_now(&mut self, dir: &std::path::Path) {
        let todo: Vec<usize> = (0..self.rows.len()).filter(|&k| self.rows[k].found.is_none()).collect();
        let wanted: Vec<Wanted> = todo.iter().map(|&k| self.rows[k].wanted.clone()).collect();
        let found = library::find_in_folder(dir, &wanted, &AtomicBool::new(false));
        self.take_found(dir, &todo, found);
    }

    fn take_found(&mut self, dir: &std::path::Path, todo: &[usize], found: Vec<Option<PathBuf>>) {
        let mut n = 0;
        for (&k, f) in todo.iter().zip(found) {
            if f.is_some() {
                n += 1;
            }
            if let Some(row) = self.rows.get_mut(k) {
                row.found = row.found.take().or(f);
            }
        }
        self.note = format!("{n} of {} found in {}.", todo.len(), dir.display());
    }

    fn poll(&mut self) {
        let Some((_, _, rx, _)) = &self.search else { return };
        if let Ok(found) = rx.try_recv() {
            if let Some((dir, todo, _, _)) = self.search.take() {
                self.take_found(&dir, &todo, found);
            }
        }
    }

    pub fn show(&mut self, ctx: &Context) -> Option<MissingResult> {
        self.poll();
        let n_found = self.rows.iter().filter(|r| r.found.is_some()).count();
        let busy = self.search.is_some();
        let (hit, close) = super::modal(ctx, "missing_media", "Media Offline", vec2(860.0, 420.0), |ui| {
            let d = crate::ui::theme::get().dialog();
            ui.label(format!("{} media file(s) used by this project could not be found. Search a folder for them, browse to each, or leave them offline.", self.rows.len()));
            ui.add_space(4.0);
            const COLS: [(&str, f32); 3] = [("Name", 170.0), ("Original location", 300.0), ("Status", 360.0)];
            super::field_box(ui, 0, |ui| {
                ui.set_width(ui.available_width());
                ui.set_height(250.0);
                widgets::header_row(ui, &COLS);
                ScrollArea::vertical().auto_shrink([false, false]).max_height(228.0).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (k, row) in self.rows.iter().enumerate() {
                        let on = self.sel == Some(k);
                        let r = widgets::list_row(ui, widgets::ROW_H, on);
                        let status = match &row.found {
                            Some(p) => format!("Found: {}", p.display()),
                            None => "Offline".to_string(),
                        };
                        let was = row.was.parent().map(|p| p.display().to_string()).unwrap_or_default();
                        widgets::row_cells(ui, r.rect, &COLS, &[row.name.clone(), was, status], if on { Color32::WHITE } else { d.text });
                        if r.clicked() {
                            self.sel = Some(k);
                        }
                    }
                });
            });
            ui.add_space(4.0);
            ui.label(RichText::new(if self.note.is_empty() { " " } else { &self.note }).color(d.text_disabled));
            ui.add_space(6.0);
            let mut act = None;
            ui.horizontal(|ui| {
                if super::sized_button(ui, "Search Folder...", vec2(110.0, 23.0), !busy).clicked() {
                    act = Some(0);
                }
                if super::sized_button(ui, "Browse...", vec2(80.0, 23.0), self.sel.is_some() && !busy).clicked() {
                    act = Some(1);
                }
                if busy && super::sized_button(ui, "Stop", vec2(60.0, 23.0), true).clicked() {
                    act = Some(4);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if super::button(ui, "Leave Offline", true, false).clicked() {
                        act = Some(3);
                    }
                    if super::classic_button(ui, &format!("Relink ({n_found})"), vec2(90.0, 23.0), n_found > 0 && !busy, true).clicked() {
                        act = Some(2);
                    }
                });
            });
            act
        });
        match hit {
            Some(0) => {
                // The folder picker is the user's own Windows dialog, opened on their click.
                if let Some(dir) = rfd::FileDialog::new().set_title("Search Folder for Missing Media").pick_folder() {
                    self.search_folder(ctx, dir);
                }
                None
            }
            Some(1) => {
                if let Some(k) = self.sel {
                    let name = self.rows[k].wanted.name.clone();
                    if let Some(p) = rfd::FileDialog::new().set_title(format!("Find {name}")).set_file_name(&name).pick_file() {
                        self.rows[k].found = Some(p);
                    }
                }
                None
            }
            Some(2) => Some(MissingResult::Relink(self.rows.iter().filter_map(|r| Some((r.id.clone(), r.found.clone()?))).collect())),
            Some(4) => {
                if let Some((_, _, _, stop)) = &self.search {
                    stop.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                None
            }
            Some(_) => Some(MissingResult::Close),
            None if close && !busy => Some(MissingResult::Close),
            None => None,
        }
    }
}

/// R45b: an imported file already in the project (same content, another path).
pub struct DuplicateAsk {
    /// The new item and the name of the pool item it repeats.
    pub item: crate::project::MediaItem,
    pub existing: String,
    pub existing_id: String,
    /// "Do the same for the other duplicates of this import".
    pub all: bool,
    pub more: usize,
}

/// The answer: add the new file anyway, or keep using the item already there.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DupChoice {
    AddAnyway,
    UseExisting,
}

impl DuplicateAsk {
    pub fn show(&mut self, ctx: &Context) -> Option<DupChoice> {
        let (hit, close) = super::modal(ctx, "duplicate_media", "Media Already in Project", vec2(460.0, 170.0), |ui| {
            ui.label(format!("{} is already in the project as {}.", self.item.path.display(), self.existing));
            ui.label("It has the same size and the same content at its start and end.");
            ui.add_space(6.0);
            if self.more > 0 {
                widgets::checkbox(ui, &mut self.all, format!("Do the same for the other {} duplicate(s)", self.more));
            } else {
                ui.add_space(18.0);
            }
            ui.add_space(8.0);
            super::button_row(ui, &[("Use Existing", true), ("Add Anyway", true)])
        });
        match hit {
            Some(0) => Some(DupChoice::UseExisting),
            Some(_) => Some(DupChoice::AddAnyway),
            None if close => Some(DupChoice::UseExisting),
            None => None,
        }
    }
}
