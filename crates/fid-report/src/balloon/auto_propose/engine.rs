//! Pipeline engine, collision-free label placement, W1 draft-branch staging, H2 human batch
//! adoption, registry `CommandDef` and exact replay for `RPT.BALLOON.AUTO.PROPOSE`.

use super::model::{
    AdoptBatchResult, AdoptDecision, AdoptedProvenance, BalloonProposal, BalloonProposalBatch, Diagnostic, OcrTextRun,
    ProposalOrigin, ProposeBalloonsParams, ProposeError, DIAG_RASTER_NEEDS_OCR, MAX_OCR_CANDIDATES,
};
use crate::balloon::auto_parse::{
    parse_callout_text, parse_drawing_text_map, CalloutKind, GdtSymbol, ParsedCallout, UnitHint,
};
use crate::balloon::auto_text::{extract_drawing_text, ExtractDrawingTextParams, PageKind, SheetRect};
use crate::balloon::char::{
    AddCharBalloonParams, AuthoredQuantity, AuthoredUnit, CharBalloonStore, Dialect, DrawingCharInput, DrawingCharKind,
};
use crate::balloon::register::{BalloonRegister, SheetPoint};
use crate::balloon::sheet::{digest_bytes, DrawingBlobBackend, DrawingStore, SheetInfo, UuidV7Gen};
use crate::doc::defs::ID_PATTERN;
use crate::doc::model::obj;
use crate::doc::text::Untrusted;
use fid_canon::{to_canonical_bytes, CValue};
use fid_gdt::chars::dprof::DprofState;
use fid_gdt::chars::spec::SpecState;
use fid_io_reg::Principal;
use fid_registry::{CommandDef, DefError};
use fid_replay::ReplayOutcome;
use std::collections::BTreeMap;

/// Canonical command / event ID for batched W1 balloon proposals (`RPT.BALLOON.AUTO.PROPOSE`).
pub const PROPOSE_BALLOONS: &str = "report.propose_balloons@1";

/// UX.REPORT.BALLOON standard balloon radius (3.5 mm = 350 cmm) and minimum center-to-center
/// clearance (8.5 mm = 850 cmm) for collision-free label placement.
const BALLOON_CLEARANCE_CMM: i64 = 850;
const BALLOON_MARGIN_CMM: i64 = 400;

/// Draft/agent branch store holding W1 proposal batches isolated from the working document (`REQ-001`).
#[derive(Debug, Default, Clone)]
pub struct ProposalBranchStore {
    branches: BTreeMap<String, BalloonProposalBatch>,
    provenance: Vec<AdoptedProvenance>,
    journal: Vec<BalloonProposalBatch>,
}

impl ProposalBranchStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn branch(&self, name: &str) -> Option<&BalloonProposalBatch> {
        self.branches.get(name)
    }

    pub fn provenance(&self) -> &[AdoptedProvenance] {
        &self.provenance
    }

    pub fn journal(&self) -> &[BalloonProposalBatch] {
        &self.journal
    }

    /// Executes `report.propose_balloons@1` (W1, D0, `REQ-001..004`, `REQ-006`, `AC-1`, `AC-3`).
    ///
    /// Never mutates `register`, `drawing_store`, or any working-document store.
    pub fn propose_balloons(
        &mut self,
        drawing_store: &DrawingStore,
        blobs: &dyn DrawingBlobBackend,
        register: &BalloonRegister,
        p: &ProposeBalloonsParams,
    ) -> Result<BalloonProposalBatch, ProposeError> {
        if p.branch.trim().is_empty() || p.branch.len() > 128 {
            return Err(ProposeError::BadParams("branch name must be 1..=128 bytes"));
        }
        if p.ocr_candidates.len() > MAX_OCR_CANDIDATES {
            return Err(ProposeError::LimitExceeded {
                limit: "ocr_candidates",
                detail: format!("ocr_candidates exceeds {MAX_OCR_CANDIDATES}"),
            });
        }
        let rev = drawing_store.get_revision(p.drawing_rev).ok_or(ProposeError::NotFound("drawing_rev"))?;

        let target_sheets = match &p.sheets {
            Some(list) if !list.is_empty() => {
                for &sid in list {
                    if !rev.sheets.contains(&sid) {
                        return Err(ProposeError::NotFound("sheet not in drawing_rev"));
                    }
                }
                list.clone()
            }
            _ => rev.sheets.clone(),
        };

        // Validate every caller-supplied OCR candidate upfront (sheet bounds + untrusted text C6, REQ-004).
        let mut ocr_by_sheet: BTreeMap<crate::balloon::sheet::SheetId, Vec<&OcrTextRun>> = BTreeMap::new();
        for cand in &p.ocr_candidates {
            let (w, h) = drawing_store.bounds(cand.sheet).ok_or(ProposeError::NotFound("ocr sheet bounds"))?;
            if cand.bbox.x0 < 0 || cand.bbox.y0 < 0 || cand.bbox.x1 > w || cand.bbox.y1 > h {
                return Err(ProposeError::BalloonOffSheet {
                    x: cand.bbox.x1.max(cand.bbox.x0),
                    y: cand.bbox.y1.max(cand.bbox.y0),
                    width_cmm: w,
                    height_cmm: h,
                });
            }
            let _ = Untrusted::text(&cand.text).map_err(|_| ProposeError::TextInvalid("invalid ocr text"))?;
            ocr_by_sheet.entry(cand.sheet).or_default().push(cand);
        }

        let mut next_num = register.max_number(p.drawing_rev).saturating_add(1);
        let mut proposals = Vec::new();
        let mut diagnostics = Vec::new();
        let mut skipped_runs: u32 = 0;

        // Pre-seed placed label centers from existing balloons so new labels don't overlap old ones.
        let mut occupied_by_sheet: BTreeMap<crate::balloon::sheet::SheetId, Vec<SheetPoint>> = BTreeMap::new();
        for b in register.list(p.drawing_rev) {
            occupied_by_sheet.entry(b.sheet).or_default().push(b.label);
        }

        for sheet_id in target_sheets {
            let (sw, sh) = drawing_store.bounds(sheet_id).ok_or(ProposeError::NotFound("sheet bounds"))?;
            let text_map = extract_drawing_text(
                drawing_store,
                blobs,
                &ExtractDrawingTextParams { sheet: sheet_id, include_frame_boxes: true },
            )
            .map_err(|e| match e {
                crate::balloon::auto_text::AutoTextError::LimitExceeded { limit, detail } => {
                    ProposeError::LimitExceeded { limit, detail }
                }
                crate::balloon::auto_text::AutoTextError::NotFound(w) => ProposeError::NotFound(w),
                other => ProposeError::Store(other.to_string()),
            })?;

            let sheet_ocrs = ocr_by_sheet.get(&sheet_id).map_or(&[][..], Vec::as_slice);
            if text_map.page_kind == PageKind::RasterOnly && sheet_ocrs.is_empty() {
                diagnostics.push(Diagnostic {
                    code: DIAG_RASTER_NEEDS_OCR.to_string(),
                    sheet: Some(sheet_id),
                    detail: "sheet is raster_only and no OCR candidates were provided".to_string(),
                });
                continue;
            }

            let parsed_vec = parse_drawing_text_map(&text_map);
            skipped_runs = skipped_runs.saturating_add(parsed_vec.excluded_run_ids.len() as u32);

            let mut sheet_candidates: Vec<(ParsedCallout, ProposalOrigin)> =
                parsed_vec.callouts.into_iter().map(|c| (c, ProposalOrigin::VectorParse)).collect();

            // Merge validated OCR candidates, deduplicating against vector callout boxes by IoU > 0.50.
            let vec_boxes: Vec<SheetRect> = sheet_candidates.iter().map(|(c, _)| c.bbox).collect();
            for cand in sheet_ocrs {
                if vec_boxes.iter().any(|&vb| iou_exceeds_half(vb, cand.bbox)) {
                    continue;
                }
                if let Some(parsed) = parse_callout_text(&cand.text, cand.bbox, Vec::new()) {
                    sheet_candidates.push((parsed, ProposalOrigin::OcrCandidate));
                } else {
                    skipped_runs = skipped_runs.saturating_add(1);
                }
            }

            // Deterministic reading order across merged candidates.
            sheet_candidates.sort_by(|(a, _), (b, _)| {
                a.bbox
                    .y0
                    .cmp(&b.bbox.y0)
                    .then_with(|| a.bbox.x0.cmp(&b.bbox.x0))
                    .then_with(|| a.raw_text.untrusted_value().as_str().cmp(b.raw_text.untrusted_value().as_str()))
            });

            let occupied = occupied_by_sheet.entry(sheet_id).or_default();
            for (callout, origin) in sheet_candidates {
                let anchor = SheetPoint::new(
                    ((callout.bbox.x0 + callout.bbox.x1) / 2).clamp(0, sw),
                    ((callout.bbox.y0 + callout.bbox.y1) / 2).clamp(0, sh),
                );
                let label = place_label_non_overlapping(callout.bbox, (sw, sh), occupied);
                occupied.push(label);

                let zone = p.zone_grid.zone_for(anchor, (sw, sh)).and_then(|z| Untrusted::text(&z).ok());

                let unit = resolve_unit(&callout, p.default_unit, parsed_vec.title_block_hints.unit_hint);
                let (char_input, missing_fields) = build_char_input(&callout, p.dialect, unit);
                let needs_confirmation = p.dialect.is_none()
                    || unit.is_none()
                    || missing_fields
                    || callout.confidence != crate::balloon::auto_parse::ConfidenceWord::High;

                let provisional_number = next_num;
                next_num = next_num.saturating_add(1);

                proposals.push(BalloonProposal {
                    provisional_number,
                    sheet: sheet_id,
                    anchor,
                    label,
                    source_bbox: callout.bbox,
                    zone,
                    raw_text: callout.raw_text,
                    char_input,
                    origin,
                    confidence: callout.confidence,
                    reasons: callout.reasons,
                    needs_confirmation,
                });
            }
        }

        // Compute deterministic canonical D0 branch digest over proposals + diagnostics (REQ-006).
        let digest_payload = obj([
            ("branch", CValue::Str(p.branch.clone())),
            ("diagnostics", CValue::Arr(diagnostics.iter().map(Diagnostic::to_value).collect())),
            ("drawing_rev", CValue::Str(p.drawing_rev.to_string())),
            ("plan", CValue::Str(p.plan.as_str().into())),
            ("proposals", CValue::Arr(proposals.iter().map(BalloonProposal::to_value).collect())),
            ("skipped_runs", CValue::Str(skipped_runs.to_string())),
        ]);
        let canon =
            to_canonical_bytes(&digest_payload).map_err(|e| ProposeError::Store(format!("canonicalise batch: {e}")))?;
        let branch_digest = digest_bytes(&canon);

        let batch = BalloonProposalBatch {
            branch: p.branch.clone(),
            plan: p.plan.clone(),
            drawing_rev: p.drawing_rev,
            proposals,
            skipped_runs,
            diagnostics,
            branch_digest,
        };

        if !p.dry_run {
            self.branches.insert(p.branch.clone(), batch.clone());
            self.journal.push(batch.clone());
        }

        Ok(batch)
    }

    /// Executes H2 batch human adoption of accepted proposals from a staged W1 draft branch
    /// (`REQ-005`, `AC-2`, CONSTITUTION C5).
    ///
    /// - Non-human principals (`Agent`, `Script`) are refused with `core.human_only`.
    /// - Runs atomically: if any accepted item fails validation, zero working-document state
    ///   changes are committed.
    #[allow(clippy::too_many_arguments)]
    pub fn adopt_proposal_batch(
        &mut self,
        branch: &str,
        principal: Principal,
        decisions: &[AdoptDecision],
        char_store: &mut CharBalloonStore,
        spec: &mut SpecState,
        dprof: &mut DprofState,
        register: &mut BalloonRegister,
        sheets: &dyn SheetInfo,
        ids: &mut dyn UuidV7Gen,
    ) -> Result<AdoptBatchResult, ProposeError> {
        // CONSTITUTION C5 / AC-2: ONLY a human principal may adopt proposals into the working doc!
        if principal != Principal::Human {
            return Err(ProposeError::HumanOnly);
        }
        let batch = self.branches.get(branch).ok_or(ProposeError::NotFound("proposal branch"))?.clone();

        // Snapshot all working-document collaborators for atomic all-or-nothing batch adoption.
        let mut char_tx = char_store.clone();
        let mut spec_tx = spec.clone();
        let mut dprof_tx = dprof.clone();
        let mut reg_tx = register.clone();
        let mut adopted = Vec::with_capacity(decisions.len());

        for dec in decisions {
            let prop = batch
                .proposals
                .iter()
                .find(|pr| pr.provisional_number == dec.provisional_number)
                .ok_or(ProposeError::NotFound("provisional_number"))?;

            let char_input = match &dec.edited_char_input {
                Some(edited) => edited.clone(),
                None => prop.char_input.clone().ok_or(ProposeError::IncompleteProposal {
                    provisional_number: prop.provisional_number,
                    detail: "proposal requires human characteristic input before adoption",
                })?,
            };
            let label = dec.edited_label.unwrap_or(prop.label);
            let zone = match &dec.edited_zone {
                Some(z) => Some(z.clone()),
                None => prop.zone.as_ref().map(|z| z.untrusted_value().as_str().to_string()),
            };

            let res = char_tx.add_char_balloon(
                &mut spec_tx,
                &mut dprof_tx,
                &mut reg_tx,
                sheets,
                ids,
                Principal::Human,
                &AddCharBalloonParams {
                    plan: batch.plan.clone(),
                    drawing_rev: batch.drawing_rev,
                    sheet: prop.sheet,
                    characteristic: char_input,
                    anchor: prop.anchor,
                    label,
                    number: Some(prop.provisional_number),
                    zone,
                    dry_run: false,
                },
            )?;

            adopted.push(AdoptedProvenance {
                characteristic_id: res.characteristic.id,
                balloon_id: res.balloon.id,
                balloon_number: res.balloon.number,
                source_sheet: prop.sheet,
                source_bbox: prop.source_bbox,
                raw_text: prop.raw_text.clone(),
                origin: prop.origin,
            });
        }

        // Commit entire H2 batch atomically.
        *char_store = char_tx;
        *spec = spec_tx;
        *dprof = dprof_tx;
        *register = reg_tx;
        self.provenance.extend(adopted.clone());

        Ok(AdoptBatchResult { branch: branch.to_string(), adopted })
    }
}

/// Exact integer IoU > 0.50 check (`2 * inter > union`) with zero floats.
fn iou_exceeds_half(a: SheetRect, b: SheetRect) -> bool {
    let ix = (a.x1.min(b.x1) - a.x0.max(b.x0)).max(0) as u128;
    let iy = (a.y1.min(b.y1) - a.y0.max(b.y0)).max(0) as u128;
    let inter = ix.saturating_mul(iy);
    if inter == 0 {
        return false;
    }
    let area_a = (a.width().max(0) as u128).saturating_mul(a.height().max(0) as u128);
    let area_b = (b.width().max(0) as u128).saturating_mul(b.height().max(0) as u128);
    let union = area_a.saturating_add(area_b).saturating_sub(inter);
    inter.saturating_mul(2) > union
}

/// Places a balloon label center along an 8-direction integer spiral outside `bbox`, keeping it
/// inside sheet bounds and non-overlapping with all previously placed `occupied` label centers.
fn place_label_non_overlapping(bbox: SheetRect, bounds: (i64, i64), occupied: &[SheetPoint]) -> SheetPoint {
    let (sw, sh) = bounds;
    let cx = (bbox.x0 + bbox.x1) / 2;
    let cy = (bbox.y0 + bbox.y1) / 2;
    let min_x = BALLOON_MARGIN_CMM.min(sw / 2);
    let max_x = (sw - BALLOON_MARGIN_CMM).max(min_x);
    let min_y = BALLOON_MARGIN_CMM.min(sh / 2);
    let max_y = (sh - BALLOON_MARGIN_CMM).max(min_y);

    // 8 canonical directions (N, NE, E, NW, SE, S, W, SW) at expanding integer radii.
    const DIRS: [(i64, i64); 8] = [(0, -1), (1, -1), (1, 0), (-1, -1), (1, 1), (0, 1), (-1, 0), (-1, 1)];
    let base_dx = (bbox.width() / 2).max(500) + 500;
    let base_dy = (bbox.height() / 2).max(400) + 550;
    let min_dist_sq = i128::from(BALLOON_CLEARANCE_CMM) * i128::from(BALLOON_CLEARANCE_CMM);

    for ring in 1..=8i64 {
        for &(dx_u, dy_u) in &DIRS {
            let lx = (cx + dx_u * (base_dx + (ring - 1) * BALLOON_CLEARANCE_CMM)).clamp(min_x, max_x);
            let ly = (cy + dy_u * (base_dy + (ring - 1) * BALLOON_CLEARANCE_CMM)).clamp(min_y, max_y);
            let cand = SheetPoint::new(lx, ly);
            let collides = occupied.iter().any(|p| {
                let ddx = i128::from(cand.x - p.x);
                let ddy = i128::from(cand.y - p.y);
                ddx * ddx + ddy * ddy < min_dist_sq
            });
            if !collides {
                return cand;
            }
        }
    }
    SheetPoint::new(cx.clamp(min_x, max_x), (bbox.y0 - 600).clamp(min_y, max_y))
}

fn resolve_unit(
    c: &ParsedCallout,
    default_unit: Option<AuthoredUnit>,
    title_unit: Option<UnitHint>,
) -> Option<AuthoredUnit> {
    if matches!(c.kind, CalloutKind::Angle) {
        return Some(AuthoredUnit::Deg);
    }
    match c.unit_hint.or(title_unit) {
        Some(UnitHint::Mm) => Some(AuthoredUnit::Mm),
        Some(UnitHint::Inch) => Some(AuthoredUnit::Inch),
        Some(UnitHint::Deg) => Some(AuthoredUnit::Deg),
        None => default_unit,
    }
}

fn build_char_input(
    c: &ParsedCallout,
    dialect: Option<Dialect>,
    unit: Option<AuthoredUnit>,
) -> (Option<DrawingCharInput>, bool) {
    let kind = match c.kind {
        CalloutKind::Linear | CalloutKind::BasicDimension | CalloutKind::ReferenceDimension => DrawingCharKind::Linear,
        CalloutKind::Diameter | CalloutKind::SphericalDiameter => DrawingCharKind::Diameter,
        CalloutKind::Radius | CalloutKind::SphericalRadius => DrawingCharKind::Radius,
        CalloutKind::Angle => DrawingCharKind::Angle,
        CalloutKind::Chamfer | CalloutKind::Thread | CalloutKind::SurfaceFinish => DrawingCharKind::Linear,
        CalloutKind::GdtFcf(sym) => match sym {
            GdtSymbol::Position => DrawingCharKind::Position,
            GdtSymbol::Flatness => DrawingCharKind::Flatness,
            GdtSymbol::Circularity => DrawingCharKind::Circularity,
            GdtSymbol::Cylindricity => DrawingCharKind::Cylindricity,
            GdtSymbol::Perpendicularity => DrawingCharKind::Perpendicularity,
            GdtSymbol::Parallelism => DrawingCharKind::Parallelism,
            GdtSymbol::Angularity => DrawingCharKind::Angularity,
            GdtSymbol::ProfileSurface => DrawingCharKind::ProfileSurface,
            GdtSymbol::ProfileLine => DrawingCharKind::ProfileLine,
            GdtSymbol::CircularRunout => DrawingCharKind::CircularRunout,
            GdtSymbol::TotalRunout => DrawingCharKind::TotalRunout,
            GdtSymbol::Concentricity => DrawingCharKind::Concentricity,
            GdtSymbol::Symmetry => DrawingCharKind::Symmetry,
            GdtSymbol::Straightness => DrawingCharKind::Straightness,
        },
    };
    let Some(u) = unit else {
        return (None, true);
    };
    let (nom_str, up_str, lo_str, missing_tol) = if matches!(c.kind, CalloutKind::GdtFcf(_)) {
        // For GD&T FCF, zone tolerance is upper bound from 0.
        let Some(tol) = c.nominal.as_ref() else {
            return (None, true);
        };
        ("0", tol.as_str(), "0", false)
    } else if let Some(nom) = c.nominal.as_ref() {
        let up = c.upper.as_ref().map_or("0", |d| d.as_str());
        let lo = c.lower.as_ref().map_or("0", |d| d.as_str());
        (nom.as_str(), up, lo, c.upper.is_none() && c.lower.is_none())
    } else if let (Some(lo), Some(hi)) = (c.lower.as_ref(), c.upper.as_ref()) {
        // Limit dimension: preserve authored upper/lower verbatim.
        (lo.as_str(), hi.as_str(), lo.as_str(), false)
    } else {
        return (None, true);
    };

    let (Ok(nominal), Ok(upper), Ok(lower)) =
        (AuthoredQuantity::new(nom_str, u), AuthoredQuantity::new(up_str, u), AuthoredQuantity::new(lo_str, u))
    else {
        return (None, true);
    };

    let ci = DrawingCharInput {
        kind,
        nominal,
        upper,
        lower,
        dialect,
        datum_refs: c.datum_refs.clone(),
        name: Some(c.raw_text.clone()),
    };
    (Some(ci), missing_tol)
}

/// Replays a W1 proposal batch run from CAS + drawing store and confirms byte-identical branch
/// digest (`REQ-006`, `AC-4`).
pub fn replay_propose_exact(
    recorded: &BalloonProposalBatch,
    params: &ProposeBalloonsParams,
    drawing_store: &DrawingStore,
    blobs: &dyn DrawingBlobBackend,
    register: &BalloonRegister,
) -> ReplayOutcome {
    let mut replay_store = ProposalBranchStore::new();
    match replay_store.propose_balloons(drawing_store, blobs, register, params) {
        Ok(got) if got.branch_digest == recorded.branch_digest && got == *recorded => ReplayOutcome::Identical,
        _ => ReplayOutcome::Divergent,
    }
}

/// Returns the registry [`CommandDef`] for `report.propose_balloons@1` (`W1`, CLI & MCP exposed).
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

    let params = object(
        &["drawing_rev", "plan"],
        obj([
            ("branch", obj([("type", text("string")), ("default", text("agent/balloons"))])),
            (
                "default_unit",
                obj([
                    ("type", strs(&["string", "null"])),
                    ("enum", CValue::Arr(vec![text("mm"), text("in"), CValue::Null])),
                    ("default", CValue::Null),
                ]),
            ),
            (
                "dialect",
                obj([
                    ("type", strs(&["string", "null"])),
                    ("enum", CValue::Arr(vec![text("ASME"), text("ISO"), CValue::Null])),
                    ("default", CValue::Null),
                ]),
            ),
            ("drawing_rev", id("Drawing revision ID (UUIDv7).")),
            ("dry_run", obj([("type", text("boolean")), ("default", CValue::Bool(false))])),
            ("ocr_candidates", obj([("type", text("array")), ("items", obj([("type", text("object"))]))])),
            ("plan", obj([("type", text("string")), ("minLength", num(1))])),
            (
                "sheets",
                obj([
                    ("type", strs(&["array", "null"])),
                    ("items", id("Sheet ID (UUIDv7).")),
                    ("default", CValue::Null),
                ]),
            ),
        ]),
    );
    let result = object(
        &["branch", "branch_digest", "diagnostics", "drawing_rev", "plan", "proposals", "skipped_runs"],
        obj([
            ("branch", obj([("type", text("string"))])),
            ("branch_digest", obj([("type", text("string"))])),
            ("diagnostics", obj([("type", text("array")), ("items", obj([("type", text("object"))]))])),
            ("drawing_rev", id("Drawing revision ID (UUIDv7).")),
            ("plan", obj([("type", text("string"))])),
            ("proposals", obj([("type", text("array")), ("items", obj([("type", text("object"))]))])),
            ("skipped_runs", obj([("type", text("string"))])),
        ]),
    );

    CommandDef::from_value(obj([
        ("id", text("report.propose_balloons")),
        ("major", num(1)),
        ("title", text("Propose numbered balloons and characteristics on a W1 draft branch")),
        (
            "description",
            text("Extracts vector text runs, parses engineering callouts, optionally merges validated OCR candidates, places collision-free balloon labels, and stages a W1 proposal batch for H2 human adoption."),
        ),
        ("params_schema", params),
        ("result_schema", result),
        ("side_effect", text("W1")),
        ("mcp_projection", text("exposed")),
        ("determinism", text("D0")),
        ("idempotency", text("natural")),
        ("fs_effect", text("none")),
        ("undo", text("not_undoable")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", strs(&[PROPOSE_BALLOONS])),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(&["cli", "mcp"])),
        ("lifecycle", obj([("state", text("active")), ("since", text("2026-11-22"))])),
        ("human_only", CValue::Bool(false)),
        ("network", text("none")),
        ("web", text("available")),
        ("cli_bare", CValue::Bool(false)),
    ]))
}
