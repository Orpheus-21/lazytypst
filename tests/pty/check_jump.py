"""#21 and #15: Alt-End and Alt-Home turn to the last and the first page. The cursor comes back after Esc."""
from ptyh import *

THREE = "= One\n#pagebreak()\n= Two\n#pagebreak()\n= Three\n"
folder = project({"doc.typ": THREE, "other.typ": "x\n"})
try:
    steps = [
        (ENTER, 0.5), (b"\x1b[1;3F", 0.4),                    # Alt-End before any compile: nothing happens
        (ctrl("b"), "Preview 1/3"),
        (b"\x1b[1;3F", "Preview 3/3"),                        # Alt-End
        (b"\x1b[1;3H", "Preview 1/3"),                        # Alt-Home
        (b"\x1b[B\x1b[B\x1b[C\x1b[C", 0.3),                   # cursor to line 3, column 3
        (ESC, 0.5), (ENTER, 0.5),                             # close and open the same file again
        (ESC, 0.5),                                           # close it, so that q quits the program
    ]
    shots, _ = session(folder, steps)
    early, last, first, moved, reopened = shots[1], shots[3], shots[4], shots[5], shots[7]
    assert "Preview" in early, f"the editor must stay up after an early Alt-End:\n{early}"
    assert "Preview 3/3" in last and "Preview 1/3" in first
    assert moved.splitlines()[-1].rstrip().endswith("3:3"), moved.splitlines()[-1]
    assert reopened.splitlines()[-1].rstrip().endswith("3:3"), f"the cursor did not come back: {reopened.splitlines()[-1]!r}"
finally:
    remove(folder)
print("jump check: OK")
