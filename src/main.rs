use ratatui::{
    DefaultTerminal,
    crossterm::event::{self, Event, KeyCode},
    widgets::{Block, Paragraph},
};

fn main() -> std::io::Result<()> {
    // ratatui::init also installs a panic hook that restores the terminal.
    let mut terminal = ratatui::init();
    let result = run(&mut terminal);
    ratatui::restore();
    result
}

fn run(terminal: &mut DefaultTerminal) -> std::io::Result<()> {
    loop {
        terminal.draw(|frame| {
            let screen = Paragraph::new("Press q to quit.")
                .block(Block::bordered().title("lazytypst"));
            frame.render_widget(screen, frame.area());
        })?;
        if let Event::Key(key) = event::read()?
            && key.code == KeyCode::Char('q')
        {
            return Ok(());
        }
    }
}
