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
use ratatui_textarea::{CursorMove, TextArea, WrapMode};

use crate::{
    compile::{self, Job, Report},
    fsutil,
    preview::{Preview, page_in},
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
    /// The main file of the project. The compile and the export use it instead of `path`.
    main: Option<PathBuf>,
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
    pages_root: PathBuf,
    /// The running compile. `None` when no compile runs.
    job: Option<Job>,
    /// The running PDF export. `None` when no export runs.
    export: Option<Job>,
    /// The PDF of the last good export. The pane shows it until the next export.
    exported: Option<PathBuf>,
    /// The report of the last finished compile.
    report: Option<Report>,
    /// Set while the editor looks for the right page after the document got shorter than the wanted page.
    /// It holds the page that the user wanted. See `poll_compile`.
    recover: Option<usize>,
    preview: Preview,
}

impl Editor {
    pub fn open(path: PathBuf, root: PathBuf, main: Option<PathBuf>, picker: Picker) -> io::Result<Self> {
        let text = fs::read_to_string(&path)?;
        let mut textarea = TextArea::new(text.lines().map(String::from).collect());
        textarea.set_cursor_line_style(Style::default());
        // A long line wraps on screen, at a word if possible. The file keeps the line as one line.
        textarea.set_wrap_mode(WrapMode::WordOrGlyph);
        Ok(Self {
            disk_time: disk_time(&path),
            conflict: false,
            path,
            root,
            main,
            textarea,
            dirty: false,
            last_edit: None,
            close_armed: false,
            message: String::new(),
            pages_root: compile::out_dir(),
            job: None,
            export: None,
            exported: None,
            report: None,
            recover: None,
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
        match fsutil::write_file(&self.path, text.as_bytes()) {
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
        } else if ctrl && key.code == KeyCode::Char('g') {
            self.go_to_first_error();
        } else if ctrl && key.code == KeyCode::Char('e') {
            if self.save(false) {
                // The new export replaces the old export. Dropping the old export kills its process.
                let target = self.compile_target().to_path_buf();
                self.export = Some(Job::start_pdf(&target, &self.root, target.with_extension("pdf")));
                self.exported = None;
                self.message = "Exporting the PDF...".into();
            }
        } else if key.modifiers.contains(KeyModifiers::ALT) && matches!(key.code, KeyCode::Down | KeyCode::Up) {
            // Alt with an arrow key arrives as one escape sequence. Alt with a letter arrives as Esc and
            // the letter, so a fast Esc and n looked like Alt-n.
            // A page turn renders the new page with a new compile. It does not save: the file on disk is
            // what the compile reads, and a refused save (a file that another program changed) must not
            // stop the turn. The old page stays on screen until the new page is ready.
            if self.preview.turn(key.code == KeyCode::Down) {
                self.recover = None; // the user's choice wins
                self.start_compile();
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

    /// Moves the cursor to the first error of the last report, if that error is in the open file.
    /// An error in another file only gets named: the cursor stays. The line and the column of the report
    /// are those of the text at the time of the compile. After more edits, a new compile makes them exact.
    fn go_to_first_error(&mut self) {
        let Some(error) = self.report.as_ref().and_then(Report::first_error) else {
            self.message = "No error to go to.".into();
            return;
        };
        let open_file = self.path.strip_prefix(&self.root).unwrap_or(&self.path);
        if error.file.as_deref() != Some(open_file) {
            let file = error.file.as_deref().unwrap_or(open_file);
            self.message = format!("The first error is in {}.", file.display());
            return;
        }
        // Typst counts from 1. The text area counts from 0, in characters, and it stops at the end of the text.
        let to_index = |number: usize| u16::try_from(number.saturating_sub(1)).unwrap_or(u16::MAX);
        let (line, column) = (error.line, error.column);
        self.textarea.move_cursor(CursorMove::Jump(to_index(line), to_index(column)));
        self.message = format!("Error at {line}:{column}: {}", error.message);
    }

    fn save_and_compile(&mut self) {
        if self.save(false) {
            self.start_compile();
        }
    }

    /// Starts a compile of the wanted page. The new compile replaces the running compile.
    fn start_compile(&mut self) {
        self.stop_compile();
        let target = self.compile_target().to_path_buf();
        let dir = compile::next_dir(&self.pages_root);
        self.job = Some(Job::start(&target, &self.root, dir, self.preview.wanted_page()));
    }

    /// The file that the compile and the export use: the main file, or else the open file.
    fn compile_target(&self) -> &Path {
        self.main.as_deref().unwrap_or(&self.path)
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
    ///
    /// Typst exits with success and writes no page when the document has fewer pages than the wanted page,
    /// for example after the user deleted pages while the preview showed the last page. Then the editor
    /// recovers in two steps, so that the user lands on the last page:
    /// 1. It compiles page 1 and learns the page count from the file name.
    /// 2. It compiles the last page (see `after_load`).
    ///
    /// `recover` is set between the steps, so a second "no page" result is an error and cannot loop.
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
            self.recover = None;
            let _ = fs::remove_dir_all(dir);
        } else if page_in(&dir).is_none() && self.recover.is_none() && self.preview.wanted_page() > 1 {
            let _ = fs::remove_dir_all(dir);
            self.recover = Some(self.preview.wanted_page());
            self.preview.want(1);
            self.start_compile();
            return true; // the old report stays on screen until the recovery ends
        } else {
            match self.preview.load(dir) {
                Ok(()) => self.after_load(),
                Err(err) => {
                    self.recover = None;
                    report = Report::failed(err);
                }
            }
        }
        self.report = Some(report);
        true
    }

    /// The second step of the recovery: if the page that the user wanted is beyond the end of the
    /// document, ask for the last page.
    fn after_load(&mut self) {
        let Some(wanted) = self.recover.take() else {
            return;
        };
        let count = self.preview.page_count();
        if wanted > count {
            self.preview.want(count);
            self.start_compile();
        }
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
            let name = pdf.strip_prefix(&self.root).unwrap_or(&pdf);
            self.message = format!("Exported {}", name.display());
            self.exported = Some(pdf);
        } else {
            self.message = "The PDF export failed. The pane shows the errors.".into();
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
        let name = self.path.strip_prefix(&self.root).unwrap_or(&self.path);
        let mut title = format!("{}{marker}", name.display());
        if let Some(main) = self.main.as_deref().filter(|main| *main != self.path) {
            let main = main.strip_prefix(&self.root).unwrap_or(main);
            title.push_str(&format!(" (main: {})", main.display()));
        }
        let block = Block::bordered().title(title);
        let inner = block.inner(body);
        frame.render_widget(block, body);
        frame.render_widget(&self.textarea, inner);
        frame.render_widget(self.compile_pane(), pane);
        self.preview.draw(frame, right);
        let hint = if self.message.is_empty() {
            "Ctrl-S save  Ctrl-B compile  Ctrl-E PDF  Ctrl-G error  Alt-Down/Alt-Up page  Esc back"
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
        open_with_main(path, None)
    }

    /// Opens the file with `main` as the main file. The root is the nearest folder above the file
    /// that holds `main`. Without a main file, the root is the folder of the file.
    fn open_with_main(path: &std::path::Path, main: Option<&str>) -> Editor {
        let root = match main {
            Some(name) => path.ancestors().skip(1).find(|dir| dir.join(name).exists()).unwrap().to_path_buf(),
            None => path.parent().unwrap().to_path_buf(),
        };
        let main = main.map(|name| root.join(name));
        let mut editor = Editor::open(path.to_path_buf(), root.clone(), main, Picker::halfblocks()).unwrap();
        editor.pages_root = root.join("pages");
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
    fn a_save_never_writes_through_a_planted_symlink() {
        // A cloned repository can contain a symlink at the old, fixed temp path.
        let path = temp_file("planted", "= Hi\n");
        let dir = path.parent().unwrap();
        let victim = dir.join("victim.txt");
        fs::write(&victim, "IMPORTANT\n").unwrap();
        std::os::unix::fs::symlink(&victim, dir.join(".doc.typ.lazytypst-tmp")).unwrap();

        let mut editor = open(&path);
        editor.handle_key(key(KeyCode::Char('X')));
        editor.handle_key(ctrl('s'));
        assert_eq!(fs::read_to_string(&victim).unwrap(), "IMPORTANT\n", "the save wrote through the link");
        assert!(!fs::symlink_metadata(&path).unwrap().file_type().is_symlink(), "doc.typ became a link");
        assert_eq!(fs::read_to_string(&path).unwrap(), "X= Hi\n");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_new_file_is_not_created_through_a_dangling_symlink() {
        let path = temp_file("dangling", "text\n");
        let dir = path.parent().unwrap();
        let mut editor = open(&path);
        editor.handle_key(key(KeyCode::Char('X')));
        fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(dir.join("elsewhere.txt"), &path).unwrap();

        editor.handle_key(ctrl('s'));
        assert!(!dir.join("elsewhere.txt").exists(), "the save created the link target");
        assert!(editor.message.contains("Save failed"), "{}", editor.message);
        fs::remove_dir_all(dir).unwrap();
    }

    /// Makes `main.typ`, which includes `chapters/one.typ` with `chapter`. Returns the path of the chapter.
    fn book(name: &str, chapter: &str) -> PathBuf {
        let main = temp_file(name, "#include \"chapters/one.typ\"\n");
        let dir = main.parent().unwrap().to_path_buf();
        fs::rename(&main, dir.join("main.typ")).unwrap();
        fs::create_dir_all(dir.join("chapters")).unwrap();
        fs::write(dir.join("chapters").join("one.typ"), chapter).unwrap();
        dir.join("chapters").join("one.typ")
    }

    #[test]
    fn with_a_main_file_ctrl_e_exports_the_main_file() {
        let chapter = book("main-export", "= One\n");
        let dir = chapter.parent().unwrap().parent().unwrap().to_path_buf();
        let mut editor = open_with_main(&chapter, Some("main.typ"));
        editor.handle_key(ctrl('e'));
        wait_for_export(&mut editor);

        assert!(dir.join("main.pdf").exists(), "main.pdf is missing");
        assert!(!dir.join("chapters").join("one.pdf").exists(), "the chapter was exported");
        assert!(editor.exported.as_ref().unwrap().ends_with("main.pdf"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn with_a_main_file_ctrl_b_compiles_the_main_file_and_names_the_chapter_in_errors() {
        let chapter = book("main-error", "#nope()\n");
        let dir = chapter.parent().unwrap().parent().unwrap().to_path_buf();
        let mut editor = open_with_main(&chapter, Some("main.typ"));
        // The buffer is clean. Ctrl-B must still compile.
        editor.handle_key(ctrl('b'));
        assert!(editor.job.is_some());
        wait_for_report(&mut editor);

        let report = editor.report.as_ref().unwrap();
        assert!(!report.ok);
        assert!(report.lines.iter().any(|l| l.starts_with("chapters/one.typ:1:")), "{:?}", report.lines);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn with_a_main_file_the_autosave_of_a_chapter_updates_the_preview_of_the_main_file() {
        let chapter = book("main-live", "= One\n");
        let dir = chapter.parent().unwrap().parent().unwrap().to_path_buf();
        let mut editor = open_with_main(&chapter, Some("main.typ"));
        editor.handle_key(key(KeyCode::Char('X')));
        assert!(editor.tick(Instant::now() + Duration::from_millis(400)));

        assert_eq!(fs::read_to_string(&chapter).unwrap(), "X= One\n", "the chapter must be saved");
        assert_eq!(fs::read_to_string(dir.join("main.typ")).unwrap(), "#include \"chapters/one.typ\"\n");
        wait_for_report(&mut editor);
        assert!(editor.report.as_ref().unwrap().ok);
        assert!(editor.preview.has_page());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_title_names_the_main_file_when_it_is_not_the_open_file() {
        let chapter = book("main-title", "= One\n");
        let dir = chapter.parent().unwrap().parent().unwrap().to_path_buf();
        let mut editor = open_with_main(&chapter, Some("main.typ"));
        let text = screen_text(&mut editor);
        assert!(text.contains("chapters/one.typ (main: main.typ)"), "{text}");

        let mut editor = open_with_main(&dir.join("main.typ"), Some("main.typ"));
        let text = screen_text(&mut editor);
        assert!(text.contains("main.typ") && !text.contains("(main:"), "{text}");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn without_a_main_file_the_open_file_is_the_compile_target() {
        let chapter = book("main-none", "= One\n");
        let dir = chapter.parent().unwrap().parent().unwrap().to_path_buf();
        let mut editor = open(&chapter);
        editor.handle_key(ctrl('e'));
        wait_for_export(&mut editor);
        assert!(dir.join("chapters").join("one.pdf").exists());
        assert!(!dir.join("main.pdf").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    /// Compiles the file in the editor and waits for the report.
    fn compile_and_wait(editor: &mut Editor) {
        editor.handle_key(ctrl('b'));
        wait_for_report(editor);
    }

    #[test]
    fn ctrl_g_moves_the_cursor_to_the_first_error() {
        let path = temp_file("goto", "= Title\n#nope()\n");
        let mut editor = open(&path);
        compile_and_wait(&mut editor);
        editor.handle_key(ctrl('g'));
        assert_eq!(editor.textarea.cursor(), (1, 0));
        assert!(editor.message.contains("2:1"), "{}", editor.message);
        assert!(!editor.dirty, "Ctrl-G must not change the text");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn ctrl_g_uses_the_column_of_the_error() {
        let path = temp_file("gotocolumn", "ab #nope()\n");
        let mut editor = open(&path);
        compile_and_wait(&mut editor);
        editor.handle_key(ctrl('g'));
        assert_eq!(editor.textarea.cursor(), (0, 3), "the cursor must stand on the #");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn ctrl_g_counts_the_column_in_characters_not_bytes() {
        for text in ["é #nope()", "संस्कृतम् #nope()", "👨\u{200d}👩\u{200d}👧\u{200d}👦 #nope()"] {
            let path = temp_file("gotounicode", &format!("{text}\n"));
            let mut editor = open(&path);
            compile_and_wait(&mut editor);
            editor.handle_key(ctrl('g'));
            let cursor = editor.textarea.cursor();
            let (row, column) = (cursor.0, cursor.1);
            assert_eq!(editor.textarea.lines()[row].chars().nth(column), Some('#'), "{text}");
            fs::remove_dir_all(path.parent().unwrap()).unwrap();
        }
    }

    #[test]
    fn ctrl_g_with_an_error_in_another_file_names_the_file_and_keeps_the_cursor() {
        let chapter = book("gotoother", "= One\n");
        let dir = chapter.parent().unwrap().parent().unwrap().to_path_buf();
        fs::write(dir.join("main.typ"), "#nope()\n#include \"chapters/one.typ\"\n").unwrap();
        let mut editor = open_with_main(&chapter, Some("main.typ"));
        editor.handle_key(key(KeyCode::Right));
        editor.handle_key(key(KeyCode::Right));
        compile_and_wait(&mut editor);

        editor.handle_key(ctrl('g'));
        assert_eq!(editor.textarea.cursor(), (0, 2), "the cursor must stay");
        assert!(editor.message.contains("main.typ"), "{}", editor.message);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn ctrl_g_without_an_error_says_so_and_keeps_the_cursor() {
        let path = temp_file("gotonone", "= Title\ntext\n");
        let mut editor = open(&path);
        editor.handle_key(key(KeyCode::Down));
        editor.handle_key(ctrl('g')); // before any compile
        assert_eq!(editor.textarea.cursor(), (1, 0));
        assert!(editor.message.contains("No error"), "{}", editor.message);

        compile_and_wait(&mut editor);
        assert!(editor.report.as_ref().unwrap().ok);
        editor.handle_key(ctrl('g'));
        assert_eq!(editor.textarea.cursor(), (1, 0));
        assert!(editor.message.contains("No error"), "{}", editor.message);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn ctrl_g_ignores_a_warning() {
        let path = temp_file("gotowarning", "#set text(font: \"NoSuchFont\")\nHello\n");
        let mut editor = open(&path);
        compile_and_wait(&mut editor);
        assert!(editor.report.as_ref().unwrap().lines.iter().any(|l| l.contains("warning")));
        editor.handle_key(ctrl('g'));
        assert_eq!(editor.textarea.cursor(), (0, 0));
        assert!(editor.message.contains("No error"), "{}", editor.message);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn ctrl_g_with_an_old_report_stays_inside_the_text() {
        let path = temp_file("gotostale", "one\ntwo\n");
        let mut editor = open(&path);
        editor.report = Some(Report::new(false, vec!["doc.typ:50:90: error: x".into()]));
        editor.handle_key(ctrl('g'));
        assert_eq!(editor.textarea.cursor(), (1, 3), "the cursor goes to the end of the last line");
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
    fn the_title_shows_the_path_relative_to_the_root() {
        let path = temp_file("title", "text\n");
        let mut editor = open(&path);
        let text = screen_text(&mut editor);
        assert!(text.contains("doc.typ"), "{text}");
        assert!(!text.contains(&path.parent().unwrap().display().to_string()), "{text}");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_long_line_wraps_on_screen_but_stays_one_line_in_the_file() {
        let long = format!("{}END", "word ".repeat(30));
        let path = temp_file("wrap", &format!("{long}\n"));
        let mut editor = open(&path);
        // The editor pane is 48 cells wide inside its border, so the line needs 4 screen rows.
        assert!(screen_text(&mut editor).contains("END"), "the end of the line is not on screen");

        editor.handle_key(key(KeyCode::Char('X')));
        editor.handle_key(ctrl('s'));
        assert_eq!(fs::read_to_string(&path).unwrap(), format!("X{long}\n"));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
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
    fn alt_down_and_alt_up_render_the_new_page_with_a_new_compile() {
        let path = temp_file("pages", "= One\n#pagebreak()\n= Two\n#pagebreak()\n= Three\n");
        let mut editor = open(&path);
        editor.handle_key(ctrl('b'));
        wait_for_report(&mut editor);
        assert!(screen_text(&mut editor).contains("Preview 1/3"));

        editor.handle_key(alt(KeyCode::Down));
        assert!(editor.job.is_some(), "a page turn must start a compile");
        assert!(screen_text(&mut editor).contains("Preview 1/3"), "the old page must stay until the new page is ready");
        wait_for_report(&mut editor);
        assert!(screen_text(&mut editor).contains("Preview 2/3"));

        // Fast presses replace each other: the last press decides the page.
        editor.handle_key(alt(KeyCode::Down));
        editor.handle_key(alt(KeyCode::Down));
        wait_for_report(&mut editor);
        assert!(screen_text(&mut editor).contains("Preview 3/3"));

        editor.handle_key(alt(KeyCode::Down));
        assert!(editor.job.is_none(), "page 3 is the last page, so no compile starts");
        editor.handle_key(alt(KeyCode::Up));
        wait_for_report(&mut editor);
        assert!(screen_text(&mut editor).contains("Preview 2/3"));
        assert!(!editor.dirty, "the page keys must not change the text");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    /// Waits up to 30 seconds until no compile runs. A recovery compile starts the next compile at once.
    fn wait_for_idle(editor: &mut Editor) {
        let start = Instant::now();
        while editor.job.is_some() {
            editor.tick(Instant::now());
            assert!(start.elapsed() < Duration::from_secs(30), "the compiles did not end");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Shows page 3 of a document of 3 pages. Then another program makes the document `new_text`, and Ctrl-B compiles it.
    fn shrink_from_page_3(name: &str, new_text: &str) -> (PathBuf, Editor) {
        let path = temp_file(name, "= One\n#pagebreak()\n= Two\n#pagebreak()\n= Three\n");
        let mut editor = open(&path);
        editor.handle_key(ctrl('b'));
        wait_for_idle(&mut editor);
        editor.handle_key(alt(KeyCode::Down));
        editor.handle_key(alt(KeyCode::Down));
        wait_for_idle(&mut editor);
        assert!(screen_text(&mut editor).contains("Preview 3/3"));

        fs::write(&path, new_text).unwrap();
        editor.handle_key(ctrl('b')); // the buffer is clean, so this compiles the new text from the disk
        wait_for_idle(&mut editor);
        (path, editor)
    }

    #[test]
    fn a_document_that_gets_shorter_than_the_wanted_page_shows_the_last_page() {
        let (path, mut editor) = shrink_from_page_3("shrink2", "= One\n#pagebreak()\n= Two\n");
        let text = screen_text(&mut editor);
        assert!(text.contains("Preview 2/2"), "{text}");
        assert!(editor.report.as_ref().unwrap().ok);
        assert_eq!(editor.preview.wanted_page(), 2);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_document_that_gets_down_to_one_page_shows_that_page() {
        let (path, mut editor) = shrink_from_page_3("shrink1", "= Only\n");
        let text = screen_text(&mut editor);
        assert!(text.contains("Preview 1/1"), "{text}");
        assert!(editor.report.as_ref().unwrap().ok);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_compile_that_gives_success_and_no_page_for_page_1_is_an_error_and_never_loops() {
        let path = temp_file("nopage", "= One\n");
        let mut editor = open(&path);
        let dir = path.parent().unwrap().join("empty-dir");
        fs::create_dir_all(&dir).unwrap();
        editor.job = Some(Job::ended_with_success(dir));
        wait_for_report(&mut editor);
        assert!(editor.job.is_none(), "a new compile started");
        let report = editor.report.as_ref().unwrap();
        assert!(!report.ok && report.lines[0].contains("no page file"), "{:?}", report.lines);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_recovery_that_finds_no_page_stops_with_an_error() {
        let (path, mut editor) = shrink_from_page_3("noloop", "= One\n#pagebreak()\n= Two\n");
        editor.preview.want(9);
        editor.recover = Some(9); // a recovery is under way, and now the compile of page 1 gives no file
        let dir = path.parent().unwrap().join("empty-dir2");
        fs::create_dir_all(&dir).unwrap();
        editor.job = Some(Job::ended_with_success(dir));
        wait_for_report(&mut editor);
        assert!(editor.job.is_none(), "the recovery looped");
        assert!(editor.recover.is_none());
        assert!(!editor.report.as_ref().unwrap().ok);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_page_turn_ends_a_recovery() {
        let path = temp_file("turnrecover", "= One\n#pagebreak()\n= Two\n#pagebreak()\n= Three\n");
        let mut editor = open(&path);
        editor.handle_key(ctrl('b'));
        wait_for_idle(&mut editor);
        editor.recover = Some(9);
        editor.handle_key(alt(KeyCode::Down));
        assert!(editor.recover.is_none(), "the recovery must end when the user turns a page");
        wait_for_idle(&mut editor);
        assert!(screen_text(&mut editor).contains("Preview 2/3"), "the user's page turn must win");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_page_turn_works_while_the_file_has_a_conflict_and_does_not_save() {
        let path = temp_file("turnconflict", "= One\n#pagebreak()\n= Two\n");
        let mut editor = open(&path);
        editor.handle_key(ctrl('b'));
        wait_for_report(&mut editor);

        editor.handle_key(key(KeyCode::Char('X')));
        fs::write(&path, "= One\n#pagebreak()\n= Two\nchanged outside\n").unwrap();
        editor.handle_key(alt(KeyCode::Down));
        assert!(editor.job.is_some(), "the turn must start a compile");
        assert!(editor.dirty, "the turn must not save");
        assert_eq!(fs::read_to_string(&path).unwrap(), "= One\n#pagebreak()\n= Two\nchanged outside\n");
        wait_for_report(&mut editor);
        assert!(screen_text(&mut editor).contains("Preview 2/2"));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn the_page_number_stays_after_an_edit() {
        let path = temp_file("pageedit", "= One\n#pagebreak()\n= Two\n#pagebreak()\n= Three\n");
        let mut editor = open(&path);
        editor.handle_key(ctrl('b'));
        wait_for_report(&mut editor);
        editor.handle_key(alt(KeyCode::Down));
        wait_for_report(&mut editor);
        assert!(screen_text(&mut editor).contains("Preview 2/3"));

        editor.handle_key(key(KeyCode::Char('X')));
        assert!(editor.tick(Instant::now() + Duration::from_millis(400)));
        wait_for_report(&mut editor);
        assert!(screen_text(&mut editor).contains("Preview 2/3"), "the live compile must render page 2");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn each_compile_folder_holds_exactly_one_png() {
        let path = temp_file("onepng", "= One\n#pagebreak()\n= Two\n#pagebreak()\n= Three\n");
        let mut editor = open(&path);
        editor.handle_key(ctrl('b'));
        wait_for_report(&mut editor);
        editor.handle_key(alt(KeyCode::Down));
        wait_for_report(&mut editor);

        let folders: Vec<_> = fs::read_dir(&editor.pages_root).unwrap().map(|e| e.unwrap().path()).collect();
        assert_eq!(folders.len(), 1, "only the folder of the shown page stays");
        let files: Vec<_> = fs::read_dir(&folders[0]).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(files, ["page-2-of-3.png"]);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn an_error_on_a_later_page_shows_while_page_1_is_wanted() {
        let path = temp_file("laterror", "= One\n#pagebreak()\n= Two\n#pagebreak()\n#nope()\n");
        let mut editor = open(&path);
        editor.handle_key(ctrl('b'));
        wait_for_report(&mut editor);
        let report = editor.report.as_ref().unwrap();
        assert!(!report.ok);
        assert!(report.lines.iter().any(|l| l.contains(":5:")), "{:?}", report.lines);
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
    fn the_status_line_reports_the_end_of_the_export() {
        let path = temp_file("exportstatus", "= Title\n");
        let mut editor = open(&path);
        editor.handle_key(ctrl('e'));
        assert!(editor.message.contains("Exporting"));
        wait_for_export(&mut editor);
        assert!(!editor.message.contains("Exporting"), "{}", editor.message);
        assert!(editor.message.contains("doc.pdf"), "{}", editor.message);

        fs::write(&path, "#nope()\n").unwrap();
        let mut editor = open(&path);
        editor.handle_key(ctrl('e'));
        wait_for_export(&mut editor);
        assert!(editor.message.contains("failed"), "{}", editor.message);
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
