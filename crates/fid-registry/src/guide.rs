//! Task-guide declarations. Spec node `CORE.REG.DEF.GUIDE`.
//!
//! A task guide names an ordered list of command steps, each with a short purpose, that an
//! agent or a person calls for one task. It holds no values, no standards text and nothing
//! executable (REQ-004): the only free text is the title, the summary and the step purposes.
//!
//! JSON shape (unknown fields refused at every level):
//!
//! ```json
//! {"id": "demo.align_scan", "title": "...", "summary": "...", "profile": "core",
//!  "steps": [{"command": "demo.load_scan@1", "purpose": "..."}]}
//! ```
//!
//! [`TaskGuide::from_value`] reads one guide and checks its own shape; [`check_guides`]
//! checks a list of guides against the command definitions of the same registry build.

use crate::def::CommandDef;
use fid_canon::{parse, CValue};
use fid_types::{CommandId, CommandName, SideEffect};
use std::collections::BTreeMap;
use std::fmt;

/// Largest title, in Unicode scalar values (REQ-001).
pub const MAX_TITLE: usize = 80;
/// Largest summary, in Unicode scalar values (REQ-001).
pub const MAX_SUMMARY: usize = 500;
/// Largest step purpose, in Unicode scalar values (REQ-001).
pub const MAX_PURPOSE: usize = 200;
/// Largest number of steps (REQ-001).
pub const MAX_STEPS: usize = 30;
/// Largest guide ID, in bytes. The pattern of REQ-001 sets no length; this build bounds it.
pub const MAX_GUIDE_ID: usize = 64;
/// JSON Schema pattern of a guide ID (REQ-001).
pub const GUIDE_ID_PATTERN: &str = "^[a-z][a-z0-9]*\\.[a-z][a-z0-9_]*$";
/// JSON Schema pattern of an MCP profile name (`AUTO.MCP.SRV` REQ-001).
pub const PROFILE_PATTERN: &str = "^[a-z][a-z0-9_]{1,31}$";
/// JSON Schema pattern of a step's command reference: a command ID whose major is written
/// without leading zeros, so the text names exactly one (ID, major).
pub const STEP_COMMAND_PATTERN: &str = "^[a-z0-9]([a-z0-9_.]*[a-z0-9])?@(0|[1-9][0-9]{0,4})$";
/// Side-effect classes a guide step may never name (REQ-003).
pub const FORBIDDEN_STEP_CLASSES: [SideEffect; 3] = [SideEffect::M1, SideEffect::S1, SideEffect::A0];

const GUIDE_FIELDS: [&str; 5] = ["id", "title", "summary", "profile", "steps"];
const STEP_FIELDS: [&str; 2] = ["command", "purpose"];

/// One step of a guide: a command (ID and major) and why it is called.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuideStep {
    pub command: CommandId,
    /// 1..=[`MAX_PURPOSE`] characters.
    pub purpose: String,
}

/// A task guide (REQ-001). Carries no numeric field and no body beyond `title`, `summary`
/// and the step purposes (REQ-004).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskGuide {
    /// Matches [`GUIDE_ID_PATTERN`], at most [`MAX_GUIDE_ID`] bytes.
    pub id: String,
    /// 1..=[`MAX_TITLE`] characters.
    pub title: String,
    /// 1..=[`MAX_SUMMARY`] characters.
    pub summary: String,
    /// MCP profile name, matching [`PROFILE_PATTERN`].
    pub profile: String,
    /// 1..=[`MAX_STEPS`] steps, in declaration order.
    pub steps: Vec<GuideStep>,
}

/// Guide errors with stable codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuideError {
    /// A step names an (ID, major) that the build does not declare, or declares as `removed`.
    UnknownCommand {
        guide: String,
        step: usize,
        command: String,
    },
    /// A step names an M1, S1 or A0 command.
    ForbiddenStep {
        guide: String,
        step: usize,
        command: String,
        class: SideEffect,
    },
    /// More than [`MAX_STEPS`] steps or a text over its limit; `field` names which.
    TooLarge {
        guide: String,
        field: String,
        limit: usize,
        actual: usize,
    },
    /// The guide ID or profile name does not match its pattern.
    BadId {
        field: &'static str,
        value: String,
    },
    /// An empty title, summary or purpose, or no steps.
    Empty {
        guide: String,
        field: String,
    },
    /// Two guides of one build share an ID.
    Duplicate(String),
    UnknownField(String),
    MissingField(String),
    BadType {
        field: String,
        expected: &'static str,
    },
    /// The JSON could not be parsed (`fid-canon` error code).
    Json(&'static str),
}

impl GuideError {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        match self {
            GuideError::UnknownCommand { .. } => "guide_unknown_command",
            GuideError::ForbiddenStep { .. } => "guide_forbidden_step",
            GuideError::TooLarge { .. } => "guide_too_large",
            GuideError::BadId { .. } => "guide_bad_id",
            GuideError::Empty { .. } => "guide_empty",
            GuideError::Duplicate(_) => "guide_duplicate",
            GuideError::UnknownField(_) => "unknown_field",
            GuideError::MissingField(_) => "missing_field",
            GuideError::BadType { .. } => "bad_type",
            GuideError::Json(c) => c,
        }
    }
}

impl fmt::Display for GuideError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            GuideError::UnknownCommand { guide, step, command } => {
                write!(f, "{code}: guide {guide} step {step} names {command}")
            }
            GuideError::ForbiddenStep { guide, step, command, class } => {
                write!(f, "{code}: guide {guide} step {step} names {command} of class {class}")
            }
            GuideError::TooLarge { guide, field, limit, actual } => {
                write!(f, "{code}: guide {guide} {field} has {actual}, limit {limit}")
            }
            GuideError::BadId { field, value } => write!(f, "{code}: {field} {value:?}"),
            GuideError::Empty { guide, field } => write!(f, "{code}: guide {guide} {field} is empty"),
            GuideError::Duplicate(id) => write!(f, "{code}: guide {id} is declared more than once"),
            GuideError::UnknownField(k) => write!(f, "{code}: {k}"),
            GuideError::MissingField(k) => write!(f, "{code}: {k}"),
            GuideError::BadType { field, expected } => write!(f, "{code}: {field} must be {expected}"),
            GuideError::Json(_) => f.write_str(code),
        }
    }
}

impl std::error::Error for GuideError {}

/// Whether `s` matches [`GUIDE_ID_PATTERN`] and is at most [`MAX_GUIDE_ID`] bytes.
pub fn is_guide_id(s: &str) -> bool {
    let Some((domain, name)) = s.split_once('.') else { return false };
    let lower = |c: char| c.is_ascii_lowercase();
    let starts_lower = |p: &str| p.chars().next().is_some_and(lower);
    s.len() <= MAX_GUIDE_ID
        && starts_lower(domain)
        && domain.chars().all(|c| lower(c) || c.is_ascii_digit())
        && starts_lower(name)
        && name.chars().all(|c| lower(c) || c.is_ascii_digit() || c == '_')
}

/// Whether `s` matches [`PROFILE_PATTERN`].
pub fn is_profile_name(s: &str) -> bool {
    (2..=32).contains(&s.len())
        && s.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Parses a step's command reference: a command ID whose major has no leading zeros.
fn parse_command_ref(s: &str) -> Option<CommandId> {
    let id = CommandId::parse(s).ok()?;
    let name = CommandName::parse(id.name()).ok()?;
    let canonical = CommandId::new(&name, id.major()).ok()?;
    (canonical.as_str() == s).then_some(canonical)
}

fn refuse_unknown(map: &BTreeMap<String, CValue>, allowed: &[&str], at: &str) -> Result<(), GuideError> {
    match map.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(k) => Err(GuideError::UnknownField(format!("{at}{k}"))),
        None => Ok(()),
    }
}

fn take_string(map: &mut BTreeMap<String, CValue>, k: &str, at: &str) -> Result<String, GuideError> {
    match map.remove(k) {
        None => Err(GuideError::MissingField(format!("{at}{k}"))),
        Some(CValue::Str(s)) => Ok(s),
        Some(_) => Err(GuideError::BadType { field: format!("{at}{k}"), expected: "a string" }),
    }
}

impl GuideStep {
    fn from_value(v: CValue, at: &str) -> Result<GuideStep, GuideError> {
        let CValue::Obj(mut o) = v else {
            return Err(GuideError::BadType { field: at.trim_end_matches('.').to_string(), expected: "an object" });
        };
        refuse_unknown(&o, &STEP_FIELDS, at)?;
        let text = take_string(&mut o, "command", at)?;
        let command = parse_command_ref(&text).ok_or_else(|| GuideError::BadType {
            field: format!("{at}command"),
            expected: "a command ID name@major (major without leading zeros)",
        })?;
        let purpose = take_string(&mut o, "purpose", at)?;
        Ok(GuideStep { command, purpose })
    }

    fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("command".to_string(), CValue::Str(self.command.as_str().to_string()));
        o.insert("purpose".to_string(), CValue::Str(self.purpose.clone()));
        CValue::Obj(o)
    }
}

fn check_text(guide: &str, field: &str, text: &str, limit: usize) -> Result<(), GuideError> {
    let n = text.chars().count();
    if n == 0 {
        return Err(GuideError::Empty { guide: guide.to_string(), field: field.to_string() });
    }
    if n > limit {
        return Err(GuideError::TooLarge { guide: guide.to_string(), field: field.to_string(), limit, actual: n });
    }
    Ok(())
}

impl TaskGuide {
    /// Builds and checks a guide from `(command, purpose)` pairs; used by [`crate::fid_guides!`].
    pub fn new(
        id: &str,
        title: &str,
        summary: &str,
        profile: &str,
        steps: &[(&str, &str)],
    ) -> Result<TaskGuide, GuideError> {
        let steps = steps
            .iter()
            .enumerate()
            .map(|(i, (command, purpose))| {
                let command = parse_command_ref(command).ok_or_else(|| GuideError::BadType {
                    field: format!("steps[{i}].command"),
                    expected: "a command ID name@major (major without leading zeros)",
                })?;
                Ok(GuideStep { command, purpose: (*purpose).to_string() })
            })
            .collect::<Result<Vec<_>, GuideError>>()?;
        let guide = TaskGuide {
            id: id.to_string(),
            title: title.to_string(),
            summary: summary.to_string(),
            profile: profile.to_string(),
            steps,
        };
        guide.validate()?;
        Ok(guide)
    }

    /// Parses one guide from JSON bytes.
    pub fn from_json(bytes: &[u8]) -> Result<TaskGuide, GuideError> {
        TaskGuide::from_value(parse(bytes).map_err(|e| GuideError::Json(e.code()))?)
    }

    /// Reads one guide, refusing unknown fields, then checks its shape with [`TaskGuide::validate`].
    pub fn from_value(v: CValue) -> Result<TaskGuide, GuideError> {
        let CValue::Obj(mut o) = v else {
            return Err(GuideError::BadType { field: "<guide>".into(), expected: "an object" });
        };
        refuse_unknown(&o, &GUIDE_FIELDS, "")?;
        let id = take_string(&mut o, "id", "")?;
        let title = take_string(&mut o, "title", "")?;
        let summary = take_string(&mut o, "summary", "")?;
        let profile = take_string(&mut o, "profile", "")?;
        let steps = match o.remove("steps") {
            None => return Err(GuideError::MissingField("steps".into())),
            Some(CValue::Arr(items)) => items
                .into_iter()
                .enumerate()
                .map(|(i, v)| GuideStep::from_value(v, &format!("steps[{i}].")))
                .collect::<Result<Vec<_>, _>>()?,
            Some(_) => return Err(GuideError::BadType { field: "steps".into(), expected: "an array" }),
        };
        let guide = TaskGuide { id, title, summary, profile, steps };
        guide.validate()?;
        Ok(guide)
    }

    /// Checks the guide on its own (REQ-001): ID and profile patterns, then the title,
    /// summary, step count and each purpose against their limits. First failure wins.
    pub fn validate(&self) -> Result<(), GuideError> {
        if !is_guide_id(&self.id) {
            return Err(GuideError::BadId { field: "id", value: self.id.clone() });
        }
        if !is_profile_name(&self.profile) {
            return Err(GuideError::BadId { field: "profile", value: self.profile.clone() });
        }
        check_text(&self.id, "title", &self.title, MAX_TITLE)?;
        check_text(&self.id, "summary", &self.summary, MAX_SUMMARY)?;
        if self.steps.is_empty() {
            return Err(GuideError::Empty { guide: self.id.clone(), field: "steps".into() });
        }
        if self.steps.len() > MAX_STEPS {
            return Err(GuideError::TooLarge {
                guide: self.id.clone(),
                field: "steps".into(),
                limit: MAX_STEPS,
                actual: self.steps.len(),
            });
        }
        for (i, step) in self.steps.iter().enumerate() {
            check_text(&self.id, &format!("steps[{i}].purpose"), &step.purpose, MAX_PURPOSE)?;
        }
        Ok(())
    }

    /// The JSON value of this guide, steps in declaration order (REQ-005).
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("id".to_string(), CValue::Str(self.id.clone()));
        o.insert("title".to_string(), CValue::Str(self.title.clone()));
        o.insert("summary".to_string(), CValue::Str(self.summary.clone()));
        o.insert("profile".to_string(), CValue::Str(self.profile.clone()));
        o.insert("steps".to_string(), CValue::Arr(self.steps.iter().map(GuideStep::to_value).collect()));
        CValue::Obj(o)
    }
}

/// Reads a JSON array of guides; the first malformed guide fails the read.
pub fn guides_from_value(v: CValue) -> Result<Vec<TaskGuide>, GuideError> {
    match v {
        CValue::Arr(items) => items.into_iter().map(TaskGuide::from_value).collect(),
        _ => Err(GuideError::BadType { field: "<root>".into(), expected: "an array of guides" }),
    }
}

/// A list of guides as JSON, sorted by guide ID (REQ-005); equal IDs keep their order.
pub fn guides_to_value(guides: &[TaskGuide]) -> CValue {
    let mut sorted: Vec<&TaskGuide> = guides.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    CValue::Arr(sorted.into_iter().map(TaskGuide::to_value).collect())
}

/// Whether a definition's lifecycle says `removed`; such a major is not in the build's
/// manifest (`CORE.REG.VER.LIFE` REQ-004), so a guide cannot name it.
fn is_removed(def: &CommandDef) -> bool {
    matches!(def.lifecycle.get("state"), Some(CValue::Str(s)) if s == "removed")
}

/// Checks the guides of one registry build against its command definitions
/// (REQ-001..REQ-003). For each guide in the given order: its own shape
/// ([`TaskGuide::validate`]; a guide that fails it is not looked up further), then each step
/// in order, `guide_unknown_command` or `guide_forbidden_step`. Last, one
/// `guide_duplicate` per guide ID declared more than once, in ID order. Empty means the
/// guides pass. The definitions are looked up by (ID, major) in a sorted index.
pub fn check_guides(guides: &[TaskGuide], defs: &[CommandDef]) -> Vec<GuideError> {
    let mut index: BTreeMap<(&str, u32), &CommandDef> = BTreeMap::new();
    for d in defs.iter().filter(|d| !is_removed(d)) {
        index.entry((d.id.as_str(), d.major)).or_insert(d);
    }
    let mut errors = Vec::new();
    for g in guides {
        if let Err(e) = g.validate() {
            errors.push(e);
            continue;
        }
        for (i, step) in g.steps.iter().enumerate() {
            let command = step.command.as_str().to_string();
            match index.get(&(step.command.name(), step.command.major())) {
                None => errors.push(GuideError::UnknownCommand { guide: g.id.clone(), step: i, command }),
                Some(d) if FORBIDDEN_STEP_CLASSES.contains(&d.side_effect) => errors.push(GuideError::ForbiddenStep {
                    guide: g.id.clone(),
                    step: i,
                    command,
                    class: d.side_effect,
                }),
                Some(_) => {}
            }
        }
    }
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    for g in guides {
        *seen.entry(g.id.as_str()).or_insert(0) += 1;
    }
    errors.extend(seen.into_iter().filter(|(_, n)| *n > 1).map(|(id, _)| GuideError::Duplicate(id.to_string())));
    errors
}

// ------------------------------------------------------------------ schema

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|x| CValue::Str((*x).to_string())).collect())
}

fn num(n: usize) -> CValue {
    CValue::Num(u32::try_from(n).map_or(f64::MAX, f64::from))
}

fn text(max: usize) -> CValue {
    obj([("type", s("string")), ("minLength", num(1)), ("maxLength", num(max))])
}

/// JSON Schema (2020-12) of one task guide; checked in as `schema/task-guide.schema.json`.
pub fn task_guide_schema() -> CValue {
    let step = obj([
        ("type", s("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&STEP_FIELDS)),
        (
            "properties",
            obj([
                ("command", obj([("type", s("string")), ("pattern", s(STEP_COMMAND_PATTERN))])),
                ("purpose", text(MAX_PURPOSE)),
            ]),
        ),
    ]);
    obj([
        ("$schema", s("https://json-schema.org/draft/2020-12/schema")),
        ("$id", s("https://metrologymaven.io/schema/task-guide.schema.json")),
        ("title", s("Metrology Maven task guide")),
        ("type", s("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&GUIDE_FIELDS)),
        (
            "properties",
            obj([
                (
                    "id",
                    obj([("type", s("string")), ("pattern", s(GUIDE_ID_PATTERN)), ("maxLength", num(MAX_GUIDE_ID))]),
                ),
                ("title", text(MAX_TITLE)),
                ("summary", text(MAX_SUMMARY)),
                ("profile", obj([("type", s("string")), ("pattern", s(PROFILE_PATTERN))])),
                (
                    "steps",
                    obj([("type", s("array")), ("minItems", num(1)), ("maxItems", num(MAX_STEPS)), ("items", step)]),
                ),
            ]),
        ),
    ])
}

/// Declares a crate's task guides next to its commands and generates
/// `pub fn guides() -> Result<Vec<TaskGuide>, GuideError>`, guides in source order. Each guide
/// is checked on its own when built; [`check_guides`] checks the references at registry build.
/// The generated code has no `unsafe`, no statics and cannot panic.
///
/// ```
/// mod demo {
///     fid_registry::fid_guides! {
///         "demo.align_scan" {
///             title: "Align a scan to CAD",
///             summary: "Load a scan, fit it to the nominal model and apply the alignment.",
///             profile: "core",
///             steps: [
///                 "demo.load_scan@1" => "Load the point cloud to align.",
///                 "demo.fit_alignment@1" => "Fit the scan to the CAD model.",
///             ],
///         }
///     }
/// }
/// assert_eq!(demo::guides().map(|g| g.len()), Ok(1));
/// ```
#[macro_export]
macro_rules! fid_guides {
    ($(
        $id:literal {
            title: $title:literal,
            summary: $summary:literal,
            profile: $profile:literal,
            steps: [ $( $command:literal => $purpose:literal ),+ $(,)? ] $(,)?
        }
    ),* $(,)?) => {
        /// The task guides this crate declares, in source order.
        pub fn guides() -> ::core::result::Result<
            ::std::vec::Vec<$crate::guide::TaskGuide>,
            $crate::guide::GuideError,
        > {
            ::core::result::Result::Ok(::std::vec![$(
                $crate::guide::TaskGuide::new($id, $title, $summary, $profile, &[$( ($command, $purpose) ),+])?
            ),*])
        }
    };
}
