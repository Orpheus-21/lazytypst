#!/usr/bin/env python3
"""Writes the lists of keys of the man page and of the website from `lazytypst --keys`.

The list of keys is in src/help.rs. The man page and the website repeat it, and tests fail if they differ.
Run this script after a change of the list, then run `cargo test`:

    scripts/sync-keys.py [path to the lazytypst program]
"""
import html
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PROGRAM = sys.argv[1] if len(sys.argv) > 1 else str(ROOT / "target" / "debug" / "lazytypst")
TITLES = {"list": "Keys in the file list", "editor": "Keys in the editor", "text": "Editing keys in the editor"}

keys = {}
for line in subprocess.run([PROGRAM, "--keys"], capture_output=True, text=True, check=True).stdout.splitlines():
    part, name, text = line.split("\t")
    keys.setdefault(part, []).append((name, text))


def roff(text):
    text = text.replace("\\", "\\e").replace("-", "\\-")
    return "\\&" + text if text.startswith((".", "'")) else text


man = [".SH KEYS"]
for part, title in TITLES.items():
    man.append(f".SS {roff(title)}")
    for name, text in keys[part]:
        man.append(f".TP\n.B {roff(name)}\n{roff(text)}")
man_text = "\n".join(man) + "\n"
path = ROOT / "docs" / "lazytypst.1"
page = path.read_text()
page = re.sub(r"\.SH KEYS\n.*?(?=\.SH ENVIRONMENT)", lambda _: man_text, page, flags=re.S)
path.write_text(page)

tables = ""
for part, title in TITLES.items():
    tables += f"<table>\n<caption>{html.escape(title)}</caption>\n<tbody>\n"
    for name, text in keys[part]:
        tables += f'<tr><th scope="row">{html.escape(name, quote=False)}</th><td>{html.escape(text, quote=False)}</td></tr>\n'
    tables += "</tbody>\n</table>\n"
path = ROOT / "site" / "index.html"
site = path.read_text()
site = re.sub(r'(<div class="wide">\n).*?(  </div>)', lambda m: m.group(1) + tables + "\n" + m.group(2), site, count=1, flags=re.S)
path.write_text(site)
print("man page and website: keys written")
