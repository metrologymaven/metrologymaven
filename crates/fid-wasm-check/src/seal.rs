//! The post-link seal step: makes every clock import fail closed (spec node KRN.MAN.WASM,
//! Design, `clock = "trap"`).
//!
//! Rust std on `wasm32-wasip1` links `clock_time_get` through the C library even though the
//! shim never reads a clock. Instead of allowing the import, [`seal`] renames the module of
//! every function import `wasi_snapshot_preview1.<name>` with `<name>` in [`CLOCK_IMPORTS`] to
//! [`TRAP_MODULE`]. The runner runs wasmtime with unknown imports trapping, so a call to a
//! renamed import is `NOT_REPLAYABLE(trap)` and can never feed a value. Only the module names
//! change: import order, count and descriptors are kept, so every function index is the same.
//!
//! The output is deterministic: the bytes before and after the import section are copied
//! unchanged, and the import section is re-encoded with minimal LEB128 lengths (so sealing a
//! sealed module returns it unchanged). The sealed module is the manifest's WASM artifact.

use crate::module::{self, ImportKind, WasmError};

/// The module name that no runtime defines; imports from it trap when called.
pub const TRAP_MODULE: &str = "fid_trap";
/// The WASI module whose clock imports are renamed.
pub const WASI_MODULE: &str = "wasi_snapshot_preview1";
/// The clock functions of WASI preview 1.
pub const CLOCK_IMPORTS: [&str; 2] = ["clock_res_get", "clock_time_get"];

/// True for a function import that sealing renames.
pub fn is_clock(module: &str, name: &str, kind: ImportKind) -> bool {
    kind == ImportKind::Func && module == WASI_MODULE && CLOCK_IMPORTS.contains(&name)
}

/// True for a function import that sealing produced (allowed: it traps when called).
pub fn is_trapped(module: &str, name: &str, kind: ImportKind) -> bool {
    kind == ImportKind::Func && module == TRAP_MODULE && CLOCK_IMPORTS.contains(&name)
}

fn uleb(out: &mut Vec<u8>, mut n: usize) {
    loop {
        let b = (n & 0x7f) as u8;
        n >>= 7;
        if n == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

fn name(out: &mut Vec<u8>, s: &str) {
    uleb(out, s.len());
    out.extend_from_slice(s.as_bytes());
}

/// Seals a module (see the module docs). Refuses any module the reader refuses, and checks
/// that the output reads back with the same imports apart from the renamed modules.
pub fn seal(bytes: &[u8]) -> Result<Vec<u8>, WasmError> {
    let m = module::parse(bytes)?;
    let Some((id_at, _, end)) = m.import_section else {
        return Ok(bytes.to_vec());
    };
    let mut payload = Vec::new();
    uleb(&mut payload, m.imports.len());
    for imp in &m.imports {
        let module_name = if is_clock(&imp.module, &imp.name, imp.kind) { TRAP_MODULE } else { imp.module.as_str() };
        name(&mut payload, module_name);
        name(&mut payload, &imp.name);
        payload.extend_from_slice(bytes.get(imp.desc.0..imp.desc.1).unwrap_or(&[]));
    }
    let mut out = Vec::with_capacity(bytes.len() + 16);
    out.extend_from_slice(bytes.get(..id_at).unwrap_or(&[]));
    out.push(2);
    uleb(&mut out, payload.len());
    out.extend_from_slice(&payload);
    out.extend_from_slice(bytes.get(end..).unwrap_or(&[]));
    let sealed = module::parse(&out)?;
    let same = sealed.imports.len() == m.imports.len()
        && sealed.imports.iter().zip(&m.imports).all(|(a, b)| {
            a.name == b.name
                && a.kind == b.kind
                && (a.module == b.module || (is_clock(&b.module, &b.name, b.kind) && a.module == TRAP_MODULE))
        });
    if !same {
        return Err(WasmError { kind: module::WasmErrorKind::SectionSizeMismatch, offset: id_at });
    }
    Ok(out)
}
