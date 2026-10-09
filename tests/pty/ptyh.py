"""A small pty harness for lazytypst. It plays the terminal: it answers the image query and reads the screen with pyte."""
import fcntl, os, pty, pyte, select, shutil, struct, tempfile, termios, time

ENTER, ESC = b"\r", b"\x1b"
STATUS = b"\x1b[0n"
last_titles = []  # the window title after each step of the last session
last_raw = b""    # all the bytes that the program wrote in the last session
# The program to test: the variable LAZYTYPST_BIN, or else the debug build of this repository.
BINARY = os.environ.get("LAZYTYPST_BIN") or os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "target", "debug", "lazytypst")


def ctrl(letter):
    return bytes([ord(letter) & 0x1F])


def session(folder, steps, binary=BINARY, cols=100, rows=24, args=(), env_extra=None, child_setup=None, kitty=False):
    """Starts the program and plays the steps. A step is (key, seconds to wait). A key is bytes, None, or a function.
    The wait can be a text instead of a number: the step then waits until that text is on the screen, for up to 10 seconds.
    A function takes the pid of the program, or no argument. The harness adds a last step that sends q. Returns (list of screen texts, one per step, the pid)."""
    env = {k: v for k, v in os.environ.items() if not k.startswith(("KITTY", "GHOSTTY", "WEZTERM", "KONSOLE", "TMUX", "ITERM"))}
    env["TERM"] = "xterm-256color"
    env.pop("TERM_PROGRAM", None)
    # Each session gets its own state folder, so no check touches the real one. The shell may already set the variable.
    own_state = None if "XDG_STATE_HOME" in (env_extra or {}) else tempfile.mkdtemp(prefix="lazytypst-state-")
    if own_state:
        env["XDG_STATE_HOME"] = own_state
    env.update(env_extra or {})
    pid, fd = pty.fork()
    if pid == 0:
        fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        if child_setup:
            child_setup(os.getpid())  # the program keeps this pid after the exec
        os.execve(binary, [binary, *args, folder], env)
    screen = pyte.Screen(cols, rows)
    stream = pyte.ByteStream(screen)
    raw = bytearray()
    answered = False

    def drain(seconds):
        nonlocal answered
        end = time.time() + seconds
        while time.time() < end:
            if select.select([fd], [], [], 0.02)[0]:
                try:
                    data = os.read(fd, 65536)
                except OSError:
                    return
                raw.extend(data)
                stream.feed(data)
                if not answered and b"\x1b[5n" in raw:
                    answered = True
                    os.write(fd, (b"\x1b_Gi=31;OK\x1b\\\x1b[?62;c\x1b[6;16;8t" if kitty else b"") + STATUS)
                if data.count(b"\x1b[6n"):
                    os.write(fd, b"\x1b[1;1R" * data.count(b"\x1b[6n"))  # a cursor position query

    drain(1.0)
    shots = []
    titles = []
    for key, wait in list(steps) + [(b"q", 0.5)]:
        if callable(key):
            key(pid) if key.__code__.co_argcount == 1 else key()
        elif key is not None:
            os.write(fd, key)
        if isinstance(wait, str):
            end = time.time() + 10
            while time.time() < end and wait not in "\n".join(screen.display):
                drain(0.05)
            drain(0.15)  # let the rest of the frame arrive
        else:
            drain(wait)
        shots.append("\n".join(screen.display))
        titles.append(screen.title)
    _, status = os.waitpid(pid, 0)
    global last_titles, last_raw
    drain(0.2)
    last_titles, last_raw = titles, bytes(raw)
    if own_state:
        shutil.rmtree(own_state, ignore_errors=True)
    assert os.WIFEXITED(status) and os.WEXITSTATUS(status) == 0, f"bad exit {status}"
    return shots, pid


def project(files):
    """Makes a temporary folder with the files {relative path: text}. Returns the folder."""
    folder = tempfile.mkdtemp()
    for rel, text in files.items():
        path = os.path.join(folder, rel)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w") as f:
            f.write(text)
    return folder


def read(folder, rel):
    with open(os.path.join(folder, rel)) as f:
        return f.read()


def remove(*folders):
    for folder in folders:
        shutil.rmtree(folder, ignore_errors=True)
