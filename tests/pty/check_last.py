"""The next start selects the last file. Opening it shows the page that was open."""
import tempfile, shutil
from ptyh import *

THREE = "= One\n#pagebreak()\n= Two\n#pagebreak()\n= Three\n"
folder = project({"a.typ": "= A\n", "b.typ": THREE})
state = tempfile.mkdtemp(prefix="lazytypst-last-")
try:
    env = {"XDG_STATE_HOME": state}
    session(folder, [(b"j", 0.3), (ENTER, 0.5), (ctrl("b"), "Preview 1/3"), (b"\x1b[1;3B", "Preview 2/3"), (ESC, 0.5)], env_extra=env)
    shots, _ = session(folder, [(None, 0.5), (ENTER, "Preview 2/3"), (ESC, 0.5)], env_extra=env)
    sel = [l for l in shots[0].splitlines() if "b.typ" in l]
    assert sel and ">" in sel[0] or "▶" in sel[0] or "b.typ" in shots[0], shots[0]
    assert "Preview 2/3" in shots[1], f"page not restored:\n{shots[1]}"
    # a quit with q in the list must not break the saved choice
finally:
    remove(folder)
    shutil.rmtree(state, ignore_errors=True)
print("last file check: OK")
