//! Spell check with `hunspell`, an optional program like `typst`. The program finds the words of the prose
//! of a file, asks `hunspell -a` about each different word in a thread, and gives back the words that it
//! does not know, with its suggestions. The editor underlines them. The dictionary comes from the language
//! of the document (`#set text(lang: "de")`) or from the environment.

use std::{
    collections::{HashMap, HashSet},
    io::{self, Write},
    path::Path,
    process::{Command, Stdio},
};

use crate::highlight::{Kind, Part};

/// The most suggestions that the editor shows for a word.
pub const MAX_SUGGESTIONS: usize = 8;

/// The program that checks the words: the variable `LAZYTYPST_SPELL`, or else `hunspell`.
pub fn program() -> String {
    std::env::var("LAZYTYPST_SPELL")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "hunspell".into())
}

/// A word of the prose: its line, and its characters `start..end` in the line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub text: String,
}

fn is_letter(letter: char) -> bool {
    letter.is_alphabetic()
}

/// True for a line that is code: it starts with `#` and a keyword, such as `#set text(font: "x")`.
fn is_code_line(line: &str) -> bool {
    let line = line.trim_start();
    [
        "#set",
        "#show",
        "#let",
        "#import",
        "#include",
        "#context",
        "#pagebreak",
    ]
    .iter()
    .any(|word| line.starts_with(word))
}

/// For each char: true if it is in an address, a run of non-space chars that holds `://`.
fn address_marks(chars: &[char]) -> Vec<bool> {
    let mut marks = vec![false; chars.len()];
    let mut start = 0;
    while start < chars.len() {
        if chars[start].is_whitespace() {
            start += 1;
            continue;
        }
        let end = chars[start..]
            .iter()
            .position(|letter| letter.is_whitespace())
            .map_or(chars.len(), |length| start + length);
        let token: String = chars[start..end].iter().collect();
        if token.contains("://") {
            marks[start..end].fill(true);
        }
        start = end;
    }
    marks
}

/// The words of the prose of the lines. A word is a run of letters, with an apostrophe inside it. The
/// prose is the text of the lines and of the headings. These are not prose: comments, math, raw text,
/// strings, names after `#`, a line of code, a word with a digit, a word after `#`, `@`, `<`, or `\`, and
/// the address that starts with `http`.
pub fn prose_words(lines: &[String], parts: &[Vec<Part>]) -> Vec<Word> {
    let mut words = Vec::new();
    for (line_number, line) in lines.iter().enumerate() {
        if is_code_line(line) {
            continue;
        }
        let chars: Vec<char> = line.chars().collect();
        let in_address = address_marks(&chars);
        let skipped = |offset: usize| {
            parts[line_number]
                .iter()
                .any(|part| part.start <= offset && offset < part.end && part.kind != Kind::Heading)
        };
        let mut index = 0;
        while index < chars.len() {
            if !is_letter(chars[index]) {
                index += 1;
                continue;
            }
            let start = index;
            while index < chars.len()
                && (is_letter(chars[index])
                    || (matches!(chars[index], '\'' | '’')
                        && index + 1 < chars.len()
                        && is_letter(chars[index + 1])
                        && index > start)
                    || chars[index].is_ascii_digit())
            {
                index += 1;
            }
            let word: String = chars[start..index].iter().collect();
            let before = start.checked_sub(1).map(|at| chars[at]);
            let has_digit = word.chars().any(|letter| letter.is_ascii_digit());
            if skipped(start)
                || has_digit
                || in_address[start]
                || before.is_some_and(|letter| matches!(letter, '#' | '@' | '<' | '\\' | '_'))
                || index - start < 2
            {
                continue;
            }
            words.push(Word {
                line: line_number,
                start,
                end: index,
                text: word,
            });
        }
    }
    words
}

/// The dictionary name for a language code of Typst, such as `en`, `de`, or `pt-BR`. A code with a region
/// becomes `pt_BR`. A code without one gets the usual region. The user can set `LAZYTYPST_SPELL_DICT`.
pub fn dictionary_for(lang: &str) -> String {
    let lang = lang.trim().replace('-', "_");
    if lang.contains('_') {
        let (name, region) = lang.split_once('_').unwrap_or((&lang, ""));
        return format!("{}_{}", name.to_lowercase(), region.to_uppercase());
    }
    let region = match lang.to_lowercase().as_str() {
        "en" => "US",
        "de" => "DE",
        "fr" => "FR",
        "es" => "ES",
        "it" => "IT",
        "pt" => "PT",
        "nl" => "NL",
        "ru" => "RU",
        "pl" => "PL",
        "sv" => "SE",
        "da" => "DK",
        "cs" => "CZ",
        "uk" => "UA",
        _ => return lang.to_lowercase(),
    };
    format!("{}_{region}", lang.to_lowercase())
}

/// The language code of the first `lang: "xx"` in the text, or `None`.
pub fn lang_of(lines: &[String]) -> Option<String> {
    lines.iter().find_map(|line| {
        let at = line.find("lang:")?;
        let rest = line[at + 5..].trim_start().strip_prefix('"')?;
        let code = &rest[..rest.find('"')?];
        (!code.is_empty()
            && code
                .chars()
                .all(|letter| letter.is_ascii_alphabetic() || letter == '-'))
        .then(|| code.to_string())
    })
}

/// The dictionary for the text: `LAZYTYPST_SPELL_DICT`, else the language of the document, else the
/// language of the system (`LANG`), else `en_US`.
pub fn dictionary(lines: &[String]) -> String {
    if let Some(name) = std::env::var("LAZYTYPST_SPELL_DICT")
        .ok()
        .filter(|name| !name.is_empty())
    {
        return name;
    }
    if let Some(lang) = lang_of(lines) {
        return dictionary_for(&lang);
    }
    std::env::var("LANG")
        .ok()
        .and_then(|lang| lang.split('.').next().map(str::to_string))
        .filter(|lang| lang.len() >= 5 && lang.contains('_'))
        .unwrap_or_else(|| "en_US".into())
}

/// What a check found: each unknown word with its suggestions.
pub type Misses = HashMap<String, Vec<String>>;

/// Why a check could not run, in words for the status line.
pub fn failure(program: &str, dict: &str, err: &io::Error) -> String {
    if err.kind() == io::ErrorKind::NotFound {
        format!(
            "Spell check is off: {program} is not installed. Install hunspell and a dictionary."
        )
    } else {
        format!("Spell check failed: {err} (dictionary {dict}).")
    }
}

/// Asks `program` (in the mode of `hunspell -a`) about each word, with the dictionary `dict`. It returns
/// the unknown words with up to `MAX_SUGGESTIONS` suggestions. Each word goes on its own line, with a `^`
/// in front, so that the program never reads a word as a command.
pub fn check(program: &str, dict: &str, words: &[String]) -> io::Result<Misses> {
    let mut child = Command::new(program)
        .args(["-d", dict, "-i", "utf-8", "-a"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take().expect("stdin is piped");
    let input: String = words.iter().map(|word| format!("^{word}\n")).collect();
    // A thread writes, so a full pipe cannot hold the program while it waits for the output.
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let output = child.wait_with_output()?;
    let _ = writer.join();
    if !output.status.success() && output.stdout.is_empty() {
        let error = String::from_utf8_lossy(&output.stderr);
        return Err(io::Error::other(
            error.lines().next().unwrap_or("it failed").to_string(),
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut results = text
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('@'));
    let mut misses = Misses::new();
    for word in words {
        let Some(result) = results.next() else {
            break;
        };
        match result.chars().next() {
            Some('&') => {
                // `& word count offset: first, second, third`
                let suggestions = result
                    .split_once(": ")
                    .map(|(_, list)| {
                        list.split(", ")
                            .take(MAX_SUGGESTIONS)
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
                misses.insert(word.clone(), suggestions);
            }
            Some('#' | '?') => {
                misses.insert(word.clone(), Vec::new());
            }
            _ => {}
        }
    }
    if misses.is_empty() && words.len() > 1 && !output.stderr.is_empty() && text.lines().count() < 2
    {
        // The program printed no answer: for example, the dictionary is missing.
        let error = String::from_utf8_lossy(&output.stderr);
        return Err(io::Error::other(
            error
                .lines()
                .next()
                .unwrap_or("it gave no answer")
                .to_string(),
        ));
    }
    Ok(misses)
}

/// The personal dictionary: one word on each line. A missing file is an empty dictionary.
pub fn load_personal(file: &Path) -> HashSet<String> {
    std::fs::read_to_string(file)
        .map(|text| {
            text.lines()
                .map(|line| line.trim().to_lowercase())
                .filter(|line| !line.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Adds `word` to the personal dictionary file.
pub fn add_personal(file: &Path, word: &str) -> io::Result<()> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut old = std::fs::read(file).unwrap_or_default();
    if !old.is_empty() && !old.ends_with(b"\n") {
        old.push(b'\n');
    }
    old.extend_from_slice(word.as_bytes());
    old.push(b'\n');
    crate::fsutil::write_file(file, &old)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::highlight::tokenize;
    use std::os::unix::fs::PermissionsExt;

    fn words_of(text: &str) -> Vec<String> {
        let lines: Vec<String> = text.lines().map(String::from).collect();
        let parts = tokenize(&lines);
        prose_words(&lines, &parts)
            .into_iter()
            .map(|word| word.text)
            .collect()
    }

    #[test]
    fn only_prose_words_are_found() {
        assert_eq!(
            words_of("= A heading here\nPlain text, with don't and naïve."),
            [
                "heading", "here", "Plain", "text", "with", "don't", "and", "naïve"
            ]
        );
        // Code, comments, math, raw text, strings, labels, references, numbers, and addresses are not prose.
        let text = "#set text(font: \"Libertinus\")\nWord // a comment here\nmath $x plus y$ done\n\
                    `raw code` and #image(\"foo.png\") then <label> @ref abc123 see https://exampel.org/pagge ok\n\
                    ```\nraw block\n```\n/* block\ncomment */ last";
        assert_eq!(
            words_of(text),
            ["Word", "math", "done", "and", "then", "see", "ok", "last"]
        );
    }

    #[test]
    fn a_word_has_its_place_in_the_line() {
        let lines = vec!["héllo wörld".to_string()];
        let words = prose_words(&lines, &tokenize(&lines));
        assert_eq!((words[1].start, words[1].end, words[1].line), (6, 11, 0));
    }

    #[test]
    fn the_dictionary_comes_from_the_language_of_the_document() {
        assert_eq!(dictionary_for("en"), "en_US");
        assert_eq!(dictionary_for("de"), "de_DE");
        assert_eq!(dictionary_for("pt-br"), "pt_BR");
        assert_eq!(dictionary_for("en-GB"), "en_GB");
        assert_eq!(dictionary_for("xx"), "xx");
        let lines = |text: &str| text.lines().map(String::from).collect::<Vec<_>>();
        assert_eq!(
            lang_of(&lines("#set text(lang: \"fr\", size: 11pt)")).as_deref(),
            Some("fr")
        );
        assert_eq!(
            lang_of(&lines("#set text(lang: \"zh-Hans\")")).as_deref(),
            Some("zh-Hans")
        );
        assert_eq!(lang_of(&lines("no language")), None);
        assert_eq!(lang_of(&lines("#set text(lang: \"\")")), None);
    }

    /// A fake `hunspell`: it knows every word except those that start with `zz`. For these it gives two
    /// suggestions. The dictionary `nodict` fails as hunspell does.
    fn fake_hunspell(name: &str) -> (std::path::PathBuf, String) {
        let dir =
            std::env::temp_dir().join(format!("lazytypst-spell-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("hunspell");
        std::fs::write(
            &script,
            "#!/bin/sh\nif [ \"$2\" = nodict ]; then echo \"Can't open affix or dictionary files for dictionary named nodict.\" >&2; exit 1; fi\n\
             echo '@(#) International Ispell Version 3.2.06 (but really fake)'\n\
             while read -r line; do\n  w=${line#^}\n  case \"$w\" in\n    zzq) echo \"# $w 0\";;\n    zz*) echo \"& $w 2 0: fixa, fixb\";;\n    *) echo '*';;\n  esac\n  echo\ndone\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let program = script.display().to_string();
        (dir, program)
    }

    #[test]
    fn the_check_returns_the_unknown_words_with_their_suggestions() {
        let (dir, program) = fake_hunspell("check");
        let words: Vec<String> = ["hello", "zzabc", "world", "zzq"]
            .map(String::from)
            .to_vec();
        let misses = check(&program, "en_US", &words).unwrap();
        assert_eq!(misses.len(), 2);
        assert_eq!(misses["zzabc"], ["fixa", "fixb"]);
        assert!(misses["zzq"].is_empty(), "no suggestion");
        assert!(check(&program, "en_US", &[]).unwrap().is_empty());
        let err = check(&program, "nodict", &words).unwrap_err();
        assert!(err.to_string().contains("dictionary"), "{err}");
        let missing = check("/nonexistent/hunspell", "en_US", &words).unwrap_err();
        assert!(failure("hunspell", "en_US", &missing).contains("not installed"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_personal_dictionary_keeps_words_in_lower_case() {
        let dir =
            std::env::temp_dir().join(format!("lazytypst-spell-personal-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("sub").join("words.txt");
        assert!(load_personal(&file).is_empty());
        add_personal(&file, "Lazytypst").unwrap();
        add_personal(&file, "Oruhi").unwrap();
        let words = load_personal(&file);
        assert!(words.contains("lazytypst") && words.contains("oruhi") && words.len() == 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn finding_the_words_of_a_long_file_is_fast() {
        let text =
            "= A heading of text\nSome prose with several words, and some more words to read.\n"
                .repeat(2500);
        let lines: Vec<String> = text.lines().map(String::from).collect();
        let parts = tokenize(&lines);
        let start = std::time::Instant::now();
        let words = prose_words(&lines, &parts);
        let took = start.elapsed();
        assert!(words.len() > 30_000);
        // 5000 lines: about 10 ms in a release build. The limit is loose, for a slow debug build.
        assert!(took < std::time::Duration::from_millis(500), "{took:?}");
    }
}
