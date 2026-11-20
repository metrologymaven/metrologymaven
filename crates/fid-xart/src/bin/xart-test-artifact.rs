//! Test-only native artifact: the stand-in test algorithms, faithful (KRN.MAN.XART AC-2).

#[allow(dead_code)] // each binary uses one `Order`
#[path = "../testart.rs"]
mod testart;

fn main() -> std::process::ExitCode {
    testart::main_with(testart::Order::Faithful)
}
