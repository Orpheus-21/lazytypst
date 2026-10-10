"""A panic puts the edits that the file lacks in the local history, and says so. (A debug build has the key Ctrl-Alt-P.)"""
import fcntl
import glob
import os
import pty
import select
import shutil
import struct
import tempfile
import termios
import time
from ptyh import *

folder = project({"doc.typ": "good\n"})
state = tempfile.mkdtemp(prefix="lazytypst-state-")
try:
    env = {k: v for k, v in os.environ.items() if not k.startswith(("KITTY", "GHOSTTY", "WEZTERM", "KONSOLE", "TMUX", "ITERM"))}
    env.update({"TERM": "xterm-256color", "XDG_STATE_HOME": state})
    env.pop("LAZYTYPST_WATCH", None)
    pid, fd = pty.fork()
    if pid == 0:
        fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 100, 0, 0))
        os.execve(BINARY, [BINARY, folder], env)
    seen = bytearray()

    def pump(seconds):
        end = time.time() + seconds
        while time.time() < end:
            if select.select([fd], [], [], 0.02)[0]:
                try:
                    data = os.read(fd, 65536)
                except OSError:
                    return
                seen.extend(data)
                if b"\x1b[5n" in data:
                    os.write(fd, b"\x1b[?62;c\x1b[6;16;8t\x1b[0n")
                if data.count(b"\x1b[6n"):
                    os.write(fd, b"\x1b[1;1R" * data.count(b"\x1b[6n"))

    pump(1.0)
    os.write(fd, b"SAVEME ")        # an edit that the autosave has not written yet
    pump(0.05)
    os.write(fd, b"\x1b\x10")       # Ctrl-Alt-P: the debug key that panics
    pump(1.5)
    _, status = os.waitpid(pid, 0)
    assert os.WIFEXITED(status) and os.WEXITSTATUS(status) == 101, status
    text = seen.decode("utf-8", "replace")
    assert "lazytypst crashed" in text and "Your edits that the file lacks are in" in text, text[-600:]
    assert read(folder, "doc.typ") == "good\n", "the panic must not write the file"
    versions = glob.glob(os.path.join(state, "lazytypst", "history", "*", "*.txt"))
    contents = [open(path).read() for path in versions]
    assert "SAVEME good\n" in contents, contents
finally:
    remove(folder)
    shutil.rmtree(state, ignore_errors=True)
print("panic check: OK")
