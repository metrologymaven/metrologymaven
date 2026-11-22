//! Built-in starter report templates (spec node `RPT.MODEL.TMPL.STARTER`).
//!
//! Ships three unlocked version-1 canonical-JSON templates embedded directly into `fid-report`:
//!
//! - [`CHAR_TABLE_NAME`] (`tmpl:char-table`): one row per characteristic (ID, nominal, limits,
//!   measured, deviation, decision), sorted ascending by characteristic ID.
//! - [`DEVIATION_SUMMARY_NAME`] (`tmpl:deviation-summary`): outcome counts table followed by
//!   nonconforming rows only.
//! - [`MULTISTAGE_TABLE_NAME`] (`tmpl:multistage-table`): stage column slot plus one row per
//!   characteristic across stages.
//!
//! Every user-visible label in these templates is a catalog key (`rpt.starter.*`) paired with
//! English source text in [`STARTER_LABELS`] (and `templates/starter/labels.en.json`).

use crate::doc::TemplateRef;
use crate::tmpl::format::{parse_template, Template, TemplateError, TemplateId};
use crate::tmpl::slot::validate_template;
use crate::tmpl::ver::{BuiltinRegistry, VerError};
use fid_project::Uuid;

/// Built-in starter version number (version 1, unlocked).
pub const STARTER_VERSION: u32 = 1;

/// Logical starter name for the characteristic table template.
pub const CHAR_TABLE_NAME: &str = "tmpl:char-table";
/// Logical starter name for the deviation summary template.
pub const DEVIATION_SUMMARY_NAME: &str = "tmpl:deviation-summary";
/// Logical starter name for the multi-stage comparison table template.
pub const MULTISTAGE_TABLE_NAME: &str = "tmpl:multistage-table";

/// Stable UUIDv7 template ID for `tmpl:char-table`.
pub const CHAR_TABLE_ID: &str = "019a9f00-0001-7000-8000-000000000001";
/// Stable UUIDv7 template ID for `tmpl:deviation-summary`.
pub const DEVIATION_SUMMARY_ID: &str = "019a9f00-0002-7000-8000-000000000001";
/// Stable UUIDv7 template ID for `tmpl:multistage-table`.
pub const MULTISTAGE_TABLE_ID: &str = "019a9f00-0003-7000-8000-000000000001";

/// Embedded canonical JSON bytes for `tmpl:char-table`.
pub const CHAR_TABLE_JSON: &[u8] = include_bytes!("../../templates/starter/char-table.json");
/// Embedded canonical JSON bytes for `tmpl:deviation-summary`.
pub const DEVIATION_SUMMARY_JSON: &[u8] = include_bytes!("../../templates/starter/deviation-summary.json");
/// Embedded canonical JSON bytes for `tmpl:multistage-table`.
pub const MULTISTAGE_TABLE_JSON: &[u8] = include_bytes!("../../templates/starter/multistage-table.json");

/// Metadata for one built-in starter template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StarterDescriptor {
    /// Starter logical name (e.g. `tmpl:char-table`).
    pub name: &'static str,
    /// Stable UUIDv7 template ID string.
    pub id: &'static str,
    /// Catalog key for the template title.
    pub title_key: &'static str,
    /// Embedded RFC 8785 canonical JSON document bytes.
    pub canonical_json: &'static [u8],
}

/// The three built-in unlocked starter templates (REQ-RPT.MODEL.TMPL.STARTER-001).
pub const STARTERS: [StarterDescriptor; 3] = [
    StarterDescriptor {
        name: CHAR_TABLE_NAME,
        id: CHAR_TABLE_ID,
        title_key: "rpt.starter.char_table.title",
        canonical_json: CHAR_TABLE_JSON,
    },
    StarterDescriptor {
        name: DEVIATION_SUMMARY_NAME,
        id: DEVIATION_SUMMARY_ID,
        title_key: "rpt.starter.deviation_summary.title",
        canonical_json: DEVIATION_SUMMARY_JSON,
    },
    StarterDescriptor {
        name: MULTISTAGE_TABLE_NAME,
        id: MULTISTAGE_TABLE_ID,
        title_key: "rpt.starter.multistage_table.title",
        canonical_json: MULTISTAGE_TABLE_JSON,
    },
];

/// English source text for every catalog key used in the three starter templates, sorted by key
/// (REQ-RPT.MODEL.TMPL.STARTER-002).
pub const STARTER_LABELS: &[(&str, &str)] = &[
    ("rpt.starter.char_table.heading", "Characteristic Table"),
    ("rpt.starter.char_table.title", "Characteristic Table"),
    ("rpt.starter.col.count", "Count"),
    ("rpt.starter.col.decision", "Decision"),
    ("rpt.starter.col.deviation", "Deviation"),
    ("rpt.starter.col.id", "Characteristic ID"),
    ("rpt.starter.col.limits", "Limits"),
    ("rpt.starter.col.measured", "Measured"),
    ("rpt.starter.col.nominal", "Nominal"),
    ("rpt.starter.col.outcome", "Outcome"),
    ("rpt.starter.col.stage_measured", "Stage Measured"),
    ("rpt.starter.deviation_summary.heading", "Deviation Summary"),
    ("rpt.starter.deviation_summary.nonconforming_heading", "Nonconforming Characteristics"),
    ("rpt.starter.deviation_summary.outcomes_heading", "Outcome Counts"),
    ("rpt.starter.deviation_summary.title", "Deviation Summary"),
    ("rpt.starter.multistage_table.heading", "Multi-Stage Comparison"),
    ("rpt.starter.multistage_table.title", "Multi-Stage Comparison Table"),
];

/// Looks up the English source text for a starter catalog key.
pub fn lookup_label(key: &str) -> Option<&'static str> {
    STARTER_LABELS.binary_search_by_key(&key, |(k, _)| *k).ok().and_then(|idx| STARTER_LABELS.get(idx).map(|(_, v)| *v))
}

/// Resolves a starter logical name (such as `tmpl:char-table`) to its [`TemplateId`].
pub fn resolve_starter_name(name: &str) -> Option<TemplateId> {
    let desc = STARTERS.iter().find(|d| d.name == name)?;
    let uuid = Uuid::parse(desc.id).ok()?;
    TemplateId::new(uuid)
}

/// Resolves a starter [`TemplateId`] back to its logical name (`tmpl:*`), if it is a starter.
pub fn starter_name_for_id(id: TemplateId) -> Option<&'static str> {
    let text = id.uuid().to_string();
    STARTERS.iter().find(|d| d.id == text).map(|d| d.name)
}

/// Parses one starter template by logical name (e.g. `tmpl:char-table`) via [`parse_template`].
pub fn parse_starter(name: &str) -> Result<Template, TemplateError> {
    let Some(desc) = STARTERS.iter().find(|d| d.name == name) else {
        return Err(TemplateError::invalid("", "unknown_starter"));
    };
    parse_template(desc.canonical_json)
}

/// Parses, validates (format + slots) and registers all three unlocked starter templates into
/// `registry` at version 1.
pub fn register_starters(registry: &mut BuiltinRegistry) -> Result<Vec<TemplateRef>, VerError> {
    let mut refs = Vec::with_capacity(STARTERS.len());
    for desc in &STARTERS {
        let tmpl = parse_template(desc.canonical_json)?;
        validate_template(&tmpl)?;
        let r = registry.register(tmpl, STARTER_VERSION)?;
        refs.push(r);
    }
    Ok(refs)
}

/// Builds a [`BuiltinRegistry`] pre-populated with all three unlocked starter templates at
/// version 1.
pub fn starter_registry() -> Result<BuiltinRegistry, VerError> {
    let mut reg = BuiltinRegistry::new();
    register_starters(&mut reg)?;
    Ok(reg)
}
