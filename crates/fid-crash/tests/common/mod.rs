//! Shared helpers of the fid-crash integration tests.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_crash::{CrashFailure, FailureKind};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

pub const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/sto/sql/crash/chain.json");
pub const BUS_V3: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/bus/pipe/err/bus-errors.v3.json");
pub const BIN: &str = env!("CARGO_BIN_EXE_fid-crash");

/// A unique directory under the system temp dir, removed on drop.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos();
        let name = format!("fid-crash-{tag}-{}-{nanos}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst));
        let p = std::path::absolute(std::env::temp_dir().join(name)).unwrap();
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn fixture() -> CValue {
    fid_canon::parse(&std::fs::read(FIXTURE).unwrap()).unwrap()
}

pub fn field<'a>(v: &'a CValue, k: &str) -> &'a CValue {
    match v {
        CValue::Obj(o) => o.get(k).unwrap_or_else(|| panic!("missing {k}")),
        _ => panic!("not an object"),
    }
}

pub fn arr(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(a) => a,
        _ => panic!("not an array"),
    }
}

pub fn num(v: &CValue) -> u64 {
    match v {
        CValue::Num(n) if *n >= 0.0 && n.fract() == 0.0 => *n as u64,
        _ => panic!("not a non-negative integer: {v:?}"),
    }
}

pub fn text(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        _ => panic!("not a string: {v:?}"),
    }
}

pub fn boolean(v: &CValue) -> bool {
    match v {
        CValue::Bool(b) => *b,
        _ => panic!("not a bool: {v:?}"),
    }
}

/// Reasons of `internal` in the frozen v3 bus error catalogue.
pub fn v3_internal_reasons() -> Vec<String> {
    let cat = fid_bus::parse_catalog(&std::fs::read(BUS_V3).unwrap()).unwrap();
    cat.get("internal").unwrap().iter().cloned().collect()
}

/// A refusal must be one of the store-integrity kinds and map to a v3 `internal` reason.
pub fn assert_refused(f: &CrashFailure, why: &str) {
    let e = f.to_bus_error();
    assert_eq!(e.code().as_str(), "internal", "{why}: {f}");
    assert!(v3_internal_reasons().iter().any(|r| r == e.reason()), "{why}: reason {} not in v3", e.reason());
    let want = if f.kind == FailureKind::AckMissing { "invariant_violated" } else { "storage" };
    assert_eq!(e.reason(), want, "{why}: {f}");
}

/// The writer command of the harness binary.
pub fn writer(mode: &str, extra: &[&str]) -> Vec<OsString> {
    let mut v: Vec<OsString> = vec![BIN.into(), "writer".into(), "--mode".into(), mode.into()];
    v.extend(extra.iter().map(OsString::from));
    v
}

/// Copies a closed project folder (retrying while a killed process's handles close).
pub fn copy_dir(from: &Path, to: &Path) {
    let start = Instant::now();
    loop {
        let _ = std::fs::remove_dir_all(to);
        match try_copy(from, to) {
            Ok(()) => return,
            Err(e) if start.elapsed() < Duration::from_secs(10) => {
                eprintln!("copy retry: {e}");
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => panic!("copy {} -> {}: {e}", from.display(), to.display()),
        }
    }
}

fn try_copy(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let target = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            try_copy(&e.path(), &target)?;
        } else {
            std::fs::copy(e.path(), &target)?;
        }
    }
    Ok(())
}
