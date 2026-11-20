//! Root handles and raw-path refusal, surface side. Spec node `AUTO.SURF.ROOTS`.
//!
//! Automation names files only through handles `root:<root_id>/<relative path>` below an
//! approved read (import) or write (export) root (REQ-AUTO.SURF-008). This module is the
//! surface-side gate in front of the bus-side guard `CORE.BUS.POL.FS` (`fid_bus::policy::fs`,
//! defence in depth). Every surface (CLI, MCP, Python, control channel) runs it before a call
//! leaves the surface:
//!
//! 1. [`is_root_config`]: one predicate, from registry metadata, for commands that change the
//!    read or write roots. Surfaces do not list ([`visible_commands`]) or invoke
//!    ([`gate_root_config`]) such a command for any principal but `human` (REQ-006).
//! 2. [`gate_raw_paths`]: a raw value in a `path_like` parameter from any principal but
//!    `human` is refused with `raw_path_refused` before anything reaches the bus (REQ-005).
//!
//! [`admit`] runs both, in that order, and [`dispatch_gated`] hands the parameters to the bus
//! only after both passed.
//!
//! **One grammar, one resolver.** Handle parsing, root lookup, containment after following
//! links and junctions, and the checked open (REQ-001 to REQ-004) are not repeated here: they
//! are `fid_bus::policy::fs::{FileHandle::parse, resolve, ResolvedPath::open_read,
//! ResolvedPath::create_new}`, the single implementation, which runs when the bus executes the
//! call. A surface crate may not link the bus (CORE.BUS.LINT `surface_dep`: surfaces reach
//! CORE only through the bus API crate), so this module duplicates exactly three small rules,
//! and `tests/roots.rs` proves on the whole hand-labelled corpus of CORE.BUS.POL.FS that the
//! surface and the bus give the same verdict:
//!
//! - [`is_handle`]: a value is in handle form when it starts with `root:` (case-sensitive);
//! - [`may_pass_raw_path`]: the principal policy table (only `human` may pass a raw path);
//! - the order of [`gate_raw_paths`]: a nested `path_like` declaration first, then the
//!   top-level `path_like` parameters in key byte order, the first raw value refused and a
//!   non-string value `bad_handle`.
//!
//! **Principal.** The kind is the one the bus derived from the call's channel
//! (`fid_bus::principal::SessionTable::derive_principal`, then `Principal::kind`), never a
//! field of the request: [`PrincipalKind`] has no parser, so a surface cannot build one from
//! request text.
//!
//! **Errors.** Every refusal is a [`RootError`] that maps to an existing bus code and reason
//! ([`RootError::bus_code`], [`RootError::bus_reason`]) and to the envelope error code set
//! ([`RootError::error_code`]). The leaf's error names map onto the bus catalog as listed in
//! [`ERROR_MAP`].

use crate::projection::{Projection, SurfaceProjection};
use crate::{Envelope, ErrorCode};
use fid_canon::CValue;
use fid_types::SideEffect;
use std::collections::BTreeMap;
use std::fmt;

/// The handle scheme (as in `fid_bus::policy::fs::HANDLE_PREFIX`).
pub const HANDLE_PREFIX: &str = "root:";

/// The declared effect (journal event type) that marks a command as changing the read or
/// write roots. A command lists it in its registry `effects`.
pub const ROOTS_EFFECT: &str = "RootsChanged";

/// The leaf's error names and the bus `code`/`reason` each one is reported as: the bus error
/// catalog (`bus-errors.json`, CORE.BUS.PIPE.ERR) is closed, so this module adds no code.
/// `unknown_root` and `wrong_access` are one bus reason (`bad_root`); `io_error` is
/// `precondition/false` for a missing file or folder and `internal/storage` for any other
/// file-system fault. The bus reports the resolution errors; the surface reports
/// `raw_path_refused`, `bad_handle` (a non-string `path_like` value), `root_config_hidden` and
/// `nested_path_like`.
pub const ERROR_MAP: &[(&str, &str, &str)] = &[
    ("bad_handle", "policy", "bad_handle"),
    ("unknown_root", "policy", "bad_root"),
    ("wrong_access", "policy", "bad_root"),
    ("root_escape", "policy", "outside_root"),
    ("raw_path_refused", "policy", "raw_path_refused"),
    ("io_error", "precondition", "false"),
    ("io_error", "internal", "storage"),
    ("root_config_hidden", "policy", "human_only"),
    ("nested_path_like", "internal", "invariant_violated"),
];

/// The kind of the principal a call runs as, as the bus derived it (CORE.BUS.PRIN). The same
/// six kinds and wire strings as `fid_bus::principal::PrincipalKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PrincipalKind {
    Human,
    Agent,
    Script,
    Service,
    Device,
    System,
}

impl PrincipalKind {
    /// Every kind, in declaration order.
    pub const ALL: &'static [PrincipalKind] = &[
        PrincipalKind::Human,
        PrincipalKind::Agent,
        PrincipalKind::Script,
        PrincipalKind::Service,
        PrincipalKind::Device,
        PrincipalKind::System,
    ];

    /// The wire string. There is deliberately no parser: the kind comes from the bus's
    /// derived principal, never from text in a request.
    pub fn as_str(self) -> &'static str {
        match self {
            PrincipalKind::Human => "human",
            PrincipalKind::Agent => "agent",
            PrincipalKind::Script => "script",
            PrincipalKind::Service => "service",
            PrincipalKind::Device => "device",
            PrincipalKind::System => "system",
        }
    }
}

impl fmt::Display for PrincipalKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The principal policy table: may a principal of this kind pass a raw path in a `path_like`
/// parameter? `human` only (REQ-005 names agent, script, service and device; `system` is
/// refused too, fail closed, as on the bus). One arm per kind, no wildcard.
pub fn may_pass_raw_path(kind: PrincipalKind) -> bool {
    match kind {
        PrincipalKind::Human => true,
        PrincipalKind::Agent
        | PrincipalKind::Script
        | PrincipalKind::Service
        | PrincipalKind::Device
        | PrincipalKind::System => false,
    }
}

/// True if `value` is in handle form (starts with `root:`, case-sensitive). Anything else in a
/// `path_like` parameter is a raw path. Whether the handle is well formed is the bus's check.
pub fn is_handle(value: &str) -> bool {
    value.starts_with(HANDLE_PREFIX)
}

/// A refusal of the surface-side gate. Each maps to one bus code and reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootError {
    /// A principal other than `human` passed a raw path for `param`.
    RawPathRefused { kind: PrincipalKind, param: String },
    /// A `path_like` parameter holds something other than a string.
    BadHandle { param: String },
    /// The command changes the roots and the principal is not `human`.
    RootConfigHidden { kind: PrincipalKind, command: String },
    /// The command declares a nested `path_like` parameter, which neither gate can check.
    NestedPathLike { command: String },
}

impl RootError {
    /// The leaf's error name (see [`ERROR_MAP`]).
    pub fn code(&self) -> &'static str {
        match self {
            RootError::RawPathRefused { .. } => "raw_path_refused",
            RootError::BadHandle { .. } => "bad_handle",
            RootError::RootConfigHidden { .. } => "root_config_hidden",
            RootError::NestedPathLike { .. } => "nested_path_like",
        }
    }

    /// The bus error code (`bus-errors.json`).
    pub fn bus_code(&self) -> &'static str {
        match self {
            RootError::RawPathRefused { .. } | RootError::BadHandle { .. } | RootError::RootConfigHidden { .. } => {
                "policy"
            }
            RootError::NestedPathLike { .. } => "internal",
        }
    }

    /// The bus reason within [`RootError::bus_code`].
    pub fn bus_reason(&self) -> &'static str {
        match self {
            RootError::RawPathRefused { .. } => "raw_path_refused",
            RootError::BadHandle { .. } => "bad_handle",
            RootError::RootConfigHidden { .. } => "human_only",
            RootError::NestedPathLike { .. } => "invariant_violated",
        }
    }

    /// The envelope error code (AUTO.SURF.ENV).
    pub fn error_code(&self) -> ErrorCode {
        match self {
            RootError::RawPathRefused { .. } => ErrorCode::RawPathRefused,
            RootError::BadHandle { .. } => ErrorCode::ValidationError,
            RootError::RootConfigHidden { .. } => ErrorCode::PolicyRefused,
            RootError::NestedPathLike { .. } => ErrorCode::InternalError,
        }
    }

    /// The `failed` envelope a surface returns for this refusal; `details` carries the bus
    /// `code` and `reason` and the leaf name.
    pub fn to_envelope(&self, command_id: &str) -> Envelope {
        let mut d = BTreeMap::new();
        d.insert("bus_code".to_string(), CValue::Str(self.bus_code().to_string()));
        d.insert("bus_reason".to_string(), CValue::Str(self.bus_reason().to_string()));
        d.insert("roots_error".to_string(), CValue::Str(self.code().to_string()));
        Envelope::failed(command_id, self.error_code(), &self.to_string(), Some(CValue::Obj(d)))
    }
}

impl fmt::Display for RootError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RootError::RawPathRefused { kind, param } => {
                write!(f, "raw_path_refused: a {kind} principal must pass a root: handle in {param:?}, not a raw path")
            }
            RootError::BadHandle { param } => write!(f, "bad_handle: path_like parameter {param:?} must be a string"),
            RootError::RootConfigHidden { kind, command } => {
                write!(f, "root_config_hidden: {command} changes the file roots; a {kind} principal may not run it")
            }
            RootError::NestedPathLike { command } => {
                write!(f, "nested_path_like: {command} declares a nested path_like parameter, which cannot be checked")
            }
        }
    }
}

impl std::error::Error for RootError {}

/// The pinned rule behind [`is_root_config`], on bare registry metadata: true if `effects`
/// lists [`ROOTS_EFFECT`], or the class is A0. Roots are an A0 admin setting (CORE.STO.SET,
/// `bus.fs_roots`) changed through the generic `settings.set_admin_value@1`, whose declared
/// effect names no key, so every A0 command counts as root-changing (fail closed; A0 is
/// human-only under CONSTITUTION §3 anyway). No command name is listed anywhere.
pub fn root_config_by_metadata(class: SideEffect, effects: &[String]) -> bool {
    class == SideEffect::A0 || effects.iter().any(|e| e == ROOTS_EFFECT)
}

/// REQ-006: true for every command whose declared metadata says it changes the read or write
/// roots ([`root_config_by_metadata`]). The one predicate every surface uses.
pub fn is_root_config(command: &SurfaceProjection) -> bool {
    root_config_by_metadata(command.side_effect, &command.effects)
}

/// Whether a surface may list or invoke `command` for a principal of `kind`: a root-changing
/// command for `human` only.
pub fn may_offer(kind: PrincipalKind, command: &SurfaceProjection) -> bool {
    kind == PrincipalKind::Human || !is_root_config(command)
}

/// The projection filter every surface lists commands through: the commands of `projection`
/// in projection order, without the root-changing ones unless `kind` is `human`.
pub fn visible_commands(kind: PrincipalKind, projection: &Projection) -> Vec<&SurfaceProjection> {
    projection.commands.iter().filter(|c| may_offer(kind, c)).collect()
}

/// Refuses a root-changing command for any principal but `human` (`root_config_hidden`).
pub fn gate_root_config(kind: PrincipalKind, command: &SurfaceProjection) -> Result<(), RootError> {
    if may_offer(kind, command) {
        Ok(())
    } else {
        Err(RootError::RootConfigHidden { kind, command: command.command_id.as_str().to_string() })
    }
}

/// REQ-005: every `path_like` parameter present in `params` must hold a string, and, for any
/// principal but `human`, a `root:` handle. Runs before anything is sent to the bus, in the
/// same order as the bus's `check_raw_paths`: a nested `path_like` declaration first
/// (`nested_path_like`), then the top-level `path_like` parameters in key byte order; the
/// first raw value gives `raw_path_refused`, a non-string value `bad_handle`. Parameters not
/// declared `path_like` are not looked at; `params` that is not an object has no file
/// parameter to refuse (the bus's schema validation refuses it).
pub fn gate_raw_paths(kind: PrincipalKind, command: &SurfaceProjection, params: &CValue) -> Result<(), RootError> {
    if command.nested_path_like {
        return Err(RootError::NestedPathLike { command: command.command_id.as_str().to_string() });
    }
    let CValue::Obj(values) = params else { return Ok(()) };
    for p in command.params.iter().filter(|p| p.path_like) {
        match values.get(&p.name) {
            None => {}
            Some(CValue::Str(s)) => {
                if !is_handle(s) && !may_pass_raw_path(kind) {
                    return Err(RootError::RawPathRefused { kind, param: p.name.clone() });
                }
            }
            Some(_) => return Err(RootError::BadHandle { param: p.name.clone() }),
        }
    }
    Ok(())
}

/// Both gates of a call, in order: [`gate_root_config`], then [`gate_raw_paths`].
pub fn admit(kind: PrincipalKind, command: &SurfaceProjection, params: &CValue) -> Result<(), RootError> {
    gate_root_config(kind, command)?;
    gate_raw_paths(kind, command, params)
}

/// Runs [`admit`] and, only if it passes, `send` (the surface's hand-off to the bus). On a
/// refusal `send` is never called, so nothing reaches the bus.
pub fn dispatch_gated<R>(
    kind: PrincipalKind,
    command: &SurfaceProjection,
    params: &CValue,
    send: impl FnOnce(&CValue) -> R,
) -> Result<R, RootError> {
    admit(kind, command, params)?;
    Ok(send(params))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_table_is_human_only() {
        let allowed: Vec<_> = PrincipalKind::ALL.iter().filter(|k| may_pass_raw_path(**k)).collect();
        assert_eq!(allowed, vec![&PrincipalKind::Human]);
    }

    #[test]
    fn handle_form_is_case_sensitive() {
        assert!(is_handle("root:in/a.stl"));
        assert!(!is_handle("ROOT:in/a.stl"));
        assert!(!is_handle(" root:in/a.stl"));
        assert!(!is_handle("C:\\data\\a.stl"));
    }

    #[test]
    fn metadata_rule() {
        let e = |xs: &[&str]| xs.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert!(root_config_by_metadata(SideEffect::A0, &[]));
        assert!(root_config_by_metadata(SideEffect::W2, &e(&["Other", ROOTS_EFFECT])));
        assert!(!root_config_by_metadata(SideEffect::W2, &e(&["Other", "rootschanged", "RootsChanged@1"])));
        for c in SideEffect::ALL.iter().filter(|c| **c != SideEffect::A0) {
            assert!(!root_config_by_metadata(*c, &[]), "{c:?}");
        }
    }

    #[test]
    fn every_leaf_error_maps_to_a_bus_pair() {
        let leaf: Vec<&str> = ERROR_MAP.iter().map(|(l, _, _)| *l).collect();
        for want in ["bad_handle", "unknown_root", "wrong_access", "root_escape", "raw_path_refused", "io_error"] {
            assert!(leaf.contains(&want), "{want}");
        }
        let errs = [
            RootError::RawPathRefused { kind: PrincipalKind::Agent, param: "p".into() },
            RootError::BadHandle { param: "p".into() },
            RootError::RootConfigHidden { kind: PrincipalKind::Script, command: "a.b@1".into() },
            RootError::NestedPathLike { command: "a.b@1".into() },
        ];
        for e in errs {
            assert!(ERROR_MAP.contains(&(e.code(), e.bus_code(), e.bus_reason())), "{e}");
            let env = e.to_envelope("a.b@1");
            assert!(env.validate().is_ok(), "{e}");
        }
    }
}
