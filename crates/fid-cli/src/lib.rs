//! Library half of the `fid` binary.
//!
//! Spec node AUTO.CLI.OUT: JSON and human output with exit codes. The exit-code table is
//! [`ExitCode`] (defined once, rendered to `docs/cli/exit-codes.md`); [`Output`] is the
//! single sink that keeps stdout for the final envelope. [`invoke`] runs one
//! `fid <domain> <verb-object> [--json]` call against a [`Bus`] and is what the fake-bus
//! acceptance tests drive until the generated command tree (AUTO.CLI.TREE) and the bus
//! API crate land.

pub mod ctrl;
pub mod exit;
pub mod output;
pub mod tree;

pub use exit::{exit_codes_markdown, ExitCode, EXIT_CODES_DOC};
pub use output::{render_human, Output, CLI_COMMAND_ID};
pub use tree::{
    embedded_tree, emit, levenshtein, nearest_match, parse_tree, resolve_args, CliCommand, CliEmitOutput, CliMajor,
    CliParam, CliTree, CliTreeError, ResolveOutcome, RESERVED_HOST_SUBCOMMANDS, SURFACE, TREE_FILE, TREE_SCHEMA,
};

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
    let out = Output::new(json, stdout, stderr);
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
    call_bus(bus, &id, out)
}

/// Resolves `args` against a generated [`CliTree`] (handling `--version`, `--help`, `--major`,
/// and nearest-match `usage_error` suggestions) and dispatches registered commands to `bus`
/// (REQ-AUTO.CLI.TREE-002..005).
pub fn invoke_with_tree(
    tree: &CliTree,
    version: &str,
    args: &[String],
    bus: &mut dyn Bus,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> ExitCode {
    match resolve_args(tree, args) {
        ResolveOutcome::Version => {
            if writeln!(stdout, "{}", tree.render_version(version)).is_err() {
                return ExitCode::Internal;
            }
            ExitCode::Success
        }
        ResolveOutcome::RootHelp { json: true } => match tree.help_json(version) {
            Ok(bytes) => {
                if stdout.write_all(&bytes).is_err() {
                    return ExitCode::Internal;
                }
                ExitCode::Success
            }
            Err(e) => {
                let _ = writeln!(stderr, "error: {e}");
                ExitCode::Internal
            }
        },
        ResolveOutcome::RootHelp { json: false } => {
            if stdout.write_all(tree.render_root_help(version).as_bytes()).is_err() {
                return ExitCode::Internal;
            }
            ExitCode::Success
        }
        ResolveOutcome::DomainHelp { domain } => {
            if let Some(text) = tree.render_domain_help(&domain) {
                if stdout.write_all(text.as_bytes()).is_err() {
                    return ExitCode::Internal;
                }
                ExitCode::Success
            } else {
                let _ = writeln!(stderr, "error: unknown domain `{domain}`");
                ExitCode::Usage
            }
        }
        ResolveOutcome::CommandHelp { domain, verb, major } => {
            if let Some(cmd) = tree.find(&domain, &verb) {
                if let Some(maj) = cmd.major(major) {
                    if stdout.write_all(tree.render_command_help(cmd, maj).as_bytes()).is_err() {
                        return ExitCode::Internal;
                    }
                    return ExitCode::Success;
                }
            }
            let _ = writeln!(stderr, "error: unknown command `{domain} {verb}`");
            ExitCode::Usage
        }
        ResolveOutcome::HostSubcommand { name, .. } => {
            let _ = writeln!(stderr, "error: host subcommand `{name}` is handled by the binary");
            ExitCode::Usage
        }
        ResolveOutcome::Invoke { command_id, json, .. } => {
            let out = Output::new(json, stdout, stderr);
            call_bus(bus, command_id.as_str(), out)
        }
        ResolveOutcome::UsageError { message, json, .. } => {
            if json {
                let out = Output::new(true, stdout, stderr);
                out.finish(&usage(&message))
            } else {
                let _ = writeln!(stderr, "error: {message}");
                ExitCode::Usage
            }
        }
    }
}

fn call_bus(bus: &mut dyn Bus, id: &str, mut out: Output<'_>) -> ExitCode {
    // A panic in the bus would exit 101, outside the table: report it as internal (exit 1).
    let called = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| bus.call(id, &mut out)));
    let env = match called {
        Ok(env) => env,
        Err(_) => Envelope::failed(id, ErrorCode::InternalError, "the command panicked", None),
    };
    out.finish(&env)
}

fn usage(msg: &str) -> Envelope {
    Envelope::failed(CLI_COMMAND_ID, ErrorCode::UsageError, msg, None)
}
