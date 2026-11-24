---
id: AUTO.CLI.TARGET.SELECT
title: Target resolution from flags and environment
level: 4
parent: AUTO.CLI.TARGET
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [fid-cli]
satisfies: [REQ-AUTO.CLI.TARGET-004, REQ-AUTO.CLI.TARGET-005]
covers: []
depends_on: [AUTO.CLI.TREE]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# AUTO.CLI.TARGET.SELECT: Target resolution from flags and environment

## Purpose

Turns the global flags `--project` and `--app` and the environment variable `FID_PROJECT` into exactly one target (headless project, running app, or no project) before any command runs, and hands the chosen target to the transport adapter. Keeping the decision in one pure function means both transports see the same command set and the same envelope.

## Scope

**In:**
- Precedence of `--project`, `--app` and `FID_PROJECT`
- Mutual exclusion of `--project` and `--app`
- Refusing project-scoped commands when no project is selected
- One `TargetTransport` trait that both targets implement, so the command tree and envelope are target-independent

**Out:**
- Opening and locking a headless project (`AUTO.CLI.TARGET.HEADLESS`)
- Connecting to a running app (`AUTO.CLI.TARGET.APP`)
- The command tree and its help (`AUTO.CLI.TREE`)

## Requirements

- **REQ-AUTO.CLI.TARGET.SELECT-001** If both `--project` and `--app` are given, then the CLI shall exit 2 with `target_conflict` and run nothing.
- **REQ-AUTO.CLI.TARGET.SELECT-002** When `--project` is given, the CLI shall select the headless target with that value, ignoring `FID_PROJECT`.
- **REQ-AUTO.CLI.TARGET.SELECT-003** When `--app` is given, the CLI shall select the running-app target, ignoring `FID_PROJECT`.
- **REQ-AUTO.CLI.TARGET.SELECT-004** When neither flag is given and `FID_PROJECT` is set to a non-empty value, the CLI shall select the headless target with that value.
- **REQ-AUTO.CLI.TARGET.SELECT-005** If no target is selected and the command's projection declares that it needs a project, then the CLI shall exit 2 with `no_project_selected` and send nothing to any bus.
- **REQ-AUTO.CLI.TARGET.SELECT-006** The command tree, help text and envelope schema shall be computed from the manifest only, never from the selected target, so they are identical for both targets under one manifest digest.

## Acceptance criteria

- **AC-1** Given fixture manifest `fixtures/manifest/min-classes.json`, when `resolve_target` runs on each row of `fixtures/cli/target/select-cases.json` (flags, environment, command), then the result equals the expected target or error code for every row.
- **AC-2** (error path) Given `--project root:projects/a.fid --app`, when any command runs, then exit 2 with `target_conflict` and neither transport is constructed.
- **AC-3** (error path) Given no flags, no `FID_PROJECT` and project-scoped `demo.set_offset@1`, then exit 2 with `no_project_selected`; given the same with `fid registry list-commands` (no project needed), it runs.
- **AC-4** Given the fake headless and fake app transports, when `fid demo show-thing --help` and the generated envelope schema are produced under each, then the outputs are byte-identical.

## Design

- **Registry command(s):** none.
- **Data structures:** `enum Target { Headless { project: ProjectRef }, App, None }`; `ProjectRef` is the unparsed string handed to HEADLESS; `trait TargetTransport: fid_surface::BusTransport { fn kind(&self) -> TargetKind; }`.
- **Algorithm:** `resolve_target(flags, env, projection) -> Result<Target, TargetError>`: conflict check, then `--project`, then `--app`, then `FID_PROJECT`, then `None`; a `None` target with a project-scoped projection is an error.
- **Errors:** `target_conflict` (exit 2), `no_project_selected` (exit 2).
- **Determinism:** D0; pure function of flags, environment snapshot and manifest.
- **Performance budget:** negligible.
- **Crate:** `fid-cli`

## Test fixtures

`fixtures/cli/target/select-cases.json`: project-authored table of flag and environment combinations with expected outcomes (CC-BY-4.0). Reuses `fixtures/manifest/min-classes.json` and the fake transports from `fid-surface` tests.

## Build prompt

> In crate `fid-cli`, implement `resolve_target(flags, env, projection) -> Result<Target, TargetError>` with `Target { Headless { project }, App, None }`. Rules: `--project` and `--app` together → exit 2 `target_conflict`; `--project` wins over `FID_PROJECT`; `--app` ignores `FID_PROJECT`; with no flag a non-empty `FID_PROJECT` selects headless; no target plus a projection that needs a project → exit 2 `no_project_selected`, nothing sent. Define `trait TargetTransport` extending `fid_surface::BusTransport` so the headless and app targets plug into the same invocation path. Help and envelope schema must be computed only from the manifest. Acceptance: the table-driven test over `fixtures/cli/target/select-cases.json`, conflict and no-project errors, and byte-identical help under both fake transports. Constraints: no panic, no `unsafe`; deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; read the environment once into a snapshot so tests are deterministic.

## Open questions

- Whether a projection's "needs a project" flag is declared per command in `CORE.REG.DEF` or derived from its parameter types is not yet fixed; this leaf reads it from the projection either way.

## Children

_Leaf: none._
