// expect: f64 field `diameter` has no quantity
// REQ-CORE.REG.DEF.MACRO-002: an f64 field needs a quantity.
use fid_registry::FidSchema;

#[derive(FidSchema)]
pub struct HoleParams {
    pub label: String,
    pub diameter: f64,
}

fn main() {}
