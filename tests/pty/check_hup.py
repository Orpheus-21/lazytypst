"""Closing the terminal (SIGHUP) or a SIGTERM with unsaved text saves the text before the program ends."""
import fcntl
import os
import pty
import select
import shutil
import struct
import tempfile
import termios
import time
from ptyh import *

def scenario(how):
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

        def pump(seconds):
            end = time.time() + seconds
            while time.time() < end:
                if select.select([fd], [], [], 0.02)[0]:
                    try:
                        data = os.read(fd, 65536)
                    except OSError:
                        return  # the program ended and closed the terminal
                    if b"\x1b[5n" in data:
                        os.write(fd, b"\x1b[?62;c\x1b[6;16;8t\x1b[0n")
                    if data.count(b"\x1b[6n"):
                        os.write(fd, b"\x1b[1;1R" * data.count(b"\x1b[6n"))

        pump(1.0)
        os.write(fd, b"LOST ")      # an edit that the autosave has not written yet
        pump(0.05)
        if how == "hup":
            os.close(fd)            # the terminal window closes: the program gets SIGHUP
        else:
            os.kill(pid, 15)        # the system asks the program to stop
        end = time.time() + 10
        while time.time() < end:
            done, _ = os.waitpid(pid, os.WNOHANG)
            if done:
                break
            if how == "term":
                pump(0.05)
            else:
                time.sleep(0.05)
        else:
            os.kill(pid, 9)
            raise AssertionError(f"the program did not end after {how}")
        text = read(folder, "doc.typ")
        assert text.startswith("LOST good"), (how, repr(text))
    finally:
        remove(folder)
        shutil.rmtree(state, ignore_errors=True)


scenario("hup")
scenario("term")
print("hup check: OK")
