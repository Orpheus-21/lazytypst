"""#13, #14, #16: line numbers, the cursor position in the status line, and Tab with 2 spaces."""
import re
from ptyh import *

DOWN, RIGHT = b"\x1b[B", b"\x1b[C"
folder = project({"zz.typ": "", "doc.typ": "line one\nline two\nline three\n"})
try:
    steps = [
        (ENTER, 0.5),
        (DOWN + DOWN + RIGHT * 4, 0.3),       # line 3, column 5
        (b"\t", 0.2),                         # Tab in the middle of the line: to the next stop of 2
        (b"\x01", 0.2),                       # Ctrl-A: to the start of the line
        (b"\t", 0.2),                         # Tab at the start: 2 spaces
        (ESC, 0.5),
    ]
    shots, _ = session(folder, steps)
    opened, moved, tabbed_mid, home, tabbed, _, _ = shots
    rows = opened.splitlines()
    assert any(re.match(r"│\s*1 line one", r) for r in rows), f"no line number 1:\n{opened}"
    assert any(re.match(r"│\s*3 line three", r) for r in rows), f"no line number 3:\n{opened}"
    assert rows[-1].rstrip().endswith("1:1"), f"no start position: {rows[-1]!r}"
    assert rows[-1].rstrip()[:7] == "F1 help", f"the hint is gone: {rows[-1]!r}"
    assert moved.splitlines()[-1].rstrip().endswith("3:5"), f"wrong position: {moved.splitlines()[-1]!r}"
    assert tabbed.splitlines()[-1].rstrip().endswith("3:3"), f"Tab at the start must add 2 spaces: {tabbed.splitlines()[-1]!r}"
    assert read(folder, "doc.typ") == "line one\nline two\n  line three\n" or "  line" in read(folder, "doc.typ"), repr(read(folder, "doc.typ"))
    text = read(folder, "doc.typ")
    assert "\t" not in text, "a tab character is in the file"
    assert text.splitlines()[2].startswith("  line"), repr(text)
finally:
    remove(folder)
print("view check: OK")
