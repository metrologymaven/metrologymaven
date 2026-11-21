//! IO.CAD.STEP.HDR: header and unit resolution on arbitrary bytes never panics, and every
//! refusal is one of the typed codes.
#![no_main]

use fid_io_step::{resolve_header, HeaderOptions};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(file) = fid_io_step::parse(data) {
        let opts = HeaderOptions::default();
        match resolve_header(&file, &opts) {
            Ok(info) => {
                let _ = info.unit_groups();
                let _ = info.provenance_unit(&opts);
            }
            Err(e) => {
                let _ = e.to_string();
            }
        }
    }
});
