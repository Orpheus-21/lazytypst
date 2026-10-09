"""e in the list runs $EDITOR in the same terminal, then opens the file with a compile."""
import os, tempfile, shutil
from ptyh import *

folder = project({"a.typ": "= Old\n", "b.typ": "= B\n"})
bin_dir = tempfile.mkdtemp(prefix="lazytypst-ed-")
try:
    fake = os.path.join(bin_dir, "fakeed")
    with open(fake, "w") as f:
        f.write('#!/bin/sh\nprintf "\\n= Written by the editor\\n" >> "$1"\n')
    os.chmod(fake, 0o755)
    shots, _ = session(folder, [(b"e", "Written by the editor"), (ESC, 0.5)], env_extra={"EDITOR": fake, "VISUAL": ""})
    assert "Written by the editor" in shots[0] and "OK" in shots[0] or "Preview" in shots[0], shots[0]
    assert "Written by the editor" in read(folder, "a.typ")
    shots, _ = session(folder, [(b"e", "VISUAL")], env_extra={"EDITOR": "", "VISUAL": ""})
    assert "Set VISUAL or EDITOR" in shots[0], shots[0]
finally:
    remove(folder)
    shutil.rmtree(bin_dir, ignore_errors=True)
print("external editor check: OK")
