#!/usr/bin/env python3
"""Generate the golden vectors of TRUST.CHAIN.CANON (AC-1, AC-3, AC-4).

This is the independent reference: it shares no code with any Rust crate. It
parses with Python's `json` module and serialises with its own RFC 8785 (JCS)
rules written here:

- object members sorted by the UTF-16 code units of their names;
- strings with minimal escaping (`\\"`, `\\\\`, the five short control escapes,
  other controls below U+0020 as lowercase `\\u00xx`, everything else literal);
- numbers as ECMAScript Number::toString, built from Python's shortest
  round-trip `repr` digits.

All inputs are written by hand or drawn from a seeded PRNG in this file; no
third-party vector data is used (D-038). CC-BY-4.0 project data.

Outputs:
  vectors/<name>.json   input text (bytes as written here)
  vectors/<name>.canon  expected canonical bytes (no trailing newline)
  bad/<name>.json       inputs that must fail with canon.invalid (hand-labelled)
  entry/vectors.json    entry-header hash vectors and convention errors

Run:  python fixtures/canon/make_vectors.py
"""

import hashlib
import json
import math
import random
import struct
from decimal import Decimal
from pathlib import Path

HERE = Path(__file__).resolve().parent
MAX_DEPTH = 128


class Invalid(Exception):
    pass


# ---------------------------------------------------------------- reference

def _pairs(pairs):
    seen = set()
    for k, _ in pairs:
        if k in seen:
            raise Invalid("duplicate key " + repr(k))
        seen.add(k)
    return pairs  # keep a list of pairs (ordered, duplicates already refused)


def _const(name):
    raise Invalid("constant " + name)


def _check_str(s):
    try:
        s.encode("utf-8", errors="strict")
    except UnicodeEncodeError as e:
        raise Invalid("lone surrogate") from e


def es_number(x: float) -> str:
    """ECMAScript Number::toString for a finite double."""
    if x != x or x in (float("inf"), float("-inf")):
        raise Invalid("number range")
    if x == 0:
        return "0"
    if x < 0:
        return "-" + es_number(-x)
    t = Decimal(repr(x)).as_tuple()
    digits = "".join(map(str, t.digits)).lstrip("0")
    exp = t.exponent  # value = int(digits) * 10**exp (before stripping)
    stripped = digits.rstrip("0")
    exp += len(digits) - len(stripped)
    s, k = stripped, len(stripped)
    n = exp + k  # value = 0.s * 10**n
    if k <= n <= 21:
        return s + "0" * (n - k)
    if 0 < n <= 21:
        return s[:n] + "." + s[n:]
    if -6 < n <= 0:
        return "0." + "0" * (-n) + s
    e = n - 1
    sign = "+" if e >= 0 else "-"
    mant = s if k == 1 else s[0] + "." + s[1:]
    return mant + "e" + sign + str(abs(e))


SHORT = {'"': '\\"', "\\": "\\\\", "\b": "\\b", "\t": "\\t", "\n": "\\n", "\f": "\\f", "\r": "\\r"}


def es_string(s: str) -> str:
    out = ['"']
    for ch in s:
        if ch in SHORT:
            out.append(SHORT[ch])
        elif ord(ch) < 0x20:
            out.append("\\u%04x" % ord(ch))
        else:
            out.append(ch)
    out.append('"')
    return "".join(out)


def utf16_key(k: str) -> bytes:
    return k.encode("utf-16-be", errors="surrogatepass")


class Obj(list):
    """Object as an ordered list of pairs (distinguishes {} from [])."""


def _pairs_obj(pairs):
    return Obj(_pairs(pairs))


def canon_bytes(data: bytes) -> bytes:
    if len(data) > 64 * 1024 * 1024:
        raise Invalid("size")
    try:
        text = data.decode("utf-8", errors="strict")
    except UnicodeDecodeError as e:
        raise Invalid("utf8") from e
    if text.startswith("﻿"):
        raise Invalid("bom")
    try:
        value = json.loads(text, object_pairs_hook=_pairs_obj, parse_float=float, parse_int=float,
                           parse_constant=_const)
    except json.JSONDecodeError as e:
        raise Invalid("syntax") from e
    except RecursionError as e:
        raise Invalid("depth") from e
    return _emit(value, 0).encode("utf-8")


def _emit(v, depth):
    if isinstance(v, Obj):
        depth += 1
        if depth > MAX_DEPTH:
            raise Invalid("depth")
        for k, _ in v:
            _check_str(k)
        members = sorted(v, key=lambda kv: utf16_key(kv[0]))
        return "{" + ",".join(es_string(k) + ":" + _emit(x, depth) for k, x in members) + "}"
    if isinstance(v, list):
        depth += 1
        if depth > MAX_DEPTH:
            raise Invalid("depth")
        return "[" + ",".join(_emit(x, depth) for x in v) + "]"
    if isinstance(v, str):
        _check_str(v)
        return es_string(v)
    if isinstance(v, float):
        return es_number(v)
    if v is None:
        return "null"
    if v is True:
        return "true"
    if v is False:
        return "false"
    raise TypeError(type(v))


# ---------------------------------------------------------------- self-checks

def _selfcheck():
    # Hand-labelled ECMAScript formatting expectations (derived from the
    # Number::toString rules, not copied from any published vector file).
    table = [
        (0.0, "0"), (-0.0, "0"), (1.0, "1"), (-1.0, "-1"), (10.0, "10"), (0.5, "0.5"),
        (1e20, "100000000000000000000"), (1e21, "1e+21"), (1.5e21, "1.5e+21"),
        (1e-6, "0.000001"), (1e-7, "1e-7"), (1.25e-7, "1.25e-7"), (123.456, "123.456"),
        (2.0 ** 53, "9007199254740992"), (5e-324, "5e-324"),
        (1.7976931348623157e308, "1.7976931348623157e+308"), (0.1 + 0.2, "0.30000000000000004"),
    ]
    for x, want in table:
        got = es_number(x)
        assert got == want, (x, got, want)
    assert sorted(["b", "a", "", "\U0001f600"], key=utf16_key) == ["a", "b", "\U0001f600", ""]


# ---------------------------------------------------------------- vectors

GOOD = {}


def good(name, text):
    data = text.encode("utf-8") if isinstance(text, str) else text
    GOOD[name] = data


good("scalar_null", "null")
good("scalar_true", " true ")
good("scalar_false", "\n\tfalse\r\n")
good("scalar_empty_string", '""')
good("scalar_zero", "0")
good("scalar_negative_zero", "-0")
good("scalar_negative_zero_float", "-0.0e7")
good("empty_array", "[ ]")
good("empty_object", "{ }")
good("nested_empties", '[{}, [], {"x": []}, [{}]]')
good("whitespace_heavy", ' {\n  "zeta" :\t[ 1 , 2 ,3 ] ,\r\n "alpha":{ "q" : null , "p":true}\n}\n')

good("numbers_integers", "[1, -1, 7, 42, 100, 65536, -2147483648, 4294967295, 9007199254740991]")
good("numbers_large_integers",
     "[9007199254740992, 9007199254740993, 18446744073709551615, 123456789012345678901234567890]")
good("numbers_exponent_boundary", "[1e20, 1e21, 99999999999999999999, 999999999999999999999, 1.5E21, 12e19]")
good("numbers_small_boundary", "[0.000001, 0.0000012, 1e-6, 1e-7, 0.0000001, 9.5e-7, 1.0e-10]")
good("numbers_decimal_forms", "[1.0, 1.50, 10.000, 0.25, -0.125, 3.14159, 2.5e0, 25e-1, 0.0025e3, 1E+2, 1e-0]")
good("numbers_extremes", "[5e-324, -5e-324, 2.2250738585072014e-308, 1.7976931348623157e308, -1.7976931348623157e308]")
good("numbers_rounding", "[0.30000000000000004, 0.1, 0.2, 0.3, 1.0000000000000002, 0.9999999999999999, 4.35, 1.005]")
good("numbers_long_mantissa",
     "[3.141592653589793238462643383279, 0.10000000000000000555111512312578270211815834045, "
     "1.00000000000000011102230246251565404236316680908203125]")
good("numbers_powers_of_two", "[" + ", ".join(repr(2.0 ** e) for e in range(-30, 80, 7)) + "]")

good("strings_escapes", r'["quote\" backslash\\ slash\/ b\b f\f n\n r\r t\t"]')
good("strings_controls", '["' + "".join("\\u%04X" % c for c in range(0x20)) + '\\u007f"]')
good("strings_unicode_escapes", r'["éé 中    � ﻿"]')
good("strings_surrogate_pair_escape", r'["😀 𝄞"]')
good("strings_raw_utf8", '["café ß Ж 中文 \U0001f30d \U00010348 \u007f"]')
good("strings_ascii_printable", json.dumps("".join(chr(c) for c in range(0x20, 0x7f)), ensure_ascii=True))

good("keys_ascii_order", '{"b": 1, "a": 2, "B": 3, "A": 4, "_": 5, "10": 6, "9": 7, "": 8, " ": 9}')
good("keys_utf16_order",
     '{"\U0001f600": "astral", "": "private", "｡": "halfwidth", "é": "latin1", '
     '"e": "ascii", "€": "euro", "\U00010000": "linear-b", "ﬁ": "ligature"}')
good("keys_prefixes", '{"abc": 1, "ab": 2, "a": 3, "abcd": 4, "abd": 5, "aa": 6}')
good("keys_nested", '{"outer": {"z": {"y": 1, "x": 2}, "a": [ {"d": 4, "c": 3} ]}, "inner": {}}')
good("keys_escaped_names", r'{"a": 1, "\n": 2, "\"": 3, "\\": 4, "é": 5}')

good("depth_128_arrays", "[" * 128 + "]" * 128)
good("depth_128_mixed", '{"a":' * 64 + "[" * 64 + "1" + "]" * 64 + "}" * 64)

good("journal_like_header", """{
  "schema_version": 1, "kind": "command", "stream": "main", "seq": "41",
  "prev": "sha256:00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
  "branch": "main", "time": {"utc": "2026-11-17T09:30:00Z", "mono_ns": "123456789012", "sync": "ntp"},
  "principal": {"kind": "human", "commit": "sha256:ffeeddccbbaa99887766554433221100ffeeddccbbaa99887766554433221100"},
  "ai_assistance": "suggested",
  "command": {"id": "geo.fit.plane", "major": 1, "class": "R1", "determinism": "D0", "seed": "0",
              "inputs": [], "outputs": [], "algorithms": ["lsq-plane@1"]},
  "manifests": {"registry": "sha256:aa", "kernel": "sha256:bb"},
  "tolerance": "0x3f50624dd2f1a9fc"
}""")


def random_number_vectors():
    rng = random.Random(8785)
    for batch in range(4):
        xs = []
        while len(xs) < 64:
            bits = rng.getrandbits(64)
            (x,) = struct.unpack("<d", struct.pack("<Q", bits))
            if x != x or x in (float("inf"), float("-inf")):
                continue
            xs.append(x)
        good("numbers_random_bits_%d" % batch, "[" + ", ".join(repr(x) for x in xs) + "]")
    xs = [rng.uniform(-1e6, 1e6) for _ in range(64)] + [round(rng.uniform(0, 100), rng.randint(0, 6)) for _ in range(64)]
    good("numbers_random_moderate", "[" + ", ".join(repr(x) for x in xs) + "]")


random_number_vectors()


def tie_vectors():
    """Doubles exactly halfway between two shortest-length decimals.

    Number::toString picks the closest shortest digit string; on an exact tie
    ECMA-262's guideline (Note 2 of Number::toString) picks the even one, as
    Python's repr does. Inputs are written as exact decimals.
    """
    rng = random.Random(1316)
    found = [277575414244990.125]
    while len(found) < 16:
        x = rng.randrange(10 ** 13, 10 ** 16) + rng.choice([0.125, 0.375, 0.625, 0.875, 0.25, 0.75])
        t = Decimal(repr(x)).as_tuple()
        s = int("".join(map(str, t.digits)))
        here = abs(Decimal(s).scaleb(t.exponent) - Decimal(x))
        for nb in (s - 1, s + 1):
            other = Decimal(nb).scaleb(t.exponent)
            if float(other) == x and abs(other - Decimal(x)) == here:
                found.append(x)
                break
    good("numbers_exact_ties", "[" + ", ".join(str(Decimal(x)) for x in found) + "]")


tie_vectors()

# Every power of two, 2^-1074 .. 2^1023. Their rounding interval is asymmetric, so
# the nearest decimal of a given length is not always the one that round-trips.
good("numbers_every_power_of_two",
     "[" + ",\n".join(repr(math.ldexp(1.0, e)) for e in range(-1074, 1024)) + "]\n")

# Hand-labelled invalid inputs: every one must fail with canon.invalid.
BAD = {
    "duplicate_key": b'{"a": 1, "b": 2, "a": 3}',
    "duplicate_key_nested": b'{"x": [{"k": 1, "k": 1}]}',
    "duplicate_key_by_escape": b'{"a": 1, "\\u0061": 2}',
    "lone_high_surrogate": b'["\\ud800"]',
    "lone_low_surrogate": b'["\\udc00x"]',
    "lone_high_surrogate_key": b'{"\\ud83d": 1}',
    "reversed_surrogates": b'["\\ude00\\ud83d"]',
    "depth_129_arrays": b"[" * 129 + b"]" * 129,
    "depth_129_objects": b'{"a":' * 129 + b"1" + b"}" * 129,
    "number_1e400": b"[1e400]",
    "number_negative_1e400": b"[-1e400]",
    "number_huge_mantissa": b"[" + b"9" * 400 + b"]",
    "nan_literal": b"[NaN]",
    "infinity_literal": b"[Infinity]",
    "leading_zero": b"[01]",
    "trailing_comma_array": b"[1,]",
    "trailing_comma_object": b'{"a":1,}',
    "single_quotes": b"['a']",
    "raw_control_in_string": b'["a\tb"]',
    "invalid_utf8": b'["\xff"]',
    "overlong_utf8": b'["\xc0\xaf"]',
    "utf8_bom": b'\xef\xbb\xbf{"a":1}',
    "trailing_garbage": b'{"a":1} x',
    "two_values": b"1 2",
    "empty_input": b"",
    "unterminated_string": b'["abc',
    "bad_escape": b'["\\x41"]',
    "short_unicode_escape": b'["\\u12"]',
    "plus_sign_number": b"[+1]",
    "dot_without_digits": b"[1.]",
    "exponent_without_digits": b"[1e]",
    "bare_word": b"[nul]",
}


# ---------------------------------------------------------------- entry headers

def entry_hash(header_text: str) -> str:
    raw = canon_bytes(header_text.encode("utf-8"))  # validates
    obj = json.loads(raw.decode("utf-8"), object_pairs_hook=_pairs_obj, parse_float=float, parse_int=float)
    obj = Obj([(k, v) for k, v in obj if k != "hash"])
    return "sha256:" + hashlib.sha256(_emit(obj, 0).encode("utf-8")).hexdigest()


HEADER_BASE = {
    "schema_version": 1, "kind": "command", "stream": "main", "seq": "7",
    "prev": "sha256:" + "1f" * 32, "branch": "main", "logical_parent": None,
    "time": {"utc": "2026-11-17T10:00:00.000Z", "mono_ns": "18446744073709551615", "sync": "ptp"},
    "principal": {"kind": "agent", "commit": "sha256:" + "2e" * 32},
    "ai_assistance": "executed",
    "command": {"id": "msr.probe.touch", "major": 2, "class": "W1", "determinism": "D1", "seed": "9007199254740993",
                "inputs": [{"tag": "r1_result", "digest": "sha256:" + "3d" * 32}],
                "outputs": [{"entity": "ent-01", "digest": "sha256:" + "00" * 32}],
                "algorithms": ["probe-comp@3"]},
    "manifests": {"registry": "sha256:" + "4c" * 32, "kernel": "sha256:" + "5b" * 32},
    "radius_mm": "0x3fe0000000000000",
    "signatures": [],
}


def header(**changes):
    h = json.loads(json.dumps(HEADER_BASE))
    for path, value in changes.items():
        cur = h
        parts = path.split("__")
        for p in parts[:-1]:
            cur = cur[p]
        if value is DELETE:
            del cur[parts[-1]]
        else:
            cur[parts[-1]] = value
    return h


DELETE = object()


def entry_vectors():
    out = []

    def ok(name, text):
        out.append({"name": name, "header": text, "expect": {"hash": entry_hash(text)}})

    def conv(name, text, path):  # hand-labelled convention failure
        out.append({"name": name, "header": text, "expect": {"error": "canon.convention", "path": path}})

    def inval(name, text):
        out.append({"name": name, "header": text, "expect": {"error": "canon.invalid"}})

    base = json.dumps(HEADER_BASE, ensure_ascii=False)
    ok("base", base)
    ok("base_pretty_reordered", json.dumps(dict(reversed(list(HEADER_BASE.items()))), indent=3, ensure_ascii=True))
    ok("with_hash_field_ignored", json.dumps(header(hash="sha256:" + "ab" * 32)))
    ok("with_hash_null_ignored", json.dumps(header(hash=None)))
    ok("session_header", json.dumps({"schema_version": 1, "kind": "session_header", "stream": "main", "seq": "0",
                                     "prev": None, "app_version": "0.0.1", "ai_assistance": "none",
                                     "time": {"utc": "2026-11-17T08:00:00Z", "mono_ns": "0", "sync": "none"},
                                     "manifests": {"registry": "sha256:" + "aa" * 32, "kernel": "sha256:" + "bb" * 32}}))
    ok("unicode_reason", json.dumps(header(reason_commit="sha256:" + "6a" * 32, stream="méß-\U0001f4cf"),
                                    ensure_ascii=False))
    ok("max_safe_integer_number", json.dumps(header(schema_version=9007199254740991)))
    ok("negative_small_integer", json.dumps(header(command__major=-3)))

    conv("seq_as_number", json.dumps(header(seq=7)), "/seq")
    conv("mono_ns_as_raw_u64", base.replace('"mono_ns": "18446744073709551615"', '"mono_ns": 18446744073709551615'),
         "/time/mono_ns")
    conv("seed_as_number", json.dumps(header(command__seed=5)), "/command/seed")
    conv("seq_leading_zero_string", json.dumps(header(seq="007")), "/seq")
    conv("seq_overflow_string", json.dumps(header(seq="18446744073709551616")), "/seq")
    conv("seq_negative_string", json.dumps(header(seq="-1")), "/seq")
    conv("unsafe_integer_elsewhere", json.dumps(header(schema_version=9007199254740992)), "/schema_version")
    conv("raw_float_in_array", json.dumps(header(command__outputs=[{"entity": "e", "digest": "d", "w": 0.5}])),
         "/command/outputs/0/w")
    conv("raw_float_odd_key", json.dumps(header(**{"a/b~c": 1.25})), "/a~1b~0c")
    conv("header_not_object", "[1, 2]", "")
    inval("header_duplicate_key", base[:-1] + ', "seq": "8"}')
    inval("header_1e400", base[:-1] + ', "x": 1e400}')
    return out


def main():
    _selfcheck()
    vec = HERE / "vectors"
    bad = HERE / "bad"
    ent = HERE / "entry"
    for d in (vec, bad, ent):
        d.mkdir(exist_ok=True)
        for f in d.iterdir():
            f.unlink()
    for name, data in sorted(GOOD.items()):
        (vec / (name + ".json")).write_bytes(data)
        (vec / (name + ".canon")).write_bytes(canon_bytes(data))
    for name, data in sorted(BAD.items()):
        try:
            canon_bytes(data)
        except Invalid:
            pass
        else:
            raise SystemExit("reference accepted bad input " + name)
        (bad / (name + ".json")).write_bytes(data)
    entries = entry_vectors()
    (ent / "vectors.json").write_text(json.dumps(entries, indent=1, ensure_ascii=True) + "\n", encoding="utf-8",
                                      newline="\n")
    print(f"{len(GOOD)} vectors, {len(BAD)} bad inputs, {len(entries)} entry vectors")


if __name__ == "__main__":
    main()
