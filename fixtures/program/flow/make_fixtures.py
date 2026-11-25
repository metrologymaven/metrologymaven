#!/usr/bin/env python3
"""Fixtures of spec node AUTO.PROG.FLOW (loops, branches, jumps and subroutines).

Writes, from hand-written programs in this file:
- fixtures/program/flow/valid/*.json and fixtures/program/flow/*.json: canonical Command
  Programs (AUTO.PROG.FMT), typed against the AUTO.PROG.EXPR.TYPES fixture manifest and
  catalogue (fixtures/manifest/prog-demo.json, fixtures/program/types/catalogue.json);
- fixtures/program/flow/raw/*.json: documents AUTO.PROG.FMT refuses (a loop without
  `max_iterations`, or with 0), for the raw-document check of REQ-AUTO.PROG.FLOW-001;
- fixtures/program/flow/valid/*.cfg.json: the expected control-flow graphs and call graph of
  each valid program (AC-1);
- fixtures/program/flow/expected.json: per program its expected CFGs and diagnostics, and per
  raw document its expected iteration-limit diagnostics.

Every expected diagnostic is labelled by hand below from the rules of
spec/auto/prog/flow/NODE.md and the pinned choices in the module docs of
crates/fid-program/src/flow.rs. The CFGs come from `lower()` here, an independent Python
implementation of those lowering rules; nothing here runs the Rust code.

Stdlib only. CC-BY-4.0 project data. `--check` regenerates in memory and fails if any
committed file differs.
"""

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]


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


SEVERITY = {"unreachable_step": "warning", "recursion": "info"}


class P:
    """Builds one program and its hand-labelled diagnostics."""

    def __init__(self, n):
        self.n = 0x4000 + n * 256
        self.pid = self.uuid()
        self.diags = []

    def uuid(self):
        self.n += 1
        return "018f0000-0000-7000-8000-%012x" % self.n

    def step(self, kind, **f):
        s = {"kind": kind, "step_id": self.uuid()}
        s.update(f)
        return s

    def diag(self, code, sub, step=None, names=()):
        """`sub` is None for main, else the subroutine name (or index, for raw documents)."""
        self.diags.append(
            {
                "code": code,
                "names": list(names),
                "severity": SEVERITY.get(code, "error"),
                "step": None if step is None else step["step_id"],
                "subroutine": sub,
            }
        )

    def doc(self, main, variables=(), subs=()):
        return {
            "format": "fid-program",
            "format_version": 1,
            "main": main,
            "meta": {"title": "AUTO.PROG.FLOW fixture"},
            "params": [],
            "program_id": self.pid,
            "subroutines": list(subs),
            "variables": [{"name": n, "type": t} for n, t in variables],
        }


def sub(name, params, body, returns=None):
    s = {"body": body, "name": name, "params": [{"name": n, "type": t} for n, t in params]}
    if returns is not None:
        s["returns"] = returns
    return s


def ex(src):
    return {"expr": src}


def lit(v):
    return {"literal": v}


QL = "quantity<length>"


# ---------------------------------------------------------------- CFG oracle


def visits(body):
    """(step, path, successor, successor kind) for every step in document order."""
    out = []

    def walk(steps, follow, end_kind, path):
        for i, s in enumerate(steps):
            if i + 1 < len(steps):
                succ, kind = steps[i + 1]["step_id"], "next"
            else:
                succ, kind = follow, end_kind
            out.append((s, path, succ, kind))
            sid = s["step_id"]
            if s["kind"] == "if":
                for k, b in enumerate(s["branches"]):
                    walk(b["body"], succ, kind, path + [(sid, k)])
                walk(s["else_body"], succ, kind, path + [(sid, len(s["branches"]))])
            elif s["kind"] == "loop":
                walk(s["body"], sid, "loop_back", path + [(sid, 0)])

    walk(body, "exit", "next", [])
    return out


def lower(doc):
    """The CFGs and call graph of a program, by the rules of the flow module docs."""
    subs = doc["subroutines"]
    first_sub = {}
    for i, s in enumerate(subs):
        first_sub.setdefault(s["name"], i)
    units = [(None, doc["main"])] + [(s["name"], s["body"]) for s in subs]
    cfgs, calls = [], set()
    for rank, (name, body) in enumerate(units):
        vs = visits(body)
        labels = {}
        for s, path, _, _ in vs:
            if s["kind"] == "label" and s["name"] not in labels:
                labels[s["name"]] = (s["step_id"], path)
        edges = []
        for s, path, succ, kind in vs:
            k = s["kind"]
            out = []

            def head(steps):
                return steps[0]["step_id"] if steps else succ

            if k in ("invoke", "assign", "operator", "comment", "label"):
                out.append((succ, kind))
            elif k == "if":
                out += [(head(b["body"]), "true") for b in s["branches"]]
                out.append((head(s["else_body"]), "false"))
            elif k == "loop":
                out.append((s["body"][0]["step_id"], "true") if s["body"] else (s["step_id"], "loop_back"))
                out.append((succ, "loop_exit"))
            elif k == "jump":
                t = labels.get(s["label"])
                if t is not None and path[: len(t[1])] == t[1]:
                    out.append((t[0], "jump"))
                if "cond" in s:
                    out.append((succ, "false"))
            elif k == "call":
                if s["subroutine"] in first_sub:
                    calls.add((rank, first_sub[s["subroutine"]] + 1))
                    out.append(("sub:" + s["subroutine"], "call"))
                out.append((succ, kind))
            elif k == "return":
                out.append(("exit", "return"))
            else:
                raise ValueError(k)
            edges += [{"from": s["step_id"], "kind": kd, "to": to} for to, kd in out]
        cfgs.append(
            {
                "edges": edges,
                "entry": body[0]["step_id"] if body else "exit",
                "nodes": [v[0]["step_id"] for v in vs],
                "subroutine": name,
            }
        )
    names = [None] + [s["name"] for s in subs]
    graph = [{"callee": names[b], "caller": names[a]} for a, b in sorted(calls)]
    return {"call_graph": graph, "cfgs": cfgs}


# ---------------------------------------------------------------- valid programs (AC-1)


def nested_loops():
    """Counted loops nested two deep, a while loop and an empty-bodied loop."""
    p = P(1)
    s4 = p.step("assign", var="total", expr="total + i * j")
    s3 = p.step("loop", mode="counted", var="j", **{"from": "1"}, to="i", step="1", max_iterations=10, body=[s4])
    s5 = p.step("comment", text="inner loop done")
    s2 = p.step("loop", mode="counted", var="i", **{"from": "1"}, to="3", step="1", max_iterations=10, body=[s3, s5])
    s7 = p.step("assign", var="total", expr="total + 1")
    s6 = p.step("loop", mode="while", cond="total < 100", max_iterations=5, body=[s7])
    s8 = p.step("loop", mode="while", cond="false", max_iterations=1, body=[])
    s1 = p.step("assign", var="total", expr="0")
    return p, p.doc([s1, s2, s6, s8], variables=[("total", "int")])


def else_if():
    """An if / else-if / else-if / else chain with an empty branch and a nested if."""
    p = P(2)
    s1 = p.step("assign", var="x", expr="5")
    s3 = p.step("assign", var="y", expr="0 - 1")
    s4 = p.step("assign", var="y", expr="1")
    s6 = p.step("comment", text="nested")
    s5 = p.step("if", branches=[{"body": [s6], "cond": "y == 1"}], else_body=[])
    s7 = p.step("assign", var="y", expr="2")
    branches = [{"body": [s3], "cond": "x < 0"}, {"body": [], "cond": "x == 0"}, {"body": [s4, s5], "cond": "x < 10"}]
    s2 = p.step("if", branches=branches, else_body=[s7])
    s8 = p.step("comment", text="end")
    return p, p.doc([s1, s2, s8], variables=[("x", "int"), ("y", "int")])


def early_return():
    """Early returns from a typed and a void subroutine, and a jump out of a loop."""
    p = P(3)
    c2 = p.step("return", expr="lim")
    c1 = p.step("if", branches=[{"body": [c2], "cond": "v > lim"}], else_body=[])
    c3 = p.step("return", expr="v")
    clamp = sub("clamp", [("v", QL), ("lim", QL)], [c1, c3], returns=QL)
    n2 = p.step("return")
    n1 = p.step("if", branches=[{"body": [n2], "cond": "flag"}], else_body=[])
    n3 = p.step("comment", text="not flagged")
    note = sub("note", [("flag", "bool")], [n1, n3])
    m1 = p.step("call", subroutine="clamp", args={"lim": ex("2[mm]"), "v": ex("3[mm]")}, bind="r")
    m2 = p.step("call", subroutine="note", args={"flag": lit(True)})
    m4 = p.step("jump", label="done")
    m3 = p.step("loop", mode="while", cond="true", max_iterations=3, body=[m4])
    m5 = p.step("comment", text="after the loop")
    m6 = p.step("label", name="done")
    return p, p.doc([m1, m2, m3, m5, m6], subs=[clamp, note])


def recursion():
    """fact(3): recursion to depth 3, reported once as information."""
    p = P(4)
    f2 = p.step("return", expr="1")
    f3 = p.step("call", subroutine="fact", args={"n": ex("n - 1")}, bind="m")
    f4 = p.step("return", expr="n * m")
    f1 = p.step("if", branches=[{"body": [f2], "cond": "n <= 1"}], else_body=[f3, f4])
    fact = sub("fact", [("n", "int")], [f1], returns="int")
    m1 = p.step("call", subroutine="fact", args={"n": lit(3)}, bind="r")
    p.diag("recursion", "fact", names=["fact"])
    return p, p.doc([m1], subs=[fact])


# ---------------------------------------------------------------- error paths


def jump_into_loop():
    """AC-2: a jump into a loop body it is not inside."""
    p = P(10)
    a1 = p.step("jump", label="inside", cond="true")
    a3 = p.step("label", name="inside")
    a4 = p.step("comment", text="in the loop")
    a2 = p.step("loop", mode="while", cond="true", max_iterations=2, body=[a3, a4])
    p.diag("illegal_jump", None, a1, ["inside", "into_body"])
    return p, p.doc([a1, a2])


def jump_other_sub():
    """AC-2: a jump to a label of another subroutine."""
    p = P(11)
    h1 = p.step("label", name="there")
    h2 = p.step("comment", text="helper")
    a1 = p.step("jump", label="there", cond="true")
    a2 = p.step("call", subroutine="helper", args={})
    p.diag("illegal_jump", None, a1, ["there", "other_subroutine"])
    return p, p.doc([a1, a2], subs=[sub("helper", [], [h1, h2])])


def jump_undefined():
    """AC-2: a jump to a label that does not exist."""
    p = P(12)
    a1 = p.step("jump", label="nowhere", cond="true")
    a2 = p.step("comment", text="fall through")
    p.diag("illegal_jump", None, a1, ["nowhere", "undefined"])
    return p, p.doc([a1, a2])


def jump_across_branches():
    """REQ-003: a jump in if branch 0 to a label in branch 1 enters a body it is not inside."""
    p = P(22)
    j1 = p.step("jump", label="other", cond="true")
    l1 = p.step("label", name="other")
    c1 = p.step("comment", text="branch 1")
    i1 = p.step("if", branches=[{"body": [j1], "cond": "true"}, {"body": [l1, c1], "cond": "false"}], else_body=[])
    p.diag("illegal_jump", None, j1, ["other", "into_body"])
    return p, p.doc([i1])


def loop_limit_high():
    """REQ-001: max_iterations above 1 000 000 (the format accepts it)."""
    p = P(13)
    b = p.step("comment", text="body")
    l1 = p.step("loop", mode="while", cond="true", max_iterations=2000000, body=[b])
    p.diag("bad_iteration_limit", None, l1, ["2000000"])
    return p, p.doc([l1])


def area_missing_return():
    """AC-4: area(w, h) -> area with a path that ends without return."""
    p = P(14)
    b2 = p.step("return", expr="w * h")
    b1 = p.step("if", branches=[{"body": [b2], "cond": "w > 0[mm]"}], else_body=[])
    area = sub("area", [("w", QL), ("h", QL)], [b1], returns="quantity<area>")
    m1 = p.step("call", subroutine="area", args={"h": ex("4[mm]"), "w": ex("3[mm]")}, bind="a")
    p.diag("missing_return", "area")
    return p, p.doc([m1], subs=[area])


def area_bad_call():
    """AC-4: the call area(w = 3[mm]) omits h."""
    p = P(15)
    b1 = p.step("return", expr="w * h")
    area = sub("area", [("w", QL), ("h", QL)], [b1], returns="quantity<area>")
    m1 = p.step("call", subroutine="area", args={"w": ex("3[mm]")}, bind="a")
    p.diag("bad_call", None, m1, ["h", "missing"])
    return p, p.doc([m1], subs=[area])


def bad_calls():
    """REQ-005: a wrong argument type, an extra argument, an unknown subroutine, a bad literal."""
    p = P(16)
    b1 = p.step("return", expr="w * h")
    area = sub("area", [("w", QL), ("h", QL)], [b1], returns="quantity<area>")
    g1 = p.step("comment", text="flagged")
    flagged = sub("flagged", [("flag", "bool")], [g1])
    m1 = p.step("call", subroutine="area", args={"h": ex("2[deg]"), "w": ex("3[mm]")})
    m2 = p.step("call", subroutine="area", args={"d": ex("1"), "h": ex("4[mm]"), "w": ex("3[mm]")})
    m3 = p.step("call", subroutine="nosuch", args={})
    m4 = p.step("call", subroutine="flagged", args={"flag": lit(1)})
    p.diag("bad_call", None, m1, ["h", "wrong_type"])
    p.diag("bad_call", None, m2, ["d", "extra"])
    p.diag("bad_call", None, m3, ["nosuch", "unknown_subroutine"])
    p.diag("bad_call", None, m4, ["flag", "wrong_type"])
    return p, p.doc([m1, m2, m3, m4], subs=[area, flagged])


def dead_code():
    """AC-5: the steps after an unconditional jump are unreachable."""
    p = P(17)
    d1 = p.step("assign", var="x", expr="1")
    d2 = p.step("jump", label="end")
    d3 = p.step("assign", var="x", expr="2")
    d5 = p.step("comment", text="dead branch")
    d4 = p.step("if", branches=[{"body": [d5], "cond": "x > 1"}], else_body=[])
    d6 = p.step("label", name="end")
    d7 = p.step("comment", text="reached")
    for s in (d3, d4, d5):
        p.diag("unreachable_step", None, s)
    return p, p.doc([d1, d2, d3, d4, d6, d7], variables=[("x", "int")])


def duplicate_label():
    """REQ-004: two labels share a name; the jump binds to the first."""
    p = P(18)
    l1 = p.step("label", name="again")
    l2 = p.step("comment", text="work")
    l3 = p.step("label", name="again")
    l4 = p.step("jump", label="again", cond="false")
    p.diag("duplicate_label", None, l3, ["again"])
    return p, p.doc([l1, l2, l3, l4])


def bad_condition():
    """REQ-002: conditions that are not bool and counted bounds that are not int."""
    p = P(19)
    c1 = p.step("if", branches=[{"body": [], "cond": "n + 1"}], else_body=[])
    c2 = p.step("loop", mode="while", cond="n", max_iterations=3, body=[])
    c3 = p.step("loop", mode="counted", var="i", **{"from": "0.5"}, to="n", step="1", max_iterations=5, body=[])
    c4 = p.step("jump", label="x", cond="n")
    c5 = p.step("label", name="x")
    p.diag("bad_condition", None, c1, ["branches.0.cond", "int"])
    p.diag("bad_condition", None, c2, ["cond", "int"])
    p.diag("bad_loop_bounds", None, c3, ["from", "decimal"])
    p.diag("bad_condition", None, c4, ["cond", "int"])
    return p, p.doc([c1, c2, c3, c4, c5], variables=[("n", "int")])


def bad_return():
    """REQ-006: a value from main and from a void subroutine; none, and a wrong type, from a typed one."""
    p = P(20)
    r1 = p.step("return", expr="1")
    void = sub("void_sub", [], [r1])
    r3 = p.step("return")
    r2 = p.step("if", branches=[{"body": [r3], "cond": "true"}], else_body=[])
    r4 = p.step("return", expr='"s"')
    typed = sub("typed", [], [r2, r4], returns="int")
    m1 = p.step("call", subroutine="void_sub", args={})
    m2 = p.step("call", subroutine="typed", args={}, bind="t")
    m3 = p.step("return", expr="2")
    p.diag("bad_return", None, m3, ["value_in_void"])
    p.diag("bad_return", "void_sub", r1, ["value_in_void"])
    p.diag("bad_return", "typed", r3, ["no_value"])
    p.diag("bad_return", "typed", r4, ["wrong_type"])
    return p, p.doc([m1, m2, m3], subs=[void, typed])


def mutual_recursion():
    """REQ-007/-008: ping and pong call each other; orphan is never called."""
    p = P(21)
    p2 = p.step("call", subroutine="pong", args={"n": ex("n - 1")})
    p1 = p.step("if", branches=[{"body": [p2], "cond": "n > 0"}], else_body=[])
    ping = sub("ping", [("n", "int")], [p1])
    q1 = p.step("call", subroutine="ping", args={"n": ex("n")})
    pong = sub("pong", [("n", "int")], [q1])
    o1 = p.step("comment", text="never called")
    orphan = sub("orphan", [], [o1])
    m1 = p.step("call", subroutine="ping", args={"n": lit(2)})
    p.diag("unreachable_step", "orphan", o1)
    p.diag("recursion", "ping", names=["ping", "pong"])
    return p, p.doc([m1], subs=[ping, pong, orphan])


VALID = {"nested-loops": nested_loops, "else-if": else_if, "early-return": early_return, "recursion": recursion}
ERRORS = {
    "jump-into-loop": jump_into_loop,
    "jump-other-sub": jump_other_sub,
    "jump-undefined": jump_undefined,
    "jump-across-branches": jump_across_branches,
    "loop-limit-high": loop_limit_high,
    "area-missing-return": area_missing_return,
    "area-bad-call": area_bad_call,
    "bad-calls": bad_calls,
    "dead-code": dead_code,
    "duplicate-label": duplicate_label,
    "bad-condition": bad_condition,
    "bad-return": bad_return,
    "mutual-recursion": mutual_recursion,
}


# ---------------------------------------------------------------- raw documents (AC-3)


def no_limit():
    """A loop without max_iterations."""
    p = P(30)
    b = p.step("comment", text="body")
    l1 = p.step("loop", mode="while", cond="true", body=[b])
    p.diag("missing_iteration_limit", None, l1)
    return p, p.doc([l1])


def zero_limit():
    """max_iterations 0, in a loop nested in a subroutine's if branch."""
    p = P(31)
    b = p.step("comment", text="body")
    l2 = p.step("loop", mode="while", cond="true", max_iterations=0, body=[b])
    i1 = p.step("if", branches=[{"body": [l2], "cond": "true"}], else_body=[])
    ok = p.step("loop", mode="while", cond="true", max_iterations=1000000, body=[])
    m1 = p.step("call", subroutine="s", args={})
    p.diag("bad_iteration_limit", 0, l2, ["0"])
    return p, p.doc([m1, ok], subs=[sub("s", [], [i1])])


RAW = {"no-limit": no_limit, "zero-limit": zero_limit}


def build():
    files, cases, raw = {}, {}, {}
    for name, f in list(VALID.items()) + list(ERRORS.items()):
        p, doc = f()
        rel = ("valid/%s" % name) if name in VALID else name
        files[HERE / (rel + ".json")] = canon(doc)
        cfg = lower(doc)
        if name in VALID:
            files[HERE / (rel + ".cfg.json")] = canon(cfg)
        cases[rel] = {"cfg": cfg, "diagnostics": p.diags}
    for name, f in RAW.items():
        p, doc = f()
        files[HERE / "raw" / (name + ".json")] = canon(doc)
        raw["raw/" + name] = p.diags
    expected = {
        "catalogue": "fixtures/program/types/catalogue.json",
        "cases": cases,
        "documents": raw,
        "manifest": "fixtures/manifest/prog-demo.json",
    }
    files[HERE / "expected.json"] = canon(expected)
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
