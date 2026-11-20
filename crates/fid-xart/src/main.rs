//! `fid-xart`: cross-host and cross-artifact bit-identity gate (spec node KRN.MAN.XART).
//!
//! ```text
//! fid-xart run --calls <calls.txt> --kind <wasm|native> --host <label> --artifact <label>
//!              --artifact-file <path> --out <table.json> [--timeout-ms <ms>] -- <program> [args...]
//! fid-xart compare --matrix <xart-matrix.toml> --tables <dir> --release <none|patch|minor|major>
//!              [--previous <table.json>] --out <report.json>
//! ```
//!
//! `run` sends every call of the call set to the artifact command (one process per call, call
//! on stdin, killed after `--timeout-ms` milliseconds, default 60 000) and writes the digest table; `--artifact-file` is the file whose SHA-256 the table
//! records (the sealed WASM module, or the native binary). It exits 0 when the table is
//! written and 2 on any error; an artifact that cannot answer a call is an error, never a
//! skipped row, and then no table is written.
//!
//! `compare` reads every `*.json` table in `--tables`, and every `failed-<host>.txt` marker a
//! host job leaves when it fails (any other entry is an error), compares them and writes the
//! report. It exits 0 when the gate passes, 1 when it fails (the report is
//! written and the failures are printed) and 2 on a usage or input error.
//!
//! Both commands remove an existing `--out` file before they start and write the new one
//! through a temporary file and a rename, so a failed run never leaves a stale or partial file.

use std::io::Read;
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

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
/// Default wall-clock deadline per call.
const DEFAULT_TIMEOUT_MS: u64 = 60_000;

/// Parsed `--flag value` pairs.
type Flags<'a> = Vec<(&'a str, &'a str)>;

/// Removes a stale output file; a missing file is fine.
fn remove_stale(out: &str) -> Result<(), String> {
    match std::fs::remove_file(out) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("{out}: cannot remove the stale file: {e}")),
    }
}

/// Writes `bytes` to a temporary file next to `out`, then renames it over `out`.
fn write_atomic(out: &str, bytes: &[u8]) -> Result<(), String> {
    let tmp = format!("{out}.tmp-{}", std::process::id());
    if let Err(e) = std::fs::write(&tmp, bytes) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("{tmp}: {e}"));
    }
    std::fs::rename(&tmp, out).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{out}: {e}")
    })
}

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
fn flags<'a>(args: &'a [String], allowed: &[&str]) -> Result<(Flags<'a>, &'a [String]), String> {
    let mut out: Flags<'a> = Vec::new();
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
    let allowed = ["--calls", "--kind", "--host", "--artifact", "--artifact-file", "--out", "--timeout-ms"];
    let (f, command) = flags(args, &allowed)?;
    let out = get(&f, "--out")?;
    remove_stale(out)?;
    let timeout_ms = match get(&f, "--timeout-ms") {
        Ok(v) => v.parse::<u64>().ok().filter(|&ms| (1..=86_400_000).contains(&ms)).ok_or("bad --timeout-ms")?,
        Err(_) => DEFAULT_TIMEOUT_MS,
    };
    let (program, rest) = command.split_first().ok_or("missing -- <program>")?;
    let set = parse_calls(&read(Path::new(get(&f, "--calls")?), MAX_CALL_SET_BYTES)?).map_err(|e| e.to_string())?;
    let kind = Kind::parse(get(&f, "--kind")?).ok_or("--kind must be wasm or native")?;
    let meta = TableMeta {
        artifact: get(&f, "--artifact")?.to_string(),
        artifact_sha256: hash_file(Path::new(get(&f, "--artifact-file")?))?,
        host: get(&f, "--host")?.to_string(),
        kind,
    };
    let mut artifact =
        CommandArtifact { program: program.clone(), args: rest.to_vec(), timeout: Duration::from_millis(timeout_ms) };
    let table = run_call_set(&set, &mut artifact, meta).map_err(|e| e.to_string())?;
    let bytes = table.to_json().map_err(|e| e.to_string())?;
    write_atomic(out, &bytes)?;
    println!("{} rows, call set {}, artifact {}", table.rows.len(), table.call_set_digest, table.artifact_sha256);
    Ok(())
}

/// The tables and the hosts that left a `failed-<host>.txt` marker, both sorted.
fn load_tables(dir: &Path) -> Result<(Vec<DigestTable>, Vec<String>), String> {
    let mut paths = Vec::new();
    let mut failed = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let path = entry.map_err(|e| format!("{}: {e}", dir.display()))?.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let marker =
            name.strip_prefix("failed-").and_then(|n| n.strip_suffix(".txt")).filter(|h| fid_xart::is_label(h));
        if !path.is_file() {
            return Err(format!("{}: not a file", path.display()));
        }
        if let Some(host) = marker {
            failed.push(host.to_string());
        } else if name.ends_with(".json") {
            paths.push(path);
        } else {
            return Err(format!("{}: neither a .json table nor a failed-<host>.txt marker", path.display()));
        }
    }
    failed.sort();
    if paths.len() > MAX_TABLES {
        return Err(format!("{}: more than {MAX_TABLES} tables", dir.display()));
    }
    paths.sort();
    let tables = paths
        .iter()
        .map(|p| DigestTable::parse(&read(p, MAX_TABLE_BYTES)?).map_err(|e| format!("{}: {e}", p.display())))
        .collect::<Result<_, _>>()?;
    Ok((tables, failed))
}

fn compare_cmd(args: &[String]) -> Result<bool, String> {
    let allowed = ["--matrix", "--tables", "--release", "--previous", "--out"];
    let (f, extra) = flags(args, &allowed)?;
    if !extra.is_empty() {
        return Err("compare takes no command".to_string());
    }
    let out = get(&f, "--out")?;
    remove_stale(out)?;
    let matrix = parse_matrix(&read(Path::new(get(&f, "--matrix")?), MAX_MATRIX_BYTES)?).map_err(|e| e.to_string())?;
    let (tables, failed) = load_tables(Path::new(get(&f, "--tables")?))?;
    let release = Release::parse(get(&f, "--release")?).ok_or("--release must be none, patch, minor or major")?;
    let previous = match get(&f, "--previous") {
        Ok(p) => Some(DigestTable::parse(&read(Path::new(p), MAX_TABLE_BYTES)?).map_err(|e| format!("{p}: {e}"))?),
        Err(_) => None,
    };
    let c = compare(&matrix, &tables, &failed, release, previous.as_ref()).map_err(|e| e.to_string())?;
    write_atomic(out, &c.report)?;
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
