"""An outside change of an included file starts a compile and the status line names the file."""
import os
from ptyh import *

folder = project({"zz.typ": "", "doc.typ": '#include "part.typ"\n', "part.typ": "= Part one\n"})
part = os.path.join(folder, "part.typ")
try:
    def outside():
        with open(part, "w") as f:
            f.write("= Part two\n")
        os.utime(part, (os.path.getatime(part), os.path.getmtime(part) + 5))

    steps = [
        (ENTER, 2.5),
        (outside, "part.typ changed"),
        (ESC, 0.5),
    ]
    shots, _ = session(folder, steps)
    assert "part.typ changed" in shots[1], f"no message:\n{shots[1]}"
finally:
    remove(folder)
print("deps check: OK")
