# Changelog

This file lists the changes that a user of lazytypst can see. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). The version numbers follow [Semantic Versioning](https://semver.org/). Before version 1.0.0, a change in the middle number can change how the program works.

Each change that a user can see adds one line under `Unreleased`, in the section Added, Changed, Fixed, or Removed. On release day, those lines move to a new entry that has the version and the date.

## [Unreleased]

### Added

- The search takes plain text by default (`f(x)` and `1+1` work), with smart case. `Alt-R` switches to a regular expression.
- A folder with a `typst.toml` entrypoint or a `main.typ` gets that file as its main file by itself. `m` removes the mark, and the choice stays.
- `Alt-S` replaces the text of the last search: `Enter` replaces one match and goes to the next, `Alt-A` replaces all. One `Ctrl-Z` takes back one replace or all.
- The preview follows the files that the document reads. If another program changes an included file, an image, or a bibliography, a compile starts and the status line names the file.
- At start, the program deletes its own leftover temp files (`.name.<pid>-...lazytypst-tmp` and `.lazytypst-<pid>-<n>.pdf.tmp`) beside the `.typ` files, if they are older than 1 hour and their process is gone.
- `Tab`, `Shift-Tab`, and the indent after `Enter` follow the indent of the file: a tab character, or 2, 3, 4, or 8 spaces.
- `lazytypst main.typ:42` and `main.typ:42:7` open the file with the cursor at that place.
- A window under 60 columns or 16 rows shows "Make the window bigger (60x16)". The state stays.
- In tmux with half blocks, the status line and `--doctor` hint `set -g allow-passthrough on`.
- `Alt-G` goes to a line, or to a line and a column.
- `Ctrl-Z` undoes, `Alt-A` selects all the text, and `Ctrl-Home` and `Ctrl-End` go to the start and the end of the file (with `Shift`, they select).
- The status line shows the heading of the section of the cursor, when the window is wide enough.
- The list sorts by path without case, and counts a number by its value: `chapter-2.typ` comes before `chapter-10.typ`.
- The gutter marks the lines with an error (red) and with a warning (yellow).
- `F8` and `Shift-F8` go to the next and the previous error.
- The title of the preview says `(old)` while the last compile failed, because the page is then from an older compile.
- `Shift-Tab` removes one indent level. Before, it added spaces.
- `Enter` keeps the indent of the line, and adds one level after `(`, `[`, or `{`.
- `lazytypst --keys` prints all keys as lines of tab separated text, for scripts.
- The page folder is made in `$XDG_RUNTIME_DIR` when it is a folder of the user, and in the temporary directory if not. The next start also deletes stale folders of older versions in the temporary directory.
- The release archive has a build attestation. `gh attestation verify` checks that GitHub built it from this repository. The release is built with a fixed version of Rust.

## [0.1.0] (2026-10-09)

The first version. It needs Typst 0.12.0 or later in `PATH`, and it is tested with Typst 0.15.1. It needs Rust 1.90 or later to build. It is tested on Linux, with the kitty image protocol in Ghostty and with half blocks. The sixel and iTerm2 protocols are not tested.

### Added

- A list of the `.typ` files in a folder, up to three folders deep. Hidden folders are skipped.
- A folder with exactly one `.typ` file opens that file in the editor at start. `Esc` shows the list.
- Keys in the list: `j` and `k` or the arrow keys, `g` and `G` or `Home` and `End`, `Enter` to open a file, and `q` to quit.
- `/` filters the list while you type. The match ignores case. `Esc` removes the filter.
- `r` reads the folder again, to show new files and to drop deleted files.
- `n` makes a new `.typ` file from a path such as `chapters/two`. The program makes missing folders and refuses a name that leaves the project.
- The list selects the file that you closed last time in the folder. Opening it shows the page of the preview that was open. The program keeps this outside the project folder.
- `s` switches the order of the list between the path and the newest change first.
- Each line of the list shows the time since the last change, such as `2 h`. A narrow terminal hides the ages.
- `e` in the list edits the selected file in `VISUAL` or `EDITOR`, and then opens it with a compile.
- `E` in the list exports the PDF of the selected file.
- `y` copies the absolute path of the selected file to the system clipboard.
- `m` marks a main file. The live compile, `Ctrl-B`, and `Ctrl-E` then use the main file, whatever file you edit. The program remembers the main file of each project, and it keeps that choice outside the project folder.
- A folder or a `.typ` file as the argument. A file opens in the editor at once, and its folder is the project root. Files in subfolders can import files from parent folders inside the root.
- `Left`, `Right`, `Backspace`, and `Delete` work on one visible character, such as a Devanagari conjunct or an emoji with joiners.
- The status line shows an approximate word count. It skips comments, math, and lines of code such as `#set` and `#import`.
- Syntax color in the editor for headings, commands after `#`, strings in code, math, raw text, and comments. The colors come from the terminal palette.
- A text area with line numbers, long lines that wrap, and Emacs style editing keys. `Tab` inserts 2 spaces. The status line shows the cursor as `line:column`. If you close a file and open it again, the cursor comes back.
- A compile starts when a file opens, so the preview shows at once.
- An autosave 300 ms after the last key, and a live compile after each save. A new compile stops the compile that still runs.
- A reload of the file when another program changes it and the text has no edits. The cursor keeps its line, and a compile starts.
- A safe save. The program writes the file only when the text has edits. It does not overwrite a change that another program made, until you press `Ctrl-S`. It writes a temp file and renames it, so a crash cannot cut the file. A symlink stays a symlink, and the permissions stay.
- A paste arrives as one piece. A long paste is fast, one undo takes it back, and a tab character in it stays a tab character.
- A live preview of the document as an image next to the editor. The program uses the kitty, sixel, or iTerm2 image protocol when the terminal has one, and half block characters if not.
- The preview renders only the page on screen, so a long document stays fast. In a release build, a book of 500 pages shows its page after 0.7 s.
- `Alt-Down`, `Alt-Up`, `Alt-Home`, and `Alt-End` change the page. The title of the preview shows the page and the page count. If the document gets shorter than the page on screen, the preview shows the last page.
- The program honors `NO_COLOR`. Text and borders have no color, and an error line is bold. The preview image keeps its colors.
- `F11` shows the preview on the full screen. There, `+`, `-`, and `0` zoom, and the arrow keys move the view of a zoomed page. Typing does nothing there.
- A compile pane under the editor. It shows the errors with their line numbers, the time of the compile, and the number of errors and warnings. Errors are red, warnings are yellow, and hints are dim. If the report does not fit, the last row says how many rows are hidden.
- `Ctrl-O` opens the last exported PDF with `xdg-open`.
- `Ctrl-Q` saves the file and quits the program from the editor.
- `Ctrl-C` and `Ctrl-X` also put the selected text on the system clipboard, with the OSC 52 escape sequence.
- `F5` turns the live compile off and on. The autosave stays on.
- `Ctrl-F` searches with a regular expression. `Enter` or `Ctrl-F` goes to the next match, and the search wraps. `Esc` closes the prompt.
- `Ctrl-G` moves the cursor to the first error of the last compile. If the error is in another file of the project, it opens that file at the error. An error in a package file is only named.
- `Ctrl-B` compiles at once, and `Ctrl-E` exports a PDF next to the file that was compiled.
- The title of the terminal window names the folder or the open file.
- `--doctor` checks the Typst program, the image protocol of the terminal, and the temporary folder, and prints one line for each.
- `?` in the list and `F1` in the list and the editor open a help window with all keys. `Enter` on a line presses the key of that line.
- The help text and the README list the editing keys of the text area.
- A man page, `docs/lazytypst.1`. A test keeps its keys the same as the help text.
- `--help` prints the keys. `--version` prints the version of lazytypst and the version of Typst.
- The variable `LAZYTYPST_TYPST` names the Typst program instead of `typst`.
- A clear message and exit code 1 when the `typst` command is missing. The program checks this before it draws anything.
- Safety with projects from other people: the list shows regular files only, an export never writes through a link, a compile stops after 60 seconds, and the memory of Typst is limited if `prlimit` is installed.
- A warning when the project has a link that points outside it. No compile starts at open then.
- A file with CRLF line ends keeps them after a save.
- A temporary folder that only your user can open. The program deletes it when it ends, and the next start deletes folders that an earlier run left behind.
