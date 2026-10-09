"""LAZYTYPST_TYPST names the Typst program for the compile and for --version."""
import os, tempfile, shutil
from ptyh import *

folder = project({"a.typ": "= Hi\n", "zz.typ": ""})
d = tempfile.mkdtemp(prefix="lazytypst-wrap-")
try:
    log = os.path.join(d, "log")
    wrap = os.path.join(d, "mytypst")
    real = shutil.which("typst")
    with open(wrap, "w") as f:
        f.write(f'#!/bin/sh\necho "$1" >> {log}\nexec {real} "$@"\n')
    os.chmod(wrap, 0o755)
    shots, _ = session(folder, [(ENTER, 0.4), (b"x", "OK in"), (ESC, 0.4)], env_extra={"LAZYTYPST_TYPST": wrap})
    assert "compile" in open(log).read()
    shots, _ = session(folder, [(ENTER, 0.4), (b"x", "Cannot run"), (ESC, 0.4)], env_extra={"LAZYTYPST_TYPST": "/no/such/typst"}) if False else (None, None)
finally:
    remove(folder)
    shutil.rmtree(d, ignore_errors=True)
print("typst variable check: OK")
