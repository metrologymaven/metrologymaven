#!/usr/bin/env python3
"""Fixture generator and independent oracle for spec node UNC.ORS.TRACE (Metrology Maven).

CC-BY-4.0 project data. Every case is synthetic: no case interprets any standard, every
clause number is invented, and no standards text appears anywhere. Expected values are
closed-form synthetic numbers; each dataset's test uncertainty is the binary64
representation bound of its value (half an ulp), never a measured or claimed uncertainty.

Writes (or, with --check, regenerates and compares byte for byte):
  dmt_stub.json, dmt_stub.digest   4 rows copied from fixtures/gdt/dmt/synthetic_table_v1.json
                                   (CC0, synthetic); digest = SHA-256 of the RFC 8785 form,
                                   computed here
  good_suite/suite.json, good_suite/cases/*.json   AC-1: 6 cases, zero findings
  bad/<name>.json                  AC-2: one fault each (bundle: {manifest, cases})
  versioning/<name>.json           AC-3: changed expected value, with and without a bump
  mutants.json                     AC-H1: 100 seeded mutants of the good suite
  expected.json                    every bundle's findings and coverage, and the AC-4 listing,
                                   all computed by the oracle below from the leaf's rules

The oracle (check_suite) is written from spec/unc/ors/trace/NODE.md ("Pinned at build"),
separately from crates/fid-ors. It does not re-validate rdl-dataset/v1 records, which the
generator builds valid; mutants only change dataset fields whose effect the pin states.

Run (stdlib only, Python 3.9+):
  python fixtures/ors/trace/make_fixtures.py           # rewrite
  python fixtures/ors/trace/make_fixtures.py --check   # fail if anything is stale
"""

import copy
import hashlib
import json
import math
import re
import struct
import sys
import unicodedata
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent.parent
SOURCE_TABLE = ROOT / "fixtures" / "gdt" / "dmt" / "synthetic_table_v1.json"

SUITE = "ors-trace-good"
ROWS = ["dmt:asme/cylindricity/none@1", "dmt:asme/flatness/none@1", "dmt:asme/position/none@1", "dmt:iso/flatness/none@1"]
OPEN = {"CC0-1.0", "CC-BY-4.0", "CC-BY-SA-4.0", "MIT", "Apache-2.0", "BSD-3-Clause", "ODbL-1.0", "PDDL-1.0"}
UNITS = {"m", "mm", "µm", "rad", "deg", "1"}
DIALECTS = {"ASME", "ISO_GPS", "none"}
STATUSES = {"draft", "in_review", "reviewed"}
MAX_TEXT = 280
PROVENANCE = ("Synthetic fixture for the UNC.ORS.TRACE checker tests; not a measurement and not an "
              "interpretation of any standard.")


def hx(x):
    return "0x%016x" % struct.unpack("<Q", struct.pack("<d", x))[0]


def jcs(v):
    """RFC 8785 for the values used here: objects, arrays, strings, booleans, null."""
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def sha(text):
    return "sha256:" + hashlib.sha256(text.encode("utf-8")).hexdigest()


# ------------------------------------------------------------------ the DMT stub

def dmt_stub():
    src = json.loads(SOURCE_TABLE.read_text(encoding="utf-8"))
    rows = [copy.deepcopy(r) for r in src["rows"] if r["key"] in ROWS]
    assert [r["key"] for r in rows] == sorted(ROWS)
    for r in rows:
        r["ors_cases"] = []
    table = {"schema": "fid.dmt-table/1", "version": "1.0.0", "rows": sorted(rows, key=lambda r: r["key"])}
    canonical = jcs({"rows": table["rows"], "schema": table["schema"], "version": table["version"]})
    return table, sha(canonical)


# ------------------------------------------------------------------ the good suite

def dataset(case, nnnn, value):
    aim = "gdt.synthetic.closed_form"
    return {
        "schema": "rdl-dataset/v1",
        "id": f"rdl:O/{aim}/{case}/{nnnn}",
        "tier": "O",
        "aim": aim,
        "inputs": [{"digest": sha(f"synthetic input {case}"), "unit": "mm", "role": "points"}],
        "reference": {
            "outputs": [{"name": "value", "kind": "length", "value_hex": hx(value),
                         "test_uncertainty_hex": hx(math.ulp(value) / 2)}],
            "precision_digits": 17,
        },
        "cross_check": [],
        "provenance": {"source": PROVENANCE},
        "licence": "CC-BY-4.0",
        "visibility": "public",
    }


def case(name, n, value, row=None, dialect="ASME"):
    c = {
        "schema": "ors-case/v1",
        "case": f"ors:trace/{name}.{n:03d}",
        "dataset": dataset(name.replace("_", "-"), f"{n:04d}", value),
        "dialect": dialect,
        "expected": {"value_hex": hx(value), "unit": "m"},
        "acceptance_tolerance": {"value_hex": hx(1e-9), "unit": "m"},
        "note": "Synthetic case; the clause number is invented for checker tests.",
    }
    if row is None:
        c["citations"] = "none"
        c["basis"] = "closed_form_math"
        c["dialect"] = "none"
        c["review"] = {"authors": ["case-author-a"], "reviewers": [], "status": "draft"}
    else:
        c["citations"] = [{"designator": "asme-y14.5", "edition": "2018", "clause": "99.1"}]
        c["dmt_row"] = row
        c["review"] = {"authors": ["case-author-a"], "reviewers": ["reviewer-one", "reviewer-two"], "status": "reviewed"}
    return c


def good_suite(digest):
    cases = [
        case("cylindricity", 1, 0.004, "dmt:asme/cylindricity/none@1"),
        case("flatness", 1, 0.002, "dmt:asme/flatness/none@1"),
        case("flatness", 2, 0.0015, "dmt:asme/flatness/none@1"),
        case("position", 1, 0.0125, "dmt:asme/position/none@1"),
        case("sphere_radius", 1, 0.025),
        case("circle_area", 1, 0.000314),
    ]
    cases.sort(key=lambda c: c["case"])
    manifest = {
        "schema": "ors-suite/v1",
        "name": SUITE,
        "version": "1.0.0",
        "licence": "CC-BY-4.0",
        "dmt_digest": digest,
        "cases": [c["case"] for c in cases],
        "changelog": [{"version": "1.0.0", "cases": [], "reason": "First release of the synthetic checker suite."}],
    }
    return {"manifest": manifest, "cases": cases}


# ------------------------------------------------------------------ the oracle

REVIEWER = re.compile(r"[a-z0-9][a-z0-9._-]{0,63}\Z")
HEX = re.compile(r"0x[0-9a-f]{16}\Z")
LABEL = re.compile(r"[a-z0-9](?:[a-z0-9]|[.-](?=[a-z0-9]))*\Z")
CLAUSE = re.compile(r"[a-z0-9]+(?:\.[a-z0-9]+)*\Z")
DMT_KEY = re.compile(r"dmt:(asme|iso)/[a-z][a-z0-9_]*/(none|[a-z0-9_]+(\+[a-z0-9_]+)*)@[1-9][0-9]*\Z")
CASE_KEYS = {"schema", "case", "dataset", "citations", "basis", "dmt_row", "dialect", "expected",
             "acceptance_tolerance", "review", "note"}


def label(s, n):
    return isinstance(s, str) and 0 < len(s) <= n and LABEL.match(s) is not None


def bad_text(s):
    return any(unicodedata.category(ch) in ("Cc", "Cf") for ch in s)


def hex_ok(s, nonneg=False):
    if not isinstance(s, str) or not HEX.match(s):
        return False
    b = int(s[2:], 16)
    if (b >> 52) & 0x7FF == 0x7FF:
        return False
    return not (nonneg and b >> 63)


def value_field(c, name, out, nonneg):
    if name not in c:
        out.append(("case_incomplete", name))
        return
    v = c[name]
    if not isinstance(v, dict):
        out.append(("case_invalid", name))
        return
    if "value_hex" not in v:
        out.append(("case_incomplete", name + ".value_hex"))
    elif not hex_ok(v["value_hex"], nonneg):
        out.append(("case_invalid", name + ".value_hex"))
    if "unit" not in v:
        out.append(("case_incomplete", name + ".unit"))
    elif v["unit"] not in UNITS:
        out.append(("case_invalid", name + ".unit"))
    if set(v) - {"value_hex", "unit"}:
        out.append(("case_invalid", name))


def id_list(v):
    return isinstance(v, list) and all(isinstance(x, str) and REVIEWER.match(x) for x in v) and len(set(v)) == len(v)


def check_case(c, rows):
    """[(code, field)] and the coverage class of one case, in the pinned order."""
    out = []
    if c.get("schema") != "ors-case/v1":
        return [("case_invalid", "schema")], "neither"
    # a. dataset
    d = c.get("dataset")
    if d is None:
        out.append(("case_incomplete", "dataset"))
    elif d.get("tier") != "O":
        out.append(("case_invalid", "dataset.tier"))
    else:
        if d.get("licence") not in OPEN:
            out.append(("licence_not_open", "dataset.licence"))
        for f in ("source", "method"):
            t = d.get("provenance", {}).get(f)
            if isinstance(t, str) and len(t) > MAX_TEXT:
                out.append(("text_too_long", "dataset.provenance." + f))
    # b. citations and basis
    cited = False
    closed = False
    if "citations" not in c:
        out.append(("citation_missing", "citations"))
    elif c["citations"] == "none":
        if c.get("basis") != "closed_form_math":
            out.append(("citation_missing", "basis"))
        else:
            closed = True
    elif isinstance(c["citations"], list):
        cs = c["citations"]
        if not cs:
            out.append(("citation_missing", "citations"))
        elif len(cs) > 16:
            out.append(("case_invalid", "citations"))
        else:
            for i, it in enumerate(cs):
                ok = (isinstance(it, dict) and set(it) == {"designator", "edition", "clause"}
                      and label(it["designator"], 64) and label(it["edition"], 32)
                      and isinstance(it["clause"], str) and len(it["clause"]) <= 32 and CLAUSE.match(it["clause"]))
                if not ok:
                    out.append(("case_invalid", f"citations.{i}"))
                    break
            else:
                cited = True
        if "basis" in c:
            out.append(("case_invalid", "basis"))
    else:
        out.append(("case_invalid", "citations"))
    # c. DMT row
    resolved = False
    has_row = "dmt_row" in c
    if has_row:
        r = c["dmt_row"]
        if not isinstance(r, str) or not DMT_KEY.match(r) or len(r) > 128:
            out.append(("case_invalid", "dmt_row"))
        elif rows is None or r not in rows:
            out.append(("dmt_row_unknown", "dmt_row"))
        else:
            resolved = True
    # d. dialect
    if "dialect" not in c:
        out.append(("case_incomplete", "dialect"))
    elif c["dialect"] not in DIALECTS:
        out.append(("case_invalid", "dialect"))
    # e, f. expected and tolerance
    value_field(c, "expected", out, False)
    value_field(c, "acceptance_tolerance", out, True)
    # g. review
    rv = c.get("review")
    if rv is None:
        out.append(("case_incomplete", "review"))
    elif not isinstance(rv, dict):
        out.append(("case_invalid", "review"))
    else:
        n0 = len(out)
        a = rv.get("authors")
        if a is None or a == []:
            out.append(("case_incomplete", "review.authors"))
        elif not id_list(a):
            out.append(("case_invalid", "review.authors"))
        rs = rv.get("reviewers")
        if rs is None:
            out.append(("case_incomplete", "review.reviewers"))
        elif not id_list(rs):
            out.append(("case_invalid", "review.reviewers"))
        st = rv.get("status")
        if st is None:
            out.append(("case_incomplete", "review.status"))
        elif st not in STATUSES:
            out.append(("case_invalid", "review.status"))
        if set(rv) - {"authors", "reviewers", "status"}:
            out.append(("case_invalid", "review"))
        if has_row and len(out) == n0:
            if st != "reviewed":
                out.append(("review_missing", "review.status"))
            elif any(x in a for x in rs):
                out.append(("review_missing", "review.reviewers"))
            elif len(rs) < 2:
                out.append(("review_missing", "review.reviewers"))
    # h. note
    if "note" in c:
        t = c["note"]
        if not isinstance(t, str):
            out.append(("case_invalid", "note"))
        elif len(t) > MAX_TEXT:
            out.append(("text_too_long", "note"))
        elif bad_text(t):
            out.append(("case_invalid", "note"))
    # i. unknown keys
    if set(c) - CASE_KEYS:
        out.append(("case_invalid", "case"))
    if closed:
        cov = "closed_form_math"
    elif cited and resolved:
        cov = "dmt_row"
    else:
        cov = "neither"
    return out, cov


def semver(s):
    return tuple(int(x) for x in s.split("."))


def check_suite(bundle, table, previous=None):
    rows = None
    m = bundle["manifest"]
    if table is not None and table[1] == m["dmt_digest"]:
        rows = {r["key"] for r in table[0]["rows"]}
    findings = []
    if m.get("licence", "CC-BY-4.0") not in OPEN:
        findings.append({"case": None, "code": "licence_not_open", "field": "licence"})
    for i, e in enumerate(m["changelog"]):
        if len(e["reason"]) > MAX_TEXT:
            findings.append({"case": None, "code": "text_too_long", "field": f"changelog.{i}.reason"})
    cov = {"closed_form_math": 0, "dmt_row": 0, "neither": 0}
    by_id = {c["case"]: c for c in bundle["cases"]}
    for cid in m["cases"]:
        fs, cl = check_case(by_id[cid], rows)
        cov[cl] += 1
        findings += [{"case": cid, "code": code, "field": f} for code, f in fs]
    if previous is not None:
        pm = previous["manifest"]
        prev = {c["case"]: c for c in previous["cases"]}
        for cid in m["cases"]:
            if cid not in prev:
                continue
            a, b = by_id[cid], prev[cid]
            if jcs(a.get("expected")) == jcs(b.get("expected")) and jcs(a.get("acceptance_tolerance")) == jcs(
                    b.get("acceptance_tolerance")):
                continue
            if semver(m["version"]) <= semver(pm["version"]):
                findings.append({"case": cid, "code": "version_not_bumped", "field": "version"})
            elif not any(e["version"] == m["version"] and cid in e["cases"] and e["reason"] for e in m["changelog"]):
                findings.append({"case": cid, "code": "version_not_bumped", "field": "changelog"})
    return {"cases": len(m["cases"]), "coverage": cov, "findings": findings, "passed": not findings}


# ------------------------------------------------------------------ variants

def find(bundle, prefix):
    return next(c for c in bundle["cases"] if c["case"].startswith(prefix))


def bad_variants(good):
    out = {}

    def var(name, fn):
        b = copy.deepcopy(good)
        fn(b)
        out[name] = b

    var("no_citation", lambda b: find(b, "ors:trace/flatness.001").pop("citations"))
    var("unknown_dmt_row", lambda b: find(b, "ors:trace/flatness.002").__setitem__("dmt_row", "dmt:asme/straightness/none@1"))
    var("missing_tolerance", lambda b: find(b, "ors:trace/position").pop("acceptance_tolerance"))
    var("long_note", lambda b: find(b, "ors:trace/circle").__setitem__("note", "n" * 600))
    var("one_reviewer", lambda b: find(b, "ors:trace/cylindricity")["review"].__setitem__("reviewers", ["reviewer-one"]))
    var("author_as_reviewer",
        lambda b: find(b, "ors:trace/position")["review"].__setitem__("reviewers", ["case-author-a", "reviewer-two"]))
    var("licence_proprietary", lambda b: b["manifest"].__setitem__("licence", "proprietary"))
    return out


def versioning(good):
    changed = copy.deepcopy(good)
    find(changed, "ors:trace/flatness.001")["expected"]["value_hex"] = hx(0.0021)
    bumped = copy.deepcopy(changed)
    bumped["manifest"]["version"] = "1.1.0"
    no_entry = copy.deepcopy(bumped)
    bumped["manifest"]["changelog"].append(
        {"version": "1.1.0", "cases": ["ors:trace/flatness.001"], "reason": "Synthetic expected value corrected."})
    tol = copy.deepcopy(good)
    find(tol, "ors:trace/position")["acceptance_tolerance"]["value_hex"] = hx(2e-9)
    return {"changed_same_version": changed, "bumped_with_changelog": bumped, "bumped_without_changelog": no_entry,
            "tolerance_same_version": tol}


class SplitMix:
    def __init__(self, seed):
        self.s = seed

    def next(self):
        m = (1 << 64) - 1
        self.s = (self.s + 0x9E3779B97F4A7C15) & m
        z = self.s
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & m
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & m
        return z ^ (z >> 31)

    def below(self, n):
        return self.next() % n


def mutations():
    """(name, applies to a case with a DMT row?, fn(case, manifest)). None: any case."""
    def drop(*path):
        def f(c, m):
            t = c
            for p in path[:-1]:
                t = t[p]
            t.pop(path[-1], None)
        return f

    def put(value, *path):
        def f(c, m):
            t = c
            for p in path[:-1]:
                t = t[p]
            t[path[-1]] = copy.deepcopy(value)
        return f

    def licence_ref(c, m):
        c["dataset"]["licence"] = "LicenseRef-closed"
        c["dataset"]["visibility"] = "reference_only"

    return [
        ("drop_citations", None, drop("citations")),
        ("empty_citations", True, put([], "citations")),
        ("bad_clause", True, lambda c, m: c["citations"][0].__setitem__("clause", "Clause 5")),
        ("none_without_basis", False, drop("basis")),
        ("wrong_basis", False, put("expert_opinion", "basis")),
        ("unknown_row", True, put("dmt:asme/straightness/none@1", "dmt_row")),
        ("bad_row_grammar", True, put("DMT:asme", "dmt_row")),
        ("drop_expected", None, drop("expected")),
        ("drop_expected_unit", None, drop("expected", "unit")),
        ("drop_tolerance_value", None, drop("acceptance_tolerance", "value_hex")),
        ("negative_tolerance", None, put(hx(-1e-9), "acceptance_tolerance", "value_hex")),
        ("drop_dialect", None, drop("dialect")),
        ("bad_dialect", None, put("DIN", "dialect")),
        ("note_281", None, put("x" * 281, "note")),
        ("note_280_benign", None, put("x" * 280, "note")),
        ("long_provenance", None, put("p" * 281, "dataset", "provenance", "source")),
        ("in_review", True, put("in_review", "review", "status")),
        ("in_review_benign", False, put("in_review", "review", "status")),
        ("one_reviewer", True, put(["reviewer-one"], "review", "reviewers")),
        ("author_reviews", True, put(["reviewer-one", "case-author-a"], "review", "reviewers")),
        ("duplicate_reviewer", True, put(["reviewer-one", "reviewer-one"], "review", "reviewers")),
        ("uppercase_reviewer", True, put(["Reviewer-One", "reviewer-two"], "review", "reviewers")),
        ("drop_authors", None, drop("review", "authors")),
        ("dataset_licence", None, licence_ref),
        ("manifest_licence", None, lambda c, m: m.__setitem__("licence", "LicenseRef-closed")),
        ("long_changelog", None, lambda c, m: m["changelog"][0].__setitem__("reason", "r" * 300)),
        ("extra_key", None, put("x", "comment")),
        ("changed_expected_vs_previous", None, put(hx(0.5), "expected", "value_hex")),
    ]


def mutants(good, table):
    g = SplitMix(0x6F72_732E_7472_6163)  # "ors.trac"
    muts = mutations()
    out = []
    for i in range(100):
        name, wants_row, fn = muts[i] if i < len(muts) else muts[g.below(len(muts))]
        pool = [c for c in good["cases"] if wants_row is None or ("dmt_row" in c) == wants_row]
        target = pool[g.below(len(pool))]["case"]
        b = copy.deepcopy(good)
        fn(next(c for c in b["cases"] if c["case"] == target), b["manifest"])
        against_previous = name.endswith("_vs_previous")
        res = check_suite(b, table, previous=good if against_previous else None)
        out.append({"name": f"{i:03d}-{name}", "target": target, "benign": name.endswith("_benign"),
                    "previous": against_previous, "suite": b, "findings": res["findings"]})
    return out


# ------------------------------------------------------------------ output

def dumps(v):
    return json.dumps(v, indent=1, sort_keys=True, ensure_ascii=False) + "\n"


def files():
    table, digest = dmt_stub()
    good = good_suite(digest)
    out = {"dmt_stub.json": dumps(table), "dmt_stub.digest": digest + "\n"}
    out["good_suite/suite.json"] = dumps(good["manifest"])
    for c in good["cases"]:
        out[f"good_suite/cases/{c['case'].split('/')[1]}.json"] = dumps(c)
    tab = (table, digest)
    expected = {"good_suite": check_suite(good, tab), "bad": {}, "versioning": {}}
    for name, b in bad_variants(good).items():
        out[f"bad/{name}.json"] = dumps(b)
        expected["bad"][name] = check_suite(b, tab)
    for name, b in versioning(good).items():
        out[f"versioning/{name}.json"] = dumps(b)
        expected["versioning"][name] = check_suite(b, tab, previous=good)
    expected["list_suite"] = [{"cases": len(good["cases"]), "dmt_digest": digest, "licence": "CC-BY-4.0",
                               "name": SUITE, "version": "1.0.0"}]
    ms = mutants(good, tab)
    assert expected["good_suite"]["passed"]
    assert all(m["findings"] or m["benign"] for m in ms), "a mutant seeds no fault"
    assert {f["code"] for m in ms for f in m["findings"]} >= {
        "citation_missing", "dmt_row_unknown", "case_incomplete", "text_too_long", "review_missing",
        "licence_not_open", "case_invalid", "version_not_bumped"}
    out["mutants.json"] = dumps({"about": "AC-H1 mutants of good_suite v1 (seeded). CC-BY-4.0.", "mutants": ms})
    out["expected.json"] = dumps(expected)
    return out


def main():
    check = "--check" in sys.argv[1:]
    stale = []
    for rel, text in sorted(files().items()):
        p = HERE / rel
        data = text.encode("utf-8")
        if check:
            if not p.exists() or p.read_bytes() != data:
                stale.append(rel)
        else:
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_bytes(data)
    if stale:
        print("stale: " + ", ".join(stale))
        return 1
    print("ok" if check else "written")
    return 0


if __name__ == "__main__":
    sys.exit(main())
