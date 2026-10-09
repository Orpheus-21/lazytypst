use std::{
    collections::HashSet,
    sync::mpsc::{self, Receiver, TryRecvError},
};

use super::*;
use crate::spell::{self, Misses, Word};

/// What the user can do with a misspelled word in the list of `Alt-;`.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Choice {
    /// Replace the word with this text.
    Replace(String),
    /// Add the word to the personal dictionary.
    Add,
    /// Do not mark the word again in this run.
    Ignore,
}

/// The state of the spell check.
pub(super) struct Spelling {
    /// True when the check is on. `F7` switches it.
    pub on: bool,
    /// The program that checks the words. Tests put a fake program here.
    pub program: String,
    /// The file of the personal dictionary. `None`: the words stay in this run only.
    pub personal_file: Option<PathBuf>,
    /// The words of the personal dictionary and the words to ignore in this run, in lower case.
    pub known: HashSet<String>,
    /// The prose words of the last finished check, and the unknown words with their suggestions.
    words: Vec<Word>,
    misses: Misses,
    /// For each line, the characters `start..end` of the words to underline.
    pub marks: Vec<Vec<(usize, usize)>>,
    /// The version of the text that `words` and `marks` belong to.
    marks_version: u64,
    /// The running check: the version of the text it reads, its words, and its answer.
    pub job: Option<(u64, Vec<Word>, Receiver<std::io::Result<Misses>>)>,
    /// The version of the text of the last check, also a failed one. A failed check is not repeated.
    pub checked_version: Option<u64>,
    /// The word of the open list.
    pub target: Option<Word>,
    /// The place where the last `Alt-;` put the cursor. At that place, the next `Alt-;` goes on to the
    /// next word. Anywhere else, a word at the cursor counts.
    last_jump: Option<(usize, usize)>,
}

impl Spelling {
    pub fn new() -> Self {
        Self {
            on: false,
            program: spell::program(),
            personal_file: None,
            known: HashSet::new(),
            words: Vec::new(),
            misses: Misses::new(),
            marks: Vec::new(),
            marks_version: 0,
            job: None,
            checked_version: None,
            target: None,
            last_jump: None,
        }
    }

    /// The characters to underline in `line`, or none while the text is newer than the check.
    pub fn marks_of(&self, line: usize, version: u64) -> &[(usize, usize)] {
        if self.marks_version != version {
            return &[];
        }
        self.marks.get(line).map_or(&[], Vec::as_slice)
    }

    /// Makes the marks from the words and the misses, without the words that the user knows.
    fn remark(&mut self, line_count: usize) {
        let mut marks = vec![Vec::new(); line_count];
        for word in &self.words {
            if self.misses.contains_key(&word.text)
                && !self.known.contains(&word.text.to_lowercase())
                && let Some(line) = marks.get_mut(word.line)
            {
                line.push((word.start, word.end));
            }
        }
        self.marks = marks;
    }

    /// The word that starts after the place (`line`, `column`), or else the first one: the next unknown
    /// word, with a wrap at the end.
    fn next_after(&self, version: u64, line: usize, column: usize) -> Option<&Word> {
        let strict = self.last_jump == Some((line, column));
        if self.marks_version != version {
            return None;
        }
        let unknown = |word: &&Word| {
            self.misses.contains_key(&word.text) && !self.known.contains(&word.text.to_lowercase())
        };
        self.words
            .iter()
            .filter(unknown)
            .find(|word| {
                let place = (word.line, word.start);
                place > (line, column) || (!strict && place == (line, column))
            })
            .or_else(|| self.words.iter().find(unknown))
    }
}

impl Editor {
    /// Says whether the spell check is on, for the program to keep.
    pub fn spell_on(&self) -> bool {
        self.spelling.on
    }

    pub fn set_spell(&mut self, on: bool, personal_file: Option<PathBuf>) {
        self.spelling.on = on;
        self.spelling.known = personal_file
            .as_deref()
            .map(spell::load_personal)
            .unwrap_or_default();
        self.spelling.personal_file = personal_file;
    }

    /// `F7`: the check on, or off.
    pub(super) fn toggle_spell(&mut self) {
        self.spelling.on = !self.spelling.on;
        self.spelling.checked_version = None;
        self.message = if self.spelling.on {
            "Spell check on. Alt-; goes to the next misspelled word.".into()
        } else {
            self.spelling.job = None;
            "Spell check off".into()
        };
    }

    /// Starts a check when the text changed and the user paused, and takes the answer of a running check.
    /// Returns true when the screen must redraw.
    pub(super) fn poll_spell(&mut self) -> bool {
        if !self.spelling.on {
            return false;
        }
        let mut changed = false;
        if let Some((version, words, receiver)) = &self.spelling.job {
            match receiver.try_recv() {
                Err(TryRecvError::Empty) => return false,
                Err(TryRecvError::Disconnected) => self.spelling.job = None,
                Ok(result) => {
                    let (version, words) = (*version, words.clone());
                    self.spelling.job = None;
                    match result {
                        Ok(misses) if version == self.text_version => {
                            self.spelling.words = words;
                            self.spelling.misses = misses;
                            self.spelling.marks_version = version;
                            let lines = self.textarea.lines().len();
                            self.spelling.remark(lines);
                        }
                        Ok(_) => {} // the text changed while the check ran: the next tick checks again
                        Err(err) => {
                            self.message = err.to_string();
                        }
                    }
                    self.spelling.checked_version = Some(version);
                    changed = true;
                }
            }
        }
        if self.spelling.job.is_none()
            && self.last_edit.is_none()
            && self.spelling.checked_version != Some(self.text_version)
        {
            self.start_spell_check();
        }
        changed
    }

    fn start_spell_check(&mut self) {
        let lines = self.textarea.lines();
        let parts = highlight::tokenize(lines);
        let words = spell::prose_words(lines, &parts);
        let mut unique: Vec<String> = words.iter().map(|word| word.text.clone()).collect();
        unique.sort();
        unique.dedup();
        let (program, dict) = (self.spelling.program.clone(), spell::dictionary(lines));
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let result = spell::check(&program, &dict, &unique).map_err(|err| {
                std::io::Error::new(err.kind(), spell::failure(&program, &dict, &err))
            });
            let _ = sender.send(result);
        });
        self.spelling.job = Some((self.text_version, words, receiver));
    }

    /// `Alt-;`: goes to the next misspelled word, and opens the list of what to do with it.
    pub(super) fn open_spell(&mut self) {
        if !self.spelling.on {
            self.message = "Spell check is off. F7 turns it on.".into();
            return;
        }
        let (line, column) = self.cursor_position();
        let Some(word) = self
            .spelling
            .next_after(self.text_version, line, column)
            .cloned()
        else {
            self.message = if self.spelling.job.is_some()
                || self.spelling.checked_version != Some(self.text_version)
            {
                "The check is not ready. Try again in a moment.".into()
            } else {
                "No misspelled words".into()
            };
            return;
        };
        self.set_cursor_position((word.line, word.start));
        self.spelling.last_jump = Some((word.line, word.start));
        let suggestions = self
            .spelling
            .misses
            .get(&word.text)
            .cloned()
            .unwrap_or_default();
        let mut entries: Vec<(Choice, String)> = suggestions
            .into_iter()
            .map(|suggestion| (Choice::Replace(suggestion.clone()), suggestion))
            .collect();
        entries.push((
            Choice::Add,
            format!("Add \"{}\" to the dictionary", word.text),
        ));
        entries.push((
            Choice::Ignore,
            format!("Ignore \"{}\" in this run", word.text),
        ));
        let title = format!("Spelling: {}", word.text);
        self.spelling.target = Some(word);
        self.mode = Mode::Spell(Box::new(Switcher::new(
            title,
            "Enter chooses  Esc closes  Alt-; next word",
            entries,
            false,
        )));
    }

    /// The keys of the list of a misspelled word.
    pub(super) fn spell_key(&mut self, key: KeyEvent) {
        let Mode::Spell(popup) = &mut self.mode else {
            return;
        };
        let outcome = popup.key(key);
        match outcome {
            Outcome::Stay => {}
            Outcome::Close => self.mode = Mode::Edit,
            Outcome::Pick(choice) => {
                self.mode = Mode::Edit;
                let Some(word) = self.spelling.target.take() else {
                    return;
                };
                self.apply_choice(&word, choice);
            }
        }
    }

    fn apply_choice(&mut self, word: &Word, choice: Choice) {
        match choice {
            Choice::Replace(text) => {
                let before = self.textarea.lines().to_vec();
                self.textarea.cancel_selection();
                self.set_cursor_position((word.line, word.start));
                self.textarea.delete_str(word.end - word.start);
                self.textarea.insert_str(&text);
                self.note_replace(before);
                self.mark_edit();
            }
            Choice::Add | Choice::Ignore => {
                self.spelling.known.insert(word.text.to_lowercase());
                self.message = if choice == Choice::Add {
                    match self
                        .spelling
                        .personal_file
                        .as_deref()
                        .map(|file| spell::add_personal(file, &word.text))
                    {
                        Some(Err(err)) => format!("Cannot save the word: {err}"),
                        _ => format!("Added \"{}\" to the dictionary", word.text),
                    }
                } else {
                    format!("Ignoring \"{}\" in this run", word.text)
                };
                let lines = self.textarea.lines().len();
                self.spelling.remark(lines);
            }
        }
    }
}
