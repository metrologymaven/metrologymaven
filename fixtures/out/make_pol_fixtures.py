#!/usr/bin/env python3
"""Independent reference for KRN.OUT.POL (outlier policy types, validation and guards)
fixtures (CC-BY-4.0 project data).

Reads   fixtures/out/pol-params-v1/cases.in    hand-written cases with hand-labelled outcomes
Writes  fixtures/out/pol-params-v1/cases.txt   the same cases, values as binary64 hex, with
                                               expected outcomes, canonical encodings, sorted
                                               masks and mask digests

Nothing here runs the Rust code (stdlib only). It is a second implementation of the rules
pinned in the module documentation of `fid_filter::policy` (crates/fid-filter/src/policy/):

1. Parameter validation: field order, the reasons non_finite, subnormal, negative_zero,
   out_of_range, above_upper and below_min_points, and the primitive minimum point counts
   (the KRN.LSQ.TRAIT requirement table, copied by hand below).
2. The canonical encoding (compact JSON, fixed key order, f64 as 16 lowercase hex digits of
   its bit pattern, integers as decimal strings) and a strict decoder with its own error
   order (malformed, missing, wrong_type, bad_float, bad_integer, unknown_value,
   unexpected_key, duplicate_key, not_canonical).
3. Exclusion masks: range check in input order, sort, dedup, and the digest
   SHA-256(u32le(point_count) || u32le(index)...) from hashlib.
4. Guards: remaining < minimum points first, then excluded / count (one IEEE-754 division,
   which CPython performs in binary64 round-to-nearest-even) > max_excluded_fraction.

Every outcome computed here must equal the hand label in cases.in, so the labels and this
model check each other. The Rust test (crates/fid-filter/tests/policy.rs) reads cases.txt.

Run:  python fixtures/out/make_pol_fixtures.py          (writes the fixture)
      python fixtures/out/make_pol_fixtures.py --check  (fails if the fixture is stale)
"""

import hashlib
import math
import struct
import sys
from pathlib import Path

HERE = Path(__file__).parent
SRC = HERE / "pol-params-v1" / "cases.in"
OUT = HERE / "pol-params-v1" / "cases.txt"

MAX_RANSAC_ITERATIONS = 1_000_000
MAX_ENCODED_BYTES = 1024
MAX_OBJECT_NESTING = 3
U32 = 2**32 - 1
U64 = 2**64 - 1

# KRN.LSQ.TRAIT requirement table: fewest points per primitive.
MIN_POINTS = {
    "point": 1,
    "line2": 2, "line3": 2, "polyline": 2, "spline": 2,
    "plane": 3, "circle2": 3, "circle3": 3,
    "slot": 5, "rectangle": 5, "ellipse": 5,
    "sphere": 4, "cylinder": 5,
    "cone": 6, "paraboloid": 6,
    "torus": 7,
}

# Fields of each policy kind, in declaration (and encoding) order, with their types.
KINDS = {
    "nsigma": [("k", "pos"), ("side", "side")],
    "robust_scale": [("k", "pos"), ("scale", "scale"), ("side", "side")],
    "ransac": [("threshold_m", "pos"), ("iterations", "u32"), ("min_inliers", "u32"), ("seed", "u64")],
    "nominal_gate": [("max_distance_m", "pos")],
    "tolerance_band": [("lower_m", "signed"), ("upper_m", "signed")],
}
ENUMS = {"side": ("both", "positive", "negative"), "scale": ("mad", "qn")}


class Invalid(Exception):
    def __init__(self, field, reason):
        super().__init__(f"{field} {reason}")
        self.field = field
        self.reason = reason


def bits(x):
    return struct.pack(">d", x).hex()


def from_bits(h):
    return struct.unpack(">d", bytes.fromhex(h))[0]


def is_subnormal(x):
    return x != 0.0 and math.isfinite(x) and abs(x) < 2.2250738585072014e-308


def is_neg_zero(x):
    return x == 0.0 and math.copysign(1.0, x) < 0


# ---------------------------------------------------------------------------- validation

def validate(spec):
    kind, fields, fraction, target = spec["kind"], spec["fields"], spec["fraction"], spec["target"]
    for name, ty in KINDS[kind]:
        v = fields[name]
        if ty == "pos":
            if not math.isfinite(v):
                raise Invalid(name, "non_finite")
            if is_subnormal(v):
                raise Invalid(name, "subnormal")
            if not v > 0.0:
                raise Invalid(name, "out_of_range")
        elif ty == "signed":
            if not math.isfinite(v):
                raise Invalid(name, "non_finite")
            if is_subnormal(v):
                raise Invalid(name, "subnormal")
            if is_neg_zero(v):
                raise Invalid(name, "negative_zero")
        elif name == "iterations":
            if v < 1 or v > MAX_RANSAC_ITERATIONS:
                raise Invalid(name, "out_of_range")
        elif name == "min_inliers":
            if v < 1:
                raise Invalid(name, "out_of_range")
    if kind == "tolerance_band" and fields["lower_m"] > fields["upper_m"]:
        raise Invalid("lower_m", "above_upper")
    if not math.isfinite(fraction):
        raise Invalid("max_excluded_fraction", "non_finite")
    if is_subnormal(fraction):
        raise Invalid("max_excluded_fraction", "subnormal")
    if is_neg_zero(fraction):
        raise Invalid("max_excluded_fraction", "negative_zero")
    if fraction < 0.0 or fraction >= 1.0:
        raise Invalid("max_excluded_fraction", "out_of_range")
    if kind == "ransac" and fields["min_inliers"] < MIN_POINTS[target]:
        raise Invalid("min_inliers", "below_min_points")


def encode(spec):
    parts = [f'"kind":"{spec["kind"]}"']
    for name, ty in KINDS[spec["kind"]]:
        v = spec["fields"][name]
        if ty in ("pos", "signed"):
            parts.append(f'"{name}":"{bits(v)}"')
        else:
            parts.append(f'"{name}":"{v}"')
    return ('{"policy":{' + ",".join(parts) + '},"max_excluded_fraction":"' + bits(spec["fraction"])
            + '","target":"' + spec["target"] + '"}')


# ---------------------------------------------------------------------------- decoding

class Malformed(Exception):
    pass


def parse(text):
    """Strict parser: objects and strings only, no whitespace, printable ASCII strings
    without escapes, at most MAX_OBJECT_NESTING nested objects. Returns ('obj', [(k, v)])
    or ('str', s)."""
    pos = 0

    def string():
        nonlocal pos
        if pos >= len(text) or text[pos] != '"':
            raise Malformed()
        pos += 1
        start = pos
        while True:
            if pos >= len(text):
                raise Malformed()
            c = text[pos]
            if c == '"':
                s = text[start:pos]
                pos += 1
                return s
            if c == "\\" or not (0x20 <= ord(c) <= 0x7E):
                raise Malformed()
            pos += 1

    def value(level):
        nonlocal pos
        if pos < len(text) and text[pos] == "{":
            if level > MAX_OBJECT_NESTING:
                raise Malformed()
            pos += 1
            members = []
            if pos < len(text) and text[pos] == "}":
                pos += 1
                return ("obj", members)
            while True:
                k = string()
                if pos >= len(text) or text[pos] != ":":
                    raise Malformed()
                pos += 1
                members.append((k, value(level + 1)))
                if pos < len(text) and text[pos] == ",":
                    pos += 1
                    continue
                if pos < len(text) and text[pos] == "}":
                    pos += 1
                    return ("obj", members)
                raise Malformed()
        return ("str", string())

    v = value(1)
    if pos != len(text):
        raise Malformed()
    return v


def get(members, key):
    for k, v in members:
        if k == key:
            return v
    raise Invalid(key, "missing")


def get_str(members, key):
    v = get(members, key)
    if v[0] != "str":
        raise Invalid(key, "wrong_type")
    return v[1]


def get_float(members, key):
    s = get_str(members, key)
    if len(s) != 16 or any(c not in "0123456789abcdef" for c in s):
        raise Invalid(key, "bad_float")
    return from_bits(s)


def get_int(members, key, limit):
    s = get_str(members, key)
    if not s or any(c not in "0123456789" for c in s) or (len(s) > 1 and s[0] == "0"):
        raise Invalid(key, "bad_integer")
    v = int(s)
    if v > limit:
        raise Invalid(key, "out_of_range")
    return v


def check_keys(members, expected):
    if len(members) == len(expected):
        return
    for k, _ in members:
        if k not in expected:
            raise Invalid("encoding", "unexpected_key")
    raise Invalid("encoding", "duplicate_key")


def decode(text):
    if len(text.encode("utf-8")) > MAX_ENCODED_BYTES:
        raise Invalid("encoding", "too_long")
    try:
        top = parse(text)
    except Malformed:
        raise Invalid("encoding", "malformed") from None
    if top[0] != "obj":
        raise Invalid("encoding", "malformed")
    tm = top[1]
    pol = get(tm, "policy")
    if pol[0] != "obj":
        raise Invalid("policy", "wrong_type")
    pm = pol[1]
    kind = get_str(pm, "kind")
    if kind not in KINDS:
        raise Invalid("kind", "unknown_value")
    fields = {}
    for name, ty in KINDS[kind]:
        if ty in ("pos", "signed"):
            fields[name] = get_float(pm, name)
        elif ty == "u32":
            fields[name] = get_int(pm, name, U32)
        elif ty == "u64":
            fields[name] = get_int(pm, name, U64)
        else:
            s = get_str(pm, name)
            if s not in ENUMS[ty]:
                raise Invalid(name, "unknown_value")
            fields[name] = s
    check_keys(pm, ["kind"] + [n for n, _ in KINDS[kind]])
    fraction = get_float(tm, "max_excluded_fraction")
    target = get_str(tm, "target")
    if target not in MIN_POINTS:
        raise Invalid("target", "unknown_value")
    check_keys(tm, ["policy", "max_excluded_fraction", "target"])
    spec = {"kind": kind, "fields": fields, "fraction": fraction, "target": target}
    validate(spec)
    if encode(spec) != text:
        raise Invalid("encoding", "not_canonical")
    return spec


# ---------------------------------------------------------------------------- masks, guards

def parse_indices(s):
    out = []
    for part in [p for p in s.split(",") if p]:
        if ".." in part:
            a, b = part.split("..")
            out.extend(range(int(a), int(b)))
        else:
            out.append(int(part))
    return out


def make_mask(count, indices):
    for i in indices:
        if i >= count:
            return ("index_out_of_range", i)
    s = sorted(set(indices))
    pre = struct.pack("<I", count) + b"".join(struct.pack("<I", i) for i in s)
    return ("ok", s, "sha256:" + hashlib.sha256(pre).hexdigest())


def guard(target, fraction, count, indices):
    excluded = len(set(indices))
    remaining = count - excluded
    if remaining < MIN_POINTS[target]:
        return f"too_few_remaining {remaining} {MIN_POINTS[target]}"
    if excluded > 0:
        frac = excluded / count
        if frac > fraction:
            return f"excessive_exclusion {bits(frac)} {bits(fraction)}"
    return "ok"


# ---------------------------------------------------------------------------- driver

def kv(tokens):
    out = {}
    for t in tokens:
        k, _, v = t.partition("=")
        out[k] = v
    return out


def spec_from(kind, args):
    fields = {}
    for name, ty in KINDS[kind]:
        raw = args[name]
        fields[name] = float(raw) if ty in ("pos", "signed") else (raw if ty in ENUMS else int(raw))
    return {"kind": kind, "fields": fields, "fraction": float(args["fraction"]), "target": args["target"]}


def outcome_of(fn):
    try:
        return "ok", fn()
    except Invalid as e:
        return f"invalid_parameter {e.field} {e.reason}", None


def fail(msg):
    raise SystemExit(f"make_pol_fixtures: {msg}")


def build():
    lines = SRC.read_text(encoding="utf-8").replace("\r\n", "\n").split("\n")
    out = [
        "# KRN.OUT.POL AC-1..AC-3: parameter sets, decoding, masks and guards.",
        "# Generated by fixtures/out/make_pol_fixtures.py from cases.in (CC-BY-4.0). Do not edit.",
        "# f64 values are binary64 bit patterns in hex. spec: typed parameters, expected",
        "# validation outcome and (if ok) the canonical encoding. decode: input bytes on the",
        "# 'input' line (everything after 'input '; a bare 'input' is empty), expected outcome. mask: point count and raw",
        "# indices, expected sorted unique indices and digest. guard: policy nsigma k=3 side=both.",
    ]
    i = 0
    seen = set()
    while i < len(lines):
        line = lines[i]
        i += 1
        if not line.strip() or line.startswith("#"):
            continue
        head, sep, label = line.partition(" => ")
        if not sep:
            fail(f"no outcome label: {line!r}")
        tok = head.split()
        what, name = tok[0], tok[1]
        if name in seen:
            fail(f"duplicate case name {name}")
        seen.add(name)
        if what == "spec":
            kind = tok[2]
            spec = spec_from(kind, kv(tok[3:]))
            got, _ = outcome_of(lambda: validate(spec))
            if got != label:
                fail(f"{name}: label {label!r}, model {got!r}")
            out.append(f"spec {name}")
            out.append(f"kind {kind}")
            for fname, ty in KINDS[kind]:
                v = spec["fields"][fname]
                out.append(f"{fname} {bits(v) if ty in ('pos', 'signed') else v}")
            out.append(f"fraction {bits(spec['fraction'])}")
            out.append(f"target {spec['target']}")
            out.append(f"expect {got}")
            if got == "ok":
                out.append(f"canonical {encode(spec)}")
            out.append("end")
        elif what == "decode":
            if i >= len(lines):
                fail(f"{name}: missing input line")
            text = lines[i]
            i += 1
            got, spec = outcome_of(lambda: decode(text))
            if got != label:
                fail(f"{name}: label {label!r}, model {got!r}")
            out.append(f"decode {name}")
            out.append("input" + (f" {text}" if text else ""))
            out.append(f"expect {got}")
            if got == "ok":
                out.append(f"canonical {encode(spec)}")
            out.append("end")
        elif what == "mask":
            args = kv(tok[2:])
            count, idx = int(args["count"]), parse_indices(args["indices"])
            res = make_mask(count, idx)
            got = "ok" if res[0] == "ok" else f"index_out_of_range {res[1]}"
            if got != label:
                fail(f"{name}: label {label!r}, model {got!r}")
            out.append(f"mask {name}")
            out.append(f"count {count}")
            out.append("indices" + "".join(f" {x}" for x in idx))
            out.append(f"expect {got}")
            if res[0] == "ok":
                out.append("sorted" + "".join(f" {x}" for x in res[1]))
                out.append(f"digest {res[2]}")
            out.append("end")
        elif what == "guard":
            args = kv(tok[2:])
            count, idx = int(args["count"]), parse_indices(args["indices"])
            fraction = float(args["fraction"])
            got = guard(args["target"], fraction, count, idx)
            want = label
            if label.startswith("excessive_exclusion"):
                _, a, b = label.split()
                want = f"excessive_exclusion {bits(float(a))} {bits(float(b))}"
            if got != want:
                fail(f"{name}: label {want!r}, model {got!r}")
            out.append(f"guard {name}")
            out.append(f"target {args['target']}")
            out.append(f"fraction {bits(fraction)}")
            out.append(f"count {count}")
            out.append("indices" + "".join(f" {x}" for x in idx))
            out.append(f"expect {got}")
            out.append("end")
        else:
            fail(f"unknown case type {what!r}")
    return "\n".join(out) + "\n"


def main():
    text = build()
    if "--check" in sys.argv[1:]:
        old = OUT.read_text(encoding="utf-8").replace("\r\n", "\n") if OUT.exists() else ""
        if old != text:
            print(f"stale: {OUT}", file=sys.stderr)
            return 1
        print("ok")
        return 0
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(text, encoding="utf-8", newline="\n")
    print(f"wrote {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
