"""Writes the hand-written fixtures of spec node OUTR.BRAND.GUIDE.NAME (CC-BY-4.0).

Every word here is project-authored. "Examplo" is a made-up name; "Fiducial" and "fid" are the
project's former working names. No fixture contains any other organisation's name or mark.
Expected results are hand-labelled from the spec's rules in EXPECTED below and asserted by
crates/fid-outreach/tests/guide_check.rs; nothing here was produced by running the Rust code.

Layout of each case: <case>/name.toml and <case>/guidelines/*.md.

Run from the repository root: python fixtures/outr/brand/guide/make_fixtures.py
"""

import os

HERE = os.path.dirname(os.path.abspath(__file__))

NAME_UNACCEPTED = """# Synthetic fixture: the name is not accepted yet.
accepted = false
working_names = ["Fiducial", "fid"]
"""

NAME_ACCEPTED = """# Synthetic fixture: the made-up name Examplo is accepted.
accepted = true
working_names = ["Fiducial", "fid"]
name = "Examplo"
decision = "D-002"
date = "2026-11-17"
"""

# Clean guideline text: the family name only as the token; the working names appear only in
# code spans, in a fenced block, in lower case or inside longer words, none of which count.
CLEAN = """# {{FAMILY_NAME}} usage

Refer to {{FAMILY_NAME}} Inspect by its full name.
The command line tool is `fid`, as in `fid-outreach guide-check`.
A fiducial is a reference mark; Fiducials and fiducially are other words.

```sh
fid inspect --plan Fiducial.toml
```
"""

SECOND = """# Apps

- {{FAMILY_NAME}} Probe
- {{FAMILY_NAME}} Report
"""

# Line 3 holds the working name as a whole word outside code.
LITERAL = """# {{FAMILY_NAME}} usage

Our old name was Fiducial, and it must not appear here.
The tool `Fiducial` in code is fine.
"""

STRAY = """# {{FAMILY_NAME}} usage

{{FAMILY_NAME}} Inspect: {{TAGLINE}}
"""

CASES = {
    "clean": (NAME_UNACCEPTED, {"usage.md": CLEAN, "sub/apps.md": SECOND}),
    "literal": (NAME_UNACCEPTED, {"usage.md": LITERAL}),
    "unaccepted": (NAME_UNACCEPTED, {"usage.md": CLEAN}),
    "accepted": (NAME_ACCEPTED, {"usage.md": CLEAN, "sub/apps.md": SECOND}),
    "stray-token": (NAME_ACCEPTED, {"usage.md": STRAY}),
}

# (case, mode) -> (exit code, sorted finding lines without the summary). Hand-labelled.
EXPECTED = {
    ("clean", "check"): (0, []),
    ("literal", "check"): (1, ["literal_name usage.md:3 Fiducial"]),
    ("unaccepted", "publish"): (1, ["name_not_accepted <name file>"]),
    ("accepted", "publish"): (0, []),
    ("stray-token", "publish"): (1, ["token_left usage.md:3 {{TAGLINE}}"]),
}


def write(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)


def main():
    for case, (name_toml, files) in CASES.items():
        write(os.path.join(HERE, case, "name.toml"), name_toml)
        for rel, text in files.items():
            write(os.path.join(HERE, case, "guidelines", *rel.split("/")), text)


if __name__ == "__main__":
    main()
