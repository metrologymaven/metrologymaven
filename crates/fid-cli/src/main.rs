//! `fid`: the Metrology Maven command line.
//!
//! First slice: `fid canon` exposes the canonical-JSON crate (CORE.JRN.CANON) so
//! people, agents and CI can hash journal-style JSON exactly the way the kernel will.
//! The registry-driven command surface (AUTO.SURF) replaces this hand-written parser later.

use std::io::{Read, Write};
use std::process::ExitCode;

const USAGE: &str = "\
fid: Metrology Maven command line (pre-alpha)

USAGE:
    fid canon [FILE] [--digest]   Print RFC 8785 canonical JSON of FILE (or stdin);
                                  with --digest print only its sha256 digest
    fid --version                 Print the version
    fid --help                    Print this help

Exit codes: 0 ok, 1 invalid input, 2 usage error.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version" | "-V") => {
            println!("fid {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        None | Some("--help" | "-h" | "help") => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some("canon") => canon(&args[1..]),
        Some(other) => {
            eprintln!("error: unknown command `{other}`\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn canon(args: &[String]) -> ExitCode {
    let mut file = None;
    let mut want_digest = false;
    for a in args {
        match a.as_str() {
            "--digest" => want_digest = true,
            s if s.starts_with('-') && s != "-" => {
                eprintln!("error: unknown option `{s}`");
                return ExitCode::from(2);
            }
            s if file.is_none() => file = Some(s.to_string()),
            _ => {
                eprintln!("error: more than one input file");
                return ExitCode::from(2);
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
                    return ExitCode::from(1);
                }
            }
        }
        Some(path) => match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("error: reading {path}: {e}");
                return ExitCode::from(1);
            }
        },
    };
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
                    ExitCode::from(1)
                }
            }
        }
        Err(e) => {
            eprintln!("error[{}]: {e}", e.code());
            ExitCode::from(1)
        }
    }
}
