---
id: ECO.REL.PKG.PLAN
title: Package plan, Tier 1 build gate and placeholder-name gate
level: 4
parent: ECO.REL.PKG
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.REL.PKG-001, REQ-ECO.REL.PKG-003, REQ-ECO.REL.PKG-005]
covers: []
depends_on: []
uses_artifacts: [PLAT.MATRIX.FILE, ECO.APPS.LIST, ECO.APPS.MANIFEST]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.REL.PKG.PLAN: Package plan, Tier 1 build gate and placeholder-name gate

## Purpose

Decides, from the support matrix, the app list and the app status data, exactly which packages a release must contain on which OS and architecture, including a standalone `fid-verify` package per combination. After the builds it turns their results into a gate verdict (any Tier 1 package missing or failed blocks the release) and applies the D-002 rule: while the product name is a placeholder, a release is internal and may not go to stable or lts.

## Scope

**In:**
- `cargo xtask rel pkg-plan`: writes `package-plan.json`
- `cargo xtask rel pkg-gate`: reads build results, writes `packages/gate.json`
- Package IDs and file-name scheme `<product>-<package>-<version>-<os>-<arch>.<ext>`
- The placeholder-name check over the branding slot and package metadata

**Out:**
- Building each package (owned by `ECO.REL.PKG.WIN`, `ECO.REL.PKG.MAC`, `ECO.REL.PKG.LINUX`)
- Tier assignment (owned by `PLAT.MATRIX.POLICY` through `PLAT.MATRIX.FILE`)
- App status values (owned by `ECO.APPS.MANIFEST`)

## Requirements

- **REQ-ECO.REL.PKG.PLAN-001** For each matrix combination whose effective tier at the current phase is Tier 1 or Tier 2, the plan shall list one package per app whose status is not `planned`, one `fid` CLI package and one standalone `fid-verify` package, each marked with the combination's tier.
- **REQ-ECO.REL.PKG.PLAN-002** The standalone `fid-verify` package entry shall declare contents limited to the `fid-verify` binary, its licence files and its documentation, and the plan check shall fail if any app or `fid` binary is listed in it.
- **REQ-ECO.REL.PKG.PLAN-003** If any Tier 1 package in the plan is missing from the build results or has status other than `success`, then the gate shall write verdict `block` naming each package.
- **REQ-ECO.REL.PKG.PLAN-004** When a Tier 2 package build fails, the gate shall list it as a release-note entry and shall not block.
- **REQ-ECO.REL.PKG.PLAN-005** If the branding slot marks the product name as placeholder, or any package metadata field contains the placeholder marker, then the gate shall set `internal: true`, and shall block a release whose channel is stable or lts.
- **REQ-ECO.REL.PKG.PLAN-006** The plan shall be sorted by (os, arch, package) and written as RFC 8785 canonical JSON.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rel/pkgplan/basic/` (matrix with Windows x86_64 Tier 1, Linux aarch64 Tier 2; apps report and inspect, inspect status `planned`), when `cargo xtask rel pkg-plan --matrix support-matrix.toml --apps apps.toml --status apps-status.json --phase P1a --out out/` runs, then the plan lists, per combination, `report`, `fid` and `fid-verify` only, and equals `expected-plan.json`.
- **AC-2** (error path) Given build results missing `report` on Windows x86_64, when `rel pkg-gate` runs, then it writes `verdict: block` naming that package.
- **AC-3** Given a failed Tier 2 `fid` build on Linux aarch64, then the verdict is `pass` with one release-note entry.
- **AC-4** (error path) Given branding slot `placeholder = true` and channel `stable`, then the verdict is `block` with reason `placeholder_name`; with channel `beta`, the verdict is `pass` with `internal: true`.
- **AC-5** (error path) Given a hand-edited plan whose `fid-verify` package lists the `fid` binary, when `rel pkg-plan --check` runs, then it exits 1 with `PLAN-VERIFY-CONTENTS`.

## Design

- **Registry command(s):** none; repository tools `cargo xtask rel pkg-plan [--check]` and `cargo xtask rel pkg-gate --plan <file> --results <dir> --channel <c> --branding <file> --out <dir>` (W1).
- **Data structures:** `PlanEntry { package, kind (app|cli|verify), os, arch, tier, contents: [binary names], file_name }`; `PkgGate { verdict, internal, blocked: [...], notes: [...] }`.
- **Algorithm:** read matrix rows through the published schema (no code link to `fid-platform`; the plan tool parses the TOML itself and a joint fixture checks agreement), filter by phase and tier, cross with the app list minus `planned` apps, add `fid` and `fid-verify`.
- **Errors:** `PLAN-VERIFY-CONTENTS`, `PLAN-UNKNOWN-APP`, `INPUT-PARSE` (exit 2).
- **Determinism:** D0.
- **Performance budget:** under 1 s.
- **Crate:** `fid-xtask` (module `rel::pkgplan`).

## Test fixtures

`fixtures/rel/pkgplan/` with small matrix, app list, status, branding and build-result files; CC-BY-4.0.

## Build prompt

> Add module `rel::pkgplan` to `fid-xtask`. `rel pkg-plan --matrix platform/support-matrix.toml --apps apps.toml --status apps-status.json --phase <P> --out <dir> [--check]`: for each matrix combination effective at the phase with tier1 or tier2, list one package per app whose status is not `planned`, one `fid` CLI package and one standalone `fid-verify` package (contents only fid-verify, licences, docs), with file names `<product>-<package>-<version>-<os>-<arch>.<ext>`; sort by (os, arch, package); RFC 8785 canonical JSON. `--check` validates an existing plan (PLAN-VERIFY-CONTENTS if fid-verify lists other binaries). `rel pkg-gate --plan <file> --results <dir> --channel <c> --branding <file> --out <dir>`: verdict block if any Tier 1 package is missing or not `success`; Tier 2 failures become notes; placeholder product name sets internal:true and blocks stable and lts. Write `packages/gate.json`. Tests: AC-1..AC-5 of spec node ECO.REL.PKG.PLAN. Constraints: no panics, deny unwrap/expect, bounded parsing, BTreeMap only.

## Open questions

- The branding slot's exact field (`placeholder = true`) belongs to `ECO.APPS.MANIFEST`, which is still open; the fixture fixes the field name until then.

## Children

_Leaf: none._
