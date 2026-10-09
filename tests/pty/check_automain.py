"""A folder with main.typ marks it as the main file by itself. The mark can be removed, and then it stays removed."""
import tempfile, shutil
from ptyh import *

folder = project({"main.typ": '#include "ch.typ"\n', "ch.typ": "= One\n"})
state = tempfile.mkdtemp(prefix="lazytypst-state-")
try:
    env = {"XDG_STATE_HOME": state}
    shots, _ = session(folder, [(None, 0.4), (b"j", 0.2), (b"m", 0.3)], env_extra=env)
    assert "main.typ [main]" in shots[0] and "(found)" in shots[0], shots[0]
    assert "[main]" not in shots[2], "m must remove the mark:\n" + shots[2]
    shots, _ = session(folder, [(None, 0.4)], env_extra=env)
    assert "[main]" not in shots[0], "the removed mark must stay removed:\n" + shots[0]
finally:
    remove(folder)
    shutil.rmtree(state, ignore_errors=True)
print("auto main check: OK")
