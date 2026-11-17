---
id: KRN.NUM.LINT.TYPED
title: Type-aware banned-API check on expanded workspace code
level: 4
parent: KRN.NUM.LINT
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-KRN.NUM.LINT-002, REQ-KRN.NUM.LINT-007]
covers: []
depends_on: [KRN.NUM.LINT.FIND, KRN.NUM.LINT.GRAPH]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# KRN.NUM.LINT.TYPED: Type-aware banned-API check on expanded workspace code

## Purpose

A syntax scan cannot know the type of a receiver or see code produced by macro expansion. This leaf closes that gap for the kernel's own and vendored crates (workspace members) by running the compiler's lint driver with generated disallowed-method, disallowed-type and disallowed-macro lists, which resolve paths after expansion and type inference, and converting its diagnostics into lint findings.

## Scope

**In:**
- Generating a `clippy.toml` (disallowed methods, types and macros) from the KNP-1 rule table
- Running `cargo clippy` with only those lints denied, on every workspace crate in the kernel closure, for each kernel target (x86_64, aarch64, wasm32)
- Mapping JSON diagnostics (span file, line, column) back to rule IDs and `Finding`s, including spans that originate in macro expansions
- A seeded fixture for the `.max(` on a float variable case and a macro-expanded `mul_add`

**Out** (and which node owns it instead):
- Third-party registry crates, which clippy does not lint (`KRN.NUM.LINT.SYN`)
- Build-flag checks (`KRN.NUM.LINT.FLAGS`)
- Workspace-wide lints outside the kernel such as `unwrap` denial (`ECO.CI`)

## Requirements

- **REQ-KRN.NUM.LINT.TYPED-001** The generated lint config shall list, as disallowed, every method, type and macro in the KNP-1 rule table that has a resolvable path (for example `f64::mul_add`, `f64::powi`, `f64::min`, `f64::max`, `std::collections::HashMap`, `std::collections::HashSet`).
- **REQ-KRN.NUM.LINT.TYPED-002** When the check runs, it shall lint every workspace crate in the closure for x86_64, aarch64 and wasm32 targets and shall map each disallowed-item diagnostic to a finding with the matching rule ID and the span of the user-written code (the macro call site when the item comes from an expansion).
- **REQ-KRN.NUM.LINT.TYPED-003** If the lint driver fails to build a crate, then the check shall report `KNP1-TYPED-BUILD` for that crate and target and count it as unwaived.
- **REQ-KRN.NUM.LINT.TYPED-004** The check's seeded fixtures (float-variable `.max(`, macro-expanded `mul_add`, aliased `HashMap`) shall each be detected.

## Acceptance criteria

- **AC-1** Given `fixtures/knp1/typed/minmax-var/` (`let a: f64 = 1.0; let b = a.max(2.0);`), when the check runs, then a `KNP1-MINMAX` finding is reported at that line.
- **AC-2** Given `fixtures/knp1/typed/macro/` (a local macro expanding to `x.mul_add(y, z)` used at line 20), then a `KNP1-FMA` finding is reported with a span at or inside line 20's call site.
- **AC-3** Given `fixtures/knp1/typed/clean/` (uses `i32::max` and `BTreeMap`), then no finding.
- **AC-4** (error path) Given a fixture crate that does not compile for wasm32, then `KNP1-TYPED-BUILD` names the crate and target.

## Design

- **Registry command(s):** none; module `typed` of tooling crate `fid-knp1-lint`, invoking the pinned toolchain's `cargo clippy --message-format=json` as a subprocess.
- **Data structures:** generated `clippy.toml` with `disallowed-methods`, `disallowed-types`, `disallowed-macros` (each with a `reason` naming the rule ID); diagnostic parser for the JSON message stream.
- **Algorithm:** write the config to a temp dir; for each target in sorted order run clippy with `-A clippy::all -D clippy::disallowed_methods -D clippy::disallowed_types -D clippy::disallowed_macros`; parse diagnostics; take the outermost user-code span from the expansion chain; deduplicate by (rule, file, line, column) across targets.
- **Errors:** `KNP1-TYPED-BUILD`, `KNP1-TYPED-DRIVER` (clippy missing or wrong version: fatal).
- **Determinism:** D0 for the findings set, given the pinned toolchain.
- **Performance budget:** dominated by compilation; < 5 min for the kernel workspace on CI [unverified until measured].
- **Crate:** `fid-knp1-lint` (tooling layer).

## Test fixtures

`fixtures/knp1/typed/`: small crates for AC-1..AC-4, project licence.

## Build prompt

> In tooling crate `fid-knp1-lint`, add module `typed`. Generate a `clippy.toml` with `disallowed-methods` (`f64::mul_add`, `f32::mul_add`, `f64::powi`, `f64::powf`, `f64::min`, `f64::max`, `f64::clamp`, the float transcendental methods), `disallowed-types` (`std::collections::HashMap`, `std::collections::HashSet`, `std::collections::hash_map::RandomState`) and `disallowed-macros` (`std::is_x86_feature_detected`, `std::arch::is_aarch64_feature_detected`), each `reason` naming its KNP-1 rule ID. For each workspace crate in the `graph` closure and each target (x86_64, aarch64, wasm32), run the pinned `cargo clippy --message-format=json` with only those three lints denied; map diagnostics to `findings::Finding` using the outermost user-code span; dedupe across targets. Build failure: `KNP1-TYPED-BUILD`; missing driver: fatal `KNP1-TYPED-DRIVER`. The `fid-num` facade implementation files are exempt from the transcendental entries only. Tests: `fixtures/knp1/typed/` for AC-1..AC-4 of spec node KRN.NUM.LINT.TYPED. Constraints (CONSTITUTION §8): no `unsafe`, deny `unwrap`/`expect`/`panic`.

## Open questions

- Whether clippy's disallowed-method lint reports calls produced inside external macro expansions on every toolchain version [unverified]; pin the toolchain and keep AC-2 as the regression test.

## Children

_Leaf: none._
