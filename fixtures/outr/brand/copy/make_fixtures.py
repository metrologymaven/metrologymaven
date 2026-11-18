"""Writes the hand-written fixtures of spec node OUTR.BRAND.COPY (CC-BY-4.0).

Every word here is project-authored: handles, releases, questions and notes are made up for
the tests. No fixture contains text from any third party's material (REQ-OUTR.BRAND.COPY-005).
Expected results are hand-labelled from the spec's rules in EXPECTED below and asserted by
crates/fid-outreach/tests/copy_guard.rs; nothing here was produced by running the Rust code.

Run from the repository root: python fixtures/outr/brand/copy/make_fixtures.py
"""

import os

HERE = os.path.dirname(os.path.abspath(__file__))

POLICY_OK = """# Copied-text policy (synthetic fixture). D-013 option C: review only.
mode = "review_only"
decision = "D-013"
"""

CHECKLIST_ITEMS = [
    ("colours", "Does the palette stay clearly our own?"),
    ("icon_style", "Are the icons drawn in our own style?"),
    ("layout", "Is the page layout our own arrangement?"),
    ("screenshots", "Do screenshots show only our own software?"),
    ("typography", "Are the typefaces and their pairing our own choice?"),
    ("copied_text", "Is every sentence written by us, with nothing lifted from other vendors?"),
]


def checklist(items):
    out = ["# Trade-dress checklist (synthetic fixture).", "version = 1", "items = ["]
    for i, q in items:
        out.append(f'  {{ id = "{i}", question = "{q}" }},')
    out.append("]")
    return "\n".join(out) + "\n"


def record(release, reviewer, date, copied="pass", note="Read every page; all wording is ours."):
    lines = [
        "# Review record (synthetic fixture).",
        f'release = "{release}"',
        f'reviewer = "{reviewer}"',
        f'date = "{date}"',
        "",
        "[answers]",
    ]
    for i, _ in CHECKLIST_ITEMS:
        if i == "copied_text":
            lines.append(f'copied_text = {{ result = "{copied}", note = "{note}" }}')
        else:
            lines.append(f'{i} = {{ result = "pass", note = "" }}')
    return "\n".join(lines) + "\n"


TWO_PASS = {
    "records/site-2027.01.toml": record("site-2027.01", "quill", "2027-01-20"),
    "records/site-2027.02.toml": record("site-2027.02", "quill", "2027-02-18"),
}

FOUND_NOTE = "The pricing blurb repeated a sentence from another vendor's flyer; rewritten."

FIXTURES = {
    "ok": {"copy-policy.toml": POLICY_OK, "checklist.toml": checklist(CHECKLIST_ITEMS), **TWO_PASS},
    "no-item": {
        "copy-policy.toml": POLICY_OK,
        "checklist.toml": checklist([c for c in CHECKLIST_ITEMS if c[0] != "copied_text"]),
        **TWO_PASS,
    },
    "found-copy": {
        "copy-policy.toml": POLICY_OK,
        "copy-policy-revisited.toml": POLICY_OK
        + """
[[revisit]]
release = "site-2027.03"
date = "2027-03-25"
note = "Copied sentence found in review; counsel review of option B scheduled per D-013."
""",
        "checklist.toml": checklist(CHECKLIST_ITEMS),
        "records/site-2027.02.toml": record("site-2027.02", "quill", "2027-02-18"),
        "records/site-2027.03.toml": record("site-2027.03", "quill", "2027-03-19", "fail", FOUND_NOTE),
    },
    "mode-changed": {
        "copy-policy.toml": """# Mode moved without a counsel review (synthetic fixture).
mode = "hashed_shingles"
decision = "D-013"
""",
        "copy-policy-reviewed.toml": """# Mode moved after a counsel review (synthetic fixture).
mode = "hashed_shingles"
decision = "D-013"
counsel_review = { date = "2027-05-02", reviewer_ref = "counsel-memo-0001" }
""",
        "checklist.toml": checklist(CHECKLIST_ITEMS),
        **TWO_PASS,
    },
    "refset": {
        "copy-policy.toml": POLICY_OK,
        "checklist.toml": checklist(CHECKLIST_ITEMS),
        **TWO_PASS,
        # A declaration only: the name marks it as a reference set; it holds no passages.
        "brand/incumbents.refset.json": '{"note":"synthetic fixture; empty on purpose","passages":[]}\n',
    },
    "kind-key": {
        "copy-policy.toml": POLICY_OK,
        "checklist.toml": checklist(CHECKLIST_ITEMS),
        **TWO_PASS,
        "brand/samples.toml": '# Declares itself a reference set; holds no passages.\nkind = "incumbent_text"\npassages = []\n',
        # Mentions the kind below the top level only: not a declaration.
        "brand/nested.json": '{"meta":{"kind":"incumbent_text"}}\n',
    },
}

# Hand-labelled expectations: (exit code, finding lines in text mode).
EXPECTED = {
    "ok": (0, []),
    "no-item": (1, ["copied_text_item_missing"]),
    "found-copy": (1, ["d013_revisit_due site-2027.03"]),
    "found-copy + copy-policy-revisited.toml": (0, []),
    "mode-changed": (1, ["counsel_review_missing"]),
    "mode-changed + copy-policy-reviewed.toml": (0, []),
    "refset": (1, ["reference_set_present brand/incumbents.refset.json"]),
    "kind-key": (1, ["reference_set_present brand/samples.toml"]),
    "missing policy": (2, []),
}


def main():
    for name, files in FIXTURES.items():
        for rel, text in files.items():
            path = os.path.join(HERE, name, *rel.split("/"))
            os.makedirs(os.path.dirname(path), exist_ok=True)
            with open(path, "w", encoding="utf-8", newline="\n") as f:
                f.write(text)


if __name__ == "__main__":
    main()
