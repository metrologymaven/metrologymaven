//! IO.CLOUD.E57.XML AC-5: the XML section reader on arbitrary bytes never panics, aborts or
//! hangs. Whatever it accepts writes back and parses again to an equal tree, unless escaping
//! takes it past the writer's size bound, in which case the writer refuses with
//! `WriteError::LimitExceeded`.
#![no_main]

use fid_io_e57::xml::{parse_slice, write_tree, WriteError};
use fid_io_reg::{Budget, LimitRequest, Principal};
use libfuzzer_sys::fuzz_target;

fn budget() -> Option<Budget> {
    let request = LimitRequest::default().bytes(1 << 20).elements(65_536).alloc(64 << 20);
    Budget::new(Principal::Script, &request).ok()
}

fuzz_target!(|data: &[u8]| {
    let Some(b) = budget() else { return };
    if let Ok(x) = parse_slice(data, b) {
        match write_tree(x.tree()) {
            Ok(bytes) => {
                if let Some(b) = budget() {
                    let back = parse_slice(&bytes, b).ok();
                    assert!(back.as_ref().map(|y| y.tree()) == Some(x.tree()), "write is not a round trip");
                }
            }
            Err(WriteError::LimitExceeded { .. }) => {}
            Err(e) => panic!("an accepted tree must write: {e}"),
        }
    }
});
