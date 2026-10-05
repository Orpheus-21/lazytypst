# Implementation Plan: lazytypst

## Overview

`lazytypst` is a terminal program. It has two jobs. First, it lists the Typst projects in a folder. Second, it edits one `.typ` file and shows a live preview of the compiled page. The program calls the `typst` CLI as a child process. It does not link to the Typst compiler.

## Architecture Decisions

- Language: Rust. Crates: `ratatui`, `crossterm`, `ratatui-textarea`, `ratatui-image`. Reason: the crates give the editor, the layout, and the image widget. We write none of them.
- Preview: `typst compile file.typ out-{p}.png`. The program shows page 1 with `ratatui-image`. The crate picks kitty, sixel, iTerm2, or half blocks. It does not need `chafa`.
- Live update: the program saves the file 300 ms after the last key. Then it runs one `typst compile`. If a new key arrives, the program kills the old compile first.
- Errors: `--diagnostic-format short`. The program shows each line in a pane under the editor.
- Browser: the program walks the root folder (default: the current folder, depth 3). It lists each `.typ` file. The user opens one with Enter.
- No config file. No plugin system. No Git features. We add them only when the user asks.

## Task List

### Phase 1: Foundation
- [ ] Task 1: Project skeleton and quit key
- [ ] Task 2: Project browser

### Checkpoint: Foundation
- [ ] `cargo build` passes. The browser lists the `.typ` files of a test folder.

### Phase 2: Editor and compile
- [ ] Task 3: Text editor with save
- [ ] Task 4: Compile and error pane

### Checkpoint: Editor
- [ ] The user opens a file, edits it, saves it, and sees the compile errors.

### Phase 3: Preview
- [ ] Task 5: Show the PNG page
- [ ] Task 6: Live update

### Checkpoint: Live preview
- [ ] The preview changes after the user stops typing. No stale compile overwrites a new one.

### Phase 4: Finish
- [ ] Task 7: Page keys and PDF export

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| The terminal cannot show images (`TERM` is `xterm-256color`, `kitty` is not installed) | High | `ratatui-image` falls back to half blocks. Task 5 tests this first. |
| Fast typing starts many compiles | Medium | Debounce 300 ms. Kill the old child process. |
| Imports with relative paths break if the file is not saved | Medium | Save to the real file before each compile. |
| `ratatui-textarea` has no Typst syntax color | Low | Accept plain text for now. |

## Open Questions

- None. The user chose Rust. The terminal is Ghostty, which supports the kitty graphics protocol, so the preview shows real pixels.
