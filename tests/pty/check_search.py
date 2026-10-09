"""Ctrl-F searches plain text. Enter goes to the next match. Alt-R switches to a regular expression, where an invalid pattern shows an error. Esc closes the prompt."""
from ptyh import *

folder = project({"a.typ": "Item one\nsecond f(x)\nItem two\n", "b.typ": ""})
try:
    steps = [(ENTER, 0.4), (ctrl("f"), "Search (text):"), (b"Item", 0.3), (b"\r", 0.3), (b"\r", 0.3),
             (b"\x1br", "Search (regex):"), (b"(", "Invalid pattern"), (ESC, 0.3), (ESC, 0.4)]
    shots, _ = session(folder, steps)
    assert "Search (text):" in shots[1]
    assert shots[3].rstrip().splitlines()[-1].rstrip().endswith("3:1"), shots[3].splitlines()[-1]
    assert shots[4].rstrip().splitlines()[-1].rstrip().endswith("1:1"), shots[4].splitlines()[-1]
    assert "Invalid pattern" in shots[6]
    assert "Search" not in shots[7].splitlines()[-1]
finally:
    remove(folder)
print("search check: OK")
