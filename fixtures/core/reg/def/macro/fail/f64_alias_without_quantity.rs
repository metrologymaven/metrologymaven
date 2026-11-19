// expect: `f64` does not implement `FidSchema`
// REQ-CORE.REG.DEF.MACRO-002 through an alias: the text check cannot see `f64` here, but f64
// has no FidSchema, so a number field without a quantity still does not compile.
use fid_registry::FidSchema;

pub type Mm = f64;

#[derive(FidSchema)]
pub struct HoleParams {
    pub diameter: Mm,
}

fn main() {}
