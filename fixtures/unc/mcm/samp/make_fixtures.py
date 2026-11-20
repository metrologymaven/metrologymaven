#!/usr/bin/env python3
"""Independent reference for UNC.PROP.MCM.SAMP fixtures (stdlib only; CC-BY-4.0 data).

Nothing here runs or imports the Rust code. The sample sets are rebuilt from scratch:

- the trial values of the UNC.PROP.MCM.BLOCK fixtures (linear2: Y = X1 + X2, seed 7,
  4 blocks of 10 000; fail_fit: a circle fit failing for a negative radius, seed 7, 2 blocks
  of 1000) are drawn by the BLOCK oracle's Philox4x32-10 and AS241 coefficient table
  (fixtures/unc/mcm/block/make_fixtures.py, loaded under its own name), and the model and
  inputs digests come from that oracle as well;
- the sample-set bytes are laid out here from the format pinned in the NODE.md of
  UNC.PROP.MCM.SAMP and the module documentation of fid-kunc `prop::mcm::samples`.

Bit exactness. A digest pins every bit of every sample, so the oracle must reproduce the
crate's doubles exactly, not within a few units in the last place. AS241's central region
uses only +, -, *, / (exact in Python's binary64). Its tails need ln: the crate's ln is the
published fdlibm `e_log.c` algorithm (fid-num facade), which is accurate to below 1 ulp but
not correctly rounded, so a correctly rounded 60-digit ln differs from it in the last place
for about 2.6 % of tail calls (and one linear2 sample changes). This file therefore ports the
published fdlibm algorithm and its published coefficients to Python binary64 arithmetic
(`ln_fdlibm`, no platform libm) and checks every call against the 60-digit decimal ln of the
BLOCK oracle: they must agree within 1 ulp. Square roots are correctly rounded from a 60-digit
decimal square root, as IEEE-754 requires of the crate's sqrt.

Format mcm-samples@1 (all integers little-endian):
  u64 H                         length of the header in bytes
  H bytes                       header, RFC 8785 JSON (sorted keys, no whitespace, every
                                integer a decimal string):
                                {"failed","inputs","model","outputs":[...],"profile","seed",
                                 "trials","version":"mcm-samples@1"}
  per output, in header order:  (trials - failed) f64, global trial order, failed omitted
  failed                        u64 global trial indices, ascending

Files (in this directory):
  linear2.sha256   AC-1: digest of the linear2 set (4 blocks of 10 000, seed 7)
  fail_fit.sha256  digest of the fail_fit set (2 blocks of 1000, seed 7), which has failed
                   trials
  expected.json    the header fields, header lengths and digests, sizes, the fail_fit failed
                   indices and the AC-2 would-be size (linear2 block 0 alone, 10 000 trials)

Usage: make_fixtures.py [--check]   (--check compares with the files on disk, writes nothing)
"""

import hashlib
import importlib.util
import json
import pathlib
import struct
import sys
from decimal import Decimal, getcontext

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[3]


def load(name, rel):
    spec = importlib.util.spec_from_file_location(name, ROOT / rel)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


block = load("mcm_block_oracle", "fixtures/unc/mcm/block/make_fixtures.py")
getcontext().prec = 60  # the loaded oracles set their own precision at import

FORMAT = "mcm-samples@1"
PROFILE = "sha256:" + hashlib.sha256(b"mcm-samp fixture profile").hexdigest()
SEED = 7

# --------------------------------------------------------------------------- arithmetic


def from_bits(b):
    return struct.unpack("<d", struct.pack("<Q", b))[0]


def bits(x):
    return struct.unpack("<Q", struct.pack("<d", x))[0]


# Published fdlibm e_log.c constants (hex words as printed there).
LN2_HI = from_bits(0x3FE62E42FEE00000)
LN2_LO = from_bits(0x3DEA39EF35793C76)
LG1, LG2, LG3, LG4, LG5, LG6, LG7 = (from_bits(w) for w in (
    0x3FE5555555555593, 0x3FD999999997FA04, 0x3FD2492494229359, 0x3FCC71C51D8E78AF,
    0x3FC7466496CB03DE, 0x3FC39A09D078C69F, 0x3FC2F112DF3E5244))


def ln_fdlibm(x):
    """fdlibm log for a normal x > 0, in binary64 (each Python op rounds once; no FMA)."""
    u = bits(x)
    hx = u >> 32
    assert 0x00100000 <= hx < 0x7FF00000 and x > 0.0
    if u == 0x3FF0000000000000:
        return 0.0
    hx = (hx + (0x3FF00000 - 0x3FE6A09E)) & 0xFFFFFFFF
    k = (hx >> 20) - 0x3FF
    hx = (hx & 0x000FFFFF) + 0x3FE6A09E
    f = from_bits((hx << 32) | (u & 0xFFFFFFFF)) - 1.0
    hfsq = 0.5 * f * f
    s = f / (2.0 + f)
    z = s * s
    w = z * z
    t1 = w * (LG2 + w * (LG4 + w * LG6))
    t2 = z * (LG1 + w * (LG3 + w * (LG5 + w * LG7)))
    r = t2 + t1
    dk = float(k)
    return (((s * (hfsq + r) + dk * LN2_LO) - hfsq) + f) + dk * LN2_HI


LN_CALLS = [0, 0]  # calls, calls differing from the correctly rounded ln


def ln(x):
    y = ln_fdlibm(x)
    exact = block.ln(x)  # 60-digit decimal ln rounded once
    LN_CALLS[0] += 1
    if y != exact:
        LN_CALLS[1] += 1
        assert abs(bits(y) - bits(exact)) <= 1, x
    return y


def sqrt(x):
    """Correctly rounded square root of a double >= 0 (60-digit decimal, rounded once)."""
    assert x >= 0.0
    return float(Decimal(x).sqrt())


AS = block.AS


def ppnd16(p):
    """AS241 PPND16 with the pinned evaluation order of UNC.MODEL.DIST."""
    q = p - 0.5
    if abs(q) <= 0.425:
        t = 0.180625 - q * q
        return q * block.horner(AS["a"], t) / block.horner(AS["b"], t)
    t = p if q < 0 else 1.0 - p
    t = sqrt(-ln(t))
    if t <= 5.0:
        t -= 1.6
        v = block.horner(AS["c"], t) / block.horner(AS["d"], t)
    else:
        t -= 5.0
        v = block.horner(AS["e"], t) / block.horner(AS["f"], t)
    return -v if q < 0 else v


def normal(mu, sigma, stream, t):
    return mu + sigma * ppnd16(block.open_unit(SEED, stream, t))


def rect(a, b, stream, t):
    return a + (b - a) * block.open_unit(SEED, stream, t)


# --------------------------------------------------------------------------- the trials

S = from_bits(0x3FE6A09E667F3BCD)  # sqrt(1/2) rounded once
COS8 = [1.0, S, 0.0, -S, -1.0, -S, 0.0, S]
SIN8 = [0.0, S, 1.0, S, 0.0, -S, -1.0, -S]


def circle_fit(r, offset):
    """The test kernel's circle fit (crates/fid-kunc/tests: 8 ring points, centroid, RMS
    radius, doubled), or None for the negative-radius failure."""
    if r < 0.0:
        return None
    xs = [offset + r * c for c in COS8]
    ys = [r * s for s in SIN8]
    cx, cy = 0.0, 0.0
    for x in xs:
        cx = cx + x
    for y in ys:
        cy = cy + y
    cx, cy = cx / 8.0, cy / 8.0
    ss = 0.0
    for x, y in zip(xs, ys):
        ss = ss + ((x - cx) * (x - cx) + (y - cy) * (y - cy))
    return 2.0 * sqrt(ss / 8.0)


def linear2_trials(n):
    (m1, s1), (m2, s2) = block.LINEAR2["x1"], block.LINEAR2["x2"]
    # Ordinals: x1 = 0, x2 = 1; the sum operator adds its terms in canonical edge order.
    return [normal(m1, s1, 0, t) + normal(m2, s2, 1, t) for t in range(n)], []


def fail_fit_trials(n):
    (mu, sigma), (a, b) = block.FAIL_FIT["radius"], block.FAIL_FIT["offset"]
    values, failed = [], []
    for t in range(n):
        # Ordinals: offset = 0, radius = 1. Both are drawn before the operator runs.
        r = normal(mu, sigma, 1, t)
        o = rect(a, b, 0, t)
        d = circle_fit(r, o)
        if d is None:
            failed.append(t)
        else:
            values.append(d)
    return values, failed


# --------------------------------------------------------------------------- the format


def header(model, trials, failed, outputs):
    doc = {
        "failed": str(failed),
        "inputs": block.inputs_digest(model),
        "model": block.model_digest(model),
        "outputs": outputs,
        "profile": PROFILE,
        "seed": str(SEED),
        "trials": str(trials),
        "version": FORMAT,
    }
    return block.canon(doc)


def encode(model, outputs, columns, failed, trials):
    h = header(model, trials, len(failed), outputs)
    out = bytearray(struct.pack("<Q", len(h)))
    out += h
    for col in columns:
        assert len(col) == trials - len(failed)
        for x in col:
            out += struct.pack("<d", x)
    for t in failed:
        out += struct.pack("<Q", t)
    return bytes(out), h


def case(model, outputs, trials_fn, trials):
    values, failed = trials_fn(trials)
    data, h = encode(model, outputs, [values], failed, trials)
    return {
        "digest": "sha256:" + hashlib.sha256(data).hexdigest(),
        "failed": [str(t) for t in failed],
        "header_digest": "sha256:" + hashlib.sha256(h).hexdigest(),
        "header_len": str(len(h)),
        "inputs_digest": block.inputs_digest(model),
        "model_digest": block.model_digest(model),
        "outputs": outputs,
        "size": str(len(data)),
        "trials": str(trials),
    }


def outputs():
    lin = case(block.linear2(), ["y"], linear2_trials, 4 * 10_000)
    ff = case(block.fail_fit(), ["d"], fail_fit_trials, 2 * 1000)
    assert ff["failed"], "fail_fit must have failed trials"
    # AC-2: linear2 block 0 alone, 10 000 trials, none failed.
    h = header(block.linear2(), 10_000, 0, ["y"])
    ac2_size = 8 + len(h) + 8 * 10_000
    assert ac2_size > 1024
    assert LN_CALLS[1] > 0, "the fdlibm port should differ from the correctly rounded ln somewhere"
    expected = {
        "format": FORMAT,
        "profile_digest": PROFILE,
        "seed": str(SEED),
        "ln_calls": [str(LN_CALLS[0]), str(LN_CALLS[1])],
        "linear2": lin,
        "fail_fit": ff,
        "ac2": {"cap": "1024", "trials": "10000", "size": str(ac2_size)},
    }
    return {
        "linear2.sha256": (lin["digest"] + "\n").encode("ascii"),
        "fail_fit.sha256": (ff["digest"] + "\n").encode("ascii"),
        "expected.json": (json.dumps(expected, indent=1, sort_keys=True, ensure_ascii=True) + "\n").encode("ascii"),
    }


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
        on_disk = {p.name for p in HERE.iterdir() if p.is_file() and p.suffix in (".json", ".sha256")}
        for extra in sorted(on_disk - set(files)):
            print("not generated: " + extra)
            bad += 1
    if bad:
        sys.exit(1)
    print("ok: %d files" % len(files))


if __name__ == "__main__":
    main()
