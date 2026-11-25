//! Tier and hardware-profile verdicts for `fid doctor` (spec node `PLAT.DOCTOR.LOOKUP`).
//!
//! [`assess`] is the pure decision step: given an [`EnvSnapshot`] (from `PLAT.DOCTOR.PROBE`),
//! the support [`Matrix`] (`PLAT.MATRIX.FILE`), the [`HwProfiles`] (`PLAT.HW.PROFILE`, optional)
//! and the build's [`Phase`], it returns the effective tier, one verdict per profile with every
//! shortfall named, and one overall [`Outcome`] that maps to an exit code.
//!
//! Determinism D0: no clock, no I/O, no hash-ordered maps. Profiles are visited in name order
//! (`BTreeMap`), fields in the fixed order of [`Field::ALL`], and [`Assessment::to_json`] is
//! RFC 8785 canonical JSON. Nothing here panics; missing inputs become `unknown` or
//! `indeterminate`, never errors.

use crate::probe::{bound_detail, EnvSnapshot, GpuSelection, Probed, UnknownReason, VolumeRole};
use fid_canon::CValue;
use fid_platform_core::hw_profile::{GpuClass, HwProfiles, Profile, MINIMUM, RECOMMENDED};
use fid_platform_core::matrix::{Arch, EffectiveTier, Matrix, Phase};
use std::collections::BTreeMap;
use std::fmt;

/// Assessment JSON schema version (the `schema_version` key of [`Assessment::to_cvalue`]).
pub const SCHEMA_VERSION: u32 = 1;

/// Dedicated VRAM, in GiB, from which a discrete adapter is `discrete_mid` rather than
/// `discrete_entry` (REQ-PLAT.DOCTOR.LOOKUP-004) [unverified: VRAM boundary to be tuned].
pub const DISCRETE_MID_VRAM_GIB: f64 = 6.0;

/// A profile field, in the fixed comparison order of the leaf design.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Field {
    CpuLevel,
    Cores,
    RamGib,
    GpuClass,
    VramGib,
    StorageFreeGib,
    Display,
}

impl Field {
    /// Every field, in comparison (and output) order.
    pub const ALL: [Field; 7] = [
        Field::CpuLevel,
        Field::Cores,
        Field::RamGib,
        Field::GpuClass,
        Field::VramGib,
        Field::StorageFreeGib,
        Field::Display,
    ];

    /// The key used in the profile file and in the assessment JSON.
    pub fn as_str(self) -> &'static str {
        match self {
            Field::CpuLevel => "cpu_level",
            Field::Cores => "cores",
            Field::RamGib => "ram_gib",
            Field::GpuClass => "gpu_class",
            Field::VramGib => "vram_gib",
            Field::StorageFreeGib => "storage_free_gib",
            Field::Display => "display",
        }
    }
}

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A required or found value in a [`Shortfall`].
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A count (physical cores).
    Count(u32),
    /// A size in GiB (RAM, VRAM, free storage); always finite and non-negative.
    Gib(f64),
    /// A name: a CPU instruction-set level or a GPU class.
    Name(String),
    /// A display resolution (width, height) in pixels.
    Resolution(u32, u32),
}

impl Value {
    fn to_cvalue(&self) -> CValue {
        match self {
            Value::Count(n) => CValue::Num(f64::from(*n)),
            Value::Gib(g) => CValue::Num(*g),
            Value::Name(s) => CValue::Str(s.clone()),
            Value::Resolution(w, h) => CValue::Arr(vec![CValue::Num(f64::from(*w)), CValue::Num(f64::from(*h))]),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Count(n) => write!(f, "{n}"),
            Value::Gib(g) => write!(f, "{g}"),
            Value::Name(s) => f.write_str(s),
            Value::Resolution(w, h) => write!(f, "{w}x{h}"),
        }
    }
}

/// One item that falls short of a profile (REQ-PLAT.DOCTOR.LOOKUP-002).
#[derive(Debug, Clone, PartialEq)]
pub struct Shortfall {
    pub field: Field,
    pub required: Value,
    pub found: Value,
}

/// The verdict of one profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Meets,
    Below,
    Indeterminate,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Meets => "meets",
            Verdict::Below => "below",
            Verdict::Indeterminate => "indeterminate",
        }
    }
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why a verdict is `indeterminate` without any comparison having been made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerdictReason {
    /// No profile file is installed (REQ-PLAT.DOCTOR.LOOKUP-006).
    NoProfiles,
    /// The profile file has no profile of this required name (`minimum` or `recommended`);
    /// a validated file always has both, so this only arises for hand-built inputs.
    MissingProfile,
}

impl VerdictReason {
    pub fn as_str(self) -> &'static str {
        match self {
            VerdictReason::NoProfiles => "no_profiles",
            VerdictReason::MissingProfile => "missing_profile",
        }
    }
}

impl fmt::Display for VerdictReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The verdict for one profile (REQ-PLAT.DOCTOR.LOOKUP-002, -003, -006).
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileVerdict {
    pub name: String,
    pub verdict: Verdict,
    /// Set only when no comparison was possible (`indeterminate(no_profiles)` and the like).
    pub reason: Option<VerdictReason>,
    /// Every known item below the profile, in [`Field::ALL`] order; empty unless `below`.
    pub shortfalls: Vec<Shortfall>,
    /// Every required item that could not be compared, in [`Field::ALL`] order. Listed for
    /// `below` verdicts too, so the report can say what was not checked.
    pub unknowns: Vec<Field>,
}

/// The overall outcome (REQ-PLAT.DOCTOR.LOOKUP-005), in order of precedence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    UnsupportedPlatform,
    BelowMinimum,
    Indeterminate,
    MeetsMinimum,
    MeetsRecommended,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::UnsupportedPlatform => "unsupported_platform",
            Outcome::BelowMinimum => "below_minimum",
            Outcome::Indeterminate => "indeterminate",
            Outcome::MeetsMinimum => "meets_minimum",
            Outcome::MeetsRecommended => "meets_recommended",
        }
    }

    /// Process exit code for `fid doctor` (REQ-PLAT.DOCTOR-005).
    pub fn exit_code(self) -> i32 {
        match self {
            Outcome::UnsupportedPlatform => 3,
            Outcome::BelowMinimum => 2,
            Outcome::Indeterminate => 4,
            Outcome::MeetsMinimum => 1,
            Outcome::MeetsRecommended => 0,
        }
    }
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The result of [`assess`].
#[derive(Debug, Clone, PartialEq)]
pub struct Assessment {
    /// The effective tier, or unknown when the OS (family or version) or architecture is.
    pub tier: Probed<EffectiveTier>,
    /// One verdict per profile, in name order.
    pub profiles: Vec<ProfileVerdict>,
    pub outcome: Outcome,
}

/// The profile GPU class a probed adapter belongs to (REQ-PLAT.DOCTOR.LOOKUP-004).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuClassFound {
    /// The class is known.
    Exact(GpuClass),
    /// A discrete adapter whose VRAM was not probed: at least `discrete_entry`, possibly
    /// `discrete_mid`. It meets requirements up to `discrete_entry` and is unknown above.
    AtLeast(GpuClass),
    /// No class can be assigned (virtual or other device types, or no device type).
    Unknown,
}

/// Maps a probed GPU selection to a profile GPU class: fallback or CPU adapter → `software`;
/// integrated → `integrated`; discrete with VRAM below [`DISCRETE_MID_VRAM_GIB`] →
/// `discrete_entry`; discrete with at least that → `discrete_mid`.
pub fn gpu_class(gpu: &GpuSelection) -> GpuClassFound {
    if gpu.fallback || gpu.device_type.as_deref() == Some("cpu") {
        return GpuClassFound::Exact(GpuClass::Software);
    }
    match gpu.device_type.as_deref() {
        Some("integrated") => GpuClassFound::Exact(GpuClass::Integrated),
        Some("discrete") => match gib(gpu.vram_gib) {
            Some(v) if v < DISCRETE_MID_VRAM_GIB => GpuClassFound::Exact(GpuClass::DiscreteEntry),
            Some(_) => GpuClassFound::Exact(GpuClass::DiscreteMid),
            None => GpuClassFound::AtLeast(GpuClass::DiscreteEntry),
        },
        _ => GpuClassFound::Unknown,
    }
}

/// Assesses `snapshot` against the support matrix at `phase` and against every profile in
/// `profiles` (`None` when no profile file is installed).
pub fn assess(snapshot: &EnvSnapshot, matrix: &Matrix, profiles: Option<&HwProfiles>, phase: Phase) -> Assessment {
    let tier = lookup_tier(snapshot, matrix, phase);
    let mut verdicts: BTreeMap<String, ProfileVerdict> = BTreeMap::new();
    match profiles {
        None => {
            for name in [MINIMUM, RECOMMENDED] {
                verdicts.insert(name.to_string(), no_comparison(name, VerdictReason::NoProfiles));
            }
        }
        Some(p) => {
            for (name, profile) in &p.profiles {
                verdicts.insert(name.clone(), compare(name, profile, snapshot));
            }
            for name in [MINIMUM, RECOMMENDED] {
                if !verdicts.contains_key(name) {
                    verdicts.insert(name.to_string(), no_comparison(name, VerdictReason::MissingProfile));
                }
            }
        }
    }
    let verdict_of = |name: &str| verdicts.get(name).map_or(Verdict::Indeterminate, |v| v.verdict);
    let outcome = outcome(&tier, verdict_of(MINIMUM), verdict_of(RECOMMENDED));
    Assessment { tier, profiles: verdicts.into_values().collect(), outcome }
}

fn lookup_tier(snapshot: &EnvSnapshot, matrix: &Matrix, phase: Phase) -> Probed<EffectiveTier> {
    let os = match &snapshot.os {
        Probed::Known(os) => os,
        Probed::Unknown { reason, detail } => return unknown_from("os", *reason, detail),
    };
    let arch = match &snapshot.arch {
        Probed::Known(a) => a,
        Probed::Unknown { reason, detail } => return unknown_from("arch", *reason, detail),
    };
    Probed::Known(matrix.lookup(&os.family, &os.version, arch.as_str(), phase))
}

fn unknown_from<T>(item: &str, reason: UnknownReason, detail: &str) -> Probed<T> {
    Probed::Unknown { reason, detail: bound_detail(&format!("{item}: {detail}")) }
}

/// Precedence of REQ-PLAT.DOCTOR.LOOKUP-005. An unknown tier is not `unsupported_platform`
/// (nothing says the platform is unlisted), but it cannot support a `meets_*` outcome either:
/// it gives `below_minimum` when the minimum is below and `indeterminate` otherwise.
fn outcome(tier: &Probed<EffectiveTier>, minimum: Verdict, recommended: Verdict) -> Outcome {
    let tier_known = match tier {
        Probed::Known(EffectiveTier::Tier(_)) => true,
        Probed::Known(EffectiveTier::Planned(..) | EffectiveTier::NotListed) => return Outcome::UnsupportedPlatform,
        Probed::Unknown { .. } => false,
    };
    match minimum {
        Verdict::Below => Outcome::BelowMinimum,
        Verdict::Indeterminate => Outcome::Indeterminate,
        Verdict::Meets if !tier_known => Outcome::Indeterminate,
        Verdict::Meets if recommended == Verdict::Meets => Outcome::MeetsRecommended,
        Verdict::Meets => Outcome::MeetsMinimum,
    }
}

fn no_comparison(name: &str, reason: VerdictReason) -> ProfileVerdict {
    ProfileVerdict {
        name: name.to_string(),
        verdict: Verdict::Indeterminate,
        reason: Some(reason),
        shortfalls: Vec::new(),
        unknowns: Vec::new(),
    }
}

/// Result of comparing one field.
enum Check {
    Meets,
    Short(Value, Value),
    Unknown,
}

fn compare(name: &str, profile: &Profile, snap: &EnvSnapshot) -> ProfileVerdict {
    let mut shortfalls = Vec::new();
    let mut unknowns = Vec::new();
    for field in Field::ALL {
        match check(field, profile, snap) {
            Check::Meets => {}
            Check::Short(required, found) => shortfalls.push(Shortfall { field, required, found }),
            Check::Unknown => unknowns.push(field),
        }
    }
    let verdict = if !shortfalls.is_empty() {
        Verdict::Below
    } else if !unknowns.is_empty() {
        Verdict::Indeterminate
    } else {
        Verdict::Meets
    };
    ProfileVerdict { name: name.to_string(), verdict, reason: None, shortfalls, unknowns }
}

fn check(field: Field, p: &Profile, snap: &EnvSnapshot) -> Check {
    match field {
        Field::CpuLevel => check_cpu_level(p, snap),
        Field::Cores => match snap.cpu.known() {
            Some(c) if c.physical_cores >= p.cores => Check::Meets,
            Some(c) => Check::Short(Value::Count(p.cores), Value::Count(c.physical_cores)),
            None => Check::Unknown,
        },
        Field::RamGib => check_gib(p.ram_gib, snap.ram.known().and_then(|r| gib(Some(r.total_gib)))),
        Field::GpuClass => check_gpu_class(p.gpu_class, snap.gpu.known().map_or(GpuClassFound::Unknown, gpu_class)),
        Field::VramGib => check_gib(p.vram_gib, snap.gpu.known().and_then(|g| gib(g.vram_gib))),
        Field::StorageFreeGib => {
            let install = snap
                .storage
                .known()
                .and_then(|vols| vols.iter().find(|v| v.role == VolumeRole::Install))
                .and_then(|v| gib(Some(v.free_gib)));
            check_gib(p.storage_free_gib, install)
        }
        Field::Display => match snap.display.known() {
            Some(&(w, h)) if w >= p.display.0 && h >= p.display.1 => Check::Meets,
            Some(&(w, h)) => Check::Short(Value::Resolution(p.display.0, p.display.1), Value::Resolution(w, h)),
            None => Check::Unknown,
        },
    }
}

/// A finite, non-negative GiB value, or `None`.
fn gib(v: Option<f64>) -> Option<f64> {
    v.filter(|g| g.is_finite() && *g >= 0.0)
}

fn check_gib(required: u32, found: Option<f64>) -> Check {
    match found {
        Some(g) if g >= f64::from(required) => Check::Meets,
        Some(g) => Check::Short(Value::Count(required), Value::Gib(g)),
        None => Check::Unknown,
    }
}

fn check_gpu_class(required: GpuClass, found: GpuClassFound) -> Check {
    match found {
        GpuClassFound::Exact(c) if c >= required => Check::Meets,
        GpuClassFound::Exact(c) => Check::Short(Value::Name(required.as_str().into()), Value::Name(c.as_str().into())),
        GpuClassFound::AtLeast(c) if c >= required => Check::Meets,
        GpuClassFound::AtLeast(_) | GpuClassFound::Unknown => Check::Unknown,
    }
}

/// Instruction-set level rank within an architecture. The probe reports `x86-64-v1` and
/// `x86-64-v4` as well as the four profile levels, so they are ranked here.
fn level_rank(arch: Arch, level: &str) -> Option<u8> {
    match (arch, level) {
        (Arch::X86_64, "x86-64-v1") => Some(1),
        (Arch::X86_64, "x86-64-v2") => Some(2),
        (Arch::X86_64, "x86-64-v3") => Some(3),
        (Arch::X86_64, "x86-64-v4") => Some(4),
        (Arch::Aarch64, "armv8.0") => Some(0),
        (Arch::Aarch64, "armv8.2") => Some(2),
        _ => None,
    }
}

fn check_cpu_level(p: &Profile, snap: &EnvSnapshot) -> Check {
    let (Some(arch), Some(cpu)) = (snap.arch.known(), snap.cpu.known()) else {
        return Check::Unknown;
    };
    // An architecture the profile does not name (or that has no profile levels) cannot be compared.
    let Some(arch) = Arch::from_name(arch.as_str()) else { return Check::Unknown };
    let Some(&required) = p.cpu_level.get(&arch) else { return Check::Unknown };
    let found = cpu.instruction_set_level.as_str();
    match (level_rank(arch, required.as_str()), level_rank(arch, found)) {
        (Some(r), Some(f)) if f >= r => Check::Meets,
        (Some(_), Some(_)) => Check::Short(Value::Name(required.as_str().into()), Value::Name(found.into())),
        _ => Check::Unknown,
    }
}

fn cstr(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn tier_to_cvalue(t: &EffectiveTier) -> CValue {
    let mut m = BTreeMap::new();
    match t {
        EffectiveTier::Tier(tier) => {
            m.insert("kind".to_string(), cstr("tier"));
            m.insert("tier".to_string(), cstr(tier.as_str()));
        }
        EffectiveTier::Planned(tier, since) => {
            m.insert("kind".to_string(), cstr("planned"));
            m.insert("since_phase".to_string(), cstr(since.as_str()));
            m.insert("tier".to_string(), cstr(tier.as_str()));
        }
        EffectiveTier::NotListed => {
            m.insert("kind".to_string(), cstr("not_listed"));
        }
    }
    CValue::Obj(m)
}

impl ProfileVerdict {
    fn to_cvalue(&self) -> CValue {
        let shortfalls = self
            .shortfalls
            .iter()
            .map(|s| {
                CValue::Obj(BTreeMap::from([
                    ("field".to_string(), cstr(s.field.as_str())),
                    ("found".to_string(), s.found.to_cvalue()),
                    ("required".to_string(), s.required.to_cvalue()),
                ]))
            })
            .collect();
        CValue::Obj(BTreeMap::from([
            ("name".to_string(), cstr(&self.name)),
            ("reason".to_string(), self.reason.map_or(CValue::Null, |r| cstr(r.as_str()))),
            ("shortfalls".to_string(), CValue::Arr(shortfalls)),
            ("unknowns".to_string(), CValue::Arr(self.unknowns.iter().map(|f| cstr(f.as_str())).collect())),
            ("verdict".to_string(), cstr(self.verdict.as_str())),
        ]))
    }
}

impl Assessment {
    /// The process exit code of the overall outcome.
    pub fn exit_code(&self) -> i32 {
        self.outcome.exit_code()
    }

    /// The verdict for the profile named `name`, if any.
    pub fn profile(&self, name: &str) -> Option<&ProfileVerdict> {
        self.profiles.iter().find(|p| p.name == name)
    }

    /// The assessment as a canonical JSON value tree (`BTreeMap`-ordered keys).
    pub fn to_cvalue(&self) -> CValue {
        let tier = match &self.tier {
            Probed::Known(t) => {
                BTreeMap::from([("status".to_string(), cstr("known")), ("value".to_string(), tier_to_cvalue(t))])
            }
            Probed::Unknown { reason, detail } => BTreeMap::from([
                ("detail".to_string(), cstr(&bound_detail(detail))),
                ("reason".to_string(), cstr(reason.as_str())),
                ("status".to_string(), cstr("unknown")),
            ]),
        };
        CValue::Obj(BTreeMap::from([
            ("exit_code".to_string(), CValue::Num(f64::from(self.exit_code()))),
            ("outcome".to_string(), cstr(self.outcome.as_str())),
            ("profiles".to_string(), CValue::Arr(self.profiles.iter().map(ProfileVerdict::to_cvalue).collect())),
            ("schema_version".to_string(), CValue::Num(f64::from(SCHEMA_VERSION))),
            ("tier".to_string(), CValue::Obj(tier)),
        ]))
    }

    /// RFC 8785 canonical JSON bytes of [`Assessment::to_cvalue`] (AC-6: byte-identical across
    /// runs). Fails only if a hand-built snapshot carries a non-finite number.
    pub fn to_json(&self) -> Result<Vec<u8>, fid_canon::CanonError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue())
    }
}
