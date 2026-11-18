//! `fid`: the Metrology Maven command line.
//!
//! First slice: `fid canon` exposes the canonical-JSON crate (CORE.JRN.CANON) so
//! people, agents and CI can hash journal-style JSON exactly the way the kernel will.
//! The registry-driven command surface (AUTO.SURF) replaces this hand-written parser later.
//! Exit codes and `--json` output follow AUTO.CLI.OUT (`fid_cli::ExitCode`, `fid_cli::Output`).

use fid_cli::{ExitCode as Fx, Output};
use std::io::{Read, Write};
use std::process::ExitCode;

const USAGE: &str = "\
fid: Metrology Maven command line (pre-alpha)

USAGE:
    fid canon [FILE] [--digest]   Print RFC 8785 canonical JSON of FILE (or stdin);
                                  with --digest print only its sha256 digest
    fid canon [FILE] --json       Print a result envelope (schema/envelope-v1.json)
                                  with the canonical value and its digest
                                  (usage and file errors still print plain text)
    fid --version                 Print the version
    fid --help                    Print this help

EXIT CODES (docs/cli/exit-codes.md):";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
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
            eprintln!("error: unknown command `{other}`\n\n{}", help());
            Fx::Usage.into()
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

fn canon(args: &[String]) -> ExitCode {
    let mut file = None;
    let mut want_digest = false;
    let mut want_json = false;
    for a in args {
        match a.as_str() {
            "--digest" => want_digest = true,
            "--json" => want_json = true,
            s if s.starts_with('-') && s != "-" => {
                eprintln!("error: unknown option `{s}`");
                return Fx::Usage.into();
            }
            s if file.is_none() => file = Some(s.to_string()),
            _ => {
                eprintln!("error: more than one input file");
                return Fx::Usage.into();
            }
        }
    }
    let input = match file.as_deref() {
        None | Some("-") => {
            let mut buf = Vec::new();
            match std::io::stdin().take(fid_canon::MAX_INPUT_BYTES as u64 + 1).read_to_end(&mut buf) {
                Ok(_) => buf,
                Err(e) => {
                    eprintln!("error: reading stdin: {e}");
                    return Fx::Failed.into();
                }
            }
        }
        Some(path) => match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("error: reading {path}: {e}");
                return Fx::Failed.into();
            }
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
    use fid_canon::CValue;
    use fid_surface::{Envelope, ErrorCode};
    use std::collections::BTreeMap;

    const COMMAND: &str = "canon.canonicalize@1";
    let env = match fid_canon::parse(input).and_then(|v| fid_canon::digest(&v).map(|d| (v, d))) {
        Ok((value, digest)) => {
            let mut result = BTreeMap::new();
            result.insert("canonical".to_string(), value);
            result.insert("digest".to_string(), CValue::Str(digest.to_string()));
            Envelope::computed(COMMAND, CValue::Obj(result))
        }
        Err(e) => {
            let details = CValue::Obj(BTreeMap::from([("canon_error".to_string(), CValue::Str(e.code().into()))]));
            Envelope::failed(COMMAND, ErrorCode::ValidationError, &e.to_string(), Some(details))
        }
    };
    let mut stdout = std::io::stdout().lock();
    let mut stderr = std::io::stderr().lock();
    Output::new(true, &mut stdout, &mut stderr).finish(&env).into()
}
