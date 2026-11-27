//! R47: the command palette, Milan Pro's own addition (VEGAS has none). One searchable list over every `Command`
//! (named by its menu path, `menus::named_commands`, and by its `keymap::TABLE` row), every plug-in the choosers
//! offer, the saved window layouts and the recent projects. Typing filters by fuzzy subsequence (`score`); Enter runs
//! the top row, ↑ ↓ pick another, Esc closes. Rows show their shortcut; rows the app cannot run now are greyed and say
//! why on hover (R0), the same `Why` the menus give. The default key is `KEY`, a chord no VEGAS row uses (a test keeps
//! it so); Options › Customize Keyboard rebinds it like any command. Drawn with the dialogs' frame (the slate
//! caption) and the results in a navy list well.
use eframe::egui::*;
use super::dialogs;
use super::keymap::{self, Command};
use super::menus::{self, CmdState};
use super::theme;
use super::widgets;
use crate::engine::plugins::{self, PluginInfo, PluginKind};

/// The default chord (`keymap::TABLE` row of `Command::CommandPalette`). Ctrl+Shift+P is VEGAS 23's Text to Speech
/// and Ctrl+P the Trimmer's Show Video in Preview, so neither is taken.
pub const KEY: &str = "Ctrl+Alt+P";

/// What a row does when chosen.
#[derive(Clone, Copy, Debug)]
pub enum Action {
    Run(Command),
    /// Apply this plug-in as a chooser double-click does (video FX on the focus video event, a transition on its
    /// start, generated media at the cursor, a compositor on the focus video track, audio FX on the focus audio event).
    Plugin(&'static PluginInfo),
}

impl PartialEq for Action {
    fn eq(&self, o: &Self) -> bool {
        match (self, o) {
            (Action::Run(a), Action::Run(b)) => a == b,
            (Action::Plugin(a), Action::Plugin(b)) => a.id == b.id,
            _ => false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Item {
    /// The row's text: the menu label without '&' and "...", a plug-in's name, a layout's or project's name.
    pub label: String,
    /// Where it comes from, in dim text: the menu path ("Edit › Group"), "Video FX", "Layout", "Recent project"...
    pub group: String,
    /// A second name the search also matches: the keymap row's name (Help › Keyboard Shortcuts wording).
    pub alias: String,
    pub shortcut: String,
    pub action: Action,
    pub enabled: bool,
    /// Why it is greyed (empty when enabled).
    pub why: String,
}

/// Why `c` is greyed right now: the R0 reason when it is unfinished, else what it is waiting for.
fn why_disabled(c: Command, st: &CmdState) -> String {
    if let Some(w) = menus::unfinished(c) {
        return w.text();
    }
    use Command::*;
    let s = match c {
        Undo => "Nothing to undo",
        Redo => "Nothing to redo",
        Paste | PasteInsert | PasteRepeat | PasteAttributes | PasteAttributesSelect => "The clipboard holds no events",
        OpenRecent(_) => "No such recent project",
        LayoutUser(_) => "This layout slot is empty",
        RollBack => "No earlier version is kept",
        _ if !st.clip && !st.tracks_sel => "Needs a selected event or track",
        _ if !st.clip => "Needs a selected event",
        _ => "Not available in the project's present state",
    };
    s.to_string()
}

fn clean(path: &str) -> String {
    path.trim_end_matches("...").to_string()
}

/// Every row the palette offers in the state `st`, in menu order: commands, then plug-ins, then layouts
/// and recent projects.
pub fn items(st: &CmdState) -> Vec<Item> {
    use Command::*;
    let mut out = Vec::new();
    for (path, c) in menus::named_commands() {
        // Layouts and recent projects get their own rows below, with their names.
        if matches!(c, CommandPalette | LayoutUser(_) | SaveLayoutUser(_) | OpenRecent(_)) {
            continue;
        }
        let (group, label) = match path.rsplit_once('.') {
            Some((g, l)) => (g.replace('.', " › "), clean(l)),
            None => ("Keyboard".to_string(), clean(&path)),
        };
        let alias = keymap::TABLE.iter().find(|r| r.0 == c).map(|r| r.1.to_string()).filter(|a| *a != label).unwrap_or_default();
        let enabled = st.enabled(c);
        out.push(Item {
            label,
            group,
            alias,
            shortcut: keymap::shortcut_text(c),
            action: Action::Run(c),
            enabled,
            why: if enabled { String::new() } else { why_disabled(c, st) },
        });
    }
    let kinds = [
        (PluginKind::VideoFx, "Video FX", st.video_clip, "Select a video event first"),
        (PluginKind::Transition, "Transition", st.video_clip, "Select a video event first"),
        (PluginKind::Generator, "Media Generator", st.video_track_focus, "Select a video track first"),
        (PluginKind::Compositor, "Compositor", st.video_track_focus, "Select a video track first"),
        (PluginKind::AudioFx, "Audio FX", st.audio_clip, "Select an audio event first"),
    ];
    for (kind, group, enabled, why) in kinds {
        for info in plugins::catalog(kind) {
            out.push(Item {
                label: info.name.to_string(),
                group: group.to_string(),
                alias: info.group.to_string(),
                shortcut: String::new(),
                action: Action::Plugin(info),
                enabled,
                why: if enabled { String::new() } else { why.to_string() },
            });
        }
    }
    for (n, name) in st.layouts.iter().enumerate() {
        let Some(name) = name else { continue };
        out.push(Item {
            label: name.clone(),
            group: "Layout".into(),
            alias: format!("User layout {n}"),
            shortcut: keymap::shortcut_text(LayoutUser(n)),
            action: Action::Run(LayoutUser(n)),
            enabled: true,
            why: String::new(),
        });
    }
    for (i, p) in st.recent.iter().enumerate() {
        let name = std::path::Path::new(p).file_name().map_or(p.clone(), |f| f.to_string_lossy().into_owned());
        out.push(Item { label: name, group: "Recent project".into(), alias: p.clone(), shortcut: String::new(), action: Action::Run(OpenRecent(i)), enabled: true, why: String::new() });
    }
    out
}

/// How well `query` matches `text` as a subsequence, case-insensitively; None when it does not. Higher is better:
/// a match at the start of the text or of a word scores high, consecutive matches add up, every skipped character
/// costs one, and the whole query as a prefix or a word-start substring is worth more still. Non-alphanumeric query characters are
/// skipped, so "track motion" and "trackmotion" match alike.
pub fn score(query: &str, text: &str) -> Option<i32> {
    let q: Vec<char> = query.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect();
    if q.is_empty() {
        return Some(0);
    }
    let t: Vec<char> = text.chars().collect();
    let tl: Vec<char> = t.iter().flat_map(|c| c.to_lowercase().next()).collect();
    if tl.len() != t.len() || tl.len() > 400 {
        return None;
    }
    // Bonus of matching at text position i: first character, a word start (after a space, a punctuation mark or a
    // lowercase-to-uppercase step), else nothing.
    let bonus = |i: usize| -> i32 {
        if i == 0 {
            8
        } else if !t[i - 1].is_alphanumeric() || (t[i - 1].is_lowercase() && t[i].is_uppercase()) {
            6
        } else {
            0
        }
    };
    // best[ti]: the best score with query[qi] matched at text position ti (None = impossible); rolled per qi.
    let mut best: Vec<Option<i32>> = (0..tl.len()).map(|ti| (tl[ti] == q[0]).then(|| 1 + bonus(ti) - (ti as i32).min(3))).collect();
    for &qc in &q[1..] {
        let mut next: Vec<Option<i32>> = vec![None; tl.len()];
        let mut run: Option<(usize, i32)> = None; // the best earlier match so far: (position, score)
        for ti in 0..tl.len() {
            if tl[ti] == qc {
                if let Some((tj, s)) = run {
                    // Consecutive characters add up; every skipped one costs, so the tightest alignment wins
                    // ("Track Motion" over "Time Select Right by Marker/Region" on "tm").
                    let gap = (ti - tj - 1) as i32;
                    let link = if gap == 0 { 4 } else { -gap };
                    next[ti] = Some(s + link + 1 + bonus(ti));
                }
            }
            if let Some(s) = best[ti] {
                // A later start only replaces an earlier one when it scores at least as well: ties keep the
                // tighter (consecutive) link the earlier position may still make.
                if run.is_none_or(|(_, rs)| s > rs || (s == rs && ti > 0)) {
                    run = Some((ti, s));
                }
            }
        }
        best = next;
    }
    let mut s = best.into_iter().flatten().max()?;
    // The whole query in one piece: at the start, at a word start ("mo" in "Track Motion"), or inside a word
    // ("tm" in "Adjustment", worth little).
    let (ql, tls): (String, String) = (q.iter().collect(), tl.iter().collect());
    if tls.starts_with(&ql) {
        s += 20;
    } else if let Some(at) = tls.find(&ql) {
        let word_start = tls[..at].chars().next_back().is_some_and(|c| !c.is_alphanumeric());
        s += if word_start { 10 } else { 3 };
    }
    Some(s)
}

/// The rows of `items` that match `query`, best first (ties alphabetically, so "Track Motion" comes before "Track
/// Mute" on "tm" and "Undo" before "Undo All"); every row, in order, for an empty query.
pub fn rank(items: &[Item], query: &str) -> Vec<usize> {
    if query.chars().all(|c| !c.is_alphanumeric()) {
        return (0..items.len()).collect();
    }
    let mut hits: Vec<(i32, usize)> = items
        .iter()
        .enumerate()
        .filter_map(|(i, it)| {
            let a = score(query, &it.label);
            let b = if it.alias.is_empty() { None } else { score(query, &it.alias).map(|s| s - 2) };
            let g = score(query, &format!("{} {}", it.group, it.label)).map(|s| s - 4);
            [a, b, g].into_iter().flatten().max().map(|s| (s, i))
        })
        .collect();
    let key = |i: usize| (items[i].label.to_lowercase(), items[i].group.to_lowercase());
    hits.sort_by(|x, y| y.0.cmp(&x.0).then_with(|| key(x.1).cmp(&key(y.1))));
    hits.into_iter().map(|h| h.1).collect()
}

/// The palette's state between frames.
#[derive(Default)]
pub struct Palette {
    pub open: bool,
    query: String,
    sel: usize,
    /// The selection moved by key: scroll the list to it.
    reveal: bool,
    /// Just opened: the search box takes focus and its text is selected.
    fresh: bool,
}

const ROW_H: f32 = 22.0;
const SIZE: Vec2 = vec2(640.0, 460.0);

impl Palette {
    /// Open with `query` typed (the harness opens it on "tm").
    pub fn open_with(&mut self, query: &str) {
        self.open = true;
        self.query = query.to_string();
        self.sel = 0;
        self.fresh = true;
        self.reveal = true;
    }

    /// Draw the palette while open. The action chosen this frame, if any; the palette closes with it.
    pub fn show(&mut self, ctx: &Context, st: &CmdState) -> Option<Action> {
        if !self.open {
            return None;
        }
        let items = items(st);
        let order = rank(&items, &self.query);
        // Keys the list takes before the text box sees them.
        let (down, up, pgdn, pgup, enter) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::PageDown),
                i.consume_key(Modifiers::NONE, Key::PageUp),
                i.consume_key(Modifiers::NONE, Key::Enter),
            )
        });
        let page = 15;
        let n = order.len();
        if n > 0 {
            let moved = match (down, up, pgdn, pgup) {
                (true, ..) => Some((self.sel + 1).min(n - 1)),
                (_, true, ..) => Some(self.sel.saturating_sub(1)),
                (_, _, true, _) => Some((self.sel + page).min(n - 1)),
                (_, _, _, true) => Some(self.sel.saturating_sub(page)),
                _ => None,
            };
            if let Some(m) = moved {
                self.sel = m;
                self.reveal = true;
            }
        }
        self.sel = self.sel.min(n.saturating_sub(1));
        let mut chosen = None;
        let t = theme::get();
        let d = t.dialog();
        let (_, close) = dialogs::modal(ctx, "command_palette", "Command Palette", SIZE, |ui| {
            let before = self.query.clone();
            let field = widgets::search_field(ui, &mut self.query, "Type a command, plug-in, layout or project name", ui.available_width());
            if self.fresh {
                field.request_focus();
                self.fresh = false;
            } else if ctx.memory(|m| m.focused().is_none()) {
                field.request_focus();
            }
            if self.query != before {
                self.sel = 0;
                self.reveal = true;
            }
            ui.add_space(4.0);
            let list_h = SIZE.y - dialogs::title_h() - 16.0 - 30.0 - 4.0 - 26.0;
            // The results in a navy list well (UI_SPEC.md §8.2), LCD ink, the selection bar.
            let well = Frame::NONE.fill(t.lcd.well).corner_radius(t.m.r_ctl).inner_margin(3).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ScrollArea::vertical().id_salt("palette_rows").max_height(list_h).min_scrolled_height(list_h).auto_shrink(false).show(ui, |ui| {
                    ui.set_min_height(list_h - 4.0);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    if order.is_empty() {
                        ui.add_space(8.0);
                        ui.label(RichText::new(format!("Nothing matches \"{}\".", self.query)).color(t.lcd.text));
                    }
                    for (k, &i) in order.iter().enumerate() {
                        let it = &items[i];
                        let selected = k == self.sel;
                        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
                        if selected {
                            widgets::selection_bar(ui.painter(), rect, t.m.r_small as f32);
                            if self.reveal {
                                resp.scroll_to_me(None);
                            }
                        }
                        let ink = match (selected, it.enabled) {
                            (true, true) => t.sel.text,
                            (true, false) => t.lcd.text_hi,
                            (false, true) => t.lcd.text,
                            (false, false) => t.lcd.text_dim,
                        };
                        let dim = if selected { t.lcd.text_hi } else { t.lcd.text_dim };
                        let font = t.font(t.font.ui);
                        let p = ui.painter();
                        let right = if it.shortcut.is_empty() {
                            rect.max.x - 8.0
                        } else {
                            let g = p.layout_no_wrap(it.shortcut.clone(), font.clone(), dim);
                            let w = g.size().x;
                            p.galley(pos2(rect.max.x - 8.0 - w, rect.center().y - g.size().y * 0.5), g, dim);
                            rect.max.x - 16.0 - w
                        };
                        let mut x = rect.min.x + 8.0;
                        let label = p.layout_no_wrap(it.label.clone(), font.clone(), ink);
                        let lw = label.size().x;
                        p.with_clip_rect(Rect::from_min_max(rect.min, pos2(right, rect.max.y))).galley(pos2(x, rect.center().y - label.size().y * 0.5), label, ink);
                        x += lw + 10.0;
                        if !it.group.is_empty() && x < right - 20.0 {
                            let g = p.layout_no_wrap(it.group.clone(), t.font(t.font.ui - 1.0), dim);
                            p.with_clip_rect(Rect::from_min_max(pos2(x, rect.min.y), pos2(right, rect.max.y))).galley(pos2(x, rect.center().y - g.size().y * 0.5), g, dim);
                        }
                        let resp = if it.enabled { resp } else { resp.on_hover_text(&it.why) };
                        if resp.clicked() {
                            if it.enabled {
                                chosen = Some(it.action);
                            } else {
                                self.sel = k;
                            }
                        }
                    }
                    self.reveal = false;
                });
            });
            widgets::lcd_well_edge(ui.painter(), well.response.rect);
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let shown = if order.len() == items.len() { format!("{} commands", items.len()) } else { format!("{} of {}", order.len(), items.len()) };
                ui.label(RichText::new(shown).color(d.text_disabled));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let key = keymap::shortcut_text(Command::CommandPalette);
                    let hint = if key.is_empty() { "Enter runs   ↑ ↓ pick   Esc closes".to_string() } else { format!("Enter runs   ↑ ↓ pick   Esc closes   {key} opens") };
                    ui.label(RichText::new(hint).color(d.text_disabled));
                });
            });
        });
        if enter {
            if let Some(&i) = order.get(self.sel) {
                let it = &items[i];
                if it.enabled {
                    chosen = Some(it.action);
                }
            }
        }
        if chosen.is_some() || close {
            self.open = false;
        }
        chosen
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use keymap::{Overrides, TABLE, bindings, conflicts, default_keys};

    fn state() -> CmdState {
        CmdState { video_track_focus: true, any_tracks: true, recent: vec!["C:\\work\\Reel.mnle".into()], layouts: [Some("Grading".into()), None, None, None, None, None, None, None, None, None], ..Default::default() }
    }

    /// "tm" puts Track Motion first; whole words and prefixes rank above scattered letters; a stale query matches nothing.
    #[test]
    fn ranking_puts_the_obvious_command_first() {
        let it = items(&state());
        let first = |q: &str| it[rank(&it, q)[0]].label.clone();
        assert_eq!(first("tm"), "Track Motion");
        assert_eq!(first("track motion"), "Track Motion");
        assert_eq!(first("undo"), "Undo");
        assert_eq!(first("save as"), "Save As");
        assert_eq!(first("Reel"), "Reel.mnle", "recent projects are rows");
        assert_eq!(first("grading"), "Grading", "a saved layout is a row, by its name");
        assert!(rank(&it, "qzqzqz").is_empty());
        assert_eq!(rank(&it, "").len(), it.len(), "an empty query lists everything in order");
        assert!(score("tm", "Track Motion") > score("tm", "Trim"));
        assert!(score("tm", "Track Motion") > score("tm", "Time"));
        assert!(score("tm", "Track Motion") > score("tm", "Adjustment Event"), "a substring inside a word is a weak match");
        assert!(score("mo", "Track Motion") > score("mo", "Remove Pan Envelope"), "a word-start substring is a strong one");
        assert_eq!(score("undo", "Undo"), score("undo", "Undo All"), "an exact and a prefix match tie; `rank` breaks it alphabetically");
        assert_eq!(score("x", "abc"), None);
        assert_eq!(score("", "abc"), Some(0));
    }

    /// Every command is a row (the palette itself and the per-slot rows aside), with its shortcut; a
    /// greyed row says why; plug-ins of every kind are rows.
    #[test]
    fn every_command_and_plugin_is_a_row() {
        let st = state();
        let it = items(&st);
        for (_, c) in menus::named_commands() {
            if matches!(c, Command::CommandPalette | Command::LayoutUser(_) | Command::SaveLayoutUser(_) | Command::OpenRecent(_)) {
                continue;
            }
            let row = it.iter().find(|i| i.action == Action::Run(c)).unwrap_or_else(|| panic!("{c:?} has no row"));
            assert_eq!(row.shortcut, keymap::shortcut_text(c));
            assert_eq!(row.enabled, st.enabled(c));
            assert_eq!(row.why.is_empty(), row.enabled, "{c:?}: {:?}", row.why);
        }
        let undo = it.iter().find(|i| i.action == Action::Run(Command::Undo)).unwrap();
        assert_eq!((undo.group.as_str(), undo.enabled, undo.why.as_str()), ("Edit", false, "Nothing to undo"));
        let tts = it.iter().find(|i| i.action == Action::Run(Command::TextToSpeech)).unwrap();
        assert!(tts.why.contains("R59"), "{}", tts.why);
        for kind in [PluginKind::VideoFx, PluginKind::Transition, PluginKind::Generator, PluginKind::AudioFx] {
            let n = plugins::catalog(kind).count();
            assert_eq!(it.iter().filter(|i| matches!(i.action, Action::Plugin(p) if p.kind == kind)).count(), n);
        }
        let afx = it.iter().find(|i| matches!(i.action, Action::Plugin(p) if p.kind == PluginKind::AudioFx)).unwrap();
        assert_eq!((afx.enabled, afx.why.as_str()), (false, "Select an audio event first"));
    }

    /// The default chord is in the table and clashes with no other row's key or chord start.
    #[test]
    fn default_key_collides_with_nothing() {
        assert_eq!(default_keys(Command::CommandPalette), &[KEY]);
        let b = bindings(&Overrides::new());
        assert_eq!(conflicts(&b, KEY, Command::CommandPalette), vec![]);
        for row in TABLE.iter().filter(|r| r.0 != Command::CommandPalette) {
            for k in default_keys(row.0) {
                assert_ne!(*k, KEY, "{:?} also has {KEY}", row.0);
            }
        }
    }

    /// Opening with a query runs the top row on Enter and closes; Esc closes without running.
    #[test]
    fn enter_runs_the_top_row() {
        let ctx = Context::default();
        let mut pal = Palette::default();
        pal.open_with("tm");
        let st = state();
        let mut got = None;
        let _ = ctx.run(RawInput::default(), |ctx| {
            got = pal.show(ctx, &st);
        });
        assert_eq!(got, None);
        assert!(pal.open);
        let press = |k: Key| RawInput { events: vec![Event::Key { key: k, physical_key: Some(k), pressed: true, repeat: false, modifiers: Modifiers::NONE }], ..Default::default() };
        let _ = ctx.run(press(Key::Enter), |ctx| {
            got = pal.show(ctx, &st);
        });
        assert_eq!(got, Some(Action::Run(Command::TrackMotion)));
        assert!(!pal.open);
        pal.open_with("");
        let _ = ctx.run(press(Key::Escape), |ctx| {
            got = pal.show(ctx, &st);
        });
        assert_eq!(got, None);
        assert!(!pal.open);
    }
}
