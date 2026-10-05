# lazytypst tasks

## Task 1: Project skeleton and quit key
**Description:** Create the Rust project. Draw an empty screen. Quit with `q`. Restore the terminal on exit and on panic.
**Acceptance criteria:**
- [ ] `cargo run` shows a screen with a title.
- [ ] `q` exits and the terminal is normal again.
**Verification:**
- [ ] `cargo build` passes.
- [ ] Manual: run, press `q`, type `ls`. The shell works.
**Dependencies:** None
**Files likely touched:** `Cargo.toml`, `src/main.rs`
**Estimated scope:** Small

## Task 2: Project browser
**Description:** Walk the root folder to depth 3. List each `.typ` file. Move with `j` and `k`. Enter opens the file.
**Acceptance criteria:**
- [ ] The list shows the relative path of each `.typ` file.
- [ ] `j`, `k`, and Enter work.
- [ ] An empty folder shows the text "No .typ files".
**Verification:**
- [ ] `cargo test` passes for the walk function (one test with a temporary folder).
- [ ] Manual: run in `~/Work/typst`.
**Dependencies:** Task 1
**Files likely touched:** `src/main.rs`, `src/browser.rs`
**Estimated scope:** Small

## Checkpoint: After Tasks 1-2
- [ ] Build passes. Browser works. Review with the user.

## Task 3: Text editor with save
**Description:** Enter opens the file in a `ratatui-textarea` widget. `Ctrl-S` saves. `Esc` goes back to the browser.
**Acceptance criteria:**
- [ ] The file text shows in the editor.
- [ ] `Ctrl-S` writes the file. The bytes on disk match the buffer.
- [ ] `Esc` returns to the browser. Unsaved text shows a warning.
**Verification:**
- [ ] Manual: edit a file, save, run `cat` on the file.
**Dependencies:** Task 2
**Files likely touched:** `src/main.rs`, `src/editor.rs`
**Estimated scope:** Small

## Task 4: Compile and error pane
**Description:** `Ctrl-B` saves, then runs `typst compile <file> <tmp>/page-{p}.png --diagnostic-format short`. The pane under the editor shows the output lines.
**Acceptance criteria:**
- [ ] A valid file shows "OK".
- [ ] A file with an error shows the error line with its line number.
- [ ] The UI does not freeze during the compile.
**Verification:**
- [ ] `cargo test` passes for the function that builds the command.
- [ ] Manual: add a bad `#let` line. The pane shows the error.
**Dependencies:** Task 3
**Files likely touched:** `src/compile.rs`, `src/main.rs`
**Estimated scope:** Medium

## Checkpoint: After Tasks 3-4
- [ ] Open, edit, save, and compile work. Errors show. Review with the user.

## Task 5: Show the PNG page
**Description:** After a good compile, show `page-1.png` in a pane next to the editor with `ratatui-image`.
**Acceptance criteria:**
- [ ] Page 1 shows right of the editor.
- [ ] If the terminal has no image protocol, the pane shows half blocks.
- [ ] A new compile replaces the old image.
**Verification:**
- [ ] Manual: compile a file with big text. The text is readable.
**Dependencies:** Task 4
**Files likely touched:** `src/preview.rs`, `src/main.rs`, `Cargo.toml`
**Estimated scope:** Medium

## Task 6: Live update
**Description:** Run the save and compile 300 ms after the last key. Kill the old compile if a new one starts.
**Acceptance criteria:**
- [ ] The preview changes about 300 ms after the user stops typing.
- [ ] Ten fast keys cause one compile, not ten.
- [ ] A slow old compile never replaces a newer image.
**Verification:**
- [ ] `cargo test` passes for the debounce logic (a fake clock).
- [ ] Manual: type fast. Check with `pgrep typst` that one process runs at most.
**Dependencies:** Task 5
**Files likely touched:** `src/main.rs`, `src/compile.rs`
**Estimated scope:** Medium

## Checkpoint: After Tasks 5-6
- [ ] The live preview works end to end. Review with the user.

## Task 7: Page keys and PDF export
**Description:** `n` and `p` change the preview page. `Ctrl-E` compiles a PDF next to the `.typ` file.
**Acceptance criteria:**
- [ ] `n` and `p` stop at the first and last page.
- [ ] `Ctrl-E` creates `<name>.pdf`. The pane shows the path.
**Verification:**
- [ ] Manual: use a file with 3 pages.
**Dependencies:** Task 6
**Files likely touched:** `src/preview.rs`, `src/compile.rs`
**Estimated scope:** Small
