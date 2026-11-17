---
id: AUTO.SCRIPTED
title: Scripted elements and scripted checks
level: 2
parent: AUTO
kind: subsystem
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-AUTO-008, REQ-AUTO-009]
covers: [GOM.AUT.03, GOM.AUT.04]
depends_on: [CORE, AUTO.PY, AUTO.PROG, AUTO.REC]
cli: [fid script create-element, fid script create-check, fid script recompute]
mcp: [script_create_element, script_create_check, script_recompute]
journal: [script.create_element, script.create_check, script.recompute]
updated: 2026-11-16
---

# AUTO.SCRIPTED: Scripted elements and scripted checks

## Purpose

User-defined element types and checks implemented by a script (Python through `AUTO.PY`, or a Command Program through `AUTO.PROG`). A scripted element or check declares its inputs, takes part in the CORE dependency graph like a built-in one, recomputes when its inputs change, and labels every ResultRecord it yields with the script's identity and version. It is the extension path for analyses that do not justify a plugin.

## Scope

**In:**
- Declaration of a scripted element or check: inputs, outputs, parameters, script reference
- Registration in the element tree and dependency graph
- Recompute when inputs or the script version change
- ResultRecord labelling with script identity (digest) and version
- Rules that keep values of record on the kernel (C1)
- Behaviour when the script fails, times out or is deprecated

**Out** (and which node owns it instead):
- Script execution and isolation (`AUTO.PY`)
- Program interpretation (`AUTO.PROG`)
- Script lifecycle and release (`AUTO.REC`)
- Dependency graph and recompute engine (`CORE`)
- Characteristic model and decision rules (`GDT`, `UNC`)
- Plugin-defined element types (`ECO`)

## Requirements

- **REQ-AUTO.SCRIPTED-001** A scripted element or check shall declare its input elements, and the system shall register those as dependency-graph edges.
- **REQ-AUTO.SCRIPTED-002** When an input of a scripted element or check changes, the dependency graph shall recompute it, and the recompute shall be journaled with the other dependents as one entry.
- **REQ-AUTO.SCRIPTED-003** Every ResultRecord produced by a scripted check shall carry the script's content digest and version alongside the algorithm ID fields.
- **REQ-AUTO.SCRIPTED-004** Every value of record in a scripted check's ResultRecord shall come from a registry command result, and if the script supplies a value that no kernel command produced, then the system shall refuse to record it as a value of record.
- **REQ-AUTO.SCRIPTED-005** Scripts behind scripted elements and checks shall run with the side-effect limits of an automation principal and shall not perform W3, X1, D1, M1, S1 or A0 commands during recompute.
- **REQ-AUTO.SCRIPTED-006** If the script fails or exceeds its time limit (60 s default [unverified]), then the element or check shall be marked failed with the error, and dependents shall be marked stale rather than computed from partial output.
- **REQ-AUTO.SCRIPTED-007** If a result of a scripted check is to be released, then the script shall be in the released state at the version recorded on that result.

## Interfaces

- **Offers:** `script.create_element@1`, `script.create_check@1`, `script.recompute@1`; a scripted element type and scripted characteristic type registered with CORE.
- **Consumes:** CORE dependency graph, element tree and journal; `AUTO.PY` host; `AUTO.PROG` interpreter; `AUTO.REC` lifecycle state.

## Decomposition hints

DECL (declaration schema), GRAPH (dependency registration and recompute), RESULT (ResultRecord labelling and value-of-record rule), FAIL (failure, timeout and staleness).

## Open questions

- REQ-AUTO.SCRIPTED-004 makes scripted checks orchestrate kernel commands rather than compute. If users need arithmetic on kernel results (for example a ratio of two distances) as a value of record, the kernel must offer a deterministic expression command. See the matching question in `AUTO.PROG`; this may need a KRN input.

## Children

_None yet: status `open`._
