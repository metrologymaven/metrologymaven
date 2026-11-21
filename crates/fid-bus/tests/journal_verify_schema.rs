//! The result and parameter schemas of `journal.verify_chain@1` and `journal.list_entries@1`
//! (spec node CORE.JRN.CHAIN.VERIFY, crate `fid-journal`) checked with the bus's validator
//! (`fid_bus::validate::Compiled`), as the bus will check them: real `VerifyReport::to_value`
//! results (ok with each anchor; every pinned failure reason with a sequence and with
//! `seq: null`; tampered checkpoint row ids), real `ListPage::to_value` pages with and without
//! `next`, and valid and invalid parameters. Lives here because `fid-journal` (layer model)
//! may not depend on `fid-bus` (engine), even for tests. Expectations are the spec's rules.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::{BusError, Detail, ValidationReason};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::{parse, to_canonical_bytes, CValue, Digest};
use fid_journal::verify::{
    list_rows, plan_list, verify_rows, Anchor, BlobSource, BlobState, Checked, CheckpointRow, Failure, ListPage,
    ListQuery, ReadError, Start, StoredEntry, VerifyOptions, VerifyReport, LIST_ENTRIES, REASONS, VERIFY_CHAIN,
};
use fid_journal::{
    build, command_defs, AiAssistance, CommandDraft, DraftBody, EntryDraft, EntryTime, Manifests, OsSalts,
    PrincipalDraft, PrincipalKind, GENESIS_PREV,
};
use fid_types::{Determinism, SideEffect};

const STREAM: &str = "record";

struct NoHooks;

impl Hooks for NoHooks {
    fn literal(&self, _: &str, _: &CValue) -> Result<Normalised, BusError> {
        Err(BusError::Validation(ValidationReason::UnitMismatch, Detail::new("no quantities here")))
    }
    fn entity(&self, _: &str) -> RefLookup {
        RefLookup::Missing
    }
}

fn def_of(id: &str) -> fid_registry::CommandDef {
    command_defs().unwrap().into_iter().find(|d| d.id == id).unwrap()
}

fn compiled(schema: &CValue) -> Compiled {
    Compiled::compile(schema).unwrap()
}

fn assert_valid(schema: &Compiled, v: &CValue, what: &str) {
    if let Err(f) = schema.evaluate(v, &NoHooks) {
        panic!("{what}: {} at {} ({})", f.keyword, f.pointer, f.message);
    }
}

fn assert_invalid(schema: &Compiled, v: &CValue, what: &str) {
    assert!(schema.evaluate(v, &NoHooks).is_err(), "{what} should be refused");
}

fn draft(seq: u64) -> EntryDraft {
    let m = Digest([7; 32]);
    EntryDraft {
        stream: STREAM.into(),
        branch: "main".into(),
        logical_parent: None,
        time: EntryTime { utc: "2026-11-20T10:00:00Z".into(), mono_ns: seq, sync: "synced".into() },
        principal: PrincipalDraft {
            kind: PrincipalKind::Script,
            identity: CValue::Str("ci".into()),
            on_behalf_of: None,
        },
        ai_assistance: AiAssistance::None,
        manifests: Manifests { registry: m, kernel: m },
        reason: None,
        human_assertion: None,
        body: DraftBody::Command(CommandDraft {
            id: "project.set_tags".into(),
            major: 1,
            class: SideEffect::W2,
            determinism: Determinism::D0,
            seed: seq,
            args: CValue::Arr(Vec::new()),
            events: Vec::new(),
            inputs: Vec::new(),
            consumed_r1: Vec::new(),
            outputs: Vec::new(),
            algorithms: Vec::new(),
        }),
    }
}

/// `n` valid chained rows, each with its payload in `payloads`.
fn chain(n: u64) -> (Vec<StoredEntry>, Vec<(Digest, Vec<u8>)>) {
    let mut rows = Vec::new();
    let mut payloads = Vec::new();
    let mut prev = GENESIS_PREV;
    for seq in 1..=n {
        let e = build(&draft(seq), seq, prev, &mut OsSalts).unwrap();
        let header = to_canonical_bytes(&e.header.to_value()).unwrap();
        let payload = to_canonical_bytes(&e.payload.to_value()).unwrap();
        let pd = fid_canon::digest_bytes(&payload);
        payloads.push((pd, payload));
        rows.push(StoredEntry {
            seq: seq as i64,
            hash: e.header.hash.to_string(),
            stream: STREAM.into(),
            branch: "main".into(),
            kind: "command".into(),
            prev_hash: Some(prev.to_string()),
            header_len: header.len() as u64,
            header,
            refs: vec![("payload".into(), pd.to_string())],
        });
        prev = e.header.hash;
    }
    (rows, payloads)
}

struct Blobs(Vec<(Digest, Vec<u8>)>);

impl BlobSource for Blobs {
    fn check(&mut self, d: &Digest) -> Result<BlobState, ReadError> {
        Ok(if self.0.iter().any(|(k, _)| k == d) { BlobState::Sound } else { BlobState::Missing })
    }
    fn payload(&mut self, d: &Digest) -> Result<Option<Vec<u8>>, ReadError> {
        Ok(self.0.iter().find(|(k, _)| k == d).map(|(_, b)| b.clone()))
    }
}

fn report_with(failure: Option<Failure>, anchor: Anchor, from_seq: u64) -> VerifyReport {
    VerifyReport {
        stream: STREAM.into(),
        from_seq,
        anchor,
        failure,
        checked: Checked { entries: 3, blobs: 1, checkpoints: 0, without_payload: 2 },
    }
}

#[test]
fn verify_results_validate_against_the_published_schema() {
    let result = compiled(&def_of(VERIFY_CHAIN).result_schema);

    // Real walks: a clean chain, and tampered checkpoint row ids (0, negative).
    let (rows, payloads) = chain(5);
    let mut blobs = Blobs(payloads);
    let ok = verify_rows(
        STREAM,
        Start::GENESIS,
        &VerifyOptions::default(),
        Vec::new(),
        rows.iter().cloned().map(Ok),
        &mut blobs,
    )
    .unwrap();
    assert!(ok.ok());
    assert_valid(&result, &ok.to_value(), "real ok result");
    for id in [0, -5, i64::MIN] {
        let cp = CheckpointRow { id, stream: STREAM.into(), entry_hash: rows[2].hash.clone(), body: b"{}".to_vec() };
        let got = verify_rows(
            STREAM,
            Start::GENESIS,
            &VerifyOptions::default(),
            vec![Ok(cp)],
            rows.iter().cloned().map(Ok),
            &mut blobs,
        )
        .unwrap();
        let f = got.failure.as_ref().unwrap();
        assert_eq!((f.reason, f.seq, f.checkpoint), ("bad_id", None, Some(id)));
        assert_valid(&result, &got.to_value(), &format!("checkpoint id {id}"));
    }
    let mut tampered = rows.clone();
    tampered[3].header[10] ^= 1;
    let got = verify_rows(
        STREAM,
        Start::GENESIS,
        &VerifyOptions::default(),
        Vec::new(),
        tampered.into_iter().map(Ok),
        &mut blobs,
    )
    .unwrap();
    assert!(!got.ok());
    assert_valid(&result, &got.to_value(), "real failure");

    // Both anchors, and every pinned reason with and without a sequence and with any row id.
    for (anchor, from) in [(Anchor::Genesis, 1), (Anchor::Trusted { seq: 300 }, 301)] {
        assert_valid(&result, &report_with(None, anchor, from).to_value(), "ok");
    }
    for (kind, reason) in REASONS {
        for seq in [Some(1), Some(u64::MAX), None] {
            for checkpoint in [None, Some(1), Some(0), Some(-1), Some(i64::MIN), Some(i64::MAX)] {
                let f = Failure { stream: STREAM.into(), seq, kind, reason, checkpoint, detail: "d".into() };
                let v = report_with(Some(f), Anchor::Genesis, 1).to_value();
                assert_valid(&result, &v, &format!("{kind}/{reason} seq {seq:?} checkpoint {checkpoint:?}"));
            }
        }
    }

    // The schema is not vacuous.
    let edit = |k: &str, v: Option<CValue>| {
        let mut r = report_with(None, Anchor::Genesis, 1).to_value();
        if let CValue::Obj(o) = &mut r {
            match v {
                Some(v) => o.insert(k.into(), v),
                None => o.remove(k),
            };
        }
        r
    };
    assert_invalid(&result, &edit("anchor", None), "missing anchor");
    assert_invalid(&result, &edit("from_seq", Some(CValue::Str("0".into()))), "from_seq 0");
    assert_invalid(&result, &edit("extra", Some(CValue::Bool(true))), "unknown field");
}

#[test]
fn list_pages_and_parameters_validate_against_the_published_schemas() {
    let result = compiled(&def_of(LIST_ENTRIES).result_schema);
    let (rows, _) = chain(1100);
    let plan = plan_list(STREAM, 1100, &ListQuery::default()).unwrap();
    let first = list_rows(&plan, rows.iter().cloned().map(Ok)).unwrap();
    assert_eq!((first.headers.len(), first.next.is_some()), (1000, true));
    assert_valid(&result, &first.to_value(), "page with next");
    let q = ListQuery { page_token: first.next.clone(), ..ListQuery::default() };
    let plan2 = plan_list(STREAM, 1100, &q).unwrap();
    let last = list_rows(&plan2, rows.iter().filter(|r| r.seq as u64 >= plan2.start).cloned().map(Ok)).unwrap();
    assert_eq!((last.headers.len(), last.next.is_none()), (100, true));
    assert_valid(&result, &last.to_value(), "page without next");
    assert_valid(&result, &ListPage { headers: Vec::new(), next: None }.to_value(), "empty page");

    let vp = compiled(&def_of(VERIFY_CHAIN).params_schema);
    for ok in [r#"{}"#, r#"{"from_seq":"251"}"#, r#"{"stream":"record","from_seq":"1"}"#] {
        assert_valid(&vp, &parse(ok.as_bytes()).unwrap(), ok);
    }
    for bad in [r#"{"from_seq":"0"}"#, r#"{"from_seq":1}"#, r#"{"stream":"Record"}"#, r#"{"x":1}"#] {
        assert_invalid(&vp, &parse(bad.as_bytes()).unwrap(), bad);
    }
    let lp = compiled(&def_of(LIST_ENTRIES).params_schema);
    let token = first.next.unwrap();
    let full = format!(
        r#"{{"branch":"main","command":"project.set_tags@1","from_seq":"1","to_seq":"9","principal_kind":"agent","page_token":"{token}"}}"#
    );
    for ok in [r#"{}"#.to_string(), full] {
        assert_valid(&lp, &parse(ok.as_bytes()).unwrap(), &ok);
    }
    for bad in
        [r#"{"principal_kind":"robot"}"#, r#"{"command":"x"}"#, r#"{"page_token":"jl1.zz.1"}"#, r#"{"to_seq":"0"}"#]
    {
        assert_invalid(&lp, &parse(bad.as_bytes()).unwrap(), bad);
    }
}
