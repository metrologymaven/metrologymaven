// expect: cannot apply unary operator `-` to type `u32`
// A default is checked against the field type: a negative default on an unsigned field.
use fid_registry::FidSchema;

#[derive(FidSchema)]
pub struct Params {
    #[fid(default = -1)]
    pub count: u32,
}

fn main() {}
