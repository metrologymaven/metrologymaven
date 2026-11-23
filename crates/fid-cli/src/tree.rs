//! Generated CLI command tree, help renderer, and runtime resolver. Spec node `AUTO.CLI.TREE`.
//!
//! - [`emit`] builds canonical RFC 8785 `cli-tree.json` bytes from a [`Projection`].
//! - [`parse_tree`] parses a `cli-tree.json` file (tolerating the generator's `_header` member)
//!   with strict size and depth bounds.
//! - [`embedded_tree`] loads the `cli-tree.json` embedded by `build.rs`.
//! - [`resolve_args`] routes argv against the tree (newest major by default, older majors via
//!   `--major <N>`, `--help`, `--help --json`, `--version`, reserved host subcommands `ctrl` and
//!   `mcp`, and nearest-match `usage_error` suggestions within edit distance 2).
//!
//! No registry command path is hard-coded here (REQ-AUTO.CLI.TREE-002, AC-5).

use std::collections::BTreeMap;
use std::fmt;

use fid_canon::{parse_with_limits, to_canonical_bytes, CValue};
use fid_surface::{ParamSpec, Projection};
use fid_types::{CommandId, CommandName, Determinism, Digest, SideEffect};

/// Surface name owned by the CLI emitter.
pub const SURFACE: &str = "cli";
/// Emitted tree filename inside the `cli` surface directory.
pub const TREE_FILE: &str = "cli-tree.json";
/// Schema tag carried inside `cli-tree.json`.
pub const TREE_SCHEMA: &str = "cli-tree-v1";
/// Maximum accepted size of `cli-tree.json`, in bytes.
pub const MAX_TREE_BYTES: usize = 32 * 1024 * 1024;
/// Maximum JSON nesting depth accepted in `cli-tree.json`.
pub const MAX_TREE_DEPTH: usize = 32;
/// Reserved host subcommands with no registry command IDs of their own.
pub const RESERVED_HOST_SUBCOMMANDS: [&str; 2] = ["ctrl", "mcp"];
/// Maximum Levenshtein edit distance for nearest-match CLI suggestions.
pub const MAX_SUGGEST_DISTANCE: usize = 2;

const EMBEDDED_TREE_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/cli-tree.json"));

/// One parameter entry in a [`CliMajor`].
#[derive(Debug, Clone, PartialEq)]
pub struct CliParam {
    pub name: String,
    pub ty: String,
    pub quantity_kind: Option<String>,
    pub unit: Option<String>,
    pub default: Option<CValue>,
    pub required: bool,
}

impl CliParam {
    fn from_spec(spec: &ParamSpec) -> Self {
        Self {
            name: spec.name.clone(),
            ty: spec.ty.clone(),
            quantity_kind: spec.quantity_kind.clone(),
            unit: spec.unit.clone(),
            default: spec.default.clone(),
            required: spec.required,
        }
    }

    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("default".to_string(), self.default.clone().unwrap_or(CValue::Null));
        o.insert("name".to_string(), CValue::Str(self.name.clone()));
        o.insert("quantity_kind".to_string(), opt_str(&self.quantity_kind));
        o.insert("required".to_string(), CValue::Bool(self.required));
        o.insert("ty".to_string(), CValue::Str(self.ty.clone()));
        o.insert("unit".to_string(), opt_str(&self.unit));
        CValue::Obj(o)
    }

    fn from_value(v: &CValue, at: &str) -> Result<Self, CliTreeError> {
        let CValue::Obj(o) = v else {
            return Err(CliTreeError::Parse(format!("{at}: param must be an object")));
        };
        let name = req_str(o, "name", at)?;
        let ty = req_str(o, "ty", at)?;
        let quantity_kind = opt_str_val(o, "quantity_kind", at)?;
        let unit = opt_str_val(o, "unit", at)?;
        let required = req_bool(o, "required", at)?;
        let default = match o.get("default") {
            None | Some(CValue::Null) => None,
            Some(other) => Some(other.clone()),
        };
        Ok(Self { name, ty, quantity_kind, unit, default, required })
    }
}

/// One live major version of a CLI command.
#[derive(Debug, Clone, PartialEq)]
pub struct CliMajor {
    pub major: u32,
    pub command_id: CommandId,
    pub params: Vec<CliParam>,
    pub summary: String,
    pub help: String,
    pub side_effect: SideEffect,
    pub determinism: Determinism,
    pub dry_run: bool,
    pub human_only: bool,
}

impl CliMajor {
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("command_id".to_string(), CValue::Str(self.command_id.as_str().to_string()));
        o.insert("determinism".to_string(), CValue::Str(self.determinism.as_str().to_string()));
        o.insert("dry_run".to_string(), CValue::Bool(self.dry_run));
        o.insert("help".to_string(), CValue::Str(self.help.clone()));
        o.insert("human_only".to_string(), CValue::Bool(self.human_only));
        o.insert("major".to_string(), CValue::Num(f64::from(self.major)));
        o.insert("params".to_string(), CValue::Arr(self.params.iter().map(CliParam::to_value).collect()));
        o.insert("side_effect".to_string(), CValue::Str(self.side_effect.as_str().to_string()));
        o.insert("summary".to_string(), CValue::Str(self.summary.clone()));
        CValue::Obj(o)
    }

    fn from_value(v: &CValue, at: &str) -> Result<Self, CliTreeError> {
        let CValue::Obj(o) = v else {
            return Err(CliTreeError::Parse(format!("{at}: major entry must be an object")));
        };
        let major = req_u32(o, "major", at)?;
        let raw_id = req_str(o, "command_id", at)?;
        let command_id = CommandId::parse(&raw_id)
            .map_err(|_| CliTreeError::Parse(format!("{at}: invalid command_id {raw_id:?}")))?;
        if command_id.major() != major {
            return Err(CliTreeError::Parse(format!("{at}: major {major} disagrees with command_id {raw_id:?}")));
        }
        let summary = req_str(o, "summary", at)?;
        let help = req_str(o, "help", at)?;
        let se_raw = req_str(o, "side_effect", at)?;
        let side_effect = SideEffect::parse(&se_raw)
            .map_err(|_| CliTreeError::Parse(format!("{at}: invalid side_effect {se_raw:?}")))?;
        let det_raw = req_str(o, "determinism", at)?;
        let determinism = Determinism::parse(&det_raw)
            .map_err(|_| CliTreeError::Parse(format!("{at}: invalid determinism {det_raw:?}")))?;
        let dry_run = req_bool(o, "dry_run", at)?;
        let human_only = req_bool(o, "human_only", at)?;
        let Some(CValue::Arr(raw_params)) = o.get("params") else {
            return Err(CliTreeError::Parse(format!("{at}: params must be an array")));
        };
        let mut params = Vec::with_capacity(raw_params.len());
        for (i, p) in raw_params.iter().enumerate() {
            params.push(CliParam::from_value(p, &format!("{at}.params[{i}]"))?);
        }
        Ok(Self { major, command_id, params, summary, help, side_effect, determinism, dry_run, human_only })
    }
}

/// One CLI command path (`fid <domain> <verb-object>`) with all its live majors.
#[derive(Debug, Clone, PartialEq)]
pub struct CliCommand {
    /// Full CLI path `["fid", domain, verb-object]`.
    pub path: Vec<String>,
    /// Bare command name without `@major`.
    pub command_id: String,
    /// Live majors sorted ascending by major number; newest is last.
    pub majors: Vec<CliMajor>,
}

impl CliCommand {
    /// The CLI verb-object segment (`path[2]`).
    pub fn verb(&self) -> Option<&str> {
        self.path.get(2).map(String::as_str)
    }

    /// The newest (highest-numbered) major, exposed at the plain path (REQ-AUTO.CLI.TREE-003).
    pub fn default_major(&self) -> Option<&CliMajor> {
        self.majors.last()
    }

    /// Looks up a specific major version (for `--major <N>`).
    pub fn major(&self, n: u32) -> Option<&CliMajor> {
        self.majors.iter().find(|m| m.major == n)
    }

    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("command_id".to_string(), CValue::Str(self.command_id.clone()));
        o.insert("majors".to_string(), CValue::Arr(self.majors.iter().map(CliMajor::to_value).collect()));
        o.insert("path".to_string(), CValue::Arr(self.path.iter().map(|s| CValue::Str(s.clone())).collect()));
        CValue::Obj(o)
    }

    fn from_value(v: &CValue, at: &str) -> Result<Self, CliTreeError> {
        let CValue::Obj(o) = v else {
            return Err(CliTreeError::Parse(format!("{at}: command must be an object")));
        };
        let command_id = req_str(o, "command_id", at)?;
        if !CommandName::is_valid(&command_id) {
            return Err(CliTreeError::Parse(format!("{at}: invalid command_id {command_id:?}")));
        }
        let Some(CValue::Arr(raw_path)) = o.get("path") else {
            return Err(CliTreeError::Parse(format!("{at}: path must be an array")));
        };
        let mut path = Vec::with_capacity(raw_path.len());
        for item in raw_path {
            let CValue::Str(s) = item else {
                return Err(CliTreeError::Parse(format!("{at}: path items must be strings")));
            };
            path.push(s.clone());
        }
        if path.len() != 3 || path[0] != "fid" {
            return Err(CliTreeError::Parse(format!("{at}: path must have form [\"fid\", domain, verb]")));
        }
        let Some(CValue::Arr(raw_majors)) = o.get("majors") else {
            return Err(CliTreeError::Parse(format!("{at}: majors must be an array")));
        };
        if raw_majors.is_empty() {
            return Err(CliTreeError::Parse(format!("{at}: majors must not be empty")));
        }
        let mut majors = Vec::with_capacity(raw_majors.len());
        for (i, m) in raw_majors.iter().enumerate() {
            majors.push(CliMajor::from_value(m, &format!("{at}.majors[{i}]"))?);
        }
        majors.sort_by_key(|m| m.major);
        Ok(Self { path, command_id, majors })
    }
}

/// Complete generated CLI tree (`cli-tree.json`).
#[derive(Debug, Clone, PartialEq)]
pub struct CliTree {
    pub schema: String,
    pub manifest_digest: Digest,
    pub domains: BTreeMap<String, Vec<CliCommand>>,
}

impl CliTree {
    /// Looks up a command by `(domain, verb-object)`.
    pub fn find(&self, domain: &str, verb: &str) -> Option<&CliCommand> {
        self.domains.get(domain)?.iter().find(|c| c.verb() == Some(verb))
    }

    /// Canonical JSON representation.
    pub fn to_value(&self) -> CValue {
        let mut domains = BTreeMap::new();
        for (dom, cmds) in &self.domains {
            domains.insert(dom.clone(), CValue::Arr(cmds.iter().map(CliCommand::to_value).collect()));
        }
        let mut o = BTreeMap::new();
        o.insert("domains".to_string(), CValue::Obj(domains));
        o.insert("manifest_digest".to_string(), CValue::Str(self.manifest_digest.as_str().to_string()));
        o.insert("schema".to_string(), CValue::Str(self.schema.clone()));
        CValue::Obj(o)
    }

    /// Canonical JSON for `fid --help --json` (AC-1).
    pub fn help_json(&self, version: &str) -> Result<Vec<u8>, CliTreeError> {
        let mut commands = Vec::new();
        for cmds in self.domains.values() {
            for cmd in cmds {
                let Some(latest) = cmd.default_major() else {
                    continue;
                };
                let mut entry = BTreeMap::new();
                entry.insert("command_id".to_string(), CValue::Str(cmd.command_id.clone()));
                entry.insert(
                    "majors".to_string(),
                    CValue::Arr(cmd.majors.iter().map(|m| CValue::Num(f64::from(m.major))).collect()),
                );
                entry
                    .insert("path".to_string(), CValue::Arr(cmd.path.iter().map(|s| CValue::Str(s.clone())).collect()));
                entry.insert("side_effect".to_string(), CValue::Str(latest.side_effect.as_str().to_string()));
                entry.insert("summary".to_string(), CValue::Str(latest.summary.clone()));
                commands.push(CValue::Obj(entry));
            }
        }
        let mut o = BTreeMap::new();
        o.insert("commands".to_string(), CValue::Arr(commands));
        o.insert(
            "host_subcommands".to_string(),
            CValue::Arr(RESERVED_HOST_SUBCOMMANDS.iter().map(|s| CValue::Str((*s).to_string())).collect()),
        );
        o.insert("manifest_digest".to_string(), CValue::Str(self.manifest_digest.as_str().to_string()));
        o.insert("version".to_string(), CValue::Str(version.to_string()));
        let mut bytes = to_canonical_bytes(&CValue::Obj(o))
            .map_err(|e| CliTreeError::Emit(format!("canonical json failed: {}", e.code())))?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    /// Renders top-level `fid --help` text.
    pub fn render_root_help(&self, version: &str) -> String {
        let mut out = String::new();
        out.push_str(&format!("fid {version}\n"));
        out.push_str(&format!("Manifest: {}\n\n", self.manifest_digest.as_str()));
        out.push_str("USAGE:\n");
        out.push_str("    fid <domain> <command> [--major <N>] [--json] [--help]\n");
        out.push_str("    fid <host-subcommand> [args...]\n");
        out.push_str("    fid --version\n");
        out.push_str("    fid --help [--json]\n\n");
        out.push_str("DOMAINS:\n");
        for (domain, cmds) in &self.domains {
            out.push_str(&format!("    {domain:<16} {} command(s)\n", cmds.len()));
        }
        out.push_str("\nHOST SUBCOMMANDS:\n");
        for host in RESERVED_HOST_SUBCOMMANDS {
            out.push_str(&format!("    {host}\n"));
        }
        out.push_str("\nEXIT CODES (docs/cli/exit-codes.md):\n");
        for code in crate::ExitCode::ALL {
            out.push_str(&format!("    {}  {:<15} {}\n", code.code(), code.name(), code.meaning()));
        }
        out
    }

    /// Renders `fid <domain> --help` text.
    pub fn render_domain_help(&self, domain: &str) -> Option<String> {
        let cmds = self.domains.get(domain)?;
        let mut out = String::new();
        out.push_str(&format!("fid {domain}\n"));
        out.push_str(&format!("Manifest: {}\n\n", self.manifest_digest.as_str()));
        out.push_str("COMMANDS:\n");
        for cmd in cmds {
            let Some(verb) = cmd.verb() else { continue };
            let Some(latest) = cmd.default_major() else { continue };
            out.push_str(&format!("    {verb:<24} [{}] {}\n", latest.side_effect.as_str(), latest.summary));
        }
        Some(out)
    }

    /// Renders `fid <domain> <verb-object> [--major <N>] --help` text (REQ-AUTO.CLI.TREE-004, AC-2).
    pub fn render_command_help(&self, cmd: &CliCommand, major: &CliMajor) -> String {
        let mut out = String::new();
        out.push_str(&format!("{}\n", cmd.path.join(" ")));
        out.push_str(&format!("{}\n\n", major.summary));
        out.push_str(&format!("{}\n\n", major.help));
        out.push_str(&format!("Command: {}\n", major.command_id.as_str()));
        out.push_str(&format!("Manifest: {}\n", self.manifest_digest.as_str()));
        out.push_str(&format!("Side-effect class: {}\n", major.side_effect.as_str()));
        out.push_str(&format!("Determinism: {}\n", major.determinism.as_str()));
        out.push_str(&format!("Dry run: {}\n", if major.dry_run { "supported" } else { "unsupported" }));
        let majors_list: Vec<String> = cmd.majors.iter().map(|m| m.major.to_string()).collect();
        out.push_str(&format!(
            "Majors: {} (default: {})\n",
            majors_list.join(", "),
            cmd.default_major().map_or(major.major, |m| m.major)
        ));
        out.push_str("\nParameters:\n");
        if major.params.is_empty() {
            out.push_str("    (none)\n");
        } else {
            for p in &major.params {
                let mut meta = vec![p.ty.clone()];
                if let Some(qk) = &p.quantity_kind {
                    meta.push(format!("quantity={qk}"));
                }
                if let Some(u) = &p.unit {
                    meta.push(format!("unit={u}"));
                }
                if p.required {
                    meta.push("required".to_string());
                }
                if let Some(d) = &p.default {
                    if let Ok(raw) = to_canonical_bytes(d) {
                        if let Ok(s) = std::str::from_utf8(&raw) {
                            meta.push(format!("default={s}"));
                        }
                    }
                }
                out.push_str(&format!("    --{} ({})\n", p.name.replace('_', "-"), meta.join(", ")));
            }
        }
        out
    }

    /// Formats `fid --version` output (REQ-AUTO.CLI.TREE-005).
    pub fn render_version(&self, version: &str) -> String {
        format!("fid {version} (manifest {})", self.manifest_digest.as_str())
    }
}

/// Output produced by [`emit`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliEmitOutput {
    pub bytes: Vec<u8>,
    pub commands: Vec<CommandId>,
}

/// Errors emitted when building or parsing a [`CliTree`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliTreeError {
    Emit(String),
    Parse(String),
}

impl CliTreeError {
    pub fn code(&self) -> &'static str {
        match self {
            CliTreeError::Emit(_) => "cli_emit_error",
            CliTreeError::Parse(_) => "cli_tree_invalid",
        }
    }
}

impl fmt::Display for CliTreeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliTreeError::Emit(msg) | CliTreeError::Parse(msg) => write!(f, "{}: {msg}", self.code()),
        }
    }
}

impl std::error::Error for CliTreeError {}

/// Builds `cli-tree.json` canonical bytes and covered command IDs from a [`Projection`]
/// (REQ-AUTO.CLI.TREE-001).
pub fn emit(p: &Projection) -> Result<CliEmitOutput, CliTreeError> {
    let mut grouped: BTreeMap<(String, String), CliCommand> = BTreeMap::new();
    let mut covered: Vec<CommandId> = Vec::with_capacity(p.commands.len());

    for cmd in &p.commands {
        if cmd.cli_path.len() != 3 || cmd.cli_path[0] != "fid" {
            return Err(CliTreeError::Emit(format!(
                "{}: unexpected CLI path {:?}",
                cmd.command_id.as_str(),
                cmd.cli_path
            )));
        }
        let domain = cmd.cli_path[1].clone();
        let verb = cmd.cli_path[2].clone();
        if RESERVED_HOST_SUBCOMMANDS.contains(&domain.as_str()) {
            return Err(CliTreeError::Emit(format!(
                "{}: domain {domain:?} collides with a reserved host subcommand",
                cmd.command_id.as_str()
            )));
        }
        covered.push(cmd.command_id.clone());
        let entry = grouped.entry((domain, verb)).or_insert_with(|| CliCommand {
            path: cmd.cli_path.clone(),
            command_id: cmd.command_id.name().to_string(),
            majors: Vec::new(),
        });
        entry.majors.push(CliMajor {
            major: cmd.major,
            command_id: cmd.command_id.clone(),
            params: cmd.params.iter().map(CliParam::from_spec).collect(),
            summary: cmd.help.title.clone(),
            help: cmd.help.description.clone(),
            side_effect: cmd.side_effect,
            determinism: cmd.determinism,
            dry_run: cmd.dry_run,
            human_only: cmd.human_only,
        });
    }

    let mut domains: BTreeMap<String, Vec<CliCommand>> = BTreeMap::new();
    for ((domain, _verb), mut cli_cmd) in grouped {
        cli_cmd.majors.sort_by_key(|m| m.major);
        domains.entry(domain).or_default().push(cli_cmd);
    }

    let tree = CliTree { schema: TREE_SCHEMA.to_string(), manifest_digest: p.manifest_digest, domains };
    let bytes = to_canonical_bytes(&tree.to_value())
        .map_err(|e| CliTreeError::Emit(format!("canonical json failed: {}", e.code())))?;
    Ok(CliEmitOutput { bytes, commands: covered })
}

/// Parses a `cli-tree.json` byte slice into a [`CliTree`] (tolerating `_header`).
pub fn parse_tree(bytes: &[u8]) -> Result<CliTree, CliTreeError> {
    let val = parse_with_limits(bytes, MAX_TREE_BYTES, MAX_TREE_DEPTH)
        .map_err(|e| CliTreeError::Parse(format!("json parse failed: {}", e.code())))?;
    let CValue::Obj(o) = val else {
        return Err(CliTreeError::Parse("cli-tree root must be a JSON object".to_string()));
    };
    let schema = req_str(&o, "schema", "root")?;
    if schema != TREE_SCHEMA {
        return Err(CliTreeError::Parse(format!("unsupported schema {schema:?} (expected {TREE_SCHEMA:?})")));
    }
    let raw_digest = req_str(&o, "manifest_digest", "root")?;
    let manifest_digest = Digest::parse(&raw_digest)
        .map_err(|_| CliTreeError::Parse(format!("invalid manifest_digest {raw_digest:?}")))?;
    let Some(CValue::Obj(raw_domains)) = o.get("domains") else {
        return Err(CliTreeError::Parse("domains must be an object".to_string()));
    };
    let mut domains = BTreeMap::new();
    for (dom, val) in raw_domains {
        let CValue::Arr(items) = val else {
            return Err(CliTreeError::Parse(format!("domains.{dom} must be an array")));
        };
        let mut cmds = Vec::with_capacity(items.len());
        for (i, item) in items.iter().enumerate() {
            cmds.push(CliCommand::from_value(item, &format!("domains.{dom}[{i}]"))?);
        }
        domains.insert(dom.clone(), cmds);
    }
    Ok(CliTree { schema, manifest_digest, domains })
}

/// Loads the `cli-tree.json` embedded into this `fid` binary at build time (REQ-AUTO.CLI.TREE-002).
pub fn embedded_tree() -> Result<CliTree, CliTreeError> {
    parse_tree(EMBEDDED_TREE_BYTES)
}

/// Result of resolving CLI arguments against a [`CliTree`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveOutcome {
    /// `fid --version`
    Version,
    /// `fid --help` (with optional `--json`)
    RootHelp { json: bool },
    /// `fid <domain> --help`
    DomainHelp { domain: String },
    /// `fid <domain> <verb-object> [--major <N>] --help`
    CommandHelp { domain: String, verb: String, major: u32 },
    /// Reserved host subcommand (`ctrl`, `mcp`) with remaining arguments.
    HostSubcommand { name: String, rest: Vec<String> },
    /// Resolved registry command invocation (`command_id@major`) and remaining args.
    Invoke { command_id: CommandId, major: u32, json: bool, rest: Vec<String> },
    /// Usage error (exit code 2), optionally with a nearest-match suggestion.
    UsageError { message: String, suggestion: Option<String>, json: bool },
}

/// Resolves CLI argv (excluding `argv[0]`) against `tree` (REQ-AUTO.CLI.TREE-002..005).
pub fn resolve_args(tree: &CliTree, args: &[String]) -> ResolveOutcome {
    let json = args.iter().any(|a| a == "--json");
    match args.first().map(String::as_str) {
        None => return ResolveOutcome::RootHelp { json: false },
        Some("--version" | "-V") => return ResolveOutcome::Version,
        Some("--help" | "-h" | "help") => return ResolveOutcome::RootHelp { json },
        Some(host) if RESERVED_HOST_SUBCOMMANDS.contains(&host) => {
            return ResolveOutcome::HostSubcommand { name: host.to_string(), rest: args[1..].to_vec() };
        }
        Some(flag) if flag.starts_with('-') => {
            if flag == "--json" && args.len() == 2 && matches!(args[1].as_str(), "--help" | "-h") {
                return ResolveOutcome::RootHelp { json: true };
            }
            return ResolveOutcome::UsageError { message: format!("unknown option `{flag}`"), suggestion: None, json };
        }
        Some(_) => {}
    }

    let domain = &args[0];
    if !tree.domains.contains_key(domain.as_str()) {
        let mut candidates: Vec<&str> = tree.domains.keys().map(String::as_str).collect();
        candidates.extend_from_slice(&RESERVED_HOST_SUBCOMMANDS);
        let suggestion = nearest_match(domain, &candidates);
        let message = match &suggestion {
            Some(s) => format!("unknown command `{domain}`; did you mean `{s}`?"),
            None => format!("unknown command `{domain}`"),
        };
        return ResolveOutcome::UsageError { message, suggestion, json };
    }

    if args.len() == 1 || matches!(args[1].as_str(), "--help" | "-h") {
        return ResolveOutcome::DomainHelp { domain: domain.clone() };
    }

    let verb = &args[1];
    let Some(cmd) = tree.find(domain, verb) else {
        let candidates: Vec<&str> = tree
            .domains
            .get(domain.as_str())
            .map_or_else(Vec::new, |list| list.iter().filter_map(CliCommand::verb).collect());
        let suggestion = nearest_match(verb, &candidates);
        let message = match &suggestion {
            Some(s) => format!("unknown command `{domain} {verb}`; did you mean `{s}`?"),
            None => format!("unknown command `{domain} {verb}`"),
        };
        return ResolveOutcome::UsageError { message, suggestion, json };
    };

    // Parse `--major <N>` and `--help` from remaining args.
    let mut requested_major: Option<u32> = None;
    let mut want_help = false;
    let mut rest = Vec::new();
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                want_help = true;
                i += 1;
            }
            "--major" => {
                let Some(val) = args.get(i + 1) else {
                    return ResolveOutcome::UsageError {
                        message: "`--major` requires an integer argument".to_string(),
                        suggestion: None,
                        json,
                    };
                };
                match val.parse::<u32>() {
                    Ok(n) => {
                        requested_major = Some(n);
                        i += 2;
                    }
                    Err(_) => {
                        return ResolveOutcome::UsageError {
                            message: format!("invalid `--major` value `{val}`"),
                            suggestion: None,
                            json,
                        };
                    }
                }
            }
            s if s.starts_with("--major=") => {
                let val = s.trim_start_matches("--major=");
                match val.parse::<u32>() {
                    Ok(n) => {
                        requested_major = Some(n);
                        i += 1;
                    }
                    Err(_) => {
                        return ResolveOutcome::UsageError {
                            message: format!("invalid `--major` value `{val}`"),
                            suggestion: None,
                            json,
                        };
                    }
                }
            }
            _ => {
                rest.push(args[i].clone());
                i += 1;
            }
        }
    }

    let selected = match requested_major {
        Some(n) => match cmd.major(n) {
            Some(m) => m,
            None => {
                let valid: Vec<String> = cmd.majors.iter().map(|m| m.major.to_string()).collect();
                return ResolveOutcome::UsageError {
                    message: format!("`{domain} {verb}` has no major {n} (available: {})", valid.join(", ")),
                    suggestion: None,
                    json,
                };
            }
        },
        None => match cmd.default_major() {
            Some(m) => m,
            None => {
                return ResolveOutcome::UsageError {
                    message: format!("`{domain} {verb}` has no available major"),
                    suggestion: None,
                    json,
                };
            }
        },
    };

    if want_help {
        return ResolveOutcome::CommandHelp { domain: domain.clone(), verb: verb.clone(), major: selected.major };
    }

    ResolveOutcome::Invoke { command_id: selected.command_id.clone(), major: selected.major, json, rest }
}

/// Returns the nearest candidate within Levenshtein distance `<= MAX_SUGGEST_DISTANCE` (ties
/// broken deterministically by ASCII byte order).
pub fn nearest_match(input: &str, candidates: &[&str]) -> Option<String> {
    if input.len() > 128 {
        return None;
    }
    let mut best: Option<(usize, &str)> = None;
    for &cand in candidates {
        if cand.len() > 128 {
            continue;
        }
        let d = levenshtein(input, cand);
        if d <= MAX_SUGGEST_DISTANCE {
            let better = match best {
                None => true,
                Some((bd, bc)) => d < bd || (d == bd && cand.as_bytes() < bc.as_bytes()),
            };
            if better {
                best = Some((d, cand));
            }
        }
    }
    best.map(|(_, c)| c.to_string())
}

/// Exact Levenshtein edit distance on Unicode scalar values.
pub fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr: Vec<usize> = vec![0; b.len() + 1];
    for (i, &ca) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

fn opt_str(v: &Option<String>) -> CValue {
    match v {
        Some(s) => CValue::Str(s.clone()),
        None => CValue::Null,
    }
}

fn req_str(o: &BTreeMap<String, CValue>, key: &str, at: &str) -> Result<String, CliTreeError> {
    match o.get(key) {
        Some(CValue::Str(s)) => Ok(s.clone()),
        _ => Err(CliTreeError::Parse(format!("{at}: missing string field {key:?}"))),
    }
}

fn opt_str_val(o: &BTreeMap<String, CValue>, key: &str, at: &str) -> Result<Option<String>, CliTreeError> {
    match o.get(key) {
        None | Some(CValue::Null) => Ok(None),
        Some(CValue::Str(s)) => Ok(Some(s.clone())),
        Some(_) => Err(CliTreeError::Parse(format!("{at}: {key:?} must be string or null"))),
    }
}

fn req_bool(o: &BTreeMap<String, CValue>, key: &str, at: &str) -> Result<bool, CliTreeError> {
    match o.get(key) {
        Some(CValue::Bool(b)) => Ok(*b),
        _ => Err(CliTreeError::Parse(format!("{at}: missing boolean field {key:?}"))),
    }
}

fn req_u32(o: &BTreeMap<String, CValue>, key: &str, at: &str) -> Result<u32, CliTreeError> {
    match o.get(key) {
        Some(CValue::Num(n)) if n.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(n) => Ok(*n as u32),
        _ => Err(CliTreeError::Parse(format!("{at}: missing integer field {key:?}"))),
    }
}
