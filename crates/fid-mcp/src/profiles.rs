//! MCP server partitioning, profile resolution and front-door tool sets (spec node `AUTO.MCP.SRV`).
//!
//! Each MCP server profile (`fid.mcp-profile` v1, TOML, <= 64 KiB) declares the domains it serves,
//! optional extra command IDs, and an ordered front-door tool list (<= 15 tools, always containing
//! every projectable command of the `tools` disclosure domain).
//!
//! All ordering uses [`BTreeMap`] / [`BTreeSet`] and serialises through `fid-canon` as RFC 8785
//! canonical JSON (`mcp-profiles.json`), stamped with the registry manifest digest (D0).

use crate::text::first_forbidden;
use crate::tools::{decide, SURFACE};
use fid_canon::{parse_with_limits, to_canonical_bytes, CValue};
use fid_surface::{McpProjection, Projection, SurfaceProjection};
use fid_types::{CommandId, Digest};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::File;
use std::io::Read as _;
use std::path::Path;

/// Required `format` field of a `fid.mcp-profile` TOML file.
pub const PROFILE_FORMAT: &str = "fid.mcp-profile";
/// Supported `version` field of a `fid.mcp-profile` TOML file.
pub const PROFILE_VERSION: i64 = 1;
/// Maximum byte length of a single profile file (64 KiB).
pub const MAX_PROFILE_BYTES: usize = 64 * 1024;
/// Maximum number of front-door tools listed at connection time (REQ-AUTO.MCP.SRV-003).
pub const MAX_FRONT_DOOR: usize = 15;
/// Maximum character length of a profile `title`.
pub const MAX_TITLE_CHARS: usize = 120;
/// Disclosure domain whose commands must appear in every profile's front door.
pub const DISCLOSURE_DOMAIN: &str = "tools";
/// Default profile selected when `--server` is omitted (REQ-AUTO.MCP.SRV-008).
pub const DEFAULT_PROFILE: &str = "core";
/// Output file name of the generated profile index (REQ-AUTO.MCP.SRV-006).
pub const PROFILES_FILE: &str = "mcp-profiles.json";
/// Exit code on usage / unknown-profile error before MCP handshake (REQ-AUTO.MCP.SRV-007).
pub const EXIT_USAGE: u8 = 2;
/// Schema format version of `mcp-profiles.json`.
pub const INDEX_FORMAT: u64 = 1;
/// Maximum size in bytes accepted when loading a `mcp-profiles.json` index.
pub const MAX_INDEX_BYTES: usize = 16 * 1024 * 1024;
/// Maximum JSON nesting depth accepted when loading a `mcp-profiles.json` index.
pub const MAX_INDEX_DEPTH: usize = 32;

/// The authored `core` profile TOML embedded in `fid-mcp` (Scope: `AUTO.MCP.SRV`).
pub const CORE_PROFILE_TOML: &str = include_str!("../profiles/core.toml");

const ALLOWED_KEYS: [&str; 7] = ["format", "version", "name", "title", "domains", "commands", "front_door"];

/// Parsed `fid.mcp-profile` v1 declaration before resolution against a manifest projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileSpec {
    pub name: String,
    pub title: String,
    pub domains: Vec<String>,
    pub commands: Vec<CommandId>,
    pub front_door: Vec<CommandId>,
}

/// One profile resolved against a registry manifest projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedProfile {
    pub name: String,
    pub title: String,
    /// Served command IDs in canonical sorted order; never includes M1, S1, A0 or `none`.
    pub served: BTreeSet<CommandId>,
    /// Authored front-door order (<= [`MAX_FRONT_DOOR`], subset of `served`, includes all `tools.*`).
    pub front_door: Vec<CommandId>,
}

/// Complete resolved index written as `mcp-profiles.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileIndex {
    pub manifest_digest: Digest,
    /// Ordered by profile `name` (UTF-8 byte order).
    pub profiles: Vec<ResolvedProfile>,
}

/// Emitted `mcp-profiles.json` bytes together with the resolved index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmittedProfiles {
    pub index: ProfileIndex,
    pub bytes: Vec<u8>,
}

/// Errors from parsing, validating, resolving, checking or selecting MCP profiles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileError {
    /// Profile TOML failed bounded reading, syntax parsing, or structural schema checks (`profile_parse`).
    ProfileParse { source: String, detail: String },
    /// Profile violated semantic rules: >15 front-door tools, missing `tools.*`, unserved or
    /// unprojectable command, duplicate entry or duplicate profile name (`profile_invalid`).
    ProfileInvalid { profile: String, entry: Option<String>, detail: String },
    /// A projectable command in the manifest is served by no profile (`command_unserved`).
    CommandUnserved { command: String },
    /// `--server <profile>` requested an unknown profile (`unknown_profile`).
    UnknownProfile { requested: String, known: Vec<String> },
    /// Canonical JSON encoding failed (`serialise_failed`).
    Serialise { detail: String },
}

impl ProfileError {
    /// Stable snake_case error code.
    pub fn code(&self) -> &'static str {
        match self {
            ProfileError::ProfileParse { .. } => "profile_parse",
            ProfileError::ProfileInvalid { .. } => "profile_invalid",
            ProfileError::CommandUnserved { .. } => "command_unserved",
            ProfileError::UnknownProfile { .. } => "unknown_profile",
            ProfileError::Serialise { .. } => "serialise_failed",
        }
    }

    /// Profile name named by this error, if any.
    pub fn profile(&self) -> Option<&str> {
        match self {
            ProfileError::ProfileInvalid { profile, .. } => Some(profile),
            ProfileError::UnknownProfile { requested, .. } => Some(requested),
            _ => None,
        }
    }

    /// Command or entry named by this error, if any.
    pub fn entry(&self) -> Option<&str> {
        match self {
            ProfileError::ProfileInvalid { entry, .. } => entry.as_deref(),
            ProfileError::CommandUnserved { command } => Some(command),
            _ => None,
        }
    }
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            ProfileError::ProfileParse { source, detail } => write!(f, "{code}: {source}: {detail}"),
            ProfileError::ProfileInvalid { profile, entry: Some(e), detail } => {
                write!(f, "{code}: profile `{profile}` entry `{e}`: {detail}")
            }
            ProfileError::ProfileInvalid { profile, entry: None, detail } => {
                write!(f, "{code}: profile `{profile}`: {detail}")
            }
            ProfileError::CommandUnserved { command } => {
                write!(f, "{code}: projectable command `{command}` is served by no profile")
            }
            ProfileError::UnknownProfile { requested, known } => {
                write!(f, "{code}: unknown profile `{requested}`; known profiles: {}", known.join(", "))
            }
            ProfileError::Serialise { detail } => write!(f, "{code}: {detail}"),
        }
    }
}

impl std::error::Error for ProfileError {}

// ------------------------------------------------------------------ parsing & schema validation

/// Reads and parses a `fid.mcp-profile` v1 TOML file with a strict 64 KiB read bound.
pub fn read_profile_file(path: &Path) -> Result<ProfileSpec, ProfileError> {
    let source = path.display().to_string();
    let file =
        File::open(path).map_err(|e| ProfileError::ProfileParse { source: source.clone(), detail: e.to_string() })?;
    let mut buf = Vec::new();
    let limit = (MAX_PROFILE_BYTES as u64).saturating_add(1);
    file.take(limit)
        .read_to_end(&mut buf)
        .map_err(|e| ProfileError::ProfileParse { source: source.clone(), detail: e.to_string() })?;
    parse_profile_bytes(&source, &buf)
}

/// Parses and validates a `fid.mcp-profile` v1 TOML byte slice (max [`MAX_PROFILE_BYTES`]).
///
/// Structural rules from `schemas/mcp-profile-1.json` (format, version, closed key set, name and
/// command ID patterns, title bounds) are enforced here. Semantic rules that depend on the
/// projection (and the 15-tool front-door cap so AC-2 reports `profile_invalid` naming the profile
/// and the 16th entry) are enforced in [`resolve_profiles`].
pub fn parse_profile_bytes(source: &str, bytes: &[u8]) -> Result<ProfileSpec, ProfileError> {
    let parse_err = |detail: String| ProfileError::ProfileParse { source: source.to_string(), detail };

    if bytes.len() > MAX_PROFILE_BYTES {
        return Err(parse_err(format!("file exceeds maximum size of {MAX_PROFILE_BYTES} bytes")));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| parse_err("profile is not valid UTF-8".to_string()))?;
    let table: toml::Table = text.parse().map_err(|e: toml::de::Error| parse_err(e.to_string()))?;

    for key in table.keys() {
        if !ALLOWED_KEYS.contains(&key.as_str()) {
            return Err(parse_err(format!("unexpected key `{key}` (additionalProperties: false)")));
        }
    }

    let format = require_str(&table, "format", source)?;
    if format != PROFILE_FORMAT {
        return Err(parse_err(format!("expected format `{PROFILE_FORMAT}`, got `{format}`")));
    }

    let version = match table.get("version") {
        Some(toml::Value::Integer(v)) => *v,
        Some(_) => return Err(parse_err("`version` must be an integer".to_string())),
        None => return Err(parse_err("missing required key `version`".to_string())),
    };
    if version != PROFILE_VERSION {
        return Err(parse_err(format!("expected version {PROFILE_VERSION}, got {version}")));
    }

    let name = require_str(&table, "name", source)?.to_string();
    if !is_valid_profile_name(&name) {
        return Err(parse_err(format!("invalid profile name `{name}` (must match ^[a-z][a-z0-9_]{{1,31}}$)")));
    }

    let title = require_str(&table, "title", source)?.to_string();
    if title.is_empty() || title.chars().count() > MAX_TITLE_CHARS {
        return Err(parse_err(format!("`title` must be 1..={MAX_TITLE_CHARS} characters")));
    }
    if let Some(ch) = first_forbidden(&title) {
        return Err(parse_err(format!("`title` contains forbidden character U+{:04X}", u32::from(ch))));
    }

    let domains = parse_domain_array(&table, "domains", source, &name)?;
    let commands = match table.get("commands") {
        Some(_) => parse_command_array(&table, "commands", source, &name)?,
        None => Vec::new(),
    };
    let front_door = parse_command_array(&table, "front_door", source, &name)?;

    Ok(ProfileSpec { name, title, domains, commands, front_door })
}

fn require_str<'a>(table: &'a toml::Table, key: &str, source: &str) -> Result<&'a str, ProfileError> {
    match table.get(key) {
        Some(toml::Value::String(s)) => Ok(s.as_str()),
        Some(_) => {
            Err(ProfileError::ProfileParse { source: source.to_string(), detail: format!("`{key}` must be a string") })
        }
        None => Err(ProfileError::ProfileParse {
            source: source.to_string(),
            detail: format!("missing required key `{key}`"),
        }),
    }
}

fn parse_domain_array(
    table: &toml::Table,
    key: &str,
    source: &str,
    profile: &str,
) -> Result<Vec<String>, ProfileError> {
    let arr = match table.get(key) {
        Some(toml::Value::Array(a)) => a,
        Some(_) => {
            return Err(ProfileError::ProfileParse {
                source: source.to_string(),
                detail: format!("`{key}` must be an array of strings"),
            });
        }
        None => {
            return Err(ProfileError::ProfileParse {
                source: source.to_string(),
                detail: format!("missing required key `{key}`"),
            });
        }
    };
    let mut out = Vec::with_capacity(arr.len());
    let mut seen = BTreeSet::new();
    for item in arr {
        let Some(raw) = item.as_str() else {
            return Err(ProfileError::ProfileParse {
                source: source.to_string(),
                detail: format!("every entry of `{key}` must be a string"),
            });
        };
        if !is_valid_domain(raw) {
            return Err(ProfileError::ProfileParse {
                source: source.to_string(),
                detail: format!("invalid domain `{raw}` in `{key}`"),
            });
        }
        if !seen.insert(raw.to_string()) {
            return Err(ProfileError::ProfileInvalid {
                profile: profile.to_string(),
                entry: Some(raw.to_string()),
                detail: format!("duplicate domain in `{key}`"),
            });
        }
        out.push(raw.to_string());
    }
    Ok(out)
}

fn parse_command_array(
    table: &toml::Table,
    key: &str,
    source: &str,
    profile: &str,
) -> Result<Vec<CommandId>, ProfileError> {
    let arr = match table.get(key) {
        Some(toml::Value::Array(a)) => a,
        Some(_) => {
            return Err(ProfileError::ProfileParse {
                source: source.to_string(),
                detail: format!("`{key}` must be an array of command ID strings"),
            });
        }
        None => {
            return Err(ProfileError::ProfileParse {
                source: source.to_string(),
                detail: format!("missing required key `{key}`"),
            });
        }
    };
    let mut out = Vec::with_capacity(arr.len());
    let mut seen = BTreeSet::new();
    for item in arr {
        let Some(raw) = item.as_str() else {
            return Err(ProfileError::ProfileParse {
                source: source.to_string(),
                detail: format!("every entry of `{key}` must be a string"),
            });
        };
        let id = CommandId::parse(raw).map_err(|_| ProfileError::ProfileParse {
            source: source.to_string(),
            detail: format!("invalid command ID `{raw}` in `{key}`"),
        })?;
        if !seen.insert(id.clone()) {
            return Err(ProfileError::ProfileInvalid {
                profile: profile.to_string(),
                entry: Some(id.as_str().to_string()),
                detail: format!("duplicate command ID in `{key}`"),
            });
        }
        out.push(id);
    }
    Ok(out)
}

/// Checks `^[a-z][a-z0-9_]{1,31}$` (REQ-AUTO.MCP.SRV-001).
pub fn is_valid_profile_name(s: &str) -> bool {
    let b = s.as_bytes();
    (2..=32).contains(&b.len())
        && b.first().is_some_and(u8::is_ascii_lowercase)
        && b.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}

/// Checks `^[a-z][a-z0-9_]{0,31}$`.
pub fn is_valid_domain(s: &str) -> bool {
    let b = s.as_bytes();
    (1..=32).contains(&b.len())
        && b.first().is_some_and(u8::is_ascii_lowercase)
        && b.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}

/// Returns the leading domain segment of a command ID (text before the first `.`).
pub fn domain_of(id: &CommandId) -> &str {
    let name = id.name();
    match name.split_once('.') {
        Some((dom, _)) => dom,
        None => name,
    }
}

/// True iff `sp` can be served on MCP: projection is not `none`, class is not M1/S1/A0, and `mcp`
/// is in `exposure` (matches [`crate::tools::decide`] and CONSTITUTION §3).
pub fn is_projectable_mcp(sp: &SurfaceProjection) -> bool {
    if sp.mcp_projection == McpProjection::None {
        return false;
    }
    let on_mcp = sp.exposure.iter().any(|e| e == SURFACE);
    decide(sp.side_effect, sp.mcp_projection.declared_form(), sp.human_only, on_mcp).is_some()
}

// ------------------------------------------------------------------ resolution & CI checks

/// Resolves a slice of parsed [`ProfileSpec`] declarations against `projection`.
///
/// Enforces REQ-AUTO.MCP.SRV-002, -003, -004 and returns a [`ProfileIndex`] ordered by profile
/// name and command ID. Does not run the global unserved-command check (see [`check_all_served`]
/// and [`emit_profiles`]).
pub fn resolve_profiles(projection: &Projection, specs: &[ProfileSpec]) -> Result<ProfileIndex, ProfileError> {
    let mut by_name: BTreeMap<&str, &ProfileSpec> = BTreeMap::new();
    for spec in specs {
        if by_name.insert(spec.name.as_str(), spec).is_some() {
            return Err(ProfileError::ProfileInvalid {
                profile: spec.name.clone(),
                entry: None,
                detail: "duplicate profile name".to_string(),
            });
        }
    }

    let mut projectable_set: BTreeSet<CommandId> = BTreeSet::new();
    let mut by_domain: BTreeMap<&str, Vec<CommandId>> = BTreeMap::new();
    let mut disclosure_cmds: Vec<CommandId> = Vec::new();

    for sp in &projection.commands {
        if is_projectable_mcp(sp) {
            let id = sp.command_id.clone();
            let dom = domain_of(&sp.command_id);
            by_domain.entry(dom).or_default().push(id.clone());
            if dom == DISCLOSURE_DOMAIN {
                disclosure_cmds.push(id.clone());
            }
            projectable_set.insert(id);
        }
    }
    disclosure_cmds.sort();

    let mut resolved_profiles = Vec::with_capacity(by_name.len());

    for (name, spec) in by_name {
        let mut served: BTreeSet<CommandId> = BTreeSet::new();

        for dom in &spec.domains {
            if let Some(cmds) = by_domain.get(dom.as_str()) {
                for id in cmds {
                    served.insert(id.clone());
                }
            }
        }

        for extra in &spec.commands {
            if !projectable_set.contains(extra) {
                return Err(ProfileError::ProfileInvalid {
                    profile: name.to_string(),
                    entry: Some(extra.as_str().to_string()),
                    detail: "extra command is unknown or has MCP projection `none`".to_string(),
                });
            }
            served.insert(extra.clone());
        }

        if let Some(sixteenth) = spec.front_door.get(MAX_FRONT_DOOR) {
            return Err(ProfileError::ProfileInvalid {
                profile: name.to_string(),
                entry: Some(sixteenth.as_str().to_string()),
                detail: format!("front_door exceeds maximum of {MAX_FRONT_DOOR} tools"),
            });
        }

        let mut front_door_set: BTreeSet<CommandId> = BTreeSet::new();
        for entry in &spec.front_door {
            if !projectable_set.contains(entry) {
                return Err(ProfileError::ProfileInvalid {
                    profile: name.to_string(),
                    entry: Some(entry.as_str().to_string()),
                    detail: "front_door command is unknown or has MCP projection `none`".to_string(),
                });
            }
            if !served.contains(entry) {
                return Err(ProfileError::ProfileInvalid {
                    profile: name.to_string(),
                    entry: Some(entry.as_str().to_string()),
                    detail: "front_door command is not served by this profile".to_string(),
                });
            }
            if !front_door_set.insert(entry.clone()) {
                return Err(ProfileError::ProfileInvalid {
                    profile: name.to_string(),
                    entry: Some(entry.as_str().to_string()),
                    detail: "duplicate command in front_door".to_string(),
                });
            }
        }

        for req_tool in &disclosure_cmds {
            if !front_door_set.contains(req_tool) {
                return Err(ProfileError::ProfileInvalid {
                    profile: name.to_string(),
                    entry: Some(req_tool.as_str().to_string()),
                    detail: format!("front_door must include every `{DISCLOSURE_DOMAIN}.*` disclosure command"),
                });
            }
        }

        resolved_profiles.push(ResolvedProfile {
            name: spec.name.clone(),
            title: spec.title.clone(),
            served,
            front_door: spec.front_door.clone(),
        });
    }

    Ok(ProfileIndex { manifest_digest: projection.manifest_digest, profiles: resolved_profiles })
}

/// CI completeness check (REQ-AUTO.MCP.SRV-005): fails with `command_unserved` if any projectable
/// command in `projection` is served by no profile in `index`.
pub fn check_all_served(projection: &Projection, index: &ProfileIndex) -> Result<(), ProfileError> {
    let mut covered: BTreeSet<&CommandId> = BTreeSet::new();
    for p in &index.profiles {
        for cmd in &p.served {
            covered.insert(cmd);
        }
    }
    for sp in &projection.commands {
        if is_projectable_mcp(sp) && !covered.contains(&sp.command_id) {
            return Err(ProfileError::CommandUnserved { command: sp.command_id.as_str().to_string() });
        }
    }
    Ok(())
}

/// Resolves all profiles, verifies every projectable command is served by at least one profile,
/// and encodes RFC 8785 canonical `mcp-profiles.json` bytes (REQ-AUTO.MCP.SRV-006).
pub fn emit_profiles(projection: &Projection, specs: &[ProfileSpec]) -> Result<EmittedProfiles, ProfileError> {
    let index = resolve_profiles(projection, specs)?;
    check_all_served(projection, &index)?;
    let bytes = index.to_canonical_bytes()?;
    Ok(EmittedProfiles { index, bytes })
}

// ------------------------------------------------------------------ canonical JSON & runtime lookup

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

impl ResolvedProfile {
    /// Canonical JSON value of one resolved profile.
    pub fn to_value(&self) -> CValue {
        let served = self.served.iter().map(|id| s(id.as_str())).collect();
        let front_door = self.front_door.iter().map(|id| s(id.as_str())).collect();
        obj([
            ("front_door", CValue::Arr(front_door)),
            ("name", s(&self.name)),
            ("served", CValue::Arr(served)),
            ("title", s(&self.title)),
        ])
    }
}

impl ProfileIndex {
    /// Canonical JSON value of `mcp-profiles.json`.
    pub fn to_value(&self) -> CValue {
        let profiles = self.profiles.iter().map(ResolvedProfile::to_value).collect();
        obj([
            ("format", CValue::Num(INDEX_FORMAT as f64)),
            ("manifest_digest", s(self.manifest_digest.as_str())),
            ("profiles", CValue::Arr(profiles)),
            ("surface", s(SURFACE)),
        ])
    }

    /// Serialises this index to RFC 8785 canonical JSON bytes.
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, ProfileError> {
        to_canonical_bytes(&self.to_value()).map_err(|e| ProfileError::Serialise { detail: e.code().to_string() })
    }

    /// Parses a `mcp-profiles.json` index from canonical JSON bytes (bounded).
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, ProfileError> {
        let err = |detail: String| ProfileError::ProfileParse { source: PROFILES_FILE.to_string(), detail };
        let val = parse_with_limits(bytes, MAX_INDEX_BYTES, MAX_INDEX_DEPTH)
            .map_err(|e| err(format!("invalid JSON ({})", e.code())))?;
        let CValue::Obj(root) = val else {
            return Err(err("index root must be a JSON object".to_string()));
        };

        let format_ok = matches!(root.get("format"), Some(CValue::Num(n)) if *n == INDEX_FORMAT as f64);
        if !format_ok {
            return Err(err(format!("expected `format` = {INDEX_FORMAT}")));
        }
        let surface_ok = matches!(root.get("surface"), Some(CValue::Str(sv)) if sv == SURFACE);
        if !surface_ok {
            return Err(err(format!("expected `surface` = `{SURFACE}`")));
        }
        let manifest_digest = match root.get("manifest_digest") {
            Some(CValue::Str(d)) => Digest::parse(d).map_err(|e| err(e.to_string()))?,
            _ => return Err(err("missing or invalid `manifest_digest`".to_string())),
        };
        let Some(CValue::Arr(raw_profiles)) = root.get("profiles") else {
            return Err(err("missing or invalid `profiles` array".to_string()));
        };

        let mut profiles = Vec::with_capacity(raw_profiles.len());
        let mut seen_names = BTreeSet::new();
        for item in raw_profiles {
            let CValue::Obj(po) = item else {
                return Err(err("each profile entry must be an object".to_string()));
            };
            let name = match po.get("name") {
                Some(CValue::Str(n)) if is_valid_profile_name(n) => n.clone(),
                _ => return Err(err("invalid or missing profile `name`".to_string())),
            };
            if !seen_names.insert(name.clone()) {
                return Err(err(format!("duplicate profile `{name}` in index")));
            }
            let title = match po.get("title") {
                Some(CValue::Str(t)) => t.clone(),
                _ => return Err(err(format!("missing `title` in profile `{name}`"))),
            };
            let served_vec = parse_cvalue_cmd_array(po.get("served"), "served", &name)?;
            let mut served = BTreeSet::new();
            for id in served_vec {
                served.insert(id);
            }
            let front_door = parse_cvalue_cmd_array(po.get("front_door"), "front_door", &name)?;
            profiles.push(ResolvedProfile { name, title, served, front_door });
        }
        profiles.sort_by(|a, b| a.name.cmp(&b.name));

        Ok(ProfileIndex { manifest_digest, profiles })
    }

    /// Sorted list of known profile names.
    pub fn known_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.profiles.iter().map(|p| p.name.clone()).collect();
        names.sort();
        names
    }

    /// Selects a profile by optional `--server` argument (defaults to [`DEFAULT_PROFILE`] = `"core"`).
    /// Fails with [`ProfileError::UnknownProfile`] listing all known profiles when not found.
    pub fn select(&self, requested: Option<&str>) -> Result<&ResolvedProfile, ProfileError> {
        let target = requested.unwrap_or(DEFAULT_PROFILE);
        self.profiles
            .iter()
            .find(|p| p.name == target)
            .ok_or_else(|| ProfileError::UnknownProfile { requested: target.to_string(), known: self.known_names() })
    }
}

fn parse_cvalue_cmd_array(value: Option<&CValue>, field: &str, profile: &str) -> Result<Vec<CommandId>, ProfileError> {
    let err = |detail: String| ProfileError::ProfileParse { source: PROFILES_FILE.to_string(), detail };
    let Some(CValue::Arr(items)) = value else {
        return Err(err(format!("profile `{profile}` missing `{field}` array")));
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let CValue::Str(raw) = item else {
            return Err(err(format!("profile `{profile}` `{field}` entries must be strings")));
        };
        let id =
            CommandId::parse(raw).map_err(|_| err(format!("invalid command ID `{raw}` in `{profile}.{field}`")))?;
        out.push(id);
    }
    Ok(out)
}

// ------------------------------------------------------------------ `--server` CLI argument helper

/// Parsed CLI options for `fid mcp` profile selection and mode startup.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ServerSelectionArgs {
    /// `--server <profile>` if passed; `None` selects [`DEFAULT_PROFILE`] (`"core"`).
    pub server: Option<String>,
    /// Optional `--profiles-index <path>` override (used by tests and custom packaging).
    pub profiles_index: Option<String>,
    /// Optional `--print-profile` flag to print the selected profile summary and exit 0.
    pub print_profile: bool,
    /// `--headless` mode flag (`AUTO.MCP.MODES.HEADLESS`).
    pub headless: bool,
    /// `--bridge` mode flag (`AUTO.MCP.MODES.BRIDGE`).
    pub bridge: bool,
    /// Optional `--project <root handle>` path (`AUTO.MCP.MODES.HEADLESS`).
    pub project: Option<String>,
    /// Optional `--manifest <path>` override for standalone manifest loading.
    pub manifest: Option<String>,
    /// Optional `--agent <name>` prefix for session principal binding (`AUTO.MCP.SESS`).
    pub agent: Option<String>,
    /// `--help` / `-h` flag.
    pub help: bool,
}

/// Parses `--server <profile>` and `AUTO.MCP` runtime flags from command-line arguments
/// (excluding argv[0]).
pub fn parse_server_args(args: &[String]) -> Result<ServerSelectionArgs, ProfileError> {
    let mut parsed = ServerSelectionArgs::default();

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--help" || arg == "-h" {
            parsed.help = true;
            i += 1;
        } else if arg == "--server" {
            let Some(val) = args.get(i + 1) else {
                return Err(ProfileError::ProfileParse {
                    source: "cli".to_string(),
                    detail: "--server requires a profile name".to_string(),
                });
            };
            parsed.server = Some(val.clone());
            i += 2;
        } else if let Some(val) = arg.strip_prefix("--server=") {
            if val.is_empty() {
                return Err(ProfileError::ProfileParse {
                    source: "cli".to_string(),
                    detail: "--server requires a non-empty profile name".to_string(),
                });
            }
            parsed.server = Some(val.to_string());
            i += 1;
        } else if arg == "--profiles-index" {
            let Some(val) = args.get(i + 1) else {
                return Err(ProfileError::ProfileParse {
                    source: "cli".to_string(),
                    detail: "--profiles-index requires a path".to_string(),
                });
            };
            parsed.profiles_index = Some(val.clone());
            i += 2;
        } else if let Some(val) = arg.strip_prefix("--profiles-index=") {
            if val.is_empty() {
                return Err(ProfileError::ProfileParse {
                    source: "cli".to_string(),
                    detail: "--profiles-index requires a non-empty path".to_string(),
                });
            }
            parsed.profiles_index = Some(val.to_string());
            i += 1;
        } else if arg == "--print-profile" {
            parsed.print_profile = true;
            i += 1;
        } else if arg == "--headless" {
            parsed.headless = true;
            i += 1;
        } else if arg == "--bridge" {
            parsed.bridge = true;
            i += 1;
        } else if arg == "--project" {
            let Some(val) = args.get(i + 1) else {
                return Err(ProfileError::ProfileParse {
                    source: "cli".to_string(),
                    detail: "--project requires a root handle".to_string(),
                });
            };
            parsed.project = Some(val.clone());
            i += 2;
        } else if let Some(val) = arg.strip_prefix("--project=") {
            if val.is_empty() {
                return Err(ProfileError::ProfileParse {
                    source: "cli".to_string(),
                    detail: "--project requires a non-empty root handle".to_string(),
                });
            }
            parsed.project = Some(val.to_string());
            i += 1;
        } else if arg == "--manifest" {
            let Some(val) = args.get(i + 1) else {
                return Err(ProfileError::ProfileParse {
                    source: "cli".to_string(),
                    detail: "--manifest requires a path".to_string(),
                });
            };
            parsed.manifest = Some(val.clone());
            i += 2;
        } else if let Some(val) = arg.strip_prefix("--manifest=") {
            if val.is_empty() {
                return Err(ProfileError::ProfileParse {
                    source: "cli".to_string(),
                    detail: "--manifest requires a non-empty path".to_string(),
                });
            }
            parsed.manifest = Some(val.to_string());
            i += 1;
        } else if arg == "--agent" {
            let Some(val) = args.get(i + 1) else {
                return Err(ProfileError::ProfileParse {
                    source: "cli".to_string(),
                    detail: "--agent requires a name".to_string(),
                });
            };
            parsed.agent = Some(val.clone());
            i += 2;
        } else if let Some(val) = arg.strip_prefix("--agent=") {
            if val.is_empty() {
                return Err(ProfileError::ProfileParse {
                    source: "cli".to_string(),
                    detail: "--agent requires a non-empty name".to_string(),
                });
            }
            parsed.agent = Some(val.to_string());
            i += 1;
        } else {
            return Err(ProfileError::ProfileParse {
                source: "cli".to_string(),
                detail: format!("unrecognised argument `{arg}`"),
            });
        }
    }

    if parsed.headless && parsed.bridge {
        return Err(ProfileError::ProfileParse {
            source: "cli".to_string(),
            detail: "mode_conflict: `--bridge` and `--headless` cannot be used together".to_string(),
        });
    }

    Ok(parsed)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn embedded_core_profile_parses_cleanly() {
        let spec = parse_profile_bytes("profiles/core.toml", CORE_PROFILE_TOML.as_bytes()).expect("core.toml valid");
        assert_eq!(spec.name, "core");
        assert!(spec.domains.contains(&"tools".to_string()));
        assert!(spec.front_door.len() <= MAX_FRONT_DOOR);
    }

    #[test]
    fn profile_name_validation_matches_regex() {
        assert!(is_valid_profile_name("core"));
        assert!(is_valid_profile_name("a1"));
        assert!(is_valid_profile_name("inspect_2"));
        assert!(!is_valid_profile_name("a")); // too short (< 2)
        assert!(!is_valid_profile_name("1abc")); // starts with digit
        assert!(!is_valid_profile_name("Core")); // uppercase
        assert!(!is_valid_profile_name("my-prof")); // hyphen
        assert!(!is_valid_profile_name(&"a".repeat(33))); // > 32
    }
}
