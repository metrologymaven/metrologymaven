//! The parts of a registry manifest (CORE.REG.MAN) the adversary reads: each command's name,
//! major, side-effect class, parameter schema, declared effects and `human_only` flag.
//! Every other field is ignored. Parsing is bounded and fails closed with
//! `manifest_unreadable`.

use crate::AdversaryError;
use fid_canon::CValue;
use fid_types::{CommandName, SideEffect};

/// Largest accepted manifest, in bytes.
pub const MAX_MANIFEST_BYTES: usize = 32 * 1024 * 1024;
/// Deepest accepted nesting of a manifest.
pub const MAX_MANIFEST_DEPTH: usize = 64;

/// One command definition, as far as the adversary needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    /// Command name without the major, e.g. `report.release_report`.
    pub name: String,
    pub major: u32,
    pub class: SideEffect,
    /// The declared `params_schema` (a JSON Schema object; anything else is treated as `{}`).
    pub params: CValue,
    /// Declared `effects`, string entries only.
    pub effects: Vec<String>,
    /// `human_only: true` (D-031).
    pub human_only: bool,
}

impl Command {
    /// `name@major`.
    pub fn id(&self) -> String {
        format!("{}@{}", self.name, self.major)
    }

    /// The MCP tool name by the CONSTITUTION §3 rule: drop `@major`, dots to underscores, and
    /// `_vN` for a major of 2 or more (a breaking change).
    pub fn tool_name(&self) -> String {
        let base = self.name.replace('.', "_");
        if self.major >= 2 {
            format!("{base}_v{}", self.major)
        } else {
            base
        }
    }

    /// The CLI words by the CONSTITUTION §3 rule: `fid <domain> <verb-object>`; each dotted
    /// segment becomes a word with underscores as hyphens.
    pub fn cli_words(&self) -> Vec<String> {
        self.name.split('.').map(|s| s.replace('_', "-")).collect()
    }

    /// The top-level `properties` of the parameter schema.
    pub fn properties(&self) -> Option<&std::collections::BTreeMap<String, CValue>> {
        match &self.params {
            CValue::Obj(m) => match m.get("properties") {
                Some(CValue::Obj(p)) => Some(p),
                _ => None,
            },
            _ => None,
        }
    }

    /// Whether a declared effect changes read or write roots (REQ-AUTO.SURF.ROOTS-006): an
    /// effect with a dotted segment `roots`, e.g. `settings.roots`.
    pub fn is_root_config(&self) -> bool {
        self.effects.iter().any(|e| e.split('.').any(|seg| seg == "roots"))
    }

    /// Promotion, adoption or approval: the verb (after the last dot) starts with `adopt_`,
    /// `promote` or `approve`, or the command is `human_only`.
    pub fn is_adoption(&self) -> bool {
        let verb = self.name.rsplit('.').next().unwrap_or("");
        verb.starts_with("adopt_") || verb.starts_with("promote") || verb.starts_with("approve") || self.human_only
    }
}

/// The commands of a manifest, sorted by (name, major).
#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    pub commands: Vec<Command>,
}

fn bad(detail: impl Into<String>) -> AdversaryError {
    AdversaryError::ManifestUnreadable(detail.into())
}

/// Parses manifest bytes.
pub fn parse_manifest(bytes: &[u8]) -> Result<Manifest, AdversaryError> {
    let root = fid_canon::parse_with_limits(bytes, MAX_MANIFEST_BYTES, MAX_MANIFEST_DEPTH)
        .map_err(|e| bad(format!("not JSON: {e}")))?;
    let CValue::Obj(top) = root else {
        return Err(bad("top level is not an object"));
    };
    let Some(CValue::Arr(list)) = top.get("commands") else {
        return Err(bad("missing array `commands`"));
    };
    let mut commands = Vec::with_capacity(list.len());
    for (i, c) in list.iter().enumerate() {
        commands.push(parse_command(c).map_err(|d| bad(format!("commands[{i}]: {d}")))?);
    }
    commands.sort_by(|a, b| (a.name.as_str(), a.major).cmp(&(b.name.as_str(), b.major)));
    for w in commands.windows(2) {
        if w[0].name == w[1].name && w[0].major == w[1].major {
            return Err(bad(format!("duplicate command {}", w[0].id())));
        }
    }
    Ok(Manifest { commands })
}

fn parse_command(c: &CValue) -> Result<Command, String> {
    let CValue::Obj(m) = c else {
        return Err("not an object".into());
    };
    let name = match m.get("id") {
        Some(CValue::Str(s)) if CommandName::is_valid(s) => s.clone(),
        _ => return Err("`id` is not a command name".into()),
    };
    let major = match m.get("major") {
        Some(CValue::Num(n)) if n.fract() == 0.0 && *n >= 0.0 && *n <= f64::from(u32::MAX) => *n as u32,
        _ => return Err("`major` is not an integer 0..=4294967295".into()),
    };
    let class = match m.get("side_effect") {
        Some(CValue::Str(s)) => SideEffect::parse(s).map_err(|_| format!("unknown side_effect {s:?}"))?,
        _ => return Err("missing `side_effect`".into()),
    };
    let params = m.get("params_schema").cloned().unwrap_or(CValue::Null);
    let effects = match m.get("effects") {
        None => Vec::new(),
        Some(CValue::Arr(a)) => a
            .iter()
            .filter_map(|e| match e {
                CValue::Str(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        Some(_) => return Err("`effects` is not an array".into()),
    };
    let human_only = match m.get("human_only") {
        None => false,
        Some(CValue::Bool(b)) => *b,
        Some(_) => return Err("`human_only` is not a boolean".into()),
    };
    Ok(Command { name, major, class, params, effects, human_only })
}
