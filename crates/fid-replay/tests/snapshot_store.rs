//! End-to-end integration tests of `CORE.RPL.RECON.SNAP` (AC-1..AC-4) against the real SQLite
//! `fid_store::Store` (`snapshots` and `entries` tables), filesystem `fid_store::Cache`, and
//! `fid_journal::stream::ActivityLog`.
#![cfg(feature = "store")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_journal::stream::{ActivityLog, ActivityLogConfig};
use fid_journal::Manifests;
use fid_replay::{
    content_digest, select_valid, ActivityWarningSink, DocState, EventBody, FoldError, SnapshotPolicy, SnapshotRecord,
    SnapshotTable, StoreLogicalAncestors, StoreSnapshotCache, StoreSnapshotTable,
};
use fid_store::{Cache, CacheConfig, CacheKey, Store, Value};
use fid_types::Digest;

const SESSION_JSON: &str = include_str!("../../../fixtures/core/rpl/recon/snap/session_1200.json");

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!("fid-replay-snap-{}-{}-{n}", std::process::id(), tag));
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

struct EntryRow {
    hash: Digest,
    parent: Option<Digest>,
    event: EventBody,
}

fn load_entries(val: &CValue) -> Vec<EntryRow> {
    let CValue::Arr(items) = val else { panic!("entries") };
    items
        .iter()
        .map(|it| {
            let CValue::Obj(o) = it else { panic!("entry") };
            let CValue::Str(h) = o.get("hash").unwrap() else { panic!("hash") };
            let parent = match o.get("parent") {
                Some(CValue::Str(p)) => Some(Digest::parse(p).unwrap()),
                _ => None,
            };
            let mut ev = BTreeMap::new();
            ev.insert("key".to_string(), o.get("key").unwrap().clone());
            ev.insert("value".to_string(), o.get("value").unwrap().clone());
            EntryRow { hash: Digest::parse(h).unwrap(), parent, event: CValue::Obj(ev) }
        })
        .collect()
}

fn insert_entry_rows(store: &Store, branch: &str, start_seq: i64, rows: &[EntryRow]) {
    let mut txn = store.transaction();
    let mut prev_hash: Option<String> = None;
    for (idx, e) in rows.iter().enumerate() {
        let mut hdr = BTreeMap::new();
        hdr.insert(
            "logical_parent".to_string(),
            match e.parent {
                Some(p) => CValue::Str(p.as_str().to_string()),
                None => CValue::Null,
            },
        );
        let hdr_bytes = to_canonical_bytes(&CValue::Obj(hdr)).unwrap();
        let prev_val = match &prev_hash {
            Some(p) => Value::Text(p.clone()),
            None => Value::Null,
        };
        txn.insert(
            "entries",
            &[
                ("seq", Value::Integer(start_seq + idx as i64)),
                ("hash", Value::Text(e.hash.as_str().to_string())),
                ("stream", Value::Text("record".to_string())),
                ("branch", Value::Text(branch.to_string())),
                ("kind", Value::Text("command".to_string())),
                ("prev_hash", prev_val),
                ("header", Value::Blob(hdr_bytes)),
            ],
        );
        prev_hash = Some(e.hash.as_str().to_string());
    }
    txn.commit().unwrap();
}

#[test]
fn store_end_to_end_ac1_ac2_ac3_ac4() {
    let doc = parse(SESSION_JSON.as_bytes()).unwrap();
    let CValue::Obj(root) = doc else { panic!("root") };
    let main_rows = load_entries(root.get("main").unwrap());
    let branch_b_rows = load_entries(root.get("branch_b").unwrap());
    let CValue::Obj(cp_obj) = root.get("checkpoints").unwrap() else { panic!("checkpoints") };
    let expected_cp = |name: &str| -> SnapshotRecord {
        let CValue::Obj(o) = cp_obj.get(name).unwrap() else { panic!("cp") };
        let s = |k: &str| match o.get(k).unwrap() {
            CValue::Str(x) => x.as_str(),
            _ => panic!("str {k}"),
        };
        let n = |k: &str| match o.get(k).unwrap() {
            CValue::Num(x) => *x as u64,
            _ => panic!("num {k}"),
        };
        SnapshotRecord {
            entry_hash: Digest::parse(s("entry_hash")).unwrap(),
            format_version: n("format_version") as u32,
            state_digest: Digest::parse(s("state_digest")).unwrap(),
            content_digest: Digest::parse(s("content_digest")).unwrap(),
            size: n("size"),
        }
    };

    let tmp = temp_dir("e2e");
    let proj_dir = tmp.join("proj");
    let cache_dir = tmp.join("cache");
    let alog_dir = tmp.join("alog");
    std::fs::create_dir_all(&cache_dir).unwrap();
    std::fs::create_dir_all(&alog_dir).unwrap();

    let store = Store::create(&proj_dir).unwrap();
    insert_entry_rows(&store, "main", 1, &main_rows);
    insert_entry_rows(&store, "b", 1201, &branch_b_rows);

    let mut raw_cache = Cache::open(&cache_dir, CacheConfig::default());
    let manifests = Manifests {
        registry: fid_canon::Digest(Digest::TOMBSTONE.to_bytes()),
        kernel: fid_canon::Digest(Digest::TOMBSTONE.to_bytes()),
    };
    let alog = ActivityLog::new(&alog_dir, ActivityLogConfig::default(), manifests);
    let mut warnings = ActivityWarningSink::new(&alog, manifests, "2026-11-21T00:00:00Z");

    // --- AC-1: commit 1,200 entries on main and close -> snapshots at 500, 1000, 1200 ---
    let mut state = ToyKvState::default();
    let mut policy = SnapshotPolicy::new();
    {
        let mut cache = StoreSnapshotCache::new(&mut raw_cache);
        let mut table = StoreSnapshotTable::new(&store);
        for e in &main_rows {
            state.apply(&e.event).unwrap();
            policy.on_commit("main", &state, e.hash, &mut cache, &mut table, &mut warnings);
        }
        policy.on_close("main", &state, main_rows.last().unwrap().hash, &mut cache, &mut table, &mut warnings);
    }

    let table = StoreSnapshotTable::new(&store);
    let ancestors = StoreLogicalAncestors::new(&store);
    let recorded = table.list_all().unwrap();
    assert_eq!(recorded.len(), 3);
    assert_eq!(recorded[0], expected_cp("main@500"));
    assert_eq!(recorded[1], expected_cp("main@1000"));
    assert_eq!(recorded[2], expected_cp("main@1200"));
    assert!(warnings.warnings().is_empty());

    // --- AC-3: branch `b` forked at 700 ignores main's 1000 and returns 500 ---
    let b_head = branch_b_rows.last().unwrap().hash;
    {
        let mut cache = StoreSnapshotCache::new(&mut raw_cache);
        let sel = select_valid::<ToyKvState>(&b_head, &ancestors, &table, &mut cache, &mut warnings)
            .unwrap()
            .expect("branch b must select snapshot at 500");
        assert_eq!(sel.0, expected_cp("main@500"));
    }

    // --- AC-2: evict 1200, flip one byte in 1000's cache file on disk -> selects 500, file deleted, ActivityLog records warning naming 1000 ---
    let h1200 = expected_cp("main@1200").entry_hash;
    let h1000 = expected_cp("main@1000").entry_hash;
    raw_cache.remove(&CacheKey::Snapshot { entry_hash: h1200, format: 1 });
    let item_1000_path = raw_cache.path_of(&CacheKey::Snapshot { entry_hash: h1000, format: 1 }).unwrap();
    assert!(item_1000_path.is_file(), "snapshot 1000 file must exist before bit flip");
    let mut on_disk = std::fs::read(&item_1000_path).unwrap();
    let last = on_disk.len() - 1;
    on_disk[last] ^= 0x01;
    std::fs::write(&item_1000_path, &on_disk).unwrap();

    {
        let mut cache = StoreSnapshotCache::new(&mut raw_cache);
        let sel = select_valid::<ToyKvState>(&h1200, &ancestors, &table, &mut cache, &mut warnings)
            .unwrap()
            .expect("must fall back to snapshot at 500");
        assert_eq!(sel.0, expected_cp("main@500"));
        assert_eq!(sel.1.digest(), expected_cp("main@500").state_digest);
    }
    assert!(!item_1000_path.exists(), "corrupted snapshot 1000 file must be evicted from cache");
    assert_eq!(warnings.warnings().len(), 1);
    assert_eq!(warnings.warnings()[0].code(), "snapshot_invalid");
    assert_eq!(warnings.warnings()[0].entry_hash(), h1000);

    // Verify the Activity Log file on disk actually holds the integrity warning naming entry 1000's hash!
    alog.close().unwrap();
    let alog_text = std::fs::read_to_string(alog_dir.join(fid_journal::stream::CURRENT_FILE)).unwrap();
    assert!(
        alog_text.contains(h1000.as_str()) && alog_text.contains(fid_replay::ACTIVITY_WARNING_COMMAND),
        "Activity Log must record integrity warning naming entry 1000: {alog_text}"
    );

    // --- AC-4: disabled / read-only cache fails write -> commit succeeds, no row written, warning logged ---
    let mut disabled = Cache::disabled();
    let before_count = table.list_all().unwrap().len();
    {
        let mut cache = StoreSnapshotCache::new(&mut disabled);
        let mut table_mut = StoreSnapshotTable::new(&store);
        let taken = fid_replay::take(&state, main_rows[10].hash, &mut cache, &mut table_mut, &mut warnings);
        assert!(taken.is_none());
    }
    assert_eq!(table.list_all().unwrap().len(), before_count, "no row written on failed cache put");
    assert_eq!(warnings.warnings().last().unwrap().code(), "cache_write_failed");

    let _ = std::fs::remove_dir_all(&tmp);
}
