---
id: AUTO
title: Automation surfaces: CLI, MCP, Python, control channel, programs
level: 1
parent: FID
kind: domain
status: split
phase: P0
size: XXL
owners: []
claimed_by:
satisfies: [REQ-FID-004, REQ-FID-005, REQ-FID-014, REQ-FID-021]
covers: [GOM.AUT.*, PW.AUT.*, PCD.PRG.*, SA.MP.*]
depends_on: [CORE]
cli: [fid *]
mcp: [*]
journal: [program.*, script.*]
updated: 2026-11-16
---

# AUTO: Automation surfaces

## Purpose

Every way to drive the platform other than clicking. All of them are **generated from the registry**:
- the `fid` CLI;
- MCP servers (`fid mcp` with bridge, headless and auto modes);
- the Python API;
- the authenticated loopback control channel (inspect UI, run commands, drive input, screenshots, record and play actions);
- **Command Programs**, the canonical recorded-and-replayable program format (variables, loops, conditions, subroutines, operator prompts, part families);
- the "record a session → edit → release a program" lifecycle.

## Scope

**In:**
- CLI generator and conventions (`--json`, dry run, exit codes)
- MCP server generator, progressive disclosure (≤ 15 front-door tools per server), Skills
- MCP modes: bridge to a running app, headless, auto
- Python API generator and out-of-process script host
- Control channel: loopback, HMAC challenge auth, token file, read/write roots
- Command Program format and interpreter (variables, expressions, loops, conditions, subroutines, operator steps, parameterised part families)
- Program editor model (command and summary views)
- Macro recording into programs
- Script lifecycle: draft → registered → released → deprecated
- Batch processing
- Scripted elements and scripted checks
- Custom dialogs for scripts

**Out:**
- Command definitions themselves (each domain registers its own)
- Journal and replay internals (CORE)
- Plugin packaging and SDK distribution (ECO)
- Motion execution (CMM)

## Requirements

- **REQ-AUTO-001** The CLI, MCP tools, Python bindings and command docs shall be generated from the registry manifest. A CI check shall fail if any registered command lacks a surface or a surface has no command.
- **REQ-AUTO-002** `fid mcp` shall run in bridge mode (drive a running app), headless mode (in-process session, no window) and auto mode, chosen by a setting when the app is closed.
- **REQ-AUTO-003** MCP projection shall follow side-effect classes. M1, A0 and S1 shall never be exposed. W3 and X1 shall be request-only. Each server shall expose at most 15 front-door tools, with the rest reachable through progressive disclosure.
- **REQ-AUTO-004** The control channel shall listen on loopback only. Authentication shall be a challenge-response with HMAC proofs from a user-only token file, so the token is never sent. The channel shall support UI inspection, command execution, input driving, screenshots and action record and playback.
- **REQ-AUTO-005** Agents and scripts shall access files only through approved read and write roots, and shall not be able to widen those roots.
- **REQ-AUTO-006** A recorded session shall be convertible into a Command Program that a human can edit (variables, loops, conditions, subroutines, operator prompts) and release.
- **REQ-AUTO-007** A released Command Program shall re-run on new data (re-targeted replay), and every run shall produce a journal that can be compared with the original.
- **REQ-AUTO-008** Python scripts shall run out of process through the same registry commands, with the same side-effect limits as agents unless a human runs them.
- **REQ-AUTO-009** Users shall define scripted elements and scripted checks that participate in the dependency graph and produce ResultRecords labelled with the script's identity and version.
- **REQ-AUTO-010** Long operations shall report progress and be cancellable from every surface, and timeouts shall say whether the command ran.

## Interfaces

- **Offers:** the `fid` binary; MCP servers; the `fiducial` Python package; the control-channel protocol spec; the Command Program format spec.
- **Consumes:** CORE registry, bus, journal and jobs.

## Decomposition hints

CLI, MCP (generator, servers, modes, Skills), PY (API + script host), CTRL (control channel), PROG (Command Program format + interpreter + editor model), REC (record-to-program lifecycle), SCRIPTED (scripted elements and checks), BATCH.

## Open questions

- Should Command Programs be authored as Python (an authoring view) with the canonical form underneath, as the baseline proposes? Yes, unless decided otherwise. Confirm when PROG is expanded.

## Split rationale

The split is by **surface and artifact**, with one shared foundation. AUTO.SURF owns everything the surfaces must agree on (the projection from the manifest, the CI completeness check, the result envelope, progress, cancellation, honest timeouts and root handles). Each surface child (CLI, MCP, PY, CTRL) then only renders that contract for its own transport and audience, so the surfaces cannot drift. The program side is split along the lifecycle of a Command Program: PROG defines what a program is and how it executes, REC turns recordings into programs and releases them, RUN re-runs released programs on new data (singly, for an operator, or in batches), and SCRIPTED plugs scripts into the dependency graph.

Shared concerns sit in exactly one child: operator-interaction descriptors in PROG (used by PY dialogs and RUN operator mode), the lifecycle in REC (used by RUN and SCRIPTED), path and timeout rules in SURF (used by every surface).

Alternatives considered:
- **One child per surface with no shared foundation** (CLI, MCP, PY, CTRL each generating their own). Rejected: four copies of the envelope, timeout and root rules, and REQ-AUTO-001's completeness check would have no owner.
- **Programs and scripts as one child.** Rejected: Command Programs are the canonical replayable format (format, interpreter, editor, debugger), while Python is an out-of-process host; combining them makes an XXL node with two unrelated runtimes.
- **Folding BATCH into REC.** Rejected: recording and release are authoring-time, while runs and batches are execution-time with their own resumability and comparison rules.
- **Separate MODES child for MCP bridge/headless/auto.** Rejected: the modes only make sense with the projection rules; they are a module of AUTO.MCP.

Seed rows owned by other domains in practice are covered here only for their automation side: GOM.AUT.05 and PW.AUT.03 (apps, plug-ins and SDK) in AUTO.PY as the scripting SDK, with packaging in ECO; GOM.AUT.06 (kiosk) in AUTO.RUN as operator playback, with the UI in VIZ; PCD.PRG.10 (DMIS) in AUTO.PROG as the mapping contract, with parsing in IO and conversion in CMM.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| AUTO.SURF | Surface generation and shared surface contract | 001 005 010 | none |
| AUTO.CLI | The fid command-line interface | 001 010 | none |
| AUTO.MCP | MCP servers, modes and side-effect projection | 001 002 003 005 010 | none |
| AUTO.CTRL | Loopback control channel | 004 005 010 | SA.MP.02 |
| AUTO.PY | Python API and out-of-process script host | 001 005 008 010 | GOM.AUT.01/05/08, PCD.PRG.06, PW.AUT.03 |
| AUTO.PROG | Command Program format, interpreter and editor model | 006 007 | PCD.PRG.01-05/07-10, PW.AUT.02, SA.MP.01/04 |
| AUTO.REC | Record to program and the release lifecycle | 006 007 | GOM.AUT.02, PW.AUT.01 |
| AUTO.RUN | Re-targeted runs, operator playback and batch processing | 007 010 | GOM.AUT.06/07, PW.AUT.04/05, SA.MP.03 |
| AUTO.SCRIPTED | Scripted elements and scripted checks | 008 009 | GOM.AUT.03/04 |
