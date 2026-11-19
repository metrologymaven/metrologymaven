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
built_in: [fid-knp1-lint]
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

### Build decisions (fid-knp1-lint `typed`)

- **Driver invocation.** One `cargo clippy` run per target (sorted), over every workspace member of the closure at once: `--manifest-path <workspace>/Cargo.toml --locked --keep-going --message-format=json --target <triple> --target-dir <scratch> -p <name>@<version> … -- -A clippy::all --force-warn clippy::disallowed_methods --force-warn clippy::disallowed_types --force-warn clippy::disallowed_macros`, with `CLIPPY_CONF_DIR` pointing at the generated `clippy.toml`. Deviation from the Design's `-D`: `--force-warn` cannot be lowered by `#[allow(...)]` in the source, and as warnings a hit does not stop the build of the crates that depend on it. Default targets: `aarch64-unknown-linux-gnu`, `wasm32-unknown-unknown`, `x86_64-unknown-linux-gnu`.
- **`KNP1-TYPED-BUILD` (never waivable).** A crate is linted for a target only if cargo reports a compiled artifact for it. A compile error, or no artifact (stale lockfile, target not installed, timeout, unreadable driver output), is `KNP1-TYPED-BUILD` at the crate's `Cargo.toml` 1:1, naming crate and target. It joins the FIND catalogue as the family `KNP1-TYPED-*`.
- **`KNP1-TYPED-DRIVER` (fatal).** `cargo clippy --version` fails or does not print the pinned `[typed] clippy_version` (required), or a disallowed-item diagnostic names no KNP-1 rule (each `reason` is the rule ID).
- **Spans.** The finding is placed at the outermost span of the primary span's macro expansion chain that lies inside the crate root (the call site of a local macro). A diagnostic with no span inside the crate is placed at `Cargo.toml` 1:1. Findings are deduplicated across targets by (crate, version, file, line, column, rule).
- **Facade.** `KNP1-LIBM` findings in the `[syn] facade_files` (crate, version, file) are notes; every other rule applies there.
- **Scope.** Only `SourceKind::Workspace` crates of the closure are linted; vendored and registry crates are left to `KRN.NUM.LINT.SYN`.
- **Table.** The Build prompt's list, extended with every KNP-1 item that has a resolvable std path (f32 and f64 alike); nightly-only methods (`gamma`, `ln_gamma`) are left out because their paths do not resolve on the pinned stable toolchain. `clippy.toml` lists entries in table order as `{ path = "…", reason = "<rule ID>" }`. CI installs the three targets for the test job.
- **Accepted gaps.** A configured path that does not resolve on a target (for example `std::arch::is_aarch64_feature_detected` on x86-64) is only a clippy warning, not a finding [unverified per clippy version]; `build.rs` output and proc-macro expansion in third-party crates stay with `KRN.NUM.LINT.SYN`.

#### Pinned table

`TYPED_TABLE` in `crates/fid-knp1-lint/src/typed.rs`; the test `table_is_pinned_in_node_md` checks it row for row, and `fixtures/knp1/typed/make_expected.py` renders the expected `clippy.toml` independently.

| Rule | Kind | Path |
|---|---|---|
| `KNP1-FMA` | method | `f32::mul_add` |
| `KNP1-FMA` | method | `f64::mul_add` |
| `KNP1-POW` | method | `f32::powf` |
| `KNP1-POW` | method | `f32::powi` |
| `KNP1-POW` | method | `f64::powf` |
| `KNP1-POW` | method | `f64::powi` |
| `KNP1-LIBM` | method | `f32::acos` |
| `KNP1-LIBM` | method | `f32::acosh` |
| `KNP1-LIBM` | method | `f32::asin` |
| `KNP1-LIBM` | method | `f32::asinh` |
| `KNP1-LIBM` | method | `f32::atan` |
| `KNP1-LIBM` | method | `f32::atan2` |
| `KNP1-LIBM` | method | `f32::atanh` |
| `KNP1-LIBM` | method | `f32::cbrt` |
| `KNP1-LIBM` | method | `f32::cos` |
| `KNP1-LIBM` | method | `f32::cosh` |
| `KNP1-LIBM` | method | `f32::exp` |
| `KNP1-LIBM` | method | `f32::exp2` |
| `KNP1-LIBM` | method | `f32::exp_m1` |
| `KNP1-LIBM` | method | `f32::hypot` |
| `KNP1-LIBM` | method | `f32::ln` |
| `KNP1-LIBM` | method | `f32::ln_1p` |
| `KNP1-LIBM` | method | `f32::log` |
| `KNP1-LIBM` | method | `f32::log10` |
| `KNP1-LIBM` | method | `f32::log2` |
| `KNP1-LIBM` | method | `f32::sin` |
| `KNP1-LIBM` | method | `f32::sin_cos` |
| `KNP1-LIBM` | method | `f32::sinh` |
| `KNP1-LIBM` | method | `f32::sqrt` |
| `KNP1-LIBM` | method | `f32::tan` |
| `KNP1-LIBM` | method | `f32::tanh` |
| `KNP1-LIBM` | method | `f64::acos` |
| `KNP1-LIBM` | method | `f64::acosh` |
| `KNP1-LIBM` | method | `f64::asin` |
| `KNP1-LIBM` | method | `f64::asinh` |
| `KNP1-LIBM` | method | `f64::atan` |
| `KNP1-LIBM` | method | `f64::atan2` |
| `KNP1-LIBM` | method | `f64::atanh` |
| `KNP1-LIBM` | method | `f64::cbrt` |
| `KNP1-LIBM` | method | `f64::cos` |
| `KNP1-LIBM` | method | `f64::cosh` |
| `KNP1-LIBM` | method | `f64::exp` |
| `KNP1-LIBM` | method | `f64::exp2` |
| `KNP1-LIBM` | method | `f64::exp_m1` |
| `KNP1-LIBM` | method | `f64::hypot` |
| `KNP1-LIBM` | method | `f64::ln` |
| `KNP1-LIBM` | method | `f64::ln_1p` |
| `KNP1-LIBM` | method | `f64::log` |
| `KNP1-LIBM` | method | `f64::log10` |
| `KNP1-LIBM` | method | `f64::log2` |
| `KNP1-LIBM` | method | `f64::sin` |
| `KNP1-LIBM` | method | `f64::sin_cos` |
| `KNP1-LIBM` | method | `f64::sinh` |
| `KNP1-LIBM` | method | `f64::sqrt` |
| `KNP1-LIBM` | method | `f64::tan` |
| `KNP1-LIBM` | method | `f64::tanh` |
| `KNP1-MINMAX` | method | `f32::clamp` |
| `KNP1-MINMAX` | method | `f32::max` |
| `KNP1-MINMAX` | method | `f32::min` |
| `KNP1-MINMAX` | method | `f64::clamp` |
| `KNP1-MINMAX` | method | `f64::max` |
| `KNP1-MINMAX` | method | `f64::min` |
| `KNP1-IO-FS` | method | `std::fs::read` |
| `KNP1-IO-FS` | method | `std::fs::read_to_string` |
| `KNP1-IO-FS` | method | `std::fs::write` |
| `KNP1-CLOCK` | method | `std::time::Instant::now` |
| `KNP1-CLOCK` | method | `std::time::SystemTime::now` |
| `KNP1-ENV` | method | `std::env::var` |
| `KNP1-ENV` | method | `std::env::var_os` |
| `KNP1-ENV` | method | `std::env::vars` |
| `KNP1-THREAD` | method | `std::process::Command::new` |
| `KNP1-THREAD` | method | `std::thread::spawn` |
| `KNP1-HASH` | type | `std::collections::HashMap` |
| `KNP1-HASH` | type | `std::collections::HashSet` |
| `KNP1-HASH` | type | `std::collections::hash_map::DefaultHasher` |
| `KNP1-HASH` | type | `std::collections::hash_map::RandomState` |
| `KNP1-IO-FS` | type | `std::fs::File` |
| `KNP1-IO-NET` | type | `std::net::TcpListener` |
| `KNP1-IO-NET` | type | `std::net::TcpStream` |
| `KNP1-IO-NET` | type | `std::net::UdpSocket` |
| `KNP1-CLOCK` | type | `std::time::Instant` |
| `KNP1-CLOCK` | type | `std::time::SystemTime` |
| `KNP1-SIMD` | macro | `std::arch::is_aarch64_feature_detected` |
| `KNP1-SIMD` | macro | `std::is_x86_feature_detected` |
| `KNP1-ENV` | macro | `std::env` |
| `KNP1-ENV` | macro | `std::option_env` |
| `KNP1-IO-FS` | macro | `std::include_bytes` |
| `KNP1-IO-FS` | macro | `std::include_str` |

## Test fixtures

`fixtures/knp1/typed/`: small crates for AC-1..AC-4, project licence.

## Build prompt

> In tooling crate `fid-knp1-lint`, add module `typed`. Generate a `clippy.toml` with `disallowed-methods` (`f64::mul_add`, `f32::mul_add`, `f64::powi`, `f64::powf`, `f64::min`, `f64::max`, `f64::clamp`, the float transcendental methods), `disallowed-types` (`std::collections::HashMap`, `std::collections::HashSet`, `std::collections::hash_map::RandomState`) and `disallowed-macros` (`std::is_x86_feature_detected`, `std::arch::is_aarch64_feature_detected`), each `reason` naming its KNP-1 rule ID. For each workspace crate in the `graph` closure and each target (x86_64, aarch64, wasm32), run the pinned `cargo clippy --message-format=json` with only those three lints denied; map diagnostics to `findings::Finding` using the outermost user-code span; dedupe across targets. Build failure: `KNP1-TYPED-BUILD`; missing driver: fatal `KNP1-TYPED-DRIVER`. The `fid-num` facade implementation files are exempt from the transcendental entries only. Tests: `fixtures/knp1/typed/` for AC-1..AC-4 of spec node KRN.NUM.LINT.TYPED. Constraints (CONSTITUTION §8): no `unsafe`, deny `unwrap`/`expect`/`panic`.

## Open questions

- Whether clippy's disallowed-method lint reports calls produced inside external macro expansions on every toolchain version [unverified]; pin the toolchain and keep AC-2 as the regression test.

## Children

_Leaf: none._
