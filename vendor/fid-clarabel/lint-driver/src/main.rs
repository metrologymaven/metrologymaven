//! Interim KNP-1 report for the vendored solver (KRN.SOLV.CONIC.PATCH AC-1).
//!
//! `fid-knp1-lint check` (KRN.NUM.LINT.CLI) is not built yet, so this driver runs the built
//! syntax-level scan of `fid-knp1-lint` (KRN.NUM.LINT.SYN, `syn_scan::scan_file`) over every
//! `.rs` file of the vendored crate's `src/` with `knp1-lint.toml` next to this file, applies no
//! waivers (there are none), builds the schema-1 report with `findings::Report`, adds the
//! `linted` entry that `cargo xtask conic-audit` reads (KRN.SOLV.CONIC.VEND), and writes the
//! canonical JSON to `vendor/fid-clarabel/knp1-findings.json`. It exits 1 if any finding
//! remains. The other lint modules (graph, flags, typed, data) are not run here.
//!
//! Run from the repository root: `cargo run --manifest-path vendor/fid-clarabel/lint-driver/Cargo.toml`.

use fid_canon::CValue;
use fid_knp1_lint::syn_scan::{parse_config, scan_file, FileRef};
use fid_knp1_lint::{Applied, InputDigests, Report};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const CRATE_NAME: &str = "fid-clarabel";
const CRATE_VERSION: &str = "0.11.1";
const TOOL_VERSION: &str = "0.0.1";

fn rust_files(root: &Path, dir: &Path, out: &mut Vec<String>) -> std::io::Result<()> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?.map(|e| e.map(|e| e.path())).collect::<Result<_, _>>()?;
    entries.sort();
    for p in entries {
        if p.is_dir() {
            rust_files(root, &p, out)?;
        } else if p.extension().is_some_and(|e| e == "rs") {
            let rel = p.strip_prefix(root).map_err(std::io::Error::other)?;
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

fn run() -> Result<bool, String> {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let vendored = here.join("..");
    let crate_root = vendored.join("crate");
    let repo = vendored.join("../..");
    let config = std::fs::read(here.join("knp1-lint.toml")).map_err(|e| format!("knp1-lint.toml: {e}"))?;
    let cfg = parse_config(&String::from_utf8_lossy(&config)).map_err(|e| e.to_string())?;
    let lockfile = std::fs::read(repo.join("Cargo.lock")).map_err(|e| format!("Cargo.lock: {e}"))?;

    let mut files = Vec::new();
    rust_files(&crate_root, &crate_root.join("src"), &mut files).map_err(|e| format!("crate/src: {e}"))?;
    let mut findings = Vec::new();
    let mut notes = 0usize;
    for file in &files {
        let bytes = std::fs::read(crate_root.join(file)).map_err(|e| format!("{file}: {e}"))?;
        let f = FileRef { crate_name: CRATE_NAME, crate_version: CRATE_VERSION, file, crate_files: &files };
        let out = scan_file(f, &bytes, &cfg);
        notes += out.notes.len();
        findings.extend(out.findings);
    }
    let passed = findings.is_empty();
    let inputs = InputDigests::of(&config, &lockfile, b"", b"");
    let report = Report::new(TOOL_VERSION, inputs, Applied::without_waivers(findings)).map_err(|e| e.to_string())?;
    let CValue::Obj(mut obj) = report.to_cvalue() else {
        return Err("report is not an object".to_string());
    };
    let mut entry = BTreeMap::new();
    entry.insert("crate_name".to_string(), CValue::Str(CRATE_NAME.to_string()));
    entry.insert("crate_version".to_string(), CValue::Str(CRATE_VERSION.to_string()));
    obj.insert("linted".to_string(), CValue::Arr(vec![CValue::Obj(entry)]));
    let bytes = fid_canon::to_canonical_bytes(&CValue::Obj(obj)).map_err(|e| e.to_string())?;
    let out = vendored.join("knp1-findings.json");
    std::fs::write(&out, bytes).map_err(|e| format!("{}: {e}", out.display()))?;
    eprintln!(
        "{} files scanned, {} findings, {notes} facade notes; wrote {}",
        files.len(),
        report.findings.len(),
        out.display()
    );
    for f in &report.findings {
        eprintln!("{} {}:{}:{} {}", f.rule.id(), f.file, f.line, f.column, f.message);
    }
    Ok(passed)
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("lint-driver: {e}");
            ExitCode::from(2)
        }
    }
}
