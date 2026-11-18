//! Metrology Maven per-user install layout and allowed write locations (spec node
//! `PLAT.DEPLOY.NOADMIN.LAYOUT`): the typed schema of `platform/peruser-layout.toml`, its strict
//! validator, the cross-check against the support matrix, template expansion with an injected
//! environment, and the write check [`is_allowed_write`].
//!
//! Determinism D0: pure functions over bytes and injected values. Nothing here reads the process
//! environment or touches the file system, except [`process_env`], which only reads environment
//! variables and is a convenience for callers. Tables are `BTreeMap`-backed (the `toml` crate
//! without `preserve_order`), so diagnostics have a fixed order (see [`validate`]).
//!
//! # File format
//!
//! ```toml
//! schema_version = 1
//! [os.windows]                  # a support-matrix OS family
//! provisional = true            # optional: the paths are a proposal, not yet audited
//! install_dir = "{local_app_data}/Programs/Fiducial"
//! allowed_writes = ["{local_app_data}/Fiducial", "{tmp}", "{project_roots}", "{export_roots}"]
//! forbidden_registrations = ["service", "driver", "machine_assoc", "scheduled_task"]
//! ```
//!
//! # Templates
//!
//! A template is one variable (see [`Var`]) followed by zero or more literal segments joined with
//! `/`, for example `{home}/Library/Caches/Fiducial`. Rules, checked without any environment:
//! - A template that does not start with a variable (an absolute or relative literal path, such as
//!   `C:/Program Files/Fiducial`) or that has a `..` segment is `OutsideProfile`
//!   (REQ-PLAT.DEPLOY.NOADMIN.LAYOUT-004).
//! - A variable name that is not one of the nine, or not defined on the OS family (the `xdg_*`
//!   variables exist only on Linux, `local_app_data` and `app_data` only on Windows), is
//!   `UnknownVariable`.
//! - `install_dir` must start with `{local_app_data}` on Windows and `{home}` elsewhere, and name
//!   a subdirectory (REQ-PLAT.DEPLOY.NOADMIN.LAYOUT-001); another variable is `OutsideProfile`.
//! - `allowed_writes` entries must start with a per-user data, cache or configuration directory
//!   (Windows `{local_app_data}`, `{app_data}`; macOS `{home}`, for `~/Library/...`; Linux the
//!   three `{xdg_*}`), each followed by an application subdirectory, or with `{tmp}`,
//!   `{project_roots}` or `{export_roots}` (REQ-PLAT.DEPLOY.NOADMIN.LAYOUT-002).
//! - Variables are allowed only as the first segment; segments must be non-empty, not `.`, and
//!   free of `\`, `:`, `{`, `}` and control characters (`BadTemplate` otherwise).
//!
//! [`PeruserLayout::expand`] checks the expanded paths again: every one must lie below `{home}`,
//! `{tmp}`, a project root or an export root, so an `XDG_DATA_HOME` pointing outside the profile
//! fails with `OutsideProfile` at expansion time.
//!
//! # Paths
//!
//! [`NormPath`] parses paths for one [`PathFlavor`] independently of the host OS, so Windows
//! rules are testable on Linux and the reverse. Normalisation turns `\` into `/` (Windows only),
//! drops empty and `.` segments and rejects `..` segments (never resolves them). Windows paths
//! must be drive-absolute (`C:\...`) or UNC (`\\server\share\...`); drive-relative (`C:x`),
//! verbatim (`\\?\`, `\\.\`) paths, segments with `:` (alternate data streams) and segments that
//! end in `.` or a space (which Windows silently strips) are rejected. POSIX paths must start
//! with `/`. Comparison is segment-wise, case-insensitive (Unicode lower case) on Windows and
//! macOS and case-sensitive on Linux (REQ-PLAT.DEPLOY.NOADMIN.LAYOUT-005). macOS Unicode
//! normalisation (NFD) is not applied.
//!
//! # Environment
//!
//! [`Env`] supplies variable values; [`MapEnv`] is a plain map for tests and callers that read
//! known folders themselves. [`resolve_env`] derives the values from raw environment variables
//! through an injected lookup (see its documentation for the per-OS rules and fallbacks).

use crate::matrix::{Matrix, OsFamily, Phase, Tier};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// The only `schema_version` this reader supports.
pub const SUPPORTED_SCHEMA: u32 = 1;
/// Largest accepted file, in bytes (64 KiB).
pub const MAX_BYTES: usize = 64 * 1024;
/// Path of the shipped layout file, relative to the repository root.
pub const DEFAULT_PATH: &str = "platform/peruser-layout.toml";
/// Path of the shipped support matrix, relative to the repository root.
pub const MATRIX_PATH: &str = "platform/support-matrix.toml";
/// Phase used by `plat-deploy validate-layout` when `--phase` is not given (the node's phase).
pub const DEFAULT_PHASE: Phase = Phase::P1a;

const TOP_KEYS: [&str; 2] = ["os", "schema_version"];
const OS_KEYS: [&str; 4] = ["allowed_writes", "forbidden_registrations", "install_dir", "provisional"];

/// How paths are written and compared on an OS family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PathFlavor {
    /// Drive or UNC paths, `\` or `/` separators, case-insensitive.
    Windows,
    /// POSIX paths, case-insensitive (the default APFS volume format).
    Macos,
    /// POSIX paths, case-sensitive.
    Linux,
}

impl PathFlavor {
    /// The flavor of a support-matrix OS family; every Linux distribution is `Linux`.
    pub fn of(os: OsFamily) -> PathFlavor {
        match os {
            OsFamily::Windows => PathFlavor::Windows,
            OsFamily::Macos => PathFlavor::Macos,
            OsFamily::Ubuntu | OsFamily::Debian | OsFamily::Fedora | OsFamily::Rhel => PathFlavor::Linux,
        }
    }

    /// Whether path segments compare case-insensitively.
    pub fn case_insensitive(self) -> bool {
        !matches!(self, PathFlavor::Linux)
    }

    /// Variables defined on this flavor.
    pub fn variables(self) -> &'static [Var] {
        use Var::*;
        match self {
            PathFlavor::Windows => &[Home, LocalAppData, AppData, Tmp, ProjectRoots, ExportRoots],
            PathFlavor::Macos => &[Home, Tmp, ProjectRoots, ExportRoots],
            PathFlavor::Linux => &[Home, XdgData, XdgCache, XdgConfig, Tmp, ProjectRoots, ExportRoots],
        }
    }

    /// Variables an `allowed_writes` template may start with.
    pub fn write_roots(self) -> &'static [Var] {
        use Var::*;
        match self {
            PathFlavor::Windows => &[LocalAppData, AppData, Tmp, ProjectRoots, ExportRoots],
            PathFlavor::Macos => &[Home, Tmp, ProjectRoots, ExportRoots],
            PathFlavor::Linux => &[XdgData, XdgCache, XdgConfig, Tmp, ProjectRoots, ExportRoots],
        }
    }

    /// The variable `install_dir` must start with.
    pub fn install_root(self) -> Var {
        match self {
            PathFlavor::Windows => Var::LocalAppData,
            PathFlavor::Macos | PathFlavor::Linux => Var::Home,
        }
    }
}

/// A template variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Var {
    Home,
    LocalAppData,
    AppData,
    XdgData,
    XdgCache,
    XdgConfig,
    Tmp,
    ProjectRoots,
    ExportRoots,
}

impl Var {
    /// Every variable, in declaration order.
    pub const ALL: [Var; 9] = [
        Var::Home,
        Var::LocalAppData,
        Var::AppData,
        Var::XdgData,
        Var::XdgCache,
        Var::XdgConfig,
        Var::Tmp,
        Var::ProjectRoots,
        Var::ExportRoots,
    ];

    /// The name used in templates (without braces).
    pub fn as_str(self) -> &'static str {
        match self {
            Var::Home => "home",
            Var::LocalAppData => "local_app_data",
            Var::AppData => "app_data",
            Var::XdgData => "xdg_data",
            Var::XdgCache => "xdg_cache",
            Var::XdgConfig => "xdg_config",
            Var::Tmp => "tmp",
            Var::ProjectRoots => "project_roots",
            Var::ExportRoots => "export_roots",
        }
    }

    /// Parses a template variable name (without braces).
    pub fn from_name(s: &str) -> Option<Var> {
        Var::ALL.iter().copied().find(|v| v.as_str() == s)
    }

    /// Whether the variable stands for a list of paths (the configured roots).
    pub fn is_list(self) -> bool {
        matches!(self, Var::ProjectRoots | Var::ExportRoots)
    }

    /// Whether a template rooted here must name a subdirectory (everything except `tmp` and the
    /// configured roots, which are write locations in their own right).
    fn needs_subdir(self) -> bool {
        !matches!(self, Var::Tmp | Var::ProjectRoots | Var::ExportRoots)
    }
}

impl fmt::Display for Var {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An OS registration a per-user install must not make (REQ-PLAT.DEPLOY.NOADMIN.LAYOUT-003).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Registration {
    Service,
    Driver,
    MachineAssoc,
    ScheduledTask,
}

impl Registration {
    /// Every registration, in declaration order.
    pub const ALL: [Registration; 4] =
        [Registration::Service, Registration::Driver, Registration::MachineAssoc, Registration::ScheduledTask];

    /// The OS-neutral name used in the file.
    pub fn as_str(self) -> &'static str {
        match self {
            Registration::Service => "service",
            Registration::Driver => "driver",
            Registration::MachineAssoc => "machine_assoc",
            Registration::ScheduledTask => "scheduled_task",
        }
    }

    /// Parses the name used in the file.
    pub fn from_name(s: &str) -> Option<Registration> {
        Registration::ALL.iter().copied().find(|r| r.as_str() == s)
    }
}

impl fmt::Display for Registration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A validated template: a variable and literal segments below it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    pub var: Var,
    pub segments: Vec<String>,
}

impl fmt::Display for Template {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{{{}}}", self.var)?;
        for s in &self.segments {
            write!(f, "/{s}")?;
        }
        Ok(())
    }
}

/// The layout of one OS family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsLayout {
    pub install_dir: Template,
    pub allowed_writes: Vec<Template>,
    /// In [`Registration::ALL`] order; always all four in a validated file.
    pub forbidden_registrations: Vec<Registration>,
    /// The paths are a proposal not yet confirmed by `PLAT.DEPLOY.NOADMIN.AUDIT`.
    pub provisional: bool,
}

/// A validated layout file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeruserLayout {
    pub schema_version: u32,
    pub os: BTreeMap<OsFamily, OsLayout>,
}

/// Which template of an OS family a diagnostic is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Entry {
    InstallDir,
    /// 0-based position in `allowed_writes`.
    AllowedWrite(usize),
}

impl fmt::Display for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Entry::InstallDir => f.write_str("install_dir"),
            Entry::AllowedWrite(i) => write!(f, "allowed_writes[{i}]"),
        }
    }
}

/// A typed validation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutError {
    /// Fatal: the file exceeds [`MAX_BYTES`].
    TooLarge { found: usize, max: usize },
    /// Fatal: `schema_version` is above [`SUPPORTED_SCHEMA`].
    UnsupportedSchema { found: u64 },
    /// Fatal: not UTF-8 or not TOML. `line` is 1-based when the parser reports a position.
    Parse { line: Option<usize>, msg: String },
    /// A structural problem: an unknown key, a wrong type, a missing field, an unknown or
    /// duplicate registration name. `field` is a dotted key path such as `os.windows.install_dir`.
    BadField { field: String, msg: String },
    /// A template names a variable that does not exist or is not defined on `os`.
    UnknownVariable { os: OsFamily, entry: Entry, name: String },
    /// A template is malformed or starts with a variable not allowed for its entry.
    BadTemplate { os: OsFamily, entry: Entry, msg: String },
    /// A template can expand outside `{home}`, `{tmp}`, the project roots and the export roots,
    /// or (`install_dir`) outside the per-user install root (REQ-PLAT.DEPLOY.NOADMIN.LAYOUT-001, -004).
    OutsideProfile { os: OsFamily, entry: Entry },
    /// `forbidden_registrations` of `os` omits a registration (REQ-PLAT.DEPLOY.NOADMIN.LAYOUT-003).
    MissingRegistration { os: OsFamily, registration: Registration },
    /// An OS family with an effective Tier 1 row has no layout (REQ-PLAT.DEPLOY.NOADMIN.LAYOUT-001).
    MissingOs { os: OsFamily },
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LayoutError::TooLarge { found, max } => write!(f, "TooLarge: {found} bytes exceeds the limit of {max}"),
            LayoutError::UnsupportedSchema { found } => {
                write!(f, "UnsupportedSchema: schema_version {found} is above the supported version {SUPPORTED_SCHEMA}")
            }
            LayoutError::Parse { line: Some(l), msg } => write!(f, "Parse: line {l}: {msg}"),
            LayoutError::Parse { line: None, msg } => write!(f, "Parse: {msg}"),
            LayoutError::BadField { field, msg } => write!(f, "BadField: {field} {msg}"),
            LayoutError::UnknownVariable { os, entry, name } => {
                write!(f, "UnknownVariable {{ os: {os}, entry: {entry}, name: {name} }}")
            }
            LayoutError::BadTemplate { os, entry, msg } => {
                write!(f, "BadTemplate {{ os: {os}, entry: {entry} }}: {msg}")
            }
            LayoutError::OutsideProfile { os, entry } => write!(f, "OutsideProfile {{ os: {os}, entry: {entry} }}"),
            LayoutError::MissingRegistration { os, registration } => {
                write!(f, "MissingRegistration {{ os: {os}, registration: {registration} }}")
            }
            LayoutError::MissingOs { os } => write!(f, "MissingOs {{ os: {os} }}"),
        }
    }
}

impl std::error::Error for LayoutError {}

/// The result of validating a layout file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Validation {
    /// Number of `[os.*]` tables (0 when a fatal error stopped parsing).
    pub families: usize,
    /// Diagnostics in the documented order. A fatal error is the only entry.
    pub errors: Vec<LayoutError>,
    /// The validated file, present only when `errors` is empty.
    pub layout: Option<PeruserLayout>,
}

impl Validation {
    /// One diagnostic per line, then `layout: <families> os, <errors> errors`. Byte-identical
    /// for identical input.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for e in &self.errors {
            out.push_str(&e.to_string());
            out.push('\n');
        }
        out.push_str(&format!("layout: {} os, {} errors\n", self.families, self.errors.len()));
        out
    }

    fn fatal(e: LayoutError) -> Validation {
        Validation { families: 0, errors: vec![e], layout: None }
    }
}

fn bad(field: String, msg: impl Into<String>) -> LayoutError {
    LayoutError::BadField { field, msg: msg.into() }
}

fn line_of(text: &str, offset: usize) -> usize {
    text.as_bytes().iter().take(offset).filter(|&&b| b == b'\n').count() + 1
}

/// Validates a layout file (REQ-PLAT.DEPLOY.NOADMIN.LAYOUT-001 to -004 without the support
/// matrix; the matrix cross-check is [`check_coverage`]). Fatal errors (`TooLarge`,
/// `UnsupportedSchema`, `Parse`) stop at once.
///
/// Order: top-level field errors (`schema_version`, unknown keys, `os`); then per OS family in
/// name order: unknown keys, `install_dir`, `allowed_writes` in list order,
/// `forbidden_registrations` (field errors in list order, then `MissingRegistration` in
/// [`Registration::ALL`] order), `provisional`.
pub fn validate(bytes: &[u8]) -> Validation {
    if bytes.len() > MAX_BYTES {
        return Validation::fatal(LayoutError::TooLarge { found: bytes.len(), max: MAX_BYTES });
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Validation::fatal(LayoutError::Parse { line: None, msg: "file is not UTF-8".into() });
    };
    let table = match text.parse::<toml::Table>() {
        Ok(t) => t,
        Err(e) => {
            let line = e.span().map(|s| line_of(text, s.start));
            return Validation::fatal(LayoutError::Parse { line, msg: e.message().trim_end().to_string() });
        }
    };
    let mut errors = Vec::new();
    match table.get("schema_version") {
        None => errors.push(bad("schema_version".into(), "is missing")),
        Some(toml::Value::Integer(n)) if *n >= 1 => {
            let n = n.unsigned_abs();
            if n > u64::from(SUPPORTED_SCHEMA) {
                return Validation::fatal(LayoutError::UnsupportedSchema { found: n });
            }
        }
        Some(_) => errors.push(bad("schema_version".into(), "is not a positive integer")),
    }
    for k in table.keys() {
        if !TOP_KEYS.contains(&k.as_str()) {
            errors.push(bad(k.clone(), "is not a known key"));
        }
    }
    let mut families = 0;
    let mut os = BTreeMap::new();
    match table.get("os") {
        None => errors.push(bad("os".into(), "is missing")),
        Some(toml::Value::Table(t)) => {
            families = t.len();
            for (name, value) in t {
                let field = format!("os.{name}");
                let Some(family) = OsFamily::from_name(name) else {
                    errors.push(bad(field, "is not an OS family of the support matrix"));
                    continue;
                };
                let toml::Value::Table(ot) = value else {
                    errors.push(bad(field, "is not a table"));
                    continue;
                };
                if let Some(l) = check_os(family, ot, &field, &mut errors) {
                    os.insert(family, l);
                }
            }
        }
        Some(_) => errors.push(bad("os".into(), "is not a table")),
    }
    let layout = errors.is_empty().then_some(PeruserLayout { schema_version: SUPPORTED_SCHEMA, os });
    Validation { families, errors, layout }
}

/// Parses and validates; `Err` holds every diagnostic.
pub fn parse(bytes: &[u8]) -> Result<PeruserLayout, Vec<LayoutError>> {
    let v = validate(bytes);
    v.layout.ok_or(v.errors)
}

fn check_os(os: OsFamily, t: &toml::Table, field: &str, errors: &mut Vec<LayoutError>) -> Option<OsLayout> {
    let before = errors.len();
    for k in t.keys() {
        if !OS_KEYS.contains(&k.as_str()) {
            errors.push(bad(format!("{field}.{k}"), "is not a known key"));
        }
    }
    let install_dir = match t.get("install_dir") {
        None => {
            errors.push(bad(format!("{field}.install_dir"), "is missing"));
            None
        }
        Some(toml::Value::String(s)) => push_err(template(s, os, Entry::InstallDir), errors),
        Some(_) => {
            errors.push(bad(format!("{field}.install_dir"), "is not a string"));
            None
        }
    };
    let mut allowed_writes = Vec::new();
    match t.get("allowed_writes") {
        None => errors.push(bad(format!("{field}.allowed_writes"), "is missing")),
        Some(toml::Value::Array(a)) if a.is_empty() => {
            errors.push(bad(format!("{field}.allowed_writes"), "is empty"));
        }
        Some(toml::Value::Array(a)) => {
            for (i, v) in a.iter().enumerate() {
                let entry = Entry::AllowedWrite(i);
                let toml::Value::String(s) = v else {
                    errors.push(bad(format!("{field}.allowed_writes[{i}]"), "is not a string"));
                    continue;
                };
                if let Some(tpl) = push_err(template(s, os, entry), errors) {
                    allowed_writes.push(tpl);
                }
            }
        }
        Some(_) => errors.push(bad(format!("{field}.allowed_writes"), "is not an array of strings")),
    }
    let mut regs = BTreeSet::new();
    match t.get("forbidden_registrations") {
        None => errors.push(bad(format!("{field}.forbidden_registrations"), "is missing")),
        Some(toml::Value::Array(a)) => {
            for (i, v) in a.iter().enumerate() {
                let f = format!("{field}.forbidden_registrations[{i}]");
                match v {
                    toml::Value::String(s) => match Registration::from_name(s) {
                        Some(r) => {
                            if !regs.insert(r) {
                                errors.push(bad(f, format!("repeats {r}")));
                            }
                        }
                        None => errors.push(bad(f, "is not one of service, driver, machine_assoc, scheduled_task")),
                    },
                    _ => errors.push(bad(f, "is not a string")),
                }
            }
            for r in Registration::ALL {
                if !regs.contains(&r) {
                    errors.push(LayoutError::MissingRegistration { os, registration: r });
                }
            }
        }
        Some(_) => errors.push(bad(format!("{field}.forbidden_registrations"), "is not an array of strings")),
    }
    let provisional = match t.get("provisional") {
        None => false,
        Some(toml::Value::Boolean(b)) => *b,
        Some(_) => {
            errors.push(bad(format!("{field}.provisional"), "is not a boolean"));
            false
        }
    };
    let install_dir = install_dir?;
    (errors.len() == before).then(|| OsLayout {
        install_dir,
        allowed_writes,
        forbidden_registrations: regs.into_iter().collect(),
        provisional,
    })
}

fn push_err<T>(r: Result<T, LayoutError>, errors: &mut Vec<LayoutError>) -> Option<T> {
    match r {
        Ok(v) => Some(v),
        Err(e) => {
            errors.push(e);
            None
        }
    }
}

/// Why a template was rejected, before the OS and entry are attached.
enum TplProblem {
    Outside,
    Unknown(String),
    Bad(String),
}

impl TplProblem {
    fn into_error(self, os: OsFamily, entry: Entry) -> LayoutError {
        match self {
            TplProblem::Outside => LayoutError::OutsideProfile { os, entry },
            TplProblem::Unknown(name) => LayoutError::UnknownVariable { os, entry, name },
            TplProblem::Bad(msg) => LayoutError::BadTemplate { os, entry, msg },
        }
    }
}

fn template(text: &str, os: OsFamily, entry: Entry) -> Result<Template, LayoutError> {
    parse_template(text, PathFlavor::of(os), entry).map_err(|p| p.into_error(os, entry))
}

fn parse_template(text: &str, flavor: PathFlavor, entry: Entry) -> Result<Template, TplProblem> {
    if text.is_empty() {
        return Err(TplProblem::Bad("is empty".into()));
    }
    let Some(inner) = text.strip_prefix('{') else {
        // A literal path is not anchored in a per-user location.
        return Err(TplProblem::Outside);
    };
    let Some(close) = inner.find('}') else {
        return Err(TplProblem::Bad("has an unclosed {".into()));
    };
    let name = &inner[..close];
    let var = match Var::from_name(name) {
        Some(v) if flavor.variables().contains(&v) => v,
        _ => return Err(TplProblem::Unknown(name.to_string())),
    };
    let rest = &inner[close + 1..];
    let mut segments = Vec::new();
    if !rest.is_empty() {
        let Some(rest) = rest.strip_prefix('/') else {
            return Err(TplProblem::Bad(format!("{{{var}}} must be followed by / or the end")));
        };
        for seg in rest.split('/') {
            if seg == ".." {
                return Err(TplProblem::Outside);
            }
            if seg.is_empty() || seg == "." {
                return Err(TplProblem::Bad("has an empty or . segment".into()));
            }
            if seg.contains(['{', '}']) {
                return Err(TplProblem::Bad("uses a variable after the first segment".into()));
            }
            if seg.chars().any(|c| c == '\\' || c == ':' || c.is_control()) {
                return Err(TplProblem::Bad(format!("segment {seg:?} contains \\, : or a control character")));
            }
            segments.push(seg.to_string());
        }
    }
    match entry {
        Entry::InstallDir => {
            if var != flavor.install_root() {
                return Err(TplProblem::Outside);
            }
        }
        Entry::AllowedWrite(_) => {
            if !flavor.write_roots().contains(&var) {
                return Err(TplProblem::Bad(format!(
                    "{{{var}}} is not a per-user data, cache or configuration directory, tmp or a configured root here"
                )));
            }
        }
    }
    if segments.is_empty() && (entry == Entry::InstallDir || var.needs_subdir()) {
        return Err(TplProblem::Bad(format!("must name a subdirectory of {{{var}}}")));
    }
    Ok(Template { var, segments })
}

/// OS families that need a layout: those with a Tier 1 row effective at `phase`
/// (`since_phase <= phase`). Support-matrix schema 1 has no desktop/server field, so every
/// Tier 1 row counts as a desktop row.
pub fn required_families(matrix: &Matrix, phase: Phase) -> BTreeSet<OsFamily> {
    matrix.rows.iter().filter(|r| r.tier == Tier::Tier1 && phase >= r.since_phase).map(|r| r.os).collect()
}

/// `MissingOs` for every required family (see [`required_families`]) without a layout, in
/// [`OsFamily::ALL`] order (REQ-PLAT.DEPLOY.NOADMIN.LAYOUT-001).
pub fn check_coverage(layout: &PeruserLayout, matrix: &Matrix, phase: Phase) -> Vec<LayoutError> {
    required_families(matrix, phase)
        .into_iter()
        .filter(|os| !layout.os.contains_key(os))
        .map(|os| LayoutError::MissingOs { os })
        .collect()
}

// ------------------------------------------------------------------------------------- paths

/// Why a path or environment value was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathProblem {
    Empty,
    /// Not absolute for the flavor (includes Windows drive-relative `C:x` and malformed UNC).
    Relative,
    /// Has a `..` segment.
    DotDot,
    /// NUL, a `:` outside the drive designator (Windows), or a segment ending in `.` or space
    /// (Windows).
    BadChar,
    /// A Windows verbatim or device path (`\\?\`, `\\.\`).
    Verbatim,
    /// A file-system or drive root where a directory below it is required.
    RootOnly,
}

impl fmt::Display for PathProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            PathProblem::Empty => "empty",
            PathProblem::Relative => "relative",
            PathProblem::DotDot => "dot_dot",
            PathProblem::BadChar => "bad_char",
            PathProblem::Verbatim => "verbatim",
            PathProblem::RootOnly => "root_only",
        })
    }
}

/// A normalised absolute path of one flavor (see the module documentation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormPath {
    flavor: PathFlavor,
    /// Windows: `C:` (upper-case letter) or `//server/share`; empty for POSIX.
    prefix: String,
    segments: Vec<String>,
}

impl NormPath {
    /// Parses and normalises `text` as an absolute path of `flavor`.
    pub fn parse(text: &str, flavor: PathFlavor) -> Result<NormPath, PathProblem> {
        if text.is_empty() {
            return Err(PathProblem::Empty);
        }
        if text.contains('\0') {
            return Err(PathProblem::BadChar);
        }
        let windows = flavor == PathFlavor::Windows;
        let s = if windows { text.replace('\\', "/") } else { text.to_string() };
        let (prefix, rest) = if windows {
            if s.starts_with("//?/") || s.starts_with("//./") {
                return Err(PathProblem::Verbatim);
            }
            if let Some(r) = s.strip_prefix("//") {
                let mut it = r.splitn(3, '/');
                let server = it.next().unwrap_or("");
                let share = it.next().unwrap_or("");
                if server.is_empty() || share.is_empty() {
                    return Err(PathProblem::Relative);
                }
                if server.contains(':') || share.contains(':') {
                    return Err(PathProblem::BadChar);
                }
                (format!("//{server}/{share}"), it.next().unwrap_or("").to_string())
            } else {
                let b = s.as_bytes();
                if b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'/' {
                    (format!("{}:", char::from(b[0].to_ascii_uppercase())), s[3..].to_string())
                } else {
                    return Err(PathProblem::Relative);
                }
            }
        } else {
            match s.strip_prefix('/') {
                Some(r) => (String::new(), r.to_string()),
                None => return Err(PathProblem::Relative),
            }
        };
        let mut segments = Vec::new();
        for seg in rest.split('/') {
            match seg {
                "" | "." => {}
                ".." => return Err(PathProblem::DotDot),
                _ => {
                    if windows && (seg.contains(':') || seg.ends_with('.') || seg.ends_with(' ')) {
                        return Err(PathProblem::BadChar);
                    }
                    segments.push(seg.to_string());
                }
            }
        }
        Ok(NormPath { flavor, prefix, segments })
    }

    /// The flavor the path was parsed with.
    pub fn flavor(&self) -> PathFlavor {
        self.flavor
    }

    /// The segments below the root, normalised.
    pub fn segments(&self) -> &[String] {
        &self.segments
    }

    fn join(&self, more: &[String]) -> NormPath {
        let mut p = self.clone();
        p.segments.extend(more.iter().cloned());
        p
    }

    /// Whether `self` is `base` or lies below it: same flavor, same root and `base`'s segments
    /// are a prefix of `self`'s, compared case-insensitively on Windows and macOS.
    pub fn starts_with(&self, base: &NormPath) -> bool {
        let ci = self.flavor.case_insensitive();
        let eq = |a: &str, b: &str| if ci { a == b || a.to_lowercase() == b.to_lowercase() } else { a == b };
        self.flavor == base.flavor
            && eq(&self.prefix, &base.prefix)
            && base.segments.len() <= self.segments.len()
            && base.segments.iter().zip(&self.segments).all(|(a, b)| eq(a, b))
    }
}

impl fmt::Display for NormPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sep = if self.flavor == PathFlavor::Windows { "\\" } else { "/" };
        f.write_str(&self.prefix.replace('/', sep))?;
        if self.segments.is_empty() {
            return f.write_str(sep);
        }
        for s in &self.segments {
            f.write_str(sep)?;
            f.write_str(s)?;
        }
        Ok(())
    }
}

// ------------------------------------------------------------------------------- environment

/// Supplies variable values for [`PeruserLayout::expand`]. Values are raw path strings in the
/// OS family's syntax; `expand` validates them.
pub trait Env {
    /// The value of a single-path variable (everything except the configured roots), or `None`.
    fn path(&self, var: Var) -> Option<String>;
    /// The configured roots for [`Var::ProjectRoots`] or [`Var::ExportRoots`] (may be empty).
    fn roots(&self, var: Var) -> Vec<String>;
}

/// A plain [`Env`]: single-path values in a map, plus the configured roots.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MapEnv {
    pub paths: BTreeMap<Var, String>,
    pub project_roots: Vec<String>,
    pub export_roots: Vec<String>,
}

impl MapEnv {
    /// Sets one single-path value (builder style).
    pub fn with(mut self, var: Var, value: &str) -> MapEnv {
        self.paths.insert(var, value.to_string());
        self
    }
}

impl Env for MapEnv {
    fn path(&self, var: Var) -> Option<String> {
        self.paths.get(&var).cloned()
    }
    fn roots(&self, var: Var) -> Vec<String> {
        match var {
            Var::ProjectRoots => self.project_roots.clone(),
            Var::ExportRoots => self.export_roots.clone(),
            _ => Vec::new(),
        }
    }
}

/// An expansion or environment error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpandError {
    /// The layout has no entry for `os`.
    MissingOs { os: OsFamily },
    /// A variable the expansion needs has no value. `source` names the environment variable
    /// when [`resolve_env`] produced the error, otherwise the template variable.
    Unset { var: Var, source: String },
    /// A value is empty, relative, has `..` or is otherwise unusable.
    BadValue { var: Var, source: String, problem: PathProblem },
    /// An expanded entry lies outside `{home}`, `{tmp}`, the project roots and the export roots.
    OutsideProfile { os: OsFamily, entry: Entry },
}

impl fmt::Display for ExpandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExpandError::MissingOs { os } => write!(f, "MissingOs {{ os: {os} }}"),
            ExpandError::Unset { var, source } => write!(f, "Unset {{ var: {var}, source: {source} }}"),
            ExpandError::BadValue { var, source, problem } => {
                write!(f, "BadValue {{ var: {var}, source: {source}, problem: {problem} }}")
            }
            ExpandError::OutsideProfile { os, entry } => write!(f, "OutsideProfile {{ os: {os}, entry: {entry} }}"),
        }
    }
}

impl std::error::Error for ExpandError {}

/// Builds a [`MapEnv`] for `os` from raw environment variables read through `get` (inject a
/// fake map in tests; [`process_env`] passes the real environment). An empty value counts as
/// unset everywhere. Rules:
///
/// - Windows: `home` = `USERPROFILE` (required); `local_app_data` = `LOCALAPPDATA`, else
///   `{home}\AppData\Local`; `app_data` = `APPDATA`, else `{home}\AppData\Roaming`; `tmp` =
///   `TEMP`, else `TMP`, else `{local_app_data}\Temp`. A relative value is an error. Callers that
///   read the known folders (`FOLDERID_LocalAppData`, `FOLDERID_RoamingAppData`) through the OS
///   API should build a [`MapEnv`] themselves.
/// - macOS: `home` = `HOME` (required); `tmp` = `TMPDIR`, else `/tmp`. A relative value is an
///   error. `~/Library/...` locations are template literals below `{home}`.
/// - Linux: `home` = `HOME` (required); `xdg_data` = `XDG_DATA_HOME`, else `{home}/.local/share`;
///   `xdg_cache` = `XDG_CACHE_HOME`, else `{home}/.cache`; `xdg_config` = `XDG_CONFIG_HOME`, else
///   `{home}/.config`; `tmp` = `TMPDIR`, else `/tmp`. Following the XDG Base Directory
///   specification, a relative `XDG_*` value is ignored (the fallback applies); any other
///   relative value is an error.
///
/// Values with `..` segments are errors everywhere. `home` and `tmp` must not be a file-system
/// root. Project and export roots are passed through; [`PeruserLayout::expand`] validates them.
pub fn resolve_env(
    os: OsFamily,
    get: &dyn Fn(&str) -> Option<String>,
    project_roots: Vec<String>,
    export_roots: Vec<String>,
) -> Result<MapEnv, ExpandError> {
    let flavor = PathFlavor::of(os);
    let read = |name: &str| get(name).filter(|v| !v.is_empty());
    // Checks a value and returns it unchanged.
    let check = |var: Var, name: &str, value: String| -> Result<String, ExpandError> {
        check_single(&value, flavor).map_err(|problem| ExpandError::BadValue { var, source: name.into(), problem })?;
        Ok(value)
    };
    let sep = if flavor == PathFlavor::Windows { "\\" } else { "/" };
    let below = |base: &str, rest: &[&str]| {
        let mut s = base.trim_end_matches(['/', '\\']).to_string();
        for r in rest {
            s.push_str(sep);
            s.push_str(r);
        }
        s
    };
    let home_name = if flavor == PathFlavor::Windows { "USERPROFILE" } else { "HOME" };
    let home = match read(home_name) {
        Some(v) => check(Var::Home, home_name, v)?,
        None => return Err(ExpandError::Unset { var: Var::Home, source: home_name.into() }),
    };
    let mut env = MapEnv { paths: BTreeMap::new(), project_roots, export_roots };
    match flavor {
        PathFlavor::Windows => {
            let local = match read("LOCALAPPDATA") {
                Some(v) => check(Var::LocalAppData, "LOCALAPPDATA", v)?,
                None => below(&home, &["AppData", "Local"]),
            };
            let roaming = match read("APPDATA") {
                Some(v) => check(Var::AppData, "APPDATA", v)?,
                None => below(&home, &["AppData", "Roaming"]),
            };
            let tmp = match (read("TEMP"), read("TMP")) {
                (Some(v), _) => check(Var::Tmp, "TEMP", v)?,
                (None, Some(v)) => check(Var::Tmp, "TMP", v)?,
                (None, None) => below(&local, &["Temp"]),
            };
            env.paths.insert(Var::LocalAppData, local);
            env.paths.insert(Var::AppData, roaming);
            env.paths.insert(Var::Tmp, tmp);
        }
        PathFlavor::Macos | PathFlavor::Linux => {
            let tmp = match read("TMPDIR") {
                Some(v) => check(Var::Tmp, "TMPDIR", v)?,
                None => "/tmp".to_string(),
            };
            env.paths.insert(Var::Tmp, tmp);
            if flavor == PathFlavor::Linux {
                let xdg = [
                    (Var::XdgData, "XDG_DATA_HOME", &[".local", "share"][..]),
                    (Var::XdgCache, "XDG_CACHE_HOME", &[".cache"][..]),
                    (Var::XdgConfig, "XDG_CONFIG_HOME", &[".config"][..]),
                ];
                for (var, name, fallback) in xdg {
                    let value = match read(name) {
                        Some(v) if NormPath::parse(&v, flavor) == Err(PathProblem::Relative) => below(&home, fallback),
                        Some(v) => check(var, name, v)?,
                        None => below(&home, fallback),
                    };
                    env.paths.insert(var, value);
                }
            }
        }
    }
    env.paths.insert(Var::Home, home);
    Ok(env)
}

/// [`resolve_env`] over the real process environment. Reads environment variables only; never
/// touches the file system.
pub fn process_env(os: OsFamily, project_roots: Vec<String>, export_roots: Vec<String>) -> Result<MapEnv, ExpandError> {
    resolve_env(os, &|name| std::env::var(name).ok(), project_roots, export_roots)
}

fn check_single(value: &str, flavor: PathFlavor) -> Result<NormPath, PathProblem> {
    let p = NormPath::parse(value, flavor)?;
    if p.segments.is_empty() {
        return Err(PathProblem::RootOnly);
    }
    Ok(p)
}

// --------------------------------------------------------------------------------- expansion

/// A layout expanded for one OS family and environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpandedLayout {
    pub os: OsFamily,
    pub install_dir: NormPath,
    /// In template order; `{project_roots}` and `{export_roots}` expand to one path per root
    /// (none when no root is configured).
    pub allowed_writes: Vec<NormPath>,
}

impl ExpandedLayout {
    /// See [`is_allowed_write`].
    pub fn is_allowed_write(&self, path: &str) -> bool {
        is_allowed_write(path, self)
    }
}

/// Whether the running app may write `path` (REQ-PLAT.DEPLOY.NOADMIN.LAYOUT-005): `path` must
/// parse as an absolute path of the layout's flavor without `..` segments (see the module
/// documentation) and lie at or below one of `expanded.allowed_writes`. Anything that does not
/// parse is not allowed.
pub fn is_allowed_write(path: &str, expanded: &ExpandedLayout) -> bool {
    match NormPath::parse(path, PathFlavor::of(expanded.os)) {
        Ok(p) => expanded.allowed_writes.iter().any(|root| p.starts_with(root)),
        Err(_) => false,
    }
}

impl PeruserLayout {
    /// Expands the layout of `os` with values from `env`. Fails when `os` has no layout, a needed
    /// value is missing or invalid (empty, relative, `..`, or a file-system root for `home`,
    /// `tmp` and the per-user directories), or an expanded entry lies outside `{home}`, `{tmp}`,
    /// the project roots and the export roots (REQ-PLAT.DEPLOY.NOADMIN.LAYOUT-004). `home` is
    /// always required; other variables only when a template uses them.
    pub fn expand(&self, os: OsFamily, env: &dyn Env) -> Result<ExpandedLayout, ExpandError> {
        let Some(l) = self.os.get(&os) else {
            return Err(ExpandError::MissingOs { os });
        };
        let flavor = PathFlavor::of(os);
        let mut cache: BTreeMap<Var, Vec<NormPath>> = BTreeMap::new();
        let mut value = |var: Var| -> Result<Vec<NormPath>, ExpandError> {
            if let Some(v) = cache.get(&var) {
                return Ok(v.clone());
            }
            let bad = |problem| ExpandError::BadValue { var, source: var.as_str().into(), problem };
            let v = if var.is_list() {
                env.roots(var).iter().map(|r| NormPath::parse(r, flavor).map_err(bad)).collect::<Result<Vec<_>, _>>()?
            } else {
                match env.path(var) {
                    Some(s) => vec![check_single(&s, flavor).map_err(bad)?],
                    None => return Err(ExpandError::Unset { var, source: var.as_str().into() }),
                }
            };
            cache.insert(var, v.clone());
            Ok(v)
        };
        let mut bases = value(Var::Home)?;
        if env.path(Var::Tmp).is_some() || l.allowed_writes.iter().any(|t| t.var == Var::Tmp) {
            bases.extend(value(Var::Tmp)?);
        }
        bases.extend(value(Var::ProjectRoots)?);
        bases.extend(value(Var::ExportRoots)?);
        let inside = |p: &NormPath| bases.iter().any(|b| p.starts_with(b));

        let mut install = value(l.install_dir.var)?.into_iter().map(|b| b.join(&l.install_dir.segments));
        let install_dir = match (install.next(), install.next()) {
            (Some(p), None) if inside(&p) => p,
            _ => return Err(ExpandError::OutsideProfile { os, entry: Entry::InstallDir }),
        };
        let mut allowed_writes = Vec::new();
        for (i, t) in l.allowed_writes.iter().enumerate() {
            for b in value(t.var)? {
                let p = b.join(&t.segments);
                if !inside(&p) {
                    return Err(ExpandError::OutsideProfile { os, entry: Entry::AllowedWrite(i) });
                }
                allowed_writes.push(p);
            }
        }
        Ok(ExpandedLayout { os, install_dir, allowed_writes })
    }
}
