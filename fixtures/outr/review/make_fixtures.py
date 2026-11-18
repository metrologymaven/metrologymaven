"""Writes the hand-written fixtures of spec node OUTR.BRAND.REVIEW (CC-BY-4.0).

Every word here is project-authored: handles (alice, bob, mallory), releases, questions and notes
are made up for the tests. No fixture contains any third party's trade dress, text or images.
Expected results are hand-labelled from the spec's rules in EXPECTED below and asserted by
crates/fid-outreach/tests/review_gate.rs; nothing here was produced by running the Rust code.

Every fixture is run as in AC-1, from inside the fixture directory:
  fid-outreach review-gate --record records/site-2027.01.toml --author alice \
      --roster roster.txt --checklist checklist.toml

Run from the repository root: python fixtures/outr/review/make_fixtures.py
"""

import os

HERE = os.path.dirname(os.path.abspath(__file__))

ITEMS = [
    ("colours", "Is the palette our own, not one a reader would link to another vendor?"),
    ("icon_style", "Are the icons drawn in our own style rather than mimicking a vendor's set?"),
    ("layout", "Is the page arrangement our own, not a copy of a vendor's screens or site?"),
    ("screenshots", "Do the screenshots show only our own software?"),
    ("typography", "Are the typefaces and their pairing our own choice?"),
    ("copied_text", "Is every sentence written by us, with nothing lifted from other vendors?"),
]


def checklist():
    out = ["# Trade-dress checklist (synthetic fixture).", "version = 1", "items = ["]
    for i, q in ITEMS:
        out.append(f'  {{ id = "{i}", question = "{q}" }},')
    out.append("]")
    return "\n".join(out) + "\n"


ROSTER = "# Reviewer roster (synthetic fixture): one handle per line.\nalice\nbob\n"


def record(reviewer, results):
    lines = [
        "# Review record (synthetic fixture).",
        'release = "site-2027.01"',
        f'reviewer = "{reviewer}"',
        'date = "2027-01-20"',
        "checklist_version = 1",
        "",
        "[answers]",
    ]
    for i, _ in ITEMS:
        if i not in results:
            continue
        note = "Checked every page." if results[i] == "pass" else "Card grid follows another vendor's start page."
        lines.append(f'{i} = {{ result = "{results[i]}", note = "{note}" }}')
    return "\n".join(lines) + "\n"


ALL_PASS = {i: "pass" for i, _ in ITEMS}

FIXTURES = {
    "ok": record("bob", ALL_PASS),
    "missing-item": record("bob", {i: r for i, r in ALL_PASS.items() if i != "screenshots"}),
    "fail-item": record("bob", {**ALL_PASS, "layout": "fail"}),
    "self": record("alice", ALL_PASS),
    "not-on-roster": record("mallory", ALL_PASS),
}

# Hand-labelled expectations: (exit code, finding lines in text mode). Author is always alice,
# release is always site-2027.01.
EXPECTED = {
    "ok": (0, []),
    "missing-item": (1, ["site-2027.01 missing_answer:screenshots"]),
    "fail-item": (1, ["site-2027.01 failed_item:layout"]),
    "self": (1, ["site-2027.01 self_review"]),
    "not-on-roster": (1, ["site-2027.01 unknown_reviewer"]),
    "missing record": (2, []),
}


def write(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)


def main():
    for name, rec in FIXTURES.items():
        d = os.path.join(HERE, name)
        write(os.path.join(d, "checklist.toml"), checklist())
        write(os.path.join(d, "roster.txt"), ROSTER)
        write(os.path.join(d, "records", "site-2027.01.toml"), rec)


if __name__ == "__main__":
    main()
