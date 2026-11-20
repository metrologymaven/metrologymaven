#!/usr/bin/env python3
"""GEO.CAD.TOPO.ADJ fixtures and expected adjacency (independent Python reference).

Writes two hand-written builder scripts that the GEO.CAD.TOPO.MODEL fixtures do not cover:

- `cylinder_seam.json`: an uncapped cylinder side, one face closed by a seam edge whose two
  coedges lie in the same loop (AC-2), in an open shell, so the two circle edges are used once;
- `bowtie_vertex.json`: two faces in two open shells that meet only at one vertex (a
  non-manifold vertex), the second face with an inner loop;

and the expected answer of every adjacency query for every entity of every valid fixture:
`cube_adjacency.json` (AC-1, the labelled cube) and `adjacency_expected.json` (the other
fixtures). It also asserts that `invalid/sphere_three_coedges.json` (a non-manifold edge with
three coedges) is refused by the model's pairing check, so no body, and no adjacency, holds a
non-manifold edge.

Independent of fid-geo-cad: canonical order and IDs come from `make_topo_fixtures.py` (the
stdlib reference of GEO.CAD.TOPO.MODEL); every query below is a literal scan over the
canonical entity lists (no index), the results sorted by the TopologyId order of
REQ-GEO.CAD.TOPO.MODEL-008 (imported labels first, bytewise, then derived UUIDs, bytewise).
The cube's answers are also checked against hand-stated rules: each face has 4 neighbours
(every face but itself and its opposite), each edge 2 faces, each vertex 3 edges and 3 faces.

ID text: an imported label verbatim, a derived ID as its hyphenated UUID (as in expected.json).

Usage: python make_adj_fixtures.py [--check]   (stdlib only; --check fails on drift)
"""

import json
import math
import sys
import uuid
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import make_topo_fixtures as topo  # noqa: E402

LICENSE = "CC-BY-4.0"
TAU = 2.0 * math.pi
QUERIES = (
    "faces_of_edge",
    "coedges_of_edge",
    "edges_of_face",
    "loops_of_face",
    "face_neighbours",
    "faces_of_vertex",
    "edges_of_vertex",
)
VALID = ("cube_unlabelled", "cylinder_capped", "sphere_hemispheres", "cylinder_seam", "bowtie_vertex", "lens_sphere")


# ------------------------------------------------------------------ hand-written scripts


def cylinder_seam():
    """Side of a radius-0.5, height-1 cylinder about +z, no caps (open shell). The face's one
    loop runs bottom circle, seam up, top circle reversed, seam down: the seam edge's two
    coedges (opposite senses) both lie in this loop."""
    r = 0.5
    circle = lambda z: {  # noqa: E731
        "type": "circle", "origin": [0.0, 0.0, z], "axis": [0.0, 0.0, 1.0], "ref_dir": [1.0, 0.0, 0.0], "radius": r
    }
    return {
        "body_id": "5ea45ea45ea45ea45ea45ea45ea45ea4",
        "vertices": [{"point": [r, 0.0, 1.0]}, {"point": [r, 0.0, 0.0]}],
        "edges": [
            {"start": 0, "end": 0, "range": [0.0, TAU], "curve": circle(1.0)},  # top
            {
                "start": 1,
                "end": 0,
                "range": [0.0, 1.0],
                "curve": {"type": "line", "origin": [r, 0.0, 0.0], "axis": [0.0, 0.0, 1.0], "ref_dir": [1.0, 0.0, 0.0]},
                "label": "#seam",
            },
            {"start": 1, "end": 1, "range": [0.0, TAU], "curve": circle(0.0)},  # bottom
        ],
        "coedges": [
            {"edge": 2, "sense": "same", "pcurve": None},
            {"edge": 1, "sense": "same", "pcurve": None},
            {"edge": 0, "sense": "reversed", "pcurve": None},
            {"edge": 1, "sense": "reversed", "pcurve": None},
        ],
        "loops": [{"coedges": [0, 1, 2, 3], "outer": True}],
        "faces": [
            {
                "surface": {
                    "type": "cylinder", "origin": [0.0, 0.0, 0.0], "axis": [0.0, 0.0, 1.0], "ref_dir": [1.0, 0.0, 0.0],
                    "radius": r,
                },
                "sense": "same",
                "loops": [0],
                "label": "#side",
            }
        ],
        "shells": [{"faces": [0], "closed": False}],
    }


def lens_sphere():
    """Unit sphere as two half-sphere lunes split by two meridians (x > 0 and x < 0 halves of
    the y = 0 great circle) running south pole to north pole: the two faces share both
    meridian edges, so each is the other's neighbour twice over, and both poles touch both."""
    surf = {"type": "sphere", "origin": [0.0, 0.0, 0.0], "axis": [0.0, 0.0, 1.0], "ref_dir": [1.0, 0.0, 0.0], "radius": 1.0}

    def meridian(ref):
        return {"type": "circle", "origin": [0.0, 0.0, 0.0], "axis": [0.0, 1.0, 0.0], "ref_dir": ref, "radius": 1.0}

    return {
        "body_id": "1e051e051e051e051e051e051e051e05",
        "vertices": [{"point": [0.0, 0.0, -1.0], "label": "#south"}, {"point": [0.0, 0.0, 1.0], "label": "#north"}],
        "edges": [
            {"start": 0, "end": 1, "range": [0.0, math.pi], "curve": meridian([0.0, 0.0, -1.0]), "label": "#m_east"},
            {"start": 0, "end": 1, "range": [0.0, math.pi], "curve": meridian([0.0, 0.0, -1.0]), "label": "#m_west"},
        ],
        "coedges": [
            {"edge": 0, "sense": "same", "pcurve": None},
            {"edge": 1, "sense": "reversed", "pcurve": None},
            {"edge": 1, "sense": "same", "pcurve": None},
            {"edge": 0, "sense": "reversed", "pcurve": None},
        ],
        "loops": [{"coedges": [0, 1], "outer": True}, {"coedges": [2, 3], "outer": True}],
        "faces": [
            {"surface": surf, "sense": "same", "loops": [0], "label": "#lune_a"},
            {"surface": dict(surf), "sense": "same", "loops": [1], "label": "#lune_b"},
        ],
        "shells": [{"faces": [0, 1], "closed": True}],
    }


def bowtie_vertex():
    """Triangle A (z = 0, first quadrant) and square B with a triangular hole (third quadrant)
    share only vertex 0; each face is its own open shell."""
    plane = {"type": "plane", "origin": [0.0, 0.0, 0.0], "axis": [0.0, 0.0, 1.0], "ref_dir": [1.0, 0.0, 0.0]}
    pts = [
        [0.0, 0.0, 0.0],  # 0 shared
        [1.0, 0.0, 0.0],  # 1 A
        [0.0, 1.0, 0.0],  # 2 A
        [-2.0, 0.0, 0.0],  # 3 B outer
        [-2.0, -2.0, 0.0],  # 4 B outer
        [0.0, -2.0, 0.0],  # 5 B outer
        [-1.5, -0.5, 0.0],  # 6 B hole
        [-0.5, -1.5, 0.0],  # 7 B hole
        [-1.5, -1.5, 0.0],  # 8 B hole
    ]
    pairs = [(0, 1), (1, 2), (2, 0), (0, 3), (3, 4), (4, 5), (5, 0), (6, 7), (7, 8), (8, 6)]

    def line(a, b):
        d = [pts[b][i] - pts[a][i] for i in range(3)]
        n = math.sqrt(sum(x * x for x in d))
        return {"type": "line", "origin": pts[a], "axis": [x / n for x in d], "ref_dir": [0.0, 0.0, 1.0]}

    return {
        "body_id": "b0b1b2b3b4b5b6b7b8b9babbbcbdbebf",
        "vertices": [{"point": p} for p in pts],
        "edges": [{"start": a, "end": b, "range": [0.0, 1.0], "curve": line(a, b)} for a, b in pairs],
        "coedges": [{"edge": e, "sense": "same", "pcurve": None} for e in range(len(pairs))],
        "loops": [
            {"coedges": [0, 1, 2], "outer": True},
            {"coedges": [3, 4, 5, 6], "outer": True},
            {"coedges": [7, 8, 9], "outer": False},
        ],
        "faces": [
            {"surface": plane, "sense": "same", "loops": [0]},
            {"surface": dict(plane), "sense": "same", "loops": [1, 2]},
        ],
        "shells": [{"faces": [0], "closed": False}, {"faces": [1], "closed": False}],
    }


# ------------------------------------------------------------------ reference queries


def id_key(ident):
    """REQ-GEO.CAD.TOPO.MODEL-008: imported labels before derived IDs, then bytewise."""
    kind, value = ident
    return (0, value.encode("utf-8")) if kind == "imported" else (1, value)


def id_text(ident):
    kind, value = ident
    return value if kind == "imported" else str(uuid.UUID(bytes=value))


def sorted_ids(idents):
    """Ascending, deduplicated, as text."""
    return [id_text(i) for i in sorted(set(idents), key=id_key)]


def adjacency(script):
    topo.check(script)
    out = topo.canonical(script)
    faces, loops, coedges, edges, vertices = (out[k] for k in ("faces", "loops", "coedges", "edges", "vertices"))
    # Ownership from the canonical traversal: each face owns the next nloops loops, each loop
    # the next ncoedges coedges.
    loop_face, coedge_loop = [], []
    for f, face in enumerate(faces):
        loop_face += [f] * face["nloops"]
    for lp, lo in enumerate(loops):
        coedge_loop += [lp] * lo["ncoedges"]
    assert len(loop_face) == len(loops) and len(coedge_loop) == len(coedges)
    coedge_face = [loop_face[coedge_loop[c]] for c in range(len(coedges))]

    def fid(i):
        return faces[i]["id"]

    q = {name: {} for name in QUERIES}
    for e, edge in enumerate(edges):
        q["faces_of_edge"][e] = [fid(coedge_face[c]) for c in range(len(coedges)) if coedges[c]["edge"] == e]
        q["coedges_of_edge"][e] = [coedges[c]["id"] for c in range(len(coedges)) if coedges[c]["edge"] == e]
    for f in range(len(faces)):
        mine = [c for c in range(len(coedges)) if coedge_face[c] == f]
        my_edges = {coedges[c]["edge"] for c in mine}
        q["edges_of_face"][f] = [edges[e]["id"] for e in my_edges]
        q["loops_of_face"][f] = [loops[lp]["id"] for lp in range(len(loops)) if loop_face[lp] == f]
        q["face_neighbours"][f] = [
            fid(coedge_face[c]) for c in range(len(coedges)) if coedges[c]["edge"] in my_edges and coedge_face[c] != f
        ]
    for v in range(len(vertices)):
        touching = {e for e in range(len(edges)) if v in (edges[e]["start"], edges[e]["end"])}
        q["edges_of_vertex"][v] = [edges[e]["id"] for e in touching]
        q["faces_of_vertex"][v] = [fid(coedge_face[c]) for c in range(len(coedges)) if coedges[c]["edge"] in touching]

    keyed = {"faces_of_edge": edges, "coedges_of_edge": edges, "edges_of_face": faces, "loops_of_face": faces,
             "face_neighbours": faces, "faces_of_vertex": vertices, "edges_of_vertex": vertices}
    result = {"faces_of_body": sorted_ids(face["id"] for face in faces)}
    for name in QUERIES:
        ents = keyed[name]
        order = sorted(range(len(ents)), key=lambda i: id_key(ents[i]["id"]))
        result[name] = {id_text(ents[i]["id"]): sorted_ids(q[name][i]) for i in order}
    return result


def check_cube_rules(adj):
    """Hand-stated rules of a cube (AC-1), independent of the scan above."""
    faces = adj["faces_of_body"]
    assert len(faces) == 6
    for f, nb in adj["face_neighbours"].items():
        assert len(nb) == 4 and f not in nb, (f, nb)
        assert len(adj["edges_of_face"][f]) == 4 and len(adj["loops_of_face"][f]) == 1
    # Exactly three opposite pairs: the face missing from a face's neighbours (and itself).
    opposite = {f: [g for g in faces if g != f and g not in adj["face_neighbours"][f]] for f in faces}
    for f, o in opposite.items():
        assert len(o) == 1 and opposite[o[0]] == [f]
    for e, fs in adj["faces_of_edge"].items():
        assert len(fs) == 2 and len(adj["coedges_of_edge"][e]) == 2
    for v in adj["edges_of_vertex"]:
        assert len(adj["edges_of_vertex"][v]) == 3 and len(adj["faces_of_vertex"][v]) == 3
    # Labelled cube: faces #51..#56 hold the planes x=0, x=1, y=0, y=1, z=0, z=1 (script
    # order, a = 0, 1, 2 and side 0, 1); opposite faces are consecutive label pairs.
    if faces[0].startswith("#"):
        for a in range(3):
            lo, hi = f"#{51 + 2 * a}", f"#{52 + 2 * a}"
            assert opposite[lo] == [hi], (lo, opposite[lo])


def load(rel):
    return json.loads((HERE / rel).read_text(encoding="utf-8"), object_pairs_hook=topo.no_dup_keys)


def build_all():
    files = {}
    for name, script, desc in (
        ("cylinder_seam", cylinder_seam(), "uncapped cylinder side: one face closed by a seam edge, open shell"),
        ("bowtie_vertex", bowtie_vertex(), "two faces in two open shells meeting only at one (non-manifold) vertex"),
        ("lens_sphere", lens_sphere(), "sphere as two lunes sharing two meridian edges and both pole vertices"),
    ):
        body = {"description": desc, "license": LICENSE}
        body.update(script)
        files[f"{name}.json"] = topo.dump(body)

    def script_of(name):
        rel = f"{name}.json"
        return json.loads(files[rel], object_pairs_hook=topo.no_dup_keys) if rel in files else load(rel)

    cube = adjacency(script_of("cube_labelled"))
    check_cube_rules(cube)
    check_cube_rules(adjacency(script_of("cube_unlabelled")))
    files["cube_adjacency.json"] = topo.dump(
        {
            "description": "GEO.CAD.TOPO.ADJ AC-1: every adjacency query on cube_labelled.json; generated by "
            "make_adj_fixtures.py (literal scans, independent of fid-geo-cad) and checked against hand-stated cube rules",
            "license": LICENSE,
            "fixture": "cube_labelled",
            "adjacency": cube,
        }
    )
    expected = {
        "description": "GEO.CAD.TOPO.ADJ expected adjacency of every other valid fixture; generated by "
        "make_adj_fixtures.py (literal scans, independent of fid-geo-cad)",
        "license": LICENSE,
        "fixtures": {name: adjacency(script_of(name)) for name in VALID},
    }

    seam = expected["fixtures"]["cylinder_seam"]
    assert seam["faces_of_edge"]["#seam"] == ["#side"] and seam["face_neighbours"]["#side"] == []
    assert len(seam["coedges_of_edge"]["#seam"]) == 2
    lens = expected["fixtures"]["lens_sphere"]
    assert lens["face_neighbours"] == {"#lune_a": ["#lune_b"], "#lune_b": ["#lune_a"]}
    for pole in ("#north", "#south"):
        assert lens["faces_of_vertex"][pole] == ["#lune_a", "#lune_b"]
        assert lens["edges_of_vertex"][pole] == ["#m_east", "#m_west"]
    bow = expected["fixtures"]["bowtie_vertex"]
    shared = [v for v, fs in bow["faces_of_vertex"].items() if len(fs) == 2]
    assert len(shared) == 1 and len(bow["edges_of_vertex"][shared[0]]) == 4
    assert all(nb == [] for nb in bow["face_neighbours"].values())
    assert sorted(len(x) for x in bow["loops_of_face"].values()) == [1, 2]

    # A non-manifold edge (three coedges) never reaches a body: the model refuses it.
    try:
        topo.check(load("invalid/sphere_three_coedges.json"))
    except topo.BuildError as e:
        assert (e.code, e.message) == ("BadPairing", "edge 0: used by more than two coedges"), (e.code, e.message)
    else:
        raise AssertionError("sphere_three_coedges was accepted")

    files["adjacency_expected.json"] = topo.dump(expected)
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
            path.write_text(text, encoding="utf-8", newline="\n")
    if drift:
        print("out of date: " + ", ".join(drift), file=sys.stderr)
        return 1
    print("ok" if check_only else "written")
    return 0


if __name__ == "__main__":
    sys.exit(main())
