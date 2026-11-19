//! The command layer of the interpretation resolver (spec node `GDT.RES.RESOLVE`): the two
//! registry commands over the pure resolver in `fid_gps::resolve`.
//!
//! - [`resolve_chain`]: `gdt.resolve_chain@1` (R1; CLI `fid gdt resolve-chain --char <handle>
//!   --json`, MCP `gdt_resolve_chain`). Calls the governing-spec gate of `GDT.CHAR.SPEC` first,
//!   then builds the resolver input from the gate's spec (dialect, editions of its standards)
//!   and its defaults profile, and resolves against the loaded DMT table.
//! - [`get_dmt_row`]: `gdt.get_dmt_row@1` (R0; CLI `fid gdt get-dmt-row --key <key> --json`,
//!   MCP `gdt_get_dmt_row`): one row's canonical JSON by exact key.
//!
//! The characteristic itself (type, modifiers, optional variant override) is passed inline as
//! a `fid.gdt-resolve-char/1` document ([`CharacteristicRef`]) until the characteristic model
//! of `GDT.CHAR` exists; its `char` field is the handle the gate checks.
//!
//! ```json
//! { "schema": "fid.gdt-resolve-char/1", "char": "flat-1", "characteristic": "flatness",
//!   "modifiers": [], "override": null }
//! ```
//!
//! Nothing here guesses: an absent or broken governing spec, an absent table, a profile entry
//! for a row that is not a variant name, two matching rows and an unknown variant all refuse.
//! Nothing panics, and every input is bounded.

use crate::chars::dprof::{Branch, CharId, DprofState, ProfileValue};
use crate::chars::spec::{SpecError, SpecState};
use crate::dmt_flow::roster::Dialect;
use fid_canon::CValue;
use fid_gps::dmt::{Dialect as GpsDialect, DmtRow, DmtTable};
use fid_gps::resolve::{Resolution, ResolveError, ResolveInput};
use fid_io_reg::Principal;
use std::collections::BTreeMap;
use std::fmt;

/// Command ID of the resolve command.
pub const RESOLVE_CHAIN_COMMAND: &str = "gdt.resolve_chain@1";
/// Command ID of the row lookup.
pub const GET_DMT_ROW_COMMAND: &str = "gdt.get_dmt_row@1";
/// `schema` value of an inline characteristic document.
pub const CHAR_SCHEMA: &str = "fid.gdt-resolve-char/1";
/// Largest inline characteristic document, in bytes.
pub const MAX_CHAR_BYTES: usize = 64 * 1024;
/// Deepest nesting of an inline characteristic document: the object and its `modifiers` list.
const MAX_CHAR_DEPTH: usize = 2;
/// Longest string field of an inline characteristic document, in bytes.
const MAX_CHAR_FIELD: usize = 128;
const CHAR_FIELDS: [&str; 5] = ["char", "characteristic", "modifiers", "override", "schema"];

/// Errors of the two commands. [`CommandError::code`] is the stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    /// An error of the governing-spec gate (`governing_spec_missing`, `char_unknown`, ...),
    /// passed through unchanged.
    Spec(SpecError),
    /// `dmt_not_loaded`: no DMT table is loaded.
    DmtNotLoaded,
    /// `bad_profile_choice{key}`: the defaults profile sets a row key (`dmt:...`) to something
    /// other than a text naming a variant.
    BadProfileChoice {
        /// The key (truncated).
        key: String,
    },
    /// `malformed_characteristic{reason}`: the inline characteristic document is invalid.
    MalformedCharacteristic {
        /// Static reason.
        reason: &'static str,
    },
    /// An error of the resolver (`unknown_variant`, `ambiguous_resolution`, `bad_input`,
    /// `profile_row_unknown`, `bad_dmt_key`, `dmt_row_unknown`).
    Resolve(ResolveError),
}

impl CommandError {
    /// The stable code.
    pub fn code(&self) -> &'static str {
        match self {
            CommandError::Spec(e) => e.code(),
            CommandError::DmtNotLoaded => "dmt_not_loaded",
            CommandError::BadProfileChoice { .. } => "bad_profile_choice",
            CommandError::MalformedCharacteristic { .. } => "malformed_characteristic",
            CommandError::Resolve(e) => e.code(),
        }
    }

    /// The static reason, where there is one.
    pub fn reason(&self) -> Option<&'static str> {
        match self {
            CommandError::Spec(e) => e.reason(),
            CommandError::MalformedCharacteristic { reason } => Some(reason),
            CommandError::Resolve(e) => e.reason(),
            _ => None,
        }
    }

    fn malformed(reason: &'static str) -> CommandError {
        CommandError::MalformedCharacteristic { reason }
    }
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommandError::Spec(e) => write!(f, "{e}"),
            CommandError::Resolve(e) => write!(f, "{e}"),
            CommandError::BadProfileChoice { key } => write!(f, "{}{{{}}}", self.code(), key.escape_debug()),
            CommandError::MalformedCharacteristic { reason } => write!(f, "{}{{{reason}}}", self.code()),
            CommandError::DmtNotLoaded => f.write_str(self.code()),
        }
    }
}

impl std::error::Error for CommandError {}

impl From<SpecError> for CommandError {
    fn from(e: SpecError) -> Self {
        CommandError::Spec(e)
    }
}

impl From<ResolveError> for CommandError {
    fn from(e: ResolveError) -> Self {
        CommandError::Resolve(e)
    }
}

/// An inline characteristic: its handle, type, modifier list and optional variant override.
/// Fields are private; [`CharacteristicRef::parse`] is the only constructor. Grammar checks of
/// the type, modifiers and override are the resolver's ([`ResolveInput::new`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacteristicRef {
    char: CharId,
    characteristic: String,
    modifiers: Vec<String>,
    variant_override: Option<String>,
}

fn field_str<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> Result<&'a str, CommandError> {
    match o.get(k) {
        Some(CValue::Str(s)) if s.len() <= MAX_CHAR_FIELD => Ok(s),
        Some(CValue::Str(_)) => Err(CommandError::malformed("too_long")),
        _ => Err(CommandError::malformed("wrong_type")),
    }
}

impl CharacteristicRef {
    /// Parses a `fid.gdt-resolve-char/1` document: at most [`MAX_CHAR_BYTES`], JSON with no
    /// duplicate key and nesting at most 2 (`malformed_characteristic{json}`), exactly the five
    /// fields (`bad_keys`), `bad_schema`, `char` a characteristic ID (`bad_char_id`), strings
    /// at most 128 bytes (`too_long`), `modifiers` an array of at most 8 strings
    /// (`too_many_modifiers`), `override` a string or null; anything else `wrong_type`.
    pub fn parse(bytes: &[u8]) -> Result<CharacteristicRef, CommandError> {
        if bytes.len() > MAX_CHAR_BYTES {
            return Err(CommandError::malformed("too_large"));
        }
        let v = fid_canon::parse_with_limits(bytes, MAX_CHAR_BYTES, MAX_CHAR_DEPTH)
            .map_err(|_| CommandError::malformed("json"))?;
        let o = match &v {
            CValue::Obj(o) => o,
            _ => return Err(CommandError::malformed("wrong_type")),
        };
        if o.len() != CHAR_FIELDS.len() || !CHAR_FIELDS.iter().all(|k| o.contains_key(*k)) {
            return Err(CommandError::malformed("bad_keys"));
        }
        if field_str(o, "schema")? != CHAR_SCHEMA {
            return Err(CommandError::malformed("bad_schema"));
        }
        let char = CharId::parse(field_str(o, "char")?).ok_or_else(|| CommandError::malformed("bad_char_id"))?;
        let characteristic = field_str(o, "characteristic")?.to_owned();
        let list = match o.get("modifiers") {
            Some(CValue::Arr(a)) => a,
            _ => return Err(CommandError::malformed("wrong_type")),
        };
        if list.len() > fid_gps::dmt::ident::MAX_MODIFIERS {
            return Err(CommandError::malformed("too_many_modifiers"));
        }
        let mut modifiers = Vec::with_capacity(list.len());
        for m in list {
            match m {
                CValue::Str(s) if s.len() <= MAX_CHAR_FIELD => modifiers.push(s.clone()),
                CValue::Str(_) => return Err(CommandError::malformed("too_long")),
                _ => return Err(CommandError::malformed("wrong_type")),
            }
        }
        let variant_override = match o.get("override") {
            Some(CValue::Null) => None,
            Some(_) => Some(field_str(o, "override")?.to_owned()),
            None => return Err(CommandError::malformed("bad_keys")),
        };
        Ok(CharacteristicRef { char, characteristic, modifiers, variant_override })
    }

    /// The characteristic handle the gate checks.
    pub fn char(&self) -> &CharId {
        &self.char
    }

    /// The characteristic type.
    pub fn characteristic(&self) -> &str {
        &self.characteristic
    }

    /// The modifiers as given.
    pub fn modifiers(&self) -> &[String] {
        &self.modifiers
    }

    /// The explicit variant override.
    pub fn variant_override(&self) -> Option<&str> {
        self.variant_override.as_deref()
    }
}

/// `gdt.resolve_chain@1` (R1). Steps, each refusing on failure:
///
/// 1. the governing-spec gate [`SpecState::require_governing_spec`] on the principal's
///    branch (its errors pass through: `governing_spec_missing`, `char_unknown`,
///    `defaults_profile_unknown`, `defaults_profile_digest_mismatch`, ...);
/// 2. `dmt_not_loaded` when `table` is `None`;
/// 3. the profile's row choices: every profile key starting `dmt:` must hold a text
///    (`bad_profile_choice{key}`); other keys are not the resolver's;
/// 4. the resolver input: dialect from the spec, editions `<designator>-<edition>` of each of
///    its standards (the defaults-profile edition form, which DMT rows use), type, modifiers
///    and override from `ch` (`bad_input{reason}`);
/// 5. [`fid_gps::resolve::resolve`] (`profile_row_unknown` when a profile row choice names
///    a row the loaded table lacks, `ambiguous_resolution`, `unknown_variant`).
///
/// Read-only: neither state is changed.
pub fn resolve_chain(
    spec: &SpecState,
    dprof: &DprofState,
    principal: Principal,
    ch: &CharacteristicRef,
    table: Option<&DmtTable>,
) -> Result<Resolution, CommandError> {
    let gs = spec.require_governing_spec(principal, ch.char.as_str(), dprof)?;
    let table = table.ok_or(CommandError::DmtNotLoaded)?;
    let stored = dprof.resolve(Branch::for_principal(principal), gs.defaults_profile()).map_err(SpecError::from)?;
    let mut choices = BTreeMap::new();
    for (k, v) in stored.doc().profile().keys() {
        if !k.starts_with("dmt:") {
            continue;
        }
        match v {
            ProfileValue::Text(t) => {
                choices.insert(k.clone(), t.clone());
            }
            _ => return Err(CommandError::BadProfileChoice { key: k.clone() }),
        }
    }
    let editions: Vec<String> = gs.standards().iter().map(|s| s.profile_edition()).collect();
    let editions: Vec<&str> = editions.iter().map(String::as_str).collect();
    let modifiers: Vec<&str> = ch.modifiers.iter().map(String::as_str).collect();
    let dialect = match gs.dialect() {
        Dialect::Asme => GpsDialect::Asme,
        Dialect::Iso => GpsDialect::Iso,
    };
    let input =
        ResolveInput::new(dialect, &ch.characteristic, &modifiers, &editions, choices, ch.variant_override.as_deref())?;
    Ok(fid_gps::resolve::resolve(&input, table)?)
}

/// `gdt.get_dmt_row@1` (R0): `dmt_not_loaded` when `table` is `None`, else the row with
/// exactly this key (`bad_dmt_key`, `dmt_row_unknown`). Its `--json` output is
/// [`DmtRow::canonical_bytes`].
pub fn get_dmt_row<'t>(table: Option<&'t DmtTable>, key: &str) -> Result<&'t DmtRow, CommandError> {
    let table = table.ok_or(CommandError::DmtNotLoaded)?;
    Ok(fid_gps::resolve::get_dmt_row(table, key)?)
}
