"""A bracketed paste arrives as one piece: tabs stay, and the terminal turns the mode on and off."""
import ptyh
from ptyh import *

folder = project({"zz.typ": "", "doc.typ": ""})
try:
    paste = b"\x1b[200~a\tb\r\n" + b"".join(b"line %d\n" % n for n in range(300)) + b"\x1b[201~"
    shots, _ = session(folder, [(ENTER, 0.5), (paste, 1.0), (ESC, 0.5)])
    data = read(folder, "doc.typ")
    assert data.startswith("a\tb\nline 0\n") and data.endswith("line 299\n\n"), repr(data[:40])
    assert data.count("\n") == 302, data.count("\n")
    raw = ptyh.last_raw
    assert b"\x1b[?2004h" in raw and raw.rfind(b"\x1b[?2004l") > raw.rfind(b"\x1b[?2004h"), "paste mode not set and reset"
finally:
    remove(folder)
print("paste check: OK")
