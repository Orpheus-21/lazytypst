use std::sync::mpsc::{self, Receiver, TryRecvError};

use super::*;
use crate::outline::{self, DocHeading};

/// The state of the preview that follows the cursor.
pub(super) struct Follow {
    /// True when the preview goes to the page of the section of the cursor. `F3` switches it.
    pub on: bool,
    /// The headings of the compiled document, with their pages.
    pub document: Vec<DocHeading>,
    /// The headings of the open file when `document` was asked for. A change of them asks again.
    signature: Option<String>,
    /// The running question to Typst.
    pub job: Option<Receiver<std::io::Result<Vec<DocHeading>>>>,
    /// The line of the cursor at the last look.
    last_row: Option<usize>,
    /// The line of the heading of the section at the last jump. The preview jumps when this changes.
    last_section: Option<usize>,
}

impl Follow {
    pub fn new() -> Self {
        Self {
            on: false,
            document: Vec::new(),
            signature: None,
            job: None,
            last_row: None,
            last_section: None,
        }
    }
}

impl Editor {
    pub fn follow_on(&self) -> bool {
        self.follow.on
    }

    pub fn set_follow(&mut self, on: bool) {
        self.follow.on = on;
    }

    /// `F3`: the preview follows the cursor, or not.
    pub(super) fn toggle_follow(&mut self) {
        self.follow.on = !self.follow.on;
        self.follow.last_section = None;
        self.message = if self.follow.on {
            "The preview follows the cursor".into()
        } else {
            self.follow.job = None;
            "The preview stays on its page".into()
        };
    }

    /// The titles of the headings of the open file, in order, with the line of each.
    fn file_headings(&self) -> (Vec<usize>, Vec<String>) {
        self.headings()
            .into_iter()
            .map(|(row, label)| (row, label.trim().to_string()))
            .unzip()
    }

    /// Asks Typst for the headings of the document after a good compile, and goes to the page of the section
    /// of the cursor when the cursor enters another section. Returns true when the screen must redraw.
    pub(super) fn poll_follow(&mut self) -> bool {
        if !self.follow.on {
            return false;
        }
        let mut changed = false;
        if let Some(receiver) = &self.follow.job {
            match receiver.try_recv() {
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => self.follow.job = None,
                Ok(result) => {
                    self.follow.job = None;
                    if let Ok(document) = result {
                        self.follow.document = document;
                        changed |= self.follow_check(true);
                    }
                }
            }
        }
        let good = self.report.as_ref().is_some_and(|report| report.ok)
            && !self.compile_busy()
            && self.compile_at.is_none()
            && self.last_edit.is_none();
        if good && self.follow.job.is_none() {
            let signature = self.file_headings().1.join("\n");
            if self.follow.signature.as_ref() != Some(&signature) {
                self.follow.signature = Some(signature);
                let (program, root) = (compile::typst_program(), self.root.clone());
                let target = self.compile_target().to_path_buf();
                let (sender, receiver) = mpsc::channel();
                std::thread::spawn(move || {
                    let _ = sender.send(outline::fetch(&program, &root, &target));
                });
                self.follow.job = Some(receiver);
            }
        }
        let row = self.cursor_position().0;
        if self.follow.last_row != Some(row) {
            self.follow.last_row = Some(row);
            changed |= self.follow_check(false);
        }
        changed
    }

    /// Goes to the page of the section of the cursor, if the section is not the one of the last jump (or
    /// `force`) and the page is not the wanted page. A heading that the document does not have gives
    /// nothing.
    fn follow_check(&mut self, force: bool) -> bool {
        let (rows, titles) = self.file_headings();
        let here = self.cursor_position().0;
        let Some(index) = rows.iter().rposition(|row| *row <= here) else {
            return false;
        };
        if !force && self.follow.last_section == Some(rows[index]) {
            return false;
        }
        self.follow.last_section = Some(rows[index]);
        let Some(page) = outline::page_of(&self.follow.document, &titles, index) else {
            return false;
        };
        if page == self.preview.wanted_page() {
            return false;
        }
        self.preview.want(page);
        self.start_compile();
        true
    }
}
