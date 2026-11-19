//! IO.CAD.WORKER.PROTO AC-5: arbitrary worker output never panics, aborts, hangs or
//! allocates past the caps. Drives the host session and the worker-side request decoder
//! under small IO.REG.LIMITS limits (16 MiB bytes, 4 MiB tracked allocation, IO.CORP.FUZZ
//! REQ-004) and a 1 MiB frame cap. Mirrors `fuzz_one` in tests/acceptance.rs.
#![no_main]

use fid_cad_proto::{FrameReader, HostSession, ProtoLimits, ProtoVersion};
use fid_io_reg::{Budget, LimitRequest, Principal};
use libfuzzer_sys::fuzz_target;

fn budget() -> Option<Budget> {
    Budget::new(Principal::Script, &LimitRequest::default().bytes(16 << 20).alloc(4 << 20)).ok()
}

fuzz_target!(|data: &[u8]| {
    let limits = ProtoLimits::DEFAULT.frame_max(1 << 20).records_max(1 << 16);
    if let Some(b) = budget() {
        let mut s = HostSession::new(data, b, limits, ProtoVersion::CURRENT);
        while let Ok(Some(_)) = s.next() {}
        assert!(s.reader().budget().alloc_used() <= 4 << 20);
    }
    if let Some(b) = budget() {
        let mut r = FrameReader::new(data, b, limits);
        while let Ok(Some(_)) = r.next_request() {}
        assert!(r.budget().alloc_used() <= 4 << 20);
    }
});
