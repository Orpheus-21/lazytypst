"""A folder with one .typ file opens it at once. Esc shows the list. Two files start in the list."""
from ptyh import *

one = project({"doc.typ": "= Hello\n"})
two = project({"a.typ": "= A\n", "b.typ": "= B\n"})
try:
    shots, _ = session(one, [(None, 0.5), (ESC, 0.5)])
    assert "Hello" in shots[0] and "Preview" in shots[0], f"not in the editor:\n{shots[0]}"
    assert "doc.typ" in shots[1] and "Preview" not in shots[1], f"no list after Esc:\n{shots[1]}"
    shots, _ = session(two, [(None, 0.5)])
    assert "a.typ" in shots[0] and "b.typ" in shots[0] and "Preview" not in shots[0]
finally:
    remove(one, two)
print("one file check: OK")
