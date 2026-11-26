#!/usr/bin/env python3
"""Fixtures of spec node AUTO.PROG.EXPR.TYPES (type system, unit algebra, scoping).

Writes, from hand-written tables in this file:
- fixtures/program/types/catalogue.json: the GEO.FRAME.UNITS catalogue
  (read from fixtures/core/reg/def/schema/catalogue/geo_frame_units.json) plus the kinds
  `area` and `time`;
- fixtures/manifest/prog-demo.json: a CORE.REG.MAN manifest with result schemas, quantity
  kinds, an enum parameter, a `value_of_record` parameter and an `x-ref: result`
  parameter (a result reference, not a value of record since CORE.REG.DEF.SCHEMA REQ-011),
  plus the `demo.*` commands of AUTO.PROG.CHECK (DEMO_COMMANDS);
  definition_digest = SHA-256 of the RFC 8785 form of each definition
  without that field, quantity_catalogue_digest = SHA-256 of catalogue.json;
- fixtures/program/types/valid/*.json and fixtures/program/types/*.json: canonical
  Command Programs (AUTO.PROG.FMT);
- fixtures/program/types/expected.json: the expected TypeReport of every program.

Every expected type and diagnostic below is labelled by hand from the rules of
spec/auto/prog/expr/types/NODE.md and the pinned choices in the module docs of
crates/fid-program/src/expr/types/mod.rs; nothing here runs the Rust code. Spans are
computed from the expression text with str.index (all sources are ASCII).

Stdlib only. CC-BY-4.0 project data. `--check` regenerates in memory and fails if any
committed file differs.
"""

import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
GEO = ROOT / "fixtures" / "core" / "reg" / "def" / "schema" / "catalogue" / "geo_frame_units.json"
MANIFEST = ROOT / "fixtures" / "manifest" / "prog-demo.json"


def canon(v):
    """RFC 8785 for this data: ASCII keys and strings, integers only (asserted)."""

    def walk(x):
        if isinstance(x, float):
            raise TypeError("floats are not used in these fixtures")
        if isinstance(x, dict):
            for k, y in x.items():
                assert k.isascii()
                walk(y)
        elif isinstance(x, list):
            for y in x:
                walk(y)
        elif isinstance(x, str):
            assert x.isascii()

    walk(v)
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()


def sha(b):
    return "sha256:" + hashlib.sha256(b).hexdigest()


# ---------------------------------------------------------------- catalogue


def catalogue():
    geo = json.loads(GEO.read_text(encoding="utf-8"))
    kinds = list(geo["kinds"])

    def u(symbol, num, den):
        return {"den": str(den), "num": str(num), "pi": False, "symbol": symbol}

    kinds.append({"canonical": "m^2", "kind": "area", "units": [u("cm^2", 1, 10000), u("m^2", 1, 1), u("mm^2", 1, 1000000)]})
    kinds.append({"canonical": "s", "kind": "time", "units": [u("h", 3600, 1), u("min", 60, 1), u("ms", 1, 1000), u("s", 1, 1)]})
    kinds.sort(key=lambda k: k["kind"])
    for k in kinds:
        assert k["units"] == sorted(k["units"], key=lambda x: x["symbol"].encode())
    return {"format": geo["format"], "kinds": kinds, "pi_hex": geo["pi_hex"]}


# ---------------------------------------------------------------- manifest

LEN = {"type": "number", "x-quantity": "length", "x-unit": "mm"}
ANG = {"type": "number", "x-quantity": "angle", "x-unit": "deg"}


def obj(props):
    return {"properties": props, "type": "object"}


COMMANDS = [
    (
        "feature.fit_cylinder",
        1,
        obj({"max_iter": {"type": "integer"}, "points": {"type": "string", "x-ref": "element"}, "tol": LEN}),
        obj(
            {
                "axis_tilt": ANG,
                "center": obj({"x": LEN, "y": LEN}),
                "diameter": LEN,
                "feature": {"type": "string", "x-ref": "feature"},
                "fit": {"$ref": "#/$defs/fit"},
                "ok": {"type": "boolean"},
                "residuals": {"items": LEN, "type": "array"},
            }
        ),
    ),
    (
        "probe.touch_point",
        2,
        obj({"approach": {"enum": ["fast", "slow"], "type": "string"}, "index": {"type": "integer"}}),
        obj({"deviation": LEN}),
    ),
    ("report.cite_result", 1, obj({"result": {"type": "string", "x-ref": "result"}}), obj({})),
    (
        "report.set_nominal",
        1,
        obj(
            {
                "characteristic": {"type": "string", "x-ref": "characteristic"},
                "nominal": dict(LEN, value_of_record=True),
                "upper": LEN,
            }
        ),
        obj({}),
    ),
]


# The `demo.*` commands of AUTO.PROG.CHECK (spec/auto/prog/check/NODE.md, Test fixtures):
# several side-effect classes, one deprecated major with a replacement, one M1 command and
# one human-only command. They are full CORE.REG.DEF.FIELD definitions (lifecycle with
# `since`) so the checker's agent-view test can export them through CORE.REG.MAN. No
# AUTO.PROG.EXPR.TYPES program uses them.
STR = {"type": "string"}
AXIS = {"enum": ["x", "y", "z"], "type": "string"}


def demo_params(props, required):
    return {"additionalProperties": False, "properties": props, "required": required, "type": "object"}


DEMO_COMMANDS = [
    (
        "demo.approve_plan",
        1,
        {
            "human_only": True,
            "mcp_projection": "request_only",
            "params": demo_params({"plan": dict(STR, maxLength=64)}, ["plan"]),
            "side_effect": "W3",
        },
    ),
    (
        "demo.measure",
        1,
        {
            "params": demo_params({"feature": dict(STR, maxLength=64), "samples": {"maximum": 100, "minimum": 1, "type": "integer"}}, ["feature"]),
            "result": obj({"deviation": LEN}),
            "side_effect": "R1",
        },
    ),
    (
        "demo.move_axis",
        1,
        {
            "exposure": ["cli", "python"],
            "mcp_projection": "never",
            "network": "device",
            "params": demo_params({"axis": AXIS, "distance": LEN}, ["axis", "distance"]),
            "side_effect": "M1",
        },
    ),
    ("demo.read_status", 1, {"params": demo_params({}, []), "result": obj({"ready": {"type": "boolean"}}), "side_effect": "R0"}),
    (
        "demo.set_offset",
        1,
        {
            "lifecycle": {"remove_after": "2027-06-01", "replaced_by": "demo.set_offset@2", "since": "2026-06-01", "state": "deprecated"},
            "params": demo_params({"offset": LEN}, ["offset"]),
            "side_effect": "W2",
        },
    ),
    (
        "demo.set_offset",
        2,
        {
            "params": demo_params({"axis": AXIS, "note": dict(STR, maxLength=64), "offset": LEN}, ["axis", "offset"]),
            "side_effect": "W2",
        },
    ),
]


def demo_definition(cid, major, f):
    d = {
        "algorithms": [],
        "description": "Fixture definition for AUTO.PROG.CHECK.",
        "determinism": "D0",
        "effects": [],
        "exposure": f.get("exposure", ["cli", "mcp", "python"]),
        "fs_effect": "none",
        "human_only": f.get("human_only", False),
        "id": cid,
        "idempotency": "natural",
        "lifecycle": f.get("lifecycle", {"since": "2026-01-15", "state": "active"}),
        "major": major,
        "mcp_projection": f.get("mcp_projection", "exposed"),
        "params_schema": f["params"],
        "preconditions": [],
        "result_schema": f.get("result", obj({})),
        "side_effect": f["side_effect"],
        "title": "Fixture %s@%d" % (cid, major),
        "undo": "not_undoable",
        "web": "available",
    }
    if "network" in f:
        d["network"] = f["network"]
    return d


def manifest(cat_digest):
    cmds = []
    for cid, major, params, result in COMMANDS:
        d = {
            "algorithms": [],
            "description": "Fixture definition for AUTO.PROG.EXPR.TYPES.",
            "determinism": "D0",
            "effects": [],
            "exposure": ["cli", "mcp", "python"],
            "fs_effect": "none",
            "human_only": False,
            "id": cid,
            "idempotency": "natural",
            "lifecycle": {"state": "active"},
            "major": major,
            "mcp_projection": "exposed",
            "params_schema": params,
            "preconditions": [],
            "result_schema": result,
            "side_effect": "R1",
            "title": "Fixture %s@%d" % (cid, major),
            "undo": "not_undoable",
            "web": "available",
        }
        d["definition_digest"] = sha(canon(d))
        cmds.append(d)
    for cid, major, fields in DEMO_COMMANDS:
        d = demo_definition(cid, major, fields)
        d["definition_digest"] = sha(canon(d))
        cmds.append(d)
    cmds.sort(key=lambda d: (d["id"], d["major"]))
    return {"commands": cmds, "format": 1, "quantity_catalogue_digest": cat_digest}


# ---------------------------------------------------------------- programs


class P:
    """Builds one program and its hand-labelled expected report."""

    def __init__(self, n):
        self.n = n * 256
        self.pid = self.uuid()
        self.types = []
        self.diags = []

    def uuid(self):
        self.n += 1
        return "018f0000-0000-7000-8000-%012x" % self.n

    def step(self, kind, **f):
        s = {"kind": kind, "step_id": self.uuid()}
        s.update(f)
        return s

    def typed(self, step, slot, src, ty):
        self.types.append({"not_of_record": True, "slot": slot, "span": {"end": len(src), "start": 0}, "step": step["step_id"], "type": ty})

    def diag(self, code, names, slot, step=None, span=None):
        self.diags.append(
            {
                "code": code,
                "names": names,
                "slot": slot,
                "span": None if span is None else {"end": span[1], "start": span[0]},
                "step": None if step is None else step["step_id"],
            }
        )

    def whole(self, src):
        return (0, len(src))

    def doc(self, main, variables=(), params=(), subs=()):
        return {
            "format": "fid-program",
            "format_version": 1,
            "main": main,
            "meta": {"title": "AUTO.PROG.EXPR.TYPES fixture"},
            "params": list(params),
            "program_id": self.pid,
            "subroutines": list(subs),
            "variables": [{"name": n, "type": t} for n, t in variables],
        }

    def report(self):
        return {"diagnostics": self.diags, "types": sorted(self.types, key=lambda t: (t["step"], t["slot"]))}


def assign(p, var, src, ty=None):
    s = p.step("assign", expr=src, var=var)
    if ty:
        p.typed(s, "expr", src, ty)
    return s


def ex(src):
    return {"expr": src}


def lit(v):
    return {"literal": v}


NOMINAL = {"name": "nominal", "quantity_kind": "length", "required": True, "type": "quantity"}
QL = "quantity<length>"


def bore():
    """AC-1: every step kind, scoping, records, enums, subroutines; no diagnostic."""
    p = P(1)
    s1 = p.step("invoke", args={"max_iter": lit(50), "points": lit("pc-17"), "tol": ex("0.05[mm]")}, bind="bore", command="feature.fit_cylinder", major=1)
    p.typed(s1, "args.tol", "0.05[mm]", QL)
    s2 = assign(p, "d", "bore.diameter", QL)
    s3 = assign(p, "tol", "2", "int")
    c4 = "abs(d - nominal) <= 0.02[mm]"
    s5 = assign(p, "ok", "true", "bool")
    s6 = assign(p, "ok", "false", "bool")
    s4 = p.step("if", branches=[{"body": [s5], "cond": c4}], else_body=[s6])
    p.typed(s4, "branches.0.cond", c4, "bool")
    s8 = p.step("invoke", args={"approach": ex("mode"), "index": ex("i")}, bind="touch", command="probe.touch_point", major=2)
    p.typed(s8, "args.approach", "mode", "enum<fast|slow>")
    p.typed(s8, "args.index", "i", "int")
    s9 = assign(p, "d", "touch.deviation + bore.center.x", QL)
    s7 = p.step("loop", body=[s8, s9], mode="counted", var="i", max_iterations=100, **{"from": "1", "to": "len(bore.residuals)", "step": "1"})
    p.typed(s7, "from", "1", "int")
    p.typed(s7, "to", "len(bore.residuals)", "int")
    p.typed(s7, "step", "1", "int")
    s11 = assign(p, "n", "n + 1", "int")
    c10 = "not ok and n < 3"
    s10 = p.step("loop", body=[s11], cond=c10, max_iterations=3, mode="while")
    p.typed(s10, "cond", c10, "bool")
    s12 = p.step("call", args={"x": ex("nominal * 2")}, bind="fine", subroutine="check")
    p.typed(s12, "args.x", "nominal * 2", QL)
    s13 = assign(p, "ok", "fine", "bool")
    s14 = assign(p, "tol", "d / nominal", "decimal")
    s15 = assign(p, "n", "round(bore.axis_tilt / 1[deg])", "int")
    s15b = assign(p, "mode", '"slow"', "string")
    s15c = assign(p, "ok", 'mode == "fast" or mode != mode', "bool")
    s15d = assign(p, "tol", "floor(n / 2) + n / 2", "decimal")
    s16 = p.step("jump", cond="ok", label="end")
    p.typed(s16, "cond", "ok", "bool")
    s17 = p.step("label", name="end")
    s18 = p.step("return")
    r = "x > 1.5[mm] and nominal < 10[mm]"
    s20 = p.step("return", expr=r)
    p.typed(s20, "expr", r, "bool")
    sub = {"body": [s20], "name": "check", "params": [{"name": "x", "type": QL}], "returns": "bool"}
    main = [s1, s2, s3, s4, s7, s10, s12, s13, s14, s15, s15b, s15c, s15d, s16, s17, s18]
    variables = [("d", QL), ("ok", "bool"), ("n", "int"), ("tol", "decimal"), ("mode", "enum<fast|slow>")]
    return p.doc(main, variables, [NOMINAL], [sub]), p.report()


def area():
    """AC-1 and AC-3: unit algebra (products, quotients, sqrt, derived kinds, functions)."""
    p = P(2)
    main = [
        assign(p, "w", "12.5[mm]", QL),
        assign(p, "h", "4[mm]", QL),
        assign(p, "area", "w * h", "quantity<area>"),
        assign(p, "rate", "w * h / (w * h)", "decimal"),
    ]
    c = "w * 1[s] > h * 2[s]"
    s = p.step("if", branches=[{"body": [], "cond": c}], else_body=[])
    p.typed(s, "branches.0.cond", c, "bool")
    main.append(s)
    main.append(assign(p, "area", "sqrt(area) * w", "quantity<area>"))
    main.append(assign(p, "rate", "sin(30[deg]) + cos(0.5)", "decimal"))
    main.append(assign(p, "w", 'to_unit(min(w, h, 3[in]), "mm")', QL))
    variables = [("w", QL), ("h", QL), ("area", "quantity<area>"), ("rate", "decimal")]
    return p.doc(main, variables), p.report()


def unit_mismatch():
    """AC-2."""
    p = P(3)
    src = "d1 + 5[deg]"
    s = assign(p, "x", src)
    p.diag("unit_mismatch", ["length", "angle"], "expr", s, p.whole(src))
    return p.doc([s], [("d1", QL), ("x", QL)]), p.report()


def area_mismatch():
    """AC-3 (error path)."""
    p = P(4)
    src = "w * h + w"
    s = assign(p, "a", src)
    p.diag("unit_mismatch", ["area", "length"], "expr", s, p.whole(src))
    return p.doc([s], [("w", QL), ("h", QL), ("a", "quantity<area>")]), p.report()


def value_of_record():
    """AC-4: an expression for a value-of-record parameter. An `x-ref: result` parameter is
    no longer one (the fallback ended with CORE.REG.DEF.SCHEMA REQ-011): a string expression
    for it is a plain type mismatch against element<result>."""
    p = P(5)
    s1 = p.step("invoke", args={"points": lit("pc-1")}, bind="r", command="feature.fit_cylinder", major=1)
    nom, up = "r.diameter * 2", "r.diameter"
    s2 = p.step("invoke", args={"characteristic": lit("c1"), "nominal": ex(nom), "upper": ex(up)}, command="report.set_nominal", major=1)
    p.typed(s2, "args.nominal", nom, QL)
    p.typed(s2, "args.upper", up, QL)
    p.diag("expression_value_of_record", ["nominal"], "args.nominal", s2, p.whole(nom))
    res = '"r1"'
    s3 = p.step("invoke", args={"result": ex(res)}, command="report.cite_result", major=1)
    p.typed(s3, "args.result", res, "string")
    p.diag("type_mismatch", ["element<result>", "string"], "args.result", s3, p.whole(res))
    return p.doc([s1, s2, s3]), p.report()


def unknown_unit():
    """AC-5 (unknown unit)."""
    p = P(6)
    src = "12[furlong]"
    s = assign(p, "x", src)
    p.diag("unknown_unit", ["furlong"], "expr", s, p.whole(src))
    return p.doc([s], [("x", QL)]), p.report()


def out_of_block():
    """AC-5 (a binding used after its block)."""
    p = P(7)
    s2 = p.step("invoke", args={"index": lit(1)}, bind="t", command="probe.touch_point", major=2)
    s1 = p.step("if", branches=[{"body": [s2], "cond": "true"}], else_body=[])
    p.typed(s1, "branches.0.cond", "true", "bool")
    src = "t.deviation > 0[mm]"
    s3 = assign(p, "ok", src)
    p.diag("undeclared", ["t"], "expr", s3, (0, 1))
    return p.doc([s1, s3], [("ok", "bool")]), p.report()


def redeclared():
    """REQ-005 redeclarations, a bad type source, and no cascade from it."""
    p = P(8)
    p.diag("redeclared", ["a"], "variables.a")
    p.diag("type_unknown", ["quantity<furlong>"], "variables.b")
    p.diag("redeclared", ["x"], "subroutines.f.params.x")
    s1 = p.step("invoke", args={"index": lit(1)}, bind="t", command="probe.touch_point", major=2)
    s2 = p.step("invoke", args={"index": lit(2)}, bind="t", command="probe.touch_point", major=2)
    p.diag("redeclared", ["t"], "bind", s2)
    s3 = assign(p, "a", "b")
    sub = {"body": [], "name": "f", "params": [{"name": "x", "type": "int"}, {"name": "x", "type": "int"}]}
    return p.doc([s1, s2, s3], [("a", "int"), ("a", "decimal"), ("b", "quantity<furlong>")], subs=[sub]), p.report()


def type_mismatch():
    """REQ-001 and REQ-007: mismatches, unknown functions and commands, syntax errors."""
    p = P(9)
    main = []
    s = assign(p, "n", "1.5", "decimal")
    p.diag("type_mismatch", ["int", "decimal"], "expr", s, (0, 3))
    main.append(s)
    s = assign(p, "ok", "n", "int")
    p.diag("type_mismatch", ["bool", "int"], "expr", s, (0, 1))
    main.append(s)
    s = p.step("if", branches=[{"body": [], "cond": "n"}], else_body=[])
    p.typed(s, "branches.0.cond", "n", "int")
    p.diag("type_mismatch", ["bool", "int"], "branches.0.cond", s, (0, 1))
    main.append(s)
    src = '"a" + 1'
    s = assign(p, "s", src)
    p.diag("type_mismatch", ["number", "string"], "expr", s, p.whole(src))
    main.append(s)
    s = assign(p, "n", "foo(1)")
    p.diag("type_unknown", ["foo"], "expr", s, (0, 6))
    main.append(s)
    s = p.step("call", args={}, subroutine="nosuch")
    p.diag("undeclared", ["nosuch"], "subroutine", s)
    main.append(s)
    s = p.step("invoke", args={}, command="probe.nope", major=1)
    p.diag("type_unknown", ["probe.nope@1"], "command", s)
    main.append(s)
    s = assign(p, "n", "1 +")
    p.diag("type_unknown", ["expr_syntax"], "expr", s, (3, 3))
    main.append(s)
    src = "w * 1[s]"
    s = assign(p, "w", src, "quantity<dim:m*s>")
    p.diag("type_mismatch", [QL, "quantity<dim:m*s>"], "expr", s, p.whole(src))
    main.append(s)
    return p.doc(main, [("n", "int"), ("s", "string"), ("ok", "bool"), ("w", QL)]), p.report()


def enums():
    """Enum equality and assignment: members only (review of PR #418)."""
    p = P(10)
    E = "enum<fast|slow>"
    main = []
    src = 'mode == "medium"'
    s = assign(p, "ok", src)
    p.diag("type_mismatch", [E, '"medium"'], "expr", s, p.whole(src))
    main.append(s)
    src = "mode == other"
    s = assign(p, "ok", src)
    p.diag("type_mismatch", [E, "enum<x|y>"], "expr", s, p.whole(src))
    main.append(s)
    src = "mode == s"
    s = assign(p, "ok", src)
    p.diag("type_mismatch", [E, "string"], "expr", s, p.whole(src))
    main.append(s)
    src = '"medium"'
    s = assign(p, "mode", src, "string")
    p.diag("type_mismatch", [E, '"medium"'], "expr", s, p.whole(src))
    main.append(s)
    s = assign(p, "mode", "s", "string")
    p.diag("type_mismatch", [E, "string"], "expr", s, (0, 1))
    main.append(s)
    s = p.step("invoke", args={"approach": ex(src), "index": lit(1)}, command="probe.touch_point", major=2)
    p.typed(s, "args.approach", src, "string")
    p.diag("type_mismatch", [E, '"medium"'], "args.approach", s, p.whole(src))
    main.append(s)
    variables = [("mode", E), ("other", "enum<x|y>"), ("s", "string"), ("ok", "bool")]
    return p.doc(main, variables), p.report()


def scoping():
    """Read-only names, loop bounds, unknown kinds and majors, subroutine scope."""
    p = P(11)
    p.diag("type_unknown", ["record<probe.touch_point@1>"], "variables.r")
    p.diag("type_unknown", ["quantity<volume>"], "variables.k")
    s1 = p.step("invoke", args={"index": lit(1)}, bind="t", command="probe.touch_point", major=2)
    s3 = assign(p, "i", "2", "int")
    s2 = p.step("loop", body=[s3], mode="counted", var="i", max_iterations=5, **{"from": "1.5", "to": "x", "step": "1"})
    p.typed(s2, "from", "1.5", "decimal")
    p.typed(s2, "to", "x", QL)
    p.typed(s2, "step", "1", "int")
    p.diag("type_mismatch", ["int", "decimal"], "from", s2, (0, 3))
    p.diag("type_mismatch", ["int", QL], "to", s2, (0, 1))
    p.diag("read_only", ["i"], "var", s3)
    s4 = assign(p, "nominal", "x", QL)
    p.diag("read_only", ["nominal"], "var", s4)
    s5 = p.step("call", args={}, subroutine="peek")
    s6 = assign(p, "x", "t.deviation")
    p.diag("undeclared", ["t"], "expr", s6, (0, 1))
    s7 = assign(p, "n", "n", "int")
    sub = {"body": [s6, s7], "name": "peek", "params": []}
    variables = [("n", "int"), ("x", QL), ("r", "record<probe.touch_point@1>"), ("k", "quantity<volume>")]
    return p.doc([s1, s2, s4, s5], variables, [NOMINAL], [sub]), p.report()


CASES = {
    "valid/bore": bore,
    "valid/area": area,
    "unit-mismatch": unit_mismatch,
    "area-mismatch": area_mismatch,
    "value-of-record": value_of_record,
    "unknown-unit": unknown_unit,
    "out-of-block": out_of_block,
    "redeclared": redeclared,
    "type-mismatch": type_mismatch,
    "enums": enums,
    "scoping": scoping,
}


def build():
    files = {}
    cat = canon(catalogue())
    files[HERE / "catalogue.json"] = cat
    files[MANIFEST] = canon(manifest(sha(cat)))
    expected = {}
    for name, fn in CASES.items():
        doc, rep = fn()
        files[HERE / (name + ".json")] = canon(doc)
        expected[name] = rep
    top = {
        "cases": expected,
        "catalogue": "fixtures/program/types/catalogue.json",
        "manifest": "fixtures/manifest/prog-demo.json",
    }
    files[HERE / "expected.json"] = canon(top)
    return files


def main():
    files = build()
    if "--check" in sys.argv[1:]:
        bad = [p for p, b in files.items() if not p.exists() or p.read_bytes() != b]
        for p in bad:
            print("stale:", p.relative_to(ROOT))
        sys.exit(1 if bad else 0)
    for p, b in files.items():
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_bytes(b)
    print("wrote %d files" % len(files))


if __name__ == "__main__":
    main()
