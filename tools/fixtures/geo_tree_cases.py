#!/usr/bin/env python3
"""Independent reference for GEO.FRAME.TREE; writes fixtures/geo/frame/tree_7.json.

Run:  python tools/fixtures/geo_tree_cases.py [--check]
With --check nothing is written; the script exits non-zero if the file differs.

Stdlib only (fractions, hashlib, json, math, random, struct); shares no code with
fid-geo-frame or with tools/fixtures/geo_xform_cases.py. It only reads the *input*
transforms (r, t, s, kind) of fixtures/geo/frame/xform_cases.json and
fixtures/geo/frame/xform_worst_residual.json, never their expected results.

tree_7 (AC-1). Seven frames, depth 4:

    world -> part_a -> align_1 -> align_2 -> inst_1
    world -> part_b -> inst_2

Each frame's to-parent transform is a named case of xform_cases.json. For all 49 ordered
pairs (a, b) the expected resolve(a, b) is computed by a bit-exact model of the order pinned
in the leaf's Design and in crates/fid-geo-frame/src/tree.rs:

* chain(f -> c) = left fold from f upward: acc = T(f); for each next frame g strictly below
  c: acc = T(g) o acc. An empty chain (f == c) is absent, not the identity;
* resolve(a, a) = exact identity;
* only a's chain present: chain(a); only b's chain: inverse(chain(b));
  both: inverse(chain(b)) o chain(a).

CPython floats are IEEE binary64 with round-to-nearest-even and never fuse a multiply-add,
so the model's bits equal the Rust results on every target. Each model result is also
compared with the exact rational composition (fractions.Fraction) within 1e-14 per rotation
entry and 1e-12 m per translation component.

Digests (AC-2, AC-5). The tree digest is SHA-256 (hashlib) over layout version 1:

    "FIDFRT" | 0x01 | n_frames u64 | world id [16]
    per frame in ascending id-byte order:
        id [16] | kind u8 | has_parent u8 | parent [16] (zeros when none)
        | name_len u16 | name UTF-8 | xform u8 (1 rigid, 2 similarity)
        | R 9 x f64 row-major | t 3 x f64 | s f64 (1.0 for rigid)

little-endian integers, f64 as IEEE bits with -0.0 written as +0.0. Kind bytes: world 1,
part 2, instrument 3, alignment 4, user 5. The digest of tree_7 and of the tree after each
step of a fixed edit sequence are recorded.

AC-4 sanity. 20 000 random points in the 100 m cube (Python's generator, seed 23), point k
on pair k mod 49, are mapped a -> b -> a through the model; the largest error must be
<= 1e-12 m. The Rust test repeats this with 10^5 fid-num points.

Path bound. A second tree built from the worst-residual rotations of
xform_worst_residual.json (each accepted with residual close to 1e-12) checks the path
round-trip bound stated in tree.rs in exact arithmetic:

    |x' - x| <= rho_sum * M + 48 * 2^-53 * (2k + 4) * M,    M = |x| + sum |t_i|

over the k edges of the path (rho_sum = sum of the edge residuals). Every pair and 400
points per pair are checked.
"""

import hashlib
import json
import math
import random
import struct
import sys
from fractions import Fraction
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SRC_XFORM = ROOT / "fixtures" / "geo" / "frame" / "xform_cases.json"
SRC_WORST = ROOT / "fixtures" / "geo" / "frame" / "xform_worst_residual.json"
OUT = ROOT / "fixtures" / "geo" / "frame" / "tree_7.json"

EPS = 2.0 ** -53
KIND_BYTE = {"world": 1, "part": 2, "instrument": 3, "alignment": 4, "user": 5}


def hx(x):
    return f"{struct.unpack('<Q', struct.pack('<d', x))[0]:016x}"


def hxs(xs):
    return " ".join(hx(x) for x in xs)


def unhex(s):
    return [struct.unpack("<d", struct.pack("<Q", int(h, 16)))[0] for h in s.split(" ")]


# ------------------------------------------------------------------ float model


def dot3(a0, a1, a2, b0, b1, b2):
    return (a0 * b0 + a1 * b1) + a2 * b2


def compose(a, b):
    """a o b (b first). A transform is (rigid, R, t, s)."""
    ra, ta, sa = a[1], a[2], a[3]
    rb, tb, sb = b[1], b[2], b[3]
    r = [[dot3(ra[i][0], ra[i][1], ra[i][2], rb[0][j], rb[1][j], rb[2][j]) for j in range(3)] for i in range(3)]
    u = [dot3(ra[i][0], ra[i][1], ra[i][2], tb[0], tb[1], tb[2]) for i in range(3)]
    return (a[0] and b[0], r, [sa * u[i] + ta[i] for i in range(3)], sa * sb)


def inverse(a):
    rigid, r, t, s = a
    si = 1.0 / s
    u = [dot3(r[0][i], r[1][i], r[2][i], t[0], t[1], t[2]) for i in range(3)]
    return (rigid, [[r[0][i], r[1][i], r[2][i]] for i in range(3)], [-(si * u[i]) for i in range(3)], si)


def apply_point(a, x):
    _, r, t, s = a
    u = [dot3(r[i][0], r[i][1], r[i][2], x[0], x[1], x[2]) for i in range(3)]
    return [s * u[i] + t[i] for i in range(3)]


IDENT = (True, [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], [0.0, 0.0, 0.0], 1.0)


def residual(r):
    acc = 0.0
    for i in range(3):
        for j in range(3):
            g = dot3(r[0][i], r[1][i], r[2][i], r[0][j], r[1][j], r[2][j])
            d = g - 1.0 if i == j else g
            acc += d * d
    return math.sqrt(acc)  # correctly rounded


# ------------------------------------------------------------------ tree model


class Tree:
    def __init__(self):
        self.frames = {}  # name -> dict(id, kind, parent name or None, xf)

    def add(self, name, fid, kind, parent, xf):
        assert name not in self.frames
        self.frames[name] = {"id": fid, "kind": kind, "parent": parent, "xf": xf}

    def ancestors(self, name):
        out = [name]
        while self.frames[out[-1]]["parent"] is not None:
            out.append(self.frames[out[-1]]["parent"])
            assert len(out) <= len(self.frames)
        return out

    def split(self, a, b):
        la, lb = self.ancestors(a), self.ancestors(b)
        i, j = len(la), len(lb)
        while i > 0 and j > 0 and la[i - 1] == lb[j - 1]:
            i -= 1
            j -= 1
        return la[:i], lb[:j]

    def chain(self, names):
        if not names:
            return None
        acc = self.frames[names[0]]["xf"]
        for g in names[1:]:
            acc = compose(self.frames[g]["xf"], acc)
        return acc

    def resolve(self, a, b):
        if a == b:
            return IDENT
        ea, eb = self.split(a, b)
        ta, tb = self.chain(ea), self.chain(eb)
        if tb is None:
            return ta
        if ta is None:
            return inverse(tb)
        return compose(inverse(tb), ta)

    def digest(self):
        out = bytearray(b"FIDFRT")
        out += bytes([1])
        out += struct.pack("<Q", len(self.frames))
        world = [f for f in self.frames.values() if f["kind"] == "world"]
        assert len(world) == 1
        out += bytes.fromhex(world[0]["id"])
        for name, f in sorted(self.frames.items(), key=lambda kv: bytes.fromhex(kv[1]["id"])):
            out += bytes.fromhex(f["id"])
            out += bytes([KIND_BYTE[f["kind"]]])
            if f["parent"] is None:
                out += bytes([0]) + bytes(16)
            else:
                out += bytes([1]) + bytes.fromhex(self.frames[f["parent"]]["id"])
            nb = name.encode("utf-8")
            out += struct.pack("<H", len(nb)) + nb
            rigid, r, t, s = f["xf"]
            out += bytes([1 if rigid else 2])
            for v in [r[0][0], r[0][1], r[0][2], r[1][0], r[1][1], r[1][2], r[2][0], r[2][1], r[2][2]] + t + [s]:
                out += struct.pack("<d", 0.0 if v == 0.0 else v)
        return "sha256:" + hashlib.sha256(bytes(out)).hexdigest()


# ------------------------------------------------------------------ exact checks


def exact(xf):
    rigid, r, t, s = xf
    return [[Fraction(v) for v in row] for row in r], [Fraction(v) for v in t], Fraction(s)


def ex_compose(a, b):
    ra, ta, sa = a
    rb, tb, sb = b
    r = [[sum(ra[i][k] * rb[k][j] for k in range(3)) for j in range(3)] for i in range(3)]
    t = [sa * sum(ra[i][k] * tb[k] for k in range(3)) + ta[i] for i in range(3)]
    return r, t, sa * sb


def ex_inverse(a):
    r, t, s = a
    si = 1 / s
    rt = [[r[j][i] for j in range(3)] for i in range(3)]
    return rt, [-si * sum(rt[i][k] * t[k] for k in range(3)) for i in range(3)], si


def ex_resolve(tree, a, b):
    if a == b:
        return exact(IDENT)
    ea, eb = tree.split(a, b)

    def ch(names):
        if not names:
            return None
        acc = exact(tree.frames[names[0]]["xf"])
        for g in names[1:]:
            acc = ex_compose(exact(tree.frames[g]["xf"]), acc)
        return acc

    ta, tb = ch(ea), ch(eb)
    if tb is None:
        return ta
    if ta is None:
        return ex_inverse(tb)
    return ex_compose(ex_inverse(tb), ta)


def check_close(model, ex, what):
    _, r, t, s = model
    er, et, es = ex
    for i in range(3):
        for j in range(3):
            assert abs(Fraction(r[i][j]) - er[i][j]) <= Fraction(1, 10**14), (what, "r", i, j)
        assert abs(Fraction(t[i]) - et[i]) <= Fraction(1, 10**12), (what, "t", i)
    assert abs(Fraction(s) - es) <= Fraction(1, 10**14) * es, (what, "s")


# ------------------------------------------------------------------ inputs


def load_cases(path):
    data = json.loads(path.read_text(encoding="utf-8"))
    out = {}
    for c in data["cases"]:
        r = unhex(c["r"])
        rigid = c.get("kind", "rigid") == "rigid"
        s = unhex(c["s"])[0] if "s" in c else 1.0
        out[c["name"]] = (rigid, [r[0:3], r[3:6], r[6:9]], unhex(c["t"]), s)
    return out


TREE7 = [
    # name, id, kind, parent, xform_cases name
    ("world", "00000000000000000000000000000000", "world", None, None),
    ("part_a", "5f1e0c3a9b2d4e6f8a7b6c5d4e3f2a10", "part", "world", "random_0"),
    ("part_b", "1b2c3d4e5f60718293a4b5c6d7e8f901", "part", "world", "random_1"),
    ("align_1", "e0d1c2b3a4958677685940312a1b0c0d", "alignment", "part_a", "random_2"),
    ("align_2", "3c4d5e6f708192a3b4c5d6e7f8091a2b", "alignment", "align_1", "rz90_translate"),
    ("inst_1", "a1b2c3d4e5f6071829304a5b6c7d8e9f", "instrument", "align_2", "random_4"),
    ("inst_2", "7a6b5c4d3e2f10f9e8d7c6b5a4938271", "instrument", "part_b", "random_3"),
]

USER_1 = ("user_1", "c0ffee00000000000000000000000001", "user")

# op, frame, new parent (or ""), xform_cases name (or "")
EDITS = [
    ("set_transform", "inst_2", "", "random_5"),
    ("reparent", "inst_2", "align_1", "random_6"),
    ("insert", "user_1", "inst_2", "random_7"),
    ("set_transform", "user_1", "", "random_8"),
    ("remove", "user_1", "", ""),
]

WORST_TREE = [
    ("world", "00000000000000000000000000000000", "world", None, None),
    ("w1", "00000000000000000000000000000011", "part", "world", "stretch_x"),
    ("w2", "00000000000000000000000000000012", "alignment", "w1", "shear_xy"),
    ("w3", "00000000000000000000000000000013", "alignment", "w2", "rotated_stretch"),
    ("w4", "00000000000000000000000000000014", "instrument", "w3", "stretch_x_translated"),
    ("w5", "00000000000000000000000000000015", "part", "world", "rotated_stretch"),
    ("w6", "00000000000000000000000000000016", "instrument", "w5", "stretch_x_translated"),
]


def xf_fields(xf):
    rigid, r, t, s = xf
    return {
        "xkind": "rigid" if rigid else "similarity",
        "r": hxs([v for row in r for v in row]),
        "t": hxs(t),
        "s": hx(s),
    }


def build(spec, cases):
    tree = Tree()
    for name, fid, kind, parent, src in spec:
        tree.add(name, fid, kind, parent, IDENT if src is None else cases[src])
    return tree


def path_bound(tree, a, b, x):
    if a == b:
        return 0.0
    ea, eb = tree.split(a, b)
    rho = 0.0
    tsum = 0.0
    for name in ea + eb:
        rigid, r, t, _ = tree.frames[name]["xf"]
        assert rigid
        rho += residual(r)
        tsum += math.sqrt(dot3(t[0], t[1], t[2], t[0], t[1], t[2]))
    nx = math.sqrt(dot3(x[0], x[1], x[2], x[0], x[1], x[2]))
    m = nx + tsum
    k = len(ea) + len(eb)
    return rho * m + (48.0 * EPS * float(2 * k + 4)) * m


def main():
    check = "--check" in sys.argv[1:]
    cases = load_cases(SRC_XFORM)
    worst = load_cases(SRC_WORST)

    tree = build(TREE7, cases)
    names = [n for n, *_ in TREE7]
    lines = []
    for name, fid, kind, parent, src in TREE7:
        rec = {"rec": "frame", "name": name, "id": fid, "frame_kind": kind, "parent": parent or "", "source": src or ""}
        rec.update(xf_fields(tree.frames[name]["xf"]))
        lines.append(rec)
    digest0 = tree.digest()
    lines.append({"rec": "tree", "digest": digest0})

    for a in names:
        for b in names:
            m = tree.resolve(a, b)
            check_close(m, ex_resolve(tree, a, b), (a, b))
            rec = {"rec": "pair", "a": a, "b": b}
            rec.update(xf_fields(m))
            lines.append(rec)

    # AC-4 sanity: a -> b -> a within 1e-12 m.
    rng = random.Random(23)
    pairs = [(a, b) for a in names for b in names]
    worst_err = 0.0
    resolved = {p: tree.resolve(*p) for p in pairs}
    for k in range(20000):
        a, b = pairs[k % len(pairs)]
        x = [rng.uniform(-100.0, 100.0) for _ in range(3)]
        y = apply_point(resolved[(b, a)], apply_point(resolved[(a, b)], x))
        e = math.sqrt(sum((y[i] - x[i]) ** 2 for i in range(3)))
        worst_err = max(worst_err, e)
    assert worst_err <= 1e-12, worst_err

    # Edit sequence (AC-5): digest after each step.
    for op, frame, parent, src in EDITS:
        if op == "set_transform":
            tree.frames[frame]["xf"] = cases[src]
        elif op == "reparent":
            tree.frames[frame]["parent"] = parent
            tree.frames[frame]["xf"] = cases[src]
        elif op == "insert":
            tree.add(frame, USER_1[1], USER_1[2], parent, cases[src])
        elif op == "remove":
            assert all(f["parent"] != frame for f in tree.frames.values())
            del tree.frames[frame]
        rec = {"rec": "edit", "op": op, "frame": frame, "parent": parent, "source": src}
        if op == "insert":
            rec["id"] = USER_1[1]
            rec["frame_kind"] = USER_1[2]
        if src:
            rec.update(xf_fields(cases[src]))
        rec["digest"] = tree.digest()
        lines.append(rec)
    # Removing user_1 restores the digest after the reparent.
    assert lines[-1]["digest"] == lines[-4]["digest"]

    # Path bound on worst-residual edges, exact check.
    wt = build(WORST_TREE, worst)
    wnames = [n for n, *_ in WORST_TREE]
    rng = random.Random(29)
    checked = 0
    for a in wnames:
        for b in wnames:
            fwd, back = wt.resolve(a, b), wt.resolve(b, a)
            for _ in range(400):
                x = [rng.uniform(-100.0, 100.0) for _ in range(3)]
                y = apply_point(back, apply_point(fwd, x))
                err2 = sum((Fraction(y[i]) - Fraction(x[i])) ** 2 for i in range(3))
                bound = Fraction(path_bound(wt, a, b, x))
                assert err2 <= bound * bound, (a, b, x)
                checked += 1
    lines.append({"rec": "bound_checked", "pairs": len(wnames) ** 2, "points": checked})

    header = {
        "generator": "tools/fixtures/geo_tree_cases.py (stdlib, independent of fid-geo-frame)",
        "license": "CC-BY-4.0",
        "note": (
            "GEO.FRAME.TREE AC-1, AC-2, AC-5: tree_7 frames (transforms from xform_cases.json), "
            "resolve(a, b) for all 49 ordered pairs, the tree digest and the digest after each edit. "
            "Hex IEEE 754 bits, row-major, space-separated."
        ),
    }
    body = ",\n".join("    " + json.dumps(rec, sort_keys=True, separators=(",", ":")) for rec in lines)
    text = "{\n"
    for k, v in header.items():
        text += f"  {json.dumps(k)}: {json.dumps(v)},\n"
    text += '  "records": [\n' + body + "\n  ]\n}\n"
    json.loads(text)

    if check:
        old = OUT.read_text(encoding="utf-8") if OUT.exists() else ""
        if old != text:
            print(f"{OUT} differs; rerun without --check", file=sys.stderr)
            return 1
        print("tree_7.json up to date")
        return 0
    OUT.write_text(text, encoding="utf-8", newline="\n")
    print(f"wrote {OUT} (AC-4 sanity max error {worst_err:.3e} m, {checked} bound checks)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
