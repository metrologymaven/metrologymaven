//! Progressive disclosure tools and session tool state (`AUTO.MCP.DISC`).
//!
//! Implements:
//! - `tools.search_tools@1` (R0; keyword/domain search, 50 per page, never M1/S1/A0 or unserved)
//! - `tools.describe_tool@1` (R0; full `McpTool` definition)
//! - `tools.invoke_readonly@1` (R1; dispatches R0/R1 served commands through `AUTO.MCP.PROJ` + `AUTO.SURF.ROOTS`)
//! - `tools.enable_tools@1` (R0; enables 1..=16 served commands up to 64 extra beyond front door)
//! - `tools.list_domains@1` & `tools.get_workflow@1` (R0 metadata helpers)

use crate::adapter::{call_tool_tainted, AgentSession, AuditSink, McpBus, ToolTable};
use crate::profiles::{domain_of, ResolvedProfile};
use crate::tools::{tool_for, McpError, McpTool};
use fid_canon::CValue;
use fid_surface::roots::{admit, PrincipalKind};
use fid_surface::{Envelope, ErrorCode, Projection, SurfaceProjection};
use fid_types::SideEffect;
use std::collections::{BTreeMap, BTreeSet};

/// Maximum number of tools enabled beyond a profile's front door (`REQ-AUTO.MCP.DISC-008`).
pub const MAX_ENABLED_EXTRA: usize = 64;
/// Maximum number of commands accepted in one `tools.enable_tools@1` call.
pub const MAX_ENABLE_BATCH: usize = 16;
/// Maximum search results per page (`REQ-AUTO.MCP.DISC-001`).
pub const PAGE_LIMIT: usize = 50;
/// Byte-identical refusal message for hidden (M1/S1/A0), unknown, and unserved commands
/// (`REQ-AUTO.MCP.DISC-003`).
pub const NOT_FOUND_IDENTICAL_MSG: &str = "not_found: command does not exist or is not served by this profile";

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

/// Strips principal, role, approval, and branch override claims from tool arguments
/// (`REQ-AUTO.MCP.SESS-002`).
pub fn sanitize_tool_args(args: &CValue) -> (CValue, Vec<String>) {
    let CValue::Obj(map) = args else {
        return (args.clone(), Vec::new());
    };
    let mut clean = BTreeMap::new();
    let mut warnings = Vec::new();
    for (k, v) in map {
        if is_override_key(k) {
            warnings.push(format!(
                "stripped untrusted override field `{k}`; principal and branch come only from the session"
            ));
        } else {
            clean.insert(k.clone(), v.clone());
        }
    }
    (CValue::Obj(clean), warnings)
}

fn is_override_key(k: &str) -> bool {
    let lower = k.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "principal"
            | "principal_id"
            | "principal_kind"
            | "role"
            | "approval"
            | "approval_of"
            | "branch"
            | "on_behalf_of"
            | "acting_for"
    ) || lower.ends_with("_override")
}

/// Converts an [`McpTool`] to MCP 2025-06-18 `tools/list` wire shape (`inputSchema`, `outputSchema`,
/// camelCase `annotations`).
pub fn tool_to_mcp_wire(tool: &McpTool) -> CValue {
    let mut ann = BTreeMap::new();
    ann.insert("destructiveHint".to_string(), CValue::Bool(tool.annotations.destructive_hint));
    ann.insert("readOnlyHint".to_string(), CValue::Bool(tool.annotations.read_only_hint));
    ann.insert("requestOnly".to_string(), CValue::Bool(tool.annotations.request_only));

    let mut obj = BTreeMap::new();
    obj.insert("annotations".to_string(), CValue::Obj(ann));
    obj.insert("description".to_string(), s(&tool.description));
    obj.insert("inputSchema".to_string(), tool.input_schema.clone());
    obj.insert("name".to_string(), s(&tool.name));
    obj.insert("outputSchema".to_string(), tool.output_schema.clone());
    obj.insert("title".to_string(), s(&tool.title));
    CValue::Obj(obj)
}

/// Per-session progressive disclosure state (`AUTO.MCP.DISC`).
#[derive(Debug, Clone)]
pub struct DisclosureState {
    profile: ResolvedProfile,
    served_projections: BTreeMap<String, SurfaceProjection>,
    served_tools: BTreeMap<String, McpTool>,
    by_mcp_name: BTreeMap<String, String>,
    front_door_cmds: BTreeSet<String>,
    enabled_extra: BTreeSet<String>,
}

impl DisclosureState {
    /// Builds disclosure state restricted strictly to `profile.served` projectable commands.
    pub fn new(projection: &Projection, profile: &ResolvedProfile) -> Result<Self, McpError> {
        let mut served_projections = BTreeMap::new();
        let mut served_tools = BTreeMap::new();
        let mut by_mcp_name = BTreeMap::new();

        for sp in &projection.commands {
            if !profile.served.contains(&sp.command_id) {
                continue;
            }
            if let Some(tool) = tool_for(sp)? {
                let cid = sp.command_id.as_str().to_string();
                by_mcp_name.insert(tool.name.clone(), cid.clone());
                served_tools.insert(cid.clone(), tool);
                served_projections.insert(cid, sp.clone());
            }
        }

        let front_door_cmds: BTreeSet<String> = profile
            .front_door
            .iter()
            .map(|id| id.as_str().to_string())
            .filter(|cid| served_tools.contains_key(cid))
            .collect();

        Ok(Self {
            profile: profile.clone(),
            served_projections,
            served_tools,
            by_mcp_name,
            front_door_cmds,
            enabled_extra: BTreeSet::new(),
        })
    }

    /// Returns the active [`ResolvedProfile`].
    pub fn profile(&self) -> &ResolvedProfile {
        &self.profile
    }

    /// Looks up a served [`SurfaceProjection`] by MCP tool name.
    pub fn projection_for_tool(&self, tool_name: &str) -> Option<&SurfaceProjection> {
        let cid = self.by_mcp_name.get(tool_name)?;
        self.served_projections.get(cid)
    }

    /// Number of tools currently enabled beyond `front_door`.
    pub fn enabled_extra_count(&self) -> usize {
        self.enabled_extra.len()
    }

    /// Tools currently exposed by `tools/list`: `front_door` in profile order followed by
    /// dynamically enabled tools in byte order.
    pub fn listed_tools(&self) -> Vec<&McpTool> {
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        for id in &self.profile.front_door {
            let key = id.as_str();
            if let Some(tool) = self.served_tools.get(key) {
                if seen.insert(key.to_string()) {
                    out.push(tool);
                }
            }
        }
        for key in &self.enabled_extra {
            if let Some(tool) = self.served_tools.get(key) {
                if seen.insert(key.clone()) {
                    out.push(tool);
                }
            }
        }
        out
    }

    fn is_enabled(&self, cid: &str) -> bool {
        self.front_door_cmds.contains(cid) || self.enabled_extra.contains(cid)
    }

    /// `tools.search_tools@1`: R0 search over served projectable commands.
    pub fn search_tools(&self, args: &CValue) -> Envelope {
        const CMD: &str = "tools.search_tools@1";
        let CValue::Obj(map) = args else {
            return Envelope::failed(CMD, ErrorCode::ValidationError, "arguments must be an object", None);
        };
        let keyword = match map.get("keyword") {
            None | Some(CValue::Null) => None,
            Some(CValue::Str(k)) => Some(k.to_ascii_lowercase()),
            Some(_) => return Envelope::failed(CMD, ErrorCode::ValidationError, "`keyword` must be a string", None),
        };
        let domain = match map.get("domain") {
            None | Some(CValue::Null) => None,
            Some(CValue::Str(d)) => Some(d.as_str()),
            Some(_) => return Envelope::failed(CMD, ErrorCode::ValidationError, "`domain` must be a string", None),
        };
        let offset = match map.get("cursor") {
            None | Some(CValue::Null) => 0usize,
            Some(CValue::Str(c)) => match c.strip_prefix("offset:").unwrap_or(c).parse::<usize>() {
                Ok(n) => n,
                Err(_) => return Envelope::failed(CMD, ErrorCode::ValidationError, "bad_cursor: invalid cursor", None),
            },
            Some(_) => {
                return Envelope::failed(CMD, ErrorCode::ValidationError, "bad_cursor: cursor must be string", None)
            }
        };

        let mut matched: Vec<CValue> = Vec::new();
        for (cid, tool) in &self.served_tools {
            let Some(sp) = self.served_projections.get(cid) else { continue };
            if let Some(dom) = domain {
                if domain_of(&sp.command_id) != dom {
                    continue;
                }
            }
            if let Some(kw) = &keyword {
                let hay_id = cid.to_ascii_lowercase();
                let hay_name = tool.name.to_ascii_lowercase();
                let hay_title = tool.title.to_ascii_lowercase();
                let hay_desc = tool.description.to_ascii_lowercase();
                if !hay_id.contains(kw) && !hay_name.contains(kw) && !hay_title.contains(kw) && !hay_desc.contains(kw) {
                    continue;
                }
            }
            let mut item = BTreeMap::new();
            item.insert("command".to_string(), s(cid));
            item.insert("enabled".to_string(), CValue::Bool(self.is_enabled(cid)));
            item.insert("side_effect".to_string(), s(sp.side_effect.as_str()));
            item.insert("title".to_string(), s(&tool.title));
            item.insert("tool".to_string(), s(&tool.name));
            matched.push(CValue::Obj(item));
        }

        if offset > matched.len() {
            return Envelope::failed(CMD, ErrorCode::ValidationError, "bad_cursor: cursor out of range", None);
        }
        let end = offset.saturating_add(PAGE_LIMIT).min(matched.len());
        let page: Vec<CValue> = matched[offset..end].to_vec();
        let next = if end < matched.len() { s(&format!("offset:{end}")) } else { CValue::Null };

        let mut res = BTreeMap::new();
        res.insert("items".to_string(), CValue::Arr(page));
        res.insert("next".to_string(), next);
        Envelope::computed(CMD, CValue::Obj(res))
    }

    /// `tools.describe_tool@1`: returns the full `McpTool` emitted by `AUTO.MCP.PROJ`.
    pub fn describe_tool(&self, args: &CValue) -> Envelope {
        const CMD: &str = "tools.describe_tool@1";
        let CValue::Obj(map) = args else {
            return Envelope::failed(CMD, ErrorCode::ValidationError, "arguments must be an object", None);
        };
        let Some(CValue::Str(cmd_ref)) = map.get("command") else {
            return Envelope::failed(CMD, ErrorCode::ValidationError, "`command` must be a string", None);
        };
        let resolved_key = if self.served_tools.contains_key(cmd_ref.as_str()) {
            Some(cmd_ref.as_str())
        } else {
            self.by_mcp_name.get(cmd_ref.as_str()).map(String::as_str)
        };
        let Some(tool) = resolved_key.and_then(|k| self.served_tools.get(k)) else {
            return Envelope::failed(CMD, ErrorCode::NotFound, NOT_FOUND_IDENTICAL_MSG, None);
        };
        Envelope::computed(CMD, tool.to_value())
    }

    /// `tools.enable_tools@1`: enables 1..=16 served tools up to [`MAX_ENABLED_EXTRA`] (64)
    /// beyond the front door. Returns `(Envelope, emit_list_changed)`.
    pub fn enable_tools(&mut self, args: &CValue) -> (Envelope, bool) {
        const CMD: &str = "tools.enable_tools@1";
        let CValue::Obj(map) = args else {
            return (Envelope::failed(CMD, ErrorCode::ValidationError, "arguments must be an object", None), false);
        };
        let Some(CValue::Arr(raw_cmds)) = map.get("commands") else {
            return (Envelope::failed(CMD, ErrorCode::ValidationError, "`commands` must be an array", None), false);
        };
        if raw_cmds.is_empty() || raw_cmds.len() > MAX_ENABLE_BATCH {
            return (
                Envelope::failed(
                    CMD,
                    ErrorCode::ValidationError,
                    &format!("`commands` must contain 1..={MAX_ENABLE_BATCH} entries"),
                    None,
                ),
                false,
            );
        }

        let mut target_cids = Vec::with_capacity(raw_cmds.len());
        for item in raw_cmds {
            let CValue::Str(cmd_ref) = item else {
                return (
                    Envelope::failed(
                        CMD,
                        ErrorCode::ValidationError,
                        "each entry of `commands` must be a string",
                        None,
                    ),
                    false,
                );
            };
            let resolved_key = if self.served_tools.contains_key(cmd_ref.as_str()) {
                Some(cmd_ref.clone())
            } else {
                self.by_mcp_name.get(cmd_ref.as_str()).cloned()
            };
            let Some(cid) = resolved_key else {
                return (Envelope::failed(CMD, ErrorCode::NotFound, NOT_FOUND_IDENTICAL_MSG, None), false);
            };
            target_cids.push(cid);
        }

        let mut new_additions = BTreeSet::new();
        for cid in &target_cids {
            if !self.is_enabled(cid) {
                new_additions.insert(cid.clone());
            }
        }
        if self.enabled_extra.len().saturating_add(new_additions.len()) > MAX_ENABLED_EXTRA {
            return (
                Envelope::failed(
                    CMD,
                    ErrorCode::PolicyRefused,
                    &format!("tool_cap_reached: enabling would exceed session cap of {MAX_ENABLED_EXTRA} tools"),
                    None,
                ),
                false,
            );
        }

        let changed = !new_additions.is_empty();
        for cid in new_additions {
            self.enabled_extra.insert(cid);
        }

        let mut enabled_names = Vec::with_capacity(target_cids.len());
        for cid in &target_cids {
            if let Some(t) = self.served_tools.get(cid) {
                enabled_names.push(s(&t.name));
            }
        }
        let mut res = BTreeMap::new();
        res.insert("enabled".to_string(), CValue::Arr(enabled_names));
        (Envelope::computed(CMD, CValue::Obj(res)), changed)
    }

    /// `tools.invoke_readonly@1`: dispatches R0/R1 served commands through `AUTO.SURF.ROOTS`
    /// and `AUTO.MCP.PROJ`'s tainted call adapter (`REQ-AUTO.MCP.DISC-005`, `-006`).
    pub fn invoke_readonly(
        &self,
        args: &CValue,
        session: &AgentSession,
        table: &ToolTable,
        bus: &mut dyn McpBus,
        audit: &mut dyn AuditSink,
    ) -> (Envelope, Option<String>, Vec<String>) {
        const CMD: &str = "tools.invoke_readonly@1";
        let CValue::Obj(map) = args else {
            return (
                Envelope::failed(CMD, ErrorCode::ValidationError, "arguments must be an object", None),
                None,
                Vec::new(),
            );
        };
        let Some(CValue::Str(cmd_ref)) = map.get("command") else {
            return (
                Envelope::failed(CMD, ErrorCode::ValidationError, "`command` must be a string", None),
                None,
                Vec::new(),
            );
        };
        let inner_args = map.get("args").cloned().unwrap_or_else(|| CValue::Obj(BTreeMap::new()));

        let resolved_cid = if self.served_projections.contains_key(cmd_ref.as_str()) {
            Some(cmd_ref.as_str())
        } else {
            self.by_mcp_name.get(cmd_ref.as_str()).map(String::as_str)
        };
        let Some(sp) = resolved_cid.and_then(|cid| self.served_projections.get(cid)) else {
            return (Envelope::failed(CMD, ErrorCode::NotFound, NOT_FOUND_IDENTICAL_MSG, None), None, Vec::new());
        };
        let Some(tool) = self.served_tools.get(sp.command_id.as_str()) else {
            return (Envelope::failed(CMD, ErrorCode::NotFound, NOT_FOUND_IDENTICAL_MSG, None), None, Vec::new());
        };

        if !matches!(sp.side_effect, SideEffect::R0 | SideEffect::R1) {
            return (
                Envelope::failed(
                    CMD,
                    ErrorCode::PolicyRefused,
                    &format!(
                        "policy_refused: `{}` is class {}; enable tool `{}` with `tools_enable_tools` to invoke it",
                        sp.command_id.as_str(),
                        sp.side_effect.as_str(),
                        tool.name
                    ),
                    None,
                ),
                None,
                Vec::new(),
            );
        }

        let (clean_inner, warnings) = sanitize_tool_args(&inner_args);
        if let Err(root_err) = admit(PrincipalKind::Agent, sp, &clean_inner) {
            return (root_err.to_envelope(sp.command_id.as_str()), None, warnings);
        }

        match call_tool_tainted(table, session, bus, audit, &tool.name, &clean_inner) {
            Ok(tainted) => (tainted.envelope, tainted.text, warnings),
            Err(refusal) => (
                Envelope::failed(
                    sp.command_id.as_str(),
                    refusal.envelope_code(),
                    &refusal.to_string(),
                    Some(refusal.to_value()),
                ),
                None,
                warnings,
            ),
        }
    }

    /// `tools.list_domains@1`: lists domains served by the active profile.
    pub fn list_domains(&self) -> Envelope {
        let mut domains = BTreeSet::new();
        for sp in self.served_projections.values() {
            domains.insert(domain_of(&sp.command_id).to_string());
        }
        let mut res = BTreeMap::new();
        res.insert("domains".to_string(), CValue::Arr(domains.into_iter().map(|d| s(&d)).collect()));
        res.insert("profile".to_string(), s(&self.profile.name));
        Envelope::computed("tools.list_domains@1", CValue::Obj(res))
    }

    /// `tools.get_workflow@1`: returns a concise workflow guide for a served domain.
    pub fn get_workflow(&self, args: &CValue) -> Envelope {
        const CMD: &str = "tools.get_workflow@1";
        let name = match args {
            CValue::Obj(m) => match m.get("workflow").or_else(|| m.get("name")) {
                Some(CValue::Str(w)) => w.as_str(),
                _ => "core",
            },
            _ => "core",
        };
        let mut res = BTreeMap::new();
        res.insert("workflow".to_string(), s(name));
        res.insert(
            "steps".to_string(),
            CValue::Arr(vec![
                s("1. Search available tools with tools_search_tools"),
                s("2. Inspect tool schema with tools_describe_tool"),
                s("3. Run R0/R1 tools directly via tools_invoke_readonly or enable W1/W2 tools via tools_enable_tools"),
            ]),
        );
        Envelope::computed(CMD, CValue::Obj(res))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::profiles::{parse_profile_bytes, resolve_profiles};
    use fid_canon::{digest, to_canonical_bytes};
    use fid_surface::project;

    fn make_many_manifest(count: usize) -> Vec<u8> {
        let qcd = "sha256:abababababababababababababababababababababababababababababababab";
        let mut cmds = Vec::new();
        for name in ["search_tools", "describe_tool", "invoke_readonly", "enable_tools"] {
            let mut m = BTreeMap::new();
            m.insert("algorithms".to_string(), CValue::Arr(vec![]));
            m.insert("description".to_string(), s(name));
            m.insert("determinism".to_string(), s("D0"));
            m.insert("effects".to_string(), CValue::Arr(vec![]));
            m.insert("exposure".to_string(), CValue::Arr(vec![s("cli"), s("mcp")]));
            m.insert("fs_effect".to_string(), s("none"));
            m.insert("human_only".to_string(), CValue::Bool(false));
            m.insert("id".to_string(), s(&format!("tools.{name}")));
            m.insert("idempotency".to_string(), s("natural"));
            m.insert("lifecycle".to_string(), CValue::Obj(BTreeMap::from([("state".to_string(), s("active"))])));
            m.insert("major".to_string(), CValue::Num(1.0));
            m.insert("mcp_projection".to_string(), s("exposed"));
            m.insert("params_schema".to_string(), CValue::Obj(BTreeMap::from([("type".to_string(), s("object"))])));
            m.insert("preconditions".to_string(), CValue::Arr(vec![]));
            m.insert("result_schema".to_string(), CValue::Obj(BTreeMap::from([("type".to_string(), s("object"))])));
            m.insert("side_effect".to_string(), s("R0"));
            m.insert("title".to_string(), s(name));
            m.insert("undo".to_string(), s("not_undoable"));
            m.insert("web".to_string(), s("available"));
            let dig = digest(&CValue::Obj(m.clone())).unwrap().to_string();
            m.insert("definition_digest".to_string(), s(&dig));
            cmds.push((format!("tools.{name}"), CValue::Obj(m)));
        }
        for i in 1..=count {
            let id = format!("demo.cmd_{i:03}");
            let mut m = BTreeMap::new();
            m.insert("algorithms".to_string(), CValue::Arr(vec![]));
            m.insert("description".to_string(), s(&format!("Demo {i:03}")));
            m.insert("determinism".to_string(), s("D0"));
            m.insert("effects".to_string(), CValue::Arr(vec![]));
            m.insert("exposure".to_string(), CValue::Arr(vec![s("cli"), s("mcp")]));
            m.insert("fs_effect".to_string(), s("none"));
            m.insert("human_only".to_string(), CValue::Bool(false));
            m.insert("id".to_string(), s(&id));
            m.insert("idempotency".to_string(), s("natural"));
            m.insert("lifecycle".to_string(), CValue::Obj(BTreeMap::from([("state".to_string(), s("active"))])));
            m.insert("major".to_string(), CValue::Num(1.0));
            m.insert("mcp_projection".to_string(), s("exposed"));
            m.insert("params_schema".to_string(), CValue::Obj(BTreeMap::from([("type".to_string(), s("object"))])));
            m.insert("preconditions".to_string(), CValue::Arr(vec![]));
            m.insert("result_schema".to_string(), CValue::Obj(BTreeMap::from([("type".to_string(), s("object"))])));
            m.insert("side_effect".to_string(), s("R0"));
            m.insert("title".to_string(), s(&format!("Demo {i:03}")));
            m.insert("undo".to_string(), s("not_undoable"));
            m.insert("web".to_string(), s("available"));
            let dig = digest(&CValue::Obj(m.clone())).unwrap().to_string();
            m.insert("definition_digest".to_string(), s(&dig));
            cmds.push((id, CValue::Obj(m)));
        }
        cmds.sort_by(|a, b| a.0.cmp(&b.0));
        let root = CValue::Obj(BTreeMap::from([
            ("commands".to_string(), CValue::Arr(cmds.into_iter().map(|(_, v)| v).collect())),
            ("format".to_string(), CValue::Num(1.0)),
            ("quantity_catalogue_digest".to_string(), s(qcd)),
        ]));
        to_canonical_bytes(&root).unwrap()
    }

    #[test]
    fn enforces_64_tool_cap_and_50_per_page_pagination() {
        let bytes = make_many_manifest(70);
        let proj = project(&bytes).unwrap();
        let toml = r#"
format = "fid.mcp-profile"
version = 1
name = "core"
title = "Core"
domains = ["tools", "demo"]
front_door = [
  "tools.search_tools@1",
  "tools.describe_tool@1",
  "tools.invoke_readonly@1",
  "tools.enable_tools@1",
]
"#;
        let spec = parse_profile_bytes("core.toml", toml.as_bytes()).unwrap();
        let idx = resolve_profiles(&proj, &[spec]).unwrap();
        let profile = idx.select(Some("core")).unwrap();
        let mut state = DisclosureState::new(&proj, profile).unwrap();

        // Search page 1 -> 50 items + next cursor.
        let p1 = state.search_tools(&CValue::Obj(BTreeMap::from([("domain".to_string(), s("demo"))])));
        let CValue::Obj(r1) = p1.result.unwrap() else { panic!("p1") };
        let CValue::Arr(items1) = &r1["items"] else { panic!("items1") };
        assert_eq!(items1.len(), 50);
        let CValue::Str(cursor) = &r1["next"] else { panic!("cursor") };

        let p2 = state.search_tools(&CValue::Obj(BTreeMap::from([
            ("domain".to_string(), s("demo")),
            ("cursor".to_string(), s(cursor)),
        ])));
        let CValue::Obj(r2) = p2.result.unwrap() else { panic!("p2") };
        let CValue::Arr(items2) = &r2["items"] else { panic!("items2") };
        assert_eq!(items2.len(), 20);
        assert_eq!(r2.get("next"), Some(&CValue::Null));

        // Enable 64 tools in batches of 16.
        for batch in 0..4 {
            let cmds: Vec<CValue> = (1..=16).map(|j| s(&format!("demo.cmd_{:03}@1", batch * 16 + j))).collect();
            let (env, changed) =
                state.enable_tools(&CValue::Obj(BTreeMap::from([("commands".to_string(), CValue::Arr(cmds))])));
            assert_eq!(env.outcome, fid_surface::Outcome::Computed);
            assert!(changed);
        }
        assert_eq!(state.enabled_extra_count(), 64);

        // 65th tool fails atomically with tool_cap_reached and emit_list_changed = false.
        let (cap_env, cap_changed) = state.enable_tools(&CValue::Obj(BTreeMap::from([(
            "commands".to_string(),
            CValue::Arr(vec![s("demo.cmd_065@1")]),
        )])));
        assert!(!cap_changed);
        assert_eq!(cap_env.outcome, fid_surface::Outcome::Failed);
        assert!(cap_env.error.unwrap().message.contains("tool_cap_reached"));
        assert_eq!(state.enabled_extra_count(), 64);
    }
}
