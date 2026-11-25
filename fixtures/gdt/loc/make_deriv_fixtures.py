#!/usr/bin/env python3
"""Independent reference for GDT.LOC.DERIV.TYPES (derived-element type and dispatch), stdlib only.

Writes two files next to this script:

- `deriv_stub_points.json` (AC-1): the measured points of a synthetic cylindrical feature, a
  synthetic operator chain whose reduction step names the stub algorithm
  `syn.axis.stub@1.0.0`, and the expected `fid.derived-element/1` record `derive()` returns
  when that stub is registered. The stub is defined here, in words, and implemented
  separately in the Rust test: it returns the axis through (x0, y0, (zmin + zmax) / 2) along
  (0, 0, 1) with extent zmax - zmin, where (x0, y0) are the first point's x and y and zmin,
  zmax the smallest and largest z of all points. The z values are multiples of 2^-8 m, so the
  midpoint and the extent are exact in binary64 and the expected record follows from the rule
  with no rounding question. The expected `alg` is the reduction's algorithm ID
  (REQ-GDT.LOC.DERIV.TYPES-001) and the flags are empty (the stub sets none).
- `deriv_elements.json` (AC-5): hand-written serialised examples of all four variants (axis,
  median line, median surface with and without normals, median points), one of them flagged
  `local_optimum_only`, in the canonical wire form of the leaf: every float as "0x" plus 16
  lowercase hex digits of its bits, keys sorted. Each example also carries its canonical JSON
  text (`canonical`), which the Rust test compares byte for byte with what fid-gps serialises
  for the same element. `check_deriv_schema.py` validates the examples against the exported
  JSON Schema.

This script does not run or mirror the Rust code. The points and examples are synthetic
inputs, not an interpretation of any standard. With `--check` the files are rebuilt in memory
and compared byte for byte with the committed ones. All geometry in metres; CC0.
"""

import json
import struct
import sys
from fractions import Fraction as F
from pathlib import Path

HERE = Path(__file__).resolve().parent
SCHEMA = "fid.derived-element/1"
STUB_ALG = "syn.axis.stub@1.0.0"


def hx(x):
    """Bits of a binary64 as 0x + 16 lowercase hex digits (negative zero keeps its sign)."""
    return "0x" + struct.pack(">d", float(x)).hex()


def v3(p):
    return [hx(c) for c in p]


def canonical(obj):
    # RFC 8785 for this content: only strings, arrays and objects with ASCII keys, so sorted
    # keys and no whitespace are the canonical form.
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def record(alg, element, flags=()):
    return {"alg": alg, "element": element, "flags": sorted(flags), "schema": SCHEMA}


def stub_points():
    # 12 points on a circle of radius 2^-7 m about (0.01, -0.004) (rational parametrisation
    # ((1 - t^2), 2 t) / (1 + t^2), each coordinate rounded once to binary64), at heights
    # z = k * 2^-8 m for k in a fixed shuffled order so zmin and zmax are not the first or last.
    r = F(1, 128)
    cx, cy = F(1, 100), F(-4, 1000)
    ts = [F(0), F(1, 3), F(1, 2), F(1), F(2), F(3), F(-3), F(-2), F(-1), F(-1, 2), F(-1, 3), F(1, 5)]
    ks = [3, 7, 1, 9, 5, 0, 11, 2, 8, 4, 10, 6]
    pts = []
    for t, k in zip(ts, ks):
        d = 1 + t * t
        x = cx + r * (1 - t * t) / d
        y = cy + r * 2 * t / d
        z = F(k, 256) - F(1, 64)
        pts.append((float(x), float(y), float(z)))
    return pts


def build_stub():
    pts = stub_points()
    zs = [p[2] for p in pts]
    zmin, zmax = min(zs), max(zs)
    # Exactness of the stub's arithmetic on these inputs (multiples of 2^-8 well inside range).
    assert F(zmin) + F(zmax) == F((zmin + zmax)), "midpoint sum not exact"
    assert F(zmax) - F(zmin) == F(zmax - zmin), "extent not exact"
    point = (pts[0][0], pts[0][1], (zmin + zmax) / 2)
    element = {"direction": v3((0.0, 0.0, 1.0)), "extent": hx(zmax - zmin), "kind": "axis", "point": v3(point)}
    chain = {
        "variant": "default",
        "steps": [
            {"alg": "syn.partition.whole_feature@1.0.0", "kind": "partition", "params": {}},
            {"alg": "syn.association.ls_cylinder@1.0.0", "kind": "association", "params": {}},
            {"alg": STUB_ALG, "kind": "reduction", "params": {}},
            {"alg": "syn.evaluation.zone@1.0.0", "kind": "evaluation", "params": {}},
        ],
    }
    expected = record(STUB_ALG, element)
    return {
        "about": "GDT.LOC.DERIV.TYPES AC-1: points, chain and the record derive() returns with the stub axis "
        "registered under syn.axis.stub@1.0.0 (see make_deriv_fixtures.py). Synthetic, CC0.",
        "chain": chain,
        "expected": expected,
        "expected_canonical": canonical(expected),
        "feature_kind": "cylinder",
        "points": [v3(p) for p in pts],
    }


def build_elements():
    examples = [
        (
            "axis_flagged",
            record(
                "syn.axis.example@1.0.0",
                {
                    "direction": v3((0.6, 0.0, -0.8)),
                    "extent": hx(0.03),
                    "kind": "axis",
                    "point": v3((0.01, -0.02, 0.015)),
                },
                ["local_optimum_only"],
            ),
        ),
        (
            "median_line",
            record(
                "syn.mline.example@1.0.0",
                {"kind": "median_line", "points": [v3((0.0, 0.0, 0.0)), v3((1e-6, -2e-6, 0.01)), v3((0.0, 3e-6, 0.02))]},
            ),
        ),
        (
            "median_surface_normals",
            record(
                "syn.msurf.example@1.0.0",
                {
                    "kind": "median_surface",
                    "normals": [v3((-0.0, 1.0, 0.0)), v3((0.0, 1.0, 0.0))],
                    "points": [v3((0.005, 0.0125, 0.0)), v3((0.015, 0.0125, -0.0))],
                },
            ),
        ),
        (
            "median_surface_plain",
            record("syn.msurf.example@1.0.0", {"kind": "median_surface", "points": [v3((0.005, 0.0125, 0.002))]}),
        ),
        (
            "median_points",
            record(
                "syn.mpts.example@2.1.0",
                {"kind": "median_points", "points": [v3((-0.001, 0.0, 0.25)), v3((1.5e-300, -1e300, 5e-324))]},
            ),
        ),
    ]
    return {
        "about": "GDT.LOC.DERIV.TYPES AC-5: serialised examples of all four DerivedElement variants "
        "(see make_deriv_fixtures.py). Synthetic, CC0.",
        "examples": [{"canonical": canonical(rec), "name": name, "record": rec} for name, rec in examples],
    }


def render(doc):
    return json.dumps(doc, indent=1, sort_keys=True) + "\n"


def main():
    check = "--check" in sys.argv[1:]
    outputs = {"deriv_stub_points.json": render(build_stub()), "deriv_elements.json": render(build_elements())}
    bad = False
    for name, text in outputs.items():
        path = HERE / name
        if check:
            if not path.exists() or path.read_text(encoding="utf-8") != text:
                print(f"{name}: differs from the reference; rerun without --check", file=sys.stderr)
                bad = True
        else:
            path.write_text(text, encoding="utf-8", newline="\n")
            print(f"wrote {path}")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
