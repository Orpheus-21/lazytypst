"""#5: Ctrl-G goes to the first error. A typed letter shows where the cursor is."""
from ptyh import *

folder = project({
    "doc.typ": "= A\n\nbc #nope()\n",
    "ok.typ": "= Fine\n",
    "é.typ": "é #nope()\n",
})
try:
    # doc.typ is the first file after sorting? Sort order is by path: doc.typ, ok.typ, é.typ.
    steps = [(ENTER, 0.5), (ctrl("b"), 2.5), (ctrl("g"), 0.3), (b"X", 0.2), (ESC, 0.5)]
    shots, _ = session(folder, steps)
    went, typed = shots[2], shots[3]
    assert "Error at 3:4: unknown variable: nope" in went, f"no message:\n{went}"
    assert "bc X#nope()" in typed, f"the cursor was not on the #:\n{typed}"

    # A file that compiles: Ctrl-G says that there is no error.
    steps = [(b"j", 0.2), (ENTER, 0.5), (ctrl("b"), 2.5), (ctrl("g"), 0.3), (ESC, 0.5)]
    shots, _ = session(folder, steps)
    assert "No error to go to." in shots[3], shots[3]

    # A non-ASCII line: the column counts characters.
    steps = [(b"jj", 0.2), (ENTER, 0.5), (ctrl("b"), 2.5), (ctrl("g"), 0.3), (b"X", 0.2), (ESC, 0.5)]
    shots, _ = session(folder, steps)
    assert "é X#nope()" in shots[4], f"wrong column for a character that is not ASCII:\n{shots[4]}"
finally:
    remove(folder)
print("goto check: OK")
