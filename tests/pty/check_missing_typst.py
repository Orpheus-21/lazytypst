"""#7 and #51: without typst in PATH, the program prints a message and exits with code 1 before it draws anything."""
import os, subprocess, tempfile
from ptyh import *

TMP = tempfile.gettempdir()
folder = project({"zz.typ": "", "a.typ": "= A\n"})
empty_path = tempfile.mkdtemp(prefix="lazytypst-nopath-")
try:
    before = set(os.listdir(TMP))
    env = {"PATH": empty_path, "HOME": os.environ["HOME"], "XDG_STATE_HOME": tempfile.mkdtemp(prefix="lazytypst-state-")}
    run = subprocess.run([BINARY, folder], env=env, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=30)
    assert run.returncode == 1, f"exit code {run.returncode}"
    assert "typst" in run.stderr and "not found in PATH" in run.stderr, run.stderr
    assert "https://github.com/typst/typst#installation" in run.stderr, run.stderr
    assert run.stdout == "", f"the program printed on stdout: {run.stdout!r}"
    assert "\x1b[?1049h" not in run.stderr + run.stdout, "the program entered the alternate screen"
    new = set(os.listdir(TMP)) - before - {os.path.basename(env["XDG_STATE_HOME"])}
    assert not [n for n in new if n.startswith("lazytypst-")], f"a temporary folder was made: {new}"
    remove(env["XDG_STATE_HOME"])

    # --version works without typst and says so. The exit code stays 0.
    for flag in ("--version", "-V"):
        run = subprocess.run([BINARY, flag], env=env, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=30)
        assert run.returncode == 0, f"exit code {run.returncode}"
        lines = run.stdout.splitlines()
        assert len(lines) == 2 and lines[0].startswith("lazytypst ") and lines[1] == "typst: not found in PATH", run.stdout

    # --version with typst shows both versions.
    run = subprocess.run([BINARY, "--version"], stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=30)
    lines = run.stdout.splitlines()
    assert run.returncode == 0 and len(lines) == 2 and lines[0].startswith("lazytypst ") and lines[1].startswith("typst 0."), run.stdout

    # With typst in PATH the program starts as before.
    shots, _ = session(folder, [(None, 0.3)])
    assert "a.typ" in shots[0], shots[0]
finally:
    remove(folder, empty_path)
print("missing typst check: OK")
