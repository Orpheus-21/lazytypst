use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

use ratatui::{
    Frame,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};
use ratatui_image::picker::Picker;
use ratatui_textarea::{CursorMove, TextArea, WrapMode};
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    compile::{self, Job, Report, Severity},
    fsutil, highlight,
    preview::{Preview, page_in},
    words,
};

mod build;
#[cfg(test)]
mod tests;
mod view;

#[cfg(test)]
use view::{counts_text, wrap_rows};

/// The height of the compile pane, with its border.
const PANE_HEIGHT: u16 = 6;

/// The time without a key after which the editor saves and compiles.
const DEBOUNCE: Duration = Duration::from_millis(300);

/// How often the editor looks at the modification time of the file while the buffer has no edits.
const WATCH_EVERY: Duration = Duration::from_secs(1);
const CONFLICT: &str = "The file changed on disk. Ctrl-S overwrites it with this text.";

/// The modification time of the file, or `None` if the file cannot be read.
/// How many indented lines the detection reads.
const INDENT_LINES: usize = 200;

/// The indent that Tab makes: a tab character, or this many spaces.
#[derive(Debug, PartialEq)]
enum Indent {
    Tab,
    Spaces(u8),
}

/// Finds the indent of a file: a tab if most of the first indented lines start with a tab. Else the most
/// common step between the indents of two lines in a row (2, 3, 4, or 8 spaces). A file without a step,
/// for example one without indent, gets 2 spaces, the usual Typst style.
fn detect_indent(text: &str) -> Indent {
    let mut tabs = 0;
    let mut widths = Vec::new();
    for line in text
        .lines()
        .filter(|line| line.starts_with([' ', '\t']))
        .take(INDENT_LINES)
    {
        if line.starts_with('\t') {
            tabs += 1;
        } else {
            widths.push(line.chars().take_while(|letter| *letter == ' ').count());
        }
    }
    if tabs > widths.len() {
        return Indent::Tab;
    }
    let mut count = [0usize; 9];
    let mut before = 0;
    for width in widths {
        let step = width.abs_diff(before);
        if matches!(step, 2 | 3 | 4 | 8) {
            count[step] += 1;
        }
        before = width;
    }
    // On a tie the smaller step wins, because the scan goes up and `>` keeps the first.
    let mut best = (2, 0);
    for step in [2, 3, 4, 8] {
        if count[step] > best.1 {
            best = (step, count[step]);
        }
    }
    Indent::Spaces(best.0 as u8)
}

/// A text area with the text and the settings of the editor.
fn new_textarea(text: &str) -> TextArea<'static> {
    let mut textarea = TextArea::new(text.lines().map(String::from).collect());
    textarea.set_cursor_line_style(Style::default());
    // A long line wraps on screen, at a word if possible. The file keeps the line as one line.
    textarea.set_wrap_mode(WrapMode::WordOrGlyph);
    // Typst code uses 2 spaces for each level, if the file shows no other indent. Tab follows the file.
    match detect_indent(text) {
        // A tab character shows 2 cells wide, as before.
        Indent::Tab => {
            textarea.set_tab_length(2);
            textarea.set_hard_tab_indent(true);
        }
        Indent::Spaces(width) => textarea.set_tab_length(width),
    }
    // Line numbers help with the error lines of Typst. A dim style keeps them from competing with the text.
    textarea.set_line_number_style(Style::new().add_modifier(Modifier::DIM));
    // The matches of a search stand out. Without colors, they are underlined.
    textarea.set_search_style(if colors_wanted() {
        Style::new().bg(Color::Blue)
    } else {
        Style::new().add_modifier(Modifier::UNDERLINED)
    });
    textarea
}

/// The keys that work in the full preview as in the editor: save, compile, export, open the PDF, go to the
/// error, quit, and the live compile switch. They do not change the text. `Ctrl-G` also shows the editor.
fn full_passthrough(key: KeyEvent) -> bool {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    (ctrl && matches!(key.code, KeyCode::Char('s' | 'b' | 'e' | 'o' | 'g' | 'q')))
        || matches!(key.code, KeyCode::F(5 | 8))
}

/// The regular expression for the text of a search. In regex mode it is the text. Else the text is plain: the
/// special characters are escaped, and a text with no capital letter ignores the case (smart case).
fn search_regex_for(typed: &str, regex: bool) -> String {
    if regex {
        return typed.to_string();
    }
    let mut pattern = String::new();
    if !typed.chars().any(char::is_uppercase) {
        pattern.push_str("(?i)");
    }
    for letter in typed.chars() {
        if "\\.+*?()|[]{}^$#&-~".contains(letter) {
            pattern.push('\\');
        }
        pattern.push(letter);
    }
    pattern
}

fn disk_time(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|meta| meta.modified()).ok()
}

/// True when the user edited the text and then typed nothing for `DEBOUNCE`.
fn debounce_done(last_edit: Option<Instant>, now: Instant) -> bool {
    last_edit.is_some_and(|edit| now.saturating_duration_since(edit) >= DEBOUNCE)
}

/// False when the variable `NO_COLOR` is set and not empty. See https://no-color.org/.
fn colors_wanted() -> bool {
    colors_wanted_for(std::env::var_os("NO_COLOR"))
}

fn colors_wanted_for(no_color: Option<std::ffi::OsString>) -> bool {
    no_color.is_none_or(|value| value.is_empty())
}

impl Drop for Editor {
    fn drop(&mut self) {
        // The page folder of a running compile goes with the editor. The preview deletes its own folder.
        self.stop_compile();
    }
}

/// What the editor shows and what takes the keys. The three modes exclude each other, so a key, a paste,
/// and a draw each use one `match` and cannot disagree about the mode.
enum Mode {
    /// The text area and the preview side by side. The keys edit the text.
    Edit,
    /// The prompt `Search:` in the status line. It takes every key and every paste. See `search_key`.
    Search(Box<TextArea<'static>>),
    /// The prompt `Line:` in the status line. It takes every key and every paste. See `line_key`.
    Line(Box<TextArea<'static>>),
    /// The preview fills the screen and the text area is hidden. Typing does nothing. See `full_key`.
    Full,
}

/// Which error `go_to_error` picks.
#[derive(Clone, Copy)]
enum ErrorPick {
    First,
    Next,
    Previous,
}

pub enum Action {
    Stay,
    /// Close the editor and go back to the file list.
    Close,
    /// Close the editor and quit the program.
    Quit,
    /// Show the help window.
    Help,
    /// Open another file of the project at `line` and `column`, counted from 1. The editor has saved its text.
    Goto {
        file: PathBuf,
        line: usize,
        column: usize,
    },
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
    /// Text that the user copied or cut. The main loop sends it to the system clipboard. See `take_clipboard`.
    clipboard: Option<String>,
    /// True if the file used CR LF line ends when it was read. The save then writes CR LF again.
    crlf: bool,
    /// What takes the keys: the text, the prompt `Search:`, or the full preview. Only one at a time.
    mode: Mode,
    /// The text of the last search, for the next prompt.
    last_search: String,
    /// True when the text of the search is a regular expression. False: it is plain text. `Alt-R` switches.
    search_regex: bool,
    /// True when the user turned the live compile off with F5. The autosave still runs.
    paused: bool,
    /// False when the user turned colors off with `NO_COLOR`. The preview image keeps its colors.
    colors: bool,
    /// The approximate word count of the text. See `words::count`.
    words: usize,
    /// The time of the last look at the file on disk. See `watch_file`.
    last_watch: Option<Instant>,
    /// The files that the last compile read, with their modification times. See `watch_deps`.
    deps: Vec<(PathBuf, Option<SystemTime>)>,
    /// The time of the last look at `deps`.
    last_deps_watch: Option<Instant>,
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
    pub fn open(
        path: PathBuf,
        root: PathBuf,
        main: Option<PathBuf>,
        picker: Picker,
    ) -> io::Result<Self> {
        let text = fsutil::read_text(&path)?;
        let textarea = new_textarea(&text);
        let mut editor = Self {
            crlf: text.contains("\r\n"),
            disk_time: disk_time(&path),
            conflict: false,
            path,
            root,
            main,
            textarea,
            dirty: false,
            last_edit: None,
            last_watch: None,
            deps: Vec::new(),
            last_deps_watch: None,
            clipboard: None,
            words: 0,
            colors: colors_wanted(),
            paused: false,
            mode: Mode::Edit,
            last_search: String::new(),
            search_regex: false,
            close_armed: false,
            message: String::new(),
            pages_root: compile::out_dir(),
            job: None,
            export: None,
            exported: None,
            report: None,
            recover: None,
            preview: Preview::new(picker),
        };
        editor.count_words();
        Ok(editor)
    }

    /// Counts the words of the text again. It runs after each change of the text.
    fn count_words(&mut self) {
        self.words = words::count(&self.textarea.lines().join("\n"));
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
        let end = if self.crlf { "\r\n" } else { "\n" };
        let mut text = self.textarea.lines().join(end);
        if !text.is_empty() {
            text.push_str(end);
        }
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

    /// Opens the last exported PDF with `program`. The program runs in the background with no input and
    /// no output, so it cannot draw on the screen of the editor.
    fn open_pdf(&mut self, program: &str) {
        let Some(pdf) = &self.exported else {
            self.message = "No PDF yet. Press Ctrl-E to export one.".into();
            return;
        };
        let spawned = std::process::Command::new(program)
            .arg(pdf)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        self.message = match spawned {
            Ok(mut child) => {
                // The thread reaps the process, so it does not stay as a zombie.
                std::thread::spawn(move || child.wait());
                format!("Opening {}", pdf.display())
            }
            Err(err) => format!("Cannot start {program}: {err}"),
        };
    }

    /// The text that the user copied since the last call, for the system clipboard.
    pub fn take_clipboard(&mut self) -> Option<String> {
        self.clipboard.take()
    }

    /// Moves the cursor to `line` and `column`, counted from 1. A place beyond the end goes to the end.
    pub fn go_to(&mut self, line: usize, column: usize) {
        self.set_cursor_position((line.saturating_sub(1), column.saturating_sub(1)));
    }

    /// Saves nothing and starts a compile of the file on disk now.
    pub fn compile_now(&mut self) {
        self.start_compile();
    }

    /// Shows `text` in the status line until the next key.
    pub fn say(&mut self, text: impl Into<String>) {
        self.message = text.into();
    }

    /// True while a compile runs, for a test.
    #[cfg(test)]
    pub fn compiling(&self) -> bool {
        self.job.is_some()
    }

    /// The page that the preview shows or waits for, counted from 1.
    pub fn page(&self) -> usize {
        self.preview.wanted_page()
    }

    /// Shows the page `page`. The compile starts at once. If the document is shorter, the preview
    /// ends on the last page (see `poll_compile`).
    pub fn show_page(&mut self, page: usize) {
        self.preview.want(page);
        self.start_compile();
    }

    /// Inserts pasted text at the cursor in one step, so one undo takes it back. A tab character stays a
    /// tab character. Terminals send a line break as CR LF or as CR: both become one line break.
    pub fn paste(&mut self, text: &str) {
        self.close_armed = false;
        self.message.clear();
        match self.mode {
            Mode::Full => return, // typing is off in the full preview
            Mode::Line(_) => {
                for letter in text.chars().filter(|letter| !matches!(letter, '\r' | '\n')) {
                    self.line_key(KeyEvent::from(KeyCode::Char(letter)));
                }
                return;
            }
            Mode::Search(_) => {
                // The prompt takes the text of a paste, as one line.
                for letter in text.chars().filter(|letter| !matches!(letter, '\r' | '\n')) {
                    self.search_key(KeyEvent::from(KeyCode::Char(letter)));
                }
                return;
            }
            Mode::Edit => {}
        }
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        if self.textarea.insert_str(text) {
            self.dirty = true;
            self.last_edit = Some(Instant::now());
            self.count_words();
        }
    }

    /// Notes that the text changed: the buffer is not on disk, the autosave waits for a pause, and the word
    /// count is made again.
    fn mark_edit(&mut self) {
        self.dirty = true;
        self.last_edit = Some(Instant::now());
        self.count_words();
    }

    /// Breaks the line at the cursor. The new line starts with the indent of the line, but not more than the
    /// part of the indent that is before the cursor. After an opening bracket, it has one level more.
    /// One undo step takes back the break and the indent.
    fn break_line_with_indent(&mut self) {
        let (row, column) = self.cursor_position();
        let line = &self.textarea.lines()[row];
        let before: String = line.chars().take(column).collect();
        let indent_len = line
            .chars()
            .take_while(|letter| matches!(letter, ' ' | '\t'))
            .count()
            .min(column);
        let mut indent: String = line.chars().take(indent_len).collect();
        if before.trim_end().ends_with(['{', '(', '[']) {
            // A level is a tab in a line that uses tabs, and a tab stop of spaces if not.
            if indent.starts_with('\t') || (indent.is_empty() && self.textarea.hard_tab_indent()) {
                indent.push('\t');
            } else {
                indent.push_str(&" ".repeat(usize::from(self.textarea.tab_length())));
            }
        }
        self.textarea.insert_str(format!("\n{indent}"));
    }

    /// Removes one indent level from the start of the line of the cursor: a tab character, or up to as many
    /// spaces as a tab stop. The cursor stays on the same letter. Returns false if the line has no indent.
    fn dedent_line(&mut self) -> bool {
        let (row, column) = self.cursor_position();
        let line = &self.textarea.lines()[row];
        let remove = if line.starts_with('\t') {
            1
        } else {
            line.chars()
                .take(usize::from(self.textarea.tab_length()))
                .take_while(|letter| *letter == ' ')
                .count()
        };
        if remove == 0 {
            return false;
        }
        self.textarea.cancel_selection();
        self.set_cursor_position((row, 0));
        self.textarea.delete_str(remove);
        self.set_cursor_position((row, column.saturating_sub(remove)));
        true
    }

    /// How many times the text area must get `key` to move or delete one visible character (a grapheme
    /// cluster): the length of the cluster in code points. It is 1 for any other key, at the end of a line,
    /// and at the start of a line, where the text area joins or crosses lines.
    fn grapheme_steps(&self, key: KeyEvent) -> usize {
        let plain = !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        let forward = match key.code {
            KeyCode::Right | KeyCode::Delete if plain => true,
            KeyCode::Left | KeyCode::Backspace if plain => false,
            _ => return 1,
        };
        // A key with a selection deletes the selection or ends it. It is one step.
        if self.textarea.selection_range().is_some() {
            return 1;
        }
        let (row, column) = self.cursor_position();
        let Some(line) = self.textarea.lines().get(row) else {
            return 1;
        };
        // The character index at which each cluster starts, and the end of the line.
        let mut starts: Vec<usize> = Vec::new();
        let mut at = 0;
        for cluster in line.graphemes(true) {
            starts.push(at);
            at += cluster.chars().count();
        }
        starts.push(at);
        let steps = if forward {
            starts
                .iter()
                .find(|start| **start > column)
                .map(|end| end - column)
        } else {
            starts
                .iter()
                .rev()
                .find(|start| **start < column)
                .map(|start| column - start)
        };
        steps.unwrap_or(1).max(1)
    }

    /// Handles `Alt-Down`, `Alt-Up`, `Alt-Home`, and `Alt-End`: they change the page. Returns true if the key
    /// was one of them.
    ///
    /// Alt with an arrow key arrives as one escape sequence. Alt with a letter arrives as Esc and the
    /// letter, so a fast Esc and n looked like Alt-n.
    /// A page turn renders the new page with a new compile. It does not save: the file on disk is what the
    /// compile reads, and a refused save (a file that another program changed) must not stop the turn.
    /// The old page stays on screen until the new page is ready. The user's choice ends a recovery.
    fn page_key(&mut self, key: KeyEvent) -> bool {
        // Alt-Shift with an arrow key is for the line keys. Only plain Alt turns pages.
        if !key.modifiers.contains(KeyModifiers::ALT) || key.modifiers.contains(KeyModifiers::SHIFT)
        {
            return false;
        }
        let changed = match key.code {
            KeyCode::Down | KeyCode::Up => self.preview.turn(key.code == KeyCode::Down),
            KeyCode::Home | KeyCode::End => self.preview.turn_to(key.code == KeyCode::End),
            _ => return false,
        };
        if changed {
            self.recover = None;
            self.start_compile();
        }
        true
    }

    /// Handles a key while the preview fills the screen. Typing does nothing here, so no text changes by
    /// accident. `F11` or `Esc` brings the editor back, and the page keys work as always. `+` and `-` zoom,
    /// `0` shows the whole page, and the arrow keys, `Home`, and `End` move the view of a zoomed page.
    fn full_key(&mut self, key: KeyEvent) -> Action {
        let plain = !key.modifiers.contains(KeyModifiers::ALT);
        let zoomed = match key.code {
            KeyCode::F(11) | KeyCode::Esc => {
                self.mode = Mode::Edit;
                // The split view cannot move the view, so it shows the whole page. A zoom would also
                // make every autosave render a big page.
                if self.preview.zoom_fit() {
                    self.start_compile();
                }
                return Action::Stay;
            }
            KeyCode::F(1) => return Action::Help,
            KeyCode::Char('+' | '=') => self.preview.zoom_step(true),
            KeyCode::Char('-') => self.preview.zoom_step(false),
            KeyCode::Char('0') => self.preview.zoom_fit(),
            KeyCode::Left if plain => {
                self.preview.pan_by(-1, 0);
                false
            }
            KeyCode::Right if plain => {
                self.preview.pan_by(1, 0);
                false
            }
            KeyCode::Up if plain => {
                self.preview.pan_by(0, -1);
                false
            }
            KeyCode::Down if plain => {
                self.preview.pan_by(0, 1);
                false
            }
            KeyCode::Home if plain => {
                self.preview.pan_to_edge(false);
                false
            }
            KeyCode::End if plain => {
                self.preview.pan_to_edge(true);
                false
            }
            _ => {
                self.page_key(key);
                false
            }
        };
        if zoomed {
            // The page needs another resolution, so a compile renders it again.
            self.start_compile();
        }
        Action::Stay
    }

    /// Handles a key while the prompt `Line:` is open (`Alt-G`). The text is a line number, or a line number,
    /// a colon, and a column, both counted from 1. `Enter` goes there, and `Esc` closes the prompt. A number
    /// beyond the end goes to the end. Only digits and the colon are typed.
    fn line_key(&mut self, key: KeyEvent) {
        let Mode::Line(prompt) = &mut self.mode else {
            return;
        };
        match key.code {
            KeyCode::Esc => self.mode = Mode::Edit,
            KeyCode::Enter => {
                let typed = prompt.lines().join("");
                if typed.is_empty() {
                    self.mode = Mode::Edit;
                    return;
                }
                let (line, column) = typed.split_once(':').unwrap_or((typed.as_str(), "1"));
                let (Ok(line), Ok(column)) = (line.parse::<usize>(), column.parse::<usize>())
                else {
                    self.message = "Type a line number, for example 42 or 42:7.".into();
                    return;
                };
                let last = self.textarea.lines().len().max(1);
                self.textarea.cancel_selection();
                self.go_to(line.max(1), column.max(1));
                self.message = if line > last {
                    format!("Line {line} is beyond the end. This is the last line, {last}.")
                } else {
                    format!("Line {line}")
                };
                self.mode = Mode::Edit;
            }
            KeyCode::Char(letter)
                if (letter.is_ascii_digit() || letter == ':')
                    && !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                prompt.input(key);
            }
            KeyCode::Backspace | KeyCode::Left | KeyCode::Right | KeyCode::Delete => {
                prompt.input(key);
            }
            _ => {}
        }
    }

    /// Handles a key while the prompt `Search:` is open. The text of the prompt is a regular expression.
    /// The matches show while you type. `Enter` or `Ctrl-F` moves the cursor to the next match, and the
    /// search wraps at the end of the file. `Esc` closes the prompt and keeps the cursor.
    fn search_key(&mut self, key: KeyEvent) {
        let Mode::Search(prompt) = &mut self.mode else {
            return;
        };
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let next = key.code == KeyCode::Enter || (ctrl && key.code == KeyCode::Char('f'));
        if key.code == KeyCode::Esc {
            self.mode = Mode::Edit;
            let _ = self.textarea.set_search_pattern("");
            return;
        }
        let switch = key.modifiers.contains(KeyModifiers::ALT) && key.code == KeyCode::Char('r');
        if switch {
            self.search_regex = !self.search_regex;
        } else if !next {
            prompt.input(key);
        }
        let pattern = prompt.lines().join("");
        self.last_search = pattern.clone();
        match self
            .textarea
            .set_search_pattern(search_regex_for(&pattern, self.search_regex))
        {
            Err(err) => {
                // An invalid pattern keeps the old matches. The first line of the error says why.
                let reason = err.to_string();
                let reason = reason.lines().last().unwrap_or("").trim();
                self.message = format!("Invalid pattern: {reason}");
            }
            Ok(()) if next && !pattern.is_empty() => {
                if !self.textarea.search_forward(false) {
                    self.message = format!("No match for {pattern}");
                }
            }
            Ok(()) => {}
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        let armed = std::mem::take(&mut self.close_armed);
        self.message.clear();
        match self.mode {
            Mode::Search(_) => {
                self.search_key(key);
                return Action::Stay;
            }
            Mode::Line(_) => {
                self.line_key(key);
                return Action::Stay;
            }
            Mode::Full if !full_passthrough(key) => return self.full_key(key),
            // The error is in the text, so the text must show.
            Mode::Full if matches!(key.code, KeyCode::Char('g') | KeyCode::F(8)) => {
                self.mode = Mode::Edit;
            }
            Mode::Full => {}
            Mode::Edit if key.code == KeyCode::F(11) => {
                self.mode = Mode::Full;
                return Action::Stay;
            }
            Mode::Edit => {}
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && key.code == KeyCode::Char('f') {
            let mut prompt = TextArea::new(vec![self.last_search.clone()]);
            prompt.set_cursor_line_style(Style::default());
            prompt.move_cursor(CursorMove::End);
            self.mode = Mode::Search(Box::new(prompt));
        } else if ctrl && key.code == KeyCode::Char('s') {
            if !self.dirty {
                self.message = "No changes to save".into();
            }
            self.save(true);
        } else if key.code == KeyCode::F(1) {
            return Action::Help;
        } else if key.code == KeyCode::F(5) {
            self.paused = !self.paused;
            self.message = if self.paused {
                "Live compile off. Ctrl-B compiles. F5 turns it on.".into()
            } else {
                "Live compile on".into()
            };
            if !self.paused {
                self.save_and_compile();
            }
        } else if ctrl && key.code == KeyCode::Char('b') {
            self.save_and_compile();
        } else if key.code == KeyCode::F(8) {
            let pick = if key.modifiers.contains(KeyModifiers::SHIFT) {
                ErrorPick::Previous
            } else {
                ErrorPick::Next
            };
            return self.go_to_error(pick);
        } else if ctrl && key.code == KeyCode::Char('g') {
            return self.go_to_error(ErrorPick::First);
        } else if ctrl && key.code == KeyCode::Char('o') {
            self.open_pdf("xdg-open");
        } else if ctrl && key.code == KeyCode::Char('e') {
            if self.save(false) {
                // The new export replaces the old export. Dropping the old export kills its process.
                let target = self.compile_target().to_path_buf();
                self.export = Some(Job::start_pdf(
                    &target,
                    &self.root,
                    target.with_extension("pdf"),
                ));
                self.exported = None;
                self.message = "Exporting the PDF...".into();
            }
        } else if self.page_key(key) {
        } else if key.code == KeyCode::Esc || (ctrl && key.code == KeyCode::Char('q')) {
            let quit = key.code != KeyCode::Esc;
            if armed || self.save(false) {
                return if quit { Action::Quit } else { Action::Close };
            }
            // `save` put the reason in the message.
            self.close_armed = true;
            self.message.push_str(if quit {
                " Ctrl-Q again quits without a save."
            } else {
                " Esc again closes without a save."
            });
        } else if key.code == KeyCode::Enter
            && !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            && self.textarea.selection_range().is_none()
        {
            self.break_line_with_indent();
            self.mark_edit();
        } else if ctrl && key.code == KeyCode::Char('z') {
            // Ctrl-Z undoes, as in most editors. The text area undoes with Ctrl-U.
            if self
                .textarea
                .input(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL))
            {
                self.mark_edit();
            } else {
                self.message = "Nothing to undo".into();
            }
        } else if key.modifiers.contains(KeyModifiers::ALT) && key.code == KeyCode::Char('g') {
            let mut prompt = TextArea::default();
            prompt.set_cursor_line_style(Style::default());
            self.mode = Mode::Line(Box::new(prompt));
        } else if key.modifiers.contains(KeyModifiers::ALT) && key.code == KeyCode::Char('a') {
            self.textarea.select_all();
        } else if ctrl && matches!(key.code, KeyCode::Home | KeyCode::End) {
            // Ctrl-Home and Ctrl-End go to the start and the end of the text. With Shift, they select.
            let end = key.code == KeyCode::End;
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                if self.textarea.selection_range().is_none() {
                    self.textarea.start_selection();
                }
            } else {
                self.textarea.cancel_selection();
            }
            let last = self.textarea.lines().len().saturating_sub(1);
            let place = if end {
                (
                    last,
                    self.textarea
                        .lines()
                        .get(last)
                        .map_or(0, |line| line.chars().count()),
                )
            } else {
                (0, 0)
            };
            self.set_cursor_position(place);
        } else if key.code == KeyCode::BackTab {
            // Shift-Tab. The text area would insert spaces, as Tab does.
            if self.dedent_line() {
                self.mark_edit();
            }
        } else {
            // Ctrl-C and Ctrl-X also go to the system clipboard. Without a selection they copy nothing.
            let copying = ctrl
                && matches!(key.code, KeyCode::Char('c' | 'x'))
                && self.textarea.selection_range().is_some();
            // A visible character can have many code points. The text area moves and deletes one code point
            // for each key, so the key is repeated for the rest of the character.
            for _ in 0..self.grapheme_steps(key) {
                if self.textarea.input(key) {
                    self.mark_edit();
                }
            }
            if copying {
                self.clipboard = Some(self.textarea.yank_text());
            }
        }
        Action::Stay
    }

    /// The file that the editor holds.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The cursor as `(row, column)`, both counted from 0, the column in characters.
    pub fn cursor_position(&self) -> (usize, usize) {
        let cursor = self.textarea.cursor();
        (cursor.0, cursor.1)
    }

    /// Moves the cursor to `(row, column)`, counted from 0. A place beyond the end of the text goes to the
    /// last line, and a column beyond the end of a line goes to the end of that line.
    pub fn set_cursor_position(&mut self, (row, column): (usize, usize)) {
        let to_u16 = |number: usize| u16::try_from(number).unwrap_or(u16::MAX);
        self.textarea
            .move_cursor(CursorMove::Jump(to_u16(row), to_u16(column)));
    }

    /// Moves the cursor to the first error of the last report (`Ctrl-G`), or to the next or the previous one
    /// (`F8` and `Shift-F8`, with a wrap at the ends). An error in a file of the project opens that file, after
    /// a save. An error in another place, such as a package, only gets named: the cursor stays. The line and
    /// the column of the report are those of the text at the time of the compile. After more edits, a new
    /// compile makes them exact.
    fn go_to_error(&mut self, pick: ErrorPick) -> Action {
        let errors: Vec<compile::Diagnostic> = self
            .report
            .iter()
            .flat_map(|report| report.diagnostics.iter())
            .filter(|diagnostic| {
                diagnostic.severity == Severity::Error && diagnostic.file.is_some()
            })
            .cloned()
            .collect();
        if errors.is_empty() {
            self.message = "No error to go to.".into();
            return Action::Stay;
        }
        let open_file = self
            .path
            .strip_prefix(&self.root)
            .unwrap_or(&self.path)
            .to_path_buf();
        // Typst counts from 1. The text area counts from 0, in characters.
        let (row, column) = self.cursor_position();
        let here = (row + 1, column + 1);
        let in_open_file =
            |error: &compile::Diagnostic| error.file.as_deref() == Some(open_file.as_path());
        let at = |error: &compile::Diagnostic| (error.line, error.column);
        let current = errors
            .iter()
            .position(|error| in_open_file(error) && at(error) == here);
        let count = errors.len();
        let index = match pick {
            ErrorPick::First => 0,
            ErrorPick::Next => match current {
                Some(index) => (index + 1) % count,
                None => errors
                    .iter()
                    .enumerate()
                    .filter(|(_, error)| in_open_file(error) && at(error) > here)
                    .min_by_key(|(_, error)| at(error))
                    .map_or(0, |(index, _)| index),
            },
            ErrorPick::Previous => match current {
                Some(index) => (index + count - 1) % count,
                None => errors
                    .iter()
                    .enumerate()
                    .filter(|(_, error)| in_open_file(error) && at(error) < here)
                    .max_by_key(|(_, error)| at(error))
                    .map_or(count - 1, |(index, _)| index),
            },
        };
        let error = &errors[index];
        let (line, column) = (error.line, error.column);
        if !in_open_file(error) {
            let file = error.file.clone().unwrap_or(open_file);
            // A file of the project opens in the editor. A file outside the project, such as a package,
            // only gets named.
            let plain = file
                .components()
                .all(|part| matches!(part, std::path::Component::Normal(_)));
            if plain && self.root.join(&file).is_file() {
                // The text goes to disk first. A refused save (see `save`) keeps the editor here.
                if !self.save(false) {
                    return Action::Stay;
                }
                return Action::Goto { file, line, column };
            }
            self.message = format!("The error is in {}.", file.display());
            return Action::Stay;
        }
        let to_index = |number: usize| u16::try_from(number.saturating_sub(1)).unwrap_or(u16::MAX);
        self.textarea
            .move_cursor(CursorMove::Jump(to_index(line), to_index(column)));
        self.message = match pick {
            ErrorPick::First => format!("Error at {line}:{column}: {}", error.message),
            _ => format!(
                "Error {} of {count} at {line}:{column}: {}",
                index + 1,
                error.message
            ),
        };
        Action::Stay
    }

    /// Runs the autosave when the user paused, and takes the report of a finished compile.
    /// The main loop calls this when no key arrives. Returns true when the screen must redraw.
    pub fn tick(&mut self, now: Instant) -> bool {
        let mut changed = false;
        if debounce_done(self.last_edit, now) {
            // `save` clears `last_edit`. If the save fails, nothing retries until the next edit.
            self.last_edit = None;
            if self.paused {
                self.save(false);
            } else {
                self.save_and_compile();
            }
            changed = true;
        }
        changed |= self.watch_file(now);
        changed |= self.watch_deps(now);
        changed |= self.poll_compile();
        changed |= self.poll_export();
        changed
    }

    /// About once a second, looks at the files that the last compile read: included files, images, and
    /// bibliographies. If another program changed one, starts a compile and names the file. The open file
    /// is left to `watch_file`. Nothing happens while the user types, while a compile runs, or while the
    /// live compile is off.
    // ponytail: the times are read when the compile ends, so a change during a compile waits for the next one.
    fn watch_deps(&mut self, now: Instant) -> bool {
        if self.dirty
            || self.paused
            || self.job.is_some()
            || self.deps.is_empty()
            || self
                .last_deps_watch
                .is_some_and(|last| now.saturating_duration_since(last) < WATCH_EVERY)
        {
            return false;
        }
        self.last_deps_watch = Some(now);
        let Some((path, _)) = self
            .deps
            .iter()
            .find(|(path, time)| *path != self.path && disk_time(path) != *time)
        else {
            return false;
        };
        let name = path
            .strip_prefix(&self.root)
            .unwrap_or(path)
            .display()
            .to_string();
        self.message = format!("{name} changed");
        // The compile ends with a new list and new times.
        self.deps.clear();
        self.start_compile();
        true
    }

    /// While the buffer has no edits, looks about once a second at the file on disk. If another program
    /// changed it, loads the new text and starts a compile. The cursor keeps its line, or goes to the
    /// last line if the file got shorter. With edits, the buffer stays: `save` shows the conflict.
    /// Returns true when the screen must redraw.
    fn watch_file(&mut self, now: Instant) -> bool {
        if self.dirty
            || self
                .last_watch
                .is_some_and(|last| now.saturating_duration_since(last) < WATCH_EVERY)
        {
            return false;
        }
        self.last_watch = Some(now);
        let time = disk_time(&self.path);
        if time == self.disk_time {
            return false;
        }
        self.disk_time = time;
        if time.is_none() {
            self.message = "The file is gone. Ctrl-S writes it again.".into();
            return true;
        }
        match fsutil::read_text(&self.path) {
            Ok(text) => {
                let place = self.cursor_position();
                self.crlf = text.contains("\r\n");
                self.textarea = new_textarea(&text);
                if matches!(self.mode, Mode::Search(_)) {
                    let _ = self
                        .textarea
                        .set_search_pattern(search_regex_for(&self.last_search, self.search_regex));
                }
                self.count_words();
                self.set_cursor_position(place);
                self.message = "Loaded the change from disk".into();
                if !self.paused {
                    self.start_compile();
                }
            }
            Err(err) => self.message = format!("Cannot read the file: {err}"),
        }
        true
    }
}
