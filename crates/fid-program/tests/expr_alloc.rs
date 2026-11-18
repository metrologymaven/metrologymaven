//! AUTO.PROG.EXPR.GRAMMAR AC-5 (memory half): no allocation above 64 MiB. Worst-case
//! sources the bounds admit (and some just beyond them) are parsed and printed while a
//! counting global allocator records the peak. Own test binary, because it installs a
//! global allocator; one test function, so nothing else allocates concurrently. The
//! crash half of AC-5 is the cargo-fuzz target `expr_parse` (crates/fid-program/fuzz).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use cap::Cap;
use fid_program::expr::syntax::{MAX_SOURCE_BYTES, MAX_TOKENS};
use fid_program::{parse_expr, print_expr};
use std::alloc::System;

#[global_allocator]
static ALLOC: Cap<System> = Cap::new(System, usize::MAX);

const LIMIT: usize = 64 * 1024 * 1024;

#[test]
fn worst_case_sources_stay_under_64_mib() {
    let fill = |unit: &str, head: &str, tail: &str, max_items: usize| {
        let n = ((MAX_SOURCE_BYTES - head.len() - tail.len()) / (unit.len() + 1)).min(max_items);
        format!("{head}{}{tail}", vec![unit; n].join(","))
    };
    let name = "n".repeat(64);
    let cases: Vec<(&str, String)> = vec![
        ("long string", format!("\"{}\"", "a".repeat(MAX_SOURCE_BYTES - 2))),
        ("escaped string", format!("\"{}\"", "\\n".repeat((MAX_SOURCE_BYTES - 2) / 2))),
        ("unicode escapes", format!("\"{}\"", "\\u{1F600}".repeat((MAX_SOURCE_BYTES - 2) / 9))),
        ("most tokens", fill("a", "f(", ")", (MAX_TOKENS - 2) / 2)),
        ("most quantities", fill("1[mm]", "f(", ")", (MAX_TOKENS - 2) / 2)),
        ("long names", fill(&name, "f(", ")", usize::MAX)),
        ("deepest parens", format!("{}x{}", "(".repeat(64), ")".repeat(64))),
        ("longest chain", vec!["a"; 512].join("+")),
        ("nested calls", format!("{}0{}", "f(".repeat(64), ")".repeat(64))),
        ("too many tokens", fill("a", "f(", ")", MAX_TOKENS)),
        ("too deep", "(".repeat(MAX_SOURCE_BYTES / 4)),
        ("too long", "a".repeat(MAX_SOURCE_BYTES + 1)),
    ];
    let mut report = Vec::new();
    let mut worst = 0usize;
    for (label, src) in cases {
        let base = ALLOC.allocated();
        if let Ok(e) = parse_expr(&src) {
            let printed = print_expr(&e);
            let back = parse_expr(&printed).unwrap();
            assert!(e.same_tree(&back), "{label}");
        }
        let peak = ALLOC.max_allocated() - base;
        worst = worst.max(peak);
        report.push(format!("{label}: {} bytes, peak {} KiB", src.len(), peak >> 10));
    }
    let report = report.join("\n");
    eprintln!("{report}");
    assert!(worst < LIMIT, "a case exceeded 64 MiB:\n{report}");
}
