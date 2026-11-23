//! FAI header and accountability data for AS9102 Forms 1 and 2 (`RPT.FORMS.HDR`,
//! `REQ-001`..`REQ-008`, `AC-1`..`AC-8`).
//!
//! Clean-room field definitions written in the project's own words (no text copied from
//! SAE AS9102), UUIDv7 stable IDs with permanent retirement, untrusted text capped at 512
//! UTF-8 bytes without NUL, acyclic Form 1 assembly links, RFC 8785 canonical JSON storage,
//! and exact journal replay.

use fid_canon::{digest_bytes as canon_sha256, to_canonical_bytes, CValue};
use fid_gdt::chars::dprof::CharId;
use fid_project::{uuid_v7, IdError, ObservationSource, Uuid};
use fid_registry::{CommandDef, DefError};
use fid_replay::ReplayOutcome;
use fid_report::doc::{is_v7, Text, Untrusted};
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Registry command / event IDs (`W2` for mutations, `R0` for show).
pub const CREATE_FAI: &str = "report.create_fai@1";
pub const SET_HEADER_FAI: &str = "report.set_header_fai@1";
pub const ADD_ITEM_FAI: &str = "report.add_item_fai@1";
pub const REMOVE_ITEM_FAI: &str = "report.remove_item_fai@1";
pub const SHOW_FAI: &str = "report.show_fai@1";

/// Maximum UTF-8 byte length of any header or accountability text field (`REQ-004`, `REQ-008`).
pub const MAX_FAI_TEXT_BYTES: usize = 512;

const LIFECYCLE_SINCE: &str = "2026-11-22";
const ID_PATTERN: &str = "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$";

macro_rules! v7_fai_id {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(Uuid);

        impl $name {
            pub fn new(id: Uuid) -> Option<$name> {
                is_v7(&id).then_some($name(id))
            }

            pub fn parse(text: &str) -> Result<$name, FaiError> {
                let u = parse_canonical_uuid(text).ok_or(FaiError::BadParams("invalid uuid"))?;
                $name::new(u).ok_or(FaiError::BadParams("uuid must be v7"))
            }

            pub fn uuid(&self) -> Uuid {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }
    };
}

v7_fai_id!(
    /// FAI record identifier (UUIDv7, `REQ-001`).
    FaiId
);
v7_fai_id!(
    /// Part revision identifier (UUIDv7, `REQ-001`).
    PartRevisionId
);
v7_fai_id!(
    /// Form 1 or Form 2 entry identifier (UUIDv7, permanently retired on removal, `REQ-007`).
    ItemId
);

/// Clean-room definition of one FAI header field (`REQ-006`, `AC-7`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldDef {
    pub id: &'static str,
    pub label_key: &'static str,
    /// Project-authored own-words label (no text copied from SAE AS9102).
    pub label: &'static str,
    pub required: bool,
    /// Optional designator/revision/clause citation only.
    pub citation: Option<&'static str>,
}

/// Static catalog of FAI header fields in canonical display order (`REQ-006`, `AC-7`).
pub const HEADER_FIELDS: &[FieldDef] = &[
    FieldDef {
        id: "part_number",
        label_key: "report.forms.hdr.part_number",
        label: "Part identifier",
        required: true,
        citation: Some("AS9102 Rev C, Form 1, Field 1"),
    },
    FieldDef {
        id: "part_name",
        label_key: "report.forms.hdr.part_name",
        label: "Part title",
        required: true,
        citation: Some("AS9102 Rev C, Form 1, Field 2"),
    },
    FieldDef {
        id: "part_revision",
        label_key: "report.forms.hdr.part_revision",
        label: "Part revision level",
        required: true,
        citation: Some("AS9102 Rev C, Form 1, Field 5"),
    },
    FieldDef {
        id: "drawing_number",
        label_key: "report.forms.hdr.drawing_number",
        label: "Engineering drawing identifier",
        required: true,
        citation: Some("AS9102 Rev C, Form 1, Field 6"),
    },
    FieldDef {
        id: "drawing_revision",
        label_key: "report.forms.hdr.drawing_revision",
        label: "Engineering drawing revision",
        required: false,
        citation: Some("AS9102 Rev C, Form 1, Field 7"),
    },
    FieldDef {
        id: "organisation",
        label_key: "report.forms.hdr.organisation",
        label: "Producing organisation",
        required: true,
        citation: Some("AS9102 Rev C, Form 1, Field 11"),
    },
    FieldDef {
        id: "supplier_code",
        label_key: "report.forms.hdr.supplier_code",
        label: "Vendor code",
        required: false,
        citation: Some("AS9102 Rev C, Form 1, Field 12"),
    },
    FieldDef {
        id: "purchase_order",
        label_key: "report.forms.hdr.purchase_order",
        label: "Order reference",
        required: false,
        citation: Some("AS9102 Rev C, Form 1, Field 13"),
    },
    FieldDef {
        id: "fai_report_number",
        label_key: "report.forms.hdr.fai_report_number",
        label: "Inspection dossier number",
        required: true,
        citation: Some("AS9102 Rev C, Form 1, Field 4"),
    },
    FieldDef {
        id: "serial_number",
        label_key: "report.forms.hdr.serial_number",
        label: "Sample serial identifier",
        required: false,
        citation: Some("AS9102 Rev C, Form 1, Field 3"),
    },
];

/// Validated header field identifier belonging to [`HEADER_FIELDS`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FieldId(String);

impl FieldId {
    pub fn parse(id: &str) -> Result<Self, FaiError> {
        if HEADER_FIELDS.iter().any(|f| f.id == id) {
            Ok(Self(id.to_owned()))
        } else {
            Err(FaiError::BadParams("unknown header field id"))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// FAI inspection scope kind (`REQ-002`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaiType {
    Full,
    Partial,
}

impl FaiType {
    pub const fn as_str(self) -> &'static str {
        match self {
            FaiType::Full => "FULL",
            FaiType::Partial => "PARTIAL",
        }
    }

    pub fn parse(s: &str) -> Result<Self, FaiError> {
        match s {
            "FULL" => Ok(FaiType::Full),
            "PARTIAL" => Ok(FaiType::Partial),
            _ => Err(FaiError::BadParams("fai_type must be FULL or PARTIAL")),
        }
    }
}

/// Header section of an [`FaiRecord`] (`REQ-002`, `REQ-003`, `REQ-008`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaiHeader {
    pub fai_type: FaiType,
    pub reason: Option<Untrusted<Text>>,
    pub scope: Vec<CharId>,
    pub fields: BTreeMap<FieldId, Untrusted<Text>>,
}

impl Default for FaiHeader {
    fn default() -> Self {
        Self { fai_type: FaiType::Full, reason: None, scope: Vec::new(), fields: BTreeMap::new() }
    }
}

impl FaiHeader {
    pub fn to_value(&self) -> CValue {
        let mut fm = BTreeMap::new();
        for (k, v) in &self.fields {
            fm.insert(k.as_str().to_owned(), v.to_value());
        }
        obj([
            ("fai_type", CValue::Str(self.fai_type.as_str().into())),
            ("fields", CValue::Obj(fm)),
            ("reason", self.reason.as_ref().map_or(CValue::Null, Untrusted::to_value)),
            ("scope", CValue::Arr(self.scope.iter().map(|c| CValue::Str(c.as_str().into())).collect())),
        ])
    }

    pub fn from_value(v: &CValue) -> Result<Self, FaiError> {
        let CValue::Obj(m) = v else { return Err(FaiError::Store("header must be object".into())) };
        let fai_type = FaiType::parse(req_str(m, "fai_type")?)?;
        let reason = match m.get("reason") {
            Some(CValue::Null) | None => None,
            Some(rv) => Some(read_untrusted_text(rv, false)?),
        };
        let scope = match m.get("scope") {
            Some(CValue::Arr(arr)) => {
                let mut out = Vec::with_capacity(arr.len());
                for item in arr {
                    let CValue::Str(s) = item else {
                        return Err(FaiError::Store("scope item must be string".into()));
                    };
                    let c = CharId::parse(s).ok_or_else(|| FaiError::Store("invalid char id in scope".into()))?;
                    out.push(c);
                }
                out
            }
            _ => return Err(FaiError::Store("missing scope".into())),
        };
        let fields = match m.get("fields") {
            Some(CValue::Obj(fm)) => {
                let mut out = BTreeMap::new();
                for (k, fv) in fm {
                    let fid = FieldId::parse(k)?;
                    let tv = read_untrusted_text(fv, false)?;
                    out.insert(fid, tv);
                }
                out
            }
            _ => return Err(FaiError::Store("missing fields".into())),
        };
        Ok(Self { fai_type, reason, scope, fields })
    }
}

/// Form 1 part-number accountability link to another [`FaiRecord`] (`REQ-005`, `REQ-007`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaiForm1Entry {
    pub id: ItemId,
    pub fai_ref: FaiId,
    pub note: Option<Untrusted<Text>>,
}

impl FaiForm1Entry {
    pub fn to_value(&self) -> CValue {
        obj([
            ("fai_ref", CValue::Str(self.fai_ref.to_string())),
            ("id", CValue::Str(self.id.to_string())),
            ("note", self.note.as_ref().map_or(CValue::Null, Untrusted::to_value)),
        ])
    }

    pub fn from_value(v: &CValue) -> Result<Self, FaiError> {
        let CValue::Obj(m) = v else { return Err(FaiError::Store("form1 entry must be object".into())) };
        let id = ItemId::parse(req_str(m, "id")?)?;
        let fai_ref = FaiId::parse(req_str(m, "fai_ref")?)?;
        let note = match m.get("note") {
            Some(CValue::Null) | None => None,
            Some(nv) => Some(read_untrusted_text(nv, false)?),
        };
        Ok(Self { id, fai_ref, note })
    }
}

/// Category of a Form 2 product accountability entry (`REQ-004`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form2Kind {
    Material,
    SpecialProcess,
    FunctionalTest,
}

impl Form2Kind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Form2Kind::Material => "material",
            Form2Kind::SpecialProcess => "special_process",
            Form2Kind::FunctionalTest => "functional_test",
        }
    }

    pub fn parse(s: &str) -> Result<Self, FaiError> {
        match s {
            "material" => Ok(Form2Kind::Material),
            "special_process" => Ok(Form2Kind::SpecialProcess),
            "functional_test" => Ok(Form2Kind::FunctionalTest),
            _ => Err(FaiError::BadParams("form2 kind must be material, special_process or functional_test")),
        }
    }
}

/// Form 2 product accountability entry (`REQ-004`, `REQ-007`, `AC-4`, `AC-6`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaiForm2Entry {
    pub id: ItemId,
    pub kind: Form2Kind,
    pub spec_ref: Untrusted<Text>,
    pub supplier: Untrusted<Text>,
    pub cert_ref: Untrusted<Text>,
    pub note: Option<Untrusted<Text>>,
}

impl FaiForm2Entry {
    pub fn to_value(&self) -> CValue {
        obj([
            ("cert_ref", self.cert_ref.to_value()),
            ("id", CValue::Str(self.id.to_string())),
            ("kind", CValue::Str(self.kind.as_str().into())),
            ("note", self.note.as_ref().map_or(CValue::Null, Untrusted::to_value)),
            ("spec_ref", self.spec_ref.to_value()),
            ("supplier", self.supplier.to_value()),
        ])
    }

    pub fn from_value(v: &CValue) -> Result<Self, FaiError> {
        let CValue::Obj(m) = v else { return Err(FaiError::Store("form2 entry must be object".into())) };
        let id = ItemId::parse(req_str(m, "id")?)?;
        let kind = Form2Kind::parse(req_str(m, "kind")?)?;
        let spec_ref =
            read_untrusted_text(m.get("spec_ref").ok_or_else(|| FaiError::Store("missing spec_ref".into()))?, false)?;
        let supplier =
            read_untrusted_text(m.get("supplier").ok_or_else(|| FaiError::Store("missing supplier".into()))?, false)?;
        let cert_ref =
            read_untrusted_text(m.get("cert_ref").ok_or_else(|| FaiError::Store("missing cert_ref".into()))?, false)?;
        let note = match m.get("note") {
            Some(CValue::Null) | None => None,
            Some(nv) => Some(read_untrusted_text(nv, false)?),
        };
        Ok(Self { id, kind, spec_ref, supplier, cert_ref, note })
    }
}

/// Complete FAI record (AS9102 Forms 1 and 2 data, `Design: Data structures`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaiRecord {
    pub id: FaiId,
    pub part_rev: PartRevisionId,
    pub header: FaiHeader,
    pub form1: Vec<FaiForm1Entry>,
    pub form2: Vec<FaiForm2Entry>,
    pub retired_ids: BTreeSet<Uuid>,
}

impl FaiRecord {
    pub fn to_value(&self) -> CValue {
        obj([
            ("form1", CValue::Arr(self.form1.iter().map(FaiForm1Entry::to_value).collect())),
            ("form2", CValue::Arr(self.form2.iter().map(FaiForm2Entry::to_value).collect())),
            ("header", self.header.to_value()),
            ("id", CValue::Str(self.id.to_string())),
            ("part_rev", CValue::Str(self.part_rev.to_string())),
            ("retired_ids", CValue::Arr(self.retired_ids.iter().map(|u| CValue::Str(u.to_string())).collect())),
        ])
    }

    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, FaiError> {
        to_canonical_bytes(&self.to_value()).map_err(|e| FaiError::Store(format!("canonicalise fai: {e}")))
    }

    pub fn canonical_digest(&self) -> Result<Digest, FaiError> {
        Ok(sha256_bytes(&self.to_canonical_bytes()?))
    }

    fn knows_item_id(&self, u: &Uuid) -> bool {
        self.retired_ids.contains(u)
            || self.form1.iter().any(|e| e.id.uuid() == *u)
            || self.form2.iter().any(|e| e.id.uuid() == *u)
    }
}

/// Summary returned by `report.create_fai@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaiSummary {
    pub fai_id: FaiId,
    pub part_rev: PartRevisionId,
    pub fai_type: FaiType,
}

/// Input item for `report.add_item_fai@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddFaiItem {
    Form1 { fai_ref: FaiId, note: Option<String> },
    Form2 { kind: Form2Kind, spec_ref: String, supplier: String, cert_ref: String, note: Option<String> },
}

/// Parameters for `report.set_header_fai@1`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SetHeaderParams {
    pub fields: BTreeMap<FieldId, Option<String>>,
    pub fai_type: Option<FaiType>,
    pub reason: Option<Option<String>>,
    pub scope: Option<Vec<CharId>>,
}

/// Read-only lookup verifying which characteristics belong to a part revision (`REQ-003`).
pub trait PartCharLookup {
    fn has_characteristic(&self, part_rev: PartRevisionId, char_id: &CharId) -> bool;
}

/// Typed errors of `RPT.FORMS.HDR` (`Design: Errors`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FaiError {
    /// `report.not_found`: named FAI record or item does not exist.
    NotFound(&'static str),
    /// `report.fai_partial_incomplete`: partial FAI lacks reason, scope, or names unknown characteristic (`REQ-003`).
    PartialIncomplete(&'static str),
    /// `report.fai_ref_invalid`: form 1 references unknown FAI record or creates a cycle (`REQ-005`).
    RefInvalid(&'static str),
    /// `report.text_invalid`: text exceeds 512 UTF-8 bytes or contains NUL (`REQ-004`, `REQ-008`).
    TextInvalid(&'static str),
    /// Invalid command parameter.
    BadParams(&'static str),
    /// Storage / ID generation failure.
    Store(String),
}

impl FaiError {
    pub fn code(&self) -> &'static str {
        match self {
            FaiError::NotFound(_) => "report.not_found",
            FaiError::PartialIncomplete(_) => "report.fai_partial_incomplete",
            FaiError::RefInvalid(_) => "report.fai_ref_invalid",
            FaiError::TextInvalid(_) => "report.text_invalid",
            FaiError::BadParams(_) => "report.bad_params",
            FaiError::Store(_) => "report.store_invalid",
        }
    }
}

impl fmt::Display for FaiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FaiError::NotFound(w) => write!(f, "report.not_found: {w}"),
            FaiError::PartialIncomplete(r) => write!(f, "report.fai_partial_incomplete: {r}"),
            FaiError::RefInvalid(r) => write!(f, "report.fai_ref_invalid: {r}"),
            FaiError::TextInvalid(r) => write!(f, "report.text_invalid: {r}"),
            FaiError::BadParams(r) => write!(f, "report.bad_params: {r}"),
            FaiError::Store(d) => write!(f, "report.store_invalid: {d}"),
        }
    }
}

impl std::error::Error for FaiError {}

/// Canonical journal events of `RPT.FORMS.HDR` (`AC-1`, `AC-8`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FaiEvent {
    CreateFai { fai_id: FaiId, part_rev: PartRevisionId, record_digest: Digest },
    SetHeaderFai { fai_id: FaiId, header: Box<FaiHeader>, record_digest: Digest },
    AddForm1Item { fai_id: FaiId, after: Option<ItemId>, entry: FaiForm1Entry, record_digest: Digest },
    AddForm2Item { fai_id: FaiId, after: Option<ItemId>, entry: Box<FaiForm2Entry>, record_digest: Digest },
    RemoveItemFai { fai_id: FaiId, item_id: ItemId, record_digest: Digest },
}

impl FaiEvent {
    pub fn kind(&self) -> &'static str {
        match self {
            FaiEvent::CreateFai { .. } => CREATE_FAI,
            FaiEvent::SetHeaderFai { .. } => SET_HEADER_FAI,
            FaiEvent::AddForm1Item { .. } | FaiEvent::AddForm2Item { .. } => ADD_ITEM_FAI,
            FaiEvent::RemoveItemFai { .. } => REMOVE_ITEM_FAI,
        }
    }

    pub fn to_value(&self) -> CValue {
        match self {
            FaiEvent::CreateFai { fai_id, part_rev, record_digest } => obj([
                ("fai_id", CValue::Str(fai_id.to_string())),
                ("kind", CValue::Str(CREATE_FAI.into())),
                ("part_rev", CValue::Str(part_rev.to_string())),
                ("record_digest", CValue::Str(record_digest.to_string())),
            ]),
            FaiEvent::SetHeaderFai { fai_id, header, record_digest } => obj([
                ("fai_id", CValue::Str(fai_id.to_string())),
                ("header", header.to_value()),
                ("kind", CValue::Str(SET_HEADER_FAI.into())),
                ("record_digest", CValue::Str(record_digest.to_string())),
            ]),
            FaiEvent::AddForm1Item { fai_id, after, entry, record_digest } => obj([
                ("after", after.map_or(CValue::Null, |a| CValue::Str(a.to_string()))),
                ("entry", entry.to_value()),
                ("fai_id", CValue::Str(fai_id.to_string())),
                ("form", CValue::Str("form1".into())),
                ("kind", CValue::Str(ADD_ITEM_FAI.into())),
                ("record_digest", CValue::Str(record_digest.to_string())),
            ]),
            FaiEvent::AddForm2Item { fai_id, after, entry, record_digest } => obj([
                ("after", after.map_or(CValue::Null, |a| CValue::Str(a.to_string()))),
                ("entry", entry.to_value()),
                ("fai_id", CValue::Str(fai_id.to_string())),
                ("form", CValue::Str("form2".into())),
                ("kind", CValue::Str(ADD_ITEM_FAI.into())),
                ("record_digest", CValue::Str(record_digest.to_string())),
            ]),
            FaiEvent::RemoveItemFai { fai_id, item_id, record_digest } => obj([
                ("fai_id", CValue::Str(fai_id.to_string())),
                ("item_id", CValue::Str(item_id.to_string())),
                ("kind", CValue::Str(REMOVE_ITEM_FAI.into())),
                ("record_digest", CValue::Str(record_digest.to_string())),
            ]),
        }
    }

    pub fn from_value(v: &CValue) -> Result<Self, FaiError> {
        let CValue::Obj(m) = v else { return Err(FaiError::Store("fai event must be object".into())) };
        let kind = req_str(m, "kind")?;
        let fai_id = FaiId::parse(req_str(m, "fai_id")?)?;
        let record_digest =
            Digest::parse(req_str(m, "record_digest")?).map_err(|_| FaiError::Store("bad record_digest".into()))?;
        match kind {
            CREATE_FAI => {
                let part_rev = PartRevisionId::parse(req_str(m, "part_rev")?)?;
                Ok(FaiEvent::CreateFai { fai_id, part_rev, record_digest })
            }
            SET_HEADER_FAI => {
                let hv = m.get("header").ok_or_else(|| FaiError::Store("missing header".into()))?;
                Ok(FaiEvent::SetHeaderFai { fai_id, header: Box::new(FaiHeader::from_value(hv)?), record_digest })
            }
            ADD_ITEM_FAI => {
                let after = match m.get("after") {
                    Some(CValue::Null) | None => None,
                    Some(CValue::Str(s)) => Some(ItemId::parse(s)?),
                    _ => return Err(FaiError::Store("invalid after".into())),
                };
                let ev = m.get("entry").ok_or_else(|| FaiError::Store("missing entry".into()))?;
                match req_str(m, "form")? {
                    "form1" => Ok(FaiEvent::AddForm1Item {
                        fai_id,
                        after,
                        entry: FaiForm1Entry::from_value(ev)?,
                        record_digest,
                    }),
                    "form2" => Ok(FaiEvent::AddForm2Item {
                        fai_id,
                        after,
                        entry: Box::new(FaiForm2Entry::from_value(ev)?),
                        record_digest,
                    }),
                    _ => Err(FaiError::Store("invalid form selector".into())),
                }
            }
            REMOVE_ITEM_FAI => {
                let item_id = ItemId::parse(req_str(m, "item_id")?)?;
                Ok(FaiEvent::RemoveItemFai { fai_id, item_id, record_digest })
            }
            _ => Err(FaiError::Store("unknown fai event kind".into())),
        }
    }
}

/// Project-scoped store holding canonical FAI records and their append-only journal.
#[derive(Debug, Default, Clone)]
pub struct FaiStore {
    records: BTreeMap<FaiId, FaiRecord>,
    journal: Vec<FaiEvent>,
}

impl FaiStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn journal(&self) -> &[FaiEvent] {
        &self.journal
    }

    /// `report.show_fai@1` (`R0`, `REQ-007`).
    pub fn show_fai(&self, fai_id: FaiId) -> Result<&FaiRecord, FaiError> {
        self.records.get(&fai_id).ok_or(FaiError::NotFound("fai_record"))
    }

    /// `report.create_fai@1` (`W2`, one journal event, `REQ-001`, `AC-1`).
    pub fn create_fai(
        &mut self,
        part_rev: PartRevisionId,
        obs: &mut dyn ObservationSource,
    ) -> Result<FaiSummary, FaiError> {
        let fai_id = FaiId::new(next_v7(obs)?).ok_or_else(|| FaiError::Store("generated id not v7".into()))?;
        if self.records.contains_key(&fai_id) {
            return Err(FaiError::Store("fai id collision".into()));
        }
        let rec = FaiRecord {
            id: fai_id,
            part_rev,
            header: FaiHeader::default(),
            form1: Vec::new(),
            form2: Vec::new(),
            retired_ids: BTreeSet::new(),
        };
        let record_digest = rec.canonical_digest()?;
        self.records.insert(fai_id, rec);
        self.journal.push(FaiEvent::CreateFai { fai_id, part_rev, record_digest });
        Ok(FaiSummary { fai_id, part_rev, fai_type: FaiType::Full })
    }

    /// `report.set_header_fai@1` (`W2`, one journal event, `REQ-002`, `REQ-003`, `REQ-008`, `AC-2`, `AC-3`, `AC-6`).
    pub fn set_header_fai(
        &mut self,
        fai_id: FaiId,
        params: &SetHeaderParams,
        chars: &dyn PartCharLookup,
    ) -> Result<FaiHeader, FaiError> {
        let existing = self.records.get(&fai_id).ok_or(FaiError::NotFound("fai_record"))?;
        let mut next_header = existing.header.clone();

        if let Some(ty) = params.fai_type {
            next_header.fai_type = ty;
        }
        if let Some(reason_opt) = &params.reason {
            next_header.reason = match reason_opt {
                Some(r) => Some(validate_fai_text(r)?),
                None => None,
            };
        }
        if let Some(scope) = &params.scope {
            next_header.scope = scope.clone();
        }
        for (fid, val_opt) in &params.fields {
            match val_opt {
                Some(text) => {
                    next_header.fields.insert(fid.clone(), validate_fai_text(text)?);
                }
                None => {
                    next_header.fields.remove(fid);
                }
            }
        }

        // REQ-002 & REQ-003: PARTIAL requires a non-empty reason and a non-empty scope of existing characteristics.
        if next_header.fai_type == FaiType::Partial {
            let has_reason =
                next_header.reason.as_ref().is_some_and(|r| !r.untrusted_value().as_str().trim().is_empty());
            if !has_reason {
                return Err(FaiError::PartialIncomplete("partial FAI requires a non-empty reason"));
            }
            if next_header.scope.is_empty() {
                return Err(FaiError::PartialIncomplete("partial FAI requires a non-empty characteristic scope"));
            }
            for cid in &next_header.scope {
                if !chars.has_characteristic(existing.part_rev, cid) {
                    return Err(FaiError::PartialIncomplete("scope references characteristic not in part revision"));
                }
            }
        }

        let mut next_rec = existing.clone();
        next_rec.header = next_header.clone();
        let record_digest = next_rec.canonical_digest()?;
        self.records.insert(fai_id, next_rec);
        self.journal.push(FaiEvent::SetHeaderFai { fai_id, header: Box::new(next_header.clone()), record_digest });
        Ok(next_header)
    }

    /// `report.add_item_fai@1` (`W2`, one journal event, `REQ-004`, `REQ-005`, `REQ-007`, `AC-4`, `AC-5`, `AC-6`).
    pub fn add_item_fai(
        &mut self,
        fai_id: FaiId,
        item: AddFaiItem,
        after: Option<ItemId>,
        obs: &mut dyn ObservationSource,
    ) -> Result<ItemId, FaiError> {
        let existing = self.records.get(&fai_id).ok_or(FaiError::NotFound("fai_record"))?;

        match item {
            AddFaiItem::Form1 { fai_ref, note } => {
                if !self.records.contains_key(&fai_ref) {
                    return Err(FaiError::RefInvalid("referenced FAI record does not exist"));
                }
                if fai_ref == fai_id || self.would_create_cycle(fai_id, fai_ref) {
                    return Err(FaiError::RefInvalid("form 1 reference would create a cycle"));
                }
                let note_u = match note {
                    Some(n) => Some(validate_fai_text(&n)?),
                    None => None,
                };
                let insert_idx = resolve_insert_index(&existing.form1, after, |e| e.id)?;
                let item_id = ItemId::new(next_v7(obs)?).ok_or_else(|| FaiError::Store("id not v7".into()))?;
                if existing.knows_item_id(&item_id.uuid()) {
                    return Err(FaiError::Store("item id collision or retired id reused".into()));
                }
                let entry = FaiForm1Entry { id: item_id, fai_ref, note: note_u };
                let mut next_rec = existing.clone();
                next_rec.form1.insert(insert_idx, entry.clone());
                let record_digest = next_rec.canonical_digest()?;
                self.records.insert(fai_id, next_rec);
                self.journal.push(FaiEvent::AddForm1Item { fai_id, after, entry, record_digest });
                Ok(item_id)
            }
            AddFaiItem::Form2 { kind, spec_ref, supplier, cert_ref, note } => {
                let spec_u = validate_fai_text(&spec_ref)?;
                let sup_u = validate_fai_text(&supplier)?;
                let cert_u = validate_fai_text(&cert_ref)?;
                let note_u = match note {
                    Some(n) => Some(validate_fai_text(&n)?),
                    None => None,
                };
                let insert_idx = resolve_insert_index(&existing.form2, after, |e| e.id)?;
                let item_id = ItemId::new(next_v7(obs)?).ok_or_else(|| FaiError::Store("id not v7".into()))?;
                if existing.knows_item_id(&item_id.uuid()) {
                    return Err(FaiError::Store("item id collision or retired id reused".into()));
                }
                let entry = FaiForm2Entry {
                    id: item_id,
                    kind,
                    spec_ref: spec_u,
                    supplier: sup_u,
                    cert_ref: cert_u,
                    note: note_u,
                };
                let mut next_rec = existing.clone();
                next_rec.form2.insert(insert_idx, entry.clone());
                let record_digest = next_rec.canonical_digest()?;
                self.records.insert(fai_id, next_rec);
                self.journal.push(FaiEvent::AddForm2Item { fai_id, after, entry: Box::new(entry), record_digest });
                Ok(item_id)
            }
        }
    }

    /// `report.remove_item_fai@1` (`W2`, one journal event, permanently retires the item ID, `REQ-007`, `AC-4`).
    pub fn remove_item_fai(&mut self, fai_id: FaiId, item_id: ItemId) -> Result<(), FaiError> {
        let existing = self.records.get(&fai_id).ok_or(FaiError::NotFound("fai_record"))?;
        let mut next_rec = existing.clone();
        let removed = if let Some(pos) = next_rec.form1.iter().position(|e| e.id == item_id) {
            next_rec.form1.remove(pos);
            true
        } else if let Some(pos) = next_rec.form2.iter().position(|e| e.id == item_id) {
            next_rec.form2.remove(pos);
            true
        } else {
            false
        };
        if !removed {
            return Err(FaiError::NotFound("fai_item"));
        }
        next_rec.retired_ids.insert(item_id.uuid());
        let record_digest = next_rec.canonical_digest()?;
        self.records.insert(fai_id, next_rec);
        self.journal.push(FaiEvent::RemoveItemFai { fai_id, item_id, record_digest });
        Ok(())
    }

    /// Returns `true` if adding edge `from -> target` introduces a cycle in the Form 1 graph (`REQ-005`).
    fn would_create_cycle(&self, from: FaiId, target: FaiId) -> bool {
        let mut visited = BTreeSet::new();
        let mut stack = vec![target];
        while let Some(cur) = stack.pop() {
            if cur == from {
                return true;
            }
            if !visited.insert(cur) {
                continue;
            }
            if let Some(rec) = self.records.get(&cur) {
                for edge in &rec.form1 {
                    stack.push(edge.fai_ref);
                }
            }
        }
        false
    }
}

/// Outcome of exact replay over [`FaiEvent`]s (`AC-8`).
#[derive(Debug, Clone)]
pub struct FaiReplayRun {
    pub outcome: ReplayOutcome,
    pub first_mismatch: Option<usize>,
    pub store: FaiStore,
}

/// Replays `events` in exact order reusing journaled IDs and comparing every record digest (`AC-8`).
pub fn replay_exact(events: &[FaiEvent]) -> FaiReplayRun {
    let mut store = FaiStore::new();
    for (idx, ev) in events.iter().enumerate() {
        let ok = match ev {
            FaiEvent::CreateFai { fai_id, part_rev, record_digest } => {
                if store.records.contains_key(fai_id) {
                    false
                } else {
                    let rec = FaiRecord {
                        id: *fai_id,
                        part_rev: *part_rev,
                        header: FaiHeader::default(),
                        form1: Vec::new(),
                        form2: Vec::new(),
                        retired_ids: BTreeSet::new(),
                    };
                    if rec.canonical_digest().ok().as_ref() == Some(record_digest) {
                        store.records.insert(*fai_id, rec);
                        true
                    } else {
                        false
                    }
                }
            }
            FaiEvent::SetHeaderFai { fai_id, header, record_digest } => {
                if let Some(mut rec) = store.records.get(fai_id).cloned() {
                    rec.header = (**header).clone();
                    if rec.canonical_digest().ok().as_ref() == Some(record_digest) {
                        store.records.insert(*fai_id, rec);
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            FaiEvent::AddForm1Item { fai_id, after, entry, record_digest } => {
                if let Some(mut rec) = store.records.get(fai_id).cloned() {
                    if rec.knows_item_id(&entry.id.uuid()) {
                        false
                    } else if let Ok(pos) = resolve_insert_index(&rec.form1, *after, |e| e.id) {
                        rec.form1.insert(pos, entry.clone());
                        if rec.canonical_digest().ok().as_ref() == Some(record_digest) {
                            store.records.insert(*fai_id, rec);
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            FaiEvent::AddForm2Item { fai_id, after, entry, record_digest } => {
                if let Some(mut rec) = store.records.get(fai_id).cloned() {
                    if rec.knows_item_id(&entry.id.uuid()) {
                        false
                    } else if let Ok(pos) = resolve_insert_index(&rec.form2, *after, |e| e.id) {
                        rec.form2.insert(pos, (**entry).clone());
                        if rec.canonical_digest().ok().as_ref() == Some(record_digest) {
                            store.records.insert(*fai_id, rec);
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            FaiEvent::RemoveItemFai { fai_id, item_id, record_digest } => {
                if let Some(mut rec) = store.records.get(fai_id).cloned() {
                    let removed = if let Some(pos) = rec.form1.iter().position(|e| e.id == *item_id) {
                        rec.form1.remove(pos);
                        true
                    } else if let Some(pos) = rec.form2.iter().position(|e| e.id == *item_id) {
                        rec.form2.remove(pos);
                        true
                    } else {
                        false
                    };
                    if removed {
                        rec.retired_ids.insert(item_id.uuid());
                        if rec.canonical_digest().ok().as_ref() == Some(record_digest) {
                            store.records.insert(*fai_id, rec);
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
        };
        if !ok {
            return FaiReplayRun { outcome: ReplayOutcome::Divergent, first_mismatch: Some(idx), store };
        }
        store.journal.push(ev.clone());
    }
    FaiReplayRun { outcome: ReplayOutcome::Identical, first_mismatch: None, store }
}

/// Normalises a label for clean-room SHA-256 comparison (`AC-7`: lowercase, collapse ASCII whitespace, strip punctuation).
pub fn normalise_cleanroom_phrase(text: &str) -> String {
    let mut out = String::new();
    let mut prev_space = true;
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            for lc in ch.to_lowercase() {
                out.push(lc);
            }
            prev_space = false;
        } else if !prev_space {
            out.push(' ');
            prev_space = true;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

/// Computes the SHA-256 hex digest of a normalised phrase (`AC-7`).
pub fn cleanroom_phrase_digest(text: &str) -> Digest {
    let norm = normalise_cleanroom_phrase(text);
    sha256_bytes(norm.as_bytes())
}

/// Returns the five validated [`CommandDef`]s of `RPT.FORMS.HDR`.
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let text = |s: &str| CValue::Str(s.to_string());
    let strs = |items: &[&str]| CValue::Arr(items.iter().map(|s| text(s)).collect());
    let id = |desc: &str| obj([("type", text("string")), ("pattern", text(ID_PATTERN)), ("description", text(desc))]);
    let object = |required: &[&str], properties: CValue| {
        obj([
            ("type", text("object")),
            ("additionalProperties", CValue::Bool(false)),
            ("required", strs(required)),
            ("properties", properties),
        ])
    };
    let build = |name: &str, title: &str, desc: &str, params: CValue, result: CValue, write: bool| {
        let ev = format!("{name}@1");
        let major_one = fid_canon::parse_with_limits(b"1", 16, 4).unwrap_or(CValue::U64(1));
        CommandDef::from_value(obj([
            ("id", text(name)),
            ("major", major_one),
            ("title", text(title)),
            ("description", text(desc)),
            ("params_schema", params),
            ("result_schema", result),
            ("side_effect", text(if write { "W2" } else { "R0" })),
            ("mcp_projection", text("exposed")),
            ("determinism", text("D0")),
            ("idempotency", text(if write { "none" } else { "natural" })),
            ("fs_effect", text("none")),
            ("undo", text(if write { "head_move" } else { "not_undoable" })),
            ("preconditions", CValue::Arr(Vec::new())),
            ("effects", if write { strs(&[ev.as_str()]) } else { CValue::Arr(Vec::new()) }),
            ("algorithms", CValue::Arr(Vec::new())),
            ("exposure", strs(&["cli", "mcp"])),
            ("lifecycle", obj([("state", text("active")), ("since", text(LIFECYCLE_SINCE))])),
            ("human_only", CValue::Bool(false)),
            ("network", text("none")),
            ("web", text("available")),
            ("cli_bare", CValue::Bool(false)),
        ]))
    };

    Ok(vec![
        build(
            "report.create_fai",
            "Create an FAI record for a part revision",
            "Creates a new FULL FAI record with UUIDv7 ID linked to a part revision.",
            object(&["part_rev"], obj([("part_rev", id("Part revision ID (UUIDv7)."))])),
            object(
                &["fai_id", "fai_type", "part_rev"],
                obj([
                    ("fai_id", id("FAI record ID.")),
                    ("fai_type", obj([("type", text("string")), ("enum", strs(&["FULL", "PARTIAL"]))])),
                    ("part_rev", id("Part revision ID.")),
                ]),
            ),
            true,
        )?,
        build(
            "report.set_header_fai",
            "Set FAI header fields, type, reason and partial scope",
            "Updates header fields and FULL/PARTIAL scope of an FAI record.",
            object(
                &["fai"],
                obj([
                    ("fai", id("FAI record ID.")),
                    ("fai_type", obj([("type", strs(&["string", "null"]))])),
                    ("fields", obj([("type", text("object"))])),
                    ("reason", obj([("type", strs(&["string", "null"])), ("x-untrusted", CValue::Bool(true))])),
                    ("scope", obj([("type", strs(&["array", "null"]))])),
                ]),
            ),
            obj([("type", text("object"))]),
            true,
        )?,
        build(
            "report.add_item_fai",
            "Add a Form 1 or Form 2 accountability item",
            "Appends or inserts an accountability entry into Form 1 or Form 2.",
            object(
                &["fai", "item"],
                obj([
                    ("after", obj([("type", strs(&["string", "null"])), ("pattern", text(ID_PATTERN))])),
                    ("fai", id("FAI record ID.")),
                    ("item", obj([("type", text("object"))])),
                ]),
            ),
            object(&["item_id"], obj([("item_id", id("Created item ID (UUIDv7)."))])),
            true,
        )?,
        build(
            "report.remove_item_fai",
            "Remove a Form 1 or Form 2 accountability item",
            "Removes an accountability item and permanently retires its UUIDv7 ID.",
            object(&["fai", "item"], obj([("fai", id("FAI record ID.")), ("item", id("Item ID to remove."))])),
            object(&["removed"], obj([("removed", obj([("type", text("boolean"))]))])),
            true,
        )?,
        build(
            "report.show_fai",
            "Show an FAI record",
            "Returns the FAI record with its header and Form 1 / Form 2 entries in insertion order.",
            object(&["fai"], obj([("fai", id("FAI record ID."))])),
            obj([("type", text("object"))]),
            false,
        )?,
    ])
}

/// Validates untrusted FAI text: at most 512 UTF-8 bytes, no NUL (`REQ-004`, `REQ-008`).
pub fn validate_fai_text(s: &str) -> Result<Untrusted<Text>, FaiError> {
    if s.len() > MAX_FAI_TEXT_BYTES {
        return Err(FaiError::TextInvalid("exceeds 512 UTF-8 bytes"));
    }
    if s.contains('\0') {
        return Err(FaiError::TextInvalid("contains NUL"));
    }
    Untrusted::text(s).map_err(|_| FaiError::TextInvalid("invalid text"))
}

fn read_untrusted_text(v: &CValue, require_non_empty: bool) -> Result<Untrusted<Text>, FaiError> {
    let ut = Untrusted::<Text>::from_value(v).map_err(|_| FaiError::Store("invalid untrusted text".into()))?;
    let s = ut.untrusted_value().as_str();
    if require_non_empty && s.is_empty() {
        return Err(FaiError::TextInvalid("text must not be empty"));
    }
    validate_fai_text(s)
}

fn resolve_insert_index<T>(items: &[T], after: Option<ItemId>, id_of: fn(&T) -> ItemId) -> Result<usize, FaiError> {
    match after {
        None => Ok(items.len()),
        Some(prev) => {
            items.iter().position(|e| id_of(e) == prev).map(|p| p + 1).ok_or(FaiError::NotFound("after_item"))
        }
    }
}

fn next_v7(obs: &mut dyn ObservationSource) -> Result<Uuid, FaiError> {
    let (ms, bits) = obs.draw_v7().map_err(|e: IdError| FaiError::Store(e.to_string()))?;
    uuid_v7(ms, &bits).map_err(|e: IdError| FaiError::Store(e.to_string()))
}

fn sha256_bytes(bytes: &[u8]) -> Digest {
    Digest::from_bytes(canon_sha256(bytes).0)
}

fn obj<const N: usize>(entries: [(&str, CValue); N]) -> CValue {
    let mut m = BTreeMap::new();
    for (k, v) in entries {
        m.insert(k.to_owned(), v);
    }
    CValue::Obj(m)
}

fn req_str<'a>(m: &'a BTreeMap<String, CValue>, k: &str) -> Result<&'a str, FaiError> {
    match m.get(k) {
        Some(CValue::Str(s)) => Ok(s.as_str()),
        _ => Err(FaiError::Store(format!("missing or non-string field {k}"))),
    }
}

fn parse_canonical_uuid(s: &str) -> Option<Uuid> {
    let u = Uuid::parse(s).ok()?;
    (u.to_string() == s).then_some(u)
}
