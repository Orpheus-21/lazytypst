"""#58: the document gets shorter than the page on screen. The preview must land on the last page."""
import os
from ptyh import *

THREE = "= One\n#pagebreak()\n= Two\n#pagebreak()\n= Three\n"
folder = project({"doc.typ": THREE})
try:
    def shorten():
        with open(os.path.join(folder, "doc.typ"), "w") as f:
            f.write("= One\n#pagebreak()\n= Two\n")

    steps = [
        (ENTER, 0.5), (ctrl("b"), 2.5),          # page 1 of 3
        (b"\x1b[1;3B", "Preview 2/3"), (b"\x1b[1;3B", "Preview 3/3"),  # page 3 of 3
        (shorten, 0.1), (ctrl("b"), "Preview 2/2"),  # the document has 2 pages now
        (ESC, 0.5),
    ]
    shots, _ = session(folder, steps)
    on3, shrunk = shots[3], shots[5]
    assert "Preview 3/3" in on3, f"did not reach page 3:\n{on3}"
    assert "Preview 2/2" in shrunk, f"the preview did not land on the last page:\n{shrunk}"
    assert "OK" in shrunk and "unknown" not in shrunk, shrunk
finally:
    remove(folder)
print("shrink check: OK")
