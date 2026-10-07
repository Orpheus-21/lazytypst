mod browser;
mod compile;
mod editor;
mod fsutil;
mod newfile;
mod preview;
mod state;

use std::{
    collections::HashMap,
    ffi::OsString,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use ratatui::{
    DefaultTerminal, Frame,
    crossterm::event::{self, Event, KeyCode, KeyEvent},
    layout::{Constraint, Layout},
    style::{Modifier, Style},
    widgets::{Block, List, ListState, Paragraph},
};

use ratatui_image::picker::Picker;
use ratatui_textarea::{CursorMove, TextArea};

use editor::{Action, Editor};

/// What the prompt asks for.
#[derive(Clone, Copy, PartialEq)]
enum PromptKind {
    NewFile,
    Filter,
}

impl PromptKind {
    fn label(self) -> &'static str {
        match self {
            PromptKind::NewFile => "New file: ",
            PromptKind::Filter => "Filter: ",
        }
    }
}

struct Prompt {
    kind: PromptKind,
    input: TextArea<'static>,
}

struct App {
    root: PathBuf,
    picker: Picker,
    files: Vec<PathBuf>,
    /// The main file, as a path relative to `root`. The compile and the export use it.
    main_file: Option<PathBuf>,
    /// The file that keeps the main file of each project. `None` if the environment names no place.
    state_file: Option<PathBuf>,
    list: ListState,
    status: String,
    /// Where the cursor was in each file when the user closed it. The positions stay in memory for this run.
    cursors: HashMap<PathBuf, (usize, usize)>,
    /// The prompt for a new file name, or for the filter. While it is open, it takes every key.
    prompt: Option<Prompt>,
    /// The part of a path that a file must have to show in the list. The match ignores case. Empty: no filter.
    filter: String,
    /// All the files that the folder has. `files` holds those that the filter lets through.
    all_files: Vec<PathBuf>,
    /// The open file. The browser shows while this is `None`.
    editor: Option<Editor>,
}

const USAGE: &str = "\
Usage: lazytypst [FOLDER | FILE.typ]

lazytypst lists the .typ files in FOLDER and opens them in an editor with a live preview.
Without FOLDER, lazytypst uses the current folder. FOLDER is also the Typst project root.
With a .typ file, lazytypst opens the file at once. The folder of the file is the project root.

Options:
  -h, --help     Show this help.
  -V, --version  Show the version of lazytypst and the version of typst.

Keys in the file list:
  j or Down      Select the next file.
  k or Up        Select the previous file.
  Enter          Open the selected file.
  n              Make a new .typ file. Type a path such as chapters/two. The ending .typ is added.
  /              Filter the list. Type a part of a path. Enter keeps the filter, Esc removes it.
  r              Read the folder again, to show new files and to drop deleted files.
  g or Home      Select the first file.
  G or End       Select the last file.
  m              Mark the selected file as the main file, or remove the mark.
                 lazytypst then compiles the main file, whatever file you edit.
  q              Quit.

Keys in the editor:
  Ctrl-S         Save the file.
  Ctrl-B         Save and compile the file.
  Ctrl-E         Save the file and export a PDF next to it.
  Ctrl-G         Go to the first error of the last compile.
  Alt-Down       Show the next page.
  Alt-Up         Show the previous page.
  Alt-Home       Show the first page.
  Alt-End        Show the last page.
  Esc            Save the file and go back to the file list.
  Ctrl-Q         Save the file and quit.
";

#[derive(Debug, PartialEq)]
enum Args {
    Run(PathBuf),
    Help,
    Version,
}

/// The commands that save the window title of the terminal and bring it back (xterm window operations 22
/// and 23). A terminal that follows xterm restores the title that the user had. Ghostty 1.3.1 ignores both,
/// as a test with a real Ghostty window showed: the title that the program set stays. The shell sets the
/// title again at the next prompt (the shell integration of Ghostty does this).
const SAVE_TITLE: &str = "\x1b[22;0t";
const RESTORE_TITLE: &str = "\x1b[23;0t";
/// Bracketed paste: the terminal sends a paste as one event and not as one key for each character.
const ENABLE_PASTE: &str = "\x1b[?2004h";
const DISABLE_PASTE: &str = "\x1b[?2004l";

fn write_to_terminal(text: &str) {
    use std::io::Write;
    let mut out = std::io::stdout();
    let _ = out.write_all(text.as_bytes());
    let _ = out.flush();
}

/// The title of the terminal window: `lazytypst: <folder name>` in the list, and
/// `lazytypst: <path of the file relative to the root>` in the editor. A window title helps to find the
/// right window when many are open.
///
/// The title goes to the terminal inside an escape sequence, and a file name comes from users and from
/// other programs. A control character in it could end the sequence early and start another one. So each
/// control character becomes a question mark.
fn window_title(root: &Path, open: Option<&Path>) -> String {
    let name = match open {
        Some(file) => file
            .strip_prefix(root)
            .unwrap_or(file)
            .display()
            .to_string(),
        None => root.file_name().map_or_else(
            || root.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        ),
    };
    let name: String = name
        .chars()
        .map(|letter| if letter.is_control() { '?' } else { letter })
        .collect();
    format!("lazytypst: {name}")
}

/// Why the `typst` command does not run, in a few words for the user.
fn typst_problem(err: &std::io::Error) -> String {
    if err.kind() == std::io::ErrorKind::NotFound {
        "not found in PATH".into()
    } else {
        err.to_string()
    }
}

/// The text that the program prints, before it draws anything, when the `typst` command does not run.
fn missing_typst_message(err: &std::io::Error) -> String {
    format!(
        "lazytypst needs the typst command, and it cannot run it: {}.\n\
         Install Typst, and make sure that it is in PATH: https://github.com/typst/typst#installation\n",
        typst_problem(err)
    )
}

/// The text of `--version`: the version of lazytypst, and the version line of typst or the reason
/// why typst does not run. A bug report needs both versions.
fn version_text(lazytypst: &str, typst: std::io::Result<String>) -> String {
    let typst = typst.unwrap_or_else(|err| format!("typst: {}", typst_problem(&err)));
    format!("lazytypst {lazytypst}\n{typst}\n")
}

/// What the first argument names: the project root, and a file to open at once if the argument is a file.
#[derive(Debug, PartialEq)]
struct Target {
    root: PathBuf,
    /// The file to open, as a path relative to `root`.
    open: Option<PathBuf>,
}

/// Finds the root and the file to open. The argument is a folder, or a file with the ending `.typ`.
/// The folder of a file is the root. A symlink keeps its own name and its own folder.
/// The error has the message and the exit code: 1 if the path cannot be opened, and 2 if it is a file
/// that does not end with `.typ`.
fn resolve_target(arg: &Path) -> Result<Target, (String, i32)> {
    let cannot_open = |err: std::io::Error| (format!("Cannot open {}: {err}", arg.display()), 1);
    // An absolute root keeps every derived path valid when typst runs inside the root.
    if std::fs::metadata(arg).map_err(cannot_open)?.is_dir() {
        return Ok(Target {
            root: std::fs::canonicalize(arg).map_err(cannot_open)?,
            open: None,
        });
    }
    if arg.extension().is_none_or(|ending| ending != "typ") {
        return Err((
            format!(
                "{} is not a .typ file. Give a folder or a .typ file.",
                arg.display()
            ),
            2,
        ));
    }
    let folder = arg
        .parent()
        .filter(|folder| !folder.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let root = std::fs::canonicalize(folder).map_err(cannot_open)?;
    Ok(Target {
        root,
        open: arg.file_name().map(PathBuf::from),
    })
}

/// Reads the arguments. `args_os` keeps a folder name that is not UTF-8. `args` would panic on it.
fn parse_args(mut args: impl Iterator<Item = OsString>) -> Result<Args, String> {
    args.next(); // the program name
    let Some(first) = args.next() else {
        return Ok(Args::Run(PathBuf::from(".")));
    };
    if args.next().is_some() {
        return Err("Give one folder only.".into());
    }
    match first.to_str() {
        Some("-h" | "--help") => Ok(Args::Help),
        Some("-V" | "--version") => Ok(Args::Version),
        Some(option) if option.starts_with('-') => Err(format!(
            "Unknown option: {option}. For a folder that starts with a dash, write ./{option}."
        )),
        _ => Ok(Args::Run(PathBuf::from(first))),
    }
}

impl App {
    fn new(root: PathBuf, files: Vec<PathBuf>, picker: Picker) -> Self {
        let first = (!files.is_empty()).then_some(0);
        App {
            root,
            picker,
            all_files: files.clone(),
            files,
            filter: String::new(),
            main_file: None,
            state_file: None,
            list: ListState::default().with_selected(first),
            status: String::new(),
            prompt: None,
            cursors: HashMap::new(),
            editor: None,
        }
    }

    /// Sets the state file and takes the saved main file of this project. A saved file that is not in the list
    /// is ignored.
    fn load_state(&mut self, state_file: Option<PathBuf>) {
        self.main_file = state_file
            .as_deref()
            .and_then(|file| state::load_main(file, &self.root))
            .filter(|main| self.all_files.contains(main));
        self.state_file = state_file;
    }

    /// Saves the main file choice. A failure only shows a message: the mark still works in this run.
    fn save_main_choice(&mut self) {
        let Some(file) = &self.state_file else {
            return;
        };
        if let Err(err) = state::save_main(file, &self.root, self.main_file.as_deref()) {
            self.status = format!("Cannot save the main file choice: {err}");
        }
    }

    /// Reads the folder again. The selection stays on the same file if it is still there, and moves to
    /// the first file if not. A main file that is gone loses its mark, and the saved choice goes with it.
    /// If the folder cannot be read, the old list stays.
    fn refresh(&mut self) {
        let files = match browser::find_typ_files(&self.root, browser::MAX_DEPTH) {
            Ok(files) => files,
            Err(err) => {
                self.status = format!("Cannot read the folder: {err}");
                return;
            }
        };
        self.status = format!("Read the folder again: {} files", files.len());
        self.all_files = files;
        self.set_filter(self.filter.clone()); // keeps the filter and the selection
        if self
            .main_file
            .as_ref()
            .is_some_and(|main| !self.all_files.contains(main))
        {
            self.main_file = None;
            self.save_main_choice();
        }
    }

    /// Opens the selected file in the editor.
    fn open_selected(&mut self) {
        if let Some(path) = self.selected_file().cloned() {
            self.open_file(path);
        }
    }

    /// Opens the file that the command line named, and selects it in the list if the list shows it.
    /// A file that the list does not show, for example a hidden file, opens too.
    fn open_target(&mut self, relative: PathBuf) {
        if let Some(index) = self.files.iter().position(|file| *file == relative) {
            self.list.select(Some(index));
        }
        self.open_file(relative);
    }

    /// Opens the file `path`, relative to the root, in the editor. A file that cannot be read shows an
    /// error in the list.
    fn open_file(&mut self, path: PathBuf) {
        let main = self.main_file.as_ref().map(|main| self.root.join(main));
        let opened = Editor::open(
            self.root.join(&path),
            self.root.clone(),
            main,
            self.picker.clone(),
        );
        self.status = match opened {
            Ok(mut editor) => {
                if let Some(place) = self.cursors.get(editor.path()) {
                    editor.set_cursor_position(*place);
                }
                self.editor = Some(editor);
                String::new()
            }
            Err(err) => format!("Cannot open {}: {err}", path.display()),
        };
    }

    /// Opens the prompt for the name of a new file, or for the filter. The filter prompt starts with the
    /// filter that is on, so the user can change it.
    fn open_prompt(&mut self, kind: PromptKind) {
        let mut input = match kind {
            PromptKind::NewFile => TextArea::default(),
            PromptKind::Filter => TextArea::new(vec![self.filter.clone()]),
        };
        input.set_cursor_line_style(Style::default());
        input.move_cursor(CursorMove::End);
        self.prompt = Some(Prompt { kind, input });
        self.status = match kind {
            PromptKind::NewFile => {
                "Type a path such as chapters/two. Enter makes the file. Esc cancels."
            }
            PromptKind::Filter => {
                "Type to filter the list. Enter keeps the filter. Esc removes it."
            }
        }
        .into();
    }

    /// Handles a key while the prompt is open. Enter makes the file and opens it. If the name is not
    /// valid, the prompt stays open and the status line says why.
    fn handle_prompt_key(&mut self, key: KeyEvent) {
        let Some(prompt) = &mut self.prompt else {
            return;
        };
        let kind = prompt.kind;
        let text = prompt.input.lines().join("");
        match (kind, key.code) {
            (PromptKind::Filter, KeyCode::Esc) => {
                self.prompt = None;
                self.status.clear();
                self.set_filter(String::new());
            }
            (PromptKind::NewFile, KeyCode::Esc) => {
                self.prompt = None;
                self.status.clear();
            }
            (PromptKind::Filter, KeyCode::Enter) => {
                self.prompt = None;
                self.status.clear();
            }
            (PromptKind::NewFile, KeyCode::Enter) => {
                match newfile::create(&self.root, &text, browser::MAX_DEPTH) {
                    Ok(made) => {
                        self.prompt = None;
                        self.filter.clear(); // the new file must show in the list
                        self.refresh();
                        if let Some(index) = self.files.iter().position(|file| *file == made) {
                            self.list.select(Some(index));
                        }
                        self.open_selected();
                    }
                    Err(message) => self.status = message,
                }
            }
            (PromptKind::Filter, _) => {
                prompt.input.input(key);
                // The list follows the text at once, while the user types.
                let text = prompt.input.lines().join("");
                self.set_filter(text);
            }
            (PromptKind::NewFile, _) => {
                prompt.input.input(key);
            }
        }
    }

    /// Sets the filter and shows the files that match. The selection stays on the same file if the filter
    /// still shows it, and moves to the first file if not.
    fn set_filter(&mut self, filter: String) {
        self.filter = filter;
        let selected = self.selected_file().cloned();
        let needle = self.filter.to_lowercase();
        self.files = self
            .all_files
            .iter()
            .filter(|path| {
                needle.is_empty() || path.to_string_lossy().to_lowercase().contains(&needle)
            })
            .cloned()
            .collect();
        let index = selected.and_then(|path| self.files.iter().position(|file| *file == path));
        self.list
            .select((!self.files.is_empty()).then_some(index.unwrap_or(0)));
    }

    /// Selects the file at `index`. `None`, or an index beyond the list, selects nothing in an empty list
    /// and the last file otherwise. The index is set at once, so a key that follows needs no redraw.
    fn select_index(&mut self, index: Option<usize>) {
        let last = self.files.len().checked_sub(1);
        self.list
            .select(index.zip(last).map(|(index, last)| index.min(last)));
    }

    /// The file that the selection is on.
    fn selected_file(&self) -> Option<&PathBuf> {
        self.list.selected().and_then(|i| self.files.get(i))
    }

    /// Marks the selected file as the main file. The same key on the main file removes the mark.
    fn toggle_main(&mut self) {
        let Some(selected) = self.selected_file().cloned() else {
            return;
        };
        if self.main_file.as_ref() == Some(&selected) {
            self.main_file = None;
            self.status = "No main file. lazytypst compiles the open file.".into();
        } else {
            self.status = format!("Main file: {}", selected.display());
            self.main_file = Some(selected);
        }
        self.save_main_choice();
    }

    /// Handles a paste. The editor inserts it. The list and the prompts ignore it.
    fn handle_paste(&mut self, text: &str) {
        if let Some(editor) = &mut self.editor {
            editor.paste(text);
        }
    }

    /// Handles one key. Returns true when the program must quit.
    fn handle_key(&mut self, key: KeyEvent) -> bool {
        if let Some(editor) = &mut self.editor {
            let action = editor.handle_key(key);
            if !matches!(action, Action::Stay) {
                self.cursors
                    .insert(editor.path().to_path_buf(), editor.cursor_position());
                self.editor = None;
            }
            return matches!(action, Action::Quit);
        }
        if self.prompt.is_some() {
            self.handle_prompt_key(key);
            return false;
        }
        match key.code {
            KeyCode::Char('q') => return true,
            KeyCode::Char('j') | KeyCode::Down if !self.files.is_empty() => self.list.select_next(),
            KeyCode::Char('k') | KeyCode::Up if !self.files.is_empty() => {
                self.list.select_previous()
            }
            KeyCode::Char('g') | KeyCode::Home => self.select_index(Some(0)),
            KeyCode::Char('G') | KeyCode::End => self.select_index(self.files.len().checked_sub(1)),
            KeyCode::Char('m') => self.toggle_main(),
            KeyCode::Char('r') => self.refresh(),
            KeyCode::Char('n') => self.open_prompt(PromptKind::NewFile),
            KeyCode::Char('/') => self.open_prompt(PromptKind::Filter),
            KeyCode::Esc if !self.filter.is_empty() => self.set_filter(String::new()),
            KeyCode::Enter => self.open_selected(),
            _ => {}
        }
        false
    }
}

fn main() -> std::io::Result<()> {
    let arg = match parse_args(std::env::args_os()) {
        Ok(Args::Run(arg)) => arg,
        Ok(Args::Help) => {
            print!("{USAGE}");
            return Ok(());
        }
        Ok(Args::Version) => {
            print!(
                "{}",
                version_text(env!("CARGO_PKG_VERSION"), compile::typst_version())
            );
            return Ok(());
        }
        Err(message) => {
            eprint!("{message}\n\n{USAGE}");
            std::process::exit(2);
        }
    };
    let Target { root, open } = match resolve_target(&arg) {
        Ok(target) => target,
        Err((message, code)) => {
            eprintln!("{message}");
            std::process::exit(code);
        }
    };
    // Check typst before anything else changes: no temporary folder is made, and the terminal is not touched.
    if let Err(err) = compile::typst_version() {
        eprint!("{}", missing_typst_message(&err));
        std::process::exit(1);
    }
    let files = browser::find_typ_files(&root, browser::MAX_DEPTH)?;
    compile::make_out_dir().map_err(|err| {
        let message = format!(
            "Cannot make the folder {}: {err}",
            compile::out_dir().display()
        );
        std::io::Error::new(err.kind(), message)
    })?;
    compile::remove_stale_dirs();
    // ratatui::init also installs a panic hook that restores the terminal.
    let mut terminal = ratatui::init();
    write_to_terminal(SAVE_TITLE);
    write_to_terminal(ENABLE_PASTE);
    // The panic hook of ratatui runs after this one, so a panic also deletes the page folder.
    let restore_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        compile::cleanup();
        write_to_terminal(RESTORE_TITLE);
        write_to_terminal(DISABLE_PASTE);
        restore_hook(info);
    }));
    // The query needs the raw terminal, and it must run before the first key is read.
    // It finds the image protocol and the font size. If it fails, the preview uses half blocks.
    let picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());
    let mut app = App::new(root, files, picker);
    app.load_state(state::default_state_file());
    if let Some(relative) = open {
        app.open_target(relative);
    }
    let result = run(&mut terminal, &mut app);
    write_to_terminal(RESTORE_TITLE);
    write_to_terminal(DISABLE_PASTE);
    ratatui::restore();
    compile::cleanup();
    result
}

fn run(terminal: &mut DefaultTerminal, app: &mut App) -> std::io::Result<()> {
    let mut redraw = true;
    let mut title = String::new();
    loop {
        // The title changes when the editor opens or closes. It is sent only when it changes.
        let wanted = window_title(&app.root, app.editor.as_ref().map(Editor::path));
        if wanted != title {
            let _ = ratatui::crossterm::execute!(
                std::io::stdout(),
                ratatui::crossterm::terminal::SetTitle(&wanted)
            );
            title = wanted;
        }
        if redraw {
            terminal.draw(|frame| draw(frame, app))?;
        }
        // While no key arrives, every 50 ms the editor checks its autosave and its compile.
        if !event::poll(Duration::from_millis(50))? {
            redraw = app
                .editor
                .as_mut()
                .is_some_and(|editor| editor.tick(Instant::now()));
            continue;
        }
        redraw = true;
        match event::read()? {
            Event::Key(key) => {
                if app.handle_key(key) {
                    return Ok(());
                }
            }
            Event::Paste(text) => app.handle_paste(&text),
            _ => {}
        }
    }
}

fn draw(frame: &mut Frame, app: &mut App) {
    if let Some(editor) = &mut app.editor {
        editor.draw(frame);
        return;
    }
    let prompt_height = u16::from(app.prompt.is_some());
    let [body, status, prompt_row] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(prompt_height),
    ])
    .areas(frame.area());
    let title = if app.filter.is_empty() {
        "lazytypst".to_string()
    } else {
        format!("lazytypst /{}", app.filter)
    };
    let block = Block::bordered().title(title);
    if app.all_files.is_empty() {
        frame.render_widget(Paragraph::new("No .typ files").block(block), body);
    } else if app.files.is_empty() {
        frame.render_widget(Paragraph::new("No match").block(block), body);
    } else {
        let items = app.files.iter().map(|path| {
            let mark = if app.main_file.as_ref() == Some(path) {
                " [main]"
            } else {
                ""
            };
            format!("{}{mark}", path.display())
        });
        let list = List::new(items)
            .block(block)
            .highlight_style(Style::new().add_modifier(Modifier::REVERSED))
            .highlight_symbol("> ");
        frame.render_stateful_widget(list, body, &mut app.list);
    }
    frame.render_widget(Paragraph::new(app.status.as_str()), status);
    if let Some(prompt) = &app.prompt {
        let label_text = prompt.kind.label();
        let label_width = u16::try_from(label_text.len()).unwrap_or(u16::MAX);
        let [label, input] =
            Layout::horizontal([Constraint::Length(label_width), Constraint::Min(1)])
                .areas(prompt_row);
        frame.render_widget(Paragraph::new(label_text), label);
        frame.render_widget(&prompt.input, input);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStringExt;

    use ratatui::{Terminal, backend::TestBackend};
    use std::fs;

    /// Makes a folder with the files `a.typ`, `sub/b.typ`, and `notes.txt`. Returns the app for it.
    fn app(name: &str) -> App {
        let root =
            std::env::temp_dir().join(format!("lazytypst-main-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("a.typ"), "text of a\n").unwrap();
        fs::write(root.join("sub").join("b.typ"), "text of b\n").unwrap();
        fs::write(root.join("notes.txt"), "").unwrap();
        let files = browser::find_typ_files(&root, browser::MAX_DEPTH).unwrap();
        App::new(root, files, Picker::halfblocks())
    }

    fn press(app: &mut App, code: KeyCode) -> bool {
        app.handle_key(KeyEvent::from(code))
    }

    /// Draws the app on a 100 by 24 screen and returns all the text on it.
    fn screen(app: &mut App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn the_list_shows_the_typ_files_only() {
        let mut app = app("list");
        let text = screen(&mut app);
        assert!(
            text.contains("a.typ") && text.contains("sub/b.typ"),
            "{text}"
        );
        assert!(!text.contains("notes.txt"));
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn j_then_enter_opens_the_second_file_and_esc_goes_back() {
        let mut app = app("open");
        assert!(!press(&mut app, KeyCode::Char('j')));
        assert!(!press(&mut app, KeyCode::Enter));
        assert!(screen(&mut app).contains("text of b"));

        assert!(!press(&mut app, KeyCode::Esc));
        assert!(app.editor.is_none());
        assert!(screen(&mut app).contains("sub/b.typ"));
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn the_selection_stops_at_both_ends_of_the_list() {
        let mut app = app("ends");
        for _ in 0..5 {
            press(&mut app, KeyCode::Down);
            screen(&mut app); // a draw clamps the selection, as in the real loop
        }
        press(&mut app, KeyCode::Enter);
        assert!(screen(&mut app).contains("text of b"));
        press(&mut app, KeyCode::Esc);
        for _ in 0..5 {
            press(&mut app, KeyCode::Char('k'));
        }
        press(&mut app, KeyCode::Enter);
        assert!(screen(&mut app).contains("text of a"));
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn m_marks_the_selected_file_as_main_and_a_second_m_removes_the_mark() {
        let mut app = app("mark");
        assert!(!screen(&mut app).contains("[main]"));

        press(&mut app, KeyCode::Char('m'));
        assert_eq!(app.main_file, Some(PathBuf::from("a.typ")));
        assert!(screen(&mut app).contains("a.typ [main]"));
        press(&mut app, KeyCode::Char('m'));
        assert_eq!(app.main_file, None);
        assert!(!screen(&mut app).contains("[main]"));
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn m_on_another_file_moves_the_mark() {
        let mut app = app("move");
        press(&mut app, KeyCode::Char('m'));
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('m'));
        assert_eq!(app.main_file, Some(PathBuf::from("sub/b.typ")));
        let text = screen(&mut app);
        assert!(
            text.contains("sub/b.typ [main]") && !text.contains("a.typ [main]"),
            "{text}"
        );
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn a_file_opens_with_the_marked_main_file() {
        let mut app = app("openmain");
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('m')); // sub/b.typ is the main file
        press(&mut app, KeyCode::Char('k'));
        press(&mut app, KeyCode::Enter); // open a.typ
        assert!(screen(&mut app).contains("a.typ (main: sub/b.typ)"));
        fs::remove_dir_all(&app.root).unwrap();
    }

    /// A state file path in a new temporary folder.
    fn state_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lazytypst-main-state-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir.join("lazytypst").join("main-files")
    }

    #[test]
    fn the_main_file_is_saved_and_comes_back_in_the_next_run() {
        let state = state_path("roundtrip");
        let mut first = app("restore");
        first.load_state(Some(state.clone()));
        press(&mut first, KeyCode::Char('j'));
        press(&mut first, KeyCode::Char('m')); // sub/b.typ

        let mut second = App::new(
            first.root.clone(),
            first.files.clone(),
            Picker::halfblocks(),
        );
        second.load_state(Some(state.clone()));
        assert_eq!(second.main_file, Some(PathBuf::from("sub/b.typ")));
        assert!(screen(&mut second).contains("sub/b.typ [main]"));

        press(&mut second, KeyCode::Char('j'));
        press(&mut second, KeyCode::Char('m')); // sub/b.typ again: removes the mark
        let mut third = App::new(
            first.root.clone(),
            first.files.clone(),
            Picker::halfblocks(),
        );
        third.load_state(Some(state.clone()));
        assert_eq!(third.main_file, None);
        fs::remove_dir_all(&first.root).unwrap();
        fs::remove_dir_all(state.parent().unwrap().parent().unwrap()).unwrap();
    }

    #[test]
    fn a_saved_main_file_that_is_not_in_the_list_is_ignored() {
        let state = state_path("notlisted");
        let mut first = app("notlisted");
        first.load_state(Some(state.clone()));
        press(&mut first, KeyCode::Char('m'));
        fs::remove_file(first.root.join("a.typ")).unwrap();

        let files = browser::find_typ_files(&first.root, browser::MAX_DEPTH).unwrap();
        let mut second = App::new(first.root.clone(), files, Picker::halfblocks());
        second.load_state(Some(state.clone()));
        assert_eq!(second.main_file, None);
        assert!(
            second.status.is_empty(),
            "no error for a missing main file: {}",
            second.status
        );
        fs::remove_dir_all(&first.root).unwrap();
        fs::remove_dir_all(state.parent().unwrap().parent().unwrap()).unwrap();
    }

    #[test]
    fn a_state_file_that_cannot_be_written_shows_a_message_and_keeps_the_mark() {
        let mut app = app("unwritable");
        // The parent of the state file is a regular file, so the folder cannot be made.
        let blocker = app.root.join("a.typ");
        app.load_state(Some(blocker.join("lazytypst").join("main-files")));
        press(&mut app, KeyCode::Char('m'));
        assert_eq!(
            app.main_file,
            Some(PathBuf::from("a.typ")),
            "the mark must work without the state file"
        );
        assert!(
            app.status.contains("Cannot save the main file"),
            "{}",
            app.status
        );
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn without_a_state_file_the_mark_still_works() {
        let mut app = app("nostate");
        app.load_state(None);
        press(&mut app, KeyCode::Char('m'));
        assert_eq!(app.main_file, Some(PathBuf::from("a.typ")));
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn r_shows_a_file_that_was_made_after_the_start() {
        let mut app = app("refresh-new");
        fs::write(app.root.join("new.typ"), "").unwrap();
        assert!(!screen(&mut app).contains("new.typ"));
        press(&mut app, KeyCode::Char('r'));
        assert!(screen(&mut app).contains("new.typ"));
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn r_removes_a_file_that_was_deleted() {
        let mut app = app("refresh-gone");
        fs::remove_file(app.root.join("sub").join("b.typ")).unwrap();
        press(&mut app, KeyCode::Char('r'));
        let text = screen(&mut app);
        assert!(text.contains("a.typ") && !text.contains("b.typ"), "{text}");
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn r_keeps_the_selection_on_the_same_file() {
        let mut app = app("refresh-keep");
        press(&mut app, KeyCode::Char('j')); // sub/b.typ
        screen(&mut app);
        fs::write(app.root.join("0first.typ"), "").unwrap(); // sorts before every other file
        press(&mut app, KeyCode::Char('r'));
        screen(&mut app);
        press(&mut app, KeyCode::Enter);
        assert!(
            screen(&mut app).contains("text of b"),
            "the selection moved to another file"
        );
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn r_selects_the_first_file_when_the_selected_file_is_gone() {
        let mut app = app("refresh-selected-gone");
        press(&mut app, KeyCode::Char('j')); // sub/b.typ
        screen(&mut app);
        fs::remove_file(app.root.join("sub").join("b.typ")).unwrap();
        press(&mut app, KeyCode::Char('r'));
        screen(&mut app);
        press(&mut app, KeyCode::Enter);
        assert!(screen(&mut app).contains("text of a"));
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn r_removes_the_main_mark_of_a_file_that_is_gone_and_saves_that() {
        let state = state_path("refresh-main");
        let mut app = app("refresh-main");
        app.load_state(Some(state.clone()));
        press(&mut app, KeyCode::Char('m')); // a.typ
        fs::remove_file(app.root.join("a.typ")).unwrap();
        press(&mut app, KeyCode::Char('r'));
        assert_eq!(app.main_file, None);
        assert!(
            !fs::read_to_string(&state).unwrap().contains("a.typ"),
            "the saved choice stays"
        );
        fs::remove_dir_all(&app.root).unwrap();
        fs::remove_dir_all(state.parent().unwrap().parent().unwrap()).unwrap();
    }

    #[test]
    fn r_keeps_the_main_mark_of_a_file_that_is_still_there() {
        let mut app = app("refresh-main-stays");
        press(&mut app, KeyCode::Char('m'));
        fs::write(app.root.join("0first.typ"), "").unwrap();
        press(&mut app, KeyCode::Char('r'));
        assert_eq!(app.main_file, Some(PathBuf::from("a.typ")));
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn r_in_a_folder_that_is_gone_shows_an_error_and_keeps_the_list() {
        let mut app = app("refresh-folder-gone");
        let root = app.root.clone();
        fs::remove_dir_all(&root).unwrap();
        press(&mut app, KeyCode::Char('r'));
        let text = screen(&mut app);
        assert!(text.contains("Cannot read the folder"), "{text}");
        assert!(text.contains("a.typ"), "the old list must stay: {text}");
    }

    #[test]
    fn r_shows_the_empty_message_when_all_files_are_gone() {
        let mut app = app("refresh-empty");
        fs::remove_file(app.root.join("a.typ")).unwrap();
        fs::remove_file(app.root.join("sub").join("b.typ")).unwrap();
        press(&mut app, KeyCode::Char('r'));
        assert!(screen(&mut app).contains("No .typ files"));
        press(&mut app, KeyCode::Enter);
        assert!(app.editor.is_none());
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn r_types_a_letter_in_the_editor() {
        let mut app = app("refresh-editor");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('r'));
        assert!(screen(&mut app).contains("rtext of a"));
        fs::remove_dir_all(&app.root).unwrap();
    }

    fn type_text(app: &mut App, text: &str) {
        for letter in text.chars() {
            press(app, KeyCode::Char(letter));
        }
    }

    #[test]
    fn n_opens_the_prompt_and_esc_closes_it_without_a_file() {
        let mut app = app("new-esc");
        press(&mut app, KeyCode::Char('n'));
        assert!(screen(&mut app).contains("New file:"));
        type_text(&mut app, "draft");
        press(&mut app, KeyCode::Esc);
        let text = screen(&mut app);
        assert!(
            !text.contains("New file:") && text.contains("a.typ"),
            "{text}"
        );
        assert!(!app.root.join("draft.typ").exists());
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn the_prompt_takes_every_key_before_the_list_does() {
        let mut app = app("new-keys");
        press(&mut app, KeyCode::Char('n'));
        for letter in "qjkmrn".chars() {
            assert!(
                !press(&mut app, KeyCode::Char(letter)),
                "{letter} quit the program"
            );
        }
        assert!(screen(&mut app).contains("New file: qjkmrn"));
        assert_eq!(app.main_file, None, "m marked a file");
        assert!(app.editor.is_none());
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn enter_makes_the_file_selects_it_and_opens_it() {
        let mut app = app("new-ok");
        press(&mut app, KeyCode::Char('n'));
        type_text(&mut app, "chapters/two");
        press(&mut app, KeyCode::Enter);

        assert!(app.root.join("chapters").join("two.typ").is_file());
        assert!(
            screen(&mut app).contains("chapters/two.typ"),
            "the editor must show the new file"
        );
        press(&mut app, KeyCode::Esc);
        let text = screen(&mut app);
        assert!(
            text.contains("chapters/two.typ") && !text.contains("New file:"),
            "{text}"
        );
        // The new file is selected: Enter opens it again.
        press(&mut app, KeyCode::Enter);
        assert!(screen(&mut app).contains("chapters/two.typ"));
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn a_bad_name_shows_the_error_and_keeps_the_prompt() {
        let mut app = app("new-bad");
        press(&mut app, KeyCode::Char('n'));
        type_text(&mut app, "../x");
        press(&mut app, KeyCode::Enter);
        let text = screen(&mut app);
        assert!(
            text.contains("inside the project") && text.contains("New file: ../x"),
            "{text}"
        );
        assert!(app.editor.is_none());
        press(&mut app, KeyCode::Esc);
        assert!(!app.root.parent().unwrap().join("x.typ").exists());
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn a_name_that_exists_already_shows_the_error_and_changes_nothing() {
        let mut app = app("new-exists");
        press(&mut app, KeyCode::Char('n'));
        type_text(&mut app, "a");
        press(&mut app, KeyCode::Enter);
        assert!(screen(&mut app).contains("a.typ exists already"));
        assert_eq!(
            fs::read_to_string(app.root.join("a.typ")).unwrap(),
            "text of a\n"
        );
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn backspace_edits_the_name() {
        let mut app = app("new-backspace");
        press(&mut app, KeyCode::Char('n'));
        type_text(&mut app, "abc");
        press(&mut app, KeyCode::Backspace);
        press(&mut app, KeyCode::Enter);
        assert!(app.root.join("ab.typ").is_file());
        assert!(!app.root.join("abc.typ").exists());
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn n_makes_the_first_file_in_an_empty_folder() {
        let root =
            std::env::temp_dir().join(format!("lazytypst-main-new-empty-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let mut app = App::new(root.clone(), Vec::new(), Picker::halfblocks());
        press(&mut app, KeyCode::Char('n'));
        type_text(&mut app, "first");
        press(&mut app, KeyCode::Enter);
        assert!(root.join("first.typ").is_file());
        assert!(app.editor.is_some());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn n_types_a_letter_in_the_editor() {
        let mut app = app("new-editor");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('n'));
        let text = screen(&mut app);
        assert!(
            text.contains("ntext of a") && !text.contains("New file:"),
            "{text}"
        );
        fs::remove_dir_all(&app.root).unwrap();
    }

    /// An app with three files: `a.typ`, `b.typ`, and `c.typ`. Each file holds `text of <name>`.
    fn app_with_three_files(name: &str) -> App {
        let root = std::env::temp_dir().join(format!(
            "lazytypst-main-three-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        for file in ["a", "b", "c"] {
            fs::write(
                root.join(format!("{file}.typ")),
                format!("text of {file}\n"),
            )
            .unwrap();
        }
        let files = browser::find_typ_files(&root, browser::MAX_DEPTH).unwrap();
        App::new(root, files, Picker::halfblocks())
    }

    #[test]
    fn capital_g_then_enter_opens_the_last_file_and_g_then_enter_opens_the_first() {
        let mut app = app_with_three_files("g");
        press(&mut app, KeyCode::Char('G'));
        press(&mut app, KeyCode::Enter); // no draw between the keys
        assert!(
            screen(&mut app).contains("text of c"),
            "G must select the last file"
        );
        press(&mut app, KeyCode::Esc);

        press(&mut app, KeyCode::Char('g'));
        press(&mut app, KeyCode::Enter);
        assert!(
            screen(&mut app).contains("text of a"),
            "g must select the first file"
        );
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn end_and_home_do_the_same_as_capital_g_and_g() {
        let mut app = app_with_three_files("home");
        press(&mut app, KeyCode::End);
        press(&mut app, KeyCode::Enter);
        assert!(screen(&mut app).contains("text of c"));
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Home);
        press(&mut app, KeyCode::Enter);
        assert!(screen(&mut app).contains("text of a"));
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn g_and_capital_g_select_from_the_middle_of_the_list() {
        let mut app = app_with_three_files("middle");
        press(&mut app, KeyCode::Char('j')); // b.typ
        screen(&mut app);
        press(&mut app, KeyCode::Char('G'));
        assert_eq!(app.list.selected(), Some(2));
        press(&mut app, KeyCode::Char('g'));
        assert_eq!(app.list.selected(), Some(0));
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn g_and_capital_g_in_an_empty_list_do_nothing() {
        let root =
            std::env::temp_dir().join(format!("lazytypst-main-g-empty-{}", std::process::id()));
        let mut app = App::new(root, Vec::new(), Picker::halfblocks());
        for code in [
            KeyCode::Char('g'),
            KeyCode::Char('G'),
            KeyCode::Home,
            KeyCode::End,
            KeyCode::Enter,
        ] {
            assert!(!press(&mut app, code));
        }
        assert_eq!(app.list.selected(), None);
        assert!(app.editor.is_none());
        assert!(screen(&mut app).contains("No .typ files"));
    }

    #[test]
    fn g_types_a_letter_in_the_editor_and_in_the_new_file_prompt() {
        let mut app = app_with_three_files("g-types");
        press(&mut app, KeyCode::Char('n'));
        type_text(&mut app, "gG");
        assert!(screen(&mut app).contains("New file: gG"));
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('G'));
        assert!(screen(&mut app).contains("Gtext of a"));
        fs::remove_dir_all(&app.root).unwrap();
    }

    /// The last row of the screen: the status line.
    fn status_row(app: &mut App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..100)
            .map(|column| buffer[(column, 23)].symbol())
            .collect()
    }

    /// An app with the files `long.typ` (10 lines) and `other.typ` (2 lines).
    fn app_with_long_file(name: &str) -> App {
        let root = std::env::temp_dir().join(format!(
            "lazytypst-main-cursor-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let long: String = (1..=10).map(|n| format!("line number {n}\n")).collect();
        fs::write(root.join("long.typ"), long).unwrap();
        fs::write(root.join("other.typ"), "first\nsecond\n").unwrap();
        let files = browser::find_typ_files(&root, browser::MAX_DEPTH).unwrap();
        App::new(root, files, Picker::halfblocks())
    }

    #[test]
    fn a_file_opens_again_with_the_cursor_where_the_user_left_it() {
        let mut app = app_with_long_file("again");
        press(&mut app, KeyCode::Enter); // long.typ
        for _ in 0..9 {
            press(&mut app, KeyCode::Down);
        }
        for _ in 0..5 {
            press(&mut app, KeyCode::Right);
        }
        assert!(
            status_row(&mut app).trim_end().ends_with("10:6"),
            "{:?}",
            status_row(&mut app)
        );
        press(&mut app, KeyCode::Esc);

        press(&mut app, KeyCode::Enter); // the same file again
        assert!(
            status_row(&mut app).trim_end().ends_with("10:6"),
            "{:?}",
            status_row(&mut app)
        );
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn another_file_opens_at_line_1() {
        let mut app = app_with_long_file("other");
        press(&mut app, KeyCode::Enter);
        for _ in 0..9 {
            press(&mut app, KeyCode::Down);
        }
        press(&mut app, KeyCode::Esc);

        press(&mut app, KeyCode::Down); // other.typ
        press(&mut app, KeyCode::Enter);
        assert!(
            status_row(&mut app).trim_end().ends_with("1:1"),
            "{:?}",
            status_row(&mut app)
        );
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn each_file_keeps_its_own_position() {
        let mut app = app_with_long_file("each");
        press(&mut app, KeyCode::Enter); // long.typ: line 4
        for _ in 0..3 {
            press(&mut app, KeyCode::Down);
        }
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter); // other.typ: line 2
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Esc);

        press(&mut app, KeyCode::Up);
        press(&mut app, KeyCode::Enter);
        assert!(
            status_row(&mut app).trim_end().ends_with("4:1"),
            "{:?}",
            status_row(&mut app)
        );
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        assert!(
            status_row(&mut app).trim_end().ends_with("2:1"),
            "{:?}",
            status_row(&mut app)
        );
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn a_file_that_got_shorter_opens_on_its_last_line() {
        let mut app = app_with_long_file("shorter");
        press(&mut app, KeyCode::Enter);
        for _ in 0..9 {
            press(&mut app, KeyCode::Down);
        }
        for _ in 0..8 {
            press(&mut app, KeyCode::Right);
        }
        press(&mut app, KeyCode::Esc);
        fs::write(app.root.join("long.typ"), "one\ntwo\nthree\n").unwrap();

        press(&mut app, KeyCode::Enter);
        // The text has 3 lines and the last line has 5 characters. The old column 8 is cut to the end.
        assert!(
            status_row(&mut app).trim_end().ends_with("3:6"),
            "{:?}",
            status_row(&mut app)
        );
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn the_position_is_kept_in_memory_only() {
        let mut first = app_with_long_file("memory");
        press(&mut first, KeyCode::Enter);
        for _ in 0..6 {
            press(&mut first, KeyCode::Down);
        }
        press(&mut first, KeyCode::Esc);

        let mut second = App::new(
            first.root.clone(),
            first.files.clone(),
            Picker::halfblocks(),
        );
        press(&mut second, KeyCode::Enter);
        assert!(
            status_row(&mut second).trim_end().ends_with("1:1"),
            "a new start begins at line 1"
        );
        fs::remove_dir_all(&first.root).unwrap();
    }

    /// An app with four files: `README.typ`, `chapters/one.typ`, `chapters/two.typ`, and `notes/draft.typ`.
    fn app_for_filter(name: &str) -> App {
        let root = std::env::temp_dir().join(format!(
            "lazytypst-main-filter-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        for file in [
            "README.typ",
            "chapters/one.typ",
            "chapters/two.typ",
            "notes/draft.typ",
            "Épilogue.typ",
        ] {
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, format!("text of {file}\n")).unwrap();
        }
        let files = browser::find_typ_files(&root, browser::MAX_DEPTH).unwrap();
        App::new(root, files, Picker::halfblocks())
    }

    /// The names that the list shows, from the screen text: the rows of the list inside its border.
    fn listed(app: &mut App) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        let buffer = terminal.backend().buffer();
        (1..20)
            .map(|row| {
                (1..99)
                    .map(|column| buffer[(column, row)].symbol())
                    .collect::<String>()
            })
            .map(|row| row.trim().trim_start_matches('>').trim().to_string())
            .filter(|row| row.ends_with(".typ") || row.contains(".typ ["))
            .collect()
    }

    #[test]
    fn slash_opens_the_filter_prompt_and_typing_filters_the_list_at_once() {
        let mut app = app_for_filter("type");
        assert_eq!(listed(&mut app).len(), 5);
        press(&mut app, KeyCode::Char('/'));
        assert!(screen(&mut app).contains("Filter:"));
        type_text(&mut app, "chap");
        assert_eq!(listed(&mut app), ["chapters/one.typ", "chapters/two.typ"]);
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn the_filter_ignores_case_and_matches_the_whole_path() {
        let mut app = app_for_filter("case");
        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, "READme");
        assert_eq!(listed(&mut app), ["README.typ"]);
        for _ in 0..6 {
            press(&mut app, KeyCode::Backspace);
        }
        type_text(&mut app, "CHAPTERS/ONE");
        assert_eq!(listed(&mut app), ["chapters/one.typ"]);
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn the_filter_ignores_case_for_letters_with_accents() {
        let mut app = app_for_filter("accent");
        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, "épi");
        assert_eq!(listed(&mut app), ["Épilogue.typ"]);
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn esc_in_the_prompt_shows_all_files_again() {
        let mut app = app_for_filter("esc");
        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, "chap");
        press(&mut app, KeyCode::Esc);
        assert_eq!(listed(&mut app).len(), 5);
        let text = screen(&mut app);
        assert!(
            !text.contains("Filter:") && !text.contains("lazytypst /"),
            "{text}"
        );
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn enter_in_the_prompt_keeps_the_filter_and_the_title_shows_it() {
        let mut app = app_for_filter("enter");
        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, "chap");
        press(&mut app, KeyCode::Enter);
        let text = screen(&mut app);
        assert!(!text.contains("Filter:"), "the prompt must close: {text}");
        assert!(text.contains("lazytypst /chap"), "{text}");
        assert_eq!(listed(&mut app), ["chapters/one.typ", "chapters/two.typ"]);
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn enter_on_a_filtered_list_opens_the_selected_file() {
        let mut app = app_for_filter("open");
        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, "two");
        press(&mut app, KeyCode::Enter); // keeps the filter
        press(&mut app, KeyCode::Enter); // opens the first match
        assert!(screen(&mut app).contains("text of chapters/two.typ"));
        press(&mut app, KeyCode::Esc);
        assert!(
            screen(&mut app).contains("lazytypst /two"),
            "the filter stays after the editor closes"
        );
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn a_filter_with_no_match_shows_no_match_and_enter_does_nothing() {
        let mut app = app_for_filter("nomatch");
        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, "zzz");
        let text = screen(&mut app);
        assert!(
            text.contains("No match") && !text.contains("No .typ files"),
            "{text}"
        );
        press(&mut app, KeyCode::Enter);
        for code in [
            KeyCode::Enter,
            KeyCode::Char('g'),
            KeyCode::Char('G'),
            KeyCode::Down,
            KeyCode::Char('m'),
        ] {
            assert!(!press(&mut app, code));
        }
        assert!(app.editor.is_none());
        assert_eq!(app.list.selected(), None);
        assert_eq!(app.main_file, None);
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn the_prompt_takes_every_key_while_the_user_filters() {
        let mut app = app_for_filter("keys");
        press(&mut app, KeyCode::Char('/'));
        for letter in "qjkgGmrn".chars() {
            assert!(
                !press(&mut app, KeyCode::Char(letter)),
                "{letter} quit the program"
            );
        }
        assert!(screen(&mut app).contains("Filter: qjkgGmrn"));
        assert_eq!(app.main_file, None);
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn the_selection_stays_on_the_same_file_while_the_filter_still_shows_it() {
        let mut app = app_for_filter("selected");
        press(&mut app, KeyCode::Char('G')); // the last file
        let last = app.selected_file().cloned().unwrap();
        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, &last.to_string_lossy()[..3].to_lowercase());
        assert_eq!(
            app.selected_file(),
            Some(&last),
            "the selection moved to another file"
        );
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn slash_again_starts_with_the_current_filter_and_esc_clears_it() {
        let mut app = app_for_filter("again");
        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, "chap");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('/'));
        assert!(
            screen(&mut app).contains("Filter: chap"),
            "the prompt must start with the filter"
        );
        press(&mut app, KeyCode::Char('s'));
        assert_eq!(
            listed(&mut app),
            Vec::<String>::new(),
            "chaps matches nothing"
        );
        press(&mut app, KeyCode::Esc);
        assert_eq!(listed(&mut app).len(), 5, "Esc removes the whole filter");
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn esc_in_the_list_removes_a_filter_that_is_on() {
        let mut app = app_for_filter("esclist");
        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, "chap");
        press(&mut app, KeyCode::Enter);
        assert_eq!(listed(&mut app).len(), 2);
        assert!(!press(&mut app, KeyCode::Esc));
        assert_eq!(listed(&mut app).len(), 5);
        assert!(
            !press(&mut app, KeyCode::Esc),
            "Esc with no filter does nothing"
        );
        assert_eq!(listed(&mut app).len(), 5);
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn r_keeps_the_filter_and_finds_new_files_that_match() {
        let mut app = app_for_filter("refresh");
        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, "chap");
        press(&mut app, KeyCode::Enter);
        fs::write(app.root.join("chapters").join("three.typ"), "").unwrap();
        fs::write(app.root.join("other.typ"), "").unwrap();
        press(&mut app, KeyCode::Char('r'));
        assert_eq!(
            listed(&mut app),
            ["chapters/one.typ", "chapters/three.typ", "chapters/two.typ"]
        );
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn a_new_file_removes_the_filter_so_that_it_shows_in_the_list() {
        let mut app = app_for_filter("newfile");
        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, "chap");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('n'));
        type_text(&mut app, "brand-new");
        press(&mut app, KeyCode::Enter);
        assert!(app.root.join("brand-new.typ").is_file());
        assert!(app.editor.is_some(), "the new file must open");
        press(&mut app, KeyCode::Esc);
        let names = listed(&mut app);
        assert!(
            names.contains(&"brand-new.typ".to_string()) && names.len() == 6,
            "{names:?}"
        );
        assert_eq!(app.selected_file(), Some(&PathBuf::from("brand-new.typ")));
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn the_main_file_mark_shows_in_a_filtered_list_and_a_hidden_main_file_stays_the_main_file() {
        let mut app = app_for_filter("main");
        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, "readme");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('m'));
        assert_eq!(listed(&mut app), ["README.typ [main]"]);

        press(&mut app, KeyCode::Char('/'));
        for _ in 0..6 {
            press(&mut app, KeyCode::Backspace);
        }
        type_text(&mut app, "chap");
        press(&mut app, KeyCode::Enter);
        assert_eq!(
            app.main_file,
            Some(PathBuf::from("README.typ")),
            "a hidden main file is still the main file"
        );
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn slash_types_a_letter_in_the_editor_and_in_the_new_file_prompt() {
        let mut app = app_for_filter("slashtypes");
        press(&mut app, KeyCode::Char('n'));
        press(&mut app, KeyCode::Char('/'));
        assert!(screen(&mut app).contains("New file: /"));
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('/'));
        assert!(
            screen(&mut app).contains("/text of"),
            "the editor must type the slash"
        );
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn the_title_in_the_list_names_the_folder() {
        assert_eq!(
            window_title(Path::new("/home/me/books/novel"), None),
            "lazytypst: novel"
        );
        assert_eq!(window_title(Path::new("/"), None), "lazytypst: /");
    }

    #[test]
    fn the_title_in_the_editor_names_the_file_relative_to_the_root() {
        let root = Path::new("/home/me/books/novel");
        assert_eq!(
            window_title(root, Some(&root.join("report.typ"))),
            "lazytypst: report.typ"
        );
        assert_eq!(
            window_title(root, Some(&root.join("chapters/one.typ"))),
            "lazytypst: chapters/one.typ"
        );
        // A file outside the root keeps its whole path.
        assert_eq!(
            window_title(root, Some(Path::new("/elsewhere/x.typ"))),
            "lazytypst: /elsewhere/x.typ"
        );
    }

    #[test]
    fn control_characters_in_a_name_never_reach_the_terminal() {
        let root = Path::new("/home/me/evil\x1b]0;PWNED\x07folder");
        let title = window_title(root, None);
        assert!(!title.chars().any(char::is_control), "{title:?}");
        assert_eq!(title, "lazytypst: evil?]0;PWNED?folder");

        let file = root.join("a\nb\u{9b}c\x7f.typ"); // a line break, a C1 control, and DEL
        let title = window_title(root, Some(&file));
        assert!(!title.chars().any(char::is_control), "{title:?}");
    }

    #[test]
    fn a_name_with_letters_that_are_not_ascii_stays_as_it_is() {
        assert_eq!(
            window_title(Path::new("/books/संस्कृतम्"), None),
            "lazytypst: संस्कृतम्"
        );
        assert_eq!(
            window_title(Path::new("/books/é"), Some(Path::new("/books/é/中文.typ"))),
            "lazytypst: 中文.typ"
        );
    }

    #[test]
    fn the_title_is_sent_as_osc_0_and_the_title_stack_commands_are_the_xterm_ones() {
        use ratatui::crossterm::Command;
        let mut bytes = String::new();
        ratatui::crossterm::terminal::SetTitle("lazytypst: a")
            .write_ansi(&mut bytes)
            .unwrap();
        assert_eq!(bytes, "\x1b]0;lazytypst: a\x07");
        assert_eq!(SAVE_TITLE, "\x1b[22;0t");
        assert_eq!(RESTORE_TITLE, "\x1b[23;0t");
    }

    #[test]
    fn the_changelog_has_an_unreleased_entry_first_and_an_entry_for_the_current_version() {
        let changelog = include_str!("../CHANGELOG.md");
        let unreleased = changelog
            .find("\n## [Unreleased]\n")
            .expect("no Unreleased entry");
        let version = env!("CARGO_PKG_VERSION");
        let current = changelog
            .find(&format!("\n## [{version}]"))
            .unwrap_or_else(|| panic!("no entry for the version {version} in CHANGELOG.md"));
        assert!(
            unreleased < current,
            "the Unreleased entry must come before the entry for {version}"
        );
    }

    #[test]
    fn m_in_an_empty_list_does_nothing() {
        let root =
            std::env::temp_dir().join(format!("lazytypst-main-emptymark-{}", std::process::id()));
        let mut app = App::new(root, Vec::new(), Picker::halfblocks());
        press(&mut app, KeyCode::Char('m'));
        assert_eq!(app.main_file, None);
    }

    #[test]
    fn q_quits_in_the_list_but_types_in_the_editor() {
        let mut app = app("quit");
        press(&mut app, KeyCode::Enter);
        assert!(
            !press(&mut app, KeyCode::Char('q')),
            "q in the editor must not quit"
        );
        assert!(screen(&mut app).contains("qtext of a"));
        press(&mut app, KeyCode::Esc);
        assert!(app.editor.is_none(), "Esc must save and close");
        assert!(press(&mut app, KeyCode::Char('q')));
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn ctrl_q_in_the_editor_saves_and_quits_the_program() {
        let mut app = app("ctrlq");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('X'));
        let quit = app.handle_key(KeyEvent::new(
            KeyCode::Char('q'),
            ratatui::crossterm::event::KeyModifiers::CONTROL,
        ));
        assert!(quit, "Ctrl-Q must quit the program");
        let text = fs::read_to_string(app.root.join("a.typ")).unwrap();
        assert!(text.starts_with("Xtext of a"), "{text}");
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn a_paste_goes_to_the_editor_and_the_list_ignores_it() {
        let mut app = app("paste");
        app.handle_paste("x\ny");
        assert!(app.editor.is_none());
        assert!(app.status.is_empty(), "{}", app.status);
        press(&mut app, KeyCode::Enter);
        app.handle_paste("P\nQ");
        assert!(screen(&mut app).contains("PQ") || screen(&mut app).contains("P"));
        press(&mut app, KeyCode::Esc);
        let text = fs::read_to_string(app.root.join("a.typ")).unwrap();
        assert!(text.starts_with("P\nQtext of a"), "{text}");
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn a_file_that_cannot_be_read_shows_an_error_in_the_list() {
        let mut app = app("unreadable");
        fs::write(app.root.join("a.typ"), b"\xff\xfe").unwrap();
        press(&mut app, KeyCode::Enter);
        assert!(app.editor.is_none());
        assert!(screen(&mut app).contains("Cannot open a.typ"));
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn an_empty_folder_shows_a_message_and_enter_does_nothing() {
        let root =
            std::env::temp_dir().join(format!("lazytypst-main-empty-{}", std::process::id()));
        let mut app = App::new(root, Vec::new(), Picker::halfblocks());
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Enter);
        assert!(app.editor.is_none());
        assert!(screen(&mut app).contains("No .typ files"));
    }

    #[test]
    fn the_reason_for_a_missing_program_says_not_found_in_path() {
        let missing = std::io::Error::from(std::io::ErrorKind::NotFound);
        assert_eq!(typst_problem(&missing), "not found in PATH");
        let other = std::io::Error::other("typst --version failed: exit status: 3");
        assert_eq!(
            typst_problem(&other),
            "typst --version failed: exit status: 3"
        );
    }

    #[test]
    fn the_version_text_has_the_two_versions() {
        let text = version_text("0.1.0", Ok("typst 0.15.1 (9dfd3a08)".into()));
        assert_eq!(text, "lazytypst 0.1.0\ntypst 0.15.1 (9dfd3a08)\n");
    }

    #[test]
    fn the_version_text_says_that_typst_is_missing() {
        let text = version_text(
            "0.1.0",
            Err(std::io::Error::from(std::io::ErrorKind::NotFound)),
        );
        assert_eq!(text, "lazytypst 0.1.0\ntypst: not found in PATH\n");
    }

    #[test]
    fn the_version_text_gives_the_reason_when_typst_fails() {
        let text = version_text(
            "0.1.0",
            Err(std::io::Error::other(
                "typst --version failed: exit status: 3",
            )),
        );
        assert_eq!(
            text,
            "lazytypst 0.1.0\ntypst: typst --version failed: exit status: 3\n"
        );
    }

    #[test]
    fn the_start_message_names_the_command_the_reason_and_the_install_page() {
        let message = missing_typst_message(&std::io::Error::from(std::io::ErrorKind::NotFound));
        assert!(message.contains("typst"), "{message}");
        assert!(message.contains("not found in PATH"), "{message}");
        assert!(
            message.contains("https://github.com/typst/typst#installation"),
            "{message}"
        );
        assert!(message.ends_with('\n'));
    }

    /// A new folder for the tests of `resolve_target`.
    fn target_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("lazytypst-target-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::canonicalize(&dir).unwrap()
    }

    #[test]
    fn a_folder_argument_is_the_root_and_opens_no_file() {
        let dir = target_dir("folder");
        let target = resolve_target(&dir).unwrap();
        assert_eq!(
            target,
            Target {
                root: dir.clone(),
                open: None
            }
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_typ_file_argument_opens_the_file_and_its_folder_is_the_root() {
        let dir = target_dir("file");
        fs::create_dir_all(dir.join("chapters")).unwrap();
        fs::write(dir.join("chapters").join("one.typ"), "= One\n").unwrap();
        let target = resolve_target(&dir.join("chapters").join("one.typ")).unwrap();
        assert_eq!(
            target,
            Target {
                root: dir.join("chapters"),
                open: Some(PathBuf::from("one.typ"))
            }
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_symlink_to_a_typ_file_uses_the_folder_of_the_link() {
        let dir = target_dir("link");
        fs::create_dir_all(dir.join("real")).unwrap();
        fs::write(dir.join("real").join("real.typ"), "").unwrap();
        std::os::unix::fs::symlink(dir.join("real").join("real.typ"), dir.join("link.typ"))
            .unwrap();
        let target = resolve_target(&dir.join("link.typ")).unwrap();
        assert_eq!(
            target,
            Target {
                root: dir.clone(),
                open: Some(PathBuf::from("link.typ"))
            }
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_hidden_typ_file_and_a_folder_named_like_a_typ_file_work() {
        let dir = target_dir("odd");
        fs::write(dir.join(".draft.typ"), "").unwrap();
        fs::create_dir_all(dir.join("book.typ")).unwrap();
        assert_eq!(
            resolve_target(&dir.join(".draft.typ")).unwrap().open,
            Some(PathBuf::from(".draft.typ"))
        );
        let folder = resolve_target(&dir.join("book.typ")).unwrap();
        assert_eq!(
            folder,
            Target {
                root: dir.join("book.typ"),
                open: None
            }
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_file_that_is_not_a_typ_file_gives_exit_code_2() {
        let dir = target_dir("notes");
        for name in ["notes.txt", "x.TYP", "typ", "x.typ.bak"] {
            fs::write(dir.join(name), "").unwrap();
            let (message, code) = resolve_target(&dir.join(name)).unwrap_err();
            assert_eq!(code, 2, "{name}");
            assert!(
                message.contains(name) && message.contains(".typ"),
                "{message}"
            );
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_path_that_does_not_exist_gives_exit_code_1() {
        let (message, code) = resolve_target(Path::new("/no/such/path.typ")).unwrap_err();
        assert_eq!(code, 1);
        assert!(
            message.contains("Cannot open /no/such/path.typ"),
            "{message}"
        );
    }

    #[test]
    fn a_bare_file_name_has_the_current_folder_as_its_root() {
        // The file is in the folder of the test run, so the test makes it there and removes it.
        let name = format!("lazytypst-bare-{}.typ", std::process::id());
        fs::write(&name, "").unwrap();
        let target = resolve_target(Path::new(&name)).unwrap();
        fs::remove_file(&name).unwrap();
        assert_eq!(target.root, fs::canonicalize(".").unwrap());
        assert_eq!(target.open, Some(PathBuf::from(&name)));
    }

    #[test]
    fn the_program_starts_with_the_file_open_and_esc_shows_the_list_of_its_folder() {
        let mut app = app_with_three_files("target");
        app.open_target(PathBuf::from("b.typ"));
        let text = screen(&mut app);
        assert!(
            text.contains("b.typ") && text.contains("text of b") && text.contains("Ctrl-S save"),
            "{text}"
        );

        press(&mut app, KeyCode::Esc);
        let text = screen(&mut app);
        assert!(text.contains("a.typ") && text.contains("c.typ"), "{text}");
        assert_eq!(
            app.list.selected(),
            Some(1),
            "the opened file must be selected in the list"
        );
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn a_file_that_the_list_does_not_show_still_opens() {
        let mut app = app_with_three_files("hidden-target");
        fs::write(app.root.join(".draft.typ"), "text of draft\n").unwrap();
        app.open_target(PathBuf::from(".draft.typ"));
        assert!(screen(&mut app).contains("text of draft"));
        press(&mut app, KeyCode::Esc);
        assert_eq!(
            app.list.selected(),
            Some(0),
            "the selection stays when the file is not in the list"
        );
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn a_file_that_cannot_be_read_shows_the_error_in_the_list() {
        let mut app = app_with_three_files("target-bad");
        fs::write(app.root.join("a.typ"), b"\xff\xfe").unwrap();
        app.open_target(PathBuf::from("a.typ"));
        assert!(app.editor.is_none());
        assert!(screen(&mut app).contains("Cannot open a.typ"));
        fs::remove_dir_all(&app.root).unwrap();
    }

    fn args(list: &[&str]) -> Result<Args, String> {
        parse_args(list.iter().map(OsString::from))
    }

    #[test]
    fn the_root_comes_from_the_first_argument_even_if_it_is_not_utf8() {
        let name = OsString::from_vec(b"dir-\xff".to_vec());
        let list = [OsString::from("lazytypst"), name.clone()];
        assert_eq!(
            parse_args(list.into_iter()),
            Ok(Args::Run(PathBuf::from(name)))
        );
    }

    #[test]
    fn the_root_is_the_current_folder_without_an_argument() {
        assert_eq!(args(&["lazytypst"]), Ok(Args::Run(PathBuf::from("."))));
    }

    #[test]
    fn help_and_version_have_a_long_and_a_short_form() {
        assert_eq!(args(&["lazytypst", "--help"]), Ok(Args::Help));
        assert_eq!(args(&["lazytypst", "-h"]), Ok(Args::Help));
        assert_eq!(args(&["lazytypst", "--version"]), Ok(Args::Version));
        assert_eq!(args(&["lazytypst", "-V"]), Ok(Args::Version));
    }

    #[test]
    fn an_unknown_option_and_a_second_folder_are_errors() {
        assert!(
            args(&["lazytypst", "--frobnicate"])
                .unwrap_err()
                .contains("--frobnicate")
        );
        assert!(
            args(&["lazytypst", "a", "b"])
                .unwrap_err()
                .contains("one folder")
        );
    }

    #[test]
    fn a_folder_that_starts_with_a_dash_works_with_a_dot_slash() {
        assert_eq!(
            args(&["lazytypst", "./-notes"]),
            Ok(Args::Run(PathBuf::from("./-notes")))
        );
    }
}
