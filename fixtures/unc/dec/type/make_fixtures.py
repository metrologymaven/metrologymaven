#!/usr/bin/env python3
"""Independent reference for UNC.DEC.TYPE fixtures (stdlib only; CC-BY-4.0 data).

Writes the fixtures of fixtures/unc/dec/type/ from the rules pinned in
spec/unc/dec/type/NODE.md (Design), independently of the Rust code (it never runs it):

catalog_v1.json     the built-in catalog dr-catalog@1 in RFC 8785 canonical form: members
                    sorted by key (all keys and strings are printable ASCII, so Python's
                    code-point order equals RFC 8785's UTF-16 order), no whitespace, no
                    escapes, no trailing newline; rules sorted by (id bytes, version)
catalog_v1.sha256   "sha256:<hex>" of those bytes, plus a newline
ids.csv             "input,expect,kind,params,version" per case: the rule-ID grammar run
                    by an independent regular-expression classifier, cross-checked against
                    the hand labels below
bad_customer.json   customer rules that each break one rule, with the hand-labelled error;
                    an independent validator below must agree with every label

Usage: make_fixtures.py [--check]   (--check compares with the files on disk, writes nothing)
"""

import hashlib
import json
import pathlib
import re
import sys
from fractions import Fraction

HERE = pathlib.Path(__file__).resolve().parent


def canon(obj):
    text = json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
    assert "\\" not in text, "the catalog never needs escapes"
    return text.encode("ascii")


# ------------------------------------------------------------------ catalog

JCGM_106 = "JCGM 106"
CATALOG_RULES = [
    {
        "id": "dr:iso14253-1:2017/default",
        "version": "1",
        "kind": "conformance_probability",
        "params": {"p_min": "0.95"},
        "standards_ref": {"designator": "ISO 14253-1", "edition": "2017", "clause": "5"},
        "pdf_source": "mc_or_validated_normal",
        "risk_reporting": "specific",
    },
    {
        "id": "dr:simple-acceptance",
        "version": "1",
        "kind": "simple_acceptance",
        "params": {},
        "standards_ref": {"designator": JCGM_106, "edition": "2012", "clause": "8.2"},
        "pdf_source": "mc_or_validated_normal",
        "risk_reporting": "specific",
    },
    {
        "id": "dr:guard-band",
        "version": "1",
        "kind": "guard_band",
        "params": {},
        "standards_ref": {"designator": JCGM_106, "edition": "2012", "clause": "8.3"},
        "pdf_source": "mc_or_validated_normal",
        "risk_reporting": "specific",
    },
    {
        "id": "dr:as-reported-by-source",
        "version": "1",
        "kind": "label_only",
        "params": {},
        "standards_ref": "none",
        "pdf_source": "none",
        "risk_reporting": "none",
    },
]


def catalog_bytes():
    rules = sorted(CATALOG_RULES, key=lambda r: (r["id"].encode("ascii"), int(r["version"])))
    return canon({"catalog": "dr-catalog@1", "rules": rules})


# ------------------------------------------------------------------ rule-ID grammar

VERSION = r"[1-9][0-9]{0,8}"
DEC = r"-?(?:0|[1-9][0-9]{0,19})(?:\.[0-9]{0,19}[1-9])?"
SEG = r"[a-z0-9][a-z0-9_-]{0,63}"
FIXED = {
    "dr:iso14253-1:2017/default": "conformance_probability",
    "dr:simple-acceptance": "simple_acceptance",
    "dr:guard-band": "guard_band",
    "dr:as-reported-by-source": "label_only",
}


def classify(s):
    """(expect, kind, params, version) of one rule reference."""
    bad = ("invalid_rule_id", "", "", "")
    if len(s.encode("utf-8")) > 256:
        return bad
    m = re.fullmatch(r"(.*)@(" + VERSION + r")", s, re.DOTALL)
    if not m:
        return bad
    ident, version = m.group(1), m.group(2)
    if ident in FIXED:
        return ("ok", FIXED[ident], "", version)
    g = re.fullmatch(r"dr:guard-band\(r=(" + DEC + r")\)", ident)
    if g and g.group(1) != "-0":
        if Fraction(g.group(1)) < 0:
            return ("invalid_rule:r", "", "", "")
        return ("ok", "guard_band", "r=" + g.group(1), version)
    c = re.fullmatch(r"dr:customer/(" + SEG + r")/(" + SEG + r")", ident)
    if c:
        return ("ok", "customer", "tenant=%s;id=%s" % (c.group(1), c.group(2)), version)
    return bad


S64 = "a" + "b" * 63
S65 = "a" + "b" * 64
ID_CASES = [
    # built-in forms and versions
    "dr:iso14253-1:2017/default@1",
    "dr:iso14253-1:2017/default@2",
    "dr:simple-acceptance@1",
    "dr:simple-acceptance@999999999",
    "dr:guard-band@1",
    "dr:as-reported-by-source@1",
    "dr:as-reported-by-source@17",
    # guard-band instances
    "dr:guard-band(r=0)@1",
    "dr:guard-band(r=0.5)@1",
    "dr:guard-band(r=2)@1",
    "dr:guard-band(r=1.25)@3",
    "dr:guard-band(r=12345678901234567890)@1",
    "dr:guard-band(r=0.00000000000000000001)@1",
    "dr:guard-band(r=10.01)@1",
    # customer rules
    "dr:customer/acme/bore-tight@1",
    "dr:customer/a/b@2",
    "dr:customer/t_1/rule_2-x@1",
    "dr:customer/0/9@1",
    "dr:customer/%s/%s@1" % (S64, S64),
    # negative guard-band factor: grammar ok, value refused
    "dr:guard-band(r=-1)@1",
    "dr:guard-band(r=-0.5)@1",
    # invalid: version
    "dr:simple-acceptance",
    "dr:simple-acceptance@",
    "dr:simple-acceptance@0",
    "dr:simple-acceptance@01",
    "dr:simple-acceptance@1000000000",
    "dr:simple-acceptance@-1",
    "dr:simple-acceptance@1.0",
    "dr:simple-acceptance@+1",
    "dr:simple-acceptance@@1",
    "dr:simple-acceptance@1@1",
    "dr:iso14253-1:2017/default@1x",
    "dr:guard-band(r=-1)@0",
    # invalid: id
    "",
    "@1",
    "dr:@1",
    "DR:simple-acceptance@1",
    "dr:Simple-acceptance@1",
    "dr:simple_acceptance@1",
    "dr:simple-acceptance @1",
    " dr:simple-acceptance@1",
    "dr:simple-acceptance@1 ",
    "dr:simple-acceptanc\u00e9@1",
    "dr:iso14253-1:2017@1",
    "dr:iso14253-1:2017/Default@1",
    "dr:guard-band()@1",
    "dr:guard-band(r=)@1",
    "dr:guard-band(r=.5)@1",
    "dr:guard-band(r=1.)@1",
    "dr:guard-band(r=0.50)@1",
    "dr:guard-band(r=0.0)@1",
    "dr:guard-band(r=01)@1",
    "dr:guard-band(r=+1)@1",
    "dr:guard-band(r=1e3)@1",
    "dr:guard-band(r=-0)@1",
    "dr:guard-band(r= 1)@1",
    "dr:guard-band(k=1)@1",
    "dr:guard-band(r=1)",
    "dr:guard-band(r=1)x@1",
    "dr:guard-band(r=123456789012345678901)@1",
    "dr:guard-band(r=0.000000000000000000001)@1",
    "dr:customer/acme@1",
    "dr:customer/acme/@1",
    "dr:customer//x@1",
    "dr:customer/Acme/x@1",
    "dr:customer/a/b/c@1",
    "dr:customer/-a/b@1",
    "dr:customer/_a/b@1",
    "dr:customer/a.b/c@1",
    "dr:customer/%s/x@1" % S65,
    "dr:customer/x/%s@1" % S65,
    "dr:" + "x" * 300 + "@1",
]

# Hand labels, from the grammar in NODE.md Design, for a subset of the cases.
HAND = {
    "dr:iso14253-1:2017/default@1": ("ok", "conformance_probability", "", "1"),
    "dr:simple-acceptance@999999999": ("ok", "simple_acceptance", "", "999999999"),
    "dr:guard-band@1": ("ok", "guard_band", "", "1"),
    "dr:as-reported-by-source@17": ("ok", "label_only", "", "17"),
    "dr:guard-band(r=1.25)@3": ("ok", "guard_band", "r=1.25", "3"),
    "dr:guard-band(r=0)@1": ("ok", "guard_band", "r=0", "1"),
    "dr:customer/acme/bore-tight@1": ("ok", "customer", "tenant=acme;id=bore-tight", "1"),
    "dr:guard-band(r=-1)@1": ("invalid_rule:r", "", "", ""),
    "dr:guard-band(r=-1)@0": ("invalid_rule_id", "", "", ""),
    "dr:simple-acceptance@0": ("invalid_rule_id", "", "", ""),
    "dr:simple-acceptance@01": ("invalid_rule_id", "", "", ""),
    "dr:simple-acceptance": ("invalid_rule_id", "", "", ""),
    "dr:guard-band(r=0.50)@1": ("invalid_rule_id", "", "", ""),
    "dr:guard-band(r=-0)@1": ("invalid_rule_id", "", "", ""),
    "dr:guard-band(r=1e3)@1": ("invalid_rule_id", "", "", ""),
    "dr:customer/a/b/c@1": ("invalid_rule_id", "", "", ""),
    "DR:simple-acceptance@1": ("invalid_rule_id", "", "", ""),
    "": ("invalid_rule_id", "", "", ""),
}


def ids_csv():
    lines = ["input,expect,kind,params,version"]
    seen = set()
    for s in ID_CASES:
        assert s not in seen, s
        seen.add(s)
        assert not any(c in s for c in ',"\r\n'), "cases must not need CSV quoting: %r" % s
        row = classify(s)
        if s in HAND:
            assert row == HAND[s], (s, row, HAND[s])
        lines.append(",".join((s,) + row))
    for s in HAND:
        assert s in seen, s
    return ("\n".join(lines) + "\n").encode("utf-8")


# ------------------------------------------------------------------ customer rules

DESIGNATOR = r"[A-Z](?:[A-Z0-9./-]| (?! ))*"
CLAUSE = r"(?:[1-9][0-9]{0,2}|[A-Z])(?:\.(?:0|[1-9][0-9]{0,2})){0,4}"
CANON_DEC = re.compile(DEC)
KINDS = {"conformance_probability", "simple_acceptance", "guard_band", "label_only"}
MEMBERS = {"id", "version", "kind", "params", "standards_ref", "pdf_source", "risk_reporting"}


def validate_customer(r):
    """Independent customer-rule validator: the first failing check, in the pinned order."""
    if set(r) != MEMBERS:
        return ("malformed", "-")
    ok, _, _, _ = classify(r["id"] + "@1") if isinstance(r["id"], str) else ("invalid_rule_id",) * 4
    if ok == "invalid_rule_id":
        return ("invalid_rule_id", "-")
    if ok != "ok":
        return ("invalid_rule", "r")
    if not r["id"].startswith("dr:customer/"):
        return ("invalid_rule", "id")
    if not (isinstance(r["version"], str) and re.fullmatch(VERSION, r["version"])):
        return ("invalid_rule", "version")
    kind = r["kind"]
    if kind not in KINDS or kind == "label_only":
        return ("invalid_rule", "kind")
    params = r["params"]
    if not isinstance(params, dict):
        return ("invalid_rule", "params")
    allowed = {"conformance_probability": {"p_min"}, "guard_band": {"r"}, "simple_acceptance": set()}[kind]
    if not set(params) <= allowed:
        return ("invalid_rule", "params")
    for name in sorted(allowed):
        v = params.get(name)
        if not isinstance(v, str) or not CANON_DEC.fullmatch(v) or v == "-0":
            return ("invalid_rule", name)
        x = Fraction(v)
        if name == "p_min" and not (Fraction(1, 2) < x < 1):
            return ("invalid_rule", name)
        if name == "r" and x < 0:
            return ("invalid_rule", name)
    sr = r["standards_ref"]
    if sr != "none":
        good = (
            isinstance(sr, dict)
            and set(sr) == {"designator", "edition", "clause"}
            and all(isinstance(v, str) for v in sr.values())
            and 2 <= len(sr["designator"]) <= 32
            and re.fullmatch(DESIGNATOR, sr["designator"])
            and not sr["designator"].endswith(" ")
            and re.fullmatch(r"[0-9]{4}", sr["edition"])
            and re.fullmatch(CLAUSE, sr["clause"])
        )
        if not good:
            return ("invalid_rule", "standards_ref")
    if r["pdf_source"] not in ("mc_or_validated_normal", "mc_only"):
        # "none" belongs to label_only only, which a customer rule never is.
        return ("invalid_rule", "pdf_source")
    if r["risk_reporting"] not in ("none", "specific", "specific_and_global"):
        return ("invalid_rule", "risk_reporting")
    return ("ok", "-")


def cust(kind, params, **kw):
    r = {
        "id": "dr:customer/acme/bore",
        "version": "1",
        "kind": kind,
        "params": params,
        "standards_ref": "none",
        "pdf_source": "mc_or_validated_normal",
        "risk_reporting": "specific",
    }
    r.update(kw)
    return r


# (name, rule, hand-labelled code, hand-labelled param)
BAD = [
    ("p_min_0_4", cust("conformance_probability", {"p_min": "0.4"}), "invalid_rule", "p_min"),
    ("r_minus_1", cust("guard_band", {"r": "-1"}), "invalid_rule", "r"),
    ("kind_label_only", cust("label_only", {}, pdf_source="none", risk_reporting="none"), "invalid_rule", "kind"),
    ("p_min_0_5_open", cust("conformance_probability", {"p_min": "0.5"}), "invalid_rule", "p_min"),
    ("p_min_1_open", cust("conformance_probability", {"p_min": "1"}), "invalid_rule", "p_min"),
    ("p_min_non_canonical", cust("conformance_probability", {"p_min": "0.950"}), "invalid_rule", "p_min"),
    ("p_min_missing", cust("conformance_probability", {}), "invalid_rule", "p_min"),
    ("kind_unknown", cust("bayesian", {}), "invalid_rule", "kind"),
    ("guard_band_r_missing", cust("guard_band", {}), "invalid_rule", "r"),
    ("guard_band_r_exponent", cust("guard_band", {"r": "1e-1"}), "invalid_rule", "r"),
    ("simple_with_param", cust("simple_acceptance", {"r": "1"}), "invalid_rule", "params"),
    ("probability_with_r", cust("conformance_probability", {"p_min": "0.9", "r": "1"}), "invalid_rule", "params"),
    ("builtin_id", cust("simple_acceptance", {}, id="dr:simple-acceptance"), "invalid_rule", "id"),
    ("version_zero", cust("simple_acceptance", {}, version="0"), "invalid_rule", "version"),
    ("pdf_source_none", cust("conformance_probability", {"p_min": "0.9"}, pdf_source="none"), "invalid_rule",
     "pdf_source"),
    ("risk_unknown", cust("simple_acceptance", {}, risk_reporting="all"), "invalid_rule", "risk_reporting"),
    ("standards_prose", cust("simple_acceptance", {}, standards_ref={
        "designator": "ISO 14253-1", "edition": "2017", "clause": "the measured value shall"}),
     "invalid_rule", "standards_ref"),
    ("standards_lowercase", cust("simple_acceptance", {}, standards_ref={
        "designator": "iso 14253-1", "edition": "2017", "clause": "5"}), "invalid_rule", "standards_ref"),
]


def bad_customer_json():
    cases = []
    for name, rule, code, param in BAD:
        got = validate_customer(rule)
        assert got == (code, param), (name, got, (code, param))
        cases.append({"name": name, "rule": rule, "expect": code, "param": param})
    text = json.dumps({"cases": cases}, sort_keys=True, indent=2, ensure_ascii=True)
    assert "\\" not in text
    return (text + "\n").encode("ascii")


def self_checks():
    # Valid customer rules pass the independent validator.
    assert validate_customer(cust("conformance_probability", {"p_min": "0.99"})) == ("ok", "-")
    assert validate_customer(cust("guard_band", {"r": "0"})) == ("ok", "-")
    assert validate_customer(cust("simple_acceptance", {}, standards_ref={
        "designator": "ASME B89.7.3.1", "edition": "2001", "clause": "4.2"})) == ("ok", "-")
    # Every catalog rule fits the designator, clause and decimal grammars used above.
    for r in CATALOG_RULES:
        sr = r["standards_ref"]
        if sr != "none":
            assert re.fullmatch(DESIGNATOR, sr["designator"]) and re.fullmatch(CLAUSE, sr["clause"])
        for v in r["params"].values():
            assert CANON_DEC.fullmatch(v)
    assert Fraction(1, 2) < Fraction("0.95") < 1


def outputs():
    self_checks()
    cat = catalog_bytes()
    return {
        "catalog_v1.json": cat,
        "catalog_v1.sha256": ("sha256:" + hashlib.sha256(cat).hexdigest() + "\n").encode("ascii"),
        "ids.csv": ids_csv(),
        "bad_customer.json": bad_customer_json(),
    }


def main(argv):
    check = "--check" in argv[1:]
    stale = []
    for name, data in outputs().items():
        path = HERE / name
        if check:
            if not path.exists() or path.read_bytes() != data:
                stale.append(name)
        else:
            path.write_bytes(data)
    if stale:
        print("stale fixtures: " + ", ".join(stale))
        return 1
    print("ok" if check else "written")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
