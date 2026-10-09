"""F7 turns the spell check on. Alt-; goes to the next unknown word and lists the choices. Enter replaces the word."""
import os
import stat
from ptyh import *

FAKE = """#!/bin/sh
echo '@(#) fake'
while read -r line; do
  w=${line#^}
  case "$w" in
    zz*) echo "& $w 2 0: fixa, fixb";;
    *) echo '*';;
  esac
  echo
done
"""

folder = project({"doc.typ": "hello zzworld okay\n"})
try:
    program = os.path.join(folder, "hunspell")
    with open(program, "w") as f:
        f.write(FAKE)
    os.chmod(program, 0o755)
    steps = [
        (None, 1.5),
        (b"\x1b[18~", "Spell check on"),   # F7
        (b"\x1b;", "Spelling: zzworld"),   # Alt-;
        (b"\r", "fixa"),                   # Enter: the first suggestion
        (ESC, 0.3),
    ]
    shots, _ = session(folder, steps, env_extra={"LAZYTYPST_SPELL": program})
    assert "fixa" in shots[3] and "zzworld" not in shots[3], shots[3]
    assert "Add \"zzworld\" to the dictionary" in shots[2], shots[2]
    assert open(os.path.join(folder, "doc.typ")).read() == "hello fixa okay\n"
finally:
    remove(folder)
print("spell check: OK")
