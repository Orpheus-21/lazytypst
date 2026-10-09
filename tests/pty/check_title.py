"""#25: the window title. A file name must not inject an escape sequence."""
import os
import ptyh
from ptyh import *

folder = project({"report.typ": "= R\n", "chapters/one.typ": "= One\n"})
evil = project({"x\x1b]0;PWNED\x07y.typ": "= E\n"})
try:
    name = os.path.basename(folder)
    session(folder, [(None, 0.3), (ENTER, 0.5), (ESC, 0.5)])
    titles, raw = ptyh.last_titles, ptyh.last_raw
    assert titles[0] == f"lazytypst: {name}", f"list title: {titles[0]!r}"
    assert titles[1] == "lazytypst: chapters/one.typ", f"editor title: {titles[1]!r}"
    assert titles[2] == f"lazytypst: {name}", f"title after Esc: {titles[2]!r}"
    assert raw.count(b"\x1b[22;0t") == 1 and raw.endswith(b"\x1b[?1049l\x1b[?25h") or b"\x1b[23;0t" in raw, "no save and restore of the title"
    assert raw.index(b"\x1b[22;0t") < raw.index(b"\x1b]0;lazytypst:"), "the title was saved after it was set"
    assert raw.rindex(b"\x1b[23;0t") > raw.rindex(b"\x1b]0;lazytypst:"), "the title was restored before the last change"

    session(evil, [(None, 0.3), (ENTER, 0.5), (ESC, 0.5)])
    raw = ptyh.last_raw
    assert b"\x1b]0;PWNED" not in raw, "a file name started its own escape sequence"
    assert any("PWNED" in t and "\x1b" not in t and "\x07" not in t for t in ptyh.last_titles), ptyh.last_titles
finally:
    remove(folder, evil)
print("title check: OK")
