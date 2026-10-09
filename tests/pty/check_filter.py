"""#24: filter the file list with /."""
from ptyh import *

folder = project({"README.typ": "= R\n", "chapters/one.typ": "= One\n", "chapters/two.typ": "= Two\n", "notes/draft.typ": "= D\n"})
try:
    steps = [
        (b"/", 0.3), (b"CHAP", 0.3),            # live: only the chapters
        (ENTER, 0.3),                            # keep the filter
        (ENTER, 0.6),                            # open the first match
        (ESC, 0.4),                              # back to the list, the filter stays
        (b"/", 0.3), (b"zzz", 0.3),              # nothing matches
        (ESC, 0.3),                              # the prompt: remove the filter
        (b"/", 0.2), (b"two", 0.2), (ENTER, 0.2), (ESC, 0.3),   # a filter, then Esc in the list removes it
    ]
    shots, _ = session(folder, steps)
    typing, kept, opened, back, nomatch, cleared, removed = shots[1], shots[2], shots[3], shots[4], shots[6], shots[7], shots[11]
    assert "Filter: CHAP" in typing and "chapters/one.typ" in typing and "README.typ" not in typing, f"the list did not follow the text:\n{typing}"
    assert "lazytypst /CHAP" in kept and "Filter:" not in kept, f"the filter was not kept:\n{kept}"
    assert "= One" in opened, f"Enter did not open the first match:\n{opened}"
    assert "lazytypst /CHAP" in back and "notes/draft.typ" not in back, f"the filter is gone after the editor:\n{back}"
    assert "No match" in nomatch, f"no 'No match' message:\n{nomatch}"
    assert "README.typ" in cleared and "notes/draft.typ" in cleared and "lazytypst /" not in cleared, f"Esc did not remove the filter:\n{cleared}"
    assert "README.typ" in removed and "lazytypst /" not in removed, f"Esc in the list did not remove the filter:\n{removed}"
finally:
    remove(folder)
print("filter check: OK")
