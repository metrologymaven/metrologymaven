#!/usr/bin/env python3
"""Independent reference for fixture `fixtures/viz/golden-cube` (VIZ.RENDER.FRAME.GRAPH AC-1,
shared with VIZ.RENDER.HEADLESS). CC-BY-4.0 project data.

Writes fixtures/viz/golden-cube/cube.txt. Stdlib only; nothing here runs the Rust code.

The scene: the cube [-1, 1]^3, six faces with render IDs 101..106, seen from eye (2.5, 2, 3.5)
looking at the origin (up +Y), on a 64 x 64 target. The projection is the reversed-Z infinite
perspective: with view coordinates (xv, yv, zv) (the camera looks down -zv),
clip = (f xv, f yv, near, -zv), f = 2 (focal length, aspect 1), near = 0.1, so the depth
near / -zv is 1 on the near plane and falls towards 0 far away; the depth buffer is cleared
to 0 and the test is `Greater`. Every face is two triangles; triangle t of face k has
primitive index 2k + t. The script writes each triangle's clip-space vertices (so the GPU
does only rasterisation, depth test and ID writes), and for every face that faces the camera
(normal . (eye - centre) > 0; the cube is convex, so these are the visible ones) the pixel
under the projected face centre, its render ID, the face's two primitive indices (the centre
lies on the shared diagonal, so either may cover it) and the reversed-Z depth at the pixel
centre, from intersecting the pixel's view ray with the face plane. Every expected pixel's
centre is checked to lie at least MARGIN pixels inside its projected face; the background
pixel is checked to be MARGIN pixels outside every face.

Run:  python fixtures/viz/golden-cube/make_fixtures.py          (writes the fixture)
      python fixtures/viz/golden-cube/make_fixtures.py --check  (fails if the fixture is stale)
"""

import math
import sys
from pathlib import Path

OUT = Path(__file__).parent / "cube.txt"
W = H = 64
EYE = (2.5, 2.0, 3.5)
F = 2.0
NEAR = 0.1
MARGIN = 2.0
BACKGROUND = (1, 1)
FIRST_ID = 101


def sub(a, b):
    return tuple(x - y for x, y in zip(a, b))


def dot(a, b):
    return sum(x * y for x, y in zip(a, b))


def cross(a, b):
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def unit(a):
    n = math.sqrt(dot(a, a))
    return tuple(x / n for x in a)


FWD = unit(sub((0.0, 0.0, 0.0), EYE))
RIGHT = unit(cross(FWD, (0.0, 1.0, 0.0)))
UP = cross(RIGHT, FWD)


def clip(p):
    d = sub(p, EYE)
    xv, yv, zv = dot(d, RIGHT), dot(d, UP), -dot(d, FWD)
    return (F * xv, F * yv, NEAR, -zv)


def pixel(c):
    x, y = c[0] / c[3], c[1] / c[3]
    return ((x + 1.0) / 2.0 * W, (1.0 - y) / 2.0 * H)


def faces():
    """(axis, sign, corners counter-clockwise seen from outside)."""
    out = []
    for axis in range(3):
        for sign in (1.0, -1.0):
            u, v = (axis + 1) % 3, (axis + 2) % 3
            corners = []
            for a, b in ((-1, -1), (1, -1), (1, 1), (-1, 1)):
                p = [0.0, 0.0, 0.0]
                p[axis], p[u], p[v] = sign, float(a), float(b) * sign
                corners.append(tuple(p))
            out.append((axis, sign, corners))
    return out


def inside_margin(q, quad):
    """Smallest distance from q to the edges of the convex quad, negative if outside."""
    area = 0.0
    for i in range(4):
        a, b = quad[i], quad[(i + 1) % 4]
        area += a[0] * b[1] - b[0] * a[1]
    s = 1.0 if area > 0 else -1.0
    worst = math.inf
    for i in range(4):
        a, b = quad[i], quad[(i + 1) % 4]
        e = (b[0] - a[0], b[1] - a[1])
        n = math.hypot(*e)
        d = s * (e[0] * (q[1] - a[1]) - e[1] * (q[0] - a[0])) / n
        worst = min(worst, d)
    return worst


def depth_at(px, py, axis, sign):
    x = (px / W) * 2.0 - 1.0
    y = 1.0 - (py / H) * 2.0
    d = tuple(RIGHT[i] * (x / F) + UP[i] * (y / F) + FWD[i] for i in range(3))
    t = (sign - EYE[axis]) / d[axis]
    return NEAR / t


def build():
    lines = [
        "# Golden cube (VIZ.RENDER.FRAME.GRAPH AC-1). Written by make_fixtures.py; do not edit.",
        "# size <width> <height>",
        "# tri <render_id> <primitive> then three vertices x y z w in clip space",
        "# expect <px> <py> <render_id> <primitive_a> <primitive_b> <reversed-Z depth at the pixel centre>",
        "# background <px> <py>",
        f"size {W} {H}",
    ]
    expects = []
    quads = []
    for k, (axis, sign, corners) in enumerate(faces()):
        rid = FIRST_ID + k
        cl = [clip(p) for p in corners]
        for t, tri in enumerate(((0, 1, 2), (0, 2, 3))):
            vs = " ".join(" ".join(repr(x) for x in cl[i]) for i in tri)
            lines.append(f"tri {rid} {2 * k + t} {vs}")
        quad = [pixel(c) for c in cl]
        quads.append(quad)
        centre = [0.0, 0.0, 0.0]
        centre[axis] = sign
        normal = [0.0, 0.0, 0.0]
        normal[axis] = sign
        if dot(normal, sub(EYE, centre)) <= 0:
            continue
        cx, cy = pixel(clip(tuple(centre)))
        px, py = int(math.floor(cx)), int(math.floor(cy))
        m = inside_margin((px + 0.5, py + 0.5), quad)
        assert m >= MARGIN, (rid, m)
        depth = depth_at(px + 0.5, py + 0.5, axis, sign)
        assert 0.0 < depth <= 1.0, depth
        expects.append(f"expect {px} {py} {rid} {2 * k} {2 * k + 1} {depth!r}")
    assert len(expects) == 3, expects
    bq = (BACKGROUND[0] + 0.5, BACKGROUND[1] + 0.5)
    for quad in quads:
        assert inside_margin(bq, quad) <= -MARGIN, quad
    lines += expects
    lines.append(f"background {BACKGROUND[0]} {BACKGROUND[1]}")
    return "\n".join(lines) + "\n"


def main():
    text = build()
    if "--check" in sys.argv[1:]:
        if OUT.read_text(encoding="utf-8") != text:
            sys.exit(f"{OUT} is stale; run {Path(__file__).name}")
        return
    OUT.write_text(text, encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
