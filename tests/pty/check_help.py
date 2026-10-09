"""The help window: ? and F1 open it, Enter runs a line, Alt-f works as an editing key."""
import os
from ptyh import *

folder = project({"a.typ": "one two\n", "b.typ": "= B\n"})
try:
    shots, _ = session(folder, [(None, 0.3), (b"?", "Keys in the editor"), (ESC, 0.3), (ENTER, 0.4), (b"\x1bOP", "Keys in the editor"), (ESC, 0.3), (b"\x1bf", 0.3), (b"X", 0.3), (ESC, 0.4)])
    assert "? help" in shots[0], shots[0]
    assert "Help" in shots[1] and "Keys in the file list:" in shots[1]
    assert "a.typ" in shots[2] and "Help" not in shots[2], shots[2]
    assert "F1 help" in shots[3]
    assert "Keys in the editor:" in shots[4]
    assert read(folder, "a.typ").startswith("one Xtwo"), repr(read(folder, "a.typ"))
    # Enter on a line runs it: E exports the PDF of the selected file
    shots, _ = session(folder, [(b"?", "Help"), (b"jjjjjj", 0.2), (b"\r", "Exported a.pdf")])
    assert os.path.exists(os.path.join(folder, "a.pdf"))
finally:
    remove(folder)
print("help check: OK")
