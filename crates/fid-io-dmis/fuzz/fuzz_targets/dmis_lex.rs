//! IO.DMIS.PARSE.LEX AC-5: the lexer on arbitrary bytes never panics, aborts or hangs,
//! with strict options and with every relaxation enabled.
#![no_main]

use fid_io_dmis::lex::{fuzz_budget, lex_slice, LexOptions, Relaxation};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let relaxed = LexOptions::strict().with(Relaxation::InlineComment).with(Relaxation::BareCrLineEnd);
    for options in [LexOptions::strict(), relaxed] {
        if let Ok(budget) = fuzz_budget() {
            let _ = lex_slice(data, budget, &options);
        }
    }
});
