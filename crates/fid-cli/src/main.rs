//! `fid`: the Metrology Maven command line.
//!
//! First slice: `fid canon` exposes the canonical-JSON crate (CORE.JRN.CANON) so
//! people, agents and CI can hash journal-style JSON exactly the way the kernel will.
//! `fid ctrl status` / `fid ctrl ping` are control-channel diagnostics (AUTO.CTRL.CLIENT).
//! The registry-driven command surface (AUTO.SURF) replaces this hand-written parser later.
//! Exit codes and `--json` output follow AUTO.CLI.OUT (`fid_cli::ExitCode`, `fid_cli::Output`):
//! under `--json` every result, including usage and input errors, is one envelope on stdout.

use fid_canon::CValue;
use fid_cli::{embedded_tree, resolve_args, ExitCode as Fx, Output, ResolveOutcome, CLI_COMMAND_ID};
use fid_surface::{Envelope, ErrorCode};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::process::ExitCode;

const CANON: &str = "canon.canonicalize@1";
const MCP_HELP: &str = "\
fid mcp: Model Context Protocol host subcommand (AUTO.MCP)

USAGE:
    fid mcp --help    Print this help
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("canon") {
        return canon(&args[1..]);
    }
    let tree = match embedded_tree() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error: embedded CLI tree is invalid: {e}");
            return Fx::Internal.into();
        }
    };
    let version = env!("CARGO_PKG_VERSION");
    match resolve_args(&tree, &args) {
        ResolveOutcome::Version => {
            println!("{}", tree.render_version(version));
            ExitCode::SUCCESS
        }
        ResolveOutcome::RootHelp { json: true } => match tree.help_json(version) {
            Ok(bytes) => {
                let mut stdout = std::io::stdout().lock();
                if stdout.write_all(&bytes).is_err() {
                    return Fx::Internal.into();
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("error: {e}");
                Fx::Internal.into()
            }
        },
        ResolveOutcome::RootHelp { json: false } => {
            print!("{}", tree.render_root_help(version));
            ExitCode::SUCCESS
        }
        ResolveOutcome::DomainHelp { domain } => match tree.render_domain_help(&domain) {
            Some(text) => {
                print!("{text}");
                ExitCode::SUCCESS
            }
            None => {
                eprintln!("error: unknown domain `{domain}`");
                Fx::Usage.into()
            }
        },
        ResolveOutcome::CommandHelp { domain, verb, major } => {
            if let Some(cmd) = tree.find(&domain, &verb) {
                if let Some(maj) = cmd.major(major) {
                    print!("{}", tree.render_command_help(cmd, maj));
                    return ExitCode::SUCCESS;
                }
            }
            eprintln!("error: unknown command `{domain} {verb}`");
            Fx::Usage.into()
        }
        ResolveOutcome::HostSubcommand { name, rest } => match name.as_str() {
            "ctrl" => {
                let env = |k: &str| std::env::var(k).ok();
                let mut stdout = std::io::stdout().lock();
                let mut stderr = std::io::stderr().lock();
                fid_cli::ctrl::run(&rest, &env, &mut stdout, &mut stderr).into()
            }
            "mcp" => {
                if rest.is_empty() || matches!(rest.first().map(String::as_str), Some("--help" | "-h")) {
                    print!("{MCP_HELP}");
                    ExitCode::SUCCESS
                } else {
                    let msg = format!("unknown mcp option `{}`", rest[0]);
                    if rest.iter().any(|a| a == "--json") {
                        emit(true, &Envelope::failed(CLI_COMMAND_ID, ErrorCode::UsageError, &msg, None))
                    } else {
                        eprintln!("error: {msg}");
                        Fx::Usage.into()
                    }
                }
            }
            other => {
                eprintln!("error: unknown host subcommand `{other}`");
                Fx::Usage.into()
            }
        },
        ResolveOutcome::Invoke { command_id, json, .. } => {
            let msg = format!(
                "command `{}` resolved from embedded CLI tree; runtime bus dispatch is provided by AUTO.CLI.DISPATCH",
                command_id.as_str()
            );
            let env = Envelope::failed(command_id.as_str(), ErrorCode::PreconditionFailed, &msg, None);
            if json {
                emit(true, &env)
            } else {
                eprintln!("error: {msg}");
                Fx::from_envelope(&env).into()
            }
        }
        ResolveOutcome::UsageError { message, json, .. } => {
            if json {
                emit(true, &Envelope::failed(CLI_COMMAND_ID, ErrorCode::UsageError, &message, None))
            } else {
                eprintln!("error: {message}");
                Fx::Usage.into()
            }
        }
    }
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
