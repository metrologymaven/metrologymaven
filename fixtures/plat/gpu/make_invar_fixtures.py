#!/usr/bin/env python3
"""Independent reference for PLAT.GPU.FALLBK.INVAR fixtures (licensed with the code).

Writes two folders of recorded digest lists, one JSON file per backend, plus the
comparison report the harness must print for them, rendered here from the report
rules in the module docs of crates/fid-xtask/src/plat_gpu_invar.rs (not by running
the Rust code):

  fixtures/plat/gpu/invar-digests-equal/  dx12, software, vulkan: all lists equal
  fixtures/plat/gpu/invar-digests-diff/   the same, but software differs at entry 4

The command sequence mirrors platform/invariance-sequence.toml (journal entries are
numbered from 1, the first journal sequence number). Result digests are SHA-256 of
a fixed label so that the fixture is reproducible; they stand in for the result
digests a real run would journal.

Run:  python fixtures/plat/gpu/make_invar_fixtures.py
"""

import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent

COMMANDS = [
    "view.pick_point@1",
    "view.resolve_lasso@1",
    "geo.resolve_selection@1",
    "fit.ls_cylinder@1",
    "check.eval_size@1",
    "report.generate_report@1",
]
BACKENDS = ["dx12", "software", "vulkan"]  # file-name order, which is the report order
DIFF_BACKEND = "software"
DIFF_INDEX = 4


def digest(label):
    return "sha256:" + hashlib.sha256(label.encode("utf-8")).hexdigest()


def entries(backend, diverge):
    out = []
    for i, cmd in enumerate(COMMANDS, start=1):
        variant = "reference"
        if diverge and backend == DIFF_BACKEND and i == DIFF_INDEX:
            variant = "diverged"
        out.append({"index": i, "command": cmd, "result": digest(f"invar-fixture-v1|{i}|{cmd}|{variant}")})
    return out


def render(lists):
    """Report rules (reference implementation)."""
    names = [b for b, _ in lists]
    lines = []
    n = max(len(e) for _, e in lists)
    first = None
    for p in range(n):
        row = [e[p] if p < len(e) else None for _, e in lists]
        if all(r == row[0] for r in row):
            continue
        groups = []  # [value, [list positions]] in order of first appearance
        for i, r in enumerate(row):
            for g in groups:
                if g[0] == r:
                    g[1].append(i)
                    break
            else:
                groups.append([r, [i]])
        best = groups[0]
        for g in groups[1:]:
            if len(g[1]) > len(best[1]):
                best = g
        ref = best[0] if best[0] is not None else next(r for r in row if r is not None)
        differ = [names[i] for i in range(len(row)) if i not in best[1]]
        first = (ref, differ, row)
        break
    lines.append("plat-gpu invariance: " + ("EQUAL" if first is None else "DIVERGED"))
    lines.append("backends: " + " ".join(names))
    lines.append("entries: " + " ".join(f"{b}={len(e)}" for b, e in lists))
    if first is not None:
        ref, differ, row = first
        lines.append(f"first difference: entry {ref['index']} command {ref['command']}")
        lines.append("differing backends: " + " ".join(differ))
        for b, r in zip(names, row):
            if r is None:
                lines.append(f"  {b}: missing")
            else:
                lines.append(f"  {b}: {r['index']} {r['command']} {r['result']}")
    return "\n".join(lines) + "\n"


def write(folder, diverge):
    d = HERE / folder
    d.mkdir(exist_ok=True)
    lists = []
    for b in BACKENDS:
        e = entries(b, diverge)
        lists.append((b, e))
        text = json.dumps({"backend": b, "entries": e}, indent=2, sort_keys=True) + "\n"
        (d / f"{b}.json").write_bytes(text.encode("utf-8"))
    (d / "expected-report.txt").write_bytes(render(lists).encode("utf-8"))


if __name__ == "__main__":
    write("invar-digests-equal", False)
    write("invar-digests-diff", True)
    print("wrote invar-digests-equal and invar-digests-diff")
