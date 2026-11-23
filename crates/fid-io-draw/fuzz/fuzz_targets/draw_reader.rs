#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    fid_io_draw::fuzz_read_all(data);
});
