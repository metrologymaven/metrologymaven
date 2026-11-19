#!/usr/bin/env python3
"""Reference check for the CORE.DEP.PROP fixtures (CC-BY-4.0 project data).

Two jobs, both from the leaf spec's Design rules and not from the Rust code:

1. Re-derives every hand-labelled case in prop/cases.txt and fails on any difference
   (the expected lines there were written by hand; this is the independent second check).
2. Writes wide_100k/prop_expected.txt: propagation from element 0 of the CORE.DEP.EDGE
   wide_100k graph (recipe.txt), every element `current`, manual mode.

Rules (Design of spec/core/dep/prop/NODE.md):
- region = changed elements plus all their transitive dependents;
- order = Kahn's algorithm over the region, smallest ready ID first; left-over elements
  mean a cycle: walk from the smallest left-over element to its smallest left-over input
  until an element repeats; that loop, reversed into edge direction and rotated to start
  at its smallest ID, is the reported cycle;
- an element with an input in the region is downstream; it is pending unless it was
  recomputed, none of its region inputs is pending and every input outside the region is
  `current`; changed elements with no region input are never pending;
- pending elements in state current or failed are marked; the event lists them ascending;
  auto mode submits exactly the marked set (ascending) when it is non-empty.
- The order digest is SHA-256 of the RFC 8785 JSON array of the order's ID strings; the
  event digest is SHA-256 of the RFC 8785 JSON {"elements": [...]}. Both hold only ASCII
  strings, so json.dumps with sorted keys and no whitespace is exactly RFC 8785.

Stdlib only. Run:  python fixtures/core/dep/make_prop_fixtures.py [--check]
"""

import hashlib
import heapq
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
MASK = (1 << 64) - 1
MARKABLE = ("current", "failed")


class Refused(Exception):
    def __init__(self, code, reason, ids):
        super().__init__(code)
        self.code, self.reason, self.ids = code, reason, ids


def propagate(nodes, edges, states, changed, recomputed, mode):
    """nodes: set of IDs; edges: set of (input, element); IDs compare as their text."""
    outs, ins = {}, {}
    for i, e in edges:
        outs.setdefault(i, set()).add(e)
        ins.setdefault(e, set()).add(i)
    if len(set(changed)) != len(changed):
        raise Refused("duplicate_element", "duplicate_element", [min(x for x in changed if changed.count(x) > 1)])
    if len(set(recomputed)) != len(recomputed):
        raise Refused("duplicate_element", "duplicate_element", [min(x for x in recomputed if recomputed.count(x) > 1)])
    for x in sorted(changed) + sorted(recomputed):
        if x not in nodes:
            raise Refused("not_found", "unknown_element", [x])

    region = set(changed)
    grew = True
    while grew:
        new = {d for r in region for d in outs.get(r, ())} - region
        grew = bool(new)
        region |= new

    indeg = {x: len(ins.get(x, set()) & region) for x in region}
    heap = [x for x in region if indeg[x] == 0]
    heapq.heapify(heap)
    order = []
    while heap:
        x = heapq.heappop(heap)
        order.append(x)
        for d in sorted(outs.get(x, ())):
            indeg[d] -= 1
            if indeg[d] == 0:
                heapq.heappush(heap, d)
    if len(order) < len(region):
        left = {x for x in region if indeg[x] > 0}
        cur, walk = min(left), []
        while cur not in walk:
            walk.append(cur)
            cur = min(ins[cur] & left)
        loop = walk[walk.index(cur):][::-1]
        k = loop.index(min(loop))
        raise Refused("cycle", "cycle", loop[k:] + loop[:k])

    for x in sorted(recomputed):
        if x not in region:
            raise Refused("not_in_pass", "not_in_pass", [x])

    pending, pending_set = [], set()
    for x in order:
        region_inputs = ins.get(x, set()) & region
        if not region_inputs:
            continue
        stale = x not in recomputed or any(i in pending_set for i in region_inputs)
        if not stale:
            for i in sorted(ins.get(x, set()) - region):
                if i not in states:
                    raise Refused("not_found", "missing_state", [i])
                if states[i] != "current":
                    stale = True
                    break
        if stale:
            if x not in states:
                raise Refused("not_found", "missing_state", [x])
            pending.append(x)
            pending_set.add(x)
    marked = [x for x in pending if states[x] in MARKABLE]
    event = sorted(marked)
    submit = sorted(marked) if mode == "auto" else []
    return order, pending, marked, event, submit


def lines(path):
    for raw in path.read_text(encoding="utf-8").splitlines():
        if raw and not raw.startswith("#"):
            yield raw.split(" ")


def check_cases():
    ids, cases, cur = {}, [], None
    for t in lines(HERE / "prop" / "cases.txt"):
        if t[0] == "elem":
            assert t[1] not in ids, t
            ids[t[1]] = t[2]
        elif t[0] == "case":
            cur = {"name": t[1], "edges": [], "states": {}, "changed": [], "recomputed": [], "expect": {}}
        elif t[0] == "end":
            cases.append(cur)
            cur = None
        elif t[0] == "edge":
            cur["edges"].append((t[1], t[2]))
        elif t[0] == "state":
            cur["states"][t[1]] = t[2]
        elif t[0] in ("changed", "recomputed"):
            cur[t[0]] = t[1:]
        elif t[0] == "mode":
            cur["mode"] = t[1]
        elif t[0] in ("order", "pending", "marked", "event", "submit", "error"):
            assert t[0] not in cur["expect"], t
            cur["expect"][t[0]] = t[1:]
        else:
            raise SystemExit(f"unknown line {t}")
    name = {v: k for k, v in ids.items()}
    failures = 0
    for c in cases:
        edges = {(ids[i], ids[e]) for i, e in c["edges"]}
        nodes = {x for pair in edges for x in pair}
        states = {ids[k]: v for k, v in c["states"].items()}
        try:
            got = propagate(nodes, edges, states, [ids[x] for x in c["changed"]],
                            [ids[x] for x in c["recomputed"]], c["mode"])
            keys = ("order", "pending", "marked", "event", "submit")
            derived = {k: [name[x] for x in v] for k, v in zip(keys, got)}
        except Refused as r:
            derived = {"error": [r.code, r.reason] + [name[x] for x in r.ids]}
        if derived != c["expect"]:
            failures += 1
            print(f"case {c['name']}: hand-labelled {c['expect']} != reference {derived}", file=sys.stderr)
    return len(cases), failures


def wide():
    cfg = {t[0]: t[1:] for t in lines(HERE / "wide_100k" / "recipe.txt")}
    n, seed, max_in = int(cfg["elements"][0]), int(cfg["seed"][0]), int(cfg["max_inputs"][0])
    state = seed

    def nxt():
        nonlocal state
        state = (state + 0x9E3779B97F4A7C15) & MASK
        z = state
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK
        return z ^ (z >> 31)

    wid = lambda i: "00000000-0000-7000-8000-%012x" % i
    edges = set()
    for i in range(1, n):
        k = 1 + nxt() % max_in
        for j in {nxt() % i for _ in range(k)}:
            edges.add((wid(j), wid(i)))
    nodes = {wid(i) for i in range(n)}
    states = {x: "current" for x in nodes}
    order, pending, marked, event, submit = propagate(nodes, edges, states, [wid(0)], [], "manual")
    canon = lambda v: json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii")
    out = ["# Written by make_prop_fixtures.py from recipe.txt; do not edit.",
           "# CORE.DEP.PROP AC-4: changed = element 0, every element current, manual mode.",
           f"region {len(order)}",
           f"pending {len(pending)}",
           f"marked {len(marked)}",
           f"submit {len(submit)}",
           "order_digest sha256:" + hashlib.sha256(canon(order)).hexdigest(),
           "event_digest sha256:" + hashlib.sha256(canon({"elements": event})).hexdigest()]
    return "\n".join(out) + "\n"


def main():
    check = "--check" in sys.argv[1:]
    total, failures = check_cases()
    print(f"prop/cases.txt: {total} cases, {failures} differ")
    path = HERE / "wide_100k" / "prop_expected.txt"
    text = wide()
    if check:
        if not path.exists() or path.read_bytes() != text.encode("utf-8"):
            print(f"{path.name} is stale; run without --check", file=sys.stderr)
            failures += 1
    else:
        path.write_bytes(text.encode("utf-8"))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
