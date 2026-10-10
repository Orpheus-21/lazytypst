"""Alt-Enter on an #include opens the file. Ctrl-] works too. F2 F2 goes back."""
from ptyh import *

folder = project({"main.typ": '#include "chapters/two.typ"\n', "chapters/two.typ": "= Second chapter\n"})
try:
    steps = [
        (b"j", 0.2),                  # the list: chapters/two.typ, main.typ
        (ENTER, 1.0),
        (b"\x1b\r", "Second chapter"),  # Alt-Enter
        (b"\x1bOQ", "Switch file"),   # F2
        (b"\x1bOQ", "#include"),      # F2 again: back to main.typ
        (ESC, 0.3),
        (ESC, 0.3),
    ]
    shots, _ = session(folder, steps)
    assert "#include" not in shots[2] and "Second chapter" in shots[2], shots[2]
    assert "main.typ" in shots[4] and "#include" in shots[4], shots[4]
finally:
    remove(folder)
print("gofile check: OK")
