---
id: ECO.CI.LINT
title: Format and never-crash lint gate
level: 3
parent: ECO.CI
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.CI-002]
covers: []
depends_on: [ECO.REPO.LAYERS]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.CI.LINT: Format and never-crash lint gate

## Purpose

The gate that enforces CONSTITUTION §8 "never crash" at build time: formatting, the denied panic-family lints, the ban on `unsafe`, and a reviewed exception list that the kernel may never appear in. It also stops crates from silently switching the lints off with local `allow` attributes.

## Scope

**In:**
- Workspace lint configuration (deny `unwrap_used`, `expect_used`, `panic`, `todo`, `unimplemented`, `unreachable`; forbid `unsafe_code`)
- The named exception list `lint-exceptions.toml` and its validation
- A source scan that rejects local `allow`/`expect` attributes for the denied lints outside the exception list
- The `cargo xtask lint-gate` command that runs format check, clippy and the scan

**Out:**
- Numeric-profile lints inside the kernel (KNP-1) (owned by `KRN`)
- Layer assignments (owned by `ECO.REPO.LAYERS`; this gate reads them)
- The process rule that every crash fix has a regression test (stated in the house rules, owned by `ECO.RULES`; reviewers enforce it)

## Requirements

- **REQ-ECO.CI.LINT-001** If any file is not formatted per the workspace formatter configuration, then the lint gate shall fail and list the files.
- **REQ-ECO.CI.LINT-002** If non-test code in any first-party crate triggers `unwrap_used`, `expect_used`, `panic`, `todo`, `unimplemented` or `unreachable`, or contains `unsafe`, then the lint gate shall fail unless an entry in `lint-exceptions.toml` names that crate and lint.
- **REQ-ECO.CI.LINT-003** If `lint-exceptions.toml` names a crate in the kernel layer, or an entry lacks a `reason` and an `owner`, then the lint gate shall fail.
- **REQ-ECO.CI.LINT-004** If a source file contains an `allow`, `expect` or `warn` lint attribute for any denied lint and its crate and lint are not in the exception list, then the lint gate shall fail and name the file and line.
- **REQ-ECO.CI.LINT-005** Code under `#[cfg(test)]`, in `tests/`, `benches/` or `examples/` shall be exempt from REQ-002.

## Acceptance criteria

- **AC-1** Given fixture workspace `fixtures/lint/clean/` (two crates, no denied constructs), when `cargo xtask lint-gate --workspace fixtures/lint/clean` runs, then it exits 0.
- **AC-2** (error path) Given `fixtures/lint/unwrap/` (one `.unwrap()` in `src/lib.rs`), when the gate runs, then it exits 1 and the output names `clippy::unwrap_used` with file and line.
- **AC-3** Given `fixtures/lint/test-only/` (`.unwrap()` only inside `#[cfg(test)]` and in `tests/`), when the gate runs, then it exits 0.
- **AC-4** (error path) Given `fixtures/lint/local-allow/` (`#![allow(clippy::panic)]` in a non-excepted crate), when the gate runs, then it exits 1 with finding `LINT-LOCAL-ALLOW`.
- **AC-5** (error path) Given `fixtures/lint/kernel-exception/` (exception entry for a crate whose layer is `kernel` in its `layers.toml`), when the gate runs, then it exits 1 with finding `LINT-KERNEL-EXCEPTION`.
- **AC-6** Given `fixtures/lint/excepted/` (an `unsafe` block in a crate listed with lint `unsafe_code`, reason and owner), when the gate runs, then it exits 0 and prints the exception as a notice.
- **AC-7** (error path) Given `fixtures/lint/unformatted/`, when the gate runs, then it exits 1 and names the unformatted file.

## Design

- **Registry command(s):** none; repository tool `cargo xtask lint-gate [--workspace <dir>]`.
- **Data structures:** `[workspace.lints]` in the root `Cargo.toml`; `lint-exceptions.toml` with `[[exception]] crate, lint, reason, owner, review_by`.
- **Algorithm:** (1) run the formatter in check mode; (2) run clippy with the workspace lints over non-test targets, with crate-level overrides generated from the exception list; (3) scan `.rs` files outside test paths with a small tokenizer for `#[allow(`, `#![allow(`, `#[expect(` and `#[warn(` naming a denied lint; (4) validate the exception list against the layer map (`layers.toml`, ECO.REPO.LAYERS); (5) print findings sorted by file, line, code.
- **Errors:** `LINT-FMT`, `LINT-DENIED`, `LINT-LOCAL-ALLOW`, `LINT-KERNEL-EXCEPTION`, `LINT-EXCEPTION-SCHEMA`, `LINT-TOOL` (formatter or clippy missing).
- **Determinism:** D0 for the gate's own output given the tool versions pinned in `rust-toolchain.toml`.
- **Performance budget:** dominated by clippy; the scan adds under 5 s on 500 000 lines [unverified until measured].
- **Crate:** `fid-xtask`.

## Test fixtures

Small fixture workspaces under `fixtures/lint/` written for this leaf (MIT OR Apache-2.0); each has its own `layers.toml`.

## Build prompt

> In the implementation repository, add `lint-gate` to `fid-xtask`. Configure the root `Cargo.toml` with `[workspace.lints.clippy]` denying `unwrap_used`, `expect_used`, `panic`, `todo`, `unimplemented`, `unreachable`, and `[workspace.lints.rust] unsafe_code = "forbid"`; every first-party crate inherits workspace lints. Add `lint-exceptions.toml` (`[[exception]] crate, lint, reason, owner, review_by`). The gate: (1) formatter check; (2) clippy on non-test targets with the exception list applied per crate; (3) tokenizer scan of `.rs` files outside `#[cfg(test)]` modules, `tests/`, `benches/`, `examples/` for lint attributes (`allow`, `expect`, `warn`) naming a denied lint, failing with `LINT-LOCAL-ALLOW` unless excepted; (4) refuse any exception for a crate in the `kernel` layer of `layers.toml` (`LINT-KERNEL-EXCEPTION`) or with empty `reason`/`owner`. Findings sorted by file, line, code; exit 0/1. Write fixture workspaces under `fixtures/lint/` and tests for AC-1..AC-7 of spec node ECO.CI.LINT. The tool itself obeys the same lints (no `unwrap`/`expect`/`panic`/`unsafe`).

## Open questions

- Exact clippy option for allowing `unwrap` in tests (`allow-unwrap-in-tests`) and its interaction with `#[cfg(test)]` helpers in non-test files [unverified]; the source scan covers the gap either way.

## Children

_Leaf: none._
