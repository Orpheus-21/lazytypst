"""s sorts the list by last change. The list shows ages."""
import os, time
from ptyh import *

folder = project({"a.typ": "= A\n", "b.typ": "= B\n"})
try:
    old = time.time() - 7200
    os.utime(os.path.join(folder, "b.typ"), (old, old))
    shots, _ = session(folder, [(None, 0.4), (b"s", 0.4), (b"s", 0.4)])
    assert "2 h" in shots[0] and "now" in shots[0], shots[0]
    lines = lambda shot: [l for l in shot.splitlines() if ".typ" in l]
    assert "a.typ" in lines(shots[0])[0] and "a.typ" in lines(shots[1])[0] and "newest first" in shots[1]
    # the selection moved with the file? a.typ is newest here, so the order matches; make b newest
    os.utime(os.path.join(folder, "b.typ"))
    os.utime(os.path.join(folder, "a.typ"), (old, old))
    shots, _ = session(folder, [(b"s", 0.4), (b"s", 0.4)])
    assert "b.typ" in lines(shots[0])[0], shots[0]
    assert "a.typ" in lines(shots[1])[0] and "newest" not in shots[1], shots[1]
finally:
    remove(folder)
print("sort check: OK")
