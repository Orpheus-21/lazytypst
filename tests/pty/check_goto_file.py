"""Ctrl-G opens the file of an error in another file of the project."""
from ptyh import *

folder = project({"main.typ": '#include "ch.typ"\n', "ch.typ": "ok\n#nope()\n"})
try:
    shots, _ = session(folder, [(b"j", 0.2), (b"m", 0.2), (ENTER, 0.3), (ctrl("b"), "unknown variable"), (ctrl("g"), 0.8), (ESC, 0.4)])
    assert "ch.typ" in shots[4] and "2:1" in shots[4].splitlines()[-1], shots[4]
finally:
    remove(folder)
print("goto file check: OK")
