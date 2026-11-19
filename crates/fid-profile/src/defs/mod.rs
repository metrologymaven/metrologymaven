//! Built-in profile definitions, schema and digest (spec node `TRUST.PROFILE.DEFS`).
//!
//! - [`ProfileName`]: exactly three built-ins, `lean`, `regulated` and `assistive_only`
//!   (REQ-TRUST.PROFILE.DEFS-001), each at [`PROFILE_VERSION`].
//! - [`Params`]: the complete parameter set (REQ-TRUST.PROFILE.DEFS-002) with the built-in
//!   values of REQ-TRUST.PROFILE.DEFS-003. Fields are private; a [`Params`] exists only as a
//!   built-in or as a built-in merged with validated [`TenantParams`].
//! - [`TenantParams`]: the tenant parameter file. Only `tenant_meanings` (every profile) and
//!   `required_meanings` (Regulated only) may be set; any other key is
//!   `profile.param_forbidden` (REQ-TRUST.PROFILE.DEFS-004). A Regulated sequence without
//!   `APPROVED`, or naming a code outside the catalogue, is `profile.param_invalid`
//!   (REQ-TRUST.PROFILE.DEFS-005). Duplicate keys and unparseable input are
//!   `profile.malformed`.
//! - [`CanonicalProfile`]: the instantiated document as RFC 8785 canonical JSON and its
//!   SHA-256 digest (REQ-TRUST.PROFILE.DEFS-006), written into packages at
//!   [`PROFILE_MEMBER_PATH`]; [`CanonicalProfile::read`] reads such a member back and fails
//!   closed unless it is exactly what instantiation would produce.
//!
//! The published schemas are `schema/profile-v1.schema.json` (document) and
//! `schema/profile-tenant-v1.schema.json` (tenant file); `tests/schema.rs` checks that they
//! and the constants here agree. Everything is D0 and free of OS and network access.

mod doc;
mod tenant;

use std::fmt;

pub use doc::{instantiate, CanonicalProfile};
pub use tenant::TenantParams;

/// Version of every built-in definition (the `version` member of the document).
pub const PROFILE_VERSION: u32 = 1;
/// Package member that holds the canonical profile document (consumed by `TRUST.PKG.CONTENT`).
pub const PROFILE_MEMBER_PATH: &str = "profile/profile.json";
/// `$id` of the published document schema.
pub const SCHEMA_ID: &str = "https://metrologymaven.org/schema/profile-v1.schema.json";
/// `$id` of the published tenant parameter file schema.
pub const TENANT_SCHEMA_ID: &str = "https://metrologymaven.org/schema/profile-tenant-v1.schema.json";

/// Built-in signature meaning codes of the catalogue (`TRUST.ESIG.MEANING`), in catalogue order.
pub const BUILTIN_MEANINGS: [&str; 4] = ["AUTHORED", "REVIEWED", "APPROVED", "RELEASED"];
/// The meaning a Regulated `required_meanings` list must contain (REQ-TRUST.PROFILE.DEFS-005).
pub const REGULATED_MANDATORY_MEANING: &str = "APPROVED";
/// Syntax of every meaning code (REQ-TRUST.ESIG.MEANING-002).
pub const MEANING_CODE_PATTERN: &str = "^[A-Z][A-Z0-9_]{1,31}$";
/// Syntax of a tenant-defined meaning code: a meaning code starting with `X_`.
pub const TENANT_CODE_PATTERN: &str = "^X_[A-Z0-9_]{0,30}$";
/// Labels and descriptions: no C0 or C1 control character and no DEL.
pub const TEXT_PATTERN: &str = "^[^\\u0000-\\u001f\\u007f-\\u009f]*$";
/// Longest `required_meanings` list: a signature sequence holds at most 8 signatures
/// (REQ-TRUST.ESIG.MULTI-001).
pub const MAX_REQUIRED_MEANINGS: usize = 8;
/// Most tenant meanings in one profile.
pub const MAX_TENANT_MEANINGS: usize = 32;
/// Longest tenant meaning label, in Unicode scalar values.
pub const MAX_LABEL_CHARS: usize = 64;
/// Longest tenant meaning description, in Unicode scalar values.
pub const MAX_DESCRIPTION_CHARS: usize = 400;
/// Largest accepted tenant parameter file or profile document, in bytes.
pub const MAX_INPUT_BYTES: usize = 128 * 1024;
/// Deepest accepted JSON nesting in a tenant file or document (documents need 4).
pub const MAX_JSON_DEPTH: usize = 8;

/// Members of `params`, in canonical (sorted) order.
pub const PARAM_KEYS: [&str; 6] = [
    "agent_writes",
    "allow_software_keys",
    "program_release_by_other",
    "release_requires_signatures",
    "required_meanings",
    "tenant_meanings",
];
/// Keys a tenant parameter file may contain (`required_meanings` for Regulated only).
pub const TENANT_KEYS: [&str; 2] = ["required_meanings", "tenant_meanings"];
/// Members of one tenant meaning, in canonical order.
pub const TENANT_MEANING_KEYS: [&str; 3] = ["code", "description", "label"];
/// Top-level members of the document, in canonical order.
pub const DOCUMENT_KEYS: [&str; 3] = ["name", "params", "version"];

/// Typed errors. [`ProfileError::code`] gives the stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileError {
    /// `profile.malformed`: not acceptable JSON (syntax, UTF-8, duplicate key, size or
    /// depth limit), not an object, or a document that is not exactly the canonical form
    /// instantiation produces.
    Malformed { reason: String },
    /// `profile.param_forbidden`: a key a tenant may not set on this profile. `key` is the
    /// offending key, cut to at most 64 characters.
    ParamForbidden { key: String },
    /// `profile.param_invalid`: an allowed parameter with a value outside its rules.
    ParamInvalid { key: &'static str, reason: String },
}

impl ProfileError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            ProfileError::Malformed { .. } => "profile.malformed",
            ProfileError::ParamForbidden { .. } => "profile.param_forbidden",
            ProfileError::ParamInvalid { .. } => "profile.param_invalid",
        }
    }

    pub(crate) fn malformed(reason: impl Into<String>) -> ProfileError {
        ProfileError::Malformed { reason: reason.into() }
    }

    pub(crate) fn forbidden(key: &str) -> ProfileError {
        ProfileError::ParamForbidden { key: key.chars().take(64).collect() }
    }

    pub(crate) fn invalid(key: &'static str, reason: impl Into<String>) -> ProfileError {
        ProfileError::ParamInvalid { key, reason: reason.into() }
    }
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProfileError::Malformed { reason } => write!(f, "profile.malformed: {reason}"),
            ProfileError::ParamForbidden { key } => {
                write!(f, "profile.param_forbidden: a tenant may not set {key:?} on this profile")
            }
            ProfileError::ParamInvalid { key, reason } => write!(f, "profile.param_invalid: {key}: {reason}"),
        }
    }
}

impl std::error::Error for ProfileError {}

/// The three built-in governance profiles (REQ-TRUST.PROFILE.DEFS-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProfileName {
    Lean,
    Regulated,
    AssistiveOnly,
}

impl ProfileName {
    /// Every built-in, in schema order.
    pub const ALL: [ProfileName; 3] = [ProfileName::Lean, ProfileName::Regulated, ProfileName::AssistiveOnly];

    /// Wire name: `lean`, `regulated` or `assistive_only`.
    pub fn as_str(self) -> &'static str {
        match self {
            ProfileName::Lean => "lean",
            ProfileName::Regulated => "regulated",
            ProfileName::AssistiveOnly => "assistive_only",
        }
    }

    /// The built-in with this wire name, if any (exact, case-sensitive).
    pub fn from_wire(s: &str) -> Option<ProfileName> {
        ProfileName::ALL.into_iter().find(|n| n.as_str() == s)
    }
}

impl fmt::Display for ProfileName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where agent principals may write (`agent_writes`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AgentWrites {
    /// Agents write W1/W2 on their agent branch.
    Branch,
    /// Agents may only create or update proposals.
    ProposalsOnly,
}

impl AgentWrites {
    /// Every value, in schema order.
    pub const ALL: [AgentWrites; 2] = [AgentWrites::Branch, AgentWrites::ProposalsOnly];

    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            AgentWrites::Branch => "branch",
            AgentWrites::ProposalsOnly => "proposals_only",
        }
    }

    /// The value with this wire name, if any.
    pub fn from_wire(s: &str) -> Option<AgentWrites> {
        AgentWrites::ALL.into_iter().find(|n| n.as_str() == s)
    }
}

/// When a program must be released by a human other than its requester
/// (`program_release_by_other`, D-008).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProgramReleaseByOther {
    /// Only for agent-authored programs.
    AgentAuthored,
    /// For every program.
    Always,
}

impl ProgramReleaseByOther {
    /// Every value, in schema order.
    pub const ALL: [ProgramReleaseByOther; 2] = [ProgramReleaseByOther::AgentAuthored, ProgramReleaseByOther::Always];

    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            ProgramReleaseByOther::AgentAuthored => "agent_authored",
            ProgramReleaseByOther::Always => "always",
        }
    }

    /// The value with this wire name, if any.
    pub fn from_wire(s: &str) -> Option<ProgramReleaseByOther> {
        ProgramReleaseByOther::ALL.into_iter().find(|n| n.as_str() == s)
    }
}

/// A signature meaning code matching [`MEANING_CODE_PATTERN`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MeaningCode(String);

impl MeaningCode {
    /// Checks the syntax of `s`; anything else is `profile.param_invalid`.
    pub fn new(s: &str) -> Result<MeaningCode, ProfileError> {
        if is_meaning_code(s) {
            Ok(MeaningCode(s.to_string()))
        } else {
            Err(ProfileError::invalid("code", format!("{:?} does not match {MEANING_CODE_PATTERN}", cut(s))))
        }
    }

    /// For the trusted constants of this module only (checked by a unit test).
    fn known(s: &'static str) -> MeaningCode {
        MeaningCode(s.to_string())
    }

    /// The code text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether this is one of [`BUILTIN_MEANINGS`].
    pub fn is_builtin(&self) -> bool {
        BUILTIN_MEANINGS.contains(&self.0.as_str())
    }

    /// Whether this is a tenant code (prefix `X_`).
    pub fn is_tenant(&self) -> bool {
        self.0.starts_with("X_")
    }
}

impl fmt::Display for MeaningCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn is_meaning_code(s: &str) -> bool {
    let b = s.as_bytes();
    (2..=32).contains(&b.len())
        && b[0].is_ascii_uppercase()
        && b[1..].iter().all(|&c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')
}

fn is_control(c: char) -> bool {
    let u = u32::from(c);
    u < 0x20 || (0x7f..=0x9f).contains(&u)
}

/// At most 40 characters of hostile text, for error messages.
pub(crate) fn cut(s: &str) -> String {
    s.chars().take(40).collect()
}

fn check_text(key: &'static str, s: &str, max: usize) -> Result<(), ProfileError> {
    let n = s.chars().count();
    if n == 0 {
        return Err(ProfileError::invalid(key, "must not be empty"));
    }
    if n > max {
        return Err(ProfileError::invalid(key, format!("{n} characters, at most {max} allowed")));
    }
    if s.chars().any(is_control) {
        return Err(ProfileError::invalid(key, "contains a control character"));
    }
    Ok(())
}

/// A tenant-defined signature meaning (code with prefix `X_`, label, description).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TenantMeaning {
    code: MeaningCode,
    label: String,
    description: String,
}

impl TenantMeaning {
    /// Checks the code ([`TENANT_CODE_PATTERN`]), the label (1 to [`MAX_LABEL_CHARS`]
    /// characters) and the description (1 to [`MAX_DESCRIPTION_CHARS`]); neither text may
    /// hold a control character. Failures are `profile.param_invalid`.
    pub fn new(code: &str, label: &str, description: &str) -> Result<TenantMeaning, ProfileError> {
        let code = MeaningCode::new(code).map_err(|_| {
            ProfileError::invalid(
                "tenant_meanings",
                format!("code {:?} does not match {TENANT_CODE_PATTERN}", cut(code)),
            )
        })?;
        if !code.is_tenant() {
            return Err(ProfileError::invalid(
                "tenant_meanings",
                format!("code {:?} does not start with X_ (built-in codes cannot be redefined)", code.as_str()),
            ));
        }
        check_text("tenant_meanings", label, MAX_LABEL_CHARS)
            .map_err(|e| prefix(e, &format!("label of {}", code.as_str())))?;
        check_text("tenant_meanings", description, MAX_DESCRIPTION_CHARS)
            .map_err(|e| prefix(e, &format!("description of {}", code.as_str())))?;
        Ok(TenantMeaning { code, label: label.to_string(), description: description.to_string() })
    }

    /// The `X_` code.
    pub fn code(&self) -> &MeaningCode {
        &self.code
    }

    /// The display label.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// The one-sentence description.
    pub fn description(&self) -> &str {
        &self.description
    }
}

fn prefix(e: ProfileError, what: &str) -> ProfileError {
    match e {
        ProfileError::ParamInvalid { key, reason } => {
            ProfileError::ParamInvalid { key, reason: format!("{what}: {reason}") }
        }
        other => other,
    }
}

/// The complete parameter set of a profile (REQ-TRUST.PROFILE.DEFS-002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Params {
    required_meanings: Vec<MeaningCode>,
    release_requires_signatures: bool,
    allow_software_keys: bool,
    agent_writes: AgentWrites,
    program_release_by_other: ProgramReleaseByOther,
    tenant_meanings: Vec<TenantMeaning>,
}

impl Params {
    /// The built-in values. REQ-TRUST.PROFILE.DEFS-003 and the leaf's open question fix the
    /// Regulated signing values, Lean's signatures and software keys, and Assistive-only's
    /// `agent_writes`. The rest follow REQ-TRUST.PROFILE.GATE.QUERY-004 (only those gates are
    /// restrictive; Lean is permissive) and REQ-CMM.EXEC.TICKET-002 (release by another human
    /// always under Regulated, otherwise for agent-authored programs); the derivation is
    /// written out in `fixtures/profile/make_fixtures.py`.
    pub fn builtin(name: ProfileName) -> Params {
        match name {
            ProfileName::Lean => Params {
                required_meanings: Vec::new(),
                release_requires_signatures: false,
                allow_software_keys: true,
                agent_writes: AgentWrites::Branch,
                program_release_by_other: ProgramReleaseByOther::AgentAuthored,
                tenant_meanings: Vec::new(),
            },
            ProfileName::Regulated => Params {
                required_meanings: vec![MeaningCode::known("AUTHORED"), MeaningCode::known("APPROVED")],
                release_requires_signatures: true,
                allow_software_keys: false,
                agent_writes: AgentWrites::Branch,
                program_release_by_other: ProgramReleaseByOther::Always,
                tenant_meanings: Vec::new(),
            },
            ProfileName::AssistiveOnly => Params {
                required_meanings: Vec::new(),
                release_requires_signatures: false,
                allow_software_keys: true,
                agent_writes: AgentWrites::ProposalsOnly,
                program_release_by_other: ProgramReleaseByOther::AgentAuthored,
                tenant_meanings: Vec::new(),
            },
        }
    }

    /// Signature meanings required before release, in the required order.
    pub fn required_meanings(&self) -> &[MeaningCode] {
        &self.required_meanings
    }

    /// Whether releasing a Result Package needs the required signatures.
    pub fn release_requires_signatures(&self) -> bool {
        self.release_requires_signatures
    }

    /// Whether software-held signing keys are accepted.
    pub fn allow_software_keys(&self) -> bool {
        self.allow_software_keys
    }

    /// Where agents may write.
    pub fn agent_writes(&self) -> AgentWrites {
        self.agent_writes
    }

    /// When program release needs a different human.
    pub fn program_release_by_other(&self) -> ProgramReleaseByOther {
        self.program_release_by_other
    }

    /// Tenant meanings, sorted by code.
    pub fn tenant_meanings(&self) -> &[TenantMeaning] {
        &self.tenant_meanings
    }
}

/// An instantiated profile document: `{ name, version, params }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileDoc {
    name: ProfileName,
    version: u32,
    params: Params,
}

impl ProfileDoc {
    /// The built-in with no tenant parameters.
    pub fn builtin(name: ProfileName) -> ProfileDoc {
        ProfileDoc { name, version: PROFILE_VERSION, params: Params::builtin(name) }
    }

    /// The built-in named by `tenant` with its tenant parameters merged in. `tenant` was
    /// validated when it was built, so this cannot fail.
    pub fn instantiate(tenant: &TenantParams) -> ProfileDoc {
        let mut doc = ProfileDoc::builtin(tenant.profile());
        doc.params.tenant_meanings = tenant.tenant_meanings().to_vec();
        if let Some(seq) = tenant.required_meanings() {
            doc.params.required_meanings = seq.to_vec();
        }
        doc
    }

    /// Which built-in this is.
    pub fn name(&self) -> ProfileName {
        self.name
    }

    /// Definition version ([`PROFILE_VERSION`]).
    pub fn version(&self) -> u32 {
        self.version
    }

    /// The parameter set.
    pub fn params(&self) -> &Params {
        &self.params
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_constants_are_valid_codes() {
        for c in BUILTIN_MEANINGS {
            assert!(is_meaning_code(c), "{c}");
            assert!(!c.starts_with("X_"), "{c}");
        }
        for n in ProfileName::ALL {
            for c in Params::builtin(n).required_meanings() {
                assert!(c.is_builtin(), "{c}");
            }
        }
        assert!(BUILTIN_MEANINGS.contains(&REGULATED_MANDATORY_MEANING));
    }

    #[test]
    fn code_syntax_edges() {
        assert!(MeaningCode::new("AB").is_ok());
        assert!(MeaningCode::new(&format!("A{}", "B".repeat(31))).is_ok());
        assert!(MeaningCode::new(&format!("A{}", "B".repeat(32))).is_err());
        assert!(MeaningCode::new("A").is_err());
        assert!(MeaningCode::new("1A").is_err());
        assert!(MeaningCode::new("_A").is_err());
        assert!(MeaningCode::new("AÉ").is_err());
        assert!(MeaningCode::new("").is_err());
        assert!(TenantMeaning::new("X_", "l", "d").is_ok());
        assert!(TenantMeaning::new("XA", "l", "d").is_err());
    }

    #[test]
    fn text_rules_count_scalar_values() {
        let label = "é".repeat(MAX_LABEL_CHARS);
        assert!(TenantMeaning::new("X_A", &label, "d").is_ok());
        assert!(TenantMeaning::new("X_A", &format!("{label}é"), "d").is_err());
        assert!(TenantMeaning::new("X_A", "a\u{85}b", "d").is_err());
        assert!(TenantMeaning::new("X_A", "a\u{7f}b", "d").is_err());
        assert!(TenantMeaning::new("X_A", "a\u{a0}b", "d").is_ok());
        assert!(TenantMeaning::new("X_A", "l", "").is_err());
    }

    #[test]
    fn forbidden_key_is_cut() {
        let long = "k".repeat(1000);
        let e = ProfileError::forbidden(&long);
        assert!(matches!(&e, ProfileError::ParamForbidden { key } if key.chars().count() == 64), "{e:?}");
    }

    #[test]
    fn wire_names_round_trip() {
        for n in ProfileName::ALL {
            assert_eq!(ProfileName::from_wire(n.as_str()), Some(n));
        }
        assert_eq!(ProfileName::from_wire("Lean"), None);
        for a in AgentWrites::ALL {
            assert_eq!(AgentWrites::from_wire(a.as_str()), Some(a));
        }
        for p in ProgramReleaseByOther::ALL {
            assert_eq!(ProgramReleaseByOther::from_wire(p.as_str()), Some(p));
        }
    }
}
