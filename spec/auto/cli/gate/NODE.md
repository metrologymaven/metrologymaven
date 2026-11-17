---
id: AUTO.CLI.GATE
title: Dry-run handling and class gating at the CLI
level: 3
parent: AUTO.CLI
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-AUTO.CLI-003, REQ-AUTO.CLI-004, REQ-AUTO.CLI-008]
covers: []
depends_on: [AUTO.CLI.TREE, AUTO.SURF.ROOTS]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.CLI.GATE: Dry-run handling and class gating at the CLI

## Purpose

The checks the CLI makes before it sends anything to the bus: whether `--dry-run` is allowed for this command, whether the command's class can be executed from a terminal at all (M1, S1 and A0 cannot), and whether raw paths are allowed for the current principal. It refuses early with a message that tells the user where the action must happen instead.

## Scope

**In:**
- `--dry-run` acceptance or refusal from the projection's dry-run flag
- Forwarding dry run to the bus as the dry-run invocation mode
- Refusal of M1, S1 and A0 commands with a pointer to the approval station, the out-of-band signing path or the GUI admin settings
- Calling the raw-path gate from `AUTO.SURF.ROOTS`

**Out:**
- Dry-run semantics inside commands and replay (`CORE.BUS`, `CORE.RPL`)
- Motion approval and run tickets (`CMM.EXEC`)
- Signing (`TRUST.SIGN`)

## Requirements

- **REQ-AUTO.CLI.GATE-001** When `--dry-run` is given and the projection declares dry-run support, the CLI shall invoke the command in dry-run mode and shall render the reported changes with outcome `dry_run`.
- **REQ-AUTO.CLI.GATE-002** If `--dry-run` is given and the projection does not declare dry-run support, then the CLI shall exit 2 with `dry_run_unsupported` and send nothing to the bus.
- **REQ-AUTO.CLI.GATE-003** If the command's class is M1, then the CLI shall exit 3 without contacting the bus and shall tell the user that motion runs only through the approval station.
- **REQ-AUTO.CLI.GATE-004** If the command's class is S1, then the CLI shall exit 3 without contacting the bus and shall name the out-of-band signing path; if it is A0, then the CLI shall exit 3 and name the GUI settings page.
- **REQ-AUTO.CLI.GATE-005** Before sending, the CLI shall pass every raw-path value to the `AUTO.SURF.ROOTS` gate with the session's principal kind and shall exit 3 on `raw_path_refused`.

## Acceptance criteria

- **AC-1** Given fixture command `demo.set_offset@1` (W2, dry-run declared), when `fid demo set-offset --offset 1mm --dry-run --json` runs on the fake bus, then the bus receives mode `dry_run` and the envelope outcome is `dry_run`; the fake journal is unchanged.
- **AC-2** (error path) Given `demo.show_thing@1` (no dry-run), when run with `--dry-run`, then exit 2 with `dry_run_unsupported` and the fake bus receives nothing.
- **AC-3** (error path) Given fixture command `demo.move_axis@1` (M1), when invoked, then exit 3, the message names the approval station, and the fake bus receives nothing.
- **AC-4** (error path) Given `demo.sign_record@1` (S1) and `demo.set_policy@1` (A0), then each exits 3 with its named path and the fake bus receives nothing.
- **AC-5** (error path) Given principal kind `script` and a raw path value for a `path_like` parameter, then exit 3 with `raw_path_refused`.

## Design

- **Registry command(s):** none.
- **Data structures:** `GateDecision { Proceed { mode: Normal|DryRun }, Refuse { code, exit: ExitCode, hint: Hint } }`; `Hint` is a closed enum (approval station, out-of-band signing, admin settings) rendered from fixed text.
- **Algorithm:** order: class check, dry-run check, raw-path gate; first refusal wins.
- **Errors:** `dry_run_unsupported`, `policy_refused`, `raw_path_refused`.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-cli`

## Test fixtures

Fixture manifest `fixtures/manifest/min-classes.json` extended with `demo.move_axis@1` (M1), `demo.sign_record@1` (S1), `demo.set_policy@1` (A0); fake bus and fake journal from `fid-surface` tests.

## Build prompt

> In crate `fid-cli`, implement a pre-send gate. Order: (1) class M1 → exit 3, message "motion runs only through the approval station"; S1 → exit 3, name the out-of-band signing path; A0 → exit 3, name the GUI settings page; never contact the bus for these. (2) `--dry-run`: if the projection declares dry-run support, invoke in dry-run mode and render outcome `dry_run`; otherwise exit 2 `dry_run_unsupported` and send nothing. (3) Pass raw-path values to `fid_surface::gate_raw_paths` with the session principal kind; exit 3 on `raw_path_refused`. Acceptance: the five cases using the fake bus and fake journal. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; hint texts are fixed project-authored strings.

## Open questions

- Which principal kind the CLI session has is D-012; this gate takes it as an input.
- Whether a human CLI user should be able to file a motion run request (instead of a refusal) belongs to `CMM.EXEC`; this leaf only refuses.

## Children

_Leaf: none._
