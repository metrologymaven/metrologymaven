//! Metrology Maven hardware profiles (spec node `PLAT.HW.PROFILE`): the typed schema of
//! `platform/hw-profiles.toml` and its strict, bounded validator. `fid doctor`, the docs and
//! the benchmark tooling read the profiles through this module, so there is one parser and
//! one meaning of "at least the minimum".
//!
//! Determinism D0: pure functions over bytes; every map is a `BTreeMap` (the `toml` crate
//! without `preserve_order`), and diagnostics have a fixed order (see [`validate`]).

use crate::matrix::Arch;
use std::collections::BTreeMap;
use std::fmt;

/// The only `schema_version` this reader supports.
pub const SUPPORTED_SCHEMA: u32 = 1;
/// Largest accepted file, in bytes (REQ-PLAT.HW.PROFILE-005).
pub const MAX_BYTES: usize = 64 * 1024;
/// Path of the shipped file, relative to the repository root.
pub const DEFAULT_PATH: &str = "platform/hw-profiles.toml";
/// The two profiles every file must contain (REQ-PLAT.HW.PROFILE-002).
pub const MINIMUM: &str = "minimum";
pub const RECOMMENDED: &str = "recommended";

const TOP_KEYS: [&str; 2] = ["profiles", "schema_version"];
const PROFILE_KEYS: [&str; 7] =
    ["cores", "cpu_level", "display", "gpu_class", "ram_gib", "storage_free_gib", "vram_gib"];

/// CPU instruction-set level. Each level belongs to one architecture; within an
/// architecture the derived order is the capability order (v2 < v3, 8.0 < 8.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CpuLevel {
    X86_64V2,
    X86_64V3,
    Armv8_0,
    Armv8_2,
}

impl CpuLevel {
    /// The name used in the profile file.
    pub fn as_str(self) -> &'static str {
        match self {
            CpuLevel::X86_64V2 => "x86-64-v2",
            CpuLevel::X86_64V3 => "x86-64-v3",
            CpuLevel::Armv8_0 => "armv8.0",
            CpuLevel::Armv8_2 => "armv8.2",
        }
    }

    /// The architecture this level belongs to.
    pub fn arch(self) -> Arch {
        match self {
            CpuLevel::X86_64V2 | CpuLevel::X86_64V3 => Arch::X86_64,
            CpuLevel::Armv8_0 | CpuLevel::Armv8_2 => Arch::Aarch64,
        }
    }

    /// Parses a level name, accepting it only for its own architecture.
    pub fn from_name(arch: Arch, s: &str) -> Option<CpuLevel> {
        let level = match s {
            "x86-64-v2" => CpuLevel::X86_64V2,
            "x86-64-v3" => CpuLevel::X86_64V3,
            "armv8.0" => CpuLevel::Armv8_0,
            "armv8.2" => CpuLevel::Armv8_2,
            _ => return None,
        };
        if level.arch() == arch {
            Some(level)
        } else {
            None
        }
    }
}

impl fmt::Display for CpuLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// GPU class (REQ-PLAT.HW.PROFILE-004); the derived order is the rank.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GpuClass {
    Software,
    Integrated,
    DiscreteEntry,
    DiscreteMid,
}

impl GpuClass {
    /// Every class, lowest rank first.
    pub const ALL: &'static [GpuClass] =
        &[GpuClass::Software, GpuClass::Integrated, GpuClass::DiscreteEntry, GpuClass::DiscreteMid];

    /// The name used in the profile file.
    pub fn as_str(self) -> &'static str {
        match self {
            GpuClass::Software => "software",
            GpuClass::Integrated => "integrated",
            GpuClass::DiscreteEntry => "discrete_entry",
            GpuClass::DiscreteMid => "discrete_mid",
        }
    }

    /// Parses the name used in the profile file.
    pub fn from_name(s: &str) -> Option<GpuClass> {
        GpuClass::ALL.iter().copied().find(|c| c.as_str() == s)
    }
}

impl fmt::Display for GpuClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One hardware profile (REQ-PLAT.HW.PROFILE-001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    /// Instruction-set level per supported architecture; at least one entry.
    pub cpu_level: BTreeMap<Arch, CpuLevel>,
    /// Physical cores, at least 1.
    pub cores: u32,
    pub ram_gib: u32,
    pub gpu_class: GpuClass,
    pub vram_gib: u32,
    pub storage_free_gib: u32,
    /// Minimum display resolution (width, height) in pixels, each at least 1.
    pub display: (u32, u32),
}

/// A validated profile file: `minimum` and `recommended` are present and recommended is
/// at least minimum on every field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HwProfiles {
    pub schema_version: u32,
    pub profiles: BTreeMap<String, Profile>,
}

impl HwProfiles {
    /// The `minimum` profile (always present in a validated file).
    pub fn minimum(&self) -> Option<&Profile> {
        self.profiles.get(MINIMUM)
    }

    /// The `recommended` profile (always present in a validated file).
    pub fn recommended(&self) -> Option<&Profile> {
        self.profiles.get(RECOMMENDED)
    }
}

/// A typed profile error. `at` is the dotted path of the table holding the field
/// (empty for the top level).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HwError {
    /// Fatal: the file exceeds [`MAX_BYTES`].
    TooLarge { found: usize, max: usize },
    /// Fatal: `schema_version` is an integer other than [`SUPPORTED_SCHEMA`].
    UnsupportedSchema { found: i64 },
    /// Not UTF-8, not TOML (fatal; `line` is 1-based when the parser reports one), or a
    /// known field that is missing or has an unusable value (`line` is `None`).
    Parse { line: Option<usize>, msg: String },
    /// A key the schema does not define.
    UnknownField { field: String, at: String },
    /// A required profile is absent.
    MissingProfile { name: &'static str },
    /// `recommended` is below `minimum` on `field` (`cpu_level.<arch>` for CPU levels).
    RecommendedBelowMinimum { field: String },
}

impl fmt::Display for HwError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HwError::TooLarge { found, max } => write!(f, "TooLarge {{ found: {found}, max: {max} }}"),
            HwError::UnsupportedSchema { found } => {
                write!(f, "UnsupportedSchema {{ found: {found} }} (supported: {SUPPORTED_SCHEMA})")
            }
            HwError::Parse { line: Some(l), msg } => write!(f, "Parse {{ line: {l} }}: {msg}"),
            HwError::Parse { line: None, msg } => write!(f, "Parse: {msg}"),
            HwError::UnknownField { field, at } if at.is_empty() => write!(f, "UnknownField {{ field: {field} }}"),
            HwError::UnknownField { field, at } => write!(f, "UnknownField {{ field: {field} }} in {at}"),
            HwError::MissingProfile { name } => write!(f, "MissingProfile {{ name: {name} }}"),
            HwError::RecommendedBelowMinimum { field } => write!(f, "RecommendedBelowMinimum {{ field: {field} }}"),
        }
    }
}

impl std::error::Error for HwError {}

fn line_of(text: &str, offset: usize) -> usize {
    text.as_bytes().iter().take(offset).filter(|&&b| b == b'\n').count() + 1
}

fn bad(at: &str, field: &str, msg: &str) -> HwError {
    HwError::Parse { line: None, msg: format!("{at}.{field} {msg}") }
}

/// Validates a profile file (REQ-PLAT.HW.PROFILE-001..005) and returns every diagnostic.
///
/// `TooLarge`, a non-UTF-8 or non-TOML file and `UnsupportedSchema` stop at once and are
/// the only entry. Otherwise the order is: top-level errors (`schema_version`, then unknown
/// keys in key order), each profile in name order (unknown keys in key order, then the
/// fields `cpu_level`, `cores`, `ram_gib`, `gpu_class`, `vram_gib`, `storage_free_gib`,
/// `display`), `MissingProfile` for `minimum` then `recommended`, and finally the
/// recommended-versus-minimum comparison in that same field order. The comparison runs only
/// when both profiles parsed without errors.
pub fn validate(bytes: &[u8]) -> Result<HwProfiles, Vec<HwError>> {
    if bytes.len() > MAX_BYTES {
        return Err(vec![HwError::TooLarge { found: bytes.len(), max: MAX_BYTES }]);
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Err(vec![HwError::Parse { line: None, msg: "file is not UTF-8".into() }]);
    };
    let table = match text.parse::<toml::Table>() {
        Ok(t) => t,
        Err(e) => {
            let line = e.span().map(|s| line_of(text, s.start));
            return Err(vec![HwError::Parse { line, msg: e.message().trim_end().to_string() }]);
        }
    };

    let mut errors = Vec::new();
    match table.get("schema_version") {
        None => errors.push(HwError::Parse { line: None, msg: "schema_version is missing".into() }),
        Some(toml::Value::Integer(n)) if *n == i64::from(SUPPORTED_SCHEMA) => {}
        Some(toml::Value::Integer(n)) => return Err(vec![HwError::UnsupportedSchema { found: *n }]),
        Some(_) => errors.push(HwError::Parse { line: None, msg: "schema_version is not an integer".into() }),
    }
    for k in table.keys() {
        if !TOP_KEYS.contains(&k.as_str()) {
            errors.push(HwError::UnknownField { field: k.clone(), at: String::new() });
        }
    }

    let mut profiles = BTreeMap::new();
    let mut names: Vec<&str> = Vec::new();
    match table.get("profiles") {
        None => errors.push(HwError::Parse { line: None, msg: "profiles is missing".into() }),
        Some(toml::Value::Table(t)) => {
            for (name, value) in t {
                names.push(name);
                let at = format!("profiles.{name}");
                match value {
                    toml::Value::Table(pt) => {
                        if let Some(p) = check_profile(&at, pt, &mut errors) {
                            profiles.insert(name.clone(), p);
                        }
                    }
                    _ => errors.push(HwError::Parse { line: None, msg: format!("{at} is not a table") }),
                }
            }
        }
        Some(_) => errors.push(HwError::Parse { line: None, msg: "profiles is not a table".into() }),
    }
    for required in [MINIMUM, RECOMMENDED] {
        if !names.contains(&required) {
            errors.push(HwError::MissingProfile { name: required });
        }
    }
    if let (Some(min), Some(rec)) = (profiles.get(MINIMUM), profiles.get(RECOMMENDED)) {
        errors.extend(below_minimum(min, rec).into_iter().map(|field| HwError::RecommendedBelowMinimum { field }));
    }

    if errors.is_empty() {
        Ok(HwProfiles { schema_version: SUPPORTED_SCHEMA, profiles })
    } else {
        Err(errors)
    }
}

/// Parses and validates a profile file, returning the first error if there is any.
pub fn parse(bytes: &[u8]) -> Result<HwProfiles, HwError> {
    validate(bytes).map_err(|errs| {
        errs.into_iter().next().unwrap_or(HwError::Parse { line: None, msg: "invalid profile file".into() })
    })
}

/// The fields on which `rec` is below `min`, in schema order (REQ-PLAT.HW.PROFILE-003).
/// For CPU levels, every architecture `min` lists must be listed by `rec` at an equal or
/// higher level; a missing architecture counts as below. Display compares width and height
/// separately.
pub fn below_minimum(min: &Profile, rec: &Profile) -> Vec<String> {
    let mut out = Vec::new();
    for (arch, lo) in &min.cpu_level {
        match rec.cpu_level.get(arch) {
            Some(hi) if hi >= lo => {}
            _ => out.push(format!("cpu_level.{arch}")),
        }
    }
    let pairs = [
        ("cores", min.cores, rec.cores),
        ("ram_gib", min.ram_gib, rec.ram_gib),
        ("gpu_class", min.gpu_class as u32, rec.gpu_class as u32),
        ("vram_gib", min.vram_gib, rec.vram_gib),
        ("storage_free_gib", min.storage_free_gib, rec.storage_free_gib),
    ];
    for (field, lo, hi) in pairs {
        if hi < lo {
            out.push(field.to_string());
        }
    }
    if rec.display.0 < min.display.0 || rec.display.1 < min.display.1 {
        out.push("display".to_string());
    }
    out
}

fn uint(t: &toml::Table, at: &str, field: &str, min: u32, errors: &mut Vec<HwError>) -> Option<u32> {
    match t.get(field) {
        None => {
            errors.push(bad(at, field, "is missing"));
            None
        }
        Some(toml::Value::Integer(n)) => match u32::try_from(*n) {
            Ok(v) if v >= min => Some(v),
            _ => {
                errors.push(bad(at, field, &format!("is not an integer from {min} to {}", u32::MAX)));
                None
            }
        },
        Some(_) => {
            errors.push(bad(at, field, &format!("is not an integer from {min} to {}", u32::MAX)));
            None
        }
    }
}

fn check_profile(at: &str, t: &toml::Table, errors: &mut Vec<HwError>) -> Option<Profile> {
    let before = errors.len();
    for k in t.keys() {
        if !PROFILE_KEYS.contains(&k.as_str()) {
            errors.push(HwError::UnknownField { field: k.clone(), at: at.to_string() });
        }
    }

    let mut cpu_level = BTreeMap::new();
    match t.get("cpu_level") {
        None => errors.push(bad(at, "cpu_level", "is missing")),
        Some(toml::Value::Table(ct)) => {
            let cat = format!("{at}.cpu_level");
            for (k, v) in ct {
                let Some(arch) = Arch::from_name(k) else {
                    errors.push(HwError::UnknownField { field: k.clone(), at: cat.clone() });
                    continue;
                };
                match v.as_str().and_then(|s| CpuLevel::from_name(arch, s)) {
                    Some(level) => {
                        cpu_level.insert(arch, level);
                    }
                    None => {
                        let allowed = match arch {
                            Arch::X86_64 => "one of x86-64-v2, x86-64-v3",
                            Arch::Aarch64 => "one of armv8.0, armv8.2",
                        };
                        errors.push(bad(&cat, k, &format!("is not {allowed}")));
                    }
                }
            }
            if ct.is_empty() {
                errors.push(bad(at, "cpu_level", "names no architecture"));
            }
        }
        Some(_) => errors.push(bad(at, "cpu_level", "is not a table")),
    }

    let cores = uint(t, at, "cores", 1, errors);
    let ram_gib = uint(t, at, "ram_gib", 0, errors);
    let gpu_class = match t.get("gpu_class") {
        None => {
            errors.push(bad(at, "gpu_class", "is missing"));
            None
        }
        Some(v) => {
            let c = v.as_str().and_then(GpuClass::from_name);
            if c.is_none() {
                errors.push(bad(at, "gpu_class", "is not one of software, integrated, discrete_entry, discrete_mid"));
            }
            c
        }
    };
    let vram_gib = uint(t, at, "vram_gib", 0, errors);
    let storage_free_gib = uint(t, at, "storage_free_gib", 0, errors);
    let display = match t.get("display") {
        None => {
            errors.push(bad(at, "display", "is missing"));
            None
        }
        Some(toml::Value::Array(a)) => {
            let dims: Vec<u32> = a
                .iter()
                .filter_map(|v| v.as_integer().and_then(|n| u32::try_from(n).ok()).filter(|&n| n >= 1))
                .collect();
            match dims.as_slice() {
                [w, h] if a.len() == 2 => Some((*w, *h)),
                _ => {
                    errors.push(bad(at, "display", "is not [width, height] in pixels, each at least 1"));
                    None
                }
            }
        }
        Some(_) => {
            errors.push(bad(at, "display", "is not [width, height] in pixels, each at least 1"));
            None
        }
    };

    if errors.len() != before {
        return None;
    }
    match (cores, ram_gib, gpu_class, vram_gib, storage_free_gib, display) {
        (Some(cores), Some(ram_gib), Some(gpu_class), Some(vram_gib), Some(storage_free_gib), Some(display)) => {
            Some(Profile { cpu_level, cores, ram_gib, gpu_class, vram_gib, storage_free_gib, display })
        }
        _ => None,
    }
}
