"""F11 shows the preview on the full screen. + zooms, 0 fits, typing does nothing, F11 and Esc go back."""
from ptyh import *

folder = project({"doc.typ": "= Title\n", "zz.typ": ""})
try:
    steps = [(ENTER, 0.4), (ctrl("b"), "Preview 1/1"), (b"\x1b[23~", "F11 back to editor"), (b"abc", 0.3),
             (b"+", "150%"), (b"\x1b[B", 0.3), (b"0", "100%"), (b"\x1b[23~", "Ctrl-S save"), (ESC, 0.4)]
    shots, _ = session(folder, steps)
    assert "F11 back to editor" in shots[2] and "= Title" not in shots[2], shots[2]
    assert "150%" in shots[4]
    assert "150%" not in shots[6]
    assert "Ctrl-S save" in shots[7]
    assert read(folder, "doc.typ") == "= Title\n"
finally:
    remove(folder)
print("full preview check: OK")
