"""#2: the main file comes back in the next run, and the state file lives outside the project."""
import os, tempfile
from ptyh import *

folder = project({"book.typ": "= Hi\n", "notes.typ": "text\n"})
state = tempfile.mkdtemp(prefix="lazytypst-state-")
env = {"XDG_STATE_HOME": state}
try:
    shots, _ = session(folder, [(b"m", 0.3)], env_extra=env)          # mark book.typ (the first file)
    assert "book.typ [main]" in shots[0], shots[0]
    saved = os.path.join(state, "lazytypst", "main-files")
    assert os.path.isfile(saved), "no state file"
    assert not any(n.startswith(".") or n == "main-files" for n in os.listdir(folder)), "state leaked into the project"

    shots, _ = session(folder, [(None, 0.3)], env_extra=env)          # a new run
    assert "book.typ [main]" in shots[0], f"the mark did not come back:\n{shots[0]}"

    shots, _ = session(folder, [(b"m", 0.3)], env_extra=env)          # remove the mark
    assert "[main]" not in shots[0]
    shots, _ = session(folder, [(None, 0.3)], env_extra=env)
    assert "[main]" not in shots[0], "the removed mark came back"
finally:
    remove(folder, state)
print("state check: OK")
