//! The public view of a record (REQ-UNC.RDL.FORMAT-003, -004).
//!
//! The view is built from an allowlist of fields, never by deleting fields from the full
//! record, so a field added to the record later cannot reach a restricted view by default.
//! Three forms, chosen fail-closed:
//!
//! - **blind** (tier B, or visibility `blind`): exactly `{aim, digest, id}`.
//! - **reference only** (visibility `reference_only`, or a licence that does not permit
//!   redistribution whatever the visibility says): `{aim, digest, id, inputs: [{digest}],
//!   licence, provenance: {source}, reference_digest, schema, tier, visibility:
//!   "reference_only"}`. Input units
//!   and roles, reference outputs, precision, generator, cross-checks and the provenance
//!   method are removed; `reference_digest` is the SHA-256 of the canonical `reference`
//!   object, so the removed outputs stay verifiable.
//! - **full** (visibility `public` with a redistributable licence): the canonical record
//!   plus `digest`.

use crate::error::{internal, RdlError};
use crate::record::{licence_redistributable, obj, RdlDataset, Tier, Visibility};
use fid_canon::CValue;
use fid_types::Digest;

/// Which public form a record gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewForm {
    Full,
    ReferenceOnly,
    Blind,
}

/// The form [`public_view`] uses for `rec`. Tier B and visibility `blind` are checked first,
/// then any restriction; only an unrestricted public record gets [`ViewForm::Full`].
pub fn view_form(rec: &RdlDataset) -> ViewForm {
    if rec.tier() == Tier::B || rec.visibility() == Visibility::Blind {
        ViewForm::Blind
    } else if rec.visibility() != Visibility::Public || !licence_redistributable(rec.licence()) {
        ViewForm::ReferenceOnly
    } else {
        ViewForm::Full
    }
}

fn s(x: &str) -> CValue {
    CValue::Str(x.to_string())
}

/// The public view of `rec` (see the module documentation for the three forms). Fails only
/// with `internal_error` if canonical encoding of the record's own value fails.
pub fn public_view(rec: &RdlDataset) -> Result<CValue, RdlError> {
    let digest = s(rec.digest().as_str());
    Ok(match view_form(rec) {
        ViewForm::Blind => obj([("aim", s(rec.aim())), ("digest", digest), ("id", s(rec.id()))]),
        ViewForm::ReferenceOnly => {
            let inputs = rec.inputs().iter().map(|i| obj([("digest", s(i.digest.as_str()))])).collect();
            let reference = match rec.to_value() {
                CValue::Obj(mut m) => m.remove("reference"),
                _ => None,
            }
            .ok_or_else(|| internal("record value has no reference"))?;
            let d = fid_canon::digest(&reference).map_err(|_| internal("canonical encoding of the reference"))?;
            let reference_digest = s(Digest::from_bytes(d.0).as_str());
            obj([
                ("aim", s(rec.aim())),
                ("digest", digest),
                ("id", s(rec.id())),
                ("inputs", CValue::Arr(inputs)),
                ("licence", s(rec.licence())),
                ("provenance", obj([("source", s(&rec.provenance().source))])),
                ("reference_digest", reference_digest),
                ("schema", s(crate::record::SCHEMA)),
                ("tier", s(rec.tier().as_str())),
                // The effective visibility of this form, whatever the record declares.
                ("visibility", s(Visibility::ReferenceOnly.as_str())),
            ])
        }
        ViewForm::Full => match rec.to_value() {
            CValue::Obj(mut m) => {
                m.insert("digest".to_string(), digest);
                CValue::Obj(m)
            }
            _ => return Err(internal("record value is not an object")),
        },
    })
}
