#!/usr/bin/env python3
"""Independent reference for KRN.SOLV.MSTART (CC-BY-4.0 project data).

Writes fixtures/solv/mstart-double-well-v1.txt: the start list, the per-start outcomes of a
toy local solver on an analytic double well, the selected start and the agreement flag, with
golden SHA-256 digests. Nothing here runs or reads the Rust code.

Double well (2 parameters x, y), split at x = 1/8:
    left  well  f(x, y) = (x + 1)^2 + y^2          global minimum 0   at (-1, 0)
    right well  f(x, y) = (x - 1)^2 + y^2 + 1/2    local  minimum 1/2 at ( 1, 0)
The well is chosen by the start's x (x < 1/8: left). The toy local solver halves the distance
to that well's minimiser 60 times per coordinate (d = p - c; d = d * 0.5; p = c + d), then
evaluates that well's objective at the result, as (x - cx) * (x - cx) + y * y [+ 0.5].
Each step is one IEEE-754 double operation, so the values are bit-reproducible.

Starts (REQ-KRN.SOLV.MSTART-001): start 0 is the primary; start i >= 1 is, per parameter j,
primary[j] + ((2 * u - 1) * half_width[j]) with u = uniform_f64 of the fid-num Philox stream
(seed, stream) at sequential index (i - 1) * n + j.

Selection (REQ-003): lowest objective in IEEE totalOrder, then the parameter vector in
lexicographic totalOrder, then the lower start index. Agreement (REQ-004): every successful
outcome within the per-parameter tolerance of the selected one.

Run:  python fixtures/solv/make_mstart_fixtures.py [--check]
"""

import hashlib
import struct
import sys
from pathlib import Path

M0, M1 = 0xD2511F53, 0xCD9E8D57
W0, W1 = 0x9E3779B9, 0xBB67AE85
MASK = 0xFFFFFFFF


def philox(ctr, key):
    c0, c1, c2, c3 = ctr
    k0, k1 = key
    for r in range(10):
        if r:
            k0, k1 = (k0 + W0) & MASK, (k1 + W1) & MASK
        p0, p1 = M0 * c0, M1 * c2
        hi0, lo0, hi1, lo1 = p0 >> 32, p0 & MASK, p1 >> 32, p1 & MASK
        c0, c1, c2, c3 = hi1 ^ c1 ^ k0, lo1, hi0 ^ c3 ^ k1, lo0
    return (c0, c1, c2, c3)


# Published Random123 known answer for philox4x32_10 (kat_vectors), as a self-check.
assert philox((0x243F6A88, 0x85A308D3, 0x13198A2E, 0x03707344), (0xA4093822, 0x299F31D0)) == (
    0xD16CFE09, 0x94FDCCEB, 0x5001E420, 0x24126EA1)


def uniform(seed, stream, index):
    w = philox((index & MASK, index >> 32, stream & MASK, stream >> 32), (seed & MASK, seed >> 32))
    return (((w[1] << 32) | w[0]) >> 11) * 2.0 ** -53


def bits(x):
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def total_key(x):
    """IEEE-754 totalOrder key for a non-NaN double."""
    b = bits(x)
    return (b ^ 0xFFFFFFFFFFFFFFFF) if b >> 63 else (b | (1 << 63))


def starts(primary, half_widths, count, seed, stream):
    n = len(primary)
    out = [list(primary)]
    for i in range(1, count):
        s = []
        for j in range(n):
            u = uniform(seed, stream, (i - 1) * n + j)
            s.append(primary[j] + ((2.0 * u - 1.0) * half_widths[j]))
        out.append(s)
    return out


SPLIT = 0.125


def toy_solve(start):
    x, y = start
    left = x < SPLIT
    cx = -1.0 if left else 1.0
    for _ in range(60):
        d = x - cx
        d = d * 0.5
        x = cx + d
        d = y - 0.0
        d = d * 0.5
        y = 0.0 + d
    dx = x - cx
    obj = dx * dx + y * y
    if not left:
        obj = obj + 0.5
    return ("left" if left else "right"), obj, [x, y]


def select(outcomes):
    best = None
    for i, (_, obj, p) in enumerate(outcomes):
        key = (total_key(obj), [total_key(v) for v in p], i)
        if best is None or key < best[0]:
            best = (key, i)
    return best[1]


def digest_floats(rows):
    h = hashlib.sha256()
    for row in rows:
        for v in row:
            h.update(struct.pack("<d", v))
    return h.hexdigest()


PRIMARY = [0.6, 0.3]
HALF = [2.0, 1.0]
COUNT, SEED, STREAM = 8, 42, 0
TOL = [1e-9, 1e-9]


def build():
    st = starts(PRIMARY, HALF, COUNT, SEED, STREAM)
    outs = [toy_solve(s) for s in st]
    wells = [w for w, _, _ in outs]
    assert "left" in wells and "right" in wells, wells
    assert wells[0] == "right", "the primary start must sit in the shallow well"
    best = select(outs)
    bp = outs[best][2]
    agree = all(abs(p[j] - bp[j]) <= TOL[j] for _, _, p in outs for j in range(2))
    assert outs[best][0] == "left" and not agree
    # Outcome digest: per start, objective then params (LE f64 bits), in index order.
    out_digest = digest_floats([[o] + p for _, o, p in outs])
    lines = [
        "# KRN.SOLV.MSTART AC-1 fixture (CC-BY-4.0), written by make_mstart_fixtures.py; do not edit.",
        "# Double well split at x = 1/8: left (x+1)^2+y^2 (min 0 at (-1,0)), right (x-1)^2+y^2+1/2",
        "# (min 1/2 at (1,0)). Toy solver: 60 halvings towards the well minimiser; see the script.",
        "# Floats are hex IEEE-754 bit patterns. starts_digest: SHA-256 of the LE f64 of every start,",
        "# start-major. outcome_digest: SHA-256 of LE f64 objective then params, per start in order.",
        f"primary {' '.join(f'{bits(v):016x}' for v in PRIMARY)}",
        f"half_widths {' '.join(f'{bits(v):016x}' for v in HALF)}",
        f"count {COUNT}",
        f"seed {SEED}",
        f"stream {STREAM}",
        f"tolerances {' '.join(f'{bits(v):016x}' for v in TOL)}",
    ]
    for i, (s, (w, o, p)) in enumerate(zip(st, outs)):
        lines.append(f"start {i} {' '.join(f'{bits(v):016x}' for v in s)} well {w} objective {bits(o):016x} "
                     f"params {' '.join(f'{bits(v):016x}' for v in p)}")
    lines += [
        f"starts_digest sha256:{digest_floats(st)}",
        f"outcome_digest sha256:{out_digest}",
        f"best_index {best}",
        f"agreement {'true' if agree else 'false'}",
        f"local_optimum_only {'false' if agree else 'true'}",
    ]
    return "\n".join(lines) + "\n"


def main():
    path = Path(__file__).parent / "mstart-double-well-v1.txt"
    text = build()
    if "--check" in sys.argv[1:]:
        if path.read_text(encoding="utf-8") != text:
            print(f"{path} is stale; rerun make_mstart_fixtures.py", file=sys.stderr)
            sys.exit(1)
        print(f"{path.name}: up to date")
        return
    path.write_text(text, encoding="utf-8", newline="\n")
    print(text, end="")


if __name__ == "__main__":
    main()
