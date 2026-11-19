//! The project file of this leaf, `fid.gdt-spec-project/2` (extension `.fidproj`): the
//! project's defaults-profile store plus every governing spec and default.
//!
//! ```json
//! { "schema": "fid.gdt-spec-project/2",
//!   "profile_store": { ..a fid.dprof-store/2 snapshot whose bindings are both {}.. },
//!   "tenant_default": { ..spec.. } | null,
//!   "plan_defaults": { "<plan>": { ..spec.. } },
//!   "characteristics": { "working": { "<char>": { ..spec.. } | null }, "agent": { } } }
//! ```
//!
//! The profile store's bindings are not stored: they mirror the governing specs and are
//! rebuilt on load (each characteristic whose profile resolves on its branch is bound to it).
//! A characteristic whose profile does not resolve still loads; the gate refuses it.

use super::state::{check_loaded_default, check_profile, SpecState, MAX_CHARS, MAX_PLANS};
use super::types::{GoverningSpec, PlanId};
use super::SpecError;
use crate::chars::dprof::profile::integer_literals_only;
use crate::chars::dprof::{Branch, CharId, DprofState, VocabularySource};
use fid_canon::{CValue, CanonError};
use std::collections::BTreeMap;

/// `schema` value of a project file.
pub const PROJECT_SCHEMA: &str = "fid.gdt-spec-project/2";
/// Largest project file accepted, in bytes.
pub const MAX_PROJECT_BYTES: usize = fid_canon::MAX_INPUT_BYTES;
/// Deepest nesting: document, `profile_store`, `profiles`, entry, profile, `keys`, list.
const MAX_PROJECT_DEPTH: usize = 7;
const TOP: [&str; 5] = ["characteristics", "plan_defaults", "profile_store", "schema", "tenant_default"];

type Obj = BTreeMap<String, CValue>;

fn bad(reason: &'static str) -> SpecError {
    SpecError::project(reason)
}

fn obj<'a>(v: Option<&'a CValue>, want: Option<&[&str]>) -> Result<&'a Obj, SpecError> {
    match (v, want) {
        (Some(CValue::Obj(o)), None) => Ok(o),
        (Some(CValue::Obj(o)), Some(w)) if o.len() == w.len() && w.iter().all(|k| o.contains_key(*k)) => Ok(o),
        (Some(CValue::Obj(_)), Some(_)) => Err(bad("bad_keys")),
        _ => Err(bad("wrong_type")),
    }
}

fn empty_bindings() -> CValue {
    let mut m = BTreeMap::new();
    m.insert("agent".to_owned(), CValue::Obj(BTreeMap::new()));
    m.insert("working".to_owned(), CValue::Obj(BTreeMap::new()));
    CValue::Obj(m)
}

fn spec(v: &CValue) -> Result<GoverningSpec, SpecError> {
    GoverningSpec::from_cvalue(v, bad)
}

/// Loads a project file strictly. Checks, in order: size (`project_invalid{too_large}`),
/// JSON with no duplicate key (`project_invalid{duplicate_key}`, else `{json}`; nesting at
/// most 7), integer number literals only (`project_invalid{bad_number}`: so no fraction, no
/// exponent and therefore no subnormal value; JSON has no NaN or infinity), the five fields
/// (`bad_keys`, `wrong_type`), `bad_schema`, the profile store (`store_bindings` unless its
/// bindings are both empty, then the `fid.dprof-store/2` loader with `vocabs`), the tenant
/// default, the plan defaults (`bad_plan_id`, `case_duplicate`, `too_many_plans`), then each
/// branch's characteristics (`bad_char_id`, `case_duplicate`, `too_many_chars`). A default
/// must be `explicit` (`default_not_explicit`) and resolve on the working branch. Specs are
/// read with [`GoverningSpec::from_cvalue`], so a missing field is
/// `governing_spec_missing{field}`. Returns the governing specs and the profile store with
/// its bindings rebuilt.
pub fn load_project(bytes: &[u8], vocabs: &dyn VocabularySource) -> Result<(SpecState, DprofState), SpecError> {
    if bytes.len() > MAX_PROJECT_BYTES {
        return Err(bad("too_large"));
    }
    let v = fid_canon::parse_with_limits(bytes, MAX_PROJECT_BYTES, MAX_PROJECT_DEPTH).map_err(|e| match e {
        CanonError::DuplicateKey(_) => bad("duplicate_key"),
        _ => bad("json"),
    })?;
    if !integer_literals_only(bytes) {
        return Err(bad("bad_number"));
    }
    let top = obj(Some(&v), Some(&TOP))?;
    if top.get("schema") != Some(&CValue::Str(PROJECT_SCHEMA.to_owned())) {
        return Err(bad("bad_schema"));
    }
    let store_v = top.get("profile_store");
    if obj(store_v, None)?.get("bindings") != Some(&empty_bindings()) {
        return Err(bad("store_bindings"));
    }
    let store_bytes = match store_v {
        Some(s) => fid_canon::to_canonical_bytes(s).map_err(|_| bad("profile_store"))?,
        None => return Err(bad("bad_keys")),
    };
    let mut dprof = DprofState::from_canonical_bytes(&store_bytes, vocabs)?;
    let mut st = SpecState::new();

    st.tenant = match top.get("tenant_default") {
        Some(CValue::Null) => None,
        Some(t) => {
            let t = spec(t)?;
            check_loaded_default(&dprof, &t)?;
            Some(t)
        }
        None => return Err(bad("bad_keys")),
    };

    let plans = obj(top.get("plan_defaults"), None)?;
    if plans.len() > MAX_PLANS {
        return Err(bad("too_many_plans"));
    }
    for (k, v) in plans {
        let plan = PlanId::parse(k).ok_or_else(|| bad("bad_plan_id"))?;
        if st.plan_case_taken(&plan) {
            return Err(bad("case_duplicate"));
        }
        let s = spec(v)?;
        check_loaded_default(&dprof, &s)?;
        st.put_plan(plan, s);
    }

    let chars = obj(top.get("characteristics"), Some(&["agent", "working"]))?;
    for branch in [Branch::Working, Branch::Agent] {
        let side = obj(chars.get(branch.as_str()), None)?;
        if side.len() > MAX_CHARS {
            return Err(bad("too_many_chars"));
        }
        for (k, v) in side {
            let id = CharId::parse(k).ok_or_else(|| bad("bad_char_id"))?;
            if st.case_taken(branch, &id) {
                return Err(bad("case_duplicate"));
            }
            let s = match v {
                CValue::Null => None,
                other => Some(spec(other)?),
            };
            if let Some(s) = &s {
                if check_profile(&dprof, branch, s.defaults_profile()).is_ok() {
                    dprof.set_binding(branch, &id, s.defaults_profile())?;
                }
            }
            st.put(branch, id, s);
        }
    }
    Ok((st, dprof))
}

/// Writes the project file as RFC 8785 canonical JSON. Refuses
/// (`project_invalid{bindings_out_of_sync}`) a profile store whose bindings are not exactly
/// the mirror the loader would rebuild, and (`project_invalid{too_large}`) output the loader
/// would refuse, so every file written loads back to equal states.
pub fn project_bytes(st: &SpecState, dprof: &DprofState) -> Result<Vec<u8>, SpecError> {
    for (branch, records) in [(Branch::Working, &st.working), (Branch::Agent, &st.agent)] {
        let mirror: BTreeMap<&CharId, &crate::chars::dprof::ProfileRef> = records
            .iter()
            .filter_map(|(c, s)| s.as_ref().map(|s| (c, s.defaults_profile())))
            .filter(|(_, r)| check_profile(dprof, branch, r).is_ok())
            .collect();
        let held = dprof.own_bindings(branch);
        if mirror.len() != held.len() || !held.iter().all(|(c, r)| mirror.get(c) == Some(&r)) {
            return Err(bad("bindings_out_of_sync"));
        }
    }
    let store_bytes = dprof.to_canonical_bytes()?;
    let mut store_v = fid_canon::parse(&store_bytes).map_err(|_| bad("profile_store"))?;
    match &mut store_v {
        CValue::Obj(o) => {
            o.insert("bindings".to_owned(), empty_bindings());
        }
        _ => return Err(bad("profile_store")),
    }
    let side = |m: &BTreeMap<CharId, Option<GoverningSpec>>| {
        CValue::Obj(m.iter().map(|(c, s)| (c.as_str().to_owned(), super::types::opt_cvalue(s.as_ref()))).collect())
    };
    let mut chars = BTreeMap::new();
    chars.insert("working".to_owned(), side(&st.working));
    chars.insert("agent".to_owned(), side(&st.agent));
    let plans: Obj = st.plans.iter().map(|(p, s)| (p.as_str().to_owned(), s.to_cvalue())).collect();
    let mut top = BTreeMap::new();
    top.insert("schema".to_owned(), CValue::Str(PROJECT_SCHEMA.to_owned()));
    top.insert("profile_store".to_owned(), store_v);
    top.insert("tenant_default".to_owned(), super::types::opt_cvalue(st.tenant.as_ref()));
    top.insert("plan_defaults".to_owned(), CValue::Obj(plans));
    top.insert("characteristics".to_owned(), CValue::Obj(chars));
    let out = fid_canon::to_canonical_bytes(&CValue::Obj(top)).map_err(|_| bad("encode"))?;
    if out.len() > MAX_PROJECT_BYTES {
        return Err(bad("too_large"));
    }
    Ok(out)
}
