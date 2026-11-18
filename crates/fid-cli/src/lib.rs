//! Library half of the `fid` binary.
//!
//! Spec node AUTO.CLI.OUT: JSON and human output with exit codes. The exit-code table is
//! [`ExitCode`] (defined once, rendered to `docs/cli/exit-codes.md`); [`Output`] is the
//! single sink that keeps stdout for the final envelope. [`invoke`] runs one
//! `fid <domain> <verb-object> [--json]` call against a [`Bus`] and is what the fake-bus
//! acceptance tests drive until the generated command tree (AUTO.CLI.TREE) and the bus
//! API crate land.

pub mod exit;
pub mod output;

pub use exit::{exit_codes_markdown, ExitCode, EXIT_CODES_DOC};
pub use output::{render_human, Output, CLI_COMMAND_ID};

use fid_surface::{Envelope, ErrorCode};
use std::io::Write;

/// What the CLI needs from the command bus. Diagnostics go through the [`Output`], which
/// only lets them reach stderr.
pub trait Bus {
    /// Resolves `<domain> <verb-object>` to a registered command ID (`name@major`).
    fn resolve(&self, domain: &str, verb_object: &str) -> Option<String>;
    /// Runs the command and returns its final envelope.
    fn call(&mut self, command_id: &str, out: &mut Output<'_>) -> Envelope;
}

/// Runs `fid <domain> <verb-object> [--json]` against `bus` and returns the exit code.
pub fn invoke(args: &[String], bus: &mut dyn Bus, stdout: &mut dyn Write, stderr: &mut dyn Write) -> ExitCode {
    let json = args.iter().any(|a| a == "--json");
    let mut out = Output::new(json, stdout, stderr);
    let mut pos = Vec::new();
    for a in args {
        match a.as_str() {
            "--json" => {}
            s if s.starts_with('-') => return out.finish(&usage(&format!("unknown option `{s}`"))),
            s => pos.push(s),
        }
    }
    let (domain, verb) = match pos.as_slice() {
        [d, v] => (*d, *v),
        _ => return out.finish(&usage("expected `fid <domain> <verb-object> [--json]`")),
    };
    let Some(id) = bus.resolve(domain, verb) else {
        return out.finish(&usage(&format!("unknown command `{domain} {verb}`")));
    };
    let env = bus.call(&id, &mut out);
    out.finish(&env)
}

fn usage(msg: &str) -> Envelope {
    Envelope::failed(CLI_COMMAND_ID, ErrorCode::UsageError, msg, None)
}
