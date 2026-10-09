//! The list of keys. The text of `--help` and the help window both come from `KEYS`, so they cannot differ.
//! Each line can also name the key that it stands for. In the window, `Enter` presses that key.

use ratatui::{
    Frame,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
    layout::{Constraint, Flex, Layout},
    style::{Modifier, Style},
    text::{Line, Text},
    widgets::{Block, Clear, List, ListState},
};

/// The part of the program where a key works.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    List,
    Editor,
    /// The editing keys of the text area. They work in the editor and the window does not run them.
    Text,
}

impl Scope {
    fn title(self) -> &'static str {
        match self {
            Scope::List => "Keys in the file list:",
            Scope::Editor => "Keys in the editor:",
            Scope::Text => "Editing keys in the editor:",
        }
    }
}

pub struct Key {
    pub scope: Scope,
    /// The keys as the text shows them.
    pub keys: &'static str,
    pub text: &'static str,
    /// The key that the window presses for `Enter`. `None` for a line that is only information.
    pub press: Option<(KeyCode, KeyModifiers)>,
}

const NONE: KeyModifiers = KeyModifiers::NONE;
const CTRL: KeyModifiers = KeyModifiers::CONTROL;
const ALT: KeyModifiers = KeyModifiers::ALT;

const fn key(
    scope: Scope,
    keys: &'static str,
    text: &'static str,
    press: Option<(KeyCode, KeyModifiers)>,
) -> Key {
    Key {
        scope,
        keys,
        text,
        press,
    }
}

pub const KEYS: &[Key] = &[
    key(Scope::List, "? or F1", "Show this help window.", None),
    key(
        Scope::List,
        "j or Down",
        "Select the next file.",
        Some((KeyCode::Char('j'), NONE)),
    ),
    key(
        Scope::List,
        "k or Up",
        "Select the previous file.",
        Some((KeyCode::Char('k'), NONE)),
    ),
    key(
        Scope::List,
        "Enter",
        "Open the selected file.",
        Some((KeyCode::Enter, NONE)),
    ),
    key(
        Scope::List,
        "n",
        "Make a new .typ file. Type a path such as chapters/two. The ending .typ is added.",
        Some((KeyCode::Char('n'), NONE)),
    ),
    key(
        Scope::List,
        "E",
        "Export the PDF of the selected file next to it.",
        Some((KeyCode::Char('E'), NONE)),
    ),
    key(
        Scope::List,
        "e",
        "Edit the selected file in $VISUAL or $EDITOR, then open it here.",
        Some((KeyCode::Char('e'), NONE)),
    ),
    key(
        Scope::List,
        "s",
        "Switch the order: by path, or the newest change first.",
        Some((KeyCode::Char('s'), NONE)),
    ),
    key(
        Scope::List,
        "y",
        "Copy the absolute path of the selected file to the system clipboard.",
        Some((KeyCode::Char('y'), NONE)),
    ),
    key(
        Scope::List,
        "/",
        "Filter the list. Type a part of a path. Enter keeps the filter, Esc removes it.",
        Some((KeyCode::Char('/'), NONE)),
    ),
    key(
        Scope::List,
        "r",
        "Read the folder again, to show new files and to drop deleted files.",
        Some((KeyCode::Char('r'), NONE)),
    ),
    key(
        Scope::List,
        "g or Home",
        "Select the first file.",
        Some((KeyCode::Char('g'), NONE)),
    ),
    key(
        Scope::List,
        "G or End",
        "Select the last file.",
        Some((KeyCode::Char('G'), NONE)),
    ),
    key(
        Scope::List,
        "m",
        "Mark the selected file as the main file, or remove the mark. lazytypst then compiles the main file, whatever file you edit.",
        Some((KeyCode::Char('m'), NONE)),
    ),
    key(Scope::List, "q", "Quit.", Some((KeyCode::Char('q'), NONE))),
    key(Scope::Editor, "F1", "Show this help window.", None),
    key(
        Scope::Editor,
        "F2",
        "Switch to another file. Type to filter, Enter opens. F2 again opens the file before this one.",
        Some((KeyCode::F(2), NONE)),
    ),
    key(
        Scope::Editor,
        "Ctrl-S",
        "Save the file.",
        Some((KeyCode::Char('s'), CTRL)),
    ),
    key(
        Scope::Editor,
        "Ctrl-B",
        "Save and compile the file.",
        Some((KeyCode::Char('b'), CTRL)),
    ),
    key(
        Scope::Editor,
        "Ctrl-E",
        "Save the file and export a PDF next to it.",
        Some((KeyCode::Char('e'), CTRL)),
    ),
    key(
        Scope::Editor,
        "Ctrl-F",
        "Search. Type the text, then Enter or Ctrl-F for the next match. Alt-R switches to a regular expression. Esc closes.",
        Some((KeyCode::Char('f'), CTRL)),
    ),
    key(
        Scope::Editor,
        "F11",
        "Show the preview on the full screen, and back. There, the plus key and the minus key zoom, 0 shows the whole page, and the arrow keys move the view. Typing does nothing there.",
        Some((KeyCode::F(11), NONE)),
    ),
    key(
        Scope::Editor,
        "F5",
        "Turn the live compile off or on. Autosave stays on.",
        Some((KeyCode::F(5), NONE)),
    ),
    key(
        Scope::Editor,
        "Ctrl-G",
        "Go to the first error of the last compile.",
        Some((KeyCode::Char('g'), CTRL)),
    ),
    key(
        Scope::Editor,
        "Alt-Enter, Ctrl-]",
        "Open the file that the string under the cursor names, for example in #include \"chapters/two.typ\". The text is saved first. F2 F2 goes back.",
        Some((KeyCode::Enter, ALT)),
    ),
    key(
        Scope::Editor,
        "F4",
        "Show the outline: the headings of the file. Type to filter, Enter jumps to the heading, F4 or Esc closes.",
        Some((KeyCode::F(4), NONE)),
    ),
    key(
        Scope::Editor,
        "Alt-S",
        "Replace the text of the last search. Type the new text. Enter replaces one match and goes to the next. Alt-A replaces all. Esc stops. Press Alt-S in the search prompt, too.",
        Some((KeyCode::Char('s'), ALT)),
    ),
    key(
        Scope::Editor,
        "Alt-G",
        "Go to a line. Type the line number, or line:column, then Enter. Esc closes.",
        Some((KeyCode::Char('g'), ALT)),
    ),
    key(
        Scope::Editor,
        "F8, Shift-F8",
        "Go to the next error, or the previous error. The search wraps at the ends.",
        Some((KeyCode::F(8), NONE)),
    ),
    key(
        Scope::Editor,
        "Ctrl-O",
        "Open the last exported PDF in the system viewer (xdg-open).",
        Some((KeyCode::Char('o'), CTRL)),
    ),
    key(
        Scope::Editor,
        "Alt-Down",
        "Show the next page.",
        Some((KeyCode::Down, ALT)),
    ),
    key(
        Scope::Editor,
        "Alt-Up",
        "Show the previous page.",
        Some((KeyCode::Up, ALT)),
    ),
    key(
        Scope::Editor,
        "Alt-Home",
        "Show the first page.",
        Some((KeyCode::Home, ALT)),
    ),
    key(
        Scope::Editor,
        "Alt-End",
        "Show the last page.",
        Some((KeyCode::End, ALT)),
    ),
    key(
        Scope::Editor,
        "Esc",
        "Save the file and go back to the file list.",
        Some((KeyCode::Esc, NONE)),
    ),
    key(
        Scope::Editor,
        "Ctrl-Q",
        "Save the file and quit.",
        Some((KeyCode::Char('q'), CTRL)),
    ),
    key(Scope::Text, "Ctrl-U, Ctrl-Z", "Undo.", None),
    key(Scope::Text, "Alt-A", "Select all the text.", None),
    key(Scope::Text, "Ctrl-R", "Redo.", None),
    key(Scope::Text, "Shift with arrow keys", "Select text.", None),
    key(
        Scope::Text,
        "Ctrl-C",
        "Copy the selection. It also goes to the system clipboard.",
        None,
    ),
    key(
        Scope::Text,
        "Ctrl-X",
        "Cut the selection. It also goes to the system clipboard.",
        None,
    ),
    key(
        Scope::Text,
        "Ctrl-Y",
        "Paste the text that the text area holds.",
        None,
    ),
    key(
        Scope::Text,
        "Shift-Tab",
        "Remove one indent level from the line.",
        None,
    ),
    key(
        Scope::Text,
        "Enter",
        "Break the line and keep its indent. After ( [ or { the new line has one level more.",
        None,
    ),
    key(
        Scope::Text,
        "Ctrl-W",
        "Delete the word before the cursor.",
        None,
    ),
    key(
        Scope::Text,
        "Ctrl-K",
        "Delete to the end of the line.",
        None,
    ),
    key(
        Scope::Text,
        "Ctrl-J",
        "Delete to the start of the line.",
        None,
    ),
    key(
        Scope::Text,
        "Alt-F, Alt-B",
        "Move one word forward or back.",
        None,
    ),
    key(
        Scope::Text,
        "Ctrl-A, End",
        "Move to the start or the end of the line.",
        None,
    ),
    key(
        Scope::Text,
        "Ctrl-Home, Ctrl-End",
        "Move to the start or the end of the file. With Shift, select to there.",
        None,
    ),
    key(
        Scope::Text,
        "Ctrl-V, Alt-V",
        "Scroll one page down or up.",
        None,
    ),
    key(
        Scope::Text,
        "Alt-<, Alt->",
        "Move to the first or the last line. The column stays.",
        None,
    ),
];

/// The lines of the keys of one scope, as the text of `--help` shows them.
fn section(scope: Scope) -> String {
    let mut text = format!("{}\n", scope.title());
    for entry in KEYS.iter().filter(|entry| entry.scope == scope) {
        text.push_str(&format!("  {:<14} {}\n", entry.keys, entry.text));
    }
    text
}

/// All the key lists, for `--help`.
pub fn usage_keys() -> String {
    [Scope::List, Scope::Editor, Scope::Text]
        .map(section)
        .join("\n")
}

/// All the keys as lines of tab separated text: the part of the program (`list`, `editor`, or `text`), the keys,
/// and the description. For scripts, such as `scripts/sync-keys.py`.
pub fn keys_tsv() -> String {
    KEYS.iter()
        .map(|entry| {
            let part = match entry.scope {
                Scope::List => "list",
                Scope::Editor => "editor",
                Scope::Text => "text",
            };
            format!("{part}\t{}\t{}\n", entry.keys, entry.text)
        })
        .collect()
}

/// The width of the column of keys in the window. "Shift with arrow keys" is the longest.
const KEYS_WIDTH: usize = 22;

/// Breaks `text` into lines of at most `width` characters, at spaces. A word that is longer than a line is
/// cut. No word is lost.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        let mut word = word.to_string();
        while word.chars().count() > width {
            let cut: String = word.chars().take(width).collect();
            word = word.chars().skip(width).collect();
            lines.push(cut);
        }
        match lines.last_mut() {
            Some(line) if line.chars().count() + 1 + word.chars().count() <= width => {
                line.push(' ');
                line.push_str(&word);
            }
            _ => lines.push(word),
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// One row of the window: a title, or a key.
enum Row {
    Title(Scope),
    Key(&'static Key),
}

fn rows() -> Vec<Row> {
    let mut rows = Vec::new();
    for scope in [Scope::List, Scope::Editor, Scope::Text] {
        rows.push(Row::Title(scope));
        rows.extend(
            KEYS.iter()
                .filter(|entry| entry.scope == scope)
                .map(Row::Key),
        );
    }
    rows
}

/// The state of the open help window.
pub struct Help {
    rows: Vec<Row>,
    state: ListState,
    /// The scope where the window works. `Enter` presses only the keys of this scope.
    scope: Scope,
}

impl Help {
    /// Opens the window with the first key of `scope` selected. The scope is `List` or `Editor`.
    pub fn new(scope: Scope) -> Self {
        let rows = rows();
        let first = rows
            .iter()
            .position(|row| matches!(row, Row::Key(entry) if entry.scope == scope))
            .unwrap_or(0);
        Help {
            rows,
            state: ListState::default().with_selected(Some(first)),
            scope,
        }
    }

    /// Moves the selection by `step` keys. A title is skipped. The selection stops at the ends.
    pub fn select(&mut self, step: isize) {
        let mut at = self.state.selected().unwrap_or(0);
        loop {
            let Some(next) = at
                .checked_add_signed(step)
                .filter(|next| *next < self.rows.len())
            else {
                return;
            };
            at = next;
            if matches!(self.rows[at], Row::Key(_)) {
                self.state.select(Some(at));
                return;
            }
        }
    }

    /// The text of the selected key, for a test.
    #[cfg(test)]
    pub fn selected_keys(&self) -> Option<&'static str> {
        match self.rows.get(self.state.selected()?)? {
            Row::Key(entry) => Some(entry.keys),
            Row::Title(_) => None,
        }
    }

    /// The key that `Enter` presses: the key of the selected line, if the window works for its scope.
    pub fn enter(&self) -> Option<KeyEvent> {
        let Row::Key(entry) = self.rows.get(self.state.selected()?)? else {
            return None;
        };
        let (code, modifiers) = entry.press.filter(|_| entry.scope == self.scope)?;
        Some(KeyEvent::new(code, modifiers))
    }

    /// Selects the line whose keys are `keys`, for a test.
    #[cfg(test)]
    pub fn select_keys(&mut self, keys: &str) {
        let at = self
            .rows
            .iter()
            .position(|row| matches!(row, Row::Key(entry) if entry.keys == keys && entry.scope == self.scope))
            .expect("the key is in the list");
        self.state.select(Some(at));
    }

    /// Draws the window over the screen. The text under it stays.
    pub fn draw(&mut self, frame: &mut Frame) {
        let [area] = Layout::horizontal([Constraint::Percentage(90)])
            .flex(Flex::Center)
            .areas(frame.area());
        let [area] = Layout::vertical([Constraint::Percentage(90)])
            .flex(Flex::Center)
            .areas(area);
        frame.render_widget(Clear, area);
        // The text column starts after two spaces, the keys, and a space. A long text wraps there, so that
        // no word is cut at the right edge. One item has many lines, and the selection marks all of them.
        let inner = usize::from(area.width.saturating_sub(2));
        let text_width = inner.saturating_sub(KEYS_WIDTH + 4).max(10);
        let items = self.rows.iter().map(|row| match row {
            Row::Title(scope) => Text::from(Line::styled(
                scope.title(),
                Style::new().add_modifier(Modifier::BOLD),
            )),
            Row::Key(entry) => {
                let mut lines = wrap(entry.text, text_width).into_iter();
                let first = lines.next().unwrap_or_default();
                let mut text = vec![Line::from(format!(
                    "  {:<width$} {first}",
                    entry.keys,
                    width = KEYS_WIDTH
                ))];
                text.extend(lines.map(|line| Line::from(format!("  {:<KEYS_WIDTH$} {line}", ""))));
                Text::from(text)
            }
        });
        let list = List::new(items)
            .block(
                Block::bordered()
                    .title("Help")
                    .title_bottom("j/k select  Enter runs the key  Esc closes"),
            )
            .highlight_style(Style::new().add_modifier(Modifier::REVERSED));
        frame.render_stateful_widget(list, area, &mut self.state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_usage_keys_have_three_lists_in_the_order_of_the_scopes() {
        let text = usage_keys();
        let list = text.find("Keys in the file list:").unwrap();
        let editor = text.find("Keys in the editor:").unwrap();
        let editing = text.find("Editing keys in the editor:").unwrap();
        assert!(list < editor && editor < editing);
        assert!(text.contains("  Ctrl-S         Save the file."));
        assert!(text.contains("  Ctrl-U, Ctrl-Z Undo."));
    }

    #[test]
    fn selecting_skips_the_titles_and_stops_at_the_ends() {
        let mut help = Help::new(Scope::List);
        assert_eq!(help.selected_keys(), Some("? or F1"));
        help.select(-1);
        assert_eq!(help.selected_keys(), Some("? or F1"));
        for _ in 0..500 {
            help.select(1);
        }
        assert_eq!(help.selected_keys(), Some("Alt-<, Alt->"));
        // Moving across a title lands on the next key.
        let mut help = Help::new(Scope::List);
        help.select_keys("q");
        help.select(1);
        assert_eq!(help.selected_keys(), Some("F1"));
    }

    #[test]
    fn enter_presses_the_key_only_in_the_scope_of_the_window() {
        let mut help = Help::new(Scope::List);
        help.select_keys("E");
        assert_eq!(
            help.enter(),
            Some(KeyEvent::new(KeyCode::Char('E'), KeyModifiers::NONE))
        );
        help.select(-1);
        help.select(1);
        // A line of the editor has no effect in the list.
        for _ in 0..30 {
            help.select(1);
        }
        assert_eq!(help.enter(), None);
        // A line that is only information has no key.
        let help = Help::new(Scope::List);
        assert_eq!(help.enter(), None, "the help line");
    }
    /// Draws the window on a screen of `width` by `height` and returns all the text on it.
    fn drawn(width: u16, height: u16) -> String {
        use ratatui::{Terminal, backend::TestBackend};
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let mut help = Help::new(Scope::List);
        terminal.draw(|frame| help.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|row| {
                (0..width)
                    .map(|column| buffer[(column, row)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn wrap_breaks_at_spaces_and_loses_no_word() {
        assert_eq!(wrap("one two three", 7), ["one two", "three"]);
        assert_eq!(wrap("", 5), [""]);
        assert_eq!(wrap("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        let text = "a bb ccc dddd eeeee ffffff";
        for width in [3, 6, 10, 40] {
            let joined = wrap(text, width).join(" ");
            assert_eq!(
                joined.replace(' ', ""),
                text.replace(' ', ""),
                "width {width}"
            );
        }
    }

    #[test]
    fn every_word_of_every_text_shows_in_the_window_at_100_and_at_60_columns() {
        for width in [100, 60] {
            let screen = drawn(width, 170);
            // Take the text of the window without the border and the keys, line by line.
            let words: std::collections::HashSet<&str> = screen.split_whitespace().collect();
            for entry in KEYS {
                for word in entry.text.split_whitespace() {
                    let shown = words.contains(word) || screen.contains(word);
                    assert!(
                        shown,
                        "the word {word:?} of {:?} is cut at {width} columns",
                        entry.keys
                    );
                }
            }
        }
    }

    #[test]
    fn no_text_touches_the_right_border_of_the_window() {
        for width in [100u16, 60] {
            let screen = drawn(width, 170);
            let window = usize::from(width) * 90 / 100;
            let left = (usize::from(width) - window) / 2;
            let last_inner = left + window - 2;
            for (number, row) in screen.lines().enumerate().skip(1).take(100) {
                let chars: Vec<char> = row.chars().collect();
                if chars.get(last_inner + 1) == Some(&'\u{2502}') {
                    assert_eq!(
                        chars[last_inner], ' ',
                        "row {number} at {width} columns: {row}"
                    );
                }
            }
        }
    }
    #[test]
    fn the_keys_as_tab_separated_lines_have_three_fields_and_name_every_key() {
        let text = keys_tsv();
        assert_eq!(text.lines().count(), KEYS.len());
        for (line, entry) in text.lines().zip(KEYS) {
            let fields: Vec<&str> = line.split('\t').collect();
            assert_eq!(fields.len(), 3, "{line}");
            assert_eq!((fields[1], fields[2]), (entry.keys, entry.text));
            assert!(["list", "editor", "text"].contains(&fields[0]));
        }
    }
    #[test]
    fn no_two_entries_of_the_same_part_use_the_same_key() {
        // The editor and the text area share the keys of the text, so they count as one part.
        use std::collections::HashMap;
        let mut seen: HashMap<(bool, String), &str> = HashMap::new();
        for entry in KEYS {
            // A line can name more than one key: "j or Down", "Ctrl-A, End".
            let tokens = entry.keys.split([',']).flat_map(|part| part.split(" or "));
            for token in tokens {
                let token = token.trim().to_string();
                let list = entry.scope == Scope::List;
                if let Some(other) = seen.insert((list, token.clone()), entry.keys) {
                    panic!(
                        "the key {token:?} is in the lines {other:?} and {:?}",
                        entry.keys
                    );
                }
            }
        }
    }
}
