// expect: `u32` cannot be a path_like parameter
// CORE.REG.DEF.SCHEMA REQ-010: a path_like field is a String or an Option<String>.
use fid_registry::FidSchema;

#[derive(FidSchema)]
pub struct ImportParams {
    #[fid(path_like, extensions = ["stl"])]
    pub source: u32,
}

fn main() {}
