//! The file switcher: a popup in the editor with the files of the project, the most recent first, and a
//! line to filter them. `F2` opens it. `F2` again opens the previous file.

use std::path::PathBuf;

use ratatui::{
    Frame,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
    layout::{Constraint, Flex, Layout},
    style::{Modifier, Style},
    widgets::{Block, Clear, List, ListState},
};

/// What a key did to the switcher.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    Stay,
    Close,
    /// Open this file, a path relative to the root.
    Open(PathBuf),
}

pub struct Switcher {
    /// The files in the order of the list: the most recent first. The open file is not in it.
    files: Vec<PathBuf>,
    filter: String,
    state: ListState,
}

impl Switcher {
    pub fn new(files: Vec<PathBuf>) -> Self {
        let mut state = ListState::default();
        state.select((!files.is_empty()).then_some(0));
        Self {
            files,
            filter: String::new(),
            state,
        }
    }

    /// The files that the filter lets through. The match ignores case.
    pub fn visible(&self) -> Vec<&PathBuf> {
        let needle = self.filter.to_lowercase();
        self.files
            .iter()
            .filter(|path| {
                needle.is_empty() || path.to_string_lossy().to_lowercase().contains(&needle)
            })
            .collect()
    }

    pub fn key(&mut self, key: KeyEvent) -> Outcome {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let count = self.visible().len();
        match key.code {
            KeyCode::Esc => return Outcome::Close,
            KeyCode::Enter => {
                let chosen = self
                    .state
                    .selected()
                    .and_then(|index| self.visible().get(index).copied().cloned());
                return chosen.map_or(Outcome::Stay, Outcome::Open);
            }
            // A second F2 is the toggle: the file that was open before this one.
            KeyCode::F(2) => {
                return self
                    .files
                    .first()
                    .cloned()
                    .map_or(Outcome::Close, Outcome::Open);
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
        // `select_next` and `select_previous` run past the ends. Keep the choice on a file.
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
        let [area] = Layout::horizontal([Constraint::Percentage(70)])
            .flex(Flex::Center)
            .areas(frame.area());
        let [area] = Layout::vertical([Constraint::Length(rows.min(frame.area().height))])
            .flex(Flex::Center)
            .areas(area);
        let title = format!("Switch file /{}", self.filter);
        let items: Vec<String> = visible
            .iter()
            .map(|path| path.display().to_string())
            .collect();
        let list = List::new(items)
            .block(
                Block::bordered()
                    .title(title)
                    .title_bottom("Type to filter  Enter opens  F2 previous  Esc closes"),
            )
            .highlight_style(Style::new().add_modifier(Modifier::REVERSED));
        frame.render_widget(Clear, area);
        frame.render_stateful_widget(list, area, &mut self.state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files() -> Switcher {
        Switcher::new(vec![
            "two.typ".into(),
            "one.typ".into(),
            "sub/three.typ".into(),
        ])
    }

    fn press(switcher: &mut Switcher, code: KeyCode) -> Outcome {
        switcher.key(KeyEvent::from(code))
    }

    fn type_text(switcher: &mut Switcher, text: &str) {
        for letter in text.chars() {
            press(switcher, KeyCode::Char(letter));
        }
    }

    #[test]
    fn the_order_is_the_order_that_the_caller_gave_and_enter_opens_the_first() {
        let mut switcher = files();
        assert_eq!(switcher.visible().len(), 3);
        assert_eq!(
            press(&mut switcher, KeyCode::Enter),
            Outcome::Open("two.typ".into())
        );
    }

    #[test]
    fn typing_filters_without_case_and_selects_the_first_match() {
        let mut switcher = files();
        press(&mut switcher, KeyCode::Down);
        type_text(&mut switcher, "THREE");
        assert_eq!(switcher.visible(), [&PathBuf::from("sub/three.typ")]);
        assert_eq!(
            press(&mut switcher, KeyCode::Enter),
            Outcome::Open("sub/three.typ".into())
        );
        press(&mut switcher, KeyCode::Backspace);
        assert_eq!(switcher.visible().len(), 1, "thre still matches one file");
        type_text(&mut switcher, "zzz");
        assert!(switcher.visible().is_empty());
        assert_eq!(
            press(&mut switcher, KeyCode::Enter),
            Outcome::Stay,
            "no file, no open"
        );
    }

    #[test]
    fn the_arrows_stop_at_the_ends() {
        let mut switcher = files();
        for _ in 0..5 {
            press(&mut switcher, KeyCode::Down);
        }
        assert_eq!(
            press(&mut switcher, KeyCode::Enter),
            Outcome::Open("sub/three.typ".into())
        );
        for _ in 0..5 {
            press(&mut switcher, KeyCode::Up);
        }
        assert_eq!(
            press(&mut switcher, KeyCode::Enter),
            Outcome::Open("two.typ".into())
        );
    }

    #[test]
    fn a_second_f2_opens_the_previous_file_and_esc_closes() {
        let mut switcher = files();
        type_text(&mut switcher, "three");
        assert_eq!(
            press(&mut switcher, KeyCode::F(2)),
            Outcome::Open("two.typ".into()),
            "the filter does not matter"
        );
        assert_eq!(press(&mut switcher, KeyCode::Esc), Outcome::Close);
        let mut none = Switcher::new(Vec::new());
        assert_eq!(press(&mut none, KeyCode::F(2)), Outcome::Close);
        assert_eq!(press(&mut none, KeyCode::Down), Outcome::Stay);
    }
}
