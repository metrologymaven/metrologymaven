//! IO.CAD.IGES.SECT AC-6: the parser on arbitrary bytes never panics, and every accepted
//! input survives parse -> write -> parse unchanged.
#![no_main]

use fid_io_iges::{parse_opts, write, IgesError, IgesUnit, ParseOptions};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // The file's own unit first; a caller unit only when the file declares none.
    let mut opts = ParseOptions::default();
    let parsed = match parse_opts(data, &opts) {
        Err(IgesError::UnitRequired) => {
            opts.unit = Some(IgesUnit::Millimetre);
            parse_opts(data, &opts)
        }
        other => other,
    };
    if let Ok(file) = parsed {
        match write(&file) {
            Ok(bytes) => match parse_opts(&bytes, &opts) {
                Ok(again) => assert!(again.content_eq(&file)),
                Err(e) => panic!("written output does not parse: {e}"),
            },
            Err(e) => panic!("accepted file is not writable: {e}"),
        }
    }
});
