// expect: this `default` literal does not fit a field of type `bool`
// A default is checked against the field type: a string literal on a bool field.
use fid_registry::FidSchema;

#[derive(FidSchema)]
pub struct Params {
    #[fid(default = "yes")]
    pub full: bool,
}

fn main() {}
