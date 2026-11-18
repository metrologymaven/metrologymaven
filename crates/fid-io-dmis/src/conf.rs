//! Generated DMIS conformance table (spec node `IO.DMIS.CONF`).
//!
//! Input: a support-declaration file (TOML, schema `fid.dmis-support` v1) with one
//! `[[statement]]` per DMIS statement keyword, and a test inventory (the output of
//! `cargo test -- --list`). Output: `dmis-conformance.json` (RFC 8785 canonical) and a
//! Markdown table, byte-identical for the same inputs (REQ-IO.DMIS.CONF-004).
//!
//! A `partial` or `full` claim must name at least one test, and every test it names must be
//! in the inventory, else generation fails with `conformance_unproven` (REQ-IO.DMIS.CONF-003).
//! Each conformance module level gets the minimum support over the statements tagged to it,
//! with `none` < `partial` < `full` (REQ-IO.DMIS.CONF-002).
//!
//! The outputs carry only keywords, module designators, levels and test IDs from the
//! declarations; no standards text.

use fid_canon::{to_canonical_bytes, CValue};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

/// Schema name of the declaration file.
pub const DECL_SCHEMA: &str = "fid.dmis-support";
/// Schema name of the generated JSON table.
pub const TABLE_SCHEMA: &str = "fid.dmis-conformance";
/// File name of the generated JSON table.
pub const JSON_FILE: &str = "dmis-conformance.json";
/// File name of the generated Markdown table.
pub const MD_FILE: &str = "dmis-conformance.md";
/// Largest accepted declaration or inventory file, in bytes.
pub const MAX_INPUT_BYTES: usize = 4 * 1024 * 1024;

const MAX_KEYWORD_LEN: usize = 32;
const MAX_MODULE_LEN: usize = 64;
const MAX_TEST_ID_LEN: usize = 256;

/// Declared support for one direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Support {
    None,
    Partial,
    Full,
}

impl Support {
    /// The declaration and output spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Support::None => "none",
            Support::Partial => "partial",
            Support::Full => "full",
        }
    }

    fn parse(s: &str) -> Option<Support> {
        match s {
            "none" => Some(Support::None),
            "partial" => Some(Support::Partial),
            "full" => Some(Support::Full),
            _ => None,
        }
    }
}

/// A conformance module designator and level.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ModuleLevel {
    pub module: String,
    pub level: u8,
}

/// One statement's support declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportDecl {
    pub keyword: String,
    pub import: Support,
    pub export: Support,
    /// Sorted, no duplicates.
    pub modules: Vec<ModuleLevel>,
    /// Sorted, no duplicates.
    pub tests: Vec<String>,
}

/// Aggregated support of one module level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelSupport {
    pub import: Support,
    pub export: Support,
    /// Keywords tagged to this module level, sorted.
    pub statements: Vec<String>,
}

/// The conformance table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    /// Sorted by keyword.
    pub statements: Vec<SupportDecl>,
    pub module_levels: BTreeMap<ModuleLevel, LevelSupport>,
}

/// Generator errors with stable codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfError {
    /// The declaration file is not valid (`malformed`).
    Malformed(String),
    /// A `partial` or `full` claim names no test, or a test missing from the inventory.
    ConformanceUnproven { statement: String },
    /// A file could not be read or written.
    Io(String),
}

impl ConfError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            ConfError::Malformed(_) => "malformed",
            ConfError::ConformanceUnproven { .. } => "conformance_unproven",
            ConfError::Io(_) => "io",
        }
    }
}

impl fmt::Display for ConfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfError::Malformed(m) => write!(f, "malformed: {m}"),
            ConfError::ConformanceUnproven { statement } => write!(f, "conformance_unproven{{statement: {statement}}}"),
            ConfError::Io(m) => write!(f, "io: {m}"),
        }
    }
}

impl std::error::Error for ConfError {}

fn malformed(msg: impl Into<String>) -> ConfError {
    ConfError::Malformed(msg.into())
}

fn valid_keyword(s: &str) -> bool {
    let mut chars = s.chars();
    s.len() <= MAX_KEYWORD_LEN
        && chars.next().is_some_and(|c| c.is_ascii_uppercase())
        && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

fn valid_module(s: &str) -> bool {
    let mut chars = s.chars();
    s.len() <= MAX_MODULE_LEN
        && chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-')
}

fn valid_test_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_TEST_ID_LEN && !s.chars().any(|c| c.is_whitespace() || c == '|' || c == '`')
}

fn get_str<'a>(t: &'a toml::Table, key: &str, ctx: &str) -> Result<&'a str, ConfError> {
    match t.get(key) {
        Some(toml::Value::String(s)) => Ok(s),
        Some(_) => Err(malformed(format!("{ctx}: `{key}` must be a string"))),
        None => Err(malformed(format!("{ctx}: missing `{key}`"))),
    }
}

fn get_support(t: &toml::Table, key: &str, ctx: &str) -> Result<Support, ConfError> {
    let s = get_str(t, key, ctx)?;
    Support::parse(s).ok_or_else(|| malformed(format!("{ctx}: `{key}` must be none, partial or full, not {s:?}")))
}

fn get_array<'a>(t: &'a toml::Table, key: &str, ctx: &str) -> Result<&'a [toml::Value], ConfError> {
    match t.get(key) {
        Some(toml::Value::Array(a)) => Ok(a),
        Some(_) => Err(malformed(format!("{ctx}: `{key}` must be an array"))),
        None => Ok(&[]),
    }
}

fn parse_module(v: &toml::Value, ctx: &str) -> Result<ModuleLevel, ConfError> {
    let toml::Value::Table(t) = v else {
        return Err(malformed(format!("{ctx}: each module entry must be a table {{ module, level }}")));
    };
    for k in t.keys() {
        if k != "module" && k != "level" {
            return Err(malformed(format!("{ctx}: unknown module key `{k}`")));
        }
    }
    let module = get_str(t, "module", ctx)?;
    if !valid_module(module) {
        return Err(malformed(format!("{ctx}: invalid module designator {module:?}")));
    }
    let level = match t.get("level") {
        Some(toml::Value::Integer(i)) => {
            u8::try_from(*i).map_err(|_| malformed(format!("{ctx}: level {i} is outside 0..=255")))?
        }
        Some(_) => return Err(malformed(format!("{ctx}: `level` must be an integer"))),
        None => return Err(malformed(format!("{ctx}: missing `level`"))),
    };
    Ok(ModuleLevel { module: module.to_string(), level })
}

fn parse_statement(v: &toml::Value, index: usize) -> Result<SupportDecl, ConfError> {
    let toml::Value::Table(t) = v else {
        return Err(malformed(format!("statement {index}: must be a table")));
    };
    for k in t.keys() {
        if !matches!(k.as_str(), "keyword" | "import" | "export" | "modules" | "tests") {
            return Err(malformed(format!("statement {index}: unknown key `{k}`")));
        }
    }
    let keyword = get_str(t, "keyword", &format!("statement {index}"))?;
    if !valid_keyword(keyword) {
        return Err(malformed(format!("statement {index}: invalid keyword {keyword:?}")));
    }
    let ctx = format!("statement {keyword}");
    let import = get_support(t, "import", &ctx)?;
    let export = get_support(t, "export", &ctx)?;
    let mut modules = BTreeSet::new();
    for m in get_array(t, "modules", &ctx)? {
        let m = parse_module(m, &ctx)?;
        if !modules.insert(m.clone()) {
            return Err(malformed(format!("{ctx}: duplicate module level {} {}", m.module, m.level)));
        }
    }
    let mut tests = BTreeSet::new();
    for id in get_array(t, "tests", &ctx)? {
        let toml::Value::String(id) = id else {
            return Err(malformed(format!("{ctx}: test IDs must be strings")));
        };
        if !valid_test_id(id) {
            return Err(malformed(format!("{ctx}: invalid test ID {id:?}")));
        }
        if !tests.insert(id.clone()) {
            return Err(malformed(format!("{ctx}: duplicate test ID {id:?}")));
        }
    }
    Ok(SupportDecl {
        keyword: keyword.to_string(),
        import,
        export,
        modules: modules.into_iter().collect(),
        tests: tests.into_iter().collect(),
    })
}

/// Parses a declaration file; the result is sorted by keyword.
pub fn parse_declarations(text: &str) -> Result<Vec<SupportDecl>, ConfError> {
    if text.len() > MAX_INPUT_BYTES {
        return Err(malformed("declaration file is too large"));
    }
    let root: toml::Table = text.parse().map_err(|e: toml::de::Error| malformed(format!("not TOML: {}", e.message())))?;
    for k in root.keys() {
        if !matches!(k.as_str(), "schema" | "version" | "statement") {
            return Err(malformed(format!("unknown top-level key `{k}`")));
        }
    }
    let schema = get_str(&root, "schema", "declaration file")?;
    if schema != DECL_SCHEMA {
        return Err(malformed(format!("schema must be {DECL_SCHEMA:?}, not {schema:?}")));
    }
    match root.get("version") {
        Some(toml::Value::Integer(1)) => {}
        _ => return Err(malformed("version must be the integer 1")),
    }
    let mut by_keyword: BTreeMap<String, SupportDecl> = BTreeMap::new();
    for (i, v) in get_array(&root, "statement", "declaration file")?.iter().enumerate() {
        let d = parse_statement(v, i)?;
        if by_keyword.contains_key(&d.keyword) {
            return Err(malformed(format!("duplicate statement {}", d.keyword)));
        }
        by_keyword.insert(d.keyword.clone(), d);
    }
    Ok(by_keyword.into_values().collect())
}

/// Reads a test inventory. Lines `<id>: test` (the `cargo test -- --list` format) give `<id>`;
/// a line without whitespace is an ID on its own; every other line is ignored.
pub fn parse_inventory(text: &str) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(id) = line.strip_suffix(": test") {
            if !id.is_empty() {
                ids.insert(id.to_string());
            }
        } else if !line.is_empty() && !line.chars().any(char::is_whitespace) {
            ids.insert(line.to_string());
        }
    }
    ids
}

/// Checks that every `partial` or `full` claim names at least one test and only tests in the
/// inventory. Declarations are checked in keyword order, so the first failure is stable.
pub fn validate(decls: &[SupportDecl], inventory: &BTreeSet<String>) -> Result<(), ConfError> {
    for d in decls {
        let claims = d.import != Support::None || d.export != Support::None;
        if claims && (d.tests.is_empty() || d.tests.iter().any(|t| !inventory.contains(t))) {
            return Err(ConfError::ConformanceUnproven { statement: d.keyword.clone() });
        }
    }
    Ok(())
}

/// Validates the declarations and aggregates them per module level (minimum over statements).
pub fn build(decls: Vec<SupportDecl>, inventory: &BTreeSet<String>) -> Result<Table, ConfError> {
    validate(&decls, inventory)?;
    let mut module_levels: BTreeMap<ModuleLevel, LevelSupport> = BTreeMap::new();
    for d in &decls {
        for m in &d.modules {
            let e = module_levels.entry(m.clone()).or_insert(LevelSupport {
                import: Support::Full,
                export: Support::Full,
                statements: Vec::new(),
            });
            e.import = e.import.min(d.import);
            e.export = e.export.min(d.export);
            e.statements.push(d.keyword.clone());
        }
    }
    Ok(Table { statements: decls, module_levels })
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

fn obj(pairs: Vec<(&str, CValue)>) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn module_obj(m: &ModuleLevel) -> CValue {
    obj(vec![("module", s(&m.module)), ("level", CValue::Num(f64::from(m.level)))])
}

impl Table {
    /// The canonical JSON table (RFC 8785, no trailing newline).
    pub fn to_json(&self) -> Result<Vec<u8>, ConfError> {
        let levels: Vec<CValue> = self
            .module_levels
            .iter()
            .map(|(m, l)| {
                obj(vec![
                    ("module", s(&m.module)),
                    ("level", CValue::Num(f64::from(m.level))),
                    ("import", s(l.import.as_str())),
                    ("export", s(l.export.as_str())),
                    ("statements", CValue::Arr(l.statements.iter().map(|k| s(k)).collect())),
                ])
            })
            .collect();
        let statements: Vec<CValue> = self
            .statements
            .iter()
            .map(|d| {
                obj(vec![
                    ("keyword", s(&d.keyword)),
                    ("import", s(d.import.as_str())),
                    ("export", s(d.export.as_str())),
                    ("module_levels", CValue::Arr(d.modules.iter().map(module_obj).collect())),
                    ("tests", CValue::Arr(d.tests.iter().map(|t| s(t)).collect())),
                ])
            })
            .collect();
        let root = obj(vec![
            ("schema", s(TABLE_SCHEMA)),
            ("version", CValue::Num(1.0)),
            ("standard", s("DMIS 5.3")),
            ("module_levels", CValue::Arr(levels)),
            ("statements", CValue::Arr(statements)),
        ]);
        to_canonical_bytes(&root).map_err(|e| ConfError::Io(format!("canonical JSON: {e}")))
    }

    /// The Markdown table (LF line endings, one trailing newline).
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str("<!-- Generated by dmis-conformance (spec node IO.DMIS.CONF). Do not edit by hand. -->\n");
        out.push_str("# DMIS 5.3 conformance table\n\n");
        out.push_str("Support is `none`, `partial` or `full` per direction. A module level is `full` only if ");
        out.push_str("every statement tagged to it is `full`; every `partial` or `full` claim names a test.\n\n");
        out.push_str("## Conformance module levels\n\n");
        if self.module_levels.is_empty() {
            out.push_str("_No statement is tagged to a conformance module level yet._\n\n");
        } else {
            out.push_str("| Module | Level | Import | Export | Statements |\n");
            out.push_str("| --- | --- | --- | --- | --- |\n");
            for (m, l) in &self.module_levels {
                out.push_str(&format!(
                    "| {} | {} | {} | {} | {} |\n",
                    m.module,
                    m.level,
                    l.import.as_str(),
                    l.export.as_str(),
                    l.statements.join(", ")
                ));
            }
            out.push('\n');
        }
        out.push_str("## Statements\n\n");
        if self.statements.is_empty() {
            out.push_str("_No statements are declared._\n");
        } else {
            out.push_str("| Statement | Import | Export | Module levels | Tests |\n");
            out.push_str("| --- | --- | --- | --- | --- |\n");
            for d in &self.statements {
                let modules = if d.modules.is_empty() {
                    "unassigned".to_string()
                } else {
                    d.modules.iter().map(|m| format!("{} level {}", m.module, m.level)).collect::<Vec<_>>().join(", ")
                };
                let tests = if d.tests.is_empty() {
                    "none".to_string()
                } else {
                    d.tests.iter().map(|t| format!("`{t}`")).collect::<Vec<_>>().join(", ")
                };
                out.push_str(&format!(
                    "| {} | {} | {} | {} | {} |\n",
                    d.keyword,
                    d.import.as_str(),
                    d.export.as_str(),
                    modules,
                    tests
                ));
            }
        }
        out
    }
}

/// Both generated files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outputs {
    pub json: Vec<u8>,
    pub markdown: String,
}

/// Generates both outputs from declaration and inventory text.
pub fn generate(decl_text: &str, inventory_text: &str) -> Result<Outputs, ConfError> {
    if inventory_text.len() > MAX_INPUT_BYTES {
        return Err(malformed("test inventory is too large"));
    }
    let table = build(parse_declarations(decl_text)?, &parse_inventory(inventory_text))?;
    Ok(Outputs { json: table.to_json()?, markdown: table.to_markdown() })
}

fn read(path: &Path) -> Result<String, ConfError> {
    std::fs::read_to_string(path).map_err(|e| ConfError::Io(format!("{}: {e}", path.display())))
}

/// Reads the inputs and generates both outputs.
pub fn generate_files(decl: &Path, inventory: &Path) -> Result<Outputs, ConfError> {
    generate(&read(decl)?, &read(inventory)?)
}

/// Writes both outputs into `dir` (created if missing).
pub fn write_outputs(out: &Outputs, dir: &Path) -> Result<(), ConfError> {
    let io = |e: std::io::Error| ConfError::Io(format!("{}: {e}", dir.display()));
    std::fs::create_dir_all(dir).map_err(io)?;
    std::fs::write(dir.join(JSON_FILE), &out.json).map_err(io)?;
    std::fs::write(dir.join(MD_FILE), out.markdown.as_bytes()).map_err(io)?;
    Ok(())
}

/// Names of the files in `dir` that are missing or differ from `out` (empty when fresh).
pub fn stale_files(out: &Outputs, dir: &Path) -> Vec<&'static str> {
    let mut stale = Vec::new();
    for (name, want) in [(JSON_FILE, out.json.as_slice()), (MD_FILE, out.markdown.as_bytes())] {
        match std::fs::read(dir.join(name)) {
            Ok(got) if got == want => {}
            _ => stale.push(name),
        }
    }
    stale
}
