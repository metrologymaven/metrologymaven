#!/usr/bin/env python3
"""Generate and verify deterministic JSON mesh fixtures for IO.MESH.MBLD.

Licence: CC-BY-4.0.
Usage:
    python fixtures/io/mesh/mbld/make_fixtures.py
    python fixtures/io/mesh/mbld/make_fixtures.py --check
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

OUT_DIR = Path(__file__).resolve().parent


def build_fixtures() -> dict[str, dict]:
    # AC-1: a closed tetrahedron (4 faces) plus one face with a repeated vertex index
    # ([0, 0, 1]) and one extra fin face ([0, 1, 4]) sharing edge (0, 1) with two
    # tetrahedron faces -> 5 vertices, 6 faces, 1 degenerate face, 1 non-manifold edge,
    # 0 unreferenced vertices.
    tetra_plus_degenerate = {
        "schema": "fid-io-mesh-fixture/1",
        "source_precision": "f64",
        "vertices_m": [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.5, -1.0, 0.0],
        ],
        "faces": [
            [0, 1, 2],
            [0, 3, 1],
            [1, 3, 2],
            [2, 3, 0],
            [0, 0, 1],
            [0, 1, 4],
        ],
        "expected": {
            "vertices": 5,
            "faces": 6,
            "degenerate_faces": 1,
            "non_manifold_edges": 1,
            "unreferenced_vertices": 0,
        },
    }

    # Companion 5-face fixture matching AC-1's literal numeric tuple (5 faces, 1 degenerate,
    # 1 non-manifold edge, 0 unreferenced vertices): 3-face open pyramid + 1 repeated-index
    # face + 1 fin face on edge (0, 1).
    open_tetra_five_faces = {
        "schema": "fid-io-mesh-fixture/1",
        "source_precision": "f64",
        "vertices_m": [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.5, -1.0, 0.0],
        ],
        "faces": [
            [0, 1, 2],
            [0, 3, 1],
            [1, 3, 2],
            [0, 0, 1],
            [0, 1, 4],
        ],
        "expected": {
            "vertices": 5,
            "faces": 5,
            "degenerate_faces": 1,
            "non_manifold_edges": 1,
            "unreferenced_vertices": 0,
        },
    }

    # Collinear zero-area degenerate face (distinct indices) + 2 unreferenced vertices.
    zero_area_and_unreferenced = {
        "schema": "fid-io-mesh-fixture/1",
        "source_precision": "f64",
        "vertices_m": [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [10.0, 10.0, 10.0],
            [-5.0, 2.0, 1.0],
        ],
        "faces": [
            [0, 1, 3],
            [0, 1, 2],
        ],
        "expected": {
            "vertices": 6,
            "faces": 2,
            "degenerate_faces": 1,
            "non_manifold_edges": 0,
            "unreferenced_vertices": 2,
        },
    }

    # AC-2: 4-vertex tetrahedron (faces 0..3 valid) + face 4 with vertex index 9.
    bad_index_face4 = {
        "schema": "fid-io-mesh-fixture/1",
        "source_precision": "f64",
        "vertices_m": [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ],
        "faces": [
            [0, 1, 2],
            [0, 3, 1],
            [1, 3, 2],
            [2, 3, 0],
            [0, 1, 9],
        ],
        "expected_error": {
            "code": "malformed",
            "face": 4,
            "vertex_index": 9,
        },
    }

    # AC-3: single vertex at (5.000000123, 0, 0) m for f32 rounding report.
    precision_vertex = {
        "schema": "fid-io-mesh-fixture/1",
        "source_precision": "f64",
        "vertices_m": [
            [5.000000123, 0.0, 0.0],
        ],
        "faces": [],
    }

    return {
        "tetra_plus_degenerate.json": tetra_plus_degenerate,
        "open_tetra_five_faces.json": open_tetra_five_faces,
        "zero_area_and_unreferenced.json": zero_area_and_unreferenced,
        "bad_index_face4.json": bad_index_face4,
        "precision_vertex.json": precision_vertex,
    }


def render(doc: dict) -> str:
    return json.dumps(doc, indent=2, sort_keys=True) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="Verify committed fixtures are up to date")
    args = parser.parse_args()

    fixtures = build_fixtures()
    mismatches: list[str] = []

    for filename, doc in sorted(fixtures.items()):
        path = OUT_DIR / filename
        want = render(doc)
        if args.check:
            if not path.is_file() or path.read_text(encoding="utf-8") != want:
                mismatches.append(filename)
        else:
            path.write_text(want, encoding="utf-8")

    if args.check:
        if mismatches:
            print(f"fixtures/io/mesh/mbld out of date: {', '.join(mismatches)}", file=sys.stderr)
            return 1
        print(f"fixtures/io/mesh/mbld: ok ({len(fixtures)} files)")
    else:
        print(f"generated {len(fixtures)} fixture files in {OUT_DIR}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
