#!/usr/bin/env python3
"""Writes the fixtures of CORE.REG.DEF.SCHEMA (stdlib only).

Every expected value here is hand-labelled from the rules in
spec/core/reg/def/schema/NODE.md; nothing is produced by the Rust code.

- valid/*.json: schemas that use every project keyword; the checker reports nothing.
- invalid/*.json: {"expect": {"code", "pointer"}, "schema": ...}; the checker reports
  exactly that one issue (AC-2).
- catalogue/geo_frame_units.json: the GEO.FRAME.UNITS catalogue in the canonical form,
  typed by hand from the factors pinned in spec/geo/frame/units/NODE.md (AC-3), and
  catalogue/geo_frame_units.sha256, its SHA-256 computed here with hashlib (AC-4).
- catalogue/one_unit_changed.json: the same with one factor changed (thou = 127/500000);
  from_json refuses it (REQ-004).
- catalogue/with_time.json: TEST DATA ONLY, not a published catalogue: the GEO.FRAME.UNITS
  catalogue plus an invented `time` kind, to show the facade serves an extension its owner
  publishes. The registry itself defines no unit.

Run from anywhere: python fixtures/core/reg/def/schema/make_fixtures.py [--check]
CC-BY-4.0.
"""

import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
DIALECT = "https://json-schema.org/draft/2020-12/schema"
PI_HEX = "c90fdaa22168c234c4c6628b80dc1cd129024e088a67cc74020bbea63b139b22"


def canonical(v) -> bytes:
    # RFC 8785 equals this for documents of ASCII strings, booleans, objects and arrays only.
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii")


def pretty(v) -> bytes:
    return (json.dumps(v, indent=2, ensure_ascii=True) + "\n").encode("ascii")


# ------------------------------------------------------------------ catalogues

LENGTH = [  # spec/geo/frame/units/NODE.md, "Exact factors to SI"
    ("m", 1, 1, False),
    ("cm", 1, 100, False),
    ("mm", 1, 1000, False),
    ("um", 1, 10**6, False),
    ("nm", 1, 10**9, False),
    ("in", 127, 5000, False),  # 254/10000 in lowest terms
    ("ft", 381, 1250, False),
    ("thou", 127, 5000000, False),
]
ANGLE = [
    ("rad", 1, 1, False),
    ("deg", 1, 180, True),
    ("arcmin", 1, 10800, True),
    ("arcsec", 1, 648000, True),
    ("gon", 1, 200, True),
]
TIME = [  # invented test data
    ("s", 1, 1, False),
    ("ms", 1, 1000, False),
    ("min", 60, 1, False),
    ("h", 3600, 1, False),
]


def kind(name, canonical_unit, units):
    rows = [
        {"den": str(d), "num": str(n), "pi": pi, "symbol": sym}
        for (sym, n, d, pi) in sorted(units, key=lambda u: u[0].encode("ascii"))
    ]
    return {"canonical": canonical_unit, "kind": name, "units": rows}


def catalogue(kinds):
    kinds = sorted(kinds, key=lambda k: k["kind"].encode("ascii"))
    return {"format": "fid-quantity-catalogue/1", "kinds": kinds, "pi_hex": PI_HEX}


GEO = catalogue([kind("length", "m", LENGTH), kind("angle", "rad", ANGLE)])
CHANGED = catalogue(
    [
        kind("length", "m", [u if u[0] != "thou" else ("thou", 127, 500000, False) for u in LENGTH]),
        kind("angle", "rad", ANGLE),
    ]
)
WITH_TIME = catalogue([kind("length", "m", LENGTH), kind("angle", "rad", ANGLE), kind("time", "s", TIME)])

# ------------------------------------------------------------------ valid schemas

VALID = {
    "all_keywords": {
        "$schema": DIALECT,
        "title": "probe.measure_point parameters",
        "type": "object",
        "required": ["target", "approach", "features", "label"],
        "additionalProperties": False,
        "properties": {
            "target": {
                "type": "string",
                "x-ref": "feature",
                "x-selector": "single",
                "x-ui": {"widget": "entity-picker", "order": 1},
            },
            "features": {
                "type": "array",
                "items": {"type": "string", "x-ref": "element"},
                "x-selector": "list",
                "minItems": 1,
                "uniqueItems": True,
            },
            "approach": {
                "type": "number",
                "x-quantity": "length",
                "x-unit": "mm",
                "exclusiveMinimum": 0,
                "x-standards-ref": "ISO-10360-2:2009 cl. 6.2",
            },
            "tilt": {"type": "number", "x-quantity": "angle", "x-unit": "deg", "default": 0},
            # REQ-011: a measured value of record (rule C1); a boolean on any schema object.
            "nominal": {"type": "number", "x-quantity": "length", "x-unit": "mm", "value_of_record": True},
            "label": {"type": "string", "maxLength": 128, "x-untrusted": True},
            "note": {"type": ["string", "null"], "x-untrusted": False},
            # REQ-010: a file parameter (CORE.BUS.POL.FS): top level, string, sorted extensions.
            "probe_file": {"type": "string", "path_like": True, "x-extensions": ["step", "stl"]},
        },
    },
    "refs_and_defs": {
        "$comment": "local references, conditionals and the other applicators",
        "type": "object",
        "$defs": {
            "distance": {"type": "number", "x-quantity": "length", "x-unit": "um", "minimum": 0},
            "pair": {"type": "array", "prefixItems": [{"$ref": "#/$defs/distance"}, {"$ref": "#/$defs/distance"}]},
        },
        "properties": {
            "offset": {"$ref": "#/$defs/distance"},
            "span": {"$ref": "#/$defs/pair"},
            "mode": {"enum": ["fast", "fine"], "examples": ["fine"]},
            "count": {"type": "integer", "minimum": 1, "maximum": 1000},
            "frame": {"type": "string", "x-ref": "frame", "x-ui": {"group": "advanced"}},
            # REQ-011: allowed below the top level too, and false is a value.
            "readings": {
                "type": "array",
                "items": {"$ref": "#/$defs/distance", "value_of_record": True},
                "value_of_record": False,
            },
        },
        "patternProperties": {"^tag_[a-z]+$": {"type": "string", "maxLength": 32}},
        "dependentRequired": {"span": ["offset"]},
        "if": {"properties": {"mode": {"const": "fine"}}},
        "then": {"required": ["offset"]},
        "else": True,
        "anyOf": [{"required": ["offset"]}, {"required": ["span"]}],
        "unevaluatedProperties": False,
    },
}

# ------------------------------------------------------------------ invalid schemas


def wrap(prop):
    return {"type": "object", "properties": {"a": prop}}


def nested_not(levels):
    # The root object is level 1; each "not" adds one level.
    inner = {}
    for _ in range(levels - 1):
        inner = {"not": inner}
    return inner


INVALID = {
    # REQ-002: a keyword outside the closed list.
    "x_foo_keyword": (
        wrap({"type": "string", "x-foo": 1}),
        "unknown_keyword",
        "/properties/a/x-foo",
    ),
    # REQ-002: a reference that leaves the schema.
    "external_ref": (
        wrap({"$ref": "https://example.com/other.schema.json"}),
        "external_ref",
        "/properties/a/$ref",
    ),
    # REQ-003: not a kind of the GEO.FRAME.UNITS catalogue.
    "unknown_quantity": (
        wrap({"type": "number", "x-quantity": "lenght"}),
        "unknown_quantity",
        "/properties/a/x-quantity",
    ),
    # REQ-003: deg is an angle unit, not a length unit.
    "angle_unit_on_length": (
        wrap({"type": "number", "x-quantity": "length", "x-unit": "deg"}),
        "unit_not_accepted",
        "/properties/a/x-unit",
    ),
    # REQ-006: x-untrusted only on string-typed schemas.
    "untrusted_on_number": (
        wrap({"type": "number", "x-quantity": "length", "x-untrusted": True}),
        "untrusted_on_non_string",
        "/properties/a/x-untrusted",
    ),
    # REQ-007: designator, edition and clause only; the trailing words are invented text.
    "standards_ref_with_text": (
        wrap({"type": "string", "x-standards-ref": "ISO-1101:2017 cl. 8.2 the zone is limited by two lines"}),
        "bad_standards_ref",
        "/properties/a/x-standards-ref",
    ),
    # REQ-011: value_of_record is a boolean, not the string "true".
    "value_of_record_not_boolean": (
        wrap({"type": "number", "x-quantity": "length", "value_of_record": "true"}),
        "bad_keyword_value",
        "/properties/a/value_of_record",
    ),
    # REQ-010: path_like only on a top-level property; here on an array's items.
    "nested_path_like": (
        wrap({"type": "array", "items": {"type": "string", "path_like": True, "x-extensions": ["stl"]}}),
        "misplaced_path_like",
        "/properties/a/items/path_like",
    ),
    # REQ-010: x-extensions only next to path_like.
    "extensions_without_path_like": (
        wrap({"type": "string", "x-extensions": ["stl"]}),
        "misplaced_path_like",
        "/properties/a/x-extensions",
    ),
    # REQ-009: 65 levels; the innermost object is the first container below level 64.
    "depth_65": (
        nested_not(65),
        "schema_too_deep",
        "/not" * 64,
    ),
}


def files():
    out = {}
    for name, schema in VALID.items():
        out[f"valid/{name}.json"] = pretty(schema)
    for name, (schema, code, pointer) in INVALID.items():
        body = {"expect": {"code": code, "pointer": pointer}, "schema": schema}
        # depth_65 is written compactly so the file stays readable.
        out[f"invalid/{name}.json"] = (
            canonical(body) + b"\n" if name == "depth_65" else pretty(body)
        )
    out["catalogue/geo_frame_units.json"] = canonical(GEO) + b"\n"
    out["catalogue/geo_frame_units.sha256"] = ("sha256:" + hashlib.sha256(canonical(GEO)).hexdigest() + "\n").encode()
    out["catalogue/one_unit_changed.json"] = canonical(CHANGED) + b"\n"
    out["catalogue/with_time.json"] = canonical(WITH_TIME) + b"\n"
    return out


def main() -> int:
    check = "--check" in sys.argv[1:]
    stale = 0
    for rel, data in files().items():
        path = HERE / rel
        if check:
            if not path.exists() or path.read_bytes() != data:
                print(f"stale: {rel}")
                stale += 1
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    if check:
        print("fixtures:", "stale" if stale else "ok")
    return 1 if stale else 0


if __name__ == "__main__":
    sys.exit(main())
