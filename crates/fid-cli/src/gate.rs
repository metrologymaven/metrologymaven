//! Pre-send policy, dry-run, and raw-path gate (spec node `AUTO.CLI.GATE`).
//!
//! Check order is fixed (REQ-AUTO.CLI.GATE-001..005):
//! 1. Side-effect class (`M1` -> approval station, `S1` -> out-of-band signing, `A0` -> GUI settings)
//! 2. `--dry-run` support check (`dry_run_unsupported`, exit 2)
//! 3. `AUTO.SURF.ROOTS` raw-path gate (`raw_path_refused`, exit 3)
//! First refusal wins; nothing reaches the bus when refused.

use crate::args::ParsedArgs;
use crate::tree::CliMajor;
use fid_canon::CValue;
use fid_surface::roots::{gate_raw_paths, may_pass_raw_path, PrincipalKind, RootError};
use fid_surface::{Envelope, ErrorCode, SurfaceProjection};
use fid_types::SideEffect;
use std::collections::BTreeMap;

/// Closed hint enum for CLI class refusals (REQ-AUTO.CLI.GATE-003, -004).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateHint {
    ApprovalStation,
    OutOfBandSigning,
    GuiAdminSettings,
}

impl GateHint {
    /// Fixed project-authored refusal guidance.
    pub const fn text(self) -> &'static str {
        match self {
            GateHint::ApprovalStation => "motion commands (M1) run only through the approval station",
            GateHint::OutOfBandSigning => "signing commands (S1) run only through the out-of-band signing path",
            GateHint::GuiAdminSettings => "admin commands (A0) run only through the GUI settings page",
        }
    }

    pub const fn code(self) -> &'static str {
        match self {
            GateHint::ApprovalStation => "approval_station_required",
            GateHint::OutOfBandSigning => "out_of_band_signing_required",
            GateHint::GuiAdminSettings => "gui_settings_required",
        }
    }
}

/// Outcome of the pre-send CLI gate.
#[derive(Debug, Clone, PartialEq)]
pub enum GateDecision {
    Proceed { dry_run: bool },
    Refuse(Envelope),
}

fn class_refusal(command_id: &str, hint: GateHint) -> Envelope {
    let mut details = BTreeMap::new();
    details.insert("hint".to_string(), CValue::Str(hint.code().to_string()));
    Envelope::failed(command_id, ErrorCode::PolicyRefused, hint.text(), Some(CValue::Obj(details)))
}

/// Evaluates class policy, `--dry-run` support, and `AUTO.SURF.ROOTS` raw-path gating before
/// contacting any bus.
pub fn evaluate_gate(
    major: &CliMajor,
    parsed: &ParsedArgs,
    principal_kind: PrincipalKind,
    surface_proj: Option<&SurfaceProjection>,
) -> GateDecision {
    let cmd_id = major.command_id.as_str();

    // 1. Class check (M1 / S1 / A0 never dispatched from CLI).
    match major.side_effect {
        SideEffect::M1 => return GateDecision::Refuse(class_refusal(cmd_id, GateHint::ApprovalStation)),
        SideEffect::S1 => return GateDecision::Refuse(class_refusal(cmd_id, GateHint::OutOfBandSigning)),
        SideEffect::A0 => return GateDecision::Refuse(class_refusal(cmd_id, GateHint::GuiAdminSettings)),
        SideEffect::R0
        | SideEffect::R1
        | SideEffect::I1
        | SideEffect::W1
        | SideEffect::W2
        | SideEffect::W3
        | SideEffect::X1
        | SideEffect::D1 => {}
    }

    // 2. Dry-run support check.
    if parsed.flags.dry_run && !major.dry_run {
        return GateDecision::Refuse(Envelope::failed(
            cmd_id,
            ErrorCode::DryRunUnsupported,
            &format!("dry_run_unsupported: `{cmd_id}` does not support `--dry-run`"),
            None,
        ));
    }

    // 3. Raw-path gate via `fid_surface::roots`.
    if let Some(sp) = surface_proj {
        if let Err(err) = gate_raw_paths(principal_kind, sp, &parsed.params) {
            return GateDecision::Refuse(err.to_envelope(cmd_id));
        }
    } else if !parsed.raw_paths.is_empty() && !may_pass_raw_path(principal_kind) {
        let param = parsed.raw_paths[0].0.clone();
        let err = RootError::RawPathRefused { kind: principal_kind, param };
        return GateDecision::Refuse(err.to_envelope(cmd_id));
    }

    GateDecision::Proceed { dry_run: parsed.flags.dry_run }
}
