// expect: `extensions` needs `path_like`
// CORE.REG.DEF.SCHEMA REQ-010: x-extensions only next to path_like.
use fid_registry::FidSchema;

#[derive(FidSchema)]
pub struct ImportParams {
    #[fid(extensions = ["stl"])]
    pub source: String,
}

fn main() {}
