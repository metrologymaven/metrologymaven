#!/usr/bin/env python3
"""Generate fixtures/core/bus/pipe/valid/{registry,state,real}.json and params.jsonl
(CORE.BUS.PIPE.VALID).

registry.json is a synthetic test registry: commands with active, deprecated, hidden and
removed majors, and parameter schemas that between them use every assertion and
applicator keyword the bus validates, the formats it asserts, and the project keywords
x-quantity, x-ref and x-selector. state.json lists the entities of the test state view.
real.json holds the parameter and result schemas of five real commands (the four request
commands of CORE.BUS.POL.PROJ and registry.show_manifest@1), transcribed from their
definitions (the Rust test checks the copies against the live definitions), with real
parameter and result documents and their expected outcomes.

params.jsonl holds one parameter document per line with its expected outcome. The
expected outcomes are computed here by an independent reference validator written for
this file with the Python standard library only (json, re, fractions, decimal,
ipaddress, datetime). It follows the rules pinned in spec/core/bus/pipe/valid/NODE.md
(Design) and in the module docs of crates/fid-bus/src/validate/schema.rs: the first
failure by instance pointer in document order (RFC 8785 member order, UTF-16 code
units), ties broken by keyword rank, then by evaluation order; literals normalised per
spec/core/bus/unit/NODE.md with exact rationals (fractions.Fraction, correctly rounded
by int/int true division). Nothing here runs or reads the Rust code. The quantity
catalogue is fixtures/core/bus/unit/catalogue.json. CC-BY-4.0 project data.

Run:    python fixtures/core/bus/pipe/valid/make_cases.py
Check:  python fixtures/core/bus/pipe/valid/make_cases.py --check
"""

import datetime
import decimal
import ipaddress
import json
import re
import struct
import sys
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[4]
CATALOGUE = json.loads((ROOT / "fixtures/core/bus/unit/catalogue.json").read_text(encoding="utf-8"))
PI = Fraction(int(CATALOGUE["pi_hex"], 16), 2**254)

# ------------------------------------------------------------------ the test registry


def command(cid, major, params, state="active", preconditions=(), extra_life=None):
    life = {"state": state, "since": "2026-01-01"}
    life.update(extra_life or {})
    return {
        "id": cid,
        "major": major,
        "title": "Test command",
        "description": "Synthetic command of the CORE.BUS.PIPE.VALID test registry.",
        "params_schema": params,
        "result_schema": {"type": "object"},
        "side_effect": "W2",
        "mcp_projection": "exposed",
        "determinism": "D0",
        "idempotency": "none",
        "fs_effect": "none",
        "undo": "head_move",
        "preconditions": list(preconditions),
        "effects": ["TestDone"],
        "algorithms": [],
        "exposure": ["cli", "mcp"],
        "lifecycle": life,
        "human_only": False,
        "network": "none",
        "web": "available",
        "cli_bare": False,
    }


SMALL = {"type": "object", "additionalProperties": False, "properties": {"n": {"type": "integer"}}}

SHAPES = {
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "additionalProperties": False,
    "required": ["count", "name"],
    "properties": {
        "name": {"type": "string", "minLength": 1, "maxLength": 8, "pattern": "^[a-z][a-z0-9_]*$"},
        "count": {"type": "integer", "minimum": 1, "maximum": 10},
        "step": {"type": "number", "x-quantity": "length", "exclusiveMinimum": 0, "multipleOf": 0.25},
        "mode": {"enum": ["fast", "slow", None]},
        "version": {"const": 2},
        "tags": {"type": "array", "items": {"type": "string", "maxLength": 4}, "maxItems": 3, "uniqueItems": True},
        "when": {"type": "string", "format": "date-time"},
        "day": {"type": "string", "format": "date"},
        "uid": {"type": "string", "format": "uuid"},
        "host": {"type": "string", "format": "hostname"},
        "ip4": {"type": "string", "format": "ipv4"},
        "ip6": {"type": "string", "format": "ipv6"},
        "span": {"type": "string", "format": "duration"},
        "flag": {"type": "boolean"},
        "note": {"type": ["string", "null"], "x-untrusted": True},
    },
}

COMBO = {
    "$defs": {"pos": {"type": "integer", "minimum": 0}, "label": {"type": "string", "pattern": "^L[0-9]+$"}},
    "type": "object",
    "properties": {
        "kind": {"enum": ["a", "b"]},
        "a_val": {"$ref": "#/$defs/pos"},
        "pair": {"type": "array", "prefixItems": [{"$ref": "#/$defs/label"}, {"type": "integer"}], "items": False},
        "list": {
            "type": "array",
            "contains": {"const": "x"},
            "minContains": 1,
            "maxContains": 2,
            "unevaluatedItems": {"type": "string"},
        },
        "pick": {"oneOf": [{"type": "integer", "multipleOf": 2}, {"type": "integer", "multipleOf": 3}]},
        "any": {"anyOf": [{"type": "string"}, {"type": "integer", "minimum": 100}]},
        "nope": {"not": {"type": "string"}},
        "both": {"allOf": [{"type": "integer"}, {"maximum": 5}]},
    },
    "patternProperties": {"^x_[a-z]+$": {"type": "integer"}},
    "propertyNames": {"maxLength": 12},
    "dependentRequired": {"kind": ["a_val"]},
    "dependentSchemas": {"pair": {"required": ["kind"]}},
    "if": {"properties": {"kind": {"const": "b"}}, "required": ["kind"]},
    "then": {"required": ["any"]},
    "else": {"properties": {"any": {"type": "string"}}},
    "unevaluatedProperties": False,
}

MEASURE = {
    "type": "object",
    "additionalProperties": False,
    "required": ["length"],
    "properties": {
        "length": {"type": "number", "x-quantity": "length", "x-unit": "mm", "minimum": 0},
        "angle": {"type": "number", "x-quantity": "angle"},
        "offset": {"type": "array", "items": {"type": "number", "x-quantity": "length"}, "maxItems": 3},
        "feature": {"type": "string", "x-ref": "feature"},
        "datums": {"type": "array", "x-ref": "datum", "x-selector": "list", "maxItems": 3},
        "frame": {"x-selector": "single"},
    },
}

GUARDED = {
    "type": "object",
    "required": ["items", "limit"],
    "properties": {"limit": {"type": "integer"}, "items": {"type": "array", "items": {"type": "integer"}}},
}

REGISTRY = {
    "commands": [
        command("t.shapes", 1, SHAPES),
        command("t.combo", 1, COMBO),
        command("t.measure", 1, MEASURE),
        command(
            "t.guarded",
            1,
            GUARDED,
            preconditions=[
                {"expr": "params.limit > 0", "bound": 10, "message_key": "t.guarded.limit_positive"},
                {"expr": "all(x in params.items, x >= 0)", "bound": 20, "message_key": "t.guarded.items_nonnegative"},
            ],
        ),
        command("t.old", 1, SMALL, "deprecated", extra_life={"replaced_by": "t.old@2", "remove_after": "2027-06-01"}),
        command("t.old", 2, SMALL),
        command("t.hidden", 1, SMALL, "hidden"),
        command("t.gone", 1, SMALL, "removed"),
        command("t.gone", 2, SMALL),
        command("t.free", 1, {"type": "object"}),
    ]
}

STATE = {
    "entities": [
        {"id": "F1", "kind": "feature.plane"},
        {"id": "F2", "kind": "feature"},
        {"id": "D1", "kind": "datum"},
        {"id": "D2", "kind": "datum.target"},
        {"id": "FX", "kind": "featurex"},
    ]
}

# ------------------------------------------------------------------ the reference validator

RANK = [
    "json", "input_limit", "false", "x-quantity", "type", "const", "enum", "multipleOf", "maximum",
    "exclusiveMaximum", "minimum", "exclusiveMinimum", "maxLength", "minLength", "pattern", "format", "x-ref",
    "x-selector", "maxItems", "minItems", "uniqueItems", "contains", "minContains", "maxContains", "maxProperties",
    "minProperties", "required", "dependentRequired", "$ref", "allOf", "anyOf", "oneOf", "not", "if", "then", "else",
    "dependentSchemas", "propertyNames", "prefixItems", "items", "properties", "patternProperties",
    "additionalProperties", "unevaluatedItems", "unevaluatedProperties", "schema",
]


class Fail:
    def __init__(self, ptr, keyword, reason="schema"):
        self.ptr = tuple(ptr)
        self.keyword = keyword
        self.reason = reason


def utf16(s):
    return s.encode("utf-16-be")


def ptr_cmp(a, b):
    for x, y in zip(a, b):
        if isinstance(x, int) and isinstance(y, int):
            c = (x > y) - (x < y)
        else:
            c = (utf16(x) > utf16(y)) - (utf16(x) < utf16(y))
        if c:
            return c
    return (len(a) > len(b)) - (len(a) < len(b))


def before(f, g):
    c = ptr_cmp(f.ptr, g.ptr)
    if c:
        return c < 0
    return RANK.index(f.keyword) < RANK.index(g.keyword)


class Best:
    def __init__(self):
        self.f = None

    def note(self, f):
        if self.f is None or before(f, self.f):
            self.f = f


class Out:
    def __init__(self):
        self.props = set()
        self.items = set()
        self.all_items = False
        self.lits = []

    def merge(self, o):
        self.props |= o.props
        self.items |= o.items
        self.all_items = self.all_items or o.all_items
        self.lits += o.lits


def kind_of(v):
    if v is None:
        return "null"
    if isinstance(v, bool):
        return "boolean"
    if isinstance(v, (int, float)):
        return "number"
    if isinstance(v, str):
        return "string"
    if isinstance(v, list):
        return "array"
    return "object"


def type_ok(t, v):
    k = kind_of(v)
    if t == "integer":
        return k == "number" and float(v).is_integer()
    return t == k


def same(a, b):
    """JSON equality: numbers by value, booleans never equal numbers."""
    if kind_of(a) != kind_of(b):
        return False
    if isinstance(a, dict):
        return a.keys() == b.keys() and all(same(a[k], b[k]) for k in a)
    if isinstance(a, list):
        return len(a) == len(b) and all(same(x, y) for x, y in zip(a, b))
    return a == b


def multiple(x, m):
    decimal.getcontext().prec = 2000
    return decimal.Decimal(repr(x)) % decimal.Decimal(repr(m)) == 0


def ecma(pattern):
    """The few ECMA-262 meanings the fixture patterns need, in Python `re` syntax:
    `$` is the end of the input only, `\\d` is ASCII."""
    out, i = [], 0
    while i < len(pattern):
        c = pattern[i]
        if c == "\\":
            nxt = pattern[i + 1]
            out.append("[0-9]" if nxt == "d" else "\\" + nxt)
            i += 2
            continue
        out.append(r"\Z" if c == "$" else c)
        i += 1
    return "".join(out)


def fmt_ok(name, s):
    a = re.ASCII
    if name == "date":
        m = re.fullmatch(r"(\d{4})-(\d{2})-(\d{2})", s, a)
        if not m:
            return False
        try:
            datetime.date(*map(int, m.groups()))
            return True
        except ValueError:
            return False
    if name in ("time", "date-time"):
        if name == "date-time":
            if len(s) < 11 or s[10] not in "Tt" or not fmt_ok("date", s[:10]):
                return False
            s = s[11:]
        m = re.fullmatch(r"(\d{2}):(\d{2}):(\d{2})(\.\d+)?([Zz]|[+-](\d{2}):(\d{2}))", s, a)
        if not m:
            return False
        h, mi, sec = int(m[1]), int(m[2]), int(m[3])
        if h > 23 or mi > 59 or sec > 60:
            return False
        off = 0
        if m[6] is not None:
            oh, om = int(m[6]), int(m[7])
            if oh > 23 or om > 59:
                return False
            off = (oh * 60 + om) * (-1 if m[5][0] == "-" else 1)
        return sec < 60 or (h * 60 + mi - off) % 1440 == 23 * 60 + 59
    if name == "duration":
        t = r"T(\d+H(\d+M(\d+S)?)?|\d+M(\d+S)?|\d+S)"
        d = r"(\d+D|\d+M(\d+D)?|\d+Y(\d+M(\d+D)?)?)"
        return re.fullmatch(rf"P({d}({t})?|{t}|\d+W)", s, a) is not None
    if name == "uuid":
        return re.fullmatch(r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}", s) is not None
    if name == "ipv4":
        parts = s.split(".")
        return len(parts) == 4 and all(
            re.fullmatch(r"0|[1-9][0-9]{0,2}", p, a) is not None and int(p) <= 255 for p in parts
        )
    if name == "ipv6":
        if not s.isascii() or "%" in s or "/" in s:
            return False
        try:
            ipaddress.IPv6Address(s)
            return True
        except ValueError:
            return False
    if name == "hostname":
        labels = s.split(".")
        return 0 < len(s) <= 253 and all(
            re.fullmatch(r"[A-Za-z0-9]([A-Za-z0-9-]{0,61}[A-Za-z0-9])?", lab, a) for lab in labels
        )
    raise ValueError("format not used by the fixtures: " + name)


def catalogue_unit(kind, unit):
    for k in CATALOGUE["kinds"]:
        if k["kind"] == kind:
            for u in k["units"]:
                if u["symbol"] == unit:
                    f = Fraction(int(u["num"]), int(u["den"]))
                    return f * PI if u["pi"] else f
    return None


def literal(kind, obj):
    """CORE.BUS.UNIT: (si float, value text, unit) or the refusal reason."""
    if set(obj) != {"value", "unit"} or not isinstance(obj["value"], str) or not isinstance(obj["unit"], str):
        return "literal_syntax"
    text, unit = obj["value"], obj["unit"]
    if len(text.encode("utf-8")) > 1024:
        return "literal_out_of_range"
    m = re.fullmatch(r"([+-]?)([0-9]+)(?:\.([0-9]+))?(?:[eE]([+-]?[0-9]+))?", text, re.ASCII)
    if not m:
        return "literal_syntax"
    digits = (m[2] + (m[3] or "")).lstrip("0")
    if len(digits) > 64:
        return "literal_out_of_range"
    written = int(m[4] or "0")
    exp10 = written - len(m[3] or "")
    lead = written if not digits else exp10 + len(digits) - 1
    if not -400 <= lead <= 400:
        return "literal_out_of_range"
    if not (1 <= len(unit) <= 16 and all(0x21 <= ord(c) <= 0x7E for c in unit)):
        return "unit_mismatch"
    factor = catalogue_unit(kind, unit)
    if factor is None:
        return "unit_mismatch"
    exact = Fraction(int(digits or "0")) * Fraction(10) ** exp10 * factor
    try:
        si = exact.numerator / exact.denominator
    except OverflowError:
        return "literal_out_of_range"
    if exact != 0 and si == 0.0:
        return "literal_out_of_range"
    if m[1] == "-":
        si = -si
    return (si, text, unit)


ENTITIES = {e["id"]: e["kind"] for e in STATE["entities"]}


def entity_check(eid, want, ptr, kw):
    if not (0 < len(eid.encode("utf-8")) <= 256) or any(ord(c) < 0x20 or 0x7F <= ord(c) <= 0x9F for c in eid):
        return Fail(ptr, kw, "bad_reference")
    if eid not in ENTITIES:
        return Fail(ptr, kw, "bad_reference")
    k = ENTITIES[eid]
    if want is not None and not (k == want or k.startswith(want + ".")):
        return Fail(ptr, kw, "bad_reference")
    return None


class Validator:
    def __init__(self, root):
        self.root = root

    def resolve(self, ref):
        node = self.root
        if ref == "#":
            return node
        for tok in ref[2:].split("/"):
            tok = tok.replace("~1", "/").replace("~0", "~")
            node = node[int(tok)] if isinstance(node, list) else node[tok]
        return node

    def node(self, s, inst, ptr, via):
        if s is True:
            return Out()
        if s is False:
            return Fail(ptr, via)
        return self.obj(s, inst, ptr)

    def obj(self, o, inst, ptr):
        out, best = Out(), Best()
        if "x-quantity" in o and isinstance(inst, dict):
            r = literal(o["x-quantity"], inst)
            if isinstance(r, str):
                return Fail(ptr, "x-quantity", r)
            inst = r[0]
            out.lits.append((tuple(ptr), r))
        self.assertions(o, inst, ptr, best)
        self.applicators(o, inst, ptr, out, best)
        return best.f if best.f else out

    def assertions(self, o, inst, ptr, best):
        if "type" in o:
            ts = o["type"] if isinstance(o["type"], list) else [o["type"]]
            if not any(type_ok(t, inst) for t in ts):
                best.note(Fail(ptr, "type"))
        if "const" in o and not same(o["const"], inst):
            best.note(Fail(ptr, "const"))
        if "enum" in o and not any(same(e, inst) for e in o["enum"]):
            best.note(Fail(ptr, "enum"))
        if kind_of(inst) == "number":
            if "multipleOf" in o and not multiple(inst, o["multipleOf"]):
                best.note(Fail(ptr, "multipleOf"))
            for kw, ok in (
                ("maximum", lambda x, l: x <= l),
                ("exclusiveMaximum", lambda x, l: x < l),
                ("minimum", lambda x, l: x >= l),
                ("exclusiveMinimum", lambda x, l: x > l),
            ):
                if kw in o and not ok(inst, o[kw]):
                    best.note(Fail(ptr, kw))
        if isinstance(inst, str):
            if "maxLength" in o and len(inst) > o["maxLength"]:
                best.note(Fail(ptr, "maxLength"))
            if "minLength" in o and len(inst) < o["minLength"]:
                best.note(Fail(ptr, "minLength"))
            if "pattern" in o and not re.search(ecma(o["pattern"]), inst):
                best.note(Fail(ptr, "pattern"))
            if "format" in o and not fmt_ok(o["format"], inst):
                best.note(Fail(ptr, "format"))
        if "x-ref" in o or "x-selector" in o:
            f = self.refs(o, inst, ptr)
            if f:
                best.note(f)
        if isinstance(inst, list):
            if "maxItems" in o and len(inst) > o["maxItems"]:
                best.note(Fail(ptr, "maxItems"))
            if "minItems" in o and len(inst) < o["minItems"]:
                best.note(Fail(ptr, "minItems"))
            if o.get("uniqueItems") and any(same(a, b) for i, a in enumerate(inst) for b in inst[i + 1 :]):
                best.note(Fail(ptr, "uniqueItems"))
        if isinstance(inst, dict):
            if "maxProperties" in o and len(inst) > o["maxProperties"]:
                best.note(Fail(ptr, "maxProperties"))
            if "minProperties" in o and len(inst) < o["minProperties"]:
                best.note(Fail(ptr, "minProperties"))
            if any(r not in inst for r in o.get("required", [])):
                best.note(Fail(ptr, "required"))
            dep = o.get("dependentRequired", {})
            if any(k in inst and any(r not in inst for r in rs) for k, rs in dep.items()):
                best.note(Fail(ptr, "dependentRequired"))

    def refs(self, o, inst, ptr):
        kw = "x-ref" if "x-ref" in o else "x-selector"
        want = o.get("x-ref")
        if o.get("x-selector", "single") == "single":
            if not isinstance(inst, str):
                return Fail(ptr, kw)
            return entity_check(inst, want, ptr, kw)
        if not isinstance(inst, list):
            return Fail(ptr, "x-selector")
        for i, item in enumerate(inst):
            at = ptr + (i,)
            f = Fail(at, kw) if not isinstance(item, str) else entity_check(item, want, at, kw)
            if f:
                return f
        return None

    def same_level(self, s, inst, ptr, via, out, best):
        r = self.node(s, inst, ptr, via)
        if isinstance(r, Fail):
            best.note(r)
            return False
        out.merge(r)
        return True

    def child(self, s, inst, ptr, via, out):
        r = self.node(s, inst, ptr, via)
        if isinstance(r, Fail):
            return r
        out.lits += r.lits
        return None

    def applicators(self, o, inst, ptr, out, best):
        if isinstance(inst, list) and "contains" in o:
            matched = 0
            for i, item in enumerate(inst):
                if not isinstance(self.node(o["contains"], item, ptr + (i,), "contains"), Fail):
                    matched += 1
                    out.items.add(i)
            if matched < o.get("minContains", 1):
                best.note(Fail(ptr, "minContains" if "minContains" in o else "contains"))
            if "maxContains" in o and matched > o["maxContains"]:
                best.note(Fail(ptr, "maxContains"))
        if "$ref" in o:
            self.same_level(self.resolve(o["$ref"]), inst, ptr, "$ref", out, best)
        for s in o.get("allOf", []):
            self.same_level(s, inst, ptr, "allOf", out, best)
        if "anyOf" in o:
            passed = [r for r in (self.node(s, inst, ptr, "anyOf") for s in o["anyOf"]) if not isinstance(r, Fail)]
            for r in passed:
                out.merge(r)
            if not passed:
                best.note(Fail(ptr, "anyOf"))
        if "oneOf" in o:
            passed = [r for r in (self.node(s, inst, ptr, "oneOf") for s in o["oneOf"]) if not isinstance(r, Fail)]
            if len(passed) == 1:
                out.merge(passed[0])
            else:
                best.note(Fail(ptr, "oneOf"))
        if "not" in o and not isinstance(self.node(o["not"], inst, ptr, "not"), Fail):
            best.note(Fail(ptr, "not"))
        if "if" in o:
            r = self.node(o["if"], inst, ptr, "if")
            if not isinstance(r, Fail):
                out.merge(r)
                if "then" in o:
                    self.same_level(o["then"], inst, ptr, "then", out, best)
            elif "else" in o:
                self.same_level(o["else"], inst, ptr, "else", out, best)
        if isinstance(inst, dict):
            for name in sorted(o.get("dependentSchemas", {})):
                if name in inst:
                    self.same_level(o["dependentSchemas"][name], inst, ptr, "dependentSchemas", out, best)
        if isinstance(inst, list):
            self.array_children(o, inst, ptr, out, best)
        if isinstance(inst, dict):
            self.object_children(o, inst, ptr, out, best)

    def array_children(self, o, a, ptr, out, best):
        prefix = o.get("prefixItems", [])
        for i, (s, item) in enumerate(zip(prefix, a)):
            f = self.child(s, item, ptr + (i,), "prefixItems", out)
            if f:
                best.note(f)
                break
            out.items.add(i)
        if "items" in o:
            for i in range(len(prefix), len(a)):
                f = self.child(o["items"], a[i], ptr + (i,), "items", out)
                if f:
                    best.note(f)
                    break
            out.all_items = True
        if "unevaluatedItems" in o and best.f is None:
            for i, item in enumerate(a):
                if out.all_items or i in out.items:
                    continue
                f = self.child(o["unevaluatedItems"], item, ptr + (i,), "unevaluatedItems", out)
                if f:
                    best.note(f)
                    break
            out.all_items = True

    def object_children(self, o, m, ptr, out, best):
        members = sorted(m, key=utf16)
        props = o.get("properties", {})
        pats = [(re.compile(ecma(p)), s) for p, s in sorted(o.get("patternProperties", {}).items())]
        if "propertyNames" in o:
            for name in members:
                if isinstance(self.node(o["propertyNames"], name, ptr + (name,), "propertyNames"), Fail):
                    best.note(Fail(ptr + (name,), "propertyNames"))
                    break
        for name in members:
            if name in props:
                f = self.child(props[name], m[name], ptr + (name,), "properties", out)
                if f:
                    best.note(f)
                    break
                out.props.add(name)
        for name in members:
            first = Best()
            for rx, s in pats:
                if rx.search(name):
                    f = self.child(s, m[name], ptr + (name,), "patternProperties", out)
                    if f:
                        first.note(f)
                    else:
                        out.props.add(name)
            if first.f:
                best.note(first.f)
                break
        if "additionalProperties" in o:
            for name in members:
                if name in props or any(rx.search(name) for rx, _ in pats):
                    continue
                f = self.child(o["additionalProperties"], m[name], ptr + (name,), "additionalProperties", out)
                if f:
                    best.note(f)
                    break
                out.props.add(name)
        if "unevaluatedProperties" in o and best.f is None:
            for name in members:
                if name in out.props:
                    continue
                f = self.child(o["unevaluatedProperties"], m[name], ptr + (name,), "unevaluatedProperties", out)
                if f:
                    best.note(f)
                    break
                out.props.add(name)


def show(ptr):
    return "".join("/" + (str(t) if isinstance(t, int) else t.replace("~", "~0").replace("/", "~1")) for t in ptr)


def bits(x):
    return "0x" + struct.pack(">d", x).hex()


def put(doc, ptr, value):
    if not ptr:
        return value
    cur = doc
    for t in ptr[:-1]:
        cur = cur[t]
    cur[ptr[-1]] = value
    return doc


def expected(cmd, principal, params):
    name, _, major = cmd.rpartition("@")
    major = int(major)
    defs = [c for c in REGISTRY["commands"] if c["id"] == name]
    if not re.fullmatch(r"[a-z0-9]([a-z0-9_.]*[a-z0-9])?", name) or not defs:
        return {"outcome": "error", "code": "not_found", "reason": "unknown_command"}
    found = [c for c in defs if c["major"] == major]
    if not found:
        return {"outcome": "error", "code": "not_found", "reason": "unknown_major"}
    d = found[0]
    state = d["lifecycle"]["state"]
    if state == "removed":
        return {"outcome": "error", "code": "not_found", "reason": "removed_major"}
    if state == "hidden" and principal != "replay":
        return {"outcome": "error", "code": "hidden_command", "reason": "hidden_major"}
    v = Validator(d["params_schema"])
    r = v.node(d["params_schema"], params, (), "false")
    if isinstance(r, Fail):
        return {"outcome": "error", "code": "validation", "reason": r.reason, "pointer": show(r.ptr), "keyword": r.keyword}
    lits = sorted({lp: lr for lp, lr in r.lits}.items(), key=lambda kv: [(0, t) if isinstance(t, int) else (1, utf16(t)) for t in kv[0]])
    doc = json.loads(json.dumps(params))
    records = []
    for p, (si, text, unit) in lits:
        doc = put(doc, list(p), si)
        records.append({"pointer": show(p), "si_bits": bits(si), "unit": unit, "value": text})
    return {"outcome": "ok", "params": doc, "literals": records}


# ------------------------------------------------------------------ the cases


def L(value, unit):
    return {"value": value, "unit": unit}


H, A, R, S = "human", "agent", "replay", "recompute"
CASES = [
    # Resolution (AC-1, AC-2).
    ("unknown_command", "nope.cmd@1", H, {}),
    ("removed_major", "t.gone@1", H, {"n": 1}),
    ("unknown_major", "t.shapes@9", H, {"name": "ab", "count": 1}),
    ("not_a_command_name", "T.Shapes@1", H, {}),
    ("hidden_human", "t.hidden@1", H, {"n": 1}),
    ("hidden_agent", "t.hidden@1", A, {"n": 1}),
    ("hidden_other_system", "t.hidden@1", S, {"n": 1}),
    ("hidden_replay_ok", "t.hidden@1", R, {"n": 1}),
    ("hidden_replay_validates", "t.hidden@1", R, {"n": "x"}),
    ("deprecated_resolves", "t.old@1", H, {"n": 1}),
    ("successor_active", "t.old@2", A, {"n": 2}),
    ("other_major_active", "t.gone@2", H, {}),
    # Assertions (t.shapes).
    ("shapes_minimal", "t.shapes@1", H, {"name": "ab", "count": 3}),
    (
        "shapes_full",
        "t.shapes@1",
        H,
        {
            "name": "probe_1", "count": 10, "step": 0.75, "mode": None, "version": 2.0, "tags": ["a", "bb"],
            "when": "2026-11-20T10:00:00.5+01:00", "day": "2024-02-29", "uid": "2eb8aa08-aa98-11ea-b4aa-73b441d16380",
            "host": "cmm-01.lab", "ip4": "10.0.0.255", "ip6": "fe80::1:2", "span": "P1Y2M3DT4H", "flag": False,
            "note": "free text",
        },
    ),
    ("missing_required", "t.shapes@1", H, {"name": "ab"}),
    ("additional_property", "t.shapes@1", H, {"name": "ab", "count": 1, "zzz": 1}),
    ("min_length", "t.shapes@1", H, {"name": "", "count": 1}),
    ("max_length", "t.shapes@1", H, {"name": "abcdefghi", "count": 1}),
    ("max_length_counts_code_points", "t.shapes@1", H, {"name": "a", "count": 1, "tags": ["éééé"]}),
    ("pattern", "t.shapes@1", H, {"name": "Ab", "count": 1}),
    ("pattern_end_of_input", "t.shapes@1", H, {"name": "ab\n", "count": 1}),
    ("string_type", "t.shapes@1", H, {"name": 5, "count": 1}),
    ("integer_type", "t.shapes@1", H, {"name": "ab", "count": 2.5}),
    ("integer_written_as_float", "t.shapes@1", H, {"name": "ab", "count": 3.0}),
    ("maximum", "t.shapes@1", H, {"name": "ab", "count": 11}),
    ("minimum", "t.shapes@1", H, {"name": "ab", "count": 0}),
    ("type_before_maximum", "t.shapes@1", H, {"name": "ab", "count": 20.5}),
    ("exclusive_minimum", "t.shapes@1", H, {"name": "ab", "count": 1, "step": 0}),
    ("multiple_of", "t.shapes@1", H, {"name": "ab", "count": 1, "step": 0.3}),
    ("multiple_of_literal", "t.shapes@1", H, {"name": "ab", "count": 1, "step": L("300", "mm")}),
    ("multiple_of_literal_ok", "t.shapes@1", H, {"name": "ab", "count": 1, "step": L("0.75", "m")}),
    ("enum", "t.shapes@1", H, {"name": "ab", "count": 1, "mode": "medium"}),
    ("const", "t.shapes@1", H, {"name": "ab", "count": 1, "version": 3}),
    ("const_bool_is_not_number", "t.shapes@1", H, {"name": "ab", "count": 1, "version": True}),
    ("unique_items", "t.shapes@1", H, {"name": "ab", "count": 1, "tags": ["a", "a"]}),
    ("max_items", "t.shapes@1", H, {"name": "ab", "count": 1, "tags": ["a", "b", "c", "d"]}),
    ("items_failure_pointer", "t.shapes@1", H, {"name": "ab", "count": 1, "tags": ["a", "abcde"]}),
    ("format_date_time", "t.shapes@1", H, {"name": "ab", "count": 1, "when": "2026-11-20"}),
    ("format_leap_second", "t.shapes@1", H, {"name": "ab", "count": 1, "when": "2026-06-30T23:59:60Z"}),
    ("format_bad_leap_second", "t.shapes@1", H, {"name": "ab", "count": 1, "when": "2026-06-30T22:59:60Z"}),
    ("format_date", "t.shapes@1", H, {"name": "ab", "count": 1, "day": "2026-02-29"}),
    ("format_uuid", "t.shapes@1", H, {"name": "ab", "count": 1, "uid": "2eb8aa08aa9811eab4aa73b441d16380"}),
    ("format_hostname", "t.shapes@1", H, {"name": "ab", "count": 1, "host": "-cmm.lab"}),
    ("format_ipv4", "t.shapes@1", H, {"name": "ab", "count": 1, "ip4": "1.2.3.04"}),
    ("format_ipv6", "t.shapes@1", H, {"name": "ab", "count": 1, "ip6": "1:::2"}),
    ("format_duration", "t.shapes@1", H, {"name": "ab", "count": 1, "span": "P1W2D"}),
    ("type_array", "t.shapes@1", H, {"name": "ab", "count": 1, "note": 5}),
    ("boolean_type", "t.shapes@1", H, {"name": "ab", "count": 1, "flag": "yes"}),
    ("root_type", "t.shapes@1", H, []),
    ("document_order_first", "t.shapes@1", H, {"count": "x", "name": 5, "zzz": 1}),
    ("required_before_children", "t.shapes@1", H, {"name": 5}),
    ("utf16_member_order", "t.shapes@1", H, {"name": "ab", "count": 1, "｡": 1, "\U0001F600": 2}),
    # Applicators (t.combo).
    ("combo_ok", "t.combo@1", H, {"kind": "a", "a_val": 1}),
    ("dependent_required", "t.combo@1", H, {"kind": "a"}),
    ("ref_minimum", "t.combo@1", H, {"a_val": -1}),
    ("dependent_schemas", "t.combo@1", H, {"pair": ["L1", 2]}),
    ("prefix_items_ref_pattern", "t.combo@1", H, {"kind": "a", "a_val": 0, "pair": ["X1", 2]}),
    ("items_false", "t.combo@1", H, {"kind": "a", "a_val": 0, "pair": ["L1", 2, 3]}),
    ("contains_ok", "t.combo@1", H, {"list": ["x", "y"]}),
    ("min_contains", "t.combo@1", H, {"list": ["y"]}),
    ("max_contains", "t.combo@1", H, {"list": ["x", "x", "x"]}),
    ("unevaluated_items", "t.combo@1", H, {"list": ["x", 5]}),
    ("one_of_two", "t.combo@1", H, {"pick": 6}),
    ("one_of_none", "t.combo@1", H, {"pick": 5}),
    ("one_of_ok", "t.combo@1", H, {"pick": 4}),
    ("any_of_none", "t.combo@1", H, {"any": 50}),
    ("any_of_ok", "t.combo@1", H, {"any": "s"}),
    ("not", "t.combo@1", H, {"nope": "s"}),
    ("not_ok", "t.combo@1", H, {"nope": 1}),
    ("all_of", "t.combo@1", H, {"both": 7}),
    ("pattern_properties_ok", "t.combo@1", H, {"x_abc": 1}),
    ("pattern_properties", "t.combo@1", H, {"x_abc": "s"}),
    ("unevaluated_properties", "t.combo@1", H, {"zz": 1}),
    ("property_names", "t.combo@1", H, {"averyverylongname": 1}),
    ("if_then", "t.combo@1", H, {"kind": "b"}),
    ("if_then_ok", "t.combo@1", H, {"kind": "b", "a_val": 0, "any": "s"}),
    ("if_else", "t.combo@1", H, {"any": 150}),
    # Project keywords (t.measure).
    ("literal_length", "t.measure@1", H, {"length": L("10.1", "mm")}),
    ("si_number", "t.measure@1", H, {"length": 0.5}),
    ("literal_unit_mismatch", "t.measure@1", H, {"length": L("5", "deg")}),
    ("literal_syntax", "t.measure@1", H, {"length": L("1.", "mm")}),
    ("literal_out_of_range", "t.measure@1", H, {"length": L("1e500", "mm")}),
    ("literal_too_many_digits", "t.measure@1", H, {"length": L("1" * 65, "mm")}),
    ("literal_shape", "t.measure@1", H, {"length": {"value": "10", "unit": "mm", "x": 1}}),
    ("literal_number_value", "t.measure@1", H, {"length": {"value": 10, "unit": "mm"}}),
    ("literal_unknown_unit", "t.measure@1", H, {"length": L("10", "furlong")}),
    ("literal_then_minimum", "t.measure@1", H, {"length": L("-1", "mm")}),
    ("literal_string_is_not_a_literal", "t.measure@1", H, {"length": "10 mm"}),
    ("literal_angle", "t.measure@1", H, {"length": 1.0, "angle": L("90", "deg")}),
    ("literal_list", "t.measure@1", H, {"length": L("0.1", "in"), "offset": [L("1", "in"), 0.002, L("3", "thou")]}),
    ("literal_list_syntax", "t.measure@1", H, {"length": 1.0, "offset": [0, L("x", "mm")]}),
    ("ref_ok", "t.measure@1", H, {"length": 1.0, "feature": "F1", "datums": ["D1", "D2"], "frame": "F2"}),
    ("ref_wrong_kind", "t.measure@1", H, {"length": 1.0, "feature": "D1"}),
    ("ref_prefix_is_not_kind", "t.measure@1", H, {"length": 1.0, "feature": "FX"}),
    ("ref_missing", "t.measure@1", H, {"length": 1.0, "feature": "F9"}),
    ("ref_bad_id", "t.measure@1", H, {"length": 1.0, "feature": "F\u0001"}),
    ("selector_list_kind", "t.measure@1", H, {"length": 1.0, "datums": ["D1", "F1"]}),
    ("selector_list_non_string", "t.measure@1", H, {"length": 1.0, "datums": ["D1", 3]}),
    ("selector_list_not_array", "t.measure@1", H, {"length": 1.0, "datums": "D1"}),
    ("selector_single_missing", "t.measure@1", H, {"length": 1.0, "frame": "R9"}),
    ("measure_required", "t.measure@1", H, {"feature": "F1"}),
]


# ------------------------------------------------------------------ real command schemas

# Transcribed from the live definitions: `fid_bus::policy::projection::command_defs()` and
# `fid_registry::manifest::command_def()`. The Rust test checks that these copies equal the
# live schemas, so a change to a real command shows up as a failing test, never as a stale
# expectation.
UUID7 = "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$"
DIGEST = "^sha256:[0-9a-f]{64}$"
CMD_ID = "^[a-z0-9]([a-z0-9_.]*[a-z0-9])?@[0-9]{1,5}$"
RID = {"type": "string", "pattern": UUID7, "description": "Request ID, a lowercase UUIDv7."}
STATES = {"enum": ["pending", "approved", "rejected", "withdrawn"]}


def obj_schema(required, properties):
    return {"type": "object", "additionalProperties": False, "required": required, "properties": properties}


def digest_schema(d):
    return {"type": "string", "pattern": DIGEST, "description": d}


def principal_schema(d):
    return {
        "type": "object",
        "additionalProperties": False,
        "required": ["id", "kind"],
        "properties": {
            "id": {"type": "string"},
            "kind": {"enum": ["human", "agent", "script", "service", "device", "system"]},
        },
        "description": d,
    }


RECORD = obj_schema(
    ["command", "filed_at_head", "params_commitment", "request", "requester", "seq", "state"],
    {
        "request": RID,
        "seq": {"type": "integer", "minimum": 1},
        "command": {"type": "string", "pattern": CMD_ID},
        "params_commitment": digest_schema("Digest of the request's canonical parameters."),
        "requester": principal_schema("Who filed the request."),
        "filed_at_head": digest_schema("Branch head when the request was filed."),
        "state": STATES,
        "resolved_by": principal_schema("Who approved, rejected or withdrew it."),
        "reason": {"type": "string", "x-untrusted": True, "description": "Rejection reason, untrusted text."},
    },
)
REASON = {
    "type": "string",
    "minLength": 1,
    "maxLength": 2000,
    "x-untrusted": True,
    "description": "Why the request is rejected; untrusted text, 1 to 2000 characters.",
}
SHOW_CHECK = (
    "The text of a manifest to verify instead of showing the one in force; at most 67108864 characters "
    "(maxLength), and refused as manifest_invalid above 67108864 bytes of UTF-8."
)
REAL = {
    "request.list_requests@1": {
        "params_schema": obj_schema([], {"state": STATES}),
        "result_schema": obj_schema(["requests"], {"requests": {"type": "array", "items": RECORD}}),
    },
    "request.withdraw_request@1": {
        "params_schema": obj_schema(["request"], {"request": RID}),
        "result_schema": obj_schema([], {}),
    },
    "request.reject_request@1": {
        "params_schema": obj_schema(["reason", "request"], {"reason": REASON, "request": RID}),
        "result_schema": obj_schema([], {}),
    },
    "request.file_request@1": {
        "params_schema": obj_schema(
            ["command", "params"], {"command": {"type": "string", "pattern": CMD_ID}, "params": {"type": "object"}}
        ),
        "result_schema": obj_schema(["request"], {"request": RID}),
    },
    "registry.show_manifest@1": {
        "params_schema": {
            "type": "object",
            "additionalProperties": False,
            "properties": {
                "full": {"type": "boolean", "default": False, "description": "Also return the manifest bytes."},
                "check": {"type": "string", "maxLength": 67108864, "description": SHOW_CHECK},
            },
        },
        "result_schema": {
            "type": "object",
            "additionalProperties": False,
            "required": ["digest"],
            "properties": {"digest": {"type": "string", "pattern": DIGEST}, "bytes": {"type": "string"}},
        },
    },
}

REQ = "0190a1b2-c3d4-7e5f-8a6b-7c8d9e0f1a2b"
SHA = "sha256:" + "0123456789abcdef" * 4
REC = {
    "request": REQ,
    "seq": 1,
    "command": "align.run_321@1",
    "params_commitment": SHA,
    "requester": {"id": "agent-1", "kind": "agent"},
    "filed_at_head": SHA,
    "state": "pending",
}


def rec(**changes):
    r = dict(REC)
    for k, v in changes.items():
        if v is None:
            r.pop(k)
        else:
            r[k] = v
    return r


REAL_CASES = [
    ("list_params_empty", "request.list_requests@1", "params", {}),
    ("list_params_state", "request.list_requests@1", "params", {"state": "pending"}),
    ("list_params_bad_state", "request.list_requests@1", "params", {"state": "open"}),
    ("list_params_extra", "request.list_requests@1", "params", {"state": "pending", "x": 1}),
    ("list_result_empty", "request.list_requests@1", "result", {"requests": []}),
    ("list_result_record", "request.list_requests@1", "result", {"requests": [REC]}),
    (
        "list_result_resolved",
        "request.list_requests@1",
        "result",
        {"requests": [rec(state="rejected", resolved_by={"id": "alice", "kind": "human"}, reason="dup")]},
    ),
    ("list_result_seq", "request.list_requests@1", "result", {"requests": [rec(seq=0)]}),
    ("list_result_state", "request.list_requests@1", "result", {"requests": [REC, rec(state="open")]}),
    (
        "list_result_requester",
        "request.list_requests@1",
        "result",
        {"requests": [rec(requester={"id": "r2", "kind": "robot"})]},
    ),
    ("list_result_uppercase_id", "request.list_requests@1", "result", {"requests": [rec(request=REQ.upper())]}),
    ("list_result_missing", "request.list_requests@1", "result", {"requests": [rec(filed_at_head=None)]}),
    ("list_result_commitment", "request.list_requests@1", "result", {"requests": [rec(params_commitment="sha256:x")]}),
    ("withdraw_ok", "request.withdraw_request@1", "params", {"request": REQ}),
    ("withdraw_missing", "request.withdraw_request@1", "params", {}),
    ("withdraw_uuid_v4", "request.withdraw_request@1", "params", {"request": "0190a1b2-c3d4-4e5f-8a6b-7c8d9e0f1a2b"}),
    ("withdraw_result", "request.withdraw_request@1", "result", {}),
    ("withdraw_result_extra", "request.withdraw_request@1", "result", {"ok": True}),
    ("reject_ok", "request.reject_request@1", "params", {"reason": "duplicate", "request": REQ}),
    ("reject_empty_reason", "request.reject_request@1", "params", {"reason": "", "request": REQ}),
    ("reject_long_reason", "request.reject_request@1", "params", {"reason": "r" * 2001, "request": REQ}),
    ("reject_reason_2000", "request.reject_request@1", "params", {"reason": "é" * 2000, "request": REQ}),
    ("file_ok", "request.file_request@1", "params", {"command": "align.run_321@1", "params": {"a": [1]}}),
    ("file_bad_command", "request.file_request@1", "params", {"command": "Align@1", "params": {}}),
    ("file_params_type", "request.file_request@1", "params", {"command": "align.run_321@1", "params": []}),
    ("file_result", "request.file_request@1", "result", {"request": REQ}),
    ("file_result_extra", "request.file_request@1", "result", {"request": REQ, "extra": 1}),
    ("show_params_empty", "registry.show_manifest@1", "params", {}),
    ("show_params_full", "registry.show_manifest@1", "params", {"full": True}),
    ("show_params_full_type", "registry.show_manifest@1", "params", {"full": "yes"}),
    ("show_params_unknown", "registry.show_manifest@1", "params", {"verbose": True}),
    ("show_result", "registry.show_manifest@1", "result", {"digest": SHA, "bytes": "{}"}),
    ("show_result_digest", "registry.show_manifest@1", "result", {"digest": SHA.upper().replace("SHA256", "sha256")}),
    ("show_result_missing", "registry.show_manifest@1", "result", {"bytes": "{}"}),
]


def real_expected(cmd, target, doc):
    schema = REAL[cmd][target + "_schema"]
    r = Validator(schema).node(schema, doc, (), "false")
    if isinstance(r, Fail):
        return {"outcome": "error", "code": "validation", "reason": r.reason, "pointer": show(r.ptr), "keyword": r.keyword}
    return {"outcome": "ok"}


def load(text):
    return json.loads(text, parse_int=float)


def render():
    out = {}
    out["registry.json"] = json.dumps(REGISTRY, indent=1, ensure_ascii=False) + "\n"
    out["state.json"] = json.dumps(STATE, indent=1, ensure_ascii=False) + "\n"
    lines = []
    seen = set()
    for name, cmd, principal, params in CASES:
        assert name not in seen, name
        seen.add(name)
        p = load(json.dumps(params))
        row = {"case": name, "command": cmd, "principal": principal, "params": params, "expect": expected(cmd, principal, p)}
        lines.append(json.dumps(row, ensure_ascii=False, sort_keys=True))
    out["params.jsonl"] = "\n".join(lines) + "\n"
    real_cases = []
    for name, cmd, target, doc in REAL_CASES:
        assert name not in seen, name
        seen.add(name)
        expect = real_expected(cmd, target, load(json.dumps(doc)))
        real_cases.append({"case": name, "command": cmd, "target": target, "document": doc, "expect": expect})
    real = {"commands": REAL, "cases": real_cases}
    out["real.json"] = json.dumps(real, indent=1, ensure_ascii=False, sort_keys=True) + "\n"
    return out


def main():
    files = render()
    if "--check" in sys.argv[1:]:
        stale = [n for n, t in files.items() if not (HERE / n).exists() or (HERE / n).read_text(encoding="utf-8") != t]
        if stale:
            print("stale:", ", ".join(stale))
            return 1
        print(f"ok: {len(CASES)} cases")
        return 0
    for n, t in files.items():
        (HERE / n).write_text(t, encoding="utf-8", newline="\n")
    print(f"wrote {len(files)} files, {len(CASES)} cases")
    return 0


if __name__ == "__main__":
    sys.exit(main())
