//! `fid-kernel-wasm`: the kernel's WASM reference command (spec node KRN.MAN.WASM).
//! Reads one canonical call on stdin (at most `MAX_CALL_BYTES`), answers it with
//! `shim::respond` and writes the answer to stdout; see `shim` for the exit codes.

use std::io::{Read, Write};
use std::process::ExitCode;

use fid_kernel_wasm::call::MAX_CALL_BYTES;
use fid_kernel_wasm::shim::{read_failed, reference_table, respond, EXIT_IO};

fn main() -> ExitCode {
    let mut input = Vec::new();
    // One byte past the limit is read so an oversized call is seen as oversized.
    let limit = MAX_CALL_BYTES as u64 + 1;
    let (code, out) = match std::io::stdin().lock().take(limit).read_to_end(&mut input) {
        Ok(_) => respond(reference_table().as_ref(), &input),
        Err(_) => read_failed(),
    };
    let mut stdout = std::io::stdout().lock();
    if stdout.write_all(&out).and_then(|()| stdout.flush()).is_err() {
        return ExitCode::from(EXIT_IO);
    }
    ExitCode::from(code)
}
