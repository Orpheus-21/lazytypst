"""The editor colors a heading and a comment, and NO_COLOR removes the colors."""
import re
import ptyh
from ptyh import *

BLUE = re.compile(rb"\x1b\[(?:[0-9;]*;)?(?:34|38;5;4)[;m]")
folder = project({"a.typ": "= Heading\n// a note\n#set text(size: 11pt)\n", "zz.typ": ""})
try:
    session(folder, [(ENTER, 0.5), (b"\x1b[B", 0.3), (ESC, 0.4)])
    assert BLUE.search(ptyh.last_raw), "no blue heading"
    session(folder, [(ENTER, 0.5), (b"\x1b[B", 0.3), (ESC, 0.4)], env_extra={"NO_COLOR": "1"})
    assert not BLUE.search(ptyh.last_raw), "color with NO_COLOR"
finally:
    remove(folder)
print("syntax check: OK")
