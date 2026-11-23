#!/usr/bin/env python3
"""Generate and verify deterministic PLY fixtures for IO.MESH.PLY.

Licence: CC-BY-4.0.
Usage:
    python fixtures/io/mesh/ply/make_fixtures.py
    python fixtures/io/mesh/ply/make_fixtures.py --check
"""

from __future__ import annotations

import argparse
import math
import struct
import sys
from pathlib import Path

OUT_DIR = Path(__file__).resolve().parent


def cube_verts_and_quads() -> tuple[list[tuple[float, float, float]], list[tuple[int, int, int, int]]]:
    s = 10.0
    verts = [
        (0.0, 0.0, 0.0),
        (s, 0.0, 0.0),
        (s, s, 0.0),
        (0.0, s, 0.0),
        (0.0, 0.0, s),
        (s, 0.0, s),
        (s, s, s),
        (0.0, s, s),
    ]
    quads = [
        (0, 3, 2, 1),
        (4, 5, 6, 7),
        (0, 1, 5, 4),
        (3, 7, 6, 2),
        (0, 4, 7, 3),
        (1, 2, 6, 5),
    ]
    return verts, quads


def make_cube_ascii(verts: list, quads: list) -> bytes:
    lines = [
        "ply",
        "format ascii 1.0",
        "comment fiducial cube ascii",
        f"element vertex {len(verts)}",
        "property float x",
        "property float y",
        "property float z",
        f"element face {len(quads)}",
        "property list uchar int vertex_indices",
        "end_header",
    ]
    for x, y, z in verts:
        lines.append(f"{x:g} {y:g} {z:g}")
    for q in quads:
        lines.append(f"4 {q[0]} {q[1]} {q[2]} {q[3]}")
    return ("\n".join(lines) + "\n").encode("ascii")


def make_cube_bin(verts: list, quads: list, endian: str) -> bytes:
    fmt = "binary_little_endian" if endian == "<" else "binary_big_endian"
    header = (
        "ply\n"
        f"format {fmt} 1.0\n"
        f"comment fiducial cube {fmt}\n"
        f"element vertex {len(verts)}\n"
        "property float x\n"
        "property float y\n"
        "property float z\n"
        f"element face {len(quads)}\n"
        "property list uchar int vertex_indices\n"
        "end_header\n"
    ).encode("ascii")
    body = bytearray()
    for x, y, z in verts:
        body.extend(struct.pack(f"{endian}3f", x, y, z))
    for q in quads:
        body.extend(struct.pack(f"{endian}B4i", 4, *q))
    return header + bytes(body)


def make_points_double_custom() -> bytes:
    """AC-2: 1,000 vertices, double xyz, float quality, uchar red/green/blue, no faces."""
    n = 1000
    header = (
        "ply\n"
        "format binary_little_endian 1.0\n"
        "comment 1000 double precision points with quality and RGB\n"
        f"element vertex {n}\n"
        "property double x\n"
        "property double y\n"
        "property double z\n"
        "property float quality\n"
        "property uchar red\n"
        "property uchar green\n"
        "property uchar blue\n"
        "end_header\n"
    ).encode("ascii")
    body = bytearray()
    for i in range(n):
        t = i / 1000.0
        # Non-trivial double-precision coordinates that lose precision if truncated to f32.
        x = 0.001 * i + 1.0 / (7.0 + i)
        y = math.sin(t * 3.141592653589793) * 0.25 + 1e-11 * i
        z = math.cos(t * 2.0) * 0.125 - 1.0 / (13.0 + i)
        q = float(struct.unpack("<f", struct.pack("<f", 0.5 + 0.0005 * i))[0])
        r = i % 256
        g = (i * 3) % 256
        b = (i * 7) % 256
        body.extend(struct.pack("<3df3B", x, y, z, q, r, g, b))
    return header + bytes(body)


def make_truncated(verts: list) -> bytes:
    """AC-4: header says 8 vertices, body has only 5."""
    lines = [
        "ply",
        "format ascii 1.0",
        "element vertex 8",
        "property float x",
        "property float y",
        "property float z",
        "end_header",
    ]
    for x, y, z in verts[:5]:
        lines.append(f"{x:g} {y:g} {z:g}")
    return ("\n".join(lines) + "\n").encode("ascii")


def make_with_edges(verts: list, quads: list) -> bytes:
    """AC-5: cube plus an extra `edge` element of 12 edges."""
    edges = [
        (0, 1), (1, 2), (2, 3), (3, 0),
        (4, 5), (5, 6), (6, 7), (7, 4),
        (0, 4), (1, 5), (2, 6), (3, 7),
    ]
    lines = [
        "ply",
        "format ascii 1.0",
        f"element vertex {len(verts)}",
        "property float x",
        "property float y",
        "property float z",
        f"element face {len(quads)}",
        "property list uchar int vertex_indices",
        f"element edge {len(edges)}",
        "property int vertex1",
        "property int vertex2",
        "end_header",
    ]
    for x, y, z in verts:
        lines.append(f"{x:g} {y:g} {z:g}")
    for q in quads:
        lines.append(f"4 {q[0]} {q[1]} {q[2]} {q[3]}")
    for a, b in edges:
        lines.append(f"{a} {b}")
    return ("\n".join(lines) + "\n").encode("ascii")


def build_fixtures() -> dict[str, bytes]:
    verts, quads = cube_verts_and_quads()
    return {
        "cube_ascii.ply": make_cube_ascii(verts, quads),
        "cube_bin_le.ply": make_cube_bin(verts, quads, "<"),
        "cube_bin_be.ply": make_cube_bin(verts, quads, ">"),
        "points_double_custom.ply": make_points_double_custom(),
        "truncated.ply": make_truncated(verts),
        "with_edges.ply": make_with_edges(verts, quads),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Generate or check IO.MESH.PLY fixtures.")
    parser.add_argument("--check", action="store_true", help="Verify fixtures match generated bytes.")
    args = parser.parse_args()

    fixtures = build_fixtures()
    mismatches: list[str] = []
    for name, expected in sorted(fixtures.items()):
        path = OUT_DIR / name
        if args.check:
            if not path.exists() or path.read_bytes() != expected:
                mismatches.append(name)
        else:
            path.write_bytes(expected)

    if args.check:
        if mismatches:
            print("IO.MESH.PLY fixture mismatch:", ", ".join(mismatches), file=sys.stderr)
            return 1
        print(f"IO.MESH.PLY fixtures OK ({len(fixtures)} files)")
    else:
        print(f"Wrote {len(fixtures)} IO.MESH.PLY fixtures to {OUT_DIR}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
