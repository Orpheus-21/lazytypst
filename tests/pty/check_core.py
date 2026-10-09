"""The core flow: open, type, autosave, compile, preview, pages, export, and Esc back."""
import os
from ptyh import *

THREE = "= One\n#pagebreak()\n= Two\n#pagebreak()\n= Three\n"
folder = project({"zz.typ": "", "doc.typ": THREE})
try:
    def white_cells(screen_text):
        return None  # the text screen has no colors; page presence is checked by the title instead

    steps = [
        (ENTER, 0.5),                       # open
        (b"X", 3.0),                        # the autosave runs and the live compile finishes
        (b"\x1b[1;3B", "Preview 2/3"),       # Alt-Down: page 2
        (b"\x1b[1;3B", "Preview 3/3"),       # page 3
        (b"\x1b[1;3B", 1.5),                 # stays on page 3 (no compile starts)
        (b"\x1b[1;3A", "Preview 2/3"),       # Alt-Up: page 2
        (ctrl("e"), 3.0),                   # export the PDF
        (ESC, 0.5),                         # back to the list
    ]
    shots, _ = session(folder, steps)
    live, p2, p3, p3b, back2, exported, closed = shots[1], shots[2], shots[3], shots[4], shots[5], shots[6], shots[7]
    assert "Preview 1/3" in live and "OK" in live, f"no live compile and preview:\n{live}"
    assert "Preview 2/3" in p2 and "Preview 3/3" in p3 and "Preview 3/3" in p3b and "Preview 2/3" in back2
    assert "Exported" in exported and "doc.pdf" in exported, f"no export:\n{exported}"
    assert read(folder, "doc.typ") == "X" + THREE, "the autosave did not write the text"
    assert open(os.path.join(folder, "doc.pdf"), "rb").read(4) == b"%PDF"
    assert "doc.typ" in closed and "Preview" not in closed, "Esc did not go back to the list"

    # An error keeps the last good page and shows the error. Esc saves and closes.
    steps = [(ENTER, 0.5), (b"#nope()\r", 3.0), (ctrl("g"), 0.3), (ESC, 0.5)]
    shots, _ = session(folder, steps)
    assert "error" in shots[1] and "unknown variable" in shots[1], f"no error shown:\n{shots[1]}"
    assert "Error at 1:1" in shots[2], f"Ctrl-G did not work:\n{shots[2]}"
    assert read(folder, "doc.typ").startswith("#nope()\nX"), "Esc did not keep the text"
finally:
    remove(folder)
print("core check: OK")
