//! `fid ctrl status` and `fid ctrl ping` (spec node AUTO.CTRL.CLIENT, REQ-004 and REQ-005).
//!
//! Host subcommands, not registry commands: they change no state in the app, so CONSTITUTION C3
//! does not apply. Both find the app only through its discovery file, authenticate with the
//! mutual HMAC handshake and close again. The envelopes they print use the command IDs
//! [`STATUS_ID`] and [`PING_ID`], which, like `cli.invoke@1`, are not registered commands.
//!
//! The runtime directory and token file locations per OS belong to PLAT, which has not fixed
//! them yet, so both are given with `--runtime-dir` / `--token-file` or the environment
//! variables [`RUNTIME_DIR_ENV`] / [`TOKEN_FILE_ENV`].
//!
//! Outcomes: an authenticated app gives `computed` (exit 0); no reachable app gives `not_run`
//! (exit 6, "nothing ran") with `found: false`; an app that was found but could not be
//! authenticated gives `failed` (exit 4, or 3 for `ctrl_non_loopback`) with `found`,
//! `authenticated` and `ctrl_error` in the error details.

use crate::{ExitCode, Output};
use fid_canon::CValue;
use fid_ctrl::client::{self, ClientError, Target, DEFAULT_TIMEOUT};
use fid_surface::{Envelope, ErrorCode, Outcome};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

/// Envelope command ID of `fid ctrl status` (not a registered command).
pub const STATUS_ID: &str = "ctrl.status@1";
/// Envelope command ID of `fid ctrl ping` (not a registered command).
pub const PING_ID: &str = "ctrl.ping@1";
/// Environment variable naming the runtime directory holding discovery files.
pub const RUNTIME_DIR_ENV: &str = "FID_CTRL_RUNTIME_DIR";
/// Environment variable naming the app's token file.
pub const TOKEN_FILE_ENV: &str = "FID_CTRL_TOKEN_FILE";
/// Most pings one `fid ctrl ping --count` sends.
pub const MAX_PINGS: u32 = 100;
/// Longest `--timeout-ms`.
pub const MAX_TIMEOUT_MS: u64 = 600_000;

/// Usage text of `fid ctrl`.
pub const USAGE: &str = "\
    fid ctrl status [OPTIONS]     Report whether an app is found, authenticated, and the
                                  protocol version (exit 6 when no app is reachable)
    fid ctrl ping [OPTIONS]       Report the round-trip time of an authenticated ping
        --runtime-dir DIR         Discovery directory (or FID_CTRL_RUNTIME_DIR)
        --token-file FILE         The app's token file (or FID_CTRL_TOKEN_FILE)
        --instance ID             Only this app instance
        --timeout-ms N            Connect and authenticate deadline (default 5000)
        --count N                 ping only: number of pings (default 1, at most 100)
        --json                    Print one result envelope";

struct Args {
    verb: Verb,
    target: Target,
    count: u32,
}

#[derive(Clone, Copy)]
enum Verb {
    Status,
    Ping,
}

impl Verb {
    fn id(self) -> &'static str {
        match self {
            Verb::Status => STATUS_ID,
            Verb::Ping => PING_ID,
        }
    }
}

fn usage(id: &str, msg: &str) -> Envelope {
    Envelope::failed(id, ErrorCode::UsageError, msg, None)
}

fn parse(args: &[String], env: &dyn Fn(&str) -> Option<String>) -> Result<Args, Envelope> {
    let verb = match args.first().map(String::as_str) {
        Some("status") => Verb::Status,
        Some("ping") => Verb::Ping,
        Some(other) => return Err(usage(crate::CLI_COMMAND_ID, &format!("unknown ctrl subcommand `{other}`"))),
        None => return Err(usage(crate::CLI_COMMAND_ID, "expected `fid ctrl status` or `fid ctrl ping`")),
    };
    let id = verb.id();
    let mut runtime_dir = None;
    let mut token = None;
    let mut instance = None;
    let mut timeout = DEFAULT_TIMEOUT;
    let mut count = 1u32;
    let mut it = args[1..].iter();
    while let Some(a) = it.next() {
        let mut value = |name: &str| it.next().cloned().ok_or_else(|| usage(id, &format!("{name} needs a value")));
        match a.as_str() {
            "--json" => {}
            "--runtime-dir" => runtime_dir = Some(PathBuf::from(value("--runtime-dir")?)),
            "--token-file" => token = Some(PathBuf::from(value("--token-file")?)),
            "--instance" => instance = Some(value("--instance")?),
            "--timeout-ms" => {
                let v = value("--timeout-ms")?;
                match v.parse::<u64>() {
                    Ok(ms) if (1..=MAX_TIMEOUT_MS).contains(&ms) => timeout = Duration::from_millis(ms),
                    _ => return Err(usage(id, "--timeout-ms must be an integer from 1 to 600000")),
                }
            }
            "--count" if matches!(verb, Verb::Ping) => {
                let v = value("--count")?;
                match v.parse::<u32>() {
                    Ok(n) if (1..=MAX_PINGS).contains(&n) => count = n,
                    _ => return Err(usage(id, "--count must be an integer from 1 to 100")),
                }
            }
            other => return Err(usage(id, &format!("unknown option `{other}`"))),
        }
    }
    let runtime_dir = runtime_dir
        .or_else(|| env(RUNTIME_DIR_ENV).filter(|s| !s.is_empty()).map(PathBuf::from))
        .ok_or_else(|| usage(id, "no runtime directory: pass --runtime-dir or set FID_CTRL_RUNTIME_DIR"))?;
    let token_path = token
        .or_else(|| env(TOKEN_FILE_ENV).filter(|s| !s.is_empty()).map(PathBuf::from))
        .ok_or_else(|| usage(id, "no token file: pass --token-file or set FID_CTRL_TOKEN_FILE"))?;
    Ok(Args { verb, target: Target { runtime_dir, token_path, instance_id: instance, timeout }, count })
}

fn obj(fields: Vec<(&str, CValue)>) -> CValue {
    CValue::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect::<BTreeMap<_, _>>())
}

fn opt_str(s: Option<String>) -> CValue {
    s.map_or(CValue::Null, CValue::Str)
}

/// Milliseconds with microsecond resolution.
fn millis(d: Duration) -> CValue {
    CValue::Num(d.as_micros() as f64 / 1000.0)
}

/// The envelope for a connection that did not get as far as an authenticated session.
fn not_connected(id: &str, e: &ClientError) -> Envelope {
    match e {
        // REQ-AUTO.CTRL.CLIENT-004: no reachable app is `not_run` (exit 6); nothing ran.
        ClientError::AppNotFound(_) => Envelope {
            result: Some(obj(vec![
                ("found", CValue::Bool(false)),
                ("authenticated", CValue::Bool(false)),
                ("version", CValue::Null),
                ("ctrl_error", CValue::Str(e.code().to_string())),
                ("message", CValue::Str(e.to_string())),
            ])),
            ..Envelope::bare(id, Outcome::NotRun)
        },
        _ => {
            let code = match e {
                ClientError::NonLoopback(_) => ErrorCode::PolicyRefused,
                ClientError::VersionUnsupported(_) => ErrorCode::VersionMismatch,
                ClientError::Timeout => ErrorCode::Timeout,
                _ => ErrorCode::PreconditionFailed,
            };
            let found = !matches!(e, ClientError::NonLoopback(_));
            let details = obj(vec![
                ("found", CValue::Bool(found)),
                ("authenticated", CValue::Bool(false)),
                ("version", CValue::Null),
                ("ctrl_error", CValue::Str(e.code().to_string())),
            ]);
            Envelope::failed(id, code, &e.to_string(), Some(details))
        }
    }
}

fn status(target: &Target) -> Envelope {
    let s = client::status(target);
    match (&s.error, s.version) {
        (None, Some(v)) => Envelope::computed(
            STATUS_ID,
            obj(vec![
                ("found", CValue::Bool(s.found)),
                ("authenticated", CValue::Bool(s.authenticated)),
                ("version", CValue::Num(f64::from(v))),
                ("instance_id", opt_str(s.instance_id)),
                ("address", opt_str(s.address.map(|a| a.ip().to_string()))),
                ("port", s.address.map_or(CValue::Null, |a| CValue::Num(f64::from(a.port())))),
            ]),
        ),
        (Some(e), _) => not_connected(STATUS_ID, e),
        (None, None) => Envelope::failed(STATUS_ID, ErrorCode::InternalError, "status without a version", None),
    }
}

fn ping(target: &Target, count: u32) -> Envelope {
    let mut c = match client::connect(target) {
        Ok(c) => c,
        Err(e) => return not_connected(PING_ID, &e),
    };
    let mut rtts = Vec::new();
    for _ in 0..count {
        match c.ping(target.timeout) {
            Ok(rtt) => rtts.push(rtt),
            Err(e) => {
                let details = obj(vec![
                    ("found", CValue::Bool(true)),
                    ("authenticated", CValue::Bool(true)),
                    ("ctrl_error", CValue::Str(e.code().to_string())),
                ]);
                let code = if e == ClientError::Timeout { ErrorCode::Timeout } else { ErrorCode::PreconditionFailed };
                return Envelope::failed(PING_ID, code, &e.to_string(), Some(details));
            }
        }
    }
    let d = c.discovery().clone();
    let version = c.version();
    c.close();
    let min = rtts.iter().min().copied().unwrap_or_default();
    let max = rtts.iter().max().copied().unwrap_or_default();
    let total: Duration = rtts.iter().sum();
    let avg = total / count.max(1);
    Envelope::computed(
        PING_ID,
        obj(vec![
            ("rtt_ms", millis(avg)),
            ("min_ms", millis(min)),
            ("max_ms", millis(max)),
            ("count", CValue::Num(f64::from(count))),
            ("version", CValue::Num(f64::from(version))),
            ("instance_id", CValue::Str(d.instance_id)),
        ]),
    )
}

/// Runs `fid ctrl <args>` (`args` without the leading `ctrl`) and returns the exit code.
/// `env` reads an environment variable.
pub fn run(
    args: &[String],
    env: &dyn Fn(&str) -> Option<String>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> ExitCode {
    let json = args.iter().any(|a| a == "--json");
    let out = Output::new(json, stdout, stderr);
    let env = match parse(args, env) {
        Ok(a) => match a.verb {
            Verb::Status => status(&a.target),
            Verb::Ping => ping(&a.target, a.count),
        },
        Err(e) => e,
    };
    out.finish(&env)
}
