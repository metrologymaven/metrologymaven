//! `fid-guide-wasm`: stdin/stdout entry point for both native CLI and `wasm32-wasip1`.

use std::io::{Read, Write};
use std::process::ExitCode;

use fid_guide_wasm::{dispatch, read_failed, EXIT_IO, MAX_REQUEST_BYTES};

fn main() -> ExitCode {
    let mut input = Vec::new();
    let limit = (MAX_REQUEST_BYTES as u64).saturating_add(1);
    let (code, out) = match std::io::stdin().lock().take(limit).read_to_end(&mut input) {
        Ok(_) => dispatch(&input),
        Err(_) => read_failed(),
    };
    let mut stdout = std::io::stdout().lock();
    if stdout.write_all(&out).and_then(|()| stdout.write_all(b"\n")).and_then(|()| stdout.flush()).is_err() {
        return ExitCode::from(EXIT_IO);
    }
    ExitCode::from(code)
}
