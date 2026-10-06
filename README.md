# lazytypst

lazytypst is a terminal program for Typst documents.

## What it does

lazytypst lists the `.typ` files in a folder. The user opens one file, edits the text, and saves it. The user can also compile the file with the `typst` command. The program shows the compile errors with their line numbers. After a good compile, the program shows the pages of the document as images next to the editor, one page at a time. A failed compile keeps the last good page on screen. A new compile keeps the page number.

Each compile renders only the page on screen, so a long document stays fast. In a release build, the page of a book of 500 pages appears after 0.7 s. When all 500 pages were rendered, it took 2.2 s. The cost is that a page turn needs one compile, about as long as a normal compile. The old page stays on screen until the new page is ready.

The program saves the file 300 ms after the last key. Then it compiles the file. A new compile kills the compile that still runs. The preview follows the text.

A project often has one main file, such as `main.typ`, that includes the other files. In the file list, press `m` to mark the selected file as the main file. The list shows `[main]` after its path. Then the live compile, `Ctrl-B`, and `Ctrl-E` use the main file, whatever file you edit. The autosave still saves the file that you edit. The editor title shows the main file. Press `m` on the main file again to remove the mark. Without a main file, the program compiles the open file.

The program remembers the main file of each project. It saves the choice in the file `$XDG_STATE_HOME/lazytypst/main-files`, or `~/.local/state/lazytypst/main-files` if the variable is not set. The program ignores a relative value of `XDG_STATE_HOME`. The project folder gets no new file. At the next start, the program marks the same file again, if the file still exists in the list. If the program cannot save the choice, the status line says so, and the mark still works in this run.

The folder that you give to the program is the Typst project root. A file in a subfolder can import a file from a parent folder, such as `../lib.typ`, if that file is inside the root. Error lines and the editor title show paths relative to the root.

## Requirements

- Rust 1.90 or later, with Cargo.
- The `typst` command in `PATH`, version 0.12.0 or later. Version 0.12.0 added the options that render one page. The program is tested with version 0.15.1. At start, the program runs `typst --version`. If that fails, the program prints which command it needs, why it cannot run it, and the Typst install page, and it exits with code 1. It makes no temporary folder and draws nothing before that.
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

Run the program with a folder as the argument. If you give no argument, the program uses the current folder. If you give a file with the ending `.typ`, the program opens that file in the editor at once. The folder of the file is the project root and the folder of the file list, and `Esc` shows that list with the file selected. A file that does not end with `.typ` gives an error and exit code 2. A path that does not exist gives an error and exit code 1. `lazytypst --help` prints the usage and the keys. `lazytypst --version` prints two lines: the version of lazytypst, and the version line of the `typst` command. If `typst` does not run, the second line says why, for example `typst: not found in PATH`. Put both lines in a bug report. A folder name that starts with a dash needs `./` in front.

```
target/release/lazytypst ~/Documents
```

The program searches the folder and its subfolders, three levels deep. It skips hidden folders.

At start, the program makes the folder `lazytypst-<process id>` in the temporary directory, with access for your user only. If that path exists already, the program stops with an error. There is one exception: a real folder of yours with this name was left by an earlier run to which the system had given the same process id. The program deletes that folder and makes it new. A link, a file, or a folder of another user still stops the program with an error. The program deletes the folder when it quits or panics. A closed terminal window or a kill can leave the folder behind. The next start deletes each such folder that belongs to you and has no running process.

Keys in the file list:

- `j` or `Down`: select the next file.
- `k` or `Up`: select the previous file.
- `Enter`: open the selected file in the editor.
- `n`: make a new file. A prompt asks for a path, for example `chapters/two`. `Enter` makes the empty file, opens it in the editor, and selects it in the list. `Esc` cancels. The program adds `.typ` if the name has no ending, and it makes missing folders. The program refuses a name that leaves the project, that has an ending other than `.typ`, that starts with a dot, that is deeper than the list reads, that goes through a link, or that exists already. The prompt then stays open and the status line gives the reason.
- `r`: read the folder again. New files appear and deleted files go. The selection stays on the same file if it is still there. A main file that is gone loses its mark.
- `g` or `Home`: select the first file. `G` or `End`: select the last file.
- `m`: mark the selected file as the main file, or remove the mark.
- `q`: quit.

Keys in the editor:

- `Ctrl-S`: save the file now. `Ctrl-S` also overwrites a file that another program changed. See the save rules below.
- `Ctrl-B`: save the file and compile it now. A compile that still runs is killed and replaced.
- `Ctrl-E`: save the file and export a PDF. The PDF has the name of the compiled file with the ending `.pdf`, in the same folder. Example: `doc.typ` becomes `doc.pdf`. With a main file, the PDF comes from the main file, for example `main.pdf`. The program replaces a PDF with this name without a question. The pane shows the path until the next export, or it shows the errors.
- `Ctrl-G`: go to the first error of the last compile. The cursor moves to the line and the column that Typst reports. If that error is in another file, the status line names the file, and the cursor stays. If the last compile has no error, the status line says so. The line and the column come from the text at the time of the compile. After more edits, compile again to get exact places.
- `Alt-Down`: show the next page. `Alt-Up`: show the previous page. Both stop at the first page and at the last page. A page turn starts a compile for the new page. It does not save the file. A fast second press replaces the compile of the first, so the last press decides the page. The title of the preview shows the page on screen and the page count. If the document gets shorter than the page on screen, the program shows the last page.
- `Esc`: save the text and go back to the file list. If the save is not possible, the program shows the reason. A second `Esc` then closes the editor without a save. Closing the editor kills a compile that still runs.

All other keys edit the text. A long line wraps on screen, at a word if possible. The file keeps it as one line. The editor shows a dim line number at the left edge of each line. A line that wraps shows its number on the first row only. The right end of the status line shows the cursor position as `line:column`. Both numbers start at 1, and the column counts characters, the same as the error lines of Typst. `Tab` goes to the next stop of 2 columns, so at the start of a line it inserts 2 spaces. It inserts spaces and never a tab character. Tab characters that are already in a file stay. The text area uses the Emacs-style keys of the `ratatui-textarea` crate. `Ctrl-B` and `Ctrl-E` do the jobs above and not the Emacs jobs. Use `Left` and `End` instead.

Save rules:

- A save writes the file only when the text has edits. A compile or an export of an unchanged file does not write it.
- Before the autosave, `Ctrl-B`, `Ctrl-E`, or `Esc` writes, the program compares the modification time of the file. If another program changed the file, the program does not write it and shows a warning. `Ctrl-S` then overwrites the file with your text. `Esc` twice closes the editor and keeps the version on disk.
- A save writes the text to a hidden temp file next to the file. Then it renames the temp file over the file. A crash during a save leaves the old text or the new text, never a cut file. A symlink stays a symlink, and the permissions stay. A hard link to the file keeps the old text.
- A save writes the text with LF line ends and one final newline. A file with CRLF line ends changes to LF when you edit it.

## How it works

- `src/browser.rs` finds the `.typ` files. `src/newfile.rs` checks the name of a new file and makes it.
- `src/editor.rs` holds the text area, the save, the 300 ms autosave, the screen layout, and the pane that shows the compile result.
- `src/preview.rs` holds the folder of the last good compile. The folder has one PNG file, named `page-<page>-of-<count>.png`, so the preview learns the page count from the name. It draws the page with the `ratatui-image` crate. It deletes the old folder when a new folder loads. It keeps the number of the wanted page.
- `src/compile.rs` parses each line of the Typst output into a diagnostic with a severity, a file, a line, and a column. It also runs `typst compile --format png --diagnostic-format short --root <folder> --pages <page>` as a job, inside the root folder. Typst exits with success and writes no file when the document has fewer pages than the page that the program asks for. The editor then compiles page 1, learns the page count, and compiles the last page. The main thread owns the `typst` process and checks it with `try_wait`. A thread reads the error output. Dropping the job kills the `typst` process. Each compile writes its one PNG file to its own new folder in the temporary directory. The editor deletes the folder of a compile that failed or that it killed. The program deletes the temporary directory when it exits. The PDF export is a second job of the same kind.
- `src/state.rs` reads and writes the state file with the main file of each project. `src/fsutil.rs` holds the safe write that the editor and the state file both use.
- `src/main.rs` reads the arguments and runs the event loop. `App::handle_key` handles the keys of the file list. Every 50 ms without a key, the loop calls the editor. The editor then runs the autosave if it is due and checks if a compile has finished.

## Measuring

`scripts/bench-pages.sh` compiles documents of 1, 50, 200, and 500 pages with the options of the program, once for all pages and once for one page. It prints the time and the disk use. Give other page counts as arguments. The script needs `typst` in `PATH`.

## License

lazytypst is free software. The license is the GNU General Public License, version 3 or any later version. The text is in the `LICENSE` file.
