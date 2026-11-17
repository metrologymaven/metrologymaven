#!/usr/bin/env python3
"""scope.py: validate and report on the recursive spec tree.

Standard library only, so any contributor or CI runner can use it.

    python tools/scope.py check              validate tree + coverage, regenerate generated/*
    python tools/scope.py open [--level N]   list open nodes (shallowest open level by default)
    python tools/scope.py stats              counts by level and status

Exit code 1 means there are errors. Warnings never fail the run.
"""
from __future__ import annotations

import argparse
import fnmatch
import re
import sys
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SPEC = ROOT / "spec"
SEEDS = ROOT / "seeds"
DECISIONS = ROOT / "decisions"
GEN = ROOT / "generated"

STATUSES = {"open", "split", "leaf", "decision", "frozen"}
KINDS = {"product", "domain", "subsystem", "module", "feature", "task"}
PHASES = {"P0", "P1a", "P1b", "P2", "P3", "P4", "later"}
SIZES = {"S", "M", "L", "XL", "XXL"}
LEAF_SIZES = {"S", "M"}
SIDE_EFFECTS = {"R0", "R1", "W1", "W2", "W3", "X1", "I1", "D1", "M1", "S1", "A0"}
DETERMINISM = {"D0", "D1", "D2", "D3", "N"}
REQUIRED = ["id", "title", "level", "parent", "kind", "status", "phase", "size"]
LIST_FIELDS = {"covers", "depends_on", "uses_artifacts", "satisfies", "cli", "mcp", "journal", "owners"}
SECTIONS_ALL = ["## Purpose", "## Scope", "## Requirements", "## Children"]
SECTIONS_LEAF = ["## Acceptance criteria", "## Design", "## Build prompt"]
MAX_DEPTH = 7
WINDOWS_RESERVED = {"con", "prn", "aux", "nul"} | {f"com{i}" for i in range(1, 10)} | {f"lpt{i}" for i in range(1, 10)}

ID_RE = re.compile(r"^[A-Z][A-Z0-9]*(\.[A-Z][A-Z0-9]{1,7})*$")
REQ_RE = re.compile(r"^\s*-\s+\*\*(REQ-[A-Z0-9.]+-\d{3})\*\*")
SEED_RE = re.compile(r"^\|\s*([A-Z]+\.[A-Z]+\.\d{2,3})\s*\|\s*([^|]*)\|")
OOS_RE = re.compile(r"^\|\s*`?([A-Z][A-Z0-9*.]*)`?\s*\|\s*([^|]*)\|")


class Report:
    def __init__(self) -> None:
        self.errors: list[str] = []
        self.warnings: list[str] = []

    def err(self, where: str, msg: str) -> None:
        self.errors.append(f"ERROR {where}: {msg}")

    def warn(self, where: str, msg: str) -> None:
        self.warnings.append(f"warn  {where}: {msg}")


def rel(p: Path) -> str:
    return p.relative_to(ROOT).as_posix()


def parse_front(text: str):
    lines = text.splitlines()
    if not lines or lines[0].strip() != "---":
        return None, "missing frontmatter"
    meta: dict = {}
    for i, line in enumerate(lines[1:], start=1):
        if line.strip() == "---":
            return meta, "\n".join(lines[i + 1:])
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        if ":" not in line:
            return None, f"bad frontmatter line: {line!r}"
        key, val = line.split(":", 1)
        key, val = key.strip(), val.strip()
        if val.startswith("[") and val.endswith("]"):
            inner = val[1:-1].strip()
            meta[key] = [x.strip().strip("'\"") for x in inner.split(",") if x.strip()] if inner else []
        else:
            meta[key] = val.strip("'\"")
    return None, "unterminated frontmatter"


class Node:
    def __init__(self, path: Path, meta: dict, body: str) -> None:
        self.path = path
        self.meta = meta
        self.body = body
        self.id = meta.get("id", "")
        self.parent = meta.get("parent", "")
        self.status = meta.get("status", "")
        try:
            self.level = int(meta.get("level", "-1"))
        except ValueError:
            self.level = -1
        self.reqs = [m.group(1) for line in body.splitlines() if (m := REQ_RE.match(line))]
        self.children: list[Node] = []

    def lst(self, key: str) -> list[str]:
        v = self.meta.get(key, [])
        return v if isinstance(v, list) else []

    @property
    def where(self) -> str:
        return rel(self.path)


def load_seeds(rep: Report):
    seeds: dict[str, tuple[str, str]] = {}
    oos: list[tuple[str, str]] = []
    if not SEEDS.exists():
        return seeds, oos
    for f in sorted(SEEDS.glob("*.md")):
        if f.name.lower() == "readme.md":
            continue
        text = f.read_text(encoding="utf-8")
        if f.name == "out-of-scope.md":
            for line in text.splitlines():
                m = OOS_RE.match(line)
                if m and "." in m.group(1):
                    oos.append((m.group(1), m.group(2).strip()))
            continue
        for line in text.splitlines():
            m = SEED_RE.match(line)
            if m:
                sid = m.group(1)
                if sid in seeds:
                    rep.err(rel(f), f"duplicate seed id {sid} (also in {seeds[sid][1]})")
                seeds[sid] = (m.group(2).strip(), f.name)
    return seeds, oos


def matches(patterns: list[str], sid: str) -> bool:
    return any(fnmatch.fnmatchcase(sid, p) for p in patterns)


def load_nodes(rep: Report) -> list[Node]:
    nodes = []
    for p in sorted(SPEC.rglob("NODE.md")):
        meta, body = parse_front(p.read_text(encoding="utf-8"))
        if meta is None:
            rep.err(rel(p), body)
            continue
        nodes.append(Node(p, meta, body))
    return nodes


def validate(nodes: list[Node], seeds, oos, rep: Report) -> dict[str, Node]:
    by_id: dict[str, Node] = {}
    for n in nodes:
        if n.id in by_id:
            rep.err(n.where, f"duplicate id {n.id} (also {by_id[n.id].where})")
        by_id[n.id] = n

    by_folder = {n.path.parent: n for n in nodes}
    all_reqs: dict[str, str] = {}
    seed_ids = list(seeds)

    for n in nodes:
        w, m = n.where, n.meta
        for k in REQUIRED:
            if k not in m or (m[k] == "" and k != "parent"):
                rep.err(w, f"missing field '{k}'")
        for k in LIST_FIELDS:
            if k in m and not isinstance(m[k], list):
                rep.err(w, f"field '{k}' must be a [list]")
        if n.id and not ID_RE.match(n.id):
            rep.err(w, f"bad id '{n.id}' (dotted uppercase, segments 2-8 chars after the first)")
        if n.id and n.id.split(".")[-1].lower() in WINDOWS_RESERVED:
            rep.err(w, f"id segment '{n.id.split('.')[-1]}' is a reserved Windows device name; folders "
                       "with that name cannot be checked out on Windows")
        if m.get("status") not in STATUSES:
            rep.err(w, f"status must be one of {sorted(STATUSES)}")
        if m.get("kind") not in KINDS:
            rep.err(w, f"kind must be one of {sorted(KINDS)}")
        if m.get("phase") not in PHASES:
            rep.err(w, f"phase must be one of {sorted(PHASES)}")
        if m.get("size") not in SIZES:
            rep.err(w, f"size must be one of {sorted(SIZES)}")

        depth = len(n.path.parent.relative_to(SPEC).parts)
        if n.level != depth:
            rep.err(w, f"level {n.level} but folder depth is {depth}")
        if n.level > MAX_DEPTH:
            rep.err(w, f"deeper than max depth {MAX_DEPTH}")
        if depth == 0:
            if n.parent:
                rep.err(w, "root node must have an empty parent")
        else:
            seg = n.id.split(".")[-1].lower() if n.id else ""
            if n.path.parent.name != seg:
                rep.err(w, f"folder '{n.path.parent.name}' should be '{seg}' (last id segment, lowercase)")
            pnode = by_folder.get(n.path.parent.parent)
            if pnode is None:
                rep.err(w, "no NODE.md in parent folder")
            else:
                if n.parent != pnode.id:
                    rep.err(w, f"parent '{n.parent}' but parent folder holds '{pnode.id}'")
                if n.level >= 2 and not n.id.startswith(pnode.id + "."):
                    rep.err(w, f"id must start with '{pnode.id}.'")
                if n.level == 1 and "." in n.id:
                    rep.err(w, "level-1 ids have no dots")
                pnode.children.append(n)

        for s in SECTIONS_ALL:
            if s not in n.body:
                rep.err(w, f"missing section '{s}'")
        if not n.reqs:
            rep.err(w, "no requirements (lines like '- **REQ-<ID>-001** ...')")
        for r in n.reqs:
            if not r.startswith(f"REQ-{n.id}-"):
                rep.err(w, f"requirement {r} must start with REQ-{n.id}-")
            if r in all_reqs:
                rep.err(w, f"duplicate requirement {r} (also {all_reqs[r]})")
            all_reqs[r] = w

        for d in n.lst("depends_on"):
            if d == n.id:
                rep.err(w, "depends on itself")
            elif d not in by_id:
                rep.warn(w, f"depends_on '{d}' does not exist yet")
        # Artifact dependencies (D-034): reads a published file or format; not a code edge, not in cycles.
        for d in n.lst("uses_artifacts"):
            if d not in by_id:
                rep.warn(w, f"uses_artifacts '{d}' does not exist yet")

        for p in n.lst("covers"):
            if seed_ids and not any(fnmatch.fnmatchcase(s, p) for s in seed_ids):
                rep.warn(w, f"covers pattern '{p}' matches no seed row")

        st = n.status
        if st == "leaf":
            if m.get("size") not in LEAF_SIZES:
                rep.err(w, "a leaf must be size S or M; otherwise split it")
            if m.get("kind") not in {"feature", "task"}:
                rep.err(w, "a leaf must be kind feature or task")
            for s in SECTIONS_LEAF:
                if s not in n.body:
                    rep.err(w, f"leaf missing section '{s}'")
            se = m.get("side_effect")
            se_list = se if isinstance(se, list) else [se]
            if not se_list or any(s not in SIDE_EFFECTS for s in se_list):
                rep.err(w, f"leaf needs side_effect in {sorted(SIDE_EFFECTS)} "
                           "(one value, or a [list] with one class per command in cli/mcp order)")
            if m.get("determinism") not in DETERMINISM:
                rep.err(w, f"leaf needs determinism in {sorted(DETERMINISM)}")
            if m.get("kind") == "feature":
                for k in ("cli", "mcp", "journal"):
                    if not n.lst(k):
                        rep.err(w, f"feature leaf needs '{k}' (CONSTITUTION C3/C4)")
        elif st in {"open", "split", "frozen"} and m.get("size") in LEAF_SIZES and n.level > 0:
            rep.warn(w, f"size {m.get('size')} on a non-leaf: finish it as a leaf?")
        if st == "decision":
            d = m.get("decision", "")
            if not d or not list(DECISIONS.glob(f"{d}*.md")):
                rep.err(w, "status decision needs 'decision: D-NNN' with a file in decisions/")

    # Parent-child traceability.
    for n in nodes:
        w = n.where
        if n.children and n.status not in {"split", "frozen"}:
            rep.err(w, f"has {len(n.children)} children but status is '{n.status}' (should be split)")
        if n.status == "split":
            if len(n.children) < 2:
                rep.err(w, "split node needs at least 2 children")
            if len(n.children) > 9 and n.level > 0:
                rep.warn(w, f"{len(n.children)} children; consider an intermediate level")
            satisfied = {r for c in n.children for r in c.lst("satisfies")}
            for r in n.reqs:
                if r not in satisfied:
                    rep.err(w, f"requirement {r} is not satisfied by any child")
            parent_seeds = [s for s in seed_ids if matches(n.lst("covers"), s)]
            for s in parent_seeds:
                if not any(matches(c.lst("covers"), s) for c in n.children):
                    rep.err(w, f"covered seed {s} dropped by all children")
        if n.level >= 1:
            pnode = by_id.get(n.parent)
            if pnode is not None:
                for r in n.lst("satisfies"):
                    if r not in pnode.reqs:
                        rep.err(w, f"satisfies {r}, which is not a requirement of parent {pnode.id}")
                if not n.lst("satisfies"):
                    rep.err(w, "satisfies no parent requirement")
                if n.level >= 2:
                    for s in seed_ids:
                        if matches(n.lst("covers"), s) and not matches(pnode.lst("covers"), s):
                            rep.warn(w, f"covers {s}, which parent {pnode.id} does not cover")

    # Phase order: a node's phase is when work on it starts, so no child may start before its parent.
    order = {p: i for i, p in enumerate(["P0", "P1a", "P1b", "P2", "P3", "P4", "later"])}
    for n in nodes:
        pnode = by_id.get(n.parent)
        if pnode is not None and n.meta.get("phase") in order and pnode.meta.get("phase") in order:
            if order[n.meta["phase"]] < order[pnode.meta["phase"]]:
                rep.warn(n.where, f"phase {n.meta['phase']} is earlier than parent {pnode.id} "
                                  f"({pnode.meta['phase']}); move the parent earlier by decision")

    # Dependency cycles. A dependency on a node also reaches all its descendants.
    edges: dict[str, set[str]] = defaultdict(set)
    for n in nodes:
        for d in n.lst("depends_on"):
            if d in by_id and d != n.id:
                edges[n.id].add(d)
        for c in n.children:
            edges[n.id].add(c.id)
    for cycle in find_cycles(edges):
        rep.err("dependencies", "cycle: " + " -> ".join(cycle + [cycle[0]]))

    # Global coverage.
    oos_patterns = [p for p, _ in oos]
    for s in seed_ids:
        if matches(oos_patterns, s):
            continue
        if not any(n.level >= 1 and matches(n.lst("covers"), s) for n in nodes):
            rep.err("coverage", f"seed {s} ({seeds[s][0]}) is not covered by any node or out-of-scope row")
    return by_id


def find_cycles(edges: dict[str, set[str]]) -> list[list[str]]:
    """Strongly connected components with more than one node (Tarjan, iterative)."""
    index: dict[str, int] = {}
    low: dict[str, int] = {}
    on_stack: set[str] = set()
    stack: list[str] = []
    out: list[list[str]] = []
    counter = 0
    for root in sorted(edges):
        if root in index:
            continue
        work = [(root, iter(sorted(edges.get(root, ()))))]
        index[root] = low[root] = counter
        counter += 1
        stack.append(root)
        on_stack.add(root)
        while work:
            v, it = work[-1]
            nxt = next(it, None)
            if nxt is not None:
                if nxt not in index:
                    index[nxt] = low[nxt] = counter
                    counter += 1
                    stack.append(nxt)
                    on_stack.add(nxt)
                    work.append((nxt, iter(sorted(edges.get(nxt, ())))))
                elif nxt in on_stack:
                    low[v] = min(low[v], index[nxt])
                continue
            work.pop()
            if work:
                low[work[-1][0]] = min(low[work[-1][0]], low[v])
            if low[v] == index[v]:
                comp = []
                while True:
                    w = stack.pop()
                    on_stack.discard(w)
                    comp.append(w)
                    if w == v:
                        break
                if len(comp) > 1:
                    out.append(sorted(comp))
    return out


def deepest_cover(nodes: list[Node], sid: str) -> list[Node]:
    hits = [n for n in nodes if n.level >= 1 and matches(n.lst("covers"), sid)]
    if not hits:
        return []
    top = max(n.level for n in hits)
    return [n for n in hits if n.level == top]


def link(n: Node, frm: Path) -> str:
    target = Path("..") / n.path.relative_to(ROOT)
    return f"[{n.id}]({target.as_posix()})"


def write_reports(nodes: list[Node], by_id, seeds, oos, rep: Report) -> None:
    GEN.mkdir(exist_ok=True)
    roots = [n for n in nodes if n.level == 0]

    # INDEX.md
    out = ["# Spec tree index", "", "_Generated by `tools/scope.py check`. Do not edit._", ""]
    counts = defaultdict(lambda: defaultdict(int))
    for n in nodes:
        counts[n.level][n.status] += 1
    out += ["| Level | " + " | ".join(sorted(STATUSES)) + " | total |",
            "|---|" + "---:|" * (len(STATUSES) + 1)]
    for lvl in sorted(counts):
        row = [str(counts[lvl][s]) for s in sorted(STATUSES)]
        out.append(f"| {lvl} | " + " | ".join(row) + f" | {sum(counts[lvl].values())} |")
    out += ["", "## Tree", ""]

    def walk(n: Node, depth: int) -> None:
        mark = {"open": "[ ]", "split": "[+]", "leaf": "[x]", "decision": "[?]", "frozen": "[=]"}.get(n.status, "[!]")
        claim = f" (claimed: {n.meta.get('claimed_by')})" if n.meta.get("claimed_by") else ""
        out.append(f"{'  ' * depth}- {mark} {link(n, GEN)} {n.meta.get('title', '')}, "
                   f"{n.meta.get('phase', '')}, {n.meta.get('size', '')}{claim}")
        for c in sorted(n.children, key=lambda c: c.id):
            walk(c, depth + 1)

    for r in roots:
        walk(r, 0)
    out += ["", "Legend: [ ] open, [+] split, [x] leaf, [?] decision, [=] frozen", ""]
    (GEN / "INDEX.md").write_text("\n".join(out), encoding="utf-8")

    # COVERAGE.md
    out = ["# Competitor feature coverage", "", "_Generated by `tools/scope.py check`. Do not edit._", "",
           "Each competitor feature (seed row) is shown with the deepest spec nodes that cover it.", ""]
    oos_patterns = [p for p, _ in oos]
    by_file = defaultdict(list)
    for sid, (feat, fname) in seeds.items():
        by_file[fname].append((sid, feat))
    total = covered = oos_n = 0
    sections = []
    for fname in sorted(by_file):
        rows = ["", f"## {fname}", "", "| Seed | Feature | Covered by | Status |", "|---|---|---|---|"]
        for sid, feat in by_file[fname]:
            total += 1
            if matches(oos_patterns, sid):
                oos_n += 1
                rows.append(f"| {sid} | {feat} | out of scope | - |")
                continue
            hits = deepest_cover(nodes, sid)
            if hits:
                covered += 1
                rows.append(f"| {sid} | {feat} | " + ", ".join(link(h, GEN) for h in hits) + " | "
                            + ", ".join(sorted({h.status for h in hits})) + " |")
            else:
                rows.append(f"| {sid} | {feat} | **UNCOVERED** | - |")
        sections += rows
    out.append(f"**{covered}** covered, **{oos_n}** out of scope, **{total - covered - oos_n}** uncovered, "
               f"of **{total}** seed features.")
    out += sections + [""]
    (GEN / "COVERAGE.md").write_text("\n".join(out), encoding="utf-8")

    # DECISIONS.md
    out = ["# Decisions", "", "_Generated by `tools/scope.py check`. Do not edit._", "",
           "| Decision | Status | Title | Blocks |", "|---|---|---|---|"]
    for f in sorted(DECISIONS.glob("D-*.md")):
        meta, _ = parse_front(f.read_text(encoding="utf-8"))
        if meta is None:
            rep.err(rel(f), "decision file needs frontmatter")
            continue
        blocked = [n.id for n in nodes if n.meta.get("decision") == meta.get("id")]
        blocks = sorted(set(meta.get("blocks", []) if isinstance(meta.get("blocks"), list) else []) | set(blocked))
        out.append(f"| [{meta.get('id')}](../decisions/{f.name}) | {meta.get('status')} | {meta.get('title')} | "
                   + ", ".join(blocks) + " |")
    out.append("")
    (GEN / "DECISIONS.md").write_text("\n".join(out), encoding="utf-8")


def cmd_check(_args) -> int:
    rep = Report()
    seeds, oos = load_seeds(rep)
    nodes = load_nodes(rep)
    by_id = validate(nodes, seeds, oos, rep)
    write_reports(nodes, by_id, seeds, oos, rep)
    for line in rep.warnings + rep.errors:
        print(line)
    leaves = sum(1 for n in nodes if n.status == "leaf")
    print(f"\n{len(nodes)} nodes ({leaves} leaves), {len(seeds)} seed features, "
          f"{len(rep.errors)} errors, {len(rep.warnings)} warnings. Reports in generated/.")
    return 1 if rep.errors else 0


def cmd_open(args) -> int:
    rep = Report()
    nodes = [n for n in load_nodes(rep) if n.status == "open"]
    if not nodes:
        return 0
    level = args.level if args.level is not None else min(n.level for n in nodes)
    picked = sorted((n for n in nodes if n.level == level), key=lambda n: n.id)
    if args.limit:
        picked = picked[: args.limit]
    for n in picked:
        print(n.where)
    return 0


def cmd_milestone(args) -> int:
    """Compute a milestone's build plan: dependency closure, leaf waves, open work, blocking decisions."""
    rep = Report()
    path = Path(args.file)
    if not path.is_absolute():
        path = ROOT / path
    meta, body = parse_front(path.read_text(encoding="utf-8"))
    if meta is None:
        print(f"ERROR {path}: {body}")
        return 1
    nodes = load_nodes(rep)
    seeds, oos = load_seeds(rep)
    by_id = validate(nodes, seeds, oos, Report())
    children = defaultdict(list)
    for n in nodes:
        if n.parent:
            children[n.parent].append(n.id)

    def subtree(nid: str) -> set[str]:
        out, stack = set(), [nid]
        while stack:
            x = stack.pop()
            if x in out:
                continue
            out.add(x)
            stack.extend(children.get(x, []))
        return out

    def ancestors(nid: str) -> list[str]:
        out, cur = [], by_id.get(nid)
        while cur is not None and cur.parent:
            out.append(cur.parent)
            cur = by_id.get(cur.parent)
        return out

    targets = meta.get("targets", []) if isinstance(meta.get("targets"), list) else []
    excludes = meta.get("exclude", []) if isinstance(meta.get("exclude"), list) else []
    excluded = set().union(*[subtree(e) for e in excludes if e in by_id]) if excludes else set()
    scope: set[str] = set()
    pulled: dict[str, str] = {}
    missing = [t for t in targets if t not in by_id]
    for t in targets:
        if t in by_id:
            scope |= subtree(t) - excluded
    blocked_by_exclude: list[str] = []
    changed = True
    while changed:
        changed = False
        for nid in sorted(scope):
            n = by_id[nid]
            for d in n.lst("depends_on"):
                if d not in by_id or d in scope:
                    continue
                if d in excluded:
                    blocked_by_exclude.append(f"{nid} -> {d}")
                    continue
                pulled[d] = nid
                scope |= subtree(d) - excluded
                changed = True

    leaves = sorted(x for x in scope if by_id[x].status == "leaf")
    open_nodes = sorted(x for x in scope if by_id[x].status == "open")
    dec_nodes = sorted(x for x in scope if by_id[x].status == "decision")
    decisions = sorted({by_id[x].meta.get("decision", "") for x in dec_nodes} - {""})

    # Effective dependencies of a leaf: its own and its ancestors' depends_on, mapped to leaves in scope.
    leaf_set = set(leaves)
    leaf_deps: dict[str, set[str]] = {}
    for lf in leaves:
        deps = set()
        # Build order uses each leaf's OWN depends_on. Ancestors' depends_on describe whole subsystems and are
        # too coarse (they create false cycles, e.g. a pure L0 leaf inheriting its parent's CORE dependency).
        for owner in [lf]:
            for d in by_id[owner].lst("depends_on"):
                if d in by_id:
                    deps |= subtree(d) & leaf_set
        deps.discard(lf)
        # A leaf never waits on its own ancestors' subtree siblings through the ancestor's own deps.
        leaf_deps[lf] = deps
    waves: list[list[str]] = []
    done: set[str] = set()
    remaining = set(leaves)
    while remaining:
        wave = sorted(x for x in remaining if leaf_deps[x] <= done)
        if not wave:
            wave = sorted(remaining)  # residual cycle: emit together, flagged below
            waves.append(["CYCLE"] + wave)
            break
        waves.append(wave)
        done |= set(wave)
        remaining -= set(wave)

    mid = meta.get("id", path.stem)
    out_dir = GEN / "milestones"
    out_dir.mkdir(parents=True, exist_ok=True)
    lines = [f"# Milestone {mid}: {meta.get('title', '')}", "",
             "_Generated by `tools/scope.py milestone`. Do not edit; edit the milestone file instead._", "",
             f"Source: [{path.name}](../../milestones/{path.name})", "",
             "| | Count |", "|---|---:|",
             f"| Nodes in scope | {len(scope)} |",
             f"| Buildable leaves | {len(leaves)} |",
             f"| Open nodes still to expand | {len(open_nodes)} |",
             f"| Nodes blocked on decisions | {len(dec_nodes)} |",
             f"| Build waves | {len([w for w in waves if w and w[0] != 'CYCLE'])} |", ""]
    if missing:
        lines += ["**Unknown targets:** " + ", ".join(missing), ""]
    if decisions:
        lines += ["## Blocking decisions", ""] + [f"- {d}" for d in decisions] + [""]
    if pulled:
        lines += ["## Pulled in by dependencies", "", "| Node | Pulled in by |", "|---|---|"]
        lines += [f"| {k} | {v} |" for k, v in sorted(pulled.items())] + [""]
    if blocked_by_exclude:
        lines += ["## Dependencies on excluded nodes (review)", ""] + [f"- {x}" for x in sorted(set(blocked_by_exclude))] + [""]
    lines += ["## Build waves", "", "Every leaf in a wave depends only on leaves in earlier waves, so a wave can be built in parallel.", ""]
    for i, w in enumerate(waves, 1):
        if w and w[0] == "CYCLE":
            lines += [f"### Unordered (dependency cycle among {len(w) - 1} leaves)", ""] + [f"- {x}" for x in w[1:]] + [""]
            continue
        lines += [f"### Wave {i} ({len(w)} leaves)", ""]
        lines += [f"- {link(by_id[x], out_dir)} {by_id[x].meta.get('title', '')} "
                  f"({by_id[x].meta.get('size', '')}, {by_id[x].meta.get('phase', '')})" for x in w] + [""]
    lines += ["## Still to expand (open)", ""] + [f"- {x} {by_id[x].meta.get('title', '')}" for x in open_nodes] + [""]
    out = out_dir / f"{mid}.md"
    text = "\n".join(lines).replace("](../spec/", "](../../spec/")
    out.write_text(text, encoding="utf-8")
    print(f"{mid}: {len(scope)} nodes, {len(leaves)} leaves in {len([w for w in waves if w[0] != 'CYCLE'])} waves, "
          f"{len(open_nodes)} open, {len(dec_nodes)} blocked on {len(decisions)} decisions, "
          f"{len(pulled)} pulled in. Wrote {rel(out)}")
    return 0


def cmd_stats(_args) -> int:
    rep = Report()
    nodes = load_nodes(rep)
    counts = defaultdict(lambda: defaultdict(int))
    for n in nodes:
        counts[n.level][n.status] += 1
    for lvl in sorted(counts):
        print(f"L{lvl}: " + ", ".join(f"{s}={c}" for s, c in sorted(counts[lvl].items())))
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("check").set_defaults(fn=cmd_check)
    o = sub.add_parser("open")
    o.add_argument("--level", type=int)
    o.add_argument("--limit", type=int, default=0)
    o.set_defaults(fn=cmd_open)
    sub.add_parser("stats").set_defaults(fn=cmd_stats)
    ms = sub.add_parser("milestone")
    ms.add_argument("file", help="milestone file, e.g. milestones/launch-1.md")
    ms.set_defaults(fn=cmd_milestone)
    args = ap.parse_args()
    return args.fn(args)


if __name__ == "__main__":
    sys.exit(main())
