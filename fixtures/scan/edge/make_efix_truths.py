#!/usr/bin/env python3
"""Independent reference for SCAN.EDGE.EST.EFIX (synthetic edge fixtures). CC0-1.0 project data.

Stdlib only. Writes `efix_truths.txt` next to this script: the case table of the eight named
fixtures `est_{sharp,round1,chamfer05,burr02}_{onesided,wrapped}` (seeds 61 to 68) plus extra
cases, the grid sizes the sampling rule gives, and the analytic truth of every station. The
Rust test `crates/fid-scan-edge/tests/efix.rs` reads this file; nothing here comes from the
crate.

Geometry (pinned in the crate's `testkit` module documentation and in the leaf's NODE.md):

* A station has origin p on the nominal edge, unit tangent t along the edge, n the nominal
  top-face normal and b the outward direction in the top-face plane, with b x t = n.
  Station-frame coordinates are (u, v): u along b, v along n.
* The sheet occupies v in [-T, 0]; the actual edge face (the trim line) is at u = e, the trim
  offset. The profile runs from the top face, inward start u = e - top_extent, outward across
  the edge and back along the bottom face to u = e - bottom_extent (wrapped coverage).
* sharp:   top (0 deg), edge face (90), bottom (180).
* round r: top to (e - r, 0), a quarter arc with centre (e - r, -r) to (e, -r), normal angle
  rising 0 to 90 deg, the edge face from (e, -r) to (e, -T) if r < T, bottom.
* chamfer c (legs c at 45 deg): top to (e - c, 0), chamfer to (e, -c) at 45 deg, edge face to
  (e, -T) if c < T, bottom.
* burr (h, w): top to (e, 0), edge face to (e, -(T - h)), ledge out to (e + w, -(T - h))
  facing +n, burr face down to (e + w, -T), bottom from (e + w, -T) inward.
* One-sided coverage is the profile up to (excluding) the first vertical or downward-facing
  segment; wrapped coverage is all of it.
* Tangency truth for threshold theta: the first profile point (in profile order) at which the
  normal's angle to n exceeds theta: on the round arc the point with angle theta, otherwise
  the start of the first segment whose angle exceeds theta. `none` if there is none.
* Silhouette truth: the point of largest u, the first one in profile order if several.
* Trim truth: e.

The truths are closed-form expressions evaluated here in 60-digit decimal arithmetic on the
exact binary values of the inputs (pi by Machin's formula, sine and cosine by their Taylor
series) and rounded once to the nearest double, so the file does not depend on the platform
libm. The station positions are s_k = L (k + 1/2) / count along the path; on an arc path of
radius R the station is at angle s_k / R about the arc centre (the origin), starting on +x.

The grid sizes follow the sampling rule in IEEE binary64 arithmetic with the crate's operation
order (each line segment's length is sqrt(du*du + dv*dv) of its float vertex differences, the
arc's is r * (pi/2) with pi/2 the double nearest; each segment gets max(1, ceil(len / spacing))
intervals; rows = ceil(L / spacing) + 1). Python floats are IEEE binary64 and sqrt is correctly
rounded, so the counts are platform independent too.

File format (LF, one record per line, floats as 16 lowercase hex digits of the IEEE-754 bits):

  case <name> profile=<kind>[:<p1>[:<p2>]] thickness=<f> trim=<f> coverage=<onesided|wrapped>
       path=<straight:<L>|arc:<R>:<L>> spacing=<f> sigma=<f> top=<f> bottom=<f> stations=<n>
       theta=<f> seed=<n> noisy_sigma=<f>
  grid <name> rows=<n> cols=<n> points=<n> triangles=<n>
  station <name> <k> p=<x>,<y>,<z> t=<x>,<y>,<z> n=<x>,<y>,<z> b=<x>,<y>,<z> trim=<f>
       tang=<u>,<v>|none silh=<u>,<v>

(each record on one line). Run:  python fixtures/scan/edge/make_efix_truths.py [--check]
With --check nothing is written; the script exits non-zero if the file differs.
"""

import decimal
import math
import os
import struct
import sys
from decimal import Decimal

decimal.getcontext().prec = 60
D = Decimal

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "efix_truths.txt")


def atan_inv(x):
    """atan(1/x) for an integer x > 1 by its Taylor series."""
    x = D(x)
    x2 = x * x
    term = D(1) / x
    total = term
    k = 1
    eps = D(10) ** -58
    while abs(term) > eps:
        term = -term / x2
        total += term / (2 * k + 1)
        k += 1
    return total


PI = 16 * atan_inv(5) - 4 * atan_inv(239)


def sin_d(x):
    x = D(x)
    term = x
    total = x
    k = 1
    eps = D(10) ** -58
    while abs(term) > eps:
        term = -term * x * x / ((2 * k) * (2 * k + 1))
        total += term
        k += 1
    return total


def cos_d(x):
    x = D(x)
    term = D(1)
    total = D(1)
    k = 1
    eps = D(10) ** -58
    while abs(term) > eps:
        term = -term * x * x / ((2 * k - 1) * (2 * k))
        total += term
        k += 1
    return total


def dbl(x):
    """The double nearest the decimal x (CPython's str to float conversion rounds correctly)."""
    return float(x)


def hx(x):
    f = dbl(x)
    if f == 0.0:
        f = 0.0  # one zero: the truths never need a signed zero
    return struct.pack(">d", f).hex()


def ex(f):
    """The exact decimal value of a double."""
    return D(f)


MM = 1e-3
UM = 1e-6

# Named fixture set (SCAN.EDGE.EST.EFIX Test fixtures): one sheet geometry, four profiles,
# two coverages. Lengths are written as Python float literals; the Rust table uses the same
# literals.
COMMON = dict(thickness=1.0e-3, trim=0.5e-3, path=("straight", 2.0e-3), spacing=50.0e-6,
              sigma=0.0, top=2.0e-3, bottom=1.0e-3, stations=4, theta=0.5235987755982989,
              noisy_sigma=10.0e-6)
PROFILES = [("sharp", ("sharp",)), ("round1", ("round", 1.0e-3)), ("chamfer05", ("chamfer", 0.5e-3)),
            ("burr02", ("burr", 0.2e-3, 0.2e-3))]


def named_cases():
    out = []
    seed = 61
    for pname, prof in PROFILES:
        for cov in ("onesided", "wrapped"):
            c = dict(COMMON)
            c.update(name="est_%s_%s" % (pname, cov), profile=prof, coverage=cov, seed=seed)
            out.append(c)
            seed += 1
    return out


def extra_cases():
    """Unit cases for the other branches of the truth rules and for an arc path."""
    base = dict(sigma=0.0, noisy_sigma=10.0e-6, seed=7)
    return [
        dict(base, name="x_arc_round05", profile=("round", 0.5e-3), thickness=1.0e-3, trim=0.3e-3,
             path=("arc", 20.0e-3, 3.0e-3), spacing=50.0e-6, top=1.5e-3, bottom=0.8e-3, stations=3,
             theta=1.0471975511965979, coverage="wrapped"),
        dict(base, name="x_chamfer_t50", profile=("chamfer", 0.4e-3), thickness=1.0e-3, trim=-0.2e-3,
             path=("straight", 1.0e-3), spacing=40.0e-6, top=1.0e-3, bottom=0.5e-3, stations=2,
             theta=0.8726646259971648, coverage="onesided"),
        dict(base, name="x_chamfer_full", profile=("chamfer", 1.0e-3), thickness=1.0e-3, trim=0.0,
             path=("straight", 1.0e-3), spacing=40.0e-6, top=1.5e-3, bottom=0.5e-3, stations=2,
             theta=1.0471975511965979, coverage="wrapped"),
        dict(base, name="x_sharp_t100", profile=("sharp",), thickness=2.0e-3, trim=0.25e-3,
             path=("straight", 1.0e-3), spacing=100.0e-6, top=1.0e-3, bottom=1.0e-3, stations=2,
             theta=1.7453292519943295, coverage="wrapped"),
        dict(base, name="x_burr_t95", profile=("burr", 0.3e-3, 0.15e-3), thickness=1.0e-3, trim=0.1e-3,
             path=("straight", 1.0e-3), spacing=50.0e-6, top=1.0e-3, bottom=0.5e-3, stations=2,
             theta=1.6580627893946132, coverage="wrapped"),
        dict(base, name="x_round_t90", profile=("round", 0.4e-3), thickness=1.0e-3, trim=0.5e-3,
             path=("arc", 5.0e-3, 2.0e-3), spacing=50.0e-6, top=1.0e-3, bottom=0.5e-3, stations=2,
             theta=1.5707963267948966, coverage="onesided"),
    ]


def check_theta_literals():
    # The theta literals are the doubles nearest pi/6, pi/3, 5 pi/18, pi/2, 5 pi/9 and 19 pi/36.
    for lit, num, den in [(0.5235987755982989, 1, 6), (1.0471975511965979, 1, 3), (0.8726646259971648, 5, 18),
                          (1.5707963267948966, 1, 2), (1.7453292519943295, 5, 9), (1.6580627893946132, 19, 36)]:
        if dbl(PI * num / den) != lit:
            raise SystemExit("theta literal %r is not the double nearest %d pi / %d" % (lit, num, den))


# ---- sampling sizes (IEEE binary64, the crate's operation order) ----

FRAC_PI_2 = dbl(PI / 2)


def line_len(a, b):
    du = b[0] - a[0]
    dv = b[1] - a[1]
    return math.sqrt(du * du + dv * dv)


def segments_f(c):
    """The profile as (kind, start, end, normal angle class) with float vertices."""
    T = c["thickness"]
    e = c["trim"]
    top = c["top"]
    bot = c["bottom"]
    prof = c["profile"]
    segs = []
    if prof[0] == "sharp":
        segs.append(("line", (e - top, 0.0), (e, 0.0), "up"))
        segs.append(("line", (e, 0.0), (e, -T), "vertical"))
        segs.append(("line", (e, -T), (e - bot, -T), "down"))
    elif prof[0] == "round":
        r = prof[1]
        segs.append(("line", (e - top, 0.0), (e - r, 0.0), "up"))
        segs.append(("arc", (e - r, 0.0), (e, -r), "up", r))
        if r < T:
            segs.append(("line", (e, -r), (e, -T), "vertical"))
        segs.append(("line", (e, -T), (e - bot, -T), "down"))
    elif prof[0] == "chamfer":
        cc = prof[1]
        segs.append(("line", (e - top, 0.0), (e - cc, 0.0), "up"))
        segs.append(("line", (e - cc, 0.0), (e, -cc), "up"))
        if cc < T:
            segs.append(("line", (e, -cc), (e, -T), "vertical"))
        segs.append(("line", (e, -T), (e - bot, -T), "down"))
    elif prof[0] == "burr":
        h, w = prof[1], prof[2]
        segs.append(("line", (e - top, 0.0), (e, 0.0), "up"))
        segs.append(("line", (e, 0.0), (e, -(T - h)), "vertical"))
        segs.append(("line", (e, -(T - h)), (e + w, -(T - h)), "up"))
        segs.append(("line", (e + w, -(T - h)), (e + w, -T), "vertical"))
        segs.append(("line", (e + w, -T), (e - bot, -T), "down"))
    else:
        raise SystemExit("unknown profile")
    return segs


def grid(c):
    segs = segments_f(c)
    if c["coverage"] == "onesided":
        kept = []
        for s in segs:
            if s[3] != "up":
                break
            kept.append(s)
        segs = kept
    sp = c["spacing"]
    cols = 1
    for s in segs:
        if s[0] == "arc":
            length = s[4] * (FRAC_PI_2 - 0.0)
        else:
            length = line_len(s[1], s[2])
        cols += max(1, math.ceil(length / sp))
    length = c["path"][-1]
    rows = math.ceil(length / sp) + 1
    return rows, cols


# ---- analytic truths (60-digit decimal on the exact input values) ----


def truths_uv(c):
    T = ex(c["thickness"])
    e = ex(c["trim"])
    th = ex(c["theta"])
    half_pi = PI / 2
    prof = c["profile"]
    kind = prof[0]
    if kind == "sharp":
        silh = (e, D(0))
        if th < half_pi:
            tang = (e, D(0))
        elif th < PI:
            tang = (e, -T)
        else:
            tang = None
    elif kind == "round":
        r = ex(prof[1])
        silh = (e, -r)
        if th < half_pi:
            tang = (e - r + r * sin_d(th), -r + r * cos_d(th))
        elif th < PI:
            tang = (e, -T)
        else:
            tang = None
    elif kind == "chamfer":
        cc = ex(prof[1])
        silh = (e, -cc)
        if th < PI / 4:
            tang = (e - cc, D(0))
        elif th < half_pi:
            tang = (e, -cc)
        elif th < PI:
            tang = (e, -T)
        else:
            tang = None
    else:
        h, w = ex(prof[1]), ex(prof[2])
        silh = (e + w, -(T - h))
        if th < half_pi:
            tang = (e, D(0))
        elif th < PI:
            tang = (e + w, -T)
        else:
            tang = None
    return tang, silh


def stations(c):
    path = c["path"]
    L = ex(path[-1])
    count = c["stations"]
    out = []
    for k in range(count):
        s = L * (2 * k + 1) / (2 * count)
        if path[0] == "straight":
            p = (D(0), s, D(0))
            t = (D(0), D(1), D(0))
            b = (D(1), D(0), D(0))
        else:
            R = ex(path[1])
            a = s / R
            ca, sa = cos_d(a), sin_d(a)
            p = (R * ca, R * sa, D(0))
            t = (-sa, ca, D(0))
            b = (ca, sa, D(0))
        n = (D(0), D(0), D(1))
        out.append((p, t, n, b))
    return out


def fmt_vec(v):
    return ",".join(hx(x) for x in v)


def fmt_profile(prof):
    if prof[0] == "sharp":
        return "sharp"
    return prof[0] + "".join(":" + hx(x) for x in prof[1:])


def fmt_path(path):
    return path[0] + "".join(":" + hx(x) for x in path[1:])


def check_ac1(c, tang, silh):
    """AC-1, hand-labelled: round r = 1 mm, T = 1 mm, trim 0.5 mm, theta = 30 deg."""
    if c["name"] != "est_round1_wrapped":
        return
    half_mm = D("0.0005")
    if abs(silh[0] - half_mm) > D("1e-18"):
        raise SystemExit("AC-1: silhouette is not the nominal edge plus 0.5 mm along b")
    r = ex(1.0e-3)
    start_of_round = ex(0.5e-3) - r
    if abs((tang[0] - start_of_round) - half_mm) > D("1e-18"):
        raise SystemExit("AC-1: tangency truth is not r sin(30 deg) = 0.5 mm past the start of the round")


def render():
    check_theta_literals()
    lines = ["# SCAN.EDGE.EST.EFIX truths, written by make_efix_truths.py (CC0-1.0). Do not edit.",
             "efix-truths 1"]
    for c in named_cases() + extra_cases():
        name = c["name"]
        lines.append(
            "case %s profile=%s thickness=%s trim=%s coverage=%s path=%s spacing=%s sigma=%s top=%s bottom=%s "
            "stations=%d theta=%s seed=%d noisy_sigma=%s"
            % (name, fmt_profile(c["profile"]), hx(c["thickness"]), hx(c["trim"]), c["coverage"],
               fmt_path(c["path"]), hx(c["spacing"]), hx(c["sigma"]), hx(c["top"]), hx(c["bottom"]),
               c["stations"], hx(c["theta"]), c["seed"], hx(c["noisy_sigma"])))
        rows, cols = grid(c)
        lines.append("grid %s rows=%d cols=%d points=%d triangles=%d"
                     % (name, rows, cols, rows * cols, 2 * (rows - 1) * (cols - 1)))
        tang, silh = truths_uv(c)
        check_ac1(c, tang, silh)
        for k, (p, t, n, b) in enumerate(stations(c)):
            tang_s = "none" if tang is None else fmt_vec(tang)
            lines.append("station %s %d p=%s t=%s n=%s b=%s trim=%s tang=%s silh=%s"
                         % (name, k, fmt_vec(p), fmt_vec(t), fmt_vec(n), fmt_vec(b), hx(ex(c["trim"])),
                            tang_s, fmt_vec(silh)))
    return "\n".join(lines) + "\n"


def main():
    text = render()
    if "--check" in sys.argv[1:]:
        with open(OUT, "rb") as f:
            have = f.read()
        if have != text.encode("ascii"):
            print("efix_truths.txt differs from make_efix_truths.py output", file=sys.stderr)
            sys.exit(1)
        print("efix_truths.txt up to date")
        return
    with open(OUT, "wb") as f:
        f.write(text.encode("ascii"))


if __name__ == "__main__":
    main()
