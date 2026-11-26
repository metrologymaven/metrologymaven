#!/usr/bin/env python3
"""Fixture of spec node AUTO.PROG.EXPR.EVAL: fixtures/program/eval/cases.json.

300 expressions, each with its typed environment and its expected result (a value in the
canonical form of `Value::to_value`, reals as IEEE 754 bits) or expected error (code, kind,
span). The expressions are built here as trees, printed to source text (every compound
node in parentheses, so spans are known), and evaluated by an independent reference
evaluator written in this file from the pinned rules of spec/auto/prog/expr/eval/NODE.md
and the module docs of crates/fid-program/src/expr/eval/. Nothing here runs the Rust code.

Arithmetic of the reference:
- exact decimals are Python integers with a scale, fitted to 38 digits and |scale| <= 400;
- one rounding into f64 is `float(Fraction)` (correctly rounded, ties to even), with the
  catalogue's Pi taken as the exact rational PI_256 / 2^254 of GEO.FRAME.UNITS;
- exact comparisons are `Fraction` comparisons;
- f64 + - * / are CPython floats (IEEE 754 binary64, round to nearest even), % is math.fmod;
- sqrt is math.sqrt (IEEE correctly rounded); sin, cos, tan and atan2 come from the
  bit-exact Python model of the vendored libm in fixtures/num/make_fn_fixtures.py
  (KRN.NUM.FN), itself checked against a 90-digit oracle there.

Stdlib only. CC-BY-4.0 project data. `--check` regenerates in memory and fails if the
committed file differs.
"""

import json
import math
import random
import struct
import sys
from decimal import Decimal
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
sys.path.insert(0, str(ROOT / "fixtures" / "num"))
import make_fn_fixtures as fn  # noqa: E402  (bit-exact model of fid_num::f)

OUT = HERE / "cases.json"
CATALOGUE_REL = "fixtures/program/types/catalogue.json"
N_CASES = 300

# GEO.FRAME.UNITS Pi: the integer nearest to pi * 2^254, from fid-geo-frame's PI_256 limbs
# (read from the catalogue's pi_hex, which CORE.REG.DEF.SCHEMA pins to that constant).
CAT = json.loads((ROOT / CATALOGUE_REL).read_text(encoding="utf-8"))
PI = Fraction(int(CAT["pi_hex"], 16), 1 << 254)

UNITS = {}  # symbol -> (kind, Fraction factor)
CANON = {}  # kind -> canonical symbol
for k in CAT["kinds"]:
    CANON[k["kind"]] = k["canonical"]
    for u in k["units"]:
        f = Fraction(int(u["num"]), int(u["den"]))
        UNITS[u["symbol"]] = (k["kind"], f * PI if u["pi"] else f, (int(u["num"]), int(u["den"]), u["pi"]))

# Dimensions of the catalogue kinds (KIND_DIMS of AUTO.PROG.EXPR.TYPES): 7 SI exponents + angle.
BASE = ["m", "kg", "s", "A", "K", "mol", "cd"]
DIMS = {
    "length": (1, 0, 0, 0, 0, 0, 0, 0),
    "area": (2, 0, 0, 0, 0, 0, 0, 0),
    "time": (0, 0, 1, 0, 0, 0, 0, 0),
    "angle": (0, 0, 0, 0, 0, 0, 0, 1),
}
NONE = (0,) * 8
ANGLE = (0, 0, 0, 0, 0, 0, 0, 1)


def dim_text(d):
    parts = []
    for sym, e in list(zip(BASE, d[:7])) + [("rad", d[7])]:
        if e == 0:
            continue
        parts.append(sym if e == 1 else f"{sym}^{e}")
    return "*".join(parts) if parts else "1"


def dim_of(kind):
    if kind[0] == "named":
        return DIMS[kind[1]]
    return kind[1]


def kind_of_dim(d):
    """None for a plain number, the one catalogue kind with that dimension, else derived."""
    if d == NONE:
        return None
    named = [k for k, v in DIMS.items() if v == d and k in CANON]
    if len(named) == 1:
        return ("named", named[0])
    return ("derived", d)


def kind_str(k):
    return k[1] if k[0] == "named" else "dim:" + dim_text(k[1])


class Fail(Exception):
    def __init__(self, code, kind=None):
        super().__init__(code)
        self.code = code
        self.kind = kind


def err(kind):
    return Fail("eval_error", kind)


MAXC = 10**38 - 1
I64_MIN, I64_MAX = -(2**63), 2**63 - 1


def fit(c, s):
    while abs(c) > MAXC or s > 400:
        if c % 10 != 0:
            raise Fail("decimal_overflow")
        c //= 10
        s -= 1
    while s < -400:
        if abs(c * 10) > MAXC:
            raise Fail("decimal_overflow")
        c *= 10
        s += 1
    return c, s


# ------------------------------------------------------------------ values
# ("bool", b) ("int", i) ("dec", coef, scale, unit|None) ("real", float, kind|None)
# ("str", s) ("enum", s) ("element", s) ("list", [v]) ("record", {k: v})


def dec_text(c, s):
    sign = "-" if c < 0 else ""
    digits = str(abs(c))
    if s <= 0:
        return sign + digits + "0" * (-s)
    if len(digits) <= s:
        digits = "0" * (s + 1 - len(digits)) + digits
    return f"{sign}{digits[:-s]}.{digits[-s:]}"


def frac_of_dec(v):
    return Fraction(v[1], 10 ** v[2]) if v[2] >= 0 else Fraction(v[1] * 10 ** (-v[2]))


def bits(x):
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def to_json(v):
    t = v[0]
    o = {"not_of_record": True, "t": t}
    if t == "bool":
        o["v"] = v[1]
    elif t == "int":
        o["v"] = str(v[1])
    elif t == "dec":
        o["v"] = dec_text(v[1], v[2])
        o["unit"] = v[3]
    elif t == "real":
        o["bits"] = "0x%016x" % bits(v[1])
        o["kind"] = None if v[2] is None else kind_str(v[2])
    elif t in ("str", "enum", "element"):
        o["v"] = v[1]
    elif t == "list":
        o["v"] = [to_json(x) for x in v[1]]
    elif t == "record":
        o["v"] = {k: to_json(x) for k, x in v[1].items()}
    return o


def is_num(v):
    return v[0] in ("int", "dec", "real")


def exact(v):
    if v[0] == "int":
        return (v[1], 0, None)
    if v[0] == "dec":
        return (v[1], v[2], v[3])
    return None


def vkind(v):
    if v[0] == "dec" and v[3] is not None:
        return ("named", UNITS[v[3]][0])
    if v[0] == "real":
        return v[2]
    return None


def round_f64(q):
    """One rounding of an exact rational; out of range or nonzero-to-zero is domain."""
    try:
        x = float(q)
    except OverflowError:
        raise err("domain")
    if math.isinf(x) or (x == 0.0 and q != 0):
        raise err("domain")
    return x


def to_f64(v):
    if v[0] == "int":
        return float(v[1])
    if v[0] == "dec":
        q = frac_of_dec(v)
        if v[3] is not None:
            q = q * UNITS[v[3]][1]
        return round_f64(q)
    return v[1]


def finite(x, kind):
    if math.isinf(x) or math.isnan(x):
        raise err("non_finite")
    return ("real", x, kind)


def product_kind(a, b, sign):
    if a is None and b is None:
        return None
    if b is None:
        return a
    if a is None and sign > 0:
        return b
    da = NONE if a is None else dim_of(a)
    db = dim_of(b)
    return kind_of_dim(tuple(x + sign * y for x, y in zip(da, db)))


def align(a, b):
    s = max(a[1], b[1])
    return a[0] * 10 ** (s - a[1]), b[0] * 10 ** (s - b[1]), s


def pow10(r):
    """k when the Fraction r is 10^k, else None."""
    for n, sign in ((r.numerator, 1), (r.denominator, -1)):
        other = r.denominator if sign == 1 else r.numerator
        if other != 1:
            continue
        k = 0
        while n > 1 and n % 10 == 0:
            n //= 10
            k += 1
        if n == 1:
            return sign * k
    return None


def unit_product(u, v):
    """Where the exact product of decimals in u and v is written: (unit or None, shift k),
    or None when it is converted to f64."""
    (ku, _, (nu, du, pu)), (kv, _, (nv, dv, pv)) = UNITS[u], UNITS[v]
    pis = int(pu) + int(pv)
    r = Fraction(nu * nv, du * dv)
    kind = product_kind(("named", ku), ("named", kv), 1)
    if kind is not None and kind[0] == "named" and pis <= 1:
        for k in CAT["kinds"]:
            if k["kind"] != kind[1]:
                continue
            for w in k["units"]:  # catalogue order: by symbol
                if (int(w["num"]), int(w["den"]), w["pi"]) == (r.numerator, r.denominator, pis == 1):
                    return (w["symbol"], 0)
    if pis:
        return None
    k = pow10(r)
    if k is None:
        return None
    if kind is None:
        return (None, k)
    if kind[0] == "named":
        return (CANON[kind[1]], k)
    return None


def arith(op, a, b):
    if a[0] == "int" and b[0] == "int":
        x, y = a[1], b[1]
        if op in ("+", "-", "*"):
            r = x + y if op == "+" else x - y if op == "-" else x * y
            if not I64_MIN <= r <= I64_MAX:
                raise err("int_overflow")
            return ("int", r)
        if op == "%":
            if y == 0:
                raise err("div_zero")
            r = abs(x) % abs(y)
            r = -r if x < 0 else r
            if not I64_MIN <= r <= I64_MAX:
                raise err("int_overflow")
            return ("int", r)
        if y == 0:
            raise err("div_zero")
        return finite(float(x) / float(y), None)
    ea, eb = exact(a), exact(b)
    if ea is not None and eb is not None and op == "*" and ea[2] is not None and eb[2] is not None:
        w = unit_product(ea[2], eb[2])
        if w is not None:
            c, s = fit(ea[0] * eb[0], ea[1] + eb[1] - w[1])
            return ("dec", c, s, w[0])
    if ea is not None and eb is not None:
        unit = "no"
        if op in ("+", "-", "%") and ea[2] == eb[2]:
            unit = ea[2]
        elif op == "*" and (ea[2] is None or eb[2] is None):
            unit = ea[2] if ea[2] is not None else eb[2]
        if unit != "no":
            if op in ("+", "-"):
                x, y, s = align(ea, eb)
                c, s = fit(x + y if op == "+" else x - y, s)
            elif op == "*":
                c, s = fit(ea[0] * eb[0], ea[1] + eb[1])
            else:
                x, y, s = align(ea, eb)
                if y == 0:
                    raise err("div_zero")
                r = abs(x) % abs(y)
                c, s = fit(-r if x < 0 else r, s)
            return ("dec", c, s, unit)
    ka, kb = vkind(a), vkind(b)
    x, y = to_f64(a), to_f64(b)
    if op == "+":
        return finite(x + y, ka or kb)
    if op == "-":
        return finite(x - y, ka or kb)
    if op == "%":
        if y == 0.0:
            raise err("div_zero")
        return finite(math.fmod(x, y), ka or kb)
    if op == "*":
        return finite(x * y, product_kind(ka, kb, 1))
    if y == 0.0:
        raise err("div_zero")
    return finite(x / y, product_kind(ka, kb, -1))


def sgn(q):
    return (q > 0) - (q < 0)


def cmp_num(a, b):
    if a[0] == "real" and b[0] == "real":
        return sgn(Fraction(a[1]) - Fraction(b[1]))
    if a[0] == "real" or b[0] == "real":
        flip = a[0] != "real"
        r, d = (b, a) if flip else (a, b)
        q = Fraction(d[1]) if d[0] == "int" else frac_of_dec(d)
        if d[0] == "dec" and d[3] is not None:
            q = q * UNITS[d[3]][1]
        o = sgn(Fraction(r[1]) - q)
        return -o if flip else o
    ea, eb = exact(a), exact(b)
    qa = frac_of_dec(("dec", ea[0], ea[1])) * (UNITS[ea[2]][1] if ea[2] is not None else 1)
    qb = frac_of_dec(("dec", eb[0], eb[1])) * (UNITS[eb[2]][1] if eb[2] is not None else 1)
    return sgn(qa - qb)


def equal(a, b):
    if is_num(a):
        return cmp_num(a, b) == 0
    if a[0] in ("str", "enum") and b[0] in ("str", "enum"):
        return a[1] == b[1]
    if a[0] == "list":
        return len(a[1]) == len(b[1]) and all(equal(x, y) for x, y in zip(a[1], b[1]))
    if a[0] == "record":
        return list(a[1]) == list(b[1]) and all(equal(a[1][k], b[1][k]) for k in a[1])
    return a[1] == b[1]


def total_cmp(x, y):
    if x == y:
        sx, sy = math.copysign(1, x) < 0, math.copysign(1, y) < 0
        return (sy - sx) if sx != sy else 0
    return -1 if x < y else 1


def rust_f64(x):
    """Rust's `{}` text of an f64: shortest round-trip digits, positional, no exponent."""
    if x == 0:
        return "-0" if math.copysign(1, x) < 0 else "0"
    s = format(Decimal(repr(x)), "f")
    if "." in s:
        s = s.rstrip("0").rstrip(".")
    return s


def display(v):
    t = v[0]
    if t == "bool":
        return "true" if v[1] else "false"
    if t == "int":
        return str(v[1])
    if t == "dec":
        txt = dec_text(v[1], v[2])
        return txt if v[3] is None else f"{txt} {v[3]}"
    if t == "real":
        txt = rust_f64(v[1])
        if v[2] is None:
            return txt
        if v[2][0] == "named":
            return f"{txt} {CANON[v[2][1]]}"
        return f"{txt} {dim_text(v[2][1])}"
    if t in ("str", "enum", "element"):
        return v[1]
    if t == "list":
        return "[" + ", ".join(display(x) for x in v[1]) + "]"
    return "{" + ", ".join(f"{k}: {display(x)}" for k, x in v[1].items()) + "}"


def fmt(tpl, args):
    out, i, used = "", 0, 0
    while i < len(tpl):
        c = tpl[i]
        nxt = tpl[i + 1] if i + 1 < len(tpl) else None
        if c == "{" and nxt == "{":
            out += "{"
            i += 2
        elif c == "{" and nxt == "}":
            if used >= len(args):
                raise err("domain")
            out += display(args[used])
            used += 1
            i += 2
        elif c == "}" and nxt == "}":
            out += "}"
            i += 2
        elif c in "{}":
            raise err("domain")
        else:
            out += c
            i += 1
    if used != len(args):
        raise err("domain")
    return ("str", out)


def to_int(v, mode):
    if v[0] == "int":
        return v
    if v[0] == "dec":
        q = frac_of_dec(v)
    else:
        if not -(2.0**63) <= v[1] < 2.0**63:
            raise err("int_overflow")
        q = Fraction(v[1])
    r = math.floor(q) if mode == "floor" else math.ceil(q) if mode == "ceil" else round(q)
    if not I64_MIN <= r <= I64_MAX:
        raise err("int_overflow")
    return ("int", r)


def to_unit(q, sym):
    if q[0] == "real":
        return q
    u = q[3]
    if u == sym:
        return q
    nu, du, pu = UNITS[u][2]
    nt, dt, pt = UNITS[sym][2]
    if pu == pt:
        r = Fraction(nu * dt, du * nt)
        d, a, b = r.denominator, 0, 0
        while d % 2 == 0:
            d //= 2
            a += 1
        while d % 5 == 0:
            d //= 5
            b += 1
        if d == 1:
            m = max(a, b)
            c, s = fit(q[1] * r.numerator * 2 ** (m - a) * 5 ** (m - b), q[2] + m)
            return ("dec", c, s, sym)
    return ("real", to_f64(q), ("named", UNITS[u][0]))


def call(name, a):
    if name == "abs":
        x = a[0]
        if x[0] == "int":
            if x[1] == I64_MIN:
                raise err("int_overflow")
            return ("int", abs(x[1]))
        if x[0] == "dec":
            return ("dec", abs(x[1]), x[2], x[3])
        return ("real", abs(x[1]), x[2])
    if name in ("min", "max"):
        want = -1 if name == "min" else 1
        acc = a[0]
        for x in a[1:]:
            o = total_cmp(x[1], acc[1]) if x[0] == "real" and acc[0] == "real" else cmp_num(x, acc)
            if o == want:
                acc = x
        if acc[0] == "int" and not all(x[0] == "int" for x in a):
            return ("dec", acc[1], 0, None)
        return acc
    if name in ("round", "floor", "ceil"):
        return to_int(a[0], {"round": "even", "floor": "floor", "ceil": "ceil"}[name])
    if name == "sqrt":
        x = a[0]
        k = vkind(x)
        if k is not None:
            d = dim_of(k)
            k = kind_of_dim(tuple(e // 2 for e in d))
        v = to_f64(x)
        if v < 0:
            raise err("domain")
        return finite(math.sqrt(v), k)
    if name in ("sin", "cos", "tan"):
        v = to_f64(a[0])
        return finite({"sin": fn.m_sin, "cos": fn.m_cos, "tan": fn.m_tan}[name](v), None)
    if name == "atan2":
        return finite(fn.m_atan2(to_f64(a[0]), to_f64(a[1])), kind_of_dim(ANGLE))
    if name == "len":
        return ("int", len(a[0][1]))
    if name == "contains":
        if a[0][0] == "str":
            return ("bool", a[1][1] in a[0][1])
        return ("bool", any(equal(x, a[1]) for x in a[0][1]))
    if name == "format":
        return fmt(a[0][1], a[1:])
    if name == "to_unit":
        return to_unit(a[0], a[1][1])
    raise AssertionError(name)


# ------------------------------------------------------------------ trees and printing


class N:
    def __init__(self, kind, *args):
        self.kind = kind
        self.args = args
        self.span = None


def Int(v):
    return N("int", v)


def Dec(t):
    return N("dec", t)


def Qty(t, u):
    return N("qty", t, u)


def Str(s):
    return N("str", s)


def Bool(b):
    return N("bool", b)


def Id(n):
    return N("ident", n)


def Bin(op, l, r):
    return N("bin", op, l, r)


def Neg(x):
    return N("neg", x)


def Not(x):
    return N("not", x)


def If(c, a, b):
    return N("cond", c, a, b)


def Call(name, *args):
    return N("call", name, *args)


def Ix(b, i):
    return N("index", b, i)


def Fld(b, n):
    return N("field", b, n)


def num(x):
    """A tree for a Python int or decimal text, negatives as unary minus."""
    if isinstance(x, int):
        return Neg(Int(-x)) if x < 0 else Int(x)
    return Neg(Dec(x[1:])) if x.startswith("-") else Dec(x)


def qty(t, u):
    return Neg(Qty(t[1:], u)) if t.startswith("-") else Qty(t, u)


def show(n, out):
    """Appends the source of `n` to `out` (a list of str) and sets every span."""
    start = sum(len(p) for p in out)
    k, a = n.kind, n.args
    if k == "int":
        out.append(str(a[0]))
    elif k == "dec":
        out.append(a[0])
    elif k == "qty":
        out.append(f"{a[0]}[{a[1]}]")
    elif k == "str":
        assert '"' not in a[0] and "\\" not in a[0] and a[0].isascii()
        out.append('"' + a[0] + '"')
    elif k == "bool":
        out.append("true" if a[0] else "false")
    elif k == "ident":
        out.append(a[0])
    elif k == "bin":
        out.append("(")
        show(a[1], out)
        out.append(f" {a[0]} ")
        show(a[2], out)
        out.append(")")
    elif k == "neg":
        out.append("(-")
        show(a[0], out)
        out.append(")")
    elif k == "not":
        out.append("(not ")
        show(a[0], out)
        out.append(")")
    elif k == "cond":
        out.append("(if ")
        show(a[0], out)
        out.append(" then ")
        show(a[1], out)
        out.append(" else ")
        show(a[2], out)
        out.append(")")
    elif k == "call":
        out.append(a[0] + "(")
        for i, x in enumerate(a[1:]):
            if i:
                out.append(", ")
            show(x, out)
        out.append(")")
    elif k == "index":
        show(a[0], out)
        out.append("[")
        show(a[1], out)
        out.append("]")
    elif k == "field":
        show(a[0], out)
        out.append("." + a[1])
    n.span = (start, sum(len(p) for p in out))


class At(Exception):
    def __init__(self, fail, span):
        super().__init__(fail.code)
        self.fail = fail
        self.span = span


def ev(n, env, types):
    """Evaluates a printed tree; errors carry the span of the failing node."""
    k, a = n.kind, n.args
    try:
        if k == "int":
            return ("int", a[0])
        if k in ("dec", "qty"):
            text = a[0]
            i, _, f = text.partition(".")
            c, s = fit(int(i + f), len(f))
            return ("dec", c, s, a[1] if k == "qty" else None)
        if k == "str":
            return ("str", a[0])
        if k == "bool":
            return ("bool", a[0])
        if k == "ident":
            return env[a[0]]
    except Fail as f:
        raise At(f, n.span)
    if k == "bin" and a[0] in ("and", "or"):
        left = ev(a[1], env, types)
        if a[0] == "and" and not left[1]:
            return ("bool", False)
        if a[0] == "or" and left[1]:
            return ("bool", True)
        return ev(a[2], env, types)
    if k == "cond":
        c = ev(a[0], env, types)
        v = ev(a[1] if c[1] else a[2], env, types)
        if v[0] == "int" and n.ty == "decimal":
            return ("dec", v[1], 0, None)
        return v
    kids = [x for x in a if isinstance(x, N)]
    vals = [ev(x, env, types) for x in kids]
    try:
        if k == "neg":
            x = vals[0]
            if x[0] == "int":
                if x[1] == I64_MIN:
                    raise err("int_overflow")
                return ("int", -x[1])
            if x[0] == "dec":
                return ("dec", -x[1], x[2], x[3])
            return ("real", -x[1], x[2])
        if k == "not":
            return ("bool", not vals[0][1])
        if k == "index":
            xs, i = vals[0][1], vals[1][1]
            if not 0 <= i < len(xs):
                raise err("index_range")
            return xs[i]
        if k == "field":
            return vals[0][1][a[1]]
        if k == "call":
            return call(a[0], vals)
        op = a[0]
        l, r = vals
        if op in ("+", "-", "*", "/", "%"):
            return arith(op, l, r)
        if op in ("<", "<=", ">", ">="):
            o = cmp_num(l, r)
            return ("bool", {"<": o < 0, "<=": o <= 0, ">": o > 0, ">=": o >= 0}[op])
        if op == "==":
            return ("bool", equal(l, r))
        if op == "!=":
            return ("bool", not equal(l, r))
    except Fail as f:
        raise At(f, n.span)
    raise AssertionError(k)


# ------------------------------------------------------------------ environments


def real(x, kind=None):
    return ("real", float(x), None if kind is None else ("named", kind))


def dec(text, unit=None):
    i, _, f = text.lstrip("-").partition(".")
    c, s = fit(int(i + f), len(f))
    return ("dec", -c if text.startswith("-") else c, s, unit)


def ints(xs):
    return ("list", [("int", x) for x in xs])


ENV_TYPES = {
    "n": "int",
    "m": "int",
    "x": "decimal",
    "y": "decimal",
    "d": "decimal",
    "q": "quantity<length>",
    "p": "quantity<length>",
    "t": "quantity<time>",
    "ang": "quantity<angle>",
    "ar": "quantity<area>",
    "ok": "bool",
    "s": "string",
    "mode": "enum<fast|slow>",
    "part": "element<part>",
    "xs": "list<int>",
    "qs": "list<quantity<length>>",
    "names": "list<string>",
    "r": "record<probe>",
}
RECORDS = {"probe": {"diameter": "quantity<length>", "hits": "int", "label": "string"}}


def base_env(rng):
    x = rng.choice(["0.1", "2.5", "-3.75", "1e-7", "123456.789", "0.3", "7"])
    return {
        "n": ("int", rng.randint(-50, 50)),
        "m": ("int", rng.choice([0, 3, 7, -4, 9223372036854775807])),
        "x": real(float(x)),
        "y": real(rng.choice([1.5, -0.25, 1e300, 2.0, 0.1 + 0.2])),
        "d": dec(rng.choice(["2.50", "-0.125", "10.000", "3"])),
        "q": real(rng.choice([0.0003, 0.001, 0.0254, 1.0, -0.5]), "length"),
        "p": dec(rng.choice(["0.3", "12.700", "1", "2.54"]), rng.choice(["mm", "in", "m"])),
        "t": dec(rng.choice(["1.5", "90"]), rng.choice(["s", "min", "h"])),
        "ang": real(rng.choice([math.pi / 6, 0.5, -1.25]), "angle"),
        "ar": dec(rng.choice(["4", "2.25", "100"]), rng.choice(["mm^2", "cm^2"])),
        "ok": ("bool", rng.random() < 0.5),
        "s": ("str", rng.choice(["probe", "", "edge-12"])),
        "mode": ("enum", rng.choice(["fast", "slow"])),
        "part": ("element", "E-17"),
        "xs": ints(rng.sample(range(-9, 20), rng.randint(1, 5))),
        "qs": ("list", [dec("0.5", "mm"), real(0.001, "length"), dec("2", "in")]),
        "names": ("list", [("str", "a"), ("str", "bb"), ("str", "")]),
        "r": ("record", {"diameter": dec("3.000", "mm"), "hits": ("int", 4), "label": ("str", "P1")}),
    }


# ------------------------------------------------------------------ case families


def dec_lit(rng, digits=None, scale=None):
    digits = digits or rng.randint(1, 12)
    scale = rng.randint(0, 6) if scale is None else scale
    c = rng.randint(0, 10**digits - 1)
    t = dec_text(c, scale) if scale > 0 else dec_text(c, 0) + ".0"
    return ("-" + t) if rng.random() < 0.3 else t


def fam_int(rng):
    a = rng.choice([rng.randint(-1000, 1000), rng.randint(-(2**62), 2**62), I64_MAX, -I64_MAX])
    b = rng.choice([rng.randint(-1000, 1000), 0, 2, -1, I64_MAX])
    op = rng.choice(["+", "-", "*", "%", "/", "<", "==", ">="])
    return Bin(op, num(a), num(b))


def fam_uprod(rng):
    """Products of two decimals in units: exact into the catalogue unit of factor fu*fv, or the
    canonical unit for a power of ten, else f64 (REQ-002)."""
    u, v = rng.choice([("mm", "mm"), ("cm", "cm"), ("m", "m"), ("mm", "cm"), ("in", "in"), ("mm", "in"), ("um", "mm"), ("deg", "deg"), ("s", "mm"), ("mm^2", "mm")])
    a, b = dec_lit(rng, 4, rng.randint(0, 3)), dec_lit(rng, 4, rng.randint(0, 3))
    prod = Bin("*", qty(a, u), qty(b, v))
    if rng.random() < 0.4 and (u, v) in (("mm", "mm"), ("cm", "cm"), ("mm", "cm")):
        return Bin("==", prod, qty(dec_lit(rng, 3, 1), rng.choice(["mm^2", "cm^2", "m^2"])))
    return prod


def fam_dec(rng):
    a, b = dec_lit(rng), dec_lit(rng)
    op = rng.choice(["+", "-", "*", "%", "<", "==", ">", "/"])
    return Bin(op, num(a), num(b) if rng.random() < 0.8 else num(rng.randint(-20, 20)))


def fam_unit(rng):
    units = ["mm", "m", "in", "um", "cm"]
    u1 = rng.choice(units)
    u2 = u1 if rng.random() < 0.5 else rng.choice(units)
    a, b = dec_lit(rng, 6, rng.randint(0, 4)), dec_lit(rng, 6, rng.randint(0, 4))
    op = rng.choice(["+", "-", "<", "==", "!=", "%", "/"])
    return Bin(op, qty(a, u1), qty(b, u2))


def fam_scale(rng):
    u = rng.choice(["mm", "in", "deg", "s", "mm^2"])
    a, b = dec_lit(rng, 5), dec_lit(rng, 4)
    op = rng.choice(["*", "/"])
    return Bin(op, qty(a, u), num(b)) if rng.random() < 0.5 else Bin("*", num(b), qty(a, u))


def fam_mixed(rng):
    lit = num(dec_lit(rng, 6))
    op = rng.choice(["+", "-", "*", "/", "<", "==", ">=", "%"])
    v = rng.choice(["x", "y"])
    return Bin(op, Id(v), lit) if rng.random() < 0.5 else Bin(op, lit, Id(v))


def fam_real_qty(rng):
    v = rng.choice(["q", "p"])
    u = rng.choice(["mm", "in", "m", "nm"])
    lit = qty(rng.choice(["0.3", "1", "25.4", "0.0254", "1000", dec_lit(rng, 5)]), u)
    op = rng.choice(["<", "<=", "==", "!=", ">", "+", "-"])
    return Bin(op, Id(v), lit) if rng.random() < 0.6 else Bin(op, lit, Id(v))


def fam_angle(rng):
    lit = qty(rng.choice(["30", "45", "0.5", "90", "180", "1"]), rng.choice(["deg", "rad", "gon", "arcmin"]))
    pick = rng.random()
    if pick < 0.4:
        return Bin(rng.choice(["<", "==", ">=", "!="]), Id("ang"), lit)
    if pick < 0.7:
        return Call(rng.choice(["sin", "cos", "tan"]), lit)
    return Bin("+", Id("ang"), lit)


def fam_func(rng):
    pick = rng.randrange(15)
    if pick == 0:
        return Call("abs", rng.choice([num(rng.randint(-99, 99)), num(dec_lit(rng)), Id("x"), Neg(Id("q"))]))
    if pick == 1:
        args = [num(rng.choice([rng.randint(-9, 9), dec_lit(rng, 3)])) for _ in range(rng.randint(2, 4))]
        return Call(rng.choice(["min", "max"]), *args)
    if pick == 2:
        return Call(rng.choice(["min", "max"]), Id("x"), Id("y"), num(dec_lit(rng, 3)))
    if pick == 3:
        arg = rng.choice([num(dec_lit(rng, 4, 1)), num(rng.choice(["2.5", "-2.5", "3.5", "-0.5", "0.49"])), Id("x"), Id("y")])
        return Call(rng.choice(["round", "floor", "ceil"]), arg)
    if pick == 4:
        return Call("sqrt", rng.choice([num(dec_lit(rng, 4)), Id("x"), Id("ar"), Qty("2", "mm^2"), num(rng.randint(0, 99))]))
    if pick == 5:
        return Call(rng.choice(["sin", "cos", "tan"]), rng.choice([num(dec_lit(rng, 4)), Id("x"), Id("ang")]))
    if pick == 6:
        a, b = rng.choice([(Id("q"), Id("p")), (num(dec_lit(rng, 3)), Id("x")), (Id("ang"), Qty("10", "deg"))])
        return Call("atan2", a, b)
    if pick == 7:
        return Call("len", rng.choice([Id("xs"), Id("s"), Id("names"), Str("caf-12")]))
    if pick == 8:
        return Call("contains", *rng.choice([(Id("xs"), num(rng.randint(-9, 19))), (Id("s"), Str("ob")), (Id("names"), Str("bb")), (Id("qs"), Qty("0.5", "mm")), (Id("qs"), Qty("1", "mm"))]))
    if pick == 9:
        return Call("format", Str("{} mm"), num(rng.choice(["2.50", "0.125", "10"])))
    if pick == 10:
        return Call("format", Str("n={} x={} q={} ok={}"), Id("n"), Id("x"), Id("q"), Id("ok"))
    if pick == 11:
        return Call("format", Str(rng.choice(["{{}} {}", "{}{}", "p={} names={}"])), Id("p"), Id("names"))
    if pick == 12:
        q = rng.choice([Id("p"), Id("q"), qty(dec_lit(rng, 5), rng.choice(["mm", "in", "um", "m"]))])
        return Call("to_unit", q, Str(rng.choice(["mm", "m", "in", "cm", "thou"])))
    if pick == 13:
        return Call("to_unit", rng.choice([Id("ang"), qty("90", "deg"), qty("1", "rad")]), Str(rng.choice(["deg", "rad", "gon"])))
    return Call("format", Str("{} at {}"), Id("t"), Id("r"))


def fam_struct(rng):
    pick = rng.randrange(9)
    if pick == 0:
        return Ix(Id("xs"), num(rng.randint(-1, 5)))
    if pick == 1:
        return Bin("+", Fld(Id("r"), "diameter"), rng.choice([Qty("0.5", "mm"), Id("q")]))
    if pick == 2:
        return Bin("*", Fld(Id("r"), "hits"), num(rng.randint(-3, 3)))
    if pick == 3:
        return Bin(rng.choice(["==", "!="]), Id("mode"), Str(rng.choice(["fast", "slow"])))
    if pick == 4:
        return Bin("==", Id("names"), Id("names"))
    if pick == 5:
        return Bin("==", Ix(Id("qs"), num(rng.randint(0, 3))), Qty(rng.choice(["0.5", "2"]), rng.choice(["mm", "in"])))
    if pick == 6:
        return Bin("==", Id("part"), Id("part"))
    if pick == 7:
        return Fld(Id("r"), "label")
    return Bin("==", Id("s"), Str(rng.choice(["probe", ""])))


def fam_logic(rng):
    pick = rng.randrange(6)
    boom = Bin("==", Bin("/", num(1), num(0)), Dec("1.0"))
    if pick == 0:
        return Bin("and", Bool(False), boom)
    if pick == 1:
        return Bin("or", Bool(True), boom)
    if pick == 2:
        return Bin(rng.choice(["and", "or"]), Id("ok"), Bin("<", Id("n"), num(rng.randint(-10, 10))))
    if pick == 3:
        return If(Bin(">", Id("n"), num(0)), num(rng.randint(-5, 5)), num(dec_lit(rng, 3)))
    if pick == 4:
        return If(Id("ok"), Bin("/", Id("x"), num(0)), Id("x"))
    return Not(Bin(rng.choice(["<", "=="]), Id("x"), Id("y")))


def fam_err(rng):
    # Every error shape twice, in order.
    pick = fam_err.count % 12
    fam_err.count += 1
    if pick == 0:
        return Bin(rng.choice(["/", "%"]), num(dec_lit(rng, 3)), Dec("0.0"))
    if pick == 1:
        return Bin(rng.choice(["/", "%"]), Id("x"), num(0))
    if pick == 2:
        return Bin("-", Neg(Int(I64_MAX)), num(rng.choice([1, 2])))
    if pick == 3:
        return Neg(Bin("-", Neg(Int(I64_MAX)), Int(1)))
    if pick == 4:
        return Call("abs", Bin("-", Neg(Int(I64_MAX)), Int(1)))
    if pick == 5:
        return Bin("*", Id("y"), Id("y"))
    if pick == 6:
        return Call("sqrt", num(dec_lit(rng, 3, 1)) if rng.random() < 0.3 else Neg(Dec("2.0")))
    if pick == 7:
        return Call("format", Str(rng.choice(["{} {}", "{", "}", "{x}"])), num(1))
    if pick == 8:
        return Bin("*", Dec("9999999999999999999999.999999999999999"), Dec(rng.choice(["3.3", "10.0", "0.1"])))
    if pick == 9:
        return Dec("1234567890123456789012345678901234567890.0")
    if pick == 10:
        return Call(rng.choice(["floor", "round"]), Id("y"))
    return Ix(Id("names"), num(rng.choice([-1, 3, 10])))


def fam_near(rng):
    """A real that is the nearest double of a decimal, against that decimal: the exact
    comparison (REQ-004) decides, never the rounded value."""
    t = rng.choice(["0.1", "0.3", "0.0003", "2.675", "1.005", "123.456", "0.07", dec_lit(rng, 9, 6)])
    t = t.lstrip("-")
    op = rng.choice(["==", "<", ">", "<=", "!="])
    if rng.random() < 0.5:
        return Bin(op, Id("x"), num(t)), {"x": real(float(Fraction(t)))}
    u = rng.choice(["mm", "in", "um"])
    return Bin(op, Id("q"), Qty(t, u)), {"q": real(float(Fraction(t) * UNITS[u][1]), "length")}


fam_err.count = 0

FAMILIES = [
    (fam_int, 24),
    (fam_near, 18),
    (fam_dec, 24),
    (fam_unit, 24),
    (fam_uprod, 18),
    (fam_scale, 18),
    (fam_mixed, 24),
    (fam_real_qty, 24),
    (fam_angle, 18),
    (fam_func, 48),
    (fam_struct, 18),
    (fam_logic, 18),
    (fam_err, 24),
]
assert sum(c for _, c in FAMILIES) == N_CASES


def type_cond(n, env):
    """Static type of `if` nodes (only the int/decimal distinction is needed): a branch of
    type decimal makes the `if` decimal when the other is int."""
    for x in n.args:
        if isinstance(x, N):
            type_cond(x, env)
    n.ty = None
    if n.kind == "cond":
        kinds = {static_num(n.args[1], env), static_num(n.args[2], env)}
        n.ty = "decimal" if kinds == {"int", "decimal"} or kinds == {"decimal"} else None


def static_num(n, env):
    k, a = n.kind, n.args
    if k == "int":
        return "int"
    if k == "dec":
        return "decimal"
    if k == "ident":
        return ENV_TYPES[a[0]] if ENV_TYPES[a[0]] in ("int", "decimal") else None
    if k == "neg":
        return static_num(a[0], env)
    if k == "bin" and a[0] in ("+", "-", "*", "%"):
        ks = {static_num(a[1], env), static_num(a[2], env)}
        return "int" if ks == {"int"} else "decimal" if ks <= {"int", "decimal"} else None
    if k == "bin" and a[0] == "/":
        ks = {static_num(a[1], env), static_num(a[2], env)}
        return "decimal" if ks <= {"int", "decimal"} else None
    return None


def build():
    rng = random.Random(10)
    cases = []
    for fam, count in FAMILIES:
        for _ in range(count):
            tree = fam(rng)
            env = base_env(rng)
            if isinstance(tree, tuple):
                tree, override = tree
                env.update(override)
            out = []
            show(tree, out)
            src = "".join(out)
            type_cond(tree, env)
            used = sorted({w for w in ENV_TYPES if w in ident_names(tree)})
            try:
                v = ev(tree, env, ENV_TYPES)
                expect = {"ok": to_json(v)}
            except At as e:
                expect = {
                    "err": {
                        "code": e.fail.code,
                        "kind": e.fail.kind,
                        "span": {"end": str(e.span[1]), "start": str(e.span[0])},
                    }
                }
            case = {
                "env": {k: {"type": ENV_TYPES[k], "value": to_json(env[k])} for k in used},
                "expect": expect,
                "id": f"{fam.__name__[4:]}-{len(cases):03d}",
                "src": src,
            }
            cases.append(case)
    return {
        "catalogue": CATALOGUE_REL,
        "cases": cases,
        "format": "fid-program-eval-cases/1",
        "records": RECORDS,
    }


def ident_names(n):
    out = set()
    if n.kind == "ident":
        out.add(n.args[0])
    for x in n.args:
        if isinstance(x, N):
            out |= ident_names(x)
    return out


def render(doc):
    return json.dumps(doc, indent=1, sort_keys=True, ensure_ascii=True) + "\n"


def main():
    text = render(build())
    if "--check" in sys.argv:
        if OUT.read_text(encoding="utf-8") != text:
            print(f"{OUT} is stale; run make_cases.py", file=sys.stderr)
            return 1
        print("ok")
        return 0
    OUT.write_text(text, encoding="utf-8", newline="\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
