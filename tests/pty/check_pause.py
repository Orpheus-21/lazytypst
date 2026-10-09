"""F5 pauses the live compile. Ctrl-B still compiles."""
from ptyh import *

folder = project({"doc.typ": "#nope()\n", "zz.typ": ""})
try:
    shots, _ = session(folder, [(ENTER, "unknown variable"), (b"\x1b[15~", "paused"), (b"\x0b", 2.0), (ctrl("b"), "OK in"), (b"\x1b[15~", "Live compile on"), (ESC, 0.4)])
    assert "Compile (paused)" in shots[1], shots[1]
    assert "unknown variable" in shots[2] and read(folder, "doc.typ") == "", shots[2]
    assert "OK in" in shots[3]
finally:
    remove(folder)
print("pause check: OK")
