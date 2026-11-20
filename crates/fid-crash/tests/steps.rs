//! Deterministic fault injection at every commit step (fid-store's commit probe, reached only
//! through the `fault-inject` feature): an injected error at each step, and a kill of the
//! writer process at each step. The expected recovered state of every case comes from the
//! oracle's step table (`fixtures/core/sto/sql/crash/chain.json`, written by make_chain.py).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::*;
use fid_crash::adapter::{CrashAdapter, StepProbe, StoreAdapter, WriterMode};
use fid_crash::{verify_store, Ack, CheckLimits, Writer};
use std::sync::Arc;

struct Step {
    step: u64,
    commit: u64,
    event: String,
    kill_rows: u64,
    fault_failed: bool,
    fault_rows: u64,
}

fn steps() -> (u64, Vec<Step>) {
    let fx = fixture();
    let m = num(field(&fx, "step_commits"));
    let steps = arr(field(&fx, "steps"))
        .iter()
        .map(|s| Step {
            step: num(field(s, "step")),
            commit: num(field(s, "commit")),
            event: text(field(s, "event")).to_string(),
            kill_rows: num(field(s, "kill_rows")),
            fault_failed: boolean(field(s, "fault_failed")),
            fault_rows: num(field(s, "fault_rows")),
        })
        .collect();
    (m, steps)
}

fn check(root: &std::path::Path, acked: &[u64]) -> u64 {
    verify_store(root, acked, CheckLimits::default(), true).unwrap_or_else(|f| panic!("check failed: {f}"))
}

/// An injected I/O error at each commit step fails that commit with nothing visible (except at
/// `Committed`, which comes after COMMIT); the store reopens with exactly the commits before
/// it, and the writer then retries and completes the chain.
#[test]
fn fault_at_every_step_writes_nothing_and_the_store_recovers() {
    let (m, steps) = steps();
    assert!(steps.len() as u64 > 3 * m);
    for s in &steps {
        let t = TempDir::new("fault");
        let root = t.0.join("p");
        let mut a = StoreAdapter::new(WriterMode::Correct);
        a.create(&root).unwrap();
        let probe = Arc::new(StepProbe::new(None, Some(s.step)));
        a.set_probe(probe.clone());
        assert_eq!(a.open_writer(&root).unwrap(), 1);
        let mut sink = Vec::new();
        let mut acked = Vec::new();
        let mut failed = None;
        let mut seq = 1;
        while seq <= m {
            let mut ack = Ack::new(&mut sink);
            match a.commit_next(seq, &mut ack) {
                Ok(()) => {
                    acked.push(seq);
                    seq += 1;
                }
                Err(e) => {
                    assert!(failed.is_none(), "step {}: a second failure: {e}", s.step);
                    assert!(e.contains("injected fault"), "step {}: {e}", s.step);
                    failed = Some(seq);
                    // Nothing of the failed commit is visible after a reopen.
                    a.close();
                    assert_eq!(check(&root, &acked), seq - 1, "step {} ({})", s.step, s.event);
                    assert_eq!(a.open_writer(&root).unwrap(), seq);
                }
            }
        }
        assert_eq!(failed.is_some(), s.fault_failed, "step {} ({})", s.step, s.event);
        if let Some(c) = failed {
            assert_eq!(c, s.commit, "step {}", s.step);
        }
        a.close();
        assert_eq!(check(&root, &acked), s.fault_rows, "step {} ({})", s.step, s.event);
        assert!(probe.steps() >= steps.len() as u64);
    }
}

/// The writer process is killed exactly at each commit step (it hangs there and is killed
/// with SIGKILL / TerminateProcess): the reopened store holds exactly the oracle's rows, every
/// acknowledged commit among them.
#[test]
fn kill_at_every_step_recovers_the_oracle_state() {
    let (m, steps) = steps();
    for s in &steps {
        let t = TempDir::new("kill");
        let root = t.0.join("p");
        StoreAdapter::new(WriterMode::Correct).create(&root).unwrap();
        let cmd = writer("correct", &["--max-commits", &m.to_string(), "--hang-at", &s.step.to_string()]);
        let mut w = Writer::spawn(&cmd, &root).unwrap();
        let hang = w.wait_for("HANG").unwrap();
        assert!(hang.contains(&s.event), "step {}: {hang} is not {}", s.step, s.event);
        let (acks, _) = w.kill().unwrap();
        assert_eq!(acks, (1..s.commit).collect::<Vec<_>>(), "step {}", s.step);
        assert_eq!(check(&root, &acks), s.kill_rows, "step {} ({})", s.step, s.event);
        // The store stays usable: a new writer continues the chain from there.
        let mut a = StoreAdapter::new(WriterMode::Correct);
        assert_eq!(a.open_writer(&root).unwrap(), s.kill_rows + 1);
        let mut sink = Vec::new();
        a.commit_next(s.kill_rows + 1, &mut Ack::new(&mut sink)).unwrap();
        a.close();
        assert_eq!(check(&root, &[s.kill_rows + 1]), s.kill_rows + 1);
    }
}
