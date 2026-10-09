"""NO_COLOR removes the text colors. Without it, an error is red."""
import re
import ptyh
from ptyh import *

COLOR = re.compile(rb"\x1b\[(?:[0-9;]*;)?(?:3[0-7]|4[0-7]|9[0-7]|10[0-7]|38|48)[;m]")
folder = project({"a.typ": "#nope()\n", "zz.typ": ""})
try:
    steps = [(ENTER, 0.4), (ctrl("b"), "unknown variable"), (ESC, 0.4)]
    session(folder, steps, env_extra={"NO_COLOR": "1"})
    plain = ptyh.last_raw
    session(folder, steps, env_extra={"NO_COLOR": ""})
    colored = ptyh.last_raw
    assert not COLOR.search(plain), COLOR.search(plain)
    assert re.search(rb"\x1b\[(?:[0-9;]*;)?(?:31|38;5;1)[;m]", colored), "no red without NO_COLOR"
finally:
    remove(folder)
print("no color check: OK")
