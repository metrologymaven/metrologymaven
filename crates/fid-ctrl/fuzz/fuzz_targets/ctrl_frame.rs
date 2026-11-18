//! AUTO.CTRL.WIRE AC-4: hostile byte streams never crash or hang the framing and schema
//! code, and nothing after an oversized line is read.
#![no_main]

use fid_ctrl::msg::{classify_stream, Verdict, MAX_MESSAGE_BYTES};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    for limit in [MAX_MESSAGE_BYTES, 64] {
        let v = classify_stream(data, limit);
        let end = v.iter().position(|x| matches!(x, Verdict::TooLarge | Verdict::Partial));
        assert!(end.map_or(true, |i| i + 1 == v.len()));
    }
});
