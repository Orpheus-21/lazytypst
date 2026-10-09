"""Alt-S replaces: Enter replaces one and goes on, Alt-A replaces all, Ctrl-Z takes it back."""
import os
from ptyh import *

folder = project({"zz.typ": "", "doc.typ": "cat and cat\nthe cat\n"})
try:
    steps = [
        (ENTER, 1.5),
        (b"\x06cat", 0.3),            # Ctrl-F, type cat
        (b"\x1bs", "Replace with"),   # Alt-S
        (b"dog", 0.2),
        (b"\r", "left"),              # Enter: one replaced
        (b"\x1ba", "Replaced 2"),     # Alt-A: all replaced
        (ESC, 0.3),
        (b"\x1a", 0.4),
        (ESC, 0.3),
    ]
    shots, _ = session(folder, steps)
    assert "Replaced. 2 left" in shots[4], f"no one-replace message:\n{shots[4]}"
    assert "Replaced 2 matches" in shots[5], f"no all message:\n{shots[5]}"
    assert "dog and dog" in shots[5] and "the dog" in shots[5], shots[5]
    assert "dog and cat" in shots[7] and "the cat" in shots[7], f"undo:\n{shots[7]}"
finally:
    remove(folder)
print("replace check: OK")
