use std::{
    fs, io,
    path::PathBuf,
    sync::mpsc::{Receiver, TryRecvError},
};

use ratatui::{
    Frame,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
    layout::{Constraint, Layout},
    style::{Color, Style},
    widgets::{Block, Paragraph, Wrap},
};
use ratatui_textarea::TextArea;

use crate::compile::{self, Report};

/// The height of the compile pane, with its border.
const PANE_HEIGHT: u16 = 6;

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
    /// The folder for the PNG pages of the compile.
    out_dir: PathBuf,
    /// The report of the running compile arrives here. `None` when no compile runs.
    pending: Option<Receiver<Report>>,
    /// The report of the last finished compile.
    report: Option<Report>,
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
            out_dir: compile::new_out_dir(),
            pending: None,
            report: None,
        })
    }

    /// Writes the buffer to the file. Returns false when the write failed.
    fn save(&mut self) -> bool {
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
                true
            }
            Err(err) => {
                self.message = format!("Save failed: {err}");
                false
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        let armed = std::mem::take(&mut self.discard_armed);
        self.message.clear();
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && key.code == KeyCode::Char('s') {
            self.save();
        } else if ctrl && key.code == KeyCode::Char('b') {
            // A compile that already runs is not started twice.
            if self.save() && self.pending.is_none() {
                self.pending = Some(compile::start(self.path.clone(), self.out_dir.clone()));
            }
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

    /// Takes the report of a finished compile. Returns true when the screen must redraw.
    pub fn poll_compile(&mut self) -> bool {
        let Some(receiver) = &self.pending else {
            return false;
        };
        match receiver.try_recv() {
            Ok(report) => self.report = Some(report),
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => {
                self.report = Some(Report::failed("The compile thread stopped."));
            }
        }
        self.pending = None;
        true
    }

    fn compile_pane(&self) -> Paragraph<'static> {
        let (color, lines) = if self.pending.is_some() {
            (Color::Yellow, vec!["Compiling...".to_string()])
        } else if let Some(report) = &self.report {
            let color = if report.ok { Color::Green } else { Color::Red };
            let head = report.ok.then(|| "OK".to_string());
            (color, head.into_iter().chain(report.lines.iter().cloned()).collect())
        } else {
            (Color::Reset, vec!["Press Ctrl-B to compile.".to_string()])
        };
        Paragraph::new(lines.join("\n"))
            .wrap(Wrap { trim: false })
            .block(Block::bordered().title("Compile").border_style(Style::new().fg(color)))
    }

    pub fn draw(&self, frame: &mut Frame) {
        let [body, pane, status] = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(PANE_HEIGHT),
            Constraint::Length(1),
        ])
        .areas(frame.area());
        let marker = if self.dirty { " [+]" } else { "" };
        let block = Block::bordered().title(format!("{}{marker}", self.path.display()));
        let inner = block.inner(body);
        frame.render_widget(block, body);
        frame.render_widget(&self.textarea, inner);
        frame.render_widget(self.compile_pane(), pane);
        let hint = if self.message.is_empty() {
            "Ctrl-S save  Ctrl-B compile  Esc back"
        } else {
            &self.message
        };
        frame.render_widget(Paragraph::new(hint), status);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
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

    /// Waits up to 20 seconds for the running compile to finish.
    fn wait_for_report(editor: &mut Editor) {
        let start = Instant::now();
        while !editor.poll_compile() {
            assert!(start.elapsed() < Duration::from_secs(20), "compile did not finish");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn ctrl_s_writes_the_buffer_to_disk() {
        let path = temp_file("save", "hello\nworld\n");
        let mut editor = Editor::open(path.clone()).unwrap();
        editor.handle_key(key(KeyCode::Char('X')));
        assert!(editor.dirty);
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello\nworld\n");

        editor.handle_key(ctrl('s'));
        assert!(!editor.dirty);
        assert_eq!(fs::read_to_string(&path).unwrap(), "Xhello\nworld\n");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn an_unchanged_file_keeps_its_bytes() {
        let path = temp_file("same", "a\n\nb\n");
        let mut editor = Editor::open(path.clone()).unwrap();
        editor.handle_key(ctrl('s'));
        assert_eq!(fs::read_to_string(&path).unwrap(), "a\n\nb\n");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn an_empty_file_stays_empty() {
        let path = temp_file("empty", "");
        let mut editor = Editor::open(path.clone()).unwrap();
        editor.handle_key(ctrl('s'));
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

    #[test]
    fn ctrl_b_saves_then_compiles() {
        let path = temp_file("build", "= Title\n");
        let mut editor = Editor::open(path.clone()).unwrap();
        editor.out_dir = path.parent().unwrap().join("pages");
        editor.handle_key(key(KeyCode::Char('X')));
        editor.handle_key(ctrl('b'));
        assert!(!editor.dirty, "Ctrl-B must save first");
        assert_eq!(fs::read_to_string(&path).unwrap(), "X= Title\n");
        assert!(editor.pending.is_some());

        wait_for_report(&mut editor);
        assert!(editor.pending.is_none());
        let report = editor.report.as_ref().unwrap();
        assert!(report.ok, "{:?}", report.lines);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_compile_error_reaches_the_report() {
        let path = temp_file("builderr", "#nope()\n");
        let mut editor = Editor::open(path.clone()).unwrap();
        editor.out_dir = path.parent().unwrap().join("pages");
        editor.handle_key(ctrl('b'));
        wait_for_report(&mut editor);
        let report = editor.report.as_ref().unwrap();
        assert!(!report.ok);
        assert!(report.lines.iter().any(|l| l.contains(":1:")), "{:?}", report.lines);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
