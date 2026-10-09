"""--doctor names the image protocol, and fails with exit code 1 when typst is missing."""
import subprocess
from ptyh import *

folder = project({"a.typ": ""})
try:
    # The harness plays a kitty terminal and ends the session with a q, so this program ends on its own.
    import os, pty, select, time
    def run(env_extra, kitty):
        env = {k: v for k, v in os.environ.items() if not k.startswith(("KITTY", "GHOSTTY", "WEZTERM", "KONSOLE", "TMUX", "ITERM"))}
        env["TERM"] = "xterm-256color"
        env.update(env_extra)
        pid, fd = pty.fork()
        if pid == 0:
            os.execve(BINARY, [BINARY, "--doctor"], env)
        out = b""
        end = time.time() + 5
        while time.time() < end:
            if select.select([fd], [], [], 0.05)[0]:
                try:
                    data = os.read(fd, 65536)
                except OSError:
                    break
                if not data:
                    break
                out += data
                if b"\x1b[5n" in out and not getattr(run, "done", False):
                    run.done = True
                    os.write(fd, (b"\x1b_Gi=31;OK\x1b\\\x1b[?62;c\x1b[6;16;8t" if kitty else b"") + STATUS)
        _, status = os.waitpid(pid, 0)
        run.done = False
        return out.decode(errors="replace"), os.WEXITSTATUS(status)
    text, code = run({}, kitty=True)
    assert "kitty graphics" in text and code == 0, (text, code)
    text, code = run({"PATH": "/nonexistent"}, kitty=False)
    assert "missing" in text and code == 1, (text, code)
    assert "half blocks" in text, text
finally:
    remove(folder)
print("doctor check: OK")
