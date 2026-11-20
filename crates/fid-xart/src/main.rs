//! `fid-xart`: cross-host and cross-artifact bit-identity gate (spec node KRN.MAN.XART).
//!
//! ```text
//! fid-xart run --calls <calls.txt> --kind <wasm|native> --host <label> --artifact <label>
//!              --artifact-file <path> --out <table.json> -- <program> [args...]
//! fid-xart compare --matrix <xart-matrix.toml> --tables <dir> --release <none|patch|minor|major>
//!              [--previous <table.json>] --out <report.json>
//! ```
//!
//! `run` sends every call of the call set to the artifact command (one process per call, call
//! on stdin) and writes the digest table; `--artifact-file` is the file whose SHA-256 the table
//! records (the sealed WASM module, or the native binary). It exits 0 when the table is
//! written and 2 on any error; an artifact that cannot answer a call is an error, never a
//! skipped row, and then no table is written.
//!
//! `compare` reads every `*.json` table in `--tables` (any other entry is an error), compares
//! them and writes the report. It exits 0 when the gate passes, 1 when it fails (the report is
//! written and the failures are printed) and 2 on a usage or input error.

use std::io::Read;
use std::path::Path;
use std::process::ExitCode;

use fid_xart::artifact::{hash_file, run_call_set, CommandArtifact, TableMeta};
use fid_xart::calls::{parse_calls, MAX_CALL_SET_BYTES};
use fid_xart::matrix::{parse_matrix, MAX_MATRIX_BYTES};
use fid_xart::report::{compare, Release};
use fid_xart::table::{DigestTable, Kind, MAX_TABLE_BYTES};

const USAGE: &str = "usage: fid-xart run --calls <calls.txt> --kind <wasm|native> --host <label> --artifact <label> \
--artifact-file <path> --out <table.json> -- <program> [args...]\n       \
fid-xart compare --matrix <xart-matrix.toml> --tables <dir> --release <none|patch|minor|major> \
[--previous <table.json>] --out <report.json>";

/// Most tables read from one directory.
const MAX_TABLES: usize = 1024;

fn read(path: &Path, max: usize) -> Result<Vec<u8>, String> {
    let f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut buf = Vec::new();
    f.take(max as u64 + 1).read_to_end(&mut buf).map_err(|e| format!("{}: {e}", path.display()))?;
    if buf.len() > max {
        return Err(format!("{}: larger than {max} bytes", path.display()));
    }
    Ok(buf)
}

/// `--flag value` pairs before an optional `--`; every flag at most once, all in `allowed`.
fn flags<'a>(args: &'a [String], allowed: &[&str]) -> Result<(Vec<(&'a str, &'a str)>, &'a [String]), String> {
    let mut out: Vec<(&str, &str)> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "--" {
            return Ok((out, &args[i + 1..]));
        }
        if !allowed.contains(&a) || out.iter().any(|(k, _)| *k == a) {
            return Err(format!("unknown or repeated option {a}"));
        }
        let v = args.get(i + 1).ok_or_else(|| format!("{a} needs a value"))?;
        out.push((a, v.as_str()));
        i += 2;
    }
    Ok((out, &[]))
}

fn get<'a>(f: &[(&str, &'a str)], key: &str) -> Result<&'a str, String> {
    f.iter().find(|(k, _)| *k == key).map(|(_, v)| *v).ok_or_else(|| format!("missing {key}"))
}

fn run(args: &[String]) -> Result<(), String> {
    let allowed = ["--calls", "--kind", "--host", "--artifact", "--artifact-file", "--out"];
    let (f, command) = flags(args, &allowed)?;
    let (program, rest) = command.split_first().ok_or("missing -- <program>")?;
    let set = parse_calls(&read(Path::new(get(&f, "--calls")?), MAX_CALL_SET_BYTES)?).map_err(|e| e.to_string())?;
    let kind = Kind::parse(get(&f, "--kind")?).ok_or("--kind must be wasm or native")?;
    let meta = TableMeta {
        artifact: get(&f, "--artifact")?.to_string(),
        artifact_sha256: hash_file(Path::new(get(&f, "--artifact-file")?))?,
        host: get(&f, "--host")?.to_string(),
        kind,
    };
    let out = get(&f, "--out")?;
    let mut artifact = CommandArtifact { program: program.clone(), args: rest.to_vec() };
    let table = run_call_set(&set, &mut artifact, meta).map_err(|e| e.to_string())?;
    let bytes = table.to_json().map_err(|e| e.to_string())?;
    std::fs::write(out, bytes).map_err(|e| format!("{out}: {e}"))?;
    println!("{} rows, call set {}, artifact {}", table.rows.len(), table.call_set_digest, table.artifact_sha256);
    Ok(())
}

fn load_tables(dir: &Path) -> Result<Vec<DigestTable>, String> {
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let path = entry.map_err(|e| format!("{}: {e}", dir.display()))?.path();
        if path.extension().and_then(|x| x.to_str()) != Some("json") || !path.is_file() {
            return Err(format!("{}: not a .json table", path.display()));
        }
        paths.push(path);
    }
    if paths.len() > MAX_TABLES {
        return Err(format!("{}: more than {MAX_TABLES} tables", dir.display()));
    }
    paths.sort();
    paths
        .iter()
        .map(|p| DigestTable::parse(&read(p, MAX_TABLE_BYTES)?).map_err(|e| format!("{}: {e}", p.display())))
        .collect()
}

fn compare_cmd(args: &[String]) -> Result<bool, String> {
    let allowed = ["--matrix", "--tables", "--release", "--previous", "--out"];
    let (f, extra) = flags(args, &allowed)?;
    if !extra.is_empty() {
        return Err("compare takes no command".to_string());
    }
    let matrix = parse_matrix(&read(Path::new(get(&f, "--matrix")?), MAX_MATRIX_BYTES)?).map_err(|e| e.to_string())?;
    let tables = load_tables(Path::new(get(&f, "--tables")?))?;
    let release = Release::parse(get(&f, "--release")?).ok_or("--release must be none, patch, minor or major")?;
    let previous = match get(&f, "--previous") {
        Ok(p) => Some(DigestTable::parse(&read(Path::new(p), MAX_TABLE_BYTES)?).map_err(|e| format!("{p}: {e}"))?),
        Err(_) => None,
    };
    let out = get(&f, "--out")?;
    let c = compare(&matrix, &tables, release, previous.as_ref()).map_err(|e| e.to_string())?;
    std::fs::write(out, &c.report).map_err(|e| format!("{out}: {e}"))?;
    for failure in &c.failures {
        println!("FAIL {failure}");
    }
    println!(
        "gate {}: {} tables, {} failures",
        if c.passed() { "pass" } else { "fail" },
        tables.len(),
        c.failures.len()
    );
    Ok(c.passed())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.split_first() {
        Some((cmd, rest)) if cmd == "run" => run(rest).map(|()| true),
        Some((cmd, rest)) if cmd == "compare" => compare_cmd(rest),
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}
