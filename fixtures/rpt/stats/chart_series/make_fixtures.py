#!/usr/bin/env python3
"""Independent reference for RPT.STATS.TREND.CONTRACT (the ChartSeries contract, major 1), stdlib only.

Writes (CC-BY-4.0 project data, hand-written examples, no third-party data):
  crates/fid-chart-series/schema/chart-series.schema.json   the JSON Schema 2020-12 of contract 1
  fixtures/rpt/stats/chart_series/v1_examples/<kind>.json    one valid series per kind (AC-1),
                                                             RFC 8785 canonical bytes, no newline
  fixtures/rpt/stats/chart_series/v1_examples/trend_minor9.json  minor 9 with one unknown field (AC-3)
  fixtures/rpt/stats/chart_series/v2_trend.json              a major-2 series (AC-2)
  fixtures/rpt/stats/chart_series/cases.json                 hand-labelled accept and refuse cases
  fixtures/rpt/stats/chart_series/expected.json              digests and outcomes of the examples

Nothing here is produced by running the Rust crate `fid-chart-series`.

The script carries three independent readings of the contract and refuses to write if any of
them disagrees with a hand label:
  1. its own canonical JSON writer (json.dumps with sorted keys and no whitespace; every key
     is ASCII, so code-point order is the RFC 8785 UTF-16 order) and SHA-256 (hashlib);
  2. a small JSON Schema 2020-12 validator for exactly the keywords the schema uses (an unknown
     keyword is an error, so the schema cannot silently use something this validator skips);
  3. a checker of the whole contract written from the leaf's rules and the conventions
     documented in crates/fid-chart-series/src/lib.rs, giving the first refusal as
     (code, field, reason).
Each refusal case has exactly one fault, so the order of checks inside (3) does not decide
the label. `schema_valid` labels say whether the JSON Schema alone accepts the case: the
schema cannot say everything (marker and reference-line order, the value-axis unit, the
histogram rules, the forbidden display characters), and those cases are labelled
schema_valid = true with a refusal from the contract.

Check: python fixtures/rpt/stats/chart_series/make_fixtures.py --check
"""

import copy
import hashlib
import json
import re
import struct
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
SCHEMA_FILE = ROOT / "crates" / "fid-chart-series" / "schema" / "chart-series.schema.json"
EXAMPLES = HERE / "v1_examples"

# ---------------------------------------------------------------- contract 1.0 constants

MAJOR = 1
MINOR = 0
MAX_MINOR = 999_999
MAX_POINTS = 1_000_000
MAX_REF_LINES = 64
MAX_MARKERS = 64
MAX_SET_VERSION = 999_999
MAX_LABEL_CHARS = 256
MAX_COUNT = 2 ** 53
MAX_CONSTANTS = 16
MAX_UNVERIFIED = 64
MAX_EXCLUDED = MAX_POINTS
MAX_SOURCE_INDEX = 2 ** 32 - 1

KINDS = ["TREND", "HISTOGRAM", "CONTROL_XBAR", "CONTROL_R", "CONTROL_S", "CONTROL_I", "CONTROL_MR",
         "VECTOR_GROUP"]
CONTROL_KINDS = [k for k in KINDS if k.startswith("CONTROL_")]
REF_KINDS = ["NOMINAL", "LSL", "USL", "CENTRE", "LCL", "UCL"]
LIMIT_REF_KINDS = ["CENTRE", "LCL", "UCL"]
MARKER_KINDS = ["OUT_OF_TOLERANCE", "INDETERMINATE", "RULE_VIOLATION"]
# fid_stats::ReviewStatus wire names, kept distinct (D-045).
REVIEWS = ["UNVERIFIED", "CLAIMED_BY_FILE", "EXPERT_REVIEWED"]
STATUSES = ["OK", "INSUFFICIENT_DATA"]
UNVERIFIED_KINDS = ["CONSTANTS", "RULE_SET"]
# The GEO.FRAME.UNITS catalogue, symbols as written there (case-sensitive ASCII).
LENGTH_UNITS = ["m", "cm", "mm", "um", "nm", "in", "ft", "thou"]
ANGLE_UNITS = ["rad", "deg", "arcmin", "arcsec", "gon"]
QUANTITIES = {"LENGTH": LENGTH_UNITS, "ANGLE": ANGLE_UNITS}

SEMVER_PART = r"(0|[1-9][0-9]{0,8})"
PAT = {
    "f64": r"^0x(?!7ff|fff)[0-9a-f]{16}$",
    "digest": r"^sha256:(?!0{64}$)[0-9a-f]{64}$",
    "algorithm": r"^[a-z][a-z0-9_-]*(\.[a-z][a-z0-9_-]*){2,}@" + SEMVER_PART + r"\." + SEMVER_PART + r"\."
                 + SEMVER_PART + r"$",
    "kernel_version": r"^" + SEMVER_PART + r"\." + SEMVER_PART + r"\." + SEMVER_PART + r"$",
    "id": r"^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$",
    "binning_rule": r"^[a-z][a-z0-9_]{0,55}@[1-9][0-9]{0,5}$",
    "set_id": r"^[a-z][a-z0-9_]{0,63}$",
    "rule_id": r"^[a-z0-9][a-z0-9_]{0,31}$",
    "label_key": r"^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)*$",
    "constants": r"^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)*@" + SEMVER_PART + r"\." + SEMVER_PART + r"\."
                 + SEMVER_PART + r"$",
    "exclusion_reason": r"^[A-Z][A-Z0-9_]{0,63}$",
}
MAX_CONSTANTS_REF_BYTES = 128
MAX_ALGORITHM_BYTES = 128
MAX_LABEL_KEY_BYTES = 128

# fid-types text::FORBIDDEN_RANGES (Unicode 16.0), plus every plane-end noncharacter.
FORBIDDEN = [
    (0x0000, 0x001F), (0x007F, 0x009F), (0x00AD, 0x00AD), (0x0600, 0x0605), (0x061C, 0x061C),
    (0x06DD, 0x06DD), (0x070F, 0x070F), (0x0890, 0x0891), (0x08E2, 0x08E2), (0x180E, 0x180E),
    (0x200B, 0x200F), (0x2028, 0x2028), (0x2029, 0x2029), (0x202A, 0x202E), (0x2060, 0x2064),
    (0x2066, 0x206F), (0xFDD0, 0xFDEF), (0xFEFF, 0xFEFF), (0xFFF9, 0xFFFB), (0x110BD, 0x110BD),
    (0x110CD, 0x110CD), (0x13430, 0x1343F), (0x1BCA0, 0x1BCA3), (0x1D173, 0x1D17A),
    (0xE0000, 0xE007F),
]


def forbidden(ch):
    u = ord(ch)
    return (u & 0xFFFE) == 0xFFFE or any(lo <= u <= hi for lo, hi in FORBIDDEN)


# ---------------------------------------------------------------- canonical JSON and numbers

def no_floats(v):
    if isinstance(v, float):
        raise AssertionError("a float in a document: write f64 values as hex bits")
    if isinstance(v, dict):
        for x in v.values():
            no_floats(x)
    if isinstance(v, list):
        for x in v:
            no_floats(x)


def canon(v):
    no_floats(v)
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False,
                      allow_nan=False).encode("utf-8")


def sha(b):
    return "sha256:" + hashlib.sha256(b).hexdigest()


def hexf(v):
    return "0x" + struct.pack(">d", v).hex()


def bits(h):
    return struct.unpack(">q", bytes.fromhex(h[2:]))[0]


def total_key(h):
    """IEEE 754 totalOrder key of a hex-bits f64 (negative values reversed)."""
    b = bits(h)
    return b ^ 0x7FFFFFFFFFFFFFFF if b < 0 else b


def value(h):
    return struct.unpack(">d", bytes.fromhex(h[2:]))[0]


# ---------------------------------------------------------------- the JSON Schema (written here)

def s_ref(name):
    return {"$ref": "#/$defs/" + name}


def s_str(pattern=None, max_len=None, min_len=None):
    out = {"type": "string"}
    if pattern:
        out["pattern"] = pattern
    if min_len is not None:
        out["minLength"] = min_len
    if max_len is not None:
        out["maxLength"] = max_len
    return out


def s_obj(required, props, strict):
    out = {"type": "object", "required": sorted(required), "properties": props}
    if strict:
        out["additionalProperties"] = False
    return out


def schema():
    defs = {
        "f64": dict(s_str(PAT["f64"]), description="IEEE 754 binary64 bits: 0x and 16 lowercase hex "
                    "digits; NaN and infinities refused."),
        "digest": dict(s_str(PAT["digest"]), description="sha256: and 64 lowercase hex digits; the "
                       "all-zero tombstone digest refused."),
        "algorithm": s_str(PAT["algorithm"], MAX_ALGORITHM_BYTES),
        "kernel_version": s_str(PAT["kernel_version"]),
        "id": s_str(PAT["id"]),
        "binning_rule": s_str(PAT["binning_rule"]),
        "set_id": s_str(PAT["set_id"]),
        "rule_id": s_str(PAT["rule_id"]),
        "label_key": s_str(PAT["label_key"], MAX_LABEL_KEY_BYTES),
        "x_label": dict(s_str(None, MAX_LABEL_CHARS, 1), description="Display text; the contract "
                        "also refuses the fid-types forbidden code points, which this schema does "
                        "not express."),
        "unit": {"enum": LENGTH_UNITS + ANGLE_UNITS},
        "review": {"enum": REVIEWS},
        "constants": s_str(PAT["constants"], MAX_CONSTANTS_REF_BYTES),
        "set_version": {"type": "integer", "minimum": 1, "maximum": MAX_SET_VERSION},
    }
    for strict, tag in ((True, "strict"), (False, "open")):
        defs["contract_" + tag] = s_obj(["major", "minor"], {
            "major": {"type": "integer", "const": MAJOR},
            "minor": {"type": "integer", "minimum": 0, "maximum": MAX_MINOR},
        }, strict)
        defs["axis_" + tag] = {"oneOf": [
            s_obj(["kind", "label_key"], {"kind": {"const": "ORDINAL"}, "label_key": s_ref("label_key")},
                  strict),
            s_obj(["kind", "label_key", "quantity", "unit"], {
                "kind": {"const": "QUANTITY"}, "label_key": s_ref("label_key"),
                "quantity": {"const": "LENGTH"}, "unit": {"enum": LENGTH_UNITS}}, strict),
            s_obj(["kind", "label_key", "quantity", "unit"], {
                "kind": {"const": "QUANTITY"}, "label_key": s_ref("label_key"),
                "quantity": {"const": "ANGLE"}, "unit": {"enum": ANGLE_UNITS}}, strict),
        ]}
        defs["marker_" + tag] = {"oneOf": [
            s_obj(["kind"], {"kind": {"const": "OUT_OF_TOLERANCE"}}, strict),
            s_obj(["kind"], {"kind": {"const": "INDETERMINATE"}}, strict),
            s_obj(["kind", "review", "rule_id", "set_id", "set_version"], {
                "kind": {"const": "RULE_VIOLATION"}, "set_id": s_ref("set_id"),
                "set_version": s_ref("set_version"), "rule_id": s_ref("rule_id"),
                "review": s_ref("review")}, strict),
        ]}
        defs["unverified_" + tag] = {"oneOf": [
            s_obj(["kind", "ref"], {"kind": {"const": "CONSTANTS"}, "ref": s_ref("constants")}, strict),
            s_obj(["kind", "set_id", "set_version"], {
                "kind": {"const": "RULE_SET"}, "set_id": s_ref("set_id"),
                "set_version": s_ref("set_version")}, strict),
        ]}
        defs["exclusion_" + tag] = s_obj(["reason", "source_index"], {
            "source_index": {"type": "integer", "minimum": 0, "maximum": MAX_SOURCE_INDEX},
            "reason": s_str(PAT["exclusion_reason"])}, strict)
        defs["point_" + tag] = s_obj(["markers", "x", "y"], {
            "x": s_ref("f64"), "y": s_ref("f64"), "x_upper": s_ref("f64"), "x_label": s_ref("x_label"),
            "record": s_ref("id"), "piece": s_ref("id"),
            "markers": {"type": "array", "maxItems": MAX_MARKERS, "items": s_ref("marker_" + tag)},
        }, strict)
        defs["ref_line_" + tag] = s_obj(["kind", "unit", "value"], {
            "kind": {"enum": REF_KINDS}, "value": s_ref("f64"), "unit": s_ref("unit")}, strict)
        defs["provenance_" + tag] = s_obj(
            ["algorithm", "constants", "dataset_digest", "excluded", "kernel_version", "unverified"], {
                "dataset_digest": s_ref("digest"), "algorithm": s_ref("algorithm"),
                "kernel_version": s_ref("kernel_version"), "frozen_limits": s_ref("id"),
                "binning_rule": s_ref("binning_rule"),
                "constants": {"type": "array", "maxItems": MAX_CONSTANTS, "items": s_ref("constants")},
                "unverified": {"type": "array", "maxItems": MAX_UNVERIFIED, "items": s_ref("unverified_" + tag)},
                "excluded": {"type": "array", "maxItems": MAX_EXCLUDED, "items": s_ref("exclusion_" + tag)},
            }, strict)
        defs["series_" + tag] = s_obj(
            ["contract", "kind", "points", "provenance", "ref_lines", "status", "x", "y"], {
            "contract": s_ref("contract_" + tag), "kind": {"enum": KINDS}, "status": {"enum": STATUSES},
            "x": s_ref("axis_" + tag), "y": s_ref("axis_" + tag),
            "points": {"type": "array", "maxItems": MAX_POINTS, "items": s_ref("point_" + tag)},
            "ref_lines": {"type": "array", "maxItems": MAX_REF_LINES, "items": s_ref("ref_line_" + tag)},
            "provenance": s_ref("provenance_" + tag)}, strict)
    return {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "urn:metrologymaven:schema:chart-series:1",
        "title": "ChartSeries contract, major 1 (RPT.STATS.TREND.CONTRACT)",
        "description": "One chart data series handed to report chart blocks and viewers. Contract "
                       "minor 0 is closed; a series of a later minor (1 and up) of major 1 may carry "
                       "optional fields a 1.0 consumer ignores (REQ-006). The crate fid-chart-series "
                       "is stricter than this schema: markers and reference lines in canonical order, "
                       "reference-line units equal to the value axis, histogram counts, "
                       "kind-specific markers, lines and provenance fields, the status rules, the "
                       "review cross-checks between markers and provenance.unverified, -0.0 "
                       "reference lines and forbidden display characters. "
                       "Written by fixtures/rpt/stats/chart_series/make_fixtures.py.",
        "type": "object",
        "required": ["contract"],
        "properties": {"contract": {"type": "object", "required": ["minor"]}},
        "if": {"properties": {"contract": {"properties": {"minor": {"const": 0}}}}},
        "then": s_ref("series_strict"),
        "else": s_ref("series_open"),
        "$defs": defs,
    }


# ---------------------------------------------------------------- a small JSON Schema validator

KNOWN_KEYWORDS = {"$schema", "$id", "$defs", "$ref", "title", "description", "type", "const", "enum",
                  "required", "properties", "additionalProperties", "items", "minItems", "maxItems",
                  "pattern", "minLength", "maxLength", "minimum", "maximum", "oneOf", "if", "then",
                  "else"}


def is_int(x):
    return (isinstance(x, int) and not isinstance(x, bool)) or (isinstance(x, float) and x.is_integer())


def is_num(x):
    return isinstance(x, (int, float)) and not isinstance(x, bool)


def json_eq(a, b):
    if is_num(a) and is_num(b):
        return a == b
    if type(a) is not type(b):
        return False
    if isinstance(a, dict):
        return a.keys() == b.keys() and all(json_eq(a[k], b[k]) for k in a)
    if isinstance(a, list):
        return len(a) == len(b) and all(json_eq(x, y) for x, y in zip(a, b))
    return a == b


TYPES = {
    "object": lambda x: isinstance(x, dict), "array": lambda x: isinstance(x, list),
    "string": lambda x: isinstance(x, str), "integer": is_int, "number": is_num,
    "boolean": lambda x: isinstance(x, bool), "null": lambda x: x is None,
}


def sv(root, s, x):
    unknown = set(s) - KNOWN_KEYWORDS
    if unknown:
        raise AssertionError("schema keyword not implemented here: %s" % sorted(unknown))
    if "$ref" in s:
        ref = s["$ref"]
        assert ref.startswith("#/$defs/"), ref
        if not sv(root, root["$defs"][ref[len("#/$defs/"):]], x):
            return False
    if "type" in s and not TYPES[s["type"]](x):
        return False
    if "const" in s and not json_eq(s["const"], x):
        return False
    if "enum" in s and not any(json_eq(e, x) for e in s["enum"]):
        return False
    if isinstance(x, dict):
        if any(k not in x for k in s.get("required", [])):
            return False
        props = s.get("properties", {})
        for k, v in x.items():
            if k in props:
                if not sv(root, props[k], v):
                    return False
            elif s.get("additionalProperties", True) is False:
                return False
    if isinstance(x, list):
        if len(x) < s.get("minItems", 0) or len(x) > s.get("maxItems", len(x)):
            return False
        if "items" in s and not all(sv(root, s["items"], v) for v in x):
            return False
    if isinstance(x, str):
        if "pattern" in s and not re.search(s["pattern"], x):
            return False
        if len(x) < s.get("minLength", 0) or len(x) > s.get("maxLength", len(x)):
            return False
    if is_num(x):
        if "minimum" in s and x < s["minimum"]:
            return False
        if "maximum" in s and x > s["maximum"]:
            return False
    if "oneOf" in s and sum(1 for b in s["oneOf"] if sv(root, b, x)) != 1:
        return False
    if "if" in s:
        branch = s.get("then") if sv(root, s["if"], x) else s.get("else")
        if branch is not None and not sv(root, branch, x):
            return False
    return True


def walk_keywords(s):
    """Every keyword anywhere in the schema must be one this validator implements."""
    assert isinstance(s, dict), s
    unknown = set(s) - KNOWN_KEYWORDS
    assert not unknown, sorted(unknown)
    for k in ("properties", "$defs"):
        for v in s.get(k, {}).values():
            walk_keywords(v)
    for k in ("items", "if", "then", "else"):
        if k in s:
            walk_keywords(s[k])
    if isinstance(s.get("additionalProperties"), dict):
        walk_keywords(s["additionalProperties"])
    for v in s.get("oneOf", []):
        walk_keywords(v)


def schema_valid(sch, text):
    try:
        doc = load(text)
    except ValueError:
        return False
    return sv(sch, sch, doc)


# ---------------------------------------------------------------- the contract checker

class Refuse(Exception):
    def __init__(self, field, reason):
        super().__init__(field, reason)
        self.field = field
        self.reason = reason


class Unsupported(Exception):
    def __init__(self, major, minor, kind):
        super().__init__(major, minor, kind)
        self.major, self.minor, self.kind = major, minor, kind


def no_dupes(pairs):
    out = {}
    for k, v in pairs:
        if k in out:
            raise ValueError("duplicate_key")
        out[k] = v
    return out


def bad_constant(_):
    raise ValueError("syntax")


def load(text):
    try:
        return json.loads(text, object_pairs_hook=no_dupes, parse_constant=bad_constant)
    except json.JSONDecodeError as e:
        raise ValueError("syntax") from e


class Ctx:
    def __init__(self, lenient):
        self.lenient = lenient
        self.ignored = []


def obj(ctx, x, at, required, optional):
    if not isinstance(x, dict):
        raise Refuse(at, "wrong_type")
    for k in sorted(x):
        if k not in required and k not in optional:
            name = k if at == "$" else at + "." + k
            if ctx.lenient:
                ctx.ignored.append(name)
            else:
                raise Refuse(name, "unknown_field")
    for k in required:
        if k not in x:
            raise Refuse(k if at == "$" else at + "." + k, "missing")
    return x


def sub(at, k):
    return k if at == "$" else at + "." + k


def integer(x, at, lo, hi):
    if not is_int(x):
        raise Refuse(at, "not_integer")
    if not lo <= x <= hi:
        raise Refuse(at, "out_of_range")
    return int(x)


def string(x, at):
    if not isinstance(x, str):
        raise Refuse(at, "wrong_type")
    return x


def pattern(x, at, name, max_len=None):
    string(x, at)
    if not re.search(PAT[name], x) or (max_len is not None and len(x.encode("utf-8")) > max_len):
        raise Refuse(at, "bad_format")
    return x


def f64(x, at):
    string(x, at)
    if not re.fullmatch(r"0x[0-9a-f]{16}", x):
        raise Refuse(at, "bad_hex")
    if x[2:5] in ("7ff", "fff"):
        raise Refuse(at, "non_finite")
    return x


def enum(x, at, values):
    string(x, at)
    if x not in values:
        raise Refuse(at, "unknown_value")
    return x


def axis(ctx, x, at):
    if not isinstance(x, dict):
        raise Refuse(at, "wrong_type")
    if "kind" not in x:
        raise Refuse(at + ".kind", "missing")
    kind = enum(x["kind"], at + ".kind", ["ORDINAL", "QUANTITY"])
    if kind == "ORDINAL":
        obj(ctx, x, at, ["kind", "label_key"], [])
        pattern(x["label_key"], at + ".label_key", "label_key", MAX_LABEL_KEY_BYTES)
        return ("ORDINAL", None)
    obj(ctx, x, at, ["kind", "label_key", "quantity", "unit"], [])
    pattern(x["label_key"], at + ".label_key", "label_key", MAX_LABEL_KEY_BYTES)
    q = enum(x["quantity"], at + ".quantity", list(QUANTITIES))
    u = enum(x["unit"], at + ".unit", LENGTH_UNITS + ANGLE_UNITS)
    if u not in QUANTITIES[q]:
        raise Refuse(at + ".unit", "unit_mismatch")
    return ("QUANTITY", u)


def marker_key(m):
    rank = MARKER_KINDS.index(m["kind"])
    if m["kind"] != "RULE_VIOLATION":
        return (rank, b"", 0, b"")
    return (rank, m["set_id"].encode(), m["set_version"], m["rule_id"].encode())


def point(ctx, x, at, kind, status):
    obj(ctx, x, at, ["markers", "x", "y"], ["piece", "record", "x_label", "x_upper"])
    px = f64(x["x"], at + ".x")
    py = f64(x["y"], at + ".y")
    if "x_upper" in x:
        f64(x["x_upper"], at + ".x_upper")
    if "x_label" in x:
        lab = string(x["x_label"], at + ".x_label")
        if not 1 <= len(lab) <= MAX_LABEL_CHARS or any(forbidden(c) for c in lab):
            raise Refuse(at + ".x_label", "bad_format")
    for k in ("record", "piece"):
        if k in x:
            pattern(x[k], at + "." + k, "id")
    ms = x["markers"]
    if not isinstance(ms, list):
        raise Refuse(at + ".markers", "wrong_type")
    if len(ms) > MAX_MARKERS:
        raise Refuse(at + ".markers", "too_many")
    for j, m in enumerate(ms):
        mat = "%s.markers[%d]" % (at, j)
        if not isinstance(m, dict):
            raise Refuse(mat, "wrong_type")
        if "kind" not in m:
            raise Refuse(mat + ".kind", "missing")
        mk = enum(m["kind"], mat + ".kind", MARKER_KINDS)
        if mk == "RULE_VIOLATION":
            obj(ctx, m, mat, ["kind", "review", "rule_id", "set_id", "set_version"], [])
            pattern(m["set_id"], mat + ".set_id", "set_id")
            integer(m["set_version"], mat + ".set_version", 1, MAX_SET_VERSION)
            pattern(m["rule_id"], mat + ".rule_id", "rule_id")
            enum(m["review"], mat + ".review", REVIEWS)
        else:
            obj(ctx, m, mat, ["kind"], [])
    for j in range(1, len(ms)):
        if not marker_key(ms[j - 1]) < marker_key(ms[j]):
            raise Refuse("%s.markers[%d]" % (at, j), "not_canonical_order")
    kinds = [m["kind"] for m in ms]
    if "OUT_OF_TOLERANCE" in kinds and "INDETERMINATE" in kinds:
        raise Refuse(at + ".markers", "conflicting_markers")
    for j, m in enumerate(ms):
        if m["kind"] == "RULE_VIOLATION":
            if kind not in CONTROL_KINDS:
                raise Refuse("%s.markers[%d]" % (at, j), "not_applicable")
            if status == "INSUFFICIENT_DATA":
                raise Refuse("%s.markers[%d]" % (at, j), "status_conflict")
    if "x_upper" in x and kind != "HISTOGRAM":
        raise Refuse(at + ".x_upper", "not_applicable")
    if kind == "HISTOGRAM":
        c = value(py)
        if bits(py) < 0 or not c.is_integer() or c > MAX_COUNT:
            raise Refuse(at + ".y", "bad_count")
        if "x_upper" in x and total_key(x["x_upper"]) < total_key(px):
            raise Refuse(at + ".x_upper", "upper_below_lower")


def check(text):
    """('ok', ignored, doc) or raises Refuse / Unsupported."""
    try:
        doc = load(text)
    except ValueError as e:
        raise Refuse("$", str(e)) from e
    if not isinstance(doc, dict):
        raise Refuse("$", "wrong_type")
    if "contract" not in doc:
        raise Refuse("contract", "missing")
    c = doc["contract"]
    if not isinstance(c, dict):
        raise Refuse("contract", "wrong_type")
    for k in ("major", "minor"):
        if k not in c:
            raise Refuse("contract." + k, "missing")
    major = integer(c["major"], "contract.major", 0, MAX_MINOR)
    minor = integer(c["minor"], "contract.minor", 0, MAX_MINOR)
    if major != MAJOR:
        raise Unsupported(major, minor, doc.get("kind") if isinstance(doc.get("kind"), str) else None)
    ctx = Ctx(minor > MINOR)
    obj(ctx, c, "contract", ["major", "minor"], [])
    obj(ctx, doc, "$", ["contract", "kind", "points", "provenance", "ref_lines", "status", "x", "y"], [])
    kind = enum(doc["kind"], "kind", KINDS)
    status = enum(doc["status"], "status", STATUSES)
    xa = axis(ctx, doc["x"], "x")
    ya = axis(ctx, doc["y"], "y")
    if kind == "HISTOGRAM":
        if xa[0] != "QUANTITY":
            raise Refuse("x.kind", "axis_kind")
        if ya[0] != "ORDINAL":
            raise Refuse("y.kind", "axis_kind")
    pts = doc["points"]
    if not isinstance(pts, list):
        raise Refuse("points", "wrong_type")
    if len(pts) > MAX_POINTS:
        raise Refuse("points", "too_many")
    for i, p in enumerate(pts):
        point(ctx, p, "points[%d]" % i, kind, status)
    if status == "INSUFFICIENT_DATA" and kind not in CONTROL_KINDS and pts:
        raise Refuse("points", "status_conflict")
    if status == "OK" and not pts:
        raise Refuse("status", "status_conflict")
    rls = doc["ref_lines"]
    if not isinstance(rls, list):
        raise Refuse("ref_lines", "wrong_type")
    if len(rls) > MAX_REF_LINES:
        raise Refuse("ref_lines", "too_many")
    value_axis = xa if kind == "HISTOGRAM" else ya
    for i, r in enumerate(rls):
        at = "ref_lines[%d]" % i
        obj(ctx, r, at, ["kind", "unit", "value"], [])
        enum(r["kind"], at + ".kind", REF_KINDS)
        f64(r["value"], at + ".value")
        u = enum(r["unit"], at + ".unit", LENGTH_UNITS + ANGLE_UNITS)
        if r["value"] == "0x8000000000000000":
            raise Refuse(at + ".value", "negative_zero")
        if value_axis[0] != "QUANTITY":
            raise Refuse(at + ".unit", "no_value_unit")
        if u != value_axis[1]:
            raise Refuse(at + ".unit", "unit_mismatch")
        if r["kind"] in LIMIT_REF_KINDS:
            if kind not in CONTROL_KINDS:
                raise Refuse(at + ".kind", "not_applicable")
            if status == "INSUFFICIENT_DATA":
                raise Refuse(at + ".kind", "status_conflict")
    for i in range(1, len(rls)):
        if rls[i - 1]["kind"] == rls[i]["kind"] and value(rls[i - 1]["value"]) == value(rls[i]["value"]):
            raise Refuse("ref_lines[%d]" % i, "duplicate")
        a = (REF_KINDS.index(rls[i - 1]["kind"]), total_key(rls[i - 1]["value"]))
        b = (REF_KINDS.index(rls[i]["kind"]), total_key(rls[i]["value"]))
        if not a < b:
            raise Refuse("ref_lines[%d]" % i, "not_canonical_order")
    pv = obj(ctx, doc["provenance"], "provenance",
             ["algorithm", "constants", "dataset_digest", "excluded", "kernel_version", "unverified"],
             ["binning_rule", "frozen_limits"])
    d = string(pv["dataset_digest"], "provenance.dataset_digest")
    if not re.fullmatch(r"sha256:[0-9a-f]{64}", d):
        raise Refuse("provenance.dataset_digest", "bad_format")
    if d == "sha256:" + "0" * 64:
        raise Refuse("provenance.dataset_digest", "tombstone")
    pattern(pv["algorithm"], "provenance.algorithm", "algorithm", MAX_ALGORITHM_BYTES)
    pattern(pv["kernel_version"], "provenance.kernel_version", "kernel_version")
    if "frozen_limits" in pv:
        pattern(pv["frozen_limits"], "provenance.frozen_limits", "id")
        if kind not in CONTROL_KINDS:
            raise Refuse("provenance.frozen_limits", "not_applicable")
    if "binning_rule" in pv:
        pattern(pv["binning_rule"], "provenance.binning_rule", "binning_rule")
        if kind != "HISTOGRAM":
            raise Refuse("provenance.binning_rule", "not_applicable")
    elif kind == "HISTOGRAM":
        raise Refuse("provenance.binning_rule", "required_for_kind")
    constants = provenance_constants(pv, kind)
    unverified = provenance_unverified(ctx, pv, kind, constants)
    provenance_excluded(ctx, pv)
    reviews_against_unverified(pts, unverified)
    return ("ok", sorted(ctx.ignored), doc)


def plist(pv, key, max_items):
    v = pv[key]
    if not isinstance(v, list):
        raise Refuse("provenance." + key, "wrong_type")
    if len(v) > max_items:
        raise Refuse("provenance." + key, "too_many")
    return v


def provenance_constants(pv, kind):
    cs = plist(pv, "constants", MAX_CONSTANTS)
    for k, c in enumerate(cs):
        pattern(c, "provenance.constants[%d]" % k, "constants", MAX_CONSTANTS_REF_BYTES)
    for k in range(1, len(cs)):
        if not cs[k - 1].encode() < cs[k].encode():
            raise Refuse("provenance.constants[%d]" % k, "not_canonical_order")
    if kind in CONTROL_KINDS and not cs:
        raise Refuse("provenance.constants", "required_for_kind")
    if kind not in CONTROL_KINDS and cs:
        raise Refuse("provenance.constants", "not_applicable")
    return cs


def unverified_key(u):
    if u["kind"] == "CONSTANTS":
        return (0, u["ref"].encode(), 0)
    return (1, u["set_id"].encode(), u["set_version"])


def provenance_unverified(ctx, pv, kind, constants):
    us = plist(pv, "unverified", MAX_UNVERIFIED)
    for k, u in enumerate(us):
        at = "provenance.unverified[%d]" % k
        if not isinstance(u, dict):
            raise Refuse(at, "wrong_type")
        if "kind" not in u:
            raise Refuse(at + ".kind", "missing")
        uk = enum(u["kind"], at + ".kind", UNVERIFIED_KINDS)
        if uk == "CONSTANTS":
            obj(ctx, u, at, ["kind", "ref"], [])
            pattern(u["ref"], at + ".ref", "constants", MAX_CONSTANTS_REF_BYTES)
        else:
            obj(ctx, u, at, ["kind", "set_id", "set_version"], [])
            pattern(u["set_id"], at + ".set_id", "set_id")
            integer(u["set_version"], at + ".set_version", 1, MAX_SET_VERSION)
    for k in range(1, len(us)):
        if not unverified_key(us[k - 1]) < unverified_key(us[k]):
            raise Refuse("provenance.unverified[%d]" % k, "not_canonical_order")
    for k, u in enumerate(us):
        if u["kind"] == "CONSTANTS" and u["ref"] not in constants:
            raise Refuse("provenance.unverified[%d]" % k, "not_referenced")
        if u["kind"] == "RULE_SET" and kind not in CONTROL_KINDS:
            raise Refuse("provenance.unverified[%d]" % k, "not_applicable")
    return us


def provenance_excluded(ctx, pv):
    es = plist(pv, "excluded", MAX_EXCLUDED)
    for k, e in enumerate(es):
        at = "provenance.excluded[%d]" % k
        obj(ctx, e, at, ["reason", "source_index"], [])
        integer(e["source_index"], at + ".source_index", 0, MAX_SOURCE_INDEX)
        pattern(e["reason"], at + ".reason", "exclusion_reason")
    for k in range(1, len(es)):
        if not es[k - 1]["source_index"] < es[k]["source_index"]:
            raise Refuse("provenance.excluded[%d]" % k, "not_canonical_order")


def reviews_against_unverified(pts, unverified):
    listed = {(u["set_id"], u["set_version"]) for u in unverified if u["kind"] == "RULE_SET"}
    seen = {}
    for i, p in enumerate(pts):
        for j, m in enumerate(p["markers"]):
            if m["kind"] != "RULE_VIOLATION":
                continue
            at = "points[%d].markers[%d].review" % (i, j)
            key = (m["set_id"], m["set_version"])
            if seen.setdefault(key, m["review"]) != m["review"]:
                raise Refuse(at, "conflicting_review")
            if m["review"] == "EXPERT_REVIEWED" and key in listed:
                raise Refuse(at, "conflicting_review")
            if m["review"] != "EXPERT_REVIEWED" and key not in listed:
                raise Refuse(at, "not_in_unverified")


# ---------------------------------------------------------------- the examples (hand-written)

D1 = "sha256:" + hashlib.sha256(b"chart-series example data set 1").hexdigest()
D2 = "sha256:" + hashlib.sha256(b"chart-series example data set 2").hexdigest()


def pt(x, y, markers=(), **extra):
    p = {"x": hexf(x), "y": hexf(y), "markers": list(markers)}
    for k, v in extra.items():
        p[k] = hexf(v) if k == "x_upper" else v
    return p


def ordinal(key):
    return {"kind": "ORDINAL", "label_key": key}


def qty(q, unit, key):
    return {"kind": "QUANTITY", "quantity": q, "unit": unit, "label_key": key}


def ref(kind, v, unit):
    return {"kind": kind, "value": hexf(v), "unit": unit}


CONSTANTS = "spc.constants.normal@1.0.0"


def prov(alg, digest=D1, constants=(), unverified=(), excluded=(), **extra):
    p = {"dataset_digest": digest, "algorithm": alg, "kernel_version": "0.1.0",
         "constants": list(constants), "unverified": list(unverified), "excluded": list(excluded)}
    p.update(extra)
    return p


def uc(ref_):
    return {"kind": "CONSTANTS", "ref": ref_}


def us(set_id, version):
    return {"kind": "RULE_SET", "set_id": set_id, "set_version": version}


def ex_(index, reason):
    return {"source_index": index, "reason": reason}


def spc_prov(digest, sets, **extra):
    """A control chart on the shipped UNVERIFIED constants table and UNVERIFIED rule sets."""
    return prov("stats.spc.chart@1.0.0", digest, [CONSTANTS], [uc(CONSTANTS)] + [us(a, v) for a, v in sets],
                **extra)


OOT = {"kind": "OUT_OF_TOLERANCE"}
IND = {"kind": "INDETERMINATE"}


def rv(set_id, version, rule, review="UNVERIFIED"):
    return {"kind": "RULE_VIOLATION", "set_id": set_id, "set_version": version, "rule_id": rule,
            "review": review}


def series(kind, x, y, points, ref_lines, provenance, status="OK"):
    return {"contract": {"major": 1, "minor": 0}, "kind": kind, "status": status, "x": x, "y": y,
            "points": points, "ref_lines": ref_lines, "provenance": provenance}


def examples():
    ex = {}
    ex["trend"] = series(
        "TREND", ordinal("stats.axis.part_sequence"), qty("LENGTH", "mm", "stats.axis.value"),
        [pt(0.0, 10.02, record="rec-0001", piece="part:1", x_label="Teil 1 – Größe"),
         pt(1.0, 10.13, [OOT], record="rec-0002", piece="part:2", x_label="part \"2\" \\ A"),
         pt(2.0, -0.0, [IND], record="rec-0003"),
         pt(3.0, 5e-324),
         pt(4.0, 9.95, record="rec-0005")],
        [ref("NOMINAL", 10.0, "mm"), ref("LSL", 9.9, "mm"), ref("USL", 10.1, "mm")],
        prov("stats.trend.build@1.0.0", excluded=[ex_(5, "NOT_DECIDABLE"), ex_(7, "USER")]))
    ex["histogram"] = series(
        "HISTOGRAM", qty("LENGTH", "um", "stats.axis.value"), ordinal("stats.axis.count"),
        [pt(-12.5, 2.0, x_upper=-7.5), pt(-7.5, 0.0, x_upper=-2.5), pt(-2.5, 5.0, x_upper=2.5),
         pt(2.5, 3.0, [OOT], x_upper=7.5)],
        [ref("NOMINAL", 0.0, "um"), ref("LSL", -10.0, "um"), ref("USL", 5.0, "um")],
        prov("stats.trend.hist@1.0.0", binning_rule="fixed_width@1"))
    ex["control_xbar"] = series(
        "CONTROL_XBAR", ordinal("stats.axis.subgroup"), qty("LENGTH", "mm", "stats.axis.subgroup_mean"),
        [pt(0.0, 25.001), pt(1.0, 25.004, [rv("nelson", 1, "1"), rv("nelson", 1, "5")]),
         pt(2.0, 24.999, [rv("nelson", 1, "2"), rv("western_electric", 1, "4")])],
        [ref("CENTRE", 25.0, "mm"), ref("LCL", 24.997, "mm"), ref("UCL", 25.003, "mm")],
        spc_prov(D2, [("nelson", 1), ("western_electric", 1)], frozen_limits="lim-2026-0001"))
    ex["control_r"] = series(
        "CONTROL_R", ordinal("stats.axis.subgroup"), qty("LENGTH", "mm", "stats.axis.subgroup_range"),
        [pt(0.0, 0.004), pt(1.0, 0.0), pt(2.0, 0.011, [rv("beyond_limits", 1, "1")])],
        [ref("CENTRE", 0.005, "mm"), ref("LCL", 0.0, "mm"), ref("UCL", 0.0106, "mm")],
        spc_prov(D2, [("beyond_limits", 1)]))
    ex["control_s"] = series(
        "CONTROL_S", ordinal("stats.axis.subgroup"), qty("LENGTH", "in", "stats.axis.subgroup_sd"),
        [pt(0.0, 0.0002), pt(1.0, 0.00031, [rv("site_rules", 2, "3", "CLAIMED_BY_FILE")])],
        [ref("CENTRE", 0.00025, "in"), ref("LCL", 0.0, "in"), ref("UCL", 0.00052, "in")],
        spc_prov(D2, [("site_rules", 2)], excluded=[ex_(0, "UNIT_MISMATCH")]))
    ex["control_i"] = series(
        "CONTROL_I", ordinal("stats.axis.part_sequence"), qty("ANGLE", "deg", "stats.axis.value"),
        [pt(0.0, 45.01, record="rec-a1"), pt(1.0, 44.98, record="rec-a2"),
         pt(2.0, 45.09, [OOT, rv("western_electric", 1, "1")], record="rec-a3")],
        [ref("NOMINAL", 45.0, "deg"), ref("USL", 45.05, "deg"), ref("CENTRE", 45.0, "deg"),
         ref("LCL", 44.91, "deg"), ref("UCL", 45.08, "deg")],
        spc_prov(D2, [("western_electric", 1)], frozen_limits="lim-2026-0002"))
    ex["control_mr"] = series(
        "CONTROL_MR", ordinal("stats.axis.part_sequence"), qty("ANGLE", "arcsec", "stats.axis.moving_range"),
        [pt(1.0, 108.0), pt(2.0, 396.0)],
        [ref("CENTRE", 120.0, "arcsec"), ref("LCL", 0.0, "arcsec"), ref("UCL", 392.04, "arcsec")],
        spc_prov(D2, []))
    ex["vector_group"] = series(
        "VECTOR_GROUP", qty("LENGTH", "mm", "lvm.axis.x"), qty("LENGTH", "mm", "lvm.axis.y"),
        [pt(-1.5, 2.25, piece="station:A"), pt(0.0, 0.0), pt(1e308, -1e-308, piece="station:B")],
        [],
        prov("lvm.plot.vector_group@1.0.0"))
    return ex


def minor9(trend):
    d = copy.deepcopy(trend)
    d["contract"]["minor"] = 9
    d["subtitle_key"] = "stats.chart.subtitle"
    return d


def v2():
    return {"contract": {"major": 2, "minor": 0}, "kind": "TREND",
            "series": [{"t": "0x0000000000000000", "v": "0x4024000000000000"}],
            "provenance": {"dataset_digest": D1, "algorithm": "stats.trend.build@2.0.0"}}


# ---------------------------------------------------------------- refusal and acceptance cases

def set_at(doc, keys, v):
    node = doc
    for k in keys[:-1]:
        node = node[k]
    node[keys[-1]] = v


def del_at(doc, keys):
    node = doc
    for k in keys[:-1]:
        node = node[k]
    del node[keys[-1]]


def cases(ex):
    """(name, text, expect, schema_valid); expect is ('ok', ignored) or (field, reason)."""
    out = []

    def mut(name, base, fn, expect, sv_label):
        d = copy.deepcopy(ex[base])
        fn(d)
        out.append((name, canon(d).decode("utf-8"), expect, sv_label))

    def put(keys, v):
        return lambda d: set_at(d, keys, v)

    def drop(keys):
        return lambda d: del_at(d, keys)

    # AC-4 and the other provenance rules.
    mut("ac4_no_dataset_digest", "trend", drop(["provenance", "dataset_digest"]),
        ("provenance.dataset_digest", "missing"), False)
    mut("no_algorithm", "trend", drop(["provenance", "algorithm"]), ("provenance.algorithm", "missing"), False)
    mut("no_kernel_version", "trend", drop(["provenance", "kernel_version"]),
        ("provenance.kernel_version", "missing"), False)
    mut("digest_upper_case", "trend", put(["provenance", "dataset_digest"], D1.upper().replace("SHA256", "sha256")),
        ("provenance.dataset_digest", "bad_format"), False)
    mut("digest_tombstone", "trend", put(["provenance", "dataset_digest"], "sha256:" + "0" * 64),
        ("provenance.dataset_digest", "tombstone"), False)
    mut("digest_null", "trend", put(["provenance", "dataset_digest"], None),
        ("provenance.dataset_digest", "wrong_type"), False)
    mut("algorithm_without_version", "trend", put(["provenance", "algorithm"], "stats.trend.build"),
        ("provenance.algorithm", "bad_format"), False)
    mut("algorithm_two_segments", "trend", put(["provenance", "algorithm"], "stats.trend@1.0.0"),
        ("provenance.algorithm", "bad_format"), False)
    mut("algorithm_leading_zero", "trend", put(["provenance", "algorithm"], "stats.trend.build@1.01.0"),
        ("provenance.algorithm", "bad_format"), False)
    mut("algorithm_too_long", "trend", put(["provenance", "algorithm"], "a." * 60 + "a.b@1.0.0"),
        ("provenance.algorithm", "bad_format"), False)
    mut("kernel_version_two_parts", "trend", put(["provenance", "kernel_version"], "0.1"),
        ("provenance.kernel_version", "bad_format"), False)
    mut("frozen_limits_on_trend", "trend", put(["provenance", "frozen_limits"], "lim-1"),
        ("provenance.frozen_limits", "not_applicable"), True)
    mut("frozen_limits_bad_id", "control_xbar", put(["provenance", "frozen_limits"], "-lim"),
        ("provenance.frozen_limits", "bad_format"), False)
    mut("histogram_without_binning_rule", "histogram", drop(["provenance", "binning_rule"]),
        ("provenance.binning_rule", "required_for_kind"), True)
    mut("binning_rule_on_trend", "trend", put(["provenance", "binning_rule"], "sturges@1"),
        ("provenance.binning_rule", "not_applicable"), True)
    mut("binning_rule_bad", "histogram", put(["provenance", "binning_rule"], "sturges"),
        ("provenance.binning_rule", "bad_format"), False)
    mut("provenance_unknown_field", "trend", put(["provenance", "note"], "x"),
        ("provenance.note", "unknown_field"), False)
    # Contract version.
    mut("contract_missing", "trend", drop(["contract"]), ("contract", "missing"), False)
    mut("major_missing", "trend", drop(["contract", "major"]), ("contract.major", "missing"), False)
    mut("major_string", "trend", put(["contract", "major"], "1"), ("contract.major", "not_integer"), False)
    mut("minor_negative", "trend", put(["contract", "minor"], -1), ("contract.minor", "out_of_range"), False)
    mut("minor_too_large", "trend", put(["contract", "minor"], MAX_MINOR + 1), ("contract.minor", "out_of_range"),
        False)
    mut("contract_unknown_field", "trend", put(["contract", "patch"], 0), ("contract.patch", "unknown_field"), False)
    # Kind and axes.
    mut("kind_unknown", "trend", put(["kind"], "PARETO"), ("kind", "unknown_value"), False)
    mut("kind_lower_case", "trend", put(["kind"], "trend"), ("kind", "unknown_value"), False)
    mut("axis_unit_unknown", "trend", put(["y", "unit"], "MM"), ("y.unit", "unknown_value"), False)
    mut("axis_unit_of_other_quantity", "trend", put(["y", "unit"], "deg"), ("y.unit", "unit_mismatch"), False)
    mut("axis_quantity_unknown", "trend", put(["y", "quantity"], "MASS"), ("y.quantity", "unknown_value"), False)
    mut("axis_ordinal_with_unit", "trend", put(["x", "unit"], "mm"), ("x.unit", "unknown_field"), False)
    mut("axis_label_key_bad", "trend", put(["x", "label_key"], "Stats.Axis"), ("x.label_key", "bad_format"), False)
    mut("axis_kind_missing", "trend", drop(["x", "kind"]), ("x.kind", "missing"), False)
    mut("histogram_ordinal_x", "histogram", put(["x"], ordinal("stats.axis.value")), ("x.kind", "axis_kind"),
        True)
    mut("histogram_quantity_y", "histogram", put(["y"], qty("LENGTH", "um", "stats.axis.count")),
        ("y.kind", "axis_kind"), True)
    # Points.
    mut("point_x_missing", "trend", drop(["points", 1, "x"]), ("points[1].x", "missing"), False)
    mut("point_y_nan", "trend", put(["points", 2, "y"], "0x7ff8000000000000"), ("points[2].y", "non_finite"), False)
    mut("point_y_negative_infinity", "trend", put(["points", 2, "y"], "0xfff0000000000000"),
        ("points[2].y", "non_finite"), False)
    mut("point_y_upper_hex", "trend", put(["points", 0, "y"], "0x40240000000000AB"),
        ("points[0].y", "bad_hex"), False)
    mut("point_y_short_hex", "trend", put(["points", 0, "y"], "0x4024"), ("points[0].y", "bad_hex"), False)
    mut("point_y_json_number", "trend", put(["points", 0, "y"], 10), ("points[0].y", "wrong_type"), False)
    mut("point_unknown_field", "trend", put(["points", 0, "note"], "x"), ("points[0].note", "unknown_field"),
        False)
    mut("point_markers_missing", "trend", drop(["points", 0, "markers"]), ("points[0].markers", "missing"), False)
    mut("point_record_bad", "trend", put(["points", 0, "record"], "rec 1"), ("points[0].record", "bad_format"),
        False)
    mut("point_x_label_empty", "trend", put(["points", 0, "x_label"], ""), ("points[0].x_label", "bad_format"),
        False)
    mut("point_x_label_bidi_override", "trend", put(["points", 0, "x_label"], "part ‮1"),
        ("points[0].x_label", "bad_format"), True)
    mut("point_x_label_257_chars", "trend", put(["points", 0, "x_label"], "é" * 257),
        ("points[0].x_label", "bad_format"), False)
    mut("point_x_upper_on_trend", "trend", put(["points", 0, "x_upper"], hexf(1.0)),
        ("points[0].x_upper", "not_applicable"), True)
    mut("histogram_fractional_count", "histogram", put(["points", 0, "y"], hexf(2.5)),
        ("points[0].y", "bad_count"), True)
    mut("histogram_negative_count", "histogram", put(["points", 0, "y"], hexf(-1.0)),
        ("points[0].y", "bad_count"), True)
    mut("histogram_negative_zero_count", "histogram", put(["points", 1, "y"], hexf(-0.0)),
        ("points[1].y", "bad_count"), True)
    mut("histogram_count_above_2_53", "histogram", put(["points", 0, "y"], hexf(2.0 ** 54)),
        ("points[0].y", "bad_count"), True)
    mut("histogram_upper_below_lower", "histogram", put(["points", 0, "x_upper"], hexf(-13.0)),
        ("points[0].x_upper", "upper_below_lower"), True)
    mut("histogram_upper_negative_zero_below_zero", "histogram",
        lambda d: (set_at(d, ["points", 2, "x"], hexf(0.0)), set_at(d, ["points", 2, "x_upper"], hexf(-0.0))),
        ("points[2].x_upper", "upper_below_lower"), True)
    # Markers.
    mut("marker_kind_unknown", "trend", put(["points", 1, "markers"], [{"kind": "OUTLIER"}]),
        ("points[1].markers[0].kind", "unknown_value"), False)
    mut("marker_order", "control_xbar", put(["points", 1, "markers"], [rv("nelson", 1, "5"), rv("nelson", 1, "1")]),
        ("points[1].markers[1]", "not_canonical_order"), True)
    mut("marker_duplicate", "trend", put(["points", 1, "markers"], [OOT, OOT]),
        ("points[1].markers[1]", "not_canonical_order"), True)
    mut("marker_kind_order", "control_i", put(["points", 2, "markers"], [rv("western_electric", 1, "1"), OOT]),
        ("points[2].markers[1]", "not_canonical_order"), True)
    mut("marker_conflict", "trend", put(["points", 1, "markers"], [OOT, IND]),
        ("points[1].markers", "conflicting_markers"), True)
    mut("marker_rule_without_set_version", "control_xbar",
        put(["points", 1, "markers"], [{"kind": "RULE_VIOLATION", "set_id": "nelson", "rule_id": "1",
                                        "review": "UNVERIFIED"}]),
        ("points[1].markers[0].set_version", "missing"), False)
    mut("marker_set_version_zero", "control_xbar", put(["points", 1, "markers"], [rv("nelson", 0, "1")]),
        ("points[1].markers[0].set_version", "out_of_range"), False)
    mut("marker_set_id_bad", "control_xbar", put(["points", 1, "markers"], [rv("Nelson", 1, "1")]),
        ("points[1].markers[0].set_id", "bad_format"), False)
    mut("marker_rule_id_bad", "control_xbar", put(["points", 1, "markers"], [rv("nelson", 1, "")]),
        ("points[1].markers[0].rule_id", "bad_format"), False)
    mut("marker_extra_field", "trend", put(["points", 1, "markers"], [{"kind": "OUT_OF_TOLERANCE", "rule_id": "1"}]),
        ("points[1].markers[0].rule_id", "unknown_field"), False)
    mut("markers_too_many", "control_xbar",
        put(["points", 1, "markers"], [rv("nelson", 1, "r%02d" % i) for i in range(MAX_MARKERS + 1)]),
        ("points[1].markers", "too_many"), False)
    # Rule-violation review status (mirrors fid_stats::RuleViolation.review, D-045).
    mut("marker_review_missing", "control_xbar", drop(["points", 1, "markers", 0, "review"]),
        ("points[1].markers[0].review", "missing"), False)
    mut("marker_review_unknown", "control_xbar", put(["points", 1, "markers", 0, "review"], "REVIEWED"),
        ("points[1].markers[0].review", "unknown_value"), False)
    mut("marker_review_lower_case", "control_xbar", put(["points", 1, "markers", 0, "review"], "unverified"),
        ("points[1].markers[0].review", "unknown_value"), False)
    mut("marker_unverified_set_not_listed", "control_xbar",
        put(["provenance", "unverified"], [uc(CONSTANTS), us("western_electric", 1)]),
        ("points[1].markers[0].review", "not_in_unverified"), True)
    mut("marker_claimed_set_not_listed", "control_s", put(["provenance", "unverified"], [uc(CONSTANTS)]),
        ("points[1].markers[0].review", "not_in_unverified"), True)
    mut("marker_expert_reviewed_but_listed", "control_s",
        put(["points", 1, "markers", 0, "review"], "EXPERT_REVIEWED"),
        ("points[1].markers[0].review", "conflicting_review"), True)
    mut("marker_reviews_disagree", "control_xbar", put(["points", 2, "markers", 0, "review"], "CLAIMED_BY_FILE"),
        ("points[2].markers[0].review", "conflicting_review"), True)
    mut("marker_expert_reviewed_accepted", "control_s",
        lambda d: (set_at(d, ["points", 1, "markers", 0, "review"], "EXPERT_REVIEWED"),
                   set_at(d, ["provenance", "unverified"], [uc(CONSTANTS)])),
        ("ok", []), True)
    mut("rule_violation_on_trend", "trend", put(["points", 1, "markers"], [OOT, rv("nelson", 1, "1")]),
        ("points[1].markers[1]", "not_applicable"), True)
    mut("rule_violation_on_histogram", "histogram", put(["points", 0, "markers"], [rv("nelson", 1, "1")]),
        ("points[0].markers[0]", "not_applicable"), True)
    mut("rule_violation_on_vector_group", "vector_group", put(["points", 0, "markers"], [rv("nelson", 1, "1")]),
        ("points[0].markers[0]", "not_applicable"), True)
    # Status (TREND.BUILD-008, SPC.CHART-004).
    mut("status_missing", "trend", drop(["status"]), ("status", "missing"), False)
    mut("status_unknown", "trend", put(["status"], "EMPTY"), ("status", "unknown_value"), False)
    mut("status_ok_without_points", "trend", put(["points"], []), ("status", "status_conflict"), True)
    mut("status_insufficient_trend_with_points", "trend", put(["status"], "INSUFFICIENT_DATA"),
        ("points", "status_conflict"), True)
    mut("status_insufficient_trend_accepted", "trend",
        lambda d: (set_at(d, ["status"], "INSUFFICIENT_DATA"), set_at(d, ["points"], [])),
        ("ok", []), True)
    mut("status_insufficient_histogram_accepted", "histogram",
        lambda d: (set_at(d, ["status"], "INSUFFICIENT_DATA"), set_at(d, ["points"], [])),
        ("ok", []), True)
    mut("status_insufficient_control_with_limits", "control_r",
        lambda d: (set_at(d, ["status"], "INSUFFICIENT_DATA"), set_at(d, ["points", 2, "markers"], [])),
        ("ref_lines[0].kind", "status_conflict"), True)
    mut("status_insufficient_control_with_rule_marker", "control_r",
        lambda d: (set_at(d, ["status"], "INSUFFICIENT_DATA"), set_at(d, ["ref_lines"], [])),
        ("points[2].markers[0]", "status_conflict"), True)
    mut("status_insufficient_control_accepted", "control_xbar",
        lambda d: (set_at(d, ["status"], "INSUFFICIENT_DATA"), set_at(d, ["ref_lines"], []),
                   [set_at(d, ["points", i, "markers"], []) for i in range(3)],
                   del_at(d, ["provenance", "frozen_limits"])),
        ("ok", []), True)
    # Provenance: constants, unverified and excluded (SPC.CHART-006).
    mut("constants_missing", "control_xbar", drop(["provenance", "constants"]),
        ("provenance.constants", "missing"), False)
    mut("constants_empty_on_control", "control_xbar",
        lambda d: (set_at(d, ["provenance", "constants"], []),
                   set_at(d, ["provenance", "unverified"], [us("nelson", 1), us("western_electric", 1)])),
        ("provenance.constants", "required_for_kind"), True)
    mut("constants_on_trend", "trend", put(["provenance", "constants"], [CONSTANTS]),
        ("provenance.constants", "not_applicable"), True)
    mut("constants_bad_ref", "control_xbar", put(["provenance", "constants"], ["spc.constants.normal"]),
        ("provenance.constants[0]", "bad_format"), False)
    mut("constants_order", "control_xbar",
        put(["provenance", "constants"], ["spc.constants.normal@1.0.0", "spc.constants.normal@0.9.0"]),
        ("provenance.constants[1]", "not_canonical_order"), True)
    mut("constants_duplicate", "control_xbar", put(["provenance", "constants"], [CONSTANTS, CONSTANTS]),
        ("provenance.constants[1]", "not_canonical_order"), True)
    mut("unverified_missing", "trend", drop(["provenance", "unverified"]),
        ("provenance.unverified", "missing"), False)
    mut("unverified_constants_not_referenced", "control_xbar",
        put(["provenance", "unverified"], [uc(CONSTANTS), uc("spc.constants.other@1.0.0"), us("nelson", 1),
                                           us("western_electric", 1)]),
        ("provenance.unverified[1]", "not_referenced"), True)
    mut("unverified_order", "control_xbar",
        put(["provenance", "unverified"], [uc(CONSTANTS), us("western_electric", 1), us("nelson", 1)]),
        ("provenance.unverified[2]", "not_canonical_order"), True)
    mut("unverified_kind_order", "control_xbar",
        put(["provenance", "unverified"], [us("nelson", 1), uc(CONSTANTS), us("western_electric", 1)]),
        ("provenance.unverified[1]", "not_canonical_order"), True)
    mut("unverified_rule_set_on_trend", "trend", put(["provenance", "unverified"], [us("nelson", 1)]),
        ("provenance.unverified[0]", "not_applicable"), True)
    mut("unverified_kind_unknown", "control_xbar", put(["provenance", "unverified", 0, "kind"], "TABLE"),
        ("provenance.unverified[0].kind", "unknown_value"), False)
    mut("unverified_set_version_zero", "control_xbar", put(["provenance", "unverified", 1, "set_version"], 0),
        ("provenance.unverified[1].set_version", "out_of_range"), False)
    mut("unverified_constants_extra_field", "control_xbar", put(["provenance", "unverified", 0, "set_id"], "x"),
        ("provenance.unverified[0].set_id", "unknown_field"), False)
    mut("excluded_missing", "trend", drop(["provenance", "excluded"]), ("provenance.excluded", "missing"), False)
    mut("excluded_order", "trend", put(["provenance", "excluded"], [ex_(7, "USER"), ex_(5, "NOT_DECIDABLE")]),
        ("provenance.excluded[1]", "not_canonical_order"), True)
    mut("excluded_duplicate_index", "trend",
        put(["provenance", "excluded"], [ex_(5, "NOT_DECIDABLE"), ex_(5, "USER")]),
        ("provenance.excluded[1]", "not_canonical_order"), True)
    mut("excluded_negative_index", "trend", put(["provenance", "excluded"], [ex_(-1, "USER")]),
        ("provenance.excluded[0].source_index", "out_of_range"), False)
    mut("excluded_index_too_large", "trend", put(["provenance", "excluded"], [ex_(2 ** 32, "USER")]),
        ("provenance.excluded[0].source_index", "out_of_range"), False)
    mut("excluded_fractional_index", "trend", put(["provenance", "excluded"], [ex_(987654321, "USER")]),
        ("provenance.excluded[0].source_index", "not_integer"), False)
    name, text, expect, svl = out.pop()
    assert text.count('"source_index":987654321') == 1
    out.append((name, text.replace('"source_index":987654321', '"source_index":1.5'), expect, svl))
    mut("excluded_reason_lower_case", "trend", put(["provenance", "excluded"], [ex_(1, "user")]),
        ("provenance.excluded[0].reason", "bad_format"), False)
    mut("excluded_reason_missing", "trend", put(["provenance", "excluded"], [{"source_index": 1}]),
        ("provenance.excluded[0].reason", "missing"), False)
    mut("excluded_largest_index_accepted", "trend", put(["provenance", "excluded"], [ex_(2 ** 32 - 1, "USER")]),
        ("ok", []), True)
    # Reference lines.
    mut("ref_negative_zero", "histogram", put(["ref_lines", 0, "value"], hexf(-0.0)),
        ("ref_lines[0].value", "negative_zero"), True)
    mut("ref_limit_line_on_trend", "trend", put(["ref_lines"], [ref("NOMINAL", 10.0, "mm"), ref("CENTRE", 10.0, "mm")]),
        ("ref_lines[1].kind", "not_applicable"), True)
    mut("ref_limit_line_on_histogram", "histogram", put(["ref_lines"], [ref("UCL", 5.0, "um")]),
        ("ref_lines[0].kind", "not_applicable"), True)
    mut("ref_limit_line_on_vector_group", "vector_group", put(["ref_lines"], [ref("LCL", 1.0, "mm")]),
        ("ref_lines[0].kind", "not_applicable"), True)
    mut("ref_unit_differs_from_axis", "trend", put(["ref_lines", 0, "unit"], "um"),
        ("ref_lines[0].unit", "unit_mismatch"), True)
    mut("histogram_ref_unit_differs_from_x", "histogram", put(["ref_lines", 0, "unit"], "mm"),
        ("ref_lines[0].unit", "unit_mismatch"), True)
    mut("ref_on_ordinal_value_axis", "trend", put(["y"], ordinal("stats.axis.value")),
        ("ref_lines[0].unit", "no_value_unit"), True)
    mut("ref_kind_unknown", "trend", put(["ref_lines", 0, "kind"], "TARGET"), ("ref_lines[0].kind", "unknown_value"),
        False)
    mut("ref_value_infinite", "trend", put(["ref_lines", 0, "value"], "0x7ff0000000000000"),
        ("ref_lines[0].value", "non_finite"), False)
    mut("ref_order", "trend", put(["ref_lines"], [ref("LSL", 9.9, "mm"), ref("NOMINAL", 10.0, "mm")]),
        ("ref_lines[1]", "not_canonical_order"), True)
    mut("ref_duplicate", "trend", put(["ref_lines"], [ref("LSL", 9.9, "mm"), ref("LSL", 9.9, "mm")]),
        ("ref_lines[1]", "duplicate"), True)
    mut("ref_same_value_other_kind_accepted", "control_i", lambda d: None, ("ok", []), True)
    mut("ref_same_kind_value_order", "trend", put(["ref_lines"], [ref("LSL", 9.9, "mm"), ref("LSL", 9.8, "mm")]),
        ("ref_lines[1]", "not_canonical_order"), True)
    mut("ref_lines_too_many", "trend",
        put(["ref_lines"], [ref("NOMINAL", float(i), "mm") for i in range(MAX_REF_LINES + 1)]),
        ("ref_lines", "too_many"), False)
    mut("ref_lines_missing", "trend", drop(["ref_lines"]), ("ref_lines", "missing"), False)
    # Root.
    mut("root_unknown_field", "trend", put(["subtitle_key"], "stats.chart.subtitle"),
        ("subtitle_key", "unknown_field"), False)
    out.append(("root_array", "[]", ("$", "wrong_type"), False))
    out.append(("syntax_trailing_comma", canon(ex["trend"]).decode("utf-8")[:-1] + ",}", ("$", "syntax"), False))
    t = canon(ex["trend"]).decode("utf-8")
    out.append(("duplicate_key", t[:-1] + ",\"kind\":\"TREND\"}", ("$", "duplicate_key"), False))
    out.append(("nan_literal", re.sub(r'"y":"0x[0-9a-f]{16}"', '"y":NaN', t, count=1), ("$", "syntax"), False))
    # Accepted: a later minor ignores unknown optional fields anywhere, and reports them.
    mut("minor1_nested_unknown_ignored", "trend",
        lambda d: (set_at(d, ["contract", "minor"], 1), set_at(d, ["points", 0, "note"], "x"),
                   set_at(d, ["provenance", "window"], 3)),
        ("ok", ["points[0].note", "provenance.window"]), True)
    mut("minor1_unknown_kind_still_refused", "trend",
        lambda d: (set_at(d, ["contract", "minor"], 1), set_at(d, ["kind"], "PARETO")),
        ("kind", "unknown_value"), False)
    mut("minor1_missing_field_still_refused", "trend",
        lambda d: (set_at(d, ["contract", "minor"], 1), del_at(d, ["provenance", "dataset_digest"])),
        ("provenance.dataset_digest", "missing"), False)
    mut("integral_float_version_accepted", "trend", lambda d: None, ("ok", []), True)
    # The last case is rewritten with a non-canonical spelling of the integer 1.
    name, text, expect, svl = out.pop()
    out.append(("integral_float_version_accepted", text.replace('"major":1', '"major":1.0', 1), expect, svl))
    return out


# ---------------------------------------------------------------- build and check

def build():
    sch = schema()
    walk_keywords(sch)
    files = {SCHEMA_FILE: json.dumps(sch, indent=2, ensure_ascii=False) + "\n"}
    ex = examples()
    expected = {"examples": {}}
    for name, doc in ex.items():
        b = canon(doc)
        text = b.decode("utf-8")
        st, ignored, _ = check(text)
        assert st == "ok" and ignored == [], name
        assert schema_valid(sch, text), name
        files[EXAMPLES / (name + ".json")] = text
        expected["examples"][name + ".json"] = {"digest": sha(b), "kind": doc["kind"],
                                                "points": len(doc["points"]),
                                                "ref_lines": len(doc["ref_lines"])}
    assert sorted(d["kind"] for d in ex.values()) == sorted(KINDS)
    m9 = canon(minor9(ex["trend"])).decode("utf-8")
    st, ignored, _ = check(m9)
    assert st == "ok" and ignored == ["subtitle_key"]
    assert schema_valid(sch, m9)
    files[EXAMPLES / "trend_minor9.json"] = m9
    expected["minor9"] = {"file": "v1_examples/trend_minor9.json", "minor": 9, "ignored": ignored,
                          "same_as_with_minor_0": "v1_examples/trend.json"}
    v2t = canon(v2()).decode("utf-8")
    try:
        check(v2t)
        raise AssertionError("v2 accepted")
    except Unsupported as u:
        assert (u.major, u.minor, u.kind) == (2, 0, "TREND")
    assert not schema_valid(sch, v2t)
    files[HERE / "v2_trend.json"] = v2t
    expected["v2"] = {"file": "v2_trend.json", "code": "report.chart_series_unsupported", "major": 2,
                      "minor": 0, "kind": "TREND"}
    rows = []
    names = set()
    for name, text, expect, svl in cases(ex):
        assert name not in names, name
        names.add(name)
        try:
            st, ignored, _ = check(text)
            got = ("ok", ignored)
        except Refuse as r:
            got = (r.field, r.reason)
        assert got == expect, (name, got, expect)
        assert schema_valid(sch, text) == svl, (name, "schema", not svl)
        if expect[0] == "ok":
            row = {"name": name, "text": text, "ok": True, "ignored": expect[1], "schema_valid": svl}
        else:
            row = {"name": name, "text": text, "ok": False, "code": "report.chart_series_invalid",
                   "field": expect[0], "reason": expect[1], "schema_valid": svl}
        rows.append(row)
    files[HERE / "cases.json"] = json.dumps({"cases": rows}, indent=1, ensure_ascii=False) + "\n"
    files[HERE / "expected.json"] = json.dumps(expected, indent=1, sort_keys=True) + "\n"
    return files


def main():
    files = build()
    if "--check" in sys.argv[1:]:
        stale = [p for p, text in files.items() if not p.exists() or p.read_bytes() != text.encode("utf-8")]
        for p in stale:
            print("stale:", p.relative_to(ROOT))
        if stale:
            sys.exit(1)
        print("chart_series fixtures up to date (%d files)" % len(files))
        return
    for p, text in files.items():
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_bytes(text.encode("utf-8"))
    print("wrote %d files" % len(files))


if __name__ == "__main__":
    main()
