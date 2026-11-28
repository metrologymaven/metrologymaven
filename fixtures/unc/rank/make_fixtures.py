#!/usr/bin/env python3
"""Independent reference for UNC.PROP.RANK (stdlib only; CC-BY-4.0 project data).

Nothing here runs or imports the Rust code. Every expected value comes from the rules in
spec/unc/prop/rank/NODE.md and the pinned operation order in the module documentation of
`fid_kunc::prop::rank`, restated below and implemented here on its own:

- Fixed-order reductions (KRN.NUM.RED): blocks of 256 terms, each summed in index order with
  Neumaier compensation from (-0.0, -0.0); block pairs combined by the fixed binary tree
  (left half floor(k / 2) blocks); result s if c == 0 else s + c. `dot` sums the plain
  products a[i] * b[i]; `sum_sq` the products x[i] * x[i]; `mean` is the sum divided by the
  length. Python floats are IEEE-754 binary64 with round-to-nearest-even and no FMA, so the
  emulation gives the exact bits of a conforming implementation.
- Square roots are correctly rounded. This script never trusts a library square root: it
  takes a candidate and corrects it with exact rational comparisons against the rounding
  midpoints (`crsqrt`), so no printed value depends on libm.
- LPU (REQ-UNC.PROP.RANK-001, -003, -004): u_i(y) = |c_i| * sqrt(S_ii); w_i = dot(S_i, c);
  v = dot(c, w); a = dot(|c|, b) with b_i = dot(|S_i|, |c|); tol = (4 (n + 1) 2^-53) a.
  v < -tol is refused (`negative_variance`); v <= tol is degenerate (shares absent;
  `zero_variance` when v == 0, else `variance_at_rounding_resolution`); otherwise
  share_i = (c_i * w_i) / v, share_sum = sum(shares), and |share_sum - 1| > 1e-12 drops the
  shares (`sum_out_of_tolerance`, share_sum still reported).
- Samples (REQ-UNC.PROP.RANK-002, -004): m_y = mean(y), d_y = y - m_y, s_yy = sum_sq(d_y);
  per input the same, s_xy = dot(d_x, d_y), r = (s_xy / sqrt(s_xx)) / sqrt(s_yy), r2 = r * r,
  a rounded r2 above 1 is reported as 1. A bitwise-constant column or a zero sum of squares
  is `zero_input_variance` (input) or degenerate `zero_output_variance` (output).
- Order (REQ-003): descending key (u_i(y) or r2; an absent r2 after every present one), ties
  by input ID bytes ascending; an entry whose key equals the previous entry's key carries
  tie_broken_by_id = true.

The 50-digit references are exact rational evaluations (fractions.Fraction) of the
binary64 inputs, written with decimal.Decimal.

Files written next to this script:

- lpu_3in.json      AC-1: c = (1, 2, -1), independent u = (0.1, 0.05, 0.2) as S = diag(u^2)
- lpu_corr.json     AC-2: two inputs with rho = -0.9 (one share is negative)
- rules.json        AC-4 (all sensitivities zero) and the other degenerate and tie rules
- mcm_samples.json  AC-3: Y = X1 + 3 X2, X1, X2 rectangular on [-1, 1] (stream 0 and 1),
                    seed 0x52414E4B, 10^5 trials, drawn as UNC.MODEL.DIST does (Philox4x32-10
                    block (seed, stream, i), open-interval uniform, a + (b - a) u); the SHA-256
                    of the three columns pins the sample set instead of a committed binary
- expected_canonical.txt  one line per case: `<case> <canonical JSON of the ranking>`

Every f64 in the JSON files is its IEEE-754 bit pattern in 16 lowercase hex digits.

Usage: make_fixtures.py [--check]   (--check compares with the files on disk, writes nothing)
"""

import hashlib
import json
import math
import pathlib
import struct
import sys
from decimal import Decimal, localcontext
from fractions import Fraction as F

HERE = pathlib.Path(__file__).resolve().parent
ALGORITHM = "unc.prop.rank@1.0.0"
U = 2.0 ** -53
BLOCK = 256
SHARE_SUM_TOL = 1e-12

# --------------------------------------------------------------------------- formatting


def hex64(x):
    return "%016x" % struct.unpack("<Q", struct.pack("<d", x))[0]


def d50(fr):
    if fr == 0:
        return "0"
    with localcontext() as ctx:
        ctx.prec = 80
        d = Decimal(fr.numerator) / Decimal(fr.denominator)
    return format(d, ".49E")


def norm0(x):
    return 0.0 if x == 0.0 else x


# --------------------------------------------------------------------------- arithmetic


def crsqrt(x):
    """Correctly rounded binary64 square root of a finite x >= 0, decided exactly."""
    assert x >= 0.0 and math.isfinite(x)
    if x == 0.0:
        return x
    fx = F(x)
    y = math.sqrt(x)  # candidate only; corrected below
    for _ in range(4):
        up = math.nextafter(y, math.inf)
        dn = math.nextafter(y, 0.0)
        if ((F(y) + F(up)) / 2) ** 2 < fx:
            y = up
        elif ((F(y) + F(dn)) / 2) ** 2 > fx:
            y = dn
        else:
            return y
    raise AssertionError("crsqrt did not settle")


def fixed_sum(terms):
    n = len(terms)
    for t in terms:
        assert math.isfinite(t)
    if n == 0:
        return 0.0

    def block(lo, hi):
        s, c = -0.0, -0.0
        for i in range(lo, hi):
            x = terms[i]
            t = s + x
            if abs(s) >= abs(x):
                c += (s - t) + x
            else:
                c += (x - t) + s
            s = t
        return s, c

    def combine(a, b):
        s = a[0] + b[0]
        e = (a[0] - s) + b[0] if abs(a[0]) >= abs(b[0]) else (b[0] - s) + a[0]
        return s, (a[1] + b[1]) + e

    def tree(first, count):
        if count == 1:
            lo = first * BLOCK
            return block(lo, min(n, lo + BLOCK))
        left = count // 2
        return combine(tree(first, left), tree(first + left, count - left))

    s, c = tree(0, -(-n // BLOCK))
    assert math.isfinite(s) and math.isfinite(c)
    r = s if c == 0.0 else s + c
    assert math.isfinite(r)
    return r


def dot(a, b):
    assert len(a) == len(b)
    return fixed_sum([a[i] * b[i] for i in range(len(a))])


def sum_sq(xs):
    return fixed_sum([x * x for x in xs])


def mean(xs):
    return fixed_sum(xs) / len(xs)


# --------------------------------------------------------------------------- ranking


def order(entries, key):
    """entries: dicts with 'input_id'; key(e) -> float or None (absent sorts last)."""

    def k(e):
        v = key(e)
        return (1, 0.0, e["input_id"].encode()) if v is None else (0, -v, e["input_id"].encode())

    out = sorted(entries, key=k)
    prev = None
    for i, e in enumerate(out):
        kv = key(e)
        e["rank"] = i + 1
        e["tie_broken_by_id"] = i > 0 and kv == prev
        prev = kv
    return out


def rank_lpu(ids, c, cov):
    n = len(c)
    rows = [cov[i * n:(i + 1) * n] for i in range(n)]
    u = [norm0(abs(c[i]) * crsqrt(rows[i][i])) for i in range(n)]
    w = [dot(rows[i], c) for i in range(n)]
    v = dot(c, w)
    b = [dot([abs(x) for x in rows[i]], [abs(x) for x in c]) for i in range(n)]
    a = dot([abs(x) for x in c], b)
    tol = (4.0 * (float(n) + 1.0) * U) * a
    if v < -tol:
        return {"error": "negative_variance"}
    entries = [{"input_id": ids[i], "sensitivity": norm0(c[i]), "u_contrib": u[i], "share": None} for i in range(n)]
    if v <= tol:
        degenerate = "zero_variance" if v == 0.0 else "variance_at_rounding_resolution"
        shares, share_sum = "degenerate", None
    else:
        degenerate = "none"
        sh = [norm0((c[i] * w[i]) / v) for i in range(n)]
        share_sum = norm0(fixed_sum(sh))
        if abs(share_sum - 1.0) > SHARE_SUM_TOL:
            shares = "sum_out_of_tolerance"
        else:
            shares = "present"
            for i in range(n):
                entries[i]["share"] = sh[i]
    out = order(entries, lambda e: e["u_contrib"])
    return {"method": "lpu", "contributors": out, "degenerate": degenerate, "shares": shares,
            "share_sum": share_sum, "v": v, "tol": tol}


def constant(col):
    return all(x == col[0] for x in col)


def rank_samples(ids, inputs, output):
    m = len(output)
    entries = []
    degenerate = "none"
    syy = None
    dy = None
    if constant(output):
        degenerate = "zero_output_variance"
    else:
        my = mean(output)
        dy = [y - my for y in output]
        syy = sum_sq(dy)
        if syy == 0.0:
            degenerate = "zero_output_variance"
    for i, col in enumerate(inputs):
        e = {"input_id": ids[i], "r2": None, "r2_absent": "none"}
        if degenerate != "none":
            e["r2_absent"] = "degenerate_output"
        elif constant(col):
            e["r2_absent"] = "zero_input_variance"
        else:
            mx = mean(col)
            dx = [x - mx for x in col]
            sxx = sum_sq(dx)
            if sxx == 0.0:
                e["r2_absent"] = "zero_input_variance"
            else:
                sxy = dot(dx, dy)
                r = (sxy / crsqrt(sxx)) / crsqrt(syy)
                r2 = r * r
                e["r2"] = norm0(1.0 if r2 > 1.0 else r2)
        entries.append(e)
    out = order(entries, lambda e: e["r2"])
    return {"method": "approximate_src2", "contributors": out, "degenerate": degenerate, "trials": m}


def canonical(res):
    """RFC 8785 bytes of the ranking (ASCII only, so sorted keys and no whitespace suffice)."""
    hx = lambda x: "absent" if x is None else hex64(x)
    if res["method"] == "lpu":
        cs = [{"input_id": e["input_id"], "rank": str(e["rank"]), "sensitivity": hex64(e["sensitivity"]),
               "share": hx(e["share"]), "tie_broken_by_id": "true" if e["tie_broken_by_id"] else "false",
               "u_contrib": hex64(e["u_contrib"])} for e in res["contributors"]]
        obj = {"algorithm": ALGORITHM, "contributors": cs, "degenerate": res["degenerate"], "method": "lpu",
               "share_sum": hx(res["share_sum"]), "shares": res["shares"]}
    else:
        cs = [{"input_id": e["input_id"], "r2": hx(e["r2"]), "r2_absent": e["r2_absent"], "rank": str(e["rank"]),
               "tie_broken_by_id": "true" if e["tie_broken_by_id"] else "false"} for e in res["contributors"]]
        obj = {"algorithm": ALGORITHM, "basis": "simulated", "contributors": cs, "degenerate": res["degenerate"],
               "method": "approximate_src2", "trials": str(res["trials"])}
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=True)


# --------------------------------------------------------------------------- exact references


def exact_shares(c, cov):
    n = len(c)
    fc = [F(x) for x in c]
    fs = [F(x) for x in cov]
    w = [sum(fs[i * n + j] * fc[j] for j in range(n)) for i in range(n)]
    v = sum(fc[i] * w[i] for i in range(n))
    return [fc[i] * w[i] / v for i in range(n)]


# --------------------------------------------------------------------------- Philox (UNC.MODEL.DIST)

M0, M1 = 0xD2511F53, 0xCD9E8D57
W0, W1 = 0x9E3779B9, 0xBB67AE85
MASK = 0xFFFFFFFF


def philox(ctr, key):
    c0, c1, c2, c3 = ctr
    k0, k1 = key
    for r in range(10):
        if r:
            k0, k1 = (k0 + W0) & MASK, (k1 + W1) & MASK
        p0, p1 = M0 * c0, M1 * c2
        c0, c1, c2, c3 = (p1 >> 32) ^ c1 ^ k0, p1 & MASK, (p0 >> 32) ^ c3 ^ k1, p0 & MASK
    return c0, c1, c2, c3


# Published Random123 known answer for philox4x32_10 (kat_vectors), checked before use.
assert philox((0, 0, 0, 0), (0, 0)) == (0x6627E8D5, 0xE169C58D, 0xBC57AC4C, 0x9B00DBD8)


def open_unit(seed, stream, index):
    w = philox((index & MASK, index >> 32, stream & MASK, stream >> 32), (seed & MASK, seed >> 32))
    x = (w[1] << 32) | w[0]
    return float((x >> 12) * 2 + 1) * U


def rect(seed, stream, n, a, b):
    return [a + (b - a) * open_unit(seed, stream, i) for i in range(n)]


# --------------------------------------------------------------------------- cases


def lpu_case(name, ids, c, cov, note):
    res = rank_lpu(ids, c, cov)
    case = {"name": name, "note": note, "ids": ids, "c": [hex64(x) for x in c], "cov": [hex64(x) for x in cov]}
    if "error" in res:
        case["error"] = res["error"]
        return case, None
    case.update({
        "order": [e["input_id"] for e in res["contributors"]],
        "tie_broken_by_id": ["true" if e["tie_broken_by_id"] else "false" for e in res["contributors"]],
        "u_contrib": [hex64(e["u_contrib"]) for e in res["contributors"]],
        "share": ["absent" if e["share"] is None else hex64(e["share"]) for e in res["contributors"]],
        "degenerate": res["degenerate"],
        "shares": res["shares"],
        "share_sum": "absent" if res["share_sum"] is None else hex64(res["share_sum"]),
    })
    if res["shares"] != "degenerate":
        ex = exact_shares(c, cov)
        by_id = dict(zip(ids, ex))
        case["share_ref50"] = [d50(by_id[i]) for i in case["order"]]
    return case, canonical(res)


def build_lpu_3in():
    case, canon = lpu_case(
        "lpu_3in", ["x1", "x2", "x3"], [1.0, 2.0, -1.0],
        [0.01, 0.0, 0.0, 0.0, 0.0025, 0.0, 0.0, 0.0, 0.04],
        "AC-1: c = (1, 2, -1), independent u = (0.1, 0.05, 0.2); shares (0.01, 0.01, 0.04) / 0.06")
    # The AC-1 statements, checked here before anything is written.
    assert case["order"] == ["x3", "x1", "x2"], case["order"]
    assert case["tie_broken_by_id"] == ["false", "false", "true"]
    want = [F(4, 6), F(1, 6), F(1, 6)]
    for h, wv in zip(case["share"], want):
        got = struct.unpack("<d", bytes.fromhex(h)[::-1])[0]
        assert abs(F(got) - wv) <= F(1, 10 ** 15), (got, wv)
    case["share_tol"] = hex64(1e-15)
    return case, canon


def build_lpu_corr():
    u1, u2, rho = 0.3, 0.1, -0.9
    s12 = -0.027  # rho u1 u2 as a decimal literal
    case, canon = lpu_case(
        "lpu_corr", ["xa", "xb"], [1.0, 2.0], [0.09, s12, s12, 0.01],
        "AC-2: c = (1, 2), u = (0.3, 0.1), rho = -0.9 (S_12 = -0.027): shares 18/11 and -7/11")
    assert (u1, u2, rho) == (0.3, 0.1, -0.9)
    for h, r in zip(case["share"], case["share_ref50"]):
        got = struct.unpack("<d", bytes.fromhex(h)[::-1])[0]
        assert abs(F(got) - F(Decimal(r))) <= F(1, 10 ** 14), (got, r)
    case["share_tol"] = hex64(1e-14)
    return case, canon


def build_rules():
    cases = []
    canon = []

    def add(*args):
        case, cn = lpu_case(*args)
        cases.append(case)
        if cn is not None:
            canon.append((case["name"], cn))

    add("all_zero_c", ["x1", "x2", "x3"], [0.0, 0.0, 0.0], [0.01, 0.0, 0.0, 0.0, 0.0025, 0.0, 0.0, 0.0, 0.04],
        "AC-4: every sensitivity zero: v = 0, shares absent, degenerate zero_variance; all keys tie")
    add("perfect_cancel", ["x1", "x2"], [1.0, 1.0], [0.1, -0.1, -0.1, 0.1],
        "rho = -1, equal u and c: w = 0 exactly, v = 0, degenerate zero_variance")
    add("rounding_resolution", ["x1", "x2"], [1.0, 1.0 + 2.0 ** -52], [0.1, -0.1, -0.1, 0.1],
        "rho = -1 and c differ by one ulp: v is below the rounding tolerance, degenerate "
        "variance_at_rounding_resolution")
    add("not_psd", ["x1", "x2"], [1.0, 1.0], [1.0, -2.0, -2.0, 1.0],
        "|S_12| > sqrt(S_11 S_22): v = -2 < -tol, refused negative_variance")
    add("negative_share", ["x1", "x2"], [1.0, 1.0], [1.0, -1.8, -1.8, 4.0],
        "u = (1, 2), rho = -0.9: the Euler share of x1 is negative (-0.8 / 1.4); shares present, "
        "statement conversion refused")
    add("single_input", ["only"], [-3.0], [4.0], "one input: u = 6, share 1")
    # Perfect anti-correlation of x1 and x2 with u = (1, 1 + t), c = (1, 1, t / 2) and an
    # independent x3 with u = 0.3: v is about t^2 and the shares about -+1/t, and with three
    # terms the rounded sum of the shares misses 1 by more than 1e-12. Two inputs alone do not
    # show it: there the two quotients' errors cancel exactly. The first such t of a fixed
    # list is used.
    for k in range(3, 8):
        t = 10.0 ** -k
        s2 = 1.0 + t
        cov = [1.0, -s2, 0.0, -s2, s2 * s2, 0.0, 0.0, 0.0, 0.3 * 0.3]
        c = [1.0, 1.0, 0.5 * t]
        res = rank_lpu(["x1", "x2", "x3"], c, cov)
        if "error" not in res and res["shares"] == "sum_out_of_tolerance":
            add("share_sum_out_of_tolerance", ["x1", "x2", "x3"], c, cov,
                f"rho_12 = -1, u = (1, 1 + 1e-{k}, 0.3), c = (1, 1, 0.5e-{k}): shares near -+1e{k} whose "
                "rounded sum misses 1 by more than 1e-12; shares dropped, share_sum reported")
            break
    else:
        raise AssertionError("no sum_out_of_tolerance case found")
    for cs in cases:
        if cs["name"] == "all_zero_c":
            assert cs["degenerate"] == "zero_variance" and cs["shares"] == "degenerate"
            assert cs["order"] == ["x1", "x2", "x3"], cs["order"]
            assert cs["tie_broken_by_id"] == ["false", "true", "true"]
        if cs["name"] == "rounding_resolution":
            assert cs["degenerate"] == "variance_at_rounding_resolution", cs
        if cs["name"] == "not_psd":
            assert cs["error"] == "negative_variance"
        if cs["name"] == "negative_share":
            assert cs["shares"] == "present"
            assert any(int(h, 16) >> 63 for h in cs["share"])  # a share with the sign bit set
    return {"cases": cases}, canon


def build_samples_rules():
    """Hand-built sample cases for the degenerate and tie rules (REQ-002, -003, -004)."""
    out = []
    canon = []

    def add(name, ids, inputs, output, note):
        res = rank_samples(ids, inputs, output)
        out.append({
            "name": name, "note": note, "ids": ids,
            "inputs": [[hex64(x) for x in col] for col in inputs],
            "output": [hex64(x) for x in output],
            "order": [e["input_id"] for e in res["contributors"]],
            "r2": ["absent" if e["r2"] is None else hex64(e["r2"]) for e in res["contributors"]],
            "r2_absent": [e["r2_absent"] for e in res["contributors"]],
            "tie_broken_by_id": ["true" if e["tie_broken_by_id"] else "false" for e in res["contributors"]],
            "degenerate": res["degenerate"],
        })
        canon.append((name, canonical(res)))
        return res

    r = add("constant_output", ["x1", "x2"], [[1.0, 2.0, 3.0], [3.0, 1.0, 2.0]], [0.1, 0.1, 0.1],
            "output bitwise constant (0.1, whose rounded mean is not 0.1): degenerate, every r2 absent")
    assert r["degenerate"] == "zero_output_variance"
    r = add("constant_input", ["a", "b", "c"], [[0.1, 0.1, 0.1, 0.1], [1.0, 2.0, 3.0, 4.0], [4.0, 3.0, 2.0, 1.0]],
            [2.0, 4.0, 6.0, 8.0],
            "input a constant: r2 absent (zero_input_variance), ranked last; b and c tie at r2 = 1 by ID")
    assert [e["input_id"] for e in r["contributors"]] == ["b", "c", "a"]
    assert [e["tie_broken_by_id"] for e in r["contributors"]] == [False, True, False]
    assert r["contributors"][0]["r2"] == 1.0 and r["contributors"][1]["r2"] == 1.0
    r = add("partial", ["p", "q"], [[1.0, 2.0, 3.0, 4.0, 5.0], [2.0, -1.0, 0.5, 3.0, -2.0]],
            [1.0, 3.0, 2.0, 5.0, 4.0], "a hand-built pair with distinct r2 below 1")
    exact = []
    for col in [[1.0, 2.0, 3.0, 4.0, 5.0], [2.0, -1.0, 0.5, 3.0, -2.0]]:
        y = [1.0, 3.0, 2.0, 5.0, 4.0]
        fx, fy = [F(x) for x in col], [F(v) for v in y]
        mx, my = sum(fx) / 5, sum(fy) / 5
        sxy = sum((a - mx) * (b - my) for a, b in zip(fx, fy))
        exact.append(sxy * sxy / (sum((a - mx) ** 2 for a in fx) * sum((b - my) ** 2 for b in fy)))
    by = {e["input_id"]: e["r2"] for e in r["contributors"]}
    assert abs(F(by["p"]) - exact[0]) < F(1, 10 ** 15) and abs(F(by["q"]) - exact[1]) < F(1, 10 ** 15)
    out[-1]["r2_ref50"] = [d50(exact[0] if i == "p" else exact[1]) for i in out[-1]["order"]]
    return out, canon


def build_mcm():
    seed, n = 0x52414E4B, 100_000
    x1 = rect(seed, 0, n, -1.0, 1.0)
    x2 = rect(seed, 1, n, -1.0, 1.0)
    y = [x1[i] + 3.0 * x2[i] for i in range(n)]
    h = hashlib.sha256()
    for col in (x1, x2, y):
        h.update(b"".join(struct.pack("<d", v) for v in col))
    res = rank_samples(["X1", "X2"], [x1, x2], y)
    assert [e["input_id"] for e in res["contributors"]] == ["X2", "X1"]
    # Exact squared correlations of the drawn samples, and the population values 0.9 / 0.1.
    exact = {}
    fy = [F(v) for v in y]
    my = sum(fy) / n
    dy = [v - my for v in fy]
    syy = sum(d * d for d in dy)
    for name, col in (("X1", x1), ("X2", x2)):
        fx = [F(v) for v in col]
        mx = sum(fx) / n
        dx = [v - mx for v in fx]
        sxy = sum(a * b for a, b in zip(dx, dy))
        exact[name] = sxy * sxy / (sum(d * d for d in dx) * syy)
    for e in res["contributors"]:
        assert abs(F(e["r2"]) - exact[e["input_id"]]) < F(1, 10 ** 13), (e, float(exact[e["input_id"]]))
    assert abs(exact["X2"] - F(9, 10)) < F(1, 100) and abs(exact["X1"] - F(1, 10)) < F(1, 100)
    case = {
        "name": "mcm_samples",
        "note": "AC-3: Y = X1 + 3 X2 (evaluated as x1 + 3.0 * x2), X1 on stream 0 and X2 on stream 1, "
                "rectangular [-1, 1] in si:m, seed 0x52414E4B, trials 0 .. 10^5",
        "seed": "%x" % seed, "trials": str(n), "streams": ["0", "1"], "a": hex64(-1.0), "b": hex64(1.0),
        "samples_sha256": h.hexdigest(),
        "order": [e["input_id"] for e in res["contributors"]],
        "r2": [hex64(e["r2"]) for e in res["contributors"]],
        "r2_ref50": [d50(exact[e["input_id"]]) for e in res["contributors"]],
        "method": "approximate_src2",
    }
    return case, canonical(res)


def dump(obj):
    return (json.dumps(obj, indent=1, sort_keys=True) + "\n").encode("ascii")


def build():
    a1, c1 = build_lpu_3in()
    a2, c2 = build_lpu_corr()
    rules, crules = build_rules()
    srules, csrules = build_samples_rules()
    rules["samples"] = srules
    m, cm = build_mcm()
    lines = ["# UNC.PROP.RANK canonical rankings (fixtures/unc/rank/make_fixtures.py). <case> <bytes>",
             "lpu_3in " + c1, "lpu_corr " + c2]
    lines += [f"{name} {cn}" for name, cn in crules + csrules]
    lines.append("mcm_samples " + cm)
    return {
        "lpu_3in.json": dump(a1),
        "lpu_corr.json": dump(a2),
        "rules.json": dump(rules),
        "mcm_samples.json": dump(m),
        "expected_canonical.txt": ("\n".join(lines) + "\n").encode("ascii"),
    }


def main():
    check = "--check" in sys.argv[1:]
    out = build()
    bad = 0
    for name, data in out.items():
        target = HERE / name
        if check:
            if not target.exists() or target.read_bytes() != data:
                print(f"stale: {target}")
                bad += 1
        else:
            target.write_bytes(data)
            print(f"wrote {target}")
    if bad:
        sys.exit(1)


if __name__ == "__main__":
    main()
