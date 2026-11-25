//! IO.CAD.IGES.SECT AC-6: the parser on arbitrary bytes never panics, and every accepted
//! input survives parse -> write -> parse unchanged.
#![no_main]

use fid_io_iges::{parse_opts, write, IgesUnit, ParseOptions};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let opts = ParseOptions { unit: Some(IgesUnit::Millimetre), ..ParseOptions::default() };
    if let Ok(file) = parse_opts(data, &opts) {
        match write(&file) {
            Ok(bytes) => match parse_opts(&bytes, &opts) {
                Ok(again) => assert!(again.content_eq(&file)),
                Err(e) => panic!("written output does not parse: {e}"),
            },
            Err(e) => panic!("accepted file is not writable: {e}"),
        }
    }
});
