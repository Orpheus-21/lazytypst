"""F6 lists the saved versions. Enter restores the text from the start of the session, and Ctrl-Z takes that back."""
import os
from ptyh import *

folder = project({"doc.typ": "good text\n"})
try:
    steps = [
        (None, 1.5),
        (b"BAD ", "Saved"),          # an edit, saved by the autosave
        (b"\x1b[17~", "History"),    # F6
        (b"\r", "Restored"),         # Enter: the version of the start
        (b"\x1a", 0.4),              # Ctrl-Z: back to the bad text
        (ESC, 0.3),
    ]
    shots, _ = session(folder, steps)
    assert "-1 +1 lines" in shots[2] and "just now" in shots[2], shots[2]
    assert "1 good text" in shots[3] and "BAD" not in shots[3].split("History")[0], shots[3]
    assert "1 BAD good text" in shots[4], shots[4]
    assert open(os.path.join(folder, "doc.typ")).read().startswith("BAD good text"), "the history never changes the file by itself"
finally:
    remove(folder)
print("history check: OK")
