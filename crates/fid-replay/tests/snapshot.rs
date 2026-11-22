//! Acceptance criteria of spec node `CORE.RPL.RECON.SNAP` (AC-1..AC-4, REQ-001..REQ-006)
//! running purely in memory so these tests execute on every target including `wasm32-wasip1`.
//! End-to-end tests against SQLite `fid_store::Store`, filesystem `fid_store::Cache` and
//! `fid_journal::stream::ActivityLog` live in `tests/snapshot_store.rs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_replay::{
    content_digest, select_valid, take, DocState, EventBody, FoldError, MemLogicalAncestors, MemSnapshotCache,
    MemSnapshotTable, SnapshotCache, SnapshotPolicy, SnapshotRecord, SnapshotTable, SnapshotWarning, VecWarningSink,
};
use fid_types::Digest;

const SESSION_JSON: &str = include_str!("../../../fixtures/core/rpl/recon/snap/session_1200.json");

/// Toy `DocState`: a sorted string -> string map (matching `fixtures/core/rpl/recon/snap/make_fixtures.py`).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct ToyKvState {
    map: BTreeMap<String, String>,
}

impl DocState for ToyKvState {
    fn apply(&mut self, event: &EventBody) -> Result<(), FoldError> {
        let CValue::Obj(o) = event else {
            return Err(FoldError::ApplyFailed { detail: "event must be an object".to_string() });
        };
        let (Some(CValue::Str(k)), Some(CValue::Str(v))) = (o.get("key"), o.get("value")) else {
            return Err(FoldError::ApplyFailed { detail: "event requires string key and value".to_string() });
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
        if format != Self::format_version() {
            return Err(FoldError::DecodeFailed { detail: format!("unsupported format {format}") });
        }
        let val = parse(bytes).map_err(|e| FoldError::DecodeFailed { detail: e.to_string() })?;
        let CValue::Obj(o) = val else {
            return Err(FoldError::DecodeFailed { detail: "encoded state must be an object".to_string() });
        };
        let mut map = BTreeMap::new();
        for (k, v) in o {
            let CValue::Str(s) = v else {
                return Err(FoldError::DecodeFailed { detail: format!("value for {k} must be a string") });
            };
            map.insert(k, s);
        }
        Ok(Self { map })
    }

    fn format_version() -> u32 {
        1
    }
}

struct FixtureEntry {
    index: usize,
    hash: Digest,
    parent: Option<Digest>,
    event: EventBody,
}

struct Fixture {
    main: Vec<FixtureEntry>,
    branch_b: Vec<FixtureEntry>,
    checkpoints: BTreeMap<String, SnapshotRecord>,
}

fn parse_entries(val: &CValue) -> Vec<FixtureEntry> {
    let CValue::Arr(items) = val else { panic!("entries must be an array") };
    items
        .iter()
        .map(|it| {
            let CValue::Obj(o) = it else { panic!("entry must be object") };
            let CValue::Num(idx) = o.get("index").unwrap() else { panic!("index") };
            let CValue::Str(h) = o.get("hash").unwrap() else { panic!("hash") };
            let parent = match o.get("parent") {
                Some(CValue::Str(p)) => Some(Digest::parse(p).unwrap()),
                Some(CValue::Null) | None => None,
                _ => panic!("parent"),
            };
            let mut ev = BTreeMap::new();
            ev.insert("key".to_string(), o.get("key").unwrap().clone());
            ev.insert("value".to_string(), o.get("value").unwrap().clone());
            FixtureEntry { index: *idx as usize, hash: Digest::parse(h).unwrap(), parent, event: CValue::Obj(ev) }
        })
        .collect()
}

fn load_fixture() -> Fixture {
    let doc = parse(SESSION_JSON.as_bytes()).unwrap();
    let CValue::Obj(root) = doc else { panic!("fixture root must be object") };
    let main = parse_entries(root.get("main").unwrap());
    let branch_b = parse_entries(root.get("branch_b").unwrap());
    let CValue::Obj(cp_obj) = root.get("checkpoints").unwrap() else { panic!("checkpoints") };
    let mut checkpoints = BTreeMap::new();
    for (name, v) in cp_obj {
        let CValue::Obj(o) = v else { panic!("checkpoint object") };
        let get_str = |k: &str| match o.get(k).unwrap() {
            CValue::Str(s) => s.as_str(),
            _ => panic!("expected str {k}"),
        };
        let get_num = |k: &str| match o.get(k).unwrap() {
            CValue::Num(n) => *n as u64,
            _ => panic!("expected num {k}"),
        };
        checkpoints.insert(
            name.clone(),
            SnapshotRecord {
                entry_hash: Digest::parse(get_str("entry_hash")).unwrap(),
                format_version: get_num("format_version") as u32,
                state_digest: Digest::parse(get_str("state_digest")).unwrap(),
                content_digest: Digest::parse(get_str("content_digest")).unwrap(),
                size: get_num("size"),
            },
        );
    }
    Fixture { main, branch_b, checkpoints }
}

fn run_session_1200(
    fix: &Fixture,
) -> (ToyKvState, MemSnapshotCache, MemSnapshotTable, MemLogicalAncestors, VecWarningSink) {
    let mut state = ToyKvState::default();
    let mut policy = SnapshotPolicy::new();
    let mut cache = MemSnapshotCache::new();
    let mut table = MemSnapshotTable::new();
    let mut ancestors = MemLogicalAncestors::new();
    let mut warnings = VecWarningSink::new();

    for e in &fix.main {
        ancestors.insert(e.hash, e.parent);
        state.apply(&e.event).unwrap();
        policy.on_commit("main", &state, e.hash, &mut cache, &mut table, &mut warnings);
    }
    let head = fix.main.last().unwrap().hash;
    policy.on_close("main", &state, head, &mut cache, &mut table, &mut warnings);

    (state, cache, table, ancestors, warnings)
}

#[test]
fn ac1_snapshots_at_500_1000_and_1200_and_nowhere_else() {
    let fix = load_fixture();
    let (_state, _cache, table, _ancestors, warnings) = run_session_1200(&fix);
    assert!(warnings.warnings.is_empty(), "clean session must emit no warnings: {:?}", warnings.warnings);

    let rows = table.list_all().unwrap();
    assert_eq!(rows.len(), 3, "expected snapshots only at 500, 1000, 1200");
    assert_eq!(rows[0], fix.checkpoints["main@500"]);
    assert_eq!(rows[1], fix.checkpoints["main@1000"]);
    assert_eq!(rows[2], fix.checkpoints["main@1200"]);

    // Verify indices match exactly 500, 1000, 1200.
    assert_eq!(fix.main[499].index, 500);
    assert_eq!(fix.main[999].index, 1000);
    assert_eq!(fix.main[1199].index, 1200);
    assert_eq!(rows[0].entry_hash, fix.main[499].hash);
    assert_eq!(rows[1].entry_hash, fix.main[999].hash);
    assert_eq!(rows[2].entry_hash, fix.main[1199].hash);
}

#[test]
fn ac2_corrupted_1000_falls_back_to_500_evicts_and_logs_warning() {
    let fix = load_fixture();
    let (_state, mut cache, table, ancestors, mut warnings) = run_session_1200(&fix);

    let h1200 = fix.checkpoints["main@1200"].entry_hash;
    let h1000 = fix.checkpoints["main@1000"].entry_hash;
    let h500 = fix.checkpoints["main@500"].entry_hash;

    // Remove 1200 from cache (normal eviction) and flip one byte in 1000's cached bytes.
    cache.evict(h1200, 1);
    let raw1000 = cache.get_raw_mut(h1000, 1).unwrap();
    raw1000[0] ^= 0x01;

    let selected = select_valid::<ToyKvState>(&h1200, &ancestors, &table, &mut cache, &mut warnings)
        .unwrap()
        .expect("should fall back to snapshot at 500");

    assert_eq!(selected.0, fix.checkpoints["main@500"]);
    assert_eq!(selected.1.digest(), fix.checkpoints["main@500"].state_digest);
    assert!(!cache.contains(h1000, 1), "corrupted snapshot 1000 must be evicted from cache");
    assert!(cache.contains(h500, 1), "valid snapshot 500 must stay cached");
    assert_eq!(warnings.warnings.len(), 1, "expected exactly one integrity warning");
    assert_eq!(warnings.warnings[0].code(), "snapshot_invalid");
    assert_eq!(warnings.warnings[0].entry_hash(), h1000);
}

#[test]
fn ac3_branch_forked_at_700_ignores_main_1000_and_selects_500() {
    let fix = load_fixture();
    let (_state, mut cache, table, mut ancestors, mut warnings) = run_session_1200(&fix);

    // Add branch `b` entries forked from `main` at entry 700 (no snapshots taken on `b` yet).
    for e in &fix.branch_b {
        ancestors.insert(e.hash, e.parent);
    }
    let b_head = fix.branch_b.last().unwrap().hash;

    let selected = select_valid::<ToyKvState>(&b_head, &ancestors, &table, &mut cache, &mut warnings)
        .unwrap()
        .expect("branch b should select shared ancestor snapshot at 500");

    assert_eq!(selected.0, fix.checkpoints["main@500"]);
    assert_eq!(selected.1.digest(), fix.checkpoints["main@500"].state_digest);
    assert!(warnings.warnings.is_empty());
}

#[test]
fn ac4_read_only_cache_skips_snapshot_with_warning_and_no_row() {
    let fix = load_fixture();
    let mut state = ToyKvState::default();
    let mut policy = SnapshotPolicy::new();
    let mut cache = MemSnapshotCache::new();
    cache.set_read_only(true);
    let mut table = MemSnapshotTable::new();
    let mut warnings = VecWarningSink::new();

    for e in &fix.main[..500] {
        state.apply(&e.event).unwrap();
        let taken = policy.on_commit("main", &state, e.hash, &mut cache, &mut table, &mut warnings);
        assert!(taken.is_none());
    }

    // Commit succeeded; no SnapshotTaken row was written; one cache_write_failed warning logged.
    assert!(table.list_all().unwrap().is_empty(), "no snapshot row may be written when cache write fails");
    assert_eq!(warnings.warnings.len(), 1);
    assert_eq!(warnings.warnings[0].code(), "cache_write_failed");
    assert_eq!(warnings.warnings[0].entry_hash(), fix.main[499].hash);
}

#[test]
fn fallback_to_none_when_all_snapshots_missing_or_state_digest_mismatched() {
    let fix = load_fixture();
    let (state, mut cache, mut table, ancestors, mut warnings) = run_session_1200(&fix);
    let h1200 = fix.checkpoints["main@1200"].entry_hash;

    // Clear cache completely -> select_valid returns Ok(None) with no integrity warnings (eviction is normal).
    let mut empty_cache = MemSnapshotCache::new();
    let none = select_valid::<ToyKvState>(&h1200, &ancestors, &table, &mut empty_cache, &mut warnings).unwrap();
    assert!(none.is_none());
    assert!(warnings.warnings.is_empty());

    // Record a snapshot row whose content_digest matches the bytes, but state_digest is tampered.
    let bytes = state.encode();
    let bad_state_row = SnapshotRecord {
        entry_hash: h1200,
        format_version: 1,
        state_digest: Digest::TOMBSTONE,
        content_digest: content_digest(&bytes),
        size: bytes.len() as u64,
    };
    let mut bad_table = MemSnapshotTable::new();
    bad_table.insert(&bad_state_row).unwrap();
    let res = select_valid::<ToyKvState>(&h1200, &ancestors, &bad_table, &mut cache, &mut warnings).unwrap();
    assert!(res.is_none());
    assert!(!cache.contains(h1200, 1), "state-digest mismatch must evict cached bytes");
    assert_eq!(warnings.warnings.len(), 1);
    assert!(matches!(warnings.warnings[0], SnapshotWarning::SnapshotInvalid { entry_hash, .. } if entry_hash == h1200));

    // Direct `take` sanity check.
    let rec = take(&state, h1200, &mut cache, &mut table, &mut warnings).unwrap();
    assert_eq!(rec, fix.checkpoints["main@1200"]);
}
