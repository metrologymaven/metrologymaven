//! Test-only native artifact whose echo concatenates inputs in reverse order: the deliberately
//! divergent native artifact of KRN.MAN.XART AC-2.

#[allow(dead_code)] // each binary uses one `Order`
#[path = "../testart.rs"]
mod testart;

fn main() -> std::process::ExitCode {
    testart::main_with(testart::Order::Reversed)
}
