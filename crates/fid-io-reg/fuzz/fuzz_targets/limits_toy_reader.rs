//! IO.REG.LIMITS AC-5: the toy reader on arbitrary bytes never panics, aborts or hangs.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(budget) = fid_io_reg::toy::fuzz_budget() {
        let _ = fid_io_reg::toy::read_slice(data, budget);
    }
});
