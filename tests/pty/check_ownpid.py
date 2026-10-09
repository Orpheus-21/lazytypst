"""#59: a leftover folder with the own pid. A real folder is replaced. A symlink stops the start."""
import os, subprocess, tempfile
from ptyh import *

TMP = tempfile.gettempdir()
folder = project({"zz.typ": "", "a.typ": "= A\n"})
elsewhere = tempfile.mkdtemp(prefix="lazytypst-elsewhere-")
try:
    # 1. A real leftover folder with stale content.
    def leave_old_folder(pid):
        old = os.path.join(TMP, f"lazytypst-{pid}")
        os.makedirs(os.path.join(old, "0"))
        open(os.path.join(old, "0", "page-1-of-1.png"), "w").write("old")
        open(os.path.join(old, "stale-marker"), "w").write("old")

    seen = {}
    def look(pid):
        mine = os.path.join(TMP, f"lazytypst-{pid}")
        seen["exists"] = os.path.isdir(mine)
        seen["content"] = sorted(os.listdir(mine)) if os.path.isdir(mine) else None

    shots, pid = session(folder, [(look, 0.2)], child_setup=leave_old_folder, env_extra={"XDG_RUNTIME_DIR": ""})
    assert "a.typ" in shots[0], f"the program did not start:\n{shots[0]}"
    assert seen == {"exists": True, "content": []}, f"the old folder was not replaced: {seen}"
    assert not os.path.exists(os.path.join(TMP, f"lazytypst-{pid}")), "the folder stays after quit"

    # 2. A symlink at the path: the program stops with a message, and the target stays.
    open(os.path.join(elsewhere, "keep"), "w").write("keep")
    def plant_link(*_):
        os.symlink(elsewhere, os.path.join(TMP, f"lazytypst-{os.getpid()}"))

    env = {k: v for k, v in os.environ.items() if k != "TERM_PROGRAM"}
    env["XDG_STATE_HOME"] = tempfile.mkdtemp(prefix="lazytypst-state-")
    env["XDG_RUNTIME_DIR"] = ""
    run = subprocess.run([BINARY, folder], env=env, stdin=subprocess.DEVNULL, capture_output=True, text=True,
                         preexec_fn=plant_link, timeout=30)
    assert run.returncode != 0, "the program started although a link was in the way"
    assert "Cannot make the folder" in run.stderr, run.stderr
    assert open(os.path.join(elsewhere, "keep")).read() == "keep" and os.listdir(elsewhere) == ["keep"], "the link target changed"
    # Remove the planted link (its name holds the pid of the stopped program).
    for name in os.listdir(TMP):
        path = os.path.join(TMP, name)
        if name.startswith("lazytypst-") and name[10:].isdigit() and os.path.islink(path) and os.readlink(path) == elsewhere:
            os.unlink(path)
    remove(env["XDG_STATE_HOME"])

    # 3. With XDG_RUNTIME_DIR set to a folder of the user, the page folder is made inside it, and not in /tmp.
    runtime = tempfile.mkdtemp(prefix="lazytypst-runtime-")
    inside = {}
    def look_runtime(pid):
        inside["runtime"] = os.path.isdir(os.path.join(runtime, f"lazytypst-{pid}"))
        inside["tmp"] = os.path.exists(os.path.join(TMP, f"lazytypst-{pid}"))
    shots, pid = session(folder, [(look_runtime, 0.2)], env_extra={"XDG_RUNTIME_DIR": runtime})
    assert inside == {"runtime": True, "tmp": False}, inside
    assert os.listdir(runtime) == [], "the folder stays in the runtime folder after quit"
    remove(runtime)
finally:
    remove(folder, elsewhere)
print("own pid check: OK")
