#!/usr/bin/env python3
"""Independent reference for UNC.UEC.STMT fixtures (stdlib only; CC-BY-4.0 data).

Writes the hand-written statements of fixtures/unc/stmt/ in the canonical form pinned in
spec/unc/uec/stmt/NODE.md (Design), independently of the Rust code:

- canonical JSON: members sorted by key (all keys and strings are printable ASCII, so
  Python's code-point order equals RFC 8785's UTF-16 order), no whitespace, no escapes,
  no trailing newline;
- every f64 is struct.pack('>d', x).hex() (16 lowercase hex digits of the bits);
- booleans and counts are strings ("true"/"false", plain decimal);
- digest = hashlib.sha256 of the bytes, written as sha256:<hex>.

valid/<name>.json        one statement per file; every class and every method occurs
valid/digests.txt        "<file> sha256:<hex>" per line, sorted by file name
invalid/<name>.json      each breaks exactly one rule (hand-labelled below)
invalid/expected.txt     "<file> <error code> <field or ->" per line

Usage: make_fixtures.py [--check]   (--check compares with the files on disk, writes nothing)
"""

import hashlib
import json
import pathlib
import struct
import sys

HERE = pathlib.Path(__file__).resolve().parent


def h(x):
    """IEEE-754 binary64 bit pattern, 16 lowercase hex digits."""
    return struct.pack(">d", x).hex()


def hb(bits):
    return "%016x" % bits


def d(label):
    return "sha256:" + hashlib.sha256(label.encode("ascii")).hexdigest()


def canon(obj):
    text = json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
    assert "\\" not in text, "canonical statements never need escapes"
    return text.encode("ascii")


UNIT = "si:m"  # closed set: si:m, si:rad, si:1
MODEL = {"id": "um:bore-diameter", "semver": "1.2.0", "digest": d("um:bore-diameter@1.2.0")}
PROV = {"rule_sets": ["uec-assign@1"], "proposal": "false"}


def src(kind, ident):
    return {"kind": kind, "id": ident, "digest": d("source:" + ident)}


def stmt(cls, **kw):
    s = {"schema": "uncertainty_statement@1", "class": cls, "sources": [], "contributors": [], "provenance": PROV}
    s.update(kw)
    s["sources"] = sorted(s["sources"], key=lambda x: x["id"])
    s["contributors"] = sorted(s["contributors"], key=lambda x: x["input_id"])
    return s


VALID = {
    "uec0_u_missing": stmt("UEC-0", uec0_reason="u_missing", unit=UNIT, model=MODEL),
    "uec0_model_missing": stmt("UEC-0", uec0_reason="model_missing", unit=UNIT),
    "uec0_instrument_missing": stmt(
        "UEC-0", uec0_reason="instrument_missing", model=MODEL, sources=[src("dcc", "dcc:ring-gauge-25")]
    ),
    "uec1_declared": stmt(
        "UEC-1",
        method="DECLARED",
        unit=UNIT,
        expanded={"U": h(4e-6), "k": h(2.0)},
        model={"id": "um:declared", "semver": "1.0.0", "digest": d("um:declared@1.0.0")},
        sources=[src("declared", "decl:2026-11-16-001")],
        provenance={"rule_sets": ["uec-assign@1"], "principal": "person:qa-lead", "principal_kind": "human",
                    "proposal": "false"},
    ),
    "uec1_spec_derived": stmt(
        "UEC-1",
        method="SPEC_DERIVED",
        unit=UNIT,
        interval={"low": h(-3e-6), "high": h(3e-6), "p": h(0.95), "kind": "prob_symmetric"},
        model={"id": "um:cmm-mpe", "semver": "0.3.1", "digest": d("um:cmm-mpe@0.3.1")},
        sources=[src("instrument_model", "inst:cmm-mpe-e0")],
        provenance={"rule_sets": ["uec-assign@1"], "principal": "agent:planner", "principal_kind": "agent",
                    "proposal": "true"},
    ),
    "uec1_lpu": stmt(
        "UEC-1",
        method="LPU",
        unit=UNIT,
        u_std=h(1.5e-6),
        expanded={"U": h(3e-6), "k": h(2.0)},
        model=MODEL,
        sources=[src("instrument_model", "inst:probe-a"), src("dcc", "dcc:ring-gauge-25")],
        contributors=[
            {"input_id": "probe_form", "sensitivity": h(1.0), "u_contrib": h(1.2e-6), "variance_share": h(0.64)},
            {"input_id": "temp", "sensitivity": h(-11.5e-6), "u_contrib": h(0.9e-6), "variance_share": h(0.36)},
        ],
        nonlinearity={"version": "nlin@1", "outcome": "linear_adequate", "v1": h(2.25e-12), "v2": h(1e-15),
                      "ratio": h(1e-15 / 2.25e-12), "probed_inputs": "2", "probed_pairs": "1",
                      "cross_terms_truncated": "false"},
        selection_rule="unc-select@1",
        estimate_convention="est-policy@1",
    ),
    "uec1_mcm": stmt(
        "UEC-1",
        method="MCM",
        unit=UNIT,
        expanded={"U": h(5.1e-6), "k": h(1.96)},
        interval={"low": h(0.024995), "high": h(0.0250052), "p": h(0.95), "kind": "shortest"},
        model=MODEL,
        sources=[src("declared", "decl:fixture-wear"), src("type_a", "ta:run-0042")],
        provenance={"rule_sets": ["est-policy@1", "uec-assign@1"], "principal": "agent:unc-helper",
                    "principal_kind": "agent", "proposal": "true", "adopted_by": "person:qa-lead",
                    "supersedes": d("statement:previous")},
    ),
    "uec2_lpu": stmt(
        "UEC-2",
        method="LPU",
        unit=UNIT,
        u_std=h(0.8e-6),
        expanded={"U": h(1.6e-6), "k": h(2.0)},
        model=MODEL,
        sources=[src("type_a", "ta:run-0042"), src("dcc", "dcc:ring-gauge-25")],
        contributors=[
            {"input_id": "repeat", "u_contrib": h(0.5e-6), "variance_share": h(0.390625)},
            {"input_id": "ref_cal", "sensitivity": h(1.0), "u_contrib": h(0.6e-6), "variance_share": h(0.5625)},
            {"input_id": "temp", "sensitivity": h(-11.5e-6), "u_contrib": h(0.0), "variance_share": h(0.0)},
        ],
        lpu_validated_by_mc="true",
        nonlinearity={"version": "nlin@1", "outcome": "linear_adequate", "v1": h(6.4e-13), "v2": h(0.0),
                      "ratio": h(0.0), "probed_inputs": "3", "probed_pairs": "3", "cross_terms_truncated": "false"},
        selection_rule="unc-select@1",
        estimate_convention="est-policy@1",
    ),
    "uec2_mcm": stmt(
        "UEC-2",
        method="MCM",
        unit=UNIT,
        u_std=h(1.1e-6),
        interval={"low": h(0.0249978), "high": h(0.0250021), "p": h(0.95), "kind": "shortest"},
        model=MODEL,
        sources=[src("dcc", "dcc:ring-gauge-25")],
        nonlinearity={"version": "nlin@1", "outcome": "use_mcm", "v1": h(1.21e-12), "v2": h(4.0e-13),
                      "probed_inputs": "40", "probed_pairs": "496", "cross_terms_truncated": "true",
                      "reason": "probe_failed"},
        selection_rule="unc-select@1",
        estimate_convention="est-policy@1",
    ),
    "uec3_iso15530_3": stmt(
        "UEC-3",
        method="ISO15530-3",
        unit=UNIT,
        expanded={"U": h(0.9e-6), "k": h(2.0)},
        model={"id": "um:subst-bore", "semver": "2.0.0", "digest": d("um:subst-bore@2.0.0")},
        sources=[src("qualification", "qual:cmm-2-2026"), src("dcc", "dcc:master-part-7")],
    ),
    "uec3_iso15530_4": stmt(
        "UEC-3",
        method="ISO15530-4",
        unit=UNIT,
        expanded={"U": h(1.2e-6), "k": h(2.0)},
        interval={"low": h(-1.25e-6), "high": h(1.15e-6), "p": h(0.95), "kind": "prob_symmetric"},
        model={"id": "um:vcmm-bore", "semver": "1.0.10", "digest": d("um:vcmm-bore@1.0.10")},
        sources=[src("qualification", "qual:vcmm-val-3"), src("dcc", "dcc:master-part-7"), src("type_a", "ta:val-runs")],
    ),
}


def without(s, *keys):
    t = dict(s)
    for k in keys:
        del t[k]
    return t


def withv(s, **kw):
    t = dict(s)
    t.update(kw)
    return t


V = VALID
# name -> (bytes, code, field)
INVALID = {
    "uec0_with_U": (canon(withv(V["uec0_u_missing"], expanded={"U": h(2e-6), "k": h(2.0)})), "uec0_has_value", "expanded"),
    "uec0_with_method": (canon(withv(V["uec0_model_missing"], method="LPU")), "uec0_has_value", "method"),
    "incomplete_method": (canon(without(V["uec1_lpu"], "method")), "statement_incomplete", "method"),
    "incomplete_unit": (canon(without(V["uec1_lpu"], "unit")), "statement_incomplete", "unit"),
    "incomplete_coverage": (canon(without(V["uec1_lpu"], "expanded")), "statement_incomplete", "coverage"),
    "incomplete_model": (canon(without(V["uec1_lpu"], "model")), "statement_incomplete", "model"),
    "incomplete_uec0_reason": (canon(without(V["uec0_model_missing"], "uec0_reason")), "statement_incomplete",
                               "uec0_reason"),
    "incomplete_class": (canon(without(V["uec1_declared"], "class")), "statement_incomplete", "class"),
    "nan_U": (canon(withv(V["uec1_declared"], expanded={"U": hb(0x7FF8000000000000), "k": h(2.0)})),
              "invalid_number", "expanded.U"),
    "inf_u_std": (canon(withv(V["uec1_lpu"], u_std=hb(0x7FF0000000000000))), "invalid_number", "u_std"),
    "neg_k": (canon(withv(V["uec1_declared"], expanded={"U": h(4e-6), "k": h(-2.0)})), "invalid_number", "expanded.k"),
    "zero_U": (canon(withv(V["uec1_declared"], expanded={"U": h(0.0), "k": h(2.0)})), "invalid_number", "expanded.U"),
    "negzero_u_std": (canon(withv(V["uec1_lpu"], u_std=hb(0x8000000000000000))), "invalid_number", "u_std"),
    "p_1": (canon(withv(V["uec1_spec_derived"], interval={"low": h(-3e-6), "high": h(3e-6), "p": h(1.0),
                                                           "kind": "prob_symmetric"})), "invalid_number", "interval.p"),
    "p_0": (canon(withv(V["uec1_spec_derived"], interval={"low": h(-3e-6), "high": h(3e-6), "p": h(0.0),
                                                           "kind": "prob_symmetric"})), "invalid_number", "interval.p"),
    "inverted_interval": (canon(withv(V["uec1_spec_derived"], interval={"low": h(3e-6), "high": h(-3e-6),
                                                                       "p": h(0.95), "kind": "prob_symmetric"})),
                          "invalid_number", "interval.high"),
    "share_above_1": (canon(withv(V["uec1_lpu"], contributors=[
        {"input_id": "probe_form", "u_contrib": h(1.2e-6), "variance_share": h(1.5)}])),
        "invalid_number", "contributors.variance_share"),
    "unknown_method": (canon(withv(V["uec1_lpu"], method="GUM")), "unknown_method", "-"),
    "lowercase_method": (canon(withv(V["uec1_lpu"], method="lpu")), "unknown_method", "-"),
    "uppercase_hex": (canon(withv(V["uec1_declared"], expanded={"U": h(4e-6).upper(), "k": h(2.0)})),
                      "invalid_field", "expanded.U"),
    "bad_unit": (canon(withv(V["uec1_lpu"], unit="mm")), "invalid_field", "unit"),
    "bad_model_id": (canon(withv(V["uec1_lpu"], model=withv(MODEL, id="bore"))), "invalid_field", "model.id"),
    "bad_semver": (canon(withv(V["uec1_lpu"], model=withv(MODEL, semver="1.02.0"))), "invalid_field", "model.semver"),
    "dup_source": (canon(withv(V["uec2_mcm"], sources=[src("dcc", "dcc:ring-gauge-25"), src("dcc", "dcc:ring-gauge-25")])),
                   "invalid_field", "sources.id"),
    "declared_as_uec2": (canon(withv(V["uec1_declared"], **{"class": "UEC-2", "sources": [src("dcc", "dcc:x")]})),
                         "class_mismatch", "method"),
    "uec3_lpu": (canon(withv(V["uec3_iso15530_3"], method="LPU")), "class_mismatch", "method"),
    "uec2_declared_source": (canon(withv(V["uec2_lpu"], sources=sorted(
        V["uec2_lpu"]["sources"] + [src("declared", "decl:guess")], key=lambda x: x["id"]))),
        "class_mismatch", "sources"),
    "uec2_no_source": (canon(withv(V["uec2_mcm"], sources=[])), "class_mismatch", "sources"),
    "uec1_with_reason": (canon(withv(V["uec1_lpu"], uec0_reason="u_missing")), "class_mismatch", "uec0_reason"),
    "agent_not_proposal": (canon(withv(V["uec1_spec_derived"], provenance=withv(
        V["uec1_spec_derived"]["provenance"], proposal="false"))), "class_mismatch", "provenance.proposal"),
    "declared_with_contributors": (canon(withv(V["uec1_declared"], contributors=[
        {"input_id": "a", "u_contrib": h(1e-6), "variance_share": h(1.0)}])), "class_mismatch", "contributors"),
    "declared_with_lpu_flag": (canon(withv(V["uec1_declared"], lpu_validated_by_mc="true")), "class_mismatch",
                               "lpu_validated_by_mc"),
    "spec_derived_with_nonlinearity": (canon(withv(V["uec1_spec_derived"], nonlinearity=V["uec1_lpu"]["nonlinearity"])),
                                       "class_mismatch", "nonlinearity"),
    "mcm_with_lpu_flag": (canon(withv(V["uec2_mcm"], lpu_validated_by_mc="false")), "class_mismatch",
                          "lpu_validated_by_mc"),
    "declared_no_principal": (canon(withv(V["uec1_declared"], provenance=PROV)), "class_mismatch", "provenance.principal"),
    "unit_not_si": (canon(withv(V["uec1_lpu"], unit="http://qudt.org/vocab/unit/M")), "invalid_field", "unit"),
    "unknown_member": (canon(withv(V["uec1_lpu"], note="x")), "malformed", "unknown_member"),
    "wrong_schema": (canon(withv(V["uec1_lpu"], schema="uncertainty_statement@2")), "malformed", "schema"),
    "number_literal": (canon(V["uec1_declared"]).replace(b'"k":"' + h(2.0).encode() + b'"', b'"k":2'),
                       "malformed", "unsupported_value"),
    "unsorted_sources": (canon(withv(V["uec2_lpu"], sources=list(reversed(V["uec2_lpu"]["sources"])))),
                         "non_canonical", "-"),
    "pretty_printed": (json.dumps(V["uec1_lpu"], sort_keys=True, indent=1).encode("ascii"), "non_canonical", "-"),
}


def outputs():
    assert b'"k":2' in INVALID["number_literal"][0], "number_literal replacement must apply"
    files = {}
    digests = []
    for name in sorted(VALID):
        b = canon(VALID[name])
        files["valid/%s.json" % name] = b
        digests.append("%s.json sha256:%s\n" % (name, hashlib.sha256(b).hexdigest()))
    files["valid/digests.txt"] = "".join(digests).encode("ascii")
    expected = []
    for name in sorted(INVALID):
        b, code, field = INVALID[name]
        files["invalid/%s.json" % name] = b
        expected.append("%s.json %s %s\n" % (name, code, field))
    files["invalid/expected.txt"] = "".join(expected).encode("ascii")
    return files


def main(argv):
    check = "--check" in argv[1:]
    files = outputs()
    for sub in ("valid", "invalid"):
        on_disk = {"%s/%s" % (sub, p.name) for p in (HERE / sub).glob("*")} if (HERE / sub).exists() else set()
        extra = on_disk - set(files)
        if extra:
            print("unexpected files: %s" % sorted(extra))
            return 1
    bad = 0
    for rel, b in sorted(files.items()):
        path = HERE / rel
        if check:
            if not path.exists() or path.read_bytes() != b:
                print("stale: %s" % rel)
                bad += 1
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b)
    if check:
        print("unc/stmt fixtures: %s" % ("ok" if bad == 0 else "%d stale" % bad))
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
