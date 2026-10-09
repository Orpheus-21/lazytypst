"""With LAZYTYPST_WATCH=1 one typst watch compiles all the edits. An error shows, and the repair is a good compile."""
import os
import shutil
import tempfile
from ptyh import *

folder = project({"doc.typ": "= Title\ntext\n"})
d = tempfile.mkdtemp(prefix="lazytypst-wrap-")
try:
    log = os.path.join(d, "log")
    wrap = os.path.join(d, "mytypst")
    with open(wrap, "w") as f:
        f.write(f'#!/bin/sh\necho "$1" >> {log}\nexec {shutil.which("typst")} "$@"\n')
    os.chmod(wrap, 0o755)
    steps = [
        (None, "OK in"),
        (b"a", "Saved"),
        (b"b", 0.2),
        (b"#nope ", "unknown variable"),   # an error
        (b"\x7f" * 6, "OK in"),            # the repair
        (ESC, 0.3),
    ]
    shots, _ = session(folder, steps, env_extra={"LAZYTYPST_WATCH": "1", "LAZYTYPST_TYPST": wrap})
    commands = open(log).read().split()
    assert commands.count("watch") == 1 and "compile" not in commands, commands
    assert "unknown variable" in shots[3], shots[3]
    assert "OK in" in shots[4], shots[4]
finally:
    remove(folder)
    shutil.rmtree(d, ignore_errors=True)
print("watch check: OK")
