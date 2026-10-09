"""Alt-I saves the clipboard image in the project and puts an #image line at the cursor (a fake wl-paste)."""
import os
from ptyh import *

FAKE = """#!/bin/sh
case "$1" in
  --list-types) printf 'text/plain\\nimage/png\\n';;
  *) printf '\\211PNG fake image bytes';;
esac
"""

folder = project({"doc.typ": "Figure: \n"})
tools = os.path.join(folder, "tools")
os.makedirs(tools)
try:
    program = os.path.join(tools, "wl-paste")
    with open(program, "w") as f:
        f.write(FAKE)
    os.chmod(program, 0o755)
    env = {"PATH": tools + os.pathsep + os.environ["PATH"], "WAYLAND_DISPLAY": "wayland-fake"}
    steps = [
        (None, 1.0),
        (b"\x1b[F", 0.2),                  # End: after the text
        (b"\x1bi", "Saved images/pasted-"),  # Alt-I
        (ESC, 0.3),
    ]
    shots, _ = session(folder, steps, env_extra=env)
    assert '#image("/images/pasted-' in shots[2], shots[2]
    images = os.listdir(os.path.join(folder, "images"))
    assert len(images) == 1 and images[0].startswith("pasted-") and images[0].endswith(".png"), images
    assert open(os.path.join(folder, "images", images[0]), "rb").read() == b"\x89PNG fake image bytes"
    assert read(folder, "doc.typ").startswith('Figure: #image("/images/pasted-'), read(folder, "doc.typ")
finally:
    remove(folder)
print("paste image check: OK")
