//! Runtime CLI command dispatch pipeline (`AUTO.CLI.DISPATCH`):
//! `args` -> `gate` -> `target` -> `fid_bus_api::CommandBus` -> `Output` -> `ExitCode`.

use crate::args::{parse_command_args, ArgError, ParamsReader, StdParamsReader};
use crate::exit::ExitCode;
use crate::gate::{evaluate_gate, GateDecision};
use crate::output::Output;
use crate::target::{
    bus_to_surface_envelope, resolve_target, surface_to_bus_envelope, validate_project_ref, AppCtrlBus, Target,
    TargetError,
};
use crate::tree::CliTree;
use fid_bus_api::{
    BusApiError, BusEnvelope, CallOptions, ChannelKind, CommandBus, CommandCall, PrincipalInfo, TargetHint,
};
use fid_canon::CValue;
use fid_surface::roots::PrincipalKind as SurfacePrincipalKind;
use fid_surface::{Envelope, ErrorCode, SurfaceProjection};
use fid_types::{CommandId, SideEffect};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::time::Duration;

/// Default command execution timeout when `--wait-timeout` and `--timeout` are omitted.
pub const DEFAULT_EXEC_TIMEOUT: Duration = Duration::from_secs(60);

/// Pure-Rust fallback bus used by standalone `fid` when no `--app` control channel or external
/// SQLite bus host is attached.
#[derive(Debug, Default, Clone, Copy)]
pub struct StandaloneCliBus;

impl CommandBus for StandaloneCliBus {
    fn dispatch(&mut self, call: &CommandCall) -> BusEnvelope {
        let full = call.full_command_id();
        BusApiError::precondition_failed(format!(
            "no_bus_host: standalone `fid` cannot execute `{full}` without a running `--app` control channel or attached bus host"
        ))
        .to_envelope(full)
    }

    fn supports(&self, _command_id: &str, _major: u32) -> bool {
        false
    }
}

/// Infers whether a command requires a project from its side-effect class and parameters.
pub fn command_needs_project(side_effect: SideEffect) -> bool {
    matches!(
        side_effect,
        SideEffect::R1
            | SideEffect::W1
            | SideEffect::W2
            | SideEffect::W3
            | SideEffect::I1
            | SideEffect::D1
            | SideEffect::M1
            | SideEffect::S1
    )
}

/// Prepared bus invocation paired with parsed `--runtime-dir` and `--token-file` flags.
pub struct PreparedDispatch {
    pub call: CommandCall,
    pub runtime_dir: Option<String>,
    pub token_file: Option<String>,
}

/// Validates and constructs a [`CommandCall`] through `args` -> `gate` -> `target` without
/// touching any bus.
#[allow(clippy::too_many_arguments)]
pub fn prepare_dispatch(
    tree: &CliTree,
    command_id: &CommandId,
    major_num: u32,
    json: bool,
    rest: &[String],
    env: &dyn Fn(&str) -> Option<String>,
    stdin: &mut dyn Read,
    surface_proj: Option<&SurfaceProjection>,
) -> Result<PreparedDispatch, Envelope> {
    let Some(cmd_entry) = tree.domains.values().flat_map(|v| v.iter()).find(|c| c.command_id == command_id.name())
    else {
        return Err(Envelope::failed(
            command_id.as_str(),
            ErrorCode::NotFound,
            &format!("unknown command `{}`", command_id.as_str()),
            None,
        ));
    };
    let Some(cli_major) = cmd_entry.major(major_num) else {
        return Err(Envelope::failed(
            command_id.as_str(),
            ErrorCode::UsageError,
            &format!("command `{}` has no major {major_num}", command_id.name()),
            None,
        ));
    };

    let path_like_set: Option<BTreeSet<String>> =
        surface_proj.map(|sp| sp.params.iter().filter(|p| p.path_like).map(|p| p.name.clone()).collect());
    let reader = StdParamsReader;
    let parsed = parse_command_args(cli_major, rest, json, stdin, path_like_set.as_ref(), &reader as &dyn ParamsReader)
        .map_err(|e| arg_error_envelope(command_id.as_str(), &e))?;

    let principal_kind = SurfacePrincipalKind::Script;
    let dry_run = match evaluate_gate(cli_major, &parsed, principal_kind, surface_proj) {
        GateDecision::Proceed { dry_run } => dry_run,
        GateDecision::Refuse(ref_env) => return Err(ref_env),
    };

    if parsed.flags.confirm_human {
        // Standalone CLI cannot synthesize OS user presence without a `CORE.BUS.PRIN` provider.
        let err = TargetError::HumanConfirmationFailed("no user-presence provider attached".to_string());
        return Err(err.to_envelope(command_id.as_str()));
    }

    let fid_proj_env = env("FID_PROJECT");
    let target = resolve_target(
        parsed.flags.project.as_deref(),
        parsed.flags.app,
        fid_proj_env.as_deref(),
        command_needs_project(cli_major.side_effect),
    )
    .map_err(|err| err.to_envelope(command_id.as_str()))?;

    if let Target::Headless { ref project } = target {
        validate_project_ref(project, principal_kind).map_err(|err| err.to_envelope(command_id.as_str()))?;
    }

    let timeout_ms = parsed.flags.timeout_ms.or_else(|| parsed.flags.wait_timeout_secs.map(|s| s.saturating_mul(1000)));
    let target_hint = match &target {
        Target::App => TargetHint::App { socket_or_endpoint: parsed.flags.runtime_dir.clone() },
        Target::Headless { project } => {
            TargetHint::Headless { project: Some(project.clone()), branch: parsed.flags.branch.clone() }
        }
        Target::None => TargetHint::None,
    };

    let call = CommandCall::new(command_id.name(), major_num, parsed.params.clone(), PrincipalInfo::script("fid-cli"))
        .with_channel(ChannelKind::Cli)
        .with_target(target_hint)
        .with_options(CallOptions {
            dry_run,
            timeout_ms,
            reason: parsed.flags.reason.clone(),
            expected_head: parsed.flags.expected_head,
            idempotency_key: None,
            approval_of: None,
            human_confirmed: parsed.flags.confirm_human,
            json: parsed.flags.json,
        });

    Ok(PreparedDispatch { call, runtime_dir: parsed.flags.runtime_dir, token_file: parsed.flags.token_file })
}

/// Dispatches one resolved command through `args` -> `gate` -> `target` -> `bus` -> `Output`.
#[allow(clippy::too_many_arguments)]
pub fn dispatch_resolved(
    tree: &CliTree,
    command_id: &CommandId,
    major_num: u32,
    json: bool,
    rest: &[String],
    bus: &mut dyn CommandBus,
    env: &dyn Fn(&str) -> Option<String>,
    stdin: &mut dyn Read,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    surface_proj: Option<&SurfaceProjection>,
) -> ExitCode {
    let out = Output::new(json, stdout, stderr);
    let prepared = match prepare_dispatch(tree, command_id, major_num, json, rest, env, stdin, surface_proj) {
        Ok(p) => p,
        Err(ref_env) => return out.finish(&ref_env),
    };

    let cmd_str = command_id.as_str().to_string();
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| bus.dispatch(&prepared.call)));
    let env = match res {
        Ok(bus_env) => bus_to_surface_envelope(&bus_env),
        Err(_) => Envelope::failed(&cmd_str, ErrorCode::InternalError, "the command panicked", None),
    };
    out.finish(&env)
}

struct LazyBinaryBus<'a> {
    runtime_dir: Option<String>,
    token_file: Option<String>,
    env: &'a dyn Fn(&str) -> Option<String>,
    stderr: &'a mut dyn Write,
}

impl CommandBus for LazyBinaryBus<'_> {
    fn dispatch(&mut self, call: &CommandCall) -> BusEnvelope {
        match &call.target {
            TargetHint::App { socket_or_endpoint } => {
                let rt = socket_or_endpoint.as_deref().or(self.runtime_dir.as_deref());
                let timeout = call.options.timeout_ms.map(Duration::from_millis).unwrap_or(DEFAULT_EXEC_TIMEOUT);
                match AppCtrlBus::connect(rt, self.token_file.as_deref(), self.env, None, timeout, self.stderr) {
                    Ok(mut app) => app.dispatch(call),
                    Err(err) => surface_to_bus_envelope(&err.to_envelope(&call.full_command_id())),
                }
            }
            TargetHint::Auto | TargetHint::Headless { .. } | TargetHint::None => {
                let mut standalone = StandaloneCliBus;
                standalone.dispatch(call)
            }
        }
    }

    fn supports(&self, _command_id: &str, _major: u32) -> bool {
        true
    }
}

/// Entry point used by the `fid` binary: evaluates args, gate, and target resolution first,
/// then connects lazily to `AppCtrlBus` (for `--app`) or [`StandaloneCliBus`].
#[allow(clippy::too_many_arguments)]
pub fn run_binary_dispatch(
    tree: &CliTree,
    command_id: &CommandId,
    major_num: u32,
    json: bool,
    rest: &[String],
    env: &dyn Fn(&str) -> Option<String>,
    stdin: &mut dyn Read,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> ExitCode {
    let sp = crate::tree::embedded_projection()
        .and_then(|proj| crate::tree::find_surface_projection(proj, tree, command_id, major_num));
    let prepared = match prepare_dispatch(tree, command_id, major_num, json, rest, env, stdin, sp) {
        Ok(p) => p,
        Err(ref_env) => return Output::new(json, stdout, stderr).finish(&ref_env),
    };

    let mut bus_stderr = Vec::new();
    let cmd_str = command_id.as_str().to_string();
    let res = {
        let mut lazy_bus = LazyBinaryBus {
            runtime_dir: prepared.runtime_dir,
            token_file: prepared.token_file,
            env,
            stderr: &mut bus_stderr,
        };
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lazy_bus.dispatch(&prepared.call)))
    };
    let env_out = match res {
        Ok(bus_env) => bus_to_surface_envelope(&bus_env),
        Err(_) => Envelope::failed(&cmd_str, ErrorCode::InternalError, "the command panicked", None),
    };
    let rc = Output::new(json, stdout, stderr).finish(&env_out);
    let _ = stderr.write_all(&bus_stderr);
    rc
}

fn arg_error_envelope(command_id: &str, err: &ArgError) -> Envelope {
    let mut details = BTreeMap::new();
    details.insert("arg_error".to_string(), CValue::Str(err.code().to_string()));
    Envelope::failed(command_id, ErrorCode::UsageError, &err.to_string(), Some(CValue::Obj(details)))
}
