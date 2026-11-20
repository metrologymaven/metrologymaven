#!/usr/bin/env python3
"""GEO.CAD.TOPO.MODEL fixtures and their expected values (independent Python reference).

Writes the synthetic builder scripts (a unit cube with and without STEP-style labels, a
capped cylinder, a sphere of two hemispheres, and invalid variants) and `expected.json`, the
canonical encoding v1, `body_digest` and IDs of every valid script and the build error of
every invalid one.

Independent of fid-geo-cad: canonical order, ID derivation (stdlib `uuid.uuid5`), the byte
layout (`struct`) and SHA-256 (`hashlib`) are re-implemented here from the pinned rules in
spec/geo/cad/topo/model/NODE.md (Design). The expected error of each invalid variant is
hand-labelled below from those rules, and this script asserts that its own checker agrees.
No geometry is evaluated: the scripts only need valid GEO.NURBS parameters, which the Rust
side checks on construction.

Usage: python make_topo_fixtures.py [--check]   (stdlib only; --check fails on drift)
"""

import copy
import hashlib
import json
import math
import struct
import sys
import uuid
from pathlib import Path

HERE = Path(__file__).resolve().parent
LICENSE = "CC-BY-4.0"
TAU = 2.0 * math.pi

NAMESPACE = uuid.uuid5(
    uuid.NAMESPACE_URL, "https://github.com/metrologymaven/metrologymaven/spec/geo/cad/topo/model#topology-id-v1"
)
assert str(NAMESPACE) == "0932dc8b-f062-5d17-b3eb-8089eddd9e6e"

MAX_ENTITIES = 10_000_000
MAX_LABEL = 256
KIND_CODE = {"shell": 1, "face": 2, "loop": 3, "coedge": 4, "edge": 5, "vertex": 6}


# ------------------------------------------------------------------ shapes


def unit(i):
    v = [0.0, 0.0, 0.0]
    v[i] = 1.0
    return v


def cube(labelled):
    """Unit cube [0,1]^3: 8 vertices, 12 line edges, 24 coedges, 6 loops, 6 planes."""
    verts = [[float(i & 1), float((i >> 1) & 1), float((i >> 2) & 1)] for i in range(8)]
    edges = []  # (start, end, axis)
    for d in range(3):
        for i in range(8):
            if not (i >> d) & 1:
                edges.append((i, i | (1 << d), d))
    edge_of = {}
    for k, (a, b, _) in enumerate(edges):
        edge_of[(a, b)] = (k, "same")
        edge_of[(b, a)] = (k, "reversed")
    coedges, loops, faces = [], [], []
    for a in range(3):
        b, c = (a + 1) % 3, (a + 2) % 3
        for side in (0, 1):
            # Counter-clockwise about the outward normal: e_b x e_c = e_a for cyclic (a, b, c).
            order = [(0, 0), (1, 0), (1, 1), (0, 1)] if side == 1 else [(0, 0), (0, 1), (1, 1), (1, 0)]
            ring = [(side << a) | (x << b) | (y << c) for x, y in order]
            loop = []
            for k in range(4):
                e, sense = edge_of[(ring[k], ring[(k + 1) % 4])]
                coedges.append({"edge": e, "sense": sense, "pcurve": None})
                loop.append(len(coedges) - 1)
            loops.append({"coedges": loop, "outer": True})
            normal = unit(a) if side == 1 else [-x for x in unit(a)]
            faces.append(
                {
                    "surface": {"type": "plane", "origin": verts[ring[0]], "axis": normal, "ref_dir": unit(b)},
                    "sense": "same",
                    "loops": [len(loops) - 1],
                }
            )
    script = {
        "body_id": "00112233445566778899aabbccddeeff",
        "vertices": [{"point": p} for p in verts],
        "edges": [
            {
                "start": s,
                "end": e,
                "range": [0.0, 1.0],
                "curve": {"type": "line", "origin": verts[s], "axis": unit(d), "ref_dir": unit((d + 1) % 3)},
            }
            for s, e, d in edges
        ],
        "coedges": coedges,
        "loops": loops,
        "faces": faces,
        "shells": [{"faces": list(range(6)), "closed": True}],
    }
    if labelled:
        n = 1
        for kind in ("vertices", "edges", "coedges", "loops", "faces"):
            for ent in script[kind]:
                ent["label"] = f"#{n}"
                n += 1
    return script


def cylinder():
    """Radius 0.5, height 1 about +z: side face with a seam, two planar caps. The seam edge is
    a degree-1 NURBS curve, the side coedges carry 2-D line pcurves, and one reference
    direction is given with a -0.0 component (stored as +0.0)."""
    r = 0.5
    circle = lambda z: {  # noqa: E731
        "type": "circle", "origin": [0.0, 0.0, z], "axis": [0.0, 0.0, 1.0], "ref_dir": [1.0, -0.0, 0.0], "radius": r
    }
    seam = {
        "type": "nurbs",
        "degree": 1,
        "knots": [0.0, 0.0, 1.0, 1.0],
        "ctrl": [[r, 0.0, 0.0], [r, 0.0, 1.0]],
        "weights": None,
        "periodic": False,
    }
    line2 = lambda o, d: {"type": "line2", "origin": o, "dir": d}  # noqa: E731
    return {
        "body_id": "0f1e2d3c4b5a69788796a5b4c3d2e1f0",
        "vertices": [{"point": [r, 0.0, 1.0]}, {"point": [r, 0.0, 0.0]}],
        "edges": [
            {"start": 0, "end": 0, "range": [0.0, TAU], "curve": circle(1.0)},  # top
            {"start": 1, "end": 0, "range": [0.0, 1.0], "curve": seam},  # seam, bottom to top
            {"start": 1, "end": 1, "range": [0.0, TAU], "curve": circle(0.0)},  # bottom
        ],
        "coedges": [
            {"edge": 2, "sense": "same", "pcurve": line2([0.0, 0.0], [1.0, 0.0])},
            {"edge": 1, "sense": "same", "pcurve": line2([TAU, 0.0], [0.0, 1.0])},
            {"edge": 0, "sense": "reversed", "pcurve": line2([0.0, 1.0], [1.0, 0.0])},
            {"edge": 1, "sense": "reversed", "pcurve": line2([0.0, 0.0], [0.0, 1.0])},
            {"edge": 0, "sense": "same", "pcurve": None},
            {"edge": 2, "sense": "reversed", "pcurve": None},
        ],
        "loops": [
            {"coedges": [4], "outer": True},
            {"coedges": [0, 1, 2, 3], "outer": True},
            {"coedges": [5], "outer": True},
        ],
        "faces": [
            {
                "surface": {"type": "plane", "origin": [0.0, 0.0, 1.0], "axis": [0.0, 0.0, 1.0], "ref_dir": [1.0, 0.0, 0.0]},
                "sense": "same",
                "loops": [0],
            },
            {
                "surface": {
                    "type": "cylinder", "origin": [0.0, 0.0, 0.0], "axis": [0.0, 0.0, 1.0], "ref_dir": [1.0, 0.0, 0.0],
                    "radius": r,
                },
                "sense": "same",
                "loops": [1],
            },
            {
                "surface": {"type": "plane", "origin": [0.0, 0.0, 0.0], "axis": [0.0, 0.0, -1.0], "ref_dir": [1.0, 0.0, 0.0]},
                "sense": "same",
                "loops": [2],
            },
        ],
        "shells": [{"faces": [1, 0, 2], "closed": True}],
    }


def sphere():
    """Unit sphere as two hemispheres on one surface, split by a closed equator circle.
    The upper coedge carries a NURBS pcurve with (all-equal) weights."""
    surf = {"type": "sphere", "origin": [0.0, 0.0, 0.0], "axis": [0.0, 0.0, 1.0], "ref_dir": [1.0, 0.0, 0.0], "radius": 1.0}
    return {
        "body_id": "a0a1a2a3a4a5a6a7a8a9aaabacadaeaf",
        "vertices": [{"point": [1.0, 0.0, 0.0]}],
        "edges": [
            {
                "start": 0,
                "end": 0,
                "range": [0.0, TAU],
                "curve": {"type": "circle", "origin": [0.0, 0.0, 0.0], "axis": [0.0, 0.0, 1.0], "ref_dir": [1.0, 0.0, 0.0], "radius": 1.0},
            }
        ],
        "coedges": [
            {"edge": 0, "sense": "reversed", "pcurve": None},
            {
                "edge": 0,
                "sense": "same",
                "pcurve": {
                    "type": "nurbs", "degree": 1, "knots": [0.0, 0.0, TAU, TAU], "ctrl": [[0.0, 0.0], [TAU, 0.0]],
                    "weights": [1.0, 1.0], "periodic": False,
                },
            },
        ],
        "loops": [{"coedges": [1], "outer": True}, {"coedges": [0], "outer": True}],
        "faces": [
            {"surface": surf, "sense": "same", "loops": [0]},
            {"surface": copy.deepcopy(surf), "sense": "same", "loops": [1]},
        ],
        "shells": [{"faces": [0, 1], "closed": True}],
    }


def invalid_variants():
    """(name, script, hand-labelled expected error) from the pinned check order."""
    out = []

    s = cube(False)
    s["faces"][1]["label"] = "#12"
    s["faces"][4]["label"] = "#12"
    out.append(("cube_duplicate_label", s, "DuplicateId", 'duplicate id "#12" at face[0,1] and face[0,4]'))

    s = cube(False)
    s["coedges"][5]["edge"] = 99
    out.append(("cube_bad_edge_ref", s, "BadReference", "coedge 5 references edge 99, which does not exist"))

    s = cube(False)
    s["loops"][2]["coedges"] = []
    out.append(("cube_empty_loop", s, "EmptyLoop", "loop 2 has no coedges"))

    s = cube(False)
    c = s["loops"][0]["coedges"]
    c[1], c[2] = c[2], c[1]
    out.append(("cube_open_loop", s, "OpenLoop", "loop 0 is open after coedge position 0"))

    s = cube(False)
    s["vertices"].append({"point": [2.0, 2.0, 2.0]})
    out.append(("cube_orphan_vertex", s, "Orphan", "vertex 8 is not reachable from any shell"))

    s = cube(False)
    s["shells"][0]["faces"].append(0)
    out.append(("cube_reused_face", s, "Reused", "face 0 is used by shell 0 and shell 0"))

    s = cube(False)
    s["vertices"][3]["label"] = ""
    out.append(("cube_empty_label", s, "BadLabel", "vertex 3 label is 0 bytes, outside 1..=256"))

    s = cube(False)
    s["edges"][7]["label"] = "x" * 257
    out.append(("cube_long_label", s, "BadLabel", "edge 7 label is 257 bytes, outside 1..=256"))

    s = cube(False)
    s["edges"][0]["range"] = [1.0, 1.0]
    out.append(("cube_bad_range", s, "BadParamRange", "edge 0 parameter range is not increasing"))

    s = sphere()
    s["coedges"][0]["sense"] = "same"
    out.append(("sphere_same_sense", s, "BadPairing", "edge 0: its two coedges have the same sense"))

    s = sphere()
    del s["faces"][1], s["loops"][1], s["coedges"][0]
    s["loops"][0]["coedges"] = [0]
    s["shells"][0]["faces"] = [0]
    out.append(("sphere_unpaired", s, "BadPairing", "edge 0: used once in closed shell 0"))

    s = sphere()
    s["coedges"].append({"edge": 0, "sense": "reversed", "pcurve": None})
    s["loops"].append({"coedges": [2], "outer": True})
    s["faces"].append(copy.deepcopy(s["faces"][0]))
    s["faces"][2]["loops"] = [2]
    s["shells"][0]["faces"].append(2)
    s["shells"][0]["closed"] = False
    out.append(("sphere_three_coedges", s, "BadPairing", "edge 0: used by more than two coedges"))

    s = sphere()
    s["shells"] = [{"faces": [0], "closed": False}, {"faces": [1], "closed": False}]
    out.append(("sphere_cross_shell", s, "BadPairing", "edge 0: its two coedges are in different shells"))

    s = cylinder()
    s["faces"][1]["loops"].append(0)
    out.append(("cylinder_loop_in_two_faces", s, "MultipleOuter", "face 1 has more than one outer loop"))

    s = cylinder()
    s["loops"][0]["outer"] = False
    s["faces"][1]["loops"].append(0)
    out.append(("cylinder_loop_reused", s, "Reused", "loop 0 is used by face 1 and face 0"))
    return out


# ------------------------------------------------------------------ independent checker


class BuildError(Exception):
    def __init__(self, code, message):
        super().__init__(message)
        self.code = code
        self.message = message


def sense_ends(script, c):
    co = script["coedges"][c]
    e = script["edges"][co["edge"]]
    return (e["start"], e["end"]) if co["sense"] == "same" else (e["end"], e["start"])


def check(script):
    V, E, C, L, F, S = (script[k] for k in ("vertices", "edges", "coedges", "loops", "faces", "shells"))
    if len(V) + len(E) + len(C) + len(L) + len(F) + len(S) > MAX_ENTITIES:
        raise BuildError("TooLarge", "too large")

    def label(kind, i, ent):
        lab = ent.get("label")
        if lab is not None:
            n = len(lab.encode("utf-8"))
            if n == 0 or n > MAX_LABEL:
                raise BuildError("BadLabel", f"{kind} {i} label is {n} bytes, outside 1..=256")

    def ref(kind, i, target, t, n):
        if not (0 <= t < n):
            raise BuildError("BadReference", f"{kind} {i} references {target} {t}, which does not exist")

    for i, v in enumerate(V):
        label("vertex", i, v)
        if not all(math.isfinite(x) for x in v["point"]):
            raise BuildError("NonFinite", f"vertex {i} has a non-finite coordinate or parameter")
    for i, e in enumerate(E):
        label("edge", i, e)
        t0, t1 = e["range"]
        if not (math.isfinite(t0) and math.isfinite(t1)):
            raise BuildError("NonFinite", f"edge {i} has a non-finite coordinate or parameter")
        if not t0 < t1:
            raise BuildError("BadParamRange", f"edge {i} parameter range is not increasing")
        ref("edge", i, "vertex", e["start"], len(V))
        ref("edge", i, "vertex", e["end"], len(V))
    for i, c in enumerate(C):
        label("coedge", i, c)
        ref("coedge", i, "edge", c["edge"], len(E))
    for i, lp in enumerate(L):
        label("loop", i, lp)
        for c in lp["coedges"]:
            ref("loop", i, "coedge", c, len(C))
        if not lp["coedges"]:
            raise BuildError("EmptyLoop", f"loop {i} has no coedges")
    for i, f in enumerate(F):
        label("face", i, f)
        for lp in f["loops"]:
            ref("face", i, "loop", lp, len(L))
        if sum(1 for lp in f["loops"] if L[lp]["outer"]) > 1:
            raise BuildError("MultipleOuter", f"face {i} has more than one outer loop")
    for i, s in enumerate(S):
        for f in s["faces"]:
            ref("shell", i, "face", f, len(F))
        if not s["faces"]:
            raise BuildError("EmptyShell", f"shell {i} has no faces")

    face_shell, loop_face, coedge_loop = [None] * len(F), [None] * len(L), [None] * len(C)

    def own(slots, kind, i, owner, by):
        if slots[i] is not None:
            raise BuildError("Reused", f"{kind} {i} is used by {owner} {slots[i]} and {owner} {by}")
        slots[i] = by

    for si, s in enumerate(S):
        for f in s["faces"]:
            own(face_shell, "face", f, "shell", si)
    for s in S:
        for f in s["faces"]:
            for lp in F[f]["loops"]:
                own(loop_face, "loop", lp, "face", f)
    for s in S:
        for f in s["faces"]:
            for lp in F[f]["loops"]:
                for c in L[lp]["coedges"]:
                    own(coedge_loop, "coedge", c, "loop", lp)
    for kind, slots in (("face", face_shell), ("loop", loop_face), ("coedge", coedge_loop)):
        for i, o in enumerate(slots):
            if o is None:
                raise BuildError("Orphan", f"{kind} {i} is not reachable from any shell")
    used_e = {c["edge"] for c in C}
    for i in range(len(E)):
        if i not in used_e:
            raise BuildError("Orphan", f"edge {i} is not reachable from any shell")
    used_v = {v for e in E for v in (e["start"], e["end"])}
    for i in range(len(V)):
        if i not in used_v:
            raise BuildError("Orphan", f"vertex {i} is not reachable from any shell")

    for li, lp in enumerate(L):
        cs = lp["coedges"]
        for k in range(len(cs)):
            if sense_ends(script, cs[k])[1] != sense_ends(script, cs[(k + 1) % len(cs)])[0]:
                raise BuildError("OpenLoop", f"loop {li} is open after coedge position {k}")

    shell_of = lambda c: face_shell[loop_face[coedge_loop[c]]]  # noqa: E731
    for ei in range(len(E)):
        uses = [c for c in range(len(C)) if C[c]["edge"] == ei]
        if len(uses) > 2:
            raise BuildError("BadPairing", f"edge {ei}: used by more than two coedges")
        if len(uses) == 2:
            a, b = uses
            if C[a]["sense"] == C[b]["sense"]:
                raise BuildError("BadPairing", f"edge {ei}: its two coedges have the same sense")
            if shell_of(a) != shell_of(b):
                raise BuildError("BadPairing", f"edge {ei}: its two coedges are in different shells")
        if len(uses) == 1 and S[shell_of(uses[0])]["closed"]:
            raise BuildError("BadPairing", f"edge {ei}: used once in closed shell {shell_of(uses[0])}")


def canonical(script):
    """Entities in canonical order with their index paths and IDs."""
    body_id = bytes.fromhex(script["body_id"])
    V, E, C, L, F, S = (script[k] for k in ("vertices", "edges", "coedges", "loops", "faces", "shells"))
    out = {k: [] for k in ("shells", "faces", "loops", "coedges", "edges", "vertices")}
    edge_new, vertex_new = {}, {}
    for sp, s in enumerate(S):
        out["shells"].append({"src": s, "path": [sp], "nfaces": len(s["faces"])})
        for fp, f in enumerate(s["faces"]):
            out["faces"].append({"src": F[f], "path": [sp, fp], "nloops": len(F[f]["loops"])})
            for lp_pos, lp in enumerate(F[f]["loops"]):
                out["loops"].append({"src": L[lp], "path": [sp, fp, lp_pos], "ncoedges": len(L[lp]["coedges"])})
                for cp, c in enumerate(L[lp]["coedges"]):
                    path = [sp, fp, lp_pos, cp]
                    e = C[c]["edge"]
                    if e not in edge_new:
                        edge_new[e] = len(out["edges"])
                        out["edges"].append({"src": E[e], "path": path})
                        for end, v in ((0, E[e]["start"]), (1, E[e]["end"])):
                            if v not in vertex_new:
                                vertex_new[v] = len(out["vertices"])
                                out["vertices"].append({"src": V[v], "path": path + [end]})
                    out["coedges"].append({"src": C[c], "path": path, "edge": edge_new[e]})
    for e in out["edges"]:
        e["start"] = vertex_new[e["src"]["start"]]
        e["end"] = vertex_new[e["src"]["end"]]
    seen = {}
    for plural, kind in (("shells", "shell"), ("faces", "face"), ("loops", "loop"), ("coedges", "coedge"),
                         ("edges", "edge"), ("vertices", "vertex")):
        for ent in out[plural]:
            lab = ent["src"].get("label") if kind != "shell" else None
            if lab is not None:
                ent["id"] = ("imported", lab)
            else:
                name = body_id + bytes([KIND_CODE[kind]]) + b"".join(struct.pack("<I", p) for p in ent["path"])
                ent["id"] = ("derived", uuid.uuid5(NAMESPACE, name).bytes)
            here = f"{kind}[{','.join(str(p) for p in ent['path'])}]"
            if ent["id"] in seen:
                shown = '"' + lab + '"' if ent["id"][0] == "imported" else str(uuid.UUID(bytes=ent["id"][1]))
                raise BuildError("DuplicateId", f"duplicate id {shown} at {seen[ent['id']]} and {here}")
            seen[ent["id"]] = here
    return out


# ------------------------------------------------------------------ independent encoder


def zero(x):
    x = float(x)
    assert math.isfinite(x)
    return 0.0 if x == 0.0 else x  # -0.0 -> +0.0


def f64(x):
    return struct.pack("<d", zero(x))


def u64(n):
    return struct.pack("<Q", n)


def enc_id(ident):
    if ident[0] == "derived":
        return b"\x00" + ident[1]
    raw = ident[1].encode("utf-8")
    return b"\x01" + struct.pack("<H", len(raw)) + raw


def enc_frame(g):
    return b"".join(f64(x) for k in ("origin", "axis", "ref_dir") for x in g[k])


def enc_weights(w):
    return b"\x00" if w is None else b"\x01" + b"".join(f64(x) for x in w)


def enc_nurbs_curve(g, dims):
    out = u64(g["degree"]) + u64(len(g["knots"])) + b"".join(f64(k) for k in g["knots"])
    out += u64(len(g["ctrl"]))
    for p in g["ctrl"]:
        assert len(p) == dims
        out += b"".join(f64(x) for x in p)
    return out + enc_weights(g["weights"]) + bytes([g["periodic"]])


def enc_curve(g):
    if g is None:
        return b"\x00"
    t = g["type"]
    if t == "line":
        return b"\x01" + enc_frame(g)
    if t == "circle":
        return b"\x02" + enc_frame(g) + f64(g["radius"])
    if t == "ellipse":
        return b"\x03" + enc_frame(g) + f64(g["semi_axis_1"]) + f64(g["semi_axis_2"])
    if t == "nurbs":
        return b"\x10" + enc_nurbs_curve(g, 3)
    raise ValueError(t)


def enc_pcurve(g):
    if g is None:
        return b"\x00"
    if g["type"] == "line2":
        return b"\x01" + b"".join(f64(x) for x in g["origin"] + g["dir"])
    if g["type"] == "nurbs":
        return b"\x10" + enc_nurbs_curve(g, 2)
    raise ValueError(g["type"])


def enc_surface(g):
    t = g["type"]
    sizes = {"plane": (1, []), "cylinder": (2, ["radius"]), "cone": (3, ["radius", "half_angle"]),
             "sphere": (4, ["radius"]), "torus": (5, ["major_radius", "minor_radius"])}
    if t in sizes:
        tag, keys = sizes[t]
        return bytes([tag]) + enc_frame(g) + b"".join(f64(g[k]) for k in keys)
    if t == "nurbs":
        out = b"\x10" + u64(g["degree_u"]) + u64(g["degree_v"])
        for k in ("knots_u", "knots_v"):
            out += u64(len(g[k])) + b"".join(f64(x) for x in g[k])
        out += u64(g["n_u"]) + u64(g["n_v"]) + u64(len(g["ctrl"]))
        out += b"".join(f64(x) for p in g["ctrl"] for x in p)
        return out + enc_weights(g["weights"]) + bytes([g["periodic_u"], g["periodic_v"]])
    raise ValueError(t)


SENSE = {"same": b"\x00", "reversed": b"\x01"}


def encode(script):
    check(script)
    can = canonical(script)
    out = b"FIDBREP" + bytes([1]) + bytes.fromhex(script["body_id"])
    for k in ("shells", "faces", "loops", "coedges", "edges", "vertices"):
        out += u64(len(can[k]))
    for s in can["shells"]:
        out += enc_id(s["id"]) + bytes([s["src"]["closed"]]) + u64(s["nfaces"])
    for f in can["faces"]:
        out += enc_id(f["id"]) + SENSE[f["src"]["sense"]] + u64(f["nloops"]) + enc_surface(f["src"]["surface"])
    for lp in can["loops"]:
        out += enc_id(lp["id"]) + bytes([lp["src"]["outer"]]) + u64(lp["ncoedges"])
    for c in can["coedges"]:
        out += enc_id(c["id"]) + u64(c["edge"]) + SENSE[c["src"]["sense"]] + enc_pcurve(c["src"]["pcurve"])
    for e in can["edges"]:
        out += enc_id(e["id"]) + u64(e["start"]) + u64(e["end"])
        out += f64(e["src"]["range"][0]) + f64(e["src"]["range"][1]) + enc_curve(e["src"]["curve"])
    for v in can["vertices"]:
        out += enc_id(v["id"]) + b"".join(f64(x) for x in v["src"]["point"])
    out += hashlib.sha256(out).digest()
    ids = {}
    for k in ("shells", "faces", "loops", "coedges", "edges", "vertices"):
        ids[k] = [e["id"][1] if e["id"][0] == "imported" else str(uuid.UUID(bytes=e["id"][1])) for e in can[k]]
    return out, ids


# ------------------------------------------------------------------ files


def dump(obj):
    return json.dumps(obj, indent=1, ensure_ascii=True, allow_nan=False) + "\n"


def no_dup_keys(pairs):
    d = {}
    for k, v in pairs:
        if k in d:
            raise ValueError(f"duplicate key {k!r}")
        d[k] = v
    return d


def build_all():
    files = {}
    valid = [
        ("cube_labelled", cube(True), "unit cube with a STEP-style label on every face, loop, coedge, edge and vertex"),
        ("cube_unlabelled", cube(False), "unit cube without labels: every ID is derived"),
        ("cylinder_capped", cylinder(), "capped cylinder with seam, NURBS seam curve and line pcurves"),
        ("sphere_hemispheres", sphere(), "unit sphere as two hemispheres split by the equator"),
    ]
    expected = {
        "description": "GEO.CAD.TOPO.MODEL expected encodings, digests and IDs; generated by make_topo_fixtures.py "
        "(stdlib uuid, struct, hashlib; independent of fid-geo-cad)",
        "license": LICENSE,
        "namespace": str(NAMESPACE),
        "valid": {},
        "invalid": {},
    }
    for name, script, desc in valid:
        body = {"description": desc, "license": LICENSE}
        body.update(script)
        files[f"{name}.json"] = dump(body)
        enc, ids = encode(json.loads(files[f"{name}.json"], object_pairs_hook=no_dup_keys))
        expected["valid"][name] = {
            "digest": "sha256:" + hashlib.sha256(enc).hexdigest(),
            "encoding_len": len(enc),
            "encoding_hex": enc.hex(),
            "ids": ids,
        }
    for name, script, code, message in invalid_variants():
        body = {"description": "invalid variant: " + message, "license": LICENSE}
        body.update(script)
        body["expect"] = {"code": code, "message": message}
        files[f"invalid/{name}.json"] = dump(body)
        try:
            encode(json.loads(files[f"invalid/{name}.json"], object_pairs_hook=no_dup_keys))
        except BuildError as e:
            assert (e.code, e.message) == (code, message), (name, e.code, e.message)
        else:
            raise AssertionError(f"{name}: no error")
        expected["invalid"][name] = {"code": code, "message": message}
    files["expected.json"] = dump(expected)
    return files


def main():
    check_only = "--check" in sys.argv[1:]
    drift = []
    for rel, text in build_all().items():
        path = HERE / rel
        if check_only:
            if not path.exists() or path.read_text(encoding="utf-8") != text:
                drift.append(rel)
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text, encoding="utf-8", newline="\n")
    if drift:
        print("out of date: " + ", ".join(drift), file=sys.stderr)
        return 1
    print("ok" if check_only else "written")
    return 0


if __name__ == "__main__":
    sys.exit(main())
