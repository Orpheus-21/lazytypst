use std::{
    fs,
    path::{Path, PathBuf},
};

use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Block, Paragraph},
};
use ratatui_image::{StatefulImage, picker::Picker, protocol::StatefulProtocol};

/// The pane that shows one compiled page as an image.
pub struct Preview {
    picker: Picker,
    /// The folder with the PNG pages that the pane shows. The preview owns the folder.
    dir: Option<PathBuf>,
    /// The number of pages in `dir`.
    count: usize,
    /// The shown page. Page 1 has the index 0.
    index: usize,
    /// The last page that loaded. It stays on screen when a later compile fails.
    page: Option<StatefulProtocol>,
}

impl Preview {
    pub fn new(picker: Picker) -> Self {
        Self { picker, dir: None, count: 0, index: 0, page: None }
    }

    /// Shows the new folder and deletes the old folder. The page number stays the same,
    /// or it moves to the last page if the new folder has fewer pages.
    /// If the page does not load, the new folder is deleted, the old page stays, and the error text comes back.
    pub fn load(&mut self, dir: PathBuf) -> Result<(), String> {
        let count = count_pages(&dir);
        let index = self.index.min(count.saturating_sub(1));
        match self.read(&dir, index) {
            Ok(page) => {
                self.page = Some(page);
                self.count = count;
                self.index = index;
                if let Some(old) = self.dir.replace(dir) {
                    let _ = fs::remove_dir_all(old);
                }
                Ok(())
            }
            Err(err) => {
                let _ = fs::remove_dir_all(&dir);
                Err(err)
            }
        }
    }

    /// Shows the next page or the previous page. At the first page and at the last page, nothing changes.
    pub fn turn(&mut self, forward: bool) -> Result<(), String> {
        let Some(dir) = &self.dir else {
            return Ok(());
        };
        let index = if forward {
            (self.index + 1).min(self.count - 1)
        } else {
            self.index.saturating_sub(1)
        };
        if index != self.index {
            self.page = Some(self.read(dir, index)?);
            self.index = index;
        }
        Ok(())
    }

    fn read(&self, dir: &Path, index: usize) -> Result<StatefulProtocol, String> {
        let path = dir.join(format!("page-{}.png", index + 1));
        let image = image::open(&path).map_err(|err| format!("Cannot load {}: {err}", path.display()))?;
        Ok(self.picker.new_resize_protocol(image))
    }

    #[cfg(test)]
    pub fn has_page(&self) -> bool {
        self.page.is_some()
    }

    pub fn draw(&mut self, frame: &mut Frame, area: Rect) {
        let title = match self.page {
            Some(_) => format!("Preview {}/{}", self.index + 1, self.count),
            None => "Preview".to_string(),
        };
        let block = Block::bordered().title(title);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        match &mut self.page {
            // ponytail: the image is resized and encoded on the UI thread, inside this call.
            // Measured in a release build: 13 ms for a page of 1191 by 1684 pixels.
            // Upgrade: ratatui_image::thread::ThreadProtocol if pages get much larger.
            Some(page) => frame.render_stateful_widget(StatefulImage::default(), inner, page),
            None => frame.render_widget(Paragraph::new("No preview yet."), inner),
        }
    }
}

/// Counts the files page-1.png, page-2.png, and so on, up to the first number that is missing.
fn count_pages(dir: &Path) -> usize {
    (1..)
        .take_while(|number| dir.join(format!("page-{number}.png")).is_file())
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend, style::Color};

    const RED: Color = Color::Rgb(255, 0, 0);
    const GREEN: Color = Color::Rgb(0, 255, 0);

    /// Makes a folder with `count` PNG files of 40 by 40 pixels, named page-1.png and so on.
    /// Page 1 is red and page 2 is green. All other pages are blue.
    fn pages(name: &str, count: usize) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lazytypst-preview-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for number in 1..=count {
            let rgb = match number {
                1 => [255, 0, 0, 255],
                2 => [0, 255, 0, 255],
                _ => [0, 0, 255, 255],
            };
            image::RgbaImage::from_pixel(40, 40, image::Rgba(rgb))
                .save(dir.join(format!("page-{number}.png")))
                .unwrap();
        }
        dir
    }

    fn red_pages(name: &str) -> PathBuf {
        pages(name, 1)
    }

    /// Draws the pane on a 30 by 10 screen. Returns the text and a flag for each color.
    fn draw(preview: &mut Preview) -> (String, bool, bool) {
        let mut terminal = Terminal::new(TestBackend::new(30, 10)).unwrap();
        terminal.draw(|frame| preview.draw(frame, frame.area())).unwrap();
        let cells = terminal.backend().buffer().content();
        let has = |color| cells.iter().any(|cell| cell.bg == color);
        (screen_text(&terminal), has(RED), has(GREEN))
    }

    fn screen_text(terminal: &Terminal<TestBackend>) -> String {
        terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect()
    }

    #[test]
    fn load_reads_page_1() {
        let dir = red_pages("load");
        let mut preview = Preview::new(Picker::halfblocks());
        assert!(!preview.has_page());
        preview.load(dir.clone()).unwrap();
        assert!(preview.has_page());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_new_load_deletes_the_old_folder() {
        let (first, second) = (red_pages("old"), red_pages("new"));
        let mut preview = Preview::new(Picker::halfblocks());
        preview.load(first.clone()).unwrap();
        preview.load(second.clone()).unwrap();
        assert!(!first.exists(), "the old folder is still there");
        assert!(second.exists());
        fs::remove_dir_all(second).unwrap();
    }

    #[test]
    fn a_failed_load_keeps_the_old_page_and_deletes_the_new_folder() {
        let good = red_pages("keep");
        let mut preview = Preview::new(Picker::halfblocks());
        preview.load(good.clone()).unwrap();

        let empty = std::env::temp_dir().join(format!("lazytypst-preview-empty-{}", std::process::id()));
        fs::create_dir_all(&empty).unwrap();
        let err = preview.load(empty.clone()).unwrap_err();
        assert!(err.contains("Cannot load"), "{err}");
        assert!(preview.has_page());
        assert!(good.exists(), "the folder of the shown page was deleted");
        assert!(!empty.exists(), "the folder of the failed load is still there");
        fs::remove_dir_all(good).unwrap();
    }

    #[test]
    fn a_file_that_is_not_a_png_is_an_error() {
        let dir = red_pages("bad");
        fs::write(dir.join("page-1.png"), "not a png").unwrap();
        let mut preview = Preview::new(Picker::halfblocks());
        assert!(preview.load(dir.clone()).is_err());
        assert!(!preview.has_page());
        assert!(!dir.exists());
    }

    #[test]
    fn the_pane_shows_a_text_before_the_first_page() {
        let mut terminal = Terminal::new(TestBackend::new(30, 10)).unwrap();
        let mut preview = Preview::new(Picker::halfblocks());
        terminal.draw(|frame| preview.draw(frame, frame.area())).unwrap();
        assert!(screen_text(&terminal).contains("No preview yet."));
    }

    #[test]
    fn the_title_shows_the_page_number_and_the_page_count() {
        let dir = pages("title", 3);
        let mut preview = Preview::new(Picker::halfblocks());
        assert!(draw(&mut preview).0.contains("Preview"));
        preview.load(dir.clone()).unwrap();
        assert!(draw(&mut preview).0.contains("Preview 1/3"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn turn_stops_at_the_first_and_the_last_page() {
        let dir = pages("turn", 3);
        let mut preview = Preview::new(Picker::halfblocks());
        preview.turn(true).unwrap(); // no folder yet: nothing happens
        preview.load(dir.clone()).unwrap();

        preview.turn(false).unwrap();
        assert!(draw(&mut preview).0.contains("Preview 1/3"));
        for _ in 0..5 {
            preview.turn(true).unwrap();
        }
        assert!(draw(&mut preview).0.contains("Preview 3/3"));
        preview.turn(false).unwrap();
        assert!(draw(&mut preview).0.contains("Preview 2/3"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn turn_changes_the_image() {
        let dir = pages("image", 3);
        let mut preview = Preview::new(Picker::halfblocks());
        preview.load(dir.clone()).unwrap();
        let (_, red, green) = draw(&mut preview);
        assert!(red && !green, "page 1 must be red");

        preview.turn(true).unwrap();
        let (_, red, green) = draw(&mut preview);
        assert!(green && !red, "page 2 must be green");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_new_load_keeps_the_page_number() {
        let (first, second) = (pages("keep-a", 3), pages("keep-b", 3));
        let mut preview = Preview::new(Picker::halfblocks());
        preview.load(first).unwrap();
        preview.turn(true).unwrap();
        preview.load(second.clone()).unwrap();
        assert!(draw(&mut preview).0.contains("Preview 2/3"));
        fs::remove_dir_all(second).unwrap();
    }

    #[test]
    fn a_new_load_with_fewer_pages_moves_to_the_last_page() {
        let (first, second) = (pages("fewer-a", 3), pages("fewer-b", 2));
        let mut preview = Preview::new(Picker::halfblocks());
        preview.load(first).unwrap();
        preview.turn(true).unwrap();
        preview.turn(true).unwrap();
        preview.load(second.clone()).unwrap();
        assert!(draw(&mut preview).0.contains("Preview 2/2"));
        fs::remove_dir_all(second).unwrap();
    }

    #[test]
    fn a_page_that_does_not_load_keeps_the_old_page() {
        let dir = pages("missing", 3);
        let mut preview = Preview::new(Picker::halfblocks());
        preview.load(dir.clone()).unwrap();
        fs::remove_file(dir.join("page-2.png")).unwrap();

        let err = preview.turn(true).unwrap_err();
        assert!(err.contains("page-2.png"), "{err}");
        assert!(draw(&mut preview).0.contains("Preview 1/3"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_pane_draws_the_page() {
        let dir = red_pages("draw");
        let mut terminal = Terminal::new(TestBackend::new(30, 10)).unwrap();
        let mut preview = Preview::new(Picker::halfblocks());
        preview.load(dir.clone()).unwrap();
        terminal.draw(|frame| preview.draw(frame, frame.area())).unwrap();

        let screen = screen_text(&terminal);
        let red = Color::Rgb(255, 0, 0);
        let cells = terminal.backend().buffer().content();
        assert!(cells.iter().any(|cell| cell.bg == red), "no red cell on screen");
        assert!(!screen.contains("No preview yet."));
        fs::remove_dir_all(dir).unwrap();
    }
}
