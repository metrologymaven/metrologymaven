//! Atomic composite handler `report.add_char_balloon@1`, canonical journal event, registry
//! definition and exact replay (`RPT.BALLOON.CHAR`, `REQ-001..006`, `AC-1..AC-5`).

use super::model::{
    AuthoredQuantity, CharBalloonError, Dialect, DrawingCharInput, DrawingCharKind, DrawingCharacteristic,
    MAX_CHAR_TEXT_BYTES, UNIT_CONVERSION_ALGO,
};
use crate::balloon::register::{AddBalloonParams, Balloon, BalloonRegister, SheetPoint};
use crate::balloon::sheet::{digest_bytes, BalloonId, DrawingRevId, SheetId, SheetInfo, UuidV7Gen};
use crate::doc::defs::ID_PATTERN;
use crate::doc::model::obj;
use crate::doc::text::Untrusted;
use fid_canon::{to_canonical_bytes, CValue};
use fid_gdt::chars::dprof::{CharId, DprofState};
use fid_gdt::chars::spec::{PlanId, SpecState};
use fid_io_reg::Principal;
use fid_registry::{CommandDef, DefError};
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use std::cmp::Ordering;
use std::collections::BTreeMap;

/// Canonical command / event ID for creating a characteristic and its balloon atomically (`W2`).
pub const ADD_CHAR_BALLOON: &str = "report.add_char_balloon@1";

/// Parameters of `report.add_char_balloon@1` (`Design: Registry command`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddCharBalloonParams {
    pub plan: PlanId,
    pub drawing_rev: DrawingRevId,
    pub sheet: SheetId,
    pub characteristic: DrawingCharInput,
    pub anchor: SheetPoint,
    pub label: SheetPoint,
    pub number: Option<u32>,
    pub zone: Option<String>,
    pub dry_run: bool,
}

/// Result of `report.add_char_balloon@1` (`Design: Registry command`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddCharBalloonResult {
    pub characteristic: DrawingCharacteristic,
    pub balloon: Balloon,
    pub committed: bool,
}

impl AddCharBalloonResult {
    pub fn to_value(&self) -> CValue {
        obj([
            ("balloon", self.balloon.to_value()),
            ("characteristic", self.characteristic.to_value()),
            ("committed", CValue::Bool(self.committed)),
        ])
    }
}

/// Single canonical journal event carrying the full created characteristic and balloon (`REQ-001`, `REQ-006`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharBalloonEvent {
    pub principal: Principal,
    pub characteristic: DrawingCharacteristic,
    pub balloon: Balloon,
    pub result_digest: Digest,
}

impl CharBalloonEvent {
    pub fn kind(&self) -> &'static str {
        ADD_CHAR_BALLOON
    }

    pub fn to_value(&self) -> CValue {
        let p_str = match self.principal {
            Principal::Human => "human",
            Principal::Agent => "agent",
            Principal::Script => "script",
        };
        obj([
            ("balloon", self.balloon.to_value()),
            ("characteristic", self.characteristic.to_value()),
            ("kind", CValue::Str(ADD_CHAR_BALLOON.into())),
            ("principal", CValue::Str(p_str.into())),
            ("result_digest", CValue::Str(self.result_digest.to_string())),
        ])
    }
}

/// Transactional store holding drawing-created characteristics alongside the composite journal.
#[derive(Debug, Default, Clone)]
pub struct CharBalloonStore {
    characteristics: BTreeMap<CharId, DrawingCharacteristic>,
    journal: Vec<CharBalloonEvent>,
}

impl CharBalloonStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, id: &CharId) -> Option<&DrawingCharacteristic> {
        self.characteristics.get(id)
    }

    pub fn all(&self) -> impl Iterator<Item = &DrawingCharacteristic> {
        self.characteristics.values()
    }

    pub fn journal(&self) -> &[CharBalloonEvent] {
        &self.journal
    }

    /// Executes `report.add_char_balloon@1` atomically across GDT `SpecState` + `DprofState` +
    /// `BalloonRegister` (`REQ-001..005`, `AC-1..AC-4`).
    ///
    /// On ANY error (missing dialect, invalid limits, GDT error, or balloon register error),
    /// every state structure is left 100% unchanged and nothing is journaled!
    #[allow(clippy::too_many_arguments)]
    pub fn add_char_balloon(
        &mut self,
        spec: &mut SpecState,
        dprof: &mut DprofState,
        register: &mut BalloonRegister,
        sheets: &dyn SheetInfo,
        ids: &mut dyn UuidV7Gen,
        principal: Principal,
        p: &AddCharBalloonParams,
    ) -> Result<AddCharBalloonResult, CharBalloonError> {
        // 1. Require explicit dialect (REQ-004, AC-4).
        let dialect = p.characteristic.dialect.ok_or(CharBalloonError::DialectMissing)?;

        // 2. Validate untrusted text fields <= 256 bytes (C6).
        if let Some(name) = &p.characteristic.name {
            if name.untrusted_value().len() > MAX_CHAR_TEXT_BYTES {
                return Err(CharBalloonError::TextInvalid("characteristic name > 256 bytes"));
            }
        }
        for d in &p.characteristic.datum_refs {
            if d.untrusted_value().len() > MAX_CHAR_TEXT_BYTES {
                return Err(CharBalloonError::TextInvalid("datum ref > 256 bytes"));
            }
        }

        // 3. Validate units match characteristic kind and lower <= upper via exact kdec (AC-3).
        validate_char_quantities(&p.characteristic)?;

        // 4. Snapshot all mutable collaborators so any failure rolls back atomically (REQ-002).
        let mut spec_tx = spec.clone();
        let mut dprof_tx = dprof.clone();
        let mut reg_tx = register.clone();

        // 5. Generate UUIDv7 characteristic ID and invoke GDT's create_characteristic handler.
        let char_uuid = ids.next_v7().map_err(|e| CharBalloonError::Store(e.to_string()))?;
        let char_id_str = char_uuid.to_string();
        let char_id =
            CharId::parse(&char_id_str).ok_or_else(|| CharBalloonError::Store("uuid invalid as CharId".into()))?;

        spec_tx.create_characteristic(&mut dprof_tx, principal, p.plan.as_str(), char_id.as_str(), false)?;

        // 6. Add balloon in register (enforces bounds, unique number, zone <= 16 bytes, etc.).
        let balloon = reg_tx.add_balloon(
            sheets,
            ids,
            &AddBalloonParams {
                drawing_rev: p.drawing_rev,
                sheet: p.sheet,
                characteristic: char_id.clone(),
                anchor: p.anchor,
                label: p.label,
                number: p.number,
                insert: false,
                zone: p.zone.clone(),
            },
        )?;

        let created_char = DrawingCharacteristic {
            id: char_id.clone(),
            plan: p.plan.clone(),
            kind: p.characteristic.kind,
            nominal: p.characteristic.nominal.clone(),
            upper: p.characteristic.upper.clone(),
            lower: p.characteristic.lower.clone(),
            dialect,
            datum_refs: p.characteristic.datum_refs.clone(),
            name: p.characteristic.name.clone(),
            unit_algo: UNIT_CONVERSION_ALGO.to_string(),
        };

        if p.dry_run {
            return Ok(AddCharBalloonResult { characteristic: created_char, balloon, committed: false });
        }

        let res =
            AddCharBalloonResult { characteristic: created_char.clone(), balloon: balloon.clone(), committed: true };
        let bytes =
            to_canonical_bytes(&res.to_value()).map_err(|e| CharBalloonError::Store(format!("canonicalise: {e}")))?;
        let result_digest = digest_bytes(&bytes);

        // 7. Commit transaction: all collaborators updated together with exactly one journal event.
        *spec = spec_tx;
        *dprof = dprof_tx;
        *register = reg_tx;
        self.characteristics.insert(char_id, created_char.clone());
        self.journal.push(CharBalloonEvent { principal, characteristic: created_char, balloon, result_digest });

        Ok(res)
    }

    pub(crate) fn insert_replayed(&mut self, ev: CharBalloonEvent) {
        self.characteristics.insert(ev.characteristic.id.clone(), ev.characteristic.clone());
        self.journal.push(ev);
    }
}

fn validate_char_quantities(inp: &DrawingCharInput) -> Result<(), CharBalloonError> {
    let want_ang = matches!(inp.kind, DrawingCharKind::Angle);
    for q in [&inp.nominal, &inp.upper, &inp.lower] {
        if q.unit.is_angular() != want_ang {
            return Err(CharBalloonError::GdtValidation(
                "unit_dimension_mismatch",
                "quantity unit dimension does not match characteristic kind",
            ));
        }
    }
    // Lower tolerance limit must not exceed upper tolerance limit (`AC-3`).
    if inp.lower.cmp_si(&inp.upper)? == Ordering::Greater {
        return Err(CharBalloonError::GdtValidation(
            "invalid_limits",
            "lower tolerance limit exceeds upper tolerance limit",
        ));
    }
    Ok(())
}

/// Outcome of exact replay of `report.add_char_balloon@1` events (`AC-5`).
#[derive(Debug, Clone)]
pub struct CharBalloonReplayRun {
    pub outcome: ReplayOutcome,
    pub first_mismatch: Option<usize>,
    pub store: CharBalloonStore,
    pub register: BalloonRegister,
}

/// Replays `events` in exact mode reading characteristic and balloon IDs back from the journal
/// (never generating fresh IDs, `REQ-006`, `AC-5`).
pub fn replay_exact(events: &[CharBalloonEvent], sheets: &dyn SheetInfo) -> CharBalloonReplayRun {
    let mut store = CharBalloonStore::new();
    let mut spec = SpecState::new();
    let mut dprof = DprofState::new();
    let mut register = BalloonRegister::new();

    for (idx, ev) in events.iter().enumerate() {
        let ok = (|| -> Result<bool, CharBalloonError> {
            let inp = DrawingCharInput {
                kind: ev.characteristic.kind,
                nominal: ev.characteristic.nominal.clone(),
                upper: ev.characteristic.upper.clone(),
                lower: ev.characteristic.lower.clone(),
                dialect: Some(ev.characteristic.dialect),
                datum_refs: ev.characteristic.datum_refs.clone(),
                name: ev.characteristic.name.clone(),
            };
            validate_char_quantities(&inp)?;
            spec.create_characteristic(
                &mut dprof,
                ev.principal,
                ev.characteristic.plan.as_str(),
                ev.characteristic.id.as_str(),
                false,
            )?;

            // Re-run balloon register with exact replayed BalloonId via single-shot ID gen.
            let mut fixed_id = FixedV7(Some(ev.balloon.id));
            let b = register.add_balloon(
                sheets,
                &mut fixed_id,
                &AddBalloonParams {
                    drawing_rev: ev.balloon.drawing_rev,
                    sheet: ev.balloon.sheet,
                    characteristic: ev.characteristic.id.clone(),
                    anchor: ev.balloon.anchor,
                    label: ev.balloon.label,
                    number: Some(ev.balloon.number),
                    insert: false,
                    zone: ev.balloon.zone.as_ref().map(|z| z.untrusted_value().as_str().to_string()),
                },
            )?;
            if b != ev.balloon {
                return Ok(false);
            }
            let res = AddCharBalloonResult { characteristic: ev.characteristic.clone(), balloon: b, committed: true };
            let bytes = to_canonical_bytes(&res.to_value())
                .map_err(|e| CharBalloonError::Store(format!("canonicalise: {e}")))?;
            Ok(digest_bytes(&bytes) == ev.result_digest)
        })();

        match ok {
            Ok(true) => store.insert_replayed(ev.clone()),
            _ => {
                return CharBalloonReplayRun {
                    outcome: ReplayOutcome::Divergent,
                    first_mismatch: Some(idx),
                    store,
                    register,
                };
            }
        }
    }

    CharBalloonReplayRun { outcome: ReplayOutcome::Identical, first_mismatch: None, store, register }
}

struct FixedV7(Option<BalloonId>);

impl UuidV7Gen for FixedV7 {
    fn next_v7(&mut self) -> Result<fid_project::Uuid, crate::balloon::sheet::SheetError> {
        self.0
            .take()
            .map(|id| id.uuid())
            .ok_or_else(|| crate::balloon::sheet::SheetError::Store("replay ID exhausted".into()))
    }
}

/// Returns the registry [`CommandDef`] for `report.add_char_balloon@1` (`W2`, CLI & MCP exposed).
pub fn command_def() -> Result<CommandDef, DefError> {
    let text = |s: &str| CValue::Str(s.to_string());
    let strs = |xs: &[&str]| CValue::Arr(xs.iter().map(|s| text(s)).collect());
    let num = crate::json_num::num_usize;
    let object = |req: &[&str], props: CValue| {
        obj([
            ("type", text("object")),
            ("additionalProperties", CValue::Bool(false)),
            ("required", strs(req)),
            ("properties", props),
        ])
    };
    let id = |desc: &str| obj([("type", text("string")), ("pattern", text(ID_PATTERN)), ("description", text(desc))]);
    let qty_schema = || {
        object(
            &["authored", "si", "unit"],
            obj([
                ("authored", obj([("type", text("string"))])),
                ("si", obj([("type", text("string"))])),
                ("unit", obj([("type", text("string"))])),
            ]),
        )
    };
    let pt_schema =
        || object(&["x", "y"], obj([("x", obj([("type", text("string"))])), ("y", obj([("type", text("string"))]))]));
    let ut_schema = || {
        object(
            &["text", "untrusted"],
            obj([
                ("text", obj([("type", text("string")), ("x-untrusted", CValue::Bool(true))])),
                ("untrusted", obj([("const", CValue::Bool(true))])),
            ]),
        )
    };

    let params = object(
        &["anchor", "characteristic", "drawing_rev", "label", "plan", "sheet"],
        obj([
            ("anchor", pt_schema()),
            (
                "characteristic",
                object(
                    &["dialect", "kind", "lower", "nominal", "upper"],
                    obj([
                        (
                            "datum_refs",
                            obj([
                                ("type", text("array")),
                                ("items", obj([("type", text("string")), ("x-untrusted", CValue::Bool(true))])),
                            ]),
                        ),
                        ("dialect", obj([("type", text("string")), ("enum", strs(&["ASME", "ISO"]))])),
                        ("kind", obj([("type", text("string"))])),
                        ("lower", obj([("type", text("string"))])),
                        ("name", obj([("type", strs(&["string", "null"])), ("x-untrusted", CValue::Bool(true))])),
                        ("nominal", obj([("type", text("string"))])),
                        ("upper", obj([("type", text("string"))])),
                    ]),
                ),
            ),
            ("drawing_rev", id("Drawing revision ID (UUIDv7).")),
            ("dry_run", obj([("type", text("boolean")), ("default", CValue::Bool(false))])),
            ("label", pt_schema()),
            ("number", obj([("type", strs(&["string", "null"])), ("default", CValue::Null)])),
            ("plan", obj([("type", text("string")), ("minLength", num(1))])),
            ("sheet", id("Sheet ID (UUIDv7).")),
            (
                "zone",
                obj([
                    ("type", strs(&["string", "null"])),
                    ("default", CValue::Null),
                    ("x-untrusted", CValue::Bool(true)),
                ]),
            ),
        ]),
    );

    let result = object(
        &["balloon", "characteristic", "committed"],
        obj([
            ("balloon", obj([("type", text("object"))])),
            (
                "characteristic",
                object(
                    &["datum_refs", "dialect", "id", "kind", "lower", "name", "nominal", "plan", "unit_algo", "upper"],
                    obj([
                        ("datum_refs", obj([("type", text("array")), ("items", ut_schema())])),
                        ("dialect", obj([("type", text("string")), ("enum", strs(&["ASME", "ISO"]))])),
                        ("id", obj([("type", text("string"))])),
                        ("kind", obj([("type", text("string"))])),
                        ("lower", qty_schema()),
                        ("name", obj([("anyOf", CValue::Arr(vec![ut_schema(), obj([("type", text("null"))])]))])),
                        ("nominal", qty_schema()),
                        ("plan", obj([("type", text("string"))])),
                        ("unit_algo", obj([("type", text("string"))])),
                        ("upper", qty_schema()),
                    ]),
                ),
            ),
            ("committed", obj([("type", text("boolean"))])),
        ]),
    );

    CommandDef::from_value(obj([
        ("id", text("report.add_char_balloon")),
        ("major", num(1)),
        ("title", text("Create a characteristic from a drawing callout and place its balloon")),
        (
            "description",
            text("Atomically creates a GDT characteristic with authored decimal literals and places its linked balloon in one journaled transaction."),
        ),
        ("params_schema", params),
        ("result_schema", result),
        ("side_effect", text("W2")),
        ("mcp_projection", text("exposed")),
        ("determinism", text("D0")),
        ("idempotency", text("none")),
        ("fs_effect", text("none")),
        ("undo", text("head_move")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", strs(&[ADD_CHAR_BALLOON])),
        ("algorithms", strs(&[UNIT_CONVERSION_ALGO])),
        ("exposure", strs(&["cli", "mcp"])),
        ("lifecycle", obj([("state", text("active")), ("since", text("2026-11-22"))])),
        ("human_only", CValue::Bool(false)),
        ("network", text("none")),
        ("web", text("available")),
        ("cli_bare", CValue::Bool(false)),
    ]))
}

/// Parses a `Dialect` from an optional JSON field, returning `report.dialect_missing` when absent.
pub fn require_dialect(raw: Option<&str>) -> Result<Dialect, CharBalloonError> {
    match raw {
        Some(s) => Dialect::parse(s).ok_or(CharBalloonError::DialectMissing),
        None => Err(CharBalloonError::DialectMissing),
    }
}

/// Helper to build a [`DrawingCharInput`] from authored literal strings (`"12.000mm"`, etc.).
pub fn make_char_input_from_literals(
    kind: &str,
    nominal: &str,
    upper: &str,
    lower: &str,
    dialect: Option<&str>,
    datum_refs: &[&str],
    name: Option<&str>,
) -> Result<DrawingCharInput, CharBalloonError> {
    let kind = DrawingCharKind::parse(kind)?;
    let dialect = match dialect {
        Some(d) => Some(Dialect::parse(d).ok_or(CharBalloonError::DialectMissing)?),
        None => None,
    };
    let nominal = AuthoredQuantity::parse_literal(nominal)?;
    let upper = AuthoredQuantity::parse_literal(upper)?;
    let lower = AuthoredQuantity::parse_literal(lower)?;
    let mut datums = Vec::with_capacity(datum_refs.len());
    for &d in datum_refs {
        datums.push(Untrusted::text(d).map_err(|_| CharBalloonError::TextInvalid("invalid datum ref"))?);
    }
    let name = match name {
        Some(n) => Some(Untrusted::text(n).map_err(|_| CharBalloonError::TextInvalid("invalid name"))?),
        None => None,
    };
    Ok(DrawingCharInput { kind, nominal, upper, lower, dialect, datum_refs: datums, name })
}
