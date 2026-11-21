//! ALN.QA.HIST recorder tests. The trace `fixtures/aln/qa/hist-manifest/trace.txt` is
//! hand-authored; the expected rows and digest come from the independent Python reference
//! `fixtures/aln/qa/hist-manifest/make_expected.py`, never from running this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_align_solve::{
    validate, verify_locks, ConstraintSet, DofLocks, FrameContext, HistError, HistoryRecorder, IterationRow,
    LockFrameRef, Outcome, StoppedBy, WeightSpec, MAX_ITERATIONS,
};
use fid_geo_frame::{FrameId, FrameTree, Rigid};
use fid_types::Digest;

fn read(name: &str) -> String {
    std::fs::read_to_string(format!("{}/fixtures/aln/qa/hist-manifest/{name}", fid_testkit::WORKSPACE_ROOT)).unwrap()
}

/// A constraint set without locks and the certified identity pose for it.
fn certified() -> (ConstraintSet, fid_align_solve::CertifiedPose) {
    let t = FrameTree::new();
    let ctx = FrameContext { tree: &t, parent: FrameId::WORLD };
    let cs =
        validate(DofLocks::NONE, LockFrameRef::Parent, &ctx, &Rigid::IDENTITY, &WeightSpec::default(), 118).unwrap();
    let pose = verify_locks(&cs, &Rigid::IDENTITY).unwrap();
    (cs, pose)
}

fn trace() -> (Vec<IterationRow>, StoppedBy) {
    let (mut rows, mut stop) = (Vec::new(), None);
    for line in read("trace.txt").lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        let f: Vec<&str> = line.split_whitespace().collect();
        match f[0] {
            "iter" => rows.push(IterationRow {
                iteration: f[1].parse().unwrap(),
                objective: f[2].parse().unwrap(),
                d_translation: f[3].parse().unwrap(),
                d_rotation: f[4].parse().unwrap(),
                n_used: f[5].parse().unwrap(),
            }),
            "stop" => {
                assert_eq!(f[1], "converged");
                stop = Some(StoppedBy::new(Outcome::Converged, f[2]).unwrap());
            }
            other => panic!("unknown line {other}"),
        }
    }
    (rows, stop.unwrap())
}

/// AC-2: 12 scripted iterations ending in converged(increment): 12 rows with the given
/// values and the stopping criterion; digest against the reference.
#[test]
fn ac2_twelve_iterations() {
    let (rows, stop) = trace();
    let (cs, pose) = certified();
    let mut rec = HistoryRecorder::new();
    for r in &rows {
        rec.record(*r).unwrap();
    }
    let h = rec.finish(stop, &pose);
    assert_eq!(h.len(), 12);
    assert_eq!(h.stopped_by().to_string(), "converged(increment)");
    assert!(h.final_pose().is_for(&cs));
    assert_eq!(h.final_pose(), &pose);
    let want = read("expected-history.txt");
    let mut n = 0;
    for line in want.lines().filter(|l| l.starts_with("row ")) {
        let f: Vec<&str> = line.split(' ').collect();
        let i: usize = f[1].parse().unwrap();
        let bits = |s: &str| u64::from_str_radix(s.trim_start_matches("0x"), 16).unwrap();
        let row = h.row(i - 1).unwrap();
        assert_eq!(row.iteration, i as u64);
        assert_eq!(row.objective.to_bits(), bits(f[2]), "row {i} objective");
        assert_eq!(row.d_translation.to_bits(), bits(f[3]), "row {i} d_translation");
        assert_eq!(row.d_rotation.to_bits(), bits(f[4]), "row {i} d_rotation");
        assert_eq!(row.n_used.to_string(), f[5], "row {i} n_used");
        assert_eq!(row, rows[i - 1], "row {i} as given");
        n += 1;
    }
    assert_eq!(n, 12);
    assert_eq!(h.iterations(), &(1..=12).collect::<Vec<u64>>()[..]);
    assert_eq!(h.objectives().len(), 12);
    assert_eq!((h.d_translations().len(), h.d_rotations().len(), h.n_used().len()), (12, 12, 12));
    let stopped = want.lines().find_map(|l| l.strip_prefix("stopped_by ")).unwrap();
    assert_eq!(h.stopped_by().to_string(), stopped);
    let digest = want.lines().find_map(|l| l.strip_prefix("digest ")).unwrap();
    assert_eq!(h.digest(), Digest::parse(digest).unwrap());
    // D0: the same rows give the same digest; another criterion another one.
    let mut again = HistoryRecorder::new();
    for r in &rows {
        again.record(*r).unwrap();
    }
    assert_eq!(again.clone().finish(h.stopped_by().clone(), &pose).digest(), h.digest());
    let limit = StoppedBy::new(Outcome::Limit, "increment").unwrap();
    assert_ne!(again.finish(limit, &pose).digest(), h.digest());
}

/// AC-3: the 100,001st iteration fails with `history_overflow`; the first 100,000 are kept.
#[test]
fn ac3_overflow() {
    let (_, pose) = certified();
    let mut rec = HistoryRecorder::new();
    let row = |i: u64| IterationRow { iteration: i, objective: 1.0, d_translation: 0.0, d_rotation: 0.0, n_used: 3 };
    for i in 1..=MAX_ITERATIONS as u64 {
        rec.record(row(i)).unwrap();
    }
    assert_eq!(MAX_ITERATIONS, 100_000);
    let e = rec.record(row(100_001)).unwrap_err();
    assert_eq!(e, HistError::Overflow);
    assert_eq!(e.code(), "history_overflow");
    assert_eq!(rec.len(), 100_000);
    let h = rec.finish(StoppedBy::new(Outcome::Limit, "max_iterations").unwrap(), &pose);
    assert_eq!(h.len(), 100_000);
    assert_eq!(h.row(99_999).unwrap().iteration, 100_000);
    assert_eq!(h.row(100_000), None);
}

/// A run that met a criterion at its start has an empty history.
#[test]
fn empty_history() {
    let (_, pose) = certified();
    let h = HistoryRecorder::new().finish(StoppedBy::new(Outcome::Converged, "objective").unwrap(), &pose);
    assert!(h.is_empty());
    assert_eq!(h.row(0), None);
}
