//! AUTO.PROG.EXPR.GRAMMAR AC-5: hostile bytes never crash the expression parser, and
//! whatever parses prints to a source no longer than the input that parses back to the
//! same tree (REQ-006). Run for 10 minutes under a 64 MiB allocation limit:
//!   cargo +nightly fuzz run expr_parse ../../../fixtures/program/expr/valid ../../../fixtures/program/expr/invalid \
//!     -- -max_total_time=600 -rss_limit_mb=256 -malloc_limit_mb=64
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(src) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(e) = fid_program::parse_expr(src) {
        let printed = fid_program::print_expr(&e);
        assert!(printed.len() <= src.len());
        let back = fid_program::parse_expr(&printed);
        assert!(back.is_ok_and(|b| b.same_tree(&e)));
    }
});
