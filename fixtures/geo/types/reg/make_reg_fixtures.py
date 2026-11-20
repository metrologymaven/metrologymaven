#!/usr/bin/env python3
"""Independent reference for GEO.TYPES.REG (CC-BY-4.0 project data).

Writes fixtures/geo/types/reg/expected.json: the exact bytes `geo.describe_element@1` must
return for the fixtures `pointset_small` and `mesh_cube` (AC-1, AC-2), the UUIDv5 of a
derived element (AC-4) and the entity ID of an element imported with a non-UUID source ID
(AC-4). Everything is computed here from the leaf's Design and the fixture files, with the
Python standard library only; nothing is taken from a run of the Rust code.

Rules applied (spec/geo/types/reg/NODE.md, Design; pinned in crates/fid-geo-cmd):
- The result is RFC 8785 canonical JSON: keys sorted, no whitespace, numbers in the
  ECMAScript Number-to-String form.
- `kind` is the type name (`PointSet`, `PointCloud`, `Mesh`, `CadBodyRef`); `points` is the
  point (or vertex) count; `triangles` is the triangle count of a mesh and null otherwise;
  `aabb` is `{"max": [x, y, z], "min": [x, y, z]}` in metres; `frame` is the 16 frame-ID
  bytes as a lowercase hyphenated UUID; `digest` is `sha256:` plus the hex of the GEO.TYPES
  digest vector; `channels` are the per-point channel names present; `topology` is the mesh
  topology facts object (null for other kinds); `id` is the entity ID and `source_id` the
  imported source ID kept verbatim (null when none).
- A derived ID is UUIDv5 (RFC 9562) in the project namespace over the canonical JSON
  `{"command": <command ID>, "inputs": [{"digest": d, "id": i}, ...]}`, inputs sorted by ID.
- An imported source ID that is not a canonical UUID is kept verbatim as `source_id`; the
  entity ID is UUIDv5 in the project namespace over `{"source_id": <text>}`.

Run:  python fixtures/geo/types/reg/make_reg_fixtures.py [--check]
"""

import json
import struct
import sys
import uuid
from decimal import Decimal
from pathlib import Path

HERE = Path(__file__).resolve().parent
TYPES = HERE.parent
OUT = HERE / "expected.json"

# Fixture constants (hand-chosen, canonical lowercase UUIDs).
NAMESPACE = "8e3f6a52-1c2d-4b7e-9f10-2a3b4c5d6e7f"
POINTSET_ID = "0190a5b2-7c3d-7000-8000-000000000001"
MESH_ID = "0190a5b2-7c3d-7000-8000-000000000002"
DERIVE_COMMAND = "geo.test_vertices_to_points@1"
IMPORTED_TEXT = "step:#123"


def es_number(x):
    """ECMAScript Number::toString for a finite double (RFC 8785 section 3.2.2.3)."""
    if x == 0:
        return "0"
    sign = "-" if x < 0 else ""
    t = Decimal(repr(abs(x))).normalize().as_tuple()
    s = "".join(str(d) for d in t.digits)
    k = len(s)
    n = t.exponent + k
    if k <= n <= 21:
        body = s + "0" * (n - k)
    elif 0 < n <= 21:
        body = s[:n] + "." + s[n:]
    elif -6 < n <= 0:
        body = "0." + "0" * (-n) + s
    else:
        e = n - 1
        es = ("+" if e >= 0 else "-") + str(abs(e))
        body = (s if k == 1 else s[0] + "." + s[1:]) + "e" + es
    return sign + body


def canon(v):
    """RFC 8785 canonical JSON for the value shapes used here (ASCII strings only)."""
    if v is None:
        return "null"
    if v is True:
        return "true"
    if v is False:
        return "false"
    if isinstance(v, int):
        return es_number(float(v))
    if isinstance(v, float):
        return es_number(v)
    if isinstance(v, str):
        assert all(0x20 <= ord(c) < 0x7F for c in v), v
        return json.dumps(v)
    if isinstance(v, list):
        return "[" + ",".join(canon(i) for i in v) + "]"
    if isinstance(v, dict):
        return "{" + ",".join(json.dumps(k) + ":" + canon(v[k]) for k in sorted(v)) + "}"
    raise TypeError(type(v))


def f64(h):
    return struct.unpack(">d", bytes.fromhex(h))[0]


def hyphenated(hex32):
    return str(uuid.UUID(hex=hex32))


def records(name):
    out = []
    for line in (TYPES / name).read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            out.append(line.split())
    return out


def digest_of(name):
    doc = json.loads((TYPES / "digest_vectors.json").read_text(encoding="utf-8"))
    hits = [v for v in doc["vectors"] if v["name"] == name]
    assert len(hits) == 1, name
    return "sha256:" + hits[0]["sha256"]


def describe_pointset_small():
    rec = {r[0]: r[1:] for r in records("pointset_small.txt") if r[0] != "p"}
    points = [r for r in records("pointset_small.txt") if r[0] == "p"]
    count = int(rec["count"][0])
    assert count == len(points) == 1000
    return {
        "aabb": {"max": [f64(h) for h in rec["aabb_max"]], "min": [f64(h) for h in rec["aabb_min"]]},
        "channels": [],
        "digest": digest_of("pointset_small"),
        "frame": hyphenated(rec["frame"][0]),
        "id": POINTSET_ID,
        "kind": "PointSet",
        "points": count,
        "source_id": POINTSET_ID,
        "topology": None,
        "triangles": None,
    }


def describe_mesh_cube():
    rows = records("mesh_cube.txt")
    frame = [r[1] for r in rows if r[0] == "frame"][0]
    verts = [[float(c) for c in r[1:]] for r in rows if r[0] == "v"]
    tris = [r for r in rows if r[0] == "t"]
    facts = json.loads((TYPES / "mesh_cube.facts.json").read_text(encoding="utf-8"))
    return {
        "aabb": {
            "max": [max(v[i] for v in verts) for i in range(3)],
            "min": [min(v[i] for v in verts) for i in range(3)],
        },
        "channels": [],
        "digest": digest_of("mesh_cube"),
        "frame": hyphenated(frame),
        "id": MESH_ID,
        "kind": "Mesh",
        "points": len(verts),
        "source_id": MESH_ID,
        "topology": facts,
        "triangles": len(tris),
    }


def v5(name):
    return str(uuid.uuid5(uuid.UUID(NAMESPACE), name))


def build():
    ps = describe_pointset_small()
    mc = describe_mesh_cube()
    assert mc["topology"]["closed"] is True and mc["triangles"] == 12
    # Given out of ID order on purpose: the mesh ID sorts after the point set ID.
    given = [[MESH_ID, mc["digest"]], [POINTSET_ID, ps["digest"]]]
    inputs = [{"digest": d, "id": i} for i, d in sorted(given)]
    derive_name = canon({"command": DERIVE_COMMAND, "inputs": inputs})
    import_name = canon({"source_id": IMPORTED_TEXT})
    return {
        "derived": {"command": DERIVE_COMMAND, "id": v5(derive_name), "inputs_given": given, "name": derive_name},
        "describe": {"mesh_cube": canon(mc), "pointset_small": canon(ps)},
        "generator": "fixtures/geo/types/reg/make_reg_fixtures.py (stdlib, independent of fid-geo-cmd)",
        "ids": {"mesh_cube": MESH_ID, "pointset_small": POINTSET_ID},
        "imported_text": {"id": v5(import_name), "name": import_name, "source_id": IMPORTED_TEXT},
        "license": "CC-BY-4.0",
        "namespace": NAMESPACE,
        "spec": "GEO.TYPES.REG",
    }


def self_test():
    # ECMAScript Number-to-String examples (ECMA-262 and RFC 8785 Appendix B).
    cases = [(0.0, "0"), (-0.0, "0"), (1.0, "1"), (1e21, "1e+21"), (1e20, "100000000000000000000"),
             (1e-7, "1e-7"), (1e-6, "0.000001"), (0.5, "0.5"), (123.456, "123.456"),
             (5e-324, "5e-324"), (1.7976931348623157e308, "1.7976931348623157e+308"),
             (-1.5e-10, "-1.5e-10"), (333333333.33333329, "333333333.3333333")]
    for x, want in cases:
        assert es_number(x) == want, (x, es_number(x), want)
    # RFC 9562 Appendix A.4 UUIDv5 vector, so the uuid module is the RFC algorithm.
    assert str(uuid.uuid5(uuid.NAMESPACE_DNS, "www.example.com")) == "2ed6657d-e927-568b-95e1-2665a8aea6a2"


def main():
    self_test()
    text = json.dumps(build(), indent=2, sort_keys=True) + "\n"
    if "--check" in sys.argv[1:]:
        have = OUT.read_text(encoding="utf-8") if OUT.exists() else None
        if have != text:
            print(f"{OUT} is stale; run {Path(__file__).name}", file=sys.stderr)
            return 1
        print("ok")
        return 0
    OUT.write_text(text, encoding="utf-8", newline="\n")
    print(f"wrote {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
