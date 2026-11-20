// AC-12 of GDT.CHAR.INTERP: one of a pair of doctests on `fid_gdt::chars::record`. In
// `record_interp.rs` the evaluating code tries to finish and serialise a record from a
// builder that never received its interpretation block and must not build; in
// `record_interp_control.rs` the builder holds the block and it builds. A test asserts the
// two files differ only in the builder's type state.
#![allow(unused_imports)]
use fid_gdt::chars::record::{Interpreted, RecordError, ResultRecordBuilder, Uninterpreted};

fn serialise(b: ResultRecordBuilder<Interpreted>) -> Result<Vec<u8>, RecordError> {
    b.finish().map(|r| r.canonical_json().to_vec())
}

fn main() {
    let _ = serialise;
}
