//! A popup list with a line to filter it. The file switcher (`F2`) lists the files of the project, the
//! most recent first. The outline (`F4`) lists the headings of the open file. Both pick one entry.

use ratatui::{
    Frame,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
    layout::{Constraint, Flex, Layout},
    style::{Modifier, Style},
    widgets::{Block, Clear, List, ListState},
};

/// What a key did to the popup.
#[derive(Debug, PartialEq)]
pub enum Outcome<T> {
    Stay,
    Close,
    /// The user picked this entry.
    Pick(T),
}

pub struct Switcher<T> {
    title: String,
    hint: &'static str,
    /// The entries in the order of the list, each with the text that the list shows and the filter reads.
    entries: Vec<(T, String)>,
    filter: String,
    state: ListState,
    /// True if a second `F2` picks the first entry. That is the toggle of the file switcher.
    toggle: bool,
}

impl<T: Clone> Switcher<T> {
    pub fn new(
        title: impl Into<String>,
        hint: &'static str,
        entries: Vec<(T, String)>,
        toggle: bool,
    ) -> Self {
        let mut state = ListState::default();
        state.select((!entries.is_empty()).then_some(0));
        Self {
            title: title.into(),
            hint,
            entries,
            filter: String::new(),
            state,
            toggle,
        }
    }

    /// Selects the entry at `index` of the whole list. Use it before the user types a filter.
    pub fn select(&mut self, index: usize) {
        if index < self.entries.len() {
            self.state.select(Some(index));
        }
    }

    /// The entries that the filter lets through. The match ignores case.
    pub fn visible(&self) -> Vec<&(T, String)> {
        let needle = self.filter.to_lowercase();
        self.entries
            .iter()
            .filter(|(_, text)| needle.is_empty() || text.to_lowercase().contains(&needle))
            .collect()
    }

    pub fn key(&mut self, key: KeyEvent) -> Outcome<T> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let count = self.visible().len();
        match key.code {
            KeyCode::Esc => return Outcome::Close,
            KeyCode::Enter => {
                let chosen = self
                    .state
                    .selected()
                    .and_then(|index| self.visible().get(index).map(|(value, _)| value.clone()));
                return chosen.map_or(Outcome::Stay, Outcome::Pick);
            }
            // A second F2 is the toggle: the file that was open before this one.
            KeyCode::F(2) if self.toggle => {
                return self
                    .entries
                    .first()
                    .map(|(value, _)| value.clone())
                    .map_or(Outcome::Close, Outcome::Pick);
            }
            KeyCode::Down | KeyCode::Tab => self.state.select_next(),
            KeyCode::Up | KeyCode::BackTab => self.state.select_previous(),
            KeyCode::Char('n') if ctrl => self.state.select_next(),
            KeyCode::Char('p') if ctrl => self.state.select_previous(),
            KeyCode::Backspace => {
                self.filter.pop();
                self.state.select((!self.visible().is_empty()).then_some(0));
            }
            KeyCode::Char(letter) if !ctrl && !alt => {
                self.filter.push(letter);
                self.state.select((!self.visible().is_empty()).then_some(0));
            }
            _ => {}
        }
        // `select_next` and `select_previous` run past the ends. Keep the choice on an entry.
        if let Some(index) = self.state.selected() {
            self.state.select(Some(index.min(count.saturating_sub(1))));
        }
        Outcome::Stay
    }

    /// Draws the popup over the screen. The text under it stays.
    pub fn draw(&mut self, frame: &mut Frame) {
        let visible = self.visible();
        let rows = u16::try_from(visible.len())
            .unwrap_or(u16::MAX)
            .saturating_add(2)
            .max(4);
        // 70 percent of the width, but not less than 56 columns, so a narrow window still shows a whole line.
        let wide = (frame.area().width / 10 * 7)
            .max(56)
            .min(frame.area().width);
        let [area] = Layout::horizontal([Constraint::Length(wide)])
            .flex(Flex::Center)
            .areas(frame.area());
        let [area] = Layout::vertical([Constraint::Length(rows.min(frame.area().height))])
            .flex(Flex::Center)
            .areas(area);
        let title = format!("{} /{}", self.title, self.filter);
        let items: Vec<String> = visible.iter().map(|(_, text)| text.clone()).collect();
        let list = List::new(items)
            .block(Block::bordered().title(title).title_bottom(self.hint))
            .highlight_style(Style::new().add_modifier(Modifier::REVERSED));
        frame.render_widget(Clear, area);
        frame.render_stateful_widget(list, area, &mut self.state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn files() -> Switcher<PathBuf> {
        let entries = ["two.typ", "one.typ", "sub/three.typ"]
            .map(|name| (PathBuf::from(name), name.to_string()))
            .to_vec();
        Switcher::new("Switch file", "hint", entries, true)
    }

    fn press(switcher: &mut Switcher<PathBuf>, code: KeyCode) -> Outcome<PathBuf> {
        switcher.key(KeyEvent::from(code))
    }

    fn type_text(switcher: &mut Switcher<PathBuf>, text: &str) {
        for letter in text.chars() {
            press(switcher, KeyCode::Char(letter));
        }
    }

    fn pick(name: &str) -> Outcome<PathBuf> {
        Outcome::Pick(name.into())
    }

    #[test]
    fn the_order_is_the_order_that_the_caller_gave_and_enter_picks_the_first() {
        let mut switcher = files();
        assert_eq!(switcher.visible().len(), 3);
        assert_eq!(press(&mut switcher, KeyCode::Enter), pick("two.typ"));
    }

    #[test]
    fn typing_filters_without_case_and_selects_the_first_match() {
        let mut switcher = files();
        press(&mut switcher, KeyCode::Down);
        type_text(&mut switcher, "THREE");
        assert_eq!(switcher.visible().len(), 1);
        assert_eq!(press(&mut switcher, KeyCode::Enter), pick("sub/three.typ"));
        press(&mut switcher, KeyCode::Backspace);
        assert_eq!(switcher.visible().len(), 1, "thre still matches one file");
        type_text(&mut switcher, "zzz");
        assert!(switcher.visible().is_empty());
        assert_eq!(
            press(&mut switcher, KeyCode::Enter),
            Outcome::Stay,
            "no entry, no pick"
        );
    }

    #[test]
    fn the_arrows_stop_at_the_ends() {
        let mut switcher = files();
        for _ in 0..5 {
            press(&mut switcher, KeyCode::Down);
        }
        assert_eq!(press(&mut switcher, KeyCode::Enter), pick("sub/three.typ"));
        for _ in 0..5 {
            press(&mut switcher, KeyCode::Up);
        }
        assert_eq!(press(&mut switcher, KeyCode::Enter), pick("two.typ"));
    }

    #[test]
    fn a_second_f2_picks_the_first_entry_if_the_toggle_is_on_and_esc_closes() {
        let mut switcher = files();
        type_text(&mut switcher, "three");
        assert_eq!(
            press(&mut switcher, KeyCode::F(2)),
            pick("two.typ"),
            "the filter does not matter"
        );
        assert_eq!(press(&mut switcher, KeyCode::Esc), Outcome::Close);
        let mut none = Switcher::<PathBuf>::new("t", "h", Vec::new(), true);
        assert_eq!(press(&mut none, KeyCode::F(2)), Outcome::Close);
        assert_eq!(press(&mut none, KeyCode::Down), Outcome::Stay);
        let mut off = Switcher::new("t", "h", vec![(1, "one".to_string())], false);
        assert_eq!(off.key(KeyEvent::from(KeyCode::F(2))), Outcome::Stay);
    }
}
