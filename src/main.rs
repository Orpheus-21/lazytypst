mod browser;
mod compile;
mod editor;
mod fsutil;
mod newfile;
mod preview;
mod state;

use std::{
    ffi::OsString,
    path::PathBuf,
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
use ratatui_textarea::TextArea;

use editor::{Action, Editor};

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
    /// The prompt for the name of a new file. While it is open, it takes every key.
    prompt: Option<TextArea<'static>>,
    /// The open file. The browser shows while this is `None`.
    editor: Option<Editor>,
}

const USAGE: &str = "\
Usage: lazytypst [FOLDER]

lazytypst lists the .typ files in FOLDER and opens them in an editor with a live preview.
Without FOLDER, lazytypst uses the current folder. FOLDER is also the Typst project root.

Options:
  -h, --help     Show this help.
  -V, --version  Show the version of lazytypst and the version of typst.

Keys in the file list:
  j or Down      Select the next file.
  k or Up        Select the previous file.
  Enter          Open the selected file.
  n              Make a new .typ file. Type a path such as chapters/two. The ending .typ is added.
  r              Read the folder again, to show new files and to drop deleted files.
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
  Esc            Save the file and go back to the file list.
";

#[derive(Debug, PartialEq)]
enum Args {
    Run(PathBuf),
    Help,
    Version,
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
            files,
            main_file: None,
            state_file: None,
            list: ListState::default().with_selected(first),
            status: String::new(),
            prompt: None,
            editor: None,
        }
    }

    /// Sets the state file and takes the saved main file of this project. A saved file that is not in the list
    /// is ignored.
    fn load_state(&mut self, state_file: Option<PathBuf>) {
        self.main_file = state_file
            .as_deref()
            .and_then(|file| state::load_main(file, &self.root))
            .filter(|main| self.files.contains(main));
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
        let selected = self.selected_file().cloned();
        let index = selected.and_then(|path| files.iter().position(|file| *file == path));
        self.list.select((!files.is_empty()).then_some(index.unwrap_or(0)));
        self.status = format!("Read the folder again: {} files", files.len());
        self.files = files;
        if self.main_file.as_ref().is_some_and(|main| !self.files.contains(main)) {
            self.main_file = None;
            self.save_main_choice();
        }
    }

    /// Opens the selected file in the editor. A file that cannot be read shows an error in the list.
    fn open_selected(&mut self) {
        let Some(path) = self.selected_file().cloned() else {
            return;
        };
        let main = self.main_file.as_ref().map(|main| self.root.join(main));
        let opened = Editor::open(self.root.join(&path), self.root.clone(), main, self.picker.clone());
        self.status = match opened {
            Ok(editor) => {
                self.editor = Some(editor);
                String::new()
            }
            Err(err) => format!("Cannot open {}: {err}", path.display()),
        };
    }

    fn open_prompt(&mut self) {
        let mut prompt = TextArea::default();
        prompt.set_cursor_line_style(Style::default());
        self.prompt = Some(prompt);
        self.status = "Type a path such as chapters/two. Enter makes the file. Esc cancels.".into();
    }

    /// Handles a key while the prompt is open. Enter makes the file and opens it. If the name is not
    /// valid, the prompt stays open and the status line says why.
    fn handle_prompt_key(&mut self, key: KeyEvent) {
        let Some(prompt) = &mut self.prompt else {
            return;
        };
        match key.code {
            KeyCode::Esc => {
                self.prompt = None;
                self.status.clear();
            }
            KeyCode::Enter => {
                let name = prompt.lines().join("");
                match newfile::create(&self.root, &name, browser::MAX_DEPTH) {
                    Ok(made) => {
                        self.prompt = None;
                        self.refresh();
                        if let Some(index) = self.files.iter().position(|file| *file == made) {
                            self.list.select(Some(index));
                        }
                        self.open_selected();
                    }
                    Err(message) => self.status = message,
                }
            }
            _ => {
                prompt.input(key);
            }
        }
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

    /// Handles one key. Returns true when the program must quit.
    fn handle_key(&mut self, key: KeyEvent) -> bool {
        if let Some(editor) = &mut self.editor {
            if matches!(editor.handle_key(key), Action::Close) {
                self.editor = None;
            }
            return false;
        }
        if self.prompt.is_some() {
            self.handle_prompt_key(key);
            return false;
        }
        match key.code {
            KeyCode::Char('q') => return true,
            KeyCode::Char('j') | KeyCode::Down => self.list.select_next(),
            KeyCode::Char('k') | KeyCode::Up => self.list.select_previous(),
            KeyCode::Char('m') => self.toggle_main(),
            KeyCode::Char('r') => self.refresh(),
            KeyCode::Char('n') => self.open_prompt(),
            KeyCode::Enter => self.open_selected(),
            _ => {}
        }
        false
    }
}

fn main() -> std::io::Result<()> {
    let root = match parse_args(std::env::args_os()) {
        Ok(Args::Run(root)) => root,
        Ok(Args::Help) => {
            print!("{USAGE}");
            return Ok(());
        }
        Ok(Args::Version) => {
            print!("{}", version_text(env!("CARGO_PKG_VERSION"), compile::typst_version()));
            return Ok(());
        }
        Err(message) => {
            eprint!("{message}\n\n{USAGE}");
            std::process::exit(2);
        }
    };
    // An absolute root keeps every derived path valid when typst runs inside the root.
    let root = match std::fs::canonicalize(&root) {
        Ok(root) => root,
        Err(err) => {
            eprintln!("Cannot open the folder {}: {err}", root.display());
            std::process::exit(1);
        }
    };
    // Check typst before anything else changes: no temporary folder is made, and the terminal is not touched.
    if let Err(err) = compile::typst_version() {
        eprint!("{}", missing_typst_message(&err));
        std::process::exit(1);
    }
    let files = browser::find_typ_files(&root, browser::MAX_DEPTH)?;
    compile::make_out_dir().map_err(|err| {
        let message = format!("Cannot make the folder {}: {err}", compile::out_dir().display());
        std::io::Error::new(err.kind(), message)
    })?;
    compile::remove_stale_dirs();
    // ratatui::init also installs a panic hook that restores the terminal.
    let mut terminal = ratatui::init();
    // The panic hook of ratatui runs after this one, so a panic also deletes the page folder.
    let restore_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        compile::cleanup();
        restore_hook(info);
    }));
    // The query needs the raw terminal, and it must run before the first key is read.
    // It finds the image protocol and the font size. If it fails, the preview uses half blocks.
    let picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());
    let mut app = App::new(root, files, picker);
    app.load_state(state::default_state_file());
    let result = run(&mut terminal, &mut app);
    ratatui::restore();
    compile::cleanup();
    result
}

fn run(terminal: &mut DefaultTerminal, app: &mut App) -> std::io::Result<()> {
    let mut redraw = true;
    loop {
        if redraw {
            terminal.draw(|frame| draw(frame, app))?;
        }
        // While no key arrives, every 50 ms the editor checks its autosave and its compile.
        if !event::poll(Duration::from_millis(50))? {
            redraw = app.editor.as_mut().is_some_and(|editor| editor.tick(Instant::now()));
            continue;
        }
        redraw = true;
        let Event::Key(key) = event::read()? else { continue };
        if app.handle_key(key) {
            return Ok(());
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
    let block = Block::bordered().title("lazytypst");
    if app.files.is_empty() {
        frame.render_widget(Paragraph::new("No .typ files").block(block), body);
    } else {
        let items = app.files.iter().map(|path| {
            let mark = if app.main_file.as_ref() == Some(path) { " [main]" } else { "" };
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
        let [label, input] = Layout::horizontal([Constraint::Length(10), Constraint::Min(1)]).areas(prompt_row);
        frame.render_widget(Paragraph::new("New file: "), label);
        frame.render_widget(prompt, input);
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
        let root = std::env::temp_dir().join(format!("lazytypst-main-{name}-{}", std::process::id()));
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
        terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect()
    }

    #[test]
    fn the_list_shows_the_typ_files_only() {
        let mut app = app("list");
        let text = screen(&mut app);
        assert!(text.contains("a.typ") && text.contains("sub/b.typ"), "{text}");
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
        assert!(text.contains("sub/b.typ [main]") && !text.contains("a.typ [main]"), "{text}");
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
        let dir = std::env::temp_dir().join(format!("lazytypst-main-state-{name}-{}", std::process::id()));
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

        let mut second = App::new(first.root.clone(), first.files.clone(), Picker::halfblocks());
        second.load_state(Some(state.clone()));
        assert_eq!(second.main_file, Some(PathBuf::from("sub/b.typ")));
        assert!(screen(&mut second).contains("sub/b.typ [main]"));

        press(&mut second, KeyCode::Char('j'));
        press(&mut second, KeyCode::Char('m')); // sub/b.typ again: removes the mark
        let mut third = App::new(first.root.clone(), first.files.clone(), Picker::halfblocks());
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
        assert!(second.status.is_empty(), "no error for a missing main file: {}", second.status);
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
        assert_eq!(app.main_file, Some(PathBuf::from("a.typ")), "the mark must work without the state file");
        assert!(app.status.contains("Cannot save the main file"), "{}", app.status);
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
        assert!(screen(&mut app).contains("text of b"), "the selection moved to another file");
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
        assert!(!fs::read_to_string(&state).unwrap().contains("a.typ"), "the saved choice stays");
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
        assert!(!text.contains("New file:") && text.contains("a.typ"), "{text}");
        assert!(!app.root.join("draft.typ").exists());
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn the_prompt_takes_every_key_before_the_list_does() {
        let mut app = app("new-keys");
        press(&mut app, KeyCode::Char('n'));
        for letter in "qjkmrn".chars() {
            assert!(!press(&mut app, KeyCode::Char(letter)), "{letter} quit the program");
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
        assert!(screen(&mut app).contains("chapters/two.typ"), "the editor must show the new file");
        press(&mut app, KeyCode::Esc);
        let text = screen(&mut app);
        assert!(text.contains("chapters/two.typ") && !text.contains("New file:"), "{text}");
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
        assert!(text.contains("inside the project") && text.contains("New file: ../x"), "{text}");
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
        assert_eq!(fs::read_to_string(app.root.join("a.typ")).unwrap(), "text of a\n");
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
        let root = std::env::temp_dir().join(format!("lazytypst-main-new-empty-{}", std::process::id()));
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
        assert!(text.contains("ntext of a") && !text.contains("New file:"), "{text}");
        fs::remove_dir_all(&app.root).unwrap();
    }

    #[test]
    fn m_in_an_empty_list_does_nothing() {
        let root = std::env::temp_dir().join(format!("lazytypst-main-emptymark-{}", std::process::id()));
        let mut app = App::new(root, Vec::new(), Picker::halfblocks());
        press(&mut app, KeyCode::Char('m'));
        assert_eq!(app.main_file, None);
    }

    #[test]
    fn q_quits_in_the_list_but_types_in_the_editor() {
        let mut app = app("quit");
        press(&mut app, KeyCode::Enter);
        assert!(!press(&mut app, KeyCode::Char('q')), "q in the editor must not quit");
        assert!(screen(&mut app).contains("qtext of a"));
        press(&mut app, KeyCode::Esc);
        assert!(app.editor.is_none(), "Esc must save and close");
        assert!(press(&mut app, KeyCode::Char('q')));
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
        let root = std::env::temp_dir().join(format!("lazytypst-main-empty-{}", std::process::id()));
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
        assert_eq!(typst_problem(&other), "typst --version failed: exit status: 3");
    }

    #[test]
    fn the_version_text_has_the_two_versions() {
        let text = version_text("0.1.0", Ok("typst 0.15.1 (9dfd3a08)".into()));
        assert_eq!(text, "lazytypst 0.1.0\ntypst 0.15.1 (9dfd3a08)\n");
    }

    #[test]
    fn the_version_text_says_that_typst_is_missing() {
        let text = version_text("0.1.0", Err(std::io::Error::from(std::io::ErrorKind::NotFound)));
        assert_eq!(text, "lazytypst 0.1.0\ntypst: not found in PATH\n");
    }

    #[test]
    fn the_version_text_gives_the_reason_when_typst_fails() {
        let text = version_text("0.1.0", Err(std::io::Error::other("typst --version failed: exit status: 3")));
        assert_eq!(text, "lazytypst 0.1.0\ntypst: typst --version failed: exit status: 3\n");
    }

    #[test]
    fn the_start_message_names_the_command_the_reason_and_the_install_page() {
        let message = missing_typst_message(&std::io::Error::from(std::io::ErrorKind::NotFound));
        assert!(message.contains("typst"), "{message}");
        assert!(message.contains("not found in PATH"), "{message}");
        assert!(message.contains("https://github.com/typst/typst#installation"), "{message}");
        assert!(message.ends_with('\n'));
    }

    fn args(list: &[&str]) -> Result<Args, String> {
        parse_args(list.iter().map(OsString::from))
    }

    #[test]
    fn the_root_comes_from_the_first_argument_even_if_it_is_not_utf8() {
        let name = OsString::from_vec(b"dir-\xff".to_vec());
        let list = [OsString::from("lazytypst"), name.clone()];
        assert_eq!(parse_args(list.into_iter()), Ok(Args::Run(PathBuf::from(name))));
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
        assert!(args(&["lazytypst", "--frobnicate"]).unwrap_err().contains("--frobnicate"));
        assert!(args(&["lazytypst", "a", "b"]).unwrap_err().contains("one folder"));
    }

    #[test]
    fn a_folder_that_starts_with_a_dash_works_with_a_dot_slash() {
        assert_eq!(args(&["lazytypst", "./-notes"]), Ok(Args::Run(PathBuf::from("./-notes"))));
    }
}
