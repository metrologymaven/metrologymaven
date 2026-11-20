#!/usr/bin/env python3
"""Writes the fixtures of spec node UX.SYSTEM.REVIEW and checks their hand labels.

Synthetic data, CC-BY-4.0. Python stdlib only (3.11+ for tomllib). Run from any directory:

    python fixtures/ux/system/review/make_fixtures.py          # (re)write the fixtures
    python fixtures/ux/system/review/make_fixtures.py --check  # fail if files differ or labels disagree

Each case is a directory `<case>/` holding `changed.txt` (the changed-path list, relative to the
design directory) and `design/` (the design directory given as `--root`). The expected results
in `expected.json` are hand-labelled from the leaf's Requirements, Design and Acceptance
criteria (written below as literals in LABELS, not computed by the code under test).

`--check` also runs an independent Python reading of the same rules over every case and fails
if it disagrees with a label. The oracle does not mirror the Rust code: globs are matched by a
recursive segment walk with `fnmatch.fnmatchcase`, the map is read with `tomllib`, JSON with a
duplicate-key hook, refused text is judged with `unicodedata` (general categories Cc and Cf,
decomposition tags, character names, `is_normalized`), and the name scan uses `os.walk` with
NFKC and `str.lower`.

The deny-list name `Examplar` is invented for these fixtures; it is not a candidate name.
"""

import fnmatch
import json
import os
import pathlib
import sys
import tomllib
import unicodedata

HERE = pathlib.Path(__file__).resolve().parent

PRINCIPLES = ["P-DATA", "P-DENSE", "P-EXPLAIN", "P-FAMILIAR", "P-KEYS", "P-NOCOLOR"]
CHECKLIST = ["C-CONTRAST", "C-DRESS", "C-KEYBOARD", "C-NAME", "C-NOCOLOR", "C-ORIGINALITY", "C-PRINCIPLES"]
RULES = ["TD-01", "TD-02", "TD-03", "TD-04", "TD-05"]
STATEMENT = "originality-statement@1"
REC = "review/records/pr-0042.json"


def principles_md():
    lines = ["# Design principles (test fixture)", "", "Synthetic, CC-BY-4.0.", ""]
    for p in PRINCIPLES:
        lines += [f"## {p}: Synthetic {p}", "", "### Do", "", f"- Follow {p}.", "", "### Don't", "", f"- Break {p}.", ""]
    return "\n".join(lines).rstrip("\n") + "\n"


def js(v):
    return json.dumps(v, indent=2, ensure_ascii=True) + "\n"


def paths_toml(globs=("icons/**", "tokens/*.tokens.json", "**/*.svg")):
    g = ", ".join(json.dumps(x) for x in globs)
    return f'# Synthetic review path map, CC-BY-4.0.\nformat = "review-paths@1"\nreview = [{g}]\n'


def dress_rules(ids=RULES):
    return js([{"id": i, "text_ref": f"synthetic brand guide rule {i}"} for i in ids])


def decl(artefact, **over):
    d = {
        "artefact": artefact,
        "author": "A. Contributor",
        "date": "2026-11-18",
        "references": ["A hand sketch made for this project"],
        "statement_version": STATEMENT,
    }
    d.update(over)
    return js(d)


def record(**over):
    r = {
        "pr": 42,
        "artefacts": ["icons/probe.svg", "tokens/color.tokens.json"],
        "principles": ["P-DATA", "P-NOCOLOR"],
        "dress": [{"rule": i, "result": "pass"} for i in RULES],
        "checklist": [{"item": c, "result": "pass"} for c in CHECKLIST],
        "approver": {"name": "A. Maintainer", "principal": "human"},
        "date": "2026-11-19",
    }
    r.update(over)
    return js(r)


def base_design():
    return {
        "principles.md": principles_md(),
        "review/paths.toml": paths_toml(),
        "review/dress-rules.json": dress_rules(),
        "tokens/color.tokens.json": js({"color": {"surface": {"$type": "color", "$value": "#f4f4f2"}}}),
        "tokens/color.tokens.json.origin.json": decl("tokens/color.tokens.json"),
        "icons/probe.svg": '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><circle cx="8" cy="8" r="3"/></svg>\n',
        "review/origin/icons/probe.svg.origin.json": decl("icons/probe.svg"),
        REC: record(),
        "notes/readme.txt": "Synthetic notes for {{FAMILY_NAME}}; not in the review map.\n",
    }


OK_CHANGED = [
    "icons/probe.svg",
    "notes/readme.txt",
    "review/origin/icons/probe.svg.origin.json",
    REC,
    "tokens/color.tokens.json",
    "tokens/color.tokens.json.origin.json",
]


def lines(ps):
    return "".join(p + "\n" for p in ps)


def case(changed=None, drop=(), **files):
    d = base_design()
    for k in drop:
        d.pop(k)
    d.update(files)
    return {"changed": lines(OK_CHANGED) if changed is None else changed, "design": d}


def checklist_without(item):
    return [{"item": c, "result": "pass"} for c in CHECKLIST if c != item]


def build_cases():
    c = {}
    c["ok"] = case()
    c["missing-record"] = case(changed="tokens/color.tokens.json\n", drop=[REC])
    c["agent-approver"] = case(**{REC: record(approver={"name": "build-agent", "principal": "agent"})})
    c["dress-gap"] = case(**{REC: record(dress=[{"rule": i, "result": "pass"} for i in RULES[:4]])})
    c["no-decl"] = case(
        changed=lines([p for p in OK_CHANGED if p != "review/origin/icons/probe.svg.origin.json"]),
        drop=["review/origin/icons/probe.svg.origin.json"],
    )
    c["name-leak"] = case(
        **{
            "components/button.md": "# Button\n\nDo not copy the EXAMPLAR toolbar look.\n",
            "copy/labels.txt": "Welcome to Exam​plar\n",
        }
    )
    c["principle-unknown"] = case(**{REC: record(principles=["P-DATA", "P-SHINY"])})
    c["stale-record"] = case(changed=lines([p for p in OK_CHANGED if p != REC]))
    c["bidi-record"] = case(**{REC: record(approver={"name": "Ana ‮Lopez", "principal": "human"})})
    c["nfd-record"] = case(**{REC: record(approver={"name": "José Ruiz", "principal": "human"})})
    c["obfuscated-changed"] = case(changed=lines(["icons/pro​be.svg"] + OK_CHANGED[1:]))
    c["empty-changed"] = case(changed="\n\n")
    dress = [{"rule": i, "result": "fail" if i == "TD-02" else "pass"} for i in RULES]
    cl = [{"item": x, "result": "n/a"} if x == "C-CONTRAST" else {"item": x, "result": "pass"} for x in CHECKLIST]
    c["result-fail"] = case(**{REC: record(dress=dress, checklist=cl)})
    c["checklist-gap"] = case(**{REC: record(checklist=checklist_without("C-KEYBOARD"))})
    c["map-weakened"] = case(
        changed=lines(sorted(OK_CHANGED + ["review/paths.toml"])), **{"review/paths.toml": paths_toml(["nothing/**"])}
    )
    c["bad-map"] = case(
        **{"review/paths.toml": 'format = "review-paths@1"\nformat = "review-paths@1"\nreview = ["**"]\n'}
    )
    c["empty-dress"] = case(**{"review/dress-rules.json": "[]\n"})
    c["bot-approver"] = case(**{REC: record(approver={"name": "design-helper[bot]", "principal": "human"})})
    c["bad-decl"] = case(
        **{
            "tokens/color.tokens.json.origin.json": decl(
                "tokens/color.tokens.json", references=["https://example.invalid/screen.png"]
            )
        }
    )
    c["two-decl"] = case(**{"icons/probe.svg.origin.json": decl("icons/probe.svg")})
    c["dup-key-record"] = case(**{REC: '{"pr": 42, "pr": 43}\n'})
    c["deleted-artefact"] = case(
        changed=lines(sorted(OK_CHANGED + ["icons/old.svg"])),
        **{REC: record(artefacts=["icons/old.svg", "icons/probe.svg", "tokens/color.tokens.json"])},
    )
    return c


SIDE = {
    "deny-names.txt": "# Synthetic deny list for UX.SYSTEM.REVIEW fixtures; the name is invented. CC-BY-4.0.\nExamplar\nQuarzolith\n",
    "deny-empty.txt": "\n# A deny list with no names.\n",
    "deny-bidi.txt": "Exam‮plar\nQuarzolith\n",
}

BOTH = ["icons/probe.svg", "tokens/color.tokens.json"]
MISS_BOTH = ["REVIEW-RECORD-MISSING icons/probe.svg", "REVIEW-RECORD-MISSING tokens/color.tokens.json"]
S_REC = f"REVIEW-SCHEMA {REC}"

# Hand labels: run key -> (case dir, deny file or None, exit, findings "CODE subject", needs_review, records).
LABELS = {
    "ok": ("ok", None, 0, [], BOTH, [REC]),
    "ok+deny": ("ok", "deny-names.txt", 0, [], BOTH, [REC]),
    "missing-record": ("missing-record", None, 1, ["REVIEW-RECORD-MISSING tokens/color.tokens.json"], ["tokens/color.tokens.json"], []),
    "agent-approver": ("agent-approver", None, 1, [f"REVIEW-APPROVER-NOT-HUMAN {REC}"] + MISS_BOTH, BOTH, []),
    "dress-gap": ("dress-gap", None, 1, ["REVIEW-DRESS-RESULT-MISSING TD-05"] + MISS_BOTH, BOTH, []),
    "no-decl": ("no-decl", None, 1, ["REVIEW-ORIGINALITY-MISSING icons/probe.svg"] + MISS_BOTH, BOTH, []),
    "name-leak": ("name-leak", "deny-names.txt", 1, ["REVIEW-NAME-LEAK components/button.md", "REVIEW-NAME-LEAK copy/labels.txt"], BOTH, [REC]),
    "name-leak-nodeny": ("name-leak", None, 0, [], BOTH, [REC]),
    "principle-unknown": ("principle-unknown", None, 1, ["REVIEW-PRINCIPLE-UNKNOWN P-SHINY"] + MISS_BOTH, BOTH, []),
    "stale-record": ("stale-record", None, 1, MISS_BOTH, BOTH, []),
    "bidi-record": ("bidi-record", None, 1, MISS_BOTH + [S_REC], BOTH, []),
    "nfd-record": ("nfd-record", None, 1, MISS_BOTH + [S_REC], BOTH, []),
    "obfuscated-changed": ("obfuscated-changed", None, 1, ["REVIEW-SCHEMA changed:1"], ["tokens/color.tokens.json"], [REC]),
    "empty-changed": ("empty-changed", None, 1, ["REVIEW-SCHEMA changed"], [], []),
    "result-fail": ("result-fail", None, 1, MISS_BOTH + ["REVIEW-RESULT-FAIL TD-02", S_REC], BOTH, []),
    "checklist-gap": ("checklist-gap", None, 1, ["REVIEW-CHECKLIST-MISSING C-KEYBOARD"] + MISS_BOTH, BOTH, []),
    "map-weakened": ("map-weakened", None, 1, ["REVIEW-RECORD-MISSING review/paths.toml"], ["review/paths.toml"], [REC]),
    "bad-map": ("bad-map", None, 1, ["REVIEW-RECORD-MISSING notes/readme.txt", "REVIEW-SCHEMA review/paths.toml"], ["icons/probe.svg", "notes/readme.txt", "tokens/color.tokens.json"], [REC]),
    "empty-dress": ("empty-dress", None, 1, MISS_BOTH + ["REVIEW-SCHEMA review/dress-rules.json", S_REC], BOTH, []),
    "bot-approver": ("bot-approver", None, 1, [f"REVIEW-APPROVER-NOT-HUMAN {REC}"] + MISS_BOTH, BOTH, []),
    "bad-decl": ("bad-decl", None, 1, ["REVIEW-ORIGINALITY-MISSING tokens/color.tokens.json"] + MISS_BOTH + ["REVIEW-SCHEMA tokens/color.tokens.json.origin.json"], BOTH, []),
    "two-decl": ("two-decl", None, 1, ["REVIEW-ORIGINALITY-MISSING icons/probe.svg"] + MISS_BOTH + ["REVIEW-SCHEMA icons/probe.svg"], BOTH, []),
    "dup-key-record": ("dup-key-record", None, 1, MISS_BOTH + [S_REC], BOTH, []),
    "deleted-artefact": ("deleted-artefact", None, 0, [], ["icons/old.svg", "icons/probe.svg", "tokens/color.tokens.json"], [REC]),
    "empty-deny": ("ok", "deny-empty.txt", 1, ["REVIEW-SCHEMA deny-names"], BOTH, [REC]),
    "deny-bidi": ("ok", "deny-bidi.txt", 1, ["REVIEW-SCHEMA deny-names"], BOTH, [REC]),
}


def expected():
    out = {}
    for k, (d, deny, ex, fs, nr, recs) in LABELS.items():
        out[k] = {"case": d, "deny": deny, "exit": ex, "findings": fs, "needs_review": nr, "records": recs}
    return out


def all_files():
    files = {}
    for name, c in build_cases().items():
        files[f"{name}/changed.txt"] = c["changed"]
        for rel, text in c["design"].items():
            files[f"{name}/design/{rel}"] = text
    files.update(SIDE)
    files["expected.json"] = json.dumps(expected(), indent=2, ensure_ascii=True) + "\n"
    return files


# ---------------------------------------------------------------------------------------------
# Independent oracle.


class DupKey(ValueError):
    pass


def no_dups(pairs):
    keys = [k for k, _ in pairs]
    if len(keys) != len(set(keys)):
        raise DupKey("duplicate key")
    return dict(pairs)


def loads(text):
    return json.loads(text, object_pairs_hook=no_dups)


def refused(s):
    """Control, format (bidi, zero-width), compatibility or mixed-script text, or not NFC."""
    if not unicodedata.is_normalized("NFC", s):
        return True
    for ch in s:
        cat = unicodedata.category(ch)
        tag = unicodedata.decomposition(ch).split(" ")[0]
        if cat in ("Cc", "Cf") or ch == "͏":
            return True
        if tag in ("<wide>", "<font>") or (tag == "<compat>" and cat.startswith("L")):
            return True
    word = ""
    for ch in s + " ":
        if ch.isalnum():
            word += ch
            continue
        scripts = {n for n in ("LATIN", "CYRILLIC", "GREEK") for c in word if unicodedata.name(c, "").startswith(n)}
        if "LATIN" in scripts and len(scripts) > 1:
            return True
        word = ""
    return False


def any_refused(v):
    if isinstance(v, str):
        return refused(v)
    if isinstance(v, list):
        return any(any_refused(x) for x in v)
    if isinstance(v, dict):
        return any(refused(k) or any_refused(x) for k, x in v.items())
    return False


def bad_path(p):
    if not p or refused(p) or any(ch in p for ch in "\\:*?[]{}"):
        return True
    return any(s in ("", ".", "..") for s in p.split("/"))


def glob_hits(pat, segs):
    if not pat:
        return not segs
    if pat[0] == "**":
        return any(glob_hits(pat[1:], segs[i:]) for i in range(len(segs) + 1))
    return bool(segs) and fnmatch.fnmatchcase(segs[0], pat[0]) and glob_hits(pat[1:], segs[1:])


def is_text(v):
    return isinstance(v, str) and v.strip() != ""


def is_date(s):
    import datetime

    if not isinstance(s, str) or len(s) != 10:
        return False
    try:
        datetime.date.fromisoformat(s)
        return True
    except ValueError:
        return False


def oracle_decl(text, artefact):
    try:
        d = loads(text)
    except ValueError:
        return False
    if not isinstance(d, dict) or any_refused(d):
        return False
    if set(d) != {"artefact", "author", "date", "references", "statement_version"}:
        return False
    refs = d["references"]
    return (
        d["artefact"] == artefact
        and is_text(d["author"])
        and is_date(d["date"])
        and isinstance(refs, list)
        and len(refs) > 0
        and all(is_text(r) and "://" not in r and not r.lower().rstrip(".").endswith((".png", ".svg", ".jpg")) for r in refs)
        and d["statement_version"] == STATEMENT
    )


def oracle_results(lst, key, allowed):
    """Returns (schema_ok, answered keys, failed keys)."""
    if not isinstance(lst, list):
        return False, set(), set()
    seen, failed, ok = set(), set(), True
    for e in lst:
        if not isinstance(e, dict) or not set(e) <= {key, "result", "reason"} or key not in e or "result" not in e:
            return False, seen, failed
        if e["result"] not in ("pass", "fail", "n/a") or e[key] in seen:
            return False, seen, failed
        if e["result"] == "n/a" and not is_text(e.get("reason")):
            ok = False
        if e[key] not in allowed:
            ok = False
        seen.add(e[key])
        if e["result"] == "fail":
            failed.add(e[key])
    return ok, seen, failed


def oracle(case_dir, deny_path):
    design = case_dir / "design"
    keys = set()
    changed = []
    raw = (case_dir / "changed.txt").read_text(encoding="utf-8").split("\n")
    nonblank = 0
    for n, line in enumerate(raw, start=1):
        if not line.strip():
            continue
        nonblank += 1
        if bad_path(line):
            keys.add(f"REVIEW-SCHEMA changed:{n}")
        elif line not in changed:
            changed.append(line)
    if nonblank == 0:
        keys.add("REVIEW-SCHEMA changed")
    try:
        m = tomllib.loads((design / "review/paths.toml").read_text(encoding="utf-8"))
        globs = m["review"] if set(m) == {"format", "review"} and m["format"] == "review-paths@1" else None
    except (tomllib.TOMLDecodeError, KeyError):
        globs = None
    if not globs:
        keys.add("REVIEW-SCHEMA review/paths.toml")
        globs = None
    try:
        rules = loads((design / "review/dress-rules.json").read_text(encoding="utf-8"))
        rule_ids = [r["id"] for r in rules] if isinstance(rules, list) and rules else None
    except ValueError:
        rule_ids = None
    if rule_ids is None:
        keys.add("REVIEW-SCHEMA review/dress-rules.json")
    known = {p for p in PRINCIPLES if f"## {p}:" in (design / "principles.md").read_text(encoding="utf-8")}

    def meta(p):
        return p.startswith("review/records/") or p.startswith("review/origin/") or p.endswith(".origin.json")

    needs = sorted(
        p
        for p in changed
        if not meta(p)
        and (
            p in ("review/paths.toml", "review/dress-rules.json")
            or globs is None
            or any(glob_hits(g.split("/"), p.split("/")) for g in globs)
        )
    )
    covered, records = set(), []
    for p in sorted(x for x in changed if x.startswith("review/records/")):
        f = design / p
        if not f.is_file():
            continue
        before = set(keys)
        try:
            r = loads(f.read_text(encoding="utf-8"))
        except ValueError:
            keys.add(f"REVIEW-SCHEMA {p}")
            continue
        if not isinstance(r, dict) or any_refused(r) or set(r) != {"pr", "artefacts", "principles", "dress", "checklist", "approver", "date"}:
            keys.add(f"REVIEW-SCHEMA {p}")
            continue
        for pid in r["principles"]:
            if pid not in known:
                keys.add(f"REVIEW-PRINCIPLE-UNKNOWN {pid}")
        if rule_ids is None:
            keys.add(f"REVIEW-SCHEMA {p}")
        else:
            ok, seen, failed = oracle_results(r["dress"], "rule", rule_ids)
            if not ok:
                keys.add(f"REVIEW-SCHEMA {p}")
            keys.update(f"REVIEW-RESULT-FAIL {x}" for x in failed)
            keys.update(f"REVIEW-DRESS-RESULT-MISSING {x}" for x in rule_ids if x not in seen)
        ok, seen, failed = oracle_results(r["checklist"], "item", CHECKLIST)
        if not ok:
            keys.add(f"REVIEW-SCHEMA {p}")
        keys.update(f"REVIEW-RESULT-FAIL {x}" for x in failed)
        keys.update(f"REVIEW-CHECKLIST-MISSING {x}" for x in CHECKLIST if x not in seen)
        ap = r["approver"]
        if ap.get("principal") != "human" or ap.get("name", "").lower().endswith("[bot]"):
            keys.add(f"REVIEW-APPROVER-NOT-HUMAN {p}")
        if not is_date(r["date"]):
            keys.add(f"REVIEW-SCHEMA {p}")
        valid = keys == before
        if valid:
            for a in r["artefacts"]:
                af = design / a
                if not af.exists():
                    if a not in changed:
                        keys.add(f"REVIEW-SCHEMA {p}")
                        valid = False
                    continue
                cands = [d for d in (design / (a + ".origin.json"), design / "review/origin" / (a + ".origin.json")) if d.is_file()]
                good = False
                if len(cands) == 2:
                    keys.add(f"REVIEW-SCHEMA {a}")
                elif len(cands) == 1:
                    good = oracle_decl(cands[0].read_text(encoding="utf-8"), a)
                    if not good:
                        keys.add(f"REVIEW-SCHEMA {cands[0].relative_to(design).as_posix()}")
                if not good:
                    keys.add(f"REVIEW-ORIGINALITY-MISSING {a}")
                    valid = False
        if valid:
            covered.update(r["artefacts"])
            records.append(p)
    for p in needs:
        if p not in covered:
            keys.add(f"REVIEW-RECORD-MISSING {p}")
    if deny_path is not None:
        names = []
        bad = False
        for line in deny_path.read_text(encoding="utf-8").split("\n"):
            t = line.strip()
            if not t or t.startswith("#"):
                continue
            if refused(t):
                bad = True
            else:
                names.append(t)
        if bad or not names:
            keys.add("REVIEW-SCHEMA deny-names")

        def key(s):
            n = unicodedata.normalize("NFKC", s)
            return "".join(ch for ch in n if unicodedata.category(ch) != "Cf").lower()

        for dirpath, _dirs, fnames in os.walk(design):
            for fn in fnames:
                full = pathlib.Path(dirpath) / fn
                rel = full.relative_to(design).as_posix()
                text = key(rel + "\n" + full.read_text(encoding="utf-8"))
                if any(key(n) in text for n in names):
                    keys.add(f"REVIEW-NAME-LEAK {rel}")
    return sorted(keys), needs, records


def oracle_check():
    errors = []
    for k, e in expected().items():
        deny = HERE / e["deny"] if e["deny"] else None
        keys, needs, recs = oracle(HERE / e["case"], deny)
        exit_code = 1 if keys else 0
        if keys != e["findings"] or needs != e["needs_review"] or recs != e["records"] or exit_code != e["exit"]:
            errors.append(f"{k}: oracle {exit_code} {keys} {needs} {recs} != label {e}")
    # The ACs of the leaf, by name.
    ac = {
        "ok": (0, None),
        "missing-record": (1, "REVIEW-RECORD-MISSING tokens/color.tokens.json"),
        "agent-approver": (1, "REVIEW-APPROVER-NOT-HUMAN"),
        "dress-gap": (1, "REVIEW-DRESS-RESULT-MISSING"),
        "no-decl": (1, "REVIEW-ORIGINALITY-MISSING"),
        "name-leak": (1, "REVIEW-NAME-LEAK"),
    }
    for k, (ex, code) in ac.items():
        e = expected()[k]
        if e["exit"] != ex or (code and not any(f.startswith(code) for f in e["findings"])):
            errors.append(f"AC label {k} does not match the leaf")
    rules = loads((HERE / "dress-gap/design/review/dress-rules.json").read_text(encoding="utf-8"))
    rec = loads((HERE / "dress-gap/design" / REC).read_text(encoding="utf-8"))
    if len(rules) != 5 or len(rec["dress"]) != 4:
        errors.append("dress-gap must have 5 rules and 4 results (AC-4)")
    return errors


def tree(root):
    out = set()
    for dirpath, _dirs, fnames in os.walk(root):
        for fn in fnames:
            out.add((pathlib.Path(dirpath) / fn).relative_to(root).as_posix())
    return out


def main():
    check = "--check" in sys.argv[1:]
    bad = []
    files = all_files()
    for name, text in files.items():
        p = HERE / name
        data = text.encode("utf-8")
        if check:
            if not p.exists() or p.read_bytes() != data:
                bad.append(f"{name} differs from make_fixtures.py output")
        else:
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_bytes(data)
    extra = tree(HERE) - set(files) - {"make_fixtures.py"}
    extra = {x for x in extra if "__pycache__" not in x}
    bad += [f"{x} is not written by make_fixtures.py" for x in sorted(extra)]
    bad += oracle_check()
    for b in bad:
        print("FAIL:", b, file=sys.stderr)
    if bad:
        return 1
    print("ok" if check else f"wrote {len(files)} files")
    return 0


if __name__ == "__main__":
    sys.exit(main())
