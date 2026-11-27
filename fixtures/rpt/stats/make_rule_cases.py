#!/usr/bin/env python3
"""Independent reference for RPT.STATS.SPC.RULES (run-rule sets and violation detection), stdlib only.

Writes (CC-BY-4.0 project data, invented inputs, D-038):
  fixtures/rpt/stats/rule_cases.json      evaluation cases, rule-set load cases, seeded 10 000 point digests
  fixtures/rpt/stats/spc_imr_30.json      a 30-point individuals series with one point above its UCL
  crates/fid-stats/src/spc_rules/*.json   the shipped rule sets, canonical JSON (REQ-004)

Nothing here is produced by running the Rust crate `fid-stats`.

Every evaluation and load case below carries a hand label worked out from the leaf's rules
(spec/rpt/stats/spc/rules/NODE.md and the conventions written down in
crates/fid-stats/src/rules.rs); the script re-derives each case with its own reading of the rules
and refuses to write if a hand label disagrees. The reading here is deliberately a different
algorithm from the kernel's: the kernel advances sliding counters once per point, this script
re-examines the whole window ending at each point.

Reading of the rules:
  sigma above = (UCL - centre) / 3, below = (centre - LCL) / 3; for zone z the thresholds are
  (UCL, LCL) when z == 3, else (centre + z * sigma_above, centre - z * sigma_below), each with
  plain IEEE 754 binary64 operations (Python floats round every +, -, *, / to nearest-even and
  never fuse). Every value is normalised with `+ 0.0`, so -0.0 equals +0.0 and the IEEE
  comparisons below are the same as totalOrder.
  above z: x > upper; below z: x < lower; beyond: either; within: lower <= x <= upper;
  above/below the centre: x > centre / x < centre; on the line otherwise.
  At point i:
    BEYOND{z}: x_i beyond z; range i..i.
    K_OF_M_BEYOND{k,m,z}: x_i above z (below z) and at least k of x_max(0,i-m+1)..x_i above z
      (below z); range from the k-th most recent such point.
    RUN_SAME_SIDE{len}: i >= len-1 and the len points ending at i all above, or all below, the centre.
    TREND{len}: the len points ending at i strictly increasing or strictly decreasing.
    ALTERNATING{len}: the len-1 differences of the len points ending at i all non-zero, alternating.
    RUN_WITHIN{len,z}: the len points ending at i all within z.
    RUN_OUTSIDE{len,z}: the len points ending at i all beyond z.
  Errors, in order: lines (non_finite_centre/ucl/lcl, ucl_not_above_centre, lcl_not_below_centre,
  span_not_finite, sigma_below_resolution with floor 16 * eps * max|line|), no points
  (insufficient_data), first non-finite point (non_finite:index), then per rule
  threshold_not_finite and zone_below_resolution.

Rule-set loading follows the order of checks documented in crates/fid-stats/src/rules_load.rs,
re-implemented here with a separate small JSON reader.

Seeded data (AC-6): x_j = (u_12j + ... + u_12j+11) - 6, the twelve fid-num Philox uniforms of
seed 11, stream 0 summed left to right from 0.0; lines centre 0, UCL 3, LCL -3.

Check: python fixtures/rpt/stats/make_rule_cases.py --check
"""

import decimal
import hashlib
import json
import struct
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
OUT = ROOT / "fixtures" / "rpt" / "stats" / "rule_cases.json"
OUT_IMR = ROOT / "fixtures" / "rpt" / "stats" / "spc_imr_30.json"
OUT_SETS = ROOT / "crates" / "fid-stats" / "src" / "spc_rules"

SCHEMA = "fid.stats.ruleset/1"
EPS = 2.0 ** -52
MAX_BYTES = 65536
MAX_DEPTH = 8
MAX_RULES = 64
MAX_WINDOW = 1000
MAX_Z = 1000.0
MAX_VERSION = 999999


def bits(x):
    return "%016x" % struct.unpack("<Q", struct.pack("<d", x))[0]


def from_bits(h):
    return struct.unpack("<d", struct.pack("<Q", int(h, 16)))[0]


# ---------------------------------------------------------------- shipped rule sets (REQ-004)
# Parameters only, written by the project after Western Electric (1956) and Nelson (1984)
# [unverified citation details and parameters]. Every set is UNVERIFIED.

def B(z):
    return {"primitive": "BEYOND", "z": z}


def KM(k, m, z):
    return {"primitive": "K_OF_M_BEYOND", "k": k, "m": m, "z": z}


def SS(n):
    return {"primitive": "RUN_SAME_SIDE", "len": n}


def TR(n):
    return {"primitive": "TREND", "len": n}


def AL(n):
    return {"primitive": "ALTERNATING", "len": n}


def RW(n, z):
    return {"primitive": "RUN_WITHIN", "len": n, "z": z}


def RO(n, z):
    return {"primitive": "RUN_OUTSIDE", "len": n, "z": z}


def make_set(id_, rules, version=1, review="UNVERIFIED"):
    return {"schema": SCHEMA, "id": id_, "version": version, "review": review,
            "rules": [dict(r, id=rid) for rid, r in rules]}


SHIPPED = [
    ("beyond_limits", make_set("beyond_limits", [("1", B(3))])),
    ("nelson", make_set("nelson", [("1", B(3)), ("2", SS(9)), ("3", TR(6)), ("4", AL(14)),
                                   ("5", KM(2, 3, 2)), ("6", KM(4, 5, 1)), ("7", RW(15, 1)), ("8", RO(8, 1))])),
    ("western_electric", make_set("western_electric", [("1", B(3)), ("2", KM(2, 3, 2)), ("3", KM(4, 5, 1)),
                                                       ("4", SS(8))])),
]


def canonical(obj):
    """Canonical JSON of a set dict: sorted keys, no whitespace; z printed like Rust's f64 Display."""
    def num(v):
        if isinstance(v, bool):
            raise ValueError("bool")
        if isinstance(v, int):
            return str(v)
        if v == int(v):
            return str(int(v))
        r = repr(v)
        if "e" in r or "E" in r:
            # Rust's f64 Display: the same shortest digits, written out without an exponent.
            r = format(decimal.Decimal(r), "f")
        return r

    def enc(v):
        if isinstance(v, dict):
            return "{" + ",".join(json.dumps(k) + ":" + enc(v[k]) for k in sorted(v)) + "}"
        if isinstance(v, list):
            return "[" + ",".join(enc(x) for x in v) + "]"
        if isinstance(v, str):
            return json.dumps(v)
        return num(v)
    return enc(obj)


# ---------------------------------------------------------------- loader (rules_load.rs order)

class Refused(Exception):
    def __init__(self, reason):
        super().__init__(reason)
        self.reason = reason


class Reader:
    def __init__(self, text):
        self.t = text
        self.i = 0

    def peek(self):
        return self.t[self.i] if self.i < len(self.t) else None

    def ws(self):
        while self.peek() in (" ", "\t", "\n", "\r"):
            self.i += 1

    def eat(self, c):
        if self.peek() != c:
            raise Refused("malformed")
        self.i += 1

    def value(self, depth):
        self.ws()
        c = self.peek()
        if c in ("{", "["):
            if depth >= MAX_DEPTH:
                raise Refused("too_deep")
            self.i += 1
            close = "}" if c == "{" else "]"
            out = []
            self.ws()
            if self.peek() == close:
                self.i += 1
                return ("obj", out) if c == "{" else ("arr", out)
            while True:
                if c == "{":
                    self.ws()
                    k = self.string()
                    self.ws()
                    self.eat(":")
                    out.append((k, self.value(depth + 1)))
                else:
                    out.append(self.value(depth + 1))
                self.ws()
                nxt = self.peek()
                if nxt == ",":
                    self.i += 1
                elif nxt == close:
                    self.i += 1
                    break
                else:
                    raise Refused("malformed")
            if c == "{":
                seen = []
                for k, _ in out:
                    if k in seen:
                        raise Refused("duplicate_key")
                    seen.append(k)
                return ("obj", out)
            return ("arr", out)
        if c == '"':
            return ("str", self.string())
        for w, v in (("true", ("bool", True)), ("false", ("bool", False)), ("null", ("null", None))):
            if self.t.startswith(w, self.i):
                self.i += len(w)
                return v
        if c is not None and (c == "-" or c.isdigit()):
            return ("num", self.number())
        raise Refused("malformed")

    def string(self):
        self.eat('"')
        start = self.i
        while True:
            c = self.peek()
            if c is None or c == "\\" or ord(c) < 0x20:
                raise Refused("malformed")
            if c == '"':
                break
            self.i += 1
        s = self.t[start:self.i]
        self.i += 1
        return s

    def number(self):
        start = self.i
        if self.peek() == "-":
            self.i += 1
        c = self.peek()
        if c == "0":
            self.i += 1
        elif c is not None and c in "123456789":
            while self.peek() is not None and self.peek() in "0123456789":
                self.i += 1
        else:
            raise Refused("malformed")
        if self.peek() == ".":
            self.i += 1
            n = self.i
            while self.peek() is not None and self.peek() in "0123456789":
                self.i += 1
            if self.i == n:
                raise Refused("malformed")
        if self.peek() in ("e", "E"):
            self.i += 1
            if self.peek() in ("+", "-"):
                self.i += 1
            n = self.i
            while self.peek() is not None and self.peek() in "0123456789":
                self.i += 1
            if self.i == n:
                raise Refused("malformed")
        return self.t[start:self.i]


def lookup(members, key):
    for k, v in members:
        if k == key:
            return v
    return None


def integer(v):
    if v is None or v[0] != "num":
        return None
    t = v[1]
    if not t or len(t) > 10 or not all(ch in "0123456789" for ch in t):
        return None
    if len(t) > 1 and t[0] == "0":
        return None
    return int(t)


def set_id_ok(s):
    return 1 <= len(s) <= 64 and "a" <= s[0] <= "z" and all(ch in "abcdefghijklmnopqrstuvwxyz0123456789_" for ch in s)


def rule_id_ok(s):
    return (1 <= len(s) <= 32 and s[0] in "abcdefghijklmnopqrstuvwxyz0123456789"
            and all(ch in "abcdefghijklmnopqrstuvwxyz0123456789_" for ch in s))


PARAMS = {"BEYOND": ["z"], "K_OF_M_BEYOND": ["k", "m", "z"], "RUN_SAME_SIDE": ["len"], "TREND": ["len"],
          "ALTERNATING": ["len"], "RUN_WITHIN": ["len", "z"], "RUN_OUTSIDE": ["len", "z"]}
LEN_MIN = {"RUN_SAME_SIDE": 2, "TREND": 3, "ALTERNATING": 3, "RUN_WITHIN": 2, "RUN_OUTSIDE": 2}


def load(raw):
    """A set dict {id, version, review, rules:[{id, primitive, params...}]} or Refused(reason)."""
    if len(raw) > MAX_BYTES:
        raise Refused("too_large")
    try:
        text = raw.decode("utf-8")
    except UnicodeDecodeError:
        raise Refused("malformed")
    r = Reader(text)
    root = r.value(0)
    r.ws()
    if r.i != len(text):
        raise Refused("malformed")
    if root[0] != "obj":
        raise Refused("wrong_type")
    m = root[1]
    allowed = ["schema", "id", "version", "review", "rules"]
    if any(k not in allowed for k, _ in m):
        raise Refused("unknown_key")
    if any(lookup(m, a) is None for a in allowed):
        raise Refused("missing_key")
    if lookup(m, "schema") != ("str", SCHEMA):
        raise Refused("bad_schema")
    v = lookup(m, "id")
    if v[0] != "str" or not set_id_ok(v[1]):
        raise Refused("bad_id")
    set_id = v[1]
    ver = integer(lookup(m, "version"))
    if ver is None or not 1 <= ver <= MAX_VERSION:
        raise Refused("bad_version")
    v = lookup(m, "review")
    if v not in (("str", "UNVERIFIED"), ("str", "EXPERT_REVIEWED"), ("str", "CLAIMED_BY_FILE")):
        raise Refused("bad_review")
    # D-045: any review a loaded file claims is CLAIMED_BY_FILE; only shipped sets may be EXPERT_REVIEWED
    # (none is today, so this reference never produces it).
    review = "UNVERIFIED" if v[1] == "UNVERIFIED" else "CLAIMED_BY_FILE"
    v = lookup(m, "rules")
    if v[0] != "arr":
        raise Refused("bad_rules")
    if not v[1]:
        raise Refused("no_rules")
    if len(v[1]) > MAX_RULES:
        raise Refused("too_many_rules")
    rules = []
    for item in v[1]:
        if item[0] != "obj":
            raise Refused("bad_rule")
        rm = item[1]
        p = lookup(rm, "primitive")
        if p is None:
            raise Refused("missing_key")
        if p[0] != "str":
            raise Refused("bad_rule")
        name = p[1]
        if name not in PARAMS:
            raise Refused("unknown_primitive")
        params = PARAMS[name]
        if any(k not in ["id", "primitive"] + params for k, _ in rm):
            raise Refused("unknown_key")
        if any(lookup(rm, k) is None for k in ["id"] + params):
            raise Refused("missing_key")
        rid = lookup(rm, "id")
        if rid[0] != "str" or not rule_id_ok(rid[1]):
            raise Refused("bad_rule_id")
        if any(x["id"] == rid[1] for x in rules):
            raise Refused("duplicate_rule_id")
        out = {"id": rid[1], "primitive": name}
        for key in params:
            pv = lookup(rm, key)
            if key == "z":
                if pv[0] != "num":
                    raise Refused("bad_parameter")
                z = float(pv[1])
                if not (z == z and abs(z) != float("inf") and 0.0 < z <= MAX_Z):
                    raise Refused("bad_parameter")
                out["z"] = z
            else:
                n = integer(pv)
                lo = {"k": 1, "m": out.get("k", 1), "len": LEN_MIN.get(name, 2)}[key]
                if n is None or not lo <= n <= MAX_WINDOW:
                    raise Refused("bad_parameter")
                out[key] = n
        rules.append(out)
    return {"id": set_id, "version": ver, "review": review, "rules": rules}


def load_token(raw):
    try:
        load(raw)
        return "ok"
    except Refused as e:
        return e.reason


# ---------------------------------------------------------------- evaluation (window reading)

def norm(x):
    return x + 0.0


def finite(x):
    return x == x and abs(x) != float("inf")


def zones(centre, ucl, lcl):
    if not finite(centre):
        raise Refused("stats.bad_lines:non_finite_centre")
    if not finite(ucl):
        raise Refused("stats.bad_lines:non_finite_ucl")
    if not finite(lcl):
        raise Refused("stats.bad_lines:non_finite_lcl")
    c, u, l = norm(centre), norm(ucl), norm(lcl)
    if not u > c:
        raise Refused("stats.bad_lines:ucl_not_above_centre")
    if not l < c:
        raise Refused("stats.bad_lines:lcl_not_below_centre")
    up, down = u - c, c - l
    if not finite(up) or not finite(down):
        raise Refused("stats.bad_lines:span_not_finite")
    floor = 16.0 * EPS * max(abs(c), abs(u), abs(l))
    if up < floor or down < floor:
        raise Refused("stats.bad_lines:sigma_below_resolution")
    return c, u, l, up / 3.0, down / 3.0, floor


def thresholds(zn, z):
    c, u, l, sa, sb, floor = zn
    above, below = z * sa, z * sb
    if z == 3.0:
        hi, lo = u, l
    else:
        hi, lo = norm(c + above), norm(c - below)
    if not finite(hi) or not finite(lo):
        raise Refused("stats.bad_lines:threshold_not_finite")
    if (z != 3.0 and (above < floor or below < floor)) or not hi > c or not lo < c:
        raise Refused("stats.bad_lines:zone_below_resolution")
    return hi, lo


def fires(rule, xs, i, zn, th):
    """The pattern's first index if `rule` fires at point i, else None (whole-window reading)."""
    p = rule["primitive"]
    c = zn[0]
    x = xs[i]
    if p == "BEYOND":
        hi, lo = th
        return i if (x > hi or x < lo) else None
    if p == "K_OF_M_BEYOND":
        hi, lo = th
        k, m = rule["k"], rule["m"]
        if x > hi:
            hit = [j for j in range(max(0, i - m + 1), i + 1) if xs[j] > hi]
        elif x < lo:
            hit = [j for j in range(max(0, i - m + 1), i + 1) if xs[j] < lo]
        else:
            return None
        return hit[-k] if len(hit) >= k else None
    n = rule["len"]
    if i < n - 1:
        return None
    w = xs[i - n + 1:i + 1]
    if p == "RUN_SAME_SIDE":
        ok = all(v > c for v in w) or all(v < c for v in w)
    elif p == "TREND":
        ok = all(w[t] < w[t + 1] for t in range(n - 1)) or all(w[t] > w[t + 1] for t in range(n - 1))
    elif p == "ALTERNATING":
        d = [(w[t + 1] > w[t]) - (w[t + 1] < w[t]) for t in range(n - 1)]
        ok = all(v != 0 for v in d) and all(d[t] == -d[t + 1] for t in range(n - 2))
    elif p == "RUN_WITHIN":
        hi, lo = th
        ok = all(lo <= v <= hi for v in w)
    elif p == "RUN_OUTSIDE":
        hi, lo = th
        ok = all(v > hi or v < lo for v in w)
    else:
        raise AssertionError(p)
    return i - n + 1 if ok else None


def min_points(rule):
    return {"BEYOND": 1}.get(rule["primitive"], rule.get("k", rule.get("len")))


def evaluate(points, lines, rs):
    """{"violations": [[i, rule_id, from, to]], "rules": [[rule_id, status, count, upper, lower]]} or error."""
    try:
        zn = zones(*lines)
        if not points:
            raise Refused("stats.insufficient_data")
        for idx, x in enumerate(points):
            if not finite(x):
                raise Refused("stats.non_finite:%d" % idx)
        xs = [norm(x) for x in points]
        ths = [thresholds(zn, r["z"]) if "z" in r else None for r in rs["rules"]]
    except Refused as e:
        return e.reason
    viol, counts = [], [0] * len(rs["rules"])
    for i in range(len(xs)):
        for pos, r in enumerate(rs["rules"]):
            f = fires(r, xs, i, zn, ths[pos])
            if f is not None:
                viol.append([i, r["id"], f, i])
                counts[pos] += 1
    rules = []
    for pos, r in enumerate(rs["rules"]):
        need = min_points(r)
        status = "evaluated" if len(xs) >= need else "too_few_points:%d" % need
        th = ths[pos]
        rules.append([r["id"], status, counts[pos], bits(th[0]) if th else None, bits(th[1]) if th else None])
    return {"violations": viol, "rules": rules}


def block(id_, res):
    if isinstance(res, str):
        return "%s error:%s\n" % (id_, res)
    s = "%s %d\n" % (id_, len(res["violations"]))
    for v in res["violations"]:
        s += "v %d %s %d %d\n" % tuple(v)
    for r in res["rules"]:
        s += "r %s %s %d %s %s\n" % (r[0], r[1], r[2], r[3] or "-", r[4] or "-")
    return s


# ---------------------------------------------------------------- hand-labelled cases

def one(prim):
    return canonical(make_set("case", [("r", prim)]))


L1 = (0.0, 3.0, -3.0)              # sigma 1 on both sides: zone 1 = +-1, zone 2 = +-2
LR = (2.0, 8.0, 0.0)               # R chart, LCL clamped to 0: sigma above 2, below 2/3
NAN = float("nan")
INF = float("inf")
NEXT2 = 2.0000000000000004         # the double after 2

# (id, note, lines, points, set text, hand label: list of violations or an error token)
EVAL = [
    # AC-1: each primitive fires exactly once on its hit sequence, never on its near miss.
    ("beyond_hit", "AC-1", L1, [0, 1, 3.5, 0], one(B(3)), [[2, "r", 2, 2]]),
    ("beyond_miss", "AC-1/AC-2: exactly at UCL and LCL", L1, [0, 3, -3, 2.9], one(B(3)), []),
    ("kofm_hit", "AC-1", L1, [0, 2.5, 0, 2.1, 0], one(KM(2, 3, 2)), [[3, "r", 1, 3]]),
    ("kofm_miss", "AC-1: the two points span 4", L1, [0, 2.5, 0, 0, 2.1], one(KM(2, 3, 2)), []),
    ("kofm_opposite_sides", "same side required", L1, [0, 2.5, -2.5, 0], one(KM(2, 3, 2)), []),
    ("kofm_start", "fewer than m points at the start", L1, [-2.5, -2.2, 0], one(KM(2, 3, 2)), [[1, "r", 0, 1]]),
    ("same_side_hit", "AC-1", L1, [-1, 1, 0.5, 2, 0.1, -1], one(SS(4)), [[4, "r", 1, 4]]),
    ("same_side_miss", "AC-1", L1, [-1, 1, 0.5, 2, -0.1], one(SS(4)), []),
    ("trend_hit", "AC-1", L1, [0, -1, -0.5, 0, 0.5, 0.2], one(TR(4)), [[4, "r", 1, 4]]),
    ("trend_miss", "AC-1", L1, [0, -1, -0.5, 0, -0.2], one(TR(4)), []),
    ("alternating_hit", "AC-1", L1, [0, 1, 0, 1, 0, -1], one(AL(5)), [[4, "r", 0, 4]]),
    ("alternating_miss", "AC-1", L1, [0, 1, 0, 1, 2], one(AL(5)), []),
    ("within_hit", "AC-1/AC-2: +-1 exactly is within", L1, [2, 0.5, -0.5, 1, -1, 1.5], one(RW(4, 1)),
     [[4, "r", 1, 4]]),
    ("within_miss", "AC-1", L1, [2, 0.5, -0.5, 1, 1.01], one(RW(4, 1)), []),
    ("outside_hit", "AC-1", L1, [0, 1.5, -2, 1.1, 0], one(RO(3, 1)), [[3, "r", 1, 3]]),
    ("outside_miss", "AC-1/AC-2: 1 exactly is not beyond", L1, [0, 1.5, -2, 1, 0], one(RO(3, 1)), []),
    # AC-2: boundaries.
    ("centre_breaks_run", "AC-2", L1, [1, 1, 0, 1, 1], one(SS(3)), []),
    ("neg_zero_on_centre", "AC-2: -0.0 sits on a +0.0 centre", L1, [1, 1, -0.0, 1, 1], one(SS(3)), []),
    ("at_z_not_beyond", "AC-2", L1, [2, -2, NEXT2, -NEXT2], one(B(2)), [[2, "r", 2, 2], [3, "r", 3, 3]]),
    ("kofm_at_z", "AC-2", L1, [2, 2, 2], one(KM(2, 3, 2)), []),
    ("equal_breaks_trend", "AC-2", L1, [0, 1, 1, 2], one(TR(3)), []),
    ("equal_breaks_alternation", "AC-2", L1, [0, 1, 1, 0], one(AL(3)), []),
    ("run_continues", "a run of 5 fires at the 4th and 5th point", L1, [1, 1, 1, 1, 1], one(SS(4)),
     [[3, "r", 0, 3], [4, "r", 1, 4]]),
    # AC-3: asymmetric limits (R chart, LCL clamped to 0).
    ("r_chart_beyond3", "AC-3: only below 0 or above UCL", LR, [0.0, 8.0, 0.5, 8.5, -1e-9, 2.0], one(B(3)),
     [[3, "r", 3, 3], [4, "r", 4, 4]]),
    ("r_chart_beyond2", "AC-3: lower zone 2 - 2*(2/3)", LR, [0.6, 0.7, 6.0, 6.1], one(B(2)),
     [[0, "r", 0, 0], [3, "r", 3, 3]]),
    # Ordering (REQ-006) and several rules per point (REQ-005).
    ("order_by_index_then_rule", "REQ-005/006", L1, [3.5, 3.6],
     canonical(make_set("multi", [("b", B(3)), ("a", KM(2, 2, 2))])),
     [[0, "b", 0, 0], [1, "b", 1, 1], [1, "a", 0, 1]]),
    ("too_few_points", "the rule cannot fire on 5 points", L1, [1, 1, 1, 1, 1], one(SS(9)), []),
    # Refusals, in the documented order.
    ("ucl_equals_centre", "lines", (0.0, 0.0, -3.0), [0], one(B(3)), "stats.bad_lines:ucl_not_above_centre"),
    ("lcl_equals_centre", "lines", (0.0, 3.0, 0.0), [0], one(B(3)), "stats.bad_lines:lcl_not_below_centre"),
    ("nan_centre", "lines", (NAN, 3.0, -3.0), [0], one(B(3)), "stats.bad_lines:non_finite_centre"),
    ("inf_ucl", "lines", (0.0, INF, -3.0), [0], one(B(3)), "stats.bad_lines:non_finite_ucl"),
    ("nan_lcl", "lines", (0.0, 3.0, NAN), [0], one(B(3)), "stats.bad_lines:non_finite_lcl"),
    ("span_overflow", "lines", (-1e308, 1.7e308, -1.5e308), [0], one(B(3)), "stats.bad_lines:span_not_finite"),
    ("sigma_floor", "lines: half-width 1e-9 at 1e6", (1e6, 1e6 + 1e-9, 1e6 - 1.0), [1e6], one(B(3)),
     "stats.bad_lines:sigma_below_resolution"),
    ("lines_before_points", "bad lines and no points", (0.0, 0.0, -3.0), [], one(B(3)),
     "stats.bad_lines:ucl_not_above_centre"),
    ("no_points", "points", L1, [], one(B(3)), "stats.insufficient_data"),
    ("nan_point", "points", L1, [0, 1, NAN, INF], one(B(3)), "stats.non_finite:2"),
    ("inf_point", "points", L1, [0, -INF], one(B(3)), "stats.non_finite:1"),
    ("zone_underflow", "z sigma rounds onto the centre", (0.0, 1e-300, -1e-300), [0], one(B(1e-30)),
     "stats.bad_lines:zone_below_resolution"),
    ("zone_floor", "z * sigma of 2e-10 at 1e6 is 2 ulp wide, below the 16 eps floor", (1e6, 1e6 + 3.0, 1e6 - 3.0),
     [1e6], one(B(2e-10)), "stats.bad_lines:zone_below_resolution"),
    ("zone_floor_ok", "z * sigma of 1e-8 at 1e6 is above the floor", (1e6, 1e6 + 3.0, 1e6 - 3.0),
     [1e6, 1e6 + 2e-8], one(B(1e-8)), [[1, "r", 1, 1]]),
    ("threshold_overflow", "1000 sigma overflows", (0.0, 1e308, -1e308), [0], one(B(1000)),
     "stats.bad_lines:threshold_not_finite"),
]

VALID = canonical(make_set("x", [("1", B(3))]))


def replace_rule(rule_text):
    return ('{"id":"x","review":"UNVERIFIED","rules":[' + rule_text + '],"schema":"fid.stats.ruleset/1",'
            '"version":1}')


def replace_root(**kv):
    base = {"schema": '"fid.stats.ruleset/1"', "id": '"x"', "version": "1", "review": '"UNVERIFIED"',
            "rules": '[{"id":"1","primitive":"BEYOND","z":3}]'}
    base.update(kv)
    return "{" + ",".join('"%s":%s' % (k, v) for k, v in base.items() if v is not None) + "}"


PRETTY = """{
  "schema": "fid.stats.ruleset/1",
  "id": "pretty",
  "version": 2,
  "review": "UNVERIFIED",
  "rules": [
    {"primitive": "K_OF_M_BEYOND", "id": "k", "k": 2, "m": 3, "z": 2.0},
    {"id": "w", "primitive": "RUN_WITHIN", "len": 15, "z": 1e0},
    {"id": "h", "primitive": "BEYOND", "z": 2.5}
  ]
}
"""
PRETTY_CANON = ('{"id":"pretty","review":"UNVERIFIED","rules":[{"id":"k","k":2,"m":3,"primitive":"K_OF_M_BEYOND","z":2},'
                '{"id":"w","len":15,"primitive":"RUN_WITHIN","z":1},{"id":"h","primitive":"BEYOND","z":2.5}],'
                '"schema":"fid.stats.ruleset/1","version":2}')

# (id, text, hand label: "ok" or a reason, canonical text for ok cases or None, pad_to bytes or None)
LOAD = [
    ("magic", replace_rule('{"id":"1","primitive":"MAGIC","z":3}'), "unknown_primitive", None, None),
    ("valid", VALID, "ok", VALID, None),
    ("pretty", PRETTY, "ok", PRETTY_CANON, None),
    ("z_max", replace_rule('{"id":"1","primitive":"BEYOND","z":1000}'), "ok",
     replace_rule('{"id":"1","primitive":"BEYOND","z":1000}'), None),
    ("reviewed_claim", replace_root(review='"EXPERT_REVIEWED"'), "ok",
     '{"id":"x","review":"CLAIMED_BY_FILE","rules":[{"id":"1","primitive":"BEYOND","z":3}],'
     '"schema":"fid.stats.ruleset/1","version":1}', None),
    ("claimed_by_file", replace_root(review='"CLAIMED_BY_FILE"'), "ok",
     '{"id":"x","review":"CLAIMED_BY_FILE","rules":[{"id":"1","primitive":"BEYOND","z":3}],'
     '"schema":"fid.stats.ruleset/1","version":1}', None),
    ("max_rules", replace_root(rules="[" + ",".join('{"id":"r%d","primitive":"BEYOND","z":3}' % n
                                                   for n in range(64)) + "]"), "ok", None, None),
    ("max_size", VALID, "ok", VALID, MAX_BYTES),
    ("too_large", VALID, "too_large", None, MAX_BYTES + 1),
    ("empty", "", "malformed", None, None),
    ("open_brace", "{", "malformed", None, None),
    ("trailing", VALID + "x", "malformed", None, None),
    ("two_values", VALID + VALID, "malformed", None, None),
    ("nan_literal", replace_rule('{"id":"1","primitive":"BEYOND","z":NaN}'), "malformed", None, None),
    ("infinity_literal", replace_rule('{"id":"1","primitive":"BEYOND","z":Infinity}'), "malformed", None, None),
    ("escape", replace_root(id='"a\\u0062"'), "malformed", None, None),
    ("control_char", replace_root(id='"a\tb"'), "malformed", None, None),
    ("leading_zero", replace_rule('{"id":"1","primitive":"BEYOND","z":03}'), "malformed", None, None),
    ("bare_dot", replace_rule('{"id":"1","primitive":"BEYOND","z":3.}'), "malformed", None, None),
    ("plus_sign", replace_rule('{"id":"1","primitive":"BEYOND","z":+3}'), "malformed", None, None),
    ("single_quotes", "{'id':'x'}", "malformed", None, None),
    ("trailing_comma", replace_rule('{"id":"1","primitive":"BEYOND","z":3},'), "malformed", None, None),
    ("too_deep", '{"id":' + "[" * 9 + "]" * 9 + "}", "too_deep", None, None),
    ("depth_ok_then_dup", '{"a":[[[[[[[]]]]]]],"a":1}', "duplicate_key", None, None),
    ("dup_root_key", '{"id":"x","id":"y"}', "duplicate_key", None, None),
    ("dup_rule_key", replace_rule('{"id":"1","primitive":"BEYOND","z":3,"z":2}'), "duplicate_key", None, None),
    ("dup_before_syntax_error", '{"r":{"a":1,"a":2},', "duplicate_key", None, None),
    ("root_array", "[]", "wrong_type", None, None),
    ("root_string", '"x"', "wrong_type", None, None),
    ("unknown_root_key", replace_root(extra="1"), "unknown_key", None, None),
    ("unknown_before_missing", replace_root(extra="1", version=None), "unknown_key", None, None),
    ("missing_version", replace_root(version=None), "missing_key", None, None),
    ("missing_schema", replace_root(schema=None), "missing_key", None, None),
    ("bad_schema", replace_root(schema='"fid.stats.ruleset/2"'), "bad_schema", None, None),
    ("schema_not_string", replace_root(schema="1"), "bad_schema", None, None),
    ("schema_before_id", replace_root(schema='"x"', id='"X"'), "bad_schema", None, None),
    ("upper_id", replace_root(id='"Nelson"'), "bad_id", None, None),
    ("digit_first_id", replace_root(id='"1x"'), "bad_id", None, None),
    ("long_id", replace_root(id='"' + "a" * 65 + '"'), "bad_id", None, None),
    ("version_zero", replace_root(version="0"), "bad_version", None, None),
    ("version_fraction", replace_root(version="1.0"), "bad_version", None, None),
    ("version_string", replace_root(version='"1"'), "bad_version", None, None),
    ("version_large", replace_root(version="1000000"), "bad_version", None, None),
    ("version_negative", replace_root(version="-1"), "bad_version", None, None),
    ("bad_review", replace_root(review='"REVIEWED"'), "bad_review", None, None),
    ("rules_object", replace_root(rules="{}"), "bad_rules", None, None),
    ("rules_empty", replace_root(rules="[]"), "no_rules", None, None),
    ("rules_65", replace_root(rules="[" + ",".join('{"id":"r%d","primitive":"BEYOND","z":3}' % n
                                                  for n in range(65)) + "]"), "too_many_rules", None, None),
    ("rule_number", replace_rule("3"), "bad_rule", None, None),
    ("rule_no_primitive", replace_rule('{"id":"1","z":3}'), "missing_key", None, None),
    ("primitive_number", replace_rule('{"id":"1","primitive":7,"z":3}'), "bad_rule", None, None),
    ("primitive_lower", replace_rule('{"id":"1","primitive":"beyond","z":3}'), "unknown_primitive", None, None),
    ("magic_before_keys", replace_rule('{"primitive":"MAGIC"}'), "unknown_primitive", None, None),
    ("rule_unknown_key", replace_rule('{"id":"1","primitive":"BEYOND","z":3,"zz":1}'), "unknown_key", None, None),
    ("param_of_other", replace_rule('{"id":"1","len":3,"primitive":"BEYOND","z":3}'), "unknown_key", None, None),
    ("missing_z", replace_rule('{"id":"1","primitive":"BEYOND"}'), "missing_key", None, None),
    ("missing_rule_id", replace_rule('{"primitive":"BEYOND","z":3}'), "missing_key", None, None),
    ("empty_rule_id", replace_rule('{"id":"","primitive":"BEYOND","z":3}'), "bad_rule_id", None, None),
    ("upper_rule_id", replace_rule('{"id":"A","primitive":"BEYOND","z":3}'), "bad_rule_id", None, None),
    ("rule_id_number", replace_rule('{"id":1,"primitive":"BEYOND","z":3}'), "bad_rule_id", None, None),
    ("dup_rule_id", replace_rule('{"id":"1","primitive":"BEYOND","z":3},{"id":"1","primitive":"BEYOND","z":2}'),
     "duplicate_rule_id", None, None),
    ("k_zero", replace_rule('{"id":"1","k":0,"m":3,"primitive":"K_OF_M_BEYOND","z":2}'), "bad_parameter", None, None),
    ("m_below_k", replace_rule('{"id":"1","k":3,"m":2,"primitive":"K_OF_M_BEYOND","z":2}'), "bad_parameter",
     None, None),
    ("m_too_big", replace_rule('{"id":"1","k":3,"m":1001,"primitive":"K_OF_M_BEYOND","z":2}'), "bad_parameter",
     None, None),
    ("z_zero", replace_rule('{"id":"1","primitive":"BEYOND","z":0}'), "bad_parameter", None, None),
    ("z_neg_zero", replace_rule('{"id":"1","primitive":"BEYOND","z":-0}'), "bad_parameter", None, None),
    ("z_negative", replace_rule('{"id":"1","primitive":"BEYOND","z":-1}'), "bad_parameter", None, None),
    ("z_overflow", replace_rule('{"id":"1","primitive":"BEYOND","z":1e999}'), "bad_parameter", None, None),
    ("z_above_max", replace_rule('{"id":"1","primitive":"BEYOND","z":1000.0000001}'), "bad_parameter", None, None),
    ("z_string", replace_rule('{"id":"1","primitive":"BEYOND","z":"3"}'), "bad_parameter", None, None),
    ("z_null", replace_rule('{"id":"1","primitive":"BEYOND","z":null}'), "bad_parameter", None, None),
    ("trend_len_2", replace_rule('{"id":"1","len":2,"primitive":"TREND"}'), "bad_parameter", None, None),
    ("alternating_len_2", replace_rule('{"id":"1","len":2,"primitive":"ALTERNATING"}'), "bad_parameter", None, None),
    ("run_len_1", replace_rule('{"id":"1","len":1,"primitive":"RUN_SAME_SIDE"}'), "bad_parameter", None, None),
    ("len_fraction", replace_rule('{"id":"1","len":3.0,"primitive":"RUN_SAME_SIDE"}'), "bad_parameter", None, None),
    ("len_exponent", replace_rule('{"id":"1","len":3e0,"primitive":"RUN_SAME_SIDE"}'), "bad_parameter", None, None),
    ("len_too_big", replace_rule('{"id":"1","len":1001,"primitive":"RUN_OUTSIDE","z":1}'), "bad_parameter",
     None, None),
    ("len_huge", replace_rule('{"id":"1","len":99999999999,"primitive":"RUN_OUTSIDE","z":1}'), "bad_parameter",
     None, None),
    ("within_z_bad", replace_rule('{"id":"1","len":3,"primitive":"RUN_WITHIN","z":true}'), "bad_parameter",
     None, None),
]


def padded(text, pad_to):
    raw = text.encode("utf-8")
    if pad_to is not None:
        assert pad_to >= len(raw)
        raw = raw + b" " * (pad_to - len(raw))
    return raw


# ---------------------------------------------------------------- AC-4 series

IMR_VALUES = ["10.012", "9.992", "10.005", "10.000", "9.989", "10.007", "10.003", "9.996", "10.010", "9.994",
              "10.002", "9.991", "10.006", "10.001", "9.997", "10.008", "9.995", "10.300", "10.004", "9.993",
              "10.009", "9.998", "10.005", "9.990", "10.003", "10.000", "9.994", "10.007", "9.999", "10.004"]
E2_TEXT = "2.660"          # spc.constants.normal@1.0.0, n = 2 row (UNVERIFIED)


def imr_doc():
    xs = [float(t) for t in IMR_VALUES]
    total = 0.0
    for x in xs:
        total = total + x
    mean = total / len(xs)
    mr_total = 0.0
    for a, b in zip(xs, xs[1:]):
        mr_total = mr_total + abs(b - a)
    mr_bar = mr_total / (len(xs) - 1)
    e2 = float(E2_TEXT)
    ucl = mean + e2 * mr_bar
    lcl = mean - e2 * mr_bar
    above = [i for i, x in enumerate(xs) if x > ucl]
    below = [i for i, x in enumerate(xs) if x < lcl]
    assert above == [17] and below == [], (above, below, ucl, lcl)
    return {
        "fixture": "RPT.STATS.SPC individuals (I-MR) series, 30 points, one point above its UCL",
        "licence": "CC-BY-4.0",
        "generator": "fixtures/rpt/stats/make_rule_cases.py",
        "note": ("Invented values. The lines are the individuals-chart limits mean +- E2 * mean moving range "
                 "with E2 = 2.660 from spc.constants.normal@1.0.0 (UNVERIFIED), sums left to right; "
                 "RPT.STATS.SPC.LIM owns the limit calculation and may extend this file."),
        "values": IMR_VALUES,
        "value_bits": [bits(x) for x in xs],
        "lines": {"centre": bits(mean), "ucl": bits(ucl), "lcl": bits(lcl), "e2": E2_TEXT,
                  "mean_moving_range": bits(mr_bar)},
        "expect_beyond_limits_v1": [[17, "1", 17, 17]],
    }, xs, (mean, ucl, lcl)


# ---------------------------------------------------------------- Philox (fid-num RNG, REQ-KRN.NUM.RNG)

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
    return (c0, c1, c2, c3)


for _ctr, _key, _want in [
    ((0, 0, 0, 0), (0, 0), (0x6627E8D5, 0xE169C58D, 0xBC57AC4C, 0x9B00DBD8)),
    ((MASK,) * 4, (MASK, MASK), (0x408F276D, 0x41C83B0E, 0xA20BC7C6, 0x6D5451FD)),
]:
    assert philox(_ctr, _key) == _want, "Philox4x32-10 known answer"


def uniform(seed, stream, index):
    w = philox((index & MASK, index >> 32, stream & MASK, stream >> 32), (seed & MASK, seed >> 32))
    return (((w[1] << 32) | w[0]) >> 11) * 2.0 ** -53


RANDOM = {"seed": 11, "stream": 0, "n": 10000, "draws_per_point": 12}


def random_points():
    out = []
    for j in range(RANDOM["n"]):
        s = 0.0
        for t in range(RANDOM["draws_per_point"]):
            s = s + uniform(RANDOM["seed"], RANDOM["stream"], RANDOM["draws_per_point"] * j + t)
        out.append(s - 6.0)
    return out


# ---------------------------------------------------------------- build

def build():
    files = {}
    shipped_sets = {}
    for name, obj in SHIPPED:
        text = canonical(obj)
        assert load_token(text.encode()) == "ok", name
        files[OUT_SETS / ("%s_v1.json" % name)] = text
        shipped_sets[name] = load(text.encode())

    ev_cases, ev_text = [], []
    ids = set()
    for id_, note, lines, points, set_text, label in EVAL:
        assert id_ not in ids, id_
        ids.add(id_)
        rs = load(set_text.encode())
        pts = [float(p) for p in points]
        got = evaluate(pts, tuple(float(v) for v in lines), rs)
        if isinstance(label, str):
            assert got == label, (id_, got, label)
        else:
            assert not isinstance(got, str), (id_, got)
            assert got["violations"] == label, (id_, got["violations"], label)
        out = {"id": id_, "note": note, "lines": [bits(float(v)) for v in lines],
               "points": [bits(p) for p in pts], "ruleset": set_text}
        out["expect"] = {"error": got} if isinstance(got, str) else got
        ev_cases.append(out)
        ev_text.append(block(id_, got))

    # The too_few_points case: the rule's status says why it raised nothing.
    tfp = [c for c in ev_cases if c["id"] == "too_few_points"][0]
    assert tfp["expect"]["rules"][0][1] == "too_few_points:9", tfp

    ld_cases = []
    for id_, text, label, canon, pad_to in LOAD:
        raw = padded(text, pad_to)
        got = load_token(raw)
        assert got == label, (id_, got, label)
        c = {"id": id_, "text": text, "expect": got}
        if pad_to is not None:
            c["pad_to"] = pad_to
        if got == "ok":
            rs = load(raw)
            mine = canonical({"schema": SCHEMA, "id": rs["id"], "version": rs["version"], "review": rs["review"],
                              "rules": rs["rules"]})
            if canon is not None:
                assert mine == canon, (id_, mine, canon)
            c["canonical"] = mine
        ld_cases.append(c)

    imr, imr_xs, (mean, ucl, lcl) = imr_doc()
    got = evaluate(imr_xs, (mean, ucl, lcl), shipped_sets["beyond_limits"])
    assert got["violations"] == imr["expect_beyond_limits_v1"], got
    files[OUT_IMR] = json.dumps(imr, indent=1, ensure_ascii=True) + "\n"

    pts = random_points()
    rnd_sets = []
    for name, _ in SHIPPED:
        res = evaluate(pts, (0.0, 3.0, -3.0), shipped_sets[name])
        text = block(name + "@1", res)
        rnd_sets.append({"set": name, "version": 1, "violations": len(res["violations"]),
                         "per_rule": [[r[0], r[2]] for r in res["rules"]],
                         "first": res["violations"][:5],
                         "digest": hashlib.sha256(text.encode("ascii")).hexdigest()})
    assert all(s["violations"] > 0 for s in rnd_sets), rnd_sets

    doc = {
        "fixture": "RPT.STATS.SPC.RULES run-rule cases",
        "licence": "CC-BY-4.0",
        "generator": "fixtures/rpt/stats/make_rule_cases.py",
        "eval_cases": ev_cases,
        "eval_digest": hashlib.sha256("".join(ev_text).encode("ascii")).hexdigest(),
        "load_cases": ld_cases,
        "random_10k": {
            "seed": RANDOM["seed"], "stream": RANDOM["stream"], "n": RANDOM["n"],
            "draws_per_point": RANDOM["draws_per_point"],
            "transform": "x_j = (sum of uniform_f64 at indices 12j..12j+11, left to right from 0.0) - 6.0",
            "lines": [bits(0.0), bits(3.0), bits(-3.0)],
            "first_points": [bits(p) for p in pts[:4]],
            "sets": rnd_sets,
        },
    }
    files[OUT] = json.dumps(doc, indent=1, ensure_ascii=True) + "\n"
    return files


def main():
    files = build()
    if "--check" in sys.argv[1:]:
        stale = [p for p, text in files.items()
                 if not p.exists() or p.read_bytes() != text.encode("utf-8")]
        for p in stale:
            print(f"{p} is out of date; run python fixtures/rpt/stats/make_rule_cases.py")
        if stale:
            return 1
        print("rule cases, spc_imr_30 and shipped rule sets up to date")
        return 0
    for p, text in files.items():
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_bytes(text.encode("utf-8"))
        print(f"wrote {p}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
