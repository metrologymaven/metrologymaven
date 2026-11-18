#!/usr/bin/env python3
"""Generate CORE.REG.DEF.PRED fixtures (CC-BY-4.0 project data).

cases.json      AC-1/AC-3: expression, optional bound and message key, context, expected
                result or error, expected step count; one parameter schema and one set of context
                types (top level) type every case. Results come from the small reference
                evaluator below, written independently of the Rust crate from the leaf's rules:
                  * one step per AST node visited, one per list element visited by all/exists;
                  * the counter is checked before each visit: at `steps == bound` evaluation
                    stops with precondition_budget (so steps == bound);
                  * a flat chain a && b && c (or ||) is one node; && and || short-circuit
                    left to right; ?: visits one branch; all stops at the first false element,
                    exists at the first true one;
                  * has(path) costs the has node plus its argument nodes;
                  * a dotted context path (params.a.b) is one node;
                  * -0.0 becomes +0.0 when it enters (literals, context reads); ints compare
                    exactly, floats by IEEE 754 total order, int vs float by exact value; NaN in
                    any comparison is nan_operand.
bad_syntax.json AC-2: declarations that must fail the registry build, with the error code and
                the byte position, hand-labelled as the offset of a named token.
Run:  python fixtures/core/reg/def/pred/make_cases.py
"""

import json
import math
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).parent
MAX_BOUND = 10000


class Ref:
    def __init__(self, v):
        self.v = v

    def __eq__(self, o):
        return isinstance(o, Ref) and o.v == self.v


class FplError(Exception):
    def __init__(self, code):
        self.code = code


# ------------------------------------------------------------------ reference parser

KEYWORDS_OPS = ["&&", "||", "==", "!=", "<=", ">="]


def tokenize(s, i):
    toks = []
    while i < len(s):
        c = s[i]
        if c in " \t\r\n":
            i += 1
            continue
        if s[i:i + 2] in KEYWORDS_OPS:
            toks.append((s[i:i + 2], None))
            i += 2
            continue
        if c in "()[],.?:!<>":
            toks.append((c, None))
            i += 1
            continue
        if c.isdigit() or (c == "-" and i + 1 < len(s) and s[i + 1].isdigit()):
            j = i + 1
            while j < len(s) and s[j].isdigit():
                j += 1
            is_float = False
            if j < len(s) and s[j] == "." and j + 1 < len(s) and s[j + 1].isdigit():
                is_float = True
                j += 1
                while j < len(s) and s[j].isdigit():
                    j += 1
            if j < len(s) and s[j] in "eE":
                is_float = True
                j += 1
                if s[j] in "+-":
                    j += 1
                while j < len(s) and s[j].isdigit():
                    j += 1
            text = s[i:j]
            toks.append(("num", float(text) if is_float else int(text)))
            i = j
            continue
        if c in "\"'":
            j = i + 1
            out = ""
            while s[j] != c:
                if s[j] == "\\":
                    out += {"n": "\n", "t": "\t", "r": "\r"}.get(s[j + 1], s[j + 1])
                    j += 2
                else:
                    out += s[j]
                    j += 1
            toks.append(("str", out))
            i = j + 1
            continue
        if c.isalpha() or c == "_":
            j = i
            while j < len(s) and (s[j].isalnum() or s[j] == "_"):
                j += 1
            toks.append(("id", s[i:j]))
            i = j
            continue
        raise ValueError("reference tokenizer: unexpected " + c)
    toks.append(("eof", None))
    return toks


class P:
    def __init__(self, toks):
        self.t = toks
        self.i = 0
        self.vars = []

    def peek(self):
        return self.t[self.i]

    def take(self, kind=None):
        tok = self.t[self.i]
        if kind is not None and tok[0] != kind:
            raise ValueError("expected %s got %r" % (kind, tok))
        self.i += 1
        return tok

    def expr(self):
        c = self.or_()
        if self.peek()[0] == "?":
            self.take()
            a = self.expr()
            self.take(":")
            b = self.expr()
            return ("cond", c, a, b)
        return c

    def or_(self):
        items = [self.and_()]
        while self.peek()[0] == "||":
            self.take()
            items.append(self.and_())
        return items[0] if len(items) == 1 else ("or", items)

    def and_(self):
        items = [self.rel()]
        while self.peek()[0] == "&&":
            self.take()
            items.append(self.rel())
        return items[0] if len(items) == 1 else ("and", items)

    def rel(self):
        a = self.unary()
        k = self.peek()
        if k[0] in ("==", "!=", "<", "<=", ">", ">="):
            self.take()
            return ("cmp", k[0], a, self.unary())
        if k == ("id", "in"):
            self.take()
            return ("in", a, self.unary())
        return a

    def unary(self):
        if self.peek()[0] == "!":
            self.take()
            return ("not", self.unary())
        e = self.primary()
        while self.peek()[0] == ".":
            self.take()
            name = self.take("id")[1]
            if e[0] == "path":
                e = ("path", e[1] + [name])
            else:
                e = ("field", e, name)
        return e

    def primary(self):
        k, v = self.take()
        if k == "num":
            return ("lit", 0.0 if v == 0 and isinstance(v, float) else v)
        if k == "str":
            return ("lit", v)
        if k == "(":
            e = self.expr()
            self.take(")")
            return e
        if k == "[":
            items = []
            if self.peek()[0] != "]":
                items.append(self.expr())
                while self.peek()[0] == ",":
                    self.take()
                    items.append(self.expr())
            self.take("]")
            return ("list", items)
        assert k == "id", (k, v)
        if v in ("true", "false"):
            return ("lit", v == "true")
        if self.peek()[0] == "(":
            self.take()
            if v in ("all", "exists"):
                var = self.take("id")[1]
                assert self.take("id")[1] == "in"
                lst = self.expr()
                self.take(",")
                self.vars.append(var)
                body = self.expr()
                self.vars.pop()
                e = ("quant", v == "all", var, lst, body)
            else:
                e = (v, self.expr())  # size / has
            self.take(")")
            return e
        if v in self.vars:
            return ("var", v)
        return ("path", [v])


# ------------------------------------------------------------------ reference evaluator

def kind(v):
    if isinstance(v, bool):
        return "bool"
    if isinstance(v, int):
        return "int"
    if isinstance(v, float):
        return "float"
    if isinstance(v, str):
        return "string"
    if isinstance(v, Ref):
        return "ref"
    if isinstance(v, list):
        return "list"
    return "map"


def fcmp(x, y):
    if x < y:
        return -1
    if x > y:
        return 1
    sx, sy = math.copysign(1, x), math.copysign(1, y)  # total order: -0.0 < +0.0
    return (sx > sy) - (sx < sy)


def numcmp(a, b):
    ka, kb = kind(a), kind(b)
    if ka not in ("int", "float") or kb not in ("int", "float"):
        return None
    if (ka == "float" and math.isnan(a)) or (kb == "float" and math.isnan(b)):
        raise FplError("nan_operand")
    if ka == "int" and kb == "int":
        return (a > b) - (a < b)
    if ka == "float" and kb == "float":
        return fcmp(a, b)
    fa = Fraction(a) if not (ka == "float" and math.isinf(a)) else a
    fb = Fraction(b) if not (kb == "float" and math.isinf(b)) else b
    return (fa > fb) - (fa < fb)


def equals(a, b):
    r = numcmp(a, b)
    if r is not None:
        return r == 0
    ka, kb = kind(a), kind(b)
    if ka == kb and ka in ("bool", "string", "ref"):
        return a == b
    raise FplError("type_error")


def order(a, b):
    r = numcmp(a, b)
    if r is not None:
        return r
    if kind(a) == "string" and kind(b) == "string":
        return (a > b) - (a < b)
    raise FplError("type_error")


class Ev:
    def __init__(self, ctx, bound):
        self.ctx, self.bound, self.steps, self.env = ctx, bound, 0, []

    def tick(self):
        if self.steps >= self.bound:
            raise FplError("precondition_budget")
        self.steps += 1

    def get(self, path):
        cur = self.ctx
        for seg in path:
            if not isinstance(cur, dict) or seg not in cur:
                raise FplError("missing_field")
            cur = cur[seg]
        return norm(cur)

    def want_bool(self, n):
        v = self.ev(n)
        if kind(v) != "bool":
            raise FplError("type_error")
        return v

    def ev(self, n):
        self.tick()
        t = n[0]
        if t == "lit":
            return n[1]
        if t == "list":
            return [self.ev(x) for x in n[1]]
        if t == "path":
            return self.get(n[1])
        if t == "var":
            for name, v in reversed(self.env):
                if name == n[1]:
                    return v
            raise AssertionError
        if t == "field":
            b = self.ev(n[1])
            if kind(b) != "map":
                raise FplError("type_error")
            if n[2] not in b:
                raise FplError("missing_field")
            return b[n[2]]
        if t == "not":
            return not self.want_bool(n[1])
        if t in ("and", "or"):
            stop = t == "or"  # value that ends the chain early
            for x in n[1]:
                if self.want_bool(x) == stop:
                    return stop
            return not stop
        if t == "cmp":
            a, b = self.ev(n[2]), self.ev(n[3])
            op = n[1]
            if op == "==":
                return equals(a, b)
            if op == "!=":
                return not equals(a, b)
            r = order(a, b)
            return {"<": r < 0, "<=": r <= 0, ">": r > 0, ">=": r >= 0}[op]
        if t == "in":
            a, b = self.ev(n[1]), self.ev(n[2])
            if kind(b) == "list":
                for x in b:
                    if equals(a, x):
                        return True
                return False
            if kind(b) == "map" and kind(a) == "string":
                return a in b
            raise FplError("type_error")
        if t == "cond":
            return self.ev(n[2]) if self.want_bool(n[1]) else self.ev(n[3])
        if t == "size":
            v = self.ev(n[1])
            if kind(v) not in ("list", "map", "string"):
                raise FplError("type_error")
            return len(v)
        if t == "has":
            arg = n[1]
            self.tick()
            if arg[0] == "path":
                try:
                    self.get(arg[1])
                    return True
                except FplError as e:
                    if e.code == "missing_field":
                        return False
                    raise
            b = self.ev(arg[1])
            if kind(b) != "map":
                raise FplError("type_error")
            return arg[2] in b
        if t == "quant":
            _, is_all, var, lst, body = n
            items = self.ev(lst)
            if kind(items) != "list":
                raise FplError("type_error")
            for it in items:
                self.tick()
                self.env.append((var, it))
                b = self.want_bool(body)
                self.env.pop()
                if b != is_all:
                    return b
            return is_all
        raise AssertionError(t)


def norm(v):
    """-0.0 -> +0.0, deep (fixture values are shallow, so recursion is fine here)."""
    if isinstance(v, float) and v == 0:
        return 0.0
    if isinstance(v, list):
        return [norm(x) for x in v]
    if isinstance(v, dict):
        return {k: norm(x) for k, x in v.items()}
    return v


def run(decl, ctx, bound=None, msg=None):
    bound = bound or MAX_BOUND
    ast = P(tokenize(decl, 0)).expr()
    ev = Ev(ctx, bound)
    try:
        ok = ev.want_bool(ast)
        res = {"result": True} if ok else {"result": False, "message_key": msg or "test.precondition_0"}
    except FplError as e:
        res = {"error": e.code}
    res["steps"] = ev.steps
    return res


# ------------------------------------------------------------------ encoding

def enc(v):
    if isinstance(v, bool) or isinstance(v, str):
        return v
    if isinstance(v, int):
        return v if abs(v) <= 2 ** 53 else {"$int": str(v)}
    if isinstance(v, float):
        return {"$float": repr(v)}
    if isinstance(v, Ref):
        return {"$ref": v.v}
    if isinstance(v, list):
        return [enc(x) for x in v]
    return {k: enc(x) for k, x in v.items()}


SCHEMA = {
    "type": "object",
    "properties": {
        "n": {"type": "integer"},
        "x": {"type": "number"},
        "name": {"type": "string"},
        "flag": {"type": "boolean"},
        "opt": {"type": "string"},
        "ids": {"type": "array", "items": {"type": "string"}},
        "nums": {"type": "array", "items": {"type": "integer"}},
        "fs": {"type": "array", "items": {"type": "number"}},
        "feature": {"type": "string", "x-ref": "feature"},
        "items": {"type": "array", "items": {"type": "object", "properties": {
            "kind": {"type": "string"}, "n": {"type": "integer"}}}},
        "tags": {"type": "object"},
        "cfg": {"type": "object", "properties": {"mode": {"type": "string"}, "limit": {"type": "number"}}},
        "load": {"type": "integer"},
        "repeat": {"type": "boolean"},
    },
}
TYPES = {
    "doc": "map", "doc.count": "int", "doc.mode": "string", "doc.active": "ref", "doc.tol": "float",
    "doc.points": "list<map>", "doc.points[].x": "float", "doc.points[].id": "ref",
    "doc.meta": "map", "doc.meta.owner": "string", "doc.flags": "list<bool>", "doc.grid": "list<list<int>>",
    "doc.now": "int",
}


def base_params():
    return {"n": 5, "x": 2.5, "name": "probe", "flag": True, "ids": ["a", "b", "c"], "nums": [1, 2, 3],
            "fs": [0.5, 1.5], "feature": Ref("F1"),
            "items": [{"kind": "plane", "n": 3}, {"kind": "circle", "n": 1}],
            "tags": {"a": "1", "b": "2"}, "cfg": {"mode": "fast", "limit": 2.0},
            "load": 4, "repeat": True}


def base_doc():
    return {"count": 3, "mode": "edit", "active": Ref("F1"), "tol": 0.01,
            "points": [{"x": 0.0, "id": Ref("P1")}, {"x": 1.0, "id": Ref("P2")}],
            "meta": {"owner": "qa"}, "flags": [True, False], "grid": [[1, 2], [3]], "now": 7}


cases = []


def case(name, expr, params=None, doc=None, **kw):
    p = base_params()
    p.update(params or {})
    d = base_doc()
    d.update(doc or {})
    for k in kw.get("drop_doc", []):
        d.pop(k)
    for k in kw.get("drop_params", []):
        p.pop(k, None)
    ctx = {"params": p, "doc": d}
    expect = run(expr, ctx, kw.get("bound"), kw.get("message_key"))
    if "want" in kw:  # hand-stated result, cross-checks the reference evaluator
        got = expect.get("error", expect.get("result"))
        assert got == kw["want"], (name, expect, kw["want"])
    c = {"name": name, "expr": expr}
    for k in ("bound", "message_key"):
        if k in kw:
            c[k] = kw[k]
    c.update(context=enc(ctx), expect=expect)
    cases.append(c)


NAN = float("nan")
case("lit_true", "true", want=True)
case("lit_false", "false", want=False)
case("not_false", "!false", want=True)
case("int_eq", "params.n == 5", want=True)
case("int_ne", "params.n != 5", want=False)
case("range_and", "params.n > 4 && params.n < 6", want=True)
case("and_short_circuit", "params.n < 4 && params.n > 0", want=False)
case("or_rhs", "params.n < 4 || params.flag", want=True)
case("or_short_circuit", "params.flag || params.n < 4", want=True)
case("float_ge", "params.x >= 2.5", want=True)
case("mixed_gt", "params.x > 2", want=True)
case("mixed_eq", "params.n == 5.0", want=True)
case("mixed_lt", "params.n < 5.5", want=True)
case("negative_literal", "params.n > -5 && -2.5 < params.x", want=True)
case("exponent_literal", "params.x < 1e3", want=True)
case("string_eq", "params.name == \"probe\"", want=True)
case("string_single_quote", "params.name == 'probe'", want=True)
case("string_escape", "params.name == \"a\\\"b\"", params={"name": "a\"b"}, want=True)
case("string_order", "params.name < \"q\"", want=True)
case("string_order_case", "\"Probe\" < params.name", want=True)
case("size_list", "size(params.ids) == 3", want=True)
case("size_string", "size(params.name) == 5", want=True)
case("size_map", "size(params.tags) == 2", want=True)
case("size_unicode", "size(\"héllo\") == 5", want=True)
case("size_empty", "size(params.ids) == 0", params={"ids": []}, want=True)
case("in_list_true", "\"b\" in params.ids", want=True)
case("in_list_false", "\"z\" in params.ids", want=False)
case("in_int_list", "2 in params.nums", want=True)
case("in_mixed", "2.0 in params.nums", want=True)
case("in_map_key", "\"a\" in params.tags", want=True)
case("in_literal_list", "params.n in [1, 3, 5]", want=True)
case("in_mixed_literal", "params.x in [1, 2.5]", want=True)
case("in_doc_mode", "doc.mode in [\"edit\", \"view\"]", want=True)
case("in_ref_list", "doc.active in [params.feature]", want=True)
case("has_absent", "has(params.opt)", drop_params=["opt"], want=False)
case("has_present", "has(params.name)", want=True)
case("has_guard", "has(params.opt) ? params.opt == \"x\" : true", want=True)
case("has_nested", "has(params.cfg.limit)", want=True)
case("has_nested_absent", "has(doc.meta.owner)", doc={"meta": {}}, want=False)
case("cond_numeric_mix", "(params.flag ? params.n : params.x) > 4", want=True)
case("cond_else", "(!params.flag ? params.n : params.x) > 4", want=False)
case("cond_nested", "params.n > 3 ? params.n > 4 ? \"big\" == \"big\" : false : false", want=True)
case("cond_literal", "false ? 1 > 2 : !false", want=True)
case("precedence_or_and", "true || false && false", want=True)
case("precedence_parens", "(true || false) && false", want=False)
case("not_or", "!(params.n > 3) || params.name != \"x\"", want=True)
case("nested_field", "params.cfg.mode == \"fast\" && params.cfg.limit > 1", want=True)
case("all_true", "all(i in params.ids, size(i) == 1)", want=True)
case("exists_true", "exists(i in params.ids, i == \"c\")", want=True)
case("exists_false", "exists(i in params.ids, i == \"q\")", want=False)
case("all_items_field", "all(it in params.items, it.n >= 1)", want=True)
case("exists_items_field", "exists(it in params.items, it.kind == \"circle\")", want=True)
case("all_points", "all(p in doc.points, p.x >= 0.0)", want=True)
case("exists_ref", "exists(p in doc.points, p.id == doc.active)", want=False)
case("ref_eq", "doc.active == params.feature", want=True)
case("nested_quant", "all(r in doc.grid, all(v in r, v > 0))", want=True)
case("exists_in_row", "exists(r in doc.grid, 3 in r)", want=True)
case("exists_bool", "exists(f in doc.flags, f)", want=True)
case("all_bool", "all(f in doc.flags, f)", want=False)
case("all_empty", "all(i in params.ids, false)", params={"ids": []}, want=True)
case("exists_empty", "exists(i in params.ids, true)", params={"ids": []}, want=False)
case("has_var_field", "all(it in params.items, has(it.n))",
     params={"items": [{"kind": "plane", "n": 3}, {"kind": "circle"}]}, want=False)
case("doc_guard", "has(doc.meta.owner) && doc.meta.owner == \"qa\"", want=True)
case("doc_and", "doc.count >= 3 && doc.mode != \"locked\"", want=True)
case("message_key", "size(params.ids) >= 4", message_key="align.needs_four", want=False)
case("bound_exact", "params.n == 5 && params.flag", bound=5, want=True)
case("bound_one_short", "params.n == 5 && params.flag", bound=4, want="precondition_budget")
case("bound_quant", "all(i in params.ids, true)", bound=3, want="precondition_budget")
case("bound_and_message", "doc.count > 5", bound=100, message_key="doc.too_few", want=False)
case("flat_and_chain_70", " && ".join(["params.flag"] * 70), want=True)
assert cases[-1]["expect"]["steps"] == 71
case("flat_or_chain_short", " || ".join(["params.n > 4"] * 70), want=True)
assert cases[-1]["expect"]["steps"] == 4
case("keyword_field_load", "params.load > 0", want=True)
case("keyword_field_repeat", "params.repeat", want=True)
case("keyword_field_now", "doc.now == 7", want=True)
case("nan_lt", "doc.tol < 1.0", doc={"tol": NAN}, want="nan_operand")
case("nan_self_eq", "doc.tol == doc.tol", doc={"tol": NAN}, want="nan_operand")
case("nan_mixed", "params.n == doc.tol", doc={"tol": NAN}, want="nan_operand")
case("nan_in_list", "1.0 in params.fs", params={"fs": [NAN, 1.0]}, want="nan_operand")
case("nan_unvisited", "params.flag || doc.tol < 1.0", doc={"tol": NAN}, want=True)
case("neg_zero_read_eq", "params.x == 0.0", params={"x": -0.0}, want=True)
case("neg_zero_read_not_lt", "params.x < 0.0", params={"x": -0.0}, want=False)
case("neg_zero_read_ge", "doc.tol >= 0.0", doc={"tol": -0.0}, want=True)
case("neg_zero_literal_eq", "0.0 == -0.0", want=True)
case("neg_zero_literal_not_lt", "-0.0 < 0.0", want=False)
case("neg_zero_in_list", "0.0 in params.fs", params={"fs": [1.0, -0.0]}, want=True)
case("neg_zero_vs_int", "0 == params.x", params={"x": -0.0}, want=True)
case("total_order_tiny", "params.x > 0.0", params={"x": 5e-324}, want=True)
case("int_max_vs_float", "params.n < 9223372036854775807.0", params={"n": 2 ** 63 - 1}, want=True)
case("exact_big_int_ne", "params.n == 9007199254740993.0", params={"n": 2 ** 53 + 1}, want=False)
case("exact_big_int_gt", "params.n > 9007199254740992.0", params={"n": 2 ** 53 + 1}, want=True)
case("missing_doc_field", "doc.count > 1", drop_doc=["count"], want="missing_field")
case("missing_item_field", "exists(it in params.items, it.n > 3)",
     params={"items": [{"kind": "plane", "n": 3}, {"kind": "circle"}]}, want="missing_field")
case("missing_unvisited_cond", "params.flag ? true : doc.count > 1", drop_doc=["count"], want=True)
case("missing_unvisited_and", "!params.flag && doc.count > 1", drop_doc=["count"], want=False)
case("runtime_type_mismatch", "doc.mode == \"edit\"", doc={"mode": 7}, want="type_error")
case("runtime_not_bool", "!params.flag", params={"flag": "yes"}, want="type_error")
case("all_stops_at_false", "all(i in params.nums, i < 2)", want=False)
assert cases[-1]["expect"]["steps"] == 2 + 2 * 4
case("exists_stops_at_true", "exists(i in params.nums, i == 1)", want=True)
assert cases[-1]["expect"]["steps"] == 2 + 4
case("exists_stops_before_error", "exists(it in params.items, it.n > 2)",
     params={"items": [{"kind": "plane", "n": 3}, {"kind": "circle"}]}, want=True)
case("all_stops_before_error", "all(it in params.items, it.n > 3)",
     params={"items": [{"kind": "plane", "n": 3}, {"kind": "circle"}]}, want=False)
case("membership_small", "all(x in params.ids, exists(y in params.ids, y == x))",
     params={"ids": ["i%d" % k for k in range(10)]}, want=True)
# 2 + sum over k of (3 + 4 * (k + 1)) for k < 10
assert cases[-1]["expect"]["steps"] == 2 + sum(3 + 4 * (k + 1) for k in range(10))
case("ac3_budget", "all(x in params.ids, exists(y in params.ids, y == x))",
     params={"ids": ["id%04d" % k for k in range(1000)]}, want="precondition_budget")
assert cases[-1]["expect"]["steps"] == 10000

# ------------------------------------------------------------------ AC-2: build failures

bad = []


def bad_case(name, decl, code, needle, nth=1):
    pos = -1
    for _ in range(nth):
        pos = decl.index(needle, pos + 1)
    bad.append({"name": name, "expr": decl, "code": code, "pos": pos})


bad_case("loop_while", "while params.n > 0", "fpl_syntax", "while")
bad_case("loop_for", "params.flag && for(i in params.ids, true)", "fpl_syntax", "for")
bad_case("user_function", "my_check(params.n)", "fpl_syntax", "my_check")
bad_case("user_function_inner", "params.n > 0 && double(params.n) > 2", "fpl_syntax", "double")
bad_case("method_call", "params.ids.contains(\"a\")", "fpl_syntax", "contains")
bad_case("lambda", "lambda x: x", "fpl_syntax", "lambda")
bad_case("assignment", "params.n = 5", "fpl_syntax", "=")
bad_case("arithmetic", "params.n + 1 > 2", "fpl_syntax", "+")
bad_case("chained_comparison", "params.n > 1 > 0", "fpl_syntax", ">", 2)
bad_case("unterminated_string", "params.name == \"abc", "fpl_syntax", "\"")
bad_case("annotation_char", "@bound(5) true", "fpl_syntax", "@")
bad_case("keyword_root", "now > 3", "fpl_syntax", "now")
bad_case("keyword_variable", "all(for in params.ids, true)", "fpl_syntax", "for")
bad_case("int_overflow", "params.n > 99999999999999999999", "fpl_syntax", "9")
bad_case("has_literal", "has(1)", "fpl_syntax", "1")
bad_case("rebound_var", "all(x in params.ids, all(x in params.ids, true))", "fpl_syntax", "x", 2)
bad_case("stray_char", "params.n > 1 # c", "fpl_syntax", "#")
bad_case("depth_not", "!" * 70 + "true", "fpl_syntax", "!", 64)
bad_case("depth_parens", "(" * 70 + "true" + ")" * 70, "fpl_syntax", "(", 65)
bad_case("depth_nested_and", "true && (" * 70 + "true" + ")" * 70, "fpl_syntax", "true", 65)
bad_case("unknown_param", "params.nope == 1", "fpl_type", "params")
bad_case("unknown_doc_field", "doc.count > 0 && doc.cnt > 0", "fpl_type", "doc.cnt")
bad_case("unknown_item_field", "all(it in params.items, it.colour == \"red\")", "fpl_type", "colour")
bad_case("untyped_map_field", "params.tags.a == \"1\"", "fpl_type", "params")
bad_case("mismatch_int_string", "params.n == \"5\"", "fpl_type", "==")
bad_case("mismatch_order", "params.name > 3", "fpl_type", ">")
bad_case("mismatch_size", "size(params.n) > 0", "fpl_type", "size")
bad_case("mismatch_and", "params.n && true", "fpl_type", "params")
bad_case("mismatch_not", "!params.name", "fpl_type", "params")
bad_case("mismatch_branches", "params.flag ? 1 : \"a\"", "fpl_type", "?")
bad_case("mismatch_list_eq", "params.ids == params.ids", "fpl_type", "==")
bad_case("mismatch_quant", "all(i in params.n, true)", "fpl_type", "all")
bad_case("mismatch_ref_string", "params.feature == \"F1\"", "fpl_type", "==")
bad_case("not_bool", "params.n", "fpl_type", "params")
bad_case("empty_list", "params.n in []", "fpl_type", "[")

HEADER = {"_license": "CC-BY-4.0", "_generator": "make_cases.py"}
(HERE / "cases.json").write_text(json.dumps(dict(HEADER, params_schema=SCHEMA, types=TYPES, cases=cases), indent=1, ensure_ascii=False) + "\n",
                                 encoding="utf-8", newline="\n")
(HERE / "bad_syntax.json").write_text(
    json.dumps(dict(HEADER, params_schema=SCHEMA, types=TYPES, cases=bad), indent=1, ensure_ascii=False) + "\n",
    encoding="utf-8", newline="\n")
print(len(cases), "cases,", len(bad), "bad declarations")
