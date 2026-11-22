#!/usr/bin/env python3
"""Independent fixture generator for KRN.LSQ.PLAN2D.ASSIGN (stdlib only; CC-BY-4.0).

Builds `fixtures/fit/assign-v1/`:
  - `slot.json`: 4-segment CCW slot (2 lines + 2 semicircular arcs), 5 000 seeded points
    including explicit equidistant tie points (e.g. origin equidistant from bottom/top lines
    and exact junction points shared by adjacent segments).
  - `rounded_rect.json`: 8-segment CCW rounded rectangle (4 lines + 4 quarter-circle arcs),
    5 000 seeded points including explicit equidistant tie points.
  - `README.md`: provenance and CC-BY-4.0 license.

Total: 10 000 seeded points across both profiles (AC-1). Every non-tie point is placed with
clear margin from any segment bisector so Python and Rust agree on every single assignment,
while explicit tie points use exact dyadic/symmetric coordinates so exact ties break strictly
to the lower segment index.

Usage:
  python3 fixtures/fit/make_assign_fixtures.py          # write files
  python3 fixtures/fit/make_assign_fixtures.py --check  # verify byte-for-byte match
"""

import json
import math
import random
import struct
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE / "assign-v1"
LICENSE = "CC-BY-4.0"
TWO_PI = 2.0 * math.pi


def hexbits(x: float) -> str:
    return "%016x" % struct.unpack("<Q", struct.pack("<d", x))[0]


def wrap_pi(a: float) -> float:
    q = a / TWO_PI
    k = float(int(q + 0.5) if q >= 0.0 else int(q - 0.5))
    r = a - k * TWO_PI
    for _ in range(8):
        if r <= -math.pi:
            r += TWO_PI
        elif r > math.pi:
            r -= TWO_PI
        else:
            break
    return r


def signed_dist(seg: dict, px: float, py: float) -> float:
    if seg["kind"] == "line":
        ax, ay = seg["a"]
        bx, by = seg["b"]
        dx = bx - ax
        dy = by - ay
        length = math.hypot(dx, dy)
        tx = dx / length
        ty = dy / length
        apx = px - ax
        apy = py - ay
        proj = apx * tx + apy * ty
        s = min(max(proj, 0.0), length)
        qx = ax + s * tx
        qy = ay + s * ty
        dist = math.hypot(px - qx, py - qy)
        side = apx * ty - apy * tx
        if dist == 0.0:
            return 0.0
        return -dist if side < 0.0 else dist
    else:
        cx, cy = seg["c"]
        r = seg["r"]
        start = seg["start"]
        sweep = seg["sweep"]
        rx = px - cx
        ry = py - cy
        rho = math.hypot(rx, ry)
        theta = math.atan2(ry, rx)
        mid = start + 0.5 * sweep
        half_sweep = 0.5 * abs(sweep)
        delta_mid = wrap_pi(theta - mid)
        if abs(delta_mid) <= half_sweep:
            signed = rho - r
            return 0.0 if signed == 0.0 else signed
        if (sweep > 0.0 and delta_mid < 0.0) or (sweep < 0.0 and delta_mid > 0.0):
            phi = start
        else:
            phi = start + sweep
        ex = cx + r * math.cos(phi)
        ey = cy + r * math.sin(phi)
        dist = math.hypot(px - ex, py - ey)
        if dist == 0.0:
            return 0.0
        return -dist if rho < r else dist


def assign_points(segments: list[dict], points: list[tuple[float, float]]):
    seg_of = []
    residuals = []
    per_seg = [[] for _ in segments]
    for px, py in points:
        best_idx = 0
        best_sd = signed_dist(segments[0], px, py)
        best_abs = abs(best_sd)
        for idx in range(1, len(segments)):
            sd = signed_dist(segments[idx], px, py)
            ad = abs(sd)
            if ad < best_abs:
                best_idx = idx
                best_sd = sd
                best_abs = ad
        seg_of.append(best_idx)
        residuals.append(best_sd)
        per_seg[best_idx].append(best_sd)

    stats = []
    for idx, vals in enumerate(per_seg):
        n = len(vals)
        if n == 0:
            stats.append({"segment": idx, "count": 0, "rms": None, "max": None, "min": None})
        else:
            rms = math.sqrt(math.fsum(v * v for v in vals) / n)
            stats.append({"segment": idx, "count": n, "rms": rms, "max": max(vals), "min": min(vals)})
    return seg_of, residuals, stats


def sample_segment(rng: random.Random, seg: dict, noise_sigma: float) -> tuple[float, float]:
    """Sample a point clearly associated with `seg` (away from endpoints)."""
    if seg["kind"] == "line":
        ax, ay = seg["a"]
        bx, by = seg["b"]
        dx, dy = bx - ax, by - ay
        length = math.hypot(dx, dy)
        tx, ty = dx / length, dy / length
        nx, ny = ty, -tx  # outward normal
        t = rng.uniform(0.1, 0.9) * length
        dn = rng.gauss(0.0, noise_sigma)
        return (ax + t * tx + dn * nx, ay + t * ty + dn * ny)
    else:
        cx, cy = seg["c"]
        r = seg["r"]
        start = seg["start"]
        sweep = seg["sweep"]
        phi = start + rng.uniform(0.1, 0.9) * sweep
        dr = rng.gauss(0.0, noise_sigma)
        return (cx + (r + dr) * math.cos(phi), cy + (r + dr) * math.sin(phi))


def build_slot():
    # CCW slot: center-to-center length 10 (x in [-5, 5]), radius 2 (y in [-2, 2]).
    segments = [
        {"kind": "line", "a": [-5.0, -2.0], "b": [5.0, -2.0]},  # 0: bottom
        {"kind": "arc", "c": [5.0, 0.0], "r": 2.0, "start": -0.5 * math.pi, "sweep": math.pi},  # 1: right arc
        {"kind": "line", "a": [5.0, 2.0], "b": [-5.0, 2.0]},  # 2: top
        {"kind": "arc", "c": [-5.0, 0.0], "r": 2.0, "start": 0.5 * math.pi, "sweep": math.pi},  # 3: left arc
    ]
    # Explicit exact equidistant tie points:
    # - (0.0, 0.0): equidistant (2.0) from seg 0 (bottom) and seg 2 (top) -> tie -> seg 0
    # - (5.0, -2.0): exact junction of seg 0 and seg 1 (dist 0.0) -> tie -> seg 0
    # - (5.0, 2.0): exact junction of seg 1 and seg 2 (dist 0.0) -> tie -> seg 1
    # - (-5.0, 2.0): exact junction of seg 2 and seg 3 (dist 0.0) -> tie -> seg 2
    # - (-5.0, -2.0): exact junction of seg 3 and seg 0 (dist 0.0) -> tie -> seg 0
    tie_points = [
        (0.0, 0.0),
        (5.0, -2.0),
        (5.0, 2.0),
        (-5.0, 2.0),
        (-5.0, -2.0),
    ]
    rng = random.Random(0x5107_0001)
    points = list(tie_points)
    for i in range(5000 - len(tie_points)):
        seg = segments[i % len(segments)]
        points.append(sample_segment(rng, seg, 0.02))

    seg_of, _, stats = assign_points(segments, points)
    assert seg_of[:5] == [0, 0, 1, 2, 0], seg_of[:5]

    return {
        "title": "KRN.LSQ.PLAN2D.ASSIGN AC-1 slot profile (5 000 seeded points incl. 5 exact ties)",
        "license": LICENSE,
        "seed": "0x51070001",
        "tie_count": len(tie_points),
        "segments": segments,
        "points_hex": [[hexbits(x), hexbits(y)] for x, y in points],
        "expected_segment_of": seg_of,
        "expected_stats": stats,
    }


def build_rounded_rect():
    # CCW rounded rectangle: half-width 8, half-height 5, corner radius 1.
    # Straight extents: x in [-7, 7], y in [-4, 4]; outer lines at x = +/-8, y = +/-5.
    segments = [
        {"kind": "line", "a": [-7.0, -5.0], "b": [7.0, -5.0]},  # 0: bottom
        {"kind": "arc", "c": [7.0, -4.0], "r": 1.0, "start": -0.5 * math.pi, "sweep": 0.5 * math.pi},  # 1: BR
        {"kind": "line", "a": [8.0, -4.0], "b": [8.0, 4.0]},  # 2: right
        {"kind": "arc", "c": [7.0, 4.0], "r": 1.0, "start": 0.0, "sweep": 0.5 * math.pi},  # 3: TR
        {"kind": "line", "a": [7.0, 5.0], "b": [-7.0, 5.0]},  # 4: top
        {"kind": "arc", "c": [-7.0, 4.0], "r": 1.0, "start": 0.5 * math.pi, "sweep": 0.5 * math.pi},  # 5: TL
        {"kind": "line", "a": [-8.0, 4.0], "b": [-8.0, -4.0]},  # 6: left
        {"kind": "arc", "c": [-7.0, -4.0], "r": 1.0, "start": math.pi, "sweep": 0.5 * math.pi},  # 7: BL
    ]
    tie_points = [
        (0.0, 0.0),    # equidistant (5.0) from seg 0 (bottom) and seg 4 (top) -> seg 0
        (7.0, -5.0),   # junction seg 0 & 1 -> seg 0
        (8.0, -4.0),   # junction seg 1 & 2 -> seg 1
        (8.0, 4.0),    # junction seg 2 & 3 -> seg 2
        (7.0, 5.0),    # junction seg 3 & 4 -> seg 3
        (-7.0, 5.0),   # junction seg 4 & 5 -> seg 4
        (-8.0, 4.0),   # junction seg 5 & 6 -> seg 5
        (-8.0, -4.0),  # junction seg 6 & 7 -> seg 6
        (-7.0, -5.0),  # junction seg 7 & 0 -> seg 0
    ]
    rng = random.Random(0x52EC_0002)
    points = list(tie_points)
    for i in range(5000 - len(tie_points)):
        seg = segments[i % len(segments)]
        points.append(sample_segment(rng, seg, 0.015))

    seg_of, _, stats = assign_points(segments, points)
    assert seg_of[:9] == [0, 0, 1, 2, 3, 4, 5, 6, 0], seg_of[:9]

    return {
        "title": "KRN.LSQ.PLAN2D.ASSIGN AC-1 rounded-rectangle profile (5 000 seeded points incl. 9 exact ties)",
        "license": LICENSE,
        "seed": "0x52EC0002",
        "tie_count": len(tie_points),
        "segments": segments,
        "points_hex": [[hexbits(x), hexbits(y)] for x, y in points],
        "expected_segment_of": seg_of,
        "expected_stats": stats,
    }


def build_readme() -> str:
    return (
        "# KRN.LSQ.PLAN2D.ASSIGN test fixtures (`assign-v1`)\n"
        "\n"
        "- **Spec node:** `KRN.LSQ.PLAN2D.ASSIGN` (`fid-fit::plan2d_assign`)\n"
        "- **License:** CC-BY-4.0 (synthetic planar profiles and seeded point sets)\n"
        "- **Generator:** `python3 fixtures/fit/make_assign_fixtures.py` (verify with `--check`)\n"
        "\n"
        "## Files\n"
        "\n"
        "- `slot.json`: 4-segment CCW slot with 5 000 seeded points (including 5 exact equidistant tie points).\n"
        "- `rounded_rect.json`: 8-segment CCW rounded rectangle with 5 000 seeded points (including 9 exact tie points).\n"
    )


def main() -> int:
    check = "--check" in sys.argv[1:]
    files = {
        OUT / "slot.json": json.dumps(build_slot(), separators=(",", ":"), sort_keys=True) + "\n",
        OUT / "rounded_rect.json": json.dumps(build_rounded_rect(), separators=(",", ":"), sort_keys=True) + "\n",
        OUT / "README.md": build_readme(),
    }
    if check:
        ok = True
        for path, expected in files.items():
            if not path.exists():
                print(f"MISSING: {path}", file=sys.stderr)
                ok = False
                continue
            if path.read_text(encoding="utf-8") != expected:
                print(f"MISMATCH: {path}", file=sys.stderr)
                ok = False
        return 0 if ok else 1
    OUT.mkdir(parents=True, exist_ok=True)
    for path, content in files.items():
        path.write_text(content, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
