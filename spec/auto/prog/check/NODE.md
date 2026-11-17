---
id: AUTO.PROG.CHECK
title: Static program checker
level: 3
parent: AUTO.PROG
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-AUTO.PROG-003]
covers: []
depends_on: [AUTO.PROG.FMT, AUTO.PROG.EXPR, AUTO.PROG.FLOW, AUTO.PROG.OPER, AUTO.PROG.FAMILY]
cli: [fid program check]
mcp: [program_check]
journal: []
updated: 2026-11-16
---

# AUTO.PROG.CHECK: Static program checker

## Purpose

`program.check@1` (R1): runs every static rule over a program against a manifest and returns one ordered diagnostic list. It resolves every `invoke` against the manifest (command and major exist, arguments match the parameter schema), runs the expression, flow, operator and family checks, and summarises the side-effect classes each step needs so a reviewer can see what the program may do before it runs.

## Scope

**In:**
- `program.check@1` command (R1), diagnostics model, ordering and severities
- Command resolution and argument-schema checks against the manifest
- Orchestration of EXPR, FLOW, OPER and FAMILY checks
- Per-step and whole-program side-effect class summary (including M1 steps that will need the approval station)

**Out:**
- The individual rules (owned by FMT, EXPR, FLOW, OPER, FAMILY)

## Requirements

- **REQ-AUTO.PROG.CHECK-001** If an `invoke` names a command ID or major absent from the manifest, then the checker shall report `unknown_command` at that step.
- **REQ-AUTO.PROG.CHECK-002** If an `invoke`'s arguments do not match the command's parameter schema, then the checker shall report `bad_arguments` naming each parameter.
- **REQ-AUTO.PROG.CHECK-003** The checker shall return diagnostics ordered by step position, each with step ID, code, severity and message.
- **REQ-AUTO.PROG.CHECK-004** The checker shall report the highest side-effect class of each step and of the whole program.
- **REQ-AUTO.PROG.CHECK-005** `program.check@1` shall be side-effect class R1 and deterministic (D0) for the same program and manifest digest.

## Interfaces

- **Offers:** `program.check@1` (R1); `fid program check`; MCP `program_check`.
- **Consumes:** FMT, EXPR, FLOW, OPER, FAMILY; CORE.REG manifest.

## Decomposition hints

RESOLVE (commands and arguments), DIAG (diagnostics model), CLASS (side-effect summary), CMD (the registry command). Could become one M leaf once EXPR and FLOW expose their check APIs.

## Open questions

- None beyond those of EXPR and FLOW.

## Children

_None yet: status `open`._
