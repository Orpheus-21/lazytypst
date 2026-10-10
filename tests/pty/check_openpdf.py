"""Ctrl-O opens the exported PDF with xdg-open. Without an export it says so. Without xdg-open the program runs on."""
import os, stat, tempfile, shutil
from ptyh import *

folder = project({"doc.typ": "= Hi\n", "zz.typ": ""})
bin_dir = tempfile.mkdtemp(prefix="lazytypst-bin-")
log = os.path.join(bin_dir, "log")
try:
    fake = os.path.join(bin_dir, "xdg-open")
    with open(fake, "w") as f:
        f.write(f"#!/bin/sh\necho \"$1\" > {log}\n")
    os.chmod(fake, 0o755)
    path = bin_dir + ":" + os.environ["PATH"]
    shots, _ = session(folder, [(ENTER, 0.5), (ctrl("o"), "No PDF yet"), (ctrl("e"), "Exported"), (ctrl("o"), "Opening"), (ESC, 0.4)], env_extra={"PATH": path})
    assert "No PDF yet" in shots[1], shots[1]
    assert "Opening" in shots[3], shots[3]
    import time; time.sleep(0.3)
    assert open(log).read().strip() == os.path.join(folder, "doc.pdf"), open(log).read()
    # no xdg-open in PATH: an error, and the program keeps running
    shots, _ = session(folder, [(ENTER, 0.5), (ctrl("e"), "Exported"), (ctrl("o"), "Cannot start"), (ESC, 0.4)], env_extra={"PATH": os.path.dirname(shutil.which("typst"))})
    assert "Cannot start xdg-open" in shots[2], shots[2]
finally:
    remove(folder)
    shutil.rmtree(bin_dir, ignore_errors=True)
print("open pdf check: OK")
