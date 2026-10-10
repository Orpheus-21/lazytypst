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
use regex::Regex;
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    compile::{self, Job, Report, Severity},
    fsutil, highlight, history,
    preview::{Preview, page_in},
    switcher::{Outcome, Switcher},
    words,
};

mod build;
mod follow;
mod image;
mod mouse;
mod spelling;
#[cfg(test)]
mod tests;
mod view;

#[cfg(test)]
use view::{counts_text, wrap_rows};

/// The height of the compile pane, with its border.
const PANE_HEIGHT: u16 = 6;

/// The time without a key after which the editor saves and compiles.
const DEBOUNCE: Duration = Duration::from_millis(300);
/// A compile that takes less than this is fast: it starts right after the save, as the autosave does.
const SLOW_COMPILE: Duration = Duration::from_secs(1);
/// The longest wait for a compile after a save. A slow document waits twice its last compile time, up to this.
const MAX_COMPILE_WAIT: Duration = Duration::from_secs(3);

/// How often the editor looks at the modification time of the file while the buffer has no edits.
const WATCH_EVERY: Duration = Duration::from_secs(1);
const CONFLICT: &str = "The file changed on disk. Ctrl-S keeps my text. Alt-L loads the file.";

/// The modification time of the file, or `None` if the file cannot be read.
/// The text of the string that holds the char column `column` of `line`, or else the first string of the
/// line. A string starts and ends with `"`. A backslash escapes the next char. `None` if there is no string.
fn string_near(line: &str, column: usize) -> Option<String> {
    let chars: Vec<char> = line.chars().collect();
    let mut strings: Vec<(usize, usize)> = Vec::new();
    let mut start = None;
    let mut index = 0;
    while index < chars.len() {
        match (chars[index], start) {
            ('\\', Some(_)) => index += 1,
            ('"', None) => start = Some(index),
            ('"', Some(begin)) => {
                strings.push((begin, index));
                start = None;
            }
            _ => {}
        }
        index += 1;
    }
    let (begin, end) = strings
        .iter()
        .copied()
        .find(|(begin, end)| *begin <= column && column <= *end)
        .or_else(|| strings.first().copied())?;
    Some(chars[begin + 1..end].iter().collect())
}

/// The file that `name` means in a document at `open`, as a path relative to `root`. Typst reads a path that
/// starts with `/` from the root, and any other path from the folder of the file. The file must exist, be
/// a `.typ` file, and stay inside the project. The error is the text for the status line.
fn resolve_file(root: &Path, open: &Path, name: &str) -> Result<PathBuf, String> {
    use std::path::Component;
    if name.starts_with('@') {
        return Err(format!("{name} is a package, not a file of the project."));
    }
    let folder = open
        .strip_prefix(root)
        .ok()
        .and_then(Path::parent)
        .unwrap_or(Path::new(""));
    let mut parts: Vec<std::ffi::OsString> = if name.starts_with('/') {
        Vec::new()
    } else {
        folder.iter().map(Into::into).collect()
    };
    for part in Path::new(name).components() {
        match part {
            Component::Normal(part) => parts.push(part.to_os_string()),
            Component::ParentDir if parts.pop().is_none() => {
                return Err(format!("{name} is outside the project."));
            }
            _ => {}
        }
    }
    let relative: PathBuf = parts.into_iter().collect();
    let full = root.join(&relative);
    if !full.is_file() {
        return Err(format!("{} is not a file.", relative.display()));
    }
    // A link can lead out of the project. Typst does not read it, so the editor does not open it.
    let inside = fs::canonicalize(&full)
        .ok()
        .zip(fs::canonicalize(root).ok())
        .is_some_and(|(full, root)| full.starts_with(root));
    if !inside {
        return Err(format!("{} is outside the project.", relative.display()));
    }
    if relative.extension().is_none_or(|ending| ending != "typ") {
        return Err(format!(
            "{} is not a .typ file. The editor opens .typ files only.",
            relative.display()
        ));
    }
    Ok(relative)
}

/// How the editor and the preview share the screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Arrangement {
    /// The editor at the left, the preview at the right.
    Side,
    /// The editor above, the preview below. It suits a narrow window.
    Stacked,
    /// The editor fills the window. The preview is hidden, and the compile pane stays.
    EditorOnly,
}

/// A window under this many columns starts stacked, because side by side leaves each part too little room.
const STACK_BELOW: u16 = 100;

impl Arrangement {
    pub fn name(self) -> &'static str {
        match self {
            Self::Side => "side",
            Self::Stacked => "stacked",
            Self::EditorOnly => "editor",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        [Self::Side, Self::Stacked, Self::EditorOnly]
            .into_iter()
            .find(|arrangement| arrangement.name() == name)
    }

    fn next(self) -> Self {
        match self {
            Self::Side => Self::Stacked,
            Self::Stacked => Self::EditorOnly,
            Self::EditorOnly => Self::Side,
        }
    }

    fn describe(self) -> &'static str {
        match self {
            Self::Side => "Layout: side by side",
            Self::Stacked => "Layout: stacked",
            Self::EditorOnly => "Layout: editor only",
        }
    }
}

/// One screen row of the text, from the last draw: where its characters are. A click finds its place here.
pub(super) struct HitRow {
    pub y: u16,
    pub line: usize,
    /// The first character of the row.
    pub start: usize,
    /// For each character: its first column, its width, and its offset in the line.
    pub cells: Vec<(u16, u16, usize)>,
}

/// A compile of `typst watch` that the editor waits for.
pub(super) struct WatchWait {
    pub since: Instant,
    /// True after the watch said that a compile started.
    pub compiling: bool,
}

/// The time after a save in which `typst watch` must start a compile. If it does not, the editor starts it again.
const WATCH_PATIENCE: Duration = Duration::from_secs(4);
/// The time that `typst watch` may need for one compile, as the time of a `typst compile`.
const WATCH_TIMEOUT: Duration = Duration::from_secs(60);

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

/// The wait before a compile, after a compile that took `last`. See `Editor::compile_wait`.
fn compile_wait_after(last: Duration) -> Duration {
    if last < SLOW_COMPILE {
        DEBOUNCE
    } else {
        (last * 2).clamp(DEBOUNCE, MAX_COMPILE_WAIT)
    }
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
    /// The prompt `Replace with:` in the status line. It takes every key and every paste. See `replace_key`.
    Replace(Box<TextArea<'static>>),
    /// The list of a misspelled word. See `open_spell`.
    Spell(Box<Switcher<spelling::Choice>>),
    /// The history popup: the saved versions of the file, the newest first. See `open_history`.
    History(Box<Switcher<history::Version>>),
    /// The outline popup. It takes every key and every paste. See `outline_key`. Each entry is the line of a
    /// heading, counted from 0.
    Outline(Box<Switcher<usize>>),
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

#[derive(Debug)]
pub enum Action {
    Stay,
    /// Close the editor and go back to the file list.
    Close,
    /// Close the editor and quit the program.
    Quit,
    /// Show the help window.
    Help,
    /// Show the file switcher. The editor has saved its text.
    Switch,
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
    /// A compile that waits after a save: when it starts, and how long the wait is. See `compile_wait`.
    compile_at: Option<(Instant, Duration)>,
    /// True when the compile uses a long running `typst watch`: the option `LAZYTYPST_WATCH=1`.
    watch_mode: bool,
    /// The running `typst watch`, and the compile that the editor waits for. See `start_watch`.
    watch: Option<compile::Watch>,
    watch_wait: Option<WatchWait>,
    /// The program that reads an image from the system clipboard, or `None`.
    clip_tool: Option<crate::clipboard::Tool>,
    /// The screen rows of the text at the last draw, the area of the text, and the area of the preview.
    hits: Vec<HitRow>,
    text_area: Rect,
    preview_area: Rect,
    /// True while the left button is down after a click in the text: the move of the mouse selects.
    dragging: bool,
    /// True when the program captures the mouse. `F9` switches it. See `mouse`.
    mouse: bool,
    /// The preview that follows the cursor. See `follow.rs`.
    follow: follow::Follow,
    /// The spell check. See `spelling.rs`.
    spelling: spelling::Spelling,
    /// Grows with each change of the text, so that a check can tell if its words are still current.
    text_version: u64,
    /// The folder of the local history, in the state folder. `None`: no history.
    history: Option<PathBuf>,
    /// The layout that the user chose with F10. `None`: the width of the window decides.
    arrangement: Option<Arrangement>,
    /// The width of the window at the last draw, for F10.
    width: u16,
    /// The text before and after the last replace. See `undo`.
    replaced: Option<(Vec<String>, Vec<String>)>,
    /// The text of the last replacement, for the next prompt.
    last_replace: String,
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
            last_replace: String::new(),
            replaced: None,
            arrangement: None,
            history: None,
            watch_mode: std::env::var_os("LAZYTYPST_WATCH").is_some_and(|value| value == "1"),
            watch: None,
            watch_wait: None,
            clip_tool: crate::clipboard::Tool::detect(),
            hits: Vec::new(),
            text_area: Rect::default(),
            preview_area: Rect::default(),
            dragging: false,
            mouse: false,
            follow: follow::Follow::new(),
            spelling: spelling::Spelling::new(),
            text_version: 0,
            compile_at: None,
            width: 0,
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
        let text = self.file_text();
        match fsutil::write_file(&self.path, text.as_bytes()) {
            Ok(()) => {
                self.keep_version(&text, false);
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

    /// The text of the buffer as the file holds it: the line ends of the file, and one at the end.
    fn file_text(&self) -> String {
        let end = if self.crlf { "\r\n" } else { "\n" };
        let mut text = self.textarea.lines().join(end);
        if !text.is_empty() {
            text.push_str(end);
        }
        text
    }

    /// Keeps `text` in the local history. A failure is not an error for the user: the history is a help.
    fn keep_version(&self, text: &str, force: bool) {
        if let Some(history) = &self.history {
            let _ = history::record(history, &self.path, text, SystemTime::now(), force);
        }
    }

    /// Sets the folder of the local history, and keeps the text of the file as it is now: the first
    /// version, so a bad edit in this run can always be undone from the history.
    pub fn set_history(&mut self, folder: Option<PathBuf>) {
        self.history = folder;
        let text = self.file_text();
        self.keep_version(&text, true);
    }

    /// F6: opens the list of the saved versions of the file. Each line says how old the version is, how
    /// large, and how many lines it lost and gained compared with the buffer. `Enter` restores a version.
    fn open_history(&mut self) {
        let Some(folder) = &self.history else {
            self.message = "There is no history: the program has no state folder.".into();
            return;
        };
        let versions = history::list(folder, &self.path);
        if versions.is_empty() {
            self.message =
                "No versions yet. The program keeps one after a save, at most every 30 s.".into();
            return;
        }
        let current = self.file_text();
        let now = SystemTime::now();
        let entries = versions
            .into_iter()
            .map(|version| {
                let age = crate::age_text(now.duration_since(version.time).unwrap_or_default());
                let (lost, gained) = version
                    .read()
                    .map_or((0, 0), |old| history::changed_lines(&current, &old));
                // The numbers say what the version would change: lines that go, and lines that come back.
                let age = if age == "now" {
                    "just now".to_string()
                } else {
                    format!("{age} ago")
                };
                let text = format!(
                    "{age:<10} {:>7} bytes  -{lost} +{gained} lines",
                    version.size
                );
                (version, text)
            })
            .collect();
        self.mode = Mode::History(Box::new(Switcher::new(
            "History",
            "Enter restores  Esc closes",
            entries,
            false,
        )));
    }

    /// The keys of the history list: `Enter` puts the version in the buffer, as one edit.
    fn history_key(&mut self, key: KeyEvent) {
        let Mode::History(popup) = &mut self.mode else {
            return;
        };
        let outcome = if key.code == KeyCode::F(6) {
            Outcome::Close
        } else {
            popup.key(key)
        };
        match outcome {
            Outcome::Stay => {}
            Outcome::Close => self.mode = Mode::Edit,
            Outcome::Pick(version) => {
                self.mode = Mode::Edit;
                match version.read() {
                    Ok(text) => self.restore_text(&text),
                    Err(err) => self.message = format!("Cannot read the version: {err}"),
                }
            }
        }
    }

    /// Puts `text` in the buffer as one edit. One Ctrl-Z takes it back.
    fn restore_text(&mut self, text: &str) {
        let before = self.textarea.lines().to_vec();
        let lines: Vec<&str> = text.lines().collect();
        let place = self.cursor_position();
        self.textarea.cancel_selection();
        self.textarea.select_all();
        self.textarea.insert_str(lines.join("\n"));
        self.note_replace(before);
        self.set_cursor_position(place);
        self.mark_edit();
        self.message = "Restored the version. Ctrl-Z undoes.".into();
    }

    /// Writes the edits that the autosave has not written yet, when the program ends: the terminal closed
    /// (SIGHUP), the system asked it to stop (SIGTERM), or a panic. If the file cannot be written (it changed
    /// on disk, or the write failed), the text goes to the local history. The answer is a sentence for the
    /// user about a text that is not in the file, or `None` if the file has all the text.
    pub fn save_for_exit(&mut self) -> Option<String> {
        if !self.dirty || self.save(false) {
            return None;
        }
        let name = self
            .path
            .strip_prefix(&self.root)
            .unwrap_or(&self.path)
            .display()
            .to_string();
        let reason = self.message.clone();
        let text = self.file_text();
        let kept = self.history.as_ref().and_then(|folder| {
            history::record(folder, &self.path, &text, SystemTime::now(), true).ok()?;
            history::list(folder, &self.path)
                .first()
                .map(|version| version.path().to_path_buf())
        });
        Some(match kept {
            Some(path) => format!(
                "{name} was not written ({reason}). Your text is in {}",
                path.display()
            ),
            None => format!("{name} was not written ({reason}). Your text is lost."),
        })
    }

    /// After a panic: puts the buffer in the local history and says where. It does not write the file, because
    /// a panic can leave the buffer in a bad state, and the file keeps the last good save. `None` if the
    /// buffer has no edits that the file lacks.
    pub fn rescue_after_panic(&mut self) -> Option<String> {
        if !self.dirty {
            return None;
        }
        let text = self.file_text();
        let kept = self.history.as_ref().and_then(|folder| {
            history::record(folder, &self.path, &text, SystemTime::now(), true).ok()?;
            history::list(folder, &self.path)
                .first()
                .map(|version| version.path().to_path_buf())
        });
        Some(match kept {
            Some(path) => format!("Your edits that the file lacks are in {}", path.display()),
            None => "Your edits that the file lacks are lost: there is no state folder.".into(),
        })
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
        self.compile_busy()
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
            Mode::Replace(_) => {
                for letter in text.chars().filter(|letter| !matches!(letter, '\r' | '\n')) {
                    self.replace_key(KeyEvent::from(KeyCode::Char(letter)));
                }
                return;
            }
            Mode::Outline(_) => {
                for letter in text.chars().filter(|letter| !matches!(letter, '\r' | '\n')) {
                    self.outline_key(KeyEvent::from(KeyCode::Char(letter)));
                }
                return;
            }
            Mode::History(_) => {
                for letter in text.chars().filter(|letter| !matches!(letter, '\r' | '\n')) {
                    self.history_key(KeyEvent::from(KeyCode::Char(letter)));
                }
                return;
            }
            Mode::Spell(_) => {
                for letter in text.chars().filter(|letter| !matches!(letter, '\r' | '\n')) {
                    self.spell_key(KeyEvent::from(KeyCode::Char(letter)));
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
        // A compile that waits would show the text before this key. The next save plans a new one.
        self.compile_at = None;
        self.text_version += 1;
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
            KeyCode::F(2) => return self.switch_action(),
            KeyCode::F(4) => {
                self.open_outline();
                return Action::Stay;
            }
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
        if key.modifiers.contains(KeyModifiers::ALT) && key.code == KeyCode::Char('s') {
            self.open_replace();
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

    /// The headings of the text: the line, and the text for the list, indented by level. A line that looks
    /// like a heading inside a comment, math, or raw text is not a heading.
    fn headings(&self) -> Vec<(usize, String)> {
        let lines = self.textarea.lines();
        let parts = highlight::tokenize(lines);
        lines
            .iter()
            .enumerate()
            .filter(|(row, _)| {
                parts[*row]
                    .first()
                    .is_some_and(|part| part.kind == highlight::Kind::Heading && part.start == 0)
            })
            .map(|(row, line)| {
                let text = line.trim();
                let level = text.chars().take_while(|letter| *letter == '=').count();
                let title = text.trim_start_matches('=').trim();
                (
                    row,
                    format!("{}{title}", "  ".repeat(level.saturating_sub(1))),
                )
            })
            .collect()
    }

    /// F4: opens the outline, with the section of the cursor selected. It shows the text, not the full
    /// preview, because the jump needs the text.
    fn open_outline(&mut self) {
        let headings = self.headings();
        if headings.is_empty() {
            self.message =
                "No headings. A heading is a line that starts with = and a space.".into();
            return;
        }
        let here = self.cursor_position().0;
        let current = headings.iter().rposition(|(row, _)| *row <= here);
        let mut popup = Switcher::new(
            "Outline",
            "Type to filter  Enter jumps  F4 or Esc closes",
            headings,
            false,
        );
        if let Some(index) = current {
            popup.select(index);
        }
        self.mode = Mode::Outline(Box::new(popup));
    }

    /// The keys of the outline: `Enter` goes to the heading, `F4` and `Esc` close it. The rest is the
    /// popup's.
    fn outline_key(&mut self, key: KeyEvent) {
        let Mode::Outline(popup) = &mut self.mode else {
            return;
        };
        let outcome = if key.code == KeyCode::F(4) {
            Outcome::Close
        } else {
            popup.key(key)
        };
        match outcome {
            Outcome::Stay => {}
            Outcome::Close => self.mode = Mode::Edit,
            Outcome::Pick(row) => {
                self.mode = Mode::Edit;
                self.textarea.cancel_selection();
                self.set_cursor_position((row, 0));
            }
        }
    }

    /// Alt-Enter and Ctrl-]: opens the file that the string under the cursor names, for example
    /// `#include "chapters/two.typ"`. The text goes to disk first. A name that is no file of the project is
    /// only named in the status line.
    fn open_named_file(&mut self) -> Action {
        let (row, column) = self.cursor_position();
        let Some(name) = string_near(&self.textarea.lines()[row], column) else {
            self.message = "No string with a file name on this line.".into();
            return Action::Stay;
        };
        match resolve_file(&self.root, &self.path, &name) {
            Ok(file) => {
                if !self.save(false) {
                    return Action::Stay;
                }
                Action::Goto {
                    file,
                    line: 1,
                    column: 1,
                }
            }
            Err(message) => {
                self.message = message;
                Action::Stay
            }
        }
    }

    /// How long after the last key a compile starts. After a fast compile it is the pause of the autosave.
    /// After a slow one (`SLOW_COMPILE` or more) it is twice that time, up to `MAX_COMPILE_WAIT`.
    fn compile_wait(&self) -> Duration {
        if self.watch_mode {
            // The write of the file starts the compile of the watch, and an incremental compile is cheap.
            return DEBOUNCE;
        }
        compile_wait_after(
            self.report
                .as_ref()
                .and_then(|report| report.elapsed)
                .unwrap_or_default(),
        )
    }

    /// The layout that the user chose, for the program to keep.
    pub fn chosen_arrangement(&self) -> Option<Arrangement> {
        self.arrangement
    }

    pub fn set_arrangement(&mut self, arrangement: Option<Arrangement>) {
        self.arrangement = arrangement;
    }

    /// The layout in a window `width` columns wide: the choice of the user, or else by the width.
    fn arrangement_for(&self, width: u16) -> Arrangement {
        self.arrangement.unwrap_or(if width < STACK_BELOW {
            Arrangement::Stacked
        } else {
            Arrangement::Side
        })
    }

    /// F10: the next layout. The window width decides the first one.
    fn cycle_arrangement(&mut self, width: u16) {
        let next = self.arrangement_for(width).next();
        self.arrangement = Some(next);
        self.message = next.describe().into();
    }

    /// F2: saves the text, then asks for the file switcher. A refused save keeps the editor here.
    fn switch_action(&mut self) -> Action {
        if self.save(false) {
            Action::Switch
        } else {
            Action::Stay
        }
    }

    /// Takes back the last edit. A replace is a delete and an insert in the text area, so if the text is
    /// still the text that the replace made, one more step takes back the delete: the user sees one undo.
    fn undo(&mut self) -> bool {
        let group = self
            .replaced
            .take()
            .filter(|(_, after)| after == self.textarea.lines());
        let undone = self.textarea.undo();
        if let Some((before, _)) = group
            && undone
            && self.textarea.lines() != before
        {
            self.textarea.undo();
        }
        undone
    }

    /// Remembers the text before and after a replace, for `undo`.
    fn note_replace(&mut self, before: Vec<String>) {
        self.replaced = Some((before, self.textarea.lines().to_vec()));
    }

    /// The regular expression of the last search, or `None` if there is no search text or it is invalid.
    fn replace_regex(&self) -> Option<Regex> {
        if self.last_search.is_empty() {
            return None;
        }
        Regex::new(&search_regex_for(&self.last_search, self.search_regex)).ok()
    }

    /// The first match that starts at or after the cursor, wrapping once at the end of the text. An empty
    /// match does not count. The match is (line, first char, end char).
    fn next_match(&self, re: &Regex) -> Option<(usize, usize, usize)> {
        let lines = self.textarea.lines();
        let (row, column) = self.cursor_position();
        let chars = |line: &str, byte: usize| line[..byte].chars().count();
        let first = |line: &str, from: usize| {
            re.find_iter(line)
                .find(|found| !found.is_empty() && found.start() >= from)
                .map(|found| (chars(line, found.start()), chars(line, found.end())))
        };
        let from = lines[row]
            .char_indices()
            .nth(column)
            .map_or(lines[row].len(), |(byte, _)| byte);
        if let Some((start, end)) = first(&lines[row], from) {
            return Some((row, start, end));
        }
        (row + 1..lines.len())
            .chain(0..=row)
            .find_map(|line| first(&lines[line], 0).map(|(start, end)| (line, start, end)))
    }

    /// The text for a match: the replacement as typed in plain mode. In regex mode `$1` and `${name}` give
    /// the groups of the match.
    fn expanded(&self, captures: &regex::Captures, replacement: &str) -> String {
        if self.search_regex {
            let mut text = String::new();
            captures.expand(replacement, &mut text);
            text
        } else {
            replacement.to_string()
        }
    }

    /// Opens the prompt `Replace with:` for the text of the last search. The cursor goes to the first match.
    fn open_replace(&mut self) {
        let Some(re) = self.replace_regex() else {
            self.mode = Mode::Edit;
            self.message = if self.last_search.is_empty() {
                "Search first: press Ctrl-F, type the text, then press Alt-S.".into()
            } else {
                "The search text is not valid.".into()
            };
            return;
        };
        let _ = self
            .textarea
            .set_search_pattern(search_regex_for(&self.last_search, self.search_regex));
        match self.next_match(&re) {
            Some((row, start, _)) => self.set_cursor_position((row, start)),
            None => self.message = format!("No match for {}", self.last_search),
        }
        let mut prompt = TextArea::new(vec![self.last_replace.clone()]);
        prompt.set_cursor_line_style(Style::default());
        prompt.move_cursor(CursorMove::End);
        self.mode = Mode::Replace(Box::new(prompt));
    }

    /// The keys of the prompt `Replace with:`. `Enter` replaces the match at the cursor and goes to the next
    /// one. `Alt-A` replaces all matches. `Esc` stops. Other keys edit the replacement.
    fn replace_key(&mut self, key: KeyEvent) {
        let Mode::Replace(prompt) = &mut self.mode else {
            return;
        };
        if key.code == KeyCode::Esc {
            self.mode = Mode::Edit;
            let _ = self.textarea.set_search_pattern("");
            return;
        }
        let all = key.modifiers.contains(KeyModifiers::ALT) && key.code == KeyCode::Char('a');
        if key.code != KeyCode::Enter && !all {
            prompt.input(key);
            self.last_replace = prompt.lines().join("");
            return;
        }
        let replacement = prompt.lines().join("");
        self.last_replace = replacement.clone();
        let Some(re) = self.replace_regex() else {
            self.message = "The search text is not valid.".into();
            return;
        };
        if all {
            self.replace_all(&re, &replacement);
        } else {
            self.replace_one(&re, &replacement);
        }
    }

    fn replace_one(&mut self, re: &Regex, replacement: &str) {
        let Some((row, start, end)) = self.next_match(re) else {
            self.message = format!("No match for {}", self.last_search);
            return;
        };
        let line = self.textarea.lines()[row].clone();
        let byte = line
            .char_indices()
            .nth(start)
            .map_or(line.len(), |(byte, _)| byte);
        let text = re.captures_at(&line, byte).map_or_else(
            || replacement.to_string(),
            |caps| self.expanded(&caps, replacement),
        );
        let before = self.textarea.lines().to_vec();
        self.textarea.cancel_selection();
        self.set_cursor_position((row, start));
        self.textarea.delete_str(end - start);
        self.textarea.insert_str(text);
        self.note_replace(before);
        self.mark_edit();
        let left: usize = self
            .textarea
            .lines()
            .iter()
            .map(|line| re.find_iter(line).filter(|found| !found.is_empty()).count())
            .sum();
        match self.next_match(re) {
            Some((row, start, _)) if left > 0 => {
                self.set_cursor_position((row, start));
                self.message = format!("Replaced. {left} left");
            }
            _ => self.message = "Replaced. No more matches".into(),
        }
    }

    fn replace_all(&mut self, re: &Regex, replacement: &str) {
        let mut count = 0;
        let lines: Vec<String> = self
            .textarea
            .lines()
            .iter()
            .map(|line| {
                re.replace_all(line, |caps: &regex::Captures| {
                    if caps[0].is_empty() {
                        return String::new();
                    }
                    count += 1;
                    self.expanded(caps, replacement)
                })
                .into_owned()
            })
            .collect();
        if count == 0 {
            self.message = format!("No match for {}", self.last_search);
            return;
        }
        // One insert over the whole text is one undo step.
        let place = self.cursor_position();
        let before = self.textarea.lines().to_vec();
        self.textarea.select_all();
        self.textarea.insert_str(lines.join("\n"));
        self.note_replace(before);
        self.set_cursor_position(place);
        self.mark_edit();
        self.message = format!(
            "Replaced {count} {}",
            if count == 1 { "match" } else { "matches" }
        );
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
            Mode::Replace(_) => {
                self.replace_key(key);
                return Action::Stay;
            }
            Mode::Outline(_) => {
                self.outline_key(key);
                return Action::Stay;
            }
            Mode::History(_) => {
                self.history_key(key);
                return Action::Stay;
            }
            Mode::Spell(_) => {
                self.spell_key(key);
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
        } else if key.code == KeyCode::F(2) {
            return self.switch_action();
        } else if (key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::ALT))
            || (ctrl && key.code == KeyCode::Char(']'))
        {
            return self.open_named_file();
        } else if key.code == KeyCode::F(4) {
            self.open_outline();
        } else if key.code == KeyCode::F(10) {
            self.cycle_arrangement(self.width);
        } else if key.code == KeyCode::F(6) {
            self.open_history();
        } else if key.code == KeyCode::F(7) {
            self.toggle_spell();
        } else if key.code == KeyCode::F(3) {
            self.toggle_follow();
        } else if key.code == KeyCode::F(9) {
            self.toggle_mouse();
        } else if key.modifiers.contains(KeyModifiers::ALT) && key.code == KeyCode::Char('i') {
            self.paste_image();
        } else if key.modifiers.contains(KeyModifiers::ALT) && key.code == KeyCode::Char('l') {
            self.load_from_disk();
        } else if cfg!(debug_assertions)
            && ctrl
            && key.modifiers.contains(KeyModifiers::ALT)
            && key.code == KeyCode::Char('p')
        {
            // Only in a debug build, for the test of the rescue after a panic. A release build has no such key.
            panic!("forced panic for the test of the rescue");
        } else if key.modifiers.contains(KeyModifiers::ALT) && key.code == KeyCode::Char(';') {
            self.open_spell();
        } else if key.code == KeyCode::F(5) {
            self.paused = !self.paused;
            if self.paused {
                // The watch would compile each save. Ctrl-B starts one that ends after its report.
                self.stop_compile();
            }
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
        } else if ctrl && matches!(key.code, KeyCode::Char('z' | 'u')) {
            // Ctrl-Z undoes, as in most editors. The text area undoes with Ctrl-U.
            if self.undo() {
                self.mark_edit();
            } else {
                self.message = "Nothing to undo".into();
            }
        } else if key.modifiers.contains(KeyModifiers::ALT) && key.code == KeyCode::Char('s') {
            self.open_replace();
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
            let edited = self.last_edit.take().unwrap_or(now);
            let saved = self.save(false);
            if saved && !self.paused {
                // The save is always quick. The compile of a slow document waits, so the machine is not busy
                // all the time while the user types.
                let wait = self.compile_wait();
                if wait <= DEBOUNCE {
                    self.compile_after_save();
                } else {
                    self.compile_at = Some((edited + wait, wait));
                }
            }
            changed = true;
        }
        if let Some((due, _)) = self.compile_at
            && now >= due
        {
            self.compile_after_save();
            changed = true;
        }
        changed |= self.watch_file(now);
        changed |= self.watch_deps(now);
        changed |= self.poll_spell();
        changed |= self.poll_follow();
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
            || self.compile_busy()
            || self.watch_mode
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

    /// Puts `text`, the text of the file, in the buffer. The cursor keeps its place if the text is long enough.
    fn replace_buffer(&mut self, text: &str) {
        let place = self.cursor_position();
        self.crlf = text.contains("\r\n");
        self.textarea = new_textarea(text);
        self.text_version += 1;
        if matches!(self.mode, Mode::Search(_)) {
            let _ = self
                .textarea
                .set_search_pattern(search_regex_for(&self.last_search, self.search_regex));
        }
        self.count_words();
        self.set_cursor_position(place);
    }

    /// `Alt-L`: takes the text of the file from disk and drops the edits of the buffer. The edits go to the
    /// local history first (`F6` lists them), so a wrong key loses nothing. Without a history the edits
    /// would be lost, so the key does nothing then.
    fn load_from_disk(&mut self) {
        let text = match fsutil::read_text(&self.path) {
            Ok(text) => text,
            Err(err) => {
                self.message = format!("Cannot read the file: {err}");
                return;
            }
        };
        if self.dirty {
            if self.history.is_none() {
                self.message =
                    "There is no history to keep your text. Ctrl-S saves it first.".into();
                return;
            }
            let mine = self.file_text();
            self.keep_version(&mine, true);
        }
        self.replace_buffer(&text);
        self.dirty = false;
        self.conflict = false;
        self.last_edit = None;
        self.disk_time = disk_time(&self.path);
        self.message = "Loaded the file from disk. Your text is in the history (F6).".into();
        if !self.paused {
            self.start_compile();
        }
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
                self.replace_buffer(&text);
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
