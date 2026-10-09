"""y in the list and Ctrl-C in the editor send OSC 52 to the terminal."""
import base64, re
import ptyh
from ptyh import *

folder = project({"a.typ": "hello world\n", "b.typ": "x\n"})
try:
    session(folder, [(b"y", 0.4)])
    sent = re.findall(rb"\x1b\]52;c;([A-Za-z0-9+/=]*)\x07", ptyh.last_raw)
    assert [base64.b64decode(s).decode() for s in sent] == [folder + "/a.typ"], sent
    session(folder, [(ENTER, 0.4), (b"\x1b[1;2C" * 5, 0.2), (ctrl("c"), 0.4), (ESC, 0.4)])
    sent = re.findall(rb"\x1b\]52;c;([A-Za-z0-9+/=]*)\x07", ptyh.last_raw)
    assert [base64.b64decode(s).decode() for s in sent] == ["hello"], sent
finally:
    remove(folder)
print("clipboard check: OK")
