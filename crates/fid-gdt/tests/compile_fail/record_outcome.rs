// AC-7 of GDT.CHAR.RECORD: one of a pair of doctests on `fid_gdt::chars::record`. In
// `record_outcome.rs` the evaluating code tries to set a conformance outcome on the builder
// directly and must not build; in `record_outcome_control.rs` it binds the decision through
// `bind_decision` and must build. A test asserts the two files differ only in that line.
use fid_gdt::chars::record::ResultRecordBuilder;

fn bind(b: ResultRecordBuilder) -> ResultRecordBuilder {
    b.outcome(fid_kdec::Outcome::Conforms)
}

fn main() {
    let _ = bind;
}
