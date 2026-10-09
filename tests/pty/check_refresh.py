"""#3: r reads the folder again."""
import os
from ptyh import *

folder = project({"zz.typ": "", "a.typ": "= A\n"})
try:
    def make_file():
        with open(os.path.join(folder, "new.typ"), "w") as f:
            f.write("= New\n")

    def delete_file():
        os.remove(os.path.join(folder, "a.typ"))

    steps = [(make_file, 0.2), (None, 0.3), (b"r", 0.3), (delete_file, 0.2), (b"r", 0.3)]
    shots, _ = session(folder, steps)
    before, after_make, after_delete = shots[1], shots[2], shots[4]
    assert "new.typ" not in before, "the new file showed without r"
    assert "new.typ" in after_make and "a.typ" in after_make, f"r did not show the new file:\n{after_make}"
    assert "new.typ" in after_delete and "a.typ" not in after_delete.replace("new.typ", ""), f"r did not drop the file:\n{after_delete}"
finally:
    remove(folder)
print("refresh check: OK")
