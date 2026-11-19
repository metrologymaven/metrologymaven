//! `fid-registry-build [--catalogue <file>]`: writes the Registry Manifest's exact bytes to
//! stdout and `sha256:<hex>` to stderr, exit 0. On failure stdout stays empty, the RFC 8785
//! error report goes to stderr, exit 1; a usage or catalogue-file problem exits 2. Without
//! `--catalogue` the workspace catalogue is used (`GEO.FRAME.UNITS` plus every crate's
//! quantity-catalogue fragment, `workspace_catalogue`; a merge refusal is a build failure,
//! exit 1); with it, a catalogue document read through `Catalogue::from_json` (which keeps
//! GEO.FRAME's kinds unchanged).

use fid_registry::catalogue::MAX_CATALOGUE_BYTES;
use fid_registry::Catalogue;
use std::io::{Read, Write};
use std::process::ExitCode;

const USAGE: &str = "usage: fid-registry-build [--catalogue <catalogue.json>]";

fn read_bounded(path: &str) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("catalogue: cannot open: {}", e.kind()))?;
    let mut bytes = Vec::new();
    file.take(MAX_CATALOGUE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("catalogue: cannot read: {}", e.kind()))?;
    if bytes.len() > MAX_CATALOGUE_BYTES {
        return Err("catalogue: catalogue_too_large".to_string());
    }
    Ok(bytes)
}

/// `None` for the workspace catalogue, else the catalogue file's.
fn catalogue(args: &[String]) -> Result<Option<Catalogue>, String> {
    match args {
        [] => Ok(None),
        [flag, path] if flag == "--catalogue" => {
            Catalogue::from_json(&read_bounded(path)?).map(Some).map_err(|e| format!("catalogue: {}", e.code()))
        }
        _ => Err(USAGE.to_string()),
    }
}

fn report(e: &fid_registry_build::BuildError) -> ExitCode {
    let report = fid_canon::to_canonical_bytes(&e.to_value()).unwrap_or_default();
    eprintln!("{}", String::from_utf8_lossy(&report));
    ExitCode::from(1)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let catalogue = match catalogue(&args) {
        Ok(Some(c)) => c,
        Ok(None) => match fid_registry_build::workspace_catalogue() {
            Ok(c) => c,
            Err(e) => return report(&e),
        },
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::from(2);
        }
    };
    match fid_registry_build::build_workspace(&catalogue) {
        Ok(export) => {
            let mut out = std::io::stdout().lock();
            if out.write_all(&export.bytes).and_then(|()| out.flush()).is_err() {
                return ExitCode::from(2);
            }
            eprintln!("{}", export.digest);
            ExitCode::SUCCESS
        }
        Err(e) => report(&e),
    }
}
