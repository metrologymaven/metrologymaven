#!/usr/bin/env python3
"""Independent reference for TRUST.REPLAY.COMPARE (CC-BY-4.0 project data).

Writes:
- fixtures/replay/compare/rep1-test.json: a REP-1 table (schema rep-table@1) that is TEST DATA
  ONLY (version `test-only.rep@0.0.1`): its bounds differ from rep-1@1.0.0 so the tests show
  the comparator reads the file, and it has no `angle` row so NO_TOLERANCE can be exercised;
- fixtures/replay/compare/rep1-test.sha256: `sha256:<hex>` of that file;
- fixtures/replay/compare/cases.json: packaged vs recomputed result pairs and package outcome
  lists, each with a hand-stated expected outcome, first differing field and reason;
- fixtures/replay/compare/package.sha256: `sha256:<hex>` of the canonical JSON (RFC 8785) of the
  package outcome for one package holding every case whose context is the default
  (same_manifest and same_platform_class true), in cases.json order, record ID = case name,
  compared with rep1-test.json. It is built here from the hand labels, not by the Rust code.

Each hand label is cross-checked before anything is written against a second statement of the
rules below (exact rationals via fractions.Fraction), independent of the Rust code.

Run:  python fixtures/replay/compare/make_cases.py           (write)
      python fixtures/replay/compare/make_cases.py --check   (fail if any file is stale)
"""

import hashlib
import json
import math
import struct
import sys
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent
TABLE_PATH = HERE / "rep1-test.json"
DIGEST_PATH = HERE / "rep1-test.sha256"
CASES_PATH = HERE / "cases.json"
PACKAGE_DIGEST_PATH = HERE / "package.sha256"

# kind -> (mode, bound text, unit). No `angle` row on purpose.
BOUNDS = {
    "length": ("abs", "1e-6", "m"),
    "dimensionless": ("rel", "1e-6", "1"),
    "expanded_uncertainty": ("rel", "1e-3", "1"),
    "p_conformance": ("abs", "1e-4", "1"),
}
KIND_ORDER = ["length", "angle", "dimensionless", "expanded_uncertainty", "p_conformance"]
TABLE = {
    "algorithm": "rdl.rep.classify@1.0.0",
    "bounds": [{"bound": BOUNDS[k][1], "kind": k, "mode": BOUNDS[k][0], "unit": BOUNDS[k][2]}
               for k in KIND_ORDER if k in BOUNDS] + [{"kind": "exact", "mode": "exact"}],
    "schema": "rep-table@1",
    "version": "test-only.rep@0.0.1",
}
TABLE_TEXT = json.dumps(TABLE, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
KNOWN_KINDS = set(KIND_ORDER) | {"exact"}

up = lambda x: math.nextafter(x, math.inf)
FIELDS = ["value", "uncertainty.status", "uncertainty.value", "decision.outcome", "decision.rule_id", "status"]
RANK = {"IDENTICAL": 0, "EQUIVALENT": 1, "NOT_REPLAYABLE": 2, "DIVERGENT": 3}


def hx(x):
    return None if x is None else struct.pack(">d", x).hex()


def bits(x):
    return struct.unpack(">Q", struct.pack(">d", x))[0]


# ---------------------------------------------------------------- independent reference rules


def bit_exact(det, ctx):
    return det == "D0" or (det in ("D1", "D2") and ctx["same_manifest"] and ctx["same_platform_class"])


def numeric_field(kind, a, b, det, ctx):
    """(outcome, reason) of one numeric field."""
    if any(v is not None and math.isnan(v) for v in (a, b)):
        return "DIVERGENT", "NAN_INPUT"
    if det == "N":
        return "NOT_REPLAYABLE", "CLASS_N"
    if a is None and b is None:
        return "IDENTICAL", None
    if a is None or b is None:
        return "DIVERGENT", "FIELD_CHANGED"
    if bits(a) == bits(b):
        return "IDENTICAL", None
    if kind == "exact":
        return "DIVERGENT", "FIELD_CHANGED"
    if bit_exact(det, ctx):
        return "DIVERGENT", "BIT_EXACT"
    if kind not in BOUNDS:
        return "DIVERGENT", "NO_TOLERANCE"
    if not (math.isfinite(a) and math.isfinite(b)):
        return "DIVERGENT", "OUTSIDE_TOLERANCE"
    mode, d, _ = BOUNDS[kind]
    scale = Fraction(1) if mode == "abs" else max(abs(Fraction(a)), abs(Fraction(b)))
    if abs(Fraction(a) - Fraction(b)) <= Fraction(d) * scale:
        return "EQUIVALENT", None
    return "DIVERGENT", "OUTSIDE_TOLERANCE"


def exact_field(a, b):
    return ("IDENTICAL", None) if a == b else ("DIVERGENT", "FIELD_CHANGED")


def marginal(kind, a, b, det, limit):
    if limit is None or det == "N" or kind not in BOUNDS:
        return False
    vals = [v for v in (a, b) if v is not None and math.isfinite(v)]
    if not vals or any(v is not None and math.isnan(v) for v in (a, b)):
        return False
    mode, d, _ = BOUNDS[kind]
    scale = Fraction(1) if mode == "abs" else max(abs(Fraction(v)) for v in vals)
    return any(abs(Fraction(v) - Fraction(limit)) <= Fraction(d) * scale for v in vals)


def ref_result(c):
    p, r, det, ctx, kind = c["packaged"], c["recomputed"], c["determinism"], c["context"], c["quantity_kind"]
    val = lambda side, key: None if side[key] is None else struct.unpack(">d", bytes.fromhex(side[key]))[0]
    dec = lambda side, key: None if side["decision"] is None else side["decision"][key]
    fields = [
        numeric_field(kind, val(p, "value"), val(r, "value"), det, ctx),
        exact_field(p["uec"], r["uec"]),
        numeric_field("expanded_uncertainty", val(p, "u"), val(r, "u"), det, ctx),
        exact_field(dec(p, "outcome") if p["decision"] else None, dec(r, "outcome") if r["decision"] else None),
        exact_field(dec(p, "rule_id") if p["decision"] else None, dec(r, "rule_id") if r["decision"] else None),
        exact_field(p["status"], r["status"]),
    ]
    outcome = max((f[0] for f in fields), key=RANK.get)
    first, reason = None, None
    if outcome != "IDENTICAL":
        i = next(i for i, f in enumerate(fields) if f[0] == outcome)
        first, reason = FIELDS[i], fields[i][1]
    m = marginal(kind, val(p, "value"), val(r, "value"), det, c.get("limit"))
    return {"outcome": outcome, "first_diff": first, "reason": reason, "marginal": m}


# ---------------------------------------------------------------- hand-labelled cases

cases = []


def side(value, u=None, uec=None, outcome="CONFORMS", rule="dr.simple@1", status="MEASURED", decision=True):
    if uec is None:
        uec = 0 if u is None else 2
    return {"value": hx(value), "uec": uec, "u": hx(u),
            "decision": {"outcome": outcome, "rule_id": rule} if decision else None, "status": status}


def case(name, det, kind, packaged, recomputed, outcome, first=None, reason=None, marginal_=False, limit=None,
         same_manifest=True, same_platform=True):
    c = {"name": name, "determinism": det, "quantity_kind": kind,
         "context": {"same_manifest": same_manifest, "same_platform_class": same_platform},
         "packaged": packaged, "recomputed": recomputed}
    if limit is not None:
        c["limit"] = limit
    want = {"outcome": outcome, "first_diff": first, "reason": reason, "marginal": marginal_}
    got = ref_result(c)
    assert got == want, (name, got, want)
    assert all(name != x["name"] for x in cases), name
    c["expect"] = want
    cases.append(c)


TOL = 1e-6  # test table length bound, metres

# AC-1 / REQ-001: bit-equal everything -> IDENTICAL.
for det in ["D0", "D1", "D2", "D3"]:
    case(f"identical_{det}", det, "length", side(0.0125, 2e-6), side(0.0125, 2e-6), "IDENTICAL")
case("identical_no_decision", "D3", "length", side(1.0, decision=False), side(1.0, decision=False), "IDENTICAL")
case("identical_uec0", "D0", "length", side(1.0), side(1.0), "IDENTICAL")

# AC-2 / REQ-002: D3 value off by half / twice the tolerance, same decision.
case("d3_half_tolerance", "D3", "length", side(0.01), side(0.01 + TOL / 2), "EQUIVALENT", "value")
case("d3_twice_tolerance", "D3", "length", side(0.01), side(0.01 + 2 * TOL), "DIVERGENT", "value",
     "OUTSIDE_TOLERANCE")
case("d3_half_tolerance_negative", "D3", "length", side(-0.5), side(-0.5 - TOL / 2), "EQUIVALENT", "value")
case("d3_dimensionless_within", "D3", "dimensionless", side(1000.0), side(1000.0005), "EQUIVALENT", "value")
case("d3_dimensionless_beyond", "D3", "dimensionless", side(1000.0), side(1000.002), "DIVERGENT", "value",
     "OUTSIDE_TOLERANCE")
case("d3_p_within", "D3", "p_conformance", side(0.95), side(0.95005), "EQUIVALENT", "value")
case("d3_u_within", "D3", "length", side(1.0, 2e-6), side(1.0, 2.001e-6), "EQUIVALENT", "uncertainty.value")
case("d3_u_beyond", "D3", "length", side(1.0, 2e-6), side(1.0, 2.01e-6), "DIVERGENT", "uncertainty.value",
     "OUTSIDE_TOLERANCE")
case("d3_value_and_u_within", "D3", "length", side(1.0, 2e-6), side(1.0 + TOL / 4, 2.001e-6), "EQUIVALENT",
     "value")
case("d1_other_platform_within", "D1", "length", side(0.2), side(0.2 + TOL / 2), "EQUIVALENT", "value",
     same_platform=False)
case("d2_other_manifest_within", "D2", "length", side(0.2), side(0.2 + TOL / 2), "EQUIVALENT", "value",
     same_manifest=False)
case("d1_same_manifest_platform_ulp", "D1", "length", side(0.2), side(up(0.2)), "DIVERGENT", "value", "BIT_EXACT")

# AC-3 / REQ-003: decision, rule or status changed -> DIVERGENT whatever the numbers.
case("d3_within_decision_changed", "D3", "length", side(0.01), side(0.01 + TOL / 2, outcome="INDETERMINATE"),
     "DIVERGENT", "decision.outcome", "FIELD_CHANGED")
case("d3_identical_decision_changed", "D3", "length", side(0.01), side(0.01, outcome="NONCONFORMS"),
     "DIVERGENT", "decision.outcome", "FIELD_CHANGED")
case("d3_rule_changed", "D3", "length", side(0.01), side(0.01, rule="dr.guard@1"), "DIVERGENT", "decision.rule_id",
     "FIELD_CHANGED")
case("d3_status_changed", "D3", "length", side(0.01), side(0.01, status="NOT_MEASURED"), "DIVERGENT", "status",
     "FIELD_CHANGED")
case("d3_decision_dropped", "D3", "length", side(0.01), side(0.01, decision=False), "DIVERGENT", "decision.outcome",
     "FIELD_CHANGED")
case("d3_uec_changed", "D3", "length", side(0.01, 2e-6, uec=2), side(0.01, 2e-6, uec=3), "DIVERGENT",
     "uncertainty.status", "FIELD_CHANGED")
case("d3_beyond_and_decision_changed", "D3", "length", side(0.01), side(0.02, outcome="NONCONFORMS"), "DIVERGENT",
     "value", "OUTSIDE_TOLERANCE")
case("d0_identical_value_status_changed", "D0", "length", side(3.0), side(3.0, status="FAILED"), "DIVERGENT",
     "status", "FIELD_CHANGED")
case("uec0_vs_evaluated", "D3", "length", side(0.01), side(0.01, 2e-6), "DIVERGENT", "uncertainty.status",
     "FIELD_CHANGED")
case("u_absent_one_side_same_uec", "D3", "length", side(0.01, None, uec=1), side(0.01, 2e-6, uec=1), "DIVERGENT",
     "uncertainty.value", "FIELD_CHANGED")

# AC-4 / REQ-004, REQ-005: D0 last bit; quantity kind missing from the table.
case("d0_last_bit", "D0", "length", side(0.0125), side(up(0.0125)), "DIVERGENT", "value", "BIT_EXACT")
case("d0_last_bit_other_manifest", "D0", "length", side(0.0125), side(up(0.0125)), "DIVERGENT", "value",
     "BIT_EXACT", same_manifest=False, same_platform=False)
case("d0_half_tolerance", "D0", "length", side(0.01), side(0.01 + TOL / 2), "DIVERGENT", "value", "BIT_EXACT")
case("d0_u_last_bit", "D0", "length", side(1.0, 2e-6), side(1.0, up(2e-6)), "DIVERGENT", "uncertainty.value",
     "BIT_EXACT")
case("missing_kind_angle", "D3", "angle", side(0.5), side(0.5 + 1e-12), "DIVERGENT", "value", "NO_TOLERANCE")
case("missing_kind_angle_identical", "D3", "angle", side(0.5), side(0.5), "IDENTICAL")
case("unknown_kind_temperature", "D3", "temperature", side(293.15), side(293.1500001), "DIVERGENT", "value",
     "NO_TOLERANCE")
case("unknown_kind_identical", "D3", "temperature", side(293.15), side(293.15), "IDENTICAL")
case("unknown_kind_d0", "D0", "temperature", side(293.15), side(up(293.15)), "DIVERGENT", "value", "BIT_EXACT")
case("exact_kind_count", "D3", "exact", side(12.0), side(13.0), "DIVERGENT", "value", "FIELD_CHANGED")

# Errors and fail-closed cases.
case("nan_recomputed", "D3", "length", side(1.0), side(math.nan), "DIVERGENT", "value", "NAN_INPUT")
case("nan_packaged_u", "D3", "length", side(1.0, 2e-6), side(1.0, math.nan), "DIVERGENT", "uncertainty.value",
     "NAN_INPUT")
case("value_absent_recomputed", "D3", "length", side(1.0), side(None), "DIVERGENT", "value", "FIELD_CHANGED")
case("inf_recomputed", "D3", "length", side(1.0), side(math.inf), "DIVERGENT", "value", "OUTSIDE_TOLERANCE")
case("class_n", "N", "length", side(1.0), side(2.0), "NOT_REPLAYABLE", "value", "CLASS_N")
case("class_n_decision_changed", "N", "length", side(1.0), side(1.0, outcome="NONCONFORMS"), "DIVERGENT",
     "decision.outcome", "FIELD_CHANGED")

# MARGINAL (informative, from rdl.rep.classify@1.0.0).
case("marginal_identical", "D3", "length", side(0.01), side(0.01), "IDENTICAL", marginal_=True, limit="0.0100005")
case("marginal_not_near", "D3", "length", side(0.01), side(0.01), "IDENTICAL", limit="0.0101")
case("marginal_equivalent", "D3", "length", side(0.01), side(0.01 + TOL / 2), "EQUIVALENT", "value",
     marginal_=True, limit="0.01")

# AC-5 / REQ-006: package aggregation, worst wins; empty is NOT_REPLAYABLE.
packages = [
    {"name": "identical_equivalent", "outcomes": ["IDENTICAL", "EQUIVALENT"], "expect": "EQUIVALENT"},
    {"name": "equivalent_not_replayable", "outcomes": ["EQUIVALENT", "NOT_REPLAYABLE"], "expect": "NOT_REPLAYABLE"},
    {"name": "not_replayable_divergent", "outcomes": ["NOT_REPLAYABLE", "DIVERGENT"], "expect": "DIVERGENT"},
    {"name": "empty", "outcomes": [], "expect": "NOT_REPLAYABLE"},
    {"name": "all_identical", "outcomes": ["IDENTICAL", "IDENTICAL"], "expect": "IDENTICAL"},
    {"name": "divergent_first", "outcomes": ["DIVERGENT", "IDENTICAL", "NOT_REPLAYABLE"], "expect": "DIVERGENT"},
]
for pk in packages:
    want = "NOT_REPLAYABLE" if not pk["outcomes"] else max(pk["outcomes"], key=RANK.get)
    assert want == pk["expect"], pk

DOC = {"note": "Synthetic TRUST.REPLAY.COMPARE cases for the TEST-ONLY table rep1-test.json; not production data.",
       "table": "rep1-test.json", "cases": cases, "packages": packages}
CASES_TEXT = json.dumps(DOC, indent=1, sort_keys=True) + "\n"
DIGEST_TEXT = "sha256:" + hashlib.sha256(TABLE_TEXT.encode("ascii")).hexdigest() + "\n"

# One package of every default-context case. Canonical JSON here has only strings, booleans,
# null, arrays and objects, so sorted keys, no whitespace and ASCII escapes are RFC 8785.
DEFAULT_CTX = {"same_manifest": True, "same_platform_class": True}
pkg_results = [{"first_diff": c["expect"]["first_diff"], "marginal": c["expect"]["marginal"],
                "outcome": c["expect"]["outcome"], "reason": c["expect"]["reason"], "record_id": c["name"]}
               for c in cases if c["context"] == DEFAULT_CTX]
assert len(pkg_results) >= 40, len(pkg_results)
PACKAGE = {
    "algorithm": "rdl.rep.classify@1.0.0",
    "outcome": max((r["outcome"] for r in pkg_results), key=RANK.get),
    "rep1_digest": DIGEST_TEXT.strip(),
    "rep1_production": False,
    "rep1_version": TABLE["version"],
    "results": pkg_results,
}
PACKAGE_TEXT = json.dumps(PACKAGE, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
PACKAGE_DIGEST_TEXT = "sha256:" + hashlib.sha256(PACKAGE_TEXT.encode("ascii")).hexdigest() + "\n"
OUTPUTS = [(TABLE_PATH, TABLE_TEXT), (DIGEST_PATH, DIGEST_TEXT), (CASES_PATH, CASES_TEXT),
           (PACKAGE_DIGEST_PATH, PACKAGE_DIGEST_TEXT)]


def main():
    if "--check" in sys.argv[1:]:
        stale = [str(p) for p, t in OUTPUTS if not p.exists() or p.read_bytes() != t.encode("ascii")]
        if stale:
            print("stale:", *stale)
            sys.exit(1)
        print(f"ok: {len(cases)} cases, {len(packages)} packages")
        return
    for p, t in OUTPUTS:
        p.write_bytes(t.encode("ascii"))
    print(f"wrote {len(cases)} cases, {len(packages)} packages")


main()
