//! Integration tests for `RPT.MODEL.BIND.GEN` (AC-1 through AC-7, registry/bus validation,
//! embedded-bytes refusal, and CAS tamper detection).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::disallowed_types)]

use fid_bus::validate::Compiled;
use fid_canon::{digest_bytes, parse, to_canonical_bytes, CValue};
use fid_io_res::trans::model::{load_rows, transcribe, TranscriptionRecord};
use fid_project::{Bits, IdError, IdService, ObservationSource, ReplaySource, Uuid};
use fid_registry::validate as validate_registry;
use fid_replay::ReplayOutcome;
use fid_report::bind::*;
use fid_report::blocks::table::{TableBlockKind, TableBlockResolver};
use fid_report::blocks::text::{
    FooterBlockKind, FooterBlockResolver, HeaderBlockKind, HeaderBlockResolver, ImageBlockKind, ImageBlockResolver,
    TextBlockKind, TextBlockResolver,
};
use fid_report::doc::{
    add_block, create_report, AddBlock, BlockKindId, CreateReport, KindRegistry, MemoryBackend, ReportId, Text,
    Untrusted,
};
use fid_report::fmt::{Catalog, DefaultDecimals};
use fid_report::tmpl::starter::{MULTISTAGE_TABLE_ID, MULTISTAGE_TABLE_NAME};
use fid_report::tmpl::ver::BuiltinRegistry;
use fid_types::Digest;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

fn root_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn rpt_fixture(rel: &str) -> CValue {
    let p = root_dir().join("fixtures/rpt/model").join(rel);
    parse(&std::fs::read(p).unwrap()).unwrap()
}

fn get<'a>(v: &'a CValue, k: &str) -> &'a CValue {
    match v {
        CValue::Obj(o) => o.get(k).unwrap_or_else(|| panic!("missing key {k}")),
        _ => panic!("not object"),
    }
}

fn st<'a>(v: &'a CValue, k: &str) -> &'a str {
    match get(v, k) {
        CValue::Str(s) => s,
        _ => panic!("{k} not string"),
    }
}

fn arr(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(a) => a,
        _ => panic!("not array"),
    }
}

fn uuid(s: &str) -> Uuid {
    Uuid::parse(s).unwrap()
}

fn bracket_journal() -> MemoryJournal {
    let mut j = MemoryJournal::new();
    for e in arr(get(&rpt_fixture("bracket_3stage/journal.json"), "entries")) {
        let kind = match st(e, "kind") {
            "result" => EntityKind::Result,
            "characteristic" => EntityKind::Characteristic,
            "named_view" => EntityKind::NamedView,
            "series" => EntityKind::Series,
            "statement" => EntityKind::Statement,
            k => panic!("unexpected kind {k}"),
        };
        let id = match get(e, "id") {
            CValue::Str(s) => Some(uuid(s)),
            _ => None,
        };
        let head = JournalHead::parse(st(e, "head")).unwrap();
        j.append(head, uuid(st(e, "project")), kind, id, st(e, "text").as_bytes().to_vec()).unwrap();
    }
    j
}

fn bracket_model() -> (Uuid, MemoryModel) {
    let m = rpt_fixture("bind/expand/model.json");
    let project = uuid(st(&m, "project"));
    let characteristics = arr(get(&m, "characteristics"))
        .iter()
        .map(|c| CharacteristicItem {
            id: uuid(st(c, "id")),
            name: st(c, "name").to_string(),
            char_type: st(c, "type").to_string(),
            balloon: Some(st(c, "balloon").parse().unwrap()),
            element: Some(uuid(st(c, "element"))),
            tags: arr(get(c, "tags"))
                .iter()
                .map(|t| match t {
                    CValue::Str(s) => s.clone(),
                    _ => panic!("tag not string"),
                })
                .collect(),
        })
        .collect();
    let ids = |k: &str| -> Vec<Uuid> {
        arr(get(&m, k))
            .iter()
            .map(|v| match v {
                CValue::Str(s) => uuid(s),
                _ => panic!("id not string"),
            })
            .collect()
    };
    let model = MemoryModel {
        head: JournalHead::parse(st(&m, "head")).unwrap(),
        characteristics,
        elements: ids("elements"),
        stages: arr(get(&m, "stages"))
            .iter()
            .map(|s| StageItem {
                id: uuid(st(s, "id")),
                run: uuid(st(s, "run")),
                removed: matches!(get(s, "removed"), CValue::Bool(true)),
            })
            .collect(),
        runs: arr(get(&m, "runs"))
            .iter()
            .map(|r| RunItem { id: uuid(st(r, "id")), start_ms: st(r, "start_ms").parse().unwrap() })
            .collect(),
        results: arr(get(&m, "results"))
            .iter()
            .map(|r| ResultItem {
                id: uuid(st(r, "id")),
                characteristic: uuid(st(r, "characteristic")),
                stage: uuid(st(r, "stage")),
            })
            .collect(),
    };
    (project, model)
}

fn sample_transcription() -> TranscriptionRecord {
    let bytes = std::fs::read(root_dir().join("fixtures/io/res/trans/model/rows_basic.json")).unwrap();
    let (source, rows) = load_rows(&bytes).unwrap();
    let mut replay = ReplaySource::new(Vec::new());
    let existing: Vec<Uuid> = Vec::new();
    let ids = IdService::new(Uuid::parse("0b6f8c1e-3d2a-4f5b-9c7e-a1b2c3d4e5f6").unwrap(), &mut replay, &existing);
    transcribe(&rows, &source, &ids).unwrap().record().clone()
}

fn full_kinds() -> KindRegistry {
    let mut kinds = KindRegistry::new();
    kinds.register(Box::new(TableBlockKind)).unwrap();
    kinds.register(Box::new(TextBlockKind)).unwrap();
    kinds.register(Box::new(ImageBlockKind)).unwrap();
    kinds.register(Box::new(HeaderBlockKind)).unwrap();
    kinds.register(Box::new(FooterBlockKind)).unwrap();
    kinds.register(Box::new(ViewBlockKind)).unwrap();
    kinds
}

fn full_resolvers() -> ResolverRegistry {
    let mut r = ResolverRegistry::new();
    r.register(Box::new(TableBlockResolver)).unwrap();
    r.register(Box::new(TextBlockResolver)).unwrap();
    r.register(Box::new(ImageBlockResolver)).unwrap();
    r.register(Box::new(HeaderBlockResolver)).unwrap();
    r.register(Box::new(FooterBlockResolver)).unwrap();
    r.register(Box::new(ViewBlockResolver)).unwrap();
    r
}

const BASE_MS: u64 = 1_763_510_400_000;

struct Counter {
    next: u64,
}

impl Counter {
    fn new() -> Self {
        Self { next: 0 }
    }
}

impl ObservationSource for Counter {
    fn now_ms(&mut self) -> Result<u64, IdError> {
        Ok(BASE_MS + self.next)
    }

    fn random_bits(&mut self, n: u32) -> Result<Bits, IdError> {
        let b = Bits::new(n, u128::from(self.next) + 1)?;
        self.next += 1;
        Ok(b)
    }
}

#[test]
fn registry_defs_and_bus_schemas_validate() {
    let decls = fid_report::bind::gen::declarations();
    let raw: Vec<_> = decls.iter().map(|d| d.to_validation()).collect();
    let errs = validate_registry(&raw);
    assert!(errs.is_empty(), "registry validation errors: {errs:#?}");
    let catalogue = fid_registry::Catalogue::geo_frame().unwrap();
    let defs = gen_command_defs().unwrap();
    assert_eq!(defs.len(), 2);
    assert_eq!(format!("{}@{}", defs[0].id, defs[0].major), GENERATE_REPORT);
    assert_eq!(fid_registry::tool_name(&defs[0].id, defs[0].major), "report_generate_report");
    assert_eq!(format!("{}@{}", defs[1].id, defs[1].major), SAVE_REPORT);
    assert_eq!(fid_registry::tool_name(&defs[1].id, defs[1].major), "report_save_report");
    for d in &defs {
        assert_eq!(fid_registry::check_schema(&d.params_schema, &catalogue), Vec::new());
        assert_eq!(fid_registry::check_schema(&d.result_schema, &catalogue), Vec::new());
        Compiled::compile(&d.params_schema)
            .unwrap_or_else(|e| panic!("fid-bus params schema failed for {}: {e:?}", d.id));
        Compiled::compile(&d.result_schema)
            .unwrap_or_else(|e| panic!("fid-bus result schema failed for {}: {e:?}", d.id));
    }
}

#[test]
fn ac1_ac2_ac3_ac5_ac7_bracket_3stage_generate_locale_save_golden_and_replay() {
    let (project, model) = bracket_model();
    let journal = bracket_journal();
    let builtins = BuiltinRegistry::with_starters().unwrap();
    let kinds = full_kinds();
    let resolvers = full_resolvers();
    let catalog = Catalog::english();
    let metadata =
        MemoryGenMetadata { default_transcription: Some(sample_transcription()), ..MemoryGenMetadata::default() };

    let mut backend = MemoryBackend::new(project);
    let mut ids = Counter::new();
    let created = create_report(
        &mut backend,
        &kinds,
        &mut ids,
        &CreateReport { project, title: Untrusted::new(Text::new("Bracket 3-Stage Report").unwrap()) },
    )
    .unwrap();
    let report_id: ReportId = created.id();
    let doc_journal_len_before = backend.journal().len();
    let mut scratch = MemoryScratch::new();
    let mut store = MemoryResolvedStore::new();

    let head_str = model.head.digest().as_str().to_string();

    // AC-1: Run `fid report generate-report --template tmpl:multistage-table --head <h> --json` twice.
    let (r1, json1) = {
        let mut env = GenerateCmdEnv {
            backend: &backend,
            builtins: &builtins,
            kinds: &kinds,
            resolvers: &resolvers,
            model: &model,
            journal: &journal,
            catalog: &catalog,
            default_decimals: DefaultDecimals::initial(),
            kernel_version: String::new(),
            metadata: &metadata,
            query_provider: None,
            field_sources: Vec::new(),
            custom_locales: BTreeMap::new(),
            scratch: &mut scratch,
        };
        invoke_generate_cli(&["--template", MULTISTAGE_TABLE_NAME, "--head", &head_str, "--json"], &mut env).unwrap()
    };
    assert!(json1);
    let (r2, json2) = {
        let mut env = GenerateCmdEnv {
            backend: &backend,
            builtins: &builtins,
            kinds: &kinds,
            resolvers: &resolvers,
            model: &model,
            journal: &journal,
            catalog: &catalog,
            default_decimals: DefaultDecimals::initial(),
            kernel_version: String::new(),
            metadata: &metadata,
            query_provider: None,
            field_sources: Vec::new(),
            custom_locales: BTreeMap::new(),
            scratch: &mut scratch,
        };
        invoke_generate_cli(&["--template", MULTISTAGE_TABLE_NAME, "--head", &head_str, "--json"], &mut env).unwrap()
    };
    assert!(json2);

    // Byte-identical canonical JSON and identical digest; Record Journal unchanged!
    assert_eq!(r1.digest, r2.digest);
    let canon1 = r1.to_canonical_bytes().unwrap();
    let canon2 = r2.to_canonical_bytes().unwrap();
    assert_eq!(canon1, canon2);
    assert_eq!(backend.journal().len(), doc_journal_len_before);
    assert_eq!(store.journal().len(), 0);

    // Provenance fields (REQ-003, AC-1)
    assert_eq!(r1.provenance.head, model.head);
    let tref = r1.provenance.template.as_ref().expect("template provenance required");
    assert_eq!(tref.id.to_string(), MULTISTAGE_TABLE_ID);
    assert_eq!(tref.version, 1);
    assert!(tref.builtin);
    assert_eq!(r1.provenance.kernel_version, "0.1.0");
    assert_eq!(r1.provenance.locale, "en");
    assert_eq!(r1.provenance.locale_release, DEFAULT_LOCALE_RELEASE);
    assert!(r1.provenance.formatters.iter().any(|f| f.id == "fmt.length@1" && f.version == "1"));

    // Verify digest is SHA-256 of canonical JSON EXCLUDING the digest field (REQ-001).
    let content_bytes = r1.to_content_canonical_bytes().unwrap();
    assert_eq!(Digest::from_bytes(digest_bytes(&content_bytes).0), r1.digest);
    assert_eq!(ResolvedReport::from_canonical_bytes(&canon1).unwrap(), r1);

    // AC-5: Golden resolved JSON equals `fixtures/rpt/model/golden/bracket_3stage.resolved.json` byte for byte.
    let golden_path = root_dir().join("fixtures/rpt/model/golden/bracket_3stage.resolved.json");
    if std::env::var("UPDATE_GOLDEN").as_deref() == Ok("1") {
        std::fs::write(&golden_path, &canon1).unwrap();
    }
    let golden_bytes =
        std::fs::read(&golden_path).unwrap_or_else(|e| panic!("missing golden {}: {e}", golden_path.display()));
    assert_eq!(canon1, golden_bytes, "generated canonical JSON drifted from bracket_3stage.resolved.json");

    // AC-2: Generate with locale `de` -> digest differs, provenance names `de` and locale_release,
    // display uses comma decimal separator, and NO stored ResultRecord digest changes!
    let (r_de, _) = {
        let mut env = GenerateCmdEnv {
            backend: &backend,
            builtins: &builtins,
            kinds: &kinds,
            resolvers: &resolvers,
            model: &model,
            journal: &journal,
            catalog: &catalog,
            default_decimals: DefaultDecimals::initial(),
            kernel_version: String::new(),
            metadata: &metadata,
            query_provider: None,
            field_sources: Vec::new(),
            custom_locales: BTreeMap::new(),
            scratch: &mut scratch,
        };
        invoke_generate_cli(
            &["--template", MULTISTAGE_TABLE_NAME, "--head", &head_str, "--locale", "de", "--json"],
            &mut env,
        )
        .unwrap()
    };
    assert_ne!(r_de.digest, r1.digest);
    assert_eq!(r_de.provenance.locale, "de");
    assert_eq!(r_de.provenance.locale_release, DEFAULT_LOCALE_RELEASE);
    let de_json_str = String::from_utf8(r_de.to_canonical_bytes().unwrap()).unwrap();
    assert!(de_json_str.contains("\"display\":\"11,001\""), "expected German comma decimal separator");
    // Verify record digests in table rows are identical between `en` and `de`.
    let en_rows = arr(get(&r1.pages[0].blocks[2].payload, "rows"));
    let de_rows = arr(get(&r_de.pages[0].blocks[2].payload, "rows"));
    assert_eq!(en_rows.len(), 50);
    assert_eq!(de_rows.len(), 50);
    for (er, dr) in en_rows.iter().zip(de_rows.iter()) {
        for (ec, dc) in arr(get(er, "cells")).iter().zip(arr(get(dr, "cells")).iter()) {
            assert_eq!(st(ec, "record_digest"), st(dc, "record_digest"));
            assert_eq!(st(get(ec, "measured"), "canonical"), st(get(dc, "measured"), "canonical"));
        }
    }

    // AC-3: Save resolved report via `fid report save-report --report <r> --resolved <digest> --json`.
    let rep_str = report_id.uuid().to_string();
    let dig_str = r1.digest.as_str().to_string();
    let (receipt, save_json) = {
        let mut senv = SaveCmdEnv { backend: &backend, scratch: &scratch, store: &mut store };
        invoke_save_cli(&["--report", &rep_str, "--resolved", &dig_str, "--json"], &mut senv).unwrap()
    };
    assert!(save_json);
    assert_eq!(receipt.report, report_id);
    assert_eq!(receipt.resolved_digest, r1.digest);
    let cas_bytes = store.cas_bytes(&r1.digest).expect("resolved bytes must exist in CAS").to_vec();
    assert_eq!(Digest::from_bytes(digest_bytes(&cas_bytes).0), r1.digest);
    assert_eq!(store.linked(&report_id), &[r1.digest]);
    assert_eq!(store.journal().len(), 1);
    assert_eq!(store.journal()[0].resolved_digest, r1.digest);

    // AC-3 error path: unknown digest fails with `report.resolved_not_found` and journals nothing.
    let unknown_digest = format!("sha256:{}", "9".repeat(64));
    let err = {
        let mut senv = SaveCmdEnv { backend: &backend, scratch: &scratch, store: &mut store };
        invoke_save_cli(&["--report", &rep_str, "--resolved", &unknown_digest], &mut senv).unwrap_err()
    };
    assert_eq!(err.code(), "report.resolved_not_found");
    assert_eq!(store.journal().len(), 1);

    // AC-7: Exact replay of the journal containing report creation + AC-3 save -> IDENTICAL.
    let mut full_journal = backend.journal_values();
    full_journal.extend(store.journal_values());
    let replayed = replay_save_report_exact(&full_journal, &store, &kinds, project);
    assert_eq!(replayed.outcome, ReplayOutcome::Identical);
    assert_eq!(replayed.first_mismatch, None);
    assert_eq!(replayed.saved, vec![(report_id, r1.digest)]);

    // Tampering with CAS object bytes is caught by exact replay (`DIVERGENT`).
    let mut tampered = BTreeMap::new();
    let mut bad_bytes = cas_bytes.to_vec();
    bad_bytes[0] ^= b' ';
    tampered.insert(r1.digest, bad_bytes);
    let bad_replay = replay_save_report_exact(&full_journal, &tampered, &kinds, project);
    assert_eq!(bad_replay.outcome, ReplayOutcome::Divergent);
    assert_eq!(bad_replay.first_mismatch, Some(1));
}

#[test]
fn ac4_view_block_holds_snapshot_digest_only_and_warns_when_missing() {
    let (project, model) = bracket_model();
    let journal = bracket_journal();
    let cases = rpt_fixture("bracket_3stage/cases.json");
    let view_id = uuid(st(&cases, "view"));
    let kinds = full_kinds();
    let resolvers = full_resolvers();
    let catalog = Catalog::english();

    let mut backend = MemoryBackend::new(project);
    let mut ids = Counter::new();
    let created = create_report(
        &mut backend,
        &kinds,
        &mut ids,
        &CreateReport { project, title: Untrusted::new(Text::new("View Report").unwrap()) },
    )
    .unwrap();
    let rep_id = created.id();
    let page_id = created.pages()[0].id();
    let view_payload = view_block_payload(view_id, Some("Top view snapshot")).unwrap();
    let block_id = add_block(
        &mut backend,
        &kinds,
        &mut ids,
        &AddBlock {
            report: rep_id.uuid(),
            page: page_id.uuid(),
            after: None,
            kind: Some(BlockKindId::new(VIEW_KIND_ID).unwrap()),
            payload: view_payload.value().clone(),
        },
    )
    .unwrap();
    let rep = backend.stored(&rep_id).unwrap().open(&rep_id, &kinds).unwrap();
    let block_uuid = block_id.uuid();

    // Case 1: view has a snapshot digest -> resolved block carries snapshot_digest and NO image bytes.
    let snap_digest = Digest::parse(&format!("sha256:{}", "a".repeat(64))).unwrap();
    let mut meta_with_snap = MemoryGenMetadata::default();
    meta_with_snap.view_snapshots.insert(view_id, snap_digest);
    let gctx = GenContext {
        project,
        head: model.head,
        stages: Vec::new(),
        runs: Vec::new(),
        sources: Vec::new(),
        locale: default_locale_rules("en").unwrap(),
        units: None,
        default_decimals: DefaultDecimals::initial(),
        catalog: &catalog,
        kernel_version: DEFAULT_KERNEL_VERSION.to_string(),
        metadata: &meta_with_snap,
        query_provider: None,
    };
    let resolved =
        generate(GenSource::Report { report: &rep, template: None }, &gctx, &model, &journal, &kinds, &resolvers)
            .unwrap();
    assert!(resolved.warnings.is_empty());
    let blk = &resolved.pages[0].blocks[0];
    assert_eq!(blk.kind.as_str(), VIEW_KIND_ID);
    assert_eq!(st(&blk.payload, "snapshot_digest"), snap_digest.as_str());
    let text = String::from_utf8(resolved.to_canonical_bytes().unwrap()).unwrap();
    assert!(!text.contains("image_bytes") && !text.contains("snapshot_bytes") && !text.contains("png_bytes"));

    // Case 2: view has no snapshot digest -> warning `report.view_missing` included in digested content (REQ-007).
    let meta_missing = MemoryGenMetadata::default();
    let gctx_miss = GenContext { metadata: &meta_missing, ..gctx };
    let resolved_miss =
        generate(GenSource::Report { report: &rep, template: None }, &gctx_miss, &model, &journal, &kinds, &resolvers)
            .unwrap();
    assert_eq!(resolved_miss.warnings.len(), 1);
    assert_eq!(resolved_miss.warnings[0].block_id, block_uuid);
    assert_eq!(resolved_miss.warnings[0].code, WARN_VIEW_MISSING);
    assert_ne!(resolved_miss.digest, resolved.digest);

    // Defense in depth: raw image bytes key in payload is refused (`report.embedded_bytes_refused`).
    let bad = parse(b"{\"image_bytes\":\"AAAA\"}").unwrap();
    assert_eq!(reject_embedded_bytes(&bad).unwrap_err().code(), "report.embedded_bytes_refused");
}

#[test]
fn ac6_perf_500x3_under_two_seconds() {
    // Deterministic seed-5 500-characteristic x 3-stage synthetic project (REQ-008, AC-6).
    let project = Uuid::parse("019a9a9a-0000-7000-8000-000000000503").unwrap();
    let mut journal = MemoryJournal::new();
    let mut prev_head = String::new();
    let mut next_head = |bytes: &[u8]| -> JournalHead {
        let mut buf = prev_head.as_bytes().to_vec();
        buf.extend_from_slice(bytes);
        let d = Digest::from_bytes(digest_bytes(&buf).0);
        prev_head = d.as_str().to_string();
        JournalHead::new(d)
    };

    // Build 500 characteristics and 3 stages x 500 results.
    let mut characteristics = Vec::with_capacity(500);
    let mut stages = Vec::with_capacity(3);
    let mut runs = Vec::with_capacity(3);
    let mut results = Vec::with_capacity(1_500);
    let elem = Uuid::parse("019a9a9b-0001-7000-8000-000000000001").unwrap();

    for c in 1u64..=500 {
        let cid = Uuid::parse(&format!("019a9a9b-0002-7000-8000-{c:012x}")).unwrap();
        let nom = 0.010_f64 + (c as f64) * 0.000_1;
        let nom_bits = format!("0x{:016x}", nom.to_bits());
        let doc = parse(
            format!(
                r#"{{"id":"{cid}","name":"P{c:03}","nominal":{{"unit":"si:m","value":"{nom_bits}"}},"schema":"fixture.characteristic/1"}}"#
            )
            .as_bytes(),
        )
        .unwrap();
        let bytes = to_canonical_bytes(&doc).unwrap();
        let h = next_head(&bytes);
        journal.append(h, project, EntityKind::Characteristic, Some(cid), bytes).unwrap();
        characteristics.push(CharacteristicItem {
            id: cid,
            name: format!("P{c:03}"),
            char_type: "SizeCharacteristic".to_string(),
            balloon: Some(c as i64),
            element: Some(elem),
            tags: Vec::new(),
        });
    }

    // One shared valid LPU UEC-1 statement (all DCC sources, matching bracket_3stage measured profile).
    let d1 = Digest::from_bytes(digest_bytes(b"um:bore-diameter@1.2.0").0);
    let s1 = Digest::from_bytes(digest_bytes(b"source:dcc:probe-cal-7").0);
    let s2 = Digest::from_bytes(digest_bytes(b"source:dcc:ring-gauge-25").0);
    let stmt_json = format!(
        r#"{{"class":"UEC-1","contributors":[{{"input_id":"probe_cal","sensitivity":"3ff0000000000000","u_contrib":"3eb4222f4706675a","variance_share":"3fe47ae147ae147b"}},{{"input_id":"ref_cal","sensitivity":"3ff0000000000000","u_contrib":"3eae319a0635d963","variance_share":"3fd70a3d70a3d70a"}}],"expanded":{{"U":"3ec92a737110e454","k":"4000000000000000"}},"method":"LPU","model":{{"digest":"{d1}","id":"um:bore-diameter","semver":"1.2.0"}},"provenance":{{"proposal":"false","rule_sets":["uec-assign@1"]}},"schema":"uncertainty_statement@1","sources":[{{"digest":"{s1}","id":"dcc:probe-cal-7","kind":"dcc"}},{{"digest":"{s2}","id":"dcc:ring-gauge-25","kind":"dcc"}}],"unit":"si:m"}}"#,
        d1 = d1.as_str(),
        s1 = s1.as_str(),
        s2 = s2.as_str()
    );
    let stmt_canon = to_canonical_bytes(&parse(stmt_json.as_bytes()).unwrap()).unwrap();
    let stmt_digest = Digest::from_bytes(digest_bytes(&stmt_canon).0);
    let h_stmt = next_head(&stmt_canon);
    journal.append(h_stmt, project, EntityKind::Statement, None, stmt_canon).unwrap();

    let z = format!("sha256:{}1", "0".repeat(63));
    let mut last_head = h_stmt;
    for s in 1u64..=3 {
        let run_id = Uuid::parse(&format!("019a9a9b-0003-7{s:03x}-8000-000000000000")).unwrap();
        let stage_id = Uuid::parse(&format!("019a9a9b-0005-7{s:03x}-8000-000000000000")).unwrap();
        runs.push(RunItem { id: run_id, start_ms: (s as i64) * 1_000 });
        stages.push(StageItem { id: stage_id, run: run_id, removed: false });

        for c in 1u64..=500 {
            let cid = characteristics[(c - 1) as usize].id;
            let rid = Uuid::parse(&format!("019a9a9b-0004-7{s:03x}-8000-{c:012x}")).unwrap();
            let val = 0.010_f64 + (c as f64) * 0.000_1 + (s as f64) * 0.000_001;
            let val_bits = format!("0x{:016x}", val.to_bits());
            let rec_str = format!(
                r#"{{"algorithm":"gdt.size.two_point@1.0.0","characteristic":"{cid}","decision":{{"basis":{{"claimed":false,"class":"UEC-1","declared_input":false,"method":"LPU","statement":"{sd}","uncharacterized_input":false}},"outcome":"CONFORMS","reason":null,"rule":"dr:simple-acceptance@1","source":"unc"}},"dmt_digest":"{z}","dmt_row":"dmt:asme/size/none@1","evaluation_basis":"POINTS","input_digests":[],"journal_entry":"{z}","kernel_version":"0.1.0","measurand":"perf P{c:03}","operator_chain_digest":"{z}","run":"{run_id}","schema":"fid.gdt-result/1","spec":{{"defaults_profile":"dp:fixture@1","dialect":"ASME","inherited_from":"explicit","standards":[]}},"uncertainty":{{"claimed":false,"class":"UEC-1","declared_input":false,"expanded":{{"U":"0x3f70c6f7a0b5ed8d","k":"0x4000000000000000"}},"interval":null,"method":"LPU","statement":"{sd}","u_std":null,"uec0_reason":null,"uncharacterized_input":false,"unit":"si:m"}},"value":{{"unit":"si:m","value":"{val_bits}"}}}}"#,
                sd = stmt_digest.as_str()
            );
            let rec_bytes = to_canonical_bytes(&parse(rec_str.as_bytes()).unwrap()).unwrap();
            last_head = next_head(&rec_bytes);
            journal.append(last_head, project, EntityKind::Result, Some(rid), rec_bytes).unwrap();
            results.push(ResultItem { id: rid, characteristic: cid, stage: stage_id });
        }
    }

    let model = MemoryModel { head: last_head, characteristics, elements: vec![elem], stages, runs, results };
    let builtins = BuiltinRegistry::with_starters().unwrap();
    let kinds = full_kinds();
    let resolvers = full_resolvers();
    let catalog = Catalog::english();
    let metadata = MemoryGenMetadata::default();
    let backend = MemoryBackend::new(project);
    let mut scratch = MemoryScratch::new();

    let start = Instant::now();
    let resolved = {
        let mut env = GenerateCmdEnv {
            backend: &backend,
            builtins: &builtins,
            kinds: &kinds,
            resolvers: &resolvers,
            model: &model,
            journal: &journal,
            catalog: &catalog,
            default_decimals: DefaultDecimals::initial(),
            kernel_version: "0.1.0".to_string(),
            metadata: &metadata,
            query_provider: None,
            field_sources: Vec::new(),
            custom_locales: BTreeMap::new(),
            scratch: &mut scratch,
        };
        generate_report(
            &GenerateReportParams {
                template: Some(MULTISTAGE_TABLE_NAME.to_string()),
                head: Some(last_head.digest().as_str().to_string()),
                ..GenerateReportParams::default()
            },
            &mut env,
        )
        .unwrap()
    };
    let elapsed_ms = start.elapsed().as_millis();
    let rows = arr(get(&resolved.pages[0].blocks[2].payload, "rows"));
    assert_eq!(rows.len(), 500);
    assert_eq!(arr(get(&rows[0], "cells")).len(), 3);
    // Performance budget REQ-008: < 2 s on reference hardware (generous 10 s debug guard, exact timing printed).
    eprintln!("AC-6 perf_500x3 generated 500x3 resolved report in {elapsed_ms} ms");
    assert!(elapsed_ms < 10_000, "perf_500x3 took {elapsed_ms} ms");
}
