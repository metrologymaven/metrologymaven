//! AUTO.PROG.FMT AC-6 / REQ-006: peak heap stays under 200 MiB (requested bytes, so
//! below the 256 MiB fuzz limit with room for allocator overhead) on worst-case documents
//! the bounds admit. Each document is scaled to just under `MAX_COST` (or `MAX_BYTES`, or
//! `MAX_STEPS`), then parsed and stored again, as the fuzz target does. Own test binary,
//! because it installs a counting global allocator; one test function, so nothing else
//! allocates concurrently.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use cap::Cap;
use fid_program::{parse_cost, Bound, Program, ProgramError, MAX_BYTES, MAX_COST, MAX_STEPS};
use std::alloc::System;

#[global_allocator]
static ALLOC: Cap<System> = Cap::new(System, usize::MAX);

const TARGET: usize = 200 * 1024 * 1024;

const HEAD: &str = "{\"format\":\"fid-program\",\"format_version\":1,\"main\":[";
const MID: &str = "],\"meta\":{";
const TAIL: &str =
    "},\"params\":[],\"program_id\":\"018f0000-0000-7000-8000-000000000000\",\"subroutines\":[],\"variables\":[]}";

/// Where the `n` repetitions of a unit go.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    /// `meta = {"a": [unit, unit, ...]}`
    MetaArray,
    /// `meta = {unit, unit, ...}`; the unit is `"key":value` and `{i}` takes the index.
    MetaKeys,
    /// `main = [unit, unit, ...]`; `{i}` takes the index (for step IDs).
    Main,
}

fn build(place: Place, unit: &str, n: usize) -> String {
    let mut s = String::with_capacity(HEAD.len() + MID.len() + TAIL.len() + 16 + n * (unit.len() + 8));
    s.push_str(HEAD);
    if place == Place::Main {
        for i in 0..n {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&unit.replace("{i}", &format!("{i:012x}")));
        }
    }
    s.push_str(MID);
    if place == Place::MetaArray {
        s.push_str("\"a\":[");
    }
    if place != Place::Main {
        for i in 0..n {
            if i > 0 {
                s.push(',');
            }
            // Zero-padded keys keep canonical (sorted) order.
            s.push_str(&unit.replace("{i}", &format!("{i:08}")));
        }
    }
    if place == Place::MetaArray {
        s.push(']');
    }
    s.push_str(TAIL);
    s
}

/// The largest document of this shape within `MAX_COST`, `MAX_BYTES` and `cap` units.
fn scaled(place: Place, unit: &str, cap: usize) -> String {
    let (d1, d2) = (build(place, unit, 10), build(place, unit, 20));
    let (c1, c2) = (parse_cost(d1.as_bytes()), parse_cost(d2.as_bytes()));
    let per_cost = (c2 - c1) / 10 + 1;
    let per_len = (d2.len() - d1.len()) / 10 + 1;
    let by_cost = (MAX_COST - c1) / per_cost + 10;
    let by_len = (MAX_BYTES - d1.len()) / per_len + 10;
    let mut n = by_cost.min(by_len).min(cap);
    loop {
        let doc = build(place, unit, n);
        if parse_cost(doc.as_bytes()) <= MAX_COST && doc.len() <= MAX_BYTES {
            return doc;
        }
        n -= n / 500 + 1;
    }
}

#[test]
fn worst_case_documents_stay_under_200_mib() {
    let invoke = "{\"args\":{\"a\":{\"literal\":0},\"b\":{\"expr\":\"x\"}},\"bind\":\"r\",\"command\":\"a.b\",\
                  \"kind\":\"invoke\",\"major\":1,\"step_id\":\"018f0000-0000-7000-8000-{i}\"}";
    let ret = "{\"kind\":\"return\",\"step_id\":\"018f0000-0000-7000-8000-{i}\"}";
    let deep = format!("{}0{}", "[".repeat(60), "]".repeat(60));
    let long = format!("\"{}\"", "x".repeat(1 << 16));
    let all = usize::MAX;
    let cases: Vec<(&str, Place, &str, usize)> = vec![
        ("objects {\"\":0}", Place::MetaArray, "{\"\":0}", all),
        ("objects {\"\":[0]}", Place::MetaArray, "{\"\":[0]}", all),
        ("empty objects", Place::MetaArray, "{}", all),
        ("numbers", Place::MetaArray, "0", all),
        ("arrays [0]", Place::MetaArray, "[0]", all),
        ("empty arrays", Place::MetaArray, "[]", all),
        ("short strings", Place::MetaArray, "\"abcdefgh\"", all),
        ("empty strings", Place::MetaArray, "\"\"", all),
        ("escaped strings", Place::MetaArray, "\"\\u0001\\n\"", all),
        ("long strings", Place::MetaArray, &long, all),
        ("deep chains", Place::MetaArray, &deep, all),
        ("mix A: 1 object to 5 numbers", Place::MetaArray, "{\"\":0},0,0,0,0,0", all),
        ("mix B: {\"\":[0]} and 2x [0]", Place::MetaArray, "{\"\":[0]},[0],[0]", all),
        ("mix C: objects, arrays, strings", Place::MetaArray, "{\"\":[\"a\"]},[{}],\"b\",0", all),
        ("one object, many keys", Place::MetaKeys, "\"k{i}\":0", all),
        ("one object, many object values", Place::MetaKeys, "\"k{i}\":{\"\":0}", all),
        ("invoke steps", Place::Main, invoke, MAX_STEPS),
        ("return steps", Place::Main, ret, MAX_STEPS),
    ];
    let mut report = Vec::new();
    let mut worst = 0usize;
    for (name, place, unit, cap) in cases {
        let doc = scaled(place, unit, cap);
        let base = ALLOC.allocated();
        let p = Program::parse(doc.as_bytes()).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(p.to_canonical().unwrap() == doc.as_bytes(), "{name}");
        p.digest().unwrap();
        drop(p);
        // Peak above the input. Conservative: if an earlier case set the maximum, this
        // reports at least that much.
        let peak = ALLOC.max_allocated() - base;
        worst = worst.max(peak);
        let cost = parse_cost(doc.as_bytes());
        report.push(format!("{name}: {} bytes, cost {} MiB, peak {} MiB", doc.len(), cost >> 20, peak >> 20));
    }

    // Numbers where steps belong are refused before anything is reserved for them.
    let doc = scaled(Place::Main, "0", all);
    let base = ALLOC.allocated();
    assert_eq!(Program::parse(doc.as_bytes()), Err(ProgramError::ProgramTooLarge(Bound::Steps)));
    report.push(format!("numbers in main (refused): peak {} MiB", (ALLOC.max_allocated() - base) >> 20));

    let report = report.join("\n");
    eprintln!("{report}");
    assert!(worst < TARGET, "a case exceeded 200 MiB:\n{report}");
}
