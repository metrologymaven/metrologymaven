//! Persona definitions file `persona-set@1` and its check (spec node FLOW.PERSONA.PERS).
//!
//! The file holds exactly one record for each persona `JOB.PER.01`..`JOB.PER.09`
//! (REQ-FLOW.PERSONA.PERS-001). Each record names a role, goals, working environment,
//! primary focused app, entry phase and usual governance profile (REQ-002). The check
//! cross-references `primary_app` against the app list file (ECO.APPS.LIST) and
//! `governance_profile` against the built-in profile names (TRUST.PROFILE.DEFS) (REQ-004).
//!
//! Output is a pure function of the two input files: findings are sorted by code, then
//! subject, then detail (determinism class D0).

use fid_canon::CValue;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

/// Format tag of the persona file.
pub const FORMAT: &str = "persona-set@1";
/// Largest accepted persona file, in bytes (Design: bounded parse, 256 KiB).
pub const MAX_INPUT_BYTES: usize = 256 * 1024;
/// Deepest accepted JSON nesting; the format needs 4 (object, array, object, array).
pub const MAX_DEPTH: usize = 8;
/// Largest accepted app list file, in bytes (the ECO.APPS.LIST bound).
pub const MAX_APP_LIST_BYTES: usize = 64 * 1024;
/// Longest `role`, in Unicode scalar values (REQ-002).
pub const ROLE_MAX_CHARS: usize = 120;
/// Longest single goal, in Unicode scalar values. The spec sets no limit; this one keeps
/// goals to one sentence.
pub const GOAL_MAX_CHARS: usize = 200;
/// Fewest and most goals per record (REQ-002).
pub const GOALS_MIN: usize = 1;
pub const GOALS_MAX: usize = 5;

/// The nine persona IDs, in order (seed rows in `seeds/journeys.md`).
pub const PERSONA_IDS: [&str; 9] = [
    "JOB.PER.01",
    "JOB.PER.02",
    "JOB.PER.03",
    "JOB.PER.04",
    "JOB.PER.05",
    "JOB.PER.06",
    "JOB.PER.07",
    "JOB.PER.08",
    "JOB.PER.09",
];
/// Allowed working environments (REQ-002).
pub const ENVIRONMENTS: [&str; 4] = ["shop_floor", "office", "lab", "field"];
/// CONSTITUTION §7 phases.
pub const PHASES: [&str; 7] = ["P0", "P1a", "P1b", "P2", "P3", "P4", "later"];
/// Built-in governance profile names (REQ-TRUST.PROFILE.DEFS-001). Kept here until the
/// profile crate exists, then read from it.
pub const BUILTIN_PROFILES: [&str; 3] = ["lean", "regulated", "assistive_only"];
/// `primary_app` values that are not apps in the app list: the verifier and the driver SDK.
pub const NON_APP_TARGETS: [&str; 2] = ["verifier", "sdk"];
/// The auditor persona (REQ-003).
pub const AUDITOR_ID: &str = "JOB.PER.06";
/// Environments allowed for the auditor (REQ-003).
pub const AUDITOR_ENVIRONMENTS: [&str; 2] = ["office", "lab"];

const TOP_FIELDS: [&str; 2] = ["format", "personas"];
const RECORD_FIELDS: [&str; 7] =
    ["id", "role", "goals", "environment", "primary_app", "entry_phase", "governance_profile"];

/// Finding codes. Declared in alphabetical order of their text so the derived order sorts
/// findings by code text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Code {
    /// `primary_app` not in the app list (and not `verifier`/`sdk`), the auditor not on the
    /// verifier, or the app list unreadable.
    App,
    /// A record whose ID is not one of the nine, or a second record for the same ID.
    Extra,
    /// One of the nine IDs has no record.
    Missing,
    /// `governance_profile` is not a built-in profile name.
    Profile,
    /// The file is unreadable, too large, not JSON, or a field breaks the schema.
    Schema,
}

impl Code {
    /// Stable text of the code.
    pub fn as_str(self) -> &'static str {
        match self {
            Code::App => "PERS-APP",
            Code::Extra => "PERS-EXTRA",
            Code::Missing => "PERS-MISSING",
            Code::Profile => "PERS-PROFILE",
            Code::Schema => "PERS-SCHEMA",
        }
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One finding: `<CODE> <subject>: <detail>`. The subject is a persona ID, a record
/// position such as `personas[3]`, or `-` for the whole file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    pub code: Code,
    pub subject: String,
    pub detail: String,
}

impl Finding {
    fn new(code: Code, subject: &str, detail: impl Into<String>) -> Finding {
        Finding { code, subject: subject.to_string(), detail: detail.into() }
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}: {}", self.code, self.subject, self.detail)
    }
}

/// One persona record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Persona {
    pub id: String,
    pub role: String,
    pub goals: Vec<String>,
    pub environment: String,
    pub primary_app: String,
    pub entry_phase: String,
    pub governance_profile: String,
}

/// The persona file, records sorted by ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonaSet {
    pub format: String,
    pub personas: Vec<Persona>,
}

/// Result of a check: the records that parsed (sorted by ID) and the sorted findings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub set: PersonaSet,
    pub findings: Vec<Finding>,
}

impl Report {
    fn failed(finding: Finding) -> Report {
        Report { set: PersonaSet { format: String::new(), personas: Vec::new() }, findings: vec![finding] }
    }

    /// True when there are no findings.
    pub fn is_ok(&self) -> bool {
        self.findings.is_empty()
    }
}

/// Reads the `id` of every `[[app]]` table in an ECO.APPS.LIST app list. Only the IDs are
/// read; the rest of the file is the business of `apps-check`.
pub fn parse_app_ids(text: &str) -> Result<BTreeSet<String>, String> {
    if text.len() > MAX_APP_LIST_BYTES {
        return Err(format!("app list exceeds {} KiB", MAX_APP_LIST_BYTES / 1024));
    }
    let table: toml::Table = text.parse().map_err(|e: toml::de::Error| format!("app list is not TOML: {}", e.message()))?;
    let mut ids = BTreeSet::new();
    match table.get("app") {
        None => {}
        Some(toml::Value::Array(apps)) => {
            for (i, app) in apps.iter().enumerate() {
                match app.get("id") {
                    Some(toml::Value::String(id)) => {
                        ids.insert(id.clone());
                    }
                    _ => return Err(format!("app list entry app[{i}] has no string `id`")),
                }
            }
        }
        Some(_) => return Err("app list `app` is not an array of tables".to_string()),
    }
    Ok(ids)
}

/// Checks a persona file against the given app IDs (REQ-001..REQ-004).
pub fn check(input: &[u8], app_ids: &BTreeSet<String>) -> Report {
    if input.len() > MAX_INPUT_BYTES {
        return Report::failed(Finding::new(Code::Schema, "-", format!("file exceeds {} KiB", MAX_INPUT_BYTES / 1024)));
    }
    let value = match fid_canon::parse_with_limits(input, MAX_INPUT_BYTES, MAX_DEPTH) {
        Ok(v) => v,
        Err(e) => return Report::failed(Finding::new(Code::Schema, "-", format!("not valid JSON ({e})"))),
    };
    let mut findings = Vec::new();
    let top = match &value {
        CValue::Obj(m) => m,
        _ => return Report::failed(Finding::new(Code::Schema, "-", "top level is not an object")),
    };
    for key in top.keys() {
        if !TOP_FIELDS.contains(&key.as_str()) {
            findings.push(Finding::new(Code::Schema, "-", format!("unknown field `{key}`")));
        }
    }
    let format = match top.get("format") {
        Some(CValue::Str(s)) if s == FORMAT => s.clone(),
        Some(CValue::Str(s)) => {
            findings.push(Finding::new(Code::Schema, "-", format!("format is `{s}`, expected `{FORMAT}`")));
            s.clone()
        }
        _ => {
            findings.push(Finding::new(Code::Schema, "-", format!("`format` must be the string `{FORMAT}`")));
            String::new()
        }
    };
    let records: &[CValue] = match top.get("personas") {
        Some(CValue::Arr(a)) => a.as_slice(),
        _ => {
            findings.push(Finding::new(Code::Schema, "-", "`personas` must be an array"));
            &[]
        }
    };

    let mut seen: BTreeMap<String, Persona> = BTreeMap::new();
    for (i, rec) in records.iter().enumerate() {
        let at = format!("personas[{i}]");
        if let Some(p) = check_record(rec, &at, app_ids, &mut findings) {
            if !PERSONA_IDS.contains(&p.id.as_str()) {
                findings.push(Finding::new(Code::Extra, &p.id, "not one of JOB.PER.01..JOB.PER.09"));
            } else if seen.contains_key(&p.id) {
                findings.push(Finding::new(Code::Extra, &p.id, format!("second record for this ID at {at}")));
            } else {
                seen.insert(p.id.clone(), p);
            }
        }
    }
    for id in PERSONA_IDS {
        if !seen.contains_key(id) {
            findings.push(Finding::new(Code::Missing, id, "no record"));
        }
    }
    findings.sort();
    findings.dedup();
    Report { set: PersonaSet { format, personas: seen.into_values().collect() }, findings }
}

/// Checks one record. Returns the record when it has a string `id` (even if other fields
/// fail, so the ID still counts as present); findings go to `out`.
fn check_record(rec: &CValue, at: &str, app_ids: &BTreeSet<String>, out: &mut Vec<Finding>) -> Option<Persona> {
    let m = match rec {
        CValue::Obj(m) => m,
        _ => {
            out.push(Finding::new(Code::Schema, at, "record is not an object"));
            return None;
        }
    };
    let id = match m.get("id") {
        Some(CValue::Str(s)) => s.clone(),
        _ => {
            out.push(Finding::new(Code::Schema, at, "`id` missing or not a string"));
            return None;
        }
    };
    let who = id.as_str();
    for key in m.keys() {
        if !RECORD_FIELDS.contains(&key.as_str()) {
            out.push(Finding::new(Code::Schema, who, format!("unknown field `{key}`")));
        }
    }

    let auditor = id == AUDITOR_ID;

    let role = text_field(m, "role", who, out);
    if let Some(role) = &role {
        let n = role.chars().count();
        if role.trim().is_empty() || n > ROLE_MAX_CHARS {
            out.push(Finding::new(Code::Schema, who, format!("`role` must be 1 to {ROLE_MAX_CHARS} characters, has {n}")));
        }
    }

    let mut goals = Vec::new();
    match m.get("goals") {
        Some(CValue::Arr(items)) => {
            if items.len() < GOALS_MIN || items.len() > GOALS_MAX {
                out.push(Finding::new(
                    Code::Schema,
                    who,
                    format!("`goals` must hold {GOALS_MIN} to {GOALS_MAX} items, has {}", items.len()),
                ));
            }
            for (j, g) in items.iter().enumerate() {
                match g {
                    CValue::Str(s) if !s.trim().is_empty() && s.chars().count() <= GOAL_MAX_CHARS => {
                        goals.push(s.clone())
                    }
                    _ => out.push(Finding::new(
                        Code::Schema,
                        who,
                        format!("goals[{j}] must be a string of 1 to {GOAL_MAX_CHARS} characters"),
                    )),
                }
            }
        }
        Some(_) => out.push(Finding::new(Code::Schema, who, "`goals` is not an array")),
        None => out.push(Finding::new(Code::Schema, who, "missing field `goals`")),
    }

    let environment = text_field(m, "environment", who, out);
    if let Some(env) = &environment {
        if !ENVIRONMENTS.contains(&env.as_str()) {
            out.push(Finding::new(Code::Schema, who, format!("unknown environment `{env}`")));
        } else if auditor && !AUDITOR_ENVIRONMENTS.contains(&env.as_str()) {
            // REQ-003.
            out.push(Finding::new(Code::Schema, who, format!("the auditor's environment must be `office` or `lab`, is `{env}`")));
        }
    }

    let primary_app = text_field(m, "primary_app", who, out);
    if let Some(app) = &primary_app {
        if auditor {
            // REQ-003 (and REQ-FLOW.PERSONA-007: the auditor maps onto the verifier only).
            if app != "verifier" {
                out.push(Finding::new(Code::App, who, format!("the auditor's primary_app must be `verifier`, is `{app}`")));
            }
        } else if !NON_APP_TARGETS.contains(&app.as_str()) && !app_ids.contains(app) {
            // REQ-004.
            out.push(Finding::new(Code::App, who, format!("primary_app `{app}` is not in the app list")));
        }
    }

    let entry_phase = text_field(m, "entry_phase", who, out);
    if let Some(phase) = &entry_phase {
        if !PHASES.contains(&phase.as_str()) {
            out.push(Finding::new(Code::Schema, who, format!("entry_phase `{phase}` is not a CONSTITUTION §7 phase")));
        }
    }

    let governance_profile = text_field(m, "governance_profile", who, out);
    if let Some(profile) = &governance_profile {
        if !BUILTIN_PROFILES.contains(&profile.as_str()) {
            // REQ-004.
            out.push(Finding::new(Code::Profile, who, format!("governance_profile `{profile}` is not a built-in profile")));
        }
    }

    Some(Persona {
        id,
        role: role.unwrap_or_default(),
        goals,
        environment: environment.unwrap_or_default(),
        primary_app: primary_app.unwrap_or_default(),
        entry_phase: entry_phase.unwrap_or_default(),
        governance_profile: governance_profile.unwrap_or_default(),
    })
}

/// Reads a required string field; pushes a schema finding when it is missing or not a string.
fn text_field(m: &BTreeMap<String, CValue>, key: &str, who: &str, out: &mut Vec<Finding>) -> Option<String> {
    match m.get(key) {
        Some(CValue::Str(s)) => Some(s.clone()),
        Some(_) => {
            out.push(Finding::new(Code::Schema, who, format!("`{key}` is not a string")));
            None
        }
        None => {
            out.push(Finding::new(Code::Schema, who, format!("missing field `{key}`")));
            None
        }
    }
}

/// Reads both files and checks them. An unreadable persona file gives `PERS-SCHEMA`; an
/// unreadable or malformed app list gives `PERS-APP` (no `primary_app` can be confirmed).
pub fn check_files(personas: &Path, apps: &Path) -> Report {
    let app_ids = match read_bounded(apps, MAX_APP_LIST_BYTES) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) => match parse_app_ids(&text) {
                Ok(ids) => ids,
                Err(e) => return Report::failed(Finding::new(Code::App, "-", e)),
            },
            Err(_) => return Report::failed(Finding::new(Code::App, "-", "app list is not UTF-8")),
        },
        Err(e) => {
            return Report::failed(Finding::new(Code::App, "-", format!("cannot read app list {}: {e}", apps.display())))
        }
    };
    match read_bounded(personas, MAX_INPUT_BYTES) {
        Ok(bytes) => check(&bytes, &app_ids),
        Err(e) => Report::failed(Finding::new(Code::Schema, "-", format!("cannot read {}: {e}", personas.display()))),
    }
}

/// Reads at most `limit + 1` bytes, so an oversized file is detected without reading it all.
fn read_bounded(path: &Path, limit: usize) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let file = std::fs::File::open(path)?;
    let mut buf = Vec::new();
    let cap = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
    file.take(cap).read_to_end(&mut buf)?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_order_is_text_order() {
        let codes = [Code::App, Code::Extra, Code::Missing, Code::Profile, Code::Schema];
        let mut texts: Vec<&str> = codes.iter().map(|c| c.as_str()).collect();
        let before = texts.clone();
        texts.sort_unstable();
        assert_eq!(texts, before);
    }
}
