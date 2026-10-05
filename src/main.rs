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
    crossterm::event::{self, Event, KeyCode},
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

/// The folder to browse: the first argument, or the current folder.
/// `args_os` keeps a folder name that is not UTF-8. `args` would panic on it.
fn root_from_args(mut args: impl Iterator<Item = OsString>) -> PathBuf {
    args.nth(1).map_or_else(|| PathBuf::from("."), PathBuf::from)
}

fn main() -> std::io::Result<()> {
    let root = root_from_args(std::env::args_os());
    let files = browser::find_typ_files(&root, browser::MAX_DEPTH)?;
    let first = (!files.is_empty()).then_some(0);
    compile::make_out_dir().map_err(|err| {
        let message = format!("Cannot make the folder {}: {err}", compile::out_dir().display());
        std::io::Error::new(err.kind(), message)
    })?;
    // ratatui::init also installs a panic hook that restores the terminal.
    let mut terminal = ratatui::init();
    // The query needs the raw terminal, and it must run before the first key is read.
    // It finds the image protocol and the font size. If it fails, the preview uses half blocks.
    let picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());
    let mut app = App {
        root,
        picker,
        files,
        list: ListState::default().with_selected(first),
        status: String::new(),
        editor: None,
    };
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
        if let Some(editor) = &mut app.editor {
            if matches!(editor.handle_key(key), Action::Close) {
                app.editor = None;
            }
            continue;
        }
        match key.code {
            KeyCode::Char('q') => return Ok(()),
            KeyCode::Char('j') | KeyCode::Down => app.list.select_next(),
            KeyCode::Char('k') | KeyCode::Up => app.list.select_previous(),
            KeyCode::Enter => {
                if let Some(path) = app.list.selected().and_then(|i| app.files.get(i)) {
                    app.status = match Editor::open(app.root.join(path), app.root.clone(), app.picker.clone()) {
                        Ok(editor) => {
                            app.editor = Some(editor);
                            String::new()
                        }
                        Err(err) => format!("Cannot open {}: {err}", path.display()),
                    };
                }
            }
            _ => {}
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

    #[test]
    fn the_root_comes_from_the_first_argument_even_if_it_is_not_utf8() {
        let name = OsString::from_vec(b"dir-\xff".to_vec());
        let args = [OsString::from("lazytypst"), name.clone()];
        assert_eq!(root_from_args(args.into_iter()), PathBuf::from(name));
    }

    #[test]
    fn the_root_is_the_current_folder_without_an_argument() {
        assert_eq!(root_from_args([OsString::from("lazytypst")].into_iter()), PathBuf::from("."));
    }
}
