"""After a slow compile, the save comes at once and the next compile waits, and the pane says so."""
import os
import re
from ptyh import *

folder = project({"doc.typ": "#for i in range(4000) [= Heading #i\n#lorem(60)\n]\n"})
try:
    steps = [
        (None, "OK in"),          # the first compile of the slow document
        (b" ", "next in"),        # a key: the save follows, the compile waits
        (None, 4.5),              # the wait ends and the compile runs
        (ESC, 0.3),
    ]
    shots, _ = session(folder, steps, rows=30)
    assert re.search(r"next in \d\.\d s", shots[1]), shots[1]
    assert shots[2].splitlines()[-1].rstrip().endswith("1:2"), shots[2].splitlines()[-1]
    assert open(os.path.join(folder, "doc.typ")).read().startswith(" #for"), "the save did not wait"
finally:
    remove(folder)
print("slow wait check: OK")
