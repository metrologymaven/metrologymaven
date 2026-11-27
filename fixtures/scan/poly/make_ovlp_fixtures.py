#!/usr/bin/env python3
"""Independent reference for SCAN.POLY.MULTI.OVLP (per-point overlap between scans). CC0-1.0 data.

Stdlib only. Writes four cases into this folder as `<case>.ovlp.txt`; `--check` regenerates
everything in memory and fails on any difference with the committed files. Nothing here was
produced by running the Rust crate `fid-scan-poly`. All point data is synthetic (invented,
generated with integer splitmix64 and IEEE `+ - * /` and `math.sqrt`), not measured; the
written decimal values are what the crate reads, so nothing depends on how a platform
evaluates them.

Cases:

* `two_sheets_overlap` (AC-1): two registered scans of a flat plate, scan 0 over
  x in [0, 30] mm and scan 1 over x in [10.125, 39.875] mm on offset 0.25 mm grids, so
  they overlap on the 20 mm strip x in [10, 30] mm; z noise of +-0.02 mm and normals
  tilted by up to about 3 degrees; d = 0.3 mm.
* `thin_wall_two_scans` (AC-2): the two sides of a 1 mm thick wall: scan 0 measures the
  front face (z = 0, normals about -z), scan 1 the back face (z = 1 mm, normals about +z)
  and also a small strip of the front face near x = 0. d = 1.5 mm, larger than the wall
  thickness, so only the normals keep the sides apart.
* `normals_mix` (REQ-001, REQ-002 edge cases on dyadic coordinates, d = 2^-10 m): a
  neighbour at exactly d (counts), a normal at exactly 60 degrees (dot exactly 0.5, does
  not count), a normal just inside 60 degrees, a neighbour without a valid normal (does not
  count for a point with a normal), a point without a valid normal (any neighbour within d
  counts), a neighbour just beyond d, and an empty fourth scan.
* `many_scans`: 66 scans (more than 64, the sorted-list representation): one isolated
  point per scan plus a hub where scans 0, 33, 64 and 65 meet with mixed normals.
* `sixty_four_scans`: exactly 64 scans (the top of the 64-bit mask range), laid out like
  `many_scans` with the hub on scans 0, 31, 62 and 63, so bit 63 is used.

The expected result follows REQ-SCAN.POLY.MULTI.OVLP-001 and -002 and the pinned rules of
the crate's `multi::overlap` module documentation, computed here by brute force over all
point pairs of all scan pairs (no k-d tree and no bounding-box prefilter):

* dist2 = (dx*dx + dy*dy) + dz*dz with dx = p_x - q_x (Python floats are IEEE binary64 and
  never fused), within d when dist2 <= d*d (d*d rounded once);
* a point with a valid normal n counts a neighbour q of scan j only when q has a valid
  normal m and (n0*m0 + n1*m1) + n2*m2 > 0.5; a point without a valid normal counts every
  neighbour within d;
* status: overlapping (some scan counted), normals_incompatible (some other scan has a point
  within d, none counted), else nothing_within_d;
* pairs: for each ordered pair (i, j), the number of points of scan i that j overlaps,
  only non-zero pairs, ascending.

Hand-labelled checks from the acceptance criteria are asserted here as well (AC-1: points
inside the strip overlap, points more than d outside it do not; AC-2: no back-face point
overlaps, and front-face points away from the front strip are normals_incompatible).

File format (one case per file; '#' lines are comments):

    case <name>
    d <metres>
    scans <count>
    scan <index> points <n>
    <x> <y> <z> <nx> <ny> <nz> <valid 0|1> <status> <ascending scans, comma separated, or ->
    ...                                   (n lines per scan, in point index order)
    pairs <m>
    <scan> <other> <points>
    summary
    <scan> <points> <overlapping> <nothing_within_d> <normals_incompatible> <without_normal>

A point whose valid flag is 0 still carries a unit normal (the cloud has one normal column
per point); the flag says it is not to be used, and the cases give such points misleading
normals so that using them would change the result.
"""

import math
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
M64 = (1 << 64) - 1
NORMAL_TOLERANCE = 1e-9


class SplitMix:
    def __init__(self, seed):
        self.s = seed & M64

    def next(self):
        self.s = (self.s + 0x9E3779B97F4A7C15) & M64
        z = self.s
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & M64
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & M64
        return z ^ (z >> 31)

    def unit(self):
        """A float in [-1, 1)."""
        return ((self.next() >> 11) * (2.0 ** -53)) * 2.0 - 1.0


def dist2(p, q):
    dx = p[0] - q[0]
    dy = p[1] - q[1]
    dz = p[2] - q[2]
    return (dx * dx + dy * dy) + dz * dz


def dot(a, b):
    return (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]


def is_unit(n):
    s = n[0] * n[0] + n[1] * n[1] + n[2] * n[2]
    return abs(math.sqrt(s) - 1.0) <= NORMAL_TOLERANCE


def tilted(g, sign, amount):
    """A unit normal near (0, 0, sign), tilted by up to `amount` per axis before normalising."""
    tx = g.unit() * amount
    ty = g.unit() * amount
    length = math.sqrt((tx * tx + ty * ty) + 1.0)
    return (tx / length, ty / length, sign / length)


def shuffled(g, pts):
    keyed = sorted((g.next(), k) for k in range(len(pts)))
    return [pts[k] for _, k in keyed]


# A point is (position, normal, valid).


def case_two_sheets():
    g = SplitMix(0x0E5_1A_2024)
    s0, s1 = [], []
    for k in range(121):
        for r in range(5):
            p = (k * 0.00025, r * 0.00025, g.unit() * 0.00002)
            s0.append((p, tilted(g, 1.0, 0.05), True))
    for k in range(120):
        for r in range(4):
            p = (0.010125 + k * 0.00025, 0.000125 + r * 0.00025, g.unit() * 0.00002)
            s1.append((p, tilted(g, 1.0, 0.05), True))
    return 0.0003, [shuffled(g, s0), shuffled(g, s1)]


def case_thin_wall():
    g = SplitMix(0x7A11_0001)
    front, back = [], []
    for k in range(21):
        for r in range(5):
            p = (k * 0.0005, r * 0.0005, g.unit() * 0.00002)
            front.append((p, tilted(g, -1.0, 0.05), True))
    for k in range(20):
        for r in range(4):
            p = (0.00025 + k * 0.0005, 0.00025 + r * 0.0005, 0.001 + g.unit() * 0.00002)
            back.append((p, tilted(g, 1.0, 0.05), True))
    strip = []
    for k in range(6):
        for r in range(5):
            p = (0.000125 + k * 0.0005, 0.000125 + r * 0.0005, g.unit() * 0.00002)
            strip.append((p, tilted(g, -1.0, 0.05), True))
    return 0.0015, [shuffled(g, front), shuffled(g, back + strip)]


def case_normals_mix():
    u = 2.0 ** -10
    up, down = (0.0, 0.0, 1.0), (0.0, 0.0, -1.0)
    sixty = (math.sqrt(0.75), 0.0, 0.5)  # dot with +z is exactly 0.5: not below 60 degrees
    c = 0.5 + 2.0 ** -20
    inside = (math.sqrt(1.0 - c * c), 0.0, c)  # dot with +z is c > 0.5
    assert dot(up, sixty) == 0.5 and dot(up, inside) > 0.5
    s0 = [
        ((0.0, 0.0, 0.0), up, True),  # p0
        ((1.0, 0.0, 0.0), down, False),  # p1: no valid normal (its stored normal is ignored)
        ((2.0, 0.0, 0.0), up, True),  # p2
        ((3.0, 0.0, 0.0), up, True),  # p3: alone
    ]
    s1 = [
        ((u, 0.0, 0.0), sixty, True),  # exactly d from p0, exactly 60 degrees: does not count
        ((u / 2.0, 0.0, 0.0), up, False),  # within d of p0 but no valid normal: does not count
        ((1.0, 0.0, u), down, True),  # exactly d from p1 (no normal): counts
        ((2.0, 0.0, u / 2.0), down, True),  # within d of p2, opposed
    ]
    s2 = [
        ((0.0, u, 0.0), inside, True),  # exactly d from p0, just inside 60 degrees: counts
        ((1.0 + u + 2.0 ** -30, 0.0, 0.0), up, True),  # just beyond d from p1
        ((5.0, 0.0, 0.0), up, True),  # alone
    ]
    return u, [s0, s1, s2, []]


def hub_scans(count, a, b, c, dd):
    """`count` scans with one isolated point each; a hub on scans a (up), b (no valid
    normal), c (up, near a) and dd (down, inserted first)."""
    u = 2.0 ** -10
    e = 2.0 ** -12
    up, down = (0.0, 0.0, 1.0), (0.0, 0.0, -1.0)
    scans = [[((float(j), 10.0, 0.0), up, True)] for j in range(count)]
    scans[a].append(((100.0, 0.0, 0.0), up, True))
    scans[c].append(((100.0 + e, 0.0, 0.0), up, True))
    scans[dd].insert(0, ((100.0, e, 0.0), down, True))
    scans[b].append(((100.0, 0.0, e), down, False))
    assert 2.0 * e < u
    return u, scans


def case_many_scans():
    return hub_scans(66, 0, 33, 64, 65)


def case_sixty_four_scans():
    return hub_scans(64, 0, 31, 62, 63)


def cases():
    return [
        ("two_sheets_overlap", case_two_sheets, "AC-1: two scans of a plate overlapping on a 20 mm strip, d = 0.3 mm"),
        ("thin_wall_two_scans", case_thin_wall, "AC-2: the two sides of a 1 mm wall (normals opposed), d = 1.5 mm"),
        ("normals_mix", case_normals_mix, "REQ-001/-002 ties and edge cases, dyadic coordinates, d = 2^-10 m"),
        ("many_scans", case_many_scans, "66 scans (sorted-list sets), a hub of four scans with mixed normals"),
        ("sixty_four_scans", case_sixty_four_scans, "64 scans (64-bit mask sets, bit 63 used), a hub of four scans"),
    ]


def solve(d, scans):
    """Brute force: per point, the ascending overlapping scans and the status."""
    d2 = d * d
    out = []
    for i, si in enumerate(scans):
        rows = []
        for p, n, valid in si:
            assert all(math.isfinite(c) for c in p) and is_unit(n)
            marked, within = [], False
            for j, sj in enumerate(scans):
                if j == i:
                    continue
                hit = False
                for q, m, qvalid in sj:
                    if dist2(p, q) <= d2:
                        within = True
                        if not valid or (qvalid and dot(n, m) > 0.5):
                            hit = True
                if hit:
                    marked.append(j)
            if marked:
                status = "overlapping"
            elif within:
                status = "normals_incompatible"
            else:
                status = "nothing_within_d"
            rows.append((marked, status))
        out.append(rows)
    return out


def hand_checks(name, d, scans, res):
    """The acceptance criteria's own labels, independent of the brute force rule above."""
    if name == "two_sheets_overlap":
        for i in range(2):
            for (p, _, _), (marked, status) in zip(scans[i], res[i]):
                x = p[0]
                if 0.010 <= x <= 0.030:
                    assert marked == [1 - i] and status == "overlapping", (i, p)
                if x < 0.010 - d or x > 0.030 + d:
                    assert marked == [] and status == "nothing_within_d", (i, p)
    if name == "thin_wall_two_scans":
        for (p, _, _), (marked, status) in zip(scans[1], res[1]):
            if p[2] > 0.0005:  # back face
                assert marked == [] and status == "normals_incompatible", p
        for (p, _, _), (marked, status) in zip(scans[0], res[0]):
            if p[0] > 0.0042:  # beyond d of the front strip of scan 1
                assert marked == [] and status == "normals_incompatible", p
    if name == "normals_mix":
        expect = [
            [([2], "overlapping"), ([1], "overlapping"), ([], "normals_incompatible"), ([], "nothing_within_d")],
            [([], "normals_incompatible"), ([0], "overlapping"), ([], "normals_incompatible"), ([], "normals_incompatible")],
            [([0], "overlapping"), ([], "nothing_within_d"), ([], "nothing_within_d")],
            [],
        ]
        assert res == expect, res
    if name in ("many_scans", "sixty_four_scans"):
        a, b, c, dd = (0, 33, 64, 65) if name == "many_scans" else (0, 31, 62, 63)
        hub = {a: ([c], "overlapping"), b: ([a, c, dd], "overlapping"), c: ([a], "overlapping"),
               dd: ([], "normals_incompatible")}
        for j, rows in enumerate(res):
            for (p, _, _), row in zip(scans[j], rows):
                if p[1] == 10.0:
                    assert row == ([], "nothing_within_d"), (j, p)
                else:
                    assert row == hub[j], (j, p, row)


def render(name, d, scans, note):
    res = solve(d, scans)
    hand_checks(name, d, scans, res)
    lines = [
        "# SCAN.POLY.MULTI.OVLP fixture, written by make_ovlp_fixtures.py (stdlib reference; CC0-1.0).",
        "# Synthetic data, not measured. " + note + ".",
        "case " + name,
        "d " + repr(d),
        "scans %d" % len(scans),
    ]
    pairs = {}
    summary = []
    for i, (si, rows) in enumerate(zip(scans, res)):
        lines.append("scan %d points %d" % (i, len(si)))
        cnt = {"overlapping": 0, "nothing_within_d": 0, "normals_incompatible": 0}
        without = 0
        for (p, n, valid), (marked, status) in zip(si, rows):
            cnt[status] += 1
            without += 0 if valid else 1
            for j in marked:
                pairs[(i, j)] = pairs.get((i, j), 0) + 1
            cols = [repr(c) for c in p] + [repr(c) for c in n]
            cols += ["1" if valid else "0", status, ",".join(str(j) for j in marked) or "-"]
            lines.append(" ".join(cols))
        summary.append("%d %d %d %d %d %d" % (i, len(si), cnt["overlapping"], cnt["nothing_within_d"],
                                               cnt["normals_incompatible"], without))
    lines.append("pairs %d" % len(pairs))
    for (i, j) in sorted(pairs):
        lines.append("%d %d %d" % (i, j, pairs[(i, j)]))
    lines.append("summary")
    lines.extend(summary)
    return "\n".join(lines) + "\n"


def main():
    check = "--check" in sys.argv[1:]
    bad = 0
    for name, make, note in cases():
        d, scans = make()
        text = render(name, d, scans, note)
        path = os.path.join(HERE, name + ".ovlp.txt")
        if check:
            try:
                with open(path, encoding="utf-8", newline="") as f:
                    old = f.read()
            except OSError:
                old = None
            if old != text:
                print("DIFFERS: " + path)
                bad += 1
        else:
            with open(path, "w", encoding="utf-8", newline="\n") as f:
                f.write(text)
            print("wrote " + path)
    if bad:
        sys.exit(1)
    if check:
        print("ovlp fixtures: all match")


if __name__ == "__main__":
    main()
