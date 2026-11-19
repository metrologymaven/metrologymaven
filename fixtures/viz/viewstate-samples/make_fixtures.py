#!/usr/bin/env python3
"""Fixtures for VIZ.VIEW.STATE (crate fid-view, module state). Stdlib only.

This is an independent reference: it never runs the Rust code. It holds its own RFC 8785
writer (ECMAScript Number::toString built from CPython's shortest repr), its own ViewState
key layout and collection order (both restated from spec/viz/view/state/NODE.md Design), and
SHA-256 from hashlib.

Outputs (all under this folder):
  samples/NN-*.json      20 hand-written ViewStates (AC-1, AC-5), canonical bytes, no newline
  digests.txt            "<file> sha256:<hex>" per sample (AC-5 golden digests)
  generated.txt          "sha256:<hex> <canonical json>" for 200 ViewStates from seed 3 with
                         random f64 bit patterns (normals, subnormals, +0 and -0) (AC-2 support)
  lenient/NAME.json      valid but non-canonical inputs (whitespace, order, -0, 1.0, 1E2,
                         upper-case UUIDs); lenient/NAME.canonical.json is their canonical form
  invalid/NAME.json      inputs that must be refused; invalid/expected.txt has the hand labels
                         "<file> <error code> <field>" taken from the rules in NODE.md Design
                         (AC-4 and the validation rules), not computed here.

Usage: python make_fixtures.py [--check]
"""

import hashlib
import json
import math
import struct
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent

# ------------------------------------------------------------------ RFC 8785 writer


def fmt_num(x):
    """RFC 8785 3.2.2.3: ECMAScript Number::toString of a finite double (ints pass through)."""
    if isinstance(x, bool):
        raise TypeError("bool is not a number")
    if isinstance(x, int):
        return str(x)
    if x != x or x in (math.inf, -math.inf):
        raise ValueError("non-finite number")
    if x == 0:
        return "0"  # covers -0.0
    sign = "-" if x < 0 else ""
    r = repr(abs(x))  # CPython: shortest round-trip digits
    if "e" in r:
        mant, e = r.split("e")
        e = int(e)
    else:
        mant, e = r, 0
    ip, _, fp = mant.partition(".")
    digits = ip + fp
    n = len(ip) + e
    stripped = digits.lstrip("0")
    n -= len(digits) - len(stripped)
    digits = stripped.rstrip("0")
    k = len(digits)
    if k <= n <= 21:
        s = digits + "0" * (n - k)
    elif 0 < n <= 21:
        s = digits[:n] + "." + digits[n:]
    elif -6 < n <= 0:
        s = "0." + "0" * (-n) + digits
    else:
        e2 = n - 1
        s = digits[0] + ("." + digits[1:] if k > 1 else "") + "e" + ("+" if e2 >= 0 else "-") + str(abs(e2))
    return sign + s


def fmt_str(s):
    out = ['"']
    for c in s:
        o = ord(c)
        if c == '"':
            out.append('\\"')
        elif c == "\\":
            out.append("\\\\")
        elif c == "\b":
            out.append("\\b")
        elif c == "\t":
            out.append("\\t")
        elif c == "\n":
            out.append("\\n")
        elif c == "\f":
            out.append("\\f")
        elif c == "\r":
            out.append("\\r")
        elif o < 0x20:
            out.append("\\u%04x" % o)
        else:
            out.append(c)
    out.append('"')
    return "".join(out)


def jcs(v):
    if v is None:
        return "null"
    if v is True:
        return "true"
    if v is False:
        return "false"
    if isinstance(v, (int, float)):
        return fmt_num(v)
    if isinstance(v, str):
        return fmt_str(v)
    if isinstance(v, list):
        return "[" + ",".join(jcs(x) for x in v) + "]"
    if isinstance(v, dict):
        keys = sorted(v, key=lambda k: k.encode("utf-16-be"))
        return "{" + ",".join(fmt_str(k) + ":" + jcs(v[k]) for k in keys) + "}"
    raise TypeError(type(v))


# ------------------------------------------------------------------ ViewState model (restated)


def canonical_model(vs):
    """Canonical collection order and spelling: clips by id, visibility and transparency by
    entity (lower-case UUID text sorts as the UUID bytes do); UUIDs lower case."""
    vs = json.loads(json.dumps(vs))
    for e in vs["visibility"] + vs["transparency"]:
        e["entity"] = e["entity"].lower()
    if vs["labels"] is not None:
        vs["labels"] = vs["labels"].lower()
    vs["clips"].sort(key=lambda c: c["id"])
    vs["visibility"].sort(key=lambda e: e["entity"])
    vs["transparency"].sort(key=lambda e: e["entity"])
    return vs


def canon_bytes(vs):
    return jcs(canonical_model(vs)).encode("utf-8")


def digest(b):
    return "sha256:" + hashlib.sha256(b).hexdigest()


def uuid(n):
    h = "%032x" % n
    return "%s-%s-%s-%s-%s" % (h[0:8], h[8:12], h[12:16], h[16:20], h[20:32])


def dg(tag):
    """A synthetic sha256: reference (the digest of a tag string)."""
    return digest(tag.encode("ascii"))


def base(**kw):
    vs = {
        "schema": 1,
        "camera": {
            "eye_m": [0.5, -0.25, 0.75],
            "target_m": [0.0, 0.0, 0.0],
            "up": [0.0, 0.0, 1.0],
            "projection": {"kind": "perspective", "fov_y_rad": 0.7853981633974483},
            "near_m": 0.01,
            "far_m": 100.0,
        },
        "viewport": {"width_px": 1920, "height_px": 1080},
        "clips": [],
        "visibility": [],
        "transparency": [],
        "colormap": None,
        "legend": None,
        "labels": None,
    }
    for k, v in kw.items():
        vs[k] = v
    return vs


def cam(**kw):
    c = base()["camera"]
    c.update(kw)
    return c


def unit(x, y, z):
    n = math.sqrt(x * x + y * y + z * z)
    return [x / n, y / n, z / n]


def samples():
    s = []
    s.append(("01-minimal", base()))
    s.append(("02-ortho", base(camera=cam(projection={"kind": "ortho", "height_m": 0.2}, near_m=-1.0, far_m=1.0))))
    s.append(
        (
            "03-all-references",
            base(colormap=dg("cmap-5bin"), legend=dg("legend-layout-tr"), labels=uuid(0x0190F3A0_0000_7000_8000_000000000001)),
        )
    )
    axes = [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]]
    s.append(
        (
            "04-six-axis-clips",
            base(clips=[{"id": 10 + i, "normal": [float(c) for c in a], "offset_m": 0.05 * (i + 1)} for i, a in enumerate(axes)]),
        )
    )
    s.append(
        (
            "05-oblique-clips",
            base(
                clips=[
                    {"id": 3, "normal": [0.6, 0.8, 0.0], "offset_m": 0.125},
                    {"id": 1, "normal": unit(1.0, 1.0, 1.0), "offset_m": -0.3},
                    {"id": 2, "normal": unit(0.0, -2.0, 5.0), "offset_m": 0.0},
                ]
            ),
        )
    )
    clips64 = []
    for i in range(64):
        # Only +, *, / and sqrt (all correctly rounded in IEEE 754), never libm cos/sin, whose
        # last bits differ between platforms and would make the fixture platform-dependent.
        clips64.append({"id": 63 - i, "normal": unit(i - 31.5, float((i * 7) % 13 - 6), 0.25), "offset_m": i * 0.001})
    s.append(("06-sixty-four-clips", base(clips=clips64)))
    s.append(
        (
            "07-visibility",
            base(visibility=[{"entity": uuid(0x1000 + 97 * i), "visible": i % 3 != 0} for i in range(10)]),
        )
    )
    alphas = [0.0, 1.0, 0.5, 0.25, 0.001, 0.1, 0.3333333333333333]
    s.append(
        (
            "08-transparency",
            base(transparency=[{"entity": uuid(0xAB00 - i), "alpha": a} for i, a in enumerate(alphas)]),
        )
    )
    s.append(
        (
            "09-subnormals",
            base(
                camera=cam(eye_m=[1.0, 5e-324, -2.2250738585072e-310], target_m=[0.0, 4.9406564584124654e-322, 0.0]),
                clips=[{"id": 0, "normal": [0.0, 0.0, 1.0], "offset_m": 1e-320}],
                transparency=[{"entity": uuid(7), "alpha": 5e-324}],
            ),
        )
    )
    s.append(
        (
            "10-number-forms",
            base(
                camera=cam(eye_m=[1e21, 1e20, 123456789012345680000.0], target_m=[0.000001, 1e-7, 1.5e-6], far_m=1e300),
                clips=[{"id": 4294967295, "normal": [1.0, 0.0, 0.0], "offset_m": 1.7976931348623157e308}],
            ),
        )
    )
    s.append(("11-negative", base(camera=cam(eye_m=[-1.25, -3.5, -0.0625], target_m=[-1.0, -3.0, -1.0], up=[0.0, -1.0, 0.0]))))
    s.append(("12-non-unit-up", base(camera=cam(up=[0.0, 0.003, 7.5]))))
    s.append(("13-fov-extremes", base(camera=cam(projection={"kind": "perspective", "fov_y_rad": 3.141592653589793 - 1e-9}))))
    s.append(
        (
            "14-tiny-ortho",
            base(camera=cam(projection={"kind": "ortho", "height_m": 2.2250738585072014e-308}, near_m=0.0, far_m=1e-300)),
        )
    )
    s.append(("15-viewport-max", base(viewport={"width_px": 32768, "height_px": 1})))
    s.append(
        (
            "16-mixed",
            base(
                camera=cam(eye_m=[0.2, 0.15, 0.4], target_m=[0.0125, 0.0, 0.03], up=[0.0, 1.0, 0.0], near_m=0.005, far_m=12.0),
                viewport={"width_px": 800, "height_px": 600},
                clips=[{"id": 5, "normal": [0.0, 1.0, 0.0], "offset_m": 0.02}, {"id": 2, "normal": [0.0, 0.0, -1.0], "offset_m": -0.01}],
                visibility=[{"entity": uuid(0x0190F3A0_0000_7000_8000_0000000000F0 + i), "visible": i % 2 == 0} for i in (3, 1, 2)],
                transparency=[{"entity": uuid(0x0190F3A0_0000_7000_8000_0000000000F1), "alpha": 0.4}],
                colormap=dg("cmap-continuous"),
                labels=uuid(0x0190F3A0_0000_7000_8000_00000000AAAA),
            ),
        )
    )
    s.append(
        (
            "17-clip-ids-unordered",
            base(clips=[{"id": i, "normal": [0.0, 0.0, 1.0], "offset_m": float(i)} for i in (9, 2, 7, 0, 4294967294)]),
        )
    )
    s.append(
        (
            "18-uuid-extremes",
            base(
                visibility=[{"entity": uuid(n), "visible": True} for n in ((1 << 128) - 1, 0, 1 << 127, (1 << 127) - 1)],
                transparency=[{"entity": uuid(n), "alpha": 0.75} for n in (0, (1 << 128) - 1)],
            ),
        )
    )
    s.append(
        (
            "19-thousand-entries",
            base(
                visibility=[{"entity": uuid((i * 2654435761) % (1 << 32)), "visible": i % 5 == 0} for i in range(1000)],
                transparency=[{"entity": uuid((i * 40503) % (1 << 16) + (5 << 120)), "alpha": (i % 101) / 100.0} for i in range(1000)],
            ),
        )
    )
    s.append(
        (
            "20-seventeen-digits",
            base(
                camera=cam(eye_m=[0.1 + 0.2, 1.0 / 3.0, 2.0 / 3.0], target_m=[9007199254740993.0, 100.0, 0.30000000000000004]),
                legend=dg("legend-17"),
            ),
        )
    )
    assert len(s) == 20
    return s


# ------------------------------------------------------------------ validity (generated cases only)


def scaled(v):
    m = 0.0
    for c in v:
        if abs(c) > m:
            m = abs(c)
    if m == 0.0:
        return None
    return [v[0] / m, v[1] / m, v[2] / m]


def dot(a, b):
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def camera_ok(c):
    u = scaled(c["up"])
    d = [c["target_m"][i] - c["eye_m"][i] for i in range(3)]
    if u is None or any(not math.isfinite(x) for x in d):
        return False
    d = scaled(d)
    if d is None:
        return False
    x = [u[1] * d[2] - u[2] * d[1], u[2] * d[0] - u[0] * d[2], u[0] * d[1] - u[1] * d[0]]
    return dot(x, x) > 1e-12 * 1e-12 * dot(u, u) * dot(d, d)


def normal_ok(n):
    return any(c != 0.0 for c in n) and abs(dot(n, n) - 1.0) <= 1e-12


# ------------------------------------------------------------------ seed-3 generated cases

MASK = (1 << 64) - 1


class SplitMix64:
    def __init__(self, seed):
        self.s = seed & MASK

    def next(self):
        self.s = (self.s + 0x9E3779B97F4A7C15) & MASK
        z = self.s
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK
        return z ^ (z >> 31)

    def below(self, n):
        return self.next() % n


def from_bits(b):
    return struct.unpack("<d", struct.pack("<Q", b))[0]


def rand_f64(r):
    """+0, -0, a subnormal, or a normal number with |exponent| <= 40 (so sums stay finite)."""
    c = r.below(8)
    sign = (r.next() & 1) << 63
    if c == 0:
        return 0.0
    if c == 1:
        return -0.0
    mant = r.next() & ((1 << 52) - 1)
    if c == 2:
        return from_bits(sign | (mant or 1))
    exp = 1023 - 40 + r.below(81)
    return from_bits(sign | (exp << 52) | mant)


def rand_nonzero_pos(r):
    while True:
        v = abs(rand_f64(r))
        if v > 0.0:
            return v


def rand_unit01(r):
    c = r.below(6)
    if c == 0:
        return 0.0
    if c == 1:
        return -0.0
    if c == 2:
        return 1.0
    if c == 3:
        return from_bits((r.next() & ((1 << 52) - 1)) or 1)
    return (r.next() >> 11) * 2.0**-53


def gen_one(r):
    while True:
        c = {"eye_m": [rand_f64(r) for _ in range(3)], "target_m": [rand_f64(r) for _ in range(3)], "up": [rand_f64(r) for _ in range(3)]}
        if r.below(2) == 0:
            c["projection"] = {"kind": "perspective", "fov_y_rad": 1e-3 + (r.next() >> 11) * 2.0**-53 * 3.0}
            c["near_m"] = rand_nonzero_pos(r)
            c["far_m"] = c["near_m"] * 2.0
        else:
            c["projection"] = {"kind": "ortho", "height_m": rand_nonzero_pos(r)}
            c["near_m"] = rand_f64(r)
            c["far_m"] = abs(c["near_m"]) * 2.0 + 1.0
        if camera_ok(c):
            break
    clips, ids = [], set()
    for _ in range(r.below(6)):
        while True:
            v = [rand_f64(r) for _ in range(3)]
            m = scaled(v)
            if m is None:
                continue
            n = unit(*m)
            if normal_ok(n):
                break
        i = r.next() & 0xFFFFFFFF
        if i in ids:
            continue
        ids.add(i)
        clips.append({"id": i, "normal": n, "offset_m": rand_f64(r)})
    vis = [{"entity": uuid((r.next() << 64) | r.next()), "visible": r.below(2) == 0} for _ in range(r.below(6))]
    tr = [{"entity": uuid((r.next() << 64) | r.next()), "alpha": rand_unit01(r)} for _ in range(r.below(6))]
    pick = r.below(8)
    return base(
        camera=c,
        viewport={"width_px": 1 + r.below(32768), "height_px": 1 + r.below(32768)},
        clips=clips,
        visibility=vis,
        transparency=tr,
        colormap=dg("cmap-%d" % r.next()) if pick & 1 else None,
        legend=dg("legend-%d" % r.next()) if pick & 2 else None,
        labels=uuid((r.next() << 64) | r.next()) if pick & 4 else None,
    )


def generated():
    r = SplitMix64(3)
    lines = []
    for _ in range(200):
        b = canon_bytes(gen_one(r))
        lines.append(digest(b) + " " + b.decode("ascii"))
    return ("\n".join(lines) + "\n").encode("ascii")


# ------------------------------------------------------------------ lenient inputs


def lenient():
    """Valid inputs in non-canonical spelling; the expected output is the canonical form."""
    out = {}
    v = base(
        clips=[{"id": 2, "normal": [0.0, 0.0, 1.0], "offset_m": 0.5}, {"id": 1, "normal": [1.0, 0.0, 0.0], "offset_m": -0.0}],
        visibility=[{"entity": uuid(0xB), "visible": False}, {"entity": uuid(0xA), "visible": True}],
    )
    out["pretty"] = json.dumps(v, indent=2, sort_keys=False)
    out["crlf"] = json.dumps(v, indent=1, sort_keys=False).replace("\n", "\r\n")
    text = jcs(base(camera=cam(eye_m=[1.0, 2.0, 3.0]), viewport={"width_px": 640, "height_px": 480}))
    for a, b in (('"eye_m":[1,2,3]', '"eye_m":[1.0,2E0,0.3e1]'), ('"width_px":640', '"width_px":6.4e2'), ('"schema":1', '"schema":1.0')):
        assert a in text, a
        text = text.replace(a, b)
    out["number-spellings"] = text
    text = jcs(base(camera=cam(eye_m=[-0.0, 0.5, 0.25]), transparency=[{"entity": uuid(0xC), "alpha": 0.5}]))
    assert '"eye_m":[0,' in text
    out["negative-zero"] = text.replace('"eye_m":[0,', '"eye_m":[-0,').replace('"alpha":0.5', '"alpha":5e-1')
    text = jcs(base(labels=uuid(0xDEADBEEF), visibility=[{"entity": uuid(0xABCDEF), "visible": True}]))
    out["upper-case-uuid"] = text.replace(uuid(0xDEADBEEF), uuid(0xDEADBEEF).upper()).replace(uuid(0xABCDEF), uuid(0xABCDEF).upper())
    return out


def strict_load(text):
    def hook(pairs):
        keys = [k for k, _ in pairs]
        assert len(keys) == len(set(keys)), keys
        return dict(pairs)

    return json.loads(text, object_pairs_hook=hook)


# ------------------------------------------------------------------ invalid inputs (hand-labelled)


def mutate(vs, old, new):
    text = jcs(canonical_model(vs))
    assert text.count(old) == 1, (old, text)
    return text.replace(old, new)


def invalid():
    """(name, text, code, field). The code and field are hand labels from NODE.md Design."""
    c1 = [{"id": 1, "normal": [0.0, 0.0, 1.0], "offset_m": 0.5}]
    cases = [
        ("nan-eye", mutate(base(), '"eye_m":[0.5,-0.25,0.75]', '"eye_m":[0.5,NaN,0.75]'), "non_finite_number", "camera.eye_m[1]"),
        ("infinity-up", mutate(base(), '"up":[0,0,1]', '"up":[0,0,Infinity]'), "non_finite_number", "camera.up[2]"),
        ("overflow-offset", mutate(base(clips=c1), '"offset_m":0.5', '"offset_m":1e999'), "non_finite_number", "clips[0].offset_m"),
        ("neg-infinity-near", mutate(base(), '"near_m":0.01', '"near_m":-Infinity'), "non_finite_number", "camera.near_m"),
        (
            "clips-65",
            jcs(base(clips=[{"id": i, "normal": [0.0, 0.0, 1.0], "offset_m": 0.0} for i in range(65)])),
            "limit_exceeded",
            "clips",
        ),
        ("depth-5", mutate(base(clips=c1), '"normal":[0,0,1]', '"normal":[[0],0,1]'), "limit_exceeded", "clips[0].normal[0]"),
        ("schema-2", mutate(base(), '"schema":1', '"schema":2'), "unsupported_schema", "schema"),
        ("schema-2-other-fields", '{"schema":2,"whatever":true}', "unsupported_schema", "schema"),
        ("zero-up", mutate(base(), '"up":[0,0,1]', '"up":[0,-0,0]'), "degenerate_camera", "camera.up"),
        ("up-parallel", mutate(base(), '"up":[0,0,1]', '"up":[-1,0.5,-1.5]'), "degenerate_camera", "camera.up"),
        ("eye-is-target", mutate(base(), '"target_m":[0,0,0]', '"target_m":[0.5,-0.25,0.75]'), "degenerate_camera", "camera.target_m"),
        ("near-equals-far", mutate(base(), '"far_m":100', '"far_m":0.01'), "degenerate_camera", "camera.far_m"),
        ("near-above-far", mutate(base(), '"far_m":100', '"far_m":0.001'), "degenerate_camera", "camera.far_m"),
        ("fov-zero", mutate(base(), '"fov_y_rad":0.7853981633974483', '"fov_y_rad":0'), "degenerate_camera", "camera.projection.fov_y_rad"),
        ("fov-pi", mutate(base(), '"fov_y_rad":0.7853981633974483', '"fov_y_rad":3.141592653589793'), "degenerate_camera", "camera.projection.fov_y_rad"),
        ("perspective-near-zero", mutate(base(), '"near_m":0.01', '"near_m":0'), "degenerate_camera", "camera.near_m"),
        (
            "ortho-height-negative",
            mutate(base(camera=cam(projection={"kind": "ortho", "height_m": 0.2})), '"height_m":0.2', '"height_m":-0.2'),
            "degenerate_camera",
            "camera.projection.height_m",
        ),
        ("normal-zero", mutate(base(clips=c1), '"normal":[0,0,1]', '"normal":[0,0,0]'), "degenerate_normal", "clips[0].normal"),
        ("normal-not-unit", mutate(base(clips=c1), '"normal":[0,0,1]', '"normal":[1,1,0]'), "non_unit_normal", "clips[0].normal"),
        ("normal-near-unit", mutate(base(clips=c1), '"normal":[0,0,1]', '"normal":[0,0,1.000000001]'), "non_unit_normal", "clips[0].normal"),
        (
            "duplicate-clip-id",
            jcs(base(clips=[{"id": 4, "normal": [0.0, 0.0, 1.0], "offset_m": 0.0}, {"id": 4, "normal": [1.0, 0.0, 0.0], "offset_m": 0.0}])),
            "duplicate_key",
            "clips[1].id",
        ),
        (
            "duplicate-visibility",
            jcs(base(visibility=[{"entity": uuid(5), "visible": True}, {"entity": uuid(5), "visible": False}])),
            "duplicate_key",
            "visibility[1].entity",
        ),
        (
            "duplicate-transparency",
            jcs(base(transparency=[{"entity": uuid(9), "alpha": 0.5}, {"entity": uuid(8), "alpha": 0.5}, {"entity": uuid(9), "alpha": 0.25}])),
            "duplicate_key",
            "transparency[2].entity",
        ),
        ("alpha-above-one", jcs(base(transparency=[{"entity": uuid(1), "alpha": 1.5}])), "out_of_range", "transparency[0].alpha"),
        ("alpha-negative", jcs(base(transparency=[{"entity": uuid(1), "alpha": -1e-300}])), "out_of_range", "transparency[0].alpha"),
        ("viewport-zero", mutate(base(), '"width_px":1920', '"width_px":0'), "out_of_range", "viewport.width_px"),
        ("viewport-too-wide", mutate(base(), '"height_px":1080', '"height_px":32769'), "out_of_range", "viewport.height_px"),
        ("missing-legend", mutate(base(), '"legend":null,', ""), "malformed", "legend"),
        ("unknown-field", mutate(base(), '"schema":1,', '"schema":1,"shading":"flat",'), "malformed", "shading"),
        ("unknown-camera-field", mutate(base(), '"far_m":100,', '"far_m":100,"roll":0,'), "malformed", "camera.roll"),
        ("bad-uuid", jcs(base(visibility=[{"entity": "not-a-uuid", "visible": True}])), "malformed", "visibility[0].entity"),
        ("bad-digest", mutate(base(colormap=dg("x")), dg("x"), dg("x").upper()), "malformed", "colormap"),
        ("visible-not-bool", jcs(base(visibility=[{"entity": uuid(2), "visible": 1}])), "malformed", "visibility[0].visible"),
        ("eye-two-components", mutate(base(), '"eye_m":[0.5,-0.25,0.75]', '"eye_m":[0.5,-0.25]'), "malformed", "camera.eye_m"),
        ("fractional-clip-id", mutate(base(clips=c1), '"id":1', '"id":1.5'), "malformed", "clips[0].id"),
        ("negative-clip-id", mutate(base(clips=c1), '"id":1', '"id":-1'), "malformed", "clips[0].id"),
        ("clip-id-too-big", mutate(base(clips=c1), '"id":1', '"id":4294967296'), "malformed", "clips[0].id"),
        ("schema-string", mutate(base(), '"schema":1', '"schema":"1"'), "malformed", "schema"),
        ("projection-kind", mutate(base(), '"kind":"perspective"', '"kind":"fisheye"'), "malformed", "camera.projection.kind"),
        ("trailing-comma", jcs(base())[:-1] + ",}", "malformed", "$"),
        ("duplicate-json-key", jcs(base())[:-1] + ',"schema":1}', "malformed", "$"),
        ("root-array", "[" + jcs(base()) + "]", "malformed", "$"),
        ("empty", "", "malformed", "$"),
    ]
    return cases


# ------------------------------------------------------------------ write / check


def outputs():
    files = {}
    dig = []
    for name, vs in samples():
        assert camera_ok(vs["camera"]) and all(normal_ok(c["normal"]) for c in vs["clips"]), name
        b = canon_bytes(vs)
        # Every sample is a fixed point of the reference parser + writer.
        assert canon_bytes(strict_load(b.decode("ascii"))) == b, name
        files["samples/%s.json" % name] = b
        dig.append("%s.json %s" % (name, digest(b)))
    files["digests.txt"] = ("\n".join(dig) + "\n").encode("ascii")
    files["generated.txt"] = generated()
    for name, text in lenient().items():
        b = text.encode("ascii")
        canon = canon_bytes(strict_load(text))
        assert canon != b, name
        files["lenient/%s.json" % name] = b
        files["lenient/%s.canonical.json" % name] = canon
    labels = []
    for name, text, code, field in invalid():
        files["invalid/%s.json" % name] = text.encode("ascii")
        labels.append("%s.json %s %s" % (name, code, field))
    files["invalid/expected.txt"] = ("\n".join(labels) + "\n").encode("ascii")
    return files


def main():
    files = outputs()
    check = "--check" in sys.argv[1:]
    bad = []
    for rel, data in sorted(files.items()):
        p = HERE / rel
        if check:
            if not p.exists() or p.read_bytes() != data:
                bad.append(rel)
        else:
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_bytes(data)
    if check:
        known = {(HERE / rel).resolve() for rel in files}
        for sub in ("samples", "lenient", "invalid"):
            for p in sorted((HERE / sub).glob("*")):
                if p.resolve() not in known:
                    bad.append("unexpected " + str(p.relative_to(HERE)))
        if bad:
            print("viewstate fixtures are stale; run make_fixtures.py:\n  " + "\n  ".join(bad), file=sys.stderr)
            return 1
        print("ok: %d viewstate fixture files" % len(files))
        return 0
    print("wrote %d viewstate fixture files" % len(files))
    return 0


if __name__ == "__main__":
    sys.exit(main())
