//! Shared crate fid-x: declares a registry command; shared crates may.

use fid_registry_macros::fid_command;

#[fid_command(id = "x.do_thing@1")]
pub fn do_thing() -> u32 {
    1
}
