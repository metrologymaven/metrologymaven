#!/usr/bin/env python3
"""Independent reference for GEO.SEL.RESOLVE; writes fixtures/geo/sel/lasso_cases/cases.txt.

Stdlib only (decimal, hashlib, math, os, random, struct, sys). Shares no code with
crates/fid-geo-sel.

Generates (seed 72, CC-BY-4.0):
  1. 200 seeded cameras and lasso polygons (convex and concave; perspective and orthographic)
     aimed at the origin / [0, 0.1]^3 region so both `mesh_sphere_1e5` and `pointset_small`
     are meaningfully intersected, together with independent brute-force reference results on
     `pointset_small` (both Through and Visible modes).
  2. 100 seeded pick pixels against `mesh_sphere_1e5` (100 820 triangles), with independent
     brute-force nearest-hit triangle index and ray parameter `t`.
  3. Edge-on-lasso cases (AC-3): points placed exactly on horizontal, vertical, diagonal,
     concave and vertex-shared edges, with independent half-open even-odd results.

Every perspective `fov_y` is verified with 40-digit Decimal arithmetic to lie well away from
any IEEE-754 halfway rounding boundary of `tan(fov_y * 0.5)`, so every faithful libm
(including `fid-num`'s pure-Rust libm and platform CPython `math.tan`) produces the exact
same 64 bits.

Run:  python tools/fixtures/geo_sel_resolve.py [--check]
With --check nothing is written; the script exits non-zero if the committed file differs.
"""

from decimal import Decimal, getcontext
import hashlib
import math
import os
import random
import struct
import sys

ROOT = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))
OUT = os.path.join(ROOT, "fixtures", "geo", "sel", "lasso_cases", "cases.txt")
POINTSET_SMALL = os.path.join(ROOT, "fixtures", "geo", "types", "pointset_small.txt")

sys.path.insert(0, os.path.join(ROOT, "tools", "fixtures"))
sys.path.insert(0, os.path.join(ROOT, "fixtures", "geo", "index"))
import geo_sel_sets as sel_ref  # noqa: E402
import make_bvh_fixtures as bvh_ref  # noqa: E402

SEED = 72
N_LASSO_CASES = 200
N_PICK_CASES = 100
DEFAULT_DEPTH_TOL_FRACTION = 1e-6


def hx(x):
    return f"{struct.unpack('<Q', struct.pack('<d', x))[0]:016x}"


def hexf(s):
    return struct.unpack("<d", struct.pack("<Q", int(s, 16)))[0]


def decimal_tan(x_float):
    """High-precision tan(x) via Taylor series for sin/cos on |x| < pi/2."""
    getcontext().prec = 50
    x = Decimal(x_float)
    x2 = x * x
    s = x
    c = Decimal(1)
    term_s = x
    term_c = Decimal(1)
    for k in range(1, 30):
        term_c = -term_c * x2 / Decimal((2 * k - 1) * (2 * k))
        term_s = -term_s * x2 / Decimal((2 * k) * (2 * k + 1))
        c += term_c
        s += term_s
    return s / c


def safe_fov_y(rng):
    """Draw fov_y in [0.55, 1.25] whose tan(fov_y * 0.5) is >= 0.05 ULP from any midpoint."""
    while True:
        fov = rng.uniform(0.55, 1.25)
        half = fov * 0.5
        t_f = math.tan(half)
        t_exact = decimal_tan(half)
        ulp = Decimal(math.ulp(t_f))
        if abs(t_exact - Decimal(t_f)) <= Decimal("0.40") * ulp:
            return fov


def dot3(a, b):
    return (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]


def cross3(a, b):
    return (
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    )


def sub3(a, b):
    return (a[0] - b[0], a[1] - b[1], a[2] - b[2])


def norm3(v):
    return math.sqrt(dot3(v, v))


def look_at_rigid(eye, target, up_hint):
    """Build right-handed view rigid transform (element -> camera, looking along -Z)."""
    f = sub3(target, eye)
    fn = norm3(f)
    fwd = (f[0] / fn, f[1] / fn, f[2] / fn)
    s = cross3(fwd, up_hint)
    sn = norm3(s)
    right = (s[0] / sn, s[1] / sn, s[2] / sn)
    up = cross3(right, fwd)
    # Camera axes: +X = right, +Y = up, +Z = -fwd
    z_axis = (-fwd[0], -fwd[1], -fwd[2])
    r = (right, up, z_axis)
    # t = -R * eye
    t = (
        -dot3(r[0], eye),
        -dot3(r[1], eye),
        -dot3(r[2], eye),
    )
    return r, t


def apply_rigid(r, t, p):
    return (
        dot3(r[0], p) + t[0],
        dot3(r[1], p) + t[1],
        dot3(r[2], p) + t[2],
    )


def invert_rigid(r, t):
    rt = (
        (r[0][0], r[1][0], r[2][0]),
        (r[0][1], r[1][1], r[2][1]),
        (r[0][2], r[1][2], r[2][2]),
    )
    ti = (
        -dot3(rt[0], t),
        -dot3(rt[1], t),
        -dot3(rt[2], t),
    )
    return rt, ti


def project_point(cam, p):
    r, t = cam["r"], cam["t"]
    c = apply_rigid(r, t, p)
    depth = -c[2]
    if depth < cam["near"] or depth > cam["far"]:
        return None
    if cam["kind"] == "persp":
        half_h = math.tan(cam["fov_y"] * 0.5)
        half_w = half_h * cam["aspect"]
        ndc_x = c[0] / (depth * half_w)
        ndc_y = c[1] / (depth * half_h)
    else:
        half_h = cam["height"] * 0.5
        half_w = half_h * cam["aspect"]
        ndc_x = c[0] / half_w
        ndc_y = c[1] / half_h
    sx = (ndc_x + 1.0) * 0.5 * float(cam["w"])
    sy = (1.0 - ndc_y) * 0.5 * float(cam["h"])
    return sx, sy, depth


def point_in_polygon(px, py, poly):
    inside = False
    a = poly[-1]
    for b in poly:
        if a[1] < b[1]:
            x0, y0, x1, y1 = a[0], a[1], b[0], b[1]
        else:
            x0, y0, x1, y1 = b[0], b[1], a[0], a[1]
        if y0 <= py < y1:
            lhs = (x1 - x0) * (py - y0)
            rhs = (px - x0) * (y1 - y0)
            if lhs > rhs:
                inside = not inside
        a = b
    return inside


def resolve_points_ref(pts, cam, poly, visible):
    w, h = cam["w"], cam["h"]
    depth_tol = (cam["far"] - cam["near"]) * DEFAULT_DEPTH_TOL_FRACTION
    zbuf = None
    if visible:
        zbuf = [math.inf] * (w * h)
        for p in pts:
            pr = project_point(cam, p)
            if pr is None:
                continue
            sx, sy, d = pr
            if 0.0 <= sx < w and 0.0 <= sy < h:
                idx = int(math.floor(sy)) * w + int(math.floor(sx))
                if d < zbuf[idx]:
                    zbuf[idx] = d
    out = []
    for i, p in enumerate(pts):
        pr = project_point(cam, p)
        if pr is None:
            continue
        sx, sy, d = pr
        if not point_in_polygon(sx, sy, poly):
            continue
        if visible:
            if not (0.0 <= sx < w and 0.0 <= sy < h):
                continue
            idx = int(math.floor(sy)) * w + int(math.floor(sx))
            bd = zbuf[idx]
            if not math.isinf(bd) and d > bd + depth_tol:
                continue
        out.append(i)
    return out


def unproject_ray(cam, px, py):
    w, h = float(cam["w"]), float(cam["h"])
    ndc_x = (px / w) * 2.0 - 1.0
    ndc_y = 1.0 - (py / h) * 2.0
    if cam["kind"] == "persp":
        half_h = math.tan(cam["fov_y"] * 0.5)
        half_w = half_h * cam["aspect"]
        o_cam = (0.0, 0.0, 0.0)
        d_cam = (ndc_x * half_w, ndc_y * half_h, -1.0)
    else:
        half_h = cam["height"] * 0.5
        half_w = half_h * cam["aspect"]
        o_cam = (ndc_x * half_w, ndc_y * half_h, 0.0)
        d_cam = (0.0, 0.0, -1.0)
    rt, ti = invert_rigid(cam["r"], cam["t"])
    o = apply_rigid(rt, ti, o_cam)
    d = (dot3(rt[0], d_cam), dot3(rt[1], d_cam), dot3(rt[2], d_cam))
    return o, d


def ray_tri(o, d, a, b, c, t_min, t_max):
    e1 = sub3(b, a)
    e2 = sub3(c, a)
    pvec = cross3(d, e2)
    det = dot3(e1, pvec)
    if det == 0.0:
        return None
    inv_det = 1.0 / det
    tvec = sub3(o, a)
    u = dot3(tvec, pvec) * inv_det
    if u < 0.0 or u > 1.0:
        return None
    qvec = cross3(tvec, e1)
    v = dot3(d, qvec) * inv_det
    if v < 0.0 or u + v > 1.0:
        return None
    t = dot3(e2, qvec) * inv_det
    if t < t_min or t > t_max:
        return None
    return t


def load_pointset_small():
    pts = []
    frame = None
    with open(POINTSET_SMALL, encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if line.startswith("frame "):
                frame = bytes.fromhex(line.split()[1])
            elif line.startswith("p "):
                parts = line.split()
                pts.append((hexf(parts[2]), hexf(parts[3]), hexf(parts[4])))
    return frame, pts


def make_camera(rng, idx):
    # Random direction on the sphere; distance 2.2..4.2 looking near (0.05, 0.05, 0.05)
    while True:
        ux = rng.uniform(-1.0, 1.0)
        uy = rng.uniform(-1.0, 1.0)
        uz = rng.uniform(-1.0, 1.0)
        n = math.sqrt(ux * ux + uy * uy + uz * uz)
        if 0.2 < n <= 1.0:
            ux, uy, uz = ux / n, uy / n, uz / n
            break
    dist = rng.uniform(2.3, 4.0)
    target = (
        0.05 + rng.uniform(-0.18, 0.18),
        0.05 + rng.uniform(-0.18, 0.18),
        0.05 + rng.uniform(-0.18, 0.18),
    )
    eye = (target[0] + ux * dist, target[1] + uy * dist, target[2] + uz * dist)
    # Choose an up vector not parallel to view direction
    up_hint = (0.0, 1.0, 0.0) if abs(uy) < 0.85 else (1.0, 0.0, 0.0)
    r, t = look_at_rigid(eye, target, up_hint)
    w, h = rng.choice([(64, 64), (80, 60), (96, 72), (96, 96), (120, 90)])
    aspect = float(w) / float(h)
    near = rng.uniform(0.3, 1.2)
    far = rng.uniform(6.0, 12.0)
    if idx % 2 == 0:
        return {
            "kind": "persp",
            "r": r,
            "t": t,
            "fov_y": safe_fov_y(rng),
            "aspect": aspect,
            "near": near,
            "far": far,
            "w": w,
            "h": h,
        }
    return {
        "kind": "ortho",
        "r": r,
        "t": t,
        "height": rng.uniform(1.6, 3.2),
        "aspect": aspect,
        "near": near,
        "far": far,
        "w": w,
        "h": h,
    }


def make_polygon(rng, w, h, concave):
    cx = rng.uniform(0.25 * w, 0.75 * w)
    cy = rng.uniform(0.25 * h, 0.75 * h)
    base_r = rng.uniform(0.18 * min(w, h), 0.45 * min(w, h))
    if not concave:
        n = rng.randint(3, 10)
        angles = sorted(rng.uniform(0.0, 2.0 * math.pi) for _ in range(n))
        # Keep convex by placing points on an ellipse with random rotation
        rx = base_r * rng.uniform(0.7, 1.3)
        ry = base_r * rng.uniform(0.7, 1.3)
        rot = rng.uniform(0.0, 2.0 * math.pi)
        cr, sr = math.cos(rot), math.sin(rot)
        pts = []
        for a in angles:
            lx = rx * math.cos(a)
            ly = ry * math.sin(a)
            pts.append((cx + lx * cr - ly * sr, cy + lx * sr + ly * cr))
        return pts
    # Concave star / indented polygon (6..24 vertices, alternating outer and inner radii)
    k = rng.randint(3, 12)
    n = 2 * k
    phase = rng.uniform(0.0, 2.0 * math.pi)
    pts = []
    for i in range(n):
        a = phase + 2.0 * math.pi * i / n
        rad = base_r * (rng.uniform(0.85, 1.25) if i % 2 == 0 else rng.uniform(0.20, 0.50))
        pts.append((cx + rad * math.cos(a), cy + rad * math.sin(a)))
    return pts


def fmt_cam(cam):
    r_hex = " ".join(hx(v) for row in cam["r"] for v in row)
    t_hex = " ".join(hx(v) for v in cam["t"])
    p0 = hx(cam["fov_y"]) if cam["kind"] == "persp" else hx(cam["height"])
    return (
        f"{cam['kind']} {cam['w']} {cam['h']} {p0} {hx(cam['aspect'])} "
        f"{hx(cam['near'])} {hx(cam['far'])} {r_hex} {t_hex}"
    )


def fmt_ranges(members):
    rs = sel_ref.ranges_of(members)
    return " ".join(f"{s}:{e}" for s, e in rs) if rs else "-"


def make_edge_cases():
    """AC-3: points placed exactly on polygon edges and vertices."""
    cases = []
    # 1. Axis-aligned rectangle [10, 20] x [10, 20]: selects exactly [10, 20) x [10, 20).
    rect = [(10.0, 10.0), (20.0, 10.0), (20.0, 20.0), (10.0, 20.0)]
    rect_pts = [
        (10.0, 10.0),  # top-left corner -> inside
        (15.0, 10.0),  # top edge (y = min_y) -> inside
        (20.0, 10.0),  # top-right corner -> outside
        (10.0, 15.0),  # left edge -> inside
        (20.0, 15.0),  # right edge -> outside
        (10.0, 20.0),  # bottom-left corner -> outside
        (15.0, 20.0),  # bottom edge (y = max_y) -> outside
        (20.0, 20.0),  # bottom-right corner -> outside
        (15.0, 15.0),  # interior -> inside
    ]
    cases.append(("rect_half_open", rect, rect_pts))

    # 2. Reversed winding of the same rectangle must give identical results.
    cases.append(("rect_reversed", list(reversed(rect)), rect_pts))

    # 3. Right triangle (0, 0), (16, 0), (0, 16) with points exactly on the diagonal x + y = 16.
    tri = [(0.0, 0.0), (16.0, 0.0), (0.0, 16.0)]
    tri_pts = [
        (0.0, 0.0),    # corner -> inside
        (8.0, 0.0),    # top edge -> inside
        (16.0, 0.0),   # right corner -> outside
        (0.0, 8.0),    # left edge -> inside
        (0.0, 16.0),   # bottom corner -> outside
        (8.0, 8.0),    # diagonal (right boundary) -> outside
        (4.0, 12.0),   # diagonal -> outside
        (12.0, 4.0),   # diagonal -> outside
        (4.0, 4.0),    # interior -> inside
    ]
    cases.append(("tri_diagonal", tri, tri_pts))

    # 4. Concave L-shape with reflex vertex at (10, 10).
    lshape = [(0.0, 0.0), (20.0, 0.0), (20.0, 10.0), (10.0, 10.0), (10.0, 20.0), (0.0, 20.0)]
    l_pts = [
        (10.0, 10.0),  # reflex vertex -> outside (right edge of lower arm)
        (15.0, 10.0),  # bottom edge of upper arm -> outside
        (10.0, 15.0),  # right edge of lower arm -> outside
        (5.0, 10.0),   # interior level with reflex vertex -> inside
        (0.0, 10.0),   # left edge level with reflex vertex -> inside
        (15.0, 5.0),   # interior upper arm -> inside
        (15.0, 15.0),  # exterior notch -> outside
    ]
    cases.append(("concave_l_reflex", lshape, l_pts))

    # 5. Adjacent triangles sharing diagonal edge (0, 0)-(16, 16): every point on the shared
    #    interior diagonal must belong to EXACTLY ONE of the two triangles.
    tri_a = [(0.0, 0.0), (16.0, 0.0), (16.0, 16.0)]
    tri_b = [(0.0, 0.0), (16.0, 16.0), (0.0, 16.0)]
    diag_pts = [(2.0, 2.0), (4.0, 4.0), (8.0, 8.0), (12.0, 12.0), (15.0, 15.0)]
    cases.append(("shared_diag_upper", tri_a, diag_pts))
    cases.append(("shared_diag_lower", tri_b, diag_pts))
    return cases


def build_text():
    rng = random.Random(SEED)
    _, pts_small = load_pointset_small()

    # Build mesh_sphere_1e5 using the exact canonical generator in make_bvh_fixtures.py.
    sphere_verts, sphere_tris = bvh_ref.sphere_mesh()
    # Partition triangle AABBs into a conservative 8x8x8 spatial grid over [-1.01, 1.01]^3 so
    # every ray tests only cells its AABB hits (100% zero-false-negative identical to full scan,
    # ~40x faster in Python for CI --check).
    grid_n = 8
    g_lo, g_hi = -1.01, 1.01
    g_step = (g_hi - g_lo) / grid_n
    cells = {}
    tri_data = []
    for t_idx, (i0, i1, i2) in enumerate(sphere_tris):
        a = sphere_verts[i0]
        b = sphere_verts[i1]
        c = sphere_verts[i2]
        lo = (min(a[0], b[0], c[0]), min(a[1], b[1], c[1]), min(a[2], b[2], c[2]))
        hi = (max(a[0], b[0], c[0]), max(a[1], b[1], c[1]), max(a[2], b[2], c[2]))
        tri_data.append((t_idx, a, b, c, lo, hi))
        ix0 = max(0, min(grid_n - 1, int(math.floor((lo[0] - g_lo) / g_step))))
        ix1 = max(0, min(grid_n - 1, int(math.floor((hi[0] - g_lo) / g_step))))
        iy0 = max(0, min(grid_n - 1, int(math.floor((lo[1] - g_lo) / g_step))))
        iy1 = max(0, min(grid_n - 1, int(math.floor((hi[1] - g_lo) / g_step))))
        iz0 = max(0, min(grid_n - 1, int(math.floor((lo[2] - g_lo) / g_step))))
        iz1 = max(0, min(grid_n - 1, int(math.floor((hi[2] - g_lo) / g_step))))
        for ix in range(ix0, ix1 + 1):
            for iy in range(iy0, iy1 + 1):
                for iz in range(iz0, iz1 + 1):
                    cells.setdefault((ix, iy, iz), []).append(t_idx)
    cell_list = []
    for (ix, iy, iz), idxs in sorted(cells.items()):
        clo = (g_lo + ix * g_step, g_lo + iy * g_step, g_lo + iz * g_step)
        chi = (g_lo + (ix + 1) * g_step, g_lo + (iy + 1) * g_step, g_lo + (iz + 1) * g_step)
        cell_list.append((clo, chi, idxs))

    lines = [
        "# GEO.SEL.RESOLVE fixture lasso_cases (AC-1, AC-2, AC-3, AC-4). CC-BY-4.0.",
        "# Written by tools/fixtures/geo_sel_resolve.py (seed 72); never edit by hand.",
        "# Format:",
        "#   lasso <idx> <convex|concave> <cam...> | <n_verts> <x0> <y0> ... | <ps_through_ranges> | <ps_visible_ranges>",
        "#   pick <idx> <cam...> | <px> <py> | <hit_tri_or_none> <hit_t_hex_or_none>",
        "#   edge <name> <n_verts> <x0> <y0> ... | <n_pts> <px0> <py0> <0|1> ...",
        f"seed {SEED}",
        f"lasso_count {N_LASSO_CASES}",
        f"pick_count {N_PICK_CASES}",
    ]

    # 1. 200 seeded cameras and lassos (100 convex, 100 concave)
    for i in range(N_LASSO_CASES):
        cam = make_camera(rng, i)
        concave = (i % 2) == 1
        poly = make_polygon(rng, cam["w"], cam["h"], concave)
        ps_through = resolve_points_ref(pts_small, cam, poly, False)
        ps_visible = resolve_points_ref(pts_small, cam, poly, True)
        poly_str = " ".join(f"{hx(x)} {hx(y)}" for x, y in poly)
        shape = "concave" if concave else "convex"
        lines.append(
            f"lasso {i} {shape} {fmt_cam(cam)} | {len(poly)} {poly_str} | "
            f"{fmt_ranges(ps_through)} | {fmt_ranges(ps_visible)}"
        )

    # 2. 100 seeded pick pixels against mesh_sphere_1e5
    for i in range(N_PICK_CASES):
        cam = make_camera(rng, i)
        # 80 picks aimed near centre (hits sphere), 20 picks across full viewport (mix of hits & misses)
        if i < 80:
            px = rng.uniform(0.30 * cam["w"], 0.70 * cam["w"])
            py = rng.uniform(0.30 * cam["h"], 0.70 * cam["h"])
        else:
            px = rng.uniform(0.0, float(cam["w"]))
            py = rng.uniform(0.0, float(cam["h"]))
        o, d = unproject_ray(cam, px, py)
        t_min, t_max = cam["near"], cam["far"]
        best = None
        seen = set()
        for clo, chi, idxs in cell_list:
            if not bvh_ref.ray_hit(clo, chi, o, d, t_min, t_max):
                continue
            for t_idx in idxs:
                if t_idx in seen:
                    continue
                seen.add(t_idx)
                _, a, b, c, lo, hi = tri_data[t_idx]
                if not bvh_ref.ray_hit(lo, hi, o, d, t_min, t_max):
                    continue
                t = ray_tri(o, d, a, b, c, t_min, t_max)
                if t is not None:
                    if best is None or t < best[0] or (t == best[0] and t_idx < best[1]):
                        best = (t, t_idx)
        if best is None:
            hit_str = "none none"
        else:
            hit_str = f"{best[1]} {hx(best[0])}"
        lines.append(f"pick {i} {fmt_cam(cam)} | {hx(px)} {hx(py)} | {hit_str}")

    # 3. Edge-on-lasso cases (AC-3)
    for name, poly, pts in make_edge_cases():
        poly_str = " ".join(f"{hx(x)} {hx(y)}" for x, y in poly)
        pt_items = []
        for px, py in pts:
            inside = 1 if point_in_polygon(px, py, poly) else 0
            pt_items.append(f"{hx(px)} {hx(py)} {inside}")
        lines.append(f"edge {name} {len(poly)} {poly_str} | {len(pts)} {' '.join(pt_items)}")

    return "\n".join(lines) + "\n"


def main():
    check = "--check" in sys.argv[1:]
    text = build_text()
    if check:
        if not os.path.isfile(OUT):
            print(f"missing fixture: {OUT}", file=sys.stderr)
            return 1
        with open(OUT, encoding="utf-8") as f:
            existing = f.read()
        if existing != text:
            print(f"fixture mismatch: {OUT}", file=sys.stderr)
            return 1
        print(f"ok ({hashlib.sha256(text.encode('utf-8')).hexdigest()[:16]})")
        return 0
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)
    print(f"wrote {OUT} ({len(text)} bytes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
