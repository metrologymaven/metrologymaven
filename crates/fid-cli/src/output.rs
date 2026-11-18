//! The single output sink (spec node AUTO.CLI.OUT, REQ-AUTO.CLI.OUT-001 and -004).
//!
//! An [`Output`] owns stdout until the final envelope is ready: logs, warnings and
//! progress can only reach stderr, and the one stdout write happens in
//! [`Output::finish`], which consumes the sink.

use crate::exit::ExitCode;
use fid_canon::to_canonical_bytes;
use fid_surface::{valid_command_id, Envelope, ErrorCode};
use std::io::Write;

/// Command ID used for envelopes the CLI writes itself when no registered command applies
/// (usage errors before a command is resolved, internal errors with no valid command ID).
/// Not a registered command; the leaf's Open questions asks maintainers to settle it.
pub const CLI_COMMAND_ID: &str = "cli.invoke@1";

/// Output sink for one invocation.
pub struct Output<'a> {
    json: bool,
    stdout: &'a mut dyn Write,
    stderr: &'a mut dyn Write,
}

impl<'a> Output<'a> {
    /// A sink rendering JSON (`json == true`) or human-readable text.
    pub fn new(json: bool, stdout: &'a mut dyn Write, stderr: &'a mut dyn Write) -> Self {
        Output { json, stdout, stderr }
    }

    /// Whether `--json` is in effect.
    pub fn json(&self) -> bool {
        self.json
    }

    /// A log line (stderr).
    pub fn log(&mut self, msg: &str) {
        self.line("", msg);
    }

    /// A warning (stderr).
    pub fn warn(&mut self, msg: &str) {
        self.line("warning: ", msg);
    }

    /// A progress update (stderr).
    pub fn progress(&mut self, msg: &str) {
        self.line("progress: ", msg);
    }

    fn line(&mut self, prefix: &str, msg: &str) {
        // A failing stderr must not change the outcome; there is nowhere left to report it.
        let _ = writeln!(self.stderr, "{prefix}{}", sanitize(msg));
    }

    /// Writes the final envelope and returns the exit code (REQ-AUTO.CLI.OUT-001, -002, -004).
    ///
    /// With `--json`, stdout receives exactly one line of RFC 8785 JSON. If the envelope
    /// cannot be encoded, stdout stays empty, an `internal_error` envelope goes to stderr
    /// (preceded, without `--json`, by an `error[internal_error]:` line) and the exit code is 1.
    pub fn finish(self, env: &Envelope) -> ExitCode {
        let Output { json, stdout, stderr } = self;
        let bytes = match env.encode() {
            Ok(b) => b,
            Err(e) => return internal(json, stderr, &env.command_id, &format!("envelope could not be encoded: {e}")),
        };
        let text = if json {
            let mut b = bytes;
            b.push(b'\n');
            b
        } else {
            render_human(env).into_bytes()
        };
        match stdout.write_all(&text).and_then(|()| stdout.flush()) {
            Ok(()) => ExitCode::from_envelope(env),
            Err(e) => internal(json, stderr, &env.command_id, &format!("writing stdout: {e}")),
        }
    }
}

fn internal(json: bool, stderr: &mut dyn Write, command_id: &str, msg: &str) -> ExitCode {
    let id = if valid_command_id(command_id) { command_id } else { CLI_COMMAND_ID };
    let env = Envelope::failed(id, ErrorCode::InternalError, msg, None);
    // Human mode gets a readable line first; both modes get the envelope (Design, Errors).
    if !json {
        let _ = writeln!(stderr, "error[internal_error]: {}", sanitize(msg));
    }
    match env.encode() {
        Ok(mut b) => {
            b.push(b'\n');
            let _ = stderr.write_all(&b);
        }
        Err(_) if json => {
            let _ = writeln!(stderr, "error[internal_error]: {}", sanitize(msg));
        }
        Err(_) => {}
    }
    let _ = stderr.flush();
    ExitCode::Internal
}

/// Human-readable rendering of an envelope (REQ-AUTO.CLI.OUT-004). Deterministic (D0).
///
/// Lines: `<command_id>: <outcome>`, then `journal entry: <hash>` when committed,
/// `error[<code>]: <message>` when failed, `job:` / `request:` when present and the
/// result as canonical JSON. Control characters in text from the bus are replaced.
pub fn render_human(env: &Envelope) -> String {
    let mut s = format!("{}: {}\n", sanitize(&env.command_id), sanitize(env.outcome.as_str()));
    if let Some(h) = &env.journal_entry_hash {
        s.push_str(&format!("journal entry: {}\n", sanitize(h)));
    }
    if let Some(e) = &env.error {
        s.push_str(&format!("error[{}]: {}\n", sanitize(e.code.as_str()), sanitize(&e.message)));
    }
    if let Some(j) = &env.job_id {
        s.push_str(&format!("job: {}\n", sanitize(j)));
    }
    if let Some(r) = &env.request_id {
        s.push_str(&format!("request: {}\n", sanitize(r)));
    }
    if let Some(r) = &env.result {
        if let Ok(b) = to_canonical_bytes(r) {
            s.push_str(&format!("result: {}\n", sanitize(&String::from_utf8_lossy(&b))));
        }
    }
    s
}

/// Replaces control characters (including newlines and escape sequences) so untrusted
/// text cannot forge extra lines or drive the terminal.
fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { '\u{fffd}' } else { c }).collect()
}
