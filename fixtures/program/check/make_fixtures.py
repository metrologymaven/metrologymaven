#!/usr/bin/env python3
"""Fixtures of spec node AUTO.PROG.CHECK (static program checker, program.check@1).

Writes, from hand-written programs in this file:
- fixtures/program/check/*.json: canonical Command Programs (AUTO.PROG.FMT), checked against
  the demo commands of fixtures/manifest/prog-demo.json (written by
  fixtures/program/types/make_fixtures.py) and the catalogue
  fixtures/program/types/catalogue.json; zero-limit.json is a document AUTO.PROG.FMT refuses;
- fixtures/program/check/clean.expected.json: the expected result of clean.json (AC-1);
- fixtures/program/check/expected.json: the expected CheckResult of every program.

How each expected value is obtained (nothing here runs the Rust code):
- the diagnostics of the other modules (expr, flow, oper, family, fmt) are labelled by hand
  from the rules of their spec nodes and the pinned choices in their module docs
  (crates/fid-program/src/expr/types/mod.rs, flow.rs, oper.rs, family.rs); spans come from
  the expression text with len();
- command resolution, argument checks, step classes, the program class, the message text and
  the order are computed by `check()` below, an independent Python model of the rules of
  spec/auto/prog/check/NODE.md and of the module docs of crates/fid-program/src/check.rs.
  Literal arguments are validated by `valid()`, a small JSON Schema model of the keywords the
  demo schemas use (type, enum, minimum, maximum, maxLength, x-quantity literals); whether an
  expression argument has the wrong type is labelled by hand per case (from the type rules);
  which steps are unreachable is labelled by hand per case (from the flow rules);
- program_digest is SHA-256 of the program bytes, manifest_digest SHA-256 of the manifest
  file (which is canonical).

Stdlib only. CC-BY-4.0 project data. `--check` regenerates in memory and fails if any
committed file differs.
"""

import hashlib
import json
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
MANIFEST = ROOT / "fixtures" / "manifest" / "prog-demo.json"
CATALOGUE = ROOT / "fixtures" / "program" / "types" / "catalogue.json"

CLASS_ORDER = ["R0", "R1", "I1", "W1", "W2", "X1", "D1", "W3", "M1", "S1", "A0"]
MODULES = ["fmt", "expr", "flow", "oper", "family", "check"]
DECIMAL = re.compile(r"^[+-]?[0-9]+(\.[0-9]+)?$")


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


def load_manifest():
    m = json.loads(MANIFEST.read_text(encoding="utf-8"))
    return {(c["id"], c["major"]): c for c in m["commands"]}


def load_units():
    cat = json.loads(CATALOGUE.read_text(encoding="utf-8"))
    return {k["kind"]: {u["symbol"] for u in k["units"]} for k in cat["kinds"]}


COMMANDS = load_manifest()
UNITS = load_units()

# ---------------------------------------------------------------- JSON Schema model


def json_type(v):
    if isinstance(v, bool):
        return {"boolean"}
    if isinstance(v, int):
        return {"integer", "number"}
    if isinstance(v, str):
        return {"string"}
    if isinstance(v, dict):
        return {"object"}
    if isinstance(v, list):
        return {"array"}
    if v is None:
        return {"null"}
    raise TypeError(v)


def valid(schema, v):
    """Whether literal `v` passes `schema`, for the keywords the demo schemas use. An
    x-quantity schema takes an authored literal {"unit", "value"}: a decimal text and a unit
    the kind accepts; the normalised value is a number."""
    known = {"type", "enum", "minimum", "maximum", "maxLength", "x-quantity", "x-unit"}
    assert set(schema) <= known, schema
    if "x-quantity" in schema and isinstance(v, dict):
        if set(v) != {"unit", "value"} or not all(isinstance(v[k], str) for k in v):
            return False
        if v["unit"] not in UNITS.get(schema["x-quantity"], set()) or not DECIMAL.match(v["value"]):
            return False
        v = 1  # a normalised SI number; no demo schema bounds a quantity
    if "type" in schema and schema["type"] not in json_type(v):
        return False
    if "enum" in schema and v not in schema["enum"]:
        return False
    if "minimum" in schema and (not isinstance(v, int) or v < schema["minimum"]):
        return False
    if "maximum" in schema and (not isinstance(v, int) or v > schema["maximum"]):
        return False
    if "maxLength" in schema and isinstance(v, str) and len(v) > schema["maxLength"]:
        return False
    return True


# ---------------------------------------------------------------- program builder


class P:
    """Builds one program and its expected result."""

    def __init__(self, n):
        self.n = n * 256
        self.pid = self.uuid()
        self.foreign = []  # hand-labelled diagnostics of the other modules
        self.unreachable = set()
        self.wrong_type = set()  # (step_id, arg name): expression arguments of the wrong type

    def uuid(self):
        self.n += 1
        return "0190c000-0000-7000-8000-%012x" % self.n

    def step(self, kind, **f):
        s = {"kind": kind, "step_id": self.uuid()}
        s.update(f)
        return s

    def invoke(self, command, major, args, bind=None):
        f = {"args": args, "command": command, "major": major}
        if bind:
            f["bind"] = bind
        return self.step("invoke", **f)

    def diag(self, module, code, names=(), step=None, slot=None, span=None, severity="error"):
        self.foreign.append(
            {
                "code": code,
                "module": module,
                "names": list(names),
                "severity": severity,
                "slot": slot,
                "span": None if span is None else {"end": span[1], "start": span[0]},
                "step_id": None if step is None else step["step_id"],
            }
        )

    def doc(self, main, variables=(), params=(), variants=None, subs=()):
        d = {
            "format": "fid-program",
            "format_version": 1,
            "main": main,
            "meta": {"title": "AUTO.PROG.CHECK fixture"},
            "params": list(params),
            "program_id": self.pid,
            "subroutines": list(subs),
            "variables": [{"name": n, "type": t} for n, t in variables],
        }
        if variants is not None:
            d["variants"] = variants
        return d


def lit(v):
    return {"literal": v}


def ex(src):
    return {"expr": src}


def qty(value, unit):
    return {"unit": unit, "value": value}


def whole(src):
    return (0, len(src))


# ---------------------------------------------------------------- the checker model


def steps_in_order(doc):
    out = []

    def walk(steps):
        for s in steps:
            out.append(s)
            if s["kind"] == "if":
                for b in s["branches"]:
                    walk(b["body"])
                walk(s["else_body"])
            elif s["kind"] == "loop":
                walk(s["body"])

    walk(doc["main"])
    for sub in doc["subroutines"]:
        walk(sub["body"])
    return out


def message(d):
    m = d["code"]
    if d["slot"] is not None:
        m += " at " + d["slot"]
    if d["names"]:
        m += ": " + ", ".join(d["names"])
    return m


def check_invoke(p, s):
    """Resolution and argument checks of one invoke: (class, human_only) or None, diagnostics."""
    label = "%s@%d" % (s["command"], s["major"])
    d = COMMANDS.get((s["command"], s["major"]))
    state = None if d is None else d["lifecycle"].get("state")

    def mk(code, names, severity="error"):
        x = {"code": code, "module": "check", "names": names, "severity": severity, "slot": None, "span": None, "step_id": s["step_id"]}
        x["message"] = message(x)
        return x

    if state not in ("active", "deprecated"):
        return None, [mk("unknown_command", [label])]
    out = []
    if state == "deprecated":
        names = [label]
        if "replaced_by" in d["lifecycle"]:
            names.append(d["lifecycle"]["replaced_by"])
        out.append(mk("deprecated_command", names, "warning"))
    schema = d["params_schema"]
    props = schema.get("properties", {})
    problems = {}
    for r in schema.get("required", []):
        if r not in s["args"]:
            problems[r] = "missing"
    for name, arg in s["args"].items():
        if name not in props:
            problems[name] = "unknown"
        elif "literal" in arg:
            if not valid(props[name], arg["literal"]):
                problems[name] = "invalid"
        elif (s["step_id"], name) in p.wrong_type:
            problems[name] = "wrong_type"
    if problems:
        names = sorted(problems, key=lambda n: n.encode())
        x = mk("bad_arguments", names)
        x["message"] = "bad_arguments: " + ", ".join("%s (%s)" % (n, problems[n]) for n in names)
        out.append(x)
    return (d["side_effect"], d["human_only"]), out


def order(items):
    """items: (position, diagnostic) in collection order; stable sort by position, module, code."""
    items = sorted(items, key=lambda it: (it[0], MODULES.index(it[1]["module"]), it[1]["code"].encode()))
    return [d for _, d in items]


def check(p, doc):
    steps = steps_in_order(doc)
    pos = {}
    for i, s in enumerate(steps):
        pos.setdefault(s["step_id"], i + 1)
    items = []
    for d in p.foreign:
        d = dict(d)
        d["message"] = message(d)
        items.append((pos.get(d["step_id"], 0), d))
    classes = []
    for s in steps:
        if s["kind"] != "invoke":
            continue
        cls, diags = check_invoke(p, s)
        classes.append(
            {
                "class": None if cls is None else cls[0],
                "command": "%s@%d" % (s["command"], s["major"]),
                "human_only": None if cls is None else cls[1],
                "reachable": s["step_id"] not in p.unreachable,
                "step_id": s["step_id"],
            }
        )
        items.extend((pos[s["step_id"]], d) for d in diags)
    diagnostics = order(items)
    reach = [c for c in classes if c["reachable"]]
    if any(c["class"] is None for c in reach):
        program_class = None
    else:
        program_class = max((c["class"] for c in reach), key=CLASS_ORDER.index, default="R0")
    return result(canon(doc), diagnostics, program_class, classes)


def result(program_bytes, diagnostics, program_class, steps):
    return {
        "diagnostics": diagnostics,
        "manifest_digest": sha(MANIFEST.read_bytes()),
        "passed": not any(d["severity"] == "error" for d in diagnostics),
        "program_class": program_class,
        "program_digest": sha(program_bytes),
        "steps": steps,
    }


# ---------------------------------------------------------------- cases


def clean():
    """AC-1: every invoke resolves, every argument fits; classes R1, W2, R0; program W2."""
    p = P(1)
    s1 = p.invoke("demo.measure", 1, {"feature": lit("bore-1"), "samples": lit(10)}, bind="m")
    s3 = p.invoke("demo.set_offset", 2, {"axis": lit("z"), "note": lit("re-zero"), "offset": ex("m.deviation")})
    s2 = p.step("if", branches=[{"body": [s3], "cond": "m.deviation > 0.1[mm]"}], else_body=[])
    s4 = p.invoke("demo.read_status", 1, {}, bind="st")
    s5 = p.step("comment", text="done")
    return p, p.doc([s1, s2, s4, s5])


def unknown_cmd():
    """AC-2: an absent command and an absent major. The type checker also reports each as
    type_unknown at `command` (AUTO.PROG.EXPR.TYPES: an invoke the manifest lacks)."""
    p = P(2)
    s1 = p.invoke("demo.nope", 1, {})
    s2 = p.invoke("demo.set_offset", 3, {"offset": lit(qty("1", "mm"))})
    s3 = p.invoke("demo.read_status", 1, {})
    p.diag("expr", "type_unknown", ["demo.nope@1"], s1, "command")
    p.diag("expr", "type_unknown", ["demo.set_offset@3"], s2, "command")
    return p, p.doc([s1, s2, s3])


def bad_args():
    """AC-3: required `axis` missing, unknown `colour`, `offset: 5[deg]` (an angle where a
    length is declared). The type checker reports the expression as type_mismatch naming
    the declared then the found type, over the whole expression."""
    p = P(3)
    src = "5[deg]"
    s1 = p.invoke("demo.set_offset", 2, {"colour": lit("red"), "offset": ex(src)})
    p.wrong_type.add((s1["step_id"], "offset"))
    p.diag("expr", "type_mismatch", ["quantity<length>", "quantity<angle>"], s1, "args.offset", whole(src))
    return p, p.doc([s1])


def mixed():
    """AC-4: a family variant binding an undeclared parameter (declaration, no step), an
    operator field with a unit its kind lacks, a type error and an illegal jump, so the
    diagnostics come from four modules in document order."""
    p = P(4)
    nominal = {"name": "nominal", "quantity_kind": "length", "required": True, "type": "quantity"}
    variants = [{"binding": {"colour": "red", "nominal": qty("10", "mm")}, "name": "small"}]
    descriptor = {
        "fields": [{"label": "Gap", "name": "gap", "quantity_kind": "length", "response": "quantity", "unit": "furlong"}],
        "format": "operator-v1",
        "prompt": "Enter the measured gap.",
    }
    s1 = p.step("operator", descriptor=descriptor)
    s2 = p.step("assign", expr='"abc"', var="n")
    s3 = p.invoke("demo.measure", 1, {"feature": lit("bore-1")})
    s4 = p.step("jump", label="nowhere")
    # AUTO.PROG.FAMILY: a variant's binding errors come wrapped; code and names of the inner one.
    p.diag("family", "unknown_parameter", ["small", "colour"])
    # AUTO.PROG.OPER: a quantity field whose unit is not one of its kind's units is bad_descriptor.
    p.diag("oper", "bad_descriptor", ["gap"], s1)
    # AUTO.PROG.EXPR.TYPES: a string assigned to an int is type_mismatch (declared, found).
    p.diag("expr", "type_mismatch", ["int", "string"], s2, "expr", whole('"abc"'))
    # AUTO.PROG.FLOW: a jump to a label that exists nowhere is illegal_jump (label, undefined).
    p.diag("flow", "illegal_jump", ["nowhere", "undefined"], s4)
    return p, p.doc([s1, s2, s3, s4], variables=[("n", "int")], params=[nominal], variants=variants)


def motion():
    """AC-5: an M1 step makes the program M1; a human-only W3 step is listed as such."""
    p = P(5)
    s1 = p.invoke("demo.move_axis", 1, {"axis": lit("x"), "distance": lit(qty("25", "mm"))})
    s2 = p.invoke("demo.approve_plan", 1, {"plan": lit("plan-7")})
    s3 = p.invoke("demo.measure", 1, {"feature": lit("bore-1")})
    return p, p.doc([s1, s2, s3])


def deprecated():
    """REQ-005: a deprecated major is a warning naming its replacement; the check passes."""
    p = P(6)
    s1 = p.invoke("demo.set_offset", 1, {"offset": lit(qty("0.5", "mm"))})
    return p, p.doc([s1])


def unreachable():
    """REQ-004: the class is over reachable steps only. The M1 step after an unconditional
    jump is unreachable_step (a warning) and does not raise the class above R1."""
    p = P(7)
    s1 = p.step("jump", label="done")
    s2 = p.invoke("demo.move_axis", 1, {"axis": lit("y"), "distance": lit(qty("5", "mm"))})
    s3 = p.step("label", name="done")
    s4 = p.invoke("demo.measure", 1, {"feature": lit("bore-1")})
    p.unreachable.add(s2["step_id"])
    p.diag("flow", "unreachable_step", [], s2, severity="warning")
    return p, p.doc([s1, s2, s3, s4])


def literals():
    """REQ-002 literals by JSON Schema: a number where a string is declared, an integer above
    its maximum, a value outside an enum and a length literal in degrees."""
    p = P(8)
    s1 = p.invoke("demo.measure", 1, {"feature": lit(7), "samples": lit(500)})
    s2 = p.invoke("demo.set_offset", 2, {"axis": lit("w"), "offset": lit(qty("1", "deg"))})
    s3 = p.invoke("demo.set_offset", 2, {"axis": lit("x"), "note": lit("n" * 65), "offset": lit(qty("1", "in"))})
    return p, p.doc([s1, s2, s3])


def limits():
    """A loop above the iteration limit in a program that parses: analyse_flow reports it
    once (bad_iteration_limit naming the value); the raw-document check is not run."""
    p = P(9)
    s2 = p.invoke("demo.read_status", 1, {})
    s1 = p.step("loop", body=[s2], cond="true", max_iterations=2000000, mode="while")
    p.diag("flow", "bad_iteration_limit", ["2000000"], s1)
    return p, p.doc([s1])


CASES = {
    "clean": clean,
    "unknown-cmd": unknown_cmd,
    "bad-args": bad_args,
    "mixed": mixed,
    "motion": motion,
    "deprecated": deprecated,
    "unreachable": unreachable,
    "literals": literals,
    "limits": limits,
}


def zero_limit():
    """A document AUTO.PROG.FMT refuses (max_iterations 0 is schema_invalid): one fmt
    diagnostic, then each loop's limit from the raw document, once per loop
    (missing_iteration_limit / bad_iteration_limit, AUTO.PROG.FLOW REQ-001), in document
    order. Nothing is classified."""
    p = P(10)
    a = p.step("loop", body=[], cond="true", max_iterations=0, mode="while")
    b = p.step("loop", body=[], cond="true", max_iterations=2000000, mode="while")
    doc = p.doc([a, b])
    raw = canon(doc)
    diags = [
        {"code": "schema_invalid", "module": "fmt", "names": [], "severity": "error", "slot": None, "span": None, "step_id": None},
        {"code": "bad_iteration_limit", "module": "flow", "names": ["0"], "severity": "error", "slot": None, "span": None, "step_id": a["step_id"]},
        {"code": "bad_iteration_limit", "module": "flow", "names": ["2000000"], "severity": "error", "slot": None, "span": None, "step_id": b["step_id"]},
    ]
    for d in diags:
        d["message"] = message(d)
    return raw, result(raw, diags, None, [])


def build():
    files = {}
    expected = {}
    for name, fn in CASES.items():
        p, doc = fn()
        files[HERE / (name + ".json")] = canon(doc)
        expected[name] = check(p, doc)
    raw, rep = zero_limit()
    files[HERE / "zero-limit.json"] = raw
    expected["zero-limit"] = rep
    files[HERE / "clean.expected.json"] = canon(expected["clean"])
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
