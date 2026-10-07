# lazytypst

lazytypst is a terminal program for Typst documents.

The project website is at https://orpheus-21.github.io/lazytypst/.

![lazytypst in a terminal: the text of a Typst file on the left with syntax color, the compile pane under it, and the page preview on the right](docs/screenshot.png)

## Quick start

1. Install lazytypst: `cargo install --locked --git https://github.com/Orpheus-21/lazytypst` (see [Install](#install)).
2. Start it in a folder with Typst files: `lazytypst ~/my-typst-folder`
3. Select a file with `j` and `k`, press `Enter`, and type. The preview follows the text. Press `F1` to see all keys.

## What it does

lazytypst lists the `.typ` files in a folder. The user opens one file, edits the text, and saves it. The user can also compile the file with the `typst` command. The program shows the compile errors with their line numbers. After a good compile, the program shows the pages of the document as images next to the editor, one page at a time. A failed compile keeps the last good page on screen. A new compile keeps the page number.

Each compile renders only the page on screen, so a long document stays fast. In a release build, the page of a book of 500 pages appears after 0.7 s. When all 500 pages were rendered, it took 2.2 s. The cost is that a page turn needs one compile, about as long as a normal compile. The old page stays on screen until the new page is ready.

When you open a file, the program compiles it at once, so the preview shows without a key. The program saves the file 300 ms after the last key. Then it compiles the file. A new compile kills the compile that still runs. The preview follows the text.

A project often has one main file, such as `main.typ`, that includes the other files. In the file list, press `m` to mark the selected file as the main file. The list shows `[main]` after its path. Then the live compile, `Ctrl-B`, and `Ctrl-E` use the main file, whatever file you edit. The autosave still saves the file that you edit. The editor title shows the main file. Press `m` on the main file again to remove the mark. Without a main file, the program compiles the open file.

The program remembers the main file of each project. It saves the choice in the file `$XDG_STATE_HOME/lazytypst/main-files`, or `~/.local/state/lazytypst/main-files` if the variable is not set. The program ignores a relative value of `XDG_STATE_HOME`. The project folder gets no new file. At the next start, the program marks the same file again, if the file still exists in the list. If the program cannot save the choice, the status line says so, and the mark still works in this run.

The program also remembers the last file that you closed in each project, and the page of its preview. It saves them in the file `last-files`, in the same folder as `main-files`. At the next start in the same folder, the list selects that file. The program does not open it. When you open it, the preview shows the saved page, or the last page if the document got shorter. If the file is gone, the list selects the first file.

The compile pane under the editor shows the result of the last compile. After a good compile, its first line says how long Typst needed, for example `OK in 310 ms`. The title shows how many errors and warnings the report has, for example `Compile: 2 errors, 1 warning`. After a failed compile, the title also shows the time, for example `Compile: 1 error (310 ms)`. A compile with warnings only is green. An error line is red, a warning line is yellow, and other lines, such as hints, are dim. The colors are the colors of the terminal palette, so they follow your theme. The pane has room for 4 rows, and long lines wrap. If the report needs more rows, the last row says how many are hidden, for example `+3 more`. The count is in screen rows after the wrap.

The folder that you give to the program is the Typst project root. A file in a subfolder can import a file from a parent folder, such as `../lib.typ`, if that file is inside the root. Error lines and the editor title show paths relative to the root.

## Scope

lazytypst does these jobs: it browses the Typst files of a project, edits one file, compiles it, shows the errors, shows the pages, and exports a PDF.

The project follows these guides: few keys, good defaults, no configuration file, and the keys on screen.

lazytypst does not act as a general file manager, as a language server client, as a plugin host, or as an editor with many open files. It does not replace the options of the Typst CLI with settings.

An idea that adds keys or settings without making this work faster is closed with a link to this section. Read [CONTRIBUTING.md](CONTRIBUTING.md) before you open an issue.

## Requirements

- Rust 1.90 or later, with Cargo.
- The `typst` command in `PATH`, version 0.12.0 or later. Version 0.12.0 added the options that render one page. The program is tested with version 0.15.1. At start, the program runs `typst --version`. If that fails, the program prints which command it needs, why it cannot run it, and the Typst install page, and it exits with code 1. It makes no temporary folder and draws nothing before that.
- The variable `LAZYTYPST_TYPST` can name another Typst program, for example a second version or a wrapper script: `LAZYTYPST_TYPST=/opt/typst-0.14/typst lazytypst`. If the variable is set and not empty, the compile, the export, and the version check of `--version` run that program instead of `typst`. If it cannot run, the compile pane shows `Cannot run` and the path. Without the variable, the program runs `typst` from `PATH`.
- Linux. The program is tested on Linux only.
- A terminal. At start, the program asks the terminal which image protocol it supports. The program uses the kitty, sixel, or iTerm2 protocol when the terminal reports one. Otherwise it draws the page with half block characters. The sixel and iTerm2 protocols are not tested.

## Install

There are four ways to install lazytypst: use Cargo, use the PKGBUILD on Arch Linux, download a release, or build the program from a clone. Each way needs the `typst` command in `PATH` (see Requirements).

### Install with Cargo

This way needs Rust 1.90 or later. It needs no clone of the repository.

```
cargo install --locked --git https://github.com/Orpheus-21/lazytypst
```

Cargo builds the program and puts it in `~/.cargo/bin`. That folder is in `PATH` for most Rust users. Then `lazytypst --version` works in a new terminal. To update, run the same command again. To remove the program, run `cargo uninstall lazytypst`.

### Install on Arch Linux

The folder `packaging/aur/` has a `PKGBUILD` for the package `lazytypst-git`. It builds the latest commit of `main` and needs `typst`, `cargo`, and `git`. To build and install it, run `makepkg -si` in that folder. The package is not on the AUR yet. After it is there, `yay -S lazytypst-git` will install it.

### Download a release

This way needs no Rust. The first release is not published yet.

1. Download these two files from the page of the release, https://github.com/Orpheus-21/lazytypst/releases:

   - `lazytypst-<version>-x86_64-linux.tar.gz`
   - `lazytypst-<version>-x86_64-linux.tar.gz.sha256`

2. Check the archive against its checksum file.

   ```
   sha256sum --check lazytypst-<version>-x86_64-linux.tar.gz.sha256
   ```

3. Unpack the archive.

   ```
   tar -xzf lazytypst-<version>-x86_64-linux.tar.gz
   ```

4. Copy the program into a folder that is in `PATH`.

   ```
   install -m 755 lazytypst-<version>-x86_64-linux/lazytypst ~/.local/bin/
   ```

The archive also holds the man page `lazytypst.1`. To install it, run `install -D -m 644 lazytypst-<version>-x86_64-linux/lazytypst.1 ~/.local/share/man/man1/lazytypst.1`. Then `man lazytypst` shows it.

The program in the archive is a static binary. It runs on any x86_64 Linux system, and it does not depend on the C library of the system. It still needs the `typst` command in `PATH`.

### Build from a clone

This way is for people who want to change the program.

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

To read the man page from a clone, run `man ./docs/lazytypst.1`.

## Usage

Run the program with a folder as the argument. If you give no argument, the program uses the current folder. If you give a file with the ending `.typ`, the program opens that file in the editor at once. The folder of the file is the project root and the folder of the file list, and `Esc` shows that list with the file selected. If the folder has exactly one `.typ` file and you name no file, the program opens that file in the editor at once, and `Esc` shows the list with the file. With no file, or with two or more files, the program starts with the list. A file that does not end with `.typ` gives an error and exit code 2. A path that does not exist gives an error and exit code 1. `lazytypst --help` prints the usage and the keys. `lazytypst --doctor` prints one line for each check and ends with exit code 0 if all checks pass, or 1 if not. It checks the Typst program (and prints its version, or `missing` with the install link), the image protocol that the terminal reports (`kitty graphics`, `sixel graphics`, `iTerm2 graphics`, or `half blocks` with the reason), that the temporary folder is writable, and how many stale `lazytypst-<process id>` folders of earlier runs exist. Half blocks are not a failure. A stale folder is not a failure: the next start deletes it. Run it in the terminal where the preview looks wrong, and put the output in a bug report. `lazytypst --version` prints two lines: the version of lazytypst, and the version line of the `typst` command. If `typst` does not run, the second line says why, for example `typst: not found in PATH`. Put both lines in a bug report. A folder name that starts with a dash needs `./` in front.

```
target/release/lazytypst ~/Documents
```

The program searches the folder and its subfolders, three levels deep. It skips hidden folders.

The program sets the title of the terminal window to `lazytypst: <folder name>` in the file list, and to `lazytypst: <path of the file>` in the editor. The path is relative to the project root. A control character in a name becomes a question mark, because the title travels inside an escape sequence. When the program starts, it sends the command that saves the window title, and when it ends, it sends the command that restores it. Terminals that follow xterm restore your old title. Ghostty 1.3.1 ignores both commands. In Ghostty, the title that the program set stays until your shell sets it again, which the shell integration of Ghostty does at the next prompt.

At start, the program makes the folder `lazytypst-<process id>` in the temporary directory, with access for your user only. If that path exists already, the program stops with an error. There is one exception: a real folder of yours with this name was left by an earlier run to which the system had given the same process id. The program deletes that folder and makes it new. A link, a file, or a folder of another user still stops the program with an error. The program deletes the folder when it quits or panics. A closed terminal window or a kill can leave the folder behind. The next start deletes each such folder that belongs to you and has no running process.

Keys in the file list. Each line shows the time since the last change of the file at the right end, in a dim style: `now`, `5 min`, `2 h`, `3 d`, `4 mo`, or `2 y`. The program hides all ages if one would cut the longest path. The list reads the times again when you press `r`, when you press `s`, and when you close the editor.

- `?` or `F1`: open the help window. The window lists all keys of the list, the editor, and the text area, the same list as `lazytypst --help`. `j` and `k` or the arrow keys select a line. `Enter` closes the window and presses the key of the line, so the window also works as a menu of actions. `Esc`, `q`, `?`, or `F1` closes it. While it is open, no other key acts. In the editor, `?` types a question mark, and `F1` opens the window.
- `j` or `Down`: select the next file.
- `k` or `Up`: select the previous file.
- `Enter`: open the selected file in the editor.
- `n`: make a new file. A prompt asks for a path, for example `chapters/two`. `Enter` makes the empty file, opens it in the editor, and selects it in the list. `Esc` cancels. The program adds `.typ` if the name has no ending, and it makes missing folders. The program refuses a name that leaves the project, that has an ending other than `.typ`, that starts with a dot, that is deeper than the list reads, that goes through a link, or that exists already. The prompt then stays open and the status line gives the reason.
- `r`: read the folder again. New files appear and deleted files go. The selection stays on the same file if it is still there. A main file that is gone loses its mark.
- `g` or `Home`: select the first file. `G` or `End`: select the last file.
- `/`: filter the list. A prompt `Filter:` asks for a part of a path. The list shows only the files whose path contains that text, and the match ignores case. The list follows the text while you type. `Enter` keeps the filter, and the list title shows it, for example `lazytypst /chap`. `Esc` removes the filter, in the prompt and also in the list. If no file matches, the list says `No match`. `r` keeps the filter. A new file removes it, so that the file shows. The filter does not change the main file.
- `s`: switch the order of the list between the path and the last change, newest first. The title of the list shows `(newest first)` in the second order. The selection stays on the same file. Files with the same time stay in the order by path.
- `e`: edit the selected file in your own editor. The program runs the command in the variable `VISUAL`, or else `EDITOR`, in the same terminal, with the file as the last argument. A value such as `code --wait` splits on spaces into the program and its arguments. The program leaves its screen, waits, and comes back. Then it opens the file in its own editor and starts a compile, so the preview shows the new text. If neither variable is set, the status line says so. If the editor ends with an error, the status line shows how it ended.
- `E`: export the PDF of the selected file, with the same job as `Ctrl-E`. The PDF goes next to the file, for example `report.typ` becomes `report.pdf`. This key uses the selected file and not the main file. The list stays usable while the export runs. The status line shows `Exported report.pdf`, or the first error line of Typst. A new export stops an export that still runs.
- `y`: copy the absolute path of the selected file to the system clipboard. The status line shows `Copied` and the path.
- `m`: mark the selected file as the main file, or remove the mark.
- `q`: quit.

Keys in the editor:

- `Ctrl-S`: save the file now. `Ctrl-S` also overwrites a file that another program changed. See the save rules below.
- `Ctrl-B`: save the file and compile it now. A compile that still runs is killed and replaced.
- `Ctrl-E`: save the file and export a PDF. The PDF has the name of the compiled file with the ending `.pdf`, in the same folder. Example: `doc.typ` becomes `doc.pdf`. With a main file, the PDF comes from the main file, for example `main.pdf`. The program replaces a PDF with this name without a question. The pane shows the path until the next export, or it shows the errors.
- `Ctrl-O`: open the last exported PDF in the system viewer. The program starts `xdg-open` in the background, so the screen stays clean. Before an export, the status line says that there is no PDF yet. If `xdg-open` cannot start, the status line shows the error.
- `Ctrl-Q`: save the file and quit the program. It does the same as `Esc` and then `q`. If the program cannot save, it shows the reason, and a second `Ctrl-Q` quits without a save.
- `F5`: turn the live compile off or on. The default is on. While it is off, typing still saves the file after a pause, but no compile starts, and the title of the compile pane says `Compile (paused)`. `Ctrl-B` still compiles. A page turn still compiles. The setting is for this editor only and it is on again when you open the next file. Use it for a very large document.
- `Ctrl-F`: search. A prompt `Search:` opens in the status line. Type a regular expression, for example `Item` or `^= `. The text area marks all matches while you type. `Enter` or `Ctrl-F` moves the cursor to the next match, and the search wraps to the top of the file after the last match. `Esc` closes the prompt and keeps the cursor where it is. If nothing matches, the status line says `No match for` and the pattern, and the cursor stays. If the pattern is not a valid regular expression, the status line shows the reason. The search is case sensitive. `Ctrl-F` does not move the cursor to the right: use `Right`.
- `F11`: show the preview on the full screen, and back. The text area and the compile pane are hidden, and the status line says `F11 back to editor` and shows the zoom. Typing does nothing there, so no text changes by accident. `F11` or `Esc` brings the editor back, and `Esc` does not go to the file list from here. `Alt-Down`, `Alt-Up`, `Alt-Home`, and `Alt-End` still change the page. The live compile and the autosave keep running.
- In the full preview, `+` zooms in one step and `-` zooms out one step. The steps are 100%, 150%, 200%, and 300%. `0` shows the whole page again. At a zoom above 100%, the arrow keys move the view by a quarter of the way that it can go, and `Home` and `End` go to the top and the bottom of the page. A zoom starts a compile at a higher resolution (`typst compile --ppi`, 144 pixels per inch at 100%), so the part on screen stays sharp. The zoom and the place stay when the page changes or a new compile arrives. A page at 300% has about 4000 by 5000 pixels, so it needs some memory and time.
- `Ctrl-G`: go to the first error of the last compile. The cursor moves to the line and the column that Typst reports. If that error is in another file of the project, for example a chapter that the main file includes, the program saves the open file, opens the other file, and moves the cursor to the error there. The compile target stays the same, so the preview shows the same document. If the error is in a file outside the project, such as a package, the status line names the file, and nothing opens. If the last compile has no error, the status line says so. The line and the column come from the text at the time of the compile. After more edits, compile again to get exact places.
- `Alt-Home`: show the first page. `Alt-End`: show the last page. Both start one compile, like a page turn. `Home` and `End` without `Alt` move the cursor.
- `Alt-Down`: show the next page. `Alt-Up`: show the previous page. Both stop at the first page and at the last page. A page turn starts a compile for the new page. It does not save the file. A fast second press replaces the compile of the first, so the last press decides the page. The title of the preview shows the page on screen and the page count. If the document gets shorter than the page on screen, the program shows the last page.
- `Ctrl-C`: copy the selected text. `Ctrl-X`: cut it. Select text with `Shift` and the arrow keys. Both keys also put the text on the system clipboard with the OSC 52 escape sequence, so you can paste it in another program. `Ctrl-Y` pastes the text that the text area holds. A terminal without OSC 52 support leaves the system clipboard unchanged.
- `Esc`: save the text and go back to the file list. The program remembers where the cursor was in each file until you quit. If you open the same file again, the cursor goes back to that place. If the file got shorter, the cursor goes to the end of the last line. A new start begins at line 1. If the save is not possible, the program shows the reason. A second `Esc` then closes the editor without a save. Closing the editor kills a compile that still runs.

The editor colors the Typst source: headings (lines that start with `=` and a space), commands after `#` (keywords such as `#set`, `#let`, and `#import` are magenta, other names such as `#image` are cyan), strings in code, math between `$` signs, raw text between backticks, and comments (`//` and `/* */`). The colors are the palette colors of your terminal, so they follow its theme. With `NO_COLOR`, the parts use bold, dim, and italic instead. The scanner is small and it is not a Typst parser: it can color a part wrong, for example in an unusual mix of code and markup. The color never changes the text. On a file of 2000 headings and 2000 text lines, the color step takes about 0.7 ms in a release build, in a draw of 4.6 ms. All other keys edit the text. A long line wraps on screen, at a word if possible. The file keeps it as one line. The editor shows a dim line number at the left edge of each line. A line that wraps shows its number on the first row only. The right end of the status line shows an approximate word count, such as `1234 words`, and then the cursor position as `line:column`. The count is approximate. A word is a part of the text between spaces that has at least one letter or digit, so a heading mark `=` is not a word. The count skips comments (`//` and `/* */`), math between `$` signs, and lines that start with `#set`, `#show`, `#import`, `#let`, `#include`, `#pagebreak`, `#colbreak`, `#bibliography`, `#outline`, `#context`, or `#counter`. If such a line opens a bracket that it does not close, the count also skips the lines up to the closing bracket. A function call in the middle of a line still adds its parts. The count updates after each change of the text. Both numbers of the cursor position start at 1, and the column counts characters, the same as the error lines of Typst. `Tab` goes to the next stop of 2 columns, so at the start of a line it inserts 2 spaces. It inserts spaces and never a tab character. Tab characters that are already in a file stay. A paste arrives as one piece: the program asks the terminal for bracketed paste. One undo takes back a whole paste. A tab character in pasted text stays a tab character, and a line end in pasted text becomes LF. The file list ignores a paste. The text area uses the Emacs-style keys of the `ratatui-textarea` crate. `Ctrl-B` and `Ctrl-E` do the jobs above and not the Emacs jobs. Use `Left` and `End` instead. These editing keys work. A test of the project checks each one.

| Key | Action |
|---|---|
| `Ctrl-U` | Undo. |
| `Ctrl-R` | Redo. |
| `Shift` with the arrow keys | Select text. |
| `Ctrl-C` | Copy the selection. |
| `Ctrl-X` | Cut the selection. |
| `Ctrl-Y` | Paste the text that the text area holds. |
| `Ctrl-W` | Delete the word before the cursor. |
| `Ctrl-K` | Delete to the end of the line. |
| `Ctrl-J` | Delete to the start of the line. |
| `Alt-F`, `Alt-B` | Move one word forward or back. |
| `Ctrl-A`, `End` | Move to the start or the end of the line. |
| `Ctrl-V`, `Alt-V` | Scroll one page down or up. |
| `Alt-<`, `Alt->` | Move to the first or the last line. The column stays. |

`Left`, `Right`, `Backspace`, and `Delete` work on one visible character, not on one code point. A Devanagari conjunct, a letter with a combining mark, and an emoji with joiners count as one character. The column in the status line still counts code points, the same as the error lines of Typst.

`Alt` with a letter arrives as `Esc` and the letter. A terminal that sends both at once gives the program the `Alt` key. `F1` shows the same table in the help window.

Save rules:

- A save writes the file only when the text has edits. A compile or an export of an unchanged file does not write it.
- Before the autosave, `Ctrl-B`, `Ctrl-E`, or `Esc` writes, the program compares the modification time of the file. If another program changed the file, the program does not write it and shows a warning. `Ctrl-S` then overwrites the file with your text. `Esc` twice closes the editor and keeps the version on disk. `Ctrl-Q` twice does the same and quits the program.
- About once a second, while the text has no edits, the program compares the modification time of the file. If another program changed the file, the program loads the new text, keeps the cursor on the same line number, and starts a compile. If the file got shorter, the cursor goes to the last line. If another program deleted the file, the status line says that the file is gone, and the text stays in the editor. `Ctrl-S` writes it again.
- A save writes the text to a hidden temp file next to the file. Then it renames the temp file over the file. A crash during a save leaves the old text or the new text, never a cut file. A symlink stays a symlink, and the permissions stay. A hard link to the file keeps the old text.
- A save writes the text with one final line end. The line end is CR LF if the file had CR LF when the program read it, and LF if not. So a diff shows only your edits.

## Safety with projects from other people

A project from the internet can hold files that harm you when a program opens them. lazytypst limits these risks.

- The file list shows only regular files. A `.typ` link counts only if it points at a regular file inside the project. A pipe, a device, a dangling link, and a link to a file outside the project are not listed. The program opens a regular file of at most 16 MiB.
- An export writes a new file next to the source and renames it onto the PDF. So a link such as `report.pdf` that points at another file of yours is replaced, and the other file stays.
- A compile or an export stops after 60 seconds. The program keeps the first 1 MiB of the error text of Typst. If the `prlimit` program of util-linux is in `PATH`, it also limits the memory of Typst to 8 GiB, so a page of 500 cm by 500 cm cannot stop the machine.
- The program compiles a file when you open it. But Typst can read a file through a link inside the project, and the file can then show in the preview and in an exported PDF. So the program looks for links that point outside the project when it reads the folder. If it finds one, it shows a warning when a file opens, and it starts no compile then. `Ctrl-B` and the autosave after an edit still compile. This is a warning, and it does not stop Typst: do not share a PDF of a project that you did not read.
- A document that imports a package from `@preview` makes Typst download the package.

## Colors

The program honors the variable `NO_COLOR` (see https://no-color.org/). If `NO_COLOR` is set and not empty, the program uses no color for text and borders. An error line in the compile pane is bold, and a warning is plain. The title of the pane still says the state, for example `Compile (running)` or `Compile: 2 errors`. The file list uses reverse video for the selected file. The page preview is an image, and it keeps its colors. With `NO_COLOR` unset or empty, an error is red, a warning is yellow, and the border of the pane is green, red, or yellow.

## Fonts and packages

lazytypst passes its environment to the `typst` command, so the variables of Typst work. This section names the ones that matter for fonts and packages. The names come from `typst compile --help` of Typst 0.15.1.

- `TYPST_FONT_PATHS`: extra folders with fonts. Example: `TYPST_FONT_PATHS=~/fonts lazytypst ~/book`. The fonts in `~/fonts` are then available to the document.
- `TYPST_IGNORE_SYSTEM_FONTS`: use only the fonts of the folders above and the fonts that Typst has built in. Example: `TYPST_IGNORE_SYSTEM_FONTS=true lazytypst ~/book`. The value is `true` or `false`.
- `TYPST_PACKAGE_PATH`: a folder with local packages. Example: `TYPST_PACKAGE_PATH=~/packages lazytypst ~/book`.
- `TYPST_PACKAGE_CACHE_PATH`: the folder where Typst keeps the packages that it downloaded.
- `TYPST_ROOT` has no effect, because lazytypst always gives `--root` with the folder of the list. An option on the command line wins over the variable.

An import such as `#import "@preview/..."` downloads the package at the first compile. That compile needs the internet and can take longer. The compile pane shows the download text of Typst. Later compiles use the cache.

## Other ways to work

lazytypst is one of several ways to work with Typst. This is how it differs from three others.

- [`typst watch`](https://github.com/typst/typst) compiles a file again each time it changes, and it has incremental compilation. It shows no preview of its own: you open the PDF in a viewer. lazytypst has no incremental compilation, but it shows the page next to the text in the same terminal, and it shows the errors there.
- The preview of the [tinymist](https://github.com/Myriad-Dreamin/tinymist) language service works in editors such as VS Code, Neovim, Emacs, Sublime Text, Helix, and Zed, and a language service gives more than a preview. lazytypst has no completion or other language features, but it needs no editor, plugin, or configuration.
- The [typst.app](https://typst.app/) web app is an online editor for teams, with instant preview and collaboration. lazytypst works on files on your own machine and offline, except for the first download of a package.

## FAQ

**Why does lazytypst save my file while I type?** The preview follows the saved file, so the program saves 300 ms after your last key and then compiles. See [What it does](#what-it-does).

**Can lazytypst lose my text?** It is built so that it does not. The program writes only when the text has edits. It does not overwrite a change that another program made, until you press `Ctrl-S`. It writes a temp file and renames it, so a crash cannot cut your file. See the save rules in [Usage](#usage).

**Where do the preview pages go, and when are they deleted?** They go into a private folder `lazytypst-<process id>` in the temporary directory. The program deletes it when it ends, and the next start deletes the folders of earlier runs that crashed. See [Usage](#usage).

**Why do the page keys use `Alt` with the arrow keys?** The plain arrow keys move the cursor in the text. `Alt-Down` and `Alt-Up` are free, and the terminal sends them as one key. See [Usage](#usage).

**Why is the preview made of blocks in my terminal?** Your terminal reports no image protocol, so the program draws half blocks. Run `lazytypst --doctor` to see what the terminal reports. See [Troubleshooting](#troubleshooting).

**Does lazytypst work without the internet?** Yes. Only the first compile of a document that imports a Typst package from `@preview` downloads the package. See [Fonts and packages](#fonts-and-packages).

## Troubleshooting

### Terminal support

The page preview is an image. The terminal must have an image protocol. The program asks the terminal at start, and `lazytypst --doctor` prints the answer.

| Terminal | Image method | State |
|---|---|---|
| Ghostty | kitty protocol | tested |
| kitty | kitty protocol | not tested |
| WezTerm | iTerm2 protocol and sixel | not tested |
| foot | sixel | not tested |
| Konsole | not known | not tested |
| Alacritty | none, so half blocks | not tested |
| tmux | depends on the terminal and the settings | not tested |

The program also works in a terminal with no image protocol. It then draws the page with half block characters.

### The preview shows blocks

The terminal has no image protocol, or the program could not read the answer of the terminal. The program then draws half blocks. Run `lazytypst --doctor`. If the line `terminal` says `half blocks`, use a terminal from the table that has an image method. Alacritty has no image protocol.

### tmux

tmux sits between the program and the terminal, and it can block the image data. Images in tmux are not tested. Newer versions of tmux have the option `allow-passthrough`, for example `set -g allow-passthrough on` in `~/.tmux.conf`. This fix is not tried.

### Alt keys do nothing

The page keys use `Alt`. Some terminals use `Alt` for their own keys, or they do not send it to the program. The `Alt` keys are tested with a test terminal. The settings of other terminals are not tested. Look for a setting that sends `Alt` as `Escape` or as the Meta key. This fix is not tried.

### typst not found

The program stops at start with a message that it cannot run `typst`. Install Typst (https://github.com/typst/typst#installation), and check that `typst --version` works in the same terminal. If the program is in another folder, give its path with `LAZYTYPST_TYPST`.

### The program stops with a folder error at start

The program makes the folder `lazytypst-<process id>` in the temporary directory, and only your user can open it. If a folder with that name exists and belongs to another user, the program stops with the message `Cannot make the folder`. It never deletes a folder of another user. The folder is safe to delete when no process with that number runs. Check with `ls /proc/<process id>`: if the command says that the file does not exist, delete the folder.

## How it works

- `src/browser.rs` finds the `.typ` files. `src/newfile.rs` checks the name of a new file and makes it.
- `src/editor.rs` holds the text area, the save, the 300 ms autosave, the screen layout, and the pane that shows the compile result.
- `src/preview.rs` holds the folder of the last good compile. The folder has one PNG file, named `page-<page>-of-<count>.png`, so the preview learns the page count from the name. It draws the page with the `ratatui-image` crate. It deletes the old folder when a new folder loads. It keeps the number of the wanted page.
- `src/compile.rs` parses each line of the Typst output into a diagnostic with a severity, a file, a line, and a column. It also runs `typst compile --format png --diagnostic-format short --root <folder> --pages <page>` as a job, inside the root folder. Typst exits with success and writes no file when the document has fewer pages than the page that the program asks for. The editor then compiles page 1, learns the page count, and compiles the last page. The main thread owns the `typst` process and checks it with `try_wait`. A thread reads the error output. Dropping the job kills the `typst` process. Each compile writes its one PNG file to its own new folder in the temporary directory. The editor deletes the folder of a compile that failed or that it killed. The program deletes the temporary directory when it exits. The PDF export is a second job of the same kind.
- `src/state.rs` reads and writes the state file with the main file of each project. `src/fsutil.rs` holds the safe write that the editor and the state file both use.
- `src/main.rs` reads the arguments and runs the event loop. `App::handle_key` handles the keys of the file list. Every 50 ms without a key, the loop calls the editor. The editor then runs the autosave if it is due and checks if a compile has finished.

## Contributing

The file `CONTRIBUTING.md` tells you how to build the program, which rules the code follows, and how a commit and a pull request must look. The page `site/index.html` is the source of the website. A test checks that it lists every key of `lazytypst --help`, so a change to a key changes the page too. The workflow `.github/workflows/pages.yml` publishes the folder `site/` when a push changes it.

## Changes

The file `CHANGELOG.md` lists the changes that a user can see, for each version. Each change that a user can see adds one line under `Unreleased` there.

## Releases

A tag that starts with `v`, for example `v0.1.0`, starts the workflow `.github/workflows/release.yml`. The workflow stops with an error if the tag and the version in `Cargo.toml` differ, or if `CHANGELOG.md` has no entry for the version, or if that entry still says that the version is not yet released. Then the workflow runs the tests, builds a static binary for x86_64 Linux, and checks the archive that it packs. At last, it makes a GitHub release with the archive, its checksum file, and the changelog entry as the text of the release.

A manual run of the workflow is a dry run. It builds and checks the archive and keeps it as an artifact for 7 days. It makes no release.

## Tests

`cargo test` runs the tests. Many tests run the real `typst` command, so `typst` must be in `PATH`. The tests run on Linux only, because some of them read `/proc`. `cargo clippy --all-targets -- -D warnings` must show no warning, and `cargo fmt --check` must pass.

GitHub Actions runs both on each push and on each pull request to `main`. The workflow is the file `.github/workflows/ci.yml`. It installs Typst 0.15.1 from the release page of Typst and checks the file against a fixed SHA-256 hash. It runs `cargo test` with the current stable version of Rust and with Rust 1.90. It runs clippy and `cargo fmt --check` with the stable version only. The run with Rust 1.90 is the check for the minimum Rust version in the requirements above.

## Measuring

`scripts/bench-pages.sh` compiles documents of 1, 50, 200, and 500 pages with the options of the program, once for all pages and once for one page. It prints the time and the disk use. Give other page counts as arguments. The script needs `typst` in `PATH`.

## License

lazytypst is free software. The license is the GNU General Public License, version 3 or any later version. The text is in the `LICENSE` file.
