use std::{
    fs,
    path::{Path, PathBuf},
};

use image::DynamicImage;
use ratatui::{
    Frame,
    layout::{Rect, Size},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Paragraph},
};
use ratatui_image::{Resize, StatefulImage, picker::Picker, protocol::StatefulProtocol};

/// The zoom steps in percent. Typst draws a page at 144 pixels per inch. At a zoom above 100, the
/// program asks Typst for a page with more pixels, so that the part on screen stays sharp.
const ZOOMS: [u32; 4] = [100, 150, 200, 300];
const DEFAULT_PPI: u32 = 144;
/// How far one arrow key moves the view, as a part of the way that the view can go.
const PAN_STEP: f32 = 0.25;

/// The part of an image that a zoom shows: `(x, y, width, height)` in pixels. `pan` is the place of
/// the view, from 0.0 (left or top) to 1.0 (right or bottom), along the way that the view can go.
fn crop_rect(width: u32, height: u32, zoom: u32, pan: (f32, f32)) -> (u32, u32, u32, u32) {
    let view = |size: u32| (size * 100 / zoom.max(100)).max(1);
    let (w, h) = (view(width), view(height));
    let place = |size: u32, view: u32, pan: f32| {
        ((size - view) as f32 * pan.clamp(0.0, 1.0)).round() as u32
    };
    (place(width, w, pan.0), place(height, h, pan.1), w, h)
}

/// The pane that shows one compiled page as an image.
///
/// A compile renders one page only. Its folder holds one file, `page-<p>-of-<t>.png`: `<p>` is the page
/// number and `<t>` is the number of pages of the document. So the preview learns the page count from
/// the name, without the other pages.
pub struct Preview {
    picker: Picker,
    /// The folder with the PNG file that the pane shows. The preview owns the folder.
    dir: Option<PathBuf>,
    /// The number of pages of the document.
    count: usize,
    /// The page on screen, counted from 1.
    shown: usize,
    /// The page that the next compile must render, counted from 1. It differs from `shown`
    /// between a page turn and the end of the compile that loads the new page.
    wanted: usize,
    /// The last page that loaded. It stays on screen when a later compile fails.
    page: Option<StatefulProtocol>,
    /// The picture of the last page that loaded, for the crop of a zoom.
    image: Option<DynamicImage>,
    /// The zoom, as an index into `ZOOMS`.
    zoom: usize,
    /// The place of the view in the page. See `crop_rect`.
    pan: (f32, f32),
    /// True while the last compile failed. The page on screen is then from an older, good compile.
    stale: bool,
}

impl Preview {
    pub fn new(picker: Picker) -> Self {
        Self {
            picker,
            dir: None,
            count: 0,
            shown: 1,
            wanted: 1,
            page: None,
            image: None,
            zoom: 0,
            pan: (0.0, 0.0),
            stale: false,
        }
    }

    /// The page that the next compile must render, counted from 1.
    pub fn wanted_page(&self) -> usize {
        self.wanted
    }

    /// The page on screen, counted from 1.
    pub fn shown_page(&self) -> usize {
        self.shown
    }

    /// The number of pages of the document, as the last loaded page file named it.
    pub fn page_count(&self) -> usize {
        self.count
    }

    /// Sets the page that the next compile must render.
    pub fn want(&mut self, page: usize) {
        self.wanted = page;
    }

    /// Shows the page in the new folder and deletes the old folder.
    /// If the folder has no page or the page does not load, the new folder is deleted, the old page
    /// stays, and the error text comes back.
    pub fn load(&mut self, dir: PathBuf) -> Result<(), String> {
        let Some((number, count)) = page_in(&dir) else {
            let _ = fs::remove_dir_all(&dir);
            return Err(format!("Typst wrote no page file in {}", dir.display()));
        };
        let path = dir.join(format!("page-{number}-of-{count}.png"));
        match image::open(&path) {
            Ok(image) => {
                self.image = Some(image);
                self.show_view();
                self.shown = number;
                self.wanted = number;
                self.count = count;
                if let Some(old) = self.dir.replace(dir) {
                    let _ = fs::remove_dir_all(old);
                }
                Ok(())
            }
            Err(err) => {
                let _ = fs::remove_dir_all(&dir);
                Err(format!("Cannot load {}: {err}", path.display()))
            }
        }
    }

    /// Asks for the next page or the previous page. At the first page and at the last page, nothing changes.
    /// Returns true when the wanted page changed. The caller must then start a compile for that page.
    /// The old page stays on screen until that compile has loaded the new page.
    pub fn turn(&mut self, forward: bool) -> bool {
        if self.dir.is_none() {
            return false; // no compile has loaded a page, so the page count is not known
        }
        let target = if forward {
            (self.wanted + 1).min(self.count)
        } else {
            self.wanted.saturating_sub(1).max(1)
        };
        std::mem::replace(&mut self.wanted, target) != target
    }

    /// Asks for the last page (`last` is true) or the first page. Like `turn`, it returns true when the wanted
    /// page changed, and the caller must then start a compile for it. It does nothing before the first load.
    pub fn turn_to(&mut self, last: bool) -> bool {
        if self.dir.is_none() {
            return false;
        }
        let target = if last { self.count } else { 1 };
        std::mem::replace(&mut self.wanted, target) != target
    }

    /// The zoom in percent.
    pub fn zoom_percent(&self) -> u32 {
        ZOOMS[self.zoom]
    }

    /// The resolution that the next compile needs, or `None` for the default.
    pub fn ppi(&self) -> Option<u32> {
        (self.zoom > 0).then(|| DEFAULT_PPI * self.zoom_percent() / 100)
    }

    /// Zooms one step in (`more`) or out. Returns true if the zoom changed. The caller must then start a
    /// compile, because the page needs another resolution.
    pub fn zoom_step(&mut self, more: bool) -> bool {
        let next = if more {
            (self.zoom + 1).min(ZOOMS.len() - 1)
        } else {
            self.zoom.saturating_sub(1)
        };
        self.set_zoom(next)
    }

    /// Shows the whole page again. Returns true if the zoom changed. See `zoom_step`.
    pub fn zoom_fit(&mut self) -> bool {
        self.set_zoom(0)
    }

    fn set_zoom(&mut self, zoom: usize) -> bool {
        let changed = std::mem::replace(&mut self.zoom, zoom) != zoom;
        if zoom == 0 {
            self.pan = (0.0, 0.0);
        }
        self.show_view();
        changed
    }

    /// Moves the view by one step in the direction `(dx, dy)`, each -1, 0, or 1. The whole page has
    /// nowhere to go.
    pub fn pan_by(&mut self, dx: i8, dy: i8) {
        if self.zoom == 0 {
            return;
        }
        let step = |pan: f32, d: i8| (pan + PAN_STEP * f32::from(d)).clamp(0.0, 1.0);
        self.pan = (step(self.pan.0, dx), step(self.pan.1, dy));
        self.show_view();
    }

    /// Moves the view to the top (`bottom` is false) or to the bottom of the page.
    pub fn pan_to_edge(&mut self, bottom: bool) {
        if self.zoom > 0 {
            self.pan.1 = if bottom { 1.0 } else { 0.0 };
            self.show_view();
        }
    }

    /// The place of the view, for a test.
    #[cfg(test)]
    pub fn pan(&self) -> (f32, f32) {
        self.pan
    }

    /// Makes the picture for the screen from the page and the zoom.
    fn show_view(&mut self) {
        let Some(image) = &self.image else {
            return;
        };
        let (x, y, w, h) = crop_rect(image.width(), image.height(), self.zoom_percent(), self.pan);
        let view = if self.zoom == 0 {
            image.clone()
        } else {
            image.crop_imm(x, y, w, h)
        };
        self.page = Some(self.picker.new_resize_protocol(view));
    }

    /// The folder of the page on screen, for a test.
    #[cfg(test)]
    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    #[cfg(test)]
    pub fn has_page(&self) -> bool {
        self.page.is_some()
    }

    /// Tells the preview if the last compile failed. The title then says that the page is old.
    pub fn set_stale(&mut self, stale: bool) {
        self.stale = stale;
    }

    pub fn draw(&mut self, frame: &mut Frame, area: Rect) {
        let mut title = match self.page {
            Some(_) if self.zoom > 0 => {
                format!(
                    "Preview {}/{} {}%",
                    self.shown,
                    self.count,
                    self.zoom_percent()
                )
            }
            Some(_) => format!("Preview {}/{}", self.shown, self.count),
            None => "Preview".to_string(),
        };
        // A page from an older compile must not look like the current one.
        let title = if self.stale && self.page.is_some() {
            title.push_str(" (old)");
            Line::styled(title, Style::new().add_modifier(Modifier::DIM))
        } else {
            Line::from(title)
        };
        let block = Block::bordered().title(title);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        match &mut self.page {
            // ponytail: the image is resized and encoded on the UI thread, inside this call.
            // Measured in a release build: 13 ms for a page of 1191 by 1684 pixels.
            // Upgrade: ratatui_image::thread::ThreadProtocol if pages get much larger.
            Some(page) => {
                let area = centered(page.size_for(Resize::Fit(None), inner.as_size()), inner);
                frame.render_stateful_widget(StatefulImage::default(), area, page);
            }
            None => frame.render_widget(Paragraph::new("No preview yet."), inner),
        }
    }
}

impl Drop for Preview {
    fn drop(&mut self) {
        if let Some(dir) = self.dir.take() {
            let _ = fs::remove_dir_all(dir);
        }
    }
}

/// The area of `size` in the middle of `outer`. The picture keeps its aspect ratio, so it is smaller than the
/// pane in one direction. Without this, it sits at the top left and the rest of the pane is empty.
fn centered(size: Size, outer: Rect) -> Rect {
    let width = size.width.min(outer.width);
    let height = size.height.min(outer.height);
    Rect::new(
        outer.x + (outer.width - width) / 2,
        outer.y + (outer.height - height) / 2,
        width,
        height,
    )
}

/// Finds the file `page-<p>-of-<t>.png` in `dir`. Returns `(p, t)`, with `1 <= p <= t`.
/// Typst writes no file when the page that the compile asked for is beyond the end of the document.
pub fn page_in(dir: &Path) -> Option<(usize, usize)> {
    fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let (number, count) = name
                .strip_prefix("page-")?
                .strip_suffix(".png")?
                .split_once("-of-")?;
            let (number, count) = (number.parse::<usize>().ok()?, count.parse::<usize>().ok()?);
            (1 <= number && number <= count).then_some((number, count))
        })
        .min()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend, style::Color};

    const RED: Color = Color::Rgb(255, 0, 0);
    const GREEN: Color = Color::Rgb(0, 255, 0);

    /// Makes a folder with the one file that a compile of page `number` writes: `page-<number>-of-<count>.png`.
    /// The image is 40 by 40 pixels. Page 1 is red, page 2 is green, and all other pages are blue.
    fn page(name: &str, number: usize, count: usize) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("lazytypst-preview-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let rgb = match number {
            1 => [255, 0, 0, 255],
            2 => [0, 255, 0, 255],
            _ => [0, 0, 255, 255],
        };
        image::RgbaImage::from_pixel(40, 40, image::Rgba(rgb))
            .save(dir.join(format!("page-{number}-of-{count}.png")))
            .unwrap();
        dir
    }

    fn red_page(name: &str) -> PathBuf {
        page(name, 1, 1)
    }

    /// Draws the pane on a 30 by 10 screen. Returns the text and a flag for each color.
    fn draw(preview: &mut Preview) -> (String, bool, bool) {
        let mut terminal = Terminal::new(TestBackend::new(30, 10)).unwrap();
        terminal
            .draw(|frame| preview.draw(frame, frame.area()))
            .unwrap();
        let cells = terminal.backend().buffer().content();
        let has = |color| cells.iter().any(|cell| cell.bg == color);
        (screen_text(&terminal), has(RED), has(GREEN))
    }

    fn screen_text(terminal: &Terminal<TestBackend>) -> String {
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn load_reads_the_page_in_the_folder() {
        let dir = red_page("load");
        let mut preview = Preview::new(Picker::halfblocks());
        assert!(!preview.has_page());
        preview.load(dir.clone()).unwrap();
        assert!(preview.has_page());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_title_shows_the_page_number_and_the_page_count_from_the_file_name() {
        let dir = page("title", 30, 67);
        let mut preview = Preview::new(Picker::halfblocks());
        assert!(draw(&mut preview).0.contains("Preview"));
        preview.load(dir.clone()).unwrap();
        assert!(draw(&mut preview).0.contains("Preview 30/67"));
        assert_eq!(preview.wanted_page(), 30);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_new_load_deletes_the_old_folder() {
        let (first, second) = (red_page("old"), red_page("new"));
        let mut preview = Preview::new(Picker::halfblocks());
        preview.load(first.clone()).unwrap();
        preview.load(second.clone()).unwrap();
        assert!(!first.exists(), "the old folder is still there");
        assert!(second.exists());
        fs::remove_dir_all(second).unwrap();
    }

    #[test]
    fn a_new_load_shows_the_new_page() {
        let (first, second) = (page("shows-a", 1, 3), page("shows-b", 2, 3));
        let mut preview = Preview::new(Picker::halfblocks());
        preview.load(first).unwrap();
        let (text, red, green) = draw(&mut preview);
        assert!(text.contains("Preview 1/3") && red && !green);

        preview.load(second.clone()).unwrap();
        let (text, red, green) = draw(&mut preview);
        assert!(text.contains("Preview 2/3") && green && !red, "{text}");
        fs::remove_dir_all(second).unwrap();
    }

    #[test]
    fn a_failed_load_keeps_the_old_page_and_deletes_the_new_folder() {
        let good = red_page("keep");
        let mut preview = Preview::new(Picker::halfblocks());
        preview.load(good.clone()).unwrap();

        let empty =
            std::env::temp_dir().join(format!("lazytypst-preview-empty-{}", std::process::id()));
        fs::create_dir_all(&empty).unwrap();
        let err = preview.load(empty.clone()).unwrap_err();
        assert!(err.contains("no page file"), "{err}");
        assert!(preview.has_page());
        assert!(good.exists(), "the folder of the shown page was deleted");
        assert!(
            !empty.exists(),
            "the folder of the failed load is still there"
        );
        assert!(draw(&mut preview).0.contains("Preview 1/1"));
        fs::remove_dir_all(good).unwrap();
    }

    #[test]
    fn a_file_that_is_not_a_png_is_an_error() {
        let dir = red_page("bad");
        fs::write(dir.join("page-1-of-1.png"), "not a png").unwrap();
        let mut preview = Preview::new(Picker::halfblocks());
        assert!(
            preview
                .load(dir.clone())
                .unwrap_err()
                .contains("Cannot load")
        );
        assert!(!preview.has_page());
        assert!(!dir.exists());
    }

    #[test]
    fn the_pane_shows_a_text_before_the_first_page() {
        let mut terminal = Terminal::new(TestBackend::new(30, 10)).unwrap();
        let mut preview = Preview::new(Picker::halfblocks());
        terminal
            .draw(|frame| preview.draw(frame, frame.area()))
            .unwrap();
        assert!(screen_text(&terminal).contains("No preview yet."));
    }

    #[test]
    fn turn_changes_the_wanted_page_and_stops_at_the_first_and_the_last_page() {
        let dir = page("turn", 1, 3);
        let mut preview = Preview::new(Picker::halfblocks());
        assert!(
            !preview.turn(true),
            "no page count is known before the first load"
        );
        preview.load(dir.clone()).unwrap();

        assert!(!preview.turn(false), "page 1 has no previous page");
        assert!(preview.turn(true));
        assert_eq!(preview.wanted_page(), 2);
        assert!(preview.turn(true));
        assert_eq!(preview.wanted_page(), 3);
        assert!(!preview.turn(true), "page 3 is the last page");
        assert_eq!(preview.wanted_page(), 3);
        assert!(preview.turn(false));
        assert_eq!(preview.wanted_page(), 2);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn turn_to_asks_for_the_first_page_and_the_last_page() {
        let dir = page("turnto", 2, 5);
        let mut preview = Preview::new(Picker::halfblocks());
        preview.load(dir.clone()).unwrap();

        assert!(preview.turn_to(true));
        assert_eq!(preview.wanted_page(), 5);
        assert!(!preview.turn_to(true), "already on the last page");
        assert!(preview.turn_to(false));
        assert_eq!(preview.wanted_page(), 1);
        assert!(!preview.turn_to(false), "already on the first page");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn turn_to_does_nothing_before_the_first_load() {
        let mut preview = Preview::new(Picker::halfblocks());
        assert!(!preview.turn_to(true));
        assert!(!preview.turn_to(false));
        assert_eq!(preview.wanted_page(), 1);
    }

    #[test]
    fn turn_to_works_when_a_page_turn_is_already_pending() {
        let dir = page("turnpending", 1, 5);
        let mut preview = Preview::new(Picker::halfblocks());
        preview.load(dir.clone()).unwrap();
        assert!(preview.turn(true)); // wanted 2, not loaded yet
        assert!(preview.turn_to(true));
        assert_eq!(preview.wanted_page(), 5);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_old_page_stays_on_screen_after_a_turn_until_a_new_page_loads() {
        let (first, second) = (page("stay-a", 1, 3), page("stay-b", 2, 3));
        let mut preview = Preview::new(Picker::halfblocks());
        preview.load(first).unwrap();
        assert!(preview.turn(true));
        let (text, red, green) = draw(&mut preview);
        assert!(
            text.contains("Preview 1/3") && red && !green,
            "the old page must stay: {text}"
        );

        preview.load(second.clone()).unwrap();
        assert!(draw(&mut preview).0.contains("Preview 2/3"));
        fs::remove_dir_all(second).unwrap();
    }

    #[test]
    fn page_in_reads_the_page_number_and_the_count_from_the_name() {
        let dir = page("name", 30, 67);
        assert_eq!(page_in(&dir), Some((30, 67)));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn page_in_ignores_names_that_are_not_a_page_file() {
        let dir =
            std::env::temp_dir().join(format!("lazytypst-preview-names-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for name in [
            "page-1.png",          // the old name
            "page-x-of-3.png",     // not a number
            "page-0-of-3.png",     // pages count from 1
            "page-4-of-3.png",     // beyond the end
            "page-1-of-3.png.tmp", // another ending
            "notes.txt",
        ] {
            fs::write(dir.join(name), "").unwrap();
        }
        assert_eq!(page_in(&dir), None);
        fs::write(dir.join("page-2-of-3.png"), "").unwrap();
        assert_eq!(page_in(&dir), Some((2, 3)));
        assert_eq!(page_in(&dir.join("missing")), None);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_pane_draws_the_page() {
        let dir = red_page("draw");
        let mut terminal = Terminal::new(TestBackend::new(30, 10)).unwrap();
        let mut preview = Preview::new(Picker::halfblocks());
        preview.load(dir.clone()).unwrap();
        terminal
            .draw(|frame| preview.draw(frame, frame.area()))
            .unwrap();

        let screen = screen_text(&terminal);
        let red = Color::Rgb(255, 0, 0);
        let cells = terminal.backend().buffer().content();
        assert!(
            cells.iter().any(|cell| cell.bg == red),
            "no red cell on screen"
        );
        assert!(!screen.contains("No preview yet."));
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn the_crop_rectangle_follows_the_zoom_and_the_pan() {
        // The whole page at 100%, wherever the pan is.
        assert_eq!(crop_rect(1000, 2000, 100, (0.7, 0.7)), (0, 0, 1000, 2000));
        // Half of the page at 200%: the pan moves the view along the other half.
        assert_eq!(crop_rect(1000, 2000, 200, (0.0, 0.0)), (0, 0, 500, 1000));
        assert_eq!(
            crop_rect(1000, 2000, 200, (1.0, 1.0)),
            (500, 1000, 500, 1000)
        );
        assert_eq!(crop_rect(1000, 2000, 200, (0.5, 0.0)), (250, 0, 500, 1000));
        // A pan outside the range stays on the page.
        assert_eq!(crop_rect(1000, 2000, 200, (3.0, -1.0)), (500, 0, 500, 1000));
        // 150%: the view is two thirds of the page.
        assert_eq!(crop_rect(900, 900, 150, (1.0, 0.0)), (300, 0, 600, 600));
    }

    #[test]
    fn zoom_steps_stop_at_the_ends_and_zero_fits_the_page_again() {
        let dir = red_page("zoom");
        let mut preview = Preview::new(Picker::halfblocks());
        preview.load(dir.clone()).unwrap();
        assert_eq!((preview.zoom_percent(), preview.ppi()), (100, None));
        assert!(preview.zoom_step(true));
        assert!(preview.zoom_step(true));
        assert_eq!((preview.zoom_percent(), preview.ppi()), (200, Some(288)));
        assert!(preview.zoom_step(true));
        assert!(!preview.zoom_step(true), "300% is the last step");
        assert_eq!(preview.zoom_percent(), 300);
        preview.pan_by(1, 1);
        assert!(preview.zoom_fit());
        assert_eq!((preview.zoom_percent(), preview.pan()), (100, (0.0, 0.0)));
        assert!(!preview.zoom_step(false), "100% is the first step");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_arrow_keys_move_the_view_only_when_zoomed_and_stay_on_the_page() {
        let dir = red_page("pan");
        let mut preview = Preview::new(Picker::halfblocks());
        preview.load(dir.clone()).unwrap();
        preview.pan_by(1, 1);
        assert_eq!(preview.pan(), (0.0, 0.0), "a whole page has nowhere to go");
        preview.zoom_step(true);
        preview.pan_by(0, 1);
        assert_eq!(preview.pan(), (0.0, 0.25));
        preview.pan_to_edge(true);
        assert_eq!(preview.pan().1, 1.0);
        preview.pan_by(0, 1);
        assert_eq!(preview.pan().1, 1.0, "the bottom is the end");
        preview.pan_to_edge(false);
        assert_eq!(preview.pan().1, 0.0);
        // A new page keeps the zoom and the place.
        preview.pan_by(1, 0);
        let again = red_page("pan2");
        preview.load(again.clone()).unwrap();
        assert_eq!((preview.zoom_percent(), preview.pan()), (150, (0.25, 0.0)));
        fs::remove_dir_all(again).unwrap();
        fs::remove_dir_all(dir).ok();
    }
    #[test]
    fn centered_puts_a_smaller_area_in_the_middle_and_keeps_a_bigger_one_inside() {
        let outer = Rect::new(10, 5, 41, 21);
        assert_eq!(centered(Size::new(10, 20), outer), Rect::new(25, 5, 10, 20));
        assert_eq!(
            centered(Size::new(41, 10), outer),
            Rect::new(10, 10, 41, 10)
        );
        assert_eq!(
            centered(Size::new(100, 100), outer),
            outer,
            "never outside the pane"
        );
        assert_eq!(centered(Size::new(0, 0), outer), Rect::new(30, 15, 0, 0));
    }

    #[test]
    fn the_page_is_drawn_in_the_middle_of_a_wide_pane() {
        let dir = red_page("middle");
        let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
        let mut preview = Preview::new(Picker::halfblocks());
        preview.load(dir.clone()).unwrap();
        terminal
            .draw(|frame| preview.draw(frame, frame.area()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let red = Color::Rgb(255, 0, 0);
        let columns: Vec<u16> = (1..79)
            .filter(|column| (1..11).any(|row| buffer[(*column, row)].bg == red))
            .collect();
        let (first, last) = (*columns.first().unwrap(), *columns.last().unwrap());
        // The inner area is the columns 1 to 78. The gap at the left and the gap at the right differ by one at most.
        let (left_gap, right_gap) = (first - 1, 78 - last);
        assert!(
            left_gap > 5,
            "the page must not sit at the left edge: {first}"
        );
        assert!(
            left_gap.abs_diff(right_gap) <= 1,
            "{left_gap} and {right_gap}"
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
