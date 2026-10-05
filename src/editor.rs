use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
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

const CONFLICT: &str = "The file changed on disk. Ctrl-S overwrites it with this text.";

/// The modification time of the file, or `None` if the file cannot be read.
fn disk_time(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|meta| meta.modified()).ok()
}

/// Writes `text` to the file at `path` so that a crash leaves the old file or the new file, never a cut file.
/// The text goes to a hidden temp file next to the real file. Then a rename replaces the real file.
/// A symlink stays a symlink, and the permissions stay. A hard link to the old file keeps the old text.
fn write_file(path: &Path, text: &str) -> io::Result<()> {
    let target = match fs::canonicalize(path) {
        Ok(target) => target,
        // The file is gone. Then no old text can be cut, and a plain write creates the file again.
        Err(err) if err.kind() == io::ErrorKind::NotFound => return fs::write(path, text),
        Err(err) => return Err(err),
    };
    // A rename can replace a file that the user cannot write. This open makes the same check as a direct write.
    fs::OpenOptions::new().write(true).open(&target)?;
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let temp = target.with_file_name(format!(".{name}.lazytypst-tmp"));
    let result = fs::write(&temp, text)
        .and_then(|()| fs::set_permissions(&temp, fs::metadata(&target)?.permissions()))
        .and_then(|()| fs::rename(&temp, &target));
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

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
    /// The project folder. Typst can read the files under it.
    root: PathBuf,
    textarea: TextArea<'static>,
    /// True when the buffer has text that is not on disk.
    dirty: bool,
    /// The modification time of the file when the editor last read it or wrote it.
    disk_time: Option<SystemTime>,
    /// True when the file changed on disk after that time. Then only Ctrl-S writes the file.
    conflict: bool,
    /// The time of the last edit that no save has covered yet.
    last_edit: Option<Instant>,
    /// True after an Esc that could not save the text. A second Esc closes without a save.
    close_armed: bool,
    message: String,
    /// The folder that holds one new subfolder with the PNG pages for each compile.
    out_dir: PathBuf,
    /// The running compile. `None` when no compile runs.
    job: Option<Job>,
    /// The running PDF export. `None` when no export runs.
    export: Option<Job>,
    /// The PDF of the last good export. The pane shows it until the next export.
    exported: Option<PathBuf>,
    /// The report of the last finished compile.
    report: Option<Report>,
    preview: Preview,
}

impl Editor {
    pub fn open(path: PathBuf, root: PathBuf, picker: Picker) -> io::Result<Self> {
        let text = fs::read_to_string(&path)?;
        let mut textarea = TextArea::new(text.lines().map(String::from).collect());
        textarea.set_cursor_line_style(Style::default());
        Ok(Self {
            disk_time: disk_time(&path),
            conflict: false,
            path,
            root,
            textarea,
            dirty: false,
            last_edit: None,
            close_armed: false,
            message: String::new(),
            out_dir: compile::out_dir(),
            job: None,
            export: None,
            exported: None,
            report: None,
            preview: Preview::new(picker),
        })
    }

    /// Writes the buffer to the file if it has edits. Returns false if the file was not written:
    /// the write failed, or another program changed the file and `overwrite` is false.
    fn save(&mut self, overwrite: bool) -> bool {
        if !self.dirty {
            return true;
        }
        if !overwrite && (self.conflict || disk_time(&self.path) != self.disk_time) {
            self.conflict = true;
            self.message = CONFLICT.into();
            return false;
        }
        let mut text = self.textarea.lines().join("\n");
        if !text.is_empty() {
            text.push('\n');
        }
        // CRLF line ends become LF.
        match write_file(&self.path, &text) {
            Ok(()) => {
                self.dirty = false;
                self.conflict = false;
                self.disk_time = disk_time(&self.path);
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
        let armed = std::mem::take(&mut self.close_armed);
        self.message.clear();
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && key.code == KeyCode::Char('s') {
            if !self.dirty {
                self.message = "No changes to save".into();
            }
            self.save(true);
        } else if ctrl && key.code == KeyCode::Char('b') {
            self.save_and_compile();
        } else if ctrl && key.code == KeyCode::Char('e') {
            if self.save(false) {
                // The new export replaces the old export. Dropping the old export kills its process.
                self.export = Some(Job::start_pdf(&self.path, &self.root, self.path.with_extension("pdf")));
                self.exported = None;
                self.message = "Exporting the PDF...".into();
            }
        } else if key.modifiers.contains(KeyModifiers::ALT) && matches!(key.code, KeyCode::Down | KeyCode::Up) {
            // Alt with an arrow key arrives as one escape sequence. Alt with a letter arrives as Esc and
            // the letter, so a fast Esc and n looked like Alt-n.
            if let Err(err) = self.preview.turn(key.code == KeyCode::Down) {
                self.message = err;
            }
        } else if key.code == KeyCode::Esc {
            if armed || self.save(false) {
                return Action::Close;
            }
            // `save` put the reason in the message.
            self.close_armed = true;
            self.message.push_str(" Esc again closes without a save.");
        } else if self.textarea.input(key) {
            self.dirty = true;
            self.last_edit = Some(Instant::now());
        }
        Action::Stay
    }

    fn save_and_compile(&mut self) {
        if self.save(false) {
            self.stop_compile();
            self.job = Some(Job::start(&self.path, &self.root, compile::next_dir(&self.out_dir)));
        }
    }

    /// Kills the running compile and deletes its page folder.
    fn stop_compile(&mut self) {
        if let Some(job) = self.job.take() {
            let dir = job.output().to_path_buf();
            drop(job);
            let _ = fs::remove_dir_all(dir);
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
        changed |= self.poll_export();
        changed
    }

    /// Takes the report of a finished compile. Returns true when the screen must redraw.
    fn poll_compile(&mut self) -> bool {
        let Some(job) = &mut self.job else {
            return false;
        };
        let Some(mut report) = job.try_report() else {
            return false;
        };
        let dir = job.output().to_path_buf();
        self.job = None;
        if !report.ok {
            let _ = fs::remove_dir_all(dir);
        } else if let Err(err) = self.preview.load(dir) {
            report = Report::failed(err);
        }
        self.report = Some(report);
        true
    }

    /// Takes the report of a finished export. The pane shows the path of the PDF, or the errors.
    fn poll_export(&mut self) -> bool {
        let Some(job) = &mut self.export else {
            return false;
        };
        let Some(report) = job.try_report() else {
            return false;
        };
        let pdf = job.output().to_path_buf();
        self.export = None;
        if report.ok {
            self.exported = Some(pdf);
        } else {
            self.report = Some(report);
        }
        true
    }

    fn compile_pane(&self) -> Paragraph<'static> {
        let (mut color, mut lines) = match &self.report {
            Some(report) => {
                let color = if report.ok { Color::Green } else { Color::Red };
                let head = report.ok.then(|| "OK".to_string());
                (color, head.into_iter().chain(report.lines.iter().cloned()).collect())
            }
            None => (Color::Reset, vec!["Press Ctrl-B to compile.".to_string()]),
        };
        let mut title = "Compile";
        if self.job.is_some() {
            // The last report stays on screen until the new report replaces it.
            color = Color::Yellow;
            title = "Compile (running)";
            if self.report.is_none() {
                lines = vec!["Compiling...".to_string()];
            }
        }
        if let Some(pdf) = &self.exported {
            lines.push(format!("Exported {}", pdf.display()));
        }
        Paragraph::new(lines.join("\n"))
            .wrap(Wrap { trim: false })
            .block(Block::bordered().title(title).border_style(Style::new().fg(color)))
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
            "Ctrl-S save  Ctrl-B compile  Ctrl-E PDF  Alt-Down/Alt-Up page  Esc back"
        } else {
            &self.message
        };
        frame.render_widget(Paragraph::new(hint), status);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    /// Opens the file. The pages of each compile go to the folder `pages` next to the file,
    /// so no test writes into the temporary folder that the whole program shares.
    fn open(path: &std::path::Path) -> Editor {
        let root = path.parent().unwrap().to_path_buf();
        let mut editor = Editor::open(path.to_path_buf(), root.clone(), Picker::halfblocks()).unwrap();
        editor.out_dir = root.join("pages");
        editor
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
    fn a_save_writes_through_a_symlink_and_keeps_it() {
        let real = temp_file("symlink", "text\n");
        let link = real.with_file_name("link.typ");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let mut editor = open(&link);
        editor.handle_key(key(KeyCode::Char('X')));
        editor.handle_key(ctrl('s'));
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(fs::read_to_string(&real).unwrap(), "Xtext\n");
        fs::remove_dir_all(real.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_save_keeps_the_permissions_and_leaves_no_temp_file() {
        use std::os::unix::fs::PermissionsExt;
        let path = temp_file("perms", "text\n");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        let mut editor = open(&path);
        editor.handle_key(key(KeyCode::Char('X')));
        editor.handle_key(ctrl('s'));
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o640);
        let names: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["doc.typ"], "a temp file is left behind");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn ctrl_s_recreates_a_file_that_another_program_deleted() {
        let path = temp_file("deleted", "text\n");
        let mut editor = open(&path);
        editor.handle_key(key(KeyCode::Char('X')));
        fs::remove_file(&path).unwrap();
        editor.handle_key(ctrl('s'));
        assert_eq!(fs::read_to_string(&path).unwrap(), "Xtext\n");
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
    fn esc_saves_the_text_then_closes() {
        let path = temp_file("escsave", "text\n");
        let mut editor = open(&path);
        editor.handle_key(key(KeyCode::Char('X')));
        assert!(matches!(editor.handle_key(key(KeyCode::Esc)), Action::Close));
        assert_eq!(fs::read_to_string(&path).unwrap(), "Xtext\n");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn esc_in_a_conflict_warns_then_closes_and_keeps_the_disk_version() {
        let path = temp_file("escconflict", "text\n");
        let mut editor = open(&path);
        editor.handle_key(key(KeyCode::Char('X')));
        fs::write(&path, "outside\n").unwrap();

        assert!(matches!(editor.handle_key(key(KeyCode::Esc)), Action::Stay));
        assert!(editor.message.contains("changed on disk"), "{}", editor.message);
        assert!(editor.message.contains("Esc again"), "{}", editor.message);
        assert!(matches!(editor.handle_key(key(KeyCode::Esc)), Action::Close));
        assert_eq!(fs::read_to_string(&path).unwrap(), "outside\n");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn esc_after_a_failed_write_warns_then_closes() {
        use std::os::unix::fs::PermissionsExt;
        let path = temp_file("escreadonly", "text\n");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
        let mut editor = open(&path);
        editor.handle_key(key(KeyCode::Char('X')));

        assert!(matches!(editor.handle_key(key(KeyCode::Esc)), Action::Stay));
        assert!(editor.message.contains("Save failed"), "{}", editor.message);
        assert!(matches!(editor.handle_key(key(KeyCode::Esc)), Action::Close));
        assert_eq!(fs::read_to_string(&path).unwrap(), "text\n");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn another_key_cancels_the_close_warning() {
        let path = temp_file("cancel", "text\n");
        let mut editor = open(&path);
        editor.handle_key(key(KeyCode::Char('X')));
        fs::write(&path, "outside\n").unwrap();
        editor.handle_key(key(KeyCode::Esc));
        editor.handle_key(key(KeyCode::Char('Y')));
        assert!(matches!(editor.handle_key(key(KeyCode::Esc)), Action::Stay));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn ctrl_b_on_an_unchanged_buffer_does_not_write_the_file() {
        let path = temp_file("nowrite", "");
        fs::write(&path, "= A\r\nb\r\n").unwrap();
        let mut editor = open(&path);
        editor.handle_key(ctrl('b'));
        assert!(editor.job.is_some(), "Ctrl-B must still compile");
        assert_eq!(fs::read(&path).unwrap(), b"= A\r\nb\r\n");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn ctrl_b_does_not_overwrite_a_change_made_by_another_program() {
        let path = temp_file("outside", "= Mine\n");
        let mut editor = open(&path);
        editor.handle_key(key(KeyCode::Char('X')));
        fs::write(&path, "= Changed outside\n").unwrap();

        editor.handle_key(ctrl('b'));
        assert_eq!(fs::read_to_string(&path).unwrap(), "= Changed outside\n");
        assert!(editor.message.contains("changed on disk"), "{}", editor.message);
        assert!(editor.job.is_none(), "no compile of a file that was not saved");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn the_autosave_does_not_overwrite_a_change_made_by_another_program() {
        let path = temp_file("outside-auto", "= Mine\n");
        let mut editor = open(&path);
        editor.handle_key(key(KeyCode::Char('X')));
        fs::write(&path, "= Changed outside\n").unwrap();

        editor.tick(Instant::now() + Duration::from_millis(400));
        assert_eq!(fs::read_to_string(&path).unwrap(), "= Changed outside\n");
        assert!(editor.dirty);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn ctrl_s_overwrites_a_change_made_by_another_program() {
        let path = temp_file("overwrite", "= Mine\n");
        let mut editor = open(&path);
        editor.handle_key(key(KeyCode::Char('X')));
        fs::write(&path, "= Changed outside\n").unwrap();
        editor.handle_key(ctrl('b')); // finds the change and refuses

        editor.handle_key(ctrl('s'));
        assert_eq!(fs::read_to_string(&path).unwrap(), "X= Mine\n");
        assert!(!editor.dirty);
        // After the overwrite, the next save works without a warning.
        editor.handle_key(key(KeyCode::Char('Y')));
        editor.handle_key(ctrl('b'));
        assert_eq!(fs::read_to_string(&path).unwrap(), "XY= Mine\n");
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

    /// Draws the editor on a 100 by 24 screen and returns all the text on it.
    fn screen_text(editor: &mut Editor) -> String {
        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        terminal.draw(|frame| editor.draw(frame)).unwrap();
        terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect()
    }

    #[test]
    fn the_pane_says_compiling_before_the_first_report() {
        let path = temp_file("pane-first", "= Title\n");
        let mut editor = open(&path);
        assert!(screen_text(&mut editor).contains("Press Ctrl-B to compile."));

        editor.handle_key(ctrl('b'));
        let text = screen_text(&mut editor);
        assert!(text.contains("Compile (running)") && text.contains("Compiling..."), "{text}");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn the_pane_keeps_the_last_report_while_a_compile_runs() {
        let path = temp_file("pane-keep", "= Title\n");
        let mut editor = open(&path);
        editor.report = Some(Report::failed("old error text"));

        editor.handle_key(ctrl('b'));
        let text = screen_text(&mut editor);
        assert!(text.contains("Compile (running)"), "{text}");
        assert!(text.contains("old error text"), "{text}");
        assert!(!text.contains("Compiling..."), "{text}");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn only_the_folder_of_the_last_compile_stays() {
        let path = temp_file("folders", "= Title\n");
        let mut editor = open(&path);
        let pages = path.parent().unwrap().join("pages");

        editor.handle_key(ctrl('b'));
        editor.handle_key(ctrl('b')); // kills the first compile and deletes its folder
        wait_for_report(&mut editor);
        editor.handle_key(ctrl('b'));
        wait_for_report(&mut editor);
        assert!(editor.report.as_ref().unwrap().ok);
        assert_eq!(fs::read_dir(&pages).unwrap().count(), 1);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_failed_compile_leaves_no_folder() {
        let path = temp_file("nofolder", "#nope()\n");
        let mut editor = open(&path);
        let pages = path.parent().unwrap().join("pages");
        editor.handle_key(ctrl('b'));
        wait_for_report(&mut editor);
        assert!(!editor.report.as_ref().unwrap().ok);
        assert_eq!(fs::read_dir(&pages).unwrap().count(), 0);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    fn alt(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::ALT)
    }

    #[test]
    fn alt_down_and_alt_up_turn_the_preview_page() {
        let path = temp_file("pages", "= One\n#pagebreak()\n= Two\n#pagebreak()\n= Three\n");
        let mut editor = open(&path);
        editor.handle_key(ctrl('b'));
        wait_for_report(&mut editor);
        assert!(screen_text(&mut editor).contains("Preview 1/3"));

        editor.handle_key(alt(KeyCode::Down));
        assert!(screen_text(&mut editor).contains("Preview 2/3"));
        editor.handle_key(alt(KeyCode::Down));
        editor.handle_key(alt(KeyCode::Down));
        assert!(screen_text(&mut editor).contains("Preview 3/3"));
        editor.handle_key(alt(KeyCode::Up));
        assert!(screen_text(&mut editor).contains("Preview 2/3"));
        assert!(!editor.dirty, "the page keys must not change the text");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    /// Waits up to 20 seconds for the running PDF export to finish.
    fn wait_for_export(editor: &mut Editor) {
        let start = Instant::now();
        while editor.export.is_some() {
            editor.tick(Instant::now());
            assert!(start.elapsed() < Duration::from_secs(20), "export did not finish");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn ctrl_e_saves_then_exports_a_pdf_next_to_the_file() {
        let path = temp_file("export", "= Title\n");
        let mut editor = open(&path);
        editor.handle_key(key(KeyCode::Char('X')));
        editor.handle_key(ctrl('e'));
        assert!(!editor.dirty, "Ctrl-E must save first");
        assert_eq!(fs::read_to_string(&path).unwrap(), "X= Title\n");
        assert!(editor.export.is_some());

        wait_for_export(&mut editor);
        let pdf = path.with_extension("pdf");
        assert_eq!(&fs::read(&pdf).unwrap()[..4], b"%PDF");
        assert_eq!(editor.exported.as_deref(), Some(pdf.as_path()));
        assert!(screen_text(&mut editor).contains("doc.pdf"));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn the_export_path_stays_on_screen_after_a_later_compile() {
        let path = temp_file("exportstays", "= Title\n");
        let mut editor = open(&path);
        editor.handle_key(ctrl('e'));
        wait_for_export(&mut editor);
        editor.handle_key(ctrl('b'));
        wait_for_report(&mut editor);
        let text = screen_text(&mut editor);
        assert!(text.contains("OK"), "{text}");
        assert!(text.contains("Exported") && text.contains("doc.pdf"), "{text}");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_failed_export_shows_the_errors_and_writes_no_file() {
        let path = temp_file("exporterr", "#nope()\n");
        let mut editor = open(&path);
        editor.handle_key(ctrl('e'));
        wait_for_export(&mut editor);
        let report = editor.report.as_ref().unwrap();
        assert!(!report.ok);
        assert!(report.lines.iter().any(|l| l.contains(":1:")), "{:?}", report.lines);
        assert!(!path.with_extension("pdf").exists());
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn plain_n_and_p_type_letters() {
        let path = temp_file("letters", "");
        let mut editor = open(&path);
        editor.handle_key(key(KeyCode::Char('n')));
        editor.handle_key(key(KeyCode::Char('p')));
        assert_eq!(editor.textarea.lines(), ["np"]);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn ctrl_b_saves_then_compiles() {
        let path = temp_file("build", "= Title\n");
        let mut editor = open(&path);
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
        editor.handle_key(ctrl('b'));
        wait_for_report(&mut editor);
        let report = editor.report.as_ref().unwrap();
        assert!(!report.ok);
        assert!(report.lines.iter().any(|l| l.contains(":1:")), "{:?}", report.lines);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
