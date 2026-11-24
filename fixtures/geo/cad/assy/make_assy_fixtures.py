#!/usr/bin/env python3
"""GEO.CAD.ASSY.MODEL fixtures and their expected IDs (independent Python reference).

Writes `two_bolts.json`, a hand-written assembly description (one invented bolt body, a
shank cylinder and a head cylinder joined by an annular plane, placed twice under one
sub-assembly occurrence), and `expected.json`, the instance ID and the per-instance face,
edge and vertex IDs of each placement.

Independent of fid-geo-cad: the definition entities' canonical order and topology IDs come
from `canonical()` and `enc_id()` of `fixtures/geo/cad/topo/make_topo_fixtures.py` (the
stdlib reference of GEO.CAD.TOPO.MODEL); the instance IDs are UUIDv5 (stdlib `uuid.uuid5`)
re-implemented here from the pinned rules in spec/geo/cad/assy/model/NODE.md (Design):

    path bytes = u32 LE component count | each component's TopologyId encoding v1
    instance ID        = UUIDv5(NS, path bytes)
    instance entity ID = UUIDv5(NS, path bytes | 0x00 | definition TopologyId encoding v1)

The bolt geometry is invented for this fixture (D-038: no third-party CAD files).

Usage: python make_assy_fixtures.py [--check]   (stdlib only; --check fails on drift)
"""

import json
import math
import struct
import sys
import uuid
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "topo"))

from make_topo_fixtures import canonical, enc_id, no_dup_keys  # noqa: E402

LICENSE = "CC-BY-4.0"
TAU = 2.0 * math.pi

NAMESPACE = uuid.uuid5(
    uuid.NAMESPACE_URL,
    "https://github.com/metrologymaven/metrologymaven/spec/geo/cad/assy/model#instance-entity-id-v1",
)


# ------------------------------------------------------------------ the bolt body


def bolt():
    """Shank radius 4 mm from z = 0 to 30 mm, head radius 6.5 mm from 30 to 35 mm, in metres.
    Faces and the four circular edges carry STEP-style labels; the two seam edges and every
    vertex are unlabelled, so their definition IDs are derived (GEO.CAD.TOPO.MODEL)."""
    rs, rh, length, head = 0.004, 0.0065, 0.03, 0.005
    top = length + head

    def frame(o, axis=(0.0, 0.0, 1.0)):
        return {"origin": list(o), "axis": list(axis), "ref_dir": [1.0, 0.0, 0.0]}

    def circle(z, r):
        g = {"type": "circle"}
        g.update(frame((0.0, 0.0, z)))
        g["radius"] = r
        return g

    def line(o):
        g = {"type": "line"}
        g.update(frame(o))
        return g

    def plane(z, axis_z):
        g = {"type": "plane"}
        g.update(frame((0.0, 0.0, z), (0.0, 0.0, axis_z)))
        return g

    def cylinder(z, r):
        g = {"type": "cylinder"}
        g.update(frame((0.0, 0.0, z)))
        g["radius"] = r
        return g

    return {
        "body_id": "b0170000000000000000000000000001",
        "vertices": [
            {"point": [rs, 0.0, 0.0]},  # 0 shank end
            {"point": [rs, 0.0, length]},  # 1 shank under the head
            {"point": [rh, 0.0, length]},  # 2 head underside rim
            {"point": [rh, 0.0, top]},  # 3 head top rim
        ],
        "edges": [
            {"start": 0, "end": 0, "range": [0.0, TAU], "curve": circle(0.0, rs), "label": "#41"},
            {"start": 0, "end": 1, "range": [0.0, length], "curve": line((rs, 0.0, 0.0))},  # shank seam
            {"start": 1, "end": 1, "range": [0.0, TAU], "curve": circle(length, rs), "label": "#43"},
            {"start": 2, "end": 2, "range": [0.0, TAU], "curve": circle(length, rh), "label": "#44"},
            {"start": 2, "end": 3, "range": [0.0, head], "curve": line((rh, 0.0, length))},  # head seam
            {"start": 3, "end": 3, "range": [0.0, TAU], "curve": circle(top, rh), "label": "#46"},
        ],
        "coedges": [
            {"edge": 0, "sense": "same", "pcurve": None},  # 0..3 shank side
            {"edge": 1, "sense": "same", "pcurve": None},
            {"edge": 2, "sense": "reversed", "pcurve": None},
            {"edge": 1, "sense": "reversed", "pcurve": None},
            {"edge": 0, "sense": "reversed", "pcurve": None},  # 4 shank end
            {"edge": 3, "sense": "same", "pcurve": None},  # 5..8 head side
            {"edge": 4, "sense": "same", "pcurve": None},
            {"edge": 5, "sense": "reversed", "pcurve": None},
            {"edge": 4, "sense": "reversed", "pcurve": None},
            {"edge": 5, "sense": "same", "pcurve": None},  # 9 head top
            {"edge": 3, "sense": "reversed", "pcurve": None},  # 10 underside, outer rim
            {"edge": 2, "sense": "same", "pcurve": None},  # 11 underside, around the shank
        ],
        "loops": [
            {"coedges": [0, 1, 2, 3], "outer": True},
            {"coedges": [4], "outer": True},
            {"coedges": [5, 6, 7, 8], "outer": True},
            {"coedges": [9], "outer": True},
            {"coedges": [10], "outer": True},
            {"coedges": [11], "outer": False},
        ],
        "faces": [
            {"surface": plane(top, 1.0), "sense": "same", "loops": [3], "label": "#31"},
            {"surface": cylinder(length, rh), "sense": "same", "loops": [2], "label": "#32"},
            {"surface": plane(length, -1.0), "sense": "same", "loops": [4, 5], "label": "#33"},
            {"surface": cylinder(0.0, rs), "sense": "same", "loops": [0], "label": "#34"},
            {"surface": plane(0.0, -1.0), "sense": "same", "loops": [1], "label": "#35"},
        ],
        "shells": [{"faces": [0, 1, 2, 3, 4], "closed": True}],
    }


# ------------------------------------------------------------------ the assembly


ASSEMBLY_FRAME = "a5500000000000000000000000000001"

OCCURRENCES = [
    # One imported occurrence label and one derived occurrence ID, so both TopologyId
    # encodings appear in a path.
    {
        "name": "bolt-1",
        "path": [{"label": "#510"}, {"label": "#611"}],
        "frame": "f0000000000000000000000000000611",
        "translation": [-0.02, 0.0, 0.0],
    },
    {
        "name": "bolt-2",
        "path": [{"label": "#510"}, {"derived": "6d2c0d6e8e1a4c3b9f00a1b2c3d4e5f6"}],
        "frame": "f0000000000000000000000000000612",
        "translation": [0.02, 0.0, 0.0],
    },
]


def component(c):
    if "label" in c:
        return ("imported", c["label"])
    return ("derived", bytes.fromhex(c["derived"]))


def path_bytes(path):
    return struct.pack("<I", len(path)) + b"".join(enc_id(component(c)) for c in path)


def id_text(ident):
    return ident[1] if ident[0] == "imported" else str(uuid.UUID(bytes=ident[1]))


def expected_ids(fixture):
    body = canonical(fixture["body"])
    out = []
    for occ in fixture["occurrences"]:
        pb = path_bytes(occ["path"])
        entities = []
        for plural, kind in (("faces", "face"), ("edges", "edge"), ("vertices", "vertex")):
            for ent in body[plural]:
                eid = uuid.uuid5(NAMESPACE, pb + b"\x00" + enc_id(ent["id"]))
                entities.append({"kind": kind, "definition": id_text(ent["id"]), "id": str(eid)})
        out.append(
            {
                "name": occ["name"],
                "path_bytes": pb.hex(),
                "instance_id": str(uuid.uuid5(NAMESPACE, pb)),
                "entities": entities,
            }
        )
    return out


# ------------------------------------------------------------------ output


def dump(obj):
    return json.dumps(obj, indent=1, ensure_ascii=True, allow_nan=False) + "\n"


def build_all():
    fixture = {
        "description": "GEO.CAD.ASSY.MODEL: one invented bolt body placed twice under one sub-assembly occurrence",
        "license": LICENSE,
        "assembly_frame": ASSEMBLY_FRAME,
        "body": bolt(),
        "occurrences": OCCURRENCES,
    }
    text = dump(fixture)
    parsed = json.loads(text, object_pairs_hook=no_dup_keys)
    instances = expected_ids(parsed)
    per_instance = 5 + 6 + 4
    ids = [e["id"] for inst in instances for e in inst["entities"]]
    assert len(ids) == 2 * per_instance and len(set(ids)) == len(ids), "AC-1: distinct IDs across the pair"
    expected = {
        "description": "GEO.CAD.ASSY.MODEL expected instance and instance-entity IDs for two_bolts.json; generated "
        "by make_assy_fixtures.py (stdlib uuid, struct; independent of fid-geo-cad)",
        "license": LICENSE,
        "namespace": str(NAMESPACE),
        "instances": instances,
    }
    return {"two_bolts.json": text, "expected.json": dump(expected)}


def main():
    check_only = "--check" in sys.argv[1:]
    drift = []
    for rel, text in build_all().items():
        path = HERE / rel
        if check_only:
            if not path.exists() or path.read_text(encoding="utf-8") != text:
                drift.append(rel)
        else:
            path.write_text(text, encoding="utf-8", newline="\n")
    if drift:
        print("out of date: " + ", ".join(drift), file=sys.stderr)
        return 1
    print("ok" if check_only else "written")
    return 0


if __name__ == "__main__":
    sys.exit(main())
