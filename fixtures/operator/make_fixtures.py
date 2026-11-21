#!/usr/bin/env python3
"""Generate the operator-descriptor fixtures of spec node AUTO.PROG.OPER.

Independent reference, stdlib only. Every expected result below is labelled by hand from
the rules pinned in spec/auto/prog/oper/NODE.md (Design). A separate oracle in this file
(`check_descriptor`, `validate`) implements those rules again in Python, with exact
decimals through `fractions.Fraction`, and must agree with every hand label before
anything is written. Nothing here runs or reads the Rust code.

Unit table for the tests: units.json (kinds length and temperature; the test catalogue
of tests/oper.rs serves exactly these symbols). Expected errors are {"code", "field",
"index"} with null where not applicable, in the order the Design pins. A valid answer is
{"<field>": {"type", "value"[, "untrusted": true]}}.

CC-BY-4.0 project data. Run:   python fixtures/operator/make_fixtures.py
Verify (CI):                    python fixtures/operator/make_fixtures.py --check
"""

import json
import re
import sys
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent

# ---------------------------------------------------------------- writer


def dump(v):
    # Keys are ASCII, values are strings, booleans, small integers, arrays and objects, so
    # sorted keys with compact separators are the RFC 8785 form.
    return (json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False) + "\n").encode("utf-8")


# ---------------------------------------------------------------- oracle

UNITS = {"length": ["m", "mm", "um"], "temperature": ["K", "°C", "°F"]}

TYPES = ["ack", "bool", "integer", "decimal", "quantity", "text", "choice", "element"]
RULES = ["choices", "max", "max_length", "min", "required"]
ALLOWED = {
    "ack": ["required"],
    "bool": ["required"],
    "element": ["required"],
    "integer": ["max", "min", "required"],
    "decimal": ["max", "min", "required"],
    "quantity": ["max", "min", "required"],
    "text": ["max_length", "required"],
    "choice": ["choices", "required"],
}
DESCRIPTOR_KEYS = ["fields", "format", "image", "prompt"]
FIELD_KEYS = ["default", "label", "name", "quantity_kind", "response", "rules", "unit"]

# CONSTITUTION C6 display-text table (fid-types `text::FORBIDDEN_RANGES`, Unicode 16.0),
# plus every plane-end noncharacter.
FORBIDDEN = [
    (0x0000, 0x001F), (0x007F, 0x009F), (0x00AD, 0x00AD), (0x0600, 0x0605), (0x061C, 0x061C),
    (0x06DD, 0x06DD), (0x070F, 0x070F), (0x0890, 0x0891), (0x08E2, 0x08E2), (0x180E, 0x180E),
    (0x200B, 0x200F), (0x2028, 0x2029), (0x202A, 0x202E), (0x2060, 0x2064), (0x2066, 0x206F),
    (0xFDD0, 0xFDEF), (0xFEFF, 0xFEFF), (0xFFF9, 0xFFFB), (0x110BD, 0x110BD), (0x110CD, 0x110CD),
    (0x13430, 0x1343F), (0x1BCA0, 0x1BCA3), (0x1D173, 0x1D17A), (0xE0000, 0xE007F),
]


def forbidden(ch):
    c = ord(ch)
    return (c & 0xFFFE) == 0xFFFE or any(lo <= c <= hi for lo, hi in FORBIDDEN)


def display_text(s, max_bytes, newline=False):
    return (isinstance(s, str) and 0 < len(s.encode("utf-8")) <= max_bytes
            and not any(forbidden(ch) and not (newline and ch == "\n") for ch in s))


DEC_RE = re.compile(r"[+-]?[0-9]+(\.[0-9]+)?\Z")
INT_RE = re.compile(r"[+-]?[0-9]+\Z")
NAME_RE = re.compile(r"[A-Za-z_][A-Za-z0-9_]{0,63}\Z")
IMAGE_RE = re.compile(r"root://[A-Za-z0-9][A-Za-z0-9_-]{0,63}(/[A-Za-z0-9._-]*[A-Za-z0-9_-])+\Z")


def decimal(s, integer=False):
    """Exact value of a decimal text, or None."""
    if not isinstance(s, str) or len(s) > 64 or not (INT_RE if integer else DEC_RE).match(s):
        return None
    digits = s.lstrip("+-").replace(".", "").lstrip("0")
    if len(digits) > 40:
        return None
    return Fraction(s)


def is_int(x):
    return isinstance(x, int) and not isinstance(x, bool)


def err(code, field=None, index=None):
    return {"code": code, "field": field, "index": index}


def check_value(f, v):
    """(value dict, None) or (None, code) for a present answer value."""
    t, rules = f["response"], f["rules"]

    def bounded(x):
        if "min" in rules and x < rules["min"]:
            return "below_min"
        if "max" in rules and x > rules["max"]:
            return "above_max"
        return None

    if t == "ack":
        return ({"type": "ack", "value": True}, None) if v is True else (None, "wrong_type")
    if t == "bool":
        return ({"type": "bool", "value": v}, None) if isinstance(v, bool) else (None, "wrong_type")
    if t in ("integer", "decimal"):
        x = decimal(v, integer=(t == "integer"))
        if x is None:
            return None, "wrong_type"
        code = bounded(x)
        return (None, code) if code else ({"type": t, "value": v}, None)
    if t == "quantity":
        if not (isinstance(v, dict) and sorted(v) == ["unit", "value"] and isinstance(v["unit"], str)):
            return None, "wrong_type"
        x = decimal(v["value"])
        if x is None:
            return None, "wrong_type"
        if v["unit"] != f["unit"]:
            return None, "wrong_unit"
        code = bounded(x)
        return (None, code) if code else ({"type": "quantity", "value": v}, None)
    if t == "text":
        if not isinstance(v, str):
            return None, "wrong_type"
        if v == "" and rules["required"]:
            return None, "missing"
        if len(v.encode("utf-8")) > 65536 or len(v) > rules.get("max_length", 65536):
            return None, "too_long"
        return {"type": "text", "value": v, "untrusted": True}, None
    if t == "choice":
        if not isinstance(v, str):
            return None, "wrong_type"
        return ({"type": "choice", "value": v}, None) if v in rules["choices"] else (None, "not_a_choice")
    if t == "element":
        ok = display_text(v, 4096)
        return ({"type": "element", "value": v, "untrusted": True}, None) if ok else (None, "wrong_type")
    raise AssertionError(t)


def check_field(i, f, out):
    name = f.get("name") if isinstance(f.get("name"), str) else None
    start = len(out)

    def bad(code="bad_descriptor"):
        out.append(err(code, name, i))

    if any(k not in FIELD_KEYS for k in f):
        bad()
    if not (isinstance(f.get("name"), str) and NAME_RE.match(f["name"])):
        bad()
    if "label" in f and not display_text(f["label"], 256):
        bad()
    t = f.get("response")
    if t not in TYPES:
        bad()
        return None
    if t == "quantity":
        k, u = f.get("quantity_kind"), f.get("unit")
        if not (isinstance(k, str) and isinstance(u, str)):
            bad()
        elif u not in UNITS.get(k, []):
            bad()
    elif "quantity_kind" in f or "unit" in f:
        bad()
    rules = {"required": True}
    raw = f.get("rules", {})
    if not isinstance(raw, dict):
        bad()
        raw = {}
    for k in sorted(raw):
        v = raw[k]
        if k not in RULES:
            bad()
        elif k not in ALLOWED[t]:
            bad("rule_not_allowed")
        elif k == "required":
            if isinstance(v, bool):
                rules["required"] = v
            else:
                bad()
        elif k in ("min", "max"):
            x = decimal(v, integer=(t == "integer"))
            if x is None:
                bad()
            else:
                rules[k] = x
        elif k == "max_length":
            if is_int(v) and 1 <= v <= 65536:
                rules[k] = v
            else:
                bad()
        elif k == "choices":
            if not (isinstance(v, list) and 1 <= len(v) <= 64):
                bad()
                continue
            kept = []
            for c in v:
                if not display_text(c, 256):
                    bad()
                elif c in kept:
                    bad()
                else:
                    kept.append(c)
            rules["choices"] = kept
    if "min" in rules and "max" in rules and rules["min"] > rules["max"]:
        bad()
    if t == "choice" and "choices" not in raw:
        bad()
    field = {"name": name, "response": t, "rules": rules, "unit": f.get("unit")}
    if "default" in f:
        if t == "ack":
            bad()
        elif len(out) == start:
            value, code = check_value(field, f["default"])
            if code:
                bad()
            else:
                field["default"] = value
    return field if len(out) == start else None


def check_descriptor(d):
    """(fields, errors) for a descriptor."""
    out = []
    top = lambda: out.append(err("bad_descriptor"))  # noqa: E731
    if not isinstance(d, dict):
        return None, [err("bad_descriptor")]
    if any(k not in DESCRIPTOR_KEYS for k in d):
        top()
    if d.get("format") != "operator-v1":
        top()
    if not display_text(d.get("prompt"), 8192, newline=True):
        top()
    if "image" in d and not (isinstance(d["image"], str) and len(d["image"]) <= 4096 and IMAGE_RE.match(d["image"])):
        top()
    fields = d.get("fields")
    if not isinstance(fields, list):
        top()
        fields = []
    elif len(fields) > 64:
        top()
        fields = []
    checked, seen = [], set()
    for i, f in enumerate(fields):
        if not isinstance(f, dict):
            out.append(err("bad_descriptor", None, i))
            continue
        n = f.get("name")
        if isinstance(n, str) and NAME_RE.match(n):
            if n.lower() in seen:
                out.append(err("bad_descriptor", n, i))
            seen.add(n.lower())
        c = check_field(i, f, out)
        if c:
            checked.append(c)
    return (checked if not out else None), out


def check_raw(text):
    """Errors for descriptor JSON text: a duplicate key, a number literal with a fraction or
    exponent, nesting deeper than 8, or a non-object refuse the document."""

    def pairs(items):
        keys = [k for k, _ in items]
        if len(keys) != len(set(keys)):
            raise ValueError("duplicate key")
        return dict(items)

    def no_float(s):
        raise ValueError("non-integer number literal " + s)

    def depth(v):
        if isinstance(v, dict):
            return 1 + max([depth(x) for x in v.values()] or [0])
        if isinstance(v, list):
            return 1 + max([depth(x) for x in v] or [0])
        return 0

    try:
        d = json.loads(text, object_pairs_hook=pairs, parse_float=no_float)
    except ValueError:
        return [err("bad_descriptor")]
    if not isinstance(d, dict) or depth(d) > 8:
        return [err("bad_descriptor")]
    return check_descriptor(d)[1]


def validate(fields, answer):
    if not isinstance(answer, dict):
        return None, [err("bad_answer")]
    if len(answer) > 256:
        return None, [err("bad_answer")]
    ok, errors = {}, []
    for f in fields:
        n = f["name"]
        if n in answer:
            value, code = check_value(f, answer[n])
            if code:
                errors.append(err(code, n))
            else:
                ok[n] = value
        elif "default" in f:
            ok[n] = f["default"]
        elif f["rules"]["required"]:
            errors.append(err("missing", n))
    names = {f["name"] for f in fields}
    for k in sorted(answer, key=lambda s: s.encode("utf-8")):
        if k not in names:
            errors.append(err("unknown_field", k))
    return (None, errors) if errors else (ok, [])


# ---------------------------------------------------------------- descriptors

DEG_C, DEG_F = "°C", "°F"

DESCRIPTORS = {
    # AC-1.
    "temp-check": {
        "format": "operator-v1",
        "prompt": "Read the part temperature at the probe datum and enter it.",
        "fields": [{"name": "temp", "label": "Part temperature", "response": "quantity",
                    "quantity_kind": "temperature", "unit": DEG_C, "rules": {"min": "18", "max": "22"}}],
    },
    # AC-2: field order (count, note, fixture) differs from key byte order (count, fixture, note).
    "form3": {
        "format": "operator-v1",
        "prompt": "Record the setup.",
        "fields": [
            {"name": "count", "response": "integer", "rules": {"min": "1", "max": "10"}},
            {"name": "note", "response": "text", "rules": {"max_length": 5}},
            {"name": "fixture", "response": "choice", "rules": {"choices": ["F1", "F2"]}},
        ],
    },
    # AC-3.
    "choice": {
        "format": "operator-v1",
        "prompt": "Which grade?",
        "fields": [{"name": "grade", "response": "choice", "rules": {"choices": ["A", "B"]}}],
    },
    # AC-4.
    "text": {
        "format": "operator-v1",
        "prompt": "Any remarks?",
        "fields": [{"name": "remark", "response": "text", "rules": {"required": False}}],
    },
    "all-types": {
        "format": "operator-v1",
        "prompt": "Prepare the part.\nThen answer every question.",
        "image": "root://shop/setup/fixture-a.png",
        "fields": [
            {"name": "ready", "response": "ack"},
            {"name": "clamped", "response": "bool", "default": False},
            {"name": "passes", "response": "integer", "rules": {"min": "-3", "max": "+3"}, "default": "0"},
            {"name": "offset", "response": "decimal", "rules": {"max": "10.1"}},
            {"name": "depth", "response": "quantity", "quantity_kind": "length", "unit": "mm",
             "rules": {"min": "0"}},
            {"name": "remark", "response": "text", "rules": {"max_length": 3, "required": False}},
            {"name": "side", "response": "choice", "rules": {"choices": ["left", "right"]}, "default": "left"},
            {"name": "feature", "response": "element", "label": "Pick the bore"},
            {"name": "big", "response": "integer", "rules": {"max": "1" + "0" * 38, "required": False}},
        ],
    },
    # Display only (a comment shown to the operator).
    "comment": {
        "format": "operator-v1",
        "prompt": "Remove the part.\nThen continue.",
        "image": "root://shop/img/unload.png",
        "fields": [],
    },
}

# Descriptor checks: name -> (descriptor, hand-labelled errors).
BOOL = {"name": "ok", "response": "bool"}


def desc(fields, **top):
    d = {"format": "operator-v1", "prompt": "p", "fields": fields}
    d.update(top)
    return d


INVALID = {
    # AC-5.
    "bool-max-length": (
        desc([{"name": "ok", "response": "bool", "rules": {"max_length": 4}}]),
        [err("rule_not_allowed", "ok", 0)],
    ),
    "rules-on-wrong-types": (
        desc([
            {"name": "a", "response": "text", "rules": {"min": "1"}},
            {"name": "b", "response": "choice", "rules": {"choices": ["x"], "max_length": 2}},
            {"name": "c", "response": "integer", "rules": {"choices": ["1"]}},
            {"name": "d", "response": "ack", "rules": {"max": "1"}},
            {"name": "e", "response": "element", "rules": {"max_length": 9}},
        ]),
        [err("rule_not_allowed", n, i) for i, n in enumerate("abcde")],
    ),
    "unknown-rule": (desc([{"name": "a", "response": "text", "rules": {"pattern": "x"}}]),
                     [err("bad_descriptor", "a", 0)]),
    "rules-not-object": (desc([{"name": "a", "response": "text", "rules": ["required"]}]),
                         [err("bad_descriptor", "a", 0)]),
    "unknown-top-key": (desc([], timeout=5), [err("bad_descriptor")]),
    "unknown-field-key": (desc([{"name": "a", "response": "bool", "hint": "x"}]), [err("bad_descriptor", "a", 0)]),
    "duplicate-name-case": (desc([{"name": "Temp", "response": "bool"}, {"name": "temp", "response": "bool"}]),
                            [err("bad_descriptor", "temp", 1)]),
    "quantity-units": (
        desc([
            {"name": "a", "response": "quantity", "quantity_kind": "temperature", "unit": "degC"},
            {"name": "b", "response": "quantity", "quantity_kind": "temperature"},
            {"name": "c", "response": "bool", "unit": "mm"},
            {"name": "d", "response": "quantity", "quantity_kind": "pressure", "unit": "Pa"},
            {"name": "e", "response": "quantity", "quantity_kind": "length", "unit": DEG_C},
        ]),
        [err("bad_descriptor", n, i) for i, n in enumerate("abcde")],
    ),
    "bounds": (
        desc([
            {"name": "a", "response": "integer", "rules": {"min": "5", "max": "4"}},
            {"name": "b", "response": "integer", "rules": {"min": "1.5"}},
            {"name": "c", "response": "decimal", "rules": {"max": 3}},
            {"name": "d", "response": "decimal", "rules": {"min": "1e3"}},
            {"name": "e", "response": "decimal", "rules": {"min": "10.10", "max": "10.1"}},
        ]),
        [err("bad_descriptor", n, i) for i, n in enumerate("abcd")],
    ),
    "choices": (
        desc([
            {"name": "a", "response": "choice"},
            {"name": "b", "response": "choice", "rules": {"choices": []}},
            {"name": "c", "response": "choice", "rules": {"choices": ["A", "A"]}},
            {"name": "d", "response": "choice", "rules": {"choices": ["A", ""]}},
            {"name": "e", "response": "choice", "rules": {"choices": ["A", "B‮"]}},
            {"name": "f", "response": "choice", "rules": {"choices": ["A", "a"]}},
        ]),
        [err("bad_descriptor", n, i) for i, n in enumerate("abcde")],
    ),
    "defaults": (
        desc([
            {"name": "a", "response": "integer", "rules": {"max": "3"}, "default": "4"},
            {"name": "b", "response": "ack", "default": True},
            {"name": "c", "response": "choice", "rules": {"choices": ["A"]}, "default": "C"},
            {"name": "d", "response": "text", "default": ""},
            {"name": "e", "response": "quantity", "quantity_kind": "length", "unit": "mm",
             "default": {"unit": "m", "value": "1"}},
            {"name": "f", "response": "bool", "default": "true"},
        ]),
        [err("bad_descriptor", n, i) for i, n in enumerate("abcdef")],
    ),
    "format-and-prompt": ({"format": "operator-v2", "prompt": "", "fields": []},
                          [err("bad_descriptor"), err("bad_descriptor")]),
    "prompt-bidi": (desc([], prompt="Press OK‮"), [err("bad_descriptor")]),
    "prompt-too-long": (desc([], prompt="x" * 8193), [err("bad_descriptor")]),
    "missing-top": ({"fields": []}, [err("bad_descriptor"), err("bad_descriptor")]),
    "image-raw-path": (desc([], image="/etc/part.png"), [err("bad_descriptor")]),
    "image-dotdot": (desc([], image="root://shop/../x.png"), [err("bad_descriptor")]),
    "image-no-file": (desc([], image="root://shop"), [err("bad_descriptor")]),
    "bad-names": (
        desc([{"name": "1x", "response": "bool"}, {"response": "bool"}, {"name": "a" * 65, "response": "bool"}]),
        [err("bad_descriptor", "1x", 0), err("bad_descriptor", None, 1), err("bad_descriptor", "a" * 65, 2)],
    ),
    "unknown-response": (desc([{"name": "a", "response": "date"}, {"name": "b", "response": "Bool"}]),
                         [err("bad_descriptor", "a", 0), err("bad_descriptor", "b", 1)]),
    "too-many-fields": (desc([{"name": f"f{i}", "response": "bool"} for i in range(65)]), [err("bad_descriptor")]),
    "fields-not-array": (desc({"a": BOOL}), [err("bad_descriptor")]),
    "field-not-object": (desc(["x", BOOL]), [err("bad_descriptor", None, 0)]),
    "max-length-values": (
        desc([
            {"name": "a", "response": "text", "rules": {"max_length": 0}},
            {"name": "b", "response": "text", "rules": {"max_length": 65537}},
            {"name": "c", "response": "text", "rules": {"max_length": "5"}},
            {"name": "d", "response": "text", "rules": {"max_length": 65536}},
        ]),
        [err("bad_descriptor", "a", 0), err("bad_descriptor", "b", 1), err("bad_descriptor", "c", 2)],
    ),
    "required-not-bool": (desc([{"name": "a", "response": "bool", "rules": {"required": "yes"}}]),
                          [err("bad_descriptor", "a", 0)]),
    "label-control": (desc([{"name": "a", "response": "bool", "label": "A\u0007"}]), [err("bad_descriptor", "a", 0)]),
    "sixty-four-fields": (desc([{"name": f"f{i}", "response": "bool"} for i in range(64)]), []),
}

# Raw descriptor texts that are not bounded JSON objects.
RAW_INVALID = {
    "dup-key": '{"fields":[],"format":"operator-v1","prompt":"a","prompt":"b"}',
    "not-object": "[]",
    "too-deep": '{"fields":[{"name":"a","response":"bool","rules":{"required":[[[[[[true]]]]]]}}],'
                '"format":"operator-v1","prompt":"p"}',
    # JSON reads 5.0 and 5e0 as numbers equal to 5; a descriptor allows integer literals only.
    "max-length-float": '{"fields":[{"name":"a","response":"text","rules":{"max_length":5.0}}],'
                        '"format":"operator-v1","prompt":"p"}',
    "max-length-exponent": '{"fields":[{"name":"a","response":"text","rules":{"max_length":5e0}}],'
                           '"format":"operator-v1","prompt":"p"}',
}

# Answers: name -> (descriptor name, answer, hand-labelled result). A result is either
# {"ok": {...}} or {"errors": [...]}.
Q = lambda v, u=DEG_C: {"unit": u, "value": v}  # noqa: E731


def ok(**fields):
    return {"ok": fields}


def errs(*e):
    return {"errors": list(e)}


def qv(v, u=DEG_C):
    return {"type": "quantity", "value": Q(v, u)}


ALL_GOOD = {
    "ready": True, "clamped": True, "passes": "-3", "offset": "10.10", "depth": Q("4.500", "mm"),
    "remark": "été", "side": "right", "feature": "elem:bore-2", "big": "1" + "0" * 38,
}

ANSWERS = {
    # AC-1.
    "temp-20": ("temp-check", {"temp": Q("20.0")}, ok(temp=qv("20.0"))),
    "temp-22.01": ("temp-check", {"temp": Q("22.01")}, errs(err("above_max", "temp"))),
    "temp-68F": ("temp-check", {"temp": Q("68", DEG_F)}, errs(err("wrong_unit", "temp"))),
    "temp-edges": ("temp-check", {"temp": Q("22.000")}, ok(temp=qv("22.000"))),
    "temp-min": ("temp-check", {"temp": Q("+18")}, ok(temp=qv("+18"))),
    "temp-just-above": ("temp-check", {"temp": Q("22.0000000001")}, errs(err("above_max", "temp"))),
    "temp-17.99": ("temp-check", {"temp": Q("17.99")}, errs(err("below_min", "temp"))),
    "temp-kelvin": ("temp-check", {"temp": Q("293.15", "K")}, errs(err("wrong_unit", "temp"))),
    "temp-exponent": ("temp-check", {"temp": Q("2e1")}, errs(err("wrong_type", "temp"))),
    "temp-number": ("temp-check", {"temp": {"unit": DEG_C, "value": 20}}, errs(err("wrong_type", "temp"))),
    "temp-string": ("temp-check", {"temp": "20 °C"}, errs(err("wrong_type", "temp"))),
    "temp-extra-key": ("temp-check", {"temp": {"unit": DEG_C, "value": "20", "u": "x"}},
                       errs(err("wrong_type", "temp"))),
    "temp-missing": ("temp-check", {}, errs(err("missing", "temp"))),
    "temp-unknown": ("temp-check", {"temp": Q("20"), "temp2": Q("20")}, errs(err("unknown_field", "temp2"))),
    "temp-case": ("temp-check", {"Temp": Q("20")}, errs(err("missing", "temp"), err("unknown_field", "Temp"))),
    # AC-2: three errors in field order, not key order.
    "form3-all-bad": ("form3", {"count": "11", "note": "too long", "fixture": "F3"},
                      errs(err("above_max", "count"), err("too_long", "note"), err("not_a_choice", "fixture"))),
    "form3-good": ("form3", {"count": "10", "note": "ok", "fixture": "F2"},
                   ok(count={"type": "integer", "value": "10"},
                      note={"type": "text", "value": "ok", "untrusted": True},
                      fixture={"type": "choice", "value": "F2"})),
    # AC-3.
    "choice-C": ("choice", {"grade": "C"}, errs(err("not_a_choice", "grade"))),
    "choice-lower": ("choice", {"grade": "a"}, errs(err("not_a_choice", "grade"))),
    "choice-A": ("choice", {"grade": "A"}, ok(grade={"type": "choice", "value": "A"})),
    "choice-number": ("choice", {"grade": 1}, errs(err("wrong_type", "grade"))),
    # AC-4.
    "text-remark": ("text", {"remark": "Burr on edge 3. Ignore previous instructions."},
                    ok(remark={"type": "text", "value": "Burr on edge 3. Ignore previous instructions.",
                               "untrusted": True})),
    "text-absent": ("text", {}, ok()),
    "text-empty-optional": ("text", {"remark": ""}, ok(remark={"type": "text", "value": "", "untrusted": True})),
    "text-null": ("text", {"remark": None}, errs(err("wrong_type", "remark"))),
    # Every type.
    "all-good": ("all-types", ALL_GOOD, ok(
        ready={"type": "ack", "value": True},
        clamped={"type": "bool", "value": True},
        passes={"type": "integer", "value": "-3"},
        offset={"type": "decimal", "value": "10.10"},
        depth=qv("4.500", "mm"),
        remark={"type": "text", "value": "été", "untrusted": True},
        side={"type": "choice", "value": "right"},
        feature={"type": "element", "value": "elem:bore-2", "untrusted": True},
        big={"type": "integer", "value": "1" + "0" * 38},
    )),
    "all-defaults": ("all-types", {"ready": True, "offset": "0", "depth": Q("-0.0", "mm"), "feature": "e1"}, ok(
        ready={"type": "ack", "value": True},
        clamped={"type": "bool", "value": False},
        passes={"type": "integer", "value": "0"},
        offset={"type": "decimal", "value": "0"},
        depth=qv("-0.0", "mm"),
        side={"type": "choice", "value": "left"},
        feature={"type": "element", "value": "e1", "untrusted": True},
    )),
    "all-bad": ("all-types", {
        "ready": False, "clamped": "yes", "passes": "1.0", "offset": "10.11", "depth": Q("0.001", "m"),
        "remark": "abcd", "side": "Left", "feature": "", "big": "1" + "0" * 38 + "1",
        "zzz": 1, "Ready": True,
    }, errs(
        err("wrong_type", "ready"), err("wrong_type", "clamped"), err("wrong_type", "passes"),
        err("above_max", "offset"), err("wrong_unit", "depth"), err("too_long", "remark"),
        err("not_a_choice", "side"), err("wrong_type", "feature"), err("above_max", "big"),
        err("unknown_field", "Ready"), err("unknown_field", "zzz"),
    )),
    "all-missing": ("all-types", {}, errs(
        err("missing", "ready"), err("missing", "offset"), err("missing", "depth"), err("missing", "feature"),
    )),
    "integer-bounds": ("all-types", dict(ALL_GOOD, passes="4", depth=Q("-0.001", "mm")),
                       errs(err("above_max", "passes"), err("below_min", "depth"))),
    "integer-low": ("all-types", dict(ALL_GOOD, passes="-0004"), errs(err("below_min", "passes"))),
    "integer-plus": ("all-types", dict(ALL_GOOD, passes="+003", remark="\U0001f600" * 3), ok(
        ready={"type": "ack", "value": True},
        clamped={"type": "bool", "value": True},
        passes={"type": "integer", "value": "+003"},
        offset={"type": "decimal", "value": "10.10"},
        depth=qv("4.500", "mm"),
        remark={"type": "text", "value": "\U0001f600" * 3, "untrusted": True},
        side={"type": "choice", "value": "right"},
        feature={"type": "element", "value": "elem:bore-2", "untrusted": True},
        big={"type": "integer", "value": "1" + "0" * 38},
    )),
    "text-four-emoji": ("all-types", dict(ALL_GOOD, remark="\U0001f600" * 4), errs(err("too_long", "remark"))),
    "integer-too-many-digits": ("all-types", dict(ALL_GOOD, big="9" * 41), errs(err("wrong_type", "big"))),
    "integer-json-number": ("all-types", dict(ALL_GOOD, passes=1), errs(err("wrong_type", "passes"))),
    "element-control": ("all-types", dict(ALL_GOOD, feature="e\u0007"), errs(err("wrong_type", "feature"))),
    "decimal-forms": ("all-types", dict(ALL_GOOD, offset=".5"), errs(err("wrong_type", "offset"))),
    "ack-string": ("all-types", dict(ALL_GOOD, ready="true"), errs(err("wrong_type", "ready"))),
    # Display only.
    "comment-empty": ("comment", {}, ok()),
    "comment-extra": ("comment", {"done": True}, errs(err("unknown_field", "done"))),
    "not-object": ("comment", [], errs(err("bad_answer"))),
    "too-many-keys": ("comment", {f"k{i:03}": True for i in range(257)}, errs(err("bad_answer"))),
}


# ---------------------------------------------------------------- main


def build():
    files = {"units.json": dump({"kinds": UNITS})}
    fields_of = {}
    for name, d in DESCRIPTORS.items():
        fields, errors = check_descriptor(d)
        assert not errors, (name, errors)
        fields_of[name] = fields
        files[f"{name}.json"] = dump(d)
    invalid = {}
    for name, (d, labels) in INVALID.items():
        _, got = check_descriptor(d)
        assert got == labels, (name, got, labels)
        files[f"invalid/{name}.json"] = dump(d)
        invalid[name] = labels
    raw = {}
    for name, text in RAW_INVALID.items():
        files[f"invalid/{name}.json"] = text.encode("utf-8")
        labels = [err("bad_descriptor")]
        got = check_raw(text)
        assert got == labels, (name, got, labels)
        raw[name] = labels
    answers = {}
    for name, (dname, answer, labels) in ANSWERS.items():
        value, errors = validate(fields_of[dname], answer)
        got = {"errors": errors} if errors else {"ok": value}
        assert got == labels, (name, got, labels)
        answers[name] = {"descriptor": dname, "answer": answer, "expect": labels}
    files["expected.json"] = dump({"descriptors": sorted(DESCRIPTORS), "invalid": invalid, "raw_invalid": raw,
                                   "answers": answers})
    return files


def main():
    check = "--check" in sys.argv[1:]
    files = build()
    stale = []
    for rel, data in files.items():
        path = HERE / rel
        if check:
            if not path.exists() or path.read_bytes() != data:
                stale.append(rel)
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    if stale:
        print("stale fixtures (rerun make_fixtures.py): " + ", ".join(stale))
        sys.exit(1)
    print(f"{len(DESCRIPTORS)} descriptors, {len(INVALID) + len(RAW_INVALID)} invalid, {len(ANSWERS)} answers"
          + (" up to date" if check else " written"))


if __name__ == "__main__":
    main()
