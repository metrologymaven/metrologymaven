---
id: OUTR.DOCS.GUIDE.WASM
title: Safe-Rust WASM Field Guide kernel runner
level: 4
parent: OUTR.DOCS.GUIDE
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.DOCS.GUIDE-001]
covers: []
depends_on: [KRN.LSQ.RND, KRN.LSQ.LIN, KRN.ENV.TAN.PLANE, KRN.ENV.MCC.CYL, GDT.ZONE.ZTYPE, UNC.DEC.EVAL, UNC.DEC.RISK, UNC.PROP.MCM.INTVL, UNC.PROP.VALID, CORE.JRN.CANON]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-22
---

# OUTR.DOCS.GUIDE.WASM: Safe-Rust WASM Field Guide kernel runner

## Purpose

A pure safe-Rust (`#[forbid(unsafe_code)]`) executable crate (`crates/fid-guide-wasm`) that compiles both natively and to `wasm32-wasip1`, reading one bounded JSON request on stdin and writing one canonical JSON response on stdout. It exposes live, real kernel computations from `fid-fit`, `fid-gps`, `fid-kunc`, `fid-kdec` and `fid-canon` so interactive Field Guide browser sandboxes and CI checks run the exact deterministic metrology kernel without third-party dependencies or `unsafe` FFI glue.

## Scope

**In:**
- Crate `crates/fid-guide-wasm` (layer `tool` in `layers.toml`, linking only L0/model crates: `fid-fit`, `fid-gps`, `fid-kunc`, `fid-kdec`, `fid-canon`, `fid-num`, `fid-types`)
- Bounded JSON stdin/stdout protocol (`MAX_REQUEST_BYTES = 262_144`) with zero panics and typed error JSON
- Seven deterministic lab operations: `fit.circle2d`, `fit.plane_datum`, `fit.cylinder_mcc`, `gdt.zone_eval`, `unc.propagate`, `dec.evaluate`, `canon.digest`
- Native unit and integration tests plus `wasm32-wasip1` compilation verification

**Out:**
- Browser JavaScript WASI host and HTML rendering (`OUTR.DOCS.GUIDE.SITE`)
- Editorial chapter text (`OUTR.DOCS.GUIDE.TOPICS`)
- The kernel verification reference command (`KRN.MAN.WASM`, `crates/fid-kernel-wasm`)

## Requirements

- **REQ-OUTR.DOCS.GUIDE.WASM-001** The crate shall compile under workspace lints (`unsafe_code = "forbid"`, deny `unwrap_used`, `expect_used`, `panic`) for both native targets and `wasm32-wasip1` with zero new external dependencies.
- **REQ-OUTR.DOCS.GUIDE.WASM-002** The runner shall read at most `MAX_REQUEST_BYTES` from stdin, parse requests using `fid-canon` (fail-closed on hostile, malformed, oversized or NaN inputs), and return a canonical JSON response with its RFC 8785 SHA-256 `result_digest`.
- **REQ-OUTR.DOCS.GUIDE.WASM-003** Every lab operation shall invoke the real underlying kernel crate (`fid-fit`, `fid-gps`, `fid-kunc`, `fid-kdec`, `fid-canon`) rather than a simplified mock, and return identical output bytes across repeated runs (D0/D1).
- **REQ-OUTR.DOCS.GUIDE.WASM-004** Operations involving GD&T interpretation (`fit.plane_datum`, `gdt.zone_eval`) shall include an explicit `interpretation_status: "provisional"` field per D-045.

## Acceptance criteria

- **AC-1** Native `cargo test -p fid-guide-wasm` exercises all 7 operations (`fit.circle2d`, `fit.plane_datum`, `fit.cylinder_mcc`, `gdt.zone_eval`, `unc.propagate`, `dec.evaluate`, `canon.digest`) and confirms bit-identical outputs on repeated calls and exact agreement with underlying crate calls.
- **AC-2** (error paths) Malformed JSON, unknown `op`, oversized payloads (>256 KiB), insufficient points (<3 for circle), and invalid tolerances return typed `{"ok":false,"error":...}` JSON with exit code 1 or 2 and never panic.
- **AC-3** `cargo build --release --target wasm32-wasip1 -p fid-guide-wasm` succeeds cleanly with zero warnings, and `cargo xtask layers`, `cargo xtask kernel-allow`, and `cargo xtask lint-crate-graph` exit 0.
- **AC-4** `cargo clippy -p fid-guide-wasm --all-targets -- -D warnings` and `cargo fmt --all --check` exit 0.

## Design

- **Crate:** `crates/fid-guide-wasm`, `layers.toml` entry `"fid-guide-wasm" = "tool"`.
- **Dependencies:** Only workspace crates `fid-fit`, `fid-gps`, `fid-kunc`, `fid-kdec`, `fid-canon`, `fid-num`, `fid-types`. Zero external crates added to `Cargo.lock`.
- **Protocol:** Input JSON `{"op": "<op>", "params": {...}}` parsed via `fid_canon::parse_with_limits`; output JSON serialized via `fid_canon::to_canonical_bytes` with `ok`, `op`, `result`, `result_digest` (or `ok: false`, `code`, `message`).
- **Operations:**
  - `fit.circle2d`: invokes `fid_fit::ls_rnd::Circle2Ls` on 2D points (with optional outlier index to compute before/after impact disclosure) and reports centre, radius, diameter, radial RMS, roundness peak-to-valley span, and per-point residuals.
  - `fit.plane_datum`: invokes `fid_fit::ls_lin::PlaneLs` and `fid_fit::tan_plane` (`TanPlane`) plus `fid_fit::tan_rock` rocking report to contrast Gaussian plane vs. tangential datum simulator (`interpretation_status: "provisional"`).
  - `fit.cylinder_mcc`: invokes `fid_fit::ls_cyl::CylinderLs` and `fid_fit::mcc_cyl` minimum circumscribed cylinder to contrast Gaussian vs. ASME Rule #1 mating envelope.
  - `gdt.zone_eval`: invokes `fid_gps::zone::shape::ZoneShape::{usage, contains}` across `two_planes`, `cylinder`, `circle`, `sphere`, and `offset` zones (`interpretation_status: "provisional"`).
  - `unc.propagate`: constructs typed `fid_kunc::dist` input distributions (Normal, Rectangular, Triangular, U-shaped), runs linear propagation (LPU) and seeded Monte Carlo sampling through `fid_kunc::prop::mcm::interval::summarize` and `fid_kunc::prop::valid::validate_lpu`, returning standard uncertainty, expanded 95% coverage interval, histogram bins, nonlinearity verdict, and UEC evidence class.
  - `dec.evaluate`: invokes `fid_kdec::evaluate` and `fid_kdec::compute_risk` across `SimpleAcceptance`, `GuardBand` (strict/relaxed multiplier `w`), and `SharedRisk` rules, returning outcome (`CONFORMS | NONCONFORMS | INDETERMINATE | NOT_DECIDABLE`), acceptance zone, conformance probability, and specific/global consumer and producer risk.
  - `canon.digest`: invokes `fid_canon::parse`, `fid_canon::to_canonical_bytes`, and `fid_canon::digest_bytes` to show exact RFC 8785 canonical JSON and SHA-256 replay identity.
- **Determinism:** D0 (pure KNP-1 `f64` and exact integer/canonical JSON).

## Test fixtures

Inline deterministic point sets, uncertainty budgets, decision inputs and malformed payloads inside `crates/fid-guide-wasm/tests/lab_ops.rs` (authored in-project, CC-BY-4.0 / MIT OR Apache-2.0).

## Build prompt

> Create crate `crates/fid-guide-wasm` (layer `tool` in `layers.toml`), depending only on workspace crates `fid-fit`, `fid-gps`, `fid-kunc`, `fid-kdec`, `fid-canon`, `fid-num` and `fid-types`. Implement a safe-Rust (`#[forbid(unsafe_code)]`, zero panics, zero `unwrap`/`expect`) stdin/stdout JSON runner (`lib.rs` exposing `dispatch(&[u8]) -> (u8, Vec<u8>)` and `main.rs` reading up to 262,144 bytes from stdin and writing stdout) supporting `fit.circle2d`, `fit.plane_datum`, `fit.cylinder_mcc`, `gdt.zone_eval`, `unc.propagate`, `dec.evaluate`, and `canon.digest`. Every operation must call the real underlying workspace crate, attach an RFC 8785 SHA-256 `result_digest`, and tag GD&T operations `interpretation_status: "provisional"`. Verify AC-1..AC-4 natively and compile cleanly to `wasm32-wasip1`.

## Children

_Leaf: none._
