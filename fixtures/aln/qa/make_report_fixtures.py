#!/usr/bin/env python3
"""Independent reference for ALN.QA.REPORT (CC-BY-4.0 project data).

Writes, next to this script:

- report-cases.json: hand-authored residual lists with their expected statistics, stats
  JSON and report digest, plus the expected refusals (AC-1, AC-3);
- report-1m.txt: the expected statistics and digest of the seeded 1,000,000-entry report
  (seed 13; AC-2, AC-5);
- get-residuals-a2-summary.json and get-residuals-a2-page.json: the expected bytes of
  `align.get_residuals@1` on alignment A2 of fixtures/aln/model/chain3 when its report is
  the case `mixed_kinds_weights` (AC-4).

Every expected value is computed here from the rules pinned in spec/aln/qa/report/NODE.md
(Design), never by running the Rust code:

- hand cases use values on a binary grid (k * 2^-e metres) so every square, weighted square
  and partial sum is exact; the script checks this and computes the sums with Fraction. The
  division is the correctly rounded quotient (Fraction -> float) and the square root is
  computed exactly with integers and rounded to nearest, ties to even;
- the 1,000,000-entry case follows the KRN.NUM.RED summation rule as its spec text states
  it (blocks of 256 in index order, Neumaier compensation from (-0.0, -0.0), a fixed binary
  tree over blocks, s + c unless c == 0) in Python binary64 arithmetic, and is cross-checked
  against math.fsum (correctly rounded) to 2^-50 relative;
- the digest is SHA-256 over "FIDRES" | 0x01 | stats_len u64 | stats JSON | SHA-256 of the
  columns layout, every integer little-endian, as pinned in the node;
- command output is RFC 8785 canonical JSON; numbers are written as ECMAScript
  Number::toString (implemented below from the shortest round-trip digits of repr()).

Stdlib only. Run:  python fixtures/aln/qa/make_report_fixtures.py [--check]
"""

import hashlib
import json
import math
import struct
import sys
from decimal import Decimal
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent
CHAIN3 = HERE.parent / "model" / "chain3"
MASK = (1 << 64) - 1
KINDS = ["point", "feature", "pair", "locked_direction"]
KIND_BYTE = {k: i + 1 for i, k in enumerate(KINDS)}
BLOCK = 256
N_BIG = 1_000_000
SEED = 13


# ------------------------------------------------------------------ numbers


def bits(x):
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def hexf(x):
    return "0x%016x" % bits(x)


def plus_zero(x):
    return 0.0 if x == 0.0 else x


def sqrt_rn(q):
    """Correctly rounded (nearest, ties to even) square root of a non-negative normal float."""
    f = Fraction(q)
    if f == 0:
        return 0.0
    assert f > 0
    # X = q * 4^t is an integer of at least 120 bits.
    t = 0
    while (f * 4**t).denominator != 1 or f * 4**t < 2**120:
        t += 1
    x = int(f * 4**t)
    s = math.isqrt(x)
    sticky = s * s != x
    shift = s.bit_length() - 53
    assert shift >= 2
    top = s >> shift
    rem = s & ((1 << shift) - 1)
    half = 1 << (shift - 1)
    if rem > half or (rem == half and (sticky or top & 1)):
        top += 1
    return math.ldexp(top, shift - t)


def es_number(x):
    """ECMAScript Number::toString of a finite float (RFC 8785 number form)."""
    if x == 0:
        return "0"
    if x < 0:
        return "-" + es_number(-x)
    sign, digits, exp = Decimal(repr(x)).normalize().as_tuple()
    d = "".join(map(str, digits))
    k = len(d)
    n = exp + k
    if k <= n <= 21:
        return d + "0" * (n - k)
    if 0 < n <= 21:
        return d[:n] + "." + d[n:]
    if -6 < n <= 0:
        return "0." + "0" * (-n) + d
    e = n - 1
    return d[0] + ("." + d[1:] if k > 1 else "") + "e" + ("+" if e >= 0 else "-") + str(abs(e))


def canon(v):
    """RFC 8785 for the values used here (ASCII keys and strings)."""
    if v is None:
        return "null"
    if v is True:
        return "true"
    if v is False:
        return "false"
    if isinstance(v, int):
        assert abs(v) < 2**53
        return str(v)
    if isinstance(v, float):
        return es_number(v)
    if isinstance(v, str):
        return json.dumps(v)
    if isinstance(v, list):
        return "[" + ",".join(canon(x) for x in v) + "]"
    if isinstance(v, dict):
        return "{" + ",".join(json.dumps(k) + ":" + canon(v[k]) for k in sorted(v)) + "}"
    raise TypeError(v)


# ------------------------------------------------------------------ fixed-order rule


def fixed_sum(terms):
    """KRN.NUM.RED summation rule over a list of finite floats."""
    n = len(terms)
    if n == 0:
        return 0.0

    def block(lo, hi):
        s, c = -0.0, -0.0
        for x in terms[lo:hi]:
            t = s + x
            if abs(s) >= abs(x):
                c = c + ((s - t) + x)
            else:
                c = c + ((x - t) + s)
            s = t
        return s, c

    def tree(first, count):
        if count == 1:
            lo = first * BLOCK
            return block(lo, min(n, lo + BLOCK))
        left = count // 2
        a = tree(first, left)
        b = tree(first + left, count - left)
        s = a[0] + b[0]
        e = (a[0] - s) + b[0] if abs(a[0]) >= abs(b[0]) else (b[0] - s) + a[0]
        return s, (a[1] + b[1]) + e

    s, c = tree(0, (n + BLOCK - 1) // BLOCK)
    assert math.isfinite(s) and math.isfinite(c)
    r = s if c == 0 else s + c
    assert math.isfinite(r)
    return r


def exact_sum(terms):
    """Exact sum, asserting that every partial sum in any order is representable."""
    fr = [Fraction(t) for t in terms]
    if not fr:
        return 0.0

    def exponent(f):
        # f = m * 2^e with m odd (f != 0); denominators of floats are powers of two.
        n, d = abs(f.numerator), f.denominator
        z = (n & -n).bit_length() - 1
        return z - (d.bit_length() - 1)

    g = min((exponent(f) for f in fr if f), default=0)
    assert sum(abs(f) for f in fr) / Fraction(2) ** g < 2**53, "partial sums would round"
    total = sum(fr)
    assert Fraction(float(total)) == total
    return float(total)


# ------------------------------------------------------------------ report model


class Report:
    def __init__(self, entries):
        # entry: dict(input, item, kind, residual, vector, weight, used, reason)
        self.entries = entries
        self.n = len(entries)
        self.weighted = bool(entries) and entries[0]["weight"] is not None

    def stats(self, exact):
        used = [e for e in self.entries if e["used"]]
        st = {"n_used": len(used), "n_rejected": self.n - len(used)}
        st.update({"rms": None, "weighted_rms": None, "max_abs": None, "max_entry": None})
        if not used:
            return st
        r = [e["residual"] for e in used]
        q = [x * x for x in r]
        assert all(math.isfinite(x) for x in q)
        ssum = exact_sum(q) if exact else fixed_sum(q)
        if not exact:
            ref = math.fsum(q)
            assert abs(ssum - ref) <= abs(ref) * 2.0**-50
        st["rms"] = sqrt_rn(float(Fraction(ssum) / len(used)))
        if self.weighted:
            w = [e["weight"] for e in used]
            wq = [a * b for a, b in zip(w, q)]
            if exact:
                assert all(Fraction(a) * Fraction(b) == Fraction(p) for a, b, p in zip(w, q, wq))
            num = exact_sum(wq) if exact else fixed_sum(wq)
            den = exact_sum(w) if exact else fixed_sum(w)
            st["weighted_rms"] = sqrt_rn(float(Fraction(num) / Fraction(den)))
        best = None
        for i, e in enumerate(self.entries):
            if e["used"]:
                a = abs(e["residual"])
                if best is None or a > best[0]:
                    best = (a, i)
        st["max_abs"], st["max_entry"] = best
        return st

    def stats_json(self, st):
        o = {"n_rejected": str(st["n_rejected"]), "n_used": str(st["n_used"])}
        for k in ("max_abs", "rms", "weighted_rms"):
            if st[k] is not None:
                o[k] = {"unit": "m", "value": hexf(st[k])}
        if st["max_entry"] is not None:
            o["max_entry"] = str(st["max_entry"])
        return json.dumps(o, sort_keys=True, separators=(",", ":")).encode("ascii")

    def columns(self):
        es = self.entries
        n = self.n
        out = [struct.pack("<QB", n, 1 if self.weighted else 0)]
        out.append(struct.pack("<%dI" % n, *[e["input"] for e in es]))
        out.append(struct.pack("<%dQ" % n, *[e["item"] for e in es]))
        out.append(bytes(KIND_BYTE[e["kind"]] for e in es))
        out.append(struct.pack("<%dd" % n, *[e["residual"] for e in es]))
        out.append(bytes(1 if e["vector"] is not None else 0 for e in es))
        vec = [c for e in es if e["vector"] is not None for c in e["vector"]]
        out.append(struct.pack("<%dd" % len(vec), *vec))
        if self.weighted:
            out.append(struct.pack("<%dd" % n, *[e["weight"] for e in es]))
        out.append(bytes(1 if e["used"] else 0 for e in es))
        rs = bytearray()
        for e in es:
            name = (e["reason"] or "").encode("ascii")
            rs.append(len(name))
            rs += name
        out.append(bytes(rs))
        return b"".join(out)

    def digest(self, st):
        sj = self.stats_json(st)
        cols = hashlib.sha256(self.columns()).digest()
        pre = b"FIDRES" + bytes([1]) + struct.pack("<Q", len(sj)) + sj + cols
        return "sha256:" + hashlib.sha256(pre).hexdigest()


def entry(input_, item, kind, residual, vector=None, weight=None, reason=None):
    return {
        "raw_residual": residual,
        "raw_vector": vector,
        "input": input_,
        "item": item,
        "kind": kind,
        "residual": plus_zero(residual),
        "vector": None if vector is None else [plus_zero(c) for c in vector],
        "weight": weight,
        "used": reason is None,
        "reason": reason,
    }


# ------------------------------------------------------------------ hand cases

U = 2.0**-12  # about 0.244 mm: every value below is an integer multiple of 2^-12 m or 2^-14 m


def hand_cases():
    cases = []

    def ok(name, entries, about):
        cases.append(("ok", name, about, entries, None))

    def bad(name, entries, about, code, index):
        cases.append(("err", name, about, entries, (code, index)))

    ok(
        "uniform_exact",
        [entry(0, i, "point", (-1) ** i * 2.0**-10) for i in range(4)],
        "equal magnitudes: RMS and max are exactly 2^-10 m; the tie goes to entry 0",
    )
    ok(
        "pythagorean",
        [entry(0, 0, "feature", 3 * U), entry(0, 1, "feature", -4 * U)],
        "RMS of 3u and -4u is sqrt(12.5) u, rounded once; max is entry 1",
    )
    ok(
        "mixed_kinds_weights",
        [
            entry(0, 0, "point", 2 * U, vector=[2 * U, 0.0, 0.0], weight=1.0),
            entry(0, 1, "point", -3 * U, vector=[0.0, -3 * U, -0.0], weight=2.0),
            entry(1, 0, "feature", 5 * U, weight=0.5, reason="outlier"),
            entry(1, 1, "pair", -1 * U, weight=1.5),
            entry(2, 7, "locked_direction", 4 * U, weight=0.25),
            entry(2, 8, "pair", -9 * U, weight=1.0, reason="beyond_cutoff"),
            entry(2, 9, "pair", -4 * U, weight=0.75),
        ],
        "every kind, vectors, weights and two rejections with different reasons; the "
        "rejected -9u and 5u do not count; max |r| ties at 4u between entries 4 and 6",
    )
    ok(
        "tie_lowest_index",
        [entry(0, 0, "pair", -3 * U), entry(0, 1, "pair", 3 * U), entry(0, 2, "pair", -3 * U)],
        "three equal magnitudes: max_entry is 0",
    )
    ok(
        "single",
        [entry(3, 41, "feature", -7 * U)],
        "one entry: RMS = |r| = max",
    )
    ok(
        "negative_zero",
        [entry(0, 0, "point", -0.0, vector=[-0.0, 0.0, -0.0])],
        "-0.0 is stored as +0.0; RMS and max are +0.0",
    )
    ok(
        "all_rejected",
        [entry(0, 0, "point", 1 * U, reason="outlier"), entry(0, 1, "point", -2 * U, reason="outlier")],
        "no used entry: counts only, no RMS or max",
    )
    ok(
        "near_overflow",
        [entry(0, 0, "feature", 2.0**511), entry(0, 1, "feature", -(2.0**511))],
        "sum of squares 2^1023 is finite; RMS is 2^511 exactly",
    )
    bad(
        "overflow",
        [entry(0, i, "feature", 2.0**511) for i in range(4)],
        "sum of squares 2^1024 exceeds f64::MAX",
        "stat_overflow",
        None,
    )
    bad(
        "square_overflow",
        [entry(0, 0, "feature", 2.0**512)],
        "the square 2^1024 overflows",
        "stat_overflow",
        None,
    )
    bad("empty", [], "no entries", "empty_report", None)
    bad(
        "nan",
        [entry(0, 0, "point", U), entry(0, 1, "point", U), entry(0, 2, "point", float("nan"))],
        "NaN residual at entry 2",
        "nan_residual",
        2,
    )
    bad("infinite", [entry(0, 0, "point", float("-inf"))], "infinite residual at entry 0", "nan_residual", 0)
    bad(
        "vector_nan",
        [entry(0, 0, "point", U, vector=[0.0, float("nan"), 0.0])],
        "NaN vector component at entry 0",
        "nan_residual",
        0,
    )
    bad(
        "mixed_weights",
        [entry(0, 0, "pair", U, weight=1.0), entry(0, 1, "pair", U)],
        "entry 1 has no weight in a weighted report",
        "mixed_weights",
        1,
    )
    bad("zero_weight", [entry(0, 0, "pair", U, weight=0.0)], "a weight must be > 0", "bad_weight", 0)
    return cases


def wire_entry(e):
    def hx(x):
        return None if x is None else hexf(x)

    return {
        "input": e["input"],
        "item": e["item"],
        "kind": e["kind"],
        "residual": hexf(e["raw_residual"]),
        "vector": None if e["raw_vector"] is None else [hexf(c) for c in e["raw_vector"]],
        "weight": hx(e["weight"]),
        "used": e["used"],
        "reason": e["reason"],
    }


def cases_json():
    out = []
    for kind, name, about, entries, err in hand_cases():
        c = {"name": name, "about": about, "entries": [wire_entry(e) for e in entries]}
        if kind == "ok":
            rep = Report(entries)
            st = rep.stats(exact=True)
            exp = {k: (None if st[k] is None else hexf(st[k])) for k in ("rms", "weighted_rms", "max_abs")}
            exp["max_entry"] = st["max_entry"]
            exp["n_used"] = st["n_used"]
            exp["n_rejected"] = st["n_rejected"]
            exp["stats_json"] = rep.stats_json(st).decode("ascii")
            exp["digest"] = rep.digest(st)
            c["expect"] = exp
        else:
            c["error"] = {"code": err[0], "index": err[1]}
        out.append(c)
    doc = {
        "about": "ALN.QA.REPORT hand cases; generated by make_report_fixtures.py. Do not edit by hand. "
        "Floats are 0x + 16 hex digits of their IEEE 754 bits; lengths are metres.",
        "cases": out,
    }
    return json.dumps(doc, indent=1, sort_keys=True) + "\n"


# ------------------------------------------------------------------ seeded 1M report


def splitmix64(state):
    state = (state + 0x9E3779B97F4A7C15) & MASK
    z = state
    z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK
    z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK
    return state, z ^ (z >> 31)


def big_entries(n=N_BIG, seed=SEED):
    """The pinned generator (also written out in the Rust tests)."""
    state = seed
    es = []
    for i in range(n):
        state, z = splitmix64(state)
        kind = KINDS[z & 3]
        used = ((z >> 2) & 15) != 0
        weight = 1.0 + ((z >> 6) & 7) * 0.25
        r = ((z >> 11) * 2.0**-53 - 0.5) * 0.002
        vec = [r * 0.6, r * 0.8, 0.0] if kind == "point" else None
        es.append(entry(i % 3, i, kind, r, vector=vec, weight=weight, reason=None if used else "outlier"))
    return es


def big_text():
    es = big_entries()
    rep = Report(es)
    st = rep.stats(exact=False)
    lines = [
        "# ALN.QA.REPORT seeded report (seed 13, 1,000,000 entries): generated by make_report_fixtures.py.",
        "# Do not edit by hand. Floats are 0x + 16 hex digits of their IEEE 754 bits.",
        "n_entries %d" % rep.n,
        "n_used %d" % st["n_used"],
        "n_rejected %d" % st["n_rejected"],
        "rms %s" % hexf(st["rms"]),
        "weighted_rms %s" % hexf(st["weighted_rms"]),
        "max_abs %s" % hexf(st["max_abs"]),
        "max_entry %d" % st["max_entry"],
        "stats_json %s" % rep.stats_json(st).decode("ascii"),
        "digest %s" % rep.digest(st),
        # Spot checks of the generator for the page test (AC-5).
        "entry_900000 %s" % hexf(es[900000]["residual"]),
        "entry_999999 %s" % hexf(es[999999]["residual"]),
    ]
    return "\n".join(lines) + "\n"


# ------------------------------------------------------------------ AC-4 command output


def chain3_ids():
    a2 = None
    for line in (CHAIN3 / "expected.txt").read_text(encoding="utf-8").splitlines():
        p = line.split()
        if p[:2] == ["id", "A2"]:
            a2 = p[2]
    report = None
    for line in (CHAIN3 / "project.txt").read_text(encoding="utf-8").splitlines():
        p = line.split()
        if p[:1] == ["report_id"]:
            report = p[1]
    assert a2 and report
    return a2, report


def length(x):
    return None if x is None else {"unit": "m", "value": x}


def summary(rep, st, report_id):
    return {
        "digest": rep.digest(st),
        "max_abs": length(st["max_abs"]),
        "max_entry": st["max_entry"],
        "n_entries": rep.n,
        "n_rejected": st["n_rejected"],
        "n_used": st["n_used"],
        "report": report_id,
        "rms": length(st["rms"]),
        "weighted": rep.weighted,
        "weighted_rms": length(st["weighted_rms"]),
    }


def entry_out(i, e):
    return {
        "index": i,
        "input": e["input"],
        "item": e["item"],
        "kind": e["kind"],
        "reason": e["reason"],
        "residual": {"unit": "m", "value": e["residual"]},
        "used": e["used"],
        "vector": None if e["vector"] is None else {"unit": "m", "value": e["vector"]},
        "weight": e["weight"],
    }


def command_outputs():
    a2, report_id = chain3_ids()
    entries = next(c[3] for c in hand_cases() if c[1] == "mixed_kinds_weights")
    rep = Report(entries)
    st = rep.stats(exact=True)
    summ = {"alignment": a2, "summary": summary(rep, st, report_id)}
    offset, limit = 1, 3
    end = min(rep.n, offset + limit)
    page = dict(summ)
    page["page"] = {
        "entries": [entry_out(i, entries[i]) for i in range(offset, end)],
        "limit": limit,
        "next_offset": end if end < rep.n else None,
        "offset": offset,
    }
    return canon(summ), canon(page)


# ------------------------------------------------------------------ main


def outputs():
    s, p = command_outputs()
    return {
        "report-cases.json": cases_json(),
        "report-1m.txt": big_text(),
        "get-residuals-a2-summary.json": s,
        "get-residuals-a2-page.json": p,
    }


def main():
    check = "--check" in sys.argv[1:]
    bad = 0
    for name, text in outputs().items():
        path = HERE / name
        if check:
            if not path.exists() or path.read_text(encoding="utf-8") != text:
                print("STALE", name)
                bad += 1
        else:
            path.write_text(text, encoding="utf-8", newline="\n")
            print("wrote", name)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
