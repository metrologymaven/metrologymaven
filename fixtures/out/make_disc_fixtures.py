#!/usr/bin/env python3
"""Independent reference for KRN.OUT.DISC (excluded-set record and impact disclosure)
fixtures (CC-BY-4.0 project data).

Writes  fixtures/out/disc-record-v1/cases.txt    the inputs: a 20-point plane case (points,
                                                 signed nominal distances, the nominal gate
                                                 that excludes the 3 planted points, the
                                                 least-squares plane and residual statistics
                                                 before and after the exclusion) and the
                                                 expected excluded set, digest, deltas
        fixtures/out/disc-record-v1/record.bin   golden canonical bytes of the record with
                                                 3 excluded points (AC-1)
        fixtures/out/disc-record-v1/empty.bin    golden canonical bytes of the record of a
                                                 gate that excludes nothing (AC-3)

Nothing here runs the Rust code (stdlib only, no third-party data). It is a second
implementation of spec node KRN.OUT.DISC and of the pinned encodings it reuses:

- Points. A 5 x 4 grid on a slightly tilted plane near z = 0, seeded (splitmix64) noise of at
  most 1 um, and 3 planted gross outliers (indices 4, 11, 17) at +0.5, -0.6 and +0.7 mm along
  z. Arithmetic only (no libm transcendental), so the output is the same on every platform.
  The signed nominal distance of each point is its z (nominal plane z = 0).
- The policy is the nominal gate of KRN.OUT.GATE (exclude |d| > max_distance_m) with
  max_distance_m = 1e-4 m, max_excluded_fraction 0.25, target plane. Its canonical encoding
  is written here from the KRN.OUT.POL pinned format (keys in fixed order, f64 as the 16
  lowercase hex digits of its bit pattern).
- The excluded set is checked against the hand labels (the planted indices). Its digest is
  SHA-256 over u32le(point count) then u32le(index) ascending (REQ-KRN.OUT.DISC-001).
- Least-squares planes, by an independent fit: the centroid by fsum, the normal the
  eigenvector of the smallest eigenvalue of the scatter matrix (cyclic Jacobi), normalised and
  sign-normalised by the fid-fit rule (first non-zero component positive). p is the centroid.
  Residuals are the signed distances, statistics by fsum and a correctly rounded sqrt
  (rms, max, min, mean, std_dev with divisor n). These values are inputs of the record (it
  stores them, it does not compute them); the Rust test also checks that fid-fit's PlaneLs
  agrees with them to a tolerance, so the case is a genuine plane fit.
- Deltas (REQ-KRN.OUT.DISC-002, AC-4): after minus before per parameter, in the feature's
  encoding order (plane: p then n), and per residual statistic (rms, max, min, mean,
  std_dev), each one binary64 subtraction (Python floats are binary64), so bit patterns.
- The record encoding (pinned in crates/fid-filter/src/disclose.rs), compact JSON:
  {"format":"filter.exclusion_record@1","algorithm":"<id>","params":<policy spec>,
   "point_count":"<i>","excluded_count":"<i>","excluded":["<i>",...],
   "excluded_digest":"sha256:<hex>","status":"<code>","impact":null | {"before":<fit>,
   "after":<fit>,"delta":[<f>...],"stats_delta":{"rms":<f>,"max":<f>,"min":<f>,"mean":<f>,
   "std_dev":<f>},"opposed_directions":[...]}}
  with <fit> = {"feature":<fid-fit feature encoding>,"stats":{"n":"<i>","rms":<f>,...}}.

The Rust test (crates/fid-filter/tests/disclose.rs) reads these files.

Run:  python fixtures/out/make_disc_fixtures.py          (writes the fixtures)
      python fixtures/out/make_disc_fixtures.py --check  (fails if they are stale)
"""

import hashlib
import math
import struct
import sys
from pathlib import Path

HERE = Path(__file__).parent
DIR = HERE / "disc-record-v1"
MASK64 = 2**64 - 1
SEED = 0x6469_7363_2D72_6563  # "disc-rec"

NX, NY = 5, 4
DX, DY = 0.025, 0.03
TILT_X, TILT_Y = 1.0e-4, -2.0e-4
NOISE = 1.0e-6
PLANTED = {4: 5.0e-4, 11: -6.0e-4, 17: 7.0e-4}
GATE = 1.0e-4
EMPTY_GATE = 1.0e-3
MAX_FRACTION = 0.25
ALGORITHM = "filter.outlier.nominal_gate@1.0.0"
FORMAT = "filter.exclusion_record@1"


def splitmix64(state):
    state = (state + 0x9E3779B97F4A7C15) & MASK64
    z = state
    z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK64
    z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK64
    return state, z ^ (z >> 31)


def hexf(x):
    return "%016x" % struct.unpack("<Q", struct.pack("<d", x))[0]


def points():
    state = SEED
    pts = []
    for j in range(NY):
        for i in range(NX):
            state, u = splitmix64(state)
            noise = ((u >> 11) / 2.0**53 * 2.0 - 1.0) * NOISE
            x = i * DX
            y = j * DY
            z = TILT_X * x + TILT_Y * y + noise
            k = len(pts)
            if k in PLANTED:
                z = z + PLANTED[k]
            pts.append((x, y, z))
    return pts


def jacobi_smallest(a):
    """Eigenvector of the smallest eigenvalue of a symmetric 3x3 matrix (cyclic Jacobi)."""
    a = [row[:] for row in a]
    v = [[1.0 if i == j else 0.0 for j in range(3)] for i in range(3)]
    for _ in range(64):
        off = math.fsum(a[i][j] * a[i][j] for i in range(3) for j in range(3) if i != j)
        if off == 0.0:
            break
        for p in range(3):
            for q in range(p + 1, 3):
                if a[p][q] == 0.0:
                    continue
                theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q])
                t = (1.0 if theta >= 0.0 else -1.0) / (abs(theta) + math.sqrt(theta * theta + 1.0))
                c = 1.0 / math.sqrt(t * t + 1.0)
                s = t * c
                for k in range(3):
                    akp, akq = a[k][p], a[k][q]
                    a[k][p] = c * akp - s * akq
                    a[k][q] = s * akp + c * akq
                for k in range(3):
                    apk, aqk = a[p][k], a[q][k]
                    a[p][k] = c * apk - s * aqk
                    a[q][k] = s * apk + c * aqk
                for k in range(3):
                    vkp, vkq = v[k][p], v[k][q]
                    v[k][p] = c * vkp - s * vkq
                    v[k][q] = s * vkp + c * vkq
    i = min(range(3), key=lambda k: a[k][k])
    return [v[0][i], v[1][i], v[2][i]]


def sign_normalise(u):
    first = next((x for x in u if x != 0.0), None)
    if first is not None and first < 0.0:
        u = [-x for x in u]
    return [x + 0.0 for x in u]


def plane_fit(pts):
    n = len(pts)
    c = [math.fsum(p[k] for p in pts) / n for k in range(3)]
    q = [[p[k] - c[k] for k in range(3)] for p in pts]
    s = [[math.fsum(r[i] * r[j] for r in q) for j in range(3)] for i in range(3)]
    e = jacobi_smallest(s)
    norm = math.sqrt(math.fsum(x * x for x in e))
    nrm = sign_normalise([x / norm for x in e])
    res = [math.fsum(r[k] * nrm[k] for k in range(3)) for r in q]
    return c, nrm, res


def stats(r):
    n = len(r)
    mean = math.fsum(r) / n
    rms = math.sqrt(math.fsum(x * x for x in r) / n)
    std = math.sqrt(math.fsum((x - mean) * (x - mean) for x in r) / n)
    return {"n": n, "rms": rms, "max": max(r), "min": min(r), "mean": mean, "std_dev": std}


STAT_KEYS = ["rms", "max", "min", "mean", "std_dev"]


def jf(x):
    return '"' + hexf(x) + '"'


def ji(v):
    return '"' + str(v) + '"'


def enc_policy(gate):
    return (
        '{"policy":{"kind":"nominal_gate","max_distance_m":' + jf(gate) + "}"
        + ',"max_excluded_fraction":' + jf(MAX_FRACTION) + ',"target":"plane"}'
    )


def enc_plane(p, n):
    return (
        '{"kind":"plane","p":[' + ",".join(jf(x) for x in p) + '],"n":['
        + ",".join(jf(x) for x in n) + "]}"
    )


def enc_stats(st):
    return '{"n":' + ji(st["n"]) + "".join(',"' + k + '":' + jf(st[k]) for k in STAT_KEYS) + "}"


def digest(count, excluded):
    b = struct.pack("<I", count) + b"".join(struct.pack("<I", i) for i in excluded)
    return "sha256:" + hashlib.sha256(b).hexdigest()


def enc_record(gate, count, excluded, impact):
    out = (
        '{"format":"' + FORMAT + '","algorithm":"' + ALGORITHM + '","params":' + enc_policy(gate)
        + ',"point_count":' + ji(count) + ',"excluded_count":' + ji(len(excluded))
        + ',"excluded":[' + ",".join(ji(i) for i in excluded) + "]"
        + ',"excluded_digest":"' + digest(count, excluded) + '","status":"decided","impact":'
    )
    if impact is None:
        out += "null"
    else:
        (bp, bn, bst), (ap, an, ast), delta, sdelta, opposed = impact
        out += (
            '{"before":{"feature":' + enc_plane(bp, bn) + ',"stats":' + enc_stats(bst) + "}"
            + ',"after":{"feature":' + enc_plane(ap, an) + ',"stats":' + enc_stats(ast) + "}"
            + ',"delta":[' + ",".join(jf(x) for x in delta) + "]"
            + ',"stats_delta":{' + ",".join('"' + k + '":' + jf(sdelta[k]) for k in STAT_KEYS) + "}"
            + ',"opposed_directions":[' + ",".join('"' + o + '"' for o in opposed) + "]}"
        )
    out += "}"
    assert out.isascii()
    return out.encode("ascii")


def build():
    pts = points()
    count = len(pts)
    d = [p[2] for p in pts]
    excluded = sorted(i for i, x in enumerate(d) if abs(x) > GATE)
    assert excluded == sorted(PLANTED), excluded  # hand labels: exactly the planted points
    assert not [i for i, x in enumerate(d) if abs(x) > EMPTY_GATE]
    assert len(excluded) / count <= MAX_FRACTION

    bc, bn, bres = plane_fit(pts)
    kept = [p for i, p in enumerate(pts) if i not in excluded]
    ac, an, ares = plane_fit(kept)
    bst, ast = stats(bres), stats(ares)
    assert bst["n"] == count and ast["n"] == count - len(excluded)
    delta = [a - b for a, b in zip(ac + an, bc + bn)]
    sdelta = {k: ast[k] - bst[k] for k in STAT_KEYS}
    dot = math.fsum(x * y for x, y in zip(bn, an))
    opposed = ["n"] if dot < 0.0 else []
    assert opposed == []
    # The outliers move the plane: every delta of the offset and the normal is non-zero
    # except where both fits agree exactly.
    assert any(x != 0.0 for x in delta)

    record = enc_record(GATE, count, excluded, ((bc, bn, bst), (ac, an, ast), delta, sdelta, opposed))
    empty = enc_record(EMPTY_GATE, count, [], None)

    lines = [
        "# KRN.OUT.DISC AC-1, AC-3, AC-4: excluded-set record and impact disclosure.",
        "# Generated by fixtures/out/make_disc_fixtures.py (CC-BY-4.0). Do not edit.",
        "# f64 values are binary64 bit patterns in hex. 'pt' lines: x y z of each point in",
        "# order; 'd': signed nominal distances; 'res': residuals of the fit to all points.",
        "# 'before'/'after': plane p (3) and n (3), then stats n rms max min mean std_dev.",
        "# 'delta': after - before per plane parameter; 'stats_delta': per statistic.",
        "# 'excluded': expected ascending excluded indices; 'digest' their SHA-256 wire form.",
        "count %d" % count,
        "max_distance_m " + hexf(GATE),
        "empty_max_distance_m " + hexf(EMPTY_GATE),
        "max_excluded_fraction " + hexf(MAX_FRACTION),
    ]
    for p in pts:
        lines.append("pt " + " ".join(hexf(x) for x in p))
    lines.append("d " + " ".join(hexf(x) for x in d))
    lines.append("res " + " ".join(hexf(x) for x in bres))

    def fitline(tag, c, n, st):
        return (
            tag + " " + " ".join(hexf(x) for x in c + n) + " " + str(st["n"]) + " "
            + " ".join(hexf(st[k]) for k in STAT_KEYS)
        )

    lines.append(fitline("before", bc, bn, bst))
    lines.append(fitline("after", ac, an, ast))
    lines.append("delta " + " ".join(hexf(x) for x in delta))
    lines.append("stats_delta " + " ".join(hexf(sdelta[k]) for k in STAT_KEYS))
    lines.append("excluded " + " ".join(str(i) for i in excluded))
    lines.append("digest " + digest(count, excluded))
    lines.append("empty_digest " + digest(count, []))
    text = ("\n".join(lines) + "\n").encode("ascii")
    return {"cases.txt": text, "record.bin": record, "empty.bin": empty}


def main():
    check = "--check" in sys.argv[1:]
    files = build()
    stale = []
    for name, data in files.items():
        target = DIR / name
        if check:
            if not target.exists() or target.read_bytes() != data:
                stale.append(name)
        else:
            DIR.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
    if stale:
        print("stale: " + ", ".join(stale) + " (run python fixtures/out/make_disc_fixtures.py)")
        return 1
    print("ok" if check else "written")
    return 0


if __name__ == "__main__":
    sys.exit(main())
