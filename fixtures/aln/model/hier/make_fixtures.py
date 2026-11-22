#!/usr/bin/env python3
"""Deterministic fixture generator for ALN.MODEL.HIER.CHAIN (CC0-1.0).

Generates:
  - fixtures/aln/model/hier/chain3.fidproj: A under root, B under A, C under B
    with known orthonormal rigid transforms, 16 test points within 10 m of the
    origin, and independent reference compositions T_A * T_B * T_C.
  - fixtures/aln/model/hier/chain64.json: a 64-deep chain of alignments with
    seeded rigid transforms and a 65th alignment ID for depth-limit checks.
"""

from __future__ import annotations

import argparse
import json
import math
import pathlib
import random
import struct
import sys

HERE = pathlib.Path(__file__).resolve().parent


def f64_hex(x: float) -> str:
    if x == 0.0:
        x = 0.0
    bits = struct.unpack(">Q", struct.pack(">d", x))[0]
    return f"0x{bits:016x}"


def rot_axis_angle(axis: tuple[float, float, float], angle: float) -> list[list[float]]:
    ax, ay, az = axis
    norm = math.sqrt(ax * ax + ay * ay + az * az)
    ux, uy, uz = ax / norm, ay / norm, az / norm
    c = math.cos(angle)
    s = math.sin(angle)
    omc = 1.0 - c
    return [
        [c + ux * ux * omc, ux * uy * omc - uz * s, ux * uz * omc + uy * s],
        [uy * ux * omc + uz * s, c + uy * uy * omc, uy * uz * omc - ux * s],
        [uz * ux * omc - uy * s, uz * uy * omc + ux * s, c + uz * uz * omc],
    ]


def mat_mul(a: list[list[float]], b: list[list[float]]) -> list[list[float]]:
    return [
        [sum(a[i][k] * b[k][j] for k in range(3)) for j in range(3)]
        for i in range(3)
    ]


def mat_vec(m: list[list[float]], v: list[float]) -> list[float]:
    return [sum(m[i][k] * v[k] for k in range(3)) for i in range(3)]


def apply_rigid(r: list[list[float]], t: list[float], p: list[float]) -> list[float]:
    rp = mat_vec(r, p)
    return [rp[0] + t[0], rp[1] + t[1], rp[2] + t[2]]


def compose_rigid(
    r1: list[list[float]],
    t1: list[float],
    r2: list[list[float]],
    t2: list[float],
) -> tuple[list[list[float]], list[float]]:
    r = mat_mul(r1, r2)
    t = apply_rigid(r1, t1, t2)
    return r, t


def uuid_for(index: int) -> str:
    return f"00000000-0000-7000-8000-{index:012x}"


def encode_rigid(r: list[list[float]], t: list[float]) -> dict[str, object]:
    return {
        "kind": "rigid",
        "r": [[f64_hex(x) for x in row] for row in r],
        "t": [f64_hex(x) for x in t],
    }


def make_chain3() -> dict[str, object]:
    # Known exact/orthonormal transforms A (under root), B (under A), C (under B).
    ra = rot_axis_angle((0.0, 0.0, 1.0), math.pi / 2.0)  # 90 deg about Z
    ta = [1.25, -0.5, 0.75]

    rb = rot_axis_angle((1.0, 0.0, 0.0), math.pi / 3.0)  # 60 deg about X
    tb = [-0.4, 2.1, 0.3]

    rc = rot_axis_angle((1.0, 2.0, -1.5), 0.7123456789)  # arbitrary axis-angle
    tc = [0.65, -1.15, 0.9]

    r_ab, t_ab = compose_rigid(ra, ta, rb, tb)
    r_abc, t_abc = compose_rigid(r_ab, t_ab, rc, tc)

    rng = random.Random(20261122)
    test_points: list[dict[str, object]] = []
    # Include origin, axis extremes within 10 m, and seeded interior points.
    raw_points: list[list[float]] = [
        [0.0, 0.0, 0.0],
        [5.0, 0.0, 0.0],
        [0.0, -6.0, 0.0],
        [0.0, 0.0, 7.5],
        [-3.2, 4.1, -2.7],
        [4.5, -5.2, 3.8],
    ]
    while len(raw_points) < 16:
        # Uniform inside ball of radius 9.5 m (< 10 m).
        while True:
            p = [rng.uniform(-9.5, 9.5) for _ in range(3)]
            if math.sqrt(p[0] ** 2 + p[1] ** 2 + p[2] ** 2) <= 9.5:
                raw_points.append(p)
                break

    for p in raw_points:
        expected = apply_rigid(r_abc, t_abc, p)
        test_points.append(
            {
                "point": [f64_hex(x) for x in p],
                "expected_root": [f64_hex(x) for x in expected],
            }
        )

    return {
        "schema": "fidproj-aln-hier-chain/v1",
        "license": "CC0-1.0",
        "description": "3-deep alignment chain A -> B -> C with known transforms and test points (ALN.MODEL.HIER.CHAIN AC-1..AC-5).",
        "input_element": uuid_for(0x10),
        "dependent_result": uuid_for(0x20),
        "alignments": [
            {
                "label": "A",
                "id": uuid_for(1),
                "parent": None,
                "transform": encode_rigid(ra, ta),
            },
            {
                "label": "B",
                "id": uuid_for(2),
                "parent": uuid_for(1),
                "transform": encode_rigid(rb, tb),
            },
            {
                "label": "C",
                "id": uuid_for(3),
                "parent": uuid_for(2),
                "transform": encode_rigid(rc, tc),
            },
        ],
        "expected_c_to_root": encode_rigid(r_abc, t_abc),
        "test_points": test_points,
    }


def make_chain64() -> dict[str, object]:
    rng = random.Random(646464)
    alignments: list[dict[str, object]] = []
    r_acc = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
    t_acc = [0.0, 0.0, 0.0]

    for i in range(1, 65):
        axis = (
            rng.uniform(-1.0, 1.0),
            rng.uniform(-1.0, 1.0),
            rng.uniform(-1.0, 1.0),
        )
        if axis == (0.0, 0.0, 0.0):
            axis = (0.0, 0.0, 1.0)
        angle = rng.uniform(-0.35, 0.35)
        r = rot_axis_angle(axis, angle)
        t = [rng.uniform(-0.1, 0.1) for _ in range(3)]
        r_acc, t_acc = compose_rigid(r_acc, t_acc, r, t)
        alignments.append(
            {
                "depth": i,
                "id": uuid_for(0x100 + i),
                "parent": None if i == 1 else uuid_for(0x100 + i - 1),
                "transform": encode_rigid(r, t),
            }
        )

    extra_id = uuid_for(0x100 + 65)
    extra_r = rot_axis_angle((0.0, 1.0, 0.0), 0.25)
    extra_t = [0.05, -0.05, 0.05]

    probe = [1.5, -2.25, 3.125]
    expected_probe = apply_rigid(r_acc, t_acc, probe)

    return {
        "schema": "fidproj-aln-hier-chain64/v1",
        "license": "CC0-1.0",
        "description": "64-deep alignment chain with seeded rigid transforms and 65th alignment candidate (ALN.MODEL.HIER.CHAIN AC-3).",
        "input_element": uuid_for(0x200),
        "alignments": alignments,
        "extra_65th": {
            "id": extra_id,
            "transform": encode_rigid(extra_r, extra_t),
        },
        "probe_point": [f64_hex(x) for x in probe],
        "expected_deepest_probe_root": [f64_hex(x) for x in expected_probe],
    }


def dump_canonical(obj: dict[str, object]) -> bytes:
    return (json.dumps(obj, sort_keys=True, indent=2) + "\n").encode("utf-8")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--check", action="store_true", help="Verify fixtures match generated bytes.")
    args = ap.parse_args()

    outputs = {
        HERE / "chain3.fidproj": dump_canonical(make_chain3()),
        HERE / "chain64.json": dump_canonical(make_chain64()),
    }

    if args.check:
        ok = True
        for path, expected in outputs.items():
            if not path.exists():
                print(f"MISSING: {path}", file=sys.stderr)
                ok = False
                continue
            actual = path.read_bytes()
            if actual != expected:
                print(f"DRIFT: {path}", file=sys.stderr)
                ok = False
        return 0 if ok else 1

    for path, content in outputs.items():
        path.write_bytes(content)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
