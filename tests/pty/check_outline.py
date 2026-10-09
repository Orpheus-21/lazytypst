"""F4 shows the outline. Typing filters, Enter jumps to the heading."""
from ptyh import *

text = "= One\n" + "filler\n" * 30 + "== Two\n" + "filler\n" * 30 + "= Three\nlast\n"
folder = project({"doc.typ": text})
try:
    steps = [
        (None, 1.5),
        (b"\x1bOS", "Outline"),   # F4
        (b"thr", 0.3),
        (b"\r", 0.6),
        (ESC, 0.3),
        (b"\x1b", 0.3),
    ]
    shots, _ = session(folder, steps)
    assert "Outline" in shots[1] and "Two" in shots[1], shots[1]
    assert "Outline" not in shots[3] and "Three" in shots[3] and "63:1" in shots[3], shots[3]
finally:
    remove(folder)
print("outline check: OK")
