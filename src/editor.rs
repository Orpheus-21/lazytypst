use std::{fs, io, path::PathBuf};

use ratatui::{
    Frame,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
    layout::{Constraint, Layout},
    style::Style,
    widgets::{Block, Paragraph},
};
use ratatui_textarea::TextArea;

pub enum Action {
    Stay,
    Close,
}

pub struct Editor {
    path: PathBuf,
    textarea: TextArea<'static>,
    /// True when the buffer has text that is not on disk.
    dirty: bool,
    /// True after the first Esc on a dirty buffer. A second Esc discards the text.
    discard_armed: bool,
    message: String,
}

impl Editor {
    pub fn open(path: PathBuf) -> io::Result<Self> {
        let text = fs::read_to_string(&path)?;
        let mut textarea = TextArea::new(text.lines().map(String::from).collect());
        textarea.set_cursor_line_style(Style::default());
        Ok(Self {
            path,
            textarea,
            dirty: false,
            discard_armed: false,
            message: String::new(),
        })
    }

    fn save(&mut self) {
        let mut text = self.textarea.lines().join("\n");
        if !text.is_empty() {
            text.push('\n');
        }
        // ponytail: writes in place, so a crash during the write can cut the file.
        // CRLF line ends become LF. Upgrade: write a temp file, then rename it.
        match fs::write(&self.path, text) {
            Ok(()) => {
                self.dirty = false;
                self.message = "Saved".into();
            }
            Err(err) => self.message = format!("Save failed: {err}"),
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        let armed = std::mem::take(&mut self.discard_armed);
        self.message.clear();
        if key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.save();
        } else if key.code == KeyCode::Esc {
            if !self.dirty || armed {
                return Action::Close;
            }
            self.discard_armed = true;
            self.message = "Unsaved changes. Press Esc again to discard them, or Ctrl-S to save.".into();
        } else if self.textarea.input(key) {
            self.dirty = true;
        }
        Action::Stay
    }

    pub fn draw(&self, frame: &mut Frame) {
        let [body, status] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());
        let marker = if self.dirty { " [+]" } else { "" };
        let block = Block::bordered().title(format!("{}{marker}", self.path.display()));
        let inner = block.inner(body);
        frame.render_widget(block, body);
        frame.render_widget(&self.textarea, inner);
        let hint = if self.message.is_empty() {
            "Ctrl-S save  Esc back"
        } else {
            &self.message
        };
        frame.render_widget(Paragraph::new(hint), status);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl_s() -> KeyEvent {
        KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)
    }

    /// Makes a file with `content` in a new temporary folder.
    fn temp_file(name: &str, content: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lazytypst-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doc.typ");
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn ctrl_s_writes_the_buffer_to_disk() {
        let path = temp_file("save", "hello\nworld\n");
        let mut editor = Editor::open(path.clone()).unwrap();
        editor.handle_key(key(KeyCode::Char('X')));
        assert!(editor.dirty);
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello\nworld\n");

        editor.handle_key(ctrl_s());
        assert!(!editor.dirty);
        assert_eq!(fs::read_to_string(&path).unwrap(), "Xhello\nworld\n");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn an_unchanged_file_keeps_its_bytes() {
        let path = temp_file("same", "a\n\nb\n");
        let mut editor = Editor::open(path.clone()).unwrap();
        editor.handle_key(ctrl_s());
        assert_eq!(fs::read_to_string(&path).unwrap(), "a\n\nb\n");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn an_empty_file_stays_empty() {
        let path = temp_file("empty", "");
        let mut editor = Editor::open(path.clone()).unwrap();
        editor.handle_key(ctrl_s());
        assert_eq!(fs::read_to_string(&path).unwrap(), "");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn esc_closes_a_clean_buffer_at_once() {
        let path = temp_file("clean", "text\n");
        let mut editor = Editor::open(path.clone()).unwrap();
        assert!(matches!(editor.handle_key(key(KeyCode::Esc)), Action::Close));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn esc_on_a_dirty_buffer_warns_then_discards() {
        let path = temp_file("dirty", "text\n");
        let mut editor = Editor::open(path.clone()).unwrap();
        editor.handle_key(key(KeyCode::Char('X')));

        assert!(matches!(editor.handle_key(key(KeyCode::Esc)), Action::Stay));
        assert!(editor.message.contains("Unsaved"));
        assert!(matches!(editor.handle_key(key(KeyCode::Esc)), Action::Close));
        assert_eq!(fs::read_to_string(&path).unwrap(), "text\n");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn another_key_cancels_the_discard_warning() {
        let path = temp_file("cancel", "text\n");
        let mut editor = Editor::open(path.clone()).unwrap();
        editor.handle_key(key(KeyCode::Char('X')));
        editor.handle_key(key(KeyCode::Esc));
        editor.handle_key(key(KeyCode::Char('Y')));
        assert!(matches!(editor.handle_key(key(KeyCode::Esc)), Action::Stay));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
