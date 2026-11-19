#!/usr/bin/env python3
"""Generate the part-family fixtures of spec node AUTO.PROG.FAMILY.

Independent reference: this script writes the programs and binding files with its own
RFC 8785 writer (only strings, booleans, integers, null, arrays and objects occur, so no
float printing is needed) and labels every expected error code by hand from the rules
pinned in spec/auto/prog/family/NODE.md (Design). Nothing here runs the Rust code.

Unit catalogue: fixtures/core/reg/def/schema/catalogue/with_time.json (kinds angle, length,
time). Expected entries are {"code", "param", "variant"} with null where not applicable,
in the order the Design pins.

CC-BY-4.0 project data. Run:  python fixtures/program/family/make_fixtures.py
"""

import copy
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent

# ---------------------------------------------------------------- RFC 8785 writer (subset)


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
    if isinstance(v, int):
        # str() is the ECMAScript form only for exactly representable integers; the family
        # accepts magnitudes up to 2^53 - 1, so canonical fixtures stay within that range.
        assert abs(v) <= 2**53 - 1
        return str(v)
    if isinstance(v, float):
        # Only values whose shortest repr is already the ECMAScript form are used here.
        r = repr(v)
        assert r in ("4.5",), r
        return r
    if isinstance(v, str):
        return jcs_string(v)
    if isinstance(v, list):
        return "[" + ",".join(jcs(x) for x in v) + "]"
    if isinstance(v, dict):
        keys = sorted(v, key=lambda k: k.encode("utf-16-be"))
        return "{" + ",".join(jcs_string(k) + ":" + jcs(v[k]) for k in keys) + "}"
    raise TypeError(type(v))


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(jcs(value).encode("utf-8"))


# ---------------------------------------------------------------- the bracket family


def q(value, unit):
    return {"unit": unit, "value": value}


SCAN = "input:scans/bracket-120.ply"

PARAMS = [
    {"description": "Centre distance of the two mounting holes", "name": "hole_spacing",
     "quantity_kind": "length", "required": True, "type": "quantity"},
    {"default": 4, "description": "Number of mounting holes", "name": "hole_count", "required": True,
     "type": "integer"},
    {"description": "Input scan of the part", "name": "scan", "required": True, "type": "data_handle"},
    {"description": "Element that carries the datum hole", "name": "datum_hole", "required": False,
     "type": "ref:element"},
]

VARIANTS = [
    {"binding": {"hole_spacing": q("120", "mm"), "scan": SCAN}, "name": "B-120"},
    {"binding": {"hole_count": 6, "hole_spacing": q("4.724", "in"), "scan": "input:scans/bracket-4in.ply"},
     "name": "B-4IN"},
]

STEP_COMMENT = {"kind": "comment", "step_id": "018f0000-0000-7000-8000-000000000101",
                "text": "Measure the bracket family"}
STEP_INVOKE = {"args": {"tolerance": {"expr": "hole_spacing"}}, "command": "align.run_bestfit", "kind": "invoke",
               "major": 1, "step_id": "018f0000-0000-7000-8000-000000000102"}


def bracket(params=None, variants=None):
    p = {"format": "fid-program", "format_version": 1, "main": [STEP_COMMENT, STEP_INVOKE],
         "meta": {"title": "Bracket family"}, "params": copy.deepcopy(PARAMS if params is None else params),
         "program_id": "018f0000-0000-7000-8000-000000000100", "subroutines": [], "variables": []}
    v = VARIANTS if variants is None else variants
    if v != "absent":
        p["variants"] = copy.deepcopy(v)
    return p


def err(code, param=None, variant=None):
    return {"code": code, "param": param, "variant": variant}


def with_param(i, **changes):
    ps = copy.deepcopy(PARAMS)
    for k, v in changes.items():
        if v is None:
            ps[i].pop(k, None)
        else:
            ps[i][k] = v
    return ps


# ---------------------------------------------------------------- programs (AC-1, AC-6)

PROGRAMS = {
    # AC-1: valid; variant B-120 checks clean.
    "bracket": (bracket(), []),
    # No variant table at all: still a valid family.
    "valid/no-variants": (bracket(variants="absent"), []),
    # Empty variant table.
    "valid/empty-variants": (bracket(variants=[]), []),
    # AC-6: two variants named B-120; the later one is named.
    "invalid/dup-variant": (bracket(variants=[VARIANTS[0], VARIANTS[0]]), [err("duplicate_variant", None, "B-120")]),
    # Names compared ignoring ASCII case.
    "invalid/case-variant": (
        bracket(variants=[VARIANTS[0], dict(copy.deepcopy(VARIANTS[0]), name="b-120")]),
        [err("duplicate_variant", None, "b-120")],
    ),
    # A variant must be a complete binding: scan is required and has no default.
    "invalid/variant-incomplete": (
        bracket(variants=[VARIANTS[0], {"binding": {"hole_spacing": q("90", "mm")}, "name": "B-90"}]),
        [err("missing_parameter", "scan", "B-90")],
    ),
    # Variant bindings are checked like any binding.
    "invalid/variant-wrong-unit": (
        bracket(variants=[{"binding": {"hole_spacing": q("5", "s"), "scan": SCAN}, "name": "B-5S"}]),
        [err("wrong_unit", "hole_spacing", "B-5S")],
    ),
    "invalid/variant-bad-name": (
        bracket(variants=[{"binding": {"hole_spacing": q("120", "mm"), "scan": SCAN}, "name": "B 120"}]),
        [err("bad_variant", None, "B 120")],
    ),
    "invalid/variant-extra-field": (
        bracket(variants=[dict(copy.deepcopy(VARIANTS[0]), note="x")]),
        [err("bad_variant", None, "B-120")],
    ),
    "invalid/dup-param": (bracket(params=PARAMS + [PARAMS[0]]), [err("duplicate_parameter", "hole_spacing")]),
    "invalid/case-param": (
        bracket(params=PARAMS + [dict(PARAMS[0], name="Hole_Spacing")]),
        [err("duplicate_parameter", "Hole_Spacing")],
    ),
    "invalid/quantity-no-kind": (
        bracket(params=with_param(0, quantity_kind=None)),
        [err("bad_declaration", "hole_spacing")],
    ),
    "invalid/kind-on-integer": (
        bracket(params=with_param(1, quantity_kind="length")),
        [err("bad_declaration", "hole_count")],
    ),
    "invalid/unknown-kind": (
        bracket(params=with_param(0, quantity_kind="mass")),
        [err("bad_declaration", "hole_spacing")],
    ),
    "invalid/unknown-type": (bracket(params=with_param(0, type="length")), [err("bad_declaration", "hole_spacing")]),
    "invalid/unknown-ref-entity": (
        bracket(params=with_param(3, type="ref:widget")),
        [err("bad_declaration", "datum_hole")],
    ),
    "invalid/bad-default": (bracket(params=with_param(1, default="four")), [err("bad_declaration", "hole_count")]),
    "invalid/default-wrong-unit": (
        bracket(params=with_param(0, default=q("5", "s"))),
        [err("bad_declaration", "hole_spacing")],
    ),
    "invalid/decl-unknown-field": (bracket(params=with_param(0, unit="mm")), [err("bad_declaration", "hole_spacing")]),
    "invalid/required-missing": (bracket(params=with_param(2, required=None)), [err("bad_declaration", "scan")]),
    "invalid/bad-param-name": (
        bracket(params=with_param(1, name="hole count")),
        [err("bad_declaration", "hole count")],
    ),
    # Two bad declarations: both reported, in declaration order; variants then not checked.
    "invalid/two-bad-decls": (
        bracket(params=[dict(PARAMS[0], type="length"), PARAMS[1], dict(PARAMS[2], required="yes"), PARAMS[3]]),
        [err("bad_declaration", "hole_spacing"), err("bad_declaration", "scan")],
    ),
}

# ---------------------------------------------------------------- bindings (AC-2 to AC-5)

OK = {"hole_spacing": q("120", "mm"), "scan": SCAN}


def b(**kv):
    out = copy.deepcopy(OK)
    for k, v in kv.items():
        if v is None:
            out.pop(k, None)
        else:
            out[k] = v
    return out


BINDINGS = {
    # AC-2
    "missing-hole-spacing": ({"scan": SCAN}, [err("missing_parameter", "hole_spacing")]),
    # AC-3
    "seconds": (b(hole_spacing=q("5", "s")), [err("wrong_unit", "hole_spacing")]),
    "hole-count-four": (b(hole_count="four"), [err("wrong_type", "hole_count")]),
    # AC-4
    "colour": (b(colour="red"), [err("unknown_parameter", "colour")]),
    # AC-5
    "inches": (b(hole_spacing=q("4.724", "in")), []),
    # More
    "full": (b(hole_count=6, datum_hole="element:hole-1"), []),
    "case-collision": (
        {"Hole_spacing": q("120", "mm"), "scan": SCAN},
        [err("missing_parameter", "hole_spacing"), err("unknown_parameter", "Hole_spacing")],
    ),
    "bare-number": (b(hole_spacing=120), [err("wrong_type", "hole_spacing")]),
    "bad-decimal": (b(hole_spacing=q("1,5", "mm")), [err("wrong_type", "hole_spacing")]),
    "number-value": (b(hole_spacing={"unit": "mm", "value": 120}), [err("wrong_type", "hole_spacing")]),
    "degrees": (b(hole_spacing=q("90", "deg")), [err("wrong_unit", "hole_spacing")]),
    "unknown-unit": (b(hole_spacing=q("5", "furlong")), [err("wrong_unit", "hole_spacing")]),
    "wrong-case-unit": (b(hole_spacing=q("5", "MM")), [err("wrong_unit", "hole_spacing")]),
    "fractional-count": (b(hole_count=4.5), [err("wrong_type", "hole_count")]),
    "bool-count": (b(hole_count=True), [err("wrong_type", "hole_count")]),
    "empty-handle": (b(scan=""), [err("wrong_type", "scan")]),
    "control-in-handle": (b(scan="input:\nscan"), [err("wrong_type", "scan")]),
    "number-handle": (b(scan=5), [err("wrong_type", "scan")]),
    "null-optional": (b(datum_hole=None) | {"datum_hole": None}, [err("wrong_type", "datum_hole")]),
    "quantity-extra-field": (
        b(hole_spacing={"note": "x", "unit": "mm", "value": "120"}),
        [err("wrong_type", "hole_spacing")],
    ),
    # Declaration order first, then unknown keys in byte order.
    "many-errors": (
        {"colour": 1, "hole_count": "four", "hole_spacing": q("5", "s"), "zeta": True},
        [
            err("wrong_unit", "hole_spacing"),
            err("wrong_type", "hole_count"),
            err("missing_parameter", "scan"),
            err("unknown_parameter", "colour"),
            err("unknown_parameter", "zeta"),
        ],
    ),
}

# Binding files written as raw JSON text (not canonical), checked like any binding.
# Integer limit is 2^53 - 1: 2^53 is refused, and 2^53 + 1 parses (to f64) as 2^53, so it
# is refused too rather than silently accepted as a different integer.
SCAN_JSON = jcs_string(SCAN)
TEXT_BINDINGS = {
    "count-2p53-minus-1": (
        '{"hole_count":%d,"hole_spacing":{"unit":"mm","value":"120"},"scan":%s}' % (2**53 - 1, SCAN_JSON),
        [],
    ),
    "count-2p53": (
        '{"hole_count":%d,"hole_spacing":{"unit":"mm","value":"120"},"scan":%s}' % (2**53, SCAN_JSON),
        [err("wrong_type", "hole_count")],
    ),
    "count-2p53-plus-1": (
        '{"hole_count":%d,"hole_spacing":{"unit":"mm","value":"120"},"scan":%s}' % (2**53 + 1, SCAN_JSON),
        [err("wrong_type", "hole_count")],
    ),
    "count-negative-2p53": (
        '{"hole_count":%d,"hole_spacing":{"unit":"mm","value":"120"},"scan":%s}' % (-(2**53), SCAN_JSON),
        [err("wrong_type", "hole_count")],
    ),
}

# Raw binding files that are not valid binding documents: (text, bad_binding reason).
RAW_BINDINGS = {
    "not-object": ("[1]", "a binding must be a JSON object"),
    "dup-key": ('{"scan":"a","scan":"b"}', "duplicate_key"),
}


def main():
    expected = {"catalogue": "fixtures/core/reg/def/schema/catalogue/with_time.json", "programs": {},
                "bindings": {}, "raw_bindings": {}}
    for name, (prog, errs) in PROGRAMS.items():
        write(HERE / f"{name}.json", prog)
        expected["programs"][name] = errs
    for name, (binding, errs) in BINDINGS.items():
        write(HERE / "bindings" / f"{name}.json", binding)
        expected["bindings"][name] = errs
    for name, (text, errs) in TEXT_BINDINGS.items():
        (HERE / "bindings" / f"{name}.json").write_bytes(text.encode("utf-8"))
        assert json.loads(text) is not None
        expected["bindings"][name] = errs
    for name, (text, reason) in RAW_BINDINGS.items():
        (HERE / "bindings" / f"{name}.json").write_bytes(text.encode("utf-8"))
        expected["raw_bindings"][name] = reason
    write(HERE / "expected.json", expected)
    # Sanity: every written file is JSON that Python reads back to the same value.
    for name, (prog, _) in PROGRAMS.items():
        assert json.loads((HERE / f"{name}.json").read_text("utf-8")) == prog
    print(f"{len(PROGRAMS)} programs, {len(BINDINGS) + len(TEXT_BINDINGS) + len(RAW_BINDINGS)} bindings")


if __name__ == "__main__":
    main()
