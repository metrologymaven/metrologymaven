#!/usr/bin/env python3
"""Reference generator for the CORE.DEP.EDGE fixtures (CC-BY-4.0 project data).

Writes chain5/expected.txt and wide_100k/expected.txt from the hand-written inputs
chain5/project.txt and wide_100k/recipe.txt, with a small dependency graph written
here from the leaf spec's rules (not by running the Rust code):

- an edge input -> element per input; queries return IDs sorted ascending;
- a commit that would close a cycle is refused with the cycle's IDs in edge direction,
  starting from the smallest ID;
- the index digest is SHA-256 of the RFC 8785 canonical JSON
  {"edges": [[input, element], ...], "elements": [...]}, both lists ascending.
  The value holds only ASCII strings, arrays and one object, so json.dumps with sorted
  keys and no whitespace is exactly RFC 8785.

Stdlib only. Run:  python fixtures/core/dep/make_fixtures.py [--check]
"""

import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
MASK = (1 << 64) - 1


class Graph:
    def __init__(self):
        self.inputs = {}  # element -> set of inputs (one entry per known element)

    def dependents_of(self, x):
        return sorted(e for e, ins in self.inputs.items() if x in ins)

    def closure(self, x, step):
        seen, todo = set(), [x]
        while todo:
            for n in step(todo.pop()):
                if n != x and n not in seen:
                    seen.add(n)
                    todo.append(n)
        return sorted(seen)

    def deps(self, x, transitive):
        if transitive:
            return self.closure(x, lambda n: sorted(self.inputs[n]))
        return sorted(self.inputs[x])

    def dependents(self, x, transitive):
        if transitive:
            return self.closure(x, self.dependents_of)
        return self.dependents_of(x)

    def cycle(self, x, new_inputs):
        """DFS from x over dependents in ascending order; the first new input reached closes a cycle."""
        if x in new_inputs:
            return [x]
        path, seen = [x], {x}

        def walk(n):
            for c in self.dependents_of(n):
                if c in new_inputs:
                    path.append(c)
                    return True
                if c not in seen:
                    seen.add(c)
                    path.append(c)
                    if walk(c):
                        return True
                    path.pop()
            return False

        if not walk(x):
            return None
        k = path.index(min(path))
        return path[k:] + path[:k]

    def set(self, x, new_inputs):
        assert all(i in self.inputs for i in new_inputs), "dangling input"
        assert self.cycle(x, set(new_inputs)) is None, "cycle"
        self.inputs[x] = set(new_inputs)

    def digest(self):
        edges = sorted([i, e] for e, ins in self.inputs.items() for i in ins)
        value = {"edges": edges, "elements": sorted(self.inputs)}
        data = json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii")
        return "sha256:" + hashlib.sha256(data).hexdigest()


def lines(path):
    for raw in path.read_text(encoding="utf-8").splitlines():
        if raw and not raw.startswith("#"):
            yield raw.split(" ")


def chain5():
    ids, g = {}, Graph()
    for t in lines(HERE / "chain5" / "project.txt"):
        if t[0] == "elem":
            ids[t[1]] = t[2]
        elif t[0] == "create":
            g.set(ids[t[1]], [ids[n] for n in t[2:]])
    name = {v: k for k, v in ids.items()}
    names = lambda xs: [name[x] for x in xs]
    out = ["# Written by make_fixtures.py from project.txt; do not edit.",
           "# query <stage> <element> <direct|transitive> <dependencies|dependents> <names...>",
           "# reject <stage> update <element> <input names...> cycle <names...>",
           "# digest <stage> <sha256>; stage load = project.txt, stage ac3 = load then `update c a`."]

    def queries(stage):
        for x in sorted(ids):
            for mode in ("direct", "transitive"):
                tr = mode == "transitive"
                out.append(" ".join(["query", stage, x, mode, "dependencies"] + names(g.deps(ids[x], tr))))
                out.append(" ".join(["query", stage, x, mode, "dependents"] + names(g.dependents(ids[x], tr))))

    queries("load")
    cyc = g.cycle(ids["a"], {ids["e"]})
    out.append(" ".join(["reject", "load", "update", "a", "e", "cycle"] + names(cyc)))
    out.append("digest load " + g.digest())
    g.set(ids["c"], [ids["a"]])
    queries("ac3")
    out.append("digest ac3 " + g.digest())
    return "\n".join(out) + "\n"


def wide():
    cfg = {t[0]: t[1:] for t in lines(HERE / "wide_100k" / "recipe.txt")}
    n, seed, max_in = int(cfg["elements"][0]), int(cfg["seed"][0]), int(cfg["max_inputs"][0])
    extra = [int(x) for x in cfg["extra"][1:]]
    assert int(cfg["extra"][0]) == len(extra)
    state = seed

    def nxt():
        nonlocal state
        state = (state + 0x9E3779B97F4A7C15) & MASK
        z = state
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK
        return z ^ (z >> 31)

    eid = lambda i: "00000000-0000-7000-8000-%012x" % i
    # Every input is older than its element, so no cycle check is needed to build it.
    edges, elements = [], [eid(0)]
    for i in range(1, n):
        k = 1 + nxt() % max_in
        ins = {nxt() % i for _ in range(k)}
        elements.append(eid(i))
        edges.extend([eid(j), eid(i)] for j in ins)

    def digest(es, els):
        value = {"edges": sorted(es), "elements": sorted(els)}
        data = json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii")
        return "sha256:" + hashlib.sha256(data).hexdigest()

    out = ["# Written by make_fixtures.py from recipe.txt; do not edit.",
           "edges %d" % len(edges),
           "digest wide " + digest(edges, elements)]
    edges.extend([eid(j), eid(n)] for j in extra)
    elements.append(eid(n))
    out.append("edges_extra %d" % len(edges))
    out.append("digest extra " + digest(edges, elements))
    return "\n".join(out) + "\n"


def main():
    check = "--check" in sys.argv[1:]
    bad = 0
    for rel, text in (("chain5/expected.txt", chain5()), ("wide_100k/expected.txt", wide())):
        path = HERE / rel
        if check:
            if not path.exists() or path.read_bytes() != text.encode("utf-8"):
                print("stale:", rel)
                bad += 1
        else:
            path.write_bytes(text.encode("utf-8"))
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
