//! `fid`: the Metrology Maven command line.
//!
//! First slice: `fid canon` exposes the canonical-JSON crate (CORE.JRN.CANON) so
//! people, agents and CI can hash journal-style JSON exactly the way the kernel will.
//! The registry-driven command surface (AUTO.SURF) replaces this hand-written parser later.
//! Exit codes and `--json` output follow AUTO.CLI.OUT (`fid_cli::ExitCode`, `fid_cli::Output`):
//! under `--json` every result, including usage and input errors, is one envelope on stdout.

use fid_canon::CValue;
use fid_cli::{ExitCode as Fx, Output, CLI_COMMAND_ID};
use fid_surface::{Envelope, ErrorCode};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::process::ExitCode;

const USAGE: &str = "\
fid: Metrology Maven command line (pre-alpha)

USAGE:
    fid canon [FILE] [--digest]   Print RFC 8785 canonical JSON of FILE (or stdin);
                                  with --digest print only its sha256 digest
    fid canon [FILE] --json       Print a result envelope (schema/envelope-v1.json)
                                  with the canonical value and its digest
    fid --version                 Print the version
    fid --help                    Print this help

With --json, stdout holds exactly one result envelope, also for usage and input errors.

EXIT CODES (docs/cli/exit-codes.md):";

const CANON: &str = "canon.canonicalize@1";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let json = args.iter().any(|a| a == "--json");
    match args.first().map(String::as_str) {
        Some("--version" | "-V") => {
            println!("fid {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        None | Some("--help" | "-h" | "help") => {
            println!("{}", help());
            ExitCode::SUCCESS
        }
        Some("canon") => canon(&args[1..]),
        Some(other) => {
            let msg = format!("unknown command `{other}`");
            if !json {
                eprintln!("error: {msg}\n\n{}", help());
                return Fx::Usage.into();
            }
            emit(true, &Envelope::failed(CLI_COMMAND_ID, ErrorCode::UsageError, &msg, None))
        }
    }
}

/// The usage text followed by the exit-code table, generated from [`fid_cli::ExitCode`].
fn help() -> String {
    let mut s = USAGE.to_string();
    for c in Fx::ALL {
        s.push_str(&format!("\n    {}  {:<15} {}", c.code(), c.name(), c.meaning()));
    }
    s
}

/// Writes the final envelope through the single output sink.
fn emit(json: bool, env: &Envelope) -> ExitCode {
    let mut stdout = std::io::stdout().lock();
    let mut stderr = std::io::stderr().lock();
    Output::new(json, &mut stdout, &mut stderr).finish(env).into()
}

/// A `fid canon` error: a failed envelope under `--json`, a plain stderr line otherwise.
fn canon_error(json: bool, code: ErrorCode, msg: &str) -> ExitCode {
    let env = Envelope::failed(CANON, code, msg, None);
    if json {
        return emit(true, &env);
    }
    eprintln!("error: {msg}");
    Fx::from_envelope(&env).into()
}

fn read_error(json: bool, what: &str, e: &std::io::Error) -> ExitCode {
    let code =
        if e.kind() == std::io::ErrorKind::NotFound { ErrorCode::NotFound } else { ErrorCode::PreconditionFailed };
    canon_error(json, code, &format!("reading {what}: {e}"))
}

fn canon(args: &[String]) -> ExitCode {
    let want_json = args.iter().any(|a| a == "--json");
    let mut file = None;
    let mut want_digest = false;
    for a in args {
        match a.as_str() {
            "--digest" => want_digest = true,
            "--json" => {}
            s if s.starts_with('-') && s != "-" => {
                return canon_error(want_json, ErrorCode::UsageError, &format!("unknown option `{s}`"));
            }
            s if file.is_none() => file = Some(s.to_string()),
            _ => return canon_error(want_json, ErrorCode::UsageError, "more than one input file"),
        }
    }
    let input = match file.as_deref() {
        None | Some("-") => {
            let mut buf = Vec::new();
            match std::io::stdin().take(fid_canon::MAX_INPUT_BYTES as u64 + 1).read_to_end(&mut buf) {
                Ok(_) => buf,
                Err(e) => return read_error(want_json, "stdin", &e),
            }
        }
        Some(path) => match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => return read_error(want_json, path, &e),
        },
    };
    if want_json {
        return canon_json(&input);
    }
    match fid_canon::canonicalize(&input) {
        Ok(bytes) => {
            let mut out = std::io::stdout().lock();
            let res = if want_digest {
                writeln!(out, "{}", fid_canon::digest_bytes(&bytes))
            } else {
                out.write_all(&bytes).and_then(|()| writeln!(out))
            };
            match res {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("error: writing output: {e}");
                    Fx::Internal.into()
                }
            }
        }
        Err(e) => {
            eprintln!("error[{}]: {e}", e.code());
            Fx::Failed.into()
        }
    }
}

/// `fid canon --json`: the same work, reported as a result envelope (AUTO.SURF.ENV)
/// through the single output sink (AUTO.CLI.OUT).
fn canon_json(input: &[u8]) -> ExitCode {
    let env = match fid_canon::parse(input).and_then(|v| fid_canon::digest(&v).map(|d| (v, d))) {
        Ok((value, digest)) => {
            let mut result = BTreeMap::new();
            result.insert("canonical".to_string(), value);
            result.insert("digest".to_string(), CValue::Str(digest.to_string()));
            Envelope::computed(CANON, CValue::Obj(result))
        }
        Err(e) => {
            let details = CValue::Obj(BTreeMap::from([("canon_error".to_string(), CValue::Str(e.code().into()))]));
            Envelope::failed(CANON, ErrorCode::ValidationError, &e.to_string(), Some(details))
        }
    };
    emit(true, &env)
}
