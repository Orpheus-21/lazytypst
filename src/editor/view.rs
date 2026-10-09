use super::*;

/// The width of the text on the screen, in cells. A wide character, such as a Chinese character, takes 2.
pub(super) fn display_width(text: &str) -> usize {
    Span::raw(text).width()
}

/// Breaks `text` into rows of at most `width` cells. A row breaks at a space when it can. A word that is
/// wider than a row is split. A text with no characters gives one empty row. No width gives no rows.
pub(super) fn wrap_rows(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }
    let mut rows = Vec::new();
    let mut row = String::new();
    for word in text.split(' ') {
        let space = usize::from(!row.is_empty());
        if display_width(&row) + space + display_width(word) <= width {
            if space == 1 {
                row.push(' ');
            }
            row.push_str(word);
            continue;
        }
        if !row.is_empty() {
            rows.push(std::mem::take(&mut row));
        }
        for letter in word.chars() {
            let mut one = [0; 4];
            if !row.is_empty()
                && display_width(&row) + display_width(letter.encode_utf8(&mut one)) > width
            {
                rows.push(std::mem::take(&mut row));
            }
            row.push(letter);
        }
    }
    rows.push(row);
    rows
}

/// Wraps the styled lines into screen rows of `width` cells, and keeps `height` rows. If rows do not fit,
/// the last kept row says how many rows are hidden, for example `+3 more`. The count is in screen rows.
pub(super) fn fit_rows(
    lines: Vec<(String, Style)>,
    width: usize,
    height: usize,
) -> Vec<Line<'static>> {
    let mut rows: Vec<Line<'static>> = lines
        .into_iter()
        .flat_map(|(text, style)| {
            wrap_rows(&text, width)
                .into_iter()
                .map(move |row| Line::styled(row, style))
        })
        .collect();
    if rows.len() > height {
        let shown = height.saturating_sub(1);
        let hidden = rows.len() - shown;
        rows.truncate(shown);
        rows.push(Line::styled(
            format!("+{hidden} more"),
            Style::new().add_modifier(Modifier::DIM),
        ));
    }
    rows
}

/// The style of a line of the compile report. The colors are the colors of the terminal palette, so they
/// follow the theme of the user: errors are red, warnings are yellow, and other lines are dim.
pub(super) fn severity_style(severity: Severity, colors: bool) -> Style {
    match (severity, colors) {
        (Severity::Error, true) => Style::new().fg(Color::Red),
        (Severity::Warning, true) => Style::new().fg(Color::Yellow),
        // Without color, an error is bold and a warning is plain. The text of the line says which it is.
        (Severity::Error, false) => Style::new().add_modifier(Modifier::BOLD),
        (Severity::Warning, false) => Style::new(),
        (Severity::Other, _) => Style::new().add_modifier(Modifier::DIM),
    }
}

/// The words for the number of errors and warnings, for the title of the compile pane:
/// `2 errors, 1 warning`. `None` if there are none.
/// The style of a line number that has an error or a warning. Without colors, an error is reversed and a
/// warning is underlined. The line numbers are dim, so a mark takes the dim off.
pub(super) fn mark_style(severity: Severity, colors: bool) -> Style {
    let style = Style::new().remove_modifier(Modifier::DIM);
    match (severity, colors) {
        (Severity::Error, true) => style.fg(Color::Red).add_modifier(Modifier::BOLD),
        (Severity::Warning, true) => style.fg(Color::Yellow),
        (Severity::Error, false) => style.add_modifier(Modifier::REVERSED),
        _ => style.add_modifier(Modifier::UNDERLINED),
    }
}

pub(super) fn counts_text(errors: usize, warnings: usize) -> Option<String> {
    let part = |count: usize, word: &str| match count {
        0 => None,
        1 => Some(format!("1 {word}")),
        _ => Some(format!("{count} {word}s")),
    };
    let parts: Vec<String> = [part(errors, "error"), part(warnings, "warning")]
        .into_iter()
        .flatten()
        .collect();
    (!parts.is_empty()).then(|| parts.join(", "))
}

impl Editor {
    pub(super) fn compile_pane(&self, area: Rect) -> Paragraph<'static> {
        let plain = Style::default();
        let (mut color, mut lines): (Color, Vec<(String, Style)>) = match &self.report {
            Some(report) => {
                let color = if report.ok { Color::Green } else { Color::Red };
                let head = report.ok.then(|| match report.elapsed {
                    Some(elapsed) => (format!("OK in {} ms", elapsed.as_millis()), plain),
                    None => ("OK".to_string(), plain),
                });
                // One line of the report has one diagnostic. The kind of the diagnostic decides the style.
                let body =
                    report
                        .lines
                        .iter()
                        .zip(&report.diagnostics)
                        .map(|(line, diagnostic)| {
                            (
                                line.clone(),
                                severity_style(diagnostic.severity, self.colors),
                            )
                        });
                (color, head.into_iter().chain(body).collect())
            }
            None => (
                Color::Reset,
                vec![("Press Ctrl-B to compile.".to_string(), plain)],
            ),
        };
        // The title: the counts of errors and warnings, and for a failed compile also its time.
        let mut title = if self.paused {
            "Compile (paused)".to_string()
        } else {
            "Compile".to_string()
        };
        if let Some(report) = &self.report {
            if let Some(counts) = counts_text(report.error_count(), report.warning_count()) {
                title = format!("{title}: {counts}");
            }
            if let Some(elapsed) = report.elapsed.filter(|_| !report.ok) {
                title = format!("{title} ({} ms)", elapsed.as_millis());
            }
        }
        if self.job.is_some() {
            // The last report stays on screen until the new report replaces it.
            color = Color::Yellow;
            title = "Compile (running)".to_string();
            if self.report.is_none() {
                lines = vec![("Compiling...".to_string(), plain)];
            }
        }
        if let Some(pdf) = &self.exported {
            lines.push((format!("Exported {}", pdf.display()), plain));
        }
        let block = Block::bordered().title(title).border_style(if self.colors {
            Style::new().fg(color)
        } else {
            Style::new()
        });
        let inner = block.inner(area);
        Paragraph::new(fit_rows(
            lines,
            usize::from(inner.width),
            usize::from(inner.height),
        ))
        .block(block)
    }

    /// Colors the cells of the text area that the text area has drawn. `ratatui-textarea` has no style for a
    /// single word, so this reads the rows from the buffer and sets only the foreground of the cells of a
    /// part (see `highlight`). The background stays, so the cursor, the selection, and the search matches
    /// keep their look.
    ///
    /// A row that starts a line shows the number of the line in the gutter. A row that continues a wrapped
    /// line has a blank gutter, and it continues where the row above stopped. The rows above the first
    /// numbered row are left plain, because the line of such a row is not known.
    /// The lines of the open file that have an error or a warning in the last compile, counted from 0. If a
    /// line has both, the error wins. A line of another file is not here.
    fn line_marks(&self) -> std::collections::HashMap<usize, Severity> {
        let mut marks = std::collections::HashMap::new();
        let Some(report) = &self.report else {
            return marks;
        };
        let open = self.path.strip_prefix(&self.root).unwrap_or(&self.path);
        for diagnostic in &report.diagnostics {
            if diagnostic.line == 0
                || diagnostic.file.as_deref() != Some(open)
                || !matches!(diagnostic.severity, Severity::Error | Severity::Warning)
            {
                continue;
            }
            let entry = marks
                .entry(diagnostic.line - 1)
                .or_insert(diagnostic.severity);
            if diagnostic.severity == Severity::Error {
                *entry = Severity::Error;
            }
        }
        marks
    }

    pub(super) fn paint(&self, buffer: &mut ratatui::buffer::Buffer, area: Rect) {
        let lines = self.textarea.lines();
        let gutter = u16::try_from(lines.len().to_string().len() + 2).unwrap_or(u16::MAX);
        let parts = highlight::tokenize(lines);
        let marks = self.line_marks();
        let mut current: Option<(usize, Vec<char>, usize)> = None; // the line, its characters, the next offset
        for y in area.top()..area.bottom() {
            let number: String = (area.left()..(area.left() + gutter).min(area.right()))
                .map(|x| buffer[(x, y)].symbol().to_string())
                .collect();
            let number = number.trim();
            if let Ok(n) = number.parse::<usize>() {
                let Some(text) = lines.get(n.wrapping_sub(1)) else {
                    current = None;
                    continue;
                };
                // The number of a line with an error or a warning in the last compile stands out.
                if let Some(severity) = marks.get(&(n - 1)) {
                    let style = mark_style(*severity, self.colors);
                    for x in area.left()..(area.left() + gutter).min(area.right()) {
                        if buffer[(x, y)].symbol().trim().is_empty() {
                            continue;
                        }
                        buffer[(x, y)].set_style(style);
                    }
                }
                current = Some((n - 1, text.chars().collect(), 0));
            } else if !number.is_empty() {
                current = None;
            }
            let Some((line, chars, offset)) = &mut current else {
                continue;
            };
            let mut x = area.left() + gutter;
            // A wrap at a space drops the space from the row. Skip it in the text.
            let first = buffer[(x.min(area.right().saturating_sub(1)), y)]
                .symbol()
                .to_string();
            while *offset < chars.len()
                && chars[*offset].is_whitespace()
                && !first.starts_with(chars[*offset])
            {
                *offset += 1;
            }
            while x < area.right() && *offset < chars.len() {
                let ch = chars[*offset];
                // The text area gives a tab the cells up to the next tab stop, counted from the start of the
                // row, and a combining mark no cell.
                let width = if ch == '\t' {
                    let stop = usize::from(self.textarea.tab_length().max(1));
                    stop - usize::from(x - area.left() - gutter) % stop
                } else {
                    Span::raw(ch.to_string()).width()
                };
                let width = u16::try_from(width).unwrap_or(1);
                if let Some(part) = parts[*line]
                    .iter()
                    .find(|part| part.start <= *offset && *offset < part.end)
                {
                    let style = highlight::style(part.kind, self.colors);
                    for cell_x in x..(x + width).min(area.right()) {
                        buffer[(cell_x, y)].set_style(style);
                    }
                }
                x += width;
                *offset += 1;
            }
        }
    }

    pub fn draw(&mut self, frame: &mut Frame) {
        let [main, status] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());
        if matches!(self.mode, Mode::Full) {
            self.preview
                .set_stale(self.report.as_ref().is_some_and(|report| !report.ok));
            self.preview.draw(frame, main);
            let zoom = format!("{}%", self.preview.zoom_percent());
            let width = u16::try_from(zoom.len() + 1).unwrap_or(u16::MAX);
            let [hint, zoom_area] =
                Layout::horizontal([Constraint::Min(0), Constraint::Length(width)]).areas(status);
            frame.render_widget(
                Paragraph::new(
                    "F11 back to editor  Alt-Up/Down page  + - 0 zoom  arrows move the view",
                ),
                hint,
            );
            frame.render_widget(Paragraph::new(zoom).alignment(Alignment::Right), zoom_area);
            return;
        }
        let [left, right] =
            Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                .areas(main);
        let [body, pane] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(PANE_HEIGHT)]).areas(left);
        let marker = if self.dirty { " [+]" } else { "" };
        let name = self.path.strip_prefix(&self.root).unwrap_or(&self.path);
        let mut title = format!("{}{marker}", name.display());
        if let Some(main) = self.main.as_deref().filter(|main| *main != self.path) {
            let main = main.strip_prefix(&self.root).unwrap_or(main);
            title.push_str(&format!(" (main: {})", main.display()));
        }
        let block = Block::bordered().title(title);
        let inner = block.inner(body);
        frame.render_widget(block, body);
        frame.render_widget(&self.textarea, inner);
        self.paint(frame.buffer_mut(), inner);
        frame.render_widget(self.compile_pane(pane), pane);
        self.preview
            .set_stale(self.report.as_ref().is_some_and(|report| !report.ok));
        self.preview.draw(frame, right);
        let hint = if self.message.is_empty() {
            "F1 help  Ctrl-S save  Ctrl-B compile  Ctrl-E PDF  Alt-Up/Down page  Esc back"
        } else {
            &self.message
        };
        // The cursor position is at the right end of the status line, also while a message shows. Line and
        // column start at 1, and the column counts characters, the same as the error lines of Typst.
        let cursor = self.textarea.cursor();
        let position = format!(
            "{} {}  {}:{}",
            self.words,
            if self.words == 1 { "word" } else { "words" },
            cursor.0 + 1,
            cursor.1 + 1
        );
        let width = u16::try_from(position.len() + 1).unwrap_or(u16::MAX);
        let [hint_area, position_area] =
            Layout::horizontal([Constraint::Min(0), Constraint::Length(width)]).areas(status);
        if let Mode::Search(prompt) = &self.mode {
            let message_width = u16::try_from(self.message.len()).unwrap_or(u16::MAX);
            let label_text = if self.search_regex {
                "Search (regex): "
            } else {
                "Search (text): "
            };
            let [label, input, message] = Layout::horizontal([
                Constraint::Length(u16::try_from(label_text.len()).unwrap_or(u16::MAX)),
                Constraint::Min(1),
                Constraint::Length(message_width),
            ])
            .areas(hint_area);
            frame.render_widget(Paragraph::new(label_text), label);
            frame.render_widget(&**prompt, input);
            frame.render_widget(Paragraph::new(self.message.as_str()), message);
        } else {
            frame.render_widget(Paragraph::new(hint), hint_area);
        }
        frame.render_widget(
            Paragraph::new(position).alignment(Alignment::Right),
            position_area,
        );
    }
}
