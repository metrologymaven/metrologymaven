#!/usr/bin/env python3
"""Generate and verify deterministic synthetic median-line bow fixture for GDT.FORM.STRAIGHT.DERIVED (CC0)."""

from __future__ import annotations

import argparse
import json
import math
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent


def build_median_line_bowed() -> dict:
    # 20 points along a circular bow in the X-Z plane from z = 0 to z = 0.100 m (100 mm)
    # with planted sagitta s = 0.010 mm (1e-5 m).
    # Circle through (0, 0), (s, L/2), (0, L) has radius R_arc = ((L/2)^2 + s^2) / (2*s),
    # center at (x_c = s - R_arc, z_c = L/2).
    # Exact minimum enclosing cylinder has axis (x = s/2, y = 0, dir = (0,0,1)) and diameter = s = 1e-5 m.
    length = 0.100
    sagitta = 1.0e-5  # 0.010 mm
    half_l = 0.5 * length
    r_arc = (half_l * half_l + sagitta * sagitta) / (2.0 * sagitta)
    x_c = sagitta - r_arc

    # Non-uniform z spacing placing index 0 at z=0, index 10 at exact z=L/2 (apex), index 19 at z=L.
    zs = [half_l * (i / 10.0) for i in range(11)] + [
        half_l + half_l * (j / 9.0) for j in range(1, 10)
    ]
    assert len(zs) == 20

    points = []
    for idx, z in enumerate(zs):
        dz = z - half_l
        if idx in (0, 19):
            x = 0.0
        elif idx == 10:
            x = sagitta
        else:
            x = x_c + math.sqrt(r_arc * r_arc - dz * dz)
        points.append([x, 0.0, z])

    return {
        "format": "fid-gdt-median-line-fixture/1",
        "licence": "CC0-1.0",
        "seed": "0x5701",
        "expected_mz_diameter_m": sagitta,
        "planted_apex_index": 10,
        "planted_contact_indices": [0, 10, 19],
        "points": points,
    }


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true")
    args = ap.parse_args()

    out_path = HERE / "median_line_bowed.json"
    expected = (
        json.dumps(build_median_line_bowed(), sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")
        + b"\n"
    )
    if args.check:
        if not out_path.exists() or out_path.read_bytes() != expected:
            print(f"mismatch: {out_path}", file=sys.stderr)
            return 1
        print(f"gdt-form-straight-derived fixture ok (1 file: {out_path.name})")
        return 0

    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_bytes(expected)
    print(f"wrote {out_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
