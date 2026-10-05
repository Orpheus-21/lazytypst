use std::{
    fs, io,
    path::PathBuf,
    time::{Duration, Instant},
};

use ratatui::{
    Frame,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
    layout::{Constraint, Layout},
    style::{Color, Style},
    widgets::{Block, Paragraph, Wrap},
};
use ratatui_image::picker::Picker;
use ratatui_textarea::TextArea;

use crate::{
    compile::{self, Job, Report},
    preview::Preview,
};

/// The height of the compile pane, with its border.
const PANE_HEIGHT: u16 = 6;

/// The time without a key after which the editor saves and compiles.
const DEBOUNCE: Duration = Duration::from_millis(300);

/// True when the user edited the text and then typed nothing for `DEBOUNCE`.
fn debounce_done(last_edit: Option<Instant>, now: Instant) -> bool {
    last_edit.is_some_and(|edit| now.saturating_duration_since(edit) >= DEBOUNCE)
}

pub enum Action {
    Stay,
    Close,
}

pub struct Editor {
    path: PathBuf,
    textarea: TextArea<'static>,
    /// True when the buffer has text that is not on disk.
    dirty: bool,
    /// The time of the last edit that no save has covered yet.
    last_edit: Option<Instant>,
    /// True after the first Esc on a dirty buffer. A second Esc discards the text.
    discard_armed: bool,
    message: String,
    /// The folder for the PNG pages of the compile.
    out_dir: PathBuf,
    /// The running compile. `None` when no compile runs.
    job: Option<Job>,
    /// The report of the last finished compile.
    report: Option<Report>,
    preview: Preview,
}

impl Editor {
    pub fn open(path: PathBuf, picker: Picker) -> io::Result<Self> {
        let text = fs::read_to_string(&path)?;
        let mut textarea = TextArea::new(text.lines().map(String::from).collect());
        textarea.set_cursor_line_style(Style::default());
        Ok(Self {
            path,
            textarea,
            dirty: false,
            last_edit: None,
            discard_armed: false,
            message: String::new(),
            out_dir: compile::new_out_dir(),
            job: None,
            report: None,
            preview: Preview::new(picker),
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
                self.last_edit = None;
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
            self.save_and_compile();
        } else if key.code == KeyCode::Esc {
            if !self.dirty || armed {
                return Action::Close;
            }
            self.discard_armed = true;
            self.message = "Unsaved changes. Press Esc again to discard them, or Ctrl-S to save.".into();
        } else if self.textarea.input(key) {
            self.dirty = true;
            self.last_edit = Some(Instant::now());
        }
        Action::Stay
    }

    fn save_and_compile(&mut self) {
        if self.save() {
            // The new job replaces the old job. Dropping the old job kills its process.
            self.job = Some(Job::start(&self.path, &self.out_dir));
        }
    }

    /// Runs the autosave when the user paused, and takes the report of a finished compile.
    /// The main loop calls this when no key arrives. Returns true when the screen must redraw.
    pub fn tick(&mut self, now: Instant) -> bool {
        let mut changed = false;
        if debounce_done(self.last_edit, now) {
            // `save` clears `last_edit`. If the save fails, nothing retries until the next edit.
            self.last_edit = None;
            self.save_and_compile();
            changed = true;
        }
        changed |= self.poll_compile();
        changed
    }

    /// Takes the report of a finished compile. Returns true when the screen must redraw.
    fn poll_compile(&mut self) -> bool {
        let Some(mut report) = self.job.as_ref().and_then(Job::try_report) else {
            return false;
        };
        if report.ok
            && let Err(err) = self.preview.load(&self.out_dir.join("page-1.png"))
        {
            report = Report::failed(err);
        }
        self.report = Some(report);
        self.job = None;
        true
    }

    fn compile_pane(&self) -> Paragraph<'static> {
        let (color, lines) = if self.job.is_some() {
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

    pub fn draw(&mut self, frame: &mut Frame) {
        let [main, status] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());
        let [left, right] =
            Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(main);
        let [body, pane] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(PANE_HEIGHT)]).areas(left);
        let marker = if self.dirty { " [+]" } else { "" };
        let block = Block::bordered().title(format!("{}{marker}", self.path.display()));
        let inner = block.inner(body);
        frame.render_widget(block, body);
        frame.render_widget(&self.textarea, inner);
        frame.render_widget(self.compile_pane(), pane);
        self.preview.draw(frame, right);
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

    fn open(path: &std::path::Path) -> Editor {
        Editor::open(path.to_path_buf(), Picker::halfblocks()).unwrap()
    }

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
        let mut editor = open(&path);
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
        let mut editor = open(&path);
        editor.handle_key(ctrl('s'));
        assert_eq!(fs::read_to_string(&path).unwrap(), "a\n\nb\n");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn an_empty_file_stays_empty() {
        let path = temp_file("empty", "");
        let mut editor = open(&path);
        editor.handle_key(ctrl('s'));
        assert_eq!(fs::read_to_string(&path).unwrap(), "");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn esc_closes_a_clean_buffer_at_once() {
        let path = temp_file("clean", "text\n");
        let mut editor = open(&path);
        assert!(matches!(editor.handle_key(key(KeyCode::Esc)), Action::Close));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn esc_on_a_dirty_buffer_warns_then_discards() {
        let path = temp_file("dirty", "text\n");
        let mut editor = open(&path);
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
        let mut editor = open(&path);
        editor.handle_key(key(KeyCode::Char('X')));
        editor.handle_key(key(KeyCode::Esc));
        editor.handle_key(key(KeyCode::Char('Y')));
        assert!(matches!(editor.handle_key(key(KeyCode::Esc)), Action::Stay));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn the_autosave_waits_for_a_quiet_300_ms() {
        let t0 = Instant::now();
        assert!(!debounce_done(None, t0));
        assert!(!debounce_done(Some(t0), t0));
        assert!(!debounce_done(Some(t0), t0 + Duration::from_millis(299)));
        assert!(debounce_done(Some(t0), t0 + DEBOUNCE));
        // A clock that runs back is not a pause.
        assert!(!debounce_done(Some(t0 + DEBOUNCE), t0));
    }

    #[test]
    fn typing_alone_does_not_save_or_compile() {
        let path = temp_file("typing", "= Title\n");
        let mut editor = open(&path);
        for c in "abcdefghij".chars() {
            editor.handle_key(key(KeyCode::Char(c)));
        }
        assert!(!editor.tick(Instant::now()));
        assert!(editor.job.is_none());
        assert!(editor.dirty);
        assert_eq!(fs::read_to_string(&path).unwrap(), "= Title\n");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_pause_after_ten_keys_saves_and_compiles_once() {
        let path = temp_file("pause", "= Title\n");
        let mut editor = open(&path);
        editor.out_dir = path.parent().unwrap().join("pages");
        for c in "abcdefghij".chars() {
            editor.handle_key(key(KeyCode::Char(c)));
        }
        let later = Instant::now() + Duration::from_millis(400);
        assert!(editor.tick(later));
        assert!(!editor.dirty);
        assert!(editor.job.is_some());
        assert_eq!(fs::read_to_string(&path).unwrap(), "abcdefghij= Title\n");

        wait_for_report(&mut editor);
        assert!(editor.report.as_ref().unwrap().ok);
        assert!(editor.preview.has_page());
        // The same burst of keys does not start a second compile.
        assert!(!editor.tick(later));
        assert!(editor.job.is_none());
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_manual_save_cancels_the_autosave() {
        let path = temp_file("cancel-auto", "= Title\n");
        let mut editor = open(&path);
        editor.handle_key(key(KeyCode::Char('X')));
        editor.handle_key(ctrl('s'));
        assert!(!editor.tick(Instant::now() + Duration::from_millis(400)));
        assert!(editor.job.is_none());
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn ctrl_b_saves_then_compiles() {
        let path = temp_file("build", "= Title\n");
        let mut editor = open(&path);
        editor.out_dir = path.parent().unwrap().join("pages");
        editor.handle_key(key(KeyCode::Char('X')));
        editor.handle_key(ctrl('b'));
        assert!(!editor.dirty, "Ctrl-B must save first");
        assert_eq!(fs::read_to_string(&path).unwrap(), "X= Title\n");
        assert!(editor.job.is_some());

        wait_for_report(&mut editor);
        assert!(editor.job.is_none());
        let report = editor.report.as_ref().unwrap();
        assert!(report.ok, "{:?}", report.lines);
        assert!(editor.preview.has_page(), "the page did not load");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_compile_error_reaches_the_report() {
        let path = temp_file("builderr", "#nope()\n");
        let mut editor = open(&path);
        editor.out_dir = path.parent().unwrap().join("pages");
        editor.handle_key(ctrl('b'));
        wait_for_report(&mut editor);
        let report = editor.report.as_ref().unwrap();
        assert!(!report.ok);
        assert!(report.lines.iter().any(|l| l.contains(":1:")), "{:?}", report.lines);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
