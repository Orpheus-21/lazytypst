# lazytypst

lazytypst is a terminal program for Typst documents.

## What it does

lazytypst lists the `.typ` files in a folder. The user opens one file, edits the text, and saves it. The user can also compile the file with the `typst` command. The program shows the compile errors with their line numbers. After a good compile, the program shows the pages of the document as images next to the editor, one page at a time. A failed compile keeps the last good page on screen. A new compile keeps the page number.

The program saves the file 300 ms after the last key. Then it compiles the file. A new compile kills the compile that still runs. The preview follows the text.

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

Run the program with a folder as the argument. If you give no argument, the program uses the current folder.

```
target/release/lazytypst ~/Documents
```

The program searches the folder and its subfolders, three levels deep. It skips hidden folders.

Keys in the file list:

- `j` or `Down`: select the next file.
- `k` or `Up`: select the previous file.
- `Enter`: open the selected file in the editor.
- `q`: quit.

Keys in the editor:

- `Ctrl-S`: save the file now.
- `Ctrl-B`: save the file and compile it now. A compile that still runs is killed and replaced.
- `Ctrl-E`: save the file and export a PDF. The PDF has the name of the file with the ending `.pdf`, in the same folder. Example: `doc.typ` becomes `doc.pdf`. The program replaces a PDF with this name without a question. The pane shows the path, or the errors.
- `Alt-n`: show the next page. `Alt-p`: show the previous page. Both stop at the first page and at the last page. The title of the preview shows the page number and the page count.
- `Esc`: go back to the file list. If the last key was less than 300 ms ago, the autosave has not run yet. The program then shows a warning. A second `Esc` before the autosave runs closes the editor and discards the text. Closing the editor kills a compile that still runs.

All other keys edit the text. The text area uses the Emacs-style keys of the `ratatui-textarea` crate. `Ctrl-B` and `Ctrl-E` do the jobs above and not the Emacs jobs. Use `Left` and `End` instead.

A save writes the text with LF line ends and one final newline. A file with CRLF line ends changes to LF.

## How it works

- `src/browser.rs` finds the `.typ` files.
- `src/editor.rs` holds the text area, the save, the 300 ms autosave, the screen layout, and the pane that shows the compile result.
- `src/preview.rs` holds the folder of PNG pages from the last good compile. It draws one page with the `ratatui-image` crate. It deletes the old folder when a new folder loads.
- `src/compile.rs` runs `typst compile --format png --diagnostic-format short` as a job. A thread collects the output. Dropping the job kills the `typst` process. Each compile writes its PNG pages to its own new folder in the temporary directory. The editor deletes the folder of a compile that failed or that it killed. The program deletes the temporary directory when it exits. The PDF export is a second job of the same kind.
- `src/main.rs` runs the event loop. Every 50 ms without a key, the loop calls the editor. The editor then runs the autosave if it is due and checks if a compile has finished.

## License

lazytypst is free software. The license is the GNU General Public License, version 3 or any later version. The text is in the `LICENSE` file.
