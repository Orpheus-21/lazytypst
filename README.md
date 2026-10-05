# lazytypst

lazytypst is a terminal program for Typst documents.

## What it does

lazytypst lists the `.typ` files in a folder. The user opens one file, edits the text, and saves it. The user can also compile the file with the `typst` command. The program shows the compile errors with their line numbers. After a good compile, the program shows the pages of the document as images next to the editor, one page at a time. A failed compile keeps the last good page on screen. A new compile keeps the page number.

The program saves the file 300 ms after the last key. Then it compiles the file. A new compile kills the compile that still runs. The preview follows the text.

The folder that you give to the program is the Typst project root. A file in a subfolder can import a file from a parent folder, such as `../lib.typ`, if that file is inside the root. Error lines and the editor title show paths relative to the root.

## Requirements

- Rust 1.90 or later, with Cargo.
- The `typst` command in `PATH`.
- Linux. The program is tested on Linux only.
- A terminal. At start, the program asks the terminal which image protocol it supports. The program uses the kitty, sixel, or iTerm2 protocol when the terminal reports one. Otherwise it draws the page with half block characters. The sixel and iTerm2 protocols are not tested.

## Install

1. Clone the repository.

   ```
   git clone https://github.com/Orpheus-21/lazytypst.git
   ```

2. Go to the folder.

   ```
   cd lazytypst
   ```

3. Build the program.

   ```
   cargo build --release
   ```

The program is the file `target/release/lazytypst`.

## Usage

Run the program with a folder as the argument. If you give no argument, the program uses the current folder. `lazytypst --help` prints the usage and the keys. `lazytypst --version` prints the version. A folder name that starts with a dash needs `./` in front.

```
target/release/lazytypst ~/Documents
```

The program searches the folder and its subfolders, three levels deep. It skips hidden folders.

At start, the program makes the folder `lazytypst-<process id>` in the temporary directory, with access for your user only. If that path exists already, the program stops with an error. The program deletes the folder when it quits or panics. A closed terminal window or a kill can leave the folder behind. The next start deletes each such folder that belongs to you and has no running process.

Keys in the file list:

- `j` or `Down`: select the next file.
- `k` or `Up`: select the previous file.
- `Enter`: open the selected file in the editor.
- `q`: quit.

Keys in the editor:

- `Ctrl-S`: save the file now. `Ctrl-S` also overwrites a file that another program changed. See the save rules below.
- `Ctrl-B`: save the file and compile it now. A compile that still runs is killed and replaced.
- `Ctrl-E`: save the file and export a PDF. The PDF has the name of the file with the ending `.pdf`, in the same folder. Example: `doc.typ` becomes `doc.pdf`. The program replaces a PDF with this name without a question. The pane shows the path until the next export, or it shows the errors.
- `Alt-Down`: show the next page. `Alt-Up`: show the previous page. Both stop at the first page and at the last page. The title of the preview shows the page number and the page count.
- `Esc`: save the text and go back to the file list. If the save is not possible, the program shows the reason. A second `Esc` then closes the editor without a save. Closing the editor kills a compile that still runs.

All other keys edit the text. A long line wraps on screen, at a word if possible. The file keeps it as one line. The text area uses the Emacs-style keys of the `ratatui-textarea` crate. `Ctrl-B` and `Ctrl-E` do the jobs above and not the Emacs jobs. Use `Left` and `End` instead.

Save rules:

- A save writes the file only when the text has edits. A compile or an export of an unchanged file does not write it.
- Before the autosave, `Ctrl-B`, `Ctrl-E`, or `Esc` writes, the program compares the modification time of the file. If another program changed the file, the program does not write it and shows a warning. `Ctrl-S` then overwrites the file with your text. `Esc` twice closes the editor and keeps the version on disk.
- A save writes the text to a hidden temp file next to the file. Then it renames the temp file over the file. A crash during a save leaves the old text or the new text, never a cut file. A symlink stays a symlink, and the permissions stay. A hard link to the file keeps the old text.
- A save writes the text with LF line ends and one final newline. A file with CRLF line ends changes to LF when you edit it.

## How it works

- `src/browser.rs` finds the `.typ` files.
- `src/editor.rs` holds the text area, the save, the 300 ms autosave, the screen layout, and the pane that shows the compile result.
- `src/preview.rs` holds the folder of PNG pages from the last good compile. It draws one page with the `ratatui-image` crate. It deletes the old folder when a new folder loads.
- `src/compile.rs` runs `typst compile --format png --diagnostic-format short --root <folder>` as a job, inside the root folder. The main thread owns the `typst` process and checks it with `try_wait`. A thread reads the error output. Dropping the job kills the `typst` process. Each compile writes its PNG pages to its own new folder in the temporary directory. The editor deletes the folder of a compile that failed or that it killed. The program deletes the temporary directory when it exits. The PDF export is a second job of the same kind.
- `src/main.rs` reads the arguments and runs the event loop. `App::handle_key` handles the keys of the file list. Every 50 ms without a key, the loop calls the editor. The editor then runs the autosave if it is due and checks if a compile has finished.

## License

lazytypst is free software. The license is the GNU General Public License, version 3 or any later version. The text is in the `LICENSE` file.
