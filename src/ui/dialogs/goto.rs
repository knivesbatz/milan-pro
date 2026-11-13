//! Go to Time (Ctrl+G), Set Time at Cursor and (R18b) Insert › Time: one time field in the ruler's format.
use eframe::egui::*;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum TimeMode {
    GoTo,
    SetRuler,
    /// Insert › Time: a length to insert at the cursor.
    Insert,
}

pub struct TimeEntry {
    pub mode: TimeMode,
    text: String,
    focused: bool,
}

impl TimeEntry {
    /// Opened on `shown`: the time as the ruler shows it now (a length for Insert).
    pub fn new(mode: TimeMode, shown: String) -> Self {
        Self { mode, text: shown, focused: false }
    }

    /// Some(Some(text)) on OK (Enter), Some(None) on Cancel, × or Esc; `valid` says whether the text is a time.
    pub fn show(&mut self, ctx: &Context, valid: impl Fn(&str) -> bool) -> Option<Option<String>> {
        let (title, label) = match self.mode {
            TimeMode::GoTo => ("Go To", "Time:"),
            TimeMode::SetRuler => ("Set Time at Cursor", "Time:"),
            TimeMode::Insert => ("Insert Time", "Length:"),
        };
        let (text, focused) = (&mut self.text, &mut self.focused);
        let (hit, close) = super::modal(ctx, "time_entry", title, vec2(300.0, 128.0), |ui| {
            let ok = valid(text);
            let enter = super::row(ui, label, 50.0, true, |ui| {
                let r = ui.add(TextEdit::singleline(text).desired_width(210.0));
                if !std::mem::replace(focused, true) {
                    // Opens with the time selected, so typing replaces it.
                    r.request_focus();
                    if let Some(mut st) = TextEdit::load_state(ui.ctx(), r.id) {
                        let all = text::CCursorRange::two(text::CCursor::new(0), text::CCursor::new(text.chars().count()));
                        st.cursor.set_char_range(Some(all));
                        st.store(ui.ctx(), r.id);
                    }
                }
                r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))
            });
            ui.add_space(12.0);
            match super::button_row(ui, &[("OK", ok), ("Cancel", true)]) {
                None if enter && ok => Some(0),
                h => h,
            }
        });
        match hit {
            Some(0) => Some(Some(self.text.clone())),
            Some(_) => Some(None),
            None => close.then_some(None),
        }
    }
}
