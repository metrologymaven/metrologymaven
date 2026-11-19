//! TRUST.PKG.READER AC-4: on arbitrary bytes the package reader returns a package or a typed
//! error with one of the six codes, and never panics, aborts, hangs or runs out of memory.
//! Every path a verifier takes is exercised: open, per-member verification, the strict
//! `verify_all`, bounded member reads and the inspect command.
#![no_main]

use fid_pkg::reader::{inspect, PackageReader};
use libfuzzer_sys::fuzz_target;
use std::io::Cursor;

const CODES: [&str; 6] =
    ["pkg.digest_mismatch", "pkg.io", "pkg.limit", "pkg.malformed", "pkg.unlisted_member", "pkg.unsupported_version"];

fuzz_target!(|data: &[u8]| {
    match PackageReader::open(Cursor::new(data)) {
        Ok(mut r) => {
            for result in r.member_results() {
                if let Err(e) = result {
                    assert!(CODES.contains(&e.code()));
                }
            }
            let names: Vec<String> = r.entry_names().into_iter().map(str::to_string).collect();
            for name in names.iter().take(16) {
                if let Err(e) = r.read_member(name, 1 << 20) {
                    assert!(CODES.contains(&e.code()));
                }
            }
            if let Err(e) = r.verify_all() {
                assert!(CODES.contains(&e.code()));
            }
        }
        Err(e) => assert!(CODES.contains(&e.code())),
    }
    let _ = inspect(Cursor::new(data));
});
