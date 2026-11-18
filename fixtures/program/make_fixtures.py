#!/usr/bin/env python3
"""Generate the Command Program fixtures of spec node AUTO.PROG.FMT.

Independent reference: this script has its own RFC 8785 writer (key order by UTF-16
code units, ECMAScript number printing, JCS string escapes) and computes digests with
hashlib. Expected error codes for the invalid fixtures are labelled by hand from the
rules in spec/auto/prog/fmt/NODE.md; nothing here runs the Rust code.
If `jsonschema` is installed and schema/program-v1.json exists, every valid fixture is
also validated against the checked-in schema, and every fixture labelled
`unknown_field` or `schema_invalid` must fail it.

CC-BY-4.0 project data. Run:  python fixtures/program/make_fixtures.py
"""

import hashlib
import json
from decimal import Decimal
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent

# ---------------------------------------------------------------- RFC 8785 writer


def es_number(x):
    """ECMAScript Number::toString for a finite double (RFC 8785 section 3.2.2.3)."""
    if isinstance(x, bool):
        raise TypeError("bool is not a number")
    x = float(x)
    if x != x or x in (float("inf"), float("-inf")):
        raise ValueError("non-finite")
    if x == 0:
        return "0"
    sign = "-" if x < 0 else ""
    d = Decimal(repr(abs(x)))  # repr is the shortest round-trip form
    _, digits, exp = d.as_tuple()
    digits = "".join(map(str, digits))
    stripped = digits.rstrip("0")
    exp += len(digits) - len(stripped)
    digits = stripped
    k = len(digits)
    n = exp + k  # value = 0.digits * 10^n
    if k <= n <= 21:
        return sign + digits + "0" * (n - k)
    if 0 < n <= 21:
        return sign + digits[:n] + "." + digits[n:]
    if -6 < n <= 0:
        return sign + "0." + "0" * (-n) + digits
    e = n - 1
    mant = digits[0] + ("." + digits[1:] if k > 1 else "")
    return sign + mant + "e" + ("-" if e < 0 else "+") + str(abs(e))


def jcs_string(s):
    out = ['"']
    for ch in s:
        c = ord(ch)
        if ch == '"':
            out.append('\\"')
        elif ch == "\\":
            out.append("\\\\")
        elif ch == "\b":
            out.append("\\b")
        elif ch == "\t":
            out.append("\\t")
        elif ch == "\n":
            out.append("\\n")
        elif ch == "\f":
            out.append("\\f")
        elif ch == "\r":
            out.append("\\r")
        elif c < 0x20:
            out.append("\\u%04x" % c)
        else:
            out.append(ch)
    out.append('"')
    return "".join(out)


def jcs(v):
    if v is None:
        return "null"
    if v is True:
        return "true"
    if v is False:
        return "false"
    if isinstance(v, (int, float)):
        return es_number(v)
    if isinstance(v, str):
        return jcs_string(v)
    if isinstance(v, list):
        return "[" + ",".join(jcs(x) for x in v) + "]"
    if isinstance(v, dict):
        keys = sorted(v, key=lambda k: k.encode("utf-16-be"))
        return "{" + ",".join(jcs_string(k) + ":" + jcs(v[k]) for k in keys) + "}"
    raise TypeError(type(v))


def canon(v):
    return jcs(v).encode("utf-8")


def digest(b):
    return "sha256:" + hashlib.sha256(b).hexdigest()


# ---------------------------------------------------------------- IDs

_counter = [0]


def uuid7(n=None, version=7):
    """Deterministic UUIDv7 (RFC 9562 section 5.7): 48-bit ms timestamp, version, variant."""
    if n is None:
        n = _counter[0]
        _counter[0] += 1
    ts = 0x018F00000000 + n * 1000
    rnd = bytearray(hashlib.sha256(b"step-%d" % n).digest()[:10])
    rnd[0] = (version << 4) | (rnd[0] & 0x0F)
    rnd[2] = 0x80 | (rnd[2] & 0x3F)
    h = ts.to_bytes(6, "big").hex() + bytes(rnd).hex()
    return f"{h[0:8]}-{h[8:12]}-{h[12:16]}-{h[16:20]}-{h[20:32]}"


def step(kind, **fields):
    s = {"step_id": uuid7(), "kind": kind}
    s.update(fields)
    return s


def program(main, **over):
    p = {"format": "fid-program", "format_version": 1, "program_id": uuid7(), "meta": {}, "params": [],
         "variables": [], "subroutines": [], "main": main}
    p.update(over)
    return p


# ---------------------------------------------------------------- valid programs

minimal = program([])

all_kinds_main = [
    step("comment", text="Measure the bore and report it."),  # 0
    step("invoke", command="feature.fit_cylinder", major=1,  # 1
         args={"points": {"literal": "pc-17"}, "max_iter": {"literal": 50}, "tol": {"expr": "tol_mm * 1 mm"}},
         bind="bore"),
    step("assign", var="d", expr="bore.diameter"),  # 2
    step("if", branches=[  # 3
        {"cond": "d > nominal + 0.02 mm", "body": [step("assign", var="ok", expr="false")]},
        {"cond": "d < nominal - 0.02 mm", "body": [step("comment", text="undersize")]},
    ], else_body=[step("assign", var="ok", expr="true")]),
    step("loop", mode="counted", var="i", **{"from": "1"}, to="3", step="1", max_iterations=3,  # 4
         body=[step("invoke", command="probe.touch_point", major=2, args={"index": {"expr": "i"}})]),
    step("loop", mode="while", cond="not ok", max_iterations=5, body=[  # 5
        step("label", name="retry"),
        step("jump", label="retry", cond="false"),
    ]),
    step("call", subroutine="check_bore", args={"nominal": {"expr": "nominal"}}, bind="ok"),  # 6
    step("operator", descriptor={"prompt": "Confirm the part is clamped", "response": "bool"}),  # 7
    step("jump", label="end"),  # 8
    step("label", name="end"),  # 9
    step("return"),  # 10
]
all_kinds_sub = [{
    "name": "check_bore",
    "params": [{"name": "nominal", "type": "length[mm]"}],
    "returns": "bool",
    "body": [step("assign", var="d", expr="bore.diameter"), step("return", expr="abs(d - nominal) <= 0.02 mm")],
}]
all_kinds = program(
    all_kinds_main,
    meta={"title": "All step kinds", "author": "fixture"},
    params=[{"name": "nominal", "type": "length", "unit": "mm", "required": True}],
    variables=[{"name": "d", "type": "length[mm]"}, {"name": "i", "type": "int"}, {"name": "ok", "type": "bool"},
               {"name": "bore", "type": "cylinder"}, {"name": "tol_mm", "type": "real"}],
    subroutines=all_kinds_sub,
)

numbers = program([step("invoke", command="data.store_values", major=1, args={
    "values": {"literal": [1, 1.5, -0.25, 1e21, 1e-7, 0.000001, 100, 123456789012, 5e-324, 1.7976931348623157e308]},
    "label": {"literal": "Bohrung Ø \"A\"\n\t/ \u0001 \U0001F600 €"},
    "nested": {"literal": {"b": 1, "a": None, "é": True, "\U0001F600": False, "": [], "": {}}},
})])

# Edit: drop main[0] and append a new comment. Every other step keeps its ID and bytes.
NEW_ID = uuid7(1000)
NEW_TEXT = "Added by an edit."
edited = json.loads(json.dumps(all_kinds))
edited["main"] = edited["main"][1:] + [{"step_id": NEW_ID, "kind": "comment", "text": NEW_TEXT}]

valid = {"minimal": minimal, "all-kinds": all_kinds, "numbers": numbers, "edited": edited}

# ---------------------------------------------------------------- invalid programs


def mutate(f):
    p = json.loads(json.dumps(all_kinds))
    f(p)
    return canon(p)


def set_(path, value):
    def f(p):
        cur = p
        for k in path[:-1]:
            cur = cur[k]
        cur[path[-1]] = value
    return f


def delete(path):
    def f(p):
        cur = p
        for k in path[:-1]:
            cur = cur[k]
        del cur[path[-1]]
    return f


def deep_literal(levels):
    v = []
    for _ in range(levels):
        v = [v]
    return v


AK = canon(all_kinds)
INVOKE_ID = all_kinds["main"][1]["step_id"]

invalid = {
    # AC-2: same content, pretty-printed.
    "whitespace": (json.dumps(all_kinds, indent=2, ensure_ascii=False).encode("utf-8"), {"code": "non_canonical"}),
    "trailing-newline": (AK + b"\n", {"code": "non_canonical"}),
    "key-order": (json.dumps(minimal, separators=(",", ":")).encode("utf-8"), {"code": "non_canonical"}),
    "escaped": (AK.replace(b"Measure the bore", b"\\u004deasure the bore"), {"code": "non_canonical"}),
    # AC-3: the call step reuses the invoke step's ID.
    "dup-step": (mutate(set_(["main", 6, "step_id"], INVOKE_ID)), {"code": "duplicate_step_id", "id": INVOKE_ID}),
    "dup-step-sub": (mutate(set_(["subroutines", 0, "body", 1, "step_id"], INVOKE_ID)),
                     {"code": "duplicate_step_id", "id": INVOKE_ID}),
    # AC-4: an assign step carrying a command field.
    "extra-field": (mutate(set_(["main", 2, "command"], "feature.fit_cylinder")),
                    {"code": "unknown_field", "field": "command", "path": "/main/2"}),
    "invoke-effect": (mutate(set_(["main", 1, "effect"], "W2")),
                      {"code": "unknown_field", "field": "effect", "path": "/main/1"}),
    "unknown-top": (mutate(set_(["extensions"], {})), {"code": "unknown_field", "field": "extensions", "path": ""}),
    "nested-extra": (mutate(set_(["main", 3, "branches", 0, "body", 0, "command"], "x.y")),
                     {"code": "unknown_field", "field": "command", "path": "/main/3/branches/0/body/0"}),
    "arg-both": (mutate(set_(["main", 1, "args", "tol", "literal"], 1)),
                 {"code": "unknown_field", "field": "expr", "path": "/main/1/args/tol"}),
    # AC-5
    "version-2": (mutate(set_(["format_version"], 2)), {"code": "format_version_unsupported", "value": "2"}),
    "version-string": (mutate(set_(["format_version"], "1")), {"code": "schema_invalid", "path": "/format_version"}),
    "format-wrong": (mutate(set_(["format"], "fid-prog")), {"code": "schema_invalid", "path": "/format"}),
    # Schema rules.
    "uuid-v4": (mutate(set_(["program_id"], uuid7(2000, version=4))),
                {"code": "schema_invalid", "path": "/program_id"}),
    "uuid-upper": (mutate(set_(["main", 0, "step_id"], all_kinds["main"][0]["step_id"].upper())),
                   {"code": "schema_invalid", "path": "/main/0/step_id"}),
    "unknown-kind": (mutate(set_(["main", 0, "kind"], "shell")), {"code": "schema_invalid", "path": "/main/0"}),
    "bad-mode": (mutate(set_(["main", 5, "mode"], "until")), {"code": "schema_invalid", "path": "/main/5"}),
    "null-bind": (mutate(set_(["main", 1, "bind"], None)), {"code": "schema_invalid", "path": "/main/1/bind"}),
    "missing-major": (mutate(delete(["main", 1, "major"])), {"code": "schema_invalid", "path": "/main/1"}),
    "major-too-big": (mutate(set_(["main", 1, "major"], 100000)), {"code": "schema_invalid", "path": "/main/1/major"}),
    "bind-newline": (mutate(set_(["main", 1, "bind"], "bore\n")), {"code": "schema_invalid", "path": "/main/1/bind"}),
    "major-fraction": (mutate(set_(["main", 1, "major"], 1.5)), {"code": "schema_invalid", "path": "/main/1/major"}),
    "command-with-major": (mutate(set_(["main", 1, "command"], "feature.fit_cylinder@1")),
                           {"code": "schema_invalid", "path": "/main/1/command"}),
    "loop-zero": (mutate(set_(["main", 4, "max_iterations"], 0)),
                  {"code": "schema_invalid", "path": "/main/4/max_iterations"}),
    "if-no-branch": (mutate(set_(["main", 3, "branches"], [])), {"code": "schema_invalid", "path": "/main/3/branches"}),
    "arg-bad-name": (mutate(set_(["main", 1, "args", "2x"], {"expr": "1"})),
                     {"code": "schema_invalid", "path": "/main/1/args"}),
    "missing-main": (mutate(delete(["main"])), {"code": "schema_invalid", "path": ""}),
    # Not JSON.
    "dup-key": (b'{"format":"fid-program","format":"fid-program"}', {"code": "invalid_json", "detail": "duplicate_key"}),
    "not-json": (b'{"format":', {"code": "invalid_json", "detail": "syntax"}),
    "bad-utf8": (b'{"format":"\xff"}', {"code": "invalid_json", "detail": "invalid_utf8"}),
    # Bounds: JSON nesting deeper than 64.
    "deep": (mutate(set_(["main", 1, "args", "points", "literal"], deep_literal(70))),
             {"code": "program_too_large", "bound": "depth"}),
}

SCHEMA_CODES = {"unknown_field", "schema_invalid"}


def strict_validator(jsonschema):
    """Draft 2020-12 whose `pattern` anchors `$` at the very end (ECMA-262 semantics).

    Python's `re.search` lets `$` match before a trailing newline, which would accept
    "bore\n" as an identifier; the Rust checks and ECMA-262 do not.
    """
    import re

    def pattern(validator, pat, instance, schema):
        if not validator.is_type(instance, "string"):
            return
        strict = pat[:-1] + r"\Z" if pat.endswith("$") else pat
        if not re.search(strict, instance):
            yield jsonschema.ValidationError(f"{instance!r} does not match {pat!r}")

    return jsonschema.validators.extend(jsonschema.Draft202012Validator, {"pattern": pattern})


def main():
    for sub in ("valid", "invalid"):
        d = HERE / sub
        d.mkdir(exist_ok=True)
        for old in d.glob("*.json"):
            old.unlink()
    expected = {"valid": {}, "invalid": {}, "edit": {
        "base": "all-kinds", "remove_main_index": 0, "append_comment": {"step_id": NEW_ID, "text": NEW_TEXT},
        "result": "edited"}}
    for name, p in valid.items():
        b = canon(p)
        (HERE / "valid" / f"{name}.json").write_bytes(b)
        expected["valid"][name] = digest(b)
    for name, (b, label) in invalid.items():
        (HERE / "invalid" / f"{name}.json").write_bytes(b)
        expected["invalid"][name] = label
    # Sanity of the hand-made invalid inputs: the canonical ones differ from all-kinds only as labelled.
    assert invalid["whitespace"][0] != AK and json.loads(invalid["whitespace"][0]) == all_kinds
    assert invalid["escaped"][0] != AK and json.loads(invalid["escaped"][0]) == all_kinds
    (HERE / "expected.json").write_text(json.dumps(expected, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    schema_path = ROOT / "schema" / "program-v1.json"
    try:
        import jsonschema
    except ImportError:
        print("jsonschema not installed; schema cross-check skipped")
        return
    if not schema_path.exists():
        print("schema/program-v1.json missing; schema cross-check skipped")
        return
    schema = json.loads(schema_path.read_text(encoding="utf-8"))
    v = strict_validator(jsonschema)(schema)
    for name, p in valid.items():
        errs = list(v.iter_errors(p))
        assert not errs, (name, errs[0].message)
    for name, (b, label) in invalid.items():
        if label["code"] in SCHEMA_CODES:
            assert list(v.iter_errors(json.loads(b))), f"{name} should fail the schema"
    print("schema cross-check passed")


if __name__ == "__main__":
    main()
