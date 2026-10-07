//! Basic syntax color for Typst source text.
//!
//! `ratatui-textarea` has no style for a single word, so the editor draws the text area as always, and
//! then `Editor::paint` colors the cells of the rows on screen. This module only finds the parts of the
//! text: it does not know the screen. A part is a range of characters in one line.
//!
//! The scanner is small and it is not a Typst parser. It knows these parts:
//! - a heading: a line that starts with `=` signs and a space,
//! - a comment: `//` to the end of the line (not in `https://`), and `/* */`, also over many lines,
//! - math between `$` signs, also over many lines,
//! - a command after `#`, such as `#set` or `#image`, and the strings inside its parentheses,
//! - raw text between backticks, also over many lines.

use ratatui::style::{Color, Modifier, Style};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Heading,
    /// A keyword after `#`, such as `set`, `let`, or `import`.
    Keyword,
    /// Another name after `#`, such as a function.
    Function,
    Str,
    Math,
    Comment,
    Raw,
}

/// A range of characters `start..end` in one line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Part {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
}

const KEYWORDS: [&str; 15] = [
    "set", "show", "let", "import", "include", "if", "else", "for", "while", "return", "break",
    "continue", "context", "in", "as",
];

/// The style of a kind. The colors are palette colors, so they follow the theme of the terminal.
/// Without colors (`NO_COLOR`), the kinds differ by bold, dim, and italic.
pub fn style(kind: Kind, colors: bool) -> Style {
    let plain = Style::new();
    match (kind, colors) {
        (Kind::Heading, true) => plain.fg(Color::Blue).add_modifier(Modifier::BOLD),
        (Kind::Heading, false) => plain.add_modifier(Modifier::BOLD),
        (Kind::Keyword, true) => plain.fg(Color::Magenta),
        (Kind::Keyword, false) => plain.add_modifier(Modifier::BOLD),
        (Kind::Function, true) => plain.fg(Color::Cyan),
        (Kind::Function, false) => plain,
        (Kind::Str, true) => plain.fg(Color::Green),
        (Kind::Str, false) => plain.add_modifier(Modifier::ITALIC),
        (Kind::Math, true) => plain.fg(Color::Yellow),
        (Kind::Math, false) => plain.add_modifier(Modifier::ITALIC),
        (Kind::Raw, true) => plain.fg(Color::Yellow),
        (Kind::Raw, false) => plain.add_modifier(Modifier::ITALIC),
        (Kind::Comment, true) => plain.fg(Color::DarkGray),
        (Kind::Comment, false) => plain.add_modifier(Modifier::DIM),
    }
}

/// What the scanner is inside of at the end of a line.
#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Text,
    BlockComment,
    Math,
    Raw,
}

/// Finds the parts of each line. The result has one list for each line.
pub fn tokenize(lines: &[String]) -> Vec<Vec<Part>> {
    let mut state = State::Text;
    lines
        .iter()
        .map(|line| {
            let (parts, next) = scan_line(&line.chars().collect::<Vec<_>>(), state);
            state = next;
            parts
        })
        .collect()
}

fn scan_line(chars: &[char], mut state: State) -> (Vec<Part>, State) {
    let mut parts = Vec::new();
    let mut i = 0;
    // The depth of the parentheses of a command, such as `#set text(font: "X")`. Strings count inside.
    let mut code_depth = 0usize;
    // After a keyword such as `set` or `import`, the rest of the line is code.
    let mut code_line = false;
    let push = |parts: &mut Vec<Part>, start: usize, end: usize, kind: Kind| {
        if end > start {
            parts.push(Part { start, end, kind });
        }
    };
    // A heading is a line that starts with `=` signs and a space.
    if state == State::Text {
        let indent = chars.iter().take_while(|ch| **ch == ' ').count();
        let signs = chars[indent..].iter().take_while(|ch| **ch == '=').count();
        if signs > 0 && chars.get(indent + signs) == Some(&' ') {
            push(&mut parts, 0, chars.len(), Kind::Heading);
            return (parts, State::Text);
        }
    }
    while i < chars.len() {
        match state {
            State::BlockComment => {
                let start = i;
                while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                    i += 1;
                }
                if i < chars.len() {
                    i += 2;
                    state = State::Text;
                }
                push(&mut parts, start, i, Kind::Comment);
            }
            State::Math => {
                let start = i;
                while i < chars.len() && chars[i] != '$' {
                    i += if chars[i] == '\\' { 2 } else { 1 };
                }
                let end = (i + 1).min(chars.len());
                if i < chars.len() {
                    i += 1;
                    state = State::Text;
                }
                push(&mut parts, start, end, Kind::Math);
            }
            State::Raw => {
                let start = i;
                while i < chars.len() && chars[i] != '`' {
                    i += 1;
                }
                if i < chars.len() {
                    i += 1;
                    state = State::Text;
                }
                push(&mut parts, start, i, Kind::Raw);
            }
            State::Text => {
                let ch = chars[i];
                let next = chars.get(i + 1).copied();
                if ch == '\\' {
                    i += 2; // an escaped character, such as `\$` or `\#`
                } else if ch == '/' && next == Some('/') && !(i > 0 && chars[i - 1] == ':') {
                    push(&mut parts, i, chars.len(), Kind::Comment);
                    i = chars.len();
                } else if ch == '/' && next == Some('*') {
                    state = State::BlockComment;
                    let start = i;
                    i += 2;
                    // The loop above finds the end, so give it the start as a part of this comment.
                    let mut end = i;
                    while end < chars.len()
                        && !(chars[end] == '*' && chars.get(end + 1) == Some(&'/'))
                    {
                        end += 1;
                    }
                    if end < chars.len() {
                        end += 2;
                        state = State::Text;
                    }
                    push(&mut parts, start, end.min(chars.len()), Kind::Comment);
                    i = end;
                } else if ch == '$' {
                    state = State::Math;
                    let start = i;
                    i += 1;
                    let mut end = i;
                    while end < chars.len() && chars[end] != '$' {
                        end += if chars[end] == '\\' { 2 } else { 1 };
                    }
                    if end < chars.len() {
                        end += 1;
                        state = State::Text;
                    }
                    push(&mut parts, start, end.min(chars.len()), Kind::Math);
                    i = end;
                } else if ch == '`' {
                    state = State::Raw;
                    let start = i;
                    i += 1;
                    let mut end = i;
                    while end < chars.len() && chars[end] != '`' {
                        end += 1;
                    }
                    if end < chars.len() {
                        end += 1;
                        state = State::Text;
                    }
                    push(&mut parts, start, end, Kind::Raw);
                    i = end;
                } else if ch == '#' && next.is_some_and(is_name_start) {
                    let start = i;
                    i += 1;
                    let name_start = i;
                    while i < chars.len() && is_name_char(chars[i]) {
                        i += 1;
                    }
                    let name: String = chars[name_start..i].iter().collect();
                    let kind = if KEYWORDS.contains(&name.as_str()) {
                        Kind::Keyword
                    } else {
                        Kind::Function
                    };
                    push(&mut parts, start, i, kind);
                    code_line |= kind == Kind::Keyword;
                    if chars.get(i) == Some(&'(') {
                        code_depth = 1;
                        i += 1;
                    }
                } else if code_depth > 0 || code_line {
                    match ch {
                        '(' => code_depth += 1,
                        ')' => code_depth = code_depth.saturating_sub(1),
                        '"' => {
                            let start = i;
                            i += 1;
                            while i < chars.len() && chars[i] != '"' {
                                i += if chars[i] == '\\' { 2 } else { 1 };
                            }
                            i = (i + 1).min(chars.len());
                            push(&mut parts, start, i, Kind::Str);
                            continue;
                        }
                        _ => {}
                    }
                    i += 1;
                } else {
                    i += 1;
                }
            }
        }
    }
    (parts, state)
}

fn is_name_start(ch: char) -> bool {
    ch.is_alphabetic() || ch == '_'
}

fn is_name_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_' || ch == '-'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(text: &str) -> Vec<Vec<(usize, usize, Kind)>> {
        let lines: Vec<String> = text.lines().map(String::from).collect();
        tokenize(&lines)
            .into_iter()
            .map(|line| line.iter().map(|p| (p.start, p.end, p.kind)).collect())
            .collect()
    }

    #[test]
    fn a_heading_is_the_whole_line() {
        assert_eq!(parts("= Title"), [[(0, 7, Kind::Heading)]]);
        assert_eq!(parts("=== Deep"), [[(0, 8, Kind::Heading)]]);
        assert_eq!(parts("=no space"), [Vec::new()]);
        assert_eq!(parts("a = b"), [Vec::new()]);
    }

    #[test]
    fn a_command_after_a_hash_is_a_keyword_or_a_function() {
        assert_eq!(parts("#set text(size: 11pt)"), [[(0, 4, Kind::Keyword)]]);
        assert_eq!(
            parts("#image(\"a.png\")"),
            [[(0, 6, Kind::Function), (7, 14, Kind::Str)]]
        );
        assert_eq!(parts("# not a command"), [Vec::new()]);
    }

    #[test]
    fn strings_count_only_inside_the_parentheses_of_a_command() {
        assert_eq!(parts("say \"hello\" now"), [Vec::new()]);
        assert_eq!(
            parts("#text(font: \"Libertinus\")"),
            [[(0, 5, Kind::Function), (12, 24, Kind::Str)]]
        );
    }

    #[test]
    fn comments_run_to_the_end_of_the_line_and_a_url_is_not_a_comment() {
        assert_eq!(parts("a // note"), [[(2, 9, Kind::Comment)]]);
        assert_eq!(parts("see https://x.org"), [Vec::new()]);
        assert_eq!(parts("a /* b */ c"), [[(2, 9, Kind::Comment)]]);
    }

    #[test]
    fn a_block_comment_can_span_lines() {
        assert_eq!(
            parts("a /* one\ntwo\nthree */ b"),
            [
                vec![(2, 8, Kind::Comment)],
                vec![(0, 3, Kind::Comment)],
                vec![(0, 8, Kind::Comment)]
            ]
        );
    }

    #[test]
    fn math_is_between_dollar_signs_also_over_lines_and_an_escaped_dollar_is_text() {
        assert_eq!(parts("the $x + y$ sum"), [[(4, 11, Kind::Math)]]);
        assert_eq!(
            parts("$ a\n b $"),
            [vec![(0, 3, Kind::Math)], vec![(0, 4, Kind::Math)]]
        );
        assert_eq!(parts("cost \\$5"), [Vec::new()]);
    }

    #[test]
    fn raw_text_is_between_backticks() {
        assert_eq!(parts("use `code` here"), [[(4, 10, Kind::Raw)]]);
    }

    #[test]
    fn the_scanner_is_fast_on_a_long_file() {
        let text: String = (0..2000)
            .map(|n| format!("= Heading {n}\nText with $x$ and #emph[word] and \"q\" // c\n"))
            .collect();
        let lines: Vec<String> = text.lines().map(String::from).collect();
        let start = std::time::Instant::now();
        let all = tokenize(&lines);
        let elapsed = start.elapsed();
        assert_eq!(all.len(), 4000);
        eprintln!("tokenize 4000 lines: {elapsed:?}");
        assert!(
            elapsed < std::time::Duration::from_millis(500),
            "{elapsed:?}"
        );
    }
}
