"""The mouse is off at first. F9 takes it. A click puts the cursor, and the wheel scrolls."""
import os
from ptyh import *

def sgr(button, column, row, press=True):
    return f"\x1b[<{button};{column};{row}{'M' if press else 'm'}".encode()

text = "hello world\nsecond line\n" + "".join(f"line {n}\n" for n in range(3, 60))
folder = project({"doc.typ": text})
state = os.path.join(folder, "state")
try:
    steps = [
        (None, 1.0),
        (sgr(0, 10, 2) + sgr(0, 10, 2, False) + b"A", 0.4),   # the mouse is off: the click does nothing, A is typed at 1:1
        (b"\x1b[20~", "Mouse on"),                             # F9
        (sgr(0, 10, 3) + sgr(0, 10, 3, False) + b"B", 0.4),   # click on line 2, column 6 (row 1 is the border) (text starts at column 5), B is typed there
        (sgr(65, 10, 5) + sgr(65, 10, 5), 0.4),               # the wheel down (button 65), twice
        (ESC, 0.3),
    ]
    shots, _ = session(folder, steps, env_extra={"XDG_STATE_HOME": state})
    assert "1 Ahello world" in shots[1], shots[1]
    assert "secoBnd line" in shots[3], shots[3]
    assert any("line 6 " in row or "line 7 " in row for row in shots[4].splitlines()), shots[4]
    assert open(os.path.join(state, "lazytypst", "mouse")).read() == "on"
finally:
    remove(folder)
print("mouse check: OK")
