#!/usr/bin/env python3
"""Independent reference for KRN.CONS.DECL (construction contract) fixtures (CC-BY-4.0 project data).

Writes fixtures/cons/decl-toy-v1/
  pairs.txt          AC-1: 100 seeded point pairs, one replacement per input, expected midpoints
  canon.txt          AC-2: canonical parameter, declaration, output, event and row bytes, digests
  near-parallel.txt  AC-3: a line 1e-12 rad from a plane, an exactly parallel line, a control

Nothing here runs the Rust code (stdlib only). The canonical byte forms are written with
json.dumps(sort_keys=True, separators=(",", ":")), which equals RFC 8785 for these values (ASCII
strings and booleans only, no numbers); digests come from hashlib.sha256. Midpoints are
(a + b) * 0.5 per coordinate in CPython floats (IEEE-754 binary64, round to nearest even, never
fused), the arithmetic the test-only midpoint construction pins.

Run:  python fixtures/cons/make_decl_fixtures.py          (writes the fixtures)
      python fixtures/cons/make_decl_fixtures.py --check  (fails if a fixture is stale)
"""

import hashlib
import json
import math
import random
import struct
import sys
from pathlib import Path

HERE = Path(__file__).parent / "decl-toy-v1"
SEED = 20261119
N_PAIRS = 100

ALG_MID = "construct.midpoint.point_point@1.0.0"
CMD_MID = "construct.midpoint@1"
ANGULAR = 1e-9
DISTANCE = 1e-6


def hx(x):
    return f"{struct.unpack('<Q', struct.pack('<d', x))[0]:016x}"


def canon(v):
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def digest(text):
    return "sha256:" + hashlib.sha256(text.encode("utf-8")).hexdigest()


def uuid_text(b):
    h = b.hex()
    return f"{h[0:8]}-{h[8:12]}-{h[12:16]}-{h[16:20]}-{h[20:32]}"


def mid(a, b):
    return [(x + y) * 0.5 for x, y in zip(a, b)]


# ---------------------------------------------------------------- AC-1 pairs


def gen_pairs():
    rng = random.Random(SEED)

    def coord():
        # Mixed magnitudes from 1e-3 m to 1e3 m, both signs.
        return (rng.random() * 2.0 - 1.0) * 10.0 ** rng.randint(-3, 3)

    def pt():
        return [coord() for _ in range(3)]

    def eid():
        while True:
            b = bytes(rng.getrandbits(8) for _ in range(16))
            if b != bytes(16):
                return b

    out = []
    for _ in range(N_PAIRS):
        ida, idb = eid(), eid()
        a, b, c, d = pt(), pt(), pt(), pt()
        out.append((ida, idb, a, b, c, d))
    return out


def make_pairs(pairs):
    lines = [
        "# KRN.CONS.DECL AC-1: test-only midpoint construction (CC-BY-4.0).",
        "# id_a id_b a.xyz b.xyz c.xyz d.xyz mid(a,b).xyz mid(c,b).xyz mid(a,d).xyz",
        "# c replaces input a, d replaces input b; every number is the f64 bit pattern in hex.",
    ]
    for ida, idb, a, b, c, d in pairs:
        vals = a + b + c + d + mid(a, b) + mid(c, b) + mid(a, d)
        lines.append(" ".join([uuid_text(ida), uuid_text(idb)] + [hx(v) for v in vals]))
    return "\n".join(lines) + "\n"


# ---------------------------------------------------------------- AC-2 canonical bytes


def params(angular, distance, values=()):
    vals = sorted(values, key=lambda v: v[0])
    unit = {"length": "m", "angle": "rad", "dimensionless": "1", "count": "1"}
    return canon(
        {
            "angular_threshold": hx(angular),
            "distance_threshold": hx(distance),
            "format": "construct.params@1",
            "values": [{"kind": k, "name": n, "unit": unit[k], "value": hx(v)} for n, k, v in vals],
        }
    )


QUANTITY = {"length": "m", "angle": "rad", "dimensionless": "1", "count": "1"}
NUMERIC = {"number", "integer", "number_array", "points"}


def field(name, ty, q=None, required=True):
    f = {"name": name, "required": required, "type": ty}
    if q is not None:
        f["x-quantity"] = q
        f["x-unit"] = QUANTITY[q]
    return f


def check_fields(fields):
    """AC-5, independent of the Rust validator: names unique, a known quantity and its SI
    unit on exactly the numeric fields."""
    names = [f["name"] for f in fields]
    assert len(names) == len(set(names)), names
    for f in fields:
        if f["type"] in NUMERIC:
            assert f.get("x-quantity") in QUANTITY, f
            assert f["x-unit"] == QUANTITY[f["x-quantity"]], f
        else:
            assert "x-quantity" not in f and "x-unit" not in f, f


def make_row():
    prm = [
        field("inputs", "element_ref_array"),
        field("thresholds.angular", "number", "angle"),
        field("thresholds.distance", "number", "length"),
    ]
    res = [
        field("algorithm", "string"),
        field("inputs", "element_ref_array"),
        field("params_digest", "string"),
        field("output_digest", "string"),
        field("element.c", "number_array", "length"),
    ]
    check_fields(prm)
    check_fields(res)
    return canon(
        {
            "algorithms": [ALG_MID],
            "command": CMD_MID,
            "determinism": "D0",
            "dry_run": "R1",
            "params": prm,
            "result": res,
            "row": "construct.command_row@1",
            "side_effect": "W2",
        }
    )


def make_canon(pairs):
    ida, idb, a, b, _, _ = pairs[0]
    p = params(ANGULAR, DISTANCE)
    p_digest = digest(p)
    inputs = [{"id": uuid_text(ida), "role": "a"}, {"id": uuid_text(idb), "role": "b"}]
    decl = canon(
        {
            "algorithm": ALG_MID,
            "format": "construct.dependency_decl@1",
            "inputs": inputs,
            "params_digest": p_digest,
        }
    )
    out = canon({"feature": {"c": [hx(v) for v in mid(a, b)], "kind": "point"}, "format": "construct.element@1"})
    out_digest = digest(out)
    event = canon({"algorithm": ALG_MID, "inputs": inputs, "output_digest": out_digest, "params_digest": p_digest})
    p_ang = params(2e-9, DISTANCE)
    p_dist = params(ANGULAR, 2e-6)
    p_vals = params(ANGULAR, DISTANCE, [("offset", "length", 0.5), ("branch", "count", 1.0)])
    value_out = canon(
        {"format": "construct.element@1", "value": {"kind": "angle", "unit": "rad", "value": hx(0.25)}}
    )
    assert len({p_digest, digest(p_ang), digest(p_dist), digest(p_vals)}) == 4
    rows = [
        ("params_bytes", p),
        ("params_digest", p_digest),
        ("params_angular_2e-9_digest", digest(p_ang)),
        ("params_distance_2e-6_digest", digest(p_dist)),
        ("params_values_bytes", p_vals),
        ("params_values_digest", digest(p_vals)),
        ("decl_bytes", decl),
        ("output_bytes", out),
        ("output_digest", out_digest),
        ("value_output_bytes", value_out),
        ("event_type", CMD_MID),
        ("event_payload", event),
        ("row_bytes", make_row()),
    ]
    head = [
        "# KRN.CONS.DECL AC-2/AC-5: canonical bytes for pair 0 of pairs.txt (CC-BY-4.0).",
        f"# params: angular_threshold {ANGULAR!r} rad, distance_threshold {DISTANCE!r} m;"
        " params_values adds offset = 0.5 m (length) and branch = 1 (count);",
        "# value_output: an angle value of 0.25 rad. One key=value per line.",
    ]
    return "\n".join(head + [f"{k}={v}" for k, v in rows]) + "\n"


# ---------------------------------------------------------------- AC-3 near-parallel line-plane


def make_near_parallel():
    theta = 1e-12
    d = [1.0, 0.0, theta]
    # The direction is unit length in binary64: sqrt(1 + 1e-24) rounds to 1.
    assert math.sqrt(d[0] * d[0] + d[1] * d[1] + d[2] * d[2]) == 1.0
    # sin(theta) and asin(theta) round to theta itself (|theta|^3 / 6 is far below half an ulp).
    assert math.asin(theta) == theta and math.sin(theta) == theta
    cases = [
        # name, line p, line d, plane p, plane n, angular threshold, expected
        ("near_parallel", [0.0, 0.0, 1.0], d, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 1e-9, "ill_conditioned"),
        ("parallel", [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 1e-9, "undefined"),
        ("control", [0.25, 0.5, 1.0], [0.0, 0.0, 1.0], [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 1e-9, "ok"),
    ]
    lines = [
        "# KRN.CONS.DECL AC-3: test-only line-plane intersection (CC-BY-4.0).",
        "# name line.p line.d plane.p plane.n angular_threshold expected value",
        "# value: ill_conditioned -> measured angle asin(|d.n|) (= theta); undefined -> reason;"
        " ok -> intersection point (exact here).",
    ]
    for name, lp, ld, pp, pn, thr, exp in cases:
        if exp == "ill_conditioned":
            value = [hx(theta)]
        elif exp == "undefined":
            value = ["parallel"]
        else:
            dn = sum(x * y for x, y in zip(ld, pn))
            t = sum((x - y) * z for x, y, z in zip(pp, lp, pn)) / dn
            value = [hx(x + t * y) for x, y in zip(lp, ld)]
        nums = [hx(v) for v in lp + ld + pp + pn + [thr]]
        lines.append(" ".join([name] + nums + [exp] + value))
    return "\n".join(lines) + "\n"


def main():
    pairs = gen_pairs()
    files = {
        "pairs.txt": make_pairs(pairs),
        "canon.txt": make_canon(pairs),
        "near-parallel.txt": make_near_parallel(),
    }
    check = "--check" in sys.argv[1:]
    stale = False
    for name, text in files.items():
        path = HERE / name
        data = text.encode("utf-8")
        if check:
            if not path.exists() or path.read_bytes() != data:
                print(f"stale: fixtures/cons/decl-toy-v1/{name}")
                stale = True
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
            print(f"wrote fixtures/cons/decl-toy-v1/{name} ({len(data)} bytes)")
    if stale:
        sys.exit(1)


if __name__ == "__main__":
    main()
