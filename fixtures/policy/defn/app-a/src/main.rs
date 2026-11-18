//! fid-app-a declaring its own command: not allowed (POLICY-DEFN).

use fid_registry_macros::fid_command;

// A comment that mentions #[fid_command] does not count.
const NOTE: &str = "#[fid_command] in a string does not count";

#[fid_command(id = "a.local@1")]
fn local() {}

fn main() {
    let _ = NOTE;
    local();
}

#[ fid_command(id = "a.spaced@1") ]
fn spaced() {}
