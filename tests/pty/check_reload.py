"""An outside change loads into an unedited buffer. An edited buffer keeps its text. A deleted file does not crash."""
import os
from ptyh import *

folder = project({"zz.typ": "", "doc.typ": "= Old title\n"})
path = os.path.join(folder, "doc.typ")
try:
    def outside():
        with open(path, "w") as f:
            f.write("= Fresh title\n")
        os.utime(path, (os.path.getatime(path), os.path.getmtime(path) + 5))

    def delete():
        os.remove(path)

    steps = [
        (ENTER, "Old title"),
        (outside, "Fresh title"),
        (delete, "gone"),
        (ESC, 0.5),
    ]
    shots, _ = session(folder, steps)
    assert "Fresh title" in shots[1], f"no reload:\n{shots[1]}"
    assert "gone" in shots[2], f"no gone message:\n{shots[2]}"
finally:
    remove(folder)
print("reload check: OK")
