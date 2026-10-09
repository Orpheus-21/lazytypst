"""A project with a link that leaves it: no compile at open, a warning, and Ctrl-B compiles."""
import os, tempfile
from ptyh import *

folder = project({"a.typ": "= Hi\n", "zz.typ": ""})
secret = tempfile.mkstemp(prefix="lazytypst-secret-")[1]
try:
    os.symlink(secret, os.path.join(folder, "s.txt"))
    shots, _ = session(folder, [(ENTER, 1.5), (ctrl("b"), "OK in"), (ESC, 0.4)])
    assert "point outside the project" in shots[0] and "OK in" not in shots[0], shots[0]
    assert "OK in" in shots[1]
finally:
    remove(folder)
    os.remove(secret)
print("outside link check: OK")
