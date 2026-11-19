//! Benchmark recipe format `bench-recipe@1` and its validator (spec node `OUTR.BENCH.RUN.RECIPE`).
//!
//! A recipe is one TOML file that states everything needed to reproduce a public benchmark
//! result: the release it applies from, the dataset (catalogue ID and SHA-256), the machine
//! profile, where the numbers come from (`[source]`), the result fields with their regression
//! tolerances, and one kind section (`[accuracy]`, `[fidelity]`, `[performance]` or
//! `[comparison]`). This module owns the common part; each kind section is handed to the
//! [`KindValidator`] registered for its kind (this crate ships accept-anything stubs, see
//! [`default_registry`]).
//!
//! Determinism D0: input files are read in sorted path order (paths shown with `/`), all maps
//! are `BTreeMap`s, findings are sorted, and `--json` output is RFC 8785 canonical JSON.

use crate::launch_gate::is_release_tag;
use fid_canon::{to_canonical_bytes, CValue};
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};

/// The schema string every recipe states.
pub const SCHEMA: &str = "bench-recipe@1";
/// Largest accepted input file (recipe or catalogue), in bytes.
pub const MAX_BYTES: usize = 1024 * 1024;
/// Deepest directory level walked under a path argument.
const MAX_DEPTH: usize = 32;
/// Recipe kinds; each has a kind section of the same name (REQ-OUTR.BENCH.RUN.RECIPE-005).
pub const KINDS: [&str; 4] = ["accuracy", "fidelity", "performance", "comparison"];
/// Programs a `command` source may run (REQ-OUTR.BENCH.RUN.RECIPE-003).
pub const PROGRAMS: [&str; 2] = ["fid", "fid-verify"];
/// Allowed `machine_profile` values.
pub const MACHINE_PROFILES: [&str; 3] = ["minimum", "recommended", "any"];
/// Allowed `better` values: which direction of a result is better (REQ-004).
pub const BETTER: [&str; 3] = ["lower", "higher", "equal"];
/// Allowed `tolerance_kind` values (REQ-004).
pub const TOLERANCE_KINDS: [&str; 2] = ["abs", "rel"];
/// Unit symbols a result field may use: `1` for counts and ratios, otherwise a small compiled-in
/// subset of unit symbols (the unit catalogue of GEO.FRAME.UNITS is not built yet).
pub const UNITS: &[&str] = &[
    "1", "m", "mm", "um", "nm", "in", "rad", "mrad", "urad", "deg", "s", "ms", "us", "ns", "B", "KiB", "MiB", "GiB",
    "Hz",
];
/// Top-level keys of the common part.
const COMMON_KEYS: [&str; 9] =
    ["schema", "id", "kind", "title", "since_release", "machine_profile", "dataset", "source", "fields"];
/// Keys of one `[[fields]]` entry, in reporting order.
const FIELD_KEYS: [&str; 5] = ["name", "unit", "better", "tolerance", "tolerance_kind"];

/// Finding codes (spec Design, "Errors").
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Code {
    FieldMissing,
    FieldMalformed,
    DatasetUnknown,
    DatasetDigestMismatch,
    SourceInvalid,
    KindUnknown,
    KindSectionMissing,
    DuplicateId,
    IdMalformed,
}

impl Code {
    /// Stable code string.
    pub fn as_str(self) -> &'static str {
        match self {
            Code::FieldMissing => "field_missing",
            Code::FieldMalformed => "field_malformed",
            Code::DatasetUnknown => "dataset_unknown",
            Code::DatasetDigestMismatch => "dataset_digest_mismatch",
            Code::SourceInvalid => "source_invalid",
            Code::KindUnknown => "kind_unknown",
            Code::KindSectionMissing => "kind_section_missing",
            Code::DuplicateId => "duplicate_id",
            Code::IdMalformed => "id_malformed",
        }
    }
}

/// One validator finding. The derived order (file, id, field, code, message) is the report order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    /// The recipe file, as shown in reports (`/` separators).
    pub file: String,
    /// The recipe's `id` when it states one as a string.
    pub id: Option<String>,
    /// The field concerned, e.g. `dataset.sha256` or `fields.max_error.tolerance`.
    pub field: Option<String>,
    pub code: Code,
    pub message: String,
}

impl Finding {
    /// A finding about `field` of the recipe in `ctx`.
    pub fn new(ctx: &RecipeContext<'_>, code: Code, field: Option<&str>, message: impl Into<String>) -> Finding {
        Finding {
            file: ctx.file.to_string(),
            id: ctx.id.map(str::to_string),
            field: field.map(str::to_string),
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for Finding {
    /// `<code> <file>[ <id>][ <field>]: <message>`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.code.as_str(), self.file)?;
        if let Some(id) = &self.id {
            write!(f, " {id}")?;
        }
        if let Some(field) = &self.field {
            write!(f, " {field}")?;
        }
        write!(f, ": {}", self.message)
    }
}

/// What a kind validator sees of the recipe its section belongs to.
#[derive(Debug, Clone, Copy)]
pub struct RecipeContext<'a> {
    pub file: &'a str,
    pub id: Option<&'a str>,
    pub kind: &'a str,
    /// The whole recipe, for kind rules that look at common fields.
    pub recipe: &'a toml::Table,
}

/// Validates one kind section (REQ-OUTR.BENCH.RUN.RECIPE-005). Kind nodes (ACC, FIDEL, PERF,
/// CMP) implement this and register it with [`register`]. The caller sets `file` and `id` of the
/// returned findings to the recipe's own.
pub trait KindValidator {
    /// The kind this validator handles, e.g. `accuracy`.
    fn kind(&self) -> &str;
    /// Findings about `section` (the table named after the kind).
    fn validate(&self, section: &toml::Table, recipe: &RecipeContext<'_>) -> Vec<Finding>;
}

/// Kind validators keyed by kind.
pub type Registry = BTreeMap<String, Box<dyn KindValidator>>;

/// Adds (or replaces) the validator for its kind.
pub fn register(registry: &mut Registry, validator: Box<dyn KindValidator>) {
    registry.insert(validator.kind().to_string(), validator);
}

/// A placeholder validator that accepts any section; replaced by the kind nodes' validators.
#[derive(Debug, Clone, Copy)]
pub struct StubValidator(pub &'static str);

impl KindValidator for StubValidator {
    fn kind(&self) -> &str {
        self.0
    }

    fn validate(&self, _section: &toml::Table, _recipe: &RecipeContext<'_>) -> Vec<Finding> {
        Vec::new()
    }
}

/// The build-time registry: one [`StubValidator`] per kind in [`KINDS`].
pub fn default_registry() -> Registry {
    let mut r = Registry::new();
    for k in KINDS {
        register(&mut r, Box::new(StubValidator(k)));
    }
    r
}

/// Input that could not be read or parsed (exit 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputError {
    /// Unreadable file or directory, or a file larger than [`MAX_BYTES`].
    Io { file: String, message: String },
    /// Not UTF-8, not valid TOML, or a catalogue entry without a usable `id` and `sha256`.
    Parse { file: String, message: String },
}

impl InputError {
    /// Stable code string: `io` or `parse`.
    pub fn code(&self) -> &'static str {
        match self {
            InputError::Io { .. } => "io",
            InputError::Parse { .. } => "parse",
        }
    }

    /// The file concerned.
    pub fn file(&self) -> &str {
        match self {
            InputError::Io { file, .. } | InputError::Parse { file, .. } => file,
        }
    }

    fn message(&self) -> &str {
        match self {
            InputError::Io { message, .. } | InputError::Parse { message, .. } => message,
        }
    }

    /// Canonical JSON: `{"error":{"code","file","message"},"status":"error"}`.
    pub fn to_json(&self) -> Vec<u8> {
        let err = obj(vec![("code", s(self.code())), ("file", s(self.file())), ("message", s(self.message()))]);
        canonical(&obj(vec![("status", s("error")), ("error", err)]))
    }
}

impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}: {}", self.code(), self.file(), self.message())
    }
}

/// The dataset catalogue as this validator needs it: `id` to SHA-256.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Catalogue {
    pub entries: BTreeMap<String, Digest>,
}

/// Parses a catalogue file: `[[dataset]]` entries, of which only `id` and `sha256` are read
/// (other fields belong to ECO.DATA.CAT). `sha256` may be bare hex or `sha256:`-prefixed.
pub fn parse_catalogue(text: &str, file: &str) -> Result<Catalogue, InputError> {
    let t = parse_table(text, file)?;
    let bad = |message: String| InputError::Parse { file: file.to_string(), message };
    let list = match t.get("dataset") {
        None => return Ok(Catalogue::default()),
        Some(toml::Value::Array(a)) => a,
        Some(_) => return Err(bad("`dataset` is not an array of tables".to_string())),
    };
    let mut entries = BTreeMap::new();
    for (n, e) in list.iter().enumerate() {
        let id = e.get("id").and_then(toml::Value::as_str);
        let digest = e.get("sha256").and_then(toml::Value::as_str).and_then(parse_digest);
        let (Some(id), Some(digest)) = (id, digest) else {
            return Err(bad(format!("dataset entry #{} has no string `id` and valid `sha256`", n + 1)));
        };
        if entries.insert(id.to_string(), digest).is_some() {
            return Err(bad(format!("dataset id {id:?} is listed twice")));
        }
    }
    Ok(Catalogue { entries })
}

/// A recipe with no findings.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ValidRecipe {
    pub file: String,
    pub id: String,
    pub kind: String,
}

/// Result of a check over readable, parsable input.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Report {
    /// Recipes without findings, in file order.
    pub recipes: Vec<ValidRecipe>,
    /// All findings, sorted.
    pub findings: Vec<Finding>,
}

impl Report {
    /// 0 with no findings, 1 with any.
    pub fn exit_code(&self) -> u8 {
        u8::from(!self.findings.is_empty())
    }

    /// Text report: one line per finding, one `recipe <id> <kind> <file>` line per valid recipe,
    /// and a summary line.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for f in &self.findings {
            out.push_str(&format!("{f}\n"));
        }
        for r in &self.recipes {
            out.push_str(&format!("recipe {} {} {}\n", r.id, r.kind, r.file));
        }
        out.push_str(&format!(
            "bench-recipe check: {} valid recipe(s), {} finding(s)\n",
            self.recipes.len(),
            self.findings.len()
        ));
        out
    }

    /// Canonical JSON report: `status` (`ok` or `findings`), `recipes`, `findings`.
    pub fn to_json(&self) -> Vec<u8> {
        let recipes =
            self.recipes.iter().map(|r| obj(vec![("file", s(&r.file)), ("id", s(&r.id)), ("kind", s(&r.kind))]));
        let findings = self.findings.iter().map(|f| {
            let mut fields = vec![("code", s(f.code.as_str())), ("file", s(&f.file)), ("message", s(&f.message))];
            if let Some(id) = &f.id {
                fields.push(("id", s(id)));
            }
            if let Some(field) = &f.field {
                fields.push(("field", s(field)));
            }
            obj(fields)
        });
        canonical(&obj(vec![
            ("status", s(if self.findings.is_empty() { "ok" } else { "findings" })),
            ("recipes", CValue::Arr(recipes.collect())),
            ("findings", CValue::Arr(findings.collect())),
        ]))
    }
}

/// Reads the catalogue and every recipe under `paths` (files, or folders walked for `*.toml`)
/// and checks them.
pub fn run(catalogue: &Path, paths: &[PathBuf], registry: &Registry) -> Result<Report, InputError> {
    let cat = parse_catalogue(&read_text(catalogue, &show(catalogue))?, &show(catalogue))?;
    let mut inputs = Vec::new();
    for (name, path) in collect_files(paths)? {
        let text = read_text(&path, &name)?;
        inputs.push((name, text));
    }
    check(&cat, &inputs, registry)
}

/// Checks recipe texts given as `(file name, text)`. Inputs are processed in sorted name order;
/// the first unparsable one is an error.
pub fn check(catalogue: &Catalogue, inputs: &[(String, String)], registry: &Registry) -> Result<Report, InputError> {
    let mut sorted: Vec<&(String, String)> = inputs.iter().collect();
    sorted.sort();
    sorted.dedup_by(|a, b| a.0 == b.0);
    let mut parsed = Vec::new();
    for (file, text) in sorted {
        parsed.push((file.as_str(), parse_table(text, file)?));
    }
    let mut findings = Vec::new();
    let mut ids: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut candidates = Vec::new();
    for (file, table) in &parsed {
        let id = table.get("id").and_then(toml::Value::as_str);
        let (kind, mut f) = check_recipe(file, id, table, catalogue, registry);
        if let Some(id) = id {
            ids.entry(id.to_string()).or_default().push(file.to_string());
        }
        if let (true, Some(id), Some(kind)) = (f.is_empty(), id, kind) {
            candidates.push(ValidRecipe { file: file.to_string(), id: id.to_string(), kind });
        }
        findings.append(&mut f);
    }
    // REQ-006: IDs unique across the inputs; every file of a duplicate group is named.
    for (id, files) in &ids {
        if files.len() > 1 {
            for file in files {
                findings.push(Finding {
                    file: file.clone(),
                    id: Some(id.clone()),
                    field: Some("id".to_string()),
                    code: Code::DuplicateId,
                    message: format!("id {id:?} is stated by {}", files.join(", ")),
                });
            }
        }
    }
    findings.sort();
    findings.dedup();
    let flagged: BTreeSet<&str> = findings.iter().map(|f| f.file.as_str()).collect();
    let recipes = candidates.into_iter().filter(|r| !flagged.contains(r.file.as_str())).collect();
    Ok(Report { recipes, findings })
}

/// Checks one parsed recipe; returns its kind when that is a known kind, and its findings.
fn check_recipe(
    file: &str,
    id: Option<&str>,
    t: &toml::Table,
    catalogue: &Catalogue,
    registry: &Registry,
) -> (Option<String>, Vec<Finding>) {
    let kind_text = t.get("kind").and_then(toml::Value::as_str).unwrap_or_default();
    let ctx = RecipeContext { file, id, kind: kind_text, recipe: t };
    let mut out = Vec::new();
    let mut from_kind = Vec::new();
    let mut add = |code: Code, field: &str, message: String| out.push(Finding::new(&ctx, code, Some(field), message));

    // schema, id, title, since_release, machine_profile (REQ-001, REQ-006).
    match t.get("schema") {
        None => add(Code::FieldMissing, "schema", "required".to_string()),
        Some(v) if v.as_str() != Some(SCHEMA) => add(Code::FieldMalformed, "schema", format!("must be {SCHEMA:?}")),
        Some(_) => {}
    }
    match t.get("id") {
        None => add(Code::FieldMissing, "id", "required".to_string()),
        Some(toml::Value::String(s)) if !id_ok(s) => {
            add(Code::IdMalformed, "id", "must match ^[a-z][a-z0-9-]*(\\.[a-z0-9-]+)+$".to_string())
        }
        Some(toml::Value::String(_)) => {}
        Some(_) => add(Code::FieldMalformed, "id", "must be a string".to_string()),
    }
    match t.get("title") {
        None => add(Code::FieldMissing, "title", "required".to_string()),
        Some(v) if v.as_str().map_or(true, |x| x.trim().is_empty()) => {
            add(Code::FieldMalformed, "title", "must be a non-empty string".to_string())
        }
        Some(_) => {}
    }
    match t.get("since_release") {
        None => add(Code::FieldMissing, "since_release", "required".to_string()),
        Some(v) if !v.as_str().is_some_and(is_release_tag) => {
            add(Code::FieldMalformed, "since_release", "must be a release tag".to_string())
        }
        Some(_) => {}
    }
    match t.get("machine_profile") {
        None => add(Code::FieldMissing, "machine_profile", "required".to_string()),
        Some(v) if !v.as_str().is_some_and(|x| MACHINE_PROFILES.contains(&x)) => {
            add(Code::FieldMalformed, "machine_profile", format!("must be one of {}", MACHINE_PROFILES.join(", ")))
        }
        Some(_) => {}
    }

    // dataset (REQ-001, REQ-002).
    match t.get("dataset") {
        None => add(Code::FieldMissing, "dataset", "required".to_string()),
        Some(toml::Value::Table(d)) => {
            for key in d.keys().filter(|k| !matches!(k.as_str(), "id" | "sha256")) {
                add(Code::FieldMalformed, &format!("dataset.{key}"), "unknown key".to_string());
            }
            let ds_id = match d.get("id") {
                None => {
                    add(Code::FieldMissing, "dataset.id", "required".to_string());
                    None
                }
                Some(toml::Value::String(x)) if !x.is_empty() => Some(x.as_str()),
                Some(_) => {
                    add(Code::FieldMalformed, "dataset.id", "must be a non-empty string".to_string());
                    None
                }
            };
            let digest = match d.get("sha256") {
                None => {
                    add(Code::FieldMissing, "dataset.sha256", "required".to_string());
                    None
                }
                Some(v) => {
                    let p = v.as_str().and_then(parse_digest);
                    if p.is_none() {
                        add(Code::FieldMalformed, "dataset.sha256", "must be 64 lowercase hex digits".to_string());
                    }
                    p
                }
            };
            if let Some(ds_id) = ds_id {
                match catalogue.entries.get(ds_id) {
                    None => add(Code::DatasetUnknown, "dataset.id", format!("{ds_id:?} is not in the catalogue")),
                    Some(want) => {
                        if let Some(got) = digest.filter(|g| g != want) {
                            add(
                                Code::DatasetDigestMismatch,
                                "dataset.sha256",
                                format!("{} differs from the catalogue's {}", got.as_str(), want.as_str()),
                            );
                        }
                    }
                }
            }
        }
        Some(_) => add(Code::FieldMalformed, "dataset", "must be a table { id, sha256 }".to_string()),
    }

    // fields (REQ-001, REQ-004).
    let names = check_fields(t.get("fields"), &mut add);

    // source (REQ-003).
    match t.get("source") {
        None => add(Code::FieldMissing, "source", "required".to_string()),
        Some(toml::Value::Table(src)) => check_source(src, &names, &mut add),
        Some(_) => add(Code::SourceInvalid, "source", "must be a table".to_string()),
    }

    // Unknown top-level keys and extra kind sections.
    for key in t.keys() {
        let k = key.as_str();
        if KINDS.contains(&k) && k != kind_text {
            add(Code::FieldMalformed, k, format!("kind section does not match kind {kind_text:?}"));
        } else if !COMMON_KEYS.contains(&k) && !KINDS.contains(&k) {
            add(Code::FieldMalformed, k, "unknown key".to_string());
        }
    }

    // kind and its section (REQ-005).
    let mut kind = None;
    match t.get("kind") {
        None => add(Code::FieldMissing, "kind", "required".to_string()),
        Some(toml::Value::String(k)) => match registry.get(k.as_str()).filter(|_| KINDS.contains(&k.as_str())) {
            None => add(Code::KindUnknown, "kind", format!("{k:?} is not one of {}", KINDS.join(", "))),
            Some(validator) => {
                kind = Some(k.clone());
                match t.get(k.as_str()) {
                    None => add(Code::KindSectionMissing, k.as_str(), format!("kind {k:?} needs a [{k}] section")),
                    Some(toml::Value::Table(section)) => {
                        for mut f in validator.validate(section, &ctx) {
                            f.file = file.to_string();
                            f.id = id.map(str::to_string);
                            from_kind.push(f);
                        }
                    }
                    Some(_) => add(Code::FieldMalformed, k.as_str(), "kind section must be a table".to_string()),
                }
            }
        },
        Some(_) => add(Code::FieldMalformed, "kind", "must be a string".to_string()),
    }
    out.append(&mut from_kind);
    (kind, out)
}

/// Checks the `[[fields]]` array; returns the well-formed, distinct field names.
fn check_fields(v: Option<&toml::Value>, add: &mut impl FnMut(Code, &str, String)) -> Vec<String> {
    let list = match v {
        None => {
            add(Code::FieldMissing, "fields", "at least one [[fields]] entry is required".to_string());
            return Vec::new();
        }
        Some(toml::Value::Array(a)) if !a.is_empty() => a,
        Some(_) => {
            add(Code::FieldMalformed, "fields", "must be a non-empty array of tables".to_string());
            return Vec::new();
        }
    };
    let mut names = Vec::new();
    for (n, entry) in list.iter().enumerate() {
        let Some(e) = entry.as_table() else {
            add(Code::FieldMalformed, &format!("fields.#{}", n + 1), "must be a table".to_string());
            continue;
        };
        let name = e.get("name").and_then(toml::Value::as_str).filter(|x| name_ok(x));
        let label = name.map_or_else(|| format!("#{}", n + 1), str::to_string);
        for key in FIELD_KEYS {
            let value = e.get(key).and_then(toml::Value::as_str);
            let problem = match (key, value) {
                (_, None) if e.contains_key(key) => Some("must be a string".to_string()),
                (_, None) => Some("missing".to_string()),
                ("name", Some(_)) if name.is_none() => Some("must match [A-Za-z0-9_.-]+".to_string()),
                ("unit", Some(u)) if !UNITS.contains(&u) => Some(format!("{u:?} is not a known unit symbol")),
                ("better", Some(b)) if !BETTER.contains(&b) => Some(format!("must be one of {}", BETTER.join(", "))),
                ("tolerance", Some(x)) if !decimal_ok(x) => Some(format!("{x:?} is not a decimal string")),
                ("tolerance_kind", Some(x)) if !TOLERANCE_KINDS.contains(&x) => {
                    Some(format!("must be one of {}", TOLERANCE_KINDS.join(", ")))
                }
                _ => None,
            };
            if let Some(p) = problem {
                add(Code::FieldMalformed, &format!("fields.{label}.{key}"), p);
            }
        }
        for key in e.keys().filter(|k| !FIELD_KEYS.contains(&k.as_str())) {
            add(Code::FieldMalformed, &format!("fields.{label}.{key}"), "unknown key".to_string());
        }
        if let Some(name) = name {
            if names.iter().any(|x| x == name) {
                add(Code::FieldMalformed, &format!("fields.{name}.name"), "field name used twice".to_string());
            } else {
                names.push(name.to_string());
            }
        }
    }
    names
}

/// Checks `[source]` (REQ-003); every problem is `source_invalid`.
fn check_source(src: &toml::Table, fields: &[String], add: &mut impl FnMut(Code, &str, String)) {
    let mut bad = |field: &str, message: String| add(Code::SourceInvalid, field, message);
    let allowed: &[&str] = match src.get("type").and_then(toml::Value::as_str) {
        Some("release_numbers") => {
            for key in ["selector", "journal_selector"] {
                if !src.get(key).and_then(toml::Value::as_str).is_some_and(pointer_ok) {
                    bad(&format!("source.{key}"), "must be a JSON Pointer starting with '/'".to_string());
                }
            }
            &["type", "selector", "journal_selector"]
        }
        Some("command") => {
            match src.get("argv").and_then(toml::Value::as_array) {
                Some(argv) if !argv.is_empty() && argv.iter().all(toml::Value::is_str) => {
                    let program = argv.first().and_then(toml::Value::as_str).unwrap_or_default();
                    if !PROGRAMS.contains(&program) {
                        bad("source.argv", format!("program {program:?} is not fid or fid-verify"));
                    }
                }
                _ => bad("source.argv", "must be a non-empty array of strings".to_string()),
            }
            match src.get("outputs").and_then(toml::Value::as_table) {
                None => bad("source.outputs", "must be a table of JSON Pointers keyed by field name".to_string()),
                Some(outputs) => {
                    for (k, v) in outputs {
                        if !fields.contains(k) {
                            bad(&format!("source.outputs.{k}"), "names no result field".to_string());
                        } else if !v.as_str().is_some_and(pointer_ok) {
                            bad(&format!("source.outputs.{k}"), "must be a JSON Pointer starting with '/'".to_string());
                        }
                    }
                    for f in fields.iter().filter(|f| !outputs.contains_key(f.as_str())) {
                        bad(&format!("source.outputs.{f}"), "result field has no output pointer".to_string());
                    }
                }
            }
            &["type", "argv", "outputs"]
        }
        Some(other) => {
            bad("source.type", format!("{other:?} is not release_numbers or command"));
            return;
        }
        None => {
            bad("source.type", "required: release_numbers or command".to_string());
            return;
        }
    };
    for key in src.keys().filter(|k| !allowed.contains(&k.as_str())) {
        bad(&format!("source.{key}"), "unknown key for this source type".to_string());
    }
}

/// `^[a-z][a-z0-9-]*(\.[a-z0-9-]+)+$` (REQ-006).
pub fn id_ok(s: &str) -> bool {
    let mut parts = s.split('.');
    let head = parts.next().unwrap_or_default();
    let seg = |p: &str| !p.is_empty() && p.bytes().all(|c| matches!(c, b'a'..=b'z' | b'0'..=b'9' | b'-'));
    let mut rest = 0;
    for p in parts {
        if !seg(p) {
            return false;
        }
        rest += 1;
    }
    rest > 0 && head.as_bytes().first().is_some_and(u8::is_ascii_lowercase) && seg(head)
}

fn name_ok(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'-'))
}

/// A non-negative decimal string: digits, optionally `.` and more digits (REQ-004).
pub fn decimal_ok(s: &str) -> bool {
    let digits = |p: &str| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit());
    match s.split_once('.') {
        Some((i, f)) => digits(i) && digits(f),
        None => digits(s),
    }
}

/// A non-empty RFC 6901 JSON Pointer: starts with `/`, and `~` only as `~0` or `~1`.
pub fn pointer_ok(s: &str) -> bool {
    let b = s.as_bytes();
    b.first() == Some(&b'/')
        && b.iter().enumerate().all(|(i, &c)| c != b'~' || matches!(b.get(i + 1), Some(b'0' | b'1')))
}

/// A digest written as `sha256:<hex>` or bare 64 lowercase hex digits.
pub fn parse_digest(s: &str) -> Option<Digest> {
    if s.starts_with("sha256:") {
        Digest::parse(s).ok()
    } else {
        Digest::parse(&format!("sha256:{s}")).ok()
    }
}

/// Path as shown in reports: `/` separators on every platform.
fn show(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// Expands the path arguments: files are taken as given, folders are walked for `*.toml`.
/// Returns `(shown name, path)` sorted by name, without duplicates.
pub fn collect_files(paths: &[PathBuf]) -> Result<Vec<(String, PathBuf)>, InputError> {
    let mut out = BTreeMap::new();
    for p in paths {
        let meta = std::fs::metadata(p).map_err(|e| InputError::Io { file: show(p), message: e.to_string() })?;
        if meta.is_dir() {
            walk(p, 0, &mut out)?;
        } else {
            out.insert(show(p), p.clone());
        }
    }
    Ok(out.into_iter().collect())
}

fn walk(dir: &Path, depth: usize, out: &mut BTreeMap<String, PathBuf>) -> Result<(), InputError> {
    let io = |e: std::io::Error| InputError::Io { file: show(dir), message: e.to_string() };
    if depth > MAX_DEPTH {
        return Err(InputError::Io { file: show(dir), message: "folder nesting too deep".to_string() });
    }
    for entry in std::fs::read_dir(dir).map_err(io)? {
        let path = entry.map_err(io)?.path();
        let meta =
            std::fs::metadata(&path).map_err(|e| InputError::Io { file: show(&path), message: e.to_string() })?;
        if meta.is_dir() {
            walk(&path, depth + 1, out)?;
        } else if path.extension().is_some_and(|x| x == "toml") {
            out.insert(show(&path), path);
        }
    }
    Ok(())
}

fn read_text(path: &Path, name: &str) -> Result<String, InputError> {
    let io = |message: String| InputError::Io { file: name.to_string(), message };
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(MAX_BYTES as u64 + 1).read_to_end(&mut bytes))
        .map_err(|e| io(e.to_string()))?;
    if bytes.len() > MAX_BYTES {
        return Err(io(format!("larger than {MAX_BYTES} bytes")));
    }
    String::from_utf8(bytes)
        .map_err(|_| InputError::Parse { file: name.to_string(), message: "not valid UTF-8".to_string() })
}

fn parse_table(text: &str, file: &str) -> Result<toml::Table, InputError> {
    text.parse::<toml::Table>().map_err(|e| {
        let at = e.span().map(|r| format!(" at byte {}", r.start)).unwrap_or_default();
        InputError::Parse { file: file.to_string(), message: format!("{}{at}", e.message()) }
    })
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

fn obj(fields: Vec<(&str, CValue)>) -> CValue {
    CValue::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect::<BTreeMap<_, _>>())
}

fn canonical(v: &CValue) -> Vec<u8> {
    // Only strings, arrays and objects with unique keys: canonicalisation cannot fail; fall back
    // to a fixed error document rather than panicking.
    to_canonical_bytes(v).unwrap_or_else(|_| b"{\"status\":\"error\"}".to_vec())
}
