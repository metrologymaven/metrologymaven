//! Acceptance criteria of spec leaf HAL.ACQ.INGEST.CREDIT (AC-1 .. AC-5) plus hostile inputs.
//! The event scripts, their expected traces and the AC-5 trace digest are written by the
//! independent reference `fixtures/hal/acq/ingest/credit/make_fixtures.py`; the hostile-input
//! cases are hand-labelled from the leaf's rules.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_hal_idl::stamp::{Gap, GapReason, SeqRange};
use fid_hal_runtime::ingest::credit::{
    Admit, Credit, CreditError, CreditLedger, CreditWindow, StreamState, CREDIT_EXCEEDED, DEFAULT_WINDOW_BATCHES,
    DEFAULT_WINDOW_BYTES,
};
use fid_num::rng::{Address, Cursor};
use fid_testkit::Sha256Hasher;
use std::collections::BTreeMap;

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/hal/acq/ingest/credit/");
const MB: u64 = 1 << 20;

fn read(name: &str) -> String {
    std::fs::read_to_string(format!("{FIX}{name}")).unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[derive(Debug, Clone, Copy)]
enum Event {
    Grant,
    Admit(u64, u64, u64),
    Ack(u64),
    BeginStop,
    FinishStop,
}

struct Script {
    window: CreditWindow,
    events: Vec<Event>,
    expected: Vec<String>,
}

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        _ => panic!("not an object"),
    }
}

fn arr(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(a) => a,
        _ => panic!("not an array"),
    }
}

fn int(v: &CValue) -> u64 {
    match v {
        CValue::Num(f) if f.fract() == 0.0 && (0.0..=9_007_199_254_740_992.0).contains(f) => *f as u64,
        _ => panic!("not an exact unsigned integer: {v:?}"),
    }
}

fn text(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        _ => panic!("not a string"),
    }
}

fn script(name: &str) -> Script {
    let doc = fid_canon::parse(read(name).as_bytes()).unwrap();
    let o = obj(&doc);
    let w = obj(&o["window"]);
    let window = CreditWindow { bytes: int(&w["bytes"]), batches: u32::try_from(int(&w["batches"])).unwrap() };
    let events = arr(&o["events"])
        .iter()
        .map(|e| {
            let e = arr(e);
            match (text(&e[0]), e.len()) {
                ("grant", 1) => Event::Grant,
                ("admit", 4) => Event::Admit(int(&e[1]), int(&e[2]), int(&e[3])),
                ("ack", 2) => Event::Ack(int(&e[1])),
                ("begin_stop", 1) => Event::BeginStop,
                ("finish_stop", 1) => Event::FinishStop,
                (op, n) => panic!("bad event {op}/{n}"),
            }
        })
        .collect();
    let expected = arr(&o["expected"]).iter().map(|l| text(l).to_string()).collect();
    Script { window, events, expected }
}

fn pair(c: Credit) -> String {
    format!("{}/{}", c.bytes, c.batches)
}

/// Applies one event and renders the trace line in the reference format.
fn step(l: &mut CreditLedger, ev: Event) -> String {
    let (op, result) = match ev {
        Event::Grant => ("grant", format!("granted={}", pair(l.grant()))),
        Event::Admit(b, first, last) => {
            let r = match l.admit(b, SeqRange { first, last }).unwrap() {
                Admit::Accepted => "accepted".to_string(),
                a @ Admit::Refused(g) => {
                    format!("refused {} gap={}-{}:{}", a.code().unwrap(), g.from, g.to, g.reason)
                }
            };
            ("admit", r)
        }
        Event::Ack(b) => ("ack", format!("returned={}", pair(l.ack(b)))),
        Event::BeginStop => {
            l.begin_stop();
            ("begin_stop", "ok".to_string())
        }
        Event::FinishStop => {
            l.finish_stop();
            ("finish_stop", "ok".to_string())
        }
    };
    let (w, o, f) = (l.window(), l.outstanding(), l.in_flight());
    // The ledger's invariant, checked on every step of every script.
    assert!(o.bytes.checked_add(f.bytes).unwrap() <= w.bytes, "bytes above window");
    assert!(o.batches.checked_add(f.batches).unwrap() <= w.batches, "batches above window");
    format!("{op} {result} {} out={} fly={}", l.state(), pair(o), pair(f))
}

fn replay(s: &Script) -> Vec<String> {
    let mut l = CreditLedger::new(s.window).unwrap();
    s.events.iter().map(|&e| step(&mut l, e)).collect()
}

fn assert_matches(name: &str) -> Script {
    let s = script(name);
    let got = replay(&s);
    assert_eq!(got.len(), s.expected.len(), "{name}: trace length");
    for (i, (g, e)) in got.iter().zip(&s.expected).enumerate() {
        assert_eq!(g, e, "{name}: event {i}");
    }
    s
}

#[test]
fn ac1_steady_flow_all_accepted() {
    let s = assert_matches("steady.json");
    assert_eq!(s.window, CreditWindow { bytes: 8 * MB, batches: 4 });
    let got = replay(&s);
    assert_eq!(got.iter().filter(|l| l.starts_with("admit accepted")).count(), 24);
    assert!(got.iter().all(|l| !l.contains("refused")));
}

#[test]
fn ac2_overrun_refused_with_gap() {
    assert_matches("overrun.json");
    // The same through the API: the fifth batch has bytes but no batch credit.
    let mut l = CreditLedger::new(CreditWindow { bytes: 8 * MB, batches: 4 }).unwrap();
    l.grant();
    for i in 0..4 {
        assert_eq!(l.admit(MB, SeqRange { first: 100 * i, last: 100 * i + 99 }).unwrap(), Admit::Accepted);
    }
    let before = l.clone();
    let a = l.admit(MB, SeqRange { first: 400, last: 499 }).unwrap();
    assert_eq!(a, Admit::Refused(Gap { from: 400, to: 499, reason: GapReason::CreditExhausted }));
    assert_eq!(a.code(), Some(CREDIT_EXCEEDED));
    assert_eq!(CREDIT_EXCEEDED, "credit_exceeded");
    assert_eq!(l, before, "a refusal changes nothing");
}

#[test]
fn req1_byte_unit_enforced() {
    assert_matches("bytes_overrun.json");
}

#[test]
fn ac3_acks_capped_at_window() {
    assert_matches("ack_cap.json");
    // u64::MAX acknowledgement (not representable in the JSON fixture): still exactly the window.
    let w = CreditWindow { bytes: 8 * MB, batches: 4 };
    let mut l = CreditLedger::new(w).unwrap();
    l.grant();
    l.admit(3 * MB, SeqRange { first: 0, last: 0 }).unwrap();
    assert_eq!(l.ack(u64::MAX), Credit { bytes: 3 * MB, batches: 1 });
    assert_eq!(l.ack(u64::MAX), Credit::ZERO);
    assert_eq!(l.outstanding(), Credit { bytes: w.bytes, batches: w.batches });
    assert_eq!(l.in_flight(), Credit::ZERO);
}

#[test]
fn ac4_no_grant_while_stopping_or_stopped() {
    assert_matches("stopping.json");
    let mut l = CreditLedger::new(CreditWindow { bytes: 8 * MB, batches: 4 }).unwrap();
    l.begin_stop();
    assert_eq!(l.state(), StreamState::Stopping);
    assert_eq!(l.grant(), Credit::ZERO, "no credit while stopping");
    assert_eq!(l.outstanding(), Credit::ZERO);
    l.finish_stop();
    assert_eq!(l.state(), StreamState::Stopped);
    l.begin_stop();
    assert_eq!(l.state(), StreamState::Stopped, "stopped is terminal");
    assert_eq!(l.grant(), Credit::ZERO, "no credit when stopped");
}

/// AC-5: the generator of make_fixtures.py on KRN.NUM.RNG (seed 11, stream = script index).
fn gen_script(s: u64) -> (CreditWindow, Vec<Event>) {
    let mut c = Cursor::new(Address { seed: 11, stream: s }, 0);
    let mut below = |n: u64| c.below(n).unwrap();
    let wb = 1 + below(1 << 20);
    let wn = 1 + below(8);
    let n = 1 + below(48);
    let mut next = 0u64;
    let mut ev = Vec::new();
    for _ in 0..n {
        let k = below(100);
        if k < 40 {
            let b = 1 + below(wb);
            let len = 1 + below(16);
            ev.push(Event::Admit(b, next, next + len - 1));
            next += len;
        } else if k < 70 {
            ev.push(Event::Ack(1 + below(wb)));
        } else if k < 90 {
            ev.push(Event::Grant);
        } else if k < 96 {
            ev.push(Event::BeginStop);
        } else {
            ev.push(Event::FinishStop);
        }
    }
    (CreditWindow { bytes: wb, batches: u32::try_from(wn).unwrap() }, ev)
}

#[test]
fn ac5_seeded_scripts_replay_identically() {
    let golden: BTreeMap<String, String> = read("property-v1.txt")
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let (k, v) = l.split_once(' ').unwrap();
            (k.to_string(), v.to_string())
        })
        .collect();
    assert_eq!(golden["seed"], "11");
    let n: u64 = golden["scripts"].parse().unwrap();
    assert_eq!(n, 10_000);
    let mut h = Sha256Hasher::new();
    for s in 0..n {
        let (window, events) = gen_script(s);
        let script = Script { window, events, expected: Vec::new() };
        let first = replay(&script);
        let second = replay(&script);
        assert_eq!(first, second, "script {s}: traces differ between runs");
        for (i, line) in first.iter().enumerate() {
            h.update(format!("{s} {i} {line}\n").as_bytes());
        }
    }
    assert_eq!(h.finish(), golden["sha256"], "traces differ from the reference model");
}

#[test]
fn default_window() {
    let w = CreditWindow::default();
    assert_eq!(w, CreditWindow { bytes: DEFAULT_WINDOW_BYTES, batches: DEFAULT_WINDOW_BATCHES });
    assert_eq!((w.bytes, w.batches), (64 * MB, 16));
    let l = CreditLedger::new(w).unwrap();
    assert_eq!(l.state(), StreamState::Active);
    assert_eq!(l.outstanding(), Credit::ZERO, "no credit before the first grant");
}

#[test]
fn hostile_inputs() {
    for w in [CreditWindow { bytes: 0, batches: 4 }, CreditWindow { bytes: 1, batches: 0 }] {
        let e = CreditLedger::new(w).unwrap_err();
        assert_eq!(e, CreditError::InvalidWindow { window: w });
        assert_eq!(e.code(), "credit_invalid_window");
    }
    let mut l = CreditLedger::new(CreditWindow { bytes: u64::MAX, batches: u32::MAX }).unwrap();
    // Before any grant nothing is admitted, not even an empty batch.
    assert!(matches!(l.admit(0, SeqRange { first: 0, last: 0 }).unwrap(), Admit::Refused(_)));
    assert_eq!(l.grant(), Credit { bytes: u64::MAX, batches: u32::MAX });
    assert_eq!(l.grant(), Credit::ZERO, "a second grant adds nothing");
    let r = SeqRange { first: 5, last: 4 };
    let before = l.clone();
    let e = l.admit(1, r).unwrap_err();
    assert_eq!(e, CreditError::InvalidRange { range: r });
    assert_eq!(e.code(), "credit_invalid_range");
    assert_eq!(l, before);
    // Extreme sizes neither overflow nor wrap.
    assert_eq!(l.admit(u64::MAX, SeqRange { first: 0, last: u64::MAX }).unwrap(), Admit::Accepted);
    assert_eq!(l.outstanding(), Credit { bytes: 0, batches: u32::MAX - 1 });
    assert!(matches!(l.admit(1, SeqRange { first: 0, last: 0 }).unwrap(), Admit::Refused(_)));
    assert_eq!(l.ack(u64::MAX), Credit { bytes: u64::MAX, batches: 1 });
    assert_eq!(l.ack(u64::MAX), Credit::ZERO);
    assert_eq!(l.in_flight(), Credit::ZERO);
}
