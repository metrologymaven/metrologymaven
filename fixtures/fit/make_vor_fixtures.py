#!/usr/bin/env python3
"""Independent fixture generator for KRN.MZ.VOR (AC-1, exact predicates). CC-BY-4.0 project data.

Stdlib only (zero shared code with `crates/fid-fit`). Writes:
  fixtures/fit/vor-small-v1/cases.txt       500 seeded sets of 5..40 planar points near a circle
                                            (random perturbations, partial arcs, far-from-origin,
                                             exact cocircular, symmetric/tied, near-degenerate)
  fixtures/fit/vor-small-v1/pred_cases.txt  hard exact-sign orient2d and incircle test cases whose
                                            expected signs are computed with exact `fractions.Fraction`

Every coordinate is stored as exact IEEE-754 binary64 hex bits (`struct.pack('>d', x).hex()`) so
Linux, Windows, macOS, ARM64 and wasm32 load bit-identical inputs.

Run:
  python fixtures/fit/make_vor_fixtures.py
  python fixtures/fit/make_vor_fixtures.py --check
"""

import argparse
import math
import struct
import sys
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT_DIR = HERE / "vor-small-v1"
SEED = 0x5EED_4B52_4E56_0001


class SplitMix64:
    def __init__(self, seed: int) -> None:
        self.state = seed & 0xFFFFFFFFFFFFFFFF

    def next_u64(self) -> int:
        self.state = (self.state + 0x9E3779B97F4A7C15) & 0xFFFFFFFFFFFFFFFF
        z = self.state
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & 0xFFFFFFFFFFFFFFFF
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & 0xFFFFFFFFFFFFFFFF
        return (z ^ (z >> 31)) & 0xFFFFFFFFFFFFFFFF

    def uniform(self, lo: float = 0.0, hi: float = 1.0) -> float:
        # 53-bit uniform in [0, 1).
        u = (self.next_u64() >> 11) * (1.0 / (1 << 53))
        return lo + (hi - lo) * u

    def randint(self, lo: int, hi: int) -> int:
        return lo + (self.next_u64() % (hi - lo + 1))


def f64_hex(x: float) -> str:
    if x == 0.0:
        x = 0.0  # normalise -0.0
    return struct.pack(">d", x).hex()


def exact_orient2d_sign(a, b, c) -> int:
    ax, ay = Fraction(a[0]), Fraction(a[1])
    bx, by = Fraction(b[0]), Fraction(b[1])
    cx, cy = Fraction(c[0]), Fraction(c[1])
    det = (ax - cx) * (by - cy) - (ay - cy) * (bx - cx)
    return 1 if det > 0 else (-1 if det < 0 else 0)


def exact_incircle_sign(a, b, c, d) -> int:
    ax, ay = Fraction(a[0]) - Fraction(d[0]), Fraction(a[1]) - Fraction(d[1])
    bx, by = Fraction(b[0]) - Fraction(d[0]), Fraction(b[1]) - Fraction(d[1])
    cx, cy = Fraction(c[0]) - Fraction(d[0]), Fraction(c[1]) - Fraction(d[1])
    al = ax * ax + ay * ay
    bl = bx * bx + by * by
    cl = cx * cx + cy * cy
    det = al * (bx * cy - cx * by) + bl * (cx * ay - ax * cy) + cl * (ax * by - bx * ay)
    return 1 if det > 0 else (-1 if det < 0 else 0)


def generate_cases() -> list[tuple[str, list[tuple[float, float]]]]:
    rng = SplitMix64(SEED)
    cases: list[tuple[str, list[tuple[float, float]]]] = []

    # 0..449: 450 seeded perturbed circles (5..40 points, varied scales, centres, arcs, noise).
    for idx in range(450):
        n = rng.randint(5, 40)
        # Varied radius from 5 mm to 2 m, varied centres (including far-from-origin every 15th case).
        radius = 10.0 ** rng.uniform(-2.3, 0.3)
        if idx % 15 == 0:
            cx = rng.uniform(-100.0, 100.0)
            cy = rng.uniform(-100.0, 100.0)
        else:
            cx = rng.uniform(-1.0, 1.0)
            cy = rng.uniform(-1.0, 1.0)

        arc = 2.0 * math.pi if (idx % 5 != 0) else rng.uniform(0.6 * math.pi, 1.7 * math.pi)
        phase = rng.uniform(0.0, 2.0 * math.pi)
        noise = radius * (10.0 ** rng.uniform(-5.5, -1.0))

        pts = []
        for k in range(n):
            theta = phase + arc * (k + rng.uniform(-0.2, 0.2)) / n
            r = radius + rng.uniform(-noise, noise)
            pts.append((cx + r * math.cos(theta), cy + r * math.sin(theta)))
        cases.append((f"rand_{idx:03d}", pts))

    # 450..469: 20 exact cocircular sets (exact rational Pythagorean points on a circle -> width == 0).
    for idx in range(20):
        n = rng.randint(5, 20)
        radius = 0.05 * (idx + 1)
        cx = 0.25 * (idx - 10)
        cy = -0.125 * idx
        pts = []
        for k in range(n):
            # Rational parametrisation of unit circle: ((1 - t^2)/(1 + t^2), 2t/(1 + t^2))
            t = (k - n / 2.0) * 0.35 + 0.1
            denom = 1.0 + t * t
            px = cx + radius * ((1.0 - t * t) / denom)
            py = cy + radius * ((2.0 * t) / denom)
            pts.append((px, py))
        cases.append((f"cocirc_{idx:02d}", pts))

    # 470..484: 15 exact symmetric / tied configurations (regular polygons & symmetric rings).
    for idx in range(15):
        half = 3 + (idx % 6)  # 6..16 points
        r_in = 0.1 * (1 + (idx % 4))
        r_out = r_in * (1.0 + 0.02 * (1 + (idx % 5)))
        pts = []
        for k in range(2 * half):
            theta = math.pi * k / half
            r = r_in if (k % 2 == 0) else r_out
            pts.append((r * math.cos(theta), r * math.sin(theta)))
        cases.append((f"sym_{idx:02d}", pts))

    # 485..499: 15 near-collinear / near-cocircular hard sets (stressing exact predicates).
    for idx in range(15):
        n = rng.randint(6, 18)
        r = 1.0
        eps = 10.0 ** (-(7 + (idx % 5)))
        pts = []
        for k in range(n):
            theta = 2.0 * math.pi * k / n
            dr = eps if (k % 3 == 0) else (-eps if k % 3 == 1 else 0.0)
            pts.append(((r + dr) * math.cos(theta), (r + dr) * math.sin(theta)))
        cases.append((f"hard_{idx:02d}", pts))

    assert len(cases) == 500
    return cases


def generate_pred_cases() -> str:
    lines = [
        "# Exact-sign orient2d and incircle predicate test cases (expected sign via fractions.Fraction).",
        "# Format: orient <ax> <ay> <bx> <by> <cx> <cy> <sign>",
        "#         incirc <ax> <ay> <bx> <by> <cx> <cy> <dx> <dy> <sign>",
    ]
    # Hard near-collinear orient2d cases around Shewchuk's classic 2^-53 boundary.
    orient_tests = [
        ((0.0, 0.0), (1.0, 1.0), (2.0, 2.0)),
        ((0.0, 0.0), (1.0, 1.0), (2.0, math.nextafter(2.0, 3.0))),
        ((0.0, 0.0), (1.0, 1.0), (2.0, math.nextafter(2.0, 1.0))),
        ((1e-8, 1e-8), (1.0, 1.0), (2.0, 2.0)),
        ((0.5, 0.5), (12.0, 12.0), (24.0, math.nextafter(24.0, 25.0))),
        ((0.5, 0.5), (12.0, 12.0), (24.0, math.nextafter(24.0, 23.0))),
        ((1.0 + 2.0**-52, 1.0 + 2.0**-52), (2.0 + 2.0**-51, 2.0 + 2.0**-51), (3.0 + 3.0 * 2.0**-52, 3.0 + 3.0 * 2.0**-52)),
        ((0.1, 0.2), (0.3, 0.6), (0.5, 1.0)),
        ((1.0, 0.0), (0.0, 1.0), (-1.0, 0.0)),
        ((1.0, 0.0), (-1.0, 0.0), (0.0, 1.0)),
    ]
    for a, b, c in orient_tests:
        s = exact_orient2d_sign(a, b, c)
        coords = " ".join(f64_hex(v) for p in (a, b, c) for v in p)
        lines.append(f"orient {coords} {s}")

    # Hard near-cocircular incircle cases (exact cocircular & 1-ULP perturbed).
    incirc_tests = [
        ((1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)),
        ((1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, math.nextafter(-1.0, 0.0))),
        ((1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, math.nextafter(-1.0, -2.0))),
        ((0.6, 0.8), (-0.8, 0.6), (-0.6, -0.8), (0.8, -0.6)),
        ((0.6, 0.8), (-0.8, 0.6), (-0.6, -0.8), (math.nextafter(0.8, 0.0), -0.6)),
        ((0.6, 0.8), (-0.8, 0.6), (-0.6, -0.8), (math.nextafter(0.8, 2.0), -0.6)),
        ((3.0, 4.0), (-4.0, 3.0), (-3.0, -4.0), (4.0, -3.0)),
        ((3.0, 4.0), (-4.0, 3.0), (-3.0, -4.0), (4.0, math.nextafter(-3.0, 0.0))),
        ((3.0, 4.0), (-4.0, 3.0), (-3.0, -4.0), (4.0, math.nextafter(-3.0, -4.0))),
        ((0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)),
    ]
    for a, b, c, d in incirc_tests:
        s = exact_incircle_sign(a, b, c, d)
        coords = " ".join(f64_hex(v) for p in (a, b, c, d) for v in p)
        lines.append(f"incirc {coords} {s}")

    return "\n".join(lines) + "\n"


def render_cases_txt(cases: list[tuple[str, list[tuple[float, float]]]]) -> str:
    lines = [
        "# Seeded 500-case fixture set for KRN.MZ.VOR (AC-1). Generated by make_vor_fixtures.py.",
        "# Format: case <name> <n> followed by n pairs of big-endian IEEE-754 f64 hex words (x y).",
    ]
    for name, pts in cases:
        hex_coords = " ".join(f"{f64_hex(x)} {f64_hex(y)}" for x, y in pts)
        lines.append(f"case {name} {len(pts)} {hex_coords}")
    return "\n".join(lines) + "\n"


def expected_outputs() -> dict[str, bytes]:
    cases = generate_cases()
    return {
        "cases.txt": render_cases_txt(cases).encode("utf-8"),
        "pred_cases.txt": generate_pred_cases().encode("utf-8"),
    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--check", action="store_true", help="Verify fixtures match regenerated bytes")
    args = ap.parse_args()

    outputs = expected_outputs()
    if args.check:
        ok = True
        for name, data in sorted(outputs.items()):
            p = OUT_DIR / name
            if not p.exists():
                print(f"MISSING {p}", file=sys.stderr)
                ok = False
            elif p.read_bytes() != data:
                print(f"DRIFT {p}", file=sys.stderr)
                ok = False
        return 0 if ok else 1

    OUT_DIR.mkdir(parents=True, exist_ok=True)
    for name, data in sorted(outputs.items()):
        (OUT_DIR / name).write_bytes(data)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
