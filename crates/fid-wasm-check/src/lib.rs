//! CI tool for the kernel's WASM reference artifact (spec node `KRN.MAN.WASM`).
//!
//! - [`module`]: a bounded, fail-closed reader for a WebAssembly module's structure and imports;
//! - [`allow`]: the import allowlist (`wasm-imports.allow`) and the import check (AC-1);
//! - [`runtime`]: the pinned wasmtime settings (`wasm-runtime.toml`) and their digest;
//! - [`runner`]: runs the module under those settings and maps stack or memory exhaustion to
//!   `NOT_REPLAYABLE(resource)` (AC-2, AC-3);
//! - [`seal`]: the post-link step that renames clock imports so they trap (`clock = "trap"`);
//! - [`manifest`]: cross-checks a kernel manifest's WASM artifact digest and replay runtime;
//! - [`sections`]: section-level comparison of two builds of the artifact (`KRN.MAN.REPRO`);
//! - [`simd`]: refuses any SIMD (`0xFD`) instruction in the code section (`KRN.MAN.REPRO`).

pub mod allow;
pub mod manifest;
pub mod module;
pub mod runner;
pub mod runtime;
pub mod seal;
pub mod sections;
pub mod simd;
