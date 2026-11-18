//! AUTO.PROG.FMT AC-6: hostile bytes never crash the parser, and whatever parses
//! serialises back to the same bytes.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(p) = fid_program::Program::parse(data) {
        assert_eq!(p.to_canonical().ok().as_deref(), Some(data));
    }
});
