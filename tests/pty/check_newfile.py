"""#4: n makes a new file from the list."""
import os
from ptyh import *

folder = project({"zz.typ": "", "a.typ": "= A\n"})
try:
    steps = [
        (b"n", 0.3),
        (b"qj", 0.3),                     # must type, not quit and not move
        (b"\x7f\x7f", 0.2),               # Backspace twice
        (b"chapters/two", 0.3),
        (ENTER, 0.8),                     # makes the file and opens it
        (b"Hello", 0.6),                  # the autosave runs
        (ESC, 0.5),
    ]
    shots, _ = session(folder, steps)
    prompt, typed, opened, back = shots[0], shots[3], shots[4], shots[6]
    assert "New file:" in prompt, prompt
    assert "New file: chapters/two" in typed, f"the prompt text is wrong:\n{typed}"
    assert "chapters/two.typ" in opened and "Ctrl-S save" in opened, f"the editor did not open:\n{opened}"
    assert read(folder, "chapters/two.typ") == "Hello\n", repr(read(folder, "chapters/two.typ"))
    assert "chapters/two.typ" in back and "New file:" not in back, f"the list is wrong:\n{back}"

    shots, _ = session(folder, [(b"n", 0.2), (b"../x", 0.2), (ENTER, 0.3), (ESC, 0.3)])
    assert "inside the project" in shots[2], shots[2]
    assert not os.path.exists(os.path.join(os.path.dirname(folder), "x.typ"))
finally:
    remove(folder)
print("new file check: OK")
