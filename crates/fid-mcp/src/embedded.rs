//! Embedded default manifest, profile index, fail-closed standalone bus, and
//! [`fid_bus_api::InProcessBus`] adapter for `fid-mcp`.

use crate::adapter::{BusCall, McpBus};
use fid_bus_api::{BusApiError, BusEnvelope, ChannelKind, CommandBus, CommandCall, InProcessBus, PrincipalInfo};
use fid_canon::{digest, to_canonical_bytes, CValue};
use fid_surface::{project, Envelope, ErrorCode, Outcome, Projection};

use crate::profiles::{parse_profile_bytes, resolve_profiles, ProfileError, ProfileIndex, CORE_PROFILE_TOML};

const QCD: &str = "sha256:abababababababababababababababababababababababababababababababab";

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

fn arr(items: Vec<CValue>) -> CValue {
    CValue::Arr(items)
}

fn obj(pairs: Vec<(&str, CValue)>) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

#[allow(clippy::too_many_arguments)]
fn make_def(
    id: &str,
    major: u32,
    side_effect: &str,
    mcp_projection: &str,
    human_only: bool,
    title: &str,
    description: &str,
    params_schema: CValue,
) -> CValue {
    let mut base = obj(vec![
        ("algorithms", arr(vec![])),
        ("description", s(description)),
        ("determinism", s("D0")),
        ("effects", arr(vec![])),
        ("exposure", arr(vec![s("cli"), s("mcp"), s("python")])),
        ("fs_effect", s("none")),
        ("human_only", CValue::Bool(human_only)),
        ("id", s(id)),
        ("idempotency", s("natural")),
        ("lifecycle", obj(vec![("state", s("active"))])),
        ("major", CValue::Num(f64::from(major))),
        ("mcp_projection", s(mcp_projection)),
        ("params_schema", params_schema),
        ("preconditions", arr(vec![])),
        ("result_schema", obj(vec![("type", s("object"))])),
        ("side_effect", s(side_effect)),
        ("title", s(title)),
        ("undo", s("not_undoable")),
        ("web", s("available")),
    ]);
    let def_digest = digest(&base).map(|d| d.to_string()).unwrap_or_default();
    if let CValue::Obj(ref mut m) = base {
        m.insert("definition_digest".to_string(), s(&def_digest));
    }
    base
}

/// Builds the canonical embedded manifest JSON bytes (`tools.*` disclosure commands + pure-Rust
/// `core` inspection/demo commands + M1/S1/A0 exclusion fixtures) so standalone `fid-mcp --headless`
/// projects cleanly with real digests.
pub fn embedded_manifest_bytes() -> Vec<u8> {
    let mut cmds = vec![
        make_def(
            "align.adopt_alignment",
            1,
            "W2",
            "exposed",
            true,
            "Adopt alignment (human-only)",
            "Adopts a candidate alignment onto the main branch.",
            obj(vec![("properties", obj(vec![])), ("type", s("object"))]),
        ),
        make_def(
            "align.run_bestfit",
            1,
            "R1",
            "exposed",
            false,
            "Run best-fit alignment",
            "Computes a deterministic rigid-body best-fit alignment residual summary.",
            obj(vec![
                (
                    "properties",
                    obj(vec![(
                        "tolerance",
                        obj(vec![("type", s("number")), ("x-quantity", s("length")), ("x-unit", s("um"))]),
                    )]),
                ),
                ("type", s("object")),
            ]),
        ),
        make_def(
            "align.run_bestfit",
            2,
            "R1",
            "exposed",
            false,
            "Run best-fit alignment (v2)",
            "Computes a deterministic rigid-body best-fit alignment residual summary (v2).",
            obj(vec![("properties", obj(vec![])), ("type", s("object"))]),
        ),
        make_def(
            "align.set_datum",
            1,
            "W2",
            "exposed",
            false,
            "Set datum offset",
            "Sets an alignment datum offset on the session branch.",
            obj(vec![
                (
                    "properties",
                    obj(vec![
                        ("offset", obj(vec![("type", s("number")), ("x-quantity", s("length")), ("x-unit", s("mm"))])),
                        ("source", obj(vec![("path_like", CValue::Bool(true)), ("type", s("string"))])),
                    ]),
                ),
                ("required", arr(vec![s("source"), s("offset")])),
                ("type", s("object")),
            ]),
        ),
        make_def(
            "canon.canonicalize",
            1,
            "R0",
            "exposed",
            false,
            "Canonicalize JSON",
            "Canonicalize a JSON value and return RFC 8785 canonical JSON and its SHA-256 digest.",
            obj(vec![
                ("properties", obj(vec![("input", obj(vec![]))])),
                ("required", arr(vec![s("input")])),
                ("type", s("object")),
            ]),
        ),
        make_def(
            "egress.send",
            1,
            "X1",
            "request_only",
            false,
            "Send external egress payload",
            "Sends an approved external egress payload (X1, request-only on MCP).",
            obj(vec![
                (
                    "properties",
                    obj(vec![(
                        "tolerance",
                        obj(vec![("type", s("number")), ("x-quantity", s("length")), ("x-unit", s("um"))]),
                    )]),
                ),
                ("type", s("object")),
            ]),
        ),
        make_def(
            "journal.list_entries",
            1,
            "R0",
            "exposed",
            false,
            "List journal entries",
            "Lists journal entry headers on the active branch.",
            obj(vec![("properties", obj(vec![])), ("type", s("object"))]),
        ),
        make_def(
            "motion.move_axis",
            1,
            "M1",
            "never",
            false,
            "Move CMM axis",
            "Physical motion command (M1, never exposed on MCP).",
            obj(vec![("properties", obj(vec![])), ("type", s("object"))]),
        ),
        make_def(
            "plugin.install_pack",
            1,
            "A0",
            "never",
            false,
            "Install admin pack",
            "Administrative pack install (A0, never exposed on MCP).",
            obj(vec![("properties", obj(vec![])), ("type", s("object"))]),
        ),
        make_def(
            "probe.read_temperature",
            1,
            "D1",
            "request_only",
            false,
            "Read probe temperature",
            "Reads hardware probe temperature sensor (D1, request-only on MCP).",
            obj(vec![
                (
                    "properties",
                    obj(vec![(
                        "tolerance",
                        obj(vec![("type", s("number")), ("x-quantity", s("length")), ("x-unit", s("um"))]),
                    )]),
                ),
                ("type", s("object")),
            ]),
        ),
        make_def(
            "program.save_draft",
            1,
            "W1",
            "exposed",
            false,
            "Save program draft",
            "Saves an inspection program draft to the agent branch.",
            obj(vec![("properties", obj(vec![("name", obj(vec![("type", s("string"))]))])), ("type", s("object"))]),
        ),
        make_def(
            "project.list_features",
            1,
            "R0",
            "exposed",
            false,
            "List project features",
            "Lists geometric features in the active project.",
            obj(vec![("properties", obj(vec![])), ("type", s("object"))]),
        ),
        make_def(
            "rpt.release_report",
            1,
            "W3",
            "request_only",
            false,
            "Release inspection report",
            "Releases a controlled inspection report (W3, request-only on MCP).",
            obj(vec![
                (
                    "properties",
                    obj(vec![(
                        "tolerance",
                        obj(vec![("type", s("number")), ("x-quantity", s("length")), ("x-unit", s("um"))]),
                    )]),
                ),
                ("type", s("object")),
            ]),
        ),
        make_def(
            "sim.run_program",
            1,
            "I1",
            "exposed",
            false,
            "Run simulation program",
            "Runs an offline inspection program simulation.",
            obj(vec![
                (
                    "properties",
                    obj(vec![(
                        "tolerance",
                        obj(vec![("type", s("number")), ("x-quantity", s("length")), ("x-unit", s("um"))]),
                    )]),
                ),
                ("type", s("object")),
            ]),
        ),
        make_def(
            "tools.describe_tool",
            1,
            "R0",
            "exposed",
            false,
            "Describe MCP tool",
            "Return the full MCP tool definition for a served command.",
            obj(vec![
                ("properties", obj(vec![("command", obj(vec![("type", s("string"))]))])),
                ("required", arr(vec![s("command")])),
                ("type", s("object")),
            ]),
        ),
        make_def(
            "tools.enable_tools",
            1,
            "R0",
            "exposed",
            false,
            "Enable MCP tools",
            "Enable 1..=16 served commands in the current MCP session.",
            obj(vec![
                (
                    "properties",
                    obj(vec![(
                        "commands",
                        obj(vec![("items", obj(vec![("type", s("string"))])), ("type", s("array"))]),
                    )]),
                ),
                ("required", arr(vec![s("commands")])),
                ("type", s("object")),
            ]),
        ),
        make_def(
            "tools.get_workflow",
            1,
            "R0",
            "exposed",
            false,
            "Get workflow guide",
            "Fetch a named metrology workflow guide.",
            obj(vec![("properties", obj(vec![("workflow", obj(vec![("type", s("string"))]))])), ("type", s("object"))]),
        ),
        make_def(
            "tools.invoke_readonly",
            1,
            "R1",
            "exposed",
            false,
            "Invoke read-only tool",
            "Invoke a served R0 or R1 command without enabling it permanently.",
            obj(vec![
                (
                    "properties",
                    obj(vec![
                        ("args", obj(vec![("type", s("object"))])),
                        ("command", obj(vec![("type", s("string"))])),
                    ]),
                ),
                ("required", arr(vec![s("command")])),
                ("type", s("object")),
            ]),
        ),
        make_def(
            "tools.list_domains",
            1,
            "R0",
            "exposed",
            false,
            "List served domains",
            "List command domains served by the active MCP profile.",
            obj(vec![("properties", obj(vec![])), ("type", s("object"))]),
        ),
        make_def(
            "tools.search_tools",
            1,
            "R0",
            "exposed",
            false,
            "Search MCP tools",
            "Search served MCP tools by keyword or domain.",
            obj(vec![
                (
                    "properties",
                    obj(vec![
                        ("cursor", obj(vec![("type", s("string"))])),
                        ("domain", obj(vec![("type", s("string"))])),
                        ("keyword", obj(vec![("type", s("string"))])),
                    ]),
                ),
                ("type", s("object")),
            ]),
        ),
        make_def(
            "trust.sign_record",
            1,
            "S1",
            "never",
            false,
            "Sign record",
            "Out-of-band signing ceremony (S1, never exposed on MCP).",
            obj(vec![("properties", obj(vec![])), ("type", s("object"))]),
        ),
        make_def(
            "units.list_units",
            1,
            "R0",
            "exposed",
            false,
            "List supported units",
            "Lists canonical SI and engineering quantity units.",
            obj(vec![("properties", obj(vec![])), ("type", s("object"))]),
        ),
        make_def(
            "view.get_status",
            1,
            "R0",
            "exposed",
            false,
            "Get view status",
            "Returns the current headless session view status.",
            obj(vec![("properties", obj(vec![])), ("type", s("object"))]),
        ),
    ];

    cmds.sort_by(|a, b| {
        let key = |v: &CValue| match v {
            CValue::Obj(m) => {
                let id = match m.get("id") {
                    Some(CValue::Str(s)) => s.clone(),
                    _ => String::new(),
                };
                let maj = match m.get("major") {
                    Some(CValue::Num(n)) => *n as u32,
                    _ => 0,
                };
                (id, maj)
            }
            _ => (String::new(), 0),
        };
        key(a).cmp(&key(b))
    });

    let root = obj(vec![("commands", arr(cmds)), ("format", CValue::Num(1.0)), ("quantity_catalogue_digest", s(QCD))]);
    to_canonical_bytes(&root).unwrap_or_default()
}

/// Builds the default [`Projection`] from [`embedded_manifest_bytes`].
pub fn embedded_projection() -> Result<Projection, fid_surface::ProjError> {
    project(&embedded_manifest_bytes())
}

/// Builds the default [`ProfileIndex`] from [`CORE_PROFILE_TOML`] and `projection`.
pub fn embedded_profile_index(projection: &Projection) -> Result<ProfileIndex, ProfileError> {
    let core = parse_profile_bytes("profiles/core.toml", CORE_PROFILE_TOML.as_bytes())?;
    resolve_profiles(projection, &[core])
}

/// Fail-closed pure-Rust [`McpBus`] for standalone `fid-mcp` when no attached bus host or
/// `--bridge` desktop app is present.
///
/// Executes stateless pure-Rust `canon.canonicalize@1` (R0) via `fid_canon` and fails closed with
/// `precondition_failed` (`no_bus_host`) on all project/engine commands.
#[derive(Debug, Default, Clone)]
pub struct StandaloneMcpBus {
    project_ref: Option<String>,
}

impl StandaloneMcpBus {
    pub fn new(project_ref: Option<String>) -> Self {
        Self { project_ref }
    }

    pub fn project_ref(&self) -> Option<&str> {
        self.project_ref.as_deref()
    }
}

fn invoke_canon_canonicalize(full: &str, params: &CValue) -> Envelope {
    let CValue::Obj(map) = params else {
        return Envelope::failed(full, ErrorCode::ValidationError, "arguments must be an object", None);
    };
    let Some(target_val) = map.get("input") else {
        return Envelope::failed(full, ErrorCode::ValidationError, "missing required parameter `input`", None);
    };
    let bytes = match to_canonical_bytes(target_val) {
        Ok(b) => b,
        Err(e) => {
            return Envelope::failed(full, ErrorCode::ValidationError, &format!("canon_error: {e}"), None);
        }
    };
    let dig = match digest(target_val) {
        Ok(d) => d,
        Err(e) => {
            return Envelope::failed(full, ErrorCode::ValidationError, &format!("canon_error: {e}"), None);
        }
    };
    let canonical_text = match String::from_utf8(bytes) {
        Ok(t) => t,
        Err(e) => {
            return Envelope::failed(full, ErrorCode::InternalError, &format!("canon_utf8: {e}"), None);
        }
    };
    Envelope::computed(
        full,
        obj(vec![
            ("canonical", target_val.clone()),
            ("canonical_text", s(&canonical_text)),
            ("digest", s(&dig.to_string())),
        ]),
    )
}

impl McpBus for StandaloneMcpBus {
    fn invoke(&mut self, call: &BusCall<'_>) -> Envelope {
        let full = call.command.as_str();
        if call.command.name() == "canon.canonicalize" && call.command.major() == 1 {
            return invoke_canon_canonicalize(full, call.params);
        }
        let err = BusApiError::precondition_failed(format!(
            "no_bus_host: standalone `fid-mcp` cannot execute `{full}` without an attached bus host or `--bridge` desktop app"
        ));
        bus_to_surface_envelope(&err.to_envelope(full))
    }
}

/// Pure-Rust [`McpBus`] adapter backed by a caller-supplied [`fid_bus_api::InProcessBus`] with no
/// pre-registered dummy handlers.
pub struct InProcessMcpBus {
    bus: InProcessBus,
    project_ref: Option<String>,
}

impl InProcessMcpBus {
    /// Wraps an [`InProcessBus`] for embedded or test-hosted sessions.
    pub fn new(bus: InProcessBus, project_ref: Option<String>) -> Self {
        Self { bus, project_ref }
    }

    pub fn inner_mut(&mut self) -> &mut InProcessBus {
        &mut self.bus
    }
}

impl McpBus for InProcessMcpBus {
    fn invoke(&mut self, call: &BusCall<'_>) -> Envelope {
        let api_call = CommandCall::new(
            call.command.name(),
            call.command.major(),
            call.params.clone(),
            PrincipalInfo::new(fid_bus_api::PrincipalKind::Agent, call.session.label(), None),
        )
        .with_channel(ChannelKind::Mcp)
        .with_target(fid_bus_api::TargetHint::Headless {
            project: self.project_ref.clone(),
            branch: Some(format!("agent/{}", call.session.label())),
        });
        bus_to_surface_envelope(&self.bus.dispatch(&api_call))
    }
}

/// Converts a [`BusEnvelope`] from `fid-bus-api` into a surface [`Envelope`].
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
