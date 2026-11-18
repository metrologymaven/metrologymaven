//! fid-app-a: the entry point of app `a`. Policy lives in the manifest, not here.

use fid_x::do_thing;

/// Composition only: every behaviour comes from shared crates.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        println!("usage: fid-app-a");
        return;
    }
    let n = do_thing();
    println!("{n}");
}

// The window, panels and defaults come from the app manifest
// (ECO.APPS.MANIFEST), loaded by the shared shell.
//
// Nothing else belongs in this file: no measurement, fitting,
// evaluation or file-format logic.
