//! R49: preferences taking effect (Preferences OK / Apply, Import Preferences), Options › Export / Import Preferences
//! (VEGAS 23's `.vegprefs`; Milan Pro's are `.milanprefs`, JSON), and the track header's Edit Visible Button Set.
use eframe::egui;
use crate::settings::Prefs;
use crate::ui::theme;
use super::MeridianApp;

/// The file type Export / Import Preferences offer.
const EXT: &str = "milanprefs";

impl MeridianApp {
    /// `new` becomes the preferences: everything that follows them is told, and they are saved.
    pub(super) fn apply_prefs(&mut self, ctx: &egui::Context, new: Prefs) {
        if new.track_colors != self.prefs.track_colors {
            theme::set_palette(ctx, new.palette());
        }
        if new.record_input != self.prefs.record_input && self.rec_start.is_none() {
            (self.input, self.input_failed) = (None, false); // the meter reopens on the new input
        }
        // R34a: new VST folders are searched.
        let rescan = new.ofx.vst_folders != self.prefs.ofx.vst_folders;
        // An import may bring other layouts: every slot either side is written.
        if new.layouts != self.prefs.layouts {
            self.layouts_changed.extend(new.layouts.keys().chain(self.prefs.layouts.keys()).cloned());
        }
        self.prefs = new;
        self.save_prefs();
        // R44: the logger follows the preferences at once.
        crate::logger::set_level(crate::logger::level_from_name(&self.prefs.log_level));
        crate::logger::set_privacy(self.prefs.crash_privacy);
        if rescan {
            self.plugin_mgr.rescan(Some(ctx), &self.prefs.ofx, false);
        }
        crate::engine::gpu::choose(&self.prefs.gpu);
        crate::engine::hw::set_prefs(&self.prefs.hw_decode, &self.prefs.hw_encode);
        crate::engine::caches::configure(&self.prefs);
        // R49: what an import can change besides the Preferences dialog's pages.
        crate::ui::keymap::set_overrides(&self.prefs.keys);
        crate::project::set_default_track_fx(self.prefs.default_track_fx.clone());
        crate::project::set_default_video_track(self.prefs.default_video_track.clone());
        crate::edit::set_normalize_peak(self.prefs.normalize_peak_db);
    }

    /// Options › Export Preferences...: every preference into a file the user picks (asked before replacing one).
    pub(super) fn export_prefs(&mut self) {
        let Some(path) = rfd::FileDialog::new().set_title("Export Preferences").add_filter("Milan Pro Preferences", &[EXT]).set_file_name(format!("Milan Pro.{EXT}")).save_file() else { return };
        let path = if path.extension().is_none() { path.with_extension(EXT) } else { path };
        if path.exists()
            && rfd::MessageDialog::new()
                .set_title("Export Preferences")
                .set_description(format!("{} already exists. Replace it?", path.display()))
                .set_buttons(rfd::MessageButtons::YesNo)
                .show()
                != rfd::MessageDialogResult::Yes
        {
            return;
        }
        self.message = Some(match self.prefs.export(&path) {
            Ok(()) => format!("Preferences exported to {}.", path.display()),
            Err(e) => format!("Could not export the preferences: {e}"),
        });
    }

    /// Options › Import Preferences...: the preferences of a file the user picks replace these (asked first).
    pub(super) fn import_prefs(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new().set_title("Import Preferences").add_filter("Milan Pro Preferences", &[EXT]).pick_file() else { return };
        let new = match Prefs::import(&path) {
            Ok(p) => p,
            Err(e) => return self.message = Some(format!("Could not import {}: {e}", path.display())),
        };
        if rfd::MessageDialog::new()
            .set_title("Import Preferences")
            .set_description(format!("Replace all of your preferences (window layouts, keyboard, toolbars and templates included) with those in {}?", path.display()))
            .set_buttons(rfd::MessageButtons::YesNo)
            .show()
            != rfd::MessageDialogResult::Yes
        {
            return;
        }
        self.apply_prefs(ctx, new);
        // The window layout the file kept comes back as it was left.
        if let Some(l) = self.prefs.layouts.get(&crate::settings::layout_key("last")).cloned() {
            self.dock.apply(&l, &theme::get());
        }
        self.message = Some(format!("Preferences imported from {}.", path.display()));
    }
}
