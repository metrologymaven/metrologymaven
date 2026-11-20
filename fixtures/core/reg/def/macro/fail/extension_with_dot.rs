// expect: is not a valid extension
// CORE.REG.DEF.SCHEMA REQ-010: an extension is lowercase, without the dot.
use fid_registry::FidSchema;

#[derive(FidSchema)]
pub struct ImportParams {
    #[fid(path_like, extensions = [".STL"])]
    pub source: String,
}

fn main() {}
