"""E in the list exports the PDF of the selected file. An error shows the error line."""
import os
from ptyh import *

folder = project({"a.typ": "= A\n", "bad.typ": "#nope()\n"})
try:
    shots, _ = session(folder, [(b"E", "Exported a.pdf"), (b"j", 0.2), (b"E", "unknown variable")])
    assert open(os.path.join(folder, "a.pdf"), "rb").read(4) == b"%PDF"
    assert "bad.typ:1:" in shots[2], shots[2]
    assert not os.path.exists(os.path.join(folder, "bad.pdf"))
finally:
    remove(folder)
print("list export check: OK")
