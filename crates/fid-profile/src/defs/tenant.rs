//! The tenant parameter file (REQ-TRUST.PROFILE.DEFS-004, -005).

use fid_trust_canon::{parse_with_limits, CanonError, Limits, Value};

use super::{
    cut, MeaningCode, ProfileError, ProfileName, TenantMeaning, MAX_INPUT_BYTES, MAX_JSON_DEPTH, MAX_REQUIRED_MEANINGS,
    MAX_TENANT_MEANINGS, REGULATED_MANDATORY_MEANING, TENANT_MEANING_KEYS,
};

/// Validated tenant parameters for one built-in profile. Tenant meanings are held sorted by
/// code; `required_meanings` is present only for Regulated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantParams {
    profile: ProfileName,
    tenant_meanings: Vec<TenantMeaning>,
    required_meanings: Option<Vec<MeaningCode>>,
}

impl TenantParams {
    /// No tenant parameters: instantiation yields the plain built-in.
    pub fn none(profile: ProfileName) -> TenantParams {
        TenantParams { profile, tenant_meanings: Vec::new(), required_meanings: None }
    }

    /// Checked constructor.
    ///
    /// - `required_meanings` on a profile other than Regulated: `profile.param_forbidden`.
    /// - More than [`MAX_TENANT_MEANINGS`] tenant meanings, or one code twice:
    ///   `profile.param_invalid`.
    /// - A `required_meanings` list that is longer than [`MAX_REQUIRED_MEANINGS`], repeats a
    ///   code, names a code that is neither built in nor one of `tenant_meanings`, or omits
    ///   `APPROVED`: `profile.param_invalid`.
    pub fn new(
        profile: ProfileName,
        mut tenant_meanings: Vec<TenantMeaning>,
        required_meanings: Option<Vec<MeaningCode>>,
    ) -> Result<TenantParams, ProfileError> {
        if required_meanings.is_some() && profile != ProfileName::Regulated {
            return Err(ProfileError::forbidden("required_meanings"));
        }
        if tenant_meanings.len() > MAX_TENANT_MEANINGS {
            return Err(too_many("tenant_meanings", tenant_meanings.len(), MAX_TENANT_MEANINGS));
        }
        tenant_meanings.sort_by(|a, b| a.code().cmp(b.code()));
        for pair in tenant_meanings.windows(2) {
            if pair[0].code() == pair[1].code() {
                return Err(ProfileError::invalid(
                    "tenant_meanings",
                    format!("code {} is defined twice", pair[0].code().as_str()),
                ));
            }
        }
        if let Some(seq) = &required_meanings {
            check_sequence(seq, &tenant_meanings)?;
        }
        Ok(TenantParams { profile, tenant_meanings, required_meanings })
    }

    /// Parses a tenant parameter file for `profile`.
    ///
    /// Input larger than [`MAX_INPUT_BYTES`], nested deeper than [`MAX_JSON_DEPTH`], not
    /// JSON, holding a repeated key at any depth, or not an object is `profile.malformed`.
    /// Every key is then checked before any value: a key other than `tenant_meanings`, or
    /// `required_meanings` on a profile other than Regulated, is `profile.param_forbidden`
    /// (the first such key in file order is named). Values of the wrong shape are
    /// `profile.param_invalid`, as are the rules of [`TenantParams::new`].
    pub fn parse(profile: ProfileName, input: &[u8]) -> Result<TenantParams, ProfileError> {
        let members = match parse_json(input)? {
            Value::Object(members) => members,
            _ => return Err(ProfileError::malformed("a tenant parameter file must be a JSON object")),
        };
        for (key, _) in &members {
            let allowed = key == "tenant_meanings" || (key == "required_meanings" && profile == ProfileName::Regulated);
            if !allowed {
                return Err(ProfileError::forbidden(key));
            }
        }
        let mut tenant_meanings = Vec::new();
        let mut required_meanings = None;
        for (key, value) in &members {
            if key == "tenant_meanings" {
                tenant_meanings = parse_tenant_meanings(value)?;
            } else {
                required_meanings = Some(parse_codes(value)?);
            }
        }
        TenantParams::new(profile, tenant_meanings, required_meanings)
    }

    /// The built-in these parameters apply to.
    pub fn profile(&self) -> ProfileName {
        self.profile
    }

    /// Tenant meanings, sorted by code.
    pub fn tenant_meanings(&self) -> &[TenantMeaning] {
        &self.tenant_meanings
    }

    /// The Regulated meaning sequence, if the tenant set one.
    pub fn required_meanings(&self) -> Option<&[MeaningCode]> {
        self.required_meanings.as_deref()
    }
}

pub(crate) fn parse_json(input: &[u8]) -> Result<Value, ProfileError> {
    parse_with_limits(input, Limits { max_bytes: MAX_INPUT_BYTES, max_depth: MAX_JSON_DEPTH }).map_err(|e| match e {
        CanonError::Invalid { kind, offset } => {
            ProfileError::malformed(format!("not acceptable JSON: {} at byte {offset}", kind.name()))
        }
        other => ProfileError::malformed(other.to_string()),
    })
}

fn too_many(key: &'static str, n: usize, max: usize) -> ProfileError {
    ProfileError::invalid(key, format!("{n} entries, at most {max} allowed"))
}

fn check_sequence(seq: &[MeaningCode], tenant: &[TenantMeaning]) -> Result<(), ProfileError> {
    const KEY: &str = "required_meanings";
    if seq.len() > MAX_REQUIRED_MEANINGS {
        return Err(too_many(KEY, seq.len(), MAX_REQUIRED_MEANINGS));
    }
    for (i, code) in seq.iter().enumerate() {
        if seq[..i].contains(code) {
            return Err(ProfileError::invalid(KEY, format!("code {} appears twice", code.as_str())));
        }
        let known = code.is_builtin() || tenant.iter().any(|m| m.code() == code);
        if !known {
            return Err(ProfileError::invalid(KEY, format!("code {} is not in the meaning catalogue", code.as_str())));
        }
    }
    if !seq.iter().any(|c| c.as_str() == REGULATED_MANDATORY_MEANING) {
        return Err(ProfileError::invalid(KEY, format!("the sequence must include {REGULATED_MANDATORY_MEANING}")));
    }
    Ok(())
}

fn parse_tenant_meanings(value: &Value) -> Result<Vec<TenantMeaning>, ProfileError> {
    const KEY: &str = "tenant_meanings";
    let items = match value {
        Value::Array(items) => items,
        _ => return Err(ProfileError::invalid(KEY, "must be an array")),
    };
    if items.len() > MAX_TENANT_MEANINGS {
        return Err(too_many(KEY, items.len(), MAX_TENANT_MEANINGS));
    }
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let members = match item {
            Value::Object(members) => members,
            _ => return Err(ProfileError::invalid(KEY, "each entry must be an object")),
        };
        let mut fields: [Option<&str>; 3] = [None; 3];
        for (k, v) in members {
            let Some(slot) = TENANT_MEANING_KEYS.iter().position(|t| t == k) else {
                return Err(ProfileError::invalid(KEY, format!("unknown member {:?} in an entry", cut(k))));
            };
            match v {
                Value::String(s) => fields[slot] = Some(s),
                _ => return Err(ProfileError::invalid(KEY, format!("member {k} must be a string"))),
            }
        }
        // TENANT_MEANING_KEYS order: code, description, label.
        match fields {
            [Some(code), Some(description), Some(label)] => out.push(TenantMeaning::new(code, label, description)?),
            _ => return Err(ProfileError::invalid(KEY, "each entry needs code, label and description")),
        }
    }
    Ok(out)
}

fn parse_codes(value: &Value) -> Result<Vec<MeaningCode>, ProfileError> {
    const KEY: &str = "required_meanings";
    let items = match value {
        Value::Array(items) => items,
        _ => return Err(ProfileError::invalid(KEY, "must be an array")),
    };
    if items.len() > MAX_REQUIRED_MEANINGS {
        return Err(too_many(KEY, items.len(), MAX_REQUIRED_MEANINGS));
    }
    items
        .iter()
        .map(|v| match v {
            Value::String(s) => MeaningCode::new(s).map_err(|_| {
                ProfileError::invalid(KEY, format!("{:?} is not a meaning code (^[A-Z][A-Z0-9_]{{1,31}}$)", cut(s)))
            }),
            _ => Err(ProfileError::invalid(KEY, "entries must be strings")),
        })
        .collect()
}
