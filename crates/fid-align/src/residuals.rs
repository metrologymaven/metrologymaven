//! `align.get_residuals@1` (R0, D0): the read of an alignment's residual report
//! (ALN.QA.REPORT REQ-005, REQ-006), and the store that keeps reports next to the alignments
//! that reference them.
//!
//! The report type, its builder, statistics and digest are kernel code in `fid-align-solve`;
//! this module only resolves `alignment -> report reference -> stored report`, checks the
//! stored report against the digest the alignment recorded, and writes the summary and one
//! page of entries as canonical JSON. No floating-point arithmetic happens here: values are
//! copied bit for bit and every value is written with its unit.
//!
//! # Parameters
//!
//! `alignment` (required, canonical ID), `summary_only` (boolean, default false), `offset`
//! (integer `>= 0`, default 0), `limit` (integer `1..=100000`, default 1000) and `digest`
//! (optional `sha256:` report digest the caller started paging from). `offset` and `limit`
//! with `summary_only: true` are refused, like any unknown key.
//!
//! # Paging
//!
//! The page holds entries `[offset, min(offset + limit, n))` in index order, and `next_offset`
//! is the offset of the next page or `null` after the last one. A report never changes, so
//! a page is stable for a given report digest; a caller that passes the `digest` of the
//! report it started from gets `report_changed` if the alignment was recomputed since.
//! `limit > 100000` gives `page_too_large`; `offset > n` gives `invalid_cursor` (`offset = n`
//! is the valid empty page after the end).
//!
//! # Result
//!
//! `{"alignment", "page"?: {"entries", "limit", "next_offset", "offset"}, "summary"}`.
//! The summary always carries the report digest: `{"digest", "max_abs", "max_entry",
//! "n_entries", "n_rejected", "n_used", "report", "rms", "weighted", "weighted_rms"}`, where a
//! length is `{"unit": "m", "value": <number>}` and absent statistics are `null`. An entry is
//! `{"index", "input", "item", "kind", "reason", "residual", "used", "vector", "weight"}`;
//! `input` indexes the alignment's `inputs` (command order).

use crate::element::{id_value, obj, parse_id};
use crate::error::AlignError;
use crate::model::{AlignmentModel, MAX_ALIGNMENTS};
use fid_align_solve::{EntryView, Length, ResidualReport};
use fid_canon::CValue;
use fid_types::Digest;
use fid_uuid::Uuid;
use std::collections::BTreeMap;

/// Command ID of the residual read.
pub const GET_RESIDUALS: &str = "align.get_residuals@1";
/// CLI subcommand of `fid align` for the residual read.
pub const CLI_RESIDUALS: &str = "get-residuals";
/// MCP tool name of the residual read.
pub const MCP_RESIDUALS: &str = "align_get_residuals";
/// Most entries one page returns (REQ-ALN.QA.REPORT-006).
pub const MAX_PAGE: u64 = 100_000;
/// Entries per page when `limit` is not given.
pub const DEFAULT_PAGE: u64 = 1_000;
/// Largest integer a JSON number carries exactly.
const MAX_SAFE: u64 = (1 << 53) - 1;

// ------------------------------------------------------------------ store

/// The residual reports of a project, keyed by report ID (the `report.id` an alignment
/// records). Reports are immutable; an alignment's `report.digest` pins which one it means.
#[derive(Debug, Clone, Default)]
pub struct ResidualStore {
    reports: BTreeMap<Uuid, ResidualReport>,
}

impl ResidualStore {
    pub fn new() -> ResidualStore {
        ResidualStore::default()
    }

    /// Stores `report` under `id`. Storing the same report again is a no-op; a different
    /// report under a taken ID gives `id_collision`; more than [`MAX_ALIGNMENTS`] reports give
    /// `input_limit`.
    pub fn insert(&mut self, id: Uuid, report: ResidualReport) -> Result<(), AlignError> {
        if let Some(old) = self.reports.get(&id) {
            return if old.digest() == report.digest() { Ok(()) } else { Err(AlignError::IdCollision(id)) };
        }
        if self.reports.len() >= MAX_ALIGNMENTS {
            return Err(AlignError::InputLimit("reports"));
        }
        self.reports.insert(id, report);
        Ok(())
    }

    pub fn get(&self, id: &Uuid) -> Option<&ResidualReport> {
        self.reports.get(id)
    }

    pub fn len(&self) -> usize {
        self.reports.len()
    }

    pub fn is_empty(&self) -> bool {
        self.reports.is_empty()
    }
}

// ------------------------------------------------------------------ parameters

/// Parameters of `align.get_residuals@1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResidualsParams {
    pub alignment: Uuid,
    pub summary_only: bool,
    pub offset: u64,
    pub limit: u64,
    /// The report digest the caller started paging from.
    pub digest: Option<Digest>,
}

/// A non-negative integer JSON number, at most 2^53 - 1.
fn count(v: &CValue, what: &'static str) -> Result<u64, AlignError> {
    match v {
        CValue::Num(f) if *f >= 0.0 && *f <= MAX_SAFE as f64 && (*f as u64) as f64 == *f => Ok(*f as u64),
        _ => Err(AlignError::BadParams(what)),
    }
}

impl ResidualsParams {
    /// Reads the parameter object. Refuses an unknown key, a malformed value or paging with
    /// `summary_only` (`bad_params`), `limit` 0 (`bad_params`) and `limit` above
    /// [`MAX_PAGE`] (`page_too_large`).
    pub fn from_value(v: &CValue) -> Result<ResidualsParams, AlignError> {
        let CValue::Obj(o) = v else { return Err(AlignError::BadParams("params is not an object")) };
        const KEYS: [&str; 5] = ["alignment", "digest", "limit", "offset", "summary_only"];
        if o.keys().any(|k| !KEYS.contains(&k.as_str())) {
            return Err(AlignError::BadParams("unknown parameter"));
        }
        let alignment = match o.get("alignment") {
            Some(CValue::Str(s)) => parse_id(s).ok_or(AlignError::BadParams("alignment is not a canonical id"))?,
            Some(_) => return Err(AlignError::BadParams("alignment is not a canonical id")),
            None => return Err(AlignError::BadParams("missing alignment")),
        };
        let summary_only = match o.get("summary_only") {
            None => false,
            Some(CValue::Bool(b)) => *b,
            Some(_) => return Err(AlignError::BadParams("summary_only is not a boolean")),
        };
        if summary_only && (o.contains_key("offset") || o.contains_key("limit")) {
            return Err(AlignError::BadParams("offset and limit need summary_only false"));
        }
        let offset = o.get("offset").map(|v| count(v, "offset is not a non-negative integer")).transpose()?;
        let limit = o.get("limit").map(|v| count(v, "limit is not a non-negative integer")).transpose()?;
        let limit = limit.unwrap_or(DEFAULT_PAGE);
        if limit == 0 {
            return Err(AlignError::BadParams("limit must be at least 1"));
        }
        if limit > MAX_PAGE {
            return Err(AlignError::PageTooLarge(limit));
        }
        let digest = match o.get("digest") {
            None => None,
            Some(CValue::Str(s)) => Some(Digest::parse(s).map_err(|_| AlignError::BadParams("digest is not sha256:"))?),
            Some(_) => return Err(AlignError::BadParams("digest is not a string")),
        };
        Ok(ResidualsParams { alignment, summary_only, offset: offset.unwrap_or(0), limit, digest })
    }
}

/// Reads `get-residuals` CLI arguments (after the subcommand) into the MCP parameter object:
/// `<alignment> [--summary-only] [--offset N] [--limit M] [--digest D] [--json]`. Unknown or
/// repeated options and non-decimal numbers are refused (`bad_params`).
pub(crate) fn cli_params(rest: &[&str]) -> Result<CValue, AlignError> {
    let mut params = BTreeMap::new();
    let mut json = false;
    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        let (key, value) = match *arg {
            "--json" if !json => {
                json = true;
                continue;
            }
            "--summary-only" => ("summary_only", CValue::Bool(true)),
            "--offset" | "--limit" => {
                let text = it.next().ok_or(AlignError::BadParams("option needs a value"))?;
                (if *arg == "--offset" { "offset" } else { "limit" }, CValue::Num(decimal(text)? as f64))
            }
            "--digest" => {
                let text = it.next().ok_or(AlignError::BadParams("option needs a value"))?;
                ("digest", CValue::Str(text.to_string()))
            }
            s if s.starts_with("--") => return Err(AlignError::BadParams("unknown option")),
            s => ("alignment", CValue::Str(s.to_string())),
        };
        if params.insert(key.to_string(), value).is_some() {
            return Err(AlignError::BadParams("repeated option"));
        }
    }
    Ok(CValue::Obj(params))
}

/// A decimal of ASCII digits (1 to 16 of them), at most 2^53 - 1.
fn decimal(text: &str) -> Result<u64, AlignError> {
    let bad = AlignError::BadParams("expected a non-negative decimal integer");
    if text.is_empty() || text.len() > 16 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(bad);
    }
    match text.parse::<u64>() {
        Ok(n) if n <= MAX_SAFE => Ok(n),
        _ => Err(bad),
    }
}

// ------------------------------------------------------------------ result

fn num(n: u64) -> CValue {
    // Every count and index here is below 2^53, so the conversion is exact.
    CValue::Num(n as f64)
}

fn length(l: Option<Length>) -> CValue {
    match l {
        Some(l) => obj([("unit", CValue::Str(l.unit().as_str().to_string())), ("value", CValue::Num(l.value()))]),
        None => CValue::Null,
    }
}

/// The summary of a stored report.
pub fn summary_value(report_id: &Uuid, report: &ResidualReport) -> CValue {
    let st = report.stats();
    obj([
        ("digest", CValue::Str(report.digest().as_str().to_string())),
        ("max_abs", length(st.max_abs())),
        ("max_entry", st.max_entry().map_or(CValue::Null, num)),
        ("n_entries", num(report.len() as u64)),
        ("n_rejected", num(st.n_rejected())),
        ("n_used", num(st.n_used())),
        ("report", id_value(report_id)),
        ("rms", length(st.rms())),
        ("weighted", CValue::Bool(report.weighted())),
        ("weighted_rms", length(st.weighted_rms())),
    ])
}

/// One entry with its units.
pub fn entry_value(e: &EntryView<'_>) -> CValue {
    let vector = match e.vector {
        Some(v) => obj([
            ("unit", CValue::Str(v.unit().as_str().to_string())),
            ("value", CValue::Arr(v.value().iter().map(|c| CValue::Num(*c)).collect())),
        ]),
        None => CValue::Null,
    };
    obj([
        ("index", num(e.index)),
        ("input", num(u64::from(e.reference.input))),
        ("item", num(e.reference.item)),
        ("kind", CValue::Str(e.kind.as_str().to_string())),
        ("reason", e.reason.map_or(CValue::Null, |r| CValue::Str(r.to_string()))),
        ("residual", length(Some(e.residual))),
        ("used", CValue::Bool(e.used)),
        ("vector", vector),
        ("weight", e.weight.map_or(CValue::Null, CValue::Num)),
    ])
}

/// The residual read. Fails `not_found` for an unknown alignment or a report missing from
/// the store, `digest_mismatch` if the stored report is not the one the alignment recorded,
/// `report_changed` if `digest` is given and differs, and `invalid_cursor` for an offset past
/// the end.
pub fn get_residuals(
    model: &AlignmentModel,
    reports: &ResidualStore,
    params: &ResidualsParams,
) -> Result<CValue, AlignError> {
    let alignment = model.get(&params.alignment).ok_or(AlignError::NotFound(params.alignment))?;
    let rref = alignment.report();
    let report = reports.get(&rref.id).ok_or(AlignError::NotFound(rref.id))?;
    if report.digest() != rref.digest {
        return Err(AlignError::DigestMismatch);
    }
    if params.digest.is_some_and(|d| d != report.digest()) {
        return Err(AlignError::ReportChanged);
    }
    let mut out = BTreeMap::new();
    out.insert("alignment".to_string(), id_value(&params.alignment));
    out.insert("summary".to_string(), summary_value(&rref.id, report));
    if !params.summary_only {
        let n = report.len() as u64;
        if params.offset > n {
            return Err(AlignError::InvalidCursor { offset: params.offset, len: n });
        }
        let end = params.offset.saturating_add(params.limit).min(n);
        // Offsets are at most n, which fits usize.
        let entries = report.entries(params.offset as usize, (end - params.offset) as usize).map(|e| entry_value(&e));
        out.insert(
            "page".to_string(),
            obj([
                ("entries", CValue::Arr(entries.collect())),
                ("limit", num(params.limit)),
                ("next_offset", if end < n { num(end) } else { CValue::Null }),
                ("offset", num(params.offset)),
            ]),
        );
    }
    Ok(CValue::Obj(out))
}
