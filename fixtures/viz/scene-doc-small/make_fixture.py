#!/usr/bin/env python3
"""Independent reference for VIZ.RENDER.SCENE.PROJ AC-1 and AC-2 (project-authored, CC0).

The document `scene-doc-small` is one part with 1 mesh, 2 point sets and 12 characteristics,
each characteristic shown with a label and a leader line. This script writes, next to itself:

- doc.txt: the document state, one element per line in tree order:
  `<entity uuid> <kind> <frame id> <status> <data digest> ox oy oz minx miny minz maxx maxy maxz`
  (floats as Python `repr`, which round-trips exactly);
- doc-changed.txt: the same document with characteristic c05 changed (new data digest, new
  kernel status, origin moved 1 mm in z);
- dirty.txt: the dirty set CORE publishes for that change, one entity per line: c05 and the
  second point set (dirty, but its data is unchanged);
- expect-full.txt / expect-full.digest: the serialised scene graph of doc.txt and its digest;
- expect-batch.txt: the change batch of re-projecting dirty.txt against doc-changed.txt;
- expect-changed.txt / expect-changed.digest: the serialised scene graph after that batch.

Nothing here runs the Rust code. The expected values follow the leaf's rules and the layout
pinned in `crates/fid-render/src/scene/proj.rs` (module docs), restated here:

- Render IDs are dense from 1 in registration order: elements in tree order, and within an
  element the nodes in the order its projector emits them (IDTAB REQ-001).
- A node line is
  `<rid> <identity> <kind> <frame> <ox> <oy> <oz> <minx> <miny> <minz> <maxx> <maxy> <maxz> <style> <status> <digest>`
  with identity `e:<uuid>` or `d:<uuid>:<op>`, kind `element:<kind>` or `derived:<op>`, the
  frame as 32 lowercase hex digits and every f64 as the 16 lowercase hex digits of its
  big-endian IEEE 754 bits. Lines are ascending by render ID and end in `\\n`.
- A primary node's data digest is the element's data digest. A derived node's data digest is
  SHA-256( b"FIDSCN" || 0x01 || source digest (32 bytes) || view-operation code (1 byte) ).
- Frame and status of every node are the element's (kernel-supplied, constitution C2).
- The graph digest is `sha256:` + SHA-256 of the serialised lines.
- A change batch lists added, updated and retired render IDs, each ascending. A dirty node
  whose regenerated line equals the old one is not listed.

The fixture's projectors (implemented again, separately, in `crates/fid-render/tests/proj.rs`):

- part, mesh, point_set: one primary node: origin and bounds of the element, style
  `geom.<kind>`.
- characteristic: primary node (style `char`, element origin and bounds); then a label at
  p = origin + (0, 0, 0.005) with a point box at p (style `char.label`); then a leader with
  origin = element origin and bounds = per-axis min/max of origin and p (style `char.leader`).

Run:  python fixtures/viz/scene-doc-small/make_fixture.py          (write)
      python fixtures/viz/scene-doc-small/make_fixture.py --check  (compare, exit 1 on drift)
"""

import hashlib
import struct
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent

PART_FRAME = "5c0e0000000000000000000000000001"
# View-operation codes pinned by VIZ.RENDER.SCENE.IDTAB (whisker 1 .. proxy 6).
OP_CODE = {"whisker": 1, "label": 2, "leader": 3, "ghost": 4, "clip_cap": 5, "proxy": 6}
LABEL_DZ = 0.005


def uuid(n):
    return "00000000-0000-4000-8000-%012x" % n


def data_digest(name, version):
    return "sha256:" + hashlib.sha256(("scene-doc-small:%s:v%d" % (name, version)).encode()).hexdigest()


def bits(x):
    return struct.pack(">d", x).hex()


def element(n, kind, status, name, version, origin, lo, hi):
    return {
        "entity": uuid(n),
        "kind": kind,
        "frame": PART_FRAME,
        "status": status,
        "digest": data_digest(name, version),
        "origin": origin,
        "min": lo,
        "max": hi,
    }


def document(changed):
    els = [
        element(1, "part", "none", "part", 1, (0.0, 0.0, 0.0), (-0.06, -0.04, -0.01), (0.06, 0.04, 0.035)),
        element(2, "mesh", "none", "mesh", 1, (0.0, 0.0, 0.0125), (-0.06, -0.04, -0.01), (0.06, 0.04, 0.035)),
        element(3, "point_set", "none", "ps1", 1, (-0.02, 0.01, 0.03), (-0.045, -0.03, 0.029), (0.004, 0.035, 0.031)),
        element(4, "point_set", "none", "ps2", 1, (0.03, -0.015, 0.0), (0.012, -0.033, -0.002), (0.047, 0.004, 0.002)),
    ]
    statuses = ["conforming", "nonconforming", "none", "conforming"]
    for i in range(1, 13):
        x = -0.055 + 0.01 * i
        y = 0.03 - 0.0045 * i
        z = 0.035 if i % 2 else -0.01
        status = statuses[i % 4]
        version = 1
        if changed and i == 5:
            z = z + 0.001
            status = "nonconforming"
            version = 2
        p = (x, y, z)
        els.append(element(10 + i, "characteristic", status, "c%02d" % i, version, p, p, p))
    return els


def doc_text(els):
    out = []
    for e in els:
        nums = list(e["origin"]) + list(e["min"]) + list(e["max"])
        out.append(" ".join([e["entity"], e["kind"], e["frame"], e["status"], e["digest"]] + [repr(v) for v in nums]))
    return "\n".join(out) + "\n"


def derived_digest(source, op):
    raw = bytes.fromhex(source[len("sha256:"):])
    return "sha256:" + hashlib.sha256(b"FIDSCN" + bytes([1]) + raw + bytes([OP_CODE[op]])).hexdigest()


def project(e):
    """The fixture's projectors: a list of (identity, kind, origin, min, max, style, digest)."""
    ent = e["entity"]
    if e["kind"] in ("part", "mesh", "point_set"):
        return [("e:" + ent, "element:" + e["kind"], e["origin"], e["min"], e["max"], "geom." + e["kind"], e["digest"])]
    if e["kind"] == "characteristic":
        o = e["origin"]
        p = (o[0], o[1], o[2] + LABEL_DZ)
        lo = tuple(min(a, b) for a, b in zip(o, p))
        hi = tuple(max(a, b) for a, b in zip(o, p))
        return [
            ("e:" + ent, "element:characteristic", o, e["min"], e["max"], "char", e["digest"]),
            ("d:%s:label" % ent, "derived:label", p, p, p, "char.label", derived_digest(e["digest"], "label")),
            ("d:%s:leader" % ent, "derived:leader", o, lo, hi, "char.leader", derived_digest(e["digest"], "leader")),
        ]
    raise ValueError("no projector for " + e["kind"])


def node_line(rid, node, e):
    identity, kind, o, lo, hi, style, digest = node
    floats = [bits(v) for v in list(o) + list(lo) + list(hi)]
    return " ".join([str(rid), identity, kind, e["frame"]] + floats + [style, e["status"], digest])


def full_projection(els):
    """identity -> (rid, line), registering in tree order."""
    graph = {}
    rid = 0
    for e in els:
        for node in project(e):
            rid += 1
            graph[node[0]] = (rid, node_line(rid, node, e))
    return graph, rid


def serialise(graph):
    lines = sorted(graph.values())
    return "".join(line + "\n" for _, line in lines)


def graph_digest(text):
    return "sha256:" + hashlib.sha256(text.encode()).hexdigest()


def reproject(graph, next_rid, els, dirty):
    by_entity = {e["entity"]: e for e in els}
    added, updated, retired = [], [], []
    graph = dict(graph)
    order = [e["entity"] for e in els if e["entity"] in dirty]
    for ent in order:
        e = by_entity[ent]
        new = project(e)
        new_ids = set(n[0] for n in new)
        for identity, (rid, _) in list(graph.items()):
            if identity.split(":")[1] == ent and identity not in new_ids:
                retired.append(rid)
                del graph[identity]
        for node in new:
            if node[0] in graph:
                rid, old = graph[node[0]]
                line = node_line(rid, node, e)
                if line != old:
                    updated.append(rid)
                    graph[node[0]] = (rid, line)
            else:
                next_rid += 1
                added.append(next_rid)
                graph[node[0]] = (next_rid, node_line(next_rid, node, e))
    return graph, (sorted(added), sorted(updated), sorted(retired))


def batch_text(batch):
    names = ["added", "updated", "retired"]
    return "".join(" ".join([n] + [str(r) for r in ids]) + "\n" for n, ids in zip(names, batch))


def outputs():
    base = document(False)
    changed = document(True)
    dirty = [uuid(15), uuid(4)]
    graph, last = full_projection(base)
    full = serialise(graph)
    graph2, batch = reproject(graph, last, changed, set(dirty))
    after = serialise(graph2)
    # Sanity on the scenario, from the leaf's AC-1 and AC-2.
    assert len(base) == 16 and sum(1 for e in base if e["kind"] == "characteristic") == 12
    assert len(graph) == 4 + 12 * 3
    c05 = [rid for ident, (rid, _) in graph.items() if ident.split(":")[1] == uuid(15)]
    assert batch == ([], sorted(c05), []) and len(c05) == 3
    return {
        "doc.txt": doc_text(base),
        "doc-changed.txt": doc_text(changed),
        "dirty.txt": "".join(d + "\n" for d in dirty),
        "expect-full.txt": full,
        "expect-full.digest": graph_digest(full) + "\n",
        "expect-batch.txt": batch_text(batch),
        "expect-changed.txt": after,
        "expect-changed.digest": graph_digest(after) + "\n",
    }


def main():
    check = "--check" in sys.argv[1:]
    drift = False
    for name, text in outputs().items():
        path = HERE / name
        if check:
            if not path.exists() or path.read_bytes() != text.encode():
                print("drift: " + name)
                drift = True
        else:
            path.write_bytes(text.encode())
    return 1 if drift else 0


if __name__ == "__main__":
    sys.exit(main())
