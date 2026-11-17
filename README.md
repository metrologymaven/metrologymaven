# Fiducial spec tree

> **Working name:** Fiducial (internal only; see [D-002](decisions/D-002-project-name.md)).
> **Stage:** specification. This repository holds requirements and design, **not implementation**.

Fiducial is an open-source dimensional metrology platform meant to replace GOM/ZEISS INSPECT, PolyWorks, CMM programming suites (PC-DMIS, CALYPSO) and SpatialAnalyzer. It is MCP and CLI first, records everything, renders on the GPU (wgpu), and is written in modern languages: Rust everywhere (including the web, via WASM) and Python for user scripting. See [design lessons from the Craft app family](research/artcraft-design-lessons.md).

This repository decomposes the whole project **recursively** until every piece is small enough to become one implementation prompt. It also contains the tooling that lets people and coding agents keep growing the tree, so it can grow without one central planner.

## How the tree grows

```
spec/NODE.md                     L0  product (FID)
spec/<domain>/NODE.md            L1  17 domains (CORE, GEO, KRN, ALN, GDT, ...)
spec/<domain>/<sub>/NODE.md      L2  subsystems (usually one crate each)
...                              L3+ modules -> features -> tasks (leaves)
```

Every node is a single `NODE.md` with typed frontmatter and a status:

| Status | Meaning |
|---|---|
| `open` | Needs work: it should either be split into children or finished as a leaf |
| `split` | Has children, and every one of its requirements is allocated to at least one child |
| `leaf` | Small enough to implement in one PR. It has acceptance criteria, a design and a self-contained build prompt |
| `decision` | Blocked on a human decision recorded in [`decisions/`](decisions/) |
| `frozen` | Accepted and locked; changes go through a decision record |

One rule drives everything: **pick an open node and either split it or finish it as a leaf.** The full procedure is in [prompts/expand-node.md](prompts/expand-node.md). Coding agents follow it exactly, and people can follow it too.

## Quick start

```bash
python tools/scope.py check          # validate the tree and coverage, regenerate generated/
python tools/scope.py open           # list open nodes at the shallowest open level
python tools/scope.py stats          # counts by level and status
```

```powershell
.\tools\grow.ps1 -DryRun             # show which nodes would be expanded next
.\tools\grow.ps1 -Parallel 4         # expand one level with coding agents, then level-check
```

## What keeps the tree honest

`tools/scope.py check` runs in CI on every PR and fails if:

- a parent's requirement is not allocated to any child (`satisfies:`);
- a competitor feature from [`seeds/`](seeds/) is neither covered by a node nor listed as out of scope, or a split parent's covered feature is dropped by all of its children;
- a leaf lacks acceptance criteria, a design or a build prompt, or is too large (larger than size M);
- a user-facing feature leaf lacks its CLI command, MCP tool or journal event;
- IDs, levels, parents or folder names disagree.

The generated reports are in [`generated/`](generated/): `INDEX.md` (the tree), `COVERAGE.md` (each competitor feature mapped to our nodes) and `DECISIONS.md`.

## Map

| Path | What |
|---|---|
| [AGENTS.md](AGENTS.md) | Rules for every coding agent (Claude Code, Codex, and others) |
| [CONTRIBUTING.md](CONTRIBUTING.md) | How people contribute, with or without AI |
| [GOVERNANCE.md](GOVERNANCE.md) | Maintainers, decisions, how to become a maintainer |
| [spec/CONSTITUTION.md](spec/CONSTITUTION.md) | Cross-cutting rules every node obeys |
| [spec/](spec/) | The tree |
| [seeds/](seeds/) | Competitor feature inventories: the coverage checklist |
| [templates/](templates/) | Node, leaf and decision templates |
| [prompts/](prompts/) | The recursive expand prompt and the level check |
| [tools/](tools/) | `scope.py` (validator and reports) and `grow.ps1` (agent driver) |
| [decisions/](decisions/) | Decision records and the open queue |

## Background

This tree is seeded from the concept workspace one folder up (`../docs/`), in particular Design Baseline v3.1 and docs 01–10. That workspace planned a solo commercial product. This repository takes the open-source, community-built direction. Where the two disagree, the disagreement is recorded as a decision instead of being resolved silently.
