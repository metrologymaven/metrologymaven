//! `fid-eval`: evaluation tooling of Metrology Maven (D-032; outside the registry, never on MCP).
//!
//! First slice: `fid-eval suite digest` (spec node AGENT.EVAL.FORMAT) and `fid-eval suite
//! check` (fail-closed load including the red-team predicate schema, AGENT.REDTEAM.PRED, and,
//! with `--base`, shell-variant coverage, AGENT.REDTEAM.SHELL).

use fid_canon::CValue;
use fid_eval_suite::{check_variants, load_suite, FormatError, PredicateKind, Suite, VariantCoverage};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "\
fid-eval: Metrology Maven evaluation tooling (pre-alpha)

USAGE:
    fid-eval suite digest --suite <DIR> [--json]
                                  Load the suite fail-closed and print its version
                                  (sha256 over the sorted file digests); with --json
                                  print {\"files\",\"suite_version\",\"tasks\"} or {\"errors\"}
    fid-eval suite check --suite <DIR> [--base <DIR>]... [--json]
                                  Load the suite fail-closed (format and red-team
                                  predicate schema) and print a summary; with --json
                                  print {\"families\",\"markers\",\"payloads\",\"predicates\",
                                  \"shell_only\",\"subfamilies\",\"suite_version\",\"tasks\",
                                  \"variants\"} or {\"errors\"}; markers counts the distinct
                                  entry_with_marker markers. With --base (repeatable),
                                  also check that every base-suite task has exactly one
                                  shell variant in the suite (AGENT.REDTEAM.SHELL) and
                                  add \"base_tasks\" to the summary
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
        ["suite", "digest", rest @ ..] => run(rest, Mode::Digest),
        ["suite", "check", rest @ ..] => run(rest, Mode::Check),
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

#[derive(Clone, Copy)]
enum Mode {
    Digest,
    Check,
}

fn summary(st: &Suite, mode: Mode, cov: Option<VariantCoverage>) -> CValue {
    let mut m = BTreeMap::from([
        ("suite_version".to_string(), s(st.version.as_str())),
        ("tasks".to_string(), CValue::Num(st.tasks.len() as f64)),
    ]);
    match mode {
        Mode::Digest => {
            m.insert("files".to_string(), CValue::Num(st.files.len() as f64));
        }
        Mode::Check => {
            let mut families: BTreeMap<String, CValue> = BTreeMap::new();
            let mut subfamilies: BTreeMap<String, CValue> = BTreeMap::new();
            let mut predicates = 0usize;
            let mut payloads = 0usize;
            let mut markers: BTreeSet<&str> = BTreeSet::new();
            let (mut variants, mut shell_only) = (0usize, 0usize);
            for t in &st.tasks {
                variants += usize::from(t.task.variant_of.is_some());
                shell_only += usize::from(t.task.shell_only);
                payloads += t.task.payloads.len();
                for p in &t.task.predicates {
                    if let PredicateKind::EntryWithMarker { marker } = &p.kind {
                        markers.insert(marker);
                    }
                }
                if let Some(f) = t.task.family {
                    let n = match families.get(f.as_str()) {
                        Some(CValue::Num(n)) => *n,
                        _ => 0.0,
                    };
                    families.insert(f.as_str().to_string(), CValue::Num(n + 1.0));
                }
                if let Some(sf) = t.task.subfamily {
                    let n = match subfamilies.get(sf.as_str()) {
                        Some(CValue::Num(n)) => *n,
                        _ => 0.0,
                    };
                    subfamilies.insert(sf.as_str().to_string(), CValue::Num(n + 1.0));
                }
                predicates += t.task.predicates.len();
            }
            m.insert("families".to_string(), CValue::Obj(families));
            m.insert("subfamilies".to_string(), CValue::Obj(subfamilies));
            m.insert("predicates".to_string(), CValue::Num(predicates as f64));
            m.insert("payloads".to_string(), CValue::Num(payloads as f64));
            m.insert("markers".to_string(), CValue::Num(markers.len() as f64));
            m.insert("variants".to_string(), CValue::Num(variants as f64));
            m.insert("shell_only".to_string(), CValue::Num(shell_only as f64));
            if let Some(c) = cov {
                m.insert("base_tasks".to_string(), CValue::Num(c.base_tasks as f64));
            }
        }
    }
    CValue::Obj(m)
}

fn run(args: &[&str], mode: Mode) -> ExitCode {
    let mut suite = None;
    let mut bases: Vec<&str> = Vec::new();
    let mut json = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match *a {
            "--json" => json = true,
            "--base" => match (mode, it.next()) {
                (Mode::Check, Some(d)) => bases.push(*d),
                (Mode::Check, None) => return usage_error("--base needs a directory"),
                (Mode::Digest, _) => return usage_error("--base is only allowed with suite check"),
            },
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
    let loaded = load_suite(Path::new(dir)).and_then(|st| {
        if bases.is_empty() {
            return Ok((st, None));
        }
        let mut errs = Vec::new();
        let mut base_suites = Vec::new();
        for b in &bases {
            match load_suite(Path::new(b)) {
                Ok(s) => base_suites.push(s),
                Err(es) => errs.extend(es.into_iter().map(|mut e| {
                    e.file = format!("{b}/{}", e.file);
                    e
                })),
            }
        }
        if !errs.is_empty() {
            return Err(errs);
        }
        let refs: Vec<(&Path, &Suite)> = bases.iter().map(|b| Path::new(*b)).zip(base_suites.iter()).collect();
        let cov = check_variants(Path::new(dir), &st, &refs)?;
        Ok((st, Some(cov)))
    });
    match loaded {
        Ok((st, cov)) => {
            match (json, mode) {
                (true, _) => print_json(&summary(&st, mode, cov)),
                (false, Mode::Digest) => println!("{}", st.version),
                (false, Mode::Check) => println!("ok: {} tasks, suite version {}", st.tasks.len(), st.version),
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
