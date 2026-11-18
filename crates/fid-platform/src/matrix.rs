//! Metrology Maven support matrix (spec node `PLAT.MATRIX.FILE`): the typed schema of
//! `platform/support-matrix.toml`, a strict bounded validator and the tier lookup.
//! Every other PLAT consumer reads the matrix through this crate, so there is exactly one
//! parser and one meaning of "tier".
//!
//! Determinism D0: pure functions over bytes; tables are `BTreeMap`-backed (the `toml`
//! crate without `preserve_order`), and diagnostics have a fixed order (see [`validate`]).

use std::cmp::Ordering;
use std::fmt;

/// Highest `schema_version` this reader supports (REQ-PLAT.MATRIX.FILE-006).
pub const SUPPORTED_SCHEMA: u32 = 1;
/// Largest accepted file, in bytes (REQ-PLAT.MATRIX.FILE-005).
pub const MAX_BYTES: usize = 1024 * 1024;
/// Largest accepted number of `[[row]]` entries (REQ-PLAT.MATRIX.FILE-005).
pub const MAX_ROWS: usize = 10_000;
/// Longest accepted `notes` string, in bytes.
pub const MAX_NOTES: usize = 4096;
/// Validation stops collecting diagnostics after this many and appends `TooManyErrors`.
pub const MAX_ERRORS: usize = 1000;

const ROW_KEYS: [&str; 7] = ["arch", "notes", "os", "since_phase", "tier", "version_max", "version_min"];
const TOP_KEYS: [&str; 2] = ["row", "schema_version"];

macro_rules! named_enum {
    ($(#[$m:meta])* $name:ident { $($var:ident = $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name { $($var),+ }
        impl $name {
            /// Every value, in declaration order.
            pub const ALL: &'static [$name] = &[$($name::$var),+];
            /// The name used in the matrix file.
            pub fn as_str(self) -> &'static str {
                match self { $($name::$var => $s),+ }
            }
            /// Parses the name used in the matrix file.
            pub fn from_name(s: &str) -> Option<Self> {
                match s { $($s => Some($name::$var),)+ _ => None }
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

named_enum!(
    /// OS family (extensible by schema bump).
    OsFamily { Windows = "windows", Ubuntu = "ubuntu", Debian = "debian", Fedora = "fedora", Rhel = "rhel", Macos = "macos" }
);
named_enum!(
    /// CPU architecture.
    Arch { X86_64 = "x86_64", Aarch64 = "aarch64" }
);
named_enum!(
    /// Support tier.
    Tier { Tier1 = "tier1", Tier2 = "tier2", Tier3 = "tier3" }
);
named_enum!(
    /// CONSTITUTION §7 phase code; the derived order is the phase order.
    Phase { P0 = "P0", P1a = "P1a", P1b = "P1b", P2 = "P2", P3 = "P3", P4 = "P4", Later = "later" }
);

/// A dotted numeric version of 1 to 4 `u32` components. Missing trailing components
/// compare as 0, so `26` == `26.0` and `22.04` == `22.4`.
#[derive(Debug, Clone, Copy)]
pub struct Version {
    parts: [u32; 4],
    len: usize,
}

impl Version {
    /// Parses `"10.0.19041"`-style text: 1 to 4 non-empty runs of ASCII digits separated by
    /// single dots, each fitting in a `u32`.
    pub fn parse(s: &str) -> Option<Version> {
        let mut parts = [0u32; 4];
        let mut len = 0;
        for piece in s.split('.') {
            if len == 4 || piece.is_empty() || piece.len() > 10 || !piece.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            parts[len] = piece.parse().ok()?;
            len += 1;
        }
        Some(Version { parts, len })
    }

    /// The numeric components as written.
    pub fn components(&self) -> &[u32] {
        &self.parts[..self.len]
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.parts == other.parts
    }
}
impl Eq for Version {}
impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.parts.cmp(&other.parts)
    }
}
impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, p) in self.components().iter().enumerate() {
            if i > 0 {
                f.write_str(".")?;
            }
            write!(f, "{p}")?;
        }
        Ok(())
    }
}

/// One matrix row: the half-open version range `[version_min, version_max)` of an OS
/// family on one architecture, its tier and the phase from which the tier applies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub os: OsFamily,
    pub version_min: Version,
    /// `None` means open-ended.
    pub version_max: Option<Version>,
    pub arch: Arch,
    pub tier: Tier,
    pub since_phase: Phase,
    pub notes: String,
}

impl Row {
    fn contains(&self, v: &Version) -> bool {
        *v >= self.version_min
            && match &self.version_max {
                Some(m) => v < m,
                None => true,
            }
    }
}

/// A validated matrix: no row errors and no overlapping ranges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matrix {
    pub schema_version: u32,
    pub rows: Vec<Row>,
}

/// The effective support of one platform at a given phase (REQ-PLAT.MATRIX.FILE-004).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectiveTier {
    /// The row's tier applies now.
    Tier(Tier),
    /// The row's tier applies only from `since_phase`, which is later than the current phase.
    Planned(Tier, Phase),
    /// No row matches.
    NotListed,
}

impl fmt::Display for EffectiveTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EffectiveTier::Tier(t) => write!(f, "{t}"),
            EffectiveTier::Planned(t, p) => write!(f, "planned({t}, {p})"),
            EffectiveTier::NotListed => f.write_str("not_listed"),
        }
    }
}

impl Matrix {
    /// Effective tier of `os` `version` on `arch` at phase `current`. Unknown OS or
    /// architecture names and malformed versions are `NotListed`. Rows are scanned in file
    /// order; a validated matrix has at most one matching row.
    pub fn lookup(&self, os: &str, version: &str, arch: &str, current: Phase) -> EffectiveTier {
        let (Some(os), Some(arch), Some(v)) = (OsFamily::from_name(os), Arch::from_name(arch), Version::parse(version))
        else {
            return EffectiveTier::NotListed;
        };
        match self.rows.iter().find(|r| r.os == os && r.arch == arch && r.contains(&v)) {
            Some(r) if current >= r.since_phase => EffectiveTier::Tier(r.tier),
            Some(r) => EffectiveTier::Planned(r.tier, r.since_phase),
            None => EffectiveTier::NotListed,
        }
    }
}

/// Which bound `TooLarge` hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limit {
    Bytes,
    Rows,
}

/// Whether a field was absent or present with an unusable value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    Missing,
    Invalid,
}

/// A typed matrix error. Row numbers are 1-based positions among the `[[row]]` entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatrixError {
    /// Fatal: the file exceeds [`MAX_BYTES`] or [`MAX_ROWS`].
    TooLarge {
        limit: Limit,
        found: usize,
        max: usize,
    },
    /// Fatal: `schema_version` is above [`SUPPORTED_SCHEMA`].
    UnsupportedSchema {
        found: u64,
    },
    /// Fatal: not UTF-8 or not TOML. `line` is 1-based when the parser reports a position.
    Parse {
        line: Option<usize>,
        msg: String,
    },
    /// A structural problem: an unknown key, a wrong type or a missing required field
    /// that has no more specific error. `row` is `None` for top-level keys.
    BadField {
        row: Option<usize>,
        field: String,
        msg: &'static str,
    },
    BadPhase {
        row: usize,
        problem: Problem,
    },
    UnknownOs {
        row: usize,
        problem: Problem,
    },
    UnknownArch {
        row: usize,
        problem: Problem,
    },
    BadTier {
        row: usize,
        problem: Problem,
    },
    BadVersion {
        row: usize,
        field: &'static str,
        problem: Problem,
    },
    /// `version_max` is not above `version_min`.
    EmptyRange {
        row: usize,
    },
    /// Rows `a` < `b` share os and arch and their version ranges overlap.
    OverlappingRange {
        a: usize,
        b: usize,
        os: OsFamily,
        arch: Arch,
    },
    /// Validation stopped after [`MAX_ERRORS`] diagnostics.
    TooManyErrors {
        limit: usize,
    },
}

fn what(problem: Problem, expected: &str) -> String {
    match problem {
        Problem::Missing => "is missing".into(),
        Problem::Invalid => format!("is not {expected}"),
    }
}

const PHASES: &str = "a phase code (P0, P1a, P1b, P2, P3, P4, later)";
const OSES: &str = "one of windows, ubuntu, debian, fedora, rhel, macos";
const ARCHES: &str = "one of x86_64, aarch64";
const TIERS: &str = "one of tier1, tier2, tier3";
const VERSIONS: &str = "a dotted numeric version of 1 to 4 u32 components";

impl fmt::Display for MatrixError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MatrixError::TooLarge { limit, found, max } => {
                let unit = match limit {
                    Limit::Bytes => "bytes",
                    Limit::Rows => "rows",
                };
                write!(f, "TooLarge: {found} {unit} exceeds the limit of {max}")
            }
            MatrixError::UnsupportedSchema { found } => {
                write!(f, "UnsupportedSchema: schema_version {found} is above the supported version {SUPPORTED_SCHEMA}")
            }
            MatrixError::Parse { line: Some(l), msg } => write!(f, "Parse: line {l}: {msg}"),
            MatrixError::Parse { line: None, msg } => write!(f, "Parse: {msg}"),
            MatrixError::BadField { row: Some(r), field, msg } => write!(f, "BadField: row {r}: {field} {msg}"),
            MatrixError::BadField { row: None, field, msg } => write!(f, "BadField: {field} {msg}"),
            MatrixError::BadPhase { row, problem } => {
                write!(f, "BadPhase: row {row}: since_phase {}", what(*problem, PHASES))
            }
            MatrixError::UnknownOs { row, problem } => write!(f, "UnknownOs: row {row}: os {}", what(*problem, OSES)),
            MatrixError::UnknownArch { row, problem } => {
                write!(f, "UnknownArch: row {row}: arch {}", what(*problem, ARCHES))
            }
            MatrixError::BadTier { row, problem } => write!(f, "BadTier: row {row}: tier {}", what(*problem, TIERS)),
            MatrixError::BadVersion { row, field, problem } => {
                write!(f, "BadVersion: row {row}: {field} {}", what(*problem, VERSIONS))
            }
            MatrixError::EmptyRange { row } => {
                write!(f, "EmptyRange: row {row}: version_max is not above version_min")
            }
            MatrixError::OverlappingRange { a, b, os, arch } => {
                write!(f, "OverlappingRange: rows {a} and {b}: {os} {arch} version ranges overlap")
            }
            MatrixError::TooManyErrors { limit } => write!(f, "TooManyErrors: stopped after {limit} errors"),
        }
    }
}

impl std::error::Error for MatrixError {}

/// The result of validating one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Validation {
    /// Number of `[[row]]` entries (0 when a fatal error stopped parsing).
    pub rows: usize,
    /// Diagnostics: top-level field errors, then row errors in row order (each row's checks
    /// in the order unknown keys, os, version_min, version_max, arch, tier, since_phase,
    /// notes, range), then overlaps sorted by (a, b). A fatal error is the only entry.
    pub errors: Vec<MatrixError>,
    /// The matrix, present only when `errors` is empty.
    pub matrix: Option<Matrix>,
}

impl Validation {
    /// The report `cargo xtask plat-matrix validate` prints: one diagnostic per line, then
    /// `<rows> rows, <errors> errors`. Byte-identical for identical input (REQ-007).
    pub fn render(&self) -> String {
        let mut out = String::new();
        for e in &self.errors {
            out.push_str(&e.to_string());
            out.push('\n');
        }
        out.push_str(&format!("{} rows, {} errors\n", self.rows, self.errors.len()));
        out
    }
}

/// Parses and validates a matrix, returning the first error if there is any.
pub fn parse(bytes: &[u8]) -> Result<Matrix, MatrixError> {
    let v = validate(bytes);
    match (v.matrix, v.errors.into_iter().next()) {
        (Some(m), None) => Ok(m),
        (_, Some(e)) => Err(e),
        (None, None) => Err(MatrixError::Parse { line: None, msg: "no matrix".into() }),
    }
}

fn fatal(e: MatrixError) -> Validation {
    Validation { rows: 0, errors: vec![e], matrix: None }
}

/// Counts `[[row]]` headers without parsing, so oversized files fail before a full parse.
/// Lines inside multi-line strings can only make the count larger, which fails closed.
fn prescan_rows(text: &str) -> usize {
    text.lines()
        .filter(|l| {
            let t = l.trim_start();
            match t.strip_prefix("[[").map(str::trim_start).and_then(|r| r.strip_prefix("row")) {
                Some(r) => r.trim_start().starts_with("]]"),
                None => false,
            }
        })
        .count()
}

fn line_of(text: &str, offset: usize) -> usize {
    text.as_bytes().iter().take(offset).filter(|&&b| b == b'\n').count() + 1
}

/// Validates a matrix file (REQ-PLAT.MATRIX.FILE-002/003/005/006/007). Fatal errors
/// (`TooLarge`, `UnsupportedSchema`, `Parse`) stop at once with no partial result.
pub fn validate(bytes: &[u8]) -> Validation {
    if bytes.len() > MAX_BYTES {
        return fatal(MatrixError::TooLarge { limit: Limit::Bytes, found: bytes.len(), max: MAX_BYTES });
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return fatal(MatrixError::Parse { line: None, msg: "file is not UTF-8".into() });
    };
    let pre = prescan_rows(text);
    if pre > MAX_ROWS {
        return fatal(MatrixError::TooLarge { limit: Limit::Rows, found: pre, max: MAX_ROWS });
    }
    let table = match text.parse::<toml::Table>() {
        Ok(t) => t,
        Err(e) => {
            let line = e.span().map(|s| line_of(text, s.start));
            return fatal(MatrixError::Parse { line, msg: e.message().trim_end().to_string() });
        }
    };

    let mut errors = Vec::new();
    match table.get("schema_version") {
        None => errors.push(MatrixError::BadField { row: None, field: "schema_version".into(), msg: "is missing" }),
        Some(toml::Value::Integer(n)) if *n >= 1 => {
            let n = n.unsigned_abs();
            if n > u64::from(SUPPORTED_SCHEMA) {
                return fatal(MatrixError::UnsupportedSchema { found: n });
            }
        }
        Some(_) => errors.push(MatrixError::BadField {
            row: None,
            field: "schema_version".into(),
            msg: "is not a positive integer",
        }),
    }
    for k in table.keys() {
        if !TOP_KEYS.contains(&k.as_str()) {
            errors.push(MatrixError::BadField { row: None, field: k.clone(), msg: "is not a known key" });
        }
    }
    let items: &[toml::Value] = match table.get("row") {
        None => &[],
        Some(toml::Value::Array(a)) => a,
        Some(_) => {
            errors.push(MatrixError::BadField { row: None, field: "row".into(), msg: "is not an array of tables" });
            &[]
        }
    };
    if items.len() > MAX_ROWS {
        return fatal(MatrixError::TooLarge { limit: Limit::Rows, found: items.len(), max: MAX_ROWS });
    }

    // (file row number, row) for rows without errors.
    let mut good: Vec<(usize, Row)> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let n = i + 1;
        match item {
            toml::Value::Table(t) => {
                if let Some(row) = check_row(n, t, &mut errors) {
                    good.push((n, row));
                }
            }
            _ => errors.push(MatrixError::BadField { row: Some(n), field: "row".into(), msg: "is not a table" }),
        }
    }

    // Overlaps: stable sort by (os, arch, version_min); each row is compared with the later
    // rows of its group whose start lies before its end, so non-adjacent overlaps are found.
    let mut sorted: Vec<&(usize, Row)> = good.iter().collect();
    sorted.sort_by_key(|x| (x.1.os, x.1.arch, x.1.version_min));
    let mut overlaps: Vec<(usize, usize, OsFamily, Arch)> = Vec::new();
    'outer: for (i, (na, ra)) in sorted.iter().map(|p| (p.0, &p.1)).enumerate() {
        for (nb, rb) in sorted[i + 1..].iter().map(|p| (p.0, &p.1)) {
            if rb.os != ra.os || rb.arch != ra.arch {
                break;
            }
            if let Some(m) = &ra.version_max {
                if *m <= rb.version_min {
                    break;
                }
            }
            overlaps.push((na.min(nb), na.max(nb), ra.os, ra.arch));
            if errors.len() + overlaps.len() > MAX_ERRORS {
                break 'outer;
            }
        }
    }
    overlaps.sort();
    errors.extend(overlaps.into_iter().map(|(a, b, os, arch)| MatrixError::OverlappingRange { a, b, os, arch }));
    if errors.len() > MAX_ERRORS {
        errors.truncate(MAX_ERRORS);
        errors.push(MatrixError::TooManyErrors { limit: MAX_ERRORS });
    }

    let matrix = if errors.is_empty() {
        Some(Matrix { schema_version: SUPPORTED_SCHEMA, rows: good.into_iter().map(|(_, r)| r).collect() })
    } else {
        None
    };
    Validation { rows: items.len(), errors, matrix }
}

fn enum_field<T>(t: &toml::Table, key: &str, parse: fn(&str) -> Option<T>) -> Result<T, Problem> {
    match t.get(key) {
        None => Err(Problem::Missing),
        Some(toml::Value::String(s)) => parse(s).ok_or(Problem::Invalid),
        Some(_) => Err(Problem::Invalid),
    }
}

fn check_row(n: usize, t: &toml::Table, errors: &mut Vec<MatrixError>) -> Option<Row> {
    let before = errors.len();
    for k in t.keys() {
        if !ROW_KEYS.contains(&k.as_str()) {
            errors.push(MatrixError::BadField { row: Some(n), field: k.clone(), msg: "is not a known key" });
        }
    }
    let os = enum_field(t, "os", OsFamily::from_name);
    if let Err(problem) = os {
        errors.push(MatrixError::UnknownOs { row: n, problem });
    }
    let vmin = enum_field(t, "version_min", Version::parse);
    if let Err(problem) = vmin {
        errors.push(MatrixError::BadVersion { row: n, field: "version_min", problem });
    }
    let vmax = match enum_field(t, "version_max", Version::parse) {
        Ok(v) => Some(v),
        Err(Problem::Missing) => None,
        Err(problem) => {
            errors.push(MatrixError::BadVersion { row: n, field: "version_max", problem });
            None
        }
    };
    let arch = enum_field(t, "arch", Arch::from_name);
    if let Err(problem) = arch {
        errors.push(MatrixError::UnknownArch { row: n, problem });
    }
    let tier = enum_field(t, "tier", Tier::from_name);
    if let Err(problem) = tier {
        errors.push(MatrixError::BadTier { row: n, problem });
    }
    let phase = enum_field(t, "since_phase", Phase::from_name);
    if let Err(problem) = phase {
        errors.push(MatrixError::BadPhase { row: n, problem });
    }
    let notes = match t.get("notes") {
        Some(toml::Value::String(s)) if s.len() <= MAX_NOTES => Some(s.clone()),
        None => {
            errors.push(MatrixError::BadField { row: Some(n), field: "notes".into(), msg: "is missing" });
            None
        }
        Some(_) => {
            errors.push(MatrixError::BadField {
                row: Some(n),
                field: "notes".into(),
                msg: "is not a string of at most 4096 bytes",
            });
            None
        }
    };
    if let (Ok(lo), Some(hi)) = (&vmin, &vmax) {
        if hi <= lo {
            errors.push(MatrixError::EmptyRange { row: n });
        }
    }
    if errors.len() != before {
        return None;
    }
    match (os, vmin, arch, tier, phase, notes) {
        (Ok(os), Ok(version_min), Ok(arch), Ok(tier), Ok(since_phase), Some(notes)) => {
            Some(Row { os, version_min, version_max: vmax, arch, tier, since_phase, notes })
        }
        _ => None,
    }
}
