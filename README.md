# Metrology Maven

**Open-source dimensional metrology software**, built in the open: [metrologymaven.io](https://metrologymaven.io)

> **Stage:** pre-alpha. The whole product is specified as a tree of small, testable work orders, and the first code (`crates/`) is being built from it. Nothing is usable for measurement yet.

Metrology Maven aims to replace GOM/ZEISS INSPECT, PolyWorks, CMM programming suites (PC-DMIS, CALYPSO) and SpatialAnalyzer with four focused apps on one shared core: **Report**, **Inspect**, **Probe** and **Network**. It is MCP and CLI first, records everything, renders on the GPU (wgpu), and is written in Rust (including the web, via WASM), with Python for user scripting. Results can be re-checked by anyone with the free verifier.

This one repository holds everything:

| Path | What |
|---|---|
| [`spec/`](spec/), [`seeds/`](seeds/), [`decisions/`](decisions/) | The specification tree, competitor inventories and decision records |
| [`crates/`](crates/) | The Rust workspace (`fid` CLI, `fid-*` crates) |
| [`site/`](site/) | The website, generated from the spec tree (`python site/build.py`) |
| [`fixtures/`](fixtures/) | Test data and conformance vectors |
| [`generated/`](generated/) | Reports generated from the tree, including the [expert queue](generated/EXPERT-QUEUE.md) |

**Want to help?** If you know GD&T, measurement uncertainty or CMM control, see the [expert queue](generated/EXPERT-QUEUE.md) and comment `/claim` on an `expert-queue` issue. Everyone else: [CONTRIBUTING.md](CONTRIBUTING.md).

The internal command and crate prefix `fid` comes from *fiducial*, a reference mark in metrology ([D-039](decisions/D-039-metrology-maven-name-and-public-home.md)).

## Build

```bash
cargo test --workspace                          # Rust crates
echo '{"b":1,"a":2}' | cargo run -q -p fid-cli -- canon
python site/build.py                            # website into site/dist/
```

## How the tree grows

```
spec/NODE.md                     L0  product
spec/<domain>/NODE.md            L1  26 domains (CORE, GEO, KRN, ALN, GDT, ...)
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

The tree was seeded from the founder's earlier design baseline. Where the open-source direction disagrees with it, the disagreement is recorded as a decision instead of being resolved silently.

## Licence

Code: MIT OR Apache-2.0. Specification and documentation: CC-BY-4.0. "Metrology Maven" is a trademark of its steward company and is not covered by these licences. See [LICENSE.md](LICENSE.md).
