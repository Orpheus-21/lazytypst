"""Ctrl-Q in the editor saves the text and quits the program."""
from ptyh import *

folder = project({"zz.typ": "", "doc.typ": "= Title\n"})
try:
    try:
        session(folder, [(ENTER, 0.5), (b"X", 0.3), (ctrl("q"), 1.0)])
    except OSError:
        pass  # the harness sends a last q to a program that has already ended
    assert read(folder, "doc.typ") == "X= Title\n", read(folder, "doc.typ")
finally:
    remove(folder)
print("ctrl-q check: OK")
