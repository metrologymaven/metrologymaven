//! The defaults-profile key vocabulary `fid.dmt-profile-keys/1`: which keys a defaults profile
//! (`GDT.CHAR.DPROF`) may set, and what each value must be. The schema owns the keys; the
//! values are expert content (D-009).
//!
//! Two families, nothing else:
//!
//! - **Row variant keys.** For every row of the table, in the profile's dialect, that offers
//!   two or more variants, the row's key text (`dmt:<dialect>/<characteristic>/<mods>@<v>`)
//!   is a key; its value must be a text naming one of that row's variants. The resolver
//!   (`GDT.RES.RESOLVE`) reads it when no characteristic override is given.
//! - **Named keys** ([`NAMED_PROFILE_KEYS`]): a closed list of settings that are not row
//!   variants, each a text value. Adding one is a schema change and bumps
//!   [`PROFILE_KEYS_SCHEMA`].

use super::ident::Dialect;
use super::table::DmtTable;
use std::collections::BTreeMap;

/// Version of this vocabulary's rules.
pub const PROFILE_KEYS_SCHEMA: &str = "fid.dmt-profile-keys/1";

/// The named (non-row) keys, ascending, with the spec node that reads each.
pub const NAMED_PROFILE_KEYS: [(&str, &str); 3] = [
    // Datum label pattern per dialect (GDT.DRF.DATUM).
    ("gdt.datum.label_pattern", "GDT.DRF.DATUM"),
    // Edge extraction method override (SCAN.EDGE.EST.DEFS asks for this key).
    ("scan.edge.extraction_method", "SCAN.EDGE.EST.DEFS"),
    // Default decision-rule hint passed to UNC (GDT.RES.ROWASME.DPROF).
    ("unc.decision_rule_hint", "GDT.RES.ROWASME.DPROF"),
];

/// What a key's value must be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileKeyRule {
    /// A text naming one of these variants (ascending) of the row with this key.
    Variant(Vec<String>),
    /// Any text value.
    Text,
}

/// The vocabulary for profiles of `dialect` against `table`: every named key plus every row
/// variant key of that dialect, by key text.
pub fn profile_vocabulary(table: &DmtTable, dialect: Dialect) -> BTreeMap<String, ProfileKeyRule> {
    let mut out: BTreeMap<String, ProfileKeyRule> =
        NAMED_PROFILE_KEYS.iter().map(|(k, _)| ((*k).to_owned(), ProfileKeyRule::Text)).collect();
    for (key, row) in table.rows() {
        if key.dialect() == dialect && row.variants().len() >= 2 {
            let names = row.variants().iter().map(|c| c.variant().to_owned()).collect();
            out.insert(key.as_str().to_owned(), ProfileKeyRule::Variant(names));
        }
    }
    out
}
