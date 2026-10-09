"""F2 saves and lists the other files. Typing filters, Enter opens. A second F2 goes back."""
import os
from ptyh import *

folder = project({"a.typ": "= Alpha\n", "b.typ": "= Beta\n", "c.typ": "= Gamma\n"})
try:
    steps = [
        (ENTER, 1.5),
        (b"X", 0.2),
        (b"\x1bOQ", "Switch file"),      # F2
        (b"c.", 0.3),
        (b"\r", 1.5),
        (b"\x1bOQ", "Switch file"),
        (b"\x1bOQ", 1.5),                # F2 F2: back to a.typ
        (ESC, 0.5),
        (b"\x1b", 0.3),
    ]
    shots, _ = session(folder, steps)
    assert "a.typ" not in shots[2].split("Switch file")[1].split("\n", 1)[1], shots[2]
    assert "c.typ" in shots[3], shots[3]
    assert "Gamma" in shots[4], shots[4]
    assert "Xlpha" not in shots[4] and open(os.path.join(folder, "a.typ")).read().startswith("X= Alpha"), "saved"
    assert "Alpha" in shots[6], shots[6]
finally:
    remove(folder)
print("switch check: OK")
