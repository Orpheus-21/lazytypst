use std::path::Path;

use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Block, Paragraph},
};
use ratatui_image::{StatefulImage, picker::Picker, protocol::StatefulProtocol};

/// The pane that shows one compiled page as an image.
pub struct Preview {
    picker: Picker,
    /// The last page that loaded. It stays on screen when a later compile fails.
    page: Option<StatefulProtocol>,
}

impl Preview {
    pub fn new(picker: Picker) -> Self {
        Self { picker, page: None }
    }

    /// Loads the PNG file. If the load fails, the old page stays and the error text comes back.
    pub fn load(&mut self, path: &Path) -> Result<(), String> {
        let image = image::open(path).map_err(|err| format!("Cannot load {}: {err}", path.display()))?;
        self.page = Some(self.picker.new_resize_protocol(image));
        Ok(())
    }

    #[cfg(test)]
    pub fn has_page(&self) -> bool {
        self.page.is_some()
    }

    pub fn draw(&mut self, frame: &mut Frame, area: Rect) {
        let block = Block::bordered().title("Preview");
        let inner = block.inner(area);
        frame.render_widget(block, area);
        match &mut self.page {
            // ponytail: the image is resized and encoded on the UI thread, inside this call.
            // Upgrade: ratatui_image::thread::ThreadProtocol if a large page makes typing slow.
            Some(page) => frame.render_stateful_widget(StatefulImage::default(), inner, page),
            None => frame.render_widget(Paragraph::new("No preview yet."), inner),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend, style::Color};
    use std::fs;

    /// Writes a solid red PNG of 40 by 40 pixels in a new temporary folder.
    fn red_png(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("lazytypst-preview-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("page-1.png");
        image::RgbaImage::from_pixel(40, 40, image::Rgba([255, 0, 0, 255]))
            .save(&path)
            .unwrap();
        path
    }

    fn screen_text(terminal: &Terminal<TestBackend>) -> String {
        terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect()
    }

    #[test]
    fn load_reads_a_png() {
        let path = red_png("load");
        let mut preview = Preview::new(Picker::halfblocks());
        assert!(!preview.has_page());
        preview.load(&path).unwrap();
        assert!(preview.has_page());
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_failed_load_keeps_the_old_page() {
        let path = red_png("keep");
        let mut preview = Preview::new(Picker::halfblocks());
        preview.load(&path).unwrap();

        let err = preview.load(&path.with_file_name("missing.png")).unwrap_err();
        assert!(err.contains("Cannot load"), "{err}");
        assert!(preview.has_page());
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_file_that_is_not_a_png_is_an_error() {
        let path = red_png("bad");
        fs::write(&path, "not a png").unwrap();
        let mut preview = Preview::new(Picker::halfblocks());
        assert!(preview.load(&path).is_err());
        assert!(!preview.has_page());
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn the_pane_shows_a_text_before_the_first_page() {
        let mut terminal = Terminal::new(TestBackend::new(30, 10)).unwrap();
        let mut preview = Preview::new(Picker::halfblocks());
        terminal.draw(|frame| preview.draw(frame, frame.area())).unwrap();
        assert!(screen_text(&terminal).contains("No preview yet."));
    }

    #[test]
    fn the_pane_draws_the_page() {
        let path = red_png("draw");
        let mut terminal = Terminal::new(TestBackend::new(30, 10)).unwrap();
        let mut preview = Preview::new(Picker::halfblocks());
        preview.load(&path).unwrap();
        terminal.draw(|frame| preview.draw(frame, frame.area())).unwrap();

        let screen = screen_text(&terminal);
        let red = Color::Rgb(255, 0, 0);
        let cells = terminal.backend().buffer().content();
        assert!(cells.iter().any(|cell| cell.bg == red), "no red cell on screen");
        assert!(!screen.contains("No preview yet."));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
