# lazytypst

lazytypst is a terminal program for Typst documents.

## What it does

lazytypst lists the `.typ` files in a folder. The user opens one file, edits the text, and saves it. The user can also compile the file with the `typst` command. The program shows the compile errors with their line numbers.

The program does not show a preview of the compiled page yet.

## Requirements

- Rust 1.89 or later, with Cargo.
- The `typst` command in `PATH`.
- Linux. The program is tested on Linux only.

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

- `Ctrl-S`: save the file.
- `Ctrl-B`: save the file and compile it.
- `Esc`: go back to the file list. If the text is not saved, the program shows a warning. Press `Esc` a second time to discard the text.

All other keys edit the text. The text area uses the Emacs-style keys of the `ratatui-textarea` crate.

A save writes the text with LF line ends and one final newline. A file with CRLF line ends changes to LF.

## How it works

- `src/browser.rs` finds the `.typ` files.
- `src/editor.rs` holds the text area, the save, and the pane that shows the compile result.
- `src/compile.rs` runs `typst compile --format png --diagnostic-format short` in a thread. The pages go to PNG files in a folder in the temporary directory. The program deletes the folder when it exits.
- `src/main.rs` runs the event loop. Every 50 ms without a key, the loop checks if a compile has finished.

## License

lazytypst is free software. The license is the GNU General Public License, version 3 or any later version. The text is in the `LICENSE` file.
