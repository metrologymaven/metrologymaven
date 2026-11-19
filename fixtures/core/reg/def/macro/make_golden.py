#!/usr/bin/env python3
"""Golden schemas for CORE.REG.DEF.MACRO AC-1 (fixture crate demo_ok/lib.rs).

Independent of the Rust code: each schema is written out here by hand from the leaf's rules
(fields in order, `additionalProperties: false`, sorted `required` left out when empty,
`Option<T>` and fields with a default not required, integer types with their bounds, unit
enums as strings in declaration order, `x-quantity`/`x-unit` from the field attribute, value
keywords on the element schema of an array) and printed as RFC 8785 canonical JSON plus a
newline. Run with `--check` to report stale files instead of writing them. Stdlib only.
"""

import json
import pathlib
import sys

U32 = {"type": "integer", "minimum": 0, "maximum": 4294967295}
EMPTY = {"type": "object", "additionalProperties": False, "properties": {}}


def obj(properties, required, **extra):
    o = {"type": "object", "additionalProperties": False, "properties": properties}
    if required:
        o["required"] = sorted(required)
    o.update(extra)
    return o


GOLDEN = {
    "demo.status@1": {
        "params": EMPTY,
        "result": obj(
            {
                "features": dict(U32, description="Number of features in the store."),
                "mode": {"type": "string", "enum": ["idle", "measuring", "off_line"]},
                "label": {"type": "string"},
            },
            ["features", "mode"],
            description="Status of the demo store.",
        ),
    },
    "demo.offset_plane@1": {
        "params": obj(
            {
                "feature": {
                    "type": "string",
                    "description": "The plane feature to offset.",
                    "pattern": "^ftr_[0-9a-f]{8}$",
                },
                "distance": {
                    "type": "number",
                    "x-quantity": "length",
                    "x-unit": "mm",
                    "minimum": -1000,
                    "maximum": 1000,
                },
                "tags": {"type": "array", "maxItems": 8, "items": {"type": "string", "maxLength": 32}},
                "dry_run": {"type": "boolean", "default": False},
            },
            ["feature", "distance", "tags"],
        ),
        "result": obj(
            {
                "revision": U32,
                "applied": {"type": "number", "x-quantity": "length", "x-unit": "mm"},
            },
            ["revision", "applied"],
        ),
    },
    "demo.set_telemetry@1": {
        "params": obj(
            {"policy": {"type": "string", "enum": ["off", "local_only", "upload"]}},
            ["policy"],
        ),
        "result": EMPTY,
    },
}


def canonical(value):
    # RFC 8785 for this data: ASCII keys sorted, integers only, no whitespace.
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True) + "\n"


def main():
    check = "--check" in sys.argv[1:]
    out = pathlib.Path(__file__).resolve().parent / "golden"
    stale = []
    for command, schemas in GOLDEN.items():
        for kind, schema in schemas.items():
            path = out / f"{command}.{kind}.json"
            text = canonical(schema)
            if check:
                if not path.exists() or path.read_text(encoding="utf-8") != text:
                    stale.append(path.name)
            else:
                out.mkdir(exist_ok=True)
                path.write_bytes(text.encode("utf-8"))
    if stale:
        print("stale:", ", ".join(stale))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
