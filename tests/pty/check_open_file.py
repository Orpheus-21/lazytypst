"""#23 and #22: open a .typ file from the command line, and select the first and the last file with g and G."""
import os, subprocess, tempfile
from ptyh import *

folder = project({"main.typ": "= Main\n", "chapters/one.typ": "= One\n", "chapters/two.typ": "= Two\n", "chapters/three.typ": "= Three\n", "notes.txt": "x\n"})
try:
    # An absolute file path: the editor opens at once. Esc shows the list of the folder of the file.
    target = os.path.join(folder, "chapters", "two.typ")
    shots, _ = session(target, [(None, 0.4), (ESC, 0.5)])
    opened, back = shots[0], shots[1]
    assert "two.typ" in opened and "Ctrl-S save" in opened and "= Two" in opened, f"the file did not open:\n{opened}"
    assert "one.typ" in back and "three.typ" in back and "main.typ" not in back, f"wrong list after Esc:\n{back}"
    selected = [l for l in back.splitlines() if "> " in l]
    assert selected and "two.typ" in selected[0], f"the opened file must be selected:\n{back}"

    # g and G in the list of a folder.
    shots, _ = session(os.path.join(folder, "chapters"), [(b"G", 0.2), (ENTER, 0.5), (ESC, 0.4), (b"g", 0.2), (ENTER, 0.5), (ESC, 0.4)])
    assert "two.typ" in shots[1] and "= Two" in shots[1], f"G then Enter must open the last file:\n{shots[1]}"
    assert "one.typ" in shots[4] and "= One" in shots[4], f"g then Enter must open the first file:\n{shots[4]}"

    # A relative path, run from another folder, and the exit code for a file that is not a .typ file.
    env = {k: v for k, v in os.environ.items() if k != "TERM_PROGRAM"}
    env["XDG_STATE_HOME"] = tempfile.mkdtemp(prefix="lazytypst-state-")
    run = subprocess.run([BINARY, "notes.txt"], cwd=folder, env=env, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=30)
    assert run.returncode == 2, f"exit code {run.returncode}"
    assert "notes.txt is not a .typ file" in run.stderr, run.stderr
    run = subprocess.run([BINARY, "no/such.typ"], cwd=folder, env=env, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=30)
    assert run.returncode == 1 and "Cannot open no/such.typ" in run.stderr, (run.returncode, run.stderr)
    remove(env["XDG_STATE_HOME"])
finally:
    remove(folder)
print("open file check: OK")
