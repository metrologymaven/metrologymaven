---
id: AUTO.PY
title: Python API and out-of-process script host
level: 2
parent: AUTO
kind: subsystem
status: open
phase: P1b
size: XL
owners: []
claimed_by:
satisfies: [REQ-AUTO-001, REQ-AUTO-005, REQ-AUTO-008, REQ-AUTO-010]
covers: [GOM.AUT.01, GOM.AUT.05, GOM.AUT.08, PCD.PRG.06, PW.AUT.03]
depends_on: [CORE, AUTO.SURF, AUTO.PROG]
cli: [fid script run*, fid script host-*]
mcp: [script_run*]
journal: [script.run*]
updated: 2026-11-16
---

# AUTO.PY: Python API and out-of-process script host

## Purpose

The generated `fiducial` Python package and the out-of-process host that runs user scripts against a session. Scripts call the same registry commands as every other surface, under the same side-effect limits as agents unless a human starts them. It also gives scripts custom dialogs (built on the operator-interaction model of `AUTO.PROG`) and the Python-facing SDK surface that third-party script bundles ("apps") are written against.

## Scope

**In:**
- Generated `fiducial` package: one typed function per command, docstrings from the manifest, unit-bearing value types
- Out-of-process script host: start, isolate, time-limit and cancel scripts
- Principal assignment for scripts (human-started vs agent- or automation-started)
- Running a script from GUI, CLI or MCP (`script.run@1` family)
- Custom dialogs for scripts, expressed with the `AUTO.PROG` operator-interaction descriptors
- External scripting from other languages through the same host protocol
- Python-facing SDK contract for script bundles ("apps")
- Supported-Python-version policy and test matrix

**Out** (and which node owns it instead):
- Plugin and app packaging, signing, distribution and store (`ECO`)
- Script lifecycle states and release (`AUTO.REC`)
- Scripted elements and checks in the dependency graph (`AUTO.SCRIPTED`)
- Dialog rendering in the GUI (`VIZ`)
- Command Programs as an authoring target (`AUTO.PROG`)

## Requirements

- **REQ-AUTO.PY-001** The `fiducial` package shall be generated from the registry manifest with one binding per registered command, and shall record the manifest digest it was generated from.
- **REQ-AUTO.PY-002** If the package's manifest digest differs from the connected session's digest, then every call shall fail with a typed version-mismatch error rather than call a command with a different signature.
- **REQ-AUTO.PY-003** User scripts shall run in a separate OS process from the app, and a crash, hang or memory exhaustion of a script shall not terminate the app or lose committed work.
- **REQ-AUTO.PY-004** Scripts shall reach the document only through registry commands on the bus, with no shared-memory or direct document access.
- **REQ-AUTO.PY-005** When a script is started by an agent or by automation, it shall run under an automation principal with the agent side-effect limits, and when started by a human from the GUI or CLI, it shall run under that human's principal.
- **REQ-AUTO.PY-006** No script, whatever its principal, shall perform M1 or S1 commands directly; such steps shall go to the approval station or the out-of-band signing path.
- **REQ-AUTO.PY-007** When a script run is cancelled, the host shall stop the script process within 5 s [unverified] and report which of its commands committed.
- **REQ-AUTO.PY-008** Scripts shall access files only through root handles, and the package shall offer no function that changes the roots.
- **REQ-AUTO.PY-009** Scripts shall be able to show custom dialogs whose fields, types, units and validation are declared with the `AUTO.PROG` operator-interaction descriptors, and every submitted dialog response shall be journaled.
- **REQ-AUTO.PY-010** The host protocol shall be documented so that a client in another language can run with the same principal, root and class rules as Python.
- **REQ-AUTO.PY-011** The project shall publish the supported CPython versions, and CI shall test the package on each of them.

## Interfaces

- **Offers:** the `fiducial` Python package; the script host and its protocol spec; `script.run@1` and related commands; the Python SDK contract for script bundles.
- **Consumes:** `AUTO.SURF` projections, envelope, jobs and root handles; CORE bus and principals; `AUTO.PROG` operator-interaction descriptors; ECO packaging for bundles.

## Decomposition hints

GEN (package generator and typed units), HOST (process isolation, limits, cancellation), PRIN (principal assignment), RUN (run commands across surfaces), DLG (custom dialogs), PROTO (host protocol for other languages), SDK (script-bundle contract).

## Open questions

- Sandboxing depth for scripts (OS-level sandbox vs process isolation only) differs by OS; may need a PLAT input.
- Whether a human-started script that later blocks for an agent's input keeps the human principal. This node assumes the principal is fixed at start.

## Children

_None yet: status `open`._
