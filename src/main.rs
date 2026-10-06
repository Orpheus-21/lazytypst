mod browser;
mod compile;
mod editor;
mod preview;

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

use editor::{Action, Editor};

struct App {
    root: PathBuf,
    picker: Picker,
    files: Vec<PathBuf>,
    list: ListState,
    status: String,
    /// The open file. The browser shows while this is `None`.
    editor: Option<Editor>,
}

const USAGE: &str = "\
Usage: lazytypst [FOLDER]

lazytypst lists the .typ files in FOLDER and opens them in an editor with a live preview.
Without FOLDER, lazytypst uses the current folder. FOLDER is also the Typst project root.

Options:
  -h, --help     Show this help.
  -V, --version  Show the version.

Keys in the file list:
  j or Down      Select the next file.
  k or Up        Select the previous file.
  Enter          Open the selected file.
  q              Quit.

Keys in the editor:
  Ctrl-S         Save the file.
  Ctrl-B         Save and compile the file.
  Ctrl-E         Save the file and export a PDF next to it.
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
            list: ListState::default().with_selected(first),
            status: String::new(),
            editor: None,
        }
    }

    /// Handles one key. Returns true when the program must quit.
    fn handle_key(&mut self, key: KeyEvent) -> bool {
        if let Some(editor) = &mut self.editor {
            if matches!(editor.handle_key(key), Action::Close) {
                self.editor = None;
            }
            return false;
        }
        match key.code {
            KeyCode::Char('q') => return true,
            KeyCode::Char('j') | KeyCode::Down => self.list.select_next(),
            KeyCode::Char('k') | KeyCode::Up => self.list.select_previous(),
            KeyCode::Enter => {
                if let Some(path) = self.list.selected().and_then(|i| self.files.get(i)) {
                    let opened = Editor::open(self.root.join(path), self.root.clone(), None, self.picker.clone());
                    self.status = match opened {
                        Ok(editor) => {
                            self.editor = Some(editor);
                            String::new()
                        }
                        Err(err) => format!("Cannot open {}: {err}", path.display()),
                    };
                }
            }
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
            println!("lazytypst {}", env!("CARGO_PKG_VERSION"));
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
    let [body, status] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());
    let block = Block::bordered().title("lazytypst");
    if app.files.is_empty() {
        frame.render_widget(Paragraph::new("No .typ files").block(block), body);
    } else {
        let items = app.files.iter().map(|path| path.display().to_string());
        let list = List::new(items)
            .block(block)
            .highlight_style(Style::new().add_modifier(Modifier::REVERSED))
            .highlight_symbol("> ");
        frame.render_stateful_widget(list, body, &mut app.list);
    }
    frame.render_widget(Paragraph::new(app.status.as_str()), status);
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
