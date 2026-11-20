// expect: `extensions` must be distinct and in sorted order
// CORE.REG.DEF.SCHEMA REQ-010: x-extensions is sorted and unique.
use fid_registry::FidSchema;

#[derive(FidSchema)]
pub struct ImportParams {
    #[fid(path_like, extensions = ["stl", "step"])]
    pub source: String,
}

fn main() {}
