//! AUTO.PROG.FMT AC-6 / REQ-006: peak heap stays under 256 MiB on the worst-case
//! documents the bounds admit. Own test binary, because it installs a counting global
//! allocator; one test function, so nothing else allocates concurrently.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use cap::Cap;
use fid_program::{Bound, Program, ProgramError, MAX_OBJECTS, MAX_TOKENS};
use std::alloc::System;

#[global_allocator]
static ALLOC: Cap<System> = Cap::new(System, usize::MAX);

const LIMIT: usize = 256 * 1024 * 1024;

fn doc(main: &str, meta: &str) -> String {
    let mut s = String::from("{\"format\":\"fid-program\",\"format_version\":1,\"main\":[");
    s.push_str(main);
    s.push_str("],\"meta\":{");
    s.push_str(meta);
    s.push_str(
        "},\"params\":[],\"program_id\":\"018f0000-0000-7000-8000-000000000000\",\"subroutines\":[],\"variables\":[]}",
    );
    s
}

/// `n` copies of `item`, comma-separated, built without an intermediate vector.
fn repeat(item: &str, n: usize) -> String {
    let mut s = String::with_capacity((item.len() + 1) * n);
    for i in 0..n {
        if i > 0 {
            s.push(',');
        }
        s.push_str(item);
    }
    s
}

#[test]
fn worst_case_documents_stay_under_256_mib() {
    // 1. Nearly MAX_TOKENS numbers where steps belong: refused as too many steps
    //    before anything is reserved for them.
    let zeros = doc(&repeat("0", MAX_TOKENS - 100), "");
    // 2. Nearly MAX_OBJECTS one-member objects in meta: valid, and the costliest tree.
    let objects = doc("", &format!("\"a\":[{}]", repeat("{\"\":0}", MAX_OBJECTS - 100)));
    // 3. Nearly MAX_TOKENS numbers inside one invoke literal: valid.
    let literal = doc(
        &format!(
            "{{\"args\":{{\"v\":{{\"literal\":[{}]}}}},\"command\":\"a.b\",\"kind\":\"invoke\",\"major\":1,\
             \"step_id\":\"018f0000-0000-7000-8000-000000000001\"}}",
            repeat("0", MAX_TOKENS - 100)
        ),
        "",
    );

    let base = ALLOC.allocated();
    ALLOC.set_limit(base + LIMIT).unwrap();

    assert_eq!(Program::parse(zeros.as_bytes()), Err(ProgramError::ProgramTooLarge(Bound::Steps)));
    for (name, bytes) in [("objects", objects.as_bytes()), ("literal", literal.as_bytes())] {
        // What the fuzz target does: parse, then store again.
        let p = Program::parse(bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(p.to_canonical().unwrap() == bytes, "{name}");
        p.digest().unwrap();
    }

    // max_allocated also covers building the inputs above, so this is conservative.
    let peak = ALLOC.max_allocated() - base;
    eprintln!("peak heap above inputs: {} MiB", peak / (1024 * 1024));
    assert!(peak < LIMIT, "peak {peak} bytes");
}
