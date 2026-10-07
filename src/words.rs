//! An approximate word count of Typst source text.
//!
//! The count skips what is not prose:
//! - comments (`//` to the end of the line, and `/* */`),
//! - math between `$` signs,
//! - lines that start with `#` and a command such as `set`, `show`, `import`, or `let`. A command that
//!   opens a bracket and does not close it on that line skips the lines up to the closing bracket.
//!
//! A word is a part of the text between spaces that has at least one letter or digit. So `=` and `-` at the
//! start of a heading or a list item are not words. The count is approximate: a function call in the middle
//! of a line, such as `#image("a.png")`, still adds its parts.

/// The commands that make a line code and not text.
const COMMANDS: [&str; 11] = [
    "set",
    "show",
    "import",
    "let",
    "include",
    "pagebreak",
    "colbreak",
    "bibliography",
    "outline",
    "context",
    "counter",
];

pub fn count(source: &str) -> usize {
    let prose = strip_comments_and_math(source);
    let mut words = 0;
    let mut depth: i32 = 0;
    for line in prose.lines() {
        if depth <= 0 && !is_code_line(line) {
            words += line
                .split_whitespace()
                .filter(|word| word.chars().any(char::is_alphanumeric))
                .count();
            continue;
        }
        // A code line, or the lines of an open bracket: skip them and follow the brackets.
        depth += line.chars().fold(0, |depth, ch| match ch {
            '(' | '[' | '{' => depth + 1,
            ')' | ']' | '}' => depth - 1,
            _ => depth,
        });
    }
    words
}

fn is_code_line(line: &str) -> bool {
    let Some(rest) = line.trim_start().strip_prefix('#') else {
        return false;
    };
    let name: String = rest
        .chars()
        .take_while(|ch| ch.is_alphanumeric() || *ch == '-')
        .collect();
    COMMANDS.contains(&name.as_str())
}

/// Removes comments and math. A line break stays, so the lines keep their places.
fn strip_comments_and_math(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let mut i = 0;
    let mut math = false;
    while i < chars.len() {
        let ch = chars[i];
        let next = chars.get(i + 1).copied();
        if ch == '\\' {
            // An escaped character, such as `\$`, is plain text. It is never a math sign.
            if !math {
                out.push(ch);
                out.extend(next);
            }
            i += 2;
        } else if ch == '/' && next == Some('/') && !(i > 0 && chars[i - 1] == ':') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if ch == '/' && next == Some('*') {
            i += 2;
            while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                if chars[i] == '\n' {
                    out.push('\n');
                }
                i += 1;
            }
            i += 2;
        } else if ch == '$' {
            math = !math;
            out.push(' ');
            i += 1;
        } else {
            if !math || ch == '\n' {
                out.push(ch);
            }
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_heading_and_a_line_of_text_count_their_words() {
        assert_eq!(count("= Title\nHello brave world\n"), 4);
        assert_eq!(count(""), 0);
        assert_eq!(count("- one\n- two three\n"), 3);
    }

    #[test]
    fn code_lines_add_no_words() {
        assert_eq!(count("#set text(size: 11pt)\nHello\n"), 1);
        assert_eq!(
            count("#import \"lib.typ\": template\n#let name = \"x\"\nHi\n"),
            1
        );
        assert_eq!(count("#show heading: it => it\nHi there\n"), 2);
    }

    #[test]
    fn a_code_command_that_spans_lines_skips_all_of_them() {
        let source = "#let items = (\n  \"one two\",\n  \"three\",\n)\nAfter the code\n";
        assert_eq!(count(source), 3);
    }

    #[test]
    fn comments_add_no_words() {
        assert_eq!(count("Hello // not these words\nworld\n"), 2);
        assert_eq!(count("a /* skip\nthis */ b\n"), 2);
        assert_eq!(count("See https://example.com now\n"), 3);
    }

    #[test]
    fn math_adds_no_words() {
        assert_eq!(count("The value $x + y = z$ is big\n"), 4);
        assert_eq!(count("$ sum_(i=1)^n i $\nAfter\n"), 1);
        assert_eq!(count("It costs \\$5 and \\$6\n"), 5);
    }
}
