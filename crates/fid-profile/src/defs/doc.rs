//! The canonical profile document and its digest (REQ-TRUST.PROFILE.DEFS-006).

use fid_trust_canon::{digest_canonical, serialize, Value};
use fid_types::Digest;

use super::tenant::parse_json;
use super::{
    MeaningCode, Params, ProfileDoc, ProfileError, ProfileName, TenantMeaning, TenantParams, DOCUMENT_KEYS, PARAM_KEYS,
    PROFILE_MEMBER_PATH, PROFILE_VERSION,
};

/// An instantiated profile with its RFC 8785 canonical bytes and their SHA-256 digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalProfile {
    doc: ProfileDoc,
    bytes: Vec<u8>,
    digest: Digest,
}

/// Instantiates built-in `profile` with an optional tenant parameter file: start from the
/// built-in, merge the allowed tenant keys, validate, canonicalise (RFC 8785) and hash.
/// Errors are those of [`TenantParams::parse`].
pub fn instantiate(profile: ProfileName, tenant_file: Option<&[u8]>) -> Result<CanonicalProfile, ProfileError> {
    let tenant = match tenant_file {
        Some(bytes) => TenantParams::parse(profile, bytes)?,
        None => TenantParams::none(profile),
    };
    CanonicalProfile::new(ProfileDoc::instantiate(&tenant))
}

impl CanonicalProfile {
    /// Canonicalises and hashes `doc`.
    pub fn new(doc: ProfileDoc) -> Result<CanonicalProfile, ProfileError> {
        let bytes = serialize(&to_value(&doc))
            .map_err(|e| ProfileError::malformed(format!("cannot canonicalise the document: {e}")))?;
        let digest = digest_canonical(&bytes);
        Ok(CanonicalProfile { doc, bytes, digest })
    }

    /// The plain built-in, canonicalised.
    pub fn builtin(name: ProfileName) -> Result<CanonicalProfile, ProfileError> {
        CanonicalProfile::new(ProfileDoc::builtin(name))
    }

    /// Reads a profile document, e.g. the [`PROFILE_MEMBER_PATH`] member of a package, and
    /// fails closed. The input must be a document this crate could have produced: a known
    /// built-in at [`PROFILE_VERSION`] whose non-tenant parameters equal the built-in's
    /// (otherwise `profile.param_forbidden`), whose tenant parameters pass
    /// [`TenantParams::new`] (otherwise `profile.param_invalid`), and whose bytes are exactly
    /// the canonical form (otherwise `profile.malformed`).
    pub fn read(input: &[u8]) -> Result<CanonicalProfile, ProfileError> {
        let top = object(parse_json(input)?, "the document")?;
        expect_keys(&top, &DOCUMENT_KEYS, "the document")?;
        let name = match member(&top, "name") {
            Some(Value::String(s)) => ProfileName::from_wire(s)
                .ok_or_else(|| ProfileError::malformed(format!("unknown profile name {:?}", super::cut(s))))?,
            _ => return Err(ProfileError::malformed("name must be a string")),
        };
        match member(&top, "version") {
            Some(Value::Number(v)) if *v == f64::from(PROFILE_VERSION) => {}
            _ => return Err(ProfileError::malformed(format!("version must be {PROFILE_VERSION}"))),
        }
        let params = match member(&top, "params") {
            Some(v) => object(v.clone(), "params")?,
            None => return Err(ProfileError::malformed("params is missing")),
        };
        expect_keys(&params, &PARAM_KEYS, "params")?;
        let builtin = Params::builtin(name);

        // Non-tenant parameters must equal the built-in's: anything else is an override of a
        // parameter no tenant may set.
        let fixed = [
            ("agent_writes", Value::String(builtin.agent_writes().as_str().to_string())),
            ("allow_software_keys", Value::Bool(builtin.allow_software_keys())),
            ("program_release_by_other", Value::String(builtin.program_release_by_other().as_str().to_string())),
            ("release_requires_signatures", Value::Bool(builtin.release_requires_signatures())),
        ];
        for (key, want) in &fixed {
            if member(&params, key) != Some(want) {
                return Err(ProfileError::forbidden(key));
            }
        }
        let builtin_seq = codes_value(builtin.required_meanings());
        let seq = match member(&params, "required_meanings") {
            Some(v) if *v == builtin_seq => None,
            Some(_) if name != ProfileName::Regulated => return Err(ProfileError::forbidden("required_meanings")),
            Some(v) => Some(read_codes(v)?),
            None => return Err(ProfileError::malformed("required_meanings is missing")),
        };
        let tenant_meanings = match member(&params, "tenant_meanings") {
            Some(v) => read_meanings(v)?,
            None => return Err(ProfileError::malformed("tenant_meanings is missing")),
        };
        let tenant = TenantParams::new(name, tenant_meanings, seq)?;
        let out = CanonicalProfile::new(ProfileDoc::instantiate(&tenant))?;
        if out.bytes != input {
            return Err(ProfileError::malformed("the document is not in canonical form"));
        }
        Ok(out)
    }

    /// The instantiated document.
    pub fn document(&self) -> &ProfileDoc {
        &self.doc
    }

    /// RFC 8785 canonical JSON of the document (no trailing newline).
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// SHA-256 of [`CanonicalProfile::bytes`]: the profile digest.
    pub fn digest(&self) -> Digest {
        self.digest
    }

    /// Package member path of the document.
    pub fn member_path(&self) -> &'static str {
        PROFILE_MEMBER_PATH
    }
}

fn text(s: &str) -> Value {
    Value::String(s.to_string())
}

fn codes_value(codes: &[MeaningCode]) -> Value {
    Value::Array(codes.iter().map(|c| text(c.as_str())).collect())
}

fn to_value(doc: &ProfileDoc) -> Value {
    let p = doc.params();
    let meanings = p
        .tenant_meanings()
        .iter()
        .map(|m| {
            Value::Object(vec![
                ("code".to_string(), text(m.code().as_str())),
                ("description".to_string(), text(m.description())),
                ("label".to_string(), text(m.label())),
            ])
        })
        .collect();
    let params = Value::Object(vec![
        ("agent_writes".to_string(), text(p.agent_writes().as_str())),
        ("allow_software_keys".to_string(), Value::Bool(p.allow_software_keys())),
        ("program_release_by_other".to_string(), text(p.program_release_by_other().as_str())),
        ("release_requires_signatures".to_string(), Value::Bool(p.release_requires_signatures())),
        ("required_meanings".to_string(), codes_value(p.required_meanings())),
        ("tenant_meanings".to_string(), Value::Array(meanings)),
    ]);
    Value::Object(vec![
        ("name".to_string(), text(doc.name().as_str())),
        ("params".to_string(), params),
        ("version".to_string(), Value::Number(f64::from(doc.version()))),
    ])
}

fn object(v: Value, what: &str) -> Result<Vec<(String, Value)>, ProfileError> {
    match v {
        Value::Object(m) => Ok(m),
        _ => Err(ProfileError::malformed(format!("{what} must be a JSON object"))),
    }
}

fn member<'a>(m: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    m.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

/// Exactly `keys`, no more (the parser already refused repeated keys).
fn expect_keys(m: &[(String, Value)], keys: &[&str], what: &str) -> Result<(), ProfileError> {
    for (k, _) in m {
        if !keys.contains(&k.as_str()) {
            return Err(ProfileError::malformed(format!("unknown member {:?} in {what}", super::cut(k))));
        }
    }
    for k in keys {
        if member(m, k).is_none() {
            return Err(ProfileError::malformed(format!("{what} lacks member {k}")));
        }
    }
    Ok(())
}

fn read_codes(v: &Value) -> Result<Vec<MeaningCode>, ProfileError> {
    match v {
        Value::Array(items) if items.len() <= super::MAX_REQUIRED_MEANINGS => items
            .iter()
            .map(|i| match i {
                Value::String(s) => MeaningCode::new(s),
                _ => Err(ProfileError::invalid("required_meanings", "entries must be strings")),
            })
            .collect(),
        Value::Array(items) => Err(ProfileError::invalid(
            "required_meanings",
            format!("{} entries, at most {} allowed", items.len(), super::MAX_REQUIRED_MEANINGS),
        )),
        _ => Err(ProfileError::invalid("required_meanings", "must be an array")),
    }
}

fn read_meanings(v: &Value) -> Result<Vec<TenantMeaning>, ProfileError> {
    const KEY: &str = "tenant_meanings";
    let items = match v {
        Value::Array(items) if items.len() <= super::MAX_TENANT_MEANINGS => items,
        Value::Array(items) => return Err(ProfileError::invalid(KEY, format!("{} entries, too many", items.len()))),
        _ => return Err(ProfileError::invalid(KEY, "must be an array")),
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let m = match item {
            Value::Object(m) => m,
            _ => return Err(ProfileError::invalid(KEY, "each entry must be an object")),
        };
        if m.len() != 3 {
            return Err(ProfileError::invalid(KEY, "each entry has exactly code, label and description"));
        }
        match (member(m, "code"), member(m, "label"), member(m, "description")) {
            (Some(Value::String(c)), Some(Value::String(l)), Some(Value::String(d))) => {
                out.push(TenantMeaning::new(c, l, d)?)
            }
            _ => return Err(ProfileError::invalid(KEY, "each entry has exactly code, label and description")),
        }
    }
    Ok(out)
}
