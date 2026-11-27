#!/usr/bin/env python3
"""Independent reference for RPT.STATS.TREND.HIST (histogram binning kernel), stdlib only.

Writes fixtures/rpt/stats/hist_cases.json (CC-BY-4.0 project data, invented inputs, D-038).
Nothing here is produced by running the Rust crate `fid-stats`.

Every case below is hand-chosen and hand-labelled from the leaf's rules
(spec/rpt/stats/trend/hist/NODE.md): the expected edges (as decimals) and counts were worked
out by hand. The script re-derives each case with its own reading of the rules
(`py_histogram`) and refuses to write if a hand label disagrees. A hand edge must equal the
computed edge exactly when the case marks it `exact`, and otherwise lie within a relative
1e-12 of it (an edge such as 3 * 1.6 is not a short decimal in binary). The written fixture
carries the computed edges as 16 hex digits of their IEEE 754 bits, which every target must
reproduce bit for bit (D0).

Python floats are IEEE 754 binary64 with each +, -, *, / rounded to nearest-even and never
fused, so `min + i * w` and `a + j * w` here are the same two roundings the kernel performs.
Comparisons that place a value in a bin are the IEEE ones (-0.0 == +0.0); min and max are
IEEE totalOrder (-0.0 below +0.0).

Reading of the rules (the order of decisions is the kernel's documented order):
  1. rule parameters: fixed_count k = 0 -> zero_bins, k > 1000 -> too_many_bins;
     fixed_width width NaN or infinite -> width_not_finite, width <= 0 -> width_not_positive,
     anchor NaN or infinite -> anchor_not_finite (all `stats.bad_binning`);
  2. no values -> `stats.insufficient_data`;
  3. the first NaN or infinite value -> `stats.non_finite{index}`;
  4. all values equal (IEEE ==) -> one bin [v, v] with v the totalOrder maximum, count n,
     degenerate;
  5. sturges@1: k = bit_length(n - 1) + 1; sturges@1 and fixed_count@1: w = (max - min) / k
     (max - min overflowing -> range_not_finite), e_i = min + i * w for i < k, e_k = max;
  6. fixed_width@1: (max - min) overflowing -> range_not_finite; (max - min) / w > 1001 ->
     too_many_bins; t = (min - a) / w, a non-finite t or |t| >= 2^52 -> anchor_too_far;
     j0 = floor(t); if a + j0 * w > min (the rounded quotient reached the next integer, so the
     literal floor would leave min outside every bin) then j0 - 1, and if that edge is still
     above min -> edges_not_increasing; edges a + j * w for j = j0, j0 + 1, ... up to and
     including the first edge > max; a non-finite edge -> edge_not_finite; more than 1000 bins
     -> too_many_bins;
  7. edges not strictly increasing (f64 cannot resolve the bin width at this magnitude) ->
     edges_not_increasing;
  8. x is counted in bin i when e_i <= x < e_(i+1), the last bin closed (bisect over the edges,
     never a division by the width); the counts sum to n.

`digest` is SHA-256 over one block per fixture case, in file order: "<id> " followed by
either "error:<code>[:<detail>]\\n" or "<rule_id> <edge count> <degenerate 0|1>\\n", then
"e <16 hex bits>\\n" per edge and "c <count>\\n" per bin. `random_10k` draws 10 000 values
v = 25.0 * u - 10.0 with u = uniform_f64 of the fid-num Philox4x32-10 stream (seed 42,
stream 0, indices 0..9999; KRN.NUM.RNG, reimplemented below and checked against the published
Random123 known answers), bins them with each listed rule and digests the blocks the same way
with id "random_10k". The Rust tests recompute both digests on every CI target (x86-64,
ARM64, wasm32), which is AC-4's cross-target identity.

Run:   python fixtures/rpt/stats/make_hist_cases.py
Check: python fixtures/rpt/stats/make_hist_cases.py --check
"""

import bisect
import hashlib
import json
import math
import struct
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE / "hist_cases.json"

MAX_BINS = 1000
ANCHOR_LIMIT = 2.0 ** 52
BAD = "stats.bad_binning"
NONF = "stats.non_finite"
INSUF = "stats.insufficient_data"


def bits(x):
    return format(struct.unpack("<Q", struct.pack("<d", x))[0], "016x")


def total_key(x):
    """IEEE totalOrder key for a non-NaN double: -0.0 sorts below +0.0."""
    u = struct.unpack("<Q", struct.pack("<d", x))[0]
    mag = u & 0x7FFFFFFFFFFFFFFF
    return -mag - 1 if u >> 63 else mag


class Refused(Exception):
    def __init__(self, token):
        super().__init__(token)
        self.token = token


def bad(reason):
    return Refused(f"{BAD}:{reason}")


def rule_id(rule):
    return {"sturges": "sturges@1", "fixed_count": "fixed_count@1", "fixed_width": "fixed_width@1"}[rule["name"]]


def py_histogram(values, rule):
    name = rule["name"]
    # 1. rule parameters
    if name == "fixed_count":
        k = rule["k"]
        if k == 0:
            raise bad("zero_bins")
        if k > MAX_BINS:
            raise bad("too_many_bins")
    elif name == "fixed_width":
        w, a = rule["width"], rule["anchor"]
        if math.isnan(w) or math.isinf(w):
            raise bad("width_not_finite")
        if not w > 0.0:
            raise bad("width_not_positive")
        if math.isnan(a) or math.isinf(a):
            raise bad("anchor_not_finite")
    else:
        assert name == "sturges", name
    # 2. no values
    n = len(values)
    if n == 0:
        raise Refused(INSUF)
    # 3. non-finite values
    for i, x in enumerate(values):
        if math.isnan(x) or math.isinf(x):
            raise Refused(f"{NONF}:{i}")
    lo = min(values, key=total_key)
    hi = max(values, key=total_key)
    # 4. all equal
    if lo == hi:
        return {"rule": rule_id(rule), "edges": [hi, hi], "counts": [n], "degenerate": True}
    # 5. / 6. edges
    if name in ("sturges", "fixed_count"):
        k = (n - 1).bit_length() + 1 if name == "sturges" else rule["k"]
        span = hi - lo
        if math.isinf(span):
            raise bad("range_not_finite")
        w = span / k
        edges = [lo + i * w for i in range(k)] + [hi]
    else:
        w, a = rule["width"], rule["anchor"]
        span = hi - lo
        if math.isinf(span):
            raise bad("range_not_finite")
        if span / w > MAX_BINS + 1:
            raise bad("too_many_bins")
        t = (lo - a) / w
        if math.isinf(t) or abs(t) >= ANCHOR_LIMIT:
            raise bad("anchor_too_far")
        j = math.floor(t)
        if a + j * w > lo:
            j -= 1
            if a + j * w > lo:
                raise bad("edges_not_increasing")
        edges = [a + j * w]
        while not edges[-1] > hi:
            if len(edges) == MAX_BINS + 1:
                raise bad("too_many_bins")
            j += 1
            e = a + j * w
            if math.isinf(e):
                raise bad("edge_not_finite")
            edges.append(e)
    # 7. strictly increasing
    for p, q in zip(edges, edges[1:]):
        if not p < q:
            raise bad("edges_not_increasing")
    # 8. counts
    counts = [0] * (len(edges) - 1)
    for x in values:
        idx = bisect.bisect_right(edges, x)
        if idx == len(edges) and x == edges[-1]:
            idx -= 1
        assert 1 <= idx <= len(edges) - 1, (x, edges)
        counts[idx - 1] += 1
    assert sum(counts) == n
    return {"rule": rule_id(rule), "edges": edges, "counts": counts, "degenerate": False}


def run_token(values, rule):
    try:
        return py_histogram(values, rule)
    except Refused as r:
        return "error:" + r.token


def block(id_, res):
    if isinstance(res, str):
        return f"{id_} {res}\n"
    out = [f"{id_} {res['rule']} {len(res['edges'])} {1 if res['degenerate'] else 0}\n"]
    out += [f"e {bits(e)}\n" for e in res["edges"]]
    out += [f"c {c}\n" for c in res["counts"]]
    return "".join(out)


# ---------------------------------------------------------------------------------------------
# Hand-labelled cases. `label` is either "error:<code>[:<detail>]" or
# (edges as decimal strings, counts, degenerate, exact) where `exact` says every hand edge is
# exactly the double the rules produce.

S = {"name": "sturges"}


def FC(k):
    return {"name": "fixed_count", "k": k}


def FW(width, anchor):
    return {"name": "fixed_width", "width": width, "anchor": anchor}


INF, NAN = float("inf"), float("nan")

CASES = [
    # AC-1: Sturges for n = 1, 2, 3, 8, 9 and 100.
    ("sturges_n1", "n = 1 is all-equal: one degenerate bin [5, 5]", [5.0], S,
     (["5", "5"], [1], True, True)),
    ("sturges_n2", "k = bit_length(1) + 1 = 2, w = 2", [4.0, 0.0], S,
     (["0", "2", "4"], [1, 1], False, True)),
    ("sturges_n3", "k = bit_length(2) + 1 = 3, w = 2; the middle bin is empty", [0.0, 1.0, 6.0], S,
     (["0", "2", "4", "6"], [2, 0, 1], False, True)),
    ("sturges_n8", "k = bit_length(7) + 1 = 4, w = 7/4 exactly", [float(i) for i in range(8)], S,
     (["0", "1.75", "3.5", "5.25", "7"], [2, 2, 2, 2], False, True)),
    ("sturges_n9", "k = bit_length(8) + 1 = 5, w = 8/5 (not exact in binary)", [float(i) for i in range(9)], S,
     (["0", "1.6", "3.2", "4.8", "6.4", "8"], [2, 2, 1, 2, 2], False, False)),
    ("sturges_n100", "1..100: k = bit_length(99) + 1 = 8, w = 99/8 = 12.375 exactly",
     [float(i) for i in range(100, 0, -1)], S,
     (["1", "13.375", "25.75", "38.125", "50.5", "62.875", "75.25", "87.625", "100"],
      [13, 12, 13, 12, 12, 13, 12, 13], False, True)),
    # AC-1: values on bin edges (half-open bins, last bin closed).
    ("on_edges", "values on every edge: e_i <= x < e_(i+1), max in the last bin",
     [0.0, 2.0, 4.0, 6.0, 8.0, 8.0, 2.0], FC(4),
     (["0", "2", "4", "6", "8"], [1, 2, 1, 3], False, True)),
    # AC-2.
    ("ac2_fixed_count_4", "AC-2: {0,1,2,3,4}, k = 4", [0.0, 1.0, 2.0, 3.0, 4.0], FC(4),
     (["0", "1", "2", "3", "4"], [1, 1, 1, 2], False, True)),
    ("fixed_count_1", "k = 1: one bin [min, max]", [3.0, -1.0, 2.0], FC(1),
     (["-1", "3"], [3], False, True)),
    # AC-1: all-equal values.
    ("all_equal_sturges", "seven equal values", [3.5] * 7, S, (["3.5", "3.5"], [7], True, True)),
    ("all_equal_fixed_width", "all-equal wins over the width rule", [-2.25] * 3, FW(0.5, 0.0),
     (["-2.25", "-2.25"], [3], True, True)),
    ("signed_zeros_equal", "-0.0 and +0.0 are equal values; the edge is the totalOrder max +0.0",
     [-0.0, 0.0, -0.0], S, (["0", "0"], [3], True, True)),
    # AC-1: fixed width with a negative anchor.
    ("fixed_width_negative_anchor", "a = -1, w = 2.5: j0 = floor(-0.8) = -1; stop at 9 > 6.5",
     [-3.0, 0.0, 1.5, 4.0, 6.5], FW(2.5, -1.0),
     (["-3.5", "-1", "1.5", "4", "6.5", "9"], [1, 1, 1, 1, 1], False, True)),
    ("fixed_width_min_on_edge", "min on an edge starts the first bin", [2.0, 3.5], FW(1.0, 0.0),
     (["2", "3", "4"], [1, 1], False, True)),
    ("fixed_width_max_on_edge", "max on an edge opens one more bin (the first edge > max)", [0.5, 3.0], FW(1.0, 0.0),
     (["0", "1", "2", "3", "4"], [1, 0, 0, 1], False, True)),
    ("fixed_width_floor_rounds_up", "a = 0, w = 0.1, min = 1.7: 1.7 / 0.1 rounds to 17.0 but 17 * 0.1 > 1.7, "
     "so j0 = 16 keeps min in a bin", [1.7, 1.75], FW(0.1, 0.0),
     (["1.6", "1.7", "1.8"], [1, 1], False, False)),
    ("fixed_width_floor_rounds_down", "a = 0.1, w = 0.1, min = 2.0: t = 18.999999999999996, j0 = 18 as REQ-003 "
     "says; the first bin [1.9, 2.0) is empty", [2.0, 2.05], FW(0.1, 0.1),
     (["1.9", "2", "2.1"], [0, 2], False, False)),
    ("fixed_width_1000_bins", "exactly 1000 bins is allowed", [0.0, 999.5], FW(1.0, 0.0),
     ([str(i) for i in range(1001)], [1] + [0] * 998 + [1], False, True)),
    # AC-3 and the other refusals.
    ("ac3_width_zero", "AC-3: width 0", [1.0, 2.0], FW(0.0, 0.0), f"error:{BAD}:width_not_positive"),
    ("width_negative_zero", "width -0.0 is not positive", [1.0, 2.0], FW(-0.0, 0.0), f"error:{BAD}:width_not_positive"),
    ("width_negative", "negative width", [1.0, 2.0], FW(-1.0, 0.0), f"error:{BAD}:width_not_positive"),
    ("width_inf", "infinite width", [1.0, 2.0], FW(INF, 0.0), f"error:{BAD}:width_not_finite"),
    ("width_nan", "NaN width", [1.0, 2.0], FW(NAN, 0.0), f"error:{BAD}:width_not_finite"),
    ("anchor_nan", "NaN anchor", [1.0, 2.0], FW(1.0, NAN), f"error:{BAD}:anchor_not_finite"),
    ("bad_width_beats_no_data", "a bad rule is refused before the data are looked at", [], FW(0.0, 0.0),
     f"error:{BAD}:width_not_positive"),
    ("count_zero", "fixed_count k = 0", [1.0, 2.0], FC(0), f"error:{BAD}:zero_bins"),
    ("count_1001", "fixed_count k = 1001", [1.0, 2.0], FC(1001), f"error:{BAD}:too_many_bins"),
    ("fixed_width_1001_bins", "1001 bins", [0.0, 1000.5], FW(1.0, 0.0), f"error:{BAD}:too_many_bins"),
    ("fixed_width_huge_span", "span / width far above 1000", [1.0, 2.0], FW(1e-300, 0.0), f"error:{BAD}:too_many_bins"),
    ("empty_sturges", "n = 0", [], S, f"error:{INSUF}"),
    ("nan_value", "NaN at index 2", [1.0, 2.0, NAN, INF], S, f"error:{NONF}:2"),
    ("inf_value", "+inf at index 0", [INF, 1.0], FC(3), f"error:{NONF}:0"),
    ("range_overflow", "max - min overflows", [-1e308, 1e308], S, f"error:{BAD}:range_not_finite"),
    ("range_overflow_width", "max - min overflows (fixed width)", [-1e308, 1e308], FW(1e306, 0.0),
     f"error:{BAD}:range_not_finite"),
    ("resolution", "min = 1e16, max = 1e16 + 2, k = 4: 1e16 + 0.5 rounds back to 1e16", [1e16, 1e16 + 2.0], FC(4),
     f"error:{BAD}:edges_not_increasing"),
    ("anchor_far", "(min - a) / w is beyond the exact integer range", [1e17, 1e17 + 512.0], FW(1.0, 0.0),
     f"error:{BAD}:anchor_too_far"),
    ("anchor_overflow", "(min - a) / w overflows", [1.0, 1.0 + 2.0 ** -40], FW(2.0 ** -40, -1e300), f"error:{BAD}:anchor_too_far"),
    ("edge_overflow", "the first edge above max overflows", [1.7e308, 1.75e308], FW(1e307, 0.0),
     f"error:{BAD}:edge_not_finite"),
]

# ---------------------------------------------------------------------------------------------
# Philox4x32-10 (KRN.NUM.RNG), reimplemented from Salmon et al., SC 2011.

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
    ((0x243F6A88, 0x85A308D3, 0x13198A2E, 0x03707344), (0xA4093822, 0x299F31D0),
     (0xD16CFE09, 0x94FDCCEB, 0x5001E420, 0x24126EA1)),
]:
    assert philox(_ctr, _key) == _want, "Philox4x32-10 known answer"


def uniform(seed, stream, index):
    w = philox((index & MASK, index >> 32, stream & MASK, stream >> 32), (seed & MASK, seed >> 32))
    return (((w[1] << 32) | w[0]) >> 11) * 2.0 ** -53


RANDOM = {"seed": 42, "stream": 0, "n": 10000, "scale": 25.0, "offset": -10.0,
          "rules": [S, FC(37), FW(0.7, -0.35)]}


def to_json_rule(rule):
    out = {"name": rule["name"]}
    if "k" in rule:
        out["k"] = rule["k"]
    if "width" in rule:
        out["width"] = bits(rule["width"])
        out["anchor"] = bits(rule["anchor"])
    return out


def check_hand(id_, got, label):
    if isinstance(label, str):
        assert got == label, (id_, got, label)
        return
    hand_edges, hand_counts, degenerate, exact = label
    assert not isinstance(got, str), (id_, got)
    assert got["counts"] == hand_counts, (id_, got["counts"], hand_counts)
    assert got["degenerate"] == degenerate, id_
    assert len(got["edges"]) == len(hand_edges), (id_, got["edges"])
    for e, h in zip(got["edges"], hand_edges):
        hv = float(h)
        if exact:
            assert bits(e) == bits(hv), (id_, e, h)
        else:
            assert abs(e - hv) <= 1e-12 * max(1.0, abs(hv)), (id_, e, h)


def build():
    cases, digest_text = [], []
    ids = set()
    for id_, note, values, rule, label in CASES:
        assert id_ not in ids, id_
        ids.add(id_)
        got = run_token(values, rule)
        check_hand(id_, got, label)
        out = {"id": id_, "note": note, "values": [bits(v) for v in values], "rule": to_json_rule(rule)}
        if isinstance(got, str):
            out["expect"] = {"error": got[len("error:"):]}
        else:
            out["expect"] = {"rule_id": got["rule"], "edges": [bits(e) for e in got["edges"]],
                             "counts": got["counts"], "degenerate": got["degenerate"],
                             "hand_edges": label[0], "hand_edges_exact": label[3]}
        digest_text.append(block(id_, got))
        cases.append(out)
    vals = [RANDOM["scale"] * uniform(RANDOM["seed"], RANDOM["stream"], i) + RANDOM["offset"]
            for i in range(RANDOM["n"])]
    rnd_text, rnd_rules = [], []
    for rule in RANDOM["rules"]:
        got = py_histogram(vals, rule)
        rnd_text.append(block("random_10k", got))
        rnd_rules.append({"rule": to_json_rule(rule), "rule_id": got["rule"], "bins": len(got["counts"]),
                          "counts": got["counts"]})
    doc = {
        "fixture": "RPT.STATS.TREND.HIST histogram binning cases",
        "licence": "CC-BY-4.0",
        "generator": "fixtures/rpt/stats/make_hist_cases.py",
        "max_bins": MAX_BINS,
        "cases": cases,
        "digest": hashlib.sha256("".join(digest_text).encode("ascii")).hexdigest(),
        "random_10k": {
            "seed": RANDOM["seed"], "stream": RANDOM["stream"], "n": RANDOM["n"],
            "transform": "v = scale * uniform_f64 + offset",
            "scale": bits(RANDOM["scale"]), "offset": bits(RANDOM["offset"]),
            "rules": rnd_rules,
            "digest": hashlib.sha256("".join(rnd_text).encode("ascii")).hexdigest(),
        },
    }
    return json.dumps(doc, indent=1, ensure_ascii=True) + "\n"


def main():
    text = build()
    if "--check" in sys.argv[1:]:
        have = OUT.read_text(encoding="utf-8") if OUT.exists() else ""
        if have != text:
            print(f"{OUT} is out of date; run python fixtures/rpt/stats/make_hist_cases.py")
            return 1
        print("hist_cases.json up to date")
        return 0
    OUT.write_text(text, encoding="utf-8", newline="\n")
    print(f"wrote {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
