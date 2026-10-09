"""A narrow window starts stacked, a wide one side by side. F10 cycles the layouts, and the choice stays."""
import os
from ptyh import *

folder = project({"doc.typ": "text\n"})
state = os.path.join(folder, "state")
try:
    def titles(shot):
        rows = shot.splitlines()
        edit = next(n for n, row in enumerate(rows) if "doc.typ" in row)
        prev = next((n for n, row in enumerate(rows) if "Preview" in row), None)
        return edit, prev

    env = {"XDG_STATE_HOME": state}
    shots, _ = session(folder, [(None, 1.5), (b"\x1b[21~", "editor only"), (b"\x1b[21~", "side by side"), (ESC, 0.3)], cols=80, rows=30, env_extra=env)
    edit, prev = titles(shots[0])
    assert prev is not None and prev > edit, f"80 columns must start stacked:\n{shots[0]}"
    assert titles(shots[1])[1] is None, "editor only hides the preview"
    edit, prev = titles(shots[2])
    assert prev == edit, "side by side again"
    assert open(os.path.join(state, "lazytypst", "layout")).read() == "side"
    # The next run, in a narrow window, keeps the choice.
    shots, _ = session(folder, [(None, 1.5), (ESC, 0.3)], cols=80, rows=30, env_extra=env)
    edit, prev = titles(shots[0])
    assert prev == edit, f"the saved layout must win over the width:\n{shots[0]}"
    shots, _ = session(folder, [(None, 1.5), (ESC, 0.3)], cols=120, rows=30, env_extra={"XDG_STATE_HOME": os.path.join(folder, "other")})
    edit, prev = titles(shots[0])
    assert prev == edit, "120 columns start side by side"
finally:
    remove(folder)
print("layout check: OK")
