"""#17 to #20: the compile pane: time, counts in the title, colors, and the count of hidden rows."""
from ptyh import *

FONTS = ", ".join(f'"NoFont{n}"' for n in range(1, 7))
folder = project({
    "a-bad.typ": "= A\n#nope1()\n",
    "b-ok.typ": "= Fine\n",
    "c-warn.typ": '#set text(font: "NoSuchFontAtAll")\nHello\n',
    "d-many.typ": f"#set text(font: ({FONTS}))\nHello\n",
})
try:
    steps = [
        (ENTER, 0.5), (None, "error"), (ESC, 0.4),                        # a-bad.typ
        (b"j", 0.2), (ENTER, 0.5), (None, 1.5), (ESC, 0.4),           # b-ok.typ
        (b"j", 0.2), (ENTER, 0.5), (None, "Compile: 1 warning"), (ESC, 0.4),       # c-warn.typ
        (b"j", 0.2), (ENTER, 0.5), (None, "Compile: 6 warnings"), (ESC, 0.4),      # d-many.typ
    ]
    shots, _ = session(folder, steps)
    bad, ok, warn, many = shots[1], shots[5], shots[9], shots[13]
    assert "Compile: 1 error (" in bad and " ms)" in bad, f"no error count and time in the title:\n{bad}"
    assert "a-bad.typ:2:1: error: unknown variable: nope1" in bad, bad
    assert "OK in " in ok and " ms" in ok and "Compile───" in ok.replace(" ", "─") or "┌Compile──" in ok, f"no time or a wrong title:\n{ok}"
    assert "Compile: 1 warning" in warn and "OK in" in warn, f"no warning count:\n{warn}"
    assert "Compile: 6 warnings" in many, f"no count of 6 warnings:\n{many}"
    # The OK line and 6 warnings. Each warning wraps into 2 rows at 48 columns: 13 rows. 3 are shown.
    assert "+10 more" in many, f"no count of hidden rows (13 rows, 3 shown):\n{many}"
finally:
    remove(folder)
print("pane check: OK")
