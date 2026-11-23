#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! RPT.OUTPUT.PDF.WRITE acceptance tests (AC-1..AC-7, registry/bus schemas, CSV parity).
//!
//! Portable tests (`ac6_cross_platform_deterministic_pdf_bytes_and_no_timestamps`,
//! `ac7_perf_50_pages_20_images_under_10s`) run identically on x86-64, ARM64 and `wasm32-wasip1`.
//! Host tests (`AC-1`, `AC-2`, `AC-4`, `AC-5`) exercise `report.export_pdf@1`, `ExportSink`,
//! CLI `fid report export-pdf --json`, `--dry-run`, journaling, and `ExportMeta` parity.

use fid_report_pdf::draw::{MemoryImageStore, PainterRegistry};
use fid_report_pdf::layout::MeasureRegistry;
use fid_report_pdf::text::FontSet;
use fid_report_pdf::write::{pdf_id_hex16, render_pdf_from_json, PDF_RENDERER_ID, PDF_RENDERER_VERSION};

const BRACKET_RESOLVED: &[u8] = include_bytes!("../../../fixtures/rpt/model/golden/bracket_3stage.resolved.json");
const BRACKET_PDF_SHA256: &str = include_str!("../../../fixtures/rpt/output/pdf/bracket_3stage.pdf.sha256");
const BALLOONED_PNG: &[u8] = include_bytes!("../../../fixtures/rpt/output/draw/ballooned_sheet.png");

#[test]
fn ac6_cross_platform_deterministic_pdf_bytes_and_no_timestamps() {
    let fonts = FontSet::bundled().unwrap();
    let images = MemoryImageStore::new();
    let measurers = MeasureRegistry::new();
    let painters = PainterRegistry::default_builtin();

    let r1 = render_pdf_from_json(BRACKET_RESOLVED, &fonts, &images, &measurers, &painters).unwrap();
    let r2 = render_pdf_from_json(BRACKET_RESOLVED, &fonts, &images, &measurers, &painters).unwrap();
    assert_eq!(r1.bytes, r2.bytes, "rendering twice must produce byte-identical PDFs (REQ-001)");
    assert_eq!(r1.sha256, r2.sha256);

    #[cfg(not(target_arch = "wasm32"))]
    if std::env::var_os("UPDATE_PDF_GOLDEN").is_some() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/rpt/output/pdf");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("bracket_3stage.pdf"), &r1.bytes).unwrap();
        std::fs::write(dir.join("bracket_3stage.pdf.sha256"), format!("{}\n", r1.sha256.as_str())).unwrap();
        return;
    }

    assert_eq!(
        r1.sha256.as_str(),
        BRACKET_PDF_SHA256.trim(),
        "PDF SHA-256 must match golden bracket_3stage.pdf.sha256 across x86-64, ARM64 and wasm32 (AC-1, AC-6)"
    );

    // AC-3: no dates, no /CreationDate, no /ModDate, and /ID == first 16 bytes of resolved_digest.
    let pdf_str = String::from_utf8_lossy(&r1.bytes);
    assert!(r1.bytes.starts_with(b"%PDF-1.7\n"));
    assert!(r1.bytes.ends_with(b"%%EOF\n"));
    assert!(!pdf_str.contains("/CreationDate"));
    assert!(!pdf_str.contains("/ModDate"));
    assert!(!pdf_str.contains("D:20"));
    assert!(!pdf_str.contains("/Author"));

    let want_id = pdf_id_hex16(&r1.resolved_digest);
    let expected_id_clause = format!("/ID [<{want_id}> <{want_id}>]");
    assert!(pdf_str.contains(&expected_id_clause), "trailer must contain exact derived /ID {expected_id_clause}");

    // Embedded ExportMeta carries renderer pdf + version.
    let meta_str = std::str::from_utf8(&r1.export_meta_json).unwrap();
    assert!(meta_str.contains(&format!("\"id\":\"{PDF_RENDERER_ID}\"")));
    assert!(meta_str.contains(&format!("\"version\":\"{PDF_RENDERER_VERSION}\"")));
    assert!(meta_str.contains(r1.resolved_digest.as_str()));
}

#[test]
fn ac7_perf_50_pages_20_images_under_10s() {
    let fonts = FontSet::bundled().unwrap();
    let mut images = MemoryImageStore::new();
    let img_digest = images.insert(BALLOONED_PNG.to_vec());
    let measurers = MeasureRegistry::new();
    let painters = PainterRegistry::default_builtin();

    // Seed-13 deterministic 50-page report with 20 embedded PNG images.
    let mut pages_json = Vec::with_capacity(50);
    let mut state: u64 = 13;
    for p in 0..50u32 {
        let mut blocks = vec![format!(
            r#"{{"id":"hdr-{p}","kind":"text","kind_version":"1","frame":null,"payload":{{"schema":"resolved_text@1","style":"heading","text":"Inspection Sheet {p}"}}}}"#
        )];
        if p < 20 {
            blocks.push(format!(
                r#"{{"id":"img-{p}","kind":"image","kind_version":"1","frame":null,"payload":{{"schema":"resolved_image@1","cas_digest":"{}","media_type":"image/png","width_01mm":8000,"height_01mm":5000}}}}"#,
                img_digest.as_str()
            ));
        }
        let mut rows = Vec::with_capacity(12);
        for r in 0..12u32 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let frac = (state >> 32) % 900;
            let disp = format!("{}.{:03}", 10 + (r % 20), frac);
            rows.push(format!(
                r#"["C{p}_{r}",{{"display":"{disp}","canonical":"{disp}","unit":"mm","markers":[]}},{{"text":"conforms"}}]"#
            ));
        }
        blocks.push(format!(
            r#"{{"id":"tbl-{p}","kind":"table","kind_version":"1","frame":null,"payload":{{"schema":"resolved_table@1","columns":[{{"title":"Char"}},{{"title":"Measured"}},{{"title":"Outcome"}}],"rows":[{}]}}}}"#,
            rows.join(",")
        ));
        pages_json.push(format!(r#"{{"id":"page-{p}","blocks":[{}]}}"#, blocks.join(",")));
    }

    let doc_json = format!(
        r#"{{"digest":"sha256:{}","schema":"resolved_report@1","schema_version":"1","provenance":{{"formatters":[{{"id":"fmt.length@1","version":"1"}}],"kernel_version":"0.1.0","template":null}},"warnings":[],"pages":[{}]}}"#,
        "13".repeat(32),
        pages_json.join(",")
    );

    #[cfg(not(target_arch = "wasm32"))]
    let t0 = std::time::Instant::now();

    let rendered = render_pdf_from_json(doc_json.as_bytes(), &fonts, &images, &measurers, &painters).unwrap();
    assert_eq!(rendered.page_count, 50);
    assert!(rendered.size_bytes > 5_000);

    #[cfg(not(target_arch = "wasm32"))]
    {
        let elapsed = t0.elapsed();
        assert!(elapsed.as_secs() < 10, "50-page 20-image PDF export took {elapsed:?}, budget < 10 s (AC-7)");
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod host_tests {
    use super::*;
    use fid_bus::validate::Compiled;
    use fid_canon::CValue;
    use fid_io_reg::roots::{RootAccess, RootId};
    use fid_io_reg::Principal;
    use fid_registry::{check_schema, tool_name, validate as validate_registry, Catalogue, SideEffect};
    use fid_report::bind::ResolvedReport;
    use fid_report_out::meta::{ExportMeta, ExportSink, LocalRootResolver, RendererVersion};
    use fid_report_pdf::write::cmd::{
        command_defs, declarations, export_pdf, invoke_export_pdf_cli, ExportPdfEnv, ExportPdfParams, MemoryPdfJournal,
        MemoryPdfResolvedLookup, EXPORT_PDF_COMMAND,
    };
    use fid_report_pdf::write::WriteError;
    use std::path::PathBuf;

    struct TmpDir {
        path: PathBuf,
    }

    impl TmpDir {
        fn new(tag: &str) -> TmpDir {
            let path = std::env::temp_dir().join(format!("fid-report-pdf-write-{}-{tag}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            TmpDir { path }
        }
    }

    impl Drop for TmpDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn obj_str(v: &CValue, key: &str) -> String {
        match v {
            CValue::Obj(m) => match m.get(key) {
                Some(CValue::Str(s)) => s.clone(),
                other => panic!("field {key} not string: {other:?}"),
            },
            other => panic!("expected object, got {other:?}"),
        }
    }

    #[test]
    fn ac1_ac2_ac5_cli_export_twice_overwrite_golden_sha_meta_parity_and_dry_run() {
        let tmp = TmpDir::new("ac125");
        let mut resolver = LocalRootResolver::new();
        resolver.insert_local(RootId::new("exports").unwrap(), RootAccess::Export, &tmp.path);
        let sink = ExportSink::new(&resolver);

        let resolved = ResolvedReport::from_canonical_bytes(BRACKET_RESOLVED).unwrap();
        let digest_str = resolved.digest.as_str().to_string();

        let mut lookup = MemoryPdfResolvedLookup::new();
        lookup.insert_resolved(&resolved).unwrap();

        let fonts = FontSet::bundled().unwrap();
        let images = MemoryImageStore::new();
        let measurers = MeasureRegistry::new();
        let painters = PainterRegistry::default_builtin();
        let mut journal = MemoryPdfJournal::new();

        // AC-5: --dry-run renders fully, returns exact SHA-256 & size, writes NO file, journals NOTHING.
        let dry_val = {
            let mut env = ExportPdfEnv {
                principal: Principal::Human,
                lookup: &lookup,
                fonts: &fonts,
                images: &images,
                measurers: &measurers,
                painters: &painters,
                sink: &sink,
                journal: &mut journal,
            };
            invoke_export_pdf_cli(
                &[
                    "--resolved",
                    &digest_str,
                    "--root",
                    "root://exports",
                    "--name",
                    "bracket.pdf",
                    "--dry-run",
                    "--json",
                ],
                &mut env,
            )
            .unwrap()
        };
        assert!(!tmp.path.join("bracket.pdf").exists(), "dry-run must not write a file");
        assert!(journal.events().is_empty(), "dry-run must journal nothing");

        // AC-1: export twice with --overwrite -> byte-identical files, golden SHA-256, 2 journaled events.
        let (out1, exists_err) = {
            let mut env = ExportPdfEnv {
                principal: Principal::Human,
                lookup: &lookup,
                fonts: &fonts,
                images: &images,
                measurers: &measurers,
                painters: &painters,
                sink: &sink,
                journal: &mut journal,
            };
            let o1 = invoke_export_pdf_cli(
                &["--resolved", &digest_str, "--root", "root://exports", "--name", "bracket.pdf", "--json"],
                &mut env,
            )
            .unwrap();
            let err = invoke_export_pdf_cli(
                &["--resolved", &digest_str, "--root", "root://exports", "--name", "bracket.pdf", "--json"],
                &mut env,
            )
            .unwrap_err();
            (o1, err)
        };
        let bytes1 = std::fs::read(tmp.path.join("bracket.pdf")).unwrap();
        assert_eq!(exists_err.code(), "report.export_exists");

        // Second export WITH --overwrite succeeds and produces byte-identical PDF + 2nd journaled event.
        {
            let mut env = ExportPdfEnv {
                principal: Principal::Human,
                lookup: &lookup,
                fonts: &fonts,
                images: &images,
                measurers: &measurers,
                painters: &painters,
                sink: &sink,
                journal: &mut journal,
            };
            invoke_export_pdf_cli(
                &[
                    "--resolved",
                    &digest_str,
                    "--root",
                    "root://exports",
                    "--name",
                    "bracket.pdf",
                    "--overwrite",
                    "--json",
                ],
                &mut env,
            )
            .unwrap();
        }
        let bytes2 = std::fs::read(tmp.path.join("bracket.pdf")).unwrap();
        assert_eq!(bytes1, bytes2, "two exports must be byte-identical");
        assert_eq!(journal.events().len(), 2, "two report.export_pdf@1 events with receipts must be journaled");
        assert_eq!(journal.events()[0].receipt.sha256, journal.events()[1].receipt.sha256);
        assert_eq!(journal.events()[0].receipt.sha256.as_str(), BRACKET_PDF_SHA256.trim());

        // Dry-run SHA-256 equals AC-1's written SHA-256.
        assert_eq!(obj_str(&dry_val, "sha256"), obj_str(&out1, "sha256"));

        // AC-2 / REQ-003: ExportMeta embedded in PDF matches fid-report-out's ExportMeta byte-for-byte!
        let want_meta =
            ExportMeta::from_resolved(&resolved, RendererVersion::new(PDF_RENDERER_ID, PDF_RENDERER_VERSION).unwrap());
        let want_meta_bytes = want_meta.to_canonical_bytes().unwrap();
        let rendered = render_pdf_from_json(BRACKET_RESOLVED, &fonts, &images, &measurers, &painters).unwrap();
        assert_eq!(
            rendered.export_meta_json, want_meta_bytes,
            "PDF embedded ExportMeta must match RPT.OUTPUT.META canonical JSON byte-for-byte"
        );
    }

    #[test]
    fn ac4_unsupported_block_fails_writes_nothing_and_journals_nothing() {
        let tmp = TmpDir::new("ac4");
        let mut resolver = LocalRootResolver::new();
        resolver.insert_local(RootId::new("exports").unwrap(), RootAccess::Export, &tmp.path);
        let sink = ExportSink::new(&resolver);

        let chart_id = "019a9f70-0001-7000-8000-000000000099";
        let digest = format!("sha256:{}", "77".repeat(32));
        let chart_doc = format!(
            r#"{{"digest":"{digest}","schema":"resolved_report@1","schema_version":"1","provenance":{{}},"warnings":[],"pages":[{{"id":"p1","blocks":[{{"id":"{chart_id}","kind":"chart","kind_version":"1","frame":null,"payload":{{}}}}]}}]}}"#
        );

        let mut lookup = MemoryPdfResolvedLookup::new();
        lookup.insert_json(&digest, chart_doc.into_bytes());

        let fonts = FontSet::bundled().unwrap();
        let images = MemoryImageStore::new();
        let measurers = MeasureRegistry::new();
        let painters = PainterRegistry::default_builtin();
        let mut journal = MemoryPdfJournal::new();

        let err = {
            let mut env = ExportPdfEnv {
                principal: Principal::Agent,
                lookup: &lookup,
                fonts: &fonts,
                images: &images,
                measurers: &measurers,
                painters: &painters,
                sink: &sink,
                journal: &mut journal,
            };
            export_pdf(
                &ExportPdfParams {
                    resolved: digest,
                    root: "root://exports".to_string(),
                    name: "chart.pdf".to_string(),
                    overwrite: false,
                    dry_run: false,
                },
                &mut env,
            )
            .unwrap_err()
        };
        assert_eq!(err.code(), "report.unsupported_block");
        assert!(err.to_string().contains(chart_id));
        assert!(!tmp.path.join("chart.pdf").exists(), "no file may be written on unsupported block");
        assert!(journal.events().is_empty(), "journal length must stay unchanged");

        // Agent raw path rejected with raw_path_forbidden.
        let mut lookup2 = MemoryPdfResolvedLookup::new();
        lookup2.insert_json("ok-report", BRACKET_RESOLVED.to_vec());
        let raw_err = {
            let mut env2 = ExportPdfEnv {
                principal: Principal::Agent,
                lookup: &lookup2,
                fonts: &fonts,
                images: &images,
                measurers: &measurers,
                painters: &painters,
                sink: &sink,
                journal: &mut journal,
            };
            export_pdf(
                &ExportPdfParams {
                    resolved: "ok-report".to_string(),
                    root: "/tmp/raw_escape".to_string(),
                    name: "escape.pdf".to_string(),
                    overwrite: false,
                    dry_run: false,
                },
                &mut env2,
            )
            .unwrap_err()
        };
        assert_eq!(raw_err.code(), "raw_path_forbidden");
        assert!(matches!(raw_err, WriteError::Export(_)));
    }

    #[test]
    fn registry_and_bus_schemas_validate() {
        let decls = declarations();
        let raw: Vec<_> = decls.iter().map(|d| d.to_validation()).collect();
        assert_eq!(validate_registry(&raw), Vec::new());

        let catalogue = Catalogue::geo_frame().unwrap();
        let defs = command_defs().unwrap();
        assert_eq!(defs.len(), 1);
        let def = &defs[0];
        assert_eq!(format!("{}@{}", def.id, def.major), EXPORT_PDF_COMMAND);
        assert_eq!(def.side_effect, SideEffect::W1);
        assert_eq!(tool_name(&def.id, def.major), "report_export_pdf");
        assert_eq!(check_schema(&def.params_schema, &catalogue), Vec::new());
        assert_eq!(check_schema(&def.result_schema, &catalogue), Vec::new());
        Compiled::compile(&def.params_schema).unwrap();
        Compiled::compile(&def.result_schema).unwrap();
    }
}
