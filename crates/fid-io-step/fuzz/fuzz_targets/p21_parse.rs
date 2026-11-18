//! IO.CAD.STEP.P21 AC-6: the parser on arbitrary bytes never panics, and every accepted
//! input survives parse -> print -> parse unchanged.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(file) = fid_io_step::parse(data) {
        if let Ok(text) = fid_io_step::print(&file) {
            match fid_io_step::parse(text.as_bytes()) {
                Ok(again) => assert!(again.content_eq(&file, true)),
                Err(e) => panic!("printed output does not parse: {e}"),
            }
        }
    }
});
