//! `fid-eval`: evaluation tooling of Metrology Maven (D-032; outside the registry, never on MCP).
//!
//! First slice: `fid-eval suite digest` (spec node AGENT.EVAL.FORMAT).

use fid_canon::CValue;
use fid_eval_suite::{load_suite, FormatError};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "\
fid-eval: Metrology Maven evaluation tooling (pre-alpha)

USAGE:
    fid-eval suite digest --suite <DIR> [--json]
                                  Load the suite fail-closed and print its version
                                  (sha256 over the sorted file digests); with --json
                                  print {\"files\",\"suite_version\",\"tasks\"} or {\"errors\"}
    fid-eval --version            Print the version
    fid-eval --help               Print this help

Exit codes: 0 ok, 1 invalid suite, 2 usage error.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["--version" | "-V"] => {
            println!("fid-eval {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        [] | ["--help" | "-h" | "help"] => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        ["suite", "digest", rest @ ..] => digest(rest),
        _ => usage_error("unknown command"),
    }
}

fn usage_error(msg: &str) -> ExitCode {
    eprintln!("error: {msg}\n\n{USAGE}");
    ExitCode::from(2)
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

fn print_json(v: &CValue) {
    match fid_canon::to_canonical_bytes(v) {
        Ok(b) => println!("{}", String::from_utf8_lossy(&b)),
        Err(e) => eprintln!("error: {e}"),
    }
}

fn error_json(e: &FormatError) -> CValue {
    CValue::Obj(BTreeMap::from([
        ("code".to_string(), s(e.code.as_str())),
        ("detail".to_string(), s(&e.detail)),
        ("field".to_string(), s(&e.field)),
        ("file".to_string(), s(&e.file)),
    ]))
}

fn digest(args: &[&str]) -> ExitCode {
    let mut suite = None;
    let mut json = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match *a {
            "--json" => json = true,
            "--suite" => match it.next() {
                Some(d) if suite.is_none() => suite = Some(*d),
                Some(_) => return usage_error("--suite given twice"),
                None => return usage_error("--suite needs a directory"),
            },
            other => return usage_error(&format!("unknown argument `{other}`")),
        }
    }
    let Some(dir) = suite else {
        return usage_error("--suite <DIR> is required");
    };
    match load_suite(Path::new(dir)) {
        Ok(st) => {
            if json {
                print_json(&CValue::Obj(BTreeMap::from([
                    ("files".to_string(), CValue::Num(st.files.len() as f64)),
                    ("suite_version".to_string(), s(st.version.as_str())),
                    ("tasks".to_string(), CValue::Num(st.tasks.len() as f64)),
                ])));
            } else {
                println!("{}", st.version);
            }
            ExitCode::SUCCESS
        }
        Err(errs) => {
            if json {
                let list = CValue::Arr(errs.iter().map(error_json).collect());
                print_json(&CValue::Obj(BTreeMap::from([("errors".to_string(), list)])));
            } else {
                for e in &errs {
                    eprintln!("error: {e}");
                }
            }
            ExitCode::from(1)
        }
    }
}
