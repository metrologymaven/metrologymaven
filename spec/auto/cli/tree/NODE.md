---
id: AUTO.CLI.TREE
title: Generated command tree and help
level: 3
parent: AUTO.CLI
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.CLI-001, REQ-AUTO.CLI-009]
covers: []
depends_on: [AUTO.SURF.PROJ, AUTO.SURF.GEN]
side_effect: R0
determinism: D0
cli: [fid *]
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.CLI.TREE: Generated command tree and help

## Purpose

The CLI emitter and the runtime command tree of `fid`. At build time it writes `cli-tree.json` (the CLI-relevant part of the projection) and the CLI `surface-index.json`; at run time `fid` builds its subcommand tree from the embedded `cli-tree.json`, so every registered command appears as `fid <domain> <verb-object>` with help taken from the registry.

## Scope

**In:**
- CLI `SurfaceEmitter` (surface `cli`)
- Runtime subcommand tree from the embedded tree file
- `--help` text for `fid`, each domain and each command, rendered from the projection's help text
- Reserved host subcommands (`mcp`, `ctrl`) registered by their owners, with no command IDs of their own

**Out:**
- Parsing parameter values (`AUTO.CLI.ARGS`)
- Output and exit codes (`AUTO.CLI.OUT`)
- Docs pages (`AUTO.SURF.DOCS`)

## Requirements

- **REQ-AUTO.CLI.TREE-001** The emitter shall write one tree entry per command major in the projection, with path `fid <domain> <verb-object>` exactly as the projection derives it, and shall list each in the CLI `surface-index.json`.
- **REQ-AUTO.CLI.TREE-002** The `fid` binary shall build its subcommand tree only from the embedded tree file, and shall contain no hand-written per-command subcommand or alias.
- **REQ-AUTO.CLI.TREE-003** When a command has majors 1 and 2 live, the tree shall expose the newest major at `fid <domain> <verb-object>` and older majors through `--major <N>`.
- **REQ-AUTO.CLI.TREE-004** The help text of each command shall be rendered from the projection's help field and shall name the manifest digest, the side-effect class and whether dry run is supported.
- **REQ-AUTO.CLI.TREE-005** When `fid --version` runs, it shall print the binary version and the embedded manifest digest.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/manifest/min-classes.json`, when the CLI is built against it and `fid --help --json` runs, then the listed command paths equal the projection's CLI paths for every command, and nothing else besides `mcp` and `ctrl`.
- **AC-2** Given the same build, when `fid align run-bestfit --help` runs, then the text equals the golden `fixtures/cli/help/align-run-bestfit.txt`, which names the manifest digest and class `W2`.
- **AC-3** Given a fixture manifest with `demo.show_thing@1` and `@2`, then `fid demo show-thing` invokes major 2 and `fid demo show-thing --major 1` invokes major 1.
- **AC-4** (error path) Given `fid align run-bestfitt`, then the command fails with exit code 2 (`usage_error`) and suggests `run-bestfit`.
- **AC-5** Given a source scan of `crates/fid-cli/src`, then no string literal equals a registry CLI path (lint test proving no hand-written subcommands).

## Design

- **Registry command(s):** none of its own; dispatches every registered command.
- **Data structures:** `CliTree { manifest_digest, domains: BTreeMap<String, Vec<CliCommand>> }`; `CliCommand { path, command_id, majors, params, help, side_effect, dry_run }`.
- **Algorithm:** emitter serialises `CliTree` canonically; build script embeds it; at start-up the binary builds a `clap::Command` tree from it (implementation choice at leaf level), registering host subcommands from `AUTO.MCP` and `AUTO.CTRL`.
- **Errors:** `usage_error` with nearest-match suggestion (edit distance ≤ 2).
- **Determinism:** D0 for the emitted tree and help text.
- **Performance budget:** `fid --help` in < 50 ms cold start with 5,000 commands [unverified until measured].
- **Crate:** `fid-cli`

## Test fixtures

`fixtures/manifest/min-classes.json` and a two-major fixture `fixtures/manifest/two-majors.json`; golden help files under `fixtures/cli/help/` (project-authored).

## Build prompt

> In crate `fid-cli`, implement the CLI surface emitter (`SurfaceEmitter` from `fid-surface-gen`, surface `cli`) that writes `cli-tree.json` (canonical JSON: per command major the path `fid <domain> <verb-object>`, command ID, params, help text, side-effect class, dry-run flag) and `surface-index.json`. In the `fid` binary, embed `cli-tree.json` and build the subcommand tree from it at start-up; newest major at the plain path, older ones via `--major N`. Help text comes only from the projection's help field and names the manifest digest, the side-effect class and dry-run support. `fid --version` prints the binary version and manifest digest. Unknown subcommands fail with exit code 2 and a nearest-match suggestion. Host subcommands `mcp` and `ctrl` are registered by their crates. Acceptance: tree equals projection for `fixtures/manifest/min-classes.json`; golden help text; two-major dispatch; usage error; a lint test proving no CLI path literal is hand-written in `src/`. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; no aliases (CONSTITUTION §3).

## Open questions

- AC-2 mentions class `W2` for `align.run_bestfit`, whereas the committed `fixtures/manifest/min-classes.json` declares `align.run_bestfit@1` and `@2` as class `R1` (and `align.set_datum@1` as `W2`). `fixtures/cli/help/align-run-bestfit.txt` faithfully reflects `min-classes.json` (`R1`), and `fixtures/cli/help/align-set-datum.txt` additionally checks a `W2` command.

## Children

_Leaf: none._
