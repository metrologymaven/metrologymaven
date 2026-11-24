//! Standalone offline Result Package verifier CLI (`fid-verify`).
//!
//! Spec nodes `TRUST.VERIFY.CLI` and `TRUST.VERIFY.REPORT`.

use fid_canon::{to_canonical_bytes, CValue};
use fid_pkg::reader::FileOpener;
use fid_verify::report::{EXIT_USAGE, REPORT_VERSION, SUPPORTED_PACKAGE_FORMATS, TOOL_NAME, TOOL_VERSION};
use fid_verify::runner::{verify_package, VerifyError, VerifyOptions};
use std::collections::BTreeMap;
use std::process::ExitCode;

const USAGE: &str = "\
usage: fid-verify <package> [options]
       fid-verify --version [--json]
       fid-verify --help

options:
  --trust-root <file>     Offline trust-root JSON file
  --revoked <file>        Offline revocation JSON file
  --keys <file>           Offline keyring JSON file (known untrusted keys)
  --as-of <rfc3339-utc>   Reference UTC timestamp (signer-asserted check only)
  --replay                Run replay verification (points/features tier)
  --rep1 <file>           Custom REP-1 equivalence table JSON
  --kernel-cache <dir>    Local directory of cached WASM kernel artifacts
  --fail-fast             Stop after the first failing section
  --json                  Emit canonical RFC 8785 JSON report
  --memory-limit <size>   Replay memory limit (e.g. 64MiB)
  --time-budget <dur>     Replay time budget (e.g. 10s, 500ms, 0s)
  --version               Print tool, report, and package format versions
  -h, --help              Print this help message
";

enum Action {
    Help,
    Version { json: bool },
    Verify { opts: VerifyOptions, json: bool },
}

fn take_value<'a, I>(flag: &str, inline: Option<&'a str>, it: &mut I) -> Result<&'a str, VerifyError>
where
    I: Iterator<Item = &'a str>,
{
    if let Some(v) = inline {
        if v.is_empty() {
            return Err(VerifyError::usage(format!("{flag} requires a non-empty value")));
        }
        return Ok(v);
    }
    match it.next() {
        Some(v) if !v.starts_with("--") && !v.is_empty() => Ok(v),
        _ => Err(VerifyError::usage(format!("{flag} requires a value"))),
    }
}

fn parse_args(args: &[String]) -> Result<Action, (VerifyError, bool)> {
    let raw: Vec<&str> = args.iter().map(String::as_str).collect();
    if raw.is_empty() {
        return Err((VerifyError::usage("missing <package> argument"), false));
    }

    if raw[0] == "mcp" {
        return Err((
            VerifyError::usage(
                "verify.mcp_unavailable: `fid-verify mcp` server is not yet implemented (TRUST.VERIFY.MCP)",
            ),
            false,
        ));
    }

    let mut json = false;
    let mut version = false;
    let mut help = false;
    let mut replay = false;
    let mut fail_fast = false;
    let mut package: Option<String> = None;
    let mut trust_root: Option<String> = None;
    let mut revoked: Option<String> = None;
    let mut keys: Option<String> = None;
    let mut as_of: Option<String> = None;
    let mut rep1: Option<String> = None;
    let mut kernel_cache: Option<String> = None;
    let mut memory_limit: Option<String> = None;
    let mut time_budget: Option<String> = None;

    let mut it = raw.iter().copied();
    while let Some(arg) = it.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if arg.starts_with("--") => (f, Some(v)),
            _ => (arg, None),
        };
        match flag {
            "-h" | "--help" if inline.is_none() => help = true,
            "--version" if inline.is_none() => version = true,
            "--json" if inline.is_none() => {
                if json {
                    return Err((VerifyError::usage("repeated --json flag"), true));
                }
                json = true;
            }
            "--replay" if inline.is_none() => {
                if replay {
                    return Err((VerifyError::usage("repeated --replay flag"), json));
                }
                replay = true;
            }
            "--fail-fast" if inline.is_none() => {
                if fail_fast {
                    return Err((VerifyError::usage("repeated --fail-fast flag"), json));
                }
                fail_fast = true;
            }
            "--trust-root" => {
                if trust_root.is_some() {
                    return Err((VerifyError::usage("repeated --trust-root"), json));
                }
                trust_root = Some(take_value(flag, inline, &mut it).map_err(|e| (e, json))?.to_string());
            }
            "--revoked" => {
                if revoked.is_some() {
                    return Err((VerifyError::usage("repeated --revoked"), json));
                }
                revoked = Some(take_value(flag, inline, &mut it).map_err(|e| (e, json))?.to_string());
            }
            "--keys" => {
                if keys.is_some() {
                    return Err((VerifyError::usage("repeated --keys"), json));
                }
                keys = Some(take_value(flag, inline, &mut it).map_err(|e| (e, json))?.to_string());
            }
            "--as-of" => {
                if as_of.is_some() {
                    return Err((VerifyError::usage("repeated --as-of"), json));
                }
                as_of = Some(take_value(flag, inline, &mut it).map_err(|e| (e, json))?.to_string());
            }
            "--rep1" => {
                if rep1.is_some() {
                    return Err((VerifyError::usage("repeated --rep1"), json));
                }
                rep1 = Some(take_value(flag, inline, &mut it).map_err(|e| (e, json))?.to_string());
            }
            "--kernel-cache" => {
                if kernel_cache.is_some() {
                    return Err((VerifyError::usage("repeated --kernel-cache"), json));
                }
                kernel_cache = Some(take_value(flag, inline, &mut it).map_err(|e| (e, json))?.to_string());
            }
            "--memory-limit" => {
                if memory_limit.is_some() {
                    return Err((VerifyError::usage("repeated --memory-limit"), json));
                }
                memory_limit = Some(take_value(flag, inline, &mut it).map_err(|e| (e, json))?.to_string());
            }
            "--time-budget" => {
                if time_budget.is_some() {
                    return Err((VerifyError::usage("repeated --time-budget"), json));
                }
                time_budget = Some(take_value(flag, inline, &mut it).map_err(|e| (e, json))?.to_string());
            }
            _ if arg.starts_with('-') => {
                return Err((VerifyError::usage(format!("unknown option {arg:?}")), json));
            }
            _ => {
                if package.is_some() {
                    return Err((VerifyError::usage("more than one <package> argument"), json));
                }
                package = Some(arg.to_string());
            }
        }
    }

    if help {
        return Ok(Action::Help);
    }
    if version {
        if package.is_some()
            || trust_root.is_some()
            || revoked.is_some()
            || keys.is_some()
            || as_of.is_some()
            || replay
            || rep1.is_some()
            || kernel_cache.is_some()
            || fail_fast
            || memory_limit.is_some()
            || time_budget.is_some()
        {
            return Err((VerifyError::usage("--version cannot be combined with verification options"), json));
        }
        return Ok(Action::Version { json });
    }

    let Some(package) = package else {
        return Err((VerifyError::usage("missing <package> argument"), json));
    };

    Ok(Action::Verify {
        opts: VerifyOptions {
            package,
            trust_root,
            revoked,
            keys,
            as_of,
            replay,
            rep1,
            kernel_cache,
            fail_fast,
            memory_limit,
            time_budget,
        },
        json,
    })
}

fn version_json() -> String {
    let mut o = BTreeMap::new();
    o.insert("report_version".to_string(), CValue::Str(REPORT_VERSION.to_string()));
    o.insert(
        "supported_package_formats".to_string(),
        CValue::Arr(SUPPORTED_PACKAGE_FORMATS.iter().map(|n| CValue::Num(f64::from(*n))).collect()),
    );
    o.insert("tool".to_string(), CValue::Str(TOOL_NAME.to_string()));
    o.insert("version".to_string(), CValue::Str(TOOL_VERSION.to_string()));
    let bytes = to_canonical_bytes(&CValue::Obj(o)).unwrap_or_default();
    String::from_utf8(bytes).unwrap_or_default()
}

fn print_error(err: &VerifyError, json: bool) -> ExitCode {
    if json {
        let bytes = err.to_canonical_json();
        let text = String::from_utf8_lossy(&bytes);
        eprintln!("{text}");
    } else {
        eprintln!("error: {err}");
        if err.exit_code() == EXIT_USAGE {
            eprintln!("{USAGE}");
        }
    }
    ExitCode::from(err.exit_code())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let action = match parse_args(&args) {
        Ok(a) => a,
        Err((err, json)) => return print_error(&err, json),
    };
    match action {
        Action::Help => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Action::Version { json: true } => {
            println!("{}", version_json());
            ExitCode::SUCCESS
        }
        Action::Version { json: false } => {
            let formats: Vec<String> = SUPPORTED_PACKAGE_FORMATS.iter().map(u32::to_string).collect();
            println!(
                "{TOOL_NAME} {TOOL_VERSION} (report_version {REPORT_VERSION}, package_formats [{}])",
                formats.join(", ")
            );
            ExitCode::SUCCESS
        }
        Action::Verify { opts, json } => {
            let opener = FileOpener;
            match verify_package(&opener, &opts) {
                Ok(report) => {
                    if json {
                        let bytes = report.to_canonical_bytes();
                        let text = String::from_utf8_lossy(&bytes);
                        println!("{text}");
                    } else {
                        print!("{}", report.render_human());
                    }
                    ExitCode::from(report.exit_code)
                }
                Err(err) => print_error(&err, json),
            }
        }
    }
}
