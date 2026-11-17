---
id: MIG.SRC.GATE
title: Admission gate, check command and register build check
level: 3
parent: MIG.SRC
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-MIG.SRC-003, REQ-MIG.SRC-004]
covers: []
depends_on: [MIG.SRC.REG, IO.REG.DETECT, IO.REG.ENV]
uses_artifacts: [CORE.REG.MAN]
side_effect: R1
determinism: D0
cli: [fid migrate check-source]
mcp: [migrate_check_source]
journal: [migrate.check_source@1]
updated: 2026-11-17
---

# MIG.SRC.GATE: Admission gate, check command and register build check

## Purpose

Enforces the register at the moment of reading. `source_gate` is called by every MIG import and conversion command inside the IO import envelope, right after format detection and before the reader consumes more bytes; it refuses any source whose register entry is not `ADMITTED` or `OPEN_EXPORT_ONLY`, naming the open-format export route when one exists. `migrate.check_source@1` lets a user or agent ask the same question ahead of time. A build check proves that every MIG command's declared input formats are in the register.

## Scope

**In:**
- `source_gate(format_id, source_entry?) -> Result<AdmittedSource, SourceNotAdmitted>` and its registration as a pre-read hook in the `IO.REG.ENV` envelope for MIG commands
- `MIG_COMMAND_INPUTS`: a static table in `fid-mig` of MIG command ID → accepted input format IDs and artefact kinds
- `migrate.check_source@1` (R1): given a file handle (detection only) or a format ID, return admission status and export route
- `cargo xtask mig sources-gate-check`: every command in `MIG_COMMAND_INPUTS` exists in the registry manifest, every `migrate.*`/`correlate.*` manifest command with an import-handle parameter is in the table, and every listed format has a register entry

**Out:**
- Register data and its checks (owned by `MIG.SRC.REG`)
- Format detection (owned by `IO.REG.DETECT`) and the envelope (owned by `IO.REG.ENV`)

## Requirements

- **REQ-MIG.SRC.GATE-001** When a MIG import or conversion command runs, the gate shall be evaluated after format detection and before the reader reads any further byte.
- **REQ-MIG.SRC.GATE-002** If the detected format has no register entry, or its entry is not `ADMITTED` or `OPEN_EXPORT_ONLY`, then the gate shall refuse with `source_not_admitted` carrying the format ID, the entry ID if any, the status and the export route if registered.
- **REQ-MIG.SRC.GATE-003** When `migrate.check_source@1` runs, the system shall return the same admit or refuse outcome that the gate would return for that input, plus the register version.
- **REQ-MIG.SRC.GATE-004** If a command in `MIG_COMMAND_INPUTS` lists a format ID with no register entry, then the build check shall fail with `GATE-UNREGISTERED` naming the command and format.
- **REQ-MIG.SRC.GATE-005** If the registry manifest contains a `migrate.*` or `correlate.*` command with an import-handle parameter that is absent from `MIG_COMMAND_INPUTS`, then the build check shall fail with `GATE-UNDECLARED` naming the command.

## Acceptance criteria

- **AC-1** Given the register fixture `fixtures/mig/src/gate/register.json` and a CSV point file, when `fid migrate check-source --file <handle> --json` runs, then the outcome is admitted (`OPEN_EXPORT_ONLY`) with the register version.
- **AC-2** (error path) Given a file detected as `native:democmm-program`, which is `NOT_ADMITTED` with export route `dmis`, when a test MIG import command runs on it, then it fails with `source_not_admitted` naming route `dmis`, and the instrumented reader recorded zero bytes read beyond the detection window.
- **AC-3** (error path) Given `inputs-bad.rs` table listing format `qif-results-x` with no register entry, when `cargo xtask mig sources-gate-check` runs, then it fails with `GATE-UNREGISTERED`.
- **AC-4** (error path) Given a manifest extract with `migrate.import_form@1` taking an import handle and a table lacking it, then the check fails with `GATE-UNDECLARED`.

## Design

- **Registry command(s):** `migrate.check_source@1`, R1; params `file?: ImportHandle`, `format?: FormatId` (exactly one); result `{ admitted: bool, entry?, status, export_route?, register_version }`.
- **Data structures:** `AdmittedSource { entry_id, status, register_version }` (journaled by callers in their event), `SourceNotAdmitted { format_id, entry_id?, status?, export_route? }`.
- **Algorithm:** detection through `IO.REG.DETECT` (bounded signature window) → register lookup by format ID and, where given, entry ID → status test. The envelope hook makes the reader unreachable on refusal.
- **Errors:** `source_not_admitted`, `check_source_bad_params`, `GATE-UNREGISTERED`, `GATE-UNDECLARED`.
- **Determinism:** D0.
- **Performance budget:** gate under 50 µs per call excluding detection [unverified until measured].
- **Crate:** `fid-mig` (module `src::gate`).

## Test fixtures

`fixtures/mig/src/gate/`: fictional-product register, small detection samples and manifest extracts, CC0-1.0.

## Build prompt

> In crate `fid-mig`, add module `src::gate` implementing spec node MIG.SRC.GATE: `source_gate(format_id, entry?)` returning `AdmittedSource` or `source_not_admitted` (format, entry, status, export route), registered as the pre-read hook of the IO import envelope for every MIG command so the reader is never invoked on refusal; a static `MIG_COMMAND_INPUTS` table; the command `migrate.check_source@1` (R1; CLI `fid migrate check-source`, MCP `migrate_check_source`) that returns the same outcome for a file handle (detection only) or a format ID; and `cargo xtask mig sources-gate-check` failing `GATE-UNREGISTERED` and `GATE-UNDECLARED` against the register and the registry manifest. Tests for AC-1..AC-4, including an instrumented reader proving no bytes are read past detection on refusal. No raw paths; no `unwrap`/`expect`/`panic`/`unsafe`.

## Open questions

- The pre-read hook point must exist in `IO.REG.ENV`; if the envelope cannot run a hook between detection and reading, the IO maintainers need to add one.

## Children

_Leaf: none._
