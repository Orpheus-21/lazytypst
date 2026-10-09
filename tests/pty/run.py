"""Runs every check of this folder, one after the other, and fails if one fails.

    cargo build && python3 tests/pty/run.py            # all checks
    python3 tests/pty/run.py check_search.py           # some checks
    LAZYTYPST_BIN=/path/to/lazytypst python3 tests/pty/run.py

Each check starts the program in a pseudo terminal and reads the screen. A check prints a line that ends
with OK when it passes. A check that hangs stops after 120 seconds.
"""
import glob
import os
import subprocess
import sys
import time

here = os.path.dirname(os.path.abspath(__file__))
names = sys.argv[1:] or sorted(os.path.basename(path) for path in glob.glob(os.path.join(here, "check_*.py")))
failed = []
for name in names:
    started = time.time()
    try:
        done = subprocess.run(
            [sys.executable, os.path.join(here, name)],
            cwd=here, capture_output=True, text=True, timeout=120,
        )
        ok = done.returncode == 0
        output = done.stdout + done.stderr
    except subprocess.TimeoutExpired as err:
        ok, output = False, f"timeout after {err.timeout} s\n"
    print(f"{'ok  ' if ok else 'FAIL'} {name} ({time.time() - started:.1f} s)", flush=True)
    if not ok:
        failed.append(name)
        print(output[-3000:], flush=True)
print(f"{len(names) - len(failed)} of {len(names)} checks passed")
sys.exit(1 if failed else 0)
