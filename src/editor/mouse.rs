use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

use super::*;

/// The rows that one turn of the wheel moves the text.
const WHEEL_ROWS: i16 = 3;

fn inside(area: Rect, x: u16, y: u16) -> bool {
    area.width > 0 && x >= area.left() && x < area.right() && y >= area.top() && y < area.bottom()
}

impl Editor {
    pub fn mouse_on(&self) -> bool {
        self.mouse
    }

    pub fn set_mouse(&mut self, on: bool) {
        self.mouse = on;
    }

    /// `F9`: the program takes the mouse, or leaves it to the terminal. With the mouse taken, the
    /// terminal cannot select text by itself: hold `Shift` while you drag to select with the terminal.
    pub(super) fn toggle_mouse(&mut self) {
        self.mouse = !self.mouse;
        self.dragging = false;
        self.message = if self.mouse {
            "Mouse on. Shift and drag selects with the terminal. F9 turns it off.".into()
        } else {
            "Mouse off".into()
        };
    }

    /// The place (line, character) under the cell (`x`, `y`) of the text. A click in the gutter or left of
    /// the text goes to the start of the row, and a click right of the text goes to the end of the row.
    fn place_at(&self, x: u16, y: u16) -> Option<(usize, usize)> {
        let hit = self.hits.iter().find(|hit| hit.y == y)?;
        let Some((first_x, _, _)) = hit.cells.first() else {
            return Some((hit.line, hit.start));
        };
        if x < *first_x {
            return Some((hit.line, hit.start));
        }
        match hit
            .cells
            .iter()
            .find(|(start, width, _)| x >= *start && x < start + width)
        {
            Some((_, _, offset)) => Some((hit.line, *offset)),
            None => hit
                .cells
                .last()
                .map(|(_, _, offset)| (hit.line, offset + 1)),
        }
    }

    /// Handles a mouse event in the editor. A click in the text puts the cursor there, a drag selects, the
    /// wheel scrolls the text, and the wheel over the preview turns the page. Other events do nothing.
    /// Returns true when the screen must redraw.
    pub fn handle_mouse(&mut self, event: MouseEvent) -> bool {
        if !self.mouse || !matches!(self.mode, Mode::Edit) {
            return false;
        }
        let (x, y) = (event.column, event.row);
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) if inside(self.text_area, x, y) => {
                let Some(place) = self.place_at(x, y) else {
                    return false;
                };
                self.close_armed = false;
                self.message.clear();
                self.textarea.cancel_selection();
                self.set_cursor_position(place);
                self.textarea.start_selection();
                self.dragging = true;
                true
            }
            MouseEventKind::Drag(MouseButton::Left) if self.dragging => {
                // A drag above or below the text goes on to the first or last row that shows.
                let y = y.clamp(
                    self.text_area.top(),
                    self.text_area.bottom().saturating_sub(1),
                );
                if let Some(place) = self.place_at(
                    x.clamp(
                        self.text_area.left(),
                        self.text_area.right().saturating_sub(1),
                    ),
                    y,
                ) {
                    self.set_cursor_position(place);
                }
                true
            }
            MouseEventKind::Up(MouseButton::Left) if self.dragging => {
                self.dragging = false;
                // A click with no move is no selection.
                if self
                    .textarea
                    .selection_range()
                    .is_some_and(|(start, end)| start == end)
                {
                    self.textarea.cancel_selection();
                }
                true
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let down = event.kind == MouseEventKind::ScrollDown;
                if inside(self.text_area, x, y) {
                    self.textarea
                        .scroll((if down { WHEEL_ROWS } else { -WHEEL_ROWS }, 0));
                    true
                } else if inside(self.preview_area, x, y) && self.preview.turn(down) {
                    self.recover = None;
                    self.start_compile();
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }
}
