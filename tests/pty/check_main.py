"""#1: the main file. Mark it in the list, open a chapter, export, and check book.pdf."""
import os
from ptyh import *

folder = project({"book.typ": '#include "chapters/one.typ"\n', "chapters/one.typ": "= One\n"})
try:
    steps = [(b"m", 0.3), (b"j", 0.2), (ENTER, 0.5), (ctrl("e"), 3.0), (ESC, 0.5)]
    shots, _ = session(folder, steps)
    marked, opened, exported, back = shots[0], shots[2], shots[3], shots[4]
    assert "book.typ [main]" in marked, f"no mark in the list:\n{marked}"
    assert "chapters/one.typ (main: book.typ)" in opened, f"no main in the title:\n{opened}"
    assert "Exported" in exported and "book.pdf" in exported, f"no export of book.pdf:\n{exported}"
    assert os.path.exists(os.path.join(folder, "book.pdf")), "book.pdf is missing"
    assert not os.path.exists(os.path.join(folder, "chapters", "one.pdf")), "the chapter was exported"
    assert "book.typ [main]" in back, "the mark is gone after Esc"
finally:
    remove(folder)
print("main file check: OK")
