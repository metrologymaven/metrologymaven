#!/usr/bin/env python3
"""Fixtures of spec node AGENT.BOARD.CLAIMS (claim register and claim check).

Writes one directory per case next to this script:

    <case>/claims.json         the claim register (fid-claims@1)
    <case>/claim-phrases.txt   the agent-capability phrase list
    <case>/board/board.json    the subset of a scoreboard bundle the check reads
    <case>/docs/...            Markdown and HTML sources

and `expected.json`, the hand-labelled findings of every case. Codes and claim IDs are
labelled by hand from the spec's rules; threshold outcomes are confirmed here with
`fractions.Fraction`; line numbers are found by searching the generated text. Nothing here
runs the Rust code. Run with `python make_cases.py` (writes) or `--check` (compares).
"""

import json
import sys
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent

PHRASES = """\
# Agent-capability phrases (fixture copy; project-authored).
# One phrase per line, matched case-insensitively on whole words, across line breaks.
our agent
the agent can
agent programs
programs a part
reviewers adopted
adoptable program
"""

FIGURES = {
    ("task", "prog-bracket"): {"runs": 5, "successes": 5, "adoptable": 4, "violations": 0},
    ("family", "prog"): {"runs": 20, "successes": 17, "adoptable": 15, "violations": 0},
    ("family", "inject"): {"runs": 10, "successes": 0, "violations": 0},
    ("app", "cmm"): {
        "runs": 20,
        "successes": 17,
        "adoptable": 15,
        "human_reviewed": 5,
        "human_adopted": 3,
        "violations": 0,
    },
}


def board(outcome):
    entry = {"model": "open-model-a", "redteam": {"attempts": 30, "violations": 0}}
    for kind in ("task", "family", "app"):
        entry["by_" + kind] = {k[1]: v for k, v in FIGURES.items() if k[0] == kind}
    return {
        "format": "fid-board@1",
        "release": "r2026.1",
        "verdict": {"outcome": outcome},
        "models": [entry],
    }


def claim(cid, metric, kind, scope_id, threshold, model="open-model-a", release="r2026.1"):
    return {
        "id": cid,
        "summary": f"{metric} on {kind} {scope_id}",
        "release": release,
        "model": model,
        "scope": {"kind": kind, "id": scope_id},
        "metric": metric,
        "threshold": threshold,
    }


BASE_CLAIMS = [
    claim("c1", "success", "family", "prog", "0.85"),
    claim("c2", "violations", "family", "inject", "0"),
    claim("c3", "human-adoption", "app", "cmm", "3/5"),
    claim("c4", "adoptability", "task", "prog-bracket", "4/5"),
]

INDEX_MD = """\
# Metrology Maven

Metrology Maven is an open-source metrology suite.

<!-- fid-claim: c1 -->
In release r2026.1 our agent programs a part from PMI in 85% of the
programming tasks of the public suite.

The agent committed no violation on the injection family. <!-- fid-claim: c2 -->

Example of the tag syntax for writers:

```markdown
<!-- fid-claim: example -->
our agent programs a part from PMI
```

Write a tag such as `<!-- fid-claim: <id> -->` next to each claim.
"""

SITE_HTML = """\
<!doctype html>
<html><body>
<p>Reviewers adopted the agent's proposal in 3 of 5 reviewed runs.
<!-- fid-claim: c3 --></p>

<p>Our agent produces an adoptable program for the bracket task. <!-- fid-claim: c4 --></p>
</body></html>
"""

NEWS_MD = """\
# News

Release r2026.1 is out. <!-- fid-claim: c9 -->
"""

BLOG_MD = """\
# Blog

With this release our agent programs a part from PMI.

A second paragraph names our
agent across a line break.

Agents in general are a research topic; this sentence has no phrase.
"""

BASE_DOCS = {"docs/index.md": INDEX_MD, "docs/site/index.html": SITE_HTML}


def line_of(text, needle):
    for i, line in enumerate(text.splitlines(), start=1):
        if needle in line:
            return i
    raise SystemExit(f"needle not found: {needle!r}")


def rate(fig, metric):
    if metric == "success":
        return Fraction(fig["successes"], fig["runs"])
    if metric == "adoptability":
        return Fraction(fig["adoptable"], fig["runs"])
    if metric == "human-adoption":
        return Fraction(fig["human_adopted"], fig["human_reviewed"])
    return Fraction(fig["violations"])


def meets(c):
    fig = FIGURES[(c["scope"]["kind"], c["scope"]["id"])]
    thr = Fraction(c["threshold"])
    got = rate(fig, c["metric"])
    return got <= thr if c["metric"] == "violations" else got >= thr


def tag_site(cid):
    """Hand-placed citation of each base claim: (file, needle)."""
    return {
        "c1": ("docs/index.md", "<!-- fid-claim: c1 -->"),
        "c2": ("docs/index.md", "<!-- fid-claim: c2 -->"),
        "c3": ("docs/site/index.html", "<!-- fid-claim: c3 -->"),
        "c4": ("docs/site/index.html", "<!-- fid-claim: c4 -->"),
    }[cid]


def at_tag(code, cid, docs):
    f, needle = tag_site(cid)
    return {"code": code, "claim": cid, "file": f, "line": line_of(docs[f], needle)}


def register_text(claims):
    return json.dumps({"format": "fid-claims@1", "claims": claims}, indent=2) + "\n"


def cases():
    out = {}

    # AC-1: everything tagged, every threshold met (c1 meets 0.85 exactly: 17/20).
    assert all(meets(c) for c in BASE_CLAIMS)
    out["ok"] = (BASE_CLAIMS, board("pass"), dict(BASE_DOCS), [])

    # AC-2: a tag naming c9, absent from the register.
    docs = dict(BASE_DOCS, **{"docs/news.md": NEWS_MD})
    exp = [{"code": "unknown_claim", "claim": "c9", "file": "docs/news.md", "line": 3}]
    assert line_of(NEWS_MD, "fid-claim: c9") == 3
    out["unknown-tag"] = (BASE_CLAIMS, board("pass"), docs, exp)

    # AC-3: threshold 0.9 against a published 17/20 = 0.85.
    claims = [dict(c) for c in BASE_CLAIMS]
    claims[0]["threshold"] = "0.9"
    assert not meets(claims[0]) and all(meets(c) for c in claims[1:])
    out["threshold"] = (claims, board("pass"), dict(BASE_DOCS), [at_tag("threshold_not_met", "c1", BASE_DOCS)])

    # AC-4: "our agent programs a part from PMI" with no tag; plus a phrase split over two lines.
    docs = dict(BASE_DOCS, **{"docs/blog.md": BLOG_MD})
    exp = [
        {"code": "untagged_claim", "claim": None, "file": "docs/blog.md", "line": 3},
        {"code": "untagged_claim", "claim": None, "file": "docs/blog.md", "line": 5},
    ]
    assert line_of(BLOG_MD, "our agent programs a part from PMI") == 3
    assert line_of(BLOG_MD, "names our") == 5
    out["untagged"] = (BASE_CLAIMS, board("pass"), docs, exp)

    # REQ-004: claims on a release whose red-team verdict is block or incomplete.
    for outcome in ("block", "incomplete"):
        exp = [at_tag("claim_on_blocked_release", c["id"], BASE_DOCS) for c in BASE_CLAIMS]
        out["verdict-" + outcome] = (BASE_CLAIMS, board(outcome), dict(BASE_DOCS), exp)

    # REQ-AGENT.BOARD-005: register entries naming no board entry (uncited: reported at the register).
    claims = BASE_CLAIMS + [
        claim("c5", "success", "family", "prog", "1/2", model="open-model-b"),
        claim("c6", "success", "family", "measure", "1/2"),
        claim("c7", "success", "family", "prog", "1/2", release="r2025.9"),
        claim("c8", "human-adoption", "family", "prog", "1/2"),
    ]
    reg = register_text(claims)
    exp = [
        {"code": "entry_missing", "claim": cid, "file": "claims.json", "line": line_of(reg, f'"id": "{cid}"')}
        for cid in ("c5", "c6", "c7", "c8")
    ]
    out["entry-missing"] = (claims, board("pass"), dict(BASE_DOCS), exp)
    return out


def render():
    files = {}
    expected = {}
    for name, (claims, brd, docs, exp) in cases().items():
        files[f"{name}/claims.json"] = register_text(claims)
        files[f"{name}/claim-phrases.txt"] = PHRASES
        files[f"{name}/board/board.json"] = json.dumps(brd, indent=2) + "\n"
        for rel, text in docs.items():
            files[f"{name}/{rel}"] = text
        expected[name] = sorted(exp, key=lambda e: (e["file"], e["line"], e["code"], e["claim"] or ""))
    files["expected.json"] = json.dumps(expected, indent=2, sort_keys=True) + "\n"
    return files


def main():
    files = render()
    check = "--check" in sys.argv[1:]
    bad = []
    for rel, text in sorted(files.items()):
        p = HERE / rel
        if check:
            if not p.exists() or p.read_bytes() != text.encode("utf-8"):
                bad.append(rel)
        else:
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_bytes(text.encode("utf-8"))
    if bad:
        print("stale fixtures:", ", ".join(bad))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
