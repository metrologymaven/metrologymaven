#!/usr/bin/env python3
"""Generate fixtures/core/rpl/classify/cases.json (CORE.RPL.CLASS AC-1).

Cases come from scenario families whose expected outcome is stated by hand from
the spec's rules, not computed by a second classifier. CC-BY-4.0 project data.
Run:  python fixtures/core/rpl/classify/make_cases.py
"""

import json
import math
from pathlib import Path

TOL = {"length_mm": {"abs": 1e-5, "rel": 0.0}, "angle_deg": {"abs": 1e-7, "rel": 1e-9}}
cases = []


def out(kind, rec, rep):
    return {"kind": kind, "recorded": rec, "replayed": rep,
            "recorded_digest": "f64:" + float(rec).hex(), "replayed_digest": "f64:" + float(rep).hex()}


def case(name, cls, outputs, expect, ann=(), decisions=(), limits=(), same_kernel=True, same_platform=True,
         missing=None):
    cases.append({"name": name, "class": cls, "same_kernel": same_kernel, "same_platform_class": same_platform,
                  "outputs": list(outputs), "decisions": [{"recorded": a, "replayed": b} for a, b in decisions],
                  "limits": [{"kind": k, "value": v} for k, v in limits], "missing": missing,
                  "expect": {"outcome": expect, "annotations": sorted(ann)}})


up = lambda x: math.nextafter(x, math.inf)
CLASSES = ["D0", "D1", "D2", "D3", "N"]

# A. Identical digests -> IDENTICAL, whatever the class (REQ-001).
for i, c in enumerate(CLASSES):
    case(f"identical {c}", c, [out("length_mm", 12.5 + i, 12.5 + i)], "IDENTICAL",
         decisions=[("CONFORMS", "CONFORMS")])

# B. Decision flip -> DIVERGENT even within tolerance (REQ-002).
for c in CLASSES:
    case(f"decision flip {c}", c, [out("length_mm", 3.0, 3.000001)], "DIVERGENT",
         decisions=[("CONFORMS", "NONCONFORMS")], same_kernel=False)
case("status flip among several", "D3", [out("length_mm", 3.0, 3.000001)], "DIVERGENT",
     decisions=[("CONFORMS", "CONFORMS"), ("MEASURED", "NOT_MEASURED")])

# C. Bit identity required for D0/D1/D2 on the same kernel and platform class (REQ-003).
for c in ["D0", "D1", "D2"]:
    case(f"last bit {c} same kernel+platform", c, [out("length_mm", 7.25, up(7.25))], "DIVERGENT")
    case(f"last bit {c} other kernel", c, [out("length_mm", 7.25, up(7.25))], "EQUIVALENT", same_kernel=False)
    case(f"last bit {c} other platform", c, [out("length_mm", 7.25, up(7.25))], "EQUIVALENT", same_platform=False)
for c in ["D3", "N"]:
    case(f"last bit {c} same kernel+platform", c, [out("length_mm", 7.25, up(7.25))], "EQUIVALENT")
case("D2 other platform beyond tolerance", "D2", [out("length_mm", 7.25, 7.26)], "DIVERGENT", same_platform=False)

# D. Tolerances (REQ-004): absolute and relative parts.
case("D3 within absolute", "D3", [out("length_mm", 10.0, 10.000004)], "EQUIVALENT")
case("D3 beyond absolute", "D3", [out("length_mm", 10.0, 10.0001)], "DIVERGENT")
case("D3 angle within relative", "D3", [out("angle_deg", 1000.0, 1000.0000005)], "EQUIVALENT")
case("D3 angle beyond relative", "D3", [out("angle_deg", 1000.0, 1000.00001)], "DIVERGENT")
case("D3 negative within", "D3", [out("length_mm", -4.0, -4.000002)], "EQUIVALENT")
case("D3 negative beyond", "D3", [out("length_mm", -4.0, -4.1)], "DIVERGENT")
case("two outputs both within", "D3", [out("length_mm", 1.0, 1.000001), out("angle_deg", 45.0, 45.00000001)],
     "EQUIVALENT")
case("two outputs one beyond", "D3", [out("length_mm", 1.0, 1.000001), out("angle_deg", 45.0, 45.1)], "DIVERGENT")
case("one identical one within", "N", [out("length_mm", 2.0, 2.0), out("angle_deg", 30.0, 30.00000001)],
     "EQUIVALENT")

# E. Missing tolerance (REQ-005).
case("tolerance missing D3", "D3", [out("roughness_um", 0.8, 0.8001)], "DIVERGENT",
     ann=["TOLERANCE_MISSING:roughness_um"])
case("tolerance missing among known", "N", [out("length_mm", 1.0, 1.000001), out("temp_c", 20.0, 20.01)],
     "DIVERGENT", ann=["TOLERANCE_MISSING:temp_c"])
case("unknown kind but identical", "D3", [out("roughness_um", 0.8, 0.8)], "IDENTICAL")

# F. Missing input (REQ-006) wins over everything else.
case("missing input with differences", "D3", [out("length_mm", 1.0, 2.0)], "NOT_REPLAYABLE", missing="scan:part-7")
case("missing input with decision flip", "D0", [out("length_mm", 1.0, 1.0)], "NOT_REPLAYABLE",
     decisions=[("CONFORMS", "NONCONFORMS")], missing="probe-file")
case("missing input identical", "D1", [out("length_mm", 1.0, 1.0)], "NOT_REPLAYABLE", missing="cad")

# G. Marginal (REQ-008) never changes the outcome.
case("AC-3 1 nm flips decision at limit", "D3", [out("length_mm", 0.0499995, 0.0500005)], "DIVERGENT",
     ann=["MARGINAL"], decisions=[("CONFORMS", "NONCONFORMS")], limits=[("length_mm", 0.05)])
case("equivalent near limit", "D3", [out("length_mm", 0.0499990, 0.0499995)], "EQUIVALENT", ann=["MARGINAL"],
     limits=[("length_mm", 0.05)])
case("identical near limit", "D0", [out("length_mm", 0.049999, 0.049999)], "IDENTICAL", ann=["MARGINAL"],
     limits=[("length_mm", 0.05)])
case("equivalent far from limit", "D3", [out("length_mm", 0.0400000, 0.0400004)], "EQUIVALENT",
     limits=[("length_mm", 0.05)])
case("limit kind without tolerance", "D3", [out("roughness_um", 0.8, 0.8)], "IDENTICAL",
     limits=[("roughness_um", 0.8)])
case("limit on other kind", "D3", [out("length_mm", 0.05, 0.050001)], "EQUIVALENT", limits=[("angle_deg", 0.05)])
case("divergent bit identity near limit", "D0", [out("length_mm", 0.05, up(0.05))], "DIVERGENT", ann=["MARGINAL"],
     limits=[("length_mm", 0.05)])

assert len(cases) >= 40, len(cases)
assert len({c["name"] for c in cases}) == len(cases)
path = Path(__file__).with_name("cases.json")
path.write_text(json.dumps({"tolerances": TOL, "cases": cases}, indent=1) + "\n", encoding="utf-8", newline="\n")
print(f"wrote {len(cases)} cases to {path}")
