#!/usr/bin/env python3
"""Independent reference for KRN.OUT.GATE (distance-to-nominal and tolerance-band policies)
fixtures (CC-BY-4.0 project data).

Writes  fixtures/out/gate-band-v1/cases.txt   2 000 signed nominal distances (binary64 hex,
                                              in point order), the band and gate parameters,
                                              the expected excluded index sets with their
                                              digests, and hand labels for the planted points

Nothing here runs the Rust code (stdlib only). It is a second implementation of the rules of
spec node KRN.OUT.GATE:

- tolerance band (REQ-KRN.OUT.GATE-002): keep exactly the points with lower_m <= d <= upper_m,
  inclusive limits, exclude all others;
- nominal gate (REQ-KRN.OUT.GATE-001): exclude each point with |d| > max_distance_m (a point
  exactly at the gate is kept);
- the excluded-set digest of KRN.OUT.POL: SHA-256(u32le(point_count) || u32le(index)...) over
  the ascending excluded indices, from hashlib.

The distances are 2 000 values: planted points (exact limits, one ulp inside and outside each
limit, signed zeros, subnormals, gross values) at fixed indices, and a seeded splitmix64 fill
drawn uniformly from [-1.5e-4, 2.5e-4) m. Every planted point carries a hand label (keep or
exclude, for the band and for the gate) written below from the spec's rules, not computed; the
model's verdict must equal each label, so the labels and the model check each other.

The Rust test (crates/fid-filter/tests/gate.rs) reads cases.txt.

Run:  python fixtures/out/make_gate_fixtures.py          (writes the fixture)
      python fixtures/out/make_gate_fixtures.py --check  (fails if the fixture is stale)
"""

import hashlib
import math
import struct
import sys
from pathlib import Path

HERE = Path(__file__).parent
OUT = HERE / "gate-band-v1" / "cases.txt"

COUNT = 2000
LOWER = -0.0001
UPPER = 0.0002
GATE = 0.0001
SEED = 0x6761_7465_2D62_616E  # "gate-ban"
FILL_LO = -1.5e-4
FILL_SPAN = 4.0e-4
STRIDE = 101  # planted point k sits at index 5 + k * STRIDE
MASK64 = 2**64 - 1

INF = math.inf
MIN_SUB = 5e-324
MAX_FIN = sys.float_info.max

# (note, value, band label, gate label): hand labels from the rules above.
PLANTED = [
    ("lower limit", LOWER, "keep", "keep"),  # |-1e-4| == gate: on the gate, kept
    ("one ulp below lower", math.nextafter(LOWER, -INF), "exclude", "exclude"),
    ("one ulp above lower", math.nextafter(LOWER, INF), "keep", "keep"),
    ("upper limit", UPPER, "keep", "exclude"),
    ("one ulp above upper", math.nextafter(UPPER, INF), "exclude", "exclude"),
    ("one ulp below upper", math.nextafter(UPPER, -INF), "keep", "exclude"),
    ("plus gate", GATE, "keep", "keep"),
    ("one ulp above plus gate", math.nextafter(GATE, INF), "keep", "exclude"),
    ("one ulp below plus gate", math.nextafter(GATE, -INF), "keep", "keep"),
    ("minus gate", -GATE, "keep", "keep"),
    ("one ulp below minus gate", math.nextafter(-GATE, -INF), "exclude", "exclude"),
    ("positive zero", 0.0, "keep", "keep"),
    ("negative zero", -0.0, "keep", "keep"),
    ("smallest positive subnormal", MIN_SUB, "keep", "keep"),
    ("smallest negative subnormal", -MIN_SUB, "keep", "keep"),
    ("plus one metre", 1.0, "exclude", "exclude"),
    ("minus one metre", -1.0, "exclude", "exclude"),
    ("largest finite", MAX_FIN, "exclude", "exclude"),
    ("most negative finite", -MAX_FIN, "exclude", "exclude"),
]


def fail(msg):
    print(f"make_gate_fixtures: {msg}", file=sys.stderr)
    sys.exit(1)


def bits(x):
    return format(struct.unpack("<Q", struct.pack("<d", x))[0], "016x")


def splitmix64(state):
    state = (state + 0x9E3779B97F4A7C15) & MASK64
    z = state
    z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK64
    z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK64
    return state, z ^ (z >> 31)


def distances():
    planted_at = {5 + k * STRIDE: k for k in range(len(PLANTED))}
    if max(planted_at) >= COUNT:
        fail("planted points do not fit")
    out, state = [], SEED
    for i in range(COUNT):
        state, z = splitmix64(state)
        if i in planted_at:
            out.append(PLANTED[planted_at[i]][1])
        else:
            u = (z >> 11) * 2.0**-53
            out.append(FILL_LO + u * FILL_SPAN)
    return out, planted_at


def band_excluded(d):
    return not (LOWER <= d <= UPPER)


def gate_excluded(d):
    return abs(d) > GATE


def digest(count, indices):
    pre = struct.pack("<I", count) + b"".join(struct.pack("<I", i) for i in indices)
    return "sha256:" + hashlib.sha256(pre).hexdigest()


def build():
    ds, planted_at = distances()
    for x in ds:
        if not math.isfinite(x):
            fail("non-finite distance")
    band = [i for i, x in enumerate(ds) if band_excluded(x)]
    gate = [i for i, x in enumerate(ds) if gate_excluded(x)]
    labels = []
    for i in sorted(planted_at):
        note, value, band_label, gate_label = PLANTED[planted_at[i]]
        b = "exclude" if band_excluded(value) else "keep"
        g = "exclude" if gate_excluded(value) else "keep"
        if (b, g) != (band_label, gate_label):
            fail(f"{note}: labels {band_label}/{gate_label}, model {b}/{g}")
        labels.append(f"label {i} {band_label} {gate_label} {note}")
    # Each limit has a point exactly on it and one ulp outside it, so both inclusive
    # comparisons are exercised in both directions.
    for want in (LOWER, UPPER, GATE, -GATE):
        if want not in ds:
            fail(f"limit {want!r} not planted")
    out = [
        "# KRN.OUT.GATE AC-1, AC-2, AC-4: tolerance band and nominal gate masks.",
        "# Generated by fixtures/out/make_gate_fixtures.py (CC-BY-4.0). Do not edit.",
        "# f64 values are binary64 bit patterns in hex. 'd' lines are the signed nominal",
        "# distances in point order (metres). '*_excluded' are the expected ascending excluded",
        "# indices, '*_digest' SHA-256(u32le(count) || u32le(index)...). 'label' lines are hand",
        "# labels of planted points: index, band verdict, gate verdict, note.",
        f"count {COUNT}",
        f"lower_m {bits(LOWER)}",
        f"upper_m {bits(UPPER)}",
        f"max_distance_m {bits(GATE)}",
    ]
    out += [f"d {bits(x)}" for x in ds]
    out.append("band_excluded" + "".join(f" {i}" for i in band))
    out.append(f"band_digest {digest(COUNT, band)}")
    out.append("gate_excluded" + "".join(f" {i}" for i in gate))
    out.append(f"gate_digest {digest(COUNT, gate)}")
    out += labels
    return "\n".join(out) + "\n"


def main():
    text = build()
    if "--check" in sys.argv[1:]:
        old = OUT.read_text(encoding="utf-8").replace("\r\n", "\n") if OUT.exists() else ""
        if old != text:
            print(f"stale: {OUT}", file=sys.stderr)
            return 1
        print("ok")
        return 0
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(text, encoding="utf-8", newline="\n")
    print(f"wrote {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
