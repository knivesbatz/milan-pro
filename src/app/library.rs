//! R45b: the media library's app state and dialogs: Missing Media (offered on Open, as VEGAS's "Media offline"
//! does, and from Project Media), File Format Properties, the duplicate question on import, and the import counter.
use eframe::egui;
use std::collections::VecDeque;
use crate::project::MediaItem;
use crate::ui::dialogs::{self, DupChoice, MissingResult};
use super::MeridianApp;

#[derive(Default)]
pub struct Lib {
    pub missing: Option<dialogs::MissingMedia>,
    pub file_props: Option<dialogs::FileProps>,
    /// Imported files that repeat a pool item, asked one at a time: (new item, the pool item's id and name).
    pub dups: VecDeque<(MediaItem, String, String)>,
    pub dup_ask: Option<dialogs::DuplicateAsk>,
    /// "Do the same for the others": the answer kept for the rest of the queue.
    pub dup_all: Option<DupChoice>,
}

impl Lib {
    pub fn modal(&self) -> bool {
        self.missing.is_some() || self.file_props.is_some() || self.dup_ask.is_some()
    }
}

impl MeridianApp {
    /// Project Media › Find Missing Media, and after Open: the files are looked for again, and the dialog lists those
    /// still gone (or says there are none).
    pub(super) fn open_missing_media(&mut self, quiet: bool) {
        // Not an edit: the marks are found again on every open.
        crate::library::refresh_missing(&mut self.project.media_pool);
        self.lib.missing = dialogs::MissingMedia::new(&self.project);
        if self.lib.missing.is_none() && !quiet {
            self.message = Some("Find Missing Media: every media file of the project is where it was.".into());
        }
    }

    pub(super) fn open_file_props(&mut self, ctx: &egui::Context, id: &str) {
        if let Some(m) = self.project.media_pool.iter().find(|m| m.id == id) {
            self.lib.file_props = Some(dialogs::FileProps::new(ctx, &m.name, &m.path));
        }
    }

    /// Imported items that duplicate pool items wait for the question; the rest go on to be added.
    pub(super) fn hold_duplicates(&mut self, ok: Vec<MediaItem>) -> Vec<MediaItem> {
        let dups = self.importer.take_duplicates();
        if dups.is_empty() {
            return ok;
        }
        let mut rest = Vec::new();
        for m in ok {
            match dups.iter().find(|(p, _)| *p == m.path) {
                Some((_, k)) => self.lib.dups.push_back((m, k.id.clone(), k.name.clone())),
                None => rest.push(m),
            }
        }
        rest
    }

    pub(super) fn show_library(&mut self, ctx: &egui::Context) {
        if let Some(d) = &mut self.lib.missing {
            match d.show(ctx) {
                Some(MissingResult::Relink(found)) => {
                    let before = self.project.clone();
                    let mut changed = false;
                    for (id, path) in &found {
                        changed |= crate::library::relink(&mut self.project, id, path);
                    }
                    if changed {
                        self.history.push(before);
                        self.message = Some(format!("Relinked {} media file(s).", found.len()));
                    }
                    // Items never probed (a VEGAS import's missing files) are probed now, filled in place.
                    let probe: Vec<_> = self.project.media_pool.iter().filter(|m| m.is_offline() && m.unsupported.is_none() && m.path.exists()).map(|m| m.path.clone()).collect();
                    if !probe.is_empty() {
                        self.import_new(ctx, probe);
                    }
                    self.lib.missing = None;
                }
                Some(MissingResult::Close) => self.lib.missing = None,
                None => {}
            }
        }
        if let Some(d) = &mut self.lib.file_props {
            if d.show(ctx) {
                self.lib.file_props = None;
            }
        }
        // Duplicates: one question at a time, unless "do the same" was ticked.
        if self.lib.dup_ask.is_none() {
            if let Some((item, id, existing)) = self.lib.dups.pop_front() {
                match self.lib.dup_all {
                    Some(c) => self.answer_duplicate(item, c),
                    None => self.lib.dup_ask = Some(dialogs::DuplicateAsk { item, existing, existing_id: id, all: false, more: self.lib.dups.len() }),
                }
            } else {
                self.lib.dup_all = None;
            }
        }
        if let Some(d) = &mut self.lib.dup_ask {
            if let Some(c) = d.show(ctx) {
                if d.all {
                    self.lib.dup_all = Some(c);
                }
                let (item, existing) = (d.item.clone(), d.existing_id.clone());
                self.lib.dup_ask = None;
                self.answer_duplicate(item, c);
                if c == DupChoice::UseExisting {
                    self.selected_media = self.project.media_pool.iter().position(|m| m.id == existing);
                }
            }
        }
    }

    fn answer_duplicate(&mut self, item: MediaItem, c: DupChoice) {
        if c == DupChoice::AddAnyway {
            let before = self.project.clone();
            if crate::media::merge_new(&mut self.project.media_pool, vec![item]) {
                self.history.push(before);
                self.project.mark_modified();
            }
        }
    }

    /// R45b: the import counter for the status bar, from Import Media and from drops.
    pub(super) fn import_progress(&self) -> Option<String> {
        let a = self.importer.progress();
        let b = self.file_drop.progress();
        let sum = match (a, b) {
            (Some(x), Some(y)) => Some((x.0 + y.0, x.1 + y.1)),
            (x, y) => x.or(y),
        };
        sum.map(crate::media::progress_text)
    }

    pub(super) fn cancel_import(&self) {
        self.importer.cancel();
        self.file_drop.cancel();
    }
}
