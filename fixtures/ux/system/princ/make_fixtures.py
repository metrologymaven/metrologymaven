#!/usr/bin/env python3
"""Writes the fixtures of spec node UX.SYSTEM.PRINC and checks their hand labels.

Synthetic data, CC-BY-4.0. Python stdlib only. Run from any directory:

    python fixtures/ux/system/princ/make_fixtures.py          # (re)write the fixtures
    python fixtures/ux/system/princ/make_fixtures.py --check  # fail if files differ or labels disagree

The expected results in `expected.json` are hand-labelled from the leaf's Requirements and
Design (written below as literals, not computed by the code under test). `--check` also runs a
small independent Python reading of the same rules over the fixtures, the shipped
`design/principles.md` and `design/reserved-words.json`, and fails if it disagrees with a label.
"""

import json
import pathlib
import re
import sys

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[3]

PRINCIPLES = ["P-DATA", "P-DENSE", "P-EXPLAIN", "P-FAMILIAR", "P-KEYS", "P-NOCOLOR"]

TITLES = {
    "P-DENSE": "Dense but calm",
    "P-DATA": "Data first",
    "P-EXPLAIN": "Every number explainable",
    "P-KEYS": "Keyboard-complete",
    "P-FAMILIAR": "Familiar workflows",
    "P-NOCOLOR": "Status never by colour alone",
}
ORDER = ["P-DENSE", "P-DATA", "P-EXPLAIN", "P-KEYS", "P-FAMILIAR", "P-NOCOLOR"]


def section(pid, do=True, dont=True, heading=None, do_body=None):
    h = heading if heading is not None else f"{pid}: {TITLES.get(pid, 'Extra')}"
    out = [f"## {h}", "", f"Synthetic rationale for {pid}.", ""]
    if do:
        out += ["### Do", ""]
        out += do_body if do_body is not None else [f"- A synthetic example that follows {pid}.", ""]
    if dont:
        out += ["### Don't", "", f"- A synthetic example that breaks {pid}.", ""]
    return out


def doc(sections):
    lines = ["# Design principles (test fixture)", "", "Synthetic, CC-BY-4.0.", ""]
    for s in sections:
        lines += s
    return "\n".join(lines).rstrip("\n") + "\n"


def no_dont():
    return doc([section(p, dont=(p != "P-DATA")) for p in ORDER])


def no_id():
    return doc([section(p, heading=("Dense but calm" if p == "P-DENSE" else None)) for p in ORDER])


def unknown():
    secs = []
    for p in ORDER:
        if p == "P-DATA":
            # A heading inside a fenced block is not a section; the block counts as Do content.
            secs.append(section(p, do_body=["```text", "## P-FAKE: not a heading", "```", ""]))
        elif p == "P-KEYS":
            # `### Do` present but empty: followed directly by `### Don't`.
            secs.append(section(p, do_body=[]))
        else:
            secs.append(section(p))
    secs.append(section("P-FAST", heading="P-FAST: Fast"))
    secs.append(section("P-NOCOLOR", heading="P-NOCOLOR: Defined twice"))
    return doc(secs)


def jsonl(rows):
    return "".join(json.dumps(r, ensure_ascii=False, separators=(",", ":")) + "\n" for r in rows)


COPY_OK = [
    {"id": "review.human.approve_button", "text": "Approve"},
    {"id": "run.approve.confirm", "text": "Approve this run? Your approval is recorded in the journal."},
    {"id": "program.release.title", "text": "Release program version {version}"},
    {"id": "program.release.sign.button", "text": "Sign and release"},
    {"id": "agent.proposal.apply_button", "text": "Apply proposal"},
    {"id": "agent.proposal.reject_button", "text": "Reject proposal"},
    {"id": "probe.signal.label", "text": "Signal strength"},
    {"id": "viz.feature.designation", "text": "Designated datum"},
    {"id": "viz.changes.label", "text": "Unreleased changes"},
    {"id": "gdt.result.status", "text": "Conforms to tolerance"},
    {"id": "trust.verify.result", "text": "Digest verified"},
    {"id": "cal.verify.title", "text": "Verify probe calibration"},
    {"id": "viz.shell.login", "text": "Log in"},
    {"id": "viz.update.banner", "text": "What's new in {version}"},
    {"id": "viz.empty.state", "text": ""},
]

COPY_BAD = [
    {"id": "agent.proposal.apply_button", "text": "Approve"},
    {"id": "agent.plan.status", "text": "Plan approved by agent"},
    {"id": "agent.proposal.bulk_button", "text": "Approve and release all"},
    {"id": "run.approved.count", "text": "Approved runs: {n}"},
    {"id": "viz.shell.login", "text": "Sign in"},
    {"id": "viz.update.banner", "text": "New release available"},
    {"id": "rpt.summary.note", "text": "All features VERIFIED"},
    {"id": "viz.inspect.badge", "text": "Conformance: 98 %"},
    {"id": "viz.inspect.title", "text": "Inspect features"},
    {"id": "program.sign.tooltip", "text": "Signed by the release manager; signature checked"},
]

COPY_SCHEMA = (
    '{"id":"a.b","text":"ok"}\n'
    "not json\n"
    '{"id":"a.c"}\n'
    '{"id":"a.b","text":"again"}\n'
    '{"id":"a.d","text":"x","note":"y"}\n'
    "[1]\n"
    '{"id":"a.e","text":"Approve","id":"z"}\n'
    '{"id":"agent.x","text":"Approved"}\n'
    "\n"
)

WORDS_BAD = (
    "[\n"
    '  {"word": "approve", "meaning": "Human approval.", "allowed_prefixes": ["review.human."], "case_insensitive": true, "inflections": ["approved"]},\n'
    '  {"word": "Approved", "meaning": "Clashes with an inflection of approve.", "allowed_prefixes": [], "case_insensitive": true},\n'
    '  {"word": "sign in", "meaning": "Two words.", "allowed_prefixes": [], "case_insensitive": true},\n'
    '  {"word": "release", "meaning": "", "allowed_prefixes": [], "case_insensitive": true},\n'
    '  {"word": "conforms", "meaning": "Prefixes not an array.", "allowed_prefixes": "gdt.", "case_insensitive": true},\n'
    '  {"word": "verified", "meaning": "Flag not a boolean.", "allowed_prefixes": [], "case_insensitive": "yes"},\n'
    '  {"word": "sign", "meaning": "Unknown key.", "allowed_prefixes": [], "case_insensitive": true, "colour": "red"},\n'
    '  {"word": "Cert", "meaning": "Case-sensitive test word.", "allowed_prefixes": ["trust."], "case_insensitive": false}\n'
    "]\n"
)

WORDS_BAD_COPY = [
    {"id": "agent.a", "text": "approved"},
    {"id": "agent.b", "text": "cert CERT"},
    {"id": "agent.c", "text": "Cert"},
    {"id": "trust.d", "text": "Cert"},
    {"id": "agent.e", "text": "sign in"},
]

WORDS_OBJECT = '{"format": "reserved-words@1", "words": []}\n'

FILES = {
    "no-dont.md": no_dont(),
    "no-id.md": no_id(),
    "unknown.md": unknown(),
    "copy-ok.jsonl": jsonl(COPY_OK),
    "copy-bad.jsonl": jsonl(COPY_BAD),
    "copy-schema.jsonl": COPY_SCHEMA,
    "words-bad.json": WORDS_BAD,
    "words-bad-copy.jsonl": jsonl(WORDS_BAD_COPY),
    "words-object.json": WORDS_OBJECT,
}

# Hand-labelled expectations: exit code and the sorted "CODE subject" of every finding.
EXPECTED = {
    "princ": {
        "shipped": {"exit": 0, "principles": PRINCIPLES, "findings": []},
        "no-dont.md": {"exit": 1, "principles": PRINCIPLES, "findings": ["PRINC-EXAMPLE-MISSING P-DATA"]},
        "no-id.md": {
            "exit": 1,
            "principles": [p for p in PRINCIPLES if p != "P-DENSE"],
            "findings": ["PRINC-ID-MISSING Dense but calm", "PRINC-ID-MISSING P-DENSE"],
        },
        "unknown.md": {
            "exit": 1,
            "principles": PRINCIPLES,
            "findings": [
                "PRINC-EXAMPLE-MISSING P-KEYS",
                "PRINC-UNKNOWN P-FAST",
                "PRINC-UNKNOWN P-NOCOLOR",
            ],
        },
    },
    "words": {
        "copy-ok.jsonl": {"words": "shipped", "exit": 0, "messages": 15, "findings": []},
        "copy-bad.jsonl": {
            "words": "shipped",
            "exit": 1,
            "messages": 10,
            "findings": [
                "WORD-RESERVED approve agent.plan.status",
                "WORD-RESERVED approve agent.proposal.apply_button",
                "WORD-RESERVED approve agent.proposal.bulk_button",
                "WORD-RESERVED approve run.approved.count",
                "WORD-RESERVED conforms viz.inspect.badge",
                "WORD-RESERVED release agent.proposal.bulk_button",
                "WORD-RESERVED release program.sign.tooltip",
                "WORD-RESERVED release viz.update.banner",
                "WORD-RESERVED sign program.sign.tooltip",
                "WORD-RESERVED sign viz.shell.login",
                "WORD-RESERVED verified rpt.summary.note",
            ],
        },
        "copy-schema.jsonl": {
            "words": "shipped",
            "exit": 1,
            "messages": 2,
            "findings": [
                "WORD-RESERVED approve agent.x",
                "WORDS-SCHEMA copy:2",
                "WORDS-SCHEMA copy:3",
                "WORDS-SCHEMA copy:4",
                "WORDS-SCHEMA copy:5",
                "WORDS-SCHEMA copy:6",
                "WORDS-SCHEMA copy:7",
            ],
        },
        "words-bad-copy.jsonl": {
            "words": "words-bad.json",
            "exit": 1,
            "messages": 5,
            "findings": [
                "WORD-RESERVED Cert agent.c",
                "WORD-RESERVED approve agent.a",
                "WORDS-SCHEMA words[1]",
                "WORDS-SCHEMA words[2]",
                "WORDS-SCHEMA words[3]",
                "WORDS-SCHEMA words[4]",
                "WORDS-SCHEMA words[5]",
                "WORDS-SCHEMA words[6]",
            ],
        },
        "copy-ok.jsonl+words-object.json": {
            "words": "words-object.json",
            "exit": 1,
            "messages": 15,
            "findings": ["WORDS-SCHEMA words"],
        },
    },
}


# ---- Independent oracle (Python reading of the leaf's rules) -------------------------------


def oracle_princ(text):
    secs, cur, sub, fenced = [], None, None, False
    for raw in text.splitlines():
        line = raw.rstrip()
        fence = line.lstrip().startswith(("```", "~~~"))
        if fenced or fence:
            if fence:
                fenced = not fenced
            if cur is not None and sub in ("do", "dont"):
                cur[sub] = True
            continue
        if line.startswith("## "):
            h = line[3:].strip()
            m = re.fullmatch(r"(P-[A-Z][A-Z0-9]*):\s*(\S.*)", h)
            cur = {"h": h, "id": m.group(1) if m else None, "do": False, "dont": False}
            secs.append(cur)
            sub = None
        elif line.startswith("# ") or line == "#":
            cur, sub = None, None
        elif line.startswith("### "):
            t = line[4:].strip()
            sub = "do" if t == "Do" else "dont" if t in ("Don't", "Don’t") else "other"
        elif line.strip() and cur is not None and sub in ("do", "dont"):
            cur[sub] = True
    found, seen = [], set()
    for s in secs:
        subj = s["id"] or s["h"]
        if s["id"] is None:
            found.append(("PRINC-ID-MISSING", s["h"]))
        else:
            if s["id"] not in PRINCIPLES or s["id"] in seen:
                found.append(("PRINC-UNKNOWN", s["id"]))
            seen.add(s["id"])
        if not s["do"]:
            found.append(("PRINC-EXAMPLE-MISSING", subj))
        if not s["dont"]:
            found.append(("PRINC-EXAMPLE-MISSING", subj))
    for p in PRINCIPLES:
        if p not in seen:
            found.append(("PRINC-ID-MISSING", p))
    keys = sorted(set(f"{c} {s}" for c, s in found))
    return sorted(p for p in seen if p in PRINCIPLES), keys


def oracle_words(copy_text, words):
    """`words`: parsed list of valid entries (the oracle trusts the shipped list's shape)."""
    out = set()
    n = 0
    for line in copy_text.splitlines():
        if not line.strip():
            continue
        m = json.loads(line)
        n += 1
        for tok in re.split(r"[^\w]|_", m["text"]):
            if not tok:
                continue
            for w in words:
                forms = [w["word"]] + w.get("inflections", [])
                hit = tok.lower() in [f.lower() for f in forms] if w["case_insensitive"] else tok in forms
                if hit and not any(m["id"].startswith(p) for p in w["allowed_prefixes"]):
                    out.add(f"WORD-RESERVED {w['word']} {m['id']}")
    return n, sorted(out)


def oracle_check():
    errors = []
    shipped = (ROOT / "design" / "principles.md").read_text(encoding="utf-8")
    cases = {"shipped": shipped}
    cases.update({k: FILES[k] for k in ("no-dont.md", "no-id.md", "unknown.md")})
    for name, text in cases.items():
        ids, keys = oracle_princ(text)
        e = EXPECTED["princ"][name]
        if keys != e["findings"] or ids != e["principles"] or e["exit"] != (1 if keys else 0):
            errors.append(f"princ {name}: oracle {ids} {keys} != label {e}")
    words = json.loads((ROOT / "design" / "reserved-words.json").read_text(encoding="utf-8"))
    for w in ("approve", "release", "sign", "conforms", "verified"):
        if w not in [x["word"] for x in words]:
            errors.append(f"shipped reserved-words.json lacks {w}")
    approve = [x for x in words if x["word"] == "approve"]
    want = ["program.release.", "review.human.", "run.approve."]
    if not approve or sorted(approve[0]["allowed_prefixes"]) != want:
        errors.append("approve must be permitted under exactly " + ", ".join(want))
    for name in ("copy-ok.jsonl", "copy-bad.jsonl"):
        n, keys = oracle_words(FILES[name], words)
        e = EXPECTED["words"][name]
        if keys != e["findings"] or n != e["messages"]:
            errors.append(f"words {name}: oracle {n} {keys} != label {e}")
    return errors


def main():
    check = "--check" in sys.argv[1:]
    bad = []
    files = dict(FILES)
    files["expected.json"] = json.dumps(EXPECTED, indent=2, ensure_ascii=False) + "\n"
    for name, text in files.items():
        p = HERE / name
        data = text.encode("utf-8")
        if check:
            if not p.exists() or p.read_bytes() != data:
                bad.append(f"{name} differs from make_fixtures.py output")
        else:
            p.write_bytes(data)
    errs = oracle_check()
    bad += errs
    for b in bad:
        print("FAIL:", b, file=sys.stderr)
    if bad:
        return 1
    print("ok" if check else f"wrote {len(files)} files")
    return 0


if __name__ == "__main__":
    sys.exit(main())
