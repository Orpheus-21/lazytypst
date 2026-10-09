//! The headings of the compiled document and their pages. `typst eval` answers this for the file as it
//! is, so the file needs no change. The editor uses it to show the page of the section of the cursor.

use std::{io, path::Path, process::Stdio};

/// A heading of the document: its text, its level, and its page, counted from 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocHeading {
    pub title: String,
    pub level: usize,
    pub page: usize,
}

/// The code that `typst eval` runs in the context of the document. It gives a list of
/// `(text, level, page)`. The text is the plain text of the content of the heading.
const EXPRESSION: &str = r#"{
  let flat(c) = if c.has("text") { c.text } else if c.has("children") { c.children.map(flat).fold("", (a, b) => a + b) } else if c.has("body") { flat(c.body) } else { " " }
  query(heading).map(h => (flat(h.body), h.level, h.location().page()))
}"#;

/// Asks Typst for the headings of the document `target`. The root is `root`.
pub fn fetch(program: &str, root: &Path, target: &Path) -> io::Result<Vec<DocHeading>> {
    let output = crate::compile::limited_command(program)
        .current_dir(root)
        .args(["eval", "--root"])
        .arg(root)
        .arg("--in")
        .arg(target)
        .arg(EXPRESSION)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        return Err(io::Error::other(
            error
                .lines()
                .next()
                .unwrap_or("typst eval failed")
                .to_string(),
        ));
    }
    parse(&String::from_utf8_lossy(&output.stdout))
        .ok_or_else(|| io::Error::other("typst eval gave text that is not a list of headings"))
}

/// Reads the JSON of the answer: a list of lists `["text", level, page]`. It reads nothing else.
pub fn parse(json: &str) -> Option<Vec<DocHeading>> {
    let mut reader = Reader {
        chars: json.chars().collect(),
        at: 0,
    };
    reader.skip_space();
    reader.expect('[')?;
    let mut headings = Vec::new();
    loop {
        reader.skip_space();
        if reader.eat(']') {
            break;
        }
        if !headings.is_empty() {
            reader.expect(',')?;
            reader.skip_space();
        }
        reader.expect('[')?;
        reader.skip_space();
        let title = reader.string()?;
        reader.skip_space();
        reader.expect(',')?;
        let level = reader.number()?;
        reader.expect(',')?;
        let page = reader.number()?;
        reader.skip_space();
        reader.expect(']')?;
        headings.push(DocHeading { title, level, page });
    }
    reader.skip_space();
    (reader.at == reader.chars.len()).then_some(headings)
}

struct Reader {
    chars: Vec<char>,
    at: usize,
}

impl Reader {
    fn skip_space(&mut self) {
        while self
            .chars
            .get(self.at)
            .is_some_and(|letter| letter.is_whitespace())
        {
            self.at += 1;
        }
    }

    fn eat(&mut self, letter: char) -> bool {
        let found = self.chars.get(self.at) == Some(&letter);
        self.at += usize::from(found);
        found
    }

    fn expect(&mut self, letter: char) -> Option<()> {
        self.eat(letter).then_some(())
    }

    fn number(&mut self) -> Option<usize> {
        self.skip_space();
        let start = self.at;
        while self.chars.get(self.at).is_some_and(char::is_ascii_digit) {
            self.at += 1;
        }
        self.chars[start..self.at]
            .iter()
            .collect::<String>()
            .parse()
            .ok()
    }

    fn string(&mut self) -> Option<String> {
        self.expect('"')?;
        let mut text = String::new();
        loop {
            let letter = *self.chars.get(self.at)?;
            self.at += 1;
            match letter {
                '"' => return Some(text),
                '\\' => {
                    let escape = *self.chars.get(self.at)?;
                    self.at += 1;
                    match escape {
                        '"' | '\\' | '/' => text.push(escape),
                        'n' => text.push('\n'),
                        't' => text.push('\t'),
                        'r' => text.push('\r'),
                        'b' | 'f' => {}
                        'u' => text.push(self.unicode()?),
                        _ => return None,
                    }
                }
                _ => text.push(letter),
            }
        }
    }

    /// The char of `\uXXXX`. A high surrogate must be followed by `\uXXXX` with a low surrogate.
    fn unicode(&mut self) -> Option<char> {
        let mut four = || -> Option<u32> {
            let digits: String = self.chars.get(self.at..self.at + 4)?.iter().collect();
            self.at += 4;
            u32::from_str_radix(&digits, 16).ok()
        };
        let first = four()?;
        if (0xD800..0xDC00).contains(&first) {
            if self.chars.get(self.at) != Some(&'\\') || self.chars.get(self.at + 1) != Some(&'u') {
                return None;
            }
            self.at += 2;
            let second = {
                let digits: String = self.chars.get(self.at..self.at + 4)?.iter().collect();
                self.at += 4;
                u32::from_str_radix(&digits, 16).ok()?
            };
            return char::from_u32(
                0x10000 + ((first - 0xD800) << 10) + (second.checked_sub(0xDC00)?),
            );
        }
        char::from_u32(first)
    }
}

/// The text for a comparison: the letters and digits of the text, in lower case. `== *Two* A` and the
/// heading `Two A` of the document give the same.
pub fn normal(text: &str) -> String {
    text.chars()
        .filter(|letter| letter.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The page of the heading number `index` of the open file. `file` has the titles of the headings of the
/// file, in order. A heading of the file is a heading of the document with the same normal text. If the
/// same text comes more than once, the n-th one in the file is the n-th one in the document (or else the
/// last one). If the titles do not match, there is no answer.
pub fn page_of(document: &[DocHeading], file: &[String], index: usize) -> Option<usize> {
    let wanted = normal(file.get(index)?);
    let rank = file[..index]
        .iter()
        .filter(|title| normal(title) == wanted)
        .count();
    let same: Vec<&DocHeading> = document
        .iter()
        .filter(|heading| normal(&heading.title) == wanted)
        .collect();
    same.get(rank).or(same.last()).map(|heading| heading.page)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heading(title: &str, level: usize, page: usize) -> DocHeading {
        DocHeading {
            title: title.into(),
            level,
            page,
        }
    }

    #[test]
    fn the_answer_of_typst_is_read() {
        assert_eq!(parse("[]"), Some(vec![]));
        assert_eq!(
            parse(r#" [["One",1,1], ["Two \"A\"\né😀",2,12 ] ] "#),
            Some(vec![heading("One", 1, 1), heading("Two \"A\"\né😀", 2, 12)])
        );
    }

    #[test]
    fn text_that_is_not_the_answer_gives_none() {
        for bad in [
            "",
            "[",
            "[[]]",
            r#"[["a",1]]"#,
            r#"[["a",1,x]]"#,
            r#"[["a",1,2],]"#,
            r#"[["\ud83d",1,2]]"#,
            "[] x",
            r#"[["a\q",1,2]]"#,
            r#"[["a",1,2"#,
        ] {
            assert_eq!(parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_heading_of_the_file_finds_its_page_in_the_document() {
        let document = vec![
            heading("One", 1, 1),
            heading("Notes", 2, 2),
            heading("Two A", 2, 3),
            heading("Notes", 2, 5),
        ];
        let file: Vec<String> = ["One", "*Two* A", "Notes", "Notes"]
            .map(String::from)
            .to_vec();
        assert_eq!(page_of(&document, &file, 0), Some(1));
        assert_eq!(
            page_of(&document, &file, 1),
            Some(3),
            "markup does not matter"
        );
        assert_eq!(page_of(&document, &file, 2), Some(2), "the first Notes");
        assert_eq!(page_of(&document, &file, 3), Some(5), "the second Notes");
        assert_eq!(page_of(&document, &["Missing".to_string()], 0), None);
        assert_eq!(page_of(&document, &file, 9), None);
        // A third `Notes` in the file with two in the document gets the last one.
        let more: Vec<String> = ["Notes", "Notes", "Notes"].map(String::from).to_vec();
        assert_eq!(page_of(&document, &more, 2), Some(5));
    }

    #[test]
    fn typst_gives_the_headings_of_a_file_without_a_change_of_the_file() {
        let dir = std::env::temp_dir().join(format!("lazytypst-outline-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("doc.typ");
        std::fs::write(
            &file,
            "#set page(width: 8cm, height: 5cm)\n= One\n#lorem(30)\n#pagebreak()\n== Two *A*\n#lorem(40)\n#pagebreak()\n= Three\n",
        )
        .unwrap();
        let headings = fetch(&crate::compile::typst_program(), &dir, &file).unwrap();
        let pages: Vec<usize> = headings.iter().map(|heading| heading.page).collect();
        let titles: Vec<&str> = headings
            .iter()
            .map(|heading| heading.title.as_str())
            .collect();
        assert_eq!(
            titles,
            ["One", "Two A", "Three"],
            "the text of a bold heading is its plain text"
        );
        assert_eq!(pages, [1, 2, 4]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
