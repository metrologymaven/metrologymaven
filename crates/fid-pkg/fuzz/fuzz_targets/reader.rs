//! TRUST.PKG.READER AC-4: on arbitrary bytes the package reader returns a package or a typed
//! error with one of the six codes, and never panics, aborts, hangs or runs out of memory.
//! Every path a verifier takes is exercised: open, per-member verification, bounded member
//! reads, `statement.json` and signature checks, and (for small inputs) the inspect command.
//! Each member is decoded at most twice, so the time per input follows the reader's own bound
//! (at most 1000 bytes of output per input byte) rather than a multiple of it.
#![no_main]

use fid_pkg::reader::{inspect, PackageReader};
use libfuzzer_sys::fuzz_target;
use std::io::Cursor;

const CODES: [&str; 6] =
    ["pkg.digest_mismatch", "pkg.io", "pkg.limit", "pkg.malformed", "pkg.unlisted_member", "pkg.unsupported_version"];

fn check<T>(r: Result<T, fid_pkg::reader::ReadError>) {
    if let Err(e) = r {
        assert!(CODES.contains(&e.code()));
    }
}

fuzz_target!(|data: &[u8]| {
    match PackageReader::open(Cursor::new(data)) {
        Ok(mut r) => {
            for result in r.member_results() {
                check(result);
            }
            // Bounded reads: a member above 64 KiB is refused (buffer_cap) before decoding.
            let names: Vec<String> = r.entry_names().into_iter().map(str::to_string).collect();
            for name in names.iter().take(16) {
                check(r.read_member(name, 64 * 1024));
            }
            for name in names.iter().filter(|n| *n == "statement.json" || n.starts_with("signatures/")) {
                check(r.verify_member(name));
            }
        }
        Err(e) => assert!(CODES.contains(&e.code())),
    }
    if data.len() <= 16 * 1024 {
        check(inspect(Cursor::new(data)));
    }
});
