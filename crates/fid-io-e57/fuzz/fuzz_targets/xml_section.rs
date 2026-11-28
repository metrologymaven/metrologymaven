//! IO.CLOUD.E57.XML AC-5: the XML section reader on arbitrary bytes never panics, aborts or
//! hangs, and whatever it accepts writes back and parses again to an equal tree.
#![no_main]

use fid_io_e57::xml::{parse_slice, write_tree};
use fid_io_reg::{Budget, LimitRequest, Principal};
use libfuzzer_sys::fuzz_target;

fn budget() -> Option<Budget> {
    let request = LimitRequest::default().bytes(1 << 20).elements(65_536).alloc(64 << 20);
    Budget::new(Principal::Script, &request).ok()
}

fuzz_target!(|data: &[u8]| {
    let Some(b) = budget() else { return };
    if let Ok(x) = parse_slice(data, b) {
        let bytes = write_tree(x.tree());
        assert!(bytes.is_ok(), "an accepted tree must write");
        if let (Ok(bytes), Some(b)) = (bytes, budget()) {
            let back = parse_slice(&bytes, b).ok();
            assert!(back.as_ref().map(|y| y.tree()) == Some(x.tree()), "write is not a round trip");
        }
    }
});
