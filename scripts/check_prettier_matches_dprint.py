#!/usr/bin/python3
"""
Confirm that .prettierrc settings match dprint.
Editors often use prettier, our `just format` command uses dprint.
"""

import json
import os
from difflib import SequenceMatcher
from pprint import pprint
from subprocess import check_output

script_dir = os.path.dirname(os.path.abspath(__file__))


def strip_comments(s: str) -> str:
    return "\n".join(line for line in s.splitlines() if not is_comment(line))


def is_comment(line: str) -> bool:
    trimmed = line.strip()
    return trimmed.startswith("#") or trimmed.startswith("//")


def js_to_json(filename: str) -> str:
    return check_output(
        ["node"],
        input=f"console.log(JSON.stringify(require({filename!r})));",
        encoding="utf8",
        cwd=script_dir,
    )


dprint = json.loads(
    strip_comments(open(os.path.join(script_dir, "../dprint.json")).read())
)
prettierrc = json.loads(js_to_json("../.prettierrc.js"))
prettierignore = strip_comments(
    open(os.path.join(script_dir, "../.prettierignore")).read()
).splitlines()

if dprint["prettier"] != prettierrc:
    pprint(dprint["prettier"])
    pprint(prettierrc)
    raise Exception(".prettierrc and dprint.json settings don't match!")

if dprint["excludes"] != prettierignore:
    for tag, i, j, k, l in SequenceMatcher(
        None, dprint["excludes"], prettierignore
    ).get_opcodes():
        if tag == "equal":
            print("both have", dprint["excludes"][i:j])
        if tag in ("delete", "replace"):
            print("  dprint excludes has", dprint["excludes"][i:j])
        if tag in ("insert", "replace"):
            print("  .prettierrc has", prettierignore[k:l])
    raise Exception(".prettierignore and dprint.json settings don't match!")
