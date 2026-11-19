---
id: KRN.MAN.WASM
title: WASM reference artifact and import check
level: 3
parent: KRN.MAN
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-kernel-wasm, fid-wasm-check]
built_pr: 206
satisfies: [REQ-KRN.MAN-005]
covers: []
depends_on: [KRN.MAN.KCI]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-18
---

# KRN.MAN.WASM: WASM reference artifact and import check

## Purpose

The one byte-identical kernel binary that runs on every platform and computes values of record in P0 and P1a. It is a WebAssembly command module that reads one canonical `KernelCall` from standard input, calls `evaluate`, and writes the `KernelResult` to standard output. The I/O shim is a separate crate outside the kernel graph, so the kernel stays free of I/O and `unsafe`. CI checks that the module imports nothing beyond an allowlisted minimal WASI set and runs it under a pinned runtime configuration.

## Scope

**In:** the shim crate `fid-kernel-wasm` (binary, target `wasm32-wasip1`); the import allowlist file; the import-section checker (a CI tool); the pinned wasmtime configuration (NaN canonicalisation on, relaxed SIMD off, threads off, fixed stack and memory limits) recorded as data; a smoke test running sample calls.
**Out:** the assembly of the algorithm table (`fid-kernel` crate, filled by generated registration code); reproducible-build settings (`KRN.MAN.REPRO`); cross-host comparisons (`KRN.MAN.XART`); verifier replay using the artifact (`TRUST`).

## Requirements

- **REQ-KRN.MAN.WASM-001** The build shall produce a `wasm32-wasip1` command module from `fid-kernel-wasm` whose `main` reads one canonical call from stdin, calls `fid_kci::evaluate` with the assembled table, writes the canonical result or error to stdout, and exits 0 on success and 1 on a kernel error.
- **REQ-KRN.MAN.WASM-002** The shim crate shall contain no `unsafe` code and no logic other than reading, calling and writing.
- **REQ-KRN.MAN.WASM-003** The import checker shall list every import of the module and fail CI if any import is not in the allowlist file (initially `fd_read`, `fd_write`, `proc_exit` and the minimum others the Rust standard library requires for a command, each justified in the file).
- **REQ-KRN.MAN.WASM-004** The runtime configuration (wasmtime version, NaN canonicalisation, relaxed SIMD, threads, stack and memory limits) shall be stored as a data file whose SHA-256 is the `replay_runtime.config_digest`.
- **REQ-KRN.MAN.WASM-005** If the module exhausts the configured stack or memory, then the runner shall report `NOT_REPLAYABLE(resource)` rather than a result.

## Acceptance criteria

- **AC-1** Given the built module, when the import checker runs, then it lists only allowlisted imports and exits 0; given a test module that imports `random_get`, then it exits non-zero naming the import.
- **AC-2** Given fixture `fixtures/man/kci-calls-v1` (from `KRN.MAN.KCI`), when each call is piped through the module under the pinned wasmtime configuration on Linux x86-64, then every stdout equals the golden result bytes.
- **AC-3** Given a call to a test algorithm that recurses past the configured stack limit, then the runner reports `NOT_REPLAYABLE(resource)`.
- **AC-4** Given the shim source, when scanned, then it contains no `unsafe` and no call into the kernel other than `evaluate`.

## Design

- **Registry command(s):** none.
- **Data structures:** `wasm-imports.allow` (one import per line with justification); `wasm-runtime.toml` (pinned runtime settings). Both live in `crates/fid-kernel-wasm/`.
- **Algorithm:** read all of stdin with a fixed maximum size, parse the canonical call, evaluate, write. The import checker parses the module's import section with a hand-written pure-Rust wasm reader in a CI tool crate (no third-party parser).
- **Errors:** shim exit codes 0, 1 (kernel error, result on stdout), 2 (malformed or oversized input), 3 (stdout could not be written); runner-level `NOT_REPLAYABLE(resource)`.
- **Determinism:** D0.
- **Performance budget:** start-up < 50 ms per call under wasmtime with a cached compiled module [unverified until measured].
- **Crate:** `fid-kernel-wasm` (shim, layer `app`) and the CI tool `fid-wasm-check` (layer `tool`).

### Pinned choices (every value-affecting choice; changing one changes outputs or verdicts)

- **Call on stdin.** At most 64 MiB (`MAX_CALL_BYTES`; the shim reads one byte more to detect an oversized call) and at most 4096 inputs. The bytes must be exactly the canonical JSON (keys in UTF-16 order, no whitespace, no trailing bytes) of `{"algorithm":"<id>","inputs":[{"bytes":"<hex>","sha256":"sha256:<64 hex>"},...],"params":"<hex>","seed":"<u64>"}`: `<id>` in its exact `AlgorithmId` form (at most 128 bytes), byte strings as lowercase hex of even length, the seed as a decimal `u64` without leading zeros, and the `seed` member absent when there is no seed. Anything else is `malformed_call`.
- **Stdout.** Success: `KernelResult::canonical_json()` bytes, exit 0. Kernel error: `{"error":"<KernelError code>"}`, exit 1. Table not buildable: `{"error":"table_unavailable"}`, exit 1. Malformed call or stdin read failure: `{"error":"malformed_call"}`, exit 2. Oversized: `{"error":"oversized_call"}`, exit 2. No trailing newline; nothing on stderr.
- **Algorithm table.** Until the `fid-kernel` assembly crate exists, the reference build registers no algorithm (every call is `unknown_algorithm`). The stand-in module is compiled only with the `test-algorithms` feature (or in unit tests), so the reference module does not contain it (CI checks the built bytes); with the feature it registers: `test.echo.identity@1.0.0` (the KRN.MAN.KCI echo), `test.stack.recurse@1.0.0` (non-tail recursion to the depth in its decimal params) and `test.memory.alloc@1.0.0` (an infallible allocation of the size in its decimal params).
- **Import allowlist file.** UTF-8, LF only (CR, tab or BOM refused), at most 64 KiB and 256 entries; `#` comment lines and blank lines; entries `<module> <name> -- <justification>` with `<module>` and `<name>` 1 to 64 bytes of `[a-z0-9_]`, one space between them, and a non-empty printable-ASCII justification without leading or trailing space. Duplicate `(module, name)` refused. Entries allow **function** imports only; a table, memory, global or tag import is denied whatever its name. Unused entries are reported, not failed. Allowed (each justified in the file): `fd_read`, `fd_write`, `proc_exit`, `environ_get`, `environ_sizes_get` (Rust std start-up). `clock_time_get` is **not** allowed (see Clock). A function import from module `fid_trap` named `clock_time_get` or `clock_res_get` passes as `trapped`; allowlist entries for module `fid_trap` are refused.
- **Wasm reader** (fail-closed; error codes in brackets). Input at most 64 MiB [`too_large`]; magic `\0asm` [`bad_magic`] and version 1 exactly, so components are refused [`bad_version`]; LEB128 `u32` at most 5 bytes with no bits above bit 31, non-minimal encodings accepted as the core spec allows [`bad_leb`]; section ids 0 to 13 only [`unknown_section`]; non-custom sections at most once and in the core-spec order type, import, function, table, memory, tag, global, export, start, element, data count, code, data [`section_order`]; each section inside the input [`section_overrun`]; type and import sections end exactly at their size [`section_size_mismatch`]; types are `0x60` function types over i32, i64, f32, f64, v128, funcref, externref, at most 65 536 types and 1000 params or results each [`bad_type`, `limit_exceeded`]; at most 4096 imports and 1024-byte names [`limit_exceeded`], names UTF-8 [`bad_utf8`]; import kinds 0 to 4 [`bad_import_kind`]; limits flags 0 or 1 only, so shared and 64-bit memories are refused, and max >= min [`bad_limits`]; function and tag type indices in range [`bad_type_index`]; reading past an end is [`truncated`]. Other section bodies are not interpreted: host access is only through imports.
- **Clock (`clock = "trap"`).** The linked module still imports `wasi_snapshot_preview1.clock_time_get` (wasi-libc links it under Rust std; checked on the CI build). It fails closed instead of being allowed: the post-link step `fid-wasm-check seal <linked> <sealed>` renames the module of every WASI function import `clock_time_get` or `clock_res_get` to `fid_trap`, keeping import order, count and descriptors (so every function index is unchanged), copying all bytes outside the import section and re-encoding the import section with minimal LEB128 lengths (sealing is idempotent). The **sealed** module is the manifest's WASM artifact and is what is digested. The runner runs wasmtime with unknown imports trapping, so any call to the clock is `NOT_REPLAYABLE(trap)`. The source scan below stays as defence in depth.
- **Runtime settings file.** Canonical form only: exactly the keys `clock`, `fuel`, `grow_failure`, `max_memory_size`, `max_wasm_stack`, `nan_canonicalization`, `relaxed_simd`, `schema`, `threads`, `wasmtime_version`, in that (byte) order, one `key = value` per line, LF after every line, nothing else; at most 4096 bytes. Policy enforced by the reader: `clock = "trap"`, `grow_failure = "trap"`, `nan_canonicalization = true`, `relaxed_simd = false`, `threads = false`, `schema = "1"`; `max_wasm_stack` in 64 KiB..16 MiB, `max_memory_size` in 64 KiB..4 GiB, `fuel` in 10^6..10^12, `wasmtime_version` `MAJOR.MINOR.PATCH`. Pinned values: wasmtime `49.0.6` (the version the CI wasm32 job installs), stack 1 MiB (1048576), memory 1 GiB (1073741824), fuel 4000000000. `replay_runtime.config_digest` = SHA-256 of the file bytes, which equal the canonical bytes.
- **Runner.** Refuses to run unless the settings parse, the module passes the import check, and the first line of `wasmtime --version` is `wasmtime` (or `wasmtime-cli`) followed by exactly the pinned version. Runs `wasmtime run -C cache=n -W nan-canonicalization=y -W relaxed-simd=n -W threads=n -W max-wasm-stack=<stack> -W max-memory-size=<memory> -W trap-on-grow-failure=y -W fuel=<fuel> -W unknown-imports-trap=y <module>` (module path must not start with `-`) with the call on stdin. Exit 0 with stdout starting `{"algorithm":"`, or exit 1 or 2 with stdout starting `{"error":"`, and empty stderr, pass through unchanged. Otherwise the verdict is anchored to what only wasmtime controls (the guest shares stderr): if wasmtime exits with its trap exit code (134 on Unix, 3 on Windows; WASI refuses guest exit statuses of 126 and above) **and** the last non-empty stderr line, with leading spaces and a `<n>: ` cause number removed, is exactly `wasm trap: call stack exhausted`, `wasm trap: out of bounds memory access` (in a module without `unsafe` only a shadow-stack overrun produces it), or `wasm trap: all fuel consumed by WebAssembly`, **or** if wasmtime exits 1 with empty stdout and that last line is exactly `forcing trap when growing memory to <digits> bytes` (wasmtime 49 reports a forced `memory.grow` failure as a host error with exit 1, as measured in CI; a guest can also exit 1, so this verdict can be forged, but only from one non-value to another), stdout is `NOT_REPLAYABLE(resource)` and the runner exits 3; anything else gives `NOT_REPLAYABLE(trap)` and exit 4. Exit 5: the runner could not run at all.
- **Limits are not portable yet.** The stack limit counts native stack bytes, which depend on the host architecture and the compiler wasmtime uses, so whether a deep call hits `resource` can differ between platforms. Fuel and memory limits are counted in wasm units. AC-2/AC-3 are certified on Linux x86-64 only.
- **Manifest cross-check** (`fid-wasm-check manifest`): the manifest is read with `KernelManifest::parse_canonical`; the module must pass the reader and the import check; the manifest must list an artifact with role `wasm` and target `wasm32-wasip1`, whose `sha256` equals the SHA-256 of the (sealed) module bytes; `replay_runtime.config_digest` must equal the settings digest and `replay_runtime.version` the pinned wasmtime version. Failures: `manifest:<code>`, `module:<code>`, `imports_denied`, `artifact_missing`, `artifact_digest_mismatch`, `config_digest_mismatch`, `runtime_version_mismatch`.
- **AC-4 scan.** No file under `crates/fid-kernel-wasm/src/` contains `unsafe`; `main.rs`, `shim.rs` and `call.rs` import from `fid_kci` only `evaluate`, `AlgorithmId`, `AlgorithmTable`, `CardRegistry`, `Digest`, `KernelCall`; the only kernel function they call is `evaluate`, and the only `Type::function(` calls on kernel types are the constructors `AlgorithmId::parse`, `Digest::parse`, `AlgorithmTable::new`, `CardRegistry::default`. Clock gate (defence in depth): no non-comment line under `src/` of `fid-kernel-wasm`, `fid-kci`, `fid-num` or `fid-types` names `std::time`, `core::time`, `Instant`, `SystemTime`, `clock_gettime` or `clock_time_get`.

## Test fixtures

`fixtures/man/kci-calls-v1` (shared with `KRN.MAN.KCI`), CC-BY-4.0; a test-only module importing `random_get`. `fixtures/man/wasm/` (written by `make_fixtures.py`, Python stdlib only): hand-encoded modules with labels, the canonical calls with golden stdout, manifests for the cross-check, and the settings digest.

## Build prompt

> Create binary crate `fid-kernel-wasm` targeting `wasm32-wasip1`: `main` reads stdin (bounded size), parses one canonical `KernelCall`, calls `fid_kci::evaluate` with the algorithm table and card registry from the `fid-kernel` assembly crate, writes the canonical `KernelResult` or error to stdout, and exits 0, 1 (kernel error) or 2 (bad input). No `unsafe`, no other logic. Create CI tool crate `fid-wasm-check` that lists a module's imports and fails on any not in `wasm-imports.allow` (each allowed import justified in the file). Store the pinned wasmtime settings (version, NaN canonicalisation on, relaxed SIMD off, threads off, stack and memory limits) in `wasm-runtime.toml`; the runner used by CI applies them and maps stack or memory exhaustion to `NOT_REPLAYABLE(resource)`. Tests: import check pass and a failing test module, `fixtures/man/kci-calls-v1` golden outputs through the module, stack-exhaustion mapping, shim scan for `unsafe`. Constitution: kernel crates have no I/O and no `unsafe`; the shim is outside the kernel graph but still has no `unsafe`; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.MAN.WASM.

## Open questions

- The concept doc's manifest example uses `wasm32-wasip2` with an empty import list. This leaf uses a `wasip1` command with an allowlisted minimal WASI import set so no `unsafe` export glue is needed; revisit if the steering group wants a strictly empty import section (that would need a named `unsafe` exception for an export shim).
- **No `fid-kernel` yet.** The assembly crate is out of scope, so the shim carries stand-in test algorithms behind the `test-algorithms` feature; the reference build registers none. When `fid-kernel` lands, `shim::reference_table` should take its table and the stand-ins should move to a test-only build.
- **KRN.NUM.LINT.FLAGS (open PR #182) not used.** Its `KNP1-FLAG-WASMFMA` check also parses the wasm reference module. This leaf does not depend on unmerged code; once #182 merges, the two wasm readers should share one parser (this one is bounded and fail-closed, see Pinned choices) and the CI wasm32 step should run the flags lint on the built module.
- **Dependency on KRN.MAN.MANI.SCHEMA** (`fid_kci::manifest`, `parse_canonical`) is used for the manifest cross-check but not recorded in `depends_on`/`uses_artifacts`, because MANI already lists this leaf in `uses_artifacts` and adding the reverse edge may form a cycle; a maintainer should decide how to record it.
- **Cross-platform replay of resource limits** belongs to `KRN.MAN.XART`: native stack use differs between hosts, so a call near the stack limit may be `resource` on one platform and a value on another. XART should decide whether resource outcomes are compared across hosts or the stack limit is replaced by a wasm-level depth measure.
- **The clock import is sealed, not removed.** Removing it needs a `no_std` shim (a named `unsafe` export exception) or a WASI libc without timed waits; either changes the artifact.
- **Resource classification relies on wasmtime's last error line and trap exit code** (see Runner). A wasmtime upgrade must re-run AC-3 before the pinned version changes.

## Children

_Leaf: none._
