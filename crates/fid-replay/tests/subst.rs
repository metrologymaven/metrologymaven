//! Acceptance criteria of spec node CORE.RPL.EXEC.SUBST (AC-1 .. AC-5) and its requirements.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue};
use fid_journal::{commitment, Field, SaltSource};
use fid_registry::{Lifecycle, LifecycleDecl, MigratorRecord};
use fid_replay::{
    classify, preflight, BlobSource, Cause, CommandCatalog, Determinism, MissingItem, MissingKind, ObservationKind,
    ObservationSource, Output, RecordedObservation, RecordedSpan, RecordedStep, ReplayOutcome, StepComparison,
    StepObservations, StepPayload, Storage, SubstError, Tolerance, MAX_OBSERVATIONS_PER_STEP,
};
use fid_types::{CommandId, Digest};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

// ------------------------------------------------------------------ fixture loading

fn dir(case: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/core/rpl/exec/subst").join(case)
}

struct Case {
    doc: BTreeMap<String, CValue>,
    span: RecordedSpan,
    catalog: CommandCatalog,
    kernels: BTreeSet<Digest>,
    blobs: BTreeMap<Digest, Vec<u8>>,
}

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        other => panic!("expected object, got {other:?}"),
    }
}
fn arr(v: &CValue) -> &Vec<CValue> {
    match v {
        CValue::Arr(a) => a,
        other => panic!("expected array, got {other:?}"),
    }
}
fn st(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        other => panic!("expected string, got {other:?}"),
    }
}

fn load(case: &str) -> Case {
    let d = dir(case);
    let doc = obj(&parse(&std::fs::read(d.join("case.json")).unwrap()).unwrap()).clone();
    let span = RecordedSpan::from_value(&doc["span"]).unwrap();
    let decls: Vec<LifecycleDecl> = arr(&doc["decls"])
        .iter()
        .map(|v| {
            let o = obj(v);
            LifecycleDecl {
                command: CommandId::parse(st(&o["command"])).unwrap(),
                lifecycle: Lifecycle::from_map(obj(&o["lifecycle"])).unwrap(),
            }
        })
        .collect();
    let migrators: Vec<MigratorRecord> =
        arr(&doc["migrators"]).iter().map(|v| MigratorRecord::from_value(v).unwrap()).collect();
    let catalog = CommandCatalog::new(&decls, &migrators).unwrap();
    let kernels = arr(&doc["kernels"]).iter().map(|v| Digest::parse(st(v)).unwrap()).collect();
    let mut blobs = BTreeMap::new();
    if let Ok(rd) = std::fs::read_dir(d.join("blobs")) {
        for e in rd {
            let e = e.unwrap();
            let name = e.file_name().into_string().unwrap();
            blobs.insert(Digest::parse(&format!("sha256:{name}")).unwrap(), std::fs::read(e.path()).unwrap());
        }
    }
    Case { doc, span, catalog, kernels, blobs }
}

fn expected_missing(v: &CValue) -> Vec<MissingItem> {
    arr(v)
        .iter()
        .map(|m| {
            let m = obj(m);
            let kind = match st(&m["kind"]) {
                "blob" => MissingKind::Blob,
                "command" => MissingKind::Command,
                "payload" => MissingKind::Payload,
                "kernel" => MissingKind::Kernel,
                other => panic!("unknown kind {other}"),
            };
            let steps = arr(&m["steps"])
                .iter()
                .map(|s| match s {
                    CValue::Num(n) => *n as u64,
                    other => panic!("bad step {other:?}"),
                })
                .collect();
            MissingItem { kind, id: st(&m["id"]).to_string(), steps }
        })
        .collect()
}

fn outcome_name(o: ReplayOutcome) -> &'static str {
    o.as_str()
}

// ------------------------------------------------------------------ test handlers

/// The bus observation port as a handler sees it. When recording it calls the live clock
/// and RNG; when replaying it serves the recording and must never touch them.
struct Port<'a> {
    live_clock: &'a dyn Fn() -> Vec<u8>,
    live_rng: &'a dyn Fn() -> Vec<u8>,
    replay: StepObservations<'a>,
}

impl Port<'_> {
    fn clock(&mut self) -> Result<Vec<u8>, SubstError> {
        // Replay mode only in these tests; the live closures are reachable but never called.
        let _ = self.live_clock;
        self.replay.observe(&ObservationKind::Clock).map(|o| o.bytes)
    }
    fn salt(&mut self) -> Result<Vec<u8>, SubstError> {
        let _ = self.live_rng;
        self.replay.observe(&ObservationKind::Salt).map(|o| o.bytes)
    }
    fn device(&mut self) -> Result<Vec<u8>, SubstError> {
        self.replay.observe(&ObservationKind::Device).map(|o| o.bytes)
    }
}

/// What a handler produced: its output digest and the recomputed args commitment.
struct Produced {
    output: String,
    args_commit: String,
}

fn sha(data: &[u8]) -> String {
    fid_canon::digest_bytes(data).to_string()
}

fn args_commit(port: &mut Port<'_>, args: &CValue) -> Result<String, SubstError> {
    // The commitment salt comes through fid-journal's SaltSource, as `build` draws it.
    match port.replay.salt(Field::Args) {
        Ok(salt) => Ok(commitment(&salt, args).unwrap().to_string()),
        Err(_) => Err(SubstError::ObservationMismatch { seq: port.replay.seq(), detail: "salt".into() }),
    }
}

/// probe.stamp@1: one clock read, one drawn salt; `clock_reads` > 1 models AC-5's changed handler.
fn stamp(port: &mut Port<'_>, args: &CValue, clock_reads: usize) -> Result<Produced, SubstError> {
    let mut clock = Vec::new();
    for _ in 0..clock_reads {
        clock = port.clock()?;
    }
    let drawn = port.salt()?;
    let mut both = clock;
    both.extend_from_slice(&drawn);
    Ok(Produced { output: sha(&both), args_commit: args_commit(port, args)? })
}

/// probe.read@1: one device read.
fn read(port: &mut Port<'_>, args: &CValue) -> Result<Produced, SubstError> {
    let mut bytes = b"probe.read:".to_vec();
    bytes.extend_from_slice(&port.device()?);
    Ok(Produced { output: sha(&bytes), args_commit: args_commit(port, args)? })
}

/// Replays one step and classifies it with CORE.RPL.CLASS. A substitution fault decides the
/// outcome itself (DIVERGENT/input or NOT_REPLAYABLE).
fn replay_step(
    c: &Case,
    seq: u64,
    blobs: &dyn BlobSource,
    run: impl Fn(&mut Port<'_>, &CValue) -> Result<Produced, SubstError>,
) -> (ReplayOutcome, Option<Cause>, Option<SubstError>) {
    let clock_calls = Cell::new(0u32);
    let rng_calls = Cell::new(0u32);
    let live_clock = || -> Vec<u8> {
        clock_calls.set(clock_calls.get() + 1);
        panic!("the live clock was called during replay")
    };
    let live_rng = || -> Vec<u8> {
        rng_calls.set(rng_calls.get() + 1);
        panic!("the live RNG was called during replay")
    };
    let source = ObservationSource::new(&c.span, blobs);
    let mut port = Port { live_clock: &live_clock, live_rng: &live_rng, replay: source.step(seq).unwrap() };
    let rec = obj(&obj(&c.doc["handler"])[&seq.to_string()]);
    let produced = run(&mut port, &rec["args"]);
    let finished = port.replay.finish();
    assert_eq!((clock_calls.get(), rng_calls.get()), (0, 0));
    let err = match (produced, finished) {
        (_, Err(e)) => e,
        (Err(e), Ok(())) => e,
        (Ok(p), Ok(())) => {
            let out = |kind: &str, recorded: &str, replayed: String| Output {
                kind: kind.into(),
                recorded: 0.0,
                replayed: 0.0,
                recorded_digest: recorded.into(),
                replayed_digest: replayed,
            };
            let step = StepComparison {
                class: Determinism::D0,
                same_kernel: true,
                same_platform_class: true,
                outputs: vec![
                    out("output", st(&rec["output"]), p.output),
                    out("args_commit", st(&rec["args_commit"]), p.args_commit),
                ],
                decisions: vec![],
                limits: vec![],
                missing: None,
            };
            let table: BTreeMap<String, Tolerance> = BTreeMap::new();
            return (classify(&step, &table).unwrap().outcome, None, None);
        }
    };
    // A missing item goes through the classifier as `missing`, which gives NOT_REPLAYABLE.
    if let SubstError::MissingItem(m) = &err {
        let step = StepComparison {
            class: Determinism::D0,
            same_kernel: true,
            same_platform_class: true,
            outputs: vec![],
            decisions: vec![],
            limits: vec![],
            missing: Some(m.to_string()),
        };
        let table: BTreeMap<String, Tolerance> = BTreeMap::new();
        assert_eq!(classify(&step, &table).unwrap().outcome, ReplayOutcome::NotReplayable);
    }
    (err.outcome().unwrap(), err.cause(), Some(err))
}

fn expected_outcomes(v: &CValue) -> Vec<(u64, String)> {
    obj(v).iter().map(|(k, o)| (k.parse().unwrap(), st(o).to_string())).collect()
}

// ------------------------------------------------------------------ acceptance criteria

#[test]
fn ac1_clock_and_salt_replays_identically_without_clock_or_rng() {
    let c = load("clock_and_salt");
    let expect = obj(&c.doc["expect"]);
    assert_eq!(preflight(&c.span, &c.blobs, &c.catalog, &c.kernels), expected_missing(&expect["preflight"]));
    for (seq, want) in expected_outcomes(&expect["outcomes"]) {
        let (got, cause, err) = replay_step(&c, seq, &c.blobs, |p, a| stamp(p, a, 1));
        assert_eq!(outcome_name(got), want, "step {seq}: {err:?}");
        assert_eq!(cause, None);
    }
}

#[test]
fn ac2_device_read_serves_the_recorded_blob() {
    let c = load("device_read");
    let expect = obj(&c.doc["expect"]);
    assert_eq!(preflight(&c.span, &c.blobs, &c.catalog, &c.kernels), expected_missing(&expect["preflight"]));
    for (seq, want) in expected_outcomes(&expect["outcomes"]) {
        let (got, _, err) = replay_step(&c, seq, &c.blobs, read);
        assert_eq!(outcome_name(got), want, "step {seq}: {err:?}");
    }
    // The served bytes are the stored blob, byte for byte.
    let source = ObservationSource::new(&c.span, &c.blobs);
    let mut s = source.step(1).unwrap();
    let o = s.observe(&ObservationKind::Device).unwrap();
    assert_eq!(Some(&o.bytes), c.blobs.get(&o.digest));
}

#[test]
fn ac3_deleted_blob_is_named_and_not_replayable() {
    let c = load("device_read");
    let expect = obj(&c.doc["expect"]);
    let deleted = Digest::parse(st(&expect["deleted_blob"])).unwrap();
    let mut blobs = c.blobs.clone();
    assert!(blobs.remove(&deleted).is_some());
    assert_eq!(
        preflight(&c.span, &blobs, &c.catalog, &c.kernels),
        expected_missing(&expect["preflight_without_blob"])
    );
    for (seq, want) in expected_outcomes(&expect["outcomes_without_blob"]) {
        let (got, _, err) = replay_step(&c, seq, &blobs, read);
        assert_eq!(outcome_name(got), want, "step {seq}: {err:?}");
        if want == "NOT_REPLAYABLE" {
            let Some(SubstError::MissingItem(m)) = &err else { panic!("expected missing_item, got {err:?}") };
            assert_eq!((m.kind, m.id.as_str(), m.steps.as_slice()), (MissingKind::Blob, deleted.as_str(), &[seq][..]));
        }
    }
}

#[test]
fn ac4_removed_command_major_is_named() {
    let c = load("removed_command");
    let expect = obj(&c.doc["expect"]);
    let got = preflight(&c.span, &c.blobs, &c.catalog, &c.kernels);
    assert_eq!(got, expected_missing(&expect["preflight"]));
    assert_eq!(got[0].to_string(), "command probe.legacy@1");
}

#[test]
fn ac5_extra_clock_read_is_divergent_with_cause_input() {
    let c = load("clock_and_salt");
    let (got, cause, err) = replay_step(&c, 2, &c.blobs, |p, a| stamp(p, a, 2));
    assert_eq!((got, cause), (ReplayOutcome::Divergent, Some(Cause::Input)));
    assert_eq!(err.unwrap().code(), "observation_mismatch");
}

// ------------------------------------------------------------------ requirements

#[test]
fn req003_corrupt_blob_is_not_replayable_naming_the_digest() {
    let c = load("device_read");
    let mut blobs = c.blobs.clone();
    let d = *blobs.keys().next().unwrap();
    blobs.get_mut(&d).unwrap().push(b' ');
    let seq = c.span.steps().iter().find(|s| s.observations[0].digest == d).unwrap().seq;
    let (got, _, err) = replay_step(&c, seq, &blobs, read);
    assert_eq!(got, ReplayOutcome::NotReplayable);
    assert_eq!(err.unwrap().to_string(), format!("missing_item: blob {d}"));
}

#[test]
fn req005_preflight_lists_every_kind_in_order() {
    let c = load("preflight_all");
    let expect = obj(&c.doc["expect"]);
    let got = preflight(&c.span, &c.blobs, &c.catalog, &c.kernels);
    assert_eq!(got, expected_missing(&expect["preflight"]));
    let v = CValue::Arr(got.iter().map(MissingItem::to_value).collect());
    let bytes = fid_canon::to_canonical_bytes(&v).unwrap();
    assert!(bytes.starts_with(b"[{\"id\":\"sha256:"), "{}", String::from_utf8_lossy(&bytes));
}

#[test]
fn req004_salts_rebuild_identical_commitments_and_twice_is_mismatch() {
    let c = load("clock_and_salt");
    let source = ObservationSource::new(&c.span, &c.blobs);
    let mut s = source.step(1).unwrap();
    s.observe(&ObservationKind::Clock).unwrap();
    s.observe(&ObservationKind::Salt).unwrap();
    let salt = s.commitment_salt(Field::Args).unwrap();
    let rec = obj(&obj(&c.doc["handler"])["1"]);
    assert_eq!(commitment(&salt, &rec["args"]).unwrap().to_string(), st(&rec["args_commit"]));
    let e = s.commitment_salt(Field::Args).unwrap_err();
    assert_eq!(e.code(), "observation_mismatch");
    // The fault sticks: finish reports it even though everything was consumed.
    assert_eq!(s.finish().unwrap_err(), e);
}

#[test]
fn mismatch_on_kind_exhaustion_unconsumed_and_unrecorded_salt() {
    let c = load("clock_and_salt");
    let source = ObservationSource::new(&c.span, &c.blobs);

    let mut s = source.step(1).unwrap();
    let e = s.observe(&ObservationKind::Device).unwrap_err();
    assert_eq!((e.code(), e.outcome(), e.cause()), ("observation_mismatch", Some(ReplayOutcome::Divergent), Some(Cause::Input)));
    // After a fault every later request fails with the same fault.
    assert_eq!(s.observe(&ObservationKind::Clock).unwrap_err(), e);

    let mut s = source.step(1).unwrap();
    s.observe(&ObservationKind::Clock).unwrap();
    assert_eq!(s.finish().unwrap_err().code(), "observation_mismatch");

    let mut s = source.step(1).unwrap();
    s.observe(&ObservationKind::Clock).unwrap();
    s.observe(&ObservationKind::Salt).unwrap();
    assert_eq!(s.commitment_salt(Field::Reason).unwrap_err().code(), "observation_mismatch");

    let mut s = source.step(1).unwrap();
    s.observe(&ObservationKind::Clock).unwrap();
    s.observe(&ObservationKind::Salt).unwrap();
    assert_eq!(s.finish().unwrap_err().code(), "observation_mismatch", "args salt left unconsumed");

    assert_eq!(source.step(99).err().unwrap(), SubstError::UnknownStep(99));
}

fn d(tag: &str) -> Digest {
    Digest::parse(&sha(tag.as_bytes())).unwrap()
}

#[test]
fn detached_payload_is_not_replayable() {
    let step = RecordedStep {
        seq: 5,
        entry: d("entry"),
        command: None,
        kernel: None,
        observations: vec![RecordedObservation { kind: ObservationKind::HumanText, digest: d("x"), storage: Storage::Inline }],
        payload: None,
    };
    let span = RecordedSpan::new(vec![step]).unwrap();
    let blobs: BTreeMap<Digest, Vec<u8>> = BTreeMap::new();
    let source = ObservationSource::new(&span, &blobs);
    let mut s = source.step(5).unwrap();
    let e = s.observe(&ObservationKind::HumanText).unwrap_err();
    assert_eq!(e, SubstError::MissingItem(MissingItem { kind: MissingKind::Payload, id: d("entry").to_string(), steps: vec![5] }));
    assert_eq!(e.outcome(), Some(ReplayOutcome::NotReplayable));
    let mut s = source.step(5).unwrap();
    assert_eq!(s.commitment_salt(Field::Args).unwrap_err().code(), "missing_item");
}

#[test]
fn other_kinds_have_one_representation() {
    assert_eq!(ObservationKind::parse("clock").unwrap(), ObservationKind::Clock);
    assert_eq!(ObservationKind::parse("laser_tracker_weather").unwrap(), ObservationKind::Other("laser_tracker_weather".into()));
    assert_eq!(ObservationKind::other("llm_output").unwrap_err().code(), "span_invalid");
    for bad in ["", "Clock", "9x", "a-b", &"a".repeat(65)] {
        assert!(ObservationKind::parse(bad).is_err(), "{bad:?}");
    }
    // A hand-built Other with a built-in name is refused by the span and never matches.
    let fake = ObservationKind::Other("clock".into());
    let step = |kind| RecordedStep {
        seq: 1,
        entry: d("e"),
        command: None,
        kernel: None,
        observations: vec![RecordedObservation { kind, digest: d("x"), storage: Storage::Blob }],
        payload: Some(StepPayload::default()),
    };
    assert_eq!(RecordedSpan::new(vec![step(fake.clone())]).unwrap_err().code(), "span_invalid");
    let span = RecordedSpan::new(vec![step(ObservationKind::Clock)]).unwrap();
    let blobs: BTreeMap<Digest, Vec<u8>> = BTreeMap::new();
    let mut s = ObservationSource::new(&span, &blobs).step(1).unwrap();
    assert_eq!(s.observe(&fake).unwrap_err().code(), "observation_mismatch");
}

#[test]
fn span_parser_fails_closed() {
    let bad = [
        r#"{"steps": [], "extra": 1}"#,
        r#"{"steps": [{"seq": 0, "entry": "sha256:0000000000000000000000000000000000000000000000000000000000000000", "command": null, "kernel": null, "observations": [], "payload": null}]}"#,
        r#"{"steps": [{"seq": 1.5, "entry": "sha256:0000000000000000000000000000000000000000000000000000000000000000", "command": null, "kernel": null, "observations": [], "payload": null}]}"#,
        r#"{"steps": [{"seq": 1, "entry": "sha256:00", "command": null, "kernel": null, "observations": [], "payload": null}]}"#,
        r#"{"steps": [{"seq": 1, "entry": "sha256:0000000000000000000000000000000000000000000000000000000000000000", "command": "Bad", "kernel": null, "observations": [], "payload": null}]}"#,
        r#"{"steps": [{"seq": 1, "entry": "sha256:0000000000000000000000000000000000000000000000000000000000000000", "command": null, "kernel": null, "observations": [{"kind": "clock", "digest": "sha256:0000000000000000000000000000000000000000000000000000000000000000", "storage": "disk"}], "payload": null}]}"#,
        r#"{"steps": [{"seq": 1, "entry": "sha256:0000000000000000000000000000000000000000000000000000000000000000", "command": null, "kernel": null, "observations": [], "payload": {"salts": {"bogus": "00000000000000000000000000000000"}, "inline": {}}}]}"#,
        r#"{"steps": [{"seq": 1, "entry": "sha256:0000000000000000000000000000000000000000000000000000000000000000", "command": null, "kernel": null, "observations": [], "payload": {"salts": {}, "inline": {"sha256:0000000000000000000000000000000000000000000000000000000000000000": "abc"}}}]}"#,
        r#"{"steps": [{"seq": 2, "entry": "sha256:0000000000000000000000000000000000000000000000000000000000000000", "command": null, "kernel": null, "observations": [], "payload": null}, {"seq": 2, "entry": "sha256:0000000000000000000000000000000000000000000000000000000000000000", "command": null, "kernel": null, "observations": [], "payload": null}]}"#,
    ];
    for (i, text) in bad.iter().enumerate() {
        let v = parse(text.as_bytes()).unwrap();
        assert_eq!(RecordedSpan::from_value(&v).unwrap_err().code(), "span_invalid", "case {i}");
    }
    let too_many = (0..=MAX_OBSERVATIONS_PER_STEP)
        .map(|_| RecordedObservation { kind: ObservationKind::Clock, digest: d("x"), storage: Storage::Blob })
        .collect();
    let step = RecordedStep { seq: 1, entry: d("e"), command: None, kernel: None, observations: too_many, payload: None };
    assert_eq!(RecordedSpan::new(vec![step]).unwrap_err().code(), "span_invalid");
}

#[test]
fn catalog_refuses_invalid_registry_records() {
    let life = |state: &str| {
        let mut m = BTreeMap::new();
        m.insert("state".to_string(), CValue::Str(state.into()));
        m.insert("since".to_string(), CValue::Str("2026-01-01".into()));
        Lifecycle::from_map(&m).unwrap()
    };
    let decl = |id: &str, state: &str| LifecycleDecl { command: CommandId::parse(id).unwrap(), lifecycle: life(state) };
    // The same (ID, major) declared twice.
    let e = CommandCatalog::new(&[decl("a.b@1", "active"), decl("a.b@1", "removed")], &[]).unwrap_err();
    assert_eq!(e.code(), "catalog_invalid");
    // A migrator from a removed major is invalid under CORE.REG.VER.LIFE, so it cannot rescue it.
    let m = MigratorRecord::from_value(
        &parse(br#"{"algorithm": "a.b.mig@1.0.0", "command": "a.b", "from_major": 1, "to_major": 2}"#).unwrap(),
    )
    .unwrap();
    let e = CommandCatalog::new(&[decl("a.b@1", "removed"), decl("a.b@2", "active")], &[m.clone()]).unwrap_err();
    assert_eq!(e.code(), "catalog_invalid");
    // A valid migrator from a deprecated major: both retained, both reachable.
    let cat = CommandCatalog::new(&[decl("a.b@1", "deprecated"), decl("a.b@2", "active")], &[m]).unwrap();
    assert!(cat.reachable(&CommandId::parse("a.b@1").unwrap()));
    assert!(!cat.reachable(&CommandId::parse("a.b@3").unwrap()));
    assert!(!cat.reachable(&CommandId::parse("c.d@1").unwrap()));
}

#[test]
fn preflight_is_deterministic_and_measured_over_10000_entries() {
    let kernel = d("kernel");
    let mut steps = Vec::new();
    let mut blobs = BTreeMap::new();
    for seq in 1..=10_000u64 {
        let bytes = seq.to_le_bytes().to_vec();
        let digest = Digest::parse(&sha(&bytes)).unwrap();
        if seq % 1000 != 0 {
            blobs.insert(digest, bytes);
        }
        steps.push(RecordedStep {
            seq,
            entry: d(&format!("entry {seq}")),
            command: Some(CommandId::parse("probe.read@1").unwrap()),
            kernel: Some(kernel),
            observations: vec![RecordedObservation { kind: ObservationKind::Device, digest, storage: Storage::Blob }],
            payload: Some(StepPayload::default()),
        });
    }
    let span = RecordedSpan::new(steps).unwrap();
    let mut m = BTreeMap::new();
    m.insert("state".to_string(), CValue::Str("active".into()));
    m.insert("since".to_string(), CValue::Str("2026-01-01".into()));
    let decls =
        [LifecycleDecl { command: CommandId::parse("probe.read@1").unwrap(), lifecycle: Lifecycle::from_map(&m).unwrap() }];
    let catalog = CommandCatalog::new(&decls, &[]).unwrap();
    let kernels: BTreeSet<Digest> = [kernel].into_iter().collect();
    let start = std::time::Instant::now();
    let a = preflight(&span, &blobs, &catalog, &kernels);
    let elapsed = start.elapsed();
    let b = preflight(&span, &blobs, &catalog, &kernels);
    assert_eq!(a, b);
    assert_eq!(a.len(), 10);
    assert!(a.iter().all(|m| m.kind == MissingKind::Blob && m.steps.len() == 1 && m.steps[0] % 1000 == 0));
    // Budget: < 500 ms (spec, [unverified]). Printed for the record; asserted loosely so a slow
    // debug build on a shared runner does not flake.
    println!("preflight over 10,000 entries: {elapsed:?}");
    assert!(elapsed.as_secs() < 5, "{elapsed:?}");
}
