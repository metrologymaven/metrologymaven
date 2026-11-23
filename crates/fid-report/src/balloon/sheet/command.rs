//! Command handlers for `report.add_drawing_balloon@1`, `report.list_drawing_balloon@1` and
//! `report.set_size_drawing_balloon@1` (`RPT.BALLOON.SHEET.IMPORT`).

use super::error::SheetError;
use super::event::{
    canonical_digest, DrawingEvent, ADD_DRAWING_BALLOON, LIST_DRAWING_BALLOON, SET_SIZE_DRAWING_BALLOON,
};
use super::model::{
    compute_raster_bounds_cmm, parse_drawing_text, DrawingRevId, DrawingRevision, DrawingSheet, ReaderWarning,
    RevisionId, SheetId, SheetStatus, SizeSpec,
};
use super::store::{digest_bytes, DrawingBlobBackend, DrawingStore};
use crate::doc::model::obj;
use fid_canon::CValue;
use fid_io_draw::{read_drawing_bytes, MAX_INPUT_BYTES};
use fid_io_reg::{Principal, RootSet};
use fid_project::Uuid;
use std::collections::BTreeMap;
use std::io::Read;

/// Maximum length of the `file` parameter (root handle or path) accepted by `add_drawing_balloon@1`.
pub const MAX_FILE_PARAM_BYTES: usize = 4096;

/// Source of UUIDv7 values for drawing revisions and sheets (production uses `fid_project::IdService`;
/// deterministic tests can supply a repeatable UUIDv7 generator).
pub trait UuidV7Gen {
    fn next_v7(&mut self) -> Result<Uuid, SheetError>;
}

/// Deterministic counter-based UUIDv7 generator for tests and headless replay verification.
#[derive(Debug, Clone)]
pub struct DeterministicV7Gen {
    ms: u64,
    seq: u64,
}

impl DeterministicV7Gen {
    pub fn new(start_ms: u64) -> Self {
        DeterministicV7Gen { ms: start_ms, seq: 1 }
    }
}

impl UuidV7Gen for DeterministicV7Gen {
    fn next_v7(&mut self) -> Result<Uuid, SheetError> {
        let mut bytes = [0u8; 16];
        let ts = self.ms & 0x0000_FFFF_FFFF_FFFF;
        bytes[0] = ((ts >> 40) & 0xFF) as u8;
        bytes[1] = ((ts >> 32) & 0xFF) as u8;
        bytes[2] = ((ts >> 24) & 0xFF) as u8;
        bytes[3] = ((ts >> 16) & 0xFF) as u8;
        bytes[4] = ((ts >> 8) & 0xFF) as u8;
        bytes[5] = (ts & 0xFF) as u8;
        let s = self.seq;
        self.seq = self.seq.saturating_add(1);
        bytes[6] = 0x70 | (((s >> 8) & 0x0F) as u8);
        bytes[7] = (s & 0xFF) as u8;
        bytes[8] = 0x80 | (((s >> 56) & 0x3F) as u8);
        bytes[9] = ((s >> 48) & 0xFF) as u8;
        bytes[10] = ((s >> 40) & 0xFF) as u8;
        bytes[11] = ((s >> 32) & 0xFF) as u8;
        bytes[12] = ((s >> 24) & 0xFF) as u8;
        bytes[13] = ((s >> 16) & 0xFF) as u8;
        bytes[14] = ((s >> 8) & 0xFF) as u8;
        bytes[15] = (s & 0xFF) as u8;
        Ok(Uuid::from_bytes(bytes))
    }
}

/// Execution context for `report.add_drawing_balloon@1` (`REQ-006`).
pub struct ImportEnv<'a> {
    pub roots: &'a RootSet,
    pub principal: Principal,
    pub blobs: &'a mut dyn DrawingBlobBackend,
    pub ids: &'a mut dyn UuidV7Gen,
}

/// Parameters of `report.add_drawing_balloon@1` (`Design: Registry commands`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddDrawingParams {
    pub part_rev: RevisionId,
    pub file: String,
    pub drawing_number: String,
    pub revision_label: String,
    pub title: Option<String>,
    pub dry_run: bool,
}

impl AddDrawingParams {
    pub fn from_value(v: &CValue) -> Result<Self, SheetError> {
        let CValue::Obj(m) = v else {
            return Err(SheetError::BadParams("params must be an object"));
        };
        let part_rev = RevisionId::parse(req_str(m, "part_rev")?)?;
        let file = req_str(m, "file")?.to_string();
        if file.is_empty() || file.len() > MAX_FILE_PARAM_BYTES {
            return Err(SheetError::BadParams("file parameter length invalid"));
        }
        let drawing_number = req_str(m, "drawing_number")?.to_string();
        let revision_label = req_str(m, "revision_label")?.to_string();
        let title = match m.get("title") {
            Some(CValue::Null) | None => None,
            Some(CValue::Str(s)) => Some(s.clone()),
            _ => return Err(SheetError::BadParams("title must be string or null")),
        };
        let dry_run = match m.get("dry_run") {
            Some(CValue::Bool(b)) => *b,
            None => false,
            _ => return Err(SheetError::BadParams("dry_run must be boolean")),
        };
        Ok(AddDrawingParams { part_rev, file, drawing_number, revision_label, title, dry_run })
    }
}

/// Result of `report.add_drawing_balloon@1` (`Design: Registry commands`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddDrawingResult {
    pub drawing_rev: DrawingRevision,
    pub sheets: Vec<DrawingSheet>,
    pub warnings: Vec<ReaderWarning>,
    pub committed: bool,
}

impl AddDrawingResult {
    pub fn to_value(&self) -> CValue {
        obj([
            ("committed", CValue::Bool(self.committed)),
            ("drawing_rev", self.drawing_rev.to_value()),
            ("sheets", CValue::Arr(self.sheets.iter().map(DrawingSheet::to_value).collect())),
            ("warnings", CValue::Arr(self.warnings.iter().map(ReaderWarning::to_value).collect())),
        ])
    }
}

/// Parameters of `report.list_drawing_balloon@1` (`Design: Registry commands`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListDrawingParams {
    pub part_rev: Option<RevisionId>,
}

impl ListDrawingParams {
    pub fn from_value(v: &CValue) -> Result<Self, SheetError> {
        let CValue::Obj(m) = v else {
            return Err(SheetError::BadParams("params must be an object"));
        };
        let part_rev = match m.get("part_rev") {
            Some(CValue::Null) | None => None,
            Some(CValue::Str(s)) => Some(RevisionId::parse(s)?),
            _ => return Err(SheetError::BadParams("part_rev must be uuid string or null")),
        };
        Ok(ListDrawingParams { part_rev })
    }
}

/// One entry returned by `report.list_drawing_balloon@1` (revision + ordered sheets).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedDrawingRevision {
    pub drawing_rev: DrawingRevision,
    pub sheets: Vec<DrawingSheet>,
}

impl ListedDrawingRevision {
    pub fn to_value(&self) -> CValue {
        obj([
            ("drawing_rev", self.drawing_rev.to_value()),
            ("sheets", CValue::Arr(self.sheets.iter().map(DrawingSheet::to_value).collect())),
        ])
    }
}

/// Parameters of `report.set_size_drawing_balloon@1` (`Design: Registry commands`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetSizeParams {
    pub sheet: SheetId,
    pub size: SizeSpec,
}

impl SetSizeParams {
    pub fn from_value(v: &CValue) -> Result<Self, SheetError> {
        let CValue::Obj(m) = v else {
            return Err(SheetError::BadParams("params must be an object"));
        };
        let sheet = SheetId::parse(req_str(m, "sheet")?)?;
        let size_v = m.get("size").ok_or(SheetError::BadParams("missing size"))?;
        let size = SizeSpec::from_value(size_v)?;
        Ok(SetSizeParams { sheet, size })
    }
}

/// Executes `report.add_drawing_balloon@1` (W2; `REQ-001..008`, `AC-1..AC-5`).
pub fn add_drawing_balloon(
    store: &mut DrawingStore,
    env: &mut ImportEnv<'_>,
    p: &AddDrawingParams,
) -> Result<AddDrawingResult, SheetError> {
    // 1. Validate untrusted metadata (<= 256 bytes, no NUL, REQ-004).
    let drawing_number = parse_drawing_text(&p.drawing_number, true)?;
    let revision_label = parse_drawing_text(&p.revision_label, true)?;
    let title = match &p.title {
        Some(t) => Some(parse_drawing_text(t, false)?),
        None => None,
    };

    // 2. Resolve file through approved import roots BEFORE reading any bytes (REQ-006, AC-4).
    let opened = env.roots.resolve_import(env.principal, &p.file).map_err(|e| SheetError::File(e.code()))?;

    // 3. Reject duplicate (part_rev, drawing_number, revision_label) without journaling (REQ-008, AC-4).
    if store.has_revision_key(
        p.part_rev,
        drawing_number.untrusted_value().as_str(),
        revision_label.untrusted_value().as_str(),
    ) {
        return Err(SheetError::DrawingRevisionExists {
            part_rev: p.part_rev.to_string(),
            drawing_number: drawing_number.escaped(),
            revision_label: revision_label.escaped(),
        });
    }

    // 4. Bounded read up to 500 MiB + 1 byte so reader rejects > 500 MiB with `limit_exceeded`.
    let limit = (MAX_INPUT_BYTES as u64).saturating_add(1);
    let mut raw_bytes = Vec::new();
    opened.file.take(limit).read_to_end(&mut raw_bytes).map_err(|_| SheetError::File("unreadable"))?;

    // 5. Decode via IO.REG.PLUGIN.DRAW (never parse PDF or images in RPT; REQ-005, AC-3).
    let parsed = read_drawing_bytes(None, &raw_bytes).map_err(|e| SheetError::DrawingUnreadable {
        code: e.code().to_string(),
        offset: e.byte_offset(),
        detail: e.to_string(),
    })?;

    let source_digest = digest_bytes(&raw_bytes);
    let warnings: Vec<ReaderWarning> = parsed.warnings.into_iter().map(ReaderWarning::from_draw).collect();

    // 6. Allocate UUIDv7 IDs for DrawingRevision and one DrawingSheet per page in order (REQ-001).
    let rev_uuid = env.ids.next_v7()?;
    let rev_id = DrawingRevId::new(rev_uuid).ok_or(SheetError::Store("id generator did not return UUIDv7".into()))?;

    let mut sheets = Vec::with_capacity(parsed.pages.len());
    let mut sheet_ids = Vec::with_capacity(parsed.pages.len());
    let mut raster_blobs: Vec<Option<Vec<u8>>> = Vec::with_capacity(parsed.pages.len());

    for page in parsed.pages {
        let su = env.ids.next_v7()?;
        let sid = SheetId::new(su).ok_or(SheetError::Store("id generator did not return UUIDv7".into()))?;
        let (status, width_cmm, height_cmm, px, blob_bytes) = match page.raster {
            Some(r) => {
                let st = if r.dpi.is_some() && page.width_cmm > 0 && page.height_cmm > 0 {
                    SheetStatus::Ready
                } else {
                    SheetStatus::NeedsSize
                };
                (st, page.width_cmm, page.height_cmm, Some((r.width, r.height)), Some(r.pixels))
            }
            None => (SheetStatus::Ready, page.width_cmm, page.height_cmm, None, None),
        };
        sheet_ids.push(sid);
        raster_blobs.push(blob_bytes);
        sheets.push(DrawingSheet {
            id: sid,
            drawing_rev: rev_id,
            index: page.index,
            width_cmm,
            height_cmm,
            status,
            image: None,
            px,
        });
    }

    if p.dry_run {
        // Compute preview BlobRefs without mutating CAS or journal.
        for (sheet, maybe_pix) in sheets.iter_mut().zip(raster_blobs.iter()) {
            if let Some(pix) = maybe_pix {
                sheet.image = Some(fid_store::cas::BlobRef { digest: digest_bytes(pix), size: pix.len() as u64 });
            }
        }
        let drawing_rev = DrawingRevision {
            id: rev_id,
            part_rev: p.part_rev,
            drawing_number,
            revision_label,
            title,
            source_digest,
            sheets: sheet_ids,
            warnings: warnings.clone(),
            carried_from: BTreeMap::new(),
        };
        return Ok(AddDrawingResult { drawing_rev, sheets, warnings, committed: false });
    }

    // 7. Store source bytes and decoded raster pixel buffers in CAS (REQ-001).
    let source_ref = env.blobs.put_blob(&raw_bytes)?;
    for (sheet, maybe_pix) in sheets.iter_mut().zip(raster_blobs.iter()) {
        if let Some(pix) = maybe_pix {
            sheet.image = Some(env.blobs.put_blob(pix)?);
        }
    }

    let drawing_rev = DrawingRevision {
        id: rev_id,
        part_rev: p.part_rev,
        drawing_number: drawing_number.clone(),
        revision_label: revision_label.clone(),
        title: title.clone(),
        source_digest,
        sheets: sheet_ids.clone(),
        warnings: warnings.clone(),
        carried_from: BTreeMap::new(),
    };

    let res = AddDrawingResult {
        drawing_rev: drawing_rev.clone(),
        sheets: sheets.clone(),
        warnings: warnings.clone(),
        committed: true,
    };
    let result_digest = canonical_digest(&res.to_value())?;

    let event = DrawingEvent::AddDrawingBalloon(Box::new(super::event::AddDrawingEvent {
        drawing_rev: rev_id,
        part_rev: p.part_rev,
        drawing_number,
        revision_label,
        title,
        source_digest,
        source_blob_digest: source_ref.digest,
        sheet_ids,
        warnings,
        result_digest,
    }));

    store.insert_revision_and_sheets(drawing_rev, sheets, event);
    Ok(res)
}

/// Executes `report.list_drawing_balloon@1` (R0; ordered by `drawing_number`, then `revision_label`,
/// then `id`, `Design: Registry commands`, `AC-5`).
pub fn list_drawing_balloon(store: &DrawingStore, p: &ListDrawingParams) -> Vec<ListedDrawingRevision> {
    let mut revs: Vec<&DrawingRevision> =
        store.all_revisions().filter(|r| p.part_rev.map_or(true, |wanted| r.part_rev == wanted)).collect();

    revs.sort_by(|a, b| {
        a.drawing_number
            .untrusted_value()
            .as_str()
            .cmp(b.drawing_number.untrusted_value().as_str())
            .then_with(|| a.revision_label.untrusted_value().as_str().cmp(b.revision_label.untrusted_value().as_str()))
            .then_with(|| a.id.cmp(&b.id))
    });

    revs.into_iter()
        .map(|rev| {
            let sheets: Vec<DrawingSheet> = rev.sheets.iter().filter_map(|sid| store.sheet(*sid).cloned()).collect();
            ListedDrawingRevision { drawing_rev: rev.clone(), sheets }
        })
        .collect()
}

/// Serialises the result of `report.list_drawing_balloon@1` to canonical JSON `CValue`.
pub fn list_drawing_value(rows: &[ListedDrawingRevision]) -> CValue {
    obj([("revisions", CValue::Arr(rows.iter().map(ListedDrawingRevision::to_value).collect()))])
}

/// Executes `report.set_size_drawing_balloon@1` (W2; `REQ-003`, `AC-2`).
pub fn set_size_drawing_balloon(store: &mut DrawingStore, p: &SetSizeParams) -> Result<DrawingSheet, SheetError> {
    let existing = store.sheet(p.sheet).ok_or(SheetError::NotFound("sheet"))?.clone();
    let px = existing.px.ok_or(SheetError::SizeInvalid("sheet is not a raster sheet"))?;
    let (width_cmm, height_cmm) = compute_raster_bounds_cmm(px, p.size)?;

    let mut updated = existing;
    updated.width_cmm = width_cmm;
    updated.height_cmm = height_cmm;
    updated.status = SheetStatus::Ready;
    let result_digest = canonical_digest(&updated.to_value())?;

    let event =
        DrawingEvent::SetSizeDrawingBalloon { sheet: p.sheet, size: p.size, width_cmm, height_cmm, result_digest };
    store.update_sheet_bounds(p.sheet, width_cmm, height_cmm, event)
}

/// Dispatches any of the three `RPT.BALLOON.SHEET.IMPORT` commands by canonical command ID
/// (`report.add_drawing_balloon@1`, `report.list_drawing_balloon@1`, `report.set_size_drawing_balloon@1`).
pub fn execute(
    command: &str,
    params: &CValue,
    store: &mut DrawingStore,
    env: &mut ImportEnv<'_>,
) -> Result<CValue, SheetError> {
    match command {
        ADD_DRAWING_BALLOON => {
            let p = AddDrawingParams::from_value(params)?;
            let res = add_drawing_balloon(store, env, &p)?;
            Ok(res.to_value())
        }
        LIST_DRAWING_BALLOON => {
            let p = ListDrawingParams::from_value(params)?;
            let rows = list_drawing_balloon(store, &p);
            Ok(list_drawing_value(&rows))
        }
        SET_SIZE_DRAWING_BALLOON => {
            let p = SetSizeParams::from_value(params)?;
            let sheet = set_size_drawing_balloon(store, &p)?;
            Ok(sheet.to_value())
        }
        _ => Err(SheetError::BadParams("unknown drawing balloon command")),
    }
}

fn req_str<'a>(m: &'a BTreeMap<String, CValue>, k: &str) -> Result<&'a str, SheetError> {
    match m.get(k) {
        Some(CValue::Str(s)) => Ok(s.as_str()),
        _ => Err(SheetError::BadParams("missing or non-string param")),
    }
}
