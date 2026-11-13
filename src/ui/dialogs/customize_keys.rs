//! Options › Customize Keyboard (R15), as VEGAS's: the keyboard map ([Default] or a map saved as an .ini file, with
//! Save As and Delete), the commands filtered by "Show commands containing", the selected command's keys with Remove
//! and Reset, and a Shortcut keys box that takes the next key pressed, says which command has it and Adds it (the
//! other command loses it). It edits the current look's keys; OK keeps them. A map copied into the maps folder is listed
//! the next time the dialog opens. VEGAS's per-window contexts are not here: Milan Pro's keys work everywhere.
use std::path::{Path, PathBuf};
use eframe::egui::*;
use crate::ui::keymap::{self, Bindings, Command, Overrides};
use crate::ui::menus;
use crate::ui::theme::{self, UiLook};

const HEADER: &str = "Milan Pro Keyboard Map";

/// A keyboard map file: its name, then each look's changed keys as `Command=Key` lines (one line per key; `Command=`
/// for a command with none).
pub fn write_map(name: &str, keys: &Overrides) -> String {
    let mut s = format!("[{HEADER}]\r\nName={name}\r\n");
    for (look, cmds) in keys {
        s += &format!("[{look}]\r\n");
        for (c, ks) in cmds {
            if ks.is_empty() {
                s += &format!("{c}=\r\n");
            }
            for k in ks {
                s += &format!("{c}={k}\r\n");
            }
        }
    }
    s
}

/// The name and keys of a keyboard map file; lines it does not know are skipped.
pub fn read_map(text: &str) -> (Option<String>, Overrides) {
    let (mut name, mut keys, mut section) = (None, Overrides::new(), String::new());
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with(';')) {
        if let Some(s) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = s.to_string();
        } else if let Some((k, v)) = line.split_once('=') {
            // A key may be "=" itself: split at the first one, command names have none.
            if section == HEADER {
                if k == "Name" && !v.trim().is_empty() {
                    name = Some(v.trim().to_string());
                }
            } else if !section.is_empty() {
                let ks = keys.entry(section.clone()).or_default().entry(k.trim().to_string()).or_default();
                if !v.is_empty() {
                    ks.push(v.to_string());
                }
            }
        }
    }
    (name, keys)
}

/// The maps in `dir` as (name, file), by name.
fn list_maps(dir: Option<&Path>) -> Vec<(String, PathBuf)> {
    let Some(rd) = dir.and_then(|d| std::fs::read_dir(d).ok()) else { return Vec::new() };
    let mut maps: Vec<(String, PathBuf)> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ini")))
        .filter_map(|p| {
            let text = std::fs::read_to_string(&p).ok()?;
            let name = read_map(&text).0.unwrap_or_else(|| p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
            Some((name, p))
        })
        .collect();
    maps.sort();
    maps
}

pub struct CustomizeKeys {
    look: UiLook,
    /// Every look's keys as edited, and this look's keys with them applied.
    pub keys: Overrides,
    binds: Bindings,
    /// The keyboard map chosen (None = [Default]).
    pub map: Option<String>,
    dir: Option<PathBuf>,
    maps: Vec<(String, PathBuf)>,
    commands: Vec<(String, Command)>,
    filter: String,
    sel: Option<Command>,
    /// Scroll the command list to `sel` (set by `select`).
    reveal: bool,
    key_sel: Option<usize>,
    /// The key pressed in the Shortcut keys box.
    key: String,
    /// Save As: the name being typed.
    naming: Option<String>,
    error: Option<String>,
}

impl CustomizeKeys {
    /// `dir` holds the keyboard maps (None: none are listed or saved).
    pub fn new(keys: &Overrides, map: Option<String>, look: UiLook, dir: Option<PathBuf>) -> Self {
        Self {
            look,
            keys: keys.clone(),
            binds: keymap::bindings(keys, look),
            map,
            maps: list_maps(dir.as_deref()),
            dir,
            commands: menus::named_commands(look),
            filter: String::new(),
            sel: None,
            reveal: false,
            key_sel: None,
            key: String::new(),
            naming: None,
            error: None,
        }
    }

    /// Select `c` and scroll the list to it.
    pub fn select(&mut self, c: Command) {
        self.sel = Some(c);
        self.key_sel = None;
        self.reveal = true;
    }

    /// The keys `c` has as edited.
    pub fn keys_of(&self, c: Command) -> Vec<String> {
        self.binds.iter().find(|b| b.0 == c).map(|b| b.1.clone()).unwrap_or_default()
    }

    fn name_of(&self, c: Command) -> String {
        self.commands.iter().find(|n| n.1 == c).map_or_else(|| keymap::name(c), |n| n.0.clone())
    }

    /// Give `c` exactly `keys`; keys equal to the defaults leave no override.
    fn set(&mut self, c: Command, keys: Vec<String>) {
        let look = keymap::look_name(self.look);
        let mine = self.keys.entry(look.to_string()).or_default();
        if keys.iter().eq(keymap::default_keys(c, self.look)) {
            mine.remove(&keymap::name(c));
        } else {
            mine.insert(keymap::name(c), keys);
        }
        if mine.is_empty() {
            self.keys.remove(look);
        }
        self.binds = keymap::bindings(&self.keys, self.look);
    }

    /// Bind `key` to `c`, taking it (and any key it clashes with) from the commands that have it.
    pub fn add(&mut self, c: Command, key: &str) {
        for (o, k) in keymap::conflicts(&self.binds, key, c) {
            let rest = self.keys_of(o).into_iter().filter(|x| *x != k).collect();
            self.set(o, rest);
        }
        let mut ks = self.keys_of(c);
        if !ks.iter().any(|k| k == key) {
            ks.push(key.to_string());
            self.set(c, ks);
        }
    }

    pub fn remove(&mut self, c: Command, k: usize) {
        let mut ks = self.keys_of(c);
        if k < ks.len() {
            ks.remove(k);
            self.set(c, ks);
        }
    }

    /// `c` gets its default keys back (from whichever commands took them).
    pub fn reset(&mut self, c: Command) {
        self.set(c, Vec::new());
        for k in keymap::default_keys(c, self.look) {
            self.add(c, k);
        }
    }

    /// [Default]: every look's keys as the table has them.
    pub fn reset_all(&mut self) {
        self.keys.clear();
        self.map = None;
        self.binds = keymap::bindings(&self.keys, self.look);
    }

    pub fn load_map(&mut self, name: &str, file: &Path) {
        match std::fs::read_to_string(file) {
            Ok(text) => {
                self.keys = read_map(&text).1;
                self.map = Some(name.to_string());
                self.binds = keymap::bindings(&self.keys, self.look);
            }
            Err(e) => self.error = Some(format!("Could not read {}: {e}", file.display())),
        }
    }

    /// Save As: the keys as edited, in `dir`\`name`.ini (the file name keeps letters, digits, spaces, - and _).
    pub fn save_as(&mut self, name: &str) -> Result<(), String> {
        let name = name.trim();
        let stem: String = name.chars().map(|c| if c.is_alphanumeric() || " -_".contains(c) { c } else { '_' }).collect();
        let Some(dir) = self.dir.clone().filter(|_| !stem.trim().is_empty()) else { return Err("Type a name for the keyboard map.".into()) };
        let file = dir.join(format!("{}.ini", stem.trim()));
        std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&file, write_map(name, &self.keys))).map_err(|e| format!("Could not save {}: {e}", file.display()))?;
        self.maps = list_maps(Some(&dir));
        self.map = Some(name.to_string());
        Ok(())
    }

    /// Delete: the chosen map's file ([Default] cannot be deleted). The keys stay as they are.
    pub fn delete_map(&mut self) {
        let Some(file) = self.maps.iter().find(|m| Some(&m.0) == self.map.as_ref()).map(|m| m.1.clone()) else { return };
        if let Err(e) = std::fs::remove_file(&file) {
            self.error = Some(format!("Could not delete {}: {e}", file.display()));
            return;
        }
        self.maps = list_maps(self.dir.as_deref());
        self.map = None;
    }

    /// Some(true) on OK, Some(false) on Cancel or ×.
    pub fn show(&mut self, ctx: &Context) -> Option<bool> {
        let (hit, close) = super::modal(ctx, "customize_keys", "Customize Keyboard", vec2(680.0, 540.0), |ui| {
            let d = theme::get().dialog();
            ui.horizontal(|ui| {
                ui.label("Keyboard map:");
                let shown = self.map.clone().unwrap_or_else(|| "[Default]".into());
                let mut pick = None;
                ComboBox::from_id_salt("key_map").width(260.0).selected_text(shown).show_ui(ui, |ui| {
                    if ui.selectable_label(self.map.is_none(), "[Default]").clicked() {
                        pick = Some(None);
                    }
                    for (n, p) in &self.maps {
                        if ui.selectable_label(self.map.as_ref() == Some(n), n).clicked() {
                            pick = Some(Some((n.clone(), p.clone())));
                        }
                    }
                });
                match pick {
                    Some(None) => self.reset_all(),
                    Some(Some((n, p))) => self.load_map(&n, &p),
                    None => {}
                }
                if super::button(ui, "Save As...", self.dir.is_some(), false).clicked() {
                    self.naming = Some(self.map.clone().unwrap_or_default());
                }
                let deletable = self.maps.iter().any(|m| Some(&m.0) == self.map.as_ref());
                if super::button(ui, "Delete", deletable, false).clicked() {
                    self.delete_map();
                }
            });
            if let Some(mut name) = self.naming.take() {
                let mut keep = true;
                ui.horizontal(|ui| {
                    ui.label("Save the keyboard map as:");
                    ui.add(TextEdit::singleline(&mut name).desired_width(240.0));
                    if super::button(ui, "Save", !name.trim().is_empty(), true).clicked() {
                        keep = false;
                        if let Err(e) = self.save_as(&name) {
                            self.error = Some(e);
                        }
                    }
                    if super::button(ui, "Cancel", true, false).clicked() {
                        keep = false;
                    }
                });
                if keep {
                    self.naming = Some(name);
                }
            }
            ui.horizontal(|ui| {
                ui.label("Show commands containing:");
                ui.add(TextEdit::singleline(&mut self.filter).desired_width(300.0));
            });
            ui.add_space(2.0);
            let list_frame = Frame::NONE.fill(d.field).stroke(Stroke::new(1.0_f32, d.field_border)).inner_margin(2);
            ui.horizontal_top(|ui| {
                list_frame.show(ui, |ui| {
                    ui.set_width(360.0);
                    ScrollArea::vertical().id_salt("key_commands").max_height(330.0).auto_shrink(false).show(ui, |ui| {
                        ui.with_layout(eframe::egui::Layout::top_down_justified(Align::LEFT), |ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            let f = self.filter.to_lowercase();
                            let mut pick = None;
                            for (n, c) in self.commands.iter().filter(|(n, _)| n.to_lowercase().contains(&f)) {
                                let r = ui.selectable_label(self.sel == Some(*c), n.as_str());
                                if self.reveal && self.sel == Some(*c) {
                                    r.scroll_to_me(Some(Align::Center));
                                }
                                if r.clicked() {
                                    pick = Some(*c);
                                }
                            }
                            self.reveal = false;
                            if let Some(c) = pick {
                                (self.sel, self.key_sel) = (Some(c), None);
                            }
                        });
                    });
                });
                ui.vertical(|ui| {
                    ui.set_width(280.0);
                    ui.label("Shortcut keys for selected command:");
                    let keys = self.sel.map(|c| self.keys_of(c)).unwrap_or_default();
                    list_frame.show(ui, |ui| {
                        ui.set_width(270.0);
                        ui.set_min_height(110.0);
                        ui.with_layout(eframe::egui::Layout::top_down_justified(Align::LEFT), |ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            for (k, key) in keys.iter().enumerate() {
                                if ui.selectable_label(self.key_sel == Some(k), keymap::fmt_key(key)).clicked() {
                                    self.key_sel = Some(k);
                                }
                            }
                        });
                    });
                    ui.horizontal(|ui| {
                        if super::button(ui, "Remove", self.sel.is_some() && self.key_sel.is_some_and(|k| k < keys.len()), false).clicked() {
                            if let (Some(c), Some(k)) = (self.sel, self.key_sel.take()) {
                                self.remove(c, k);
                            }
                        }
                        if super::button(ui, "Reset", self.sel.is_some(), false).clicked() {
                            if let Some(c) = self.sel {
                                self.reset(c);
                            }
                        }
                    });
                    ui.add_space(8.0);
                    ui.label("Shortcut keys:");
                    ui.horizontal(|ui| {
                        self.capture_box(ui);
                        if super::button(ui, "Add", self.sel.is_some() && !self.key.is_empty(), false).clicked() {
                            if let Some(c) = self.sel {
                                let k = std::mem::take(&mut self.key);
                                self.add(c, &k);
                            }
                        }
                    });
                    let users: Vec<String> = match self.sel.filter(|_| !self.key.is_empty()) {
                        Some(c) => keymap::conflicts(&self.binds, &self.key, c).iter().map(|(o, _)| self.name_of(*o)).collect(),
                        None => Vec::new(),
                    };
                    ui.add_space(4.0);
                    ui.label("Shortcut currently used by:");
                    let used = if users.is_empty() { "(none)".to_string() } else { users.join(", ") };
                    ui.add(Label::new(RichText::new(used).color(if users.is_empty() { d.text_disabled } else { d.text })).wrap());
                });
            });
            ui.add_space(6.0);
            if let Some(e) = &self.error {
                ui.label(e.as_str());
            }
            super::button_row(ui, &[("OK", true), ("Cancel", true)])
        });
        match hit {
            Some(0) => Some(true),
            Some(_) => Some(false),
            None if close => Some(false),
            None => None,
        }
    }

    /// The Shortcut keys box: click it, then press the keys (Esc closes the dialog, as VEGAS's does).
    fn capture_box(&mut self, ui: &mut Ui) {
        let d = theme::get().dialog();
        let id = ui.id().with("key_capture");
        // Before the box takes this frame's input, so Space or Enter are captured rather than clicking it.
        if ui.memory(|m| m.has_focus(id)) {
            ui.memory_mut(|m| m.set_focus_lock_filter(id, EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: false }));
            if let Some(k) = keymap::capture(ui.ctx()) {
                self.key = k;
            }
        }
        let (_, r) = ui.allocate_space(vec2(190.0, 21.0));
        let resp = ui.interact(r, id, Sense::click());
        if resp.clicked() {
            resp.request_focus();
        }
        let focused = resp.has_focus();
        ui.painter().rect(r, 0.0, d.field, Stroke::new(1.0_f32, if focused { d.accent } else { d.field_border }), StrokeKind::Inside);
        let (text, color) = match (self.key.is_empty(), focused) {
            (false, _) => (keymap::fmt_key(&self.key), d.text),
            (true, true) => ("Press a key".to_string(), d.text_disabled),
            (true, false) => ("Click here, then press a key".to_string(), d.text_disabled),
        };
        ui.painter().text(r.left_center() + vec2(4.0, 0.0), Align2::LEFT_CENTER, text, FontId::proportional(12.0), color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Command::*;

    fn dialog(dir: Option<PathBuf>) -> CustomizeKeys {
        CustomizeKeys::new(&Overrides::new(), None, UiLook::Vegas23, dir)
    }

    /// Rebinding takes the key from its old command; Remove, Reset (the default comes back from whoever took it) and
    /// [Default] restore.
    #[test]
    fn rebind_conflict_and_reset() {
        let mut d = dialog(None);
        d.add(Split, "Ctrl+O");
        assert_eq!(d.keys_of(Split), ["S", "Ctrl+O"]);
        assert!(d.keys_of(Open).is_empty(), "Open loses Ctrl+O");
        assert_eq!(d.keys["vp23"].len(), 2);
        d.add(Split, "Alt+D");
        assert!(d.keys_of(LayoutDefault).is_empty() && d.keys_of(LayoutUser(3)).is_empty(), "the Alt+D chords go");
        d.remove(Split, 0);
        assert_eq!(d.keys_of(Split), ["Ctrl+O", "Alt+D"]);
        d.add(NextTool, "S");
        d.reset(Open);
        assert_eq!((d.keys_of(Open), d.keys_of(Split)), (vec!["Ctrl+O".to_string()], vec!["Alt+D".to_string()]));
        d.reset(Split);
        assert_eq!((d.keys_of(Split), d.keys_of(NextTool)), (vec!["S".to_string()], vec!["D".to_string()]));
        d.reset_all();
        assert!(d.keys.is_empty() && d.keys_of(LayoutDefault) == ["Alt+D D"]);
        // A command the table has no keys for takes one; back to none, no override is left.
        d.add(Preferences, "Ctrl+Alt+F9");
        assert_eq!(d.keys_of(Preferences), ["Ctrl+Alt+F9"]);
        d.remove(Preferences, 0);
        assert!(d.keys.is_empty());
    }

    /// Save As writes a map that reads back the same; it is listed by its name, loads, and Delete removes the file.
    #[test]
    fn keyboard_maps_save_load_and_delete() {
        let dir = std::env::temp_dir().join(format!("milan-keymaps-test-{}", std::process::id()));
        let mut d = dialog(Some(dir.clone()));
        d.add(Split, "Ctrl+=");
        d.add(Undo, "=");
        d.remove(Undo, 0);
        d.remove(Undo, 0);
        d.keys.entry("vp16".into()).or_default().insert("Save".into(), vec!["F4".into()]);
        let text = write_map("My Keys: v2", &d.keys);
        assert_eq!(read_map(&text), (Some("My Keys: v2".to_string()), d.keys.clone()));
        d.save_as("My Keys: v2").unwrap();
        assert!(dir.join("My Keys_ v2.ini").is_file());
        assert_eq!(d.map.as_deref(), Some("My Keys: v2"));
        let saved = d.keys.clone();
        let mut e = dialog(Some(dir.clone()));
        assert_eq!(e.maps.iter().map(|m| m.0.as_str()).collect::<Vec<_>>(), ["My Keys: v2"]);
        let (n, p) = e.maps[0].clone();
        e.load_map(&n, &p);
        assert_eq!((&e.keys, e.keys_of(Split), e.keys_of(Undo)), (&saved, vec!["S".to_string(), "Ctrl+=".to_string()], vec!["=".to_string()]));
        assert!(e.keys_of(PitchUp).is_empty() && e.keys_of(PitchCentUp).is_empty(), "= and Ctrl+= were taken from the pitch keys");
        e.delete_map();
        assert!(e.maps.is_empty() && e.map.is_none() && !p.exists());
        assert!(e.save_as("  ").is_err() && dialog(None).save_as("x").is_err());
        let _ = std::fs::remove_dir(&dir);
        // Unknown lines are skipped; a map without a name is listed by its file name.
        assert_eq!(read_map("junk\n[vp23]\nSplit=F5\n=x\n[Other]\n").1["vp23"]["Split"], ["F5"]);
    }
}
