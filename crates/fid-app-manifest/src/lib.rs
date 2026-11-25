//! The app manifest `app.toml` v1, its validation and the start-up loader (spec node
//! `ECO.APPS.MANIFEST.SCHEMA`, REQ-ECO.APPS.MANIFEST-001..004 and 007).
//!
//! An app is the shared engine plus one declarative `app.toml`. Every app binary calls
//! [`load_manifest`] first and refuses to start when it returns findings. The repository
//! check `fid-app-manifest check --manifest <file> --catalogues <dir> [--seeds <dir>]` runs
//! the same validation from catalogue files in CI ([`cli`]).
//!
//! # Schema `app.toml` v1
//!
//! The top level holds only these keys (REQ-001); any other key is `MAN-SCHEMA <key>`. No key
//! can change a command's side-effect class or MCP projection: an app selects namespaces, it
//! never alters commands.
//!
//! | Key | Type | Required | Rule |
//! |---|---|---|---|
//! | `schema` | integer | yes | exactly `1` (`MAN-SCHEMA schema`) |
//! | `id` | string | yes | an `[[app]]` id of `apps.toml` (`MAN-APP-ID <id>`) |
//! | `status` | string | yes | `alpha`, `beta` or `stable` (`MAN-STATUS status`) |
//! | `status_scope` | string | unless `stable` | 1 to 120 characters, no control characters, saying which part of the job the release covers (D-017; `MAN-STATUS status_scope`) |
//! | `known_gaps` | array | yes (may be empty) | each `{ seed = "<seed ID>" }` or `{ text = "<gap>", node = "<spec node ID>" }`; with a seed list, an unknown seed is `MAN-GAP <seed>`; a malformed entry is `MAN-GAP known_gaps[<i>]` |
//! | `namespaces` | array of strings | yes | registry namespaces (command name before the first dot); unknown: `MAN-UNKNOWN-NS <ns>` |
//! | `forbidden_classes` | array of strings | no | side-effect classes the app must never enable; an enabled command of such a class is `MAN-FORBIDDEN-CLASS <name@major> <class>` |
//! | `panels` | array of strings | yes | panel IDs of the panel catalogue (`MAN-UNKNOWN-PANEL <id>`) |
//! | `layout` | string | yes | a layout ID of the layout catalogue (`MAN-UNKNOWN-LAYOUT <id>`) |
//! | `templates` | array of strings | no | report template IDs (`MAN-UNKNOWN-TEMPLATE <id>`) |
//! | `defaults` | table | no | setting key (quoted, e.g. `"ui.show_tips" = false`) to value; unknown key: `MAN-UNKNOWN-SETTING <key>`; a value that fails the setting's schema, has no JSON form (a TOML date, an integer beyond 2^53, a non-finite float) or targets an admin (A0) setting: `MAN-DEFAULT <key> <schema\|value\|admin>` |
//! | `branding` | table | yes | exactly `name`, `icon`, optional `colours` (array of strings) and `internal`; while D-002 is undecided `internal = true` and `name`, `icon` and every colour match `^placeholder-[a-z0-9-]+$` (`MAN-BRANDING branding.<field>`) |
//!
//! A wrong type or a repeated entry in a list is `MAN-SCHEMA <key>`; a file that is not UTF-8
//! TOML (a duplicate key included) is `MAN-SCHEMA toml`; a file over [`MAX_MANIFEST_BYTES`] or
//! an unreadable file is `MAN-IO manifest`. Names are echoed only when they are plain tokens
//! (`[A-Za-z0-9._@-]`, at most 128 bytes); anything else is reported by its key path.
//!
//! # Findings
//!
//! All findings of one manifest are reported in one pass, sorted by code, then name, then
//! detail, without repeats (REQ-009). One line each: `<code> <name>[ <detail>]`.

mod catalogue;
mod check;
pub mod cli;
mod schema;

pub use catalogue::{
    read_app_ids, read_id_list, seed_ids, Catalogues, RegistryView, APPS_FILE, LAYOUTS_FILE, PANELS_FILE,
    QUANTITIES_FILE, REGISTRY_FILE, SETTINGS_FILE, TEMPLATES_FILE,
};
pub use check::validate;
pub use schema::parse_manifest;

use fid_canon::CValue;
use fid_types::{CommandId, SideEffect};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

/// The only schema version this crate reads.
pub const SCHEMA_VERSION: u32 = 1;
/// Largest manifest read, in bytes (REQ-009).
pub const MAX_MANIFEST_BYTES: usize = 256 * 1024;
/// Longest `status_scope`, in characters (D-017).
pub const MAX_STATUS_SCOPE_CHARS: usize = 120;
/// Longest free-text gap, in characters.
pub const MAX_GAP_TEXT_CHARS: usize = 500;
/// The top-level keys of schema v1, sorted (REQ-001).
pub const TOP_KEYS: [&str; 12] = [
    "branding",
    "defaults",
    "forbidden_classes",
    "id",
    "known_gaps",
    "layout",
    "namespaces",
    "panels",
    "schema",
    "status",
    "status_scope",
    "templates",
];

/// Finding codes.
pub mod code {
    pub const SCHEMA: &str = "MAN-SCHEMA";
    pub const UNKNOWN_NS: &str = "MAN-UNKNOWN-NS";
    pub const UNKNOWN_PANEL: &str = "MAN-UNKNOWN-PANEL";
    pub const UNKNOWN_LAYOUT: &str = "MAN-UNKNOWN-LAYOUT";
    pub const UNKNOWN_TEMPLATE: &str = "MAN-UNKNOWN-TEMPLATE";
    pub const UNKNOWN_SETTING: &str = "MAN-UNKNOWN-SETTING";
    pub const DEFAULT: &str = "MAN-DEFAULT";
    pub const FORBIDDEN_CLASS: &str = "MAN-FORBIDDEN-CLASS";
    pub const APP_ID: &str = "MAN-APP-ID";
    pub const STATUS: &str = "MAN-STATUS";
    pub const GAP: &str = "MAN-GAP";
    pub const BRANDING: &str = "MAN-BRANDING";
    pub const IO: &str = "MAN-IO";
}

/// One problem: a code, the name it concerns and an optional short detail token.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Finding {
    pub code: &'static str,
    pub name: String,
    pub detail: String,
}

impl Finding {
    /// A finding; `detail` may be empty.
    pub fn new(code: &'static str, name: impl Into<String>, detail: &str) -> Finding {
        Finding { code, name: name.into(), detail: detail.to_string() }
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.code, self.name)?;
        if !self.detail.is_empty() {
            write!(f, " {}", self.detail)?;
        }
        Ok(())
    }
}

/// Sorts by (code, name, detail) and drops repeats (REQ-009).
pub fn sort_findings(mut findings: Vec<Finding>) -> Vec<Finding> {
    findings.sort();
    findings.dedup();
    findings
}

/// Declared release status (D-017).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Status {
    Alpha,
    Beta,
    Stable,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Alpha => "alpha",
            Status::Beta => "beta",
            Status::Stable => "stable",
        }
    }

    pub fn parse(s: &str) -> Option<Status> {
        match s {
            "alpha" => Some(Status::Alpha),
            "beta" => Some(Status::Beta),
            "stable" => Some(Status::Stable),
            _ => None,
        }
    }
}

/// One known gap: a seed row ID or a free-text gap with its owning spec node.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Gap {
    Seed(String),
    Text { text: String, node: String },
}

/// The branding slot (REQ-ECO.APPS.MANIFEST-007).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branding {
    pub name: String,
    pub icon: String,
    pub colours: Option<Vec<String>>,
    pub internal: bool,
}

impl Branding {
    /// True when the slot holds only internal placeholders, so release packaging can detect
    /// an unbranded build (REQ-ECO.APPS.MANIFEST-007).
    pub fn is_placeholder(&self) -> bool {
        self.internal
            && is_placeholder(&self.name)
            && is_placeholder(&self.icon)
            && self.colours.as_ref().is_none_or(|c| c.iter().all(|s| is_placeholder(s)))
    }
}

/// A parsed `app.toml` v1.
#[derive(Debug, Clone, PartialEq)]
pub struct AppManifest {
    pub schema: u32,
    pub id: String,
    pub status: Status,
    pub status_scope: Option<String>,
    pub known_gaps: Vec<Gap>,
    pub namespaces: BTreeSet<String>,
    pub forbidden_classes: BTreeSet<SideEffect>,
    pub panels: Vec<String>,
    pub layout: String,
    pub templates: Vec<String>,
    /// Setting key to value, in JSON form (as the settings validator reads it).
    pub defaults: BTreeMap<String, CValue>,
    pub branding: Branding,
}

/// A manifest that passed every rule against the catalogues in force.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedManifest {
    manifest: AppManifest,
    effective_commands: BTreeSet<CommandId>,
}

impl LoadedManifest {
    pub fn manifest(&self) -> &AppManifest {
        &self.manifest
    }

    /// Registry commands whose namespace the app enables, sorted.
    pub fn effective_commands(&self) -> &BTreeSet<CommandId> {
        &self.effective_commands
    }

    /// True when the branding slot holds placeholders only (REQ-ECO.APPS.MANIFEST-007).
    pub fn branding_is_placeholder(&self) -> bool {
        self.manifest.branding.is_placeholder()
    }
}

/// The start-up loader: reads at most [`MAX_MANIFEST_BYTES`] of `path` and validates it
/// against `catalogues`. `Err` holds every finding, sorted; the app must not start.
pub fn load_manifest(path: &Path, catalogues: &Catalogues) -> Result<LoadedManifest, Vec<Finding>> {
    let bytes = catalogue::read_bounded(path, MAX_MANIFEST_BYTES)
        .map_err(|why| vec![Finding::new(code::IO, "manifest", why)])?;
    load_manifest_bytes(&bytes, catalogues)
}

/// [`load_manifest`] on manifest bytes already in memory.
pub fn load_manifest_bytes(bytes: &[u8], catalogues: &Catalogues) -> Result<LoadedManifest, Vec<Finding>> {
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(vec![Finding::new(code::IO, "manifest", "too_large")]);
    }
    let (draft, mut findings) = schema::parse_draft(bytes);
    if let Some(d) = &draft {
        findings.extend(check::check(d, catalogues));
    }
    let findings = sort_findings(findings);
    if !findings.is_empty() {
        return Err(findings);
    }
    // Every missing required field is reported by the parser, so this only keeps the
    // function total.
    let Some(manifest) = draft.and_then(schema::Draft::into_manifest) else {
        return Err(vec![Finding::new(code::SCHEMA, "manifest", "incomplete")]);
    };
    let effective_commands = check::effective_commands(&manifest.namespaces, &catalogues.registry);
    Ok(LoadedManifest { manifest, effective_commands })
}

/// `^placeholder-[a-z0-9-]+$` (REQ-008).
pub fn is_placeholder(s: &str) -> bool {
    s.strip_prefix("placeholder-")
        .is_some_and(|r| !r.is_empty() && r.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'))
}

/// A name that may be echoed in a finding: 1 to 128 bytes of `[A-Za-z0-9._@-]`.
pub fn is_token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'@' || b == b'-')
}

/// A seed row ID such as `PW.RPT.01`: `^[A-Z][A-Z0-9]*(\.[A-Z0-9]+)+$`, at most 64 bytes.
pub fn is_seed_id(s: &str) -> bool {
    let mut parts = s.split('.');
    let first_ok = parts.next().is_some_and(|p| p.as_bytes().first().is_some_and(u8::is_ascii_uppercase) && upper(p));
    let rest: Vec<&str> = parts.collect();
    s.len() <= 64 && first_ok && !rest.is_empty() && rest.iter().all(|p| !p.is_empty() && upper(p))
}

/// A spec node ID such as `RPT.MODEL`: `^[A-Z][A-Z0-9]*(\.[A-Z0-9]+)*$`, at most 128 bytes.
pub fn is_node_id(s: &str) -> bool {
    let mut parts = s.split('.');
    let first_ok = parts.next().is_some_and(|p| p.as_bytes().first().is_some_and(u8::is_ascii_uppercase) && upper(p));
    s.len() <= 128 && first_ok && parts.all(|p| !p.is_empty() && upper(p))
}

/// A free-text gap: not blank, at most [`MAX_GAP_TEXT_CHARS`] characters, no control characters.
pub(crate) fn is_gap_text(t: &str) -> bool {
    !t.trim().is_empty() && t.chars().count() <= MAX_GAP_TEXT_CHARS && !t.chars().any(char::is_control)
}

fn upper(p: &str) -> bool {
    p.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}
