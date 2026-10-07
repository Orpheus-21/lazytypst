# Changelog

This file lists the changes that a user of lazytypst can see. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). The version numbers follow [Semantic Versioning](https://semver.org/). Before version 1.0.0, a change in the middle number can change how the program works.

Each change that a user can see adds one line under `Unreleased`, in the section Added, Changed, Fixed, or Removed. On release day, those lines move to a new entry that has the version and the date.

## [Unreleased]

## [0.1.0] (not yet released)

The first version. It needs Typst 0.12.0 or later in `PATH`, and it is tested with Typst 0.15.1. It needs Rust 1.90 or later to build. It is tested on Linux, with the kitty image protocol in Ghostty and with half blocks. The sixel and iTerm2 protocols are not tested.

### Added

- A list of the `.typ` files in a folder, up to three folders deep. Hidden folders are skipped.
- Keys in the list: `j` and `k` or the arrow keys, `g` and `G` or `Home` and `End`, `Enter` to open a file, and `q` to quit.
- `/` filters the list while you type. The match ignores case. `Esc` removes the filter.
- `r` reads the folder again, to show new files and to drop deleted files.
- `n` makes a new `.typ` file from a path such as `chapters/two`. The program makes missing folders and refuses a name that leaves the project.
- `m` marks a main file. The live compile, `Ctrl-B`, and `Ctrl-E` then use the main file, whatever file you edit. The program remembers the main file of each project, and it keeps that choice outside the project folder.
- A folder or a `.typ` file as the argument. A file opens in the editor at once, and its folder is the project root. Files in subfolders can import files from parent folders inside the root.
- A text area with line numbers, long lines that wrap, and Emacs style editing keys. `Tab` inserts 2 spaces. The status line shows the cursor as `line:column`. If you close a file and open it again, the cursor comes back.
- An autosave 300 ms after the last key, and a live compile after each save. A new compile stops the compile that still runs.
- A reload of the file when another program changes it and the text has no edits. The cursor keeps its line, and a compile starts.
- A safe save. The program writes the file only when the text has edits. It does not overwrite a change that another program made, until you press `Ctrl-S`. It writes a temp file and renames it, so a crash cannot cut the file. A symlink stays a symlink, and the permissions stay.
- A live preview of the document as an image next to the editor. The program uses the kitty, sixel, or iTerm2 image protocol when the terminal has one, and half block characters if not.
- The preview renders only the page on screen, so a long document stays fast. In a release build, a book of 500 pages shows its page after 0.7 s.
- `Alt-Down`, `Alt-Up`, `Alt-Home`, and `Alt-End` change the page. The title of the preview shows the page and the page count. If the document gets shorter than the page on screen, the preview shows the last page.
- A compile pane under the editor. It shows the errors with their line numbers, the time of the compile, and the number of errors and warnings. Errors are red, warnings are yellow, and hints are dim. If the report does not fit, the last row says how many rows are hidden.
- `Ctrl-G` moves the cursor to the first error of the last compile.
- `Ctrl-B` compiles at once, and `Ctrl-E` exports a PDF next to the file that was compiled.
- The title of the terminal window names the folder or the open file.
- `--help` prints the keys. `--version` prints the version of lazytypst and the version of Typst.
- A clear message and exit code 1 when the `typst` command is missing. The program checks this before it draws anything.
- A temporary folder that only your user can open. The program deletes it when it ends, and the next start deletes folders that an earlier run left behind.
