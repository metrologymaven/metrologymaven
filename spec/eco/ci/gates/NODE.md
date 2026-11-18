---
id: ECO.CI.GATES
title: Path-scoped check planning and CI-change isolation
level: 3
parent: ECO.CI
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-ECO.CI-004, REQ-ECO.CI-006]
covers: []
depends_on: [ECO.REPO, ECO.RULES, ECO.COMM]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.CI.GATES: Path-scoped check planning and CI-change isolation

## Purpose

Decides, from the list of files a PR changes, which governed checks must run (layering, house rules, spec tree, app list, parity drift and any later gate), and fails a PR that mixes CI workflow changes with other changes. It is a small planner that the reusable workflows call first, so the mapping from paths to checks lives in one reviewed file instead of being scattered across workflow conditions.

## Scope

**In:**
- `ci-gates.toml`: each gate's ID, the command that runs it and the path globs it governs
- The planner `cargo xtask ci-plan`, which outputs the gates to run as JSON
- The CI-change isolation rule

**Out:**
- The gates' own logic (owned by `ECO.REPO`, `ECO.RULES`, `ECO.COMM`, `ECO.APPS.LIST`, `ECO.PARITY`)
- Running the planned gates as jobs (owned by `ECO.CI.WF`)

## Requirements

- **REQ-ECO.CI.GATES-001** When given a list of changed paths, the planner shall output every gate whose globs match at least one path, plus every gate marked `always = true`, sorted by gate ID.
- **REQ-ECO.CI.GATES-002** If `ci-gates.toml` itself or any shared check tool changes, then the planner shall select every gate.
- **REQ-ECO.CI.GATES-003** If the changed paths include at least one CI path (default globs `.github/workflows/**`, `.github/actions/**`, `ci/**`) and at least one other path, then the planner shall exit 1 with finding `CI-MIXED` listing the non-CI paths.
- **REQ-ECO.CI.GATES-004** The default gate set shall map `layers.toml`, `kernel-allowlist.toml`, any `Cargo.toml` and `Cargo.lock` to the layering gate; `AGENTS.md`, `CLAUDE.md` and the house-rules source to the rules gate; `spec/**`, `seeds/**` and `decisions/**` to the spec-tree gate; and `apps.toml` to the app-list gate.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gates/ci-gates.toml` and changed-file list `fixtures/gates/kernel-change.txt` (`crates/fid-krn/Cargo.toml`), when `cargo xtask ci-plan --config fixtures/gates/ci-gates.toml --changed fixtures/gates/kernel-change.txt --json` runs, then it exits 0 and prints `{"gates":["layers"]}` plus any `always` gates.
- **AC-2** Given `fixtures/gates/spec-change.txt` (`spec/eco/NODE.md`, `decisions/D-010-x.md`), when the planner runs, then the output contains `spec` and no `layers`.
- **AC-3** (error path) Given `fixtures/gates/mixed.txt` (`.github/workflows/ci.yml` and `src/lib.rs`), when the planner runs, then it exits 1 with finding `CI-MIXED src/lib.rs` and prints no plan.
- **AC-4** Given `fixtures/gates/config-change.txt` (`ci-gates.toml`), when the planner runs, then every configured gate is selected.
- **AC-5** Given `fixtures/gates/ci-only.txt` (only workflow files), when the planner runs, then it exits 0.
- **AC-6** (determinism) The same inputs in a shuffled order produce byte-identical output.

## Design

- **Registry command(s):** none; repository tool `cargo xtask ci-plan --config <file> --changed <file> [--json]`.
- **Data structures:** `[[gate]] id, run, globs = [...], always = false`; `[isolation] ci_globs = [...]`.
- **Algorithm:** read config (bounded, 64 KiB) and changed list (bounded, 100 000 lines); normalise paths to forward slashes; glob-match with a fixed matcher (`**`, `*`, `?` only); apply rules REQ-002 and REQ-003; sort and emit.
- **Errors:** `CI-MIXED`, `GATES-CONFIG` (bad or unknown keys), `GATES-IO`.
- **Determinism:** D0; output sorted, no clock or environment reads.
- **Performance budget:** under 1 s for 10 000 changed paths [unverified until measured].
- **Crate:** `fid-xtask`.

## Test fixtures

Hand-written config and changed-file lists under `fixtures/gates/` (MIT OR Apache-2.0).

## Build prompt

> In the implementation repository, add `ci-plan` to the `fid-xtask` crate. Inputs: `--config ci-gates.toml` (`[[gate]]` with `id`, `run`, `globs`, optional `always`; `[isolation] ci_globs`, default `.github/workflows/**`, `.github/actions/**`, `ci/**`) and `--changed <file>` (one repo-relative path per line). Output (`--json`): `{"gates":[...]}` sorted by id. Rules: select a gate if any changed path matches its globs or it is `always`; if `ci-gates.toml` or any path under `xtask/**` changed, select every gate; if changed paths include both CI paths and non-CI paths, exit 1 with `CI-MIXED <sorted non-CI paths>` and no plan. Ship a default `ci-gates.toml` with gates `layers` (`layers.toml`, `kernel-allowlist.toml`, `**/Cargo.toml`, `Cargo.lock`), `rules` (`AGENTS.md`, `CLAUDE.md`, `house-rules/**`), `spec` (`spec/**`, `seeds/**`, `decisions/**`) and `apps` (`apps.toml`). Implement your own small glob matcher supporting `**`, `*`, `?`. Bounded inputs (64 KiB config, 100 000 paths), fail closed on unknown keys. Write fixtures and tests for AC-1..AC-6 from spec node ECO.CI.GATES. No `unwrap`/`expect`/`panic`/`unsafe` in non-test code.

## Open questions

- Gates for parity drift (ECO.PARITY.DRIFT) and later checks are added to `ci-gates.toml` by their own leaves.
- Follow-up from ECO.APPS.POLICY (PR #116): add an `apps-policy` gate running `cargo xtask apps-policy --workspace . --registry-manifests <dir> --apps-status <json>` once app crates, per-app registry manifests (`fid --app <id> registry show-manifest --full`) and `apps-status.json` (ECO.APPS.MANIFEST.EXPORT) exist; globs `layers.toml`, `apps.toml`, `apps-policy.toml`, `crates/**`. Until then the check is not run on main.

## Children

_Leaf: none._
