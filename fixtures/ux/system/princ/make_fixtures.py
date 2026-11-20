#!/usr/bin/env python3
"""Writes the fixtures of spec node UX.SYSTEM.PRINC and checks their hand labels.

Synthetic data, CC-BY-4.0. Python stdlib only. Run from any directory:

    python fixtures/ux/system/princ/make_fixtures.py          # (re)write the fixtures
    python fixtures/ux/system/princ/make_fixtures.py --check  # fail if files differ or labels disagree

The expected results in `expected.json` are hand-labelled from the leaf's Requirements and
Design (written below as literals, not computed by the code under test). `--check` also runs an
independent Python reading of the same rules over every fixture and the shipped
`design/principles.md` and `design/reserved-words.json`, and fails if it disagrees with a label.
The oracle does not mirror the Rust tokenizer: it searches each reserved form with a regular
expression bounded by non-word characters, classifies characters with `unicodedata`
(general category, decomposition tag, character name) rather than code-point ranges, and
validates both input files with `json` and a duplicate-key hook.
"""

import json
import pathlib
import re
import sys
import unicodedata

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[3]

PRINCIPLES = ["P-DATA", "P-DENSE", "P-EXPLAIN", "P-FAMILIAR", "P-KEYS", "P-NOCOLOR"]
REQUIRED_WORDS = ["approve", "conforms", "release", "sign", "verified"]

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
    {"id": "rpt.export.title", "text": "Export report…"},
    # Accented Latin copy (German, French) and all-Greek copy stay clean.
    {"id": "viz.sample.de", "text": "Größe prüfen"},
    {"id": "viz.sample.fr", "text": "Vérifier la cote"},
    {"id": "viz.sample.el", "text": "Μέτρηση"},
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

# Evasion attempts (review of #297) and the forms added to the shipped list.
COPY_OBFUSCATED = [
    {"id": "agent.a", "text": "аpprove"},  # Cyrillic a + Latin
    {"id": "agent.b", "text": "ap­prove"},  # soft hyphen
    {"id": "agent.c", "text": "appr​ove"},  # zero-width space
    {"id": "agent.cgj", "text": "͏approve"},  # combining grapheme joiner
    {"id": "agent.d", "text": "Veriﬁed"},  # fi ligature
    {"id": "agent.e", "text": "ＡＰＰＲＯＶＥ"},  # fullwidth APPROVE
    {"id": "agent.f", "text": "\U0001d400\U0001d429\U0001d429\U0001d42b\U0001d428\U0001d42f\U0001d41e"},  # math bold
    {"id": "agent.g", "text": "VERİFİED"},  # Turkish dotted capital I
    {"id": "agent.g2", "text": "Approvıng"},  # Turkish dotless i
    {"id": "agent.h", "text": "approve⁠"},  # word joiner
    {"id": "agent.i", "text": "‮evorppa"},  # right-to-left override
    {"id": "agent.bom", "text": "Sign﻿ed"},  # byte-order mark inside a word
    {"id": "agent.rlm", "text": "‏Release"},  # right-to-left mark
    {"id": "agent.greek", "text": "Vεrified"},  # Greek epsilon + Latin
    {"id": "agent.k", "text": "Nonconforming part"},
    {"id": "agent.l", "text": "Preapproved"},
    {"id": "agent.l2", "text": "Unapproved, then reapproved"},
    {"id": "agent.m", "text": "Signoff required"},
    {"id": "agent.m2", "text": "Cosigned and countersigned"},
    {"id": "agent.n", "text": "Verifier output"},
    {"id": "agent.o", "text": "Conformity, conformant, nonconformity, nonconformance, nonconformant"},
    {"id": "agent.sl", "text": "ſign here"},  # long s (compatibility form)
    {"id": "agent.lls", "text": "ℜelease now"},  # letterlike black-letter R
    {"id": "agent.tag", "text": "approve\U000e0041"},  # tag character after the word
    {"id": "agent.hf", "text": "approveㅤ"},  # Hangul filler inside the word
    # All-Cyrillic look-alikes are a documented gap: not flagged.
    {"id": "viz.sample.ru", "text": "Проверено"},
    {"id": "viz.sample.de", "text": "Maße bestätigt"},
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
    '{"id":"Agent.Upper","text":"x"}\n'
    '{"id":"agent-x","text":"x"}\n'
    '{"id":"","text":"x"}\n'
    "\n"
)

COPY_EMPTY = "\n\n"

WORDS_BAD = (
    "[\n"
    '  {"word": "approve", "meaning": "Human approval.", "allowed_prefixes": ["review.human."], "case_insensitive": true, "inflections": ["approved"]},\n'
    '  {"word": "Approved", "meaning": "Clashes with an inflection of approve.", "allowed_prefixes": [], "case_insensitive": true},\n'
    '  {"word": "sign in", "meaning": "Two words.", "allowed_prefixes": [], "case_insensitive": true},\n'
    '  {"word": "release", "meaning": "", "allowed_prefixes": [], "case_insensitive": true},\n'
    '  {"word": "conforms", "meaning": "Prefixes not an array.", "allowed_prefixes": "gdt.", "case_insensitive": true},\n'
    '  {"word": "verified", "meaning": "Flag not a boolean.", "allowed_prefixes": [], "case_insensitive": "yes"},\n'
    '  {"word": "sign", "meaning": "Unknown key.", "allowed_prefixes": [], "case_insensitive": true, "colour": "red"},\n'
    '  {"word": "cert", "meaning": "Case-sensitive matching is not allowed.", "allowed_prefixes": ["trust."], "case_insensitive": false},\n'
    '  {"word": "audit", "meaning": "Prefix without a trailing dot.", "allowed_prefixes": ["trust"], "case_insensitive": true}\n'
    "]\n"
)

WORDS_BAD_COPY = [
    {"id": "agent.a", "text": "approved"},
    {"id": "agent.b", "text": "cert CERT"},
    {"id": "agent.c", "text": "audit log"},
    {"id": "agent.e", "text": "sign in"},
]

WORDS_OBJECT = '{"format": "reserved-words@1", "words": []}\n'
WORDS_EMPTY = "[]\n"

FILES = {
    "no-dont.md": no_dont(),
    "no-id.md": no_id(),
    "unknown.md": unknown(),
    "copy-ok.jsonl": jsonl(COPY_OK),
    "copy-bad.jsonl": jsonl(COPY_BAD),
    "copy-obfuscated.jsonl": jsonl(COPY_OBFUSCATED),
    "copy-schema.jsonl": COPY_SCHEMA,
    "copy-empty.jsonl": COPY_EMPTY,
    "words-bad.json": WORDS_BAD,
    "words-bad-copy.jsonl": jsonl(WORDS_BAD_COPY),
    "words-object.json": WORDS_OBJECT,
    "words-empty.json": WORDS_EMPTY,
}

# Hand-labelled expectations: exit code and the "CODE subject" of every finding (the report
# lists them sorted; the labels are sorted here so they can be written in any order).
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
                "PRINC-DUPLICATE P-NOCOLOR",
                "PRINC-EXAMPLE-MISSING P-KEYS",
                "PRINC-UNKNOWN P-FAST",
            ],
        },
    },
    "words": {
        "copy-ok.jsonl": {"words": "shipped", "exit": 0, "messages": 19, "findings": []},
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
        "copy-obfuscated.jsonl": {
            "words": "shipped",
            "exit": 1,
            "messages": 27,
            "findings": [
                "WORDS-OBFUSCATED agent.sl",
                "WORDS-OBFUSCATED agent.lls",
                "WORDS-OBFUSCATED agent.tag",
                "WORD-RESERVED approve agent.tag",
                "WORDS-OBFUSCATED agent.hf",
                "WORDS-OBFUSCATED agent.a",
                "WORDS-OBFUSCATED agent.b",
                "WORDS-OBFUSCATED agent.c",
                "WORDS-OBFUSCATED agent.cgj",
                "WORD-RESERVED approve agent.cgj",
                "WORDS-OBFUSCATED agent.d",
                "WORDS-OBFUSCATED agent.e",
                "WORDS-OBFUSCATED agent.f",
                "WORD-RESERVED verified agent.g",
                "WORD-RESERVED approve agent.g2",
                "WORDS-OBFUSCATED agent.h",
                "WORD-RESERVED approve agent.h",
                "WORDS-OBFUSCATED agent.i",
                "WORDS-OBFUSCATED agent.bom",
                "WORD-RESERVED sign agent.bom",
                "WORDS-OBFUSCATED agent.rlm",
                "WORD-RESERVED release agent.rlm",
                "WORDS-OBFUSCATED agent.greek",
                "WORD-RESERVED conforms agent.k",
                "WORD-RESERVED approve agent.l",
                "WORD-RESERVED approve agent.l2",
                "WORD-RESERVED sign agent.m",
                "WORD-RESERVED sign agent.m2",
                "WORD-RESERVED verified agent.n",
                "WORD-RESERVED conforms agent.o",
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
                "WORDS-SCHEMA copy:9",
                "WORDS-SCHEMA copy:10",
                "WORDS-SCHEMA copy:11",
            ],
        },
        "copy-empty.jsonl": {"words": "shipped", "exit": 1, "messages": 0, "findings": ["WORDS-SCHEMA copy"]},
        "words-bad-copy.jsonl": {
            "words": "words-bad.json",
            "exit": 1,
            "messages": 4,
            "findings": [
                "WORD-RESERVED approve agent.a",
                "WORDS-SCHEMA words",
                "WORDS-SCHEMA words[1]",
                "WORDS-SCHEMA words[2]",
                "WORDS-SCHEMA words[3]",
                "WORDS-SCHEMA words[4]",
                "WORDS-SCHEMA words[5]",
                "WORDS-SCHEMA words[6]",
                "WORDS-SCHEMA words[7]",
                "WORDS-SCHEMA words[8]",
            ],
        },
        "copy-ok.jsonl+words-object.json": {
            "words": "words-object.json",
            "exit": 1,
            "messages": 19,
            "findings": ["WORDS-SCHEMA words"],
        },
        "copy-ok.jsonl+words-empty.json": {
            "words": "words-empty.json",
            "exit": 1,
            "messages": 19,
            "findings": ["WORDS-SCHEMA words"],
        },
    },
}
for _case in EXPECTED["words"].values():
    _case["findings"] = sorted(_case["findings"])


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
            if s["id"] not in PRINCIPLES:
                found.append(("PRINC-UNKNOWN", s["id"]))
            if s["id"] in seen:
                found.append(("PRINC-DUPLICATE", s["id"]))
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


class DupKey(ValueError):
    pass


def no_dups(pairs):
    keys = [k for k, _ in pairs]
    if len(keys) != len(set(keys)):
        raise DupKey("duplicate key")
    return dict(pairs)


def loads(text):
    return json.loads(text, object_pairs_hook=no_dups)


ID_RE = re.compile(r"[a-z0-9._]+")
WORD_RE = re.compile(r"[^\W_]+")


def oracle_words_file(text):
    """Returns (valid entries, finding subjects) for a reserved-word list."""
    try:
        doc = loads(text)
    except ValueError:
        return [], ["words"]
    if not isinstance(doc, list) or not doc:
        return [], ["words"]
    valid, subjects, owners = [], [], {}
    for i, e in enumerate(doc):
        ok = (
            isinstance(e, dict)
            and set(e) <= {"word", "meaning", "allowed_prefixes", "case_insensitive", "inflections"}
            and isinstance(e.get("word"), str)
            and WORD_RE.fullmatch(e["word"]) is not None
            and isinstance(e.get("meaning"), str)
            and e["meaning"].strip() != ""
            and isinstance(e.get("allowed_prefixes"), list)
            and all(isinstance(p, str) and ID_RE.fullmatch(p) and p.endswith(".") and len(p) > 1 for p in e["allowed_prefixes"])
            and e.get("case_insensitive") is True
            and isinstance(e.get("inflections", []), list)
            and all(isinstance(x, str) and WORD_RE.fullmatch(x) for x in e.get("inflections", []))
        )
        if ok:
            forms = [e["word"]] + e.get("inflections", [])
            keys = [fold(f) for f in forms]
            if len(set(keys)) != len(keys) or any(k in owners for k in keys):
                ok = False
            else:
                for k in keys:
                    owners[k] = e["word"]
        if ok:
            valid.append(e)
        else:
            subjects.append(f"words[{i}]")
    names = {fold(e["word"]) for e in valid}
    if any(r not in names for r in REQUIRED_WORDS):
        subjects.append("words")
    return valid, subjects


def oracle_copy(text):
    """Returns (messages, finding subjects) for a UI-copy export."""
    msgs, subjects, seen = [], [], set()
    for n, line in enumerate(text.split("\n"), start=1):
        if not line.strip():
            continue
        try:
            m = loads(line)
        except ValueError:
            subjects.append(f"copy:{n}")
            continue
        if (
            not isinstance(m, dict)
            or set(m) != {"id", "text"}
            or not isinstance(m["id"], str)
            or not isinstance(m["text"], str)
            or not ID_RE.fullmatch(m["id"])
            or m["id"] in seen
        ):
            subjects.append(f"copy:{n}")
            continue
        seen.add(m["id"])
        msgs.append(m)
    if not msgs:
        subjects.append("copy")
    return msgs, subjects


def fold(s):
    return s.replace("İ", "i").replace("ı", "i").lower()


def suspicious(text):
    """Invisible, compatibility-form or mixed-script content, judged by Unicode properties."""
    for ch in text:
        cat = unicodedata.category(ch)
        tag = unicodedata.decomposition(ch).split(" ")[0]
        # Hangul fillers are letters (Lo) by category but render as nothing.
        if cat == "Cf" or ch == "͏" or ch in "ᅟᅠㅤﾠ":
            return True
        if tag in ("<wide>", "<font>") or (tag == "<compat>" and cat.startswith("L")):
            return True
    for w in re.findall(r"\w+", text):
        scripts = set()
        for ch in w:
            name = unicodedata.name(ch, "")
            for s in ("LATIN", "CYRILLIC", "GREEK"):
                if name.startswith(s):
                    scripts.add(s)
        if "LATIN" in scripts and len(scripts) > 1:
            return True
    return False


def oracle_words(copy_text, words_text):
    words, subjects = oracle_words_file(words_text)
    msgs, copy_subjects = oracle_copy(copy_text)
    keys = {f"WORDS-SCHEMA {s}" for s in subjects + copy_subjects}
    for m in msgs:
        text = fold(m["text"])
        if suspicious(m["text"]):
            keys.add(f"WORDS-OBFUSCATED {m['id']}")
        for w in words:
            if any(m["id"].startswith(p) for p in w["allowed_prefixes"]):
                continue
            for form in [w["word"]] + w.get("inflections", []):
                pat = r"(?<![^\W_])" + re.escape(fold(form)) + r"(?![^\W_])"
                if re.search(pat, text):
                    keys.add(f"WORD-RESERVED {w['word']} {m['id']}")
                    break
    return len(msgs), sorted(keys)


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
    shipped_words = (ROOT / "design" / "reserved-words.json").read_text(encoding="utf-8")
    words = loads(shipped_words)
    approve = [x for x in words if x["word"] == "approve"]
    want = ["program.release.", "review.human.", "run.approve."]
    if not approve or sorted(approve[0]["allowed_prefixes"]) != want:
        errors.append("approve must be permitted under exactly " + ", ".join(want))
    for key, e in EXPECTED["words"].items():
        copy = FILES[key.split("+")[0]]
        wtext = shipped_words if e["words"] == "shipped" else FILES[e["words"]]
        n, keys = oracle_words(copy, wtext)
        if keys != e["findings"] or n != e["messages"] or e["exit"] != (1 if keys else 0):
            errors.append(f"words {key}: oracle {n} {keys} != label {e}")
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
    bad += oracle_check()
    for b in bad:
        print("FAIL:", b, file=sys.stderr)
    if bad:
        return 1
    print("ok" if check else f"wrote {len(files)} files")
    return 0


if __name__ == "__main__":
    sys.exit(main())
