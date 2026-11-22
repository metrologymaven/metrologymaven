//! Generated cargo-fuzz target for registered reader `mesh.ply` (spec node IO.CORP.FUZZ).
//! Drives the reader through `fid-io-reg` with small fuzz limits (16 MiB bytes, depth 64).
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let readers = fid_io_corp::builtin_readers();
    if let Some(reader) = readers.get("mesh.ply") {
        let _ = fid_io_corp::drive_reader(reader.as_ref(), data);
    }
});
