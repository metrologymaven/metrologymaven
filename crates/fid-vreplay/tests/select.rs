//! TRUST.REPLAY.SELECT AC-1 .. AC-5 and REQ-001 .. REQ-005 over `fixtures/replay/select/`.
//!
//! Every package, expected plan and expected error comes from the independent reference
//! `fixtures/replay/select/make_cases.py` (its own statement of the rules plus hand labels),
//! never from this crate. The packages are opened with the real package reader
//! (`fid_pkg::reader::PackageReader`), adapted to `select::PackageView`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::Cursor;

use fid_canon::CValue;
use fid_pkg::reader::PackageReader;
use fid_types::Digest;
use fid_vreplay::compare::ResultInput;
use fid_vreplay::select::{
    plan, plan_with, ArtifactSource, Limits, MemberInfo, PackageError, PackageView, Reason, Selected, Selection,
};

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/replay/select");

struct Reader(PackageReader<Cursor<Vec<u8>>>);

impl PackageView for Reader {
    fn evidence_tier(&self) -> String {
        self.0.manifest().evidence_tier.as_str().to_string()
    }
    fn kernel_manifest_digest(&self) -> Digest {
        self.0.manifest().kernel_manifest_digest
    }
    fn members(&self) -> Vec<MemberInfo> {
        self.0
            .manifest()
            .members
            .iter()
            .map(|m| MemberInfo { path: m.path.as_str().to_string(), sha256: m.sha256 })
            .collect()
    }
    fn read_member(&mut self, path: &str, max_bytes: u64) -> Result<Option<Vec<u8>>, PackageError> {
        self.0
            .read_member(path, max_bytes)
            .map_err(|e| PackageError { code: e.code().to_string(), detail: e.to_string() })
    }
}

struct Available(bool);

impl ArtifactSource for Available {
    fn is_available(&self, _: &Digest) -> bool {
        self.0
    }
}

fn open(rel: &str) -> Reader {
    let path = format!("{FIX}/{rel}");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    Reader(PackageReader::open(Cursor::new(bytes)).unwrap_or_else(|e| panic!("{path}: {e}")))
}

fn expected(rel: &str) -> Vec<u8> {
    let path = format!("{FIX}/{rel}");
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn select(case: &str) -> Selection {
    plan(&mut open(&format!("{case}.zip")), &Available(true)).unwrap_or_else(|e| panic!("{case}: {e}"))
}

/// The selection must equal the reference's expected plan, byte for byte.
fn check(case: &str, variant: Option<&str>, sel: &Selection) {
    let name = match variant {
        Some(v) => format!("{case}.{v}.plan.json"),
        None => format!("{case}.plan.json"),
    };
    let want = expected(&name);
    let got = sel.canonical_json();
    assert_eq!(String::from_utf8_lossy(&got), String::from_utf8_lossy(&want), "{name}");
}

fn reason_of(sel: &Selection, id: &str) -> Option<Reason> {
    sel.results.iter().find(|r| r.record_id() == id).and_then(|r| match r {
        Selected::NotReplayable(n) => Some(n.reason),
        Selected::Replayable { .. } => None,
    })
}

fn entry_indices(sel: &Selection, id: &str) -> Vec<usize> {
    let (_, calls) = sel.plan.results.iter().find(|(r, _)| r == id).unwrap_or_else(|| panic!("{id} not planned"));
    calls.iter().map(|&k| sel.plan.calls[k].entry_index).collect()
}

#[test]
fn ac1_points_all_replayable_with_the_expected_plan() {
    let sel = select("points");
    check("points", None, &sel);
    assert_eq!(sel.plan.results.len(), 3);
    assert_eq!(sel.not_replayable().count(), 0);
    assert_eq!(sel.plan.calls.len(), 3);
    for (k, c) in sel.plan.calls.iter().enumerate() {
        assert_eq!(c.algorithm_id, "fit.circle.ls@1.0.0");
        assert_eq!(c.seed, Some(0));
        assert_eq!(c.inputs.len(), 1);
        assert_eq!(c.params_bytes(), br#"{"method":"ls"}"#.to_vec());
        assert_eq!(sel.plan.results[k].1, vec![k]);
    }
}

#[test]
fn ac2_results_only_package() {
    let sel = select("results");
    check("results", None, &sel);
    assert!(sel.plan.calls.is_empty());
    assert_eq!(sel.results.len(), 3);
    for r in &sel.results {
        assert_eq!(reason_of(&sel, r.record_id()), Some(Reason::BasisResultsOnly));
    }
}

#[test]
fn ac3_missing_point_set() {
    let sel = select("missing-points");
    check("missing-points", None, &sel);
    assert_eq!(reason_of(&sel, "hole-2"), Some(Reason::PointsMissing));
    assert_eq!(reason_of(&sel, "hole-1"), None);
    assert_eq!(reason_of(&sel, "hole-3"), None);
    assert_eq!(sel.plan.calls.len(), 2);
}

#[test]
fn ac4_constructed_feature_plans_both_fits_once_before_the_distance() {
    let sel = select("constructed");
    check("constructed", None, &sel);
    let algs: Vec<&str> = sel.plan.calls.iter().map(|c| c.algorithm_id.as_str()).collect();
    assert_eq!(algs, ["fit.circle.ls@1.0.0", "fit.circle.ls@1.0.0", "construct.distance.pair@1.0.0"]);
    assert_eq!(entry_indices(&sel, "distance-ab"), [3, 5, 6]);
    assert_eq!(entry_indices(&sel, "circle-a"), [3]);
    assert_eq!(entry_indices(&sel, "circle-b"), [5]);
    // REQ-004: journal order, each call once.
    let idx: Vec<usize> = sel.plan.calls.iter().map(|c| c.entry_index).collect();
    assert!(idx.windows(2).all(|w| w[0] < w[1]));
}

#[test]
fn ac5_unknown_algorithm() {
    let sel = select("unknown-algo");
    check("unknown-algo", None, &sel);
    assert_eq!(reason_of(&sel, "hole-1"), Some(Reason::AlgorithmUnknown));
    assert_eq!(reason_of(&sel, "hole-2"), None);
}

#[test]
fn req001_every_reason_and_detail() {
    let sel = select("reasons");
    check("reasons", None, &sel);
    let seen: std::collections::BTreeSet<Reason> = sel.not_replayable().map(|n| n.reason).collect();
    for r in Reason::ALL {
        if r != Reason::PlanTooLarge {
            assert!(seen.contains(&r), "{r} not exercised");
        }
    }
    // REQ-003: a non-kernel step whose output is packaged is substituted.
    assert_eq!(reason_of(&sel, "substituted-non-kernel"), None);
    // Kernel outputs are recomputed even when packaged.
    assert_eq!(entry_indices(&sel, "kernel-output-recomputed"), [2, 16, 17]);
}

#[test]
fn kernel_artifact_unavailable() {
    let sel = plan(&mut open("points.zip"), &Available(false)).unwrap();
    check("points", Some("unavailable"), &sel);
    assert!(sel.plan.calls.is_empty());
}

#[test]
fn req005_plan_too_large_and_work_budget() {
    let small = |max_plan_calls| Limits { max_plan_calls, ..Limits::default() };
    let sel = plan_with(&mut open("constructed.zip"), &Available(true), small(2)).unwrap();
    check("constructed", Some("limit2"), &sel);
    assert_eq!(reason_of(&sel, "distance-ab"), Some(Reason::PlanTooLarge));
    let sel = plan_with(&mut open("constructed.zip"), &Available(true), small(3)).unwrap();
    check("constructed", Some("limit3"), &sel);
    let sel =
        plan_with(&mut open("points.zip"), &Available(true), Limits { work_budget: 0, ..Limits::default() }).unwrap();
    check("points", Some("budget0"), &sel);
    assert_eq!(Limits::default().max_plan_calls, 100_000);
}

#[test]
fn malformed_packages_fail_closed() {
    let CValue::Arr(cases) = fid_canon::parse(&expected("malformed.json")).unwrap() else { panic!("list") };
    assert!(!cases.is_empty());
    for c in &cases {
        let CValue::Obj(o) = c else { panic!("case") };
        let s = |k: &str| match o.get(k) {
            Some(CValue::Str(s)) => s.clone(),
            other => panic!("{k}: {other:?}"),
        };
        let name = s("name");
        let err = plan(&mut open(&format!("malformed/{name}.zip")), &Available(true))
            .expect_err(&format!("{name}: accepted"));
        assert_eq!(err.code(), s("code"), "{name}: {err}");
        assert_eq!(err.reason(), Some(s("reason").as_str()), "{name}: {err}");
    }
}

#[test]
fn not_replayable_feeds_compare_with_the_select_code() {
    let sel = select("missing-points");
    let n = sel.not_replayable().next().unwrap();
    match ResultInput::from(n) {
        ResultInput::NotReplayable { record_id, reason } => {
            assert_eq!(record_id, "hole-2");
            assert_eq!(reason, "POINTS_MISSING");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn reader_errors_pass_through() {
    struct Broken;
    impl PackageView for Broken {
        fn evidence_tier(&self) -> String {
            "points".into()
        }
        fn kernel_manifest_digest(&self) -> Digest {
            Digest::TOMBSTONE
        }
        fn members(&self) -> Vec<MemberInfo> {
            vec![MemberInfo { path: "results/records.json".into(), sha256: Digest::TOMBSTONE }]
        }
        fn read_member(&mut self, _: &str, _: u64) -> Result<Option<Vec<u8>>, PackageError> {
            Err(PackageError { code: "pkg.digest_mismatch".into(), detail: "test".into() })
        }
    }
    let err = plan(&mut Broken, &Available(true)).unwrap_err();
    assert_eq!(err.code(), "pkg.digest_mismatch");
    assert_eq!(err.reason(), None);

    struct Tier(&'static str);
    impl PackageView for Tier {
        fn evidence_tier(&self) -> String {
            self.0.into()
        }
        fn kernel_manifest_digest(&self) -> Digest {
            Digest::TOMBSTONE
        }
        fn members(&self) -> Vec<MemberInfo> {
            Vec::new()
        }
        fn read_member(&mut self, _: &str, _: u64) -> Result<Option<Vec<u8>>, PackageError> {
            Ok(None)
        }
    }
    let err = plan(&mut Tier("everything"), &Available(true)).unwrap_err();
    assert_eq!(err.reason(), Some("evidence_tier"));
    let err = plan(&mut Tier("points"), &Available(true)).unwrap_err();
    assert_eq!(err.reason(), Some("records_missing"));
}
