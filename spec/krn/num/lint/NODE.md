---
id: KRN.NUM.LINT
title: KNP-1 lint and kernel dependency allowlist check
level: 3
parent: KRN.NUM
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-KRN.NUM-001, REQ-KRN.NUM-008, REQ-KRN.NUM-009]
covers: []
depends_on: []
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# KRN.NUM.LINT: KNP-1 lint and kernel dependency allowlist check

## Purpose

The CI tool that enforces KNP-1 on the whole kernel dependency graph, not just the kernel's own crates. It scans the source of every crate reachable from the kernel roots, checks build flags, checks each dependency against the allowlist, and bans references to reference datasets so agent-written code cannot fit to the answers.

## Scope

**In:**
- Resolving the kernel dependency closure from the lockfile (kernel root crates listed in a config file)
- Source scan of every crate in the closure for banned APIs: `mul_add`, `powi`, `powf` and other std float transcendentals, `f64::min`/`max`, std `HashMap`/`HashSet`, `is_x86_feature_detected!` and other runtime CPU-feature dispatch, `unsafe`, `std::fs`, `std::net`, `std::time`, `std::env`, threads
- Build-flag checks: no `target-cpu=native`, no `+fma` target feature, pinned toolchain, `codegen-units=1`
- Dependency allowlist check: crate name, version and SPDX licence against the allowlist (written by `ECO.RULES` under D-001)
- Dataset-reference ban: no kernel source reference to a reference-dataset ID, file name, or embedded reference constant (denylist patterns supplied by `UNC.RDL`)
- Machine-readable findings (crate, file, line, rule) and a non-zero exit on any finding; a reviewed waiver file for vendored code with justification per waiver

**Out** (and which node owns it instead):
- Patching vendored solvers to pass the lint (`KRN.SOLV`)
- The allowlist content and licence policy (`ECO.RULES`)
- Reference dataset content and its denylist patterns (`UNC.RDL`)
- General workspace lints such as `unwrap` denial outside the kernel (`ECO.CI`)

## Requirements

- **REQ-KRN.NUM.LINT-001** When CI runs, the lint shall compute the dependency closure of the configured kernel root crates from the lockfile and scan every source file of every crate in it.
- **REQ-KRN.NUM.LINT-002** The lint shall report a finding with crate, file, line and rule ID for each use of a banned API in the KNP-1 list, and shall exit non-zero if any unwaived finding exists.
- **REQ-KRN.NUM.LINT-003** The lint shall fail if the kernel build configuration enables `target-cpu=native`, any FMA target feature, more than one codegen unit, or an unpinned toolchain.
- **REQ-KRN.NUM.LINT-004** The lint shall fail if any crate in the kernel closure is absent from the allowlist or carries a licence expression not on the SPDX allowlist.
- **REQ-KRN.NUM.LINT-005** The lint shall fail on any kernel source token that matches a pattern in the reference-dataset denylist (dataset IDs, file names, or numeric constants of 12 or more significant digits that appear in a reference result file).
- **REQ-KRN.NUM.LINT-006** A waiver shall name the crate, file, line range, rule and a reviewer, and the lint shall fail if a waived line changes without the waiver being renewed.
- **REQ-KRN.NUM.LINT-007** The lint shall itself have a test suite of seeded violations, one per rule, each of which must be detected.

## Interfaces

- **Offers:** `fid-knp1-lint` CLI tool used by CI (not a registry command); JSON findings report consumed by `KRN.MAN` (lint result in the manifest and solver audit) and `KRN.SOLV` (audit record).
- **Consumes:** the lockfile; the allowlist from `ECO.RULES`; the dataset denylist from `UNC.RDL`.

## Decomposition hints

GRAPH (closure from lockfile and allowlist check), SCAN (syntax-level source scan per rule), FLAGS (build-configuration check), DATA (dataset denylist matcher), WAIVE (waiver file and staleness check).

## Open questions

- Whether FMA contraction can be excluded by build flags alone on every target, or whether a codegen-level check (for example scanning emitted assembly or wasm for fused instructions) is also needed [unverified].
- Macro-generated code: a syntax-level scan may miss banned calls produced by macros from dependencies; scanning expanded code may be required [unverified].

## Children

_None yet: status `open`._
