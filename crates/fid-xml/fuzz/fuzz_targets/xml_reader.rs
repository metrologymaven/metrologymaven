//! IO.QIF.XML.PARSE AC-5: on arbitrary bytes the reader returns events or a typed error
//! with a position, and never panics, aborts or hangs. The first input byte picks the
//! read chunk size, so tokens are also assembled across buffer refills.
#![no_main]

use fid_xml::{fuzz_budget, read_slice_with, XmlOptions};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&first, rest)) = data.split_first() else {
        return;
    };
    let options = XmlOptions::default().with_read_chunk(usize::from(first) + 1);
    if let Ok(budget) = fuzz_budget() {
        if let Err(e) = read_slice_with(rest, budget, options) {
            let at = e.position();
            assert!(at.is_some_and(|p| p.line.is_some() && p.col.is_some()));
        }
    }
});
