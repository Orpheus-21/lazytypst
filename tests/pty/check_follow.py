"""The preview follows the cursor: when the cursor enters another section, it goes to the page of the heading."""
from ptyh import *

text = "#set page(width: 8cm, height: 5cm)\n= One\ntext one\n#pagebreak()\n== Two A\ntext two\n#pagebreak()\n= Three\ntext three\n"
folder = project({"doc.typ": text})
DOWN = b"\x1b[B"
try:
    steps = [
        (None, "Preview 1/3"),
        (None, 1.5),                    # the answer about the headings
        (DOWN * 4, "Preview 2/3"),      # the cursor enters "== Two A"
        (DOWN * 4, "Preview 3/3"),      # the cursor enters "= Three"
        (b"\x1b[13~", "stays on its page"),  # F3: off
        (ESC, 0.3),
    ]
    shots, _ = session(folder, steps, rows=30)
    assert "Preview 3/3" in shots[3], shots[3]
finally:
    remove(folder)
print("follow check: OK")
