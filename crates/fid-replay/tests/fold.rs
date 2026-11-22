//! Acceptance and edge-case tests for `CORE.RPL.RECON.FOLD` (`fid_replay::fold`).
//! Runs on native targets AND `wasm32-wasip1` (default features).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::time::Instant;

use fid_canon::{digest_bytes, parse, to_canonical_bytes, CValue};
use fid_journal::{commitment, Salt};
use fid_replay::{
    check_fold_invariant, content_digest, reconstruct, take, CloseSampler, DocState, DurableEntry, EventBody,
    FoldError, FoldInvariantOutcome, JournalReconstructor, LiveState, MemJournalReader, MemSnapshotCache,
    MemSnapshotTable, SnapshotCache, SnapshotRecord, SnapshotTable, VecIncidentSink, VecWarningSink,
    CLOSE_CHECK_INTERVAL,
};
use fid_types::Digest;

const SESSION_1200_JSON: &str = include_str!("../../../fixtures/core/rpl/recon/fold/session_1200.json");
const BENCH_10000_JSON: &str = include_str!("../../../fixtures/core/rpl/recon/fold/bench_10000.json");

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

/// Toy state with an injected bug that drops events whose `kind == "tag"` (for AC-5).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct BuggyKvState {
    inner: ToyKvState,
    drop_kind: Option<&'static str>,
}

impl BuggyKvState {
    fn apply_live(&mut self, event: &EventBody) -> Result<(), FoldError> {
        if let CValue::Obj(o) = event {
            if let (Some(drop_k), Some(CValue::Str(k))) = (self.drop_kind, o.get("kind")) {
                if k == drop_k {
                    return Ok(());
                }
            }
        }
        self.inner.apply(event)
    }
}

fn str_field<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> &'a str {
    match o.get(k) {
        Some(CValue::Str(s)) => s.as_str(),
        other => panic!("expected string at {k}, got {other:?}"),
    }
}

fn load_entry(v: &CValue) -> DurableEntry {
    let CValue::Obj(o) = v else { panic!("entry object") };
    let entry_hash = Digest::parse(str_field(o, "entry_hash")).unwrap();
    let header_bytes = to_canonical_bytes(o.get("header").unwrap()).unwrap();
    let logical_parent = match o.get("logical_parent") {
        Some(CValue::Str(s)) => Some(Digest::parse(s).unwrap()),
        _ => None,
    };
    let events_commitment = Some(Digest::parse(str_field(o, "events_commit")).unwrap());
    let payload_bytes = Some(to_canonical_bytes(o.get("payload").unwrap()).unwrap());
    DurableEntry { entry_hash, header_bytes: Some(header_bytes), logical_parent, events_commitment, payload_bytes }
}

struct Session1200 {
    journal: MemJournalReader,
    snapshots: MemSnapshotTable,
    cache: MemSnapshotCache,
    head_1197: Digest,
    head_1199: Digest,
    head_1200: Digest,
    side_head: Digest,
    digest_1197: Digest,
    digest_1199: Digest,
    digest_1200: Digest,
    side_digest: Digest,
    entry_hash_at_100: Digest,
    entry_hash_at_1150: Digest,
}

fn load_session_1200() -> Session1200 {
    let CValue::Obj(root) = parse(SESSION_1200_JSON.as_bytes()).unwrap() else { panic!("root object") };
    let mut journal = MemJournalReader::new();
    let CValue::Arr(entries) = &root["entries"] else { panic!("entries") };
    let mut hashes = Vec::with_capacity(entries.len());
    for e in entries {
        let de = load_entry(e);
        hashes.push(de.entry_hash);
        journal.insert(de);
    }
    let CValue::Arr(side_entries) = &root["side_branch_entries"] else { panic!("side entries") };
    for e in side_entries {
        journal.insert(load_entry(e));
    }

    let mut snapshots = MemSnapshotTable::new();
    let mut cache = MemSnapshotCache::new();
    let CValue::Arr(snaps) = &root["snapshots"] else { panic!("snapshots") };
    for s in snaps {
        let CValue::Obj(so) = s else { panic!("snap obj") };
        let entry_hash = Digest::parse(str_field(so, "entry_hash")).unwrap();
        let state_val = so.get("state").unwrap();
        let enc = to_canonical_bytes(state_val).unwrap();
        let rec = SnapshotRecord {
            entry_hash,
            format_version: 1,
            state_digest: Digest::parse(str_field(so, "state_digest")).unwrap(),
            content_digest: Digest::parse(str_field(so, "content_digest")).unwrap(),
            size: enc.len() as u64,
        };
        snapshots.insert(&rec).unwrap();
        cache.put(entry_hash, 1, &enc).unwrap();
    }

    let CValue::Obj(hd) = &root["head_digests"] else { panic!("head_digests") };
    Session1200 {
        journal,
        snapshots,
        cache,
        head_1197: Digest::parse(str_field(&root, "head_1197")).unwrap(),
        head_1199: Digest::parse(str_field(&root, "head_1199")).unwrap(),
        head_1200: Digest::parse(str_field(&root, "head_1200")).unwrap(),
        side_head: Digest::parse(str_field(&root, "side_branch_head")).unwrap(),
        digest_1197: Digest::parse(str_field(hd, "1197")).unwrap(),
        digest_1199: Digest::parse(str_field(hd, "1199")).unwrap(),
        digest_1200: Digest::parse(str_field(&root, "state_digest")).unwrap(),
        side_digest: Digest::parse(str_field(&root, "side_branch_digest")).unwrap(),
        entry_hash_at_100: hashes[99],
        entry_hash_at_1150: hashes[1149],
    }
}

/// AC-1: `session_1200` reconstructed at head 1200 with snapshots matches `session_1200.state_digest`;
/// with the snapshot cache deleted, full reconstruction produces the exact same digest.
#[test]
fn ac1_reconstruct_with_snapshots_and_with_deleted_cache_match_digest() {
    let mut s = load_session_1200();
    let mut warnings = VecWarningSink::new();

    // Reconstruct using snapshots (starts at entry 1000, applies 200 entries).
    let with_snap =
        reconstruct::<ToyKvState, _, _, _, _>(s.head_1200, &s.journal, &s.snapshots, &mut s.cache, &mut warnings)
            .unwrap();
    assert_eq!(with_snap.digest, s.digest_1200);
    assert!(with_snap.from_snapshot.is_some());
    assert_eq!(with_snap.applied_entries, 200);
    assert_eq!(with_snap.applied_events, 200);
    assert!(warnings.warnings.is_empty());

    // Delete entire snapshot cache and reconstruct from entry 1.
    let mut empty_cache = MemSnapshotCache::new();
    let without_cache =
        reconstruct::<ToyKvState, _, _, _, _>(s.head_1200, &s.journal, &s.snapshots, &mut empty_cache, &mut warnings)
            .unwrap();
    assert_eq!(without_cache.digest, s.digest_1200);
    assert_eq!(without_cache.state, with_snap.state);
    assert!(without_cache.from_snapshot.is_none());
    assert_eq!(without_cache.applied_entries, 1200);
}

/// AC-3: Undo 3 entries (1200 -> 1197), redo 2 entries (1197 -> 1199), and switch to side branch;
/// every head move matches its committed digest.
#[test]
fn ac3_undo_redo_and_branch_switch_match_recorded_digests() {
    let mut s = load_session_1200();
    let mut warnings = VecWarningSink::new();
    let mut recon = JournalReconstructor {
        journal: &s.journal,
        snapshots: &s.snapshots,
        cache: &mut s.cache,
        warnings: &mut warnings,
    };

    let mut live = LiveState::new(ToyKvState::default(), None);
    let d1200 = live.move_head(s.head_1200, &mut recon).unwrap();
    assert_eq!(d1200, s.digest_1200);
    assert_eq!(live.head(), Some(s.head_1200));

    // Undo 3 entries -> head 1197.
    let d1197 = live.move_head(s.head_1197, &mut recon).unwrap();
    assert_eq!(d1197, s.digest_1197);
    assert_eq!(live.head(), Some(s.head_1197));

    // Redo 2 entries -> head 1199.
    let d1199 = live.move_head(s.head_1199, &mut recon).unwrap();
    assert_eq!(d1199, s.digest_1199);
    assert_eq!(live.head(), Some(s.head_1199));

    // Branch switch to side branch rooted at entry 500.
    let d_side = live.move_head(s.side_head, &mut recon).unwrap();
    assert_eq!(d_side, s.side_digest);
    assert_eq!(live.head(), Some(s.side_head));
}

/// AC-4 (error path): Detached payload with no covering snapshot fails with `payload_missing`
/// naming the entry and leaves the previous live state untouched; a covering snapshot allows
/// reconstruction to succeed.
#[test]
fn ac4_detached_payload_fails_and_preserves_live_state() {
    let mut s = load_session_1200();
    let mut warnings = VecWarningSink::new();

    // Establish live state at head 1197 first.
    let mut live = {
        let mut recon = JournalReconstructor {
            journal: &s.journal,
            snapshots: &s.snapshots,
            cache: &mut s.cache,
            warnings: &mut warnings,
        };
        let mut l = LiveState::new(ToyKvState::default(), None);
        l.move_head(s.head_1197, &mut recon).unwrap();
        l
    };
    let live_snapshot_before = live.state().clone();

    // Detach payload at entry 100 (covered by snapshots 500 & 1000): reconstruction at 1200 SUCCEEDS!
    assert!(s.journal.detach_payload(&s.entry_hash_at_100));
    {
        let mut recon = JournalReconstructor {
            journal: &s.journal,
            snapshots: &s.snapshots,
            cache: &mut s.cache,
            warnings: &mut warnings,
        };
        let d = live.move_head(s.head_1200, &mut recon).unwrap();
        assert_eq!(d, s.digest_1200);
    }

    // Move back to 1197, then detach payload at entry 1150 (after snapshot 1000 -> NO covering snapshot).
    {
        let mut recon = JournalReconstructor {
            journal: &s.journal,
            snapshots: &s.snapshots,
            cache: &mut s.cache,
            warnings: &mut warnings,
        };
        live.move_head(s.head_1197, &mut recon).unwrap();
    }
    assert!(s.journal.detach_payload(&s.entry_hash_at_1150));

    let err = {
        let mut recon = JournalReconstructor {
            journal: &s.journal,
            snapshots: &s.snapshots,
            cache: &mut s.cache,
            warnings: &mut warnings,
        };
        live.move_head(s.head_1200, &mut recon).unwrap_err()
    };
    assert_eq!(err, FoldError::PayloadMissing { entry_hash: s.entry_hash_at_1150 });
    assert_eq!(err.code(), "payload_missing");
    // Previous live state is untouched!
    assert_eq!(live.head(), Some(s.head_1197));
    assert_eq!(live.digest(), s.digest_1197);
    assert_eq!(live.state(), &live_snapshot_before);
}

/// AC-5: Toy `DocState` with an injected bug dropping one event kind triggers a mismatch on
/// `check_fold_invariant` and logs 1 integrity incident; `CloseSampler` runs deterministically
/// at least 1 close in 20 by counter.
#[test]
fn ac5_fold_invariant_detects_buggy_live_state_and_close_sampler_is_deterministic() {
    let mut journal = MemJournalReader::new();
    let snapshots = MemSnapshotTable::new();
    let mut cache = MemSnapshotCache::new();
    let mut warnings = VecWarningSink::new();
    let mut incidents = VecIncidentSink::new();

    let mut buggy_live = BuggyKvState { inner: ToyKvState::default(), drop_kind: Some("tag") };
    let mut parent: Option<Digest> = None;
    let mut head = Digest::from_bytes([0u8; 32]);

    for (seq, (kind, key, val)) in
        [(1u64, ("set", "a", "1")), (2u64, ("tag", "label", "critical")), (3u64, ("set", "b", "2"))]
    {
        let mut ev = BTreeMap::new();
        ev.insert("key".to_string(), CValue::Str(key.to_string()));
        ev.insert("kind".to_string(), CValue::Str(kind.to_string()));
        ev.insert("value".to_string(), CValue::Str(val.to_string()));
        let events_arr = CValue::Arr(vec![CValue::Obj(ev.clone())]);
        let salt = Salt([seq as u8; 16]);
        let ev_commit = Digest::from_bytes(commitment(&salt, &events_arr).unwrap().0);

        let mut hdr = BTreeMap::new();
        hdr.insert("events_commit".to_string(), CValue::Str(ev_commit.as_str().to_string()));
        if let Some(p) = parent {
            hdr.insert("logical_parent".to_string(), CValue::Str(p.as_str().to_string()));
        }
        hdr.insert("seq".to_string(), CValue::Num(seq as f64));
        let header_bytes = to_canonical_bytes(&CValue::Obj(hdr)).unwrap();
        let entry_hash = Digest::from_bytes(digest_bytes(&header_bytes).0);

        let mut salts = BTreeMap::new();
        salts.insert("events".to_string(), CValue::Str(salt.to_hex()));
        let mut payload = BTreeMap::new();
        payload.insert("events".to_string(), events_arr);
        payload.insert("salts".to_string(), CValue::Obj(salts));
        let payload_bytes = to_canonical_bytes(&CValue::Obj(payload)).unwrap();

        journal.insert(DurableEntry {
            entry_hash,
            header_bytes: Some(header_bytes),
            logical_parent: parent,
            events_commitment: Some(ev_commit),
            payload_bytes: Some(payload_bytes),
        });
        buggy_live.apply_live(&CValue::Obj(ev)).unwrap();
        parent = Some(entry_hash);
        head = entry_hash;
    }

    let outcome =
        check_fold_invariant(&buggy_live.inner, head, &journal, &snapshots, &mut cache, &mut warnings, &mut incidents)
            .unwrap();
    match outcome {
        FoldInvariantOutcome::Mismatch(m) => {
            assert_eq!(m.head, head);
            assert_ne!(m.live_digest, m.folded_digest);
        }
        other => panic!("expected Mismatch, got {other:?}"),
    }
    assert_eq!(incidents.incidents.len(), 1);
    assert_eq!(incidents.incidents[0].head, head);

    // CloseSampler runs deterministically on every 20th close (and clamps intervals > 20 to 20).
    let mut sampler = CloseSampler::new(999);
    assert_eq!(sampler.interval(), CLOSE_CHECK_INTERVAL);
    let mut checked_on = Vec::new();
    let mut inc2 = VecIncidentSink::new();
    for close_idx in 1..=40u64 {
        let res = sampler
            .on_close(&buggy_live.inner, head, &journal, &snapshots, &mut cache, &mut warnings, &mut inc2)
            .unwrap();
        if res.is_some() {
            checked_on.push(close_idx);
        }
    }
    assert_eq!(checked_on, vec![20, 40]);
    assert_eq!(inc2.incidents.len(), 2);
}

/// AC-6: 10,000-entry project reconstructed from a snapshot 400 entries old completes well under 2 s
/// and matches the independent Python reference digest.
#[test]
fn ac6_bench_10000_entries_reconstructs_under_2s_and_matches_digest() {
    let CValue::Obj(meta) = parse(BENCH_10000_JSON.as_bytes()).unwrap() else { panic!("meta") };
    let want_head_digest = Digest::parse(str_field(&meta, "head_digest")).unwrap();
    let want_snap_digest = Digest::parse(str_field(&meta, "snapshot_digest")).unwrap();
    let snap_at = 9600usize;

    let mut journal = MemJournalReader::new();
    let mut snapshots = MemSnapshotTable::new();
    let mut cache = MemSnapshotCache::new();
    let mut warnings = VecWarningSink::new();

    let mut state = ToyKvState::default();
    let mut parent: Option<Digest> = None;
    let mut head = Digest::from_bytes([0u8; 32]);

    for seq in 1..=10_000usize {
        let key = format!("k_{:03}", seq % 251);
        let val = format!("b_{seq:05}");
        let mut ev = BTreeMap::new();
        ev.insert("key".to_string(), CValue::Str(key));
        ev.insert("value".to_string(), CValue::Str(val));
        let ev_val = CValue::Obj(ev);
        state.apply(&ev_val).unwrap();

        let events_arr = CValue::Arr(vec![ev_val]);
        let mut sb = [0u8; 16];
        sb[..8].copy_from_slice(&(seq as u64).to_le_bytes());
        let salt = Salt(sb);
        let ev_commit = Digest::from_bytes(commitment(&salt, &events_arr).unwrap().0);

        let mut hdr = BTreeMap::new();
        hdr.insert("events_commit".to_string(), CValue::Str(ev_commit.as_str().to_string()));
        if let Some(p) = parent {
            hdr.insert("logical_parent".to_string(), CValue::Str(p.as_str().to_string()));
        }
        hdr.insert("seq".to_string(), CValue::Num(seq as f64));
        let header_bytes = to_canonical_bytes(&CValue::Obj(hdr)).unwrap();
        let entry_hash = Digest::from_bytes(digest_bytes(&header_bytes).0);

        let mut salts = BTreeMap::new();
        salts.insert("events".to_string(), CValue::Str(salt.to_hex()));
        let mut payload = BTreeMap::new();
        payload.insert("events".to_string(), events_arr);
        payload.insert("salts".to_string(), CValue::Obj(salts));
        let payload_bytes = to_canonical_bytes(&CValue::Obj(payload)).unwrap();

        journal.insert(DurableEntry {
            entry_hash,
            header_bytes: Some(header_bytes),
            logical_parent: parent,
            events_commitment: Some(ev_commit),
            payload_bytes: Some(payload_bytes),
        });
        parent = Some(entry_hash);
        head = entry_hash;

        if seq == snap_at {
            assert_eq!(state.digest(), want_snap_digest);
            let rec = take(&state, entry_hash, &mut cache, &mut snapshots, &mut warnings);
            assert!(rec.is_some());
        }
    }

    let start = Instant::now();
    let rebuilt = reconstruct::<ToyKvState, _, _, _, _>(head, &journal, &snapshots, &mut cache, &mut warnings).unwrap();
    let elapsed = start.elapsed();

    assert_eq!(rebuilt.digest, want_head_digest);
    assert_eq!(rebuilt.applied_entries, 400);
    assert!(
        elapsed.as_secs_f64() < 2.0,
        "10,000-entry reconstruction took {:.3}s (budget < 2.0s)",
        elapsed.as_secs_f64()
    );
}

/// Tampered payload commitment or cyclic logical parents fail closed with `decode_failed`.
#[test]
fn tampered_commitment_and_cycle_fail_closed() {
    let mut s = load_session_1200();
    let mut warnings = VecWarningSink::new();

    // Tamper payload at entry 1150 (after snapshot 1000).
    let mut tampered = s.journal.clone();
    let bad_entry = DurableEntry {
        entry_hash: s.entry_hash_at_1150,
        header_bytes: None,
        logical_parent: None,
        events_commitment: Some(Digest::from_bytes([0xab; 32])),
        payload_bytes: Some(br#"{"events":[],"salts":{"events":"00000000000000000000000000000000"}}"#.to_vec()),
    };
    tampered.insert(bad_entry);
    let err = reconstruct::<ToyKvState, _, _, _, _>(
        s.entry_hash_at_1150,
        &tampered,
        &MemSnapshotTable::new(),
        &mut s.cache,
        &mut warnings,
    )
    .unwrap_err();
    assert_eq!(err.code(), "decode_failed");

    // Cycle: entry A -> parent B -> parent A.
    let mut cyc = MemJournalReader::new();
    let a = Digest::from_bytes([1u8; 32]);
    let b = Digest::from_bytes([2u8; 32]);
    cyc.insert(DurableEntry {
        entry_hash: a,
        header_bytes: None,
        logical_parent: Some(b),
        events_commitment: None,
        payload_bytes: None,
    });
    cyc.insert(DurableEntry {
        entry_hash: b,
        header_bytes: None,
        logical_parent: Some(a),
        events_commitment: None,
        payload_bytes: None,
    });
    let err_cyc = reconstruct::<ToyKvState, _, _, _, _>(
        a,
        &cyc,
        &MemSnapshotTable::new(),
        &mut MemSnapshotCache::new(),
        &mut warnings,
    )
    .unwrap_err();
    assert_eq!(err_cyc.code(), "decode_failed");
}
