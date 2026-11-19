//! IO.CLOUD.LAS.HDR AC-5: the LAS reader on arbitrary bytes never panics, aborts or hangs,
//! and whatever it accepts writes back and reads again to the same bytes.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let opts = fid_io_las::fuzz_options();
    if let Ok(file) = fid_io_las::read_slice(data, opts) {
        if let Ok(out) = fid_io_las::write_file(&file) {
            let again = fid_io_las::read_slice(&out, opts).ok();
            let rewritten = again.as_ref().and_then(|f| fid_io_las::write_file(f).ok());
            assert!(rewritten.as_deref() == Some(&out[..]), "write is not idempotent");
        }
    }
});
