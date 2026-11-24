#!/usr/bin/env python3
"""Independent reference for `fid-design glyph-distinct` (spec node UX.ICONS.CLASS).

CC-BY-4.0 project data. Python standard library only.

Rasterises the 16 px masters of the meaning-carrying glyphs (side-effect class icons,
display-state glyphs and provenance badges) the way the UX.ICONS.CLASS Design states:
4x supersampling with anti-aliasing off (one sample at the centre of each quarter-pixel
cell, 64 x 64 samples), box-downsampled, a pixel covered when at least 8 of its 16 samples
are covered (coverage threshold 0.5). Every covered sample is decided with exact dyadic
arithmetic for the shapes used by the shipped glyphs (lines, polylines, polygons, straight
path segments, circles and rectangles with equal corner radii), so this script and the Rust
tool must agree bit for bit.

Metric: for each pair of glyphs in a group, the symmetric difference of covered pixels
divided by their union, minimised over integer shifts of the second glyph in -1..1 px on both
axes. Silhouettes (for the shape-family rules REQ-003 and REQ-004) are the covered pixels plus
every pixel the background cannot reach from the canvas border through 4-connected
uncovered pixels.

Only the SVG subset the shipped glyphs use is read here (no curves, no arcs); the Rust tool
supports more and is unit-tested on hand-labelled cases for the rest.

Run:  python fixtures/ux/icons/class/glyph_oracle.py            (writes expected.json)
      python fixtures/ux/icons/class/glyph_oracle.py --check    (fails if it is stale)
      python fixtures/ux/icons/class/glyph_oracle.py --preview design/icons   (ASCII art)
"""

import json
import math
import re
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

REPO = Path(__file__).resolve().parents[4]
HERE = Path(__file__).resolve().parent
OUT = HERE / "expected.json"

SS = 4  # supersampling factor
N = 16  # master size
COVER_MIN = 8  # samples of 16 for a covered pixel

GROUPS = [
    ("class", ["class.r0", "class.r1", "class.w1", "class.w2", "class.w3", "class.x1",
               "class.i1", "class.d1", "class.m1", "class.s1", "class.a0"]),
    ("state", ["state.conforms", "state.nonconforms", "state.indeterminate",
               "state.not-decidable", "state.not-evaluated", "state.stale"]),
    ("prov", ["prov.computed", "prov.measured", "prov.entered", "prov.reported",
              "prov.suggested"]),
]
FAMILY = ["class.w3", "class.m1", "class.s1", "class.a0"]
STATE_APART = (["state.stale", "state.not-evaluated"], ["state.conforms", "state.nonconforms"])
# Family members' silhouettes must differ by at most 1/10 [unverified] (a shared outer shape).
FAMILY_SAME = (1, 10)

ROOTS = [
    "design/icons",
    "fixtures/ux/icons/class/twins",
    "fixtures/ux/icons/class/missing-class",
]

NS = "{http://www.w3.org/2000/svg}"
INHERIT = ["fill", "stroke", "stroke-width", "stroke-linecap", "stroke-linejoin", "fill-rule"]
DEFAULTS = {"fill": "currentColor", "stroke": "none", "stroke-width": "1",
            "stroke-linecap": "butt", "stroke-linejoin": "miter", "fill-rule": "nonzero"}


def nums(s):
    return [float(t) for t in re.findall(r"[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?", s)]


def parse_path(d):
    """Subpaths [(points, closed)] for M/L/H/V/Z (absolute and relative) only."""
    toks = re.findall(r"[A-Za-z]|[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?", d)
    subs, pts, closed = [], None, False
    x = y = sx = sy = 0.0
    cmd, i = None, 0
    while i < len(toks):
        t = toks[i]
        if t.isalpha():
            cmd = t
            i += 1
            if cmd in "Zz":
                if pts is not None:
                    subs.append((pts, True))
                pts = None
                x, y = sx, sy
                continue
        if cmd in "Mm":
            if pts is not None:
                subs.append((pts, False))
            a, b = float(toks[i]), float(toks[i + 1])
            i += 2
            x, y = (a, b) if cmd == "M" else (x + a, y + b)
            sx, sy = x, y
            pts = [(x, y)]
            cmd = "L" if cmd == "M" else "l"
        elif cmd in "Ll":
            a, b = float(toks[i]), float(toks[i + 1])
            i += 2
            x, y = (a, b) if cmd == "L" else (x + a, y + b)
            if pts is None:
                pts = [(sx, sy)]
            pts.append((x, y))
        elif cmd in "Hh":
            a = float(toks[i])
            i += 1
            x = a if cmd == "H" else x + a
            if pts is None:
                pts = [(sx, sy)]
            pts.append((x, y))
        elif cmd in "Vv":
            a = float(toks[i])
            i += 1
            y = a if cmd == "V" else y + a
            if pts is None:
                pts = [(sx, sy)]
            pts.append((x, y))
        else:
            raise ValueError(f"path command {cmd!r} is outside the oracle's subset")
    if pts is not None:
        subs.append((pts, False))
    return subs


def shapes_of(svg_path):
    """List of shape dicts with resolved paint attributes."""
    root = ET.parse(svg_path).getroot()
    out = []

    def walk(el, inh):
        cur = dict(inh)
        for k in INHERIT:
            if k in el.attrib:
                cur[k] = el.attrib[k]
        tag = el.tag.replace(NS, "")
        shape = None
        a = el.attrib
        if tag == "circle":
            shape = {"kind": "circle", "c": (float(a.get("cx", 0)), float(a.get("cy", 0))), "r": float(a["r"])}
        elif tag == "rect":
            rx, ry = a.get("rx"), a.get("ry")
            if rx is None:
                rx = ry
            if ry is None:
                ry = rx
            x, y, w, h = (float(a.get("x", 0)), float(a.get("y", 0)), float(a["width"]), float(a["height"]))
            rx = min(float(rx or 0), w / 2)
            ry = min(float(ry or 0), h / 2)
            if rx != ry:
                raise ValueError("rect with rx != ry is outside the oracle's subset")
            shape = {"kind": "rect", "x": x, "y": y, "w": w, "h": h, "rho": rx,
                     "miter": cur["stroke-linejoin"] in ("miter", "miter-clip")}
        elif tag == "line":
            p = [(float(a.get("x1", 0)), float(a.get("y1", 0))), (float(a.get("x2", 0)), float(a.get("y2", 0)))]
            shape = {"kind": "poly", "subs": [(p, False)], "line": True}
        elif tag in ("polyline", "polygon"):
            v = nums(a["points"])
            p = [(v[k], v[k + 1]) for k in range(0, len(v), 2)]
            shape = {"kind": "poly", "subs": [(p, tag == "polygon")], "line": False}
        elif tag == "path":
            shape = {"kind": "poly", "subs": parse_path(a["d"]), "line": False}
        if shape is not None:
            shape.update(
                fill=cur["fill"] != "none" and not shape.get("line", False),
                stroke=cur["stroke"] != "none",
                hw=float(cur["stroke-width"]) / 2,
                cap=cur["stroke-linecap"],
                evenodd=cur["fill-rule"] == "evenodd",
            )
            out.append(shape)
        for ch in el:
            walk(ch, cur)

    walk(root, DEFAULTS)
    return out


def seg_body(px, py, a, b, hw):
    """Inside the butt-ended stroke body of segment a-b (exact for dyadic inputs)."""
    dx, dy = b[0] - a[0], b[1] - a[1]
    dd = dx * dx + dy * dy
    if dd == 0:
        return False
    ux, uy = px - a[0], py - a[1]
    s = ux * dx + uy * dy
    if s < 0 or s > dd:
        return False
    cr = ux * dy - uy * dx
    return cr * cr <= hw * hw * dd


def disk(px, py, c, hw):
    ux, uy = px - c[0], py - c[1]
    return ux * ux + uy * uy <= hw * hw


def square_cap(px, py, end, other, hw):
    """The square cap beyond `end` of segment other->end."""
    dx, dy = end[0] - other[0], end[1] - other[1]
    dd = dx * dx + dy * dy
    if dd == 0:
        return False
    ln = math.sqrt(dd)
    ux, uy = px - end[0], py - end[1]
    s = ux * dx + uy * dy
    if s < 0 or s > hw * ln:
        return False
    cr = ux * dy - uy * dx
    return cr * cr <= hw * hw * dd


def poly_stroke(px, py, pts, closed, hw, cap):
    n = len(pts)
    segs = [(pts[k], pts[k + 1]) for k in range(n - 1)]
    if closed and n > 1:
        segs.append((pts[-1], pts[0]))
    for a, b in segs:
        if seg_body(px, py, a, b, hw):
            return True
    # Joins (every join rasterised as round) and caps.
    if closed:
        for v in pts:
            if disk(px, py, v, hw):
                return True
        return False
    for v in pts[1:-1]:
        if disk(px, py, v, hw):
            return True
    if n < 2:
        return False
    if cap == "round":
        return disk(px, py, pts[0], hw) or disk(px, py, pts[-1], hw)
    if cap == "square" and n > 1:
        return square_cap(px, py, pts[0], pts[1], hw) or square_cap(px, py, pts[-1], pts[-2], hw)
    return False


def winding(px, py, subs):
    w = 0
    for pts, _closed in subs:
        n = len(pts)
        for k in range(n):
            a, b = pts[k], pts[(k + 1) % n]
            cr = (b[0] - a[0]) * (py - a[1]) - (px - a[0]) * (b[1] - a[1])
            if a[1] <= py < b[1] and cr > 0:
                w += 1
            elif b[1] <= py < a[1] and cr < 0:
                w -= 1
    return w


def crossings(px, py, subs):
    c = 0
    for pts, _closed in subs:
        n = len(pts)
        for k in range(n):
            a, b = pts[k], pts[(k + 1) % n]
            cr = (b[0] - a[0]) * (py - a[1]) - (px - a[0]) * (b[1] - a[1])
            if (a[1] <= py < b[1] and cr > 0) or (b[1] <= py < a[1] and cr < 0):
                c += 1
    return c


def covers(sh, px, py):
    hw = sh["hw"]
    if sh["kind"] == "circle":
        cx, cy = sh["c"]
        r = sh["r"]
        d2 = (px - cx) * (px - cx) + (py - cy) * (py - cy)
        if sh["fill"] and d2 <= r * r:
            return True
        if sh["stroke"]:
            lo = r - hw
            hi = r + hw
            if d2 <= hi * hi and (lo <= 0 or d2 >= lo * lo):
                return True
        return False
    if sh["kind"] == "rect":
        hx, hy = sh["w"] / 2, sh["h"] / 2
        if hx <= 0 or hy <= 0:
            return False
        cx, cy = sh["x"] + hx, sh["y"] + hy
        rho = sh["rho"]
        qx = abs(px - cx) - (hx - rho)
        qy = abs(py - cy) - (hy - rho)
        corner = qx > 0 and qy > 0
        if sh["fill"]:
            if corner and qx * qx + qy * qy <= rho * rho:
                return True
            if not corner and max(qx, qy) <= rho:
                return True
        if sh["stroke"]:
            if rho == 0 and sh["miter"]:
                # Square corners: the outer box minus the inner box.
                ax, ay = abs(px - cx), abs(py - cy)
                return ax <= hx + hw and ay <= hy + hw and not (ax < hx - hw and ay < hy - hw)
            if corner:
                d2 = qx * qx + qy * qy
                lo, hi = rho - hw, rho + hw
                if d2 <= hi * hi and (lo <= 0 or d2 >= lo * lo):
                    return True
            elif abs(max(qx, qy) - rho) <= hw:
                return True
        return False
    subs = sh["subs"]
    if sh["fill"]:
        if sh["evenodd"]:
            if crossings(px, py, subs) % 2 == 1:
                return True
        elif winding(px, py, subs) != 0:
            return True
    if sh["stroke"]:
        for pts, closed in subs:
            if poly_stroke(px, py, pts, closed, hw, sh["cap"]):
                return True
    return False


def raster(svg_path):
    shapes = shapes_of(svg_path)
    rows = []
    for py_ in range(N):
        row = 0
        for px_ in range(N):
            cnt = 0
            for sy in range(SS):
                for sx in range(SS):
                    x = px_ + (sx + 0.5) / SS
                    y = py_ + (sy + 0.5) / SS
                    if any(covers(s, x, y) for s in shapes):
                        cnt += 1
            if cnt >= COVER_MIN:
                row |= 1 << px_
        rows.append(row)
    return rows


def silhouette(rows):
    full = (1 << N) - 1
    seen = [[False] * N for _ in range(N)]
    stack = []
    for i in range(N):
        for (x, y) in ((i, 0), (i, N - 1), (0, i), (N - 1, i)):
            if not (rows[y] >> x) & 1 and not seen[y][x]:
                seen[y][x] = True
                stack.append((x, y))
    while stack:
        x, y = stack.pop()
        for nx, ny in ((x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)):
            if 0 <= nx < N and 0 <= ny < N and not seen[ny][nx] and not (rows[ny] >> nx) & 1:
                seen[ny][nx] = True
                stack.append((nx, ny))
    out = []
    for y in range(N):
        r = 0
        for x in range(N):
            if not seen[y][x]:
                r |= 1 << x
        out.append(r & full)
    return out


def shifted(rows, dx, dy):
    full = (1 << N) - 1
    out = []
    for y in range(N):
        sy = y - dy
        r = rows[sy] if 0 <= sy < N else 0
        r = (r << dx) & full if dx >= 0 else r >> (-dx)
        out.append(r)
    return out


def popcount(v):
    return bin(v).count("1")


def best(a, b):
    """(symdiff, union, dx, dy) with the smallest ratio; ties keep the first (dy, dx order)."""
    res = None
    for dy in (-1, 0, 1):
        for dx in (-1, 0, 1):
            s = shifted(b, dx, dy)
            sd = sum(popcount(a[y] ^ s[y]) for y in range(N))
            un = sum(popcount(a[y] | s[y]) for y in range(N))
            if un == 0:
                continue
            if res is None or sd * res[1] < res[0] * un:
                res = (sd, un, dx, dy)
    return res


def below(sd, un, num, den):
    """sd/un < num/den."""
    return sd * den < num * un


def check_root(rel, num=20, den=100):
    root = REPO / rel
    idx = {}
    ip = root / "index.json"
    if ip.exists():
        for e in json.loads(ip.read_text(encoding="utf-8"))["icons"]:
            idx[e["name"]] = e
    findings = []
    rasters = {}
    for _g, names in GROUPS:
        for n in names:
            p = root / "svg" / f"{n}.16.svg"
            e = idx.get(n)
            if e is None or e.get("a11y_id") != f"icon.{n}":
                findings.append(["GLYPH-MISSING", n])
            if not p.exists():
                findings.append(["GLYPH-MISSING", n])
                continue
            r = raster(p)
            if not any(r):
                findings.append(["GLYPH-RASTER", n])
                continue
            rasters[n] = r
    groups = {}
    for g, names in GROUPS:
        have = [n for n in names if n in rasters]
        mn = None
        for i in range(len(have)):
            for j in range(i + 1, len(have)):
                sd, un, dx, dy = best(rasters[have[i]], rasters[have[j]])
                if below(sd, un, num, den):
                    findings.append(["GLYPH-SIMILAR", f"{have[i]} {have[j]}"])
                if mn is None or sd * mn["union"] < mn["symdiff"] * un:
                    mn = {"a": have[i], "b": have[j], "symdiff": sd, "union": un, "shift": [dx, dy]}
        groups[g] = {"glyphs": len(have), "min": mn}
    sil = {n: silhouette(r) for n, r in rasters.items()}
    classes = GROUPS[0][1]
    fam = [n for n in classes if n in FAMILY and n in sil]
    for i in range(len(fam)):
        for j in range(i + 1, len(fam)):
            sd, un, _, _ = best(sil[fam[i]], sil[fam[j]])
            if not sd * FAMILY_SAME[1] <= FAMILY_SAME[0] * un:
                findings.append(["GLYPH-FAMILY", f"{fam[i]} {fam[j]}"])
    for o in [n for n in classes if n not in FAMILY and n in sil]:
        for f in fam:
            sd, un, _, _ = best(sil[o], sil[f])
            if below(sd, un, num, den):
                findings.append(["GLYPH-FAMILY", f"{o} {f}"])
    for a in STATE_APART[0]:
        for b in STATE_APART[1]:
            if a in sil and b in sil:
                sd, un, _, _ = best(sil[a], sil[b])
                if below(sd, un, num, den):
                    findings.append(["GLYPH-FAMILY", f"{a} {b}"])
    findings = sorted({tuple(f) for f in findings})
    findings = [list(f) for f in findings]
    return {"groups": groups, "findings": findings}


def build():
    return {"format": "glyph-distinct-expected@1", "threshold": "0.20",
            "roots": {r: check_root(r) for r in ROOTS}}


def preview(rel):
    root = REPO / rel
    for _g, names in GROUPS + [("pattern", ["pattern.oot-stripe"])]:
        for n in names:
            p = root / "svg" / f"{n}.16.svg"
            if not p.exists():
                print(n, "(missing)")
                continue
            r = raster(p)
            s = silhouette(r)
            print(n)
            for y in range(N):
                print("  " + "".join("#" if (r[y] >> x) & 1 else ("+" if (s[y] >> x) & 1 else ".")
                                     for x in range(N)))
    res = check_root(rel)
    print(json.dumps(res, indent=1))


def main():
    if len(sys.argv) > 2 and sys.argv[1] == "--preview":
        preview(sys.argv[2])
        return 0
    text = json.dumps(build(), indent=2, sort_keys=True) + "\n"
    if "--check" in sys.argv:
        if not OUT.exists() or OUT.read_text(encoding="utf-8") != text:
            print(f"{OUT.relative_to(REPO)} is stale: run {Path(__file__).relative_to(REPO)}", file=sys.stderr)
            return 1
        print("ok")
        return 0
    OUT.write_text(text, encoding="utf-8", newline="\n")
    print(f"wrote {OUT.relative_to(REPO)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
