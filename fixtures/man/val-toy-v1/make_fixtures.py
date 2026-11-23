#!/usr/bin/env python3
"""Deterministic fixture generator for KRN.MAN.VAL.RUN (CC-BY-4.0).

Generates fixtures/man/val-toy-v1/:
  - mappings.toml: versioned case-to-call mapping data
  - criteria/toy-set-1.0.toml: versioned pass-criteria file
  - release.json: 12 synthetic cases (6 planes incl. 1 deliberately failing case,
    6 circles) with exact reference parameters
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import pathlib
import struct
import sys

HERE = pathlib.Path(__file__).resolve().parent


def f64_hex(x: float) -> str:
    if x == 0.0:
        x = 0.0
    bits = struct.unpack(">Q", struct.pack(">d", x))[0]
    return f"0x{bits:016x}"


def unit(v: tuple[float, float, float]) -> list[float]:
    n = math.sqrt(v[0] ** 2 + v[1] ** 2 + v[2] ** 2)
    return [v[0] / n, v[1] / n, v[2] / n]


def basis_from_normal(n: list[float]) -> tuple[list[float], list[float]]:
    helper = [1.0, 0.0, 0.0] if abs(n[0]) < 0.9 else [0.0, 1.0, 0.0]
    u = [
        n[1] * helper[2] - n[2] * helper[1],
        n[2] * helper[0] - n[0] * helper[2],
        n[0] * helper[1] - n[1] * helper[0],
    ]
    un = math.sqrt(sum(c * c for c in u))
    u = [c / un for c in u]
    v = [
        n[1] * u[2] - n[2] * u[1],
        n[2] * u[0] - n[0] * u[2],
        n[0] * u[1] - n[1] * u[0],
    ]
    return u, v


def plane_points(origin: list[float], normal: list[float], scale: float) -> list[list[str]]:
    u, v = basis_from_normal(normal)
    grid = [
        (-1.0, -1.0),
        (1.0, -1.0),
        (1.0, 1.0),
        (-1.0, 1.0),
        (0.0, -1.5),
        (0.0, 1.5),
        (-1.5, 0.0),
        (1.5, 0.0),
    ]
    pts = []
    for a, b in grid:
        p = [origin[i] + scale * (a * u[i] + b * v[i]) for i in range(3)]
        pts.append([f64_hex(c) for c in p])
    return pts


def circle_points(centre: list[float], normal: list[float], radius: float, count: int = 12) -> list[list[str]]:
    u, v = basis_from_normal(normal)
    pts = []
    for k in range(count):
        ang = 2.0 * math.pi * k / count
        ca, sa = math.cos(ang), math.sin(ang)
        p = [centre[i] + radius * (ca * u[i] + sa * v[i]) for i in range(3)]
        pts.append([f64_hex(c) for c in p])
    return pts


def make_mappings_toml() -> bytes:
    return (
        "# Versioned case-to-call mapping data for fixtures/man/val-toy-v1 (CC-BY-4.0)\n"
        'version = "val-map-toy@1"\n'
        "\n"
        "[[mapping]]\n"
        'set = "toy-set"\n'
        'primitive = "plane"\n'
        'case_type = "ls"\n'
        'algorithm = "ls.plane@1.0.0"\n'
        "\n"
        "[mapping.params_template]\n"
        'method = "orthogonal_ls"\n'
        "\n"
        "[[mapping.compare_rules]]\n"
        'param = "point"\n'
        'kind = "plane_offset"\n'
        'normal_param = "normal"\n'
        "\n"
        "[[mapping.compare_rules]]\n"
        'param = "normal"\n'
        'kind = "direction"\n'
        "sign_invariant = true\n"
        "\n"
        "[[mapping.compare_rules]]\n"
        'param = "rms"\n'
        'kind = "scalar"\n'
        "\n"
        "[[mapping]]\n"
        'set = "toy-set"\n'
        'primitive = "circle"\n'
        'case_type = "ls"\n'
        'algorithm = "ls.circle@1.0.0"\n'
        "\n"
        "[mapping.params_template]\n"
        'method = "algebraic_ls"\n'
        "\n"
        "[[mapping.compare_rules]]\n"
        'param = "centre"\n'
        'kind = "point3d"\n'
        "\n"
        "[[mapping.compare_rules]]\n"
        'param = "normal"\n'
        'kind = "direction"\n'
        "sign_invariant = true\n"
        "\n"
        "[[mapping.compare_rules]]\n"
        'param = "radius"\n'
        'kind = "scalar"\n'
        "\n"
        "[[mapping.compare_rules]]\n"
        'param = "rms"\n'
        'kind = "scalar"\n'
    ).encode("utf-8")


def make_criteria_toml() -> bytes:
    return (
        "# Versioned pass criteria for toy-set 1.0 (CC-BY-4.0; internal toy limits [unverified])\n"
        'set = "toy-set"\n'
        'version = "1.0"\n'
        "\n"
        "[limits]\n"
        "centre = 1.0e-8\n"
        "normal = 1.0e-8\n"
        "point = 1.0e-8\n"
        "radius = 1.0e-8\n"
        "rms = 1.0e-9\n"
    ).encode("utf-8")


def make_release_json() -> bytes:
    cases: list[dict[str, object]] = []

    plane_specs = [
        ("plane-01", [0.0, 0.0, 0.25], unit((0.0, 0.0, 1.0)), 0.1, 0.0),
        ("plane-02", [1.5, -0.5, 0.0], unit((1.0, 0.0, 0.0)), 0.2, 0.0),
        ("plane-03", [0.2, 0.3, -0.4], unit((1.0, 1.0, 1.0)), 0.15, 0.0),
        ("plane-04", [-0.7, 0.8, 0.9], unit((0.2, -0.5, 0.8)), 0.25, 0.0),
        ("plane-05", [0.0, -1.2, 0.6], unit((0.3, 0.4, -0.5)), 0.12, 0.0),
        # Deliberately failing case: reference point shifted +0.005 m along the normal.
        ("plane-06-fail", [0.1, 0.2, 0.3], unit((0.0, 0.0, 1.0)), 0.1, 0.005),
    ]
    for cid, orig, norm, scale, ref_shift in plane_specs:
        ref_pt = [orig[i] + ref_shift * norm[i] for i in range(3)]
        cases.append(
            {
                "case_id": cid,
                "set": "toy-set",
                "set_version": "1.0",
                "primitive": "plane",
                "case_type": "ls",
                "points": plane_points(orig, norm, scale),
                "reference": {
                    "point": {"value": [f64_hex(c) for c in ref_pt], "uncertainty": f64_hex(0.0)},
                    "normal": {"value": [f64_hex(c) for c in norm], "uncertainty": f64_hex(0.0)},
                    "rms": {"value": f64_hex(0.0), "uncertainty": f64_hex(0.0)},
                },
            }
        )

    circle_specs = [
        ("circle-01", [0.0, 0.0, 0.0], unit((0.0, 0.0, 1.0)), 0.05),
        ("circle-02", [0.5, -0.25, 0.1], unit((0.0, 0.0, 1.0)), 0.12),
        ("circle-03", [-0.3, 0.4, 0.2], unit((1.0, 0.0, 0.0)), 0.08),
        ("circle-04", [0.1, 0.2, -0.3], unit((0.0, 1.0, 0.0)), 0.15),
        ("circle-05", [0.25, -0.15, 0.35], unit((1.0, 1.0, 0.0)), 0.075),
        ("circle-06", [-0.4, -0.2, 0.5], unit((0.3, -0.4, 0.5)), 0.2),
    ]
    for cid, ctr, norm, rad in circle_specs:
        cases.append(
            {
                "case_id": cid,
                "set": "toy-set",
                "set_version": "1.0",
                "primitive": "circle",
                "case_type": "ls",
                "points": circle_points(ctr, norm, rad),
                "reference": {
                    "centre": {"value": [f64_hex(c) for c in ctr], "uncertainty": f64_hex(0.0)},
                    "normal": {"value": [f64_hex(c) for c in norm], "uncertainty": f64_hex(0.0)},
                    "radius": {"value": f64_hex(rad), "uncertainty": f64_hex(0.0)},
                    "rms": {"value": f64_hex(0.0), "uncertainty": f64_hex(0.0)},
                },
            }
        )

    assert len(cases) == 12
    body_bytes = json.dumps(cases, sort_keys=True, separators=(",", ":")).encode("utf-8")
    suite_digest = "sha256:" + hashlib.sha256(body_bytes).hexdigest()
    release = {
        "schema": "val-release/v1",
        "license": "CC-BY-4.0",
        "release_id": "val-toy-v1",
        "suite_digest": suite_digest,
        "cases": cases,
    }
    return (json.dumps(release, sort_keys=True, indent=2) + "\n").encode("utf-8")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--check", action="store_true", help="Verify committed fixtures match generated bytes.")
    args = ap.parse_args()

    outputs = {
        HERE / "mappings.toml": make_mappings_toml(),
        HERE / "criteria" / "toy-set-1.0.toml": make_criteria_toml(),
        HERE / "release.json": make_release_json(),
    }

    if args.check:
        ok = True
        for path, expected in outputs.items():
            if not path.exists():
                print(f"MISSING: {path}", file=sys.stderr)
                ok = False
                continue
            if path.read_bytes() != expected:
                print(f"DRIFT: {path}", file=sys.stderr)
                ok = False
        return 0 if ok else 1

    for path, content in outputs.items():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
