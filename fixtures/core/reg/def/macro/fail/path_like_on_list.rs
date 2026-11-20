// expect: a field with #[fid(path_like)] must be a String or an Option<String>
// CORE.REG.DEF.SCHEMA REQ-010: path_like is not allowed on an array (CORE.BUS.POL.FS refuses nested file parameters).
use fid_registry::FidSchema;

#[derive(FidSchema)]
pub struct ImportParams {
    #[fid(path_like, extensions = ["stl"])]
    pub sources: Vec<String>,
}

fn main() {}
