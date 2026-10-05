mod browser;

use std::path::PathBuf;

use ratatui::{
    DefaultTerminal, Frame,
    crossterm::event::{self, Event, KeyCode},
    layout::{Constraint, Layout},
    style::{Modifier, Style},
    widgets::{Block, List, ListState, Paragraph},
};

struct App {
    files: Vec<PathBuf>,
    list: ListState,
    status: String,
}

fn main() -> std::io::Result<()> {
    let root = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    let files = browser::find_typ_files(&root, browser::MAX_DEPTH)?;
    let first = (!files.is_empty()).then_some(0);
    let mut app = App {
        files,
        list: ListState::default().with_selected(first),
        status: String::new(),
    };
    // ratatui::init also installs a panic hook that restores the terminal.
    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app);
    ratatui::restore();
    result
}

fn run(terminal: &mut DefaultTerminal, app: &mut App) -> std::io::Result<()> {
    loop {
        terminal.draw(|frame| draw(frame, app))?;
        let Event::Key(key) = event::read()? else { continue };
        match key.code {
            KeyCode::Char('q') => return Ok(()),
            KeyCode::Char('j') | KeyCode::Down => app.list.select_next(),
            KeyCode::Char('k') | KeyCode::Up => app.list.select_previous(),
            KeyCode::Enter => {
                if let Some(path) = app.list.selected().and_then(|i| app.files.get(i)) {
                    // ponytail: Task 3 replaces this line with "open the editor".
                    app.status = format!("Selected: {}", path.display());
                }
            }
            _ => {}
        }
    }
}

fn draw(frame: &mut Frame, app: &mut App) {
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
