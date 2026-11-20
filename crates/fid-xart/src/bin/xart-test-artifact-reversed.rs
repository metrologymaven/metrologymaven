//! Test-only native artifact whose echo concatenates inputs in reverse order: the deliberately
//! divergent native artifact of KRN.MAN.XART AC-2.

fn main() -> std::process::ExitCode {
    fid_xart::testart::main_with(fid_xart::testart::Order::Reversed)
}
