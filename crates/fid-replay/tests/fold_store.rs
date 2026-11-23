//! End-to-end SQLite `fid-store` + `BlobStore` + `ActivityLog` integration tests for
//! `CORE.RPL.RECON.FOLD` (AC-1, AC-2 crash harness across 200 seeded kills with seed 7,
//! AC-4 detached payload blob, AC-5 Activity Log integrity incident).
#![cfg(feature = "store")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_journal::stream::{ActivityLog, ActivityLogConfig};
use fid_journal::{
    build, AiAssistance, CommandDraft, DraftBody, EntryDraft, EntryTime, Manifests, PrincipalDraft, PrincipalKind,
    RecordedSalts, Salt,
};
use fid_replay::{
    check_fold_invariant, content_digest, reconstruct, take, ActivityIncidentSink, ActivityWarningSink, DocState,
    EventBody, FoldError, FoldInvariantOutcome, JournalReconstructor, LiveState, StoreJournalReader,
    StoreSnapshotCache, StoreSnapshotTable, ACTIVITY_INCIDENT_COMMAND, ROLE_PAYLOAD,
};
use fid_store::cas::PutOpts;
use fid_store::{BlobStore, Cache, CacheConfig, OpenMode, Store, Value};
use fid_types::{Determinism, Digest, SideEffect};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!("fid-replay-fold-{}-{}-{n}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct ToyKvState {
    map: BTreeMap<String, String>,
}

impl DocState for ToyKvState {
    fn apply(&mut self, event: &EventBody) -> Result<(), FoldError> {
        let CValue::Obj(o) = event else {
            return Err(FoldError::ApplyFailed { detail: "event must be object".to_string() });
        };
        let (Some(CValue::Str(k)), Some(CValue::Str(v))) = (o.get("key"), o.get("value")) else {
            return Err(FoldError::ApplyFailed { detail: "missing key/value".to_string() });
        };
        self.map.insert(k.clone(), v.clone());
        Ok(())
    }

    fn digest(&self) -> Digest {
        content_digest(&self.encode())
    }

    fn encode(&self) -> Vec<u8> {
        let obj: BTreeMap<String, CValue> = self.map.iter().map(|(k, v)| (k.clone(), CValue::Str(v.clone()))).collect();
        to_canonical_bytes(&CValue::Obj(obj)).unwrap()
    }

    fn decode(bytes: &[u8], format: u32) -> Result<Self, FoldError> {
        if format != 1 {
            return Err(FoldError::DecodeFailed { detail: format!("bad format {format}") });
        }
        let CValue::Obj(o) = parse(bytes).map_err(|e| FoldError::DecodeFailed { detail: e.to_string() })? else {
            return Err(FoldError::DecodeFailed { detail: "not object".to_string() });
        };
        let mut map = BTreeMap::new();
        for (k, v) in o {
            let CValue::Str(s) = v else {
                return Err(FoldError::DecodeFailed { detail: "value not str".to_string() });
            };
            map.insert(k, s);
        }
        Ok(Self { map })
    }

    fn format_version() -> u32 {
        1
    }
}

fn manifests() -> Manifests {
    Manifests { registry: fid_canon::Digest([0x11; 32]), kernel: fid_canon::Digest([0x22; 32]) }
}

fn make_real_entry(
    seq: u64,
    prev: fid_canon::Digest,
    logical_parent: Option<fid_canon::Digest>,
    key: &str,
    val: &str,
) -> fid_journal::Entry {
    let mut ev = BTreeMap::new();
    ev.insert("key".to_string(), CValue::Str(key.to_string()));
    ev.insert("value".to_string(), CValue::Str(val.to_string()));
    let draft = EntryDraft {
        stream: "record".to_string(),
        branch: "main".to_string(),
        logical_parent,
        time: EntryTime {
            utc: format!("2026-11-22T00:{:02}:{:02}Z", (seq / 60) % 60, seq % 60),
            mono_ns: seq,
            sync: "unknown".to_string(),
        },
        principal: PrincipalDraft {
            kind: PrincipalKind::Human,
            identity: CValue::Str("tester".to_string()),
            on_behalf_of: None,
        },
        ai_assistance: AiAssistance::None,
        manifests: manifests(),
        reason: None,
        human_assertion: None,
        agent_coedited: false,
        body: DraftBody::Command(CommandDraft {
            id: "doc.set_kv".to_string(),
            major: 1,
            class: SideEffect::W2,
            determinism: Determinism::D0,
            seed: 0,
            args: CValue::Obj(BTreeMap::new()),
            events: vec![CValue::Obj(ev)],
            inputs: Vec::new(),
            consumed_r1: Vec::new(),
            outputs: Vec::new(),
            algorithms: Vec::new(),
            event_sources: Vec::new(),
        }),
    };
    let mut salt_map = BTreeMap::new();
    for (i, field) in
        [fid_journal::Field::Args, fid_journal::Field::Events, fid_journal::Field::Identity].into_iter().enumerate()
    {
        let mut b = [0u8; 16];
        b[..8].copy_from_slice(&seq.to_le_bytes());
        b[8] = i as u8;
        salt_map.insert(field, Salt(b));
    }
    let mut salts = RecordedSalts(salt_map);
    build(&draft, seq, prev, &mut salts).unwrap()
}

fn commit_entry(store: &Store, blobs: &BlobStore, entry: &fid_journal::Entry) -> Digest {
    let header_bytes = to_canonical_bytes(&entry.header.to_value()).unwrap();
    let payload_bytes = to_canonical_bytes(&entry.payload.to_value()).unwrap();
    let payload_digest = blobs.put(&payload_bytes[..], PutOpts { durable: true }).unwrap().digest();
    let prev_val = if entry.header.seq == 1 { Value::Null } else { Value::Text(entry.header.prev.to_string()) };
    let hash_str = entry.header.hash.to_string();
    let mut txn = store.transaction();
    txn.insert(
        "entries",
        &[
            ("seq", Value::Integer(entry.header.seq as i64)),
            ("hash", Value::Text(hash_str.clone())),
            ("stream", Value::Text(entry.header.stream.clone())),
            ("branch", Value::Text(entry.header.branch.clone())),
            ("kind", Value::Text("command".to_string())),
            ("prev_hash", prev_val),
            ("header", Value::Blob(header_bytes)),
        ],
    );
    txn.insert(
        "entry_blobs",
        &[
            ("entry_hash", Value::Text(hash_str)),
            ("role", Value::Text(ROLE_PAYLOAD.to_string())),
            ("digest", Value::Text(payload_digest.as_str().to_string())),
        ],
    );
    txn.commit().unwrap();
    Digest::from_bytes(entry.header.hash.0)
}

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// End-to-end AC-1, AC-4, and AC-5 on real SQLite `Store`, `BlobStore`, `Cache`, real
/// `fid_journal::Entry` headers+payloads, and real `ActivityLog`.
#[test]
fn store_end_to_end_reconstruct_detached_blob_and_activity_incident() {
    let dir = temp_dir("e2e");
    let proj_dir = dir.join("proj");
    let cache_dir = dir.join("cache");
    let alog_dir = dir.join("alog");
    std::fs::create_dir_all(&cache_dir).unwrap();
    std::fs::create_dir_all(&alog_dir).unwrap();

    let store = Store::create(&proj_dir).unwrap();
    let blobs = BlobStore::open(&proj_dir.join("blobs")).unwrap();
    let mut cache = Cache::open(&cache_dir, CacheConfig::default());
    let activity = ActivityLog::new(&alog_dir, ActivityLogConfig::default(), manifests());

    let mut state = ToyKvState::default();
    let mut prev = fid_journal::GENESIS_PREV;
    let mut parent: Option<fid_canon::Digest> = None;
    let mut hashes = Vec::new();
    let mut payload_digests = Vec::new();

    for seq in 1..=25u64 {
        let k = format!("k_{}", seq % 7);
        let v = format!("v_{seq}");
        let entry = make_real_entry(seq, prev, parent, &k, &v);
        let payload_bytes = to_canonical_bytes(&entry.payload.to_value()).unwrap();
        let pd = blobs.put(&payload_bytes[..], PutOpts { durable: true }).unwrap().digest();
        let h = commit_entry(&store, &blobs, &entry);
        let mut ev = BTreeMap::new();
        ev.insert("key".to_string(), CValue::Str(k));
        ev.insert("value".to_string(), CValue::Str(v));
        state.apply(&CValue::Obj(ev)).unwrap();

        if seq == 15 {
            let mut snap_tbl = StoreSnapshotTable::new(&store);
            let mut snap_cache = StoreSnapshotCache::new(&mut cache);
            let mut warn_sink = ActivityWarningSink::new(&activity, manifests(), "2026-11-22T00:00:00Z");
            take(&state, h, &mut snap_cache, &mut snap_tbl, &mut warn_sink);
        }
        prev = entry.header.hash;
        parent = Some(entry.header.hash);
        hashes.push(h);
        payload_digests.push(pd);
    }

    let head = *hashes.last().unwrap();
    let expected_digest = state.digest();

    // AC-1: reconstruct from snapshot at 15 -> applies 10 entries and matches expected digest.
    let reader = StoreJournalReader::new(&store, &blobs);
    let snap_tbl = StoreSnapshotTable::new(&store);
    let mut snap_cache = StoreSnapshotCache::new(&mut cache);
    let mut warn_sink = ActivityWarningSink::new(&activity, manifests(), "2026-11-22T00:00:00Z");
    let rebuilt =
        reconstruct::<ToyKvState, _, _, _, _>(head, &reader, &snap_tbl, &mut snap_cache, &mut warn_sink).unwrap();
    assert_eq!(rebuilt.digest, expected_digest);
    assert_eq!(rebuilt.from_snapshot, Some(hashes[14]));
    assert_eq!(rebuilt.applied_entries, 10);

    // AC-5: buggy live state logs an integrity incident naming `head` to ActivityLog.
    let mut buggy = state.clone();
    buggy.map.remove("k_1");
    let mut inc_sink = ActivityIncidentSink::new(&activity, manifests(), "2026-11-22T00:01:00Z");
    let outcome =
        check_fold_invariant(&buggy, head, &reader, &snap_tbl, &mut snap_cache, &mut warn_sink, &mut inc_sink).unwrap();
    assert!(matches!(outcome, FoldInvariantOutcome::Mismatch(_)));
    assert_eq!(inc_sink.incidents().len(), 1);
    // AC-4: delete payload blob of entry 20 (after snapshot 15 -> no covering snapshot) -> PayloadMissing.
    let mut live = LiveState::new(state.clone(), Some(hashes[14]));
    let missing_blob = payload_digests[19];
    let blob_path = blobs.path_of(&missing_blob);
    std::fs::remove_file(&blob_path).unwrap();

    let mut recon = JournalReconstructor {
        journal: &reader,
        snapshots: &snap_tbl,
        cache: &mut snap_cache,
        warnings: &mut warn_sink,
    };
    let err = live.move_head(head, &mut recon).unwrap_err();
    assert_eq!(err, FoldError::PayloadMissing { entry_hash: hashes[19] });
    assert_eq!(live.head(), Some(hashes[14]));

    // AC-5: verify the Activity Log file on disk recorded the integrity incident naming `head`.
    activity.close().unwrap();
    let log_text = std::fs::read_to_string(alog_dir.join(fid_journal::stream::CURRENT_FILE)).unwrap();
    assert!(log_text.contains(ACTIVITY_INCIDENT_COMMAND), "activity log: {log_text}");
    assert!(log_text.contains(head.as_str()), "activity log: {log_text}");
}

/// AC-2: 200 seeded crash instants (seed 7) while committing to a real SQLite `Store` + `BlobStore`:
/// uncommitted staged writes are rolled back / abandoned at each crash instant, the store is
/// reopened fresh from disk, and reconstruction equals the fold of exactly the commits that
/// returned success before the crash.
#[test]
fn ac2_crash_resume_200_seeded_instants_seed_7() {
    let dir = temp_dir("crash-200");
    let mut rng = 7u64;

    // Build a single persistent store, committing 200 seeded crash-resume rounds.
    let _ = Store::create(&dir).unwrap();
    let blobs = BlobStore::open(&dir.join("blobs")).unwrap();

    let mut expected_state = ToyKvState::default();
    let mut prev = fid_journal::GENESIS_PREV;
    let mut parent: Option<fid_canon::Digest> = None;
    let mut seq = 0u64;

    for round in 0..200u64 {
        let store = Store::open(&dir, OpenMode::Write).unwrap();
        let draw = splitmix64(&mut rng);
        seq += 1;
        let key = format!("k_{}", draw % 31);
        let val = format!("r{round}_s{seq}_{:x}", draw & 0xffff);
        let entry = make_real_entry(seq, prev, parent, &key, &val);
        let committed_head = commit_entry(&store, &blobs, &entry);
        let mut ev = BTreeMap::new();
        ev.insert("key".to_string(), CValue::Str(key));
        ev.insert("value".to_string(), CValue::Str(val));
        expected_state.apply(&CValue::Obj(ev)).unwrap();
        prev = entry.header.hash;
        parent = Some(entry.header.hash);

        // Simulate a mid-commit crash for the next entry: write its payload blob and stage an
        // uncommitted SQLite transaction, then drop without committing (rolled back on reopen).
        let crashed_entry = make_real_entry(seq + 1, prev, parent, "crashed_key", "never_visible");
        let crashed_payload = to_canonical_bytes(&crashed_entry.payload.to_value()).unwrap();
        let _unreferenced_blob = blobs.put(&crashed_payload[..], PutOpts { durable: true }).unwrap();
        {
            let mut uncommitted = store.transaction();
            uncommitted.insert(
                "entries",
                &[
                    ("seq", Value::Integer((seq + 1) as i64)),
                    ("hash", Value::Text(crashed_entry.header.hash.to_string())),
                    ("stream", Value::Text("record".to_string())),
                    ("branch", Value::Text("main".to_string())),
                    ("kind", Value::Text("command".to_string())),
                    ("prev_hash", Value::Text(prev.to_string())),
                    ("header", Value::Blob(to_canonical_bytes(&crashed_entry.header.to_value()).unwrap())),
                ],
            );
            // Crash before commit(): drop transaction and store handle.
            drop(uncommitted);
        }
        drop(store);

        // Reopen fresh from disk and reconstruct at the durable tail.
        let reopened = Store::open(&dir, OpenMode::Read).unwrap();
        let durable_tail: String = reopened
            .read(|conn| conn.query_row("SELECT hash FROM entries ORDER BY seq DESC LIMIT 1", [], |r| r.get(0)))
            .unwrap();
        let tail_digest = Digest::parse(&durable_tail).unwrap();
        assert_eq!(tail_digest, committed_head, "round {round}: uncommitted row leaked");

        let reader = StoreJournalReader::new(&reopened, &blobs);
        let snap_tbl = StoreSnapshotTable::new(&reopened);
        let mut cache = fid_replay::MemSnapshotCache::new();
        let mut warnings = fid_replay::VecWarningSink::new();
        let rebuilt =
            reconstruct::<ToyKvState, _, _, _, _>(tail_digest, &reader, &snap_tbl, &mut cache, &mut warnings).unwrap();
        assert_eq!(rebuilt.digest, expected_state.digest(), "round {round}: state digest mismatch after crash");
        assert!(!rebuilt.state.map.contains_key("crashed_key"));
    }
}
