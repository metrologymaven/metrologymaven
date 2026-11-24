//! Target resolution and transport adapters (`AUTO.CLI.TARGET.{SELECT, APP, HEADLESS}`).

use crate::job::{run_tracked_invocation, InterruptSource, NoInterrupts};
use fid_bus_api::{BusApiErrorCode, BusEnvelope, BusOutcome, CommandBus, CommandCall};
use fid_canon::CValue;
use fid_ctrl::client::{self, ClientError, CtrlClient, Target as CtrlTarget, DEFAULT_TIMEOUT};
use fid_surface::jobs::{InstantClock, SystemKeySource, Tracker};
use fid_surface::roots::{is_handle, may_pass_raw_path, PrincipalKind};
use fid_surface::{Envelope, ErrorCode, Outcome};
use std::collections::BTreeMap;
use std::fmt;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

/// Resolved target for a CLI invocation (`AUTO.CLI.TARGET.SELECT`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Headless { project: String },
    App,
    None,
}

/// Target kind tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    Headless,
    App,
    None,
}

impl Target {
    pub fn kind(&self) -> TargetKind {
        match self {
            Target::Headless { .. } => TargetKind::Headless,
            Target::App => TargetKind::App,
            Target::None => TargetKind::None,
        }
    }
}

/// Target resolution or connection precondition errors (`AUTO.CLI.TARGET.*`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetError {
    TargetConflict,
    NoProjectSelected,
    RawPathRefused { param: String },
    HumanConfirmationFailed(String),
    AppNotFound(String),
    ServerAuthFailed(String),
    Conflict { holder: String },
}

impl TargetError {
    pub fn code(&self) -> &'static str {
        match self {
            TargetError::TargetConflict => "target_conflict",
            TargetError::NoProjectSelected => "no_project_selected",
            TargetError::RawPathRefused { .. } => "raw_path_refused",
            TargetError::HumanConfirmationFailed(_) => "human_confirmation_failed",
            TargetError::AppNotFound(_) => "app_not_found",
            TargetError::ServerAuthFailed(_) => "server_auth_failed",
            TargetError::Conflict { .. } => "conflict",
        }
    }

    pub fn to_envelope(&self, command_id: &str) -> Envelope {
        match self {
            TargetError::AppNotFound(msg) => {
                let mut m = BTreeMap::new();
                m.insert("reason".to_string(), CValue::Str("app_not_found".to_string()));
                m.insert("message".to_string(), CValue::Str(msg.clone()));
                Envelope { result: Some(CValue::Obj(m)), ..Envelope::bare(command_id, Outcome::NotRun) }
            }
            TargetError::ServerAuthFailed(msg) => {
                let mut m = BTreeMap::new();
                m.insert("reason".to_string(), CValue::Str("server_auth_failed".to_string()));
                m.insert("message".to_string(), CValue::Str(msg.clone()));
                Envelope { result: Some(CValue::Obj(m)), ..Envelope::bare(command_id, Outcome::NotRun) }
            }
            TargetError::TargetConflict | TargetError::NoProjectSelected => {
                Envelope::failed(command_id, ErrorCode::UsageError, &self.to_string(), None)
            }
            TargetError::RawPathRefused { .. } => {
                Envelope::failed(command_id, ErrorCode::RawPathRefused, &self.to_string(), None)
            }
            TargetError::HumanConfirmationFailed(_) => {
                Envelope::failed(command_id, ErrorCode::PolicyRefused, &self.to_string(), None)
            }
            TargetError::Conflict { .. } => Envelope::failed(command_id, ErrorCode::Conflict, &self.to_string(), None),
        }
    }
}

impl fmt::Display for TargetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TargetError::TargetConflict => {
                f.write_str("target_conflict: `--project` and `--app` cannot be used together")
            }
            TargetError::NoProjectSelected => {
                f.write_str("no_project_selected: command requires a project (`--project`, `--app`, or `FID_PROJECT`)")
            }
            TargetError::RawPathRefused { param } => {
                write!(f, "raw_path_refused: `{param}` requires a `root:` handle for non-human principal")
            }
            TargetError::HumanConfirmationFailed(why) => write!(f, "human_confirmation_failed: {why}"),
            TargetError::AppNotFound(why) => write!(f, "app_not_found: {why}"),
            TargetError::ServerAuthFailed(why) => write!(f, "server_auth_failed: {why}"),
            TargetError::Conflict { holder } => write!(f, "conflict: project locked by {holder}"),
        }
    }
}

impl std::error::Error for TargetError {}

/// Pure target resolution from CLI flags and environment snapshot
/// (REQ-AUTO.CLI.TARGET.SELECT-001..005).
pub fn resolve_target(
    project_flag: Option<&str>,
    app_flag: bool,
    fid_project_env: Option<&str>,
    needs_project: bool,
) -> Result<Target, TargetError> {
    if project_flag.is_some() && app_flag {
        return Err(TargetError::TargetConflict);
    }
    if let Some(proj) = project_flag {
        return Ok(Target::Headless { project: proj.to_string() });
    }
    if app_flag {
        return Ok(Target::App);
    }
    if let Some(proj) = fid_project_env.filter(|s| !s.is_empty()) {
        return Ok(Target::Headless { project: proj.to_string() });
    }
    if needs_project {
        return Err(TargetError::NoProjectSelected);
    }
    Ok(Target::None)
}

/// Validates a `--project` / `FID_PROJECT` value against the session principal kind
/// (REQ-AUTO.CLI.TARGET.HEADLESS-005).
pub fn validate_project_ref(project: &str, principal_kind: PrincipalKind) -> Result<(), TargetError> {
    if !is_handle(project) && !may_pass_raw_path(principal_kind) {
        return Err(TargetError::RawPathRefused { param: "--project".to_string() });
    }
    Ok(())
}

/// Control-channel transport implementing [`CommandBus`] over [`CtrlClient`] + [`Tracker`]
/// (`AUTO.CLI.TARGET.APP`).
pub struct AppCtrlBus<'a> {
    tracker: Tracker<CtrlClient, InstantClock, SystemKeySource>,
    timeout: Duration,
    stderr: &'a mut dyn Write,
}

impl<'a> AppCtrlBus<'a> {
    /// Connects and authenticates to the running app within `connect_timeout` (default 5 s).
    pub fn connect(
        runtime_dir: Option<&str>,
        token_file: Option<&str>,
        env: &dyn Fn(&str) -> Option<String>,
        connect_timeout: Option<Duration>,
        exec_timeout: Duration,
        stderr: &'a mut dyn Write,
    ) -> Result<Self, TargetError> {
        let rt_dir = runtime_dir
            .map(ToString::to_string)
            .or_else(|| env(crate::ctrl::RUNTIME_DIR_ENV).filter(|s| !s.is_empty()))
            .ok_or_else(|| TargetError::AppNotFound("no runtime directory configured".to_string()))?;
        let tok_path = token_file
            .map(ToString::to_string)
            .or_else(|| env(crate::ctrl::TOKEN_FILE_ENV).filter(|s| !s.is_empty()))
            .ok_or_else(|| TargetError::AppNotFound("no token file configured".to_string()))?;

        let target = CtrlTarget {
            runtime_dir: PathBuf::from(rt_dir),
            token_path: PathBuf::from(tok_path),
            instance_id: None,
            timeout: connect_timeout.unwrap_or(DEFAULT_TIMEOUT),
        };
        let client = client::connect(&target).map_err(map_ctrl_error)?;
        let tracker = Tracker::new(client, InstantClock::new(), SystemKeySource::new());
        Ok(Self { tracker, timeout: exec_timeout, stderr })
    }

    /// Runs a command with a caller-supplied [`InterruptSource`] (used by `AUTO.CLI.JOB`).
    pub fn dispatch_with_interrupts(
        &mut self,
        call: &CommandCall,
        interrupts: &mut dyn InterruptSource,
        is_tty: bool,
    ) -> BusEnvelope {
        let cmd = match call.parsed_command_id() {
            Ok(id) => id,
            Err(err) => return err.to_envelope(call.full_command_id()),
        };
        let timeout = call.options.timeout_ms.map(Duration::from_millis).unwrap_or(self.timeout);
        let env = run_tracked_invocation(
            &mut self.tracker,
            cmd,
            call.params.clone(),
            timeout,
            interrupts,
            is_tty,
            self.stderr,
        );
        surface_to_bus_envelope(&env)
    }
}

impl CommandBus for AppCtrlBus<'_> {
    fn dispatch(&mut self, call: &CommandCall) -> BusEnvelope {
        let mut no_ints = NoInterrupts;
        self.dispatch_with_interrupts(call, &mut no_ints, false)
    }

    fn supports(&self, _command_id: &str, _major: u32) -> bool {
        true
    }
}

fn map_ctrl_error(err: ClientError) -> TargetError {
    match err {
        ClientError::ServerAuthFailed | ClientError::Token(_) | ClientError::InstanceMismatch => {
            TargetError::ServerAuthFailed(err.to_string())
        }
        other => TargetError::AppNotFound(other.to_string()),
    }
}

/// Converts a `fid_surface::Envelope` into a `fid_bus_api::BusEnvelope`.
pub fn surface_to_bus_envelope(env: &Envelope) -> BusEnvelope {
    BusEnvelope {
        command: env.command_id.clone(),
        outcome: BusOutcome::from_wire(env.outcome.as_str()),
        result: env.result.clone(),
        error_code: env.error.as_ref().map(|e| BusApiErrorCode::from_wire(e.code.as_str())),
        message: env.error.as_ref().map(|e| e.message.clone()),
        error_details: env.error.as_ref().and_then(|e| e.details.clone()),
        request_id: env.request_id.clone(),
        journal_entry: env.journal_entry_hash.clone(),
        job_id: env.job_id.clone(),
        artifacts: Vec::new(),
    }
}

/// Converts a `fid_bus_api::BusEnvelope` into a `fid_surface::Envelope`.
pub fn bus_to_surface_envelope(env: &BusEnvelope) -> Envelope {
    let outcome = Outcome::from_wire(env.outcome.as_str());
    let error = if outcome == Outcome::Failed {
        let code = env.error_code.as_ref().map_or(ErrorCode::InternalError, |c| ErrorCode::from_wire(c.as_str()));
        Some(fid_surface::ErrorBody {
            code,
            message: env.message.clone().unwrap_or_else(|| "command failed".to_string()),
            details: env.error_details.clone(),
        })
    } else {
        None
    };
    Envelope {
        envelope_version: fid_surface::ENVELOPE_VERSION,
        command_id: env.command.clone(),
        outcome,
        error,
        journal_entry_hash: env.journal_entry.clone(),
        job_id: env.job_id.clone(),
        request_id: env.request_id.clone(),
        result: env.result.clone(),
    }
}
