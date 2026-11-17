---
id: AUTO.PY.SDK
title: Script-bundle (app) contract
level: 3
parent: AUTO.PY
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-AUTO.PY-001, REQ-AUTO.PY-008]
covers: [GOM.AUT.05, PW.AUT.03]
depends_on: [AUTO.PY.GEN, AUTO.PY.PROTO]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.PY.SDK: Script-bundle (app) contract

## Purpose

The Python-facing contract for script bundles ("apps"): a set of scripts, dialogs, scripted elements and checks, and task guides shipped together, declaring the manifest-digest range they work with, the commands and side-effect classes they use, and the roots they need. Packaging, signing and distribution of bundles belong to ECO; this node defines what a bundle may contain and how the host loads it.

## Scope

**In:**
- Bundle manifest schema (entry points, required commands and majors, declared classes, required roots)
- Load-time checks: digest compatibility, declared classes within the starter's limits
- Python API stability promise for bundles (which package surfaces are stable)

**Out:**
- Packaging, signing, store and install (`ECO.SDK`, `ECO.APPS`; installing is A0 per CONSTITUTION §3.1)
- Native plugins (`ECO.SDK`)

## Requirements

- **REQ-AUTO.PY.SDK-001** A bundle shall declare every command and major it calls, and the host shall refuse to load it if any is absent from the session manifest.
- **REQ-AUTO.PY.SDK-002** A bundle shall declare the highest side-effect class it uses, and the host shall refuse to start any of its entry points under a principal that may not perform that class.
- **REQ-AUTO.PY.SDK-003** A bundle shall reach files only through roots it declares, resolved through root handles.

## Interfaces

- **Offers:** bundle manifest schema; host load checks.
- **Consumes:** `AUTO.PY.GEN` package; `AUTO.PY.PROTO`; `ECO.SDK` packaging.

## Decomposition hints

SCHEMA (bundle manifest), LOAD (load-time checks), STABLE (API stability rules).

## Open questions

- Boundary with `ECO.SDK` on who owns the bundle manifest file format; proposed here: AUTO owns the runtime fields, ECO owns packaging fields.

## Children

_None yet: status `open`._
