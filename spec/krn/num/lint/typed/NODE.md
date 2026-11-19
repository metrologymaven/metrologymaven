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
built_pr: 235
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

### Build decisions (fid-knp1-lint `typed`, PR #235)

- **Driver invocation.** For every target (sorted) and each of two feature sets (default features, then `--all-features`), one `cargo clippy` run over every workspace member of the closure at once: `--manifest-path <workspace>/Cargo.toml --locked --keep-going --message-format=json --target <triple> --target-dir <scratch> [--all-features] -p <name>@<version> … -- -A clippy::all --force-warn clippy::disallowed_methods --force-warn clippy::disallowed_types --force-warn clippy::disallowed_macros`, with `CLIPPY_CONF_DIR` pointing at the generated `clippy.toml`. Deviation from the Design's `-D`: `--force-warn` cannot be lowered by `#[allow(...)]` in the source, and as warnings a hit does not stop the build of the crates that depend on it. Default targets: `aarch64-apple-darwin`, `aarch64-unknown-linux-gnu`, `wasm32-unknown-unknown`, `x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu` (CI installs all five). Features other than the default set and the full set (single non-default features in isolation) are not linted separately.
- **Environment.** The driver gets `CARGO_ENCODED_RUSTFLAGS` set to empty, which takes precedence over `RUSTFLAGS` and over every `build.rustflags` / `target.<…>.rustflags` of `.cargo/config.toml`. `RUSTFLAGS`, `CARGO_BUILD_RUSTFLAGS`, `RUSTC_WRAPPER`, `RUSTC_WORKSPACE_WRAPPER`, `CARGO_BUILD_RUSTC_WRAPPER`, `CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER`, `CLIPPY_ARGS` and every `CARGO_TARGET_*_RUSTFLAGS` / `CARGO_TARGET_*_RUNNER` are removed. Other `.cargo/config.toml` settings (for example `[build] rustc` or `[target.*] linker`) are not neutralised; `KRN.NUM.LINT.FLAGS` checks that file.
- **`KNP1-TYPED-BUILD` (never waivable).** A crate is linted for a run only if cargo reports a compiled artifact for it whose `target.kind` is a library, binary or proc-macro kind (`lib`, `rlib`, `dylib`, `cdylib`, `staticlib`, `bin`, `proc-macro`); a build script (`custom-build`) does not count. A compile error, or no such artifact (stale lockfile, target not installed, timeout, driver failure), is `KNP1-TYPED-BUILD` at the crate's `Cargo.toml` 1:1, naming crate, target and feature set. `KNP1-TYPED-*` is a never-waivable FIND family.
- **`KNP1-TYPED-NOSPAN` (never waivable).** A disallowed-item diagnostic with no span inside the crate is reported at the crate's `Cargo.toml` 1:1 under this rule (naming the original rule), so a waiver can never cover it.
- **`KNP1-TYPED-DRIVER` (fatal).** `cargo clippy --version` fails or does not print the pinned `[typed] clippy_version` (required); a line of driver output is not a readable cargo message (the bounded JSON reader accepts nesting up to `MAX_EXPANSION_DEPTH * 2 + 16` = 528 levels, enough for a 256-deep expansion chain); a disallowed-item diagnostic names no KNP-1 rule (each `reason` is the rule ID); or the driver reports a table path that "does not refer to" an item, unless it is on the pinned per-target allowlist: `std::arch::is_aarch64_feature_detected` may be unresolved off `aarch64*` targets, and `std::is_x86_feature_detected` off `x86_64*`, `i686*` and `i586*` targets.
- **`KNP1-TYPED-IO` (fatal).** The scratch directory or the generated `clippy.toml` cannot be written.
- **Spans.** The finding is placed at the outermost span of the primary span's macro expansion chain (up to 256 levels) that lies inside the crate root, which is the call site of a local macro. Findings are deduplicated across targets and feature sets by (crate, version, file, line, column, rule).
- **Facade.** `KNP1-LIBM` findings in the `[syn] facade_files` (crate, version, file) are notes; every other rule applies there.
- **Scope.** Only `SourceKind::Workspace` crates of the closure are linted; vendored and registry crates are left to `KRN.NUM.LINT.SYN`.
- **Table.** The Build prompt's list, extended to f32 and f64 alike and audited against the std `env`, `fs`, `net`, `process`, `thread` and `time` modules for every function, type and macro that reads the environment, the file system, the network or a clock, or starts threads or processes. Nightly-only methods (`gamma`, `ln_gamma`) are left out because their paths do not resolve on the pinned stable toolchain. `clippy.toml` lists entries in table order as `{ path = "…", reason = "<rule ID>" }`.
- **Fixtures** cover a float variable, a local macro, a 70-deep recursive macro, `allow` attributes, target-specific code, `fold(.., f64::max)`, a closure, `type F = f64; F::max`, an associated type, a generic trait impl, an aliased `HashMap`, the facade split, a crate that does not build for wasm32, and the driver-output failure modes.

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
| `KNP1-IO-FS` | method | `std::fs::canonicalize` |
| `KNP1-IO-FS` | method | `std::fs::copy` |
| `KNP1-IO-FS` | method | `std::fs::create_dir` |
| `KNP1-IO-FS` | method | `std::fs::create_dir_all` |
| `KNP1-IO-FS` | method | `std::fs::exists` |
| `KNP1-IO-FS` | method | `std::fs::hard_link` |
| `KNP1-IO-FS` | method | `std::fs::metadata` |
| `KNP1-IO-FS` | method | `std::fs::read` |
| `KNP1-IO-FS` | method | `std::fs::read_dir` |
| `KNP1-IO-FS` | method | `std::fs::read_link` |
| `KNP1-IO-FS` | method | `std::fs::read_to_string` |
| `KNP1-IO-FS` | method | `std::fs::remove_dir` |
| `KNP1-IO-FS` | method | `std::fs::remove_dir_all` |
| `KNP1-IO-FS` | method | `std::fs::remove_file` |
| `KNP1-IO-FS` | method | `std::fs::rename` |
| `KNP1-IO-FS` | method | `std::fs::set_permissions` |
| `KNP1-IO-FS` | method | `std::fs::symlink_metadata` |
| `KNP1-IO-FS` | method | `std::fs::write` |
| `KNP1-CLOCK` | method | `std::time::Instant::now` |
| `KNP1-CLOCK` | method | `std::time::SystemTime::now` |
| `KNP1-ENV` | method | `std::env::args` |
| `KNP1-ENV` | method | `std::env::args_os` |
| `KNP1-ENV` | method | `std::env::current_dir` |
| `KNP1-ENV` | method | `std::env::current_exe` |
| `KNP1-ENV` | method | `std::env::home_dir` |
| `KNP1-ENV` | method | `std::env::join_paths` |
| `KNP1-ENV` | method | `std::env::remove_var` |
| `KNP1-ENV` | method | `std::env::set_current_dir` |
| `KNP1-ENV` | method | `std::env::set_var` |
| `KNP1-ENV` | method | `std::env::split_paths` |
| `KNP1-ENV` | method | `std::env::temp_dir` |
| `KNP1-ENV` | method | `std::env::var` |
| `KNP1-ENV` | method | `std::env::var_os` |
| `KNP1-ENV` | method | `std::env::vars` |
| `KNP1-ENV` | method | `std::env::vars_os` |
| `KNP1-THREAD` | method | `std::process::Command::new` |
| `KNP1-THREAD` | method | `std::process::abort` |
| `KNP1-THREAD` | method | `std::process::exit` |
| `KNP1-THREAD` | method | `std::process::id` |
| `KNP1-THREAD` | method | `std::thread::available_parallelism` |
| `KNP1-THREAD` | method | `std::thread::current` |
| `KNP1-THREAD` | method | `std::thread::park` |
| `KNP1-THREAD` | method | `std::thread::park_timeout` |
| `KNP1-THREAD` | method | `std::thread::scope` |
| `KNP1-THREAD` | method | `std::thread::sleep` |
| `KNP1-THREAD` | method | `std::thread::spawn` |
| `KNP1-THREAD` | method | `std::thread::yield_now` |
| `KNP1-HASH` | type | `std::collections::HashMap` |
| `KNP1-HASH` | type | `std::collections::HashSet` |
| `KNP1-HASH` | type | `std::collections::hash_map::DefaultHasher` |
| `KNP1-HASH` | type | `std::collections::hash_map::RandomState` |
| `KNP1-IO-FS` | type | `std::fs::DirBuilder` |
| `KNP1-IO-FS` | type | `std::fs::File` |
| `KNP1-IO-FS` | type | `std::fs::OpenOptions` |
| `KNP1-IO-NET` | type | `std::net::TcpListener` |
| `KNP1-IO-NET` | type | `std::net::TcpStream` |
| `KNP1-IO-NET` | type | `std::net::UdpSocket` |
| `KNP1-CLOCK` | type | `std::time::Instant` |
| `KNP1-CLOCK` | type | `std::time::SystemTime` |
| `KNP1-THREAD` | type | `std::process::Child` |
| `KNP1-THREAD` | type | `std::process::Command` |
| `KNP1-THREAD` | type | `std::thread::Builder` |
| `KNP1-SIMD` | macro | `std::arch::is_aarch64_feature_detected` |
| `KNP1-SIMD` | macro | `std::is_x86_feature_detected` |
| `KNP1-ENV` | macro | `std::env` |
| `KNP1-ENV` | macro | `std::option_env` |
| `KNP1-IO-FS` | macro | `std::include` |
| `KNP1-IO-FS` | macro | `std::include_bytes` |
| `KNP1-IO-FS` | macro | `std::include_str` |
| `KNP1-THREAD` | macro | `std::thread_local` |

## Test fixtures

`fixtures/knp1/typed/`: small crates for AC-1..AC-4, project licence.

## Build prompt

> In tooling crate `fid-knp1-lint`, add module `typed`. Generate a `clippy.toml` with `disallowed-methods` (`f64::mul_add`, `f32::mul_add`, `f64::powi`, `f64::powf`, `f64::min`, `f64::max`, `f64::clamp`, the float transcendental methods), `disallowed-types` (`std::collections::HashMap`, `std::collections::HashSet`, `std::collections::hash_map::RandomState`) and `disallowed-macros` (`std::is_x86_feature_detected`, `std::arch::is_aarch64_feature_detected`), each `reason` naming its KNP-1 rule ID. For each workspace crate in the `graph` closure and each target (x86_64, aarch64, wasm32), run the pinned `cargo clippy --message-format=json` with only those three lints denied; map diagnostics to `findings::Finding` using the outermost user-code span; dedupe across targets. Build failure: `KNP1-TYPED-BUILD`; missing driver: fatal `KNP1-TYPED-DRIVER`. The `fid-num` facade implementation files are exempt from the transcendental entries only. Tests: `fixtures/knp1/typed/` for AC-1..AC-4 of spec node KRN.NUM.LINT.TYPED. Constraints (CONSTITUTION §8): no `unsafe`, deny `unwrap`/`expect`/`panic`.

## Open questions

- Whether clippy's disallowed-method lint reports calls produced inside external macro expansions on every toolchain version [unverified]; pin the toolchain and keep AC-2 as the regression test.

## Children

_Leaf: none._
