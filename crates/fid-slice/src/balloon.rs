//! Day-1 2D PDF Auto-Ballooning pipeline wired into `fid-slice` (`milestones/vertical-slice-0.md`,
//! Exit Criterion #3):
//!
//! 1. `IO.REG.PLUGIN.DRAW` + `RPT.BALLOON.SHEET.IMPORT` (`fid-io-draw`, `fid-report::balloon::sheet`)
//! 2. `RPT.BALLOON.AUTO.TEXT` (`fid-report::balloon::auto_text`)
//! 3. `RPT.BALLOON.AUTO.PARSE` (`fid-report::balloon::auto_parse`)
//! 4. `RPT.BALLOON.AUTO.PROPOSE` (`W1` draft branch -> `H2` human adoption via `RPT.BALLOON.CHAR` + `RPT.BALLOON.REG`)
//! 5. `RPT.BALLOON.AUTO.EXPORT` (deterministic vector-overlaid `ballooned.pdf` + AS9102 `form3-starter.csv`)

use std::collections::BTreeMap;
use std::path::Path;

use fid_gdt::chars::dprof::DprofState;
use fid_gdt::chars::spec::{PlanId, SpecState};
use fid_io_reg::{Principal, RootAccess, RootId, RootSet};
use fid_project::Uuid;
use fid_report::balloon::auto_export::{
    export_balloon_pack, BalloonPackResult, EvaluatedRowValue, ExportBalloonPackParams,
};
use fid_report::balloon::auto_propose::{
    AdoptDecision, BalloonProposal, ProposalBranchStore, ProposeBalloonsParams, ZoneGridSpec,
};
use fid_report::balloon::char::{AuthoredUnit, CharBalloonStore, Dialect};
use fid_report::balloon::register::BalloonRegister;
use fid_report::balloon::sheet::{
    add_drawing_balloon, AddDrawingParams, DeterministicV7Gen, DrawingStore, ImportEnv, MemoryDrawingBlobs, RevisionId,
};
use fid_types::Digest;

use crate::SliceError;

/// Embedded A3 engineering drawing fixture (`fixtures/rpt/balloon/auto/vector_bracket.pdf`,
/// 12 dimensions + 3 GD&T feature control frames) for zero-setup `--balloon-demo`.
pub const DEMO_DRAWING_PDF: &[u8] = include_bytes!("../../../fixtures/rpt/balloon/auto/vector_bracket.pdf");

/// Summary row for one adopted balloon in [`BalloonSliceOutput`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BalloonSummaryRow {
    pub number: u32,
    pub zone: String,
    pub kind: String,
    pub raw_text: String,
    pub confidence: String,
    pub anchor_cmm: (i64, i64),
    pub label_cmm: (i64, i64),
}

/// Complete deterministic output of the Day-1 2D PDF Auto-Ballooning wedge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BalloonSliceOutput {
    pub drawing_number: String,
    pub revision_label: String,
    pub source_digest: Digest,
    pub sheet_count: usize,
    pub proposal_branch: String,
    pub branch_digest: Digest,
    pub rows: Vec<BalloonSummaryRow>,
    pub pack: BalloonPackResult,
}

/// Runs the full Day-1 2D PDF Auto-Ballooning pipeline on `pdf_path` and writes deterministic
/// `ballooned.pdf` + `form3-starter.csv` into `out_dir`.
pub fn run_balloon_file(
    pdf_path: &Path,
    out_dir: &Path,
    dialect: Dialect,
    unit: AuthoredUnit,
) -> Result<BalloonSliceOutput, SliceError> {
    let parent = pdf_path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    let file_name = pdf_path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| SliceError::new("bad_path", "drawing path must have a valid UTF-8 filename"))?;

    std::fs::create_dir_all(out_dir)
        .map_err(|e| SliceError::new("io_error", format!("creating {}: {e}", out_dir.display())))?;

    let mut roots = RootSet::new();
    let in_root = RootId::new("drawings").ok_or_else(|| SliceError::new("internal", "bad import root id"))?;
    let out_root = RootId::new("exports").ok_or_else(|| SliceError::new("internal", "bad export root id"))?;
    roots.insert(in_root, RootAccess::Import, parent);
    roots.insert(out_root, RootAccess::Export, out_dir);

    let mut drawing_store = DrawingStore::new();
    let mut blobs = MemoryDrawingBlobs::new();
    let mut ids = DeterministicV7Gen::new(1_760_000_000_000);
    let part_rev = RevisionId::new(Uuid::from_bytes([
        0x01, 0x90, 0x00, 0x00, 0x00, 0x50, 0x70, 0x00, 0x80, 0x00, 0, 0, 0, 0, 0, 1,
    ]))
    .ok_or_else(|| SliceError::new("internal", "invalid demo RevisionId"))?;

    let drawing_number = strip_pdf_suffix_ascii(&file_name).to_string();
    let imported = {
        let mut env = ImportEnv { roots: &roots, principal: Principal::Agent, blobs: &mut blobs, ids: &mut ids };
        add_drawing_balloon(
            &mut drawing_store,
            &mut env,
            &AddDrawingParams {
                part_rev,
                file: format!("root://drawings/{file_name}"),
                drawing_number: drawing_number.clone(),
                revision_label: "A".into(),
                title: Some("Vertical Slice 0 Drawing".into()),
                dry_run: false,
            },
        )
        .map_err(|e| SliceError::new(e.code(), e.to_string()))?
    };
    let drawing_rev = imported.drawing_rev.id;

    let plan = PlanId::parse("SLICE-0").ok_or_else(|| SliceError::new("internal", "invalid plan id"))?;
    let mut spec = SpecState::new();
    let mut dprof = DprofState::new();
    let mut register = BalloonRegister::new();
    let mut char_store = CharBalloonStore::new();
    let mut proposals = ProposalBranchStore::new();

    let batch = proposals
        .propose_balloons(
            &drawing_store,
            &blobs,
            &register,
            &ProposeBalloonsParams {
                plan,
                drawing_rev,
                sheets: None,
                dialect: Some(dialect),
                default_unit: Some(unit),
                zone_grid: ZoneGridSpec::default(),
                ocr_candidates: Vec::new(),
                branch: "agent/slice-balloons".into(),
                dry_run: false,
            },
        )
        .map_err(|e| SliceError::new(e.code(), e.to_string()))?;

    let decisions: Vec<AdoptDecision> = batch
        .proposals
        .iter()
        .filter(|pr| pr.char_input.is_some())
        .map(|pr| AdoptDecision {
            provisional_number: pr.provisional_number,
            edited_char_input: None,
            edited_label: None,
            edited_zone: None,
        })
        .collect();

    proposals
        .adopt_proposal_batch(
            &batch.branch,
            Principal::Human,
            &decisions,
            &mut char_store,
            &mut spec,
            &mut dprof,
            &mut register,
            &drawing_store,
            &mut ids,
        )
        .map_err(|e| SliceError::new(e.code(), e.to_string()))?;

    let evaluated: BTreeMap<_, EvaluatedRowValue> = BTreeMap::new();
    let mut export_journal = Vec::new();
    let pack = export_balloon_pack(
        &drawing_store,
        &blobs,
        &register,
        &char_store,
        &evaluated,
        &roots,
        Principal::Agent,
        &mut export_journal,
        &ExportBalloonPackParams {
            drawing_rev,
            out_dir: "root://exports".into(),
            outcome_marking: false,
            include_unmeasured: true,
            dry_run: false,
        },
    )
    .map_err(|e| SliceError::new(e.code(), e.to_string()))?;

    let rows = batch.proposals.iter().map(proposal_to_row).collect();

    Ok(BalloonSliceOutput {
        drawing_number,
        revision_label: "A".into(),
        source_digest: imported.drawing_rev.source_digest,
        sheet_count: imported.sheets.len(),
        proposal_branch: batch.branch,
        branch_digest: batch.branch_digest,
        rows,
        pack,
    })
}

/// Runs the Day-1 2D PDF Auto-Ballooning demo on the bundled `vector_bracket.pdf` drawing
/// (`12 dimensions + 3 GD&T frames`), writing `ballooned.pdf` and `form3-starter.csv` into `out_dir`.
pub fn run_balloon_demo(out_dir: &Path) -> Result<BalloonSliceOutput, SliceError> {
    std::fs::create_dir_all(out_dir)
        .map_err(|e| SliceError::new("io_error", format!("creating {}: {e}", out_dir.display())))?;
    let demo_pdf = out_dir.join("vector_bracket.pdf");
    std::fs::write(&demo_pdf, DEMO_DRAWING_PDF)
        .map_err(|e| SliceError::new("io_error", format!("writing {}: {e}", demo_pdf.display())))?;
    run_balloon_file(&demo_pdf, out_dir, Dialect::Asme, AuthoredUnit::Mm)
}

fn proposal_to_row(p: &BalloonProposal) -> BalloonSummaryRow {
    let kind = p.char_input.as_ref().map_or("unparsed", |ci| ci.kind.as_str()).to_string();
    let zone = p.zone.as_ref().map_or("", |z| z.untrusted_value().as_str()).to_string();
    BalloonSummaryRow {
        number: p.provisional_number,
        zone,
        kind,
        raw_text: p.raw_text.untrusted_value().as_str().to_string(),
        confidence: p.confidence.as_str().to_string(),
        anchor_cmm: (p.anchor.x, p.anchor.y),
        label_cmm: (p.label.x, p.label.y),
    }
}

fn strip_pdf_suffix_ascii(name: &str) -> &str {
    if name.len() >= 4 && name.as_bytes()[name.len() - 4..].eq_ignore_ascii_case(b".pdf") {
        &name[..name.len() - 4]
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::strip_pdf_suffix_ascii;

    #[test]
    fn strip_pdf_suffix_is_single_and_case_insensitive() {
        assert_eq!(strip_pdf_suffix_ascii("vector_bracket.pdf"), "vector_bracket");
        assert_eq!(strip_pdf_suffix_ascii("VECTOR_BRACKET.PDF"), "VECTOR_BRACKET");
        assert_eq!(strip_pdf_suffix_ascii("Bracket.Pdf"), "Bracket");
        assert_eq!(strip_pdf_suffix_ascii("foo.pdf.pdf"), "foo.pdf");
        assert_eq!(strip_pdf_suffix_ascii("drawing.dwg"), "drawing.dwg");
        assert_eq!(strip_pdf_suffix_ascii(".PDF"), "");
        assert_eq!(strip_pdf_suffix_ascii("零件.PDF"), "零件");
    }
}
