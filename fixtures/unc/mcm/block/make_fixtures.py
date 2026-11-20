#!/usr/bin/env python3
"""Independent reference for UNC.PROP.MCM.BLOCK fixtures (stdlib only; CC-BY-4.0 data).

Nothing here runs or imports the Rust code. The models are hand-written for this leaf; the
expected samples are drawn here by a separate implementation of the addressing and the
samplers pinned in UNC.MODEL.DIST and in the module documentation of fid-kunc
`prop::mcm::block`:

- RNG: an independent Philox4x32-10 (checked against the published Random123 known answers
  below), block (seed, stream, index) with counter (index lo, index hi, stream lo,
  stream hi) and key (seed lo, seed hi);
- uniform on the open interval: u = (2 k + 1) 2^-53, k the top 52 bits of w1 << 32 | w0;
- normal: mu + sigma * ppnd16(u), Wichura's AS241 in binary64 with the coefficient table of
  fixtures/unc/dist (checked there against a 60-digit inverse normal). ln is a 60-digit
  decimal logarithm rounded once and sqrt is IEEE-754 correctly rounded, so nothing depends
  on the platform libm. In the central region (|u - 0.5| <= 0.425) only +, -, *, / are
  used, so those samples equal the crate's bit for bit; in the tails the crate's ln may
  differ in the last place, so the tests allow a few units in the last place there;
- rectangular: a + (b - a) * u;
- input ordinal = position in the model's ID-sorted inputs; stream = ordinal;
- inputs digest: SHA-256 of the sampling plan's canonical JSON (sorted keys, no
  whitespace): {"algorithm", "model", "samplers": [{"inputs", "kind", "stream"}]}.

The model documents use the uncertainty-model/v1 form; their canonical form and digest come
from the UNC.MODEL.GRAPH.SCHEMA oracle (fixtures/unc/model/graph/make_fixtures.py, loaded
under its own name).

Files (in this directory):
  manifest.json   canonical kernel manifest (KRN.MAN.MANI.SCHEMA rules) listing the test
                  kernel table of crates/fid-kunc/tests/block.rs; its core digest, written to
                  expected.json, plays the digest compiled into that test kernel
  linear2.json    AC-1, AC-2: Y = X1 + X2 with normal inputs (operator test.mcm.sum)
  fail_fit.json   AC-3: a circle fit (test.mcm.circle_fit) that fails for a negative
                  sampled radius
  rect4.json      AC-H1: Y = X1 + X2 + X3 + X4, Xi rectangular on [-sqrt 3, sqrt 3], the
                  rectangular-input additive model of JCGM 101:2008 [unverified: clause 9.2
                  variant]
  expected.json   digests, the X1 and X2 samples of linear2 at trial 12 345 (seed 7), the
                  linear2 Y values of every 101st trial of blocks 0..3 (seed 7, size 10 000),
                  the failed trials of fail_fit blocks 0 and 1 (seed 7, size 1000) with spot
                  samples, and the AC-H1 decision constants:
                  - ks_c_001: the 0.99 quantile of the asymptotic Kolmogorov distribution
                    (bisection on its alternating series, 60-digit decimals);
                  - binom_band: the exact two-sided 99 % acceptance band [lo, hi] for the
                    number of rejections among 100 seeds under Binomial(100, 0.01), lo the
                    largest k with P(K < k) <= 0.005 and hi the smallest k with
                    P(K > k) <= 0.005, in exact rationals.

Usage: make_fixtures.py [--check]   (--check compares with the files on disk, writes nothing)
"""

import hashlib
import importlib.util
import json
import math
import pathlib
import struct
import sys
from decimal import Decimal, getcontext
from fractions import Fraction

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[3]
getcontext().prec = 60


def load(name, rel):
    spec = importlib.util.spec_from_file_location(name, ROOT / rel)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


schema = load("graph_schema_oracle", "fixtures/unc/model/graph/make_fixtures.py")
dist = load("dist_oracle", "fixtures/unc/dist/make_fixtures.py")
getcontext().prec = 60  # the loaded oracles set their own precision at import

ALGORITHM_ID = "unc.prop.mcm_block@1.0.0"

# --------------------------------------------------------------------------- RNG

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
        hi0, lo0, hi1, lo1 = p0 >> 32, p0 & MASK, p1 >> 32, p1 & MASK
        c0, c1, c2, c3 = hi1 ^ c1 ^ k0, lo1, hi0 ^ c3 ^ k1, lo0
    return (c0, c1, c2, c3)


# Published Random123 known answers for philox4x32_10 (kat_vectors).
for _ctr, _key, _want in [
    ((0, 0, 0, 0), (0, 0), (0x6627E8D5, 0xE169C58D, 0xBC57AC4C, 0x9B00DBD8)),
    ((MASK,) * 4, (MASK, MASK), (0x408F276D, 0x41C83B0E, 0xA20BC7C6, 0x6D5451FD)),
    ((0x243F6A88, 0x85A308D3, 0x13198A2E, 0x03707344), (0xA4093822, 0x299F31D0),
     (0xD16CFE09, 0x94FDCCEB, 0x5001E420, 0x24126EA1)),
]:
    assert philox(_ctr, _key) == _want


def open_unit(seed, stream, index):
    w = philox((index & MASK, index >> 32, stream & MASK, stream >> 32), (seed & MASK, seed >> 32))
    x = (w[1] << 32) | w[0]
    return ((x >> 12) * 2 + 1) * 2.0 ** -53


# --------------------------------------------------------------------------- samplers

AS = dist.AS241


def horner(c, x):
    acc = c[7]
    for k in range(6, -1, -1):
        acc = acc * x + c[k]
    return acc


def ln(x):
    """ln(x) correctly rounded from a 60-digit decimal logarithm (no platform libm)."""
    return float(Decimal(x).ln())


def ppnd16(p):
    """AS241 PPND16 in binary64; returns (x, central)."""
    q = p - 0.5
    if abs(q) <= 0.425:
        t = 0.180625 - q * q
        return q * horner(AS["a"], t) / horner(AS["b"], t), True
    t = p if q < 0 else 1.0 - p
    t = math.sqrt(-ln(t))  # IEEE-754 square root, correctly rounded everywhere
    if t <= 5.0:
        t -= 1.6
        v = horner(AS["c"], t) / horner(AS["d"], t)
    else:
        t -= 5.0
        v = horner(AS["e"], t) / horner(AS["f"], t)
    return (-v if q < 0 else v), False


def normal(mu, sigma, seed, stream, t):
    z, central = ppnd16(open_unit(seed, stream, t))
    return mu + sigma * z, central


def rect(a, b, seed, stream, t):
    return a + (b - a) * open_unit(seed, stream, t)


def h(x):
    return format(struct.unpack("<Q", struct.pack("<d", x))[0], "016x")


# --------------------------------------------------------------------------- models

def sha(label):
    return "sha256:" + hashlib.sha256(label.encode("ascii")).hexdigest()


def canon(obj):
    text = json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
    assert "\\" not in text
    return text.encode("ascii")


TEST_ALGORITHMS = ["test.mcm.sum@1.0.0", "test.mcm.circle_fit@1.0.0", "test.mcm.neg@1.0.0", "test.mcm.bad@1.0.0"]


def alg_key(entry):
    name, version = entry["id"].split("@")
    return (tuple(name.split(".")), tuple(int(x) for x in version.split(".")))


def manifest():
    core = {
        "schema_version": "1",
        "name": "fid-kernel-mcm-block-test",
        "semver": "0.1.0",
        "numeric_profile": "knp-1",
        "source_tree_digest": sha("mcm block test source tree"),
        "lockfile_digest": sha("mcm block test Cargo.lock"),
        "toolchain_digest": sha("mcm block test toolchain"),
        "replay_runtime": {"version": "0.1.0", "config_digest": sha("mcm block test replay config")},
        "algorithms": sorted(
            ({"id": i, "card_digest": sha("card " + i), "impl_digest": sha("impl " + i)} for i in TEST_ALGORITHMS),
            key=alg_key),
        "facade_selftest_digest": sha("mcm block test facade self-test"),
        "solver_limits_digest": sha("mcm block test solver limits"),
        "reference_suite_digest": sha("mcm block test reference suite"),
        "reference_results_digest": sha("mcm block test reference results"),
        "native_gate": "measured-only",
    }
    artifacts = [{"role": "wasm", "target": "wasm32-unknown-unknown", "sha256": sha("mcm block test wasm")}]
    return {"artifacts": artifacts, "core": core, "core_digest": "sha256:" + hashlib.sha256(canon(core)).hexdigest()}


inp, port_in, port_out, edge = schema.inp, schema.port_in, schema.port_out, schema.edge

LINEAR2 = {"x1": (1.0e-2, 1.0e-5), "x2": (2.0e-2, 2.0e-5)}
FAIL_FIT = {"radius": (2.0e-3, 1.0e-3), "offset": (-1.0e-4, 1.0e-4)}
H = math.sqrt(3.0)


def op(ident, alg, params, ports_in, ports_out):
    name, version = alg.split("@")
    return {"id": ident, "algorithm_id": name, "algorithm_version": version, "params": params,
            "ports_in": ports_in, "ports_out": ports_out}


def linear2():
    return {
        "schema": "uncertainty-model/v1", "name": "linear2", "semver": "1.0.0",
        "inputs": [
            inp("x2", "length", "si:m", "normal", {"mu": LINEAR2["x2"][0], "sigma": LINEAR2["x2"][1]},
                "declared", "decl:linear2-x2", "metrology-maven fixtures"),
            inp("x1", "length", "si:m", "normal", {"mu": LINEAR2["x1"][0], "sigma": LINEAR2["x1"][1]},
                "declared", "decl:linear2-x1", "metrology-maven fixtures"),
        ],
        "operators": [op("y_sum", "test.mcm.sum@1.0.0", {"mode": "sequential"},
                         [port_in("terms", "length", "si:m", 2)], [port_out("value", "length", "si:m")])],
        "outputs": [{"id": "y", "quantity_kind": "length", "unit": "si:m"}],
        "edges": [edge("x2", "value", "y_sum", "terms"), edge("x1", "value", "y_sum", "terms"),
                  edge("y_sum", "value", "y", "value")],
        "correlations": [],
    }


def fail_fit():
    r, o = FAIL_FIT["radius"], FAIL_FIT["offset"]
    return {
        "schema": "uncertainty-model/v1", "name": "fail-fit", "semver": "1.0.0",
        "inputs": [
            inp("radius", "length", "si:m", "normal", {"mu": r[0], "sigma": r[1]},
                "declared", "decl:fail-fit-radius", "metrology-maven fixtures"),
            inp("offset", "length", "si:m", "rect", {"a": o[0], "b": o[1]},
                "declared", "decl:fail-fit-offset", "metrology-maven fixtures"),
        ],
        "operators": [op("fit", "test.mcm.circle_fit@1.0.0", {"points": "8"},
                         [port_in("radius", "length", "si:m", 1), port_in("offset", "length", "si:m", 1)],
                         [port_out("diameter", "length", "si:m")])],
        "outputs": [{"id": "d", "quantity_kind": "length", "unit": "si:m"}],
        "edges": [edge("radius", "value", "fit", "radius"), edge("offset", "value", "fit", "offset"),
                  edge("fit", "diameter", "d", "value")],
        "correlations": [],
    }


def rect4():
    xs = ["x1", "x2", "x3", "x4"]
    return {
        "schema": "uncertainty-model/v1", "name": "rect4", "semver": "1.0.0",
        "inputs": [inp(x, "length", "si:m", "rect", {"a": -H, "b": H}, "declared", "decl:rect4-" + x,
                       "metrology-maven fixtures") for x in xs],
        "operators": [op("y_sum", "test.mcm.sum@1.0.0", {"mode": "sequential"},
                         [port_in("terms", "length", "si:m", 4)], [port_out("value", "length", "si:m")])],
        "outputs": [{"id": "y", "quantity_kind": "length", "unit": "si:m"}],
        "edges": [edge(x, "value", "y_sum", "terms") for x in xs] + [edge("y_sum", "value", "y", "value")],
        "correlations": [],
    }


def model_digest(m):
    return "sha256:" + hashlib.sha256(schema.canon_bytes(schema.canonical(m))).hexdigest()


def inputs_digest(m):
    """Plan of a model without correlation groups: one univariate sampler per input."""
    assert not m["correlations"]
    ids = sorted(x["id"] for x in m["inputs"])
    kinds = {x["id"]: x["distribution"]["kind"] for x in m["inputs"]}
    plan = {
        "algorithm": ALGORITHM_ID,
        "model": model_digest(m),
        "samplers": [{"inputs": [i], "kind": kinds[i], "stream": str(k)} for k, i in enumerate(ids)],
    }
    return "sha256:" + hashlib.sha256(canon(plan)).hexdigest()


# --------------------------------------------------------------------------- expectations

SEED = 7
L2_SIZE, L2_BLOCKS, L2_STEP = 10_000, 4, 101
FF_SIZE, FF_BLOCKS = 1000, 2


def linear2_expected():
    (m1, s1), (m2, s2) = LINEAR2["x1"], LINEAR2["x2"]
    rows = []
    for t in range(0, L2_SIZE * L2_BLOCKS, L2_STEP):
        x1, c1 = normal(m1, s1, SEED, 0, t)
        x2, c2 = normal(m2, s2, SEED, 1, t)
        rows.append([str(t), h(x1 + x2), "exact" if c1 and c2 else "tail"])
    x1, c1 = normal(m1, s1, SEED, 0, 12345)
    x2, c2 = normal(m2, s2, SEED, 1, 12345)
    return {
        "seed": str(SEED), "block_size": str(L2_SIZE), "blocks": str(L2_BLOCKS),
        "x1_12345": h(x1), "x1_12345_region": "exact" if c1 else "tail",
        "x2_12345": h(x2), "x2_12345_region": "exact" if c2 else "tail",
        "y_every_101": rows,
    }


def fail_fit_expected():
    (mu, sigma), (a, b) = FAIL_FIT["radius"], FAIL_FIT["offset"]
    failed, spots = [], []
    for t in range(FF_SIZE * FF_BLOCKS):
        r, _ = normal(mu, sigma, SEED, 1, t)  # radius: ordinal 1 ("offset" < "radius")
        assert abs(r) > 1e-9, "a radius this close to zero is not a clear-cut fixture"
        if r < 0:
            failed.append(t)
        elif t % 97 == 0:
            spots.append([str(t), h(r), h(rect(a, b, SEED, 0, t))])
    assert failed, "the fixture must have failing trials"
    return {"seed": str(SEED), "block_size": str(FF_SIZE), "blocks": str(FF_BLOCKS),
            "failed": [str(t) for t in failed], "spot_radius_offset": spots}


def kolmogorov_quantile(level):
    """x with P(K <= x) = level for the asymptotic Kolmogorov distribution."""
    def tail(x):
        s, k = Decimal(0), 1
        while True:
            term = (-2 * k * k * x * x).exp()
            if term < Decimal(10) ** -50:
                return 2 * s
            s += term if k % 2 == 1 else -term
            k += 1
    target = 1 - Decimal(level)
    lo, hi = Decimal("0.5"), Decimal(3)
    for _ in range(200):
        mid = (lo + hi) / 2
        if tail(mid) > target:
            lo = mid
        else:
            hi = mid
    return float(lo)


def binomial_band(n, p, two_sided):
    pmf = [math.comb(n, k) * p ** k * (1 - p) ** (n - k) for k in range(n + 1)]
    half = Fraction(two_sided) / 2
    lo = max(k for k in range(n + 1) if sum(pmf[:k]) <= half)
    hi = min(k for k in range(n + 1) if sum(pmf[k + 1:]) <= half)
    return lo, hi


def ac_h1_expected():
    c = kolmogorov_quantile(Decimal("0.99"))
    assert abs(c - 1.6276) < 1e-3
    lo, hi = binomial_band(100, Fraction(1, 100), Fraction(1, 100))
    return {"alpha": "0.01", "ks_c_001": h(c), "ks_c_001_decimal": repr(c), "seeds": "100",
            "binom_band": [str(lo), str(hi)], "half_width": h(H)}


def outputs():
    man = manifest()
    models = {"linear2.json": linear2(), "fail_fit.json": fail_fit(), "rect4.json": rect4()}
    expected = {
        "core_digest": man["core_digest"],
        "manifest_digest": "sha256:" + hashlib.sha256(canon(man)).hexdigest(),
        "models": {name: {"model_digest": model_digest(m), "inputs_digest": inputs_digest(m)}
                   for name, m in models.items()},
        "linear2": linear2_expected(),
        "fail_fit": fail_fit_expected(),
        "ac_h1": ac_h1_expected(),
    }
    files = {"manifest.json": canon(man)}
    for name, m in models.items():
        files[name] = (json.dumps(m, indent=2, ensure_ascii=True) + "\n").encode("ascii")
    files["expected.json"] = (json.dumps(expected, indent=1, sort_keys=True, ensure_ascii=True) + "\n").encode("ascii")
    return files


def main():
    check = "--check" in sys.argv[1:]
    bad = 0
    files = outputs()
    for rel, data in sorted(files.items()):
        path = HERE / rel
        if check:
            if not path.exists() or path.read_bytes() != data:
                print("stale: " + rel)
                bad += 1
        else:
            path.write_bytes(data)
    if check:
        on_disk = {p.name for p in HERE.iterdir() if p.is_file() and p.suffix == ".json"}
        for extra in sorted(on_disk - set(files)):
            print("not generated: " + extra)
            bad += 1
    if bad:
        sys.exit(1)
    print("ok: %d files" % len(files))


if __name__ == "__main__":
    main()
